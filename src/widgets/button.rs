//! Button widget — standalone tiny-skia rendered button.

use super::{WidgetColors, rounded_rect_path};
use tiny_skia::*;

pub struct Button {
    pub label: String,
    pub disabled: bool,
    pub pressed: bool,
    pub focused: bool,
    pub colors: WidgetColors,
    pub width: f32,
    pub height: f32,
}

impl Button {
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_string(),
            disabled: false,
            pressed: false,
            focused: false,
            colors: WidgetColors {
                background: (239, 239, 239, 255),
                border: (118, 118, 118, 255),
                ..WidgetColors::default()
            },
            width: 80.0,
            height: 28.0,
        }
    }

    /// Paint the button (background + border). Text drawn by caller.
    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let ts = Transform::from_scale(scale, scale);
        let mut paint = Paint::default();
        paint.anti_alias = true;

        // Background (slightly darker when pressed)
        let (r, g, b, a) = if self.pressed {
            (200, 200, 200, 255)
        } else {
            self.colors.background
        };
        paint.set_color_rgba8(r, g, b, a);
        if let Some(path) = rounded_rect_path(x, y, self.width, self.height, 4.0) {
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
        stroke.width = 1.0;
        if let Some(path) = rounded_rect_path(x, y, self.width, self.height, 4.0) {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn click(&mut self, x: f32, y: f32) -> bool {
        if self.disabled {
            return false;
        }
        x >= 0.0 && y >= 0.0 && x <= self.width && y <= self.height
    }
}
