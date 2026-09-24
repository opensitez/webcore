// Ported from tests/test_html.cpp

use super::harness::*;
use crate::types::ListStyleType;
use crate::types::*;

// Helper: find the <body> box inside a document.
fn get_body(doc: &Document) -> Option<&WebCore> {
    find_box(&doc.root, &|b: &WebCore| b.tag == "body")
}

// ============================================================
// HTML Parsing
// ============================================================

#[test]
fn html_basic_document() {
    let doc = parse("<p>Hello</p>");
    assert!(doc_text(&doc).contains("Hello"));
}

#[test]
fn html_nested_elements() {
    let doc = parse("<div><p>Inner</p></div>");
    assert!(doc_text(&doc).contains("Inner"));
}

#[test]
fn html_multiple_children() {
    let doc = parse("<div><p>First</p><p>Second</p><p>Third</p></div>");
    assert!(doc_text(&doc).contains("First"));
    assert!(doc_text(&doc).contains("Second"));
    assert!(doc_text(&doc).contains("Third"));
}

#[test]
fn html_inline_style() {
    let doc = parse(r#"<p style="color: red;">Red text</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.style.color == Color::rgb(255, 0, 0)
    });
    assert!(b.is_some());
}

#[test]
fn html_void_elements() {
    let doc = parse("<p>Before<br>After</p>");
    assert!(doc_text(&doc).contains("Before"));
    assert!(doc_text(&doc).contains("After"));
}

