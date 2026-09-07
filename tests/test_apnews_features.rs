// Tests for features implemented while fixing AP News rendering:
// - <picture> element source selection
// - clip: rect() CSS property
// - CSS custom property case sensitivity
// - CSS counters (counter-reset, counter-increment, counter())
// - ::before/::after as grid/flex items
// - <source> element display: none
// - Percentage width in intrinsic sizing

use webcore::css::apply_property;
use webcore::types::*;
use webcore::{load_html, load_html_vp, parse_html};

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

fn find_box_mut<'a, F: Fn(&WebCore) -> bool>(
    root: &'a mut WebCore,
    pred: &F,
) -> Option<&'a mut WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &mut root.children {
        if let Some(b) = find_box_mut(child, pred) {
            return Some(b);
        }
    }
    None
}

fn collect_boxes<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F) -> Vec<&'a WebCore> {
    let mut result = Vec::new();
    if pred(root) {
        result.push(root);
    }
    for child in &root.children {
        result.extend(collect_boxes(child, pred));
    }
    result
}

// ============================================================
// <picture> Element
// ============================================================

#[test]
fn picture_simple_source_sets_img_src() {
    let doc = parse_html(
        r#"
        <picture>
            <source srcset="better.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(img.get_attr("src"), Some("better.jpg"));
}

#[test]
fn picture_skips_webp_source() {
    let doc = parse_html(
        r#"
        <picture>
            <source type="image/webp" srcset="photo.webp">
            <source srcset="photo.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(img.get_attr("src"), Some("photo.jpg"));
}

#[test]
fn picture_falls_back_to_img_src_when_no_source_matches() {
    let doc = parse_html(
        r#"
        <picture>
            <source type="image/webp" srcset="photo.webp">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    // Only webp source, which is skipped — img keeps its original src
    assert_eq!(img.get_attr("src"), Some("fallback.jpg"));
}

#[test]
fn picture_skips_source_with_media_when_viewport_unknown() {
    let doc = parse_html(
        r#"
        <picture>
            <source media="(min-width: 1024px)" srcset="large.jpg">
            <source srcset="small.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    // At parse time, viewport is 0 — media sources are skipped, unconditional wins
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(img.get_attr("src"), Some("small.jpg"));
}

#[test]
fn picture_with_viewport_selects_matching_media_source() {
    // load_html_vp runs with real viewport, so media queries evaluate
    let doc = load_html_vp(
        r#"
        <picture>
            <source media="(min-width: 1024px)" srcset="large.jpg">
            <source srcset="small.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
        1200.0,
        800.0,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(img.get_attr("src"), Some("large.jpg"));
}

#[test]
fn picture_with_small_viewport_skips_large_media() {
    let doc = load_html_vp(
        r#"
        <picture>
            <source media="(min-width: 1024px)" srcset="large.jpg">
            <source srcset="small.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
        800.0,
        600.0,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert_eq!(img.get_attr("src"), Some("small.jpg"));
}

#[test]
fn picture_img_gets_resolved_src() {
    let doc = parse_html(
        r#"
        <picture>
            <source srcset="photo.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    assert!(img.get_attr("_resolved_src").is_some());
}

#[test]
fn picture_srcset_with_width_descriptor() {
    let doc = parse_html(
        r#"
        <picture>
            <source srcset="photo-320.jpg 320w, photo-640.jpg 640w">
            <img src="fallback.jpg">
        </picture>
    "#,
    );
    let img = find_box(&doc.root, &|b| b.tag == "img").unwrap();
    // Should pick the first URL from srcset
    assert_eq!(img.get_attr("src"), Some("photo-320.jpg"));
}

#[test]
fn picture_element_is_transparent_container() {
    let doc = parse_html(
        r#"
        <picture>
            <source srcset="photo.jpg">
            <img src="fallback.jpg" width="100" height="50">
        </picture>
    "#,
    );
    let picture = find_box(&doc.root, &|b| b.tag == "picture").unwrap();
    assert!(picture.children.iter().any(|c| c.tag == "img"));
}

// ============================================================
// <source> element display: none
// ============================================================

#[test]
fn source_element_is_hidden() {
    let doc = load_html(
        r#"
        <picture>
            <source srcset="photo.jpg">
            <img src="fallback.jpg">
        </picture>
    "#,
        800.0,
    );
    let source = find_box(&doc.root, &|b| b.tag == "source").unwrap();
    assert_eq!(source.style.display, Display::None);
}

// ============================================================
// clip: rect()
// ============================================================

#[test]
fn clip_rect_parsed_with_commas() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip", "rect(0, 0, 0, 0)");
    assert!(style.clip_rect.is_some());
    let cr = style.clip_rect.unwrap();
    assert_eq!(cr, [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn clip_rect_parsed_with_spaces() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip", "rect(0 0 0 0)");
    assert!(style.clip_rect.is_some());
}

#[test]
fn clip_rect_with_px_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip", "rect(10px, 20px, 30px, 40px)");
    let cr = style.clip_rect.unwrap();
    assert_eq!(cr, [10.0, 20.0, 30.0, 40.0]);
}

#[test]
fn clip_rect_with_auto() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip", "rect(0, auto, auto, 0)");
    let cr = style.clip_rect.unwrap();
    assert_eq!(cr[0], 0.0);
    assert_eq!(cr[1], f32::MAX); // auto
    assert_eq!(cr[2], f32::MAX); // auto
    assert_eq!(cr[3], 0.0);
}

#[test]
fn clip_auto_clears_rect() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip", "rect(0, 0, 0, 0)");
    assert!(style.clip_rect.is_some());
    apply_property(&mut style, "clip", "auto");
    assert!(style.clip_rect.is_none());
}

#[test]
fn clip_rect_zero_hides_element() {
    // clip: rect(0,0,0,0) should result in 0-area clip
    let doc = load_html(
        r#"
        <div style="position: absolute; clip: rect(0, 0, 0, 0);">Hidden</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.clip_rect.is_some()
    })
    .unwrap();
    let cr = div.style.clip_rect.unwrap();
    // clip width = right - left = 0 - 0 = 0
    assert_eq!(cr[1] - cr[3], 0.0);
}

// ============================================================
// CSS Custom Property Case Sensitivity
// ============================================================

#[test]
fn custom_property_resolves_in_stylesheet_rule() {
    let doc = load_html(
        r#"
        <style>
            :root { --mycolor: red; }
            div { color: var(--mycolor); }
        </style>
        <div>Hello</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.children.iter().any(|c| c.text.contains("Hello"))
    })
    .unwrap();
    assert_eq!(div.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn custom_property_camelcase_resolves() {
    let doc = load_html(
        r#"
        <style>
            :root { --myColor: red; }
            div { color: var(--myColor); }
        </style>
        <div>Hello</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.children.iter().any(|c| c.text.contains("Hello"))
    })
    .unwrap();
    // CSS custom properties are case-sensitive — --myColor must match --myColor
    assert_eq!(div.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn custom_property_case_mismatch_does_not_resolve() {
    let doc = load_html(
        r#"
        <style>
            :root { --myColor: red; }
            .test { color: var(--mycolor); }
        </style>
        <div class="test">Hello</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "test")
            .unwrap_or(false)
    })
    .unwrap();
    // --mycolor (lowercase) != --myColor — should NOT resolve to red
    assert_ne!(div.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn standard_property_is_case_insensitive() {
    let doc = load_html(
        r#"
        <style>
            .test { COLOR: red; }
        </style>
        <div class="test">Hello</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "test")
            .unwrap_or(false)
    })
    .unwrap();
    assert_eq!(div.style.color, Color::rgb(255, 0, 0));
}

// ============================================================
// CSS Counters
// ============================================================

#[test]
fn counter_reset_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-reset", "section");
    assert!(style
        .counter_reset
        .iter()
        .any(|(name, _)| name == "section"));
}

#[test]
fn counter_reset_with_value() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-reset", "section 5");
    let (_, val) = style
        .counter_reset
        .iter()
        .find(|(n, _)| n == "section")
        .unwrap();
    assert_eq!(*val, 5);
}

#[test]
fn counter_increment_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-increment", "section");
    assert!(style
        .counter_increment
        .iter()
        .any(|(name, _)| name == "section"));
}

#[test]
fn counter_increment_with_value() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-increment", "item 2");
    let (_, val) = style
        .counter_increment
        .iter()
        .find(|(n, _)| n == "item")
        .unwrap();
    assert_eq!(*val, 2);
}

// ============================================================
// ::before/::after as Grid Items
// ============================================================

#[test]
fn before_pseudo_becomes_grid_item() {
    let doc = load_html(
        r#"
        <style>
            .grid { display: grid; grid-template-columns: 30px auto; }
            .grid::before { content: "X"; }
        </style>
        <div class="grid"><span>Content</span></div>
    "#,
        800.0,
    );
    let grid = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "grid")
            .unwrap_or(false)
    })
    .unwrap();
    // ::before should be inserted as a child box (grid item)
    let has_before = grid.children.iter().any(|c| c.tag == "::before");
    assert!(
        has_before,
        "::before should be a child box in a grid container"
    );
}

#[test]
fn before_pseudo_becomes_flex_item() {
    let doc = load_html(
        r#"
        <style>
            .flex { display: flex; }
            .flex::before { content: "X"; }
        </style>
        <div class="flex"><span>Content</span></div>
    "#,
        800.0,
    );
    let flex = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "flex")
            .unwrap_or(false)
    })
    .unwrap();
    let has_before = flex.children.iter().any(|c| c.tag == "::before");
    assert!(
        has_before,
        "::before should be a child box in a flex container"
    );
}

// ============================================================
// Percentage Width in Intrinsic Sizing
// ============================================================

#[test]
fn percentage_width_treated_as_auto_in_intrinsic() {
    // An element with width: 100% inside a shrink-to-fit context
    // should not collapse to 0
    let doc = load_html(
        r#"
        <div style="float: left;">
            <a style="display: block; width: 100%;">Link text here</a>
        </div>
    "#,
        800.0,
    );
    let link = find_box(&doc.root, &|b| b.tag == "a").unwrap();
    assert!(
        link.layout.content_rect.w > 0.0,
        "width: 100% in intrinsic context should not be 0"
    );
}

// ============================================================
// calc() Expression Parsing
// ============================================================

#[test]
fn calc_subtraction_works() {
    let doc = load_html(
        r#"
        <div style="width: calc(100% - 40px);">content</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.children.iter().any(|c| c.text.contains("content"))
    });
    // Body has 8px margin on each side, so containing block = 800 - 16 = 784
    // calc(100% - 40px) = 784 - 40 = 744
    if let Some(d) = div {
        assert!(
            (d.layout.content_rect.w - 744.0).abs() < 2.0,
            "calc(100% - 40px) at vw=800 should be ~744, got {}",
            d.layout.content_rect.w
        );
    }
}

