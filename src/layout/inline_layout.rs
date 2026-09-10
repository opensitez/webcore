use super::Constraints;
use crate::layout::block::collapse_two;
use crate::layout::text::resolve_bidi_line;
use crate::layout::{layout_positioned, FloatContext, FloatSide, LayoutEngine, ResolvedBox};
use crate::types::*;
use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping, Stretch, Style as CTextStyle, Weight};

/// Lay out a box whose children are inline-level (text runs, inline-block).
/// Returns total outer height of the box.
/// `float_ctx` is the float context from the containing block (may be None).
pub fn layout_inline_block(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    c: &Constraints,
    parent_float_ctx: Option<&mut FloatContext>,
) -> f32 {
    let containing_w = c.available_width;
    let x = c.x;
    let y = c.y;
    let font_px = c.parent_font_px;
    let root_font_px = c.root_font_px;
    // Create a local float context when this box establishes a BFC; otherwise
    // clone the parent context so inline content can avoid ancestor floats.
    let mut fc_owned = FloatContext::default();
    let establishes_own_float_context = crate::layout::block::establishes_bfc(&node.style);
    let has_parent_fc = parent_float_ctx.is_some() && !establishes_own_float_context;
    let mut float_ctx: Option<&mut FloatContext> = if establishes_own_float_context {
        Some(&mut fc_owned)
    } else if let Some(fc) = parent_float_ctx {
        fc_owned = fc.clone();
        Some(&mut fc_owned)
    } else {
        Some(&mut fc_owned)
    };

    let raw_w = match rbox.content_width {
        Some(w) => w,
        None => (containing_w - rbox.h_space()).max(0.0),
    };
    // CSS Sizing §5 — the intrinsic keywords, resolved here because this is
    // where the node is in hand. See the same branch in `block.rs`: they read
    // as `auto` to every caller that cannot measure content, so a
    // `width: min-content` box otherwise filled its containing block.
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

    // Apply min/max-width, converting from border-box to content-box when needed.
    // CSS: with box-sizing:border-box, min/max-width refer to the border box, not the content box.
    let bb_extra = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
        rbox.padding_left + rbox.padding_right + rbox.border_left + rbox.border_right
    } else {
        0.0
    };
    // An intrinsic keyword on min-/max-width names a CONTENT size, so it is
    // measured, not resolved, and `box_sizing` has nothing to convert. Same
    // rule as `block.rs`.
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

    // Auto margin centering (CSS 2.1 §10.3.3)
    let margin_left;
    let margin_right;
    let left_is_auto = node.style.margin_left.is_auto();
    let right_is_auto = node.style.margin_right.is_auto();
    if !node.style.width.is_auto() && (left_is_auto || right_is_auto) {
        let non_margin_space = rbox.border_left
            + rbox.padding_left
            + content_w
            + rbox.padding_right
            + rbox.border_right;
        let available = (containing_w - non_margin_space).max(0.0);
        if left_is_auto && right_is_auto {
            margin_left = (available / 2.0).floor();
            margin_right = available - margin_left;
        } else if left_is_auto {
            margin_left = available - rbox.margin_right;
            margin_right = rbox.margin_right;
        } else {
            margin_left = rbox.margin_left;
            margin_right = available - rbox.margin_left;
        }
    } else {
        margin_left = rbox.margin_left;
        margin_right = rbox.margin_right;
    }

    let content_x = x + margin_left + rbox.border_left + rbox.padding_left;
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    // Set float context origin now that content_y is known
    if let Some(ref mut fc) = float_ctx {
        if establishes_own_float_context || (fc.origin_y == 0.0 && fc.floats.is_empty()) {
            fc.origin_x = content_x;
            fc.origin_y = content_y;
        }
    }

    // ── 0. Pre-layout atomic inline-block and float children so sizes are known ──
    //    Mirrors C++ LayoutInlines(): LayoutBox(*run.atomicBox, …) before line-breaking
    //    Also recurse into inline children to pre-layout nested inline-blocks (e.g.
    //    <input type="radio"> inside a <label>).
    prelayout_nested_inline_blocks(engine, node, content_w, font_px, root_font_px);

    for ci in 0..node.children.len() {
        if matches!(node.children[ci].style.display, Display::Contents) {
            let child_font_px = node.children[ci].style.font_size_px(font_px, root_font_px);
            prelayout_nested_inline_blocks(
                engine,
                &mut node.children[ci],
                content_w,
                child_font_px,
                root_font_px,
            );
            continue;
        }
        if matches!(
            node.children[ci].style.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        ) || is_atomic_inline_replaced(&node.children[ci])
        {
            engine.layout_box(
                &mut node.children[ci],
                &Constraints::new(content_w, 0.0, 0.0, font_px, root_font_px),
            );
            // Shrink-to-fit for auto-width inline-block (CSS §10.3.9):
            // InlineBlock with width:auto should size to content, not expand to fill container.
            if node.children[ci].style.width.is_auto() {
                // Use line.width (raw text content width) not line.x + line.width - origin,
                // because line.x includes the text-align centering offset which inflates
                // the result when text-align:center is inherited.
                let max_line_w = node.children[ci]
                    .layout
                    .line_cache
                    .iter()
                    .map(|l| l.width)
                    .fold(0.0_f32, f32::max);
                // The first pass may have expanded fluid descendants (for
                // example a flex link inside a nav item) to the temporary
                // containing width. Prefer max-content when available; fall
                // back to the laid-out line width only for boxes whose
                // intrinsic walk has no useful answer.
                let max_content_w =
                    engine.max_content_width(&node.children[ci], font_px, root_font_px);
                let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                {
                    let irb = &node.children[ci];
                    let shrink_w = intrinsic_w.ceil()
                        + shrink_to_fit_slop(irb)
                        + irb.layout.resolved_pad_left
                        + irb.layout.resolved_pad_right
                        + irb.layout.resolved_border_left
                        + irb.layout.resolved_border_right
                        + irb.layout.resolved_margin_left
                        + irb.layout.resolved_margin_right;
                    if shrink_w < content_w {
                        engine.layout_box(
                            &mut node.children[ci],
                            &Constraints::new(shrink_w, 0.0, 0.0, font_px, root_font_px),
                        );
                    }
                }
            }
        } else if node.children[ci].style.is_inline_level()
            && has_in_flow_block_children(&node.children[ci])
        {
            // Inline element containing block-level children (e.g. <a><strong style="display:block">).
            // Per CSS, this creates an anonymous block formatting context. We approximate by
            // pre-laying the element out as a block container so its children get proper dimensions.
            engine.layout_box(
                &mut node.children[ci],
                &Constraints::new(content_w, 0.0, 0.0, font_px, root_font_px),
            );
        } else if !matches!(node.children[ci].style.float, crate::types::Float::None) {
            // Float children need to be laid out to get valid dimensions.
            engine.layout_box(
                &mut node.children[ci],
                &Constraints::new(content_w, content_x, content_y, font_px, root_font_px),
            );
            // Shrink-to-fit for auto-width floats
            if node.children[ci].style.width.is_auto() {
                let max_line_w = node.children[ci]
                    .layout
                    .line_cache
                    .iter()
                    .map(|line| line.width)
                    .fold(0.0_f32, f32::max);
                let max_content_w =
                    engine.max_content_width(&node.children[ci], font_px, root_font_px);
                let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                if intrinsic_w > 0.0 && intrinsic_w < content_w {
                    let irb = &node.children[ci];
                    let shrink_w = intrinsic_w.ceil()
                        + shrink_to_fit_slop(irb)
                        + irb.layout.resolved_pad_left
                        + irb.layout.resolved_pad_right
                        + irb.layout.resolved_border_left
                        + irb.layout.resolved_border_right
                        + irb.layout.resolved_margin_left
                        + irb.layout.resolved_margin_right;
                    engine.layout_box(
                        &mut node.children[ci],
                        &Constraints::new(shrink_w, content_x, content_y, font_px, root_font_px),
                    );
                }
            }
        }
    }

    // ── 1. Measure ::before / ::after pseudo-element widths ───────────────────
    let pseudo_font_px = |ps: Option<&ComputedStyle>| -> f32 {
        ps.and_then(|s| {
            let f = s.font_size.resolve(font_px, 0.0, root_font_px);
            if f > 0.0 {
                Some(f)
            } else {
                None
            }
        })
        .unwrap_or(font_px)
    };
    let scale = engine.scale;
    let font_system = unsafe { engine.font_system.map(|fs| &mut *fs) };
    let before_w = if !node.style.before_content.is_empty() {
        let bfpx = pseudo_font_px(node.style.before_style.as_deref());
        measure_text_width_scaled(&node.style.before_content, bfpx, font_system, scale)
    } else {
        0.0
    };
    let font_system = unsafe { engine.font_system.map(|fs| &mut *fs) };
    let after_w = if !node.style.after_content.is_empty() {
        let afpx = pseudo_font_px(node.style.after_style.as_deref());
        measure_text_width_scaled(&node.style.after_content, afpx, font_system, scale)
    } else {
        0.0
    };

    // ── 2. Collect flat inline items from all inline children ─────────────────
    let mut text_offset = 0usize;
    let mut items: Vec<InlineItem> = Vec::new();
    let mut runs: Vec<InlineRun> = Vec::new();
    // Pass the containing element's style to children for text-decoration and href
    let container_deco = if node.style.text_decoration.underline
        || node.style.text_decoration.overline
        || node.style.text_decoration.strikethrough
        || !node.style.href.is_empty()
    {
        Some(node.style.as_ref())
    } else {
        None
    };
    let generated_content = node.style.rare().content.as_str();
    if !generated_content.is_empty() {
        let mut tmp_node = WebCore::new("#text");
        tmp_node.text = generated_content.to_string();
        tmp_node.style = node.style.clone();
        collect_items(
            engine,
            &tmp_node,
            font_px,
            root_font_px,
            &mut items,
            &mut runs,
            &mut text_offset,
            0,
            false,
            &[],
        );
    } else {
        let mut previous_collapsible_space = false;
        for (i, child) in node.children.iter().enumerate() {
            if matches!(child.style.display, Display::None) {
                continue;
            }
            collect_items_inner(
                engine,
                child,
                font_px,
                root_font_px,
                &mut items,
                &mut runs,
                &mut text_offset,
                i,
                true,
                &[],
                container_deco,
                &mut previous_collapsible_space,
            );
        }
    }
    // Also collect from own text (text directly inside element)
    if generated_content.is_empty() && !node.text.is_empty() {
        if node.is_text_node() {
            // Text node laid out directly (e.g. as a flex child): collect self,
            // but skip whitespace-only nodes (handled by parent inline layout).
            if !node.text.chars().all(|c| c.is_ascii_whitespace()) {
                collect_items(
                    engine,
                    node,
                    font_px,
                    root_font_px,
                    &mut items,
                    &mut runs,
                    &mut text_offset,
                    0,
                    false,
                    &[],
                );
            }
        } else if !node.layout.inline_runs.is_empty() {
            // Block has pre-built inline runs (e.g. from the markdown parser).
            // Emit one #text item per run to preserve bold, italic, link color, etc.
            // The plain "node.style" path below would silently drop all inline formatting.
            let saved_runs = node.layout.inline_runs.clone();
            for run in &saved_runs {
                let end = (run.text_offset + run.length).min(node.text.len());
                if run.text_offset >= end {
                    continue;
                }
                let run_text = node.text[run.text_offset..end].to_string();
                let mut tmp = WebCore::new("#text");
                tmp.text = run_text;
                tmp.style = std::sync::Arc::new(run.style.clone());
                collect_items(
                    engine,
                    &tmp,
                    font_px,
                    root_font_px,
                    &mut items,
                    &mut runs,
                    &mut text_offset,
                    0,
                    false,
                    &[],
                );
            }
        } else {
            let mut tmp_node = WebCore::new("#text");
            tmp_node.text = node.text.clone();
            tmp_node.style = node.style.clone();
            collect_items(
                engine,
                &tmp_node,
                font_px,
                root_font_px,
                &mut items,
                &mut runs,
                &mut text_offset,
                0,
                false,
                &[],
            );
        }
    }

    // ── 3. Save old lines for early-stop optimization ─────────────────────────
    let old_lines: Vec<LayoutLine> = std::mem::take(&mut node.layout.line_cache);

    if items.is_empty() {
        // Nothing to lay out.
        // For non-void blocks with no children (e.g. empty <p> after Enter), add a
        // placeholder line so the caret has a home and the block has visible height.
        const VOID_TAGS: &[&str] = &[
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
            "source", "track", "wbr",
        ];
        let is_void = VOID_TAGS.contains(&node.tag.as_str());
        // Only add a placeholder line for elements that can hold inline/prose content
        // and need a visible cursor when empty. Generic structural divs/sections must
        // NOT get one — it would break margin collapsing for empty blocks.
        let is_prose_tag = matches!(
            node.tag.as_str(),
            "p" | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "li"
                | "dt"
                | "dd"
                | "pre"
                | "blockquote"
                | "td"
                | "th"
                | "caption"
        );
        let is_contenteditable = node
            .attributes
            .get("contenteditable")
            .map(|v| v == "true")
            .unwrap_or(false);
        let has_pseudo_content = before_w > 0.0 || after_w > 0.0;
        let add_placeholder = !is_void
            && node.children.is_empty()
            && rbox.content_height.is_none()
            && (is_prose_tag || is_contenteditable || has_pseudo_content);
        if add_placeholder {
            let pseudo_w = before_w + after_w;
            let ps_font = if has_pseudo_content {
                pseudo_font_px(
                    node.style
                        .before_style
                        .as_deref()
                        .or(node.style.after_style.as_deref()),
                )
            } else {
                font_px
            };
            let eff_fpx = if has_pseudo_content { ps_font } else { font_px };
            let line_h = eff_fpx * 1.2;
            node.layout.line_cache = vec![LayoutLine {
                text_start: text_offset,
                text_length: 0,
                x: content_x,
                y: content_y,
                width: pseudo_w,
                height: line_h,
                ascent: eff_fpx,
                descent: eff_fpx * 0.2,
                extra_space_per_word: 0.0,
                text_x_offset: 0.0,
                visual_segments: Vec::new(),
                char_x: Vec::new(),
                has_clamped_continuation: false,
                char_x_key: 0,
            }];
        }
        // <br> in block context (no inline siblings collected it as a Break item)
        // must still produce a line-height of vertical space, just like it would
        // inside a paragraph.
        let br_h = if node.tag == "br" { font_px * 1.2 } else { 0.0 };
        let eff_placeholder_fpx = if has_pseudo_content {
            pseudo_font_px(
                node.style
                    .before_style
                    .as_deref()
                    .or(node.style.after_style.as_deref()),
            )
        } else {
            font_px
        };
        let placeholder_h = if add_placeholder {
            eff_placeholder_fpx * 1.2
        } else {
            br_h
        };
        let min_h = engine.res_len(&node.style.min_height, font_px, 0.0, root_font_px);
        let max_h = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
            f32::MAX
        } else {
            let v = engine.res_len(&node.style.max_height, font_px, 0.0, root_font_px);
            // Percentage max-height against unknown (0) containing height → treat as none
            if v == 0.0 && matches!(node.style.max_height, CssLength::Percent(_)) {
                f32::MAX
            } else {
                v
            }
        };
        let content_h = if let Some(h) = rbox.content_height {
            h
        } else if let Some(ratio) = node.style.aspect_ratio {
            if ratio > 0.0 {
                (content_w / ratio).max(0.0).max(min_h).min(max_h)
            } else {
                placeholder_h
            }
        } else {
            placeholder_h
        };
        let content_h = content_h.max(min_h).min(max_h);
        set_box_rects(
            node,
            content_x,
            content_y,
            content_w,
            content_h,
            rbox,
            margin_left,
            margin_right,
        );
        node.layout.inline_runs = runs;
        // Still need to lay out absolutely/fixed positioned children.
        // Use collect_grid_children to flatten through display:contents,
        // matching block layout behaviour (CSS §2.7).
        let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style)
        {
            node.layout.padding_rect
        } else {
            engine.pos_cb.get()
        };
        let eff = crate::layout::grid::collect_grid_children(node);
        let abs_paths: Vec<Vec<usize>> = eff
            .into_iter()
            .filter(|path| {
                let c = crate::layout::grid::grid_child_ref(node, path);
                matches!(c.style.position, Position::Absolute | Position::Fixed)
            })
            .collect();
        for path in &abs_paths {
            let child = crate::layout::grid::grid_child_mut(node, path);
            layout_positioned(engine, child, containing_rect, font_px, root_font_px);
            // All-auto correction: when no insets are specified, place at containing block's origin.
            let child = crate::layout::grid::grid_child_mut(node, path);
            let all_auto = child.style.left.is_auto()
                && child.style.right.is_auto()
                && child.style.top.is_auto()
                && child.style.bottom.is_auto();
            if all_auto && matches!(child.style.position, Position::Absolute) {
                let dx = containing_rect.x - child.layout.border_rect.x;
                let dy = containing_rect.y - child.layout.border_rect.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    crate::layout::shift_rects(child, dx, dy);
                }
            }
        }
        return node.layout.margin_rect.h;
    }

    // ── 4. Paragraph cache: skip line-breaking if text + width unchanged ────
    // If content hasn't changed since last layout, the old line_cache is valid.
    // Just reposition and return.
    if !old_lines.is_empty()
        && !node.layout.layout_dirty
        && (node.layout.last_containing_width - content_w).abs() < 0.5
        && float_ctx.is_none()
    {
        // Reuse old lines — just shift to new position
        node.layout.line_cache = old_lines;
        let dy = content_y - node.layout.line_cache.first().map_or(content_y, |l| l.y);
        if dy.abs() > 0.01 {
            for line in &mut node.layout.line_cache {
                line.y += dy;
            }
        }
        let bottom = node
            .layout
            .line_cache
            .last()
            .map(|l| l.y + l.height)
            .unwrap_or(content_y);
        let raw_h = (bottom - content_y).max(0.0);
        let content_h = match rbox.content_height {
            Some(h) => h,
            None => raw_h,
        };
        let min_h = engine.res_len(&node.style.min_height, font_px, 0.0, root_font_px);
        let max_h = if node.style.max_height.is_none() || node.style.max_height.is_auto() {
            f32::MAX
        } else {
            engine.res_len(&node.style.max_height, font_px, 0.0, root_font_px)
        };
        let content_h = content_h.max(min_h).min(max_h).max(0.0);
        crate::layout::block::build_box_rects(
            node,
            rbox,
            content_x,
            content_y,
            content_w,
            content_h,
            margin_left,
            margin_right,
        );
        if let Some(last_line) = node.layout.line_cache.last() {
            node.layout.baseline = last_line.y + last_line.ascent;
        }
        node.layout.layout_dirty = false;
        node.layout.last_containing_width = c.available_width;
        return node.layout.margin_rect.h;
    }

    // ── Line-by-line layout (float-aware) ────────────────────────────────────
    let floats_before = float_ctx.as_ref().map_or(0, |fc| fc.floats.len());
    let text_indent = engine.res_len(&node.style.text_indent, font_px, content_w, root_font_px);
    let is_rtl = node.style.direction == Direction::RTL;

    let balance_inline_width = if matches!(node.style.text_wrap.as_str(), "balance" | "pretty")
        && !matches!(node.style.white_space, WhiteSpace::Pre | WhiteSpace::Nowrap)
        && text_indent.abs() < 0.01
        && before_w <= 0.01
        && after_w <= 0.01
        && floats_before == float_ctx.as_ref().map_or(0, |fc| fc.floats.len())
    {
        Some(if node.style.text_wrap == "pretty" {
            pretty_wrap_width(&items, content_w)
        } else {
            balanced_wrap_width(&items, content_w)
        })
    } else {
        None
    };

    let mut cursor_y = content_y;
    let mut item_idx = 0usize;
    let mut line_cache: Vec<LayoutLine> = Vec::new();
    let mut atomic_pos: Vec<(Vec<usize>, f32, f32)> = Vec::new(); // (path, x, y)
    let mut inline_fragment_rects: std::collections::HashMap<Vec<usize>, Rect> =
        std::collections::HashMap::new();
    let mut old_line_idx = 0usize;
    let mut ends_with_break = false;
    let mut loop_guard = 0usize;

    while item_idx < items.len() {
        loop_guard += 1;
        if loop_guard > 10000 {
            break;
        }
        let is_first_line = line_cache.is_empty();

        // ── Place leading floats before current line ──────────────────────────
        while item_idx < items.len() {
            if let InlineItemKind::Float { path } = &items[item_idx].kind {
                if let Some(ref mut fc) = float_ctx {
                    let Some(child) = resolve_path_mut(node, path) else {
                        item_idx += 1;
                        continue;
                    };
                    let float_w = (child.layout.border_rect.w
                        + child.layout.resolved_margin_left
                        + child.layout.resolved_margin_right)
                        .max(0.0);
                    let float_h = child.layout.margin_rect.h;
                    let side = if child.style.float == crate::types::Float::Right {
                        FloatSide::Right
                    } else {
                        FloatSide::Left
                    };
                    let local_x = content_x - fc.origin_x;
                    let placed = fc.place_float_in(
                        local_x,
                        cursor_y - fc.origin_y,
                        float_w,
                        float_h,
                        content_w,
                        side,
                        &child.style.shape_outside,
                        child.style.shape_margin.resolve(
                            child.style.font_size_px(font_px, root_font_px),
                            content_w,
                            root_font_px,
                        ),
                    );
                    let dx = content_x + placed.x - child.layout.margin_rect.x;
                    let dy = fc.origin_y + placed.y - child.layout.margin_rect.y;
                    crate::layout::shift_rects(child, dx, dy);
                }
                item_idx += 1;
            } else {
                break;
            }
        }

        // Query float context for available horizontal band at this Y
        let est_line_h = font_px * 1.2;
        let (mut fc_left, mut fc_right) = (0.0f32, content_w);
        if let Some(fc) = float_ctx.as_ref() {
            let local_x = content_x - fc.origin_x;
            fc.available_width_in(
                local_x,
                cursor_y - fc.origin_y,
                est_line_h,
                content_w,
                &mut fc_left,
                &mut fc_right,
            );
        }

        // Apply text-indent and ::before on first line
        if is_first_line {
            fc_left += text_indent;
            fc_left += before_w;
        }

        // white-space:pre and nowrap never wrap at word boundaries.
        // avail_w controls line-breaking; align_w is used for text alignment
        // (always finite so right/center alignment doesn't overflow).
        let finite_w = (fc_right - fc_left).max(0.0);
        let mut avail_w = if matches!(node.style.white_space, WhiteSpace::Pre | WhiteSpace::Nowrap)
        {
            f32::MAX
        } else {
            finite_w
        };
        if let Some(balance_w) = balance_inline_width {
            avail_w = avail_w.min(balance_w).max(1.0);
        }
        let align_w = finite_w;

        // Break items for this line
        let (mut line_end, mut next_start, mut was_break) =
            break_one_line(&items, item_idx, avail_w);

        // Greedily pull in and place floats that were included in the line slice
        // If a float placement narrows the width such that items no longer fit,
        // we must re-break the line.
        let mut i = item_idx;
        while i < line_end {
            if let InlineItemKind::Float { path } = &items[i].kind {
                if let Some(ref mut fc) = float_ctx {
                    let Some(child) = resolve_path_mut(node, path) else {
                        i += 1;
                        continue;
                    };
                    let float_w = (child.layout.border_rect.w
                        + child.layout.resolved_margin_left
                        + child.layout.resolved_margin_right)
                        .max(0.0);
                    let float_h = child.layout.margin_rect.h;
                    let side = if child.style.float == crate::types::Float::Right {
                        FloatSide::Right
                    } else {
                        FloatSide::Left
                    };
                    let local_x = content_x - fc.origin_x;
                    let placed = fc.place_float_in(
                        local_x,
                        cursor_y - fc.origin_y,
                        float_w,
                        float_h,
                        content_w,
                        side,
                        &child.style.shape_outside,
                        child.style.shape_margin.resolve(
                            child.style.font_size_px(font_px, root_font_px),
                            content_w,
                            root_font_px,
                        ),
                    );
                    let dx = content_x + placed.x - child.layout.margin_rect.x;
                    let dy = fc.origin_y + placed.y - child.layout.margin_rect.y;
                    crate::layout::shift_rects(child, dx, dy);

                    // Width might have changed
                    let local_x = content_x - fc.origin_x;
                    fc.available_width_in(
                        local_x,
                        cursor_y - fc.origin_y,
                        est_line_h,
                        content_w,
                        &mut fc_left,
                        &mut fc_right,
                    );
                    let temp_fc_left = if is_first_line {
                        fc_left + text_indent + before_w
                    } else {
                        fc_left
                    };
                    avail_w = (fc_right - temp_fc_left).max(0.0);

                    // Re-evaluate line break from THIS point forward
                    let (new_end, new_next, new_break) = break_one_line(&items, i + 1, avail_w);
                    line_end = new_end;
                    next_start = new_next;
                    was_break = new_break;
                }
            }
            i += 1;
        }

        ends_with_break = was_break;

        if line_end == item_idx && !was_break {
            // Safety: avoid infinite loop if no progress made
            if item_idx < items.len()
                && matches!(items[item_idx].kind, InlineItemKind::Float { .. })
            {
                item_idx += 1;
                continue;
            }
            break;
        }

        let line_items = &items[item_idx..line_end];

        // Compute line metrics from the items and the STRUT.
        //
        // CSS 2.1 §10.8: every line box contains a strut — a zero-width inline
        // box carrying the block's own font and line-height — whose ascent and
        // descent take part in the line box's height. Without it a line holding
        // only atomic inlines had no room below the baseline at all: a 20px
        // image or inline-block produced a 20px line where a browser gives 25.
        // The `line-height` was instead bolted on afterwards, and only ever to
        // GROW the line, so it could not shrink one either.
        let (strut_asc, strut_desc) = strut_metrics(engine, node, font_px, root_font_px);
        let (raw_h, line_asc, mut line_desc) = measure_metrics(line_items, strut_asc, strut_desc);
        // A line box occupies whole pixels, which is what a browser reports:
        // half-leading lands on a half pixel, and leaving it there made every
        // line half a pixel short — invisible on one line and a drift of one
        // pixel per two lines down a page. Rounded, not ceiled: the font
        // extent carries floating-point dust, and `ceil` turned an exact 19
        // into 20. The extra goes BELOW the baseline so the text does not move.
        let line_h = raw_h.round();
        line_desc += line_h - raw_h;

        // Measure content width: CSS requires stripping leading/trailing
        // collapsible whitespace from each line before alignment.
        // Find the first and last non-space, non-break items.
        let first_content = line_items
            .iter()
            .position(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
        let last_content = line_items
            .iter()
            .rposition(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
        let content_line_w: f32 = match (first_content, last_content) {
            (Some(f), Some(l)) => line_items[f..=l].iter().map(|it| it.advance).sum(),
            _ => 0.0,
        };

        // Resolve text-align Start/End based on direction
        let effective_align = match node.style.text_align {
            TextAlign::Start => {
                if is_rtl {
                    TextAlign::Right
                } else {
                    TextAlign::Left
                }
            }
            TextAlign::End => {
                if is_rtl {
                    TextAlign::Left
                } else {
                    TextAlign::Right
                }
            }
            a => a,
        };

        // Justify: compute extra space per word gap (use align_w, never f32::MAX)
        let extra_per_gap =
            if effective_align == TextAlign::Justify && !was_break && next_start < items.len() {
                let gaps = line_items.iter().filter(|it| it.is_space).count() as f32;
                if gaps > 0.0 {
                    ((align_w - content_line_w) / gaps).max(0.0)
                } else {
                    0.0
                }
            } else {
                0.0
            };

        // X offset for text alignment (use align_w, never f32::MAX)
        let mut line_x = content_x
            + fc_left
            + match effective_align {
                TextAlign::Right => (align_w - content_line_w).max(0.0),
                TextAlign::Center => ((align_w - content_line_w) / 2.0).max(0.0),
                _ => 0.0,
            };
        if text_indent >= 0.0 && line_x < content_x {
            line_x = content_x;
        }

        // Account for ::before on first line, ::after on last line
        let is_last_line = next_start >= items.len();
        if is_first_line && before_w > 0.0 {
            line_x -= before_w;
            if text_indent >= 0.0 && line_x < content_x {
                line_x = content_x;
            }
        }
        let line_w_total = content_line_w
            + if is_first_line { before_w } else { 0.0 }
            + if is_last_line { after_w } else { 0.0 };

        // Compute text range for this line, stripping leading/trailing collapsible
        // whitespace per CSS §16.6.1. Use first_content/last_content from above.
        let content_items = match (first_content, last_content) {
            (Some(f), Some(l)) => &line_items[f..=l],
            _ => &line_items[0..0],
        };
        let text_s = content_items
            .iter()
            .filter_map(|it| {
                if let InlineItemKind::Text { text_start, .. } = &it.kind {
                    Some(*text_start)
                } else {
                    None
                }
            })
            .min()
            .unwrap_or_else(|| {
                // Fallback: use any item if all are spaces
                line_items
                    .iter()
                    .filter_map(|it| {
                        if let InlineItemKind::Text { text_start, .. } = &it.kind {
                            Some(*text_start)
                        } else {
                            None
                        }
                    })
                    .min()
                    .unwrap_or(0)
            });
        let text_e = content_items
            .iter()
            .filter_map(|it| {
                if let InlineItemKind::Text {
                    text_start,
                    text_len,
                    ..
                } = &it.kind
                {
                    Some(text_start + text_len)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or_else(|| {
                line_items
                    .iter()
                    .filter_map(|it| {
                        if let InlineItemKind::Text {
                            text_start,
                            text_len,
                            ..
                        } = &it.kind
                        {
                            Some(text_start + text_len)
                        } else {
                            None
                        }
                    })
                    .max()
                    .unwrap_or(text_s)
            });

        // Early-stop: if matching an old cached line with same breaks at same X and Y
        // (only when no floats involved; check x so different column positions don't reuse cache)
        if float_ctx.is_none() && old_line_idx > 0 && old_line_idx < old_lines.len() {
            let ol = &old_lines[old_line_idx];
            if ol.text_start == text_s
                && ol.text_length == text_e.saturating_sub(text_s)
                && ol.y == cursor_y
                && (ol.x - line_x).abs() < 0.5
            {
                // Rest of lines unchanged — copy them
                for j in old_line_idx..old_lines.len() {
                    line_cache.push(old_lines[j].clone());
                }
                node.layout.line_cache = line_cache;
                node.layout.inline_runs = runs;
                // Update box rects with cached height
                let bottom = node
                    .layout
                    .line_cache
                    .last()
                    .map(|l| l.y + l.height)
                    .unwrap_or(cursor_y);
                let raw_h = (bottom - content_y).max(0.0);
                let content_h = match rbox.content_height {
                    Some(h) => h,
                    None => raw_h,
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
                let content_h = content_h.max(min_h).min(max_h);
                set_box_rects(
                    node,
                    content_x,
                    content_y,
                    content_w,
                    content_h,
                    rbox,
                    margin_left,
                    margin_right,
                );
                return node.layout.margin_rect.h;
            }
        }

        // Collect atomic positions and inline fragment rects on this line
        {
            let mut cur_x = line_x;
            let mut prefix_w = 0.0f32;
            let mut seen_atomic_before_text = false;
            for item in line_items {
                let atomic_x = if is_rtl {
                    line_x + (line_w_total - prefix_w - item.advance).max(0.0)
                } else {
                    cur_x
                };
                match &item.kind {
                    InlineItemKind::Atomic { path, .. } => {
                        let child_node = resolve_path(node, path);
                        let box_h = child_node
                            .map(|n| n.layout.margin_rect.h)
                            .unwrap_or(item.height);
                        let box_w = child_node
                            .map(|n| n.layout.margin_rect.w)
                            .unwrap_or(item.advance);
                        let valign = child_node
                            .map(|n| n.style.vertical_align.clone())
                            .unwrap_or(crate::types::VerticalAlign::Baseline);
                        let ay = match valign {
                            crate::types::VerticalAlign::Top => cursor_y,
                            crate::types::VerticalAlign::Bottom => cursor_y + line_h - box_h,
                            crate::types::VerticalAlign::Middle => {
                                cursor_y + (line_h - box_h) / 2.0
                            }
                            _ => {
                                // Baseline alignment uses the item's synthesized
                                // ascent. For inline-block that is the bottom edge;
                                // for inline-flex/grid it can be the container's
                                // own baseline contribution.
                                let ay = cursor_y + line_asc - item.ascent;
                                ay.max(cursor_y)
                            }
                        };
                        atomic_pos.push((path.clone(), atomic_x, ay));
                        let ar = Rect::new(atomic_x, ay, box_w, box_h);
                        for len in 1..path.len() {
                            let p = path[..len].to_vec();
                            inline_fragment_rects
                                .entry(p)
                                .and_modify(|existing| union_rect(existing, &ar))
                                .or_insert(ar);
                        }
                        seen_atomic_before_text = true;
                    }
                    InlineItemKind::Text { path, .. } => {
                        let text_x = if is_rtl && seen_atomic_before_text {
                            atomic_x
                        } else {
                            cur_x
                        };
                        let r = Rect::new(text_x, cursor_y, item.advance, line_h);
                        for len in 1..=path.len() {
                            let p = path[..len].to_vec();
                            inline_fragment_rects
                                .entry(p)
                                .and_modify(|existing| union_rect(existing, &r))
                                .or_insert(r);
                        }
                    }
                    _ => {}
                }
                cur_x += item.advance;
                prefix_w += item.advance;
            }
        }

        // Build LayoutLine
        // Compute text_x_offset: sum of advances before the first retained text
        // byte on the line. Leading collapsible whitespace can be a Text item
        // before an atomic inline child; stopping at that whitespace paints the
        // visible text under the atomic child.
        let flat_text = collect_flat_text(node);
        let mut text_x_off = 0.0f32;
        let mut prefix_w = 0.0f32;
        let mut seen_atomic_before_text = false;
        for item in line_items.iter() {
            match &item.kind {
                InlineItemKind::Text {
                    text_start,
                    text_len,
                    ..
                } if *text_start + *text_len > text_s => {
                    let seg_start = floor_cb(&flat_text, (*text_start).max(text_s));
                    let seg_end =
                        floor_cb(&flat_text, (*text_start + *text_len).min(flat_text.len()));
                    if seg_start < seg_end && flat_text[seg_start..seg_end].trim().is_empty() {
                        if !is_rtl {
                            text_x_off += item.advance;
                        }
                        prefix_w += item.advance;
                        continue;
                    }
                    if is_rtl && seen_atomic_before_text {
                        text_x_off = (line_w_total - prefix_w - item.advance).max(0.0);
                    }
                    break;
                }
                InlineItemKind::Atomic { .. } => {
                    if !is_rtl {
                        text_x_off += item.advance;
                    }
                    prefix_w += item.advance;
                    seen_atomic_before_text = true;
                }
                _ => {
                    if !is_rtl {
                        text_x_off += item.advance;
                    }
                    prefix_w += item.advance;
                }
            }
        }

        let mut ll = LayoutLine {
            text_start: text_s,
            text_length: text_e.saturating_sub(text_s),
            x: line_x,
            y: cursor_y,
            width: line_w_total,
            height: line_h,
            ascent: line_asc,
            descent: line_desc,
            extra_space_per_word: extra_per_gap,
            text_x_offset: text_x_off,
            visual_segments: Vec::new(),
            char_x: Vec::new(),
            has_clamped_continuation: false,
            char_x_key: 0,
        };

        // Resolve BiDi visual segments for this line
        let para_dir = node.style.direction;
        resolve_bidi_line(&flat_text, &mut ll, para_dir, node.style.unicode_bidi);

        // Fill per-character x positions using real glyph metrics, shaped at
        // physical pixel size so positions match the renderer exactly.
        // Skip for lines very far below the viewport (they won't be rendered this frame).
        // Use a generous threshold — BiDi text and scrollable content need char_x
        // even when initially off-screen.
        let line_visible = engine.viewport_h <= 0.0 || ll.y < engine.viewport_h * 10.0;
        if line_visible {
            // ⛔ Re-shaping this line is the most expensive thing a layout does
            // (72,805 profile samples on Wikipedia). Nothing about the glyph
            // positions changes unless the line's TEXT, the STYLES of the runs
            // covering it, its justification spacing, its BiDi segmentation or
            // the device scale change — so fingerprint exactly those and reuse
            // the previous line's `char_x` when they match.
            //
            // The existing early-stop cannot do this job: it needs
            // `old_line_idx > 0` and copies the whole TAIL, so the first line is
            // always re-shaped and a block of one or two lines never benefits.
            let key = char_x_fingerprint(&flat_text, &runs, &ll, engine.scale);
            let reused = old_lines.get(old_line_idx).and_then(|ol| {
                (ol.char_x_key == key && key != 0 && !ol.char_x.is_empty())
                    .then(|| ol.char_x.clone())
            });
            match reused {
                Some(prev) => {
                    ll.char_x = prev;
                    if let Some(ol) = old_lines.get(old_line_idx) {
                        ll.visual_segments = ol.visual_segments.clone();
                    }
                    ll.char_x_key = key;
                }
                None => {
                    if let Some(fs_ptr) = engine.font_system {
                        let fs = unsafe { &mut *fs_ptr };
                        fill_char_x_for_line(fs, &flat_text, &runs, &mut ll, engine.scale);
                        ll.char_x_key = key;
                    }
                }
            }
        }

        line_cache.push(ll);
        cursor_y += line_h;
        item_idx = next_start;
        old_line_idx += 1;
    }

    // Empty block with no content: add one empty line so the caret has a home.
    if line_cache.is_empty() && items.is_empty() {
        line_cache.push(LayoutLine {
            text_start: text_offset,
            text_length: 0,
            x: content_x,
            y: cursor_y,
            width: 0.0,
            height: font_px * 1.2,
            ascent: font_px * 1.2,
            descent: 0.0,
            extra_space_per_word: 0.0,
            text_x_offset: 0.0,
            visual_segments: Vec::new(),
            char_x: Vec::new(),
            has_clamped_continuation: false,
            char_x_key: 0,
        });
        cursor_y += font_px * 1.2;
    }

    // Trailing empty line after <br> (for caret positioning after Enter)
    if ends_with_break {
        line_cache.push(LayoutLine {
            text_start: text_offset,
            text_length: 0,
            x: content_x,
            y: cursor_y,
            width: 0.0,
            height: font_px * 1.2,
            ascent: font_px * 1.2,
            descent: 0.0,
            extra_space_per_word: 0.0,
            text_x_offset: 0.0,
            visual_segments: Vec::new(),
            char_x: Vec::new(),
            has_clamped_continuation: false,
            char_x_key: 0,
        });
        cursor_y += font_px * 1.2;
    }

    if let Some(limit) = node.style.line_clamp {
        let limit = limit as usize;
        if limit > 0 && line_cache.len() > limit {
            line_cache.truncate(limit);
            if let Some(last) = line_cache.last_mut() {
                last.has_clamped_continuation = true;
            }
            cursor_y = line_cache
                .last()
                .map(|line| line.y + line.height)
                .unwrap_or(content_y);
        }
    }

    // ── 5. Compute content height ──────────────────────────────────────────────
    let inline_h = (cursor_y - content_y).max(0.0);
    // Include float bottom so the container encloses its floats.
    // A BFC owner (!has_parent_fc) always contains all its floats.
    // A non-BFC element that placed its OWN floats also needs to expand
    // (CSS §9.5: containers with floated children don't collapse).
    let has_own_floats = float_ctx
        .as_ref()
        .map_or(false, |fc| fc.floats.len() > floats_before);
    let float_bottom = if !has_parent_fc || has_own_floats {
        if let Some(ref fc) = float_ctx {
            let offset = content_y - fc.origin_y;
            fc.floats
                .iter()
                .map(|f| (f.clear - offset).max(0.0))
                .fold(0.0f32, f32::max)
        } else {
            0.0
        }
    } else {
        0.0
    };
    let raw_h = inline_h.max(float_bottom);
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
    let content_h = match rbox.content_height {
        Some(h) => h,
        None => raw_h,
    };
    let content_h = content_h.max(min_h).min(max_h);

    // Apply aspect-ratio: if height is auto and aspect_ratio is set, derive height from width
    let content_h = if rbox.content_height.is_none() {
        if let Some(ratio) = node.style.aspect_ratio {
            if ratio > 0.0 {
                (content_w / ratio).max(0.0).max(min_h).min(max_h)
            } else {
                content_h
            }
        } else {
            content_h
        }
    } else {
        content_h
    };

    set_box_rects(
        node,
        content_x,
        content_y,
        content_w,
        content_h,
        rbox,
        margin_left,
        margin_right,
    );
    if let Some(last_line) = line_cache.last() {
        node.layout.baseline = last_line.y + last_line.ascent;
    }
    node.layout.line_cache = line_cache;
    node.layout.inline_runs = runs;

    // ── 5b. Scroll extent for overflow:scroll/auto inline containers ───────────
    if matches!(
        node.style.overflow_x,
        crate::types::Overflow::Scroll | crate::types::Overflow::Auto
    ) || matches!(
        node.style.overflow_y,
        crate::types::Overflow::Scroll | crate::types::Overflow::Auto
    ) {
        // Natural content height from inline lines
        let natural_h = raw_h.max(content_h);
        // Natural content width: max width across all lines
        let natural_w = node
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .fold(content_w, f32::max);
        node.layout.scroll_height = natural_h;
        node.layout.scroll_width = natural_w;
        let max_v = (node.layout.scroll_height - content_h).max(0.0);
        let max_h = (node.layout.scroll_width - content_w).max(0.0);
        node.layout.scroll_top = node.layout.scroll_top.min(max_v).max(0.0);
        node.layout.scroll_left = node.layout.scroll_left.min(max_h).max(0.0);
    } else {
        node.layout.scroll_height = content_h;
        node.layout.scroll_width = content_w;
        node.layout.scroll_top = 0.0;
        node.layout.scroll_left = 0.0;
    }

    // ── 6. Position atomic inline-block children ──────────────────────────────
    //    Separate pass after all lines are built (mirrors C++ post-loop pass)
    for (path, ax, ay) in atomic_pos {
        if let Some(target) = resolve_path_mut(node, &path) {
            // Shift child rects to final position
            let dx = ax - target.layout.margin_rect.x;
            let dy = ay - target.layout.margin_rect.y;
            crate::layout::shift_rects(target, dx, dy);
        }
    }

    // ── 6b. Position non-atomic inline element fragments ─────────────────────
    for (path, rect) in inline_fragment_rects {
        if let Some(target) = resolve_path_mut(node, &path) {
            if target.style.is_inline_level()
                && !matches!(
                    target.style.display,
                    Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
                )
            {
                let child_font_px = target.style.font_size_px(font_px, root_font_px);
                let rb = engine.res_box(&target.style, child_font_px, 0.0, root_font_px);
                target.layout.content_rect = rect;
                target.layout.padding_rect = Rect::new(
                    rect.x - rb.padding_left,
                    rect.y - rb.padding_top,
                    rect.w + rb.padding_left + rb.padding_right,
                    rect.h + rb.padding_top + rb.padding_bottom,
                );
                target.layout.border_rect = Rect::new(
                    target.layout.padding_rect.x - rb.border_left,
                    target.layout.padding_rect.y - rb.border_top,
                    target.layout.padding_rect.w + rb.border_left + rb.border_right,
                    target.layout.padding_rect.h + rb.border_top + rb.border_bottom,
                );
                target.layout.margin_rect = Rect::new(
                    target.layout.border_rect.x - rb.margin_left,
                    target.layout.border_rect.y - rb.margin_top,
                    target.layout.border_rect.w + rb.margin_left + rb.margin_right,
                    target.layout.border_rect.h + rb.margin_top + rb.margin_bottom,
                );
                target.layout.scroll_width = target.layout.content_rect.w;
                target.layout.scroll_height = target.layout.content_rect.h;
            }
        }
    }

    // ── 7. Absolutely/fixed positioned children ────────────────────────────────
    //    Inline containers can still be containing blocks for absolutely-positioned
    //    children (e.g. a `position:relative` div whose only visible in-flow content
    //    is text while its absolutely-placed children are out-of-flow).
    let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style) {
        node.layout.padding_rect
    } else {
        engine.pos_cb.get()
    };
    let eff2 = crate::layout::grid::collect_grid_children(node);
    let abs_paths2: Vec<Vec<usize>> = eff2
        .into_iter()
        .filter(|path| {
            let c = crate::layout::grid::grid_child_ref(node, path);
            matches!(c.style.position, Position::Absolute | Position::Fixed)
        })
        .collect();
    for path in &abs_paths2 {
        let fallback_static_x = node.layout.line_cache.last().map(|line| {
            if node.style.direction == Direction::RTL {
                line.x
            } else {
                line.x + line.width
            }
        });
        let child = crate::layout::grid::grid_child_mut(node, path);
        // Record static position: where this element would sit in normal flow
        // at the inline cursor after laid-out in-flow content.
        if child.layout.abs_static_x.is_none() {
            let static_x = fallback_static_x.unwrap_or(content_x);
            child.layout.abs_static_x = Some(static_x);
        }
        if child.layout.abs_static_y.is_none() {
            child.layout.abs_static_y = Some(content_y);
        }
        let had_static_x = child.layout.abs_static_x.is_some();
        let had_static_y = child.layout.abs_static_y.is_some();
        layout_positioned(engine, child, containing_rect, font_px, root_font_px);
        let child = crate::layout::grid::grid_child_mut(node, path);
        let all_auto = child.style.left.is_auto()
            && child.style.right.is_auto()
            && child.style.top.is_auto()
            && child.style.bottom.is_auto();
        if all_auto
            && matches!(child.style.position, Position::Absolute)
            && !had_static_x
            && !had_static_y
        {
            let dx = containing_rect.x - child.layout.border_rect.x;
            let dy = containing_rect.y - child.layout.border_rect.y;
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                crate::layout::shift_rects(child, dx, dy);
            }
        }
    }

    node.layout.layout_dirty = false;
    node.layout.last_containing_width = content_w;
    node.layout.margin_rect.h
}

// ─── Path resolution helpers ─────────────────────────────────────────────────

/// Follow a chain of child indices to find the target node (immutable).
fn resolve_path<'a>(root: &'a WebCore, path: &[usize]) -> Option<&'a WebCore> {
    let mut cur = root;
    for &idx in path {
        if idx >= cur.children.len() {
            return None;
        }
        cur = &cur.children[idx];
    }
    Some(cur)
}

/// Follow a chain of child indices to find the target node (mutable).
fn resolve_path_mut<'a>(root: &'a mut WebCore, path: &[usize]) -> Option<&'a mut WebCore> {
    let mut cur = root;
    for &idx in path {
        if idx >= cur.children.len() {
            return None;
        }
        cur = &mut cur.children[idx];
    }
    Some(cur)
}

