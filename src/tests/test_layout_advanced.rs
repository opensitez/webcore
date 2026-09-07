// Ported from tests/test_layout_advanced.cpp

use super::harness::*;
use crate::css::apply_property;
use crate::html::parse_html;
use crate::layout::LayoutEngine;
use crate::types::*;

// ── Min-Height / Max-Height Parsing ───────────────────────────────────────────

#[test]
fn layoutadv_min_height_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "min-height", "100px");
    assert_eq!(s.min_height.resolve(16.0, 0.0, 16.0), 100.0);
}

#[test]
fn layoutadv_max_height_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "max-height", "200px");
    assert_eq!(s.max_height.resolve(16.0, 0.0, 16.0), 200.0);
}

#[test]
fn margin_trim_block_start_removes_first_child_margin() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { margin-trim: block-start; border: 1px solid black; width: 100px; }
        .child { margin-top: 20px; height: 10px; }
        </style>
        <div class="box"><div id="child" class="child"></div></div>
        "#,
        200.0,
    );
    let parent = find_box(&doc.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"child".to_string())
    })
    .unwrap();
    assert!(
        (child.layout.border_rect.y - parent.layout.content_rect.y).abs() < 0.5,
        "first child top margin should be trimmed at the parent's block-start edge"
    );
}

#[test]
fn margin_trim_block_end_removes_last_child_margin_from_parent_height() {
    let mut renderer = crate::Renderer::new();
    let without_trim = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { border: 1px solid black; width: 100px; }
        .child { height: 10px; margin-bottom: 20px; }
        </style>
        <div class="box"><div class="child"></div></div>
        "#,
        200.0,
    );
    let with_trim = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { margin-trim: block-end; border: 1px solid black; width: 100px; }
        .child { height: 10px; margin-bottom: 20px; }
        </style>
        <div class="box"><div class="child"></div></div>
        "#,
        200.0,
    );
    let normal = find_box(&without_trim.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    let trimmed = find_box(&with_trim.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    assert!(
        normal.layout.border_rect.h - trimmed.layout.border_rect.h > 15.0,
        "block-end trimming should remove the last child bottom margin from parent height; normal={} trimmed={}",
        normal.layout.border_rect.h,
        trimmed.layout.border_rect.h
    );
}

#[test]
fn inline_replaced_image_uses_percent_width_and_css_aspect_ratio() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
        * { box-sizing: border-box; }
        .wrap { width: 320px; }
        img { width: 100%; aspect-ratio: 16 / 9; display: inline; }
        </style>
        <div class="wrap"><a><img id="pic" src=""></a></div>
        "#,
        800.0,
    );
    let img = find_box(&doc.root, &|b| {
        b.tag == "img" && b.attributes.get("id") == Some(&"pic".to_string())
    })
    .unwrap();
    assert!(
        (img.layout.border_rect.w - 320.0).abs() < 0.5,
        "inline replaced image should use the containing width; got {}",
        img.layout.border_rect.w
    );
    assert!(
        (img.layout.border_rect.h - 180.0).abs() < 0.5,
        "inline replaced image should transfer height from aspect-ratio; got {}",
        img.layout.border_rect.h
    );
}

#[test]
fn margin_trim_inline_start_removes_first_child_margin() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { margin-trim: inline-start; border: 1px solid black; width: 100px; }
        .child { margin-left: 20px; width: 10px; height: 10px; }
        </style>
        <div class="box"><div id="child" class="child"></div></div>
        "#,
        200.0,
    );
    let parent = find_box(&doc.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"child".to_string())
    })
    .unwrap();
    assert!(
        (child.layout.border_rect.x - parent.layout.content_rect.x).abs() < 0.5,
        "inline-start trimming should remove the first child start margin"
    );
}

#[test]
fn margin_trim_inline_end_removes_last_child_margin_from_scroll_width() {
    let mut renderer = crate::Renderer::new();
    let without_trim = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { width: 20px; overflow: auto; }
        .child { width: 30px; height: 10px; margin-right: 40px; }
        </style>
        <div class="box"><div class="child"></div></div>
        "#,
        200.0,
    );
    let with_trim = renderer.load_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box { margin-trim: inline-end; width: 20px; overflow: auto; }
        .child { width: 30px; height: 10px; margin-right: 40px; }
        </style>
        <div class="box"><div class="child"></div></div>
        "#,
        200.0,
    );
    let normal = find_box(&without_trim.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    let trimmed = find_box(&with_trim.root, &|b| {
        b.tag == "div" && b.attributes.get("class") == Some(&"box".to_string())
    })
    .unwrap();
    assert!(
        normal.layout.scroll_width - trimmed.layout.scroll_width > 35.0,
        "inline-end trimming should remove the last child end margin from scroll width; normal={} trimmed={}",
        normal.layout.scroll_width,
        trimmed.layout.scroll_width
    );
}

#[test]
fn text_wrap_balance_rebalances_last_line_width() {
    let html = |wrap: &str| {
        format!(
            r#"
            <style>
            * {{ margin: 0; padding: 0; }}
            #box {{
                width: 165px;
                font: 16px sans-serif;
                text-wrap: {wrap};
            }}
            </style>
            <div id="box">alpha beta gamma delta epsilon</div>
            "#
        )
    };
    let mut renderer = crate::Renderer::new();
    let normal_doc = renderer.load_html(&html("wrap"), 400.0);
    let balanced_doc = renderer.load_html(&html("balance"), 400.0);
    let normal = find_box(&normal_doc.root, &|b| {
        b.attributes.get("id") == Some(&"box".to_string())
    })
    .unwrap();
    let balanced = find_box(&balanced_doc.root, &|b| {
        b.attributes.get("id") == Some(&"box".to_string())
    })
    .unwrap();

    assert_eq!(normal.layout.line_cache.len(), 2);
    assert_eq!(balanced.layout.line_cache.len(), 2);
    let normal_last = normal.layout.line_cache.last().unwrap().width;
    let balanced_last = balanced.layout.line_cache.last().unwrap().width;
    assert!(
        balanced_last > normal_last + 20.0,
        "balanced text should avoid a short leftover line: normal={normal_last}, balanced={balanced_last}"
    );
}

