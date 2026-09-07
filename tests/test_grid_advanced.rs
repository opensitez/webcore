// Comprehensive grid layout tests — covers advanced features, edge cases,
// and real-world patterns from BBC, AP News, Wikipedia, Al Jazeera.

use webcore::load_html;
use webcore::types::*;

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

fn find_all<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut result = Vec::new();
    if pred(root) {
        result.push(root);
    }
    for child in &root.children {
        result.extend(find_all(child, pred));
    }
    result
}

fn by_id<'a>(root: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    find_box(root, &|b| {
        b.attributes.get("id").map(|v| v == id).unwrap_or(false)
    })
}

// ============================================================
// grid-template-columns: repeat(N, minmax(0, 1fr))
// (BBC pattern)
// ============================================================

#[test]
fn grid_repeat_minmax_0_1fr() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:repeat(4, minmax(0, 1fr)); width:1200px; gap:16px'>",
        "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div><div id='d'>D</div>",
        "</div>",
    ), 1280.0);
    let a = by_id(&doc.root, "a").unwrap();
    let d = by_id(&doc.root, "d").unwrap();
    // 4 equal columns: (1200 - 3*16) / 4 = 288px each
    let expected = (1200.0 - 48.0) / 4.0;
    assert!(
        (a.layout.content_rect.w - expected).abs() < 2.0,
        "column width {:.1} should be ~{:.1}",
        a.layout.content_rect.w,
        expected
    );
    assert!(
        d.layout.content_rect.x > a.layout.content_rect.x + 200.0,
        "d should be right of a"
    );
}

// ============================================================
// grid-template-areas with spanning
// (BBC pattern: hero spans 2 cols)
// ============================================================

#[test]
fn grid_template_areas_spanning() {
    let doc = load_html(
        concat!(
            "<style>",
            ".grid { display:grid; width:1200px; gap:16px;",
            "  grid-template-columns: repeat(4, minmax(0,1fr));",
            "  grid-template-areas: 'hero hero side1 side2' 'hero hero side3 side4'; }",
            ".grid > :nth-child(1) { grid-area: hero; }",
            ".grid > :nth-child(2) { grid-area: side1; }",
            ".grid > :nth-child(3) { grid-area: side2; }",
            ".grid > :nth-child(4) { grid-area: side3; }",
            ".grid > :nth-child(5) { grid-area: side4; }",
            "</style>",
            "<div class='grid'>",
            "<div id='hero'>Hero</div><div id='s1'>S1</div><div id='s2'>S2</div>",
            "<div id='s3'>S3</div><div id='s4'>S4</div></div>",
        ),
        1280.0,
    );
    let hero = by_id(&doc.root, "hero").unwrap();
    let s1 = by_id(&doc.root, "s1").unwrap();
    // Hero spans 2 columns
    assert!(
        hero.layout.content_rect.w > s1.layout.content_rect.w * 1.8,
        "hero w={:.0} should be ~2x side w={:.0}",
        hero.layout.content_rect.w,
        s1.layout.content_rect.w
    );
    // Hero spans 2 rows
    assert!(
        hero.layout.content_rect.h > s1.layout.content_rect.h * 1.5,
        "hero h={:.0} should span 2 rows vs side h={:.0}",
        hero.layout.content_rect.h,
        s1.layout.content_rect.h
    );
}

// ============================================================
// fit-content() column sizing
// ============================================================

#[test]
fn grid_fit_content_column() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:fit-content(200px) 1fr; width:800px; gap:10px'>",
        "<div id='label'>Short</div><div id='content'>Main content area</div>",
        "</div>",
    ), 1000.0);
    let label = by_id(&doc.root, "label").unwrap();
    let content = by_id(&doc.root, "content").unwrap();
    // fit-content(200px): label should be content-width, max 200px
    assert!(
        label.layout.content_rect.w <= 205.0,
        "fit-content label w={:.0} should be <= 200",
        label.layout.content_rect.w
    );
    assert!(
        label.layout.content_rect.w > 10.0,
        "fit-content label should have some width"
    );
    // Content fills the rest
    assert!(
        content.layout.content_rect.w > 500.0,
        "1fr content w={:.0} should fill remaining",
        content.layout.content_rect.w
    );
}

#[test]
fn grid_fit_content_percent() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:fit-content(25%) auto; width:1000px; gap:10px'>",
        "<div id='a'>Label</div><div id='b'>Content</div>",
        "</div>",
    ), 1100.0);
    let a = by_id(&doc.root, "a").unwrap();
    // fit-content(25%) of 1000px = max 250px
    assert!(
        a.layout.content_rect.w <= 255.0,
        "fit-content(25%%) w={:.0} should be <= 250",
        a.layout.content_rect.w
    );
}

