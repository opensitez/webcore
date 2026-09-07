// Ported from cpptests/test_cascade.cpp
// CSS Cascade Priority Tests: UA stylesheet < author <style> < inline style=""

use webcore::css::apply_property;
use webcore::layout::LayoutEngine;
use webcore::types::*;
use webcore::{load_html, parse_html};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parse(html: &str) -> Document {
    parse_html(html)
}

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

fn walk_boxes<F: FnMut(&WebCore)>(root: &WebCore, visitor: &mut F) {
    visitor(root);
    for child in &root.children {
        walk_boxes(child, visitor);
    }
}

fn count_boxes<F: Fn(&WebCore) -> bool>(root: &WebCore, pred: &F) -> usize {
    let mut n = if pred(root) { 1 } else { 0 };
    for child in &root.children {
        n += count_boxes(child, pred);
    }
    n
}

fn doc_text(doc: &Document) -> String {
    doc.root.text_content()
}

// ============================================================
// UA stylesheet defaults
// ============================================================

#[test]
fn cascade_heading_is_bold() {
    let doc = parse_and_layout("<h1>Title</h1>", 800.0);
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some(), "h1 box not found");
    assert!(h1.unwrap().style.font_weight.is_bold());
}

#[test]
fn cascade_paragraph_has_margin() {
    let doc = parse_and_layout("<p>text</p>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some(), "p box not found");
    let s = &p.unwrap().style;
    assert!(!s.margin_top.is_none(), "p should have top margin");
    assert!(!s.margin_bottom.is_none(), "p should have bottom margin");
}

// ============================================================
// Author <style> overrides UA
// ============================================================

#[test]
fn cascade_author_style_overrides_ua_heading() {
    let doc = parse("<style>h1 { font-weight: normal; }</style><h1>Title</h1>");
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some());
    assert!(!h1.unwrap().style.font_weight.is_bold());
}

#[test]
fn cascade_author_class_overrides_ua() {
    let doc = parse(
        "<style>.compact { margin-top: 0px; margin-bottom: 0px; }</style>\
         <p class=\"compact\">text</p>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "p"
            && b.attributes
                .get("class")
                .map(|v| v == "compact")
                .unwrap_or(false)
    });
    assert!(p.is_some());
    let s = &p.unwrap().style;
    assert_eq!(s.margin_top, CssLength::Px(0.0));
    assert_eq!(s.margin_bottom, CssLength::Px(0.0));
}

// ============================================================
// Inline style="" overrides everything
// ============================================================

#[test]
fn cascade_inline_style_overrides_author() {
    let doc = parse(
        "<style>div { color: red; }</style>\
         <div style=\"color: green;\">text</div>",
    );
    let div = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(div.is_some());
    assert_eq!(div.unwrap().style.color, Color::rgb(0, 128, 0));
}

#[test]
fn cascade_inline_style_overrides_ua() {
    let doc = parse("<h1 style=\"font-weight: normal;\">Title</h1>");
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some());
    assert!(!h1.unwrap().style.font_weight.is_bold());
}

#[test]
fn cascade_inline_style_overrides_author_and_ua() {
    let doc = parse(
        "<style>p { margin-top: 50px; }</style>\
         <p style=\"margin-top: 5px;\">text</p>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.margin_top, CssLength::Px(5.0));
}

// ============================================================
// Color inheritance
// ============================================================

#[test]
fn cascade_color_inherited_from_parent() {
    let doc = parse("<div style=\"color: purple;\"><p>text</p></div>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(128, 0, 128));
}

#[test]
fn cascade_inline_style_beats_inheritance() {
    let doc = parse("<div style=\"color: red;\"><p style=\"color: blue;\">text</p></div>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(0, 0, 255));
}

// ============================================================
// Specificity within author styles
// ============================================================

#[test]
fn cascade_class_beats_tag() {
    let doc = parse(
        "<style>p { color: blue; } .red { color: red; }</style>\
         <p class=\"red\">text</p>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "p"
            && b.attributes
                .get("class")
                .map(|v| v == "red")
                .unwrap_or(false)
    });
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(255, 0, 0));
}

#[test]
fn cascade_id_beats_class() {
    let doc = parse(
        "<style>.red { color: red; } #special { color: green; }</style>\
         <p class=\"red\" id=\"special\">text</p>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("id")
            .map(|v| v == "special")
            .unwrap_or(false)
    });
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(0, 128, 0));
}

// ============================================================
// Font inheritance
// ============================================================

