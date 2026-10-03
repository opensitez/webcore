//! Display list replay — rasterize paint commands to a pixmap.
//!
//! This replays a DisplayList (built by display_list_builder) to a
//! tiny_skia Pixmap, handling fills, borders, text, images, clips,
//! opacity, and transforms.

use super::display_list::{DisplayList, ImageRef, PaintCmd};
use crate::types::{
    Color, GradientDirection, Rect, reduce_shape_box_radii as reduce_corner_radii_xy,
};
use cosmic_text::{
    Attrs, Buffer, Color as CTextColor, FontSystem, Metrics, Shaping, Style as CTextStyle,
    SwashCache, Weight as CTextWeight,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::{
    Color as SkColor, FillRule, Paint, PathBuilder, Pixmap, Point, Rect as SkRect, Transform,
};

/// Replay a display list onto a pixmap (no text — use replay_with_text for full rendering).
pub fn replay(list: &DisplayList, pixmap: &mut Pixmap, scale: f32) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
}

#[inline]
pub(super) fn rgba_is_opaque(rgba: &[u8]) -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        return unsafe { rgba_is_opaque_neon(rgba) };
    }
    #[cfg(not(target_arch = "aarch64"))]
    rgba.chunks_exact(4).all(|pixel| pixel[3] == 255)
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn rgba_is_opaque_neon(rgba: &[u8]) -> bool {
    use std::arch::aarch64::*;
    let mut offset = 0;
    while offset + 64 <= rgba.len() {
        let channels = unsafe { vld4q_u8(rgba.as_ptr().add(offset)) };
        if vminvq_u8(channels.3) != 255 {
            return false;
        }
        offset += 64;
    }
    rgba[offset..].chunks_exact(4).all(|pixel| pixel[3] == 255)
}

#[test]
fn rgba_opacity_check_covers_vector_and_tail_pixels() {
    for count in [1, 15, 16, 17, 31, 32, 33] {
        let mut pixels = vec![255; count * 4];
        assert!(rgba_is_opaque(&pixels));
        for index in [0, count / 2, count - 1] {
            pixels[index * 4 + 3] = 254;
            assert!(!rgba_is_opaque(&pixels));
            pixels[index * 4 + 3] = 255;
        }
    }
}

#[test]
fn opaque_aligned_fill_matches_tiny_skia_with_binary_clip() {
    let rect = Rect::new(2.0, 3.0, 20.0, 13.0);
    let color = Color::rgba(17, 87, 153, 255);
    let transform = Transform::from_scale(2.0, 2.0);
    let mut mask = tiny_skia::Mask::new(48, 40).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(8.0, 8.0, 29.0, 24.0).unwrap()),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    for clip in [None, Some(&mask)] {
        let mut expected = Pixmap::new(48, 40).unwrap();
        let mut actual = Pixmap::new(48, 40).unwrap();
        expected.fill(tiny_skia::Color::from_rgba8(40, 30, 20, 255));
        actual.fill(tiny_skia::Color::from_rgba8(40, 30, 20, 255));
        let mut paint = Paint::default();
        paint.set_color(to_sk_color(&color));
        expected.fill_rect(
            SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h).unwrap(),
            &paint,
            transform,
            clip,
        );
        assert!(fill_opaque_aligned_rect(
            &mut actual,
            rect,
            color,
            transform,
            clip,
        ));
        assert_eq!(actual.data(), expected.data());
    }
    assert!(!fill_opaque_aligned_rect(
        &mut Pixmap::new(48, 40).unwrap(),
        rect,
        color,
        Transform::from_translate(0.5, 0.0),
        None,
    ));
}

#[test]
fn opaque_rounded_band_fill_matches_tiny_skia() {
    let rect = Rect::new(2.0, 3.0, 20.0, 13.0);
    let color = Color::rgba(17, 87, 153, 255);
    let radius = [4.0; 4];
    let transform = Transform::from_scale(2.0, 2.0);
    let mut mask = tiny_skia::Mask::new(48, 40).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(8.0, 8.0, 29.0, 24.0).unwrap()),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    let path =
        rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, radius, radius).unwrap();
    for clip in [None, Some(&mask)] {
        let mut expected = Pixmap::new(48, 40).unwrap();
        let mut actual = Pixmap::new(48, 40).unwrap();
        expected.fill(tiny_skia::Color::from_rgba8(40, 30, 20, 255));
        actual.fill(tiny_skia::Color::from_rgba8(40, 30, 20, 255));
        let mut paint = Paint::default();
        paint.set_color(to_sk_color(&color));
        expected.fill_path(&path, &paint, FillRule::Winding, transform, clip);
        assert!(fill_opaque_rounded_rect_bands(
            &mut actual,
            rect,
            color,
            radius,
            radius,
            transform,
            clip,
        ));
        assert_eq!(actual.data(), expected.data());
    }
}

pub(super) fn blit_opaque_unscaled_image(
    target: &mut Pixmap,
    rgba: &[u8],
    width: u32,
    height: u32,
    transform: Transform,
) -> bool {
    if transform.sx != 1.0
        || transform.sy != 1.0
        || transform.kx != 0.0
        || transform.ky != 0.0
        || !transform.tx.is_finite()
        || !transform.ty.is_finite()
        || transform.tx.fract() != 0.0
        || transform.ty.fract() != 0.0
        || !rgba_is_opaque(rgba)
    {
        return false;
    }
    let left = transform.tx as i64;
    let top = transform.ty as i64;
    let x0 = left.max(0);
    let y0 = top.max(0);
    let x1 = left
        .saturating_add(i64::from(width))
        .min(i64::from(target.width()));
    let y1 = top
        .saturating_add(i64::from(height))
        .min(i64::from(target.height()));
    if x0 >= x1 || y0 >= y1 {
        return true;
    }
    let copy_bytes = (x1 - x0) as usize * 4;
    let source_stride = width as usize * 4;
    let target_stride = target.width() as usize * 4;
    let source_x = (x0 - left) as usize * 4;
    let target_x = x0 as usize * 4;
    let target_bytes = target.data_mut();
    for y in y0..y1 {
        let source = (y - top) as usize * source_stride + source_x;
        let destination = y as usize * target_stride + target_x;
        target_bytes[destination..destination + copy_bytes]
            .copy_from_slice(&rgba[source..source + copy_bytes]);
    }
    true
}

fn fill_opaque_aligned_rect(
    target: &mut Pixmap,
    rect: Rect,
    color: Color,
    transform: Transform,
    mask: Option<&tiny_skia::Mask>,
) -> bool {
    if color.a != 255
        || transform.kx != 0.0
        || transform.ky != 0.0
        || transform.sx <= 0.0
        || transform.sy <= 0.0
    {
        return false;
    }
    let aligned = |value: f32| {
        let rounded = value.round();
        (value.is_finite() && (value - rounded).abs() < 0.0001).then_some(rounded as i64)
    };
    let Some(left) = aligned(rect.x * transform.sx + transform.tx) else {
        return false;
    };
    let Some(top) = aligned(rect.y * transform.sy + transform.ty) else {
        return false;
    };
    let Some(right) = aligned(rect.right() * transform.sx + transform.tx) else {
        return false;
    };
    let Some(bottom) = aligned(rect.bottom() * transform.sy + transform.ty) else {
        return false;
    };
    let tw = target.width() as usize;
    let th = target.height() as usize;
    let x0 = left.clamp(0, tw as i64) as usize;
    let y0 = top.clamp(0, th as i64) as usize;
    let x1 = right.clamp(0, tw as i64) as usize;
    let y1 = bottom.clamp(0, th as i64) as usize;
    if x0 >= x1 || y0 >= y1 {
        return true;
    }
    let coverage = mask.map(tiny_skia::Mask::data);
    if coverage.is_some_and(|data| data.len() != tw * th) {
        return false;
    }
    let mut full_rows = Vec::new();
    if let Some(coverage) = coverage {
        full_rows.reserve(y1 - y0);
        for y in y0..y1 {
            let row = &coverage[y * tw + x0..y * tw + x1];
            let full = row.iter().all(|&value| value == 255);
            if !full && row.iter().any(|&value| value != 0 && value != 255) {
                return false;
            }
            full_rows.push(full);
        }
    }
    let pixel = tiny_skia::PremultipliedColorU8::from_rgba(color.r, color.g, color.b, 255)
        .expect("opaque color");
    let pixels = target.pixels_mut();
    for y in y0..y1 {
        let row_start = y * tw;
        if coverage.is_none() || full_rows[y - y0] {
            pixels[row_start + x0..row_start + x1].fill(pixel);
        } else if let Some(coverage) = coverage {
            let mut x = x0;
            while x < x1 {
                while x < x1 && coverage[row_start + x] == 0 {
                    x += 1;
                }
                let start = x;
                while x < x1 && coverage[row_start + x] == 255 {
                    x += 1;
                }
                pixels[row_start + start..row_start + x].fill(pixel);
            }
        }
    }
    true
}

fn fill_opaque_rounded_rect_bands(
    target: &mut Pixmap,
    rect: Rect,
    color: Color,
    radius: [f32; 4],
    radius_y: [f32; 4],
    transform: Transform,
    mask: Option<&tiny_skia::Mask>,
) -> bool {
    if color.a != 255
        || transform.kx != 0.0
        || transform.ky != 0.0
        || transform.sx <= 0.0
        || transform.sy <= 0.0
    {
        return false;
    }
    let top = rect.y * transform.sy + transform.ty;
    let bottom = rect.bottom() * transform.sy + transform.ty;
    let left = rect.x * transform.sx + transform.tx;
    let right = rect.right() * transform.sx + transform.tx;
    if [top, bottom, left, right]
        .iter()
        .any(|v| !v.is_finite() || (v.round() - v).abs() > 0.0001)
    {
        return false;
    }
    let band = (radius_y.iter().copied().fold(0.0_f32, f32::max) * transform.sy).ceil() as i64;
    let top = top.round() as i64;
    let bottom = bottom.round() as i64;
    if band <= 0 || top + band >= bottom - band {
        return false;
    }
    let center_top = top + band;
    let center_bottom = bottom - band;
    let center = Rect::new(
        rect.x,
        (center_top as f32 - transform.ty) / transform.sy,
        rect.w,
        (center_bottom - center_top) as f32 / transform.sy,
    );
    // The fast interior path requires a binary clip; keep antialiased clips on tiny-skia.
    let Some(path) = rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, radius, radius_y)
    else {
        return false;
    };
    let tw = target.width() as usize;
    let th = target.height() as usize;
    if let Some(clip) = mask {
        if clip.data().len() != tw * th {
            return false;
        }
    }
    if !fill_opaque_aligned_rect(target, center, color, transform, mask) {
        return false;
    }
    let mut paint = Paint::default();
    paint.set_color(to_sk_color(&color));
    for (start, end) in [(top, center_top), (center_bottom, bottom)] {
        let start = start.clamp(0, th as i64) as usize;
        let end = end.clamp(0, th as i64) as usize;
        if start >= end {
            continue;
        }
        let byte_start = start * tw * 4;
        let byte_end = end * tw * 4;
        let Some(mut band_target) = tiny_skia::PixmapMut::from_bytes(
            &mut target.data_mut()[byte_start..byte_end],
            tw as u32,
            (end - start) as u32,
        ) else {
            continue;
        };
        let band_mask = mask.and_then(|clip| {
            let mut cropped = tiny_skia::Mask::new(tw as u32, (end - start) as u32)?;
            cropped
                .data_mut()
                .copy_from_slice(&clip.data()[start * tw..end * tw]);
            Some(cropped)
        });
        let mut local = transform;
        local.ty -= start as f32;
        band_target.fill_path(&path, &paint, FillRule::Winding, local, band_mask.as_ref());
    }
    true
}

fn blit_opaque_scaled_image(
    target: &mut Pixmap,
    rgba: &[u8],
    width: u32,
    height: u32,
    transform: Transform,
    mask: &tiny_skia::Mask,
) -> bool {
    blit_opaque_scaled_image_impl::<true, true, true>(target, rgba, width, height, transform, mask)
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn bilinear_opaque_eight_neon(
    upper: &[[u16; 3]],
    lower: &[[u16; 3]],
    weight: u16,
    destination: &mut [u8],
) {
    use std::arch::aarch64::*;

    #[inline]
    unsafe fn blend(upper: uint16x8_t, lower: uint16x8_t, weight: u16) -> uint8x8_t {
        unsafe {
            let inverse = 256 - weight;
            let low = vshrq_n_u32::<16>(vaddq_u32(
                vmlal_n_u16(
                    vmull_n_u16(vget_low_u16(upper), inverse),
                    vget_low_u16(lower),
                    weight,
                ),
                vdupq_n_u32(32768),
            ));
            let high = vshrq_n_u32::<16>(vaddq_u32(
                vmlal_n_u16(
                    vmull_n_u16(vget_high_u16(upper), inverse),
                    vget_high_u16(lower),
                    weight,
                ),
                vdupq_n_u32(32768),
            ));
            vqmovn_u16(vcombine_u16(vmovn_u32(low), vmovn_u32(high)))
        }
    }

    let upper = unsafe { vld3q_u16(upper.as_ptr().cast::<u16>()) };
    let lower = unsafe { vld3q_u16(lower.as_ptr().cast::<u16>()) };
    let red = unsafe { blend(upper.0, lower.0, weight) };
    let green = unsafe { blend(upper.1, lower.1, weight) };
    let blue = unsafe { blend(upper.2, lower.2, weight) };
    let channels = uint8x8x4_t(red, green, blue, vdup_n_u8(255));
    unsafe { vst4_u8(destination.as_mut_ptr(), channels) };
}

fn blit_opaque_scaled_image_impl<
    const CACHE_ROWS: bool,
    const PARALLEL: bool,
    const VECTOR: bool,
>(
    target: &mut Pixmap,
    rgba: &[u8],
    width: u32,
    height: u32,
    transform: Transform,
    mask: &tiny_skia::Mask,
) -> bool {
    if transform.kx != 0.0
        || transform.ky != 0.0
        || !transform.sx.is_finite()
        || !transform.sy.is_finite()
        || transform.sx <= 0.0
        || transform.sy <= 0.0
        || !transform.tx.is_finite()
        || !transform.ty.is_finite()
        || width < 2
        || height < 2
        || !rgba_is_opaque(rgba)
    {
        return false;
    }
    let tw = target.width() as usize;
    let th = target.height() as usize;
    let x0 = (transform.tx.ceil() as i64).clamp(0, tw as i64) as usize;
    let y0 = (transform.ty.ceil() as i64).clamp(0, th as i64) as usize;
    let x1 =
        ((transform.tx + width as f32 * transform.sx).ceil() as i64).clamp(0, tw as i64) as usize;
    let y1 =
        ((transform.ty + height as f32 * transform.sy).ceil() as i64).clamp(0, th as i64) as usize;
    if x0 >= x1 || y0 >= y1 {
        return true;
    }
    let sample = |pixel: usize, origin: f32, scale: f32, limit: u32| {
        let position = (pixel as f32 + 0.5 - origin) / scale - 0.5;
        let base = position.floor().clamp(0.0, (limit - 2) as f32) as usize;
        let fraction = (position - base as f32).clamp(0.0, 1.0);
        (base, (fraction * 256.0).round() as u32)
    };
    let columns: Vec<_> = (x0..x1)
        .map(|x| sample(x, transform.tx, transform.sx, width))
        .collect();
    let coverage = mask.data();
    let pixels = target.data_mut();
    let source_stride = width as usize * 4;
    let interpolate_row = |source_y: usize, output: &mut [[u16; 3]]| {
        let source_row = source_y * source_stride;
        for (column, &(sx, wx)) in columns.iter().enumerate() {
            let source = source_row + sx * 4;
            for channel in 0..3 {
                output[column][channel] = (u32::from(rgba[source + channel]) * (256 - wx)
                    + u32::from(rgba[source + 4 + channel]) * wx)
                    as u16;
            }
        }
    };
    let paint_rows = |first_y: usize, row_pixels: &mut [u8]| {
        let mut upper_row = vec![[0u16; 3]; columns.len()];
        let mut lower_row = vec![[0u16; 3]; columns.len()];
        let mut cached_source_y = None;
        let last_y = first_y + row_pixels.len() / (tw * 4);
        for y in y0.max(first_y)..y1.min(last_y) {
            let (sy, wy) = sample(y, transform.ty, transform.sy, height);
            if CACHE_ROWS && cached_source_y != Some(sy) {
                if cached_source_y.is_some_and(|previous| previous + 1 == sy) {
                    std::mem::swap(&mut upper_row, &mut lower_row);
                } else {
                    interpolate_row(sy, &mut upper_row);
                }
                interpolate_row(sy + 1, &mut lower_row);
                cached_source_y = Some(sy);
            }
            let mut column = 0;
            while column < columns.len() {
                let x = x0 + column;
                let index = y * tw + x;
                #[cfg(target_arch = "aarch64")]
                if VECTOR
                    && CACHE_ROWS
                    && column + 8 <= columns.len()
                    && coverage[index..index + 8] == [255; 8]
                {
                    let destination = ((y - first_y) * tw + x) * 4;
                    unsafe {
                        bilinear_opaque_eight_neon(
                            &upper_row[column..],
                            &lower_row[column..],
                            wy as u16,
                            &mut row_pixels[destination..destination + 32],
                        );
                    }
                    column += 8;
                    continue;
                }
                let alpha = u32::from(coverage[index]);
                if alpha == 0 {
                    column += 1;
                    continue;
                }
                let destination = ((y - first_y) * tw + x) * 4;
                for channel in 0..3 {
                    let (upper, lower) = if CACHE_ROWS {
                        (
                            u32::from(upper_row[column][channel]),
                            u32::from(lower_row[column][channel]),
                        )
                    } else {
                        let (sx, wx) = columns[column];
                        let top = sy * source_stride + sx * 4;
                        let bottom = top + source_stride;
                        (
                            (u32::from(rgba[top + channel]) * (256 - wx)
                                + u32::from(rgba[top + 4 + channel]) * wx
                                + 128)
                                >> 8,
                            (u32::from(rgba[bottom + channel]) * (256 - wx)
                                + u32::from(rgba[bottom + 4 + channel]) * wx
                                + 128)
                                >> 8,
                        )
                    };
                    let value = if CACHE_ROWS {
                        (upper * (256 - wy) + lower * wy + 32768) >> 16
                    } else {
                        (upper * (256 - wy) + lower * wy + 128) >> 8
                    };
                    row_pixels[destination + channel] = if alpha == 255 {
                        value as u8
                    } else {
                        ((value * alpha
                            + u32::from(row_pixels[destination + channel]) * (255 - alpha)
                            + 127)
                            / 255) as u8
                    };
                }
                row_pixels[destination + 3] = if alpha == 255 {
                    255
                } else {
                    (alpha + u32::from(row_pixels[destination + 3]) * (255 - alpha) / 255) as u8
                };
                column += 1;
            }
        }
    };
    if PARALLEL && (x1 - x0) * (y1 - y0) >= 1_000_000 && rayon::current_num_threads() > 1 {
        pixels
            .par_chunks_mut(tw * 4 * 128)
            .enumerate()
            .for_each(|(chunk, rows)| paint_rows(chunk * 128, rows));
    } else {
        paint_rows(0, pixels);
    }
    true
}

#[test]
fn opaque_blit_matches_pixmap_paint_and_rejects_other_cases() {
    let rgba: Vec<u8> = (0..16)
        .flat_map(|index| [index as u8 * 13, index as u8 * 7, index as u8 * 3, 255])
        .collect();
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, 4, 4).unwrap();
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    for (x, y) in [(0, 0), (2, 3), (-2, -1), (7, 7), (-8, 0)] {
        let transform = Transform::from_translate(x as f32, y as f32);
        let mut expected = Pixmap::new(8, 8).unwrap();
        let mut actual = Pixmap::new(8, 8).unwrap();
        expected.draw_pixmap(0, 0, source, &paint, transform, None);
        assert!(blit_opaque_unscaled_image(
            &mut actual,
            &rgba,
            4,
            4,
            transform
        ));
        assert_eq!(actual.data(), expected.data(), "offset ({x}, {y})");
    }
    let mut target = Pixmap::new(8, 8).unwrap();
    let mut transparent = rgba.clone();
    transparent[3] = 128;
    assert!(!blit_opaque_unscaled_image(
        &mut target,
        &transparent,
        4,
        4,
        Transform::identity(),
    ));
    assert!(!blit_opaque_unscaled_image(
        &mut target,
        &rgba,
        4,
        4,
        Transform::from_scale(2.0, 2.0),
    ));
}

#[test]
fn opaque_scaled_blit_tracks_bilinear_clip() {
    let rgba: Vec<u8> = (0..32 * 24)
        .flat_map(|index| {
            let index = index as u32;
            [
                (index * 13) as u8,
                (index * 7) as u8,
                (index * 3) as u8,
                255,
            ]
        })
        .collect();
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, 32, 24).unwrap();
    let mut mask = tiny_skia::Mask::new(32, 24).unwrap();
    mask.fill_path(
        &PathBuilder::from_circle(16.0, 12.0, 10.0).unwrap(),
        FillRule::Winding,
        true,
        Transform::identity(),
    );
    let transform = Transform::from_translate(-3.5, -2.0).pre_scale(1.25, 1.25);
    let mut expected = Pixmap::new(32, 24).unwrap();
    let mut actual = Pixmap::new(32, 24).unwrap();
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    expected.draw_pixmap(0, 0, source, &paint, transform, Some(&mask));
    assert!(blit_opaque_scaled_image(
        &mut actual,
        &rgba,
        32,
        24,
        transform,
        &mask
    ));
    let mut max_error = 0u8;
    for (actual, expected) in actual.data().iter().zip(expected.data()) {
        max_error = max_error.max(actual.abs_diff(*expected));
    }
    assert!(max_error <= 2, "maximum channel error {max_error}");
}

#[test]
fn opaque_scaled_blit_matches_video_edges_on_background() {
    let rgba: Vec<u8> = (0..32 * 24)
        .flat_map(|index| {
            let index = index as u32;
            [
                (index * 13) as u8,
                (index * 7) as u8,
                (index * 3) as u8,
                255,
            ]
        })
        .collect();
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, 32, 24).unwrap();
    let mut mask = tiny_skia::Mask::new(32, 29).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(0.0, 0.0, 32.0, 29.0).unwrap()),
        FillRule::Winding,
        true,
        Transform::identity(),
    );
    let transform = Transform::from_translate(-3.0, 0.0).pre_scale(1.1875, 1.1875);
    let mut expected = Pixmap::new(32, 29).unwrap();
    let mut actual = Pixmap::new(32, 29).unwrap();
    expected.fill(tiny_skia::Color::from_rgba8(20, 30, 40, 255));
    actual.fill(tiny_skia::Color::from_rgba8(20, 30, 40, 255));
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    expected.draw_pixmap(0, 0, source, &paint, transform, Some(&mask));
    assert!(blit_opaque_scaled_image(
        &mut actual,
        &rgba,
        32,
        24,
        transform,
        &mask
    ));
    let mut max_error = 0u8;
    let mut worst = 0usize;
    for (index, (actual, expected)) in actual.data().iter().zip(expected.data()).enumerate() {
        let error = actual.abs_diff(*expected);
        if error > max_error {
            max_error = error;
            worst = index;
        }
    }
    assert!(
        max_error <= 2,
        "maximum channel error {max_error} at byte {worst}"
    );
}

#[test]
fn opaque_scaled_blit_avoids_double_rounding() {
    let (width, height) = (19u32, 13u32);
    let rgba: Vec<u8> = (0..width * height)
        .flat_map(|index| {
            [
                (index * 37) as u8,
                (index * 83) as u8,
                (index * 149) as u8,
                255,
            ]
        })
        .collect();
    let mut mask = tiny_skia::Mask::new(37, 27).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(0.0, 0.0, 37.0, 27.0).unwrap()),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    let transform = Transform::from_scale(1.7, 1.7);
    let mut old = Pixmap::new(37, 27).unwrap();
    let mut precise = Pixmap::new(37, 27).unwrap();
    assert!(blit_opaque_scaled_image_impl::<false, false, true>(
        &mut old, &rgba, width, height, transform, &mask
    ));
    assert!(blit_opaque_scaled_image_impl::<true, false, true>(
        &mut precise,
        &rgba,
        width,
        height,
        transform,
        &mask
    ));
    let mut old_error = 0u64;
    let mut precise_error = 0u64;
    for y in 0..(height as f64 * 1.7).ceil() as usize {
        for x in 0..(width as f64 * 1.7).ceil() as usize {
            let sx = ((x as f64 + 0.5) / 1.7 - 0.5).clamp(0.0, width as f64 - 1.0);
            let sy = ((y as f64 + 0.5) / 1.7 - 0.5).clamp(0.0, height as f64 - 1.0);
            let x0 = sx.floor() as usize;
            let y0 = sy.floor() as usize;
            let x1 = (x0 + 1).min(width as usize - 1);
            let y1 = (y0 + 1).min(height as usize - 1);
            let fx = sx - x0 as f64;
            let fy = sy - y0 as f64;
            for channel in 0..3 {
                let at =
                    |cx: usize, cy: usize| rgba[(cy * width as usize + cx) * 4 + channel] as f64;
                let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
                let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
                let expected = (top * (1.0 - fy) + bottom * fy).round() as u8;
                let pixel = (y * 37 + x) * 4 + channel;
                old_error += u64::from(old.data()[pixel].abs_diff(expected));
                precise_error += u64::from(precise.data()[pixel].abs_diff(expected));
            }
        }
    }
    assert!(
        precise_error < old_error,
        "precise={precise_error} old={old_error}"
    );
}

#[test]
fn opaque_scaled_parallel_and_vector_blit_match_scalar_with_clip() {
    let (width, height) = (960u32, 540u32);
    let rgba: Vec<u8> = (0..width * height)
        .flat_map(|index| {
            [
                (index * 13) as u8,
                (index * 7) as u8,
                (index * 3) as u8,
                255,
            ]
        })
        .collect();
    let mut mask = tiny_skia::Mask::new(1440, 810).unwrap();
    mask.fill_path(
        &PathBuilder::from_circle(720.0, 405.0, 380.0).unwrap(),
        FillRule::Winding,
        true,
        Transform::identity(),
    );
    let transform = Transform::from_scale(1.5, 1.5);
    let mut scalar = Pixmap::new(1440, 810).unwrap();
    let mut serial = Pixmap::new(1440, 810).unwrap();
    let mut parallel = Pixmap::new(1440, 810).unwrap();
    scalar.fill(tiny_skia::Color::from_rgba8(20, 30, 40, 255));
    serial.fill(tiny_skia::Color::from_rgba8(20, 30, 40, 255));
    parallel.fill(tiny_skia::Color::from_rgba8(20, 30, 40, 255));
    assert!(blit_opaque_scaled_image_impl::<true, false, false>(
        &mut scalar,
        &rgba,
        width,
        height,
        transform,
        &mask,
    ));
    assert!(blit_opaque_scaled_image_impl::<true, false, true>(
        &mut serial,
        &rgba,
        width,
        height,
        transform,
        &mask,
    ));
    assert!(blit_opaque_scaled_image_impl::<true, true, true>(
        &mut parallel,
        &rgba,
        width,
        height,
        transform,
        &mask,
    ));
    assert_eq!(serial.data(), scalar.data());
    assert_eq!(parallel.data(), scalar.data());
}

#[test]
#[ignore = "run explicitly when measuring 720p video paint"]
fn benchmark_video_sized_image_paint() {
    let (width, height) = (1280u32, 720u32);
    let rgba = vec![255u8; (width * height * 4) as usize];
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, width, height).unwrap();
    let mut target = Pixmap::new(width, height).unwrap();
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    let start = std::time::Instant::now();
    for _ in 0..40 {
        target.draw_pixmap(0, 0, source, &paint, Transform::identity(), None);
        std::hint::black_box(&target);
    }
    let draw_time = start.elapsed();
    let start = std::time::Instant::now();
    for _ in 0..40 {
        assert!(blit_opaque_unscaled_image(
            &mut target,
            &rgba,
            width,
            height,
            Transform::identity(),
        ));
        std::hint::black_box(&target);
    }
    eprintln!(
        "720p paint x40: tiny-skia {draw_time:?}, opaque blit {:?}",
        start.elapsed()
    );
}

#[test]
#[ignore = "run explicitly when measuring scaled video paint"]
fn benchmark_scaled_video_paint() {
    let (width, height) = (1280u32, 720u32);
    let rgba = vec![255u8; (width * height * 4) as usize];
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, width, height).unwrap();
    let mut target = Pixmap::new(1280, 855).unwrap();
    let mut mask = tiny_skia::Mask::new(1280, 855).unwrap();
    mask.fill_path(
        &PathBuilder::from_circle(640.0, 427.0, 425.0).unwrap(),
        FillRule::Winding,
        true,
        Transform::identity(),
    );
    let transform = Transform::from_translate(-120.0, 0.0).pre_scale(1.1875, 1.1875);
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    let start = std::time::Instant::now();
    for _ in 0..20 {
        target.draw_pixmap(0, 0, source, &paint, transform, Some(&mask));
        std::hint::black_box(&target);
    }
    let baseline = start.elapsed();
    for cache_rows in [false, true, true, false] {
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let painted = if cache_rows {
                blit_opaque_scaled_image_impl::<true, false, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                )
            } else {
                blit_opaque_scaled_image_impl::<false, false, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                )
            };
            assert!(painted);
            std::hint::black_box(&target);
        }
        eprintln!(
            "scaled 720p x20 cached rows={cache_rows}: {:?}",
            start.elapsed()
        );
    }
    eprintln!("scaled 720p x20 tiny-skia: {baseline:?}");
    for parallel in [false, true, true, false] {
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let painted = if parallel {
                blit_opaque_scaled_image_impl::<true, true, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                )
            } else {
                blit_opaque_scaled_image_impl::<true, false, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                )
            };
            assert!(painted);
            std::hint::black_box(&target);
        }
        eprintln!("scaled 720p x20 parallel={parallel}: {:?}", start.elapsed());
    }
}

#[test]
#[ignore = "run explicitly when measuring fullscreen video paint"]
fn benchmark_fullscreen_scaled_video_paint() {
    let (width, height) = (1280u32, 720u32);
    let rgba: Vec<u8> = (0..width * height)
        .flat_map(|index| {
            [
                (index * 13) as u8,
                (index * 7) as u8,
                (index * 3) as u8,
                255,
            ]
        })
        .collect();
    let mut target = Pixmap::new(2560, 1440).unwrap();
    let mut mask = tiny_skia::Mask::new(2560, 1440).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(0.0, 0.0, 2560.0, 1440.0).unwrap()),
        FillRule::Winding,
        false,
        Transform::identity(),
    );
    let transform = Transform::from_scale(2.0, 2.0);
    for (parallel, vector) in [
        (false, false),
        (false, true),
        (true, false),
        (true, true),
        (true, true),
        (true, false),
        (false, true),
        (false, false),
    ] {
        let start = std::time::Instant::now();
        for _ in 0..12 {
            let painted = match (parallel, vector) {
                (false, false) => blit_opaque_scaled_image_impl::<true, false, false>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                ),
                (false, true) => blit_opaque_scaled_image_impl::<true, false, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                ),
                (true, false) => blit_opaque_scaled_image_impl::<true, true, false>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                ),
                (true, true) => blit_opaque_scaled_image_impl::<true, true, true>(
                    &mut target,
                    &rgba,
                    width,
                    height,
                    transform,
                    &mask,
                ),
            };
            assert!(painted);
            std::hint::black_box(&target);
        }
        eprintln!(
            "fullscreen scaled video x12 parallel={parallel} vector={vector}: {:?}",
            start.elapsed()
        );
    }
}

#[test]
#[ignore = "run explicitly when measuring tiled video paint"]
fn benchmark_scaled_video_tiles() {
    let (width, height) = (1280u32, 720u32);
    let rgba = vec![255u8; (width * height * 4) as usize];
    let source = tiny_skia::PixmapRef::from_bytes(&rgba, width, height).unwrap();
    let mut target = Pixmap::new(512, 512).unwrap();
    let mut mask = tiny_skia::Mask::new(512, 512).unwrap();
    mask.fill_path(
        &PathBuilder::from_rect(SkRect::from_xywh(0.0, 0.0, 512.0, 512.0).unwrap()),
        FillRule::Winding,
        true,
        Transform::identity(),
    );
    let paint = tiny_skia::PixmapPaint {
        quality: tiny_skia::FilterQuality::Bilinear,
        ..tiny_skia::PixmapPaint::default()
    };
    let transforms: Vec<_> = (0..2)
        .flat_map(|row| {
            (0..3).map(move |col| {
                Transform::from_translate(-120.0 - col as f32 * 512.0, -(row as f32) * 512.0)
                    .pre_scale(1.1875, 1.1875)
            })
        })
        .collect();
    let start = std::time::Instant::now();
    for _ in 0..20 {
        for transform in &transforms {
            target.draw_pixmap(0, 0, source, &paint, *transform, Some(&mask));
            std::hint::black_box(&target);
        }
    }
    let baseline = start.elapsed();
    let start = std::time::Instant::now();
    for _ in 0..20 {
        for transform in &transforms {
            assert!(blit_opaque_scaled_image(
                &mut target,
                &rgba,
                width,
                height,
                *transform,
                &mask
            ));
            std::hint::black_box(&target);
        }
    }
    eprintln!(
        "scaled video six tiles x20: tiny-skia {baseline:?}, opaque scaler {:?}",
        start.elapsed()
    );
}

/// Replay with text rendering via cosmic_text.
/// How far outside the viewport a command is still painted. Generous, because
/// a command's own bounds do not account for shadows, outlines or decoration
/// that spill beyond them.
const CULL_MARGIN: f32 = 512.0;
const DIRTY_CULL_MARGIN: f32 = 64.0;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct ShadowRasterKey {
    outer: [u32; 4],
    interior: [u32; 4],
    radii: [u32; 8],
    transform: [u32; 6],
    size: [u32; 2],
    blur: u32,
    spread: u32,
    color: [u8; 4],
}

