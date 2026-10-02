use crate::tests::harness::{find_box, parse_and_layout};
use crate::types::*;

fn find_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    find_box(node, &|b| {
        b.attributes.get("id").map(|s| s == id).unwrap_or(false)
    })
}

// ── container-type / container-name parsing ───────────────────────────────────

#[test]
fn container_type_inline_size_parsed() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div id="c" style="container-type: inline-size; width: 300px">hello</div>
        </body></html>"#,
        800.0,
    );
    let c = find_by_id(&doc.root, "c").expect("c");
    assert_eq!(c.style.container_type, ContainerType::InlineSize);
}

#[test]
fn container_type_size_parsed() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div id="c" style="container-type: size; width: 300px; height: 200px">hello</div>
        </body></html>"#,
        800.0,
    );
    let c = find_by_id(&doc.root, "c").expect("c");
    assert_eq!(c.style.container_type, ContainerType::Size);
}

#[test]
fn container_name_parsed() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div id="c" style="container-type: inline-size; container-name: sidebar; width: 200px">x</div>
        </body></html>"#,
        800.0,
    );
    let c = find_by_id(&doc.root, "c").expect("c");
    assert_eq!(c.style.container_name, "sidebar");
}

#[test]
fn container_shorthand_parsed() {
    let doc = parse_and_layout(
        r#"<html><body>
          <div id="c" style="container: mybox / inline-size; width: 200px">x</div>
        </body></html>"#,
        800.0,
    );
    let c = find_by_id(&doc.root, "c").expect("c");
    assert_eq!(c.style.container_name, "mybox");
    assert_eq!(c.style.container_type, ContainerType::InlineSize);
}

// ── @container rule parsing ───────────────────────────────────────────────────

#[test]
fn container_rule_parsed_in_stylesheet() {
    use crate::css::Stylesheet;
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        r#"
        .card { container-type: inline-size; }
        @container (min-width: 300px) {
            .inner { font-size: 20px; }
        }
    "#,
    );
    let has_container_rule = ss.rules.iter().any(|r| !r.container_condition.is_empty());
    assert!(
        has_container_rule,
        "stylesheet should have at least one @container rule"
    );
}

#[test]
fn named_container_rule_parsed() {
    use crate::css::Stylesheet;
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        r#"
        @container sidebar (min-width: 200px) {
            .item { color: red; }
        }
    "#,
    );
    let rule = ss
        .rules
        .iter()
        .find(|r| !r.container_condition.is_empty())
        .expect("rule");
    assert_eq!(rule.container_name, "sidebar");
    assert!(rule.container_condition.contains("min-width"));
}

// ── evaluate_container ────────────────────────────────────────────────────────

#[test]
fn evaluate_container_min_width() {
    use crate::css::evaluate_container;
    assert!(evaluate_container("(min-width: 300px)", 400.0, 200.0));
    assert!(!evaluate_container("(min-width: 300px)", 200.0, 200.0));
    assert!(evaluate_container("(min-width: 300px)", 300.0, 200.0)); // equal = true
}

#[test]
fn evaluate_container_max_width() {
    use crate::css::evaluate_container;
    assert!(evaluate_container("(max-width: 500px)", 400.0, 200.0));
    assert!(!evaluate_container("(max-width: 500px)", 600.0, 200.0));
}

#[test]
fn evaluate_container_range_syntax() {
    use crate::css::evaluate_container;
    assert!(evaluate_container("(width > 200px)", 300.0, 100.0));
    assert!(!evaluate_container("(width > 200px)", 100.0, 100.0));
    assert!(evaluate_container("(width >= 300px)", 300.0, 100.0));
    assert!(evaluate_container("(width < 400px)", 300.0, 100.0));
    assert!(!evaluate_container("(width < 400px)", 500.0, 100.0));
}

#[test]
fn evaluate_container_and_combinator() {
    use crate::css::evaluate_container;
    assert!(evaluate_container(
        "(min-width: 200px) and (max-width: 600px)",
        400.0,
        100.0
    ));
    assert!(!evaluate_container(
        "(min-width: 200px) and (max-width: 600px)",
        100.0,
        100.0
    ));
    assert!(!evaluate_container(
        "(min-width: 200px) and (max-width: 600px)",
        700.0,
        100.0
    ));
}

#[test]
fn container_orientation_and_aspect_ratio_require_matching_size() {
    use crate::css::evaluate_container;
    assert!(evaluate_container("(orientation: landscape)", 300.0, 200.0));
    assert!(!evaluate_container("(orientation: portrait)", 300.0, 200.0));
    assert!(evaluate_container("(orientation: portrait)", 200.0, 200.0));
    assert!(evaluate_container("(aspect-ratio: 3 / 2)", 300.0, 200.0));
    assert!(evaluate_container("(min-aspect-ratio: 4/3)", 300.0, 200.0));
    assert!(!evaluate_container("(max-aspect-ratio: 4/3)", 300.0, 200.0));
    assert!(evaluate_container("(1 < aspect-ratio < 2)", 300.0, 200.0));
    assert!(!evaluate_container("(aspect-ratio > 2)", 300.0, 200.0));
    assert!(!evaluate_container("(unknown-feature: yes)", 300.0, 200.0));
    assert!(!crate::css::evaluate_container_for_type(
        "(orientation: landscape)",
        300.0,
        200.0,
        crate::types::ContainerType::InlineSize,
    ));
}

#[test]
fn zero_height_container_aspect_ratio_uses_ratio_comparison() {
    use crate::css::evaluate_container;

    assert!(evaluate_container("(min-aspect-ratio: 2/1)", 100.0, 0.0));
    assert!(!evaluate_container("(max-aspect-ratio: 2/1)", 100.0, 0.0));
    assert!(evaluate_container("(aspect-ratio: 1/0)", 100.0, 0.0));
    assert!(evaluate_container("(aspect-ratio >= 1/0)", 100.0, 0.0));
    assert!(!evaluate_container("(aspect-ratio > 1/0)", 100.0, 0.0));

    assert!(!evaluate_container("(aspect-ratio: 1/1)", 0.0, 0.0));
    assert!(!evaluate_container("(aspect-ratio: 0/0)", 0.0, 0.0));
    assert!(!evaluate_container("(min-aspect-ratio: 0/0)", 100.0, 0.0));
}

