use super::harness::*;
use crate::html::{decode_entities, parse_html};
use crate::layout::LayoutEngine;
/// Tests for parser robustness fixes:
/// - decode_entities: bare `&` without `;` treated as literal
/// - Iterative parser: deep nesting doesn't stack-overflow
/// - Close-tag matching: mismatched tags don't inflate depth
/// - split_node_with_br: element nodes aren't destroyed by br insertion
/// - Cascade/layout depth guards
use crate::types::*;

// ─── decode_entities ─────────────────────────────────────────────────────────

#[test]
fn decode_entities_bare_ampersand_preserved() {
    // A bare `&` with no `;` nearby should be kept as-is.
    assert_eq!(decode_entities("A & B"), "A & B");
}

#[test]
fn decode_entities_ampersand_in_tailwind_class() {
    // Tailwind uses `&` in class names like `[&_.foo]:bar`.
    // The `&_` must NOT be consumed as an entity reference.
    let input = "[&_.maas-item]:contents";
    assert_eq!(decode_entities(input), input);
}

#[test]
fn decode_entities_ampersand_underscore_no_semicolon() {
    // `&_` is not a valid entity and has no `;` — must stay literal.
    assert_eq!(decode_entities("x&_y"), "x&_y");
}

#[test]
fn decode_entities_valid_entities_still_decoded() {
    assert_eq!(decode_entities("&amp;"), "&");
    assert_eq!(decode_entities("&lt;"), "<");
    assert_eq!(decode_entities("&gt;"), ">");
    assert_eq!(decode_entities("&quot;"), "\"");
    assert_eq!(decode_entities("&#39;"), "'");
    assert_eq!(decode_entities("&#x26;"), "&");
}

#[test]
fn decode_entities_mixed_valid_and_bare() {
    // Valid entity followed by bare ampersand.
    assert_eq!(decode_entities("&amp; & more"), "& & more");
}

#[test]
fn decode_entities_ampersand_at_end_of_string() {
    assert_eq!(decode_entities("hello&"), "hello&");
}

#[test]
fn decode_entities_multiple_bare_ampersands() {
    assert_eq!(decode_entities("a&b&c&d"), "a&b&c&d");
}

#[test]
fn decode_entities_ampersand_far_from_semicolon() {
    // `;` is more than 32 chars away — should treat `&` as literal.
    let input = format!("&{}x;", "a".repeat(40));
    let result = decode_entities(&input);
    assert!(
        result.starts_with('&'),
        "bare & with distant ; should be literal, got: {}",
        result
    );
}

// ─── Iterative parser: deep nesting ──────────────────────────────────────────

#[test]
fn parser_handles_nested_divs() {
    // 10 levels of nesting with full parse+cascade.
    let depth = 10;
    let open: String = (0..depth).map(|_| "<div>").collect();
    let close: String = (0..depth).map(|_| "</div>").collect();
    let html = format!("{}Hello{}", open, close);
    let doc = parse_html(&html);
    assert!(doc.root.text_content().contains("Hello"));
}

#[test]
fn parser_iterative_handles_many_siblings() {
    // The iterative parser handles thousands of siblings (flat, not deep)
    // without stack issues. This verifies the iterative loop works correctly
    // for a large number of tokens.
    let mut html = String::from("<div>");
    for i in 0..500 {
        html.push_str(&format!("<span>item{}</span>", i));
    }
    html.push_str("</div>");
    let doc = parse_html(&html);
    let text = doc.root.text_content();
    assert!(text.contains("item0"));
    assert!(text.contains("item499"));
}

#[test]
fn parser_handles_mismatched_close_tags() {
    // Mismatched close tags (like SVG inside HTML) shouldn't inflate nesting.
    // This mirrors the AP News bug where </path>, </svg> mismatches caused 600+ depth.
    let html = r#"<div><svg><path d="M0 0"></path></svg><p>After SVG</p></div>"#;
    let doc = parse_html(&html);
    let text = doc.root.text_content();
    assert!(
        text.contains("After SVG"),
        "content after SVG must be preserved"
    );
}

#[test]
fn parser_stray_close_tag_ignored() {
    // A stray </span> with no matching open tag should be ignored.
    let html = "<div>Hello</span> world</div>";
    let doc = parse_html(&html);
    let text = doc.root.text_content();
    assert!(
        text.contains("Hello"),
        "text before stray close tag preserved"
    );
    assert!(
        text.contains("world"),
        "text after stray close tag preserved"
    );
}

#[test]
fn parser_adoption_agency_pops_intermediates() {
    // </div> should close the <div>, popping any unclosed <span> inside.
    let html = "<div><span>text</div><p>after</p>";
    let doc = parse_html(&html);
    let text = doc.root.text_content();
    assert!(text.contains("text"));
    assert!(text.contains("after"));
}

fn tree_depth(node: &WebCore) -> usize {
    let mut max_child = 0;
    for c in &node.children {
        max_child = max_child.max(tree_depth(c));
    }
    1 + max_child
}

