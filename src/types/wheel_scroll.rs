//! Routing a wheel event to the box that should scroll.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Wheel-event scroll dispatch ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelScrollResult {
    None,
    Scrolled(u32),
    Blocked,
}

/// Search a subtree for absolute/fixed descendants that contain `pt` and can
/// be scrolled. Used when an in-flow ancestor fails the hit test but its
/// absolute children may still be under the cursor.
fn scroll_abs_in(node: &mut WebCore, pt: (f32, f32), delta_x: f32, delta_y: f32) -> WheelScrollResult {
    for child in &mut node.children {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            let mr = child.layout.margin_rect;
            if pt.0 >= mr.x && pt.0 < mr.x + mr.w && pt.1 >= mr.y && pt.1 < mr.y + mr.h {
                let result = scroll_box_at(child, pt, delta_x, delta_y);
                if result != WheelScrollResult::None {
                    return result;
                }
            }
        } else {
            let result = scroll_abs_in(child, pt, delta_x, delta_y);
            if result != WheelScrollResult::None {
                return result;
            }
        }
    }
    WheelScrollResult::None
}

/// Walk the box tree and scroll the *innermost* scrollable box that contains
/// `pt` (in accumulated-scroll document coordinates).  Returns `true` if a box
/// was scrolled.
///
/// `pt` on entry is already adjusted for all ancestor scroll offsets so that it
/// can be compared directly against children's `margin_rect` (layout-space) positions.
pub(crate) fn scroll_box_at(
    node: &mut WebCore,
    pt: (f32, f32),
    delta_x: f32,
    delta_y: f32,
) -> WheelScrollResult {
    if matches!(node.style.display, Display::None) {
        return WheelScrollResult::None;
    }

    // Adjust pt for this node's own scroll so we can test its children,
    // whose margin_rect positions are in layout space (scroll = 0 reference).
    let local_pt = (
        pt.0 + node.layout.scroll_left,
        pt.1 + node.layout.scroll_top,
    );

    // Recurse depth-first; innermost hit wins.
    for child in &mut node.children {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            // Out-of-flow boxes use the viewport coordinate rather than parent scroll.
            let mr = child.layout.margin_rect;
            if pt.0 >= mr.x && pt.0 < mr.x + mr.w && pt.1 >= mr.y && pt.1 < mr.y + mr.h {
                let result = scroll_box_at(child, pt, delta_x, delta_y);
                if result != WheelScrollResult::None {
                    return result;
                }
            }
            continue;
        }
        let mr = child.layout.margin_rect;
        if local_pt.0 >= mr.x
            && local_pt.0 < mr.x + mr.w
            && local_pt.1 >= mr.y
            && local_pt.1 < mr.y + mr.h
        {
            let result = scroll_box_at(child, local_pt, delta_x, delta_y);
            if result != WheelScrollResult::None {
                return result;
            }
        } else {
            // Even though cursor is outside this in-flow child's bounds, its
            // absolute/fixed descendants may be positioned at the cursor location.
            let result = scroll_abs_in(child, pt, delta_x, delta_y);
            if result != WheelScrollResult::None {
                return result;
            }
        }
    }

    // Check whether *this* node is scrollable.
    let can_v = node.style.visibility && delta_y.abs() > 0.1
        && matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto)
        && node.layout.scroll_height > node.layout.content_rect.h;
    let can_h = node.style.visibility && delta_x.abs() > 0.1
        && matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto)
        && node.layout.scroll_width > node.layout.content_rect.w;

    let mut scrolled = false;

    if can_v {
        let max_scroll = (node.layout.scroll_height - node.layout.content_rect.h).max(0.0);
        let before = node.layout.scroll_top;
        node.layout.scroll_top = (node.layout.scroll_top - delta_y).clamp(0.0, max_scroll);
        if (node.layout.scroll_top - before).abs() > 1e-3 {
            apply_scroll_snap_y(node);
            scrolled = true;
        }
    }
    if can_h {
        let max_scroll = (node.layout.scroll_width - node.layout.content_rect.w).max(0.0);
        let before = node.layout.scroll_left;
        node.layout.scroll_left = (node.layout.scroll_left - delta_x).clamp(0.0, max_scroll);
        if (node.layout.scroll_left - before).abs() > 1e-3 {
            apply_scroll_snap_x(node);
            scrolled = true;
        }
    }

    if scrolled {
        return WheelScrollResult::Scrolled(node.node_id);
    }

    // overscroll-behavior: if this element is a scroll container but couldn't
    // scroll (already at boundary), check whether it should swallow the event anyway.
    let is_v_container = matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto);
    let is_h_container = matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto);
    if node.style.visibility && delta_y.abs() > 0.1
        && is_v_container
        && node.style.overscroll_behavior_y != OverscrollBehavior::Auto
    {
        return WheelScrollResult::Blocked; // Contain/None: don't chain to parent.
    }
    if node.style.visibility && delta_x.abs() > 0.1
        && is_h_container
        && node.style.overscroll_behavior_x != OverscrollBehavior::Auto
    {
        return WheelScrollResult::Blocked;
    }

    WheelScrollResult::None
}