// ============================================================
// @supports (display: grid) override
// (BBC pattern)
// ============================================================

#[test]
fn supports_grid_overrides_flex_fallback() {
    let doc = load_html(
        concat!(
            "<style>",
            ".items { display: flex; flex-wrap: wrap; width: 1000px; }",
            ".items > * { width: calc(100% / 3); }",
            "@supports (display: grid) {",
            "  .items { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }",
            "  .items > * { width: initial; }",
            "}",
            "</style>",
            "<div class='items'>",
            "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div>",
            "</div>",
        ),
        1100.0,
    );
    let container = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "items")
            .unwrap_or(false)
    })
    .unwrap();
    assert_eq!(
        container.style.display,
        Display::Grid,
        "should be Grid from @supports"
    );
    let a = by_id(&doc.root, "a").unwrap();
    // (1000 - 2*10) / 3 ≈ 326.7
    assert!(
        a.layout.content_rect.w > 300.0,
        "grid item w={:.0} should be ~326 (1/3 of grid)",
        a.layout.content_rect.w
    );
}

// ============================================================
// CSS initial/unset properly resets properties
// ============================================================

#[test]
fn css_initial_resets_width_to_auto() {
    let doc = load_html(
        concat!(
            "<style>",
            ".box { width: 200px; }",
            ".reset { width: initial; }",
            "</style>",
            "<div class='box reset' style='height:50px'>Content</div>",
        ),
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c.contains("reset"))
            .unwrap_or(false)
    })
    .unwrap();
    // width:initial should reset to auto (fill container)
    assert!(
        b.layout.content_rect.w > 400.0,
        "width:initial should be auto (full width), got {:.0}",
        b.layout.content_rect.w
    );
}

#[test]
fn css_unset_resets_non_inherited() {
    let doc = load_html(
        concat!(
            "<style>",
            ".box { background-color: red; }",
            ".clear { background-color: unset; }",
            "</style>",
            "<div class='box clear'>Content</div>",
        ),
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c.contains("clear"))
            .unwrap_or(false)
    })
    .unwrap();
    // background-color:unset = initial for non-inherited = transparent
    assert_eq!(
        b.style.background_color.a, 0,
        "background-color:unset should be transparent"
    );
}

// ============================================================
// Grid auto-placement with explicit spans
// ============================================================

#[test]
fn grid_auto_place_with_col_span() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:repeat(3, 1fr); width:900px; gap:10px'>",
        "<div id='wide' style='grid-column: span 2'>Wide</div>",
        "<div id='normal'>Normal</div>",
        "<div id='another'>Another</div>",
        "</div>",
    ),
        1000.0,
    );
    let wide = by_id(&doc.root, "wide").unwrap();
    let normal = by_id(&doc.root, "normal").unwrap();
    // Wide spans 2 cols: ~586px, Normal is ~293px
    assert!(
        wide.layout.content_rect.w > normal.layout.content_rect.w * 1.8,
        "span 2 w={:.0} should be ~2x normal w={:.0}",
        wide.layout.content_rect.w,
        normal.layout.content_rect.w
    );
}

#[test]
fn grid_auto_place_with_row_span() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px; gap:10px'>",
            "<div id='tall' style='grid-row: span 2'>Tall</div>",
            "<div id='a'>A</div>",
            "<div id='b'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let tall = by_id(&doc.root, "tall").unwrap();
    let a = by_id(&doc.root, "a").unwrap();
    // Tall spans 2 rows, should be taller than A
    assert!(
        tall.layout.content_rect.h >= a.layout.content_rect.h * 1.5,
        "row span 2 h={:.0} should be >= 1.5x normal h={:.0}",
        tall.layout.content_rect.h,
        a.layout.content_rect.h
    );
}

// ============================================================
// Grid with named lines
// ============================================================

#[test]
fn grid_named_line_placement() {
    let doc = load_html(
        concat!(
            "<style>",
            ".grid { display:grid; width:900px;",
            "  grid-template-columns: [start] 1fr [mid] 1fr [end]; }",
            "#a { grid-column: start / mid; }",
            "#b { grid-column: mid / end; }",
            "</style>",
            "<div class='grid'><div id='a'>Left</div><div id='b'>Right</div></div>",
        ),
        1000.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Both should be ~450px
    assert!(
        (a.layout.content_rect.w - 450.0).abs() < 10.0,
        "a w={:.0} should be ~450",
        a.layout.content_rect.w
    );
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x,
        "b should be right of a"
    );
}

// ============================================================
// Grid items: no overflow beyond container
// ============================================================

