// Table tests – ported from cpptests/test_table.cpp
// Render smoke tests skipped. Tests using FindAllBoxes replaced with walk_boxes.
use webcore::types::*;
use webcore::{load_html, parse_html};

fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

fn count_boxes(root: &WebCore, pred: &dyn Fn(&WebCore) -> bool) -> usize {
    let mut n = if pred(root) { 1 } else { 0 };
    for child in &root.children {
        n += count_boxes(child, pred);
    }
    n
}

fn walk_boxes<'a>(root: &'a WebCore, out: &mut Vec<&'a WebCore>, pred: &dyn Fn(&WebCore) -> bool) {
    if pred(root) {
        out.push(root);
    }
    for child in &root.children {
        walk_boxes(child, out, pred);
    }
}

fn find_all_boxes<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut v = Vec::new();
    walk_boxes(root, &mut v, pred);
    v
}

// ============================================================
// Table Structure
// ============================================================

#[test]
fn basic_structure() {
    let doc = parse_html(
        "<table><tr><td>A</td><td>B</td></tr>\
         <tr><td>C</td><td>D</td></tr></table>",
    );
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
}

#[test]
fn row_count() {
    let doc =
        parse_html("<table><tr><td>A</td></tr><tr><td>B</td></tr><tr><td>C</td></tr></table>");
    let count = count_boxes(&doc.root, &|b| b.tag == "tr");
    assert_eq!(count, 3);
}

#[test]
fn cell_count() {
    let doc = parse_html(
        "<table><tr><td>A</td><td>B</td></tr>\
         <tr><td>C</td><td>D</td></tr></table>",
    );
    let count = count_boxes(&doc.root, &|b| b.tag == "td");
    assert_eq!(count, 4);
}

#[test]
fn th_elements() {
    let doc = parse_html("<table><tr><th>Header</th></tr><tr><td>Data</td></tr></table>");
    let th = find_box(&doc.root, &|b| b.tag == "th");
    assert!(th.is_some());
}

// ============================================================
// Table Properties
// ============================================================

#[test]
fn border_collapse_parsed() {
    let doc = parse_html("<table style=\"border-collapse: collapse;\"><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table" && b.style.border_collapse);
    assert!(table.is_some());
}

// ============================================================
// Table Layout
// ============================================================

#[test]
fn layout_produces_rects() {
    let doc = load_html("<table><tr><td>A</td><td>B</td></tr></table>", 800.0);
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.layout.content_rect.w > 0.0);
    assert!(table.layout.content_rect.h > 0.0);
}

#[test]
fn cells_have_dimensions() {
    let doc = load_html(
        "<table><tr><td>Cell A</td><td>Cell B</td></tr></table>",
        800.0,
    );
    let count = count_boxes(&doc.root, &|b| {
        b.tag == "td" && b.layout.content_rect.w > 0.0 && b.layout.content_rect.h > 0.0
    });
    assert_eq!(count, 2);
}

#[test]
fn cells_side_by_side() {
    let doc = load_html("<table><tr><td>A</td><td>B</td></tr></table>", 800.0);
    let mut cells = Vec::new();
    walk_boxes(&doc.root, &mut cells, &|b| b.tag == "td");
    assert_eq!(cells.len(), 2);
    assert!(cells[1].layout.content_rect.x > cells[0].layout.content_rect.x);
}

#[test]
fn rows_stacked() {
    let doc = load_html(
        "<table><tr><td>Row1</td></tr><tr><td>Row2</td></tr></table>",
        800.0,
    );
    let mut rows = Vec::new();
    walk_boxes(&doc.root, &mut rows, &|b| b.tag == "tr");
    assert_eq!(rows.len(), 2);
    assert!(rows[1].layout.content_rect.y > rows[0].layout.content_rect.y);
}

#[test]
fn explicit_width() {
    let doc = load_html(
        "<table style=\"width: 600px;\"><tr><td>A</td><td>B</td></tr></table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.layout.content_rect.w >= 590.0 && table.layout.content_rect.w <= 610.0);
}

// ============================================================
// Row Groups (thead/tbody/tfoot)
// ============================================================

#[test]
fn thead_tbody_tfoot() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <thead><tr><th>Header</th></tr></thead>\
         <tbody><tr><td>Body</td></tr></tbody>\
         <tfoot><tr><td>Footer</td></tr></tfoot>\
         </table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.layout.content_rect.h > 0.0);
}

