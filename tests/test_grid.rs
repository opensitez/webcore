// Grid tests – ported from cpptests/test_grid.cpp
use webcore::css::apply_property;
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

// ============================================================
// CSS property parsing: display: grid
// ============================================================

#[test]
fn grid_display_parse() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "display", "grid");
    assert_eq!(style.display, Display::Grid);
}

// ============================================================
// grid-auto-flow
// ============================================================

#[test]
fn grid_auto_flow_row() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-flow", "row");
    assert_eq!(style.grid_auto_flow, GridAutoFlow::Row);
}

#[test]
fn grid_auto_flow_column() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-flow", "column");
    assert_eq!(style.grid_auto_flow, GridAutoFlow::Column);
}

#[test]
fn grid_auto_flow_row_dense() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-flow", "row dense");
    assert_eq!(style.grid_auto_flow, GridAutoFlow::RowDense);
}

#[test]
fn grid_auto_flow_column_dense() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-flow", "column dense");
    assert_eq!(style.grid_auto_flow, GridAutoFlow::ColumnDense);
}

// ============================================================
// grid-column-start / end / grid-row-start / end
// ============================================================

#[test]
fn grid_column_start_number() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-column-start", "2");
    assert_eq!(style.grid_column_start, 2);
}

#[test]
fn grid_column_end_number() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-column-end", "4");
    assert_eq!(style.grid_column_end, 4);
}

#[test]
fn grid_row_start_number() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-row-start", "1");
    assert_eq!(style.grid_row_start, 1);
}

#[test]
fn grid_column_span() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-column-start", "span 2");
    assert_eq!(style.grid_column_start, -10002);
}

// ============================================================
// grid-template-columns / grid-template-rows parsing
// ============================================================

#[test]
fn grid_template_columns_two_fixed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "100px 200px");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Fixed
    );
    assert!((style.rare().grid_template_columns[0].value - 100.0).abs() < 1.0);
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Fixed
    );
    assert!((style.rare().grid_template_columns[1].value - 200.0).abs() < 1.0);
}

#[test]
fn grid_template_columns_fr() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "1fr 2fr");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Fractional
    );
    assert!((style.rare().grid_template_columns[0].value - 1.0).abs() < 0.01);
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Fractional
    );
    assert!((style.rare().grid_template_columns[1].value - 2.0).abs() < 0.01);
}

#[test]
fn grid_template_rows_fixed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-rows", "50px 100px");
    assert_eq!(style.rare().grid_template_rows.len(), 2);
    assert_eq!(
        style.rare().grid_template_rows[0].kind,
        GridTrackKind::Fixed
    );
    assert!((style.rare().grid_template_rows[0].value - 50.0).abs() < 1.0);
}

#[test]
fn grid_template_columns_auto() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "auto auto");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Auto
    );
}

#[test]
fn grid_template_columns_repeat() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "repeat(3, 1fr)");
    assert_eq!(style.rare().grid_template_columns.len(), 3);
    for i in 0..3 {
        assert_eq!(
            style.rare().grid_template_columns[i].kind,
            GridTrackKind::Fractional
        );
    }
}

#[test]
fn grid_repeat_expands_correctly() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "repeat(4, 50px)");
    assert_eq!(style.rare().grid_template_columns.len(), 4);
    for i in 0..4 {
        assert_eq!(
            style.rare().grid_template_columns[i].kind,
            GridTrackKind::Fixed
        );
        assert!(
            style.rare().grid_template_columns[i].value > 49.0
                && style.rare().grid_template_columns[i].value < 51.0
        );
    }
}

#[test]
fn grid_repeat_mixed_tracks() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "grid-template-columns",
        "100px repeat(2, 1fr) 50px",
    );
    assert_eq!(style.rare().grid_template_columns.len(), 4);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Fixed
    );
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Fractional
    );
    assert_eq!(
        style.rare().grid_template_columns[2].kind,
        GridTrackKind::Fractional
    );
    assert_eq!(
        style.rare().grid_template_columns[3].kind,
        GridTrackKind::Fixed
    );
}

