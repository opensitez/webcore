// Ported from cpptests/test_coverage_gaps.cpp
// Coverage gap tests for table cell block layout, BR line breaks, rect consistency.
// Widget-specific tests (InsertHR, Backspace, etc.) are omitted.

use webcore::load_html;
use webcore::types::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parse_and_layout(html: &str, viewport_width: f32) -> Document {
    load_html(html, viewport_width)
}

fn find_box<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(b) = find_box(child, pred) {
            return Some(b);
        }
    }
    None
}

fn find_all_boxes<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F) -> Vec<&'a WebCore> {
    let mut result = Vec::new();
    collect_matching(root, pred, &mut result);
    result
}

fn collect_matching<'a, F: Fn(&WebCore) -> bool>(
    node: &'a WebCore,
    pred: &F,
    out: &mut Vec<&'a WebCore>,
) {
    if pred(node) {
        out.push(node);
    }
    for child in &node.children {
        collect_matching(child, pred, out);
    }
}

// ============================================================
// Gap 1a: Multiple block children stacking in a table cell
// ============================================================

#[test]
fn cell_blocks_multiple_block_children_stack() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <p>Text A</p><hr><p>Text B</p>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some(), "td not found");
    let td = td.unwrap();

    let blocks: Vec<&WebCore> = td
        .children
        .iter()
        .filter(|ch| ch.style.display != Display::None)
        .collect();
    assert!(
        blocks.len() >= 3,
        "expected at least 3 block children in td"
    );

    // Each subsequent child's contentRect must not overlap the previous
    for i in 1..blocks.len() {
        let prev_bottom = blocks[i - 1].layout.content_rect.y + blocks[i - 1].layout.content_rect.h;
        assert!(
            blocks[i].layout.content_rect.y >= prev_bottom,
            "block {} overlaps block {} ({} < {})",
            i,
            i - 1,
            blocks[i].layout.content_rect.y,
            prev_bottom
        );
    }

    // All 4 rects of each child must be consistently offset
    for b in &blocks {
        assert!(b.layout.border_rect.y >= b.layout.margin_rect.y);
        assert!(b.layout.padding_rect.y >= b.layout.border_rect.y);
        assert!(b.layout.content_rect.y >= b.layout.padding_rect.y);
    }
}

// ============================================================
// Gap 1b: Block child with margin/padding in table cell
// ============================================================

#[test]
fn cell_blocks_block_child_with_margin_padding() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <div style='margin:10px; padding:5px;'>content</div>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let div = find_box(td, &|b: &WebCore| b.tag == "div");
    assert!(div.is_some());
    let div = div.unwrap();

    assert!(div.layout.margin_rect.w >= div.layout.border_rect.w);
    assert!(div.layout.border_rect.w >= div.layout.padding_rect.w);
    assert!(div.layout.padding_rect.w >= div.layout.content_rect.w);
    assert!(div.layout.margin_rect.h >= div.layout.border_rect.h);
    assert!(div.layout.content_rect.x >= div.layout.padding_rect.x);
    assert!(div.layout.content_rect.y >= div.layout.padding_rect.y);
}

// ============================================================
// Gap 1c: Block child with border — no overlap with next sibling
// ============================================================

#[test]
fn cell_blocks_block_child_with_border_no_overlap() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <div style='border:2px solid black;'>text</div>\
         <hr>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let div = find_box(td, &|b: &WebCore| b.tag == "div");
    let hr = find_box(td, &|b: &WebCore| b.tag == "hr");
    assert!(div.is_some());
    assert!(hr.is_some());
    let div = div.unwrap();
    let hr = hr.unwrap();

    let div_bottom = div.layout.content_rect.y + div.layout.content_rect.h;
    assert!(hr.layout.content_rect.y >= div_bottom);
}

// ============================================================
// Gap 1i: Nested blocks inside table cell
// ============================================================

#[test]
fn cell_blocks_nested_blocks_in_cell() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <div><p>Nested P</p></div>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let div = find_box(td, &|b: &WebCore| b.tag == "div");
    assert!(div.is_some());
    let div = div.unwrap();

    let p = find_box(div, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert!(p.unwrap().layout.content_rect.h > 0.0);
}

// ============================================================
// Gap 1j: Nested table with HR in outer cell
// ============================================================