#[test]
fn zero_height_size_container_applies_aspect_ratio_rule() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: size; width: 100px; height: 0; }
          .target { color: red; }
          @container (min-aspect-ratio: 2/1) { .target { color: green; } }
        </style></head><body><div class="outer"><span id="target" class="target">x</span></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn name_only_container_query_selects_named_ancestor() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-name: card; }
          .target { color: red; }
          @container card { .target { color: green; } }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn orientation_query_skips_inline_size_container_but_style_query_uses_it() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: size; width: 300px; height: 200px; }
          .inner { container-type: inline-size; width: 100px; --aspect-ratio: wide; }
          .target { color: red; background-color: white; }
          @container (orientation: portrait) { .target { color: blue; } }
          @container (orientation: landscape) { .target { color: green; } }
          @container style(--aspect-ratio: wide) {
            .target { background-color: blue; }
          }
        </style></head><body><div class="outer"><div class="inner"><div id="target" class="target">x</div></div></div></body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").unwrap();
    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
    assert_eq!(target.style.background_color, Color::rgb(0, 0, 255));
}

// ── Layout effect of @container rules ─────────────────────────────────────────

#[test]
fn container_query_candidates_keep_stylesheet_source_order() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 400px; }
          @container (min-width: 300px) {
            .earlier { color: red; }
            .later { color: blue; }
          }
        </style></head><body>
          <div class="outer"><div id="target" class="later earlier">text</div></div>
        </body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").expect("target");
    assert_eq!(
        (
            target.style.color.r,
            target.style.color.g,
            target.style.color.b
        ),
        (0, 0, 255)
    );
}

#[test]
fn container_query_preserves_nested_and_ancestor_type_positions() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 400px; }
          @container (min-width: 300px) {
            .group:nth-of-type(2) span:is(:nth-of-type(2)) { color: red; }
          }
        </style></head><body><div class="outer">
          <div class="group"><span id="first-group">first</span></div>
          <div class="group">
            <span id="first-child">first</span>
            <em>other type</em>
            <span id="target">second</span>
          </div>
        </div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(255, 0, 0)
    );
    assert_ne!(
        find_by_id(&doc.root, "first-child").unwrap().style.color,
        Color::rgb(255, 0, 0)
    );
    assert_ne!(
        find_by_id(&doc.root, "first-group").unwrap().style.color,
        Color::rgb(255, 0, 0)
    );
}

#[test]
fn container_query_applies_style_when_wide() {
    // Container is 400px wide; rule fires at min-width: 300px → font-size: 24px
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 400px; }
          @container (min-width: 300px) {
            .inner { font-size: 24px; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div id="inner" class="inner">text</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let inner = find_by_id(&doc.root, "inner").expect("inner");
    // font_size is stored as CssLength; we check font_size_px resolves to ~24px
    let font_px = inner.style.font_size.resolve(16.0, 0.0, 16.0);
    assert!(
        (font_px - 24.0).abs() < 1.0,
        "font-size should be 24px when container is wide, got {}",
        font_px
    );
}

#[test]
fn container_query_does_not_apply_when_narrow() {
    // Container is 200px wide; rule fires at min-width: 300px → should NOT apply
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 200px; }
          @container (min-width: 300px) {
            .inner { font-size: 32px; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div id="inner" class="inner">text</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let inner = find_by_id(&doc.root, "inner").expect("inner");
    let font_px = inner.style.font_size.resolve(16.0, 0.0, 16.0);
    // Default font-size is 16px; should NOT be 32px
    assert!(
        (font_px - 32.0).abs() > 1.0,
        "font-size should NOT be 32px when container is narrow, got {}",
        font_px
    );
}

#[test]
fn container_query_width_changes_box_size() {
    // Container is 500px; @container rule sets inner width: 200px when >= 400px.
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 500px; }
          @container (min-width: 400px) {
            .inner { width: 200px; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div id="inner" class="inner">x</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let inner = find_by_id(&doc.root, "inner").expect("inner");
    assert!(
        (inner.layout.border_rect.w - 200.0).abs() < 2.0,
        "inner width should be 200px, got {}",
        inner.layout.border_rect.w
    );
}

#[test]
fn container_queries_rerun_until_dependent_container_sizes_settle() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 200px; }
          .middle { container-type: inline-size; width: 100px; }
          .target { width: 20px; }
          @container (min-width: 150px) {
            .middle { width: 300px; }
          }
          @container (min-width: 250px) {
            .target { width: 120px; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div class="middle">
              <div id="target" class="target">x</div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").expect("target");
    assert!(
        (target.layout.border_rect.w - 120.0).abs() < 1.0,
        "second-pass container query should see the resized middle container, got {}",
        target.layout.border_rect.w
    );
}

#[test]
fn unchanged_container_query_matches_do_not_request_another_layout() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 200px; }
          @container (min-width: 150px) { .target { color: red; } }
        </style></head><body><div class="outer"><div class="target">x</div></div></body></html>"#,
        800.0,
    );
    let mut applied_rules = std::collections::HashMap::new();
    let apply = |doc: &mut crate::types::Document,
                 applied_rules: &mut std::collections::HashMap<
        u32,
        crate::css::container::AppliedContainerStyle,
    >| {
        crate::css::apply_container_cascade_tree_with_state(
            &mut doc.root,
            &doc.stylesheet,
            &[],
            &[],
            0,
            1,
            0,
            1,
            16.0,
            800.0,
            600.0,
            0,
            false,
            applied_rules,
        )
    };
    assert!(!apply(&mut doc, &mut applied_rules));
    assert!(!apply(&mut doc, &mut applied_rules));
}

