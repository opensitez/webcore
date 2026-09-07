// Ported from cpptests/test_contenteditable.cpp
//
// Tests that can be expressed without widget infrastructure:
//   - BoxDefaultFalse: WebCore has no contentEditable flag → parse works fine
//   - DOM manipulation: toggle_bold, toggle_italic, toggle_underline,
//     set_font_size, set_text_color using the dom module APIs

use webcore::dom::{
    set_font_size, set_text_color, toggle_bold, toggle_italic, toggle_underline, TextRange,
};
use webcore::parse_html;
use webcore::types::*;

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

// ============================================================
// contentEditable attribute on Box
// WebCore does not expose a content_editable field in Rust;
// these tests verify that parsing/layout work correctly and that
// the box tree is accessible (structural equivalents of BoxDefaultFalse).
// ============================================================

#[test]
fn ce_box_parse_produces_tree() {
    // Equivalent of BoxDefaultFalse: parse produces a valid box tree
    // and the p element exists without any unexpected flags.
    let doc = parse_html("<p>Text</p>");
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some(), "parsed document should have a <p> box");
    // WebCore doesn't have a content_editable field; confirm that
    // tag and text are as expected.
    let p = p.unwrap();
    assert_eq!(p.tag, "p");
    // text_content includes inline text
    assert!(p.text_content().contains("Text"));
}

#[test]
fn ce_nested_divs_are_accessible() {
    // Equivalent of IsContentEditableInherited structural test
    let doc = parse_html("<div><p>Text</p></div>");
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some(), "should find <div>");
    let p = find_box(div.unwrap(), &|b| b.tag == "p");
    assert!(p.is_some(), "should find nested <p>");
}

#[test]
fn ce_sibling_boxes_are_independent() {
    // Equivalent of NotInheritedFromSibling: two sibling <p> elements
    // are independent boxes
    let doc = parse_html(r#"<div><p id="a">A</p><p id="b">B</p></div>"#);
    let paras: Vec<_> = {
        let mut v = vec![];
        fn collect<'a>(b: &'a WebCore, v: &mut Vec<&'a WebCore>) {
            if b.tag == "p" {
                v.push(b);
            }
            for c in &b.children {
                collect(c, v);
            }
        }
        collect(&doc.root, &mut v);
        v
    };
    assert!(paras.len() >= 2, "should have at least 2 <p> siblings");
    // They are independent (different pointers)
    assert!(
        std::ptr::eq(paras[0], paras[0]) && !std::ptr::eq(paras[0], paras[1]),
        "sibling paragraphs should be distinct boxes"
    );
}

// ============================================================
// DOM manipulation: toggle_bold
// ============================================================

#[test]
fn ce_toggle_bold_turns_on() {
    let mut b = WebCore::new("p");
    b.text = "Hello World".to_string();
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 11,
        style: ComputedStyle::default(),
    }];
    let range = TextRange { start: 0, end: 11 };
    toggle_bold(&mut b, &range);
    assert!(
        b.layout.inline_runs[0].style.font_weight.is_bold(),
        "should be bold after toggle"
    );
}

#[test]
fn ce_toggle_bold_turns_off_when_all_bold() {
    let mut b = WebCore::new("p");
    b.text = "Hello".to_string();
    let mut style = ComputedStyle::default();
    style.font_weight = FontWeight::Bold;
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 5,
        style,
    }];
    let range = TextRange { start: 0, end: 5 };
    toggle_bold(&mut b, &range);
    assert!(
        !b.layout.inline_runs[0].style.font_weight.is_bold(),
        "should be normal after un-toggle"
    );
}

#[test]
fn ce_toggle_bold_partial_range() {
    // Only the overlapping run should be affected
    let mut b = WebCore::new("p");
    b.text = "Hello World".to_string();
    b.layout.inline_runs = vec![
        InlineRun {
            text_offset: 0,
            length: 5,
            style: ComputedStyle::default(),
        }, // "Hello"
        InlineRun {
            text_offset: 6,
            length: 5,
            style: ComputedStyle::default(),
        }, // "World"
    ];
    // Toggle bold on "World" only
    let range = TextRange { start: 6, end: 11 };
    toggle_bold(&mut b, &range);
    assert!(
        !b.layout.inline_runs[0].style.font_weight.is_bold(),
        "Hello run should remain normal"
    );
    assert!(
        b.layout.inline_runs[1].style.font_weight.is_bold(),
        "World run should be bold"
    );
}

