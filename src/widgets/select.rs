//! Select/dropdown widget — standalone tiny-skia rendered select box.

use super::WidgetColors;
use tiny_skia::*;

const INDICATOR_INLINE_FACTOR: f32 = 1.5;
const LABEL_INSET_FACTOR: f32 = 0.25;

pub(crate) fn indicator_inline_size(font_px: f32) -> f32 {
    font_px * INDICATOR_INLINE_FACTOR
}

pub(crate) fn label_inline_inset(font_px: f32) -> f32 {
    font_px * LABEL_INSET_FACTOR
}

pub struct Select {
    pub options: Vec<String>,
    pub selected_index: usize,
    pub open: bool,
    pub disabled: bool,
    pub focused: bool,
    pub colors: WidgetColors,
    pub width: f32,
    pub height: f32,
}

impl Select {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            options,
            selected_index: 0,
            open: false,
            disabled: false,
            focused: false,
            colors: WidgetColors::default(),
            width: 200.0,
            height: 24.0,
        }
    }

    pub fn selected_text(&self) -> &str {
        self.options
            .get(self.selected_index)
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// Paint the select box (closed state). Draws border + dropdown arrow.
    /// Text rendering is handled by the caller.
    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let ts = Transform::from_scale(scale, scale);
        let mut paint = Paint::default();
        paint.anti_alias = true;

        // Background
        let (r, g, b, a) = self.colors.background;
        paint.set_color_rgba8(r, g, b, a);
        if let Some(path) = super::rounded_rect_path(x, y, self.width, self.height, 3.0) {
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
        if let Some(path) = super::rounded_rect_path(x, y, self.width, self.height, 3.0) {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }

        // Dropdown arrow
        let arrow_x = x + self.width - 16.0;
        let arrow_y = y + self.height / 2.0;
        let (r, g, b, a) = self.colors.foreground;
        paint.set_color_rgba8(r, g, b, a);
        stroke.width = 1.5;
        let mut pb = PathBuilder::new();
        pb.move_to(arrow_x - 4.0, arrow_y - 2.0);
        pb.line_to(arrow_x, arrow_y + 2.0);
        pb.line_to(arrow_x + 4.0, arrow_y - 2.0);
        if let Some(path) = pb.finish() {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn click(&mut self, _x: f32, _y: f32) -> bool {
        if self.disabled {
            return false;
        }
        self.open = !self.open;
        true
    }

    pub fn select_index(&mut self, idx: usize) {
        if idx < self.options.len() {
            self.selected_index = idx;
            self.open = false;
        }
    }
}