#[test]
fn text_wrap_pretty_avoids_single_word_last_line_when_possible() {
    let html = |wrap: &str| {
        format!(
            r#"
            <style>
            * {{ margin: 0; padding: 0; }}
            #box {{
                width: 165px;
                font: 16px sans-serif;
                text-wrap: {wrap};
            }}
            </style>
            <div id="box">alpha beta gamma delta epsilon</div>
            "#
        )
    };
    let mut renderer = crate::Renderer::new();
    let normal_doc = renderer.load_html(&html("wrap"), 400.0);
    let pretty_doc = renderer.load_html(&html("pretty"), 400.0);
    let normal = find_box(&normal_doc.root, &|b| {
        b.attributes.get("id") == Some(&"box".to_string())
    })
    .unwrap();
    let pretty = find_box(&pretty_doc.root, &|b| {
        b.attributes.get("id") == Some(&"box".to_string())
    })
    .unwrap();

    assert_eq!(normal.layout.line_cache.len(), 2);
    assert_eq!(pretty.layout.line_cache.len(), 2);
    let normal_last = normal.layout.line_cache.last().unwrap().width;
    let pretty_last = pretty.layout.line_cache.last().unwrap().width;
    assert!(
        pretty_last > normal_last + 20.0,
        "pretty text should avoid a single-word final line when it can: normal={normal_last}, pretty={pretty_last}"
    );
}

#[test]
fn percentage_line_height_uses_font_size_for_line_boxes() {
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        h2 { font-size: 22px; line-height: 140%; }
        </style>
        <h2 id="title">Eugenics and the Nihilism of Law-and-Order Politics</h2>
        <p id="after">After</p>
        "#,
        360.0,
    );
    let title = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"title".to_string())
    })
    .unwrap();
    let after = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"after".to_string())
    })
    .unwrap();

    assert!(
        title.layout.content_rect.h > 30.0,
        "percentage line-height should reserve font-size-based line box height, got {}",
        title.layout.content_rect.h
    );
    assert!(
        after.layout.content_rect.y >= title.layout.content_rect.y + title.layout.content_rect.h,
        "following content must be placed after the heading line box; title={:?} after={:?}",
        title.layout.content_rect,
        after.layout.content_rect
    );
}

#[test]
fn variable_percentage_line_height_uses_font_size_for_line_boxes() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
        :root { --lh-title: 125%; }
        * { margin: 0; padding: 0; }
        .text { font-size: 24px; line-height: var(--lh-title); }
        </style>
        <h2 id="title" class="text">She Wrote Me a Letter</h2>
        <p id="after">After</p>
        "#,
        360.0,
    );
    let title = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"title".to_string())
    })
    .unwrap();
    let after = find_box(&doc.root, &|b| {
        b.attributes.get("id") == Some(&"after".to_string())
    })
    .unwrap();

    assert!(
        title.layout.content_rect.h >= 29.0,
        "var() percentage line-height should resolve against font size, got {}",
        title.layout.content_rect.h
    );
    assert!(
        after.layout.content_rect.y >= title.layout.content_rect.y + title.layout.content_rect.h,
        "following content must not overlap a var() percentage line-height heading"
    );
}

#[test]
fn layoutadv_min_height_percent() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "min-height", "50%");
    assert!(matches!(s.min_height, CssLength::Percent(_)));
    assert_eq!(s.min_height.resolve(16.0, 400.0, 16.0), 200.0);
}

#[test]
fn layoutadv_max_height_percent() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "max-height", "75%");
    assert!(matches!(s.max_height, CssLength::Percent(_)));
    assert_eq!(s.max_height.resolve(16.0, 400.0, 16.0), 300.0);
}

// ── Min-Height / Max-Height Layout Enforcement ────────────────────────────────

#[test]
fn layoutadv_min_height_enforced() {
    let doc = parse_and_layout(r#"<div style="min-height: 200px;">Short</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div"
            && !b.style.min_height.is_auto()
            && (b.style.min_height.resolve(16.0, 0.0, 16.0) - 200.0).abs() < 1.0
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.h >= 200.0);
}

#[test]
fn layoutadv_max_height_enforced() {
    let doc = parse_and_layout(
        r#"<div style="max-height: 50px; overflow: hidden;"><p>Line1</p><p>Line2</p><p>Line3</p><p>Line4</p><p>Line5</p></div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && !b.style.max_height.is_none()
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.h <= 50.0);
}

// ── Margin Collapsing ─────────────────────────────────────────────────────────

#[test]
fn layoutadv_margin_collapsing_positive() {
    let doc = parse_and_layout(
        r#"<div style="margin-bottom: 30px;">A</div><div style="margin-top: 20px;">B</div>"#,
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b| b.tag == "div");
    assert!(divs.len() >= 2);
    let content_gap = divs[1].layout.content_rect.y
        - (divs[0].layout.content_rect.y + divs[0].layout.content_rect.h);
    // Collapsed to max(30,20)=30, not 50
    assert!(
        content_gap < 45.0,
        "expected collapsed gap < 45, got {}",
        content_gap
    );
}

#[test]
fn layoutadv_margin_collapsing_equal() {
    let doc = parse_and_layout(
        r#"<div style="margin-bottom: 20px;">A</div><div style="margin-top: 20px;">B</div>"#,
        800.0,
    );
    let divs = find_all_boxes(&doc.root, &|b| b.tag == "div");
    assert!(divs.len() >= 2);
    let content_gap = divs[1].layout.content_rect.y
        - (divs[0].layout.content_rect.y + divs[0].layout.content_rect.h);
    // Should be ~20 (collapsed), not 40 (sum)
    assert!(
        content_gap < 35.0,
        "expected collapsed gap < 35, got {}",
        content_gap
    );
}

// ── Semantic HTML5 Elements ───────────────────────────────────────────────────

