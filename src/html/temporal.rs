//! HTML date/time string grammar and input value sanitization (HTML 2.3.5).

fn year_remainder(year: &str) -> Option<u32> {
    if year.len() < 4
        || !year.bytes().all(|b| b.is_ascii_digit())
        || !year.bytes().any(|b| b != b'0')
    {
        return None;
    }
    Some(year.bytes().fold(0, |remainder, digit| {
        (remainder * 10 + u32::from(digit - b'0')) % 400
    }))
}

fn two_digits(raw: &str) -> Option<u32> {
    let bytes = raw.as_bytes();
    if bytes.len() != 2 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0'))
}

pub(crate) fn month_parts(raw: &str) -> Option<(&str, u32)> {
    let (year, month) = raw.rsplit_once('-')?;
    year_remainder(year)?;
    let month = two_digits(month)?;
    (1..=12).contains(&month).then_some((year, month))
}

pub(crate) fn date_parts(raw: &str) -> Option<(&str, u32, u32)> {
    let (month, day) = raw.rsplit_once('-')?;
    let (year, month) = month_parts(month)?;
    let day = two_digits(day)?;
    let y = year_remainder(year)?;
    let leap = y % 4 == 0 && (y % 100 != 0 || y == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days).contains(&day).then_some((year, month, day))
}

fn valid_week(raw: &str) -> bool {
    let Some((year, week)) = raw.rsplit_once("-W") else {
        return false;
    };
    let Some(y) = year_remainder(year) else {
        return false;
    };
    let Some(week) = two_digits(week) else {
        return false;
    };
    // Gregorian weekdays and leap years repeat every 400 years.
    let year = if y == 0 { 400 } else { y };
    let previous = year - 1;
    let january_first = (previous + previous / 4 - previous / 100 + previous / 400) % 7;
    let leap = y % 4 == 0 && (y % 100 != 0 || y == 0);
    let max = if january_first == 3 || (january_first == 2 && leap) {
        53
    } else {
        52
    };
    (1..=max).contains(&week)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Time {
    pub(crate) hour: u32,
    pub(crate) minute: u32,
    pub(crate) second: u32,
    pub(crate) millisecond: u32,
}

pub(crate) fn time_parts(raw: &str) -> Option<Time> {
    let mut parts = raw.split(':');
    let hour = two_digits(parts.next()?)?;
    let minute = two_digits(parts.next()?)?;
    if hour > 23 || minute > 59 {
        return None;
    }
    let (second, millisecond) = match parts.next() {
        None => (0, 0),
        Some(raw) => {
            let (seconds, fraction) = raw
                .split_once('.')
                .map_or((raw, None), |(s, f)| (s, Some(f)));
            let second = two_digits(seconds)?;
            if second > 59 {
                return None;
            }
            let millisecond = match fraction {
                None => 0,
                Some(f) if (1..=3).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()) => {
                    f.parse::<u32>().ok()? * 10_u32.pow(3 - f.len() as u32)
                }
                _ => return None,
            };
            (second, millisecond)
        }
    };
    if parts.next().is_some() {
        return None;
    }
    Some(Time {
        hour,
        minute,
        second,
        millisecond,
    })
}

impl Time {
    fn milliseconds(self) -> f64 {
        f64::from(((self.hour * 60 + self.minute) * 60 + self.second) * 1000 + self.millisecond)
    }

    pub(crate) fn normalized(self) -> String {
        let mut result = format!("{:02}:{:02}", self.hour, self.minute);
        if self.second != 0 || self.millisecond != 0 {
            result.push_str(&format!(":{:02}", self.second));
            if self.millisecond != 0 {
                result.push('.');
                result.push_str(format!("{:03}", self.millisecond).trim_end_matches('0'));
            }
        }
        result
    }
}

const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;
const MILLISECONDS_PER_WEEK: f64 = 7.0 * MILLISECONDS_PER_DAY;
const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// Gregorian date inverse for the numeric domains used by date and week inputs.
pub(crate) fn date_from_epoch_days(days: f64) -> Option<(i32, u32, u32)> {
    if !days.is_finite() {
        return None;
    }
    let absolute = days.floor() + days_before_year(1970.0);
    if absolute < 0.0 || absolute >= days_before_year(f64::from(i32::MAX)) {
        return None;
    }
    let (mut low, mut high) = (1, i32::MAX);
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if days_before_year(f64::from(middle)) <= absolute {
            low = middle;
        } else {
            high = middle;
        }
    }
    let mut ordinal = (absolute - days_before_year(f64::from(low))) as u32;
    for month in 1..=12 {
        let count = crate::widgets::days_in_month(low, month);
        if ordinal < count {
            return Some((low, month, ordinal + 1));
        }
        ordinal -= count;
    }
    None
}

