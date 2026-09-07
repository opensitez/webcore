// Ported from cpptests/test_flex.cpp
// Flex container / item CSS property parsing and flex layout tests.
// NOTE: Smoke tests that require Render(dc, …) are omitted (no rendering DC in Rust).
// NOTE: Tests referencing box->parent are adapted to use tree walking.

use webcore::css::apply_property;
use webcore::types::*;
use webcore::{load_html, parse_html};

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

fn walk_boxes<F: FnMut(&WebCore)>(root: &WebCore, visitor: &mut F) {
    visitor(root);
    for child in &root.children {
        walk_boxes(child, visitor);
    }
}

// ============================================================
// Flex Card Sizing Diagnostic
// ============================================================

#[test]
fn flex_three_cards_equal_sizing() {
    // Reproduce the "Flexbox Layout" section from demo.html
    let html = r#"<html><body><div style="display:flex;gap:12px;">
<div style="background-color:#eaf2f8;padding:12px;border:1px solid #3498db;"><b>Card 1</b><br>Flex items arranged horizontally with gap.</div>
<div style="background-color:#fef9e7;padding:12px;border:1px solid #f1c40f;"><b>Card 2</b><br>Each card is a separate editable region.</div>
<div style="background-color:#fdedec;padding:12px;border:1px solid #e74c3c;"><b>Card 3</b><br>Background, border, padding, font.</div>
</div></body></html>"#;
    let mut doc = parse_and_layout(html, 800.0);

    // Simulate a resize by running layout again (same width)
    let mut engine = webcore::LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    // And again — this is the pattern that degrades on repeated layout
    engine.layout(&mut doc, 800.0);

    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex).expect("flex container");
    let cards: Vec<&WebCore> = flex
        .children
        .iter()
        .filter(|c| c.style.display != Display::None && c.tag != "#text")
        .collect();
    assert_eq!(cards.len(), 3, "expected 3 flex cards");
    let w0 = cards[0].layout.border_rect.w;
    let w1 = cards[1].layout.border_rect.w;
    let w2 = cards[2].layout.border_rect.w;
    eprintln!("Card widths after re-layout: {:.1} {:.1} {:.1}", w0, w1, w2);
    eprintln!(
        "Card 0 line widths: {:?}",
        cards[0]
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .collect::<Vec<_>>()
    );
    eprintln!(
        "Card 1 line widths: {:?}",
        cards[1]
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .collect::<Vec<_>>()
    );
    eprintln!(
        "Card 2 line widths: {:?}",
        cards[2]
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .collect::<Vec<_>>()
    );
    let max_w = w0.max(w1).max(w2);
    let min_w = w0.min(w1).min(w2);
    assert!(min_w > 0.0, "cards should have positive width");
    assert!(
        max_w / min_w < 2.0,
        "cards are too unequal after re-layout: {:.1} {:.1} {:.1} (ratio {:.2})",
        w0,
        w1,
        w2,
        max_w / min_w
    );
}

// ============================================================
// Flex Container Properties
// ============================================================

#[test]
fn flex_direction_row() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-direction", "row");
    assert_eq!(style.flex_direction, FlexDirection::Row);
}

#[test]
fn flex_direction_column() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-direction", "column");
    assert_eq!(style.flex_direction, FlexDirection::Column);
}

#[test]
fn flex_direction_row_reverse() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-direction", "row-reverse");
    assert_eq!(style.flex_direction, FlexDirection::RowReverse);
}

#[test]
fn flex_direction_column_reverse() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-direction", "column-reverse");
    assert_eq!(style.flex_direction, FlexDirection::ColumnReverse);
}

#[test]
fn flex_wrap_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-wrap", "wrap");
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
    apply_property(&mut style, "flex-wrap", "nowrap");
    assert_eq!(style.flex_wrap, FlexWrap::Nowrap);
    apply_property(&mut style, "flex-wrap", "wrap-reverse");
    assert_eq!(style.flex_wrap, FlexWrap::WrapReverse);
}

