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

#[derive(Clone, Copy)]
pub(crate) struct ScrollSnapContext {
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub root_font_px: f32,
}

#[derive(Clone, Copy)]
struct SnapPoint {
    position: f32,
    area: Rect,
    stop_always: bool,
}

/// Search a subtree for absolute/fixed descendants that contain `pt` and can
/// be scrolled. Used when an in-flow ancestor fails the hit test but its
/// absolute children may still be under the cursor.
fn scroll_abs_in(
    node: &mut WebCore,
    pt: (f32, f32),
    delta_x: f32,
    delta_y: f32,
    context: ScrollSnapContext,
) -> WheelScrollResult {
    for child in &mut node.children {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            let mr = child.layout.margin_rect;
            if pt.0 >= mr.x && pt.0 < mr.x + mr.w && pt.1 >= mr.y && pt.1 < mr.y + mr.h {
                let result = scroll_box_at(child, pt, delta_x, delta_y, context);
                if result != WheelScrollResult::None {
                    return result;
                }
            }
        } else {
            let result = scroll_abs_in(child, pt, delta_x, delta_y, context);
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
    context: ScrollSnapContext,
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
                let result = scroll_box_at(child, pt, delta_x, delta_y, context);
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
            let result = scroll_box_at(child, local_pt, delta_x, delta_y, context);
            if result != WheelScrollResult::None {
                return result;
            }
        } else {
            // Even though cursor is outside this in-flow child's bounds, its
            // absolute/fixed descendants may be positioned at the cursor location.
            let result = scroll_abs_in(child, pt, delta_x, delta_y, context);
            if result != WheelScrollResult::None {
                return result;
            }
        }
    }

    // Check whether *this* node is scrollable.
    let can_v = node.style.visibility
        && delta_y.abs() > 0.1
        && matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto)
        && node.layout.scroll_height > node.layout.content_rect.h;
    let can_h = node.style.visibility
        && delta_x.abs() > 0.1
        && matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto)
        && node.layout.scroll_width > node.layout.content_rect.w;

    let mut scrolled = false;

    if can_v {
        let max_scroll = (node.layout.scroll_height - node.layout.content_rect.h).max(0.0);
        let before = node.layout.scroll_top;
        node.layout.scroll_top = (node.layout.scroll_top - delta_y).clamp(0.0, max_scroll);
        if (node.layout.scroll_top - before).abs() > 1e-3 {
            apply_scroll_snap_y(node, before, context);
            scrolled |= (node.layout.scroll_top - before).abs() > 1e-3;
        }
    }
    if can_h {
        let max_scroll = (node.layout.scroll_width - node.layout.content_rect.w).max(0.0);
        let before = node.layout.scroll_left;
        node.layout.scroll_left = (node.layout.scroll_left - delta_x).clamp(0.0, max_scroll);
        if (node.layout.scroll_left - before).abs() > 1e-3 {
            apply_scroll_snap_x(node, before, context);
            scrolled |= (node.layout.scroll_left - before).abs() > 1e-3;
        }
    }

    if scrolled {
        return WheelScrollResult::Scrolled(node.node_id);
    }

    // overscroll-behavior: if this element is a scroll container but couldn't
    // scroll (already at boundary), check whether it should swallow the event anyway.
    let is_v_container = matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto);
    let is_h_container = matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto);
    if node.style.visibility
        && delta_y.abs() > 0.1
        && is_v_container
        && node.style.overscroll_behavior_y != OverscrollBehavior::Auto
    {
        return WheelScrollResult::Blocked; // Contain/None: don't chain to parent.
    }
    if node.style.visibility
        && delta_x.abs() > 0.1
        && is_h_container
        && node.style.overscroll_behavior_x != OverscrollBehavior::Auto
    {
        return WheelScrollResult::Blocked;
    }

    WheelScrollResult::None
}

/// Snap the vertical scroll position of `node` to the nearest child snap point,
/// if the element has `scroll-snap-type` with a Y/Both axis.
pub(super) fn apply_scroll_snap_y(node: &mut WebCore, before: f32, context: ScrollSnapContext) {
    node.layout.scroll_top = snapped_scroll_y(node, before, node.layout.scroll_top, context);
}

pub(crate) fn snapped_scroll_y(
    node: &WebCore,
    before: f32,
    current: f32,
    context: ScrollSnapContext,
) -> f32 {
    let max_scroll = (node.layout.scroll_height - node.layout.content_rect.h).max(0.0);
    snapped_scroll_y_in_port(
        node,
        before,
        current,
        node.layout.padding_rect,
        max_scroll,
        node.layout.scroll_left,
        context,
    )
}

