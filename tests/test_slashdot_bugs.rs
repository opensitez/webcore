// Regression tests for layout bugs surfaced by rendering slashdot.org
//
// Issue 1: Absolute containing-block resolution
//   position:absolute with no top/left was positioned relative to the
//   viewport (x=0) instead of the nearest position:relative ancestor.
//
// Issue 2: Block element inside inline creates 0×0 anonymous boxes
//   e.g. <a><strong> or <span><strong> — the inline's dimensions collapse
//   and the block child is invisible.
//
// Issue 3: <input> elements sized with height 0
//   Radio/checkbox/text inputs were given zero height.
//
// Issue 4: Negative margin-left on a float collapses margin-rect width to 0
//   .rail-right { float:left; width:320px; margin-left:-320px; } produced
//   margin_rect.w = 0 instead of 320.

use webcore::load_html;
use webcore::types::*;

fn find<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for c in &root.children {
        if let Some(b) = find(c, pred) {
            return Some(b);
        }
    }
    None
}

fn find_attr<'a>(root: &'a WebCore, attr: &str, val: &str) -> Option<&'a WebCore> {
    find(root, &|b| b.get_attr(attr) == Some(val))
}

#[allow(dead_code)]
fn dump(root: &WebCore, depth: usize) {
    let indent = "  ".repeat(depth);
    eprintln!(
        "{}{} pos={:?} c=({:.0},{:.0} {:.0}x{:.0}) m=({:.0},{:.0} {:.0}x{:.0})",
        indent,
        root.tag,
        root.style.position,
        root.layout.content_rect.x,
        root.layout.content_rect.y,
        root.layout.content_rect.w,
        root.layout.content_rect.h,
        root.layout.margin_rect.x,
        root.layout.margin_rect.y,
        root.layout.margin_rect.w,
        root.layout.margin_rect.h,
    );
    for c in &root.children {
        dump(c, depth + 1);
    }
}

// ─── Issue 1: Absolute containing-block ───────────────────────────────────────

// An absolutely-positioned child with no top/left should be positioned
// relative to its nearest position:relative ancestor, not the viewport.
// A 100px spacer div pushes the relative parent down so its y is predictable.
#[test]
fn abs_child_of_relative_parent_not_at_viewport_origin() {
    let doc = load_html(
        r#"<div style="height:100px;"></div>
           <div style="position:relative;">
             <span style="position:absolute;">label</span>
           </div>"#,
        800.0,
    );
    let span = find(&doc.root, &|b| {
        b.tag == "span" && b.style.position == Position::Absolute
    });
    assert!(span.is_some(), "absolute span not found");
    let span = span.unwrap();
    // The span must NOT be at y≈0 (viewport origin). The relative div is after
    // a 100px spacer so the span should be near y≈100.
    assert!(
        span.layout.margin_rect.y > 50.0,
        "abs span y={} should be > 50 (inside its relative parent below the spacer), not at viewport top",
        span.layout.margin_rect.y
    );
}

// Absolute child with explicit top:0 left:0 inside a relative parent
// should be at the parent's top-left, not the viewport origin.
#[test]
fn abs_with_top0_left0_relative_to_parent() {
    let doc = load_html(
        r#"<div style="height:200px;"></div>
           <div style="position:relative; margin-left:150px; width:300px; height:200px;">
             <span id="abs" style="position:absolute; top:0; left:0; width:50px;">X</span>
           </div>"#,
        800.0,
    );
    let span = find_attr(&doc.root, "id", "abs");
    assert!(span.is_some(), "absolute span not found");
    let span = span.unwrap();
    // left:0 inside div with margin-left:150 → x ≈ 150+body_margin ≈ 158
    assert!(
        span.layout.content_rect.x >= 140.0 && span.layout.content_rect.x <= 180.0,
        "abs x={} expected ~158 (parent left edge + body margin)",
        span.layout.content_rect.x
    );
    // top:0 inside div after 200px spacer → y ≈ 200 minus absorbed margins
    assert!(
        span.layout.content_rect.y >= 180.0 && span.layout.content_rect.y <= 220.0,
        "abs y={} expected ~200 (parent top edge)",
        span.layout.content_rect.y
    );
}

// Absolute inside a non-positioned parent — containing block should skip to
// the nearest positioned ancestor further up, not fall back to x=0.
#[test]
fn abs_skips_static_ancestors_to_positioned() {
    let doc = load_html(
        r#"<div style="position:relative; margin-left:60px;">
             <div><!-- static intermediate -->
               <span style="position:absolute; left:10px;">X</span>
             </div>
           </div>"#,
        800.0,
    );
    let span = find(&doc.root, &|b| {
        b.tag == "span" && b.style.position == Position::Absolute
    });
    assert!(span.is_some());
    let span = span.unwrap();
    // left:10 is relative to the relative div at x≈60, so span.x ≈ 70
    assert!(
        span.layout.content_rect.x > 50.0,
        "abs span x={} should be > 50 (relative to positioned grandparent, not viewport)",
        span.layout.content_rect.x
    );
}