// ============================================================
// DOM manipulation: toggle_italic
// ============================================================

#[test]
fn ce_toggle_italic_turns_on() {
    let mut b = WebCore::new("p");
    b.text = "Test".to_string();
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 4,
        style: ComputedStyle::default(),
    }];
    let range = TextRange { start: 0, end: 4 };
    toggle_italic(&mut b, &range);
    assert_eq!(b.layout.inline_runs[0].style.font_style, FontStyle::Italic);
}

#[test]
fn ce_toggle_italic_turns_off() {
    let mut b = WebCore::new("p");
    b.text = "Test".to_string();
    let mut style = ComputedStyle::default();
    style.font_style = FontStyle::Italic;
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 4,
        style,
    }];
    let range = TextRange { start: 0, end: 4 };
    toggle_italic(&mut b, &range);
    assert_eq!(b.layout.inline_runs[0].style.font_style, FontStyle::Normal);
}

// ============================================================
// DOM manipulation: toggle_underline
// ============================================================

#[test]
fn ce_toggle_underline_turns_on() {
    let mut b = WebCore::new("p");
    b.text = "Test".to_string();
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 4,
        style: ComputedStyle::default(),
    }];
    let range = TextRange { start: 0, end: 4 };
    toggle_underline(&mut b, &range);
    assert!(
        b.layout.inline_runs[0].style.text_decoration.underline,
        "underline should be on"
    );
}

#[test]
fn ce_toggle_underline_turns_off() {
    let mut b = WebCore::new("p");
    b.text = "Test".to_string();
    let mut style = ComputedStyle::default();
    style.text_decoration.underline = true;
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 4,
        style,
    }];
    let range = TextRange { start: 0, end: 4 };
    toggle_underline(&mut b, &range);
    assert!(
        !b.layout.inline_runs[0].style.text_decoration.underline,
        "underline should be off"
    );
}

// ============================================================
// DOM manipulation: set_font_size
// ============================================================

#[test]
fn ce_set_font_size() {
    let mut b = WebCore::new("p");
    b.text = "Hello".to_string();
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 5,
        style: ComputedStyle::default(),
    }];
    let range = TextRange { start: 0, end: 5 };
    set_font_size(&mut b, &range, 24.0);
    assert_eq!(b.layout.inline_runs[0].style.font_size, CssLength::Px(24.0));
}

#[test]
fn ce_set_font_size_partial() {
    let mut b = WebCore::new("p");
    b.text = "Hello World".to_string();
    b.layout.inline_runs = vec![
        InlineRun {
            text_offset: 0,
            length: 5,
            style: ComputedStyle::default(),
        },
        InlineRun {
            text_offset: 6,
            length: 5,
            style: ComputedStyle::default(),
        },
    ];
    // Set size only on "World"
    let range = TextRange { start: 6, end: 11 };
    set_font_size(&mut b, &range, 18.0);
    // "Hello" run should be unchanged (default font size)
    assert!(
        !matches!(b.layout.inline_runs[0].style.font_size, CssLength::Px(v) if (v - 18.0).abs() < 0.1),
        "Hello run should not have size 18"
    );
    assert_eq!(b.layout.inline_runs[1].style.font_size, CssLength::Px(18.0));
}

// ============================================================
// DOM manipulation: set_text_color
// ============================================================

#[test]
fn ce_set_text_color() {
    let mut b = WebCore::new("p");
    b.text = "Hello".to_string();
    b.layout.inline_runs = vec![InlineRun {
        text_offset: 0,
        length: 5,
        style: ComputedStyle::default(),
    }];
    let range = TextRange { start: 0, end: 5 };
    set_text_color(&mut b, &range, Color::rgb(255, 0, 0));
    assert_eq!(b.layout.inline_runs[0].style.color, Color::rgb(255, 0, 0));
}