#[test]
fn grid_items_within_container_bounds() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:repeat(3, 1fr); width:900px; gap:10px'>",
        "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div>",
        "<div id='d'>D</div><div id='e'>E</div><div id='f'>F</div>",
        "</div>",
    ),
        1000.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let grid_right = grid.layout.content_rect.x + grid.layout.content_rect.w;
    let grid_left = grid.layout.content_rect.x;
    for id in &["a", "b", "c", "d", "e", "f"] {
        let item = by_id(&doc.root, id).unwrap();
        let right = item.layout.content_rect.x + item.layout.content_rect.w;
        assert!(
            right <= grid_right + 1.0,
            "item {} right {:.0} overflows grid right {:.0}",
            id,
            right,
            grid_right
        );
        assert!(
            item.layout.content_rect.x >= grid_left - 1.0,
            "item {} left {:.0} before grid left {:.0}",
            id,
            item.layout.content_rect.x,
            grid_left
        );
    }
}

// ============================================================
// Grid: auto-fill creates appropriate number of columns
// ============================================================

#[test]
fn grid_auto_fill_column_count() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:repeat(auto-fill, minmax(200px, 1fr)); width:1000px; gap:10px'>",
        "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div>",
        "<div id='d'>D</div><div id='e'>E</div>",
        "</div>",
    ), 1100.0);
    let a = by_id(&doc.root, "a").unwrap();
    let d = by_id(&doc.root, "d").unwrap();
    // 1000px / 200px = 5 columns max, but with gaps (4*10=40) → 4 columns fit
    // Items a-d on row 1, e on row 2
    assert!(
        (a.layout.content_rect.y - d.layout.content_rect.y).abs() < 2.0,
        "a and d should be on same row"
    );
    let e = by_id(&doc.root, "e").unwrap();
    assert!(
        e.layout.content_rect.y > a.layout.content_rect.y + 5.0,
        "e should be on second row"
    );
}

// ============================================================
// Grid: percentage heights
// ============================================================

#[test]
fn grid_percentage_row_height() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-rows:50% 50%; height:400px; width:600px'>",
            "<div id='top'>Top</div><div id='bottom'>Bottom</div>",
            "</div>",
        ),
        700.0,
    );
    let top = by_id(&doc.root, "top").unwrap();
    let bottom = by_id(&doc.root, "bottom").unwrap();
    assert!(
        (top.layout.content_rect.h - 200.0).abs() < 5.0,
        "top h={:.0} should be ~200 (50%% of 400)",
        top.layout.content_rect.h
    );
    assert!(
        bottom.layout.content_rect.y >= top.layout.content_rect.y + 195.0,
        "bottom should start after top"
    );
}

// ============================================================
// Grid: align-content / justify-content
// ============================================================

#[test]
fn grid_justify_content_space_around() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:100px 100px; width:500px; justify-content:space-around'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 600.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Space around: each col gets equal space on both sides
    // Total space = 500 - 200 = 300, distributed as 75px | 100px | 150px | 100px | 75px
    assert!(a.layout.content_rect.x > 50.0, "a should have left space");
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 150.0,
        "b should have space between"
    );
}

// ============================================================
// Grid: subgrid basics (if supported)
// ============================================================

#[test]
fn grid_nested_inherits_parent_columns() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr 1fr; width:900px; gap:10px'>",
            "<div id='span' style='grid-column: 1/3; display:grid; grid-template-columns:1fr 1fr'>",
            "  <div id='inner1'>I1</div><div id='inner2'>I2</div>",
            "</div>",
            "<div id='c'>C</div>",
            "</div>",
        ),
        1000.0,
    );
    let span = by_id(&doc.root, "span").unwrap();
    let c = by_id(&doc.root, "c").unwrap();
    // Span covers 2 cols of outer grid
    assert!(
        span.layout.content_rect.w > c.layout.content_rect.w * 1.8,
        "span w={:.0} should be ~2x c w={:.0}",
        span.layout.content_rect.w,
        c.layout.content_rect.w
    );
    // Inner items should split the span evenly
    let i1 = by_id(&doc.root, "inner1").unwrap();
    let i2 = by_id(&doc.root, "inner2").unwrap();
    assert!(
        (i1.layout.content_rect.w - i2.layout.content_rect.w).abs() < 5.0,
        "inner items should be equal width"
    );
}

// ============================================================
// Grid: implicit rows/columns
// ============================================================

#[test]
fn grid_implicit_columns_from_placement() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px; gap:10px'>",
            "<div id='a'>A</div>",
            "<div id='b'>B</div>",
            "<div id='c' style='grid-column:3'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let c = by_id(&doc.root, "c").unwrap();
    // C is placed in implicit column 3
    let a = by_id(&doc.root, "a").unwrap();
    assert!(
        c.layout.content_rect.x > a.layout.content_rect.x + 200.0,
        "c (col 3) should be right of a (col 1)"
    );
}