#[test]
fn grid_template_columns_minmax() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "grid-template-columns",
        "minmax(100px, 1fr) 200px",
    );
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MinMax
    );
    assert_eq!(
        style.rare().grid_template_columns[0].min_kind,
        GridTrackKind::Fixed
    );
    assert_eq!(
        style.rare().grid_template_columns[0].max_kind,
        GridTrackKind::Fractional
    );
}

#[test]
fn grid_minmax_with_percent_min() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "minmax(20%, 1fr)");
    assert_eq!(style.rare().grid_template_columns.len(), 1);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MinMax
    );
    assert_eq!(
        style.rare().grid_template_columns[0].min_kind,
        GridTrackKind::Percent
    );
    assert_eq!(
        style.rare().grid_template_columns[0].max_kind,
        GridTrackKind::Fractional
    );
}

#[test]
fn grid_minmax_with_auto_min() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "minmax(auto, 300px)");
    assert_eq!(style.rare().grid_template_columns.len(), 1);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MinMax
    );
    assert_eq!(
        style.rare().grid_template_columns[0].min_kind,
        GridTrackKind::Auto
    );
    assert_eq!(
        style.rare().grid_template_columns[0].max_kind,
        GridTrackKind::Fixed
    );
}

#[test]
fn grid_minmax_min_content_max() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "grid-template-columns",
        "minmax(min-content, 1fr)",
    );
    assert_eq!(style.rare().grid_template_columns.len(), 1);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MinMax
    );
    assert_eq!(
        style.rare().grid_template_columns[0].min_kind,
        GridTrackKind::MinContent
    );
    assert_eq!(
        style.rare().grid_template_columns[0].max_kind,
        GridTrackKind::Fractional
    );
}

// ============================================================
// Percentage track sizes
// ============================================================

#[test]
fn grid_percent_columns_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "display", "grid");
    apply_property(&mut style, "grid-template-columns", "25% 75%");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Percent
    );
    assert!(
        style.rare().grid_template_columns[0].value > 24.0
            && style.rare().grid_template_columns[0].value < 26.0
    );
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Percent
    );
}

#[test]
fn grid_percent_row_height() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-rows", "50% 50%");
    assert_eq!(style.rare().grid_template_rows.len(), 2);
    assert_eq!(
        style.rare().grid_template_rows[0].kind,
        GridTrackKind::Percent
    );
}

// ============================================================
// min-content / max-content / fit-content()
// ============================================================

#[test]
fn grid_min_content_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "min-content 1fr");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MinContent
    );
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Fractional
    );
}

#[test]
fn grid_max_content_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "max-content auto");
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::MaxContent
    );
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Auto
    );
}

#[test]
fn grid_fit_content_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "grid-template-columns",
        "fit-content(200px) 1fr",
    );
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::FitContent
    );
    assert!(
        style.rare().grid_template_columns[0].value > 199.0
            && style.rare().grid_template_columns[0].value < 201.0
    );
}

#[test]
fn grid_fit_content_percent() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "fit-content(50%)");
    assert_eq!(style.rare().grid_template_columns.len(), 1);
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::FitContent
    );
    assert_eq!(
        style.rare().grid_template_columns[0].max_kind,
        GridTrackKind::Percent
    );
    assert!(
        style.rare().grid_template_columns[0].value > 49.0
            && style.rare().grid_template_columns[0].value < 51.0
    );
}

// ============================================================
// grid-area / grid-template-areas
// ============================================================

#[test]
fn grid_area_named() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-area", "header");
    assert_eq!(style.grid_area, "header");
}

#[test]
fn grid_area_numeric_four_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-area", "1 / 2 / 3 / 4");
    assert_eq!(style.grid_row_start, 1);
    assert_eq!(style.grid_column_start, 2);
    assert_eq!(style.grid_row_end, 3);
    assert_eq!(style.grid_column_end, 4);
}