#[test]
fn layoutadv_article_is_block() {
    let doc = parse(r#"<article>Content</article>"#);
    let b = find_box(&doc.root, &|b| b.tag == "article");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_section_is_block() {
    let doc = parse(r#"<section>Content</section>"#);
    let b = find_box(&doc.root, &|b| b.tag == "section");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_header_is_block() {
    let doc = parse(r#"<header>Content</header>"#);
    let b = find_box(&doc.root, &|b| b.tag == "header");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_footer_is_block() {
    let doc = parse(r#"<footer>Content</footer>"#);
    let b = find_box(&doc.root, &|b| b.tag == "footer");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_nav_is_block() {
    let doc = parse(r#"<nav>Content</nav>"#);
    let b = find_box(&doc.root, &|b| b.tag == "nav");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_aside_is_block() {
    let doc = parse(r#"<aside>Content</aside>"#);
    let b = find_box(&doc.root, &|b| b.tag == "aside");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_main_is_block() {
    let doc = parse(r#"<main>Content</main>"#);
    let b = find_box(&doc.root, &|b| b.tag == "main");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_figure_is_block() {
    let doc = parse(r#"<figure><figcaption>Caption</figcaption></figure>"#);
    let b = find_box(&doc.root, &|b| b.tag == "figure");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

#[test]
fn layoutadv_figcaption_is_block() {
    let doc = parse(r#"<figure><figcaption>Caption</figcaption></figure>"#);
    let b = find_box(&doc.root, &|b| b.tag == "figcaption");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.display, Display::Block);
}

// ── Table VALIGN ──────────────────────────────────────────────────────────────

#[test]
fn layoutadv_table_valign_top() {
    let doc = parse(r#"<table><tr><td valign="top">Top</td></tr></table>"#);
    let cell = find_box(&doc.root, &|b| b.tag == "td");
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().style.vertical_align, VerticalAlign::Top);
}

#[test]
fn layoutadv_table_valign_middle() {
    let doc = parse(r#"<table><tr><td valign="middle">Mid</td></tr></table>"#);
    let cell = find_box(&doc.root, &|b| b.tag == "td");
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().style.vertical_align, VerticalAlign::Middle);
}

#[test]
fn layoutadv_table_valign_bottom() {
    let doc = parse(r#"<table><tr><td valign="bottom">Bot</td></tr></table>"#);
    let cell = find_box(&doc.root, &|b| b.tag == "td");
    assert!(cell.is_some());
    assert_eq!(cell.unwrap().style.vertical_align, VerticalAlign::Bottom);
}

// ── Display Inline-Block/Flex/Grid Parsing ────────────────────────────────────

#[test]
fn layoutadv_inline_block_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "display", "inline-block");
    assert_eq!(s.display, Display::InlineBlock);
}

#[test]
fn layoutadv_inline_flex_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "display", "inline-flex");
    assert_eq!(s.display, Display::InlineFlex);
}

#[test]
fn layoutadv_inline_grid_parsed() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "display", "inline-grid");
    assert_eq!(s.display, Display::InlineGrid);
}

// ── Nested Layout Smoke ───────────────────────────────────────────────────────

#[test]
fn layoutadv_deeply_nested_layout() {
    let doc = parse_and_layout(
        r#"<div style="padding: 10px;"><div style="margin: 5px; border: 1px solid black;"><div style="padding: 5px;"><p>Deeply nested</p></div></div></div>"#,
        800.0,
    );
    // Just verify it doesn't panic
    let _ = &doc.root;
}

#[test]
fn layoutadv_mixed_flow_layout() {
    let doc = parse_and_layout(
        r#"<div><div style="float: left; width: 200px;">Sidebar</div><div style="display: inline-block; width: 100px;">Inline</div><div>Block content</div></div>"#,
        800.0,
    );
    // Just verify it doesn't panic
    let _ = &doc.root;
}

// ── Child combinator + flex-basis applied via "> *" ──────────────────────────

#[test]
fn layoutadv_child_combinator_flex_basis_applied() {
    // ".grid > *" with "flex: 1 1 260px" must apply flex-basis to direct children
    // so they are laid out side by side, not one per row.
    let doc = parse_and_layout(
        r#"
        <style>
            .grid { display: flex; flex-wrap: wrap; gap: 20px; }
            .grid > * { flex: 1 1 260px; }
            .card { padding: 20px; }
        </style>
        <div class="grid">
            <div class="card">A</div>
            <div class="card">B</div>
            <div class="card">C</div>
        </div>
    "#,
        1024.0,
    );

    let cards: Vec<_> = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "card")
            .unwrap_or(false)
    });
    assert_eq!(cards.len(), 3);

    // All three cards should be on the same row (same y position).
    let y0 = cards[0].layout.margin_rect.y;
    assert!(
        (cards[1].layout.margin_rect.y - y0).abs() < 2.0,
        "card B should be on the same row as card A (child combinator not applying flex-basis)"
    );
    assert!(
        (cards[2].layout.margin_rect.y - y0).abs() < 2.0,
        "card C should be on the same row as card A"
    );

    // They should be side-by-side (x positions increasing).
    assert!(
        cards[1].layout.margin_rect.x > cards[0].layout.margin_rect.x + 50.0,
        "card B should be to the right of card A"
    );
    assert!(
        cards[2].layout.margin_rect.x > cards[1].layout.margin_rect.x + 50.0,
        "card C should be to the right of card B"
    );
}

// ── compute_intrinsic_width: auto margins must not inflate parent width ────────

#[test]
fn layoutadv_auto_margin_does_not_inflate_intrinsic_width() {
    // An element with "margin: 0 auto" inside a flex container should not
    // cause its parent's intrinsic width to be the full container width.
    // Before the fix, flex items defaulting to auto flex-basis would call
    // compute_intrinsic_width, which used margin_rect.x + margin_rect.w
    // even for auto-margin elements, giving a ~container-width result.
    let doc = parse_and_layout(
        r#"
        <style>
            .flex { display: flex; gap: 10px; }
            .box { padding: 10px; }
            .inner { width: 80px; height: 80px; margin: 0 auto; }
        </style>
        <div class="flex">
            <div class="box"><div class="inner"></div><p>A</p></div>
            <div class="box"><div class="inner"></div><p>B</p></div>
            <div class="box"><div class="inner"></div><p>C</p></div>
        </div>
    "#,
        900.0,
    );

    let boxes: Vec<_> = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "box")
            .unwrap_or(false)
    });
    assert_eq!(boxes.len(), 3);

    // All three flex items should be on the same row.
    let y0 = boxes[0].layout.margin_rect.y;
    assert!(
        (boxes[1].layout.margin_rect.y - y0).abs() < 2.0,
        "box B on same row as A — auto margin in child should not inflate intrinsic width"
    );
    assert!(
        (boxes[2].layout.margin_rect.y - y0).abs() < 2.0,
        "box C on same row as A"
    );

    // Each box should be much narrower than the full container.
    for b in &boxes {
        assert!(
            b.layout.margin_rect.w < 400.0,
            "auto-margin child must not inflate flex item intrinsic width to container width"
        );
    }
}

// ── Flex-stretch height on initial layout and after viewport resize ───────────

fn find_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if node.attributes.get("id").map(|s| s == id).unwrap_or(false) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(b) = find_by_id(child, id) {
            return Some(b);
        }
    }
    None
}

fn find_by_id_mut<'a>(node: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
    if node.attributes.get("id").map(|s| s == id).unwrap_or(false) {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(b) = find_by_id_mut(child, id) {
            return Some(b);
        }
    }
    None
}

/// Flex-stretch: sidebar fills the full viewport height on initial layout.
#[test]
fn flex_stretch_sidebar_fills_height_initial() {
    // A classic sidebar layout: row flex container at 100vh, sidebar stretches.
    let html = r#"<html><head><style>
        html, body { margin: 0; padding: 0; }
        .app  { display: flex; flex-direction: row; height: 100vh; }
        .side { width: 200px; background: navy; }
        .main { flex: 1; background: white; }
    </style></head><body>
        <div class="app">
            <div id="side" class="side"></div>
            <div id="main" class="main"></div>
        </div>
    </body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    let side = find_by_id(&doc.root, "side").expect("side");
    assert!(
        (side.layout.border_rect.h - 600.0).abs() < 2.0,
        "sidebar should stretch to 100vh=600px on initial layout, got {}",
        side.layout.border_rect.h
    );
}

/// Flex-stretch: sidebar correctly updates its height when the window is resized.
#[test]
fn flex_stretch_sidebar_updates_on_viewport_height_resize() {
    let html = r#"<html><head><style>
        html, body { margin: 0; padding: 0; }
        .app  { display: flex; flex-direction: row; height: 100vh; }
        .side { width: 200px; background: navy; }
        .main { flex: 1; background: white; }
    </style></head><body>
        <div class="app">
            <div id="side" class="side"></div>
            <div id="main" class="main"></div>
        </div>
    </body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();

    // Initial layout at 600px tall.
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    {
        let side = find_by_id(&doc.root, "side").expect("side");
        assert!(
            (side.layout.border_rect.h - 600.0).abs() < 2.0,
            "initial: sidebar should be 600px, got {}",
            side.layout.border_rect.h
        );
    }

    // Simulate window resize: taller viewport. Width unchanged so pruning would
    // incorrectly skip re-layout without the viewport_h guard.
    engine.viewport_h = 900.0;
    engine.layout(&mut doc, 800.0);

    {
        let side = find_by_id(&doc.root, "side").expect("side");
        assert!(
            (side.layout.border_rect.h - 900.0).abs() < 2.0,
            "after resize to 900px: sidebar should be 900px, got {}",
            side.layout.border_rect.h
        );
    }
}

/// Fixed-width elements should still be pruned (not re-laid out) when only
/// viewport width changes within the same column (regression guard).
#[test]
fn layout_pruning_still_active_on_width_only_resize() {
    // A fixed-width card inside a fluid container. When the viewport is widened,
    // the card's content width never changes — it should be pruned (not re-laid out).
    // We verify the card's position shifts correctly without failing.
    let html = r#"<html><head><style>
        html, body { margin: 0; padding: 0; }
        .wrap { display: flex; justify-content: center; }
        .card { width: 300px; height: 200px; background: blue; }
    </style></head><body>
        <div class="wrap">
            <div id="card" class="card"></div>
        </div>
    </body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 600.0);

    {
        let card = find_by_id(&doc.root, "card").expect("card");
        assert!(
            (card.layout.border_rect.w - 300.0).abs() < 2.0,
            "initial: card 300px wide"
        );
        assert!(
            (card.layout.border_rect.h - 200.0).abs() < 2.0,
            "initial: card 200px tall"
        );
    }

    // Widen viewport; card dimensions unchanged, only centering margin shifts.
    engine.layout(&mut doc, 1000.0);

    {
        let card = find_by_id(&doc.root, "card").expect("card");
        assert!(
            (card.layout.border_rect.w - 300.0).abs() < 2.0,
            "after width resize: card still 300px"
        );
        assert!(
            (card.layout.border_rect.h - 200.0).abs() < 2.0,
            "after width resize: card still 200px"
        );
    }
}

// ── Replaced-element intrinsic contribution (CSS2.1 §10.4) ────────────────────

/// Give every `<img>` under `node` the same natural dimensions; returns how
/// many were found.
fn set_natural_size(node: &mut WebCore, w: u32, h: u32) -> usize {
    let mut n = 0;
    if node.tag == "img" {
        node.image_width = w;
        node.image_height = h;
        n += 1;
    }
    for ch in &mut node.children {
        n += set_natural_size(ch, w, h);
    }
    n
}

/// **An image with a definite height contributes `height × ratio`, not its
/// natural width.** The intrinsic walk used to early-return the natural width,
/// so a 1024×1024 photo shown at `height=150` made its container 1024px wide
/// while the image itself laid out at 150×150.
#[test]
fn layoutadv_replaced_intrinsic_width_follows_definite_height() {
    let mut doc = parse("<div id=card><a><img height='150' src='x.png'></a></div>");
    assert_eq!(set_natural_size(&mut doc.root, 1024, 1024), 1);
    let engine = LayoutEngine::new();
    let card = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("card")
    })
    .expect("card");
    assert_eq!(engine.max_content_width(card, 16.0, 16.0), 150.0);
    assert_eq!(engine.min_content_width(card, 16.0, 16.0), 150.0);
}

