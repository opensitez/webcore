use super::Constraints;
#[allow(unused_imports)]
use crate::css::parse_single_track;
use crate::layout::block::apply_relative_offset;
use crate::layout::{layout_positioned, shift_rects, LayoutEngine, ResolvedBox};
use crate::types::*;

/// Resolve a child by path through `display: contents` wrappers.
pub fn grid_child_ref<'a>(node: &'a WebCore, path: &[usize]) -> &'a WebCore {
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
pub fn grid_child_mut<'a>(node: &'a mut WebCore, path: &[usize]) -> &'a mut WebCore {
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

/// Collect effective children, flattening `display: contents`.
/// Used by grid, and also by block/flex layout for display:contents support.
pub fn collect_grid_children(node: &WebCore) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    let mut path = Vec::new();
    collect_grid_inner(node, &mut path, &mut result);
    result
}
fn collect_grid_inner(node: &WebCore, path: &mut Vec<usize>, result: &mut Vec<Vec<usize>>) {
    for (idx, child) in node.effective_children().iter().enumerate() {
        path.push(idx);
        if matches!(child.style.display, Display::Contents) {
            collect_grid_inner(child, path, result);
        } else {
            result.push(path.clone());
        }
        path.pop();
    }
}

// ─── Subgrid support ─────────────────────────────────────────────────────────

/// Resolved track context passed from a parent grid into a subgrid child.
#[derive(Clone, Debug)]
pub struct SubgridContext {
    /// Pixel widths of parent grid columns.
    pub col_px: Vec<f32>,
    /// X offsets of parent columns relative to parent content_x.
    pub col_x: Vec<f32>,
    /// Column gap from parent.
    pub col_gap: f32,
    /// Pixel heights of parent grid rows.
    pub row_heights: Vec<f32>,
    /// Y offsets of parent rows relative to parent content_y.
    pub row_y: Vec<f32>,
    /// Row gap from parent.
    pub row_gap: f32,
    /// 0-based column span this subgrid occupies: [start, end)
    pub col_span: (usize, usize),
    /// 0-based row span this subgrid occupies: [start, end)
    pub row_span: (usize, usize),
}

impl SubgridContext {
    fn inherited_col_px(&self) -> &[f32] {
        let (cs, ce) = self.col_span;
        &self.col_px[cs.min(self.col_px.len())..ce.min(self.col_px.len())]
    }
    fn inherited_row_heights(&self) -> &[f32] {
        let (rs, re) = self.row_span;
        &self.row_heights[rs.min(self.row_heights.len())..re.min(self.row_heights.len())]
    }
    fn local_col_x(&self) -> Vec<f32> {
        let (cs, ce) = self.col_span;
        let cs = cs.min(self.col_x.len());
        let ce = ce.min(self.col_x.len());
        let origin = self.col_x.get(cs).copied().unwrap_or(0.0);
        self.col_x[cs..ce].iter().map(|&x| x - origin).collect()
    }
    fn local_row_y(&self) -> Vec<f32> {
        let (rs, re) = self.row_span;
        let rs = rs.min(self.row_y.len());
        let re = re.min(self.row_y.len());
        let origin = self.row_y.get(rs).copied().unwrap_or(0.0);
        self.row_y[rs..re].iter().map(|&y| y - origin).collect()
    }
}

/// Layout a grid item that uses `grid-template-columns: subgrid` and/or
/// `grid-template-rows: subgrid`. Inherits track sizes from the parent.
pub fn layout_grid_subgrid(
    engine: &LayoutEngine,
    node: &mut WebCore,
    rbox: &ResolvedBox,
    ctx: &SubgridContext,
    x: f32,
    y: f32,
    font_px: f32,
    root_font_px: f32,
) -> f32 {
    let content_x = x + rbox.margin_left + rbox.border_left + rbox.padding_left;
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    let col_is_sub = node.style.subgrid_columns;
    let row_is_sub = node.style.subgrid_rows;

    // --- Column axis ---
    let (col_px, col_x_local, col_gap) = if col_is_sub {
        (
            ctx.inherited_col_px().to_vec(),
            ctx.local_col_x(),
            ctx.col_gap,
        )
    } else {
        let cw = rbox.content_width.unwrap_or({
            ctx.inherited_col_px().iter().sum::<f32>()
                + ctx.col_gap * ctx.inherited_col_px().len().saturating_sub(1) as f32
        });
        let tracks = resolve_track_sizes(
            &node.style.rare().grid_template_columns,
            &node.style.rare().auto_repeat_columns,
            cw,
            font_px,
            root_font_px,
        );
        let gap = engine.res_len(&node.style.column_gap, font_px, cw, root_font_px);
        let n = tracks.len().max(1);
        let dummy_widths = vec![0.0f32; n];
        let px = resolve_to_pixels(
            &tracks,
            &node.style.grid_auto_columns,
            cw,
            gap,
            n,
            font_px,
            root_font_px,
            &dummy_widths,
            &dummy_widths,
        );
        let x_offsets: Vec<f32> = {
            let mut xs = Vec::with_capacity(px.len());
            let mut cx = 0.0f32;
            for &w in &px {
                xs.push(cx);
                cx += w + gap;
            }
            xs
        };
        (px, x_offsets, gap)
    };

    let content_w: f32 =
        col_px.iter().sum::<f32>() + col_gap * col_px.len().saturating_sub(1) as f32;
    let n_cols = col_px.len().max(1);
    // --- Collect visible items ---
    let mut item_indices: Vec<Vec<usize>> = collect_grid_children(node)
        .into_iter()
        .filter(|path| {
            let c = grid_child_ref(node, path);
            !matches!(c.style.display, Display::None)
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                && !(c.tag == "#text" && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
        })
        .collect();
    item_indices.sort_by(|a, b| {
        grid_child_ref(node, a)
            .style
            .order
            .cmp(&grid_child_ref(node, b).style.order)
    });
    let n_items = item_indices.len();

    // CSS Grid §5.4: blockify grid items — inline items become block-level.
    for path in &item_indices {
        let child = grid_child_mut(node, path);
        match child.style.display {
            Display::Inline => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Block;
            }
            Display::InlineFlex => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Flex;
            }
            Display::InlineGrid => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Grid;
            }
            _ => {}
        }
    }

    // --- Placement ---
    let area_map = build_area_map(&node.style.rare().grid_template_areas);
    let sub_col_names = node.style.grid_col_line_names.clone();
    let sub_row_names = node.style.grid_row_line_names.clone();
    let mut placements: Vec<(usize, usize, usize, usize)> = vec![(0, 1, 0, 1); n_items];
    let mut max_row = 0usize;
    let mut max_col = n_cols;

    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        let n_sub_rows = node.style.rare().grid_template_rows.len();
        let (cs, ce, rs, re) = resolve_placement(
            child,
            &area_map,
            n_cols,
            n_sub_rows,
            &sub_col_names,
            &sub_row_names,
        );
        placements[ii] = (cs, ce, rs, re);
        if ce > max_col {
            max_col = ce;
        }
        if re > max_row {
            max_row = re;
        }
    }

    // Auto-place (row-flow)
    let mut occ: Vec<Vec<bool>> = vec![vec![false; max_col.max(1)]; (max_row + n_items + 1).max(1)];
    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        if is_explicitly_placed(child, &area_map, &sub_col_names, &sub_row_names) {
            let (cs, ce, rs, re) = placements[ii];
            for r in rs..re {
                ensure_row(&mut occ, r, max_col);
                for c in cs..ce {
                    if c < occ[r].len() {
                        occ[r][c] = true;
                    }
                }
            }
        }
    }
    let mut auto_row = 0usize;
    let mut auto_col = 0usize;
    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        if is_explicitly_placed(child, &area_map, &sub_col_names, &sub_row_names) {
            continue;
        }

        // If column is definite but row is auto, preserve the resolved column
        // placement and only auto-place the row.
        let (pcs, pce, _, _) = placements[ii];
        let (cs_span, cs_val) = decode_grid_line(child.style.grid_column_start);
        let col_is_definite = (!cs_span && cs_val != 0)
            || (!child.style.grid_column_start_name.is_empty()
                && lookup_named_line(&child.style.grid_column_start_name, &sub_col_names)
                    .is_some());

        let (use_cs, use_ce, span_col) = if col_is_definite {
            // Column already resolved — keep it
            (pcs, pce, (pce - pcs).max(1))
        } else {
            let sc = get_span_col(child).min(n_cols);
            (auto_col, auto_col + sc, sc)
        };
        let span_row = get_span_row(child);

        // Auto-place the row
        if col_is_definite {
            auto_col = use_cs;
        }
        'outer: loop {
            if auto_row > MAX_GRID_SPAN {
                break 'outer;
            }
            ensure_row(&mut occ, auto_row + span_row, max_col);
            if auto_col + span_col > max_col {
                auto_row += 1;
                auto_col = if col_is_definite { use_cs } else { 0 };
                continue;
            }
            let mut fits = true;
            'chk: for r in auto_row..auto_row + span_row {
                ensure_row(&mut occ, r, max_col);
                for c in auto_col..auto_col + span_col {
                    if c < occ[r].len() && occ[r][c] {
                        fits = false;
                        break 'chk;
                    }
                }
            }
            if fits {
                break 'outer;
            }
            auto_col += 1;
            if auto_col + span_col > max_col {
                auto_row += 1;
                auto_col = if col_is_definite { use_cs } else { 0 };
            }
        }
        let final_cs = if col_is_definite { use_cs } else { auto_col };
        let final_ce = if col_is_definite {
            use_ce
        } else {
            auto_col + span_col
        };
        placements[ii] = (final_cs, final_ce, auto_row, auto_row + span_row);
        for r in auto_row..auto_row + span_row {
            ensure_row(&mut occ, r, max_col);
            for c in auto_col..auto_col + span_col {
                if c < occ[r].len() {
                    occ[r][c] = true;
                }
            }
        }
        if placements[ii].3 > max_row {
            max_row = placements[ii].3;
        }
        auto_col += span_col;
    }
    let n_rows = max_row.max(1);

    // --- Row axis ---
    let (row_heights, row_y_local, row_gap) = if row_is_sub {
        (
            ctx.inherited_row_heights().to_vec(),
            ctx.local_row_y(),
            ctx.row_gap,
        )
    } else {
        let rgap = engine.res_len(&node.style.row_gap, font_px, content_w, root_font_px);
        let mut heights = vec![0.0f32; n_rows];
        // First pass: measure children
        for (ii, path) in item_indices.iter().enumerate() {
            let (cs, ce, rs, re) = placements[ii];
            let cs = cs.min(n_cols.saturating_sub(1));
            let ce = ce.min(n_cols).max(cs + 1);
            let sw = span_width(&col_px, &col_x_local, cs, ce, col_gap, content_w);
            let child = grid_child_mut(node, path);
            engine.layout_box(
                child,
                &Constraints::new(sw, content_x, 0.0, font_px, root_font_px),
            );
            let cf = child.style.font_size_px(font_px, root_font_px);
            let cr = engine.res_box(&child.style, cf, sw, root_font_px);
            let h = child.layout.border_rect.h + cr.margin_top + cr.margin_bottom;
            let rspan = (re - rs).max(1);
            let hp = h / rspan as f32;
            for r in rs..re.min(n_rows) {
                if hp > heights[r] {
                    heights[r] = hp;
                }
            }
        }
        // Apply explicit row track sizes
        for (ri, track) in node.style.rare().grid_template_rows.iter().enumerate() {
            if ri < heights.len() {
                let px = track_to_px(track, content_w, font_px, root_font_px);
                if px > 0.0 && px > heights[ri] {
                    heights[ri] = px;
                }
            }
        }
        let ys: Vec<f32> = {
            let mut v = Vec::with_capacity(n_rows);
            let mut cy = 0.0f32;
            for &h in &heights {
                v.push(cy);
                cy += h + rgap;
            }
            v
        };
        (heights, ys, rgap)
    };

    // --- Second pass: position children ---
    let node_align_items = node.style.align_items;
    let node_justify_items = node.style.justify_items;
    for (ii, path) in item_indices.iter().enumerate() {
        let (cs, ce, rs, re) = placements[ii];
        let cs = cs.min(n_cols.saturating_sub(1));
        let ce = ce.min(n_cols).max(cs + 1);
        let rs = rs.min(n_rows.saturating_sub(1));
        let re = re.min(n_rows).max(rs + 1);

        let sw = span_width(&col_px, &col_x_local, cs, ce, col_gap, content_w);
        let cell_h =
            row_heights[rs..re].iter().sum::<f32>() + row_gap * (re - rs).saturating_sub(1) as f32;
        let ix = content_x + col_x_local.get(cs).copied().unwrap_or(0.0);
        let iy = content_y + row_y_local.get(rs).copied().unwrap_or(0.0);

        let child = grid_child_mut(node, path);
        let cf = child.style.font_size_px(font_px, root_font_px);
        let eff_align = effective_align_self_grid(child, node_align_items);
        if eff_align == AlignItems::Stretch && child.style.height.is_auto() {
            let cr = engine.res_box(&child.style, cf, sw, root_font_px);
            let css_h = if child.style.box_sizing == BoxSizing::BorderBox {
                (cell_h - cr.margin_top - cr.margin_bottom).max(0.0)
            } else {
                (cell_h
                    - cr.margin_top
                    - cr.margin_bottom
                    - cr.padding_top
                    - cr.padding_bottom
                    - cr.border_top
                    - cr.border_bottom)
                    .max(0.0)
            };
            let saved = child.style.height.clone();
            std::sync::Arc::make_mut(&mut child.style).height = CssLength::Px(css_h);
            engine.layout_box(child, &Constraints::new(sw, ix, iy, font_px, root_font_px));
            std::sync::Arc::make_mut(&mut child.style).height = saved;
        } else {
            engine.layout_box(child, &Constraints::new(sw, ix, iy, font_px, root_font_px));
        }

        let cr = engine.res_box(&child.style, cf, sw, root_font_px);
        let eff_justify = effective_justify_self(child, node_justify_items);
        let cell_w = sw;
        let dx_align = match eff_justify {
            AlignItems::FlexEnd => {
                cell_w - child.layout.border_rect.w - cr.margin_left - cr.margin_right
            }
            AlignItems::Center => {
                (cell_w - child.layout.border_rect.w - cr.margin_left - cr.margin_right) / 2.0
            }
            _ => 0.0,
        };
        let dy_align = match eff_align {
            AlignItems::FlexEnd => {
                cell_h - child.layout.border_rect.h - cr.margin_top - cr.margin_bottom
            }
            AlignItems::Center => {
                (cell_h - child.layout.border_rect.h - cr.margin_top - cr.margin_bottom) / 2.0
            }
            _ => 0.0,
        };
        let target_x = ix + cr.margin_left + dx_align;
        let target_y = iy + cr.margin_top + dy_align;
        let dx = target_x - child.layout.border_rect.x;
        let dy = target_y - child.layout.border_rect.y;
        shift_rects(child, dx, dy);

        if matches!(child.style.position, Position::Relative | Position::Sticky) {
            apply_relative_offset(child, cf, content_w, root_font_px);
        }
    }

    let total_h =
        row_y_local.last().copied().unwrap_or(0.0) + row_heights.last().copied().unwrap_or(0.0);
    let ch = rbox.content_height.unwrap_or(total_h);

    node.layout.layout_dirty = false;
    layout_abs_children(engine, node, font_px, root_font_px);
    finish_grid(node, rbox, content_x, content_y, content_w, ch)
}