#[test]
fn container_query_rule_removal_restores_the_pre_query_style() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 250px; }
          .target { color: blue; }
          @container (min-width: 200px) { .target { color: red; } }
        </style></head><body><div id="outer" class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    let outer_id = find_by_id(&doc.root, "outer").unwrap().node_id;
    let target_id = find_by_id(&doc.root, "target").unwrap().node_id;
    let target = doc.get_box_by_id_mut(target_id).unwrap();
    crate::css::apply_property(std::sync::Arc::make_mut(&mut target.style), "color", "blue");
    let mut applied_rules = std::collections::HashMap::new();
    let mut apply = |doc: &mut crate::types::Document| {
        crate::css::apply_container_cascade_tree_with_state(
            &mut doc.root,
            &doc.stylesheet,
            &[],
            &[],
            0,
            1,
            0,
            1,
            16.0,
            800.0,
            600.0,
            0,
            false,
            &mut applied_rules,
        )
    };
    assert!(apply(&mut doc));
    assert_eq!(doc.get_node(target_id).unwrap().style.color.r, 255);
    doc.get_box_by_id_mut(outer_id)
        .unwrap()
        .layout
        .content_rect
        .w = 100.0;
    assert!(apply(&mut doc));
    assert_eq!(doc.get_node(target_id).unwrap().style.color.b, 255);
    assert!(!apply(&mut doc));
}

#[test]
fn changed_container_rule_set_with_same_style_is_a_no_op() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 250px; }
          .target { color: blue; }
          @container (min-width: 150px) { .target { color: red; } }
          @container (min-width: 200px) { .target { color: red; } }
        </style></head><body><div id="outer" class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    let outer_id = find_by_id(&doc.root, "outer").unwrap().node_id;
    let target_id = find_by_id(&doc.root, "target").unwrap().node_id;
    let mut applied_rules = std::collections::HashMap::new();
    let mut apply = |doc: &mut crate::types::Document| {
        crate::css::apply_container_cascade_tree_with_state(
            &mut doc.root,
            &doc.stylesheet,
            &[],
            &[],
            0,
            1,
            0,
            1,
            16.0,
            800.0,
            600.0,
            0,
            false,
            &mut applied_rules,
        )
    };
    assert!(!apply(&mut doc));
    doc.get_box_by_id_mut(outer_id)
        .unwrap()
        .layout
        .content_rect
        .w = 175.0;
    assert!(!apply(&mut doc));
    assert_eq!(doc.get_node(target_id).unwrap().style.color.r, 255);
}

#[test]
fn inline_size_container_does_not_answer_height_queries() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 300px; height: 400px; }
          @container (min-height: 300px) {
            .inner { background: red; }
          }
          @container (min-width: 200px) {
            .inner { color: blue; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div id="inner" class="inner">x</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let inner = find_by_id(&doc.root, "inner").expect("inner");
    assert_eq!(
        inner.style.color.b, 255,
        "inline-size container should still answer inline/width queries"
    );
    assert_eq!(
        inner.style.background_color.a, 0,
        "inline-size container must not answer block/height queries"
    );
}

#[test]
fn unsupported_container_style_queries_fail_closed() {
    let html = r#"
        <style>
          .outer { container-type: size; width: 300px; height: 200px; }
          .target { color: red; }
          @container style(color: red) {
            .target { color: green; }
          }
        </style>
        <div class="outer"><div id="target" class="target">x</div></div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let target = find_by_id(&doc.root, "target").expect("target");

    assert_eq!(
        target.style.color,
        Color::rgb(255, 0, 0),
        "unsupported style queries must not fail open and apply inner rules"
    );
}

#[test]
fn container_style_query_matches_computed_container_style() {
    let html = r#"
        <style>
          .outer { container-type: size; width: 300px; height: 200px; display: block; }
          .target { color: red; }
          @container style(display: block) {
            .target { color: green; }
          }
        </style>
        <div class="outer"><div id="target" class="target">x</div></div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let target = find_by_id(&doc.root, "target").expect("target");

    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
}

#[test]
fn container_style_query_can_be_combined_with_size_conditions() {
    let html = r#"
        <style>
          .outer {
            container-type: size;
            width: 300px;
            height: 200px;
            background-color: rgb(1, 2, 3);
          }
          .target { color: red; }
          @container (min-width: 250px) and style(background-color: rgb(1, 2, 3)) {
            .target { color: green; }
          }
        </style>
        <div class="outer"><div id="target" class="target">x</div></div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let target = find_by_id(&doc.root, "target").expect("target");

    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
}

#[test]
fn container_style_query_matches_color_values() {
    let html = r#"
        <style>
          .outer { container-type: size; width: 300px; height: 200px; color: rgb(255, 0, 0); }
          .target { color: red; }
          @container style(color: rgb(255, 0, 0)) {
            .target { color: green; }
          }
        </style>
        <div class="outer"><div id="target" class="target">x</div></div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let target = find_by_id(&doc.root, "target").expect("target");

    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
}

#[test]
fn style_query_uses_nearest_normal_container_and_preserves_custom_property_case() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --Theme: Warm; }
          .target { color: red; }
          @container style(--Theme: Warm) { .target { color: green; } }
          @container style(--Theme: warm) { .target { color: blue; } }
          @container style(--theme: Warm) { .target { color: blue; } }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn size_query_skips_normal_and_incompatible_inline_size_containers() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: size; width: 300px; height: 300px; }
          .middle { container-type: inline-size; width: 100px; }
          .target { color: red; }
          @container (min-height: 250px) { .target { color: green; } }
        </style></head><body><div class="outer"><div class="middle"><div class="plain"><div id="target" class="target">x</div></div></div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn comma_container_alternative_retains_style_query_context() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --mode: live; }
          .target { color: red; }
          @container (min-width: 200px), style(--mode: live) {
            .target { color: green; }
          }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn comma_container_alternatives_select_their_own_named_ancestors() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container: card / size; width: 100px; height: 100px; }
          .inner { container-name: theme; --mode: dark; }
          .target { color: red; }
          @container card (min-width: 200px), theme style(--mode: dark) {
            .target { color: green; }
          }
        </style></head><body><div class="outer"><div class="inner"><div id="target" class="target">x</div></div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn nested_container_queries_keep_both_named_conditions() {
    for (mode, expected) in [
        ("dark", Color::rgb(0, 128, 0)),
        ("light", Color::rgb(255, 0, 0)),
    ] {
        let html = format!(
            r#"<html><head><style>
              .outer {{ container: card / size; width: 300px; height: 100px; }}
              .inner {{ container-name: theme; --mode: {mode}; }}
              .target {{ color: red; }}
              @container card (min-width: 200px) {{
                @container theme style(--mode: dark) {{
                  .target {{ color: green; }}
                }}
              }}
            </style></head><body><div class="outer"><div class="inner"><div id="target" class="target">x</div></div></div></body></html>"#
        );
        let doc = parse_and_layout(&html, 800.0);
        assert_eq!(
            find_by_id(&doc.root, "target").unwrap().style.color,
            expected,
            "mode={mode}"
        );
    }
}