#[test]
fn flex_justify_content() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-content", "center");
    assert_eq!(style.justify_content, JustifyContent::Center);
    apply_property(&mut style, "justify-content", "space-between");
    assert_eq!(style.justify_content, JustifyContent::SpaceBetween);
    apply_property(&mut style, "justify-content", "space-around");
    assert_eq!(style.justify_content, JustifyContent::SpaceAround);
    apply_property(&mut style, "justify-content", "flex-start");
    assert_eq!(style.justify_content, JustifyContent::FlexStart);
    apply_property(&mut style, "justify-content", "flex-end");
    assert_eq!(style.justify_content, JustifyContent::FlexEnd);
}

#[test]
fn flex_align_items() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-items", "center");
    assert_eq!(style.align_items, AlignItems::Center);
    apply_property(&mut style, "align-items", "flex-start");
    assert_eq!(style.align_items, AlignItems::FlexStart);
    apply_property(&mut style, "align-items", "flex-end");
    assert_eq!(style.align_items, AlignItems::FlexEnd);
    apply_property(&mut style, "align-items", "stretch");
    assert_eq!(style.align_items, AlignItems::Stretch);
    apply_property(&mut style, "align-items", "baseline");
    assert_eq!(style.align_items, AlignItems::Baseline);
}

#[test]
fn flex_gap_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "gap", "10px");
    assert_eq!(style.gap.resolve(16.0, 0.0, 16.0) as i32, 10);
}

#[test]
fn flex_row_column_gap() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "row-gap", "10px");
    apply_property(&mut style, "column-gap", "20px");
    assert_eq!(style.row_gap.resolve(16.0, 0.0, 16.0) as i32, 10);
    assert_eq!(style.column_gap.resolve(16.0, 0.0, 16.0) as i32, 20);
}

// ============================================================
// Flex Item Properties
// ============================================================

#[test]
fn flex_grow() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-grow", "2");
    assert!(style.flex_grow > 1.99 && style.flex_grow < 2.01);
}

#[test]
fn flex_shrink() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-shrink", "0");
    assert!(style.flex_shrink < 0.01);
}

#[test]
fn flex_basis() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-basis", "200px");
    assert_eq!(style.flex_basis.resolve(16.0, 0.0, 16.0) as i32, 200);
}

#[test]
fn flex_align_self_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-self", "center");
    assert_eq!(style.align_self, AlignSelf::Center);
    apply_property(&mut style, "align-self", "flex-start");
    assert_eq!(style.align_self, AlignSelf::FlexStart);
}

#[test]
fn flex_order_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "order", "3");
    assert_eq!(style.order, 3);
}

// ============================================================
// Flex Layout Tests
// ============================================================

#[test]
fn flex_basic_row_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    assert_eq!(items.len(), 2);
    // Items should be side by side
    assert!(items[1].layout.content_rect.x > items[0].layout.content_rect.x);
}

#[test]
fn flex_grow_distribution() {
    let doc = parse_and_layout(
        r#"<div style="display: flex;">
            <div style="flex: 1;">A</div>
            <div style="flex: 2;">B</div>
        </div>"#,
        900.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // B should be roughly twice the width of A
        assert!(items[1].layout.content_rect.w > items[0].layout.content_rect.w);
    }
}

#[test]
fn flex_column_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-direction: column;">
            <div>A</div>
            <div>B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.style.display == Display::Flex && b.style.flex_direction == FlexDirection::Column
    });
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // Items should be stacked vertically
        assert!(items[1].layout.content_rect.y > items[0].layout.content_rect.y);
    }
}

#[test]
fn flex_gap_between_items() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; gap: 20px;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex
        .children
        .iter()
        .filter(|c| c.layout.content_rect.w > 95.0 && c.layout.content_rect.w < 105.0)
        .collect();
    if items.len() >= 2 {
        let gap_actual = items[1].layout.content_rect.x
            - (items[0].layout.content_rect.x + items[0].layout.content_rect.w);
        assert!(
            gap_actual >= 15.0 && gap_actual <= 25.0,
            "gap_actual = {gap_actual}"
        );
    }
}

// ============================================================
// flex-flow shorthand
// ============================================================

