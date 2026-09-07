// Ported from cpptests/test_layout_advanced.cpp
// Advanced layout tests: min/max height, margin collapsing, semantic HTML5,
// table valign, display inline-flex/grid.
//
// DeeplyNestedLayout and MixedFlowLayout use wxBitmap/wxMemoryDC — SKIPPED.

use webcore::css::apply_property;
use webcore::types::*;
use webcore::{load_html, parse_html};

fn style_with(prop: &str, val: &str) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, prop, val);
    style
}

fn parse(html: &str) -> Document {
    parse_html(html)
}

fn parse_and_layout(html: &str, vw: f32) -> Document {
    load_html(html, vw)
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

fn find_all<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F, out: &mut Vec<&'a WebCore>) {
    if pred(root) {
        out.push(root);
    }
    for child in &root.children {
        find_all(child, pred, out);
    }
}

// ============================================================
// Min-Height / Max-Height Parsing
// ============================================================

#[test]
fn layout_adv_min_height_parsed() {
    let s = style_with("min-height", "100px");
    assert_eq!(s.min_height, CssLength::Px(100.0));
}

#[test]
fn layout_adv_max_height_parsed() {
    let s = style_with("max-height", "200px");
    assert_eq!(s.max_height, CssLength::Px(200.0));
}

#[test]
fn layout_adv_min_height_percent() {
    let s = style_with("min-height", "50%");
    assert!(
        matches!(s.min_height, CssLength::Percent(_)),
        "min-height: 50% should parse as Percent"
    );
}

#[test]
fn layout_adv_max_height_percent() {
    let s = style_with("max-height", "75%");
    assert!(
        matches!(s.max_height, CssLength::Percent(_)),
        "max-height: 75% should parse as Percent"
    );
}

// ============================================================
// Min-Height / Max-Height Layout Enforcement
// ============================================================

#[test]
fn layout_adv_min_height_enforced() {
    let doc = parse_and_layout("<div style='min-height: 200px;'>Short</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.min_height == CssLength::Px(200.0)
    });
    assert!(div.is_some(), "div with min-height: 200px not found");
    assert!(
        div.unwrap().layout.content_rect.h >= 200.0,
        "min-height should enforce h >= 200, got {}",
        div.unwrap().layout.content_rect.h
    );
}

#[test]
fn layout_adv_max_height_enforced() {
    let doc = parse_and_layout(
        "<div style='max-height: 50px; overflow: hidden;'>\
         <p>Line1</p><p>Line2</p><p>Line3</p><p>Line4</p><p>Line5</p>\
         </div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.max_height != CssLength::None
    });
    assert!(div.is_some(), "div with max-height not found");
    assert!(
        div.unwrap().layout.content_rect.h <= 50.0,
        "max-height should cap h <= 50, got {}",
        div.unwrap().layout.content_rect.h
    );
}

// ============================================================
// Margin Collapsing
// ============================================================

#[test]
fn layout_adv_margin_collapsing_positive() {
    // Two adjacent blocks: margin-bottom=30, margin-top=20 should collapse to 30
    let doc = parse_and_layout(
        "<div style='margin-bottom: 30px;'>A</div>\
         <div style='margin-top: 20px;'>B</div>",
        800.0,
    );
    let mut divs = Vec::new();
    find_all(&doc.root, &|b: &WebCore| b.tag == "div", &mut divs);
    assert!(divs.len() >= 2, "Expected at least 2 divs");
    let a = divs[0];
    let b = divs[1];
    let content_gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // With margin collapsing: gap = max(30, 20) = 30, not 50
    assert!(
        content_gap < 45.0,
        "Collapsed margin should be ~30 not 50, got gap={}",
        content_gap
    );
}