#[test]
fn cascade_font_inherited_from_parent() {
    let doc = parse("<div style=\"font-size: 20px;\"><p>text</p></div>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.font_size, CssLength::Px(20.0));
}

#[test]
fn cascade_inline_style_font_overrides_inheritance() {
    let doc = parse("<div style=\"font-size: 20px;\"><p style=\"font-size: 10px;\">text</p></div>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.font_size, CssLength::Px(10.0));
}

// ============================================================
// Multiple levels of nesting
// ============================================================

#[test]
fn cascade_deep_inheritance() {
    let doc = parse(
        "<div style=\"color: red;\">\
           <div><div><p>deep</p></div></div>\
         </div>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(255, 0, 0));
}

#[test]
fn cascade_deep_override() {
    let doc = parse(
        "<div style=\"color: red;\">\
           <div style=\"color: blue;\"><p>text</p></div>\
         </div>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color, Color::rgb(0, 0, 255));
}

// ============================================================
// Named CSS color tests
// ============================================================

#[test]
fn cascade_named_color_red() {
    let doc = parse("<div style=\"color: red;\">r</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.color, Color::rgb(255, 0, 0));
}

#[test]
fn cascade_named_color_navy() {
    let doc = parse("<div style=\"color: navy;\">n</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.color, Color::rgb(0, 0, 128));
}

#[test]
fn cascade_named_color_darkred() {
    let doc = parse("<div style=\"color: darkred;\">t</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.color, Color::rgb(139, 0, 0));
}

#[test]
fn cascade_named_color_green() {
    let doc = parse("<div style=\"color: green;\">g</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.color, Color::rgb(0, 128, 0));
}

#[test]
fn cascade_named_color_white() {
    let doc = parse("<div style=\"color: white;\">w</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.color, Color::rgb(255, 255, 255));
}

#[test]
fn cascade_named_color_in_border() {
    let doc = parse("<p style=\"border: 2px solid blue;\">b</p>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    let s = &p.unwrap().style;
    assert_eq!(s.border_top_color, Color::rgb(0, 0, 255));
    assert_eq!(s.border_top_width, CssLength::Px(2.0));
}

// ============================================================
// UA stylesheet element defaults
// ============================================================

#[test]
fn cascade_th_is_bold_and_center() {
    let doc = parse("<table><tr><th>H</th></tr></table>");
    let th = find_box(&doc.root, &|b: &WebCore| b.tag == "th");
    assert!(th.is_some());
    let s = &th.unwrap().style;
    assert!(s.font_weight.is_bold());
    assert_eq!(s.text_align, TextAlign::Center);
}

#[test]
fn cascade_pre_has_white_space_pre() {
    let doc = parse("<pre>  spaces  </pre>");
    let pre = find_box(&doc.root, &|b: &WebCore| b.tag == "pre");
    assert!(pre.is_some());
    assert_eq!(pre.unwrap().style.white_space, WhiteSpace::Pre);
}

#[test]
fn cascade_center_has_text_align_center() {
    let doc = parse("<center>text</center>");
    let c = find_box(&doc.root, &|b: &WebCore| b.tag == "center");
    assert!(c.is_some());
    assert_eq!(c.unwrap().style.text_align, TextAlign::Center);
}

// ============================================================
// Clip-path polygon re-application (no double points)
// ============================================================

#[test]
fn cascade_clip_path_polygon_no_duplication() {
    let doc = parse("<div style='clip-path: polygon(50% 0%, 100% 100%, 0% 100%);'>T</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.clip_path.points.len(), 3);
}

// ============================================================
// CSS Margin Collapsing Tests (CSS 2.1 §8.3.1)
// ============================================================

#[test]
fn cascade_sibling_margins_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-bottom: 20px;'>A</div>\
         <div style='margin-top: 30px;'>B</div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let a = divs[0];
    let b = divs[1];
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // collapsed = max(20, 30) = 30, not sum (50)
    assert!(gap <= 35.0, "gap {gap} should be <= 35 (collapsed)");
    assert!(gap < 45.0, "gap {gap} should be < 45 (not stacked)");
}

#[test]
fn cascade_sibling_negative_margins_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-bottom: -10px;'>A</div>\
         <div style='margin-top: -20px;'>B</div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let a = divs[0];
    let b = divs[1];
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    assert!(gap < 0.0, "gap {gap} should be negative (overlap)");
    assert!(gap >= -25.0);
}

#[test]
fn cascade_sibling_mixed_margins_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-bottom: 30px;'>A</div>\
         <div style='margin-top: -10px;'>B</div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let a = divs[0];
    let b = divs[1];
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // collapsed = 30 + (-10) = 20
    assert!(gap >= 15.0 && gap <= 25.0, "gap {gap} should be ~20");
}

// ============================================================
// Parent-first-child margin collapsing
// ============================================================

#[test]
fn cascade_parent_first_child_top_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-top: 10px;'><div style='margin-top: 40px;'>child</div></div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let parent = divs[0];
    let child = divs[1];
    // Parent-first-child collapse: child's margin-top (40px) collapses with parent's (10px).
    // Collapsed margin = max(10, 40) = 40. Body margin (8px) also collapses with parent.
    // Final: child.y = max(8, 10, 40) = 40 (all margins collapse through).
    // Child should be at same y as parent (no internal gap from margin).
    assert!(
        (child.layout.content_rect.y - parent.layout.content_rect.y).abs() < 5.0,
        "child should be at parent's content edge, child.y={:.0} parent.y={:.0}",
        child.layout.content_rect.y,
        parent.layout.content_rect.y
    );
    assert!(parent.layout.collapsed_margin_top >= 35.0);
}

#[test]
fn cascade_parent_first_child_blocked_by_padding() {
    let doc = parse_and_layout(
        "<div style='margin-top: 10px; padding-top: 5px;'>\
         <div style='margin-top: 40px;'>child</div></div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let parent = divs[0];
    let child = divs[1];
    assert!(child.layout.content_rect.y >= 35.0);
    assert!(parent.layout.collapsed_margin_top <= 15.0);
}

#[test]
fn cascade_parent_first_child_blocked_by_border() {
    let doc = parse_and_layout(
        "<div style='margin-top: 10px; border-top: 1px solid black;'>\
         <div style='margin-top: 40px;'>child</div></div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let child = divs[1];
    assert!(child.layout.content_rect.y >= 35.0);
}

// ============================================================
// Parent-last-child bottom collapsing
// ============================================================

#[test]
fn cascade_parent_last_child_bottom_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-bottom: 10px;'>\
         <div style='margin-bottom: 40px;'>child</div></div>\
         <div>after</div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(!divs.is_empty());
    let parent = divs[0];
    assert!(parent.layout.collapsed_margin_bottom >= 35.0);
}

// ============================================================
// BFC prevents collapsing
// ============================================================

#[test]
fn cascade_overflow_hidden_blocks_collapsing() {
    let doc = parse_and_layout(
        "<div style='margin-top: 10px; overflow: hidden;'>\
         <div style='margin-top: 40px;'>child</div></div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(divs.len() >= 2);
    let parent = divs[0];
    let child = divs[1];
    assert!(child.layout.content_rect.y >= 35.0);
    assert!(parent.layout.collapsed_margin_top <= 15.0);
}

// ============================================================
// Empty block collapsing
// ============================================================

#[test]
fn cascade_empty_block_margins_collapse() {
    let doc = parse_and_layout(
        "<div style='margin-top: 20px; margin-bottom: 30px;'></div>\
         <div>after</div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(!divs.is_empty());
    let empty = divs[0];
    assert!(empty.layout.collapsed_margin_top >= 25.0);
    assert_eq!(empty.layout.collapsed_margin_bottom, 0.0);
}

#[test]
fn cascade_empty_block_with_border_not_empty() {
    let doc = parse_and_layout(
        "<div style='margin-top: 20px; margin-bottom: 30px; \
         border: 1px solid black;'></div>",
        800.0,
    );
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    let d = d.unwrap();
    assert!(d.layout.collapsed_margin_top >= 15.0 && d.layout.collapsed_margin_top <= 25.0);
    assert!(d.layout.collapsed_margin_bottom >= 25.0);
}

// ============================================================
// Heading margins from UA stylesheet
// ============================================================

#[test]
fn cascade_heading_has_margins() {
    let doc = parse_and_layout("<h1>Title</h1>", 800.0);
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some());
    let s = &h1.unwrap().style;
    assert!(!s.margin_top.is_none());
    assert!(!s.margin_bottom.is_none());
}

// ============================================================
// Grandchild margin pass-through
// ============================================================

#[test]
fn cascade_grandchild_margin_pass_through() {
    let doc = parse_and_layout(
        "<div style='margin-top: 5px;'>\
           <div>\
             <div style='margin-top: 50px;'>deep</div>\
           </div>\
         </div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(!divs.is_empty());
    let outer = divs[0];
    assert!(outer.layout.collapsed_margin_top >= 45.0);
}

// ============================================================
// Page-break CSS properties
// ============================================================

#[test]
fn cascade_page_break_before_parsed() {
    let doc = parse("<style>h2 { page-break-before: always; }</style><h2>Heading</h2>");
    let h2 = find_box(&doc.root, &|b: &WebCore| b.tag == "h2");
    assert!(h2.is_some());
    assert_eq!(h2.unwrap().style.break_before, BreakValue::Always);
}

#[test]
fn cascade_page_break_after_avoid() {
    let doc = parse("<style>h3 { page-break-after: avoid; }</style><h3>Heading</h3>");
    let h3 = find_box(&doc.root, &|b: &WebCore| b.tag == "h3");
    assert!(h3.is_some());
    assert_eq!(h3.unwrap().style.break_after, BreakValue::Avoid);
}

#[test]
fn cascade_break_inside_avoid() {
    let doc = parse(
        "<style>.card { break-inside: avoid; }</style>\
         <div class=\"card\">content</div>",
    );
    let card = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div"
            && b.attributes
                .get("class")
                .map(|v| v == "card")
                .unwrap_or(false)
    });
    assert!(card.is_some());
    assert_eq!(card.unwrap().style.break_inside, BreakInside::Avoid);
}

#[test]
fn cascade_orphans_widows_parsed() {
    let doc = parse("<style>p { orphans: 4; widows: 3; }</style><p>text</p>");
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    let s = &p.unwrap().style;
    assert_eq!(s.orphans, 4);
    assert_eq!(s.widows, 3);
}

#[test]
fn cascade_inline_page_break_before() {
    let doc = parse("<div style=\"page-break-before: always;\">content</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.break_before, BreakValue::Always);
}

#[test]
fn cascade_break_before_page_value() {
    let doc = parse("<style>div { break-before: page; }</style><div>content</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.break_before, BreakValue::Always);
}

// ============================================================
// Cascade re-apply preserves parser attributes
// ============================================================

#[test]
fn cascade_reapply_preserves_dir_attribute() {
    let doc = parse("<div dir=\"rtl\">text</div>");
    let d = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(d.is_some());
    assert_eq!(d.unwrap().style.direction, Direction::RTL);
}

// ============================================================
// UA stylesheet defaults — link color
// ============================================================

#[test]
fn cascade_link_gets_ua_blue() {
    // Links should get blue color from UA stylesheet
    let doc = parse_and_layout("<a href=\"http://example.com\">link</a>", 800.0);
    let mut found_blue = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() && run.style.color == Color::rgb(0, 0, 238) {
                found_blue = true;
            }
        }
    });
    assert!(found_blue, "link run should have UA blue color");
}

#[test]
fn cascade_author_style_overrides_ua() {
    // Author <style> a { color: red } should override UA a { color: blue }
    let doc = parse_and_layout(
        "<style>a { color: red; }</style><a href=\"#\">link</a>",
        800.0,
    );
    let mut found_red = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() && run.style.color == Color::rgb(255, 0, 0) {
                found_red = true;
            }
        }
    });
    assert!(
        found_red,
        "link run should have author red color overriding UA blue"
    );
}

#[test]
fn cascade_link_color_beats_body_color() {
    // UA a{color:blue} should beat body's inherited text color
    let doc = parse_and_layout(
        "<body style=\"color: #2c3e50;\"><a href=\"#\">link</a></body>",
        800.0,
    );
    let mut found_link_run = false;
    let mut not_body_color = true;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() {
                found_link_run = true;
                // Should NOT be the body's inherited color
                if run.style.color == Color::rgb(0x2c, 0x3e, 0x50) {
                    not_body_color = false;
                }
            }
        }
    });
    assert!(found_link_run, "should find a link run");
    assert!(
        not_body_color,
        "link color should not be body's inherited color"
    );
}

#[test]
fn cascade_inline_style_on_link_beats_ua() {
    // <a style="color:white"> should beat UA a{color:blue}
    let doc = parse_and_layout(
        "<div style=\"background: #2c3e50;\"><a href=\"#\" style=\"color: white;\">Nav</a></div>",
        800.0,
    );
    let mut found = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() && run.style.color == Color::rgb(255, 255, 255) {
                found = true;
            }
        }
    });
    assert!(found, "link with inline style should have white color");
}

