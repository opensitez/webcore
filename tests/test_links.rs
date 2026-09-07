// Tests for link parsing and hit-testing.

use webcore::layout::hit_test::*;
use webcore::layout::LayoutEngine;
use webcore::parse_html;
use webcore::types::*;

fn layout(html: &str, width: f32) -> Document {
    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, width);
    doc
}

#[test]
fn link_parsing_href() {
    let doc = parse_html(r#"<a href="http://example.com">Click</a>"#);
    let a = webcore::dom::query_selector(&doc.root, "a").expect("a tag not found");
    assert_eq!(a.tag, "a");
    assert_eq!(a.attributes.get("href").unwrap(), "http://example.com");
}

#[test]
fn link_hit_test() {
    let doc = layout(
        r#"<p><a href="http://example.com">Link Text</a></p>"#,
        800.0,
    );

    // Find the link text position
    let text = doc.root.text_content();
    let pos = text.find("Link").unwrap();

    // offset_to_point requires (root, box_ptr, local_offset, scroll_x, scroll_y)
    // For simplicity, let's find the box containing the link
    let a_box = doc.root.query_selector_all("a")[0];
    let pt = offset_to_point(&doc.root, a_box.node_id, 0, 0.0, 0.0).unwrap();

    let url = hit_test_link(&doc.root, (pt.0 + 2.0, pt.1 + 2.0), 0);
    assert_eq!(url, Some("http://example.com".to_string()));
}

#[test]
fn link_hit_test_no_link() {
    let doc = layout("<p>No link here</p>", 800.0);
    let url = hit_test_link(&doc.root, (10.0, 10.0), 0);
    assert!(url.is_none());
}

// ============================================================
// Link Parsing
// ============================================================

#[test]
fn link_anchor_text_preserved() {
    let doc = parse_html(r#"<a href="http://example.com">Link Text</a>"#);
    let text = doc.root.text_content();
    assert!(
        text.contains("Link Text"),
        "anchor text must be in the document text"
    );
}

#[test]
fn link_anchor_is_inline_in_paragraph() {
    let doc = parse_html(r##"<p><a href="#">Link</a> text</p>"##);
    use webcore::dom::query_selector;
    let p = query_selector(&doc.root, "p").expect("p not found");
    // The anchor should be a child of the paragraph
    let has_a = p.children.iter().any(|c| c.tag == "a");
    assert!(has_a, "<a> should be a child of <p>");
}

#[test]
fn link_multiple_links_parsed() {
    let doc = parse_html(r#"<p><a href="http://a.com">A</a> and <a href="http://b.com">B</a></p>"#);
    use webcore::dom::query_selector_all;
    let links = query_selector_all(&doc.root, "a");
    assert!(
        links.len() >= 2,
        "expected at least 2 anchor elements; got {}",
        links.len()
    );
    let hrefs: Vec<_> = links
        .iter()
        .filter_map(|a| a.attributes.get("href"))
        .collect();
    assert!(
        hrefs.iter().any(|h| h.contains("a.com")),
        "first href should be a.com"
    );
    assert!(
        hrefs.iter().any(|h| h.contains("b.com")),
        "second href should be b.com"
    );
}

#[test]
fn link_nested_link_in_div() {
    let doc = parse_html(r#"<div><a href="http://example.com">Nested</a></div>"#);
    use webcore::dom::query_selector;
    let a = query_selector(&doc.root, "a").expect("a not found inside div");
    assert_eq!(
        a.attributes.get("href").map(String::as_str),
        Some("http://example.com")
    );
}

// ============================================================
// Link Serialization
// ============================================================

#[test]
fn link_serialization_preserves_href() {
    let doc = parse_html(r#"<p><a href="http://example.com">Link</a></p>"#);
    let html = webcore::html::serialize_html(&doc);
    assert!(
        html.contains("http://example.com"),
        "serialized HTML must contain the href URL"
    );
    assert!(
        html.contains("href"),
        "serialized HTML must contain the 'href' attribute"
    );
}

#[test]
fn link_round_trip_preserves_href() {
    let original = r#"<p><a href="http://example.com">Click</a></p>"#;
    let doc = parse_html(original);
    let serialized = webcore::html::serialize_html(&doc);
    // Re-parse the serialized HTML
    let doc2 = parse_html(&serialized);
    use webcore::dom::query_selector;
    let a = query_selector(&doc2.root, "a").expect("anchor not found after round-trip");
    assert_eq!(
        a.attributes.get("href").map(String::as_str),
        Some("http://example.com"),
        "href must survive serialization/parse round-trip"
    );
}

// ============================================================
// Link Hit-Testing
// ============================================================

#[test]
fn link_hit_test_smoke() {
    // Clicking somewhere in the link area must not panic.
    // If a URL is returned it must match the expected one.
    let doc = layout(
        r#"<p><a href="http://example.com">Link text here</a></p>"#,
        800.0,
    );
    let url = hit_test_link(&doc.root, (30.0, 8.0), 0);
    if let Some(u) = url {
        assert_eq!(u, "http://example.com");
    }
    // Smoke: must not crash regardless
}

#[test]
fn link_hit_test_box_at_smoke() {
    let doc = layout(
        r#"<div style="width: 200px; height: 100px;">Box</div>"#,
        800.0,
    );
    let nid = hit_test_box_at(&doc.root, (100.0, 50.0), 0);
    // hit_test_box_at always returns at least the root — never 0
    assert!(nid != 0, "hit_test_box_at must return a non-zero node_id");
}

#[test]
fn link_hit_test_box_at_empty_doc() {
    // Empty document: hit_test_box_at must not crash and must return at least root.
    let doc = layout("", 800.0);
    let nid = hit_test_box_at(&doc.root, (0.0, 0.0), 0);
    assert!(nid != 0, "hit_test_box_at on empty doc must not return 0");
}

// ============================================================
// PointToOffset (via point_to_hit)
// ============================================================

#[test]
fn link_point_to_offset_smoke() {
    let doc = layout("<p>Hello World</p>", 800.0);
    // A hit must be found for a point within the content area
    let hit = point_to_hit(&doc.root, (0.0, 5.0), 0);
    // Should return Some; local_offset must be within the paragraph text length
    assert!(
        hit.is_some(),
        "point_to_hit must return Some for a point inside content"
    );
    let h = hit.unwrap();
    // node_id must be valid
    assert!(h.node_id != 0, "hit must return a valid node_id");
}

#[test]
fn link_point_to_offset_out_of_bounds() {
    // A point far below all content — must not panic and must return Some or None
    let doc = layout("<p>Text</p>", 800.0);
    // No panic is the main assertion; result may be Some or None
    let _hit = point_to_hit(&doc.root, (0.0, 9999.0), 0);
}

// ============================================================
// OffsetToPoint (via offset_to_point)
// ============================================================

#[test]
fn link_offset_to_point_smoke() {
    let doc = layout("<p>Hello</p>", 800.0);
    use webcore::dom::query_selector;
    let p = query_selector(&doc.root, "p").unwrap();
    let pt = offset_to_point(&doc.root, p.node_id, 0, 0.0, 0.0);
    assert!(
        pt.is_some(),
        "offset_to_point must return Some for offset 0"
    );
    let (x, y) = pt.unwrap();
    assert!(x >= 0.0, "x must be non-negative");
    assert!(y >= 0.0, "y must be non-negative");
}

#[test]
fn link_offset_to_point_at_end() {
    let doc = layout("<p>Hello</p>", 800.0);
    use webcore::dom::query_selector;
    let p = query_selector(&doc.root, "p").unwrap();
    let text_len = p.text_content().len();
    // Must not panic for offset at end of text
    let pt = offset_to_point(&doc.root, p.node_id, text_len, 0.0, 0.0);
    // Should return a valid point
    if let Some((x, y)) = pt {
        assert!(
            x >= 0.0 || y >= 0.0,
            "end-of-text point must have non-negative coordinates"
        );
    }
}