// ─── Box rect helper ──────────────────────────────────────────────────────────

fn set_box_rects(
    node: &mut WebCore,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
    rbox: &ResolvedBox,
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
    let mr_w =
        (node.layout.border_rect.w + margin_left + margin_right).max(node.layout.border_rect.w);
    node.layout.margin_rect = Rect::new(
        node.layout.border_rect.x - margin_left,
        node.layout.border_rect.y - rbox.margin_top,
        mr_w,
        node.layout.border_rect.h + rbox.margin_top + rbox.margin_bottom,
    );
    node.layout.baseline = content_y + content_h;
    // Cache resolved values (same as build_box_rects in block.rs)
    node.layout.resolved_margin_top = rbox.margin_top;
    node.layout.resolved_margin_right = margin_right;
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
    // Expose own margins for parent's margin-collapsing logic.
    // For "empty" blocks (no content, no border, no padding, no explicit height),
    // top and bottom margins collapse into each other per CSS 2.1 §8.3.1.
    let is_empty = content_h == 0.0
        && rbox.border_top == 0.0
        && rbox.border_bottom == 0.0
        && rbox.padding_top == 0.0
        && rbox.padding_bottom == 0.0
        && rbox.content_height.is_none();
    if is_empty && node.style.min_height.is_auto() {
        node.layout.collapsed_margin_top = collapse_two(rbox.margin_top, rbox.margin_bottom);
        node.layout.collapsed_margin_bottom = 0.0;
    } else {
        node.layout.collapsed_margin_top = rbox.margin_top;
        node.layout.collapsed_margin_bottom = rbox.margin_bottom;
    }
}