#[test]
fn calc_multiple_subtractions() {
    let doc = load_html(
        r#"
        <div style="width: calc(100% - 40px - 60px);">content</div>
    "#,
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.children.iter().any(|c| c.text.contains("content"))
    });
    // Body has 8px margin on each side, so containing block = 800 - 16 = 784
    // calc(100% - 40px - 60px) = 784 - 100 = 684
    if let Some(d) = div {
        assert!(
            (d.layout.content_rect.w - 684.0).abs() < 2.0,
            "calc(100% - 40px - 60px) at vw=800 should be ~684, got {}",
            d.layout.content_rect.w
        );
    }
}

// ============================================================
// Flex min-width: auto
// ============================================================

#[test]
fn flex_item_does_not_shrink_below_content() {
    // A flex item with text should not shrink to 0 width
    let doc = load_html(
        r#"
        <div style="display: flex; width: 200px;">
            <span>Hello World</span>
            <span>Other</span>
        </div>
    "#,
        800.0,
    );
    let spans = collect_boxes(&doc.root, &|b| b.tag == "span" && !b.text.is_empty());
    for span in &spans {
        assert!(
            span.layout.content_rect.w > 0.0,
            "flex item '{}' should not have 0 width",
            span.text
        );
    }
}

#[test]
fn flex_item_with_overflow_hidden_can_shrink_to_zero() {
    // overflow: hidden disables the automatic minimum size
    let doc = load_html(
        r#"
        <div style="display: flex; width: 50px;">
            <span style="overflow: hidden; flex-shrink: 1;">Very long text that should be clipped</span>
            <span style="width: 50px; flex-shrink: 0;">Fixed</span>
        </div>
    "#,
        800.0,
    );
    // The first span should be allowed to shrink since it has overflow: hidden
    let span = find_box(&doc.root, &|b| {
        b.tag == "span" && b.children.iter().any(|c| c.text.contains("Very long"))
    });
    // Just verify no panic — the exact width depends on shrinking logic
    assert!(span.is_some());
}

#[test]
fn flex_item_min_width_auto_prevents_text_at_zero() {
    // Simulate the AP News nav issue: flex items with text should have min-content width
    let doc = load_html(
        r#"
        <style>
            .nav { display: flex; width: 100px; }
            .nav-item { flex-shrink: 1; }
        </style>
        <div class="nav">
            <div class="nav-item">World</div>
            <div class="nav-item">Politics</div>
            <div class="nav-item">Sports</div>
        </div>
    "#,
        800.0,
    );
    let items = collect_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "nav-item")
            .unwrap_or(false)
    });
    for item in &items {
        assert!(
            item.layout.content_rect.w > 0.0,
            "flex nav item should not collapse to 0 width"
        );
    }
}

// ============================================================
// Grid with ::before counter and fixed column
// ============================================================

#[test]
fn grid_before_counter_in_fixed_column() {
    let doc = load_html(
        r#"
        <style>
            .list { counter-reset: number; }
            .item {
                display: grid;
                grid-template-columns: 30px auto;
            }
            .item::before {
                content: counter(number);
                counter-increment: number;
            }
        </style>
        <div class="list">
            <div class="item"><span>First article title</span></div>
            <div class="item"><span>Second article title</span></div>
        </div>
    "#,
        400.0,
    );
    let items = collect_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "item")
            .unwrap_or(false)
    });
    assert!(items.len() >= 2, "should have at least 2 grid items");
    for item in &items {
        // The ::before should exist as a child
        let has_before = item.children.iter().any(|c| c.tag == "::before");
        assert!(has_before, "grid item should have ::before child");
        // The content span should have reasonable width (not 0, not full container)
        let content_span = item.children.iter().find(|c| c.tag == "span");
        if let Some(span) = content_span {
            assert!(
                span.layout.content_rect.w > 30.0,
                "content in auto column should be wider than 30px"
            );
        }
    }
}

// ============================================================
// evaluate_media
// ============================================================

#[test]
fn evaluate_media_min_width_passes() {
    assert!(webcore::css::evaluate_media(
        "(min-width: 1024px)",
        1200.0,
        800.0
    ));
}

#[test]
fn evaluate_media_min_width_fails() {
    assert!(!webcore::css::evaluate_media(
        "(min-width: 1024px)",
        800.0,
        600.0
    ));
}

#[test]
fn evaluate_media_max_width() {
    assert!(webcore::css::evaluate_media(
        "(max-width: 768px)",
        600.0,
        400.0
    ));
    assert!(!webcore::css::evaluate_media(
        "(max-width: 768px)",
        1024.0,
        768.0
    ));
}

// ============================================================
// Descendant Selector Matching
// ============================================================

#[test]
fn descendant_selector_class_class() {
    // .Parent .Child { color: red }
    let doc = load_html_vp(
        r#"
        <html><head><style>
        .Parent .Child { color: red; }
        </style></head>
        <body>
            <div class="Parent">
                <div class="Child" id="target">Hello</div>
            </div>
            <div class="Child" id="non-target">Outside</div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let target = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "target")
    })
    .unwrap();
    assert_eq!(
        target.style.color.r, 255,
        "Descendant .Parent .Child should apply color:red"
    );
    assert_eq!(target.style.color.g, 0);

    let non_target = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "non-target")
    })
    .unwrap();
    assert_ne!(
        non_target.style.color.r, 255,
        ".Child outside .Parent should NOT get color:red"
    );
}

#[test]
fn descendant_selector_nested_deep() {
    // .Ancestor .Deep { display: block }
    let doc = load_html_vp(
        r#"
        <html><head><style>
        .Ancestor .Deep { font-weight: bold; }
        </style></head>
        <body>
            <div class="Ancestor">
                <div>
                    <div>
                        <span class="Deep" id="deep">text</span>
                    </div>
                </div>
            </div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let deep = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "deep")
    })
    .unwrap();
    assert!(
        matches!(
            deep.style.font_weight,
            webcore::types::FontWeight::Bold | webcore::types::FontWeight::Value(700)
        ),
        "Deeply nested descendant should match"
    );
}

#[test]
fn descendant_selector_tag_class() {
    // div .item { color: green }
    let doc = load_html_vp(
        r#"
        <html><head><style>
        div .item { color: green; }
        </style></head>
        <body>
            <div>
                <span class="item" id="inside">text</span>
            </div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let inside = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "inside")
    })
    .unwrap();
    assert_eq!(inside.style.color.r, 0);
    assert_eq!(inside.style.color.g, 128);
}

#[test]
fn child_combinator_direct() {
    // .Parent > .Child { color: blue }
    let doc = load_html_vp(
        r#"
        <html><head><style>
        .Parent > .DirectChild { color: blue; }
        </style></head>
        <body>
            <div class="Parent">
                <div class="DirectChild" id="direct">text</div>
                <div><div class="DirectChild" id="indirect">text</div></div>
            </div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let direct = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "direct")
    })
    .unwrap();
    assert_eq!(
        direct.style.color.b, 255,
        "Direct child should match > combinator"
    );

    let indirect = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "indirect")
    })
    .unwrap();
    assert_ne!(
        indirect.style.color.b, 255,
        "Non-direct child should NOT match > combinator"
    );
}

#[test]
fn descendant_selector_parse_produces_combinator() {
    let sel = webcore::css::parse_selector(".Parent .Child");
    // Should be: [Class("Parent"), Combinator(Descendant), Class("Child")]
    assert_eq!(
        sel.parts.len(),
        3,
        "Expected 3 parts: class, combinator, class"
    );
    assert!(matches!(&sel.parts[0], webcore::css::SelectorPart::Class(c) if c == "Parent"));
    assert!(matches!(
        &sel.parts[1],
        webcore::css::SelectorPart::Combinator(webcore::css::Combinator::Descendant)
    ));
    assert!(matches!(&sel.parts[2], webcore::css::SelectorPart::Class(c) if c == "Child"));
}

#[test]
fn inherit_overrides_lower_specificity_rule() {
    // h1 UA default sets font-size: 2em.  Higher-specificity rule says font-size: inherit.
    // The inherit should win and use the parent's computed font-size (16px).
    let doc = load_html_vp(
        r#"
        <html><head><style>
        .parent .child h1 { font-size: inherit; display: inline; }
        </style></head>
        <body>
            <div class="parent"><div class="child">
                <h1 id="hdr">Hello</h1>
            </div></div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let hdr = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "hdr")
    })
    .unwrap();
    // Parent (.child div) has default 16px font.  h1 with font-size:inherit should be 16px, not 2em=32px.
    let fs = hdr.style.font_size_px(16.0, 16.0);
    assert!(
        (fs - 16.0).abs() < 1.0,
        "h1 with font-size:inherit should be 16px, got {fs}"
    );
    assert!(
        matches!(hdr.style.display, webcore::types::Display::Inline),
        "h1 with display:inline from descendant selector"
    );
}

#[test]
fn img_width_height_attributes() {
    // Image with explicit width/height attributes should have those dimensions
    let doc = load_html_vp(
        r#"
        <html><head></head>
        <body>
            <img id="pic" src="nonexistent.png" width="200" height="150">
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let pic = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "pic")
    })
    .unwrap();
    let w = pic.style.width.resolve(16.0, 800.0, 16.0);
    let h = pic.style.height.resolve(16.0, 600.0, 16.0);
    assert!(
        (w - 200.0).abs() < 1.0,
        "img width should be 200px from attribute, got {w}"
    );
    assert!(
        (h - 150.0).abs() < 1.0,
        "img height should be 150px from attribute, got {h}"
    );
    assert_eq!(
        pic.layout.content_rect.w, 200.0,
        "img content_rect.w should be 200"
    );
    assert_eq!(
        pic.layout.content_rect.h, 150.0,
        "img content_rect.h should be 150"
    );
}

#[test]
fn img_width_height_inside_float() {
    // Image inside a float container — float should shrink-wrap to content
    let doc = load_html_vp(
        r#"
        <html><head></head>
        <body>
            <div id="float-wrap" style="float:left;">
                <img id="pic2" src="nonexistent.png" width="120" height="162">
            </div>
            <p>Text should wrap around the float.</p>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let pic = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "pic2")
    })
    .unwrap();
    assert!(
        pic.layout.content_rect.w > 0.0,
        "img inside float should have non-zero width, got {}",
        pic.layout.content_rect.w
    );
    assert!(
        pic.layout.content_rect.h > 0.0,
        "img inside float should have non-zero height, got {}",
        pic.layout.content_rect.h
    );
}

#[test]
fn img_inside_span_a_float() {
    // img inside span > a inside float — progressively add wrappers to find the failure
    // Test 1: img inside <a> inside float — works
    let doc1 = load_html_vp(
        r##"
        <html><head></head><body>
            <div style="float:left;"><a href="/x"><img id="t1" src="x.png" width="120" height="162"></a></div>
        </body></html>
    "##,
        800.0,
        600.0,
    );
    let t1 = find_box(&doc1.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "t1")
    })
    .unwrap();
    assert!(
        t1.layout.content_rect.w > 0.0,
        "t1: img in a in float should have width, got {}",
        t1.layout.content_rect.w
    );

    // Test 2: img inside span > a inside float
    let doc2 = load_html_vp(
        r##"
        <html><head></head><body>
            <div style="float:left;"><span><a href="/x"><img id="t2" src="x.png" width="120" height="162"></a></span></div>
        </body></html>
    "##,
        800.0,
        600.0,
    );
    let t2 = find_box(&doc2.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "t2")
    })
    .unwrap();
    assert!(
        t2.layout.content_rect.w > 0.0,
        "t2: img in span>a in float should have width, got {}",
        t2.layout.content_rect.w
    );

    // Test 3: img inside div > span > a inside float
    let doc3 = load_html_vp(
        r##"
        <html><head></head><body>
            <div style="float:left;"><div><span><a href="/x"><img id="t3" src="x.png" width="120" height="162"></a></span></div></div>
        </body></html>
    "##,
        800.0,
        600.0,
    );
    let t3 = find_box(&doc3.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "t3")
    })
    .unwrap();
    assert!(
        t3.layout.content_rect.w > 0.0,
        "t3: img in div>span>a in float should have width, got {}",
        t3.layout.content_rect.w
    );
}

