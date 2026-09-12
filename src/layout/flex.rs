use super::Constraints;
use crate::layout::{layout_positioned, shift_rects, LayoutEngine, ResolvedBox};
use crate::types::*;

/// Resolve a child by path through `display: contents` wrappers.
// Depth 0 goes through `effective_children`, so a shadow host's items are its
// SHADOW tree. Below that it is ordinary children, matching grid's resolver.
fn child_ref<'a>(node: &'a WebCore, path: &[usize]) -> &'a WebCore {
    let mut n = node;
    for (depth, &i) in path.iter().enumerate() {
        n = if depth == 0 {
            &n.effective_children()[i]
        } else {
            &n.children[i]
        };
    }
    n
}
fn child_mut<'a>(node: &'a mut WebCore, path: &[usize]) -> &'a mut WebCore {
    let mut n = node;
    for (depth, &i) in path.iter().enumerate() {
        n = if depth == 0 {
            &mut n.effective_children_mut()[i]
        } else {
            &mut n.children[i]
        };
    }
    n
}

/// Collect effective flex children, flattening `display: contents`.
fn collect_flex_children(node: &WebCore) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    let mut path = Vec::new();
    collect_inner(node, &mut path, &mut result);
    result
}
fn collect_inner(node: &WebCore, path: &mut Vec<usize>, result: &mut Vec<Vec<usize>>) {
    for (idx, child) in node.effective_children().iter().enumerate() {
        path.push(idx);
        if matches!(child.style.display, Display::Contents) {
            collect_inner(child, path, result);
        } else {
            result.push(path.clone());
        }
        path.pop();
    }
}

/// The content height an item takes when nothing forces its main size — the
/// measurement behind a content-based flex base size and the column
/// content-based minimum.
///
/// The item's own `height` is set aside for the measurement. Both callers want
/// a size derived from the CONTENT: `flex-basis: content` is defined to ignore
/// the specified size (Flexbox §7.2.3), and §4.5's content size suggestion is
/// the min-content size, which the specified size only CAPS afterwards. Reading
/// the declared height back made `flex-basis: 50%` in an auto-height column
/// report that height instead of the content's.
///
/// The measurement leaves the item laid out at that size, and `layout_box`
/// skips a repeat call at the same containing width, so the item is marked
/// dirty again: without that the REAL layout at the flexed main size was
/// skipped and the item kept whatever the measurement produced.
fn measure_content_height(
    engine: &LayoutEngine,
    child: &mut WebCore,
    content_w: f32,
    content_x: f32,
    content_y: f32,
    font_px: f32,
    root_font_px: f32,
) -> f32 {
    let saved = if child.style.height.is_auto() {
        None
    } else {
        let h = child.style.height.clone();
        std::sync::Arc::make_mut(&mut child.style).height = CssLength::Auto;
        Some(h)
    };
    engine.layout_box(
        child,
        &Constraints::new(content_w, content_x, content_y, font_px, root_font_px),
    );
    let h = child.layout.content_rect.h;
    if let Some(sh) = saved {
        std::sync::Arc::make_mut(&mut child.style).height = sh;
    }
    child.layout.layout_dirty = true;
    h
}

