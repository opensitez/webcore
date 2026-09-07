// Advanced table layout tests — covers column width algorithms, percentage widths,
// auto sizing, complex colspan/rowspan, nested tables, table in flex/grid,
// overflow, wrapping, and real-world patterns.

use webcore::load_html;
use webcore::types::*;

fn by_id<'a>(root: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if root.attributes.get("id").map(|v| v == id).unwrap_or(false) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(f) = by_id(child, id) {
            return Some(f);
        }
    }
    None
}
fn find<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(f) = find(child, pred) {
            return Some(f);
        }
    }
    None
}
fn find_all<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut r = Vec::new();
    if pred(root) {
        r.push(root);
    }
    for c in &root.children {
        r.extend(find_all(c, pred));
    }
    r
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  AUTO COLUMN WIDTH DISTRIBUTION                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_auto_equal_columns() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='a'>A</td><td id='b'>B</td><td id='c'>C</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // Equal content → roughly equal widths
    let diff = (a.layout.content_rect.w - b.layout.content_rect.w).abs();
    assert!(
        diff < 30.0,
        "auto cols should be roughly equal a={:.0} b={:.0}",
        a.layout.content_rect.w,
        b.layout.content_rect.w
    );
}

#[test]
fn table_auto_content_based_widths() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='short'>Hi</td>",
            "<td id='long'>This cell has a lot more content than the other one</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let short = by_id(&d.root, "short").unwrap();
    let long = by_id(&d.root, "long").unwrap();
    assert!(
        long.layout.content_rect.w > short.layout.content_rect.w,
        "longer content gets wider column: short={:.0} long={:.0}",
        short.layout.content_rect.w,
        long.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  PERCENTAGE COLUMN WIDTHS                                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_percentage_columns() {
    let d = load_html(
        concat!(
            "<table style='width:800px'><tr>",
            "<td id='a' style='width:25%'>25%</td>",
            "<td id='b' style='width:75%'>75%</td>",
            "</tr></table>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 20.0,
        "25%={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 600.0).abs() < 20.0,
        "75%={:.0}",
        b.layout.content_rect.w
    );
}

#[test]
fn table_mixed_percent_and_auto() {
    let d = load_html(
        concat!(
            "<table style='width:800px'><tr>",
            "<td id='fixed' style='width:30%'>Fixed</td>",
            "<td id='auto1'>Auto 1</td>",
            "<td id='auto2'>Auto 2</td>",
            "</tr></table>",
        ),
        900.0,
    );
    let fixed = by_id(&d.root, "fixed").unwrap();
    let auto1 = by_id(&d.root, "auto1").unwrap();
    // Fixed gets 30% ≈ 240px, remaining split between autos
    assert!(
        (fixed.layout.content_rect.w - 240.0).abs() < 30.0,
        "30%={:.0}",
        fixed.layout.content_rect.w
    );
    assert!(auto1.layout.content_rect.w > 100.0, "auto gets remainder");
}

#[test]
fn table_percentage_over_100() {
    // Percentages > 100% should be clamped/proportioned
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='a' style='width:60%'>A</td>",
            "<td id='b' style='width:60%'>B</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // Total 120% → each proportioned to 50%
    let total = a.layout.content_rect.w + b.layout.content_rect.w;
    assert!(
        (total - 600.0).abs() < 30.0,
        "columns should fit in table width total={:.0}",
        total
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FIXED PIXEL COLUMN WIDTHS                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_pixel_column_widths() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='a' style='width:200px'>A</td>",
            "<td id='b' style='width:150px'>B</td>",
            "<td id='c'>Auto</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 15.0,
        "200px={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 150.0).abs() < 15.0,
        "150px={:.0}",
        b.layout.content_rect.w
    );
    // c gets remainder ≈ 250px
    assert!(
        c.layout.content_rect.w > 200.0,
        "auto gets remainder c={:.0}",
        c.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE-LAYOUT: FIXED                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_layout_fixed_ignores_content() {
    let d = load_html(
        concat!(
            "<table style='width:600px;table-layout:fixed'><tr>",
            "<td id='a'>Very long cell content here</td>",
            "<td id='b'>Short</td>",
            "<td id='c'>X</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // table-layout:fixed → all columns equal (200px each)
    assert!(
        (a.layout.content_rect.w - b.layout.content_rect.w).abs() < 10.0,
        "fixed layout: equal cols a={:.0} b={:.0}",
        a.layout.content_rect.w,
        b.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - c.layout.content_rect.w).abs() < 10.0,
        "fixed layout: equal cols b={:.0} c={:.0}",
        b.layout.content_rect.w,
        c.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COLSPAN                                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn colspan_spans_multiple_columns() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='span' colspan='2'>Spanning two columns</td>",
            "<td id='single'>Col</td>",
            "</tr><tr>",
            "<td id='c1'>Col</td><td id='c2'>Col</td><td id='c3'>Col</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let span = by_id(&d.root, "span").unwrap();
    let single = by_id(&d.root, "single").unwrap();
    // With equal content, colspan=2 should be roughly 2x single
    assert!(
        span.layout.content_rect.w > single.layout.content_rect.w * 1.5,
        "colspan=2 w={:.0} should be ~2x single w={:.0}",
        span.layout.content_rect.w,
        single.layout.content_rect.w
    );
}

#[test]
fn colspan_full_width() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='full' colspan='3'>Full width header</td>",
            "</tr><tr>",
            "<td>A</td><td>B</td><td>C</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let full = by_id(&d.root, "full").unwrap();
    assert!(
        (full.layout.content_rect.w - 600.0).abs() < 30.0,
        "colspan=3 spans full width w={:.0}",
        full.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ROWSPAN                                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn rowspan_spans_rows() {
    let d = load_html(
        concat!(
            "<table style='width:400px'><tr>",
            "<td id='rs' rowspan='2' style='width:100px'>Spanning</td>",
            "<td id='r1c2'>R1C2</td>",
            "</tr><tr>",
            "<td id='r2c2'>R2C2</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let rs = by_id(&d.root, "rs").unwrap();
    let r1c2 = by_id(&d.root, "r1c2").unwrap();
    let r2c2 = by_id(&d.root, "r2c2").unwrap();
    // Rowspan cell should be taller than single row
    assert!(
        rs.layout.content_rect.h >= r1c2.layout.content_rect.h * 1.5,
        "rowspan h={:.0} should span 2 rows vs {:.0}",
        rs.layout.content_rect.h,
        r1c2.layout.content_rect.h
    );
    // R2C2 should be below R1C2
    assert!(
        r2c2.layout.content_rect.y > r1c2.layout.content_rect.y + 10.0,
        "r2c2 below r1c2"
    );
}

#[test]
fn rowspan_and_colspan_together() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td id='big' colspan='2' rowspan='2'>Big cell</td>",
            "<td id='r1c3'>R1C3</td>",
            "</tr><tr>",
            "<td id='r2c3'>R2C3</td>",
            "</tr><tr>",
            "<td id='r3c1'>R3C1</td><td id='r3c2'>R3C2</td><td id='r3c3'>R3C3</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let big = by_id(&d.root, "big").unwrap();
    let r3c1 = by_id(&d.root, "r3c1").unwrap();
    // Big spans 2 cols and 2 rows
    assert!(
        big.layout.content_rect.w > r3c1.layout.content_rect.w * 1.5,
        "colspan=2 w={:.0} vs single={:.0}",
        big.layout.content_rect.w,
        r3c1.layout.content_rect.w
    );
    assert!(
        big.layout.content_rect.h > r3c1.layout.content_rect.h * 1.5,
        "rowspan=2 h={:.0} vs single={:.0}",
        big.layout.content_rect.h,
        r3c1.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BORDER COLLAPSE                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn border_collapse_cells_touch() {
    let d = load_html(
        concat!(
            "<table style='width:400px;border-collapse:collapse;border:1px solid black'><tr>",
            "<td id='a' style='border:1px solid black'>A</td>",
            "<td id='b' style='border:1px solid black'>B</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // In collapse mode, cells should be adjacent (no gap)
    let gap = b.layout.border_rect.x - (a.layout.border_rect.x + a.layout.border_rect.w);
    assert!(
        gap.abs() < 2.0,
        "collapse: cells should touch, gap={:.1}",
        gap
    );
}

#[test]
fn border_separate_has_spacing() {
    let d = load_html(
        concat!(
            "<table style='width:400px;border-collapse:separate;border-spacing:10px'><tr>",
            "<td id='a'>A</td><td id='b'>B</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let gap = b.layout.border_rect.x - (a.layout.border_rect.x + a.layout.border_rect.w);
    assert!(
        (gap - 10.0).abs() < 3.0,
        "separate: spacing={:.1} should be 10",
        gap
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  VERTICAL ALIGN                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn cell_vertical_align_top() {
    let d = load_html(
        concat!(
            "<table style='width:400px'><tr style='height:100px'>",
            "<td id='top' style='vertical-align:top'>Top</td>",
            "<td id='tall' style='height:100px'>Tall</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let top = by_id(&d.root, "top").unwrap();
    let tall = by_id(&d.root, "tall").unwrap();
    // Top-aligned cell content should be at the top of the row
    assert!(
        (top.layout.content_rect.y - tall.layout.content_rect.y).abs() < 5.0,
        "top-aligned at same y as row start"
    );
}

#[test]
fn cell_vertical_align_bottom() {
    let d = load_html(
        concat!(
            "<table style='width:400px'><tr>",
            "<td id='bot' style='vertical-align:bottom;height:100px'>Bottom</td>",
            "<td style='height:100px'>Tall</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let bot = by_id(&d.root, "bot").unwrap();
    // Bottom-aligned content should be near bottom of cell
    // Content y + content h should be near cell bottom
    let cell_bottom = bot.layout.border_rect.y + bot.layout.border_rect.h;
    let content_bottom = bot.layout.content_rect.y + bot.layout.content_rect.h;
    // Content is near cell bottom (within padding)
    assert!(
        content_bottom >= cell_bottom - 20.0,
        "bottom-align: content_bottom={:.0} cell_bottom={:.0}",
        content_bottom,
        cell_bottom
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CAPTION                                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn caption_top_above_table() {
    let d = load_html(
        concat!(
            "<table style='width:400px;caption-side:top'>",
            "<caption id='cap'>Table Title</caption>",
            "<tr><td id='cell'>Data</td></tr>",
            "</table>",
        ),
        500.0,
    );
    let cap = by_id(&d.root, "cap").unwrap();
    let cell = by_id(&d.root, "cell").unwrap();
    assert!(
        cap.layout.content_rect.y < cell.layout.content_rect.y,
        "caption above cells: cap_y={:.0} cell_y={:.0}",
        cap.layout.content_rect.y,
        cell.layout.content_rect.y
    );
}

#[test]
fn caption_bottom_below_table() {
    let d = load_html(
        concat!(
            "<table style='width:400px;caption-side:bottom'>",
            "<caption id='cap'>Table Title</caption>",
            "<tr><td id='cell'>Data</td></tr>",
            "</table>",
        ),
        500.0,
    );
    let cap = by_id(&d.root, "cap").unwrap();
    let cell = by_id(&d.root, "cell").unwrap();
    assert!(
        cap.layout.content_rect.y > cell.layout.content_rect.y,
        "caption below cells: cap_y={:.0} cell_y={:.0}",
        cap.layout.content_rect.y,
        cell.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  THEAD / TBODY / TFOOT ordering                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn thead_tbody_tfoot_visual_order() {
    let d = load_html(
        concat!(
            "<table style='width:400px'>",
            "<tfoot><tr><td id='foot'>Foot</td></tr></tfoot>",
            "<thead><tr><td id='head'>Head</td></tr></thead>",
            "<tbody><tr><td id='body1'>Body</td></tr></tbody>",
            "</table>",
        ),
        500.0,
    );
    let head = by_id(&d.root, "head").unwrap();
    let body1 = by_id(&d.root, "body1").unwrap();
    let foot = by_id(&d.root, "foot").unwrap();
    // Visual order: head → body → foot regardless of source order
    assert!(
        head.layout.content_rect.y < body1.layout.content_rect.y,
        "head before body"
    );
    assert!(
        body1.layout.content_rect.y < foot.layout.content_rect.y,
        "body before foot"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE AUTO WIDTH (shrink-to-fit)                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_auto_width_shrinks() {
    let d = load_html(
        concat!(
            "<table id='t'><tr>",
            "<td>Short</td><td>Med text</td>",
            "</tr></table>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Auto width table shrinks to content
    assert!(
        t.layout.content_rect.w < 600.0,
        "auto table w={:.0} should shrink to content",
        t.layout.content_rect.w
    );
}

#[test]
fn table_width_100_percent() {
    let d = load_html(
        concat!(
            "<div style='width:700px'>",
            "<table id='t' style='width:100%'><tr><td>Full</td></tr></table>",
            "</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 700.0).abs() < 20.0,
        "100%% table w={:.0} should be ~700",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  NESTED TABLES                                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn nested_table_layout() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr><td>",
            "<table id='inner' style='width:100%'><tr>",
            "<td id='ic1'>Inner1</td><td id='ic2'>Inner2</td>",
            "</tr></table>",
            "</td><td id='outer'>Outer</td></tr></table>",
        ),
        700.0,
    );
    let inner = by_id(&d.root, "inner").unwrap();
    let ic1 = by_id(&d.root, "ic1").unwrap();
    let ic2 = by_id(&d.root, "ic2").unwrap();
    // Inner table fits within outer cell
    assert!(inner.layout.content_rect.w > 100.0, "inner has width");
    assert!(
        ic2.layout.content_rect.x > ic1.layout.content_rect.x + 30.0,
        "inner cells side by side"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE IN FLEX/GRID                                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_inside_flex() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "<table id='t' style='flex:1'><tr><td id='c'>Cell</td></tr></table>",
            "<div style='width:200px'>Side</div>",
            "</div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Table as flex item gets flex:1 of remaining space
    assert!(
        t.layout.content_rect.w > 400.0,
        "table flex:1 w={:.0} should fill space",
        t.layout.content_rect.w
    );
}

#[test]
fn table_inside_grid() {
    let d = load_html(
        concat!(
            "<div style='display:grid;grid-template-columns:1fr 1fr;width:800px'>",
            "<table id='t'><tr><td>A</td><td>B</td></tr></table>",
            "<div>Side</div>",
            "</div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Auto-width table in grid shrinks to content (grid stretch doesn't force width on tables)
    assert!(
        t.layout.content_rect.w > 0.0,
        "table in grid renders w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE WITH POSITIONED ELEMENTS                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_cell_relative_position() {
    let d = load_html(
        concat!(
        "<table style='width:400px'><tr>",
        "<td id='rel' style='position:relative'>",
        "  <div id='abs' style='position:absolute;top:0;right:0;width:20px;height:20px'>X</div>",
        "  Cell text",
        "</td>",
        "</tr></table>",
    ),
        500.0,
    );
    let abs = by_id(&d.root, "abs").unwrap();
    assert!(abs.layout.content_rect.w > 0.0, "abs in td renders");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE CELL TEXT WRAPPING                                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn cell_text_wraps() {
    let d = load_html(
        concat!(
        "<table style='width:300px'><tr>",
        "<td id='c' style='width:100px'>This text should wrap within the narrow cell width</td>",
        "</tr></table>",
    ),
        400.0,
    );
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        c.layout.content_rect.h > 30.0,
        "text wraps → multi-line h={:.0}",
        c.layout.content_rect.h
    );
}

#[test]
fn cell_nowrap() {
    let d = load_html(concat!(
        "<table style='width:600px'><tr>",
        "<td id='nw' style='white-space:nowrap'>This text should not wrap to the next line at all</td>",
        "</tr></table>",
    ), 700.0);
    let nw = by_id(&d.root, "nw").unwrap();
    assert!(nw.layout.line_cache.len() <= 1, "nowrap = 1 line");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Email-style table layout                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn email_table_layout() {
    let d = load_html(concat!(
        "<table style='width:600px;margin:0 auto;border-collapse:collapse'>",
        "<tr><td colspan='2' style='height:80px;background:navy;color:white' id='header'>Header</td></tr>",
        "<tr>",
        "  <td style='width:70%;vertical-align:top;padding:20px' id='main'>Main content area with text</td>",
        "  <td style='width:30%;vertical-align:top;padding:10px;background:lightgray' id='side'>Sidebar</td>",
        "</tr>",
        "<tr><td colspan='2' style='height:50px;background:gray' id='footer'>Footer</td></tr>",
        "</table>",
    ), 700.0);
    let header = by_id(&d.root, "header").unwrap();
    let main = by_id(&d.root, "main").unwrap();
    let side = by_id(&d.root, "side").unwrap();
    let footer = by_id(&d.root, "footer").unwrap();
    // Header spans full width
    assert!(
        header.layout.content_rect.w > 550.0,
        "header full width w={:.0}",
        header.layout.content_rect.w
    );
    // Main and side side by side
    assert!(
        side.layout.content_rect.x > main.layout.content_rect.x + 100.0,
        "side right of main"
    );
    assert!(
        (main.layout.content_rect.y - side.layout.content_rect.y).abs() < 5.0,
        "same row"
    );
    // Footer below content
    assert!(
        footer.layout.content_rect.y > main.layout.content_rect.y + 10.0,
        "footer below content"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Data table with header                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn data_table_header_alignment() {
    let d = load_html(
        concat!(
            "<table style='width:800px;border-collapse:collapse'>",
            "<thead><tr>",
            "<th id='h1' style='width:200px;text-align:left'>Name</th>",
            "<th id='h2' style='width:100px;text-align:right'>Price</th>",
            "<th id='h3'>Description</th>",
            "</tr></thead>",
            "<tbody><tr>",
            "<td id='d1'>Widget A</td>",
            "<td id='d2' style='text-align:right'>$9.99</td>",
            "<td id='d3'>A useful widget</td>",
            "</tr></tbody>",
            "</table>",
        ),
        900.0,
    );
    let h1 = by_id(&d.root, "h1").unwrap();
    let d1 = by_id(&d.root, "d1").unwrap();
    // Header and data cells should align vertically
    assert!(
        (h1.layout.content_rect.x - d1.layout.content_rect.x).abs() < 5.0,
        "header and data align x: h={:.0} d={:.0}",
        h1.layout.content_rect.x,
        d1.layout.content_rect.x
    );
    assert!(
        (h1.layout.content_rect.w - d1.layout.content_rect.w).abs() < 5.0,
        "header and data same width: h={:.0} d={:.0}",
        h1.layout.content_rect.w,
        d1.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn empty_table_no_crash() {
    let d = load_html("<table style='width:400px'></table>", 500.0);
    let t = find(&d.root, &|b| b.tag == "table").unwrap();
    assert!(t.layout.content_rect.h >= 0.0, "empty table");
}

#[test]
fn table_single_cell() {
    let d = load_html(
        "<table style='width:300px'><tr><td id='only'>Only cell</td></tr></table>",
        400.0,
    );
    let only = by_id(&d.root, "only").unwrap();
    assert!(
        only.layout.content_rect.w > 250.0,
        "single cell fills table w={:.0}",
        only.layout.content_rect.w
    );
}

#[test]
fn table_empty_cells() {
    let d = load_html(
        concat!(
            "<table style='width:400px'><tr>",
            "<td id='a'>A</td><td id='empty'></td><td id='c'>C</td>",
            "</tr></table>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let empty = by_id(&d.root, "empty").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // Empty cell still takes space
    assert!(empty.layout.content_rect.w > 10.0, "empty cell has width");
    assert!(
        c.layout.content_rect.x > empty.layout.content_rect.x,
        "c after empty"
    );
}

#[test]
fn table_many_columns() {
    let mut html = String::from("<table style='width:1000px'><tr>");
    for i in 0..20 {
        html.push_str(&format!("<td id='c{}'>C{}</td>", i, i));
    }
    html.push_str("</tr></table>");
    let d = load_html(&html, 1100.0);
    let c0 = by_id(&d.root, "c0").unwrap();
    let c19 = by_id(&d.root, "c19").unwrap();
    // 20 columns in 1000px ≈ 50px each
    assert!(
        c19.layout.content_rect.x > c0.layout.content_rect.x + 800.0,
        "last column far right"
    );
    assert!(
        c0.layout.content_rect.w > 30.0,
        "each col has width c0={:.0}",
        c0.layout.content_rect.w
    );
}

#[test]
fn table_many_rows() {
    let mut html = String::from("<table style='width:400px'>");
    for i in 0..50 {
        html.push_str(&format!("<tr><td id='r{}'>Row {}</td></tr>", i, i));
    }
    html.push_str("</table>");
    let d = load_html(&html, 500.0);
    let r0 = by_id(&d.root, "r0").unwrap();
    let r49 = by_id(&d.root, "r49").unwrap();
    assert!(
        r49.layout.content_rect.y > r0.layout.content_rect.y + 500.0,
        "50 rows stack"
    );
}

#[test]
fn table_with_images() {
    let d = load_html(
        concat!(
            "<table style='width:500px'><tr>",
            "<td><img id='img' width='100' height='75' src='test.png'></td>",
            "<td id='text'>Text next to image</td>",
            "</tr></table>",
        ),
        600.0,
    );
    let img = by_id(&d.root, "img").unwrap();
    let text = by_id(&d.root, "text").unwrap();
    assert!(
        text.layout.content_rect.x > img.layout.content_rect.x + 90.0,
        "text right of image"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE CENTERING                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_margin_auto_centers() {
    let d = load_html(
        concat!(
            "<div style='width:800px'>",
            "<table id='t' style='width:400px;margin:0 auto'><tr><td>Centered</td></tr></table>",
            "</div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Table should be centered: x ≈ (800-400)/2 = 200
    assert!(
        (t.layout.border_rect.x - 200.0).abs() < 30.0,
        "centered table x={:.0} should be ~200",
        t.layout.border_rect.x
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TABLE MIN/MAX WIDTH                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn table_min_width() {
    let d = load_html(
        concat!("<table id='t' style='min-width:500px'><tr><td>Short</td></tr></table>",),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w >= 495.0,
        "min-width:500 w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn table_max_width() {
    let d = load_html(
        concat!(
            "<table id='t' style='width:100%;max-width:600px'><tr>",
            "<td>Content in a max-width constrained table</td></tr></table>",
        ),
        1000.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w <= 610.0,
        "max-width:600 w={:.0}",
        t.layout.content_rect.w
    );
}