// ─── Line metrics ─────────────────────────────────────────────────────────────

/// Line metrics: the tallest ascent and deepest descent among the line's items
/// AND the strut, which is always present (CSS 2.1 §10.8). An empty line is
/// exactly the strut.
fn measure_metrics(items: &[InlineItem], strut_asc: f32, strut_desc: f32) -> (f32, f32, f32) {
    let mut max_asc = strut_asc;
    let mut max_desc = strut_desc;
    let mut max_atomic_h = 0.0f32;
    let mut max_atomic_asc = 0.0f32;
    let mut max_atomic_desc = 0.0f32;
    let mut has_atomic = false;
    let mut has_text = false;
    for it in items {
        if matches!(it.kind, InlineItemKind::Break) {
            continue;
        }
        if matches!(it.kind, InlineItemKind::Atomic { .. }) {
            has_atomic = true;
            max_atomic_h = max_atomic_h.max(it.height);
            max_atomic_asc = max_atomic_asc.max(it.ascent);
            max_atomic_desc = max_atomic_desc.max(it.descent);
        } else if !it.is_space {
            has_text = true;
        }
        if it.ascent > max_asc {
            max_asc = it.ascent;
        }
        if it.descent > max_desc {
            max_desc = it.descent;
        }
    }
    if has_atomic && !has_text && max_atomic_desc < 0.5 {
        let line_h = baseline_bottom_atomic_line_height(max_atomic_h, strut_asc, strut_desc);
        let line_asc = max_atomic_asc.max(strut_asc).min(line_h);
        return (line_h, line_asc, (line_h - line_asc).max(0.0));
    }
    (max_asc + max_desc, max_asc, max_desc)
}