// ============================================================
// CSS matching on inline elements
// ============================================================

#[test]
fn cascade_span_class_styled() {
    // <style>.highlight { color: green }</style> should match <span class="highlight">
    let doc = parse_and_layout(
        "<style>.highlight { color: green; }</style>\
         <p>normal <span class=\"highlight\">green</span> normal</p>",
        800.0,
    );
    let mut found_green = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.color == Color::rgb(0, 128, 0) {
                found_green = true;
            }
        }
    });
    assert!(
        found_green,
        "span with .highlight class should have green color"
    );
}

#[test]
fn cascade_em_gets_css_italic() {
    // UA stylesheet sets em { font-style: italic }
    let doc = parse_and_layout("<p>normal <em>italic</em> normal</p>", 800.0);
    let mut found_italic = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_style == FontStyle::Italic {
                found_italic = true;
            }
        }
    });
    assert!(found_italic, "em run should be italic");
}

#[test]
fn cascade_strong_gets_css_bold() {
    // UA stylesheet sets strong { font-weight: bold }
    let doc = parse_and_layout("<p>normal <strong>bold</strong> normal</p>", 800.0);
    let mut found_bold = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_weight.is_bold() {
                found_bold = true;
            }
        }
    });
    assert!(found_bold, "strong run should be bold");
}