#[derive(Default)]
struct ShadowRasterCache {
    entries: HashMap<ShadowRasterKey, (Arc<Pixmap>, u64)>,
    bytes: usize,
    access: u64,
}

impl ShadowRasterCache {
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    const MAX_ENTRY_BYTES: usize = 8 * 1024 * 1024;

    fn get(&mut self, key: &ShadowRasterKey) -> Option<Arc<Pixmap>> {
        self.access = self.access.wrapping_add(1);
        let (pixmap, last_access) = self.entries.get_mut(key)?;
        *last_access = self.access;
        Some(pixmap.clone())
    }

    fn insert(&mut self, key: ShadowRasterKey, pixmap: Arc<Pixmap>) {
        let bytes = pixmap.data().len();
        if bytes > Self::MAX_ENTRY_BYTES {
            return;
        }
        while self.bytes + bytes > Self::MAX_BYTES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, access))| *access)
                .map(|(key, _)| *key)
            else {
                break;
            };
            if let Some((old, _)) = self.entries.remove(&oldest) {
                self.bytes -= old.data().len();
            }
        }
        self.access = self.access.wrapping_add(1);
        self.bytes += bytes;
        self.entries.insert(key, (pixmap, self.access));
    }
}

thread_local! {
    static SHADOW_RASTER_CACHE: std::cell::RefCell<ShadowRasterCache> =
        std::cell::RefCell::new(ShadowRasterCache::default());
}

#[derive(Clone)]
struct SharedClipMask {
    id: u64,
    mask: Arc<tiny_skia::Mask>,
}

/// Per-replay reuse: coordinates, transforms and parent coverage are immutable
/// here. No mask survives a frame or consumes an unbounded global cache.
#[derive(Default)]
struct ClipMaskCache {
    masks: HashMap<([u32; 18], u64), (SharedClipMask, u64)>,
    bytes: usize,
    next_id: u64,
    access: u64,
}

impl ClipMaskCache {
    const MIN_BYTES: usize = 8 * 1024 * 1024;
    const MAX_BYTES: usize = 32 * 1024 * 1024;
    // Gmail's nested clips exceed the old four-mask target on a 512px tile.
    // This remains bounded by MAX_BYTES and lives only for one tile replay.
    const TARGET_REUSABLE_MASKS: usize = 96;

    fn budget(mask_bytes: usize) -> usize {
        mask_bytes
            .saturating_mul(Self::TARGET_REUSABLE_MASKS)
            .clamp(Self::MIN_BYTES, Self::MAX_BYTES)
    }

    fn share(&mut self, mask: tiny_skia::Mask) -> SharedClipMask {
        self.next_id += 1;
        SharedClipMask {
            id: self.next_id,
            mask: Arc::new(mask),
        }
    }

    fn rect(
        &mut self,
        rect: &Rect,
        radius: &[f32; 4],
        radius_y: &[f32; 4],
        pw: u32,
        ph: u32,
        ts: Transform,
        parent: Option<&SharedClipMask>,
    ) -> Option<SharedClipMask> {
        let key = (
            [
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                radius[0],
                radius[1],
                radius[2],
                radius[3],
                radius_y[0],
                radius_y[1],
                radius_y[2],
                radius_y[3],
                ts.sx,
                ts.kx,
                ts.ky,
                ts.sy,
                ts.tx,
                ts.ty,
            ]
            .map(f32::to_bits),
            parent.map_or(0, |p| p.id),
        );
        self.access += 1;
        if let Some((mask, used)) = self.masks.get_mut(&key) {
            *used = self.access;
            return Some(mask.clone());
        }
        let _mask_build = crate::profile::is_enabled()
            .then(|| crate::profile::span(crate::profile::Phase::RasterClipMaskBuild));
        let mut mask = build_clip_mask_with_transform(rect, radius, radius_y, pw, ph, ts)?;
        if let Some(parent) = parent {
            multiply_clip_mask_by_parent(&mut mask, parent.mask.as_ref(), *rect, ts);
        }
        let bytes = mask.data().len();
        let mask = self.share(mask);
        let budget = Self::budget(bytes);
        if bytes <= budget {
            while self.bytes + bytes > budget {
                let oldest = self
                    .masks
                    .iter()
                    .min_by_key(|(_, (_, used))| *used)
                    .map(|(key, _)| *key);
                let Some(oldest) = oldest else {
                    break;
                };
                if let Some((removed, _)) = self.masks.remove(&oldest) {
                    self.bytes -= removed.mask.data().len();
                }
            }
            self.bytes += bytes;
            self.masks.insert(key, (mask.clone(), self.access));
        }
        Some(mask)
    }
}

fn multiply_clip_mask_by_parent(
    mask: &mut tiny_skia::Mask,
    parent: &tiny_skia::Mask,
    rect: Rect,
    ts: Transform,
) {
    let width = mask.width() as usize;
    let height = mask.height() as usize;
    let bounds = transformed_bounds_to_viewport(ts, rect, 1.0);
    let (left, top, right, bottom) = if let Some(bounds) = bounds {
        // Include antialiased edge pixels; outside this rectangle the fresh
        // mask is zero, so multiplying by the parent cannot change it.
        let left = (bounds.x.floor() - 2.0).max(0.0) as usize;
        let top = (bounds.y.floor() - 2.0).max(0.0) as usize;
        let right = (bounds.right().ceil() + 2.0).min(width as f32) as usize;
        let bottom = (bounds.bottom().ceil() + 2.0).min(height as f32) as usize;
        (left, top, right, bottom)
    } else {
        (0, 0, width, height)
    };
    if left >= right || top >= bottom {
        return;
    }
    let dst = mask.data_mut();
    let src = parent.data();
    for row in top..bottom {
        let start = row * width + left;
        let end = row * width + right;
        for (dst, src) in dst[start..end].iter_mut().zip(&src[start..end]) {
            *dst = (*dst as u16 * *src as u16 / 255) as u8;
        }
    }
}

#[test]
fn clip_cache_preserves_transforms_radii_and_parent_coverage() {
    let mut cache = ClipMaskCache::default();
    let rect = Rect::new(8.25, 6.5, 34.0, 27.0);
    let parent = cache.share(
        build_clip_mask_with_transform(
            &Rect::new(0.0, 0.0, 25.0, 50.0),
            &[0.0; 4],
            &[0.0; 4],
            64,
            64,
            Transform::identity(),
        )
        .unwrap(),
    );
    for ts in [
        Transform::identity(),
        Transform::from_rotate(12.0),
        Transform::from_scale(1.25, 0.75),
    ] {
        for radius in [[0.0; 4], [8.0, 4.0, 6.0, 2.0]] {
            let mut expected =
                build_clip_mask_with_transform(&rect, &radius, &radius, 64, 64, ts).unwrap();
            for (dst, src) in expected.data_mut().iter_mut().zip(parent.mask.data()) {
                *dst = (*dst as u16 * *src as u16 / 255) as u8;
            }
            let first = cache
                .rect(&rect, &radius, &radius, 64, 64, ts, Some(&parent))
                .unwrap();
            let second = cache
                .rect(&rect, &radius, &radius, 64, 64, ts, Some(&parent))
                .unwrap();
            assert_eq!(first.mask.data(), expected.data());
            assert!(Arc::ptr_eq(&first.mask, &second.mask));
            let unclipped = cache
                .rect(&rect, &radius, &radius, 64, 64, ts, None)
                .unwrap();
            assert_ne!(first.id, unclipped.id);
        }
    }
}

#[test]
fn bounded_parent_clip_multiplication_matches_full_mask() {
    let (width, height) = (256, 192);
    let parent = build_clip_mask_with_transform(
        &Rect::new(13.25, 9.5, 180.0, 150.0),
        &[7.0; 4],
        &[7.0; 4],
        width,
        height,
        Transform::identity(),
    )
    .unwrap();
    let cases = [
        (Rect::new(25.25, 14.75, 19.5, 24.25), Transform::identity()),
        (
            Rect::new(25.25, 14.75, 19.5, 24.25),
            Transform::from_rotate(37.0).post_translate(90.0, 20.0),
        ),
        (
            Rect::new(-14.5, -9.25, 40.5, 27.0),
            Transform::from_scale(1.5, 0.75),
        ),
        (Rect::new(220.0, 160.0, 80.0, 70.0), Transform::identity()),
    ];
    for (rect, transform) in cases {
        for radii in [[0.0; 4], [6.0; 4]] {
            let mut bounded =
                build_clip_mask_with_transform(&rect, &radii, &radii, width, height, transform)
                    .unwrap();
            let mut full = bounded.clone();
            for (dst, src) in full.data_mut().iter_mut().zip(parent.data()) {
                *dst = (*dst as u16 * *src as u16 / 255) as u8;
            }
            multiply_clip_mask_by_parent(&mut bounded, &parent, rect, transform);
            assert_eq!(
                bounded.data(),
                full.data(),
                "rect={rect:?}, transform={transform:?}"
            );
        }
    }
}

#[test]
fn nested_viewport_clip_masks_remain_reusable_within_one_replay() {
    let mut cache = ClipMaskCache::default();
    let size = (2560, 1640);
    let outer = Rect::new(0.0, 0.0, size.0 as f32, size.1 as f32);
    let inner = Rect::new(8.0, 8.0, size.0 as f32 - 16.0, size.1 as f32 - 16.0);
    let radius = [4.0; 4];
    let transform = Transform::identity();
    let parent = cache
        .rect(&outer, &radius, &radius, size.0, size.1, transform, None)
        .unwrap();
    let child = cache
        .rect(
            &inner,
            &radius,
            &radius,
            size.0,
            size.1,
            transform,
            Some(&parent),
        )
        .unwrap();
    let parent_again = cache
        .rect(&outer, &radius, &radius, size.0, size.1, transform, None)
        .unwrap();
    let child_again = cache
        .rect(
            &inner,
            &radius,
            &radius,
            size.0,
            size.1,
            transform,
            Some(&parent_again),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&parent.mask, &parent_again.mask));
    assert!(Arc::ptr_eq(&child.mask, &child_again.mask));
    assert!(cache.bytes <= ClipMaskCache::MAX_BYTES);
}

#[test]
fn viewport_sized_nested_clip_preserves_parent_mask() {
    let commands = vec![
        PaintCmd::PushClip {
            rect: Rect::new(0.0, 0.0, 12.0, 32.0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        },
        PaintCmd::PushClip {
            rect: Rect::new(0.0, 0.0, 32.0, 32.0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        },
        PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 32.0, 32.0),
            color: Color::BLACK,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        },
        PaintCmd::PopClip,
        PaintCmd::PopClip,
    ];
    let mut pixmap = Pixmap::new(32, 32).unwrap();
    replay_commands_inner(
        &commands,
        &mut pixmap,
        1.0,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
    assert_eq!(pixmap.pixel(5, 5).unwrap().alpha(), 255);
    assert_eq!(pixmap.pixel(20, 5).unwrap().alpha(), 0);
}

thread_local! {
    /// (font-DB face count, shaped buffers by text+attrs). See the note at the
    /// shaping site: this is the difference between re-shaping every visible
    /// text run on every frame and shaping each distinct one once.
    static SHAPED: std::cell::RefCell<(usize, std::collections::HashMap<u64, Buffer>)> =
        std::cell::RefCell::new((usize::MAX, std::collections::HashMap::new()));
}

/// The conservative document-space extent of a DRAWING command, or `None` for
/// a command that must never be skipped (anything that manipulates a stack).
fn cmd_bounds(cmd: &PaintCmd) -> Option<Rect> {
    match cmd {
        PaintCmd::FillRect { rect, .. }
        | PaintCmd::Border { rect, .. }
        | PaintCmd::BorderImage { rect, .. }
        | PaintCmd::Image { rect, .. }
        | PaintCmd::BackdropFilter { rect, .. }
        | PaintCmd::Outline { rect, .. }
        | PaintCmd::ResizeGrip { rect, .. }
        | PaintCmd::FormElement { rect, .. } => Some(*rect),
        PaintCmd::Gradient { clip, .. } => Some(*clip),
        PaintCmd::BackgroundImage { clip, .. } => Some(*clip),
        PaintCmd::HorizontalRule { x1, y1, x2 } => {
            let left = x1.min(*x2);
            let right = x1.max(*x2);
            Some(Rect::new(left - 4.0, y1 - 4.0, right - left + 8.0, 8.0))
        }
        PaintCmd::ListMarker {
            x,
            y,
            size,
            text_align,
            font_size,
            line_height,
            text,
            ..
        } => {
            let pad = size.max(*font_size).max(*line_height) * 2.0;
            let width = size.max(*font_size * text.chars().count().max(1) as f32);
            let left = if *text_align == crate::types::TextAlign::Right {
                x - width
            } else {
                *x
            };
            Some(Rect::new(left - pad, y - pad, width + pad * 2.0, pad * 2.0))
        }
        PaintCmd::Text {
            x,
            y,
            text,
            font_size,
            line_height,
            letter_spacing,
            word_spacing,
            ..
        } => {
            let pad = font_size.max(*line_height) * 2.0;
            let width = text.chars().count().max(1) as f32
                * (*font_size * 0.8 + letter_spacing.max(0.0))
                + text.matches(char::is_whitespace).count() as f32 * word_spacing.max(0.0);
            Some(Rect::new(x - pad, y - pad, width + pad * 2.0, pad * 2.0))
        }
        PaintCmd::TextShadow {
            x,
            y,
            text,
            font_size,
            line_height,
            blur,
            ..
        } => {
            let pad = font_size.max(*line_height) * 2.0 + blur * 4.0 + 8.0;
            let width = text.chars().count().max(1) as f32 * *font_size * 0.9;
            Some(Rect::new(x - pad, y - pad, width + pad * 2.0, pad * 2.0))
        }
        PaintCmd::BoxShadow {
            rect,
            offset_x,
            offset_y,
            blur,
            spread,
            ..
        } => {
            let left = rect.x + offset_x - spread - blur * 4.0 - 8.0;
            let top = rect.y + offset_y - spread - blur * 4.0 - 8.0;
            let right = rect.x + rect.w + offset_x + spread + blur * 4.0 + 8.0;
            let bottom = rect.y + rect.h + offset_y + spread + blur * 4.0 + 8.0;
            Some(Rect::new(left, top, right - left, bottom - top))
        }
        _ => None,
    }
}

fn rect_outside_view(rect: Rect, left: f32, top: f32, right: f32, bottom: f32) -> bool {
    rect.right() < left || rect.x > right || rect.bottom() < top || rect.y > bottom
}

fn gradient_stops_with_transparent_hues(
    stops: &[(Color, f32)],
    opacity: f32,
) -> Vec<tiny_skia::GradientStop> {
    let mut result = Vec::with_capacity(stops.len() + 2);
    for (index, (color, position)) in stops.iter().enumerate() {
        let alpha = (color.a as f32 * opacity).clamp(0.0, 255.0) as u8;
        if color.a == 0 {
            let before = stops[..index].iter().rev().find(|(c, _)| c.a > 0);
            let after = stops[index + 1..].iter().find(|(c, _)| c.a > 0);
            if let Some((neighbor, _)) = before {
                result.push(tiny_skia::GradientStop::new(
                    *position,
                    tiny_skia::Color::from_rgba8(neighbor.r, neighbor.g, neighbor.b, 0),
                ));
            }
            if let Some((neighbor, _)) = after {
                result.push(tiny_skia::GradientStop::new(
                    *position,
                    tiny_skia::Color::from_rgba8(neighbor.r, neighbor.g, neighbor.b, 0),
                ));
            }
            if before.is_some() || after.is_some() {
                continue;
            }
        }
        result.push(tiny_skia::GradientStop::new(
            *position,
            tiny_skia::Color::from_rgba8(color.r, color.g, color.b, alpha),
        ));
    }
    result
}

fn transformed_bounds_to_viewport(ts: Transform, rect: Rect, scale: f32) -> Option<Rect> {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let points = [
        (rect.x, rect.y),
        (rect.x + rect.w, rect.y),
        (rect.x, rect.y + rect.h),
        (rect.x + rect.w, rect.y + rect.h),
    ];
    let inv_scale = 1.0 / scale.max(0.001);
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (x, y) in points {
        let px = (ts.sx * x + ts.kx * y + ts.tx) * inv_scale;
        let py = (ts.ky * x + ts.sy * y + ts.ty) * inv_scale;
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
        return None;
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

#[cfg(test)]
mod transform_bounds_tests {
    use super::*;

    #[test]
    fn text_raster_resolution_includes_scale_rotation_and_skew() {
        assert_eq!(
            text_transform_raster_scale(Transform::from_scale(1.5, 2.0)),
            2.0
        );
        assert!((text_transform_raster_scale(Transform::from_rotate(30.0)) - 1.0).abs() < 0.001);
        let skew = Transform::from_row(1.0, 0.0, 1.0, 1.0, 0.0, 0.0);
        assert!((text_transform_raster_scale(skew) - 1.618034).abs() < 0.001);
        assert_eq!(
            text_transform_raster_scale(Transform::from_scale(-3.0, 2.0)),
            3.0
        );
    }

    #[test]
    fn rotated_paint_bounds_follow_tiny_skia_matrix() {
        let transform = Transform::from_rotate_at(67.0, 843.0, 623.0);
        let rect = Rect::new(827.0, 607.0, 32.0, 24.0);
        let bounds = transformed_bounds_to_viewport(transform, rect, 1.0).unwrap();
        for (x, y) in [
            (rect.x, rect.y),
            (rect.right(), rect.y),
            (rect.x, rect.bottom()),
            (rect.right(), rect.bottom()),
        ] {
            let mut point = tiny_skia::Point::from_xy(x, y);
            transform.map_point(&mut point);
            assert!(point.x >= bounds.x - 0.01 && point.x <= bounds.right() + 0.01);
            assert!(point.y >= bounds.y - 0.01 && point.y <= bounds.bottom() + 0.01);
        }
    }
}

pub fn replay_with_text(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        0.0,
        0.0,
        None,
        None,
        None,
    );
}

/// Replay with a scroll offset — the display list is in document coordinates,
/// the scroll offset translates to screen coordinates during replay.
pub fn replay_with_scroll(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        None,
        None,
    );
}

/// Replay with scroll plus per-node animated transform matrices. The display
/// list remains cached; transform animation updates are applied while replaying
/// `PushTransform` commands instead of rebuilding the list every frame.
pub fn replay_with_scroll_and_transform_overrides(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
    transform_overrides: &HashMap<u32, [f32; 6]>,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        None,
        Some(transform_overrides),
    );
}

/// Replay only a viewport-local dirty rectangle. The display list remains in
/// document coordinates; `clip` is in viewport coordinates after scroll.
pub fn replay_with_scroll_clip(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
    clip: Rect,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        Some(clip),
        None,
    );
}

pub fn replay_with_scroll_clip_and_transform_overrides(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
    clip: Rect,
    transform_overrides: &HashMap<u32, [f32; 6]>,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        Some(clip),
        Some(transform_overrides),
    );
}

pub fn replay_commands_with_scroll(
    commands: &[PaintCmd],
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
) {
    replay_commands_inner(
        commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        None,
        None,
    );
}

pub fn replay_commands_with_scroll_and_transform_overrides(
    commands: &[PaintCmd],
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
    transform_overrides: &HashMap<u32, [f32; 6]>,
) {
    replay_commands_inner(
        commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        None,
        Some(transform_overrides),
    );
}

pub fn replay_commands_with_scroll_clip_and_transform_overrides(
    commands: &[PaintCmd],
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scroll_x: f32,
    scroll_y: f32,
    clip: Rect,
    transform_overrides: &HashMap<u32, [f32; 6]>,
) {
    replay_commands_inner(
        commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
        None,
        Some(clip),
        Some(transform_overrides),
    );
}

/// A tile's raster origin is not the viewport origin. Fixed boxes must follow
/// the viewport even when ordinary document boxes are rasterized into tiles.
pub fn replay_tile_with_scroll_and_transform_overrides(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    tile_x: f32,
    tile_y: f32,
    viewport_x: f32,
    viewport_y: f32,
    transform_overrides: Option<&HashMap<u32, [f32; 6]>>,
) {
    replay_commands_inner(
        &list.commands,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        tile_x,
        tile_y,
        Some((viewport_x, viewport_y)),
        None,
        transform_overrides,
    );
}

/// Layer for blend mode / opacity compositing.
struct Layer {
    pixmap: Pixmap,
    blend_mode: u8,
    alpha: f32,
    has_content: bool,
    paint_bounds: Option<Rect>,
}

fn layer_pixel_region(bounds: Option<Rect>, width: u32, height: u32) -> Option<tiny_skia::IntRect> {
    let bounds = bounds.unwrap_or(Rect::new(0.0, 0.0, width as f32, height as f32));
    tiny_skia::IntRect::from_ltrb(
        bounds.x.floor().max(0.0).min(width as f32) as i32,
        bounds.y.floor().max(0.0).min(height as f32) as i32,
        bounds.right().ceil().max(0.0).min(width as f32) as i32,
        bounds.bottom().ceil().max(0.0).min(height as f32) as i32,
    )
}

fn clear_layer_region(pixmap: &mut Pixmap, region: tiny_skia::IntRect) {
    let stride = pixmap.width() as usize * 4;
    let start = region.x() as usize * 4;
    let end = start + region.width() as usize * 4;
    for row in pixmap
        .data_mut()
        .chunks_exact_mut(stride)
        .skip(region.y() as usize)
        .take(region.height() as usize)
    {
        row[start..end].fill(0);
    }
}

fn composite_opacity_region(
    target: &mut Pixmap,
    source: &Pixmap,
    alpha: f32,
    region: tiny_skia::IntRect,
) {
    // Equivalent to draw_pixmap, but retain the full source's coordinate system
    // and restrict the raster bounds instead of copying a cropped bitmap.
    let paint = Paint {
        shader: tiny_skia::Pattern::new(
            source.as_ref(),
            tiny_skia::SpreadMode::Pad,
            tiny_skia::FilterQuality::Nearest,
            alpha,
            Transform::identity(),
        ),
        anti_alias: false,
        ..Paint::default()
    };
    target.fill_rect(region.to_rect(), &paint, Transform::identity(), None);
}