#[test]
fn flex_flow_row_wrap() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-flow", "row wrap");
    assert_eq!(style.flex_direction, FlexDirection::Row);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
}

#[test]
fn flex_flow_column_reverse() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-flow", "column-reverse nowrap");
    assert_eq!(style.flex_direction, FlexDirection::ColumnReverse);
    assert_eq!(style.flex_wrap, FlexWrap::Nowrap);
}

#[test]
fn flex_flow_wrap_only() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-flow", "wrap-reverse");
    assert_eq!(style.flex_wrap, FlexWrap::WrapReverse);
}

// ============================================================
// flex shorthand edge cases
// ============================================================

#[test]
fn flex_shorthand_none() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex", "none");
    assert!(style.flex_grow < 0.01);
    assert!(style.flex_shrink < 0.01);
    assert!(style.flex_basis.is_auto());
}

#[test]
fn flex_shorthand_auto() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex", "auto");
    assert!(style.flex_grow > 0.99);
    assert!(style.flex_shrink > 0.99);
    assert!(style.flex_basis.is_auto());
}

#[test]
fn flex_shorthand_single_number() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex", "3");
    assert!(style.flex_grow > 2.99 && style.flex_grow < 3.01);
    assert!(style.flex_shrink > 0.99);
    assert_eq!(style.flex_basis.resolve(16.0, 0.0, 16.0) as i32, 0);
}

#[test]
fn flex_shorthand_three_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex", "2 0 100px");
    assert!(style.flex_grow > 1.99);
    assert!(style.flex_shrink < 0.01);
    assert_eq!(style.flex_basis.resolve(16.0, 0.0, 16.0) as i32, 100);
}

// ============================================================
// align-content in flex (multi-line)
// ============================================================

#[test]
fn flex_align_content_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-content", "center");
    assert_eq!(style.align_content, AlignContent::Center);
    apply_property(&mut style, "align-content", "space-between");
    assert_eq!(style.align_content, AlignContent::SpaceBetween);
    apply_property(&mut style, "align-content", "stretch");
    assert_eq!(style.align_content, AlignContent::Stretch);
}

#[test]
fn flex_align_content_center_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; height: 400px;
            align-content: center; width: 200px;">
            <div style="width: 200px;">A</div>
            <div style="width: 200px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.style.display == Display::Flex && b.style.flex_wrap == FlexWrap::Wrap
    });
    assert!(flex.is_some());
    assert_eq!(flex.unwrap().style.align_content, AlignContent::Center);
}

#[test]
fn flex_align_content_space_between_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; height: 400px;
            align-content: space-between; width: 200px;">
            <div style="width: 200px;">A</div>
            <div style="width: 200px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.style.display == Display::Flex && b.style.flex_wrap == FlexWrap::Wrap
    });
    assert!(flex.is_some());
    assert_eq!(
        flex.unwrap().style.align_content,
        AlignContent::SpaceBetween
    );
}

// ============================================================
// margin: auto in flex
// ============================================================

#[test]
fn flex_margin_auto_main_axis() {
    let doc = parse_and_layout(
        r#"<div style="display: flex;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px; margin-left: auto;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // B should be pushed far to the right
        assert!(
            items[1].layout.content_rect.x > 500.0,
            "B.x = {}",
            items[1].layout.content_rect.x
        );
        // A should remain at the left
        assert!(items[0].layout.content_rect.x < 150.0);
    }
}

// ============================================================
// justify-content: space-evenly
// ============================================================

#[test]
fn flex_space_evenly_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-content", "space-evenly");
    assert_eq!(style.justify_content, JustifyContent::SpaceEvenly);
}

// ============================================================
// Row-reverse layout
// ============================================================

#[test]
fn flex_row_reverse_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-direction: row-reverse;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.style.display == Display::Flex && b.style.flex_direction == FlexDirection::RowReverse
    });
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // In row-reverse, A should be to the right of B
        assert!(
            items[0].layout.content_rect.x > items[1].layout.content_rect.x,
            "A.x={} B.x={}",
            items[0].layout.content_rect.x,
            items[1].layout.content_rect.x
        );
    }
}

