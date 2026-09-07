// Tests for Agent C layout features:
// position:sticky, aspect-ratio, multi-column, @font-face, will-change, contain, scroll-padding

use webcore::css::{apply_property, Stylesheet};
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

// ============================================================
// position: sticky — parsing and in-flow treatment
// ============================================================

#[test]
fn sticky_parsed() {
    let doc = parse_html("<div style=\"position: sticky; top: 20px;\">Sticky</div>");
    let b = find_box(&doc.root, &|b| b.style.position == Position::Sticky);
    assert!(b.is_some(), "position:sticky should be parsed");
}

#[test]
fn sticky_has_top_offset() {
    let s = {
        let mut s = ComputedStyle::default();
        apply_property(&mut s, "position", "sticky");
        apply_property(&mut s, "top", "30px");
        s
    };
    assert_eq!(s.position, Position::Sticky);
    assert_eq!(s.top, CssLength::Px(30.0));
}

#[test]
fn sticky_stays_in_flow() {
    // Sticky elements must not be removed from normal flow like absolute/fixed.
    // Subsequent siblings should be laid out below the sticky element.
    let doc = load_html(
        "<div style=\"position: sticky; top: 0; height: 50px;\">Header</div>\
         <div id=\"sibling\" style=\"height: 50px;\">Content</div>",
        800.0,
    );
    let header = find_box(&doc.root, &|b| b.style.position == Position::Sticky);
    let sibling = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|s| s == "sibling")
            .unwrap_or(false)
    });
    assert!(header.is_some() && sibling.is_some());
    // Sibling must start below the sticky header (not overlapping from y=0)
    let h = header.unwrap();
    let s = sibling.unwrap();
    assert!(
        s.layout.content_rect.y >= h.layout.content_rect.y + h.layout.content_rect.h - 1.0,
        "sibling y={} should be at or below sticky bottom={}",
        s.layout.content_rect.y,
        h.layout.content_rect.y + h.layout.content_rect.h
    );
}

// ============================================================
// aspect-ratio
// ============================================================

#[test]
fn aspect_ratio_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "aspect-ratio", "16 / 9");
    let ratio = s.aspect_ratio.expect("aspect_ratio should be Some");
    assert!((ratio - 16.0 / 9.0).abs() < 0.01, "ratio={}", ratio);
}

#[test]
fn aspect_ratio_plain_number() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "aspect-ratio", "2");
    let ratio = s.aspect_ratio.expect("aspect_ratio should be Some");
    assert!((ratio - 2.0).abs() < 0.01);
}

#[test]
fn aspect_ratio_auto_resets() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "aspect-ratio", "2");
    apply_property(&mut s, "aspect-ratio", "auto");
    assert!(s.aspect_ratio.is_none(), "auto should clear aspect_ratio");
}

#[test]
fn aspect_ratio_drives_height() {
    // A div with width:200px, no height, aspect-ratio:2 → height should be ~100px
    let doc = load_html(
        "<div style=\"width: 200px; aspect-ratio: 2;\">Box</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.style.aspect_ratio.is_some());
    assert!(b.is_some(), "box with aspect-ratio not found");
    let b = b.unwrap();
    let h = b.layout.content_rect.h;
    // Allow a small tolerance (padding/border may not be present here)
    assert!(
        (h - 100.0).abs() < 5.0,
        "height={} should be ~100 for 200px width and ratio 2",
        h
    );
}

#[test]
fn aspect_ratio_square() {
    let doc = load_html(
        "<div style=\"width: 150px; aspect-ratio: 1;\">Square</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.style.aspect_ratio == Some(1.0));
    assert!(b.is_some());
    let b = b.unwrap();
    assert!(
        (b.layout.content_rect.h - b.layout.content_rect.w).abs() < 2.0,
        "w={} h={}",
        b.layout.content_rect.w,
        b.layout.content_rect.h
    );
}

// ============================================================
// Multi-column layout
// ============================================================

#[test]
fn column_count_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "column-count", "3");
    assert_eq!(s.column_count, Some(3));
}

#[test]
fn column_width_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "column-width", "200px");
    assert_eq!(s.column_width, CssLength::Px(200.0));
}

#[test]
fn column_gap_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "column-gap", "20px");
    assert_eq!(s.column_gap, CssLength::Px(20.0));
}

