//! Vibe Widgets — standalone tiny-skia GUI widgets.
//!
//! These widgets render form elements using only tiny-skia. They have zero
//! dependency on the HTML/CSS/DOM engine and can be used standalone:
//!
//! ```ignore
//! use vibe_widgets::{Checkbox, TextInput, Widget};
//!
//! let mut checkbox = Checkbox::new("Accept terms");
//! checkbox.paint(&mut pixmap, 10.0, 10.0, 1.0);
//! if checkbox.click(mouse_x, mouse_y) { /* toggled */ }
//! ```
//!
//! Inside the browser engine, these same widgets are used to render HTML form
//! elements (`<input>`, `<select>`, `<textarea>`, etc.).

pub mod button;
pub mod checkbox;
pub mod color_swatch;
pub mod date_field;
pub mod file_button;
pub mod progress;
pub mod radio;
pub mod select;
pub mod slider;
pub mod stepper;
pub mod textfield;

pub use button::Button;
pub use checkbox::Checkbox;
pub use color_swatch::{ColorSwatch, PALETTE, PALETTE_CELL, PALETTE_COLUMNS, to_simple_colour};
pub use date_field::{
    Calendar, DateField, Kind as DateKind, days_in_month, first_weekday, parse_date, to_date_value,
};
pub use file_button::{CHOOSE, FileButton, NOTHING_CHOSEN};
pub use progress::{Band, Gauge, meter_band};
pub use radio::Radio;
pub use select::Select;
pub use slider::Slider;
pub use stepper::Stepper;
pub use textfield::TextInput;

use tiny_skia::PathBuilder;

/// Colors used by widgets.
#[derive(Clone, Copy, Debug)]
pub struct WidgetColors {
    pub foreground: (u8, u8, u8, u8),
    pub background: (u8, u8, u8, u8),
    pub border: (u8, u8, u8, u8),
    pub accent: (u8, u8, u8, u8),
    pub placeholder: (u8, u8, u8, u8),
    pub focus_ring: (u8, u8, u8, u8),
}

impl Default for WidgetColors {
    fn default() -> Self {
        Self {
            foreground: (51, 51, 51, 255),
            background: (255, 255, 255, 255),
            border: (118, 118, 118, 255),
            accent: (0, 102, 204, 255),
            placeholder: (128, 128, 128, 128),
            focus_ring: (0, 102, 204, 128),
        }
    }
}

/// Shared helper: draw a rounded rectangle path.
pub fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Shared helper: draw a circle path using 4 quadratic bezier arcs.
pub fn circle_path(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
    let k = r * 0.5522848; // magic number for circular arc approximation
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - r);
    pb.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
    pb.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
    pb.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
    pb.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
    pb.close();
    pb.finish()
}