#[test]
fn opacity_region_composite_matches_cropped_reference() {
    let mut source = Pixmap::new(57, 43).unwrap();
    for (index, pixel) in source.pixels_mut().iter_mut().enumerate() {
        let alpha = (index % 256) as u8;
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(alpha / 2, alpha / 3, alpha / 4, alpha)
            .unwrap();
    }
    for opacity in [0.0, 0.17, 0.5, 1.0] {
        for region in [
            tiny_skia::IntRect::from_xywh(0, 0, 57, 43).unwrap(),
            tiny_skia::IntRect::from_xywh(13, 7, 29, 21).unwrap(),
            tiny_skia::IntRect::from_xywh(56, 42, 1, 1).unwrap(),
        ] {
            let mut reference = Pixmap::new(57, 43).unwrap();
            reference.fill(tiny_skia::Color::from_rgba8(17, 89, 173, 127));
            let mut actual = reference.clone();
            reference.draw_pixmap(
                region.x(),
                region.y(),
                source.clone_rect(region).unwrap().as_ref(),
                &tiny_skia::PixmapPaint {
                    opacity,
                    quality: tiny_skia::FilterQuality::Nearest,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
            composite_opacity_region(&mut actual, &source, opacity, region);
            assert_eq!(
                actual.data(),
                reference.data(),
                "opacity {opacity}, region {region:?}"
            );
        }
    }
}

#[test]
fn opacity_pool_clears_only_its_previous_paint_region() {
    let mut surface = Pixmap::new(57, 43).unwrap();
    surface.fill(tiny_skia::Color::WHITE);
    let bounds = Rect::new(13.25, 7.25, 28.25, 20.25);
    let region = layer_pixel_region(Some(bounds), 57, 43).unwrap();
    clear_layer_region(&mut surface, region);
    for y in 0..43 {
        for x in 0..57 {
            let cleared = x >= 13 && x < 42 && y >= 7 && y < 28;
            assert_eq!(
                surface.pixel(x, y).unwrap().alpha(),
                if cleared { 0 } else { 255 }
            );
        }
    }
    assert!(layer_pixel_region(Some(Rect::new(70.0, 0.0, 5.0, 5.0)), 57, 43).is_none());
}

#[test]
fn reused_opacity_surfaces_preserve_disjoint_nested_and_invisible_groups() {
    let fill = |rect, color| PaintCmd::FillRect {
        rect,
        color,
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    let groups = [
        (0.0, Rect::new(2.0, 2.0, 15.0, 15.0), Color::rgb(220, 0, 0)),
        (
            0.6,
            Rect::new(27.25, 4.5, 15.0, 12.0),
            Color::rgba(0, 160, 0, 190),
        ),
        (0.4, Rect::new(8.5, 29.25, 24.0, 8.0), Color::rgb(0, 0, 220)),
        (
            0.8,
            Rect::new(19.0, 11.0, 12.0, 14.0),
            Color::rgba(180, 80, 0, 90),
        ),
    ];
    for scale in [1.0, 2.0] {
        let mut expected = Pixmap::new(100, 90).unwrap();
        expected.fill(tiny_skia::Color::WHITE);
        let mut actual = expected.clone();
        let mut commands = Vec::new();
        for (alpha, rect, color) in groups {
            let inner = [
                PaintCmd::PushOpacity { alpha: 0.7 },
                fill(rect, color),
                PaintCmd::PopOpacity,
            ];
            let mut source = Pixmap::new(100, 90).unwrap();
            replay_commands_inner(&inner, &mut source, scale, None, 0.0, 0.0, None, None, None);
            expected.draw_pixmap(
                0,
                0,
                source.as_ref(),
                &tiny_skia::PixmapPaint {
                    opacity: alpha,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
            commands.push(PaintCmd::PushOpacity { alpha });
            commands.extend(inner);
            commands.push(PaintCmd::PopOpacity);
        }
        replay_commands_inner(
            &commands,
            &mut actual,
            scale,
            None,
            0.0,
            0.0,
            None,
            None,
            None,
        );
        assert_eq!(actual.data(), expected.data(), "scale {scale}");
    }
}

fn replay_commands_inner(
    commands: &[PaintCmd],
    pixmap: &mut Pixmap,
    scale: f32,
    text_ctx: Option<(&mut FontSystem, &mut SwashCache)>,
    scroll_x: f32,
    scroll_y: f32,
    fixed_viewport_scroll: Option<(f32, f32)>,
    dirty_clip: Option<Rect>,
    transform_overrides: Option<&HashMap<u32, [f32; 6]>>,
) {
    // Damage is a set of device pixels, not an antialiased CSS clip. Match
    // the outward rounding used when clearing the retained surface so every
    // cleared edge pixel is completely repainted.
    let dirty_clip = dirty_clip.map(|clip| round_damage_clip_to_device_pixels(clip, scale));
    // Bound intermediate opacity/blend surfaces as well as drawing. Filters
    // retain the viewport surface because they may sample outside the damage.
    if let Some(clip) = dirty_clip.filter(|_| {
        scale.is_finite()
            && scale > 0.0
            && !commands.iter().any(|cmd| {
                matches!(
                    cmd,
                    PaintCmd::PushFilter { .. } | PaintCmd::BackdropFilter { .. }
                )
            })
    }) {
        let left = (clip.x * scale).floor().max(0.0).min(pixmap.width() as f32) as i32;
        let top = (clip.y * scale)
            .floor()
            .max(0.0)
            .min(pixmap.height() as f32) as i32;
        let right = (clip.right() * scale)
            .ceil()
            .max(0.0)
            .min(pixmap.width() as f32) as i32;
        let bottom = (clip.bottom() * scale)
            .ceil()
            .max(0.0)
            .min(pixmap.height() as f32) as i32;
        let Some(rect) = tiny_skia::IntRect::from_ltrb(left, top, right, bottom) else {
            return;
        };
        if let Some(mut surface) = pixmap.clone_rect(rect) {
            let origin_x = left as f32 / scale;
            let origin_y = top as f32 / scale;
            replay_commands_on_surface(
                commands,
                &mut surface,
                scale,
                text_ctx,
                scroll_x + origin_x,
                scroll_y + origin_y,
                Some(fixed_viewport_scroll.unwrap_or((scroll_x, scroll_y))),
                Some(Rect {
                    x: clip.x - origin_x,
                    y: clip.y - origin_y,
                    ..clip
                }),
                transform_overrides,
            );
            // Replace rather than blend: the surface already includes the
            // original backdrop, including its alpha.
            let stride = pixmap.width() as usize * 4;
            let row_bytes = surface.width() as usize * 4;
            for (row, pixels) in surface.data().chunks_exact(row_bytes).enumerate() {
                let start = (top as usize + row) * stride + left as usize * 4;
                pixmap.data_mut()[start..start + row_bytes].copy_from_slice(pixels);
            }
            return;
        }
    }
    replay_commands_on_surface(
        commands,
        pixmap,
        scale,
        text_ctx,
        scroll_x,
        scroll_y,
        fixed_viewport_scroll,
        dirty_clip,
        transform_overrides,
    );
}

fn round_damage_clip_to_device_pixels(clip: Rect, scale: f32) -> Rect {
    if !scale.is_finite() || scale <= 0.0 {
        return clip;
    }
    let left = (clip.x * scale).floor() / scale;
    let top = (clip.y * scale).floor() / scale;
    let right = (clip.right() * scale).ceil() / scale;
    let bottom = (clip.bottom() * scale).ceil() / scale;
    Rect::new(left, top, right - left, bottom - top)
}

#[test]
fn bounded_opacity_matches_full_surface_compositing() {
    let angle = 0.4_f32;
    let commands = vec![
        PaintCmd::PushTransform {
            node_id: 1,
            transform: [
                angle.cos(),
                angle.sin(),
                -angle.sin(),
                angle.cos(),
                24.0,
                20.0,
            ],
        },
        PaintCmd::FillRect {
            rect: Rect::new(-20.0, -10.0, 40.0, 25.0),
            color: crate::types::Color::rgb(200, 40, 80),
            radius: [3.0; 4],
            radius_y: [3.0; 4],
        },
        PaintCmd::PopTransform,
        PaintCmd::BeginFixedPosition,
        PaintCmd::FillRect {
            rect: Rect::new(70.25, 20.5, 12.0, 8.0),
            color: crate::types::Color::rgb(20, 100, 200),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        },
        PaintCmd::EndFixedPosition,
    ];
    let mut grouped = vec![PaintCmd::PushOpacity { alpha: 0.37 }];
    grouped.extend(commands.iter().cloned());
    grouped.push(PaintCmd::PopOpacity);
    for scale in [1.0, 1.5, 2.0] {
        let mut layer = Pixmap::new(200, 180).unwrap();
        replay_commands_inner(
            &commands, &mut layer, scale, None, 3.0, 5.0, None, None, None,
        );
        let mut expected = Pixmap::new(200, 180).unwrap();
        expected.fill(tiny_skia::Color::from_rgba8(100, 90, 80, 128));
        let mut actual = expected.clone();
        let paint = tiny_skia::PixmapPaint {
            opacity: 0.37,
            ..Default::default()
        };
        expected.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
        replay_commands_inner(
            &grouped,
            &mut actual,
            scale,
            None,
            3.0,
            5.0,
            None,
            None,
            None,
        );
        assert_eq!(actual.data(), expected.data(), "scale={scale}");
    }
}

#[test]
fn opacity_gradient_uses_paint_clip_without_cropping_repeated_tiles() {
    for repeat in [0, 1] {
        let gradient = PaintCmd::Gradient {
            rect: Rect::new(25.0, 20.0, 30.0, 18.0),
            clip: Rect::new(12.0, 10.0, 115.0, 72.0),
            repeat_x_mode: repeat,
            repeat_y_mode: repeat,
            gradient_type: 1,
            angle: 90.0,
            direction: GradientDirection::Angle(90.0),
            radial_center_x: 0.5,
            radial_center_y: 0.5,
            radial_radius_x: 0.5,
            radial_radius_y: 0.5,
            stops: vec![
                (Color::rgb(210, 30, 20), 0.0),
                (Color::rgb(20, 60, 220), 1.0),
            ],
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            opacity: 1.0,
            blend_mode: 0,
        };
        assert_eq!(
            cmd_bounds(&gradient),
            Some(Rect::new(12.0, 10.0, 115.0, 72.0))
        );
        for scale in [1.0, 1.5] {
            let mut layer = Pixmap::new(200, 150).unwrap();
            replay_commands_inner(
                &[gradient.clone()],
                &mut layer,
                scale,
                None,
                0.0,
                0.0,
                None,
                None,
                None,
            );
            let mut expected = Pixmap::new(200, 150).unwrap();
            expected.fill(tiny_skia::Color::from_rgba8(80, 90, 100, 255));
            let mut actual = expected.clone();
            expected.draw_pixmap(
                0,
                0,
                layer.as_ref(),
                &tiny_skia::PixmapPaint {
                    opacity: 0.6,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
            replay_commands_inner(
                &[
                    PaintCmd::PushOpacity { alpha: 0.6 },
                    gradient.clone(),
                    PaintCmd::PopOpacity,
                ],
                &mut actual,
                scale,
                None,
                0.0,
                0.0,
                None,
                None,
                None,
            );
            assert_eq!(
                actual.data(),
                expected.data(),
                "repeat={repeat}, scale={scale}"
            );
        }
    }
}

#[test]
fn blurred_shadow_cache_preserves_pixels_and_skips_empty_boxes() {
    let shadow = |height: f32, color: Color| PaintCmd::BoxShadow {
        rect: Rect::new(50.0, 50.0, 80.0, height),
        color,
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 10.0,
        spread: 0.0,
        inset: false,
        radii: [4.0; 4],
        radii_y: [4.0; 4],
    };
    let render = |command| {
        let mut pixmap = Pixmap::new(200, 200).unwrap();
        replay_commands_inner(
            &[command],
            &mut pixmap,
            1.0,
            None,
            0.0,
            0.0,
            None,
            None,
            None,
        );
        pixmap
    };
    let first = render(shadow(80.0, Color::rgba(0, 0, 0, 180)));
    let repeated = render(shadow(80.0, Color::rgba(0, 0, 0, 180)));
    assert_eq!(first.data(), repeated.data());
    let recolored = render(shadow(80.0, Color::rgba(180, 0, 0, 180)));
    assert_ne!(first.data(), recolored.data());
    let empty = render(shadow(0.0, Color::rgba(0, 0, 0, 180)));
    assert!(empty.data().iter().all(|channel| *channel == 0));
}

#[test]
fn translated_blurred_shadows_share_a_raster() {
    let shadow = |x: f32| PaintCmd::BoxShadow {
        rect: Rect::new(x, 50.0, 80.0, 80.0),
        color: Color::rgba(0, 0, 0, 180),
        offset_x: 0.0,
        offset_y: 4.0,
        blur: 10.0,
        spread: 0.0,
        inset: false,
        radii: [4.0; 4],
        radii_y: [4.0; 4],
    };
    let render = |x| {
        let mut pixmap = Pixmap::new(240, 200).unwrap();
        replay_commands_inner(
            &[shadow(x)],
            &mut pixmap,
            1.0,
            None,
            0.0,
            0.0,
            None,
            None,
            None,
        );
        pixmap
    };
    let first = render(50.0);
    let entries_after_first = SHADOW_RASTER_CACHE.with(|cache| cache.borrow().entries.len());
    let second = render(100.0);
    let entries_after_second = SHADOW_RASTER_CACHE.with(|cache| cache.borrow().entries.len());
    assert_eq!(entries_after_second, entries_after_first);
    for y in 0..200 {
        for x in 0..140 {
            let a = ((y * 240 + x) * 4) as usize;
            let b = ((y * 240 + x + 50) * 4) as usize;
            assert_eq!(&first.data()[a..a + 4], &second.data()[b..b + 4]);
        }
    }
}

#[test]
fn culled_opacity_groups_preserve_nested_visible_content() {
    let fill = |y| PaintCmd::FillRect {
        rect: Rect::new(0.0, y, 20.0, 20.0),
        color: crate::types::Color::rgb(255, 0, 0),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    let mut commands = Vec::new();
    for _ in 0..1000 {
        commands.extend([
            PaintCmd::PushOpacity { alpha: 0.5 },
            fill(5000.0),
            PaintCmd::PopOpacity,
        ]);
    }
    commands.extend([
        PaintCmd::PushOpacity { alpha: 0.5 },
        PaintCmd::PushOpacity { alpha: 0.5 },
        fill(0.0),
        PaintCmd::PopOpacity,
        PaintCmd::PopOpacity,
    ]);
    let mut actual = Pixmap::new(32, 32).unwrap();
    replay_commands_inner(
        &commands,
        &mut actual,
        1.0,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
    let pixel = actual.pixel(10, 10).unwrap();
    assert!(
        (63..=65).contains(&pixel.alpha()),
        "nested opacity: {pixel:?}"
    );
    assert_eq!(pixel.red(), pixel.alpha());
    assert_eq!(actual.pixel(30, 30).unwrap().alpha(), 0);
}

#[test]
fn culled_blend_groups_preserve_nested_visible_content() {
    let fill = |y| PaintCmd::FillRect {
        rect: Rect::new(0.0, y, 20.0, 20.0),
        color: crate::types::Color::rgb(255, 0, 0),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    let mut commands = Vec::new();
    for _ in 0..100 {
        commands.extend([
            PaintCmd::PushBlendMode { mode: 2 },
            fill(5000.0),
            PaintCmd::PopBlendMode,
        ]);
    }
    commands.extend([
        PaintCmd::PushBlendMode { mode: 2 },
        PaintCmd::PushBlendMode { mode: 2 },
        fill(0.0),
        PaintCmd::PopBlendMode,
        PaintCmd::PopBlendMode,
    ]);
    let mut actual = Pixmap::new(32, 32).unwrap();
    replay_commands_inner(
        &commands,
        &mut actual,
        1.0,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
    assert_eq!(actual.pixel(10, 10).unwrap().red(), 255);
    assert_eq!(actual.pixel(10, 10).unwrap().alpha(), 255);
    assert_eq!(actual.pixel(25, 25).unwrap().alpha(), 0);
}

#[test]
fn offscreen_mask_group_does_not_affect_visible_paint() {
    let fill = |rect, color| PaintCmd::FillRect {
        rect,
        color,
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    let mask = ImageRef::Owned(vec![255; 4], 1, 1);
    let visible = fill(
        Rect::new(4.0, 4.0, 12.0, 12.0),
        crate::types::Color::rgb(0, 80, 200),
    );
    let commands = vec![
        PaintCmd::PushMask {
            rect: Rect::new(5000.0, 5000.0, 16.0, 16.0),
            no_clip: false,
            origin: Rect::new(5000.0, 5000.0, 16.0, 16.0),
            tile: Rect::new(5000.0, 5000.0, 16.0, 16.0),
            data: mask.clone(),
            luminance: false,
            repeat_x_mode: 0,
            repeat_y_mode: 0,
        },
        fill(
            Rect::new(5000.0, 5000.0, 16.0, 16.0),
            crate::types::Color::rgb(200, 0, 0),
        ),
        PaintCmd::PopMask,
        PaintCmd::PushMask {
            rect: Rect::new(0.0, 0.0, 32.0, 32.0),
            no_clip: false,
            origin: Rect::new(0.0, 0.0, 32.0, 32.0),
            tile: Rect::new(0.0, 0.0, 32.0, 32.0),
            data: mask,
            luminance: false,
            repeat_x_mode: 0,
            repeat_y_mode: 0,
        },
        visible.clone(),
        PaintCmd::PopMask,
    ];
    let mut actual = Pixmap::new(32, 32).unwrap();
    replay_commands_inner(
        &commands,
        &mut actual,
        1.0,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
    let mut expected = Pixmap::new(32, 32).unwrap();
    replay_commands_inner(
        &[visible],
        &mut expected,
        1.0,
        None,
        0.0,
        0.0,
        None,
        None,
        None,
    );
    assert_eq!(actual.data(), expected.data());
}

#[test]
fn damage_surface_matches_viewport_replay_with_opacity_and_fixed_content() {
    let fill = |rect, color| PaintCmd::FillRect {
        rect,
        color,
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    let commands = vec![
        PaintCmd::PushOpacity { alpha: 0.6 },
        fill(
            Rect::new(0.0, 70.0, 100.0, 90.0),
            crate::types::Color::rgb(255, 0, 0),
        ),
        PaintCmd::BeginFixedPosition,
        PaintCmd::PushOpacity { alpha: 0.5 },
        fill(
            Rect::new(24.0, 18.0, 36.0, 38.0),
            crate::types::Color::rgb(0, 0, 255),
        ),
        PaintCmd::PopOpacity,
        PaintCmd::EndFixedPosition,
        PaintCmd::PopOpacity,
    ];
    for scale in [1.0, 1.5, 2.0] {
        for clip in [
            Rect::new(20.0, 15.0, 45.0, 48.0),
            Rect::new(-5.5, 12.5, 80.0, 41.0),
        ] {
            let device_clip = round_damage_clip_to_device_pixels(clip, scale);
            let mut expected = Pixmap::new(200, 180).unwrap();
            expected.fill(tiny_skia::Color::from_rgba8(20, 40, 60, 128));
            let mut actual = expected.clone();
            replay_commands_on_surface(
                &commands,
                &mut expected,
                scale,
                None,
                3.0,
                80.0,
                None,
                Some(device_clip),
                None,
            );
            replay_commands_inner(
                &commands,
                &mut actual,
                scale,
                None,
                3.0,
                80.0,
                None,
                Some(clip),
                None,
            );
            if let Some(index) = actual
                .data()
                .iter()
                .zip(expected.data())
                .position(|(a, b)| a != b)
            {
                let pixel = index / 4;
                let offset = pixel * 4;
                panic!(
                    "scale={scale}, clip={clip:?}, first difference at ({}, {}): actual={:?}, expected={:?}",
                    pixel % actual.width() as usize,
                    pixel / actual.width() as usize,
                    &actual.data()[offset..offset + 4],
                    &expected.data()[offset..offset + 4],
                );
            }
        }
    }
}

fn replay_commands_on_surface(
    commands: &[PaintCmd],
    pixmap: &mut Pixmap,
    scale: f32,
    mut text_ctx: Option<(&mut FontSystem, &mut SwashCache)>,
    scroll_x: f32,
    scroll_y: f32,
    fixed_viewport_scroll: Option<(f32, f32)>,
    dirty_clip: Option<Rect>,
    transform_overrides: Option<&HashMap<u32, [f32; 6]>>,
) {
    // Start with scale + scroll translation. Display list is in document
    // coordinates; the scroll offset maps to screen coordinates.
    let mut ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
    let mut fixed_stack: Vec<(Transform, f32, f32, i32)> = Vec::new();
    let (mut active_scroll_x, mut active_scroll_y) = (scroll_x, scroll_y);
    let mut transform_stack: Vec<Transform> = Vec::new();
    let mut filter_stack: Vec<Vec<(u8, f32, f32, f32, crate::types::Color)>> = Vec::new();
    let mut clip_stack: Vec<Rect> = Vec::new();
    let pw = pixmap.width();
    let ph = pixmap.height();
    let mut clip_cache = ClipMaskCache::default();
    let mut clip_mask_stack: Vec<Option<SharedClipMask>> = dirty_clip
        .map(|clip| build_viewport_clip_mask(clip, pw, ph, scale).map(|m| clip_cache.share(m)))
        .into_iter()
        .collect();
    let mut layer_stack: Vec<Layer> = Vec::new();
    let mut opacity_surface_pool: Vec<(Pixmap, Option<tiny_skia::IntRect>)> = Vec::new();
    let mut mask_stack: Vec<Vec<MaskLayer>> = Vec::new();
    let mut text_gradient_stack: Vec<&PaintCmd> = Vec::new();

    // ── Viewport culling ────────────────────────────────────────────────────
    //
    // ⛔ Replay used to paint the WHOLE document every frame. The display list
    // covers the full page height, so on a long page most commands drew far
    // outside the pixmap — and a `Text` command costs a cosmic-text shaping
    // pass whether or not any of it lands on screen. Measured on Wikipedia: a
    // render with NOTHING changed cost 5567 ms, and so did every scroll.
    //
    // Only DRAWING commands are skipped, and only while no transform is in
    // effect — a transform can move a command anywhere, and the push/pop
    // commands must all run or the clip and layer stacks desync.
    let inv_scale = 1.0 / scale.max(0.001);
    let view_w = (pw as f32) * inv_scale;
    let view_h = (ph as f32) * inv_scale;
    let (clip_left, clip_top, clip_right, clip_bottom) = if let Some(clip) = dirty_clip {
        let margin = DIRTY_CULL_MARGIN;
        (
            clip.x - margin,
            clip.y - margin,
            clip.right() + margin,
            clip.bottom() + margin,
        )
    } else {
        (
            -CULL_MARGIN,
            -CULL_MARGIN,
            view_w + CULL_MARGIN,
            view_h + CULL_MARGIN,
        )
    };
    let (vis_left, vis_top, vis_right, vis_bot) = if let Some(clip) = dirty_clip {
        let margin = DIRTY_CULL_MARGIN;
        (
            scroll_x + clip.x - margin,
            scroll_y + clip.y - margin,
            scroll_x + clip.right() + margin,
            scroll_y + clip.bottom() + margin,
        )
    } else {
        (
            scroll_x - CULL_MARGIN,
            scroll_y - CULL_MARGIN,
            scroll_x + view_w + CULL_MARGIN,
            scroll_y + view_h + CULL_MARGIN,
        )
    };
    let mut transform_depth = 0i32;
    let mut skip_clip_depth = 0u32;
    let profile_replay = crate::profile::is_enabled();

    for cmd in commands {
        match cmd {
            PaintCmd::BeginFixedPosition => {
                fixed_stack.push((ts, active_scroll_x, active_scroll_y, transform_depth));
                let (viewport_x, viewport_y) =
                    fixed_viewport_scroll.unwrap_or((scroll_x, scroll_y));
                active_scroll_x = scroll_x - viewport_x;
                active_scroll_y = scroll_y - viewport_y;
                ts = Transform::from_scale(scale, scale)
                    .pre_translate(-active_scroll_x, -active_scroll_y);
                transform_depth = 0;
                continue;
            }
            PaintCmd::EndFixedPosition => {
                if let Some((saved_ts, saved_x, saved_y, saved_depth)) = fixed_stack.pop() {
                    ts = saved_ts;
                    active_scroll_x = saved_x;
                    active_scroll_y = saved_y;
                    transform_depth = saved_depth;
                }
                continue;
            }
            _ => {}
        }
        let (vis_left, vis_top, vis_right, vis_bot) = (
            vis_left - (scroll_x - active_scroll_x),
            vis_top - (scroll_y - active_scroll_y),
            vis_right - (scroll_x - active_scroll_x),
            vis_bot - (scroll_y - active_scroll_y),
        );
        if skip_clip_depth > 0 {
            match cmd {
                PaintCmd::PushClip { .. }
                | PaintCmd::PushClipPath { .. }
                | PaintCmd::PushClipSvgPath { .. } => {
                    skip_clip_depth += 1;
                }
                PaintCmd::PopClip => {
                    skip_clip_depth -= 1;
                }
                _ => {}
            }
            continue;
        }

        match cmd {
            PaintCmd::PushTransform { .. } => transform_depth += 1,
            PaintCmd::PopTransform => transform_depth -= 1,
            _ => {}
        }
        if let PaintCmd::BoxShadow {
            rect,
            inset: false,
            radii,
            radii_y,
            ..
        } = cmd
            && transform_depth == 0
            && filter_stack.is_empty()
            && outer_shadow_has_no_pixels_in_surface(
                *rect,
                *radii,
                *radii_y,
                scale,
                active_scroll_x,
                active_scroll_y,
                pw,
                ph,
            )
        {
            continue;
        }
        // Layer-backed commands paint into an offscreen pixmap with the same
        // document-to-viewport transform, so drawing commands outside the
        // visible band can still be skipped. Stack commands themselves keep
        // returning `None` from `cmd_y_range`, preserving layer/clip balance.
        if let Some(bounds) = cmd_bounds(cmd) {
            // These commands include their full paint extent (including shadow
            // blur). Unlike glyph bounds, they need no extra neighboring tile.
            // A surrounding filter can spread pixels beyond that extent.
            let bounded_paint = filter_stack.is_empty()
                && matches!(
                    cmd,
                    PaintCmd::FillRect { .. }
                        | PaintCmd::Border { .. }
                        | PaintCmd::Image { .. }
                        | PaintCmd::BackgroundImage { .. }
                        | PaintCmd::Gradient { .. }
                        | PaintCmd::BoxShadow { .. }
                );
            if bounded_paint {
                if let Some(bounds) = transformed_bounds_to_viewport(ts, bounds, scale) {
                    let clip = dirty_clip.unwrap_or(Rect::new(0.0, 0.0, view_w, view_h));
                    // Preserve one device pixel at edges for antialiasing.
                    if rect_outside_view(
                        bounds,
                        clip.x - inv_scale,
                        clip.y - inv_scale,
                        clip.right() + inv_scale,
                        clip.bottom() + inv_scale,
                    ) {
                        continue;
                    }
                }
            } else if transform_depth == 0 {
                if rect_outside_view(bounds, vis_left, vis_top, vis_right, vis_bot) {
                    continue;
                }
            } else if let Some(bounds) = transformed_bounds_to_viewport(ts, bounds, scale) {
                if rect_outside_view(bounds, clip_left, clip_top, clip_right, clip_bottom) {
                    continue;
                }
            }
        }
        // Get the current clip mask (topmost on the stack)
        let clip_mask = clip_mask_stack
            .last()
            .and_then(|m| m.as_ref())
            .map(|m| m.mask.as_ref());

        // Stack commands are retained across viewport culling, but an opacity
        // group containing only culled draws has nothing to composite. Mark
        // ancestors too so nested masks/filters preserve their parent content.
        if let Some(bounds) = cmd_bounds(cmd) {
            let bounded = filter_stack.is_empty()
                && matches!(
                    cmd,
                    PaintCmd::FillRect { .. }
                        | PaintCmd::Border { .. }
                        | PaintCmd::Image { .. }
                        | PaintCmd::BackgroundImage { .. }
                        | PaintCmd::Gradient { .. }
                );
            let bounds = if bounded {
                transformed_bounds_to_viewport(ts, bounds, scale)
                    .map(|r| {
                        Rect::new(
                            r.x * scale - 1.0,
                            r.y * scale - 1.0,
                            r.w * scale + 2.0,
                            r.h * scale + 2.0,
                        )
                    })
                    .unwrap_or(Rect::new(0.0, 0.0, pw as f32, ph as f32))
            } else {
                // Glyph ink, decorations, filters and shadows need conservative
                // bounds; their layout boxes are not guaranteed paint extents.
                Rect::new(0.0, 0.0, pw as f32, ph as f32)
            };
            for layer in &mut layer_stack {
                layer.has_content = true;
                layer.paint_bounds = Some(match layer.paint_bounds {
                    Some(old) => {
                        let x = old.x.min(bounds.x);
                        let y = old.y.min(bounds.y);
                        Rect::new(
                            x,
                            y,
                            old.right().max(bounds.right()) - x,
                            old.bottom().max(bounds.bottom()) - y,
                        )
                    }
                    None => bounds,
                });
            }
        }

        let _command_timing = if profile_replay {
            use crate::profile::Phase;
            let phase = match cmd {
                PaintCmd::FillRect { .. } => Some(Phase::RasterFill),
                PaintCmd::Image { .. }
                | PaintCmd::BackgroundImage { .. }
                | PaintCmd::BorderImage { .. } => Some(Phase::RasterImage),
                PaintCmd::Text { .. } => Some(Phase::RasterText),
                PaintCmd::BoxShadow { .. } | PaintCmd::TextShadow { .. } => {
                    Some(Phase::RasterShadow)
                }
                PaintCmd::PushOpacity { .. }
                | PaintCmd::PushFilter { .. }
                | PaintCmd::PushBlendMode { .. }
                | PaintCmd::PushMask { .. }
                | PaintCmd::PushMaskGroup { .. }
                | PaintCmd::PopOpacity
                | PaintCmd::PopFilter
                | PaintCmd::PopBlendMode
                | PaintCmd::PopMask => Some(Phase::RasterLayer),
                PaintCmd::PushClip { .. }
                | PaintCmd::PushClipPath { .. }
                | PaintCmd::PushClipSvgPath { .. }
                | PaintCmd::PopClip => Some(Phase::RasterClip),
                _ => None,
            };
            phase.map(crate::profile::span)
        } else {
            None
        };

        match cmd {
            PaintCmd::PushTextGradient { .. } => {
                text_gradient_stack.push(cmd);
            }
            PaintCmd::PopTextGradient => {
                text_gradient_stack.pop();
            }
            PaintCmd::FillRect {
                rect,
                color,
                radius,
                radius_y,
            } => {
                let alpha = 1.0;
                let c = apply_opacity(color, alpha);
                let mut paint = Paint::default();
                paint.set_color(to_sk_color(&c));
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let max_r = radius[0].max(radius[1]).max(radius[2]).max(radius[3]);
                if max_r <= 0.5 && fill_opaque_aligned_rect(target, *rect, c, ts, clip_mask) {
                    continue;
                }
                if max_r > 0.5 {
                    if fill_opaque_rounded_rect_bands(
                        target, *rect, c, *radius, *radius_y, ts, clip_mask,
                    ) {
                        continue;
                    }
                    if let Some(path) = rounded_rect_path_corners_xy(
                        rect.x, rect.y, rect.w, rect.h, *radius, *radius_y,
                    ) {
                        target.fill_path(&path, &paint, FillRule::Winding, ts, clip_mask);
                    }
                } else if let Some(r) = SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h) {
                    target.fill_rect(r, &paint, ts, clip_mask);
                }
            }

            PaintCmd::Border {
                rect,
                widths,
                colors,
                styles,
                radii,
                radii_y,
                opacity,
            } => {
                let alpha = *opacity;
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);

                // Uniform border with border-radius → use stroked rounded rect
                let uniform_width =
                    widths[0] == widths[1] && widths[1] == widths[2] && widths[2] == widths[3];
                let uniform_color =
                    colors[0] == colors[1] && colors[1] == colors[2] && colors[2] == colors[3];
                let uniform_style = styles.iter().all(|style| *style == styles[0]);

                let collapsed_inner =
                    rect.w <= widths[1] + widths[3] || rect.h <= widths[0] + widths[2];
                let straight_segment = max_r <= 0.5
                    && (0..4).filter(|&side| widths[side] > 0.0).count() == 1
                    && (widths[0] > 0.0 && (rect.h - widths[0]).abs() < 0.01
                        || widths[3] > 0.0 && (rect.w - widths[3]).abs() < 0.01);
                if straight_segment {
                    let side = if widths[0] > 0.0 { 0 } else { 3 };
                    paint_straight_border_segment(
                        target,
                        *rect,
                        styles[side],
                        apply_opacity(&colors[side], alpha),
                        side == 0,
                        ts,
                        clip_mask,
                    );
                } else if !collapsed_inner
                    && uniform_width
                    && uniform_color
                    && uniform_style
                    && styles[0] == 4
                    && widths[0] >= 3.0
                    && colors[0].a != 0
                {
                    let stripe = widths[0] / 3.0;
                    let mut paint = Paint::default();
                    paint.set_color(to_sk_color(&apply_opacity(&colors[0], alpha)));
                    paint.anti_alias = true;
                    let mut stroke = tiny_skia::Stroke::default();
                    stroke.width = stripe;
                    // Two stripes and their intervening gap occupy the used border width.
                    for inset in [stripe / 2.0, widths[0] - stripe / 2.0] {
                        if let Some(path) = rounded_rect_path_corners_xy(
                            rect.x + inset,
                            rect.y + inset,
                            rect.w - inset * 2.0,
                            rect.h - inset * 2.0,
                            radii.map(|radius| (radius - inset).max(0.0)),
                            radii_y.map(|radius| (radius - inset).max(0.0)),
                        ) {
                            target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                        }
                    }
                } else if !collapsed_inner
                    && uniform_width
                    && uniform_color
                    && uniform_style
                    && matches!(styles[0], 2 | 3)
                    && widths[0] > 0.0
                    && colors[0].a != 0
                {
                    let half = widths[0] / 2.0;
                    if let Some(path) = rounded_rect_path_corners_xy(
                        rect.x + half,
                        rect.y + half,
                        rect.w - widths[0],
                        rect.h - widths[0],
                        radii.map(|radius| (radius - half).max(0.0)),
                        radii_y.map(|radius| (radius - half).max(0.0)),
                    ) {
                        paint_patterned_border_path(
                            target,
                            &path,
                            widths[0],
                            styles[0],
                            apply_opacity(&colors[0], alpha),
                            ts,
                            clip_mask,
                        );
                    }
                } else if max_r > 0.5 && collapsed_inner {
                    // Border-only shapes have no centerline to stroke. Keep
                    // their side wedges and clip them to the rounded exterior.
                    if let (Some(path), Some(mut mask)) = (
                        rounded_rect_path_corners_xy(
                            rect.x, rect.y, rect.w, rect.h, *radii, *radii_y,
                        ),
                        tiny_skia::Mask::new(pw, ph),
                    ) {
                        mask.fill_path(&path, FillRule::Winding, true, ts);
                        if let Some(clip) = clip_mask {
                            for (pixel, clip) in mask.data_mut().iter_mut().zip(clip.data()) {
                                *pixel = ((*pixel as u16 * *clip as u16 + 127) / 255) as u8;
                            }
                        }
                        for side in 0..4 {
                            if widths[side] <= 0.0 || colors[side].a == 0 {
                                continue;
                            }
                            if let Some(path) = border_side_path(*rect, *widths, side) {
                                let mut paint = Paint::default();
                                paint.set_color(to_sk_color(&apply_opacity(&colors[side], alpha)));
                                target.fill_path(&path, &paint, FillRule::Winding, ts, Some(&mask));
                            }
                        }
                    }
                } else if max_r > 0.5
                    && uniform_width
                    && uniform_color
                    && uniform_style
                    && styles[0] == 1
                    && widths[0] > 0.0
                    && colors[0].a != 0
                {
                    let bw = widths[0];
                    let half = bw / 2.0;
                    // Inset the path by half the border width so the stroke straddles the edge
                    if let Some(path) = rounded_rect_path_corners(
                        rect.x + half,
                        rect.y + half,
                        rect.w - bw,
                        rect.h - bw,
                        (radii[0] - half).max(0.0),
                        (radii[1] - half).max(0.0),
                        (radii[2] - half).max(0.0),
                        (radii[3] - half).max(0.0),
                        (radii_y[0] - half).max(0.0),
                        (radii_y[1] - half).max(0.0),
                        (radii_y[2] - half).max(0.0),
                        (radii_y[3] - half).max(0.0),
                    ) {
                        let mut paint = Paint::default();
                        paint.set_color(to_sk_color(&apply_opacity(&colors[0], alpha)));
                        paint.anti_alias = true;
                        let mut stroke = tiny_skia::Stroke::default();
                        stroke.width = bw;
                        target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                    }
                } else if max_r > 0.5 && widths.iter().any(|w| *w > 0.0) {
                    for side in 0..4 {
                        let bw = widths[side];
                        let color = colors[side];
                        if bw <= 0.0 || color.a == 0 {
                            continue;
                        }
                        let half = bw / 2.0;
                        if let Some(path) = rounded_rect_side_path(
                            side,
                            rect.x + half,
                            rect.y + half,
                            rect.w - bw,
                            rect.h - bw,
                            [
                                (radii[0] - half).max(0.0),
                                (radii[1] - half).max(0.0),
                                (radii[2] - half).max(0.0),
                                (radii[3] - half).max(0.0),
                            ],
                            [
                                (radii_y[0] - half).max(0.0),
                                (radii_y[1] - half).max(0.0),
                                (radii_y[2] - half).max(0.0),
                                (radii_y[3] - half).max(0.0),
                            ],
                        ) {
                            if matches!(styles[side], 2 | 3) {
                                paint_patterned_border_path(
                                    target,
                                    &path,
                                    bw,
                                    styles[side],
                                    apply_opacity(&color, alpha),
                                    ts,
                                    clip_mask,
                                );
                                continue;
                            }
                            let mut paint = Paint::default();
                            paint.set_color(to_sk_color(&apply_opacity(&color, alpha)));
                            paint.anti_alias = true;
                            let mut stroke = tiny_skia::Stroke::default();
                            stroke.width = bw;
                            target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                        }
                    }
                } else {
                    for side in 0..4 {
                        if widths[side] <= 0.0 || colors[side].a == 0 {
                            continue;
                        }
                        if matches!(styles[side], 4..=8) && !collapsed_inner {
                            paint_square_border_bands(
                                target,
                                *rect,
                                *widths,
                                side,
                                styles[side],
                                apply_opacity(&colors[side], alpha),
                                ts,
                                clip_mask,
                            );
                            continue;
                        }
                        if matches!(styles[side], 2 | 3) && !collapsed_inner {
                            let half = widths[side] / 2.0;
                            if let Some(path) = rounded_rect_side_path(
                                side,
                                rect.x + half,
                                rect.y + half,
                                rect.w - widths[side],
                                rect.h - widths[side],
                                [0.0; 4],
                                [0.0; 4],
                            ) {
                                paint_patterned_border_path(
                                    target,
                                    &path,
                                    widths[side],
                                    styles[side],
                                    apply_opacity(&colors[side], alpha),
                                    ts,
                                    clip_mask,
                                );
                            }
                            continue;
                        }
                        if let Some(path) = border_side_path(*rect, *widths, side) {
                            let mut paint = Paint::default();
                            paint.set_color(to_sk_color(&apply_opacity(&colors[side], alpha)));
                            paint.anti_alias = true;
                            target.fill_path(&path, &paint, FillRule::Winding, ts, clip_mask);
                        }
                    }
                }
            }

            PaintCmd::BorderImage {
                rect,
                widths,
                slices,
                repeat_x_mode,
                repeat_y_mode,
                fill_center,
                data,
            } => {
                let (rgba, iw, ih) = match data {
                    ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
                    ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
                };
                if iw == 0 || ih == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                draw_border_image_stretch(
                    target,
                    rgba,
                    iw,
                    ih,
                    rect,
                    widths,
                    slices,
                    *repeat_x_mode,
                    *repeat_y_mode,
                    *fill_center,
                    ts,
                    clip_mask,
                );
            }

            PaintCmd::Image { rect, data } => {
                let (rgba, iw, ih) = match data {
                    ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
                    ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
                };
                if iw == 0 || ih == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                if let Some(img_pixmap) = tiny_skia::PixmapRef::from_bytes(rgba, iw, ih) {
                    let sx = rect.w / iw as f32;
                    let sy = rect.h / ih as f32;
                    let img_ts = ts.pre_translate(rect.x, rect.y).pre_scale(sx, sy);
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    if clip_mask.is_none()
                        && blit_opaque_unscaled_image(target, rgba, iw, ih, img_ts)
                    {
                        continue;
                    }
                    if let Some(mask) = clip_mask {
                        if blit_opaque_scaled_image(target, rgba, iw, ih, img_ts, mask) {
                            continue;
                        }
                    }
                    let paint = tiny_skia::PixmapPaint {
                        quality: tiny_skia::FilterQuality::Bilinear,
                        ..tiny_skia::PixmapPaint::default()
                    };
                    target.draw_pixmap(0, 0, img_pixmap, &paint, img_ts, clip_mask);
                }
            }

            PaintCmd::Text {
                x,
                y,
                text,
                font_family,
                font_size,
                font_weight,
                font_style,
                font_stretch,
                line_height,
                color,
                letter_spacing,
                word_spacing,
                small_caps,
                decoration,
            } => {
                let text_gradient = if color.a == 0 {
                    text_gradient_stack
                        .last()
                        .and_then(|gradient| TextGradientSampler::new(gradient, ts))
                } else {
                    None
                };
                let phys_x = ts.sx * *x + ts.kx * *y + ts.tx;
                let phys_y = ts.ky * *x + ts.sy * *y + ts.ty;

                // Skip text that is wildly off-screen (handles text-indent:-9999px)
                if phys_x < -20000.0
                    || phys_x > (pw as f32 + 20000.0)
                    || phys_y < -20000.0
                    || phys_y > (ph as f32 + 20000.0)
                {
                    continue;
                }

                if let Some((ref mut fs, ref mut sc)) = text_ctx {
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    if transform_stack.is_empty() {
                        let (text_scale, text_x, text_y) = transformed_text_origin(&ts, *x, *y);
                        draw_text_cmd_with_gradient(
                            target,
                            *fs,
                            *sc,
                            text_scale,
                            text_x,
                            text_y,
                            text,
                            font_family,
                            *font_size,
                            *font_weight,
                            *font_style,
                            *font_stretch,
                            *line_height,
                            color,
                            decoration,
                            *letter_spacing,
                            *word_spacing,
                            *small_caps,
                            clip_mask,
                            text_gradient.as_ref(),
                        );
                    } else {
                        draw_transformed_text_cmd(
                            target,
                            *fs,
                            *sc,
                            scale,
                            ts,
                            *x,
                            *y,
                            text,
                            font_family,
                            *font_size,
                            *font_weight,
                            *font_style,
                            *font_stretch,
                            *line_height,
                            color,
                            decoration,
                            *letter_spacing,
                            *word_spacing,
                            *small_caps,
                            clip_mask,
                        );
                    }
                }
            }

            PaintCmd::PushClip {
                rect,
                radius,
                radius_y,
            } => {
                // Unlike glyph ink, a clip cannot paint outside its bounds.
                // Do not rasterize neighboring rows just because text culling
                // reserves a generous margin. Filters can spread that content.
                if rect.w <= 0.0
                    || rect.h <= 0.0
                    || (filter_stack.is_empty()
                        && transformed_bounds_to_viewport(ts, *rect, scale).is_some_and(|bounds| {
                            rect_outside_view(
                                bounds,
                                -inv_scale,
                                -inv_scale,
                                view_w + inv_scale,
                                view_h + inv_scale,
                            )
                        }))
                {
                    skip_clip_depth = 1;
                    continue;
                }
                if transform_depth == 0 {
                    if rect.right() < vis_left
                        || rect.x > vis_right
                        || rect.bottom() < vis_top
                        || rect.y > vis_bot
                    {
                        skip_clip_depth = 1;
                        continue;
                    }
                } else if let Some(bounds) = transformed_bounds_to_viewport(ts, *rect, scale) {
                    if rect_outside_view(bounds, clip_left, clip_top, clip_right, clip_bottom) {
                        skip_clip_depth = 1;
                        continue;
                    }
                }
                clip_stack.push(*rect);
                // Build a clip mask from the clip rect. A clip pushed inside a
                // transformed stacking context lives in that same transformed
                // coordinate space, just like the paint commands it clips.
                let parent = clip_mask_stack.last().and_then(|m| m.as_ref());
                let mask = if transform_depth == 0
                    && simple_clip_contains_viewport(
                        rect,
                        radius,
                        radius_y,
                        pw,
                        ph,
                        scale,
                        active_scroll_x,
                        active_scroll_y,
                    ) {
                    parent.cloned()
                } else {
                    clip_cache.rect(rect, radius, radius_y, pw, ph, ts, parent)
                };
                clip_mask_stack.push(mask);
            }
            PaintCmd::PushClipPath { points, even_odd } => {
                let bounds = polygon_bounds(points).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
                if transform_depth == 0 {
                    if bounds.right() < vis_left
                        || bounds.x > vis_right
                        || bounds.bottom() < vis_top
                        || bounds.y > vis_bot
                    {
                        skip_clip_depth = 1;
                        continue;
                    }
                } else if let Some(bounds) = transformed_bounds_to_viewport(ts, bounds, scale) {
                    if rect_outside_view(bounds, clip_left, clip_top, clip_right, clip_bottom) {
                        skip_clip_depth = 1;
                        continue;
                    }
                }
                clip_stack.push(bounds);
                let mut mask = if transform_depth > 0 {
                    build_polygon_clip_mask_with_transform(points, *even_odd, pw, ph, ts)
                } else {
                    build_polygon_clip_mask(
                        points,
                        *even_odd,
                        pw,
                        ph,
                        scale,
                        active_scroll_x,
                        active_scroll_y,
                    )
                };
                if let (Some(m), Some(prev)) =
                    (&mut mask, clip_mask_stack.last().and_then(|x| x.as_ref()))
                {
                    for (dst, src) in m.data_mut().iter_mut().zip(prev.mask.data().iter()) {
                        *dst = (*dst as u16 * *src as u16 / 255) as u8;
                    }
                }
                clip_mask_stack.push(mask.map(|m| clip_cache.share(m)));
            }
            PaintCmd::PushClipSvgPath {
                path,
                fill_rule,
                origin,
            } => {
                let path_bounds = path.bounds();
                let bounds = Rect::new(
                    origin.0 + path_bounds.x(),
                    origin.1 + path_bounds.y(),
                    path_bounds.width(),
                    path_bounds.height(),
                );
                if transform_depth == 0 {
                    if bounds.right() < vis_left
                        || bounds.x > vis_right
                        || bounds.bottom() < vis_top
                        || bounds.y > vis_bot
                    {
                        skip_clip_depth = 1;
                        continue;
                    }
                } else if let Some(view_bounds) = transformed_bounds_to_viewport(ts, bounds, scale)
                {
                    if rect_outside_view(view_bounds, clip_left, clip_top, clip_right, clip_bottom)
                    {
                        skip_clip_depth = 1;
                        continue;
                    }
                }
                clip_stack.push(bounds);
                let mut mask = tiny_skia::Mask::new(pw, ph);
                if let Some(mask) = &mut mask {
                    let transform = ts.pre_translate(origin.0, origin.1);
                    mask.fill_path(path.as_ref(), *fill_rule, true, transform);
                    if let Some(parent) = clip_mask_stack.last().and_then(|entry| entry.as_ref()) {
                        for (dst, src) in mask.data_mut().iter_mut().zip(parent.mask.data()) {
                            *dst = (*dst as u16 * *src as u16 / 255) as u8;
                        }
                    }
                }
                clip_mask_stack.push(mask.map(|mask| clip_cache.share(mask)));
            }
            PaintCmd::PopClip => {
                clip_stack.pop();
                clip_mask_stack.pop();
            }

            PaintCmd::PushOpacity { alpha } => {
                let _opacity_timing = crate::profile::is_enabled()
                    .then(|| crate::profile::span(crate::profile::Phase::RasterOpacityPush));
                if let Some((mut layer_pixmap, dirty_region)) = opacity_surface_pool
                    .pop()
                    .or_else(|| Pixmap::new(pw, ph).map(|pm| (pm, None)))
                {
                    if let Some(region) = dirty_region {
                        clear_layer_region(&mut layer_pixmap, region);
                    }
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 0,
                        alpha: alpha.clamp(0.0, 1.0),
                        has_content: false,
                        paint_bounds: None,
                    });
                }
            }
            PaintCmd::PopOpacity => {
                let _opacity_timing = crate::profile::is_enabled()
                    .then(|| crate::profile::span(crate::profile::Phase::RasterOpacityPop));
                if let Some(layer) = layer_stack.pop() {
                    let region = layer
                        .has_content
                        .then(|| layer_pixel_region(layer.paint_bounds, pw, ph))
                        .flatten();
                    if !layer.has_content || layer.alpha == 0.0 {
                        opacity_surface_pool.push((layer.pixmap, region));
                        continue;
                    }
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    if let Some(region) = region {
                        composite_opacity_region(target, &layer.pixmap, layer.alpha, region);
                    }
                    opacity_surface_pool.push((layer.pixmap, region));
                }
            }

            PaintCmd::PushTransform {
                node_id,
                transform: m,
            } => {
                // Apply CSS transform by modifying the global transform matrix.
                // The transform matrix m = [a,b,c,d,e,f] is a 2D affine transform
                // that already includes translate-to-origin and translate-back.
                let m = transform_overrides
                    .and_then(|overrides| overrides.get(node_id))
                    .unwrap_or(m);
                let css_t = Transform::from_row(m[0], m[1], m[2], m[3], m[4], m[5]);
                let new_ts = ts.pre_concat(css_t);
                // Push old ts onto a stack so we can restore it
                transform_stack.push(ts);
                ts = new_ts;
            }
            PaintCmd::PopTransform => {
                if let Some(old_ts) = transform_stack.pop() {
                    ts = old_ts;
                }
            }

            PaintCmd::PushFilter { filters } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    // Store filter ops encoded in the blend_mode field won't work,
                    // so we store them separately via a filter_stack
                    filter_stack.push(filters.clone());
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 254,
                        alpha: 1.0,
                        has_content: false,
                        paint_bounds: None,
                    });
                }
            }
            PaintCmd::PopFilter => {
                let filters = filter_stack.pop().unwrap_or_default();
                if let Some(layer) = layer_stack.pop() {
                    if !layer.has_content {
                        continue;
                    }
                    let mut pm = layer.pixmap;
                    if !pm.pixels().iter().any(|pixel| pixel.alpha() != 0) {
                        continue;
                    }
                    for (filter_type, value, dx, dy, color) in &filters {
                        if *filter_type == 9 {
                            crate::canvas::effects::drop_shadow(&mut pm, *dx, *dy, *value, *color);
                        } else {
                            apply_pixel_filter(&mut pm, *filter_type, *value);
                        }
                    }
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    target.draw_pixmap(
                        0,
                        0,
                        pm.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
            }
            PaintCmd::BackdropFilter {
                rect,
                radii,
                radii_y,
                filters,
            } => {
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let margin = filters
                    .iter()
                    .fold(0.0_f32, |margin, (kind, value, dx, dy, _)| {
                        if *kind == 0 {
                            margin + value.max(0.0) * 4.0
                        } else if *kind == 9 {
                            margin + dx.abs().max(dy.abs()) + value.max(0.0) * 4.0
                        } else {
                            margin
                        }
                    });
                let Some(bounds) = transformed_bounds_to_viewport(ts, *rect, 1.0) else {
                    continue;
                };
                let margin = margin * scale;
                let left = (bounds.x - margin).floor().clamp(0.0, pw as f32) as i32;
                let top = (bounds.y - margin).floor().clamp(0.0, ph as f32) as i32;
                let right = (bounds.right() + margin).ceil().clamp(0.0, pw as f32) as i32;
                let bottom = (bounds.bottom() + margin).ceil().clamp(0.0, ph as f32) as i32;
                let Some(region) = tiny_skia::IntRect::from_ltrb(left, top, right, bottom) else {
                    continue;
                };
                let Some(mut backdrop) = target.clone_rect(region) else {
                    continue;
                };
                for (filter_type, value, dx, dy, color) in filters {
                    if *filter_type == 9 {
                        crate::canvas::effects::drop_shadow(
                            &mut backdrop,
                            *dx,
                            *dy,
                            *value,
                            *color,
                        );
                    } else {
                        apply_pixel_filter(&mut backdrop, *filter_type, *value);
                    }
                }
                let parent = clip_mask_stack.last().and_then(|entry| entry.as_ref());
                let mask = clip_cache.rect(rect, radii, radii_y, pw, ph, ts, parent);
                target.draw_pixmap(
                    left,
                    top,
                    backdrop.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    Transform::identity(),
                    mask.as_ref().map(|mask| mask.mask.as_ref()),
                );
            }
            PaintCmd::PushMask {
                rect,
                no_clip,
                origin,
                tile,
                data,
                luminance,
                repeat_x_mode,
                repeat_y_mode,
            } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    mask_stack.push(vec![MaskLayer {
                        clip: *rect,
                        no_clip: *no_clip,
                        origin: *origin,
                        tile: *tile,
                        data: Some(data.clone()),
                        luminance: *luminance,
                        repeat_x_mode: *repeat_x_mode,
                        repeat_y_mode: *repeat_y_mode,
                        composite: 0,
                    }]);
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 253,
                        alpha: 1.0,
                        has_content: false,
                        paint_bounds: None,
                    });
                }
            }
            PaintCmd::PushMaskGroup { layers } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    mask_stack.push(
                        layers
                            .iter()
                            .map(|mask| MaskLayer {
                                clip: mask.rect,
                                no_clip: mask.no_clip,
                                origin: mask.origin,
                                tile: mask.tile,
                                data: mask.data.clone(),
                                luminance: mask.luminance,
                                repeat_x_mode: mask.repeat_x_mode,
                                repeat_y_mode: mask.repeat_y_mode,
                                composite: mask.composite,
                            })
                            .collect(),
                    );
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 253,
                        alpha: 1.0,
                        has_content: false,
                        paint_bounds: None,
                    });
                }
            }
            PaintCmd::PopMask => {
                if let Some(layer) = layer_stack.pop() {
                    if let Some(masks) = mask_stack.pop() {
                        if !layer.has_content {
                            continue;
                        }
                        let target = layer_stack
                            .last_mut()
                            .map(|l| &mut l.pixmap)
                            .unwrap_or(pixmap);
                        composite_masked_layers(
                            target,
                            &layer.pixmap,
                            &masks,
                            scale,
                            active_scroll_x,
                            active_scroll_y,
                        );
                    }
                }
            }
            PaintCmd::PushBlendMode { mode } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: *mode,
                        alpha: 1.0,
                        has_content: false,
                        paint_bounds: None,
                    });
                }
            }
            PaintCmd::PopBlendMode => {
                if let Some(layer) = layer_stack.pop() {
                    if !layer.has_content {
                        continue;
                    }
                    // Composite into the current stacking context, not always
                    // the root pixmap.
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    blend_composite(target, &layer.pixmap, layer.blend_mode);
                }
            }

            PaintCmd::BoxShadow {
                rect,
                color,
                offset_x,
                offset_y,
                blur,
                spread,
                inset,
                radii,
                radii_y,
            } => {
                let alpha = 1.0;
                if *inset {
                    let c = apply_opacity(color, alpha);
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    let mut paint = Paint::default();
                    paint.set_color(to_sk_color(&c));
                    let blurred = if *blur <= 0.0 {
                        false
                    } else if transform_depth != 0 {
                        paint_transformed_blurred_inset_box_shadow(
                            target, *rect, *offset_x, *offset_y, *blur, *spread, *radii, *radii_y,
                            &paint, scale, ts, clip_mask,
                        )
                    } else {
                        paint_blurred_inset_box_shadow(
                            target,
                            *rect,
                            *offset_x,
                            *offset_y,
                            *blur,
                            *spread,
                            *radii,
                            *radii_y,
                            &paint,
                            scale,
                            active_scroll_x,
                            active_scroll_y,
                            clip_mask,
                        )
                    };
                    if !blurred {
                        fill_inset_box_shadow_shape(
                            target, *rect, *offset_x, *offset_y, *blur, *spread, *radii, *radii_y,
                            &paint, ts, clip_mask,
                        );
                    }
                } else {
                    let sr = Rect::new(
                        rect.x + offset_x - spread,
                        rect.y + offset_y - spread,
                        rect.w + spread * 2.0,
                        rect.h + spread * 2.0,
                    );
                    if sr.w <= 0.0 || sr.h <= 0.0 {
                        continue;
                    }
                    let c = apply_opacity(color, alpha);
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    if *blur > 0.0 {
                        if transform_depth == 0
                            && paint_cached_outer_shadow(
                                target,
                                *rect,
                                sr,
                                *radii,
                                *radii_y,
                                *blur,
                                *spread,
                                c,
                                scale,
                                active_scroll_x,
                                active_scroll_y,
                                clip_mask,
                            )
                        {
                            continue;
                        }
                        if let Some(shadow_bounds) = transformed_bounds_to_viewport(ts, sr, scale) {
                            let shadow_pad = (*blur * scale * 4.0 + 4.0).ceil();
                            let dev_left =
                                (shadow_bounds.x * scale - shadow_pad).floor().max(0.0) as u32;
                            let dev_top =
                                (shadow_bounds.y * scale - shadow_pad).floor().max(0.0) as u32;
                            let dev_right =
                                ((shadow_bounds.x + shadow_bounds.w) * scale + shadow_pad)
                                    .ceil()
                                    .min(pw as f32)
                                    .max(dev_left as f32) as u32;
                            let dev_bottom =
                                ((shadow_bounds.y + shadow_bounds.h) * scale + shadow_pad)
                                    .ceil()
                                    .min(ph as f32)
                                    .max(dev_top as f32) as u32;
                            let local_w = dev_right.saturating_sub(dev_left);
                            let local_h = dev_bottom.saturating_sub(dev_top);
                            if local_w > 0 && local_h > 0 {
                                let mut local_ts = ts;
                                local_ts.tx -= dev_left as f32;
                                local_ts.ty -= dev_top as f32;
                                let key = ShadowRasterKey {
                                    outer: [
                                        sr.x.to_bits(),
                                        sr.y.to_bits(),
                                        sr.w.to_bits(),
                                        sr.h.to_bits(),
                                    ],
                                    interior: [
                                        rect.x.to_bits(),
                                        rect.y.to_bits(),
                                        rect.w.to_bits(),
                                        rect.h.to_bits(),
                                    ],
                                    radii: std::array::from_fn(|i| {
                                        if i < 4 {
                                            radii[i].to_bits()
                                        } else {
                                            radii_y[i - 4].to_bits()
                                        }
                                    }),
                                    transform: [
                                        local_ts.sx.to_bits(),
                                        local_ts.kx.to_bits(),
                                        local_ts.ky.to_bits(),
                                        local_ts.sy.to_bits(),
                                        local_ts.tx.to_bits(),
                                        local_ts.ty.to_bits(),
                                    ],
                                    size: [local_w, local_h],
                                    blur: (*blur * scale).to_bits(),
                                    spread: spread.to_bits(),
                                    color: [c.r, c.g, c.b, c.a],
                                };
                                let cached =
                                    SHADOW_RASTER_CACHE.with(|cache| cache.borrow_mut().get(&key));
                                let layer = cached.or_else(|| {
                                    let mut layer = Pixmap::new(local_w, local_h)?;
                                    let mut paint = Paint::default();
                                    paint.set_color(to_sk_color(&c));
                                    let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
                                    if max_r > 0.5 {
                                        let (expanded_radii, expanded_radii_y) =
                                            outer_shadow_radii(*rect, *radii, *radii_y, *spread);
                                        if let Some(path) = rounded_rect_path_corners_xy(
                                            sr.x,
                                            sr.y,
                                            sr.w,
                                            sr.h,
                                            expanded_radii,
                                            expanded_radii_y,
                                        ) {
                                            layer.fill_path(
                                                &path,
                                                &paint,
                                                FillRule::Winding,
                                                local_ts,
                                                None,
                                            );
                                        }
                                    } else if let Some(r) =
                                        SkRect::from_xywh(sr.x, sr.y, sr.w, sr.h)
                                    {
                                        layer.fill_rect(r, &paint, local_ts, None);
                                    }
                                    crate::canvas::blur_pixmap(&mut layer, *blur * scale);
                                    clear_outer_shadow_interior(
                                        &mut layer, *rect, *radii, *radii_y, local_ts,
                                    );
                                    let layer = Arc::new(layer);
                                    SHADOW_RASTER_CACHE.with(|cache| {
                                        cache.borrow_mut().insert(key, layer.clone())
                                    });
                                    Some(layer)
                                });
                                if let Some(layer) = layer {
                                    target.draw_pixmap(
                                        dev_left as i32,
                                        dev_top as i32,
                                        layer.as_ref().as_ref(),
                                        &tiny_skia::PixmapPaint::default(),
                                        Transform::identity(),
                                        clip_mask,
                                    );
                                }
                            }
                        } else if let Some(mut layer) = Pixmap::new(pw, ph) {
                            let mut paint = Paint::default();
                            paint.set_color(to_sk_color(&c));
                            let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
                            if max_r > 0.5 {
                                let (expanded_radii, expanded_radii_y) =
                                    outer_shadow_radii(*rect, *radii, *radii_y, *spread);
                                if let Some(path) = rounded_rect_path_corners_xy(
                                    sr.x,
                                    sr.y,
                                    sr.w,
                                    sr.h,
                                    expanded_radii,
                                    expanded_radii_y,
                                ) {
                                    layer.fill_path(
                                        &path,
                                        &paint,
                                        FillRule::Winding,
                                        ts,
                                        clip_mask,
                                    );
                                }
                            } else if let Some(r) = SkRect::from_xywh(sr.x, sr.y, sr.w, sr.h) {
                                layer.fill_rect(r, &paint, ts, clip_mask);
                            }
                            crate::canvas::blur_pixmap(&mut layer, *blur * scale);
                            clear_outer_shadow_interior(&mut layer, *rect, *radii, *radii_y, ts);
                            target.draw_pixmap(
                                0,
                                0,
                                layer.as_ref(),
                                &tiny_skia::PixmapPaint::default(),
                                Transform::identity(),
                                clip_mask,
                            );
                        }
                    } else {
                        let mut paint = Paint::default();
                        paint.set_color(to_sk_color(&c));
                        fill_outer_box_shadow_shape(
                            target, sr, *rect, *radii, *radii_y, *spread, &paint, ts, clip_mask,
                        );
                    }
                }
            }

            PaintCmd::BeginStackingContext { .. } => {}
            PaintCmd::EndStackingContext => {}
            PaintCmd::BeginFixedPosition | PaintCmd::EndFixedPosition => {}

            PaintCmd::Gradient {
                rect,
                clip,
                repeat_x_mode,
                repeat_y_mode,
                gradient_type,
                angle,
                direction,
                radial_center_x,
                radial_center_y,
                radial_radius_x,
                radial_radius_y,
                stops,
                radii,
                radii_y,
                opacity: grad_opacity,
                blend_mode,
            } => {
                use tiny_skia::{LinearGradient, Point as SkPoint, RadialGradient, SpreadMode};
                if stops.len() < 2 {
                    continue;
                }
                let a2 = 1.0;
                let combined_opacity = a2 * grad_opacity;
                // `pw`/`ph` name the pixmap here; keep them before the gradient
                // box shadows them for a mask.
                let (mask_w, mask_h) = (pw, ph);

                let pw = rect.w;
                let ph = rect.h;
                if pw <= 0.0 || ph <= 0.0 {
                    continue;
                }

                let sk_stops = gradient_stops_with_transparent_hues(stops, combined_opacity);

                // The gradient image is the size of the POSITIONING area
                // (`background-origin`) and is drawn once per tile, so its
                // geometry is measured from each tile's own origin.
                let shader_for = |px: f32, py: f32| -> Option<tiny_skia::Shader<'static>> {
                    match gradient_type {
                        1 => {
                            // The gradient line (css-images-3 §3.4.1): it runs through
                            // the centre of the box in the direction of `angle` — 0deg
                            // points up, angles turn clockwise — and is long enough that
                            // its perpendicular endpoints touch the two opposite corners.
                            let used_angle = match direction {
                                GradientDirection::Angle(_) => *angle,
                                GradientDirection::Corner { x, y } => {
                                    let dx = *x as f32 * pw;
                                    let dy = *y as f32 * ph;
                                    dx.atan2(-dy).to_degrees().rem_euclid(360.0)
                                }
                            };
                            let rad = used_angle * std::f32::consts::PI / 180.0;
                            let dx = rad.sin();
                            let dy = -rad.cos();
                            let half = ((pw * dx).abs() + (ph * dy).abs()) / 2.0;
                            if half <= 0.0 {
                                return None;
                            }
                            let cx = px + pw / 2.0;
                            let cy = py + ph / 2.0;

                            LinearGradient::new(
                                SkPoint::from_xy(cx - dx * half, cy - dy * half),
                                SkPoint::from_xy(cx + dx * half, cy + dy * half),
                                sk_stops.clone(),
                                SpreadMode::Pad,
                                Transform::identity(),
                            )
                        }
                        2 => {
                            let cx = px + *radial_center_x;
                            let cy = py + *radial_center_y;
                            let rx = (*radial_radius_x).max(1.0);
                            let ry = (*radial_radius_y).max(1.0);
                            let r = rx.max(ry);
                            let center = SkPoint::from_xy(cx, cy);
                            let sx = rx / r;
                            let sy = ry / r;
                            let transform = Transform::from_translate(-cx, -cy)
                                .post_scale(1.0 / sx, 1.0 / sy)
                                .post_translate(cx, cy);
                            RadialGradient::new(
                                center,
                                0.0,
                                center,
                                r,
                                sk_stops.clone(),
                                SpreadMode::Pad,
                                transform,
                            )
                        }
                        _ => None,
                    }
                };

                // Like any background image, the gradient TILES across the
                // PAINTING area (`background-clip`) when `background-repeat`
                // allows it (css-backgrounds-3 §3.5): the strip under a
                // transparent border shows the next repetition, not a gap.
                // Without repetition only the positioning area is painted, and
                // either way nothing is drawn outside the painting area.
                let xs = background_axis_tiles(*repeat_x_mode, rect.x, pw, clip.x, clip.w.max(0.0));
                let ys = background_axis_tiles(*repeat_y_mode, rect.y, ph, clip.y, clip.h.max(0.0));
                let tiled = xs.len() > 1 || ys.len() > 1;

                let [r_tl, r_tr, r_br, r_bl] = radii;
                let max_r = (*r_tl).max(*r_tr).max(*r_br).max(*r_bl);
                // One tile keeps the corner rounding in the fill path, which is
                // exact. Several tiles share one mask instead: a per-tile path
                // would round every internal tile edge as well.
                let tile_mask = if tiled && max_r > 0.0 {
                    clip_cache.rect(
                        clip,
                        radii,
                        radii_y,
                        mask_w,
                        mask_h,
                        ts,
                        clip_mask_stack.last().and_then(|m| m.as_ref()),
                    )
                } else {
                    None
                };
                let tile_mask_ref = tile_mask.as_ref().map(|m| m.mask.as_ref()).or(clip_mask);

                // A degenerate positioning area next to a large painting area
                // would spin here, so the tile count is capped.
                const MAX_TILES: usize = 4096;
                if xs.len().saturating_mul(ys.len()) > MAX_TILES {
                    continue;
                }

                for (ty, tile_h) in ys {
                    for (tx, tile_w) in &xs {
                        // Only the part of the tile inside the painting area is drawn.
                        let fx = tx.max(clip.x);
                        let fy = ty.max(clip.y);
                        let fw = (tx + *tile_w).min(clip.right()) - fx;
                        let fh = (ty + tile_h).min(clip.bottom()) - fy;
                        if fw > 0.0 && fh > 0.0 {
                            if let Some(shader) = shader_for(*tx, ty) {
                                let mut paint = Paint::default();
                                paint.anti_alias = true;
                                paint.shader = shader;
                                if *blend_mode != 0 {
                                    if let Some(mut layer) = Pixmap::new(mask_w, mask_h) {
                                        if max_r > 0.0 && !tiled {
                                            if let Some(path) = rounded_rect_path_corners(
                                                fx, fy, fw, fh, *r_tl, *r_tr, *r_br, *r_bl,
                                                radii_y[0], radii_y[1], radii_y[2], radii_y[3],
                                            ) {
                                                layer.fill_path(
                                                    &path,
                                                    &paint,
                                                    FillRule::Winding,
                                                    ts,
                                                    clip_mask,
                                                );
                                            }
                                        } else if let Some(r) = SkRect::from_xywh(fx, fy, fw, fh) {
                                            layer.fill_rect(r, &paint, ts, tile_mask_ref);
                                        }
                                        let target = layer_stack
                                            .last_mut()
                                            .map(|l| &mut l.pixmap)
                                            .unwrap_or(pixmap);
                                        blend_composite(target, &layer, *blend_mode);
                                    }
                                } else {
                                    let target = layer_stack
                                        .last_mut()
                                        .map(|l| &mut l.pixmap)
                                        .unwrap_or(pixmap);
                                    if max_r > 0.0 && !tiled {
                                        if let Some(path) = rounded_rect_path_corners(
                                            fx, fy, fw, fh, *r_tl, *r_tr, *r_br, *r_bl, radii_y[0],
                                            radii_y[1], radii_y[2], radii_y[3],
                                        ) {
                                            target.fill_path(
                                                &path,
                                                &paint,
                                                FillRule::Winding,
                                                ts,
                                                clip_mask,
                                            );
                                        }
                                    } else if let Some(r) = SkRect::from_xywh(fx, fy, fw, fh) {
                                        target.fill_rect(r, &paint, ts, tile_mask_ref);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            PaintCmd::Outline {
                rect,
                width,
                color,
                style: _,
                offset: _,
                radii,
                radii_y,
            } => {
                let a2 = 1.0;
                let mut paint = Paint::default();
                paint.set_color(to_sk_color(&apply_opacity(color, a2)));
                paint.anti_alias = true;
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = *width;
                let (rx, ry) = reduce_corner_radii_xy(rect.w, rect.h, *radii, *radii_y);
                if let Some(path) =
                    rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, rx, ry)
                {
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                }
            }

            PaintCmd::ResizeGrip { rect, color, mode } => {
                let a2 = 1.0;
                let mut paint = Paint::default();
                paint.set_color(to_sk_color(&apply_opacity(color, a2)));
                paint.anti_alias = true;
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = 1.0;
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let mut pb = tiny_skia::PathBuilder::new();
                let right = rect.x + rect.w;
                let bottom = rect.y + rect.h;
                let size = crate::types::CSS_RESIZE_GRIP_PX
                    .min(rect.w)
                    .min(rect.h)
                    .max(0.0);
                if size > 2.0 {
                    let draw_diag = *mode == 1 || *mode == 2 || *mode == 3;
                    if draw_diag {
                        for inset in [3.0_f32, 6.0, 9.0] {
                            pb.move_to(right - inset, bottom - 1.0);
                            pb.line_to(right - 1.0, bottom - inset);
                        }
                    }
                    if let Some(path) = pb.finish() {
                        target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                    }
                }
            }

            PaintCmd::HorizontalRule { x1, y1, x2 } => {
                let mut paint = Paint::default();
                paint.set_color_rgba8(128, 128, 128, 255);
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = 1.0;
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(*x1, *y1);
                pb.line_to(*x2, *y1);
                if let Some(path) = pb.finish() {
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                }
            }

            PaintCmd::ListMarker {
                marker_type,
                text_align,
                x,
                y,
                size,
                color,
                text,
                image,
                font_family,
                font_size,
                font_weight,
                font_style,
                line_height,
            } => {
                let a2 = 1.0;
                let c = apply_opacity(color, a2);
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                match marker_type {
                    0 => {
                        // disc
                        let mut paint = Paint::default();
                        paint.set_color(to_sk_color(&c));
                        let mut pb = PathBuilder::new();
                        pb.push_circle(*x, *y, *size);
                        if let Some(path) = pb.finish() {
                            target.fill_path(&path, &paint, FillRule::Winding, ts, clip_mask);
                        }
                    }
                    1 => {
                        // circle
                        let mut paint = Paint::default();
                        paint.set_color(to_sk_color(&c));
                        let mut pb = PathBuilder::new();
                        pb.push_circle(*x, *y, *size);
                        if let Some(path) = pb.finish() {
                            let mut stroke = tiny_skia::Stroke::default();
                            stroke.width = 1.0;
                            target.stroke_path(&path, &paint, &stroke, ts, clip_mask);
                        }
                    }
                    2 => {
                        // square
                        let mut paint = Paint::default();
                        paint.set_color(to_sk_color(&c));
                        let half = size / 2.0;
                        if let Some(r) = SkRect::from_xywh(x - half, y - half, *size, *size) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                    3 => {
                        // text marker
                        if let Some((ref mut fs, ref mut sc)) = text_ctx {
                            let marker_x = if *text_align == crate::types::TextAlign::Right {
                                let width =
                                    crate::layout::inline_layout::measure_text_width_fs_attrs(
                                        fs,
                                        text,
                                        *font_size,
                                        cosmic_text::Weight(*font_weight),
                                        match font_style {
                                            1 => CTextStyle::Italic,
                                            2 => CTextStyle::Oblique,
                                            _ => CTextStyle::Normal,
                                        },
                                        1.0,
                                        font_family,
                                        crate::layout::inline_layout::stretch_from_percent(100.0),
                                    );
                                x - width
                            } else {
                                *x
                            };
                            let (text_scale, text_x, text_y) =
                                transformed_text_origin(&ts, marker_x, *y);
                            draw_text_cmd(
                                target,
                                *fs,
                                *sc,
                                text_scale,
                                text_x,
                                text_y,
                                text,
                                font_family,
                                *font_size,
                                *font_weight,
                                *font_style,
                                100.0,
                                *line_height,
                                &c,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
                                clip_mask,
                            );
                        }
                    }
                    4 => {
                        let side = (*size).max(*font_size * 0.75);
                        if let Some(image) = image {
                            let (rgba, iw, ih) = match image {
                                ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
                                ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
                            };
                            if iw > 0 && ih > 0 {
                                if let Some(img_pixmap) =
                                    tiny_skia::PixmapRef::from_bytes(rgba, iw, ih)
                                {
                                    let img_ts = ts.pre_translate(*x, *y);
                                    target.draw_pixmap(
                                        0,
                                        0,
                                        img_pixmap,
                                        &tiny_skia::PixmapPaint::default(),
                                        img_ts,
                                        clip_mask,
                                    );
                                }
                            }
                        } else {
                            let mut paint = Paint::default();
                            paint.set_color(to_sk_color(&c));
                            if let Some(r) = SkRect::from_xywh(*x, *y, side, side) {
                                target.fill_rect(r, &paint, ts, clip_mask);
                            }
                        }
                    }
                    _ => {}
                }
            }

            PaintCmd::FormElement {
                tag,
                input_type,
                rect,
                node_id,
                attributes,
                font_size,
                font_weight,
                font_family,
                color,
                text_indent,
                text_align,
                direction,
                placeholder_color,
                placeholder_typography,
                file_button_color,
                file_button_background,
                file_button_font_size,
                file_button_font_weight,
                file_button_font_family,
                checked,
                value,
                placeholder,
                input_cursor,
                appearance_none,
                vertical,
                options,
                selected,
                selected_all,
            } => {
                // CSS background/border/padding are drawn by the normal pipeline.
                // FormElement only draws the CONTENT: value text, check marks, radio dots, etc.
                let a2 = 1.0;
                let (form_scale, rect) = transformed_axis_aligned_rect(&ts, *rect);
                let scale = form_scale;
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let _ = (
                    node_id,
                    attributes,
                    input_cursor,
                    &options,
                    selected,
                    &selected_all,
                ); // suppress warnings

                match (tag.as_str(), input_type.as_str()) {
                    ("input", "checkbox") => {
                        if *appearance_none {
                            continue;
                        }
                        // Use Checkbox widget
                        let sz = rect.w.min(rect.h);
                        let bx = rect.x + (rect.w - sz) / 2.0;
                        let by = rect.y + (rect.h - sz) / 2.0;
                        let mut cb = crate::widgets::Checkbox::new("");
                        cb.checked = *checked;
                        cb.size = sz;
                        cb.paint(target, bx, by, scale);
                    }
                    ("input", "radio") => {
                        if *appearance_none {
                            continue;
                        }
                        // Use Radio widget
                        let sz = rect.w.min(rect.h);
                        let bx = rect.x + (rect.w - sz) / 2.0;
                        let by = rect.y + (rect.h - sz) / 2.0;
                        let mut rb = crate::widgets::Radio::new("");
                        rb.selected = *checked;
                        rb.size = sz;
                        rb.paint(target, bx, by, scale);
                    }
                    // **A LIST BOX, not a dropdown** — HTML §4.10.7: a
                    // `<select>` with `size` above one, or `multiple`, shows
                    // its options as ROWS instead of one closed value. Both
                    // spellings mean the same control; `size` alone was not
                    // enough, since `<select multiple>` defaults to a list.
                    //
                    // This was painted as a closed combobox whatever the
                    // markup said: a four-row list drew one row and a dropdown
                    // arrow, showing only the first option. The options and the
                    // selected index reach here on the display item precisely
                    // so the rows can be drawn.
                    // `<progress>` and `<meter>` — HTML §4.10.13/§4.10.14.
                    //
                    // Neither is expressible in CSS: the fill is a FRACTION of
                    // two attributes. With no arm here they fell to the generic
                    // text branch below and drew their value as a STRING, which
                    // is why the widget gallery showed `0.6` where a bar goes.
                    ("progress", _) | ("meter", _) => {
                        let attr = |name: &str| {
                            attributes
                                .iter()
                                .find(|(k, _)| k == name)
                                .and_then(|(_, v)| v.trim().parse::<f32>().ok())
                        };
                        // The defaults are the spec's: `max` is 1 for a
                        // progress bar and for a meter, `min` is 0.
                        let min = attr("min").unwrap_or(0.0);
                        let max = attr("max").unwrap_or(1.0).max(min);
                        let has_value = attributes.iter().any(|(k, _)| k == "value");
                        let value = attr("value").unwrap_or(min).clamp(min, max);
                        let span = max - min;

                        let mut gauge = crate::widgets::Gauge::new(if span > 0.0 {
                            (value - min) / span
                        } else {
                            0.0
                        });
                        gauge.width = rect.w;
                        gauge.height = rect.h;
                        // **A `<progress>` with no `value` is indeterminate**,
                        // which HTML distinguishes from `value="0"`. A meter
                        // has no such state — `value` is required.
                        gauge.indeterminate = tag == "progress" && !has_value;
                        if tag == "meter" {
                            gauge.band = crate::widgets::meter_band(
                                value,
                                min,
                                max,
                                attr("low").unwrap_or(min),
                                attr("high").unwrap_or(max),
                                attr("optimum").unwrap_or((min + max) / 2.0),
                            );
                        }
                        gauge.paint(target, rect.x, rect.y, scale);
                    }
                    // A list box is decided by DISPLAY SIZE (HTML §15.5.16),
                    // which defaults to 4 under `multiple` and 1 otherwise.
                    // The predicate here used to be `multiple || size > 1`,
                    // read off the raw attribute with Rust's own parser — close
                    // enough to agree most of the time, and wrong for
                    // `multiple size=1` (a multi-select DROP-DOWN) and for the
                    // lenient integer parsing HTML actually specifies.
                    ("select", _)
                        if attributes
                            .iter()
                            .find(|(k, _)| k == "size")
                            .and_then(|(_, v)| crate::html::forms::parse_non_negative_integer(v))
                            .unwrap_or(if attributes.iter().any(|(k, _)| k == "multiple") {
                                4
                            } else {
                                1
                            })
                            > 1 =>
                    {
                        let ts = Transform::from_scale(scale, scale);
                        if !*appearance_none {
                            // The box itself: the UA sheet gives a `<select>` a
                            // white field and a grey border, and a list box is the
                            // same field with rows in it.
                            let mut fill = Paint::default();
                            fill.anti_alias = true;
                            fill.set_color_rgba8(255, 255, 255, 255);
                            if let Some(r) = SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h) {
                                target.fill_rect(r, &fill, ts, None);
                            }
                        }

                        // Shared with the hit test, so a click cannot land on a
                        // row other than the one drawn here.
                        let line_h = crate::html::forms::list_box_row_height(*font_size);
                        let pad = crate::html::forms::LIST_BOX_PADDING;
                        for (i, label) in options.iter().enumerate() {
                            let row_y = rect.y + pad + i as f32 * line_h;
                            // Clip to the box: a list shows the rows that FIT
                            // and scrolls the rest, and drawing past the border
                            // would paint over whatever is beside it.
                            if row_y + line_h > rect.y + rect.h - pad {
                                break;
                            }
                            let mut text_color = apply_opacity(color, a2);
                            // EVERY selected row, not one index — a `multiple`
                            // list box can have several, and a fresh one has
                            // none at all.
                            if selected_all.get(i).copied().unwrap_or(false) {
                                // The selected row is a filled bar with
                                // reversed text, which is what every browser
                                // and every toolkit draws.
                                let mut bar = Paint::default();
                                bar.set_color_rgba8(0, 120, 215, 255);
                                if let Some(r) = SkRect::from_xywh(
                                    rect.x + 1.0,
                                    row_y,
                                    (rect.w - 2.0).max(0.0),
                                    line_h,
                                ) {
                                    target.fill_rect(r, &bar, ts, None);
                                }
                                text_color = crate::types::Color::rgba(255, 255, 255, 255);
                            }
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    rect.x + 4.0,
                                    row_y,
                                    label,
                                    font_family,
                                    *font_size,
                                    *font_weight,
                                    0,
                                    rect.w - 8.0,
                                    line_h,
                                    &text_color,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
                                    clip_mask,
                                );
                            }
                        }
                    }
                    ("select", _) => {
                        // The transformed rectangle is in scaled logical coordinates,
                        // while clip masks and glyph coverage use device pixels.
                        let mut control_clip = build_clip_mask_with_transform(
                            &rect,
                            &[0.0; 4],
                            &[0.0; 4],
                            pw,
                            ph,
                            Transform::from_scale(scale, scale),
                        );
                        if let (Some(control), Some(ancestor)) = (&mut control_clip, clip_mask) {
                            for (dst, src) in control.data_mut().iter_mut().zip(ancestor.data()) {
                                *dst = (*dst as u16 * *src as u16 / 255) as u8;
                            }
                        }
                        if !*appearance_none {
                            // Draw dropdown chevron arrow using the element's text color
                            let arrow_x = rect.x + rect.w - 14.0;
                            let arrow_y = rect.y + rect.h / 2.0;
                            let ts = Transform::from_scale(scale, scale);
                            let c = apply_opacity(color, a2);
                            let mut paint = Paint::default();
                            paint.anti_alias = true;
                            paint.set_color_rgba8(c.r, c.g, c.b, c.a);
                            let mut stroke = tiny_skia::Stroke::default();
                            stroke.width = 1.5;
                            let mut pb = PathBuilder::new();
                            pb.move_to(arrow_x - 4.0, arrow_y - 2.0);
                            pb.line_to(arrow_x, arrow_y + 2.0);
                            pb.line_to(arrow_x + 4.0, arrow_y - 2.0);
                            if let Some(path) = pb.finish() {
                                target.stroke_path(&path, &paint, &stroke, ts, None);
                            }
                        }
                        // Draw selected value text
                        let display_text = if value.is_empty() { placeholder } else { value };
                        if !display_text.is_empty() {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let c = apply_opacity(color, a2);
                                let line_h = *font_size * 1.2;
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    rect.x + 4.0,
                                    text_y,
                                    display_text,
                                    font_family,
                                    *font_size,
                                    *font_weight,
                                    0,
                                    100.0,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
                                    control_clip.as_ref().or(clip_mask),
                                );
                            }
                        }
                    }
                    // **`<input type=image>` is an image AND a submit button**
                    // (HTML §4.10.5.1.19). The image itself is painted by the
                    // `<img>` path — `is_image_element` is what lets it in —
                    // so all that is left here is the spec's fallback: "if the
                    // image is unavailable, the alt text is used". Without an
                    // arm it fell to the generic text branch and drew its
                    // VALUE, which for a submit button is the submission name,
                    // not anything a person should see.
                    ("input", "image") => {
                        // `src` alone: the resolved URL is a node FIELD now and
                        // the display list only carries content attributes.
                        let has_image = attributes.iter().any(|(k, _)| k == "src");
                        let alt = attributes
                            .iter()
                            .find(|(k, _)| k == "alt")
                            .map(|(_, v)| v.as_str())
                            .unwrap_or("");
                        // The alt is drawn only when there is no image to show
                        // — an image that HAS loaded is painted by the image
                        // command and must not have text over it.
                        if !alt.is_empty() && !has_image {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let c = apply_opacity(color, a2);
                                let line_h = *font_size * 1.2;
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    rect.x + 2.0,
                                    text_y,
                                    alt,
                                    font_family,
                                    *font_size,
                                    *font_weight,
                                    0,
                                    rect.w,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
                                    clip_mask,
                                );
                            }
                        }
                    }
                    // `<input type=file>` is a BUTTON plus the chosen file's
                    // name (HTML §4.10.5.1.18) — it fell to the generic arm and
                    // drew the value as bare text, which for an empty control
                    // is nothing at all.
                    ("input", "file") => {
                        let button_line_h = *file_button_font_size * 1.2;
                        let button_text_y = rect.y + (rect.h - button_line_h).max(0.0) / 2.0;
                        // Measured from the label so the chrome cannot clip its
                        // own word in a large font.
                        let label_w = crate::widgets::CHOOSE.chars().count() as f32
                            * *file_button_font_size
                            * 0.55;
                        let button_w = crate::widgets::FileButton::width_for(label_w).min(rect.w);
                        if !*appearance_none {
                            if file_button_background.a > 0 {
                                let bg = apply_opacity(file_button_background, a2);
                                let mut paint = Paint::default();
                                paint.anti_alias = true;
                                paint.set_color_rgba8(bg.r, bg.g, bg.b, bg.a);
                                if let Some(r) = SkRect::from_xywh(rect.x, rect.y, button_w, rect.h)
                                {
                                    target.fill_rect(
                                        r,
                                        &paint,
                                        Transform::from_scale(scale, scale),
                                        clip_mask,
                                    );
                                }
                            } else {
                                let mut button = crate::widgets::FileButton::new(button_w, rect.h);
                                button.disabled = attributes.iter().any(|(k, _)| k == "disabled");
                                button.paint(target, rect.x, rect.y, scale);
                            }
                        }
                        if let Some((ref mut fs, ref mut sc)) = text_ctx {
                            let button_color = apply_opacity(file_button_color, a2);
                            draw_text_cmd(
                                target,
                                *fs,
                                *sc,
                                scale,
                                rect.x + 8.0,
                                button_text_y,
                                crate::widgets::CHOOSE,
                                file_button_font_family,
                                *file_button_font_size,
                                *file_button_font_weight,
                                0,
                                button_w,
                                button_line_h,
                                &button_color,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
                                clip_mask,
                            );
                            // ⛔ The empty case is a LABEL, not the value: a
                            // file control with nothing chosen has `value ==
                            // ""`, and drawing this string from the value would
                            // be a control that submits "No file chosen".
                            let name = if value.is_empty() {
                                crate::widgets::NOTHING_CHOSEN
                            } else {
                                value
                            };
                            let line_h = *font_size * 1.2;
                            let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                            let c = apply_opacity(color, a2);
                            draw_text_cmd(
                                target,
                                *fs,
                                *sc,
                                scale,
                                rect.x + button_w + 8.0,
                                text_y,
                                name,
                                font_family,
                                *font_size,
                                *font_weight,
                                0,
                                (rect.w - button_w - 8.0).max(0.0),
                                line_h,
                                &c,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
                                clip_mask,
                            );
                        }
                    }
                    // The date and time family — a formatted field with a
                    // picker affordance. Five input types, one control.
                    ("input", _)
                        if crate::widgets::DateKind::for_input(input_type.as_str()).is_some() =>
                    {
                        let (kind, pattern) =
                            crate::widgets::DateKind::for_input(input_type.as_str())
                                .expect("guarded above");
                        let mut field = crate::widgets::DateField::new(kind, rect.w, rect.h);
                        field.disabled = attributes.iter().any(|(k, _)| k == "disabled");
                        field.paint(target, rect.x, rect.y, scale);
                        if let Some((ref mut fs, ref mut sc)) = text_ctx {
                            // An empty field shows the PATTERN, dimmed — the
                            // same treatment a placeholder gets, and what makes
                            // an empty date input tell you what it wants.
                            let mut c = apply_opacity(color, a2);
                            let shown = if value.is_empty() {
                                c.a = (c.a as f32 * 0.5) as u8;
                                pattern
                            } else {
                                value
                            };
                            let line_h = *font_size * 1.2;
                            let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                            let room =
                                (rect.w - crate::widgets::DateField::glyph_width(rect.h) - 4.0)
                                    .max(0.0);
                            draw_text_cmd(
                                target,
                                *fs,
                                *sc,
                                scale,
                                rect.x + 4.0,
                                text_y,
                                shown,
                                font_family,
                                *font_size,
                                *font_weight,
                                0,
                                room,
                                line_h,
                                &c,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
                                clip_mask,
                            );
                        }
                    }
                    // `<input type=color>` is a SWATCH, not a text field. It
                    // fell to the generic arm and rendered `#3366cc` as a
                    // string — the one thing this control never shows.
                    ("input", "color") => {
                        let mut swatch = crate::widgets::ColorSwatch::new(
                            crate::widgets::ColorSwatch::parse(value),
                        );
                        swatch.width = rect.w;
                        swatch.height = rect.h;
                        swatch.paint(target, rect.x, rect.y, scale);
                    }
                    ("input", "text")
                    | ("input", "tel")
                    | ("input", "email")
                    | ("input", "password")
                    | ("input", "search")
                    | ("input", "url")
                    | ("input", "number")
                    | ("input", "date")
                    | ("input", "month")
                    | ("input", "week")
                    | ("input", "time")
                    | ("input", "datetime-local")
                    | ("textarea", _) => {
                        // Draw value or placeholder text.
                        //
                        // ⛔ **A password field must not draw what it holds.**
                        // HTML §4.10.5.1.5: the value is "obscured so that
                        // people cannot read it" — so the characters are
                        // replaced one for one, which keeps the caret and the
                        // measured width honest. This arm drew the value
                        // verbatim, so `<input type=password value="hunter2">`
                        // rendered the password on screen.
                        //
                        // The PLACEHOLDER is not obscured: it is not the value,
                        // and every browser shows it.
                        let masked: String;
                        let transformed_placeholder = if value.is_empty() {
                            placeholder_typography.as_ref().map(|style| {
                                super::display_list_builder::apply_text_transform(
                                    placeholder,
                                    style.text_transform,
                                )
                            })
                        } else {
                            None
                        };
                        let display_text = if value.is_empty() {
                            transformed_placeholder.as_deref().unwrap_or(placeholder)
                        } else if input_type == "password" {
                            masked = value.chars().map(|_| '\u{2022}').collect();
                            &masked
                        } else {
                            value
                        };
                        let placeholder_alpha = if value.is_empty() {
                            placeholder_typography
                                .as_ref()
                                .map_or(1.0, |style| style.opacity.clamp(0.0, 1.0))
                        } else {
                            1.0
                        };
                        if !display_text.is_empty() && placeholder_alpha > 0.0 {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let typography = if value.is_empty() {
                                    placeholder_typography.as_ref()
                                } else {
                                    None
                                };
                                let text_font_size =
                                    typography.map_or(*font_size, |style| style.font_size);
                                let text_font_weight =
                                    typography.map_or(*font_weight, |style| style.font_weight);
                                let text_font_style =
                                    typography.map_or(0, |style| style.font_style);
                                let text_font_family = typography
                                    .map_or(font_family.as_str(), |style| {
                                        style.font_family.as_str()
                                    });
                                let text_font_stretch =
                                    typography.map_or(100.0, |style| style.font_stretch);
                                let letter_spacing =
                                    typography.map_or(0.0, |style| style.letter_spacing);
                                let word_spacing =
                                    typography.map_or(0.0, |style| style.word_spacing);
                                let c = if value.is_empty() {
                                    apply_opacity(placeholder_color, a2 * placeholder_alpha)
                                } else {
                                    apply_opacity(color, a2)
                                };
                                // Vertically center the text in the element
                                let line_h =
                                    typography.map_or(*font_size * 1.2, |style| style.line_height);
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                                let text_w =
                                    crate::layout::inline_layout::measure_text_width_fs_attrs(
                                        fs,
                                        display_text,
                                        text_font_size,
                                        cosmic_text::Weight(text_font_weight),
                                        match text_font_style {
                                            1 => CTextStyle::Italic,
                                            2 => CTextStyle::Oblique,
                                            _ => CTextStyle::Normal,
                                        },
                                        scale,
                                        text_font_family,
                                        crate::layout::inline_layout::stretch_from_percent(
                                            text_font_stretch,
                                        ),
                                    ) + letter_spacing
                                        * display_text.chars().count().saturating_sub(1) as f32
                                        + word_spacing
                                            * display_text.chars().filter(|ch| *ch == ' ').count()
                                                as f32;
                                let rtl = *direction == crate::types::Direction::RTL;
                                let alignment = match text_align {
                                    crate::types::TextAlign::Center => 0.5,
                                    crate::types::TextAlign::Right => 1.0,
                                    crate::types::TextAlign::Start if rtl => 1.0,
                                    crate::types::TextAlign::End if !rtl => 1.0,
                                    _ => 0.0,
                                };
                                let text_x = rect.x
                                    + (rect.w - text_w).max(0.0) * alignment
                                    + text_indent * scale * if rtl { -1.0 } else { 1.0 };
                                if let Some(shadow) =
                                    typography.and_then(|style| style.shadow.as_ref())
                                {
                                    paint_text_shadow(
                                        target,
                                        *fs,
                                        *sc,
                                        scale,
                                        text_x + shadow.offset_x,
                                        text_y + shadow.offset_y,
                                        &ShadowText {
                                            text: display_text,
                                            font_family: text_font_family,
                                            font_size: text_font_size,
                                            font_weight: text_font_weight,
                                            font_style: text_font_style,
                                            font_stretch: text_font_stretch,
                                            line_height: line_h,
                                            color: apply_opacity(
                                                &shadow.color,
                                                a2 * placeholder_alpha,
                                            ),
                                            blur: shadow.blur,
                                            letter_spacing,
                                            word_spacing,
                                            small_caps: false,
                                        },
                                        clip_mask,
                                        transform_depth == 0,
                                    );
                                }
                                let default_decoration =
                                    super::display_list::TextDecoration::default();
                                let decoration = typography
                                    .map_or(&default_decoration, |style| &style.decoration);
                                let faded_decoration;
                                let decoration = if placeholder_alpha < 1.0 {
                                    faded_decoration = {
                                        let mut decoration = decoration.clone();
                                        decoration.color =
                                            apply_opacity(&decoration.color, placeholder_alpha);
                                        decoration
                                    };
                                    &faded_decoration
                                } else {
                                    decoration
                                };
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    text_x,
                                    text_y,
                                    display_text,
                                    text_font_family,
                                    text_font_size,
                                    text_font_weight,
                                    text_font_style,
                                    text_font_stretch,
                                    line_h,
                                    &c,
                                    decoration,
                                    letter_spacing,
                                    word_spacing,
                                    false,
                                    clip_mask,
                                );
                            }
                        }
                        // **The spinner, drawn after the field's own text.**
                        // `<input type=number>` IS a text field with a stepper
                        // on it: the field is a CSS box with a text run, which
                        // the engine already draws, and the two arrows are the
                        // part no declaration expresses. Last, so the well
                        // covers a value long enough to reach it — which is
                        // what a browser does as well.
                        if input_type == "number" {
                            let mut stepper = crate::widgets::Stepper::new(rect.w, rect.h);
                            stepper.disabled = attributes.iter().any(|(k, _)| k == "disabled");
                            stepper.paint(target, rect.x, rect.y, scale);
                        }
                    }
                    ("input", "range") => {
                        // Use Slider widget
                        // ⛔ The SPEC's number parser, the same one the click
                        // path uses. Rust's `parse` rejects the trailing junk
                        // HTML's rules ignore, so `min="10 "` read as 0 here
                        // and as 10 in the hit test: the thumb drew in one
                        // place and landed in another.
                        let attr = |name: &str| {
                            attributes
                                .iter()
                                .find(|(k, _)| k == name)
                                .and_then(|(_, v)| crate::html::forms::parse_floating_point(v))
                                .map(|n| n as f32)
                        };
                        let min: f32 = attr("min").unwrap_or(0.0);
                        let max: f32 = attr("max").unwrap_or(100.0);
                        // The value has already been sanitized into range by
                        // the time it reaches paint, so its own fallback is the
                        // state's default rather than a bare 50.
                        let val: f32 = crate::html::forms::parse_floating_point(value)
                            .map(|n| n as f32)
                            .unwrap_or_else(|| {
                                if max < min {
                                    min
                                } else {
                                    min + (max - min) / 2.0
                                }
                            });
                        let mut slider = crate::widgets::Slider::new(min, max, val);
                        slider.width = rect.w;
                        slider.height = rect.h;
                        slider.vertical = *vertical;
                        slider.paint(target, rect.x, rect.y, scale);
                    }
                    // `<button>` takes its label from its CHILDREN, which the
                    // inline text pipeline lays out and draws. Nothing to do.
                    ("button", _) => {}
                    // An absent value gets the UA label; an explicitly empty
                    // value paints no label.
                    ("input", "submit") | ("input", "button") | ("input", "reset") => {
                        let label = if value.is_empty()
                            && !attributes
                                .iter()
                                .any(|(name, _)| name.eq_ignore_ascii_case("value"))
                        {
                            match input_type.as_str() {
                                "submit" => "Submit",
                                "reset" => "Reset",
                                _ => "",
                            }
                        } else {
                            value.as_str()
                        };
                        if !label.is_empty() {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let text_w =
                                    crate::layout::inline_layout::measure_text_width_fs_attrs(
                                        fs,
                                        label,
                                        *font_size,
                                        cosmic_text::Weight(*font_weight),
                                        CTextStyle::Normal,
                                        scale,
                                        font_family,
                                        crate::layout::inline_layout::stretch_from_percent(100.0),
                                    );
                                let line_h = *font_size * 1.2;
                                let c = apply_opacity(color, a2);
                                let label_x =
                                    rect.x + (rect.w - text_w).max(0.0) / 2.0 + text_indent * scale;
                                if label_x + text_w <= rect.x || label_x >= rect.x + rect.w {
                                    continue;
                                }
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    label_x,
                                    rect.y + (rect.h - line_h).max(0.0) / 2.0,
                                    label,
                                    font_family,
                                    *font_size,
                                    *font_weight,
                                    0,
                                    100.0,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
                                    clip_mask,
                                );
                            }
                        }
                    }
                    _ => {
                        // Other form elements: draw value text if present
                        let display_text = if value.is_empty() { placeholder } else { value };
                        if !display_text.is_empty() {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let c = apply_opacity(color, a2);
                                // Vertically center the text in the element
                                let line_h = *font_size * 1.2;
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                                let text_max_w = (rect.w - 4.0).max(0.0);
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    rect.x + 2.0,
                                    text_y,
                                    display_text,
                                    font_family,
                                    *font_size,
                                    *font_weight,
                                    0,
                                    text_max_w,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
                                    clip_mask,
                                );
                            }
                        }
                    }
                }
            }

            PaintCmd::TextShadow {
                x,
                y,
                text,
                font_family,
                font_size,
                font_weight,
                font_style,
                font_stretch,
                line_height,
                color,
                blur,
                letter_spacing,
                word_spacing,
                small_caps,
            } => {
                if let Some((ref mut fs, ref mut sc)) = text_ctx {
                    let (text_scale, text_x, text_y) = transformed_text_origin(&ts, *x, *y);
                    let target = layer_stack
                        .last_mut()
                        .map(|layer| &mut layer.pixmap)
                        .unwrap_or(pixmap);
                    paint_text_shadow(
                        target,
                        *fs,
                        *sc,
                        text_scale,
                        text_x,
                        text_y,
                        &ShadowText {
                            text,
                            font_family,
                            font_size: *font_size,
                            font_weight: *font_weight,
                            font_style: *font_style,
                            font_stretch: *font_stretch,
                            line_height: *line_height,
                            color: apply_opacity(color, 1.0),
                            blur: *blur,
                            letter_spacing: *letter_spacing,
                            word_spacing: *word_spacing,
                            small_caps: *small_caps,
                        },
                        clip_mask,
                        transform_depth == 0,
                    );
                }
            }

            PaintCmd::BackgroundImage {
                container: _,
                clip,
                data,
                size_mode: _,
                draw_w,
                draw_h,
                pos_x,
                pos_y,
                repeat_x_mode,
                repeat_y_mode,
                radii,
                radii_y,
                blend_mode,
            } => {
                // Draw the background image, positioned in `container` and
                // clipped to `clip`.
                let (rgba, iw, ih) = match data {
                    ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
                    ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
                };
                if iw == 0 || ih == 0 || *draw_w <= 0.0 || *draw_h <= 0.0 {
                    continue;
                }
                // The mask keeps the image from bleeding out of the painting
                // area — essential for CSS sprites, whose background-position is
                // negative. Nothing is painted outside the PAINTING area
                // (`background-clip`); the tile grid below is still anchored in
                // the POSITIONING area (css-backgrounds-3 §3.6, §3.7).
                let bg_clip = if transform_depth > 0 {
                    build_clip_mask_with_transform(clip, radii, radii_y, pw, ph, ts)
                } else {
                    build_clip_mask(
                        clip,
                        radii,
                        radii_y,
                        pw,
                        ph,
                        scale,
                        active_scroll_x,
                        active_scroll_y,
                    )
                };
                let bg_clip_ref = bg_clip.as_ref().or(clip_mask);
                if let Some(img_pixmap) = tiny_skia::PixmapRef::from_bytes(rgba, iw, ih) {
                    let paint = tiny_skia::PixmapPaint {
                        quality: tiny_skia::FilterQuality::Bilinear,
                        ..tiny_skia::PixmapPaint::default()
                    };
                    let mut blend_layer = if *blend_mode != 0 {
                        Pixmap::new(pw, ph)
                    } else {
                        None
                    };
                    let target = blend_layer
                        .as_mut()
                        .or_else(|| layer_stack.last_mut().map(|l| &mut l.pixmap))
                        .unwrap_or(pixmap);
                    if iw == 1
                        && ih == 1
                        && rgba[3] == 255
                        && *repeat_x_mode == 1
                        && *repeat_y_mode == 1
                    {
                        let mut fill = Paint::default();
                        fill.set_color(SkColor::from_rgba8(rgba[0], rgba[1], rgba[2], 255));
                        if let Some(rect) = SkRect::from_xywh(clip.x, clip.y, clip.w, clip.h) {
                            target.fill_rect(rect, &fill, ts, bg_clip_ref);
                        }
                    } else if *repeat_x_mode != 0 || *repeat_y_mode != 0 {
                        let xs = background_axis_tiles(
                            *repeat_x_mode,
                            *pos_x,
                            *draw_w,
                            clip.x,
                            clip.w.max(0.0),
                        );
                        let ys = background_axis_tiles(
                            *repeat_y_mode,
                            *pos_y,
                            *draw_h,
                            clip.y,
                            clip.h.max(0.0),
                        );
                        for (ty, tile_h) in ys {
                            for (tx, tile_w) in &xs {
                                let tile_ts = ts
                                    .pre_translate(*tx, ty)
                                    .pre_scale(*tile_w / iw as f32, tile_h / ih as f32);
                                target.draw_pixmap(0, 0, img_pixmap, &paint, tile_ts, bg_clip_ref);
                            }
                        }
                    } else {
                        let sx_img = draw_w / iw as f32;
                        let sy_img = draw_h / ih as f32;
                        let img_ts = ts.pre_translate(*pos_x, *pos_y).pre_scale(sx_img, sy_img);
                        target.draw_pixmap(0, 0, img_pixmap, &paint, img_ts, bg_clip_ref);
                    }
                    if let Some(layer) = blend_layer {
                        let target = layer_stack
                            .last_mut()
                            .map(|l| &mut l.pixmap)
                            .unwrap_or(pixmap);
                        blend_composite(target, &layer, *blend_mode);
                    }
                }
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

struct MaskLayer {
    clip: Rect,
    no_clip: bool,
    origin: Rect,
    tile: Rect,
    data: Option<ImageRef>,
    luminance: bool,
    repeat_x_mode: u8,
    repeat_y_mode: u8,
    composite: u8,
}

fn mask_axis_sample(
    coordinate: f32,
    origin_start: f32,
    origin_length: f32,
    tile_start: f32,
    tile_length: f32,
    repeat_mode: u8,
) -> Option<f32> {
    if !coordinate.is_finite() || !tile_length.is_finite() || tile_length <= 0.0 {
        return None;
    }
    let (start, length) = match repeat_mode {
        1 => return Some((coordinate - tile_start).rem_euclid(tile_length) / tile_length),
        2 if origin_length >= tile_length * 2.0 => {
            let count = (origin_length / tile_length).floor();
            let gap = (origin_length - tile_length * count) / (count - 1.0);
            let stride = tile_length + gap;
            let offset = coordinate - origin_start;
            if offset < 0.0 || offset >= origin_length {
                return None;
            }
            (offset.rem_euclid(stride), tile_length)
        }
        3 if origin_length > 0.0 => {
            let count = (origin_length / tile_length).round().max(1.0);
            let length = origin_length / count;
            let offset = coordinate - origin_start;
            if offset < 0.0 || offset >= origin_length {
                return None;
            }
            (offset.rem_euclid(length), length)
        }
        _ => (coordinate - tile_start, tile_length),
    };
    (start >= 0.0 && start < length).then_some(start / length)
}

fn mask_axis_paint_bounds(
    origin_start: f32,
    origin_length: f32,
    tile_start: f32,
    tile_length: f32,
    repeat_mode: u8,
) -> Option<(f32, f32)> {
    match repeat_mode {
        1 => None,
        2 if origin_length >= tile_length * 2.0 => {
            Some((origin_start, origin_start + origin_length))
        }
        3 => Some((origin_start, origin_start + origin_length)),
        _ => Some((tile_start, tile_start + tile_length)),
    }
}

fn composite_masked_layer(
    target: &mut Pixmap,
    layer: &Pixmap,
    mask: &MaskLayer,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) {
    let Some(data) = mask.data.as_ref() else {
        return;
    };
    let (mask_rgba, mw, mh) = match data {
        ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
        ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
    };
    let rect = mask.clip;
    if mw == 0
        || mh == 0
        || mask.tile.w <= 0.0
        || mask.tile.h <= 0.0
        || (!mask.no_clip && (rect.w <= 0.0 || rect.h <= 0.0))
    {
        return;
    }
    let effective_scale = scale.max(0.001);
    let x_bounds = if mask.no_clip {
        mask_axis_paint_bounds(
            mask.origin.x,
            mask.origin.w,
            mask.tile.x,
            mask.tile.w,
            mask.repeat_x_mode,
        )
    } else {
        Some((rect.x, rect.right()))
    };
    let y_bounds = if mask.no_clip {
        mask_axis_paint_bounds(
            mask.origin.y,
            mask.origin.h,
            mask.tile.y,
            mask.tile.h,
            mask.repeat_y_mode,
        )
    } else {
        Some((rect.y, rect.bottom()))
    };
    let device_axis = |bounds: Option<(f32, f32)>, scroll: f32, extent: u32| {
        bounds
            .filter(|(start, end)| start.is_finite() && end.is_finite())
            .map(|(start, end)| {
                let lo = (((start - scroll) * effective_scale).floor() - 1.0)
                    .clamp(0.0, extent as f32) as i32;
                let hi = (((end - scroll) * effective_scale).ceil() + 1.0).clamp(0.0, extent as f32)
                    as i32;
                (lo, hi)
            })
            .unwrap_or((0, extent as i32))
    };
    let (left, right) = device_axis(x_bounds, scroll_x, layer.width());
    let (top, bottom) = device_axis(y_bounds, scroll_y, layer.height());
    let region = tiny_skia::IntRect::from_ltrb(left, top, right, bottom);
    let Some(region) = region else {
        return;
    };
    let Some(mut masked) = layer.clone_rect(region) else {
        return;
    };
    let width = masked.width() as usize;
    let inv_scale = 1.0 / scale.max(0.001);
    let x_samples: Vec<Option<usize>> = (0..width)
        .map(|x| {
            let doc_x = (region.x() as usize + x) as f32 * inv_scale + scroll_x;
            if !mask.no_clip && (doc_x < rect.x || doc_x >= rect.right()) {
                return None;
            }
            mask_axis_sample(
                doc_x,
                mask.origin.x,
                mask.origin.w,
                mask.tile.x,
                mask.tile.w,
                mask.repeat_x_mode,
            )
            .map(|sample| (sample * mw as f32).floor().clamp(0.0, (mw - 1) as f32) as usize)
        })
        .collect();
    let y_samples: Vec<Option<usize>> = (0..masked.height() as usize)
        .map(|y| {
            let doc_y = (region.y() as usize + y) as f32 * inv_scale + scroll_y;
            if !mask.no_clip && (doc_y < rect.y || doc_y >= rect.bottom()) {
                return None;
            }
            mask_axis_sample(
                doc_y,
                mask.origin.y,
                mask.origin.h,
                mask.tile.y,
                mask.tile.h,
                mask.repeat_y_mode,
            )
            .map(|sample| (sample * mh as f32).floor().clamp(0.0, (mh - 1) as f32) as usize)
        })
        .collect();
    for (i, px) in masked.data_mut().chunks_exact_mut(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let mask_alpha = if let (Some(u), Some(v)) = (x_samples[i % width], y_samples[i / width]) {
            let base = (v * mw as usize + u) * 4;
            if mask.luminance {
                let r = mask_rgba.get(base).copied().unwrap_or(0) as u32;
                let g = mask_rgba.get(base + 1).copied().unwrap_or(0) as u32;
                let b = mask_rgba.get(base + 2).copied().unwrap_or(0) as u32;
                // Image decoders publish premultiplied RGBA, so these RGB
                // channels already include the mask pixel's alpha.
                ((r * 2126 + g * 7152 + b * 722 + 5000) / 10_000) as u8
            } else {
                mask_rgba.get(base + 3).copied().unwrap_or(0)
            }
        } else {
            0
        };
        let a = mask_alpha as u32;
        px[0] = ((px[0] as u32 * a) / 255) as u8;
        px[1] = ((px[1] as u32 * a) / 255) as u8;
        px[2] = ((px[2] as u32 * a) / 255) as u8;
        px[3] = ((px[3] as u32 * a) / 255) as u8;
    }
    target.draw_pixmap(
        region.x(),
        region.y(),
        masked.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

fn composite_masked_layers(
    target: &mut Pixmap,
    layer: &Pixmap,
    masks: &[MaskLayer],
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) {
    if let [mask] = masks {
        composite_masked_layer(target, layer, mask, scale, scroll_x, scroll_y);
        return;
    }
    if masks.is_empty() {
        return;
    }

    let effective_scale = scale.max(0.001);
    let device_axis = |bounds: Option<(f32, f32)>, scroll: f32, extent: u32| {
        bounds
            .filter(|(start, end)| start.is_finite() && end.is_finite())
            .map(|(start, end)| {
                let lo = (((start - scroll) * effective_scale).floor() - 1.0)
                    .clamp(0.0, extent as f32) as i32;
                let hi = (((end - scroll) * effective_scale).ceil() + 1.0).clamp(0.0, extent as f32)
                    as i32;
                (lo, hi)
            })
            .unwrap_or((0, extent as i32))
    };
    let mut bounds = (layer.width() as i32, 0, layer.height() as i32, 0);
    for mask in masks {
        let Some(data) = mask.data.as_ref() else {
            continue;
        };
        let (_, mw, mh) = match data {
            ImageRef::Owned(bytes, w, h) => (bytes.as_slice(), *w, *h),
            ImageRef::Shared(bytes, w, h) => (bytes.as_slice(), *w, *h),
        };
        if mw == 0 || mh == 0 || mask.tile.w <= 0.0 || mask.tile.h <= 0.0 {
            continue;
        }
        let x_bounds = if mask.no_clip {
            mask_axis_paint_bounds(
                mask.origin.x,
                mask.origin.w,
                mask.tile.x,
                mask.tile.w,
                mask.repeat_x_mode,
            )
        } else {
            Some((mask.clip.x, mask.clip.right()))
        };
        let y_bounds = if mask.no_clip {
            mask_axis_paint_bounds(
                mask.origin.y,
                mask.origin.h,
                mask.tile.y,
                mask.tile.h,
                mask.repeat_y_mode,
            )
        } else {
            Some((mask.clip.y, mask.clip.bottom()))
        };
        let (left, right) = device_axis(x_bounds, scroll_x, layer.width());
        let (top, bottom) = device_axis(y_bounds, scroll_y, layer.height());
        bounds.0 = bounds.0.min(left);
        bounds.1 = bounds.1.max(right);
        bounds.2 = bounds.2.min(top);
        bounds.3 = bounds.3.max(bottom);
    }
    let Some(region) = tiny_skia::IntRect::from_ltrb(bounds.0, bounds.2, bounds.1, bounds.3) else {
        return;
    };
    let Some(mut masked) = layer.clone_rect(region) else {
        return;
    };
    let width = masked.width() as usize;
    let inv_scale = 1.0 / effective_scale;

    struct PreparedMask<'a> {
        rgba: &'a [u8],
        width: usize,
        luminance: bool,
        x: Vec<Option<usize>>,
        y: Vec<Option<usize>>,
    }
    let prepared: Vec<Option<PreparedMask<'_>>> = masks
        .iter()
        .map(|mask| {
            let data = mask.data.as_ref()?;
            let (rgba, mw, mh) = match data {
                ImageRef::Owned(bytes, w, h) => (bytes.as_slice(), *w, *h),
                ImageRef::Shared(bytes, w, h) => (bytes.as_slice(), *w, *h),
            };
            if mw == 0 || mh == 0 || mask.tile.w <= 0.0 || mask.tile.h <= 0.0 {
                return None;
            }
            let x = (0..width)
                .map(|x| {
                    let doc_x = (region.x() as usize + x) as f32 * inv_scale + scroll_x;
                    if !mask.no_clip && (doc_x < mask.clip.x || doc_x >= mask.clip.right()) {
                        return None;
                    }
                    mask_axis_sample(
                        doc_x,
                        mask.origin.x,
                        mask.origin.w,
                        mask.tile.x,
                        mask.tile.w,
                        mask.repeat_x_mode,
                    )
                    .map(|sample| (sample * mw as f32).floor().clamp(0.0, (mw - 1) as f32) as usize)
                })
                .collect();
            let y = (0..masked.height() as usize)
                .map(|y| {
                    let doc_y = (region.y() as usize + y) as f32 * inv_scale + scroll_y;
                    if !mask.no_clip && (doc_y < mask.clip.y || doc_y >= mask.clip.bottom()) {
                        return None;
                    }
                    mask_axis_sample(
                        doc_y,
                        mask.origin.y,
                        mask.origin.h,
                        mask.tile.y,
                        mask.tile.h,
                        mask.repeat_y_mode,
                    )
                    .map(|sample| (sample * mh as f32).floor().clamp(0.0, (mh - 1) as f32) as usize)
                })
                .collect();
            Some(PreparedMask {
                rgba,
                width: mw as usize,
                luminance: mask.luminance,
                x,
                y,
            })
        })
        .collect();

    for (i, px) in masked.data_mut().chunks_exact_mut(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let mut alpha = 0_u32;
        for (index, mask) in masks.iter().enumerate().rev() {
            let source = prepared[index]
                .as_ref()
                .and_then(|sample| {
                    let u = sample.x[i % width]?;
                    let v = sample.y[i / width]?;
                    let base = (v * sample.width + u) * 4;
                    Some(if sample.luminance {
                        let r = sample.rgba.get(base).copied().unwrap_or(0) as u32;
                        let g = sample.rgba.get(base + 1).copied().unwrap_or(0) as u32;
                        let b = sample.rgba.get(base + 2).copied().unwrap_or(0) as u32;
                        (r * 2126 + g * 7152 + b * 722 + 5000) / 10_000
                    } else {
                        sample.rgba.get(base + 3).copied().unwrap_or(0) as u32
                    })
                })
                .unwrap_or(0);
            alpha = if index == masks.len() - 1 {
                source
            } else {
                match mask.composite {
                    1 => source * (255 - alpha) / 255,
                    2 => source * alpha / 255,
                    3 => (source * (255 - alpha) + alpha * (255 - source)) / 255,
                    _ => source + alpha * (255 - source) / 255,
                }
            };
        }
        px[0] = ((px[0] as u32 * alpha) / 255) as u8;
        px[1] = ((px[1] as u32 * alpha) / 255) as u8;
        px[2] = ((px[2] as u32 * alpha) / 255) as u8;
        px[3] = ((px[3] as u32 * alpha) / 255) as u8;
    }
    target.draw_pixmap(
        region.x(),
        region.y(),
        masked.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

#[test]
fn cropped_mask_composite_matches_full_surface_reference() {
    let mask_bytes = vec![
        255, 255, 255, 0, 255, 255, 255, 96, 255, 255, 255, 192, 255, 255, 255, 255,
    ];
    let mask = ImageRef::Owned(mask_bytes.clone(), 2, 2);
    let mut layer = Pixmap::new(100, 80).unwrap();
    layer.fill(tiny_skia::Color::from_rgba8(220, 40, 80, 192));

    for scale in [1.0_f32, 1.5, 2.0] {
        for (scroll_x, scroll_y) in [(0.0, 0.0), (3.25, 7.5)] {
            for rect in [
                Rect::new(6.3, 4.7, 19.6, 23.2),
                Rect::new(-4.2, -6.1, 18.0, 15.0),
                Rect::new(110.0, 90.0, 12.0, 12.0),
            ] {
                let mut expected_layer = layer.clone();
                let width = expected_layer.width() as usize;
                let inv_scale = 1.0 / scale.max(0.001);
                for (i, px) in expected_layer.data_mut().chunks_exact_mut(4).enumerate() {
                    let doc_x = (i % width) as f32 * inv_scale + scroll_x;
                    let doc_y = (i / width) as f32 * inv_scale + scroll_y;
                    let alpha = if doc_x >= rect.x
                        && doc_x < rect.right()
                        && doc_y >= rect.y
                        && doc_y < rect.bottom()
                    {
                        let u =
                            (((doc_x - rect.x) / rect.w) * 2.0).floor().clamp(0.0, 1.0) as usize;
                        let v =
                            (((doc_y - rect.y) / rect.h) * 2.0).floor().clamp(0.0, 1.0) as usize;
                        mask_bytes[(v * 2 + u) * 4 + 3] as u32
                    } else {
                        0
                    };
                    for channel in px {
                        *channel = ((*channel as u32 * alpha) / 255) as u8;
                    }
                }
                let mut expected = Pixmap::new(100, 80).unwrap();
                expected.fill(tiny_skia::Color::from_rgba8(15, 30, 45, 255));
                let mut actual = expected.clone();
                expected.draw_pixmap(
                    0,
                    0,
                    expected_layer.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
                composite_masked_layer(
                    &mut actual,
                    &layer,
                    &MaskLayer {
                        clip: rect,
                        no_clip: false,
                        origin: rect,
                        tile: rect,
                        data: Some(mask.clone()),
                        luminance: false,
                        repeat_x_mode: 0,
                        repeat_y_mode: 0,
                        composite: 0,
                    },
                    scale,
                    scroll_x,
                    scroll_y,
                );
                assert_eq!(
                    actual.data(),
                    expected.data(),
                    "scale={scale}, scroll=({scroll_x},{scroll_y}), rect={rect:?}"
                );
            }
        }
    }
}

fn border_side_path(rect: Rect, widths: [f32; 4], side: usize) -> Option<tiny_skia::Path> {
    let (x0, y0) = (rect.x, rect.y);
    let (x1, y1) = (rect.x + rect.w, rect.y + rect.h);
    let [top, right, bottom, left] = widths;
    let points = match side {
        0 => [
            (x0, y0),
            (x1, y0),
            (x1 - right, y0 + top),
            (x0 + left, y0 + top),
        ],
        1 => [
            (x1, y0),
            (x1, y1),
            (x1 - right, y1 - bottom),
            (x1 - right, y0 + top),
        ],
        2 => [
            (x1, y1),
            (x0, y1),
            (x0 + left, y1 - bottom),
            (x1 - right, y1 - bottom),
        ],
        3 => [
            (x0, y1),
            (x0, y0),
            (x0 + left, y0 + top),
            (x0 + left, y1 - bottom),
        ],
        _ => return None,
    };
    let mut path = PathBuilder::new();
    path.move_to(points[0].0, points[0].1);
    for point in points.iter().skip(1) {
        path.line_to(point.0, point.1);
    }
    path.close();
    path.finish()
}

fn paint_patterned_border_path(
    target: &mut Pixmap,
    path: &tiny_skia::Path,
    width: f32,
    style: u8,
    color: Color,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if width <= 0.0 || !width.is_finite() || color.a == 0 {
        return;
    }
    // CSS Backgrounds 3: dotted uses round dots; dashed uses square-ended dashes.
    const DASH_LENGTH_IN_WIDTHS: f32 = 3.0;
    const DASH_GAP_IN_WIDTHS: f32 = 1.0;
    const DOT_SPACING_IN_WIDTHS: f32 = 2.0;
    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = width;
    stroke.dash = match style {
        2 => tiny_skia::StrokeDash::new(
            vec![width * DASH_LENGTH_IN_WIDTHS, width * DASH_GAP_IN_WIDTHS],
            0.0,
        ),
        3 => {
            stroke.line_cap = tiny_skia::LineCap::Round;
            tiny_skia::StrokeDash::new(vec![0.0, width * DOT_SPACING_IN_WIDTHS], 0.0)
        }
        _ => return,
    };
    if stroke.dash.is_none() {
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(to_sk_color(&color));
    paint.anti_alias = true;
    target.stroke_path(path, &paint, &stroke, transform, clip_mask);
}

fn paint_square_border_bands(
    target: &mut Pixmap,
    rect: Rect,
    widths: [f32; 4],
    side: usize,
    style: u8,
    color: Color,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    // Proportional insets preserve the shared miter between unequal adjacent widths.
    let mut fill_band = |start: f32, end: f32, color: Color| {
        let inset = widths.map(|width| width * start);
        let band_rect = Rect::new(
            rect.x + inset[3],
            rect.y + inset[0],
            rect.w - inset[1] - inset[3],
            rect.h - inset[0] - inset[2],
        );
        if let Some(path) =
            border_side_path(band_rect, widths.map(|width| width * (end - start)), side)
        {
            let mut paint = Paint::default();
            paint.set_color(to_sk_color(&color));
            paint.anti_alias = true;
            target.fill_path(&path, &paint, FillRule::Winding, transform, clip_mask);
        }
    };
    match style {
        4 if widths[side] >= 3.0 => {
            fill_band(0.0, 1.0 / 3.0, color);
            fill_band(2.0 / 3.0, 1.0, color);
        }
        5 | 6 => {
            let outer_is_light = (style == 6) == matches!(side, 0 | 3);
            fill_band(0.0, 0.5, shade_border_color(color, outer_is_light));
            fill_band(0.5, 1.0, shade_border_color(color, !outer_is_light));
        }
        7 | 8 => {
            let is_light = (style == 8) == matches!(side, 0 | 3);
            fill_band(0.0, 1.0, shade_border_color(color, is_light));
        }
        _ => fill_band(0.0, 1.0, color),
    }
}

fn paint_straight_border_segment(
    target: &mut Pixmap,
    rect: Rect,
    style: u8,
    color: Color,
    horizontal: bool,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if style == 0 || color.a == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let thickness = if horizontal { rect.h } else { rect.w };
    let length = if horizontal { rect.w } else { rect.h };
    let mut fill = |start: f32, end: f32, near: f32, far: f32, color: Color| {
        let subrect = if horizontal {
            SkRect::from_xywh(rect.x + start, rect.y + near, end - start, far - near)
        } else {
            SkRect::from_xywh(rect.x + near, rect.y + start, far - near, end - start)
        };
        if let Some(subrect) = subrect {
            let mut paint = Paint::default();
            paint.set_color(to_sk_color(&color));
            target.fill_rect(subrect, &paint, transform, clip_mask);
        }
    };

    match style {
        2 => {
            const DASH_LENGTH: f32 = 3.0;
            const DASH_GAP: f32 = 1.0;
            let dash = (thickness * DASH_LENGTH).max(1.0);
            let step = dash + (thickness * DASH_GAP).max(1.0);
            let mut start = 0.0;
            while start < length {
                fill(start, (start + dash).min(length), 0.0, thickness, color);
                start += step;
            }
        }
        3 => {
            let radius = thickness / 2.0;
            let mut center = radius;
            while center < length {
                let (cx, cy) = if horizontal {
                    (rect.x + center, rect.y + radius)
                } else {
                    (rect.x + radius, rect.y + center)
                };
                let mut path = PathBuilder::new();
                path.push_circle(cx, cy, radius);
                if let Some(path) = path.finish() {
                    let mut paint = Paint::default();
                    paint.set_color(to_sk_color(&color));
                    target.fill_path(&path, &paint, FillRule::Winding, transform, clip_mask);
                }
                center += thickness * 2.0;
            }
        }
        4 if thickness >= 3.0 => {
            let stripe = thickness / 3.0;
            fill(0.0, length, 0.0, stripe, color);
            fill(0.0, length, thickness - stripe, thickness, color);
        }
        5 | 6 => {
            let middle = thickness / 2.0;
            let dark = shade_border_color(color, false);
            let light = shade_border_color(color, true);
            let (near, far) = if style == 5 {
                (dark, light)
            } else {
                (light, dark)
            };
            fill(0.0, length, 0.0, middle, near);
            fill(0.0, length, middle, thickness, far);
        }
        7 | 8 => {
            let shaded = shade_border_color(color, style == 8);
            fill(0.0, length, 0.0, thickness, shaded);
        }
        _ => fill(0.0, length, 0.0, thickness, color),
    }
}

fn shade_border_color(color: Color, lighter: bool) -> Color {
    const SHADE_AMOUNT: f32 = 0.4;
    let shade = |component: u8| {
        let value = component as f32;
        if lighter {
            (value + (255.0 - value) * SHADE_AMOUNT).round() as u8
        } else {
            (value * (1.0 - SHADE_AMOUNT)).round() as u8
        }
    };
    Color::rgba(shade(color.r), shade(color.g), shade(color.b), color.a)
}

fn draw_border_image_stretch(
    target: &mut Pixmap,
    rgba: &[u8],
    iw: u32,
    ih: u32,
    rect: &Rect,
    widths: &[f32; 4],
    slices: &[f32; 4],
    repeat_x_mode: u8,
    repeat_y_mode: u8,
    fill_center: bool,
    ts: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    let visible = border_image_visible_rect(target, ts).unwrap_or(*rect);
    let widths = border_image_used_widths(*widths, *rect);
    let sw = iw as f32;
    let sh = ih as f32;
    let st = slices[0].clamp(0.0, sh);
    let sr = slices[1].clamp(0.0, sw);
    let sb = slices[2].clamp(0.0, sh);
    let sl = slices[3].clamp(0.0, sw);
    let dt = widths[0];
    let dr = widths[1];
    let db = widths[2];
    let dl = widths[3];

    let sx = [0.0, sl, (sw - sr).max(sl), sw];
    let sy = [0.0, st, (sh - sb).max(st), sh];
    let dx = [
        rect.x,
        rect.x + dl,
        (rect.right() - dr).max(rect.x + dl),
        rect.right(),
    ];
    let dy = [
        rect.y,
        rect.y + dt,
        (rect.bottom() - db).max(rect.y + dt),
        rect.bottom(),
    ];

    for row in 0..3 {
        for col in 0..3 {
            if row == 1 && col == 1 && !fill_center {
                continue;
            }
            let src = Rect::new(
                sx[col],
                sy[row],
                sx[col + 1] - sx[col],
                sy[row + 1] - sy[row],
            );
            let dst = Rect::new(
                dx[col],
                dy[row],
                dx[col + 1] - dx[col],
                dy[row + 1] - dy[row],
            );
            if row == 1 && col == 1 {
                let scale_x = if st > 0.0 && dt > 0.0 {
                    dt / st
                } else if sb > 0.0 && db > 0.0 {
                    db / sb
                } else {
                    1.0
                };
                let scale_y = if sl > 0.0 && dl > 0.0 {
                    dl / sl
                } else if sr > 0.0 && dr > 0.0 {
                    dr / sr
                } else {
                    1.0
                };
                draw_border_image_center(
                    target,
                    rgba,
                    iw,
                    ih,
                    src,
                    dst,
                    src.w * scale_x,
                    src.h * scale_y,
                    repeat_x_mode,
                    repeat_y_mode,
                    visible,
                    ts,
                    clip_mask,
                );
                continue;
            }
            let repeat_mode = match (row, col) {
                (0 | 2, 1) => repeat_x_mode,
                (1, 0 | 2) => repeat_y_mode,
                _ => 0,
            };
            let horizontal = row != 1;
            draw_border_image_patch(
                target,
                rgba,
                iw,
                ih,
                src,
                dst,
                repeat_mode,
                horizontal,
                visible,
                ts,
                clip_mask,
            );
        }
    }
}

fn border_image_visible_rect(target: &Pixmap, transform: Transform) -> Option<Rect> {
    let inverse = transform.invert()?;
    let w = target.width() as f32;
    let h = target.height() as f32;
    let mut corners = [
        Point::from_xy(-2.0, -2.0),
        Point::from_xy(w + 2.0, -2.0),
        Point::from_xy(w + 2.0, h + 2.0),
        Point::from_xy(-2.0, h + 2.0),
    ];
    inverse.map_points(&mut corners);
    if corners.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return None;
    }
    let left = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let top = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let right = corners
        .iter()
        .map(|p| p.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let bottom = corners
        .iter()
        .map(|p| p.y)
        .fold(f32::NEG_INFINITY, f32::max);
    Some(Rect::new(left, top, right - left, bottom - top))
}

fn border_image_used_widths(widths: [f32; 4], rect: Rect) -> [f32; 4] {
    let widths = widths.map(|width| width.max(0.0));
    let horizontal = widths[1] + widths[3];
    let vertical = widths[0] + widths[2];
    let mut factor: f32 = 1.0;
    if horizontal > 0.0 {
        factor = factor.min(rect.w.max(0.0) / horizontal);
    }
    if vertical > 0.0 {
        factor = factor.min(rect.h.max(0.0) / vertical);
    }
    widths.map(|width| width * factor)
}

fn draw_border_image_center(
    target: &mut Pixmap,
    rgba: &[u8],
    iw: u32,
    ih: u32,
    src: Rect,
    dst: Rect,
    tile_w: f32,
    tile_h: f32,
    repeat_x_mode: u8,
    repeat_y_mode: u8,
    visible: Rect,
    ts: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if src.w <= 0.0 || src.h <= 0.0 || dst.w <= 0.0 || dst.h <= 0.0 {
        return;
    }
    let x_tiles = border_image_axis_tiles_visible(
        repeat_x_mode,
        dst.x,
        dst.w,
        tile_w,
        visible.x,
        visible.right(),
    );
    for (tile_y, tile_h) in border_image_axis_tiles_visible(
        repeat_y_mode,
        dst.y,
        dst.h,
        tile_h,
        visible.y,
        visible.bottom(),
    ) {
        let visible_y = tile_y.max(dst.y);
        let visible_h = (tile_y + tile_h).min(dst.bottom()) - visible_y;
        if visible_h <= 0.0 {
            continue;
        }
        let src_y = (visible_y - tile_y) / tile_h;
        let src_h = visible_h / tile_h;
        for &(tile_x, tile_w) in &x_tiles {
            let visible_x = tile_x.max(dst.x);
            let visible_w = (tile_x + tile_w).min(dst.right()) - visible_x;
            if visible_w <= 0.0 {
                continue;
            }
            let src_x = (visible_x - tile_x) / tile_w;
            let src_w = visible_w / tile_w;
            draw_rgba_patch(
                target,
                rgba,
                iw,
                ih,
                Rect::new(
                    src.x + src.w * src_x,
                    src.y + src.h * src_y,
                    src.w * src_w,
                    src.h * src_h,
                ),
                Rect::new(visible_x, visible_y, visible_w, visible_h),
                ts,
                clip_mask,
            );
        }
    }
}

fn draw_border_image_patch(
    target: &mut Pixmap,
    rgba: &[u8],
    iw: u32,
    ih: u32,
    src: Rect,
    dst: Rect,
    repeat_mode: u8,
    horizontal: bool,
    visible: Rect,
    ts: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if repeat_mode == 0 {
        draw_rgba_patch(target, rgba, iw, ih, src, dst, ts, clip_mask);
        return;
    }

    let perpendicular_scale = if horizontal {
        if src.h <= 0.0 {
            return;
        }
        dst.h / src.h
    } else {
        if src.w <= 0.0 {
            return;
        }
        dst.w / src.w
    };
    let natural_main = if horizontal {
        src.w * perpendicular_scale
    } else {
        src.h * perpendicular_scale
    };
    if natural_main <= 0.0 {
        return;
    }

    let main_start = if horizontal { dst.x } else { dst.y };
    let main_len = if horizontal { dst.w } else { dst.h };
    let (visible_start, visible_end) = if horizontal {
        (visible.x, visible.right())
    } else {
        (visible.y, visible.bottom())
    };
    for (tile_start, tile_len) in border_image_axis_tiles_visible(
        repeat_mode,
        main_start,
        main_len,
        natural_main,
        visible_start,
        visible_end,
    ) {
        let visible_start = tile_start.max(main_start);
        let visible_end = (tile_start + tile_len).min(main_start + main_len);
        let visible_len = visible_end - visible_start;
        if visible_len <= 0.0 {
            continue;
        }
        let tile_offset = visible_start - tile_start;
        let src_fraction_start = (tile_offset / tile_len).clamp(0.0, 1.0);
        let src_fraction_len = (visible_len / tile_len).clamp(0.0, 1.0 - src_fraction_start);

        let (tile_src, tile_dst) = if horizontal {
            (
                Rect::new(
                    src.x + src.w * src_fraction_start,
                    src.y,
                    src.w * src_fraction_len,
                    src.h,
                ),
                Rect::new(visible_start, dst.y, visible_len, dst.h),
            )
        } else {
            (
                Rect::new(
                    src.x,
                    src.y + src.h * src_fraction_start,
                    src.w,
                    src.h * src_fraction_len,
                ),
                Rect::new(dst.x, visible_start, dst.w, visible_len),
            )
        };
        draw_rgba_patch(target, rgba, iw, ih, tile_src, tile_dst, ts, clip_mask);
    }
}

#[cfg(test)]
fn border_image_axis_tiles(mode: u8, start: f32, len: f32, tile: f32) -> Vec<(f32, f32)> {
    border_image_axis_tiles_visible(mode, start, len, tile, start, start + len)
}

fn border_image_axis_tiles_visible(
    mode: u8,
    start: f32,
    len: f32,
    tile: f32,
    visible_start: f32,
    visible_end: f32,
) -> Vec<(f32, f32)> {
    if tile <= 0.0 || len <= 0.0 || !tile.is_finite() || !len.is_finite() {
        return Vec::new();
    }
    let lo = visible_start.max(start);
    let hi = visible_end.min(start + len);
    if lo >= hi {
        return Vec::new();
    }
    if mode == 0 {
        return vec![(start, len)];
    }
    let (first, step, tile_size, count) = match mode {
        1 => (start + (len - tile) / 2.0, tile, tile, None),
        2 if tile >= len => (start + (len - tile) / 2.0, tile, tile, Some(1)),
        2 => {
            let count = (len / tile).floor().max(1.0) as i64;
            if count <= 1 {
                (start + (len - tile) / 2.0, tile, tile, Some(1))
            } else {
                let gap = (len - tile * count as f32) / (count + 1) as f32;
                (start + gap, tile + gap, tile, Some(count))
            }
        }
        3 => {
            let count = (len / tile).round().max(1.0) as i64;
            let rounded = len / count as f32;
            (start, rounded, rounded, Some(count))
        }
        _ => return vec![(start, len)],
    };
    if step <= 0.0 || !step.is_finite() {
        return Vec::new();
    }
    let first_index = ((lo - first - tile_size) / step).floor() as i64 + 1;
    let last_index = ((hi - first) / step).ceil() as i64;
    let begin = count.map_or(first_index, |count| first_index.clamp(0, count));
    let end = count.map_or(last_index, |count| last_index.clamp(0, count));
    (begin..end)
        .map(|index| (first + index as f32 * step, tile_size))
        .filter(|(pos, width)| *pos < hi && *pos + *width > lo)
        .collect()
}

#[cfg(test)]
mod border_image_tiling_tests {
    use super::{
        border_image_axis_tiles, border_image_axis_tiles_visible, border_image_used_widths,
    };
    use crate::types::Rect;

    #[test]
    fn space_distributes_gaps_at_both_ends() {
        assert_eq!(
            border_image_axis_tiles(2, 0.0, 10.0, 3.0),
            vec![(0.25, 3.0), (3.5, 3.0), (6.75, 3.0)],
        );
    }

    #[test]
    fn repeat_centers_the_first_tile() {
        assert_eq!(
            border_image_axis_tiles(1, 0.0, 10.0, 3.0),
            vec![(-2.5, 3.0), (0.5, 3.0), (3.5, 3.0), (6.5, 3.0), (9.5, 3.0)],
        );
    }

    #[test]
    fn overlapping_widths_use_one_proportional_factor() {
        assert_eq!(
            border_image_used_widths([8.0; 4], Rect::new(0.0, 0.0, 10.0, 20.0)),
            [5.0; 4],
        );
    }

    #[test]
    fn million_pixel_region_only_generates_visible_tiles() {
        let tiles = border_image_axis_tiles_visible(1, 0.0, 1_000_000.0, 3.0, 100.0, 130.0);
        assert!(
            tiles.len() >= 10 && tiles.len() <= 12,
            "visible tiles: {tiles:?}"
        );
        assert!(tiles.iter().all(|(x, w)| *x < 130.0 && *x + *w > 100.0));
    }
}

fn draw_rgba_patch(
    target: &mut Pixmap,
    rgba: &[u8],
    iw: u32,
    ih: u32,
    src: Rect,
    dst: Rect,
    ts: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if src.w <= 0.0 || src.h <= 0.0 || dst.w <= 0.0 || dst.h <= 0.0 {
        return;
    }
    let x0 = src.x.floor().clamp(0.0, iw as f32) as u32;
    let y0 = src.y.floor().clamp(0.0, ih as f32) as u32;
    let x1 = (src.x + src.w).ceil().clamp(0.0, iw as f32) as u32;
    let y1 = (src.y + src.h).ceil().clamp(0.0, ih as f32) as u32;
    let pw = x1.saturating_sub(x0);
    let ph = y1.saturating_sub(y0);
    if pw == 0 || ph == 0 {
        return;
    }
    let mut patch = Vec::with_capacity((pw * ph * 4) as usize);
    for y in y0..y1 {
        let start = ((y * iw + x0) * 4) as usize;
        let end = ((y * iw + x1) * 4) as usize;
        if let Some(row) = rgba.get(start..end) {
            patch.extend_from_slice(row);
        }
    }
    if patch.len() != (pw * ph * 4) as usize {
        return;
    }
    if let Some(patch_pixmap) = tiny_skia::PixmapRef::from_bytes(&patch, pw, ph) {
        let img_ts = ts
            .pre_translate(dst.x, dst.y)
            .pre_scale(dst.w / pw as f32, dst.h / ph as f32);
        target.draw_pixmap(
            0,
            0,
            patch_pixmap,
            &tiny_skia::PixmapPaint::default(),
            img_ts,
            clip_mask,
        );
    }
}

fn background_axis_tiles(mode: u8, pos: f32, tile: f32, start: f32, len: f32) -> Vec<(f32, f32)> {
    if tile <= 0.0 || len <= 0.0 {
        return Vec::new();
    }
    let area_end = start + len;
    match mode {
        1 => {
            let offset = ((pos - start) % tile + tile) % tile;
            let first = start - (tile - offset) % tile;
            let mut out = Vec::new();
            let mut cursor = first;
            while cursor < area_end {
                out.push((cursor, tile));
                cursor += tile;
            }
            out
        }
        2 => {
            if tile >= len {
                return vec![(start + (len - tile) / 2.0, tile)];
            }
            let count = (len / tile).floor().max(1.0) as usize;
            if count <= 1 {
                return vec![(start + (len - tile) / 2.0, tile)];
            }
            let gap = (len - tile * count as f32) / (count - 1) as f32;
            (0..count)
                .map(|i| (start + i as f32 * (tile + gap), tile))
                .collect()
        }
        3 => {
            let count = (len / tile).round().max(1.0) as usize;
            let rounded = len / count as f32;
            (0..count)
                .map(|i| (start + i as f32 * rounded, rounded))
                .collect()
        }
        _ => vec![(pos, tile)],
    }
}

fn to_sk_color(c: &Color) -> SkColor {
    SkColor::from_rgba8(c.r, c.g, c.b, c.a)
}

fn apply_opacity(c: &Color, alpha: f32) -> Color {
    if alpha >= 1.0 {
        return *c;
    }
    Color::rgba(c.r, c.g, c.b, (c.a as f32 * alpha) as u8)
}

/// Samples an element's background in document coordinates at each glyph pixel.
struct TextGradientSampler<'a> {
    rect: Rect,
    background_color: Color,
    gradient_type: u8,
    angle: f32,
    direction: GradientDirection,
    radial_center_x: f32,
    radial_center_y: f32,
    radial_radius_x: f32,
    radial_radius_y: f32,
    stops: &'a [(Color, f32)],
    inverse: Transform,
}

impl<'a> TextGradientSampler<'a> {
    fn new(command: &'a PaintCmd, transform: Transform) -> Option<Self> {
        let PaintCmd::PushTextGradient {
            rect,
            background_color,
            gradient_type,
            angle,
            direction,
            radial_center_x,
            radial_center_y,
            radial_radius_x,
            radial_radius_y,
            stops,
        } = command
        else {
            return None;
        };
        Some(Self {
            rect: *rect,
            background_color: *background_color,
            gradient_type: *gradient_type,
            angle: *angle,
            direction: *direction,
            radial_center_x: *radial_center_x,
            radial_center_y: *radial_center_y,
            radial_radius_x: *radial_radius_x,
            radial_radius_y: *radial_radius_y,
            stops,
            inverse: transform.invert()?,
        })
    }

    fn sample(&self, physical_x: f32, physical_y: f32) -> Color {
        let mut point = tiny_skia::Point::from_xy(physical_x + 0.5, physical_y + 0.5);
        self.inverse.map_point(&mut point);
        let rect = self.rect;
        if point.x < rect.x
            || point.x >= rect.right()
            || point.y < rect.y
            || point.y >= rect.bottom()
        {
            return self.background_color;
        }
        let position = if self.gradient_type == 1 {
            let angle = match self.direction {
                GradientDirection::Angle(_) => self.angle,
                GradientDirection::Corner { x, y } => (x as f32 * rect.w)
                    .atan2(-(y as f32 * rect.h))
                    .to_degrees()
                    .rem_euclid(360.0),
            };
            let rad = angle.to_radians();
            let dx = rad.sin();
            let dy = -rad.cos();
            let half = ((rect.w * dx).abs() + (rect.h * dy).abs()) / 2.0;
            if half <= 0.0 {
                return self.background_color;
            }
            let cx = rect.x + rect.w / 2.0;
            let cy = rect.y + rect.h / 2.0;
            ((point.x - cx) * dx + (point.y - cy) * dy) / (2.0 * half) + 0.5
        } else {
            let cx = rect.x + self.radial_center_x;
            let cy = rect.y + self.radial_center_y;
            let rx = self.radial_radius_x.max(1.0);
            let ry = self.radial_radius_y.max(1.0);
            (((point.x - cx) / rx).powi(2) + ((point.y - cy) / ry).powi(2)).sqrt()
        };
        let foreground = sample_text_gradient_stops(self.stops, position);
        composite_text_background(foreground, self.background_color)
    }
}

fn composite_text_background(foreground: Color, background: Color) -> Color {
    let fa = foreground.a as f32 / 255.0;
    let ba = background.a as f32 / 255.0;
    let alpha = fa + ba * (1.0 - fa);
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |front: u8, back: u8| {
        ((front as f32 * fa + back as f32 * ba * (1.0 - fa)) / alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::rgba(
        channel(foreground.r, background.r),
        channel(foreground.g, background.g),
        channel(foreground.b, background.b),
        (alpha * 255.0).round() as u8,
    )
}

fn sample_text_gradient_stops(stops: &[(Color, f32)], position: f32) -> Color {
    let Some(&(first, first_at)) = stops.first() else {
        return Color::TRANSPARENT;
    };
    if position <= first_at {
        return first;
    }
    for pair in stops.windows(2) {
        let (a, a_at) = pair[0];
        let (b, b_at) = pair[1];
        if position <= b_at {
            let t = if b_at > a_at {
                ((position - a_at) / (b_at - a_at)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let alpha = a.a as f32 * (1.0 - t) + b.a as f32 * t;
            if alpha <= 0.0 {
                return Color::TRANSPARENT;
            }
            let channel = |left: u8, right: u8| -> u8 {
                ((left as f32 * a.a as f32 * (1.0 - t) + right as f32 * b.a as f32 * t) / alpha)
                    .round()
                    .clamp(0.0, 255.0) as u8
            };
            return Color::rgba(
                channel(a.r, b.r),
                channel(a.g, b.g),
                channel(a.b, b.b),
                alpha.round() as u8,
            );
        }
    }
    stops.last().map(|(color, _)| *color).unwrap_or(first)
}

#[allow(clippy::too_many_arguments)]
fn blit_shaped_buffer_with_gradient(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    buf: &mut Buffer,
    _text: &str,
    phys_x: f32,
    phys_y: f32,
    _letter_spacing: f32,
    _word_spacing: f32,
    color: CTextColor,
    clip_mask: Option<&tiny_skia::Mask>,
    text_gradient: Option<&TextGradientSampler<'_>>,
) {
    struct PixmapTextRenderer<'a> {
        pixmap: &'a mut Pixmap,
        font_system: &'a mut FontSystem,
        swash_cache: &'a mut SwashCache,
        origin_x: i32,
        origin_y: i32,
        color_alpha: u32,
        clip_mask: Option<&'a tiny_skia::Mask>,
        text_gradient: Option<&'a TextGradientSampler<'a>>,
    }

    impl cosmic_text::Renderer for PixmapTextRenderer<'_> {
        fn rectangle(&mut self, x: i32, y: i32, w: u32, h: u32, color: CTextColor) {
            blit_text_pixel_rect(
                self.pixmap,
                self.origin_x + x,
                self.origin_y + y,
                w,
                h,
                color,
                self.color_alpha,
                self.clip_mask,
                self.text_gradient,
            );
        }

        fn glyph(&mut self, physical_glyph: cosmic_text::PhysicalGlyph, color: CTextColor) {
            let origin_x = self.origin_x;
            let origin_y = self.origin_y;
            let color_alpha = self.color_alpha;
            let clip_mask = self.clip_mask;
            let text_gradient = self.text_gradient;
            self.swash_cache.with_pixels(
                self.font_system,
                physical_glyph.cache_key,
                color,
                |x, y, pixel_color| {
                    blit_text_pixel_rect(
                        self.pixmap,
                        origin_x + physical_glyph.x + x,
                        origin_y + physical_glyph.y + y,
                        1,
                        1,
                        pixel_color,
                        color_alpha,
                        clip_mask,
                        text_gradient,
                    );
                },
            );
        }
    }

    let mut renderer = PixmapTextRenderer {
        pixmap,
        font_system,
        swash_cache,
        origin_x: phys_x as i32,
        origin_y: phys_y as i32,
        color_alpha: color.a() as u32,
        clip_mask,
        text_gradient,
    };
    buf.render(&mut renderer, color);
}

fn blit_text_pixel_rect(
    pixmap: &mut Pixmap,
    bx: i32,
    by: i32,
    gw: u32,
    gh: u32,
    gc: CTextColor,
    color_a: u32,
    clip_mask: Option<&tiny_skia::Mask>,
    text_gradient: Option<&TextGradientSampler<'_>>,
) {
    let ga = gc.a();
    if ga == 0 {
        return;
    }
    let eff_a = ga as u32 * color_a / 255;
    if eff_a == 0 {
        return;
    }
    let pix_w = pixmap.width() as i32;
    let pix_h = pixmap.height() as i32;
    let stride = pix_w as usize;
    let pixels = pixmap.pixels_mut();
    let mask_data = clip_mask.map(|m| m.data());
    for dy in 0..gh as i32 {
        let py = by + dy;
        if py < 0 || py >= pix_h {
            continue;
        }
        let row = py as usize * stride;
        for dx in 0..gw as i32 {
            let px_x = bx + dx;
            if px_x < 0 || px_x >= pix_w {
                continue;
            }
            let idx = row + px_x as usize;
            let sampled = text_gradient.map(|gradient| gradient.sample(px_x as f32, py as f32));
            let final_a = if let Some(m) = mask_data {
                let mask_val = m.get(idx).copied().unwrap_or(0);
                if mask_val == 0 {
                    continue;
                }
                (eff_a * mask_val as u32) / 255
            } else {
                eff_a
            };
            let final_a = sampled
                .as_ref()
                .map_or(final_a, |sample| final_a * sample.a as u32 / 255);
            if final_a == 0 {
                continue;
            }
            let sa = final_a;
            let ia = 255 - sa;
            let (sr, sg, sb) = sampled
                .map(|sample| (sample.r, sample.g, sample.b))
                .unwrap_or((gc.r(), gc.g(), gc.b()));
            let pr = sr as u32 * sa / 255;
            let pg = sg as u32 * sa / 255;
            let pb = sb as u32 * sa / 255;
            let dst = &mut pixels[idx];
            let r = (pr + dst.red() as u32 * ia / 255) as u8;
            let g = (pg + dst.green() as u32 * ia / 255) as u8;
            let b = (pb + dst.blue() as u32 * ia / 255) as u8;
            let a = (sa + dst.alpha() as u32 * ia / 255) as u8;
            if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, a) {
                *dst = p;
            }
        }
    }
}

struct ShadowText<'a> {
    text: &'a str,
    font_family: &'a str,
    font_size: f32,
    font_weight: u16,
    font_style: u8,
    font_stretch: f32,
    line_height: f32,
    color: Color,
    blur: f32,
    letter_spacing: f32,
    word_spacing: f32,
    small_caps: bool,
}

#[test]
fn clipped_local_text_shadows_match_full_viewport_blurs() {
    let mut fonts = FontSystem::new();
    let mut glyphs = SwashCache::new();
    for scale in [1.0, 1.5, 2.0] {
        for (x, y) in [(60.25, 48.5), (-6.0, 10.0), (240.0, 135.0)] {
            let width = (320.0 * scale) as u32;
            let height = (180.0 * scale) as u32;
            let mut mask = tiny_skia::Mask::new(width, height).unwrap();
            mask.fill_path(
                &PathBuilder::from_rect(SkRect::from_xywh(12.0, 8.0, 268.0, 148.0).unwrap()),
                FillRule::Winding,
                true,
                Transform::from_scale(scale, scale),
            );
            for (text, style, blur) in [("Video overlay", 0, 12.0), ("Italic shadow", 1, 3.0)] {
                let shadow = ShadowText {
                    text, font_family: "sans-serif", font_size: 19.0,
                    font_weight: 600, font_style: style, font_stretch: 100.0,
                    line_height: 26.0, color: Color::rgba(24, 48, 72, 185),
                    blur, letter_spacing: 0.5, word_spacing: 1.0, small_caps: false,
                };
                let mut expected = Pixmap::new(width, height).unwrap();
                let mut actual = Pixmap::new(width, height).unwrap();
                expected.fill(tiny_skia::Color::from_rgba8(40, 60, 80, 255));
                actual.fill(tiny_skia::Color::from_rgba8(40, 60, 80, 255));
                paint_text_shadow(&mut expected, &mut fonts, &mut glyphs, scale, x, y, &shadow, Some(&mask), false);
                paint_text_shadow(&mut actual, &mut fonts, &mut glyphs, scale, x, y, &shadow, Some(&mask), true);
                let difference = actual.data().iter().zip(expected.data()).position(|(a, b)| a != b);
                assert_eq!(difference, None, "scale={scale}, x={x}, y={y}, text={text}");
            }
        }
    }
}

#[test]
#[ignore = "manual software video overlay benchmark"]
fn benchmark_clipped_text_shadow_fallback() {
    let mut fonts = FontSystem::new();
    let mut glyphs = SwashCache::new();
    let mut target = Pixmap::new(2560, 1800).unwrap();
    let mut mask = tiny_skia::Mask::new(2560, 1800).unwrap();
    mask.data_mut().fill(255);
    let shadow = ShadowText {
        text: "VP9 video playback", font_family: "sans-serif", font_size: 36.0,
        font_weight: 700, font_style: 0, font_stretch: 100.0,
        line_height: 44.0, color: Color::rgba(0, 0, 0, 220),
        blur: 12.0, letter_spacing: 0.0, word_spacing: 0.0, small_caps: false,
    };
    for local in [false, true] {
        paint_text_shadow(&mut target, &mut fonts, &mut glyphs, 2.0, 32.0, 700.0, &shadow, Some(&mask), local);
        let start = std::time::Instant::now();
        for _ in 0..20 {
            paint_text_shadow(&mut target, &mut fonts, &mut glyphs, 2.0, 32.0, 700.0, &shadow, Some(&mask), local);
            std::hint::black_box(&target);
        }
        eprintln!("clipped text shadow local={local}: {:?} / 20", start.elapsed());
    }
}

fn paint_text_shadow(
    target: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scale: f32,
    x: f32,
    y: f32,
    shadow: &ShadowText<'_>,
    clip_mask: Option<&tiny_skia::Mask>,
    local_shadow: bool,
) {
    let draw = |pixmap: &mut Pixmap,
                x: f32,
                y: f32,
                font_system: &mut FontSystem,
                swash_cache: &mut SwashCache| {
        draw_text_cmd(
            pixmap,
            font_system,
            swash_cache,
            scale,
            x,
            y,
            shadow.text,
            shadow.font_family,
            shadow.font_size,
            shadow.font_weight,
            shadow.font_style,
            shadow.font_stretch,
            shadow.line_height,
            &shadow.color,
            &super::display_list::TextDecoration::default(),
            shadow.letter_spacing,
            shadow.word_spacing,
            shadow.small_caps,
            None,
        );
    };
    if shadow.blur <= 0.0 {
        draw_text_cmd(
            target,
            font_system,
            swash_cache,
            scale,
            x,
            y,
            shadow.text,
            shadow.font_family,
            shadow.font_size,
            shadow.font_weight,
            shadow.font_style,
            shadow.font_stretch,
            shadow.line_height,
            &shadow.color,
            &super::display_list::TextDecoration::default(),
            shadow.letter_spacing,
            shadow.word_spacing,
            shadow.small_caps,
            clip_mask,
        );
        return;
    }

    // Clipping is applied when compositing the blurred layer, not while
    // generating it. A clip must not turn a small glyph shadow into a
    // viewport-sized blur on every video frame.
    let (left, top, width, height) = if local_shadow {
        let text_width = crate::layout::inline_layout::measure_text_width_fs_attrs(
            font_system,
            shadow.text,
            shadow.font_size,
            cosmic_text::Weight(shadow.font_weight),
            match shadow.font_style {
                1 => CTextStyle::Italic,
                2 => CTextStyle::Oblique,
                _ => CTextStyle::Normal,
            },
            scale,
            shadow.font_family,
            crate::layout::inline_layout::stretch_from_percent(shadow.font_stretch),
        ) + shadow.letter_spacing * shadow.text.chars().count() as f32
            + shadow.word_spacing
                * shadow.text.chars().filter(|c| c.is_whitespace()).count() as f32;
        let text_height = shadow.line_height.max(shadow.font_size).max(1.0);
        let pad = (shadow.blur * 4.0 + 4.0).ceil();
        let left = (x * scale - pad).floor().max(0.0) as u32;
        let top = (y * scale - pad).floor().max(0.0) as u32;
        let right = ((x + text_width.max(1.0)) * scale + pad)
            .ceil()
            .min(target.width() as f32)
            .max(left as f32) as u32;
        let bottom = ((y + text_height) * scale + pad)
            .ceil()
            .min(target.height() as f32)
            .max(top as f32) as u32;
        (
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    } else {
        (0, 0, target.width(), target.height())
    };
    if width == 0 || height == 0 {
        return;
    }
    let Some(mut layer) = Pixmap::new(width, height) else {
        return;
    };
    draw(
        &mut layer,
        x - left as f32 / scale.max(0.001),
        y - top as f32 / scale.max(0.001),
        font_system,
        swash_cache,
    );
    crate::canvas::blur_pixmap(&mut layer, shadow.blur);
    target.draw_pixmap(
        left as i32,
        top as i32,
        layer.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        clip_mask,
    );
}

fn draw_text_cmd(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scale: f32,
    x: f32,
    y: f32,
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: u16,
    font_style: u8,
    font_stretch: f32,
    line_height: f32,
    color: &Color,
    decoration: &super::display_list::TextDecoration,
    _letter_spacing: f32,
    word_spacing: f32,
    _small_caps: bool,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    draw_text_cmd_with_gradient(
        pixmap,
        font_system,
        swash_cache,
        scale,
        x,
        y,
        text,
        font_family,
        font_size,
        font_weight,
        font_style,
        font_stretch,
        line_height,
        color,
        decoration,
        _letter_spacing,
        word_spacing,
        _small_caps,
        clip_mask,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_text_cmd_with_gradient(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scale: f32,
    x: f32,
    y: f32,
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: u16,
    font_style: u8,
    font_stretch: f32,
    line_height: f32,
    color: &Color,
    decoration: &super::display_list::TextDecoration,
    _letter_spacing: f32,
    word_spacing: f32,
    _small_caps: bool,
    clip_mask: Option<&tiny_skia::Mask>,
    text_gradient: Option<&TextGradientSampler<'_>>,
) {
    if text.is_empty() {
        return;
    }
    if word_spacing == 0.0 && text.starts_with(char::is_whitespace) {
        let visible_start = text
            .char_indices()
            .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(idx));
        let Some(visible_start) = visible_start else {
            return;
        };
        let leading = &text[..visible_start];
        let visible = &text[visible_start..];
        let leading_advance = crate::layout::inline_layout::measure_text_width_fs_attrs(
            font_system,
            leading,
            font_size,
            cosmic_text::Weight(font_weight),
            match font_style {
                1 => CTextStyle::Italic,
                2 => CTextStyle::Oblique,
                _ => CTextStyle::Normal,
            },
            scale,
            font_family,
            crate::layout::inline_layout::stretch_from_percent(font_stretch),
        ) + _letter_spacing * leading.chars().count() as f32;
        draw_text_cmd_with_gradient(
            pixmap,
            font_system,
            swash_cache,
            scale,
            x + leading_advance,
            y,
            visible,
            font_family,
            font_size,
            font_weight,
            font_style,
            font_stretch,
            line_height,
            color,
            decoration,
            _letter_spacing,
            word_spacing,
            _small_caps,
            clip_mask,
            text_gradient,
        );
        return;
    }
    let sc = scale;
    let size_adjust =
        crate::layout::inline_layout::font_size_adjust_scale(font_system, font_family);
    let phys_px = (font_size * size_adjust * sc).max(1.0);
    let phys_lh = (line_height * sc).max(1.0); // cosmic-text panics on 0
    let metrics = Metrics::new(phys_px, phys_lh);

    // Use the same available-family resolver as measurement so paint does not
    // pick a different face from the CSS family stack.
    let resolved = crate::layout::inline_layout::resolve_css_family(font_system, font_family);
    let family = resolved.as_family();
    let ct_w = CTextWeight(font_weight);
    let ct_s = match font_style {
        1 => CTextStyle::Italic,
        2 => CTextStyle::Oblique,
        _ => CTextStyle::Normal,
    };
    let ct_stretch = crate::layout::inline_layout::stretch_from_percent(font_stretch);
    let mut attrs = Attrs::new()
        .weight(ct_w)
        .style(ct_s)
        .stretch(ct_stretch)
        .family(family);
    let letter_spacing_attr = if _letter_spacing != 0.0 {
        (_letter_spacing * sc) / phys_px
    } else {
        0.0
    };
    if letter_spacing_attr != 0.0 {
        attrs = attrs.letter_spacing(letter_spacing_attr);
    }
    // Private-use code points have no portable meaning outside their icon font.
    // If that named face failed to load, a generic fallback may map the same
    // code point to an unrelated icon; show a missing-glyph square instead.
    let missing_icon_font = matches!(
        crate::layout::inline_layout::css_family_to_cosmic(font_family),
        cosmic_text::Family::Name(_)
    ) && matches!(
        resolved,
        crate::layout::inline_layout::ResolvedFamily::Generic(_)
    );
    let fallback_text = if missing_icon_font && text.chars().any(is_private_use_character) {
        Some(
            text.chars()
                .map(|ch| {
                    if is_private_use_character(ch) {
                        '\u{25a1}'
                    } else {
                        ch
                    }
                })
                .collect::<String>(),
        )
    } else {
        None
    };
    let text_for_shape = fallback_text.as_deref().unwrap_or(text);
    let phys_x = x * sc;
    let phys_y = y * sc;
    let ct_color = if text_gradient.is_some() {
        CTextColor::rgba(255, 255, 255, 255)
    } else {
        CTextColor::rgba(color.r, color.g, color.b, color.a)
    };

    // ⛔ SHAPE ONCE, BLIT MANY. This built a `Buffer` and ran a full
    // cosmic-text shaping pass for EVERY text run on EVERY frame — so a page
    // that had not changed at all re-shaped all its visible text just to put
    // the same pixels back. `SwashCache` caches RASTERISED GLYPHS, which is a
    // different thing and does not help here.
    //
    // The shaped buffer depends only on the string and the font attributes, so
    // it is cached on those. Position and colour are applied at blit time.
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text_for_shape.hash(&mut h);
        phys_px.to_bits().hash(&mut h);
        phys_lh.to_bits().hash(&mut h);
        font_weight.hash(&mut h);
        font_style.hash(&mut h);
        font_family.hash(&mut h);
        font_stretch.to_bits().hash(&mut h);
        _letter_spacing.to_bits().hash(&mut h);
        word_spacing.to_bits().hash(&mut h);
        _small_caps.hash(&mut h);
        h.finish()
    };

    let (line_w, run_line_y) = SHAPED.with(|cell| {
        let mut map = cell.borrow_mut();
        // A font load changes what any string shapes to, so the whole cache
        // goes when the face count moves.
        let faces = font_system.db().len();
        if map.0 != faces {
            map.1.clear();
            map.0 = faces;
        }
        // Bounded: a long session on many pages should not grow for ever.
        if map.1.len() > 8192 {
            map.1.clear();
        }

        if !map.1.contains_key(&key) {
            let mut buf = Buffer::new(font_system, metrics);
            buf.set_size(font_system, None, Some((phys_lh + 4.0).max(1.0)));
            if word_spacing != 0.0 && text_for_shape.contains(' ') {
                let word_attrs = attrs
                    .clone()
                    .letter_spacing(((_letter_spacing + word_spacing) * sc) / phys_px);
                let mut spans = Vec::new();
                let mut rest = text_for_shape;
                while !rest.is_empty() {
                    match rest.find(' ') {
                        Some(at) => {
                            if at > 0 {
                                spans.push((&rest[..at], attrs.clone()));
                            }
                            spans.push((&rest[at..at + 1], word_attrs.clone()));
                            rest = &rest[at + 1..];
                        }
                        None => {
                            spans.push((rest, attrs.clone()));
                            break;
                        }
                    }
                }
                let spans: Vec<_> = spans
                    .iter()
                    .flat_map(|(s, a)| {
                        crate::layout::inline_layout::css_font_spans(font_system, s, font_family, a)
                    })
                    .collect();
                buf.set_rich_text(
                    font_system,
                    spans.iter().map(|(s, a)| (*s, a.as_attrs())),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
            } else {
                let spans = crate::layout::inline_layout::css_font_spans(
                    font_system,
                    text_for_shape,
                    font_family,
                    &attrs,
                );
                buf.set_rich_text(
                    font_system,
                    spans.iter().map(|(s, a)| (*s, a.as_attrs())),
                    &attrs,
                    Shaping::Advanced,
                    None,
                );
            }
            buf.shape_until_scroll(font_system, false);
            map.1.insert(key, buf);
        }
        let buf = map.1.get_mut(&key).expect("just inserted");
        blit_shaped_buffer_with_gradient(
            pixmap,
            font_system,
            swash_cache,
            buf,
            text_for_shape,
            phys_x,
            phys_y,
            _letter_spacing * sc,
            word_spacing * sc,
            ct_color,
            clip_mask,
            text_gradient,
        );
        buf.layout_runs()
            .next()
            .map(|r| (r.line_w, Some(r.line_y)))
            .unwrap_or((0.0, None))
    });

    // Draw text decorations (underline, overline, strikethrough)
    let thickness = (decoration.thickness * sc).max(1.0);
    let mut paint = Paint::default();
    paint.set_color(to_sk_color(&Color::rgba(
        decoration.color.r,
        decoration.color.g,
        decoration.color.b,
        decoration.color.a,
    )));
    paint.anti_alias = true;

    let draw_deco_line = |pixmap: &mut Pixmap, x: f32, w: f32, y: f32, style: u8| {
        match style {
            0 => {
                // solid
                if let Some(r) = SkRect::from_xywh(x, y, w, thickness) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), clip_mask);
                }
            }
            1 => {
                // double
                if let Some(r) = SkRect::from_xywh(x, y, w, 1.0f32.max(thickness * 0.4)) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), clip_mask);
                }
                if let Some(r) =
                    SkRect::from_xywh(x, y + thickness * 1.5, w, 1.0f32.max(thickness * 0.4))
                {
                    pixmap.fill_rect(r, &paint, Transform::identity(), clip_mask);
                }
            }
            2 => {
                // dotted
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = thickness;
                stroke.dash =
                    tiny_skia::StrokeDash::new(vec![thickness * 1.5, thickness * 2.0], 0.0);
                let mut pb = PathBuilder::new();
                pb.move_to(x, y + thickness / 2.0);
                pb.line_to(x + w, y + thickness / 2.0);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), clip_mask);
                }
            }
            3 => {
                // dashed
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = thickness;
                stroke.dash =
                    tiny_skia::StrokeDash::new(vec![thickness * 4.0, thickness * 3.0], 0.0);
                let mut pb = PathBuilder::new();
                pb.move_to(x, y + thickness / 2.0);
                pb.line_to(x + w, y + thickness / 2.0);
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), clip_mask);
                }
            }
            4 => {
                // wavy
                let wave_h = thickness * 1.5;
                let wave_len = thickness * 4.0;
                let mut pb = PathBuilder::new();
                let mut cx = x;
                pb.move_to(cx, y);
                while cx < x + w {
                    pb.quad_to(cx + wave_len * 0.25, y - wave_h, cx + wave_len * 0.5, y);
                    pb.quad_to(cx + wave_len * 0.75, y + wave_h, cx + wave_len, y);
                    cx += wave_len;
                }
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = thickness;
                if let Some(path) = pb.finish() {
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), clip_mask);
                }
            }
            _ => {
                // fallback to solid
                if let Some(r) = SkRect::from_xywh(x, y, w, thickness) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), clip_mask);
                }
            }
        }
    };

    let baseline_y = run_line_y
        .map(|ly| phys_y + ly)
        .unwrap_or(phys_y + phys_px * 0.82);

    if decoration.underline {
        // Position underline below the baseline by default. `under` is lower,
        // on the under side of the em box, matching the authored intent for
        // scripts where baseline underlines cut through glyphs.
        let offset = if decoration.underline_offset > 0.0 {
            decoration.underline_offset * sc
        } else {
            thickness * 1.5
        };
        let uy = if matches!(
            decoration.underline_position,
            crate::types::TextUnderlinePosition::Under
        ) {
            baseline_y + phys_px * 0.18 + offset
        } else {
            baseline_y + offset
        };
        if decoration.skip_ink {
            for (start, width) in underline_skip_ink_segments(text, phys_x, line_w) {
                draw_deco_line(pixmap, start, width, uy, decoration.style);
            }
        } else {
            draw_deco_line(pixmap, phys_x, line_w, uy, decoration.style);
        }
    }
    if decoration.overline {
        let oy = run_line_y.map(|ly| phys_y + ly - phys_px).unwrap_or(phys_y) - thickness;
        draw_deco_line(pixmap, phys_x, line_w, oy, decoration.style);
    }
    if decoration.strikethrough {
        let sy = run_line_y
            .map(|ly| phys_y + ly - phys_px * 0.3)
            .unwrap_or(phys_y + phys_px * 0.4);
        draw_deco_line(pixmap, phys_x, line_w, sy, decoration.style);
    }
}