pub(crate) fn snapped_viewport_scroll_y(
    root: &WebCore,
    before: f32,
    current: f32,
    document_height: f32,
    cross_offset: f32,
    context: ScrollSnapContext,
) -> f32 {
    let scrollport = Rect::new(0.0, 0.0, context.viewport_w, context.viewport_h);
    let max_scroll = (document_height - context.viewport_h).max(0.0);
    snapped_scroll_y_in_port(
        root,
        before,
        current,
        scrollport,
        max_scroll,
        cross_offset,
        context,
    )
}

fn snapped_scroll_y_in_port(
    node: &WebCore,
    before: f32,
    current: f32,
    scrollport: Rect,
    max_scroll: f32,
    cross_offset: f32,
    context: ScrollSnapContext,
) -> f32 {
    if !node.style.scroll_snap_type.snaps_y(node.style.writing_mode) {
        return current;
    }
    let snapport = snapport_rect(node, scrollport, context);
    let mut snap_points = collect_snap_points_y(node, scrollport, snapport, context);
    snap_points.retain(|point| {
        let target = point.position.clamp(0.0, max_scroll);
        overlaps(
            point.area.x - cross_offset,
            point.area.right() - cross_offset,
            snapport.x,
            snapport.right(),
        ) && overlaps(
            point.area.y - target,
            point.area.bottom() - target,
            snapport.y,
            snapport.bottom(),
        )
    });
    if snap_points.is_empty() {
        return current;
    }
    nearest_snap(
        before,
        current,
        &snap_points,
        snapport.h,
        node.style.scroll_snap_type.mandatory,
    )
    .clamp(0.0, max_scroll)
}

/// Snap the horizontal scroll position of `node`.
pub(super) fn apply_scroll_snap_x(node: &mut WebCore, before: f32, context: ScrollSnapContext) {
    node.layout.scroll_left = snapped_scroll_x(node, before, node.layout.scroll_left, context);
}

pub(crate) fn snapped_scroll_x(
    node: &WebCore,
    before: f32,
    current: f32,
    context: ScrollSnapContext,
) -> f32 {
    let max_scroll = (node.layout.scroll_width - node.layout.content_rect.w).max(0.0);
    snapped_scroll_x_in_port(
        node,
        before,
        current,
        node.layout.padding_rect,
        max_scroll,
        node.layout.scroll_top,
        context,
    )
}

pub(crate) fn snapped_viewport_scroll_x(
    root: &WebCore,
    before: f32,
    current: f32,
    document_width: f32,
    cross_offset: f32,
    context: ScrollSnapContext,
) -> f32 {
    let scrollport = Rect::new(0.0, 0.0, context.viewport_w, context.viewport_h);
    let max_scroll = (document_width - context.viewport_w).max(0.0);
    snapped_scroll_x_in_port(
        root,
        before,
        current,
        scrollport,
        max_scroll,
        cross_offset,
        context,
    )
}

fn snapped_scroll_x_in_port(
    node: &WebCore,
    before: f32,
    current: f32,
    scrollport: Rect,
    max_scroll: f32,
    cross_offset: f32,
    context: ScrollSnapContext,
) -> f32 {
    if !node.style.scroll_snap_type.snaps_x(node.style.writing_mode) {
        return current;
    }
    let snapport = snapport_rect(node, scrollport, context);
    let mut snap_points = collect_snap_points_x(node, scrollport, snapport, context);
    snap_points.retain(|point| {
        let target = point.position.clamp(0.0, max_scroll);
        overlaps(
            point.area.x - target,
            point.area.right() - target,
            snapport.x,
            snapport.right(),
        ) && overlaps(
            point.area.y - cross_offset,
            point.area.bottom() - cross_offset,
            snapport.y,
            snapport.bottom(),
        )
    });
    if snap_points.is_empty() {
        return current;
    }
    nearest_snap(
        before,
        current,
        &snap_points,
        snapport.w,
        node.style.scroll_snap_type.mandatory,
    )
    .clamp(0.0, max_scroll)
}

fn resolve_snap_length(
    length: &CssLength,
    style: &ComputedStyle,
    basis: f32,
    context: ScrollSnapContext,
) -> f32 {
    let font_px = style.font_size_px(context.root_font_px, context.root_font_px);
    length.resolve_vp(
        font_px,
        basis,
        context.root_font_px,
        context.viewport_w,
        context.viewport_h,
    )
}

