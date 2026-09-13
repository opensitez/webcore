//! Checkbox widget — standalone tiny-skia rendered checkbox.

use super::{WidgetColors, rounded_rect_path};
use tiny_skia::*;

pub struct Checkbox {
    pub checked: bool,
    pub label: String,
    pub disabled: bool,
    pub focused: bool,
    pub colors: WidgetColors,
    pub size: f32,
}

impl Checkbox {
    pub fn new(label: &str) -> Self {
        Self {
            checked: false,
            label: label.to_string(),
            disabled: false,
            focused: false,
            colors: WidgetColors::default(),
            size: 16.0,
        }
    }

    /// Paint the checkbox at (x, y) into the pixmap.
    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let ts = Transform::from_scale(scale, scale);
        let sz = self.size;
        let mut paint = Paint::default();
        paint.anti_alias = true;

        // Background
        let (r, g, b, a) = self.colors.background;
        paint.set_color_rgba8(r, g, b, a);
        if let Some(path) = rounded_rect_path(x, y, sz, sz, 2.0) {
            pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
        }

        // Border
        let (r, g, b, a) = if self.focused {
            self.colors.focus_ring
        } else {
            self.colors.border
        };
        paint.set_color_rgba8(r, g, b, a);
        let mut stroke = Stroke::default();
        stroke.width = if self.focused { 2.0 } else { 1.0 };
        if let Some(path) = rounded_rect_path(x, y, sz, sz, 2.0) {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }

        // Check mark
        if self.checked {
            let cx = x + sz / 2.0;
            let cy = y + sz / 2.0;
            let s = sz * 0.3;
            let (r, g, b, a) = self.colors.foreground;
            paint.set_color_rgba8(r, g, b, a);
            stroke.width = 2.0;
            let mut pb = PathBuilder::new();
            pb.move_to(cx - s, cy);
            pb.line_to(cx - s * 0.3, cy + s * 0.7);
            pb.line_to(cx + s, cy - s * 0.6);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &paint, &stroke, ts, None);
            }
        }
    }

    /// Returns (width, height) for layout.
    pub fn measure(&self) -> (f32, f32) {
        (self.size, self.size)
    }

    /// Handle a click at (x, y) relative to the widget's origin.
    /// Returns true if the checkbox was toggled.
    pub fn click(&mut self, x: f32, y: f32) -> bool {
        if self.disabled {
            return false;
        }
        if x >= 0.0 && y >= 0.0 && x <= self.size && y <= self.size {
            self.checked = !self.checked;
            return true;
        }
        false
    }

    /// Toggle the checked state.
    pub fn toggle(&mut self) {
        if !self.disabled {
            self.checked = !self.checked;
        }
    }
}