fn is_private_use_character(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

#[allow(clippy::too_many_arguments)]
fn draw_transformed_text_cmd(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    scale: f32,
    transform: Transform,
    x: f32,
    y: f32,
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: u16,
    font_style: u8,
    font_stretch: f32,
    line_height: f32,
    color: &Color,
    decoration: &super::display_list::TextDecoration,
    letter_spacing: f32,
    word_spacing: f32,
    small_caps: bool,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if text.is_empty() || scale <= 0.0 {
        return;
    }

    // Uniform, axis-aligned text can be rasterized directly at device size.
    if transform.kx == 0.0
        && transform.ky == 0.0
        && transform.sx == transform.sy
        && transform.sx > 0.0
    {
        let (raster_scale, text_x, text_y) = transformed_text_origin(&transform, x, y);
        draw_text_cmd(
            pixmap,
            font_system,
            swash_cache,
            raster_scale,
            text_x,
            text_y,
            text,
            font_family,
            font_size,
            font_weight,
            font_style,
            font_stretch,
            line_height,
            color,
            decoration,
            letter_spacing,
            word_spacing,
            small_caps,
            clip_mask,
        );
        return;
    }

    let ct_style = match font_style {
        1 => CTextStyle::Italic,
        2 => CTextStyle::Oblique,
        _ => CTextStyle::Normal,
    };
    let measured = crate::layout::inline_layout::measure_text_width_fs_attrs(
        font_system,
        text,
        font_size,
        cosmic_text::Weight(font_weight),
        ct_style,
        1.0,
        font_family,
        crate::layout::inline_layout::stretch_from_percent(font_stretch),
    );
    let spacing = letter_spacing * text.chars().count() as f32
        + word_spacing * text.chars().filter(|c| c.is_whitespace()).count() as f32;
    let line_h = line_height.max(font_size * 1.2).max(1.0);
    let pad = line_h.max(font_size).max(1.0) * 2.0 + 8.0;
    let logical_w = (measured + spacing + pad * 2.0).max(1.0);
    let logical_h = (line_h + pad * 2.0).max(1.0);
    // Rasterize for the largest stretch of the full CSS-to-device transform,
    // then resample only the residual rotation/skew/anisotropic compression.
    // Limiting resolution, rather than clipping dimensions, preserves the run.
    const MAX_TEXT_LAYER_SIDE: f32 = 16384.0;
    let raster_scale = text_transform_raster_scale(transform)
        .min(MAX_TEXT_LAYER_SIDE / logical_w)
        .min(MAX_TEXT_LAYER_SIDE / logical_h);
    if !raster_scale.is_finite() || raster_scale <= 0.0 {
        return;
    }
    let pix_w = (logical_w * raster_scale).ceil().max(1.0) as u32;
    let pix_h = (logical_h * raster_scale).ceil().max(1.0) as u32;
    let Some(mut layer) = Pixmap::new(pix_w, pix_h) else {
        return;
    };
    layer.fill(tiny_skia::Color::TRANSPARENT);

    draw_text_cmd(
        &mut layer,
        font_system,
        swash_cache,
        raster_scale,
        pad,
        pad,
        text,
        font_family,
        font_size,
        font_weight,
        font_style,
        font_stretch,
        line_height,
        color,
        decoration,
        letter_spacing,
        word_spacing,
        small_caps,
        None,
    );

    let origin_x = x - pad;
    let origin_y = y - pad;
    let image_transform = transform
        .pre_translate(origin_x, origin_y)
        .pre_scale(1.0 / raster_scale, 1.0 / raster_scale);
    pixmap.draw_pixmap(
        0,
        0,
        layer.as_ref(),
        &tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bilinear,
            ..Default::default()
        },
        image_transform,
        clip_mask,
    );
}