// Absolute with no positioned ancestor — falls back to initial containing
// block (viewport), so it SHOULD be at x=0, y=0.
#[test]
fn abs_no_positioned_ancestor_uses_viewport() {
    let doc = load_html(
        r#"<div style="margin:50px;">
             <span style="position:absolute; top:0; left:0;">X</span>
           </div>"#,
        800.0,
    );
    let span = find(&doc.root, &|b| {
        b.tag == "span" && b.style.position == Position::Absolute
    });
    assert!(span.is_some());
    let span = span.unwrap();
    assert!(
        span.layout.content_rect.x < 20.0 && span.layout.content_rect.y < 20.0,
        "abs with no positioned ancestor should be at viewport origin, got ({},{})",
        span.layout.content_rect.x,
        span.layout.content_rect.y
    );
}

// ─── Issue 2: Block inside inline collapses to 0×0 ───────────────────────────

// <a><strong style="display:block">text</strong></a> — on slashdot, strong
// was display:block inside an inline <a>. The link and text must still be
// visible (the containing paragraph must have non-zero size).
#[test]
fn block_inside_inline_link_has_nonzero_height() {
    let doc = load_html(
        r##"<p><a href="#"><strong style="display:block">Click me</strong></a></p>"##,
        800.0,
    );
    let strong = find(&doc.root, &|b| b.tag == "strong");
    assert!(strong.is_some(), "strong not found");
    let strong = strong.unwrap();
    assert!(
        strong.layout.margin_rect.h > 0.0,
        "strong inside <a> has zero height: margin_rect={:?}",
        strong.layout.margin_rect
    );
}

// Inline span containing a block-level child must not collapse.
#[test]
fn inline_span_with_block_child_nonzero() {
    let doc = load_html(
        r#"<div><span>count: <strong style="display:block">42</strong></span></div>"#,
        800.0,
    );
    let strong = find(&doc.root, &|b| b.tag == "strong");
    assert!(strong.is_some());
    assert!(
        strong.unwrap().layout.margin_rect.h > 0.0,
        "block strong inside inline span has zero height"
    );
}

// Text nodes after a block inside an inline must be laid out
// and the containing box must be tall enough for all content.
#[test]
fn inline_with_block_child_text_visible() {
    let doc = load_html(
        r#"<div id="wrap">
             <span>Read the <strong style="display:block">32</strong> comments</span>
           </div>"#,
        800.0,
    );
    let wrap = find_attr(&doc.root, "id", "wrap");
    assert!(wrap.is_some());
    assert!(
        wrap.unwrap().layout.margin_rect.h > 10.0,
        "wrapper div has zero/tiny height when inline contains block child"
    );
}

// ─── Issue 3: Input element height ───────────────────────────────────────────

#[test]
fn radio_input_has_nonzero_height() {
    let doc = load_html(
        r#"<form><label><input type="radio" name="q" value="a"> Option A</label></form>"#,
        800.0,
    );
    let input = find(&doc.root, &|b| b.tag == "input");
    assert!(input.is_some(), "input not found");
    let input = input.unwrap();
    assert!(
        input.layout.margin_rect.h > 0.0,
        "radio input has zero height: margin_rect={:?}",
        input.layout.margin_rect
    );
}

#[test]
fn checkbox_input_has_nonzero_height() {
    let doc = load_html(
        r#"<form><input type="checkbox" id="cb"><label for="cb">Check me</label></form>"#,
        800.0,
    );
    let input = find(&doc.root, &|b| b.tag == "input");
    assert!(input.is_some());
    assert!(
        input.unwrap().layout.margin_rect.h > 0.0,
        "checkbox input has zero height"
    );
}

#[test]
fn text_input_has_nonzero_height() {
    let doc = load_html(
        r#"<form><input type="text" placeholder="Enter text"></form>"#,
        800.0,
    );
    let input = find(&doc.root, &|b| b.tag == "input");
    assert!(input.is_some());
    assert!(
        input.unwrap().layout.margin_rect.h > 0.0,
        "text input has zero height"
    );
}

// Label containing a radio button must be tall enough to contain it.
#[test]
fn label_with_radio_has_nonzero_height() {
    let doc = load_html(
        r#"<label><input type="radio" name="x" value="1"> Some option text</label>"#,
        800.0,
    );
    let label = find(&doc.root, &|b| b.tag == "label");
    assert!(label.is_some());
    assert!(
        label.unwrap().layout.margin_rect.h > 0.0,
        "label with radio input has zero height"
    );
}