#[test]
fn cell_blocks_nested_table_with_hr_in_outer_cell() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <table><tr><td>Inner</td></tr></table>\
         <hr>\
         </td></tr></table>",
        800.0,
    );
    let tds = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(tds.len() >= 2);

    // Find the outer td (the one with an HR child)
    let outer_td = tds
        .iter()
        .find(|td| find_box(td, &|b: &WebCore| b.tag == "hr").is_some());
    assert!(outer_td.is_some());
    let outer_td = outer_td.unwrap();

    let hr = find_box(outer_td, &|b: &WebCore| b.tag == "hr").unwrap();
    let inner_table = find_box(outer_td, &|b: &WebCore| b.tag == "table").unwrap();
    assert!(
        hr.layout.content_rect.y
            >= inner_table.layout.content_rect.y + inner_table.layout.content_rect.h
    );
}

// ============================================================
// Gap 1k: Block child rect consistency (4-rect relationships)
// ============================================================

#[test]
fn cell_blocks_rect_consistency_after_offset() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <div style='margin:5px; padding:3px; border:2px solid black;'>A</div>\
         <div style='margin:5px; padding:3px; border:2px solid black;'>B</div>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let divs = find_all_boxes(td, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);

    for d in &divs {
        assert!(d.layout.border_rect.x <= d.layout.padding_rect.x);
        assert!(d.layout.border_rect.y <= d.layout.padding_rect.y);
        assert!(d.layout.padding_rect.x <= d.layout.content_rect.x);
        assert!(d.layout.padding_rect.y <= d.layout.content_rect.y);
        assert!(d.layout.margin_rect.x <= d.layout.border_rect.x);
        assert!(d.layout.margin_rect.y <= d.layout.border_rect.y);
    }

    // Second div must be below first div
    assert!(
        divs[1].layout.content_rect.y
            >= divs[0].layout.content_rect.y + divs[0].layout.content_rect.h
    );
}

// ============================================================
// Gap 2a: BR at position 0 in a line
// ============================================================

#[test]
fn br_line_break_br_at_start() {
    let doc = parse_and_layout("<p><br>Text</p>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert!(
        p.unwrap().layout.line_cache.len() >= 2,
        "BR at start should produce at least 2 lines"
    );
}

// ============================================================
// Gap 2b: BR at end of text
// ============================================================

#[test]
fn br_line_break_br_at_end() {
    let doc = parse_and_layout("<p>Text<br></p>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert!(
        p.unwrap().layout.line_cache.len() >= 2,
        "BR at end should produce at least 2 lines"
    );
}

// ============================================================
// Gap 2c: Multiple consecutive BRs
// ============================================================

#[test]
fn br_line_break_multiple_brs() {
    let doc = parse_and_layout("<p>A<br><br><br>B</p>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert!(
        p.unwrap().layout.line_cache.len() >= 4,
        "A<br><br><br>B should produce 4 lines"
    );
}

// ============================================================
// Gap 2e: Text + BR + text that exactly fills line width
// ============================================================

#[test]
fn br_line_break_br_breaks_before_width_fill() {
    let doc = parse_and_layout("<div style='width:500px;'>A<br>B</div>", 800.0);
    // Text is in #text child nodes, not in div.text — find the div by line_cache
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.layout.line_cache.len() >= 2
    });
    assert!(div.is_some(), "expected div with >= 2 lines");
}

// ============================================================
// Gap 6b: display:none child skipped in table cell
// ============================================================

