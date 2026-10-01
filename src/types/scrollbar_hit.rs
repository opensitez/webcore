//! Per-element scrollbar hit-testing.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Per-element scrollbar hit-test ──────────────────────────────────────────

pub(crate) fn viewport_scrollbar_thumb(
    track_h: f32,
    visible_h: f32,
    document_h: f32,
    scroll_y: f32,
) -> (f32, f32, f32) {
    if track_h <= 0.0 || visible_h <= 0.0 || document_h <= 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let thumb_h = (track_h * visible_h / document_h.max(visible_h))
        .max(20.0)
        .min(track_h);
    let travel = (track_h - thumb_h).max(0.0);
    let max_scroll = (document_h - visible_h).max(0.0);
    let thumb_y = if max_scroll > 0.0 {
        scroll_y.clamp(0.0, max_scroll) * travel / max_scroll
    } else {
        0.0
    };
    let scroll_per_px = if travel > 0.0 {
        max_scroll / travel
    } else {
        0.0
    };
    (thumb_h, thumb_y, scroll_per_px)
}

#[cfg(test)]
mod tests {
    use super::viewport_scrollbar_thumb;

    #[test]
    fn viewport_thumb_stays_inside_short_tracks() {
        for scroll_y in [0.0, 5.0, 10.0] {
            let (height, y, per_pixel) = viewport_scrollbar_thumb(12.0, 12.0, 22.0, scroll_y);
            assert_eq!((height, y, per_pixel), (12.0, 0.0, 0.0));
        }
        assert_eq!(
            viewport_scrollbar_thumb(0.0, 0.0, 0.0, 0.0),
            (0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn viewport_thumb_position_and_drag_scale_share_geometry() {
        let (height, y, per_pixel) = viewport_scrollbar_thumb(200.0, 200.0, 800.0, 300.0);
        assert_eq!(height, 50.0);
        assert_eq!(y, 75.0);
        assert_eq!(per_pixel, 4.0);
    }
}

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
    let pry = pr.y - sy;

    let show_v = node.style.overflow_y == Overflow::Scroll
        || (node.style.overflow_y == Overflow::Auto && node.layout.scroll_height > cr.h);
    let scrollport_w = node.scrollport_content_width();
    let show_h = node.style.overflow_x == Overflow::Scroll
        || (node.style.overflow_x == Overflow::Auto && node.layout.scroll_width > scrollport_w);
    let sbw = node.style.scrollbar_width_px();
    if show_v && sbw > 0.0 {
        // Scrollbar is at the right edge of the padding box (matches draw_scrollbars).
        let track_x = prx + pr.w - sbw;
        let track_h = (pr.h - if show_h { sbw } else { 0.0 }).max(0.0);
        if track_h > 0.0
            && screen_x >= track_x
            && screen_x < prx + pr.w
            && screen_y >= pry
            && screen_y < pry + track_h
        {
            let scrollable_h = node.layout.scroll_height.max(cr.h) + (pr.h - cr.h).max(0.0);
            let thumb_h = (track_h * pr.h / scrollable_h).max(20.0).min(track_h);
            let max_s = (node.layout.scroll_height - cr.h).max(0.0);
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
            let local_y = screen_y - pry;

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

    if show_h && sbw > 0.0 {
        let (left_gutter, right_gutter) = node.style.scrollbar_gutter_edges(show_v);
        let track_x = prx + if left_gutter { sbw } else { 0.0 };
        let track_w =
            (pr.w - if left_gutter { sbw } else { 0.0 } - if right_gutter { sbw } else { 0.0 })
                .max(0.0);
        let track_y = pry + pr.h - sbw;
        if track_w > 0.0
            && screen_x >= track_x
            && screen_x < track_x + track_w
            && screen_y >= track_y
            && screen_y < track_y + sbw
        {
            let scrollable_w =
                node.layout.scroll_width.max(scrollport_w) + (track_w - scrollport_w).max(0.0);
            let thumb_w = (track_w * track_w / scrollable_w).max(20.0).min(track_w);
            let max_s = (node.layout.scroll_width - scrollport_w).max(0.0);
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
            let local_x = screen_x - track_x;

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
