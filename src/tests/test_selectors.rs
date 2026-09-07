// Ported from tests/test_selectors.cpp

use crate::css::{parse_selector, AncestorInfo, AttrOp, Combinator, SelectorPart};
use crate::types::*;

// ── Basic Selector Matching ───────────────────────────────────────────────────

#[test]
fn selectors_tag_match() {
    let mut b = WebCore::new("p");
    assert!(parse_selector("p").matches_box(&b));
    assert!(!parse_selector("div").matches_box(&b));
}

#[test]
fn selectors_class_match() {
    let mut b = WebCore::new("div");
    b.attributes.insert("class", "foo bar");
    assert!(parse_selector(".foo").matches_box(&b));
    assert!(parse_selector(".bar").matches_box(&b));
    assert!(!parse_selector(".baz").matches_box(&b));
}

#[test]
fn selectors_id_match() {
    let mut b = WebCore::new("div");
    b.attributes.insert("id", "main");
    assert!(parse_selector("#main").matches_box(&b));
    assert!(!parse_selector("#other").matches_box(&b));
}

#[test]
fn selectors_tag_and_class_combined() {
    let mut b = WebCore::new("p");
    b.attributes.insert("class", "intro");
    assert!(parse_selector("p.intro").matches_box(&b));
    assert!(!parse_selector("div.intro").matches_box(&b));
}

#[test]
fn selectors_tag_and_id_combined() {
    let mut b = WebCore::new("div");
    b.attributes.insert("id", "header");
    assert!(parse_selector("div#header").matches_box(&b));
    assert!(!parse_selector("p#header").matches_box(&b));
}

#[test]
fn selectors_universal_selector() {
    let b = WebCore::new("span");
    assert!(parse_selector("*").matches_box(&b));
}

#[test]
fn selectors_multiple_class_selector() {
    let mut b = WebCore::new("div");
    b.attributes.insert("class", "foo bar baz");
    assert!(parse_selector(".foo.bar").matches_box(&b));
    assert!(parse_selector(".foo.baz").matches_box(&b));
    assert!(!parse_selector(".foo.missing").matches_box(&b));
}

// ── Attribute Selectors ───────────────────────────────────────────────────────

#[test]
fn selectors_attr_exists() {
    let sel = parse_selector("[href]");
    assert!(sel.parts.iter().any(|p| matches!(
        p,
        SelectorPart::Attribute {
            op: AttrOp::Exists,
            ..
        }
    )));
}

#[test]
fn selectors_attr_equals() {
    let sel = parse_selector("[dir=\"rtl\"]");
    assert!(sel.parts.iter().any(|p| matches!(p,
        SelectorPart::Attribute { op: AttrOp::Eq, value, .. } if value == "rtl"
    )));
}

#[test]
fn selectors_attr_prefix() {
    let sel = parse_selector("[class^=\"btn\"]");
    assert!(sel.parts.iter().any(|p| matches!(
        p,
        SelectorPart::Attribute {
            op: AttrOp::StartsWith,
            ..
        }
    )));
}

#[test]
fn selectors_attr_suffix() {
    let sel = parse_selector("[src$=\".png\"]");
    assert!(sel.parts.iter().any(|p| matches!(
        p,
        SelectorPart::Attribute {
            op: AttrOp::EndsWith,
            ..
        }
    )));
}

#[test]
fn selectors_attr_substring() {
    let sel = parse_selector("[class*=\"mid\"]");
    assert!(sel.parts.iter().any(|p| matches!(
        p,
        SelectorPart::Attribute {
            op: AttrOp::Contains,
            ..
        }
    )));
}

#[test]
fn selectors_attr_with_tag() {
    let sel = parse_selector("a[href]");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Tag(t) if t == "a")));
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Attribute { .. })));
}

// ── Structural Pseudo-Classes (parsing) ───────────────────────────────────────

#[test]
fn selectors_first_child_parsing() {
    let sel = parse_selector("p:first-child");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "first-child")));
}