#[test]
fn multi_column_children_spread_horizontally() {
    // 3-column container, 3 block children → each child in its own column
    let doc = load_html(
        "<div style=\"column-count: 3; width: 600px;\">\
           <p>One</p><p>Two</p><p>Three</p>\
         </div>",
        800.0,
    );
    let container = find_box(&doc.root, &|b| b.style.column_count == Some(3));
    assert!(container.is_some(), "multi-column container not found");
    let container = container.unwrap();

    // Collect p children (not the container itself)
    let mut ps: Vec<&WebCore> = Vec::new();
    for c in &container.children {
        if c.tag == "p" {
            ps.push(c);
        }
    }
    assert_eq!(ps.len(), 3, "should have 3 <p> children");

    // Each <p> should be in a different horizontal column (non-overlapping x positions)
    let x0 = ps[0].layout.content_rect.x;
    let x1 = ps[1].layout.content_rect.x;
    let x2 = ps[2].layout.content_rect.x;
    assert!(
        x1 > x0 + 10.0,
        "col1 x={} should be right of col0 x={}",
        x1,
        x0
    );
    assert!(
        x2 > x1 + 10.0,
        "col2 x={} should be right of col1 x={}",
        x2,
        x1
    );
}

#[test]
fn multi_column_two_cols_stack_vertically() {
    // 2-column container, 4 equal-height children.
    // Balance algorithm fills col 0 to ~half total height, then col 1.
    // With 4 identical-height items: A,B in col 0 — C,D in col 1.
    let doc = load_html(
        "<div style=\"column-count: 2; width: 400px;\">\
           <p>A</p><p>B</p><p>C</p><p>D</p>\
         </div>",
        800.0,
    );
    let container = find_box(&doc.root, &|b| b.style.column_count == Some(2));
    assert!(container.is_some());
    let container = container.unwrap();
    let ps: Vec<&WebCore> = container.children.iter().filter(|c| c.tag == "p").collect();
    assert_eq!(ps.len(), 4);
    // A and B are both in column 0 (same x)
    assert!(
        (ps[0].layout.content_rect.x - ps[1].layout.content_rect.x).abs() < 2.0,
        "A and B should be in the same column"
    );
    // C is in column 1 (x further right than A)
    assert!(
        ps[2].layout.content_rect.x > ps[0].layout.content_rect.x + 10.0,
        "C x={} should be in col 1 (right of A x={})",
        ps[2].layout.content_rect.x,
        ps[0].layout.content_rect.x
    );
    // B is below A (stacked in col 0)
    assert!(
        ps[1].layout.content_rect.y > ps[0].layout.content_rect.y,
        "B y={} should be below A y={}",
        ps[1].layout.content_rect.y,
        ps[0].layout.content_rect.y
    );
}

// ============================================================
// @font-face — stylesheet extraction
// ============================================================

#[test]
fn font_face_extracted_from_stylesheet() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add("@font-face { font-family: 'MyFont'; src: url('/fonts/myfont.ttf'); }");
    assert_eq!(ss.font_faces.len(), 1);
    assert_eq!(ss.font_faces[0].family, "MyFont");
}

#[test]
fn font_face_src_stored() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add("@font-face { font-family: TestFont; src: url('test.woff2'); }");
    assert_eq!(ss.font_faces.len(), 1);
    assert!(
        ss.font_faces[0].src.contains("test.woff2"),
        "src='{}' should contain 'test.woff2'",
        ss.font_faces[0].src
    );
}

#[test]
fn font_face_multiple_declarations() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        "@font-face { font-family: A; src: url('a.ttf'); }\
         @font-face { font-family: B; src: url('b.ttf'); }",
    );
    assert_eq!(ss.font_faces.len(), 2);
    assert_eq!(ss.font_faces[0].family, "A");
    assert_eq!(ss.font_faces[1].family, "B");
}

#[test]
fn font_face_mixed_with_normal_rules() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        "body { color: black; }\
         @font-face { font-family: Custom; src: url('c.woff'); }\
         p { font-size: 14px; }",
    );
    // Normal rules are unaffected
    assert!(!ss.rules.is_empty());
    // Font face extracted
    assert_eq!(ss.font_faces.len(), 1);
    assert_eq!(ss.font_faces[0].family, "Custom");
}

// ============================================================
// will-change
// ============================================================

#[test]
fn will_change_transform_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "will-change", "transform");
    assert!(s.will_change_transform);
}

