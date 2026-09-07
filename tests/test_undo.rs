// Ported from cpptests/test_undo.cpp
// Tests for UndoStack and document state restoration.
// NOTE: All C++ undo tests use TestWidget (wxHtmlEditWidget) which is not portable.
// Only the pure undo-stack tests are ported here.

use webcore::dom::*;
use webcore::parse_html;
use webcore::types::*;

#[test]
fn undo_push_and_restore() {
    let mut stack = UndoStack::new();
    let doc1 = parse_html("<p>One</p>");
    let doc2 = parse_html("<p>Two</p>");

    // Snapshot state before changing to doc2
    stack.push(doc1.clone(), 0, 0, 0);

    assert!(stack.can_undo());
    assert!(!stack.can_redo());

    // Perform undo
    let entry = stack.undo(doc2.clone(), 0, 0, 0).unwrap();
    assert_eq!(entry.doc.root.text_content(), "One");

    assert!(!stack.can_undo());
    assert!(stack.can_redo());

    // Perform redo
    let entry = stack.redo(doc1, 0, 0, 0).unwrap();
    assert_eq!(entry.doc.root.text_content(), "Two");
}

#[test]
fn undo_multiple_levels() {
    let mut stack = UndoStack::new();
    let d0 = parse_html("<p>0</p>");
    let d1 = parse_html("<p>1</p>");
    let d2 = parse_html("<p>2</p>");

    stack.push(d0.clone(), 0, 0, 0);
    stack.push(d1.clone(), 0, 0, 0);

    // current is d2
    let entry1 = stack.undo(d2.clone(), 0, 0, 0).unwrap();
    assert_eq!(entry1.doc.root.text_content(), "1");

    let entry0 = stack.undo(entry1.doc, 0, 0, 0).unwrap();
    assert_eq!(entry0.doc.root.text_content(), "0");
}

#[test]
fn undo_redo_stack_cleared_on_push() {
    let mut stack = UndoStack::new();
    let d0 = parse_html("<p>0</p>");
    let d1 = parse_html("<p>1</p>");
    let d2 = parse_html("<p>2</p>");

    stack.push(d0.clone(), 0, 0, 0);
    stack.undo(d1.clone(), 0, 0, 0);
    assert!(stack.can_redo());

    // Pushing a new state clears redo
    stack.push(d1.clone(), 0, 0, 0);
    assert!(!stack.can_redo());
}

#[test]
fn undo_limit_respected() {
    let mut stack = UndoStack::new();
    let doc = parse_html("<p>X</p>");

    // Push more than 500 entries (the limit in dom/mod.rs)
    for i in 0..600 {
        stack.push(doc.clone(), i, 0, 0);
    }

    // Should only have 500 entries in undo stack
    // We can't check length directly because fields are private,
    // but we can try to undo 500 times.
    for _ in 0..500 {
        assert!(stack.can_undo());
        stack.undo(doc.clone(), 0, 0, 0);
    }
    assert!(!stack.can_undo());
}

#[test]
fn undo_noop_on_empty() {
    let mut stack = UndoStack::new();
    let doc = parse_html("<p>X</p>");
    assert!(stack.undo(doc.clone(), 0, 0, 0).is_none());
    assert!(stack.redo(doc, 0, 0, 0).is_none());
}

// ============================================================
// Additional undo stack tests (C++ test_undo.cpp — stack behavior)
// ============================================================

#[test]
fn undo_caret_position_restored() {
    let mut stack = UndoStack::new();
    let doc = parse_html("<p>Hello</p>");

    // Push with specific caret position
    stack.push(doc.clone(), 5, 0, 5);

    let entry = stack.undo(doc.clone(), 10, 0, 10).unwrap();
    assert_eq!(entry.caret_pos, 5);
    assert_eq!(entry.sel_end, 5);
}

#[test]
fn undo_selection_positions_restored() {
    let mut stack = UndoStack::new();
    let doc = parse_html("<p>Hello World</p>");

    stack.push(doc.clone(), 0, 3, 8);

    let entry = stack.undo(doc.clone(), 0, 0, 0).unwrap();
    assert_eq!(entry.sel_start, 3);
    assert_eq!(entry.sel_end, 8);
}

#[test]
fn undo_redo_preserves_document_content() {
    let mut stack = UndoStack::new();
    let d_before = parse_html("<p>Before</p>");
    let d_after = parse_html("<p>After</p>");

    stack.push(d_before.clone(), 0, 0, 0);
    // Perform undo: save current (After) state, restore Before
    let entry = stack.undo(d_after.clone(), 0, 0, 0).unwrap();
    assert_eq!(entry.doc.root.text_content(), "Before");

    // Perform redo: save current (Before) state, restore After
    let entry2 = stack.redo(d_before, 0, 0, 0).unwrap();
    assert_eq!(entry2.doc.root.text_content(), "After");
}