#[test]
fn cascade_inline_class_specificity() {
    // Class on inline element should override tag-level rule
    let doc = parse_and_layout(
        "<style>span { color: red; } .blue { color: blue; }</style>\
         <p><span class=\"blue\">text</span></p>",
        800.0,
    );
    let mut found_blue = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.color == Color::rgb(0, 0, 255) {
                found_blue = true;
            }
        }
    });
    assert!(found_blue, "class .blue should override span tag rule");
}

#[test]
fn cascade_nested_inline_elements() {
    // <a><em>text</em></a> — em should be italic AND have link href
    let doc = parse_and_layout(
        "<p><a href=\"http://test.com\"><em>linked italic</em></a></p>",
        800.0,
    );
    let mut found_linked_italic = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() && run.style.font_style == FontStyle::Italic {
                found_linked_italic = true;
            }
        }
    });
    assert!(
        found_linked_italic,
        "em inside a should be italic and have href"
    );
}

// ============================================================
// Block box run style propagation
// ============================================================

#[test]
fn cascade_h1_run_gets_bold_font() {
    // After cascade, the h1's inline runs must have bold + large font
    let doc = parse_and_layout("<h1>Heading</h1>", 800.0);
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some(), "h1 not found");
    let h1 = h1.unwrap();
    assert!(
        !h1.layout.inline_runs.is_empty(),
        "h1 should have inline runs"
    );
    assert!(
        h1.layout.inline_runs[0].style.font_weight.is_bold(),
        "h1 run should be bold"
    );
    // h1 font-size is 2em = 32px (default 16px * 2)
    let font_px = match h1.layout.inline_runs[0].style.font_size {
        CssLength::Px(px) => px,
        _ => 0.0,
    };
    assert!(
        font_px > 20.0,
        "h1 run font-size should be > 20px, got {font_px}"
    );
}