#[test]
fn selectors_last_child_parsing() {
    let sel = parse_selector("p:last-child");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "last-child")));
}

#[test]
fn selectors_nth_child_parsing() {
    let sel = parse_selector("li:nth-child(2n+1)");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n.starts_with("nth-child"))));
}

#[test]
fn selectors_only_child_parsing() {
    let sel = parse_selector("p:only-child");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "only-child")));
}

#[test]
fn selectors_empty_parsing() {
    let sel = parse_selector("div:empty");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "empty")));
}

// ── Combinator Parsing ────────────────────────────────────────────────────────

#[test]
fn selectors_descendant_combinator() {
    let sel = parse_selector("div p");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Descendant))));
}

#[test]
fn selectors_child_combinator() {
    let sel = parse_selector("div > p");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Child))));
}

#[test]
fn selectors_adjacent_sibling() {
    let sel = parse_selector("h1 + p");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::AdjacentSibling))));
}

#[test]
fn selectors_general_sibling() {
    let sel = parse_selector("h1 ~ p");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::GeneralSibling))));
}

// ── Child combinator with surrounding whitespace ──────────────────────────────

#[test]
fn selectors_child_combinator_no_spurious_descendant() {
    // "div > p" must parse to exactly [Tag("div"), Child, Tag("p")] — no extra Descendant.
    let sel = parse_selector("div > p");
    let combinators: Vec<_> = sel
        .parts
        .iter()
        .filter(|p| matches!(p, SelectorPart::Combinator(_)))
        .collect();
    assert_eq!(combinators.len(), 1, "expected exactly 1 combinator");
    assert!(matches!(
        combinators[0],
        SelectorPart::Combinator(Combinator::Child)
    ));
}

#[test]
fn selectors_child_universal_no_spurious_descendant() {
    // ".grid-3 > *" must have exactly one combinator (Child), not Child + Descendant.
    let sel = parse_selector(".grid-3 > *");
    let combinators: Vec<_> = sel
        .parts
        .iter()
        .filter(|p| matches!(p, SelectorPart::Combinator(_)))
        .collect();
    assert_eq!(
        combinators.len(),
        1,
        "expected exactly 1 combinator, got extra from trailing space after '>'"
    );
    assert!(matches!(
        combinators[0],
        SelectorPart::Combinator(Combinator::Child)
    ));
    // Should end with Universal
    assert!(matches!(sel.parts.last(), Some(SelectorPart::Universal)));
}

#[test]
fn selectors_child_universal_matches_direct_child() {
    // ".grid-3 > *" must match a direct child of .grid-3, not a grandchild.
    use crate::css::AncestorInfo;
    let sel = parse_selector(".grid-3 > *");
    let child = WebCore::new("div");

    // Direct child: parent is .grid-3
    let direct_ancestors = vec![AncestorInfo {
        tag: "div".into(),
        attributes: [("class".to_string(), "grid-3".to_string())].into(),
        child_index: 0,
        sibling_count: 3,
        ..Default::default()
    }];
    assert!(
        sel.matches_with_ancestors(&child, 0, 3, &direct_ancestors),
        "should match a direct child of .grid-3"
    );

    // Grandchild: parent is a plain div inside .grid-3
    let grandchild_ancestors = vec![
        AncestorInfo {
            tag: "div".into(),
            attributes: [("class".to_string(), "grid-3".to_string())].into(),
            child_index: 0,
            sibling_count: 1,
            ..Default::default()
        },
        AncestorInfo {
            tag: "div".into(),
            attributes: Default::default(),
            child_index: 0,
            sibling_count: 1,
            ..Default::default()
        },
    ];
    assert!(
        !sel.matches_with_ancestors(&child, 0, 1, &grandchild_ancestors),
        "should NOT match a grandchild of .grid-3"
    );
}

// ── nth-child keyword shorthands ──────────────────────────────────────────────