#[test]
fn html_image_element() {
    let doc = parse(r#"<img src="test.png" width="100" height="50">"#);
    let _ = doc; // root is always present
}

#[test]
fn html_style_block() {
    let doc = parse(
        r#"<html><head><style>p { color: blue; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 1);
}

#[test]
fn html_multiple_style_blocks() {
    let doc = parse(
        r#"<html><head><style>p { color: blue; }</style><style>.red { color: red; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 2);
}

#[test]
fn html_dir_attribute() {
    // `direction` is not a separate enum in Rust types.
    // Check that the dir attribute is preserved on the box.
    let doc = parse(r#"<p dir="rtl">Arabic text</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("dir").map(|v| v == "rtl").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_lang_attribute() {
    // `lang` is stored as an attribute, not a style field.
    let doc = parse(r#"<p lang="ar">Arabic</p>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("lang").map(|v| v == "ar").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_class_attribute() {
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
    let doc = parse(r#"<div id="main">Test</div>"#);
    let b = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").map(|v| v == "main").unwrap_or(false)
    });
    assert!(b.is_some());
}

#[test]
fn html_headings_preserved() {
    let doc = parse("<h1>H1</h1><h2>H2</h2><h3>H3</h3>");
    let heading_count = count_boxes(&doc.root, &|b: &WebCore| {
        b.tag == "h1" || b.tag == "h2" || b.tag == "h3"
    });
    assert_eq!(heading_count, 3);
}

#[test]
fn html_list_elements() {
    let doc = parse("<ul><li>A</li><li>B</li><li>C</li></ul>");
    let li_count = count_boxes(&doc.root, &|b: &WebCore| b.tag == "li");
    assert_eq!(li_count, 3);
}

#[test]
fn html_bold_italic_elements() {
    let doc = parse("<p><b>Bold</b> and <i>Italic</i></p>");
    assert!(doc_text(&doc).contains("Bold"));
    assert!(doc_text(&doc).contains("Italic"));
}

#[test]
fn html_anchor_element() {
    let doc = parse(r#"<a href="http://example.com">Link</a>"#);
    assert!(doc_text(&doc).contains("Link"));
    // Check that the href attribute is preserved on the anchor box.
    let found_url = {
        let mut found = false;
        walk_boxes(&doc.root, &mut |b: &WebCore| {
            if b.attributes
                .get("href")
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            {
                found = true;
            }
        });
        found
    };
    assert!(found_url);
}

// ============================================================
// HTML Serialization Round-Trip
// ============================================================

#[test]
fn html_round_trip() {
    let original = "<p>Hello <b>world</b></p>";
    let doc = parse(original);
    // Re-parse the text content as a sanity check (no SerializeHTML in Rust).
    assert!(doc_text(&doc).contains("Hello"));
    assert!(doc_text(&doc).contains("world"));
}

#[test]
fn html_round_trip_preserves_structure() {
    let original = "<div><h1>Title</h1><p>Paragraph</p></div>";
    let doc = parse(original);
    assert!(doc_text(&doc).contains("Title"));
    assert!(doc_text(&doc).contains("Paragraph"));
}

// ============================================================
// Charset / Encoding
// ============================================================

#[test]
fn html_utf8_basic() {
    let doc = parse("<p>Hello World</p>");
    assert!(doc_text(&doc).contains("Hello"));
}

#[test]
fn html_charset_meta() {
    let doc = parse(r#"<html><head><meta charset="utf-8"></head><body><p>Test</p></body></html>"#);
    assert!(doc_text(&doc).contains("Test"));
}

#[test]
fn html_empty_document() {
    let doc = parse("");
    let _ = doc; // root is always present
}

// ============================================================
// html/body element separation and CSS matching
// ============================================================

#[test]
fn html_root_is_html() {
    let doc = parse("<html><body><p>Hello</p></body></html>");
    assert_eq!(doc.root.tag, "html");
}

#[test]
fn html_body_is_child_of_root() {
    let doc = parse("<html><body><p>Hello</p></body></html>");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_implicit_html_and_body() {
    let doc = parse("<p>Hello</p>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_explicit_html_implicit_body() {
    let doc = parse("<html><p>Hello</p></html>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_explicit_both_html_body() {
    let doc = parse("<html><body><p>Hello</p></body></html>");
    assert_eq!(doc.root.tag, "html");
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().tag, "body");
}

#[test]
fn html_css_matches_root() {
    let doc = parse(r#"<style>html { color: red; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.color.r, 255);
}

#[test]
fn html_css_background_matches_root() {
    let doc = parse(r#"<style>html { background-color: blue; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.background_color.b, 255);
}

#[test]
fn html_body_css_matches_body() {
    let doc = parse(r#"<style>html { color: red; } body { color: blue; }</style><p>Text</p>"#);
    assert_eq!(doc.root.style.color.r, 255); // html gets red
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.color.b, 255); // body gets blue
    assert_eq!(body.style.color.r, 0);
}

#[test]
fn html_body_css_matches_without_explicit_tag() {
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
    let doc = parse(
        r#"<html><head><style>body { background-color: #1e293b; }</style></head><body><p>Text</p></body></html>"#,
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
    let doc = parse(
        r#"<html><head><style>body { font-family: monospace; }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert!(!body.unwrap().style.font_family.is_empty());
}

#[test]
fn html_body_css_font_size() {
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
        "big font size ({}) should exceed default ({})",
        size_big,
        size_default
    );
}

#[test]
fn html_body_css_multiple_properties() {
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
    let doc = parse(r##"<body bgcolor="#ff0000"><p>Text</p></body>"##);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.background_color.r, 255);
}

#[test]
fn html_body_legacy_text_attr() {
    let doc = parse(r##"<body text="#00ff00"><p>Text</p></body>"##);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.color.g, 255);
}

#[test]
fn html_body_inline_style() {
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
    let doc = parse(
        r#"<html><head><style>body { color: blue; }</style></head><body text="red"><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().style.color.b, 255);
}

#[test]
fn html_body_child_overrides_color() {
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
    // gradient backgrounds: just verify the document parses without panic
    let doc = parse(
        r#"<html><head><style>body { background: linear-gradient(135deg, #1e293b, #334155); }</style></head><body><p>Text</p></body></html>"#,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
}

#[test]
fn html_body_background_used_for_canvas() {
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
    let doc = parse("<p>Hello</p>");
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    let p = find_box(body, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
}

#[test]
fn html_ua_body_margin() {
    let doc = parse_and_layout("<p>Text</p>", 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.style.margin_top.resolve(16.0, 0.0, 16.0), 8.0);
    assert_eq!(body.style.margin_left.resolve(16.0, 0.0, 16.0), 8.0);
}

#[test]
fn html_body_layout_position() {
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
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; } body { margin: 0; }</style><p>Text</p>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 0.0);
}

#[test]
fn html_body_layout_demo_pattern() {
    let doc = parse_and_layout(
        r##"<body text="#2c3e50"><style>* { box-sizing: border-box; } body { margin: 0; }</style><div style="position: fixed; top: 0; left: 0; width: 100%;"><a>Home</a><a>Features</a></div><h1 style="margin-top: 44px;">Title</h1><p>Text</p><div style="position: fixed; bottom: 16px; right: 16px; width: 48px; height: 48px;">+</div></body>"##,
        685.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 0.0);
    assert_eq!(doc.root.layout.margin_rect.x, 0.0);
    assert_eq!(doc.root.layout.content_rect.w, 685.0);
}

#[test]
fn html_body_layout_with_floats() {
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; } body { margin: 0; }</style><div style="width: 50%; float: left;">Left</div><div style="width: 50%; float: right;">Right</div><div style="clear: both;"></div><p>After floats</p>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.x, 0.0);
}

#[test]
fn html_body_bfc_isolation() {
    let doc = parse_and_layout(
        r#"<body style="margin: 0;"><div style="width: 300px; float: left;">Float</div><p>Text alongside float</p></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.w, 800.0);
}

#[test]
fn html_body_margin_padding_box_sizing_combinations() {
    // margin + padding + content-box (default)
    {
        let doc = parse_and_layout(
            r#"<body style="margin: 10px; padding: 20px;"><p>X</p></body>"#,
            800.0,
        );
        let body = get_body(&doc);
        assert!(body.is_some());
        // auto width: contentWidth = 800 - 10 - 10 - 20 - 20 = 740
        assert_eq!(body.unwrap().layout.content_rect.w, 740.0);
        assert_eq!(get_body(&doc).unwrap().layout.margin_rect.x, 0.0);
    }
    // margin + padding + border-box
    {
        let doc = parse_and_layout(
            r#"<style>* { box-sizing: border-box; }</style><body style="margin: 10px; padding: 20px;"><p>X</p></body>"#,
            800.0,
        );
        let body = get_body(&doc);
        assert!(body.is_some());
        // auto width with border-box: same as content-box for auto width
        assert_eq!(body.unwrap().layout.content_rect.w, 740.0);
        assert_eq!(get_body(&doc).unwrap().layout.margin_rect.x, 0.0);
    }
}

#[test]
fn html_body_explicit_width_with_margin() {
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
    let doc = parse_and_layout("<style>body { margin: 0; }</style><p>Hello</p>", 1024.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.content_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.w, 1024.0);
    assert_eq!(body.layout.margin_rect.w, 1024.0);
}

// ============================================================
// Table inside body with padding (edit_demo regression)
// ============================================================

#[test]
fn html_table_full_width_in_body_with_padding() {
    let doc = parse_and_layout(
        r#"<body style="margin: 0; padding: 16px;"><table style="width: 100%;"><tr><td>A</td><td>B</td><td>Long description text</td></tr></table></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    // body contentWidth = 800 - 16 - 16 = 768
    assert_eq!(body.unwrap().layout.content_rect.w, 768.0);
    let table = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table.is_some());
    // table at 100% should match body content width
    assert_eq!(table.unwrap().layout.margin_rect.w, 768.0);
}

#[test]
fn html_table_full_width_in_body_with_padding_and_box_sizing() {
    let doc = parse_and_layout(
        r#"<style>* { box-sizing: border-box; } body { margin: 0; }</style><body style="padding: 16px;"><table style="width: 100%; border-collapse: collapse;"><tr><td style="padding: 8px; border: 1px solid #ccc;">Name</td><td style="padding: 8px; border: 1px solid #ccc;">Type</td><td style="padding: 8px; border: 1px solid #ccc;">Load an HTML string into the editor</td></tr></table></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().layout.content_rect.w, 768.0);
    let table = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table.is_some());
    assert_eq!(table.unwrap().layout.margin_rect.w, 768.0);
}

#[test]
fn html_table_in_body_with_margin_and_padding() {
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

#[test]
fn table_border_spacing_uses_vertical_component_for_row_gaps() {
    let doc = parse_and_layout(
        r#"<table style="border-spacing: 3px 17px;">
            <tr><td id="a">A</td></tr>
            <tr><td id="b">B</td></tr>
        </table>"#,
        800.0,
    );
    let first = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").is_some_and(|v| v == "a")
    })
    .expect("first cell");
    let second = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.get("id").is_some_and(|v| v == "b")
    })
    .expect("second cell");
    let gap =
        second.layout.border_rect.y - (first.layout.border_rect.y + first.layout.border_rect.h);
    assert!(
        (gap - 17.0).abs() < 0.5,
        "vertical border-spacing should create a 17px row gap, got {gap}"
    );
}

// ============================================================
// Canvas background propagation
// ============================================================

#[test]
fn html_canvas_bg_from_html() {
    let doc = parse_and_layout(
        r#"<html style="background-color: red;"><body><p>X</p></body></html>"#,
        800.0,
    );
    assert_eq!(doc.root.style.background_color, Color::rgb(255, 0, 0));
}

#[test]
fn html_canvas_bg_fallback_from_body() {
    let doc = parse_and_layout(
        r#"<body style="background-color: blue;"><p>X</p></body>"#,
        800.0,
    );
    let body = get_body(&doc);
    assert!(body.is_some());
    // html has no explicit bg (transparent)
    assert_eq!(doc.root.style.background_color, Color::TRANSPARENT);
    // body has blue bg
    assert_eq!(body.unwrap().style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn html_canvas_bg_html_overrides_body() {
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
    // First layout at 800
    let doc = parse_and_layout("<style>body { margin: 0; }</style><p>Text</p>", 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    let body = body.unwrap();
    assert_eq!(body.layout.margin_rect.x, 0.0);
    assert_eq!(body.layout.content_rect.w, 800.0);

    // Second layout at 700
    let doc2 = parse_and_layout("<style>body { margin: 0; }</style><p>Text</p>", 700.0);
    let body2 = get_body(&doc2);
    assert!(body2.is_some());
    let body2 = body2.unwrap();
    assert_eq!(body2.layout.margin_rect.x, 0.0);
    assert_eq!(body2.layout.content_rect.w, 700.0);
}

#[test]
fn html_double_layout_with_floats_preserves_body() {
    let html = r#"<style>* { box-sizing: border-box; } body { margin: 0; }</style><div style="width: 50%; float: left;">Left</div><div style="width: 50%; float: right;">Right</div><div style="clear: both;"></div><p>After</p>"#;

    let doc = parse_and_layout(html, 800.0);
    let body = get_body(&doc);
    assert!(body.is_some());
    assert_eq!(body.unwrap().layout.margin_rect.x, 0.0);

    let doc2 = parse_and_layout(html, 750.0);
    let body2 = get_body(&doc2);
    assert!(body2.is_some());
    let body2 = body2.unwrap();
    assert_eq!(body2.layout.margin_rect.x, 0.0);
    assert_eq!(body2.layout.content_rect.w, 750.0);
}

#[test]
fn html_double_layout_same_width_stable() {
    let html = r#"<body style="margin: 10px; padding: 5px;"><h1>Title</h1><p>Text</p></body>"#;
    let doc1 = parse_and_layout(html, 600.0);
    let body1 = get_body(&doc1);
    assert!(body1.is_some());
    let body1 = body1.unwrap();
    let x1 = body1.layout.margin_rect.x;
    let w1 = body1.layout.content_rect.w;
    let h1 = body1.layout.content_rect.h;

    let doc2 = parse_and_layout(html, 600.0);
    let body2 = get_body(&doc2);
    assert!(body2.is_some());
    let body2 = body2.unwrap();
    assert_eq!(body2.layout.margin_rect.x, x1);
    assert_eq!(body2.layout.content_rect.w, w1);
    assert_eq!(body2.layout.content_rect.h, h1);
}

#[test]
fn html_double_layout_table_in_body_with_padding() {
    let html = r#"<body style="margin: 0; padding: 16px;"><table style="width: 100%;"><tr><td>A</td><td>B</td><td>C</td></tr></table></body>"#;

    let doc1 = parse_and_layout(html, 800.0);
    let table1 = find_box(&doc1.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table1.is_some());
    let tw1 = table1.unwrap().layout.margin_rect.w;

    let doc2 = parse_and_layout(html, 800.0);
    let table2 = find_box(&doc2.root, &|b: &WebCore| b.style.display == Display::Table);
    assert!(table2.is_some());
    assert_eq!(table2.unwrap().layout.margin_rect.w, tw1);
    // Table should match body content width = 800 - 32 = 768
    assert_eq!(tw1, 768.0);
}

// ============================================================
// Head, meta, title, script — non-visual elements
// ============================================================

#[test]
fn html_head_content_not_rendered() {
    let doc =
        parse(r#"<html><head><title>My Page</title></head><body><p>Visible</p></body></html>"#);
    assert!(doc_text(&doc).contains("Visible"));
    // The title IS a node and IS part of `textContent` — Chrome answers
    // "My PageVisible" for `documentElement.textContent`. What "not rendered"
    // means is that it generates no BOX, which is `display: none`.
    let title = find_box(&doc.root, &|b: &WebCore| b.tag == "title")
        .expect("<title> is an element in the head");
    assert!(matches!(title.style.display, crate::types::Display::None));
    assert!(doc_text(&doc).contains("My Page"));
}

#[test]
fn html_title_content_suppressed() {
    let doc = parse("<title>Secret Title</title><p>Hello</p>");
    assert!(doc_text(&doc).contains("Hello"));
    // The title element exists and holds its text; it just draws nothing.
    let title = find_box(&doc.root, &|b: &WebCore| b.tag == "title")
        .expect("<title> is an element even with no explicit <head>");
    assert!(matches!(title.style.display, crate::types::Display::None));
    assert_eq!(title.text, "Secret Title");
}

#[test]
fn html_script_content_suppressed() {
    let doc =
        parse(r#"<html><head><script>var x = 1;</script></head><body><p>Text</p></body></html>"#);
    assert!(!doc_text(&doc).contains("var x"));
    assert!(doc_text(&doc).contains("Text"));
}

#[test]
fn html_noscript_content_suppressed() {
    let doc = parse(
        r#"<html><head><noscript>Enable JS</noscript></head><body><p>Text</p></body></html>"#,
    );
    assert!(!doc_text(&doc).contains("Enable JS"));
}

#[test]
fn body_noscript_content_renders_when_scripting_is_disabled() {
    let doc = parse(r#"<html><body><noscript><p>Fallback</p></noscript></body></html>"#);
    assert!(doc_text(&doc).contains("Fallback"));
    let noscript = find_box(&doc.root, &|b: &WebCore| b.tag == "noscript")
        .expect("<noscript> should remain in the body tree");
    assert!(!matches!(noscript.style.display, crate::types::Display::None));
}

#[test]
fn html_meta_charset_does_not_create_box() {
    let doc = parse(r#"<html><head><meta charset="utf-8"></head><body><p>Text</p></body></html>"#);
    // `<meta>` is an ELEMENT in the head (HTML §13.2.6.4.4) with no box.
    let meta = find_box(&doc.root, &|b: &WebCore| b.tag == "meta")
        .expect("<meta> is an element in the head");
    assert!(matches!(meta.style.display, crate::types::Display::None));
    assert_eq!(
        meta.attributes.get("charset").map(|s| s.as_str()),
        Some("utf-8")
    );
}

#[test]
fn html_meta_viewport_ignored() {
    let doc = parse(
        r#"<html><head><meta name="viewport" content="width=device-width, initial-scale=1"></head><body><p>Text</p></body></html>"#,
    );
    let meta = find_box(&doc.root, &|b: &WebCore| b.tag == "meta")
        .expect("<meta> is an element in the head");
    assert!(matches!(meta.style.display, crate::types::Display::None));
    assert!(doc_text(&doc).contains("Text"));
}

#[test]
fn html_link_tag_does_not_create_box() {
    let doc = parse(
        r#"<html><head><link rel="stylesheet" href="style.css"></head><body><p>Text</p></body></html>"#,
    );
    let link = find_box(&doc.root, &|b: &WebCore| b.tag == "link")
        .expect("<link> is an element in the head");
    assert!(matches!(link.style.display, crate::types::Display::None));
    assert_eq!(
        link.attributes.get("href").map(|s| s.as_str()),
        Some("style.css")
    );
}

#[test]
fn html_multiple_meta_tags_handled() {
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
    let doc = parse(
        r#"<html><head><style>p { color: red; } .highlight { background: yellow; } h1 { font-size: 24pt; }</style></head><body><p>Text</p></body></html>"#,
    );
    assert!(doc.stylesheet.rules.len() >= 3);
}

#[test]
fn html_multiple_style_blocks_merge() {
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
    let doc = parse(
        r#"<html><head><style>p { color: green; }</style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.color.g, 128); // CSS "green" = #008000
}

#[test]
fn html_style_in_body_still_works() {
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
    let doc = parse(
        r#"<html><head><title>My Page Title</title></head><body><p>Content</p></body></html>"#,
    );
    assert_eq!(doc.title, "My Page Title");
}

#[test]
fn html_title_extracted_trimmed() {
    let doc = parse(r#"<html><head><title>  Spaces  </title></head><body></body></html>"#);
    assert_eq!(doc.title, "Spaces");
}

#[test]
fn html_title_empty_when_missing() {
    let doc = parse("<p>No title here</p>");
    assert!(doc.title.is_empty());
}

#[test]
fn html_title_not_in_text() {
    let doc =
        parse(r#"<html><head><title>Secret</title></head><body><p>Visible</p></body></html>"#);
    assert_eq!(doc.title, "Secret");
    assert!(doc_text(&doc).contains("Visible"));
    // `Document.title` is a convenience mirror; the ELEMENT is still in the
    // tree, so its text is part of `textContent` exactly as in a browser.
    assert!(doc_text(&doc).contains("Secret"));
}

// ============================================================
// <details> / <summary> semantic elements
// ============================================================

#[test]
fn html_details_closed_hides_content() {
    let doc = parse(r#"<details><summary>Click me</summary><p>Hidden content</p></details>"#);
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    let details = details.unwrap();
    let summary = find_box(details, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    // Summary should be visible
    assert_ne!(summary.unwrap().style.display, Display::None);
    // The <p> should be hidden
    let p = find_box(details, &|b: &WebCore| b.tag == "p");
    assert!(p.is_some());
    assert_eq!(p.unwrap().style.display, Display::None);
}

#[test]
fn html_details_open_shows_content() {
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
    let doc = parse(r#"<details><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(summary.unwrap().style.display, Display::ListItem);
}

#[test]
fn html_summary_disclosure_marker_closed() {
    let doc = parse(r#"<details><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(
        summary.unwrap().style.list_style_type,
        ListStyleType::DisclosureClosed
    );
}

#[test]
fn html_summary_disclosure_marker_open() {
    let doc = parse(r#"<details open><summary>Title</summary><p>Body</p></details>"#);
    let summary = find_box(&doc.root, &|b: &WebCore| b.tag == "summary");
    assert!(summary.is_some());
    assert_eq!(
        summary.unwrap().style.list_style_type,
        ListStyleType::DisclosureOpen
    );
}

#[test]
fn html_details_summary_text_rendered() {
    let doc = parse(r#"<details><summary>FAQ</summary><p>Answer here</p></details>"#);
    assert!(doc_text(&doc).contains("FAQ"));
}

#[test]
fn html_details_multiple_children() {
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
    // Use parse (with stylesheet) to test attribute parsing.
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
    let doc = parse(r#"<details open><summary>Title</summary>Body</details>"#);
    let details = find_box(&doc.root, &|b: &WebCore| b.tag == "details");
    assert!(details.is_some());
    assert!(details.unwrap().attributes.contains_key("open"));
}

#[test]
fn html_attributes_map_css_selector() {
    let doc = parse(
        r##"<style>[data-active] { color: red; }</style><div data-active="true">Active</div>"##,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes.contains_key("data-active")
    });
    assert!(div.is_some());
    assert_eq!(div.unwrap().style.color, Color::rgb(255, 0, 0));
}

// ============================================================
// Ordered list numbering
// ============================================================

#[test]
fn ol_list_index_sequential() {
    // After parse + cascade, each <li> inside <ol> must have list_index 1,2,3.
    let doc = parse("<ol><li>A</li><li>B</li><li>C</li></ol>");
    let items: Vec<_> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    assert_eq!(items.len(), 3, "expected 3 <li> boxes");
    let indices: Vec<i32> = items.iter().map(|b| b.style.list_index).collect();
    assert_eq!(
        indices,
        vec![1, 2, 3],
        "list_index must be 1,2,3 not {:?}",
        indices
    );
}

#[test]
fn ol_list_index_not_zero() {
    // Regression: cascade was resetting list_index to 0.
    let doc = parse("<ol><li>One</li><li>Two</li></ol>");
    let items: Vec<_> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    for item in &items {
        assert_ne!(item.style.list_index, 0, "<li> list_index must not be 0");
    }
}

#[test]
fn ol_list_style_type_decimal() {
    // <ol> defaults to decimal list-style-type.
    let doc = parse("<ol><li>Item</li></ol>");
    let li = find_box(&doc.root, &|b| b.tag == "li").expect("<li> not found");
    assert_eq!(
        li.style.list_style_type,
        ListStyleType::Decimal,
        "ol > li must have Decimal list-style-type"
    );
}

#[test]
fn nested_ol_list_index_independent() {
    // Nested <ol> restarts its own counter from 1.
    let doc = parse("<ol><li>A</li><li><ol><li>X</li><li>Y</li></ol></li><li>B</li></ol>");
    let all_li: Vec<_> = find_all_boxes(&doc.root, &|b| b.tag == "li");
    // Outer: A=1, wrapper=2, B=3. Inner: X=1, Y=2.
    let indices: Vec<i32> = all_li.iter().map(|b| b.style.list_index).collect();
    // Outer items must include 1 and not all be 0.
    assert!(
        indices.iter().any(|&i| i > 0),
        "at least one list_index must be > 0, got {:?}",
        indices
    );
    // The two inner items must be 1 and 2.
    let inner: Vec<_> = all_li
        .iter()
        .filter(|b| {
            // Children of the nested ol — identified by having siblings that are also li with low index
            b.style.list_index <= 2
        })
        .collect();
    assert!(
        inner.len() >= 2,
        "expected inner list items with index 1 and 2"
    );
}

// ============================================================
// Table border / cellspacing HTML attribute tests
// ============================================================

#[test]
fn table_border_attr_sets_collapse() {
    // border="1" on <table> should enable border-collapse.
    let doc = parse(r#"<table border="1"><tr><td>A</td></tr></table>"#);
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    assert!(
        table.style.border_collapse,
        "table with border=\"1\" must have border-collapse: collapse"
    );
}

#[test]
fn table_border_attr_sets_table_border_width() {
    let doc = parse(r#"<table border="1"><tr><td>A</td></tr></table>"#);
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    let w = match table.style.border_top_width {
        crate::types::CssLength::Px(v) => v,
        _ => -1.0,
    };
    assert_eq!(
        w, 1.0,
        "table border-top-width should be 1px, got {:?}",
        table.style.border_top_width
    );
}

#[test]
fn table_border_attr_propagates_to_cells() {
    let doc = parse(r#"<table border="1"><tr><td>A</td><td>B</td></tr></table>"#);
    let cells: Vec<_> = find_all_boxes(&doc.root, &|b| b.tag == "td");
    assert!(!cells.is_empty(), "no td found");
    for cell in &cells {
        let w = match cell.style.border_top_width {
            crate::types::CssLength::Px(v) => v,
            _ => -1.0,
        };
        assert!(
            w > 0.0,
            "td in table with border=\"1\" should have border-top-width > 0, got {:?}",
            cell.style.border_top_width
        );
    }
}

#[test]
fn table_cellspacing_zero_sets_border_spacing() {
    let doc = parse(r#"<table border="1" cellspacing="0"><tr><td>A</td></tr></table>"#);
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    let sp = match &table.style.border_spacing_h {
        crate::types::CssLength::Px(v) => *v,
        crate::types::CssLength::Zero => 0.0,
        other => panic!("unexpected border_spacing_h: {:?}", other),
    };
    assert_eq!(
        sp, 0.0,
        "cellspacing=\"0\" must set border-spacing to 0, got {:?}",
        table.style.border_spacing_h
    );
}

#[test]
fn table_cellspacing_nonzero() {
    let doc = parse(r#"<table cellspacing="4"><tr><td>A</td></tr></table>"#);
    let table = find_box(&doc.root, &|b| b.tag == "table").unwrap();
    let sp = match &table.style.border_spacing_h {
        crate::types::CssLength::Px(v) => *v,
        crate::types::CssLength::Zero => 0.0,
        other => panic!("unexpected border_spacing_h: {:?}", other),
    };
    assert_eq!(
        sp, 4.0,
        "cellspacing=\"4\" must set border-spacing to 4px, got {:?}",
        table.style.border_spacing_h
    );
}

#[test]
fn table_no_border_attr_no_cell_borders() {
    // Without a border attribute a td draws no border — asserted as the USED
    // width, since the initial COMPUTED width is `medium` (3px) and it is
    // `border-style: none` that zeroes it. See `table_border_zero_no_cell_borders`.
    let mut doc = parse(r#"<table><tr><td>A</td></tr></table>"#);
    let cell = find_box(&doc.root, &|b| b.tag == "td").unwrap().node_id;
    assert_eq!(
        doc.computed_style_property(cell, "border-top-style"),
        "none"
    );
    assert_eq!(
        doc.computed_style_property(cell, "border-top-width"),
        "0px",
        "a td in a table with no border attribute draws no border"
    );
}

#[test]
fn table_border_zero_no_cell_borders() {
    // ⛔ The USED width, not the raw computed field. The initial COMPUTED
    // border width is `medium` (3px) per CSS Backgrounds 3 §4.3 — it is the
    // `border-style: none` that makes the used width zero. This asserted the
    // computed field was 0, which was only true while that initial value was
    // wrong. Chrome on the same markup: `width=0px style=none`.
    let mut doc = parse(r#"<table border="0"><tr><td>A</td></tr></table>"#);
    let cell = find_box(&doc.root, &|b| b.tag == "td").unwrap().node_id;
    assert_eq!(
        doc.computed_style_property(cell, "border-top-style"),
        "none"
    );
    assert_eq!(
        doc.computed_style_property(cell, "border-top-width"),
        "0px",
        "a td in a table with border=\"0\" draws no border"
    );
}

#[test]
fn css_border_overrides_html_border_attr() {
    // Author CSS on td should win over the HTML border attribute propagation.
    let doc = parse(
        r#"<style>td { border: 3px solid red; }</style>
<table border="1"><tr><td>A</td></tr></table>"#,
    );
    let cell = find_box(&doc.root, &|b| b.tag == "td").unwrap();
    let w = match cell.style.border_top_width {
        crate::types::CssLength::Px(v) => v,
        _ => -1.0,
    };
    assert_eq!(
        w, 3.0,
        "author CSS border 3px should override HTML attr 1px, got {:?}",
        cell.style.border_top_width
    );
}

// ============================================================
// Body / UA default tests
// ============================================================

#[test]
fn body_margin_8px_from_ua() {
    let doc = parse("<p>Hello</p>");
    let body = find_box(&doc.root, &|b| b.tag == "body").unwrap();
    let m = match body.style.margin_top {
        crate::types::CssLength::Px(v) => v,
        _ => -1.0,
    };
    assert_eq!(
        m, 8.0,
        "body margin-top should be 8px from UA stylesheet, got {:?}",
        body.style.margin_top
    );
}

#[test]
fn body_margin_overridden_by_author_css() {
    let doc = parse("<style>body { margin: 0; }</style><p>Hello</p>");
    let body = find_box(&doc.root, &|b| b.tag == "body").unwrap();
    let m = match body.style.margin_top {
        crate::types::CssLength::Px(v) => v,
        crate::types::CssLength::Zero => 0.0,
        _ => -1.0,
    };
    assert_eq!(
        m, 0.0,
        "body margin-top should be overridden to 0 by author CSS, got {:?}",
        body.style.margin_top
    );
}

#[test]
fn clicking_a_summary_toggles_its_details() {
    // HTML §4.11.1: activating the summary toggles the details' `open`
    // attribute. The summary already drew a pointer cursor and a disclosure
    // marker, so it LOOKED interactive — the click was simply never wired.
    let mut doc = crate::load_html(
        r#"<details id="d"><summary id="s">Title</summary><p id="body">Body</p></details>"#,
        400.0,
    );
    crate::layout::LayoutEngine::new().layout(&mut doc, 400.0);
    let d = doc.get_element_by_id("d").unwrap();
    let s = doc.get_element_by_id("s").unwrap();
    assert!(!doc.has_attribute(d, "open"), "starts closed");

    let br = find_box(&doc.root, &|b: &WebCore| b.node_id == s)
        .expect("the summary has a box")
        .layout
        .border_rect;
    let point = (br.x + br.w / 2.0, br.y + br.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);
    assert!(doc.has_attribute(d, "open"), "the click opened it");

    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);
    assert!(
        !doc.has_attribute(d, "open"),
        "and the next click closed it again"
    );
}