#[test]
fn cascade_h2_run_gets_bold_font() {
    let doc = parse_and_layout("<h2>Sub</h2>", 800.0);
    let h2 = find_box(&doc.root, &|b: &WebCore| b.tag == "h2");
    assert!(h2.is_some(), "h2 not found");
    let h2 = h2.unwrap();
    assert!(
        !h2.layout.inline_runs.is_empty(),
        "h2 should have inline runs"
    );
    assert!(
        h2.layout.inline_runs[0].style.font_weight.is_bold(),
        "h2 run should be bold"
    );
    let font_px = match h2.layout.inline_runs[0].style.font_size {
        CssLength::Px(px) => px,
        _ => 0.0,
    };
    assert!(
        font_px > 14.0,
        "h2 run font-size should be > 14px, got {font_px}"
    );
}

#[test]
fn cascade_pre_run_gets_monospace() {
    let doc = parse_and_layout("<pre>code</pre>", 800.0);
    let pre = find_box(&doc.root, &|b: &WebCore| b.tag == "pre");
    assert!(pre.is_some(), "pre not found");
    let pre = pre.unwrap();
    assert!(
        !pre.layout.inline_runs.is_empty(),
        "pre should have inline runs"
    );
    assert_eq!(pre.layout.inline_runs[0].style.font_family, "monospace");
}

#[test]
fn cascade_block_run_inherits_color() {
    // p inside colored div: p's runs must have the inherited color
    let doc = parse_and_layout("<div style=\"color: red;\"><p>text</p></div>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some(), "p not found");
    let p = p.unwrap();
    assert!(
        !p.layout.inline_runs.is_empty(),
        "p should have inline runs"
    );
    assert_eq!(p.layout.inline_runs[0].style.color, Color::rgb(255, 0, 0));
}

#[test]
fn cascade_body_text_color_inherits_to_runs() {
    // body text="" attribute color should reach runs
    let doc = parse_and_layout("<body text=\"#2c3e50\"><p>text</p></body>", 800.0);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some(), "p not found");
    let p = p.unwrap();
    assert!(
        !p.layout.inline_runs.is_empty(),
        "p should have inline runs"
    );
    assert_eq!(
        p.layout.inline_runs[0].style.color,
        Color::rgb(0x2c, 0x3e, 0x50)
    );
}

// ============================================================
// Inline element styles in flattened runs
// ============================================================

#[test]
fn cascade_inline_span_color_in_flattened_run() {
    let doc = parse_and_layout("<p>a <span style=\"color: red;\">b</span> c</p>", 800.0);
    let mut found_red = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.color == Color::rgb(255, 0, 0) {
                found_red = true;
            }
        }
    });
    assert!(found_red, "span with red color should produce a red run");
}

#[test]
fn cascade_bold_tag_run_in_flattened_run() {
    let doc = parse_and_layout("<p>a <b>bold</b> c</p>", 800.0);
    let mut found_bold = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_weight.is_bold() {
                found_bold = true;
            }
        }
    });
    assert!(found_bold, "b tag should produce a bold run");
}

#[test]
fn cascade_italic_tag_run_in_flattened_run() {
    let doc = parse_and_layout("<p>a <i>ital</i> c</p>", 800.0);
    let mut found_italic = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_style == FontStyle::Italic {
                found_italic = true;
            }
        }
    });
    assert!(found_italic, "i tag should produce an italic run");
}

#[test]
fn cascade_underline_tag_run_in_flattened_run() {
    let doc = parse_and_layout("<p>a <u>under</u> c</p>", 800.0);
    let mut found_underline = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.text_decoration.underline {
                found_underline = true;
            }
        }
    });
    assert!(found_underline, "u tag should produce an underlined run");
}

#[test]
fn cascade_strike_tag_run_in_flattened_run() {
    let doc = parse_and_layout("<p>a <s>strike</s> c</p>", 800.0);
    let mut found_strike = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.text_decoration.strikethrough {
                found_strike = true;
            }
        }
    });
    assert!(found_strike, "s tag should produce a strikethrough run");
}

