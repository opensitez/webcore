// Comprehensive editor operation tests.
//
// Covers: insert_char, multi-node delete (overwrite fix), insert_newline (Enter),
// insert_hr, insert_br, toggle_bullet_list, increase/decrease_indent,
// increase/decrease_quote_level.

use webcore::dom::{
    get_text_content, insert_hr, query_selector, query_selector_all, query_selector_mut,
    toggle_bold, Editor, TextRange,
};
use webcore::layout::LayoutEngine;
use webcore::parse_html;
use webcore::types::WebCore;

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_and_layout(html: &str) -> webcore::types::Document {
    let mut doc = parse_html(html);
    LayoutEngine::new().layout(&mut doc, 800.0);
    doc
}

/// Point the editor caret at `element` with the given flat-text offset.
/// # Safety
/// The pointer must remain valid (no tree mutation) until the next editor call.
fn set_caret(editor: &mut Editor, element: &WebCore, offset: usize) {
    editor.caret_box = Some(element.node_id);
    editor.collapse_to(offset);
}

/// Collect all text tags found in the document depth-first.
fn tags(root: &WebCore) -> Vec<String> {
    let mut v = Vec::new();
    collect_tags(root, &mut v);
    v
}
fn collect_tags(node: &WebCore, out: &mut Vec<String>) {
    out.push(node.tag.clone());
    for c in &node.children {
        collect_tags(c, out);
    }
}

// ── 1. insert_char: basic text insertion ──────────────────────────────────────

#[test]
fn insert_char_at_start() {
    let mut doc = parse_and_layout("<p>world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.insert_char(&mut doc.root, 'X');
    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "Xworld");
}

#[test]
fn insert_char_in_middle() {
    let mut doc = parse_and_layout("<p>Helo</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 3); // after "Hel"
    }
    doc.editor.insert_char(&mut doc.root, 'l');
    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "Hello");
}

#[test]
fn insert_char_at_end() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor.insert_char(&mut doc.root, '!');
    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "Hello!");
}

#[test]
fn insert_char_utf8_multibyte() {
    let mut doc = parse_and_layout("<p>caf</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 3);
    }
    doc.editor.insert_char(&mut doc.root, 'é'); // 2 bytes in UTF-8
    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "café");
    // Caret should advance by 2 bytes
    assert_eq!(doc.editor.caret_local, 5);
}

// ── 2. delete_range_full: multi-node selection delete (overwrite fix) ─────────

#[test]
fn delete_selection_replaces_single_node_text() {
    // Selection within a single text node — was working before, must still work.
    let mut doc = parse_and_layout("<p>Hello world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
        doc.editor.sel_start = 0;
        doc.editor.sel_end = 5; // select "Hello"
    }
    doc.editor.insert_char(&mut doc.root, 'X');
    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "X world");
}

#[test]
fn delete_selection_across_inline_element() {
    // Selecting from inside <b>Hello</b> into the trailing " world" text node.
    // Old delete_range only removed 1 char; new delete_range_full removes the full range.
    let mut doc = parse_and_layout("<p><b>Hello</b> world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
        doc.editor.sel_start = 3; // offset 3: inside "Hello" (after "Hel")
        doc.editor.sel_end = 8; // offset 8: " wo" — spans the node boundary
        doc.editor.caret_local = 8;
    }
    doc.editor.insert_char(&mut doc.root, 'X');
    let p = query_selector(&doc.root, "p").unwrap();
    // "Hel" + "X" + "rld" = "HelXrld"
    assert_eq!(get_text_content(p), "HelXrld");
}

#[test]
fn backspace_single_char() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor.delete_selection_or_before(&mut doc.root);
    assert_eq!(
        get_text_content(query_selector(&doc.root, "p").unwrap()),
        "Hell"
    );
    assert_eq!(doc.editor.caret_local, 4);
}

#[test]
fn delete_key_single_char() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.delete_selection_or_at(&mut doc.root);
    assert_eq!(
        get_text_content(query_selector(&doc.root, "p").unwrap()),
        "ello"
    );
    assert_eq!(doc.editor.caret_local, 0);
}

#[test]
fn delete_selection_multi_node_via_backspace() {
    // Select across the <b> boundary and backspace.
    let mut doc = parse_and_layout("<p><b>Hello</b> world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 8);
        doc.editor.sel_start = 4;
        doc.editor.sel_end = 8;
    }
    doc.editor.delete_selection_or_before(&mut doc.root);
    let p = query_selector(&doc.root, "p").unwrap();
    // "Hell" (offset 0-4) + "rld" (offset 8-11) → "Hellrld"
    assert_eq!(get_text_content(p), "Hellrld");
    assert_eq!(doc.editor.caret_local, 4);
}

