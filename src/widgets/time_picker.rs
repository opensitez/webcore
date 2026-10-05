//! Draft time selection; HTML value grammar remains in the shared temporal module.

use super::Calendar;
use crate::html::temporal::{Time, time_parts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimeAction {
    Adjust(usize, i32),
    Select(usize),
    Clear,
    Commit,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimePicker {
    time: Time,
    pub(crate) active: usize,
    digits: usize,
}

impl TimePicker {
    pub(crate) const COLUMNS: usize = 4;
    pub(crate) const ROW: f32 = Calendar::CELL;
    pub(crate) const HEIGHT: f32 = Self::ROW * 5.0;
    pub(crate) const LABELS: [&'static str; 4] = ["Hour", "Min", "Sec", "ms"];
    const LIMITS: [u32; 4] = [24, 60, 60, 1000];

    pub(crate) fn new(value: &str) -> Self {
        Self {
            time: time_parts(value).unwrap_or_default(),
            active: 0,
            digits: 0,
        }
    }

    pub(crate) fn value(&self) -> String {
        self.time.normalized()
    }

    pub(crate) fn parts(&self) -> [u32; 4] {
        [
            self.time.hour,
            self.time.minute,
            self.time.second,
            self.time.millisecond,
        ]
    }

    fn set_part(&mut self, column: usize, value: u32) {
        match column {
            0 => self.time.hour = value,
            1 => self.time.minute = value,
            2 => self.time.second = value,
            3 => self.time.millisecond = value,
            _ => return,
        }
    }

    pub(crate) fn adjust(&mut self, column: usize, delta: i32) {
        if column >= Self::COLUMNS {
            return;
        }
        let value = (i64::from(self.parts()[column]) + i64::from(delta))
            .rem_euclid(i64::from(Self::LIMITS[column])) as u32;
        self.set_part(column, value);
        self.select(column);
    }

    pub(crate) fn select(&mut self, column: usize) {
        if column < Self::COLUMNS {
            self.active = column;
            self.digits = 0;
        }
    }

    pub(crate) fn move_active(&mut self, delta: i32) {
        self.select((self.active as i32 + delta).rem_euclid(Self::COLUMNS as i32) as usize);
    }

    pub(crate) fn set_endpoint(&mut self, end: bool) {
        self.set_part(
            self.active,
            if end {
                Self::LIMITS[self.active] - 1
            } else {
                0
            },
        );
        self.digits = 0;
    }

    pub(crate) fn digit(&mut self, digit: u32) -> bool {
        if digit > 9 {
            return false;
        }
        let width = if self.active == 3 { 3 } else { 2 };
        let previous = if self.digits == 0 || self.digits == width {
            0
        } else {
            self.parts()[self.active]
        };
        let next = previous * 10 + digit;
        if next >= Self::LIMITS[self.active] {
            return false;
        }
        self.set_part(self.active, next);
        self.digits = if self.digits == width {
            1
        } else {
            self.digits + 1
        };
        true
    }

    pub(crate) fn action_at(local: (f32, f32)) -> Option<TimeAction> {
        if local.0 < 0.0 || local.0 >= Calendar::width() || local.1 < 0.0 || local.1 >= Self::HEIGHT
        {
            return None;
        }
        let column = (local.0 / (Calendar::width() / Self::COLUMNS as f32)) as usize;
        match (local.1 / Self::ROW) as usize {
            1 => Some(TimeAction::Adjust(column, 1)),
            2 => Some(TimeAction::Select(column)),
            3 => Some(TimeAction::Adjust(column, -1)),
            4 if local.0 < Calendar::width() / 2.0 => Some(TimeAction::Clear),
            4 => Some(TimeAction::Commit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_picker_draft_preserves_precision_and_wraps_fields() {
        let mut picker = TimePicker::new("23:59:59.125");
        picker.adjust(0, 1);
        assert_eq!(picker.value(), "00:59:59.125");
        picker.adjust(1, 1);
        picker.adjust(2, 1);
        assert_eq!(picker.value(), "00:00:00.125");
        picker.select(3);
        for digit in [9, 9, 9] {
            assert!(picker.digit(digit));
        }
        assert_eq!(picker.value(), "00:00:00.999");
        picker.adjust(3, 1);
        assert_eq!(picker.value(), "00:00");
    }

    #[test]
    fn time_picker_actions_share_geometry_and_reject_outside_points() {
        let width = Calendar::width();
        assert_eq!(
            TimePicker::action_at((width / 8.0, TimePicker::ROW * 1.5)),
            Some(TimeAction::Adjust(0, 1))
        );
        assert_eq!(
            TimePicker::action_at((width * 0.75, TimePicker::ROW * 4.5)),
            Some(TimeAction::Commit)
        );
        assert_eq!(
            TimePicker::action_at((width * 0.25, TimePicker::ROW * 4.5)),
            Some(TimeAction::Clear)
        );
        assert_eq!(TimePicker::action_at((-1.0, TimePicker::ROW)), None);
        assert_eq!(TimePicker::action_at((width, TimePicker::ROW)), None);
    }
}
