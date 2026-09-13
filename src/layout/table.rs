use super::Constraints;
use crate::layout::block::apply_relative_offset;
use crate::layout::{LayoutEngine, ResolvedBox, layout_positioned, shift_rects};
use crate::types::*;

// ─── Border conflict resolution for border-collapse ───────────────────────────
// CSS 2.1 §17.6.2.1: hidden > wider > style-priority > first cell wins.
// Mirrors C++ BorderStylePriority / BorderWins / ResolveCollapsedBorders.

fn border_style_priority(s: BorderStyle) -> i32 {
    match s {
        BorderStyle::Double => 4,
        BorderStyle::Groove => 4,
        BorderStyle::Ridge => 4,
        BorderStyle::Solid => 3,
        BorderStyle::Inset => 3,
        BorderStyle::Outset => 3,
        BorderStyle::Dashed => 2,
        BorderStyle::Dotted => 1,
        BorderStyle::Hidden => 5,
        BorderStyle::None => 0,
    }
}

/// Returns true if side `a` wins over side `b` in border-collapse conflict resolution.
fn border_wins(a_width: f32, a_style: BorderStyle, b_width: f32, b_style: BorderStyle) -> bool {
    if a_style == BorderStyle::None && b_style == BorderStyle::None {
        return true;
    }
    if a_style == BorderStyle::None {
        return false;
    }
    if b_style == BorderStyle::None {
        return true;
    }
    if a_width != b_width {
        return a_width > b_width;
    }
    let pa = border_style_priority(a_style);
    let pb = border_style_priority(b_style);
    if pa != pb {
        return pa > pb;
    }
    true // tie: first cell wins
}

// ─── TableCellSlot: 2D grid for rowspan/colspan tracking ─────────────────────

#[derive(Default, Clone)]
struct TableCellSlot {
    /// Index into the owning row's children.
    box_path: Option<(usize /*row*/, usize /*cell_in_row*/)>,
    owner_row: usize,
    colspan: usize,
    rowspan: usize,
}

// ─── Helper: read colspan/rowspan from HTML attributes ────────────────────────