// ============================================================
// Wrap layout
// ============================================================

#[test]
fn flex_wrap_second_line_below() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; width: 300px;">
            <div style="width: 200px;">A</div>
            <div style="width: 200px;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.style.display == Display::Flex && b.style.flex_wrap == FlexWrap::Wrap
    });
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // B should wrap to second line (below A)
        assert!(items[1].layout.content_rect.y > items[0].layout.content_rect.y);
    }
}

// ============================================================
// Flex shrink layout
// ============================================================

#[test]
fn flex_shrink_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; width: 300px;">
            <div style="width: 200px; flex-shrink: 1;">A</div>
            <div style="width: 200px; flex-shrink: 1;">B</div>
        </div>"#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // Both items should shrink to fit 300px
        assert!(items[0].layout.content_rect.w < 200.0);
        assert!(items[1].layout.content_rect.w < 200.0);
        let total = items[0].layout.margin_rect.w + items[1].layout.margin_rect.w;
        assert!(total <= 310.0, "total = {total}");
    }
}

// ============================================================
// start/end aliases
// ============================================================

#[test]
fn flex_justify_content_start() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-content", "start");
    assert_eq!(style.justify_content, JustifyContent::FlexStart);
}

#[test]
fn flex_justify_content_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "justify-content", "end");
    assert_eq!(style.justify_content, JustifyContent::FlexEnd);
}

#[test]
fn flex_align_items_start_alias() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-items", "start");
    assert_eq!(style.align_items, AlignItems::FlexStart);
}

#[test]
fn flex_align_self_end_alias() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-self", "end");
    assert_eq!(style.align_self, AlignSelf::FlexEnd);
}

// ============================================================
// align-content: space-evenly
// ============================================================

#[test]
fn flex_align_content_space_evenly_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-content", "space-evenly");
    assert_eq!(style.align_content, AlignContent::SpaceEvenly);
}

#[test]
fn flex_align_content_space_evenly_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; height: 300px;
            align-content: space-evenly; width: 100px;">
            <div style="width: 100px; height: 30px;">A</div>
            <div style="width: 100px; height: 30px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        assert!(
            items[0].layout.margin_rect.y > 50.0,
            "A.y = {}",
            items[0].layout.margin_rect.y
        );
        assert!(
            items[1].layout.margin_rect.y
                > items[0].layout.margin_rect.y + items[0].layout.margin_rect.h
        );
    }
}

// ============================================================
// Column-direction stretch
// ============================================================

#[test]
fn flex_column_stretch_width() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-direction: column; width: 300px;">
            <div>A</div>
            <div>B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        // A should stretch to ~300px width
        assert!(
            items[0].layout.margin_rect.w >= 295.0,
            "A.w = {}",
            items[0].layout.margin_rect.w
        );
    }
}

// ============================================================
// place-self shorthand
// ============================================================

#[test]
fn flex_place_self_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "place-self", "center end");
    assert_eq!(style.align_self, AlignSelf::Center);
    assert_eq!(style.justify_self, AlignSelf::FlexEnd);
}

#[test]
fn flex_place_self_single_value() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "place-self", "center");
    assert_eq!(style.align_self, AlignSelf::Center);
    assert_eq!(style.justify_self, AlignSelf::Center);
}

// ============================================================
// place-content shorthand
// ============================================================

#[test]
fn flex_place_content_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "place-content", "space-between center");
    assert_eq!(style.align_content, AlignContent::SpaceBetween);
    assert_eq!(style.justify_content, JustifyContent::Center);
}

#[test]
fn flex_place_content_space_evenly() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "place-content", "space-evenly space-evenly");
    assert_eq!(style.align_content, AlignContent::SpaceEvenly);
    assert_eq!(style.justify_content, JustifyContent::SpaceEvenly);
}

// ============================================================
// flex shorthand two values
// ============================================================

#[test]
fn flex_shorthand_two_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex", "2 0");
    assert!(style.flex_grow > 1.9 && style.flex_grow < 2.1);
    assert!(style.flex_shrink < 0.1);
}