/// CSS Grid layout.
/// Returns total outer height.
/// Mirrors C++ LayoutGrid.
pub fn layout_grid(
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
    let content_w = match rbox.content_width {
        Some(w) => w,
        None => (containing_w - rbox.h_space()).max(0.0),
    };
    let content_x = x + rbox.margin_left + rbox.border_left + rbox.padding_left;
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    let col_gap = engine.res_len(&node.style.column_gap, font_px, content_w, root_font_px);
    let row_gap = engine.res_len(&node.style.row_gap, font_px, content_w, root_font_px);

    // Resolve column track sizes (pass gap for auto-fill/fit count)
    let col_gap_for_count =
        engine.res_len(&node.style.column_gap, font_px, content_w, root_font_px);
    let col_tracks = resolve_track_sizes_with_gap(
        &node.style.rare().grid_template_columns,
        &node.style.rare().auto_repeat_columns,
        content_w,
        font_px,
        root_font_px,
        col_gap_for_count,
    );
    let row_tracks = node.style.rare().grid_template_rows.clone();

    // Collect visible items (non-abs-positioned)
    // CSS Grid §4: whitespace-only anonymous grid items are not rendered
    // display:contents wrappers are flattened so their children become grid items
    let mut item_indices: Vec<Vec<usize>> = collect_grid_children(node)
        .into_iter()
        .filter(|path| {
            let c = grid_child_ref(node, path);
            !matches!(c.style.display, Display::None)
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                && !(c.tag == "#text" && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
        })
        .collect();
    // Sort by order property (CSS order: default 0)
    item_indices.sort_by(|a, b| {
        grid_child_ref(node, a)
            .style
            .order
            .cmp(&grid_child_ref(node, b).style.order)
    });

    // CSS Grid §5.4: blockify grid items — inline items become block-level.
    for path in &item_indices {
        let child = grid_child_mut(node, path);
        match child.style.display {
            Display::Inline => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Block;
            }
            Display::InlineFlex => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Flex;
            }
            Display::InlineGrid => {
                std::sync::Arc::make_mut(&mut child.style).display = Display::Grid;
            }
            _ => {}
        }
    }

    let n_items = item_indices.len();
    if n_items == 0 {
        let ch = rbox.content_height.unwrap_or(0.0);
        node.layout.layout_dirty = false;
        let result = finish_grid(node, rbox, content_x, content_y, content_w, ch);
        layout_abs_children(engine, node, font_px, root_font_px);
        return result;
    }

    // ── Grid placement algorithm ─────────────────────────────────────────────

    // Named area lookup
    let area_map = build_area_map(&node.style.rare().grid_template_areas);
    let col_line_names = node.style.grid_col_line_names.clone();
    let row_line_names = node.style.grid_row_line_names.clone();

    let n_explicit_cols = col_tracks.len();
    let n_explicit_rows = row_tracks.len();

    // Pre-compute area dimensions from grid-template-areas
    let area_cols = if !node.style.rare().grid_template_areas.is_empty() {
        node.style.rare().grid_template_areas[0].len()
    } else {
        0
    };
    let area_rows = node.style.rare().grid_template_areas.len();

    // Placement result: (col_start, col_end, row_start, row_end) — all 0-based
    let mut placements: Vec<(usize, usize, usize, usize)> = vec![(0, 1, 0, 1); n_items];

    // Pass 1: Place items with explicit positions
    let mut max_row = if area_rows > 0 { area_rows } else { 0 };
    let mut max_col = if n_explicit_cols > 0 {
        n_explicit_cols
    } else if area_cols > 0 {
        area_cols
    } else {
        1
    };

    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        let (cs, ce, rs, re) = resolve_placement(
            child,
            &area_map,
            n_explicit_cols,
            n_explicit_rows,
            &col_line_names,
            &row_line_names,
        );
        placements[ii] = (cs, ce, rs, re);
        if ce > max_col {
            max_col = ce;
        }
        if re > max_row {
            max_row = re;
        }
    }

    // Pass 2: Auto-place remaining items
    let col_flow = matches!(
        node.style.grid_auto_flow,
        GridAutoFlow::Column | GridAutoFlow::ColumnDense
    );
    let dense = matches!(
        node.style.grid_auto_flow,
        GridAutoFlow::RowDense | GridAutoFlow::ColumnDense
    );

    let mut occ: Vec<Vec<bool>> = vec![vec![false; max_col.max(1)]; (max_row + n_items + 1).max(1)];

    // Mark explicitly placed items
    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        if is_explicitly_placed(child, &area_map, &col_line_names, &row_line_names) {
            let (cs, ce, rs, re) = placements[ii];
            for r in rs..re {
                ensure_row(&mut occ, r, max_col);
                for c in cs..ce {
                    if c < occ[r].len() {
                        occ[r][c] = true;
                    }
                }
            }
        }
    }

    // Step 2 (CSS Grid spec §8.5): items with a definite row but auto column.
    // They are locked to their specified row; only the column is auto-placed.
    // The column cursor advances independently per row-start key.
    {
        let mut row_cursors: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        for (ii, path) in item_indices.iter().enumerate() {
            let child = grid_child_ref(node, path);
            if is_explicitly_placed(child, &area_map, &col_line_names, &row_line_names) {
                continue;
            }
            // Row-locked: row_start is a definite line (positive/negative or named, not span/auto)
            let (rs_is_span, rs_val) = decode_grid_line(child.style.grid_row_start);
            let rs_has_name = !child.style.grid_row_start_name.is_empty()
                && lookup_named_line(&child.style.grid_row_start_name, &row_line_names).is_some();
            if !rs_has_name && (rs_is_span || rs_val == 0) {
                continue;
            } // not row-locked

            let span_col = get_span_col(child);
            let rs = resolve_line_start_named(
                child.style.grid_row_start,
                &child.style.grid_row_start_name,
                n_explicit_rows,
                &row_line_names,
            );
            let re = resolve_line_end_named(
                child.style.grid_row_end,
                &child.style.grid_row_end_name,
                rs,
                n_explicit_rows,
                &row_line_names,
            );

            let mut col = row_cursors.get(&rs).copied().unwrap_or(0);
            loop {
                let needed = col + span_col;
                if needed > max_col {
                    max_col = needed;
                }
                ensure_row(&mut occ, re, max_col);
                let mut fits = true;
                'chk2: for r in rs..re {
                    ensure_row(&mut occ, r, max_col);
                    for c in col..col + span_col {
                        if c < occ[r].len() && occ[r][c] {
                            fits = false;
                            break 'chk2;
                        }
                    }
                }
                if fits {
                    break;
                }
                col += 1;
            }

            placements[ii] = (col, col + span_col, rs, re);
            for r in rs..re {
                ensure_row(&mut occ, r, max_col);
                for c in col..col + span_col {
                    if c < occ[r].len() {
                        occ[r][c] = true;
                    }
                }
            }
            if re > max_row {
                max_row = re;
            }
            row_cursors.insert(rs, col + span_col);
        }
    }

    // Step 2.5: Column-locked items (explicit column, auto row).
    // Use the column from resolve_placement (pass 1) and auto-place the row.
    {
        for (ii, path) in item_indices.iter().enumerate() {
            let child = grid_child_ref(node, path);
            if is_explicitly_placed(child, &area_map, &col_line_names, &row_line_names) {
                continue;
            }
            // Check for row-locked (handled in step 2)
            let (rs_is_span, rs_val) = decode_grid_line(child.style.grid_row_start);
            let rs_has_name = !child.style.grid_row_start_name.is_empty()
                && lookup_named_line(&child.style.grid_row_start_name, &row_line_names).is_some();
            if !rs_is_span && rs_val != 0 {
                continue;
            }
            if rs_has_name {
                continue;
            } // row-locked via name, handled in step 2
              // Check if column IS explicitly set (via number or named line)
            let (cs_is_span, cs_val) = decode_grid_line(child.style.grid_column_start);
            let cs_has_name = !child.style.grid_column_start_name.is_empty()
                && lookup_named_line(&child.style.grid_column_start_name, &col_line_names)
                    .is_some();
            if !cs_has_name && (cs_is_span || cs_val == 0) {
                continue;
            } // not column-locked

            // Use columns from pass 1 placement
            let (cs, ce, _rs, _re) = placements[ii];
            let span_row = get_span_row(child);

            // Find first available row at these columns
            let mut row = 0usize;
            loop {
                ensure_row(&mut occ, row + span_row, max_col);
                let mut fits = true;
                'chk3: for r in row..row + span_row {
                    ensure_row(&mut occ, r, max_col);
                    for c in cs..ce {
                        if c < occ[r].len() && occ[r][c] {
                            fits = false;
                            break 'chk3;
                        }
                    }
                }
                if fits {
                    break;
                }
                row += 1;
            }

            placements[ii] = (cs, ce, row, row + span_row);
            for r in row..row + span_row {
                ensure_row(&mut occ, r, max_col);
                for c in cs..ce {
                    if c < occ[r].len() {
                        occ[r][c] = true;
                    }
                }
            }
            if row + span_row > max_row {
                max_row = row + span_row;
            }
        }
    }

    // Step 3: Auto-place remaining (auto row AND auto column)
    let mut auto_row = 0usize;
    let mut auto_col = 0usize;

    for (ii, path) in item_indices.iter().enumerate() {
        let child = grid_child_ref(node, path);
        if is_explicitly_placed(child, &area_map, &col_line_names, &row_line_names) {
            continue;
        }
        // Skip row-locked items (handled in step 2)
        let (rs_is_span2, rs_val2) = decode_grid_line(child.style.grid_row_start);
        let rs_has_name2 = !child.style.grid_row_start_name.is_empty()
            && lookup_named_line(&child.style.grid_row_start_name, &row_line_names).is_some();
        if rs_has_name2 || (!rs_is_span2 && rs_val2 != 0) {
            continue;
        }
        // Skip column-locked items (handled in step 2.5)
        let (cs_is_span2, cs_val2) = decode_grid_line(child.style.grid_column_start);
        let cs_has_name2 = !child.style.grid_column_start_name.is_empty()
            && lookup_named_line(&child.style.grid_column_start_name, &col_line_names).is_some();
        if cs_has_name2 || (!cs_is_span2 && cs_val2 != 0) {
            continue;
        }

        let span_col = get_span_col(child);
        let span_row = get_span_row(child).max(1);
        // If span exceeds grid columns, expand the grid to fit
        if span_col > max_col {
            max_col = span_col;
        }

        if dense {
            auto_row = 0;
            auto_col = 0;
        }

        if col_flow {
            // Column-flow: fill down each column, then move to next column
            // Number of rows per column = explicit row track count (or unlimited if none)
            let col_row_limit = n_rows_from_template(&row_tracks, n_items);
            // auto_col = current column, auto_row = current row within column
            'outer_col: loop {
                if auto_row > MAX_GRID_SPAN || auto_col > MAX_GRID_SPAN {
                    break 'outer_col;
                }
                ensure_row(&mut occ, auto_row + span_row, max_col);
                if auto_row + span_row > col_row_limit {
                    // Move to next column
                    auto_col += 1;
                    auto_row = 0;
                    if auto_col >= max_col {
                        max_col += 1;
                        ensure_row(&mut occ, 1, max_col);
                    }
                    ensure_row(&mut occ, auto_row + span_row, max_col);
                    continue;
                }
                let mut fits = true;
                'check_col: for r in auto_row..auto_row + span_row {
                    ensure_row(&mut occ, r, max_col);
                    for c in auto_col..auto_col + span_col {
                        if c < occ[r].len() && occ[r][c] {
                            fits = false;
                            break 'check_col;
                        }
                    }
                }
                if fits {
                    break 'outer_col;
                }
                auto_row += 1;
            }
        } else {
            // Row-flow: fill across each row, then move to next row
            'outer: loop {
                if auto_row > MAX_GRID_SPAN {
                    break 'outer;
                }
                ensure_row(&mut occ, auto_row + span_row, max_col);
                if auto_col + span_col > max_col {
                    auto_row += 1;
                    auto_col = 0;
                    ensure_row(&mut occ, auto_row + span_row, max_col);
                    continue;
                }
                let mut fits = true;
                'check: for r in auto_row..auto_row + span_row {
                    ensure_row(&mut occ, r, max_col);
                    for c in auto_col..auto_col + span_col {
                        if c < occ[r].len() && occ[r][c] {
                            fits = false;
                            break 'check;
                        }
                    }
                }
                if fits {
                    break 'outer;
                }
                auto_col += 1;
                if auto_col + span_col > max_col {
                    auto_row += 1;
                    auto_col = 0;
                }
            }
        }

        placements[ii] = (auto_col, auto_col + span_col, auto_row, auto_row + span_row);

        // Mark occupied
        if auto_row + span_row <= MAX_GRID_SPAN {
            for r in auto_row..auto_row + span_row {
                ensure_row(&mut occ, r, max_col);
                for c in auto_col..auto_col + span_col {
                    if c < occ[r].len() {
                        occ[r][c] = true;
                    }
                }
            }
        }
        if re_from(&placements[ii]) > max_row {
            max_row = re_from(&placements[ii]);
        }

        if col_flow {
            auto_row += span_row;
        } else {
            auto_col += span_col;
        }
    }

    let n_rows = if max_row > 0 {
        max_row
    } else {
        // Auto rows based on child count
        ((n_items + max_col - 1) / max_col).max(1)
    };
    if n_rows == 0 {
        let ch = rbox.content_height.unwrap_or(0.0);
        node.layout.layout_dirty = false;
        let result = finish_grid(node, rbox, content_x, content_y, content_w, ch);
        layout_abs_children(engine, node, font_px, root_font_px);
        return result;
    }

    // ── Resolve column pixel widths ──────────────────────────────────────────

    // Measure items for intrinsic track sizing (CSS Grid §12.5).
    // A track needs BOTH contributions: `min-content` and the min side of a
    // `minmax()` size from the min-content widths, `max-content`/`auto` from
    // the max-content widths. Using max-content for both made `min-content` a
    // synonym for `max-content`.
    let n_measured_cols = n_explicit_cols.max(max_col);
    let mut col_content_widths = vec![0.0f32; n_measured_cols];
    let mut col_min_widths = vec![0.0f32; n_measured_cols];
    // (col_start, col_end, min-content, max-content) per item.
    let mut col_spans: Vec<(usize, usize, f32, f32)> = Vec::with_capacity(item_indices.len());
    for (ii, path) in item_indices.iter().enumerate() {
        let (cs, ce, _rs, _re) = placements[ii];
        let child = grid_child_ref(node, path);
        // intrinsic_sizes (unified) rather than layout_box(10000), which would
        // leak the dummy width into the child's cached layout.
        let sz = engine.intrinsic_sizes(child, font_px, root_font_px);
        col_spans.push((
            cs.min(n_measured_cols),
            ce.min(n_measured_cols),
            sz.min_content,
            sz.max_content,
        ));
    }
    // ⛔ TWO PHASES, in this order (CSS Grid §12.5) — the same shape the row
    // axis uses. Tracks are sized from single-span items first; a spanning item
    // then contributes only the EXCESS over the tracks it already spans, shared
    // among them, never its whole size divided by its span and used as a floor.
    // The floor inflated a narrow column and squeezed a wide one: `auto auto`
    // holding a 100px and a 40px item plus a 300px item across both gave
    // 150/150 where Chrome gives 180/120.
    for &(cs, ce, mn, mx) in &col_spans {
        if ce.saturating_sub(cs) == 1 && cs < n_measured_cols {
            if span_only_flexible_tracks(&col_tracks, cs, ce) {
                continue;
            }
            if mx > col_content_widths[cs] {
                col_content_widths[cs] = mx;
            }
            if mn > col_min_widths[cs] {
                col_min_widths[cs] = mn;
            }
        }
    }
    let mut col_spanning: Vec<(usize, usize, f32, f32)> = col_spans
        .iter()
        .copied()
        .filter(|(cs, ce, _, _)| ce.saturating_sub(*cs) > 1)
        .collect();
    // Smallest spans first, as the spec requires.
    col_spanning.sort_by_key(|(cs, ce, _, _)| ce - cs);
    for (cs, ce, mn, mx) in col_spanning {
        if ce <= cs {
            continue;
        }
        if span_only_flexible_tracks(&col_tracks, cs, ce) {
            continue;
        }
        let n = ce - cs;
        let gaps = col_gap * n.saturating_sub(1) as f32;
        for (widths, want) in [(&mut col_content_widths, mx), (&mut col_min_widths, mn)] {
            let already: f32 = widths[cs..ce].iter().sum::<f32>() + gaps;
            let extra = want - already;
            if extra <= 0.0 {
                continue;
            }
            let share = extra / n as f32;
            for c in cs..ce {
                widths[c] += share;
            }
        }
    }
    // A track's min-content contribution can never exceed its max-content one.
    for c in 0..n_measured_cols {
        if col_min_widths[c] > col_content_widths[c] {
            col_content_widths[c] = col_min_widths[c];
        }
    }

    let col_px = resolve_to_pixels(
        &col_tracks,
        &node.style.grid_auto_columns,
        content_w,
        col_gap,
        n_explicit_cols.max(max_col),
        font_px,
        root_font_px,
        &col_content_widths,
        &col_min_widths,
    );
    let n_cols_actual = col_px.len();
    // justify-content: compute extra horizontal space distribution
    let grid_total_w: f32 =
        col_px.iter().sum::<f32>() + col_gap * n_cols_actual.saturating_sub(1) as f32;
    let extra_x = (content_w - grid_total_w).max(0.0);
    let (grid_offset_x, extra_gap_col) =
        compute_justify_content(node.style.justify_content, extra_x, n_cols_actual);

    // Column X positions (including justify-content offset)
    let is_rtl = node.style.direction == crate::types::Direction::RTL;
    let col_x: Vec<f32> = {
        let mut xs = vec![0.0f32; n_cols_actual];
        if is_rtl {
            // RTL: column 0 is at the RIGHT, column N-1 at the LEFT
            // col_x values are relative to content_x (added later at positioning)
            let total_gaps = col_gap * n_cols_actual.saturating_sub(1) as f32
                + extra_gap_col * n_cols_actual.saturating_sub(1) as f32;
            let total_cols: f32 = col_px.iter().sum::<f32>();
            let mut cx = grid_offset_x + total_cols + total_gaps;
            for i in 0..n_cols_actual {
                cx -= col_px[i];
                xs[i] = cx;
                cx -= col_gap + extra_gap_col;
            }
        } else {
            let mut cx = grid_offset_x;
            for i in 0..n_cols_actual {
                xs[i] = cx;
                cx += col_px[i] + col_gap + extra_gap_col;
            }
        }
        xs
    };

    // ── First pass: layout all items to get row heights ──────────────────────

    let mut row_heights: Vec<f32> = vec![0.0; n_rows];
    // (row_start, row_end, outer height) per item — applied in two phases below.
    let mut row_spans: Vec<(usize, usize, f32)> = Vec::new();
    let node_align_items = node.style.align_items;
    // Distance from an item's margin edge to its first baseline, per item, for
    // the items that take part in baseline alignment (Box Alignment §9).
    let mut item_ascent: Vec<Option<f32>> = Vec::with_capacity(item_indices.len());

    for (ii, path) in item_indices.iter().enumerate() {
        let (cs, ce, rs, re) = placements[ii];
        let cs = cs.min(n_cols_actual.saturating_sub(1));
        let ce = ce.min(n_cols_actual).max(cs + 1);

        // Compute span width
        // ⛔ A grid AREA spans the gutters between its tracks, INCLUDING the
        // extra a content-distribution value put there. Passing the bare
        // `col_gap` left a spanning item short by `extra_gap_col * (span-1)`
        // in a `justify-content: space-*` grid.
        let span_w = span_width(&col_px, &col_x, cs, ce, col_gap + extra_gap_col, content_w);

        let child = grid_child_mut(node, path);
        let child_font = child.style.font_size_px(font_px, root_font_px);
        let crbox = engine.res_box(&child.style, child_font, span_w, root_font_px);

        // Check for column-subgrid child — must use layout_grid_subgrid so children are
        // placed across the inherited columns instead of being stacked in a single fraction.
        // Row-subgrid is excluded: its row heights come from the parent (not measured here).
        let child_is_col_subgrid = child.style.subgrid_columns
            && !child.style.subgrid_rows
            && matches!(child.style.display, Display::Grid | Display::InlineGrid);

        if child_is_col_subgrid {
            let sctx = SubgridContext {
                col_px: col_px.clone(),
                col_x: col_x.clone(),
                col_gap,
                row_heights: vec![], // not yet known; subgrid measures its own rows
                row_y: vec![],
                row_gap,
                col_span: (cs, ce),
                row_span: (rs, re),
            };
            layout_grid_subgrid(
                engine,
                child,
                &crbox,
                &sctx,
                content_x,
                0.0,
                font_px,
                root_font_px,
            );
        } else {
            engine.layout_box(
                child,
                &Constraints::new(span_w, content_x, 0.0, font_px, root_font_px),
            );
        }

        let h = child.layout.border_rect.h + crbox.margin_top + crbox.margin_bottom;
        row_spans.push((rs, re.min(n_rows), h));
        // A baseline-aligned item is never stretched, so the height and
        // baseline it has here are the ones it keeps.
        item_ascent.push(
            if matches!(
                effective_align_self_grid(child, node_align_items),
                AlignItems::Baseline | AlignItems::LastBaseline
            ) && re.saturating_sub(rs) == 1
            {
                Some(child.layout.baseline - child.layout.margin_rect.y)
            } else {
                None
            },
        );
    }

    // ⛔ TWO PHASES, in this order (CSS Grid §12.5). Tracks are sized from
    // single-span items first; only then does a spanning item contribute, and
    // what it contributes is the EXCESS over the tracks it already spans,
    // shared among them — never its whole size divided by its span and used as
    // a floor. That floor let one tall spanning item inflate every short track
    // it crossed: a 6810px sidebar pushed two ~40px rows to 1811px each.
    //
    // Growth limits are not modelled, so the excess is shared equally rather
    // than freezing tracks as they reach a limit.
    for &(rs, re, h) in &row_spans {
        if re.saturating_sub(rs) <= 1 {
            if rs < n_rows && h > row_heights[rs] {
                row_heights[rs] = h;
            }
        }
    }
    let mut spanning: Vec<(usize, usize, f32)> = row_spans
        .iter()
        .copied()
        .filter(|(rs, re, _)| re.saturating_sub(*rs) > 1)
        .collect();
    // Smallest spans first, as the spec requires.
    spanning.sort_by_key(|(rs, re, _)| re - rs);
    for (rs, re, h) in spanning {
        if re <= rs {
            continue;
        }
        let n = re - rs;
        let already: f32 =
            row_heights[rs..re].iter().sum::<f32>() + row_gap * n.saturating_sub(1) as f32;
        let extra = h - already;
        if extra <= 0.0 {
            continue;
        }
        let share = extra / n as f32;
        for r in rs..re {
            row_heights[r] += share;
        }
    }

    // ── Baseline alignment groups (Box Alignment §9) ────────────────────────
    // The items whose row START is row R and whose `align-self` is a baseline
    // value share a baseline: each is pushed down by the group's largest ascent
    // minus its own. The row must then be tall enough for the shifted group,
    // which is max-ascent + max-descent — larger than the tallest item whenever
    // two items lean opposite ways.
    let mut row_baseline: Vec<Option<f32>> = vec![None; n_rows];
    {
        let mut descent: Vec<f32> = vec![0.0; n_rows];
        for (ii, &(rs, _re, h)) in row_spans.iter().enumerate() {
            if rs >= n_rows {
                continue;
            }
            if let Some(a) = item_ascent[ii] {
                if row_baseline[rs].map_or(true, |m| a > m) {
                    row_baseline[rs] = Some(a);
                }
                if h - a > descent[rs] {
                    descent[rs] = h - a;
                }
            }
        }
        for r in 0..n_rows {
            if let Some(a) = row_baseline[r] {
                let need = a + descent[r];
                if need > row_heights[r] {
                    row_heights[r] = need;
                }
            }
        }
    }

    // ⛔ A ROW track's percentage resolves against the grid's HEIGHT, never its
    // width (CSS Grid §7.2.3). When that height is indefinite the percentage
    // contributes NOTHING and the track behaves as `auto`, which a 0 basis
    // gives exactly. Passing the width here made `grid-template-rows: 50%` in
    // an auto-height grid a fraction of the COLUMN measure.
    let row_pct_basis = rbox.content_height.unwrap_or(0.0);

    // Apply explicit row track sizes where specified (with MinMax clamping)
    for (ri, track) in row_tracks.iter().enumerate() {
        if ri < row_heights.len() {
            match track.kind {
                GridTrackKind::MinMax => {
                    let min_h = track_to_px(
                        &GridTrackSize {
                            kind: track.min_kind,
                            value: track.min_value,
                            ..Default::default()
                        },
                        row_pct_basis,
                        font_px,
                        root_font_px,
                    );
                    let max_h = track_to_px(
                        &GridTrackSize {
                            kind: track.max_kind,
                            value: track.max_value,
                            ..Default::default()
                        },
                        row_pct_basis,
                        font_px,
                        root_font_px,
                    );
                    let mut h = row_heights[ri].max(min_h);
                    if max_h > 0.0 {
                        h = h.min(max_h);
                    }
                    h = h.max(min_h);
                    row_heights[ri] = h;
                }
                // ⛔ `fit-content(X)` is a CEILING, never a floor — and on the
                // block axis it cannot cut into the content either, because the
                // track's MIN sizing function is `auto`. Its min-content height
                // is never below its max-content height, so the clamp has
                // nothing left to bite: the row is its content height.
                // `track_to_px` hands back X, and feeding that into the branch
                // below FORCED a 50px row to `fit-content(200px)`.
                GridTrackKind::FitContent => {}
                _ => {
                    let px = track_to_px(track, row_pct_basis, font_px, root_font_px);
                    if px > 0.0 {
                        // Fixed/percent tracks set exact height; fr/auto use content
                        match track.kind {
                            GridTrackKind::Fixed | GridTrackKind::Percent | GridTrackKind::Calc => {
                                row_heights[ri] = px;
                            }
                            _ => {
                                if px > row_heights[ri] {
                                    row_heights[ri] = px;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Apply grid-auto-rows to implicit rows (rows beyond the explicit row count)
    {
        let n_explicit_rows = node.style.rare().grid_template_rows.len();
        let ar = &node.style.grid_auto_rows;
        if ar.kind == GridTrackKind::MinMax {
            // minmax(min, max): enforce min as floor, max as ceiling
            let min_h = track_to_px(
                &GridTrackSize {
                    kind: ar.min_kind,
                    value: ar.min_value,
                    ..Default::default()
                },
                row_pct_basis,
                font_px,
                root_font_px,
            );
            let max_h = track_to_px(
                &GridTrackSize {
                    kind: ar.max_kind,
                    value: ar.max_value,
                    ..Default::default()
                },
                row_pct_basis,
                font_px,
                root_font_px,
            );
            for r in n_explicit_rows..n_rows {
                let mut h = row_heights[r].max(min_h);
                if max_h > 0.0 {
                    h = h.min(max_h);
                }
                row_heights[r] = h.max(min_h);
            }
        } else if ar.kind == GridTrackKind::FitContent {
            // `fit-content(X)` is a ceiling, not a floor — see the explicit
            // row tracks above. `track_to_px` returns X, which would have
            // forced every implicit row up to the limit.
        } else {
            let auto_h = track_to_px(ar, row_pct_basis, font_px, root_font_px);
            if auto_h > 0.0 {
                for r in n_explicit_rows..n_rows {
                    if auto_h > row_heights[r] {
                        row_heights[r] = auto_h;
                    }
                }
            }
        }
    }

    // Distribute fr units and percentages for row tracks when container has explicit height
    if let Some(container_h) = rbox.content_height {
        let total_gap = row_gap * n_rows.saturating_sub(1) as f32;
        // Resolve percentage rows
        for (ri, track) in row_tracks.iter().enumerate() {
            if ri < row_heights.len() && track.kind == GridTrackKind::Percent {
                row_heights[ri] = (track.value / 100.0 * container_h).max(0.0);
            }
        }
        // Distribute fractional rows
        let mut fr_total = 0.0f32;
        let mut fixed_total = 0.0f32;
        for (ri, track) in row_tracks.iter().enumerate() {
            if ri < row_heights.len() {
                if track.kind == GridTrackKind::Fractional {
                    fr_total += track.value;
                } else {
                    fixed_total += row_heights[ri];
                }
            }
        }
        if fr_total > 0.0 {
            let available = (container_h - fixed_total - total_gap).max(0.0);
            for (ri, track) in row_tracks.iter().enumerate() {
                if ri < row_heights.len() && track.kind == GridTrackKind::Fractional {
                    row_heights[ri] = (available * track.value / fr_total).max(0.0);
                }
            }
        }
    }

    // CSS Grid §11.1 steps 2-3: the grid container's size is found first, with
    // cyclic percentages treated as `auto`; the tracks are then sized again
    // with the percentages resolved against that size. The container keeps the
    // size step 2 gave it, so a percentage row may overflow it — that is what
    // Chrome does with `grid-template-rows: 50% auto` in an auto-height grid.
    let cyclic_pct_h = if rbox.content_height.is_none()
        && row_tracks
            .iter()
            .take(n_rows)
            .any(|t| t.kind == GridTrackKind::Percent)
    {
        Some(row_heights.iter().sum::<f32>() + row_gap * n_rows.saturating_sub(1) as f32)
    } else {
        None
    };
    if let Some(basis) = cyclic_pct_h {
        for (ri, track) in row_tracks.iter().enumerate() {
            if ri < row_heights.len() && track.kind == GridTrackKind::Percent {
                row_heights[ri] = (track.value / 100.0 * basis).max(0.0);
            }
        }
    }

    // ── §12.7 Stretch auto Tracks, block axis ────────────────────────────────
    // With `align-content: normal | stretch` the leftover definite free space
    // is divided EQUALLY among the rows whose MAX sizing function is `auto`.
    // `stretch` is the initial value, so this is what every grid does by
    // default: without it a `height:400px` grid with two auto rows left them at
    // content height and wasted the rest of the box.
    if node.style.align_content == AlignContent::Stretch {
        if let Some(container_h) = rbox.content_height {
            let n_explicit_rows = row_tracks.len();
            let auto_rows: Vec<usize> = (0..n_rows)
                .filter(|&r| {
                    let t = if r < n_explicit_rows {
                        &row_tracks[r]
                    } else {
                        &node.style.grid_auto_rows
                    };
                    track_max_is_auto(t)
                })
                .collect();
            if !auto_rows.is_empty() {
                let used: f32 = row_heights.iter().take(n_rows).sum::<f32>()
                    + row_gap * n_rows.saturating_sub(1) as f32;
                let leftover = container_h - used;
                if leftover > 0.01 {
                    let share = leftover / auto_rows.len() as f32;
                    for r in auto_rows {
                        row_heights[r] += share;
                    }
                }
            }
        }
    }

    // align-content: compute extra vertical space distribution
    let grid_total_h: f32 =
        row_heights.iter().sum::<f32>() + row_gap * n_rows.saturating_sub(1) as f32;
    let container_h = rbox.content_height.unwrap_or(grid_total_h);
    let extra_y = (container_h - grid_total_h).max(0.0);
    let (grid_offset_y, extra_gap_row) =
        compute_align_content(node.style.align_content, extra_y, n_rows);

    // Row Y positions (including align-content offset)
    let row_y: Vec<f32> = {
        let mut ys = Vec::with_capacity(n_rows);
        let mut cy = grid_offset_y;
        for i in 0..n_rows {
            ys.push(cy);
            cy += row_heights[i] + row_gap + extra_gap_row;
        }
        ys
    };

    // ── Second pass: position items ─────────────────────────────────────────

    let node_align_items = node.style.align_items;
    let node_justify_items = node.style.justify_items;
    for (ii, path) in item_indices.iter().enumerate() {
        let (cs, ce, rs, re) = placements[ii];
        let cs = cs.min(n_cols_actual.saturating_sub(1));
        let ce = ce.min(n_cols_actual).max(cs + 1);
        let rs = rs.min(n_rows.saturating_sub(1));
        let re = re.min(n_rows).max(rs + 1);

        let span_w = span_width(&col_px, &col_x, cs, ce, col_gap + extra_gap_col, content_w);
        let cell_h = row_heights[rs..re].iter().sum::<f32>()
            + (row_gap + extra_gap_row) * (re - rs).saturating_sub(1) as f32;

        // In RTL grids, col_x is reversed so col_x[cs] may be > col_x[ce-1].
        // Use the minimum x across the span as the left edge.
        let ix = if is_rtl && ce > cs {
            let min_x = col_x[cs..ce].iter().copied().fold(f32::MAX, f32::min);
            content_x + min_x
        } else {
            content_x + col_x.get(cs).copied().unwrap_or(0.0)
        };
        let iy = content_y + row_y.get(rs).copied().unwrap_or(0.0);

        let child = grid_child_mut(node, path);
        let child_font = child.style.font_size_px(font_px, root_font_px);
        let crbox = engine.res_box(&child.style, child_font, span_w, root_font_px);

        // Check for subgrid child
        let child_is_subgrid = (child.style.subgrid_columns || child.style.subgrid_rows)
            && matches!(child.style.display, Display::Grid | Display::InlineGrid);

        if child_is_subgrid {
            let sctx = SubgridContext {
                col_px: col_px.clone(),
                col_x: col_x.clone(),
                col_gap,
                row_heights: row_heights.clone(),
                row_y: row_y.clone(),
                row_gap,
                col_span: (cs, ce),
                row_span: (rs, re),
            };
            layout_grid_subgrid(engine, child, &crbox, &sctx, ix, iy, font_px, root_font_px);
            // Align subgrid box within its cell
            let target_x = ix + crbox.margin_left;
            let target_y = iy + crbox.margin_top;
            let ddx = target_x - child.layout.border_rect.x;
            let ddy = target_y - child.layout.border_rect.y;
            if ddx.abs() > 0.01 || ddy.abs() > 0.01 {
                shift_rects(child, ddx, ddy);
            }
            if matches!(child.style.position, Position::Relative | Position::Sticky) {
                apply_relative_offset(child, child_font, content_w, root_font_px);
            }
            continue;
        }

        // Handle justify-self / align-self
        let eff_justify = effective_justify_self(child, node_justify_items);
        let eff_align = effective_align_self_grid(child, node_align_items);

        // Stretch align-self: set explicit height and re-layout
        if eff_align == AlignItems::Stretch && child.style.height.is_auto() {
            // Compute the height value to assign to child.style.height.
            // resolve_box will later interpret this through box-sizing:
            //   border-box → subtracts padding+border from the assigned value
            //   content-box → uses the assigned value as-is
            // So for border-box we pass (cell_h - margins) and let resolve_box
            // subtract padding+border. For content-box we subtract them ourselves.
            let css_h = if child.style.box_sizing == BoxSizing::BorderBox {
                (cell_h - crbox.margin_top - crbox.margin_bottom).max(0.0)
            } else {
                (cell_h
                    - crbox.margin_top
                    - crbox.margin_bottom
                    - crbox.padding_top
                    - crbox.padding_bottom
                    - crbox.border_top
                    - crbox.border_bottom)
                    .max(0.0)
            };
            let saved_h = child.style.height.clone();
            std::sync::Arc::make_mut(&mut child.style).height = CssLength::Px(css_h);
            child.layout.layout_dirty = true; // force re-layout with new height
            engine.layout_box(
                child,
                &Constraints::new(span_w, ix, iy, font_px, root_font_px),
            );
            std::sync::Arc::make_mut(&mut child.style).height = saved_h;
        } else {
            engine.layout_box(
                child,
                &Constraints::new(span_w, ix, iy, font_px, root_font_px),
            );
        }

        // Re-read crbox after potential re-layout
        let crbox = engine.res_box(&child.style, child_font, span_w, root_font_px);
        let cell_w = span_w;

        let dx_align = match eff_justify {
            AlignItems::FlexStart => 0.0,
            AlignItems::FlexEnd => {
                cell_w - child.layout.border_rect.w - crbox.margin_left - crbox.margin_right
            }
            AlignItems::Center => {
                (cell_w - child.layout.border_rect.w - crbox.margin_left - crbox.margin_right) / 2.0
            }
            AlignItems::Stretch => 0.0,
            // The INLINE-axis baseline group is not modelled — there is one
            // baseline per box and it is a block-axis one — so an inline-axis
            // baseline value falls back to `start`, as the spec allows when a
            // box has no baseline in that axis.
            AlignItems::Baseline | AlignItems::LastBaseline => 0.0,
        };
        let dy_align = match eff_align {
            AlignItems::FlexStart => 0.0,
            AlignItems::FlexEnd => {
                cell_h - child.layout.border_rect.h - crbox.margin_top - crbox.margin_bottom
            }
            AlignItems::Center => {
                (cell_h - child.layout.border_rect.h - crbox.margin_top - crbox.margin_bottom) / 2.0
            }
            AlignItems::Stretch => 0.0,
            // Box Alignment §9: shift the item down by the row group's largest
            // ascent minus its own, so every member's baseline lands on the
            // same line. `last baseline` uses the same recorded baseline —
            // only one baseline per box is tracked.
            AlignItems::Baseline | AlignItems::LastBaseline => {
                match (
                    row_baseline.get(rs).copied().flatten(),
                    item_ascent.get(ii).copied().flatten(),
                ) {
                    (Some(group), Some(own)) => (group - own).max(0.0),
                    _ => 0.0,
                }
            }
        };

        // Target border_rect position
        let target_x = ix + crbox.margin_left + dx_align;
        let target_y = iy + crbox.margin_top + dy_align;

        let dx = target_x - child.layout.border_rect.x;
        let dy = target_y - child.layout.border_rect.y;
        shift_rects(child, dx, dy);

        // Apply relative offset
        if matches!(child.style.position, Position::Relative | Position::Sticky) {
            apply_relative_offset(child, child_font, content_w, root_font_px);
        }
    }

    let total_h = row_y.last().copied().unwrap_or(0.0) + row_heights.last().copied().unwrap_or(0.0);

    let ch = rbox
        .content_height
        .unwrap_or(cyclic_pct_h.unwrap_or(total_h));

    // Save restored heights
    for path in &item_indices {
        let child = grid_child_mut(node, path);
        child.layout.scroll_height = child.layout.content_rect.h;
    }

    // Collapsed margins: no pass-through
    node.layout.collapsed_margin_top = rbox.margin_top;
    node.layout.collapsed_margin_bottom = rbox.margin_bottom;
    node.layout.layout_dirty = false;

    let result = finish_grid(node, rbox, content_x, content_y, content_w, ch);
    layout_abs_children(engine, node, font_px, root_font_px);
    result
}

// ─── Placement helpers ────────────────────────────────────────────────────────

fn build_area_map(
    areas: &[Vec<String>],
) -> std::collections::HashMap<String, (usize, usize, usize, usize)> {
    let mut map = std::collections::HashMap::new();
    for (r, row) in areas.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            if name == "." {
                continue;
            }
            let e = map.entry(name.clone()).or_insert((c, c + 1, r, r + 1));
            if c < e.0 {
                e.0 = c;
            }
            if c + 1 > e.1 {
                e.1 = c + 1;
            }
            if r < e.2 {
                e.2 = r;
            }
            if r + 1 > e.3 {
                e.3 = r + 1;
            }
        }
    }
    map
}

/// Decode a grid line value from parse_grid_line encoding.
/// Returns (is_span, value) where:
///   is_span=true, value=N  means "span N"
///   is_span=false, value=N means explicit line N (1-based positive, or negative from end)
///   value=0 means auto
fn decode_grid_line(raw: i32) -> (bool, i32) {
    if raw <= -10000 {
        // Span encoding: -(span_count + 10000)
        (true, -(raw + 10000))
    } else {
        (false, raw)
    }
}

/// Look up a named line in the line-name map. Returns the first matching line index.
fn lookup_named_line(
    name: &str,
    line_names: &std::collections::HashMap<String, Vec<usize>>,
) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    if let Some(indices) = line_names.get(name) {
        return indices.first().copied();
    }
    None
}

/// Resolve a grid line start value to a 0-based column/row index.
fn resolve_line_start(raw: i32, n_explicit: usize) -> usize {
    let (is_span, val) = decode_grid_line(raw);
    if is_span || val == 0 {
        return 0;
    } // span or auto → 0 (auto-placed later)
    if val > 0 {
        (val as usize).saturating_sub(1)
    } else {
        let from_end = (-val) as usize;
        (n_explicit + 1).saturating_sub(from_end)
    }
}

/// Resolve a grid line start, with named line fallback.
fn resolve_line_start_named(
    raw: i32,
    name: &str,
    n_explicit: usize,
    line_names: &std::collections::HashMap<String, Vec<usize>>,
) -> usize {
    // Try named line first if numeric is auto
    if raw == 0 && !name.is_empty() {
        if let Some(idx) = lookup_named_line(name, line_names) {
            return idx;
        }
    }
    resolve_line_start(raw, n_explicit)
}

/// Resolve a grid line end value to a 0-based column/row index.
fn resolve_line_end(raw: i32, start: usize, n_explicit: usize) -> usize {
    let (is_span, val) = decode_grid_line(raw);
    if is_span {
        start + val as usize
    } else if val == 0 {
        start + 1 // auto
    } else if val > 0 {
        (val as usize).saturating_sub(1).max(start + 1)
    } else {
        let from_end = (-val) as usize;
        let line = (n_explicit + 1).saturating_sub(from_end);
        line.max(start + 1)
    }
}

/// Resolve a grid line end, with named line fallback.
fn resolve_line_end_named(
    raw: i32,
    name: &str,
    start: usize,
    n_explicit: usize,
    line_names: &std::collections::HashMap<String, Vec<usize>>,
) -> usize {
    // Try named line first if numeric is auto
    if raw == 0 && !name.is_empty() {
        if let Some(idx) = lookup_named_line(name, line_names) {
            return idx.max(start + 1);
        }
    }
    resolve_line_end(raw, start, n_explicit)
}

fn is_explicitly_placed(
    child: &WebCore,
    area_map: &std::collections::HashMap<String, (usize, usize, usize, usize)>,
    col_line_names: &std::collections::HashMap<String, Vec<usize>>,
    row_line_names: &std::collections::HashMap<String, Vec<usize>>,
) -> bool {
    if !child.style.grid_area.is_empty() && area_map.contains_key(&child.style.grid_area) {
        return true;
    }
    let (cs_span, cs_val) = decode_grid_line(child.style.grid_column_start);
    let (rs_span, rs_val) = decode_grid_line(child.style.grid_row_start);
    // Column is definite if it has a numeric value OR a resolvable named line
    let col_definite = (!cs_span && cs_val != 0)
        || (!child.style.grid_column_start_name.is_empty()
            && lookup_named_line(&child.style.grid_column_start_name, col_line_names).is_some());
    let row_definite = (!rs_span && rs_val != 0)
        || (!child.style.grid_row_start_name.is_empty()
            && lookup_named_line(&child.style.grid_row_start_name, row_line_names).is_some());
    col_definite && row_definite
}

/// Resolve placement to (col_start, col_end, row_start, row_end), all 0-based.
fn resolve_placement(
    child: &WebCore,
    area_map: &std::collections::HashMap<String, (usize, usize, usize, usize)>,
    n_cols: usize,
    n_rows: usize,
    col_line_names: &std::collections::HashMap<String, Vec<usize>>,
    row_line_names: &std::collections::HashMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
    // Named grid area
    if !child.style.grid_area.is_empty() {
        if let Some(&(cs, ce, rs, re)) = area_map.get(&child.style.grid_area) {
            return (cs, ce, rs, re);
        }
    }

    let cs = resolve_line_start_named(
        child.style.grid_column_start,
        &child.style.grid_column_start_name,
        n_cols,
        col_line_names,
    );
    let ce = resolve_line_end_named(
        child.style.grid_column_end,
        &child.style.grid_column_end_name,
        cs,
        n_cols,
        col_line_names,
    );
    let rs = resolve_line_start_named(
        child.style.grid_row_start,
        &child.style.grid_row_start_name,
        n_rows,
        row_line_names,
    );
    let re = resolve_line_end_named(
        child.style.grid_row_end,
        &child.style.grid_row_end_name,
        rs,
        n_rows,
        row_line_names,
    );

    (cs, ce, rs, re)
}

/// Maximum grid span/row/col to prevent pathological allocation.
const MAX_GRID_SPAN: usize = 200;

fn get_span_col(child: &WebCore) -> usize {
    let (end_is_span, end_val) = decode_grid_line(child.style.grid_column_end);
    if end_is_span {
        return end_val as usize;
    }
    let (start_is_span, start_val) = decode_grid_line(child.style.grid_column_start);
    if start_is_span {
        return start_val as usize;
    }
    if child.style.grid_column_end > 0 && child.style.grid_column_start > 0 {
        (child.style.grid_column_end - child.style.grid_column_start).max(1) as usize
    } else {
        1
    }
}

fn get_span_row(child: &WebCore) -> usize {
    let (end_is_span, end_val) = decode_grid_line(child.style.grid_row_end);
    if end_is_span {
        return end_val as usize;
    }
    let (start_is_span, start_val) = decode_grid_line(child.style.grid_row_start);
    if start_is_span {
        return start_val as usize;
    }
    if child.style.grid_row_end > 0 && child.style.grid_row_start > 0 {
        (child.style.grid_row_end - child.style.grid_row_start).max(1) as usize
    } else {
        1
    }
}

fn re_from(p: &(usize, usize, usize, usize)) -> usize {
    p.3
}

fn n_rows_from_template(row_tracks: &[GridTrackSize], n_items: usize) -> usize {
    if !row_tracks.is_empty() {
        row_tracks.len()
    } else {
        n_items.max(1)
    }
}

fn ensure_row(occ: &mut Vec<Vec<bool>>, row: usize, n_cols: usize) {
    let row = row.min(MAX_GRID_SPAN);
    let n_cols = n_cols.min(MAX_GRID_SPAN);
    while occ.len() <= row {
        occ.push(vec![false; n_cols.max(1)]);
    }
    // Extend existing rows if n_cols grew
    for r in occ.iter_mut() {
        while r.len() < n_cols.max(1) {
            r.push(false);
        }
    }
}

// ─── Track resolution ────────────────────────────────────────────────────────

/// Resolve auto-repeat and other tracks to a final list.
fn resolve_track_sizes(
    tracks: &[GridTrackSize],
    auto_repeat: &[GridTrackSize],
    container: f32,
    font_px: f32,
    root_font_px: f32,
) -> Vec<GridTrackSize> {
    resolve_track_sizes_with_gap(tracks, auto_repeat, container, font_px, root_font_px, 0.0)
}

fn resolve_track_sizes_with_gap(
    tracks: &[GridTrackSize],
    auto_repeat: &[GridTrackSize],
    container: f32,
    font_px: f32,
    root_font_px: f32,
    gap: f32,
) -> Vec<GridTrackSize> {
    let mut result = Vec::new();
    for t in tracks {
        if t.kind == GridTrackKind::Auto && t.value == -1.0 {
            // Auto-fill/fit placeholder — expand using auto_repeat_columns
            if !auto_repeat.is_empty() {
                // For auto-fill/fit, use the minimum track size to determine count.
                // minmax(200px, 1fr) → min is 200px, that determines how many fit.
                let mut total_min = 0.0f32;
                for rt in auto_repeat {
                    let px = match rt.kind {
                        GridTrackKind::MinMax => {
                            let min_t = GridTrackSize {
                                kind: rt.min_kind,
                                value: rt.min_value,
                                ..Default::default()
                            };
                            let min_px = track_to_px(&min_t, container, font_px, root_font_px);
                            if min_px > 0.0 {
                                min_px
                            } else {
                                track_to_px(rt, container, font_px, root_font_px).max(50.0)
                            }
                        }
                        _ => {
                            let px = track_to_px(rt, container, font_px, root_font_px);
                            if px > 0.0 {
                                px
                            } else {
                                50.0
                            }
                        }
                    };
                    total_min += px;
                }
                let pattern_w = total_min.max(1.0);
                // Repeat count: N patterns need N-1 gaps.
                // N * pattern_w + (N-1) * gap <= container
                // N <= (container + gap) / (pattern_w + gap)
                let count = ((container + gap) / (pattern_w + gap))
                    .floor()
                    .max(1.0)
                    .min(100.0) as usize;
                for _ in 0..count {
                    for rt in auto_repeat {
                        result.push(rt.clone());
                    }
                }
            }
        } else {
            result.push(t.clone());
        }
    }
    if result.is_empty() && !auto_repeat.is_empty() {
        // Only auto-repeat — use minimum track sizes
        let mut total_min = 0.0f32;
        for rt in auto_repeat {
            let px = match rt.kind {
                GridTrackKind::MinMax => {
                    let min_t = GridTrackSize {
                        kind: rt.min_kind,
                        value: rt.min_value,
                        ..Default::default()
                    };
                    let min_px = track_to_px(&min_t, container, font_px, root_font_px);
                    if min_px > 0.0 {
                        min_px
                    } else {
                        track_to_px(rt, container, font_px, root_font_px).max(50.0)
                    }
                }
                _ => {
                    let px = track_to_px(rt, container, font_px, root_font_px);
                    if px > 0.0 {
                        px
                    } else {
                        50.0
                    }
                }
            };
            total_min += px;
        }
        let pattern_w = total_min.max(1.0);
        let count = ((container + gap) / (pattern_w + gap))
            .floor()
            .max(1.0)
            .min(100.0) as usize;
        for _ in 0..count {
            for rt in auto_repeat {
                result.push(rt.clone());
            }
        }
    }
    result
}

/// Convert a single track to approximate pixel value for auto-fill estimation.
pub fn track_to_px(track: &GridTrackSize, container: f32, font_px: f32, root_font_px: f32) -> f32 {
    match track.kind {
        GridTrackKind::Fixed => track.value,
        GridTrackKind::Percent => track.value / 100.0 * container,
        GridTrackKind::Auto | GridTrackKind::MinContent | GridTrackKind::MaxContent => 0.0,
        GridTrackKind::Fractional => 0.0, // resolved below
        GridTrackKind::MinMax => {
            // Use max value
            let max_t = GridTrackSize {
                kind: track.max_kind,
                value: track.max_value,
                ..Default::default()
            };
            track_to_px(&max_t, container, font_px, root_font_px)
        }
        GridTrackKind::FitContent => {
            // fit-content(X) = min(max-content, max(min-content, X))
            // For track_to_px we return the clamp limit (X resolved as length/percentage)
            if track.max_kind == GridTrackKind::Percent {
                track.max_value / 100.0 * container
            } else {
                track.value
            }
        }
        GridTrackKind::Subgrid => 0.0,
        GridTrackKind::Calc => {
            if let Some(ref len) = track.calc_length {
                len.resolve(font_px, container, root_font_px)
            } else {
                0.0
            }
        }
    }
}

fn span_only_flexible_tracks(tracks: &[GridTrackSize], start: usize, end: usize) -> bool {
    if start >= end || end > tracks.len() {
        return false;
    }
    tracks[start..end].iter().all(|track| {
        track.kind == GridTrackKind::Fractional
            || (track.kind == GridTrackKind::MinMax && track.max_kind == GridTrackKind::Fractional)
    })
}

/// Resolve a track list to pixel sizes, running the CSS Grid track sizing
/// algorithm in the order the spec gives it: initialize base sizes and growth
/// limits (§12.4/§12.5), Maximize Tracks (§12.6), Expand Flexible Tracks
/// (§12.8), Stretch auto Tracks (§12.7).
///
/// `content_widths` is the MAX-content contribution per track, `min_widths` the
/// MIN-content one. A track needs both: they are the two ends of its base size
/// and growth limit, and using max-content for both makes `min-content` a
/// synonym for `max-content`.
fn resolve_to_pixels(
    tracks: &[GridTrackSize],
    auto_cols: &GridTrackSize,
    container: f32,
    gap: f32,
    n_cols: usize,
    font_px: f32,
    root_font_px: f32,
    content_widths: &[f32],
    min_widths: &[f32],
) -> Vec<f32> {
    let effective_n = tracks.len().max(n_cols);
    let total_gap = gap * effective_n.saturating_sub(1) as f32;

    // Per-track sizing state.
    let mut base = vec![0.0f32; effective_n];
    let mut limit = vec![0.0f32; effective_n];
    let mut is_fr = vec![false; effective_n];
    let mut fr_value = vec![0.0f32; effective_n];
    // A track only takes part in §12.7 "Stretch auto Tracks" when its MAX
    // sizing function is `auto` — a `min-content` or `max-content` track
    // freezes at its content size and leaves the leftover space unused.
    let mut max_is_auto = vec![false; effective_n];
    // Tracks Maximize (§12.6) may grow: everything whose growth limit exceeds
    // its base and that is not flexible (a flexible track's growth limit IS
    // its base size, so it is frozen from the start).
    let mut growable = vec![false; effective_n];

    let pct = |v: f32| v / 100.0 * container;

    for i in 0..effective_n {
        let mn = min_widths.get(i).copied().unwrap_or(0.0);
        let mx = content_widths.get(i).copied().unwrap_or(0.0).max(mn);
        let track = if i < tracks.len() {
            Some(&tracks[i])
        } else if auto_cols.kind != GridTrackKind::Auto {
            Some(auto_cols)
        } else {
            None
        };
        let t = match track {
            Some(t) => t,
            // Implicit track with `grid-auto-columns: auto`.
            None => {
                base[i] = mn;
                limit[i] = mx;
                max_is_auto[i] = true;
                growable[i] = true;
                continue;
            }
        };
        match t.kind {
            GridTrackKind::Fixed => {
                base[i] = t.value;
                limit[i] = t.value;
            }
            GridTrackKind::Percent => {
                base[i] = pct(t.value);
                limit[i] = base[i];
            }
            GridTrackKind::Calc => {
                let px = t
                    .calc_length
                    .as_ref()
                    .map(|l| l.resolve(font_px, container, root_font_px))
                    .unwrap_or(0.0);
                base[i] = px;
                limit[i] = px;
            }
            // `<flex>` is shorthand for `minmax(auto, <flex>)`: the base comes
            // from the content, the growth limit equals the base.
            GridTrackKind::Fractional => {
                is_fr[i] = true;
                fr_value[i] = t.value;
                base[i] = mn;
                limit[i] = mn;
            }
            GridTrackKind::Auto | GridTrackKind::Subgrid => {
                base[i] = mn;
                limit[i] = mx;
                max_is_auto[i] = true;
                growable[i] = true;
            }
            GridTrackKind::MinContent => {
                base[i] = mn;
                limit[i] = mn;
            }
            GridTrackKind::MaxContent => {
                base[i] = mx;
                limit[i] = mx;
            }
            GridTrackKind::FitContent => {
                // fit-content(x) = minmax(auto, max-content) clamped by x. Its
                // max sizing function is not `auto`, so §12.7 skips it.
                let clamp = if t.max_kind == GridTrackKind::Percent {
                    pct(t.max_value)
                } else {
                    t.value
                };
                base[i] = mn;
                limit[i] = mx.min(clamp.max(mn));
                growable[i] = true;
            }
            GridTrackKind::MinMax => {
                base[i] = match t.min_kind {
                    GridTrackKind::Fixed => t.min_value,
                    GridTrackKind::Percent => pct(t.min_value),
                    GridTrackKind::MaxContent => mx,
                    _ => mn, // auto / min-content
                };
                if t.max_kind == GridTrackKind::Fractional {
                    is_fr[i] = true;
                    fr_value[i] = t.max_value;
                    limit[i] = base[i];
                } else {
                    limit[i] = match t.max_kind {
                        GridTrackKind::Fixed => t.max_value,
                        GridTrackKind::Percent => pct(t.max_value),
                        GridTrackKind::MinContent => mn,
                        _ => mx, // auto / max-content
                    };
                    if t.max_kind == GridTrackKind::Auto {
                        max_is_auto[i] = true;
                    }
                    growable[i] = true;
                }
            }
        }
        if limit[i] < base[i] {
            limit[i] = base[i];
        }
        if base[i] < 0.0 {
            base[i] = 0.0;
        }
    }

    // ── §12.6 Maximize Tracks ────────────────────────────────────────────────
    // Distribute the free space equally to the base sizes, FREEZING each track
    // as it reaches its growth limit and continuing to grow the rest. One flat
    // share per track stretched a small `auto` column past its max-content
    // while a large sibling still needed room.
    let mut free = container - total_gap - base.iter().sum::<f32>();
    let mut open: Vec<usize> = (0..effective_n)
        .filter(|&i| growable[i] && !is_fr[i] && limit[i] > base[i])
        .collect();
    while free > 0.01 && !open.is_empty() {
        let share = free / open.len() as f32;
        let mut still_open = Vec::with_capacity(open.len());
        let mut used = 0.0f32;
        for &i in &open {
            let room = limit[i] - base[i];
            if room <= share {
                base[i] = limit[i];
                used += room;
            } else {
                base[i] += share;
                used += share;
                still_open.push(i);
            }
        }
        free -= used;
        if still_open.len() == open.len() {
            break;
        } // everyone took a full share
        open = still_open;
    }

    // ── §12.8 Expand Flexible Tracks ─────────────────────────────────────────
    // ⛔ The leftover space for `fr` excludes the base sizes of the FLEXIBLE
    // tracks themselves. Counting a `minmax(50px, 1fr)` base against it charged
    // that track twice — once as free space and again as an fr share — so
    // `minmax(50px,1fr) 1fr` in a 300px grid gave 125 where Chrome gives 150,
    // and `repeat(auto-fill, minmax(80px,1fr))` never grew past 80.
    let total_fr: f32 = (0..effective_n)
        .filter(|&i| is_fr[i])
        .map(|i| fr_value[i])
        .sum();
    if total_fr > 0.0 {
        let non_flex: f32 = (0..effective_n)
            .filter(|&i| !is_fr[i])
            .map(|i| base[i])
            .sum();
        let leftover = (container - total_gap - non_flex).max(0.0);
        for i in 0..effective_n {
            if is_fr[i] {
                let fr_w = leftover * fr_value[i] / total_fr;
                if fr_w > base[i] {
                    base[i] = fr_w;
                }
            }
        }
    }

    // ── §12.7 Stretch auto Tracks ────────────────────────────────────────────
    // The remaining definite free space is divided equally among the tracks
    // whose MAX sizing function is `auto`. `justify-content: normal` is the
    // condition the spec names; the JustifyContent enum has no `normal`, so the
    // inline axis always stretches, which is right for the initial value.
    let stretch: Vec<usize> = (0..effective_n).filter(|&i| max_is_auto[i]).collect();
    if !stretch.is_empty() {
        let leftover = container - total_gap - base.iter().sum::<f32>();
        if leftover > 0.01 {
            let share = leftover / stretch.len() as f32;
            for i in stretch {
                base[i] += share;
            }
        }
    }

    for v in base.iter_mut() {
        if *v < 0.0 {
            *v = 0.0;
        }
    }
    base
}

fn span_width(
    col_px: &[f32],
    _col_x: &[f32],
    cs: usize,
    ce: usize,
    col_gap: f32,
    fallback: f32,
) -> f32 {
    if col_px.is_empty() {
        return fallback;
    }
    let cs = cs.min(col_px.len().saturating_sub(1));
    let ce = ce.min(col_px.len());
    if cs >= ce {
        return col_px.get(cs).copied().unwrap_or(fallback);
    }
    let w: f32 = col_px[cs..ce].iter().sum::<f32>() + col_gap * (ce - cs).saturating_sub(1) as f32;
    w
}

// ─── Alignment helpers ────────────────────────────────────────────────────────

/// A track takes part in §12.7 "Stretch auto Tracks" only when its MAX sizing
/// function is `auto`. A `min-content` / `max-content` / fixed / `fr` track
/// freezes at the size it was given and leaves the leftover space unused.
fn track_max_is_auto(t: &GridTrackSize) -> bool {
    match t.kind {
        GridTrackKind::Auto => true,
        GridTrackKind::MinMax => t.max_kind == GridTrackKind::Auto,
        _ => false,
    }
}

fn effective_justify_self(child: &WebCore, parent: AlignItems) -> AlignItems {
    match child.style.justify_self {
        AlignSelf::Auto => parent,
        AlignSelf::Stretch => AlignItems::Stretch,
        AlignSelf::FlexStart => AlignItems::FlexStart,
        AlignSelf::FlexEnd => AlignItems::FlexEnd,
        AlignSelf::Center => AlignItems::Center,
        AlignSelf::Baseline => AlignItems::Baseline,
        AlignSelf::LastBaseline => AlignItems::LastBaseline,
    }
}

fn effective_align_self_grid(child: &WebCore, parent: AlignItems) -> AlignItems {
    match child.style.align_self {
        AlignSelf::Auto => parent,
        AlignSelf::Stretch => AlignItems::Stretch,
        AlignSelf::FlexStart => AlignItems::FlexStart,
        AlignSelf::FlexEnd => AlignItems::FlexEnd,
        AlignSelf::Center => AlignItems::Center,
        AlignSelf::Baseline => AlignItems::Baseline,
        AlignSelf::LastBaseline => AlignItems::LastBaseline,
    }
}

// ─── finish & abs children ───────────────────────────────────────────────────

fn finish_grid(
    node: &mut WebCore,
    rbox: &ResolvedBox,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
) -> f32 {
    let ch = rbox.content_height.unwrap_or(content_h);
    node.layout.content_rect = Rect::new(content_x, content_y, content_w, ch);
    node.layout.padding_rect = Rect::new(
        content_x - rbox.padding_left,
        content_y - rbox.padding_top,
        content_w + rbox.padding_left + rbox.padding_right,
        ch + rbox.padding_top + rbox.padding_bottom,
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
    node.layout.baseline = node.layout.content_rect.y + ch;

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
    crate::layout::update_scroll_extents_from_children(node, content_x, content_y, content_w, ch);

    node.layout.margin_rect.h
}

fn layout_abs_children(engine: &LayoutEngine, node: &mut WebCore, font_px: f32, root_font_px: f32) {
    let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style) {
        node.layout.padding_rect
    } else {
        engine.pos_cb.get()
    };
    let indices: Vec<usize> = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c.style.position, Position::Absolute | Position::Fixed))
        .map(|(i, _)| i)
        .collect();
    for i in indices {
        layout_positioned(
            engine,
            &mut node.children[i],
            containing_rect,
            font_px,
            root_font_px,
        );
    }
}

// ─── justify-content / align-content distribution ────────────────────────────

/// Returns (offset, extra_gap) for justify-content.
/// Mirrors C++ grid justify-content calculation.
fn compute_justify_content(jc: JustifyContent, extra: f32, n: usize) -> (f32, f32) {
    if n == 0 || extra <= 0.0 {
        return (0.0, 0.0);
    }
    match jc {
        JustifyContent::Center => (extra / 2.0, 0.0),
        JustifyContent::FlexEnd | JustifyContent::Right => (extra, 0.0),
        JustifyContent::SpaceBetween => {
            if n > 1 {
                (0.0, extra / (n - 1) as f32)
            } else {
                (0.0, 0.0)
            }
        }
        JustifyContent::SpaceAround => {
            let g = extra / n as f32;
            (g / 2.0, g)
        }
        JustifyContent::SpaceEvenly => {
            let g = extra / (n + 1) as f32;
            (g, g)
        }
        JustifyContent::FlexStart | JustifyContent::Left => (0.0, 0.0),
    }
}

/// Returns (offset, extra_gap) for align-content.
/// Mirrors C++ grid align-content calculation.
fn compute_align_content(ac: AlignContent, extra: f32, n: usize) -> (f32, f32) {
    if n == 0 || extra <= 0.0 {
        return (0.0, 0.0);
    }
    match ac {
        AlignContent::Center => (extra / 2.0, 0.0),
        AlignContent::FlexEnd => (extra, 0.0),
        AlignContent::SpaceBetween => {
            if n > 1 {
                (0.0, extra / (n - 1) as f32)
            } else {
                (0.0, 0.0)
            }
        }
        AlignContent::SpaceAround => {
            let g = extra / n as f32;
            (g / 2.0, g)
        }
        AlignContent::SpaceEvenly => {
            let g = extra / (n + 1) as f32;
            (g, g)
        }
        AlignContent::FlexStart | AlignContent::Stretch => (0.0, 0.0),
    }
}

// ─── Old parse_track_sizes kept for backward compat ──────────────────────────

/// Parse grid-template-columns / grid-template-rows into pixel sizes (legacy string API).
/// Used by table.rs and any remaining callers.
pub fn parse_track_sizes(
    template: &str,
    container: f32,
    font_px: f32,
    root_font_px: f32,
) -> Vec<f32> {
    if template.is_empty() {
        return Vec::new();
    }

    let expanded = expand_repeat(template, container);
    let mut sizes = Vec::new();
    let mut fr_indices: Vec<usize> = Vec::new();
    let mut fr_values: Vec<f32> = Vec::new();
    let mut used = 0.0f32;

    for token in expanded.split_whitespace() {
        let token = token.trim_matches(|c: char| c == '(' || c == ')');
        if token.ends_with("fr") {
            let fr: f32 = token[..token.len() - 2].parse().unwrap_or(1.0);
            fr_indices.push(sizes.len());
            fr_values.push(fr);
            sizes.push(0.0);
        } else if token.ends_with("px") {
            let px: f32 = token[..token.len() - 2].parse().unwrap_or(0.0);
            used += px;
            sizes.push(px);
        } else if token.ends_with('%') {
            let pct: f32 = token[..token.len() - 1].parse().unwrap_or(0.0);
            let px = pct / 100.0 * container;
            used += px;
            sizes.push(px);
        } else if token.starts_with("minmax") {
            let inner = token
                .trim_start_matches("minmax")
                .trim_matches(|c: char| c == '(' || c == ')');
            let parts: Vec<&str> = inner.splitn(2, ',').collect();
            if let Some(max_part) = parts.get(1) {
                let p = max_part.trim();
                if p.ends_with("fr") {
                    let fr: f32 = p[..p.len() - 2].parse().unwrap_or(1.0);
                    fr_indices.push(sizes.len());
                    fr_values.push(fr);
                    sizes.push(0.0);
                } else {
                    let px = crate::css::parse_length(p).resolve(font_px, container, root_font_px);
                    used += px;
                    sizes.push(px);
                }
            }
        } else if token == "auto" {
            sizes.push(0.0);
        }
    }

    if !fr_indices.is_empty() {
        let remaining = (container - used).max(0.0);
        let total_fr: f32 = fr_values.iter().sum();
        for (ii, &idx) in fr_indices.iter().enumerate() {
            sizes[idx] = remaining * fr_values[ii] / total_fr;
        }
    }

    sizes
}

fn expand_repeat(template: &str, container: f32) -> String {
    if !template.contains("repeat") {
        return template.to_string();
    }

    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("repeat(") {
        out.push_str(&rest[..start]);
        let inner_start = start + 7;
        let mut depth = 1;
        let mut end = inner_start;
        let bytes = rest.as_bytes();
        while end < bytes.len() {
            if bytes[end] == b'(' {
                depth += 1;
            }
            if bytes[end] == b')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            end += 1;
        }
        let inner = &rest[inner_start..end];
        let comma_pos = inner.find(',').unwrap_or(0);
        let count_str = inner[..comma_pos].trim();
        let track_str = inner[comma_pos + 1..].trim();

        let count = if count_str == "auto-fill" || count_str == "auto-fit" {
            let ts = crate::css::parse_length(track_str)
                .resolve(16.0, container, 16.0)
                .max(1.0);
            (container / ts) as usize
        } else if let Ok(n) = count_str.parse::<usize>() {
            n
        } else {
            // Handle calc() in repeat count, e.g. repeat(calc(5 - 1), ...)
            let resolved = crate::css::parse_length(count_str).resolve(16.0, container, 16.0);
            if resolved > 0.0 {
                resolved as usize
            } else {
                1
            }
        };

        for i in 0..count {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(track_str);
        }

        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}
