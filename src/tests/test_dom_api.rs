//! Tests for the public DOM API (query, read, mutate, classList, style, query_selector).

use crate::dom::arena::NodeId;
use crate::html::parse_html;

// ── Query ──────────────────────────────────────────────────────────────────

#[test]
fn get_element_by_id_finds_element() {
    let doc = parse_html(r#"<div id="main"><span id="inner">text</span></div>"#);
    let inner = doc.get_element_by_id("inner");
    assert!(inner.is_some());
    assert_eq!(doc.tag_name(inner.unwrap()), Some("span"));
}

#[test]
fn get_element_by_id_returns_none_for_missing() {
    let doc = parse_html("<div>hello</div>");
    assert!(doc.get_element_by_id("nope").is_none());
}

#[test]
fn query_selector_by_tag() {
    let doc = parse_html("<div><p>one</p><p>two</p><span>three</span></div>");
    let p = doc.query_selector("p");
    assert!(p.is_some());
    assert_eq!(doc.tag_name(p.unwrap()), Some("p"));
}

#[test]
fn query_selector_by_class() {
    let doc = parse_html(r#"<div><span class="a">x</span><span class="b">y</span></div>"#);
    let b = doc.query_selector(".b");
    assert!(b.is_some());
    assert_eq!(
        doc.get_attribute(b.unwrap(), "class"),
        Some("b".to_string())
    );
}

#[test]
fn query_selector_by_id() {
    let doc = parse_html(r#"<div><p id="target">hello</p></div>"#);
    let target = doc.query_selector("#target");
    assert!(target.is_some());
    assert_eq!(doc.tag_name(target.unwrap()), Some("p"));
}

#[test]
fn get_client_rects_returns_one_rect_per_owned_line_fragment() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0} #p{width:45px;font-size:16px;font-family:sans-serif}</style>\
         <p id=p>alpha beta gamma</p>",
        800.0,
    );
    let p = doc.get_element_by_id("p").expect("p");
    let rects = doc.get_client_rects(p);

    assert!(
        rects.len() >= 2,
        "wrapped paragraph should expose one client rect per line, got {rects:?}"
    );
    assert!(
        rects.iter().all(|r| r.w > 0.0 && r.h > 0.0),
        "client rect fragments should be non-empty: {rects:?}"
    );
}

#[test]
fn query_selector_descendant() {
    let doc = parse_html(r#"<div><ul><li>item</li></ul></div>"#);
    let li = doc.query_selector("div li");
    assert!(li.is_some());
    assert_eq!(doc.tag_name(li.unwrap()), Some("li"));
}

#[test]
fn query_selector_all_returns_all_matches() {
    let doc = parse_html("<ul><li>a</li><li>b</li><li>c</li></ul>");
    let items = doc.query_selector_all("li");
    assert_eq!(items.len(), 3);
    for &id in &items {
        assert_eq!(doc.tag_name(id), Some("li"));
    }
}

#[test]
fn query_selector_returns_none_for_no_match() {
    let doc = parse_html("<div>hello</div>");
    assert!(doc.query_selector("span").is_none());
    assert!(doc.query_selector_all("span").is_empty());
}

// ── Read ───────────────────────────────────────────────────────────────────

#[test]
fn dom_tag_returns_tag_name() {
    let doc = parse_html("<div><p>hi</p></div>");
    let p = doc.query_selector("p").unwrap();
    assert_eq!(doc.tag_name(p), Some("p"));
}

#[test]
fn dom_get_attribute_returns_value() {
    let doc = parse_html(r#"<a href="/link" title="tip">click</a>"#);
    let a = doc.query_selector("a").unwrap();
    assert_eq!(doc.get_attribute(a, "href"), Some("/link".to_string()));
    assert_eq!(doc.get_attribute(a, "title"), Some("tip".to_string()));
    assert_eq!(doc.get_attribute(a, "missing"), None);
}

#[test]
fn dom_text_content_collects_all_text() {
    let doc = parse_html("<div>Hello <span>World</span>!</div>");
    let div = doc.query_selector("div").unwrap();
    let text = doc.text_content(div);
    assert!(text.contains("Hello"));
    assert!(text.contains("World"));
}

#[test]
fn dom_parent_and_children() {
    let doc = parse_html("<ul><li>a</li><li>b</li></ul>");
    let ul = doc.query_selector("ul").unwrap();
    let children = doc.child_nodes(ul);
    assert_eq!(children.len(), 2);

    for &child_id in &children {
        assert_eq!(doc.parent_node(child_id), ul);
    }
}

#[test]
fn dom_sibling_navigation() {
    let doc = parse_html("<div><span>a</span><p>b</p><em>c</em></div>");
    let span = doc.query_selector("span").unwrap();
    let p = doc.query_selector("p").unwrap();
    let em = doc.query_selector("em").unwrap();

    assert_eq!(doc.next_sibling(span), p);
    assert_eq!(doc.next_sibling(p), em);
    assert_eq!(doc.next_sibling(em), 0);

    assert_eq!(doc.previous_sibling(em), p);
    assert_eq!(doc.previous_sibling(p), span);
    assert_eq!(doc.previous_sibling(span), 0);
}

// ── Mutate ─────────────────────────────────────────────────────────────────

#[test]
fn create_and_append_element() {
    let mut doc = parse_html("<div></div>");
    let div = doc.query_selector("div").unwrap();

    let p = doc.create_element("p");
    assert!(p != 0);
    doc.append_child(div, p);

    // Verify in arena
    let children = doc.child_nodes(div);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0], p);
    assert_eq!(doc.tag_name(p), Some("p"));

    // Verify in WebCore tree
    let div_box = doc.get_box_by_id(div).unwrap();
    assert_eq!(div_box.children.len(), 1);
    assert_eq!(div_box.children[0].tag, "p");
    assert_eq!(div_box.children[0].node_id, p);
}

#[test]
fn create_and_append_text() {
    let mut doc = parse_html("<p></p>");
    let p = doc.query_selector("p").unwrap();

    let text = doc.create_text_node("Hello World");
    doc.append_child(p, text);

    assert_eq!(doc.text_content(p), "Hello World");

    let p_box = doc.get_box_by_id(p).unwrap();
    assert_eq!(p_box.children.len(), 1);
    assert_eq!(p_box.children[0].text, "Hello World");
}

#[test]
fn insert_before_works() {
    let mut doc = parse_html("<ul><li>b</li></ul>");
    let ul = doc.query_selector("ul").unwrap();
    let li_b = doc.query_selector("li").unwrap();

    let li_a = doc.create_element("li");
    doc.insert_before(ul, li_a, li_b);

    let children = doc.child_nodes(ul);
    assert_eq!(children.len(), 2);
    assert_eq!(children[0], li_a);
    assert_eq!(children[1], li_b);

    // WebCore tree matches
    let ul_box = doc.get_box_by_id(ul).unwrap();
    assert_eq!(ul_box.children.len(), 2);
    assert_eq!(ul_box.children[0].node_id, li_a);
    assert_eq!(ul_box.children[1].node_id, li_b);
}

#[test]
fn remove_child_works() {
    let mut doc = parse_html("<ul><li>a</li><li>b</li><li>c</li></ul>");
    let ul = doc.query_selector("ul").unwrap();
    let items = doc.query_selector_all("li");
    assert_eq!(items.len(), 3);

    // Remove middle item
    doc.remove_child(items[1]);

    let remaining = doc.child_nodes(ul);
    assert_eq!(remaining.len(), 2);
    assert_eq!(remaining[0], items[0]);
    assert_eq!(remaining[1], items[2]);

    // WebCore tree matches
    let ul_box = doc.get_box_by_id(ul).unwrap();
    assert_eq!(ul_box.children.len(), 2);
}

