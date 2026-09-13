//! `<progress>` and `<meter>` — HTML §4.10.13 and §4.10.14.
//!
//! Both draw a track with a filled portion, and both are elements CSS alone
//! cannot render: the fill is a FRACTION of two attributes, and no declaration
//! expresses "six tenths of this box". Without a painter they fell through to
//! the generic text arm and rendered their VALUE as a string — a gauge reading
//! `0.6` where a bar belongs.
//!
//! `<meter>` is not a second progress bar. Progress is "how much of a task is
//! done", so it has one colour; a meter is "where in a range this measurement
//! sits", so it is coloured by which range the value falls into — green in the
//! optimum band, yellow in the suboptimal one, red when it is out of bounds.
//! That colouring is the whole difference between the two elements, and
//! spelling them as one control would lose it.

use tiny_skia::{FillRule, Paint, Pixmap, Stroke, Transform};

use super::{WidgetColors, rounded_rect_path};

/// Which band a `<meter>`'s value falls into, which is what colours it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// `<progress>`, and a meter in its optimum range.
    Optimum,
    /// Between `low`/`high` and the optimum end.
    Suboptimal,
    /// Below `low` or above `high` when that is the wrong side of `optimum`.
    Bad,
}

/// A track with a filled portion — `<progress>` and `<meter>`.
pub struct Gauge {
    /// `value / max`, already resolved. Clamped when drawn, not when set: a
    /// fraction is a question about the RANGE, and attributes arrive in
    /// whatever order the markup wrote them.
    pub fraction: f32,
    /// `<progress>` with no `value` is INDETERMINATE — the task is running and
    /// its extent is unknown, which HTML distinguishes from `value="0"`. A
    /// browser animates a sliver; a still frame draws a track and a marker so
    /// it cannot be mistaken for "nothing done".
    pub indeterminate: bool,
    pub band: Band,
    pub width: f32,
    pub height: f32,
    pub colors: WidgetColors,
}

impl Gauge {
    pub fn new(fraction: f32) -> Self {
        Self {
            fraction,
            indeterminate: false,
            band: Band::Optimum,
            width: 160.0,
            height: 16.0,
            colors: WidgetColors {
                background: (240, 240, 240, 255),
                border: (171, 171, 171, 255),
                accent: (25, 118, 210, 255),
                ..WidgetColors::default()
            },
        }
    }

    /// The fill colour for the band. `<progress>` is always `Optimum` and so
    /// always takes the accent.
    fn fill_rgba(&self) -> (u8, u8, u8, u8) {
        match self.band {
            Band::Optimum => self.colors.accent,
            Band::Suboptimal => (230, 180, 20, 255),
            Band::Bad => (200, 60, 50, 255),
        }
    }

    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        if self.width <= 0.0 || self.height <= 0.0 {
            return;
        }
        let ts = Transform::from_scale(scale, scale);
        let mut paint = Paint::default();
        paint.anti_alias = true;
        // A pill, as browsers draw both elements: the radius is half the bar
        // rather than a fixed 4px, so a tall bar does not come out a rectangle
        // with nicked corners.
        let radius = (self.height / 2.0).min(self.width / 2.0);

        let (r, g, b, a) = self.colors.background;
        paint.set_color_rgba8(r, g, b, a);
        if let Some(path) = rounded_rect_path(x, y, self.width, self.height, radius) {
            pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
        }

        let (fr, fg, fb, fa) = self.fill_rgba();
        paint.set_color_rgba8(fr, fg, fb, fa);
        if self.indeterminate {
            // A third of the track, a third of the way along — the still-frame
            // stand-in for the animation, and unmistakably not a value.
            let w = self.width / 3.0;
            if let Some(path) = rounded_rect_path(x + w, y, w, self.height, radius) {
                pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
            }
        } else {
            let fill_w = self.fraction.clamp(0.0, 1.0) * self.width;
            if fill_w > 0.0 {
                if let Some(path) = rounded_rect_path(x, y, fill_w, self.height, radius) {
                    pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
                }
            }
        }

        let (br, bg, bb, ba) = self.colors.border;
        paint.set_color_rgba8(br, bg, bb, ba);
        let mut stroke = Stroke::default();
        stroke.width = 1.0;
        if let Some(path) = rounded_rect_path(x, y, self.width, self.height, radius) {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }
    }
}

/// Which band a `<meter>`'s value sits in — HTML §4.10.14's own rules.
///
/// `low`/`high` split the range into three, and `optimum` says which of them is
/// the GOOD one. That last part is why a meter cannot be coloured by magnitude:
/// for disk space used, low is good; for battery charge, high is.
pub fn meter_band(value: f32, min: f32, max: f32, low: f32, high: f32, optimum: f32) -> Band {
    if value < low || value > high {
        // Outside the stated band. Whether that is merely suboptimal or bad
        // depends on which side `optimum` is on.
        let optimum_low = optimum < low;
        let optimum_high = optimum > high;
        if (value < low && optimum_high) || (value > high && optimum_low) {
            return Band::Bad;
        }
        return Band::Suboptimal;
    }
    let _ = (min, max);
    Band::Optimum
}
