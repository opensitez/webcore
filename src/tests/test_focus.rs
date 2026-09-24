// Tests for Tab/focus keyboard navigation and :focus visual indicator.

use crate::css::apply_cascade_vp;
use crate::html::parse_html;
use crate::layout::LayoutEngine;
use crate::types::{BorderStyle, Display};
use crate::types::{Color, Document};

// ── helpers ───────────────────────────────────────────────────────────────────

fn parse_and_layout(html: &str) -> Document {
    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);
    doc
}

// ── 1. Tab advances focus through native-focusable elements ───────────────────

#[test]
fn tab_focus_advances_in_document_order() {
    let mut doc = parse_and_layout(
        "<html><body><button id=a>A</button><button id=b>B</button><button id=c>C</button></body></html>",
    );

    // Initially no focus.
    assert!(doc.focused_box == 0, "no initial focus");

    // Tab → first focusable (button A).
    assert!(doc.focus_next(), "first Tab must return true");
    let a_id = crate::dom::query_selector(&doc.root, "#a")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, a_id,
        "focus must be on button A after first Tab"
    );

    // Tab → second (B).
    assert!(doc.focus_next());
    let b_id = crate::dom::query_selector(&doc.root, "#b")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(doc.focused_box, b_id, "focus must be on button B");

    // Tab → third (C).
    assert!(doc.focus_next());
    let c_id = crate::dom::query_selector(&doc.root, "#c")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(doc.focused_box, c_id, "focus must be on button C");

    // Tab → wraps back to A.
    assert!(doc.focus_next());
    assert_eq!(doc.focused_box, a_id, "focus must wrap to button A");
}

#[test]
fn shift_tab_reverses_focus() {
    let mut doc = parse_and_layout(
        "<html><body><button id=a>A</button><button id=b>B</button></body></html>",
    );
    // Focus A, then Shift+Tab should go to B (wrap).
    doc.focus_next();
    doc.focus_prev();
    let b_id = crate::dom::query_selector(&doc.root, "#b")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, b_id,
        "Shift+Tab from first element must wrap to last"
    );
}

#[test]
fn tab_includes_inputs_and_anchors() {
    let mut doc = parse_and_layout(
        "<html><body>\
            <input type=\"text\" id=i>\
            <a href=\"/\" id=l>Link</a>\
            <textarea id=t></textarea>\
        </body></html>",
    );
    let mut visited_ids: Vec<&str> = Vec::new();
    for _ in 0..3 {
        doc.focus_next();
        let focused = doc.focused_box;
        for id in ["i", "l", "t"] {
            if let Some(node) = crate::dom::query_selector(&doc.root, &format!("#{id}")) {
                if node.node_id == focused {
                    visited_ids.push(id);
                }
            }
        }
    }
    assert_eq!(
        visited_ids.len(),
        3,
        "Tab must visit input, link, and textarea"
    );
}

// ── 2. tabindex=-1 excluded, tabindex=0 included in normal order ──────────────

#[test]
fn tabindex_minus1_excluded_from_tab_order() {
    let mut doc = parse_and_layout(
        "<html><body>\
            <button id=a>A</button>\
            <button id=b tabindex=\"-1\">B (skip)</button>\
            <button id=c>C</button>\
        </body></html>",
    );
    doc.focus_next(); // → A
    doc.focus_next(); // → C (B is skipped)
    let c_id = crate::dom::query_selector(&doc.root, "#c")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, c_id,
        "tabindex=-1 element must be skipped in Tab order"
    );
}

#[test]
fn tabindex_zero_included_in_normal_order() {
    let mut doc = parse_and_layout(
        "<html><body>\
            <div id=d tabindex=\"0\">Div</div>\
            <button id=b>Button</button>\
        </body></html>",
    );
    doc.focus_next(); // → div (tabindex=0, first in document order)
    let d_id = crate::dom::query_selector(&doc.root, "#d")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, d_id,
        "tabindex=0 element must be in tab order"
    );
}

// ── 3. Positive tabindex ordering ─────────────────────────────────────────────

#[test]
fn positive_tabindex_sorted_before_normal_focusable() {
    let mut doc = parse_and_layout(
        "<html><body>\
            <button id=first>First (no tabindex)</button>\
            <button id=high tabindex=\"3\">High index</button>\
            <button id=low  tabindex=\"1\">Low index</button>\
        </body></html>",
    );
    // Tab order: tabindex=1 → tabindex=3 → natural (no tabindex)
    doc.focus_next();
    let low_id = crate::dom::query_selector(&doc.root, "#low")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, low_id,
        "tabindex=1 must be first in tab order"
    );

    doc.focus_next();
    let high_id = crate::dom::query_selector(&doc.root, "#high")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, high_id,
        "tabindex=3 must come before natural focusable"
    );

    doc.focus_next();
    let first_id = crate::dom::query_selector(&doc.root, "#first")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(doc.focused_box, first_id, "natural button must come last");
}

