// Serialization tests – ported from cpptests/test_serialization.cpp
use webcore::html::serialize_html;
use webcore::types::*;
use webcore::{load_html, parse_html};

/// Parse → Serialize helper
fn serialize(input: &str) -> String {
    let doc = parse_html(input);
    serialize_html(&doc)
}

/// Parse → Serialize → Parse round-trip helper
fn round_trip(input: &str) -> Document {
    let html = serialize(input);
    parse_html(&html)
}

// ============================================================
// Plain Text
// ============================================================

#[test]
fn plain_text_preserved() {
    let html = serialize("<p>Hello World</p>");
    assert!(html.contains("Hello World"));
}

#[test]
fn multi_paragraph_text() {
    let html = serialize("<p>First</p><p>Second</p>");
    assert!(html.contains("First"));
    assert!(html.contains("Second"));
}

// ============================================================
// Tags
// ============================================================

#[test]
fn nested_div_structure() {
    let html = serialize("<div><div>Inner</div></div>");
    assert!(html.contains("<div"));
    assert!(html.contains("Inner"));
}

#[test]
fn heading_tags() {
    let html = serialize("<h1>Title</h1>");
    assert!(html.contains("h1"));
    assert!(html.contains("Title"));
}

#[test]
fn paragraph_tag() {
    let html = serialize("<p>Paragraph text</p>");
    assert!(html.contains("<p"));
    assert!(html.contains("</p>"));
}

// ============================================================
// Inline formatting
// ============================================================

#[test]
fn bold_round_trip() {
    let html = serialize("<p><b>Bold</b></p>");
    assert!(html.contains("Bold"));
}

#[test]
fn italic_round_trip() {
    let html = serialize("<p><i>Italic</i></p>");
    assert!(html.contains("Italic"));
}

// ============================================================
// Attributes
// ============================================================

#[test]
fn link_href_preserved() {
    let html = serialize("<a href=\"http://example.com\">Link</a>");
    assert!(html.contains("href"));
    assert!(html.contains("example.com"));
}

#[test]
fn id_preserved() {
    let html = serialize("<div id=\"main\">Content</div>");
    assert!(html.contains("id=\"main\""));
}

#[test]
fn class_preserved() {
    let html = serialize("<div class=\"highlight\">Content</div>");
    assert!(html.contains("class=\"highlight\""));
}

#[test]
fn img_src_preserved() {
    let html = serialize("<img src=\"test.jpg\">");
    assert!(html.contains("src=\"test.jpg\""));
}

#[test]
fn img_is_void() {
    let html = serialize("<img src=\"test.jpg\">");
    assert!(!html.contains("</img>"));
}

// ============================================================
// Table Structure
// ============================================================

#[test]
fn table_structure() {
    let html = serialize("<table><tr><td>A</td><td>B</td></tr></table>");
    assert!(html.contains("<table"));
    assert!(html.contains("<tr"));
    assert!(html.contains("<td"));
}

#[test]
fn colspan_preserved() {
    let html = serialize("<table><tr><td colspan=\"2\">Span</td></tr></table>");
    assert!(html.contains("colspan=\"2\""));
}

#[test]
fn rowspan_preserved() {
    let html = serialize("<table><tr><td rowspan=\"3\">Tall</td></tr></table>");
    assert!(html.contains("rowspan=\"3\""));
}

// ============================================================
// Inline Style Round-Trip
// ============================================================

#[test]
fn width_style_preserved() {
    let html = serialize("<div style=\"width: 200px;\">Content</div>");
    assert!(html.contains("width"));
    assert!(html.contains("200"));
}

#[test]
fn background_color_style_preserved() {
    let html = serialize("<div style=\"background-color: rgb(255, 0, 0);\">Red</div>");
    assert!(html.contains("background"));
}

#[test]
fn color_style_preserved() {
    let html = serialize("<div style=\"color: blue;\">Blue</div>");
    assert!(html.contains("color"));
}

#[test]
fn margin_style_preserved() {
    let html = serialize("<div style=\"margin: 10px;\">Content</div>");
    assert!(html.contains("margin"));
}

#[test]
fn padding_style_preserved() {
    let html = serialize("<div style=\"padding: 5px;\">Content</div>");
    assert!(html.contains("padding"));
}

#[test]
fn border_style_preserved() {
    let html = serialize("<div style=\"border: 1px solid black;\">Content</div>");
    assert!(html.contains("border"));
}