// ============================================================
// Grid: min/max constraints
// ============================================================

#[test]
fn grid_minmax_clamps_column() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:minmax(100px, 300px) 1fr; width:800px'>",
        "<div id='clamped'>Clamped</div><div id='flex'>Flex</div>",
        "</div>",
    ),
        900.0,
    );
    let clamped = by_id(&doc.root, "clamped").unwrap();
    assert!(
        clamped.layout.content_rect.w >= 95.0 && clamped.layout.content_rect.w <= 305.0,
        "minmax(100,300) w={:.0} should be between 100 and 300",
        clamped.layout.content_rect.w
    );
}

// ============================================================
// Grid: order property
// ============================================================

#[test]
fn grid_order_reorders_visually() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr 1fr; width:600px'>",
            "<div id='a' style='order:3'>A</div>",
            "<div id='b' style='order:1'>B</div>",
            "<div id='c' style='order:2'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    let c = by_id(&doc.root, "c").unwrap();
    // Visual order: B(1) C(2) A(3)
    assert!(
        b.layout.content_rect.x < c.layout.content_rect.x,
        "B before C"
    );
    assert!(
        c.layout.content_rect.x < a.layout.content_rect.x,
        "C before A"
    );
}

// ============================================================
// Grid: empty grid doesn't crash
// ============================================================

#[test]
fn grid_empty_no_crash() {
    let doc = load_html(
        "<div style='display:grid; grid-template-columns:1fr 1fr; width:400px'></div>",
        500.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(
        grid.layout.content_rect.h >= 0.0,
        "empty grid should have non-negative height"
    );
}

// ============================================================
// Grid: single item
// ============================================================

#[test]
fn grid_single_item_fills_column() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr; width:500px'>",
            "<div id='only'>Only</div>",
            "</div>",
        ),
        600.0,
    );
    let only = by_id(&doc.root, "only").unwrap();
    assert!(
        (only.layout.content_rect.w - 500.0).abs() < 5.0,
        "single item w={:.0} should fill grid",
        only.layout.content_rect.w
    );
}

// ============================================================
// Grid: many items wrap to new rows
// ============================================================

#[test]
fn grid_items_wrap_to_rows() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:repeat(3, 1fr); width:900px; gap:10px'>",
        "<div>1</div><div>2</div><div>3</div>",
        "<div>4</div><div>5</div><div>6</div>",
        "<div id='seven'>7</div>",
        "</div>",
    ),
        1000.0,
    );
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    let seven = by_id(&doc.root, "seven").unwrap();
    // Item 7 should be on row 3
    assert!(
        seven.layout.content_rect.y > grid.layout.content_rect.y + 20.0,
        "item 7 should be on a later row"
    );
}

// ============================================================
// Grid: align-items stretch (default)
// ============================================================

#[test]
fn grid_align_items_stretch_default() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px'>",
            "<div id='short'>Short</div>",
            "<div id='tall' style='height:100px'>Tall</div>",
            "</div>",
        ),
        700.0,
    );
    let short = by_id(&doc.root, "short").unwrap();
    let tall = by_id(&doc.root, "tall").unwrap();
    // With stretch, short should match tall's row height
    assert!(
        (short.layout.content_rect.h - tall.layout.content_rect.h).abs() < 5.0,
        "stretch: short h={:.0} should match tall h={:.0}",
        short.layout.content_rect.h,
        tall.layout.content_rect.h
    );
}

// ============================================================
// Grid: 12-column pattern (Bootstrap/AP News)
// ============================================================

#[test]
fn grid_12_column_bootstrap_pattern() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:repeat(12, 1fr); width:1140px; gap:15px'>",
        "<div id='full' style='grid-column: span 12'>Full</div>",
        "<div id='half1' style='grid-column: span 6'>Half1</div>",
        "<div id='half2' style='grid-column: span 6'>Half2</div>",
        "<div id='third1' style='grid-column: span 4'>T1</div>",
        "<div id='third2' style='grid-column: span 4'>T2</div>",
        "<div id='third3' style='grid-column: span 4'>T3</div>",
        "</div>",
    ),
        1200.0,
    );
    let full = by_id(&doc.root, "full").unwrap();
    let half1 = by_id(&doc.root, "half1").unwrap();
    let third1 = by_id(&doc.root, "third1").unwrap();
    // Full width = 1140px
    assert!(
        (full.layout.content_rect.w - 1140.0).abs() < 5.0,
        "span 12 w={:.0} should be ~1140",
        full.layout.content_rect.w
    );
    // Half ≈ 562
    assert!(
        half1.layout.content_rect.w > 500.0 && half1.layout.content_rect.w < 600.0,
        "span 6 w={:.0} should be ~562",
        half1.layout.content_rect.w
    );
    // Third ≈ 370
    assert!(
        third1.layout.content_rect.w > 330.0 && third1.layout.content_rect.w < 400.0,
        "span 4 w={:.0} should be ~370",
        third1.layout.content_rect.w
    );
}

