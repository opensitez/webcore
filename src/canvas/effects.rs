//! Browser CSS filter types adapted to the shared Canvas raster operations.

use tiny_skia::Pixmap;

use super::Color;
use crate::types::{Color as CssColor, CssFilters, FilterOp};

pub use webcanvas::canvas::effects::{blur_pixmap, shadow_layer, tint_to};

pub fn apply_filter_list(pixmap: &mut Pixmap, filters: &CssFilters) {
    for op in &filters.ops {
        apply_filter_op(pixmap, op);
    }
}

pub fn apply_filter_op(pixmap: &mut Pixmap, op: &FilterOp) {
    use webcanvas::canvas::FilterOp as CanvasOp;
    let converted = match op {
        FilterOp::Blur(v) => CanvasOp::Blur(*v),
        FilterOp::Brightness(v) => CanvasOp::Brightness(*v),
        FilterOp::Contrast(v) => CanvasOp::Contrast(*v),
        FilterOp::Grayscale(v) => CanvasOp::Grayscale(*v),
        FilterOp::HueRotate(v) => CanvasOp::HueRotate(*v),
        FilterOp::Invert(v) => CanvasOp::Invert(*v),
        FilterOp::Opacity(v) => CanvasOp::Opacity(*v),
        FilterOp::Saturate(v) => CanvasOp::Saturate(*v),
        FilterOp::Sepia(v) => CanvasOp::Sepia(*v),
        FilterOp::DropShadow {
            dx,
            dy,
            blur,
            color,
        } => CanvasOp::DropShadow {
            dx: *dx,
            dy: *dy,
            blur: *blur,
            color: Color::rgba(color.r, color.g, color.b, color.a),
        },
    };
    webcanvas::canvas::effects::apply_filter_op(pixmap, &converted);
}

pub(crate) fn drop_shadow(pixmap: &mut Pixmap, dx: f32, dy: f32, blur: f32, color: CssColor) {
    webcanvas::canvas::effects::drop_shadow(
        pixmap,
        dx,
        dy,
        blur,
        Color::rgba(color.r, color.g, color.b, color.a),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_square() -> Pixmap {
        let mut p = Pixmap::new(41, 41).expect("a pixmap");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        p.fill_rect(
            tiny_skia::Rect::from_xywh(15.0, 15.0, 11.0, 11.0).expect("a rect"),
            &paint,
            tiny_skia::Transform::identity(),
            None,
        );
        p
    }

    #[test]
    fn replay_blur_filter_uses_the_shared_blur_primitive() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(10, 20).expect("in bounds").alpha(), 0);
        crate::renderer::display_list_replay::apply_pixel_filter(&mut p, 0, 4.0);
        assert!(
            p.pixel(10, 20).expect("in bounds").alpha() > 0,
            "display-list blur should spread alpha just like canvas blur"
        );
    }

    #[test]
    fn drop_shadow_paints_offset_shadow_under_source() {
        let mut p = opaque_square();
        drop_shadow(&mut p, 10.0, 0.0, 0.0, CssColor::rgba(0, 0, 255, 255));

        let offset = p.pixel(30, 20).expect("in bounds");
        assert!(
            offset.blue() > 0 && offset.alpha() > 0,
            "offset shadow should be painted at the requested dx"
        );
        let source = p.pixel(20, 20).expect("in bounds");
        assert_eq!(source.red(), 255, "source must remain above the shadow");
    }

    #[test]
    fn a_blurred_pixel_stays_a_valid_premultiplied_colour() {
        // Averaging the four planes independently can push a colour channel
        // above its own alpha, which is not representable. The clamp in
        // `from_planes` is what this checks.
        let mut p = Pixmap::new(20, 20).expect("a pixmap");
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(255, 255, 255, 40));
        p.fill_rect(
            tiny_skia::Rect::from_xywh(5.0, 5.0, 10.0, 10.0).expect("a rect"),
            &paint,
            tiny_skia::Transform::identity(),
            None,
        );
        blur_pixmap(&mut p, 2.0);
        for px in p.pixels() {
            assert!(
                px.red() <= px.alpha() && px.green() <= px.alpha() && px.blue() <= px.alpha(),
                "channel above alpha: {} {} {} / {}",
                px.red(),
                px.green(),
                px.blue(),
                px.alpha()
            );
        }
    }

    #[test]
    fn tinting_keeps_the_shape_and_replaces_the_colour() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(20, 20).expect("in bounds").red(), 255, "red first");
        tint_to(&mut p, Color::rgb(0, 0, 255));
        let inside = p.pixel(20, 20).expect("in bounds");
        assert_eq!(inside.red(), 0);
        assert_eq!(inside.blue(), 255);
        assert_eq!(inside.alpha(), 255, "the shape is unchanged");
        assert_eq!(
            p.pixel(2, 2).expect("in bounds").alpha(),
            0,
            "and nothing appeared outside it"
        );
    }

    #[test]
    fn tinting_with_a_translucent_colour_scales_the_alpha() {
        let mut p = opaque_square();
        tint_to(&mut p, Color::rgba(0, 0, 255, 128));
        assert_eq!(p.pixel(20, 20).expect("in bounds").alpha(), 128);
    }
}