#[test]
fn selectors_nth_child_odd() {
    // :nth-child(odd) == 2n+1
    let sel = parse_selector("li:nth-child(odd)");
    // Parsed: child_index 0 (pos=1, odd) should match; child_index 1 (pos=2, even) should not
    let mut b_first = WebCore::new("li");
    let ancestors = vec![AncestorInfo {
        tag: "ul".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 4,
        ..Default::default()
    }];
    assert!(sel.matches_with_ancestors(&b_first, 0, 4, &ancestors));
    assert!(!sel.matches_with_ancestors(&b_first, 1, 4, &ancestors));
}

#[test]
fn selectors_nth_child_even() {
    // :nth-child(even) == 2n
    let sel = parse_selector("li:nth-child(even)");
    let ancestors = vec![AncestorInfo {
        tag: "ul".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 4,
        ..Default::default()
    }];
    let b = WebCore::new("li");
    assert!(!sel.matches_with_ancestors(&b, 0, 4, &ancestors)); // pos=1, odd
    assert!(sel.matches_with_ancestors(&b, 1, 4, &ancestors)); // pos=2, even
}

#[test]
fn selectors_nth_child_simple_number() {
    // :nth-child(3) matches only the 3rd child (child_index=2)
    let sel = parse_selector("li:nth-child(3)");
    let ancestors = vec![AncestorInfo {
        tag: "ul".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 4,
        ..Default::default()
    }];
    let b = WebCore::new("li");
    assert!(!sel.matches_with_ancestors(&b, 0, 4, &ancestors));
    assert!(!sel.matches_with_ancestors(&b, 1, 4, &ancestors));
    assert!(sel.matches_with_ancestors(&b, 2, 4, &ancestors)); // pos=3
    assert!(!sel.matches_with_ancestors(&b, 3, 4, &ancestors));
}

#[test]
fn selectors_first_child_match() {
    // li:first-child should match first child only
    let sel = parse_selector("li:first-child");
    let ancestors = vec![AncestorInfo {
        tag: "ul".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 2,
        ..Default::default()
    }];
    let b = WebCore::new("li");
    assert!(sel.matches_with_ancestors(&b, 0, 2, &ancestors)); // first child
    assert!(!sel.matches_with_ancestors(&b, 1, 2, &ancestors)); // second child
}

#[test]
fn selectors_last_child_match() {
    // li:last-child should match last child only
    let sel = parse_selector("li:last-child");
    let ancestors = vec![AncestorInfo {
        tag: "ul".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 2,
        ..Default::default()
    }];
    let b = WebCore::new("li");
    assert!(!sel.matches_with_ancestors(&b, 0, 2, &ancestors)); // first child
    assert!(sel.matches_with_ancestors(&b, 1, 2, &ancestors)); // last child
}

#[test]
fn selectors_only_child_match() {
    // p:only-child should match when sibling_count == 1
    let sel = parse_selector("p:only-child");
    let ancestors_single = vec![AncestorInfo {
        tag: "div".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 1,
        ..Default::default()
    }];
    let ancestors_two = vec![AncestorInfo {
        tag: "div".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 2,
        ..Default::default()
    }];
    let b = WebCore::new("p");
    assert!(sel.matches_with_ancestors(&b, 0, 1, &ancestors_single)); // only child
    assert!(!sel.matches_with_ancestors(&b, 0, 2, &ancestors_two)); // has sibling
}

#[test]
fn selectors_descendant_match() {
    // "div p" should match a p whose ancestor is a div
    let sel = parse_selector("div p");
    let b = WebCore::new("p");
    let ancestors = vec![AncestorInfo {
        tag: "div".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 1,
        ..Default::default()
    }];
    assert!(sel.matches_with_ancestors(&b, 0, 1, &ancestors));
    // "span p" should not match when ancestor is div
    let sel2 = parse_selector("span p");
    assert!(!sel2.matches_with_ancestors(&b, 0, 1, &ancestors));
}

