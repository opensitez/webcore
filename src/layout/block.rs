use super::Constraints;
use crate::layout::grid::{collect_grid_children, grid_child_mut, grid_child_ref};
use crate::layout::{
    layout_positioned, shift_rects, FloatContext, FloatSide, LayoutEngine, ResolvedBox,
};
use crate::types::*;
use std::collections::HashMap;

// ─── Margin collapsing helpers ────────────────────────────────────────────────

/// Collapse two adjoining margins per CSS spec:
/// both positive → max; both negative → min (most negative); mixed → sum.
pub fn collapse_two(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a <= 0.0 && b <= 0.0 {
        a.min(b)
    } else {
        a + b
    }
}

// ─── BFC detection ───────────────────────────────────────────────────────────

/// Returns true if this node establishes a Block Formatting Context.
/// Mirrors C++ EstablishesBFC.
pub fn establishes_bfc(style: &ComputedStyle) -> bool {
    matches!(style.float, Float::Left | Float::Right)
        || !matches!(style.overflow_x, Overflow::Visible)
        || !matches!(style.overflow_y, Overflow::Visible)
        || matches!(
            style.display,
            Display::InlineBlock
                | Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
                | Display::Table
                | Display::FlowRoot
        )
        || matches!(style.position, Position::Absolute | Position::Fixed)
}

fn text_decoration_source(style: &ComputedStyle) -> Option<ComputedStyle> {
    if style.text_decoration.underline
        || style.text_decoration.overline
        || style.text_decoration.strikethrough
    {
        Some(style.clone())
    } else {
        None
    }
}

fn apply_parent_text_decoration(run_style: &mut ComputedStyle, parent: &ComputedStyle) {
    if parent.text_decoration.underline {
        run_style.text_decoration.underline = true;
    }
    if parent.text_decoration.overline {
        run_style.text_decoration.overline = true;
    }
    if parent.text_decoration.strikethrough {
        run_style.text_decoration.strikethrough = true;
    }
    run_style.text_decoration_color = Some(parent.text_decoration_color.unwrap_or(parent.color));
    if matches!(run_style.text_decoration_style, TextDecorationStyle::Solid) {
        run_style.text_decoration_style = parent.text_decoration_style;
    }
    if run_style.text_decoration_thickness.is_auto() {
        run_style.text_decoration_thickness = parent.text_decoration_thickness.clone();
    }
}

fn scrollbar_gutter_stable(value: &str) -> bool {
    value.split_whitespace().any(|token| token == "stable")
}

fn scrollbar_gutter_both_edges(value: &str) -> bool {
    value.split_whitespace().any(|token| token == "both-edges")
}

fn propagate_text_decoration_to_inline_runs(node: &mut WebCore, parent: &ComputedStyle) {
    for run in &mut node.layout.inline_runs {
        apply_parent_text_decoration(&mut run.style, parent);
    }
    for child in &mut node.children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        propagate_text_decoration_to_inline_runs(child, parent);
    }
}

/// Can top margin of this box collapse with its first child's top margin?
/// Mirrors C++ CanCollapseTopWithFirstChild.
fn can_collapse_top_with_first_child(node: &WebCore, rbox: &ResolvedBox) -> bool {
    if establishes_bfc(&node.style) {
        return false;
    }
    // The root element (<html>) is the initial containing block / BFC root
    if node.tag == "html" {
        return false;
    }
    if rbox.border_top > 0.0 {
        return false;
    }
    if rbox.padding_top > 0.0 {
        return false;
    }
    if !node.layout.line_cache.is_empty() {
        return false;
    }
    true
}

/// Can bottom margin of this box collapse with its last child's bottom margin?
/// Mirrors C++ CanCollapseBottomWithLastChild.
fn can_collapse_bottom_with_last_child(node: &WebCore, rbox: &ResolvedBox) -> bool {
    if establishes_bfc(&node.style) {
        return false;
    }
    if rbox.border_bottom > 0.0 {
        return false;
    }
    if rbox.padding_bottom > 0.0 {
        return false;
    }
    if rbox.content_height.is_some() {
        return false;
    }
    if !node.style.min_height.is_auto() {
        return false;
    }
    if !node.layout.line_cache.is_empty() {
        return false;
    }
    true
}

fn margin_trim_has(value: &str, keyword: &str) -> bool {
    value
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case(keyword))
}

fn trim_child_margin_left(node: &mut WebCore, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    shift_rects(node, -amount, 0.0);
    node.layout.margin_rect.x += amount;
    node.layout.margin_rect.w = (node.layout.margin_rect.w - amount).max(0.0);
    node.layout.resolved_margin_left = (node.layout.resolved_margin_left - amount).max(0.0);
}

fn trim_child_margin_right(node: &mut WebCore, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    node.layout.margin_rect.w = (node.layout.margin_rect.w - amount).max(0.0);
    node.layout.resolved_margin_right = (node.layout.resolved_margin_right - amount).max(0.0);
}

/// Is this an "empty" block (no borders, padding, inline content, explicit height, in-flow children)?
/// Mirrors C++ IsEmptyBlock.
fn is_empty_block(node: &WebCore, rbox: &ResolvedBox) -> bool {
    if rbox.border_top != 0.0 || rbox.border_bottom != 0.0 {
        return false;
    }
    if rbox.padding_top != 0.0 || rbox.padding_bottom != 0.0 {
        return false;
    }
    if !node.layout.line_cache.is_empty() {
        return false;
    }
    if rbox.content_height.is_some() {
        return false;
    }
    if !node.style.min_height.is_auto() {
        return false;
    }
    // Has in-flow block children?
    //
    // `effective_children`, not `children`: a shadow HOST has an empty light
    // tree and all its content in the shadow root. Asking `children` said
    // "empty block", so the host collapsed to zero height and its shadow
    // content was never laid out at all — shadow DOM rendered nothing.
    // ⛔ A float is not in-flow, so it normally cannot stop a box being
    // self-collapsing — but a box that ESTABLISHES a BFC is stretched to
    // contain its floats (§10.6.3), so its used height is not zero and §8.3.1
    // does not apply. Skipping floats unconditionally made a float-only
    // `overflow: hidden` container collapse its own top and bottom margins
    // together, which pushed its PARENT down by the container's bottom margin.
    let bfc = establishes_bfc(&node.style);
    for child in node.effective_children() {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        if !matches!(child.style.float, Float::None) {
            if bfc {
                return false;
            } // the float gives this box height
            continue;
        }
        return false; // has in-flow child
    }
    true
}

// ─── Shrink-to-fit intrinsic width ────────────────────────────────────────────

/// Compute the intrinsic (max-content) width of a box that was laid out at a
/// larger containing width.  Mirrors C++ ComputeIntrinsicWidth.
pub fn compute_intrinsic_width(node: &WebCore) -> f32 {
    let cached = node.layout.cached_intrinsic_w.get();
    if !cached.is_nan() {
        return cached;
    }
    let result = compute_intrinsic_width_inner(node);
    node.layout.cached_intrinsic_w.set(result);
    result
}

fn shrink_to_fit_slop(node: &WebCore) -> f32 {
    if node.style.aspect_ratio.is_some()
        && node.style.width.is_auto()
        && !node.style.height.is_auto()
        && !matches!(node.style.height, CssLength::Percent(_))
    {
        0.0
    } else if !node.layout.line_cache.is_empty()
        && node
            .layout
            .line_cache
            .iter()
            .all(|line| line.text_length == 0)
    {
        0.0
    } else {
        1.0
    }
}