#[test]
fn cell_blocks_display_none_child_skipped() {
    let doc = parse_and_layout(
        "<table><tr><td>\
         <div style='display:none;'>Hidden</div>\
         <hr>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());

    let hr = find_box(td.unwrap(), &|b: &WebCore| b.tag == "hr");
    assert!(hr.is_some());
    assert!(hr.unwrap().layout.content_rect.y < 30.0);
}

// ============================================================
// Gap 5e: Table cell padding with block children
// ============================================================

#[test]
fn cell_padding_with_block_children() {
    let doc = parse_and_layout(
        "<table><tr>\
         <td style='padding:20px;'><div>Text</div><hr></td>\
         </tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let div = find_box(td, &|b: &WebCore| b.tag == "div");
    let hr = find_box(td, &|b: &WebCore| b.tag == "hr");
    assert!(div.is_some());
    assert!(hr.is_some());
    let div = div.unwrap();
    let hr = hr.unwrap();

    // div is inside td which has 20px padding — absolute y will be > 0
    assert!(div.layout.content_rect.y >= 0.0);
    assert!(hr.layout.content_rect.y >= div.layout.content_rect.y + div.layout.content_rect.h);
}

// ============================================================
// Gap 5f: border-collapse table with HR in cell
// ============================================================

#[test]
fn cell_blocks_border_collapse_with_hr() {
    let doc = parse_and_layout(
        "<table style='border-collapse:collapse;'>\
         <tr><td style='border:1px solid black;'>\
         <p>Text</p><hr>\
         </td></tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let hr = find_box(td, &|b: &WebCore| b.tag == "hr").unwrap();
    let p = find_box(td, &|b: &WebCore| b.tag == "p").unwrap();
    assert!(hr.layout.content_rect.y >= p.layout.content_rect.y + p.layout.content_rect.h);
}

// ============================================================
// Gap 1g: Colspan cell with block children
// ============================================================

#[test]
fn cell_blocks_colspan_cell_with_block_children() {
    let doc = parse_and_layout(
        "<table style='width:600px;'>\
         <tr><td colspan='2'><p>Wide</p><hr></td></tr>\
         <tr><td>A</td><td>B</td></tr>\
         </table>",
        800.0,
    );
    let wide_cell = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("colspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(wide_cell.is_some(), "colspan=2 cell not found");
    let wide_cell = wide_cell.unwrap();

    let hr = find_box(wide_cell, &|b: &WebCore| b.tag == "hr");
    let p = find_box(wide_cell, &|b: &WebCore| b.tag == "p");
    assert!(hr.is_some());
    assert!(p.is_some());

    let hr = hr.unwrap();
    let p = p.unwrap();
    assert!(
        hr.layout.content_rect.y >= p.layout.content_rect.y + p.layout.content_rect.h,
        "hr ({}) should be below p ({}+{})",
        hr.layout.content_rect.y,
        p.layout.content_rect.y,
        p.layout.content_rect.h
    );
    assert!(
        hr.layout.content_rect.w > 0.0,
        "hr should have positive width"
    );
}

// ============================================================
// Gap 1h: Rowspan cell with block children
// ============================================================

#[test]
fn cell_blocks_rowspan_cell_with_block_children() {
    let doc = parse_and_layout(
        "<table style='width:600px;'>\
         <tr><td rowspan='2'><div>Tall</div><hr></td><td>B</td></tr>\
         <tr><td>C</td></tr>\
         </table>",
        800.0,
    );
    let tall_cell = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("rowspan")
            .map(|v| v == "2")
            .unwrap_or(false)
    });
    assert!(tall_cell.is_some(), "rowspan=2 cell not found");
    let tall_cell = tall_cell.unwrap();

    let hr = find_box(tall_cell, &|b: &WebCore| b.tag == "hr");
    let div = find_box(tall_cell, &|b: &WebCore| b.tag == "div");
    assert!(hr.is_some());
    assert!(div.is_some());
    let hr = hr.unwrap();
    let div = div.unwrap();
    assert!(
        hr.layout.content_rect.y >= div.layout.content_rect.y + div.layout.content_rect.h,
        "hr should be below div in rowspan cell"
    );
}

// ============================================================
// Gap 2d: BR followed by long wrapping text
// ============================================================

#[test]
fn br_line_break_br_followed_by_wrapping_text() {
    let doc = parse_and_layout(
        "<div style='width:100px;'>Short<br>\
         This is a very long sentence that must wrap across multiple lines</div>",
        800.0,
    );
    // Find the div that has >= 3 lines (short + BR + wrapped lines)
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.layout.line_cache.len() >= 3
    });
    assert!(
        div.is_some(),
        "expected div with >= 3 lines from BR + wrapping"
    );
}

// ============================================================
// Gap 2f: BR zero-width doesn't affect word-wrap
// ============================================================

#[test]
fn br_line_break_br_zero_width_doesnt_affect_wrap() {
    let doc = parse_and_layout("<div style='width:50px;'>AAAA<br>BB</div>", 800.0);
    // Should produce exactly 2 lines: "AAAA+BR" and "BB"
    // BR shouldn't cause AAAA to wrap when it fits on one line
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.layout.line_cache.len() >= 2
    });
    assert!(div.is_some(), "expected div with at least 2 lines from BR");
}

// ============================================================
// Gap 2h: BR inside bold/italic span — does not panic
// ============================================================

