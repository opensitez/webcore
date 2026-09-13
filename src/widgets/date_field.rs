//! The date and time inputs — HTML §4.10.5.1.7 through §4.10.5.1.12.
//!
//! `date`, `month`, `week`, `time` and `datetime-local` are one control with
//! five value formats. Each is a text field showing a FORMATTED value plus an
//! affordance that opens a picker — and like the colour picker and the file
//! chooser, the picker itself is user-agent chrome that appears on activation
//! and is not what a page lays out.
//!
//! Lifted from `vybe_widgets::datetime` in the same way as the others: the
//! drawing, without the toolkit's widget shell or its own calendar state.
//!
//! Without this these five fell to the plain text arm and rendered as ordinary
//! text boxes — no affordance, and an empty one indistinguishable from a
//! `<input type=text>`.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// Which glyph the field carries, which is decided by the value's FORMAT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `date`, `month`, `week`, `datetime-local` — a calendar.
    Calendar,
    /// `time` — a clock.
    Clock,
}

impl Kind {
    /// The input type's glyph, and the pattern shown when there is no value.
    ///
    /// The pattern is UA-defined; these are the spellings browsers use, and
    /// each one mirrors the value format the spec requires for that type
    /// (`yyyy-mm-dd`, `yyyy-Www`, and so on) in the order a reader expects.
    pub fn for_input(input_type: &str) -> Option<(Kind, &'static str)> {
        Some(match input_type {
            "date" => (Kind::Calendar, "yyyy-mm-dd"),
            "month" => (Kind::Calendar, "yyyy-mm"),
            "week" => (Kind::Calendar, "yyyy-Www"),
            "time" => (Kind::Clock, "--:--"),
            "datetime-local" => (Kind::Calendar, "yyyy-mm-dd --:--"),
            _ => return None,
        })
    }
}

/// The picker affordance at the field's trailing edge.
pub struct DateField {
    pub kind: Kind,
    pub width: f32,
    pub height: f32,
    pub disabled: bool,
}

impl DateField {
    pub fn new(kind: Kind, width: f32, height: f32) -> Self {
        Self {
            kind,
            width,
            height,
            disabled: false,
        }
    }

    /// How much room the glyph takes at the right-hand edge — the text must
    /// stop before it, which is why the caller needs this too.
    pub fn glyph_width(height: f32) -> f32 {
        (height * 0.8).clamp(14.0, 24.0)
    }

    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        if self.width <= 0.0 || self.height <= 4.0 {
            return;
        }
        let ts = Transform::from_scale(scale, scale);
        let mut paint = Paint::default();
        paint.anti_alias = true;
        if self.disabled {
            paint.set_color_rgba8(170, 170, 170, 255);
        } else {
            paint.set_color_rgba8(90, 90, 90, 255);
        }
        let stroke = Stroke {
            width: 1.0,
            ..Stroke::default()
        };

        let gw = Self::glyph_width(self.height);
        let size = (gw * 0.62).min(self.height * 0.62);
        let cx = x + self.width - gw / 2.0;
        let cy = y + self.height / 2.0;

        match self.kind {
            Kind::Calendar => {
                // A page with a bound spine: the outline, the header rule, and
                // the two tabs above it — the shape everything from a desk
                // calendar to a browser's own icon uses.
                let left = cx - size / 2.0;
                let top = cy - size / 2.0;
                let mut pb = PathBuilder::new();
                pb.push_rect(
                    tiny_skia::Rect::from_xywh(left, top, size, size)
                        .unwrap_or(tiny_skia::Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()),
                );
                pb.move_to(left, top + size * 0.32);
                pb.line_to(left + size, top + size * 0.32);
                pb.move_to(left + size * 0.28, top);
                pb.line_to(left + size * 0.28, top - size * 0.16);
                pb.move_to(left + size * 0.72, top);
                pb.line_to(left + size * 0.72, top - size * 0.16);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, ts, None);
                }
            }
            Kind::Clock => {
                // A face and two hands, at ten past ten — the angle every clock
                // in every catalogue is set to, because it leaves the face open.
                let r = size / 2.0;
                let mut pb = PathBuilder::new();
                pb.push_circle(cx, cy, r);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, ts, None);
                }
                let mut hands = PathBuilder::new();
                hands.move_to(cx, cy);
                hands.line_to(cx, cy - r * 0.55);
                hands.move_to(cx, cy);
                hands.line_to(cx + r * 0.45, cy + r * 0.2);
                if let Some(path) = hands.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, ts, None);
                }
            }
        }
        let _ = FillRule::Winding;
    }
}

/// The calendar popup's geometry and its day grid.
///
/// Seven columns, six rows — the most any month needs — plus a header row for
/// the month and one for the weekday initials. Fixed at six so the popup does
/// not change height as you page through months, which is what browsers do and
/// what stops the control jumping under the pointer.
pub struct Calendar;

impl Calendar {
    pub const COLUMNS: usize = 7;
    pub const WEEKS: usize = 6;
    pub const CELL: f32 = 22.0;
    /// The month caption and the weekday initials, above the grid.
    pub const HEADER: f32 = 44.0;

