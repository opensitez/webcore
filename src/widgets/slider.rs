//! Range slider widget — standalone tiny-skia rendered slider.

use super::{WidgetColors, circle_path, rounded_rect_path};
use tiny_skia::*;

pub struct Slider {
    pub value: f32, // 0.0..1.0
    pub min: f32,
    pub max: f32,
    pub disabled: bool,
    pub focused: bool,
    pub dragging: bool,
    pub colors: WidgetColors,
    pub width: f32,
    pub height: f32,
    pub track_height: f32,
    pub thumb_radius: f32,
    /// Runs top-to-bottom instead of left-to-right — a VERTICAL writing mode.
    ///
    /// **This is the whole of what makes a range input vertical**, and it is
    /// CSS that says so (`writing-mode: vertical-rl`/`vertical-lr`), not a
    /// second element and not the non-standard `orient` attribute. A control is
    /// laid out along its inline axis; a vertical writing mode turns that axis.
    ///
    /// The value increases DOWNWARD, following that inline axis — the same
    /// direction the text would run — so the minimum is at the top. `direction`
    /// would flip it, which is not modelled here.
    pub vertical: bool,
}

impl Slider {
    pub fn new(min: f32, max: f32, value: f32) -> Self {
        let pct = if max > min {
            (value - min) / (max - min)
        } else {
            0.0
        };
        Self {
            value: pct.clamp(0.0, 1.0),
            min,
            max,
            disabled: false,
            focused: false,
            dragging: false,
            colors: WidgetColors::default(),
            width: 200.0,
            height: 20.0,
            track_height: 4.0,
            thumb_radius: 8.0,
            vertical: false,
        }
    }

    /// Get the actual value (mapped from 0..1 to min..max).
    pub fn actual_value(&self) -> f32 {
        self.min + self.value * (self.max - self.min)
    }

    pub fn paint(&self, pixmap: &mut Pixmap, x: f32, y: f32, scale: f32) {
        let ts = Transform::from_scale(scale, scale);
        let mut paint = Paint::default();
        paint.anti_alias = true;

        // The track runs along the INLINE axis and is centred on the other one;
        // the thumb travels the track's length, inset by its own radius at each
        // end so it stops flush rather than half off the control. Both axes are
        // the same geometry with the roles of width and height exchanged.
        let (track_x, track_y, track_w, track_h) = if self.vertical {
            (
                x + (self.width - self.track_height) / 2.0,
                y,
                self.track_height,
                self.height,
            )
        } else {
            (
                x,
                y + (self.height - self.track_height) / 2.0,
                self.width,
                self.track_height,
            )
        };
        let travel = if self.vertical {
            self.height
        } else {
            self.width
        } - self.thumb_radius * 2.0;
        let along = self.thumb_radius + self.value * travel.max(0.0);
        let (thumb_x, thumb_y) = if self.vertical {
            (x + self.width / 2.0, y + along)
        } else {
            (x + along, y + self.height / 2.0)
        };

        // Track background
        paint.set_color_rgba8(200, 200, 200, 255);
        if let Some(path) = rounded_rect_path(track_x, track_y, track_w, track_h, 2.0) {
            pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
        }

        // Filled portion — from the track's start up to the thumb.
        let (r, g, b, a) = self.colors.accent;
        paint.set_color_rgba8(r, g, b, a);
        let filled = if self.vertical {
            thumb_y - y
        } else {
            thumb_x - x
        };
        if filled > 0.0 {
            let (fw, fh) = if self.vertical {
                (track_w, filled)
            } else {
                (filled, track_h)
            };
            if let Some(path) = rounded_rect_path(track_x, track_y, fw, fh, 2.0) {
                pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
            }
        }

        // Thumb
        let (r, g, b, a) = self.colors.background;
        paint.set_color_rgba8(r, g, b, a);
        if let Some(path) = circle_path(thumb_x, thumb_y, self.thumb_radius) {
            pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
        }
        let (r, g, b, a) = if self.focused {
            self.colors.focus_ring
        } else {
            self.colors.border
        };
        paint.set_color_rgba8(r, g, b, a);
        let mut stroke = Stroke::default();
        stroke.width = 1.0;
        if let Some(path) = circle_path(thumb_x, thumb_y, self.thumb_radius) {
            pixmap.stroke_path(&path, &paint, &stroke, ts, None);
        }
    }

    pub fn measure(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    /// Handle mouse down — start dragging if on thumb.
    pub fn mouse_down(&mut self, x: f32, y: f32) -> bool {
        if self.disabled {
            return false;
        }
        self.dragging = true;
        self.set_from_pointer(x, y);
        true
    }

    /// Handle mouse move during drag.
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        if self.dragging {
            self.set_from_pointer(x, y);
        }
    }

    /// Handle mouse up — stop dragging.
    pub fn mouse_up(&mut self) {
        self.dragging = false;
    }

    /// Where along the track the pointer landed. Reads the axis the control
    /// actually runs on — a vertical slider that measured `x` would answer with
    /// whatever the pointer's horizontal position happened to be.
    fn set_from_pointer(&mut self, x: f32, y: f32) {
        let (along, extent) = if self.vertical {
            (y, self.height)
        } else {
            (x, self.width)
        };
        let usable = extent - self.thumb_radius * 2.0;
        if usable <= 0.0 {
            return;
        }
        self.value = ((along - self.thumb_radius) / usable).clamp(0.0, 1.0);
    }
}