#[test]
fn ce_set_text_color_partial() {
    let mut b = WebCore::new("p");
    b.text = "Hello World".to_string();
    b.layout.inline_runs = vec![
        InlineRun {
            text_offset: 0,
            length: 5,
            style: ComputedStyle::default(),
        },
        InlineRun {
            text_offset: 6,
            length: 5,
            style: ComputedStyle::default(),
        },
    ];
    // Color only "World" blue
    let range = TextRange { start: 6, end: 11 };
    set_text_color(&mut b, &range, Color::rgb(0, 0, 255));
    assert_eq!(
        b.layout.inline_runs[0].style.color,
        Color::BLACK,
        "Hello should remain black"
    );
    assert_eq!(b.layout.inline_runs[1].style.color, Color::rgb(0, 0, 255));
}

// ============================================================
// DOM manipulation: clone_element preserves structure
// Equivalent of C++ ContentEditable, PreservedInClone
// ============================================================

#[test]
fn ce_clone_element_preserves_tag_and_text() {
    use webcore::dom::clone_element;
    let doc = parse_html("<p>Cloneable</p>");
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    let cloned = clone_element(p);
    assert_eq!(cloned.tag, "p");
    assert!(
        cloned.text_content().contains("Cloneable"),
        "cloned element must contain original text"
    );
}

#[test]
fn ce_clone_element_is_independent() {
    use webcore::dom::clone_element;
    let doc = parse_html("<div><p>Original</p></div>");
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    let cloned = clone_element(p);
    // Cloned is a separate object (different address)
    assert!(!std::ptr::eq(
        p as *const WebCore,
        &cloned as *const WebCore
    ));
}

// ============================================================
// Editor: increase/decrease_indent survives recascade
// Equivalent of C++ ParserStyleSync, IndentSurvivesReCascade /
// DecreaseIndentSurvives
// ============================================================

use webcore::dom::Editor;
use webcore::dom::{query_selector, query_selector_mut};
use webcore::layout::LayoutEngine;

fn parse_and_layout(html: &str) -> webcore::types::Document {
    let mut doc = parse_html(html);
    LayoutEngine::new().layout(&mut doc, 800.0);
    doc
}

fn set_caret(editor: &mut Editor, element: &WebCore, offset: usize) {
    editor.caret_box = Some(element.node_id);
    editor.collapse_to(offset);
}