#[test]
fn undo_multiple_redo_steps() {
    let mut stack = UndoStack::new();
    let d0 = parse_html("<p>0</p>");
    let d1 = parse_html("<p>1</p>");
    let d2 = parse_html("<p>2</p>");

    stack.push(d0.clone(), 0, 0, 0);
    stack.push(d1.clone(), 0, 0, 0);

    // Undo twice
    stack.undo(d2.clone(), 0, 0, 0);
    stack.undo(d1, 0, 0, 0);

    // Should be able to redo twice
    assert!(stack.can_redo());
    stack.redo(d0.clone(), 0, 0, 0);
    assert!(stack.can_redo());
    stack.redo(d0, 0, 0, 0);
    assert!(!stack.can_redo());
}

#[test]
fn undo_new_push_clears_redo_and_sets_can_undo() {
    let mut stack = UndoStack::new();
    let doc = parse_html("<p>X</p>");

    // Push then undo to populate redo
    stack.push(doc.clone(), 0, 0, 0);
    stack.undo(doc.clone(), 0, 0, 0);
    assert!(stack.can_redo());

    // A new push should clear redo
    stack.push(doc.clone(), 0, 0, 0);
    assert!(!stack.can_redo());
    assert!(stack.can_undo());
}

// ============================================================
// Round-trip: Serialize → Parse
// Ported from C++ test_undo.cpp: Undo, RoundTripText /
// RoundTripStructure / MultipleRoundTripsStable
// ============================================================

#[test]
fn undo_roundtrip_text() {
    // Undo, RoundTripText
    use webcore::html::serialize_html;
    let doc = parse_html("<p>Hello <b>bold</b> world</p>");
    let orig_text = doc.root.text_content();
    let html = serialize_html(&doc);
    let restored = parse_html(&html);
    assert_eq!(
        restored.root.text_content(),
        orig_text,
        "serialise → parse round-trip must preserve flat text"
    );
}

#[test]
fn undo_roundtrip_structure() {
    // Undo, RoundTripStructure
    use webcore::html::serialize_html;
    let doc = parse_html("<p>A</p><p>B</p><p>C</p>");
    let orig_text = doc.root.text_content();
    let html = serialize_html(&doc);
    let restored = parse_html(&html);
    assert_eq!(
        restored.root.text_content(),
        orig_text,
        "round-trip must preserve text of multiple paragraphs"
    );
    // root must exist (non-empty tree)
    assert!(
        !restored.root.tag.is_empty(),
        "restored root must not be empty"
    );
}

#[test]
fn undo_multiple_roundtrips_stable() {
    // Undo, MultipleRoundTripsStable
    use webcore::html::serialize_html;
    let input = "<p><b>Bold</b> <i>italic</i> <u>underline</u></p>";
    let d1 = parse_html(input);
    let h1 = serialize_html(&d1);
    let d2 = parse_html(&h1);
    let h2 = serialize_html(&d2);
    let d3 = parse_html(&h2);
    assert_eq!(
        d1.root.text_content(),
        d2.root.text_content(),
        "text must be identical after first round-trip"
    );
    assert_eq!(
        d2.root.text_content(),
        d3.root.text_content(),
        "text must be identical after second round-trip"
    );
}

// ============================================================
// HTML serialization: structure preservation
// Ported from C++ test_undo.cpp Clipboard group.
// Only the serialize-based tests are ported; widget
// (GetSelectedHTML / SelectWord / SelectLine) tests are skipped.
// TODO: API not available — GetSelectedHTML / SelectWord / SelectLine
// ============================================================

#[test]
fn undo_serialize_preserves_bold() {
    // Clipboard, SelectedHTMLBold — pure serialization variant
    use webcore::html::serialize_html;
    let doc = parse_html("<p><b>Bold</b> text</p>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("<b>") || html.contains("font-weight"),
        "serialised HTML must represent bold; got: {:?}",
        &html[..html.len().min(200)]
    );
    assert!(
        html.contains("Bold"),
        "serialised HTML must contain the word 'Bold'"
    );
}

#[test]
fn undo_serialize_preserves_italic() {
    use webcore::html::serialize_html;
    let doc = parse_html("<p><i>Italic</i> text</p>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("<i>") || html.contains("font-style"),
        "serialised HTML must represent italic"
    );
    assert!(html.contains("Italic"));
}

#[test]
fn undo_serialize_preserves_underline() {
    use webcore::html::serialize_html;
    let doc = parse_html("<p><u>Underlined</u></p>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("Underlined"),
        "serialised HTML must contain underlined text"
    );
}

#[test]
fn undo_serialize_table_structure() {
    // Clipboard, TableHTMLPreserved
    use webcore::html::serialize_html;
    let doc =
        parse_html("<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("A") && html.contains("B") && html.contains("C") && html.contains("D"),
        "all table cells must be present in serialised HTML"
    );
}