fn compute_intrinsic_width_inner(node: &WebCore) -> f32 {
    // If the box has a fixed width, that IS its intrinsic width (min and max).
    // (In a more complete engine we'd distinguish min-content vs max-content,
    // but for now max-content is what matters for 'Auto' tracks).
    if let crate::types::CssLength::Px(px) = node.style.width {
        if px >= 0.0 {
            return px;
        }
    }

    // For row-direction flex containers, the intrinsic width is the SUM of all
    // flex items' intrinsic widths (+ padding/border/margin), not the max of their
    // laid-out margin_rect positions (which reflect the container width, not content).
    let is_row_flex = matches!(node.style.display, Display::Flex | Display::InlineFlex)
        && matches!(
            node.style.flex_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        );
    if is_row_flex {
        let mut total = 0.0f32;
        for ch in node.effective_children() {
            if matches!(ch.style.display, Display::None) {
                continue;
            }
            if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                continue;
            }
            let child_w = compute_intrinsic_width(ch)
                + ch.layout.resolved_pad_left
                + ch.layout.resolved_pad_right
                + ch.layout.resolved_border_left
                + ch.layout.resolved_border_right
                + ch.layout.resolved_margin_left
                + ch.layout.resolved_margin_right;
            total += child_w;
        }
        return if total > 0.0 { total + 1.0 } else { total };
    }

    let origin = node.layout.content_rect.x;
    let mut w = 0.0f32;
    // Inline line widths — use line.width (raw content width) directly.
    // line.x includes the text-align offset (e.g. centred text shifts line.x right by
    // (avail_w − text_w) / 2), so line.x + line.width − origin would over-report the
    // intrinsic width for centred/right-aligned content.
    for line in &node.layout.line_cache {
        if line.width > w {
            w = line.width;
        }
    }
    // In a flex/grid formatting context, children are positioned by flex/grid layout
    // (not stacked vertically), so use their actual margin_rect extents for all children.
    let is_flex_or_grid = matches!(
        node.style.display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    );

    // Children — the EFFECTIVE ones, so a shadow host measures its shadow
    // tree rather than its (empty) light tree.
    for ch in node.effective_children() {
        if matches!(ch.style.display, Display::None) {
            continue;
        }
        if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        // Inline-display children: measure text nodes and inline elements that were
        // laid out as standalone flex/grid items. Regular inline content is in line_cache.
        if !is_flex_or_grid && matches!(ch.style.display, Display::Inline) {
            if ch.is_text_node() && !ch.layout.line_cache.is_empty() {
                // Text node flex child: its intrinsic width is its own line widths
                let cw = compute_intrinsic_width(ch);
                let total = cw
                    + ch.layout.resolved_pad_left
                    + ch.layout.resolved_pad_right
                    + ch.layout.resolved_border_left
                    + ch.layout.resolved_border_right
                    + ch.layout.resolved_margin_left
                    + ch.layout.resolved_margin_right;
                if total > w {
                    w = total;
                }
            } else if !ch.is_text_node() && node.layout.line_cache.is_empty() {
                // In a block context with mixed block/inline children, line_cache is
                // empty so inline children aren't captured there.  Recurse to get
                // their intrinsic width (e.g. <a> wrapping an <img width=200>).
                let cw = compute_intrinsic_width(ch);
                let total = cw
                    + ch.layout.resolved_pad_left
                    + ch.layout.resolved_pad_right
                    + ch.layout.resolved_border_left
                    + ch.layout.resolved_border_right
                    + ch.layout.resolved_margin_left
                    + ch.layout.resolved_margin_right;
                if total > w {
                    w = total;
                }
            }
            // When line_cache IS populated, non-text inline elements are already
            // captured there. Their margin_rect.x is stale after shift_rects, so skip.
            continue;
        }
        // Block children with auto width: their marginRect is inflated to containing width.
        // Recurse to get real content width.
        // Floated children are positioned by the float algorithm (not stacked
        // vertically), so use their laid-out right edge for intrinsic width.
        if !matches!(ch.style.float, Float::None) {
            let right = (ch.layout.margin_rect.x - origin) + ch.layout.margin_rect.w;
            if right > w {
                w = right;
            }
            continue;
        }
        // Container children with auto or percentage width: their margin_rect
        // is inflated to the containing width during layout, so recurse to get the
        // real intrinsic content width. Percentage widths resolve to the container
        // width during layout, which doesn't reflect intrinsic content width.
        let is_fluid_width_container = (ch.style.width.is_auto()
            || matches!(ch.style.width, CssLength::Percent(_)))
            && matches!(
                ch.style.display,
                Display::Block
                    | Display::ListItem
                    | Display::Flex
                    | Display::InlineFlex
                    | Display::Grid
                    | Display::InlineGrid
            );
        if is_fluid_width_container {
            let child_content = compute_intrinsic_width(ch);
            let total = child_content
                + ch.layout.resolved_pad_left
                + ch.layout.resolved_pad_right
                + ch.layout.resolved_border_left
                + ch.layout.resolved_border_right
                + ch.layout.resolved_margin_left
                + ch.layout.resolved_margin_right;
            if total > w {
                w = total;
            }
        } else {
            // Skip whitespace-only text nodes in flex/grid containers — they are not
            // laid out as flex items and their margin_rect accumulates stale position
            // offsets across re-renders (via shift_rects), producing spuriously large widths.
            if is_flex_or_grid
                && ch.is_text_node()
                && ch.text.chars().all(|c| c.is_ascii_whitespace())
            {
                continue;
            }
            // InlineBlock/InlineFlex/InlineGrid children inside inline flow are already
            // captured as Atomic items in the parent's line_cache widths.  Their
            // margin_rect.x includes text-align centering offsets which would inflate
            // the intrinsic width.  Skip them when line_cache is non-empty.
            if !node.layout.line_cache.is_empty()
                && matches!(
                    ch.style.display,
                    Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
                )
            {
                continue;
            }
            // Fixed-width or non-block child. Avoid counting auto margins (e.g. `margin: 0 auto`
            // on a centered image): those expand to the container width during layout but are
            // not part of the element's intrinsic size.
            let has_auto_h_margin =
                ch.style.margin_left.is_auto() || ch.style.margin_right.is_auto();
            let rw = if has_auto_h_margin {
                // Content + padding + border + any non-auto margins.
                ch.layout.content_rect.w
                    + ch.layout.resolved_pad_left
                    + ch.layout.resolved_pad_right
                    + ch.layout.resolved_border_left
                    + ch.layout.resolved_border_right
                    + (if ch.style.margin_left.is_auto() {
                        0.0
                    } else {
                        ch.layout.resolved_margin_left
                    })
                    + (if ch.style.margin_right.is_auto() {
                        0.0
                    } else {
                        ch.layout.resolved_margin_right
                    })
            } else {
                (ch.layout.margin_rect.x - origin) + ch.layout.margin_rect.w
            };
            if rw > w {
                w = rw;
            }
        }
    }
    // Add 1px epsilon to prevent floating-point rounding from causing spurious wraps
    // when the layout re-runs at exactly the measured width.
    if w > 0.0 {
        w + 1.0
    } else {
        w
    }
}

// ─── Apply relative offset ────────────────────────────────────────────────────

/// Apply position:relative offset to a node's rects after layout.
/// Mirrors C++ ApplyRelativeOffset.
pub fn apply_relative_offset(
    node: &mut WebCore,
    font_px: f32,
    containing_w: f32,
    root_font_px: f32,
) {
    if !matches!(node.style.position, Position::Relative) {
        return;
    }
    let dx = if !node.style.left.is_auto() {
        node.style.left.resolve(font_px, containing_w, root_font_px)
    } else if !node.style.right.is_auto() {
        -node
            .style
            .right
            .resolve(font_px, containing_w, root_font_px)
    } else {
        0.0
    };
    let dy = if !node.style.top.is_auto() {
        node.style.top.resolve(font_px, containing_w, root_font_px)
    } else if !node.style.bottom.is_auto() {
        -node
            .style
            .bottom
            .resolve(font_px, containing_w, root_font_px)
    } else {
        0.0
    };
    if dx != 0.0 || dy != 0.0 {
        shift_rects(node, dx, dy);
    }
}

fn find_last_in_flow_baseline(node: &WebCore) -> Option<f32> {
    if !matches!(node.style.overflow_x, Overflow::Visible)
        || !matches!(node.style.overflow_y, Overflow::Visible)
    {
        return None;
    }
    if let Some(last_line) = node.layout.line_cache.last() {
        if last_line.height > 0.0 {
            return Some(last_line.y + last_line.ascent);
        }
    }
    for child in node.children.iter().rev() {
        if child.style.position == Position::Absolute
            || child.style.position == Position::Fixed
            || matches!(child.style.display, Display::None)
            || !matches!(child.style.float, Float::None)
        {
            continue;
        }
        if let Some(b) = find_last_in_flow_baseline(child) {
            return Some(b);
        }
    }
    None
}

// ─── Build box rects and cache resolved values ────────────────────────────────