#[test]
fn will_change_auto_does_not_set_flag() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "will-change", "auto");
    assert!(!s.will_change_transform);
}

#[test]
fn will_change_opacity_does_not_set_transform_flag() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "will-change", "opacity");
    assert!(!s.will_change_transform);
}

// ============================================================
// contain
// ============================================================

#[test]
fn contain_layout_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "contain", "layout");
    assert!(s.contain_layout);
}

#[test]
fn contain_paint_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "contain", "paint");
    assert!(s.contain_paint);
}

#[test]
fn contain_size_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "contain", "size");
    assert!(s.contain_size);
}

#[test]
fn contain_strict_sets_all() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "contain", "strict");
    assert!(
        s.contain_size && s.contain_layout && s.contain_paint,
        "strict should set size, layout, and paint"
    );
}

#[test]
fn contain_content_sets_layout_and_paint() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "contain", "content");
    assert!(
        s.contain_layout && s.contain_paint,
        "content should set layout and paint"
    );
}

// ============================================================
// scroll-padding
// ============================================================

#[test]
fn scroll_padding_shorthand_uniform() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "scroll-padding", "20px");
    assert_eq!(s.scroll_padding_top, CssLength::Px(20.0));
    assert_eq!(s.scroll_padding_right, CssLength::Px(20.0));
    assert_eq!(s.scroll_padding_bottom, CssLength::Px(20.0));
    assert_eq!(s.scroll_padding_left, CssLength::Px(20.0));
}

#[test]
fn scroll_padding_individual_top() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "scroll-padding-top", "30px");
    assert_eq!(s.scroll_padding_top, CssLength::Px(30.0));
}

#[test]
fn scroll_padding_individual_sides() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "scroll-padding-left", "10px");
    apply_property(&mut s, "scroll-padding-right", "15px");
    apply_property(&mut s, "scroll-padding-bottom", "5px");
    assert_eq!(s.scroll_padding_left, CssLength::Px(10.0));
    assert_eq!(s.scroll_padding_right, CssLength::Px(15.0));
    assert_eq!(s.scroll_padding_bottom, CssLength::Px(5.0));
}

// ── <br> in block context produces vertical space ────────────────────────────

fn walk_boxes_t<F: FnMut(&WebCore)>(root: &WebCore, f: &mut F) {
    f(root);
    for child in &root.children {
        walk_boxes_t(child, f);
    }
}

#[test]
fn br_between_block_containers_creates_vertical_gap() {
    // The <br> between two flex containers must produce a line-height of vertical
    // space so they are not flush against each other.
    let html = r#"<html><body>
        <div style="display:flex;gap:12px;" id="row1">
            <div style="width:50%;background:linear-gradient(to bottom,#667eea,#764ba2);">A</div>
            <div style="width:50%;background:linear-gradient(to right,#f093fb,#f5576c);">B</div>
        </div>
        <br>
        <div style="display:flex;gap:12px;" id="row2">
            <div style="width:33%;">C</div>
            <div style="width:33%;">D</div>
            <div style="width:33%;">E</div>
        </div>
    </body></html>"#;

    let doc = load_html(html, 800.0);

    let row1 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "row1").unwrap_or(false)
    })
    .expect("row1 not found");
    let row2 = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "row2").unwrap_or(false)
    })
    .expect("row2 not found");

    let row1_bottom = row1.layout.border_rect.y + row1.layout.border_rect.h;
    let row2_top = row2.layout.border_rect.y;
    let gap = row2_top - row1_bottom;

    // The <br> should contribute at least font_px * 1.2 (default 16px * 1.2 = 19.2px)
    assert!(
        gap >= 19.0,
        "expected vertical gap >= 19px between rows, got {:.1}px",
        gap
    );
}

#[test]
fn br_in_block_has_nonzero_height() {
    // A standalone <br> inside a block container must have a nonzero margin_rect.h.
    let html =
        r#"<html><body><div id="outer"><div>Row 1</div><br><div>Row 2</div></div></body></html>"#;
    let doc = load_html(html, 800.0);

    let br = find_box(&doc.root, &|b| b.tag == "br").expect("br not found");
    assert!(
        br.layout.margin_rect.h > 0.0,
        "br in block context must have nonzero height, got {}",
        br.layout.margin_rect.h
    );
}