/// A non-square ratio, so the assertion cannot pass by accident.
#[test]
fn layoutadv_replaced_intrinsic_width_uses_the_ratio() {
    let mut doc = parse("<div id=card><img height='100' src='x.png'></div>");
    assert_eq!(set_natural_size(&mut doc.root, 800, 200), 1);
    let engine = LayoutEngine::new();
    let card = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("card")
    })
    .expect("card");
    assert_eq!(engine.max_content_width(card, 16.0, 16.0), 400.0);
}

/// With both dimensions auto the natural width is still the answer.
#[test]
fn layoutadv_replaced_intrinsic_width_defaults_to_natural() {
    let mut doc = parse("<div id=card><img src='x.png'></div>");
    assert_eq!(set_natural_size(&mut doc.root, 640, 480), 1);
    let engine = LayoutEngine::new();
    let card = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("card")
    })
    .expect("card");
    assert_eq!(engine.max_content_width(card, 16.0, 16.0), 640.0);
}

#[test]
fn inline_block_image_wrapper_obeys_max_width_percent() {
    let mut doc = parse(
        "<style>
           #lead { width: 584px; }
           #wrap { display: inline-block; max-width: 100%; position: relative; }
           img { display: block; max-width: 100%; height: auto; }
         </style>
         <div id=lead><a id=wrap><img id=pic src=x.webp></a></div>",
    );
    assert_eq!(set_natural_size(&mut doc.root, 1312, 738), 1);

    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 1280.0);

    let wrap = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("wrap")
    })
    .expect("wrapper");
    assert!(
        (wrap.layout.content_rect.w - 584.0).abs() < 1.0,
        "inline-block image wrapper should be clamped by max-width:100%; got {}",
        wrap.layout.content_rect.w
    );
    assert!(
        (wrap.layout.content_rect.h - 329.0).abs() < 2.0,
        "inline-block image wrapper should keep the image aspect ratio after clamp; got {}",
        wrap.layout.content_rect.h
    );

    let pic = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("pic")
    })
    .expect("pic");
    assert!(
        (pic.layout.content_rect.w - 584.0).abs() < 1.0,
        "child image should be laid out at the clamped wrapper width; got {}",
        pic.layout.content_rect.w
    );
}