#[test]
fn grid_template_areas_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "grid-template-areas",
        "'header header' 'sidebar main' 'footer footer'",
    );
    assert_eq!(style.rare().grid_template_areas.len(), 3);
    assert_eq!(
        style.rare().grid_template_areas[0],
        vec!["header", "header"]
    );
    assert_eq!(style.rare().grid_template_areas[1], vec!["sidebar", "main"]);
    assert_eq!(
        style.rare().grid_template_areas[2],
        vec!["footer", "footer"]
    );
}

// ============================================================
// grid-column / grid-row shorthands
// ============================================================

#[test]
fn grid_column_shorthand_start_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-column", "2 / 4");
    assert_eq!(style.grid_column_start, 2);
    assert_eq!(style.grid_column_end, 4);
}

#[test]
fn grid_row_shorthand_start_span() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-row", "1 / span 2");
    assert_eq!(style.grid_row_start, 1);
    assert_eq!(style.grid_row_end, -10002);
}

#[test]
fn grid_column_span_parsing() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-column", "1 / span 3");
    assert_eq!(style.grid_column_start, 1);
    assert_eq!(style.grid_column_end, -10003);
}

// ============================================================
// grid-auto-rows / grid-auto-columns
// ============================================================

#[test]
fn grid_auto_rows_fixed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-rows", "60px");
    assert_eq!(style.grid_auto_rows.kind, GridTrackKind::Fixed);
    assert!((style.grid_auto_rows.value - 60.0).abs() < 1.0);
}

#[test]
fn grid_auto_columns_fixed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-columns", "120px");
    assert_eq!(style.grid_auto_columns.kind, GridTrackKind::Fixed);
    assert!((style.grid_auto_columns.value - 120.0).abs() < 1.0);
}

#[test]
fn grid_auto_rows_percent() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-rows", "25%");
    assert_eq!(style.grid_auto_rows.kind, GridTrackKind::Percent);
    assert!(style.grid_auto_rows.value > 24.0 && style.grid_auto_rows.value < 26.0);
}

#[test]
fn grid_auto_columns_percent() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-columns", "50%");
    assert_eq!(style.grid_auto_columns.kind, GridTrackKind::Percent);
}

#[test]
fn grid_auto_columns_min_content() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-columns", "min-content");
    assert_eq!(style.grid_auto_columns.kind, GridTrackKind::MinContent);
}

#[test]
fn grid_auto_rows_max_content() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-auto-rows", "max-content");
    assert_eq!(style.grid_auto_rows.kind, GridTrackKind::MaxContent);
}

// ============================================================
// grid-template / grid shorthands
// ============================================================

#[test]
fn grid_template_shorthand() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template", "100px 200px / 1fr 2fr");
    assert_eq!(style.rare().grid_template_rows.len(), 2);
    assert_eq!(style.rare().grid_template_columns.len(), 2);
    assert_eq!(
        style.rare().grid_template_rows[0].kind,
        GridTrackKind::Fixed
    );
    assert_eq!(
        style.rare().grid_template_columns[0].kind,
        GridTrackKind::Fractional
    );
    assert_eq!(
        style.rare().grid_template_columns[1].kind,
        GridTrackKind::Fractional
    );
}

#[test]
fn grid_template_none() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid-template-columns", "1fr 1fr");
    apply_property(&mut style, "grid-template", "none");
    assert_eq!(style.rare().grid_template_columns.len(), 0);
    assert_eq!(style.rare().grid_template_rows.len(), 0);
}

#[test]
fn grid_shorthand() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "grid", "auto / 1fr 1fr 1fr");
    assert_eq!(style.rare().grid_template_columns.len(), 3);
    assert_eq!(style.rare().grid_template_rows.len(), 1);
    assert_eq!(style.rare().grid_template_rows[0].kind, GridTrackKind::Auto);
}

// ============================================================
// auto-fill parsing
// ============================================================