#[test]
fn flex_basis_content() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-basis", "content");
    assert!(style.flex_basis.is_auto());
}

#[test]
fn flex_basis_percent() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-basis", "50%");
    assert!(!style.flex_basis.is_auto());
}

// ============================================================
// Inline flex
// ============================================================

#[test]
fn flex_inline_flex_display() {
    let doc = parse_and_layout(
        r#"<span style="display: inline-flex;">
            <span>A</span><span>B</span>
        </span>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::InlineFlex);
    assert!(flex.is_some());
}

// ============================================================
// Wrapping stress tests
// ============================================================

#[test]
fn flex_wrap_reverse_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap-reverse; width: 100px; height: 200px;">
            <div style="width: 100px; height: 30px;">A</div>
            <div style="width: 100px; height: 30px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // In wrap-reverse, first line is at bottom, second at top
        assert!(
            items[0].layout.margin_rect.y > items[1].layout.margin_rect.y,
            "A.y={} B.y={}",
            items[0].layout.margin_rect.y,
            items[1].layout.margin_rect.y
        );
    }
}

#[test]
fn flex_wrap_with_grow() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; width: 300px;">
            <div style="flex: 1 0 200px;">A</div>
            <div style="flex: 1 0 200px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        // Each item wraps to its own line and grows to fill 300px
        assert!(
            items[0].layout.margin_rect.w >= 295.0,
            "A.w = {}",
            items[0].layout.margin_rect.w
        );
    }
}

// ============================================================
// Column direction tests
// ============================================================

#[test]
fn flex_column_reverse_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-direction: column-reverse; height: 200px;">
            <div>A</div><div>B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // In column-reverse, A (first in DOM) should be below B
        assert!(
            items[0].layout.margin_rect.y > items[1].layout.margin_rect.y,
            "A.y={} B.y={}",
            items[0].layout.margin_rect.y,
            items[1].layout.margin_rect.y
        );
    }
}

#[test]
fn flex_column_with_explicit_height() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-direction: column; height: 300px;
            justify-content: center;">
            <div style="height: 50px;">A</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        // Centered in 300px with 50px height: should be around y=125
        assert!(
            items[0].layout.margin_rect.y > 100.0 && items[0].layout.margin_rect.y < 150.0,
            "A.y = {}",
            items[0].layout.margin_rect.y
        );
    }
}

// ============================================================
// Min/max constraints
// ============================================================

#[test]
fn flex_min_width_constraint() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; width: 200px;">
            <div style="flex: 1; min-width: 150px;">A</div>
            <div style="flex: 1;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        assert!(
            items[0].layout.margin_rect.w >= 150.0,
            "A.w = {}",
            items[0].layout.margin_rect.w
        );
    }
}

#[test]
fn flex_max_width_constraint() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; width: 400px;">
            <div style="flex: 1; max-width: 100px;">A</div>
            <div style="flex: 1;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        assert!(
            items[0].layout.margin_rect.w <= 105.0,
            "A.w = {}",
            items[0].layout.margin_rect.w
        );
    }
}

// ============================================================
// Alignment with single item
// ============================================================

#[test]
fn flex_justify_content_center_single() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; justify-content: center; width: 400px;">
            <div style="width: 100px;">A</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        // Body has 8px left margin (UA stylesheet); flex content_x = 8.
        // Centered: 8 + (400-100)/2 = 158
        let x = items[0].layout.margin_rect.x;
        assert!(x >= 153.0 && x <= 163.0, "A.x = {}", x);
    }
}

#[test]
fn flex_align_items_baseline_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "align-items", "baseline");
    assert_eq!(style.align_items, AlignItems::Baseline);
}

#[test]
fn flex_space_around_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; justify-content: space-around; width: 400px;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        assert!(
            items[0].layout.margin_rect.x > 30.0,
            "A.x = {}",
            items[0].layout.margin_rect.x
        );
        assert!(
            items[1].layout.margin_rect.x > 200.0,
            "B.x = {}",
            items[1].layout.margin_rect.x
        );
    }
}

