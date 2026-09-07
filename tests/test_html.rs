// Ported from cpptests/test_html.cpp
// Integration tests using the public webcore API.

use webcore::types::*;
use webcore::{load_html, parse_html};

// ── Helpers ──────────────────────────────────────────────────────────────────

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

fn get_body(doc: &Document) -> Option<&WebCore> {
    find_box(&doc.root, &|b: &WebCore| b.tag == "body")
}

// ============================================================
// HTML Parsing
// ============================================================

#[test]
fn html_basic_document() {
    // BasicDocument: doc.root is non-null and contains text.
    // We use text_content() instead of doc.text (unavailable in Rust).
    let doc = parse("<p>Hello</p>");
    assert!(doc_text(&doc).contains("Hello"));
}

#[test]
fn html_nested_elements() {
    // NestedElements: nested structure parses correctly.
    let doc = parse("<div><p>Inner</p></div>");
    assert!(doc_text(&doc).contains("Inner"));
}

#[test]
fn html_multiple_children() {
    // MultipleChildren: multiple siblings all produce text.
    let doc = parse("<div><p>First</p><p>Second</p><p>Third</p></div>");
    assert!(doc_text(&doc).contains("First"));
    assert!(doc_text(&doc).contains("Second"));
    assert!(doc_text(&doc).contains("Third"));
}

#[test]
fn html_inline_style() {
    // InlineStyle: inline style color parsed into ComputedStyle.
    let doc = parse(r#"<p style="color: red;">Red text</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.style.color == Color::rgb(255, 0, 0)
    });
    assert!(b.is_some());
}

#[test]
fn html_void_elements() {
    // VoidElements: text around <br> is preserved.
    let doc = parse("<p>Before<br>After</p>");
    assert!(doc_text(&doc).contains("Before"));
    assert!(doc_text(&doc).contains("After"));
}