    pub fn width() -> f32 {
        Self::COLUMNS as f32 * Self::CELL
    }

    pub fn height() -> f32 {
        Self::HEADER + Self::WEEKS as f32 * Self::CELL
    }

    /// Which day-of-month a point lands on, given the month's shape.
    ///
    /// `first_weekday` is the column the 1st falls in (0 = Monday), and
    /// `days` is the length of the month. Returns `None` for the leading and
    /// trailing blanks, which are not days of THIS month and must not be
    /// pickable — clicking the gap before the 1st selecting the 1st is the
    /// classic calendar bug.
    pub fn day_at(local: (f32, f32), first_weekday: usize, days: u32) -> Option<u32> {
        if local.1 < Self::HEADER {
            return None;
        }
        let col = (local.0 / Self::CELL) as usize;
        let row = ((local.1 - Self::HEADER) / Self::CELL) as usize;
        if col >= Self::COLUMNS || row >= Self::WEEKS {
            return None;
        }
        let index = row * Self::COLUMNS + col;
        let day = index.checked_sub(first_weekday)? + 1;
        (day as u32 <= days).then_some(day as u32)
    }
}

/// How many days a month has — the proleptic Gregorian rule, leap years and
/// all, because a February that is wrong every fourth year is worse than none.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 30,
    }
}

/// Which weekday the 1st of a month falls on, 0 = Monday.
///
/// Sakamoto's method, which is exact for the proleptic Gregorian calendar and
/// needs no date library — this engine has none and should not grow one to put
/// a month on screen.
pub fn first_weekday(year: i32, month: u32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut y = year;
    if month < 3 {
        y -= 1;
    }
    let m = (month as usize).clamp(1, 12) - 1;
    let sunday_based = (y + y / 4 - y / 100 + y / 400 + T[m] + 1).rem_euclid(7);
    // Sakamoto counts from Sunday; the grid starts on Monday.
    ((sunday_based + 6) % 7) as usize
}

/// `yyyy-mm-dd` — the value format HTML requires of a date input.
pub fn parse_date(value: &str) -> Option<(i32, u32, u32)> {
    let mut parts = value.trim().split('-');
    let y = parts.next()?.parse::<i32>().ok()?;
    let m = parts.next()?.parse::<u32>().ok()?;
    let d = parts.next()?.parse::<u32>().ok()?;
    (1..=12).contains(&m).then_some(())?;
    (1..=31).contains(&d).then_some(())?;
    Some((y, m, d))
}

pub fn to_date_value(year: i32, month: u32, day: u32) -> String {
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::Kind;

    #[test]
    fn each_date_type_has_its_own_glyph_and_pattern() {
        // `time` is the only one of the five that is not a calendar, and the
        // patterns follow the value formats the spec defines per type.
        assert_eq!(Kind::for_input("time").map(|(k, _)| k), Some(Kind::Clock));
        for calendar in ["date", "month", "week", "datetime-local"] {
            assert_eq!(
                Kind::for_input(calendar).map(|(k, _)| k),
                Some(Kind::Calendar),
                "{calendar} should carry a calendar"
            );
        }
        assert_eq!(Kind::for_input("date").map(|(_, p)| p), Some("yyyy-mm-dd"));
        // Not a date input at all — no glyph, and the caller must fall through
        // to the ordinary text field rather than draw one.
        assert!(Kind::for_input("text").is_none());
    }

    #[test]
    fn the_month_grid_is_arithmetic_not_a_guess() {
        use super::{Calendar, days_in_month, first_weekday, parse_date, to_date_value};
        // Known anchors: 2026-08-01 is a Saturday, 2000-02-01 a Tuesday.
        assert_eq!(first_weekday(2026, 8), 5, "Saturday, counting from Monday");
        assert_eq!(first_weekday(2000, 2), 1, "Tuesday");
        // The century rule, which is where naive leap-year code goes wrong.
        assert_eq!(days_in_month(2000, 2), 29, "2000 is a leap year");
        assert_eq!(days_in_month(1900, 2), 28, "1900 is NOT");
        assert_eq!(days_in_month(2026, 2), 28);

        // The blanks before the 1st are not days and must not be pickable.
        let first = first_weekday(2026, 8);
        let days = days_in_month(2026, 8);
        let cell = Calendar::CELL;
        assert_eq!(
            Calendar::day_at((cell * 0.5, Calendar::HEADER + 2.0), first, days),
            None
        );
        assert_eq!(
            Calendar::day_at(
                (cell * (first as f32 + 0.5), Calendar::HEADER + 2.0),
                first,
                days
            ),
            Some(1)
        );
        // Nor are the ones after the month's end.
        assert_eq!(
            Calendar::day_at((cell * 6.5, Calendar::HEADER + cell * 5.5), first, days),
            None
        );

        assert_eq!(parse_date("2026-08-24"), Some((2026, 8, 24)));
        assert_eq!(parse_date("not-a-date"), None);
        assert_eq!(to_date_value(2026, 8, 4), "2026-08-04");
    }
}