#[test]
fn cascade_code_tag_run_gets_monospace() {
    let doc = parse_and_layout("<p>a <code>mono</code> c</p>", 800.0);
    let mut found_mono = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_family == "monospace" {
                found_mono = true;
            }
        }
    });
    assert!(found_mono, "code tag should produce a monospace run");
}

#[test]
fn cascade_mark_tag_run_gets_yellow_bg() {
    // UA stylesheet sets mark { background-color: yellow }
    // Check on the mark element's own style (background-color is not inherited by #text children)
    let doc = parse_and_layout("<p>a <mark>hi</mark> c</p>", 800.0);
    let mark = find_box(&doc.root, &|b: &WebCore| b.tag == "mark");
    assert!(mark.is_some(), "mark element not found");
    assert_eq!(
        mark.unwrap().style.background_color,
        Color::rgb(255, 255, 0),
        "mark should have yellow background from UA"
    );
}

#[test]
fn cascade_named_color_yellow() {
    // UA mark rule sets background-color: yellow — check on the mark element directly
    let doc = parse_and_layout("<mark>text</mark>", 800.0);
    let mark = find_box(&doc.root, &|b: &WebCore| b.tag == "mark");
    assert!(mark.is_some(), "mark element not found");
    assert_eq!(
        mark.unwrap().style.background_color,
        Color::rgb(255, 255, 0),
        "mark element should have yellow background from UA"
    );
}

#[test]
fn cascade_link_run_gets_url_and_blue() {
    let doc = parse_and_layout("<p><a href=\"http://x\">link</a></p>", 800.0);
    let mut found = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if !run.style.href.is_empty() {
                assert_eq!(
                    run.style.color,
                    Color::rgb(0, 0, 238),
                    "link should be UA blue"
                );
                assert!(
                    run.style.text_decoration.underline,
                    "link should be underlined"
                );
                found = true;
            }
        }
    });
    assert!(found, "should find a link run with href");
}

// ============================================================
// Font sub-property inheritance
// ============================================================

#[test]
fn cascade_bold_does_not_prevent_font_size_inheritance() {
    // b { font-weight: bold } should NOT prevent font-size inheritance
    let doc = parse_and_layout(
        "<div style=\"font-size: 20px;\"><p><b>big bold</b></p></div>",
        800.0,
    );
    let mut found = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_weight.is_bold() {
                let px = match run.style.font_size {
                    CssLength::Px(px) => px,
                    _ => 0.0,
                };
                if (px - 20.0).abs() < 1.0 {
                    found = true;
                }
            }
        }
    });
    assert!(found, "bold run should still inherit 20px font-size");
}

#[test]
fn cascade_italic_does_not_prevent_font_family_inheritance() {
    // <pre><i>mono italic</i></pre> — should be both italic and monospace
    let doc = parse_and_layout("<pre><i>mono italic</i></pre>", 800.0);
    let mut found = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_style == FontStyle::Italic && run.style.font_family == "monospace" {
                found = true;
            }
        }
    });
    assert!(found, "italic inside pre should be italic AND monospace");
}

#[test]
fn cascade_monospace_does_not_prevent_bold_inheritance() {
    // <b><code>x</code></b> — code gets monospace from UA, bold from parent
    let doc = parse_and_layout("<p><b><code>x</code></b></p>", 800.0);
    let mut found = false;
    walk_boxes(&doc.root, &mut |b: &WebCore| {
        for run in &b.layout.inline_runs {
            if run.style.font_weight.is_bold() && run.style.font_family == "monospace" {
                found = true;
            }
        }
    });
    assert!(found, "code inside b should be bold AND monospace");
}

// ============================================================
// UA stylesheet element defaults (additional)
// ============================================================

#[test]
fn cascade_hr_has_border() {
    let doc = parse_and_layout("<hr>", 800.0);
    let hr = find_box(&doc.root, &|b: &WebCore| b.tag == "hr");
    assert!(hr.is_some(), "hr not found");
    let s = &hr.unwrap().style;
    // UA now uses border-top: 1px solid silver (not inset)
    assert_eq!(s.border_top_style, BorderStyle::Solid);
    let w = match s.border_top_width {
        CssLength::Px(px) => px,
        _ => 0.0,
    };
    assert!(w >= 1.0, "hr border-top-width should be >= 1px");
}

#[test]
fn cascade_blockquote_has_margins() {
    let doc = parse_and_layout("<blockquote>q</blockquote>", 800.0);
    let bq = find_box(&doc.root, &|b: &WebCore| b.tag == "blockquote");
    assert!(bq.is_some(), "blockquote not found");
    let s = &bq.unwrap().style;
    assert!(
        !s.margin_left.is_none(),
        "blockquote should have left margin"
    );
    assert!(
        !s.margin_right.is_none(),
        "blockquote should have right margin"
    );
    // margin-left: 40px
    if let CssLength::Px(v) = s.margin_left {
        assert!(v > 0.0);
    }
    if let CssLength::Px(v) = s.margin_right {
        assert!(v > 0.0);
    }
}