#[test]
fn set_attribute_updates_both_trees() {
    let mut doc = parse_html(r#"<div id="target"></div>"#);
    let div = doc.get_element_by_id("target").unwrap();

    doc.set_attribute(div, "data-value", "42");

    // Arena updated
    assert_eq!(doc.get_attribute(div, "data-value"), Some("42".to_string()));

    // WebCore updated
    let div_box = doc.get_box_by_id(div).unwrap();
    assert_eq!(
        div_box.attributes.get("data-value").map(|s| s.as_str()),
        Some("42")
    );
}

#[test]
fn remove_attribute_updates_both_trees() {
    let mut doc = parse_html(r#"<div id="target" data-x="old"></div>"#);
    let div = doc.get_element_by_id("target").unwrap();

    doc.remove_attribute(div, "data-x");

    assert_eq!(doc.get_attribute(div, "data-x"), None);
    let div_box = doc.get_box_by_id(div).unwrap();
    assert!(!div_box.attributes.contains_key("data-x"));
}

#[test]
fn set_text_content_replaces_children() {
    let mut doc = parse_html("<div><p>old</p><span>stuff</span></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_text_content(div, "new text only");

    assert_eq!(doc.text_content(div), "new text only");
    // **DOM §4.4 "string replace all": the children are replaced by ONE Text
    // node** — not by a string parked on the element.
    //
    // This asserted `0` and `div_box.text`, which is what the implementation
    // did and not what the DOM says. It matters beyond tidiness: layout reads a
    // box's own `text` only for a text node and a pseudo-element, so text
    // written this way was in the DOM, in `textContent` and in `outerHTML` and
    // painted NOWHERE. Every caption of every .NET control arrives exactly this
    // way, and every one of them rendered as an empty rectangle.
    let children = doc.child_nodes(div);
    assert_eq!(children.len(), 1);
    assert_eq!(doc.node_type(children[0]), 3, "the child is a Text node");

    let div_box = doc.get_box_by_id(div).unwrap();
    assert_eq!(div_box.children.len(), 1);
    assert_eq!(div_box.children[0].text, "new text only");

    // Empty text is the spec's null case: children removed, nothing inserted.
    doc.set_text_content(div, "");
    assert_eq!(doc.child_nodes(div).len(), 0);
}

#[test]
fn set_inner_html_parses_and_replaces() {
    let mut doc = parse_html(r#"<div id="container">old</div>"#);
    let div = doc.get_element_by_id("container").unwrap();

    doc.set_inner_html(div, "<p>new</p><span>content</span>");

    let children = doc.child_nodes(div);
    assert_eq!(children.len(), 2);
    assert_eq!(doc.tag_name(children[0]), Some("p"));
    assert_eq!(doc.tag_name(children[1]), Some("span"));

    let div_box = doc.get_box_by_id(div).unwrap();
    assert_eq!(div_box.children.len(), 2);
    assert_eq!(div_box.children[0].tag, "p");
    assert_eq!(div_box.children[1].tag, "span");
}

// ── classList ──────────────────────────────────────────────────────────────

#[test]
fn class_list_add_and_contains() {
    let mut doc = parse_html("<div></div>");
    let div = doc.query_selector("div").unwrap();

    assert!(!doc.class_list_contains(div, "active"));
    doc.class_list_add(div, "active");
    assert!(doc.class_list_contains(div, "active"));

    // Adding again doesn't duplicate
    doc.class_list_add(div, "active");
    assert_eq!(doc.get_attribute(div, "class"), Some("active".to_string()));
}

#[test]
fn class_list_remove() {
    let mut doc = parse_html(r#"<div class="a b c"></div>"#);
    let div = doc.query_selector("div").unwrap();

    doc.class_list_remove(div, "b");
    assert!(!doc.class_list_contains(div, "b"));
    assert!(doc.class_list_contains(div, "a"));
    assert!(doc.class_list_contains(div, "c"));
}

#[test]
fn class_list_toggle() {
    let mut doc = parse_html(r#"<div class="on"></div>"#);
    let div = doc.query_selector("div").unwrap();

    let result = doc.class_list_toggle(div, "on");
    assert!(!result); // was present, now removed
    assert!(!doc.class_list_contains(div, "on"));

    let result = doc.class_list_toggle(div, "on");
    assert!(result); // was absent, now added
    assert!(doc.class_list_contains(div, "on"));
}

// ── Inline style ───────────────────────────────────────────────────────────

#[test]
fn set_and_get_style_property() {
    let mut doc = parse_html("<div></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(div, "color", "red");
    assert_eq!(
        doc.get_style_property(div, "color"),
        Some("red".to_string())
    );

    doc.set_style_property(div, "font-size", "16px");
    assert_eq!(
        doc.get_style_property(div, "font-size"),
        Some("16px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "color"),
        Some("red".to_string())
    );
}

#[test]
fn set_style_property_overwrites() {
    let mut doc = parse_html(r#"<div style="color: blue"></div>"#);
    let div = doc.query_selector("div").unwrap();

    assert_eq!(
        doc.get_style_property(div, "color"),
        Some("blue".to_string())
    );
    doc.set_style_property(div, "color", "red");
    assert_eq!(
        doc.get_style_property(div, "color"),
        Some("red".to_string())
    );
}

#[test]
fn set_style_property_ignores_unknown_properties_but_keeps_custom_properties() {
    let mut doc = parse_html(r#"<div style="color: blue"></div>"#);
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(div, "colour", "red");
    assert_eq!(doc.get_style_property(div, "colour"), None);
    assert!(!doc.get_attribute(div, "style").unwrap().contains("colour"));

    doc.set_style_property(div, "--brand-color", "red");
    assert_eq!(
        doc.get_style_property(div, "--brand-color"),
        Some("red".to_string())
    );
}

#[test]
fn inline_style_property_value_strips_important_priority() {
    let doc = parse_html(r#"<div style="color: red !important; width: 10px"></div>"#);
    let div = doc.query_selector("div").unwrap();

    assert_eq!(
        doc.get_style_property(div, "color"),
        Some("red".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "color"),
        Some("important".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "width"),
        Some("10px".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "width"),
        Some(String::new())
    );
}

#[test]
fn set_style_property_serializes_trailing_semicolons() {
    let mut doc = parse_html("<div></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(div, "color", "red");
    assert_eq!(
        doc.get_attribute(div, "style").as_deref(),
        Some("color: red;")
    );

    doc.set_style_property(div, "font-size", "16px");
    assert_eq!(
        doc.get_attribute(div, "style").as_deref(),
        Some("color: red; font-size: 16px;")
    );
}

#[test]
fn remove_style_property() {
    let mut doc = parse_html(r#"<div style="color: red; font-size: 14px"></div>"#);
    let div = doc.query_selector("div").unwrap();

    assert_eq!(
        doc.remove_style_property(div, "color"),
        Some("red".to_string())
    );
    assert_eq!(doc.get_style_property(div, "color"), None);
    assert_eq!(
        doc.get_style_property(div, "font-size"),
        Some("14px".to_string())
    );
}

#[test]
fn inline_style_enumerates_items_and_remove_returns_old_value() {
    let mut doc = parse_html(r#"<div style="color: red !important; margin-left: 2px"></div>"#);
    let div = doc.query_selector("div").unwrap();

    assert_eq!(doc.style_property_len(div), 2);
    assert_eq!(doc.style_property_item(div, 0), Some("color".to_string()));
    assert_eq!(
        doc.style_property_item(div, 1),
        Some("margin-left".to_string())
    );
    assert_eq!(doc.style_property_item(div, 2), None);

    assert_eq!(
        doc.remove_style_property(div, "color"),
        Some("red".to_string())
    );
    assert_eq!(doc.remove_style_property(div, "missing"), None);
    assert_eq!(
        doc.get_attribute(div, "style").as_deref(),
        Some("margin-left: 2px;")
    );
}

#[test]
fn document_stylesheet_rules_can_be_inserted_deleted_and_recascaded() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html("<style>p { color: red; }</style><p id=p>x</p>", 400.0);
    let p = doc.get_element_by_id("p").unwrap();

    assert_eq!(doc.style_sheet_len(), 1);
    let rule_count = doc.style_sheet_css_rules(0).unwrap().len();
    assert!(
        doc.style_sheet_css_rules(0)
            .unwrap()
            .contains(&"p { color: red; }".to_string()),
        "the aggregate document stylesheet should expose author rules"
    );
    assert_eq!(
        doc.insert_style_sheet_rule(0, "p { color: blue; }", rule_count),
        Ok(rule_count)
    );

    crate::layout::LayoutEngine::new().layout(&mut doc, 400.0);
    assert_eq!(doc.computed_style_property(p, "color"), "rgb(0, 0, 255)");

    assert_eq!(doc.delete_style_sheet_rule(0, rule_count), Ok(()));
    crate::layout::LayoutEngine::new().layout(&mut doc, 400.0);
    assert_eq!(doc.computed_style_property(p, "color"), "rgb(255, 0, 0)");

    assert_eq!(
        doc.insert_style_sheet_rule(1, "p { color: green; }", 0),
        Err("IndexSizeError".to_string())
    );
}

#[test]
fn inline_style_parser_keeps_semicolons_inside_url_and_strings() {
    let mut doc = parse_html(
        r#"<div style='background-image: url("data:image/svg+xml;base64,AAAA"); content: "a;b"; color: red'></div>"#,
    );
    let div = doc.query_selector("div").unwrap();

    assert_eq!(
        doc.get_style_property(div, "background-image").as_deref(),
        Some(r#"url("data:image/svg+xml;base64,AAAA")"#)
    );
    assert_eq!(
        doc.get_style_property(div, "content").as_deref(),
        Some(r#""a;b""#)
    );

    doc.set_style_property(div, "color", "blue");
    let style = doc.get_attribute(div, "style").unwrap();
    assert!(style.contains(r#"background-image: url("data:image/svg+xml;base64,AAAA");"#));
    assert!(style.contains(r#"content: "a;b";"#));
    assert!(style.contains("color: blue;"));
}

#[test]
fn set_style_property_expands_box_and_pair_shorthands() {
    let mut doc = parse_html("<div></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(div, "margin", "10px 20px");
    assert_eq!(doc.get_style_property(div, "margin"), None);
    assert_eq!(
        doc.get_style_property(div, "margin-top"),
        Some("10px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "margin-right"),
        Some("20px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "margin-bottom"),
        Some("10px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "margin-left"),
        Some("20px".to_string())
    );

    doc.set_style_property(div, "gap", "4px 8px !important");
    assert_eq!(doc.get_style_property(div, "gap"), None);
    assert_eq!(
        doc.get_style_property(div, "row-gap"),
        Some("4px".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "row-gap"),
        Some("important".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "column-gap"),
        Some("8px".to_string())
    );

    doc.set_style_property(div, "border", "2px solid red !important");
    assert_eq!(doc.get_style_property(div, "border"), None);
    assert_eq!(
        doc.get_style_property(div, "border-top-width"),
        Some("2px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-right-style"),
        Some("solid".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-bottom-color"),
        Some("red".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "border-left-width"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "border-left", "thick dashed blue");
    assert_eq!(
        doc.get_style_property(div, "border-left-width"),
        Some("thick".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-left-style"),
        Some("dashed".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-left-color"),
        Some("blue".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-right-style"),
        Some("solid".to_string())
    );

    doc.set_style_property(div, "outline", "3px dotted green");
    assert_eq!(doc.get_style_property(div, "outline"), None);
    assert_eq!(
        doc.get_style_property(div, "outline-width"),
        Some("3px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "outline-style"),
        Some("dotted".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "outline-color"),
        Some("rgb(0, 128, 0)".to_string())
    );

    doc.set_style_property(div, "columns", "120px 3 !important");
    assert_eq!(doc.get_style_property(div, "columns"), None);
    assert_eq!(
        doc.get_style_property(div, "column-width"),
        Some("120px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "column-count"),
        Some("3".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "column-count"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "column-rule", "4px dashed blue");
    assert_eq!(doc.get_style_property(div, "column-rule"), None);
    assert_eq!(
        doc.get_style_property(div, "column-rule-width"),
        Some("4px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "column-rule-style"),
        Some("dashed".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "column-rule-color"),
        Some("rgb(0, 0, 255)".to_string())
    );

    doc.set_style_property(div, "margin", "");
    assert_eq!(doc.get_style_property(div, "margin-top"), None);
    assert_eq!(doc.get_style_property(div, "margin-left"), None);
    assert_eq!(
        doc.get_style_property(div, "row-gap"),
        Some("4px".to_string())
    );
}

#[test]
fn set_style_property_expands_font_shorthand() {
    let mut doc = parse_html("<div style='font-variant-numeric: tabular-nums;'></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "font",
        r#"italic small-caps 700 condensed 16px/20px "A B" !important"#,
    );

    assert_eq!(doc.get_style_property(div, "font"), None);
    assert_eq!(
        doc.get_style_property(div, "font-style"),
        Some("italic".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-variant"),
        Some("small-caps".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-weight"),
        Some("700".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-stretch"),
        Some("condensed".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-size"),
        Some("16px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "line-height"),
        Some("20px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-family"),
        Some(r#""A B""#.to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "font-family"),
        Some("important".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "font-variant-numeric"),
        Some("normal".to_string())
    );

    doc.set_style_property(div, "font", "");
    assert_eq!(doc.get_style_property(div, "font-size"), None);
    assert_eq!(doc.get_style_property(div, "font-variant-numeric"), None);
}

#[test]
fn set_style_property_expands_background_shorthand() {
    let mut doc = parse_html(
        "<div style='background-position: 10px 20px; background-blend-mode: multiply;'></div>",
    );
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "background",
        r#"red url("hero.png") left top / cover no-repeat fixed content-box padding-box !important"#,
    );

    assert_eq!(doc.get_style_property(div, "background"), None);
    assert_eq!(
        doc.get_style_property(div, "background-color"),
        Some("rgb(255, 0, 0)".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-image"),
        Some(r#"url("hero.png")"#.to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-position"),
        Some("0% 0%".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-size"),
        Some("cover".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-repeat"),
        Some("no-repeat".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-attachment"),
        Some("fixed".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-origin"),
        Some("content-box".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-clip"),
        Some("padding-box".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "background-blend-mode"),
        Some("normal".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "background-image"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "background", "");
    assert_eq!(doc.get_style_property(div, "background-color"), None);
    assert_eq!(doc.get_style_property(div, "background-image"), None);
}

#[test]
fn set_style_property_expands_mask_shorthand() {
    let mut doc = parse_html("<div style='mask-position: left top; mask-repeat: repeat-x;'></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "mask",
        r#"url("mask.svg") no-repeat center / contain content-box border-box alpha exclude !important"#,
    );

    assert_eq!(doc.get_style_property(div, "mask"), None);
    assert_eq!(
        doc.get_style_property(div, "mask-image"),
        Some(r#"url("mask.svg")"#.to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-repeat"),
        Some("no-repeat".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-position"),
        Some("center".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-size"),
        Some("contain".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-origin"),
        Some("content-box".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-clip"),
        Some("border-box".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-mode"),
        Some("alpha".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "mask-composite"),
        Some("exclude".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "mask-image"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "mask", "");
    assert_eq!(doc.get_style_property(div, "mask-image"), None);
    assert_eq!(doc.get_style_property(div, "mask-repeat"), None);
}

#[test]
fn set_style_property_expands_border_image_shorthand() {
    let mut doc = parse_html(
        "<div style='border-image-source: url(old.png); border-image-repeat: repeat;'></div>",
    );
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "border-image",
        "url(border.png) 30 fill / 10px / 2px round stretch !important",
    );

    assert_eq!(doc.get_style_property(div, "border-image"), None);
    assert_eq!(
        doc.get_style_property(div, "border-image-source"),
        Some("url(border.png)".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-image-slice"),
        Some("30 fill".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-image-width"),
        Some("10px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-image-outset"),
        Some("2px".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "border-image-repeat"),
        Some("round stretch".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "border-image-source"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "border-image", "");
    assert_eq!(doc.get_style_property(div, "border-image-source"), None);
    assert_eq!(doc.get_style_property(div, "border-image-repeat"), None);
}

#[test]
fn set_style_property_expands_list_style_shorthand() {
    let mut doc = parse_html(
        "<ul><li style='list-style-position: outside; list-style-image: url(old.svg);'></li></ul>",
    );
    let li = doc.query_selector("li").unwrap();

    doc.set_style_property(
        li,
        "list-style",
        r#"inside square url("marker.svg") !important"#,
    );

    assert_eq!(doc.get_style_property(li, "list-style"), None);
    assert_eq!(
        doc.get_style_property(li, "list-style-type"),
        Some("square".to_string())
    );
    assert_eq!(
        doc.get_style_property(li, "list-style-position"),
        Some("inside".to_string())
    );
    assert_eq!(
        doc.get_style_property(li, "list-style-image"),
        Some(r#"url("marker.svg")"#.to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(li, "list-style-image"),
        Some("important".to_string())
    );

    doc.set_style_property(li, "list-style", "");
    assert_eq!(doc.get_style_property(li, "list-style-type"), None);
    assert_eq!(doc.get_style_property(li, "list-style-image"), None);
}

#[test]
fn set_style_property_expands_transition_shorthand() {
    let mut doc =
        parse_html("<div style='transition-duration: 4s; transition-property: width;'></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "transition",
        "opacity 200ms ease-in 50ms, display 1s step-end allow-discrete !important",
    );

    assert_eq!(doc.get_style_property(div, "transition"), None);
    assert_eq!(
        doc.get_style_property(div, "transition-property"),
        Some("opacity, display".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "transition-duration"),
        Some("200ms, 1s".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "transition-timing-function"),
        Some("ease-in, step-end".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "transition-delay"),
        Some("50ms, 0s".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "transition-behavior"),
        Some("normal, allow-discrete".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "transition-property"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "transition", "none");
    assert_eq!(
        doc.get_style_property(div, "transition-property"),
        Some("all".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "transition-duration"),
        Some("0s".to_string())
    );
}

#[test]
fn set_style_property_expands_animation_shorthand() {
    let mut doc = parse_html("<div style='animation-duration: 4s; animation-name: old;'></div>");
    let div = doc.query_selector("div").unwrap();

    doc.set_style_property(
        div,
        "animation",
        "fade 200ms ease-in 50ms infinite alternate both paused add, slide 1s step-end 2 reverse forwards running accumulate !important",
    );

    assert_eq!(doc.get_style_property(div, "animation"), None);
    assert_eq!(
        doc.get_style_property(div, "animation-name"),
        Some("fade, slide".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-duration"),
        Some("200ms, 1s".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-timing-function"),
        Some("ease-in, step-end".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-delay"),
        Some("50ms, 0s".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-iteration-count"),
        Some("infinite, 2".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-direction"),
        Some("alternate, reverse".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-fill-mode"),
        Some("both, forwards".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-play-state"),
        Some("paused, running".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-composition"),
        Some("add, accumulate".to_string())
    );
    assert_eq!(
        doc.get_style_property_priority(div, "animation-name"),
        Some("important".to_string())
    );

    doc.set_style_property(div, "animation", "none");
    assert_eq!(
        doc.get_style_property(div, "animation-name"),
        Some("none".to_string())
    );
    assert_eq!(
        doc.get_style_property(div, "animation-duration"),
        Some("0s".to_string())
    );
}

// ── Layout queries ─────────────────────────────────────────────────────────

#[test]
fn dom_offset_returns_zero_without_layout() {
    let doc = parse_html("<div>hi</div>");
    let div = doc.query_selector("div").unwrap();
    // Without layout pass, dimensions are 0
    assert_eq!(doc.offset_width(div), 0.0);
    assert_eq!(doc.offset_height(div), 0.0);
}

// ── Move existing node ─────────────────────────────────────────────────────

#[test]
fn append_child_moves_existing_node() {
    let mut doc = parse_html("<div><p>a</p></div><section></section>");
    let p = doc.query_selector("p").unwrap();
    let section = doc.query_selector("section").unwrap();
    let div = doc.query_selector("div").unwrap();

    // Move <p> from <div> to <section>
    doc.append_child(section, p);

    // div should have 0 children
    assert_eq!(doc.child_nodes(div).len(), 0);
    // section should have 1 child
    let sec_children = doc.child_nodes(section);
    assert_eq!(sec_children.len(), 1);
    assert_eq!(sec_children[0], p);
    assert_eq!(doc.parent_node(p), section);
}

// ── Dirty flags ────────────────────────────────────────────────────────────

#[test]
fn attribute_mutation_sets_dirty_flag() {
    let mut doc = parse_html(r#"<div id="target"></div>"#);
    let div = doc.get_element_by_id("target").unwrap();

    // Clear dirty flags
    doc.arena.get_mut(NodeId(div)).dirty = crate::dom::arena::DirtyFlags::NONE;

    doc.set_attribute(div, "class", "active");

    // Should be style-dirty now
    assert!(doc
        .arena
        .get(NodeId(div))
        .dirty
        .contains(crate::dom::arena::DirtyFlags::STYLE));
}

// ─── WHATWG conformance ─────────────────────────────────────────────────────
//
// These pin the parts of the DOM this engine has to answer the same way any
// browser does, because the target is running the same programs under a real
// one. Written against the IDL, not against whatever we happened to implement.

#[test]
fn node_type_uses_the_dom_numbering() {
    let mut doc = parse_html("<div id='d'>text</div>");
    let div = doc.get_element_by_id("d").unwrap();
    assert_eq!(doc.node_type(div), 1, "an element is 1");

    let t = doc.create_text_node("plain");
    assert_eq!(doc.node_type(t), 3, "a text node is 3");

    let c = doc.create_cdata_section("raw");
    assert_eq!(doc.node_type(c), 4, "CDATA is 4");

    let pi = doc.create_processing_instruction("xml-stylesheet", "href='a.css'");
    assert_eq!(doc.node_type(pi), 7, "a processing instruction is 7");

    let comment = doc.create_comment("hi");
    assert_eq!(doc.node_type(comment), 8, "a comment is 8");
}

#[test]
fn node_value_is_null_for_an_element_and_data_for_the_rest() {
    let mut doc = parse_html("<div id='d'>text</div>");
    let div = doc.get_element_by_id("d").unwrap();
    assert_eq!(doc.node_value(div), None, "an element has no nodeValue");

    let comment = doc.create_comment("hi");
    assert_eq!(doc.node_value(comment).as_deref(), Some("hi"));
    assert_eq!(doc.node_name(comment), "#comment");

    let cdata = doc.create_cdata_section("a < b");
    assert_eq!(doc.node_value(cdata).as_deref(), Some("a < b"));
    assert_eq!(doc.node_name(cdata), "#cdata-section");

    // A processing instruction's nodeName is its TARGET, not a tag.
    let pi = doc.create_processing_instruction("xml-stylesheet", "href='a.css'");
    assert_eq!(doc.node_name(pi), "xml-stylesheet");
    assert_eq!(doc.node_value(pi).as_deref(), Some("href='a.css'"));
}

#[test]
fn text_content_is_text_only_and_skips_comments() {
    let mut doc = parse_html("<div id='d'></div>");
    let div = doc.get_element_by_id("d").unwrap();
    let visible = doc.create_text_node("visible");
    let hidden = doc.create_comment("hidden");
    doc.append_child(div, visible);
    doc.append_child(div, hidden);

    // DOM §4.4: textContent concatenates TEXT descendants. Comments carry data
    // but are not text, so they must not leak into it.
    assert_eq!(doc.text_content(div), "visible");
}

#[test]
fn a_namespaced_element_reports_its_parts() {
    const SVG: &str = "http://www.w3.org/2000/svg";
    let mut doc = parse_html("<div id='d'></div>");
    let rect = doc.create_element_ns(SVG, "svg:rect");

    assert_eq!(doc.namespace_uri(rect).as_deref(), Some(SVG));
    assert_eq!(doc.prefix(rect).as_deref(), Some("svg"));
    assert_eq!(doc.local_name(rect), "rect");
    // nodeName is the QUALIFIED name, prefix included.
    assert_eq!(doc.node_name(rect), "svg:rect");

    // An unprefixed HTML element has no prefix — null, not "".
    let div = doc.get_element_by_id("d").unwrap();
    assert_eq!(doc.prefix(div), None);
    assert_eq!(doc.local_name(div), "div");
}

#[test]
fn get_attribute_ns_tells_apart_two_attributes_sharing_a_local_name() {
    const XLINK: &str = "http://www.w3.org/1999/xlink";
    let mut doc = parse_html("<a id='a'></a>");
    let a = doc.get_element_by_id("a").unwrap();

    doc.set_attribute(a, "href", "plain");
    doc.set_attribute_ns(a, XLINK, "xlink:href", "linked");

    assert_eq!(
        doc.get_attribute_ns(a, XLINK, "href").as_deref(),
        Some("linked")
    );
    // The null namespace is a DIFFERENT attribute, not a fallback.
    assert_eq!(
        doc.get_attribute_ns(a, "", "href").as_deref(),
        Some("plain")
    );
    // A namespace nothing was written under matches nothing.
    assert_eq!(
        doc.get_attribute_ns(a, "http://example.invalid/ns", "href"),
        None
    );
}

#[test]
fn an_html_document_folds_names_but_an_xml_document_does_not() {
    let mut doc = parse_html("<div id='d'></div>");
    let upper = doc.create_element("DIV");
    assert_eq!(
        doc.tag_name(upper),
        Some("div"),
        "HTML lowercases tag names"
    );

    let d = doc.get_element_by_id("d").unwrap();
    doc.set_attribute(d, "DATA-X", "1");
    assert_eq!(doc.get_attribute(d, "data-x").as_deref(), Some("1"));
    assert_eq!(doc.get_attribute(d, "Data-X").as_deref(), Some("1"));

    // XML is case-SENSITIVE: <Rect> and <rect> are two different elements, and
    // folding one into the other would silently merge them.
    let mut xml = parse_html("<div></div>");
    xml.kind = crate::types::DocumentKind::Xml;
    let rect = xml.create_element("Rect");
    assert_eq!(xml.tag_name(rect), Some("Rect"));
    xml.set_attribute(rect, "viewBox", "0 0 1 1");
    assert_eq!(
        xml.get_attribute(rect, "viewBox").as_deref(),
        Some("0 0 1 1")
    );
    assert_eq!(xml.get_attribute(rect, "viewbox"), None);
}

#[test]
fn clone_node_is_detached_and_deep_only_when_asked() {
    let mut doc = parse_html("<div id='host'><span>child</span></div>");
    let host = doc.get_element_by_id("host").unwrap();
    doc.set_attribute(host, "data-k", "v");

    let shallow = doc.clone_node(host, false);
    assert_ne!(shallow, host, "a clone is a NEW node");
    assert!(
        !doc.is_connected(shallow),
        "a clone has no parent until inserted"
    );
    assert_eq!(
        doc.get_attribute(shallow, "data-k").as_deref(),
        Some("v"),
        "attributes are copied even by a shallow clone"
    );
    assert!(
        doc.child_nodes(shallow).is_empty(),
        "shallow copies no children"
    );

    let deep = doc.clone_node(host, true);
    assert!(
        !doc.child_nodes(deep).is_empty(),
        "a deep clone copies the subtree"
    );
}

#[test]
fn replace_child_swaps_in_place() {
    let mut doc = parse_html("<div id='host'></div>");
    let host = doc.get_element_by_id("host").unwrap();
    let first = doc.create_element("span");
    let second = doc.create_element("b");
    doc.append_child(host, first);
    doc.append_child(host, second);

    let fresh = doc.create_element("i");
    assert!(doc.replace_child(host, fresh, first));

    let kids = doc.child_nodes(host);
    assert_eq!(kids.len(), 2, "replace must not change the child count");
    assert_eq!(kids[0], fresh, "the replacement takes the old node's PLACE");
    assert_eq!(kids[1], second);
}

#[test]
fn select_value_is_the_selected_options_value_not_its_index() {
    let mut doc = parse_html("<select id='s'></select>");
    let s = doc.get_element_by_id("s").unwrap();
    doc.add_item(s, "one");
    doc.add_item(s, "two");

    assert_eq!(doc.item_text(s, 0), "one");
    assert_eq!(doc.item_text(s, 1), "two");

    // HTML §4.10.7: assigning to `value` selects the option WORTH that value.
    // The index is `selectedIndex`, its own IDL member.
    doc.set_value(s, "two");
    assert_eq!(doc.value(s), "two");
    assert_eq!(doc.selected_index(s), 1);
}

#[test]
fn a_dialog_is_closed_until_shown_and_only_a_modal_is_fixed() {
    // Through the RENDERER, because the assertion below is about the UA
    // stylesheet applying — `parse_html` alone never runs the cascade.
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html("<dialog id='d'></dialog><dialog id='m'></dialog>", 400.0);
    let plain = doc.get_element_by_id("d").unwrap();
    let modal = doc.get_element_by_id("m").unwrap();
    assert!(!doc.dialog_open(plain), "a fresh dialog is not open");

    doc.show_dialog(plain, false);
    assert!(doc.dialog_open(plain), "show() sets the open attribute");

    doc.show_dialog(modal, true);
    // The UA stylesheet's own distinction: `dialog:modal` is position:fixed,
    // a non-modal stays in flow. If both looked alike, showModal() would be
    // show() under another name.
    //
    // ⛔ This asserted the INLINE style, which is what `show_dialog` used to
    // write — so it passed while the `dialog:modal` rule the comment describes
    // did not exist and `:modal` matched nothing. Modality is top-layer
    // membership now, and the COMPUTED value is what shows the rule applying.
    assert_eq!(
        doc.top_layer_nodes(),
        &[modal],
        "only the modal is in the top layer"
    );
    // The rule applies through the CASCADE, so it has to run.
    doc.recascade();
    // ⛔ Read from the CASCADED style, not `computed_style_property` — that
    // one answers a handful of properties and otherwise falls back to the
    // INLINE style, so it reports `""` for a `position` that came from a
    // stylesheet. Noted in `architecture.md`; asserting through it here would
    // have tested the fallback rather than the rule.
    let pos = |d: &crate::types::Document, id: u32| d.get_computed_style(id).map(|s| s.position);
    assert_eq!(
        pos(&doc, modal),
        Some(crate::types::Position::Fixed),
        "through the UA sheet's `dialog:modal` rule"
    );
    assert_ne!(pos(&doc, plain), Some(crate::types::Position::Fixed));

    doc.close_dialog(plain);
    assert!(!doc.dialog_open(plain), "close() clears the open attribute");
}

// ─── Traversal, selectors and the ChildNode/ParentNode mixins ───────────────
//
// The DOM offers the same tree through several vocabularies and a browser must
// answer every one. The distinction these pin down is the one that bites:
// `childNodes` counts text and comments, `children` counts only elements — a
// page walking the wrong one sees whitespace as a node.

#[test]
fn children_counts_elements_and_child_nodes_counts_everything() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let span = doc.create_element("span");
    let text = doc.create_text_node("between");
    let comment = doc.create_comment("hidden");
    doc.append_child(d, span);
    doc.append_child(d, text);
    doc.append_child(d, comment);

    assert_eq!(doc.child_nodes(d).len(), 3, "childNodes counts every kind");
    assert_eq!(doc.children(d).len(), 1, "children counts only elements");
    assert_eq!(doc.child_element_count(d), 1);
    assert!(doc.has_child_nodes(d));

    assert_eq!(doc.first_child(d), Some(span));
    assert_eq!(doc.last_child(d), Some(comment), "lastChild is the comment");
    assert_eq!(doc.first_element_child(d), Some(span));
    assert_eq!(
        doc.last_element_child(d),
        Some(span),
        "lastElementChild skips the comment"
    );
}

#[test]
fn element_siblings_skip_text_and_comments() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let a = doc.create_element("a");
    let text = doc.create_text_node("x");
    let b = doc.create_element("b");
    for child in [a, text, b] {
        doc.append_child(d, child);
    }

    // The plain sibling walk sees the text node…
    assert_eq!(doc.next_sibling(a), text);
    assert_eq!(doc.previous_sibling(b), text);
    // …the element walk does not. That is the whole difference.
    assert_eq!(doc.next_element_sibling(a), Some(b));
    assert_eq!(doc.previous_element_sibling(b), Some(a));
    assert_eq!(doc.next_element_sibling(b), None, "none at the end");
    assert_eq!(doc.previous_element_sibling(a), None);
}

#[test]
fn parent_element_is_none_when_the_parent_is_not_an_element() {
    let mut doc = parse_html("<div id='d'><span id='s'>t</span></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let s = doc.get_element_by_id("s").unwrap();
    assert_eq!(doc.parent_element(s), Some(d));

    // A detached node has no parent at all.
    let loose = doc.create_element("i");
    assert_eq!(doc.parent_element(loose), None);
}

#[test]
fn contains_includes_the_node_itself() {
    let doc = parse_html("<div id='outer'><span id='inner'>t</span></div>");
    let outer = doc.get_element_by_id("outer").unwrap();
    let inner = doc.get_element_by_id("inner").unwrap();

    assert!(
        doc.contains(outer, inner),
        "an ancestor contains a descendant"
    );
    // DOM §4.4: a node contains ITSELF. The part that surprises.
    assert!(doc.contains(outer, outer));
    assert!(!doc.contains(inner, outer), "not the other way round");
}

#[test]
fn matches_and_closest_start_at_the_element_itself() {
    let doc = parse_html(
        r#"<div id="outer" class="box"><p id="mid"><span id="inner">t</span></p></div>"#,
    );
    let outer = doc.get_element_by_id("outer").unwrap();
    let inner = doc.get_element_by_id("inner").unwrap();

    assert!(doc.matches(inner, "span"));
    assert!(!doc.matches(inner, "div"));

    // `closest` is ancestor-OR-SELF, so a matching element answers itself.
    assert_eq!(doc.closest(inner, "span"), Some(inner));
    assert_eq!(doc.closest(inner, ".box"), Some(outer));
    assert_eq!(doc.closest(inner, "table"), None);
}

#[test]
fn get_elements_by_class_name_requires_every_named_class() {
    let doc = parse_html(
        r#"<div><i id="a" class="x"></i><i id="b" class="x y"></i><i id="c" class="y"></i></div>"#,
    );
    let b = doc.get_element_by_id("b").unwrap();

    assert_eq!(doc.get_elements_by_class_name("x").len(), 2);
    // ALL of them, not any — "x y" is an intersection.
    assert_eq!(doc.get_elements_by_class_name("x y"), vec![b]);
    assert!(doc.get_elements_by_class_name("").is_empty());
    assert!(doc.get_elements_by_class_name("nope").is_empty());
}

#[test]
fn toggle_attribute_reports_presence_afterwards() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    assert!(!doc.has_attributes(d) || !doc.has_attribute(d, "hidden"));

    assert!(doc.toggle_attribute(d, "hidden"), "returns the NEW state");
    assert!(doc.has_attribute(d, "hidden"));
    assert!(doc.has_attributes(d));

    assert!(!doc.toggle_attribute(d, "hidden"));
    assert!(!doc.has_attribute(d, "hidden"));
}

#[test]
fn class_name_and_id_are_the_attributes() {
    let mut doc = parse_html("<div id='d' class='a b'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    assert_eq!(doc.class_name(d), "a b");
    assert_eq!(doc.id(d), "d");

    doc.set_class_name(d, "c");
    assert_eq!(doc.get_attribute(d, "class").as_deref(), Some("c"));
    doc.set_id(d, "renamed");
    assert_eq!(doc.get_element_by_id("renamed"), Some(d));
}

#[test]
fn document_element_and_body_resolve() {
    let doc = parse_html("<html><head><title>t</title></head><body><p>x</p></body></html>");
    assert!(doc.document_element().is_some());
    assert!(doc.body().is_some(), "body resolves");
    assert_eq!(doc.title(), "t", "the title survives parsing");

    // `<head>` is optional in the MARKUP and mandatory in the TREE — HTML
    // §13.2.6 inserts one whether or not the source wrote it.
    assert!(doc.head().is_some(), "head resolves");
}

#[test]
fn head_is_synthesised_even_when_the_markup_omits_it() {
    // No `<html>`, no `<head>`, no `<body>` in the source. All three must
    // still be in the tree, which is what makes `document.head` a safe thing
    // for a page to reach for.
    let doc = parse_html("<div>bare fragment</div>");
    assert!(doc.document_element().is_some(), "html is implied");
    assert!(doc.head().is_some(), "head is implied");
    assert!(doc.body().is_some(), "body is implied");

    // …and the implied head renders nothing. Asked of the RESOLVED style, not
    // `get_style_property` — that one answers the declared (inline `style`)
    // value per CSSOM §6.4.2, and a UA default was never declared anywhere.
    let head_box = doc.root.children.iter().find(|c| c.tag == "head").unwrap();
    assert_eq!(
        head_box.style.display,
        crate::types::Display::None,
        "head is in the tree but draws nothing"
    );
}

#[test]
fn a_misplaced_head_does_not_move_the_head_element() {
    // The parser never fails on bad markup, it NORMALISES it. HTML §13.2.6
    // runs one "before head" insertion mode: the head element is created the
    // moment content is seen, so it is html's first child no matter where —
    // or whether — the source wrote the tag. A later `<head>` is a parse error
    // and cannot relocate it.
    //
    // This is why `head` is the first child and `body` the second in EVERY
    // document, and why nothing downstream has to cope with a third order.
    for source in [
        "<div>x</div>",                              // no head at all
        "<head><title>t</title></head><div>x</div>", // head where it belongs
        "<div>x</div><head><title>t</title></head>", // head at the END
        "<body><div>x</div></body><head></head>",    // after an explicit body
    ] {
        let doc = parse_html(source);
        let tags: Vec<&str> = doc.root.children.iter().map(|c| c.tag.as_str()).collect();
        assert_eq!(
            tags,
            vec!["head", "body"],
            "html's children should normalise to head,body for {source:?}"
        );
    }
}

#[test]
fn a_late_head_token_is_ignored_and_its_contents_stay_in_the_body() {
    // The other half of §13.2.6's rule. Ignoring a misplaced `<head>` means
    // ignoring the TOKEN — the markup it wrapped is still content and is
    // still parsed. Before the insertion-mode flag existed, the parser
    // re-entered head parsing here and `parse_head_content` ate the `<p>`,
    // because head parsing discards everything it does not recognise.
    let doc = parse_html("<p>first</p><head><p>second</p></head><p>third</p>");
    let body = doc.root.children.iter().find(|c| c.tag == "body").unwrap();
    let paragraphs: Vec<&str> = body
        .children
        .iter()
        .filter(|c| c.tag == "p")
        .map(|c| c.children.first().map(|t| t.text.trim()).unwrap_or(""))
        .collect();
    assert_eq!(
        paragraphs,
        vec!["first", "second", "third"],
        "content wrapped in a stray <head> is body content, not head content"
    );
}

#[test]
fn focus_and_blur_move_active_element() {
    let mut doc = parse_html(r#"<input id="a"><input id="b">"#);
    let a = doc.get_element_by_id("a").unwrap();
    let b = doc.get_element_by_id("b").unwrap();

    doc.focus(a);
    assert_eq!(doc.active_element(), Some(a));
    doc.focus(b);
    assert_eq!(
        doc.active_element(),
        Some(b),
        "focus MOVES, it does not add"
    );
    doc.blur(b);
    assert_eq!(doc.active_element(), None);
}

#[test]
fn node_kind_predicates_agree_with_node_type() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let t = doc.create_text_node("x");
    let c = doc.create_comment("x");

    assert!(doc.is_element(d) && !doc.is_text_node(d) && !doc.is_character_data(d));
    assert!(doc.is_text_node(t) && doc.is_character_data(t) && !doc.is_element(t));
    assert!(doc.is_comment_node(c) && doc.is_character_data(c));
}

#[test]
fn text_data_is_the_nodes_own_data_not_its_subtree() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let t = doc.create_text_node("hello");
    doc.append_child(d, t);

    // `textContent` concatenates the subtree; `data` is the node's own.
    assert_eq!(doc.text_content(d), "hello");
    assert_eq!(doc.text_data(t), "hello");
    assert_eq!(doc.text_data(d), "", "an element has no data of its own");

    doc.set_text_data(t, "changed");
    assert_eq!(doc.text_content(d), "changed");
}

#[test]
fn select_items_can_be_read_replaced_removed_and_cleared() {
    let mut doc = parse_html("<select id='s'></select>");
    let s = doc.get_element_by_id("s").unwrap();
    doc.add_item(s, "one");
    doc.add_item(s, "two");
    doc.add_item(s, "three");
    assert_eq!(doc.item_count(s), 3);

    doc.set_item_text(s, 1, "TWO");
    assert_eq!(doc.item_text(s, 1), "TWO");

    doc.set_selected_index(s, 2);
    assert_eq!(doc.selected_index(s), 2);

    doc.remove_item(s, 0);
    assert_eq!(doc.item_count(s), 2);
    assert_eq!(doc.item_text(s, 0), "TWO");

    doc.clear_items(s);
    assert_eq!(doc.item_count(s), 0);
    assert_eq!(doc.selected_index(s), -1, "empty select selects nothing");
}

#[test]
fn setting_checked_changes_the_state_and_not_the_markup() {
    // HTML §4.10.5.3: `input.checked` is CHECKEDNESS; the `checked` content
    // attribute is `defaultChecked`, what a form reset restores to. Setting
    // the IDL member must not rewrite the document.
    //
    // This test asserted the opposite — that `set_checked(true)` adds the
    // attribute — because state and markup were one store. That is exactly the
    // conflation the dirty checkedness flag exists to prevent: with one store
    // the reset algorithm has nothing left to restore to, and
    // `getAttribute("checked")` reports the user's last click.
    let mut doc = parse_html(r#"<input id="c" type="checkbox">"#);
    let c = doc.get_element_by_id("c").unwrap();
    assert!(!doc.checked(c));
    assert!(!doc.has_attribute(c, "checked"));

    doc.set_checked(c, true);
    assert!(doc.checked(c), "checkedness follows the IDL setter");
    assert!(
        !doc.has_attribute(c, "checked"),
        "setting `checked` wrote into the markup"
    );

    doc.set_checked(c, false);
    assert!(!doc.checked(c));
    assert!(!doc.has_attribute(c, "checked"));

    // And the attribute is the DEFAULT: it no longer reaches a control whose
    // checkedness something has already claimed.
    doc.set_attribute(c, "checked", "");
    assert!(doc.has_attribute(c, "checked"));
    assert!(
        !doc.checked(c),
        "the attribute overwrote checkedness the caller had already set"
    );
}

#[test]
fn get_attribute_names_lists_what_was_set() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    doc.set_attribute(d, "data-a", "1");
    doc.set_attribute(d, "data-b", "2");

    let names = doc.get_attribute_names(d);
    assert!(names.iter().any(|n| n == "data-a"));
    assert!(names.iter().any(|n| n == "data-b"));
    assert!(names.iter().any(|n| n == "id"));
}

#[test]
fn set_title_writes_through_to_the_title_element() {
    let mut doc = parse_html("<html><head><title>old</title></head><body></body></html>");
    assert_eq!(doc.title(), "old");
    doc.set_title("new");
    assert_eq!(doc.title(), "new");
}

#[test]
fn inner_html_serialises_children_only() {
    let doc = parse_html(r#"<div id="d"><p>a</p><span>b</span></div>"#);
    let d = doc.get_element_by_id("d").unwrap();
    let html = doc.inner_html(d);
    assert!(
        html.contains("<p"),
        "innerHTML holds the children: {html:?}"
    );
    assert!(html.contains("span"));
    assert!(
        !html.starts_with("<div"),
        "and NOT the element itself: {html:?}"
    );
}

#[test]
fn document_kind_is_html_unless_asked_otherwise() {
    let doc = parse_html("<div></div>");
    assert_eq!(doc.kind(), crate::types::DocumentKind::Html);
}

#[test]
fn computed_style_property_resolves_geometry_after_layout() {
    let mut doc = parse_html(r#"<div id="d" style="width: 120px; height: 40px">x</div>"#);
    doc.set_viewport(800.0, 600.0);
    let d = doc.get_element_by_id("d").unwrap();

    // The DECLARED value is what was authored…
    assert_eq!(doc.get_style_property(d, "width").as_deref(), Some("120px"));
    // …and the COMPUTED value comes off the laid-out box.
    let w = doc.computed_style_property(d, "width");
    assert!(w.ends_with("px"), "computed width should be in px: {w:?}");
}

#[test]
fn get_elements_by_tag_name_is_case_insensitive_and_star_means_elements() {
    let doc = parse_html("<div><P>a</P><p>b</p><span>c</span></div>");

    // HTML tag matching is ASCII case-insensitive, so `<P>` and `<p>` are one
    // tag asked for either way.
    assert_eq!(doc.get_elements_by_tag_name("p").len(), 2);
    assert_eq!(doc.get_elements_by_tag_name("P").len(), 2);
    assert_eq!(doc.get_elements_by_tag_name("span").len(), 1);
    assert!(doc.get_elements_by_tag_name("table").is_empty());

    // `*` collects ELEMENTS — not the text nodes inside them.
    let all = doc.get_elements_by_tag_name("*");
    assert!(all.len() >= 4, "html/div/p/p/span at least: {}", all.len());
    assert!(
        all.iter().all(|id| doc.is_element(*id)),
        "`*` must not collect text or comment nodes"
    );
}

// ── The methods that arrived with the second engine ────────────────────────
//
// `vybe_widgets` carries the same tests under the same names, because the two
// engines are meant to be swappable and a shared SIGNATURE proves nothing
// about shared BEHAVIOUR. Anything asserted here should be asserted there.

#[test]
fn a_removed_node_survives_and_can_be_inserted_elsewhere() {
    // `removeChild` DETACHES; it does not destroy (DOM §4.2.3). Removing then
    // appending elsewhere is how every "move this node" is written, and it
    // read a freed arena slot until the node was kept.
    let mut doc = parse_html("<div id='from'><p id='moved'>text</p></div><div id='to'></div>");
    let from = doc.get_element_by_id("from").unwrap();
    let to = doc.get_element_by_id("to").unwrap();
    let moved = doc.get_element_by_id("moved").unwrap();

    doc.remove_child(moved);
    assert!(doc.child_nodes(from).is_empty(), "it left its old parent");
    assert_eq!(
        doc.local_name(moved),
        "p",
        "…and is still a <p>, not a dead slot"
    );

    doc.append_child(to, moved);
    let tags: Vec<String> = doc
        .child_nodes(to)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(tags, vec!["p"]);
    assert_eq!(doc.text_content(moved), "text", "the subtree came with it");
}

#[test]
fn appending_a_fragment_moves_its_children_and_not_itself() {
    let mut doc = parse_html("<div id='d'><i>keep</i></div>");
    let d = doc.get_element_by_id("d").unwrap();

    let fragment = doc.create_document_fragment();
    assert!(doc.is_document_fragment(fragment));
    assert_eq!(doc.node_type(fragment), 11);
    assert_eq!(doc.node_name(fragment), "#document-fragment");
    // A BLOCK and an inline — see the widgets twin of this test.
    for tag in ["div", "b"] {
        let child = doc.create_element(tag);
        doc.append_child(fragment, child);
    }

    doc.append_child(d, fragment);

    // The fragment is NOT in the tree — its children are, in order, and at the
    // level the caller asked for rather than one deeper.
    let tags: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(tags, vec!["i", "div", "b"]);
    assert!(
        !tags.iter().any(|t| t == "#document-fragment"),
        "the fragment itself must not land in the tree"
    );
    // …and it is empty afterwards, having handed its children over.
    assert!(doc.child_nodes(fragment).is_empty());
}

#[test]
fn inserting_a_fragment_splices_it_in_order() {
    let mut doc = parse_html("<div id='d'><i id='pivot'>z</i></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let pivot = doc.get_element_by_id("pivot").unwrap();

    let fragment = doc.create_document_fragment();
    // A BLOCK and an inline — see the widgets twin of this test.
    for tag in ["div", "b"] {
        let child = doc.create_element(tag);
        doc.append_child(fragment, child);
    }

    doc.insert_before(d, fragment, pivot);

    // Each child goes before the SAME reference, so they arrive in the order
    // they were in — not reversed.
    let tags: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(tags, vec!["div", "b", "i"]);
}

#[test]
fn normalize_merges_adjacent_text_and_drops_empties() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    for part in ["a", "", "b"] {
        let t = doc.create_text_node(part);
        doc.append_child(d, t);
    }
    let span = doc.create_element("span");
    doc.append_child(d, span);
    let tail = doc.create_text_node("c");
    doc.append_child(d, tail);

    doc.normalize(d);

    // "a" and "b" merge THROUGH the empty node between them — a zero-length
    // text node is removed, and removing it does not break adjacency. "c" is
    // separated by an element, so it stays its own node.
    let kinds: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| {
            if doc.is_text_node(c) {
                doc.text_data(c)
            } else {
                format!("<{}>", doc.local_name(c))
            }
        })
        .collect();
    assert_eq!(kinds, vec!["ab", "<span>", "c"]);
}

#[test]
fn is_equal_node_ignores_identity_and_attribute_order() {
    let doc = parse_html(
        "<div><p id='a' class='x'>hi</p><p class='x' id='a'>hi</p><p id='a'>bye</p></div>",
    );
    let ps = doc.get_elements_by_tag_name("p");
    assert_eq!(ps.len(), 3);

    // Two distinct nodes, written with their attributes in opposite orders.
    assert_ne!(ps[0], ps[1], "these must be different NODES");
    assert!(
        doc.is_equal_node(ps[0], ps[1]),
        "attribute order is not part of equality"
    );

    // Same attributes, different text.
    assert!(!doc.is_equal_node(ps[0], ps[2]));

    // A node always equals itself; nothing equals a node that is not there.
    assert!(doc.is_equal_node(ps[0], ps[0]));
    assert!(!doc.is_equal_node(ps[0], 0));
}

#[test]
fn compare_document_position_reports_containment_and_order() {
    let doc = parse_html("<div id='outer'><p id='first'>a</p><p id='second'>b</p></div>");
    let outer = doc.get_element_by_id("outer").unwrap();
    let first = doc.get_element_by_id("first").unwrap();
    let second = doc.get_element_by_id("second").unwrap();

    assert_eq!(doc.compare_document_position(outer, outer), 0);
    // CONTAINED_BY | FOLLOWING — containment always carries a direction.
    assert_eq!(doc.compare_document_position(outer, first), 0x10 | 0x04);
    // CONTAINS | PRECEDING, the exact mirror.
    assert_eq!(doc.compare_document_position(first, outer), 0x08 | 0x02);
    assert_eq!(doc.compare_document_position(first, second), 0x04);
    assert_eq!(doc.compare_document_position(second, first), 0x02);
}

#[test]
fn prepend_keeps_the_given_order() {
    let mut doc = parse_html("<div id='d'><i>z</i></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let a = doc.create_element("a");
    let b = doc.create_element("b");

    doc.prepend(d, &[a, b]);

    // The naive implementation — insert each before the CURRENT first child —
    // yields b, a, z. The nodes must arrive in the order they were given.
    let tags: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(tags, vec!["a", "b", "i"]);
}

#[test]
fn before_after_and_replace_with_place_nodes_around_a_sibling() {
    let mut doc = parse_html("<div id='d'><i id='pivot'>z</i></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let pivot = doc.get_element_by_id("pivot").unwrap();

    let a = doc.create_element("a");
    let b = doc.create_element("b");
    doc.before(pivot, &[a]);
    doc.after(pivot, &[b]);
    let tags: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(tags, vec!["a", "i", "b"]);

    let u = doc.create_element("u");
    doc.replace_with(pivot, &[u]);
    let tags: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(
        tags,
        vec!["a", "u", "b"],
        "the pivot is gone and `u` sits where it was"
    );
}

#[test]
fn insert_adjacent_element_places_at_all_four_positions() {
    let mut doc = parse_html("<div id='d'><p id='p'>x</p></div>");
    let d = doc.get_element_by_id("d").unwrap();
    let p = doc.get_element_by_id("p").unwrap();

    for (position, tag) in [
        ("beforebegin", "a"),
        ("afterbegin", "b"),
        ("beforeend", "u"),
        ("afterend", "s"),
    ] {
        let node = doc.create_element(tag);
        assert_eq!(
            doc.insert_adjacent_element(p, position, node),
            Some(node),
            "{position} should return the inserted element"
        );
    }

    // `beforebegin`/`afterend` are p's SIBLINGS…
    let outer: Vec<String> = doc
        .child_nodes(d)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(outer, vec!["a", "p", "s"]);
    // …and `afterbegin`/`beforeend` are its children, around the text.
    let inner: Vec<String> = doc
        .child_nodes(p)
        .iter()
        .map(|&c| doc.local_name(c))
        .collect();
    assert_eq!(inner, vec!["b", "#text", "u"]);

    assert_eq!(doc.insert_adjacent_element(p, "sideways", d), None);
}

#[test]
fn dataset_maps_camel_case_to_data_attributes() {
    let mut doc = parse_html("<div id='d' data-user-name='ada' data-id='7' class='c'></div>");
    let d = doc.get_element_by_id("d").unwrap();

    // The attribute is `data-user-name`; the IDL name is `userName`.
    assert_eq!(doc.dataset_get(d, "userName").as_deref(), Some("ada"));
    assert_eq!(doc.dataset_get(d, "id").as_deref(), Some("7"));
    // `class` is not `data-*` and must not appear.
    assert_eq!(
        doc.dataset(d),
        vec![
            ("id".to_string(), "7".to_string()),
            ("userName".to_string(), "ada".to_string()),
        ]
    );

    doc.set_dataset(d, "roleName", "admin");
    assert_eq!(
        doc.get_attribute(d, "data-role-name").as_deref(),
        Some("admin")
    );
    doc.remove_dataset(d, "roleName");
    assert_eq!(doc.dataset_get(d, "roleName"), None);
}

#[test]
fn inner_text_skips_hidden_subtrees() {
    let doc = parse_html(
        "<div id='d'><p>shown</p><p style='display:none'>hidden</p><span>tail</span></div>",
    );
    let d = doc.get_element_by_id("d").unwrap();

    let text = doc.inner_text(d);
    assert!(text.contains("shown"), "got {text:?}");
    assert!(text.contains("tail"), "got {text:?}");
    assert!(
        !text.contains("hidden"),
        "innerText is the RENDERED text — this is the whole difference from textContent: {text:?}"
    );
    // …and textContent still reports everything, unchanged.
    assert!(doc.text_content(d).contains("hidden"));
}

#[test]
fn tab_index_defaults_by_element_kind() {
    let doc = parse_html(
        "<div id='d'></div><button id='b'></button><a id='bare'>x</a><a id='link' href='/'>y</a><i id='t' tabindex='3'></i>",
    );
    let by = |id: &str| doc.tab_index(doc.get_element_by_id(id).unwrap());

    assert_eq!(by("d"), -1, "a div is not in the tab sequence by default");
    assert_eq!(by("b"), 0, "a button is");
    assert_eq!(by("bare"), -1, "an anchor with nowhere to go is not");
    assert_eq!(by("link"), 0, "an anchor with an href is");
    assert_eq!(by("t"), 3, "an explicit tabindex wins over any default");
}

#[test]
fn outer_html_includes_the_element_itself() {
    let doc = parse_html("<div id='d'><p>x</p></div>");
    let d = doc.get_element_by_id("d").unwrap();

    let inner = doc.inner_html(d);
    let outer = doc.outer_html(d);
    assert!(inner.contains("<p>"), "got {inner:?}");
    assert!(
        !inner.contains("<div"),
        "innerHTML is the CHILDREN only: {inner:?}"
    );
    assert!(
        outer.contains("<div"),
        "outerHTML includes the element: {outer:?}"
    );
    assert!(outer.contains("<p>"), "…and its children: {outer:?}");
}

#[test]
fn has_and_remove_attribute_ns_match_by_local_name() {
    let mut doc = parse_html("<div id='d'></div>");
    let d = doc.get_element_by_id("d").unwrap();
    doc.set_attribute_ns(d, "http://www.w3.org/1999/xlink", "xlink:href", "/there");
    doc.set_attribute(d, "href", "/here");

    assert!(doc.has_attribute_ns(d, "http://www.w3.org/1999/xlink", "href"));
    assert!(!doc.has_attribute_ns(d, "urn:other", "href"));

    doc.remove_attribute_ns(d, "http://www.w3.org/1999/xlink", "href");
    assert!(!doc.has_attribute_ns(d, "http://www.w3.org/1999/xlink", "href"));
    assert_eq!(
        doc.get_attribute(d, "href").as_deref(),
        Some("/here"),
        "the un-namespaced attribute shares a local name and must survive"
    );
}

#[test]
fn window_device_pixel_ratio_defaults_and_can_be_set_by_host() {
    let w = crate::window::open("dpr-test", "width=320,height=200");
    assert_eq!(
        crate::window::device_pixel_ratio(w),
        1.0,
        "a headless/default browsing context reports a standard 1x ratio"
    );

    crate::window::set_device_pixel_ratio(w, 2.0);
    assert_eq!(crate::window::device_pixel_ratio(w), 2.0);

    crate::window::set_device_pixel_ratio(w, 0.0);
    assert_eq!(
        crate::window::device_pixel_ratio(w),
        2.0,
        "invalid host ratios are ignored"
    );
}

#[test]
fn window_visual_viewport_reports_layout_viewport_and_scroll() {
    let w = crate::window::open("vv-test", "width=320,height=200");
    let doc_id = crate::window::document(w).unwrap();
    crate::dom::registry::with_document(doc_id, |doc| {
        doc.scroll_x = 12.0;
        doc.scroll_y = 34.0;
    });

    let vv = crate::window::visual_viewport(w).expect("open window has a visual viewport");
    assert_eq!(vv.width, 320.0);
    assert_eq!(vv.height, 200.0);
    assert_eq!(vv.scale, 1.0);
    assert_eq!(vv.offset_left, 0.0);
    assert_eq!(vv.offset_top, 0.0);
    assert_eq!(vv.page_left, 12.0);
    assert_eq!(vv.page_top, 34.0);

    crate::window::resize_to(w, 480.0, 240.0);
    let vv = crate::window::visual_viewport(w).unwrap();
    assert_eq!(vv.width, 480.0);
    assert_eq!(vv.height, 240.0);

    crate::window::close(w);
    assert_eq!(crate::window::visual_viewport(w), None);
}

#[test]
fn scoped_query_selector_matches_scope_root() {
    let doc = parse_html("<section id='scope'><p id='a'></p><div><p id='b'></p></div></section>");
    let scope_id = doc.get_element_by_id("scope").unwrap();
    let scope = doc.get_box_by_id(scope_id).unwrap();

    let scoped = crate::dom::query::matching_ids_from(scope, ":scope > p", false);
    assert_eq!(
        scoped,
        vec![doc.get_element_by_id("a").unwrap()],
        ":scope should name the subtree root, not every descendant"
    );

    let root_only = crate::dom::query::matching_ids_from(scope, ":scope", false);
    assert_eq!(root_only, vec![scope_id]);
}

#[test]
fn document_query_selector_has_url_state_pseudo_classes() {
    let doc = crate::html::parse_html_with_base(
        "<section id=outer><p id=hit>target</p></section>\
         <a id=local href=\"#hit\">local</a>\
         <a id=remote href=\"https://example.test/other.html#hit\">remote</a>",
        "https://example.test/page.html#hit",
    );

    assert_eq!(
        doc.query_selector(":target").unwrap(),
        doc.get_element_by_id("hit").unwrap()
    );
    assert_eq!(
        doc.query_selector("section:target-within").unwrap(),
        doc.get_element_by_id("outer").unwrap()
    );
    assert_eq!(
        doc.query_selector("a:local-link").unwrap(),
        doc.get_element_by_id("local").unwrap()
    );
    assert_eq!(
        doc.query_selector_all("a:local-link"),
        vec![doc.get_element_by_id("local").unwrap()]
    );
}

#[test]
fn svg_animation_dom_controls_target_projected_animation_node() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="click" dur="2s"/>
             </rect>
           </svg>"#,
    );
    let animate = doc.get_element_by_id("move").unwrap();
    assert!(doc.svg_begin_element(animate));
    assert!(doc.svg_end_element_at(animate, 1.0));

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 2);
    assert_eq!(root.svg_animation_controls[0].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[0].1, "begin");
    assert_eq!(root.svg_animation_controls[1].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[1].1, "end");
}

#[test]
fn svg_click_event_starts_matching_smil_animation() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="click" dur="2s"/>
             </rect>
           </svg>"#,
    );
    let rect = doc.get_element_by_id("box").unwrap();
    doc.fire_element_click(rect);

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 1);
    assert_eq!(root.svg_animation_controls[0].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[0].1, "begin");
}

#[test]
fn svg_mouseover_event_starts_matching_smil_animation() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="mouseover" dur="2s"/>
             </rect>
           </svg>"#,
    );
    let rect = doc.get_element_by_id("box").unwrap();
    assert!(doc.svg_trigger_event(rect, "mouseover"));

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 1);
    assert_eq!(root.svg_animation_controls[0].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[0].1, "begin");
}

#[test]
fn svg_dispatch_event_starts_matching_smil_animation() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="custom" dur="2s"/>
             </rect>
           </svg>"#,
    );
    let rect = doc.get_element_by_id("box").unwrap();
    let mut event = crate::dom::events::DomEvent::new("custom", rect);
    assert!(doc.dispatch_event(&mut event));

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 1);
    assert_eq!(root.svg_animation_controls[0].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[0].1, "begin");
}

#[test]
fn svg_begin_element_at_preserves_negative_offset() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="indefinite" dur="2s"/>
             </rect>
           </svg>"#,
    );
    let animate = doc.get_element_by_id("move").unwrap();
    assert!(doc.svg_begin_element_at(animate, -0.5));

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 1);
    assert!(root.svg_animation_controls[0].2 < 0.0);
}

#[test]
fn svg_access_key_animation_starts_from_keydown() {
    let mut doc = parse_html(
        r#"<svg id="icon" viewBox="0 0 10 10">
             <rect id="box" x="0" width="10" height="10">
               <animate id="move" attributeName="x" values="0;10" begin="accessKey(m)" dur="2s"/>
             </rect>
           </svg>"#,
    );

    assert!(doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'm' as u32,
        Some('m'),
        false,
        false,
        false,
        false,
    ));

    let svg = doc.get_element_by_id("icon").unwrap();
    let root = doc.get_box_by_id(svg).unwrap();
    assert_eq!(root.svg_animation_controls.len(), 1);
    assert_eq!(root.svg_animation_controls[0].0, vec![0, 0]);
    assert_eq!(root.svg_animation_controls[0].1, "begin");
}