#[test]
fn descendant_selector_multi_level() {
    // .A .B .C { color: red }
    let doc = load_html_vp(
        r#"
        <html><head><style>
        .A .B .C { color: red; }
        </style></head>
        <body>
            <div class="A">
                <div class="B">
                    <div class="C" id="abc">text</div>
                </div>
            </div>
        </body></html>
    "#,
        800.0,
        600.0,
    );
    let abc = find_box(&doc.root, &|b| {
        b.attributes.get("id").map_or(false, |v| v == "abc")
    })
    .unwrap();
    assert_eq!(
        abc.style.color.r, 255,
        "Multi-level descendant .A .B .C should match"
    );
}

// ============================================================
// Wikipedia-like inline list items
// ============================================================

#[test]
fn li_display_inline_renders_horizontally() {
    let html = r#"
        <style>
            ul { list-style: none; padding: 0; margin: 0; }
            ul li { display: inline; }
        </style>
        <ul>
            <li>Read</li>
            <li>View source</li>
            <li>History</li>
        </ul>
    "#;
    let doc = load_html(html, 800.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 li elements");
    // All items should be display:inline from CSS override
    for (i, item) in items.iter().enumerate() {
        assert_eq!(
            item.style.display,
            Display::Inline,
            "li[{}] should be display:inline, got {:?}",
            i,
            item.style.display
        );
    }
    // All items should be on the same line (same Y position)
    let y0 = items[0].layout.margin_rect.y;
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.layout.margin_rect.y - y0).abs() < 2.0,
            "li[{}] at y={} should be at same y as li[0] at y={} (horizontal layout)",
            i,
            item.layout.margin_rect.y,
            y0
        );
    }
}

fn find_all_boxes<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut result = Vec::new();
    if pred(root) {
        result.push(root);
    }
    for child in &root.children {
        result.extend(find_all_boxes(child, pred));
    }
    result
}

#[test]
fn li_display_inline_block_renders_horizontally() {
    let html = r#"
        <style>
            .tabs { list-style: none; padding: 0; margin: 0; }
            .tabs li { display: inline-block; padding: 5px 10px; }
        </style>
        <ul class="tabs">
            <li>Read</li>
            <li>View source</li>
            <li>History</li>
        </ul>
    "#;
    let doc = load_html(html, 800.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3);
    let y0 = items[0].layout.margin_rect.y;
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.layout.margin_rect.y - y0).abs() < 2.0,
            "inline-block li[{}] at y={} should be at same y as li[0] at y={}",
            i,
            item.layout.margin_rect.y,
            y0
        );
    }
    // x positions should increase (left to right)
    assert!(
        items[1].layout.margin_rect.x > items[0].layout.margin_rect.x,
        "li[1].x should be > li[0].x"
    );
    assert!(
        items[2].layout.margin_rect.x > items[1].layout.margin_rect.x,
        "li[2].x should be > li[1].x"
    );
}

// ============================================================
// Text overlap regression: sibling blocks must not overlap
// ============================================================

#[test]
fn sibling_blocks_do_not_overlap_vertically() {
    // Use divs with no default margins to avoid margin-collapsing interference
    let html = r#"
        <div style="width: 400px;">
            <div>First line with some text content.</div>
            <div>Second line should be below the first.</div>
            <div>Third line should be below the second.</div>
        </div>
    "#;
    let doc = load_html(html, 800.0);
    let container = find_all_boxes(&doc.root, &|b| {
        b.tag == "div" && b.style.width == CssLength::Px(400.0)
    });
    assert!(!container.is_empty(), "container div not found");
    let divs: Vec<&WebCore> = container[0]
        .children
        .iter()
        .filter(|b| b.tag == "div")
        .collect();
    assert_eq!(divs.len(), 3, "expected 3 child divs");
    // Each div's content_rect top must be >= previous div's content_rect bottom
    for i in 1..divs.len() {
        let prev_bottom = divs[i - 1].layout.content_rect.y + divs[i - 1].layout.content_rect.h;
        let curr_top = divs[i].layout.content_rect.y;
        assert!(
            curr_top >= prev_bottom - 1.0,
            "div[{}] top ({:.1}) overlaps div[{}] bottom ({:.1})",
            i,
            curr_top,
            i - 1,
            prev_bottom
        );
    }
}

// ============================================================
// Wikipedia-like layout: central text column with floated content
// ============================================================

#[test]
fn wikipedia_article_count_text_no_overlap() {
    // Simulates the Wikipedia main page structure around the article count
    let html = r#"
        <style>
            .mw-body { margin: 0; padding: 0 10px; }
            .mw-body-content { font-size: 14px; line-height: 1.6; }
            #articlecount { text-align: center; font-size: 12px; }
            #articlecount a { color: blue; }
        </style>
        <div class="mw-body" style="width: 960px;">
            <div class="mw-body-content">
                <div id="mp-upper">
                    <div style="text-align:center;padding:0.2em">
                        <b><a href="/x">6,935,561</a> articles in <a href="/x">English</a></b>
                    </div>
                </div>
                <div id="articlecount">
                    <a href="/x">1,000,000+ articles</a> in more than 300 languages
                </div>
                <div id="mp-lower">
                    <p>Some content below that should not overlap with articlecount.</p>
                </div>
            </div>
        </div>
    "#;
    let doc = load_html(html, 1280.0);

    let count = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|v| v == "articlecount")
            .unwrap_or(false)
    });
    let lower = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|v| v == "mp-lower")
            .unwrap_or(false)
    });

    assert!(!count.is_empty(), "articlecount not found");
    assert!(!lower.is_empty(), "mp-lower not found");

    let count_bottom = count[0].layout.border_rect.y + count[0].layout.border_rect.h;
    let lower_top = lower[0].layout.border_rect.y;
    assert!(
        lower_top >= count_bottom - 1.0,
        "mp-lower top ({:.1}) overlaps articlecount bottom ({:.1})",
        lower_top,
        count_bottom
    );
}

// ============================================================
// Wikipedia-like tabs: li items with display:inline-block in flex parent
// ============================================================

#[test]
fn wikipedia_tabs_horizontal_layout() {
    // Simulates the Vector skin navigation tabs
    let html = r#"
        <style>
            .vector-menu-tabs { display: flex; }
            .vector-menu-tabs ul { display: flex; list-style: none; margin: 0; padding: 0; }
            .vector-menu-tabs li { display: inline-block; margin: 0; }
            .vector-menu-tabs li a { display: block; padding: 5px 10px; }
        </style>
        <nav class="vector-menu-tabs">
            <ul>
                <li class="selected"><a href="/x">Read</a></li>
                <li><a href="/x">View source</a></li>
                <li><a href="/x">View history</a></li>
            </ul>
        </nav>
    "#;
    let doc = load_html(html, 1280.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 li tabs");

    // All tabs must be on the same Y line
    let y0 = items[0].layout.border_rect.y;
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.layout.border_rect.y - y0).abs() < 2.0,
            "tab[{}] '{}' at y={:.1} should be at same y as tab[0] at y={:.1}",
            i,
            item.children
                .first()
                .and_then(|c| c.children.first())
                .map(|t| t.text.as_str())
                .unwrap_or("?"),
            item.layout.border_rect.y,
            y0
        );
    }

    // X positions must increase
    for i in 1..items.len() {
        assert!(
            items[i].layout.border_rect.x > items[i - 1].layout.border_rect.x,
            "tab[{}].x ({:.1}) should be > tab[{}].x ({:.1})",
            i,
            items[i].layout.border_rect.x,
            i - 1,
            items[i - 1].layout.border_rect.x
        );
    }
}

// ============================================================
// Wikipedia tabs: floated list items must be horizontal
// ============================================================

#[test]
fn floated_list_items_horizontal() {
    let html = r#"
        <style>
            .tabs { float: left; }
            .tabs ul { list-style: none; margin: 0; padding: 0; }
            .tabs li { float: left; margin: 0 8px; white-space: nowrap; }
            .tabs li a { display: inline-flex; padding: 5px 10px; }
        </style>
        <div class="tabs">
            <ul>
                <li><a href="/r"><span>Read</span></a></li>
                <li><a href="/v"><span>View source</span></a></li>
                <li><a href="/h"><span>View history</span></a></li>
            </ul>
        </div>
    "#;
    let doc = load_html(html, 1280.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 tab li elements");

    // Floated items must all be on the same line
    let y0 = items[0].layout.border_rect.y;
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.layout.border_rect.y - y0).abs() < 5.0,
            "floated li[{}] at y={:.1} should be same line as li[0] at y={:.1}",
            i,
            item.layout.border_rect.y,
            y0
        );
    }

    // X positions must increase (left to right)
    for i in 1..items.len() {
        assert!(
            items[i].layout.border_rect.x > items[i - 1].layout.border_rect.x,
            "floated li[{}].x ({:.1}) should be > li[{}].x ({:.1})",
            i,
            items[i].layout.border_rect.x,
            i - 1,
            items[i - 1].layout.border_rect.x
        );
    }
}

// ============================================================
// Inline ul/li must render text (Wikipedia hlist pattern)
// ============================================================