/// Set node rects from rbox and geometry.
/// Mirrors C++ BuildBoxRects.
pub fn build_box_rects(
    node: &mut WebCore,
    rbox: &ResolvedBox,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
    margin_left: f32,
    margin_right: f32,
) {
    node.layout.content_rect = Rect::new(content_x, content_y, content_w, content_h);
    node.layout.padding_rect = Rect::new(
        content_x - rbox.padding_left,
        content_y - rbox.padding_top,
        content_w + rbox.padding_left + rbox.padding_right,
        content_h + rbox.padding_top + rbox.padding_bottom,
    );
    node.layout.border_rect = Rect::new(
        node.layout.padding_rect.x - rbox.border_left,
        node.layout.padding_rect.y - rbox.border_top,
        node.layout.padding_rect.w + rbox.border_left + rbox.border_right,
        node.layout.padding_rect.h + rbox.border_top + rbox.border_bottom,
    );
    // For the margin-rect width, negative margins can collapse it to zero or less.
    // Clamp to at least the border-box width so floats with negative margins
    // (e.g. float:left; width:320px; margin-left:-320px) occupy their visual width.
    let mr_w =
        (node.layout.border_rect.w + margin_left + margin_right).max(node.layout.border_rect.w);
    node.layout.margin_rect = Rect::new(
        node.layout.border_rect.x - margin_left,
        node.layout.border_rect.y - rbox.margin_top,
        mr_w,
        node.layout.border_rect.h + rbox.margin_top + rbox.margin_bottom,
    );
    node.layout.baseline = find_last_in_flow_baseline(node).unwrap_or(content_y + content_h);

    // Cache resolved values
    node.layout.resolved_margin_top = rbox.margin_top;
    node.layout.resolved_margin_right = rbox.margin_right;
    node.layout.resolved_margin_bottom = rbox.margin_bottom;
    node.layout.resolved_margin_left = margin_left;
    node.layout.resolved_border_top = rbox.border_top;
    node.layout.resolved_border_right = rbox.border_right;
    node.layout.resolved_border_bottom = rbox.border_bottom;
    node.layout.resolved_border_left = rbox.border_left;
    node.layout.resolved_pad_top = rbox.padding_top;
    node.layout.resolved_pad_right = rbox.padding_right;
    node.layout.resolved_pad_bottom = rbox.padding_bottom;
    node.layout.resolved_pad_left = rbox.padding_left;
    node.layout.resolved_content_width = content_w;
}

// ─── Block formatting context layout ─────────────────────────────────────────

/// Block formatting context layout.
/// Mirrors C++ LayoutBlockFlow.
pub fn layout_block(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    containing_w: f32,
    x: f32,
    y: f32,
    font_px: f32,
    root_font_px: f32,
) -> f32 {
    layout_block_with_fc(
        engine,
        node,
        rbox,
        &Constraints::new(containing_w, x, y, font_px, root_font_px),
        None,
    )
}