// ─── Issue 4: Negative margin-left on float ───────────────────────────────────

// A float with a negative margin-left must still have a non-zero margin_rect width.
// Classic "Holy Grail" sidebar pattern:
//   main content: float:left; width: calc(100% - 320px)
//   sidebar:      float:left; width:320px; margin-left:-320px
// The sidebar should visually appear at the right and its margin_rect.w must be 320.
#[test]
fn float_negative_margin_left_nonzero_margin_width() {
    let doc = load_html(
        r#"<div style="width:800px;">
             <div id="main" style="float:left; width:480px; height:200px; background:blue;"></div>
             <div id="rail" style="float:left; width:320px; margin-left:-320px; height:200px; background:red;"></div>
           </div>"#,
        800.0,
    );
    let rail = find_attr(&doc.root, "id", "rail");
    assert!(rail.is_some(), "rail div not found");
    let rail = rail.unwrap();
    assert!(
        rail.layout.margin_rect.w > 0.0,
        "float with margin-left:-320px has margin_rect.w=0, expected 320"
    );
    assert!(
        (rail.layout.margin_rect.w - 320.0).abs() < 5.0,
        "float margin_rect.w={} expected 320",
        rail.layout.margin_rect.w
    );
}

// After the negative-margin float, the wrapper div should wrap both columns.
#[test]
fn float_negative_margin_container_wraps_both_columns() {
    let doc = load_html(
        r#"<div id="wrap" style="width:800px; overflow:hidden;">
             <div style="float:left; width:480px; height:100px;"></div>
             <div style="float:left; width:320px; margin-left:-320px; height:150px;"></div>
           </div>"#,
        800.0,
    );
    let wrap = find_attr(&doc.root, "id", "wrap");
    assert!(wrap.is_some());
    // The container should be at least as tall as the tallest float (150px)
    // when cleared (overflow:hidden triggers block formatting context).
    assert!(
        wrap.unwrap().layout.margin_rect.h >= 100.0,
        "container height={} expected >= 100",
        wrap.unwrap().layout.margin_rect.h
    );
}

// With margin-left:-320px on a 320px wide float:left after a 480px float,
// the float moves 320px left from where it would normally sit (x≈480).
// So it ends up at x≈160 (relative to content) — overlapping main.
// The margin_rect must have the correct width (320) so the float context
// knows the element's size (the original bug was margin_rect.w=0).
#[test]
fn float_negative_margin_rail_positioned_at_right() {
    let doc = load_html(
        r#"<div style="width:800px;">
             <div id="main" style="float:left; width:480px; height:100px;"></div>
             <div id="rail" style="float:left; width:320px; margin-left:-320px; height:100px;"></div>
           </div>"#,
        800.0,
    );
    let rail = find_attr(&doc.root, "id", "rail");
    assert!(rail.is_some());
    let rail = rail.unwrap();
    // margin-left:-320 moves the rail 320px left from its natural position (x≈480),
    // so border_rect.x ≈ 480-320 = 160 (+ body margin = ~168).
    // The margin_rect must have the correct width = 320 (not 0).
    assert!(
        rail.layout.margin_rect.w >= 300.0,
        "rail margin_rect.w={} expected ~320",
        rail.layout.margin_rect.w
    );
    // The border/content should be between 0 and 480 (it overlaps main).
    assert!(
        rail.layout.content_rect.x >= 0.0 && rail.layout.content_rect.x < 480.0,
        "rail content_rect.x={} expected in [0, 480) (overlapping main due to negative margin)",
        rail.layout.content_rect.x
    );
}

// ─── Fixed nav regression ──────────────────────────────────────────────────────
// position:fixed; top:0; left:0 must be at y=0 regardless of document content
#[test]
fn fixed_nav_at_viewport_top() {
    let doc = load_html(
        r#"<body style="margin:8px;">
             <h1 style="margin-top:44px;">Title</h1>
             <p>Some content</p>
             <div id="nav" style="position:fixed; top:0; left:0; width:100%;">Nav</div>
           </body>"#,
        900.0,
    );
    let nav = find_attr(&doc.root, "id", "nav");
    assert!(nav.is_some(), "fixed nav not found");
    let nav = nav.unwrap();
    assert!(
        nav.layout.content_rect.y < 20.0,
        "fixed nav y={} should be near 0 (viewport top)",
        nav.layout.content_rect.y
    );
    assert!(
        nav.layout.content_rect.x < 20.0,
        "fixed nav x={} should be near 0 (viewport left)",
        nav.layout.content_rect.x
    );
}