fn union_rect(a: &mut Rect, b: &Rect) {
    if b.w <= 0.0 && b.h <= 0.0 {
        return;
    }
    if a.w <= 0.0 && a.h <= 0.0 {
        *a = *b;
        return;
    }
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    a.x = x0;
    a.y = y0;
    a.w = (x1 - x0).max(0.0);
    a.h = (y1 - y0).max(0.0);
}

fn shifted_inline_metrics(
    vertical_align: &VerticalAlign,
    font_px: f32,
    ascent: f32,
    descent: f32,
) -> (f32, f32) {
    let shift = vertical_align_shift(vertical_align, font_px, ascent + descent);
    match vertical_align {
        VerticalAlign::Super => (ascent + shift, (descent - shift).max(0.0)),
        VerticalAlign::Sub => ((ascent - shift).max(0.0), descent + shift),
        VerticalAlign::TextTop => (ascent + shift, (descent - shift).max(0.0)),
        VerticalAlign::TextBottom => ((ascent + shift).max(0.0), descent - shift),
        VerticalAlign::Length(_) if shift >= 0.0 => (ascent + shift, (descent - shift).max(0.0)),
        VerticalAlign::Length(_) => ((ascent + shift).max(0.0), descent - shift),
        _ => (ascent, descent),
    }
}

pub(crate) fn vertical_align_shift(
    vertical_align: &VerticalAlign,
    font_px: f32,
    line_height_px: f32,
) -> f32 {
    match vertical_align {
        VerticalAlign::Super => font_px * 0.35,
        VerticalAlign::Sub => font_px * 0.20,
        VerticalAlign::TextTop => font_px * 0.25,
        VerticalAlign::TextBottom => -(font_px * 0.25),
        VerticalAlign::Length(length) => length.resolve(font_px, line_height_px, font_px),
        _ => 0.0,
    }
}

fn effective_inline_vertical_align(
    own: VerticalAlign,
    parent: Option<&crate::types::ComputedStyle>,
) -> VerticalAlign {
    if own != VerticalAlign::Baseline {
        own
    } else {
        parent
            .map(|style| style.vertical_align.clone())
            .filter(|vertical_align| *vertical_align != VerticalAlign::Baseline)
            .unwrap_or(own)
    }
}

/// Split an inline box's leading evenly above and below its font
/// (CSS 2.1 §10.8.1). A `line-height` under the font's own height gives
/// negative leading, which is how a tight `line-height` shrinks a line.
fn half_leading(font_asc: f32, font_desc: f32, line_h: f32) -> (f32, f32) {
    let lead = line_h - (font_asc + font_desc);
    let half = lead / 2.0;
    (font_asc + half, font_desc + (lead - half))
}

/// The height of a line box holding a single atomic inline `box_h` tall, strut
/// included. `block.rs` builds such lines directly when a block mixes inline
/// children with block-level ones, and it needs the same answer this module
/// computes for a full inline formatting context.
pub fn strut_line_height(
    engine: &LayoutEngine,
    node: &WebCore,
    font_px: f32,
    root_font_px: f32,
    box_h: f32,
) -> f32 {
    let (strut_asc, strut_desc) = strut_metrics(engine, node, font_px, root_font_px);
    baseline_bottom_atomic_line_height(box_h, strut_asc, strut_desc)
}

fn baseline_bottom_atomic_line_height(box_h: f32, strut_asc: f32, strut_desc: f32) -> f32 {
    let strut_h = strut_asc + strut_desc;
    if strut_desc > box_h * 0.25 {
        box_h.max(strut_h).round()
    } else {
        (box_h + strut_desc).max(strut_h).round()
    }
}

fn resolve_line_height(
    engine: &LayoutEngine,
    line_height: &CssLength,
    font_px: f32,
    root_font_px: f32,
) -> Option<f32> {
    if line_height.is_auto() {
        None
    } else {
        Some(engine.res_len(line_height, font_px, font_px, root_font_px))
    }
}

/// The strut for a block: its own font and `line-height`.
fn strut_metrics(
    engine: &LayoutEngine,
    node: &WebCore,
    font_px: f32,
    root_font_px: f32,
) -> (f32, f32) {
    let fs = unsafe { engine.font_system.map(|fs| &mut *fs) };
    let (fa, fd, natural_lh) = font_metrics(fs, &node.style.font_family, font_px);
    let line_h = resolve_line_height(engine, &node.style.line_height, font_px, root_font_px)
        .unwrap_or(natural_lh);
    half_leading(fa, fd, line_h)
}

// ─── Inline Item ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum InlineItemKind {
    /// A word or space segment. text_start/text_len are offsets into the
    /// concatenated text collected by `collect_items`.
    Text {
        text_start: usize,
        text_len: usize,
        path: Vec<usize>,
    },
    /// An atomic inline-level child. `path` is a chain of child indices from
    /// the block container down to the actual node (e.g. [2, 0, 0] means
    /// node.children[2].children[0].children[0]).
    Atomic { path: Vec<usize>, display: Display },
    /// Forced line break (<br>).
    Break,
    /// A floated child. `path` is rooted at the inline formatting context.
    Float { path: Vec<usize> },
}

#[derive(Debug, Clone)]
pub struct InlineItem {
    pub kind: InlineItemKind,
    pub advance: f32,
    pub ascent: f32,
    pub descent: f32,
    pub height: f32,
    pub is_space: bool,
    pub breakable: bool,
}

// ─── Collect inline items ────────────────────────────────────────────────────

/// Walk a node and emit InlineItems into `items`, also building style runs.
/// `text_offset` tracks the current byte position in the global flat-text string.
/// `is_direct_child` must be `true` only when `node` is an immediate child of the
/// inline container being laid out.
pub fn collect_items(
    engine: &LayoutEngine,
    node: &WebCore,
    parent_font_px: f32,
    root_font_px: f32,
    items: &mut Vec<InlineItem>,
    runs: &mut Vec<InlineRun>,
    text_offset: &mut usize,
    box_idx: usize,
    is_direct_child: bool,
    ancestor_path: &[usize],
) {
    // Text decoration is not inherited via CSS cascade, but visually paints
    // across descendants. Track the nearest ancestor's decoration to apply
    // to text runs within decorated inline elements.
    collect_items_inner(
        engine,
        node,
        parent_font_px,
        root_font_px,
        items,
        runs,
        text_offset,
        box_idx,
        is_direct_child,
        ancestor_path,
        None,
        &mut false,
    );
}

fn collect_items_inner(
    engine: &LayoutEngine,
    node: &WebCore,
    parent_font_px: f32,
    root_font_px: f32,
    items: &mut Vec<InlineItem>,
    runs: &mut Vec<InlineRun>,
    text_offset: &mut usize,
    box_idx: usize,
    is_direct_child: bool,
    ancestor_path: &[usize],
    parent_decoration: Option<&crate::types::ComputedStyle>,
    previous_collapsible_space: &mut bool,
) {
    if matches!(node.style.display, Display::None) {
        return;
    }
    let generated_content = node.style.rare().content.as_str();
    let mut current_path = ancestor_path.to_vec();
    current_path.push(box_idx);

    // Absolutely/fixed positioned elements are out of flow — skip them here;
    // they are laid out separately by layout_positioned.
    // Record the static position so deeply nested abs elements can use it.
    if matches!(node.style.position, Position::Absolute | Position::Fixed) {
        // Note: we don't have cursor_y here, but the parent's content_y is available
        // through the node's parent position. We'll set abs_static_x/y in layout_inline_block
        // after items are collected.
        return;
    }

    // ── Float ─────────────────────────────────────────────────────────────
    if !matches!(node.style.float, crate::types::Float::None) {
        items.push(InlineItem {
            kind: InlineItemKind::Float { path: current_path },
            advance: 0.0,
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
            is_space: false,
            breakable: false,
        });
        return;
    }

    let font_px = node.style.font_size_px(parent_font_px, root_font_px);
    let font_system = unsafe { engine.font_system.map(|fs| &mut *fs) };
    let (font_asc, font_desc, natural_lh) =
        font_metrics(font_system, &node.style.font_family, font_px);
    // `line-height: normal` is the font's own natural line height.
    let line_h = resolve_line_height(engine, &node.style.line_height, font_px, root_font_px)
        .unwrap_or(natural_lh);
    // CSS 2.1 §10.8.1: an inline box's leading is split evenly above and below
    // its font, and it is THOSE half-leading-adjusted edges that size the line
    // box — not the bare font metrics. Using the raw metrics put the baseline
    // too high and let a large-font span inflate the line past its own
    // `line-height`.
    let (ascent, descent) = half_leading(font_asc, font_desc, line_h);

    // ── Text node ─────────────────────────────────────────────────────────
    if node.is_text_node() {
        let rendered_text = if generated_content.is_empty() {
            node.text.as_str()
        } else {
            generated_content
        };
        if !rendered_text.is_empty() {
            let start = *text_offset;
            let (letter_s, word_s) = resolved_spacings(engine, &node.style, font_px, root_font_px);
            let vertical_align = effective_inline_vertical_align(
                node.style.vertical_align.clone(),
                parent_decoration,
            );
            let (item_ascent, item_descent) =
                shifted_inline_metrics(&vertical_align, font_px, ascent, descent);
            tokenize_text(
                engine,
                rendered_text,
                node.style.white_space,
                start,
                font_px,
                item_ascent,
                item_descent,
                line_h,
                &current_path,
                items,
                node.style.font_weight,
                node.style.font_style,
                &node.style.font_family,
                letter_s,
                word_s,
                node.style.word_break,
                node.style.overflow_wrap,
                node.style.hyphens,
                previous_collapsible_space,
                node.style.text_transform,
            );
            // ⛔ `node.style.clone()` now clones the ARC. This wants the
            // VALUE — it is mutated below and stored in an `InlineRun`.
            let mut run_style = (*node.style).clone();
            // Inherit non-inherited visual properties from parent inline element
            // (span, a, etc.) that paint across descendants:
            // - text-decoration (not CSS-inherited but visually applies to children)
            // - href (from <a> elements, needed for hit-testing links)
            if let Some(ps) = parent_decoration {
                if ps.text_decoration.underline {
                    run_style.text_decoration.underline = true;
                }
                if ps.text_decoration.overline {
                    run_style.text_decoration.overline = true;
                }
                if ps.text_decoration.strikethrough {
                    run_style.text_decoration.strikethrough = true;
                }
                if run_style.text_decoration_color.is_none() {
                    run_style.text_decoration_color = ps.text_decoration_color;
                }
                if run_style.text_decoration_style == TextDecorationStyle::Solid {
                    run_style.text_decoration_style = ps.text_decoration_style;
                }
                if run_style.text_decoration_thickness.is_auto() {
                    run_style.text_decoration_thickness = ps.text_decoration_thickness.clone();
                }
                // Propagate href from parent <a> element
                if run_style.href.is_empty() && !ps.href.is_empty() {
                    run_style.href = ps.href.clone();
                }
                if run_style.vertical_align == VerticalAlign::Baseline
                    && ps.vertical_align != VerticalAlign::Baseline
                {
                    run_style.vertical_align = ps.vertical_align.clone();
                }
            }
            runs.push(InlineRun {
                text_offset: start,
                length: rendered_text.len(),
                style: run_style,
            });
            *text_offset += rendered_text.len();
        }
        return;
    }

    // ── Forced line break ─────────────────────────────────────────────────
    if node.tag == "br" {
        *previous_collapsible_space = false;
        items.push(InlineItem {
            kind: InlineItemKind::Break,
            advance: 0.0,
            ascent,
            descent,
            height: line_h,
            is_space: false,
            breakable: false,
        });
        return;
    }

    // ── Atomic inline-block ───────────────────────────────────────────────
    // Also treat inline elements that contain block-level children as atomic.
    // This handles the "block inside inline" case (e.g. <a><strong display:block>).
    let is_inline_with_block_children = is_direct_child
        && node.style.is_inline_level()
        && !node.is_text_node()
        && has_in_flow_block_children(node);
    if matches!(
        node.style.display,
        Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
    ) || is_inline_with_block_children
        || is_atomic_inline_replaced(node)
    {
        // Use the pre-laid-out border box plus the resolved margins. The
        // stored margin rect deliberately clamps negative margins for float
        // placement, but inline layout must let negative margins reduce the
        // advance width; Slashdot's nav uses `li { margin-left:-4px }`.
        let used_margin_w = node.layout.border_rect.w
            + node.layout.resolved_margin_left
            + node.layout.resolved_margin_right;
        let box_w = if used_margin_w > 0.0 {
            used_margin_w
        } else if node.layout.margin_rect.w > 0.0 {
            node.layout.margin_rect.w
        } else {
            50.0
        };
        let box_h = if node.layout.margin_rect.h > 0.0 {
            node.layout.margin_rect.h
        } else {
            font_px * 1.2
        };
        let full_path = current_path.clone();
        let (item_ascent, item_descent) = if matches!(
            node.style.display,
            Display::InlineFlex | Display::InlineGrid
        ) {
            let a = box_h.min(ascent);
            (a, (box_h - a).max(0.0))
        } else if matches!(node.style.display, Display::InlineBlock) {
            // CSS 2.1 §10.8.1: baseline of an 'inline-block' is the baseline of its
            // LAST line box in normal flow, unless it has no in-flow line boxes
            // or overflow is not 'visible' — then baseline is the bottom margin edge.
            let has_visible_overflow = matches!(node.style.overflow_x, Overflow::Visible)
                && matches!(node.style.overflow_y, Overflow::Visible);
            if has_visible_overflow && node.layout.baseline > 0.0 {
                let a = (node.layout.baseline - node.layout.margin_rect.y).max(0.0);
                (a, (box_h - a).max(0.0))
            } else {
                (box_h, 0.0)
            }
        } else {
            (box_h, 0.0)
        };
        items.push(InlineItem {
            kind: InlineItemKind::Atomic {
                path: full_path,
                display: node.style.display,
            },
            advance: box_w,
            ascent: item_ascent,
            descent: item_descent,
            height: box_h,
            is_space: false,
            breakable: true,
        });
        *previous_collapsible_space = false;
        return;
    }

    if !node.style.before_content.is_empty() {
        let pseudo_style = node
            .style
            .before_style
            .as_deref()
            .unwrap_or_else(|| node.style.as_ref());
        emit_generated_inline_content(
            engine,
            &node.style.before_content,
            pseudo_style,
            font_px,
            root_font_px,
            items,
            runs,
            text_offset,
            &current_path,
            previous_collapsible_space,
        );
    }

    // ── Own text ──────────────────────────────────────────────────────────
    let rendered_text = if generated_content.is_empty() {
        node.text.as_str()
    } else {
        generated_content
    };
    if !rendered_text.is_empty() {
        let start = *text_offset;
        let (letter_s, word_s) = resolved_spacings(engine, &node.style, font_px, root_font_px);
        let (item_ascent, item_descent) =
            shifted_inline_metrics(&node.style.vertical_align, font_px, ascent, descent);
        tokenize_text(
            engine,
            rendered_text,
            node.style.white_space,
            start,
            font_px,
            item_ascent,
            item_descent,
            line_h,
            &current_path,
            items,
            node.style.font_weight,
            node.style.font_style,
            &node.style.font_family,
            letter_s,
            word_s,
            node.style.word_break,
            node.style.overflow_wrap,
            node.style.hyphens,
            previous_collapsible_space,
            node.style.text_transform,
        );
        runs.push(InlineRun {
            text_offset: start,
            length: rendered_text.len(),
            style: (*node.style).clone(),
        });
        *text_offset += rendered_text.len();
    }

    // ── Inline box decoration: account for padding/border/margin ────────
    // CSS inline elements (not the block container itself) with
    // padding/border/margin add to the line width at the start and end.
    let has_inline_decoration = !is_direct_child && matches!(node.style.display, Display::Inline);
    let (inline_left, inline_right) = if has_inline_decoration {
        // ⛔ THROUGH `res_box`, which is the one place that knows a border with
        // no style occupies nothing (CSS Backgrounds §4.3). `border-width`
        // computes to `medium` — 3px — so reading the width on its own put 3px
        // on the first line and 3px on the last of every nested inline box, and
        // a flex item sized to its own text then broke onto a second line.
        let rb = engine.res_box(&node.style, font_px, 0.0, root_font_px);
        (
            rb.padding_left + rb.border_left + rb.margin_left,
            rb.padding_right + rb.border_right + rb.margin_right,
        )
    } else {
        (0.0, 0.0)
    };

    // Emit left decoration as a non-breakable zero-height advance
    if inline_left > 0.0 {
        items.push(InlineItem {
            kind: InlineItemKind::Text {
                text_start: *text_offset,
                text_len: 0,
                path: current_path.clone(),
            },
            advance: inline_left,
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
            is_space: false,
            breakable: false,
        });
    }

    // ── Recurse into children ─────────────────────────────────────────────
    let runs_before = runs.len();
    let child_path = current_path.clone();
    // Pass this element's style to children for inline visual effects that
    // apply through the inline box rather than normal CSS inheritance.
    let deco_source = if node.style.text_decoration.underline
        || node.style.text_decoration.overline
        || node.style.text_decoration.strikethrough
        || !node.style.href.is_empty()
        || node.style.vertical_align != VerticalAlign::Baseline
    {
        Some(node.style.as_ref())
    } else {
        parent_decoration
    };
    if generated_content.is_empty() {
        for (i, child) in node.children.iter().enumerate() {
            collect_items_inner(
                engine,
                child,
                font_px,
                root_font_px,
                items,
                runs,
                text_offset,
                i,
                false,
                &child_path,
                deco_source,
                previous_collapsible_space,
            );
        }
    }

    if !node.style.after_content.is_empty() {
        let pseudo_style = node
            .style
            .after_style
            .as_deref()
            .unwrap_or_else(|| node.style.as_ref());
        emit_generated_inline_content(
            engine,
            &node.style.after_content,
            pseudo_style,
            font_px,
            root_font_px,
            items,
            runs,
            text_offset,
            &current_path,
            previous_collapsible_space,
        );
    }

    // Emit right decoration
    if inline_right > 0.0 {
        items.push(InlineItem {
            kind: InlineItemKind::Text {
                text_start: *text_offset,
                text_len: 0,
                path: current_path.clone(),
            },
            advance: inline_right,
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
            is_space: false,
            breakable: false,
        });
    }
    // CSS background-color is not inherited, but an inline element's background
    // must visually paint behind its descendant text runs.  Propagate this
    // element's background-color down to any child run that has none of its own.
    if node.style.background_color.a > 0 && !node.is_text_node() {
        for run in &mut runs[runs_before..] {
            if run.style.background_color.a == 0 {
                run.style.background_color = node.style.background_color;
            }
        }
    }
}