// ── 3. insert_newline: Enter splits the block ─────────────────────────────────

#[test]
fn insert_newline_splits_paragraph_in_middle() {
    let mut doc = parse_and_layout("<p>Hello world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5); // after "Hello"
    }
    doc.editor.insert_newline(&mut doc.root);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2, "should have two <p> elements after Enter");
    assert_eq!(get_text_content(paras[0]), "Hello");
    assert_eq!(get_text_content(paras[1]), " world");
    // Caret should be at start of the new paragraph
    assert_eq!(doc.editor.caret_local, 0);
}

#[test]
fn insert_newline_at_start_creates_empty_paragraph_before() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.insert_newline(&mut doc.root);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2);
    assert_eq!(get_text_content(paras[0]), ""); // original paragraph now empty
    assert_eq!(get_text_content(paras[1]), "Hello");
}

#[test]
fn insert_newline_at_end_creates_empty_paragraph_after() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor.insert_newline(&mut doc.root);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2);
    assert_eq!(get_text_content(paras[0]), "Hello");
    assert_eq!(get_text_content(paras[1]), "");
}

#[test]
fn insert_newline_in_heading_preserves_tag() {
    let mut doc = parse_and_layout("<h2>Section title</h2>");
    {
        let h = query_selector_mut(&mut doc.root, "h2").unwrap();
        set_caret(&mut doc.editor, h, 7); // after "Section"
    }
    doc.editor.insert_newline(&mut doc.root);

    let headings: Vec<_> = query_selector_all(&doc.root, "h2");
    assert_eq!(headings.len(), 2, "should produce two <h2> elements");
    assert_eq!(get_text_content(headings[0]), "Section");
    assert_eq!(get_text_content(headings[1]), " title");
}

#[test]
fn insert_newline_in_list_item_creates_new_li() {
    let mut doc = parse_and_layout("<ul><li>Item one</li></ul>");
    {
        let li = query_selector_mut(&mut doc.root, "li").unwrap();
        set_caret(&mut doc.editor, li, 4); // after "Item"
    }
    doc.editor.insert_newline(&mut doc.root);

    let items: Vec<_> = query_selector_all(&doc.root, "li");
    assert_eq!(items.len(), 2, "should have two <li> elements");
    assert_eq!(get_text_content(items[0]), "Item");
    assert_eq!(get_text_content(items[1]), " one");
}

// ── 4. insert_hr ─────────────────────────────────────────────────────────────

#[test]
fn insert_hr_after_paragraph() {
    let mut doc = parse_and_layout("<p>Hello</p><p>World</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap(); // first p
        set_caret(&mut doc.editor, p, 0);
    }
    insert_hr(&doc.editor, &mut doc.root);

    // Body should now contain: p("Hello"), hr, p("World")
    let body = query_selector(&doc.root, "body").unwrap();
    let child_tags: Vec<&str> = body.children.iter().map(|c| c.tag.as_str()).collect();
    assert!(
        child_tags.contains(&"hr"),
        "body should contain an <hr>; children: {:?}",
        child_tags
    );
    // hr should come AFTER the first p
    let p_pos = child_tags.iter().position(|&t| t == "p").unwrap();
    let hr_pos = child_tags.iter().position(|&t| t == "hr").unwrap();
    assert!(hr_pos > p_pos, "<hr> should follow the first <p>");
}

#[test]
fn insert_hr_does_not_move_caret() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 3);
    }
    insert_hr(&doc.editor, &mut doc.root);
    // Caret offset must not change
    assert_eq!(doc.editor.caret_local, 3);
}

// ── 5. insert_br ─────────────────────────────────────────────────────────────

#[test]
fn insert_br_splits_text_within_paragraph() {
    let mut doc = parse_and_layout("<p>Hello world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor.insert_br(&mut doc.root);

    let p = query_selector(&doc.root, "p").unwrap();
    // The paragraph must now contain a <br> child
    let has_br = p.children.iter().any(|c| c.tag == "br");
    assert!(has_br, "<p> should contain a <br> after insert_br");
    // Text before br = "Hello", text after br = " world"
    let text_before = p
        .children
        .iter()
        .take_while(|c| c.tag != "br")
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .concat();
    let text_after = p
        .children
        .iter()
        .skip_while(|c| c.tag != "br")
        .skip(1) // skip the br itself
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .concat();
    assert_eq!(text_before, "Hello");
    assert_eq!(text_after, " world");
}

#[test]
fn insert_br_at_start() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.insert_br(&mut doc.root);

    let p = query_selector(&doc.root, "p").unwrap();
    assert!(p.children.iter().any(|c| c.tag == "br"));
    // All text should still be present
    assert!(get_text_content(p).contains("Hello"));
}