/// Snap the vertical scroll position of `node` to the nearest child snap point,
/// if the element has `scroll-snap-type` with a Y/Both axis.
fn apply_scroll_snap_y(node: &mut WebCore) {
    if !node.style.scroll_snap_type.snaps_y() {
        return;
    }
    let content_y = node.layout.content_rect.y;
    let content_h = node.layout.content_rect.h;
    let snap_points = collect_snap_points_y(node, content_y, content_h);
    if snap_points.is_empty() {
        return;
    }
    let max_scroll = (node.layout.scroll_height - content_h).max(0.0);
    let target = nearest_snap(
        node.layout.scroll_top,
        &snap_points,
        content_h,
        node.style.scroll_snap_type.mandatory,
    );
    node.layout.scroll_top = target.clamp(0.0, max_scroll);
}

/// Snap the horizontal scroll position of `node`.
fn apply_scroll_snap_x(node: &mut WebCore) {
    if !node.style.scroll_snap_type.snaps_x() {
        return;
    }
    let content_x = node.layout.content_rect.x;
    let content_w = node.layout.content_rect.w;
    let snap_points = collect_snap_points_x(node, content_x, content_w);
    if snap_points.is_empty() {
        return;
    }
    let max_scroll = (node.layout.scroll_width - content_w).max(0.0);
    let target = nearest_snap(
        node.layout.scroll_left,
        &snap_points,
        content_w,
        node.style.scroll_snap_type.mandatory,
    );
    node.layout.scroll_left = target.clamp(0.0, max_scroll);
}

fn collect_snap_points_y(node: &WebCore, content_y: f32, content_h: f32) -> Vec<f32> {
    let mut pts = Vec::new();
    for child in &node.children {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        let mr = child.layout.margin_rect;
        let pt = match child.style.scroll_snap_align {
            ScrollSnapAlign::Start => mr.y - content_y,
            ScrollSnapAlign::End => mr.y + mr.h - content_y - content_h,
            ScrollSnapAlign::Center => mr.y + mr.h * 0.5 - content_y - content_h * 0.5,
            ScrollSnapAlign::None => continue,
        };
        pts.push(pt);
    }
    pts
}

fn collect_snap_points_x(node: &WebCore, content_x: f32, content_w: f32) -> Vec<f32> {
    let mut pts = Vec::new();
    for child in &node.children {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        let mr = child.layout.margin_rect;
        let pt = match child.style.scroll_snap_align {
            ScrollSnapAlign::Start => mr.x - content_x,
            ScrollSnapAlign::End => mr.x + mr.w - content_x - content_w,
            ScrollSnapAlign::Center => mr.x + mr.w * 0.5 - content_x - content_w * 0.5,
            ScrollSnapAlign::None => continue,
        };
        pts.push(pt);
    }
    pts
}

/// Return the snap target closest to `current`.
/// For proximity snapping, only snap if within half the viewport size.
fn nearest_snap(current: f32, pts: &[f32], viewport_size: f32, mandatory: bool) -> f32 {
    pts.iter()
        .copied()
        .min_by(|a, b| {
            (a - current)
                .abs()
                .partial_cmp(&(b - current).abs())
                .unwrap()
        })
        .map(|nearest| {
            if mandatory || (nearest - current).abs() <= viewport_size * 0.5 {
                nearest
            } else {
                current
            }
        })
        .unwrap_or(current)
}