// ── 4. Viewport stored in Document after layout ───────────────────────────────

#[test]
fn layout_stores_viewport_in_doc() {
    let doc = parse_and_layout("<html><body></body></html>");
    assert_eq!(doc.viewport_w, 800.0);
    assert_eq!(doc.viewport_h, 600.0);
}

// ── 5. focus_next / focus_prev fire Focus/Blur events ─────────────────────────

#[test]
fn tab_fires_focus_event() {
    use crate::dom::HtmlEventType;
    use std::sync::{Arc, Mutex};

    let mut doc = parse_and_layout(
        "<html><body><button id=a>A</button><button id=b>B</button></body></html>",
    );

    let focused_count = Arc::new(Mutex::new(0u32));
    let fc = focused_count.clone();

    // One listener per button, the DOM way. `focus` does not bubble, so it
    // fires on the focused element itself.
    for id in doc.query_selector_all("button") {
        let c = fc.clone();
        doc.add_event_listener(
            id,
            "focus",
            Box::new(move |_evt, _d| {
                *c.lock().unwrap() += 1;
            }),
            crate::dom::events::ListenerOptions::default(),
        );
    }

    doc.focus_next(); // A gets focus → Focus fires on A
    doc.focus_next(); // B gets focus → Focus fires on B

    assert_eq!(
        *focused_count.lock().unwrap(),
        2,
        "each Tab must fire a Focus event on the target"
    );
}

// ── 6. :focus-visible indicator — keyboard only, not mouse ───────────────────

#[test]
fn keyboard_focused_element_has_ua_outline() {
    let mut doc = parse_and_layout("<html><body><button id=btn>Click</button></body></html>");
    // Tab = keyboard focus → :focus-visible fires → UA outline appears.
    doc.focus_next();
    assert!(
        doc.keyboard_focus,
        "focus_next must set keyboard_focus=true"
    );

    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    assert!(
        btn.style.outline_width > 0.0,
        "keyboard-focused element must have UA outline"
    );
    assert_ne!(
        btn.style.outline_style,
        BorderStyle::None,
        "keyboard-focused element must have non-None outline_style"
    );
}

#[test]
fn unfocused_element_has_no_ua_outline() {
    let doc = parse_and_layout("<html><body><button id=btn>Click</button></body></html>");
    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    assert_eq!(
        btn.style.outline_style,
        BorderStyle::None,
        "unfocused element must not have an outline"
    );
}

#[test]
fn mouse_focus_does_not_show_outline() {
    // Simulate what happens after a mouse click sets focus (keyboard_focus = false).
    // :focus-visible must NOT match, so no UA outline.
    let mut doc = parse_and_layout("<html><body><button id=btn>Click</button></body></html>");
    // Manually set focus as if from mouse (keyboard_focus stays false).
    let btn_id = crate::dom::query_selector(&doc.root, "#btn")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = btn_id;
    doc.keyboard_focus = false;
    // Recascade with keyboard_focus=false.
    doc.stylesheet.rebuild_index();
    crate::css::apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );

    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    assert_eq!(
        btn.style.outline_style,
        BorderStyle::None,
        "mouse-focused element must NOT get the :focus-visible UA outline"
    );
}

#[test]
fn mouse_focus_styles_apply_on_next_layout() {
    let mut doc = parse_and_layout(
        "<style>input:focus { background-color: #123456 } form:focus-within { color: #abcdef }</style><form id=f><input id=i></form>",
    );
    let rect = crate::dom::query_selector(&doc.root, "#i")
        .unwrap()
        .layout
        .border_rect;
    let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    assert!(doc.style_dirty);

    let mut engine = LayoutEngine::new();
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    assert_eq!(
        crate::dom::query_selector(&doc.root, "#i")
            .unwrap()
            .style
            .background_color,
        Color::rgb(0x12, 0x34, 0x56),
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#f")
            .unwrap()
            .style
            .color,
        Color::rgb(0xab, 0xcd, 0xef),
    );
}

#[test]
fn author_can_override_focus_outline_color() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            button:focus { outline-color: #ff0000; }
        </style></head><body><button id=btn>B</button></body></html>"#,
    );
    doc.focus_next();

    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    // Author rule overrides UA outline color to red.
    assert_eq!(
        btn.style.outline_color,
        Color::rgb(0xff, 0x00, 0x00),
        "author :focus outline-color must override the UA default"
    );
}