/// ISO week-year and week containing a Gregorian day, including year boundaries.
pub(crate) fn week_from_date(year: i32, month: u32, day: u32) -> Option<String> {
    let date = format!("{year:04}-{month:02}-{day:02}");
    let days = value_number("date", &date)? / MILLISECONDS_PER_DAY;
    let monday = days - (days + 3.0).rem_euclid(7.0);
    let (week_year, _, _) = date_from_epoch_days(monday + 3.0)?;
    let first = value_number("week", &format!("{week_year:04}-W01"))? / MILLISECONDS_PER_DAY;
    Some(format!(
        "{week_year:04}-W{:02}",
        ((monday - first) / 7.0) as u32 + 1
    ))
}

pub(crate) fn week_date(raw: &str) -> Option<(i32, u32, u32)> {
    // Thursday always lies in the ISO week-year, useful when opening its month.
    date_from_epoch_days(value_number("week", raw)? / MILLISECONDS_PER_DAY + 3.0)
}

pub(crate) fn current_utc_date() -> (i32, u32, u32) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_else(|error| -error.duration().as_secs_f64());
    date_from_epoch_days(now / (MILLISECONDS_PER_DAY / MILLISECONDS_PER_SECOND))
        .expect("system clock is within the supported Gregorian calendar")
}

fn days_before_year(year: f64) -> f64 {
    let previous = year - 1.0;
    previous * 365.0 + (previous / 4.0).floor() - (previous / 100.0).floor()
        + (previous / 400.0).floor()
}