#[test]
fn grid_auto_fill_parses_pattern() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "display", "grid");
    apply_property(
        &mut style,
        "grid-template-columns",
        "repeat(auto-fill, 150px)",
    );
    assert_eq!(style.rare().auto_repeat_columns.len(), 1);
    assert_eq!(
        style.rare().auto_repeat_columns[0].kind,
        GridTrackKind::Fixed
    );
    assert!(
        style.rare().auto_repeat_columns[0].value > 149.0
            && style.rare().auto_repeat_columns[0].value < 151.0
    );
    assert_eq!(style.rare().grid_template_columns.len(), 0); // no explicit columns
}

// ============================================================
// justify-items parsing
// ============================================================

#[test]
fn grid_justify_items_start() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-items", "start");
    assert_eq!(style.justify_items, AlignItems::FlexStart);
}

#[test]
fn grid_justify_items_center() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-items", "center");
    assert_eq!(style.justify_items, AlignItems::Center);
}

#[test]
fn grid_justify_items_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-items", "end");
    assert_eq!(style.justify_items, AlignItems::FlexEnd);
}

// ============================================================
// Layout tests: basic grid
// ============================================================

#[test]
fn grid_two_column_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr;\">\
           <div>A</div><div>B</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid);
    assert!(grid.is_some());
    let grid = grid.unwrap();
    assert!(grid.children.len() >= 2);
    let a = &grid.children[0];
    let b = &grid.children[1];
    // Should be side by side
    assert!(b.layout.margin_rect.x > a.layout.margin_rect.x);
    // Each roughly 400px
    assert!(a.layout.margin_rect.w > 350.0);
    assert!(b.layout.margin_rect.w > 350.0);
}

#[test]
fn grid_three_column_equal() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr;\">\
           <div>A</div><div>B</div><div>C</div>\
         </div>",
        900.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(grid.children.len() >= 3);
    let a = &grid.children[0];
    let b = &grid.children[1];
    let c = &grid.children[2];
    // Each ~300px
    assert!(a.layout.margin_rect.w > 250.0 && a.layout.margin_rect.w < 350.0);
    assert!(b.layout.margin_rect.w > 250.0 && b.layout.margin_rect.w < 350.0);
    assert!(c.layout.margin_rect.w > 250.0 && c.layout.margin_rect.w < 350.0);
}

#[test]
fn grid_fixed_plus_fr() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 200px 1fr;\">\
           <div>Fixed</div><div>Flex</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(grid.children.len() >= 2);
    let fixed = &grid.children[0];
    let flex = &grid.children[1];
    // Fixed ~200px
    assert!(fixed.layout.margin_rect.w > 190.0 && fixed.layout.margin_rect.w < 210.0);
    // Flex gets remainder ~400px
    assert!(flex.layout.margin_rect.w > 350.0);
}

#[test]
fn grid_two_by_two() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr;\">\
           <div>A</div><div>B</div><div>C</div><div>D</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(grid.children.len() >= 4);
    let a = &grid.children[0];
    let c = &grid.children[2];
    // A and C should be in different rows
    assert!(c.layout.margin_rect.y > a.layout.margin_rect.y);
    // Same column
    assert!((a.layout.margin_rect.x - c.layout.margin_rect.x).abs() < 5.0);
}

#[test]
fn grid_gap_property() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr; gap: 20px;\">\
           <div>A</div><div>B</div>\
         </div>",
        420.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let b = &grid.children[1];
    let gap = b.layout.margin_rect.x - (a.layout.margin_rect.x + a.layout.margin_rect.w);
    assert!(gap > 15.0 && gap < 25.0);
}

#[test]
fn grid_row_and_column_gap() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr; \
                      row-gap: 20px; column-gap: 10px;\">\
           <div>A</div><div>B</div>\
           <div>C</div><div>D</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let b = &grid.children[1];
    let c = &grid.children[2];
    // Column gap ~10px
    let col_gap = b.layout.margin_rect.x - (a.layout.margin_rect.x + a.layout.margin_rect.w);
    assert!(col_gap >= 8.0 && col_gap <= 12.0);
    // Row gap ~20px
    let row_gap = c.layout.margin_rect.y - (a.layout.margin_rect.y + a.layout.margin_rect.h);
    assert!(row_gap >= 18.0 && row_gap <= 22.0);
}