#[test]
fn flex_space_evenly_layout() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; justify-content: space-evenly; width: 400px;">
            <div style="width: 100px;">A</div>
            <div style="width: 100px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        assert!(
            items[0].layout.margin_rect.x > 50.0,
            "A.x = {}",
            items[0].layout.margin_rect.x
        );
        assert!(
            items[1].layout.margin_rect.x > 200.0,
            "B.x = {}",
            items[1].layout.margin_rect.x
        );
    }
}

// ============================================================
// Cross-axis auto margin centering
// ============================================================

#[test]
fn flex_cross_auto_margin_center() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; height: 200px;">
            <div style="margin-top: auto; margin-bottom: auto; height: 50px;">A</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if !items.is_empty() {
        // Should be centered vertically: (200-50)/2 = 75
        assert!(
            items[0].layout.margin_rect.y > 60.0 && items[0].layout.margin_rect.y < 90.0,
            "A.y = {}",
            items[0].layout.margin_rect.y
        );
    }
}

// ============================================================
// flex-flow shorthand
// ============================================================

#[test]
fn flex_flow_column_wrap() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-flow", "column wrap");
    assert_eq!(style.flex_direction, FlexDirection::Column);
    assert_eq!(style.flex_wrap, FlexWrap::Wrap);
}

// ============================================================
// Multiple items with different grow/shrink
// ============================================================

#[test]
fn flex_different_grow_ratios() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; width: 400px;">
            <div style="flex: 1 0 0;">A</div>
            <div style="flex: 2 0 0;">B</div>
            <div style="flex: 1 0 0;">C</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 3 {
        // B (grow:2) should be wider than A (grow:1)
        assert!(
            items[1].layout.margin_rect.w > items[0].layout.margin_rect.w + 50.0,
            "B.w={} A.w={}",
            items[1].layout.margin_rect.w,
            items[0].layout.margin_rect.w
        );
        assert!(items[0].layout.margin_rect.w > 50.0);
        assert!(items[2].layout.margin_rect.w > 50.0);
    }
}

#[test]
fn flex_shrink_with_flex_basis() {
    let doc = parse_and_layout(
        r#"<div style="display: flex; width: 200px;">
            <div style="flex: 0 1 150px;">A</div>
            <div style="flex: 0 1 150px;">B</div>
        </div>"#,
        400.0,
    );
    let flex = find_box(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(flex.is_some());
    let flex = flex.unwrap();
    let items: Vec<&WebCore> = flex.children.iter().filter(|c| c.tag == "div").collect();
    if items.len() >= 2 {
        // Each should shrink from 150 to ~100
        assert!(
            items[0].layout.margin_rect.w < 140.0,
            "A.w = {}",
            items[0].layout.margin_rect.w
        );
        assert!(
            items[1].layout.margin_rect.w < 140.0,
            "B.w = {}",
            items[1].layout.margin_rect.w
        );
    }
}

// ============================================================
// Flex shrink-to-fit (width:auto items use max-content width)
// ============================================================

#[test]
fn flex_auto_width_items_shrink_to_content() {
    // Items with width:auto should use their intrinsic (max-content) width
    // as the flex basis — NOT the full container width.  Regression: when the
    // basis was mistakenly set to the container width every item would overflow
    // the main axis and wrap to its own line even when all items easily fit.
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; width: 800px;">
            <div id="a" style="padding: 0 10px;">Hello</div>
            <div id="b" style="padding: 0 10px;">World</div>
            <div id="c" style="padding: 0 10px;">Test</div>
        </div>"#,
        800.0,
    );
    let a = find_box(&doc.root, &|b| b.get_attr("id") == Some("a"));
    let b = find_box(&doc.root, &|b| b.get_attr("id") == Some("b"));
    let c = find_box(&doc.root, &|b| b.get_attr("id") == Some("c"));
    assert!(a.is_some() && b.is_some() && c.is_some());
    let a = a.unwrap();
    let b = b.unwrap();
    let c = c.unwrap();
    // All three items should fit on the same line (same Y)
    assert_eq!(
        a.layout.margin_rect.y, b.layout.margin_rect.y,
        "a and b should be on the same line: a.y={} b.y={}",
        a.layout.margin_rect.y, b.layout.margin_rect.y
    );
    assert_eq!(
        b.layout.margin_rect.y, c.layout.margin_rect.y,
        "b and c should be on the same line: b.y={} c.y={}",
        b.layout.margin_rect.y, c.layout.margin_rect.y
    );
    // Each item should be much narrower than the container
    assert!(
        a.layout.margin_rect.w < 200.0,
        "auto-width flex item should shrink to content, got w={}",
        a.layout.margin_rect.w
    );
}

