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

// ── Layout effect of @container rules ─────────────────────────────────────────

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