/// Block formatting context layout with optional parent float context.
/// Non-BFC blocks share parent's float context; BFC blocks get their own.
pub fn layout_block_with_fc(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    c: &Constraints,
    parent_fc: Option<&mut FloatContext>,
) -> f32 {
    let containing_w = c.available_width;
    let x = c.x;
    let y = c.y;
    let font_px = c.parent_font_px;
    let root_font_px = c.root_font_px;
    let decorating_style = text_decoration_source(&node.style);
    node.layout.line_cache.clear();
    node.layout.inline_runs.clear();
    // **This block IS its children's containing block, height included.**
    //
    // CSS 2.1 §10.5: a percentage height resolves against the containing
    // block's height when that height is definite, and is `auto` when it is
    // not. `resolve_box_vp` implements exactly that and reads the height from
    // `Constraints::available_height` — which block layout never set, so every
    // child of every block saw `None` and every percentage height in the
    // engine collapsed to zero.
    //
    // The tell was an app shell: `html, body { height: 100% }` with a
    // `height: 100%` root box rendered a blank page, and Flutter — whose
    // `Scaffold` is `height: 100%` and whose `Expanded` is `flex: 1` — laid
    // every row out at zero height inside a full-width row that was placed
    // perfectly. Horizontal was right and vertical was empty, which is what
    // "widths resolve against a width nobody forgot to pass" looks like.
    let child_h = rbox.content_height;
    let child_c = |available_width: f32, cx: f32, cy: f32| match child_h {
        Some(h) => Constraints::with_height(available_width, h, cx, cy, font_px, root_font_px),
        None => Constraints::new(available_width, cx, cy, font_px, root_font_px),
    };
    // Content width: respect box-sizing (already resolved in rbox via resolve_box).
    let raw_w = match rbox.content_width {
        Some(w) => w,
        None => {
            if matches!(
                node.style.display,
                Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
            ) {
                let mc = engine.max_content_width(node, font_px, root_font_px);
                if matches!(node.style.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre) {
                    mc
                } else {
                    (containing_w - rbox.h_space()).max(0.0).min(mc)
                }
            } else {
                (containing_w - rbox.h_space()).max(0.0)
            }
        }
    };

    // CSS Sizing §5: an intrinsic keyword sizes the box from its own content.
    // These read as `auto` everywhere that cannot measure content, so the
    // fallback above filled the containing block — a `width: min-content` box
    // came out full width. Here the node IS in hand, so they resolve.
    // Only when nothing definite was resolved: a forced size — the main size
    // flex hands its items — outranks the item's own intrinsic keyword.
    let raw_w = match node
        .style
        .width
        .intrinsic()
        .filter(|_| rbox.content_width.is_none())
    {
        Some(kind) => {
            engine.intrinsic_width(&kind, node, raw_w, font_px, root_font_px, containing_w)
        }
        None => raw_w,
    };

    // Apply min/max-width constraints, converting from border-box to content-box when needed.
    // CSS: with box-sizing:border-box, min/max-width refer to the border box, not the content box.
    let bb_extra = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
        rbox.padding_left + rbox.padding_right + rbox.border_left + rbox.border_right
    } else {
        0.0
    };
    // The available width an intrinsic keyword on min-/max-width measures
    // against. A keyword names a CONTENT size directly, so `box-sizing` has
    // nothing to convert and `bb_extra` does not apply to it.
    let avail_w = (containing_w - rbox.h_space()).max(0.0);
    let min_w = match engine.res_len_sizing(
        &node.style.min_width,
        node,
        avail_w,
        font_px,
        containing_w,
        root_font_px,
    ) {
        Some(v) => v,
        None => {
            let v = engine.res_len(&node.style.min_width, font_px, containing_w, root_font_px);
            (v - bb_extra).max(0.0)
        }
    };
    let max_w = match engine.res_len_sizing(
        &node.style.max_width,
        node,
        avail_w,
        font_px,
        containing_w,
        root_font_px,
    ) {
        Some(v) => v,
        None if node.style.max_width.is_none() || node.style.max_width.is_auto() => f32::MAX,
        None => {
            let v = engine.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
            (v - bb_extra).max(0.0)
        }
    };
    let content_w = raw_w.max(min_w).min(max_w);

    // ── Scrollbar width reservation ───────────────────────────────────────────
    // A vertical scrollbar overlays the right edge of the content box.
    // Without reserving that space, children are laid out at full content_w and
    // their rightmost strip gets painted over by the scrollbar.
    //
    // • overflow-y: scroll → scrollbar is always present: always reserve.
    // • overflow-y: auto with max-height → scrollbar appears when content
    //   overflows max-height, which is the common case for demo panels; reserve
    //   proactively.  (A full two-pass layout would be needed for perfect accuracy
    //   but is unnecessary for the demos that trigger this path.)
    let sbw = node.style.scrollbar_width_px();
    let reserve_v_scrollbar = matches!(node.style.overflow_y, Overflow::Scroll)
        || (matches!(node.style.overflow_y, Overflow::Auto)
            && !node.style.max_height.is_none()
            && !node.style.max_height.is_auto());
    let stable_gutter = scrollbar_gutter_stable(&node.style.scrollbar_gutter)
        && !matches!(node.style.overflow_y, Overflow::Visible);
    let reserve_scrollbar_gutter = reserve_v_scrollbar || stable_gutter;
    let gutter_edges = if reserve_scrollbar_gutter && sbw > 0.0 {
        if scrollbar_gutter_both_edges(&node.style.scrollbar_gutter) {
            2.0
        } else {
            1.0
        }
    } else {
        0.0
    };
    let child_content_w = if gutter_edges > 0.0 {
        (content_w - sbw * gutter_edges).max(0.0)
    } else {
        content_w
    };

    // Auto margin centering (CSS 2.1 §10.3.3 / css-sizing): resolve after
    // min/max-width clamping so `width:auto; max-width:...; margin:0 auto`
    // centers the clamped box instead of sticking to inline-start.
    // CSS 2.1 §10.3.3 applies only to block-level non-replaced elements in normal flow.
    // For inline-level elements (inline-block, inline-flex, etc.), auto margins evaluate to 0 (CSS 2.1 §10.3.10).
    let left_is_auto = node.style.margin_left.is_auto();
    let right_is_auto = node.style.margin_right.is_auto();
    let is_block_box = node.style.is_block_level()
        && !matches!(
            node.style.display,
            Display::InlineBlock | Display::Inline | Display::InlineFlex | Display::InlineGrid
        );
    let (margin_left, margin_right) = if is_block_box && (left_is_auto || right_is_auto) {
        let non_margin_space = rbox.border_left
            + rbox.padding_left
            + content_w
            + rbox.padding_right
            + rbox.border_right;
        let available = (containing_w - non_margin_space).max(0.0);
        if !node.style.width.is_auto() || available > 0.0 {
            if left_is_auto && right_is_auto {
                let ml = (available / 2.0).floor();
                (ml, available - ml)
            } else if left_is_auto {
                (available - rbox.margin_right, rbox.margin_right)
            } else {
                (rbox.margin_left, available - rbox.margin_left)
            }
        } else {
            (rbox.margin_left, rbox.margin_right)
        }
    } else {
        (rbox.margin_left, rbox.margin_right)
    };

    let is_bfc = establishes_bfc(&node.style);

    let content_x = x
        + margin_left
        + rbox.border_left
        + rbox.padding_left
        + if gutter_edges > 1.0 { sbw } else { 0.0 };
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    // ─── CSS margin collapsing setup ──────────────────────────────────────────
    let can_collapse_top = can_collapse_top_with_first_child(node, rbox);
    let can_collapse_bottom = can_collapse_bottom_with_last_child(node, rbox);

    // ─── Wrap mixed inline/block children in anonymous blocks (CSS 2.1 §9.2.1.1)
    wrap_mixed_children_in_anonymous_blocks(node);

    // ─── Flatten display:contents and collect effective children ────────────
    let eff_children = collect_grid_children(node);

    // ─── Collect out-of-flow children ─────────────────────────────────────────
    let mut abs_children: Vec<(Vec<usize>, usize)> = Vec::new(); // (path, dom_index)
    for (idx, path) in eff_children.iter().enumerate() {
        let child = grid_child_ref(node, path);
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            abs_children.push((path.clone(), idx));
        }
    }

    // ─── Float context ────────────────────────────────────────────────────────
    // BFC roots always get their own float context so that their floated children
    // are contained within them (CSS §9.4.1).  Non-BFC blocks share the parent's
    // float context so floats can escape and affect ancestor layout.
    let mut fc_owned;
    let fc = if is_bfc {
        fc_owned = FloatContext::default();
        fc_owned.origin_x = content_x;
        fc_owned.origin_y = content_y;
        &mut fc_owned
    } else if let Some(f) = parent_fc {
        f
    } else {
        fc_owned = FloatContext::default();
        fc_owned.origin_x = content_x;
        fc_owned.origin_y = content_y;
        &mut fc_owned
    };

    // ─── Multi-column layout (early return path) ──────────────────────────────
    if establishes_column_context(&node.style) && !node.children.is_empty() {
        let col_h = layout_columns(
            engine,
            node,
            rbox,
            content_x,
            content_y,
            content_w,
            font_px,
            root_font_px,
        );
        let content_h = match rbox.content_height {
            Some(h) => h,
            None => col_h,
        };
        let min_h = engine.res_len(&node.style.min_height, font_px, 0.0, root_font_px);
        let max_h = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
            f32::MAX
        } else {
            let v = engine.res_len(&node.style.max_height, font_px, 0.0, root_font_px);
            if v == 0.0 && matches!(node.style.max_height, CssLength::Percent(_)) {
                f32::MAX
            } else {
                v
            }
        };
        let content_h = content_h.max(min_h).min(max_h).max(0.0);
        build_box_rects(
            node,
            rbox,
            content_x,
            content_y,
            content_w,
            content_h,
            margin_left,
            margin_right,
        );
        // Absolute/fixed children
        let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style)
        {
            node.layout.padding_rect
        } else {
            engine.pos_cb.get()
        };
        for (path, _) in &abs_children {
            let child = grid_child_mut(node, path);
            layout_positioned(engine, child, containing_rect, font_px, root_font_px);
        }
        node.layout.layout_dirty = false;
        node.layout.last_containing_width = containing_w;
        return node.layout.margin_rect.h;
    }

    // ─── Main block children loop ─────────────────────────────────────────────
    let mut child_y = 0.0f32;
    let mut prev_bottom_margin = 0.0f32;
    let mut is_first_in_flow = true;
    let mut first_child_collapsed = false;
    let mut first_in_flow_path: Option<Vec<usize>> = None;
    let mut last_in_flow_path: Option<Vec<usize>> = None;
    let mut block_in_flow_paths: Vec<Vec<usize>> = Vec::new();
    // If the parent passed a float context with floats, children need to
    // receive it so their inline content wraps around those floats.
    let mut seen_float = !is_bfc && !fc.floats.is_empty();
    // Inline flow state for anonymous inline formatting contexts.
    let mut inline_x = 0.0f32;
    let mut inline_line_h = 0.0f32;
    // Where the current line's inline run starts, and which children are on
    // it: a left float placed mid-line has to move them aside.
    let mut inline_line_start_x = 0.0f32;
    let mut inline_line_paths: Vec<Vec<usize>> = Vec::new();

    // Track static positions for absolute children (indexed by eff_children position)
    let mut abs_static_x: HashMap<usize, f32> = HashMap::new();
    let mut abs_static_y: HashMap<usize, f32> = HashMap::new();

    for (eff_idx, path) in eff_children.iter().enumerate() {
        let ch = grid_child_ref(node, path);
        let child_display = ch.style.display;
        let child_float = ch.style.float;
        let child_clear = ch.style.clear;
        let child_position = ch.style.position;

        if matches!(child_display, Display::None) {
            continue;
        }
        if matches!(child_position, Position::Absolute | Position::Fixed) {
            // Record absolute document-space static position for this abs child.
            // content_x/content_y are already in document space; inline_x/child_y
            // are relative to that content origin.
            let sy = content_y + child_y;
            let sx = if node.style.direction == Direction::RTL {
                None
            } else {
                Some(content_x + inline_x.max(inline_line_start_x))
            };
            if let Some(sx) = sx {
                abs_static_x.insert(eff_idx, sx);
            }
            abs_static_y.insert(eff_idx, sy);
            // Also store on the node itself for deeply nested abs elements whose
            // containing block is an ancestor further up the tree.
            let child = grid_child_mut(node, path);
            child.layout.abs_static_x = sx;
            child.layout.abs_static_y = Some(sy);
            continue;
        }

        // Handle clear
        match child_clear {
            Clear::None => {}
            clear => {
                child_y = fc.clear_y(content_y + child_y - fc.origin_y, clear)
                    - (content_y - fc.origin_y);
                prev_bottom_margin = 0.0;
            }
        }

        // Flush any pending inline line before a BLOCK child.
        //
        // ⛔ A float does not flush it. CSS 2.1 §9.5.1: a float's top may not be
        // higher than the top of the current line box — it sits ON that line,
        // and the inline content moves aside for it. Flushing here dropped the
        // float onto the next line whenever any inline content preceded it.
        if grid_child_ref(node, path).style.is_block_level()
            && matches!(child_float, Float::None)
            && inline_line_h > 0.0
        {
            child_y += inline_line_h;
            inline_x = 0.0;
            inline_line_start_x = 0.0;
            inline_line_paths.clear();
            inline_line_h = 0.0;
        }

        if !matches!(child_float, Float::None) {
            seen_float = true;
            // Layout float to get natural size
            engine.layout_box(
                grid_child_mut(node, path),
                &child_c(child_content_w, content_x, content_y + child_y),
            );
            // Shrink-to-fit for auto-width floats
            if grid_child_ref(node, path).style.width.is_auto() {
                let ch = grid_child_ref(node, path);
                let max_line_w = ch
                    .layout
                    .line_cache
                    .iter()
                    .map(|line| line.width)
                    .fold(0.0_f32, f32::max);
                let intrinsic_w = if max_line_w > 0.0 {
                    max_line_w
                } else {
                    engine
                        .intrinsic_sizes(ch, font_px, root_font_px)
                        .max_content
                };
                if intrinsic_w > 0.0 && intrinsic_w < child_content_w {
                    let irb = grid_child_ref(node, path);
                    let shrink_w = intrinsic_w.ceil()
                        + shrink_to_fit_slop(irb)
                        + irb.layout.resolved_pad_left
                        + irb.layout.resolved_pad_right
                        + irb.layout.resolved_border_left
                        + irb.layout.resolved_border_right
                        + irb.layout.resolved_margin_left
                        + irb.layout.resolved_margin_right;
                    engine.layout_box(
                        grid_child_mut(node, path),
                        &child_c(shrink_w, content_x, content_y + child_y),
                    );
                }
            }
            let ch = grid_child_ref(node, path);
            let effective_w = ch.layout.border_rect.w
                + ch.layout.resolved_margin_left
                + ch.layout.resolved_margin_right;
            let float_w = effective_w.max(0.0);
            let float_h = ch.layout.margin_rect.h;
            let side = if child_float == Float::Left {
                FloatSide::Left
            } else {
                FloatSide::Right
            };
            let local_x = content_x - fc.origin_x;
            let placed = fc.place_float_in(
                local_x,
                content_y + child_y - fc.origin_y,
                float_w,
                float_h,
                child_content_w,
                side,
                &ch.style.shape_outside,
                ch.style.shape_margin.resolve(
                    ch.style.font_size_px(font_px, root_font_px),
                    child_content_w,
                    root_font_px,
                ),
            );
            let ch = grid_child_ref(node, path);
            // ⛔ Back into document space through the CONTEXT's origin, which is
            // where `placed` is measured from — not through this block's own
            // content top. The two are the same only while a block owns its
            // context; once it shares a parent's they differ by exactly the
            // offset between them, and every float in a shared context landed
            // that far down the page.
            let dx = content_x + placed.x - ch.layout.margin_rect.x;
            let dy = fc.origin_y + placed.y - ch.layout.margin_rect.y;
            shift_rects(grid_child_mut(node, path), dx, dy);

            // A LEFT float takes the near edge of the line it lands on, so any
            // inline content already placed there slides right by as much as
            // the float occupies. A right float takes the far edge and moves
            // nothing.
            if child_float == Float::Left && !inline_line_paths.is_empty() {
                let new_start = placed.x + float_w;
                let dx = new_start - inline_line_start_x;
                if dx > 0.01 {
                    for p in &inline_line_paths {
                        shift_rects(grid_child_mut(node, p), dx, 0.0);
                    }
                    inline_x += dx;
                    inline_line_start_x = new_start;
                }
            }

            if matches!(
                grid_child_ref(node, path).style.position,
                Position::Relative | Position::Sticky
            ) {
                let rel_font_px = grid_child_ref(node, path)
                    .style
                    .font_size_px(font_px, root_font_px);
                apply_relative_offset(
                    grid_child_mut(node, path),
                    rel_font_px,
                    child_content_w,
                    root_font_px,
                );
            }
            continue;
        }

        if grid_child_ref(node, path).style.is_block_level() {
            // Progressive layout: defer children below the viewport cutoff
            let cutoff = engine.progressive_cutoff;
            if cutoff > 0.0 && (content_y + child_y) > cutoff {
                // Give deferred child a zero-height placeholder
                let child = grid_child_mut(node, path);
                child.layout.content_rect =
                    crate::types::Rect::new(content_x, content_y + child_y, child_content_w, 0.0);
                child.layout.padding_rect = child.layout.content_rect;
                child.layout.border_rect = child.layout.content_rect;
                child.layout.margin_rect = child.layout.content_rect;
                child.layout.layout_dirty = true; // will be laid out in remainder pass
                continue;
            }

            let child = grid_child_ref(node, path);
            // Incremental layout: skip clean children whose containing width hasn't changed.
            // Just reposition them at the current child_y.
            let can_skip = !child.layout.layout_dirty
                && !child.has_dirty_descendant
                && child.layout.last_containing_width > 0.0
                && (child.layout.last_containing_width - child_content_w).abs() < 0.01
                && child.layout.margin_rect.h > 0.0;
            if can_skip {
                // Reposition only — keep cached geometry
                let child = grid_child_mut(node, path);
                let dx = content_x - child.layout.margin_rect.x;
                let dy = (content_y + child_y) - child.layout.margin_rect.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    crate::layout::shift_rects(child, dx, dy);
                }
            } else {
                let child_is_bfc_pre = establishes_bfc(&grid_child_ref(node, path).style);
                // A child that does NOT establish a BFC shares this one, and
                // that matters before any float has been seen: the floats
                // INSIDE it belong to our context, so later siblings avoid them
                // too. Gating on `seen_float` meant the first such child built
                // its own context and its floats vanished from the parent's
                // list — a float overflowing an `overflow: visible` block
                // stopped affecting everything after it.
                if child_is_bfc_pre {
                    engine.layout_box(
                        grid_child_mut(node, path),
                        &child_c(child_content_w, content_x, content_y + child_y),
                    );
                } else {
                    engine.layout_box_with_fc(
                        grid_child_mut(node, path),
                        &child_c(child_content_w, content_x, content_y + child_y),
                        Some(&mut *fc),
                    );
                }
            }

            if let Some(parent) = decorating_style.as_ref() {
                propagate_text_decoration_to_inline_runs(grid_child_mut(node, path), parent);
            }

            let ch = grid_child_ref(node, path);
            let child_top_margin = ch.layout.collapsed_margin_top;
            let child_bottom_margin = ch.layout.collapsed_margin_bottom;

            if is_first_in_flow && can_collapse_top && !seen_float {
                first_child_collapsed = true;
                first_in_flow_path = Some(path.clone());
            } else {
                let collapsed = collapse_two(prev_bottom_margin, child_top_margin);
                child_y += collapsed - prev_bottom_margin;
                is_first_in_flow = false;
            }
            let _ = is_first_in_flow;

            let ch = grid_child_ref(node, path);
            let child_h = ch.layout.margin_rect.h;
            let mut left_edge = 0.0f32;
            let mut right_edge = child_content_w;
            let child_is_bfc = establishes_bfc(&ch.style);
            if child_is_bfc {
                let local_x = content_x - fc.origin_x;
                fc.available_width_in(
                    local_x,
                    content_y + child_y - fc.origin_y,
                    child_h,
                    child_content_w,
                    &mut left_edge,
                    &mut right_edge,
                );
            }

            let ch = grid_child_ref(node, path);
            let child_margin_left = ch.layout.resolved_margin_left;
            let child_margin_right = ch.layout.resolved_margin_right;
            let child_border_top = ch.layout.resolved_border_top;
            let child_pad_top = ch.layout.resolved_pad_top;
            let child_content_h = ch.layout.content_rect.h;
            let child_rbox_copy = ResolvedBox {
                margin_top: ch.layout.resolved_margin_top,
                margin_right: child_margin_right,
                margin_bottom: ch.layout.resolved_margin_bottom,
                margin_left: child_margin_left,
                border_top: child_border_top,
                border_right: ch.layout.resolved_border_right,
                border_bottom: ch.layout.resolved_border_bottom,
                border_left: ch.layout.resolved_border_left,
                padding_top: ch.layout.resolved_pad_top,
                padding_right: ch.layout.resolved_pad_right,
                padding_bottom: ch.layout.resolved_pad_bottom,
                padding_left: ch.layout.resolved_pad_left,
                content_width: Some(ch.layout.resolved_content_width),
                content_height: Some(child_content_h),
            };
            let cx = content_x
                + left_edge
                + child_margin_left
                + child_rbox_copy.border_left
                + child_rbox_copy.padding_left;
            let cy = content_y + child_y + child_border_top + child_pad_top;
            let ch = grid_child_ref(node, path);
            let dx = cx - ch.layout.content_rect.x;
            let dy = cy - ch.layout.content_rect.y;
            shift_rects(grid_child_mut(node, path), dx, dy);

            // Compute child_y from normal flow position BEFORE relative offset,
            // so that relative positioning doesn't shift subsequent siblings.
            let ch = grid_child_ref(node, path);
            child_y = ch.layout.margin_rect.y - content_y + ch.layout.margin_rect.h;
            prev_bottom_margin = child_bottom_margin;
            last_in_flow_path = Some(path.clone());
            block_in_flow_paths.push(path.clone());
            is_first_in_flow = false;

            if matches!(
                grid_child_ref(node, path).style.position,
                Position::Relative | Position::Sticky
            ) {
                let rel_font_px = grid_child_ref(node, path)
                    .style
                    .font_size_px(font_px, root_font_px);
                apply_relative_offset(
                    grid_child_mut(node, path),
                    rel_font_px,
                    child_content_w,
                    root_font_px,
                );
            }
        } else if grid_child_ref(node, path).style.is_inline_level() {
            if is_empty_line_neutral_inline(grid_child_ref(node, path)) {
                continue;
            }
            let is_whitespace_only_text = grid_child_ref(node, path).is_text_node()
                && grid_child_ref(node, path)
                    .text
                    .chars()
                    .all(|c| c.is_ascii_whitespace());
            if node.layout.line_cache.is_empty() && !is_whitespace_only_text {
                engine.layout_box(
                    grid_child_mut(node, path),
                    &child_c(child_content_w, content_x, content_y + child_y),
                );
                // Shrink-to-fit for inline children (inline, inline-block, inline-flex, inline-grid)
                let ch = grid_child_ref(node, path);
                if ch.style.width.is_auto() {
                    let max_line_w = ch
                        .layout
                        .line_cache
                        .iter()
                        .map(|l| l.width)
                        .fold(0.0_f32, f32::max);
                    let intrinsic_w = if max_line_w > 0.0 {
                        max_line_w
                    } else {
                        engine.max_content_width(ch, font_px, root_font_px)
                    };
                    if intrinsic_w > 0.0 {
                        let shrink_w = intrinsic_w.ceil()
                            + shrink_to_fit_slop(ch)
                            + ch.layout.resolved_pad_left
                            + ch.layout.resolved_pad_right
                            + ch.layout.resolved_border_left
                            + ch.layout.resolved_border_right
                            + ch.layout.resolved_margin_left
                            + ch.layout.resolved_margin_right;
                        if shrink_w < child_content_w {
                            engine.layout_box(
                                grid_child_mut(node, path),
                                &child_c(shrink_w, content_x, content_y + child_y),
                            );
                        }
                    }
                }

                if let Some(parent) = decorating_style.as_ref() {
                    propagate_text_decoration_to_inline_runs(grid_child_mut(node, path), parent);
                }

                let ch = grid_child_ref(node, path);
                let child_mw = ch.layout.margin_rect.w;
                let child_mh = ch.layout.margin_rect.h;

                if inline_x > 0.0 && inline_x + child_mw > child_content_w {
                    child_y += inline_line_h;
                    inline_x = 0.0;
                    inline_line_start_x = 0.0;
                    inline_line_paths.clear();
                    inline_line_h = 0.0;
                }
                if inline_x < inline_line_start_x {
                    inline_x = inline_line_start_x;
                }

                let ch = grid_child_ref(node, path);
                let dx = content_x + inline_x - ch.layout.margin_rect.x;
                let dy = content_y + child_y - ch.layout.margin_rect.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    shift_rects(grid_child_mut(node, path), dx, dy);
                }

                inline_x += child_mw;
                inline_line_paths.push(path.clone());
                // CSS 2.1 §10.8: this anonymous line has a strut too. An
                // atomic inline sits ON the baseline, so the line must still
                // reserve the strut's descent below it — without that the line
                // was exactly as tall as the box and everything after it rode
                // a few pixels high.
                let line_min = crate::layout::inline_layout::strut_line_height(
                    engine,
                    node,
                    font_px,
                    root_font_px,
                    child_mh,
                );
                if line_min > inline_line_h {
                    inline_line_h = line_min;
                }

                if matches!(
                    grid_child_ref(node, path).style.position,
                    Position::Relative | Position::Sticky
                ) {
                    let rel_font_px = grid_child_ref(node, path)
                        .style
                        .font_size_px(font_px, root_font_px);
                    apply_relative_offset(
                        grid_child_mut(node, path),
                        rel_font_px,
                        child_content_w,
                        root_font_px,
                    );
                }
            }
        }
    }

    // Flush trailing inline line
    if inline_line_h > 0.0 {
        child_y += inline_line_h;
    }

    let trims_block_start = margin_trim_has(&node.style.margin_trim, "block")
        || margin_trim_has(&node.style.margin_trim, "block-start");
    if trims_block_start {
        if let Some(first_path) = block_in_flow_paths.first() {
            let mt = grid_child_ref(node, first_path)
                .layout
                .resolved_margin_top
                .max(0.0);
            if mt > 0.0 {
                for path in &block_in_flow_paths {
                    shift_rects(grid_child_mut(node, path), 0.0, -mt);
                }
                child_y = (child_y - mt).max(0.0);
            }
        }
    }

    // ─── Parent-last-child bottom margin collapsing ───────────────────────────
    let _last_child_collapsed_bottom = if let Some(ref p) = last_in_flow_path {
        if can_collapse_bottom {
            let lcb = grid_child_ref(node, p).layout.collapsed_margin_bottom;
            child_y -= lcb;
            lcb
        } else {
            0.0
        }
    } else {
        0.0
    };

    let trims_block_end = margin_trim_has(&node.style.margin_trim, "block")
        || margin_trim_has(&node.style.margin_trim, "block-end");
    if trims_block_end {
        if let Some(last_path) = block_in_flow_paths.last() {
            let mb = grid_child_ref(node, last_path)
                .layout
                .resolved_margin_bottom
                .max(0.0);
            if mb > 0.0 {
                child_y = (child_y - mb).max(0.0);
            }
        }
    }

    let trims_inline_start = margin_trim_has(&node.style.margin_trim, "inline")
        || margin_trim_has(&node.style.margin_trim, "inline-start");
    if trims_inline_start {
        if let Some(first_path) = block_in_flow_paths.first().cloned() {
            let first = grid_child_ref(node, &first_path);
            if node.style.direction == Direction::RTL {
                let mr = first.layout.resolved_margin_right.max(0.0);
                trim_child_margin_right(grid_child_mut(node, &first_path), mr);
            } else {
                let ml = first.layout.resolved_margin_left.max(0.0);
                trim_child_margin_left(grid_child_mut(node, &first_path), ml);
            }
        }
    }

    let trims_inline_end = margin_trim_has(&node.style.margin_trim, "inline")
        || margin_trim_has(&node.style.margin_trim, "inline-end");
    if trims_inline_end {
        if let Some(last_path) = block_in_flow_paths.last().cloned() {
            let last = grid_child_ref(node, &last_path);
            if node.style.direction == Direction::RTL {
                let ml = last.layout.resolved_margin_left.max(0.0);
                trim_child_margin_left(grid_child_mut(node, &last_path), ml);
            } else {
                let mr = last.layout.resolved_margin_right.max(0.0);
                trim_child_margin_right(grid_child_mut(node, &last_path), mr);
            }
        }
    }

    // ─── Content height ───────────────────────────────────────────────────────
    // Include float bottom when:
    // - This element is a BFC root (spec-correct), OR
    // - This element has its OWN directly floated children (practical fix —
    //   without this, containers collapse and subsequent siblings overlap).
    //   Only use own-float expansion for auto-height containers to avoid
    //   breaking elements with explicit heights.
    // ⛔ Only a block that ESTABLISHES a BFC is stretched to contain its floats
    // (CSS 2.1 §10.6.3). Everywhere else the float overflows and the parent's
    // auto height is computed as if it were not there — which is the whole
    // reason `overflow: hidden` and `display: flow-root` are used as clearfix.
    // Growing every container made `overflow: visible` behave like `hidden`: a
    // 60px float gave its parent 60px where a browser gives 0, and the overflow
    // that following content should flow around disappeared.
    let float_bottom = if is_bfc {
        // BFC: fc.origin_y == content_y since we created our own FC
        fc.floats.iter().map(|f| f.clear).fold(0.0f32, f32::max)
    } else {
        0.0
    };
    // Include inline content (line_cache) height
    let inline_bottom = if !node.layout.line_cache.is_empty() {
        let last = node.layout.line_cache.last().unwrap();
        last.y - content_y + last.height
    } else {
        0.0
    };
    let natural_h = child_y.max(float_bottom).max(inline_bottom);

    let content_h = match rbox.content_height {
        Some(h) => h,
        None => natural_h,
    };

    // Apply min/max-height
    let min_h = engine.res_len(&node.style.min_height, font_px, 0.0, root_font_px);
    let max_h = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
        f32::MAX
    } else {
        let v = engine.res_len(&node.style.max_height, font_px, 0.0, root_font_px);
        if v == 0.0 && matches!(node.style.max_height, CssLength::Percent(_)) {
            f32::MAX
        } else {
            v
        }
    };
    let content_h = content_h.max(min_h).min(max_h).max(0.0);

    // Apply aspect-ratio: if height is auto and aspect_ratio is set, derive height from width
    let content_h = if rbox.content_height.is_none() {
        if let Some(ratio) = node.style.aspect_ratio {
            if ratio > 0.0 {
                (content_w / ratio).max(0.0)
            } else {
                content_h
            }
        } else {
            content_h
        }
    } else {
        content_h
    };

    // ─── Build rects ──────────────────────────────────────────────────────────
    build_box_rects(
        node,
        rbox,
        content_x,
        content_y,
        content_w,
        content_h,
        margin_left,
        margin_right,
    );

    // ─── Scroll extent ────────────────────────────────────────────────────────
    if matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto)
        || matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto)
    {
        let natural_scroll_h = child_y.max(float_bottom).max(inline_bottom).max(content_h);
        let natural_scroll_w = node
            .children
            .iter()
            .filter(|child| !matches!(child.style.display, Display::None))
            .map(|child| child.layout.margin_rect.x + child.layout.margin_rect.w - content_x)
            .fold(content_w, f32::max);
        node.layout.scroll_height = natural_scroll_h;
        node.layout.scroll_width = natural_scroll_w;
        let max_scroll_y = (node.layout.scroll_height - content_h).max(0.0);
        let max_scroll_x = (node.layout.scroll_width - content_w).max(0.0);
        node.layout.scroll_top = node.layout.scroll_top.min(max_scroll_y).max(0.0);
        node.layout.scroll_left = node.layout.scroll_left.min(max_scroll_x).max(0.0);
    } else {
        node.layout.scroll_height = content_h;
        node.layout.scroll_width = content_w;
        node.layout.scroll_top = 0.0;
        node.layout.scroll_left = 0.0;
    }

    // ─── Collapsed margins (pass-through to parent) ───────────────────────────
    node.layout.collapsed_margin_top = rbox.margin_top;
    node.layout.collapsed_margin_bottom = rbox.margin_bottom;

    if is_empty_block(node, rbox) {
        // Empty block: own top and bottom margins collapse
        let own = collapse_two(rbox.margin_top, rbox.margin_bottom);
        node.layout.collapsed_margin_top = own;
        node.layout.collapsed_margin_bottom = 0.0;
    } else {
        // Parent-first-child collapsing
        if first_child_collapsed {
            if let Some(ref p) = first_in_flow_path {
                node.layout.collapsed_margin_top = collapse_two(
                    rbox.margin_top,
                    grid_child_ref(node, p).layout.collapsed_margin_top,
                );
            }
        }
        // Parent-last-child collapsing
        if can_collapse_bottom {
            if let Some(ref p) = last_in_flow_path {
                node.layout.collapsed_margin_bottom = collapse_two(
                    rbox.margin_bottom,
                    grid_child_ref(node, p).layout.collapsed_margin_bottom,
                );
            }
        }
    }

    // ─── Absolute/fixed children ──────────────────────────────────────────────
    // Use the padding box as the containing block for positioned children
    // (CSS: containing block for absolutely positioned elements is the padding box
    //  of the nearest positioned ancestor).
    let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style) {
        node.layout.padding_rect
    } else {
        engine.pos_cb.get()
    };
    for (path, dom_idx) in &abs_children {
        let sx = abs_static_x.get(dom_idx).copied();
        let sy = abs_static_y.get(dom_idx).copied();
        crate::layout::layout_positioned_static(
            engine,
            grid_child_mut(node, path),
            containing_rect,
            font_px,
            root_font_px,
            sx,
            sy,
        );
        // Only force-shift to containing block origin when we have NO static position info.
        // When static position is available, layout_positioned_static already placed it correctly.
        if sx.is_none() && sy.is_none() {
            let child = grid_child_mut(node, path);
            let all_auto = child.style.left.is_auto()
                && child.style.right.is_auto()
                && child.style.top.is_auto()
                && child.style.bottom.is_auto();
            if all_auto && matches!(child.style.position, Position::Absolute) {
                let dx = containing_rect.x - child.layout.border_rect.x;
                let dy = containing_rect.y - child.layout.border_rect.y;
                if dx != 0.0 || dy != 0.0 {
                    crate::layout::shift_rects(child, dx, dy);
                }
            }
        }
    }

    // ─── Relative offsets ─────────────────────────────────────────────────────
    // (already applied per-child above; nothing more needed here)

    node.layout.layout_dirty = false;
    node.layout.last_containing_width = containing_w;

    node.layout.margin_rect.h
}