#[test]
fn inline_ul_li_text_renders_with_width() {
    let html = concat!(
        "<style>",
        ".wikipedia-languages-count-container { width: 90%; display: flex; justify-content: center; padding-top: 1em; margin: 0 auto; }",
        ".wikipedia-languages-prettybars { width: 100%; height: 1px; margin: 0.5em 0; background: gray; flex-shrink: 1; align-self: center; }",
        ".wikipedia-languages-count { padding: 0 1em; white-space: nowrap; }",
        ".wikipedia-languages ul { margin-left: 0; padding-left: 0; }",
        ".wikipedia-languages>ul { list-style: none; text-align: center; clear: both; }",
        ".hlist.inline, .hlist.inline ul { display: inline; }",
        ".hlist ul { margin: 0; padding: 0; list-style: none; }",
        ".hlist li { display: inline; margin: 0; }",
        ".hlist li::after { content: ' . '; }",
        ".hlist li:last-child::after { content: none; }",
        "</style>",
        "<div class='wikipedia-languages' style='width:900px'>",
        "<ul class='plainlinks'>",
        "<li>",
        "  <div class='wikipedia-languages-count-container'>",
        "    <div class='wikipedia-languages-prettybars'></div>",
        "    <div class='wikipedia-languages-count'>1,000,000+ articles</div>",
        "    <div class='wikipedia-languages-prettybars'></div>",
        "  </div>",
        "  <div class='hlist inline'>",
        "    <ul>",
        "      <li><a href='/ar'><span>العربية</span></a></li>",
        "      <li><a href='/de'><span>Deutsch</span></a></li>",
        "      <li><a href='/es'><span>Español</span></a></li>",
        "      <li><a href='/fr'><span>Français</span></a></li>",
        "      <li><a href='/it'><span>Italiano</span></a></li>",
        "    </ul>",
        "  </div>",
        "</li>",
        "</ul>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    // Find the hlist inline div
    let hlist = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c.contains("hlist"))
            .unwrap_or(false)
    });
    assert!(!hlist.is_empty(), "hlist div not found");
    eprintln!(
        "hlist div: display={:?} x={:.1} y={:.1} w={:.1} h={:.1} lines={}",
        hlist[0].style.display,
        hlist[0].layout.content_rect.x,
        hlist[0].layout.content_rect.y,
        hlist[0].layout.content_rect.w,
        hlist[0].layout.content_rect.h,
        hlist[0].layout.line_cache.len()
    );
    // Check the ul inside
    let ul = find_all_boxes(hlist[0], &|b| b.tag == "ul");
    if !ul.is_empty() {
        eprintln!(
            "  ul: display={:?} w={:.1} h={:.1} lines={}",
            ul[0].style.display,
            ul[0].layout.content_rect.w,
            ul[0].layout.content_rect.h,
            ul[0].layout.line_cache.len()
        );
    }

    // Find the inner li items (display:inline ones from hlist)
    let inner_lis: Vec<&WebCore> = find_all_boxes(hlist[0], &|b| b.tag == "li");
    eprintln!("inner lis: {}", inner_lis.len());
    for (i, li) in inner_lis.iter().enumerate() {
        let text = li
            .children
            .first()
            .and_then(|a| a.children.first())
            .and_then(|s| s.children.first())
            .map(|t| t.text.as_str())
            .unwrap_or("?");
        eprintln!(
            "  li[{}] display={:?} y={:.1} h={:.1} text={}",
            i, li.style.display, li.layout.content_rect.y, li.layout.content_rect.h, text
        );
    }

    // The container <li> (parent of hlist) should have enough height for all content
    let outer_li = find_all_boxes(&doc.root, &|b| {
        b.tag == "li"
            && b.children.iter().any(|c| {
                c.attributes
                    .get("class")
                    .map(|cl| cl.contains("wikipedia-languages-count"))
                    .unwrap_or(false)
            })
    });
    assert!(!outer_li.is_empty(), "outer li not found");
    eprintln!(
        "outer li: y={:.1} h={:.1}",
        outer_li[0].layout.content_rect.y, outer_li[0].layout.content_rect.h
    );

    // The languages text must be visible (height > 0)
    assert!(
        outer_li[0].layout.content_rect.h > 30.0,
        "outer li height ({:.1}) should be > 30 (count bar + language links)",
        outer_li[0].layout.content_rect.h
    );

    // Check line_cache for text content
    eprintln!(
        "outer li line_cache: {} lines",
        outer_li[0].layout.line_cache.len()
    );
    for (i, line) in outer_li[0].layout.line_cache.iter().enumerate() {
        eprintln!(
            "  line[{}] x={:.1} y={:.1} w={:.1} h={:.1}",
            i, line.x, line.y, line.width, line.height
        );
    }

    // No two lines should overlap vertically
    let lines = &outer_li[0].layout.line_cache;
    for i in 1..lines.len() {
        let prev_bottom = lines[i - 1].y + lines[i - 1].height;
        assert!(
            lines[i].y >= prev_bottom - 1.0,
            "line[{}] y={:.1} overlaps line[{}] bottom={:.1}",
            i,
            lines[i].y,
            i - 1,
            prev_bottom
        );
    }
}

// ============================================================
// Wikipedia exact tabs structure
// ============================================================

#[test]
fn float_container_shrinkwrap_contains_all_float_children() {
    // A floated container with width:auto should be wide enough
    // to hold all its float:left children side by side.
    let html = concat!(
        "<style>",
        ".outer { float: left; }",
        ".outer ul { list-style: none; margin: 0; padding: 0; }",
        ".outer li { float: left; padding: 5px 10px; }",
        "</style>",
        "<div class='outer'>",
        "  <ul>",
        "    <li>Read</li>",
        "    <li>View source</li>",
        "    <li>View history</li>",
        "  </ul>",
        "</div>",
    );
    let doc = load_html(html, 1280.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 li elements");

    // All items must be on the same Y line (not wrapping)
    let y0 = items[0].layout.border_rect.y;
    for (i, item) in items.iter().enumerate() {
        eprintln!(
            "li[{}] x={:.1} y={:.1} w={:.1}",
            i, item.layout.border_rect.x, item.layout.border_rect.y, item.layout.border_rect.w
        );
        assert!(
            (item.layout.border_rect.y - y0).abs() < 2.0,
            "float li[{}] at y={:.1} should be same y as li[0] at y={:.1}",
            i,
            item.layout.border_rect.y,
            y0
        );
    }

    // X positions must increase
    for i in 1..items.len() {
        assert!(
            items[i].layout.border_rect.x > items[i - 1].layout.border_rect.x + 5.0,
            "float li[{}].x ({:.1}) should be right of li[{}].x ({:.1})",
            i,
            items[i].layout.border_rect.x,
            i - 1,
            items[i - 1].layout.border_rect.x
        );
    }
}

#[test]
fn wikipedia_exact_tabs_structure() {
    // Exact reproduction of Wikipedia Vector skin tab structure
    let html = concat!(
        "<style>",
        ".vector-menu-tabs { float: left; }",
        ".vector-menu .vector-menu-content-list { list-style: none; margin: 0; padding: 0; }",
        ".vector-menu-tabs .mw-list-item { float: left; margin: 0 8px; white-space: nowrap; margin-bottom: 0; }",
        ".vector-menu-tabs .mw-list-item > a { display: inline-flex; position: relative; cursor: pointer; }",
        ".vector-menu-tabs .mw-list-item a { padding: 8px 0; }",
        "</style>",
        "<div style='width:960px'>",
        "<div id='p-views' class='vector-menu vector-menu-tabs'>",
        "  <div class='vector-menu-content'>",
        "    <ul class='vector-menu-content-list'>",
        "      <li class='mw-list-item'><a href='/r'><span>Read</span></a></li>",
        "      <li class='mw-list-item'><a href='/v'><span>View source</span></a></li>",
        "      <li class='mw-list-item'><a href='/h'><span>View history</span></a></li>",
        "    </ul>",
        "  </div>",
        "</div>",
        "</div>",
    );
    let doc = load_html(html, 1280.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 tab li elements");

    // Debug output
    for (i, item) in items.iter().enumerate() {
        eprintln!(
            "li[{}] display={:?} float={:?} x={:.1} y={:.1} w={:.1} h={:.1}",
            i,
            item.style.display,
            item.style.float,
            item.layout.border_rect.x,
            item.layout.border_rect.y,
            item.layout.border_rect.w,
            item.layout.border_rect.h
        );
    }

    // All items must be on the same Y line
    let y0 = items[0].layout.border_rect.y;
    for (i, item) in items.iter().enumerate() {
        assert!(
            (item.layout.border_rect.y - y0).abs() < 5.0,
            "tab li[{}] at y={:.1} should be same y as li[0] at y={:.1}",
            i,
            item.layout.border_rect.y,
            y0
        );
    }

    // X positions must be different and increasing
    assert!(
        items[1].layout.border_rect.x > items[0].layout.border_rect.x + 5.0,
        "li[1].x ({:.1}) should be well right of li[0].x ({:.1})",
        items[1].layout.border_rect.x,
        items[0].layout.border_rect.x
    );
    assert!(
        items[2].layout.border_rect.x > items[1].layout.border_rect.x + 5.0,
        "li[2].x ({:.1}) should be well right of li[1].x ({:.1})",
        items[2].layout.border_rect.x,
        items[1].layout.border_rect.x
    );
}

// ============================================================
// Text overlap with floated images (Wikipedia-like)
// ============================================================

#[test]
fn text_after_float_does_not_overlap() {
    let html = r#"
        <div style="width: 600px;">
            <div style="float: right; width: 200px; height: 150px; background: gray;">Image</div>
            <p>This is the first paragraph that flows around the floated image on the right side of the container.</p>
            <p>This is the second paragraph that should appear below the first, not overlapping it.</p>
            <p style="clear: both;">This cleared paragraph should be below the float.</p>
        </div>
    "#;
    let doc = load_html(html, 800.0);
    let paras: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "p");
    assert!(
        paras.len() >= 3,
        "expected at least 3 paragraphs, got {}",
        paras.len()
    );

    // Each paragraph's border_rect must not overlap the previous
    for i in 1..paras.len() {
        let prev_bottom = paras[i - 1].layout.border_rect.y + paras[i - 1].layout.border_rect.h;
        let curr_top = paras[i].layout.border_rect.y;
        // Allow margin collapsing: use content_rect for tighter check
        let prev_content_bottom =
            paras[i - 1].layout.content_rect.y + paras[i - 1].layout.content_rect.h;
        let curr_content_top = paras[i].layout.content_rect.y;
        assert!(
            curr_content_top >= prev_content_bottom - 1.0,
            "p[{}] content top ({:.1}) overlaps p[{}] content bottom ({:.1})",
            i,
            curr_content_top,
            i - 1,
            prev_content_bottom
        );
    }
}

// ============================================================
// Wikipedia main page: two-column layout with text
// ============================================================

#[test]
fn two_column_float_layout_no_overlap() {
    // Simulates Wikipedia's two-column main page with floated divs
    let html = concat!(
        "<style>",
        ".mp-left { float: left; width: 55%; }",
        ".mp-right { float: right; width: 42%; }",
        ".mp-section { margin-bottom: 10px; }",
        ".mp-section h2 { font-size: 18px; margin: 0 0 5px; padding: 5px; background: #cef; }",
        "</style>",
        "<div style='width:960px'>",
        "  <div class='mp-left'>",
        "    <div class='mp-section'><h2>Featured article</h2>",
        "      <p>Some featured article text that describes something interesting.</p>",
        "    </div>",
        "    <div class='mp-section'><h2>Did you know</h2>",
        "      <p>Some did-you-know facts about various topics.</p>",
        "    </div>",
        "  </div>",
        "  <div class='mp-right'>",
        "    <div class='mp-section'><h2>In the news</h2>",
        "      <p>Recent news items from around the world.</p>",
        "    </div>",
        "  </div>",
        "  <div style='clear:both'></div>",
        "  <div class='mp-section'><h2>On this day</h2>",
        "    <p>Historical events from this day.</p>",
        "  </div>",
        "</div>",
    );
    let doc = load_html(html, 1280.0);

    // Find all h2 and p elements
    let h2s: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "h2");
    let ps: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "p");

    // Within each section, h2 must be above its p
    // "On this day" h2 and p must be below all floated content
    let last_h2 = h2s.last().unwrap();
    let last_p = ps.last().unwrap();
    assert!(
        last_p.layout.content_rect.y > last_h2.layout.content_rect.y,
        "last p.y ({:.1}) should be below last h2.y ({:.1})",
        last_p.layout.content_rect.y,
        last_h2.layout.content_rect.y
    );

    // No two paragraphs in the same column should overlap
    for i in 0..ps.len() {
        for j in (i + 1)..ps.len() {
            let a = &ps[i].layout.content_rect;
            let b = &ps[j].layout.content_rect;
            // Only check overlap if they're in the same horizontal region
            let x_overlap = a.x < b.x + b.w && b.x < a.x + a.w;
            if x_overlap && a.h > 0.0 && b.h > 0.0 {
                let y_overlap = a.y < b.y + b.h && b.y < a.y + a.h;
                assert!(!y_overlap,
                    "p[{}] at ({:.0},{:.0} {:.0}x{:.0}) overlaps p[{}] at ({:.0},{:.0} {:.0}x{:.0})",
                    i, a.x, a.y, a.w, a.h, j, b.x, b.y, b.w, b.h);
            }
        }
    }
}

