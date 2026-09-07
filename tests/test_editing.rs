// Tests for editing API (toggling styles, contenteditable logic, etc.)

use webcore::dom::*;
use webcore::layout::LayoutEngine;
use webcore::parse_html;
use webcore::types::*;

#[test]
fn editing_toggle_bold() {
    let mut doc = parse_html("<p>Hello world</p>");
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let mut p = query_selector_mut(&mut doc.root, "p").unwrap();
    let range = TextRange { start: 0, end: 5 };

    // Initial state: not bold
    assert!(!p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight.is_bold()));

    toggle_bold(p, &range);
    assert!(p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight.is_bold()));

    toggle_bold(p, &range);
    assert!(!p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_weight.is_bold()));
}

#[test]
fn editing_toggle_italic() {
    let mut doc = parse_html("<p>Hello world</p>");
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let mut p = query_selector_mut(&mut doc.root, "p").unwrap();
    let range = TextRange { start: 0, end: 5 };

    toggle_italic(p, &range);
    assert!(p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic));

    toggle_italic(p, &range);
    assert!(!p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_style == FontStyle::Italic));
}

#[test]
fn editing_set_font_size() {
    let mut doc = parse_html("<p>Hello world</p>");
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let mut p = query_selector_mut(&mut doc.root, "p").unwrap();
    let range = TextRange { start: 0, end: 5 };

    set_font_size(p, &range, 24.0);
    assert!(p
        .layout
        .inline_runs
        .iter()
        .any(|r| r.style.font_size == CssLength::Px(24.0)));
}

#[test]
fn editing_set_text_color() {
    let mut doc = parse_html("<p>Hello world</p>");
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let mut p = query_selector_mut(&mut doc.root, "p").unwrap();
    let range = TextRange { start: 0, end: 5 };
    let color = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    set_text_color(p, &range, color);
    assert!(p.layout.inline_runs.iter().any(|r| r.style.color == color));
}

#[test]
fn editing_content_editable_check() {
    let doc = parse_html(r#"<div id="parent" contenteditable="true"><p id="child">Text</p></div>"#);
    let parent = query_selector(&doc.root, "#parent").unwrap();

    // Check attribute exists
    assert_eq!(parent.get_attr("contenteditable"), Some("true"));
}