#[test]
fn insert_br_at_end() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor.insert_br(&mut doc.root);

    let p = query_selector(&doc.root, "p").unwrap();
    // A <br> must have been inserted somewhere
    assert!(
        p.children.iter().any(|c| c.tag == "br") || {
            // Might be appended as last child when caret is at very end
            let all_tags = tags(p);
            all_tags.contains(&"br".to_string())
        }
    );
    assert!(get_text_content(p).contains("Hello"));
}

// ── 6. toggle_bullet_list ─────────────────────────────────────────────────────

#[test]
fn toggle_bullet_wraps_paragraph_in_ul_li() {
    let mut doc = parse_and_layout("<div><p>Item</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.toggle_bullet_list(&mut doc.root);

    // The <div> should now contain <ul><li>
    let ul = query_selector(&doc.root, "ul");
    assert!(ul.is_some(), "document should have a <ul>");
    let li = query_selector(&doc.root, "li");
    assert!(li.is_some(), "document should have a <li>");
    assert_eq!(get_text_content(li.unwrap()), "Item");
}

#[test]
fn toggle_bullet_wraps_and_caret_moves_to_li() {
    let mut doc = parse_and_layout("<div><p>Item</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.toggle_bullet_list(&mut doc.root);

    // Caret box should now point inside the <li>
    if let Some(caret_id) = doc.editor.caret_box {
        let li = query_selector(&doc.root, "li").unwrap();
        assert!(caret_id == li.node_id, "caret should be on the <li>");
    } else {
        panic!("caret_box should be set after toggle_bullet_list");
    }
}

#[test]
fn toggle_bullet_unwraps_single_li_removes_ul() {
    // When the <ul> has only one item, unwrapping should remove the <ul> entirely.
    let mut doc = parse_and_layout("<div><ul><li>Only item</li></ul></div>");
    {
        let li = query_selector_mut(&mut doc.root, "li").unwrap();
        set_caret(&mut doc.editor, li, 0);
    }
    doc.editor.toggle_bullet_list(&mut doc.root);

    // No <ul> or <li> should remain
    assert!(
        query_selector(&doc.root, "ul").is_none(),
        "<ul> should be gone after toggle-off"
    );
    assert!(
        query_selector(&doc.root, "li").is_none(),
        "<li> should be gone after toggle-off"
    );
    // Text should still be accessible
    let p = query_selector(&doc.root, "p");
    assert!(p.is_some(), "should now have a <p>");
    assert_eq!(get_text_content(p.unwrap()), "Only item");
}

// ── 7. increase_indent / decrease_indent ─────────────────────────────────────

#[test]
fn increase_indent_adds_margin_left() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_indent(&mut doc.root, 40.0);

    let p = query_selector(&doc.root, "p").unwrap();
    let style_attr = p.attributes.get("style").cloned().unwrap_or_default();
    assert!(
        style_attr.contains("margin-left"),
        "style attribute should contain margin-left; got: {:?}",
        style_attr
    );
    // Computed style should reflect 40px
    match &p.style.margin_left {
        webcore::types::CssLength::Px(v) => assert!((*v - 40.0).abs() < 0.01),
        other => panic!("expected Px(40), got {:?}", other),
    }
}

#[test]
fn increase_indent_accumulates() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_indent(&mut doc.root, 40.0);
    doc.editor.increase_indent(&mut doc.root, 40.0);

    let p = query_selector(&doc.root, "p").unwrap();
    match &p.style.margin_left {
        webcore::types::CssLength::Px(v) => assert!((*v - 80.0).abs() < 0.01),
        other => panic!("expected Px(80), got {:?}", other),
    }
}