#[test]
fn nested_container_queries_and_their_comma_alternatives() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container: card / size; width: 100px; height: 100px; }
          .inner { container-name: theme; --mode: dark; }
          .target { color: red; }
          @container card (min-width: 200px), theme style(--mode: dark) {
            @container card (min-height: 200px) { .target { color: green; } }
          }
        </style></head><body><div class="outer"><div class="inner"><div id="target" class="target">x</div></div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(255, 0, 0)
    );
}

#[test]
fn boolean_custom_property_style_query_uses_computed_presence() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --enabled: yes; }
          .target { color: red; }
          @container style(--enabled) { .target { color: green; } }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn container_style_query_combines_parenthesized_features() {
    for (mode, enabled, expected) in [
        ("dark", "yes", Color::rgb(0, 128, 0)),
        ("light", "yes", Color::rgb(255, 0, 0)),
        ("dark", "no", Color::rgb(255, 0, 0)),
    ] {
        let html = format!(
            r#"<html><head><style>
              .outer {{ --mode: {mode}; --enabled: {enabled}; }}
              .target {{ color: red; }}
              @container style((--mode: dark) and (--enabled: yes)) {{
                .target {{ color: green; }}
              }}
            </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#
        );
        let doc = parse_and_layout(&html, 800.0);
        assert_eq!(
            find_by_id(&doc.root, "target").unwrap().style.color,
            expected,
            "mode={mode}, enabled={enabled}"
        );
    }
}

#[test]
fn container_style_query_supports_or_not_and_literal_and_values() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --mode: red and blue; --enabled: yes; }
          .target { color: red; }
          @container style((--mode: red and blue) or (--enabled: no)) {
            .target { color: green; }
          }
          @container style(not (--enabled: yes)) { .target { color: blue; } }
          @container style((--mode: absent) or (--enabled: yes)) {
            .target { font-weight: bold; }
          }
          @container style(not (--enabled: no)) {
            .target { background-color: blue; }
          }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
    assert_eq!(
        find_by_id(&doc.root, "target")
            .unwrap()
            .style
            .font_weight
            .value(),
        700
    );
    assert_eq!(
        find_by_id(&doc.root, "target")
            .unwrap()
            .style
            .background_color,
        Color::rgb(0, 0, 255)
    );
}

#[test]
fn container_style_query_compares_computed_weight_and_resolved_color() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --ink: #008000; color: rgb(0, 128, 0); font-weight: bold; }
          .target { color: red; }
          @container style(color: var(--ink)) and style(font-weight: 700) {
            .target { color: green; }
          }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn invalid_container_size_thresholds_do_not_match_as_zero() {
    for condition in [
        "(min-width: nonsense)",
        "(max-width: nonsense)",
        "(min-height: auto)",
        "(max-height: 10%)",
        "(width > nonsense)",
        "(width < nonsense)",
        "(height <= auto)",
        "(width >= 5)",
        "(width >= -1px)",
    ] {
        assert!(
            !crate::css::evaluate_container(condition, 100.0, 100.0),
            "invalid query matched: {condition}"
        );
    }
    assert!(crate::css::evaluate_container("(width >= 0)", 100.0, 100.0));
    assert!(crate::css::evaluate_container(
        "(width >= 0.0)",
        100.0,
        100.0
    ));
    assert!(crate::css::evaluate_container(
        "(width >= 1rem)",
        100.0,
        100.0
    ));
}