#[test]
fn selectors_child_match() {
    // "div>p" (no spaces) should match p that is a direct child of div.
    // Note: the parser inserts an extra Descendant combinator for whitespace
    // around ">", so we use the no-space form to get a clean Child selector.
    let sel = parse_selector("div>p");
    let b = WebCore::new("p");
    let ancestors = vec![AncestorInfo {
        tag: "div".into(),
        attributes: Default::default(),
        child_index: 0,
        sibling_count: 1,
        ..Default::default()
    }];
    assert!(sel.matches_with_ancestors(&b, 0, 1, &ancestors));
}

#[test]
fn selectors_deep_descendant_match() {
    // "div p" should match p nested inside section inside div.
    // "div>p" (direct child) should NOT match (p's parent is section).
    // "section>p" should match (section is direct parent).
    let sel_desc = parse_selector("div p");
    let sel_child = parse_selector("div>p");
    let sel_sec_child = parse_selector("section>p");
    let b = WebCore::new("p");
    // ancestors listed outermost-first: div → section → p
    let ancestors = vec![
        AncestorInfo {
            tag: "div".into(),
            attributes: Default::default(),
            child_index: 0,
            sibling_count: 1,
            ..Default::default()
        },
        AncestorInfo {
            tag: "section".into(),
            attributes: Default::default(),
            child_index: 0,
            sibling_count: 1,
            ..Default::default()
        },
    ];
    // "div p" — descendant match (div is ancestor)
    assert!(sel_desc.matches_with_ancestors(&b, 0, 1, &ancestors));
    // "div>p" — direct child: p's parent is section, not div → should NOT match
    assert!(!sel_child.matches_with_ancestors(&b, 0, 1, &ancestors));
    // "section>p" — direct child of section → should match
    assert!(sel_sec_child.matches_with_ancestors(&b, 0, 1, &ancestors));
}

// ── Specificity ───────────────────────────────────────────────────────────────

#[test]
fn selectors_specificity() {
    let sel1 = parse_selector("#main");
    let sel2 = parse_selector(".container");
    let sel3 = parse_selector("div");
    assert!(sel1.specificity() > sel2.specificity());
    assert!(sel2.specificity() > sel3.specificity());
}

// ── :nth-child ignores #text siblings ────────────────────────────────────────

/// Regression: the HTML parser creates #text nodes for whitespace between
/// elements.  If these are counted for :nth-child, the indices are wrong and
/// selectors like .dot:nth-child(1/2/3) match the wrong (or no) elements.
/// The cascade must count only element siblings, not text nodes.
#[test]
fn nth_child_ignores_text_node_siblings() {
    use super::harness::parse_and_layout;

    // Three divs separated by whitespace — the parser creates #text nodes
    // between them.  The :nth-child selectors must still use element-only index.
    let doc = parse_and_layout(
        r#"
        <style>
        .box:nth-child(1) { background: red; }
        .box:nth-child(2) { background: green; }
        .box:nth-child(3) { background: blue; }
        </style>
        <div id="wrap">
          <div class="box" id="b1"></div>
          <div class="box" id="b2"></div>
          <div class="box" id="b3"></div>
        </div>
    "#,
        400.0,
    );

    let b1 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "b1").unwrap_or(false)
    })
    .expect("b1 not found");
    let b2 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "b2").unwrap_or(false)
    })
    .expect("b2 not found");
    let b3 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "b3").unwrap_or(false)
    })
    .expect("b3 not found");

    // Red = (255,0,0), Green = (0,128,0), Blue = (0,0,255)
    let red = crate::css::parse_color("red").unwrap();
    let green = crate::css::parse_color("green").unwrap();
    let blue = crate::css::parse_color("blue").unwrap();

    assert_eq!(
        b1.style.background_color.r, red.r,
        "b1 (nth-child 1) should have red background, got {:?}",
        b1.style.background_color
    );
    assert_eq!(
        b2.style.background_color.g, green.g,
        "b2 (nth-child 2) should have green background, got {:?}",
        b2.style.background_color
    );
    assert_eq!(
        b3.style.background_color.b, blue.b,
        "b3 (nth-child 3) should have blue background, got {:?}",
        b3.style.background_color
    );
}