fn emit_generated_inline_content(
    engine: &LayoutEngine,
    content: &str,
    style: &ComputedStyle,
    parent_font_px: f32,
    root_font_px: f32,
    items: &mut Vec<InlineItem>,
    runs: &mut Vec<InlineRun>,
    text_offset: &mut usize,
    path: &[usize],
    previous_collapsible_space: &mut bool,
) {
    if content.is_empty() {
        return;
    }
    let font_px = style.font_size_px(parent_font_px, root_font_px);
    let font_system = unsafe { engine.font_system.map(|fs| &mut *fs) };
    let (font_asc, font_desc, natural_lh) = font_metrics(font_system, &style.font_family, font_px);
    let line_h = resolve_line_height(engine, &style.line_height, font_px, root_font_px)
        .unwrap_or(natural_lh);
    let (ascent, descent) = half_leading(font_asc, font_desc, line_h);
    let rb = engine.res_box(style, font_px, 0.0, root_font_px);
    let left = rb.margin_left + rb.border_left + rb.padding_left;
    let right = rb.padding_right + rb.border_right + rb.margin_right;
    if left > 0.0 {
        items.push(InlineItem {
            kind: InlineItemKind::Text {
                text_start: *text_offset,
                text_len: 0,
                path: path.to_vec(),
            },
            advance: left,
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
            is_space: false,
            breakable: false,
        });
    }
    let start = *text_offset;
    let items_before_text = items.len();
    let (letter_s, word_s) = resolved_spacings(engine, style, font_px, root_font_px);
    let (item_ascent, item_descent) =
        shifted_inline_metrics(&style.vertical_align, font_px, ascent, descent);
    tokenize_text(
        engine,
        content,
        style.white_space,
        start,
        font_px,
        item_ascent,
        item_descent,
        line_h,
        path,
        items,
        style.font_weight,
        style.font_style,
        &style.font_family,
        letter_s,
        word_s,
        style.word_break,
        style.overflow_wrap,
        style.hyphens,
        previous_collapsible_space,
        style.text_transform,
    );
    let emitted_text_w = items[items_before_text..]
        .iter()
        .map(|item| item.advance)
        .sum::<f32>();
    if matches!(
        style.display,
        Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
    ) && !style.width.is_auto()
        && !matches!(style.width, CssLength::Percent(_))
    {
        let target_w = engine.res_len(&style.width, font_px, 0.0, root_font_px);
        let extra = (target_w - emitted_text_w).max(0.0);
        if extra > 0.0 {
            items.push(InlineItem {
                kind: InlineItemKind::Text {
                    text_start: *text_offset,
                    text_len: 0,
                    path: path.to_vec(),
                },
                advance: extra,
                ascent: 0.0,
                descent: 0.0,
                height: 0.0,
                is_space: false,
                breakable: false,
            });
        }
    }
    runs.push(InlineRun {
        text_offset: start,
        length: content.len(),
        style: style.clone(),
    });
    *text_offset += content.len();
    if right > 0.0 {
        items.push(InlineItem {
            kind: InlineItemKind::Text {
                text_start: *text_offset,
                text_len: 0,
                path: path.to_vec(),
            },
            advance: right,
            ascent: 0.0,
            descent: 0.0,
            height: 0.0,
            is_space: false,
            breakable: false,
        });
    }
}

/// Used values of `letter-spacing` and `word-spacing`, in px.
///
/// `normal` computes to `auto` here, which is zero spacing — resolving it as a
/// length would hand back whatever `auto` degrades to.
fn resolved_spacings(
    engine: &LayoutEngine,
    style: &ComputedStyle,
    font_px: f32,
    root_font_px: f32,
) -> (f32, f32) {
    let res = |len: &CssLength| -> f32 {
        if len.is_auto() {
            0.0
        } else {
            engine.res_len(len, font_px, 0.0, root_font_px)
        }
    };
    (res(&style.letter_spacing), res(&style.word_spacing))
}

/// Split `text` at whitespace boundaries and emit word/space InlineItems.
/// For `white-space: pre`, `pre-wrap`, and `pre-line`, newlines (`\n`) produce
/// a forced `Break` item rather than being treated as a collapsible space.
///
/// `letter_spacing` and `word_spacing` are the resolved used values in px.
/// **They are part of the ADVANCE of the items, not a paint-time flourish**
/// (css-text-3 §8.1/§8.2): the same items are summed for the shrink-to-fit
/// width, walked by `break_one_line` to pick wrap points, and measured again
/// by `fill_char_x_for_line`. Leaving the spacing out of the advance sizes the
/// box for text narrower than what is painted into it, which clips the last
/// character inside an `overflow: hidden` box and overlaps the next inline
/// outside one.
fn tokenize_text(
    engine: &LayoutEngine,
    text: &str,
    white_space: WhiteSpace,
    base_offset: usize,
    font_px: f32,
    ascent: f32,
    descent: f32,
    line_h: f32,
    path: &[usize],
    items: &mut Vec<InlineItem>,
    font_weight: FontWeight,
    font_style: FontStyle,
    font_family: &str,
    letter_spacing: f32,
    word_spacing: f32,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
    hyphens: Hyphens,
    previous_collapsible_space: &mut bool,
    text_transform: TextTransform,
) {
    if text.is_empty() {
        return;
    }

    let preserve_newlines = matches!(
        white_space,
        WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine
    );

    let measure_transformed = |engine: &LayoutEngine, s: &str| -> (f32, f32) {
        let transformed =
            crate::renderer::display_list_builder::apply_text_transform(s, text_transform);
        let w =
            engine.measure_text_cached(&transformed, font_px, font_weight, font_style, font_family);
        let tracking = letter_spacing * transformed.chars().count() as f32;
        (w, tracking)
    };

    let mut word_start = 0usize;
    let mut i = 0usize;

    while i <= text.len() {
        let at_end = i == text.len();
        let ch = if at_end {
            None
        } else {
            text[i..].chars().next()
        };
        let ch_len = ch.map_or(0, |c| c.len_utf8());
        let is_nl = matches!(ch, Some('\n')) && preserve_newlines;
        let is_space = ch.is_some_and(|c| !is_nl && c.is_ascii_whitespace());

        if (at_end || is_space || is_nl) && i > word_start {
            *previous_collapsible_space = false;
            // Emit word — use cached measurement to avoid redundant font shaping
            let word = &text[word_start..i];
            let break_inside_word = matches!(word_break, WordBreak::BreakAll);
            let has_subword_breaks = word.chars().any(|ch| {
                (ch == '\u{00ad}' && hyphens != Hyphens::None)
                    || ch == '-'
                    || ch == '\u{2013}'
                    || ch == '\u{2014}'
                    || (word_break != WordBreak::KeepAll
                        && ((ch >= '\u{3000}' && ch <= '\u{9fff}')
                            || (ch >= '\u{f900}' && ch <= '\u{faff}')))
            });
            if break_inside_word {
                for (rel, ch) in word.char_indices() {
                    let next = rel + ch.len_utf8();
                    let part = &word[rel..next];
                    let (w, _) = measure_transformed(engine, part);
                    items.push(InlineItem {
                        kind: InlineItemKind::Text {
                            text_start: base_offset + word_start + rel,
                            text_len: next - rel,
                            path: path.to_vec(),
                        },
                        advance: w + letter_spacing,
                        ascent,
                        descent,
                        height: line_h,
                        is_space: false,
                        breakable: word_start > 0 || rel > 0,
                    });
                }
            } else if has_subword_breaks {
                let mut segment_start = 0usize;
                let mut starts_after_break = word_start > 0;
                for (rel, ch) in word.char_indices() {
                    let next_rel = rel + ch.len_utf8();
                    if ch == '\u{00ad}' && hyphens != Hyphens::None {
                        if rel > segment_start {
                            let segment = &word[segment_start..rel];
                            let (w, tracking) = measure_transformed(engine, segment);
                            items.push(InlineItem {
                                kind: InlineItemKind::Text {
                                    text_start: base_offset + word_start + segment_start,
                                    text_len: rel - segment_start,
                                    path: path.to_vec(),
                                },
                                advance: w + tracking,
                                ascent,
                                descent,
                                height: line_h,
                                is_space: false,
                                breakable: starts_after_break,
                            });
                        }
                        segment_start = next_rel;
                        starts_after_break = true;
                    } else {
                        let is_cjk = word_break != WordBreak::KeepAll
                            && ((ch >= '\u{3000}' && ch <= '\u{9fff}')
                                || (ch >= '\u{f900}' && ch <= '\u{faff}'));
                        let is_break_after =
                            ch == '-' || ch == '\u{2013}' || ch == '\u{2014}' || is_cjk;
                        let is_break_before_cjk = is_cjk && rel > segment_start;
                        if is_break_before_cjk {
                            let segment = &word[segment_start..rel];
                            let (w, tracking) = measure_transformed(engine, segment);
                            items.push(InlineItem {
                                kind: InlineItemKind::Text {
                                    text_start: base_offset + word_start + segment_start,
                                    text_len: rel - segment_start,
                                    path: path.to_vec(),
                                },
                                advance: w + tracking,
                                ascent,
                                descent,
                                height: line_h,
                                is_space: false,
                                breakable: starts_after_break,
                            });
                            segment_start = rel;
                            starts_after_break = true;
                        }
                        if is_break_after {
                            let segment = &word[segment_start..next_rel];
                            let (w, tracking) = measure_transformed(engine, segment);
                            items.push(InlineItem {
                                kind: InlineItemKind::Text {
                                    text_start: base_offset + word_start + segment_start,
                                    text_len: next_rel - segment_start,
                                    path: path.to_vec(),
                                },
                                advance: w + tracking,
                                ascent,
                                descent,
                                height: line_h,
                                is_space: false,
                                breakable: starts_after_break,
                            });
                            segment_start = next_rel;
                            starts_after_break = true;
                        }
                    }
                }
                if segment_start < word.len() {
                    let segment = &word[segment_start..];
                    let (w, tracking) = measure_transformed(engine, segment);
                    items.push(InlineItem {
                        kind: InlineItemKind::Text {
                            text_start: base_offset + word_start + segment_start,
                            text_len: word.len() - segment_start,
                            path: path.to_vec(),
                        },
                        advance: w + tracking,
                        ascent,
                        descent,
                        height: line_h,
                        is_space: false,
                        breakable: starts_after_break,
                    });
                }
            } else {
                let (w, tracking) = measure_transformed(engine, word);
                items.push(InlineItem {
                    kind: InlineItemKind::Text {
                        text_start: base_offset + word_start,
                        text_len: i - word_start,
                        path: path.to_vec(),
                    },
                    advance: w + tracking,
                    ascent,
                    descent,
                    height: line_h,
                    is_space: false,
                    breakable: word_start > 0,
                });
            }
        }
        if at_end {
            break;
        }

        if is_nl {
            *previous_collapsible_space = false;
            // Newline in a pre-like context: forced line break.
            // The newline byte itself is represented as a 1-byte Text item with
            // zero advance so caret offsets stay in sync.
            items.push(InlineItem {
                kind: InlineItemKind::Text {
                    text_start: base_offset + i,
                    text_len: ch_len,
                    path: path.to_vec(),
                },
                advance: 0.0,
                ascent,
                descent,
                height: line_h,
                is_space: false,
                breakable: false,
            });
            items.push(InlineItem {
                kind: InlineItemKind::Break,
                advance: 0.0,
                ascent,
                descent,
                height: line_h,
                is_space: false,
                breakable: false,
            });
            i += ch_len;
            word_start = i;
            continue;
        }

        if is_space {
            // Emit one space item per space character so caret byte offsets stay in sync.
            // (Previously all consecutive spaces were collapsed to one rendered item,
            //  causing the caret to drift right while text stayed left.)
            let space_w =
                engine.measure_text_cached(" ", font_px, font_weight, font_style, font_family);
            // A space is a word separator (css-text-3 §8.1) and a character
            // (§8.2), so it carries both spacings.
            let space_w = space_w + word_spacing + letter_spacing;
            // In white-space:pre / pre-wrap, spaces are significant (not collapsible).
            // Mark them as non-space so break_one_line doesn't strip leading whitespace,
            // and non-breakable in pre mode (only \n breaks lines).
            let preserve_spaces = matches!(white_space, WhiteSpace::Pre | WhiteSpace::PreWrap);
            let collapsible_space = !preserve_spaces;
            let advance = if collapsible_space && *previous_collapsible_space {
                0.0
            } else {
                space_w
            };
            items.push(InlineItem {
                kind: InlineItemKind::Text {
                    text_start: base_offset + i,
                    text_len: ch_len,
                    path: path.to_vec(),
                },
                advance,
                ascent,
                descent,
                height: line_h,
                is_space: !preserve_spaces,
                breakable: !matches!(white_space, WhiteSpace::Pre),
            });
            *previous_collapsible_space = collapsible_space;
            i += ch_len; // consume exactly one space character
            word_start = i;
            continue;
        }

        i += ch_len;
    }
}

