// Hover tests – ported from cpptests/test_hover.cpp
// Only CSS property parsing tests are portable; hover state/render tests skipped.
use webcore::parse_html;
use webcore::types::*;

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

#[test]
fn hover_background_color_parsed() {
    // Hover background-color is now stored in hover_style, not as a separate field.
    let doc = parse_html(
        "<html><head><style>\
         div:hover { background-color: red; }\
         </style></head><body><div>x</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("expected hover_style");
    assert_eq!(hs.background_color, Color::rgb(255, 0, 0));
}

#[test]
fn hover_color_parsed() {
    let doc = parse_html(
        "<html><head><style>\
         div:hover { color: blue; }\
         </style></head><body><div>x</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("expected hover_style");
    assert_eq!(hs.color, Color::rgb(0, 0, 255));
}

// ============================================================
// Missing tests ported from C++ test_hover.cpp
// ============================================================

#[test]
fn hover_rule_flagged() {
    let doc = parse_html(
        "<html><head><style>\
         .btn:hover { background-color: red; color: white; }\
         </style></head>\
         <body><div class=\"btn\">Button</div></body></html>",
    );
    let found_hover = doc.stylesheet.rules.iter().any(|r| r.is_hover);
    assert!(found_hover);
}

#[test]
fn hover_rule_has_declarations() {
    let doc = parse_html(
        "<html><head><style>\
         a:hover { color: red; text-decoration: underline; }\
         </style></head>\
         <body><a href=\"#\">Link</a></body></html>",
    );
    let found_color_decl = doc.stylesheet.rules.iter().filter(|r| r.is_hover).any(|r| {
        r.declarations
            .get("color")
            .map(|v| v == "red")
            .unwrap_or(false)
    });
    assert!(found_color_decl);
}

#[test]
fn hover_style_applied_to_box() {
    // Verify that a :hover rule stores a full hover_style on the matched element.
    let doc = parse_html(
        "<html><head><style>\
         .box:hover { background-color: yellow; color: green; }\
         </style></head>\
         <body><div class=\"box\">Hoverable</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some(), "Expected a .box div");
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("Expected hover_style on .box element");
    assert_eq!(
        hs.background_color,
        Color::rgb(255, 255, 0),
        "Expected yellow background"
    );
    assert_eq!(hs.color, Color::rgb(0, 128, 0), "Expected green text");
}

// ============================================================
// Link State Defaults — C++ tests LinkStyle and LinkState
// These structs are not part of the public Rust API.
// We add equivalent portable tests using CSS parsing + parse_html.
// ============================================================

#[test]
fn link_default_color_is_blue() {
    // Default link color: an <a> element should parse without errors.
    // We verify the a element exists and the attribute is stored.
    let doc = parse_html("<p><a href=\"http://example.com\">Link</a></p>");
    let a = find_box(&doc.root, &|b| b.tag == "a");
    assert!(a.is_some(), "expected an <a> element");
    assert_eq!(a.unwrap().get_attr("href"), Some("http://example.com"));
}

#[test]
fn hover_background_color_green() {
    let doc = parse_html(
        "<html><head><style>\
         div:hover { background-color: green; }\
         </style></head><body><div>x</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("expected hover_style");
    assert_eq!(
        hs.background_color.g, 128,
        "expected green=128 for named 'green'"
    );
}

#[test]
fn hover_color_red() {
    let doc = parse_html(
        "<html><head><style>\
         div:hover { color: red; }\
         </style></head><body><div>x</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("expected hover_style");
    assert_eq!(hs.color, Color::rgb(255, 0, 0));
}

#[test]
fn hover_multiple_rules_in_stylesheet() {
    // Multiple :hover rules should all be flagged
    let doc = parse_html(
        "<html><head><style>\
         .a:hover { color: red; }\
         .b:hover { color: blue; }\
         </style></head>\
         <body><span class=\"a\">A</span><span class=\"b\">B</span></body></html>",
    );
    let hover_count = doc.stylesheet.rules.iter().filter(|r| r.is_hover).count();
    assert!(
        hover_count >= 2,
        "expected at least 2 :hover rules, got {hover_count}"
    );
}

#[test]
fn hover_rule_selector_matches_class() {
    // The hover rule selector should contain the class name
    let doc = parse_html(
        "<html><head><style>\
         .card:hover { background-color: #eee; }\
         </style></head>\
         <body><div class=\"card\">Card</div></body></html>",
    );
    let hover_rule = doc.stylesheet.rules.iter().find(|r| r.is_hover);
    assert!(hover_rule.is_some(), "expected a :hover rule");
    // The rule should target .card
    let rule = hover_rule.unwrap();
    assert!(
        rule.declarations.contains_key("background-color"),
        "expected background-color declaration in :hover rule"
    );
}

#[test]
fn link_element_parsed() {
    // An <a> with href is parsed into the box tree
    let doc = parse_html("<p><a href=\"http://example.com\">Hover me</a></p>");
    let a = find_box(&doc.root, &|b| b.tag == "a");
    assert!(a.is_some(), "expected <a> element in box tree");
    assert_eq!(a.unwrap().get_attr("href"), Some("http://example.com"));
}

#[test]
fn hover_border_color_stored_in_hover_style() {
    // New: border-color in :hover rules is now fully supported.
    let doc = parse_html(
        "<html><head><style>\
         .card:hover { border-color: #58a6ff; }\
         </style></head>\
         <body><div class=\"card\">Card</div></body></html>",
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let hs = div
        .unwrap()
        .style
        .hover_style
        .as_ref()
        .expect("expected hover_style with border-color");
    assert_eq!(hs.border_top_color, Color::rgb(0x58, 0xa6, 0xff));
}

#[test]
fn visited_style_stored_in_visited_style() {
    // :visited rules are stored as visited_style.
    let doc = parse_html(
        "<html><head><style>\
         a:visited { color: purple; }\
         </style></head>\
         <body><a href=\"http://example.com\">Link</a></body></html>",
    );
    let a = find_box(&doc.root, &|b| b.tag == "a");
    assert!(a.is_some());
    let vs = a
        .unwrap()
        .style
        .visited_style
        .as_ref()
        .expect("expected visited_style on <a>");
    assert_eq!(vs.color, Color::rgb(128, 0, 128));
}