#[test]
fn author_can_suppress_focus_outline() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            button:focus { outline: none; }
        </style></head><body><button id=btn>B</button></body></html>"#,
    );
    doc.focus_next();

    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    assert_eq!(
        btn.style.outline_style,
        BorderStyle::None,
        "author outline:none must suppress the UA focus outline"
    );
}

#[test]
fn focus_within_matches_custom_element_ancestor_across_siblings() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            x-menu:not([loaded], :focus-within) [slot=dropdown] { display: none; }
        </style></head><body>
            <x-menu id="menu">
                <button id="trigger">HTML</button>
                <div id="panel" slot="dropdown">Panel</div>
            </x-menu>
        </body></html>"#,
    );

    let panel = crate::dom::query_selector(&doc.root, "#panel").unwrap();
    assert_eq!(
        panel.style.display,
        Display::None,
        "unfocused custom menu fallback should hide its dropdown"
    );

    let trigger_id = crate::dom::query_selector(&doc.root, "#trigger")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = trigger_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );

    let panel = crate::dom::query_selector(&doc.root, "#panel").unwrap();
    assert_ne!(
        panel.style.display,
        Display::None,
        "focused sibling should make the custom-element ancestor match :focus-within"
    );
}

#[test]
fn focus_within_revealed_absolute_child_under_display_contents_gets_layout() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            .tab { position: relative; width: 320px; height: 40px; }
            x-menu { display: contents; }
            x-menu:not([loaded], :focus-within) [slot=dropdown] { display: none; }
            [slot=dropdown] {
                position: absolute;
                top: 24px;
                left: 0;
                width: 180px;
                height: 48px;
                display: block;
            }
        </style></head><body>
            <div class="tab">
                <x-menu>
                    <button id="trigger">HTML</button>
                    <div id="panel" slot="dropdown">Panel</div>
                </x-menu>
            </div>
        </body></html>"#,
    );

    let trigger_id = crate::dom::query_selector(&doc.root, "#trigger")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = trigger_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );
    crate::dom::mark_layout_dirty(&mut doc.root);
    let mut engine = LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    let panel = crate::dom::query_selector(&doc.root, "#panel").unwrap();
    assert_eq!(panel.style.display, Display::Block);
    assert!(
        panel.layout.border_rect.w >= 179.0 && panel.layout.border_rect.h >= 47.0,
        "revealed absolute dropdown should have non-zero layout, got {:?}",
        panel.layout.border_rect
    );
}

#[test]
fn abspos_stretch_dropdown_under_display_contents_sizes_from_content() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            .tab { position: relative; width: 360px; min-height: 40px; }
            x-menu { display: contents; }
            x-menu:not([loaded], :focus-within) [slot=dropdown] { display: none; }
            [slot=dropdown] {
                position: absolute;
                left: 0;
                right: 0;
                margin-top: -1px;
                display: block;
                background: #111;
            }
            .content { display: grid; grid-template-columns: repeat(3, 1fr); padding: 16px; gap: 16px; }
        </style></head><body>
            <div class="tab">
                <x-menu>
                    <button id="trigger">HTML</button>
                    <div id="panel" slot="dropdown">
                        <p><a>HTML: Markup language</a></p>
                        <div class="content"><dl><dt>HTML reference</dt><dd>Elements</dd></dl></div>
                    </div>
                </x-menu>
            </div>
        </body></html>"#,
    );

    let trigger_id = crate::dom::query_selector(&doc.root, "#trigger")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = trigger_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );
    crate::dom::mark_layout_dirty(&mut doc.root);
    let mut engine = LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    let panel = crate::dom::query_selector(&doc.root, "#panel").unwrap();
    assert_eq!(panel.style.display, Display::Block);
    assert!(
        panel.layout.border_rect.w >= 350.0,
        "left/right abspos stretch should use containing width, got {:?}",
        panel.layout.border_rect
    );
    assert!(
        panel.layout.border_rect.h > 20.0,
        "auto-height abspos dropdown should size from content, got {:?}",
        panel.layout.border_rect
    );
}