// ============================================================
// Grid: negative line numbers
// ============================================================

#[test]
fn grid_negative_line_number() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr 1fr; width:900px'>",
            "<div id='last' style='grid-column: -2 / -1'>Last col</div>",
            "</div>",
        ),
        1000.0,
    );
    let last = by_id(&doc.root, "last").unwrap();
    // -1 is the end line, -2 is one before end → last column
    assert!(
        last.layout.content_rect.x > 550.0,
        "grid-column:-2/-1 should be in the last column, x={:.0}",
        last.layout.content_rect.x
    );
}

// ============================================================
// Grid: gap only (no explicit columns)
// ============================================================

#[test]
fn grid_gap_with_auto_columns() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; gap:20px; width:500px'>",
            "<div id='a'>Row 1</div>",
            "<div id='b'>Row 2</div>",
            "</div>",
        ),
        600.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // With gap:20px, rows should be 20px apart
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    assert!(
        (gap - 20.0).abs() < 3.0,
        "row gap should be 20px, got {:.0}",
        gap
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID-TEMPLATE-ROWS: advanced sizing                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_template_rows_fr_units() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-rows:1fr 2fr; height:300px; width:400px'>",
            "<div id='r1'>Row1</div><div id='r2'>Row2</div>",
            "</div>",
        ),
        500.0,
    );
    let r1 = by_id(&doc.root, "r1").unwrap();
    let r2 = by_id(&doc.root, "r2").unwrap();
    // 1fr=100px, 2fr=200px
    assert!(
        (r1.layout.content_rect.h - 100.0).abs() < 5.0,
        "1fr row h={:.0} should be ~100",
        r1.layout.content_rect.h
    );
    assert!(
        (r2.layout.content_rect.h - 200.0).abs() < 5.0,
        "2fr row h={:.0} should be ~200",
        r2.layout.content_rect.h
    );
}

#[test]
fn grid_template_rows_repeat() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-rows:repeat(3, 80px); width:400px'>",
            "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div>",
            "</div>",
        ),
        500.0,
    );
    for id in &["a", "b", "c"] {
        let item = by_id(&doc.root, id).unwrap();
        assert!(
            (item.layout.content_rect.h - 80.0).abs() < 5.0,
            "{} row h={:.0} should be 80",
            id,
            item.layout.content_rect.h
        );
    }
}

#[test]
fn grid_template_rows_minmax() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-rows:minmax(50px, auto); width:400px'>",
            "<div id='short'>X</div>",
            "</div>",
        ),
        500.0,
    );
    let s = by_id(&doc.root, "short").unwrap();
    // minmax(50px, auto): content is short, but min is 50px
    assert!(
        s.layout.content_rect.h >= 48.0,
        "minmax(50,auto) h={:.0} should be >= 50",
        s.layout.content_rect.h
    );
}

#[test]
fn grid_template_rows_mixed_fixed_fr() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-rows:60px 1fr; height:300px; width:400px'>",
            "<div id='fixed'>Header</div><div id='flex'>Content</div>",
            "</div>",
        ),
        500.0,
    );
    let fixed = by_id(&doc.root, "fixed").unwrap();
    let flex = by_id(&doc.root, "flex").unwrap();
    assert!(
        (fixed.layout.content_rect.h - 60.0).abs() < 5.0,
        "fixed row h={:.0} should be 60",
        fixed.layout.content_rect.h
    );
    assert!(
        (flex.layout.content_rect.h - 240.0).abs() < 5.0,
        "1fr row h={:.0} should be ~240",
        flex.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  AUTO-FIT vs AUTO-FILL                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_auto_fit_collapses_empty_tracks() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:repeat(auto-fit, minmax(100px, 1fr)); width:500px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 600.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // auto-fit: 5 possible columns, but only 2 items → empty tracks collapse
    // Items should expand to fill: 500/2 = 250px each
    assert!(
        a.layout.content_rect.w > 200.0,
        "auto-fit item w={:.0} should expand (>200)",
        a.layout.content_rect.w
    );
    assert!(
        (a.layout.content_rect.w - b.layout.content_rect.w).abs() < 5.0,
        "both items should be equal width"
    );
}