#[test]
fn text_align_style_preserved() {
    let html = serialize("<p style=\"text-align: center;\">Center</p>");
    assert!(html.contains("text-align"));
}

#[test]
fn position_style_preserved() {
    let html = serialize("<div style=\"position: absolute;\">Abs</div>");
    assert!(html.contains("position"));
}

#[test]
fn float_style_preserved() {
    let html = serialize("<div style=\"float: left;\">Float</div>");
    assert!(html.contains("float"));
}

#[test]
fn z_index_style_preserved() {
    let html = serialize("<div style=\"z-index: 5;\">Layered</div>");
    assert!(html.contains("z-index"));
}

#[test]
fn opacity_style_preserved() {
    let html = serialize("<div style=\"opacity: 0.5;\">Half</div>");
    assert!(html.contains("opacity"));
}

#[test]
fn flex_style_preserved() {
    let html = serialize("<div style=\"display: flex;\">Flex</div>");
    assert!(html.contains("flex"));
}

#[test]
fn object_fit_style_preserved() {
    let html = serialize("<img style=\"object-fit: cover;\" src=\"img.jpg\">");
    assert!(html.contains("object-fit"));
}

// ============================================================
// Stylesheet Round-Trip
// ============================================================

#[test]
fn stylesheet_preserved() {
    let html = serialize(
        "<html><head><style>.test { color: red; }</style></head>\
         <body><div class=\"test\">Styled</div></body></html>",
    );
    assert!(html.contains("color"));
}

#[test]
fn stylesheet_heading_with_color() {
    let html = serialize(
        "<html><head><style>h1 { color: blue; }</style></head>\
         <body><h1>Title</h1></body></html>",
    );
    assert!(html.contains("h1"));
    assert!(html.contains("color"));
}

#[test]
fn selector_with_class() {
    let html = serialize(
        "<html><head><style>div.box { background-color: yellow; }</style></head>\
         <body><div class=\"box\">Box</div></body></html>",
    );
    assert!(html.contains(".box"));
}

#[test]
fn selector_with_id() {
    let html = serialize(
        "<html><head><style>#header { font-size: 24px; }</style></head>\
         <body><div id=\"header\">Header</div></body></html>",
    );
    assert!(html.contains("#header"));
}

#[test]
fn child_combinator_selector() {
    let html = serialize(
        "<html><head><style>div > p { color: green; }</style></head>\
         <body><div><p>Text</p></div></body></html>",
    );
    assert!(html.contains(">"));
}

// ============================================================
// Lists
// ============================================================

#[test]
fn unordered_list() {
    let html = serialize("<ul><li>A</li><li>B</li></ul>");
    assert!(html.contains("<ul"));
    assert!(html.contains("<li"));
    assert!(html.contains("</ul>"));
}

#[test]
fn ordered_list() {
    let html = serialize("<ol><li>First</li><li>Second</li></ol>");
    assert!(html.contains("<ol"));
    assert!(html.contains("<li"));
}

// ============================================================
// Void Elements
// ============================================================

#[test]
fn hr_is_void() {
    let html = serialize("<hr>");
    assert!(html.contains("<hr"));
    assert!(!html.contains("</hr>"));
}

// ============================================================
// Double Round-Trip Fidelity
// ============================================================

#[test]
fn double_round_trip() {
    let original = "<div id=\"main\"><p>Hello <b>world</b></p></div>";
    let doc1 = parse_html(original);
    let ser1 = serialize_html(&doc1);
    let doc2 = parse_html(&ser1);
    let ser2 = serialize_html(&doc2);
    assert_eq!(ser1, ser2);
}

#[test]
fn double_round_trip_with_styles() {
    let original = "<div style=\"width: 200px; background-color: #ff0000;\">\
         <p style=\"text-align: center;\">Styled</p></div>";
    let doc1 = parse_html(original);
    let ser1 = serialize_html(&doc1);
    let doc2 = parse_html(&ser1);
    let ser2 = serialize_html(&doc2);
    assert_eq!(ser1, ser2);
}

#[test]
fn double_round_trip_table() {
    let original = "<table><tr><td colspan=\"2\">Header</td></tr>\
         <tr><td>A</td><td>B</td></tr></table>";
    let doc1 = parse_html(original);
    let ser1 = serialize_html(&doc1);
    let doc2 = parse_html(&ser1);
    let ser2 = serialize_html(&doc2);
    assert_eq!(ser1, ser2);
}

