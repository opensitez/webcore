// Ported from cpptests/test_css.cpp
// CSS declaration parsing, stylesheet parsing, selector parsing, property application.

use webcore::css::{
    apply_property, parse_declarations, parse_selector, parse_stylesheet, Combinator,
    PseudoElement, SelectorPart,
};
use webcore::parse_html;
use webcore::types::*;

fn style_with(prop: &str, val: &str) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, prop, val);
    style
}

// ============================================================
// CSS Declaration Parsing
// ============================================================

#[test]
fn css_basic_declarations() {
    let decls = parse_declarations("color: red; font-size: 16px;");
    assert!(decls.len() >= 2);
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("red"));
    assert_eq!(decls.get("font-size").map(|s| s.as_str()), Some("16px"));
}

#[test]
fn css_empty_declarations() {
    let decls = parse_declarations("");
    assert_eq!(decls.len(), 0);
}

#[test]
fn css_trailing_semicolon() {
    let decls = parse_declarations("color: red;");
    assert!(decls.len() >= 1);
    assert!(decls.contains_key("color"));
}

#[test]
fn css_no_semicolon() {
    let decls = parse_declarations("color: red");
    assert!(decls.len() >= 1);
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("red"));
}

#[test]
fn css_multiple_values() {
    let decls = parse_declarations("margin: 10px 20px 30px 40px;");
    assert!(decls.len() >= 1);
    assert!(decls.contains_key("margin"));
}

// ============================================================
// Stylesheet Parsing
// ============================================================

#[test]
fn css_stylesheet_rules() {
    let rules =
        parse_stylesheet("p { color: blue; } .big { font-size: 24px; }").unwrap_or_default();
    assert!(rules.len() >= 2);
}

#[test]
fn css_stylesheet_multiple_selectors() {
    let rules = parse_stylesheet("h1, h2, h3 { font-weight: bold; }").unwrap_or_default();
    assert!(!rules.is_empty());
}

#[test]
fn css_variables_in_root() {
    let doc = parse_html(
        "<style>:root { --main-color: #ff0000; --gap: 10px; } p { color: var(--main-color); }</style>\
         <p>text</p>");
    assert!(
        doc.stylesheet.variables.contains_key("--main-color"),
        "--main-color variable should be stored"
    );
    assert_eq!(
        doc.stylesheet
            .variables
            .get("--main-color")
            .map(|s| s.as_str()),
        Some("#ff0000")
    );
    assert!(doc.stylesheet.variables.contains_key("--gap"));
}

#[test]
fn css_variable_with_fallback() {
    let rules = parse_stylesheet("p { color: var(--missing, red); }").unwrap_or_default();
    assert!(!rules.is_empty());
}

#[test]
fn css_hover_rule() {
    let rules = parse_stylesheet("a:hover { color: red; }").unwrap_or_default();
    assert!(
        rules.iter().any(|r| r.is_hover),
        "a:hover rule should set is_hover=true"
    );
}

#[test]
fn css_pseudo_element_before() {
    let rules = parse_stylesheet("p::before { content: \">\"; }").unwrap_or_default();
    assert!(
        rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::Before),
        "p::before rule should have pseudo_element == Before"
    );
}

#[test]
fn css_pseudo_element_after() {
    let rules = parse_stylesheet("p::after { content: \"<\"; }").unwrap_or_default();
    assert!(
        rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::After),
        "p::after rule should have pseudo_element == After"
    );
}

// ============================================================
// Selector Parsing
// ============================================================

#[test]
fn css_selector_with_class() {
    let sel = parse_selector("div.container");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Tag(t) if t == "div")));
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Class(c) if c == "container")));
}

#[test]
fn css_selector_with_id() {
    let sel = parse_selector("#main");
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Id(id) if id == "main")));
}

#[test]
fn css_selector_multiple_classes() {
    let sel = parse_selector(".foo.bar.baz");
    let class_count = sel
        .parts
        .iter()
        .filter(|p| matches!(p, SelectorPart::Class(_)))
        .count();
    assert!(
        class_count >= 3,
        "Expected at least 3 classes, got {}",
        class_count
    );
}

#[test]
fn css_descendant_combinator() {
    let sel = parse_selector("div p");
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Descendant))),
        "div p should have a Descendant combinator"
    );
}

#[test]
fn css_child_combinator() {
    let sel = parse_selector("div > p");
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Child))),
        "div > p should have a Child combinator"
    );
}

#[test]
fn css_adjacent_sibling_combinator() {
    let sel = parse_selector("h1 + p");
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::AdjacentSibling))),
        "h1 + p should have an AdjacentSibling combinator"
    );
}

#[test]
fn css_general_sibling_combinator() {
    let sel = parse_selector("h1 ~ p");
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::GeneralSibling))),
        "h1 ~ p should have a GeneralSibling combinator"
    );
}

