pub mod compositor;
pub mod display_list;
pub mod display_list_builder;
pub mod display_list_replay;
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
    cached_surface: Option<Pixmap>,
    cached_surface_w: u32,
    cached_surface_h: u32,
    cached_surface_scale: f32,
    cached_surface_zoom: f32,
    cached_surface_scroll_x: f32,
    cached_surface_scroll_y: f32,
    cached_surface_layout_generation: u64,
    cached_surface_hovered_id: u32,
    cached_surface_active_id: u32,
    cached_surface_caret_visible: bool,
    pending_resource_relayout: bool,
    last_resource_relayout: Option<std::time::Instant>,
    last_idle_scroll_x: f32,
    last_idle_scroll_y: f32,
    dirty_paint_rects: Vec<Rect>,
    dropdown_hover_idx: i32,
    pub content_offset_y: f32,
    /// Compositor layer tree — built after layout, used for scroll/transform/opacity.
    pub compositor: compositor::Compositor,
    /// Tile manager — caches rasterized tiles for fast scroll.
    pub tile_manager: tiles::TileManager,
    /// Whether to use tiled rendering (can be disabled for debugging).
    pub use_tiles: bool,
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
    let mut paint = Paint::default();
    paint.set_color(color);
    if let Some(rect) = SkRect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
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

