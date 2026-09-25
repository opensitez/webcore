//! Shadow and filter — the two things a drawing passes through before it lands.
//!
//! HTML §4.12.5.1.13 defines one drawing model for every canvas operation, and
//! it is not "paint the shape". The shape is rendered to its own bitmap, that
//! bitmap is FILTERED, a shadow is derived from the filtered bitmap's alpha,
//! and only then is the pair composited onto the canvas. Doing it any other way
//! gets observable things wrong — a shadow of an already-shadowed shape, a
//! filter that misses the shadow, a `globalAlpha` applied twice.
//!
//! ## Why the blur lives here
//!
//! webcore had no blur at all. `display_list_replay.rs` says
//! `0 => {} // blur — needs convolution, skipped`, and `PaintCmd::BoxShadow`
//! and `PaintCmd::TextShadow` both destructure `blur: _`. So `shadowBlur`,
//! `filter: blur()`, CSS `box-shadow` and CSS `text-shadow` were four features
//! waiting on one missing primitive. [`blur_pixmap`] is that primitive; the
//! canvas uses it here, and the three renderer sites are now one call away from
//! using it too.

use rayon::prelude::*;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::types::{Color as CssColor, CssFilters, FilterOp};

use super::Color;

/// Gaussian blur, by three successive box blurs.
///
/// **Three boxes, not a true Gaussian kernel**, and that is the specified
/// algorithm rather than a shortcut: SVG's `feGaussianBlur` — which is what
/// `shadowBlur` and CSS `blur()` are both defined in terms of — says outright
/// that three box blurs approximate a Gaussian closely enough, and gives this
/// box size for a given standard deviation. A real Gaussian convolution would
/// be slower and no more correct.
///
/// Operates on PREMULTIPLIED pixels, which is why it can average the four
/// channels alike. Blurring un-premultiplied colour bleeds the RGB of fully
/// transparent pixels into visible ones and haloes every soft edge.
pub fn blur_pixmap(pixmap: &mut Pixmap, std_dev: f32) {
    if std_dev <= 0.0 || !std_dev.is_finite() {
        return;
    }
    // SVG's own formula for the box width that approximates `std_dev`.
    let box_size = (std_dev * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0 + 0.5).floor();
    let radius = (box_size as i32 / 2).max(1);

    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    for (i, pixel) in pixmap.pixels().iter().enumerate() {
        if pixel.alpha() != 0 {
            let x = i % w;
            let y = i / w;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
    }
    if min_x == w {
        return;
    }
    // Three box passes can spread each pixel by at most three radii. Cropping
    // transparent tile margins before allocating channel planes preserves the
    // exact filter result while avoiding a full-tile convolution for small art.
    let spread = (radius as usize).saturating_mul(3);
    let left = min_x.saturating_sub(spread);
    let top = min_y.saturating_sub(spread);
    let right = max_x.saturating_add(spread).min(w);
    let bottom = max_y.saturating_add(spread).min(h);
    let crop_w = right - left;
    let crop_h = bottom - top;
    if crop_w.saturating_mul(crop_h) < w.saturating_mul(h) * 3 / 4 {
        if let Some(mut cropped) = Pixmap::new(crop_w as u32, crop_h as u32) {
            for y in 0..crop_h {
                let src = ((top + y) * w + left) * 4;
                let dst = y * crop_w * 4;
                cropped.data_mut()[dst..dst + crop_w * 4]
                    .copy_from_slice(&pixmap.data()[src..src + crop_w * 4]);
            }
            blur_pixmap_full(&mut cropped, radius);
            pixmap.fill(tiny_skia::Color::TRANSPARENT);
            for y in 0..crop_h {
                let src = y * crop_w * 4;
                let dst = ((top + y) * w + left) * 4;
                pixmap.data_mut()[dst..dst + crop_w * 4]
                    .copy_from_slice(&cropped.data()[src..src + crop_w * 4]);
            }
            return;
        }
    }
    blur_pixmap_full(pixmap, radius);
}

fn blur_pixmap_full(pixmap: &mut Pixmap, radius: i32) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    // Work in u32 channel planes: repeated averaging on u8 loses a level each
    // pass, and three passes of that is visible banding on a soft shadow.
    let mut channels = to_planes(pixmap);
    channels.par_iter_mut().for_each(|plane| {
        let mut scratch = vec![0u32; w * h];
        for _ in 0..3 {
            box_blur_horizontal(plane, &mut scratch, w, h, radius);
            box_blur_vertical(&mut scratch, plane, w, h, radius);
        }
    });
    from_planes(pixmap, &channels);
}

fn to_planes(pixmap: &Pixmap) -> [Vec<u32>; 4] {
    let pixels = pixmap.pixels();
    let mut planes = [
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
        vec![0u32; pixels.len()],
    ];
    for (i, px) in pixels.iter().enumerate() {
        planes[0][i] = px.red() as u32;
        planes[1][i] = px.green() as u32;
        planes[2][i] = px.blue() as u32;
        planes[3][i] = px.alpha() as u32;
    }
    planes
}