#[test]
fn cascade_th_run_is_bold() {
    let doc = parse_and_layout("<table><tr><th>H</th></tr></table>", 800.0);
    let th = find_box(&doc.root, &|b: &WebCore| b.tag == "th");
    assert!(th.is_some(), "th not found");
    let th = th.unwrap();
    assert!(
        !th.layout.inline_runs.is_empty(),
        "th should have inline runs"
    );
    assert!(
        th.layout.inline_runs[0].style.font_weight.is_bold(),
        "th run should be bold"
    );
}

#[test]
fn cascade_sub_has_vertical_align_sub() {
    // UA stylesheet: sub { vertical-align: sub }
    // Check on the sub element's own style (vertical-align is not inherited by #text children)
    let doc = parse_and_layout("<p>x<sub>2</sub></p>", 800.0);
    let sub = find_box(&doc.root, &|b: &WebCore| b.tag == "sub");
    assert!(sub.is_some(), "sub element not found");
    assert_eq!(
        sub.unwrap().style.vertical_align,
        VerticalAlign::Sub,
        "sub should have vertical-align: sub from UA"
    );
}

#[test]
fn cascade_sup_has_vertical_align_super() {
    // UA stylesheet: sup { vertical-align: super }
    // Check on the sup element's own style (vertical-align is not inherited by #text children)
    let doc = parse_and_layout("<p>x<sup>2</sup></p>", 800.0);
    let sup = find_box(&doc.root, &|b: &WebCore| b.tag == "sup");
    assert!(sup.is_some(), "sup element not found");
    assert_eq!(
        sup.unwrap().style.vertical_align,
        VerticalAlign::Super,
        "sup should have vertical-align: super from UA"
    );
}

#[test]
fn cascade_ua_stylesheet_headings_break_avoid() {
    // UA stylesheet adds break-after: avoid; break-inside: avoid to headings
    let doc = parse_and_layout("<h1>Title</h1><h2>Sub</h2>", 800.0);
    let h1 = find_box(&doc.root, &|b: &WebCore| b.tag == "h1");
    assert!(h1.is_some(), "h1 not found");
    assert_eq!(h1.unwrap().style.break_after, BreakValue::Avoid);
    assert_eq!(h1.unwrap().style.break_inside, BreakInside::Avoid);

    let h2 = find_box(&doc.root, &|b: &WebCore| b.tag == "h2");
    assert!(h2.is_some(), "h2 not found");
    assert_eq!(h2.unwrap().style.break_after, BreakValue::Avoid);
    assert_eq!(h2.unwrap().style.break_inside, BreakInside::Avoid);
}

// ============================================================
// BFC: inline-block blocks collapsing
// ============================================================

#[test]
fn cascade_inline_block_blocks_collapsing() {
    // display: inline-block establishes BFC — no parent-child margin collapsing
    let doc = parse_and_layout(
        "<div style='margin-top: 10px; display: inline-block;'>\
         <div style='margin-top: 40px;'>child</div></div>",
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(!divs.is_empty(), "should find divs");
    let parent = divs[0];
    assert!(
        parent.layout.collapsed_margin_top <= 15.0,
        "inline-block should not collapse margin through BFC"
    );
}

#[test]
fn cascade_float_before_first_child_blocks_collapse() {
    // A float before the first block child prevents parent-first-child collapsing
    let doc = parse_and_layout(
        "<div style='margin-top: 10px;'>\
           <div style='float: left; width: 50px; height: 50px;'>F</div>\
           <div style='margin-top: 40px;'>child</div>\
         </div>",
        800.0,
    );
    // Find the outer non-floated div
    let parent = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && matches!(b.style.float, Float::None)
    });
    assert!(parent.is_some(), "outer non-float div not found");
    let parent = parent.unwrap();
    // Float breaks collapsing — parent keeps its own 10px, not child's 40px
    assert!(
        parent.layout.collapsed_margin_top <= 15.0,
        "float should block margin collapsing"
    );
}

#[test]
fn cascade_heading_margins_collapse_with_siblings() {
    // Adjacent heading margins collapse (not stack)
    let doc = parse_and_layout("<h3>Three</h3><h4>Four</h4>", 800.0);
    let h3 = find_box(&doc.root, &|b: &WebCore| b.tag == "h3");
    let h4 = find_box(&doc.root, &|b: &WebCore| b.tag == "h4");
    assert!(h3.is_some(), "h3 not found");
    assert!(h4.is_some(), "h4 not found");
    let h3 = h3.unwrap();
    let h4 = h4.unwrap();
    let gap = h4.layout.content_rect.y - (h3.layout.content_rect.y + h3.layout.content_rect.h);
    // Gap should be > 0 (some margin between headings)
    assert!(
        gap > 0.0,
        "there should be a gap between h3 and h4, got {gap}"
    );
}

// ============================================================
// Media queries — skipped: not evaluated during cascade in Rust
// ============================================================
// cascade_media_query_reapply — SKIPPED: media conditions stored but not evaluated
// cascade_media_print_type    — SKIPPED: MediaType not implemented in Rust cascade
// cascade_media_screen_type   — SKIPPED: MediaType not implemented in Rust cascade