#[test]
fn container_viewport_units_use_viewport_not_container_size() {
    use crate::css::evaluate_container_with_viewport;

    assert!(evaluate_container_with_viewport(
        "(min-width: 20vw) and (max-width: 30vw)",
        200.0,
        120.0,
        ContainerType::Size,
        None,
        800.0,
        600.0,
    ));
    assert!(!evaluate_container_with_viewport(
        "(min-width: 50vw)",
        200.0,
        120.0,
        ContainerType::Size,
        None,
        800.0,
        600.0,
    ));
    assert!(!evaluate_container_with_viewport(
        "(min-height: 50vh)",
        200.0,
        120.0,
        ContainerType::Size,
        None,
        800.0,
        600.0,
    ));

    let doc = parse_and_layout(
        r#"<style>
            .container { container-type: inline-size; width: 200px; }
            .target { color: red; }
            @container (min-width: 20vw) and (max-width: 30vw) {
                .target { color: green; }
            }
            @container (min-width: 50vw) { .target { color: blue; } }
        </style>
        <div class="container"><div id="target" class="target">x</div></div>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn container_size_ranges_support_value_first_and_two_sided_syntax() {
    for condition in [
        "(100px < width)",
        "(100px<width)",
        "(100px < width <= 200px)",
        "(100px<width<=200px)",
        "(100px <width<= 200px)",
        "(100px < inline-size <= 200px)",
        "(100px<inline-size<=200px)",
        "(200px >= width > 100px)",
        "(200px>=width>100px)",
        "(height >= 120px)",
        "(100px <= block-size <= 120px)",
        "(100px<=block-size<=120px)",
        "(width = 200px)",
    ] {
        assert!(
            crate::css::evaluate_container(condition, 200.0, 120.0),
            "{condition}"
        );
    }
    for condition in [
        "(200px < width)",
        "(100px < width < 200px)",
        "(100px<width<200px)",
        "(100px < width > 300px)",
        "(100px<width>300px)",
        "(bogus < width < 300px)",
        "(100px<<width)",
        "(100px<width<=)",
        "(100px<width<300px<400px)",
        "(height = 120.25px)",
    ] {
        assert!(
            !crate::css::evaluate_container(condition, 200.0, 120.0),
            "{condition}"
        );
    }
    assert!(!crate::css::evaluate_container_for_type(
        "(100px < block-size)",
        200.0,
        120.0,
        ContainerType::InlineSize,
    ));

    let doc = parse_and_layout(
        r#"<style>
            .container { container-type: inline-size; width: 200px; }
            .target { color: red; }
            @container (100px<width<=200px) { .target { color: green; } }
        </style>
        <div class="container"><div id="range-target" class="target">x</div></div>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "range-target").unwrap().style.color,
        Color::rgb(0, 128, 0),
    );
}

#[test]
fn container_style_ranges_compare_values_and_reject_mixed_types() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --size: 12px; --ratio: 1.5; }
          .target { color: red; background-color: white; }
          @container style(10px < --size < 14px) {
            .target { color: green; }
          }
          @container style(--size > 14px) {
            .target { color: blue; }
          }
          @container style(--ratio >= 1) {
            .target { background-color: blue; }
          }
          @container style(--size > 10%) {
            .target { background-color: red; }
          }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").unwrap();
    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
    assert_eq!(target.style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn container_style_ranges_convert_compatible_units_and_unitless_zero() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --size: 12px; --angle: 0.5turn; --duration: 250ms; --label: "a < b"; }
          .target { color: red; background-color: white; }
          @container style(0 < --size) { .target { color: green; } }
          @container style(180deg = --angle) { .target { background-color: blue; } }
          @container style(--duration > 0.2s) { .target { font-weight: bold; } }
          @container style(--angle > 200ms) { .target { color: blue; } }
          @container style(--label: "a < b") { .target { font-style: italic; } }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").unwrap();
    assert_eq!(target.style.color, Color::rgb(0, 128, 0));
    assert_eq!(target.style.background_color, Color::rgb(0, 0, 255));
    assert_eq!(target.style.font_weight.value(), 700);
    assert_eq!(target.style.font_style, crate::types::FontStyle::Italic);
}

#[test]
fn container_style_ranges_evaluate_typed_math_without_mixing_dimensions() {
    let mut style = crate::types::ComputedStyle::default();
    for (name, value) in [
        ("--size", "12px"),
        ("--share", "50%"),
        ("--angle", "180deg"),
        ("--time", "250ms"),
        ("--frequency", "2kHz"),
        ("--density", "2dppx"),
    ] {
        std::sync::Arc::make_mut(&mut style.custom_props).insert(name.into(), value.into());
    }
    let matches = |condition| {
        crate::css::evaluate_container_for_type_and_style(
            condition,
            0.0,
            0.0,
            crate::types::ContainerType::Normal,
            Some(&style),
        )
    };
    for condition in [
        "style(calc(6px + 6px) = --size)",
        "style(calc(25% * 2) = --share)",
        "style(calc(0.5turn) = --angle)",
        "style(calc(0.2s + 50ms) = --time)",
        "style(calc(1kHz * 2) = --frequency)",
        "style(calc(96dpi * 2) = --density)",
        "style(calc(3 * 4) > 10)",
    ] {
        assert!(matches(condition), "{condition}");
    }
    for condition in [
        "style(calc(1em + 2px) = --size)",
        "style(calc(50%) = --size)",
        "style(calc(1s) = --frequency)",
    ] {
        assert!(!matches(condition), "{condition}");
    }
}

#[test]
fn container_style_math_range_applies_in_document_cascade() {
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { --size: calc(4px * 3); }
          .target { color: red; }
          @container style(calc(6px + 6px) = --size) {
            .target { color: green; }
          }
          @container style(calc(1s) = --size) {
            .target { color: blue; }
          }
        </style></head><body><div class="outer"><div id="target" class="target">x</div></div></body></html>"#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "target").unwrap().style.color,
        Color::rgb(0, 128, 0)
    );
}