fn animation_override_rects(
    root: &WebCore,
    overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
) -> Vec<Rect> {
    fn walk(
        node: &WebCore,
        overrides: &std::collections::HashMap<u32, Vec<(String, String)>>,
        out: &mut Vec<Rect>,
    ) {
        if let Some(props) = overrides.get(&node.node_id) {
            let mut rect = node.layout.border_rect;
            if rect.w <= 0.0 || rect.h <= 0.0 {
                rect = node.layout.margin_rect;
            }
            if props.iter().any(|(prop, _)| prop == "transform") {
                let pad = rect.w.max(rect.h).max(32.0);
                rect = inflate_rect(rect, pad);
            }
            out.push(rect);
        }
        for child in &node.children {
            walk(child, overrides, out);
        }
    }
    let mut out = Vec::new();
    walk(root, overrides, &mut out);
    out
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
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
            cached_surface: None,
            cached_surface_w: 0,
            cached_surface_h: 0,
            cached_surface_scale: 0.0,
            cached_surface_zoom: 0.0,
            cached_surface_scroll_x: f32::NAN,
            cached_surface_scroll_y: f32::NAN,
            cached_surface_layout_generation: 0,
            cached_surface_hovered_id: 0,
            cached_surface_active_id: 0,
            cached_surface_caret_visible: false,
            pending_resource_relayout: false,
            last_resource_relayout: None,
            last_idle_scroll_x: f32::NAN,
            last_idle_scroll_y: f32::NAN,
            dirty_paint_rects: Vec::new(),
            dropdown_hover_idx: -1,
            content_offset_y: 0.0,
            compositor: compositor::Compositor::new(),
            tile_manager: tiles::TileManager::new(),
            use_tiles: false,
        }
    }

    pub fn invalidate_display_list(&mut self) {
        self.display_list_dirty = true;
        self.cached_paint_top = 0.0;
        self.cached_paint_bottom = 0.0;
        self.cached_surface = None;
        self.dirty_paint_rects.clear();
    }

    fn invalidate_display_list_for_paint_rects<I>(&mut self, rects: I)
    where
        I: IntoIterator<Item = Rect>,
    {
        self.display_list_dirty = true;
        self.invalidate_paint_rects(rects);
    }

    pub fn invalidate_paint_rects<I>(&mut self, rects: I)
    where
        I: IntoIterator<Item = Rect>,
    {
        for rect in rects {
            if rect.w > 0.0 && rect.h > 0.0 {
                self.dirty_paint_rects.push(inflate_rect(rect, 8.0));
            }
        }
        if self.dirty_paint_rects.is_empty() {
            return;
        }
        if !self.dirty_paint_rects.is_empty() {
            let viewport = Rect::new(
                self.cached_scroll_x,
                self.cached_scroll_y,
                self.cached_surface_w as f32 / self.cached_surface_scale.max(0.001),
                self.cached_surface_h as f32 / self.cached_surface_scale.max(0.001),
            );
            self.dirty_paint_rects =
                coalesce_dirty_rects(std::mem::take(&mut self.dirty_paint_rects), viewport);
        }
        if self.dirty_paint_rects.is_empty() {
            return;
        }
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

        if doc.editor.has_focus && doc.editor.blink_update() {
            needs_redraw = true;
            if trace_idle {
                trace_reasons.push("editor-blink");
            }
        }
        if scroll_changed && trace_idle {
            trace_reasons.push("scroll-priority");
        }
        if doc.poll_pending_stylesheets() {
            self.layout_engine().invalidate_cascade();
            resource_requested_relayout = true;
            if trace_idle {
                trace_reasons.push("stylesheet");
            }
        }
        let image_poll = doc.poll_pending_images_budgeted(32, std::time::Duration::from_millis(8));
        if image_poll.loaded_any {
            resource_requested_relayout |= image_poll.needs_relayout;
            if image_poll.needs_relayout {
                // Intrinsic size changes are folded into the coalesced
                // resource relayout below. Do not throw away the display
                // list here for every arriving image.
            } else {
                self.invalidate_paint_rects(image_poll.paint_rects.clone());
            }
            needs_redraw = true;
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
            if !pending_resources || elapsed >= std::time::Duration::from_millis(80) {
                needs_relayout = true;
                self.pending_resource_relayout = false;
                self.last_resource_relayout = Some(now);
                if trace_idle {
                    trace_reasons.push("resource-layout");
                }
            }
        }
        if !scroll_changed && doc.needs_animation_frame && !needs_relayout {
            doc.tick_animations(now);
            let css_animations_running = doc.needs_animation_frame;
            let svg_animations_running = crate::svg::tick_svg_animations(&mut doc.root, now);
            if svg_animations_running {
                doc.needs_animation_frame = true;
            }
            let animation_needs_layout = doc.animation_overrides.values().any(|props| {
                crate::types::animation_runtime::animation_properties_affect_layout(props)
            });
            if animation_needs_layout && !doc.animation_overrides.is_empty() {
                let overrides = doc.animation_overrides.clone();
                crate::css::apply_animation_overrides(&mut doc.root, &overrides);
                self.invalidate_display_list();
                needs_redraw = true;
                if trace_idle {
                    trace_reasons.push("css-animation-layout");
                }
            } else if !doc.animation_overrides.is_empty() {
                // Paint-only animations do not affect geometry, but they still
                // must present a new frame. Skeleton loaders commonly animate
                // `background-position` over a gradient. Rebuild the viewport
                // display list with the new sampled style, but keep the old
                // surface and repaint only the animated boxes.
                let viewport = Rect::new(doc.scroll_x, doc.scroll_y, viewport_w, viewport_h);
                let paint_rects = animation_override_rects(&doc.root, &doc.animation_overrides)
                    .into_iter()
                    .filter(|rect| rect_intersects(*rect, viewport))
                    .collect::<Vec<_>>();
                let overrides = doc.animation_overrides.clone();
                crate::css::apply_animation_overrides(&mut doc.root, &overrides);
                if !paint_rects.is_empty() {
                    self.invalidate_display_list_for_paint_rects(paint_rects);
                    needs_redraw = true;
                }
                doc.needs_animation_frame = css_animations_running || svg_animations_running;
                if trace_idle {
                    trace_reasons.push("css-animation-paint");
                }
            }
            if animation_needs_layout {
                needs_relayout = true;
                self.invalidate_display_list();
            }
        } else if !scroll_changed && doc.needs_animation_frame {
            self.invalidate_display_list();
            if trace_idle {
                trace_reasons.push("animation-deferred");
            }
        }
        if !scroll_changed && doc.hover_changed {
            let hover_needs_style = doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc.hover_sensitive_nodes.contains(&doc.prev_hovered_box);
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
                if image_tick.paint_rects.is_empty() {
                    self.invalidate_display_list();
                } else {
                    self.invalidate_paint_rects(image_tick.paint_rects);
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
        let has_timed_work = doc.editor.has_focus
            || doc.needs_animation_frame
            || has_visible_animated_images
            || doc.pending_images.is_some()
            || doc.pending_stylesheets.is_some()
            || self.layout_engine().has_pending_fonts()
            || self.pending_resource_relayout;

        if has_timed_work {
            let mut deadline = if doc.needs_animation_frame {
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
            if doc.editor.has_focus {
                deadline = deadline.min(doc.editor.next_blink_deadline());
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
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
                    let hit_id = crate::layout::hit_test::point_to_hit(&doc.root, doc_pt, 0)
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
        let doc_h = crate::types::Document::scroll_height(&doc.root);
        let doc_w = doc.root.layout.margin_rect.w;
        doc.scroll_y = doc.scroll_y.max(0.0).min((doc_h - view_h).max(0.0));
        doc.scroll_x = doc.scroll_x.max(0.0).min((doc_w - view_w).max(0.0));
        let surface_hover_changed = self.cached_surface_hovered_id != doc.hovered_box
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc
                    .hover_sensitive_nodes
                    .contains(&self.cached_surface_hovered_id));
        let can_reuse_surface = self.cached_surface.as_ref().is_some_and(|surface| {
            surface.width() == pixmap.width()
                && surface.height() == pixmap.height()
                && (self.cached_surface_scale - scale).abs() < 0.001
                && (self.cached_surface_zoom - zoom).abs() < 0.001
                && (self.cached_surface_scroll_x - doc.scroll_x).abs() < 0.5
                && (self.cached_surface_scroll_y - doc.scroll_y).abs() < 0.5
                && self.cached_surface_layout_generation == doc.layout_generation
                && !surface_hover_changed
                && self.cached_surface_active_id == doc.active_box
                && self.cached_surface_caret_visible == doc.editor.caret_visible
                && !self.display_list_dirty
        });
        if can_reuse_surface {
            if let Some(surface) = self.cached_surface.as_ref() {
                pixmap.data_mut().copy_from_slice(surface.data());
                if trace_render {
                    eprintln!(
                        "[webcore render] total={}ms cached-surface=true scroll=({:.1},{:.1})",
                        render_start.elapsed().as_millis(),
                        doc.scroll_x,
                        doc.scroll_y,
                    );
                }
                return;
            }
        }
        let canvas_color = doc
            .root
            .children
            .iter()
            .find(|c| c.tag == "body")
            .map(|body| body.style.background_color)
            .filter(|c| c.a > 0)
            .or_else(|| {
                let c = doc.root.style.background_color;
                if c.a > 0 { Some(c) } else { None }
            })
            .map(|c| c.to_tiny_skia())
            .unwrap_or(tiny_skia::Color::WHITE);

        // Keep a deep retained paint band around the viewport. A shallow band
        // made ordinary wheel/scrollbar movement fall out of the cached display
        // list almost immediately, so scrolling paid a full record/replay pass
        // instead of shifting the existing backing surface and painting only
        // the newly exposed strip.
        let paint_overscan = (view_h * 8.0).max(6000.0);
        let paint_top = (doc.scroll_y - paint_overscan).max(0.0);
        let paint_bottom = (doc.scroll_y + view_h + paint_overscan).min(doc_h.max(view_h));

        // Check what changed since last render
        let layout_changed = doc.layout_generation != self.cached_layout_generation;
        let hover_changed = doc.hovered_box != self.cached_hovered_id
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc.hover_sensitive_nodes.contains(&self.cached_hovered_id));
        let scroll_outside_cached_band = self.cached_display_list.is_none()
            || doc.scroll_y < self.cached_paint_top
            || doc.scroll_y + view_h > self.cached_paint_bottom;
        let _scroll_only = !layout_changed
            && !hover_changed
            && !self.display_list_dirty
            && !scroll_outside_cached_band
            && self.cached_display_list.is_some();

        let dirty_paint_rects = self.dirty_paint_rects.clone();
        let dirty_paint_only = !dirty_paint_rects.is_empty()
            && !layout_changed
            && !hover_changed
            && !scroll_outside_cached_band
            && self.cached_display_list.is_some()
            && self.cached_surface.as_ref().is_some_and(|surface| {
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
        let needs_rebuild = self.display_list_dirty
            || self.cached_display_list.is_none()
            || layout_changed
            || hover_changed
            || scroll_outside_cached_band;
        let scroll_band_rebuild_only = scroll_outside_cached_band
            && !self.display_list_dirty
            && !layout_changed
            && !hover_changed
            && self.cached_display_list.is_some()
            && self.cached_surface.is_some();

        if needs_rebuild {
            let build_start = std::time::Instant::now();
            // Build only a generous viewport band, still in document
            // coordinates. Scrolling inside the band reuses the same list; when
            // the viewport leaves the band we rebuild a new slice instead of
            // traversing and recording the whole page.
            let list = display_list_builder::build_display_list_viewport(
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
            );
            self.cached_display_list = Some(list);
            self.cached_paint_top = paint_top;
            self.cached_paint_bottom = paint_bottom;
            self.cached_hovered_id = doc.hovered_box;
            self.cached_layout_generation = doc.layout_generation;
            self.display_list_dirty = false;
            self.tile_manager.invalidate_all();

            // Rebuild compositor layer tree on layout change
            if layout_changed {
                self.compositor.build_layers(&doc.root, view_w, view_h);
            }
            build_ms = build_start.elapsed().as_millis();
            rebuilt_display_list = true;
        }
        self.cached_scroll_x = doc.scroll_x;
        self.cached_scroll_y = doc.scroll_y;

        // Replay display list (cached — only rebuilt on layout/hover change)
        let mut used_scroll_surface = false;
        let mut used_dirty_surface = false;
        if dirty_paint_only {
            if let (Some(surface), Some(list)) = (
                self.cached_surface.as_ref(),
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
                        }
                    }
                    replay_ms = replay_start.elapsed().as_millis();
                    used_dirty_surface = true;
                    self.display_list_dirty = false;
                    self.dirty_paint_rects.clear();
                }
            }
        }
        if !used_dirty_surface && (!needs_rebuild || scroll_band_rebuild_only) {
            if let (Some(surface), Some(list)) = (
                self.cached_surface.as_ref(),
                self.cached_display_list.as_ref(),
            ) {
                let tile_scale = scale * zoom;
                let dx_px = ((doc.scroll_x - self.cached_surface_scroll_x) * tile_scale).round();
                let dy_px = ((doc.scroll_y - self.cached_surface_scroll_y) * tile_scale).round();
                let scroll_surface_reject =
                    if surface.width() != pixmap.width() || surface.height() != pixmap.height() {
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
                    } else if self.cached_surface_active_id != doc.active_box {
                        Some("active")
                    } else if self.cached_surface_caret_visible != doc.editor.caret_visible {
                        Some("caret-visibility")
                    } else if !self.component_registry.map.is_empty()
                        || !self.component_registry.components.is_empty()
                    {
                        Some("custom-components")
                    } else if doc.open_select != 0 || doc.open_picker != 0 {
                        Some("popup")
                    } else if doc.editor.has_selection() || doc.editor.caret_visible {
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

        if !used_dirty_surface && !used_scroll_surface {
            pixmap.fill(canvas_color);
        }
        if !used_dirty_surface
            && !used_scroll_surface
            && let Some(ref list) = self.cached_display_list
        {
            let replay_start = std::time::Instant::now();
            if self.use_tiles {
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
                        if let Some(tile) = self.tile_manager.tiles.get_mut(&(tx, ty)) {
                            tile.pixmap.fill(canvas_color);
                            let tile_scroll_x = tx as f32 * tiles::TILE_SIZE;
                            let tile_scroll_y = ty as f32 * tiles::TILE_SIZE;
                            display_list_replay::replay_with_scroll(
                                list,
                                &mut tile.pixmap,
                                tile_scale,
                                &mut self.font_system,
                                &mut self.swash_cache,
                                tile_scroll_x,
                                tile_scroll_y,
                            );
                            tile.dirty = false;
                        }
                    }
                }
                self.tile_manager.evict_distant();
                self.tile_manager
                    .composite_to(pixmap, doc.scroll_x, doc.scroll_y, tile_scale);
            } else {
                // The scroll offset is applied HERE, at replay, which is what makes
                // one cached list serve every scroll position.
                display_list_replay::replay_with_scroll(
                    list,
                    pixmap,
                    scale * zoom,
                    &mut self.font_system,
                    &mut self.swash_cache,
                    doc.scroll_x,
                    doc.scroll_y,
                );
            }
            // `position: fixed` content, at scroll 0 — it does not move.
            if !list.fixed_commands.is_empty() {
                let fixed = crate::renderer::display_list::DisplayList {
                    commands: list.fixed_commands.clone(),
                    fixed_commands: Vec::new(),
                };
                display_list_replay::replay_with_scroll(
                    &fixed,
                    pixmap,
                    scale * zoom,
                    &mut self.font_system,
                    &mut self.swash_cache,
                    0.0,
                    0.0,
                );
            }
            replay_ms = replay_start.elapsed().as_millis();
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
                self.draw_select_dropdown(sel_node, pixmap, doc.scroll_x, doc.scroll_y);
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
            if let Some((caret_id, _)) = doc.editor.caret_info() {
                if crate::dom::is_in_contenteditable_by_id(&doc.root, caret_id) {
                    self.scale = scale * zoom;
                    self.draw_selection_highlight(
                        &doc.root,
                        pixmap,
                        doc.scroll_x,
                        doc.scroll_y,
                        caret_id,
                        doc.editor.sel_start,
                        doc.editor.sel_end,
                    );
                }
            }
        }
        if doc.editor.caret_visible {
            if let Some((caret_id, caret_local)) = doc.editor.caret_info() {
                if crate::dom::is_in_contenteditable_by_id(&doc.root, caret_id) {
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
            let thumb_h = (track_h * view_h / doc_h).max(20.0);
            let max_s = doc_h - view_h;
            let thumb_y = if max_s > 0.0 {
                doc.scroll_y * (track_h - thumb_h) / max_s
            } else {
                0.0
            };
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
        if self.cached_surface.as_ref().is_none_or(|surface| {
            surface.width() != pixmap.width() || surface.height() != pixmap.height()
        }) {
            self.cached_surface = Pixmap::new(pixmap.width(), pixmap.height());
        }
        if let Some(surface) = self.cached_surface.as_mut() {
            surface.data_mut().copy_from_slice(pixmap.data());
            self.cached_surface_w = pixmap.width();
            self.cached_surface_h = pixmap.height();
            self.cached_surface_scale = scale;
            self.cached_surface_zoom = zoom;
            self.cached_surface_scroll_x = doc.scroll_x;
            self.cached_surface_scroll_y = doc.scroll_y;
            self.cached_surface_layout_generation = doc.layout_generation;
            self.cached_surface_hovered_id = doc.hovered_box;
            self.cached_surface_active_id = doc.active_box;
            self.cached_surface_caret_visible = doc.editor.caret_visible;
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
        paint.set_color_rgba8(250, 250, 250, 255);
        if let Some(r) = tiny_skia::Rect::from_xywh(x - 4.0, y - 4.0, pw + 8.0, ph + 8.0) {
            pixmap.fill_rect(r, &paint, ts, None);
        }
        paint.set_color_rgba8(150, 150, 150, 255);
        if let Some(r) = tiny_skia::Rect::from_xywh(x - 4.0, y - 4.0, pw + 8.0, ph + 8.0) {
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
        if matches!(
            doc.picker_kind(doc.open_picker),
            Some(crate::types::PickerKind::Calendar)
        ) {
            self.draw_calendar(doc, pixmap, x, y);
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

        let caption = format!("{} {year}", MONTHS[(month as usize - 1).min(11)]);
        self.draw_text_run(
            &caption,
            x + 6.0,
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
            let mut ink = CTextColor::rgba(30, 30, 30, 255);
            if selected == Some(day) {
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

    fn draw_select_dropdown(&mut self, node: &WebCore, pixmap: &mut Pixmap, sx: f32, sy: f32) {
        let br = node.layout.border_rect;
        let popup_x = br.x - sx;
        let popup_y = br.y + br.h - sy;
        let popup_w = br.w.max(150.0);
        // Which row the open popup highlights: SELECTEDNESS, the same state the
        // pick writes.
        //
        // ⛔ `Option`, not a sentinel. `usize::MAX` is already taken — it is
        // what an OPTGROUP HEADER carries as its index below — so spelling
        // "nothing selected" that way would make every group header compare
        // equal to the selection.
        let selected_idx: Option<usize> = match crate::html::forms::selected_index(node) {
            i if i >= 0 => Some(i as usize),
            _ => None,
        };
        struct DropdownItem<'a> {
            node: &'a WebCore,
            is_group: bool,
            text: String,
            index: usize,
        }
        let mut items: Vec<DropdownItem> = Vec::new();
        let mut opt_idx = 0usize;
        for child in &node.children {
            if child.tag == "option" {
                // The option's LABEL, which HTML §4.11.3.5 defines over DESCENDANT text —
                // so a label wrapped in an element still reads. Collecting direct
                // `#text` children only is what made those entries blank.
                let text: String = crate::renderer::display_list_builder::option_label(child);
                items.push(DropdownItem {
                    node: child,
                    is_group: false,
                    text: text.trim().to_string(),
                    index: opt_idx,
                });
                opt_idx += 1;
            } else if child.tag == "optgroup" {
                let label = child.attributes.get("label").cloned().unwrap_or_default();
                items.push(DropdownItem {
                    node: child,
                    is_group: true,
                    text: label,
                    index: usize::MAX,
                });
                for gc in &child.children {
                    if gc.tag == "option" {
                        let text: String = crate::renderer::display_list_builder::option_label(gc);
                        items.push(DropdownItem {
                            node: gc,
                            is_group: false,
                            text: text.trim().to_string(),
                            index: opt_idx,
                        });
                        opt_idx += 1;
                    }
                }
            }
        }
        if items.is_empty() {
            return;
        }
        let font_px = node.style.font_size_px(16.0, 16.0);
        let item_h = font_px * 1.8;
        let group_h = font_px * 1.5;
        let padding = 4.0;
        let total_h: f32 = items
            .iter()
            .map(|i| if i.is_group { group_h } else { item_h })
            .sum::<f32>()
            + padding * 2.0;
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 50);
        if let Some(r) = tiny_skia::Rect::from_xywh(
            (popup_x + 3.0) * self.scale,
            (popup_y + 3.0) * self.scale,
            popup_w * self.scale,
            total_h * self.scale,
        ) {
            pixmap.fill_rect(r, &paint, Transform::identity(), None);
        }
        paint.set_color_rgba8(255, 255, 255, 252);
        if let Some(r) = tiny_skia::Rect::from_xywh(
            popup_x * self.scale,
            popup_y * self.scale,
            popup_w * self.scale,
            total_h * self.scale,
        ) {
            pixmap.fill_rect(r, &paint, Transform::identity(), None);
        }
        self.stroke_rect(
            pixmap,
            popup_x,
            popup_y,
            popup_w,
            total_h,
            [180, 180, 180, 255],
            1.0,
            None,
        );
        let mut y = popup_y + padding;
        for item in &items {
            if item.is_group {
                paint.set_color_rgba8(245, 245, 245, 255);
                if let Some(r) = tiny_skia::Rect::from_xywh(
                    (popup_x + 1.0) * self.scale,
                    y * self.scale,
                    (popup_w - 2.0) * self.scale,
                    group_h * self.scale,
                ) {
                    pixmap.fill_rect(r, &paint, Transform::identity(), None);
                }
                let label_y = y + (group_h - font_px * 1.2) / 2.0;
                self.draw_text_run(
                    &item.text,
                    popup_x + 8.0,
                    label_y,
                    font_px * 0.85,
                    font_px,
                    crate::types::FontWeight::Bold,
                    node.style.font_style,
                    &node.style.font_family,
                    CTextColor::rgba(100, 100, 100, 255),
                    pixmap,
                    None,
                );
                y += group_h;
            } else {
                let is_selected = selected_idx == Some(item.index);
                let is_hovered = item.index as i32 == self.dropdown_hover_idx;
                let opt_bg = item.node.style.background_color;
                let opt_color = item.node.style.color;
                if is_selected {
                    paint.set_color_rgba8(66, 133, 244, 255);
                    if let Some(r) = tiny_skia::Rect::from_xywh(
                        (popup_x + 1.0) * self.scale,
                        y * self.scale,
                        (popup_w - 2.0) * self.scale,
                        item_h * self.scale,
                    ) {
                        pixmap.fill_rect(r, &paint, Transform::identity(), None);
                    }
                } else if is_hovered {
                    paint.set_color_rgba8(229, 239, 255, 255);
                    if let Some(r) = tiny_skia::Rect::from_xywh(
                        (popup_x + 1.0) * self.scale,
                        y * self.scale,
                        (popup_w - 2.0) * self.scale,
                        item_h * self.scale,
                    ) {
                        pixmap.fill_rect(r, &paint, Transform::identity(), None);
                    }
                } else if opt_bg.a > 0 {
                    paint.set_color_rgba8(opt_bg.r, opt_bg.g, opt_bg.b, opt_bg.a);
                    if let Some(r) = tiny_skia::Rect::from_xywh(
                        (popup_x + 1.0) * self.scale,
                        y * self.scale,
                        (popup_w - 2.0) * self.scale,
                        item_h * self.scale,
                    ) {
                        pixmap.fill_rect(r, &paint, Transform::identity(), None);
                    }
                }
                let text_color = if is_selected {
                    CTextColor::rgba(255, 255, 255, 255)
                } else if opt_bg.a > 0 {
                    CTextColor::rgba(opt_color.r, opt_color.g, opt_color.b, opt_color.a)
                } else {
                    CTextColor::rgba(33, 33, 33, 255)
                };
                let text_y = y + (item_h - font_px * 1.2) / 2.0;
                self.draw_text_run(
                    &item.text,
                    popup_x + 8.0,
                    text_y,
                    font_px,
                    font_px * 1.2,
                    item.node.style.font_weight,
                    item.node.style.font_style,
                    if item.node.style.font_family.is_empty() {
                        &node.style.font_family
                    } else {
                        &item.node.style.font_family
                    },
                    text_color,
                    pixmap,
                    None,
                );
                y += item_h;
            }
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