#[test]
fn dirty_loaded_image_reflows_fit_content_wrapper() {
    let mut doc = parse(
        "<style>
           #lead { --img-width: 100%; width: 584px; }
           #wrap { display: inline-block; position: relative; height: min-content; width: 100%; }
           img { display: block; width: var(--img-width, unset); height: auto; }
         </style>
         <div id=lead><a id=wrap><img id=pic src=x.webp></a></div>",
    );

    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 1280.0);
    assert!(
        (find_by_id(&doc.root, "wrap").unwrap().layout.content_rect.w - 584.0).abs() < 1.0,
        "width:100% wrapper should resolve against its container before image dimensions arrive"
    );

    let pic = find_by_id_mut(&mut doc.root, "pic").expect("pic");
    pic.image_width = 1312;
    pic.image_height = 738;
    pic.image_data = Some(std::sync::Arc::new(vec![0xff; 1312 * 738 * 4]));
    pic.layout.layout_dirty = true;
    pic.layout.cached_intrinsic_w.set(f32::NAN);
    pic.layout.intrinsic_dirty = true;

    engine.layout_no_cascade(&mut doc, 1280.0);

    let wrap = find_by_id(&doc.root, "wrap").expect("wrapper");
    assert!(
        (wrap.layout.content_rect.w - 584.0).abs() < 1.0,
        "loaded image should dirty ancestors and reflow wrapper to max-width:100%; got {}",
        wrap.layout.content_rect.w
    );
}

/// The end-to-end shape of the tikshbila.com gallery: wrapping flex items whose
/// only sizeable content is a photo shown at a fixed height.
#[test]
fn layoutadv_flex_items_size_to_the_displayed_image_not_the_photo() {
    let mut doc = parse(
        "<style>.row{display:flex;flex-wrap:wrap}</style>\
         <div class=row><div id=a><img height='150' src='1.png'></div>\
         <div id=b><img height='150' src='2.png'></div></div>",
    );
    assert_eq!(
        set_natural_size(&mut doc.root, 1024, 1024),
        2,
        "both images present"
    );
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 1256.0);
    let a = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("a")
    })
    .expect("a");
    let b = find_box(&doc.root, &|n: &WebCore| {
        n.attributes.get("id").map(String::as_str) == Some("b")
    })
    .expect("b");
    assert_eq!(a.layout.margin_rect.w, 150.0, "item a");
    assert_eq!(b.layout.margin_rect.w, 150.0, "item b");
    assert_eq!(
        b.layout.margin_rect.y, a.layout.margin_rect.y,
        "same flex line"
    );
}

/// **Inline nesting must not lose the spaces between words.** The max-content
/// width of `<a><span>Faire un don</span></a>` has to equal that of the bare
/// text; fr.wikipedia's header links were sized ~7px short — two space widths —
/// so each one broke onto a second line inside a box built for one.
#[test]
fn layoutadv_max_content_width_survives_inline_nesting() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        "<div id=plain>Faire un don</div>\
         <div id=nested><a><span>Faire un don</span></a></div>",
        800.0,
    );
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let engine = renderer.layout_engine();
    let plain = engine.max_content_width(find("plain"), 16.0, 16.0);
    let nested = engine.max_content_width(find("nested"), 16.0, 16.0);
    assert!(plain > 40.0, "the text was measured at all: {plain}");
    assert!(
        (plain - nested).abs() < 0.5,
        "nesting changed the max-content width: plain {plain} vs nested {nested}"
    );
}