#[test]
fn decrease_indent_reduces_margin_left() {
    let mut doc = parse_and_layout("<p style=\"margin-left: 80px\">Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.decrease_indent(&mut doc.root, 40.0);

    let p = query_selector(&doc.root, "p").unwrap();
    match &p.style.margin_left {
        webcore::types::CssLength::Px(v) => assert!((*v - 40.0).abs() < 0.01),
        other => panic!("expected Px(40), got {:?}", other),
    }
}

#[test]
fn decrease_indent_does_not_go_below_zero() {
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.decrease_indent(&mut doc.root, 40.0);

    let p = query_selector(&doc.root, "p").unwrap();
    match &p.style.margin_left {
        webcore::types::CssLength::Px(v) => assert!(*v >= 0.0, "margin-left must not go negative"),
        webcore::types::CssLength::Zero => {} // ok
        other => panic!("unexpected {:?}", other),
    }
}

#[test]
fn indent_survives_recascade() {
    // margin-left set via increase_indent must be stored in the style attribute
    // and survive a CSS re-cascade.
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_indent(&mut doc.root, 40.0);
    doc.recascade();

    let p = query_selector(&doc.root, "p").unwrap();
    match &p.style.margin_left {
        webcore::types::CssLength::Px(v) => assert!((*v - 40.0).abs() < 0.01),
        other => panic!("indent should survive recascade; got {:?}", other),
    }
}

// ── 8. increase_quote_level / decrease_quote_level ───────────────────────────

#[test]
fn increase_quote_level_wraps_in_blockquote() {
    let mut doc = parse_and_layout("<div><p>Hello</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_quote_level(&mut doc.root);

    let bq = query_selector(&doc.root, "blockquote");
    assert!(bq.is_some(), "document should have a <blockquote>");
    let p_inside = query_selector(bq.unwrap(), "p");
    assert!(p_inside.is_some(), "<p> should be inside <blockquote>");
    assert_eq!(get_text_content(p_inside.unwrap()), "Hello");
}

#[test]
fn increase_quote_level_twice_nests_blockquotes() {
    let mut doc = parse_and_layout("<div><p>Hello</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_quote_level(&mut doc.root);
    doc.editor.increase_quote_level(&mut doc.root);

    // Should have two nested blockquotes
    let outer = query_selector(&doc.root, "blockquote").unwrap();
    let inner = query_selector(outer, "blockquote");
    assert!(inner.is_some(), "should have nested <blockquote>");
    assert_eq!(
        get_text_content(query_selector(inner.unwrap(), "p").unwrap()),
        "Hello"
    );
}

#[test]
fn decrease_quote_level_unwraps_blockquote() {
    let mut doc = parse_and_layout("<div><blockquote><p>Hello</p></blockquote></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.decrease_quote_level(&mut doc.root);

    // The blockquote should be gone; <p> should be directly inside <div>
    assert!(
        query_selector(&doc.root, "blockquote").is_none(),
        "<blockquote> should be removed after decrease_quote_level"
    );
    let div = query_selector(&doc.root, "div").unwrap();
    let p = query_selector(div, "p");
    assert!(p.is_some(), "<p> should still exist inside <div>");
    assert_eq!(get_text_content(p.unwrap()), "Hello");
}

#[test]
fn decrease_quote_level_noop_when_not_in_blockquote() {
    let mut doc = parse_and_layout("<div><p>Hello</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    // Calling decrease_quote_level when there is no enclosing blockquote must not panic.
    doc.editor.decrease_quote_level(&mut doc.root);

    let p = query_selector(&doc.root, "p").unwrap();
    assert_eq!(get_text_content(p), "Hello");
}

#[test]
fn quote_roundtrip_increase_then_decrease() {
    let mut doc = parse_and_layout("<div><p>Hello</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_quote_level(&mut doc.root);
    doc.editor.decrease_quote_level(&mut doc.root);

    assert!(query_selector(&doc.root, "blockquote").is_none());
    assert_eq!(
        get_text_content(query_selector(&doc.root, "p").unwrap()),
        "Hello"
    );
}

// ── 9. insert_newline: non-prose containers get <br>, not a sibling ──────────

#[test]
fn enter_in_div_inserts_br_not_new_div() {
    // A <div> is a structural container — Enter must NOT create a sibling <div>.
    let mut doc = parse_and_layout("<div>Hello world</div>");
    {
        let d = query_selector_mut(&mut doc.root, "div").unwrap();
        set_caret(&mut doc.editor, d, 5); // after "Hello"
    }
    doc.editor.insert_newline(&mut doc.root);

    let divs: Vec<_> = query_selector_all(&doc.root, "div");
    assert_eq!(
        divs.len(),
        1,
        "Enter in a <div> must not create a second <div>; found: {}",
        divs.len()
    );
    // The <div> should now contain a <br>
    let has_br = tags(query_selector(&doc.root, "div").unwrap()).contains(&"br".to_string());
    assert!(has_br, "<div> should contain a <br> after Enter");
}

#[test]
fn enter_in_blockquote_inserts_br() {
    let mut doc = parse_and_layout("<blockquote>Hello world</blockquote>");
    {
        let bq = query_selector_mut(&mut doc.root, "blockquote").unwrap();
        set_caret(&mut doc.editor, bq, 5);
    }
    doc.editor.insert_newline(&mut doc.root);

    let bqs: Vec<_> = query_selector_all(&doc.root, "blockquote");
    assert_eq!(
        bqs.len(),
        1,
        "Enter in a <blockquote> must not create a second one"
    );
    let has_br = tags(bqs[0]).contains(&"br".to_string());
    assert!(has_br, "<blockquote> should contain a <br> after Enter");
}

// ── 10. handle_key_event: Enter dispatches to insert_newline ─────────────────

#[test]
fn key_enter_splits_paragraph() {
    use webcore::dom::HtmlEventType;
    let mut doc = parse_and_layout("<p>AB</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 1); // after "A"
    }
    let redraw = doc.editor.handle_key_event(
        &mut doc.root,
        HtmlEventType::KeyDown,
        13, // Enter
        None,
        false,
    );
    assert!(redraw, "Enter should signal a redraw");
    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2, "Enter should produce two paragraphs");
    assert_eq!(get_text_content(paras[0]), "A");
    assert_eq!(get_text_content(paras[1]), "B");
}

#[test]
fn key_enter_does_not_insert_literal_newline() {
    use webcore::dom::HtmlEventType;
    let mut doc = parse_and_layout("<p>Hello</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5);
    }
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);

    // There should now be two paragraphs; neither should contain '\n'
    let all_text = query_selector_all(&doc.root, "p")
        .iter()
        .map(|p| get_text_content(p))
        .collect::<Vec<_>>();
    for t in &all_text {
        assert!(
            !t.contains('\n'),
            "text must not contain a literal newline; got: {:?}",
            all_text
        );
    }
}

// ── 10. Formatting on ranges (existing API regression) ────────────────────────

#[test]
fn toggle_bold_on_range_after_layout() {
    let mut doc = parse_and_layout("<p>Hello world</p>");
    let p = query_selector_mut(&mut doc.root, "p").unwrap();
    let range = TextRange { start: 0, end: 5 };
    toggle_bold(p, &range);
    assert!(p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight.is_bold()));
}

// ── 11. Space then text in table cell and grid div ────────────────────────────

/// Typing a space then a letter in a table cell: space must be inserted at
/// caret position and the following letter must land AFTER the space.
#[test]
fn space_then_letter_in_table_cell() {
    let html = "<table><tr><td>Hi</td></tr></table>";
    let mut doc = parse_and_layout(html);

    // Place caret at end of the cell text (offset 2, after "Hi")
    {
        let td = query_selector_mut(&mut doc.root, "td").unwrap();
        set_caret(&mut doc.editor, td, 2);
    }

    // Press space
    doc.editor.insert_char(&mut doc.root, ' ');
    // Press 'X'
    doc.editor.insert_char(&mut doc.root, 'X');

    // Relayout so line_cache is fresh
    LayoutEngine::new().layout(&mut doc, 800.0);

    let td = query_selector(&doc.root, "td").unwrap();
    let text = get_text_content(td);
    assert_eq!(
        text, "Hi X",
        "space and letter must be inserted in order; got {:?}",
        text
    );
    // Caret must be just after the 'X' (offset 4)
    assert_eq!(doc.editor.caret_local, 4, "caret should be after 'X'");
}

/// Typing a space then a letter in a grid div: same guarantee.
#[test]
fn space_then_letter_in_grid_div() {
    let html = r#"<html><body><div id="cell">Cell</div></body></html>"#;
    let mut doc = parse_and_layout(html);

    // Place caret after "Cell".
    {
        let cell = query_selector_mut(&mut doc.root, "#cell").unwrap();
        set_caret(&mut doc.editor, cell, 4);
    }

    doc.editor.insert_char(&mut doc.root, ' ');
    doc.editor.insert_char(&mut doc.root, 'Z');

    LayoutEngine::new().layout(&mut doc, 800.0);

    let cell = query_selector(&doc.root, "#cell").unwrap();
    let text = get_text_content(cell);
    assert_eq!(text, "Cell Z", "space and letter in div; got {:?}", text);
    assert_eq!(doc.editor.caret_local, 6);
}

// ── 12. Arrow keys update caret_local; Enter splits at the moved position ─────

/// Two right-arrow presses then Enter must split the paragraph at the
/// byte offset that the arrows moved to — not at the original click position.
#[test]
fn arrow_right_twice_then_enter_splits_at_correct_offset() {
    use webcore::dom::HtmlEventType;
    let mut doc = parse_and_layout("<p>Hello world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0); // start at beginning
    }

    // Press right twice — caret_local should advance to 2
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 39, None, false);
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 39, None, false);
    assert_eq!(
        doc.editor.caret_local, 2,
        "two right presses must move caret to offset 2"
    );

    // Press Enter — paragraph should split at offset 2
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(
        paras.len(),
        2,
        "Enter after arrow must create two paragraphs"
    );
    assert_eq!(
        get_text_content(paras[0]),
        "He",
        "first para must contain text before split"
    );
    assert_eq!(
        get_text_content(paras[1]),
        "llo world",
        "second para must contain text after split"
    );
    assert_eq!(
        doc.editor.caret_local, 0,
        "caret must be at start of new paragraph"
    );
}

/// Arrow right into inline children then Enter: split must be at the correct
/// flat-text position, preserving text before and after the caret.
#[test]
fn arrow_right_into_inline_child_then_enter() {
    use webcore::dom::HtmlEventType;
    // flat text: "ABCDEF" (6 bytes)
    let mut doc = parse_and_layout("<p>AB<b>CD</b>EF</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }

    // Move past "A" and "B"
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 39, None, false);
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 39, None, false);
    assert_eq!(doc.editor.caret_local, 2);

    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2);
    assert_eq!(get_text_content(paras[0]), "AB");
    assert_eq!(get_text_content(paras[1]), "CDEF");
}