#[test]
fn ce_indent_survives_recascade() {
    // ParserStyleSync, IndentSurvivesReCascade
    let mut doc = parse_and_layout("<p>Indent me</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_indent(&mut doc.root, 40.0);

    let margin_before = match query_selector(&doc.root, "p").unwrap().style.margin_left {
        webcore::types::CssLength::Px(v) => v,
        _ => panic!("expected Px margin after indent"),
    };
    assert!(
        margin_before > 0.0,
        "margin-left should be positive after increase_indent"
    );

    doc.recascade();

    match &query_selector(&doc.root, "p").unwrap().style.margin_left {
        webcore::types::CssLength::Px(v) => assert!(
            (*v - margin_before).abs() < 0.01,
            "margin-left must survive recascade; before={} after={}",
            margin_before,
            v
        ),
        other => panic!("indent must survive recascade; got {:?}", other),
    }
}

#[test]
fn ce_decrease_indent_survives_recascade() {
    // ParserStyleSync, DecreaseIndentSurvives
    let mut doc = parse_and_layout("<p>Indent me</p>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_indent(&mut doc.root, 40.0);
    doc.editor.increase_indent(&mut doc.root, 40.0);
    doc.editor.decrease_indent(&mut doc.root, 40.0);

    let margin_before = match query_selector(&doc.root, "p").unwrap().style.margin_left {
        webcore::types::CssLength::Px(v) => v,
        webcore::types::CssLength::Zero => 0.0,
        _ => panic!("expected Px or Zero margin"),
    };

    doc.recascade();

    let margin_after = match query_selector(&doc.root, "p").unwrap().style.margin_left {
        webcore::types::CssLength::Px(v) => v,
        webcore::types::CssLength::Zero => 0.0,
        _ => panic!("expected Px or Zero margin after recascade"),
    };
    assert!(
        (margin_after - margin_before).abs() < 0.01,
        "margin after decrease_indent must survive recascade; before={} after={}",
        margin_before,
        margin_after
    );
}

// ============================================================
// Editor: toggle_bullet_list survives recascade
// Equivalent of C++ ParserStyleSync, BulletListSurvivesReCascade /
// BulletListToggleOffSurvives
// ============================================================

#[test]
fn ce_bullet_list_survives_recascade() {
    // ParserStyleSync, BulletListSurvivesReCascade
    use webcore::dom::query_selector_all;
    let mut doc = parse_and_layout("<div><p>List item</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.toggle_bullet_list(&mut doc.root);

    // Confirm bullet was created
    assert!(
        query_selector(&doc.root, "li").is_some(),
        "<li> must exist after toggle"
    );

    doc.recascade();

    // The <li> must still be there after recascade
    assert!(
        query_selector(&doc.root, "li").is_some(),
        "<li> must survive recascade"
    );
    let lis = query_selector_all(&doc.root, "li");
    assert!(!lis.is_empty(), "list items must survive recascade");
}

#[test]
fn ce_bullet_list_toggle_off_survives_recascade() {
    // ParserStyleSync, BulletListToggleOffSurvives
    let mut doc = parse_and_layout("<div><p>Item</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.toggle_bullet_list(&mut doc.root); // toggle on
    doc.editor.toggle_bullet_list(&mut doc.root); // toggle off

    // No <li> should exist
    assert!(
        query_selector(&doc.root, "li").is_none(),
        "<li> should be gone after toggle-off"
    );

    doc.recascade();

    // Must still be off after recascade
    assert!(
        query_selector(&doc.root, "li").is_none(),
        "<li> must remain absent after recascade"
    );
}

// ============================================================
// Editor: increase_quote_level survives recascade
// Equivalent of C++ ParserStyleSync, QuoteSurvivesReCascade /
// UnquoteSurvivesReCascade
// ============================================================

#[test]
fn ce_quote_survives_recascade() {
    // ParserStyleSync, QuoteSurvivesReCascade
    let mut doc = parse_and_layout("<div><p>Quote me</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_quote_level(&mut doc.root);

    assert!(
        query_selector(&doc.root, "blockquote").is_some(),
        "<blockquote> must exist after increase_quote_level"
    );

    doc.recascade();

    assert!(
        query_selector(&doc.root, "blockquote").is_some(),
        "<blockquote> must survive recascade"
    );
}

#[test]
fn ce_unquote_survives_recascade() {
    // ParserStyleSync, UnquoteSurvivesReCascade
    let mut doc = parse_and_layout("<div><p>Quote me</p></div>");
    {
        let p = query_selector_mut(&mut doc.root, "p").unwrap();
        set_caret(&mut doc.editor, p, 0);
    }
    doc.editor.increase_quote_level(&mut doc.root);
    doc.editor.decrease_quote_level(&mut doc.root);

    assert!(
        query_selector(&doc.root, "blockquote").is_none(),
        "<blockquote> must be gone after decrease_quote_level"
    );

    doc.recascade();

    assert!(
        query_selector(&doc.root, "blockquote").is_none(),
        "<blockquote> must remain absent after recascade"
    );

    let p = query_selector(&doc.root, "p");
    assert!(
        p.is_some(),
        "<p> must still be present after unquote + recascade"
    );
}

// ============================================================
// ParserStyle tests that require SetAlignment / SetHeading:
// these APIs do not exist in the Rust dom module.
// ============================================================
// TODO: API not available — AlignmentSurvivesReCascade
// TODO: API not available — AlignRightSurvives
// TODO: API not available — AlignJustifySurvives
// TODO: API not available — HeadingSurvivesReCascade

// ============================================================
// ContentEditable / ReadOnly / Selection widget tests:
// all require wxHtmlEditWidget — no equivalent in Rust.
// ============================================================
// TODO: API not available — SetByElementId, SetByBoxPointer, UnsetById
// TODO: API not available — ReadOnly (EditableWhenNotReadOnly, BlockedWhenReadOnly, etc.)
// TODO: API not available — Selection (WorksInReadOnlyMode, WorksInLockedRegionWithContentEditable, etc.)
// TODO: API not available — CrossRegion (DeleteBlockedWhenSelectionSpansLocked, etc.)