fn from_planes(pixmap: &mut Pixmap, planes: &[Vec<u32>; 4]) {
    for (i, px) in pixmap.pixels_mut().iter_mut().enumerate() {
        let a = planes[3][i].min(255) as u8;
        // Averaging the planes independently can leave a channel above its own
        // alpha, which is not a representable premultiplied pixel. Clamping to
        // alpha is what keeps the result valid.
        let r = planes[0][i].min(planes[3][i]).min(255) as u8;
        let g = planes[1][i].min(planes[3][i]).min(255) as u8;
        let b = planes[2][i].min(planes[3][i]).min(255) as u8;
        if let Some(p) = PremultipliedColorU8::from_rgba(r, g, b, a) {
            *px = p;
        }
    }
}

/// One box-blur pass along x, using a running sum so the cost is independent of
/// the radius.
fn box_blur_horizontal(src: &[u32], dst: &mut [u32], w: usize, h: usize, radius: i32) {
    let span = (radius * 2 + 1) as u32;
    for y in 0..h {
        let row = y * w;
        // Seed the window at x = 0, with the left half clamped to the edge
        // pixel — the same edge handling `feGaussianBlur` uses (`duplicate`).
        let mut sum: u32 = 0;
        for k in -radius..=radius {
            let x = k.clamp(0, w as i32 - 1) as usize;
            sum += src[row + x];
        }
        for x in 0..w {
            dst[row + x] = sum / span;
            let leaving = (x as i32 - radius).clamp(0, w as i32 - 1) as usize;
            let entering = (x as i32 + radius + 1).clamp(0, w as i32 - 1) as usize;
            sum = sum + src[row + entering] - src[row + leaving];
        }
    }
}

/// The same pass along y. Separable: a 2D Gaussian is the product of two 1D
/// ones, so two passes give the 2D result for a fraction of the work.
fn box_blur_vertical(src: &[u32], dst: &mut [u32], w: usize, h: usize, radius: i32) {
    let span = (radius * 2 + 1) as u32;
    for x in 0..w {
        let mut sum: u32 = 0;
        for k in -radius..=radius {
            let y = k.clamp(0, h as i32 - 1) as usize;
            sum += src[y * w + x];
        }
        for y in 0..h {
            dst[y * w + x] = sum / span;
            let leaving = (y as i32 - radius).clamp(0, h as i32 - 1) as usize;
            let entering = (y as i32 + radius + 1).clamp(0, h as i32 - 1) as usize;
            sum = sum + src[entering * w + x] - src[leaving * w + x];
        }
    }
}

/// Replace every pixel's colour with `color`, keeping the shape's own alpha.
///
/// This is what makes a shadow a SHADOW rather than a copy: the spec derives it
/// from the alpha channel of the drawing alone, so a multicoloured shape casts
/// a single-coloured one.
pub fn tint_to(pixmap: &mut Pixmap, color: Color) {
    let alpha_scale = color.a as u32;
    for px in pixmap.pixels_mut().iter_mut() {
        let a = (px.alpha() as u32 * alpha_scale / 255).min(255);
        if a == 0 {
            *px = PremultipliedColorU8::from_rgba(0, 0, 0, 0).expect("transparent is valid");
            continue;
        }
        // Premultiplied by the combined alpha, so the channels stay <= alpha.
        let r = (color.r as u32 * a / 255) as u8;
        let g = (color.g as u32 * a / 255) as u8;
        let b = (color.b as u32 * a / 255) as u8;
        if let Some(p) = PremultipliedColorU8::from_rgba(r, g, b, a as u8) {
            *px = p;
        }
    }
}

/// Apply a parsed CSS filter list to a pixmap, in order.
///
/// Order matters and is the author's: `blur(2px) brightness(2)` is not
/// `brightness(2) blur(2px)`, because the second brightens what the blur
/// already averaged.
pub fn apply_filter_list(pixmap: &mut Pixmap, filters: &CssFilters) {
    for op in &filters.ops {
        apply_filter_op(pixmap, op);
    }
}

