//! Scrollbar dragging and wheel scrolling.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

impl Document {
    pub(crate) fn resize_grip_at(&self, screen_x: f32, screen_y: f32) -> Option<ResizeDrag> {
        let doc_pt = (screen_x, screen_y + self.scroll_y);
        let mut node_id = crate::layout::hit_test::hit_test_box_at(&self.root, doc_pt, 0);
        while node_id != 0 {
            let node = self.get_node(node_id)?;
            let rect = node.layout.border_rect;
            let grip = CSS_RESIZE_GRIP_PX.min(rect.w).min(rect.h);
            if node.style.resize != crate::types::Resize::None
                && !matches!(
                    (node.style.overflow_x, node.style.overflow_y),
                    (Overflow::Visible, Overflow::Visible)
                )
                && doc_pt.0 >= rect.x + rect.w - grip
                && doc_pt.0 < rect.x + rect.w
                && doc_pt.1 >= rect.y + rect.h - grip
                && doc_pt.1 < rect.y + rect.h
            {
                let size = if node.style.box_sizing == BoxSizing::BorderBox {
                    (rect.w, rect.h)
                } else {
                    (node.layout.content_rect.w, node.layout.content_rect.h)
                };
                return Some(ResizeDrag {
                    node_id,
                    axes: node.style.resize.axes(node.style.writing_mode),
                    start_mouse: (screen_x, screen_y),
                    start_size: size,
                });
            }
            node_id = find_parent_node_id_by_id(&self.root, node_id);
        }
        None
    }