/// Arrow left then Enter: split must be at the position moved to, not the original.
#[test]
fn arrow_left_then_enter_splits_at_moved_position() {
    use webcore::dom::HtmlEventType;
    let mut doc = parse_and_layout("<p>Hello world</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 5); // after "Hello"
    }

    // Move one step left — caret should be at 4
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 37, None, false);
    assert_eq!(doc.editor.caret_local, 4);

    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);

    let paras: Vec<_> = query_selector_all(&doc.root, "p");
    assert_eq!(paras.len(), 2);
    assert_eq!(get_text_content(paras[0]), "Hell");
    assert_eq!(get_text_content(paras[1]), "o world");
}

// ── 13. Positioned element layout ─────────────────────────────────────────────

/// An absolutely-positioned child must receive non-zero layout coordinates
/// after layout, resolved against its containing block.
#[test]
fn absolute_child_gets_nonzero_layout() {
    let html = r#"<div style="position: relative; width: 200px; height: 100px;">
        <div id="abs" style="position: absolute; top: 10px; left: 20px; width: 50px; height: 30px;">x</div>
    </div>"#;
    let mut doc = parse_and_layout(html);
    LayoutEngine::new().layout(&mut doc, 800.0);

    let abs = query_selector(&doc.root, "#abs").unwrap();
    // After layout the positioned child must be placed at (20, 10) relative to
    // the containing block — meaning its border_rect.x and border_rect.y
    // should be non-zero and match the top/left offsets.
    assert!(
        abs.layout.border_rect.x > 0.0 || abs.layout.border_rect.y > 0.0,
        "absolute child must have non-zero position after layout; got border_rect={:?}",
        abs.layout.border_rect
    );
    // top: 10px, left: 20px — y should be >= 10, x >= 20 (plus any outer margins/padding)
    assert!(
        abs.layout.border_rect.y >= 9.0,
        "absolute child top should be ~10px; got y={}",
        abs.layout.border_rect.y
    );
    assert!(
        abs.layout.border_rect.x >= 19.0,
        "absolute child left should be ~20px; got x={}",
        abs.layout.border_rect.x
    );
}

/// Absolute children must NOT appear in the inline flow of their parent.
/// The parent's flat text must not include the absolute child's text.
#[test]
fn absolute_child_excluded_from_parent_flat_text() {
    use webcore::layout::inline_layout::collect_flat_text;
    let html = r#"<div id="rel" style="position: relative;">
        normal text
        <span id="abs" style="position: absolute; top: 0; left: 0;">abs text</span>
    </div>"#;
    let mut doc = parse_and_layout(html);

    let rel = query_selector(&doc.root, "#rel").unwrap();
    let flat = collect_flat_text(rel);
    assert!(
        !flat.contains("abs text"),
        "flat text of relative container must not include absolute child's text; got {:?}",
        flat
    );
}
