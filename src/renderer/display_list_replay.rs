//! Display list replay — rasterize paint commands to a pixmap.
//!
//! This replays a DisplayList (built by display_list_builder) to a
//! tiny_skia Pixmap, handling fills, borders, text, images, clips,
//! opacity, and transforms.

use super::display_list::{DisplayList, ImageRef, PaintCmd};
use crate::types::{Color, GradientDirection, Rect};
use cosmic_text::{
    Attrs, Buffer, Color as CTextColor, FontSystem, Metrics, Shaping, Style as CTextStyle,
    SwashCache, Weight as CTextWeight,
};
use tiny_skia::{
    Color as SkColor, FillRule, Paint, PathBuilder, Pixmap, Rect as SkRect, Transform,
};

/// Replay a display list onto a pixmap (no text — use replay_with_text for full rendering).
pub fn replay(list: &DisplayList, pixmap: &mut Pixmap, scale: f32) {
    replay_inner(list, pixmap, scale, None, 0.0, 0.0);
}

/// Replay with text rendering via cosmic_text.
/// How far outside the viewport a command is still painted. Generous, because
/// a command's own bounds do not account for shadows, outlines or decoration
/// that spill beyond them.
const CULL_MARGIN: f32 = 512.0;

thread_local! {
    /// (font-DB face count, shaped buffers by text+attrs). See the note at the
    /// shaping site: this is the difference between re-shaping every visible
    /// text run on every frame and shaping each distinct one once.
    static SHAPED: std::cell::RefCell<(usize, std::collections::HashMap<u64, Buffer>)> =
        std::cell::RefCell::new((usize::MAX, std::collections::HashMap::new()));
}

/// The document-space vertical extent of a DRAWING command, or `None` for a
/// command that must never be skipped (anything that manipulates a stack, and
/// anything whose extent is not simply its rect).
fn cmd_y_range(cmd: &PaintCmd) -> Option<(f32, f32)> {
    match cmd {
        PaintCmd::FillRect { rect, .. }
        | PaintCmd::Border { rect, .. }
        | PaintCmd::BorderImage { rect, .. }
        | PaintCmd::Image { rect, .. }
        | PaintCmd::Gradient { rect, .. }
        | PaintCmd::BackdropFilter { rect, .. }
        | PaintCmd::Outline { rect, .. }
        | PaintCmd::ResizeGrip { rect, .. } => Some((rect.y, rect.y + rect.h)),
        PaintCmd::Text {
            y,
            font_size,
            line_height,
            ..
        } => {
            let pad = font_size.max(*line_height) * 2.0;
            Some((y - pad, y + pad))
        }
        _ => None,
    }
}