// ============================================================
// Column start/end layout
// ============================================================

#[test]
fn grid_column_start_end_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr;\">\
           <div style=\"grid-column-start: 2; grid-column-end: 4;\">Wide</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let wide = &grid.children[0];
    // Spans columns 2-3, so ~400px
    assert!(wide.layout.margin_rect.w > 350.0);
}

// ============================================================
// grid-template-rows layout
// ============================================================

#[test]
fn grid_template_rows_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; \
                      grid-template-rows: 50px 100px;\">\
           <div>A</div><div>B</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let elems: Vec<&WebCore> = grid.children.iter().filter(|c| c.tag != "#text").collect();
    let a = elems[0];
    let b = elems[1];
    // First row ~50px
    assert!(
        a.layout.margin_rect.h >= 45.0 && a.layout.margin_rect.h <= 55.0,
        "first row should be ~50px, got {}",
        a.layout.margin_rect.h
    );
    // Second row ~100px
    assert!(
        b.layout.margin_rect.h >= 95.0 && b.layout.margin_rect.h <= 105.0,
        "second row should be ~100px, got {}",
        b.layout.margin_rect.h
    );
}

// ============================================================
// grid-area layout
// ============================================================

#[test]
fn grid_area_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr; \
                      grid-template-areas: 'a a' 'b c';\">\
           <div style=\"grid-area: a;\">Header</div>\
           <div style=\"grid-area: b;\">Left</div>\
           <div style=\"grid-area: c;\">Right</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let header = &grid.children[0];
    let left = &grid.children[1];
    let right = &grid.children[2];
    // Header should span full width
    assert!(header.layout.margin_rect.w > 500.0);
    // Left and Right in same row, below header
    assert!(left.layout.margin_rect.y > header.layout.margin_rect.y);
    assert!((left.layout.margin_rect.y - right.layout.margin_rect.y).abs() < 5.0);
}

#[test]
fn grid_template_areas_complex() {
    let doc = load_html(
        "<div style=\"display: grid; \
                      grid-template-areas: 'header header header' 'sidebar main main' 'footer footer footer'; \
                      grid-template-columns: 100px 1fr 1fr; \
                      grid-template-rows: 50px 1fr 30px;\">\
           <div style=\"grid-area: header;\">H</div>\
           <div style=\"grid-area: sidebar;\">S</div>\
           <div style=\"grid-area: main;\">M</div>\
           <div style=\"grid-area: footer;\">F</div>\
         </div>", 600.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let header = &grid.children[0];
    let sidebar = &grid.children[1];
    let main = &grid.children[2];
    let footer = &grid.children[3];
    // Header spans full width
    assert!(header.layout.margin_rect.w > 500.0);
    // Sidebar narrower than main
    assert!(sidebar.layout.margin_rect.w < main.layout.margin_rect.w);
    // Footer below sidebar and main
    assert!(footer.layout.margin_rect.y > sidebar.layout.margin_rect.y);
    assert!(footer.layout.margin_rect.y > main.layout.margin_rect.y);
}

// ============================================================
// Percent columns layout
// ============================================================

#[test]
fn grid_percent_columns_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 50% 50%; width: 400px;\">\
           <div>A</div><div>B</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let b = &grid.children[1];
    assert!(a.layout.margin_rect.w >= 195.0 && a.layout.margin_rect.w <= 205.0);
    assert!(b.layout.margin_rect.w >= 195.0 && b.layout.margin_rect.w <= 205.0);
}

#[test]
fn grid_mixed_percent_and_fr() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 30% 1fr; width: 400px;\">\
           <div>A</div><div>B</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    // 30% of 400 = 120
    assert!(a.layout.margin_rect.w >= 115.0 && a.layout.margin_rect.w <= 125.0);
}

// ============================================================
// auto-fill / auto-fit layout
// ============================================================

#[test]
fn grid_auto_fill_repeat_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: repeat(auto-fill, 100px); width: 400px;\">\
           <div>A</div><div>B</div><div>C</div><div>D</div>\
         </div>", 400.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(grid.children.len() >= 4);
    let a = &grid.children[0];
    let d = &grid.children[3];
    // All 4 on same row
    assert!((a.layout.margin_rect.y - d.layout.margin_rect.y).abs() < 5.0);
}