fn is_empty_line_neutral_inline(node: &WebCore) -> bool {
    if node.style.display != Display::Inline
        || node.is_text_node()
        || !node.children.is_empty()
        || node.tag == "br"
    {
        return false;
    }
    if !node.text.is_empty()
        || !node.style.before_content.is_empty()
        || !node.style.after_content.is_empty()
        || (matches!(node.tag.as_str(), "::before" | "::after" | "::marker")
            && !node.style.rare().content.is_empty())
    {
        return false;
    }
    let has_box_decoration = node.layout.resolved_pad_left.abs() > 0.01
        || node.layout.resolved_pad_right.abs() > 0.01
        || node.layout.resolved_pad_top.abs() > 0.01
        || node.layout.resolved_pad_bottom.abs() > 0.01
        || node.layout.resolved_border_left.abs() > 0.01
        || node.layout.resolved_border_right.abs() > 0.01
        || node.layout.resolved_border_top.abs() > 0.01
        || node.layout.resolved_border_bottom.abs() > 0.01
        || node.layout.resolved_margin_left.abs() > 0.01
        || node.layout.resolved_margin_right.abs() > 0.01
        || node.layout.resolved_margin_top.abs() > 0.01
        || node.layout.resolved_margin_bottom.abs() > 0.01
        || node.style.background_color.a > 0
        || !node.style.background_image_url.is_empty()
        || node.bg_image_data.is_some();
    !has_box_decoration
}