#[test]
fn grid_auto_fill_keeps_empty_tracks() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:repeat(auto-fill, minmax(100px, 1fr)); width:500px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 600.0);
    let a = by_id(&doc.root, "a").unwrap();
    // auto-fill: 5 columns of 100px, items stay at 100px (empty tracks remain)
    // Each column ≈ 100px (may stretch slightly with 1fr max)
    assert!(
        a.layout.content_rect.w < 200.0,
        "auto-fill item w={:.0} should NOT expand beyond column size",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID-AUTO-ROWS/COLUMNS with minmax                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_auto_rows_minmax() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:1fr; grid-auto-rows:minmax(40px, auto); width:400px'>",
        "<div id='short'>X</div>",
        "<div id='tall' style='height:200px'>Tall</div>",
        "</div>",
    ), 500.0);
    let short = by_id(&doc.root, "short").unwrap();
    let tall = by_id(&doc.root, "tall").unwrap();
    assert!(
        short.layout.content_rect.h >= 38.0,
        "auto-rows minmax(40,auto): short h={:.0} should be >= 40",
        short.layout.content_rect.h
    );
    assert!(
        tall.layout.content_rect.h >= 195.0,
        "auto-rows minmax(40,auto): tall h={:.0} should be >= 200",
        tall.layout.content_rect.h
    );
}

#[test]
fn grid_auto_columns_with_placement() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:100px; grid-auto-columns:150px; width:600px'>",
        "<div id='a'>A</div>",
        "<div id='b' style='grid-column:2'>B</div>",
        "<div id='c' style='grid-column:3'>C</div>",
        "</div>",
    ), 700.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Column 1 = 100px (explicit), columns 2-3 = 150px (auto)
    assert!(
        (a.layout.content_rect.w - 100.0).abs() < 5.0,
        "col 1 w={:.0} should be 100",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 150.0).abs() < 5.0,
        "auto col w={:.0} should be 150",
        b.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  MIXED UNITS: fixed + fr + %                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_mixed_fixed_fr_percent_columns() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:200px 1fr 25%; width:1000px'>",
            "<div id='fixed'>Fixed</div><div id='fr'>Fr</div><div id='pct'>Pct</div>",
            "</div>",
        ),
        1100.0,
    );
    let fixed = by_id(&doc.root, "fixed").unwrap();
    let fr = by_id(&doc.root, "fr").unwrap();
    let pct = by_id(&doc.root, "pct").unwrap();
    assert!(
        (fixed.layout.content_rect.w - 200.0).abs() < 5.0,
        "fixed={:.0}",
        fixed.layout.content_rect.w
    );
    assert!(
        (pct.layout.content_rect.w - 250.0).abs() < 5.0,
        "25%%={:.0}",
        pct.layout.content_rect.w
    );
    // fr gets remainder: 1000 - 200 - 250 = 550
    assert!(
        (fr.layout.content_rect.w - 550.0).abs() < 5.0,
        "fr={:.0}",
        fr.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID CONTAINER: padding, border, box-sizing                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_container_with_padding() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:500px; padding:20px'>",
            "<div id='a'>A</div><div id='b'>B</div>",
            "</div>",
        ),
        600.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    // Content area = 500px, each col = 250px
    assert!(
        (a.layout.content_rect.w - 250.0).abs() < 5.0,
        "col w={:.0} should be 250",
        a.layout.content_rect.w
    );
    // Items should be inset by padding
    assert!(
        a.layout.content_rect.x >= grid.layout.border_rect.x + 18.0,
        "items should be inside padding"
    );
}

#[test]
fn grid_container_border_box() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:1fr 1fr; width:500px; padding:20px; box-sizing:border-box'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 600.0);
    let a = by_id(&doc.root, "a").unwrap();
    // border-box: 500px includes padding. Content = 500 - 40 = 460. Each col = 230px
    assert!(
        (a.layout.content_rect.w - 230.0).abs() < 5.0,
        "border-box col w={:.0} should be ~230",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID ITEMS: margin, min/max width, explicit width          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_item_margins() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr; width:400px'>",
            "<div id='m' style='margin:10px 20px'>Margined</div>",
            "</div>",
        ),
        500.0,
    );
    let m = by_id(&doc.root, "m").unwrap();
    // Content width = 400 - 40 (left+right margin) = 360
    assert!(
        (m.layout.content_rect.w - 360.0).abs() < 5.0,
        "item w={:.0} should be 360 (400 - 2*20)",
        m.layout.content_rect.w
    );
}

#[test]
fn grid_item_explicit_width_in_stretch() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr; width:400px'>",
            "<div id='narrow' style='width:200px'>Narrow</div>",
            "</div>",
        ),
        500.0,
    );
    let narrow = by_id(&doc.root, "narrow").unwrap();
    // Explicit width should be respected even with stretch
    assert!(
        (narrow.layout.content_rect.w - 200.0).abs() < 5.0,
        "explicit w={:.0} should be 200",
        narrow.layout.content_rect.w
    );
}