#[test]
fn grid_auto_fill_repeat_overflow() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: repeat(auto-fill, 200px); width: 500px;\">\
           <div>A</div><div>B</div><div>C</div>\
         </div>", 500.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let c = &grid.children[2];
    // C wraps to next row
    assert!(c.layout.margin_rect.y > a.layout.margin_rect.y);
}

// ============================================================
// min-content layout
// ============================================================

#[test]
fn grid_min_content_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: min-content 1fr; width: 400px;\">\
           <div>Hi</div><div>World</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    // The fr column should get most space
    let b = &grid.children[1];
    assert!(b.layout.margin_rect.w > 200.0);
}

// ============================================================
// minmax row height
// ============================================================

#[test]
fn grid_minmax_row_height() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-rows: minmax(50px, 100px); width: 200px;\">\
           <div>Hi</div>\
         </div>",
        200.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let child = &grid.children[0];
    assert!(child.layout.margin_rect.h >= 50.0);
}

// ============================================================
// justify-items / justify-self layout
// ============================================================

#[test]
fn grid_justify_items_center_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; justify-items: center;\">\
           <div style=\"width: 200px;\">Center</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        // Centered in 800px column
        assert!(child.layout.margin_rect.x > 200.0);
        assert!(child.layout.margin_rect.x < 400.0);
    }
}

#[test]
fn grid_justify_items_end_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; justify-items: end;\">\
           <div style=\"width: 200px;\">End</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        assert!(child.layout.margin_rect.x > 500.0);
    }
}

#[test]
fn grid_justify_self_override() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; justify-items: start;\">\
           <div style=\"width: 200px; justify-self: end;\">End</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        assert!(child.layout.margin_rect.x > 500.0);
    }
}

// ============================================================
// align-items / align-self
// ============================================================

#[test]
fn grid_align_items_center() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; \
                      grid-template-rows: 200px; align-items: center;\">\
           <div>Short</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        assert!(child.layout.margin_rect.y > 50.0);
    }
}

#[test]
fn grid_align_self_end() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; \
                      grid-template-rows: 200px;\">\
           <div style=\"align-self: end;\">Bottom</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        let bottom = child.layout.margin_rect.y + child.layout.margin_rect.h;
        assert!(bottom >= 195.0);
    }
}

// ============================================================
// justify-content / align-content
// ============================================================

#[test]
fn grid_justify_content_center() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 200px 200px; justify-content: center;\">\
           <div>A</div><div>B</div>\
         </div>", 800.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let a = &grid.children[0];
        assert!(a.layout.content_rect.x > 150.0);
        assert!(a.layout.content_rect.x < 250.0);
    }
}

#[test]
fn grid_justify_content_space_between() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 200px 200px; justify-content: space-between;\">\
           <div>A</div><div>B</div>\
         </div>", 800.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 2 {
        let a = &grid.children[0];
        let b = &grid.children[1];
        assert!(a.layout.content_rect.x < 50.0);
        assert!(b.layout.content_rect.x > 500.0);
    }
}

#[test]
fn grid_space_evenly_justify_content() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 50px 50px; \
                      justify-content: space-evenly; width: 200px;\">\
           <div>A</div><div>B</div>\
         </div>",
        200.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let b = &grid.children[1];
    assert!(a.layout.margin_rect.x > 25.0);
    assert!(b.layout.margin_rect.x > a.layout.margin_rect.x + a.layout.margin_rect.w);
}

#[test]
fn grid_align_content_center() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; height: 400px; \
                      align-content: center; align-items: start;\">\
           <div>A</div><div>B</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert_eq!(grid.style.align_content, AlignContent::Center);
}

#[test]
fn grid_align_content_space_between() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; \
                      height: 200px; align-content: space-between;\">\
           <div>A</div><div>B</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let a = &grid.children[0];
    let b = &grid.children[1];
    assert!(b.layout.margin_rect.y > a.layout.margin_rect.h + 10.0);
}