// ─── Multi-column layout ──────────────────────────────────────────────────────

/// Returns true if this element establishes a multi-column container.
pub fn establishes_column_context(style: &ComputedStyle) -> bool {
    style.column_count.is_some() || !style.column_width.is_auto()
}

/// A child that takes part in column flow.
fn in_column_flow(c: &WebCore) -> bool {
    !matches!(c.style.display, Display::None)
        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
        && !(c.tag == "#text" && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
}

/// Path from a multi-column container down to the box whose children are the
/// ones that actually get distributed.
///
/// ⛔ Multi-column is a FRAGMENTATION container (css-multicol-1 §3): the column
/// boxes carry the container's CONTENT, which is not the same as its direct
/// children. A single wrapper block in between — which is how MediaWiki writes
/// every one of its `colonnes` blocks — used to put the whole list in column 1.
/// Seeing through plain wrappers is not full fragmentation (a single tall child
/// still cannot be split), but it is what the common markup needs.
fn distribution_path(node: &WebCore) -> Vec<usize> {
    let mut path = Vec::new();
    let mut cur = node;
    while path.len() < 8 {
        let mut inflow = cur
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| in_column_flow(c));
        let (i, child) = match inflow.next() {
            Some(first) if inflow.next().is_none() => first,
            _ => return path,
        };
        // Only see through a plain, auto-sized block that has content of its own.
        if child.children.is_empty() {
            return path;
        }
        if !matches!(
            child.style.display,
            Display::Block | Display::FlowRoot | Display::ListItem
        ) {
            return path;
        }
        if !child.style.width.is_auto() || !child.style.height.is_auto() {
            return path;
        }
        if matches!(child.style.break_inside, BreakInside::Avoid) {
            return path;
        }
        if child.style.column_span_all {
            return path;
        }
        path.push(i);
        cur = child;
    }
    path
}