// ============================================================
// Pseudo-element selector isolation
// ============================================================

#[test]
fn pseudo_element_webkit_scrollbar_not_applied_to_elements() {
    // ::-webkit-scrollbar { width: 6px } must NOT set width:6px on html/body
    let doc = parse_and_layout(
        r#"<style>
            ::-webkit-scrollbar { width: 6px; }
            ::-webkit-scrollbar-track { background: transparent; }
            ::-webkit-scrollbar-thumb { background: #ccc; border-radius: 3px; }
        </style><p>Hello</p>"#,
        900.0,
    );
    // The root (html/body) should be viewport-wide, not 6px
    assert!(
        doc.root.layout.content_rect.w > 100.0,
        "root unexpectedly narrow ({}) — ::-webkit-scrollbar leaked to real elements",
        doc.root.layout.content_rect.w
    );
    let body = find_box(&doc.root, &|b: &WebCore| b.tag == "body");
    assert!(body.is_some(), "body not found");
    assert!(
        body.unwrap().layout.content_rect.w > 100.0,
        "body width {} — ::-webkit-scrollbar leaked to body",
        body.unwrap().layout.content_rect.w
    );
}

#[test]
fn pseudo_element_selector_does_not_match_real_elements() {
    // Any unknown ::pseudo-element rule should be silently ignored for real elements
    let doc = parse_and_layout(
        r#"<style>
            ::selection { background: blue; }
            ::placeholder { color: red; }
            div::marker { color: green; }
            p { color: black; }
        </style><div><p id="p1">Text</p></div>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|s| s == "p1").unwrap_or(false)
    });
    assert!(p.is_some(), "p#p1 not found");
    // p should have black color (from `p { color: black }`), not red from ::placeholder
    assert_eq!(
        p.unwrap().style.color,
        Color::rgb(0, 0, 0),
        "::placeholder color leaked to <p>"
    );
}

// ============================================================
// Viewport units (vh / vw)
// ============================================================

#[test]
fn vh_resolves_to_viewport_height() {
    // height: 100vh on a block should equal the viewport height passed to the engine
    let doc = webcore::load_html_vp(
        r#"<div id="box" style="height:100vh; background:red;"></div>"#,
        900.0,
        600.0,
    );
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    });
    assert!(b.is_some(), "box not found");
    let h = b.unwrap().layout.border_rect.h;
    assert!(
        (h - 600.0).abs() < 2.0,
        "height:100vh should be 600px (viewport_h=600), got {h}"
    );
}

#[test]
fn vw_resolves_to_viewport_width() {
    let doc = webcore::load_html_vp(
        r#"<div id="box" style="width:50vw; height:10px;"></div>"#,
        800.0,
        600.0,
    );
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|s| s == "box").unwrap_or(false)
    });
    assert!(b.is_some(), "box not found");
    let w = b.unwrap().layout.border_rect.w;
    assert!(
        (w - 400.0).abs() < 2.0,
        "width:50vw should be 400px (viewport_w=800), got {w}"
    );
}

#[test]
fn vh_on_flex_item_resolves_correctly() {
    // A flex item with height:100vh in a column flex container should get full viewport height
    let doc = webcore::load_html_vp(
        r#"<style>
            body { display:flex; flex-direction:column; height:100vh; margin:0; }
            #app  { flex:1; }
        </style><div id="app"></div>"#,
        900.0,
        800.0,
    );
    let app = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|s| s == "app").unwrap_or(false)
    });
    assert!(app.is_some(), "app not found");
    let h = app.unwrap().layout.border_rect.h;
    assert!(
        h > 700.0,
        "#app with flex:1 in 100vh body should fill ~800px, got {h}"
    );
}

#[test]
fn three_column_flex_layout_with_vh() {
    // Regression: email-style three-column layout should lay out all three columns
    let doc = webcore::load_html_vp(
        r#"<style>
            body { display:flex; flex-direction:column; height:100vh; margin:0; }
            .app  { display:flex; flex-direction:row; flex:1; }
            .sidebar { width:200px; flex-shrink:0; background:#f5f5f5; }
            .main    { flex:1; }
        </style>
        <div class="app">
          <div id="sidebar" class="sidebar">Sidebar</div>
          <div id="main"    class="main">Main</div>
        </div>"#,
        900.0,
        800.0,
    );
    let sidebar = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("id")
            .map(|s| s == "sidebar")
            .unwrap_or(false)
    });
    let main = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|s| s == "main").unwrap_or(false)
    });
    assert!(sidebar.is_some(), "sidebar not found");
    assert!(main.is_some(), "main not found");
    let sw = sidebar.unwrap().layout.border_rect.w;
    let mw = main.unwrap().layout.border_rect.w;
    assert!(
        (sw - 200.0).abs() < 2.0,
        "sidebar width should be 200px, got {sw}"
    );
    assert!(
        (mw - 700.0).abs() < 2.0,
        "main should fill remaining 700px, got {mw}"
    );
}