#[test]
fn double_round_trip_stylesheet() {
    let original = "<html><head><style>\
         .box { color: red; margin: 10px; }\
         </style></head>\
         <body><div class=\"box\">Content</div></body></html>";
    let doc1 = parse_html(original);
    let ser1 = serialize_html(&doc1);
    let doc2 = parse_html(&ser1);
    let ser2 = serialize_html(&doc2);
    assert_eq!(ser1, ser2);
}

// ============================================================
// Edge Cases
// ============================================================

#[test]
fn empty_paragraph() {
    let html = serialize("<p></p>");
    assert!(html.contains("<p"));
    assert!(html.contains("</p>"));
}

#[test]
fn deeply_nested() {
    let html = serialize("<div><div><div><div><p>Deep</p></div></div></div></div>");
    assert!(html.contains("Deep"));
}

#[test]
fn multiple_classes_on_element() {
    let html = serialize("<div class=\"a b c\">Multi</div>");
    assert!(html.contains("class=\"a b c\""));
}

// ============================================================
// HTML Attributes Round-Trip
// ============================================================

#[test]
fn data_attribute_round_trip() {
    let html = serialize("<div data-value=\"42\">content</div>");
    assert!(html.contains("data-value=\"42\""));
}

#[test]
fn custom_data_attribute_round_trip() {
    let html = serialize("<div data-custom=\"hello\">content</div>");
    assert!(html.contains("data-custom=\"hello\""));
}

#[test]
fn title_attribute_round_trip() {
    let html = serialize("<div title=\"tooltip text\">content</div>");
    assert!(html.contains("title=\"tooltip text\""));
}

#[test]
fn alt_attribute_round_trip() {
    let html = serialize("<img src=\"test.jpg\" alt=\"description\">");
    assert!(html.contains("alt=\"description\""));
}

#[test]
fn role_attribute_round_trip() {
    let html = serialize("<div role=\"button\">Click</div>");
    assert!(html.contains("role=\"button\""));
}

#[test]
fn aria_attribute_round_trip() {
    let html = serialize("<div aria-label=\"close button\">X</div>");
    assert!(html.contains("aria-label=\"close button\""));
}

#[test]
fn multiple_custom_attributes() {
    let html = serialize("<div data-x=\"1\" data-y=\"2\" title=\"tip\">content</div>");
    assert!(html.contains("data-x=\"1\""));
    assert!(html.contains("data-y=\"2\""));
    assert!(html.contains("title=\"tip\""));
}

#[test]
fn boolean_attribute_round_trip() {
    let html = serialize("<details open>content</details>");
    assert!(html.contains("open"));
}

#[test]
fn name_attribute_round_trip() {
    let html = serialize("<div name=\"myfield\">content</div>");
    assert!(html.contains("name=\"myfield\""));
}

#[test]
fn dir_attribute_round_trip() {
    let html = serialize("<div dir=\"rtl\">content</div>");
    assert!(html.contains("dir=\"rtl\""));
}

#[test]
fn lang_attribute_round_trip() {
    let html = serialize("<div lang=\"fr\">Bonjour</div>");
    assert!(html.contains("lang=\"fr\""));
}

#[test]
fn attributes_preserved_after_layout_round_trip() {
    let doc1 = load_html(
        "<div data-info=\"test\" title=\"hello\">content</div>",
        800.0,
    );
    let serialized = serialize_html(&doc1);
    assert!(serialized.contains("data-info=\"test\""));
    assert!(serialized.contains("title=\"hello\""));
}

#[test]
fn block_element_with_multiple_attrs_round_trip() {
    let html = serialize("<div data-type=\"highlight\" tabindex=\"0\">text</div>");
    assert!(html.contains("data-type=\"highlight\""));
    assert!(html.contains("tabindex=\"0\""));
}

#[test]
fn table_cell_attribute_round_trip() {
    let html = serialize("<table><tr><td data-cell=\"A1\">val</td></tr></table>");
    assert!(html.contains("data-cell=\"A1\""));
}

// ============================================================
// Missing tests ported from C++ test_serialization.cpp
// ============================================================

#[test]
fn special_characters_escaped() {
    let html = serialize("<p>A &amp; B &lt; C &gt; D</p>");
    // Should contain escaped entities or the original characters
    assert!(html.contains("&amp;") || html.contains("& B"));
}

#[test]
fn span_in_paragraph() {
    // Span is inline — text content should be preserved in round-trip
    let doc2 = round_trip("<p><span>text</span></p>");
    assert!(doc2.root.text_content().contains("text"));
}

