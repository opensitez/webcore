//! Radio button widget — standalone tiny-skia rendered radio button.

use super::{WidgetColors, circle_path};
use tiny_skia::*;

pub struct Radio {
    pub selected: bool,
    pub label: String,
    pub disabled: bool,
    pub focused: bool,
    pub colors: WidgetColors,
    pub size: f32,
}

impl Radio {
    pub fn new(label: &str) -> Self {
        Self {
            selected: false,
            label: label.to_string(),
            disabled: false,
            focused: false,
            colors: WidgetColors::default(),
            size: 16.0,
        }
    }

    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let ts = Transform::from_scale(scale, scale);
        let sz = self.size;
        let cx = x + sz / 2.0;
        let cy = y + sz / 2.0;
        let r = sz / 2.0;
        let mut paint = Paint::default();
        paint.anti_alias = true;

        // Outer circle
        if let Some(path) = circle_path(cx, cy, r) {
            let (cr, cg, cb, ca) = self.colors.background;
            paint.set_color_rgba8(cr, cg, cb, ca);
            pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);

            let (cr, cg, cb, ca) = if self.focused {
                self.colors.focus_ring
            } else {
                self.colors.border
            };
            paint.set_color_rgba8(cr, cg, cb, ca);
            let mut stroke = Stroke::default();
            stroke.width = if self.focused { 2.0 } else { 1.0 };
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }

        // Inner dot
        if self.selected {
            let ir = r * 0.45;
            if let Some(path) = circle_path(cx, cy, ir) {
                let (cr, cg, cb, ca) = self.colors.foreground;
                paint.set_color_rgba8(cr, cg, cb, ca);
                pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
            }
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        (self.size, self.size)
    }

    pub fn click(&mut self, x: f32, y: f32) -> bool {
        if self.disabled {
            return false;
        }
        let cx = self.size / 2.0;
        let cy = self.size / 2.0;
        let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        if dist <= self.size / 2.0 {
            self.selected = true;
            return true;
        }
        false
    }
}