/// Flexbox layout (CSS Flexible Box).
/// Faithful port of C++ LayoutFlex.
pub fn layout_flex(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    c: &Constraints,
) -> f32 {
    let containing_w = c.available_width;
    let x = c.x;
    let y = c.y;
    let font_px = c.parent_font_px;
    let root_font_px = c.root_font_px;
    let mut content_w = match rbox.content_width {
        Some(w) => w,
        None => (containing_w - rbox.h_space()).max(0.0),
    };
    // The same intrinsic keywords on the flex container's own width. See the
    // note in `block.rs`: they read as `auto` to every caller that cannot
    // measure content, so without this a `width: min-content` flex container
    // filled its containing block.
    if let Some(kind) = node
        .style
        .width
        .intrinsic()
        .filter(|_| rbox.content_width.is_none())
    {
        content_w =
            engine.intrinsic_width(&kind, node, content_w, font_px, root_font_px, containing_w);
    }

    // ⛔ A flex container is a block-level box, and `min-width` / `max-width`
    // clamp it like any other. This took the resolved or available width and
    // stopped, so `max-width: 150px` on a flex container did nothing at all —
    // it filled its containing block and flexed its items across the full
    // width. Mirrors the clamp in `block.rs`, border-box conversion included.
    {
        let bb_extra = if node.style.box_sizing == BoxSizing::BorderBox {
            rbox.padding_left + rbox.padding_right + rbox.border_left + rbox.border_right
        } else {
            0.0
        };
        let min_w = {
            let v = engine.res_len(&node.style.min_width, font_px, containing_w, root_font_px);
            (v - bb_extra).max(0.0)
        };
        let max_w = if node.style.max_width.is_none() || node.style.max_width.is_auto() {
            f32::MAX
        } else {
            let v = engine.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
            (v - bb_extra).max(0.0)
        };
        content_w = content_w.max(min_w).min(max_w);
    }
    let shrink_to_fit = node.style.display == Display::InlineFlex && rbox.content_width.is_none();
    let content_x = x + rbox.margin_left + rbox.border_left + rbox.padding_left;
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    let flex_direction_is_row = matches!(
        node.style.flex_direction,
        FlexDirection::Row | FlexDirection::RowReverse
    );
    let is_row = if flex_direction_is_row {
        inline_axis_is_horizontal(node.style.writing_mode)
    } else {
        !inline_axis_is_horizontal(node.style.writing_mode)
    };
    // In RTL context, flex-direction:row is visually reversed (items flow right-to-left)
    let rtl_row =
        flex_direction_is_row && is_row && node.style.direction == crate::types::Direction::RTL;
    let is_reversed = matches!(
        node.style.flex_direction,
        FlexDirection::RowReverse | FlexDirection::ColumnReverse
    ) ^ rtl_row; // XOR: RTL flips row direction
    let can_wrap = node.style.flex_wrap != FlexWrap::Nowrap;
    let wrap_reverse = node.style.flex_wrap == FlexWrap::WrapReverse;

    // Main axis size of container. `None` means INDEFINITE: a column whose
    // `height` is `auto` has no main size until its content is measured
    // (Flexbox §9.2 step 3). `min-height` does NOT make it definite — it only
    // clamps the size the content produces — so it must not be substituted
    // here, or line breaking measures against it and wraps items that belong
    // on one line.
    let mut definite_main: Option<f32> = if is_row {
        Some(content_w)
    } else {
        rbox.content_height
    };
    // The column main-axis clamps, applied to the content-derived size when
    // the container's own main size is indefinite. This is what keeps the
    // "holy grail" `min-height` column filling its viewport.
    let (col_min_main, col_max_main) = if is_row {
        (0.0, f32::MAX)
    } else {
        let bb = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
            rbox.padding_top + rbox.padding_bottom + rbox.border_top + rbox.border_bottom
        } else {
            0.0
        };
        let mn = (node.style.min_height.resolve_vp(
            font_px,
            0.0,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        ) - bb)
            .max(0.0);
        let mx = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
            f32::MAX
        } else {
            (node.style.max_height.resolve_vp(
                font_px,
                0.0,
                root_font_px,
                engine.viewport_w,
                engine.viewport_h,
            ) - bb)
                .max(0.0)
        };
        (mn, mx)
    };

    // A percentage gap resolves against the container's own content box in the
    // gap's OWN axis (Box Alignment §8.3): `row-gap` against the height,
    // `column-gap` against the width. Both read the width, so `row-gap: 10%`
    // in a 300x120 column came out 30px instead of 12.
    let content_h_basis = rbox.content_height.unwrap_or(0.0);
    let column_gap = node.style.column_gap.resolve_vp(
        font_px,
        content_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let row_gap = node.style.row_gap.resolve_vp(
        font_px,
        content_h_basis,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let gap_main = if is_row { column_gap } else { row_gap };
    let gap_cross = if is_row { row_gap } else { column_gap };

    // ── Collect flex items ────────────────────────────────────────────────────

    struct FlexItem {
        path: Vec<usize>,
        order: i32,
        flex_grow: f32,
        flex_shrink: f32,
        /// Flex base size (content-box, before min/max clamp)
        base_main: f32,
        /// Hypothetical main size (content-box, after min/max clamp)
        hyp: f32,
        /// Main-axis min/max clamps (content-box)
        min_main: f32,
        max_main: f32,
        /// Padding + border + margin on main axis
        outer_extra: f32,
        /// Final content-box main size after grow/shrink
        main_used: f32,
        /// Final margin-box cross size
        cross_size: f32,
        /// Distance from the margin-box cross-start edge to the item's first
        /// baseline, when it has one (`align-items: baseline`)
        baseline_off: Option<f32>,
        /// Final main position (relative to content origin)
        main_pos: f32,
        /// Final cross position (relative to content origin)
        cross_pos: f32,
        /// `auto` margins as the AUTHOR wrote them, recorded before flexbox
        /// neutralises them on the item's own style. Order: main-start,
        /// main-end, cross-start, cross-end.
        auto_margins: [bool; 4],
        /// The four margins as written, restored after positioning when
        /// flexbox had to neutralise an `auto` one.
        saved_margins: Option<[CssLength; 4]>,
        /// Saved CSS width/height/display before flex mutation (restored after positioning)
        saved_width: CssLength,
        saved_height: CssLength,
        saved_display: Display,
    }

    let mut items: Vec<FlexItem> = Vec::new();
    let child_paths = collect_flex_children(node);

    for path in &child_paths {
        let child = child_mut(node, path);
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        // CSS Flexbox §4.1: whitespace-only anonymous flex items are not rendered
        if child.tag == "#text" && child.text.chars().all(|c| c.is_ascii_whitespace()) {
            continue;
        }
        // CSS Flexbox §4: blockify inline-level flex items (temporary — restored after layout)
        let saved_display = child.style.display;
        if child.tag != "#text" && matches!(child.style.display, Display::Inline) {
            std::sync::Arc::make_mut(&mut child.style).display = Display::Block;
        }

        // Flexbox §8.1: an `auto` margin on a flex item absorbs the line's free
        // space — it is NOT the block-level "centre me in my containing block"
        // margin. The item's own layout would resolve it that way and then
        // flexbox would add its share on top, so the item is laid out with the
        // `auto` sides at zero and flexbox does the distribution itself.
        let auto_margins = if is_row {
            [
                child.style.margin_left.is_auto(),
                child.style.margin_right.is_auto(),
                child.style.margin_top.is_auto(),
                child.style.margin_bottom.is_auto(),
            ]
        } else {
            [
                child.style.margin_top.is_auto(),
                child.style.margin_bottom.is_auto(),
                child.style.margin_left.is_auto(),
                child.style.margin_right.is_auto(),
            ]
        };
        let saved_margins = if auto_margins.iter().any(|&a| a) {
            let st = std::sync::Arc::make_mut(&mut child.style);
            let saved = [
                st.margin_top.clone(),
                st.margin_right.clone(),
                st.margin_bottom.clone(),
                st.margin_left.clone(),
            ];
            if st.margin_top.is_auto() {
                st.margin_top = CssLength::Zero;
            }
            if st.margin_right.is_auto() {
                st.margin_right = CssLength::Zero;
            }
            if st.margin_bottom.is_auto() {
                st.margin_bottom = CssLength::Zero;
            }
            if st.margin_left.is_auto() {
                st.margin_left = CssLength::Zero;
            }
            Some(saved)
        } else {
            None
        };

        let child_font = child.style.font_size_px(font_px, root_font_px);
        let irb = engine.res_box(&child.style, child_font, content_w, root_font_px);

        // Outer extra = padding + border + margin on main axis
        let outer_extra = if is_row {
            irb.padding_left
                + irb.padding_right
                + irb.border_left
                + irb.border_right
                + irb.margin_left
                + irb.margin_right
        } else {
            irb.padding_top
                + irb.padding_bottom
                + irb.border_top
                + irb.border_bottom
                + irb.margin_top
                + irb.margin_bottom
        };

        // Resolve flex-basis → basis_main (content-box size)
        // CSS §9.2.3: A percentage flex-basis resolves against the flex container's
        // inner main size. If that size is indefinite (auto), the percentage is
        // treated as 'auto' (content-based sizing).
        // CSS Sizing §5.1: a definite CROSS size plus an `aspect-ratio` gives a
        // transferred MAIN size, which is what the item uses as its flex base
        // size when neither `flex-basis` nor the main-axis size is set.
        let transferred_main: Option<f32> = match child.style.aspect_ratio {
            Some(ratio) if ratio > 0.0 => {
                if is_row {
                    if child.style.height.is_auto() {
                        None
                    } else {
                        let raw = child.style.height.resolve_vp(
                            child_font,
                            rbox.content_height.unwrap_or(0.0),
                            root_font_px,
                            engine.viewport_w,
                            engine.viewport_h,
                        );
                        let cb = if child.style.box_sizing == BoxSizing::BorderBox {
                            (raw - irb.border_top
                                - irb.border_bottom
                                - irb.padding_top
                                - irb.padding_bottom)
                                .max(0.0)
                        } else {
                            raw.max(0.0)
                        };
                        Some(cb * ratio)
                    }
                } else if child.style.width.is_auto() {
                    None
                } else {
                    let raw = child.style.width.resolve_vp(
                        child_font,
                        content_w,
                        root_font_px,
                        engine.viewport_w,
                        engine.viewport_h,
                    );
                    let cb = if child.style.box_sizing == BoxSizing::BorderBox {
                        (raw - irb.border_left
                            - irb.border_right
                            - irb.padding_left
                            - irb.padding_right)
                            .max(0.0)
                    } else {
                        raw.max(0.0)
                    };
                    Some(cb / ratio)
                }
            }
            _ => None,
        };

        let basis_is_percent_auto =
            !is_row && child.style.flex_basis.has_percentage() && rbox.content_height.is_none();
        // `flex-basis: content` sizes from the content and IGNORES the item's
        // own `width`/`height` (Flexbox §7.2.3), so it skips straight to the
        // content-based branch rather than falling back to the specified size.
        // A percentage basis that has nothing definite to resolve against is
        // treated the same way (§9.2.3 step B): it becomes a CONTENT size, not
        // the item's own `height`, so `flex-basis: 50%` in an auto-height
        // column sizes from the content instead of reading `height`.
        let basis_is_content =
            matches!(child.style.flex_basis, CssLength::Content) || basis_is_percent_auto;
        // An intrinsic keyword on `flex-basis`, or on the item's own size when
        // the basis is `auto` (Sizing §5). Both read as `auto` everywhere else,
        // so the flex algorithm is the one place that can honour them.
        let intrinsic_basis = child.style.flex_basis.intrinsic().or_else(|| {
            // The item's own size is only the basis when `flex-basis` is
            // `auto`. Reading it unconditionally let `width: min-content`
            // override an explicit `flex-basis: 50px`.
            if child.style.flex_basis.is_auto() {
                if is_row {
                    child.style.width.intrinsic()
                } else {
                    child.style.height.intrinsic()
                }
            } else {
                None
            }
        });

        let basis_main: f32 = if let Some(kind) = intrinsic_basis {
            if is_row {
                match kind {
                    CssLength::MinContent => {
                        engine.min_content_width_of_content(child, font_px, root_font_px)
                    }
                    CssLength::MaxContent => {
                        engine.max_content_width_of_content(child, font_px, root_font_px)
                    }
                    // `fit-content` is the max-content size clamped to the
                    // available space, floored by the min-content size
                    // (Sizing §5.1) — NOT simply the max-content size, which
                    // ignored the container and overflowed it.
                    _ => {
                        let mn = engine.min_content_width_of_content(child, font_px, root_font_px);
                        let mx = engine.max_content_width_of_content(child, font_px, root_font_px);
                        mx.min(content_w).max(mn)
                    }
                }
            } else {
                measure_content_height(
                    engine,
                    child,
                    content_w,
                    content_x,
                    content_y,
                    font_px,
                    root_font_px,
                )
            }
        } else if basis_is_content {
            if is_row {
                engine.max_content_width_of_content(child, font_px, root_font_px)
            } else {
                measure_content_height(
                    engine,
                    child,
                    content_w,
                    content_x,
                    content_y,
                    font_px,
                    root_font_px,
                )
            }
        } else if !child.style.flex_basis.is_auto() && !basis_is_percent_auto {
            let raw = child.style.flex_basis.resolve_vp(
                child_font,
                if is_row {
                    content_w
                } else {
                    rbox.content_height.unwrap_or(0.0)
                },
                root_font_px,
                engine.viewport_w,
                engine.viewport_h,
            );
            if child.style.box_sizing == BoxSizing::BorderBox {
                if is_row {
                    (raw - irb.border_left
                        - irb.border_right
                        - irb.padding_left
                        - irb.padding_right)
                        .max(0.0)
                } else {
                    (raw - irb.border_top
                        - irb.border_bottom
                        - irb.padding_top
                        - irb.padding_bottom)
                        .max(0.0)
                }
            } else {
                raw.max(0.0)
            }
        } else if is_row && !child.style.width.is_auto() {
            let raw = child.style.width.resolve_vp(
                child_font,
                content_w,
                root_font_px,
                engine.viewport_w,
                engine.viewport_h,
            );
            if child.style.box_sizing == BoxSizing::BorderBox {
                (raw - irb.border_left - irb.border_right - irb.padding_left - irb.padding_right)
                    .max(0.0)
            } else {
                raw.max(0.0)
            }
        } else if !is_row && !child.style.height.is_auto() {
            let main_ref = rbox.content_height.unwrap_or(0.0);
            let raw = child.style.height.resolve_vp(
                child_font,
                main_ref,
                root_font_px,
                engine.viewport_w,
                engine.viewport_h,
            );
            if child.style.box_sizing == BoxSizing::BorderBox {
                (raw - irb.border_top - irb.border_bottom - irb.padding_top - irb.padding_bottom)
                    .max(0.0)
            } else {
                raw.max(0.0)
            }
        } else if let Some(t) = transferred_main {
            t
        } else {
            // Content-based: compute max-content size on the main axis.
            if is_row {
                // Use lightweight intrinsic_sizes to avoid exponential
                // layout_box calls in deeply nested flex hierarchies.
                engine
                    .intrinsic_sizes(child, font_px, root_font_px)
                    .max_content
            } else {
                // Column direction needs actual height — must do full layout.
                measure_content_height(
                    engine,
                    child,
                    content_w,
                    content_x,
                    content_y,
                    font_px,
                    root_font_px,
                )
            }
        };

        // Apply min/max constraints on main axis.
        // For border-box items, min/max refer to the border box; convert to content-box.
        let bb_main = if child.style.box_sizing == BoxSizing::BorderBox {
            if is_row {
                irb.padding_left + irb.padding_right + irb.border_left + irb.border_right
            } else {
                irb.padding_top + irb.padding_bottom + irb.border_top + irb.border_bottom
            }
        } else {
            0.0
        };
        let max_main: f32 = if is_row {
            if !child.style.max_width.is_none() && !child.style.max_width.is_auto() {
                let v = child.style.max_width.resolve_vp(
                    child_font,
                    content_w,
                    root_font_px,
                    engine.viewport_w,
                    engine.viewport_h,
                );
                (v - bb_main).max(0.0)
            } else {
                f32::MAX
            }
        } else {
            if !child.style.max_height.is_none() && !child.style.max_height.is_auto() {
                let v = child.style.max_height.resolve_vp(
                    child_font,
                    0.0,
                    root_font_px,
                    engine.viewport_w,
                    engine.viewport_h,
                );
                (v - bb_main).max(0.0)
            } else {
                f32::MAX
            }
        };

        // Flexbox §4.5, the CONTENT-BASED MINIMUM SIZE. It is built from three
        // suggestions, all content-box:
        //   * the SPECIFIED size suggestion — the item's own `width`/`height`
        //     when it is definite. It CAPS the result, so an item that declares
        //     a size is never forced wider than it asked for. Reading the flex
        //     base size here instead was wrong whenever `flex-basis` and the
        //     size property disagree: `flex: 0 1 200px; width: 30px` refused to
        //     shrink below its 90px min-content instead of stopping at 30.
        //   * the TRANSFERRED size suggestion — a definite cross size sent
        //     through `aspect-ratio`.
        //   * the CONTENT size suggestion — the min-content size, which must
        //     ignore the item's own size or the cap above is a no-op.
        // A non-replaced item takes the LARGER of the content and transferred
        // suggestions; a replaced one the smaller. In both cases the result is
        // then clamped by a definite maximum main size.
        let specified_sugg: Option<f32> = {
            let len = if is_row {
                &child.style.width
            } else {
                &child.style.height
            };
            // A percentage against an indefinite container main size is not
            // definite, so it is no suggestion at all.
            let definite = !len.is_auto()
                && len.intrinsic().is_none()
                && !(len.has_percentage() && !is_row && rbox.content_height.is_none());
            if definite {
                let basis = if is_row {
                    content_w
                } else {
                    rbox.content_height.unwrap_or(0.0)
                };
                let v = len.resolve_vp(
                    child_font,
                    basis,
                    root_font_px,
                    engine.viewport_w,
                    engine.viewport_h,
                );
                Some((v - bb_main).max(0.0))
            } else {
                None
            }
        };
        let replaced_item = child.is_image_element();
        let auto_min_main = move |content_sugg: f32| -> f32 {
            let mut m = content_sugg;
            if let Some(t) = transferred_main {
                m = if replaced_item { m.min(t) } else { m.max(t) };
            }
            if let Some(sp) = specified_sugg {
                m = m.min(sp);
            }
            m.min(max_main).max(0.0)
        };

        let min_main: f32 = if is_row {
            if !child.style.min_width.is_auto() {
                let v = child.style.min_width.resolve_vp(
                    child_font,
                    content_w,
                    root_font_px,
                    engine.viewport_w,
                    engine.viewport_h,
                );
                (v - bb_main).max(0.0)
            } else if child.style.overflow_x != Overflow::Visible {
                // overflow: hidden/scroll/auto → automatic minimum is 0
                0.0
            } else {
                auto_min_main(engine.min_content_width_of_content(child, font_px, root_font_px))
            }
        } else {
            if !child.style.min_height.is_auto() {
                let v = child.style.min_height.resolve_vp(
                    child_font,
                    0.0,
                    root_font_px,
                    engine.viewport_w,
                    engine.viewport_h,
                );
                (v - bb_main).max(0.0)
            } else if child.style.overflow_y != Overflow::Visible {
                // overflow: hidden/scroll/auto → automatic minimum is 0
                0.0
            } else {
                // In a column the content size suggestion is the item's content
                // HEIGHT, which only a layout can report. Measuring it here puts
                // the minimum into `min_main`, where §9.7's min-violation loop
                // enforces it — the old deferred pass read the height back AFTER
                // the item had already been forced to its flexed size, so it
                // always saw that size and the minimum never applied.
                auto_min_main(measure_content_height(
                    engine,
                    child,
                    content_w,
                    content_x,
                    content_y,
                    font_px,
                    root_font_px,
                ))
            }
        };
        let hyp = basis_main.max(min_main).min(max_main);

        items.push(FlexItem {
            path: path.clone(),
            order: child.style.order,
            flex_grow: child.style.flex_grow,
            flex_shrink: child.style.flex_shrink,
            base_main: basis_main,
            hyp,
            min_main,
            max_main,
            outer_extra,
            main_used: hyp,
            cross_size: 0.0,
            baseline_off: None,
            main_pos: 0.0,
            cross_pos: 0.0,
            saved_width: child.style.width.clone(),
            saved_height: child.style.height.clone(),
            saved_display,
            auto_margins,
            saved_margins,
        });
    }

    // Sort by order (stable)
    items.sort_by(|a, b| a.order.cmp(&b.order));

    // Shrink-to-fit for inline-flex with auto width: content_w = sum of item intrinsic sizes
    if shrink_to_fit && is_row && !items.is_empty() {
        let total: f32 = items.iter().map(|i| i.hyp + i.outer_extra).sum::<f32>()
            + gap_main * items.len().saturating_sub(1) as f32;
        let max_w = (containing_w - rbox.h_space()).max(0.0);
        content_w = total.min(max_w);
        definite_main = Some(content_w);
    }

    let bb_h = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
        rbox.padding_top + rbox.padding_bottom + rbox.border_top + rbox.border_bottom
    } else {
        0.0
    };
    let flex_min_h = (node.style.min_height.resolve_vp(
        font_px,
        0.0,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    ) - bb_h)
        .max(0.0);
    let flex_max_h = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
        f32::MAX
    } else {
        (node.style.max_height.resolve_vp(
            font_px,
            0.0,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        ) - bb_h)
            .max(0.0)
    };

    if items.is_empty() {
        let ch = if let Some(h) = rbox.content_height {
            h
        } else if let Some(ratio) = node.style.aspect_ratio {
            if ratio > 0.0 {
                (content_w / ratio).max(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let ch = ch.max(flex_min_h).min(flex_max_h).max(0.0);
        finish_flex(node, rbox, content_x, content_y, content_w, ch);
        node.layout.collapsed_margin_top = rbox.margin_top;
        node.layout.collapsed_margin_bottom = rbox.margin_bottom;
        layout_abs_children(engine, node, font_px, root_font_px);
        return node.layout.margin_rect.h;
    }

    // ── Wrap into flex lines ──────────────────────────────────────────────────

    struct FlexLine {
        start: usize, // index into items
        count: usize,
        main_used: f32,
        cross_size: f32,
        cross_offset: f32,
    }

    let mut lines: Vec<FlexLine> = Vec::new();
    {
        let mut i = 0;
        while i < items.len() {
            let mut line = FlexLine {
                start: i,
                count: 0,
                main_used: 0.0,
                cross_size: 0.0,
                cross_offset: 0.0,
            };
            let mut count = 0usize;
            while i < items.len() {
                let item_outer = items[i].hyp + items[i].outer_extra;
                let total_with_gap =
                    line.main_used + (if count > 0 { gap_main } else { 0.0 }) + item_outer;
                // An INDEFINITE main size never forces a break: the line has
                // infinite room, so every item stays on it (Flexbox §9.2).
                if can_wrap && count > 0 && total_with_gap > definite_main.unwrap_or(f32::INFINITY)
                {
                    break;
                }
                line.main_used += (if count > 0 { gap_main } else { 0.0 }) + item_outer;
                count += 1;
                i += 1;
            }
            line.count = count;
            lines.push(line);
        }
    }

    // An indefinite main size becomes definite once the lines are known: it is
    // the largest line's main size, clamped by the container's own main-axis
    // min/max (Flexbox §9.2 step 4 + §9.9). Everything downstream — flexing,
    // `justify-content` free space — works from that resolved number.
    let effective_main_size = match definite_main {
        Some(m) => m,
        None => {
            let mut ms = 0.0f32;
            for line in &lines {
                ms = ms.max(line.main_used);
            }
            ms.max(col_min_main).min(col_max_main)
        }
    };

    // ── Resolve flexible lengths per line ─────────────────────────────────────

    for line in &lines {
        let free_space = effective_main_size - line.main_used;
        let growing = free_space > 0.0;
        let range = line.start..line.start + line.count;

        // CSS Flexbox §9.7. An item is frozen once its size is final: either it
        // has no flex factor on the active side, or its flex base size already
        // sits past the hypothetical size in the direction we would move it.
        let mut frozen: Vec<bool> = Vec::with_capacity(line.count);
        for idx in range.clone() {
            let it = &items[idx];
            let factor = if growing {
                it.flex_grow
            } else {
                it.flex_shrink
            };
            let past = if growing {
                it.base_main > it.hyp
            } else {
                it.base_main < it.hyp
            };
            frozen.push(factor <= 0.0 || past);
        }
        for idx in range.clone() {
            items[idx].main_used = items[idx].hyp;
        }

        // §9.7 step 4, the INITIAL free space: measured once, before any item
        // has flexed, and kept for the whole loop. Step 5b needs it to cap the
        // distribution when the flex factors sum to less than one.
        let initial_free = {
            let mut used = gap_main * line.count.saturating_sub(1) as f32;
            for (j, idx) in range.clone().enumerate() {
                used += items[idx].outer_extra
                    + if frozen[j] {
                        items[idx].main_used
                    } else {
                        items[idx].base_main
                    };
            }
            effective_main_size - used
        };

        // Repeat until every item is frozen: distribute the free space over the
        // unfrozen items, clamp each to its min/max, and freeze whichever items
        // violated their clamp in the direction of the total violation. Their
        // clamped size then feeds the next round's free space, so a maxed-out
        // item hands its surplus to its siblings instead of leaving overflow.
        while frozen.iter().any(|f| !f) {
            let mut used = 0.0f32;
            for (j, idx) in range.clone().enumerate() {
                used += items[idx].outer_extra
                    + if frozen[j] {
                        items[idx].main_used
                    } else {
                        items[idx].base_main
                    };
            }
            used += gap_main * line.count.saturating_sub(1) as f32;
            let mut free = effective_main_size - used;

            let total: f32 = range
                .clone()
                .enumerate()
                .filter(|(j, _)| !frozen[*j])
                .map(|(_, idx)| {
                    if growing {
                        items[idx].flex_grow
                    } else {
                        items[idx].flex_shrink * items[idx].base_main
                    }
                })
                .sum();

            // §9.7 step 5b: flex factors below one distribute only that
            // FRACTION of the initial free space, so `flex: 0.5 0 0` in a 300px
            // row takes 150px and leaves the rest of the line empty rather than
            // swallowing everything the way a factor of 1 would.
            let factor_sum: f32 = range
                .clone()
                .enumerate()
                .filter(|(j, _)| !frozen[*j])
                .map(|(_, idx)| {
                    if growing {
                        items[idx].flex_grow
                    } else {
                        items[idx].flex_shrink
                    }
                })
                .sum();
            if factor_sum < 1.0 {
                let scaled = initial_free * factor_sum;
                if scaled.abs() < free.abs() {
                    free = scaled;
                }
            }

            let mut violation = 0.0f32;
            let mut min_violators: Vec<usize> = Vec::new();
            let mut max_violators: Vec<usize> = Vec::new();
            for (j, idx) in range.clone().enumerate() {
                if frozen[j] {
                    continue;
                }
                let unclamped = if total <= 0.0 {
                    items[idx].base_main
                } else if growing {
                    items[idx].base_main + free * items[idx].flex_grow / total
                } else {
                    items[idx].base_main
                        + free * (items[idx].flex_shrink * items[idx].base_main) / total
                };
                let clamped = unclamped
                    .max(items[idx].min_main)
                    .min(items[idx].max_main)
                    .max(0.0);
                items[idx].main_used = clamped;
                violation += clamped - unclamped;
                if clamped > unclamped {
                    min_violators.push(j);
                }
                if clamped < unclamped {
                    max_violators.push(j);
                }
            }

            if violation > 0.0 {
                for j in min_violators {
                    frozen[j] = true;
                }
            } else if violation < 0.0 {
                for j in max_violators {
                    frozen[j] = true;
                }
            } else {
                for f in frozen.iter_mut() {
                    *f = true;
                }
            }
        }
    }

    // ── Layout each item at its resolved main size, compute cross sizes ────────
    // Mirror C++: set item.box->style.width/height = {cssW/cssH, Px} before LayoutBox

    let parent_align = node.style.align_items;
    for item in &mut items {
        let child = child_mut(node, &item.path);

        // Pass resolved flex size via Constraints.forced_width/height.
        // No style mutation — forced dimensions override rbox.content_width/height.
        // Always pass CONTENT-BOX size (item.main_used) since forced_width/height
        // is applied directly to rbox.content_width which is always content-box.
        // The item's containing block is the flex container's CONTENT BOX, so
        // every percentage on the item — margin, padding, its own `width` —
        // resolves against `content_w`. Handing the item its own outer main
        // size instead made a percentage margin self-referential:
        // `margin-left: 10%` on a 150px item in a 300px row measured 18px
        // (10% of 180) where a browser gives 30.
        let (item_containing, forced_w, forced_h) = if is_row {
            (content_w, Some(item.main_used), None)
        } else {
            (content_w, None, Some(item.main_used))
        };

        engine.layout_box(
            child,
            &item_constraints(
                rbox.content_height,
                item_containing,
                content_x,
                content_y,
                font_px,
                root_font_px,
                forced_w,
                forced_h,
            ),
        );

        item.cross_size = if is_row {
            child.layout.margin_rect.h
        } else {
            child.layout.margin_rect.w
        };
        let wants_baseline = match child.style.align_self {
            AlignSelf::Baseline | AlignSelf::LastBaseline => true,
            AlignSelf::Auto => matches!(
                parent_align,
                AlignItems::Baseline | AlignItems::LastBaseline
            ),
            _ => false,
        };
        item.baseline_off = if is_row && wants_baseline {
            first_baseline_offset(child)
        } else {
            None
        };
    }

    // ── Compute line cross sizes ──────────────────────────────────────────────

    for line in &mut lines {
        // Baseline-aligned items are stacked so their baselines coincide, so
        // the line has to be tall enough for the deepest part above any
        // baseline plus the deepest part below any baseline (Flexbox §8.3).
        let mut above = 0.0f32;
        let mut below = 0.0f32;
        for j in 0..line.count {
            let cs = items[line.start + j].cross_size;
            if cs > line.cross_size {
                line.cross_size = cs;
            }
            if let Some(b) = items[line.start + j].baseline_off {
                above = above.max(b);
                below = below.max(cs - b);
            }
        }
        if above + below > line.cross_size {
            line.cross_size = above + below;
        }
    }

    // Flexbox §9.4 step 8: a SINGLE-line container with a definite cross size
    // hands that size to its one line. Taking the largest item instead meant an
    // item taller than the container grew the line to fit it, so `align-items`
    // had no free space to work with and `center` could not overflow the way
    // the spec asks.
    if !can_wrap && lines.len() == 1 {
        let definite_cross = if is_row {
            rbox.content_height
        } else {
            Some(content_w)
        };
        if let Some(cs) = definite_cross {
            lines[0].cross_size = cs;
        }
    }

    // ── Total cross size and align-content ────────────────────────────────────

    let total_cross: f32 = lines.iter().map(|l| l.cross_size).sum::<f32>()
        + gap_cross * (lines.len().saturating_sub(1)) as f32;

    let container_cross = if is_row {
        if let Some(h) = rbox.content_height {
            h
        } else if let Some(ratio) = node.style.aspect_ratio {
            if ratio > 0.0 {
                (content_w / ratio).max(total_cross)
            } else {
                total_cross
            }
        } else {
            total_cross
        }
    } else {
        content_w
    };

    let free_cross = container_cross - total_cross;
    // The cross axis takes the same overflow fallback as the main axis. This
    // clamped the free space to zero instead, so `align-content: center` and
    // `flex-end` did nothing at all once the lines overflowed, when they should
    // move the lines up past the container's cross-start edge.
    let ac_safe = node.style.align_safety & crate::css::property_defs::SAFETY_ALIGN_CONTENT != 0;
    let ac = if free_cross < 0.0 && ac_safe {
        AlignContent::FlexStart
    } else if free_cross < 0.0 {
        match node.style.align_content {
            AlignContent::SpaceBetween
            | AlignContent::SpaceAround
            | AlignContent::SpaceEvenly
            | AlignContent::Stretch => AlignContent::FlexStart,
            other => other,
        }
    } else {
        node.style.align_content
    };
    // `wrap-reverse` flips the cross-START edge (Flexbox §5.2), so the line
    // stack packs against the opposite side: `flex-start` puts it at the bottom
    // of a row container and `flex-end` at the top. Only the line ORDER was
    // being reversed, so both values packed the stack at the same edge.
    let ac = if wrap_reverse {
        match ac {
            AlignContent::FlexStart => AlignContent::FlexEnd,
            AlignContent::FlexEnd => AlignContent::FlexStart,
            other => other,
        }
    } else {
        ac
    };
    let (align_content_offset, extra_cross_gap) = if free_cross != 0.0 && !lines.is_empty() {
        match ac {
            AlignContent::Center => (free_cross / 2.0, 0.0),
            AlignContent::FlexEnd => (free_cross, 0.0),
            AlignContent::SpaceBetween => {
                if lines.len() > 1 {
                    (0.0, free_cross / (lines.len() - 1) as f32)
                } else {
                    (0.0, 0.0)
                }
            }
            AlignContent::SpaceAround => {
                let g = free_cross / lines.len() as f32;
                (g / 2.0, g)
            }
            AlignContent::SpaceEvenly => {
                let g = free_cross / (lines.len() + 1) as f32;
                (g, g)
            }
            AlignContent::Stretch => {
                let extra = free_cross / lines.len() as f32;
                for line in &mut lines {
                    line.cross_size += extra;
                }
                (0.0, 0.0)
            }
            AlignContent::FlexStart => (0.0, 0.0),
        }
    } else {
        (0.0, 0.0)
    };

    // ── Position items ────────────────────────────────────────────────────────

    let mut cross_offset = align_content_offset;

    for li in 0..lines.len() {
        let line_idx = if wrap_reverse {
            lines.len() - 1 - li
        } else {
            li
        };
        lines[line_idx].cross_offset = cross_offset;

        // Main-axis: total used
        let total_items_main: f32 = items
            [lines[line_idx].start..lines[line_idx].start + lines[line_idx].count]
            .iter()
            .map(|i| i.main_used + i.outer_extra)
            .sum();
        let total_gaps = gap_main * lines[line_idx].count.saturating_sub(1) as f32;
        let free_main = effective_main_size - total_items_main - total_gaps;

        // Check for explicit auto margins on main axis
        let has_main_auto = items
            [lines[line_idx].start..lines[line_idx].start + lines[line_idx].count]
            .iter()
            .any(|item| item.auto_margins[0] || item.auto_margins[1]);

        let (main_start, main_extra_gap) = if has_main_auto {
            // Auto margins on main axis absorb all free space,
            // overriding justify-content. Distribute evenly.
            let mut auto_count = 0usize;
            for j in 0..lines[line_idx].count {
                let am = items[lines[line_idx].start + j].auto_margins;
                if am[0] {
                    auto_count += 1;
                }
                if am[1] {
                    auto_count += 1;
                }
            }
            let auto_margin_size = if auto_count > 0 && free_main > 0.0 {
                free_main / auto_count as f32
            } else {
                0.0
            };

            let mut auto_pos = 0.0f32;
            for ii in 0..lines[line_idx].count {
                let idx = if is_reversed {
                    lines[line_idx].start + lines[line_idx].count - 1 - ii
                } else {
                    lines[line_idx].start + ii
                };
                if items[idx].auto_margins[0] {
                    auto_pos += auto_margin_size;
                }
                items[idx].main_pos = auto_pos;
                auto_pos += items[idx].main_used + items[idx].outer_extra;
                if items[idx].auto_margins[1] {
                    auto_pos += auto_margin_size;
                }
                auto_pos += gap_main;
            }
            (0.0, gap_main) // signal that main_pos was already set
        } else {
            justify_spacing(
                node.style.justify_content,
                free_main,
                lines[line_idx].count,
                gap_main,
                is_reversed,
                node.style.align_safety & crate::css::property_defs::SAFETY_JUSTIFY_CONTENT != 0,
            )
        };

        let mut main_pos = main_start;

        for ii in 0..lines[line_idx].count {
            let item_idx = if is_reversed {
                lines[line_idx].start + lines[line_idx].count - 1 - ii
            } else {
                lines[line_idx].start + ii
            };

            let lc = lines[line_idx].cross_size;
            // The line's deepest baseline, and how tall the baseline-sharing
            // group is once every item hangs from it.
            let (line_baseline, line_baseline_group_depth) = {
                let mut above = 0.0f32;
                let mut below = 0.0f32;
                let mut any = false;
                for j in 0..lines[line_idx].count {
                    let it = &items[lines[line_idx].start + j];
                    if let Some(b) = it.baseline_off {
                        above = above.max(b);
                        below = below.max(it.cross_size - b);
                        any = true;
                    }
                }
                if any {
                    (Some(above), above + below)
                } else {
                    (None, 0.0)
                }
            };
            let eff_align = effective_align_self(
                child_ref(node, &items[item_idx].path),
                node.style.align_items,
                wrap_reverse,
            );

            // Cross-axis: check auto margins (overrides align-items/align-self)
            let cross_start_auto = items[item_idx].auto_margins[2];
            let cross_end_auto = items[item_idx].auto_margins[3];

            // Cross-axis alignment
            let cross_extra = items[item_idx].cross_size;
            let item_baseline = items[item_idx].baseline_off;
            let cross_pos = if let (Some(lb), Some(ib)) = (line_baseline, item_baseline) {
                // Shift the item down so its own baseline sits on the line's.
                let pos = (lb - ib).max(0.0);
                // `last baseline` aligns the same way, then packs the whole
                // baseline-sharing group against the line's cross-END edge
                // (Box Alignment §4.1) instead of leaving it at the start.
                if eff_align == AlignItems::LastBaseline {
                    pos + (lc - line_baseline_group_depth).max(0.0)
                } else {
                    pos
                }
            } else if cross_start_auto || cross_end_auto {
                // Auto margins on cross axis absorb extra space
                let extra = lc - cross_extra;
                if extra > 0.0 {
                    if cross_start_auto && cross_end_auto {
                        extra / 2.0
                    } else if cross_end_auto {
                        0.0
                    } else {
                        extra
                    }
                } else {
                    0.0
                }
            } else if eff_align == AlignItems::Stretch {
                // Stretch: re-layout with explicit cross-axis size, mirrors C++
                let child = child_mut(node, &items[item_idx].path);
                let child_font = child.style.font_size_px(font_px, root_font_px);
                let irb = engine.res_box(&child.style, child_font, content_w, root_font_px);
                let item_containing = content_w;
                if is_row {
                    // Stretch cross-axis (height) via forced_height (content-box)
                    let cross_extra = irb.padding_top
                        + irb.padding_bottom
                        + irb.border_top
                        + irb.border_bottom
                        + irb.margin_top
                        + irb.margin_bottom;
                    let target_h = (lc - cross_extra).max(0.0);
                    // ⛔ `stretch` applies ONLY when the item's cross size is
                    // `auto` (Flexbox §5.2 / §9.4). This stretched regardless,
                    // so `<i style="height:20px">` in a 60px-tall flex row came
                    // out 60 tall — the declared height was simply discarded.
                    let cross_is_auto = child.style.height.is_auto();
                    if cross_is_auto
                        && target_h > 0.0
                        && (target_h - child.layout.content_rect.h).abs() > 0.5
                    {
                        child.layout.layout_dirty = true;
                        // ⛔ Keep the flex-resolved MAIN size. This passed
                        // `None` for the forced width, so the re-layout fell
                        // back to the item's own `width`, undoing grow and
                        // shrink. A `display:flex` row with a definite HEIGHT
                        // — which is what makes stretch re-lay at all — did not
                        // shrink its items: a 400px item in a 300px row stayed
                        // 400 instead of becoming 250.
                        engine.layout_box(
                            child,
                            &item_constraints(
                                rbox.content_height,
                                item_containing,
                                content_x,
                                content_y,
                                font_px,
                                root_font_px,
                                Some(items[item_idx].main_used),
                                Some(target_h),
                            ),
                        );
                        items[item_idx].cross_size = child.layout.margin_rect.h;
                    }
                } else if !is_row {
                    // Stretch cross-axis (width) via forced_width (content-box)
                    let cross_extra = irb.padding_left
                        + irb.padding_right
                        + irb.border_left
                        + irb.border_right
                        + irb.margin_left
                        + irb.margin_right;
                    let stretch_w = (lc - cross_extra).max(0.0);
                    // The same, mirrored: in a column the cross axis is width.
                    let cross_is_auto = child.style.width.is_auto();
                    if cross_is_auto
                        && stretch_w > 0.0
                        && (stretch_w - child.layout.content_rect.w).abs() > 0.5
                    {
                        child.layout.layout_dirty = true;
                        // The same, mirrored: in a COLUMN the main axis is the
                        // height, and passing `None` for it discarded the
                        // resolved main size.
                        engine.layout_box(
                            child,
                            &item_constraints(
                                rbox.content_height,
                                stretch_w + cross_extra - irb.margin_left - irb.margin_right,
                                content_x,
                                content_y,
                                font_px,
                                root_font_px,
                                Some(stretch_w),
                                Some(items[item_idx].main_used),
                            ),
                        );
                        items[item_idx].cross_size = child.layout.margin_rect.w;
                    }
                }
                // ⛔ The stretch branch's own position. It returned a hardcoded
                // 0.0 — the line's cross-START — which `wrap-reverse` flips to
                // the far edge (Flexbox §5.2). An item that actually stretched
                // has `cross_size == lc`, so this stays 0 for it; one with a
                // definite cross size sits at the flipped edge, which is what
                // Chrome does.
                if wrap_reverse {
                    (lc - items[item_idx].cross_size).max(0.0)
                } else {
                    0.0
                }
            } else {
                // For column direction with Center/FlexStart/FlexEnd: shrink auto-width
                // children to their intrinsic (max-content) width, matching browser behavior.
                if !is_row {
                    let child_width_is_auto =
                        child_ref(node, &items[item_idx].path).style.width.is_auto();
                    if child_width_is_auto {
                        let child = child_mut(node, &items[item_idx].path);
                        let intrinsic_w = engine
                            .max_content_width(child, font_px, root_font_px)
                            .min(content_w);
                        if intrinsic_w < items[item_idx].cross_size - 0.5 {
                            // ⛔ Keep the flex-resolved MAIN size. Shrinking the
                            // item to its intrinsic width re-lays it out, and
                            // without the forced height that re-layout fell back
                            // to the item's own content height — so `flex: 1` in
                            // a 90px column produced 20px items, not 45px ones.
                            engine.layout_box(
                                child,
                                &item_constraints(
                                    rbox.content_height,
                                    intrinsic_w,
                                    content_x,
                                    content_y,
                                    font_px,
                                    root_font_px,
                                    None,
                                    Some(items[item_idx].main_used),
                                ),
                            );
                            items[item_idx].cross_size = child.layout.margin_rect.w;
                        }
                    }
                }
                let cross_extra = items[item_idx].cross_size;
                // `safe` on the cross axis, same rule as the main axis: once
                // the item is taller than its line, the alignment gives way to
                // the cross-start edge rather than overflowing it.
                let safe = {
                    let ci = child_ref(node, &items[item_idx].path);
                    let bit = if matches!(ci.style.align_self, AlignSelf::Auto) {
                        crate::css::property_defs::SAFETY_ALIGN_ITEMS
                    } else {
                        crate::css::property_defs::SAFETY_ALIGN_SELF
                    };
                    let style = if matches!(ci.style.align_self, AlignSelf::Auto) {
                        &node.style
                    } else {
                        &ci.style
                    };
                    style.align_safety & bit != 0
                };
                if safe && cross_extra > lc {
                    return_cross_start(wrap_reverse, lc, cross_extra)
                } else {
                    match eff_align {
                        AlignItems::FlexEnd => lc - cross_extra,
                        AlignItems::Center => (lc - cross_extra) / 2.0,
                        // ⛔ `FlexStart` has ALREADY been flipped by
                        // `effective_align_self` when `wrap-reverse` is on, so it
                        // must NOT be flipped again here — doing so made
                        // `flex-start` and `flex-end` land in the same place.
                        AlignItems::FlexStart => 0.0,
                        // The `stretch` FALLBACK, for an item with a definite cross
                        // size that cannot stretch. `stretch` is not flipped by
                        // `effective_align_self` (it is not an edge), so the flip
                        // belongs here: `wrap-reverse` moves the cross-START to the
                        // far edge (Flexbox §5.2). An item that really did stretch
                        // has `cross_extra == lc`, so this is 0 either way for it.
                        _ => 0.0,
                    }
                }
            };

            if !has_main_auto {
                items[item_idx].main_pos = main_pos;
            }
            items[item_idx].cross_pos = cross_offset + cross_pos;

            if !has_main_auto {
                main_pos +=
                    items[item_idx].main_used + items[item_idx].outer_extra + main_extra_gap;
            }
        }

        cross_offset += lines[line_idx].cross_size + gap_cross + extra_cross_gap;
    }

    // ── Set final child positions ─────────────────────────────────────────────

    for item in &items {
        let child = child_mut(node, &item.path);
        let (target_x, target_y) = if is_row {
            (content_x + item.main_pos, content_y + item.cross_pos)
        } else {
            (content_x + item.cross_pos, content_y + item.main_pos)
        };
        // Shift so that margin_rect origin is at (target_x, target_y)
        let dx = target_x - child.layout.margin_rect.x;
        let dy = target_y - child.layout.margin_rect.y;
        shift_rects(child, dx, dy);

        // Apply relative offset if position:relative
        if matches!(child.style.position, Position::Relative | Position::Sticky) {
            let child_font = child.style.font_size_px(font_px, root_font_px);
            crate::layout::block::apply_relative_offset(child, child_font, content_w, root_font_px);
        }
    }

    // ── Restore original CSS dimensions so re-layout works correctly ──────────
    // Mirrors C++: item.box->style.width = item.savedWidth; etc.

    for item in &items {
        let sw = item.saved_width.clone();
        let sh = item.saved_height.clone();
        let sd = item.saved_display;
        std::sync::Arc::make_mut(&mut child_mut(node, &item.path).style).width = sw;
        std::sync::Arc::make_mut(&mut child_mut(node, &item.path).style).height = sh;
        std::sync::Arc::make_mut(&mut child_mut(node, &item.path).style).display = sd;
        if let Some(m) = &item.saved_margins {
            let st = std::sync::Arc::make_mut(&mut child_mut(node, &item.path).style);
            st.margin_top = m[0].clone();
            st.margin_right = m[1].clone();
            st.margin_bottom = m[2].clone();
            st.margin_left = m[3].clone();
        }
    }

    // ── Content height ────────────────────────────────────────────────────────

    let content_h = if let Some(h) = rbox.content_height {
        h
    } else if let Some(ratio) = node.style.aspect_ratio {
        // Derive height from width via aspect-ratio when no explicit height is set
        if ratio > 0.0 {
            (content_w / ratio).max(0.0)
        } else {
            0.0
        }
    } else if is_row {
        // Cross axis = height, which is cross_offset minus trailing gap
        let used = cross_offset - if lines.is_empty() { 0.0 } else { gap_cross };
        used.max(0.0)
    } else {
        // Column: main axis is vertical
        let mut max_main = 0.0f32;
        for item in &items {
            let end = item.main_pos + item.main_used + item.outer_extra;
            if end > max_main {
                max_main = end;
            }
        }
        max_main.max(0.0)
    };

    let content_h = content_h.max(flex_min_h).min(flex_max_h).max(0.0);
    finish_flex(node, rbox, content_x, content_y, content_w, content_h);

    node.layout.collapsed_margin_top = rbox.margin_top;
    node.layout.collapsed_margin_bottom = rbox.margin_bottom;
    node.layout.layout_dirty = false;

    layout_abs_children(engine, node, font_px, root_font_px);
    node.layout.margin_rect.h
}

// ─── justify-content spacing ─────────────────────────────────────────────────

/// Offset of the first item from the main-START edge, and the gap between
/// items.
///
/// ⛔ `reversed` matters. In `row-reverse` and `column-reverse` the main-START
/// is the far edge (Flexbox §5.1), so `flex-start` packs against the RIGHT (or
/// bottom) and `flex-end` against the left (or top) — the two swap. The item
/// ORDER was already being reversed, but the packing was not, so a
/// `row-reverse` row put its items at the left edge in reverse order: measured
/// 50/0 where Chrome gives 250/200.
///
/// The symmetric values — space-between, -around, -evenly and center — are
/// unaffected by which end is the start.
fn justify_spacing(
    jc: JustifyContent,
    free: f32,
    n: usize,
    base_gap: f32,
    reversed: bool,
    safe: bool,
) -> (f32, f32) {
    if n == 0 {
        return (0.0, base_gap);
    }
    // The distribution values have a fallback for the overflow case
    // (CSS Box Alignment §4.4): `space-between` behaves as `flex-start`,
    // `space-around` and `space-evenly` as `center`. Without it a negative
    // free space became negative spacing and the items overlapped.
    // The distribution values have a fallback for the overflow case (CSS Box
    // Alignment §4.4): `space-between` behaves as `flex-start`, and
    // `space-around` / `space-evenly` as SAFE `center`, which is itself
    // `flex-start` once the free space is negative. An explicit `center` or
    // `flex-end` is unsafe and does overflow, so it is left alone.
    let jc = if free < 0.0 {
        // An explicit `safe` asks for the fallback on ANY position value, not
        // just the distributions (Box Alignment §4.4).
        if safe {
            JustifyContent::FlexStart
        } else {
            match jc {
                JustifyContent::SpaceBetween
                | JustifyContent::SpaceAround
                | JustifyContent::SpaceEvenly => JustifyContent::FlexStart,
                other => other,
            }
        }
    } else {
        jc
    };
    let jc = if reversed {
        match jc {
            JustifyContent::FlexStart => JustifyContent::FlexEnd,
            JustifyContent::FlexEnd => JustifyContent::FlexStart,
            other => other,
        }
    } else {
        jc
    };
    match jc {
        JustifyContent::FlexStart => (0.0, base_gap),
        JustifyContent::FlexEnd => (free, base_gap),
        // Physical, so they do NOT follow the walk direction: `main_start` is
        // the leftmost item's offset either way, which makes 0 the left edge
        // and `free` the right edge whether or not the axis is reversed.
        JustifyContent::Left => (0.0, base_gap),
        JustifyContent::Right => (free, base_gap),
        JustifyContent::Center => (free / 2.0, base_gap),
        JustifyContent::SpaceBetween => {
            if n > 1 {
                (0.0, base_gap + free / (n - 1) as f32)
            } else {
                // Box Alignment: with one item, space-between falls back to
                // flex-start. In a reversed main axis, flex-start is the far edge.
                (if reversed { free } else { 0.0 }, base_gap)
            }
        }
        JustifyContent::SpaceAround => {
            let s = free / n as f32;
            (s / 2.0, base_gap + s)
        }
        JustifyContent::SpaceEvenly => {
            let s = free / (n + 1) as f32;
            (s, base_gap + s)
        }
    }
}

// ─── effective align-self ─────────────────────────────────────────────────────

/// The cross-axis alignment an item actually uses.
///
/// ⛔ `wrap_reverse` matters. It flips the CROSS-START edge (Flexbox §5.2), so
/// `flex-start` aligns to the BOTTOM of the line and `flex-end` to the top —
/// they swap, exactly as `flex-start`/`flex-end` swap on the main axis under
/// `row-reverse`. The LINE order was already being reversed and the item
/// alignment inside each line was not, so a `wrap-reverse` container stacked
/// its lines correctly and then put every item against the wrong edge of its
/// line: measured 30/0 where Chrome gives 40/10.
/// Distance from a flex item's margin-box top to its first line box's
/// baseline (Flexbox §8.5). A box with no line box of its own has no baseline
/// to share, so `None` sends the item back to `flex-start`.
/// Constraints for laying out one flex item. A percentage on the item resolves
/// against the flex container's inner size (Flexbox §9.8), so a definite
/// container height has to travel with the item — `Constraints::with_forced`
/// leaves the available height `None`, which made `height: 50%` resolve to 0.
fn item_constraints(
    available_h: Option<f32>,
    available_width: f32,
    x: f32,
    y: f32,
    parent_font_px: f32,
    root_font_px: f32,
    forced_width: Option<f32>,
    forced_height: Option<f32>,
) -> Constraints {
    let mut c = Constraints::with_forced(
        available_width,
        x,
        y,
        parent_font_px,
        root_font_px,
        forced_width,
        forced_height,
    );
    c.available_height = available_h;
    c
}

/// The line's cross-START edge for an item, which `wrap-reverse` moves to the
/// far side (Flexbox §5.2).
fn return_cross_start(wrap_reverse: bool, lc: f32, cross_extra: f32) -> f32 {
    if wrap_reverse {
        lc - cross_extra
    } else {
        0.0
    }
}

fn first_baseline_offset(node: &WebCore) -> Option<f32> {
    if let Some(line) = node.layout.line_cache.first() {
        if line.height > 0.0 {
            return Some(line.y + line.ascent - node.layout.margin_rect.y);
        }
    }
    for ch in &node.children {
        if ch.style.position == Position::Absolute || ch.style.position == Position::Fixed {
            continue;
        }
        if let Some(off) = first_baseline_offset(ch) {
            return Some(off + ch.layout.margin_rect.y - node.layout.margin_rect.y);
        }
    }
    None
}

fn effective_align_self(
    child: &WebCore,
    parent_align: AlignItems,
    wrap_reverse: bool,
) -> AlignItems {
    let a = effective_align_self_inner(child, parent_align);
    if wrap_reverse {
        match a {
            AlignItems::FlexStart => AlignItems::FlexEnd,
            AlignItems::FlexEnd => AlignItems::FlexStart,
            other => other,
        }
    } else {
        a
    }
}

fn effective_align_self_inner(child: &WebCore, parent_align: AlignItems) -> AlignItems {
    match child.style.align_self {
        AlignSelf::Auto => parent_align,
        AlignSelf::Stretch => AlignItems::Stretch,
        AlignSelf::FlexStart => AlignItems::FlexStart,
        AlignSelf::FlexEnd => AlignItems::FlexEnd,
        AlignSelf::Center => AlignItems::Center,
        AlignSelf::Baseline => AlignItems::Baseline,
        AlignSelf::LastBaseline => AlignItems::LastBaseline,
    }
}

// ─── finish: set box rects ───────────────────────────────────────────────────

fn finish_flex(
    node: &mut WebCore,
    rbox: &ResolvedBox,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
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
    node.layout.margin_rect = Rect::new(
        node.layout.border_rect.x - rbox.margin_left,
        node.layout.border_rect.y - rbox.margin_top,
        node.layout.border_rect.w + rbox.margin_left + rbox.margin_right,
        node.layout.border_rect.h + rbox.margin_top + rbox.margin_bottom,
    );
    node.layout.baseline = content_y + content_h;

    node.layout.resolved_margin_top = rbox.margin_top;
    node.layout.resolved_margin_right = rbox.margin_right;
    node.layout.resolved_margin_bottom = rbox.margin_bottom;
    node.layout.resolved_margin_left = rbox.margin_left;
    node.layout.resolved_border_top = rbox.border_top;
    node.layout.resolved_border_right = rbox.border_right;
    node.layout.resolved_border_bottom = rbox.border_bottom;
    node.layout.resolved_border_left = rbox.border_left;
    node.layout.resolved_pad_top = rbox.padding_top;
    node.layout.resolved_pad_right = rbox.padding_right;
    node.layout.resolved_pad_bottom = rbox.padding_bottom;
    node.layout.resolved_pad_left = rbox.padding_left;
    node.layout.resolved_content_width = content_w;
    crate::layout::update_scroll_extents_from_children(
        node, content_x, content_y, content_w, content_h,
    );
}

fn layout_abs_children(engine: &LayoutEngine, node: &mut WebCore, font_px: f32, root_font_px: f32) {
    // CSS spec: containing block for absolutely positioned children is the padding box
    // of the nearest positioned ancestor.
    let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style) {
        node.layout.padding_rect
    } else {
        engine.pos_cb.get()
    };
    let justify_content = node.style.justify_content;
    let align_items = node.style.align_items;
    let child_paths = collect_flex_children(node);
    let abs_paths: Vec<Vec<usize>> = child_paths
        .into_iter()
        .filter(|p| {
            let c = child_ref(node, p);
            matches!(c.style.position, Position::Absolute | Position::Fixed)
        })
        .collect();
    for path in abs_paths {
        let parent_flex_direction = node.style.flex_direction;
        let parent_direction = node.style.direction;
        let child = child_mut(node, &path);
        layout_positioned(engine, child, containing_rect, font_px, root_font_px);

        // For absolutely-positioned children with all insets auto, apply the CSS
        // "static position" AFTER layout_positioned: where the item would go if it
        // were a normal flex item.  abs children are skipped by the flex pass so
        // layout_positioned leaves them at (0,0); we correct that here.
        let all_auto = child.style.left.is_auto()
            && child.style.right.is_auto()
            && child.style.top.is_auto()
            && child.style.bottom.is_auto();
        if all_auto && matches!(child.style.position, Position::Absolute) {
            let cw = child.layout.border_rect.w;
            let ch = child.layout.border_rect.h;
            let is_row = matches!(
                parent_flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );
            let is_rtl = parent_direction == crate::types::Direction::RTL;
            let is_reversed = matches!(
                parent_flex_direction,
                FlexDirection::RowReverse | FlexDirection::ColumnReverse
            ) ^ (is_row && is_rtl);

            let (target_x, target_y) = if is_row {
                let start_x = if is_reversed {
                    containing_rect.x + containing_rect.w - cw
                } else {
                    containing_rect.x
                };
                let end_x = if is_reversed {
                    containing_rect.x
                } else {
                    containing_rect.x + containing_rect.w - cw
                };
                let tx = match justify_content {
                    JustifyContent::Center => containing_rect.x + (containing_rect.w - cw) / 2.0,
                    JustifyContent::FlexEnd => end_x,
                    _ => start_x,
                };
                let ty = match effective_align_self_inner(child, align_items) {
                    AlignItems::Center => containing_rect.y + (containing_rect.h - ch) / 2.0,
                    AlignItems::FlexEnd => containing_rect.y + containing_rect.h - ch,
                    _ => containing_rect.y,
                };
                (tx, ty)
            } else {
                let start_y = if is_reversed {
                    containing_rect.y + containing_rect.h - ch
                } else {
                    containing_rect.y
                };
                let end_y = if is_reversed {
                    containing_rect.y
                } else {
                    containing_rect.y + containing_rect.h - ch
                };
                let ty = match justify_content {
                    JustifyContent::Center => containing_rect.y + (containing_rect.h - ch) / 2.0,
                    JustifyContent::FlexEnd => end_y,
                    _ => start_y,
                };
                let cross_align = effective_align_self_inner(child, align_items);
                let tx = match cross_align {
                    AlignItems::Center => containing_rect.x + (containing_rect.w - cw) / 2.0,
                    AlignItems::FlexEnd => {
                        if is_rtl {
                            containing_rect.x
                        } else {
                            containing_rect.x + containing_rect.w - cw
                        }
                    }
                    _ => {
                        if is_rtl {
                            containing_rect.x + containing_rect.w - cw
                        } else {
                            containing_rect.x
                        }
                    }
                };
                (tx, ty)
            };
            let dx = target_x - child.layout.border_rect.x;
            let dy = target_y - child.layout.border_rect.y;
            if dx != 0.0 || dy != 0.0 {
                crate::layout::shift_rects(child, dx, dy);
            }
        }
    }
}