fn epoch_days(year: &str, month: u32, day: u32) -> Option<f64> {
    let numeric_year = year.parse::<f64>().ok()?;
    let y = year_remainder(year)?;
    let leap = y % 4 == 0 && (y % 100 != 0 || y == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let before_month: u32 = month_days[..(month - 1) as usize].iter().sum();
    let days = days_before_year(numeric_year) - days_before_year(1970.0)
        + f64::from(before_month + day - 1);
    days.is_finite().then_some(days)
}

/// HTML's numeric domain for range and step constraints, not a local timezone date.
pub(crate) fn value_number(kind: &str, raw: &str) -> Option<f64> {
    let number = match kind {
        "date" => {
            let (year, month, day) = date_parts(raw)?;
            epoch_days(year, month, day)? * MILLISECONDS_PER_DAY
        }
        "month" => {
            let (year, month) = month_parts(raw)?;
            (year.parse::<f64>().ok()? - 1970.0) * 12.0 + f64::from(month - 1)
        }
        "week" => {
            if !valid_week(raw) {
                return None;
            }
            let (year, week) = raw.rsplit_once("-W")?;
            // ISO week one contains January 4; weeks begin on Monday.
            let january_fourth = epoch_days(year, 1, 4)?;
            let monday = january_fourth - (january_fourth + 3.0).rem_euclid(7.0);
            (monday + f64::from(two_digits(week)? - 1) * 7.0) * MILLISECONDS_PER_DAY
        }
        "time" => time_parts(raw)?.milliseconds(),
        "datetime-local" => {
            let (date, time) = raw.split_once(['T', ' '])?;
            value_number("date", date)? + time_parts(time)?.milliseconds()
        }
        _ => return None,
    };
    number.is_finite().then_some(number)
}

pub(crate) struct StepDomain {
    pub scale: f64,
    pub default_step: f64,
    pub default_base: f64,
    pub periodic: bool,
}

pub(crate) fn step_domain(kind: &str) -> Option<StepDomain> {
    let (scale, default_step, default_base) = match kind {
        "date" => (MILLISECONDS_PER_DAY, 1.0, 0.0),
        "month" => (1.0, 1.0, 0.0),
        "week" => (MILLISECONDS_PER_WEEK, 1.0, -3.0 * MILLISECONDS_PER_DAY),
        "time" | "datetime-local" => (MILLISECONDS_PER_SECOND, 60.0, 0.0),
        _ => return None,
    };
    Some(StepDomain {
        scale,
        default_step,
        default_base,
        periodic: kind == "time",
    })
}

#[derive(Default)]
pub(crate) struct Constraints {
    pub range_applicable: bool,
    pub underflow: bool,
    pub overflow: bool,
    pub step_mismatch: bool,
}

/// Shared by DOM validity and selector matching so controls cannot paint a different state.
pub(crate) fn constraints(
    kind: &str,
    value: &str,
    min: Option<&str>,
    max: Option<&str>,
    step: Option<&str>,
    default_value: Option<&str>,
) -> Option<Constraints> {
    let domain = step_domain(kind)?;
    let min = min.and_then(|raw| value_number(kind, raw));
    let max = max.and_then(|raw| value_number(kind, raw));
    let mut result = Constraints {
        range_applicable: min.is_some() || max.is_some(),
        ..Constraints::default()
    };
    let Some(number) = value_number(kind, value) else {
        return Some(result);
    };
    match min.zip(max) {
        Some((min, max)) if domain.periodic && min > max => {
            let in_gap = number < min && number > max;
            result.underflow = in_gap;
            result.overflow = in_gap;
        }
        _ => {
            result.underflow = min.is_some_and(|min| number < min);
            result.overflow = max.is_some_and(|max| number > max);
        }
    }
    if !step.is_some_and(|raw| raw.eq_ignore_ascii_case("any")) {
        let step = step
            .and_then(super::forms::parse_floating_point)
            .filter(|step| *step > 0.0)
            .unwrap_or(domain.default_step)
            * domain.scale;
        let base = min
            .or_else(|| default_value.and_then(|raw| value_number(kind, raw)))
            .unwrap_or(domain.default_base);
        let offset = (number - base) / step;
        result.step_mismatch = (offset - offset.round()).abs() > 1e-9;
    }
    Some(result)
}

/// `None` means this is not a temporal input; invalid temporal values become empty.
pub(crate) fn sanitize(input_type: &str, raw: &str) -> Option<String> {
    let valid = match input_type {
        "date" => date_parts(raw).is_some(),
        "month" => month_parts(raw).is_some(),
        "week" => valid_week(raw),
        "time" => time_parts(raw).is_some(),
        "datetime-local" => {
            return Some(
                raw.split_once(['T', ' '])
                    .and_then(|(date, time)| {
                        date_parts(date)?;
                        Some(format!("{date}T{}", time_parts(time)?.normalized()))
                    })
                    .unwrap_or_default(),
            );
        }
        _ => return None,
    };
    Some(if valid { raw.to_owned() } else { String::new() })
}

#[cfg(test)]
mod picker_tests {
    use super::*;

    #[test]
    fn picker_week_conversion_handles_iso_year_boundaries() {
        for (date, expected) in [
            ("2015-12-31", "2015-W53"),
            ("2016-01-01", "2015-W53"),
            ("2016-01-04", "2016-W01"),
            ("2020-12-31", "2020-W53"),
            ("2021-01-03", "2020-W53"),
            ("2024-12-30", "2025-W01"),
        ] {
            let (year, month, day) = date_parts(date).unwrap();
            assert_eq!(
                week_from_date(year.parse().unwrap(), month, day).as_deref(),
                Some(expected)
            );
            let thursday = week_date(expected).unwrap();
            assert_eq!(
                week_from_date(thursday.0, thursday.1, thursday.2).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn picker_date_inverse_roundtrips_gregorian_month_boundaries() {
        for year in [
            1, 1600, 1900, 1969, 1970, 2000, 2024, 2026, 2100, 2400, 10000,
        ] {
            for month in 1..=12 {
                for day in [1, crate::widgets::days_in_month(year, month)] {
                    let date = format!("{year:04}-{month:02}-{day:02}");
                    let days = value_number("date", &date).unwrap() / MILLISECONDS_PER_DAY;
                    assert_eq!(
                        date_from_epoch_days(days),
                        Some((year, month, day)),
                        "{date}"
                    );
                }
            }
        }
        assert_eq!(date_from_epoch_days(f64::NAN), None);
    }
}