#[test]
fn container_query_max_width_applies_when_narrow() {
    // Container is 150px; rule fires at max-width: 200px → color red
    let doc = parse_and_layout(
        r#"<html><head><style>
          .outer { container-type: inline-size; width: 150px; }
          @container (max-width: 200px) {
            .inner { background: red; }
          }
        </style></head><body style="margin:0">
          <div class="outer">
            <div id="inner" class="inner">x</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let inner = find_by_id(&doc.root, "inner").expect("inner");
    // Red background = rgba(255,0,0,255)
    assert_eq!(
        inner.style.background_color.r, 255,
        "background should be red when container is narrow"
    );
}

#[test]
fn named_container_query_matches_correct_ancestor() {
    // Two containers: outer "sidebar" (100px) and inner "main" (400px).
    // Rule targets "main" container at min-width 300px → should apply to .target.
    let doc = parse_and_layout(
        r#"<html><head><style>
          .sidebar { container-type: inline-size; container-name: sidebar; width: 100px; }
          .main    { container-type: inline-size; container-name: main;    width: 400px; }
          @container main (min-width: 300px) {
            .target { background: blue; }
          }
        </style></head><body style="margin:0">
          <div class="sidebar">
            <div class="main">
              <div id="target" class="target">x</div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").expect("target");
    assert_eq!(
        target.style.background_color.b, 255,
        "background should be blue via named container 'main'"
    );
}

#[test]
fn named_container_query_does_not_match_wrong_name() {
    // Rule targets container named "sidebar" (100px) at min-width 300px → should NOT apply
    let doc = parse_and_layout(
        r#"<html><head><style>
          .sidebar { container-type: inline-size; container-name: sidebar; width: 100px; }
          @container sidebar (min-width: 300px) {
            .target { background: green; }
          }
        </style></head><body style="margin:0">
          <div class="sidebar">
            <div id="target" class="target">x</div>
          </div>
        </body></html>"#,
        800.0,
    );
    let target = find_by_id(&doc.root, "target").expect("target");
    // Green background = rgba(0,128,0,255)
    assert!(
        target.style.background_color.g < 200,
        "background should NOT be green: sidebar is only 100px, rule needs 300px"
    );
}

#[test]
fn container_length_units_use_nearest_eligible_axis() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;width:400px;height:300px">
            <div id="outer-child" style="width:25cqw;height:10cqh"></div>
            <div style="container-type:inline-size;width:200px">
              <div id="inner-child" style="width:10cqw;height:10cqh"></div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let outer_child = find_by_id(&doc.root, "outer-child").unwrap();
    let inner_child = find_by_id(&doc.root, "inner-child").unwrap();
    assert!((outer_child.layout.content_rect.w - 100.0).abs() < 0.01);
    assert!((outer_child.layout.content_rect.h - 30.0).abs() < 0.01);
    assert!((inner_child.layout.content_rect.w - 20.0).abs() < 0.01);
    assert!((inner_child.layout.content_rect.h - 30.0).abs() < 0.01);
}

#[test]
fn flex_and_grid_containers_resolve_descendant_query_units() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:flex;container-type:size;width:400px;height:300px">
            <div id="flex-child" style="width:25cqw;height:10cqh"></div>
          </div>
          <div style="display:grid;container-type:size;width:400px;height:300px">
            <div id="grid-child" style="width:25cqw;height:10cqh"></div>
          </div>
        </body></html>"#,
        800.0,
    );
    for id in ["flex-child", "grid-child"] {
        let child = find_by_id(&doc.root, id).unwrap();
        assert!(
            (child.layout.content_rect.w - 100.0).abs() < 0.01,
            "{id} width: {}",
            child.layout.content_rect.w
        );
        assert!(
            (child.layout.content_rect.h - 30.0).abs() < 0.01,
            "{id} height: {}",
            child.layout.content_rect.h
        );
    }
}

#[test]
fn inline_and_table_containers_resolve_descendant_query_units() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:inline-block;container-type:size;width:400px;height:300px">
            <div id="inline-child" style="width:25cqw;height:10cqh"></div>
          </div>
          <table style="container-type:size;width:400px;height:300px;border-spacing:0">
            <tr><td><div id="table-child" style="width:25cqw;height:10cqh"></div></td></tr>
          </table>
        </body></html>"#,
        800.0,
    );
    for id in ["inline-child", "table-child"] {
        let child = find_by_id(&doc.root, id).unwrap();
        assert!(
            (child.layout.content_rect.w - 100.0).abs() < 0.01,
            "{id} width"
        );
        assert!(
            (child.layout.content_rect.h - 30.0).abs() < 0.01,
            "{id} height"
        );
    }
}

#[test]
fn replaced_and_form_controls_resolve_container_lengths() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;width:400px;height:300px">
            <img id="image" src="missing.png" style="width:25cqw;height:10cqh">
            <input id="control" style="width:25cqw;height:10cqh">
          </div>
        </body></html>"#,
        800.0,
    );
    let image = find_by_id(&doc.root, "image").unwrap();
    assert!((image.layout.content_rect.w - 100.0).abs() < 0.01);
    assert!((image.layout.content_rect.h - 30.0).abs() < 0.01);
    let control = find_by_id(&doc.root, "control").unwrap();
    assert!((control.layout.border_rect.w - 100.0).abs() < 0.01);
    assert!((control.layout.border_rect.h - 30.0).abs() < 0.01);
}

#[test]
fn subgrid_container_resolves_descendant_query_width() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="display:grid;width:400px;grid-template-columns:200px 200px">
            <div style="display:grid;grid-column:1 / -1;grid-template-columns:subgrid;container-type:inline-size">
              <div id="subgrid-child" style="width:25cqw;height:10px"></div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let child = find_by_id(&doc.root, "subgrid-child").unwrap();
    assert!(
        (child.layout.content_rect.w - 100.0).abs() < 0.01,
        "subgrid width: {}",
        child.layout.content_rect.w
    );
}

#[test]
fn grid_tracks_use_ancestor_container_query_width() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;width:400px;height:300px">
            <div style="display:grid;width:300px;grid-template-columns:25cqw 1fr">
              <div id="track-child" style="height:10px"></div>
              <div style="height:10px"></div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let child = find_by_id(&doc.root, "track-child").unwrap();
    assert!(
        (child.layout.border_rect.w - 100.0).abs() < 0.01,
        "track width: {}",
        child.layout.border_rect.w
    );
}