#[test]
fn tbody_rows_stacked() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <tbody><tr><td>Row1</td></tr><tr><td>Row2</td></tr></tbody>\
         </table>",
        800.0,
    );
    let mut rows = Vec::new();
    walk_boxes(&doc.root, &mut rows, &|b| b.tag == "tr");
    assert_eq!(rows.len(), 2);
    assert!(rows[1].layout.content_rect.y > rows[0].layout.content_rect.y);
}

// ============================================================
// Caption
// ============================================================

#[test]
fn caption_exists() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <caption>My Table</caption>\
         <tr><td>A</td></tr>\
         </table>",
        800.0,
    );
    let cap = find_box(&doc.root, &|b| b.tag == "caption");
    assert!(cap.is_some());
}

// ============================================================
// table-layout: fixed
// ============================================================

#[test]
fn table_layout_fixed_parsed() {
    let doc = parse_html("<table style='table-layout: fixed;'><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.style.table_layout_fixed);
}

// ============================================================
// border-spacing CSS property
// ============================================================

#[test]
fn border_collapse_layout() {
    let doc = load_html(
        "<table style='width: 400px; border-collapse: collapse;'>\
         <tr><td>A</td><td>B</td></tr>\
         <tr><td>C</td><td>D</td></tr>\
         </table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.style.border_collapse);
    assert!(table.layout.content_rect.w > 0.0);
    assert!(table.layout.content_rect.h > 0.0);
}

// ============================================================
// empty-cells: hide
// ============================================================

#[test]
fn empty_cells_hide_parsed() {
    let doc = parse_html("<table style='empty-cells: hide;'><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(table.style.empty_cells_hide);
}

// ============================================================
// Caption-side
// ============================================================

#[test]
fn caption_side_top() {
    let doc = load_html(
        "<table><caption>Title</caption>\
         <tr><td>A</td></tr></table>",
        400.0,
    );
    let caption = find_box(&doc.root, &|b| b.style.display == Display::TableCaption);
    let row = find_box(&doc.root, &|b| b.style.display == Display::TableRow);
    assert!(caption.is_some());
    assert!(row.is_some());
    assert!(caption.unwrap().layout.margin_rect.y < row.unwrap().layout.content_rect.y);
}

#[test]
fn caption_side_bottom() {
    let doc = load_html(
        "<table><caption style=\"caption-side: bottom;\">Title</caption>\
         <tr><td>A</td></tr></table>",
        400.0,
    );
    let caption = find_box(&doc.root, &|b| b.style.display == Display::TableCaption);
    let row = find_box(&doc.root, &|b| b.style.display == Display::TableRow);
    assert!(caption.is_some());
    assert!(row.is_some());
    assert!(caption.unwrap().layout.margin_rect.y > row.unwrap().layout.content_rect.y);
}

// ============================================================
// tfoot ordering
// ============================================================

#[test]
fn tfoot_rendered_after_tbody() {
    let doc = load_html(
        "<table>\
         <tfoot><tr><td>Footer</td></tr></tfoot>\
         <tbody><tr><td>Body</td></tr></tbody>\
         </table>",
        400.0,
    );
    let tfoot = find_box(&doc.root, &|b| b.tag == "tfoot");
    let tbody = find_box(&doc.root, &|b| b.tag == "tbody");
    assert!(tfoot.is_some());
    assert!(tbody.is_some());
    let tfoot_row = find_box(tfoot.unwrap(), &|b| b.style.display == Display::TableRow);
    let tbody_row = find_box(tbody.unwrap(), &|b| b.style.display == Display::TableRow);
    assert!(tfoot_row.is_some());
    assert!(tbody_row.is_some());
    assert!(tbody_row.unwrap().layout.content_rect.y < tfoot_row.unwrap().layout.content_rect.y);
}

// ============================================================
// col/colgroup
// ============================================================

#[test]
fn col_display_type() {
    let doc = parse_html("<table><col><tr><td>A</td></tr></table>");
    let col = find_box(&doc.root, &|b| b.style.display == Display::TableColumn);
    assert!(col.is_some());
}

#[test]
fn colgroup_display_type() {
    let doc = parse_html("<table><colgroup><col></colgroup><tr><td>A</td></tr></table>");
    let cg = find_box(&doc.root, &|b| b.style.display == Display::TableColumnGroup);
    assert!(cg.is_some());
}

// ============================================================
// Table Properties (colspan / rowspan via attributes)
// ============================================================

#[test]
fn colspan_attribute() {
    let doc = parse_html(
        "<table><tr><td colspan=\"2\">Wide</td></tr>\
         <tr><td>A</td><td>B</td></tr></table>",
    );
    let wide = find_box(&doc.root, &|b| {
        b.attributes
            .get("colspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(wide.is_some());
}

#[test]
fn rowspan_attribute() {
    let doc = parse_html(
        "<table><tr><td rowspan=\"2\">Tall</td><td>B</td></tr>\
         <tr><td>C</td></tr></table>",
    );
    let tall = find_box(&doc.root, &|b| {
        b.attributes
            .get("rowspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(tall.is_some());
}

#[test]
fn cell_padding_smoke() {
    let doc = parse_html("<table cellpadding=\"10\"><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
}

// ============================================================
// Table Layout – colspan / rowspan with dimensions
// ============================================================

#[test]
fn colspan_widens_cells() {
    let doc = load_html(
        "<table style=\"width: 400px;\">\
         <tr><td colspan=\"2\">Wide</td></tr>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        800.0,
    );
    let wide = find_box(&doc.root, &|b| {
        b.attributes
            .get("colspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(wide.is_some());
    let wide = wide.unwrap();
    let normal = find_box(&doc.root, &|b| {
        b.tag == "td"
            && b.attributes
                .get("colspan")
                .map(|v| v == "1")
                .unwrap_or(true)
            && !b.attributes.contains_key("colspan")
            && b.layout.content_rect.w > 0.0
    });
    if let Some(normal) = normal {
        assert!(
            wide.layout.padding_rect.w > normal.layout.padding_rect.w,
            "colspan cell ({}) should be wider than normal cell ({})",
            wide.layout.padding_rect.w,
            normal.layout.padding_rect.w
        );
    }
}

#[test]
fn rowspan_layout() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <tr><td rowspan='2'>Tall</td><td>B</td></tr>\
         <tr><td>C</td></tr>\
         </table>",
        800.0,
    );
    let tall = find_box(&doc.root, &|b| {
        b.attributes
            .get("rowspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(tall.is_some(), "rowspan=2 cell not found");
    let mut rows = Vec::new();
    walk_boxes(&doc.root, &mut rows, &|b| b.tag == "tr");
    assert_eq!(rows.len(), 2);
    let total_row_height = rows[0].layout.content_rect.h + rows[1].layout.content_rect.h;
    let tall = tall.unwrap();
    assert!(
        tall.layout.padding_rect.h >= total_row_height - 2.0,
        "tall cell height {} should span both rows ({})",
        tall.layout.padding_rect.h,
        total_row_height
    );
}

#[test]
fn rowspan_three_rows() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <tr><td rowspan='3'>Tall</td><td>A</td></tr>\
         <tr><td>B</td></tr>\
         <tr><td>C</td></tr>\
         </table>",
        800.0,
    );
    let tall = find_box(&doc.root, &|b| {
        b.attributes
            .get("rowspan")
            .map(|v| v == "3")
            .unwrap_or(false)
    });
    assert!(tall.is_some());
    let mut rows = Vec::new();
    walk_boxes(&doc.root, &mut rows, &|b| b.tag == "tr");
    assert_eq!(rows.len(), 3);
}

// ============================================================
// Smoke tests (parse + layout, no render needed)
// ============================================================

#[test]
fn styled_table_smoke() {
    let doc = load_html(
        "<table style=\"width: 100%; border: 1px solid black; border-collapse: collapse;\">\
         <tr style=\"background-color: #eee;\">\
           <th style=\"border: 1px solid #999; padding: 8px;\">Name</th>\
           <th style=\"border: 1px solid #999; padding: 8px;\">Value</th>\
         </tr>\
         <tr><td style=\"border: 1px solid #999; padding: 8px;\">Alpha</td>\
             <td style=\"border: 1px solid #999; padding: 8px;\">100</td></tr>\
         <tr><td style=\"border: 1px solid #999; padding: 8px;\">Beta</td>\
             <td style=\"border: 1px solid #999; padding: 8px;\">200</td></tr>\
         </table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
}

#[test]
fn nested_table_smoke() {
    let doc = load_html(
        "<table><tr><td>\
           <table><tr><td>Inner</td></tr></table>\
         </td><td>Outer</td></tr></table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
}

#[test]
fn email_style_smoke() {
    let doc = load_html(
        "<table role='presentation' width='600' cellpadding='0' cellspacing='0' \
         style='width: 600px; border-collapse: collapse; border-spacing: 0;'>\
         <tr>\
           <td style='padding: 20px; background-color: #f4f4f4;'>\
             <table width='100%' cellpadding='0' cellspacing='0'>\
             <tr><td style='font-size: 24px; font-weight: bold; padding-bottom: 10px;'>\
               Welcome!</td></tr>\
             <tr><td style='font-size: 14px; line-height: 1.5; color: #333;'>\
               This is a test email layout.</td></tr>\
             </table>\
           </td>\
         </tr>\
         </table>",
        800.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
}

// ============================================================
// Content-based and explicit cell sizing
// ============================================================

#[test]
fn content_based_sizing() {
    let doc = load_html(
        "<table style='width: 600px;'>\
         <tr>\
         <td>Short</td>\
         <td>This is a much longer piece of text content</td>\
         </tr>\
         </table>",
        800.0,
    );
    let mut cells = Vec::new();
    walk_boxes(&doc.root, &mut cells, &|b| b.tag == "td");
    assert_eq!(cells.len(), 2);
    assert!(cells[0].layout.content_rect.w > 0.0);
    assert!(cells[1].layout.content_rect.w > 0.0);
}

#[test]
fn explicit_cell_width() {
    let doc = load_html(
        "<table style='width: 600px;'>\
         <tr>\
         <td style='width: 200px;'>Fixed</td>\
         <td>Flex</td>\
         </tr>\
         </table>",
        800.0,
    );
    let mut cells = Vec::new();
    walk_boxes(&doc.root, &mut cells, &|b| b.tag == "td");
    assert_eq!(cells.len(), 2);
    assert!(
        cells[0].layout.padding_rect.w >= 180.0,
        "fixed cell paddingRect.w {} should be >= 180",
        cells[0].layout.padding_rect.w
    );
    assert!(
        cells[0].layout.padding_rect.w <= 220.0,
        "fixed cell paddingRect.w {} should be <= 220",
        cells[0].layout.padding_rect.w
    );
}

// ============================================================
// Vertical alignment
// ============================================================

#[test]
fn vertical_align_middle() {
    let doc = load_html(
        "<table style='width: 400px;'>\
         <tr>\
         <td style='height: 100px; vertical-align: middle;'>Middle</td>\
         <td>Normal text that fills less than 100px</td>\
         </tr>\
         </table>",
        800.0,
    );
    // Check that the cell with vertical-align:middle has VerticalAlign::Middle style
    let middle_cell = find_box(&doc.root, &|b| {
        b.tag == "td" && b.style.vertical_align == VerticalAlign::Middle
    });
    assert!(
        middle_cell.is_some(),
        "cell with vertical-align:middle not found"
    );
}

// ============================================================
// Colspan + Rowspan combined
// ============================================================

#[test]
fn colspan_and_rowspan() {
    let doc = load_html(
        "<table style='width: 600px;'>\
         <tr><td colspan='2'>Wide</td><td>C</td></tr>\
         <tr><td rowspan='2'>Tall</td><td>E</td><td>F</td></tr>\
         <tr><td>H</td><td>I</td></tr>\
         </table>",
        800.0,
    );
    let wide = find_box(&doc.root, &|b| {
        b.attributes
            .get("colspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    let tall = find_box(&doc.root, &|b| {
        b.attributes
            .get("rowspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(wide.is_some());
    assert!(tall.is_some());
    // wide cell should be wider than a normal (no span) cell
    let normal = find_box(&doc.root, &|b| {
        b.tag == "td"
            && !b.attributes.contains_key("colspan")
            && !b.attributes.contains_key("rowspan")
            && b.layout.padding_rect.w > 0.0
    });
    if let Some(normal) = normal {
        assert!(
            wide.unwrap().layout.padding_rect.w > normal.layout.padding_rect.w,
            "colspan cell should be wider than normal cell"
        );
    }
}

// ============================================================
// border-spacing CSS property
// ============================================================

#[test]
fn border_spacing_css() {
    let doc = parse_html("<table style='border-spacing: 10px;'><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    // border-spacing: 10px maps to border_spacing_h = Px(10.0)
    assert_eq!(
        table.style.border_spacing_h,
        CssLength::Px(10.0),
        "border_spacing_h should be Px(10)"
    );
}

#[test]
fn border_spacing_zero() {
    let doc = parse_html("<table style='border-spacing: 0;'><tr><td>A</td></tr></table>");
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    let spacing_px = table.style.border_spacing_h.resolve(16.0, 0.0, 16.0);
    assert!(
        spacing_px == 0.0,
        "border-spacing: 0 should resolve to 0, got {}",
        spacing_px
    );
}

// ============================================================
// table-layout: fixed  (equal column sizing)
// ============================================================

#[test]
fn table_layout_fixed_equal_columns() {
    let doc = load_html(
        "<table style='width: 600px; table-layout: fixed;'>\
         <tr><td>Short</td><td>Much longer text here</td></tr>\
         </table>",
        800.0,
    );
    let mut cells = Vec::new();
    walk_boxes(&doc.root, &mut cells, &|b| b.tag == "td");
    assert_eq!(cells.len(), 2);
    let diff = (cells[0].layout.padding_rect.w - cells[1].layout.padding_rect.w).abs();
    assert!(
        diff <= 2.0,
        "fixed-layout equal columns should differ by <= 2px, got {}",
        diff
    );
}

#[test]
fn table_layout_fixed_respects_explicit() {
    let doc = load_html(
        "<table style='width: 600px; table-layout: fixed;'>\
         <tr><td style='width: 200px;'>Fixed</td><td>Auto</td></tr>\
         </table>",
        800.0,
    );
    let mut cells = Vec::new();
    walk_boxes(&doc.root, &mut cells, &|b| b.tag == "td");
    assert_eq!(cells.len(), 2);
    assert!(
        cells[0].layout.padding_rect.w >= 190.0,
        "fixed-width cell should be >= 190, got {}",
        cells[0].layout.padding_rect.w
    );
    assert!(
        cells[0].layout.padding_rect.w <= 210.0,
        "fixed-width cell should be <= 210, got {}",
        cells[0].layout.padding_rect.w
    );
    assert!(
        cells[1].layout.padding_rect.w > cells[0].layout.padding_rect.w,
        "auto cell should be wider than fixed cell"
    );
}

// ============================================================
// empty-cells: hide / show / interaction with border-collapse
// ============================================================

#[test]
fn empty_cells_hide() {
    // empty-cells: hide on the table — parsed and stored
    let doc = load_html(
        "<table style=\"empty-cells: hide; border-collapse: separate;\">\
         <tr><td style=\"border: 2px solid red; background: yellow;\">Content</td>\
         <td style=\"border: 2px solid red; background: yellow;\"></td></tr>\
         </table>",
        400.0,
    );
    // The table itself should have empty_cells_hide = true
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
    assert!(
        table.unwrap().style.empty_cells_hide,
        "table with empty-cells:hide should have empty_cells_hide=true"
    );
    // Cells must still be laid out (no crash)
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2, "should have 2 cells");
}

#[test]
fn empty_cells_show() {
    // Default (no empty-cells property) — table should NOT have empty_cells_hide
    let doc = load_html(
        "<table style=\"border-collapse: separate;\">\
         <tr><td style=\"border: 2px solid red;\">Content</td>\
         <td style=\"border: 2px solid red;\"></td></tr>\
         </table>",
        400.0,
    );
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
    assert!(
        !table.unwrap().style.empty_cells_hide,
        "table without empty-cells: hide should have empty_cells_hide=false"
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Both cells keep their border style (Solid from inline style)
    assert_eq!(
        cells[0].style.border_top_style,
        BorderStyle::Solid,
        "content cell keeps border style"
    );
    assert_eq!(
        cells[1].style.border_top_style,
        BorderStyle::Solid,
        "empty cell also keeps border style with default empty-cells: show"
    );
}

#[test]
fn empty_cells_hide_not_in_collapse() {
    let doc = load_html(
        "<table style=\"empty-cells: hide; border-collapse: collapse;\">\
         <tr><td style=\"border: 2px solid red;\">Content</td>\
         <td style=\"border: 2px solid red;\"></td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // In collapse mode, empty-cells: hide should be ignored at the table level.
    // Both cells should have been laid out.
    assert!(cells.len() >= 2, "should have 2 cells in collapse mode");
    // The table must still have positive dimensions (layout didn't break)
    let table = find_box(&doc.root, &|b| b.tag == "table");
    assert!(table.is_some());
    assert!(
        table.unwrap().layout.content_rect.w > 0.0,
        "table with collapse+empty-cells:hide should still have positive width"
    );
}

// ============================================================
// Border-collapse conflict resolution
// ============================================================

#[test]
fn border_collapse_spacing_zero() {
    let doc = load_html(
        "<table style=\"border-collapse: collapse; border-spacing: 10px;\">\
         <tr><td>A</td><td>B</td></tr></table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // In collapse mode, cells should be adjacent (gap <= 0)
    let gap = cells[1].layout.padding_rect.x
        - (cells[0].layout.padding_rect.x + cells[0].layout.padding_rect.w);
    assert!(
        gap <= 0.0,
        "cells in collapse mode should be adjacent, got gap={}",
        gap
    );
}

#[test]
fn border_collapse_adjacent_border_resolution() {
    let doc = load_html(
        "<table style=\"border-collapse: collapse;\">\
         <tr><td style=\"border-right: 3px solid red;\">A</td>\
             <td style=\"border-left: 1px solid blue;\">B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Cell A's right border (3px) should win over Cell B's left border (1px)
    assert_eq!(
        cells[0].style.border_right_width,
        CssLength::Px(3.0),
        "winning border (3px) should be kept"
    );
    // The losing border is zeroed — it may be Px(0.0) or Zero
    let left_w = cells[1].style.border_left_width.resolve(16.0, 0.0, 16.0);
    assert_eq!(
        left_w, 0.0,
        "losing border (1px) should be zeroed, got {:?}",
        cells[1].style.border_left_width
    );
}

#[test]
fn border_collapse_vertical_resolution() {
    let doc = load_html(
        "<table style=\"border-collapse: collapse;\">\
         <tr><td style=\"border-bottom: 4px solid green;\">Top</td></tr>\
         <tr><td style=\"border-top: 1px solid black;\">Bot</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Top cell's bottom border (4px) wins
    assert_eq!(cells[0].style.border_bottom_width, CssLength::Px(4.0));
    let top_w = cells[1].style.border_top_width.resolve(16.0, 0.0, 16.0);
    assert_eq!(
        top_w, 0.0,
        "losing top border should be zeroed, got {:?}",
        cells[1].style.border_top_width
    );
}

#[test]
fn border_collapse_style_priority() {
    // Equal width: double style wins over solid
    let doc = load_html(
        "<table style=\"border-collapse: collapse;\">\
         <tr><td style=\"border-right: 2px solid red;\">A</td>\
             <td style=\"border-left: 2px double blue;\">B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Cell B's double border wins over Cell A's solid (same width)
    let right_w = cells[0].style.border_right_width.resolve(16.0, 0.0, 16.0);
    assert_eq!(
        right_w, 0.0,
        "solid loser should be zeroed, got {:?}",
        cells[0].style.border_right_width
    );
    assert_eq!(
        cells[1].style.border_left_width,
        CssLength::Px(2.0),
        "double winner should be kept"
    );
}

#[test]
fn border_collapse_separate_noop() {
    // border-collapse: separate should NOT resolve borders
    let doc = load_html(
        "<table style=\"border-collapse: separate;\">\
         <tr><td style=\"border: 2px solid red;\">A</td>\
             <td style=\"border: 1px solid blue;\">B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Both cells keep their original borders
    assert_eq!(cells[0].style.border_right_width, CssLength::Px(2.0));
    assert_eq!(cells[1].style.border_left_width, CssLength::Px(1.0));
}

// ============================================================
// thead / tbody / tfoot ordering
// ============================================================

#[test]
fn tfoot_at_end_in_source() {
    let doc = load_html(
        "<table>\
         <thead><tr><td>H</td></tr></thead>\
         <tbody><tr><td>B</td></tr></tbody>\
         <tfoot><tr><td>F</td></tr></tfoot>\
         </table>",
        400.0,
    );
    let rows = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableRow);
    assert!(rows.len() >= 3, "expected at least 3 rows");
    assert!(
        rows[0].layout.content_rect.y < rows[1].layout.content_rect.y,
        "first row must be above second"
    );
    assert!(
        rows[1].layout.content_rect.y < rows[2].layout.content_rect.y,
        "second row must be above third"
    );
}

#[test]
fn thead_tbody_tfoot_order() {
    // Full ordering: thead, then tbody, then tfoot regardless of source order
    let doc = load_html(
        "<table>\
         <tfoot><tr><td>F</td></tr></tfoot>\
         <thead><tr><td>H</td></tr></thead>\
         <tbody><tr><td>B</td></tr></tbody>\
         </table>",
        400.0,
    );
    let thead = find_box(&doc.root, &|b| b.tag == "thead");
    let tbody = find_box(&doc.root, &|b| b.tag == "tbody");
    let tfoot = find_box(&doc.root, &|b| b.tag == "tfoot");
    assert!(thead.is_some());
    assert!(tbody.is_some());
    assert!(tfoot.is_some());
    let thead_row = find_box(thead.unwrap(), &|b| b.style.display == Display::TableRow);
    let tbody_row = find_box(tbody.unwrap(), &|b| b.style.display == Display::TableRow);
    let tfoot_row = find_box(tfoot.unwrap(), &|b| b.style.display == Display::TableRow);
    assert!(thead_row.is_some());
    assert!(tbody_row.is_some());
    assert!(tfoot_row.is_some());
    assert!(
        thead_row.unwrap().layout.content_rect.y < tbody_row.unwrap().layout.content_rect.y,
        "thead must be above tbody"
    );
    assert!(
        tbody_row.unwrap().layout.content_rect.y < tfoot_row.unwrap().layout.content_rect.y,
        "tbody must be above tfoot"
    );
}

// ============================================================
// col/colgroup width attributes
// ============================================================

#[test]
fn col_width_attribute() {
    let doc = load_html(
        "<table>\
         <col width=\"100\"><col>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    let col1w = cells[0].layout.padding_rect.w;
    assert!(
        col1w >= 95.0 && col1w <= 105.0,
        "col width=100 should be close to 100px, got {}",
        col1w
    );
}

#[test]
fn col_width_css() {
    let doc = load_html(
        "<table>\
         <col style=\"width: 150px;\"><col>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    let col1w = cells[0].layout.padding_rect.w;
    assert!(
        col1w >= 145.0 && col1w <= 155.0,
        "col style width=150px should be close to 150px, got {}",
        col1w
    );
}

#[test]
fn colgroup_with_cols() {
    let doc = load_html(
        "<table>\
         <colgroup><col width=\"120\"><col></colgroup>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // Both cells should have positive width (basic layout sanity check)
    assert!(
        cells[0].layout.padding_rect.w > 0.0,
        "first cell should have positive width, got {}",
        cells[0].layout.padding_rect.w
    );
}

#[test]
fn col_width_percent() {
    let doc = load_html(
        "<table style=\"width: 400px;\">\
         <col width=\"50%\"><col>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        400.0,
    );
    let cells = find_all_boxes(&doc.root, &|b| b.style.display == Display::TableCell);
    assert!(cells.len() >= 2);
    // 50% of available space should be > 150px
    let col1w = cells[0].layout.padding_rect.w;
    assert!(col1w > 150.0, "50% column should be > 150px, got {}", col1w);
}