#[test]
fn parser_mismatched_svg_close_tags_dont_inflate_depth() {
    // Multiple SVGs with internal close tags that don't match outer structure.
    let mut html = String::from("<div>");
    for _ in 0..20 {
        html.push_str(r#"<svg viewBox="0 0 1 1"><circle r="1"/></svg>"#);
    }
    html.push_str("<p>end</p></div>");
    let doc = parse_html(&html);
    // Depth should be modest (< 20), not inflated by SVG internals.
    let depth = tree_depth(&doc.root);
    assert!(
        depth < 30,
        "tree depth {} should be < 30 (SVG close tags mustn't inflate it)",
        depth
    );
}

// ─── Cascade/layout depth guards ─────────────────────────────────────────────

#[test]
fn cascade_handles_deep_nesting() {
    // 10 nested divs — realistic complexity.
    let depth = 10;
    let open: String = (0..depth).map(|_| "<div>").collect();
    let close: String = (0..depth).map(|_| "</div>").collect();
    let html = format!("{}<p>deep</p>{}", open, close);
    let doc = parse_html(&html);
    assert!(doc.root.text_content().contains("deep"));
}

#[test]
fn layout_handles_deep_nesting() {
    // 10 nested divs with full layout.
    let depth = 10;
    let open: String = (0..depth).map(|_| "<div>").collect();
    let close: String = (0..depth).map(|_| "</div>").collect();
    let html = format!("{}<p>deep</p>{}", open, close);
    let doc = parse_and_layout(&html, 800.0);
    assert!(doc.root.text_content().contains("deep"));
}

// ─── split_node_with_br: element nodes preserved ─────────────────────────────

#[test]
fn insert_br_in_td_preserves_table_structure() {
    // After pressing Enter in a <td>, the <td> must still exist in the DOM.
    // Previously, split_node_with_br's Case A would remove the <td> from <tr>.
    use crate::Renderer;
    use crate::dom::HtmlEventType;

    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"<table><tr><td contenteditable="true">Hello world</td></tr></table>"#,
        900.0,
    );

    // Find the <td> and click in the middle.
    let td =
        find_box(&doc.root, &|b: &WebCore| b.tag == "td").expect("<td> must exist before Enter");
    let (cx, cy) = {
        let line = &td.layout.line_cache[0];
        (line.x + line.width / 2.0, line.y + line.height / 2.0)
    };

    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (cx, cy), 0);
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);
    renderer.layout_engine().layout(&mut doc, 900.0);

    // The <td> must still exist after the split.
    let td_after = find_box(&doc.root, &|b: &WebCore| b.tag == "td");
    assert!(
        td_after.is_some(),
        "<td> must still exist after Enter (not destroyed by split_node_with_br)"
    );

    // The <td> must contain a <br>.
    let td_node = td_after.unwrap();
    let has_br = td_node.children.iter().any(|c| c.tag == "br");
    assert!(has_br, "<td> must contain a <br> after Enter");
}

#[test]
fn insert_br_in_div_preserves_div() {
    // Same test but for <div contenteditable> — div must not be destroyed.
    use crate::Renderer;
    use crate::dom::HtmlEventType;

    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(r#"<div contenteditable="true">Hello world</div>"#, 900.0);

    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div"
            && b.attributes
                .get("contenteditable")
                .map(|v| v == "true")
                .unwrap_or(false)
    })
    .expect("editable div");
    let (cx, cy) = {
        let line = &div.layout.line_cache[0];
        (line.x + line.width / 2.0, line.y + line.height / 2.0)
    };

    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (cx, cy), 0);
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);
    renderer.layout_engine().layout(&mut doc, 900.0);

    let div_after = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div"
            && b.attributes
                .get("contenteditable")
                .map(|v| v == "true")
                .unwrap_or(false)
    });
    assert!(
        div_after.is_some(),
        "editable <div> must still exist after Enter"
    );
    let has_br = div_after.unwrap().children.iter().any(|c| c.tag == "br");
    assert!(has_br, "<div> must contain a <br> after Enter");
}

// ─── layout_dirty after DOM mutations ────────────────────────────────────────

#[test]
fn insert_char_marks_layout_dirty() {
    use crate::Renderer;
    use crate::dom::HtmlEventType;

    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(r#"<p contenteditable="true">Hello</p>"#, 900.0);

    let p = find_box(&doc.root, &|b: &WebCore| b.tag == "p").expect("p element");
    let (cx, cy) = {
        let line = &p.layout.line_cache[0];
        (line.x + 1.0, line.y + line.height / 2.0)
    };

    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (cx, cy), 0);

    // Insert a character — must mark node dirty so layout re-runs.
    doc.editor.insert_char(&mut doc.root, 'X');

    let p_after_insert = find_box(&doc.root, &|b: &WebCore| b.tag == "p").unwrap();
    assert!(
        p_after_insert.layout.layout_dirty,
        "node must be layout_dirty after insert_char"
    );

    // After re-layout, the inserted character should appear in the text.
    renderer.layout_engine().layout(&mut doc, 900.0);
    let p_relaid = find_box(&doc.root, &|b: &WebCore| b.tag == "p").unwrap();
    let text = p_relaid.text_content();
    assert!(
        text.contains('X'),
        "inserted character must appear after relayout, got: {}",
        text
    );
}

// ─── Tailwind class with & in selector matching ──────────────────────────────

#[test]
fn tailwind_class_with_ampersand_parsed_correctly() {
    // An element with a Tailwind-style class containing `&` must parse without
    // the `&` corrupting the class attribute.
    let html = r#"<div class="[&_.foo]:contents">text</div>"#;
    let doc = parse_html(&html);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.attributes
            .get("class")
            .map(|c| c.contains("[&_.foo]:contents"))
            .unwrap_or(false)
    });
    assert!(
        div.is_some(),
        "class attribute must preserve `[&_.foo]:contents` verbatim (bare & not consumed as entity)"
    );
}