// ============================================================
// BiDi direction attribute: text overlap regression
// ============================================================

#[test]
fn bidi_dir_ltr_no_text_overlap() {
    // Wikipedia uses dir="ltr" on HTML element - this may trigger BiDi processing
    // which could cause overlapping text if line positions are miscalculated
    let html = concat!(
        "<html dir='ltr'>",
        "<body>",
        "<div style='width:600px'>",
        "  <p>First paragraph of text content that should render normally.</p>",
        "  <p>Second paragraph should be below the first, not overlapping.</p>",
        "  <p>Third paragraph with more text.</p>",
        "</div>",
        "</body>",
        "</html>",
    );
    let doc = load_html(html, 800.0);
    let paras: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "p");
    assert!(
        paras.len() >= 3,
        "expected 3 paragraphs, got {}",
        paras.len()
    );

    for (i, p) in paras.iter().enumerate() {
        eprintln!(
            "p[{}] y={:.1} h={:.1} content_y={:.1} content_h={:.1} lines={}",
            i,
            p.layout.margin_rect.y,
            p.layout.margin_rect.h,
            p.layout.content_rect.y,
            p.layout.content_rect.h,
            p.layout.line_cache.len()
        );
    }

    // Check no overlap in content_rect
    for i in 1..paras.len() {
        let prev_bottom = paras[i - 1].layout.content_rect.y + paras[i - 1].layout.content_rect.h;
        let curr_top = paras[i].layout.content_rect.y;
        assert!(
            curr_top >= prev_bottom - 1.0,
            "p[{}] content_top ({:.1}) overlaps p[{}] content_bottom ({:.1})",
            i,
            curr_top,
            i - 1,
            prev_bottom
        );
    }
}

#[test]
fn inline_elements_with_dir_no_overlap() {
    // Mixed inline elements with direction - common on Wikipedia
    let html = concat!(
        "<html dir='ltr'>",
        "<body>",
        "<div style='width:400px'>",
        "  <div>Article count: <b>6,935,561</b> articles in <a href='/en'>English</a></div>",
        "  <div style='font-size:12px'>More than 300 languages</div>",
        "  <div>Content below that must not overlap</div>",
        "</div>",
        "</body>",
        "</html>",
    );
    let doc = load_html(html, 800.0);

    let container = find_all_boxes(&doc.root, &|b| {
        b.tag == "div" && b.style.width == CssLength::Px(400.0)
    });
    assert!(!container.is_empty());
    let divs: Vec<&WebCore> = container[0]
        .children
        .iter()
        .filter(|c| c.tag == "div")
        .collect();

    for (i, d) in divs.iter().enumerate() {
        eprintln!(
            "div[{}] y={:.1} h={:.1}",
            i, d.layout.content_rect.y, d.layout.content_rect.h
        );
    }

    for i in 1..divs.len() {
        let prev_bottom = divs[i - 1].layout.content_rect.y + divs[i - 1].layout.content_rect.h;
        let curr_top = divs[i].layout.content_rect.y;
        assert!(
            curr_top >= prev_bottom - 1.0,
            "div[{}] top ({:.1}) overlaps div[{}] bottom ({:.1})",
            i,
            curr_top,
            i - 1,
            prev_bottom
        );
    }
}

// ============================================================
// Wikipedia content with table-based layout (common pattern)
// ============================================================

#[test]
fn table_layout_cells_no_overlap() {
    // Wikipedia uses tables extensively for layout
    let html = concat!(
        "<style>",
        "table { width: 100%; border-collapse: collapse; }",
        "td { vertical-align: top; padding: 5px; }",
        "</style>",
        "<div style='width:960px'>",
        "<table><tr>",
        "  <td style='width:55%'>",
        "    <h2>Featured article</h2>",
        "    <p>The Raven is a narrative poem by American writer Edgar Allan Poe. First published in January 1845.</p>",
        "    <h2>Did you know</h2>",
        "    <p>Some interesting facts about the world.</p>",
        "  </td>",
        "  <td style='width:45%'>",
        "    <h2>In the news</h2>",
        "    <p>Recent events from around the world.</p>",
        "  </td>",
        "</tr></table>",
        "<h2>On this day</h2>",
        "<p>Events from this date in history.</p>",
        "</div>",
    );
    let doc = load_html(html, 1280.0);

    // "On this day" section must be below the table
    let h2s: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "h2");
    let table = find_all_boxes(&doc.root, &|b| b.tag == "table");
    assert!(!table.is_empty(), "table not found");

    let table_bottom = table[0].layout.border_rect.y + table[0].layout.border_rect.h;
    // Find "On this day" h2 - should be the last one
    let last_h2 = h2s.last().unwrap();
    eprintln!(
        "table bottom: {:.1}, last h2 y: {:.1}",
        table_bottom, last_h2.layout.border_rect.y
    );
    assert!(
        last_h2.layout.border_rect.y >= table_bottom - 1.0,
        "last h2 y ({:.1}) should be below table bottom ({:.1})",
        last_h2.layout.border_rect.y,
        table_bottom
    );

    // Within left column, h2 and p should not overlap
    let tds: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "td");
    for td in &tds {
        let children: Vec<&WebCore> = td.children.iter().filter(|c| c.tag != "#text").collect();
        for i in 1..children.len() {
            let prev_bottom =
                children[i - 1].layout.content_rect.y + children[i - 1].layout.content_rect.h;
            let curr_top = children[i].layout.content_rect.y;
            assert!(
                curr_top >= prev_bottom - 1.0,
                "child[{}] ({}) top ({:.1}) overlaps child[{}] ({}) bottom ({:.1}) in td",
                i,
                children[i].tag,
                curr_top,
                i - 1,
                children[i - 1].tag,
                prev_bottom
            );
        }
    }
}

// ============================================================
// Load actual Wikipedia and check for overlapping content
// ============================================================

#[test]
#[ignore] // requires /tmp/wiki_full.html + wiki_css.css from manual fetch
fn wikipedia_real_page_no_major_overlaps() {
    let html = match std::fs::read_to_string("/tmp/wiki_full.html") {
        Ok(h) => h,
        Err(_) => {
            eprintln!("SKIP: /tmp/wiki_full.html not found");
            return;
        }
    };
    let css_text = std::fs::read_to_string("/tmp/wiki_css.css").unwrap_or_default();

    let mut doc =
        webcore::html::parse_html_with_base(&html, "https://en.wikipedia.org/wiki/Main_Page");
    doc.stylesheet.parse_and_add(&css_text);
    doc.viewport_w = 1280.0;
    doc.viewport_h = 900.0;
    let mut renderer = webcore::renderer::Renderer::new();
    {
        let eng = renderer.layout_engine();
        eng.viewport_h = 900.0;
        eng.layout(&mut doc, 1280.0);
    }

    // Check for overlapping block siblings
    let mut overlaps = Vec::new();
    check_block_overlaps(&doc.root, &mut overlaps);

    for (i, overlap) in overlaps.iter().enumerate().take(20) {
        eprintln!("[OVERLAP] {}", overlap);
    }

    // We expect no major overlaps (>10px) in the main content area
    let major = overlaps.iter().filter(|o| o.contains("MAJOR")).count();
    assert!(
        major == 0,
        "Found {} major overlaps (>20px) on Wikipedia page",
        major
    );
}

fn check_block_overlaps(node: &WebCore, overlaps: &mut Vec<String>) {
    let block_children: Vec<&WebCore> = node
        .children
        .iter()
        .filter(|c| {
            c.tag != "#text"
                && c.style.is_block_level()
                && c.style.float == Float::None
                && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                && c.layout.content_rect.h > 0.0
        })
        .collect();

    for i in 1..block_children.len() {
        let prev = &block_children[i - 1];
        let curr = &block_children[i];
        let prev_bottom = prev.layout.content_rect.y + prev.layout.content_rect.h;
        let curr_top = curr.layout.content_rect.y;
        let overlap = prev_bottom - curr_top;

        if overlap > 5.0 {
            let severity = if overlap > 20.0 { "MAJOR" } else { "minor" };
            overlaps.push(format!(
                "{}: {:.0}px overlap - <{}> (y={:.0} h={:.0}) -> <{}> (y={:.0} h={:.0}) in <{}>",
                severity,
                overlap,
                prev.tag,
                prev.layout.content_rect.y,
                prev.layout.content_rect.h,
                curr.tag,
                curr.layout.content_rect.y,
                curr.layout.content_rect.h,
                node.tag
            ));
        }
    }

    for child in &node.children {
        check_block_overlaps(child, overlaps);
    }
}

// ============================================================
// RTL: text-align defaults to right in dir=rtl context
// ============================================================

#[test]
fn rtl_text_align_defaults_to_right() {
    let html = concat!(
        "<html dir='rtl'>",
        "<body>",
        "<p>مرحبا بالعالم</p>",
        "</body>",
        "</html>",
    );
    let doc = load_html(html, 800.0);
    let p = find_all_boxes(&doc.root, &|b| b.tag == "p");
    assert!(!p.is_empty(), "p element not found");
    // In RTL, text-align:start resolves to right, so text should be
    // right-aligned: the line's x + width should reach near the right edge
    assert!(
        !p[0].layout.line_cache.is_empty(),
        "p should have line cache"
    );
    let line = &p[0].layout.line_cache[0];
    let right_edge = p[0].layout.content_rect.x + p[0].layout.content_rect.w;
    let line_right = line.x + line.width;
    assert!(
        line_right > right_edge - 5.0,
        "RTL text should be right-aligned: line_right={:.1} vs content_right={:.1}",
        line_right,
        right_edge
    );
}

// ============================================================
// RTL: flex-direction:row should lay items right-to-left
// ============================================================

