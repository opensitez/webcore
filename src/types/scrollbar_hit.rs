//! Per-element scrollbar hit-testing.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Per-element scrollbar hit-test ──────────────────────────────────────────

/// Walk the box tree and hit-test per-element scrollbars.
///
/// `sx`/`sy` are the accumulated scroll offsets for this node's ancestors,
/// matching the renderer's coordinate system (`draw_scrollbars(node, pixmap, sx, sy)`).
/// Screen-space position of a node's content area: `cx = cr.x - sx`, `cy = cr.y - sy`.
///
/// Recurses depth-first (children first) so the innermost scrollable element wins.
/// On hit: optionally jump-scrolls to the click position, then writes a
/// `ScrollbarDrag` with the matching element scrollbar axis into `drag_out`.
pub(crate) fn scrollbar_hit_test(
    node: &mut WebCore,
    screen_x: f32,
    screen_y: f32,
    sx: f32,
    sy: f32,
    _fallback_sbw: f32,
    drag_out: &mut Option<ScrollbarDrag>,
) -> bool {
    if matches!(node.style.display, Display::None) {
        return false;
    }

    // Children are rendered with the parent's scroll added.
    let child_sx = sx + node.layout.scroll_left;
    let child_sy = sy + node.layout.scroll_top;

    for child in node.children.iter_mut() {
        if scrollbar_hit_test(
            child,
            screen_x,
            screen_y,
            child_sx,
            child_sy,
            _fallback_sbw,
            drag_out,
        ) {
            return true;
        }
    }

    let cr = node.layout.content_rect;
    let pr = node.layout.padding_rect;
    let prx = pr.x - sx;
    let cx = cr.x - sx;
    let pry = pr.y - sy;
    let cy = cr.y - sy;

    let show_v = node.style.overflow_y == Overflow::Scroll
        || (node.style.overflow_y == Overflow::Auto && node.layout.scroll_height > cr.h);
    let show_h = node.style.overflow_x == Overflow::Scroll
        || (node.style.overflow_x == Overflow::Auto && node.layout.scroll_width > cr.w);
    let sbw = node.style.scrollbar_width_px();

    if show_v && node.layout.scroll_height > cr.h && sbw > 0.0 {
        // Scrollbar is at the right edge of the padding box (matches draw_scrollbars).
        let track_x = prx + pr.w - sbw;
        if screen_x >= track_x && screen_x < prx + pr.w && screen_y >= cy && screen_y < cy + cr.h {
            let track_h = cr.h;
            let thumb_h = (track_h * track_h / node.layout.scroll_height).max(20.0);
            let max_s = node.layout.scroll_height - cr.h;
            let scroll_per_px = if track_h - thumb_h > 0.0 {
                max_s / (track_h - thumb_h)
            } else {
                0.0
            };
            let thumb_y = if max_s > 0.0 {
                node.layout.scroll_top * (track_h - thumb_h) / max_s
            } else {
                0.0
            };
            let local_y = screen_y - cy;

            // Jump-scroll if click is outside the thumb.
            if !(local_y >= thumb_y && local_y < thumb_y + thumb_h) {
                let new_thumb_y = (local_y - thumb_h * 0.5).clamp(0.0, track_h - thumb_h);
                node.layout.scroll_top = (new_thumb_y * scroll_per_px).clamp(0.0, max_s);
            }

            *drag_out = Some(ScrollbarDrag {
                kind: ScrollbarDragKind::ElementVertical(node.node_id),
                start_mouse_x: screen_x,
                start_mouse_y: screen_y,
                start_scroll: node.layout.scroll_top,
                scroll_per_px,
            });
            return true;
        }
    }

    if show_h && node.layout.scroll_width > cr.w && sbw > 0.0 {
        let track_w = (cr.w
            - if show_v && node.layout.scroll_height > cr.h {
                sbw
            } else {
                0.0
            })
        .max(0.0);
        let track_y = pry + pr.h - sbw;
        if track_w > 0.0
            && screen_x >= cx
            && screen_x < cx + track_w
            && screen_y >= track_y
            && screen_y < track_y + sbw
        {
            let thumb_w = (track_w * cr.w / node.layout.scroll_width)
                .max(20.0)
                .min(track_w);
            let max_s = node.layout.scroll_width - cr.w;
            let scroll_per_px = if track_w - thumb_w > 0.0 {
                max_s / (track_w - thumb_w)
            } else {
                0.0
            };
            let thumb_x = if max_s > 0.0 {
                node.layout.scroll_left * (track_w - thumb_w) / max_s
            } else {
                0.0
            };
            let local_x = screen_x - cx;

            if !(local_x >= thumb_x && local_x < thumb_x + thumb_w) {
                let new_thumb_x = (local_x - thumb_w * 0.5).clamp(0.0, track_w - thumb_w);
                node.layout.scroll_left = (new_thumb_x * scroll_per_px).clamp(0.0, max_s);
            }

            *drag_out = Some(ScrollbarDrag {
                kind: ScrollbarDragKind::ElementHorizontal(node.node_id),
                start_mouse_x: screen_x,
                start_mouse_y: screen_y,
                start_scroll: node.layout.scroll_left,
                scroll_per_px,
            });
            return true;
        }
    }

    false
}