fn text_transform_raster_scale(transform: Transform) -> f32 {
    // Largest singular value of the linear part, including device pixel ratio.
    let (a, b, c, d) = (
        f64::from(transform.sx),
        f64::from(transform.ky),
        f64::from(transform.kx),
        f64::from(transform.sy),
    );
    let xx = a * a + b * b;
    let yy = c * c + d * d;
    let xy = a * c + b * d;
    ((xx + yy + (xx - yy).hypot(2.0 * xy)) / 2.0).sqrt() as f32
}

fn underline_skip_ink_segments(text: &str, x: f32, width: f32) -> Vec<(f32, f32)> {
    let count = text.chars().count();
    if count == 0 || width <= 0.0 {
        return Vec::new();
    }
    let advance = width / count as f32;
    let mut out = Vec::new();
    let mut start = x;
    let mut cursor = x;
    for ch in text.chars() {
        let next = cursor + advance;
        if matches!(
            ch,
            'g' | 'j' | 'p' | 'q' | 'y' | 'G' | 'J' | 'Q' | ',' | ';'
        ) {
            if cursor > start {
                out.push((start, cursor - start));
            }
            start = next;
        }
        cursor = next;
    }
    if cursor > start {
        out.push((start, cursor - start));
    }
    out
}

fn outer_shadow_has_no_pixels_in_surface(
    border_rect: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
    surface_width: u32,
    surface_height: u32,
) -> bool {
    if !scale.is_finite()
        || scale <= 0.0
        || !border_rect.x.is_finite()
        || !border_rect.y.is_finite()
        || !border_rect.w.is_finite()
        || !border_rect.h.is_finite()
        || border_rect.w <= 0.0
        || border_rect.h <= 0.0
        || radii
            .iter()
            .chain(radii_y.iter())
            .any(|radius| !radius.is_finite())
    {
        return false;
    }
    let radius_x = radii.into_iter().fold(0.0_f32, f32::max);
    let radius_y = radii_y.into_iter().fold(0.0_f32, f32::max);
    let guard = 2.0 / scale;
    scroll_x >= border_rect.x + radius_x + guard
        && scroll_y >= border_rect.y + radius_y + guard
        && scroll_x + surface_width as f32 / scale <= border_rect.right() - radius_x - guard
        && scroll_y + surface_height as f32 / scale <= border_rect.bottom() - radius_y - guard
}