// ============================================================
// place-items shorthand
// ============================================================

#[test]
fn grid_place_items_center() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; \
                      grid-template-rows: 200px; place-items: center center;\">\
           <div style=\"width: 100px;\">C</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 1 {
        let child = &grid.children[0];
        assert!(child.layout.margin_rect.x > 200.0);
        assert!(child.layout.margin_rect.y > 50.0);
    }
}

#[test]
fn grid_place_self_center() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 200px; grid-template-rows: 100px;\">\
           <div style=\"justify-self: center; align-self: center; width: 50px; height: 30px;\">X</div>\
         </div>", 400.0);
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let x = &grid.children[0];
    assert!(x.layout.margin_rect.x > 50.0);
    assert!(x.layout.margin_rect.y > 20.0);
}

// ============================================================
// Column auto-flow layout
// ============================================================

#[test]
fn grid_auto_flow_column_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-rows: 1fr 1fr; \
                      grid-template-columns: 1fr 1fr; grid-auto-flow: column;\">\
           <div>A</div><div>B</div><div>C</div><div>D</div>\
         </div>",
        800.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 4 {
        let a = &grid.children[0];
        let b = &grid.children[1];
        let c = &grid.children[2];
        // A and B in same column
        assert!((a.layout.content_rect.x - b.layout.content_rect.x).abs() < 5.0);
        // B below A
        assert!(b.layout.content_rect.y > a.layout.content_rect.y);
        // C in next column
        assert!(c.layout.content_rect.x > a.layout.content_rect.x);
    }
}

// ============================================================
// Dense / sparse packing
// ============================================================

#[test]
fn grid_dense_packing_fills_gaps() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr; \
                      grid-auto-flow: row dense;\">\
           <div style=\"grid-column: span 2;\">A</div>\
           <div style=\"grid-column: span 2;\">B</div>\
           <div>C</div>\
         </div>",
        900.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 3 {
        let a = &grid.children[0];
        let c = &grid.children[2];
        // With dense, C fills gap at row 0 col 2
        assert!((c.layout.content_rect.y - a.layout.content_rect.y).abs() < 5.0);
        assert!(c.layout.content_rect.x > a.layout.content_rect.x);
    }
}

#[test]
fn grid_sparse_packing_leaves_gaps() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr; \
                      grid-auto-flow: row;\">\
           <div style=\"grid-column: span 2;\">A</div>\
           <div style=\"grid-column: span 2;\">B</div>\
           <div>C</div>\
         </div>",
        900.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 3 {
        let b = &grid.children[1];
        let c = &grid.children[2];
        // Without dense, C goes to same row as B
        assert!((c.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0);
    }
}

#[test]
fn grid_explicit_items_dont_overlap_auto() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr;\">\
           <div>A</div>\
           <div style=\"grid-column: 2; grid-row: 1;\">Explicit</div>\
           <div>C</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    if grid.children.len() >= 3 {
        let a = &grid.children[0];
        let expl = &grid.children[1];
        let c = &grid.children[2];
        assert!(a.layout.content_rect.y < c.layout.content_rect.y);
        assert!((a.layout.content_rect.x - c.layout.content_rect.x).abs() < 5.0);
        assert!(expl.layout.content_rect.x > a.layout.content_rect.x);
    }
}

// ============================================================
// Order property
// ============================================================

#[test]
fn grid_order_property() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr;\">\
           <div style=\"order: 2;\">Second</div>\
           <div style=\"order: 1;\">First</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let dom_first = &grid.children[0]; // order: 2
    let dom_second = &grid.children[1]; // order: 1
    assert!(dom_second.layout.margin_rect.x < dom_first.layout.margin_rect.x);
}

#[test]
fn grid_order_default_zero() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr;\">\
           <div>A</div>\
           <div style=\"order: -1;\">B</div>\
           <div>C</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let b = &grid.children[1]; // order: -1
    let a = &grid.children[0]; // order: 0
    assert!(b.layout.margin_rect.x < a.layout.margin_rect.x);
}