#[test]
fn html_image_element() {
    // ImageElement: <img> parses without panic.
    let doc = parse(r#"<img src="test.png" width="100" height="50">"#);
    let _ = doc;
}

#[test]
fn html_style_block() {
    // StyleBlock: rules collected from <style>.
    let doc = parse(
        r#"<html><head><style>p { color: blue; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 1);
}

#[test]
fn html_multiple_style_blocks() {
    // MultipleStyleBlocks: rules from multiple <style> blocks all collected.
    let doc = parse(
        r#"<html><head><style>p { color: blue; }</style><style>.red { color: red; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 2);
}

#[test]
fn html_dir_attribute() {
    // DirAttribute: dir="rtl" stored in attributes (no Direction::RTL style field in Rust).
    let doc = parse(r#"<p dir="rtl">Arabic text</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("dir").map(|v| v == "rtl").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_lang_attribute() {
    // LangAttribute: lang stored as attribute (not a style field in Rust).
    let doc = parse(r#"<p lang="ar">Arabic</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("lang").map(|v| v == "ar").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_class_attribute() {
    // ClassAttribute: class stored in attributes map.
    // C++ uses b.className — Rust uses b.attributes.get("class").
    let doc = parse(r#"<div class="foo bar">Test</div>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|v| v == "foo bar")
            .unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_id_attribute() {
    // IdAttribute: id stored in attributes map.
    // C++ uses b.id — Rust uses b.attributes.get("id").
    let doc = parse(r#"<div id="main">Test</div>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|v| v == "main").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_headings_preserved() {
    // HeadingsPreserved: h1, h2, h3 all appear as boxes.
    let doc = parse("<h1>H1</h1><h2>H2</h2><h3>H3</h3>");
    let heading_count = count_boxes(&doc.root, &|b: &WebCore| {
        b.tag == "h1" || b.tag == "h2" || b.tag == "h3"
    });
    assert_eq!(heading_count, 3);
}

#[test]
fn html_list_elements() {
    // ListElements: three <li> boxes created.
    let doc = parse("<ul><li>A</li><li>B</li><li>C</li></ul>");
    let li_count = count_boxes(&doc.root, &|b: &WebCore| b.tag == "li");
    assert_eq!(li_count, 3);
}

#[test]
fn html_bold_italic_elements() {
    // BoldItalicElements: b and i text preserved.
    // SKIP doc.text — use text_content().
    let doc = parse("<p><b>Bold</b> and <i>Italic</i></p>");
    assert!(doc_text(&doc).contains("Bold"));
    assert!(doc_text(&doc).contains("Italic"));
}

// SKIP: AnchorElement — uses WalkBoxes + b.inlineContent + b.style.url, not available in Rust.

// SKIP: RoundTrip, RoundTripPreservesStructure — use SerializeHTML, covered in test_serialization.rs.

// ============================================================
// Charset / Encoding
// ============================================================

#[test]
fn html_utf8_basic() {
    // UTF8Basic: simple UTF-8 text parses.
    let doc = parse("<p>Hello World</p>");
    assert!(doc_text(&doc).contains("Hello"));
}

#[test]
fn html_charset_meta() {
    // CharsetMeta: charset meta doesn't break parsing.
    let doc = parse(r#"<html><head><meta charset="utf-8"></head><body><p>Test</p></body></html>"#);
    assert!(doc_text(&doc).contains("Test"));
}

#[test]
fn html_empty_document() {
    // EmptyDocument: empty string still produces a root.
    let doc = parse("");
    let _ = doc;
}

// ============================================================
// html/body structure
// ============================================================

#[test]
fn html_root_is_html() {
    // RootIsHtml: root tag is "html".
    let doc = parse("<html><body><p>Hello</p></body></html>");
    assert_eq!(doc.root.tag, "html");
}

#[test]
fn html_body_is_child_of_root() {
    // BodyIsChildOfRoot: body exists and has correct tag.
    // C++ uses GetBody(doc) — Rust: find by tag == "body".
    let doc = parse("<html><body><p>Hello</p></body></html>");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_implicit_html_and_body() {
    // ImplicitHtmlAndBody: even without explicit tags, html and body are created.
    let doc = parse("<p>Hello</p>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_explicit_html_implicit_body() {
    // ExplicitHtmlImplicitBody: explicit html, implicit body.
    let doc = parse("<html><p>Hello</p></html>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_explicit_both_html_body() {
    // ExplicitBothHtmlBody: both tags explicit.
    let doc = parse("<html><body><p>Hello</p></body></html>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

// ============================================================
// CSS matching on html/body
// ============================================================

#[test]
fn html_css_matches_root() {
    // HtmlCSSMatchesRoot: html { color: red } applied to root box.
    // C++ checks doc.root->style.color.Red() == 255.
    let doc = parse(r#"<style>html { color: red; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.color.r, 255);
}

#[test]
fn html_css_background_matches_root() {
    // HtmlCSSBackgroundMatchesRoot: html { background-color: blue } applied to root.
    let doc = parse(r#"<style>html { background-color: blue; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.background_color.b, 255);
}

#[test]
fn html_body_css_matches_body() {
    // BodyCSSMatchesBody: html gets red, body gets blue.
    let doc = parse(r#"<style>html { color: red; } body { color: blue; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.color.r, 255);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.color.b, 255);
    assert_eq!(body.style.color.r, 0);
}

#[test]
fn html_body_css_matches_without_explicit_tag() {
    // BodyCSSMatchesWithoutExplicitTag: implicit body still gets style.
    let doc = parse(r#"<style>body { color: red; background: #0d1117; }</style><p>Text</p>"#);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.tag, "body");
    assert_eq!(body.style.color.r, 255);
    assert_eq!(body.style.background_color.r, 0x0d);
}

#[test]
fn html_body_css_color_matches() {
    // BodyCSSColorMatches: explicit body + head style.
    let doc = parse(
        r#"<html><head><style>body { color: red; }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.color.r, 255);
    assert_eq!(body.style.color.g, 0);
    assert_eq!(body.style.color.b, 0);
}

#[test]
fn html_body_css_color_inherits() {
    // BodyCSSColorInherits: body color inherits to child <p> after layout.
    let doc = parse_and_layout(
        r#"<html><head><style>body { color: blue; }</style></head><body><p>Text</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    let p = p.unwrap();
    assert_eq!(p.style.color.b, 255);
    assert_eq!(p.style.color.r, 0);
}

#[test]
fn html_body_css_background_color() {
    // BodyCSSBackgroundColor: hex background-color on body.
    let doc = parse(
        r##"<html><head><style>body { background-color: #1e293b; }</style></head><body><p>Text</p></body></html>"##,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.background_color.r, 0x1e);
    assert_eq!(body.style.background_color.g, 0x29);
    assert_eq!(body.style.background_color.b, 0x3b);
}

#[test]
fn html_body_css_margin() {
    // BodyCSSMargin: body margin: 0 resolves to zero.
    let doc = parse_and_layout(
        r#"<html><head><style>body { margin: 0; }</style></head><body><p>Text</p></body></html>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 0.0);
    assert_eq!(body.style.margin_right.resolve(16.0, 0.0, 16.0), 0.0);
    assert_eq!(body.style.margin_bottom.resolve(16.0, 0.0, 16.0), 0.0);
    assert_eq!(body.style.margin_left.resolve(16.0, 0.0, 16.0), 0.0);
}

#[test]
fn html_body_css_padding() {
    // BodyCSSPadding: body padding: 20px resolves correctly.
    let doc = parse_and_layout(
        r#"<html><head><style>body { padding: 20px; }</style></head><body><p>Text</p></body></html>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.padding_top.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(body.style.padding_right.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(body.style.padding_bottom.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(body.style.padding_left.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn html_body_css_font_family() {
    // BodyCSSFontFamily: font-family set on body is non-empty.
    let doc = parse(
        r#"<html><head><style>body { font-family: monospace; }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert!(!body.unwrap().style.font_family.is_empty());
}

#[test]
fn html_body_css_font_size() {
    // BodyCSSFontSize: 24pt font-size is larger than default.
    let doc_default = parse_and_layout(r#"<html><body><p>Text</p></body></html>"#, 800.0);
    let doc_big = parse_and_layout(
        r#"<html><head><style>body { font-size: 24pt; }</style></head><body><p>Text</p></body></html>"#,
        800.0,
    );
    let p_default = find_box(&doc_default.root, &|b: &WebCore| b.tag == "p");
    let p_big = find_box(&doc_big.root, &|b: &WebCore| b.tag == "p");
    assert!(p_default.is_some());
    assert!(p_big.is_some());
    let size_default = p_default.unwrap().style.font_size.resolve(16.0, 0.0, 16.0);
    let size_big = p_big.unwrap().style.font_size.resolve(16.0, 0.0, 16.0);
    assert!(
        size_big > size_default,
        "big font ({}) should exceed default ({})",
        size_big,
        size_default
    );
}

#[test]
fn html_body_css_multiple_properties() {
    // BodyCSSMultipleProperties: multiple body CSS properties all applied.
    let doc = parse(
        r#"<html><head><style>body { color: #c9d1d9; background-color: #0d1117; margin: 0; padding: 10px; }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.color.r, 0xc9);
    assert_eq!(body.style.background_color.r, 0x0d);
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 0.0);
    assert_eq!(body.style.padding_top.resolve(16.0, 0.0, 16.0), 10.0);
}

#[test]
fn html_body_legacy_bgcolor() {
    // BodyLegacyBgcolor: bgcolor attribute applied to body background.
    let doc = parse(r##"<body bgcolor="#ff0000"><p>Text</p></body>"##);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.background_color.r, 255);
}

#[test]
fn html_body_legacy_text_attr() {
    // BodyLegacyTextAttr: text attribute applied to body color.
    let doc = parse(r##"<body text="#00ff00"><p>Text</p></body>"##);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.color.g, 255);
}

#[test]
fn html_body_inline_style() {
    // BodyInlineStyle: inline style on body.
    let doc = parse(r#"<body style="color: orange;"><p>Text</p></body>"#);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.color.r, 255);
    assert!(body.style.color.g > 100);
    assert_eq!(body.style.color.b, 0);
}

#[test]
fn html_body_css_overrides_legacy() {
    // BodyCSSOverridesLegacy: stylesheet color wins over legacy text attr.
    let doc = parse(
        r#"<html><head><style>body { color: blue; }</style></head><body text="red"><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.color.b, 255);
}

#[test]
fn html_body_child_overrides_color() {
    // BodyChildOverridesColor: .special selector overrides body color.
    let doc = parse_and_layout(
        r#"<html><head><style>body { color: red; } .special { color: green; }</style></head><body><p class="special">Text</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    let p = p.unwrap();
    assert_eq!(p.style.color.g, 128); // CSS "green" = #008000
    assert_eq!(p.style.color.r, 0);
}

#[test]
fn html_body_css_border() {
    // BodyCSSBorder: body border applied.
    let doc = parse(
        r#"<html><head><style>body { border: 2px solid red; }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.border_top_width.resolve(16.0, 0.0, 16.0), 2.0);
    assert_eq!(body.style.border_top_color.r, 255);
}

#[test]
fn html_body_css_background_shorthand() {
    // BodyCSSBackgroundShorthand: background shorthand as hex color.
    let doc = parse(
        r##"<html><head><style>body { background: #0d1117; }</style></head><body><p>Text</p></body></html>"##,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.background_color.r, 0x0d);
    assert_eq!(body.style.background_color.g, 0x11);
    assert_eq!(body.style.background_color.b, 0x17);
}

#[test]
fn html_body_css_gradient_background() {
    // BodyCSSGradientBackground: gradient parses without panic.
    let doc = parse(
        r#"<html><head><style>body { background: linear-gradient(135deg, #1e293b, #334155); }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
}

#[test]
fn html_body_background_used_for_canvas() {
    // BodyBackgroundUsedForCanvas: body bg preserved after layout.
    let doc = parse_and_layout(
        r##"<html><head><style>body { background: #0d1117; margin: 0; }</style></head><body><p>Text</p></body></html>"##,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.background_color.r, 0x0d);
}

#[test]
fn html_html_body_separate_margin() {
    // HtmlBodySeparateMargin: body margin set, root is html.
    let doc = parse_and_layout(r#"<style>body { margin: 20px; }</style><p>Text</p>"#, 800.0);
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(body.style.margin_left.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn html_html_background_body_margin() {
    // HtmlBackgroundBodyMargin: html red, body blue, each correct.
    let doc = parse(
        r#"<style>html { background-color: red; } body { background-color: blue; margin: 20px; }</style><p>Text</p>"#,
    );
    assert_eq!(doc.root.style.background_color.r, 255);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.background_color.b, 255);
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn html_content_goes_in_body() {
    // ContentGoesInBody: <p> is a descendant of body.
    let doc = parse("<p>Hello</p>");
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    let p = find_box(body, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
}

#[test]
fn html_ua_body_margin() {
    // UABodyMargin: UA stylesheet gives body 8px margin (browser default).
    let doc = parse_and_layout("<p>Text</p>", 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 8.0);
    assert_eq!(body.style.margin_left.resolve(16.0, 0.0, 16.0), 8.0);
}

#[test]
fn html_body_layout_position() {
    // BodyLayoutPosition: body margin_rect at x=0; content inset by UA 8px margin.
    let doc = parse_and_layout("<p>Text</p>", 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 8.0);
    assert_eq!(body.layout.content_rect.w, 784.0);
}

#[test]
fn html_body_layout_with_explicit_margin() {
    // BodyLayoutWithExplicitMargin: 8px margin insets content.
    let doc = parse_and_layout(r#"<body style="margin: 8px;"><p>Text</p></body>"#, 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 8.0);
    assert_eq!(body.layout.content_rect.w, 784.0);
}

#[test]
fn html_body_layout_with_box_sizing() {
    // BodyLayoutWithBoxSizing: box-sizing: border-box doesn't affect margin offset.
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; }</style><p>Text</p>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 8.0); // UA body margin: 8px
}

#[test]
fn html_body_layout_demo_pattern() {
    // BodyLayoutDemoPattern: complex demo pattern — body stays at x=0.
    let doc = parse_and_layout(
        r##"<body text="#2c3e50"><style>* { box-sizing: border-box; }</style><div style="position: fixed; top: 0; left: 0; width: 100%;"><a>Home</a><a>Features</a></div><h1 style="margin-top: 44px;">Title</h1><p>Text</p><div style="position: fixed; bottom: 16px; right: 16px; width: 48px; height: 48px;">+</div></body>"##,
        685.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 8.0); // UA body margin: 8px
    assert_eq!(doc.root.layout.margin_rect.x, 0.0);
    assert_eq!(doc.root.layout.content_rect.w, 685.0);
}

#[test]
fn html_body_layout_with_floats() {
    // BodyLayoutWithFloats: floats inside body don't shift body.
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; }</style><div style="width: 50%; float: left;">Left</div><div style="width: 50%; float: right;">Right</div><div style="clear: both;"></div><p>After floats</p>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 8.0); // UA body margin: 8px
}

#[test]
fn html_body_bfc_isolation() {
    // BodyBFCIsolation: float inside body doesn't affect body x.
    let doc = parse_and_layout(
        r#"<body><div style="width: 300px; float: left;">Float</div><p>Text alongside float</p></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.w, 784.0); // 800 - 2*8px UA body margin
}

#[test]
fn html_body_margin_padding_box_sizing_combinations() {
    // BodyMarginPaddingBoxSizingCombinations: content-box and border-box.
    {
        let doc = parse_and_layout(
            r#"<body style="margin: 10px; padding: 20px;"><p>X</p></body>"#,
            800.0,
        );
        let body = get_body(&doc);
        assert!(body.is_some());
        // contentWidth = 800 - 10 - 10 - 20 - 20 = 740
        assert_eq!(body.unwrap().layout.content_rect.w, 740.0);
        assert_eq!(get_body(&doc).unwrap().layout.margin_rect.x, 0.0);
    }
    {
        let doc = parse_and_layout(
            r#"<style>* { box-sizing: border-box; }</style><body style="margin: 10px; padding: 20px;"><p>X</p></body>"#,
            800.0,
        );
        let body = get_body(&doc);
        assert!(body.is_some());
        assert_eq!(body.unwrap().layout.content_rect.w, 740.0);
        assert_eq!(get_body(&doc).unwrap().layout.margin_rect.x, 0.0);
    }
}

#[test]
fn html_body_explicit_width_with_margin() {
    // BodyExplicitWidthWithMargin: explicit width on body not reduced by margin.
    let doc = parse_and_layout(
        r#"<body style="width: 600px; margin: 10px;"><p>X</p></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.content_rect.w, 600.0);
    assert_eq!(body.layout.margin_rect.x, 0.0);
}

#[test]
fn html_body_no_margin_full_viewport() {
    // BodyNoMarginFullViewport: UA body margin 8px insets content from full 1024px viewport.
    let doc = parse_and_layout("<p>Hello</p>", 1024.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.content_rect.x, 8.0); // UA body margin: 8px
    assert_eq!(body.layout.content_rect.w, 1008.0); // 1024 - 2*8px
    assert_eq!(body.layout.margin_rect.w, 1024.0); // margin_rect spans full viewport
}

// ============================================================
// Table inside body with padding (edit_demo regression)
// ============================================================

#[test]
fn html_table_full_width_in_body_with_padding() {
    // TableFullWidthInBodyWithPadding: table matches body content width.
    // body: UA margin 8px + explicit padding 16px → content = 800 - 2*8 - 2*16 = 752
    let doc = parse_and_layout(
        r#"<body style="padding: 16px;"><table style="width: 100%;"><tr><td>A</td><td>B</td><td>Long description text</td></tr></table></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().layout.content_rect.w, 752.0);
    let table = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table.is_some());
    assert_eq!(table.unwrap().layout.margin_rect.w, 752.0);
}

#[test]
fn html_table_full_width_in_body_with_padding_and_box_sizing() {
    // TableFullWidthInBodyWithPaddingAndBoxSizing: same with border-box.
    // body: UA margin 8px + explicit padding 16px → content = 800 - 2*8 - 2*16 = 752
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; }</style><body style="padding: 16px;"><table style="width: 100%; border-collapse: collapse;"><tr><td style="padding: 8px; border: 1px solid #ccc;">Name</td><td style="padding: 8px; border: 1px solid #ccc;">Type</td><td style="padding: 8px; border: 1px solid #ccc;">Load an HTML string into the editor</td></tr></table></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().layout.content_rect.w, 752.0);
    let table = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table.is_some());
    assert_eq!(table.unwrap().layout.margin_rect.w, 752.0);
}

#[test]
fn html_table_in_body_with_margin_and_padding() {
    // TableInBodyWithMarginAndPadding: margin + padding reduce body content width.
    let doc = parse_and_layout(
        r#"<body style="margin: 8px; padding: 16px;"><table style="width: 100%;"><tr><td>Cell</td></tr></table></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    // contentWidth = 800 - 8 - 8 - 16 - 16 = 752
    assert_eq!(body.unwrap().layout.content_rect.w, 752.0);
    let table = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table.is_some());
    assert_eq!(table.unwrap().layout.margin_rect.w, 752.0);
}

// ============================================================
// Canvas background propagation
// ============================================================

#[test]
fn html_canvas_bg_from_html() {
    // CanvasBgFromHtml: html background-color set.
    let doc = parse_and_layout(
        r#"<html style="background-color: red;"><body><p>X</p></body></html>"#,
        800.0,
    );
    assert_eq!(doc.root.style.background_color, Color::rgb(255, 0, 0));
}

#[test]
fn html_canvas_bg_fallback_from_body() {
    // CanvasBgFallbackFromBody: html has no bg, body has blue.
    let doc = parse_and_layout(
        r#"<body style="background-color: blue;"><p>X</p></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(doc.root.style.background_color, Color::TRANSPARENT);
    assert_eq!(body.unwrap().style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn html_canvas_bg_html_overrides_body() {
    // CanvasBgHtmlOverridesBody: html green, body yellow, both correct.
    let doc = parse_and_layout(
        r#"<html style="background-color: green;"><body style="background-color: yellow;"><p>X</p></body></html>"#,
        800.0,
    );
    assert_eq!(doc.root.style.background_color, Color::rgb(0, 128, 0));
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(
        body.unwrap().style.background_color,
        Color::rgb(255, 255, 0)
    );
}

#[test]
fn html_canvas_bg_neither_set() {
    // CanvasBgNeitherSet: both html and body have transparent background.
    let doc = parse_and_layout("<p>Plain text</p>", 800.0);
    assert_eq!(doc.root.style.background_color, Color::TRANSPARENT);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.background_color, Color::TRANSPARENT);
}

// ============================================================
// Double layout (widget relayouts on width change)
// ============================================================

#[test]
fn html_double_layout_preserves_body_position() {
    // DoubleLayoutPreservesBodyPosition: separate parse+layout at different widths.
    let doc = parse_and_layout("<p>Text</p>", 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.w, 784.0); // 800 - 2*8px UA margin

    let doc2 = parse_and_layout("<p>Text</p>", 700.0);
    let body2 = get_body(&doc2);
    assert!(body2.is_some());
    let body2 = body2.unwrap();
    assert_eq!(body2.layout.margin_rect.x, 0.0);
    assert_eq!(body2.layout.content_rect.w, 684.0); // 700 - 2*8px UA margin
}

#[test]
fn html_double_layout_with_floats_preserves_body() {
    // DoubleLayoutWithFloatsPreservesBody: floats don't leak across re-layouts.
    let html = r#"<style>* { box-sizing: border-box; }</style><div style="width: 50%; float: left;">Left</div><div style="width: 50%; float: right;">Right</div><div style="clear: both;"></div><p>After</p>"#;
    let doc = parse_and_layout(html, 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().layout.margin_rect.x, 0.0);

    let doc2 = parse_and_layout(html, 750.0);
    let body2 = get_body(&doc2);
    assert!(body2.is_some());
    let body2 = body2.unwrap();
    assert_eq!(body2.layout.margin_rect.x, 0.0);
    assert_eq!(body2.layout.content_rect.w, 734.0); // 750 - 2*8px UA margin
}

#[test]
fn html_double_layout_same_width_stable() {
    // DoubleLayoutSameWidthStable: identical results for same-width re-layout.
    let html = r#"<body style="margin: 10px; padding: 5px;"><h1>Title</h1><p>Text</p></body>"#;
    let doc1 = parse_and_layout(html, 600.0);
    let body1 = get_body(&doc1).unwrap();
    let x1 = body1.layout.margin_rect.x;
    let w1 = body1.layout.content_rect.w;
    let h1 = body1.layout.content_rect.h;

    let doc2 = parse_and_layout(html, 600.0);
    let body2 = get_body(&doc2).unwrap();
    assert_eq!(body2.layout.margin_rect.x, x1);
    assert_eq!(body2.layout.content_rect.w, w1);
    assert_eq!(body2.layout.content_rect.h, h1);
}

#[test]
fn html_double_layout_table_in_body_with_padding() {
    // DoubleLayoutTableInBodyWithPadding: table width stable across re-layouts.
    let html = r#"<body style="padding: 16px;"><table style="width: 100%;"><tr><td>A</td><td>B</td><td>C</td></tr></table></body>"#;

    let doc1 = parse_and_layout(html, 800.0);
    let table1 = find_box(&doc1.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table1.is_some());
    let tw1 = table1.unwrap().layout.margin_rect.w;

    let doc2 = parse_and_layout(html, 800.0);
    let table2 = find_box(&doc2.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table2.is_some());
    assert_eq!(table2.unwrap().layout.margin_rect.w, tw1);
    assert_eq!(tw1, 752.0); // 800 - 2*8px UA margin - 2*16px body padding
}

// ============================================================
// Head, meta, title, script — non-visual elements
// ============================================================

#[test]
fn html_head_content_not_rendered() {
    // HeadContentNotRendered: title text does not appear in rendered text.
    let doc =
        parse(r#"<html><head><title>My Page</title></head><body><p>Visible</p></body></html>"#);
    assert!(doc_text(&doc).contains("Visible"));
    assert!(!doc_text(&doc).contains("My Page"));
}

#[test]
fn html_title_content_suppressed() {
    // TitleContentSuppressed: title text not in box tree or text buffer.
    let doc = parse("<title>Secret Title</title><p>Hello</p>");
    assert!(!doc_text(&doc).contains("Secret Title"));
    assert!(doc_text(&doc).contains("Hello"));
    let found = find_box(&doc.root, &|b: &WebCore| b.text.contains("Secret Title"));
    assert!(found.is_none());
}

#[test]
fn html_script_content_suppressed() {
    // ScriptContentSuppressed: script text not in rendered output.
    let doc =
        parse(r#"<html><head><script>var x = 1;</script></head><body><p>Text</p></body></html>"#);
    assert!(!doc_text(&doc).contains("var x"));
    assert!(doc_text(&doc).contains("Text"));
}

#[test]
fn html_noscript_content_suppressed() {
    // NoscriptContentSuppressed: noscript text not in rendered output.
    let doc = parse(
        r#"<html><head><noscript>Enable JS</noscript></head><body><p>Text</p></body></html>"#,
    );
    assert!(!doc_text(&doc).contains("Enable JS"));
}

#[test]
fn html_meta_charset_does_not_create_box() {
    // MetaCharsetDoesNotCreateBox: no meta box in tree.
    let doc = parse(r#"<html><head><meta charset="utf-8"></head><body><p>Text</p></body></html>"#);
    let meta = find_box(&doc.root, &|b: &WebCore| b.tag == "meta");
    assert!(meta.is_none());
}

#[test]
fn html_meta_viewport_ignored() {
    // MetaViewportIgnored: viewport meta creates no box.
    let doc = parse(
        r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><p>Text</p></body></html>"#,
    );
    let meta = find_box(&doc.root, &|b: &WebCore| b.tag == "meta");
    assert!(meta.is_none());
    assert!(doc_text(&doc).contains("Text"));
}

#[test]
fn html_link_tag_does_not_create_box() {
    // LinkTagDoesNotCreateBox: link element not in box tree.
    let doc = parse(
        r#"<html><head><link rel="stylesheet" href="style.css"></head><body><p>Text</p></body></html>"#,
    );
    let link = find_box(&doc.root, &|b: &WebCore| b.tag == "link");
    assert!(link.is_none());
}

#[test]
fn html_multiple_meta_tags_handled() {
    // MultipleMetaTagsHandled: multiple meta tags, none leak content.
    let doc = parse(
        r#"<html><head><meta charset="utf-8"><meta name="description" content="A test page"><meta name="viewport" content="width=device-width"></head><body><p>Content</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert!(doc_text(&doc).contains("Content"));
    assert!(!doc_text(&doc).contains("A test page"));
}

#[test]
fn html_style_block_collects_rules() {
    // StyleBlockCollectsRules: three rules in stylesheet.
    let doc = parse(
        r#"<html><head><style>p { color: red; } .highlight { background: yellow; } h1 { font-size: 24pt; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 3);
}

#[test]
fn html_multiple_style_blocks_merge() {
    // MultipleStyleBlocksMerge: rules from 3 style blocks all collected; .blue applied.
    let doc = parse(
        r#"<html><head><style>p { color: red; }</style><style>.blue { color: blue; }</style><style>h1 { font-size: 20pt; }</style></head><body><p class="blue">Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 3);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color.b, 255);
}

#[test]
fn html_head_before_body_order() {
    // HeadBeforeBodyOrder: style in head applied to body content.
    let doc = parse(
        r#"<html><head><style>p { color: green; }</style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color.g, 128); // CSS "green" = #008000
}

#[test]
fn html_style_in_body_still_works() {
    // StyleInBodyStillWorks: <style> in <body> still parsed.
    let doc = parse(r#"<body><style>p { color: red; }</style><p>Text</p></body>"#);
    assert!(doc.stylesheet.rules.len() >= 1);
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color.r, 255);
}

// ============================================================
// Title extraction
// ============================================================

#[test]
fn html_title_extracted() {
    // TitleExtracted: title field populated from <title> tag.
    let doc = parse(
        r#"<html><head><title>My Page Title</title></head><body><p>Content</p></body></html>"#,
    );
    assert_eq!(doc.title, "My Page Title");
}

#[test]
fn html_title_extracted_trimmed() {
    // TitleExtractedTrimmed: whitespace stripped from title.
    let doc = parse(r#"<html><head><title>  Spaces  </title></head><body></body></html>"#);
    assert_eq!(doc.title, "Spaces");
}

#[test]
fn html_title_empty_when_missing() {
    // TitleEmptyWhenMissing: no <title> → empty string.
    let doc = parse("<p>No title here</p>");
    assert!(doc.title.is_empty());
}

#[test]
fn html_title_not_in_text() {
    // TitleNotInText: title extracted but not in rendered text.
    let doc =
        parse(r#"<html><head><title>Secret</title></head><body><p>Visible</p></body></html>"#);
    assert_eq!(doc.title, "Secret");
    assert!(!doc_text(&doc).contains("Secret"));
    assert!(doc_text(&doc).contains("Visible"));
}

// ============================================================
// <details> / <summary> semantic elements
// ============================================================

#[test]
fn html_details_closed_hides_content() {
    // DetailsClosedHidesContent: non-summary children are display:none when closed.
    let doc = parse(r#"<details><summary>Click me</summary><p>Hidden content</p></details>"#);
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    let details = details.unwrap();
    let summary = find_box(details, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_ne!(summary.unwrap().style.display, Display::None);
    let p = find_box(details, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.display, Display::None);
}

#[test]
fn html_details_open_shows_content() {
    // DetailsOpenShowsContent: <details open> makes children visible.
    let doc = parse(r#"<details open><summary>Click me</summary><p>Visible content</p></details>"#);
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    let details = details.unwrap();
    let p = find_box(details, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_ne!(p.unwrap().style.display, Display::None);
}

#[test]
fn html_summary_is_list_item() {
    // SummaryIsListItem: summary has display:list-item.
    let doc = parse(r#"<details><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(summary.unwrap().style.display, Display::ListItem);
}

#[test]
fn html_summary_disclosure_marker_closed() {
    // SummaryDisclosureMarkerClosed: Disclosure variant used for closed state.
    // Note: Rust uses a single Disclosure variant; C++ had DisclosureClosed/Open.
    let doc = parse(r#"<details><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(
        summary.unwrap().style.list_style_type,
        ListStyleType::Disclosure
    );
}

#[test]
fn html_summary_disclosure_marker_open() {
    // SummaryDisclosureMarkerOpen: same Disclosure variant for open state.
    let doc = parse(r#"<details open><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(
        summary.unwrap().style.list_style_type,
        ListStyleType::Disclosure
    );
}

#[test]
fn html_details_summary_text_rendered() {
    // DetailsSummaryTextRendered: summary text visible in output.
    // Note: doc.text not available — use text_content().
    let doc = parse(r#"<details><summary>FAQ</summary><p>Answer here</p></details>"#);
    assert!(doc_text(&doc).contains("FAQ"));
}

#[test]
fn html_details_multiple_children() {
    // DetailsMultipleChildren: all non-summary children hidden when closed.
    let doc = parse(
        r#"<details><summary>More info</summary><p>Para 1</p><p>Para 2</p><div>Div content</div></details>"#,
    );
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    let details = details.unwrap();
    let hidden_count = details
        .children
        .iter()
        .filter(|ch| ch.tag != "summary" && ch.style.display == Display::None)
        .count();
    assert_eq!(hidden_count, 3);
}

#[test]
fn html_details_nested_in_body() {
    // DetailsNestedInBody: details works inside body with other elements.
    // SKIP doc.text.Contains checks — use text_content().
    let doc = parse(
        r#"<body><h1>Page</h1><details><summary>Show</summary><p>Hidden</p></details><p>After</p></body>"#,
    );
    assert!(doc_text(&doc).contains("Page"));
    assert!(doc_text(&doc).contains("Show"));
    assert!(doc_text(&doc).contains("After"));
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
}

// ============================================================
// HTML Attributes Map
// ============================================================

#[test]
fn html_attributes_map_populated() {
    // AttributesMapPopulated: id, class, title all in attributes map.
    // C++ uses b.id — Rust uses b.attributes.get("id").
    let doc = parse(r##"<div id="test" class="foo" title="tip">content</div>"##);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|v| v == "test").unwrap_or(false)
    });
    assert!(div.is_some());
    let div = div.unwrap();
    assert_eq!(div.attributes.get("id").map(|s| s.as_str()), Some("test"));
    assert_eq!(div.attributes.get("class").map(|s| s.as_str()), Some("foo"));
    assert_eq!(div.attributes.get("title").map(|s| s.as_str()), Some("tip"));
}

#[test]
fn html_attributes_map_custom_data() {
    // AttributesMapCustomData: data-* attribute preserved.
    let doc = parse(r#"<div data-custom="value123">content</div>"#);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.contains_key("data-custom")
    });
    assert!(div.is_some());
    assert_eq!(
        div.unwrap()
            .attributes
            .get("data-custom")
            .map(|s| s.as_str()),
        Some("value123")
    );
}

#[test]
fn html_attributes_map_multiple() {
    // AttributesMapMultiple: role, aria-label, tabindex preserved.
    let doc = parse(r##"<div id="x" role="button" aria-label="close" tabindex="0">X</div>"##);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|v| v == "x").unwrap_or(false)
    });
    assert!(div.is_some());
    let div = div.unwrap();
    assert_eq!(
        div.attributes.get("role").map(|s| s.as_str()),
        Some("button")
    );
    assert_eq!(
        div.attributes.get("aria-label").map(|s| s.as_str()),
        Some("close")
    );
    assert_eq!(
        div.attributes.get("tabindex").map(|s| s.as_str()),
        Some("0")
    );
}

#[test]
fn html_attributes_map_inline_element() {
    // AttributesMapInlineElement: data-type on <span> preserved.
    // C++ uses ParseHTML (no stylesheet) — Rust parse_html includes cascade, same result.
    let doc = parse(r#"<p><span data-type="highlight">text</span></p>"#);
    let span = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "span" && b.attributes.contains_key("data-type")
    });
    assert!(span.is_some());
    assert_eq!(
        span.unwrap()
            .attributes
            .get("data-type")
            .map(|s| s.as_str()),
        Some("highlight")
    );
}

#[test]
fn html_attributes_map_img() {
    // AttributesMapImg: src and alt preserved on img box.
    let doc = parse(r##"<div><img src="test.jpg" alt="photo" width="100"></div>"##);
    let img = find_box(&doc.root, &|b: &WebCore| b.tag == "img");
    assert!(img.is_some());
    let img = img.unwrap();
    assert_eq!(img.attributes.get("alt").map(|s| s.as_str()), Some("photo"));
    assert_eq!(
        img.attributes.get("src").map(|s| s.as_str()),
        Some("test.jpg")
    );
}

#[test]
fn html_attributes_map_boolean_attr() {
    // AttributesMapBooleanAttr: boolean "open" attribute in map.
    let doc = parse(r#"<details open><summary>Title</summary>Body</details>"#);
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    assert!(details.unwrap().attributes.contains_key("open"));
}

#[test]
fn html_attributes_map_css_selector() {
    // AttributesMapCSSSelector: [data-active] CSS selector matches attribute in map.
    let doc = parse(
        r##"<style>[data-active] { color: red; }</style><div data-active="true">Active</div>"##,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.contains_key("data-active")
    });
    assert!(div.is_some());
    assert_eq!(div.unwrap().style.color, Color::rgb(255, 0, 0));
}