#[test]
fn css_specificity_ordering() {
    let sel1 = parse_selector("#main");
    let sel2 = parse_selector(".container");
    let sel3 = parse_selector("div");
    assert!(
        sel1.specificity() > sel2.specificity(),
        "#main specificity ({}) should > .container ({})",
        sel1.specificity(),
        sel2.specificity()
    );
    assert!(
        sel2.specificity() > sel3.specificity(),
        ".container specificity ({}) should > div ({})",
        sel2.specificity(),
        sel3.specificity()
    );
}

// ============================================================
// CSS Property Application
// ============================================================

#[test]
fn css_display_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "display", "inline");
    assert_eq!(s.display, Display::Inline);
    apply_property(&mut s, "display", "inline-block");
    assert_eq!(s.display, Display::InlineBlock);
    apply_property(&mut s, "display", "flex");
    assert_eq!(s.display, Display::Flex);
    apply_property(&mut s, "display", "grid");
    assert_eq!(s.display, Display::Grid);
    apply_property(&mut s, "display", "none");
    assert_eq!(s.display, Display::None);
    apply_property(&mut s, "display", "list-item");
    assert_eq!(s.display, Display::ListItem);
}

#[test]
fn css_overflow_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "overflow", "hidden");
    assert_eq!(s.overflow_x, Overflow::Hidden);
    apply_property(&mut s, "overflow", "scroll");
    assert_eq!(s.overflow_x, Overflow::Scroll);
    apply_property(&mut s, "overflow", "auto");
    assert_eq!(s.overflow_x, Overflow::Auto);
    apply_property(&mut s, "overflow", "visible");
    assert_eq!(s.overflow_x, Overflow::Visible);
}

#[test]
fn css_opacity_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "opacity", "0.5");
    assert!(
        (s.opacity - 0.5).abs() < 0.01,
        "opacity 0.5 expected, got {}",
        s.opacity
    );
    apply_property(&mut s, "opacity", "0");
    assert!(s.opacity < 0.01, "opacity 0 expected, got {}", s.opacity);
    apply_property(&mut s, "opacity", "1");
    assert!(s.opacity > 0.99, "opacity 1 expected, got {}", s.opacity);
}

#[test]
fn css_position_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "position", "static");
    assert_eq!(s.position, Position::Static);
    apply_property(&mut s, "position", "relative");
    assert_eq!(s.position, Position::Relative);
    apply_property(&mut s, "position", "absolute");
    assert_eq!(s.position, Position::Absolute);
    apply_property(&mut s, "position", "fixed");
    assert_eq!(s.position, Position::Fixed);
}

#[test]
fn css_box_sizing_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "box-sizing", "border-box");
    assert_eq!(s.box_sizing, BoxSizing::BorderBox);
    apply_property(&mut s, "box-sizing", "content-box");
    assert_eq!(s.box_sizing, BoxSizing::ContentBox);
}

#[test]
fn css_text_overflow_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "text-overflow", "ellipsis");
    assert_eq!(s.text_overflow, TextOverflow::Ellipsis);
    apply_property(&mut s, "text-overflow", "clip");
    assert_eq!(s.text_overflow, TextOverflow::Clip);
}

#[test]
fn css_outline_properties() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "outline-width", "2px");
    assert!(
        (s.outline_width - 2.0).abs() < 0.1,
        "outline-width 2px expected"
    );
    apply_property(&mut s, "outline-style", "solid");
    assert_eq!(s.outline_style, BorderStyle::Solid);
    apply_property(&mut s, "outline-style", "dashed");
    assert_eq!(s.outline_style, BorderStyle::Dashed);
    apply_property(&mut s, "outline-offset", "3px");
    assert!(
        (s.outline_offset - 3.0).abs() < 0.1,
        "outline-offset 3px expected"
    );
}

#[test]
fn css_background_size_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "background-size", "cover");
    assert_eq!(s.background_size, BackgroundSize::Cover);
    apply_property(&mut s, "background-size", "contain");
    assert_eq!(s.background_size, BackgroundSize::Contain);
    apply_property(&mut s, "background-size", "auto");
    assert_eq!(s.background_size, BackgroundSize::Auto);
}

#[test]
fn css_object_fit_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "object-fit", "contain");
    assert_eq!(s.object_fit, ObjectFit::Contain);
    apply_property(&mut s, "object-fit", "cover");
    assert_eq!(s.object_fit, ObjectFit::Cover);
}

#[test]
fn css_z_index_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "z-index", "10");
    assert_eq!(s.z_index, 10);
    apply_property(&mut s, "z-index", "-5");
    assert_eq!(s.z_index, -5);
}

#[test]
fn css_border_radius_property() {
    let s = style_with("border-radius", "8px");
    assert_eq!(s.border_radius, CssLength::Px(8.0));
}

#[test]
fn css_letter_spacing_property() {
    let s = style_with("letter-spacing", "2px");
    assert_eq!(s.letter_spacing, CssLength::Px(2.0));
}

#[test]
fn css_word_spacing_property() {
    let s = style_with("word-spacing", "5px");
    assert_eq!(s.word_spacing, CssLength::Px(5.0));
}