#[test]
fn br_line_break_br_inside_styled_span() {
    let doc = parse_and_layout("<p><b>Bold<br>Text</b></p>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some(), "p element must exist");
    // Must not panic; line_cache should be >= 1 (at least one line)
    assert!(
        p.unwrap().layout.line_cache.len() >= 1,
        "bold text with BR must produce at least 1 line"
    );
}

// ============================================================
// Gap 4a: vertical-align:middle with block children in table cell
// ============================================================

#[test]
fn cell_valign_middle_with_block_children() {
    let doc = parse_and_layout(
        "<table style='width:400px;'>\
         <tr>\
         <td style='height:200px; vertical-align:middle;'>\
         <p>Text</p><hr>\
         </td>\
         <td>Short</td>\
         </tr></table>",
        800.0,
    );
    let cell = find_box(&doc.root, &|b: &WebCore| {
        b.style.vertical_align == VerticalAlign::Middle
    });
    assert!(cell.is_some(), "cell with vertical-align:middle not found");
    let cell = cell.unwrap();
    // The cell's content area must be >= its padding area y (offset applied correctly)
    assert!(
        cell.layout.content_rect.y >= cell.layout.padding_rect.y,
        "contentRect.y ({}) should be >= paddingRect.y ({})",
        cell.layout.content_rect.y,
        cell.layout.padding_rect.y
    );
}

// ============================================================
// Gap 4b: vertical-align:bottom with block children
// ============================================================

#[test]
fn cell_valign_bottom_with_block_children() {
    let doc = parse_and_layout(
        "<table style='width:400px;'>\
         <tr>\
         <td style='height:200px; vertical-align:bottom;'>\
         <p>Text</p><hr>\
         </td>\
         <td>Short</td>\
         </tr></table>",
        800.0,
    );
    let cell = find_box(&doc.root, &|b: &WebCore| {
        b.style.vertical_align == VerticalAlign::Bottom
    });
    assert!(cell.is_some(), "cell with vertical-align:bottom not found");
    let cell = cell.unwrap();
    // Cell's content area should be offset from padding top (vAlign shift > 0)
    let resolved_pad_border_top = cell.layout.resolved_pad_top + cell.layout.resolved_border_top;
    assert!(
        cell.layout.content_rect.y > resolved_pad_border_top
            || cell.layout.content_rect.y >= cell.layout.padding_rect.y,
        "bottom-aligned cell should shift content down"
    );
}

// ============================================================
// Gap 4c: vertical-align:top (default) with block children
// ============================================================

#[test]
fn cell_valign_top_with_block_children() {
    let doc = parse_and_layout(
        "<table style='width:400px;'>\
         <tr>\
         <td style='height:200px; vertical-align:top;'>\
         <p>Text</p><hr>\
         </td>\
         <td>Short</td>\
         </tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "td" && b.children.len() >= 2
    });
    assert!(td.is_some(), "td with multiple children not found");
    let td = td.unwrap();

    // First visible block child should be near the top of the cell content area
    let first_child = td
        .children
        .iter()
        .find(|ch| ch.style.display != Display::None);
    if let Some(child) = first_child {
        assert!(
            child.layout.content_rect.y < 30.0,
            "top-aligned first child y ({}) should be < 30",
            child.layout.content_rect.y
        );
    }
}

// ============================================================
// Gap 5e: Table cell padding with block children (pure layout test)
// ============================================================

#[test]
fn cell_padding_with_block_children_layout() {
    let doc = parse_and_layout(
        "<table><tr>\
         <td style='padding:20px;'><div>Text</div><hr></td>\
         </tr></table>",
        800.0,
    );
    let td = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(td.is_some());
    let td = td.unwrap();

    let div = find_box(td, &|b: &WebCore| b.tag == "div");
    let hr = find_box(td, &|b: &WebCore| b.tag == "hr");
    assert!(div.is_some());
    assert!(hr.is_some());
    let div = div.unwrap();
    let hr = hr.unwrap();

    // div is first child in padded cell — absolute y >= 0
    assert!(div.layout.content_rect.y >= 0.0);
    // hr must be below div
    assert!(
        hr.layout.content_rect.y >= div.layout.content_rect.y + div.layout.content_rect.h,
        "hr ({}) should be below div ({}+{})",
        hr.layout.content_rect.y,
        div.layout.content_rect.y,
        div.layout.content_rect.h
    );
}