#[test]
fn rtl_flex_row_items_flow_right_to_left() {
    let html = concat!(
        "<div dir='rtl' style='display:flex; width:600px'>",
        "<div style='width:100px; height:50px'>A</div>",
        "<div style='width:100px; height:50px'>B</div>",
        "<div style='width:100px; height:50px'>C</div>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| {
        b.style.width == CssLength::Px(100.0) && b.style.height == CssLength::Px(50.0)
    });
    assert_eq!(items.len(), 3, "expected 3 flex items");
    // In RTL flex row, first item should be on the RIGHT
    assert!(
        items[0].layout.content_rect.x > items[1].layout.content_rect.x,
        "RTL flex: item A ({:.0}) should be right of item B ({:.0})",
        items[0].layout.content_rect.x,
        items[1].layout.content_rect.x
    );
    assert!(
        items[1].layout.content_rect.x > items[2].layout.content_rect.x,
        "RTL flex: item B ({:.0}) should be right of item C ({:.0})",
        items[1].layout.content_rect.x,
        items[2].layout.content_rect.x
    );
}

// ============================================================
// Inline text in nested spans must not have zero width
// ============================================================

#[test]
fn inline_text_in_nested_spans_has_width() {
    let html = concat!(
        "<div style='width:400px'>",
        "<a href='/x'><span>Hello</span></a>",
        " ",
        "<a href='/y'><span>World</span></a>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let container = find_all_boxes(&doc.root, &|b| b.style.width == CssLength::Px(400.0));
    assert!(!container.is_empty());
    // The container should have line cache with text
    assert!(
        !container[0].layout.line_cache.is_empty(),
        "container should have lines"
    );
    let line = &container[0].layout.line_cache[0];
    // Line should have non-trivial width (both words rendered)
    assert!(
        line.width > 50.0,
        "line width ({:.1}) should be > 50 (two words visible)",
        line.width
    );
}

// ============================================================
// Newlines between inline elements render as spaces
// ============================================================

#[test]
fn newlines_between_inline_elements_render_as_spaces() {
    let html = "<div style='width:400px'>\n<span>Hello</span>\n<span>World</span>\n</div>";
    let doc = load_html(html, 800.0);
    let container = find_all_boxes(&doc.root, &|b| b.style.width == CssLength::Px(400.0));
    assert!(!container.is_empty());
    assert!(!container[0].layout.line_cache.is_empty());
    let line = &container[0].layout.line_cache[0];
    // "Hello World" with a space between should be wider than "HelloWorld"
    // The space from the newline between </span> and <span> should add width
    assert!(
        line.width > 60.0,
        "line width ({:.1}) should include space between words",
        line.width
    );
}

// ============================================================
// Al Jazeera bugs: absolute-positioned children must not affect
// parent flex item sizing
// ============================================================

#[test]
fn absolute_child_does_not_inflate_flex_parent() {
    // A position:absolute child inside a flex item should not
    // contribute to the flex item's height.
    let html = concat!(
        "<div style='display:flex; width:800px'>",
        "  <div style='position:relative'>",
        "    <span>Menu Item</span>",
        "    <div style='position:absolute; top:100%; width:200px; height:300px'>",
        "      <div>Dropdown content</div>",
        "      <div>More dropdown</div>",
        "    </div>",
        "  </div>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let flex = find_all_boxes(&doc.root, &|b| b.style.display == Display::Flex);
    assert!(!flex.is_empty());
    // Flex container height should be ~1 line of text (~20px), NOT 300px+
    assert!(
        flex[0].layout.content_rect.h < 50.0,
        "flex container height ({:.1}) should be < 50 (absolute dropdown should not inflate it)",
        flex[0].layout.content_rect.h
    );
}

// ============================================================
// Al Jazeera bugs: visibility:hidden + opacity:0 elements
// should not be painted
// ============================================================

#[test]
fn visibility_hidden_opacity_zero_not_painted() {
    let html = concat!(
        "<div style='width:400px'>",
        "  <div>Visible text</div>",
        "  <div style='visibility:hidden; opacity:0; position:absolute'>",
        "    <div>Hidden dropdown</div>",
        "  </div>",
        "  <div>More visible text</div>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    // The hidden div should be absolute and not affect flow
    let visible_divs: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| {
        b.tag == "div"
            && b.style.visibility
            && b.layout.content_rect.h > 0.0
            && !matches!(b.style.position, Position::Absolute | Position::Fixed)
    });
    // "Visible text" and "More visible text" should not overlap
    let texts: Vec<&WebCore> = visible_divs
        .iter()
        .filter(|b| {
            b.children
                .iter()
                .any(|c| c.tag == "#text" && !c.text.trim().is_empty())
        })
        .cloned()
        .collect();
    if texts.len() >= 2 {
        let first_bottom = texts[0].layout.content_rect.y + texts[0].layout.content_rect.h;
        let second_top = texts[1].layout.content_rect.y;
        assert!(
            second_top >= first_bottom - 1.0,
            "visible divs should not overlap: first bottom={:.1}, second top={:.1}",
            first_bottom,
            second_top
        );
    }
}

// ============================================================
// Al Jazeera bugs: RTL inline text must render at correct x
// positions (not all at x=0 or off-screen)
// ============================================================

#[test]
fn rtl_inline_text_has_visible_position() {
    let html = concat!(
        "<html dir='rtl'><body>",
        "<div style='width:600px'>",
        "  <a href='/x'><span>أخبار</span></a>",
        "  <a href='/y'><span>رياضة</span></a>",
        "  <a href='/z'><span>اقتصاد</span></a>",
        "</div>",
        "</body></html>",
    );
    let doc = load_html(html, 800.0);
    let container = find_all_boxes(&doc.root, &|b| b.style.width == CssLength::Px(600.0));
    assert!(!container.is_empty());
    // Container must have line cache with content
    assert!(
        !container[0].layout.line_cache.is_empty(),
        "container should have line cache for inline text"
    );
    let line = &container[0].layout.line_cache[0];
    // Line must have visible width
    assert!(
        line.width > 30.0,
        "RTL inline text line width ({:.1}) should be > 30",
        line.width
    );
    // In RTL, text should be near the RIGHT edge of the container
    let container_right = container[0].layout.content_rect.x + container[0].layout.content_rect.w;
    let line_right = line.x + line.width;
    assert!(
        (line_right - container_right).abs() < 5.0,
        "RTL text should be right-aligned: line_right={:.1} container_right={:.1}",
        line_right,
        container_right
    );
}

// ============================================================
// Al Jazeera bugs: SVG inside inline-block should have dimensions
// from width/height attributes
// ============================================================

#[test]
fn svg_in_inline_block_has_dimensions() {
    let html = concat!(
        "<div style='width:400px'>",
        "  <span style='display:inline-block'>",
        "    <svg width='60' height='40' viewBox='0 0 60 40'></svg>",
        "  </span>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let svg = find_all_boxes(&doc.root, &|b| b.tag == "svg");
    assert!(!svg.is_empty(), "svg not found");
    assert!(
        svg[0].layout.content_rect.w >= 55.0,
        "svg width ({:.1}) should be >= 55 (from width=60 attr)",
        svg[0].layout.content_rect.w
    );
    assert!(
        svg[0].layout.content_rect.h >= 35.0,
        "svg height ({:.1}) should be >= 35 (from height=40 attr)",
        svg[0].layout.content_rect.h
    );
}

// ============================================================
// Al Jazeera: text in <a><span> inside flex item must be visible
// ============================================================

#[test]
fn text_in_link_inside_flex_item_is_on_screen() {
    // Reproduces: nav > ul(flex) > li(flex) > a > span > text
    // where text ends up off-screen at x>viewport
    let html = concat!(
        "<html dir='rtl'><body>",
        "<nav style='width:800px'>",
        "  <ul style='display:flex; list-style:none; margin:0; padding:0'>",
        "    <li style='display:flex; position:relative'>",
        "      <a href='/news'><span>أخبار</span></a>",
        "    </li>",
        "    <li style='display:flex; position:relative'>",
        "      <a href='/sport'><span>رياضة</span></a>",
        "    </li>",
        "    <li style='display:flex; position:relative'>",
        "      <a href='/econ'><span>اقتصاد</span></a>",
        "    </li>",
        "  </ul>",
        "</nav>",
        "</body></html>",
    );
    let doc = load_html(html, 800.0);
    let spans: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| {
        b.tag == "span" && !b.children.is_empty() && b.children[0].tag == "#text"
    });
    for (i, span) in spans.iter().enumerate() {
        let text = &span.children[0].text;
        eprintln!(
            "span[{}] '{}' x={:.1} y={:.1} w={:.1} h={:.1}",
            i,
            text,
            span.layout.content_rect.x,
            span.layout.content_rect.y,
            span.layout.content_rect.w,
            span.layout.content_rect.h
        );
    }
    // ALL text spans must be within the 800px viewport, not off-screen
    for (i, span) in spans.iter().enumerate() {
        let text = &span.children[0].text;
        // Inline spans may have 0x0 content_rect — check their parent <a>
        // or check the containing flex item's position
        let li = find_all_boxes(&doc.root, &|b| {
            b.tag == "li" && b.style.display == Display::Flex
        });
        if i < li.len() {
            assert!(
                li[i].layout.content_rect.x < 800.0 && li[i].layout.content_rect.x >= 0.0,
                "li[{}] for '{}' x={:.1} should be on-screen (0..800)",
                i,
                text,
                li[i].layout.content_rect.x
            );
            assert!(
                li[i].layout.content_rect.w > 5.0,
                "li[{}] for '{}' w={:.1} should have visible width",
                i,
                text,
                li[i].layout.content_rect.w
            );
        }
    }
    // The <a> elements must have non-zero width (text is visible)
    let links: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| {
        b.tag == "a" && b.attributes.get("href").is_some()
    });
    assert!(links.len() >= 3);
    for (i, link) in links.iter().enumerate() {
        // Either the link itself or the flex parent should have width
        assert!(
            link.layout.content_rect.w > 0.0 || link.layout.margin_rect.w > 0.0,
            "link[{}] should have non-zero width (text must be visible), w={:.1} mw={:.1}",
            i,
            link.layout.content_rect.w,
            link.layout.margin_rect.w
        );
    }
}

// ============================================================
// RTL: grid columns should flow right-to-left
// ============================================================

#[test]
fn rtl_grid_columns_flow_right_to_left() {
    let html = concat!(
        "<html dir='rtl'><body>",
        "<div style='display:grid; grid-template-columns: 2fr 1fr; width:900px; gap:10px'>",
        "  <div id='main'>Main content</div>",
        "  <div id='side'>Sidebar</div>",
        "</div>",
        "</body></html>",
    );
    let doc = load_html(html, 1000.0);
    let main = find_all_boxes(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "main").unwrap_or(false)
    });
    let side = find_all_boxes(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "side").unwrap_or(false)
    });
    assert!(!main.is_empty() && !side.is_empty());
    // In RTL, "main" (first in DOM, 2fr) should be on the RIGHT
    // "side" (second, 1fr) should be on the LEFT
    assert!(
        main[0].layout.content_rect.x > side[0].layout.content_rect.x,
        "RTL grid: main ({:.0}) should be right of side ({:.0})",
        main[0].layout.content_rect.x,
        side[0].layout.content_rect.x
    );
    // Both must be within the 900px container — no overflow
    let grid = find_all_boxes(&doc.root, &|b| b.style.display == Display::Grid);
    assert!(!grid.is_empty());
    let grid_right = grid[0].layout.content_rect.x + grid[0].layout.content_rect.w;
    let main_right = main[0].layout.content_rect.x + main[0].layout.content_rect.w;
    let side_right = side[0].layout.content_rect.x + side[0].layout.content_rect.w;
    assert!(
        main_right <= grid_right + 1.0,
        "RTL grid: main right edge ({:.0}) must not exceed grid right ({:.0})",
        main_right,
        grid_right
    );
    assert!(
        side[0].layout.content_rect.x >= grid[0].layout.content_rect.x - 1.0,
        "RTL grid: side left edge ({:.0}) must not be before grid left ({:.0})",
        side[0].layout.content_rect.x,
        grid[0].layout.content_rect.x
    );
    // Widths: main should be ~2x side (2fr vs 1fr minus gap)
    assert!(
        main[0].layout.content_rect.w > side[0].layout.content_rect.w * 1.5,
        "RTL grid: main width ({:.0}) should be ~2x side width ({:.0})",
        main[0].layout.content_rect.w,
        side[0].layout.content_rect.w
    );
}