fn clear_outer_shadow_interior(
    layer: &mut Pixmap,
    border_rect: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    transform: Transform,
) {
    // Outer shadows are clipped outside the border box even when its
    // background is transparent. Cut out after blur, not before it.
    let mut paint = Paint::default();
    paint.set_color(tiny_skia::Color::BLACK);
    paint.blend_mode = tiny_skia::BlendMode::DestinationOut;
    if let Some(path) = rounded_rect_path_corners_xy(
        border_rect.x,
        border_rect.y,
        border_rect.w,
        border_rect.h,
        radii,
        radii_y,
    ) {
        layer.fill_path(&path, &paint, FillRule::Winding, transform, None);
    }
}

fn outer_shadow_radii(
    border_rect: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    spread: f32,
) -> ([f32; 4], [f32; 4]) {
    fn adjusted_radius(radius: f32, spread: f32, coverage: f32) -> f32 {
        if radius <= 0.0 {
            return 0.0;
        }
        if spread <= 0.0 || radius > spread || coverage > 1.0 {
            return (radius + spread).max(0.0);
        }
        let ratio = radius / spread;
        radius + spread * (1.0 - (1.0 - ratio).powi(3) * (1.0 - coverage.powi(3)))
    }

    let mut out_x = [0.0; 4];
    let mut out_y = [0.0; 4];
    for i in 0..4 {
        let coverage = if border_rect.w > 0.0 && border_rect.h > 0.0 {
            2.0 * (radii[i] / border_rect.w).min(radii_y[i] / border_rect.h)
        } else {
            0.0
        };
        out_x[i] = adjusted_radius(radii[i], spread, coverage);
        out_y[i] = adjusted_radius(radii_y[i], spread, coverage);
    }
    (out_x, out_y)
}