fn for_each_snap_area(node: &WebCore, mut visit: impl FnMut(&WebCore)) {
    let mut pending: Vec<&WebCore> = node.children.iter().collect();
    while let Some(child) = pending.pop() {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        visit(child);
        let scrolls_x = child.layout.scroll_width > child.layout.content_rect.w;
        let scrolls_y = child.layout.scroll_height > child.layout.content_rect.h;
        let owns_scrollport = matches!(child.style.overflow_x, Overflow::Scroll | Overflow::Hidden)
            || matches!(child.style.overflow_y, Overflow::Scroll | Overflow::Hidden)
            || child.style.overflow_x == Overflow::Auto && scrolls_x
            || child.style.overflow_y == Overflow::Auto && scrolls_y;
        if !owns_scrollport {
            pending.extend(child.children.iter());
        }
    }
}

fn snap_align_for_axis(
    container: &ComputedStyle,
    child: &ComputedStyle,
    horizontal: bool,
    snap_area_size: f32,
    snapport_size: f32,
) -> ScrollSnapAlignValue {
    let container_vertical = !matches!(container.writing_mode, WritingMode::HorizontalTB);
    let align = if horizontal == container_vertical {
        child.scroll_snap_align.block()
    } else {
        child.scroll_snap_align.inline()
    };
    let reference = if snap_area_size > snapport_size {
        child
    } else {
        container
    };
    let start_is_high = if horizontal {
        match reference.writing_mode {
            WritingMode::HorizontalTB => reference.direction == Direction::RTL,
            WritingMode::VerticalRL | WritingMode::SidewaysRL => true,
            WritingMode::VerticalLR | WritingMode::SidewaysLR => false,
        }
    } else {
        !matches!(reference.writing_mode, WritingMode::HorizontalTB)
            && reference.direction == Direction::RTL
    };
    if start_is_high {
        match align {
            ScrollSnapAlignValue::Start => ScrollSnapAlignValue::End,
            ScrollSnapAlignValue::End => ScrollSnapAlignValue::Start,
            other => other,
        }
    } else {
        align
    }
}

fn snapport_rect(node: &WebCore, scrollport: Rect, context: ScrollSnapContext) -> Rect {
    let style = &node.style;
    let left = scrollport.x
        + resolve_snap_length(&style.scroll_padding_left, style, scrollport.w, context).max(0.0);
    let top = scrollport.y
        + resolve_snap_length(&style.scroll_padding_top, style, scrollport.h, context).max(0.0);
    let right = (scrollport.right()
        - resolve_snap_length(&style.scroll_padding_right, style, scrollport.w, context).max(0.0))
    .max(left);
    let bottom = (scrollport.bottom()
        - resolve_snap_length(&style.scroll_padding_bottom, style, scrollport.h, context).max(0.0))
    .max(top);
    Rect::new(left, top, right - left, bottom - top)
}

fn snap_area_rect(child: &WebCore, scrollport: Rect, context: ScrollSnapContext) -> Rect {
    let style = &child.style;
    let border = child.layout.border_rect;
    let left =
        border.x - resolve_snap_length(&style.scroll_margin_left, style, scrollport.w, context);
    let top =
        border.y - resolve_snap_length(&style.scroll_margin_top, style, scrollport.h, context);
    let right = border.right()
        + resolve_snap_length(&style.scroll_margin_right, style, scrollport.w, context);
    let bottom = border.bottom()
        + resolve_snap_length(&style.scroll_margin_bottom, style, scrollport.h, context);
    Rect::new(left, top, right - left, bottom - top)
}

fn collect_snap_points_y(
    node: &WebCore,
    scrollport: Rect,
    snapport: Rect,
    context: ScrollSnapContext,
) -> Vec<SnapPoint> {
    let mut pts = Vec::new();
    for_each_snap_area(node, |child| {
        let area = snap_area_rect(child, scrollport, context);
        let align = snap_align_for_axis(&node.style, &child.style, false, area.h, snapport.h);
        let position = match align {
            ScrollSnapAlignValue::Start => area.y - snapport.y,
            ScrollSnapAlignValue::End => area.bottom() - snapport.bottom(),
            ScrollSnapAlignValue::Center => {
                (area.y + area.bottom() - snapport.y - snapport.bottom()) * 0.5
            }
            ScrollSnapAlignValue::None => return,
        };
        pts.push(SnapPoint {
            position,
            area,
            stop_always: child.style.scroll_snap_stop.eq_ignore_ascii_case("always"),
        });
    });
    pts
}