#[test]
fn flex_auto_width_wrap_only_when_needed() {
    // Seven small items in a 400px container: they should all fit on one line.
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; width: 400px;">
            <div id="i1">A</div>
            <div id="i2">B</div>
            <div id="i3">C</div>
            <div id="i4">D</div>
            <div id="i5">E</div>
            <div id="i6">F</div>
            <div id="i7">G</div>
        </div>"#,
        400.0,
    );
    let i1 = find_box(&doc.root, &|b| b.get_attr("id") == Some("i1"));
    let i7 = find_box(&doc.root, &|b| b.get_attr("id") == Some("i7"));
    assert!(i1.is_some() && i7.is_some());
    // First and last item should be on the same row (same Y)
    assert_eq!(
        i1.unwrap().layout.margin_rect.y,
        i7.unwrap().layout.margin_rect.y,
        "all 7 small items should fit on one line"
    );
}

#[test]
fn flex_auto_width_wraps_when_truly_too_wide() {
    // An item wider than the container must still wrap.
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; width: 200px;">
            <div id="a" style="width: 150px;">First</div>
            <div id="b" style="width: 150px;">Second</div>
        </div>"#,
        400.0,
    );
    let a = find_box(&doc.root, &|b| b.get_attr("id") == Some("a"));
    let b = find_box(&doc.root, &|b| b.get_attr("id") == Some("b"));
    assert!(a.is_some() && b.is_some());
    // 150 + 150 > 200, so second item wraps
    assert!(
        b.unwrap().layout.margin_rect.y > a.unwrap().layout.margin_rect.y,
        "items that exceed container width should wrap to next line"
    );
}

#[test]
fn flex_toolbar_all_buttons_on_one_line() {
    // Simulates the dom_demo toolbar: multiple buttons with text in a flex-wrap
    // container.  All buttons should fit on one line since their total text
    // width is well under the container width.
    let doc = parse_and_layout(
        r#"<div style="display: flex; flex-wrap: wrap; gap: 4px; width: 900px; padding: 4px 10px;">
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b1">Dark</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b2">Compact</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b3">Pause</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b4">Chaos</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b5">+ Service</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b6">Alerts</div>
            <div style="padding: 0 10px; border: 1px solid #ccc;" id="b7">Feed</div>
        </div>"#,
        900.0,
    );
    let b1 = find_box(&doc.root, &|b| b.get_attr("id") == Some("b1")).unwrap();
    let b7 = find_box(&doc.root, &|b| b.get_attr("id") == Some("b7")).unwrap();
    assert_eq!(
        b1.layout.margin_rect.y, b7.layout.margin_rect.y,
        "all toolbar buttons should be on one line: b1.y={} b7.y={}",
        b1.layout.margin_rect.y, b7.layout.margin_rect.y
    );
    // Each button should be well under 200px wide
    assert!(
        b1.layout.margin_rect.w < 200.0,
        "button should shrink to content width, got {}",
        b1.layout.margin_rect.w
    );
}

// ============================================================
// button UA stylesheet: inline-flex with text node children
// ============================================================

#[test]
fn button_is_inline_flex_by_ua_stylesheet() {
    // <button> gets display:inline-flex from UA stylesheet
    let doc = parse_html("<button>Click me</button>");
    let btn = find_box(&doc.root, &|b| b.tag == "button");
    assert!(btn.is_some(), "button element not found");
    assert_eq!(
        btn.unwrap().style.display,
        Display::InlineFlex,
        "button UA stylesheet should set display:inline-flex"
    );
}