/// One filter function.
///
/// The colour-matrix cases delegate to the renderer's existing implementation
/// rather than restating it — there is one set of CSS filter maths in this
/// crate and this is not a second copy of it. Blur and drop-shadow are the two
/// the renderer could not do, and they are done here.
pub fn apply_filter_op(pixmap: &mut Pixmap, op: &FilterOp) {
    use crate::renderer::display_list_replay::apply_pixel_filter;
    match op {
        // CSS `blur(r)` names the standard deviation directly, unlike
        // `shadowBlur`, which names twice it.
        FilterOp::Blur(radius) => blur_pixmap(pixmap, *radius),
        FilterOp::Brightness(v) => apply_pixel_filter(pixmap, 1, *v),
        FilterOp::Contrast(v) => apply_pixel_filter(pixmap, 2, *v),
        FilterOp::Grayscale(v) => apply_pixel_filter(pixmap, 3, *v),
        FilterOp::HueRotate(v) => apply_pixel_filter(pixmap, 4, *v),
        FilterOp::Invert(v) => apply_pixel_filter(pixmap, 5, *v),
        FilterOp::Opacity(v) => apply_pixel_filter(pixmap, 6, *v),
        FilterOp::Saturate(v) => apply_pixel_filter(pixmap, 7, *v),
        FilterOp::Sepia(v) => apply_pixel_filter(pixmap, 8, *v),
        FilterOp::DropShadow {
            dx,
            dy,
            blur,
            color,
        } => drop_shadow(pixmap, *dx, *dy, *blur, *color),
    }
}

/// `drop-shadow(dx dy blur color)` — a shadow of the pixmap, drawn beneath it.
///
/// Unlike `shadowBlur`, CSS names the standard deviation directly here, so the
/// radius is passed through rather than halved.
pub(crate) fn drop_shadow(pixmap: &mut Pixmap, dx: f32, dy: f32, blur: f32, color: CssColor) {
    let Some(shadow) = shadow_layer(
        pixmap,
        Color::rgba(color.r, color.g, color.b, color.a),
        blur,
    ) else {
        return;
    };
    let Some(mut out) = Pixmap::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    let paint = tiny_skia::PixmapPaint::default();
    out.draw_pixmap(
        dx.round() as i32,
        dy.round() as i32,
        shadow.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    out.draw_pixmap(
        0,
        0,
        pixmap.as_ref(),
        &paint,
        tiny_skia::Transform::identity(),
        None,
    );
    *pixmap = out;
}

/// The shadow cast by `source`: its alpha, tinted and blurred.
///
/// `std_dev` is already a standard deviation — `shadowBlur` is TWICE this, and
/// halving it is the caller's job, because CSS `drop-shadow()` and canvas
/// `shadowBlur` disagree about which of the two their argument names.
pub fn shadow_layer(source: &Pixmap, color: Color, std_dev: f32) -> Option<Pixmap> {
    let mut layer = source.to_owned();
    tint_to(&mut layer, color);
    blur_pixmap(&mut layer, std_dev);
    Some(layer)
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
    fn a_blur_spreads_alpha_outside_the_original_shape() {
        let mut p = opaque_square();
        assert_eq!(p.pixel(5, 20).expect("in bounds").alpha(), 0, "clear first");
        blur_pixmap(&mut p, 4.0);
        assert!(
            p.pixel(10, 20).expect("in bounds").alpha() > 0,
            "alpha reached outside the square"
        );
        assert!(
            p.pixel(20, 20).expect("in bounds").alpha() < 255,
            "and the middle softened"
        );
    }

    #[test]
    fn a_blur_conserves_roughly_the_total_alpha() {
        // A blur redistributes coverage, it does not create or destroy it.
        // Getting this wrong is how a blurred shadow comes out too faint.
        let before: u32 = opaque_square()
            .pixels()
            .iter()
            .map(|px| px.alpha() as u32)
            .sum();
        let mut p = opaque_square();
        blur_pixmap(&mut p, 3.0);
        let after: u32 = p.pixels().iter().map(|px| px.alpha() as u32).sum();
        let drift = (before as f32 - after as f32).abs() / before as f32;
        assert!(drift < 0.15, "before {before}, after {after}");
    }

    #[test]
    fn a_zero_blur_changes_nothing() {
        let mut p = opaque_square();
        let before: Vec<u8> = p.data().to_vec();
        blur_pixmap(&mut p, 0.0);
        assert_eq!(p.data(), before.as_slice());
    }

    #[test]
    fn cropped_blur_matches_full_tile_at_edges_and_center() {
        for (x, y) in [(0.0, 0.0), (61.0, 61.0), (119.0, 119.0)] {
            let mut cropped = Pixmap::new(128, 128).unwrap();
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(tiny_skia::Color::from_rgba8(120, 40, 220, 200));
            cropped.fill_rect(
                tiny_skia::Rect::from_xywh(x, y, 8.0, 8.0).unwrap(),
                &paint,
                tiny_skia::Transform::identity(),
                None,
            );
            let mut full = cropped.clone();
            for std_dev in [1.0, 3.0, 8.0] {
                let radius = (((std_dev * 3.0 * (2.0 * std::f32::consts::PI).sqrt() / 4.0
                    + 0.5)
                    .floor() as i32)
                    / 2)
                    .max(1);
                blur_pixmap_full(&mut full, radius);
                blur_pixmap(&mut cropped, std_dev);
                assert_eq!(cropped.data(), full.data(), "x={x}, y={y}, blur={std_dev}");
            }
        }
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