// ─── Per-line breaker ─────────────────────────────────────────────────────────

/// Break one line from `items[start_idx..]` fitting in `avail_w`.
///
/// Returns `(line_end, next_start, was_forced_break)`:
/// - `items[start_idx..line_end]` are on the current line.
/// - `items[next_start..]` remain for subsequent lines.
/// - `was_forced_break` is true if a `<br>` item terminated the line.
fn break_one_line(items: &[InlineItem], start_idx: usize, avail_w: f32) -> (usize, usize, bool) {
    const LINE_BREAK_EPSILON: f32 = 0.5;
    // Skip leading spaces
    let mut i = start_idx;
    while i < items.len() && items[i].is_space {
        i += 1;
    }
    let line_start = i;

    let mut cur_w = 0.0f32;
    let mut last_bp: Option<usize> = None; // items index of last break opportunity

    while i < items.len() {
        let item = &items[i];

        // Forced break: terminate line here, consume the break item
        if matches!(item.kind, InlineItemKind::Break) {
            return (i, i + 1, true);
        }

        // Text items mark a break opportunity before the item. Atomic
        // inline-level boxes mark one after the box, but only after this item
        // has actually fit; otherwise the overflowing item would be included
        // on the previous line.
        if item.breakable && !matches!(item.kind, InlineItemKind::Atomic { .. }) {
            last_bp = Some(i);
        }

        let new_w = cur_w + item.advance;
        if new_w > avail_w + LINE_BREAK_EPSILON && i > line_start {
            if let Some(bp) = last_bp {
                // Trim trailing spaces from line
                let mut line_end = bp;
                while line_end > line_start && items[line_end - 1].is_space {
                    line_end -= 1;
                }
                // Skip leading spaces at start of next line
                let mut next = bp;
                while next < items.len() && items[next].is_space {
                    next += 1;
                }
                return (line_end, next, false);
            } else {
                // No valid break point: this is one unbreakable run. Let it
                // overflow the line instead of splitting generated box-model
                // fragments away from their glyph/content.
                cur_w = new_w;
                i += 1;
                continue;
            }
        }

        cur_w += item.advance;
        if item.breakable && matches!(item.kind, InlineItemKind::Atomic { .. }) {
            last_bp = Some(i + 1);
        }
        i += 1;
    }

    // Consumed all items
    (i, i, false)
}