fn paint_cached_outer_shadow(
    target: &mut Pixmap,
    rect: Rect,
    sr: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    blur: f32,
    spread: f32,
    color: Color,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
    clip_mask: Option<&tiny_skia::Mask>,
) -> bool {
    let scroll_px_x = scroll_x * scale;
    let scroll_px_y = scroll_y * scale;
    if !scroll_px_x.is_finite()
        || !scroll_px_y.is_finite()
        || scroll_px_x.fract() != 0.0
        || scroll_px_y.fract() != 0.0
    {
        return false;
    }
    let pad = (blur * scale * 4.0 + 4.0).ceil();
    let left = (sr.x * scale - pad).floor();
    let top = (sr.y * scale - pad).floor();
    let right = ((sr.x + sr.w) * scale + pad).ceil();
    let bottom = ((sr.y + sr.h) * scale + pad).ceil();
    if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
        return false;
    }
    let width = (right - left) as u32;
    let height = (bottom - top) as u32;
    if width == 0
        || height == 0
        || (width as usize) * (height as usize) * 4 > ShadowRasterCache::MAX_ENTRY_BYTES
    {
        return false;
    }

    let local_sr = Rect::new(sr.x - left / scale, sr.y - top / scale, sr.w, sr.h);
    let local_rect = Rect::new(rect.x - left / scale, rect.y - top / scale, rect.w, rect.h);
    let local_ts = Transform::from_scale(scale, scale);
    let key = ShadowRasterKey {
        outer: [
            local_sr.x.to_bits(),
            local_sr.y.to_bits(),
            local_sr.w.to_bits(),
            local_sr.h.to_bits(),
        ],
        interior: [
            local_rect.x.to_bits(),
            local_rect.y.to_bits(),
            local_rect.w.to_bits(),
            local_rect.h.to_bits(),
        ],
        radii: std::array::from_fn(|i| {
            if i < 4 {
                radii[i].to_bits()
            } else {
                radii_y[i - 4].to_bits()
            }
        }),
        transform: [scale.to_bits(), 0, 0, scale.to_bits(), 0, 0],
        size: [width, height],
        blur: (blur * scale).to_bits(),
        spread: spread.to_bits(),
        color: [color.r, color.g, color.b, color.a],
    };
    let layer = SHADOW_RASTER_CACHE
        .with(|cache| cache.borrow_mut().get(&key))
        .or_else(|| {
            let mut layer = Pixmap::new(width, height)?;
            let mut paint = Paint::default();
            paint.set_color(to_sk_color(&color));
            let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
            if max_r > 0.5 {
                let (expanded_radii, expanded_radii_y) =
                    outer_shadow_radii(rect, radii, radii_y, spread);
                if let Some(path) = rounded_rect_path_corners_xy(
                    local_sr.x,
                    local_sr.y,
                    local_sr.w,
                    local_sr.h,
                    expanded_radii,
                    expanded_radii_y,
                ) {
                    layer.fill_path(&path, &paint, FillRule::Winding, local_ts, None);
                }
            } else if let Some(r) =
                SkRect::from_xywh(local_sr.x, local_sr.y, local_sr.w, local_sr.h)
            {
                layer.fill_rect(r, &paint, local_ts, None);
            }
            crate::canvas::blur_pixmap(&mut layer, blur * scale);
            clear_outer_shadow_interior(&mut layer, local_rect, radii, radii_y, local_ts);
            let layer = Arc::new(layer);
            SHADOW_RASTER_CACHE.with(|cache| cache.borrow_mut().insert(key, layer.clone()));
            Some(layer)
        });
    let Some(layer) = layer else {
        return false;
    };
    target.draw_pixmap(
        (left - scroll_px_x) as i32,
        (top - scroll_px_y) as i32,
        layer.as_ref().as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        clip_mask,
    );
    true
}