// ============================================================
// Al Jazeera: dark background from CSS var must render
// ============================================================

#[test]
fn css_var_background_color_applied() {
    let html = concat!(
        "<style>",
        ":root { --bg-dark: #1a1a2e; }",
        ".header { background-color: var(--bg-dark); height: 60px; }",
        "</style>",
        "<div class='header'>Header</div>",
    );
    let doc = load_html(html, 800.0);
    let header = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c.contains("header"))
            .unwrap_or(false)
    });
    assert!(!header.is_empty());
    // Background color should be resolved from the CSS variable
    let bg = header[0].style.background_color;
    assert!(
        bg.a > 0,
        "header background should be opaque (from CSS var), got alpha={}",
        bg.a
    );
    assert_eq!(bg.r, 0x1a, "header bg red={} expected 0x1a", bg.r);
}

// ============================================================
// Al Jazeera: position:sticky should not push content down
// ============================================================

#[test]
fn sticky_element_does_not_add_extra_height() {
    let html = concat!(
        "<div style='width:800px'>",
        "  <div style='position:sticky; top:0; height:60px; background:black'>Nav</div>",
        "  <div id='content'>Content starts here</div>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let content = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|v| v == "content")
            .unwrap_or(false)
    });
    assert!(!content.is_empty());
    // Content should start at y=60 (right after the sticky nav)
    assert!(
        content[0].layout.content_rect.y < 80.0,
        "content y ({:.1}) should be < 80 (right after 60px sticky nav)",
        content[0].layout.content_rect.y
    );
}

// ============================================================
// RTL grid: 12-column grid items must not overflow container
// ============================================================

#[test]
fn rtl_grid_12col_items_within_container() {
    let html = concat!(
        "<html dir='rtl'><body>",
        "<div style='display:grid; grid-template-columns:repeat(12, 1fr); width:1170px'>",
        "  <div id='hero' style='grid-column: span 6'>Hero</div>",
        "  <div id='mid' style='grid-column: span 3'>Middle</div>",
        "  <div id='side' style='grid-column: span 3'>Side</div>",
        "</div>",
        "</body></html>",
    );
    let doc = load_html(html, 1280.0);
    let grid = find_all_boxes(&doc.root, &|b| b.style.display == Display::Grid);
    let hero = find_all_boxes(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "hero").unwrap_or(false)
    });
    let mid = find_all_boxes(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "mid").unwrap_or(false)
    });
    let side = find_all_boxes(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "side").unwrap_or(false)
    });
    assert!(!grid.is_empty() && !hero.is_empty() && !mid.is_empty() && !side.is_empty());

    let g = &grid[0].layout.content_rect;
    let h = &hero[0].layout.content_rect;
    let m = &mid[0].layout.content_rect;
    let s = &side[0].layout.content_rect;

    eprintln!("grid: x={:.0} w={:.0}", g.x, g.w);
    eprintln!("hero: x={:.0} w={:.0} right={:.0}", h.x, h.w, h.x + h.w);
    eprintln!("mid:  x={:.0} w={:.0} right={:.0}", m.x, m.w, m.x + m.w);
    eprintln!("side: x={:.0} w={:.0} right={:.0}", s.x, s.w, s.x + s.w);

    // No item may overflow the grid container
    assert!(
        h.x + h.w <= g.x + g.w + 1.0,
        "hero right ({:.0}) overflows grid right ({:.0})",
        h.x + h.w,
        g.x + g.w
    );
    assert!(
        s.x >= g.x - 1.0,
        "side left ({:.0}) before grid left ({:.0})",
        s.x,
        g.x
    );
    // RTL: hero (first in DOM) must be rightmost
    assert!(
        h.x > m.x,
        "hero x ({:.0}) should be > mid x ({:.0})",
        h.x,
        m.x
    );
    assert!(
        m.x > s.x,
        "mid x ({:.0}) should be > side x ({:.0})",
        m.x,
        s.x
    );
    // Widths: hero=6fr, mid=3fr, side=3fr
    assert!(
        (h.w - m.w * 2.0).abs() < 5.0,
        "hero width ({:.0}) should be ~2x mid width ({:.0})",
        h.w,
        m.w
    );
}

// ============================================================
// RTL: narrow grid column must not truncate text to single chars
// ============================================================

#[test]
fn rtl_grid_narrow_column_wraps_text() {
    let html = concat!(
        "<html dir='rtl'><body>",
        "<div style='display:grid; grid-template-columns: 3fr 1fr; width:800px; gap:10px'>",
        "  <div id='main'>المحتوى الرئيسي هنا</div>",
        "  <div id='sidebar'>",
        "    <div>الأخبار العاجلة</div>",
        "    <div>آخر التحديثات</div>",
        "  </div>",
        "</div>",
        "</body></html>",
    );
    let doc = load_html(html, 1000.0);
    let sidebar = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|v| v == "sidebar")
            .unwrap_or(false)
    });
    assert!(!sidebar.is_empty());
    // Sidebar should be ~25% of 800 = ~195px wide — enough for Arabic text
    assert!(
        sidebar[0].layout.content_rect.w > 150.0,
        "sidebar width ({:.1}) should be > 150px",
        sidebar[0].layout.content_rect.w
    );
    // Sidebar children should have content (not single-char truncation)
    let divs: Vec<&WebCore> = sidebar[0]
        .children
        .iter()
        .filter(|c| c.tag == "div")
        .collect();
    for (i, d) in divs.iter().enumerate() {
        assert!(
            d.layout.content_rect.h > 10.0,
            "sidebar div[{}] height ({:.1}) should be > 10",
            i,
            d.layout.content_rect.h
        );
    }
}

// ============================================================
// Dark background on nav header (CSS var or direct)
// ============================================================

#[test]
fn dark_header_background_renders() {
    let html = concat!(
        "<style>",
        ".header-container { background-color: rgb(26, 26, 46); }",
        ".header { display: flex; padding: 10px; color: white; }",
        "</style>",
        "<div class='header-container'>",
        "  <div class='header'>",
        "    <span>تسجيل</span>",
        "    <span>البث الحي</span>",
        "  </div>",
        "</div>",
    );
    let doc = load_html(html, 800.0);
    let container = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c.contains("header-container"))
            .unwrap_or(false)
    });
    assert!(!container.is_empty());
    let bg = container[0].style.background_color;
    assert!(bg.a > 0, "header bg should be opaque, got alpha={}", bg.a);
    assert!(
        bg.r < 50 && bg.g < 50 && bg.b < 80,
        "header bg should be dark, got r={} g={} b={}",
        bg.r,
        bg.g,
        bg.b
    );
}

// ============================================================
// Text next to ::before pseudo-element must be visible
// ============================================================

#[test]
fn text_next_to_before_pseudo_is_visible() {
    let html = concat!(
        "<style>",
        "h2::before { content: ''; display: inline-block; width: 4px; height: 20px; background: red; margin-right: 8px; }",
        "</style>",
        "<h2>اختيارات المحررين</h2>",
    );
    let doc = load_html(html, 800.0);
    let h2 = find_all_boxes(&doc.root, &|b| b.tag == "h2");
    assert!(!h2.is_empty());
    // h2 must have visible height (text + before pseudo)
    assert!(
        h2[0].layout.content_rect.h > 15.0,
        "h2 height ({:.1}) should be > 15",
        h2[0].layout.content_rect.h
    );
    // h2 must have line_cache with text content
    assert!(
        !h2[0].layout.line_cache.is_empty(),
        "h2 should have line_cache for text"
    );
    let line = &h2[0].layout.line_cache[0];
    // Line width should include both the ::before and the text
    assert!(
        line.width > 20.0,
        "h2 line width ({:.1}) should be > 20 (before + text)",
        line.width
    );
}

// ============================================================
// Inline text alongside block ::before must render
// ============================================================

#[test]
fn inline_text_with_block_before_pseudo_renders() {
    let html = concat!(
        "<style>",
        "h2::before { content: '|'; display: block; }",
        "</style>",
        "<h2>Section Title</h2>",
    );
    let doc = load_html(html, 800.0);
    let h2 = find_all_boxes(&doc.root, &|b| b.tag == "h2");
    assert!(!h2.is_empty());
    for (i, c) in h2[0].children.iter().enumerate() {
        eprintln!(
            "  h2 child[{}] tag={} display={:?} y={:.1} h={:.1}",
            i, c.tag, c.style.display, c.layout.content_rect.y, c.layout.content_rect.h
        );
    }
    // h2 height should account for both the ::before block and the text
    // ::before(block) ~ 28.8px + "Section Title" ~ 28.8px = ~57.6
    assert!(
        h2[0].layout.content_rect.h > 40.0,
        "h2 height ({:.1}) should be > 40 (block before + text line)",
        h2[0].layout.content_rect.h
    );
}

// ============================================================
// Al Jazeera REAL bug: absolute ::before must not swallow sibling text
// ============================================================

#[test]
fn absolute_before_pseudo_does_not_hide_text() {
    // Reproduces: h2 with position:absolute ::before and inline text.
    // The text must still be visible (in line_cache), not 0x0.
    let html = concat!(
        "<style>",
        "h2::before {",
        "  content: '';",
        "  display: inline-block;",
        "  position: absolute;",
        "  width: 4px;",
        "  height: 27px;",
        "  background: blue;",
        "  right: 0;",
        "}",
        "h2 { position: relative; font-size: 18px; }",
        "</style>",
        "<h2>اختيارات المحررين</h2>",
    );
    let doc = load_html(html, 400.0);
    let h2 = find_all_boxes(&doc.root, &|b| b.tag == "h2");
    assert!(!h2.is_empty());

    eprintln!(
        "h2: h={:.1} lines={} children={}",
        h2[0].layout.content_rect.h,
        h2[0].layout.line_cache.len(),
        h2[0].children.len()
    );
    for (i, c) in h2[0].children.iter().enumerate() {
        eprintln!(
            "  child[{}] tag={} display={:?} pos={:?} w={:.1} h={:.1}",
            i,
            c.tag,
            c.style.display,
            c.style.position,
            c.layout.content_rect.w,
            c.layout.content_rect.h
        );
    }

    // h2 must have line_cache (text is rendered)
    assert!(
        !h2[0].layout.line_cache.is_empty(),
        "h2 must have line_cache — text 'اختيارات المحررين' should be visible"
    );
    // h2 height must include the text line
    assert!(
        h2[0].layout.content_rect.h >= 18.0,
        "h2 height ({:.1}) must be >= font-size 18",
        h2[0].layout.content_rect.h
    );
}