/// **The intrinsic measurement must count the spaces between words.** A
/// max-content width that measures `"Faire un don"` as if it were
/// `"Faireundon"` is two space widths short of the line the breaker then
/// builds, so the text wraps inside a box sized for exactly one line.
#[test]
fn layoutadv_max_content_width_counts_inter_word_spaces() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        "<div id=spaced>Faire un don</div><div id=joined>Faireundon</div>",
        800.0,
    );
    let engine = renderer.layout_engine();
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let spaced = engine.max_content_width(find("spaced"), 16.0, 16.0);
    let joined = engine.max_content_width(find("joined"), 16.0, 16.0);
    assert!(
        spaced > joined + 4.0,
        "the two spaces were not measured: 'Faire un don' {spaced} vs 'Faireundon' {joined}"
    );
}

#[test]
fn layoutadv_max_content_width_counts_css_text_spacing() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
        #plain, #spaced { display: inline-block; font-size: 16px; }
        #spaced { letter-spacing: 4px; word-spacing: 10px; }
        </style>
        <div id=plain>a b</div><div id=spaced>a b</div>
    "#,
        800.0,
    );
    let engine = renderer.layout_engine();
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let plain = engine.max_content_width(find("plain"), 16.0, 16.0);
    let spaced = engine.max_content_width(find("spaced"), 16.0, 16.0);
    let added = spaced - plain;
    assert!(
        (21.5..=23.0).contains(&added),
        "three letters/spaces at 4px plus one word gap at 10px should add 22px; \
         plain={plain} spaced={spaced} added={added}"
    );
}

// ── Multi-column: a wrapper must not collapse every column into one ──────────

/// Column positions of the leaf items, left to right.
fn column_xs(root: &WebCore, class: &str) -> Vec<f32> {
    let mut xs = Vec::new();
    fn walk(n: &WebCore, class: &str, xs: &mut Vec<f32>) {
        if n.attributes
            .get("class")
            .map_or(false, |c| c.split_whitespace().any(|w| w == class))
        {
            xs.push(n.layout.margin_rect.x);
        }
        for c in &n.children {
            walk(c, class, xs);
        }
    }
    walk(root, class, &mut xs);
    xs
}

/// **Multi-column is a fragmentation container, not a round-robin over direct
/// children.** fr.wikipedia wraps both of its multicol blocks in a single
/// `<div>` — `column-count:3` over the community links and `column-count:5`
/// over the sister projects — so distributing the container's own children put
/// everything in column 1 and the lists rendered one item per line.
#[test]
fn layoutadv_multicol_distributes_through_a_wrapper() {
    let mut renderer = crate::renderer::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>* { margin:0; padding:0 } .cols { column-count: 3; column-gap: 10px; width: 600px }\
         .item { height: 20px }</style>\
         <div class=cols><div class=wrap>\
           <div class=item>a</div><div class=item>b</div><div class=item>c</div>\
           <div class=item>d</div><div class=item>e</div><div class=item>f</div>\
         </div></div>",
        800.0);
    let mut pm = tiny_skia::Pixmap::new(800, 300).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    let xs = column_xs(&doc.root, "item");
    assert_eq!(xs.len(), 6, "all six items are present");
    let mut distinct: Vec<f32> = xs.clone();
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap());
    distinct.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(
        distinct.len(),
        3,
        "items should occupy 3 column positions, got {distinct:?} from {xs:?}"
    );
}

/// The direct-children case must keep working.
#[test]
fn layoutadv_multicol_still_distributes_direct_children() {
    let mut renderer = crate::renderer::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>* { margin:0; padding:0 } .cols { column-count: 3; column-gap: 10px; width: 600px }\
         .item { height: 20px }</style>\
         <div class=cols>\
           <div class=item>a</div><div class=item>b</div><div class=item>c</div>\
           <div class=item>d</div><div class=item>e</div><div class=item>f</div>\
         </div>",
        800.0);
    let mut pm = tiny_skia::Pixmap::new(800, 300).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    let xs = column_xs(&doc.root, "item");
    let mut distinct = xs.clone();
    distinct.sort_by(|a, b| a.partial_cmp(b).unwrap());
    distinct.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(distinct.len(), 3, "got {distinct:?} from {xs:?}");
}

/// **`column-gap: normal` computes to 1em in a multi-column container**
/// (css-multicol-1 §4.2). Our initial value was `Zero`, indistinguishable from
/// an author writing `column-gap: 0`, so every multicol block that did not set
/// a gap rendered with its columns touching.
#[test]
fn layoutadv_multicol_default_gap_is_one_em() {
    let mut renderer = crate::renderer::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>* { margin:0; padding:0 } \
         .cols { column-count: 3; width: 600px; font-size: 16px } .item { height: 20px }</style>\
         <div class=cols>\
           <div class=item>a</div><div class=item>b</div><div class=item>c</div>\
         </div>",
        800.0,
    );
    let mut pm = tiny_skia::Pixmap::new(800, 200).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    let xs = {
        let mut v = column_xs(&doc.root, "item");
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        v
    };
    assert_eq!(xs.len(), 3, "three columns, got {xs:?}");
    // col_w = (600 - 2*16) / 3 = 189.33; column starts 0, 205.33, 410.67.
    let step = xs[1] - xs[0];
    assert!(
        (step - 205.33).abs() < 1.0,
        "a 1em gap gives a 205.33px column pitch, got {step} from {xs:?}"
    );
}

/// **max-content is the content laid out with no soft wrap taken**
/// (css-sizing-3 §5.1), so inline-level siblings that share a line are SUMMED.
/// Taking the MAX over a block's children measured `Hello <b>World</b>` as the
/// wider single word, and any shrink-to-fit box sized from it then wrapped.
#[test]
fn layoutadv_max_content_sums_inline_siblings() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        "<div id=split>Hello <span>World</span></div><div id=whole>Hello World</div>",
        800.0,
    );
    let engine = renderer.layout_engine();
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let split = engine.max_content_width(find("split"), 16.0, 16.0);
    let whole = engine.max_content_width(find("whole"), 16.0, 16.0);
    assert!(whole > 40.0, "the control measured something: {whole}");
    // Within one space width: the text node's own max-content collapses its
    // trailing space, which is a separate (known) gap in cross-node whitespace.
    assert!(
        split > whole - 6.0 && split <= whole + 1.0,
        "inline siblings must sum on one line: split={split} vs whole={whole}"
    );
}