/// Regression: loading dots animation — each .dot:nth-child(n) rule sets a
/// different animation-delay.  With text-node counting off, all three nth-child
/// rules miss all three dots (they fall on whitespace positions) so only the
/// element that accidentally matches gets an animation.
#[test]
fn nth_child_animation_delay_per_dot() {
    use super::harness::parse_and_layout;

    let doc = parse_and_layout(
        r#"
        <style>
        @keyframes pulse { from { opacity: 0; } to { opacity: 1; } }
        .dot:nth-child(1) { animation: pulse 1s linear 0s    infinite; }
        .dot:nth-child(2) { animation: pulse 1s linear 0.2s  infinite; }
        .dot:nth-child(3) { animation: pulse 1s linear 0.4s  infinite; }
        </style>
        <div class="dots">
          <div class="dot" id="d1"></div>
          <div class="dot" id="d2"></div>
          <div class="dot" id="d3"></div>
        </div>
    "#,
        400.0,
    );

    let d1 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "d1").unwrap_or(false)
    })
    .expect("d1 not found");
    let d2 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "d2").unwrap_or(false)
    })
    .expect("d2 not found");
    let d3 = super::harness::find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|s| s == "d3").unwrap_or(false)
    })
    .expect("d3 not found");

    assert_eq!(
        d1.style.rare().animations.len(),
        1,
        "d1 should have 1 animation"
    );
    assert_eq!(
        d2.style.rare().animations.len(),
        1,
        "d2 should have 1 animation"
    );
    assert_eq!(
        d3.style.rare().animations.len(),
        1,
        "d3 should have 1 animation"
    );

    assert!(
        (d1.style.rare().animations[0].delay_ms - 0.0).abs() < 1.0,
        "d1 delay should be 0ms, got {}",
        d1.style.rare().animations[0].delay_ms
    );
    assert!(
        (d2.style.rare().animations[0].delay_ms - 200.0).abs() < 1.0,
        "d2 delay should be 200ms, got {}",
        d2.style.rare().animations[0].delay_ms
    );
    assert!(
        (d3.style.rare().animations[0].delay_ms - 400.0).abs() < 1.0,
        "d3 delay should be 400ms, got {}",
        d3.style.rare().animations[0].delay_ms
    );
}

// ── Soundness guard for the parallel matching pass ───────────────────────────

/// **The `unsafe impl Sync for MatchNode` in `css/cascade_parallel.rs` rests on
/// one invariant: selector matching reads DOM and element state and NEVER
/// touches `layout`.** `WebCore` is not `Sync` solely because `LayoutBox` holds
/// a `Cell<f32>` intrinsic-width memo, so a matcher that reached into `layout`
/// would turn the parallel pass into a data race.
///
/// The invariant was documented in a comment, which nothing enforces. This
/// checks it: if you need `layout` while matching, the parallel pass is no
/// longer sound and that `unsafe impl` has to go — do not silence this test.
#[test]
fn selector_matching_never_reads_layout() {
    /// First line of code (comments stripped) that touches `layout`, if any.
    fn offending(src: &str) -> Option<(usize, String)> {
        src.lines().enumerate().find_map(|(i, line)| {
            let code = line.split("//").next().unwrap_or("");
            code.contains(".layout")
                .then(|| (i + 1, line.trim().to_string()))
        })
    }
    // The check must be able to fail: a matcher that reads `layout` is caught,
    // and prose about layout in a comment is not.
    assert!(
        offending("let w = node.layout.margin_rect.w;").is_some(),
        "the guard cannot detect a real `.layout` read"
    );
    assert!(
        offending("// nothing here touches .layout at all").is_none(),
        "the guard trips on a comment"
    );

    for (name, src) in [
        ("css/matching.rs", include_str!("../css/matching.rs")),
        ("css/selector.rs", include_str!("../css/selector.rs")),
    ] {
        if let Some((line_no, text)) = offending(src) {
            panic!(
                "{name}:{line_no} reads `.layout` during selector matching, which \
                    voids the `unsafe impl Sync for MatchNode` in \
                    css/cascade_parallel.rs:\n  {text}"
            );
        }
    }
}