fn collect_snap_points_x(
    node: &WebCore,
    scrollport: Rect,
    snapport: Rect,
    context: ScrollSnapContext,
) -> Vec<SnapPoint> {
    let mut pts = Vec::new();
    for_each_snap_area(node, |child| {
        let area = snap_area_rect(child, scrollport, context);
        let align = snap_align_for_axis(&node.style, &child.style, true, area.w, snapport.w);
        let position = match align {
            ScrollSnapAlignValue::Start => area.x - snapport.x,
            ScrollSnapAlignValue::End => area.right() - snapport.right(),
            ScrollSnapAlignValue::Center => {
                (area.x + area.right() - snapport.x - snapport.right()) * 0.5
            }
            ScrollSnapAlignValue::None => return,
        };
        pts.push(SnapPoint {
            position,
            area,
            stop_always: child.style.scroll_snap_stop.eq_ignore_ascii_case("always"),
        });
    });
    pts
}

fn overlaps(start: f32, end: f32, port_start: f32, port_end: f32) -> bool {
    start < port_end && end > port_start
}

/// Return the snap target closest to `current`.
/// For proximity snapping, only snap if within half the viewport size.
fn nearest_snap(
    before: f32,
    current: f32,
    pts: &[SnapPoint],
    viewport_size: f32,
    mandatory: bool,
) -> f32 {
    let first_trap = if current > before {
        pts.iter()
            .filter(|pt| pt.stop_always && pt.position > before && pt.position <= current)
            .map(|pt| pt.position)
            .min_by(f32::total_cmp)
    } else {
        pts.iter()
            .filter(|pt| pt.stop_always && pt.position < before && pt.position >= current)
            .map(|pt| pt.position)
            .max_by(f32::total_cmp)
    };
    if let Some(position) = first_trap {
        return position;
    }
    pts.iter()
        .map(|pt| pt.position)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_snap_axes_follow_container_writing_mode() {
        let block = ScrollSnapType {
            axis: ScrollSnapAxis::Block,
            mandatory: true,
        };
        let inline = ScrollSnapType {
            axis: ScrollSnapAxis::Inline,
            mandatory: true,
        };
        assert!(block.snaps_y(WritingMode::HorizontalTB));
        assert!(!block.snaps_x(WritingMode::HorizontalTB));
        assert!(inline.snaps_x(WritingMode::HorizontalTB));
        assert!(block.snaps_x(WritingMode::VerticalRL));
        assert!(!block.snaps_y(WritingMode::VerticalRL));
        assert!(inline.snaps_y(WritingMode::VerticalLR));
        assert!(!inline.snaps_x(WritingMode::VerticalLR));
    }

    #[test]
    fn rtl_inline_start_and_end_reverse_horizontal_alignment() {
        let mut container = ComputedStyle::default();
        container.direction = Direction::RTL;
        let mut child = ComputedStyle::default();
        child.scroll_snap_align =
            ScrollSnapAlign::two(ScrollSnapAlignValue::None, ScrollSnapAlignValue::Start);
        assert_eq!(
            snap_align_for_axis(&container, &child, true, 50.0, 100.0),
            ScrollSnapAlignValue::End
        );
        child.scroll_snap_align =
            ScrollSnapAlign::two(ScrollSnapAlignValue::None, ScrollSnapAlignValue::End);
        assert_eq!(
            snap_align_for_axis(&container, &child, true, 50.0, 100.0),
            ScrollSnapAlignValue::Start
        );
    }

    #[test]
    fn oversized_snap_area_uses_its_own_direction() {
        let container = ComputedStyle::default();
        let mut child = ComputedStyle::default();
        child.direction = Direction::RTL;
        child.scroll_snap_align = ScrollSnapAlign::Start;
        assert_eq!(
            snap_align_for_axis(&container, &child, true, 101.0, 100.0),
            ScrollSnapAlignValue::End
        );
        assert_eq!(
            snap_align_for_axis(&container, &child, true, 100.0, 100.0),
            ScrollSnapAlignValue::Start
        );
    }

    #[test]
    fn vertical_writing_mode_uses_block_direction_for_x_and_inline_direction_for_y() {
        let mut container = ComputedStyle::default();
        container.writing_mode = WritingMode::VerticalRL;
        container.direction = Direction::RTL;
        let mut child = ComputedStyle::default();
        child.scroll_snap_align =
            ScrollSnapAlign::two(ScrollSnapAlignValue::Start, ScrollSnapAlignValue::End);
        assert_eq!(
            snap_align_for_axis(&container, &child, true, 50.0, 100.0),
            ScrollSnapAlignValue::End
        );
        assert_eq!(
            snap_align_for_axis(&container, &child, false, 50.0, 100.0),
            ScrollSnapAlignValue::Start
        );
    }
}