/// A block-level child still starts a new line, so it is MAXed, not summed.
#[test]
fn layoutadv_max_content_maxes_block_siblings() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        "<div id=blocks><div>Hello</div><div>World</div></div><div id=one>Hello</div>",
        800.0,
    );
    let engine = renderer.layout_engine();
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let blocks = engine.max_content_width(find("blocks"), 16.0, 16.0);
    let one = engine.max_content_width(find("one"), 16.0, 16.0);
    assert!(
        (blocks - one).abs() < 6.0,
        "two block children stack, so max-content is the wider one: {blocks} vs {one}"
    );
}

/// **`box-sizing: border-box` means the specified width ALREADY includes
/// padding and border** (css-sizing-3 §6.2). The intrinsic walk returned the
/// raw `width` and its caller then added the child's padding/border on top, so
/// a border-box child made its shrink-to-fit parent that much too wide. With
/// `* { box-sizing: border-box }` in nearly every real stylesheet, this hit
/// almost every float, inline-block and fit-content box.
#[test]
fn layoutadv_border_box_width_is_not_double_counted() {
    let mut renderer = crate::renderer::Renderer::new();
    let doc = renderer.load_html(
        "<div id=bb><div style='box-sizing:border-box;width:200px;padding:20px'>x</div></div>\
         <div id=cb><div style='box-sizing:content-box;width:160px;padding:20px'>x</div></div>",
        800.0,
    );
    let engine = renderer.layout_engine();
    let find = |id: &str| {
        fn walk<'a>(n: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap()
    };
    let bb = engine.max_content_width(find("bb"), 16.0, 16.0);
    let cb = engine.max_content_width(find("cb"), 16.0, 16.0);
    assert_eq!(
        bb, 200.0,
        "border-box: the 200px already includes the 40px padding"
    );
    assert_eq!(
        cb, 200.0,
        "content-box: 160 + 40 padding = 200 (the control)"
    );
}

/// **css-sizing-3 §6.1 — the `fit-content(<length>)` function form must
/// parse and clamp.** `value_parse.rs:24` only recognises the bare
/// `fit-content` keyword string; `fit-content(200px)` falls through to
/// `parse_length_inner` and comes back `Auto` (confirmed directly:
/// `parse_length("fit-content(200px)")` → `Auto` against the clean worktree).
/// As a `width` that is indistinguishable from plain `width:auto` — and a
/// block box with `width:auto` fills its containing block rather than
/// shrink-to-fitting — so the box comes out at the full available width
/// (884px in this fixture's 900px viewport) instead of clamped to 200.
///
/// The fixture's content is many short repeated words so max-content is far
/// above 200px and min-content is far below it regardless of the exact font
/// metrics — the clamp value 200 is the only number that can come out
/// correctly, so no Chrome measurement or pixel tolerance for text is
/// needed; a generous tolerance is kept only for layout rounding.
///
/// Expectation source: the fit-content(X) formula itself (X clamped between
/// min- and max-content, both of which this fixture keeps clear of 200 by a
/// wide margin), not Chrome.
/// Destination: `src/tests/test_layout_advanced.rs`.
#[test]
fn fit_content_function_form_clamps_the_box() {
    let html = r#"
        <div id="box" style="width:fit-content(200px);">
          <p style="margin:0">word word word word word word word word word word word word word word word</p>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let b = find_by_id(&doc.root, "box").unwrap();
    assert!(
        (b.layout.border_rect.w - 200.0).abs() < 3.0,
        "fit-content(200px) should clamp the box to ~200px, got {}",
        b.layout.border_rect.w
    );
}