    /// Handle a mouse event for scrollbars (click, drag, release).
    ///
    /// Call this **before** `process_mouse_event` on every mouse down/move/up.
    /// Coordinates are in **screen-space logical pixels** (physical / scale),
    /// i.e. *without* any scroll offset added — the same values you get from
    /// `(position.x as f32 / scale, position.y as f32 / scale)`.
    ///
    /// `viewport_w` and `viewport_h` are the logical viewport dimensions.
    /// Returns `true` if the event was consumed by a scrollbar (no further
    /// processing needed).
    pub fn process_scrollbar_event(
        &mut self,
        etype: crate::dom::HtmlEventType,
        screen_x: f32,
        screen_y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        use crate::dom::HtmlEventType::*;
        let sbw = self.root.style.scrollbar_width_px();

        if let Some(drag) = self.resize_drag.clone() {
            match etype {
                MouseMove | MouseUp => {
                    let delta_x = screen_x - drag.start_mouse.0;
                    let delta_y = screen_y - drag.start_mouse.1;
                    if drag.axes.0 {
                        let width = (drag.start_size.0 + delta_x).max(0.0);
                        self.set_style_property(drag.node_id, "width", &format!("{width}px"));
                    }
                    if drag.axes.1 {
                        let height = (drag.start_size.1 + delta_y).max(0.0);
                        self.set_style_property(drag.node_id, "height", &format!("{height}px"));
                    }
                    if etype == MouseUp {
                        self.resize_drag = None;
                    }
                    return true;
                }
                _ => {}
            }
        }

        if etype == MouseDown {
            if let Some(drag) = self.resize_grip_at(screen_x, screen_y) {
                self.resize_drag = Some(drag);
                return true;
            }
        }

        match etype {
            // ── MouseMove: continue drag ──────────────────────────────────────
            MouseMove => {
                if let Some(drag) = self.scrollbar_drag.clone() {
                    let dy = screen_y - drag.start_mouse_y;
                    let mut scrolled_element = None;
                    match drag.kind {
                        ScrollbarDragKind::Viewport => {
                            let old_y = self.scroll_y;
                            let new_scroll = (drag.start_scroll + dy * drag.scroll_per_px).max(0.0);
                            let doc_h = self.cached_scroll_height();
                            let max_s = (doc_h - viewport_h).max(0.0);
                            self.scroll_y = new_scroll.min(max_s);
                            if (self.scroll_y - old_y).abs() > 0.01 {
                                self.fire_window_event("scroll");
                            }
                        }
                        ScrollbarDragKind::ElementVertical(nid) => {
                            let new_scroll = (drag.start_scroll + dy * drag.scroll_per_px).max(0.0);
                            if let Some(node) = self.get_box_by_id_mut(nid) {
                                let old_y = node.layout.scroll_top;
                                let max_s = (node.layout.scroll_height
                                    - node.layout.content_rect.h)
                                    .max(0.0);
                                node.layout.scroll_top = new_scroll.min(max_s);
                                if (node.layout.scroll_top - old_y).abs() > 0.01 {
                                    scrolled_element = Some(nid);
                                }
                            }
                        }
                        ScrollbarDragKind::ElementHorizontal(nid) => {
                            let dx = screen_x - drag.start_mouse_x;
                            let new_scroll = (drag.start_scroll + dx * drag.scroll_per_px).max(0.0);
                            if let Some(node) = self.get_box_by_id_mut(nid) {
                                let old_x = node.layout.scroll_left;
                                let max_s = (node.layout.scroll_width - node.layout.content_rect.w)
                                    .max(0.0);
                                node.layout.scroll_left = new_scroll.min(max_s);
                                if (node.layout.scroll_left - old_x).abs() > 0.01 {
                                    scrolled_element = Some(nid);
                                }
                            }
                        }
                    }
                    if let Some(nid) = scrolled_element {
                        let mut event = crate::dom::events::DomEvent::new("scroll", nid);
                        self.dispatch_dom_event(&mut event);
                    }
                    return true;
                }
                false
            }

            // ── MouseUp: end drag ─────────────────────────────────────────────
            MouseUp => {
                let Some(drag) = self.scrollbar_drag.take() else {
                    return false;
                };
                let context = ScrollSnapContext {
                    viewport_w: self.viewport_w,
                    viewport_h: self.viewport_h,
                    root_font_px: self.root_font_px(),
                };
                let snapped_element = match drag.kind {
                    ScrollbarDragKind::ElementVertical(nid) => {
                        self.get_box_by_id_mut(nid).and_then(|node| {
                            let before = node.layout.scroll_top;
                            apply_scroll_snap_y(node, before, context);
                            ((node.layout.scroll_top - before).abs() > 0.01).then_some(nid)
                        })
                    }
                    ScrollbarDragKind::ElementHorizontal(nid) => {
                        self.get_box_by_id_mut(nid).and_then(|node| {
                            let before = node.layout.scroll_left;
                            apply_scroll_snap_x(node, before, context);
                            ((node.layout.scroll_left - before).abs() > 0.01).then_some(nid)
                        })
                    }
                    ScrollbarDragKind::Viewport => {
                        let before = self.scroll_y;
                        let doc_h = self.cached_scroll_height();
                        self.scroll_y = snapped_viewport_scroll_y(
                            &self.root,
                            before,
                            before,
                            doc_h,
                            self.scroll_x,
                            context,
                        );
                        if (self.scroll_y - before).abs() > 0.01 {
                            self.fire_window_event("scroll");
                        }
                        None
                    }
                };
                if let Some(nid) = snapped_element {
                    let mut event = crate::dom::events::DomEvent::new("scroll", nid);
                    self.dispatch_dom_event(&mut event);
                }
                true
            }

            // ── MouseDown: hit-test scrollbars, start drag ────────────────────
            MouseDown => {
                // Viewport scrollbar — right edge of window.
                let doc_h = self.cached_scroll_height();
                if doc_h > viewport_h
                    && sbw > 0.0
                    && screen_x >= viewport_w - sbw
                    && screen_x < viewport_w
                    && screen_y >= 0.0
                    && screen_y < viewport_h
                {
                    let track_h = viewport_h;
                    let (thumb_h, thumb_y, scale) = super::scrollbar_hit::viewport_scrollbar_thumb(
                        track_h,
                        viewport_h,
                        doc_h,
                        self.scroll_y,
                    );
                    let max_s = (doc_h - viewport_h).max(0.0);

                    // Click in track but outside thumb → jump to that position.
                    if !(screen_y >= thumb_y && screen_y < thumb_y + thumb_h) {
                        let before = self.scroll_y;
                        let new_thumb_y =
                            (screen_y - thumb_h * 0.5).clamp(0.0, (track_h - thumb_h).max(0.0));
                        self.scroll_y = (new_thumb_y * scale).min(max_s).max(0.0);
                        if (self.scroll_y - before).abs() > 0.01 {
                            self.fire_window_event("scroll");
                        }
                    }
                    self.scrollbar_drag = Some(ScrollbarDrag {
                        kind: ScrollbarDragKind::Viewport,
                        start_mouse_x: screen_x,
                        start_mouse_y: screen_y,
                        start_scroll: self.scroll_y,
                        scroll_per_px: scale,
                    });
                    return true;
                }

                // Per-element scrollbars — walk tree looking for scrollbar hit.
                // We pass accumulated offsets (sx, sy) matching the renderer.
                let sy = self.scroll_y;
                let sx = self.scroll_x;
                let before = collect_element_scroll_offsets(&self.root);
                if scrollbar_hit_test(
                    &mut self.root,
                    screen_x,
                    screen_y,
                    sx,
                    sy,
                    sbw,
                    &mut self.scrollbar_drag,
                ) {
                    self.dispatch_element_scroll_changes(&before);
                    return true;
                }

                false
            }

            _ => false,
        }
    }