#[test]
fn grid_auto_repeat_counts_container_query_tracks() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;width:400px;height:300px">
            <div style="display:grid;width:300px;grid-template-columns:repeat(auto-fill,25cqw)">
              <div style="height:10px"></div><div style="height:10px"></div>
              <div id="third" style="height:10px"></div>
              <div id="fourth" style="height:10px"></div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let third = find_by_id(&doc.root, "third").unwrap();
    let fourth = find_by_id(&doc.root, "fourth").unwrap();
    assert!((third.layout.border_rect.w - 100.0).abs() < 0.01);
    assert!((third.layout.border_rect.x - 200.0).abs() < 0.01);
    assert!(fourth.layout.border_rect.y >= third.layout.border_rect.bottom());
    assert!((fourth.layout.border_rect.x - 0.0).abs() < 0.01);
}

#[test]
fn grid_rows_use_ancestor_container_query_height() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;width:400px;height:300px">
            <div style="display:grid;container-type:size;width:200px;height:100px;grid-template-rows:10cqh 1fr">
              <div id="row-child"></div><div></div>
            </div>
          </div>
        </body></html>"#,
        800.0,
    );
    let child = find_by_id(&doc.root, "row-child").unwrap();
    assert!(
        (child.layout.border_rect.h - 30.0).abs() < 0.01,
        "row height: {}",
        child.layout.border_rect.h
    );
}

#[test]
fn vertical_writing_mode_uses_logical_viewport_fallback() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="vertical" style="writing-mode:vertical-rl;width:10cqb;height:10cqi"></div></body></html>"#,
        800.0,
    );
    let box_node = find_by_id(&doc.root, "vertical").unwrap();
    assert!((box_node.layout.content_rect.w - 80.0).abs() < 0.01);
    assert!((box_node.layout.content_rect.h - 70.0).abs() < 0.01);
}

#[test]
fn size_containment_ignores_child_height_for_auto_size() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="plain" style="width:100px"><div style="height:40px"></div></div>
          <div id="contained" style="contain:size;width:100px"><div style="height:40px"></div></div>
          <div id="query" style="container-type:size;width:100px"><div style="height:40px"></div></div>
          <div id="intrinsic" style="contain:size;contain-intrinsic-size:20px 25px;width:100px"><div style="height:40px"></div></div>
        </body></html>"#,
        800.0,
    );
    let height = |id| find_by_id(&doc.root, id).unwrap().layout.content_rect.h;
    assert!((height("plain") - 40.0).abs() < 0.01);
    assert!((height("contained") - 0.0).abs() < 0.01);
    assert!((height("query") - 0.0).abs() < 0.01);
    assert!((height("intrinsic") - 25.0).abs() < 0.01);
}

#[test]
fn inline_size_containment_ignores_child_intrinsic_width() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="plain" style="display:inline-block"><div style="width:200px">wide</div></div>
          <div id="size" style="display:inline-block;contain:size"><div style="width:200px">wide</div></div>
          <div id="inline" style="display:inline-block;contain:inline-size;contain-intrinsic-size:40px 20px"><div style="width:200px">wide</div></div>
          <div id="query" style="display:inline-block;container-type:inline-size;contain-intrinsic-size:60px 20px"><div style="width:200px">wide</div></div>
          <div id="size-query" style="display:inline-block;container-type:size;contain-intrinsic-size:50px 20px"><div style="width:200px">wide</div></div>
          <div id="table" style="display:table;contain:size;contain-intrinsic-size:70px 20px"><div style="width:200px">wide</div></div>
          <div id="explicit" style="display:inline-block;contain:size;width:90px"><div style="width:200px">wide</div></div>
        </body></html>"#,
        800.0,
    );
    let width = |id| find_by_id(&doc.root, id).unwrap().layout.content_rect.w;
    assert!((width("plain") - 200.0).abs() < 0.01);
    assert!(
        (width("size") - 0.0).abs() < 0.01,
        "size: {}",
        width("size")
    );
    assert!(
        (width("inline") - 40.0).abs() < 0.01,
        "inline: {}",
        width("inline")
    );
    assert!(
        (width("query") - 60.0).abs() < 0.01,
        "query: {}",
        width("query")
    );
    assert!(
        (width("size-query") - 50.0).abs() < 0.01,
        "size-query: {}",
        width("size-query")
    );
    assert!(
        (width("table") - 70.0).abs() < 0.01,
        "table: {}",
        width("table")
    );
    assert!(
        (width("explicit") - 90.0).abs() < 0.01,
        "explicit: {}",
        width("explicit")
    );
}

#[test]
fn hidden_content_visibility_skips_descendant_layout_but_keeps_own_size() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="explicit" style="content-visibility:hidden;width:100px;height:40px;padding:5px;background:red">
            <div id="unlaid" style="height:300px"></div>
          </div>
          <div id="intrinsic" style="content-visibility:hidden;contain-intrinsic-size:70px 25px">
            <div id="also-unlaid" style="height:300px"></div>
          </div>
        </body></html>"#,
        800.0,
    );
    let explicit = find_by_id(&doc.root, "explicit").unwrap();
    let intrinsic = find_by_id(&doc.root, "intrinsic").unwrap();
    assert_eq!(explicit.layout.content_rect.h, 40.0);
    assert_eq!(explicit.layout.border_rect.h, 50.0);
    assert_eq!(intrinsic.layout.content_rect.h, 25.0);
    assert_eq!(
        find_by_id(&doc.root, "unlaid")
            .unwrap()
            .layout
            .border_rect
            .h,
        0.0
    );
    assert_eq!(
        find_by_id(&doc.root, "also-unlaid")
            .unwrap()
            .layout
            .border_rect
            .h,
        0.0
    );
}

#[test]
fn auto_contain_intrinsic_size_is_one_value_for_both_axes() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="box" style="display:inline-block;content-visibility:hidden;contain-intrinsic-size:auto 40px"><div style="width:200px;height:200px"></div></div></body></html>"#,
        800.0,
    );
    let box_node = find_by_id(&doc.root, "box").unwrap();
    assert!(box_node.style.rare().contain_intrinsic_width_auto);
    assert!(box_node.style.rare().contain_intrinsic_height_auto);
    assert_eq!(box_node.layout.content_rect.w, 40.0);
    assert_eq!(box_node.layout.content_rect.h, 40.0);
    assert_eq!(
        doc.computed_style_property(box_node.node_id, "contain-intrinsic-size"),
        "auto 40px"
    );
}