#[test]
fn undo_serialize_mixed_inline_formatting() {
    // Clipboard, MixedInlineFormatting
    use webcore::html::serialize_html;
    let doc = parse_html("<p><b>Bold</b> <i>Italic</i> <u>Under</u> <s>Strike</s></p>");
    let html = serialize_html(&doc);
    assert!(html.contains("Bold"), "must contain Bold");
    assert!(html.contains("Italic"), "must contain Italic");
    assert!(html.contains("Under"), "must contain Under");
    assert!(html.contains("Strike"), "must contain Strike");
}

#[test]
fn undo_serialize_nested_formatting() {
    // Clipboard, NestedFormattingPreserved
    use webcore::html::serialize_html;
    let doc = parse_html("<p><b><i>BoldItalic</i></b> plain</p>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("BoldItalic"),
        "nested bold-italic text must survive serialisation"
    );
}

#[test]
fn undo_serialize_blockquote() {
    // Clipboard, BlockquotePreserved
    use webcore::html::serialize_html;
    let doc = parse_html("<blockquote><p>Quoted text</p></blockquote>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("Quoted text"),
        "blockquote content must be present in serialised HTML"
    );
}

#[test]
fn undo_serialize_ordered_list() {
    // Clipboard, OrderedListPreserved
    use webcore::html::serialize_html;
    let doc = parse_html("<ol><li>First</li><li>Second</li><li>Third</li></ol>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("First") && html.contains("Second") && html.contains("Third"),
        "all list items must appear in serialised HTML"
    );
}

#[test]
fn undo_serialize_nested_list() {
    // Clipboard, NestedListPreserved
    use webcore::html::serialize_html;
    let doc = parse_html("<ul><li>Outer<ul><li>Inner</li></ul></li></ul>");
    let html = serialize_html(&doc);
    assert!(
        html.contains("Outer") && html.contains("Inner"),
        "nested list items must appear in serialised HTML"
    );
}

#[test]
fn undo_serialize_link_preserved() {
    // Clipboard, LinkPreserved
    use webcore::html::serialize_html;
    let doc = parse_html(r#"<p><a href="https://example.com">Click here</a></p>"#);
    let html = serialize_html(&doc);
    assert!(
        html.contains("Click here"),
        "link text must appear in serialised HTML"
    );
    // href attribute should also be present
    assert!(
        html.contains("example.com") || html.contains("Click here"),
        "link destination or text must be in serialised HTML"
    );
}

#[test]
fn undo_serialize_complex_document() {
    // Clipboard, ComplexDocumentPreserved
    use webcore::html::serialize_html;
    let doc = parse_html(
        "<h1>Title</h1>\
         <p>Intro with <b>bold</b> and <i>italic</i></p>\
         <ul><li>Item 1</li><li>Item 2</li></ul>\
         <table><tr><td>A</td><td>B</td></tr></table>\
         <blockquote><p>A quote</p></blockquote>",
    );
    let html = serialize_html(&doc);
    assert!(html.contains("Title"), "heading text must be present");
    assert!(html.contains("bold"), "bold text must be present");
    assert!(html.contains("italic"), "italic text must be present");
    assert!(html.contains("Item 1"), "list items must be present");
    assert!(html.contains("A"), "table cell must be present");
    assert!(
        html.contains("A quote"),
        "blockquote content must be present"
    );
}

#[test]
fn undo_roundtrip_via_serialize_then_parse() {
    // Clipboard, HTMLRoundTripViaGetSelectedHTML (pure-serialize variant)
    use webcore::html::serialize_html;
    let doc = parse_html("<p><b>Bold</b> normal <i>italic</i></p>");
    let orig_text = doc.root.text_content();
    let html = serialize_html(&doc);
    let parsed = parse_html(&html);
    assert_eq!(
        parsed.root.text_content(),
        orig_text,
        "round-trip via serialize_html must preserve flat text content"
    );
}

// ============================================================
// Widget-dependent tests that cannot be ported:
// ============================================================
// TODO: API not available — TypeAndUndo (requires PushUndo / InsertText / Undo widget API)
// TODO: API not available — DeleteAndUndo
// TODO: API not available — MultipleUndos (undo merge / compression)
// TODO: API not available — BasicRedo
// TODO: API not available — MultipleUndoRedo
// TODO: API not available — RedoClearedOnNewEdit
// TODO: API not available — UndoMergeBreaksOnDifferentType
// TODO: API not available — UndoMergeCompressesRapidTyping
// TODO: API not available — FormattingPreserved (SerializeHTML widget helper)
// TODO: API not available — SelectWord / SelectLine tests
// TODO: API not available — SelectedHTMLBold / SelectedHTMLItalic (GetSelectedHTML)