// ============================================================
// grid-auto-rows layout
// ============================================================

#[test]
fn grid_auto_rows_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr; grid-auto-rows: 80px;\">\
           <div>A</div><div>B</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let b = &grid.children[1];
    assert!(b.layout.margin_rect.h >= 80.0);
}

// ============================================================
// Inline-grid
// ============================================================

#[test]
fn grid_inline_grid_display() {
    let doc = load_html(
        "<span style=\"display: inline-grid; grid-template-columns: 50px 50px;\">\
           <span>A</span><span>B</span>\
         </span>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::InlineGrid);
    assert!(grid.is_some());
}

// ============================================================
// Column span with auto-place
// ============================================================

#[test]
fn grid_col_span_with_auto_place() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr 1fr;\">\
           <div style=\"grid-column: span 2;\">Wide</div>\
           <div>B</div>\
         </div>",
        600.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let wide = &grid.children[0];
    let b = &grid.children[1];
    assert!(wide.layout.margin_rect.w > 350.0);
    assert!(b.layout.margin_rect.w < 250.0);
}

#[test]
fn grid_row_span_with_explicit() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr; \
                      grid-template-rows: 50px 50px;\">\
           <div style=\"grid-row: 1 / 3;\">Tall</div>\
           <div>B</div><div>C</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let tall = &grid.children[0];
    assert!(tall.layout.margin_rect.h >= 100.0);
}

// ============================================================
// Auto-flow column dense layout
// ============================================================

#[test]
fn grid_auto_flow_column_dense_layout() {
    let doc = load_html(
        "<div style=\"display: grid; grid-template-columns: 1fr 1fr; \
                      grid-template-rows: 50px 50px; grid-auto-flow: column dense;\">\
           <div style=\"grid-row: span 2;\">Tall</div>\
           <div>B</div><div>C</div>\
         </div>",
        400.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let tall = &grid.children[0];
    let b = &grid.children[1];
    assert!(b.layout.margin_rect.x > tall.layout.margin_rect.x);
}

#[test]
fn grid_cell_stretch_border_box() {
    // Regression: with box-sizing: border-box, align-self: stretch was
    // double-subtracting padding/border from the cell height, making content_h = 0.
    use webcore::load_html;
    use webcore::types::{Display, WebCore};

    fn find_grid<'a>(root: &'a WebCore) -> Option<&'a WebCore> {
        if root.style.display == Display::Grid {
            return Some(root);
        }
        for c in &root.children {
            if let Some(g) = find_grid(c) {
                return Some(g);
            }
        }
        None
    }

    // Without box-sizing: border-box — baseline
    let doc = load_html(
        r#"<html><head></head><body>
<div style="display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 10px;">
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 1</div>
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 2</div>
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 3</div>
</div></body></html>"#,
        900.0,
    );
    let grid = find_grid(&doc.root).unwrap();
    let cell = grid.children.iter().find(|c| c.tag == "div").unwrap();
    let h_no_bb = cell.layout.padding_rect.h;

    // With box-sizing: border-box globally (as in demo.html via * { box-sizing: border-box })
    let doc = load_html(
        r#"<html><head><style>* { box-sizing: border-box; }</style></head><body>
<div style="display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 10px;">
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 1</div>
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 2</div>
  <div style="padding: 10px; border: 1px solid #aaa;">Cell 3</div>
</div></body></html>"#,
        900.0,
    );
    let grid = find_grid(&doc.root).unwrap();
    let cell = grid.children.iter().find(|c| c.tag == "div").unwrap();
    let h_bb = cell.layout.padding_rect.h;

    // Both should produce the same cell height — border-box shouldn't shrink cells to padding-only
    assert!(
        (h_bb - h_no_bb).abs() < 1.0,
        "border-box cell height {h_bb} should match content-box height {h_no_bb}"
    );
    // And the height should include the text content (not just padding)
    assert!(
        h_bb > 30.0,
        "cell should be taller than just padding (got {h_bb})"
    );
}