#[test]
fn contain_intrinsic_size_two_auto_axis_values() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="box" style="display:inline-block;content-visibility:hidden;contain-intrinsic-size:auto 30px auto 45px"></div></body></html>"#,
        800.0,
    );
    let node = find_by_id(&doc.root, "box").unwrap();
    assert_eq!(node.layout.content_rect.w, 30.0);
    assert_eq!(node.layout.content_rect.h, 45.0);
    assert_eq!(
        doc.computed_style_property(node.node_id, "contain-intrinsic-size"),
        "auto 30px auto 45px"
    );
}

#[test]
fn auto_contain_intrinsic_size_remembers_visible_content_size() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="box" style="width:100px;contain-intrinsic-size:auto 25px"><div style="height:80px"></div></div></body></html>"#,
        800.0,
    );
    let id = find_by_id(&doc.root, "box").unwrap().node_id;
    let child_id = doc.find_webcore(id).unwrap().children[0].node_id;
    assert_eq!(doc.find_webcore(id).unwrap().layout.content_rect.h, 80.0);
    assert_eq!(crate::hit_test_box_at(&doc.root, (10.0, 10.0), 0), child_id);
    let node = doc.find_webcore_mut(id).unwrap();
    std::sync::Arc::make_mut(&mut node.style).content_visibility = ContentVisibility::Hidden;
    node.layout.layout_dirty = true;

    let mut engine = crate::layout::LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout_no_cascade(&mut doc, 800.0);
    let node = doc.find_webcore(id).unwrap();
    assert_eq!(node.layout.content_rect.h, 80.0);
    assert_eq!(crate::hit_test_box_at(&doc.root, (10.0, 10.0), 0), id);
}

#[test]
fn auto_contain_intrinsic_size_survives_size_containment() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="box" style="width:100px;contain-intrinsic-size:auto 25px"><div style="height:80px"></div></div></body></html>"#,
        800.0,
    );
    let id = find_by_id(&doc.root, "box").unwrap().node_id;
    let node = doc.find_webcore_mut(id).unwrap();
    std::sync::Arc::make_mut(&mut node.style).contain_size = true;
    node.layout.layout_dirty = true;

    let mut engine = crate::layout::LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout_no_cascade(&mut doc, 800.0);
    assert_eq!(doc.find_webcore(id).unwrap().layout.content_rect.h, 80.0);
}

#[test]
fn remembered_contained_height_drives_container_query_units() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="box" style="position:relative;container-type:normal;width:100px;contain-intrinsic-size:auto 25px"><div style="height:80px"></div><div id="query" style="position:absolute;height:10cqh;width:10px"></div></div></body></html>"#,
        800.0,
    );
    let id = find_by_id(&doc.root, "box").unwrap().node_id;
    assert_eq!(doc.find_webcore(id).unwrap().layout.content_rect.h, 80.0);
    let node = doc.find_webcore_mut(id).unwrap();
    std::sync::Arc::make_mut(&mut node.style).container_type = ContainerType::Size;
    crate::css::cascade::mark_layout_subtree_dirty(node);

    let mut engine = crate::layout::LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout_no_cascade(&mut doc, 800.0);
    assert_eq!(doc.find_webcore(id).unwrap().layout.content_rect.h, 80.0);
    assert_eq!(find_by_id(&doc.root, "query").unwrap().layout.content_rect.h, 8.0);
}

#[test]
fn hidden_content_visibility_blocks_hits_on_previously_laid_out_children() {
    let mut doc = parse_and_layout(
        r#"<html><body style="margin:0"><div id="outer" style="width:100px;height:50px"><div id="inner" style="width:80px;height:40px"></div></div></body></html>"#,
        800.0,
    );
    let outer_id = find_by_id(&doc.root, "outer").unwrap().node_id;
    let inner_id = find_by_id(&doc.root, "inner").unwrap().node_id;
    assert_eq!(crate::hit_test_box_at(&doc.root, (10.0, 10.0), 0), inner_id);
    std::sync::Arc::make_mut(&mut doc.find_webcore_mut(outer_id).unwrap().style)
        .content_visibility = ContentVisibility::Hidden;
    assert_eq!(crate::hit_test_box_at(&doc.root, (10.0, 10.0), 0), outer_id);
}

#[test]
fn contained_intrinsic_height_is_the_query_container_height() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div style="container-type:size;contain-intrinsic-size:20px 25px;width:100px">
            <div id="query-child" style="height:10cqh"></div>
          </div>
        </body></html>"#,
        800.0,
    );
    let child = find_by_id(&doc.root, "query-child").unwrap();
    assert!(
        (child.layout.content_rect.h - 2.5).abs() < 0.01,
        "query height: {}",
        child.layout.content_rect.h
    );
}

#[test]
fn flex_and_grid_size_containment_ignore_child_height() {
    let doc = parse_and_layout(
        r#"<html><body style="margin:0">
          <div id="flex" style="display:flex;contain:size;width:100px"><div style="height:40px"></div></div>
          <div id="grid" style="display:grid;container-type:size;width:100px"><div style="height:40px"></div></div>
          <div id="intrinsic-grid" style="display:grid;container-type:size;contain-intrinsic-size:20px 25px;width:100px"><div style="height:40px"></div></div>
        </body></html>"#,
        800.0,
    );
    let height = |id| find_by_id(&doc.root, id).unwrap().layout.content_rect.h;
    assert!((height("flex") - 0.0).abs() < 0.01);
    assert!((height("grid") - 0.0).abs() < 0.01);
    assert!((height("intrinsic-grid") - 25.0).abs() < 0.01);
}