pub fn replay_with_text(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    replay_inner(
        list,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        0.0,
        0.0,
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
    replay_inner(
        list,
        pixmap,
        scale,
        Some((font_system, swash_cache)),
        scroll_x,
        scroll_y,
    );
}

/// Layer for blend mode / opacity compositing.
struct Layer {
    pixmap: Pixmap,
    blend_mode: u8,
    alpha: f32,
}

fn replay_inner(
    list: &DisplayList,
    pixmap: &mut Pixmap,
    scale: f32,
    mut text_ctx: Option<(&mut FontSystem, &mut SwashCache)>,
    scroll_x: f32,
    scroll_y: f32,
) {
    // Start with scale + scroll translation. Display list is in document
    // coordinates; the scroll offset maps to screen coordinates.
    let mut ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
    let mut transform_stack: Vec<Transform> = Vec::new();
    let mut filter_stack: Vec<Vec<(u8, f32, f32, f32, crate::types::Color)>> = Vec::new();
    let mut clip_stack: Vec<Rect> = Vec::new();
    let mut clip_mask_stack: Vec<Option<tiny_skia::Mask>> = Vec::new();
    let mut layer_stack: Vec<Layer> = Vec::new();
    let mut mask_stack: Vec<(Rect, ImageRef)> = Vec::new();

    let pw = pixmap.width();
    let ph = pixmap.height();

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
    let vis_top = scroll_y - CULL_MARGIN;
    let vis_bot = scroll_y + (ph as f32) / scale.max(0.001) + CULL_MARGIN;
    let mut transform_depth = 0i32;

    for cmd in &list.commands {
        match cmd {
            PaintCmd::PushTransform { .. } => transform_depth += 1,
            PaintCmd::PopTransform => transform_depth -= 1,
            _ => {}
        }
        // ⛔ Also never while a LAYER is active. `PushOpacity`, `PushFilter`
        // and `PushBlendMode` redirect drawing into an offscreen pixmap that is
        // composited later, so a command inside one cannot be judged against
        // the document band the way an ordinary command can.
        if transform_depth == 0 && layer_stack.is_empty() {
            if let Some((top, bot)) = cmd_y_range(cmd) {
                if bot < vis_top || top > vis_bot {
                    continue;
                }
            }
        }
        // Get the current clip mask (topmost on the stack)
        let clip_mask = clip_mask_stack.last().and_then(|m| m.as_ref());

        match cmd {
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
                if max_r > 0.5 {
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
                styles: _,
                radii,
                radii_y,
            } => {
                let alpha = 1.0;
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

                if max_r > 0.5 && uniform_width && uniform_color && widths[0] > 0.0 {
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
                } else {
                    // Fallback: draw borders as filled rectangles (no rounding)
                    let mut paint = Paint::default();
                    if widths[0] > 0.0 {
                        paint.set_color(to_sk_color(&apply_opacity(&colors[0], alpha)));
                        if let Some(r) = SkRect::from_xywh(rect.x, rect.y, rect.w, widths[0]) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                    if widths[2] > 0.0 {
                        paint.set_color(to_sk_color(&apply_opacity(&colors[2], alpha)));
                        if let Some(r) = SkRect::from_xywh(
                            rect.x,
                            rect.y + rect.h - widths[2],
                            rect.w,
                            widths[2],
                        ) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                    if widths[3] > 0.0 {
                        paint.set_color(to_sk_color(&apply_opacity(&colors[3], alpha)));
                        if let Some(r) = SkRect::from_xywh(rect.x, rect.y, widths[3], rect.h) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                    if widths[1] > 0.0 {
                        paint.set_color(to_sk_color(&apply_opacity(&colors[1], alpha)));
                        if let Some(r) = SkRect::from_xywh(
                            rect.x + rect.w - widths[1],
                            rect.y,
                            widths[1],
                            rect.h,
                        ) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                }
            }

            PaintCmd::BorderImage {
                rect,
                widths,
                slices,
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
                // Skip text that's entirely outside the current clip region
                // (handles text-indent:-9999px with overflow:hidden)
                if let Some(clip) = clip_stack.last() {
                    if *x + 1000.0 < clip.x
                        || *x > clip.right()
                        || *y + *line_height < clip.y
                        || *y > clip.bottom()
                    {
                        continue;
                    }
                }
                let alpha = 1.0;
                if let Some((ref mut fs, ref mut sc)) = text_ctx {
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    // Apply the current transform to text position and scale.
                    // Extract effective scale factor from the transform matrix.
                    let eff_sx = (ts.sx * ts.sx + ts.ky * ts.ky).sqrt();
                    let eff_sy = (ts.kx * ts.kx + ts.sy * ts.sy).sqrt();
                    let eff_scale = eff_sx.max(eff_sy);
                    // Transform the text origin
                    let phys_x = ts.sx * *x + ts.ky * *y + ts.tx;
                    let phys_y = ts.kx * *x + ts.sy * *y + ts.ty;
                    // draw_text_cmd expects logical coords that it will multiply by scale.
                    // We pass pre-transformed coords divided by eff_scale so the multiplication
                    // brings them back to the correct physical position.
                    let text_x = phys_x / eff_scale;
                    let text_y = phys_y / eff_scale;
                    draw_text_cmd(
                        target,
                        *fs,
                        *sc,
                        eff_scale,
                        text_x,
                        text_y,
                        text,
                        font_family,
                        *font_size,
                        *font_weight,
                        *font_style,
                        *font_stretch,
                        *line_height,
                        &apply_opacity(color, alpha),
                        decoration,
                        *letter_spacing,
                        *word_spacing,
                        *small_caps,
                    );
                }
            }

            PaintCmd::PushClip {
                rect,
                radius,
                radius_y,
            } => {
                clip_stack.push(*rect);
                // Build a clip mask from the clip rect
                let mask =
                    build_clip_mask(rect, radius, radius_y, pw, ph, scale, scroll_x, scroll_y);
                clip_mask_stack.push(mask);
            }
            PaintCmd::PushClipPath { points } => {
                let bounds = polygon_bounds(points).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
                clip_stack.push(bounds);
                let mask = build_polygon_clip_mask(points, pw, ph, scale, scroll_x, scroll_y);
                clip_mask_stack.push(mask);
            }
            PaintCmd::PopClip => {
                clip_stack.pop();
                clip_mask_stack.pop();
            }

            PaintCmd::PushOpacity { alpha } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 0,
                        alpha: alpha.clamp(0.0, 1.0),
                    });
                }
            }
            PaintCmd::PopOpacity => {
                if let Some(layer) = layer_stack.pop() {
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    let paint = tiny_skia::PixmapPaint {
                        opacity: layer.alpha,
                        blend_mode: tiny_skia::BlendMode::SourceOver,
                        quality: tiny_skia::FilterQuality::Nearest,
                    };
                    target.draw_pixmap(
                        0,
                        0,
                        layer.pixmap.as_ref(),
                        &paint,
                        Transform::identity(),
                        None,
                    );
                }
            }

            PaintCmd::PushTransform { transform: m } => {
                // Apply CSS transform by modifying the global transform matrix.
                // The transform matrix m = [a,b,c,d,e,f] is a 2D affine transform
                // that already includes translate-to-origin and translate-back.
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
                    });
                }
            }
            PaintCmd::PopFilter => {
                let filters = filter_stack.pop().unwrap_or_default();
                if let Some(layer) = layer_stack.pop() {
                    let mut pm = layer.pixmap;
                    // Apply each filter to the layer pixels
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
            PaintCmd::BackdropFilter { rect, filters } => {
                let target = layer_stack
                    .last_mut()
                    .map(|l| &mut l.pixmap)
                    .unwrap_or(pixmap);
                let mut backdrop = target.clone();
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
                let mask = build_clip_mask(
                    rect, &[0.0; 4], &[0.0; 4], pw, ph, scale, scroll_x, scroll_y,
                );
                target.draw_pixmap(
                    0,
                    0,
                    backdrop.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    Transform::identity(),
                    mask.as_ref(),
                );
            }
            PaintCmd::PushMask { rect, data } => {
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    mask_stack.push((*rect, data.clone()));
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: 253,
                        alpha: 1.0,
                    });
                }
            }
            PaintCmd::PopMask => {
                if let Some(layer) = layer_stack.pop() {
                    if let Some((rect, data)) = mask_stack.pop() {
                        let target = layer_stack
                            .last_mut()
                            .map(|l| &mut l.pixmap)
                            .unwrap_or(pixmap);
                        composite_masked_layer(
                            target,
                            &layer.pixmap,
                            rect,
                            &data,
                            scale,
                            scroll_x,
                            scroll_y,
                        );
                    }
                }
            }
            PaintCmd::PushBlendMode { mode } => {
                // Create a temporary layer for blend compositing
                if let Some(layer_pixmap) = Pixmap::new(pw, ph) {
                    layer_stack.push(Layer {
                        pixmap: layer_pixmap,
                        blend_mode: *mode,
                        alpha: 1.0,
                    });
                }
            }
            PaintCmd::PopBlendMode => {
                if let Some(layer) = layer_stack.pop() {
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
                if !inset {
                    let sr = Rect::new(
                        rect.x + offset_x - spread,
                        rect.y + offset_y - spread,
                        rect.w + spread * 2.0,
                        rect.h + spread * 2.0,
                    );
                    let c = apply_opacity(color, alpha);
                    let target = layer_stack
                        .last_mut()
                        .map(|l| &mut l.pixmap)
                        .unwrap_or(pixmap);
                    if *blur > 0.0 {
                        if let Some(mut layer) = Pixmap::new(pw, ph) {
                            let mut paint = Paint::default();
                            paint.set_color(to_sk_color(&c));
                            let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
                            if max_r > 0.5 {
                                let expanded_radii = [
                                    (radii[0] + spread).max(0.0),
                                    (radii[1] + spread).max(0.0),
                                    (radii[2] + spread).max(0.0),
                                    (radii[3] + spread).max(0.0),
                                ];
                                let expanded_radii_y = [
                                    (radii_y[0] + spread).max(0.0),
                                    (radii_y[1] + spread).max(0.0),
                                    (radii_y[2] + spread).max(0.0),
                                    (radii_y[3] + spread).max(0.0),
                                ];
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
                            crate::canvas::blur_pixmap(&mut layer, *blur);
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
                        let max_r = radii[0].max(radii[1]).max(radii[2]).max(radii[3]);
                        if max_r > 0.5 {
                            let expanded_radii = [
                                (radii[0] + spread).max(0.0),
                                (radii[1] + spread).max(0.0),
                                (radii[2] + spread).max(0.0),
                                (radii[3] + spread).max(0.0),
                            ];
                            let expanded_radii_y = [
                                (radii_y[0] + spread).max(0.0),
                                (radii_y[1] + spread).max(0.0),
                                (radii_y[2] + spread).max(0.0),
                                (radii_y[3] + spread).max(0.0),
                            ];
                            if let Some(path) = rounded_rect_path_corners_xy(
                                sr.x,
                                sr.y,
                                sr.w,
                                sr.h,
                                expanded_radii,
                                expanded_radii_y,
                            ) {
                                target.fill_path(&path, &paint, FillRule::Winding, ts, clip_mask);
                            }
                        } else if let Some(r) = SkRect::from_xywh(sr.x, sr.y, sr.w, sr.h) {
                            target.fill_rect(r, &paint, ts, clip_mask);
                        }
                    }
                }
            }

            PaintCmd::BeginStackingContext { .. } => {}
            PaintCmd::EndStackingContext => {}

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
                use tiny_skia::{
                    GradientStop as SkStop, LinearGradient, Point as SkPoint, RadialGradient,
                    SpreadMode,
                };
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

                let sk_stops: Vec<SkStop> = stops
                    .iter()
                    .map(|(color, pos)| {
                        let a = ((color.a as f32) * combined_opacity) as u8;
                        SkStop::new(
                            *pos,
                            tiny_skia::Color::from_rgba8(color.r, color.g, color.b, a),
                        )
                    })
                    .collect();

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
                    build_clip_mask(
                        clip, radii, radii_y, mask_w, mask_h, scale, scroll_x, scroll_y,
                    )
                } else {
                    None
                };
                let tile_mask_ref = tile_mask.as_ref().or(clip_mask);

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
            } => {
                let a2 = 1.0;
                let mut paint = Paint::default();
                paint.set_color(to_sk_color(&apply_opacity(color, a2)));
                paint.anti_alias = true;
                let mut stroke = tiny_skia::Stroke::default();
                stroke.width = *width;
                if let Some(path) = rounded_rect_path(rect.x, rect.y, rect.w, rect.h, 0.0) {
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
                let size = 12.0_f32.min(rect.w).min(rect.h).max(0.0);
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
                            let (text_scale, text_x, text_y) = transformed_text_origin(&ts, *x, *y);
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
                                    let sx = side / iw as f32;
                                    let sy = side / ih as f32;
                                    let img_ts = ts.pre_translate(*x, *y).pre_scale(sx, sy);
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
                placeholder_color,
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
                                );
                            }
                        }
                    }
                    ("select", _) => {
                        if !*appearance_none {
                            // Use Select widget for the arrow
                            let mut sel = crate::widgets::Select::new(vec![]);
                            sel.width = rect.w;
                            sel.height = rect.h;
                            sel.paint(target, rect.x, rect.y, scale);
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
                                    rect.x + 2.0,
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
                                );
                            }
                        }
                    }
                    // `<input type=file>` is a BUTTON plus the chosen file's
                    // name (HTML §4.10.5.1.18) — it fell to the generic arm and
                    // drew the value as bare text, which for an empty control
                    // is nothing at all.
                    ("input", "file") => {
                        let line_h = *font_size * 1.2;
                        let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                        // Measured from the label so the chrome cannot clip its
                        // own word in a large font.
                        let label_w =
                            crate::widgets::CHOOSE.chars().count() as f32 * *font_size * 0.55;
                        let button_w = crate::widgets::FileButton::width_for(label_w).min(rect.w);
                        if !*appearance_none {
                            let mut button = crate::widgets::FileButton::new(button_w, rect.h);
                            button.disabled = attributes.iter().any(|(k, _)| k == "disabled");
                            button.paint(target, rect.x, rect.y, scale);
                        }
                        if let Some((ref mut fs, ref mut sc)) = text_ctx {
                            let c = apply_opacity(color, a2);
                            draw_text_cmd(
                                target,
                                *fs,
                                *sc,
                                scale,
                                rect.x + 8.0,
                                text_y,
                                crate::widgets::CHOOSE,
                                font_family,
                                *font_size,
                                *font_weight,
                                0,
                                button_w,
                                line_h,
                                &c,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
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
                        let display_text = if value.is_empty() {
                            placeholder
                        } else if input_type == "password" {
                            masked = value.chars().map(|_| '\u{2022}').collect();
                            &masked
                        } else {
                            value
                        };
                        if !display_text.is_empty() {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let c = if value.is_empty() {
                                    apply_opacity(placeholder_color, a2)
                                } else {
                                    apply_opacity(color, a2)
                                };
                                // Vertically center the text in the element
                                let line_h = *font_size * 1.2;
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
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
                                    100.0,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
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
                    // ⛔ An `<input>` button is a VOID element — it has no
                    // children for that pipeline to find, and its label is the
                    // `value` ATTRIBUTE (HTML §4.10.5.1.19). Sharing the arm
                    // with `<button>` meant `<input type=submit value="Send">`
                    // drew an empty pill: correct chrome, no word on it.
                    //
                    // With no `value` the UA supplies the label, which is why a
                    // bare `<input type=submit>` reads "Submit" in every
                    // browser rather than being blank.
                    ("input", "submit") | ("input", "button") | ("input", "reset") => {
                        let label: &str = if !value.is_empty() {
                            value
                        } else {
                            match input_type.as_str() {
                                "submit" => "Submit",
                                "reset" => "Reset",
                                _ => "",
                            }
                        };
                        if !label.is_empty() {
                            if let Some((ref mut fs, ref mut sc)) = text_ctx {
                                let c = apply_opacity(color, a2);
                                let line_h = *font_size * 1.2;
                                // Centred both ways, as button chrome is.
                                let text_y = rect.y + (rect.h - line_h).max(0.0) / 2.0;
                                let text_w = label.chars().count() as f32 * *font_size * 0.5;
                                let text_x = rect.x + (rect.w - text_w).max(0.0) / 2.0;
                                draw_text_cmd(
                                    target,
                                    *fs,
                                    *sc,
                                    scale,
                                    text_x,
                                    text_y,
                                    label,
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
                                    100.0,
                                    line_h,
                                    &c,
                                    &super::display_list::TextDecoration::default(),
                                    0.0,
                                    0.0,
                                    false,
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
            } => {
                if let Some((ref mut fs, ref mut sc)) = text_ctx {
                    let c = apply_opacity(color, 1.0);
                    let (text_scale, text_x, text_y) = transformed_text_origin(&ts, *x, *y);
                    if *blur > 0.0 {
                        if let Some(mut layer) = Pixmap::new(pw, ph) {
                            draw_text_cmd(
                                &mut layer,
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
                                &c,
                                &super::display_list::TextDecoration::default(),
                                0.0,
                                0.0,
                                false,
                            );
                            crate::canvas::blur_pixmap(&mut layer, *blur);
                            let target = layer_stack
                                .last_mut()
                                .map(|l| &mut l.pixmap)
                                .unwrap_or(pixmap);
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
                        let target = layer_stack
                            .last_mut()
                            .map(|l| &mut l.pixmap)
                            .unwrap_or(pixmap);
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
                            *font_stretch,
                            *line_height,
                            &c,
                            &super::display_list::TextDecoration::default(),
                            0.0,
                            0.0,
                            false,
                        );
                    }
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
                let bg_clip =
                    build_clip_mask(clip, radii, radii_y, pw, ph, scale, scroll_x, scroll_y);
                let bg_clip_ref = bg_clip.as_ref().or(clip_mask);
                if let Some(img_pixmap) = tiny_skia::PixmapRef::from_bytes(rgba, iw, ih) {
                    let paint = tiny_skia::PixmapPaint::default();
                    let mut blend_layer = if *blend_mode != 0 {
                        Pixmap::new(pw, ph)
                    } else {
                        None
                    };
                    let target = blend_layer
                        .as_mut()
                        .or_else(|| layer_stack.last_mut().map(|l| &mut l.pixmap))
                        .unwrap_or(pixmap);
                    if *repeat_x_mode != 0 || *repeat_y_mode != 0 {
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

fn composite_masked_layer(
    target: &mut Pixmap,
    layer: &Pixmap,
    rect: Rect,
    mask: &ImageRef,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) {
    let (mask_rgba, mw, mh) = match mask {
        ImageRef::Owned(d, w, h) => (d.as_slice(), *w, *h),
        ImageRef::Shared(d, w, h) => (d.as_slice(), *w, *h),
    };
    if mw == 0 || mh == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let Some(mut masked) = Pixmap::new(layer.width(), layer.height()) else {
        return;
    };
    masked.data_mut().copy_from_slice(layer.data());
    let width = masked.width() as usize;
    let inv_scale = 1.0 / scale.max(0.001);
    for (i, px) in masked.data_mut().chunks_exact_mut(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let x = (i % width) as f32;
        let y = (i / width) as f32;
        let doc_x = x * inv_scale + scroll_x;
        let doc_y = y * inv_scale + scroll_y;
        let mask_alpha = if doc_x >= rect.x
            && doc_x < rect.x + rect.w
            && doc_y >= rect.y
            && doc_y < rect.y + rect.h
        {
            let u = (((doc_x - rect.x) / rect.w) * mw as f32)
                .floor()
                .clamp(0.0, (mw - 1) as f32) as usize;
            let v = (((doc_y - rect.y) / rect.h) * mh as f32)
                .floor()
                .clamp(0.0, (mh - 1) as f32) as usize;
            let base = (v * mw as usize + u) * 4;
            let mr = mask_rgba.get(base).copied().unwrap_or(0) as u32;
            let mg = mask_rgba.get(base + 1).copied().unwrap_or(0) as u32;
            let mb = mask_rgba.get(base + 2).copied().unwrap_or(0) as u32;
            let ma = mask_rgba.get(base + 3).copied().unwrap_or(0) as u32;
            let lum = (mr * 77 + mg * 150 + mb * 29) >> 8;
            (lum * ma / 255) as u8
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
        0,
        0,
        masked.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

fn draw_border_image_stretch(
    target: &mut Pixmap,
    rgba: &[u8],
    iw: u32,
    ih: u32,
    rect: &Rect,
    widths: &[f32; 4],
    slices: &[f32; 4],
    fill_center: bool,
    ts: Transform,
    clip_mask: Option<&tiny_skia::Mask>,
) {
    let sw = iw as f32;
    let sh = ih as f32;
    let st = slices[0].clamp(0.0, sh);
    let sr = slices[1].clamp(0.0, sw);
    let sb = slices[2].clamp(0.0, sh);
    let sl = slices[3].clamp(0.0, sw);
    let dt = widths[0].max(0.0).min(rect.h);
    let dr = widths[1].max(0.0).min(rect.w);
    let db = widths[2].max(0.0).min(rect.h);
    let dl = widths[3].max(0.0).min(rect.w);

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
            draw_rgba_patch(target, rgba, iw, ih, src, dst, ts, clip_mask);
        }
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

/// Blit an ALREADY-SHAPED cosmic-text buffer onto the pixmap at a physical
/// origin, source-over.
///
/// Extracted from `draw_text_cmd` so the canvas can share it. The two callers
/// need the same last step and nothing before it: `draw_text_cmd` shapes from a
/// display-list command, `<canvas>`'s `fillText` shapes from the 2D context's
/// own font state, and only then do both want these exact glyphs composited.
///
/// `color`'s alpha scales every glyph on top of whatever cosmic-text resolves
/// per glyph — a span carrying its own colour keeps it, and the parameter still
/// controls the overall opacity.
pub(crate) fn blit_shaped_buffer(
    pixmap: &mut Pixmap,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    buf: &mut Buffer,
    text: &str,
    phys_x: f32,
    phys_y: f32,
    _letter_spacing: f32,
    _word_spacing: f32,
    color: CTextColor,
) {
    struct PixmapTextRenderer<'a> {
        pixmap: &'a mut Pixmap,
        font_system: &'a mut FontSystem,
        swash_cache: &'a mut SwashCache,
        origin_x: i32,
        origin_y: i32,
        color_alpha: u32,
        tracking: f32,
        word_offsets: Vec<f32>,
        glyph_index: usize,
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
            );
        }

        fn glyph(&mut self, mut physical_glyph: cosmic_text::PhysicalGlyph, color: CTextColor) {
            let word_offset = self
                .word_offsets
                .get(self.glyph_index)
                .copied()
                .unwrap_or(0.0);
            physical_glyph.x +=
                (self.glyph_index as f32 * self.tracking + word_offset).round() as i32;
            self.glyph_index += 1;
            let origin_x = self.origin_x;
            let origin_y = self.origin_y;
            let color_alpha = self.color_alpha;
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
                    );
                },
            );
        }
    }

    let word_offsets: Vec<f32> = {
        let mut offset = 0.0;
        let mut out = Vec::new();
        for ch in text.chars() {
            if ch.is_whitespace() {
                offset += _word_spacing;
            } else {
                out.push(offset);
            }
        }
        out
    };
    let mut renderer = PixmapTextRenderer {
        pixmap,
        font_system,
        swash_cache,
        origin_x: phys_x as i32,
        origin_y: phys_y as i32,
        color_alpha: color.a() as u32,
        tracking: _letter_spacing,
        word_offsets,
        glyph_index: 0,
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
    let sa = eff_a;
    let ia = 255 - sa;
    let pr = gc.r() as u32 * sa / 255;
    let pg = gc.g() as u32 * sa / 255;
    let pb = gc.b() as u32 * sa / 255;
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
            let dst = &mut pixels[row + px_x as usize];
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
) {
    if text.is_empty() {
        return;
    }
    if word_spacing != 0.0 && text.chars().any(char::is_whitespace) {
        let mut cursor_x = x;
        let mut segment = String::new();
        let flush_segment = |segment: &mut String,
                             cursor_x: &mut f32,
                             pixmap: &mut Pixmap,
                             font_system: &mut FontSystem,
                             swash_cache: &mut SwashCache| {
            if segment.is_empty() {
                return;
            }
            let advance = crate::layout::inline_layout::measure_text_width_fs_attrs(
                font_system,
                segment,
                font_size,
                cosmic_text::Weight(font_weight),
                match font_style {
                    1 => CTextStyle::Italic,
                    2 => CTextStyle::Oblique,
                    _ => CTextStyle::Normal,
                },
                scale,
                font_family,
            ) + _letter_spacing * segment.chars().count() as f32;
            draw_text_cmd(
                pixmap,
                font_system,
                swash_cache,
                scale,
                *cursor_x,
                y,
                segment,
                font_family,
                font_size,
                font_weight,
                font_style,
                font_stretch,
                line_height,
                color,
                decoration,
                _letter_spacing,
                0.0,
                _small_caps,
            );
            *cursor_x += advance;
            segment.clear();
        };

        for ch in text.chars() {
            if ch.is_whitespace() {
                flush_segment(
                    &mut segment,
                    &mut cursor_x,
                    pixmap,
                    font_system,
                    swash_cache,
                );
                cursor_x += crate::layout::inline_layout::measure_text_width_fs_attrs(
                    font_system,
                    &ch.to_string(),
                    font_size,
                    cosmic_text::Weight(font_weight),
                    match font_style {
                        1 => CTextStyle::Italic,
                        2 => CTextStyle::Oblique,
                        _ => CTextStyle::Normal,
                    },
                    scale,
                    font_family,
                ) + _letter_spacing
                    + word_spacing;
            } else {
                segment.push(ch);
            }
        }
        flush_segment(
            &mut segment,
            &mut cursor_x,
            pixmap,
            font_system,
            swash_cache,
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
    if _letter_spacing != 0.0 && phys_px > 0.0 {
        attrs = attrs.letter_spacing((_letter_spacing * sc) / phys_px);
    }

    let phys_x = x * sc;
    let phys_y = y * sc;
    let ct_color = CTextColor::rgba(color.r, color.g, color.b, color.a);

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
        text.hash(&mut h);
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

    let line_w = SHAPED.with(|cell| {
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
            buf.set_text(font_system, text, &attrs, Shaping::Advanced, None);
            buf.shape_until_scroll(font_system, false);
            map.1.insert(key, buf);
        }
        let buf = map.1.get_mut(&key).expect("just inserted");
        blit_shaped_buffer(
            pixmap,
            font_system,
            swash_cache,
            buf,
            text,
            phys_x,
            phys_y,
            _letter_spacing * sc,
            word_spacing * sc,
            ct_color,
        );
        buf.layout_runs().next().map(|r| r.line_w).unwrap_or(0.0)
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
                    pixmap.fill_rect(r, &paint, Transform::identity(), None);
                }
            }
            1 => {
                // double
                if let Some(r) = SkRect::from_xywh(x, y, w, 1.0f32.max(thickness * 0.4)) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), None);
                }
                if let Some(r) =
                    SkRect::from_xywh(x, y + thickness * 1.5, w, 1.0f32.max(thickness * 0.4))
                {
                    pixmap.fill_rect(r, &paint, Transform::identity(), None);
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
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
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
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
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
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            _ => {
                // fallback to solid
                if let Some(r) = SkRect::from_xywh(x, y, w, thickness) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), None);
                }
            }
        }
    };

    if decoration.underline {
        // Position underline below the baseline by default. `under` is lower,
        // on the under side of the em box, matching the authored intent for
        // scripts where baseline underlines cut through glyphs.
        let baseline_y = phys_y + phys_px * 0.82;
        let offset = if decoration.underline_offset > 0.0 {
            decoration.underline_offset * sc
        } else {
            thickness * 2.0
        };
        let uy = if matches!(
            decoration.underline_position,
            crate::types::TextUnderlinePosition::Under
        ) {
            phys_y + phys_px + offset
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
        let oy = phys_y - thickness;
        draw_deco_line(pixmap, phys_x, line_w, oy, decoration.style);
    }
    if decoration.strikethrough {
        let sy = phys_y + phys_px * 0.4;
        draw_deco_line(pixmap, phys_x, line_w, sy, decoration.style);
    }
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

/// Composite a layer onto the destination pixmap with a blend mode.
fn blend_composite(dst: &mut Pixmap, src: &Pixmap, mode: u8) {
    let dst_pixels = dst.pixels_mut();
    let src_pixels = src.pixels();
    for (d, s) in dst_pixels.iter_mut().zip(src_pixels.iter()) {
        if s.alpha() == 0 {
            continue;
        }
        let (sr, sg, sb, sa) = (
            s.red() as u32,
            s.green() as u32,
            s.blue() as u32,
            s.alpha() as u32,
        );
        let (dr, dg, db, _da) = (
            d.red() as u32,
            d.green() as u32,
            d.blue() as u32,
            d.alpha() as u32,
        );

        // Unpremultiply for blending
        let (sr_n, sg_n, sb_n) = if sa > 0 {
            (sr * 255 / sa, sg * 255 / sa, sb * 255 / sa)
        } else {
            (0, 0, 0)
        };
        let (dr_n, dg_n, db_n) = (dr.min(255), dg.min(255), db.min(255));

        let (br, bg, bb) = match mode {
            1 => {
                // multiply
                (dr_n * sr_n / 255, dg_n * sg_n / 255, db_n * sb_n / 255)
            }
            2 => {
                // screen
                (
                    dr_n + sr_n - dr_n * sr_n / 255,
                    dg_n + sg_n - dg_n * sg_n / 255,
                    db_n + sb_n - db_n * sb_n / 255,
                )
            }
            3 => {
                // overlay
                let blend = |d: u32, s: u32| -> u32 {
                    if d < 128 {
                        2 * d * s / 255
                    } else {
                        255 - 2 * (255 - d) * (255 - s) / 255
                    }
                };
                (blend(dr_n, sr_n), blend(dg_n, sg_n), blend(db_n, sb_n))
            }
            4 => (dr_n.min(sr_n), dg_n.min(sg_n), db_n.min(sb_n)), // darken
            5 => (dr_n.max(sr_n), dg_n.max(sg_n), db_n.max(sb_n)), // lighten
            6 => {
                // color-dodge
                let dodge = |d: u32, s: u32| -> u32 {
                    if s >= 255 {
                        255
                    } else {
                        (d * 255 / (255 - s)).min(255)
                    }
                };
                (dodge(dr_n, sr_n), dodge(dg_n, sg_n), dodge(db_n, sb_n))
            }
            7 => {
                // color-burn
                let burn = |d: u32, s: u32| -> u32 {
                    if s == 0 {
                        0
                    } else {
                        255 - ((255 - d) * 255 / s).min(255)
                    }
                };
                (burn(dr_n, sr_n), burn(dg_n, sg_n), burn(db_n, sb_n))
            }
            8 => {
                // hard-light (like overlay but src/dst swapped)
                let blend = |d: u32, s: u32| -> u32 {
                    if s < 128 {
                        2 * d * s / 255
                    } else {
                        255 - 2 * (255 - d) * (255 - s) / 255
                    }
                };
                (blend(dr_n, sr_n), blend(dg_n, sg_n), blend(db_n, sb_n))
            }
            9 => {
                // soft-light
                let soft = |d: u32, s: u32| -> u32 {
                    let df = d as f32 / 255.0;
                    let sf = s as f32 / 255.0;
                    let r = if sf <= 0.5 {
                        df - (1.0 - 2.0 * sf) * df * (1.0 - df)
                    } else {
                        let g = if df <= 0.25 {
                            ((16.0 * df - 12.0) * df + 4.0) * df
                        } else {
                            df.sqrt()
                        };
                        df + (2.0 * sf - 1.0) * (g - df)
                    };
                    (r * 255.0).round().clamp(0.0, 255.0) as u32
                };
                (soft(dr_n, sr_n), soft(dg_n, sg_n), soft(db_n, sb_n))
            }
            10 => {
                // difference
                let diff = |a: u32, b: u32| -> u32 {
                    if a > b {
                        a - b
                    } else {
                        b - a
                    }
                };
                (diff(dr_n, sr_n), diff(dg_n, sg_n), diff(db_n, sb_n))
            }
            11 => {
                // exclusion
                let excl = |a: u32, b: u32| -> u32 { a + b - 2 * a * b / 255 };
                (excl(dr_n, sr_n), excl(dg_n, sg_n), excl(db_n, sb_n))
            }
            _ => (sr_n, sg_n, sb_n), // normal / hue / saturation / color / luminosity fallback
        };

        // Premultiply result and composite with source alpha
        let ia = 255 - sa;
        let fr = (br * sa / 255 + dr * ia / 255).min(255) as u8;
        let fg = (bg * sa / 255 + dg * ia / 255).min(255) as u8;
        let fb = (bb * sa / 255 + db * ia / 255).min(255) as u8;
        let fa = (sa + _da * ia / 255).min(255) as u8;
        if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(fr, fg, fb, fa) {
            *d = p;
        }
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
    let phys_x = ts.sx * x + ts.ky * y + ts.tx;
    let phys_y = ts.kx * x + ts.sy * y + ts.ty;
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
    let mut mask = tiny_skia::Mask::new(pw, ph)?;
    let ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
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

fn build_polygon_clip_mask(
    points: &[(f32, f32)],
    pw: u32,
    ph: u32,
    scale: f32,
    scroll_x: f32,
    scroll_y: f32,
) -> Option<tiny_skia::Mask> {
    if points.len() < 3 {
        return None;
    }
    let mut mask = tiny_skia::Mask::new(pw, ph)?;
    let ts = Transform::from_scale(scale, scale).pre_translate(-scroll_x, -scroll_y);
    let mut pb = PathBuilder::new();
    let (x0, y0) = points[0];
    pb.move_to(x0, y0);
    for &(x, y) in &points[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    if let Some(path) = pb.finish() {
        mask.fill_path(&path, FillRule::Winding, true, ts);
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
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl_x, y);
    pb.line_to(x + w - tr_x, y);
    pb.quad_to(x + w, y, x + w, y + tr_y);
    pb.line_to(x + w, y + h - br_y);
    pb.quad_to(x + w, y + h, x + w - br_x, y + h);
    pb.line_to(x + bl_x, y + h);
    pb.quad_to(x, y + h, x, y + h - bl_y);
    pb.line_to(x, y + tl_y);
    pb.quad_to(x, y, x + tl_x, y);
    pb.close();
    pb.finish()
}

pub(crate) fn reduce_corner_radii(w: f32, h: f32, radii: [f32; 4]) -> [f32; 4] {
    reduce_corner_radii_xy(w, h, radii, radii).0
}

fn reduce_corner_radii_xy(
    w: f32,
    h: f32,
    radii_x: [f32; 4],
    radii_y: [f32; 4],
) -> ([f32; 4], [f32; 4]) {
    if w <= 0.0 || h <= 0.0 {
        return ([0.0; 4], [0.0; 4]);
    }
    let mut scale = 1.0_f32;
    let pairs = [
        (radii_x[0] + radii_x[1], w),
        (radii_x[3] + radii_x[2], w),
        (radii_y[0] + radii_y[3], h),
        (radii_y[1] + radii_y[2], h),
    ];
    for (sum, side) in pairs {
        if sum > side && sum > 0.0 {
            scale = scale.min(side / sum);
        }
    }
    (
        radii_x.map(|r| r.max(0.0) * scale),
        radii_y.map(|r| r.max(0.0) * scale),
    )
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    rounded_rect_path_corners_xy(x, y, w, h, [r; 4], [r; 4])
}