fn fill_outer_box_shadow_shape(
    target: &mut Pixmap,
    shadow_rect: Rect,
    border_rect: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    spread: f32,
    paint: &Paint,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    if radii.iter().any(|&radius| radius > 0.5) {
        let (outer_radii, outer_radii_y) = outer_shadow_radii(border_rect, radii, radii_y, spread);
        if let (Some(outer), Some(inner)) = (
            rounded_rect_path_corners_xy(
                shadow_rect.x,
                shadow_rect.y,
                shadow_rect.w,
                shadow_rect.h,
                outer_radii,
                outer_radii_y,
            ),
            rounded_rect_path_corners_xy(
                border_rect.x,
                border_rect.y,
                border_rect.w,
                border_rect.h,
                radii,
                radii_y,
            ),
        ) {
            let mut ring = PathBuilder::new();
            ring.push_path(&outer);
            ring.push_path(&inner);
            if let Some(path) = ring.finish() {
                target.fill_path(&path, paint, FillRule::EvenOdd, transform, clip_mask);
            }
        }
        return;
    }
    fn fill_piece(
        target: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        paint: &Paint,
        transform: Transform,
        clip_mask: Option<&tiny_skia::Mask>,
    ) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        if let Some(rect) = SkRect::from_xywh(x, y, w, h) {
            target.fill_rect(rect, paint, transform, clip_mask);
        }
    }

    let shadow_right = shadow_rect.x + shadow_rect.w;
    let shadow_bottom = shadow_rect.y + shadow_rect.h;
    let border_right = border_rect.x + border_rect.w;
    let border_bottom = border_rect.y + border_rect.h;

    let ix0 = shadow_rect.x.max(border_rect.x);
    let iy0 = shadow_rect.y.max(border_rect.y);
    let ix1 = shadow_right.min(border_right);
    let iy1 = shadow_bottom.min(border_bottom);

    if ix0 >= ix1 || iy0 >= iy1 {
        fill_piece(
            target,
            shadow_rect.x,
            shadow_rect.y,
            shadow_rect.w,
            shadow_rect.h,
            paint,
            transform,
            clip_mask,
        );
        return;
    }

    fill_piece(
        target,
        shadow_rect.x,
        shadow_rect.y,
        shadow_rect.w,
        iy0 - shadow_rect.y,
        paint,
        transform,
        clip_mask,
    );
    fill_piece(
        target,
        shadow_rect.x,
        iy1,
        shadow_rect.w,
        shadow_bottom - iy1,
        paint,
        transform,
        clip_mask,
    );
    fill_piece(
        target,
        shadow_rect.x,
        iy0,
        ix0 - shadow_rect.x,
        iy1 - iy0,
        paint,
        transform,
        clip_mask,
    );
    fill_piece(
        target,
        ix1,
        iy0,
        shadow_right - ix1,
        iy1 - iy0,
        paint,
        transform,
        clip_mask,
    );
}

fn paint_blurred_inset_box_shadow(
    target: &mut Pixmap,
    rect: Rect,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    radii: [f32; 4],
    radii_y: [f32; 4],
    paint: &Paint,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
    clip_mask: Option<&tiny_skia::Mask>,
) -> bool {
    let padding = (blur * 4.0 * scale).ceil() as i32 + 2;
    let left = (((rect.x - scroll_x) * scale).floor() as i32 - padding).max(-padding);
    let top = (((rect.y - scroll_y) * scale).floor() as i32 - padding).max(-padding);
    let right = (((rect.right() - scroll_x) * scale).ceil() as i32 + padding)
        .min(target.width() as i32 + padding);
    let bottom = (((rect.bottom() - scroll_y) * scale).ceil() as i32 + padding)
        .min(target.height() as i32 + padding);
    if right <= 0 || bottom <= 0 || left >= target.width() as i32 || top >= target.height() as i32 {
        return true;
    }
    if right <= left || bottom <= top {
        return true;
    }
    let width = (right - left) as u32;
    let height = (bottom - top) as u32;
    let (Some(mut layer), Some(mut mask)) = (
        Pixmap::new(width, height),
        tiny_skia::Mask::new(width, height),
    ) else {
        return false;
    };
    let local_ts = Transform::from_scale(scale, scale).pre_translate(
        -scroll_x - left as f32 / scale.max(0.001),
        -scroll_y - top as f32 / scale.max(0.001),
    );
    raster_blurred_inset_box_shadow(
        &mut layer, &mut mask, rect, offset_x, offset_y, blur, spread, radii, radii_y, paint,
        scale, local_ts,
    );
    target.draw_pixmap(
        left,
        top,
        layer.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        clip_mask,
    );
    true
}

fn paint_transformed_blurred_inset_box_shadow(
    target: &mut Pixmap,
    rect: Rect,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    radii: [f32; 4],
    radii_y: [f32; 4],
    paint: &Paint,
    scale: f32,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) -> bool {
    if !scale.is_finite() || scale <= 0.0 || !rect.w.is_finite() || !rect.h.is_finite() {
        return false;
    }
    let pad = blur * 4.0 + 2.0 / scale;
    let origin_x = rect.x - pad;
    let origin_y = rect.y - pad;
    let width = ((rect.w + pad * 2.0) * scale).ceil();
    let height = ((rect.h + pad * 2.0) * scale).ceil();
    if !(1.0..=u32::MAX as f32).contains(&width) || !(1.0..=u32::MAX as f32).contains(&height) {
        return false;
    }
    let (Some(mut layer), Some(mut mask)) = (
        Pixmap::new(width as u32, height as u32),
        tiny_skia::Mask::new(width as u32, height as u32),
    ) else {
        return false;
    };
    let local_transform = Transform::from_scale(scale, scale).pre_translate(-origin_x, -origin_y);
    raster_blurred_inset_box_shadow(
        &mut layer,
        &mut mask,
        rect,
        offset_x,
        offset_y,
        blur,
        spread,
        radii,
        radii_y,
        paint,
        scale,
        local_transform,
    );
    let image_transform = transform
        .pre_translate(origin_x, origin_y)
        .pre_scale(1.0 / scale, 1.0 / scale);
    target.draw_pixmap(
        0,
        0,
        layer.as_ref(),
        &tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bilinear,
            ..Default::default()
        },
        image_transform,
        clip_mask,
    );
    true
}

fn raster_blurred_inset_box_shadow(
    layer: &mut Pixmap,
    mask: &mut tiny_skia::Mask,
    rect: Rect,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    radii: [f32; 4],
    radii_y: [f32; 4],
    paint: &Paint,
    scale: f32,
    local_transform: Transform,
) {
    if let Some(full) = SkRect::from_xywh(0.0, 0.0, layer.width() as f32, layer.height() as f32) {
        layer.fill_rect(full, paint, Transform::identity(), None);
    }
    let inner = Rect::new(
        rect.x + offset_x + spread,
        rect.y + offset_y + spread,
        rect.w - spread * 2.0,
        rect.h - spread * 2.0,
    );
    if inner.w > 0.0 && inner.h > 0.0 {
        let inner_radii = radii.map(|r| (r - spread).max(0.0));
        let inner_radii_y = radii_y.map(|r| (r - spread).max(0.0));
        if let Some(path) = rounded_rect_path_corners_xy(
            inner.x,
            inner.y,
            inner.w,
            inner.h,
            inner_radii,
            inner_radii_y,
        ) {
            let mut clear = Paint::default();
            clear.blend_mode = tiny_skia::BlendMode::Clear;
            layer.fill_path(&path, &clear, FillRule::Winding, local_transform, None);
        }
    }
    crate::canvas::blur_pixmap(layer, blur * scale);
    if let Some(path) = rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, radii, radii_y)
    {
        mask.fill_path(&path, FillRule::Winding, true, local_transform);
        layer.apply_mask(mask);
    }
}

fn fill_inset_box_shadow_shape(
    target: &mut Pixmap,
    rect: Rect,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    radii: [f32; 4],
    radii_y: [f32; 4],
    paint: &Paint,
    transform: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    fn fill_piece(
        target: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        paint: &Paint,
        transform: Transform,
        clip_mask: Option<&tiny_skia::Mask>,
    ) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        if let Some(rect) = SkRect::from_xywh(x, y, w, h) {
            target.fill_rect(rect, paint, transform, clip_mask);
        }
    }

    let blur_extent = blur.max(0.0) * 2.0;
    let top = (offset_y + spread + blur_extent).max(0.0).min(rect.h);
    let bottom = (spread - offset_y + blur_extent).max(0.0).min(rect.h);
    let left = (offset_x + spread + blur_extent).max(0.0).min(rect.w);
    let right = (spread - offset_x + blur_extent).max(0.0).min(rect.w);

    if radii
        .iter()
        .chain(radii_y.iter())
        .any(|&radius| radius > 0.0)
    {
        if let Some(outer) =
            rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, radii, radii_y)
        {
            let mut ring = PathBuilder::new();
            ring.push_path(&outer);
            let inner_w = rect.w - left - right;
            let inner_h = rect.h - top - bottom;
            if inner_w > 0.0 && inner_h > 0.0 {
                let inner_radii = [
                    (radii[0] - spread).max(0.0),
                    (radii[1] - spread).max(0.0),
                    (radii[2] - spread).max(0.0),
                    (radii[3] - spread).max(0.0),
                ];
                let inner_radii_y = [
                    (radii_y[0] - spread).max(0.0),
                    (radii_y[1] - spread).max(0.0),
                    (radii_y[2] - spread).max(0.0),
                    (radii_y[3] - spread).max(0.0),
                ];
                if let Some(inner) = rounded_rect_path_corners_xy(
                    rect.x + left,
                    rect.y + top,
                    inner_w,
                    inner_h,
                    inner_radii,
                    inner_radii_y,
                ) {
                    ring.push_path(&inner);
                }
            }
            if let Some(path) = ring.finish() {
                target.fill_path(&path, paint, FillRule::EvenOdd, transform, clip_mask);
                return;
            }
        }
    }

    fill_piece(
        target, rect.x, rect.y, rect.w, top, paint, transform, clip_mask,
    );
    fill_piece(
        target,
        rect.x,
        rect.bottom() - bottom,
        rect.w,
        bottom,
        paint,
        transform,
        clip_mask,
    );
    let vertical_y = rect.y + top;
    let vertical_h = (rect.h - top - bottom).max(0.0);
    fill_piece(
        target, rect.x, vertical_y, left, vertical_h, paint, transform, clip_mask,
    );
    fill_piece(
        target,
        rect.right() - right,
        vertical_y,
        right,
        vertical_h,
        paint,
        transform,
        clip_mask,
    );
}

/// Composite a layer onto the destination pixmap with a blend mode.
fn blend_composite(dst: &mut Pixmap, src: &Pixmap, mode: u8) {
    if mode == 1 {
        blend_common_multiply(dst, src);
        return;
    }
    use tiny_skia::BlendMode;
    let blend_mode = match mode {
        2 => BlendMode::Screen,
        3 => BlendMode::Overlay,
        4 => BlendMode::Darken,
        5 => BlendMode::Lighten,
        6 => BlendMode::ColorDodge,
        7 => BlendMode::ColorBurn,
        8 => BlendMode::HardLight,
        9 => BlendMode::SoftLight,
        10 => BlendMode::Difference,
        11 => BlendMode::Exclusion,
        12 => BlendMode::Hue,
        13 => BlendMode::Saturation,
        14 => BlendMode::Color,
        15 => BlendMode::Luminosity,
        _ => BlendMode::SourceOver,
    };
    dst.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &tiny_skia::PixmapPaint {
            blend_mode,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );
}

fn blend_common_multiply(dst: &mut Pixmap, src: &Pixmap) {
    let pixels = src.pixels();
    let Some(common) = pixels
        .get(pixels.len() / 2)
        .filter(|pixel| pixel.alpha() != 0)
        .or_else(|| pixels.iter().find(|pixel| pixel.alpha() != 0))
    else {
        return;
    };
    let color = (common.red(), common.green(), common.blue(), common.alpha());

    let alpha = color.3 as u32;
    let source = [color.0 as u32, color.1 as u32, color.2 as u32];
    let mut table = [[0u8; 256]; 3];
    for (channel, &premultiplied) in source.iter().enumerate() {
        let straight = premultiplied * 255 / alpha;
        for value in 0..=255u32 {
            let blended = value * straight / 255;
            table[channel][value as usize] =
                (blended * alpha / 255 + value * (255 - alpha) / 255).min(255) as u8;
        }
    }

    for (pixel, source) in dst.pixels_mut().iter_mut().zip(pixels) {
        if source.alpha() == 0 {
            continue;
        }
        if pixel.alpha() == 255
            && (source.red(), source.green(), source.blue(), source.alpha()) == color
        {
            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                table[0][pixel.red() as usize],
                table[1][pixel.green() as usize],
                table[2][pixel.blue() as usize],
                255,
            )
            .unwrap();
        } else {
            let sa = source.alpha() as u32;
            let da = pixel.alpha() as u32;
            let blend = |s: u8, d: u8| {
                let s = s as u32;
                let d = d as u32;
                // Premultiplied source-over with multiply:
                // Cs*(1-Ab) + Cb*(1-As) + Cs*Cb.
                if da != 255 {
                    return ((s * (255 - da) + d * (255 - sa) + s * d + 127) / 255).min(255) as u8;
                }
                let straight_source = s * 255 / sa;
                let straight_dest = d.min(255);
                let multiplied = straight_dest * straight_source / 255;
                (multiplied * sa / 255 + d * (255 - sa) / 255).min(255) as u8
            };
            *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
                blend(source.red(), pixel.red()),
                blend(source.green(), pixel.green()),
                blend(source.blue(), pixel.blue()),
                (sa + da * (255 - sa) / 255).min(255) as u8,
            )
            .unwrap();
        }
    }
}

#[cfg(test)]
#[test]
fn bounded_backdrop_filter_matches_full_surface() {
    let mut original = Pixmap::new(96, 72).unwrap();
    for (index, pixel) in original.pixels_mut().iter_mut().enumerate() {
        let x = index % 96;
        let y = index / 96;
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
            (x * 2) as u8,
            (y * 3) as u8,
            ((x + y) * 2) as u8,
            255,
        )
        .unwrap();
    }
    let rect = Rect::new(17.25, 11.5, 23.5, 19.25);
    for (kind, value) in [(1, 1.5), (0, 2.0)] {
        let mut bounded = original.clone();
        let mut expected = original.clone();
        let mut full_backdrop = expected.clone();
        apply_pixel_filter(&mut full_backdrop, kind, value);
        let mask = build_clip_mask(&rect, &[0.0; 4], &[0.0; 4], 96, 72, 1.0, 0.0, 0.0);
        expected.draw_pixmap(
            0,
            0,
            full_backdrop.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            mask.as_ref(),
        );
        replay_commands_inner(
            &[PaintCmd::BackdropFilter {
                rect,
                radii: [0.0; 4],
                radii_y: [0.0; 4],
                filters: vec![(kind, value, 0.0, 0.0, crate::types::Color::BLACK)],
            }],
            &mut bounded,
            1.0,
            None,
            0.0,
            0.0,
            None,
            None,
            None,
        );
        assert_eq!(bounded.data(), expected.data(), "filter kind {kind}");
    }
}

#[cfg(test)]
#[test]
fn solid_multiply_matches_general_blend_for_opaque_backdrop() {
    let mut destination = Pixmap::new(4, 1).unwrap();
    let mut source = Pixmap::new(4, 1).unwrap();
    let overlay = tiny_skia::PremultipliedColorU8::from_rgba(4, 7, 22, 61).unwrap();
    for (index, pixel) in destination.pixels_mut().iter_mut().enumerate() {
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(
            [0, 64, 128, 255][index],
            [255, 128, 32, 0][index],
            [17, 31, 63, 127][index],
            255,
        )
        .unwrap();
        source.pixels_mut()[index] = overlay;
    }
    source.pixels_mut()[0] = tiny_skia::PremultipliedColorU8::from_rgba(1, 2, 6, 17).unwrap();
    source.pixels_mut()[3] = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
    let original = destination.clone();
    blend_common_multiply(&mut destination, &source);
    for ((actual, before), overlay) in destination
        .pixels()
        .iter()
        .zip(original.pixels())
        .zip(source.pixels())
    {
        if overlay.alpha() == 0 {
            assert_eq!(actual, before);
            continue;
        }
        let expected = |d: u8, s: u8| {
            let d = d as u32;
            let s = s as u32;
            let alpha = overlay.alpha() as u32;
            let straight = s * 255 / alpha;
            let multiplied = d * straight / 255;
            (multiplied * alpha / 255 + d * (255 - alpha) / 255) as u8
        };
        assert_eq!(actual.red(), expected(before.red(), overlay.red()));
        assert_eq!(actual.green(), expected(before.green(), overlay.green()));
        assert_eq!(actual.blue(), expected(before.blue(), overlay.blue()));
        assert_eq!(actual.alpha(), 255);
    }
}

#[cfg(test)]
#[test]
fn multiply_preserves_source_over_on_transparent_and_translucent_backdrops() {
    let alphas = [0, 1, 64, 128, 254, 255];
    let mut destination = Pixmap::new(36, 1).unwrap();
    let mut source = Pixmap::new(36, 1).unwrap();
    for (i, &da) in alphas.iter().enumerate() {
        for (j, &sa) in alphas.iter().enumerate() {
            destination.pixels_mut()[i * 6 + j] =
                tiny_skia::ColorU8::from_rgba(199, 73, 41, da).premultiply();
            source.pixels_mut()[i * 6 + j] =
                tiny_skia::ColorU8::from_rgba(39, 147, 221, sa).premultiply();
        }
    }
    let mut expected = destination.clone();
    expected.draw_pixmap(
        0,
        0,
        source.as_ref(),
        &tiny_skia::PixmapPaint {
            blend_mode: tiny_skia::BlendMode::Multiply,
            ..Default::default()
        },
        Transform::identity(),
        None,
    );
    blend_common_multiply(&mut destination, &source);
    for (i, (actual, expected)) in destination.data().iter().zip(expected.data()).enumerate() {
        assert!(
            (i32::from(*actual) - i32::from(*expected)).abs() <= 2,
            "byte {i}: {actual} vs {expected}"
        );
    }
}

/// Apply a CSS filter to a pixmap's pixels in-place.
/// filter_type: 0=blur,1=brightness,2=contrast,3=grayscale,4=hue-rotate,5=invert,6=opacity,7=saturate,8=sepia
///
/// `pub(crate)` so `canvas::effects` can reach the colour maths instead of
/// keeping a second copy of it.
pub(crate) fn apply_pixel_filter(pm: &mut Pixmap, filter_type: u8, value: f32) {
    if filter_type == 0 {
        crate::canvas::blur_pixmap(pm, value);
        return;
    }

    let pixels = pm.pixels_mut();

    // Helper: un-premultiply, apply transform, re-premultiply
    let process = |px: &mut tiny_skia::PremultipliedColorU8,
                   f: &dyn Fn(f32, f32, f32) -> (f32, f32, f32)| {
        let a = px.alpha();
        if a == 0 {
            return;
        }
        // Un-premultiply
        let af = a as f32 / 255.0;
        let r = px.red() as f32 / af;
        let g = px.green() as f32 / af;
        let b = px.blue() as f32 / af;
        let (r2, g2, b2) = f(r, g, b);
        // Re-premultiply
        let pr = (r2 * af).round().clamp(0.0, 255.0) as u8;
        let pg = (g2 * af).round().clamp(0.0, 255.0) as u8;
        let pb = (b2 * af).round().clamp(0.0, 255.0) as u8;
        if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(pr, pg, pb, a) {
            *px = p;
        }
    };

    match filter_type {
        0 => unreachable!(),
        1 => {
            // brightness
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    (
                        (r * value).min(255.0),
                        (g * value).min(255.0),
                        (b * value).min(255.0),
                    )
                });
            }
        }
        2 => {
            // contrast
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let adj = |c: f32| ((c / 255.0 - 0.5) * value + 0.5) * 255.0;
                    (
                        adj(r).clamp(0.0, 255.0),
                        adj(g).clamp(0.0, 255.0),
                        adj(b).clamp(0.0, 255.0),
                    )
                });
            }
        }
        3 => {
            // grayscale
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let mix = |c: f32| c * (1.0 - value) + lum * value;
                    (mix(r), mix(g), mix(b))
                });
            }
        }
        4 => {
            // hue-rotate
            let rad = value * std::f32::consts::PI / 180.0;
            let cos = rad.cos();
            let sin = rad.sin();
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let (rf, gf, bf) = (r / 255.0, g / 255.0, b / 255.0);
                    let r2 = ((0.213 + 0.787 * cos - 0.213 * sin) * rf
                        + (0.715 - 0.715 * cos - 0.715 * sin) * gf
                        + (0.072 - 0.072 * cos + 0.928 * sin) * bf)
                        * 255.0;
                    let g2 = ((0.213 - 0.213 * cos + 0.143 * sin) * rf
                        + (0.715 + 0.285 * cos + 0.140 * sin) * gf
                        + (0.072 - 0.072 * cos - 0.283 * sin) * bf)
                        * 255.0;
                    let b2 = ((0.213 - 0.213 * cos - 0.787 * sin) * rf
                        + (0.715 - 0.715 * cos + 0.715 * sin) * gf
                        + (0.072 + 0.928 * cos + 0.072 * sin) * bf)
                        * 255.0;
                    (
                        r2.clamp(0.0, 255.0),
                        g2.clamp(0.0, 255.0),
                        b2.clamp(0.0, 255.0),
                    )
                });
            }
        }
        5 => {
            // invert
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let inv = |c: f32| (255.0 - c) * value + c * (1.0 - value);
                    (inv(r), inv(g), inv(b))
                });
            }
        }
        6 => {
            // opacity — operates on premultiplied alpha directly
            for px in pixels.iter_mut() {
                let a = px.alpha();
                if a == 0 {
                    continue;
                }
                let new_a = (a as f32 * value).round().clamp(0.0, 255.0) as u8;
                let scale_factor = if a > 0 { new_a as f32 / a as f32 } else { 0.0 };
                let pr = (px.red() as f32 * scale_factor).round().clamp(0.0, 255.0) as u8;
                let pg = (px.green() as f32 * scale_factor).round().clamp(0.0, 255.0) as u8;
                let pb = (px.blue() as f32 * scale_factor).round().clamp(0.0, 255.0) as u8;
                if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(pr, pg, pb, new_a) {
                    *px = p;
                }
            }
        }
        7 => {
            // saturate
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                    let sat = |c: f32| (lum + (c - lum) * value).clamp(0.0, 255.0);
                    (sat(r), sat(g), sat(b))
                });
            }
        }
        8 => {
            // sepia
            for px in pixels.iter_mut() {
                process(px, &|r, g, b| {
                    let sr = (0.393 * r + 0.769 * g + 0.189 * b).min(255.0);
                    let sg = (0.349 * r + 0.686 * g + 0.168 * b).min(255.0);
                    let sb = (0.272 * r + 0.534 * g + 0.131 * b).min(255.0);
                    let mix = |c: f32, s: f32| c * (1.0 - value) + s * value;
                    (mix(r, sr), mix(g, sg), mix(b, sb))
                });
            }
        }
        _ => {} // drop-shadow or unknown
    }
}

fn transformed_text_origin(ts: &Transform, x: f32, y: f32) -> (f32, f32, f32) {
    let eff_sx = (ts.sx * ts.sx + ts.ky * ts.ky).sqrt();
    let eff_sy = (ts.kx * ts.kx + ts.sy * ts.sy).sqrt();
    let eff_scale = eff_sx.max(eff_sy).max(0.001);
    let phys_x = ts.sx * x + ts.kx * y + ts.tx;
    let phys_y = ts.ky * x + ts.sy * y + ts.ty;
    (eff_scale, phys_x / eff_scale, phys_y / eff_scale)
}

fn transformed_axis_aligned_rect(ts: &Transform, rect: Rect) -> (f32, Rect) {
    let (scale, x, y) = transformed_text_origin(ts, rect.x, rect.y);
    let sx = (ts.sx * ts.sx + ts.ky * ts.ky).sqrt();
    let sy = (ts.kx * ts.kx + ts.sy * ts.sy).sqrt();
    (
        scale,
        Rect::new(x, y, rect.w * sx / scale, rect.h * sy / scale),
    )
}

fn build_clip_mask(
    rect: &Rect,
    radius: &[f32; 4],
    radius_y: &[f32; 4],
    pw: u32,
    ph: u32,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<tiny_skia::Mask> {
    let ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
    build_clip_mask_with_transform(rect, radius, radius_y, pw, ph, ts)
}

fn build_clip_mask_with_transform(
    rect: &Rect,
    radius: &[f32; 4],
    radius_y: &[f32; 4],
    pw: u32,
    ph: u32,
    ts: Transform,
) -> Option<tiny_skia::Mask> {
    let mut mask = tiny_skia::Mask::new(pw, ph)?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let max_r = radius[0].max(radius[1]).max(radius[2]).max(radius[3]);
    if max_r > 0.5 {
        if let Some(path) =
            rounded_rect_path_corners_xy(rect.x, rect.y, rect.w, rect.h, *radius, *radius_y)
        {
            mask.fill_path(&path, FillRule::Winding, true, ts);
        }
    } else {
        // Simple rect clip
        if let Some(path) = {
            let mut pb = PathBuilder::new();
            if let Some(r) = SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h) {
                pb.push_rect(r);
            }
            pb.finish()
        } {
            mask.fill_path(&path, FillRule::Winding, true, ts);
        }
    }
    Some(mask)
}

fn build_viewport_clip_mask(clip: Rect, pw: u32, ph: u32, scale: f32) -> Option<tiny_skia::Mask> {
    if clip.w <= 0.0 || clip.h <= 0.0 {
        return None;
    }
    let mut mask = tiny_skia::Mask::new(pw, ph)?;
    let mut pb = PathBuilder::new();
    let x = clip.x * scale;
    let y = clip.y * scale;
    let w = clip.w * scale;
    let h = clip.h * scale;
    if let Some(r) = SkRect::from_xywh(x, y, w, h) {
        pb.push_rect(r);
    }
    if let Some(path) = pb.finish() {
        mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    }
    Some(mask)
}

fn simple_clip_contains_viewport(
    rect: &Rect,
    radius: &[f32; 4],
    radius_y: &[f32; 4],
    pw: u32,
    ph: u32,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> bool {
    if radius.iter().any(|r| *r > 0.5) || radius_y.iter().any(|r| *r > 0.5) {
        return false;
    }
    let inv_scale = 1.0 / scale.max(0.001);
    let view_w = pw as f32 * inv_scale;
    let view_h = ph as f32 * inv_scale;
    rect.x <= scroll_x
        && rect.y <= scroll_y
        && rect.right() >= scroll_x + view_w
        && rect.bottom() >= scroll_y + view_h
}

fn build_polygon_clip_mask(
    points: &[(f32, f32)],
    even_odd: bool,
    pw: u32,
    ph: u32,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<tiny_skia::Mask> {
    let ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
    build_polygon_clip_mask_with_transform(points, even_odd, pw, ph, ts)
}

fn build_polygon_clip_mask_with_transform(
    points: &[(f32, f32)],
    even_odd: bool,
    pw: u32,
    ph: u32,
    ts: Transform,
) -> Option<tiny_skia::Mask> {
    let mut mask = tiny_skia::Mask::new(pw, ph)?;
    if points.len() < 3 {
        // A valid degenerate polygon clips everything, rather than disabling clipping.
        return Some(mask);
    }
    let mut pb = PathBuilder::new();
    let (x0, y0) = points[0];
    pb.move_to(x0, y0);
    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        let fill_rule = if even_odd {
            FillRule::EvenOdd
        } else {
            FillRule::Winding
        };
        mask.fill_path(&path, fill_rule, true, ts);
    }
    Some(mask)
}

fn polygon_bounds(points: &[(f32, f32)]) -> Option<Rect> {
    let (&(first_x, first_y), rest) = points.split_first()?;
    let (mut min_x, mut max_x) = (first_x, first_x);
    let (mut min_y, mut max_y) = (first_y, first_y);
    for &(x, y) in rest {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    Some(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
}

fn rounded_rect_path_corners(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r_tl: f32,
    r_tr: f32,
    r_br: f32,
    r_bl: f32,
    r_tl_y: f32,
    r_tr_y: f32,
    r_br_y: f32,
    r_bl_y: f32,
) -> Option<tiny_skia::Path> {
    rounded_rect_path_corners_xy(
        x,
        y,
        w,
        h,
        [r_tl, r_tr, r_br, r_bl],
        [r_tl_y, r_tr_y, r_br_y, r_bl_y],
    )
}

fn rounded_rect_side_path(
    side: usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii_x: [f32; 4],
    radii_y: [f32; 4],
) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let ([tl_x, tr_x, br_x, bl_x], [tl_y, tr_y, br_y, bl_y]) =
        reduce_corner_radii_xy(w, h, radii_x, radii_y);
    const K: f32 = 0.552_284_8;
    let mut pb = PathBuilder::new();
    match side {
        0 => {
            pb.move_to(x, y + tl_y);
            if tl_x > 0.0 || tl_y > 0.0 {
                pb.cubic_to(x, y + tl_y - tl_y * K, x + tl_x - tl_x * K, y, x + tl_x, y);
            }
            pb.line_to(x + w - tr_x, y);
            if tr_x > 0.0 || tr_y > 0.0 {
                pb.cubic_to(
                    x + w - tr_x + tr_x * K,
                    y,
                    x + w,
                    y + tr_y - tr_y * K,
                    x + w,
                    y + tr_y,
                );
            }
        }
        1 => {
            pb.move_to(x + w - tr_x, y);
            if tr_x > 0.0 || tr_y > 0.0 {
                pb.cubic_to(
                    x + w - tr_x + tr_x * K,
                    y,
                    x + w,
                    y + tr_y - tr_y * K,
                    x + w,
                    y + tr_y,
                );
            }
            pb.line_to(x + w, y + h - br_y);
            if br_x > 0.0 || br_y > 0.0 {
                pb.cubic_to(
                    x + w,
                    y + h - br_y + br_y * K,
                    x + w - br_x + br_x * K,
                    y + h,
                    x + w - br_x,
                    y + h,
                );
            }
        }
        2 => {
            pb.move_to(x + w, y + h - br_y);
            if br_x > 0.0 || br_y > 0.0 {
                pb.cubic_to(
                    x + w,
                    y + h - br_y + br_y * K,
                    x + w - br_x + br_x * K,
                    y + h,
                    x + w - br_x,
                    y + h,
                );
            }
            pb.line_to(x + bl_x, y + h);
            if bl_x > 0.0 || bl_y > 0.0 {
                pb.cubic_to(
                    x + bl_x - bl_x * K,
                    y + h,
                    x,
                    y + h - bl_y + bl_y * K,
                    x,
                    y + h - bl_y,
                );
            }
        }
        _ => {
            pb.move_to(x + bl_x, y + h);
            if bl_x > 0.0 || bl_y > 0.0 {
                pb.cubic_to(
                    x + bl_x - bl_x * K,
                    y + h,
                    x,
                    y + h - bl_y + bl_y * K,
                    x,
                    y + h - bl_y,
                );
            }
            pb.line_to(x, y + tl_y);
            if tl_x > 0.0 || tl_y > 0.0 {
                pb.cubic_to(x, y + tl_y - tl_y * K, x + tl_x - tl_x * K, y, x + tl_x, y);
            }
        }
    }
    pb.finish()
}

fn rounded_rect_path_corners_xy(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii_x: [f32; 4],
    radii_y: [f32; 4],
) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let ([tl_x, tr_x, br_x, bl_x], [tl_y, tr_y, br_y, bl_y]) =
        reduce_corner_radii_xy(w, h, radii_x, radii_y);
    // Approximate each quarter ellipse with a cubic Bézier. A single
    // quadratic-to-corner makes `border-radius: 50%` visibly squarish, which is
    // especially obvious on animated dots/spinners.
    const K: f32 = 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl_x, y);
    pb.line_to(x + w - tr_x, y);
    if tr_x > 0.0 || tr_y > 0.0 {
        pb.cubic_to(
            x + w - tr_x + tr_x * K,
            y,
            x + w,
            y + tr_y - tr_y * K,
            x + w,
            y + tr_y,
        );
    }
    pb.line_to(x + w, y + h - br_y);
    if br_x > 0.0 || br_y > 0.0 {
        pb.cubic_to(
            x + w,
            y + h - br_y + br_y * K,
            x + w - br_x + br_x * K,
            y + h,
            x + w - br_x,
            y + h,
        );
    }
    pb.line_to(x + bl_x, y + h);
    if bl_x > 0.0 || bl_y > 0.0 {
        pb.cubic_to(
            x + bl_x - bl_x * K,
            y + h,
            x,
            y + h - bl_y + bl_y * K,
            x,
            y + h - bl_y,
        );
    }
    pb.line_to(x, y + tl_y);
    if tl_x > 0.0 || tl_y > 0.0 {
        pb.cubic_to(x, y + tl_y - tl_y * K, x + tl_x - tl_x * K, y, x + tl_x, y);
    }
    pb.close();
    pb.finish()
}

#[cfg(test)]
pub(crate) fn reduce_corner_radii(w: f32, h: f32, radii: [f32; 4]) -> [f32; 4] {
    reduce_corner_radii_xy(w, h, radii, radii).0
}
