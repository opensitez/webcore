//! Scrollbar dragging and wheel scrolling.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

impl Document {
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
                            let doc_h = Document::scroll_height(&self.root);
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
                let was_dragging = self.scrollbar_drag.is_some();
                self.scrollbar_drag = None;
                was_dragging
            }

            // ── MouseDown: hit-test scrollbars, start drag ────────────────────
            MouseDown => {
                // Viewport scrollbar — right edge of window.
                let doc_h = Document::scroll_height(&self.root);
                if doc_h > viewport_h && sbw > 0.0 && screen_x >= viewport_w - sbw {
                    let track_h = viewport_h;
                    let thumb_h = (track_h * track_h / doc_h).max(20.0);
                    let max_s = (doc_h - viewport_h).max(0.0);
                    let scale = if track_h - thumb_h > 0.0 {
                        max_s / (track_h - thumb_h)
                    } else {
                        0.0
                    };
                    let thumb_y = if max_s > 0.0 {
                        self.scroll_y * (track_h - thumb_h) / max_s
                    } else {
                        0.0
                    };

                    // Click in track but outside thumb → jump to that position.
                    if !(screen_y >= thumb_y && screen_y < thumb_y + thumb_h) {
                        let new_thumb_y =
                            (screen_y - thumb_h * 0.5).max(0.0).min(track_h - thumb_h);
                        self.scroll_y = (new_thumb_y * scale).min(max_s).max(0.0);
                    }
                    let thumb_y = if max_s > 0.0 {
                        self.scroll_y * (track_h - thumb_h) / max_s
                    } else {
                        0.0
                    };
                    self.scrollbar_drag = Some(ScrollbarDrag {
                        kind: ScrollbarDragKind::Viewport,
                        start_mouse_x: screen_x,
                        start_mouse_y: screen_y,
                        start_scroll: self.scroll_y,
                        scroll_per_px: scale,
                    });
                    let _ = thumb_y;
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
        let before = collect_element_scroll_offsets(&self.root);
        if scroll_box_at(&mut self.root, doc_pt, delta_x, delta_y) {
            self.dispatch_element_scroll_changes(&before);
            return true;
        }
        let old_x = self.scroll_x;
        let old_y = self.scroll_y;
        self.scroll_x -= delta_x;
        if !self.viewport_y_scroll_locked() {
            self.scroll_y -= delta_y;
        }
        let doc_h = Document::scroll_height(&self.root);
        let viewport_h = self.viewport_h.max(0.0);
        let max_y = (doc_h - viewport_h).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_y);
        self.scroll_x = self.scroll_x.max(0.0);
        let changed = self.scroll_x != old_x || self.scroll_y != old_y;
        if changed {
            self.fire_window_event("scroll");
        }
        changed
    }

    fn dispatch_element_scroll_changes(&mut self, before: &[(u32, f32, f32)]) {
        let after = collect_element_scroll_offsets(&self.root);
        for (id, old_x, old_y) in before {
            let Some((_, new_x, new_y)) = after.iter().find(|(after_id, _, _)| after_id == id)
            else {
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
