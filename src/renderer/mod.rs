pub mod compositor;
pub mod display_list;
pub mod display_list_builder;
pub mod display_list_replay;
mod svg_raster_cache;
pub mod tiles;

use crate::layout::inline_layout::collect_flat_text;
use crate::layout::inline_layout::{resolve_css_family, stretch_from_percent, weight_from_style};
use crate::types::*;
use cosmic_text::{
    Attrs, Buffer, Color as CTextColor, FontSystem, Metrics, Shaping, Style as CTextStyle,
    SwashCache,
};
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, Rect as SkRect, Stroke, Transform};
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::Key;

const NATIVE_CONTROL_CARET_WIDTH_PX: f32 = 1.5;

fn native_control_text_rect(node: &WebCore) -> Rect {
    let mut rect = node.layout.content_rect;
    if node.tag == "textarea" {
        rect.x += node.scrollbar_gutter_widths().0;
        rect.w = node.scrollport_content_width();
    }
    if node.tag == "input"
        && node.attributes.get("type").is_some_and(|kind| {
            crate::widgets::DateKind::for_input(&kind.trim().to_ascii_lowercase()).is_some()
        })
    {
        crate::widgets::DateField::text_rect(rect)
    } else {
        rect
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NativeControlPaintState {
    node_id: u32,
    selection: Option<(usize, usize)>,
    scroll_bits: (u32, u32),
}

pub struct Renderer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub component_registry: ComponentRegistry,
    layout_engine_inner: crate::layout::LayoutEngine,
    pub zoom: f32,
    scale: f32,
    shape_buf: Option<Buffer>,
    ctrl_held: bool,
    shift_held: bool,
    touches: std::collections::HashMap<u64, (f64, f64)>,
    pinch_dist: Option<f32>,
    touch_centroid: Option<(f32, f32)>,
    cursor_physical: (f32, f32),
    viewport_h: f32,
    cached_display_list: Option<display_list::DisplayList>,
    cached_scroll_x: f32,
    cached_scroll_y: f32,
    cached_paint_top: f32,
    cached_paint_bottom: f32,
    cached_hovered_id: u32,
    display_list_dirty: bool,
    cached_layout_generation: u64,
    cached_content_surface: Option<Pixmap>,
    cached_surface_animation_rects: Vec<Rect>,
    cached_surface_scale: f32,
    cached_surface_zoom: f32,
    cached_surface_scroll_x: f32,
    cached_surface_scroll_y: f32,
    cached_surface_layout_generation: u64,
    cached_surface_hovered_id: u32,
    cached_surface_active_id: u32,
    cached_surface_caret_visible: bool,
    cached_native_control: Option<NativeControlPaintState>,
    native_vertical_goal: Option<(u32, usize, u64, Option<f32>)>,
    paint_only_display_list_dirty: bool,
    pending_resource_relayout: bool,
    last_resource_relayout: Option<std::time::Instant>,
    last_animation_layout_values: std::collections::HashMap<u32, Vec<(String, String)>>,
    last_idle_scroll_x: f32,
    last_idle_scroll_y: f32,
    dirty_paint_rects: Vec<Rect>,
    pub content_offset_y: f32,
    /// Compositor layer tree — built after layout, used for scroll/transform/opacity.
    pub compositor: compositor::Compositor,
    /// Tile manager — caches rasterized tiles for fast scroll.
    pub tile_manager: tiles::TileManager,
    paint_segments: Option<compositor::PaintSegments>,
    /// Whether to use tiled rendering (can be disabled for debugging).
    pub use_tiles: bool,
    #[cfg(test)]
    disable_opaque_occlusion: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RendererMemoryStats {
    pub cached_content_surface_bytes: usize,
    pub cached_surface_bytes: usize,
    pub tile_surface_bytes: usize,
    pub tile_count: usize,
    pub display_list_commands: usize,
    pub display_list_estimated_bytes: usize,
    pub display_list_inline_bytes: usize,
    pub display_list_heap_bytes: usize,
    pub display_list_text_bytes: usize,
    pub display_list_image_bytes: usize,
    pub display_list_vector_bytes: usize,
}

fn copy_surface_shifted_y(src: &[u8], dst: &mut [u8], width: u32, height: u32, dy_px: i32) {
    let row_bytes = width as usize * 4;
    let height = height as i32;
    if row_bytes == 0 || height <= 0 {
        return;
    }
    if dy_px > 0 {
        let dy = dy_px.min(height) as usize;
        let rows = height as usize - dy;
        for row in 0..rows {
            let src_off = (row + dy) * row_bytes;
            let dst_off = row * row_bytes;
            if src_off + row_bytes <= src.len() && dst_off + row_bytes <= dst.len() {
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }
        }
    } else if dy_px < 0 {
        let dy = (-dy_px).min(height) as usize;
        let rows = height as usize - dy;
        for row in (0..rows).rev() {
            let src_off = row * row_bytes;
            let dst_off = (row + dy) * row_bytes;
            if src_off + row_bytes <= src.len() && dst_off + row_bytes <= dst.len() {
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }
        }
    }
}

fn fill_physical_rect(
    pixmap: &mut Pixmap,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    color: tiny_skia::Color,
) {
    if w == 0 || h == 0 {
        return;
    }
    if color.is_opaque() {
        let x0 = x.min(pixmap.width()) as usize;
        let y0 = y.min(pixmap.height()) as usize;
        let x1 = x.saturating_add(w).min(pixmap.width()) as usize;
        let y1 = y.saturating_add(h).min(pixmap.height()) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = pixmap.width() as usize;
        let pixel = color.premultiply().to_color_u8();
        let pixels = pixmap.pixels_mut();
        for row in y0..y1 {
            pixels[row * stride + x0..row * stride + x1].fill(pixel);
        }
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(color);
    if let Some(rect) = SkRect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

#[test]
fn opaque_physical_fill_matches_tiny_skia() {
    let color = tiny_skia::Color::from_rgba8(23, 87, 151, 255);
    for (x, y, w, h) in [(2, 3, 7, 6), (0, 0, 16, 12), (13, 9, 9, 9)] {
        let mut actual = Pixmap::new(16, 12).unwrap();
        let mut expected = Pixmap::new(16, 12).unwrap();
        actual.fill(tiny_skia::Color::from_rgba8(17, 19, 29, 255));
        expected.fill(tiny_skia::Color::from_rgba8(17, 19, 29, 255));
        fill_physical_rect(&mut actual, x, y, w, h, color);
        let mut paint = Paint::default();
        paint.set_color(color);
        expected.fill_rect(
            SkRect::from_xywh(x as f32, y as f32, w as f32, h as f32).unwrap(),
            &paint,
            Transform::identity(),
            None,
        );
        assert_eq!(actual.data(), expected.data());
    }
}

#[test]
fn offscreen_image_publication_invalidates_retained_preview() {
    let mut renderer = Renderer::new();
    renderer.use_tiles = true;
    let mut doc = renderer.load_html_vp(
        "<style>body{margin:0}</style><div style='height:2000px'></div><img id='image' width='80' height='40'><div style='height:100px'></div>",
        160.0, 80.0,
    );
    let id = doc.get_element_by_id("image").unwrap();
    let install = |doc: &mut Document, red: u8| {
        crate::html::set_image_on_node(
            doc.find_webcore_mut(id).unwrap(),
            vec![red, 0, 0, 255],
            1,
            1,
        );
    };
    install(&mut doc, 40);
    let rect = doc.find_webcore(id).unwrap().layout.border_rect;
    let mut surface = Pixmap::new(160, 80).unwrap();
    doc.scroll_y = rect.y;
    renderer.render(&mut doc, &mut surface, 1.0);
    assert_eq!(surface.pixel(10, 10).unwrap().red(), 40);
    doc.scroll_y = 0.0;
    renderer.render(&mut doc, &mut surface, 1.0);
    install(&mut doc, 200);
    assert!(
        !renderer.invalidate_resource_paint_rects(&[rect]),
        "offscreen resources must not repaint the current viewport"
    );
    doc.scroll_y = rect.y;
    renderer.render(&mut doc, &mut surface, 1.0);
    assert_eq!(
        surface.pixel(10, 10).unwrap().red(),
        200,
        "scrolling back must use the final buffer, not the retained preview"
    );
}

#[test]
fn software_video_updates_retain_static_overlays_and_match_fresh_paint() {
    let html = r#"<style>
        html, body { width:100%; height:100%; overflow:hidden; }
        body { margin:0; background:#234; }
        video { position:fixed; inset:0; width:100%; height:100%; object-fit:cover; }
        #overlay { position:fixed; left:20px; top:30px; color:white;
            font:20px sans-serif; text-shadow:0 2px 12px black; }
        </style><video id="video" controls></video><div id="overlay">Video overlay</div>"#;
    for (opaque_background, scale) in [(false, 1.0), (false, 2.0), (true, 1.0), (true, 2.0)] {
        let html = if opaque_background {
            html.replace("video {", "video { background:black;")
        } else {
            html.to_owned()
        };
        let mut renderer = Renderer::new();
        renderer.use_tiles = true;
        let mut doc = renderer.load_html_vp(&html, 320.0, 180.0);
        let id = doc.get_element_by_id("video").unwrap();
        let mut actual = Pixmap::new((320.0 * scale) as u32, (180.0 * scale) as u32).unwrap();
        let mut retained_overlay = None;
        for frame in 0..3 {
            let node = doc.find_webcore_mut(id).unwrap();
            node.image_data = Some(std::sync::Arc::new(
                [40 + frame * 50, 100, 180, 255].repeat(64),
            ));
            node.image_width = 8;
            node.image_height = 8;
            node.media_paused = false;
            node.media_duration = Some(10.0);
            node.media_current_time = frame as f32;
            let rect = node.layout.content_rect;
            if frame != 0 {
                renderer.invalidate_paint_only_display_list();
                renderer.invalidate_paint_rects([rect]);
            }
            renderer.render(&mut doc, &mut actual, scale);
            if opaque_background {
                assert!(
                    renderer
                        .paint_segments
                        .as_ref()
                        .unwrap()
                        .segments
                        .iter()
                        .filter(|segment| !segment.fixed)
                        .all(|segment| segment.tiles.tiles.is_empty()),
                    "fully hidden background was rasterized"
                );
            }
            if frame != 0 {
                let video = renderer
                    .paint_segments
                    .as_ref()
                    .unwrap()
                    .segments
                    .iter()
                    .find(|segment| {
                        segment
                            .list
                            .commands
                            .iter()
                            .any(|cmd| matches!(cmd, display_list::PaintCmd::Image { .. }))
                    })
                    .unwrap();
                assert_eq!(
                    video.fixed_surface.is_none(),
                    opaque_background,
                    "opaque_background={opaque_background}, scale={scale}"
                );
            }
            let overlay = renderer
                .paint_segments
                .as_ref()
                .unwrap()
                .segments
                .iter()
                .find(|segment| {
                    segment.list.commands.iter().any(|cmd| {
                        matches!(cmd,
                    display_list::PaintCmd::TextShadow { text, .. } if text == "Video overlay")
                    })
                })
                .unwrap()
                .fixed_surface
                .as_ref()
                .unwrap();
            if let Some(previous) = retained_overlay {
                assert_eq!(
                    overlay.image.data().as_ptr(),
                    previous,
                    "static overlay was rerasterized"
                );
            }
            retained_overlay = Some(overlay.image.data().as_ptr());
            let mut reference = Renderer::new();
            reference.use_tiles = true;
            reference.disable_opaque_occlusion = true;
            let mut expected = Pixmap::new(actual.width(), actual.height()).unwrap();
            reference.render(&mut doc, &mut expected, scale);
            if opaque_background {
                assert!(
                    reference
                        .paint_segments
                        .as_ref()
                        .unwrap()
                        .segments
                        .iter()
                        .filter(|segment| !segment.fixed)
                        .any(|segment| !segment.tiles.tiles.is_empty()),
                    "reference did not paint the hidden background"
                );
            }
            let difference = actual
                .data()
                .iter()
                .zip(expected.data())
                .position(|(a, b)| a != b);
            assert_eq!(
                difference, None,
                "frame={frame}, scale={scale}, opaque={opaque_background}"
            );
        }
    }
}

#[test]
#[ignore = "run explicitly when measuring fullscreen software video composition"]
fn benchmark_fullscreen_video_occlusion() {
    let html = "<style>html,body{width:100%;height:100%;overflow:hidden}body{margin:0;background:#234}
        video{position:fixed;inset:0;width:100%;height:100%;object-fit:cover;background:black}
        #overlay{position:fixed;left:20px;top:30px;color:white;font:20px sans-serif;text-shadow:0 2px 12px black}
        </style><video id='video' controls></video><div id='overlay'>Video overlay</div>";
    let frames = [
        std::sync::Arc::new([40, 100, 180, 255].repeat(1920 * 1080)),
        std::sync::Arc::new([90, 100, 180, 255].repeat(1920 * 1080)),
    ];
    for trial in 0..4 {
        for disabled in if trial % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let mut renderer = Renderer::new();
            renderer.use_tiles = true;
            renderer.disable_opaque_occlusion = disabled;
            let mut doc = renderer.load_html_vp(html, 1920.0, 1080.0);
            let id = doc.get_element_by_id("video").unwrap();
            let mut pixels = Pixmap::new(3840, 2160).unwrap();
            let mut total = std::time::Duration::ZERO;
            for frame in 0..16 {
                let node = doc.find_webcore_mut(id).unwrap();
                node.image_data = Some(frames[frame % 2].clone());
                node.image_width = 1920;
                node.image_height = 1080;
                node.media_paused = false;
                node.media_duration = Some(20.0);
                node.media_current_time = frame as f32;
                let rect = node.layout.content_rect;
                renderer.invalidate_paint_only_display_list();
                renderer.invalidate_paint_rects([rect]);
                let start = std::time::Instant::now();
                renderer.render(&mut doc, &mut pixels, 2.0);
                if frame >= 4 {
                    total += start.elapsed();
                }
                std::hint::black_box(pixels.data());
            }
            eprintln!(
                "fullscreen CPU composition trial={trial} occlusion={} x12: {total:?}",
                !disabled
            );
        }
    }
}

fn fill_viewport_clip(pixmap: &mut Pixmap, clip: Rect, scale: f32, color: tiny_skia::Color) {
    let x = (clip.x * scale).floor().max(0.0) as u32;
    let y = (clip.y * scale).floor().max(0.0) as u32;
    let right = (clip.right() * scale)
        .ceil()
        .min(pixmap.width() as f32)
        .max(x as f32) as u32;
    let bottom = (clip.bottom() * scale)
        .ceil()
        .min(pixmap.height() as f32)
        .max(y as f32) as u32;
    fill_physical_rect(
        pixmap,
        x,
        y,
        right.saturating_sub(x),
        bottom.saturating_sub(y),
        color,
    );
}

fn viewport_clip_from_doc_rect(
    rect: Rect,
    scroll_x: f32,
    scroll_y: f32,
    view_w: f32,
    view_h: f32,
) -> Option<Rect> {
    let x0 = (rect.x - scroll_x).max(0.0);
    let y0 = (rect.y - scroll_y).max(0.0);
    let x1 = (rect.right() - scroll_x).min(view_w);
    let y1 = (rect.bottom() - scroll_y).min(view_h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

fn inflate_rect(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        rect.x - amount,
        rect.y - amount,
        rect.w + amount * 2.0,
        rect.h + amount * 2.0,
    )
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = a.right().max(b.right());
    let bottom = a.bottom().max(b.bottom());
    Rect::new(left, top, right - left, bottom - top)
}

fn rect_intersects(a: Rect, b: Rect) -> bool {
    a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
}

pub fn retained_paint_band_for_doc(doc: &Document, viewport_w: f32, viewport_h: f32) -> Rect {
    let doc_h = doc.cached_scroll_height().max(viewport_h);
    const OVERSCAN_VIEWPORTS: f32 = 1.5;
    const MIN_OVERSCAN_CSS_PX: f32 = 1200.0;
    let overscan = (viewport_h * OVERSCAN_VIEWPORTS).max(MIN_OVERSCAN_CSS_PX);
    let top = (doc.scroll_y - overscan).max(0.0);
    let bottom = (doc.scroll_y + viewport_h + overscan).min(doc_h);
    Rect::new(
        doc.scroll_x,
        top,
        doc.root.layout.margin_rect.w.max(viewport_w).max(1.0),
        (bottom - top).max(0.0),
    )
}

#[cfg(test)]
mod retained_paint_band_tests {
    use super::*;

    #[test]
    fn far_offscreen_media_enters_band_only_when_scrolled_near() {
        let mut renderer = Renderer::new();
        let mut doc = renderer.load_html_vp("<div style='height:10000px'></div>", 1280.0, 820.0);
        let initial = retained_paint_band_for_doc(&doc, 1280.0, 820.0);
        assert!(initial.bottom() < 3200.0);
        doc.scroll_y = 3000.0;
        let scrolled = retained_paint_band_for_doc(&doc, 1280.0, 820.0);
        assert!(scrolled.y <= 3200.0 && scrolled.bottom() > 3200.0);
    }
}

fn rect_area(rect: Rect) -> f32 {
    rect.w.max(0.0) * rect.h.max(0.0)
}

fn coalesce_dirty_rects(mut rects: Vec<Rect>, viewport: Rect) -> Vec<Rect> {
    const MAX_RECTS: usize = 16;
    const MERGE_PAD: f32 = 24.0;
    if rects.is_empty() {
        return rects;
    }
    let mut merged: Vec<Rect> = Vec::new();
    for mut rect in rects.drain(..) {
        let x0 = rect.x.max(viewport.x);
        let y0 = rect.y.max(viewport.y);
        let x1 = rect.right().min(viewport.right());
        let y1 = rect.bottom().min(viewport.bottom());
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        rect = Rect::new(x0, y0, x1 - x0, y1 - y0);
        let padded = inflate_rect(rect, MERGE_PAD);
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| rect_intersects(inflate_rect(**existing, MERGE_PAD), padded))
        {
            *existing = rect_union(*existing, rect);
        } else {
            merged.push(rect);
        }
    }
    if merged.len() > MAX_RECTS {
        let total_area: f32 = merged.iter().copied().map(rect_area).sum();
        let union = merged.into_iter().reduce(rect_union).unwrap_or(viewport);
        if rect_area(union) <= total_area * 2.5 {
            return vec![union];
        }
        return vec![viewport];
    }
    merged
}

pub(crate) fn animation_override_rects_with_ids(
    root: &WebCore,
    overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
    viewport_w: f32,
    viewport_h: f32,
) -> Vec<(u32, Rect)> {
    fn walk(
        node: &WebCore,
        overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
        out: &mut Vec<(u32, Rect)>,
        ctx: &crate::types::TransformCtx,
    ) {
        if let Some(props) = overrides.get(&node.node_id) {
            let mut rect = node.layout.border_rect;
            if rect.w <= 0.0 || rect.h <= 0.0 {
                rect = node.layout.margin_rect;
            }
            if rect.w <= 0.0 || rect.h <= 0.0 {
                for child in &node.children {
                    walk(child, overrides, out, ctx);
                }
                return;
            }
            if let Some((_, value)) = props.iter().rev().find(|(prop, _)| prop == "transform") {
                let mut style = node.style.as_ref().clone();
                crate::css::apply_property(&mut style, "transform", value);
                let local_ctx = crate::types::TransformCtx {
                    font_px: style.font_size_px(ctx.root_font_px, ctx.root_font_px),
                    ..*ctx
                };
                let [a, b, c, d, e, f] = display_list_builder::compute_transform_matrix(
                    &style,
                    &node.layout.border_rect,
                    &local_ctx,
                );
                // A translation can travel arbitrarily far beyond the original
                // box. Include all transformed corners, not a fixed travel pad.
                let corners = [
                    (rect.x, rect.y),
                    (rect.x + rect.w, rect.y),
                    (rect.x, rect.y + rect.h),
                    (rect.x + rect.w, rect.y + rect.h),
                ];
                let mut left = rect.x;
                let mut top = rect.y;
                let mut right = rect.x + rect.w;
                let mut bottom = rect.y + rect.h;
                for (x, y) in corners {
                    let tx = a * x + c * y + e;
                    let ty = b * x + d * y + f;
                    left = left.min(tx);
                    top = top.min(ty);
                    right = right.max(tx);
                    bottom = bottom.max(ty);
                }
                let transformed = Rect::new(left, top, right - left, bottom - top);
                let long_inline_strip = rect.w > rect.h.max(1.0) * 8.0;
                let pad = if long_inline_strip {
                    rect.h.max(32.0).min(192.0)
                } else {
                    rect.w.max(rect.h).max(32.0).min(384.0)
                };
                rect = inflate_rect(rect, pad);
                let left = rect.x.min(transformed.x);
                let top = rect.y.min(transformed.y);
                rect = Rect::new(
                    left,
                    top,
                    (rect.x + rect.w).max(transformed.x + transformed.w) - left,
                    (rect.y + rect.h).max(transformed.y + transformed.h) - top,
                );
            }
            out.push((node.node_id, rect));
        }
        for child in &node.children {
            walk(child, overrides, out, ctx);
        }
    }
    let mut out = Vec::new();
    let initial = ComputedStyle::INITIAL_FONT_SIZE_PX;
    let root_font_px = root.style.font_size_px(initial, initial);
    walk(
        root,
        overrides,
        &mut out,
        &crate::types::TransformCtx {
            font_px: root_font_px,
            root_font_px,
            viewport_w,
            viewport_h,
        },
    );
    out
}

fn animation_overrides_are_transform_only_for_ids(
    overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
    ids: &std::collections::HashSet<u32>,
) -> bool {
    !ids.is_empty()
        && ids.iter().all(|id| {
            overrides.get(id).is_some_and(|props| {
                !props.is_empty()
                    && props.iter().all(|(prop, _)| {
                        crate::types::animation_runtime::animation_property_is_transform(prop)
                    })
            })
        })
}

fn display_list_has_transform_slots_for_ids(
    list: &display_list::DisplayList,
    ids: &std::collections::HashSet<u32>,
) -> bool {
    !ids.is_empty()
        && ids.iter().all(|node_id| {
            list.commands.iter().chain(&list.fixed_commands).any(|cmd| {
                matches!(cmd, display_list::PaintCmd::PushTransform { node_id: id, .. } if id == node_id)
            })
        })
}

fn animation_transform_matrices(
    root: &WebCore,
    overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
    viewport_w: f32,
    viewport_h: f32,
) -> std::collections::HashMap<u32, [f32; 6]> {
    if overrides.is_empty() {
        return std::collections::HashMap::new();
    }
    fn walk(
        node: &WebCore,
        overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
        out: &mut std::collections::HashMap<u32, [f32; 6]>,
        root_font_px: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        if let Some(props) = overrides.get(&node.node_id)
            && let Some((_, transform)) = props.iter().rev().find(|(prop, _)| prop == "transform")
        {
            let mut style = node.style.as_ref().clone();
            crate::css::apply_property(&mut style, "transform", transform);
            let font_px = style.font_size_px(root_font_px, root_font_px);
            let matrix = display_list_builder::compute_transform_matrix(
                &style,
                &node.layout.border_rect,
                &crate::types::TransformCtx {
                    font_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                },
            );
            out.insert(node.node_id, matrix);
        }
        for child in &node.children {
            walk(child, overrides, out, root_font_px, viewport_w, viewport_h);
        }
    }

    let initial_font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
    let root_font_px = root.style.font_size_px(initial_font_px, initial_font_px);
    let mut out = std::collections::HashMap::new();
    walk(
        root,
        overrides,
        &mut out,
        root_font_px,
        viewport_w,
        viewport_h,
    );
    out
}

impl Renderer {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        configure_generic_font_families(&mut font_system);
        Self {
            font_system,
            swash_cache: SwashCache::new(),
            component_registry: ComponentRegistry::default(),
            layout_engine_inner: crate::layout::LayoutEngine::new(),
            zoom: 1.0,
            scale: 1.0,
            shape_buf: None,
            ctrl_held: false,
            shift_held: false,
            touches: std::collections::HashMap::new(),
            pinch_dist: None,
            touch_centroid: None,
            cursor_physical: (0.0, 0.0),
            viewport_h: 700.0,
            cached_display_list: None,
            cached_scroll_x: 0.0,
            cached_scroll_y: 0.0,
            cached_paint_top: 0.0,
            cached_paint_bottom: 0.0,
            cached_hovered_id: 0,
            display_list_dirty: true,
            cached_layout_generation: 0,
            cached_content_surface: None,
            cached_surface_animation_rects: Vec::new(),
            cached_surface_scale: 0.0,
            cached_surface_zoom: 0.0,
            cached_surface_scroll_x: f32::NAN,
            cached_surface_scroll_y: f32::NAN,
            cached_surface_layout_generation: 0,
            cached_surface_hovered_id: 0,
            cached_surface_active_id: 0,
            cached_surface_caret_visible: false,
            cached_native_control: None,
            native_vertical_goal: None,
            paint_only_display_list_dirty: false,
            pending_resource_relayout: false,
            last_resource_relayout: None,
            last_animation_layout_values: std::collections::HashMap::new(),
            last_idle_scroll_x: f32::NAN,
            last_idle_scroll_y: f32::NAN,
            dirty_paint_rects: Vec::new(),
            content_offset_y: 0.0,
            compositor: compositor::Compositor::new(),
            tile_manager: tiles::TileManager::new(),
            paint_segments: None,
            use_tiles: std::env::var_os("WEBCORE_DISABLE_TILES").is_none(),
            #[cfg(test)]
            disable_opaque_occlusion: false,
        }
    }

    pub fn invalidate_display_list(&mut self) {
        self.display_list_dirty = true;
        // Keep the old segments until the rebuilt list can identify which
        // raster layers are unchanged. Changed layers are discarded there.
        self.paint_only_display_list_dirty = false;
        self.cached_paint_top = 0.0;
        self.cached_paint_bottom = 0.0;
        self.cached_content_surface = None;
        self.cached_surface_animation_rects.clear();
        self.dirty_paint_rects.clear();
    }

    pub(crate) fn invalidate_paint_only_display_list(&mut self) {
        self.display_list_dirty = true;
        self.paint_only_display_list_dirty = true;
        // render() invalidates tiles intersecting the accumulated damage.
        // Clearing every tile here discarded unchanged scroll content on each
        // image or animation frame, even for an offscreen animation.
    }

    #[cfg(test)]
    pub(crate) fn dirty_paint_rect_count_for_test(&self) -> usize {
        self.dirty_paint_rects.len()
    }

    #[cfg(test)]
    pub(crate) fn paint_only_display_list_dirty_for_test(&self) -> bool {
        self.paint_only_display_list_dirty
    }

    #[cfg(test)]
    pub(crate) fn display_list_dirty_for_test(&self) -> bool {
        self.display_list_dirty
    }

    pub fn invalidate_paint_rects<I>(&mut self, rects: I) -> bool
    where
        I: IntoIterator<Item = Rect>,
    {
        for rect in rects {
            if rect.w > 0.0 && rect.h > 0.0 {
                self.dirty_paint_rects.push(inflate_rect(rect, 8.0));
            }
        }
        if self.dirty_paint_rects.is_empty() {
            return false;
        }
        let Some(surface) = self.cached_content_surface.as_ref() else {
            return true;
        };
        let viewport = Rect::new(
            self.cached_scroll_x,
            self.cached_scroll_y,
            surface.width() as f32 / self.cached_surface_scale.max(0.001),
            surface.height() as f32 / self.cached_surface_scale.max(0.001),
        );
        self.dirty_paint_rects =
            coalesce_dirty_rects(std::mem::take(&mut self.dirty_paint_rects), viewport);
        !self.dirty_paint_rects.is_empty()
    }

    pub(crate) fn invalidate_resource_paint_rects(&mut self, rects: &[Rect]) -> bool {
        if rects.is_empty() {
            return false;
        }
        // Resource publication changes captured pixel buffers even outside the
        // viewport. Retained tiles must not survive until a later scroll with
        // their old preview, but offscreen damage need not paint immediately.
        for rect in rects {
            let rect = inflate_rect(*rect, 8.0);
            self.tile_manager.invalidate_rect(&rect);
            if let Some(segments) = &mut self.paint_segments {
                segments.invalidate_rect(&rect);
            }
        }
        self.invalidate_paint_only_display_list();
        self.invalidate_paint_rects(rects.iter().copied())
    }

    pub(crate) fn invalidate_animation_paint_rects(
        &mut self,
        doc: &Document,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        if doc.animation_overrides.is_empty() {
            return false;
        }
        let viewport = Rect::new(doc.scroll_x, doc.scroll_y, viewport_w, viewport_h);
        let rects = animation_override_rects_with_ids(
            &doc.root,
            &doc.animation_overrides,
            viewport_w,
            viewport_h,
        )
        .into_iter()
        .filter_map(|(_, rect)| rect_intersects(rect, viewport).then_some(rect))
        .collect::<Vec<_>>();
        if rects.is_empty() {
            return false;
        }
        self.invalidate_paint_rects(rects)
    }

    pub(crate) fn invalidate_non_transform_animation_paint_rects(
        &mut self,
        doc: &Document,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        if doc.animation_overrides.is_empty() {
            return false;
        }
        let viewport = Rect::new(doc.scroll_x, doc.scroll_y, viewport_w, viewport_h);
        let rects = animation_override_rects_with_ids(
            &doc.root,
            &doc.animation_overrides,
            viewport_w,
            viewport_h,
        )
        .into_iter()
        .filter_map(|(node_id, rect)| {
            let props = doc.animation_overrides.get(&node_id)?;
            let transform_only = !props.is_empty()
                && props.iter().all(|(prop, _)| {
                    crate::types::animation_runtime::animation_property_is_transform(prop)
                });
            (!transform_only && rect_intersects(rect, viewport)).then_some(rect)
        })
        .collect::<Vec<_>>();
        if rects.is_empty() {
            return false;
        }
        self.invalidate_paint_rects(rects)
    }

    /// Run browser-owned idle work for a document and configure the next event
    /// loop wakeup. Browser shells should call this from `about_to_wait` instead
    /// of scheduling individual engine features themselves.
    pub fn drive_document_idle(
        &mut self,
        event_loop: &ActiveEventLoop,
        doc: Option<&mut Document>,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        let Some(doc) = doc else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return false;
        };

        let trace_idle = std::env::var_os("WEBCORE_TRACE_IDLE").is_some();
        let mut trace_reasons: Vec<&'static str> = Vec::new();
        let mut needs_redraw = false;
        let mut needs_relayout = false;
        let now = std::time::Instant::now();
        let mut resource_requested_relayout = false;
        let scroll_changed = (doc.scroll_x - self.last_idle_scroll_x).abs() >= 0.5
            || (doc.scroll_y - self.last_idle_scroll_y).abs() >= 0.5;
        let editor_overlay_active =
            doc.editor.has_focus && (doc.editor.caret_box.is_some() || doc.editor.has_selection());

        if editor_overlay_active && doc.editor.blink_update() {
            needs_redraw = true;
            if trace_idle {
                trace_reasons.push("editor-blink");
            }
        }
        if scroll_changed {
            needs_redraw = true;
            if trace_idle {
                trace_reasons.push("scroll-priority");
            }
        } else {
            if doc.poll_pending_stylesheets_budgeted(32, std::time::Duration::from_millis(6)) {
                self.layout_engine().invalidate_cascade();
                resource_requested_relayout = true;
                if trace_idle {
                    trace_reasons.push("stylesheet");
                }
            }
            let image_poll =
                doc.poll_pending_images_budgeted(32, std::time::Duration::from_millis(8));
            if image_poll.loaded_any {
                resource_requested_relayout |= image_poll.needs_relayout;
                let mut visible_image_damage = false;
                if image_poll.needs_relayout {
                    // Intrinsic size changes are folded into the coalesced
                    // resource relayout below. Do not throw away the display
                    // list here for every arriving image.
                } else {
                    let paint_band = retained_paint_band_for_doc(doc, viewport_w, viewport_h);
                    let visible_rects = image_poll
                        .paint_rects
                        .iter()
                        .copied()
                        .filter(|rect| rect_intersects(*rect, paint_band))
                        .collect::<Vec<_>>();
                    visible_image_damage = self.invalidate_paint_rects(visible_rects);
                }
                needs_redraw |= visible_image_damage;
                if trace_idle {
                    trace_reasons.push(if image_poll.needs_relayout {
                        "image-layout"
                    } else {
                        "image-paint"
                    });
                }
            }
            if self
                .layout_engine()
                .poll_pending_fonts_budgeted(8, std::time::Duration::from_millis(8))
            {
                self.layout_engine().invalidate_cascade();
                doc.style_dirty = true;
                resource_requested_relayout = true;
                if trace_idle {
                    trace_reasons.push("font");
                }
            }
        }
        if resource_requested_relayout {
            self.pending_resource_relayout = true;
        }
        let pending_resources = doc.pending_images.is_some()
            || doc.pending_stylesheets.is_some()
            || self.layout_engine().has_pending_fonts();
        if self.pending_resource_relayout {
            let elapsed = self
                .last_resource_relayout
                .map(|last| now.saturating_duration_since(last))
                .unwrap_or(std::time::Duration::from_millis(100));
            if !scroll_changed
                && (!pending_resources || elapsed >= std::time::Duration::from_millis(32))
            {
                needs_relayout = true;
                self.pending_resource_relayout = false;
                self.last_resource_relayout = Some(now);
                if trace_idle {
                    trace_reasons.push("resource-layout");
                }
            }
        }
        if doc.needs_animation_frame && !needs_relayout && !scroll_changed {
            let previous_rects = animation_override_rects_with_ids(
                &doc.root,
                &doc.animation_overrides,
                viewport_w,
                viewport_h,
            );
            let overrides_changed = doc.tick_animations(now);
            let finished_rects = previous_rects.into_iter().filter_map(|(id, rect)| {
                (!doc.animation_overrides.contains_key(&id)).then_some(rect)
            });
            if self.invalidate_paint_rects(finished_rects) {
                self.invalidate_paint_only_display_list();
                needs_redraw = true;
            }
            let css_animations_running = doc.needs_animation_frame;
            let (svg_animations_running, svg_damage) =
                crate::svg::animation::tick_svg_animations_with_damage(&mut doc.root, now);
            if !svg_damage.is_empty() && self.invalidate_paint_rects(svg_damage) {
                self.invalidate_paint_only_display_list();
                needs_redraw = true;
            }
            let media_running = doc.tick_media(now);
            if svg_animations_running {
                doc.needs_animation_frame = true;
            }
            if media_running {
                needs_redraw = true;
            }
            let layout_values =
                crate::types::animation_runtime::layout_animation_values(&doc.animation_overrides);
            let animation_needs_layout = layout_values != self.last_animation_layout_values;
            self.last_animation_layout_values = layout_values;
            if animation_needs_layout {
                self.invalidate_display_list();
                needs_redraw = true;
                if trace_idle {
                    trace_reasons.push("css-animation-layout");
                }
            } else if overrides_changed && !doc.animation_overrides.is_empty() {
                // Paint-only animations do not affect geometry, but they still
                // must present a new frame. Skeleton loaders commonly animate
                // `background-position` over a gradient. Rebuild the viewport
                // display list with the new sampled style, but keep the old
                // surface and repaint only the animated boxes.
                let viewport = Rect::new(doc.scroll_x, doc.scroll_y, viewport_w, viewport_h);
                let visible_animation_rects = animation_override_rects_with_ids(
                    &doc.root,
                    &doc.animation_overrides,
                    viewport_w,
                    viewport_h,
                )
                .into_iter()
                .filter(|(_, rect)| rect_intersects(*rect, viewport))
                .collect::<Vec<_>>();
                let visible_animation_ids = visible_animation_rects
                    .iter()
                    .map(|(id, _)| *id)
                    .collect::<std::collections::HashSet<_>>();
                let transform_only = animation_overrides_are_transform_only_for_ids(
                    &doc.animation_overrides,
                    &visible_animation_ids,
                );
                let paint_rects = visible_animation_rects
                    .into_iter()
                    .map(|(_, rect)| rect)
                    .collect::<Vec<_>>();
                if !paint_rects.is_empty() {
                    let can_replay_transform = transform_only
                        && self.cached_display_list.as_ref().is_some_and(|list| {
                            display_list_has_transform_slots_for_ids(list, &visible_animation_ids)
                        });
                    if can_replay_transform {
                        needs_redraw |= self.invalidate_paint_rects(paint_rects);
                    } else {
                        // Non-transform paint animations change the sampled
                        // paint commands, so the cached display list cannot be
                        // reused as-is. Keep the stable page surface and the
                        // existing list; render() will rebuild only a clipped
                        // temporary paint list for the animated boxes.
                        self.invalidate_paint_only_display_list();
                        needs_redraw |= self.invalidate_paint_rects(paint_rects);
                    }
                }
                if trace_idle {
                    trace_reasons.push("css-animation-paint");
                }
            }
            if animation_needs_layout {
                needs_relayout = true;
                self.invalidate_display_list();
            }
            if media_running && !needs_relayout {
                self.invalidate_display_list();
            }
            doc.needs_animation_frame =
                css_animations_running || svg_animations_running || media_running;
        } else if doc.needs_animation_frame && needs_relayout {
            self.invalidate_display_list();
            if trace_idle {
                trace_reasons.push("animation-deferred");
            }
        }
        if !scroll_changed && doc.hover_changed {
            let hover_needs_style = crate::css::hover_change_requires_style(
                &doc.root,
                &doc.stylesheet,
                doc.prev_hovered_box,
                doc.hovered_box,
                &doc.hover_sensitive_nodes,
            );
            if hover_needs_style {
                needs_relayout = true;
                self.invalidate_display_list();
                if trace_idle {
                    trace_reasons.push("hover");
                }
            } else {
                doc.hover_changed = false;
                doc.prev_hovered_box = doc.hovered_box;
            }
        }
        if !needs_relayout && !scroll_changed && (doc.style_dirty || doc.has_dirty_layout()) {
            needs_relayout = true;
            self.invalidate_display_list();
            if trace_idle {
                trace_reasons.push("dom-mutation");
            }
        }
        if needs_relayout {
            self.invalidate_display_list();
            let engine = self.layout_engine();
            engine.viewport_h = viewport_h;
            engine.layout(doc, viewport_w);
            needs_redraw = true;
            if trace_idle {
                trace_reasons.push("layout");
            }
        }
        if !scroll_changed {
            let image_tick =
                doc.tick_animated_images_in_viewport_detailed(now, doc.scroll_y, viewport_h);
            if image_tick.changed_any {
                // Image commands carry the decoded pixel buffer captured when
                // the display list was built. Animated image ticks replace the
                // node's current frame buffer, so the visible viewport paint
                // list has to be rebuilt even when geometry is unchanged. Keep
                // the stable page surface and limit damage to the image boxes
                // when the ticker reported them.
                if image_tick.paint_rects.is_empty() {
                    self.invalidate_display_list();
                } else {
                    self.invalidate_paint_only_display_list();
                    let _ = self.invalidate_paint_rects(image_tick.paint_rects);
                }
                needs_redraw = true;
                if trace_idle {
                    trace_reasons.push("animated-image");
                }
            }
        }

        let next_animated_image_deadline =
            doc.next_visible_animated_image_deadline(now, doc.scroll_y, viewport_h);
        let has_visible_animated_images = next_animated_image_deadline.is_some();
        let has_timed_work = editor_overlay_active
            || doc.needs_animation_frame
            || has_visible_animated_images
            || doc.pending_images.is_some()
            || doc.pending_stylesheets.is_some()
            || self.layout_engine().has_pending_fonts()
            || self.pending_resource_relayout;

        if has_timed_work {
            if doc.needs_animation_frame {
                // CSS/SVG animations need a browser frame clock while active,
                // not an unbounded spin loop. Schedule the next sample at
                // roughly 60Hz; the host only wakes us, webcore owns the timer.
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    now + std::time::Duration::from_millis(16),
                ));
            } else {
                let resource_work_pending = doc.pending_images.is_some()
                    || doc.pending_stylesheets.is_some()
                    || self.layout_engine().has_pending_fonts()
                    || self.pending_resource_relayout;
                let mut deadline = if doc.needs_animation_frame || resource_work_pending {
                    now + std::time::Duration::from_millis(16)
                } else {
                    now + std::time::Duration::from_millis(250)
                };
                if let Some(image_deadline) = next_animated_image_deadline {
                    deadline = deadline.min(image_deadline.max(now));
                }
                if self.pending_resource_relayout {
                    deadline = deadline.min(now + std::time::Duration::from_millis(50));
                }
                if editor_overlay_active {
                    deadline = deadline.min(doc.editor.next_blink_deadline());
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if trace_idle && (!trace_reasons.is_empty() || has_timed_work) {
            eprintln!(
                "[webcore idle] redraw={} timed={} reasons={} pending_images={} pending_css={} pending_fonts={} anim_frame={}",
                needs_redraw,
                has_timed_work,
                if trace_reasons.is_empty() {
                    "none".to_string()
                } else {
                    trace_reasons.join(",")
                },
                doc.pending_images.is_some(),
                doc.pending_stylesheets.is_some(),
                self.layout_engine().has_pending_fonts(),
                doc.needs_animation_frame,
            );
        }

        self.last_idle_scroll_x = doc.scroll_x;
        self.last_idle_scroll_y = doc.scroll_y;
        needs_redraw
    }

    #[cfg(test)]
    pub(crate) fn display_list_cache_state(&self) -> Option<(u64, usize, usize, bool)> {
        self.cached_display_list.as_ref().map(|list| {
            (
                self.cached_layout_generation,
                list.commands.len(),
                list.fixed_commands.len(),
                self.display_list_dirty,
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn has_cached_content_surface(&self) -> bool {
        self.cached_content_surface.is_some()
    }

    pub(crate) fn memory_stats(&self) -> RendererMemoryStats {
        let cached_content_surface_bytes = self
            .cached_content_surface
            .as_ref()
            .map(|surface| surface.data().len())
            .unwrap_or(0);
        let cached_surface_bytes = self
            .paint_segments
            .as_ref()
            .map(|segments| {
                segments
                    .segments
                    .iter()
                    .filter_map(|segment| segment.fixed_surface.as_ref())
                    .map(|surface| surface.image.data().len())
                    .sum()
            })
            .unwrap_or(0);
        let mut tile_surface_bytes: usize = self
            .tile_manager
            .tiles
            .values()
            .map(|tile| tile.pixmap.data().len())
            .sum();
        let mut tile_count = self.tile_manager.tiles.len();
        if let Some(segments) = &self.paint_segments {
            for segment in &segments.segments {
                tile_count += segment.tiles.tiles.len();
                tile_surface_bytes += segment
                    .tiles
                    .tiles
                    .values()
                    .map(|tile| tile.pixmap.data().len())
                    .sum::<usize>();
            }
        }
        let display_list = self
            .cached_display_list
            .as_ref()
            .map(|list| list.memory_estimate())
            .unwrap_or_default();
        RendererMemoryStats {
            cached_content_surface_bytes,
            cached_surface_bytes,
            tile_surface_bytes,
            tile_count,
            display_list_commands: display_list.commands,
            display_list_estimated_bytes: display_list.total_bytes(),
            display_list_inline_bytes: display_list.inline_command_bytes,
            display_list_heap_bytes: display_list.heap_bytes,
            display_list_text_bytes: display_list.text_bytes,
            display_list_image_bytes: display_list.image_bytes,
            display_list_vector_bytes: display_list.vector_bytes,
        }
    }

    pub fn handle_window_event(
        &mut self,
        event: &WindowEvent,
        mut doc: Option<&mut crate::types::Document>,
    ) -> bool {
        match event {
            WindowEvent::ModifiersChanged(mods) => {
                self.ctrl_held = mods.state().control_key();
                self.shift_held = mods.state().shift_key();
                false
            }
            WindowEvent::PinchGesture { delta, .. } => {
                self.zoom = (self.zoom * (1.0 + *delta as f32)).clamp(0.1, 8.0);
                true
            }
            WindowEvent::PanGesture { delta, .. } => {
                if let Some(doc) = doc {
                    let zoom = self.zoom;
                    doc.scroll_x = (doc.scroll_x - delta.x / zoom).max(0.0);
                    doc.scroll_y = (doc.scroll_y - delta.y / zoom).max(0.0);
                }
                true
            }
            WindowEvent::Touch(winit::event::Touch {
                phase,
                location,
                id,
                ..
            }) => match phase {
                TouchPhase::Started => {
                    self.touches.insert(*id, (location.x, location.y));
                    if self.touches.len() < 2 {
                        self.pinch_dist = None;
                        self.touch_centroid = None;
                    }
                    false
                }
                TouchPhase::Moved => {
                    self.touches.insert(*id, (location.x, location.y));
                    if self.touches.len() == 2 {
                        let pts: Vec<(f64, f64)> = self.touches.values().copied().collect();
                        let cx = ((pts[0].0 + pts[1].0) / 2.0) as f32;
                        let cy = ((pts[0].1 + pts[1].1) / 2.0) as f32;
                        let dx = pts[0].0 - pts[1].0;
                        let dy = pts[0].1 - pts[1].1;
                        let new_dist = ((dx * dx + dy * dy) as f32).sqrt();
                        if let Some(prev_dist) = self.pinch_dist {
                            if prev_dist > 1.0 {
                                self.zoom = (self.zoom * new_dist / prev_dist).clamp(0.1, 8.0);
                            }
                        }
                        if let (Some((px, py)), Some(doc)) =
                            (self.touch_centroid, doc.as_deref_mut())
                        {
                            let sc = self.scale.max(1.0);
                            let zoom = self.zoom;
                            doc.scroll_x = (doc.scroll_x - (cx - px) / sc / zoom).max(0.0);
                            doc.scroll_y -= (cy - py) / sc / zoom;
                        }
                        self.pinch_dist = Some(new_dist);
                        self.touch_centroid = Some((cx, cy));
                        true
                    } else {
                        false
                    }
                }
                TouchPhase::Ended | TouchPhase::Cancelled => {
                    self.touches.remove(id);
                    if self.touches.len() < 2 {
                        self.pinch_dist = None;
                        self.touch_centroid = None;
                    }
                    false
                }
            },
            WindowEvent::MouseWheel { delta, .. } if self.ctrl_held => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => *y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 20.0,
                };
                self.zoom = (self.zoom * 1.1f32.powf(dy)).clamp(0.1, 8.0);
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let sc = self.scale.max(1.0);
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (*x * 20.0, -*y * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        (p.x as f32 / sc, -(p.y as f32 / sc))
                    }
                };
                if let Some(doc) = doc {
                    let client_x = self.cursor_physical.0 / sc;
                    let client_y = self.cursor_physical.1 / sc;
                    let doc_pt = (
                        client_x / self.zoom + doc.scroll_x,
                        client_y / self.zoom + doc.scroll_y,
                    );
                    let mut evt = crate::dom::HtmlEvent::new(crate::dom::HtmlEventType::Wheel);
                    evt.client_pos = (client_x, client_y);
                    evt.doc_pos = doc_pt;
                    evt.delta_x = dx;
                    evt.delta_y = dy;
                    let hit_id = crate::layout::hit_test::point_to_hit_scrolled(
                        &doc.root,
                        doc_pt,
                        (doc.scroll_x, doc.scroll_y),
                        0,
                    )
                    .map(|h| h.node_id)
                    .unwrap_or(0);
                    evt.target = hit_id;
                    doc.dispatch_input_event(evt);
                    return doc.process_wheel_event_xy(doc_pt, -dx, -dy);
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. }
                if self.ctrl_held && event.state == winit::event::ElementState::Pressed =>
            {
                match &event.logical_key {
                    Key::Character(s) if s == "=" || s == "+" => {
                        self.zoom = (self.zoom * 1.2).clamp(0.1, 8.0);
                        true
                    }
                    Key::Character(s) if s == "-" => {
                        self.zoom = (self.zoom / 1.2).clamp(0.1, 8.0);
                        true
                    }
                    Key::Character(s) if s == "0" => {
                        self.zoom = 1.0;
                        true
                    }
                    _ => false,
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_physical = (position.x as f32, position.y as f32);
                if let Some(doc) = doc {
                    let sc = self.scale.max(1.0);
                    let zoom = self.zoom;
                    let sx = self.cursor_physical.0 / sc;
                    let sy = (self.cursor_physical.1 / sc) - self.content_offset_y;
                    if sy < 0.0 {
                        return false;
                    }
                    let pt = (sx / zoom, sy / zoom + doc.scroll_y);
                    let mut redraw =
                        doc.process_mouse_event(crate::dom::HtmlEventType::MouseMove, pt, 0);
                    redraw |=
                        doc.process_mouse_event(crate::dom::HtmlEventType::PointerMove, pt, 0);
                    redraw |= doc.dispatch_over_out(pt);
                    return redraw;
                }
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let bt = match button {
                    winit::event::MouseButton::Left => 0u8,
                    winit::event::MouseButton::Middle => 1,
                    winit::event::MouseButton::Right => 2,
                    _ => 0,
                };
                if let Some(doc) = doc {
                    let sc = self.scale.max(1.0);
                    let zoom = self.zoom;
                    let sx = self.cursor_physical.0 / sc;
                    let sy = (self.cursor_physical.1 / sc) - self.content_offset_y;
                    if sy < 0.0 {
                        return false;
                    }
                    let pt = (sx / zoom, sy / zoom + doc.scroll_y);
                    let (mouse_type, ptr_type) = if *state == winit::event::ElementState::Pressed {
                        (
                            crate::dom::HtmlEventType::MouseDown,
                            crate::dom::HtmlEventType::PointerDown,
                        )
                    } else {
                        (
                            crate::dom::HtmlEventType::MouseUp,
                            crate::dom::HtmlEventType::PointerUp,
                        )
                    };
                    let mut redraw = doc.process_mouse_event(mouse_type, pt, bt);
                    redraw |= doc.process_mouse_event(ptr_type, pt, bt);
                    if bt == 2 && *state == winit::event::ElementState::Released {
                        redraw |=
                            doc.process_mouse_event(crate::dom::HtmlEventType::ContextMenu, pt, bt);
                    }
                    return redraw;
                }
                false
            }
            WindowEvent::Resized(size) => {
                if let Some(doc) = doc {
                    let mut evt = crate::dom::HtmlEvent::new(crate::dom::HtmlEventType::Resize);
                    evt.client_pos = (size.width as f32, size.height as f32);
                    doc.dispatch_input_event(evt);
                }
                false
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == winit::event::ElementState::Pressed =>
            {
                if let Key::Named(winit::keyboard::NamedKey::Tab) = &event.logical_key {
                    if let Some(doc) = doc {
                        return if self.shift_held {
                            doc.focus_prev()
                        } else {
                            doc.focus_next()
                        };
                    }
                }
                if let Some(doc) = doc {
                    let ch = match &event.logical_key {
                        Key::Character(s) => s.chars().next(),
                        Key::Named(winit::keyboard::NamedKey::Space) => Some(' '),
                        _ => None,
                    };
                    let kc = match &event.logical_key {
                        Key::Named(winit::keyboard::NamedKey::Backspace) => 8u32,
                        Key::Named(winit::keyboard::NamedKey::Delete) => 46,
                        Key::Named(winit::keyboard::NamedKey::Enter) => 13,
                        Key::Named(winit::keyboard::NamedKey::ArrowLeft) => 37,
                        Key::Named(winit::keyboard::NamedKey::ArrowRight) => 39,
                        Key::Named(winit::keyboard::NamedKey::Home) => 36,
                        Key::Named(winit::keyboard::NamedKey::End) => 35,
                        Key::Named(winit::keyboard::NamedKey::Space) => 32,
                        Key::Character(_) => 0,
                        _ => 0,
                    };
                    if kc != 0 || ch.is_some() {
                        let effective_kc = if kc != 0 {
                            kc
                        } else {
                            ch.unwrap_or(' ') as u32
                        };
                        if doc.process_key_event(
                            crate::dom::HtmlEventType::KeyDown,
                            effective_kc,
                            ch,
                            self.ctrl_held,
                            self.shift_held,
                            false,
                            false,
                        ) {
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    pub fn is_shift_held(&self) -> bool {
        self.shift_held
    }

    pub(crate) fn place_native_control_caret(
        &mut self,
        doc: &mut Document,
        point: (f32, f32),
        anchor: Option<usize>,
    ) -> bool {
        self.native_vertical_goal = None;
        let id = doc.focused_box;
        let Some(node) = doc.get_node(id) else {
            return false;
        };
        if (node.tag != "textarea" && (node.tag != "input" || !crate::types::is_text_input(node)))
            || doc.is_actually_disabled(id)
        {
            return false;
        }
        let rect = native_control_text_rect(node);
        let point = (point.0 + doc.scroll_x, point.1 + doc.scroll_y);
        if anchor.is_none() && !rect.contains(point.0, point.1) {
            return false;
        }
        let initial = ComputedStyle::INITIAL_FONT_SIZE_PX;
        let root_font = doc.root.style.font_size_px(initial, initial);
        let typography = display_list_builder::native_control_typography(
            node,
            &node.style,
            root_font,
            root_font,
            rect.w,
        );
        let raw = crate::types::input_value(node);
        let password = node
            .attributes
            .get("type")
            .is_some_and(|value| value.eq_ignore_ascii_case("password"));
        let display = |value: &str| {
            if password {
                value.chars().map(|_| '\u{2022}').collect::<String>()
            } else {
                display_list_builder::apply_text_transform(value, typography.text_transform)
            }
        };
        let text = display(&raw);
        let scale = self.scale.max(f32::EPSILON);
        let width = if typography.layout.is_some() {
            0.0
        } else {
            crate::layout::inline_layout::measure_text_width_fs_attrs(
                &mut self.font_system,
                &text,
                typography.font_size,
                cosmic_text::Weight(typography.font_weight),
                match typography.font_style {
                    1 => cosmic_text::Style::Italic,
                    2 => cosmic_text::Style::Oblique,
                    _ => cosmic_text::Style::Normal,
                },
                scale,
                &typography.font_family,
                crate::layout::inline_layout::stretch_from_percent(typography.font_stretch),
            ) + typography.letter_spacing * text.chars().count().saturating_sub(1) as f32
                + typography.word_spacing * text.chars().filter(|ch| *ch == ' ').count() as f32
        };
        let rtl = node.style.direction == Direction::RTL;
        let alignment = match node.style.text_align {
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
            TextAlign::Start if rtl => 1.0,
            TextAlign::End if !rtl => 1.0,
            _ => 0.0,
        };
        let indent = node.style.text_indent.resolve_vp(
            typography.font_size,
            rect.w,
            typography.font_size,
            0.0,
            0.0,
        );
        let origin_x = rect.x
            + if typography.layout.is_some() {
                0.0
            } else {
                (rect.w - width).max(0.0) * alignment
            }
            + indent * if rtl { -1.0 } else { 1.0 };
        let line_height = typography.used_line_height(
            &mut self.font_system,
            node.tag == "input"
                && crate::types::input_uses_minimum_normal_line_height(
                    node.attributes.get("type").map_or("text", String::as_str),
                ),
        );
        let origin_y = rect.y
            + if node.tag == "textarea" {
                0.0
            } else {
                (rect.h - line_height).max(0.0) / 2.0
            };
        let Some(display_index) = display_list_replay::painted_control_hit(
            &mut self.font_system,
            &text,
            &typography,
            line_height,
            scale,
            (
                point.0 - origin_x + node.layout.scroll_left,
                point.1 - origin_y + node.layout.scroll_top,
            ),
        ) else {
            return false;
        };
        // CSS text transforms can expand a character, while the value stays unchanged.
        let cursor = if password || typography.text_transform == TextTransform::None {
            display_index.min(raw.chars().count())
        } else {
            let boundaries: Vec<_> = std::iter::once(0)
                .chain(raw.char_indices().map(|(at, ch)| at + ch.len_utf8()))
                .collect();
            boundaries
                .partition_point(|&at| display(&raw[..at]).chars().count() < display_index)
                .min(boundaries.len().saturating_sub(1))
        };
        let Some(node) = doc.get_box_by_id_mut(id) else {
            return false;
        };
        node.input_cursor = cursor;
        node.input_sel_anchor = anchor.unwrap_or(cursor).min(raw.chars().count());
        node.input_sel_direction = if node.input_cursor < node.input_sel_anchor {
            SelectionDirection::Backward
        } else if node.input_cursor > node.input_sel_anchor {
            SelectionDirection::Forward
        } else {
            SelectionDirection::None
        };
        doc.editor.caret_visible = true;
        doc.editor.last_blink = std::time::Instant::now();
        true
    }

    pub(crate) fn move_native_control_vertically(
        &mut self,
        node: &mut WebCore,
        root_font: f32,
        key_code: u32,
        shift: bool,
    ) -> bool {
        let typography = display_list_builder::native_control_typography(
            node,
            &node.style,
            root_font,
            root_font,
            native_control_text_rect(node).w,
        );
        let value = crate::types::input_value(node);
        let display = |text: &str| {
            display_list_builder::apply_text_transform(text, typography.text_transform)
        };
        let text = display(&value);
        let cursor = display(&value.chars().take(node.input_cursor).collect::<String>())
            .chars()
            .count();
        let previous = self
            .native_vertical_goal
            .filter(|(id, _, _, _)| *id == node.node_id)
            .map(|(_, cursor, key, goal)| (cursor, key, goal));
        let line_height = typography.used_line_height(&mut self.font_system, false);
        let Some((next, key, goal)) = display_list_replay::painted_control_vertical_motion(
            &mut self.font_system,
            &text,
            cursor,
            &typography,
            line_height,
            self.scale.max(f32::EPSILON),
            key_code == 40,
            previous,
        ) else {
            return false;
        };
        node.input_cursor = if typography.text_transform == TextTransform::None {
            next
        } else {
            let boundaries: Vec<_> = std::iter::once(0)
                .chain(value.char_indices().map(|(at, ch)| at + ch.len_utf8()))
                .collect();
            boundaries
                .partition_point(|&at| display(&value[..at]).chars().count() < next)
                .min(boundaries.len().saturating_sub(1))
        };
        if !shift {
            node.input_sel_anchor = node.input_cursor;
        }
        node.input_sel_direction = if node.input_cursor < node.input_sel_anchor {
            SelectionDirection::Backward
        } else if node.input_cursor > node.input_sel_anchor {
            SelectionDirection::Forward
        } else {
            SelectionDirection::None
        };
        self.native_vertical_goal = Some((node.node_id, next, key, goal));
        true
    }

    pub(crate) fn reset_native_vertical_goal(&mut self) {
        self.native_vertical_goal = None;
    }

    pub(crate) fn reveal_native_control_caret(&mut self, doc: &mut Document) -> bool {
        let id = doc.focused_box;
        let Some(node) = doc.get_node(id) else {
            return false;
        };
        if !crate::types::is_text_input(node) || doc.is_actually_disabled(id) {
            return false;
        }
        let initial = ComputedStyle::INITIAL_FONT_SIZE_PX;
        let root_font = doc.root.style.font_size_px(initial, initial);
        let typography = display_list_builder::native_control_typography(
            node,
            &node.style,
            root_font,
            root_font,
            native_control_text_rect(node).w,
        );
        let value = crate::types::input_value(node);
        let password = node.tag == "input"
            && node
                .attributes
                .get("type")
                .is_some_and(|value| value.eq_ignore_ascii_case("password"));
        let text = if password {
            value.chars().map(|_| '\u{2022}').collect()
        } else {
            display_list_builder::apply_text_transform(&value, typography.text_transform)
        };
        let prefix: String = value.chars().take(node.input_cursor).collect();
        let cursor = if password {
            prefix.chars().count()
        } else {
            display_list_builder::apply_text_transform(&prefix, typography.text_transform)
                .chars()
                .count()
        };
        let line_height = typography.used_line_height(
            &mut self.font_system,
            node.tag == "input"
                && crate::types::input_uses_minimum_normal_line_height(
                    node.attributes.get("type").map_or("text", String::as_str),
                ),
        );
        let scale = self.scale.max(f32::EPSILON);
        let Some((x, y)) = display_list_replay::painted_control_cursor(
            &mut self.font_system,
            &text,
            cursor,
            &typography,
            line_height,
            scale,
        ) else {
            return false;
        };
        if node.tag == "input" {
            let width = native_control_text_rect(node).w;
            let old = node.layout.scroll_left;
            // Reveal the entire painted caret, not just its insertion coordinate.
            let caret_width = NATIVE_CONTROL_CARET_WIDTH_PX;
            let next = if x < old {
                x
            } else if x + caret_width > old + width {
                x + caret_width - width
            } else {
                old
            }
            .max(0.0);
            if (next - old).abs() < f32::EPSILON {
                return false;
            }
            doc.get_box_by_id_mut(id).unwrap().layout.scroll_left = next;
            doc.note_scroll_action(id);
            let mut event = crate::dom::events::DomEvent::new("scroll", id);
            doc.dispatch_dom_event(&mut event);
            return true;
        }
        let width = native_control_text_rect(node).w;
        let old_left = node.layout.scroll_left;
        let next_left = if x < old_left {
            x
        } else if x + NATIVE_CONTROL_CARET_WIDTH_PX > old_left + width {
            x + NATIVE_CONTROL_CARET_WIDTH_PX - width
        } else {
            old_left
        }
        .clamp(0.0, (node.layout.scroll_width - width).max(0.0));
        let height = node.layout.content_rect.h;
        let old = node.layout.scroll_top;
        let next = if y < old {
            y
        } else if y + line_height > old + height {
            y + line_height - height
        } else {
            old
        };
        let next = next.clamp(0.0, (node.layout.scroll_height - height).max(0.0));
        if (next - old).abs() < f32::EPSILON && (next_left - old_left).abs() < f32::EPSILON {
            return false;
        }
        let node = doc.get_box_by_id_mut(id).unwrap();
        node.layout.scroll_top = next;
        node.layout.scroll_left = next_left;
        doc.note_scroll_action(id);
        let mut event = crate::dom::events::DomEvent::new("scroll", id);
        doc.dispatch_dom_event(&mut event);
        true
    }

    pub fn cursor_icon(&self, doc: &crate::types::Document) -> CSSCursor {
        let hovered_id = doc.hovered_box;
        if hovered_id == 0 {
            return CSSCursor::Default;
        }
        let node = match doc.get_node(hovered_id) {
            Some(n) => n,
            None => return CSSCursor::Default,
        };
        if node.style.cursor != CSSCursor::Auto {
            return node.style.cursor;
        }
        fn is_link_or_button(n: &crate::types::WebCore) -> bool {
            match n.tag.as_str() {
                "a" => n.attributes.contains_key("href"),
                "button" | "summary" | "label" => true,
                "input" => matches!(
                    n.attributes.get("type").map(|s| s.as_str()),
                    Some("submit") | Some("button") | Some("reset") | Some("image")
                ),
                _ => false,
            }
        }
        if is_link_or_button(node) {
            return CSSCursor::Pointer;
        }
        if crate::types::is_text_input(node) {
            return CSSCursor::Text;
        }
        CSSCursor::Default
    }

    pub fn register_component(
        &mut self,
        tag: &str,
        measure: ComponentMeasureFn,
        paint: ComponentPaintFn,
    ) {
        self.component_registry.register(tag, measure, paint);
    }

    /// Register a trait-based custom component (new API).
    pub fn register_trait_component(
        &mut self,
        tag: &str,
        component: impl crate::types::Component + 'static,
    ) {
        self.component_registry.register_component(tag, component);
    }
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    pub fn layout_engine(&mut self) -> &mut crate::layout::LayoutEngine {
        self.layout_engine_inner.font_system = Some(&mut self.font_system as *mut _);
        self.layout_engine_inner.component_registry = self.component_registry.clone();
        self.layout_engine_inner.viewport_h = self.viewport_h;
        self.layout_engine_inner.scale = self.scale;
        &mut self.layout_engine_inner
    }

    pub fn load_html(&mut self, html: &str, viewport_width: f32) -> crate::Document {
        self.load_html_vp(html, viewport_width, 700.0)
    }
    pub fn load_html_vp(
        &mut self,
        html: &str,
        viewport_width: f32,
        viewport_height: f32,
    ) -> crate::Document {
        self.load_html_with_base(html, "", viewport_width, viewport_height)
    }
    pub fn load_html_with_base(
        &mut self,
        html: &str,
        base_url: &str,
        viewport_width: f32,
        viewport_height: f32,
    ) -> crate::Document {
        // Pass our component registry so the initial layout uses the same
        // intrinsic measurements as subsequent relayouts.
        // ⛔ `self`, not a fresh `Renderer`. This called `load_html_with_registry`,
        // which built its own — a second `FontSystem` (3.2 s cold, 173 ms warm)
        // constructed, used for one layout and dropped, while ours sat unused.
        let registry = self.component_registry.clone();
        let doc = crate::load_html_reusing(
            html,
            base_url,
            viewport_width,
            viewport_height,
            registry,
            Some(self),
        );
        // Sync engine state so subsequent layout() calls use the right viewport
        let engine = self.layout_engine();
        engine.viewport_h = viewport_height;
        doc
    }

    pub fn load_html_with_base_and_stylesheet_loader(
        &mut self,
        html: &str,
        base_url: &str,
        viewport_width: f32,
        viewport_height: f32,
        stylesheet_loader: std::sync::Arc<
            dyn Fn(&str) -> Result<String, String> + Send + Sync + 'static,
        >,
    ) -> crate::Document {
        let registry = self.component_registry.clone();
        let doc = crate::load_html_reusing_with_stylesheet_loader(
            html,
            base_url,
            viewport_width,
            viewport_height,
            registry,
            Some(self),
            Some(stylesheet_loader),
        );
        let engine = self.layout_engine();
        engine.viewport_h = viewport_height;
        doc
    }

    pub fn load_html_with_base_and_stylesheet_loader_css_wait(
        &mut self,
        html: &str,
        base_url: &str,
        viewport_width: f32,
        viewport_height: f32,
        stylesheet_loader: std::sync::Arc<
            dyn Fn(&str) -> Result<String, String> + Send + Sync + 'static,
        >,
        css_wait: std::time::Duration,
    ) -> crate::Document {
        let registry = self.component_registry.clone();
        let doc = crate::load_html_reusing_with_stylesheet_loader_and_wait(
            html,
            base_url,
            viewport_width,
            viewport_height,
            registry,
            Some(self),
            Some(stylesheet_loader),
            css_wait,
        );
        let engine = self.layout_engine();
        engine.viewport_h = viewport_height;
        doc
    }

    pub fn load_html_with_base_and_resource_loaders_css_wait(
        &mut self,
        html: &str,
        base_url: &str,
        viewport_width: f32,
        viewport_height: f32,
        stylesheet_loader: std::sync::Arc<
            dyn Fn(&str) -> Result<String, String> + Send + Sync + 'static,
        >,
        streaming_stylesheet_loader: Option<crate::StreamingStylesheetLoader>,
        image_loader: Option<
            std::sync::Arc<
                dyn Fn(&str) -> Option<crate::html::DecodedImage> + Send + Sync + 'static,
            >,
        >,
        load_images: bool,
        css_wait: std::time::Duration,
    ) -> crate::Document {
        let registry = self.component_registry.clone();
        let doc = crate::load_html_reusing_with_resource_loaders_and_wait(
            html,
            base_url,
            viewport_width,
            viewport_height,
            registry,
            Some(self),
            Some(stylesheet_loader),
            streaming_stylesheet_loader,
            image_loader,
            load_images,
            css_wait,
        );
        let engine = self.layout_engine();
        engine.viewport_h = viewport_height;
        doc
    }

    pub fn render(&mut self, doc: &mut Document, pixmap: &mut Pixmap, scale: f32) {
        let _profile_render = crate::profile::span(crate::profile::Phase::Render);
        let trace_render = std::env::var_os("WEBCORE_TRACE_RENDER").is_some();
        let render_start = std::time::Instant::now();
        let mut build_ms = 0u128;
        let mut replay_ms = 0u128;
        let mut rebuilt_display_list = false;
        self.scale = scale;
        let zoom = self.zoom.clamp(0.1, 8.0);
        let w = pixmap.width() as f32 / scale;
        let h = pixmap.height() as f32 / scale;
        let view_w = w / zoom;
        let view_h = h / zoom;
        self.viewport_h = view_h;
        let doc_h = doc.cached_scroll_height();
        let doc_w = doc.root.layout.margin_rect.w;
        doc.scroll_y = doc.scroll_y.max(0.0).min((doc_h - view_h).max(0.0));
        doc.scroll_x = doc.scroll_x.max(0.0).min((doc_w - view_w).max(0.0));
        let surface_hover_changed = self.cached_surface_hovered_id != doc.hovered_box
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc
                    .hover_sensitive_nodes
                    .contains(&self.cached_surface_hovered_id));
        let editor_overlay_active =
            doc.editor.has_focus && (doc.editor.caret_box.is_some() || doc.editor.has_selection());
        let viewport = Rect::new(doc.scroll_x, doc.scroll_y, view_w, view_h);
        let visible_animation_ids = if doc.animation_overrides.is_empty() {
            std::collections::HashSet::new()
        } else {
            animation_override_rects_with_ids(&doc.root, &doc.animation_overrides, view_w, view_h)
                .into_iter()
                .filter(|(_, rect)| rect_intersects(*rect, viewport))
                .map(|(id, _)| id)
                .collect::<std::collections::HashSet<_>>()
        };
        let visible_transform_only_animation = animation_overrides_are_transform_only_for_ids(
            &doc.animation_overrides,
            &visible_animation_ids,
        );
        // Do not keep a second full-frame exact surface here. The platform
        // pixmap already contains the presented frame, and `cached_content_surface`
        // below is the retained scroll/compositor surface. Keeping both doubled
        // viewport backing memory and made browser.rs look far heavier than it is.
        let canvas_color = (doc.root.style.background_color.a > 0)
            .then_some(doc.root.style.background_color)
            .or_else(|| {
                doc.root
                    .children
                    .iter()
                    .find(|c| c.tag == "body")
                    .map(|body| body.style.background_color)
                    .filter(|c| c.a > 0)
            })
            .map(|c| c.to_tiny_skia())
            .unwrap_or(tiny_skia::Color::WHITE);

        // Keep a deep retained paint band around the viewport. A shallow band
        // made ordinary wheel/scrollbar movement fall out of the cached display
        // list almost immediately, so scrolling paid a full record/replay pass
        // instead of shifting the existing backing surface and painting only
        // the newly exposed strip.
        let paint_band = retained_paint_band_for_doc(doc, view_w, view_h);
        let paint_top = paint_band.y;
        let paint_bottom = paint_band.bottom();

        // Native selection/scroll changes must refresh retained commands,
        // even when the value, cascade and layout generation are unchanged.
        let control = doc
            .get_node(doc.focused_box)
            .filter(|node| crate::types::is_text_input(node) || node.tag == "select")
            .map(|node| NativeControlPaintState {
                node_id: node.node_id,
                selection: (crate::types::is_text_input(node)
                    && node.input_cursor != node.input_sel_anchor)
                    .then_some((
                        node.input_cursor.min(node.input_sel_anchor),
                        node.input_cursor.max(node.input_sel_anchor),
                    )),
                scroll_bits: (
                    node.layout.scroll_left.to_bits(),
                    node.layout.scroll_top.to_bits(),
                ),
            });
        if control != self.cached_native_control {
            let scrolled =
                control
                    .zip(self.cached_native_control)
                    .is_some_and(|(current, previous)| {
                        current.node_id == previous.node_id
                            && current.scroll_bits != previous.scroll_bits
                    });
            let damage: Vec<_> = [self.cached_native_control, control]
                .into_iter()
                .flatten()
                .filter(|state| scrolled || state.selection.is_some())
                .filter_map(|state| {
                    doc.get_node(state.node_id)
                        .map(|node| node.layout.border_rect)
                })
                .collect();
            if !damage.is_empty() {
                self.invalidate_paint_rects(damage);
                self.invalidate_paint_only_display_list();
            }
            self.cached_native_control = control;
        }

        // Check what changed since last render
        let layout_changed = doc.layout_generation != self.cached_layout_generation;
        let hover_changed = doc.hovered_box != self.cached_hovered_id
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc.hover_sensitive_nodes.contains(&self.cached_hovered_id));
        let scroll_outside_cached_band = self.cached_display_list.is_none()
            || doc.scroll_y < self.cached_paint_top
            || doc.scroll_y + view_h > self.cached_paint_bottom;
        let scroll_changed_since_surface = (self.cached_surface_scroll_x - doc.scroll_x).abs()
            >= 0.5
            || (self.cached_surface_scroll_y - doc.scroll_y).abs() >= 0.5;
        let sticky_scroll_changed = scroll_changed_since_surface
            && self
                .cached_display_list
                .as_ref()
                .is_some_and(|list| list.has_scroll_dependent_sticky);
        let _scroll_only = !layout_changed
            && !hover_changed
            && !self.display_list_dirty
            && !scroll_outside_cached_band
            && self.cached_display_list.is_some();

        let mut dirty_paint_rects = self.dirty_paint_rects.clone();
        if !dirty_paint_rects.is_empty() {
            // The retained surface may contain an earlier animation sample.
            // Erase its footprint as well as painting the new position.
            dirty_paint_rects.extend(self.cached_surface_animation_rects.iter().copied());
            dirty_paint_rects = coalesce_dirty_rects(dirty_paint_rects, viewport);
        }
        if self.use_tiles {
            for rect in &dirty_paint_rects {
                self.tile_manager.invalidate_rect(rect);
                // Rebuilt command lists determine which segments changed.
                // Invalidating them first destroys unchanged fixed overlays.
                if !self.display_list_dirty
                    && let Some(segments) = &mut self.paint_segments
                {
                    segments.invalidate_rect(rect);
                }
            }
            if self.paint_segments.is_none()
                && scroll_changed_since_surface
                && self.cached_display_list.as_ref().is_some_and(|list| {
                    list.commands
                        .iter()
                        .any(|cmd| matches!(cmd, display_list::PaintCmd::BeginFixedPosition))
                })
            {
                self.tile_manager.invalidate_all();
            }
        }
        let animation_transform_overrides =
            animation_transform_matrices(&doc.root, &doc.animation_overrides, view_w, view_h);
        let transform_only_animation_frame =
            !animation_transform_overrides.is_empty() && visible_transform_only_animation;
        let dirty_base_surface = self.cached_content_surface.as_ref();
        let dirty_paint_only = !dirty_paint_rects.is_empty()
            && !layout_changed
            && !hover_changed
            && !scroll_outside_cached_band
            && !sticky_scroll_changed
            && !self.display_list_dirty
            && self.cached_display_list.is_some()
            && dirty_base_surface.is_some_and(|surface| {
                surface.width() == pixmap.width()
                    && surface.height() == pixmap.height()
                    && (self.cached_surface_scale - scale).abs() < 0.001
                    && (self.cached_surface_zoom - zoom).abs() < 0.001
                    && (self.cached_surface_scroll_x - doc.scroll_x).abs() < 0.5
                    && (self.cached_surface_scroll_y - doc.scroll_y).abs() < 0.5
                    && self.cached_surface_layout_generation == doc.layout_generation
            });
        let dirty_display_list_paint_only = self.paint_only_display_list_dirty
            && !dirty_paint_rects.is_empty()
            && !layout_changed
            && !hover_changed
            && !scroll_outside_cached_band
            && !sticky_scroll_changed
            && self.cached_display_list.is_some()
            && dirty_base_surface.is_some_and(|surface| {
                surface.width() == pixmap.width()
                    && surface.height() == pixmap.height()
                    && (self.cached_surface_scale - scale).abs() < 0.001
                    && (self.cached_surface_zoom - zoom).abs() < 0.001
                    && (self.cached_surface_scroll_x - doc.scroll_x).abs() < 0.5
                    && (self.cached_surface_scroll_y - doc.scroll_y).abs() < 0.5
                    && self.cached_surface_layout_generation == doc.layout_generation
            });

        // Only rebuild display list when layout/hover changed — NOT on scroll
        // and NOT for paint-only dirty rects. Paint-only invalidation reuses
        // the existing list and cached surface below; rebuilding here made
        // every small animation/image update pay the full page recording cost.
        let needs_rebuild = !dirty_display_list_paint_only
            && (self.display_list_dirty
                || self.cached_display_list.is_none()
                || layout_changed
                || hover_changed
                || sticky_scroll_changed
                || scroll_outside_cached_band);
        let scroll_band_rebuild_only = scroll_outside_cached_band
            && !self.display_list_dirty
            && !layout_changed
            && !hover_changed
            && self.cached_display_list.is_some()
            && self.cached_content_surface.is_some();

        if needs_rebuild {
            let build_start = std::time::Instant::now();
            let animation_restore = if doc.animation_overrides.is_empty() {
                Vec::new()
            } else {
                let overrides = doc.animation_overrides.clone();
                crate::css::apply_animation_overrides_scoped(&mut doc.root, &overrides)
            };
            // Build only a generous viewport band, still in document
            // coordinates. Scrolling inside the band reuses the same list; when
            // the viewport leaves the band we rebuild a new slice instead of
            // traversing and recording the whole page.
            let font_system = Some(&mut self.font_system as *mut _);
            let record_start = std::time::Instant::now();
            let list = display_list_builder::build_display_list_viewport_with_font_system(
                &doc.root,
                view_w,
                view_h,
                doc.scroll_x,
                doc.scroll_y,
                paint_top,
                paint_bottom,
                doc.hovered_box,
                doc.active_box,
                &doc.visited_urls,
                &doc.base_url,
                font_system,
            );
            crate::profile::record(
                crate::profile::Phase::DisplayListRecord,
                record_start.elapsed(),
            );
            if !animation_restore.is_empty() {
                crate::css::restore_animation_overrides(&mut doc.root, animation_restore);
            }
            let segments_start = std::time::Instant::now();
            let mut paint_segments = self
                .use_tiles
                .then(|| compositor::PaintSegments::from_display_list(&list, view_w, doc_h))
                .flatten();
            crate::profile::record(
                crate::profile::Phase::DisplayListSegments,
                segments_start.elapsed(),
            );
            let previous_segment_count = self
                .paint_segments
                .as_ref()
                .map_or(0, |segments| segments.segments.len());
            let mut retained_segment_count = 0;
            let retain_start = std::time::Instant::now();
            if let (Some(new), Some(previous)) = (&mut paint_segments, self.paint_segments.take()) {
                retained_segment_count = new.retain_unchanged_rasters(previous);
            }
            crate::profile::record(
                crate::profile::Phase::DisplayListRetain,
                retain_start.elapsed(),
            );
            if trace_render {
                eprintln!(
                    "[webcore render] paint_segments previous={} current={} retained={}",
                    previous_segment_count,
                    paint_segments
                        .as_ref()
                        .map_or(0, |segments| segments.segments.len()),
                    retained_segment_count,
                );
            }
            self.paint_segments = paint_segments;
            self.cached_display_list = Some(list);
            self.cached_paint_top = paint_top;
            self.cached_paint_bottom = paint_bottom;
            self.cached_hovered_id = doc.hovered_box;
            self.cached_layout_generation = doc.layout_generation;
            self.display_list_dirty = false;
            self.paint_only_display_list_dirty = false;
            self.tile_manager.invalidate_all();

            // Rebuild compositor layer tree on layout change
            if layout_changed {
                self.compositor.build_layers(&doc.root, view_w, view_h);
            }
            build_ms = build_start.elapsed().as_millis();
            crate::profile::record(crate::profile::Phase::DisplayList, build_start.elapsed());
            rebuilt_display_list = true;
        }
        self.cached_scroll_x = doc.scroll_x;
        self.cached_scroll_y = doc.scroll_y;

        // Replay display list (cached — only rebuilt on layout/hover change)
        let mut used_scroll_surface = false;
        let mut used_dirty_surface = false;
        let mut page_content_repainted = false;
        if dirty_display_list_paint_only {
            if let Some(surface) = self.cached_content_surface.as_ref() {
                let replay_start = std::time::Instant::now();
                let tile_scale = scale * zoom;
                if dirty_paint_rects.is_empty() {
                    pixmap.data_mut().copy_from_slice(surface.data());
                }
                if !dirty_paint_rects.is_empty() {
                    // Refresh the retained command band as well as its damaged
                    // pixels. A transient list leaves old opacity/image commands
                    // in the cache, resurrecting them on the next tile replay.
                    let clip_top = self.cached_paint_top;
                    let clip_bottom = self.cached_paint_bottom;
                    let animation_restore = if doc.animation_overrides.is_empty() {
                        Vec::new()
                    } else {
                        let overrides = doc.animation_overrides.clone();
                        crate::css::apply_animation_overrides_scoped(&mut doc.root, &overrides)
                    };
                    let font_system = Some(&mut self.font_system as *mut _);
                    let paint_list =
                        display_list_builder::build_display_list_viewport_with_font_system(
                            &doc.root,
                            view_w,
                            view_h,
                            doc.scroll_x,
                            doc.scroll_y,
                            clip_top,
                            clip_bottom,
                            doc.hovered_box,
                            doc.active_box,
                            &doc.visited_urls,
                            &doc.base_url,
                            font_system,
                        );
                    if !animation_restore.is_empty() {
                        crate::css::restore_animation_overrides(&mut doc.root, animation_restore);
                    }
                    let mut segments = self
                        .use_tiles
                        .then(|| {
                            compositor::PaintSegments::from_display_list(&paint_list, view_w, doc_h)
                        })
                        .flatten();
                    if let (Some(new), Some(previous)) = (&mut segments, self.paint_segments.take())
                    {
                        new.retain_unchanged_rasters(previous);
                    }
                    // Large paint updates should composite retained layers;
                    // a clipped replay would rerasterize all static overlays.
                    let retained_layers = segments.is_some()
                        && dirty_paint_rects.iter().any(|rect| {
                            viewport_clip_from_doc_rect(
                                *rect,
                                doc.scroll_x,
                                doc.scroll_y,
                                view_w,
                                view_h,
                            )
                            .is_some_and(|clip| clip.w * clip.h >= view_w * view_h * 0.5)
                        });
                    if !retained_layers {
                        pixmap.data_mut().copy_from_slice(surface.data());
                    }
                    for rect in dirty_paint_rects.iter().filter(|_| !retained_layers) {
                        if let Some(clip) = viewport_clip_from_doc_rect(
                            *rect,
                            doc.scroll_x,
                            doc.scroll_y,
                            view_w,
                            view_h,
                        ) {
                            fill_viewport_clip(pixmap, clip, tile_scale, canvas_color);
                            display_list_replay::replay_with_scroll_clip(
                                &paint_list,
                                pixmap,
                                tile_scale,
                                &mut self.font_system,
                                &mut self.swash_cache,
                                doc.scroll_x,
                                doc.scroll_y,
                                clip,
                            );
                        }
                    }
                    self.paint_segments = segments;
                    self.cached_display_list = Some(paint_list);
                    replay_ms = replay_start.elapsed().as_millis();
                    used_dirty_surface = !retained_layers;
                    page_content_repainted = true;
                    self.display_list_dirty = false;
                    self.paint_only_display_list_dirty = false;
                    self.dirty_paint_rects.clear();
                }
            }
        }
        if dirty_paint_only && !used_dirty_surface {
            if let (Some(surface), Some(list)) = (
                self.cached_content_surface.as_ref(),
                self.cached_display_list.as_ref(),
            ) {
                let tile_scale = scale * zoom;
                let can_repaint_dirty = surface.width() == pixmap.width()
                    && surface.height() == pixmap.height()
                    && (self.cached_surface_scale - scale).abs() < 0.001
                    && (self.cached_surface_zoom - zoom).abs() < 0.001
                    && (self.cached_surface_scroll_x - doc.scroll_x).abs() < 0.5
                    && (self.cached_surface_scroll_y - doc.scroll_y).abs() < 0.5
                    && self.cached_surface_layout_generation == doc.layout_generation;
                if can_repaint_dirty {
                    let replay_start = std::time::Instant::now();
                    pixmap.data_mut().copy_from_slice(surface.data());
                    for rect in &dirty_paint_rects {
                        if let Some(clip) = viewport_clip_from_doc_rect(
                            *rect,
                            doc.scroll_x,
                            doc.scroll_y,
                            view_w,
                            view_h,
                        ) {
                            fill_viewport_clip(pixmap, clip, tile_scale, canvas_color);
                            if animation_transform_overrides.is_empty() {
                                display_list_replay::replay_with_scroll_clip(
                                    list,
                                    pixmap,
                                    tile_scale,
                                    &mut self.font_system,
                                    &mut self.swash_cache,
                                    doc.scroll_x,
                                    doc.scroll_y,
                                    clip,
                                );
                            } else {
                                // The dirty rectangle can also contain unrelated content.
                                // Repainting only the animated transform subtree after
                                // clearing it erases everything else beneath that rectangle.
                                display_list_replay::replay_with_scroll_clip_and_transform_overrides(
                                    list,
                                    pixmap,
                                    tile_scale,
                                    &mut self.font_system,
                                    &mut self.swash_cache,
                                    doc.scroll_x,
                                    doc.scroll_y,
                                    clip,
                                    &animation_transform_overrides,
                                );
                            }
                        }
                    }
                    replay_ms = replay_start.elapsed().as_millis();
                    used_dirty_surface = true;
                    page_content_repainted = true;
                    self.display_list_dirty = false;
                    self.dirty_paint_rects.clear();
                }
            }
        }
        // Do not shift the previous viewport bitmap as a scroll shortcut.
        //
        // The display list is already cached in document coordinates, so a
        // scroll can be replayed without rebuilding layout or re-recording the
        // list. Shifting the last viewport surface and repainting only the
        // exposed strip was faster on simple pages, but it made any stale clip,
        // text paint ownership, image/font update, or newly applied style stick
        // to the scrolled pixels. Real pages then looked correct on first paint
        // but lost/overlapped text after scrolling away and back.
        const ENABLE_SCROLL_SURFACE_SHIFT: bool = false;
        if ENABLE_SCROLL_SURFACE_SHIFT
            && !self.use_tiles
            && !used_dirty_surface
            && (!needs_rebuild || scroll_band_rebuild_only)
        {
            if let (Some(surface), Some(list)) = (
                self.cached_content_surface.as_ref(),
                self.cached_display_list.as_ref(),
            ) {
                let tile_scale = scale * zoom;
                let dx_px = ((doc.scroll_x - self.cached_surface_scroll_x) * tile_scale).round();
                let dy_px = ((doc.scroll_y - self.cached_surface_scroll_y) * tile_scale).round();
                let scroll_surface_reject = if surface.width() != pixmap.width()
                    || surface.height() != pixmap.height()
                {
                    Some("surface-size")
                } else if (self.cached_surface_scale - scale).abs() >= 0.001
                    || (self.cached_surface_zoom - zoom).abs() >= 0.001
                {
                    Some("scale")
                } else if dx_px.abs() >= 0.5 {
                    Some("horizontal-scroll")
                } else if dy_px.abs() < 1.0 {
                    Some("no-scroll-delta")
                } else if dy_px.abs() >= pixmap.height() as f32 {
                    Some("large-scroll-delta")
                } else if self.cached_surface_layout_generation != doc.layout_generation {
                    Some("layout-generation")
                } else if surface_hover_changed {
                    Some("hover")
                } else if editor_overlay_active && self.cached_surface_active_id != doc.active_box {
                    Some("active")
                } else if editor_overlay_active
                    && self.cached_surface_caret_visible != doc.editor.caret_visible
                {
                    Some("caret-visibility")
                } else if !self.component_registry.map.is_empty()
                    || !self.component_registry.components.is_empty()
                {
                    Some("custom-components")
                } else if doc.open_select != 0 || doc.open_picker != 0 {
                    Some("popup")
                } else if editor_overlay_active {
                    Some("editing-overlay")
                } else {
                    None
                };
                let can_shift_surface = scroll_surface_reject.is_none();
                if can_shift_surface {
                    let replay_start = std::time::Instant::now();
                    let dy_px = dy_px as i32;
                    let pm_w = pixmap.width();
                    let pm_h = pixmap.height();
                    copy_surface_shifted_y(surface.data(), pixmap.data_mut(), pm_w, pm_h, dy_px);
                    let abs_dy = dy_px.unsigned_abs().min(pixmap.height());
                    let dirty_clip = if dy_px > 0 {
                        let y_px = pixmap.height().saturating_sub(abs_dy);
                        fill_physical_rect(pixmap, 0, y_px, pixmap.width(), abs_dy, canvas_color);
                        Rect::new(
                            0.0,
                            y_px as f32 / tile_scale.max(0.001),
                            view_w,
                            abs_dy as f32 / tile_scale.max(0.001) + 2.0,
                        )
                    } else {
                        fill_physical_rect(pixmap, 0, 0, pixmap.width(), abs_dy, canvas_color);
                        Rect::new(
                            0.0,
                            0.0,
                            view_w,
                            abs_dy as f32 / tile_scale.max(0.001) + 2.0,
                        )
                    };
                    if animation_transform_overrides.is_empty() {
                        display_list_replay::replay_with_scroll_clip(
                            list,
                            pixmap,
                            tile_scale,
                            &mut self.font_system,
                            &mut self.swash_cache,
                            doc.scroll_x,
                            doc.scroll_y,
                            dirty_clip,
                        );
                    } else {
                        display_list_replay::replay_with_scroll_clip_and_transform_overrides(
                            list,
                            pixmap,
                            tile_scale,
                            &mut self.font_system,
                            &mut self.swash_cache,
                            doc.scroll_x,
                            doc.scroll_y,
                            dirty_clip,
                            &animation_transform_overrides,
                        );
                    }
                    let scrollbar_w = doc.root.style.scrollbar_width_px();
                    if scrollbar_w > 0.0 {
                        let x = pixmap
                            .width()
                            .saturating_sub((scrollbar_w * scale).ceil() as u32);
                        fill_physical_rect(
                            pixmap,
                            x,
                            0,
                            pixmap.width().saturating_sub(x),
                            pixmap.height(),
                            canvas_color,
                        );
                    }
                    replay_ms = replay_start.elapsed().as_millis();
                    used_scroll_surface = true;
                    page_content_repainted = true;
                } else if trace_render && dy_px.abs() >= 1.0 {
                    eprintln!(
                        "[webcore render] scroll_surface_reject={} dx_px={:.0} dy_px={:.0}",
                        scroll_surface_reject.unwrap_or("unknown"),
                        dx_px,
                        dy_px,
                    );
                }
            }
        }

        let opaque_base_segment = if self.use_tiles
            && view_w * scale * zoom >= pixmap.width() as f32
            && view_h * scale * zoom >= pixmap.height() as f32
        {
            self.paint_segments.as_ref().and_then(|segments| {
                segments.segments.iter().rposition(|segment| {
                    segment.fixed
                        && !segment.backdrop_dependent
                        && !segment.has_animated_transform(&animation_transform_overrides)
                        && segment.starts_with_opaque_viewport_base(view_w, view_h)
                })
            })
        } else {
            None
        };
        #[cfg(test)]
        let opaque_base_segment = if self.disable_opaque_occlusion {
            None
        } else {
            opaque_base_segment
        };
        if !used_dirty_surface && !used_scroll_surface && opaque_base_segment.is_none() {
            pixmap.fill(canvas_color);
        }
        if !used_dirty_surface
            && !used_scroll_surface
            && let Some(ref list) = self.cached_display_list
        {
            let replay_start = std::time::Instant::now();
            if self.use_tiles {
                if let Some(segments) = &mut self.paint_segments {
                    let tile_scale = scale * zoom;
                    // The opaque base overwrites every destination pixel, making
                    // all preceding segments irrelevant to this composition.
                    for segment in segments
                        .segments
                        .iter_mut()
                        .skip(opaque_base_segment.unwrap_or(0))
                    {
                        if segment.fixed {
                            let fixed_start =
                                crate::profile::is_enabled().then(std::time::Instant::now);
                            let direct_update = segment.direct_image_update
                                && view_w * tile_scale >= pixmap.width() as f32
                                && view_h * tile_scale >= pixmap.height() as f32
                                && segment.starts_with_opaque_viewport_base(view_w, view_h);
                            let _direct_span = direct_update
                                .then(|| crate::profile::span(crate::profile::Phase::DirectReplay));
                            // Backdrop effects need the real destination; other fixed
                            // segments can be composited from a retained viewport layer.
                            if segment.has_animated_transform(&animation_transform_overrides)
                                || segment.backdrop_dependent
                                || direct_update
                            {
                                segment.direct_image_update = false;
                                segment.fixed_surface = None;
                                if animation_transform_overrides.is_empty() {
                                    display_list_replay::replay_commands_with_scroll(
                                        &segment.list.commands,
                                        pixmap,
                                        tile_scale,
                                        &mut self.font_system,
                                        &mut self.swash_cache,
                                        0.0,
                                        0.0,
                                    );
                                } else {
                                    display_list_replay::replay_commands_with_scroll_and_transform_overrides(
                                        &segment.list.commands,
                                        pixmap,
                                        tile_scale,
                                        &mut self.font_system,
                                        &mut self.swash_cache,
                                        0.0,
                                        0.0,
                                        &animation_transform_overrides,
                                    );
                                }
                            } else {
                                let cache_matches =
                                    segment.fixed_surface.as_ref().is_some_and(|surface| {
                                        surface.matches(pixmap.width(), pixmap.height(), tile_scale)
                                    });
                                if !cache_matches {
                                    segment.fixed_surface = None;
                                    if let Some(mut surface) =
                                        Pixmap::new(pixmap.width(), pixmap.height())
                                    {
                                        display_list_replay::replay_commands_with_scroll(
                                            &segment.list.commands,
                                            &mut surface,
                                            tile_scale,
                                            &mut self.font_system,
                                            &mut self.swash_cache,
                                            0.0,
                                            0.0,
                                        );
                                        segment.fixed_surface =
                                            Some(compositor::FixedSurface::from_viewport(
                                                surface, tile_scale,
                                            ));
                                    }
                                }
                                if let Some(surface) = &segment.fixed_surface {
                                    surface.composite(pixmap);
                                } else {
                                    display_list_replay::replay_commands_with_scroll(
                                        &segment.list.commands,
                                        pixmap,
                                        tile_scale,
                                        &mut self.font_system,
                                        &mut self.swash_cache,
                                        0.0,
                                        0.0,
                                    );
                                }
                            }
                            if let Some(started) = fixed_start {
                                crate::profile::record(
                                    crate::profile::Phase::FixedReplay,
                                    started.elapsed(),
                                );
                            }
                            continue;
                        }
                        segment.tiles.doc_width = doc_w.max(view_w);
                        segment.tiles.doc_height = doc_h.max(view_h);
                        let needed = segment.tiles.update_viewport(
                            Rect::new(doc.scroll_x, doc.scroll_y, view_w, view_h),
                            tile_scale,
                        );
                        for (tx, ty) in needed {
                            if segment.tiles.ensure_tile(tx, ty) {
                                let tile_profile_start =
                                    crate::profile::is_enabled().then(std::time::Instant::now);
                                if let Some(tile) = segment.tiles.tiles.get_mut(&(tx, ty)) {
                                    tile.pixmap.fill(tiny_skia::Color::TRANSPARENT);
                                    display_list_replay::replay_tile_with_scroll_and_transform_overrides(
                                        &segment.list,
                                        &mut tile.pixmap,
                                        tile_scale,
                                        &mut self.font_system,
                                        &mut self.swash_cache,
                                        tx as f32 * tiles::TILE_SIZE,
                                        ty as f32 * tiles::TILE_SIZE,
                                        doc.scroll_x,
                                        doc.scroll_y,
                                        (!animation_transform_overrides.is_empty())
                                            .then_some(&animation_transform_overrides),
                                    );
                                }
                                segment.tiles.mark_clean(tx, ty);
                                if let Some(started) = tile_profile_start {
                                    crate::profile::record(
                                        crate::profile::Phase::TileRaster,
                                        started.elapsed(),
                                    );
                                }
                            }
                        }
                        segment.tiles.evict_distant();
                        segment.tiles.composite_over(
                            pixmap,
                            doc.scroll_x,
                            doc.scroll_y,
                            tile_scale,
                        );
                    }
                } else {
                    self.tile_manager.doc_width = doc_w.max(view_w);
                    self.tile_manager.doc_height = doc_h.max(view_h);
                    let tile_scale = scale * zoom;
                    let needed_tiles = self.tile_manager.update_viewport(
                        Rect::new(doc.scroll_x, doc.scroll_y, view_w, view_h),
                        tile_scale,
                    );
                    for (tx, ty) in needed_tiles {
                        let needs_tile = self.tile_manager.ensure_tile(tx, ty);
                        if needs_tile {
                            let tile_profile_start =
                                crate::profile::is_enabled().then(std::time::Instant::now);
                            if let Some(tile) = self.tile_manager.tiles.get_mut(&(tx, ty)) {
                                tile.pixmap.fill(canvas_color);
                                let tile_scroll_x = tx as f32 * tiles::TILE_SIZE;
                                let tile_scroll_y = ty as f32 * tiles::TILE_SIZE;
                                display_list_replay::replay_tile_with_scroll_and_transform_overrides(
                                list,
                                &mut tile.pixmap,
                                tile_scale,
                                &mut self.font_system,
                                &mut self.swash_cache,
                                tile_scroll_x,
                                tile_scroll_y,
                                doc.scroll_x,
                                doc.scroll_y,
                                (!animation_transform_overrides.is_empty())
                                    .then_some(&animation_transform_overrides),
                            );
                                tile.dirty = false;
                            }
                            if let Some(started) = tile_profile_start {
                                crate::profile::record(
                                    crate::profile::Phase::TileRaster,
                                    started.elapsed(),
                                );
                            }
                        }
                    }
                    self.tile_manager.evict_distant();
                    let composite_profile_start =
                        crate::profile::is_enabled().then(std::time::Instant::now);
                    self.tile_manager
                        .composite_to(pixmap, doc.scroll_x, doc.scroll_y, tile_scale);
                    if let Some(started) = composite_profile_start {
                        crate::profile::record(
                            crate::profile::Phase::TileComposite,
                            started.elapsed(),
                        );
                    }
                }
            } else {
                let direct_profile_start =
                    crate::profile::is_enabled().then(std::time::Instant::now);
                // The scroll offset is applied HERE, at replay, which is what makes
                // one cached list serve every scroll position.
                if animation_transform_overrides.is_empty() {
                    display_list_replay::replay_with_scroll(
                        list,
                        pixmap,
                        scale * zoom,
                        &mut self.font_system,
                        &mut self.swash_cache,
                        doc.scroll_x,
                        doc.scroll_y,
                    );
                } else {
                    display_list_replay::replay_with_scroll_and_transform_overrides(
                        list,
                        pixmap,
                        scale * zoom,
                        &mut self.font_system,
                        &mut self.swash_cache,
                        doc.scroll_x,
                        doc.scroll_y,
                        &animation_transform_overrides,
                    );
                }
                if let Some(started) = direct_profile_start {
                    crate::profile::record(crate::profile::Phase::DirectReplay, started.elapsed());
                }
            }
            page_content_repainted = true;
            replay_ms = replay_start.elapsed().as_millis();
        }
        let should_cache_content_surface =
            page_content_repainted && (!transform_only_animation_frame || !used_dirty_surface);
        if should_cache_content_surface {
            let cache_start = crate::profile::is_enabled().then(std::time::Instant::now);
            self.cache_content_surface(pixmap);
            self.cached_surface_animation_rects = animation_override_rects_with_ids(
                &doc.root,
                &doc.animation_overrides,
                view_w,
                view_h,
            )
            .into_iter()
            .map(|(_, rect)| rect)
            .collect();
            if let Some(started) = cache_start {
                crate::profile::record(crate::profile::Phase::ContentCache, started.elapsed());
            }
            self.cached_surface_scale = scale;
            self.cached_surface_zoom = zoom;
            self.cached_surface_scroll_x = doc.scroll_x;
            self.cached_surface_scroll_y = doc.scroll_y;
            self.cached_surface_layout_generation = doc.layout_generation;
            self.cached_surface_hovered_id = doc.hovered_box;
            self.cached_surface_active_id = doc.active_box;
            self.cached_surface_caret_visible = doc.editor.caret_visible;
        }
        if page_content_repainted {
            self.dirty_paint_rects.clear();
            self.paint_only_display_list_dirty = false;
        }
        if page_content_repainted
            && let Some(ref list) = self.cached_display_list
            && !list.fixed_commands.is_empty()
        {
            let fixed_start = crate::profile::is_enabled().then(std::time::Instant::now);
            if animation_transform_overrides.is_empty() {
                display_list_replay::replay_commands_with_scroll(
                    &list.fixed_commands,
                    pixmap,
                    scale * zoom,
                    &mut self.font_system,
                    &mut self.swash_cache,
                    0.0,
                    0.0,
                );
            } else {
                display_list_replay::replay_commands_with_scroll_and_transform_overrides(
                    &list.fixed_commands,
                    pixmap,
                    scale * zoom,
                    &mut self.font_system,
                    &mut self.swash_cache,
                    0.0,
                    0.0,
                    &animation_transform_overrides,
                );
            }
            if let Some(started) = fixed_start {
                crate::profile::record(crate::profile::Phase::FixedReplay, started.elapsed());
            }
        }
        // Paint custom components on top of the display list
        if !self.component_registry.map.is_empty() || !self.component_registry.components.is_empty()
        {
            self.paint_custom_components(
                &doc.root,
                pixmap,
                doc.scroll_x,
                doc.scroll_y,
                scale * zoom,
            );
        }
        if doc.open_select != 0 {
            if let Some(sel_node) = doc.get_node(doc.open_select) {
                self.scale = scale * zoom;
                self.draw_select_dropdown(
                    sel_node,
                    pixmap,
                    doc.scroll_x,
                    doc.scroll_y,
                    doc.dropdown_hover_idx,
                    &doc.select_popup().unwrap(),
                );
            }
        }
        // The colour picker sits on the SAME overlay surface as the dropdown,
        // drawn after the page for the same reason: a popup is not in the flow
        // and must paint over whatever it covers.
        if doc.open_picker != 0 {
            self.scale = scale * zoom;
            self.draw_color_picker(doc, pixmap, doc.scroll_x, doc.scroll_y);
        }
        if doc.editor.has_selection() {
            for (caret_id, start, end) in doc.editor.selection_segments(&doc.root) {
                self.scale = scale * zoom;
                self.draw_selection_highlight(
                    &doc.root,
                    pixmap,
                    doc.scroll_x,
                    doc.scroll_y,
                    caret_id,
                    start,
                    end,
                );
            }
        }
        if doc.editor.caret_visible {
            if let Some((caret_id, caret_local)) = doc.editor.caret_info() {
                if !doc.editor.read_only
                    || crate::dom::is_in_contenteditable_by_id(&doc.root, caret_id)
                {
                    self.scale = scale * zoom;
                    self.draw_caret(
                        &doc.root,
                        pixmap,
                        doc.scroll_x,
                        doc.scroll_y,
                        caret_id,
                        caret_local,
                    );
                }
            }
        }
        if let Some(node) = doc.get_node(doc.focused_box)
            && doc.editor.caret_visible
            && node.input_cursor == node.input_sel_anchor
            && (node.tag == "textarea"
                || (node.tag == "input" && crate::types::is_text_input(node)))
            && !doc.is_actually_disabled(doc.focused_box)
        {
            let initial_font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
            let root_font_px = doc
                .root
                .style
                .font_size_px(initial_font_px, initial_font_px);
            let typography = display_list_builder::native_control_typography(
                node,
                &node.style,
                root_font_px,
                root_font_px,
                native_control_text_rect(node).w,
            );
            let font_px = typography.font_size;
            let value = crate::types::input_value(node);
            let password = node
                .attributes
                .get("type")
                .is_some_and(|kind| kind.eq_ignore_ascii_case("password"));
            let displayed = |text: &str| {
                if password {
                    text.chars().map(|_| '\u{2022}').collect::<String>()
                } else {
                    display_list_builder::apply_text_transform(text, typography.text_transform)
                }
            };
            let prefix = displayed(&value.chars().take(node.input_cursor).collect::<String>());
            let value = displayed(&value);
            let mut measure = |text: &str| {
                crate::layout::inline_layout::measure_text_width_fs_attrs(
                    &mut self.font_system,
                    text,
                    font_px,
                    cosmic_text::Weight(typography.font_weight),
                    match typography.font_style {
                        1 => cosmic_text::Style::Italic,
                        2 => cosmic_text::Style::Oblique,
                        _ => cosmic_text::Style::Normal,
                    },
                    scale * zoom,
                    &typography.font_family,
                    crate::layout::inline_layout::stretch_from_percent(typography.font_stretch),
                ) + typography.letter_spacing * text.chars().count().saturating_sub(1) as f32
                    + typography.word_spacing * text.chars().filter(|ch| *ch == ' ').count() as f32
            };
            let advance = if typography.layout.is_some() {
                0.0
            } else {
                measure(&prefix)
            };
            let value_width = if typography.layout.is_some() || prefix == value {
                advance
            } else {
                measure(&value)
            };
            let rect = native_control_text_rect(node);
            let rtl = node.style.direction == crate::types::Direction::RTL;
            let alignment = match node.style.text_align {
                crate::types::TextAlign::Center => 0.5,
                crate::types::TextAlign::Right => 1.0,
                crate::types::TextAlign::Start if rtl => 1.0,
                crate::types::TextAlign::End if !rtl => 1.0,
                _ => 0.0,
            };
            let indent = node
                .style
                .text_indent
                .resolve_vp(font_px, rect.w, font_px, 0.0, 0.0);
            let text_x = rect.x
                + if typography.layout.is_some() {
                    0.0
                } else {
                    (rect.w - value_width).max(0.0) * alignment
                }
                + indent * if rtl { -1.0 } else { 1.0 };
            let line_height = typography.used_line_height(
                &mut self.font_system,
                node.tag == "input"
                    && crate::types::input_uses_minimum_normal_line_height(
                        node.attributes.get("type").map_or("text", String::as_str),
                    ),
            );
            let (cursor_x, cursor_y) = display_list_replay::painted_control_cursor(
                &mut self.font_system,
                &value,
                prefix.chars().count(),
                &typography,
                line_height,
                scale * zoom,
            )
            .unwrap_or((advance, 0.0));
            let caret_x = (text_x + cursor_x - node.layout.scroll_left).clamp(
                rect.x,
                (rect.right() - NATIVE_CONTROL_CARET_WIDTH_PX).max(rect.x),
            ) - doc.scroll_x;
            let text_y = if node.tag == "textarea" {
                rect.y
            } else {
                rect.y + (rect.h - line_height).max(0.0) / 2.0
            };
            let caret_top = text_y + cursor_y - node.layout.scroll_top;
            let clipped_top = caret_top.max(rect.y);
            let caret_h = ((caret_top + line_height).min(rect.bottom()) - clipped_top).max(0.0);
            let caret_y = clipped_top - doc.scroll_y;
            let color = node.style.caret_color.unwrap_or(node.style.color);
            let mut paint = Paint::default();
            paint.set_color(color.to_tiny_skia());
            if let Some(caret) =
                SkRect::from_xywh(caret_x, caret_y, NATIVE_CONTROL_CARET_WIDTH_PX, caret_h)
            {
                pixmap.fill_rect(
                    caret,
                    &paint,
                    Transform::from_scale(scale * zoom, scale * zoom),
                    None,
                );
            }
        }
        self.scale = scale;
        let scrollbar_w = doc.root.style.scrollbar_width_px();
        if doc_h > view_h && scrollbar_w > 0.0 {
            let thumb_col = doc
                .root
                .style
                .scrollbar_thumb_color
                .unwrap_or(Color::rgba(128, 128, 128, 160));
            let track_col = doc
                .root
                .style
                .scrollbar_track_color
                .unwrap_or(Color::rgba(128, 128, 128, 40));
            let track_h = h;
            let (thumb_h, thumb_y, _) = crate::types::scrollbar_hit::viewport_scrollbar_thumb(
                track_h,
                view_h,
                doc_h,
                doc.scroll_y,
            );
            let track_x = w - scrollbar_w;
            let ts = Transform::from_scale(self.scale, self.scale);
            let mut paint = Paint::default();
            paint.set_color(track_col.to_tiny_skia());
            if let Some(r) = SkRect::from_xywh(track_x, 0.0, scrollbar_w, track_h) {
                pixmap.fill_rect(r, &paint, ts, None);
            }
            paint.set_color(thumb_col.to_tiny_skia());
            if let Some(path) = rounded_rect_path(
                track_x + 1.0,
                thumb_y + 1.0,
                (scrollbar_w - 2.0).max(1.0),
                thumb_h - 2.0,
                3.0,
            ) {
                pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
            }
        }
        if trace_render {
            let commands = self
                .cached_display_list
                .as_ref()
                .map(|list| list.commands.len() + list.fixed_commands.len())
                .unwrap_or(0);
            eprintln!(
                "[webcore render] total={}ms build={}ms replay={}ms rebuilt={} scroll_surface={} dirty_surface={} commands={} layout_changed={} hover_changed={} dirty={} scroll_band={} scroll=({:.1},{:.1})",
                render_start.elapsed().as_millis(),
                build_ms,
                replay_ms,
                rebuilt_display_list,
                used_scroll_surface,
                used_dirty_surface,
                commands,
                layout_changed,
                hover_changed,
                self.display_list_dirty,
                scroll_outside_cached_band,
                doc.scroll_x,
                doc.scroll_y,
            );
        }
        if transform_only_animation_frame && used_dirty_surface {
            // Transform-only CSS animations are composited from the retained
            // unanimated page surface. Do not overwrite that base surface with
            // the current animated sample, or the next frame has to erase the
            // previous sample before drawing the new one and marquee/ticker
            // animations visibly crawl or leave stale strips.
            return;
        }
    }

    fn cache_content_surface(&mut self, pixmap: &Pixmap) {
        if self.cached_content_surface.as_ref().is_none_or(|surface| {
            surface.width() != pixmap.width() || surface.height() != pixmap.height()
        }) {
            self.cached_content_surface = Pixmap::new(pixmap.width(), pixmap.height());
        }
        if let Some(surface) = self.cached_content_surface.as_mut() {
            surface.data_mut().copy_from_slice(pixmap.data());
        }
    }

    fn draw_text_run(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font_px: f32,
        line_h: f32,
        weight: FontWeight,
        font_style: FontStyle,
        font_family: &str,
        color: CTextColor,
        pixmap: &mut Pixmap,
        mask: Option<&Mask>,
    ) -> f32 {
        self.draw_text_run_ex(
            text,
            x,
            y,
            font_px,
            line_h,
            weight,
            font_style,
            font_family,
            100.0,
            &[],
            color,
            pixmap,
            mask,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_run_ex(
        &mut self,
        text: &str,
        x: f32,
        y: f32,
        font_px: f32,
        line_h: f32,
        weight: FontWeight,
        font_style: FontStyle,
        font_family: &str,
        font_stretch: f32,
        variation: &[(String, f32)],
        color: CTextColor,
        pixmap: &mut Pixmap,
        mask: Option<&Mask>,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let sc = self.scale;
        let phys_px = font_px * sc;
        let phys_lh = line_h * sc;
        let metrics = Metrics::new(phys_px, phys_lh);
        let resolved = resolve_css_family(&self.font_system, font_family);
        let family = resolved.as_family();
        let ct_w = weight_from_style(weight, variation);
        let ct_s = match font_style {
            FontStyle::Italic => CTextStyle::Italic,
            FontStyle::Oblique => CTextStyle::Oblique,
            FontStyle::Normal => CTextStyle::Normal,
        };
        let ct_stretch = stretch_from_percent(font_stretch);
        let attrs = Attrs::new()
            .weight(ct_w)
            .style(ct_s)
            .stretch(ct_stretch)
            .family(family);
        if self.shape_buf.is_none() {
            self.shape_buf = Some(Buffer::new(&mut self.font_system, metrics));
        }
        let mut buf = self.shape_buf.take().unwrap();
        buf.set_metrics(&mut self.font_system, metrics);
        buf.set_size(&mut self.font_system, None, Some((phys_lh + 4.0).max(1.0)));
        buf.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(&mut self.font_system, false);
        let mut phys_advance = 0.0f32;
        for run in buf.layout_runs() {
            if run.line_w > phys_advance {
                phys_advance = run.line_w;
            }
        }
        let logical_advance = phys_advance / sc;
        let phys_x = x * sc;
        let phys_y = y * sc;
        let color_a = color.a() as u32;
        if mask.is_none() {
            let pix_w = pixmap.width() as i32;
            let pix_h = pixmap.height() as i32;
            let stride = pix_w as usize;
            let pixels = pixmap.pixels_mut();
            buf.draw(
                &mut self.font_system,
                &mut self.swash_cache,
                color,
                |gx, gy, gw, gh, gc| {
                    let ga = gc.a();
                    if ga == 0 {
                        return;
                    }
                    let eff_a = ga as u32 * color_a / 255;
                    if eff_a == 0 {
                        return;
                    }
                    let bx = phys_x as i32 + gx;
                    let by = phys_y as i32 + gy;
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
                            let px = bx + dx;
                            if px < 0 || px >= pix_w {
                                continue;
                            }
                            let dst = &mut pixels[row + px as usize];
                            let r = (pr + dst.red() as u32 * ia / 255) as u8;
                            let g = (pg + dst.green() as u32 * ia / 255) as u8;
                            let b = (pb + dst.blue() as u32 * ia / 255) as u8;
                            let a = (sa + dst.alpha() as u32 * ia / 255) as u8;
                            if let Some(p) = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, a)
                            {
                                *dst = p;
                            }
                        }
                    }
                },
            );
        } else {
            buf.draw(
                &mut self.font_system,
                &mut self.swash_cache,
                color,
                |gx, gy, gw, gh, gc| {
                    let eff_a = (gc.a() as u32 * color_a / 255) as u8;
                    if eff_a == 0 {
                        return;
                    }
                    if let Some(rect) = SkRect::from_xywh(
                        phys_x + gx as f32,
                        phys_y + gy as f32,
                        gw as f32,
                        gh as f32,
                    ) {
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(gc.r(), gc.g(), gc.b(), eff_a);
                        paint.anti_alias = true;
                        pixmap.fill_rect(rect, &paint, Transform::identity(), mask);
                    }
                },
            );
        }
        self.shape_buf = Some(buf);
        logical_advance
    }

    fn stroke_rect(
        &self,
        pixmap: &mut Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [u8; 4],
        width: f32,
        mask: Option<&Mask>,
    ) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
        let ts = Transform::from_scale(self.scale, self.scale);
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(path) = pb.finish() {
            let mut stroke = Stroke::default();
            stroke.width = width;
            pixmap.stroke_path(&path, &paint, &stroke, ts, mask);
        }
    }

    fn paint_custom_components(
        &self,
        node: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        scale: f32,
    ) {
        // Trait-based component — same coordinate contract as legacy: logical coords, scale passed separately
        if let Some(component) = self.component_registry.get_component(&node.tag) {
            let r = &node.layout.content_rect;
            component.paint(node, pixmap, r.x - sx, r.y - sy, r.w, r.h, scale);
        }
        // Legacy callback-based component
        else if let Some(callbacks) = self.component_registry.map.get(&node.tag) {
            let r = &node.layout.content_rect;
            (callbacks.paint)(node, pixmap, r.x - sx, r.y - sy, r.w, r.h, scale);
        }
        for child in &node.children {
            self.paint_custom_components(child, pixmap, sx, sy, scale);
        }
    }

    fn draw_caret(
        &mut self,
        root: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        caret_node_id: u32,
        caret_local: usize,
    ) {
        self.draw_caret_walk(root, pixmap, sx, sy, caret_node_id, caret_local);
    }

    fn draw_selection_highlight(
        &mut self,
        root: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        caret_node_id: u32,
        sel_start: usize,
        sel_end: usize,
    ) {
        self.draw_selection_highlight_walk(root, pixmap, sx, sy, caret_node_id, sel_start, sel_end);
    }

    fn draw_selection_highlight_walk(
        &mut self,
        node: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        caret_node_id: u32,
        sel_start: usize,
        sel_end: usize,
    ) -> bool {
        if node.node_id == caret_node_id {
            if node.layout.line_cache.is_empty() {
                for child in &node.children {
                    let Some((offset, limit)) =
                        crate::layout::inline_layout::flat_text_child_range(node, child)
                    else {
                        continue;
                    };
                    let len = limit - offset;
                    let start = sel_start.saturating_sub(offset).min(len);
                    let end = sel_end.saturating_sub(offset).min(len);
                    if start < end {
                        self.draw_selection_highlight_walk(
                            child,
                            pixmap,
                            sx,
                            sy,
                            child.node_id,
                            start,
                            end,
                        );
                    }
                }
                return true;
            }
            let flat = collect_flat_text(node);
            if flat.is_empty() {
                return true;
            }
            let mut color = Color::rgba(0, 120, 215, 160);
            let mut foreground = Color::rgb(255, 255, 255);
            if let Some(selection_style) = node.style.selection_style.as_deref() {
                if selection_style.background_color.a > 0 {
                    color = selection_style.background_color;
                }
                foreground = selection_style.color;
            }

            let mut paint = Paint::default();
            paint.set_color(color.to_tiny_skia());
            let transform = Transform::from_scale(self.scale, self.scale);
            let mut selected_runs: Vec<(String, f32, f32)> = Vec::new();
            for line in &node.layout.line_cache {
                let line_start = line.text_start.min(flat.len());
                let line_end = (line.text_start + line.text_length).min(flat.len());
                let start = sel_start.max(line_start).min(line_end);
                let end = sel_end.max(line_start).min(line_end);
                if start >= end {
                    continue;
                }
                let x1 = crate::layout::hit_test::get_caret_x(
                    &flat,
                    &node.layout.inline_runs,
                    line,
                    start,
                ) - sx;
                let x2 = crate::layout::hit_test::get_caret_x(
                    &flat,
                    &node.layout.inline_runs,
                    line,
                    end,
                ) - sx;
                let left = x1.min(x2);
                let width = (x2 - x1).abs().max(1.0);
                if let Some(rect) =
                    SkRect::from_xywh(left, line.y - sy, width, line.height.max(1.0))
                {
                    pixmap.fill_rect(rect, &paint, transform, None);
                }
                let text_start = crate::layout::text::floor_char_boundary(&flat, start);
                let text_end = crate::layout::text::floor_char_boundary(&flat, end);
                if text_start < text_end {
                    selected_runs.push((flat[text_start..text_end].to_string(), left, line.y - sy));
                }
            }

            let font_px = node.style.font_size_px(16.0, 16.0);
            let line_h = node
                .style
                .line_height
                .resolve(font_px, 0.0, 16.0)
                .max(font_px * 1.2);
            let text_color =
                CTextColor::rgba(foreground.r, foreground.g, foreground.b, foreground.a);
            for (text, x, y) in selected_runs {
                self.draw_text_run(
                    &text,
                    x,
                    y,
                    font_px,
                    line_h,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                    text_color,
                    pixmap,
                    None,
                );
            }
            return true;
        }
        for child in &node.children {
            if self.draw_selection_highlight_walk(
                child,
                pixmap,
                sx,
                sy,
                caret_node_id,
                sel_start,
                sel_end,
            ) {
                return true;
            }
        }
        false
    }

    fn draw_caret_walk(
        &mut self,
        node: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        caret_node_id: u32,
        caret_local: usize,
    ) -> bool {
        if node.node_id == caret_node_id {
            let flat = collect_flat_text(node);
            let font_px = node.style.font_size_px(16.0, 16.0);
            let mut caret_x = node.layout.border_rect.x - sx;
            let mut caret_y = node.layout.border_rect.y - sy;
            let mut caret_h = font_px * 1.2;
            let mut found_line = false;
            for line in &node.layout.line_cache {
                let line_end = line.text_start + line.text_length;
                if caret_local >= line.text_start && caret_local <= line_end {
                    caret_y = line.y - sy;
                    caret_h = line.height.max(font_px * 1.0);
                    let cx = crate::layout::hit_test::get_caret_x(
                        &flat,
                        &node.layout.inline_runs,
                        line,
                        caret_local,
                    );
                    caret_x = cx - sx;
                    found_line = true;
                    if caret_local == line.text_start {
                        break;
                    }
                }
            }
            if !found_line && !node.layout.line_cache.is_empty() {
                let last = node.layout.line_cache.last().unwrap();
                caret_y = last.y - sy;
                caret_h = last.height.max(font_px);
                caret_x = last.x - sx + last.width;
            }
            let col = node.style.caret_color.unwrap_or(node.style.color);
            let mut paint = Paint::default();
            paint.set_color(col.to_tiny_skia());
            let mut stroke = Stroke::default();
            stroke.width = 1.5;
            if let Some(path) = line_path(caret_x, caret_y, caret_x, caret_y + caret_h) {
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &stroke,
                    Transform::from_scale(self.scale, self.scale),
                    None,
                );
            }
            return true;
        }
        for child in &node.children {
            if self.draw_caret_walk(child, pixmap, sx, sy, caret_node_id, caret_local) {
                return true;
            }
        }
        false
    }

    /// The open colour picker: the palette, over the page.
    ///
    /// Geometry comes from `Document::picker_rect`, which the hit test also
    /// uses — one source, so a click cannot land on a swatch the paint drew
    /// somewhere else.
    fn draw_color_picker(
        &mut self,
        doc: &crate::types::Document,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
    ) {
        let Some((px, py, pw, ph)) = doc.picker_rect(doc.open_picker) else {
            return;
        };
        let (x, y) = (px - sx, py - sy);
        let ts = tiny_skia::Transform::from_scale(self.scale, self.scale);
        let mut paint = tiny_skia::Paint::default();
        paint.anti_alias = true;

        // A card under the swatches: the popup has to read as a surface of its
        // own, not as colours floating on the page.
        let inset = crate::types::PickerKind::FRAME_INSET_PX;
        paint.set_color_rgba8(250, 250, 250, 255);
        if let Some(r) =
            tiny_skia::Rect::from_xywh(x - inset, y - inset, pw + inset * 2.0, ph + inset * 2.0)
        {
            pixmap.fill_rect(r, &paint, ts, None);
        }
        paint.set_color_rgba8(150, 150, 150, 255);
        if let Some(r) =
            tiny_skia::Rect::from_xywh(x - inset, y - inset, pw + inset * 2.0, ph + inset * 2.0)
        {
            pixmap.stroke_path(
                &tiny_skia::PathBuilder::from_rect(r),
                &paint,
                &tiny_skia::Stroke {
                    width: 1.0,
                    ..Default::default()
                },
                ts,
                None,
            );
        }

        // A calendar is the same popup with a different face: the card above
        // is already drawn, so only the contents differ.
        let calendar_mode = matches!(
            doc.picker_kind(doc.open_picker),
            Some(crate::types::PickerKind::Calendar | crate::types::PickerKind::Month)
        );
        if calendar_mode {
            self.draw_calendar(doc, pixmap, x, y);
        }
        if let Some((tx, ty, _, _)) = doc.time_picker_rect(doc.open_picker) {
            let (x, y) = (tx - sx, ty - sy);
            if let Some(draft) = doc.picker_time.as_ref() {
                let row = crate::widgets::TimePicker::ROW;
                let column_width =
                    crate::widgets::Calendar::width() / crate::widgets::TimePicker::COLUMNS as f32;
                let font_px = row * 0.55;
                let parts = draft.parts();
                for column in 0..crate::widgets::TimePicker::COLUMNS {
                    let left = x + column as f32 * column_width;
                    if column == draft.active {
                        paint.set_color_rgba8(210, 230, 255, 255);
                        if let Some(rect) =
                            SkRect::from_xywh(left, y + row * 2.0, column_width, row)
                        {
                            pixmap.fill_rect(rect, &paint, ts, None);
                        }
                    }
                    let value = if column == 3 {
                        format!("{:03}", parts[column])
                    } else {
                        format!("{:02}", parts[column])
                    };
                    for (index, label) in [
                        crate::widgets::TimePicker::LABELS[column],
                        "+",
                        value.as_str(),
                        "-",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        self.draw_text_run(
                            label,
                            left + column_width * 0.1,
                            y + row * index as f32 + row * 0.2,
                            font_px,
                            row,
                            FontWeight::Normal,
                            FontStyle::Normal,
                            "sans-serif",
                            CTextColor::rgb(30, 30, 30),
                            pixmap,
                            None,
                        );
                    }
                }
                for (index, label) in ["Clear", "Done"].into_iter().enumerate() {
                    let color = if index == 1
                        && !doc
                            .time_picker_value(doc.open_picker)
                            .is_some_and(|value| doc.picker_value_allowed(doc.open_picker, &value))
                    {
                        CTextColor::rgb(140, 140, 140)
                    } else {
                        CTextColor::rgb(30, 30, 30)
                    };
                    self.draw_text_run(
                        label,
                        x + index as f32 * column_width * 2.0 + column_width * 0.1,
                        y + row * 4.2,
                        font_px,
                        row,
                        FontWeight::Normal,
                        FontStyle::Normal,
                        "sans-serif",
                        color,
                        pixmap,
                        None,
                    );
                }
            }
            return;
        }
        if calendar_mode {
            return;
        }

        let cell = crate::widgets::PALETTE_CELL;
        let cols = crate::widgets::PALETTE_COLUMNS;
        for (i, (r, g, b)) in crate::widgets::PALETTE.iter().enumerate() {
            let cx = x + (i % cols) as f32 * cell;
            let cy = y + (i / cols) as f32 * cell;
            paint.set_color_rgba8(*r, *g, *b, 255);
            // Inset by a pixel so each swatch is separated — a grid of touching
            // colours reads as one smear.
            if let Some(rect) =
                tiny_skia::Rect::from_xywh(cx + 1.0, cy + 1.0, cell - 2.0, cell - 2.0)
            {
                pixmap.fill_rect(rect, &paint, ts, None);
            }
        }
    }

    /// The month grid of an open date picker.
    ///
    /// Geometry is `widgets::Calendar`'s, which the hit test also uses — one
    /// source, so a click cannot land on a day the paint drew elsewhere.
    fn draw_calendar(&mut self, doc: &crate::types::Document, pixmap: &mut Pixmap, x: f32, y: f32) {
        const MONTHS: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        // Monday first, matching `first_weekday`'s zero.
        const DAYS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];

        let (year, month, selected) = doc.picker_month(doc.open_picker);
        let first = crate::widgets::first_weekday(year, month);
        let count = crate::widgets::days_in_month(year, month);
        let cell = crate::widgets::Calendar::CELL;
        let font_px = 12.0;

        if let Some(height) = doc.calendar_picker_height(doc.open_picker) {
            self.draw_text_run(
                "Clear",
                x + cell,
                y + height - cell + 4.0,
                font_px,
                font_px,
                crate::types::FontWeight::Normal,
                crate::types::FontStyle::Normal,
                "sans-serif",
                CTextColor::rgba(30, 30, 30, 255),
                pixmap,
                None,
            );
        }

        let month_mode = doc.picker_kind(doc.open_picker) == Some(crate::types::PickerKind::Month);
        let caption = if month_mode {
            year.to_string()
        } else {
            format!("{} {year}", MONTHS[(month as usize - 1).min(11)])
        };
        self.draw_text_run(
            &caption,
            x + cell,
            y + 6.0,
            font_px,
            font_px,
            crate::types::FontWeight::Bold,
            crate::types::FontStyle::Normal,
            "sans-serif",
            CTextColor::rgba(30, 30, 30, 255),
            pixmap,
            None,
        );
        for (label, left) in [
            ("<", x),
            (">", x + crate::widgets::Calendar::width() - cell),
        ] {
            self.draw_text_run(
                label,
                left + cell / 3.0,
                y + 6.0,
                font_px,
                font_px,
                crate::types::FontWeight::Bold,
                crate::types::FontStyle::Normal,
                "sans-serif",
                CTextColor::rgba(30, 30, 30, 255),
                pixmap,
                None,
            );
        }
        if month_mode {
            let selected_month = doc
                .find_webcore(doc.open_picker)
                .map(crate::types::input_value)
                .and_then(|value| {
                    crate::html::temporal::month_parts(&value).map(|(y, m)| (y.to_owned(), m))
                })
                .filter(|(y, _)| y.parse::<i32>().ok() == Some(year))
                .map(|(_, m)| m);
            let width =
                crate::widgets::Calendar::width() / crate::widgets::MonthGrid::COLUMNS as f32;
            let row_height = crate::widgets::MonthGrid::ROW_HEIGHT;
            let ts = tiny_skia::Transform::from_scale(self.scale, self.scale);
            for (i, label) in MONTHS.iter().enumerate() {
                let cx = x + (i % crate::widgets::MonthGrid::COLUMNS) as f32 * width;
                let cy = y
                    + crate::widgets::Calendar::HEADER
                    + (i / crate::widgets::MonthGrid::COLUMNS) as f32 * row_height;
                let allowed =
                    doc.picker_value_allowed(doc.open_picker, &format!("{year:04}-{:02}", i + 1));
                let selected = selected_month == Some(i as u32 + 1);
                if selected {
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(0, 120, 215, 255);
                    if let Some(rect) = tiny_skia::Rect::from_xywh(
                        cx + 1.0,
                        cy + 1.0,
                        width - 2.0,
                        row_height - 2.0,
                    ) {
                        pixmap.fill_rect(rect, &paint, ts, None);
                    }
                }
                self.draw_text_run(
                    &label[..3],
                    cx + 6.0,
                    cy + (row_height - font_px) / 2.0,
                    font_px,
                    font_px,
                    crate::types::FontWeight::Normal,
                    crate::types::FontStyle::Normal,
                    "sans-serif",
                    if selected {
                        CTextColor::rgba(255, 255, 255, 255)
                    } else if !allowed {
                        CTextColor::rgba(150, 150, 150, 255)
                    } else {
                        CTextColor::rgba(30, 30, 30, 255)
                    },
                    pixmap,
                    None,
                );
            }
            return;
        }
        for (i, d) in DAYS.iter().enumerate() {
            self.draw_text_run(
                d,
                x + i as f32 * cell + cell / 2.0 - 3.0,
                y + 24.0,
                font_px,
                font_px,
                crate::types::FontWeight::Normal,
                crate::types::FontStyle::Normal,
                "sans-serif",
                CTextColor::rgba(120, 120, 120, 255),
                pixmap,
                None,
            );
        }

        let ts = tiny_skia::Transform::from_scale(self.scale, self.scale);
        let mut paint = tiny_skia::Paint::default();
        paint.anti_alias = true;
        for day in 1..=count {
            let index = first + day as usize - 1;
            let cx = x + (index % 7) as f32 * cell;
            let cy = y + crate::widgets::Calendar::HEADER + (index / 7) as f32 * cell;
            let candidate = doc.calendar_value(doc.open_picker, year, month, day);
            let allowed = candidate
                .as_ref()
                .is_some_and(|value| doc.picker_value_allowed(doc.open_picker, value));
            let selected_week = doc.find_webcore(doc.open_picker).is_some_and(|node| {
                node.attributes
                    .get("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("week"))
                    && candidate.as_deref() == Some(crate::types::input_value(node).as_str())
            });
            let mut ink = CTextColor::rgba(30, 30, 30, 255);
            if !allowed {
                ink = CTextColor::rgba(150, 150, 150, 255);
            }
            if selected == Some(day) || selected_week {
                paint.set_color_rgba8(0, 120, 215, 255);
                if let Some(r) =
                    tiny_skia::Rect::from_xywh(cx + 1.0, cy + 1.0, cell - 2.0, cell - 2.0)
                {
                    pixmap.fill_rect(r, &paint, ts, None);
                }
                ink = CTextColor::rgba(255, 255, 255, 255);
            }
            let label = day.to_string();
            // Nudged for the width of a two-digit day, so the column reads
            // straight rather than drifting after the 9th.
            let dx = if day < 10 {
                cell / 2.0 - 3.0
            } else {
                cell / 2.0 - 6.0
            };
            self.draw_text_run(
                &label,
                cx + dx,
                cy + 4.0,
                font_px,
                font_px,
                crate::types::FontWeight::Normal,
                crate::types::FontStyle::Normal,
                "sans-serif",
                ink,
                pixmap,
                None,
            );
        }
    }

    fn draw_select_dropdown(
        &mut self,
        node: &WebCore,
        pixmap: &mut Pixmap,
        sx: f32,
        sy: f32,
        hovered_index: i32,
        popup: &crate::types::select_popup::SelectPopup<'_>,
    ) {
        use crate::types::select_popup::SELECT_POPUP_PADDING;
        let rect = Rect::new(
            popup.rect.x - sx,
            popup.rect.y - sy,
            popup.rect.w,
            popup.rect.h,
        );
        let Some(bounds) = SkRect::from_xywh(rect.x, rect.y, rect.w, rect.h) else {
            return;
        };
        let transform = Transform::from_scale(self.scale, self.scale);
        let mut paint = Paint::default();
        let background = if node.style.background_color.a > 0 {
            node.style.background_color
        } else {
            Color::WHITE
        };
        paint.set_color(background.to_tiny_skia());
        pixmap.fill_path(
            &PathBuilder::from_rect(bounds),
            &paint,
            FillRule::Winding,
            transform,
            None,
        );
        self.stroke_rect(
            pixmap,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            [180, 180, 180, 255],
            1.0,
            None,
        );
        let Some(mut clip) = tiny_skia::Mask::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        clip.fill_path(
            &PathBuilder::from_rect(bounds),
            FillRule::Winding,
            false,
            transform,
        );
        let selected = crate::html::forms::selected_index(node);
        for row in &popup.rows {
            let y = rect.y + SELECT_POPUP_PADDING + row.top - popup.scroll;
            if y + row.height <= rect.y || y >= rect.bottom() {
                continue;
            }
            let is_group = row.index.is_none();
            let is_selected = row.index.is_some_and(|i| i as i32 == selected);
            let is_hovered = row.index.is_some_and(|i| i as i32 == hovered_index) && !row.disabled;
            let row_color = if is_selected {
                Color::rgba(66, 133, 244, 255)
            } else if is_hovered {
                Color::rgba(229, 239, 255, 255)
            } else {
                row.node.style.background_color
            };
            paint.set_color(row_color.to_tiny_skia());
            if let Some(row_rect) =
                SkRect::from_xywh(rect.x + 1.0, y, (rect.w - 2.0).max(0.0), row.height)
            {
                pixmap.fill_rect(row_rect, &paint, transform, Some(&clip));
            }
            let text = if is_group {
                row.node
                    .attributes
                    .get("label")
                    .cloned()
                    .unwrap_or_default()
            } else {
                crate::renderer::display_list_builder::option_label(row.node)
            };
            let mut color = if is_selected {
                Color::WHITE
            } else {
                row.node.style.color
            };
            if row.disabled {
                color.a = (color.a as f32 * 0.5) as u8;
            }
            let font_px = popup.font_px;
            self.draw_text_run(
                &text,
                rect.x + SELECT_POPUP_PADDING * 2.0,
                y + (row.height - font_px * 1.2) / 2.0,
                font_px,
                font_px * 1.2,
                if is_group {
                    FontWeight::Bold
                } else {
                    row.node.style.font_weight
                },
                row.node.style.font_style,
                if row.node.style.font_family.is_empty() {
                    &node.style.font_family
                } else {
                    &row.node.style.font_family
                },
                CTextColor::rgba(color.r, color.g, color.b, color.a),
                pixmap,
                Some(&clip),
            );
        }
    }
}

fn configure_generic_font_families(font_system: &mut FontSystem) {
    fn installed_name(db: &fontdb::Database, candidates: &[&str]) -> Option<String> {
        candidates.iter().find_map(|candidate| {
            db.faces()
                .flat_map(|face| face.families.iter())
                .find(|(name, _)| name.eq_ignore_ascii_case(candidate))
                .map(|(name, _)| name.clone())
        })
    }

    let db = font_system.db_mut();
    if db
        .query(&fontdb::Query {
            families: &[cosmic_text::Family::Serif],
            ..fontdb::Query::default()
        })
        .is_none()
    {
        if let Some(name) = installed_name(
            db,
            &[
                "Times New Roman",
                "Georgia",
                "Liberation Serif",
                "Noto Serif",
                "DejaVu Serif",
                "Times",
            ],
        ) {
            db.set_serif_family(name);
        }
    }
    if db
        .query(&fontdb::Query {
            families: &[cosmic_text::Family::SansSerif],
            ..fontdb::Query::default()
        })
        .is_none()
    {
        if let Some(name) = installed_name(
            db,
            &[
                "Arial",
                "Helvetica",
                "Segoe UI",
                "Liberation Sans",
                "Noto Sans",
                "DejaVu Sans",
                "Open Sans",
            ],
        ) {
            db.set_sans_serif_family(name);
        }
    }
    if db
        .query(&fontdb::Query {
            families: &[cosmic_text::Family::Monospace],
            ..fontdb::Query::default()
        })
        .is_none()
    {
        if let Some(name) = installed_name(
            db,
            &[
                "Menlo",
                "Consolas",
                "Liberation Mono",
                "Noto Sans Mono",
                "DejaVu Sans Mono",
                "Courier New",
            ],
        ) {
            db.set_monospace_family(name);
        }
    }
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x, y);
    pb.line_to(x + w, y);
    pb.line_to(x + w, y + h);
    pb.line_to(x, y + h);
    pb.close();
    pb.finish()
}
fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    rounded_rect_path_corners(x, y, w, h, r, r, r, r)
}
fn rounded_rect_path_corners(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tl: f32,
    tr: f32,
    br: f32,
    bl: f32,
) -> Option<tiny_skia::Path> {
    let max_r = (w / 2.0).min(h / 2.0);
    let tl = tl.min(max_r);
    let tr = tr.min(max_r);
    let br = br.min(max_r);
    let bl = bl.min(max_r);
    if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
        return rect_path(x, y, w, h);
    }
    let k = 0.5522848_f32;
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    if tr > 0.0 {
        pb.cubic_to(
            x + w - tr + tr * k,
            y,
            x + w,
            y + tr - tr * k,
            x + w,
            y + tr,
        );
    }
    pb.line_to(x + w, y + h - br);
    if br > 0.0 {
        pb.cubic_to(
            x + w,
            y + h - br + br * k,
            x + w - br + br * k,
            y + h,
            x + w - br,
            y + h,
        );
    }
    pb.line_to(x + bl, y + h);
    if bl > 0.0 {
        pb.cubic_to(
            x + bl - bl * k,
            y + h,
            x,
            y + h - bl + bl * k,
            x,
            y + h - bl,
        );
    }
    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.cubic_to(x, y + tl - tl * k, x + tl - tl * k, y, x + tl, y);
    }
    pb.close();
    pb.finish()
}
fn line_path(x1: f32, y1: f32, x2: f32, y2: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    pb.finish()
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn draw_inspect_overlay(
    node: &WebCore,
    pixmap: &mut Pixmap,
    scroll_x: f32,
    scroll_y: f32,
    scale: f32,
) {
    let fill_rect =
        |pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: u8, g: u8, b: u8, a: u8| {
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, a);
            if let Some(rect) =
                tiny_skia::Rect::from_xywh(x * scale, y * scale, w * scale, h * scale)
            {
                pm.fill_rect(rect, &paint, Transform::identity(), None);
            }
        };
    let m = node.layout.margin_rect;
    let b = node.layout.border_rect;
    let p = node.layout.padding_rect;
    let c = node.layout.content_rect;
    let sx = scroll_x;
    let sy = scroll_y;
    fill_rect(pixmap, m.x - sx, m.y - sy, m.w, b.y - m.y, 255, 152, 0, 80);
    fill_rect(
        pixmap,
        m.x - sx,
        b.y + b.h - sy,
        m.w,
        (m.y + m.h) - (b.y + b.h),
        255,
        152,
        0,
        80,
    );
    fill_rect(pixmap, m.x - sx, b.y - sy, b.x - m.x, b.h, 255, 152, 0, 80);
    fill_rect(
        pixmap,
        b.x + b.w - sx,
        b.y - sy,
        (m.x + m.w) - (b.x + b.w),
        b.h,
        255,
        152,
        0,
        80,
    );
    fill_rect(
        pixmap,
        p.x - sx,
        p.y - sy,
        p.w,
        c.y - p.y,
        128,
        200,
        120,
        80,
    );
    fill_rect(
        pixmap,
        p.x - sx,
        c.y + c.h - sy,
        p.w,
        (p.y + p.h) - (c.y + c.h),
        128,
        200,
        120,
        80,
    );
    fill_rect(
        pixmap,
        p.x - sx,
        c.y - sy,
        c.x - p.x,
        c.h,
        128,
        200,
        120,
        80,
    );
    fill_rect(
        pixmap,
        c.x + c.w - sx,
        c.y - sy,
        (p.x + p.w) - (c.x + c.w),
        c.h,
        128,
        200,
        120,
        80,
    );
    fill_rect(pixmap, c.x - sx, c.y - sy, c.w, c.h, 100, 150, 255, 60);
}