fn balanced_wrap_width(items: &[InlineItem], max_w: f32) -> f32 {
    if !max_w.is_finite() || max_w <= 0.0 || items.is_empty() {
        return max_w;
    }
    let greedy = line_stats_for_width(items, max_w);
    if greedy.count <= 1 {
        return max_w;
    }

    let min_w = items
        .iter()
        .filter(|item| !item.is_space && !matches!(item.kind, InlineItemKind::Break))
        .map(|item| item.advance)
        .fold(1.0_f32, f32::max)
        .min(max_w);
    let mut lo = min_w;
    let mut hi = max_w;
    for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        let stats = line_stats_for_width(items, mid);
        if stats.count > greedy.count {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi.clamp(min_w, max_w)
}

fn pretty_wrap_width(items: &[InlineItem], max_w: f32) -> f32 {
    if !max_w.is_finite() || max_w <= 0.0 || items.is_empty() {
        return max_w;
    }
    let greedy = line_stats_for_width(items, max_w);
    if greedy.count <= 1 || !greedy.last_line_is_single_word {
        return max_w;
    }
    let balanced = balanced_wrap_width(items, max_w);
    let balanced_stats = line_stats_for_width(items, balanced);
    if balanced_stats.count == greedy.count && !balanced_stats.last_line_is_single_word {
        balanced
    } else {
        max_w
    }
}

#[derive(Default)]
struct LineStats {
    count: usize,
    max_width: f32,
    last_line_is_single_word: bool,
}

fn line_stats_for_width(items: &[InlineItem], width: f32) -> LineStats {
    let mut stats = LineStats::default();
    let mut idx = 0usize;
    let mut guard = 0usize;
    while idx < items.len() {
        guard += 1;
        if guard > items.len() + 4 {
            break;
        }
        let (line_end, next, was_break) = break_one_line(items, idx, width);
        if line_end > idx {
            let line_items = &items[idx..line_end];
            let line_w = trimmed_line_width(line_items);
            stats.max_width = stats.max_width.max(line_w);
            stats.last_line_is_single_word = trimmed_line_word_count(line_items) == 1;
            stats.count += 1;
            idx = next;
        } else if was_break {
            stats.last_line_is_single_word = false;
            stats.count += 1;
            idx = next;
        } else {
            idx += 1;
        }
    }
    stats
}

fn trimmed_line_word_count(items: &[InlineItem]) -> usize {
    let first = items
        .iter()
        .position(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
    let last = items
        .iter()
        .rposition(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
    let Some((f, l)) = first.zip(last) else {
        return 0;
    };
    let mut words = 0usize;
    let mut in_word = false;
    for item in &items[f..=l] {
        if item.is_space || matches!(item.kind, InlineItemKind::Break) {
            in_word = false;
        } else if !in_word {
            words += 1;
            in_word = true;
        }
    }
    words
}

fn trimmed_line_width(items: &[InlineItem]) -> f32 {
    let first = items
        .iter()
        .position(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
    let last = items
        .iter()
        .rposition(|it| !it.is_space && !matches!(it.kind, InlineItemKind::Break));
    match (first, last) {
        (Some(f), Some(l)) => items[f..=l].iter().map(|it| it.advance).sum(),
        _ => 0.0,
    }
}

// ─── Legacy full-document line breaker (kept for compatibility) ───────────────

/// Greedy line-breaker. Returns a vec of LineBuild, each holding its items
/// plus computed metrics (height, ascent, descent) and text byte-range.
/// NOTE: Does not handle floats; use `layout_inline_block` with a `FloatContext`
/// for float-aware layout.
pub fn break_lines(items: &[InlineItem], max_w: f32) -> Vec<LineBuild> {
    let mut lines: Vec<LineBuild> = Vec::new();
    let mut idx = 0;
    while idx < items.len() {
        let (line_end, next, was_break) = break_one_line(items, idx, max_w);
        if line_end > idx {
            push_line_from_slice(&mut lines, &items[idx..line_end]);
        } else if !was_break && line_end == idx {
            // No progress and no break — guard against infinite loop
            idx += 1;
            continue;
        }
        if was_break {
            // Emit empty break line
            push_line_from_slice(&mut lines, &[]);
        }
        idx = next;
    }
    lines
}

// ─── Line buffer ──────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct LineBuild {
    pub items: Vec<InlineItem>,
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Byte offset of the first character on this line in the flat text string.
    pub text_start: usize,
    /// Total byte length of text on this line.
    pub text_len: usize,
}

fn push_line_from_slice(lines: &mut Vec<LineBuild>, slice: &[InlineItem]) {
    let mut line = LineBuild::default();
    let mut text_start = usize::MAX;
    let mut text_end = 0usize;

    for item in slice {
        line.items.push(item.clone());
        line.width += item.advance;
        if item.ascent > line.ascent {
            line.ascent = item.ascent;
        }
        if item.descent > line.descent {
            line.descent = item.descent;
        }
        if item.height > line.height {
            line.height = item.height;
        }

        if let InlineItemKind::Text {
            text_start: ts,
            text_len: tl,
            ..
        } = &item.kind
        {
            if *ts < text_start {
                text_start = *ts;
            }
            let end = ts + tl;
            if end > text_end {
                text_end = end;
            }
        }
    }

    if line.height == 0.0 {
        line.height = (line.ascent + line.descent).max(16.0);
    }

    line.text_start = if text_start == usize::MAX {
        0
    } else {
        text_start
    };
    line.text_len = if text_end > line.text_start {
        text_end - line.text_start
    } else {
        0
    };

    lines.push(line);
}

// ─── Font resolution helpers ───────────────────────────────────────────────────

/// Resolve the first family name from a CSS `font-family` value (comma-separated,
/// possibly with quoted names) into a cosmic-text `Family`.
///
/// The returned `Family::Name` borrows directly from `raw` (zero-copy for named fonts).
/// Generic keywords (`sans-serif`, `serif`, …) return the corresponding enum variant.
pub(crate) fn css_family_to_cosmic(raw: &str) -> Family<'_> {
    let first = extract_first_css_family(raw);
    match first {
        "serif" => Family::Serif,
        "sans-serif" => Family::SansSerif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        // ⛔ `system-ui` IS a generic, and `resolve_css_family` — the resolver
        // the MEASURING path uses — treats it as one. Passing it through as a
        // face name here meant a box was measured with the sans-serif generic
        // and painted with whatever cosmic-text made of the literal name.
        "system-ui" => Family::SansSerif,
        "" => Family::SansSerif,
        name => Family::Name(name),
    }
}

/// A resolved font family, owned so it can be cached.
#[derive(Clone, Debug)]
pub(crate) enum ResolvedFamily {
    Generic(&'static str),
    /// ⛔ `Rc<str>`, not `String`. This is looked up per shaped segment per
    /// line; a `String` clone on every cache HIT allocated more than the
    /// fallback scan it exists to avoid.
    Named(std::rc::Rc<str>),
}

impl ResolvedFamily {
    pub(crate) fn as_family(&self) -> Family<'_> {
        match self {
            ResolvedFamily::Generic("serif") => Family::Serif,
            ResolvedFamily::Generic("monospace") => Family::Monospace,
            ResolvedFamily::Generic("cursive") => Family::Cursive,
            ResolvedFamily::Generic("fantasy") => Family::Fantasy,
            ResolvedFamily::Generic(_) => Family::SansSerif,
            ResolvedFamily::Named(s) => Family::Name(&**s),
        }
    }
}

thread_local! {
    /// Lowercased names of every family the font system holds, plus the face
    /// count it was built from so adding a font rebuilds it.
    static AVAILABLE: std::cell::RefCell<(usize, std::collections::HashSet<String>)> =
        std::cell::RefCell::new((usize::MAX, std::collections::HashSet::new()));
    /// CSS `font-family` string → the family actually used.
    static FAMILY_CACHE: std::cell::RefCell<std::collections::HashMap<Box<str>, ResolvedFamily>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Resolve a CSS `font-family` stack to the first family that EXISTS.
///
/// ⛔ This used to take the FIRST name in the stack and hand it to cosmic-text
/// whatever it was. CSS says "first AVAILABLE family" (CSS Fonts §5.2), and the
/// difference is not cosmetic: Wikipedia asks for
/// `"Linux Libertine", Georgia, Times, serif`, and on a machine without Linux
/// Libertine cosmic-text answered by walking its whole face list calling
/// `face_contains_family` — **per shaped run**. That was the single largest
/// cost in a page load, ahead of layout itself.
///
/// Cached per stack string, and the availability set is rebuilt whenever the
/// font DB grows (a `@font-face` load).
pub(crate) fn resolve_css_family(fs: &cosmic_text::FontSystem, raw: &str) -> ResolvedFamily {
    // ⛔ A front cache keyed on the string's ADDRESS, not its contents.
    // This is called once per shaped segment per line; hashing a font stack
    // ("Linux Libertine", Georgia, Times, "Source Serif Pro", serif) on every
    // one of those cost 12.7 us a call — more than the fallback scan it
    // exists to prevent. `font_family` lives in an `Arc<ComputedStyle>` shared
    // across elements, so the same pointer comes back over and over.
    FRONT
        .with(|f| {
            let f = f.borrow();
            for (ptr, len, fam) in f.iter() {
                if *ptr == raw.as_ptr() && *len == raw.len() {
                    return Some(fam.clone());
                }
            }
            None
        })
        .map(Some)
        .unwrap_or(None)
        .map_or_else(|| resolve_css_family_slow(fs, raw), |f| f)
}

thread_local! {
    /// (address, length, family) — a tiny direct scan, cleared when it fills.
    static FRONT: std::cell::RefCell<Vec<(*const u8, usize, ResolvedFamily)>> =
        std::cell::RefCell::new(Vec::new());
}

pub(crate) fn clear_font_family_caches() {
    FAMILY_CACHE.with(|c| c.borrow_mut().clear());
    FRONT.with(|f| f.borrow_mut().clear());
    FONT_RATIOS.with(|c| c.borrow_mut().clear());
    AVAILABLE.with(|a| {
        let mut a = a.borrow_mut();
        a.0 = 0;
        a.1.clear();
    });
}

fn resolve_css_family_slow(fs: &cosmic_text::FontSystem, raw: &str) -> ResolvedFamily {
    let found = FAMILY_CACHE.with(|c| c.borrow().get(raw).cloned());
    if let Some(hit) = found {
        FRONT.with(|f| {
            let mut f = f.borrow_mut();
            if f.len() >= 16 {
                f.clear();
            }
            f.push((raw.as_ptr(), raw.len(), hit.clone()));
        });
        return hit;
    }
    let faces = fs.db().len();
    AVAILABLE.with(|a| {
        let mut a = a.borrow_mut();
        if a.0 != faces {
            a.1.clear();
            for face in fs.db().faces() {
                for (name, _) in &face.families {
                    a.1.insert(name.to_ascii_lowercase());
                }
            }
            a.0 = faces;
        }
    });

    let mut chosen: Option<ResolvedFamily> = None;
    for part in raw.split(',') {
        let name = part.trim().trim_matches('"').trim_matches('\'').trim();
        if name.is_empty() {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        match lower.as_str() {
            "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" | "system-ui" => {
                let g = match lower.as_str() {
                    "serif" => "serif",
                    "monospace" => "monospace",
                    "cursive" => "cursive",
                    "fantasy" => "fantasy",
                    _ => "sans-serif",
                };
                chosen = Some(ResolvedFamily::Generic(g));
                break;
            }
            _ => {
                let present = AVAILABLE.with(|a| a.borrow().1.contains(&lower));
                if present {
                    chosen = Some(ResolvedFamily::Named(std::rc::Rc::from(name)));
                    break;
                }
            }
        }
    }
    let chosen = chosen.unwrap_or(ResolvedFamily::Generic("sans-serif"));
    FAMILY_CACHE.with(|c| c.borrow_mut().insert(Box::from(raw), chosen.clone()));
    FRONT.with(|f| {
        let mut f = f.borrow_mut();
        if f.len() >= 16 {
            f.clear();
        }
        f.push((raw.as_ptr(), raw.len(), chosen.clone()));
    });
    chosen
}

/// Extract the first font-family name as a `&str` slice into `raw`.
/// Strips surrounding quotes for quoted names.
fn extract_first_css_family(raw: &str) -> &str {
    let raw = raw.trim();
    if raw.is_empty() {
        return "";
    }
    // Quoted name: `"Times New Roman"` or `'Foo'`
    if raw.starts_with('"') {
        return raw[1..].split('"').next().unwrap_or("").trim();
    }
    if raw.starts_with('\'') {
        return raw[1..].split('\'').next().unwrap_or("").trim();
    }
    // Unquoted: up to the first comma
    raw.split(',').next().unwrap_or(raw).trim()
}

/// Map a `font_stretch` percentage (100.0 = normal) to a cosmic-text `Stretch` variant.
pub(crate) fn stretch_from_percent(pct: f32) -> Stretch {
    // CSS spec breakpoints (midpoints between defined values):
    if pct <= 56.25 {
        Stretch::UltraCondensed
    } else if pct <= 68.75 {
        Stretch::ExtraCondensed
    } else if pct <= 81.25 {
        Stretch::Condensed
    } else if pct <= 93.75 {
        Stretch::SemiCondensed
    } else if pct <= 106.25 {
        Stretch::Normal
    } else if pct <= 118.75 {
        Stretch::SemiExpanded
    } else if pct <= 137.5 {
        Stretch::Expanded
    } else if pct <= 175.0 {
        Stretch::ExtraExpanded
    } else {
        Stretch::UltraExpanded
    }
}

/// Build a cosmic-text `Weight` from a `FontWeight` enum, optionally overridden
/// by a `font-variation-settings` `"wght"` axis.
pub(crate) fn weight_from_style(weight: FontWeight, var: &[(String, f32)]) -> Weight {
    // font-variation-settings 'wght' overrides the logical font-weight.
    for (tag, val) in var {
        if tag == "wght" {
            return Weight(*val as u16);
        }
    }
    Weight(weight.value())
}

// ─── Text measurement ─────────────────────────────────────────────────────────

pub fn measure_text_width(
    text: &str,
    font_px: f32,
    font_system: Option<&mut cosmic_text::FontSystem>,
) -> f32 {
    measure_text_width_scaled(text, font_px, font_system, 1.0)
}

pub fn measure_text_width_scaled(
    text: &str,
    font_px: f32,
    font_system: Option<&mut cosmic_text::FontSystem>,
    scale: f32,
) -> f32 {
    if let Some(fs) = font_system {
        measure_text_width_fs(fs, text, font_px, scale)
    } else {
        measure_text_width_ts(text, font_px, 8)
    }
}

pub fn measure_text_width_weighted(
    text: &str,
    font_px: f32,
    font_system: Option<&mut cosmic_text::FontSystem>,
    weight: FontWeight,
    style: FontStyle,
    scale: f32,
    font_family: &str,
) -> f32 {
    if let Some(fs) = font_system {
        let ct_weight = Weight(weight.value());
        let ct_style = match style {
            FontStyle::Italic => CTextStyle::Italic,
            FontStyle::Oblique => CTextStyle::Oblique,
            FontStyle::Normal => CTextStyle::Normal,
        };
        measure_text_width_fs_attrs(fs, text, font_px, ct_weight, ct_style, scale, font_family)
    } else {
        let w = measure_text_width_ts(text, font_px, 8);
        if weight.is_bold() {
            w * 1.15
        } else {
            w
        }
    }
}

pub fn measure_text_width_fs(
    fs: &mut cosmic_text::FontSystem,
    text: &str,
    font_px: f32,
    scale: f32,
) -> f32 {
    measure_text_width_fs_attrs(
        fs,
        text,
        font_px,
        Weight::NORMAL,
        CTextStyle::Normal,
        scale,
        "",
    )
}

/// Measure text width by shaping at physical pixel size (font_px * scale) and
/// scaling the result back to logical pixels.  This matches the renderer which
/// also shapes at physical size, ensuring line-breaking decisions agree with
/// the actual rendered glyph widths.
pub fn measure_text_width_fs_attrs(
    fs: &mut cosmic_text::FontSystem,
    text: &str,
    font_px: f32,
    weight: Weight,
    style: CTextStyle,
    scale: f32,
    font_family: &str,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let size_adjust = font_size_adjust_scale(fs, font_family);
    let phys_px = font_px * size_adjust * scale.max(1.0);
    let inv = if scale > 1.0 { 1.0 / scale } else { 1.0 };
    let metrics = Metrics::new(phys_px, phys_px * 1.2);
    let mut buffer = Buffer::new(fs, metrics);
    let mut attrs = Attrs::new().weight(weight).style(style);
    // Set the correct font family so monospace/serif/etc. are measured accurately
    let resolved;
    if !font_family.is_empty() {
        resolved = resolve_css_family(fs, font_family);
        attrs = attrs.family(resolved.as_family());
    }
    buffer.set_text(fs, text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);

    let mut max_w = 0.0f32;
    for run in buffer.layout_runs() {
        if run.line_w > max_w {
            max_w = run.line_w;
        }
    }
    max_w * inv
}

/// Map a CSS font-family string to a cosmic_text Family.
fn cosmic_text_family(family: &str) -> cosmic_text::Family<'_> {
    let f = family.trim().trim_matches('"').trim_matches('\'');
    match f.to_ascii_lowercase().as_str() {
        "monospace" | "courier" | "courier new" => cosmic_text::Family::Monospace,
        "serif" | "times" | "times new roman" | "georgia" => cosmic_text::Family::Serif,
        "sans-serif" | "arial" | "helvetica" => cosmic_text::Family::SansSerif,
        "cursive" => cosmic_text::Family::Cursive,
        "fantasy" => cosmic_text::Family::Fantasy,
        _ => cosmic_text::Family::Name(f),
    }
}

pub fn measure_text_width_ts(text: &str, font_px: f32, tab_size: i32) -> f32 {
    let char_w = font_px * 0.55;
    let space_w = char_w * 0.35;
    let ts = (tab_size.max(1)) as f32;
    text.chars()
        .map(|c| {
            if c == '\t' {
                space_w * ts
            } else if "iIlj1!|:;,.'`".contains(c) {
                char_w * 0.45
            } else if "mwMW".contains(c) {
                char_w * 1.20
            } else if c == ' ' {
                space_w
            } else if c.is_ascii() {
                char_w
            } else {
                font_px * 1.0
            } // emoji / CJK: full square width
        })
        .sum()
}

/// The font's ascent, descent and natural line height at `font_px`.
///
/// ⛔ Reads the REAL font when a font system is available. The flat
/// `0.80 / 0.20` guess it used to return sums to one em, but a text font's
/// ascent and descent sum to well over that — Menlo is 0.928 / 0.236 — so the
/// baseline sat too high inside every line box and `line-height: normal` was
/// the wrong height. The ratios are cached per family: this is the layout hot
/// path, and shaping a probe per element would be a per-frame cost.
pub fn font_metrics(
    fs: Option<&mut cosmic_text::FontSystem>,
    family: &str,
    font_px: f32,
) -> (f32, f32, f32) {
    // Fallback ratios, used when no font system is reachable. They are a
    // typical text font's, not an em box.
    // (ascent, descent, leading) as fractions of the em.
    const FALLBACK: (f32, f32, f32) = (0.8, 0.2, 0.2);
    let (a, d, lead) = match fs {
        Some(fs) => font_metric_ratios(fs, family).unwrap_or(FALLBACK),
        None => FALLBACK,
    };
    // ⛔ Rounded to whole pixels, which is what a browser's font metrics are:
    // the leading is then computed from the ROUNDED extent, so a `line-height`
    // one pixel off the font's own height splits into a clean half-pixel above
    // and below. Leaving the raw fractions in put every such line a pixel
    // short of what a browser reports.
    let asc = (a * font_px).round();
    let desc = (d * font_px).round();
    (asc, desc, asc + desc + (lead * font_px).round())
}

thread_local! {
    static FONT_RATIOS: std::cell::RefCell<std::collections::HashMap<String, Option<(f32, f32, f32)>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static FONT_METRIC_OVERRIDES: std::cell::RefCell<std::collections::HashMap<String, FontMetricOverride>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FontMetricOverride {
    pub size_adjust: Option<f32>,
    pub ascent: Option<f32>,
    pub descent: Option<f32>,
    pub line_gap: Option<f32>,
}

impl FontMetricOverride {
    fn apply(self, asc: f32, desc: f32, leading: f32) -> (f32, f32, f32) {
        let size_adjust = self.size_adjust.unwrap_or(1.0).max(0.0);
        let mut asc = asc * size_adjust;
        let mut desc = desc * size_adjust;
        let mut leading = leading * size_adjust;
        if let Some(v) = self.ascent {
            asc = v.max(0.0);
        }
        if let Some(v) = self.descent {
            desc = v.max(0.0);
        }
        if let Some(v) = self.line_gap {
            leading = v.max(0.0);
        }
        (asc, desc, leading)
    }
}

pub(crate) fn set_font_metric_override(family: &str, metrics: FontMetricOverride) {
    let key = extract_first_css_family(family).trim().to_ascii_lowercase();
    if key.is_empty() {
        return;
    }
    FONT_METRIC_OVERRIDES.with(|m| {
        m.borrow_mut().insert(key.clone(), metrics);
    });
    FONT_RATIOS.with(|c| {
        c.borrow_mut().remove(&key);
    });
}

pub(crate) fn font_size_adjust_scale(fs: &cosmic_text::FontSystem, family: &str) -> f32 {
    let key = extract_first_css_family(family).trim().to_ascii_lowercase();
    if key.is_empty() || !font_family_available(fs, &key) {
        return 1.0;
    }
    FONT_METRIC_OVERRIDES
        .with(|m| m.borrow().get(&key).and_then(|metrics| metrics.size_adjust))
        .unwrap_or(1.0)
        .max(0.0)
}

/// Ascent, descent and leading as fractions of the em.
fn font_metric_ratios(fs: &mut cosmic_text::FontSystem, family: &str) -> Option<(f32, f32, f32)> {
    let key = family.trim().to_ascii_lowercase();
    if let Some(hit) = FONT_RATIOS.with(|c| c.borrow().get(&key).copied()) {
        return hit;
    }
    let computed = measure_font_ratios(fs, family).map(|(asc, desc, leading)| {
        let override_key = extract_first_css_family(family).trim().to_ascii_lowercase();
        FONT_METRIC_OVERRIDES
            .with(|m| m.borrow().get(&override_key).copied())
            .filter(|_| font_family_available(fs, &override_key))
            .map(|metrics| metrics.apply(asc, desc, leading))
            .unwrap_or((asc, desc, leading))
    });
    FONT_RATIOS.with(|c| {
        c.borrow_mut().insert(key, computed);
    });
    computed
}

fn font_family_available(fs: &cosmic_text::FontSystem, lower_family: &str) -> bool {
    if lower_family.is_empty() {
        return false;
    }
    fs.db().faces().any(|face| {
        face.families
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(lower_family))
    })
}

fn measure_font_ratios(fs: &mut cosmic_text::FontSystem, family: &str) -> Option<(f32, f32, f32)> {
    let resolved;
    let mut attrs = Attrs::new();
    if !family.is_empty() {
        resolved = resolve_css_family(fs, family);
        attrs = attrs.family(resolved.as_family());
    }
    // Shape one glyph purely to learn which face the family resolves to.
    let mut buffer = Buffer::new(fs, Metrics::new(16.0, 16.0));
    buffer.set_text(fs, "x", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    let font_id = buffer.layout_runs().next()?.glyphs.first()?.font_id;
    // The regular face's metrics stand in for every weight and style of the
    // family. Vertical metrics rarely differ across a family's faces, and
    // keying the cache on weight and style would multiply the probes on the
    // layout hot path for a sub-pixel difference.
    let font = fs.get_font(font_id, Weight::NORMAL)?;
    let m = font.metrics();
    let upem = m.units_per_em as f32;
    if upem <= 0.0 {
        return None;
    }
    // `descent` is negative in font units — it measures DOWN from the baseline.
    let asc = m.ascent / upem;
    let desc = -m.descent / upem;
    if asc <= 0.0 || desc < 0.0 {
        return None;
    }
    // The leading is carried as its own ratio rather than folded into a
    // natural line height and subtracted back out later — that subtraction
    // left floating-point dust on an exact value.
    Some((asc, desc, m.leading.max(0.0) / upem))
}

/// Everything `fill_char_x_for_line` shapes from, as one number.
///
/// ⛔ Must cover every input that moves a glyph. A field left out here is a
/// stale-position bug that renders as text drawn at the wrong offsets, which no
/// other test would catch. `0` means "do not reuse".
pub(crate) fn floor_cb(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn char_x_fingerprint(
    flat: &str,
    runs: &[InlineRun],
    line: &crate::types::LayoutLine,
    scale: f32,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let start = line.text_start;
    let end = (line.text_start + line.text_length).min(flat.len());
    if end <= start {
        return 0;
    }
    if !flat.is_char_boundary(start) || !flat.is_char_boundary(end) {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    flat[start..end].hash(&mut h);
    line.extra_space_per_word.to_bits().hash(&mut h);
    line.text_x_offset.to_bits().hash(&mut h);
    scale.to_bits().hash(&mut h);
    for vs in &line.visual_segments {
        vs.logical_start.hash(&mut h);
        vs.length.hash(&mut h);
        vs.level.hash(&mut h);
    }
    for r in runs {
        // Only the runs that touch this line can affect its glyphs.
        if r.text_offset + r.length <= start || r.text_offset >= end {
            continue;
        }
        r.text_offset.hash(&mut h);
        r.length.hash(&mut h);
        r.style.font_size_px(16.0, 16.0).to_bits().hash(&mut h);
        r.style
            .word_spacing
            .resolve(16.0, 0.0, 16.0)
            .to_bits()
            .hash(&mut h);
        // Tracking moves every glyph on the line, so a run that only changed
        // its `letter-spacing` must not be handed the previous `char_x`.
        r.style
            .letter_spacing
            .resolve(16.0, 0.0, 16.0)
            .to_bits()
            .hash(&mut h);
        r.style.font_weight.value().hash(&mut h);
        (r.style.font_style as u8).hash(&mut h);
        r.style.font_stretch.to_bits().hash(&mut h);
        r.style.font_family.hash(&mut h);
    }
    let v = h.finish();
    if v == 0 {
        1
    } else {
        v
    }
}

// ─── Accurate per-character x positions using cosmic_text ────────────────────

/// Populate `line.char_x` with real glyph x positions (relative to `line.x`).
///
/// Each entry `char_x[i]` is the visual x of the caret at byte offset
/// `line.text_start + i` within `flat`.  Uses the same shaping as the renderer
/// (Basic for ASCII, Advanced otherwise) so click-to-caret and caret rendering
/// agree exactly with the rendered text positions.
pub fn fill_char_x_for_line(
    fs: &mut cosmic_text::FontSystem,
    flat: &str,
    runs: &[InlineRun],
    line: &mut LayoutLine,
    scale: f32,
) {
    let line_start = line.text_start;
    let line_end = (line.text_start + line.text_length).min(flat.len());
    let range_len = line_end.saturating_sub(line_start);
    if range_len == 0 {
        return;
    }

    // One entry per byte boundary plus one for end-of-line.
    let mut positions = vec![f32::NAN; range_len + 1];

    let inv_scale = if scale > 0.0 { 1.0 / scale } else { 1.0 };

    // Helper: measure a text segment and fill char_x positions
    let measure_segment = |fs: &mut cosmic_text::FontSystem,
                           s: usize,
                           e: usize,
                           cursor_x: f32,
                           run: &InlineRun,
                           positions: &mut Vec<f32>|
     -> f32 {
        let seg_text = &flat[s..e];
        let font_px = run.style.font_size_px(16.0, 16.0);
        let ct_w = weight_from_style(
            run.style.font_weight,
            &run.style.rare().font_variation_settings,
        );
        let ct_s = match run.style.font_style {
            FontStyle::Italic => CTextStyle::Italic,
            FontStyle::Oblique => CTextStyle::Oblique,
            FontStyle::Normal => CTextStyle::Normal,
        };
        let ct_stretch = stretch_from_percent(run.style.font_stretch);
        let size_adjust = font_size_adjust_scale(fs, &run.style.font_family);
        let phys_px = font_px * size_adjust * scale;
        let metrics = Metrics::new(phys_px, phys_px * 1.2);
        let mut buf = Buffer::new(fs, metrics);
        let resolved = resolve_css_family(fs, &run.style.font_family);
        let family = resolved.as_family();
        let attrs = Attrs::new()
            .weight(ct_w)
            .style(ct_s)
            .stretch(ct_stretch)
            .family(family);
        buf.set_text(fs, seg_text, &attrs, Shaping::Advanced, None);
        buf.shape_until_scroll(fs, false);

        let mut seg_advance = 0.0f32;
        for lr in buf.layout_runs() {
            for glyph in lr.glyphs {
                let abs_s = s + glyph.start;
                let abs_e = s + glyph.end;
                let i_s = abs_s.saturating_sub(line_start);
                let i_e = abs_e.saturating_sub(line_start).min(positions.len() - 1);
                let x0 = cursor_x + glyph.x * inv_scale;
                let x1 = cursor_x + (glyph.x + glyph.w) * inv_scale;
                let span = (i_e.saturating_sub(i_s)).max(1);
                for k in 0..=span {
                    let idx = i_s + k;
                    if idx < positions.len() && positions[idx].is_nan() {
                        positions[idx] = x0 + (x1 - x0) * k as f32 / span as f32;
                    }
                }
                let right = x1 - cursor_x;
                if right > seg_advance {
                    seg_advance = right;
                }
            }
            let lw = lr.line_w * inv_scale;
            if lw > seg_advance {
                seg_advance = lw;
            }
        }
        // The shaper knows nothing about the two spacing properties, so the
        // segment's advance has to carry them or this cursor drifts out of
        // step with the item advances the line was built from.
        let word_s = run.style.word_spacing.resolve(font_px, 0.0, 16.0);
        let letter_s = run.style.letter_spacing.resolve(font_px, 0.0, 16.0);
        let extra = line.extra_space_per_word;
        let mut adjustment = 0.0;
        for (rel, ch) in seg_text.char_indices() {
            let idx = s + rel - line_start;
            if idx < positions.len() && positions[idx].is_finite() {
                positions[idx] += adjustment;
            }
            adjustment += letter_s;
            if ch == ' ' {
                adjustment += word_s + extra;
            }
        }
        let end_idx = e - line_start;
        if end_idx < positions.len() && positions[end_idx].is_finite() {
            positions[end_idx] += adjustment;
        }
        let n_spc = seg_text.chars().filter(|&c| c == ' ').count() as f32;
        let n_chars = seg_text.chars().count() as f32;
        seg_advance + n_spc * (word_s + extra) + n_chars * letter_s
    };

    let mut cursor_x = 0.0f32;

    if !line.visual_segments.is_empty() {
        // BiDi: iterate in visual segment order so cursor_x advances
        // left-to-right in visual order, and RTL text gets correct positions.
        for vs_idx in 0..line.visual_segments.len() {
            let vs_start = line.visual_segments[vs_idx].logical_start;
            let vs_end = vs_start + line.visual_segments[vs_idx].length;
            let segment_x = cursor_x;

            // Find the inline run(s) that cover this visual segment
            for run in runs.iter() {
                let rs = line_start.max(run.text_offset);
                let re = line_end.min(run.text_offset + run.length);
                let cs = vs_start.max(rs);
                let ce = vs_end.min(re);
                if cs >= ce {
                    continue;
                }

                let mut s = cs;
                while s < flat.len() && !flat.is_char_boundary(s) {
                    s += 1;
                }
                let mut e = ce;
                while e > 0 && !flat.is_char_boundary(e) {
                    e -= 1;
                }
                if s >= e {
                    continue;
                }

                let advance = measure_segment(fs, s, e, cursor_x, run, &mut positions);
                cursor_x += advance;
            }

            line.visual_segments[vs_idx].x = segment_x;
            line.visual_segments[vs_idx].width = (cursor_x - segment_x).max(0.0);
        }

        // Forward-fill NaN gaps
        {
            let mut last = 0.0f32;
            for p in positions.iter_mut() {
                if p.is_nan() {
                    *p = last;
                } else {
                    last = *p;
                }
            }
        }

        line.char_x = positions;
        return;
    }

    // No BiDi: iterate inline runs in DOM order (all LTR)
    for run in runs {
        let seg_s = line_start.max(run.text_offset);
        let seg_e = line_end.min(run.text_offset + run.length);
        if seg_s >= seg_e {
            continue;
        }

        let mut s = seg_s;
        while s < flat.len() && !flat.is_char_boundary(s) {
            s += 1;
        }
        let mut e = seg_e;
        while e > 0 && !flat.is_char_boundary(e) {
            e -= 1;
        }
        if s >= e {
            continue;
        }

        let advance = measure_segment(fs, s, e, cursor_x, run, &mut positions);
        cursor_x += advance;
    }

    // End-of-line position.
    positions[range_len] = cursor_x;

    // Forward-fill NaN gaps (intermediate bytes of multi-byte / ligature glyphs).
    let mut last = 0.0f32;
    for p in positions.iter_mut() {
        if p.is_nan() {
            *p = last;
        } else {
            last = *p;
        }
    }

    line.char_x = positions;
}

// ─── Collect flat text (same traversal as collect_items) ─────────────────────
/// Used by the renderer to map text_start offsets back to characters.
pub fn collect_flat_text(node: &WebCore) -> String {
    let mut out = String::new();
    collect_flat_text_inner(node, &mut out, true);
    out
}

fn collect_flat_text_inner(node: &WebCore, out: &mut String, is_root: bool) {
    let generated_content = node.style.rare().content.as_str();
    if !is_root
        && matches!(
            node.style.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        )
    {
        return;
    }
    if !is_root && !node.style.before_content.is_empty() {
        push_flat_rendered_text(out, &node.style.before_content, &node.style);
    }
    if node.is_text_node() {
        let rendered_text = if generated_content.is_empty() {
            node.text.as_str()
        } else {
            generated_content
        };
        push_flat_rendered_text(out, rendered_text, &node.style);
        return;
    }
    if matches!(node.style.display, Display::None) {
        return;
    }
    if node.tag == "br" {
        return;
    }
    // Floats are emitted as Float items in collect_items and their text is not
    // counted in text_offset — skip them here to keep byte offsets in sync.
    // Exception: when called as root (rendering the float itself), include its text.
    if !is_root && !matches!(node.style.float, crate::types::Float::None) {
        return;
    }
    // Atomic inline-blocks are emitted as Atomic items by the parent; their internal
    // text is NOT part of the parent's flat-text string. However when we are rendering
    // the inline-block itself (is_root=true) we DO want its own text content.
    let rendered_text = if generated_content.is_empty() {
        node.text.as_str()
    } else {
        generated_content
    };
    if !rendered_text.is_empty() && (is_root || node.children.is_empty()) {
        out.push_str(rendered_text);
    }
    if !generated_content.is_empty() {
        return;
    }
    let children = node.effective_children();
    for child in children {
        // Skip out-of-flow children — they don't contribute to this box's flat text.
        // (collect_flat_text is called separately on each positioned box for its own content.)
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        collect_flat_text_inner(child, out, false);
    }
    if !is_root && !node.style.after_content.is_empty() {
        push_flat_rendered_text(out, &node.style.after_content, &node.style);
    }
}

fn push_flat_rendered_text(out: &mut String, text: &str, style: &ComputedStyle) {
    // Normalize newlines/tabs to spaces in normal white-space mode so that flat
    // text matches what tokenize_text rendered.
    if matches!(style.white_space, WhiteSpace::Normal | WhiteSpace::Nowrap) {
        for c in text.chars() {
            out.push(if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            });
        }
    } else {
        out.push_str(text);
    }
}

// ─── Recursive inline-block pre-layout ───────────────────────────────────────

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

fn shrink_to_fit_intrinsic_width(line_w: f32, max_content_w: f32) -> f32 {
    if max_content_w > 0.0 {
        max_content_w
    } else {
        line_w.max(0.0)
    }
}

fn is_atomic_inline_replaced(node: &WebCore) -> bool {
    node.is_image_element() || matches!(node.tag.as_str(), "svg" | "canvas" | "video" | "iframe")
}

/// Recursively walk inline children and pre-layout any nested inline-block
/// elements (e.g. `<input>` inside `<label>`).  This ensures that when
/// `collect_items` encounters an `InlineBlock`, its `margin_rect` is non-zero
/// so the item gets the correct advance width and ascent.
fn prelayout_nested_inline_blocks(
    engine: &LayoutEngine,
    node: &mut WebCore,
    content_w: f32,
    font_px: f32,
    root_font_px: f32,
) {
    for ci in 0..node.children.len() {
        if matches!(node.children[ci].style.display, Display::None) {
            continue;
        }
        if matches!(
            node.children[ci].style.position,
            Position::Absolute | Position::Fixed
        ) {
            continue;
        }
        if !matches!(node.children[ci].style.float, crate::types::Float::None) {
            engine.layout_box(
                &mut node.children[ci],
                &Constraints::new(content_w, 0.0, 0.0, font_px, root_font_px),
            );
            if node.children[ci].style.width.is_auto() {
                let max_line_w = node.children[ci]
                    .layout
                    .line_cache
                    .iter()
                    .map(|l| l.width)
                    .fold(0.0_f32, f32::max);
                let max_content_w =
                    engine.max_content_width(&node.children[ci], font_px, root_font_px);
                let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                let fc = &node.children[ci];
                let shrink_w = intrinsic_w.ceil()
                    + shrink_to_fit_slop(fc)
                    + fc.layout.resolved_pad_left
                    + fc.layout.resolved_pad_right
                    + fc.layout.resolved_border_left
                    + fc.layout.resolved_border_right
                    + fc.layout.resolved_margin_left
                    + fc.layout.resolved_margin_right;
                if shrink_w > 0.0 && shrink_w < content_w {
                    engine.layout_box(
                        &mut node.children[ci],
                        &Constraints::new(shrink_w, 0.0, 0.0, font_px, root_font_px),
                    );
                }
            }
            continue;
        }
        if matches!(
            node.children[ci].style.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        ) || is_atomic_inline_replaced(&node.children[ci])
        {
            // When called from the top-level (block container), direct inline-block
            // children are already handled by the step 0 loop in layout_inline_block().
            // Only lay out here in the recursive case (node is an inline wrapper,
            // e.g. span > a > img where this function was called on the <a>).
            if matches!(node.style.display, Display::Inline | Display::Contents) {
                engine.layout_box(
                    &mut node.children[ci],
                    &Constraints::new(content_w, 0.0, 0.0, font_px, root_font_px),
                );
                if node.children[ci].style.width.is_auto() {
                    let max_line_w = node.children[ci]
                        .layout
                        .line_cache
                        .iter()
                        .map(|l| l.width)
                        .fold(0.0_f32, f32::max);
                    let max_content_w =
                        engine.max_content_width(&node.children[ci], font_px, root_font_px);
                    let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                    let gc = &node.children[ci];
                    let shrink_w = intrinsic_w.ceil()
                        + shrink_to_fit_slop(gc)
                        + gc.layout.resolved_pad_left
                        + gc.layout.resolved_pad_right
                        + gc.layout.resolved_border_left
                        + gc.layout.resolved_border_right
                        + gc.layout.resolved_margin_left
                        + gc.layout.resolved_margin_right;
                    if shrink_w < content_w {
                        engine.layout_box(
                            &mut node.children[ci],
                            &Constraints::new(shrink_w, 0.0, 0.0, font_px, root_font_px),
                        );
                    }
                }
            }
            continue;
        }
        // Inline children: recurse to find nested inline-blocks.
        if matches!(
            node.children[ci].style.display,
            Display::Inline | Display::Contents
        ) {
            let child_font_px = node.children[ci].style.font_size_px(font_px, root_font_px);
            // Pre-layout any inline-block grandchildren inside this inline child.
            for gci in 0..node.children[ci].children.len() {
                let grandchild_display = node.children[ci].children[gci].style.display;
                if !matches!(
                    node.children[ci].children[gci].style.float,
                    crate::types::Float::None
                ) {
                    engine.layout_box(
                        &mut node.children[ci].children[gci],
                        &Constraints::new(content_w, 0.0, 0.0, child_font_px, root_font_px),
                    );
                    if node.children[ci].children[gci].style.width.is_auto() {
                        let max_line_w = node.children[ci].children[gci]
                            .layout
                            .line_cache
                            .iter()
                            .map(|l| l.width)
                            .fold(0.0_f32, f32::max);
                        let max_content_w = engine.max_content_width(
                            &node.children[ci].children[gci],
                            font_px,
                            root_font_px,
                        );
                        let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                        let gc = &node.children[ci].children[gci];
                        let shrink_w = intrinsic_w.ceil()
                            + shrink_to_fit_slop(gc)
                            + gc.layout.resolved_pad_left
                            + gc.layout.resolved_pad_right
                            + gc.layout.resolved_border_left
                            + gc.layout.resolved_border_right
                            + gc.layout.resolved_margin_left
                            + gc.layout.resolved_margin_right;
                        if shrink_w > 0.0 && shrink_w < content_w {
                            engine.layout_box(
                                &mut node.children[ci].children[gci],
                                &Constraints::new(shrink_w, 0.0, 0.0, child_font_px, root_font_px),
                            );
                        }
                    }
                } else if matches!(
                    grandchild_display,
                    Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
                ) || is_atomic_inline_replaced(&node.children[ci].children[gci])
                {
                    engine.layout_box(
                        &mut node.children[ci].children[gci],
                        &Constraints::new(content_w, 0.0, 0.0, child_font_px, root_font_px),
                    );
                    // Shrink-to-fit for auto-width nested inline-blocks
                    if node.children[ci].children[gci].style.width.is_auto() {
                        let max_line_w = node.children[ci].children[gci]
                            .layout
                            .line_cache
                            .iter()
                            .map(|l| l.width)
                            .fold(0.0_f32, f32::max);
                        let max_content_w = engine.max_content_width(
                            &node.children[ci].children[gci],
                            font_px,
                            root_font_px,
                        );
                        let intrinsic_w = shrink_to_fit_intrinsic_width(max_line_w, max_content_w);
                        let gc = &node.children[ci].children[gci];
                        let shrink_w = intrinsic_w.ceil()
                            + shrink_to_fit_slop(gc)
                            + gc.layout.resolved_pad_left
                            + gc.layout.resolved_pad_right
                            + gc.layout.resolved_border_left
                            + gc.layout.resolved_border_right
                            + gc.layout.resolved_margin_left
                            + gc.layout.resolved_margin_right;
                        if shrink_w < content_w {
                            engine.layout_box(
                                &mut node.children[ci].children[gci],
                                &Constraints::new(shrink_w, 0.0, 0.0, child_font_px, root_font_px),
                            );
                        }
                    }
                } else if matches!(grandchild_display, Display::Inline) {
                    // One more level: recurse for deeper nesting.
                    prelayout_nested_inline_blocks(
                        engine,
                        &mut node.children[ci].children[gci],
                        content_w,
                        child_font_px,
                        root_font_px,
                    );
                }
            }
        }
    }
}

fn has_in_flow_block_children(node: &WebCore) -> bool {
    node.effective_children().iter().any(|c| {
        if matches!(c.style.display, Display::None) {
            return false;
        }
        if matches!(c.style.display, Display::Contents) {
            return has_in_flow_block_children(c);
        }
        matches!(
            c.style.position,
            Position::Static | Position::Relative | Position::Sticky
        ) && matches!(c.style.float, crate::types::Float::None)
            && c.style.is_block_level()
    })
}