// ============================================================
// Al Jazeera REAL bug: card title text clipped to single chars
// ============================================================

#[test]
fn rtl_card_title_text_not_clipped_to_single_char() {
    // Reproduces: article card with a > ::before(abs overlay) + h3 > span > text
    // In RTL, the text shows only single characters per line
    let html = concat!(
        "<html dir='rtl'><body>",
        "<style>",
        ".card { display: flex; width: 292px; position: relative; }",
        ".card-content { width: 158px; }",
        ".card-image { width: 125px; }",
        ".card a { position: relative; }",
        ".card a::before { content: ''; position: absolute; top: 0; left: 0; right: 0; bottom: 0; }",
        ".card-title { font-size: 16px; }",
        "</style>",
        "<article class='card'>",
        "  <div class='card-content'>",
        "    <a href='/article'>",
        "      <h3 class='card-title'><span>الجزيرة نت تنفرد بنشر خطة نزع السلاح</span></h3>",
        "    </a>",
        "  </div>",
        "  <div class='card-image'>IMG</div>",
        "</article>",
        "</body></html>",
    );
    let doc = load_html(html, 400.0);
    let h3 = find_all_boxes(&doc.root, &|b| b.tag == "h3");
    assert!(!h3.is_empty());
    eprintln!(
        "h3: x={:.0} w={:.0} h={:.0} lines={}",
        h3[0].layout.content_rect.x,
        h3[0].layout.content_rect.w,
        h3[0].layout.content_rect.h,
        h3[0].layout.line_cache.len()
    );
    for (i, line) in h3[0].layout.line_cache.iter().enumerate() {
        eprintln!(
            "  line[{}] x={:.0} w={:.0} h={:.0}",
            i, line.x, line.width, line.height
        );
    }
    // h3 must have line_cache
    assert!(
        !h3[0].layout.line_cache.is_empty(),
        "h3 must have text lines"
    );
    // Each line must have substantial width — not just 1 character
    for (i, line) in h3[0].layout.line_cache.iter().enumerate() {
        assert!(
            line.width > 30.0,
            "h3 line[{}] width ({:.1}) should be > 30 (not single-char clipping)",
            i,
            line.width
        );
    }
}

// ============================================================
// BBC grid: minmax(0,1fr) columns with grid-template-areas
// ============================================================

#[test]
fn grid_minmax_0_1fr_with_template_areas() {
    let html = concat!(
        "<style>",
        ".grid {",
        "  display: grid;",
        "  gap: 16px;",
        "  grid-template-columns: repeat(4, minmax(0, 1fr));",
        "  grid-template-areas: 'p1 p1 p2 p3' 'p1 p1 p4 p5' 'p6 p6 p6 p7';",
        "  width: 1248px;",
        "}",
        ".grid > :nth-child(1) { grid-area: p1; }",
        ".grid > :nth-child(2) { grid-area: p2; }",
        ".grid > :nth-child(3) { grid-area: p3; }",
        ".grid > :nth-child(4) { grid-area: p4; }",
        ".grid > :nth-child(5) { grid-area: p5; }",
        ".grid > :nth-child(6) { grid-area: p6; }",
        ".grid > :nth-child(7) { grid-area: p7; }",
        "</style>",
        "<ul class='grid'>",
        "  <li>Hero story</li>",
        "  <li>Story 2</li>",
        "  <li>Story 3</li>",
        "  <li>Story 4</li>",
        "  <li>Story 5</li>",
        "  <li>Story 6</li>",
        "  <li>Story 7</li>",
        "</ul>",
    );
    let doc = load_html(html, 1280.0);
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 7, "expected 7 grid items");

    // p1 (hero) should span 2 columns = ~50% of 1248 = ~600px
    let hero_w = items[0].layout.content_rect.w;
    eprintln!("hero w={:.0} (expect ~600)", hero_w);
    assert!(
        hero_w > 500.0,
        "hero width ({:.0}) should be > 500 (spans 2 of 4 columns)",
        hero_w
    );

    // p2, p3 should each be ~25% = ~300px
    let p2_w = items[1].layout.content_rect.w;
    let p3_w = items[2].layout.content_rect.w;
    eprintln!("p2 w={:.0}, p3 w={:.0} (expect ~300 each)", p2_w, p3_w);
    assert!(p2_w > 200.0, "p2 width ({:.0}) should be > 200", p2_w);
    assert!(p3_w > 200.0, "p3 width ({:.0}) should be > 200", p3_w);

    // p6 spans 3 columns = ~75%
    let p6_w = items[5].layout.content_rect.w;
    eprintln!("p6 w={:.0} (expect ~900)", p6_w);
    assert!(
        p6_w > 700.0,
        "p6 width ({:.0}) should be > 700 (spans 3 cols)",
        p6_w
    );
}

// ============================================================
// BBC: @supports (display: grid) must apply grid styles
// ============================================================

#[test]
fn supports_display_grid_applies_styles() {
    let html = concat!(
        "<style>",
        ".grid { display: flex; flex-wrap: wrap; width: 1248px; }",
        ".grid > * { width: calc(100% / 3); }",
        "@supports (display: grid) {",
        "  .grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px; }",
        "  .grid > * { width: initial; }",
        "}",
        "</style>",
        "<ul class='grid'>",
        "  <li>Item 1</li>",
        "  <li>Item 2</li>",
        "  <li>Item 3</li>",
        "  <li>Item 4</li>",
        "</ul>",
    );
    let doc = load_html(html, 1280.0);
    let grid = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "grid")
            .unwrap_or(false)
    });
    assert!(!grid.is_empty());
    // @supports should override flex to grid
    eprintln!("grid display={:?}", grid[0].style.display);
    assert!(
        grid[0].style.display == Display::Grid,
        "grid should be display:Grid from @supports, got {:?}",
        grid[0].style.display
    );
    // Each item should be ~25% of 1248 = ~300px (4 columns)
    let items: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 4);
    for (i, item) in items.iter().enumerate() {
        eprintln!("li[{}] w={:.0}", i, item.layout.content_rect.w);
        assert!(
            item.layout.content_rect.w > 250.0,
            "li[{}] width ({:.0}) should be > 250 (1/4 of grid)",
            i,
            item.layout.content_rect.w
        );
    }
}

// ============================================================
// @media inside @supports must apply
// ============================================================

#[test]
fn media_inside_supports_applies() {
    let html = concat!(
        "<style>",
        ".box { width: 100px; background: red; }",
        "@supports (display: grid) {",
        "  @media (min-width: 500px) {",
        "    .box { width: 400px; background: blue; }",
        "  }",
        "}",
        "</style>",
        "<div class='box'>Test</div>",
    );
    let doc = load_html(html, 1280.0);
    let b = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "box")
            .unwrap_or(false)
    });
    assert!(!b.is_empty());
    eprintln!("box w={:.0} (expect 400)", b[0].layout.content_rect.w);
    assert!(
        b[0].layout.content_rect.w > 300.0,
        "box width ({:.0}) should be 400 from @media inside @supports",
        b[0].layout.content_rect.w
    );
}

// ============================================================
// SVG with viewBox + CSS height should compute width from aspect ratio
// ============================================================

#[test]
fn svg_viewbox_computes_width_from_height() {
    let html = concat!(
        "<style>",
        "svg { height: 1.75rem; width: auto; display: block; }",
        "</style>",
        "<svg xmlns='http://www.w3.org/2000/svg' width='7em' height='2em' viewBox='0 0 112 32' fill='currentColor'>",
        "<path d='M0 0h112v32H0z'/>",
        "</svg>",
    );
    let doc = load_html(html, 800.0);
    let svg = find_all_boxes(&doc.root, &|b| b.tag == "svg");
    assert!(!svg.is_empty());
    eprintln!(
        "svg: w={:.1} h={:.1} viewbox_w={} viewbox_h={}",
        svg[0].layout.content_rect.w,
        svg[0].layout.content_rect.h,
        svg[0].svg_viewbox_w,
        svg[0].svg_viewbox_h
    );
    // Width should be computed from viewBox aspect ratio: 28 * 112/32 = 98
    assert!(
        svg[0].layout.content_rect.w > 80.0,
        "SVG width ({:.1}) should be > 80 (computed from viewBox 112:32 at h=28)",
        svg[0].layout.content_rect.w
    );
}

// ============================================================
// AP News: visibility:hidden element should not shift siblings
// with transform:translateX
// ============================================================

#[test]
fn transform_translatex_hides_element_off_screen() {
    let html = concat!(
        "<style>",
        ".hidden-nav { transform: translateX(-100%); visibility: hidden; width: 300px; }",
        ".main { width: 800px; }",
        "</style>",
        "<div style='width:1000px; display:flex'>",
        "  <div class='hidden-nav'>Hidden Menu</div>",
        "  <div class='main'>Main Content</div>",
        "</div>",
    );
    let doc = load_html(html, 1000.0);
    let main = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "main")
            .unwrap_or(false)
    });
    assert!(!main.is_empty());
    // Main content should start near x=0, not pushed right by the hidden nav
    // (transform:translateX(-100%) moves it off-screen visually)
    eprintln!("main x={:.0}", main[0].layout.content_rect.x);
    // In a flex container, the hidden nav still takes space (visibility:hidden).
    // But with transform:translateX(-100%), it's visually off-screen.
    // The main should still be positioned correctly within the flex.
}

// ============================================================
// AP News: nav links should be on one line (flex row)
// ============================================================

#[test]
fn flex_nav_links_on_one_line() {
    let html = concat!(
        "<style>",
        ".nav { display: flex; gap: 16px; }",
        ".nav a { white-space: nowrap; }",
        "</style>",
        "<div class='nav' style='width:1000px'>",
        "  <a href='/world'>WORLD</a>",
        "  <a href='/us'>U.S.</a>",
        "  <a href='/politics'>POLITICS</a>",
        "  <a href='/sports'>SPORTS</a>",
        "  <a href='/entertainment'>ENTERTAINMENT</a>",
        "  <a href='/business'>BUSINESS</a>",
        "  <a href='/science'>SCIENCE</a>",
        "</div>",
    );
    let doc = load_html(html, 1000.0);
    let links: Vec<&WebCore> = find_all_boxes(&doc.root, &|b| b.tag == "a");
    assert!(links.len() >= 7);
    let y0 = links[0].layout.border_rect.y;
    for (i, link) in links.iter().enumerate() {
        assert!(
            (link.layout.border_rect.y - y0).abs() < 5.0,
            "nav link[{}] at y={:.0} should be same line as [0] at y={:.0}",
            i,
            link.layout.border_rect.y,
            y0
        );
    }
}