#[test]
fn layout_adv_margin_collapsing_equal() {
    // Two identical margins collapse to one
    let doc = parse_and_layout(
        "<div style='margin-bottom: 20px;'>A</div>\
         <div style='margin-top: 20px;'>B</div>",
        800.0,
    );
    let mut divs = Vec::new();
    find_all(&doc.root, &|b: &WebCore| b.tag == "div", &mut divs);
    assert!(divs.len() >= 2);
    let a = divs[0];
    let b = divs[1];
    let content_gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // Should collapse to ~20, not 40
    assert!(
        content_gap < 35.0,
        "Equal margins should collapse to ~20, got gap={}",
        content_gap
    );
}

// ============================================================
// Semantic HTML5 Elements
// ============================================================

#[test]
fn layout_adv_article_is_block() {
    let doc = parse("<article>Content</article>");
    let article = find_box(&doc.root, &|b: &WebCore| b.tag == "article");
    assert!(article.is_some(), "article element not found");
    assert_eq!(
        article.unwrap().style.display,
        Display::Block,
        "article should be Display::Block"
    );
}

#[test]
fn layout_adv_section_is_block() {
    let doc = parse("<section>Content</section>");
    let section = find_box(&doc.root, &|b: &WebCore| b.tag == "section");
    assert!(section.is_some(), "section element not found");
    assert_eq!(section.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_header_is_block() {
    let doc = parse("<header>Content</header>");
    let header = find_box(&doc.root, &|b: &WebCore| b.tag == "header");
    assert!(header.is_some(), "header element not found");
    assert_eq!(header.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_footer_is_block() {
    let doc = parse("<footer>Content</footer>");
    let footer = find_box(&doc.root, &|b: &WebCore| b.tag == "footer");
    assert!(footer.is_some(), "footer element not found");
    assert_eq!(footer.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_nav_is_block() {
    let doc = parse("<nav>Content</nav>");
    let nav = find_box(&doc.root, &|b: &WebCore| b.tag == "nav");
    assert!(nav.is_some(), "nav element not found");
    assert_eq!(nav.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_aside_is_block() {
    let doc = parse("<aside>Content</aside>");
    let aside = find_box(&doc.root, &|b: &WebCore| b.tag == "aside");
    assert!(aside.is_some(), "aside element not found");
    assert_eq!(aside.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_main_is_block() {
    let doc = parse("<main>Content</main>");
    let main_el = find_box(&doc.root, &|b: &WebCore| b.tag == "main");
    assert!(main_el.is_some(), "main element not found");
    assert_eq!(main_el.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_figure_is_block() {
    let doc = parse("<figure><figcaption>Caption</figcaption></figure>");
    let figure = find_box(&doc.root, &|b: &WebCore| b.tag == "figure");
    assert!(figure.is_some(), "figure element not found");
    assert_eq!(figure.unwrap().style.display, Display::Block);
}

#[test]
fn layout_adv_figcaption_is_block() {
    let doc = parse("<figure><figcaption>Caption</figcaption></figure>");
    let figcaption = find_box(&doc.root, &|b: &WebCore| b.tag == "figcaption");
    assert!(figcaption.is_some(), "figcaption element not found");
    assert_eq!(figcaption.unwrap().style.display, Display::Block);
}

// ============================================================
// Display: inline-flex / inline-grid parsing
// ============================================================

#[test]
fn layout_adv_inline_block_parsed() {
    let s = style_with("display", "inline-block");
    assert_eq!(s.display, Display::InlineBlock);
}

#[test]
fn layout_adv_inline_flex_parsed() {
    let s = style_with("display", "inline-flex");
    assert_eq!(s.display, Display::InlineFlex);
}

#[test]
fn layout_adv_inline_grid_parsed() {
    let s = style_with("display", "inline-grid");
    assert_eq!(s.display, Display::InlineGrid);
}

// DeeplyNestedLayout — SKIPPED: uses wxBitmap/wxMemoryDC rendering infrastructure
// MixedFlowLayout    — SKIPPED: uses wxBitmap/wxMemoryDC rendering infrastructure

// TableValignTop/Middle/Bottom — SKIPPED: WebCore has no `table_cell_valign` field
// (the `valign` attribute is applied via `vertical_align` on the cell style).
// These are tested indirectly through the coverage_gaps valign tests.