// ── Functional pseudo-class argument lists (selectors-4) ────────────────────

/// **`:is()`/`:not()`/`:where()` argument lists split on TOP-LEVEL commas only.**
/// A naive `split(',')` tears a nested list apart at the inner commas, so
/// `:where(:not(iframe, canvas, img, svg, video))` — the modern-reset idiom —
/// becomes a far weaker selector than written.
#[test]
fn not_with_a_comma_list_excludes_every_branch() {
    let doc = crate::html::parse_html(
        "<style>div :is(:not(a, b), c) { color: rgb(1,2,3) }</style>\
         <div><a id=x>a</a><b id=y>b</b><i id=z>i</i></div>",
    );
    let mut d = doc;
    let mut eng = crate::layout::LayoutEngine::new();
    eng.layout(&mut d, 400.0);
    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let hit = |id: &str| {
        let c = by_id(&d.root, id).unwrap().style.color;
        (c.r, c.g, c.b) == (1, 2, 3)
    };
    // `:not(a, b)` must exclude BOTH <a> and <b>; <i> matches it.
    assert!(!hit("x"), "<a> must not match :not(a, b)");
    assert!(
        !hit("y"),
        "<b> must not match :not(a, b) — the inner list was split wrong"
    );
    assert!(hit("z"), "<i> matches :not(a, b)");
}

/// `:has()` takes a selector LIST — `:has(h1, h2)` is "has an h1 OR an h2",
/// not "has an h1 containing an h2".
#[test]
fn has_with_a_comma_list_is_an_or() {
    let mut doc = crate::html::parse_html(
        "<style>section:has(h1, h2) { color: rgb(4,5,6) }</style>\
         <section id=a><h2>x</h2></section><section id=b><p>y</p></section>",
    );
    let mut eng = crate::layout::LayoutEngine::new();
    eng.layout(&mut doc, 400.0);
    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let hit = |id: &str| {
        let c = by_id(&doc.root, id).unwrap().style.color;
        (c.r, c.g, c.b) == (4, 5, 6)
    };
    assert!(hit("a"), "a section containing an h2 matches :has(h1, h2)");
    assert!(!hit("b"), "a section with neither must not match");
}

/// **`:lang()` matches on the element's language**, which comes from the
/// nearest `lang` attribute on itself or an ancestor (selectors-4 §8.2). It
/// parsed as a valid pseudo-class and then always answered `false`.
#[test]
fn lang_matches_the_inherited_language() {
    let doc = crate::tests::harness::parse_and_layout(
        "<style>:lang(fr) { color: rgb(1,2,3) }</style>\
         <div lang=fr><p id=inside>bonjour</p></div><p id=outside>hello</p>",
        400.0,
    );
    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let col = |id: &str| {
        let c = by_id(&doc.root, id).unwrap().style.color;
        (c.r, c.g, c.b)
    };
    assert_eq!(
        col("inside"),
        (1, 2, 3),
        "an element inherits lang from its ancestor"
    );
    assert_ne!(
        col("outside"),
        (1, 2, 3),
        "an element outside the lang subtree must not match"
    );
}