/// **css-sizing-3 §6.1 — `min-width: max-content` must expand the box to its
/// content's max-content size, not collapse it.** `CssLength::resolve_vp`
/// (`src/types/length.rs:138`) returns `0.0` for `MinContent`/`MaxContent`/
/// `FitContent`, and `is_auto()` (`:150`) reports `true` for all three, so
/// `block.rs:404`'s `min_w` is always 0 for these keywords — a `width:0` box
/// with `min-width:max-content` stays at 0 instead of growing to fit.
///
/// Two fixed-width inline-blocks (no text) so the expected max-content is
/// exact arithmetic (50+50=100), matching the convention already used by
/// `grid_min_content_track_uses_the_min_content_contribution` in
/// `test_grid.rs` for the same reason: no font-dependent measurement.
///
/// Expectation source: spec arithmetic (sum of the two fixed widths), not Chrome.
/// Destination: `src/tests/test_layout_advanced.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): this fixture
/// currently measures W=0.
#[test]
fn min_width_max_content_expands_a_zero_width_box() {
    let html = r#"
        <div id="box" style="width:0px; min-width:max-content;">
          <span style="display:inline-block; width:50px; height:10px"></span><span style="display:inline-block; width:50px; height:10px"></span>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let b = find_by_id(&doc.root, "box").unwrap();
    assert!(
        (b.layout.border_rect.w - 100.0).abs() < 0.5,
        "min-width:max-content on a width:0 box should give 100 (50+50), got {}",
        b.layout.border_rect.w
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Multi-column (css-multicol-1, css-break-3)
// ═══════════════════════════════════════════════════════════════════════════

/// **css-multicol-1 §7, §8.2 — `column-fill: auto` with a constrained
/// `height` must overflow into a second column once the first is full.**
/// `layout_columns` (`block.rs:1034`) takes `_rbox` (leading underscore: truly
/// unused) and never reads any height constraint; with `balance = false`
/// (`column-fill:auto`), `target_col_h` is set to `f32::MAX`
/// (`block.rs:1090`) unconditionally, so the "does this overflow the column"
/// check at `:1119` can never fire. Content stacks in column 1 and spills
/// past the container's own height instead of flowing into column 2.
///
/// Expectation source: spec behaviour (a full column moves to the next),
/// not Chrome — this only needs the item count in column 1 vs. column 2 to
/// differ, not exact pixel positions.
/// Destination: `src/tests/test_layout_advanced.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): all four
/// items currently land at the same x (8, 8, 8, 8) — one distinct column
/// position, not two.
#[test]
fn column_fill_auto_overflows_a_full_column() {
    let html = r#"
        <div style="column-fill:auto; column-count:2; height:50px; width:200px; column-gap:0;">
          <div class="item" style="height:20px">a</div>
          <div class="item" style="height:20px">b</div>
          <div class="item" style="height:20px">c</div>
          <div class="item" style="height:20px">d</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let items = crate::tests::harness::find_all_boxes(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c == "item")
            .unwrap_or(false)
    });
    let mut xs: Vec<f32> = items.iter().map(|b| b.layout.margin_rect.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(xs.len(), 2,
        "a full first column (height:50, 2×20px items) must overflow into a second column, got {} distinct x positions", xs.len());
}

/// **css-break-3 §3 — `break-before: column` must force a new column,
/// independent of whether the current column has room.** A grep of
/// `block.rs` for `break_inside`/`break_before`/`break_after` returns zero
/// hits — the properties are parsed and cascaded but consulted nowhere in
/// `layout_columns`.
///
/// `column-fill:auto` keeps this isolated from the balancing algorithm (a
/// separate gap, tested above/below): with no height constraint and no
/// forced break, everything would naturally stay in column 1, so any move
/// to column 2 here can only be the forced break firing.
///
/// Expectation source: spec behaviour (forced break starts a new column),
/// not Chrome.
/// Destination: `src/tests/test_layout_advanced.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): item `b`
/// currently lands at the same x as `a` (x=8), stacked directly below it.
#[test]
fn break_before_column_forces_a_new_column() {
    let html = r#"
        <div style="column-fill:auto; column-count:3; width:300px; column-gap:0;">
          <div id="a" style="height:10px">a</div>
          <div id="b" style="break-before:column; height:10px">b</div>
          <div id="c" style="height:10px">c</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let a = find_by_id(&doc.root, "a").unwrap();
    let b = find_by_id(&doc.root, "b").unwrap();
    assert!(
        (b.layout.margin_rect.x - a.layout.margin_rect.x).abs() > 1.0,
        "break-before:column on b must move it to a new column (a.x={}, b.x={})",
        a.layout.margin_rect.x,
        b.layout.margin_rect.x
    );
}

#[test]
fn break_inside_avoid_keeps_plain_wrapper_in_one_column() {
    let html = r#"
        <div style="column-fill:auto; column-count:2; height:50px; width:200px; column-gap:0;">
          <div id="group" style="break-inside:avoid">
            <div id="a" style="height:30px">a</div>
            <div id="b" style="height:30px">b</div>
          </div>
          <div id="c" style="height:10px">c</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let a = find_by_id(&doc.root, "a").unwrap();
    let b = find_by_id(&doc.root, "b").unwrap();
    assert!(
        (a.layout.margin_rect.x - b.layout.margin_rect.x).abs() < 0.5,
        "break-inside:avoid group children must stay in the same column, got a.x={} b.x={}",
        a.layout.margin_rect.x,
        b.layout.margin_rect.x
    );
}

/// **css-multicol-1 §7 — balancing must not leave a column empty while
/// unplaced content remains.** The algorithm in `block.rs:1087`–`1132` uses
/// one global average (`total_content_h / n_cols`) with a flat 1.1 slack and
/// checks *before* placing each item whether it alone would overshoot —
/// including the very first item in a column. A single item taller than the
/// average blows the slack on the FIRST column before anything has been
/// placed there, so the whole column is skipped and every remaining item
/// (including that first, tall one) piles into the last column.
///
/// This asserts the invariant the naive-average approach violates — every
/// column receives some content — rather than a specific pixel split. A real
/// shortest-fit balance could legitimately choose several different splits
/// (and this engine is not a fragmentation container, so it cannot split the
/// 100px item across columns the way Chrome might); what no correct
/// algorithm does is leave a column at zero while content is left over.
///
/// Expectation source: the invariant itself (no starved column), not a
/// specific Chrome layout — deliberately robust to whichever correct
/// algorithm the fix picks.
/// Destination: `src/tests/test_layout_advanced.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): all five
/// items currently land at the same x (column 2); column 1 gets nothing.
#[test]
fn balance_never_starves_a_column() {
    let html = r#"
        <div style="column-count:2; width:200px; column-gap:0;">
          <div class="item" style="height:100px">a</div>
          <div class="item" style="height:1px">b</div>
          <div class="item" style="height:1px">c</div>
          <div class="item" style="height:1px">d</div>
          <div class="item" style="height:1px">e</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let items = crate::tests::harness::find_all_boxes(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c == "item")
            .unwrap_or(false)
    });
    let mut xs: Vec<f32> = items.iter().map(|b| b.layout.margin_rect.x).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(xs.len(), 2,
        "with 104px of content over 2 columns, both columns must receive some content, got {} distinct x positions (all content in one column)", xs.len());
}

/// **css-multicol-1 §3.4 step 11 — column width is `max(0, …)`, not
/// `max(1px, …)`.** `block.rs:1071`: `let col_w = ((content_w - total_gaps) /
/// n_cols as f32).max(1.0);`. With more columns/gap than available width the
/// formula goes negative and the spec floors it at 0; this floors it at 1px
/// instead, so every column paints/measures 1px wider than it should in that
/// (admittedly extreme) case.
///
/// Expectation source: spec arithmetic — `(5 - 9×10) / 10 = -8.5`, floored to
/// 0 by the spec and to 1 by the engine — not Chrome.
/// Destination: `src/tests/test_layout_advanced.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): the column
/// items currently measure width 1.0, not 0.0.
#[test]
fn column_width_floors_at_zero_not_one_pixel() {
    let html = r#"
        <div style="column-count:10; column-gap:10px; width:5px;">
          <div id="a">a</div><div id="b">b</div><div id="c">c</div>
        </div>
    "#;
    let doc = parse_and_layout(html, 900.0);
    let a = find_by_id(&doc.root, "a").unwrap();
    assert!(
        a.layout.border_rect.w < 0.5,
        "(5 - 9×10)/10 = -8.5 must floor at 0, not 1px, got {}",
        a.layout.border_rect.w
    );
}