#[test]
fn grid_item_max_width() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr; width:800px'>",
            "<div id='capped' style='max-width:400px'>Capped</div>",
            "</div>",
        ),
        900.0,
    );
    let capped = by_id(&doc.root, "capped").unwrap();
    assert!(
        capped.layout.content_rect.w <= 405.0,
        "max-width:400 w={:.0} should be <=400",
        capped.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID with special children                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_display_none_child_skipped() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px'>",
            "<div id='a'>A</div>",
            "<div style='display:none'>Hidden</div>",
            "<div id='b'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // display:none is skipped, so A and B are the two grid items
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "a and b should be on same row (none skipped)"
    );
}

#[test]
fn grid_absolute_child_out_of_flow() {
    let doc = load_html(
        concat!(
        "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px; position:relative'>",
        "<div id='a'>A</div>",
        "<div id='abs' style='position:absolute; top:0; right:0; width:100px'>Abs</div>",
        "<div id='b'>B</div>",
        "</div>",
    ),
        700.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Absolute child is out of flow, A and B are the two grid items
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "a and b should be on same row (absolute out of flow)"
    );
}

#[test]
fn grid_visibility_hidden_takes_space() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr 1fr; width:600px'>",
            "<div id='a'>A</div>",
            "<div id='hidden' style='visibility:hidden'>Hidden</div>",
            "<div id='c'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let hidden = by_id(&doc.root, "hidden").unwrap();
    let c = by_id(&doc.root, "c").unwrap();
    // visibility:hidden still takes space
    assert!(
        hidden.layout.content_rect.w > 150.0,
        "hidden item should take space, w={:.0}",
        hidden.layout.content_rect.w
    );
    assert!(
        c.layout.content_rect.x > hidden.layout.content_rect.x,
        "c should be after hidden item"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID GAP: different row/column values                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_different_row_column_gaps() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:1fr 1fr; row-gap:30px; column-gap:10px; width:600px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "<div id='c'>C</div><div id='d'>D</div>",
        "</div>",
    ), 700.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    let c = by_id(&doc.root, "c").unwrap();
    // Column gap = 10px
    let col_gap = b.layout.content_rect.x - (a.layout.content_rect.x + a.layout.content_rect.w);
    assert!(
        (col_gap - 10.0).abs() < 3.0,
        "col gap={:.0} should be 10",
        col_gap
    );
    // Row gap = 30px
    let row_gap = c.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    assert!(
        (row_gap - 30.0).abs() < 3.0,
        "row gap={:.0} should be 30",
        row_gap
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  MINMAX variations                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_minmax_auto_1fr() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:minmax(auto, 1fr) minmax(auto, 1fr); width:600px'>",
        "<div id='a'>Short</div><div id='b'>Longer content here</div>",
        "</div>",
    ), 700.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Both should be equal width (1fr each)
    assert!(
        (a.layout.content_rect.w - b.layout.content_rect.w).abs() < 5.0,
        "minmax(auto,1fr): a={:.0} b={:.0} should be equal",
        a.layout.content_rect.w,
        b.layout.content_rect.w
    );
}