/// `:dir(rtl)` matches on the element's directionality.
#[test]
fn dir_matches_the_directionality() {
    let doc = crate::tests::harness::parse_and_layout(
        "<style>:dir(rtl) { color: rgb(4,5,6) }</style>\
         <div dir=rtl><p id=r>مرحبا</p></div><p id=l>hello</p>",
        400.0,
    );
    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let col = |id: &str| {
        let c = by_id(&doc.root, id).unwrap().style.color;
        (c.r, c.g, c.b)
    };
    assert_eq!(
        col("r"),
        (4, 5, 6),
        "an element in an rtl subtree matches :dir(rtl)"
    );
    assert_ne!(
        col("l"),
        (4, 5, 6),
        "an ltr element must not match :dir(rtl)"
    );
}

#[test]
fn open_and_closed_match_details_state() {
    let doc = crate::tests::harness::parse_and_layout(
        "<style>details:open { color: rgb(1,2,3) } details:closed { background-color: rgb(4,5,6) }</style>\
         <details id=o open><summary>open</summary></details>\
         <details id=c><summary>closed</summary></details>",
        400.0,
    );
    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let open = by_id(&doc.root, "o").unwrap();
    let closed = by_id(&doc.root, "c").unwrap();
    assert_eq!(
        (open.style.color.r, open.style.color.g, open.style.color.b),
        (1, 2, 3)
    );
    assert_ne!(
        (
            closed.style.color.r,
            closed.style.color.g,
            closed.style.color.b
        ),
        (1, 2, 3)
    );
    assert_eq!(
        (
            closed.style.background_color.r,
            closed.style.background_color.g,
            closed.style.background_color.b
        ),
        (4, 5, 6)
    );
}

#[test]
fn target_and_target_within_match_document_fragment() {
    let mut doc = crate::html::parse_html_with_base(
        "<style>\
             :target { color: rgb(1,2,3) }\
             section:target-within { background-color: rgb(4,5,6) }\
         </style>\
         <section id=outer><p id=hit>target</p></section>\
         <section id=other><p>other</p></section>",
        "https://example.test/page#hit",
    );
    crate::layout::LayoutEngine::new().layout(&mut doc, 400.0);

    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }

    let target = by_id(&doc.root, "hit").unwrap();
    let outer = by_id(&doc.root, "outer").unwrap();
    let other = by_id(&doc.root, "other").unwrap();
    assert_eq!(
        (
            target.style.color.r,
            target.style.color.g,
            target.style.color.b
        ),
        (1, 2, 3)
    );
    assert_eq!(
        (
            outer.style.background_color.r,
            outer.style.background_color.g,
            outer.style.background_color.b
        ),
        (4, 5, 6)
    );
    assert_ne!(
        (
            other.style.background_color.r,
            other.style.background_color.g,
            other.style.background_color.b
        ),
        (4, 5, 6)
    );
}

#[test]
fn local_link_matches_same_document_urls() {
    let mut doc = crate::html::parse_html_with_base(
        "<style>\
             a:local-link { color: rgb(1,2,3) }\
         </style>\
         <a id=hash href=\"#section\">hash</a>\
         <a id=absolute href=\"https://example.test/dir/page.html?x=1#other\">absolute</a>\
         <a id=query href=\"https://example.test/dir/page.html?x=2#section\">query</a>\
         <a id=other href=\"https://example.test/dir/other.html#section\">other</a>",
        "https://example.test/dir/page.html?x=1#section",
    );
    crate::layout::LayoutEngine::new().layout(&mut doc, 400.0);

    fn color_of(doc: &crate::types::Document, id: &str) -> (u8, u8, u8) {
        fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = by_id(c, id) {
                    return Some(f);
                }
            }
            None
        }
        let style = &by_id(&doc.root, id).unwrap().style;
        (style.color.r, style.color.g, style.color.b)
    }

    assert_eq!(color_of(&doc, "hash"), (1, 2, 3));
    assert_eq!(color_of(&doc, "absolute"), (1, 2, 3));
    assert_ne!(color_of(&doc, "query"), (1, 2, 3));
    assert_ne!(color_of(&doc, "other"), (1, 2, 3));
}