#[test]
fn css_vertical_align_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "vertical-align", "middle");
    assert_eq!(s.vertical_align, VerticalAlign::Middle);
    apply_property(&mut s, "vertical-align", "top");
    assert_eq!(s.vertical_align, VerticalAlign::Top);
    apply_property(&mut s, "vertical-align", "bottom");
    assert_eq!(s.vertical_align, VerticalAlign::Bottom);
    apply_property(&mut s, "vertical-align", "super");
    assert_eq!(s.vertical_align, VerticalAlign::Super);
    apply_property(&mut s, "vertical-align", "sub");
    assert_eq!(s.vertical_align, VerticalAlign::Sub);
}

#[test]
fn css_list_style_type_property() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "list-style-type", "decimal");
    assert_eq!(s.list_style_type, ListStyleType::Decimal);
    apply_property(&mut s, "list-style-type", "circle");
    assert_eq!(s.list_style_type, ListStyleType::Circle);
    apply_property(&mut s, "list-style-type", "none");
    assert_eq!(s.list_style_type, ListStyleType::None);
}

// ============================================================
// !important stripping
// ============================================================

#[test]
fn css_important_stripped_from_color() {
    let decls = parse_declarations("color: #ff0000 !important;");
    assert_eq!(decls.len(), 1);
    assert_eq!(
        decls.get("color").map(|s| s.as_str()),
        Some("#ff0000"),
        "!important should be stripped, leaving just the value"
    );
}

#[test]
fn css_important_stripped_from_multiple() {
    let decls = parse_declarations(
        "background: #21262d !important; color: #6e7681 !important; cursor: default;",
    );
    assert_eq!(decls.len(), 3);
    assert_eq!(decls.get("background").map(|s| s.as_str()), Some("#21262d"));
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("#6e7681"));
    assert_eq!(decls.get("cursor").map(|s| s.as_str()), Some("default"));
}

#[test]
fn css_important_color_applied() {
    let doc = parse_html(
        "<html><head><style>.red { color: #ff0000 !important; }</style></head>\
         <body><p class=\"red\">Text</p></body></html>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some(), "p element should be found");
    let p = p.unwrap();
    assert_eq!(
        p.style.color,
        Color::rgb(255, 0, 0),
        "color: #ff0000 !important should be applied"
    );
}

#[test]
fn css_important_background_applied() {
    let doc = parse_html(
        "<html><head><style>.bg { background-color: #334155 !important; }</style></head>\
         <body><div class=\"bg\">Box</div></body></html>",
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div"
            && b.attributes
                .get("class")
                .map(|v| v == "bg")
                .unwrap_or(false)
    });
    assert!(div.is_some(), "div.bg should be found");
    let c = div.unwrap().style.background_color;
    assert_eq!(c.r, 0x33, "background red channel should be 0x33");
}

// ============================================================
// !important cascade priority
// ============================================================

#[test]
fn css_important_beats_higher_specificity() {
    // A low-specificity rule with !important should override a higher-specificity rule without it.
    let doc = parse_html(
        "<html><head><style>\
           p { color: red !important; }\
           body p.special { color: blue; }\
         </style></head>\
         <body><p class=\"special\">Text</p></body></html>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "p" && b.attributes.get("class").map_or(false, |v| v == "special")
    });
    assert!(p.is_some());
    // !important (specificity 1) should beat normal (specificity 12)
    assert_eq!(
        p.unwrap().style.color,
        Color::rgb(255, 0, 0),
        "!important should override higher-specificity normal rule"
    );
}

#[test]
fn css_important_beats_inline_style() {
    // An !important stylesheet rule should override an inline style.
    let doc = parse_html(
        "<html><head><style>\
           p { color: green !important; }\
         </style></head>\
         <body><p style=\"color: blue;\">Text</p></body></html>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(
        p.unwrap().style.color,
        Color::rgb(0, 128, 0),
        "!important in stylesheet should override inline style"
    );
}

#[test]
fn css_inline_important_beats_stylesheet_important() {
    // An inline style with !important should beat a stylesheet !important.
    let doc = parse_html(
        "<html><head><style>\
           p { color: green !important; }\
         </style></head>\
         <body><p style=\"color: blue !important;\">Text</p></body></html>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(
        p.unwrap().style.color,
        Color::rgb(0, 0, 255),
        "inline !important should beat stylesheet !important"
    );
}

#[test]
fn css_important_does_not_affect_other_properties() {
    // !important on one property shouldn't affect other properties in the same rule.
    let doc = parse_html(
        "<html><head><style>\
           p { color: red !important; background-color: yellow; }\
           p.x { color: blue; background-color: green; }\
         </style></head>\
         <body><p class=\"x\">Text</p></body></html>",
    );
    let p = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "p" && b.attributes.get("class").map_or(false, |v| v == "x")
    });
    assert!(p.is_some());
    let p = p.unwrap();
    // color: red !important should win over color: blue (higher specificity)
    assert_eq!(p.style.color, Color::rgb(255, 0, 0));
    // background-color: green (higher specificity, normal) should win over yellow (normal)
    assert_eq!(p.style.background_color, Color::rgb(0, 128, 0));
}

// ── find_box helper ───────────────────────────────────────────────────────────
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