    /// Handle a wheel/scroll event.
    ///
    /// `doc_pt` is the cursor position in document coordinates.
    /// `delta_y` is the vertical scroll amount in logical pixels (negative = scroll down,
    /// positive = scroll up).  Horizontal scroll is handled internally by the renderer via
    /// `process_wheel_event_xy`.
    ///
    /// Finds the innermost scrollable box under the cursor and scrolls it.
    /// Respects `overscroll-behavior` to control scroll chaining.
    /// Returns `true` if any scroll position changed.
    pub fn process_wheel_event(&mut self, doc_pt: (f32, f32), delta_y: f32) -> bool {
        self.process_wheel_event_xy(doc_pt, 0.0, delta_y)
    }

    /// Like `process_wheel_event` but also accepts a horizontal delta.
    /// Used by the renderer when handling trackpad/horizontal wheel events.
    pub fn process_wheel_event_xy(
        &mut self,
        doc_pt: (f32, f32),
        delta_x: f32,
        delta_y: f32,
    ) -> bool {
        let snap_context = ScrollSnapContext {
            viewport_w: self.viewport_w,
            viewport_h: self.viewport_h,
            root_font_px: self.root_font_px(),
        };
        match scroll_box_at(&mut self.root, doc_pt, delta_x, delta_y, snap_context) {
            WheelScrollResult::Scrolled(node_id) => {
                if node_id != 0 {
                    let mut event = crate::dom::events::DomEvent::new("scroll", node_id);
                    self.dispatch_dom_event(&mut event);
                }
                return true;
            }
            WheelScrollResult::Blocked => return false,
            WheelScrollResult::None => {}
        }
        self.scroll_viewport_by_user(-delta_x, -delta_y, self.viewport_w, self.viewport_h)
    }

    pub(crate) fn scroll_viewport_by_user(
        &mut self,
        dx: f32,
        dy: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        let old_x = self.scroll_x;
        let old_y = self.scroll_y;
        let context = ScrollSnapContext {
            viewport_w,
            viewport_h,
            root_font_px: self.root_font_px(),
        };
        let next_x = if dx.abs() > 0.01 {
            let doc_w = self.cached_scroll_width();
            let candidate = (old_x + dx).clamp(0.0, (doc_w - viewport_w).max(0.0));
            if (candidate - old_x).abs() > 0.01 {
                snapped_viewport_scroll_x(&self.root, old_x, candidate, doc_w, old_y, context)
            } else {
                old_x
            }
        } else {
            old_x
        };
        let next_y = if dy.abs() > 0.01 && !self.viewport_y_scroll_locked() {
            let doc_h = self.cached_scroll_height();
            let candidate = (old_y + dy).clamp(0.0, (doc_h - viewport_h).max(0.0));
            if (candidate - old_y).abs() > 0.01 {
                snapped_viewport_scroll_y(&self.root, old_y, candidate, doc_h, next_x, context)
            } else {
                old_y
            }
        } else {
            old_y
        };
        self.scroll_x = next_x;
        self.scroll_y = next_y;
        let changed = self.scroll_x != old_x || self.scroll_y != old_y;
        if changed {
            self.fire_window_event("scroll");
        }
        changed
    }

    fn dispatch_element_scroll_changes(&mut self, before: &[(u32, f32, f32)]) {
        let after: HashMap<_, _> = collect_element_scroll_offsets(&self.root)
            .into_iter()
            .map(|(id, x, y)| (id, (x, y)))
            .collect();
        for (id, old_x, old_y) in before {
            let Some((new_x, new_y)) = after.get(id) else {
                continue;
            };
            if (new_x - old_x).abs() > 0.01 || (new_y - old_y).abs() > 0.01 {
                let mut event = crate::dom::events::DomEvent::new("scroll", *id);
                self.dispatch_dom_event(&mut event);
            }
        }
    }
}

fn collect_element_scroll_offsets(root: &WebCore) -> Vec<(u32, f32, f32)> {
    let mut out = Vec::new();
    collect_element_scroll_offsets_inner(root, &mut out);
    out
}

fn collect_element_scroll_offsets_inner(node: &WebCore, out: &mut Vec<(u32, f32, f32)>) {
    out.push((
        node.node_id,
        node.layout.scroll_left,
        node.layout.scroll_top,
    ));
    for child in &node.children {
        collect_element_scroll_offsets_inner(child, out);
    }
}