#[test]
fn grid_minmax_min_content_1fr() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:minmax(min-content, 1fr) 2fr; width:600px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 700.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // 1fr vs 2fr: a=200, b=400
    assert!(
        b.layout.content_rect.w > a.layout.content_rect.w * 1.5,
        "2fr w={:.0} should be ~2x 1fr w={:.0}",
        b.layout.content_rect.w,
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC() in grid tracks                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_calc_column_width() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:calc(50% - 20px) calc(50% - 20px); width:800px; gap:40px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 900.0);
    let a = by_id(&doc.root, "a").unwrap();
    // calc(50% - 20px) = 400 - 20 = 380px
    assert!(
        (a.layout.content_rect.w - 380.0).abs() < 5.0,
        "calc(50%%-20px) w={:.0} should be ~380",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DENSE PACKING with various spans                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_dense_fills_gaps_with_small_items() {
    let doc = load_html(concat!(
        "<div style='display:grid; grid-template-columns:repeat(3, 1fr); grid-auto-flow:dense; width:600px'>",
        "<div id='wide' style='grid-column:span 2'>Wide</div>",
        "<div id='small1'>S1</div>",
        "<div id='small2'>S2</div>",
        "</div>",
    ), 700.0);
    let wide = by_id(&doc.root, "wide").unwrap();
    let s1 = by_id(&doc.root, "small1").unwrap();
    // Dense: wide takes cols 1-2, s1 goes to col 3 (same row), s2 wraps
    assert!(
        (wide.layout.content_rect.y - s1.layout.content_rect.y).abs() < 5.0,
        "dense: wide and s1 should be on same row"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SPANNING across implicit tracks                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_span_into_implicit_columns() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:600px'>",
            "<div id='span3' style='grid-column:span 3'>Spans 3</div>",
            "</div>",
        ),
        700.0,
    );
    let span = by_id(&doc.root, "span3").unwrap();
    // Spans 3 columns (2 explicit + 1 implicit)
    assert!(
        span.layout.content_rect.w > 400.0,
        "span 3 w={:.0} should be > 400 (spans beyond explicit)",
        span.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  NESTED GRIDS                                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_nested_grids_independent() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:800px; gap:10px'>",
            "  <div id='outer1' style='display:grid; grid-template-columns:1fr 1fr; gap:5px'>",
            "    <div id='i1'>I1</div><div id='i2'>I2</div>",
            "  </div>",
            "  <div id='outer2'>Simple</div>",
            "</div>",
        ),
        900.0,
    );
    let outer1 = by_id(&doc.root, "outer1").unwrap();
    let i1 = by_id(&doc.root, "i1").unwrap();
    let i2 = by_id(&doc.root, "i2").unwrap();
    // Outer grid: 2 cols of ~395px each
    assert!(
        outer1.layout.content_rect.w > 350.0,
        "outer1 w={:.0}",
        outer1.layout.content_rect.w
    );
    // Inner grid: 2 cols within outer1
    assert!(
        i1.layout.content_rect.w > 150.0,
        "inner i1 w={:.0}",
        i1.layout.content_rect.w
    );
    assert!(
        (i1.layout.content_rect.w - i2.layout.content_rect.w).abs() < 5.0,
        "inner items should be equal"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  GRID SHORTHAND                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_shorthand_rows_and_cols() {
    let doc = load_html(concat!(
        "<style>.g { display:grid; grid: 100px 200px / 1fr 2fr; width:600px; }</style>",
        "<div class='g'><div id='a'>A</div><div id='b'>B</div><div id='c'>C</div><div id='d'>D</div></div>",
    ), 700.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    let c = by_id(&doc.root, "c").unwrap();
    // Rows: 100px, 200px. Cols: 1fr(200), 2fr(400)
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 5.0,
        "1fr col={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 400.0).abs() < 5.0,
        "2fr col={:.0}",
        b.layout.content_rect.w
    );
    assert!(
        (a.layout.content_rect.h - 100.0).abs() < 5.0,
        "row1={:.0}",
        a.layout.content_rect.h
    );
    assert!(
        (c.layout.content_rect.h - 200.0).abs() < 5.0,
        "row2={:.0}",
        c.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn grid_single_column_many_rows() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr; width:300px; gap:5px'>",
            "<div>1</div><div>2</div><div>3</div><div>4</div><div>5</div>",
            "<div>6</div><div>7</div><div>8</div><div>9</div><div id='last'>10</div>",
            "</div>",
        ),
        400.0,
    );
    let last = by_id(&doc.root, "last").unwrap();
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(
        last.layout.content_rect.y > grid.layout.content_rect.y + 50.0,
        "10th item should be well below grid start"
    );
    assert!(
        (last.layout.content_rect.w - 300.0).abs() < 5.0,
        "single col item w={:.0} should fill grid",
        last.layout.content_rect.w
    );
}

#[test]
fn grid_zero_width_container() {
    let doc = load_html(
        "<div style='display:grid; grid-template-columns:1fr; width:0'><div>X</div></div>",
        100.0,
    );
    // Should not crash
    let grid = find_box(&doc.root, &|b| b.style.display == Display::Grid).unwrap();
    assert!(grid.layout.content_rect.w <= 1.0, "zero width grid");
}

#[test]
fn grid_very_large_span() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:repeat(3, 1fr); width:600px'>",
            "<div id='huge' style='grid-column:span 100'>Huge span</div>",
            "</div>",
        ),
        700.0,
    );
    let huge = by_id(&doc.root, "huge").unwrap();
    // Should not crash, span clamped to grid size
    assert!(huge.layout.content_rect.w > 0.0, "huge span should render");
}

#[test]
fn grid_overlapping_explicit_placements() {
    let doc = load_html(
        concat!(
            "<div style='display:grid; grid-template-columns:1fr 1fr; width:400px'>",
            "<div id='a' style='grid-column:1; grid-row:1'>A</div>",
            "<div id='b' style='grid-column:1; grid-row:1'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // Both placed in same cell - should overlap (z-order determined by DOM order)
    assert!(
        (a.layout.content_rect.x - b.layout.content_rect.x).abs() < 2.0,
        "overlapping items should be at same position"
    );
}