#[test]
fn underline_round_trip() {
    let html = serialize("<p><u>Underline</u></p>");
    assert!(html.contains("<u>"));
    assert!(html.contains("</u>"));
}

#[test]
fn strikethrough_round_trip() {
    let html = serialize("<p><s>Strike</s></p>");
    assert!(html.contains("<s>"));
    assert!(html.contains("</s>"));
}

#[test]
fn mixed_formatting_round_trip() {
    let html = serialize("<p><b><i>BoldItalic</i></b></p>");
    assert!(html.contains("BoldItalic"));
    assert!(html.contains("<b>"));
    assert!(html.contains("<i>"));
}

#[test]
fn link_text_preserved() {
    let doc2 = round_trip("<p><a href=\"http://test.com\">Click here</a></p>");
    assert!(doc2.root.text_content().contains("Click here"));
}

#[test]
fn id_and_class_preserved() {
    let html = serialize("<div id=\"box\" class=\"highlight\">Text</div>");
    assert!(html.contains("id=\"box\""));
    assert!(html.contains("class=\"highlight\""));
}

#[test]
fn text_align_right() {
    let html = serialize("<p style=\"text-align: right;\">Right</p>");
    assert!(html.contains("text-align"));
    assert!(html.contains("right"));
}

#[test]
fn position_relative_style() {
    let html = serialize("<div style=\"position: relative; top: 10px;\">Pos</div>");
    assert!(html.contains("position"));
    assert!(html.contains("relative"));
    assert!(html.contains("top"));
}

#[test]
fn flex_direction_column() {
    let html = serialize("<div style=\"display: flex; flex-direction: column;\">Col</div>");
    assert!(html.contains("flex-direction"));
    assert!(html.contains("column"));
}

#[test]
fn multi_rule_stylesheet() {
    let html = serialize(
        "<html><head><style>\
         h1 { color: blue; }\
         p { margin: 10px; }\
         </style></head>\
         <body><h1>Title</h1><p>Body</p></body></html>",
    );
    assert!(html.contains("h1"));
    assert!(html.contains("color"));
}

#[test]
fn br_becomes_newline() {
    // BR is a void element in the DOM — text content from both sides preserved
    let doc2 = round_trip("<p>Line1<br>Line2</p>");
    assert!(doc2.root.text_content().contains("Line1"));
    assert!(doc2.root.text_content().contains("Line2"));
}

#[test]
fn empty_document() {
    use webcore::html::serialize_html;
    let doc = webcore::types::Document::new();
    // Serializing an empty document should not panic and produce minimal/empty output
    let html = serialize_html(&doc);
    // Either empty or just a root element tag; should not contain content
    assert!(!html.contains("undefined"));
}

#[test]
fn unknown_pseudo_element_survives_roundtrip() {
    // ::-webkit-scrollbar and other unknown pseudo-elements should be preserved
    // verbatim through a parse → serialize cycle, just like browsers do.
    use webcore::html::serialize_html;
    let html = r#"<html><head><style>
::-webkit-scrollbar { width: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: #ccc; border-radius: 3px; }
::cue { color: white; background: rgba(0,0,0,0.8); }
</style></head><body><p>Hello</p></body></html>"#;

    let doc = webcore::load_html(html, 800.0);
    let out = serialize_html(&doc);

    assert!(
        out.contains("::-webkit-scrollbar"),
        "`::-webkit-scrollbar` rule was dropped during serialization"
    );
    assert!(
        out.contains("::-webkit-scrollbar-track"),
        "`::-webkit-scrollbar-track` rule was dropped"
    );
    assert!(
        out.contains("::-webkit-scrollbar-thumb"),
        "`::-webkit-scrollbar-thumb` rule was dropped"
    );
    assert!(out.contains("::cue"), "`::cue` rule was dropped");
}

#[test]
fn known_pseudo_elements_survive_roundtrip() {
    use webcore::html::serialize_html;
    let html = r#"<html><head><style>
p::before { content: ">> "; color: red; }
p::after  { content: " <<"; }
::selection { background-color: yellow; }
li::marker { color: blue; }
p::first-line { font-size: 18px; }
</style></head><body><p>Hi</p></body></html>"#;

    let doc = webcore::load_html(html, 800.0);
    let out = serialize_html(&doc);

    assert!(out.contains("p::before"), "p::before lost");
    assert!(out.contains("p::after"), "p::after lost");
    assert!(out.contains("::selection"), "::selection lost");
    assert!(out.contains("li::marker"), "li::marker lost");
    assert!(out.contains("p::first-line"), "p::first-line lost");
}