#[test]
fn focused_display_contents_dropdown_relayouts_incrementally() {
    let mut doc = parse_and_layout(
        r#"<html><head><style>
            .nav { width: 500px; }
            .tab { position: relative; width: 240px; min-height: 32px; }
            x-menu { display: contents; }
            x-menu:not(:focus-within) [slot=dropdown] { display: none; }
            button { display: block; width: 80px; height: 24px; }
            [slot=dropdown] {
                position: absolute;
                left: 0;
                right: 0;
                top: 24px;
                display: block;
                padding: 12px;
                border: 1px solid black;
            }
            .panel-content { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; }
        </style></head><body>
            <div class="nav">
                <div class="tab">
                    <x-menu>
                        <button id="trigger">HTML</button>
                        <div id="panel" slot="dropdown">
                            <div id="content" class="panel-content">
                                <p>HTML reference</p><p>Markup languages</p>
                            </div>
                        </div>
                    </x-menu>
                </div>
            </div>
        </body></html>"#,
    );

    let trigger_id = crate::dom::query_selector(&doc.root, "#trigger")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = trigger_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );

    let mut engine = LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);

    let panel = crate::dom::query_selector(&doc.root, "#panel").unwrap();
    let content = crate::dom::query_selector(&doc.root, "#content").unwrap();
    assert_eq!(panel.style.display, Display::Block);
    assert!(
        panel.layout.border_rect.w >= 230.0 && panel.layout.border_rect.h > 24.0,
        "incrementally revealed dropdown should be laid out, got {:?}",
        panel.layout.border_rect
    );
    assert!(
        content.layout.border_rect.w > 0.0 && content.layout.border_rect.h > 0.0,
        "revealed dropdown descendants should not keep stale zero boxes, got {:?}",
        content.layout.border_rect
    );
}

#[test]
fn empty_content_after_pseudo_with_size_gets_layout_box() {
    let doc = parse_and_layout(
        r#"<html><head><style>
            button::after {
                content: "";
                display: inline-block;
                width: 20px;
                height: 12px;
            }
        </style></head><body><button id="trigger">HTML</button></body></html>"#,
    );

    let button = crate::dom::query_selector(&doc.root, "#trigger").unwrap();
    let after = button
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("button should have an ::after pseudo box");
    assert!(
        after.layout.border_rect.w >= 19.0 && after.layout.border_rect.h >= 11.0,
        "sized empty-content pseudo should create a box, got {:?}",
        after.layout.border_rect
    );
}

// ── 7. text inputs always show :focus-visible even on mouse click ─────────────

#[test]
fn mouse_focused_text_input_shows_outline() {
    // <input type="text"> is a text-entry control: :focus-visible must match
    // even when keyboard_focus=false (mouse click).
    let mut doc = parse_and_layout("<html><body><input type=\"text\" id=inp></body></html>");
    let inp_id = crate::dom::query_selector(&doc.root, "#inp")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = inp_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );

    let inp = crate::dom::query_selector(&doc.root, "#inp").unwrap();
    assert!(
        inp.style.outline_width > 0.0,
        "mouse-focused text input must still get :focus-visible outline"
    );
}

#[test]
fn mouse_focused_button_no_outline() {
    // <button> is not text-entry: no :focus-visible on mouse click.
    let mut doc = parse_and_layout("<html><body><button id=btn>Click</button></body></html>");
    let btn_id = crate::dom::query_selector(&doc.root, "#btn")
        .map(|n| n.node_id)
        .unwrap();
    doc.focused_box = btn_id;
    doc.keyboard_focus = false;
    doc.stylesheet.rebuild_index();
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        doc.viewport_w,
        doc.viewport_h,
        doc.focused_box,
        false,
    );

    let btn = crate::dom::query_selector(&doc.root, "#btn").unwrap();
    assert_eq!(
        btn.style.outline_style,
        BorderStyle::None,
        "mouse-focused button must NOT show :focus-visible outline"
    );
}

// ── 8. contenteditable included in tab order ──────────────────────────────────

#[test]
fn contenteditable_is_focusable() {
    let mut doc = parse_and_layout(
        "<html><body><div id=ed contenteditable=\"true\">Edit me</div></body></html>",
    );
    doc.focus_next();
    let ed_id = crate::dom::query_selector(&doc.root, "#ed")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, ed_id,
        "contenteditable must be in tab order"
    );
}

// ── 8. display:none elements skipped ─────────────────────────────────────────

#[test]
fn display_none_element_skipped_in_tab_order() {
    let mut doc = parse_and_layout(
        "<html><head><style>#hidden { display: none; }</style></head>\
         <body><button id=hidden>Hidden</button><button id=vis>Visible</button></body></html>",
    );
    doc.focus_next();
    let vis_id = crate::dom::query_selector(&doc.root, "#vis")
        .map(|n| n.node_id)
        .unwrap();
    assert_eq!(
        doc.focused_box, vis_id,
        "display:none button must not be in tab order"
    );
}