fn get_colspan(cell: &WebCore) -> usize {
    cell.attributes
        .get("colspan")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

fn get_rowspan(cell: &WebCore) -> usize {
    cell.attributes
        .get("rowspan")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

// ─── Collect rows: thead first, then tbody/bare, then tfoot ─────────────────
// Returns (row_box_path, is_group_child) where row_box_path is (child_idx, grandchild_idx_or_none)

#[derive(Clone)]
struct RowRef {
    child_idx: usize,              // index into table.children
    grandchild_idx: Option<usize>, // None = direct row, Some(j) = row inside row group
}

fn collect_rows(
    table: &WebCore,
    abs_children: &mut Vec<usize>,
    caption_idx: &mut Option<usize>,
    col_indices: &mut Vec<usize>,
) -> Vec<RowRef> {
    let mut thead: Vec<RowRef> = Vec::new();
    let mut tbody: Vec<RowRef> = Vec::new();
    let mut tfoot: Vec<RowRef> = Vec::new();

    for (i, child) in table.children.iter().enumerate() {
        if matches!(child.style.display, Display::None) {
            continue;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            abs_children.push(i);
            continue;
        }
        match child.style.display {
            Display::TableRow => {
                tbody.push(RowRef {
                    child_idx: i,
                    grandchild_idx: None,
                });
            }
            Display::TableCaption => {
                if caption_idx.is_none() {
                    *caption_idx = Some(i);
                }
            }
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                let target = if child.tag == "thead"
                    || matches!(child.style.display, Display::TableHeaderGroup)
                {
                    &mut thead
                } else if child.tag == "tfoot"
                    || matches!(child.style.display, Display::TableFooterGroup)
                {
                    &mut tfoot
                } else {
                    &mut tbody
                };
                for (j, gc) in child.children.iter().enumerate() {
                    if matches!(gc.style.display, Display::TableRow) {
                        target.push(RowRef {
                            child_idx: i,
                            grandchild_idx: Some(j),
                        });
                    }
                }
            }
            Display::Contents => {
                // Transparent wrapper (e.g. <form> inside <table>): look through it
                for (j, gc) in child.children.iter().enumerate() {
                    if matches!(gc.style.display, Display::None) {
                        continue;
                    }
                    match gc.style.display {
                        Display::TableRow => {
                            tbody.push(RowRef {
                                child_idx: i,
                                grandchild_idx: Some(j),
                            });
                        }
                        Display::TableHeaderGroup => {
                            for (_k, ggc) in gc.children.iter().enumerate() {
                                if matches!(ggc.style.display, Display::TableRow) {
                                    thead.push(RowRef {
                                        child_idx: i,
                                        grandchild_idx: Some(j),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Display::TableColumn => {
                col_indices.push(i);
            }
            Display::TableColumnGroup => {
                // Collect col children, or treat group itself as implicit col
                let mut has_cols = false;
                for (_, gc) in child.children.iter().enumerate() {
                    if matches!(gc.style.display, Display::TableColumn) {
                        col_indices.push(i);
                        has_cols = true;
                    }
                }
                if !has_cols {
                    col_indices.push(i);
                }
            }
            _ => {}
        }
    }
    // Visual order: thead → tbody → tfoot
    let mut all = Vec::new();
    all.extend(thead);
    all.extend(tbody);
    all.extend(tfoot);
    all
}

fn wrap_direct_table_cells_in_anonymous_rows(table: &mut WebCore) {
    if !table.children.iter().any(|c| {
        matches!(
            c.style.display,
            Display::TableCell | Display::TableHeaderCell
        )
    }) {
        return;
    }

    let children = std::mem::take(&mut table.children);
    let mut out: Vec<WebCore> = Vec::with_capacity(children.len());
    let mut row: Option<WebCore> = None;
    for child in children {
        if matches!(
            child.style.display,
            Display::TableCell | Display::TableHeaderCell
        ) {
            let r = row.get_or_insert_with(|| {
                let mut n = WebCore::new("anonymous-table-row");
                std::sync::Arc::make_mut(&mut n.style).display = Display::TableRow;
                n
            });
            r.children.push(child);
        } else {
            if let Some(r) = row.take() {
                out.push(r);
            }
            out.push(child);
        }
    }
    if let Some(r) = row {
        out.push(r);
    }
    table.children = out;
}

fn distribute_spanned_width(
    widths: &mut [f32],
    start_col: usize,
    colspan: usize,
    required_width: f32,
    spacing_h: f32,
) {
    if start_col >= widths.len() || colspan == 0 {
        return;
    }
    let end = (start_col + colspan).min(widths.len());
    let span = end - start_col;
    if span == 0 {
        return;
    }

    let current: f32 =
        widths[start_col..end].iter().sum::<f32>() + spacing_h * span.saturating_sub(1) as f32;
    if required_width <= current {
        return;
    }

    let add = (required_width - current) / span as f32;
    for w in &mut widths[start_col..end] {
        *w += add;
    }
}

/// Get a reference to a row box given a RowRef.
fn row_ref<'a>(table: &'a WebCore, rr: &RowRef) -> &'a WebCore {
    match rr.grandchild_idx {
        None => &table.children[rr.child_idx],
        Some(j) => &table.children[rr.child_idx].children[j],
    }
}
fn row_ref_mut<'a>(table: &'a mut WebCore, rr: &RowRef) -> &'a mut WebCore {
    match rr.grandchild_idx {
        None => &mut table.children[rr.child_idx],
        Some(j) => &mut table.children[rr.child_idx].children[j],
    }
}

// ─── Main table layout ────────────────────────────────────────────────────────

/// CSS table layout.
/// Mirrors C++ LayoutTable.
pub fn layout_table(
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
    if node.children.is_empty() {
        return finish_table(
            node,
            rbox,
            x + rbox.margin_left + rbox.border_left + rbox.padding_left,
            y + rbox.margin_top + rbox.border_top + rbox.padding_top,
            0.0,
            0.0,
        );
    }

    let spacing_h = {
        let raw = engine.res_len(
            &node.style.border_spacing_h,
            font_px,
            containing_w,
            root_font_px,
        );
        // HTML default: tables get 2px spacing when no CSS border-spacing is set
        let raw = if raw == 0.0 && node.style.border_spacing_h.is_auto() {
            2.0
        } else {
            raw
        };
        if node.style.border_collapse { 0.0 } else { raw }
    };
    let spacing_v = {
        let raw = engine.res_len(
            &node.style.border_spacing_v,
            font_px,
            containing_w,
            root_font_px,
        );
        let raw = if raw == 0.0 && node.style.border_spacing_v.is_auto() {
            2.0
        } else {
            raw
        };
        if node.style.border_collapse { 0.0 } else { raw }
    };
    let cellpad = node.style.cell_padding.clone();
    let collapse = node.style.border_collapse;

    let content_w = match rbox.content_width {
        Some(w) => w,
        None => (containing_w - rbox.h_space()).max(0.0),
    };
    // x already includes auto margin offset (computed in layout_box_with_fc)
    let content_x = x + rbox.margin_left + rbox.border_left + rbox.padding_left;
    let content_y = y + rbox.margin_top + rbox.border_top + rbox.padding_top;

    // Apply max-width constraint
    let mut table_width = if !node.style.width.is_auto() {
        engine.res_len(&node.style.width, font_px, containing_w, root_font_px)
    } else {
        content_w
    };
    if !node.style.max_width.is_none() && !node.style.max_width.is_auto() {
        let max_w = engine.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
        if max_w > 0.0 && table_width > max_w {
            table_width = max_w;
        }
    }

    // CSS table fixup: a table-cell cannot be a direct child of a table box.
    // Real HTML `<table><td>` is normalized by the tree builder; CSS display
    // tables need the same anonymous row box here in layout.
    wrap_direct_table_cells_in_anonymous_rows(node);

    // ── Collect rows ─────────────────────────────────────────────────────────
    let mut abs_children: Vec<usize> = Vec::new();
    let mut caption_idx: Option<usize> = None;
    let mut col_indices: Vec<usize> = Vec::new();
    let row_refs = collect_rows(node, &mut abs_children, &mut caption_idx, &mut col_indices);
    let num_rows = row_refs.len();
    let caption_side = caption_idx.map(|ci| node.children[ci].style.caption_side);
    let caption_at_bottom = matches!(
        caption_side,
        Some(CaptionSide::Bottom | CaptionSide::BlockEnd)
    );
    let caption_inline_start = matches!(caption_side, Some(CaptionSide::InlineStart));
    let caption_inline_end = matches!(caption_side, Some(CaptionSide::InlineEnd));

    if num_rows == 0 {
        return finish_table(node, rbox, content_x, content_y, table_width, 0.0);
    }

    // ── Count columns (considering colspan) ──────────────────────────────────
    let num_cols = {
        let mut nc = 0usize;
        for rr in &row_refs {
            let row = row_ref(node, rr);
            let cols_in_row: usize = row
                .children
                .iter()
                .filter(|c| {
                    matches!(
                        c.style.display,
                        Display::TableCell | Display::TableHeaderCell
                    )
                })
                .map(|c| get_colspan(c))
                .sum();
            if cols_in_row > nc {
                nc = cols_in_row;
            }
        }
        nc
    };
    if num_cols == 0 {
        return finish_table(node, rbox, content_x, content_y, table_width, 0.0);
    }

    // ── Build 2D cell grid for rowspan tracking ───────────────────────────────
    // grid[row][col] = slot describing which cell occupies it
    let mut grid: Vec<Vec<TableCellSlot>> =
        vec![vec![TableCellSlot::default(); num_cols]; num_rows];

    for (r, rr) in row_refs.iter().enumerate() {
        let row = row_ref(node, rr);
        let mut col = 0usize;
        for (ci, cell) in row.children.iter().enumerate() {
            if !matches!(
                cell.style.display,
                Display::TableCell | Display::TableHeaderCell
            ) {
                continue;
            }
            // Skip columns occupied by rowspan from above rows
            while col < num_cols && grid[r][col].box_path.is_some() {
                col += 1;
            }
            if col >= num_cols {
                break;
            }

            let cs = get_colspan(cell).min(num_cols - col);
            let rs = get_rowspan(cell).min(num_rows - r);

            // Fill occupied slots
            for rr2 in r..r + rs {
                for cc in col..col + cs {
                    grid[rr2][cc] = TableCellSlot {
                        box_path: Some((r, ci)),
                        owner_row: r,
                        colspan: cs,
                        rowspan: rs,
                    };
                }
            }
            col += cs;
        }
    }

    // ── For auto-width tables, pre-measure content to shrink-to-fit ──────────
    let total_spacing = spacing_h * (num_cols + 1) as f32;
    // Auto-width tables: measure intrinsic content width, then clamp to containing width.
    // This is the CSS shrink-to-fit algorithm (CSS 2.1 §10.3.5):
    // width = min(max(preferred minimum width, available width), preferred width)
    // For tables: preferred width = sum of column max-content widths.
    if node.style.width.is_auto() {
        // Measure each column's max-content width
        let mut col_max_content: Vec<f32> = vec![0.0; num_cols];
        for r in 0..num_rows {
            for c in 0..num_cols {
                let slot = &grid[r][c];
                if slot.owner_row != r {
                    continue;
                }
                if let Some((row_idx, ci)) = slot.box_path {
                    if c > 0 && grid[r][c - 1].box_path == Some((row_idx, ci)) {
                        continue;
                    }
                    let cell = &row_ref(node, &row_refs[row_idx]).children[ci];
                    let cw = if !cell.style.width.is_auto() {
                        engine.res_len(&cell.style.width, font_px, containing_w, root_font_px)
                    } else {
                        engine
                            .intrinsic_sizes(cell, font_px, root_font_px)
                            .max_content
                    };
                    if slot.colspan == 1 {
                        if cw > col_max_content[c] {
                            col_max_content[c] = cw;
                        }
                    } else {
                        distribute_spanned_width(
                            &mut col_max_content,
                            c,
                            slot.colspan,
                            cw,
                            spacing_h,
                        );
                    }
                }
            }
        }
        let mut intrinsic_w: f32 = col_max_content.iter().sum::<f32>() + total_spacing;
        if let Some(ci) = caption_idx {
            let cap_w = engine
                .intrinsic_sizes(&node.children[ci], font_px, root_font_px)
                .max_content;
            if !caption_inline_start && !caption_inline_end && cap_w > intrinsic_w {
                intrinsic_w = cap_w;
            }
        }
        // Shrink-to-fit: use intrinsic width but don't exceed container
        // and don't go below min-width
        let mut shrunk = intrinsic_w.min(content_w).max(0.0);
        if !node.style.min_width.is_auto() {
            let min_w = engine.res_len(&node.style.min_width, font_px, containing_w, root_font_px);
            if min_w > shrunk {
                shrunk = min_w;
            }
        }
        table_width = shrunk;
    }

    // ── Determine column widths ───────────────────────────────────────────────
    let cell_area = (table_width - total_spacing).max(0.0);

    let mut col_widths: Vec<f32> = vec![0.0; num_cols];
    let mut col_has_explicit: Vec<bool> = vec![false; num_cols];
    let mut explicit_count = 0usize;

    // Apply COL element widths (respecting col span)
    {
        let mut ci = 0usize;
        for &col_idx in &col_indices {
            if ci >= num_cols {
                break;
            }
            let col_box = &node.children[col_idx];
            let span = col_box
                .attributes
                .get("span")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            for _ in 0..span {
                if ci >= num_cols {
                    break;
                }
                if !col_box.style.width.is_auto() {
                    col_widths[ci] =
                        engine.res_len(&col_box.style.width, font_px, cell_area, root_font_px);
                    col_has_explicit[ci] = true;
                    explicit_count += 1;
                }
                ci += 1;
            }
        }
    }

    if node.style.table_layout_fixed {
        // Fixed layout: first row / col element widths only
        for c in 0..num_cols {
            let slot = &grid[0][c];
            if slot.owner_row == 0 {
                if let Some((_, ci)) = slot.box_path {
                    let cell = &row_ref(node, &row_refs[0]).children[ci];
                    if !cell.style.width.is_auto() {
                        let w = engine.res_len(&cell.style.width, font_px, cell_area, root_font_px);
                        if slot.colspan == 1 {
                            col_widths[c] = w;
                            if !col_has_explicit[c] {
                                col_has_explicit[c] = true;
                                explicit_count += 1;
                            }
                        } else {
                            distribute_spanned_width(
                                &mut col_widths,
                                c,
                                slot.colspan,
                                w,
                                spacing_h,
                            );
                            for cc in c..(c + slot.colspan).min(num_cols) {
                                if !col_has_explicit[cc] {
                                    col_has_explicit[cc] = true;
                                    explicit_count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        // Distribute remaining equally
        let used: f32 = col_widths.iter().sum();
        let remaining = cell_area - used;
        let flex_cols = num_cols - explicit_count;
        if flex_cols > 0 && remaining > 0.0 {
            let per = remaining / flex_cols as f32;
            for c in 0..num_cols {
                if !col_has_explicit[c] {
                    col_widths[c] = per;
                }
            }
        } else if flex_cols == 0 && remaining > 0.0 {
            let extra = remaining / num_cols as f32;
            for c in 0..num_cols {
                col_widths[c] += extra;
            }
        }
        if explicit_count == 0 {
            let per = cell_area / num_cols as f32;
            for c in 0..num_cols {
                col_widths[c] = per;
            }
        }
    } else {
        // Auto layout: three-pass — measure content, reserve min-content, then distribute.
        // CSS table spec: each auto column must receive at least its min-content width;
        // remaining space is then distributed proportionally by max-content.

        // Pass 1: Collect explicit widths AND measure min/max content for auto columns.
        let mut col_content_widths: Vec<f32> = vec![0.0; num_cols]; // max-content per col
        let mut col_min_widths: Vec<f32> = vec![0.0; num_cols]; // min-content per col
        for r in 0..num_rows {
            for c in 0..num_cols {
                let slot = &grid[r][c];
                if slot.owner_row != r {
                    continue;
                }
                if let Some((row_idx, ci)) = slot.box_path {
                    let cell = &row_ref(node, &row_refs[row_idx]).children[ci];
                    if !cell.style.width.is_auto() {
                        let w = engine.res_len(&cell.style.width, font_px, cell_area, root_font_px);
                        if slot.colspan == 1 {
                            if w > col_widths[c] {
                                if !col_has_explicit[c] {
                                    col_has_explicit[c] = true;
                                    explicit_count += 1;
                                }
                                col_widths[c] = w;
                            }
                        } else {
                            distribute_spanned_width(
                                &mut col_widths,
                                c,
                                slot.colspan,
                                w,
                                spacing_h,
                            );
                            for cc in c..(c + slot.colspan).min(num_cols) {
                                if !col_has_explicit[cc] {
                                    col_has_explicit[cc] = true;
                                    explicit_count += 1;
                                }
                            }
                        }
                    } else {
                        // Measure both min-content and max-content for auto columns.
                        let isz = engine.intrinsic_sizes(cell, font_px, root_font_px);
                        let cw = isz.max_content;
                        let mn = isz.min_content;
                        if slot.colspan == 1 {
                            if cw > col_content_widths[c] {
                                col_content_widths[c] = cw;
                            }
                            if mn > col_min_widths[c] {
                                col_min_widths[c] = mn;
                            }
                        } else {
                            distribute_spanned_width(
                                &mut col_content_widths,
                                c,
                                slot.colspan,
                                cw,
                                spacing_h,
                            );
                            distribute_spanned_width(
                                &mut col_min_widths,
                                c,
                                slot.colspan,
                                mn,
                                spacing_h,
                            );
                        }
                    }
                }
            }
        }

        // Pass 2: Reserve min-content widths for auto columns.
        // Explicit columns already have col_widths[c] set; only flex (auto) columns get min-content.
        {
            let used_explicit: f32 = col_widths.iter().sum();
            let mut flex_min_total: f32 = 0.0;
            for c in 0..num_cols {
                if !col_has_explicit[c] {
                    flex_min_total += col_min_widths[c];
                }
            }
            let available_for_flex = (cell_area - used_explicit).max(0.0);
            if flex_min_total > 0.0 {
                if flex_min_total <= available_for_flex {
                    // Enough room: give each auto column its min-content.
                    for c in 0..num_cols {
                        if !col_has_explicit[c] {
                            col_widths[c] = col_min_widths[c];
                        }
                    }
                } else {
                    // Not enough room even for minimums: distribute proportionally.
                    for c in 0..num_cols {
                        if !col_has_explicit[c] {
                            col_widths[c] =
                                (available_for_flex * col_min_widths[c] / flex_min_total).max(1.0);
                        }
                    }
                }
            }
        }

        // Pass 3: Distribute any remaining space proportionally by max-content surplus.
        {
            let used: f32 = col_widths.iter().sum();
            let remaining = cell_area - used;
            let flex_cols = num_cols - explicit_count;
            if remaining > 0.0 && flex_cols > 0 {
                // Surplus above min-content drives proportional distribution.
                let surplus: Vec<f32> = (0..num_cols)
                    .map(|c| {
                        if col_has_explicit[c] {
                            0.0
                        } else {
                            (col_content_widths[c] - col_min_widths[c]).max(0.0)
                        }
                    })
                    .collect();
                let total_surplus: f32 = surplus.iter().sum();
                if total_surplus > 0.0 {
                    for c in 0..num_cols {
                        if !col_has_explicit[c] {
                            col_widths[c] += remaining * surplus[c] / total_surplus;
                        }
                    }
                } else {
                    // All columns are at max-content already; split remainder equally.
                    let per = remaining / flex_cols as f32;
                    for c in 0..num_cols {
                        if !col_has_explicit[c] {
                            col_widths[c] += per;
                        }
                    }
                }
            } else if remaining > 0.0 {
                let extra = remaining / num_cols as f32;
                for c in 0..num_cols {
                    col_widths[c] += extra;
                }
            } else if remaining < 0.0 && used > 0.0 {
                for c in 0..num_cols {
                    col_widths[c] = (col_widths[c] * cell_area / used).max(1.0);
                }
            }
        }

        // Fallback: all zero → equal distribution
        if col_widths.iter().all(|&w| w == 0.0) {
            let per = cell_area / num_cols as f32;
            for c in 0..num_cols {
                col_widths[c] = per;
            }
        }
    }

    // Column X positions
    let col_x: Vec<f32> = {
        let mut xs = Vec::with_capacity(num_cols);
        let mut cx = spacing_h;
        for c in 0..num_cols {
            xs.push(cx);
            cx += col_widths[c] + spacing_h;
        }
        xs
    };

    // ── Layout caption ────────────────────────────────────────────────────────
    let mut caption_h = 0.0f32;
    let mut caption_w = 0.0f32;
    if let Some(ci) = caption_idx {
        engine.layout_box(
            &mut node.children[ci],
            &Constraints::new(table_width, content_x, content_y, font_px, root_font_px),
        );
        caption_h = node.children[ci].layout.margin_rect.h;
        caption_w = node.children[ci].layout.margin_rect.w;
        // Position at top for now; bottom case handled later
        let (dx, dy) = (
            content_x - node.children[ci].layout.margin_rect.x,
            content_y - node.children[ci].layout.margin_rect.y,
        );
        shift_rects(&mut node.children[ci], dx, dy);
    }

    // ── Layout cells, compute row heights ─────────────────────────────────────
    let mut row_heights: Vec<f32> = vec![0.0; num_rows];

    // First pass: layout cells at their owner row, compute heights
    for r in 0..num_rows {
        for c in 0..num_cols {
            let slot = &grid[r][c];
            if slot.owner_row != r {
                continue;
            }
            if let Some((row_idx, ci)) = slot.box_path {
                // Skip cells already counted (multi-column, not leftmost slot)
                if c > 0 && grid[r][c - 1].box_path == Some((row_idx, ci)) {
                    continue;
                }

                let cs = slot.colspan;
                let cell_w: f32 = (0..cs)
                    .map(|cc| col_widths[c + cc] + if cc > 0 { spacing_h } else { 0.0 })
                    .sum();

                // Apply cellpadding as a presentational hint.
                // Per the HTML spec, cellpadding maps to padding on td/th and sits
                // above the UA origin in the cascade. Override auto or UA-default
                // padding (1px), but not explicit author CSS padding.
                if !cellpad.is_auto() {
                    let row = row_ref_mut(node, &row_refs[row_idx]);
                    let cell = &mut row.children[ci];
                    let is_ua_default =
                        |v: &CssLength| v.is_auto() || matches!(v, CssLength::Px(p) if *p == 1.0);
                    if is_ua_default(&cell.style.padding_left) {
                        std::sync::Arc::make_mut(&mut cell.style).padding_left = cellpad.clone();
                    }
                    if is_ua_default(&cell.style.padding_right) {
                        std::sync::Arc::make_mut(&mut cell.style).padding_right = cellpad.clone();
                    }
                    if is_ua_default(&cell.style.padding_top) {
                        std::sync::Arc::make_mut(&mut cell.style).padding_top = cellpad.clone();
                    }
                    if is_ua_default(&cell.style.padding_bottom) {
                        std::sync::Arc::make_mut(&mut cell.style).padding_bottom = cellpad.clone();
                    }
                }

                // Layout cell — cell_w is already the resolved column width,
                // so percentage widths resolve correctly against it via Constraints.
                {
                    engine.layout_box(
                        &mut row_ref_mut(node, &row_refs[row_idx]).children[ci],
                        &Constraints::new(cell_w, content_x, content_y, font_px, root_font_px),
                    );
                }
                let (content_h, pad_top, pad_bottom, border_top, border_bottom) = {
                    let row = row_ref(node, &row_refs[row_idx]);
                    let cell = &row.children[ci];
                    (
                        cell.layout.content_rect.h,
                        cell.layout.resolved_pad_top,
                        cell.layout.resolved_pad_bottom,
                        cell.layout.resolved_border_top,
                        cell.layout.resolved_border_bottom,
                    )
                };
                let total_h = content_h + pad_top + pad_bottom + border_top + border_bottom;

                let rs = slot.rowspan;
                if rs == 1 {
                    if total_h > row_heights[r] {
                        row_heights[r] = total_h;
                    }
                }
            }
        }
    }

    for r in 0..num_rows {
        let row = row_ref(node, &row_refs[r]);
        if !row.style.height.is_auto() {
            let row_h = engine.res_len(&row.style.height, font_px, 0.0, root_font_px);
            if row_h > row_heights[r] {
                row_heights[r] = row_h;
            }
        }
    }

    // Handle rowspan: distribute spanned cell heights
    for r in 0..num_rows {
        for c in 0..num_cols {
            let slot = &grid[r][c];
            if slot.owner_row != r || slot.rowspan <= 1 {
                continue;
            }
            if let Some((row_idx, ci)) = slot.box_path {
                if c > 0 && grid[r][c - 1].box_path == Some((row_idx, ci)) {
                    continue;
                }

                let (content_h, pad_top, pad_bottom, border_top, border_bottom) = {
                    let row = row_ref(node, &row_refs[row_idx]);
                    let cell = &row.children[ci];
                    (
                        cell.layout.content_rect.h,
                        cell.layout.resolved_pad_top,
                        cell.layout.resolved_pad_bottom,
                        cell.layout.resolved_border_top,
                        cell.layout.resolved_border_bottom,
                    )
                };
                let total_h = content_h + pad_top + pad_bottom + border_top + border_bottom;
                let rs = slot.rowspan;

                let current_total: f32 = (r..r + rs)
                    .map(|rr| row_heights[rr] + spacing_v)
                    .sum::<f32>()
                    - spacing_v;
                if total_h > current_total {
                    let extra = total_h - current_total;
                    let per_row = extra / rs as f32;
                    for rr in r..r + rs {
                        row_heights[rr] += per_row;
                    }
                }
            }
        }
    }

    // ── Position cells ────────────────────────────────────────────────────────
    let caption_top_h = if !caption_at_bottom && !caption_inline_start && !caption_inline_end {
        caption_h
    } else {
        0.0
    };
    let side_caption_left_w = if caption_inline_start { caption_w } else { 0.0 };
    let side_caption_right_w = if caption_inline_end { caption_w } else { 0.0 };
    let grid_content_x = content_x + side_caption_left_w;
    let mut y_cursor = content_y + caption_top_h + spacing_v;

    for r in 0..num_rows {
        let row_h = row_heights[r];

        for c in 0..num_cols {
            let slot = grid[r][c].clone();
            if slot.owner_row != r {
                continue;
            }
            let (row_idx, ci) = match slot.box_path {
                Some(p) => p,
                None => continue,
            };
            if c > 0 && grid[r][c - 1].box_path == Some((row_idx, ci)) {
                continue;
            }

            let cs = slot.colspan;
            let rs = slot.rowspan;

            let cell_w: f32 = (0..cs)
                .map(|cc| col_widths[c + cc] + if cc > 0 { spacing_h } else { 0.0 })
                .sum();
            let cell_h: f32 = (0..rs)
                .map(|rr| row_heights[r + rr] + if rr > 0 { spacing_v } else { 0.0 })
                .sum();

            let (
                nat_h,
                v_align,
                empty_cells_hide,
                pos,
                old_bx,
                old_by,
                pad_top,
                pad_bottom,
                border_top,
                border_bottom,
            ) = {
                let row = row_ref(node, &row_refs[row_idx]);
                let cell = &row.children[ci];

                // Compute natural content height
                let cell_content_y = cell.layout.content_rect.y;
                let nat = {
                    let mut nh = 0.0f32;
                    if let Some(last) = cell.layout.line_cache.last() {
                        let bottom = last.y - cell_content_y + last.height;
                        if bottom > nh {
                            nh = bottom;
                        }
                    }
                    for ch in &cell.children {
                        if matches!(ch.style.display, Display::None) {
                            continue;
                        }
                        let cb = ch.layout.margin_rect.y + ch.layout.margin_rect.h - cell_content_y;
                        if cb > nh {
                            nh = cb;
                        }
                    }
                    if nh <= 0.0 {
                        nh = cell.layout.content_rect.h;
                    }
                    nh
                };

                let is_empty = cell.layout.inline_runs.is_empty() && cell.children.is_empty();

                (
                    nat,
                    cell.style.vertical_align.clone(),
                    cell.style.empty_cells_hide && is_empty,
                    cell.style.position,
                    cell.layout.border_rect.x,
                    cell.layout.border_rect.y,
                    cell.layout.resolved_pad_top,
                    cell.layout.resolved_pad_bottom,
                    cell.layout.resolved_border_top,
                    cell.layout.resolved_border_bottom,
                )
            };

            // Vertical alignment offset
            let avail_h = cell_h - pad_top - pad_bottom - border_top - border_bottom;
            let v_offset = match v_align {
                VerticalAlign::Middle => ((avail_h - nat_h) / 2.0).max(0.0),
                VerticalAlign::Bottom => (avail_h - nat_h).max(0.0),
                _ => 0.0,
            };

            let cell_x = grid_content_x + col_x[c];

            // Position cell: shift entire subtree from layout position to final grid position
            {
                let row = row_ref_mut(node, &row_refs[row_idx]);
                let cell = &mut row.children[ci];

                let dx = cell_x - old_bx;
                let dy = y_cursor - old_by;
                if dx != 0.0 || dy != 0.0 {
                    shift_rects(cell, dx, dy);
                }

                // Expand border/margin/padding rects to the allocated cell size
                cell.layout.border_rect.w = cell_w;
                cell.layout.border_rect.h = cell_h;
                cell.layout.margin_rect = cell.layout.border_rect;
                cell.layout.padding_rect.w =
                    cell_w - cell.layout.resolved_border_left - cell.layout.resolved_border_right;
                cell.layout.padding_rect.h =
                    cell_h - cell.layout.resolved_border_top - cell.layout.resolved_border_bottom;
                cell.layout.content_rect.w = cell.layout.padding_rect.w
                    - cell.layout.resolved_pad_left
                    - cell.layout.resolved_pad_right;
                cell.layout.content_rect.h = cell.layout.padding_rect.h
                    - cell.layout.resolved_pad_top
                    - cell.layout.resolved_pad_bottom;

                // Apply vertical alignment offset to content and children.
                // Use absolute positioning from the padding top to avoid
                // accumulating offsets across multiple layout passes.
                if v_offset > 0.0 {
                    let target_y = cell.layout.padding_rect.y + pad_top + v_offset;
                    let dy_align = target_y - cell.layout.content_rect.y;
                    if dy_align.abs() > 0.01 {
                        cell.layout.content_rect.y = target_y;
                        for ln in &mut cell.layout.line_cache {
                            ln.y += dy_align;
                        }
                        shift_children_y(&mut cell.children, dy_align);
                    }
                }

                // empty-cells: hide in separate border mode
                if !collapse && empty_cells_hide {
                    std::sync::Arc::make_mut(&mut cell.style).border_top_style = BorderStyle::None;
                    std::sync::Arc::make_mut(&mut cell.style).border_right_style =
                        BorderStyle::None;
                    std::sync::Arc::make_mut(&mut cell.style).border_bottom_style =
                        BorderStyle::None;
                    std::sync::Arc::make_mut(&mut cell.style).border_left_style = BorderStyle::None;
                    std::sync::Arc::make_mut(&mut cell.style).background_color = Color::TRANSPARENT;
                }

                if matches!(pos, Position::Relative | Position::Sticky) {
                    apply_relative_offset(
                        cell,
                        cell.style.font_size_px(font_px, root_font_px),
                        table_width,
                        root_font_px,
                    );
                }
            }
        }

        // Position row box
        {
            let row = row_ref_mut(node, &row_refs[r]);
            row.layout.content_rect = Rect::new(grid_content_x, y_cursor, table_width, row_h);
            row.layout.padding_rect = row.layout.content_rect;
            row.layout.border_rect = row.layout.content_rect;
            row.layout.margin_rect = row.layout.content_rect;
            if matches!(row.style.position, Position::Relative | Position::Sticky) {
                apply_relative_offset(
                    row,
                    row.style.font_size_px(font_px, root_font_px),
                    table_width,
                    root_font_px,
                );
            }
        }

        y_cursor += row_h + spacing_v;
    }

    // ── Position row groups ───────────────────────────────────────────────────
    for child in node.children.iter_mut() {
        if matches!(
            child.style.display,
            Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup
        ) {
            let mut min_y = f32::MAX;
            let mut max_bottom = 0.0f32;
            for gc in &child.children {
                if matches!(gc.style.display, Display::TableRow) {
                    if gc.layout.content_rect.y < min_y {
                        min_y = gc.layout.content_rect.y;
                    }
                    let b = gc.layout.content_rect.y + gc.layout.content_rect.h;
                    if b > max_bottom {
                        max_bottom = b;
                    }
                }
            }
            if min_y == f32::MAX {
                min_y = 0.0;
            }
            child.layout.content_rect =
                Rect::new(grid_content_x, min_y, table_width, max_bottom - min_y);
            child.layout.padding_rect = child.layout.content_rect;
            child.layout.border_rect = child.layout.content_rect;
            child.layout.margin_rect = child.layout.content_rect;
        }
    }

    // ── Border-collapse: resolve conflicts ────────────────────────────────────
    if collapse && num_rows > 0 && num_cols > 0 {
        resolve_collapsed_borders(node, &row_refs, &grid, num_rows, num_cols);
        // Clear table's own border in collapse mode (cells handle it)
        std::sync::Arc::make_mut(&mut node.style).border_top_style = BorderStyle::None;
        std::sync::Arc::make_mut(&mut node.style).border_top_width = CssLength::Zero;
        std::sync::Arc::make_mut(&mut node.style).border_right_style = BorderStyle::None;
        std::sync::Arc::make_mut(&mut node.style).border_right_width = CssLength::Zero;
        std::sync::Arc::make_mut(&mut node.style).border_bottom_style = BorderStyle::None;
        std::sync::Arc::make_mut(&mut node.style).border_bottom_width = CssLength::Zero;
        std::sync::Arc::make_mut(&mut node.style).border_left_style = BorderStyle::None;
        std::sync::Arc::make_mut(&mut node.style).border_left_width = CssLength::Zero;
    }

    // ── Position caption ──────────────────────────────────────────────────────
    if caption_at_bottom {
        if let Some(ci) = caption_idx {
            let (dx, dy) = (
                content_x - node.children[ci].layout.margin_rect.x,
                y_cursor - node.children[ci].layout.margin_rect.y,
            );
            shift_rects(&mut node.children[ci], dx, dy);
            y_cursor += caption_h;
        }
    } else if caption_inline_start || caption_inline_end {
        if let Some(ci) = caption_idx {
            let target_x = if caption_inline_end {
                grid_content_x + table_width
            } else {
                content_x
            };
            let (dx, dy) = (
                target_x - node.children[ci].layout.margin_rect.x,
                content_y - node.children[ci].layout.margin_rect.y,
            );
            shift_rects(&mut node.children[ci], dx, dy);
            y_cursor = y_cursor.max(content_y + caption_h);
        }
    }

    let table_height = y_cursor - content_y;
    let table_total_width = table_width + side_caption_left_w + side_caption_right_w;

    // ── Clear dirty flags on all descendants ─────────────────────────────────
    clear_dirty(node);

    // ── Finalize table box ────────────────────────────────────────────────────
    let result = finish_table(
        node,
        rbox,
        content_x,
        content_y,
        table_total_width,
        table_height,
    );

    // Absolute children
    let containing_rect = if crate::layout::establishes_positioned_containing_block(&node.style) {
        node.layout.padding_rect
    } else {
        engine.pos_cb.get()
    };
    for &i in &abs_children {
        layout_positioned(
            engine,
            &mut node.children[i],
            containing_rect,
            font_px,
            root_font_px,
        );
    }

    // Tables establish BFC: no margin collapsing through them
    node.layout.collapsed_margin_top = 0.0;
    node.layout.collapsed_margin_bottom = 0.0;
    node.layout.layout_dirty = false;

    result
}

// ─── Border-collapse conflict resolution ─────────────────────────────────────

fn resolve_collapsed_borders(
    node: &mut WebCore,
    row_refs: &[RowRef],
    grid: &[Vec<TableCellSlot>],
    num_rows: usize,
    num_cols: usize,
) {
    // Horizontal edges: between row r bottom and row r+1 top
    for r in 0..num_rows.saturating_sub(1) {
        for c in 0..num_cols {
            let top_path = grid[r][c].box_path;
            let bot_path = grid[r + 1][c].box_path;
            if top_path.is_none() || bot_path.is_none() {
                continue;
            }
            if top_path == bot_path {
                continue;
            } // same cell (rowspan)
            if c > 0
                && grid[r][c - 1].box_path == top_path
                && grid[r + 1][c - 1].box_path == bot_path
            {
                continue;
            }

            let (top_w, top_s) = {
                let (ri, ci) = top_path.unwrap();
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_bottom_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_bottom_style,
                )
            };
            let (bot_w, bot_s) = {
                let (ri, ci) = bot_path.unwrap();
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_top_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_top_style,
                )
            };
            if border_wins(top_w, top_s, bot_w, bot_s) {
                let (ri, ci) = bot_path.unwrap();
                let row = row_ref_mut(node, &row_refs[ri]);
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_top_style =
                    BorderStyle::None;
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_top_width =
                    CssLength::Zero;
            } else {
                let (ri, ci) = top_path.unwrap();
                let row = row_ref_mut(node, &row_refs[ri]);
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_bottom_style =
                    BorderStyle::None;
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_bottom_width =
                    CssLength::Zero;
            }
        }
    }

    // Vertical edges: between col c right and col c+1 left
    for r in 0..num_rows {
        for c in 0..num_cols.saturating_sub(1) {
            let left_path = grid[r][c].box_path;
            let right_path = grid[r][c + 1].box_path;
            if left_path.is_none() || right_path.is_none() {
                continue;
            }
            if left_path == right_path {
                continue;
            }
            if r > 0
                && grid[r - 1][c].box_path == left_path
                && grid[r - 1][c + 1].box_path == right_path
            {
                continue;
            }

            let (l_w, l_s) = {
                let (ri, ci) = left_path.unwrap();
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_right_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_right_style,
                )
            };
            let (r_w, r_s) = {
                let (ri, ci) = right_path.unwrap();
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_left_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_left_style,
                )
            };
            if border_wins(l_w, l_s, r_w, r_s) {
                let (ri, ci) = right_path.unwrap();
                let row = row_ref_mut(node, &row_refs[ri]);
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_left_style =
                    BorderStyle::None;
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_left_width =
                    CssLength::Zero;
            } else {
                let (ri, ci) = left_path.unwrap();
                let row = row_ref_mut(node, &row_refs[ri]);
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_right_style =
                    BorderStyle::None;
                std::sync::Arc::make_mut(&mut row.children[ci].style).border_right_width =
                    CssLength::Zero;
            }
        }
    }

    // Table border vs. outer cells (CSS 2.1 §17.6.2.1)
    // Snapshot table border values before mutating cells
    let tbl_top_w = node.style.border_top_width.clone();
    let tbl_top_s = node.style.border_top_style;
    let tbl_top_c = node.style.border_top_color;
    let tbl_bot_w = node.style.border_bottom_width.clone();
    let tbl_bot_s = node.style.border_bottom_style;
    let tbl_bot_c = node.style.border_bottom_color;
    let tbl_left_w = node.style.border_left_width.clone();
    let tbl_left_s = node.style.border_left_style;
    let tbl_left_c = node.style.border_left_color;
    let tbl_right_w = node.style.border_right_width.clone();
    let tbl_right_s = node.style.border_right_style;
    let tbl_right_c = node.style.border_right_color;

    // Top row
    for c in 0..num_cols {
        if let Some((ri, ci)) = grid[0][c].box_path {
            let (tw, ts) = (tbl_top_w.resolve(0.0, 0.0, 0.0), tbl_top_s);
            let (cw, cs) = {
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_top_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_top_style,
                )
            };
            if border_wins(tw, ts, cw, cs) {
                let cell = &mut row_ref_mut(node, &row_refs[ri]).children[ci];
                std::sync::Arc::make_mut(&mut cell.style).border_top_width = tbl_top_w.clone();
                std::sync::Arc::make_mut(&mut cell.style).border_top_style = tbl_top_s;
                std::sync::Arc::make_mut(&mut cell.style).border_top_color = tbl_top_c;
            }
        }
    }
    // Bottom row
    for c in 0..num_cols {
        if let Some((ri, ci)) = grid[num_rows - 1][c].box_path {
            let (tw, ts) = (tbl_bot_w.resolve(0.0, 0.0, 0.0), tbl_bot_s);
            let (cw, cs) = {
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_bottom_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_bottom_style,
                )
            };
            if border_wins(tw, ts, cw, cs) {
                let cell = &mut row_ref_mut(node, &row_refs[ri]).children[ci];
                std::sync::Arc::make_mut(&mut cell.style).border_bottom_width = tbl_bot_w.clone();
                std::sync::Arc::make_mut(&mut cell.style).border_bottom_style = tbl_bot_s;
                std::sync::Arc::make_mut(&mut cell.style).border_bottom_color = tbl_bot_c;
            }
        }
    }
    // Left column
    for r in 0..num_rows {
        if let Some((ri, ci)) = grid[r][0].box_path {
            let (tw, ts) = (tbl_left_w.resolve(0.0, 0.0, 0.0), tbl_left_s);
            let (cw, cs) = {
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_left_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_left_style,
                )
            };
            if border_wins(tw, ts, cw, cs) {
                let cell = &mut row_ref_mut(node, &row_refs[ri]).children[ci];
                std::sync::Arc::make_mut(&mut cell.style).border_left_width = tbl_left_w.clone();
                std::sync::Arc::make_mut(&mut cell.style).border_left_style = tbl_left_s;
                std::sync::Arc::make_mut(&mut cell.style).border_left_color = tbl_left_c;
            }
        }
    }
    // Right column
    for r in 0..num_rows {
        if let Some((ri, ci)) = grid[r][num_cols - 1].box_path {
            let (tw, ts) = (tbl_right_w.resolve(0.0, 0.0, 0.0), tbl_right_s);
            let (cw, cs) = {
                let cell = &row_ref(node, &row_refs[ri]).children[ci];
                (
                    cell.style.border_right_width.resolve(0.0, 0.0, 0.0),
                    cell.style.border_right_style,
                )
            };
            if border_wins(tw, ts, cw, cs) {
                let cell = &mut row_ref_mut(node, &row_refs[ri]).children[ci];
                std::sync::Arc::make_mut(&mut cell.style).border_right_width = tbl_right_w.clone();
                std::sync::Arc::make_mut(&mut cell.style).border_right_style = tbl_right_s;
                std::sync::Arc::make_mut(&mut cell.style).border_right_color = tbl_right_c;
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn shift_children_y(children: &mut Vec<WebCore>, dy: f32) {
    for child in children.iter_mut() {
        shift_rects(child, 0.0, dy);
    }
}

fn clear_dirty(node: &mut WebCore) {
    node.layout.layout_dirty = false;
    for child in &mut node.children {
        clear_dirty(child);
    }
}

fn finish_table(
    node: &mut WebCore,
    rbox: &ResolvedBox,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
) -> f32 {
    let ch = rbox.content_height.unwrap_or(content_h).max(0.0);
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
    node.layout.baseline = content_y + ch;
    node.layout.margin_rect.h
}