#[test]
fn button_text_node_renders_as_flex_child() {
    // Text inside a button (inline-flex) must be laid out and have non-zero size
    let doc = load_html("<button>Hello</button>", 800.0);
    let btn = find_box(&doc.root, &|b| b.tag == "button").unwrap();
    // The button should have non-zero width/height from its text content
    assert!(
        btn.layout.border_rect.w > 0.0,
        "button should have non-zero width"
    );
    assert!(
        btn.layout.border_rect.h > 0.0,
        "button should have non-zero height"
    );
    // Width should be shrunk to text (much less than viewport)
    assert!(
        btn.layout.border_rect.w < 200.0,
        "button should shrink to text content, got w={}",
        btn.layout.border_rect.w
    );
}

#[test]
fn button_text_node_stays_on_one_line() {
    // Text inside a button should not wrap: the button must grow wide enough
    let doc = load_html(
        "<button style=\"height: 30px; padding: 0 10px;\">Compact Label</button>",
        800.0,
    );
    let btn = find_box(&doc.root, &|b| b.tag == "button").unwrap();
    // A button with "Compact Label" at 16px should be wider than ~60px and under 250px
    assert!(
        btn.layout.border_rect.w > 30.0,
        "button too narrow, text likely wrapped: w={}",
        btn.layout.border_rect.w
    );
    assert!(
        btn.layout.border_rect.w < 250.0,
        "button unexpectedly wide: w={}",
        btn.layout.border_rect.w
    );
    // Height must match the explicit 30px
    assert!(
        (btn.layout.border_rect.h - 30.0).abs() < 4.0,
        "button height should be ~30px, got {}",
        btn.layout.border_rect.h
    );
}

#[test]
fn button_emoji_text_stays_on_one_line() {
    // Emoji + text in a button: both must fit on one line (no wrap)
    let doc = load_html(
        "<button style=\"height: 30px; padding: 0 10px; font-size: 12px;\">&#128207; Compact</button>",
        800.0);
    let btn = find_box(&doc.root, &|b| b.tag == "button").unwrap();
    // Button should be wider than just the emoji (>20px) but narrow (<200px)
    assert!(
        btn.layout.border_rect.w > 20.0,
        "button too narrow (emoji+text likely clipped): w={}",
        btn.layout.border_rect.w
    );
    assert!(
        btn.layout.border_rect.w < 200.0,
        "button unexpectedly wide: w={}",
        btn.layout.border_rect.w
    );
}

#[test]
fn buttons_with_emoji_all_on_one_line_in_toolbar() {
    // Simulates dom_demo toolbar with emoji buttons: all must fit on one row
    let doc = load_html(
        r#"<div style="display: flex; flex-wrap: wrap; gap: 4px; width: 1000px; padding: 4px 10px;">
            <button id="b1" style="height: 30px; padding: 0 10px; font-size: 12px;">&#127769; Dark</button>
            <button id="b2" style="height: 30px; padding: 0 10px; font-size: 12px;">&#128207; Compact</button>
            <button id="b3" style="height: 30px; padding: 0 10px; font-size: 12px;">&#9997; Pause</button>
            <button id="b4" style="height: 30px; padding: 0 10px; font-size: 12px;">&#43; Service</button>
        </div>"#,
        1000.0,
    );
    let b1 = find_box(&doc.root, &|b| b.get_attr("id") == Some("b1")).unwrap();
    let b4 = find_box(&doc.root, &|b| b.get_attr("id") == Some("b4")).unwrap();
    assert_eq!(
        b1.layout.border_rect.y, b4.layout.border_rect.y,
        "all emoji buttons should be on the same line: b1.y={} b4.y={}",
        b1.layout.border_rect.y, b4.layout.border_rect.y
    );
    // Each button must be narrow (text fits on one line)
    assert!(
        b1.layout.border_rect.w < 150.0,
        "button too wide (text wrapped?): w={}",
        b1.layout.border_rect.w
    );
}