fn child_at_mut<'a>(node: &'a mut WebCore, path: &[usize]) -> &'a mut WebCore {
    let mut cur = node;
    for &i in path {
        let tmp = cur;
        cur = &mut tmp.children[i];
    }
    cur
}

/// Lay out node's children in a multi-column arrangement.
/// Returns the total content height.
pub fn layout_columns(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    font_px: f32,
    root_font_px: f32,
) -> f32 {
    // 1. Determine column gap
    let gap = if !node.style.column_gap.is_auto() {
        engine.res_len(&node.style.column_gap, font_px, content_w, root_font_px)
    } else {
        font_px // Default gap is 1em
    };

    // 2. Determine column count
    let col_count_from_width = if !node.style.column_width.is_auto() {
        let cw = engine.res_len(&node.style.column_width, font_px, content_w, root_font_px);
        if cw > 0.0 {
            ((content_w + gap) / (cw + gap)).floor().max(1.0) as u32
        } else {
            1
        }
    } else {
        u32::MAX
    };

    let n_cols = match node.style.column_count {
        Some(c) if c > 0 => {
            let c = c as u32;
            if !node.style.column_width.is_auto() {
                c.min(col_count_from_width)
            } else {
                c
            }
        }
        _ => {
            if col_count_from_width == u32::MAX {
                1
            } else {
                col_count_from_width
            }
        }
    }
    .max(1);

    // 3. Column width
    let total_gaps = gap * (n_cols - 1) as f32;
    // css-multicol-1 §3.4 step 11: the used column width is `max(0, …)`. A 1px
    // floor made every column one pixel wider than the spec allows once the
    // gaps alone exceeded the available width.
    let col_w = ((content_w - total_gaps) / n_cols as f32).max(0.0);

    // 4. First-pass layout to get child heights (with span-all flag)
    let path = distribution_path(node);
    let mut child_heights: Vec<(f32, bool)> = Vec::new(); // (height, is_span_all)
    {
        let target = child_at_mut(node, &path);
        for child in target.children.iter_mut() {
            if matches!(child.style.display, Display::None) {
                continue;
            }
            if matches!(child.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            let h = engine.layout_box(
                child,
                &Constraints::new(col_w, content_x, content_y, font_px, root_font_px),
            );
            child_heights.push((h, child.style.column_span_all));
        }
    }

    // 5. Distribute children into columns
    let balance = node.style.column_fill; // true = balance
                                          // Exclude column-span:all children from balance total (they don't occupy a column)
    let total_content_h: f32 = child_heights
        .iter()
        .filter(|(_, span)| !span)
        .map(|(h, _)| h)
        .sum();
    // css-multicol-1 §7: `column-fill: balance` splits the content evenly;
    // `column-fill: auto` fills each column to the container's own height and
    // then moves on. That second case read `f32::MAX`, so a column could never
    // be full and everything stacked in column one, overflowing the container.
    let target_col_h = if balance && n_cols > 1 {
        (total_content_h / n_cols as f32).max(1.0)
    } else if let Some(h) = rbox.content_height {
        h
    } else {
        f32::MAX
    };

    let mut col_idx = 0usize;
    let mut col_cursor: Vec<f32> = vec![0.0; n_cols as usize];
    let mut in_flow_idx = 0usize;
    // Tracks the y-offset added by column-span:all elements
    let mut span_all_y_offset = 0.0f32;

    let target = child_at_mut(node, &path);
    for i in 0..target.children.len() {
        if matches!(target.children[i].style.display, Display::None) {
            continue;
        }
        if matches!(
            target.children[i].style.position,
            Position::Absolute | Position::Fixed
        ) {
            continue;
        }

        let (child_h, _) = child_heights[in_flow_idx];
        in_flow_idx += 1;

        // column-span: all — lay out across full width, then resume all columns below it
        if target.children[i].style.column_span_all {
            let max_col_y = col_cursor.iter().cloned().fold(0.0f32, f32::max);
            let span_y = content_y + span_all_y_offset + max_col_y;
            // Re-layout at full content_w to get correct height (first pass used col_w)
            let actual_span_h = engine.layout_box(
                &mut target.children[i],
                &Constraints::new(content_w, content_x, span_y, font_px, root_font_px),
            );
            span_all_y_offset += max_col_y + actual_span_h;
            col_cursor = vec![0.0; n_cols as usize];
            col_idx = 0;
            continue;
        }

        // css-break-3 §3: a forced break moves to the next column whether or
        // not the current one is full. `avoid` is advisory and ignored here.
        let forced = matches!(
            target.children[i].style.break_before,
            BreakValue::Column | BreakValue::Always
        );
        // A column that has received nothing yet cannot be "too full" — the
        // check ran before anything was placed, so one item taller than the
        // average skipped its whole column and piled the rest into the last.
        let budget = if balance {
            target_col_h * 1.1
        } else {
            target_col_h
        };
        let overflows = col_cursor[col_idx] > 0.0 && col_cursor[col_idx] + child_h > budget;
        if col_idx + 1 < n_cols as usize && (forced || overflows) {
            col_idx += 1;
        }
        if col_idx >= n_cols as usize {
            col_idx = n_cols as usize - 1;
        }

        let col_x = content_x + col_idx as f32 * (col_w + gap);
        let col_y = content_y + span_all_y_offset + col_cursor[col_idx];

        // Re-layout child at its column position
        engine.layout_box(
            &mut target.children[i],
            &Constraints::new(col_w, col_x, col_y, font_px, root_font_px),
        );

        if let Some(max_col_h) = rbox.content_height {
            if target.children[i].layout.border_rect.h > max_col_h {
                let diff = target.children[i].layout.border_rect.h - max_col_h;
                target.children[i].layout.border_rect.h = max_col_h;
                target.children[i].layout.content_rect.h =
                    (target.children[i].layout.content_rect.h - diff).max(0.0);
                target.children[i].layout.padding_rect.h =
                    (target.children[i].layout.padding_rect.h - diff).max(0.0);
                target.children[i].layout.margin_rect.h =
                    (target.children[i].layout.margin_rect.h - diff).max(0.0);
            }
        }

        col_cursor[col_idx] += target.children[i].layout.margin_rect.h;

        let forced_after = matches!(
            target.children[i].style.break_after,
            BreakValue::Column | BreakValue::Always
        );
        if (forced_after || col_cursor[col_idx] >= target_col_h) && col_idx + 1 < n_cols as usize {
            col_idx += 1;
        }
    }

    let max_col_y = col_cursor.iter().cloned().fold(0.0f32, f32::max);
    let total_h = span_all_y_offset + max_col_y;

    // The wrappers this saw through are still real boxes. Give each the
    // container's content area so painting and hit-testing do not read the
    // stale single-column geometry from the first pass.
    let span = Rect::new(content_x, content_y, content_w, total_h);
    let mut cur: &mut WebCore = node;
    for &i in &path {
        let tmp = cur;
        cur = &mut tmp.children[i];
        cur.layout.content_rect = span;
        cur.layout.padding_rect = span;
        cur.layout.border_rect = span;
        cur.layout.margin_rect = span;
    }
    total_h
}

fn is_in_flow_block(c: &WebCore) -> bool {
    !matches!(c.style.display, Display::None)
        && matches!(c.style.float, Float::None)
        && matches!(c.style.position, Position::Static | Position::Relative | Position::Sticky)
        && c.style.is_block_level()
}

fn is_in_flow_inline(c: &WebCore) -> bool {
    !matches!(c.style.display, Display::None)
        && matches!(c.style.float, Float::None)
        && matches!(c.style.position, Position::Static | Position::Relative | Position::Sticky)
        && (c.style.is_inline_level() || c.is_text_node())
}

fn has_renderable_content(anon: &WebCore) -> bool {
    anon.children.iter().any(|c| {
        !c.is_text_node() || !c.text.chars().all(|ch| ch.is_ascii_whitespace())
    })
}

fn make_anonymous_block(parent: &WebCore) -> WebCore {
    let mut anon = WebCore::new("anonymous-block");
    let mut style = (*parent.style).clone();
    style.display = Display::Block;
    style.margin_top = CssLength::Px(0.0);
    style.margin_bottom = CssLength::Px(0.0);
    style.margin_left = CssLength::Px(0.0);
    style.margin_right = CssLength::Px(0.0);
    style.padding_top = CssLength::Px(0.0);
    style.padding_bottom = CssLength::Px(0.0);
    style.padding_left = CssLength::Px(0.0);
    style.padding_right = CssLength::Px(0.0);
    style.border_top_width = CssLength::Px(0.0);
    style.border_bottom_width = CssLength::Px(0.0);
    style.border_left_width = CssLength::Px(0.0);
    style.border_right_width = CssLength::Px(0.0);
    style.background_color = Color::TRANSPARENT;
    style.box_shadow = Vec::new();
    style.position = Position::Static;
    style.float = Float::None;
    style.clear = Clear::None;
    style.width = CssLength::Auto;
    style.height = CssLength::Auto;
    style.min_width = CssLength::Auto;
    style.min_height = CssLength::Auto;
    style.max_width = CssLength::None;
    style.max_height = CssLength::None;
    anon.style = std::sync::Arc::new(style);
    anon.node_id = 0;
    anon.layout.layout_dirty = true;
    anon
}

/// Recursively unwraps any synthetic `anonymous-block` elements in the tree
/// so that cascading and re-layout operate on clean, idempotent DOM structures.
pub fn unwrap_all_anonymous_blocks(node: &mut WebCore) {
    if let Some(ref mut shadow) = node.shadow_root {
        for child in &mut shadow.children {
            unwrap_all_anonymous_blocks(child);
        }
    }
    for child in &mut node.children {
        unwrap_all_anonymous_blocks(child);
    }
    if node.children.iter().any(|c| c.tag == "anonymous-block") {
        let old_children = std::mem::take(&mut node.children);
        for child in old_children {
            if child.tag == "anonymous-block" {
                node.children.extend(child.children);
            } else {
                node.children.push(child);
            }
        }
    }
}

pub fn wrap_mixed_children_in_anonymous_blocks(node: &mut WebCore) {
    // First, unwrap any anonymous blocks from a previous layout pass so that
    // re-layout is idempotent and doesn't create nested anonymous blocks.
    if node.children.iter().any(|c| c.tag == "anonymous-block") {
        let old_children = std::mem::take(&mut node.children);
        for child in old_children {
            if child.tag == "anonymous-block" {
                node.children.extend(child.children);
            } else {
                node.children.push(child);
            }
        }
    }

    if !node.children.iter().any(is_in_flow_block) {
        return;
    }

    let has_non_whitespace_inline = node.children.iter().any(|c| {
        is_in_flow_inline(c) && !(c.is_text_node() && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
    });

    if !has_non_whitespace_inline {
        return;
    }

    node.layout.line_cache.clear();
    node.layout.inline_runs.clear();

    let old_children = std::mem::take(&mut node.children);
    let mut new_children: Vec<WebCore> = Vec::with_capacity(old_children.len());
    let mut current_anon: Option<WebCore> = None;

    for child in old_children {
        if is_in_flow_block(&child) {
            if let Some(anon) = current_anon.take() {
                if has_renderable_content(&anon) {
                    new_children.push(anon);
                }
            }
            new_children.push(child);
        } else if is_in_flow_inline(&child) {
            let is_ws = child.is_text_node() && child.text.chars().all(|ch| ch.is_ascii_whitespace());
            if is_ws && current_anon.is_none() {
                // Inter-block whitespace text node, leave as is
                new_children.push(child);
            } else {
                let anon = current_anon.get_or_insert_with(|| make_anonymous_block(node));
                anon.children.push(child);
            }
        } else {
            // Out of flow (e.g. float or absolute)
            if let Some(ref mut anon) = current_anon {
                anon.children.push(child);
            } else {
                new_children.push(child);
            }
        }
    }
    if let Some(anon) = current_anon {
        if has_renderable_content(&anon) {
            new_children.push(anon);
        }
    }
    node.children = new_children;
}
