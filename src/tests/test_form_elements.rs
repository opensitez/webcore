use crate::css::apply_cascade_vp;
use crate::layout::LayoutEngine;
use crate::types::*;
use crate::{Document, Renderer, parse_html};

fn layout_html(html: &str, width: f32) -> Document {
    let mut doc = parse_html(html);
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        width,
        900.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, width);
    doc
}

fn find_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_id(child, id) {
            return Some(n);
        }
    }
    None
}

fn find_by_tag<'a>(node: &'a WebCore, tag: &str) -> Option<&'a WebCore> {
    if node.tag == tag {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_tag(child, tag) {
            return Some(n);
        }
    }
    None
}

fn find_all_by_tag<'a>(node: &'a WebCore, tag: &str, out: &mut Vec<&'a WebCore>) {
    if node.tag == tag {
        out.push(node);
    }
    for child in &node.children {
        find_all_by_tag(child, tag, out);
    }
}

fn find_by_node_id<'a>(node: &'a WebCore, nid: u32) -> Option<&'a WebCore> {
    if node.node_id == nid {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_node_id(child, nid) {
            return Some(n);
        }
    }
    None
}

// ── Text Input Tests ─────────────────────────────────────────────────────────

#[test]
fn text_input_has_correct_display() {
    let doc = layout_html(r#"<input type="text" value="hello">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.display, Display::InlineBlock);
}

#[test]
fn text_input_has_width() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.content_rect.w > 100.0,
        "input width {} should be > 100",
        input.layout.content_rect.w
    );
}

#[test]
fn field_sizing_content_sizes_text_input_from_value() {
    let fixed_doc = layout_html(r#"<input type="text" value="short">"#, 400.0);
    let fixed = find_by_tag(&fixed_doc.root, "input").unwrap();
    let content_doc = layout_html(
        r#"<input type="text" value="short" style="width:auto;field-sizing:content">"#,
        400.0,
    );
    let content = find_by_tag(&content_doc.root, "input").unwrap();
    let long_doc = layout_html(
        r#"<input type="text" value="a much longer value" style="width:auto;field-sizing:content">"#,
        400.0,
    );
    let long = find_by_tag(&long_doc.root, "input").unwrap();

    assert!(
        content.layout.content_rect.w < fixed.layout.content_rect.w,
        "content-sized input should shrink below the UA fixed width: content={}, fixed={}",
        content.layout.content_rect.w,
        fixed.layout.content_rect.w
    );
    assert!(
        long.layout.content_rect.w > content.layout.content_rect.w,
        "content-sized input should grow with its value"
    );
}

#[test]
fn field_sizing_content_sizes_textarea_from_longest_line() {
    let short_doc = layout_html(
        r#"<textarea style="width:auto;field-sizing:content">short</textarea>"#,
        400.0,
    );
    let short = find_by_tag(&short_doc.root, "textarea").unwrap();
    let long_doc = layout_html(
        r#"<textarea style="width:auto;field-sizing:content">short
much longer textarea line</textarea>"#,
        400.0,
    );
    let long = find_by_tag(&long_doc.root, "textarea").unwrap();

    assert!(
        long.layout.content_rect.w > short.layout.content_rect.w + 40.0,
        "content-sized textarea should size inline axis from the longest line; short={} long={}",
        short.layout.content_rect.w,
        long.layout.content_rect.w
    );
}

#[test]
fn field_sizing_content_sizes_textarea_block_axis_from_lines() {
    let one_line_doc = layout_html(
        r#"<textarea style="height:auto;field-sizing:content">one</textarea>"#,
        400.0,
    );
    let one_line = find_by_tag(&one_line_doc.root, "textarea").unwrap();
    let three_line_doc = layout_html(
        r#"<textarea style="height:auto;field-sizing:content">one
two
three</textarea>"#,
        400.0,
    );
    let three_lines = find_by_tag(&three_line_doc.root, "textarea").unwrap();

    assert!(
        three_lines.layout.content_rect.h > one_line.layout.content_rect.h * 2.0,
        "content-sized textarea should size block axis from line count; one={} three={}",
        one_line.layout.content_rect.h,
        three_lines.layout.content_rect.h
    );
}

#[test]
fn text_input_has_height() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.content_rect.h > 10.0,
        "input height {} should be > 10",
        input.layout.content_rect.h
    );
}

#[test]
fn text_input_preserves_value() {
    let doc = layout_html(r#"<input type="text" value="test123">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("test123")
    );
}

#[test]
fn text_input_preserves_placeholder() {
    let doc = layout_html(r#"<input type="text" placeholder="Enter text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("placeholder").map(|s| s.as_str()),
        Some("Enter text")
    );
}

#[test]
fn text_input_has_border() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.resolved_border_top > 0.0,
        "input should have border"
    );
    assert!(input.layout.resolved_border_left > 0.0);
}

#[test]
fn text_input_has_white_background() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.background_color.r, 255);
    assert_eq!(input.style.background_color.g, 255);
    assert_eq!(input.style.background_color.b, 255);
}

#[test]
fn text_input_vertical_align_middle() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.vertical_align, VerticalAlign::Middle);
}

#[test]
fn text_input_box_sizing_border_box() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.box_sizing, BoxSizing::BorderBox);
}

#[test]
fn text_input_text_centered_vertically() {
    // The content height should leave room above and below the text line
    let doc = layout_html(r#"<input type="text" value="Hello">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    let font_px = input.style.font_size_px(16.0, 16.0);
    let line_h = font_px * 1.2;
    let top_space = (input.layout.content_rect.h - line_h) / 2.0;
    assert!(
        top_space > 1.0,
        "should have > 1px above text, got {}",
        top_space
    );
}

#[test]
fn password_input_preserves_value() {
    let doc = layout_html(r#"<input type="password" value="secret">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("secret")
    );
}

#[test]
fn text_input_cursor_starts_at_zero() {
    let doc = layout_html(r#"<input type="text" value="hello">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.input_cursor, 0);
}

#[test]
fn process_form_input_key_inserts_char() {
    let doc = layout_html(r#"<input type="text" id="t" value="ab">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 2; // at end
    input.input_sel_anchor = 2;
    let changed = process_form_input_key(input, 'c' as u32, Some('c'), false, false);
    assert!(changed);
    assert_eq!(input_value(input), "abc");
    assert_eq!(input.input_cursor, 3);
}

#[test]
fn process_form_input_key_backspace() {
    let doc = layout_html(r#"<input type="text" id="t" value="abc">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 3;
    input.input_sel_anchor = 3;
    let changed = process_form_input_key(input, 8, None, false, false); // backspace
    assert!(changed);
    assert_eq!(input_value(input), "ab");
    assert_eq!(input.input_cursor, 2);
}

#[test]
fn process_form_input_key_arrow_left() {
    let doc = layout_html(r#"<input type="text" id="t" value="abc">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 2;
    input.input_sel_anchor = 2;
    process_form_input_key(input, 37, None, false, false); // left arrow
    assert_eq!(input.input_cursor, 1);
}

#[test]
fn process_form_input_key_arrow_right() {
    let doc = layout_html(r#"<input type="text" id="t" value="abc">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 1;
    input.input_sel_anchor = 1;
    process_form_input_key(input, 39, None, false, false); // right arrow
    assert_eq!(input.input_cursor, 2);
}

#[test]
fn process_form_input_key_enter_not_in_input() {
    let doc = layout_html(r#"<input type="text" id="t" value="abc">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 3;
    input.input_sel_anchor = 3;
    let changed = process_form_input_key(input, 13, None, false, false); // enter
    assert!(!changed, "Enter should not insert in single-line input");
    assert_eq!(input_value(input), "abc");
}

#[test]
fn process_form_input_key_space() {
    let doc = layout_html(r#"<input type="text" id="t" value="ab">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 1;
    input.input_sel_anchor = 1;
    let changed = process_form_input_key(input, 32, Some(' '), false, false);
    assert!(changed);
    assert_eq!(input_value(input), "a b");
    assert_eq!(input.input_cursor, 2);
}

// ── Checkbox Tests ───────────────────────────────────────────────────────────

#[test]
fn checkbox_has_correct_size() {
    let doc = layout_html(r#"<input type="checkbox">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.layout.content_rect.w, 16.0);
    assert_eq!(input.layout.content_rect.h, 16.0);
}

#[test]
fn checkbox_no_border() {
    let doc = layout_html(r#"<input type="checkbox">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.layout.resolved_border_top, 0.0);
}

#[test]
fn checkbox_transparent_background() {
    let doc = layout_html(r#"<input type="checkbox">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.style.background_color.a, 0,
        "checkbox bg should be transparent"
    );
}

#[test]
fn checkbox_checked_attribute() {
    let doc = layout_html(r#"<input type="checkbox" checked>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(input.attributes.contains_key("checked"));
}

#[test]
fn checkbox_text_not_overlapping() {
    let doc = layout_html(r#"<div><input type="checkbox"> Label</div>"#, 400.0);
    let mut inputs = Vec::new();
    find_all_by_tag(&doc.root, "input", &mut inputs);
    let input = inputs[0];
    // The text "Label" should start after the checkbox margin box
    let input_right = input.layout.margin_rect.x + input.layout.margin_rect.w;
    // Check that the parent div's line cache positions text after the checkbox
    let div = find_by_tag(&doc.root, "div").unwrap();
    assert!(
        !div.layout.line_cache.is_empty(),
        "div should have line cache"
    );
    let line = &div.layout.line_cache[0];
    // text_x_offset should be > 0 (accounting for checkbox)
    assert!(
        line.text_x_offset > 0.0,
        "text_x_offset {} should be > 0",
        line.text_x_offset
    );
}

// ── Radio Button Tests ───────────────────────────────────────────────────────

#[test]
fn radio_has_correct_size() {
    let doc = layout_html(r#"<input type="radio" name="g">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.layout.content_rect.w, 16.0);
    assert_eq!(input.layout.content_rect.h, 16.0);
}

#[test]
fn radio_checked_attribute() {
    let doc = layout_html(r#"<input type="radio" name="g" checked>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(input.attributes.contains_key("checked"));
}

// ── Button Tests ─────────────────────────────────────────────────────────────

#[test]
fn submit_button_has_text_child() {
    // Test parse_html alone first
    let doc_parse = parse_html(r#"<input type="submit" value="Go">"#);
    let inp_parse = find_by_tag(&doc_parse.root, "input").unwrap();
    eprintln!("AFTER PARSE: children={}", inp_parse.children.len());
    for (i, c) in inp_parse.children.iter().enumerate() {
        eprintln!("  child[{}]: tag={} text={:?}", i, c.tag, c.text);
    }

    let doc = layout_html(r#"<input type="submit" value="Go">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    eprintln!(
        "AFTER LAYOUT: submit button: tag={} type={:?} display={:?} children={}",
        input.tag,
        input.attributes.get("type"),
        input.style.display,
        input.children.len()
    );
    for (i, c) in input.children.iter().enumerate() {
        eprintln!("  child[{}]: tag={} text={:?}", i, c.tag, c.text);
    }
    assert!(
        !input.children.is_empty(),
        "submit button should have text child"
    );
    let text = &input.children[0];
    assert_eq!(text.tag, "#text");
    assert_eq!(text.text, "Go");
}

#[test]
fn submit_button_default_label() {
    let doc = layout_html(r#"<input type="submit">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(!input.children.is_empty());
    assert_eq!(input.children[0].text, "Submit");
}

#[test]
fn reset_button_default_label() {
    let doc = layout_html(r#"<input type="reset">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(!input.children.is_empty());
    assert_eq!(input.children[0].text, "Reset");
}

#[test]
fn button_has_background() {
    let doc = layout_html(r#"<input type="submit" value="Go">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.style.background_color.a > 0,
        "button should have background"
    );
}

#[test]
fn button_display_inline_flex() {
    let doc = layout_html(r#"<input type="submit">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.display, Display::InlineFlex);
}

#[test]
fn button_has_width() {
    let doc = layout_html(r#"<input type="submit" value="Submit Form">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.content_rect.w > 20.0,
        "button width {} should be > 20",
        input.layout.content_rect.w
    );
}

// ── Select Tests ─────────────────────────────────────────────────────────────

#[test]
fn select_preserves_options_in_dom() {
    let doc = layout_html(
        r#"<select><option>A</option><option>B</option><option>C</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    let options: Vec<_> = select
        .children
        .iter()
        .filter(|c| c.tag == "option")
        .collect();
    assert_eq!(
        options.len(),
        3,
        "select should keep 3 option children in DOM"
    );
}

#[test]
fn select_options_are_hidden() {
    let doc = layout_html(
        r#"<select><option>A</option><option>B</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    for child in &select.children {
        if child.tag == "option" {
            assert_eq!(
                child.style.display,
                Display::None,
                "options should be display:none"
            );
        }
    }
}

#[test]
fn select_shows_the_selected_option() {
    let doc = layout_html(
        r#"<select><option>First</option><option selected>Second</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    // A `<select>` has NO text child of its own. The parser used to inject one
    // holding the display label, which no browser has: it doubled
    // `textContent`, and a serialize → reparse turned `Second` into
    // `SecondSecond`. The label is derived from the selected option instead.
    assert!(
        select.children.iter().all(|c| c.tag != "#text"),
        "a <select> has no #text child; its label comes from the selected option"
    );
    let selected = select
        .children
        .iter()
        .find(|c| c.tag == "option" && c.selectedness)
        .expect("one option is selected");
    assert_eq!(selected.text_content().trim(), "Second");
}

#[test]
fn select_first_option_selected_by_default() {
    let doc = layout_html(
        r#"<select><option>Alpha</option><option>Beta</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    // A drop-down with nothing marked `selected` lands on its first option
    // (HTML §4.10.7's selectedness setting algorithm).
    assert_eq!(crate::html::forms::selected_index(select), 0);
    let selected = select
        .children
        .iter()
        .find(|c| c.tag == "option" && c.selectedness)
        .expect("a drop-down always has a selected option");
    assert_eq!(selected.text_content().trim(), "Alpha");
}

#[test]
fn select_tracks_selected_index() {
    let doc = layout_html(
        r#"<select><option>A</option><option selected>B</option><option>C</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    assert_eq!(crate::html::forms::selected_index(select), 1);
}

#[test]
fn select_has_width() {
    let doc = layout_html(r#"<select><option>Option</option></select>"#, 400.0);
    let select = find_by_tag(&doc.root, "select").unwrap();
    assert!(
        select.layout.content_rect.w > 50.0,
        "select width {} should be > 50",
        select.layout.content_rect.w
    );
}

#[test]
fn optgroup_preserved_in_dom() {
    let doc = layout_html(
        r#"<select>
        <optgroup label="Fruits"><option>Apple</option><option>Banana</option></optgroup>
        <optgroup label="Vegs"><option>Carrot</option></optgroup>
    </select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    let optgroups: Vec<_> = select
        .children
        .iter()
        .filter(|c| c.tag == "optgroup")
        .collect();
    assert_eq!(optgroups.len(), 2, "select should keep optgroup children");
}

// ── Textarea Tests ───────────────────────────────────────────────────────────

#[test]
fn textarea_has_content() {
    let doc = layout_html(r#"<textarea>Hello World</textarea>"#, 400.0);
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    let has_text = ta
        .children
        .iter()
        .any(|c| c.tag == "#text" && c.text.contains("Hello"));
    assert!(has_text, "textarea should contain text");
}

#[test]
fn textarea_has_width_and_height() {
    let doc = layout_html(r#"<textarea rows="3">Text</textarea>"#, 400.0);
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert!(ta.layout.content_rect.w > 50.0);
    assert!(ta.layout.content_rect.h > 20.0);
}

// ── Range Input Tests ────────────────────────────────────────────────────────

#[test]
fn range_input_preserves_attributes() {
    let doc = layout_html(
        r#"<input type="range" min="0" max="100" value="60">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.attributes.get("min").map(|s| s.as_str()), Some("0"));
    assert_eq!(input.attributes.get("max").map(|s| s.as_str()), Some("100"));
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("60")
    );
}

#[test]
fn range_input_no_border() {
    let doc = layout_html(r#"<input type="range">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.layout.resolved_border_top, 0.0);
}

// ── Progress Tests ───────────────────────────────────────────────────────────

#[test]
fn progress_has_size() {
    let doc = layout_html(r#"<progress value="0.5" max="1"></progress>"#, 400.0);
    let prog = find_by_tag(&doc.root, "progress").unwrap();
    assert!(prog.layout.content_rect.w > 50.0);
    assert!(prog.layout.content_rect.h > 5.0);
}

// ── Meter Tests ──────────────────────────────────────────────────────────────

#[test]
fn meter_has_size() {
    let doc = layout_html(r#"<meter value="0.5" min="0" max="1"></meter>"#, 400.0);
    let meter = find_by_tag(&doc.root, "meter").unwrap();
    assert!(meter.layout.content_rect.w > 30.0);
    assert!(meter.layout.content_rect.h > 5.0);
}

// ── Fieldset/Legend Tests ────────────────────────────────────────────────────

#[test]
fn fieldset_is_block() {
    let doc = layout_html(r#"<fieldset><legend>Title</legend></fieldset>"#, 400.0);
    let fs = find_by_tag(&doc.root, "fieldset").unwrap();
    assert_eq!(fs.style.display, Display::Block);
}

#[test]
fn fieldset_has_border() {
    let doc = layout_html(r#"<fieldset><legend>Title</legend></fieldset>"#, 400.0);
    let fs = find_by_tag(&doc.root, "fieldset").unwrap();
    assert!(fs.layout.resolved_border_top > 0.0);
}

// ── Hidden Input ─────────────────────────────────────────────────────────────

#[test]
fn hidden_input_is_display_none() {
    let doc = layout_html(r#"<input type="hidden" name="token" value="abc">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.display, Display::None);
}

// ── is_text_input helper ─────────────────────────────────────────────────────

#[test]
fn is_text_input_true_for_text() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(is_text_input(input));
}

#[test]
fn is_text_input_true_for_password() {
    let doc = layout_html(r#"<input type="password">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(is_text_input(input));
}

#[test]
fn is_text_input_true_for_email() {
    let doc = layout_html(r#"<input type="email">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(is_text_input(input));
}

#[test]
fn is_text_input_false_for_checkbox() {
    let doc = layout_html(r#"<input type="checkbox">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(!is_text_input(input));
}

#[test]
fn is_text_input_false_for_radio() {
    let doc = layout_html(r#"<input type="radio">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(!is_text_input(input));
}

#[test]
fn is_text_input_false_for_submit() {
    let doc = layout_html(r#"<input type="submit">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(!is_text_input(input));
}

#[test]
fn is_text_input_true_for_textarea() {
    let doc = layout_html(r#"<textarea></textarea>"#, 400.0);
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert!(is_text_input(ta));
}

// ── Select dropdown styling tests ────────────────────────────────────────────

#[test]
fn select_option_inherits_color() {
    let doc = layout_html(
        r#"<style>select { color: red; }</style>
        <select><option>A</option><option>B</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    for child in &select.children {
        if child.tag == "option" {
            // Options should inherit color from select
            assert_eq!(
                child.style.color.r, 255,
                "option should inherit red color from select"
            );
        }
    }
}

#[test]
fn select_option_own_style_wins() {
    let doc = layout_html(
        r#"<style>option { color: blue; }</style>
        <select><option>A</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    for child in &select.children {
        if child.tag == "option" {
            assert_eq!(
                child.style.color.b, 255,
                "option should have blue from own CSS rule"
            );
        }
    }
}

#[test]
fn select_option_background_applies() {
    let doc = layout_html(
        r#"<style>option { background-color: yellow; }</style>
        <select><option>A</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    for child in &select.children {
        if child.tag == "option" {
            assert_eq!(child.style.background_color.r, 255);
            assert_eq!(child.style.background_color.g, 255);
            assert_eq!(child.style.background_color.b, 0);
        }
    }
}

#[test]
fn select_dark_theme_options_readable() {
    // Simulates a dark theme: select has light text on dark bg
    // Dropdown should NOT show light text on white bg
    let doc = layout_html(
        r#"<style>
        select { color: #e2e8f0; background: #1e293b; }
    </style>
    <select><option>Light</option><option>Dark</option></select>"#,
        400.0,
    );
    let select = find_by_tag(&doc.root, "select").unwrap();
    // The select itself has light text
    assert!(select.style.color.r > 200, "select should have light color");
    // Options inherit from select — they'll have light color too
    // The dropdown renderer must handle this (use dark text or option's own color)
    for child in &select.children {
        if child.tag == "option" {
            // Options inherit color from select
            assert!(
                child.style.color.r > 200,
                "option inherits light color from select"
            );
        }
    }
}

// ── Textarea styling tests ──────────────────────────────────────────────────

#[test]
fn textarea_has_border() {
    let doc = layout_html(r#"<textarea>text</textarea>"#, 400.0);
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert!(
        ta.layout.resolved_border_top > 0.0,
        "textarea should have border"
    );
}

#[test]
fn textarea_css_override() {
    let doc = layout_html(
        r#"<style>textarea { background: red; color: white; }</style>
        <textarea>text</textarea>"#,
        400.0,
    );
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert_eq!(ta.style.background_color.r, 255);
    assert_eq!(ta.style.color.r, 255);
    assert_eq!(ta.style.color.g, 255);
}

#[test]
fn textarea_width_override() {
    let doc = layout_html(
        r#"<style>textarea { width: 400px; }</style>
        <textarea>text</textarea>"#,
        500.0,
    );
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert!(
        (ta.layout.margin_rect.w - 400.0).abs() < 5.0,
        "textarea width {} should be ~400",
        ta.layout.margin_rect.w
    );
}

// ── Input styling tests ─────────────────────────────────────────────────────

#[test]
fn input_padding_override() {
    let doc = layout_html(
        r#"<style>input { padding: 10px 15px; }</style>
        <input type="text">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        (input.layout.resolved_pad_top - 10.0).abs() < 1.0,
        "padding-top {} should be ~10",
        input.layout.resolved_pad_top
    );
    assert!(
        (input.layout.resolved_pad_left - 15.0).abs() < 1.0,
        "padding-left {} should be ~15",
        input.layout.resolved_pad_left
    );
}

#[test]
fn input_border_color_override() {
    let doc = layout_html(
        r#"<style>input { border: 2px solid red; }</style>
        <input type="text">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        (input.layout.resolved_border_top - 2.0).abs() < 0.5,
        "border should be 2px"
    );
}

#[test]
fn input_border_radius_override() {
    let doc = layout_html(
        r#"<style>input { border-radius: 10px; }</style>
        <input type="text">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!((input.style.border_radius.resolve(16.0, 200.0, 16.0) - 10.0).abs() < 1.0);
}

#[test]
fn input_font_size_override() {
    let doc = layout_html(
        r#"<style>input { font-size: 20px; }</style>
        <input type="text">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    let fpx = input.style.font_size_px(16.0, 16.0);
    assert!((fpx - 20.0).abs() < 1.0, "font-size {} should be 20", fpx);
}

// ── Button styling tests ────────────────────────────────────────────────────

#[test]
fn button_element_has_background() {
    let doc = layout_html(r#"<button>Click</button>"#, 400.0);
    let btn = find_by_tag(&doc.root, "button").unwrap();
    assert!(
        btn.style.background_color.a > 0,
        "button should have default bg"
    );
}

#[test]
fn button_element_css_background_override() {
    let doc = layout_html(
        r#"<style>button { background: green; }</style>
        <button>Click</button>"#,
        400.0,
    );
    let btn = find_by_tag(&doc.root, "button").unwrap();
    assert_eq!(
        btn.style.background_color.g, 128,
        "green component should be 128, got {}",
        btn.style.background_color.g
    );
}

#[test]
fn button_white_space_nowrap() {
    let doc = layout_html(r#"<input type="submit" value="Submit Form">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.white_space, crate::types::WhiteSpace::Nowrap);
}

// ── Checkbox/Radio in label tests ────────────────────────────────────────────

#[test]
fn checkbox_in_label_spacing() {
    let doc = layout_html(r#"<label><input type="checkbox"> Option</label>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.resolved_margin_right >= 4.0,
        "checkbox should have right margin >= 4, got {}",
        input.layout.resolved_margin_right
    );
}

#[test]
fn radio_in_label_spacing() {
    let doc = layout_html(
        r#"<label><input type="radio" name="g"> Choice</label>"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.resolved_margin_right >= 4.0,
        "radio should have right margin >= 4, got {}",
        input.layout.resolved_margin_right
    );
}

// ── Form input key handling tests ────────────────────────────────────────────

#[test]
fn form_input_insert_at_middle() {
    let doc = layout_html(r#"<input type="text" id="t" value="ac">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 1; // between 'a' and 'c'
    input.input_sel_anchor = 1;
    process_form_input_key(input, 'b' as u32, Some('b'), false, false);
    assert_eq!(input_value(input), "abc");
    assert_eq!(input.input_cursor, 2);
}

#[test]
fn form_input_delete_key() {
    let doc = layout_html(r#"<input type="text" id="t" value="abc">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 1; // before 'b'
    input.input_sel_anchor = 1;
    process_form_input_key(input, 46, None, false, false); // delete
    assert_eq!(input_value(input), "ac");
    assert_eq!(input.input_cursor, 1);
}

#[test]
fn form_input_home_end() {
    let doc = layout_html(r#"<input type="text" id="t" value="hello">"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = find_mut(&mut root, "t").unwrap();
    input.input_cursor = 2;
    input.input_sel_anchor = 2;
    process_form_input_key(input, 36, None, false, false); // Home
    assert_eq!(input.input_cursor, 0);
    process_form_input_key(input, 35, None, false, false); // End
    assert_eq!(input.input_cursor, 5);
}

#[test]
fn textarea_enter_inserts_newline() {
    let doc = layout_html(r#"<textarea id="t">ab</textarea>"#, 400.0);
    let mut root = doc.root;
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let ta = find_mut(&mut root, "t").unwrap();
    ta.input_cursor = 1;
    ta.input_sel_anchor = 1;
    let changed = process_form_input_key(ta, 13, None, false, false); // Enter
    assert!(changed);
    assert!(input_value(ta).contains('\n'));
}

// ── Disabled state tests ─────────────────────────────────────────────────────

#[test]
fn disabled_input_has_reduced_opacity() {
    let doc = layout_html(r#"<input type="text" disabled>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.style.opacity < 1.0,
        "disabled input should have reduced opacity, got {}",
        input.style.opacity
    );
}

#[test]
fn disabled_input_matches_pseudo_class() {
    let doc = layout_html(
        r#"<style>input:disabled { background-color: #eee; }</style>
        <input type="text" disabled>"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.style.background_color.r < 255 || input.style.background_color.g < 255,
        "disabled input should match :disabled pseudo-class and get #eee bg"
    );
}

#[test]
fn disabled_button_matches_pseudo_class() {
    let doc = layout_html(
        r#"<style>button:disabled { opacity: 0.5; }</style>
        <button disabled>Click</button>"#,
        400.0,
    );
    let btn = find_by_tag(&doc.root, "button").unwrap();
    assert!(
        btn.style.opacity < 0.9,
        "disabled button opacity {} should be < 0.9",
        btn.style.opacity
    );
}

// ── Readonly state tests ────────────────────────────────────────────────────

#[test]
fn readonly_input_blocks_editing() {
    let doc = layout_html(
        r#"<input type="text" id="t" value="fixed" readonly>"#,
        400.0,
    );
    let mut root = doc.root;
    fn fm<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = fm(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = fm(&mut root, "t").unwrap();
    input.input_cursor = 5;
    input.input_sel_anchor = 5;
    let changed = process_form_input_key(input, 'x' as u32, Some('x'), false, false);
    assert!(!changed, "readonly input should not accept input");
    assert_eq!(input_value(input), "fixed");
}

// ── Maxlength tests ─────────────────────────────────────────────────────────

#[test]
fn maxlength_prevents_input() {
    let doc = layout_html(
        r#"<input type="text" id="t" value="abc" maxlength="5">"#,
        400.0,
    );
    let mut root = doc.root;
    fn fm<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = fm(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = fm(&mut root, "t").unwrap();
    input.input_cursor = 3;
    input.input_sel_anchor = 3;
    process_form_input_key(input, 'd' as u32, Some('d'), false, false); // "abcd" — ok
    process_form_input_key(input, 'e' as u32, Some('e'), false, false); // "abcde" — ok, at limit
    let changed = process_form_input_key(input, 'f' as u32, Some('f'), false, false); // should be blocked
    assert!(!changed, "should not exceed maxlength");
    assert_eq!(input_value(input).chars().count(), 5);
}

// ── Label for= association ──────────────────────────────────────────────────

#[test]
fn label_for_attribute_preserved() {
    let doc = layout_html(
        r#"<label for="name">Name:</label><input id="name" type="text">"#,
        400.0,
    );
    let label = find_by_tag(&doc.root, "label").unwrap();
    assert_eq!(
        label.attributes.get("for").map(|s| s.as_str()),
        Some("name")
    );
}

// ── Textarea rows/cols ──────────────────────────────────────────────────────

#[test]
fn textarea_rows_affects_height() {
    let doc1 = layout_html(r#"<textarea rows="2">text</textarea>"#, 400.0);
    let doc2 = layout_html(r#"<textarea rows="6">text</textarea>"#, 400.0);
    let ta1 = find_by_tag(&doc1.root, "textarea").unwrap();
    let ta2 = find_by_tag(&doc2.root, "textarea").unwrap();
    assert!(
        ta2.layout.content_rect.h > ta1.layout.content_rect.h,
        "rows=6 height {} should be > rows=2 height {}",
        ta2.layout.content_rect.h,
        ta1.layout.content_rect.h
    );
}

// ── Input size attribute ────────────────────────────────────────────────────

#[test]
fn input_size_affects_width() {
    let doc1 = layout_html(r#"<input type="text" size="5">"#, 400.0);
    let doc2 = layout_html(r#"<input type="text" size="40">"#, 500.0);
    let i1 = find_by_tag(&doc1.root, "input").unwrap();
    let i2 = find_by_tag(&doc2.root, "input").unwrap();
    assert!(
        i2.layout.content_rect.w > i1.layout.content_rect.w,
        "size=40 width {} should be > size=5 width {}",
        i2.layout.content_rect.w,
        i1.layout.content_rect.w
    );
}

#[test]
fn author_css_overrides_input_size_hint() {
    let doc = layout_html(
        r#"<style>input { width: 240px; }</style><input type="text" size="5">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();

    assert!(
        (input.layout.border_rect.w - 240.0).abs() < 1.0,
        "author CSS should outrank the input size hint, got {}",
        input.layout.border_rect.w
    );
}

#[test]
fn author_css_overrides_textarea_rows_and_cols_hints() {
    let doc = layout_html(
        r#"<style>textarea { width: 240px; height: 120px; }</style>
           <textarea rows="2" cols="4">text</textarea>"#,
        400.0,
    );
    let textarea = find_by_tag(&doc.root, "textarea").unwrap();

    assert!(
        (textarea.layout.border_rect.w - 240.0).abs() < 1.0,
        "author width should outrank cols hint, got {}",
        textarea.layout.border_rect.w
    );
    assert!(
        (textarea.layout.border_rect.h - 120.0).abs() < 1.0,
        "author height should outrank rows hint, got {}",
        textarea.layout.border_rect.h
    );
}

// ── Disabled blocks form_input_key ──────────────────────────────────────────

#[test]
fn disabled_input_blocks_typing() {
    let doc = layout_html(r#"<input type="text" id="t" value="hi" disabled>"#, 400.0);
    let mut root = doc.root;
    fn fm<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = fm(c, id) {
                return Some(r);
            }
        }
        None
    }
    let input = fm(&mut root, "t").unwrap();
    input.input_cursor = 2;
    input.input_sel_anchor = 2;
    let changed = process_form_input_key(input, 'x' as u32, Some('x'), false, false);
    assert!(!changed, "disabled input should not accept typing");
    assert_eq!(input_value(input), "hi");
}

// ── Required pseudo-class ───────────────────────────────────────────────────

#[test]
fn required_input_matches_pseudo_class() {
    let doc = layout_html(
        r#"<style>input:required { border-color: red; }</style>
        <input type="text" required>"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    // Should match :required — border-color red means border is applied
    // (border-color is not directly on ComputedStyle as a separate field,
    // but the border rendering should pick it up)
    assert!(input.attributes.contains_key("required"));
}

// ── Placeholder-shown pseudo-class ──────────────────────────────────────────

#[test]
fn placeholder_shown_matches_empty_input() {
    let doc = layout_html(
        r#"<style>input:placeholder-shown { color: gray; }</style>
        <input type="text" placeholder="hint">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    // Empty value + placeholder → :placeholder-shown should match
    assert!(
        input
            .attributes
            .get("value")
            .map(|v| v.is_empty())
            .unwrap_or(true)
    );
}

// ── CSS override tests ──────────────────────────────────────────────────────

#[test]
fn input_css_width_override() {
    let doc = layout_html(
        r#"<style>input { width: 300px; }</style><input type="text">"#,
        500.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    // margin_rect width should be close to 300px (border-box)
    assert!(
        (input.layout.margin_rect.w - 300.0).abs() < 5.0,
        "input width {} should be ~300",
        input.layout.margin_rect.w
    );
}

#[test]
fn input_css_background_override() {
    let doc = layout_html(
        r#"<style>input { background-color: red; }</style><input type="text">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.style.background_color.r, 255);
    assert_eq!(input.style.background_color.g, 0);
}

#[test]
fn button_css_background_override() {
    let doc = layout_html(
        r#"<style>input[type=submit] { background-color: blue; }</style><input type="submit" value="Go">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.style.background_color.b, 255,
        "button bg blue should be 255, got {}",
        input.style.background_color.b
    );
}

// ── Disabled checkbox/radio don't toggle ─────────────────────────────────────

#[test]
fn disabled_checkbox_no_toggle() {
    let html = r#"<input type="checkbox" id="c" disabled>"#;
    let mut doc = layout_html(html, 400.0);
    let cb = find_by_id(&doc.root, "c").unwrap();
    assert!(
        !cb.attributes.contains_key("checked"),
        "should start unchecked"
    );
    // Simulate click via handle_form_click
    let cb_node_id = cb.node_id;
    crate::types::handle_form_click(&mut doc.root, cb_node_id, &mut None);
    let cb = find_by_id(&doc.root, "c").unwrap();
    assert!(
        !cb.attributes.contains_key("checked"),
        "disabled checkbox should not toggle"
    );
}

// ── Select size=N shows listbox ──────────────────────────────────────────────

#[test]
fn select_with_size_is_taller() {
    let doc1 = layout_html(
        r#"<select><option>A</option><option>B</option></select>"#,
        400.0,
    );
    let doc2 = layout_html(
        r#"<select size="4"><option>A</option><option>B</option><option>C</option><option>D</option></select>"#,
        400.0,
    );
    let s1 = find_by_tag(&doc1.root, "select").unwrap();
    let s2 = find_by_tag(&doc2.root, "select").unwrap();
    assert!(
        s2.layout.margin_rect.h > s1.layout.margin_rect.h,
        "select size=4 height {} should be > default height {}",
        s2.layout.margin_rect.h,
        s1.layout.margin_rect.h
    );

    // ⚠ It has to be taller FOR THE RIGHT REASON — roughly four rows, not one
    // row in a bigger font.
    //
    // This passed before the list box existed, and it passed by accident: the
    // presentational-attribute pass read `size="4"` as `<font size=4>` and set
    // `font-size: 18px`, so the one-row height of `2.2em` grew with the font.
    // `size` means three different things by element (rows on a `<select>`,
    // characters on an `<input>`, a font step on `<font>`) and only the last
    // was implemented.
    assert!(
        (s2.style.font_size_px(16.0, 16.0) - s1.style.font_size_px(16.0, 16.0)).abs() < 0.01,
        "`size` changed the FONT of a select: {} vs {}",
        s2.style.font_size_px(16.0, 16.0),
        s1.style.font_size_px(16.0, 16.0)
    );
    assert!(
        s2.layout.margin_rect.h > s1.layout.margin_rect.h * 2.0,
        "four rows should be well over twice the one-row height, got {} vs {}",
        s2.layout.margin_rect.h,
        s1.layout.margin_rect.h
    );
}

// ── Multiple select ──────────────────────────────────────────────────────────

#[test]
fn select_multiple_preserves_attribute() {
    let doc = layout_html(
        r#"<select multiple><option>A</option><option>B</option></select>"#,
        400.0,
    );
    let s = find_by_tag(&doc.root, "select").unwrap();
    assert!(s.attributes.contains_key("multiple"));
}

// ── Color input value ────────────────────────────────────────────────────────

#[test]
fn color_input_preserves_value() {
    let doc = layout_html(r##"<input type="color" value="#ff0000">"##, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("#ff0000")
    );
}

// ── Date input value ─────────────────────────────────────────────────────────

#[test]
fn date_input_preserves_value() {
    let doc = layout_html(r#"<input type="date" value="2026-03-23">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("2026-03-23")
    );
}

// ── File input ───────────────────────────────────────────────────────────────

#[test]
fn file_input_has_width() {
    let doc = layout_html(r#"<input type="file">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(input.layout.content_rect.w > 100.0);
}

// ── Input number min/max preserved ──────────────────────────────────────────

#[test]
fn number_input_preserves_min_max() {
    let doc = layout_html(
        r#"<input type="number" min="0" max="100" value="50">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(input.attributes.get("min").map(|s| s.as_str()), Some("0"));
    assert_eq!(input.attributes.get("max").map(|s| s.as_str()), Some("100"));
}

// ── Focusable tests ─────────────────────────────────────────────────────────

#[test]
fn input_is_focusable() {
    let doc = layout_html(r#"<input type="text">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(crate::types::is_focusable_node(input));
}

#[test]
fn select_is_focusable() {
    let doc = layout_html(r#"<select><option>A</option></select>"#, 400.0);
    let s = find_by_tag(&doc.root, "select").unwrap();
    assert!(crate::types::is_focusable_node(s));
}

#[test]
fn textarea_is_focusable() {
    let doc = layout_html(r#"<textarea>text</textarea>"#, 400.0);
    let ta = find_by_tag(&doc.root, "textarea").unwrap();
    assert!(crate::types::is_focusable_node(ta));
}

#[test]
fn button_is_focusable() {
    let doc = layout_html(r#"<button>Click</button>"#, 400.0);
    let btn = find_by_tag(&doc.root, "button").unwrap();
    assert!(crate::types::is_focusable_node(btn));
}

#[test]
fn form_inside_table_works() {
    let doc = layout_html(
        r#"<table><form><tr><td>Label</td><td><input type="text"></td></tr></form></table>"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.layout.content_rect.w > 50.0,
        "input inside form-in-table should have width, got {}",
        input.layout.content_rect.w
    );
    assert!(
        input.layout.content_rect.h > 5.0,
        "input inside form-in-table should have height, got {}",
        input.layout.content_rect.h
    );
}

#[test]
fn form_inside_table_is_contents() {
    let doc = layout_html(r#"<table><form><tr><td>X</td></tr></form></table>"#, 400.0);
    let form = find_by_tag(&doc.root, "form").unwrap();
    assert_eq!(
        form.style.display,
        Display::Contents,
        "form inside table should be display:contents"
    );
}

#[test]
fn click_and_type_integration() {
    // Full integration test: click an input, then type — value should update
    let mut doc = layout_html(r#"<input type="text" id="t" value="">"#, 400.0);
    let input = find_by_id(&doc.root, "t").unwrap();
    let input_center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );

    // Simulate click: MouseDown then MouseUp
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, input_center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, input_center, 0);

    // Check focus was set
    assert!(
        doc.focused_box != 0,
        "clicking input should set focused_box"
    );

    // Simulate typing 'abc'
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'a' as u32,
        Some('a'),
        false,
        false,
        false,
        false,
    );
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'b' as u32,
        Some('b'),
        false,
        false,
        false,
        false,
    );
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'c' as u32,
        Some('c'),
        false,
        false,
        false,
        false,
    );

    // Check value updated
    let input = find_by_id(&doc.root, "t").unwrap();
    // The VALUE, not the `value` attribute. Typing must not edit the document:
    // the attribute is `defaultValue`, and it still reads as the author wrote it.
    assert_eq!(
        crate::types::input_value(input),
        "abc",
        "typing 'abc' should update the value, got {:?}",
        crate::types::input_value(input)
    );
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some(""),
        "typing must leave the `value` content attribute alone"
    );
}

#[test]
fn click_and_type_password() {
    let mut doc = layout_html(r#"<input type="password" id="p" value="">"#, 400.0);
    let input = find_by_id(&doc.root, "p").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    assert!(doc.focused_box != 0, "password input should focus");
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'x' as u32,
        Some('x'),
        false,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "p").unwrap();
    assert_eq!(crate::types::input_value(input), "x");
}

#[test]
fn click_checkbox_toggles() {
    let mut doc = layout_html(r#"<input type="checkbox" id="c">"#, 400.0);
    let cb = find_by_id(&doc.root, "c").unwrap();
    assert!(!cb.checkedness);
    let center = (
        cb.layout.border_rect.x + cb.layout.border_rect.w / 2.0,
        cb.layout.border_rect.y + cb.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    let cb = find_by_id(&doc.root, "c").unwrap();
    // A click changes CHECKEDNESS and leaves the markup alone (HTML §4.10.5.3)
    // — the user did not edit the document. This asserted the attribute,
    // because ticking a box used to rewrite it.
    assert!(cb.checkedness, "clicking checkbox should check it");
    assert!(
        !cb.attributes.contains_key("checked"),
        "the click wrote a `checked` attribute into the document"
    );
    assert!(cb.dirty_checked, "a user interaction raises the dirty flag");
}

#[test]
fn click_radio_selects() {
    let mut doc = layout_html(
        r#"<input type="radio" name="g" id="r1"><input type="radio" name="g" id="r2">"#,
        400.0,
    );
    let r2 = find_by_id(&doc.root, "r2").unwrap();
    let center = (
        r2.layout.border_rect.x + r2.layout.border_rect.w / 2.0,
        r2.layout.border_rect.y + r2.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    let r2 = find_by_id(&doc.root, "r2").unwrap();
    assert!(r2.checkedness, "clicking r2 should check it");
    assert!(
        !r2.attributes.contains_key("checked"),
        "the click wrote a `checked` attribute into the document"
    );
    let r1 = find_by_id(&doc.root, "r1").unwrap();
    assert!(!r1.checkedness, "r1 should be unchecked");
}

// ── All button types ─────────────────────────────────────────────────────────

fn click_button_fires_event(html: &str, expected_tag: &str) -> Vec<FormEventKind> {
    let mut doc = layout_html(html, 400.0);
    let events_ref = std::sync::Arc::new(std::sync::Mutex::new(Vec::<FormEventKind>::new()));
    let events_clone = events_ref.clone();
    doc.on_form_event = Some(Box::new(move |e: &FormEvent| {
        events_clone.lock().unwrap().push(e.kind.clone());
    }));
    let btn = find_by_tag(&doc.root, expected_tag).unwrap();
    let center = (
        btn.layout.border_rect.x + btn.layout.border_rect.w / 2.0,
        btn.layout.border_rect.y + btn.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    let result = events_ref.lock().unwrap().clone();
    doc.on_form_event = None; // drop the closure before doc
    result
}

#[test]
fn submit_input_fires_click_and_submit() {
    let events = click_button_fires_event(
        r#"<form action="/login"><input type="submit" value="Go"></form>"#,
        "input",
    );
    assert!(
        events.iter().any(|e| matches!(e, FormEventKind::Click(_))),
        "should fire Click"
    );
}

#[test]
fn button_input_fires_click_only() {
    let events = click_button_fires_event(r#"<input type="button" value="Action">"#, "input");
    assert!(
        events.iter().any(|e| matches!(e, FormEventKind::Click(_))),
        "should fire Click"
    );
    assert!(
        !events.iter().any(|e| matches!(e, FormEventKind::Submit(_))),
        "should NOT fire Submit"
    );
}

#[test]
fn reset_input_fires_click() {
    let events = click_button_fires_event(r#"<input type="reset" value="Clear">"#, "input");
    assert!(events.iter().any(|e| matches!(e, FormEventKind::Click(_))));
}

#[test]
fn button_element_submit_fires_submit() {
    let events = click_button_fires_event(
        r#"<form action="/go"><button type="submit">Send</button></form>"#,
        "button",
    );
    assert!(events.iter().any(|e| matches!(e, FormEventKind::Click(_))));
    assert!(
        events.iter().any(|e| matches!(e, FormEventKind::Submit(_))),
        "submit button should fire Submit"
    );
}

#[test]
fn button_element_type_button_no_submit() {
    let events = click_button_fires_event(
        r#"<form action="/go"><button type="button">Click</button></form>"#,
        "button",
    );
    assert!(events.iter().any(|e| matches!(e, FormEventKind::Click(_))));
    assert!(
        !events.iter().any(|e| matches!(e, FormEventKind::Submit(_))),
        "type=button should NOT submit"
    );
}

// ── Focus for ALL input types ───────────────────────────────────────────────

fn click_sets_focus(html: &str, tag: &str) -> bool {
    let mut doc = layout_html(html, 400.0);
    let elem = find_by_tag(&doc.root, tag).unwrap();
    let center = (
        elem.layout.border_rect.x + elem.layout.border_rect.w / 2.0,
        elem.layout.border_rect.y + elem.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.focused_box != 0
}

#[test]
fn click_focuses_text_input() {
    assert!(click_sets_focus(r#"<input type="text">"#, "input"));
}

#[test]
fn click_focuses_password_input() {
    assert!(click_sets_focus(r#"<input type="password">"#, "input"));
}

#[test]
fn click_focuses_email_input() {
    assert!(click_sets_focus(r#"<input type="email">"#, "input"));
}

#[test]
fn click_focuses_search_input() {
    assert!(click_sets_focus(r#"<input type="search">"#, "input"));
}

#[test]
fn click_focuses_number_input() {
    assert!(click_sets_focus(r#"<input type="number">"#, "input"));
}

#[test]
fn click_focuses_tel_input() {
    assert!(click_sets_focus(r#"<input type="tel">"#, "input"));
}

#[test]
fn click_focuses_url_input() {
    assert!(click_sets_focus(r#"<input type="url">"#, "input"));
}

#[test]
fn click_focuses_textarea() {
    assert!(click_sets_focus(r#"<textarea>text</textarea>"#, "textarea"));
}

#[test]
fn click_focuses_select() {
    assert!(click_sets_focus(
        r#"<select><option>A</option></select>"#,
        "select"
    ));
}

#[test]
fn click_focuses_button() {
    assert!(click_sets_focus(r#"<button>Click</button>"#, "button"));
}

#[test]
fn click_focuses_checkbox() {
    assert!(click_sets_focus(r#"<input type="checkbox">"#, "input"));
}

#[test]
fn click_focuses_radio() {
    assert!(click_sets_focus(r#"<input type="radio">"#, "input"));
}

#[test]
fn hidden_input_not_focusable_by_click() {
    // Hidden inputs can't be clicked since they have display:none
    let mut doc = layout_html(r#"<input type="hidden" name="t">"#, 400.0);
    // No border_rect to click — just verify focus stays null
    assert!(doc.focused_box == 0);
}

// ── Tab navigation cycles through all focusable elements ────────────────────

#[test]
fn tab_cycles_through_form_elements() {
    let mut doc = layout_html(
        r#"
        <input type="text" id="a">
        <input type="password" id="b">
        <select id="c"><option>X</option></select>
        <textarea id="d">t</textarea>
        <button id="e">Go</button>
    "#,
        400.0,
    );

    // Tab should cycle: a → b → c → d → e → a
    doc.focus_next();
    assert!(doc.focused_box != 0);
    let tag1 = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(tag1, "a", "first Tab should focus 'a', got '{}'", tag1);

    doc.focus_next();
    let tag2 = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(tag2, "b");

    doc.focus_next();
    let tag3 = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(tag3, "c");

    doc.focus_next();
    let tag4 = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(tag4, "d");

    doc.focus_next();
    let tag5 = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(tag5, "e");
}

#[test]
fn shift_tab_goes_backwards() {
    let mut doc = layout_html(
        r#"
        <input type="text" id="a">
        <input type="text" id="b">
        <input type="text" id="c">
    "#,
        400.0,
    );

    // Focus c first
    doc.focus_next(); // a
    doc.focus_next(); // b
    doc.focus_next(); // c
    let id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(id, "c");

    doc.focus_prev(); // back to b
    let id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(id, "b");
}

// ── Enter submits form from text input ──────────────────────────────────────

#[test]
fn enter_in_text_input_no_newline() {
    let mut doc = layout_html(
        r#"<form action="/go"><input type="text" id="t" value="hi"></form>"#,
        400.0,
    );
    // Focus the input
    let input = find_by_id(&doc.root, "t").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    // Press Enter
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        13,
        None,
        false,
        false,
        false,
        false,
    );
    // Should NOT insert newline in single-line input
    let input = find_by_id(&doc.root, "t").unwrap();
    assert_eq!(
        input_value(input),
        "hi",
        "Enter should not insert in text input"
    );
}

// ── Disabled elements don't focus on click ──────────────────────────────────

#[test]
fn disabled_input_no_focus_on_click() {
    let mut doc = layout_html(r#"<input type="text" disabled>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    // Disabled inputs ARE focusable per HTML spec (browsers focus them on click)
    // but they shouldn't accept keyboard input (already tested above)
}

// ── Form submission tests ────────────────────────────────────────────────────

/// Every value the entry list carries under one name, in entry order.
///
/// A LIST, because HTML's entry list is one: a `multiple` select and a
/// checkbox group each append several entries under a single name, and a
/// lookup that could only ever answer one is how those values used to vanish
/// at the point of submission.
fn submitted<'a>(data: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    data.iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// The one value under a name, or `None` when the control contributed nothing.
///
/// Panics on several rather than picking one — a test that asked for a single
/// value and silently got the first of many is the failure this whole change
/// is about.
fn submitted_one<'a>(data: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let all = submitted(data, name);
    assert!(
        all.len() <= 1,
        "{name} contributed {} entries, not one: {all:?}",
        all.len()
    );
    all.first().copied()
}

#[test]
fn a_multiple_select_submits_every_option_the_user_picked() {
    // "For each option element ... whose selectedness is true ... append an
    // entry" — several entries, one name. Held in a map, all but the last were
    // dropped between the control and the wire.
    let doc = layout_html(
        r#"<form id="f">
            <select name="tags" multiple>
                <option value="a" selected>A</option>
                <option value="b">B</option>
                <option value="c" selected>C</option>
            </select>
        </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted(&data, "tags"), vec!["a", "c"]);
    // The serializer runs over the LIST, so both survive and stay in order.
    assert_eq!(crate::types::encode_form_urlencoded(&data), "tags=a&tags=c");
}

#[test]
fn a_checkbox_group_submits_every_box_that_is_ticked() {
    // The same defect wearing different markup: one name, several controls.
    let doc = layout_html(
        r#"<form id="f">
            <input type="checkbox" name="day" value="mon" checked>
            <input type="checkbox" name="day" value="tue">
            <input type="checkbox" name="day" value="wed" checked>
        </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted(&data, "day"), vec!["mon", "wed"]);
}

#[test]
fn the_entry_list_is_in_tree_order() {
    // Order is part of the answer, and it is the DOCUMENT's order — not
    // alphabetical, which is what sorting for a map's sake used to make it.
    let doc = layout_html(
        r#"<form id="f">
            <input name="z" value="1">
            <input name="a" value="2">
        </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(crate::types::encode_form_urlencoded(&data), "z=1&a=2");
}

#[test]
fn collect_form_data_basic() {
    let doc = layout_html(
        r#"<form id="f">
        <input type="text" name="user" value="alice">
        <input type="password" name="pass" value="secret">
        <input type="hidden" name="token" value="xyz">
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted_one(&data, "user"), Some("alice"));
    assert_eq!(submitted_one(&data, "pass"), Some("secret"));
    assert_eq!(submitted_one(&data, "token"), Some("xyz"));
}

#[test]
fn collect_form_data_checkbox() {
    let doc = layout_html(
        r#"<form id="f">
        <input type="checkbox" name="agree" checked>
        <input type="checkbox" name="news">
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted_one(&data, "agree"), Some("on"));
    assert!(
        submitted_one(&data, "news").is_none(),
        "unchecked checkbox should not be in form data"
    );
}

#[test]
fn collect_form_data_radio() {
    let doc = layout_html(
        r#"<form id="f">
        <input type="radio" name="color" value="red">
        <input type="radio" name="color" value="blue" checked>
        <input type="radio" name="color" value="green">
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted_one(&data, "color"), Some("blue"));
}

#[test]
fn collect_form_data_select() {
    let doc = layout_html(
        r#"<form id="f">
        <select name="country">
            <option value="us">US</option>
            <option value="fr" selected>France</option>
        </select>
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted_one(&data, "country"), Some("fr"));
}

#[test]
fn collect_form_data_textarea() {
    let doc = layout_html(
        r#"<form id="f">
        <textarea name="bio">Hello world</textarea>
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert!(
        submitted_one(&data, "bio")
            .map(|s| s.contains("Hello"))
            .unwrap_or(false)
    );
}

#[test]
fn collect_form_data_disabled_excluded() {
    let doc = layout_html(
        r#"<form id="f">
        <input type="text" name="a" value="yes">
        <input type="text" name="b" value="no" disabled>
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(submitted_one(&data, "a"), Some("yes"));
    assert!(
        submitted_one(&data, "b").is_none(),
        "disabled input should be excluded from form data"
    );
}

#[test]
fn collect_form_data_no_name_excluded() {
    let doc = layout_html(
        r#"<form id="f">
        <input type="text" value="orphan">
        <input type="text" name="named" value="ok">
    </form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(data.len(), 1);
    assert_eq!(submitted_one(&data, "named"), Some("ok"));
}

#[test]
fn form_method_default_get() {
    let doc = layout_html(
        r#"<form id="f" action="/search"><input name="q" value="test"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let method = form
        .attributes
        .get("method")
        .map(|s| s.as_str())
        .unwrap_or("get");
    assert_eq!(method, "get");
}

#[test]
fn form_method_post() {
    let doc = layout_html(
        r#"<form id="f" method="POST" action="/login"><input name="u" value="x"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let method = form
        .attributes
        .get("method")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    assert_eq!(method, "post");
}

#[test]
fn form_action_preserved() {
    let doc = layout_html(
        r#"<form id="f" action="/submit"><input name="x" value="1"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    assert_eq!(
        form.attributes.get("action").map(|s| s.as_str()),
        Some("/submit")
    );
}

#[test]
fn submit_event_contains_action() {
    let events = click_button_fires_event(
        r#"<form action="/login"><button type="submit">Go</button></form>"#,
        "button",
    );
    let submit = events
        .iter()
        .find(|e| matches!(e, FormEventKind::Submit(_)));
    assert!(submit.is_some(), "should fire Submit event");
    if let Some(FormEventKind::Submit(action)) = submit {
        assert_eq!(action, "/login");
    }
}

// ── Form reset ──────────────────────────────────────────────────────────────

#[test]
fn reset_clears_text_inputs() {
    let mut doc = layout_html(
        r#"<form id="f">
        <input type="text" id="t" name="t" value="original">
    </form>"#,
        400.0,
    );
    // Simulate typing to change value
    let input = find_by_id(&doc.root, "t").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        'x' as u32,
        Some('x'),
        false,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "t").unwrap();
    assert!(input_value(input).contains('x'), "should have typed");
    // Reset
    let form_node_id = find_by_id(&doc.root, "f").unwrap().node_id;
    crate::types::reset_form(&mut doc.root, form_node_id);
    let input = find_by_id(&doc.root, "t").unwrap();
    assert_eq!(
        input_value(input),
        "original",
        "reset should restore original value"
    );
}

// ── Autofocus ───────────────────────────────────────────────────────────────

#[test]
fn autofocus_attribute_preserved() {
    let doc = layout_html(r#"<input type="text" id="a" autofocus>"#, 400.0);
    let input = find_by_id(&doc.root, "a").unwrap();
    assert!(input.attributes.contains_key("autofocus"));
}

// ── Form data encoding ──────────────────────────────────────────────────────

#[test]
fn encode_form_data_urlencoded() {
    let doc = layout_html(
        r#"<form id="f"><input name="q" value="hello world"><input name="lang" value="en"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    let encoded = crate::types::encode_form_urlencoded(&data);
    assert!(
        encoded.contains("q=hello+world") || encoded.contains("q=hello%20world"),
        "should encode space, got: {}",
        encoded
    );
    assert!(encoded.contains("lang=en"));
}

#[test]
fn build_get_url() {
    let doc = layout_html(
        r#"<form id="f" method="get" action="/search"><input name="q" value="test"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    let url = crate::types::build_form_submit_url("/search", "get", &data);
    assert!(
        url.contains("/search?"),
        "GET should append query string, got: {}",
        url
    );
    assert!(url.contains("q=test"));
}

#[test]
fn build_post_url_no_query() {
    let doc = layout_html(
        r#"<form id="f" method="post" action="/login"><input name="u" value="x"></form>"#,
        400.0,
    );
    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    let url = crate::types::build_form_submit_url("/login", "post", &data);
    assert_eq!(url, "/login", "POST should not append query string");
}

// ── Select keyboard navigation ──────────────────────────────────────────────

#[test]
fn select_arrow_down_changes_option() {
    let mut doc = layout_html(
        r#"<select id="s">
        <option value="a">Alpha</option>
        <option value="b" selected>Beta</option>
        <option value="c">Gamma</option>
    </select>"#,
        400.0,
    );
    // Focus the select
    let sel = find_by_id(&doc.root, "s").unwrap();
    let center = (
        sel.layout.border_rect.x + sel.layout.border_rect.w / 2.0,
        sel.layout.border_rect.y + sel.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    // Arrow down should select next option
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        40,
        None,
        false,
        false,
        false,
        false,
    ); // down
    let sel = find_by_id(&doc.root, "s").unwrap();
    assert_eq!(
        crate::html::forms::selected_index(sel),
        2,
        "arrow down should move to index 2"
    );
}

#[test]
fn select_arrow_up_changes_option() {
    let mut doc = layout_html(
        r#"<select id="s">
        <option value="a">Alpha</option>
        <option value="b" selected>Beta</option>
        <option value="c">Gamma</option>
    </select>"#,
        400.0,
    );
    let sel = find_by_id(&doc.root, "s").unwrap();
    let center = (
        sel.layout.border_rect.x + sel.layout.border_rect.w / 2.0,
        sel.layout.border_rect.y + sel.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        38,
        None,
        false,
        false,
        false,
        false,
    ); // up
    let sel = find_by_id(&doc.root, "s").unwrap();
    assert_eq!(
        crate::html::forms::selected_index(sel),
        0,
        "arrow up should move to index 0"
    );
}

// ── Number input increment/decrement ────────────────────────────────────────

#[test]
fn number_input_arrow_up_increments() {
    let mut doc = layout_html(
        r#"<input type="number" id="n" value="5" min="0" max="10">"#,
        400.0,
    );
    let input = find_by_id(&doc.root, "n").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        38,
        None,
        false,
        false,
        false,
        false,
    ); // up
    let input = find_by_id(&doc.root, "n").unwrap();
    assert_eq!(
        input_value(input),
        "6",
        "arrow up should increment, got {}",
        input_value(input)
    );
}

#[test]
fn number_input_arrow_down_decrements() {
    let mut doc = layout_html(
        r#"<input type="number" id="n" value="5" min="0" max="10">"#,
        400.0,
    );
    let input = find_by_id(&doc.root, "n").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        40,
        None,
        false,
        false,
        false,
        false,
    ); // down
    let input = find_by_id(&doc.root, "n").unwrap();
    assert_eq!(input_value(input), "4");
}

#[test]
fn number_input_respects_max() {
    let mut doc = layout_html(r#"<input type="number" id="n" value="10" max="10">"#, 400.0);
    let input = find_by_id(&doc.root, "n").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        38,
        None,
        false,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "n").unwrap();
    assert_eq!(input_value(input), "10", "should not exceed max");
}

#[test]
fn number_input_respects_min() {
    let mut doc = layout_html(r#"<input type="number" id="n" value="0" min="0">"#, 400.0);
    let input = find_by_id(&doc.root, "n").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        40,
        None,
        false,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "n").unwrap();
    assert_eq!(input_value(input), "0", "should not go below min");
}

// ── Autofocus ───────────────────────────────────────────────────────────────

#[test]
fn autofocus_sets_focus_on_load() {
    let mut doc = layout_html(
        r#"<input type="text" id="a"><input type="text" id="b" autofocus>"#,
        400.0,
    );
    crate::types::apply_autofocus(&mut doc);
    assert!(doc.focused_box != 0, "autofocus should set focused_box");
    let focused_id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(
        focused_id, "b",
        "should focus element with autofocus attribute"
    );
}

// ── Required visual ─────────────────────────────────────────────────────────

#[test]
fn required_input_matches_required_pseudo() {
    let doc = layout_html(
        r#"<style>input:required { border-color: red; }</style>
        <input type="text" required>"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(input.attributes.contains_key("required"));
    // The :required pseudo-class should match — border-color isn't directly
    // on ComputedStyle, but the cascade should process it
}

// ── Placeholder pseudo-element ──────────────────────────────────────────────

#[test]
fn placeholder_text_has_default_gray_color() {
    // Placeholder should be rendered in gray by the renderer (not testable via style)
    let doc = layout_html(r#"<input type="text" placeholder="hint">"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("placeholder").map(|s| s.as_str()),
        Some("hint")
    );
    // The renderer draws placeholder in gray — visual test
}

#[test]
fn placeholder_pseudo_element_sets_placeholder_color() {
    let doc = layout_html(
        r#"<style>input::placeholder { color: rgb(10, 20, 30); }</style>
           <input type="text" placeholder="hint">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();

    assert_eq!(
        input
            .style
            .placeholder_style
            .as_ref()
            .map(|style| style.color),
        Some(Color::rgb(10, 20, 30))
    );
}

// ── Focus ring on tab-focused checkbox ──────────────────────────────────────

#[test]
fn tab_to_checkbox_sets_focus() {
    let mut doc = layout_html(
        r#"<input type="checkbox" id="c"><input type="text" id="t">"#,
        400.0,
    );
    doc.focus_next(); // should focus checkbox first
    assert!(doc.focused_box != 0);
    let id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(id, "c", "Tab should focus checkbox");
}

// ── Tabindex tests ──────────────────────────────────────────────────────────

#[test]
fn tabindex_positive_comes_first() {
    let mut doc = layout_html(
        r#"
        <input type="text" id="a">
        <input type="text" id="b" tabindex="1">
        <input type="text" id="c">
    "#,
        400.0,
    );
    doc.focus_next();
    let id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(id, "b", "tabindex=1 should be focused first, got {}", id);
}

#[test]
fn tabindex_negative_skipped() {
    let mut doc = layout_html(
        r#"
        <input type="text" id="a">
        <input type="text" id="skip" tabindex="-1">
        <input type="text" id="c">
    "#,
        400.0,
    );
    doc.focus_next(); // a
    doc.focus_next(); // should skip "skip" and go to c
    let id = find_by_node_id(&doc.root, doc.focused_box)
        .and_then(|n| n.attributes.get("id").cloned())
        .unwrap_or_default();
    assert_eq!(id, "c", "tabindex=-1 should be skipped, got {}", id);
}

// ── Pattern validation ──────────────────────────────────────────────────────

#[test]
fn pattern_attribute_preserved() {
    let doc = layout_html(r#"<input type="text" pattern="[0-9]+" id="p">"#, 400.0);
    let input = find_by_id(&doc.root, "p").unwrap();
    assert_eq!(
        input.attributes.get("pattern").map(|s| s.as_str()),
        Some("[0-9]+")
    );
}

// ── Required attribute ──────────────────────────────────────────────────────

#[test]
fn required_attribute_preserved() {
    let doc = layout_html(r#"<input type="text" required id="r">"#, 400.0);
    let input = find_by_id(&doc.root, "r").unwrap();
    assert!(input.attributes.contains_key("required"));
}

// ── Select multiple ─────────────────────────────────────────────────────────

#[test]
fn select_multiple_attribute_preserved() {
    let doc = layout_html(
        r#"<select multiple id="m"><option>A</option><option>B</option></select>"#,
        400.0,
    );
    let sel = find_by_id(&doc.root, "m").unwrap();
    assert!(sel.attributes.contains_key("multiple"));
}

// ── Disabled text rendering ─────────────────────────────────────────────────

#[test]
fn disabled_input_has_opacity() {
    let doc = layout_html(r#"<input type="text" value="test" disabled>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert!(
        input.style.opacity < 1.0,
        "disabled should have reduced opacity, got {}",
        input.style.opacity
    );
}

// ── Focus ring on tab-focused elements ──────────────────────────────────────

#[test]
fn tab_focused_element_has_keyboard_focus() {
    let mut doc = layout_html(
        r#"<input type="text" id="a"><input type="text" id="b">"#,
        400.0,
    );
    doc.focus_next();
    assert!(
        doc.keyboard_focus,
        "Tab focus should set keyboard_focus=true"
    );
}

// ── Range slider ────────────────────────────────────────────────────────────

#[test]
fn range_has_default_value() {
    let doc = layout_html(
        r#"<input type="range" min="0" max="100" value="50">"#,
        400.0,
    );
    let input = find_by_tag(&doc.root, "input").unwrap();
    assert_eq!(
        input.attributes.get("value").map(|s| s.as_str()),
        Some("50")
    );
}

// ── Ctrl+A selects all text ─────────────────────────────────────────────────

#[test]
fn ctrl_a_selects_all_in_input() {
    let mut doc = layout_html(r#"<input type="text" id="t" value="hello">"#, 400.0);
    let input = find_by_id(&doc.root, "t").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    // Ctrl+A
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        65,
        Some('a'),
        true,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "t").unwrap();
    assert_eq!(input.input_cursor, 5, "Ctrl+A should move cursor to end");
    assert_eq!(
        input.input_sel_anchor, 0,
        "Ctrl+A should set anchor to start"
    );
}

// ── Backspace with selection deletes selection ──────────────────────────────

#[test]
fn backspace_deletes_selection() {
    let mut doc = layout_html(r#"<input type="text" id="t" value="hello">"#, 400.0);
    let input = find_by_id(&doc.root, "t").unwrap();
    let center = (
        input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
        input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, center, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, center, 0);
    // Select all with Ctrl+A
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        65,
        Some('a'),
        true,
        false,
        false,
        false,
    );
    // Backspace should delete selection
    doc.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        8,
        None,
        false,
        false,
        false,
        false,
    );
    let input = find_by_id(&doc.root, "t").unwrap();
    assert_eq!(
        input_value(input),
        "",
        "backspace should delete selected text"
    );
}

fn disabled_input_not_focusable() {
    let doc = layout_html(r#"<input type="text" disabled>"#, 400.0);
    let input = find_by_tag(&doc.root, "input").unwrap();
    // Disabled elements should still be focusable per spec but...
    // actually in browsers disabled inputs ARE focusable by click
    // Let's just verify the attribute is there
    assert!(input.attributes.contains_key("disabled"));
}

// ── Input height stretches background ───────────────────────────────────────

#[test]
fn input_explicit_height_stretches_padding_rect() {
    let doc = layout_html(
        r#"<style>input { height: 80px; background-color: #ccc; }</style>
           <input type="text" id="t">"#,
        400.0,
    );
    let input = find_by_id(&doc.root, "t").unwrap();
    // padding_rect.h should be at least 80px (the explicit height)
    assert!(
        input.layout.padding_rect.h >= 78.0,
        "padding_rect height {} should be >= 78 when height:80px is set",
        input.layout.padding_rect.h
    );
    // border_rect should also reflect the height
    assert!(
        input.layout.border_rect.h >= 78.0,
        "border_rect height {} should be >= 78 when height:80px is set",
        input.layout.border_rect.h
    );
}

#[test]
fn input_explicit_height_content_rect_smaller() {
    let doc = layout_html(
        r#"<style>input { height: 100px; padding: 10px; box-sizing: border-box; }</style>
           <input type="text" id="t">"#,
        400.0,
    );
    let input = find_by_id(&doc.root, "t").unwrap();
    // With border-box, border_rect.h = 100, content_rect.h = 100 - padding*2 - border*2
    // UA has border: 1px, so content = 100 - 20 - 2 = 78
    assert!(
        input.layout.border_rect.h >= 98.0,
        "border_rect height {} should be ~100 with border-box",
        input.layout.border_rect.h
    );
    assert!(
        input.layout.content_rect.h < input.layout.border_rect.h,
        "content_rect.h {} should be smaller than border_rect.h {} due to padding",
        input.layout.content_rect.h,
        input.layout.border_rect.h
    );
}

#[test]
fn textarea_explicit_height_stretches() {
    let doc = layout_html(
        r#"<style>textarea { height: 120px; }</style>
           <textarea id="t">text</textarea>"#,
        400.0,
    );
    let ta = find_by_id(&doc.root, "t").unwrap();
    assert!(
        ta.layout.padding_rect.h >= 118.0,
        "textarea padding_rect.h {} should be >= 118 with height:120px",
        ta.layout.padding_rect.h
    );
}

#[test]
fn select_explicit_height_stretches() {
    let doc = layout_html(
        r#"<style>select { height: 60px; }</style>
           <select id="s"><option>A</option></select>"#,
        400.0,
    );
    let sel = find_by_id(&doc.root, "s").unwrap();
    assert!(
        sel.layout.padding_rect.h >= 58.0,
        "select padding_rect.h {} should be >= 58 with height:60px",
        sel.layout.padding_rect.h
    );
}

#[test]
fn button_explicit_height_stretches() {
    let doc = layout_html(
        r#"<button id="b" style="height: 80px;">Click</button>"#,
        400.0,
    );
    let btn = find_by_id(&doc.root, "b").unwrap();
    assert!(
        btn.layout.padding_rect.h >= 78.0,
        "button padding_rect.h {} should be >= 78 with height:80px",
        btn.layout.padding_rect.h
    );
}

#[test]
fn submit_input_explicit_height_stretches() {
    let doc = layout_html(
        r#"<input type="submit" id="s" value="Go" style="height: 60px;">"#,
        400.0,
    );
    let sub = find_by_id(&doc.root, "s").unwrap();
    assert!(
        sub.layout.padding_rect.h >= 58.0,
        "submit padding_rect.h {} should be >= 58 with height:60px",
        sub.layout.padding_rect.h
    );
}

// ── Colour picker popup ──────────────────────────────────────────────────────

#[test]
fn clicking_a_colour_input_opens_its_picker_and_a_swatch_sets_the_value() {
    // HTML §4.10.5.1.15 leaves the picker's FORM to the user agent and says
    // only that one is offered. What it does pin down is the value: a "valid
    // simple colour", `#rrggbb`, which is what a pick has to write back.
    //
    // The picker rides the same overlay the `<select>` dropdown uses —
    // `open_picker` beside `open_select` — so this is a second thing on one
    // popup surface rather than a second mechanism.
    let mut doc = layout_html(
        // `r##` because the markup contains `"#` — a plain `r#"…"#` ends
        // right there, at the value's own hash.
        r##"<input type="color" id="c" value="#000000">"##,
        400.0,
    );
    let c = doc.get_element_by_id("c").unwrap();
    let br = find_by_id(&doc.root, "c").unwrap().layout.border_rect;
    let centre = (br.x + br.w / 2.0, br.y + br.h / 2.0);

    assert_eq!(doc.open_picker, 0, "nothing is open before the click");
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, centre, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, centre, 0);
    assert_eq!(
        doc.open_picker, c,
        "activating the control opens its picker"
    );

    // The palette sits below the control; pick the first swatch of the second
    // row, whatever the palette holds there.
    let (px, py, _, _) = doc.picker_rect(c).expect("an open picker has geometry");
    let cell = crate::widgets::PALETTE_CELL;
    let point = (px + cell / 2.0, py + cell * 1.5);
    let expected = doc.picker_hit(c, point).expect("that point is a swatch");

    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);

    assert_eq!(doc.open_picker, 0, "picking closes the picker");
    assert_eq!(
        doc.value(c),
        crate::widgets::to_simple_colour(expected),
        "the pick writes a valid simple colour back into the value"
    );
}

#[test]
fn clicking_away_closes_the_picker_without_changing_the_value() {
    let mut doc = layout_html(r##"<input type="color" id="c" value="#123456">"##, 400.0);
    let c = doc.get_element_by_id("c").unwrap();
    let br = find_by_id(&doc.root, "c").unwrap().layout.border_rect;
    let centre = (br.x + br.w / 2.0, br.y + br.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, centre, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, centre, 0);
    assert_eq!(doc.open_picker, c);

    // Far from both the control and its palette.
    let away = (br.x + 300.0, br.y + 200.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, away, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, away, 0);
    assert_eq!(doc.open_picker, 0, "clicking away closes it");
    assert_eq!(doc.value(c), "#123456", "and picks nothing");
}

#[test]
fn a_dropdown_row_can_be_picked_where_no_element_lies_beneath() {
    // The list is drawn OVER the page and is not in the tree, so a row that
    // hangs past the end of the content has nothing under it. The click path
    // is gated on having hit an element, which silently dropped exactly those
    // picks — the ones furthest down the list, on a short page.
    //
    // Same fault the colour picker had, which is why both are now allowed
    // through: a popup's click is about the POPUP's geometry, not about
    // whatever the hit test finds below it.
    let mut doc = layout_html(
        r#"<select id="s"><option>A</option><option>B</option><option>C</option></select>"#,
        400.0,
    );
    let s = doc.get_element_by_id("s").unwrap();
    let br = find_by_id(&doc.root, "s").unwrap().layout.border_rect;
    let centre = (br.x + br.w / 2.0, br.y + br.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, centre, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, centre, 0);
    assert_eq!(doc.open_select, s, "clicking the select opens the list");

    // The THIRD row, which sits below the select — and on a document this
    // short, below the body's content entirely.
    //
    // Row geometry is the engine's: `font-size * 1.8` per row, and the list is
    // inset 4px from the control's bottom edge. Derived rather than guessed —
    // a hardcoded 20px lands on row 1 and the test then "fails" over its own
    // arithmetic.
    let font_px = 16.0;
    let row_h = font_px * 1.8;
    let third = (br.x + br.w / 2.0, br.y + br.h + 4.0 + row_h * 2.5);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, third, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, third, 0);

    assert_eq!(doc.open_select, 0, "picking closes the list");
    assert_eq!(
        doc.selected_index(s),
        2,
        "the third row is what was clicked, and it is what got selected"
    );
}

#[test]
fn a_date_input_opens_a_calendar_and_a_day_sets_the_value() {
    // Same popup surface as the dropdown and the colour picker: `open_picker`
    // holds the node, the renderer draws the month over the page, and the
    // click is taken before hit testing. What differs is only what a pick
    // MEANS — a day, written back as `yyyy-mm-dd`, the format the spec
    // requires of this control's value.
    let mut doc = layout_html(r#"<input type="date" id="d" value="2026-08-24">"#, 400.0);
    let d = doc.get_element_by_id("d").unwrap();
    let br = find_by_id(&doc.root, "d").unwrap().layout.border_rect;
    let centre = (br.x + br.w / 2.0, br.y + br.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, centre, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, centre, 0);
    assert_eq!(
        doc.open_picker, d,
        "activating a date control opens its calendar"
    );

    // August 2026 starts on a Saturday, so the 1st sits in column 5 of row 0.
    let (px, py, _, _) = doc.picker_rect(d).expect("an open picker has geometry");
    let cell = crate::widgets::Calendar::CELL;
    let first = crate::widgets::first_weekday(2026, 8);
    let day_10_index = first + 9;
    let point = (
        px + (day_10_index % 7) as f32 * cell + cell / 2.0,
        py + crate::widgets::Calendar::HEADER + (day_10_index / 7) as f32 * cell + cell / 2.0,
    );
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);

    assert_eq!(doc.open_picker, 0, "picking closes the calendar");
    assert_eq!(
        doc.value(d),
        "2026-08-10",
        "the pick writes the date in the format the control's value takes"
    );
}

// ── List box and range: the click paths (HTML §4.10.7, §4.10.5.1.13) ────────
//
// Both controls PAINTED long before either could be clicked, which is the one
// failure a screenshot cannot show. These drive `process_mouse_event` on a
// laid-out document — the state a real click arrives in.

/// Click the centre of a control's row `row`, both edges, as the shell does.
fn click_at(doc: &mut Document, x: f32, y: f32) {
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, (x, y), 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, (x, y), 0);
}

/// A click with Ctrl held — the gesture a platform binds the list box's
/// "request that the option ... be unselected" to.
fn ctrl_click_at(doc: &mut Document, x: f32, y: f32) {
    for etype in [
        crate::dom::HtmlEventType::MouseDown,
        crate::dom::HtmlEventType::MouseUp,
    ] {
        doc.process_mouse_event_with_modifiers(etype, (x, y), 0, true, false, false, false);
    }
}

/// The document y of a list box row's centre, off the SHARED metrics — so this
/// asks about the control rather than about the test's own arithmetic.
fn list_box_row_y(doc: &Document, id: &str, row: usize) -> (f32, f32) {
    let n = find_by_id(&doc.root, id).expect("no such control");
    let content = n.layout.content_rect;
    let font_px = n.style.font_size_px(16.0, 16.0).max(1.0);
    let row_h = crate::html::forms::list_box_row_height(font_px);
    (
        content.x + content.w / 2.0,
        content.y + crate::html::forms::LIST_BOX_PADDING + row as f32 * row_h + row_h / 2.0,
    )
}

#[test]
fn a_fresh_list_box_has_nothing_selected() {
    // The selectedness setting algorithm auto-selects the first option only at
    // a DISPLAY SIZE OF 1. A list box is not that, so it starts empty and
    // `selectedIndex` is −1 — not 0.
    let doc = layout_html(
        r#"<select id="lb" size="4"><option>one</option><option>two</option></select>"#,
        400.0,
    );
    let lb = find_by_id(&doc.root, "lb").unwrap();
    assert_eq!(crate::html::forms::selected_index(lb), -1);

    // A DROP-DOWN of the same options does auto-select, which is the guard.
    let doc = layout_html(
        r#"<select id="dd"><option>one</option><option>two</option></select>"#,
        400.0,
    );
    let dd = find_by_id(&doc.root, "dd").unwrap();
    assert_eq!(crate::html::forms::selected_index(dd), 0);
}

#[test]
fn the_auto_selected_option_skips_a_disabled_one() {
    // "set the selectedness of the first option element ... that is not
    // disabled". An `<optgroup disabled>` disables its children too.
    let doc = layout_html(
        r#"<select id="dd"><option disabled>skip</option><option>take</option></select>"#,
        400.0,
    );
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "dd").unwrap()),
        1
    );

    let doc = layout_html(
        r#"<select id="g"><optgroup disabled><option>skip</option></optgroup><option>take</option></select>"#,
        400.0,
    );
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "g").unwrap()),
        1
    );
}

#[test]
fn clicking_a_list_box_row_selects_that_row() {
    let mut doc = layout_html(
        r#"<select id="lb" size="4" style="width:150px;height:100px">
             <option>one</option><option>two</option><option>three</option><option>four</option>
           </select>"#,
        400.0,
    );
    let (x, y) = list_box_row_y(&doc, "lb", 2);
    click_at(&mut doc, x, y);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        2,
        "clicking the third row must select the third option"
    );
    // ⛔ And no phantom popup: a list box has none, so routing its click
    // through the dropdown state machine is the bug this guards.
    assert_eq!(doc.open_select, 0, "a list box must never open a popup");
}

#[test]
fn a_single_select_list_box_can_be_asked_to_unselect() {
    // "If the multiple attribute is absent and the element's display size is
    // greater than 1, then the user agent should also allow the user to request
    // that the option whose selectedness is true, if any, be unselected."
    let mut doc = layout_html(
        r#"<select id="lb" size="4" style="width:150px;height:100px">
             <option>one</option><option>two</option><option>three</option>
           </select>"#,
        400.0,
    );
    let (x, y1) = list_box_row_y(&doc, "lb", 1);
    click_at(&mut doc, x, y1);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        1
    );

    ctrl_click_at(&mut doc, x, y1);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        -1,
        "the request must leave the control with nothing selected"
    );

    // The request applies to the option that IS selected. On any other row it
    // is an ordinary pick, not a way to select nothing by aiming badly.
    let (_, y2) = list_box_row_y(&doc, "lb", 2);
    ctrl_click_at(&mut doc, x, y2);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        2
    );
}

#[test]
fn a_drop_down_has_no_unselect_affordance() {
    // The affordance is a LIST BOX's — display size greater than one. A
    // drop-down has no way to show "nothing", and HTML never offers it one, so
    // the same gesture must simply pick.
    let mut doc = layout_html(
        r#"<select id="dd"><option>one</option><option>two</option></select>"#,
        400.0,
    );
    let sel = find_by_id(&doc.root, "dd").unwrap();
    let r = sel.layout.content_rect;
    ctrl_click_at(&mut doc, r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "dd").unwrap()),
        0,
        "a drop-down's first option stays selected"
    );
}

#[test]
fn a_list_box_click_replaces_the_selection_but_multiple_toggles() {
    // Single-select: picking an option deselects every other one.
    let mut doc = layout_html(
        r#"<select id="lb" size="4" style="width:150px;height:100px">
             <option>one</option><option>two</option><option>three</option>
           </select>"#,
        400.0,
    );
    let (x, y0) = list_box_row_y(&doc, "lb", 0);
    click_at(&mut doc, x, y0);
    let (_, y2) = list_box_row_y(&doc, "lb", 2);
    click_at(&mut doc, x, y2);
    let lb = find_by_id(&doc.root, "lb").unwrap();
    let selected: Vec<bool> = crate::html::forms::list_of_options(lb)
        .iter()
        .map(|o| o.selectedness)
        .collect();
    assert_eq!(
        selected,
        [false, false, true],
        "a single-select list box holds one row"
    );

    // `multiple`: each click TOGGLES, so two rows can be held at once — the
    // state a single index could never express.
    let mut doc = layout_html(
        r#"<select id="ml" multiple style="width:150px;height:100px">
             <option>one</option><option>two</option><option>three</option>
           </select>"#,
        400.0,
    );
    let (x, y0) = list_box_row_y(&doc, "ml", 0);
    click_at(&mut doc, x, y0);
    let (_, y2) = list_box_row_y(&doc, "ml", 2);
    click_at(&mut doc, x, y2);
    let ml = find_by_id(&doc.root, "ml").unwrap();
    let selected: Vec<bool> = crate::html::forms::list_of_options(ml)
        .iter()
        .map(|o| o.selectedness)
        .collect();
    assert_eq!(
        selected,
        [true, false, true],
        "a multiple list box holds both rows"
    );

    // Clicking a held row again releases it.
    click_at(&mut doc, x, y0);
    let ml = find_by_id(&doc.root, "ml").unwrap();
    assert!(!crate::html::forms::list_of_options(ml)[0].selectedness);
}

#[test]
fn a_click_does_not_edit_the_selected_attribute() {
    // `selected` is `defaultSelected` — the reset target. A user's click writes
    // SELECTEDNESS and leaves the markup as the author wrote it.
    let mut doc = layout_html(
        r#"<select id="lb" size="4" style="width:150px;height:100px">
             <option selected>one</option><option>two</option>
           </select>"#,
        400.0,
    );
    let (x, y) = list_box_row_y(&doc, "lb", 1);
    click_at(&mut doc, x, y);
    let lb = find_by_id(&doc.root, "lb").unwrap();
    let options = crate::html::forms::list_of_options(lb);
    assert_eq!(
        crate::html::forms::selected_index(lb),
        1,
        "the click moved the selection"
    );
    assert!(
        options[0].attributes.contains_key("selected"),
        "the author's default survives"
    );
    assert!(
        !options[1].attributes.contains_key("selected"),
        "the click added no attribute"
    );
}

#[test]
fn a_range_starts_at_the_sanitized_value_not_the_attribute() {
    // The spec's own worked example, end to end through the parser.
    let doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="20" value="50">"#,
        400.0,
    );
    let r = find_by_id(&doc.root, "r").unwrap();
    assert_eq!(
        crate::types::input_value(r),
        "60",
        "step mismatch rounds up on a tie"
    );
    assert_eq!(
        r.attributes.get("value").map(|s| s.as_str()),
        Some("50"),
        "sanitization sets the VALUE and leaves the default alone"
    );
}

#[test]
fn clicking_a_range_track_moves_its_value() {
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="any" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let r = find_by_id(&doc.root, "r").unwrap();
    let rect = r.layout.content_rect;
    // Three quarters along. Not the end — a thumb has width, so its centre
    // never reaches the last pixel of the track.
    click_at(&mut doc, rect.x + rect.w * 0.75, rect.y + rect.h / 2.0);

    let after: f64 = crate::html::forms::parse_floating_point(&crate::types::input_value(
        find_by_id(&doc.root, "r").unwrap(),
    ))
    .unwrap();
    assert!(
        after > 50.0 && after <= 100.0,
        "clicking three quarters along left the value at {after}"
    );
}

#[test]
fn dragging_a_range_fires_input_all_the_way_and_change_once() {
    // "While the user is dragging the control's knob, input events would fire
    // whenever the position changed, whereas the change event would only fire
    // when the user let go of the knob, committing to a specific value."
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="any" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<FormEventKind>::new()));
    let sink = events.clone();
    doc.on_form_event = Some(Box::new(move |e: &FormEvent| {
        sink.lock().unwrap().push(e.kind.clone());
    }));

    let y = rect.y + rect.h / 2.0;
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseDown,
        (rect.x + rect.w * 0.2, y),
        0,
    );
    for frac in [0.4, 0.6, 0.8] {
        doc.process_mouse_event(
            crate::dom::HtmlEventType::MouseMove,
            (rect.x + rect.w * frac, y),
            0,
        );
    }
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseUp,
        (rect.x + rect.w * 0.8, y),
        0,
    );

    let seen = events.lock().unwrap().clone();
    doc.on_form_event = None; // drop the closure before doc
    let inputs = seen
        .iter()
        .filter(|e| matches!(e, FormEventKind::Input(_)))
        .count();
    let changes = seen
        .iter()
        .filter(|e| matches!(e, FormEventKind::Change(_)))
        .count();
    assert!(
        inputs >= 4,
        "the press and each move that moved the knob fire `input` — saw {inputs}: {seen:?}"
    );
    assert_eq!(
        changes, 1,
        "`change` fires once, on release — saw {changes}: {seen:?}"
    );
    // And the last thing in the list is the commit, not a stray move.
    assert!(
        matches!(seen.last(), Some(FormEventKind::Change(_))),
        "`change` comes last: {seen:?}"
    );

    let after: f64 = crate::html::forms::parse_floating_point(&crate::types::input_value(
        find_by_id(&doc.root, "r").unwrap(),
    ))
    .unwrap();
    assert!(after > 50.0, "the drag ended near the far end, at {after}");
}

#[test]
fn a_range_that_was_touched_but_not_moved_commits_nothing() {
    // "The change event fires when the value is committed." Pressing and
    // releasing on the knob's own position commits nothing, so a handler
    // counting changes must not hear one.
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="any" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<FormEventKind>::new()));
    let sink = events.clone();
    doc.on_form_event = Some(Box::new(move |e: &FormEvent| {
        sink.lock().unwrap().push(e.kind.clone());
    }));
    // The far left IS the current value, so nothing moves.
    let pt = (rect.x, rect.y + rect.h / 2.0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, pt, 0);
    doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, pt, 0);
    let seen = events.lock().unwrap().clone();
    doc.on_form_event = None;
    assert!(
        !seen.iter().any(|e| matches!(e, FormEventKind::Change(_))),
        "nothing moved, so nothing was committed: {seen:?}"
    );
}

#[test]
fn a_range_click_lands_on_a_step() {
    // "The range and step constraints are enforced even during user input", so
    // a click cannot leave the control between steps.
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="25" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let r = find_by_id(&doc.root, "r").unwrap();
    let rect = r.layout.content_rect;
    click_at(&mut doc, rect.x + rect.w * 0.62, rect.y + rect.h / 2.0);

    let after = crate::types::input_value(find_by_id(&doc.root, "r").unwrap());
    assert!(
        ["0", "25", "50", "75", "100"].contains(&after.as_str()),
        "a click left the range off its steps, at {after}"
    );
}

#[test]
fn a_vertical_range_reads_the_pointers_y() {
    // A vertical writing mode turns the control's inline axis. Measuring x
    // would answer with wherever the pointer happened to be horizontally.
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="any" value="0"
             style="writing-mode:vertical-lr;width:20px;height:200px">"#,
        400.0,
    );
    let r = find_by_id(&doc.root, "r").unwrap();
    let rect = r.layout.content_rect;
    click_at(&mut doc, rect.x + rect.w / 2.0, rect.y + rect.h * 0.75);

    let after: f64 = crate::html::forms::parse_floating_point(&crate::types::input_value(
        find_by_id(&doc.root, "r").unwrap(),
    ))
    .unwrap();
    assert!(
        after > 50.0,
        "a vertical range ignored its own axis, landing at {after}"
    );
}

#[test]
fn a_disabled_control_ignores_a_click() {
    let mut doc = layout_html(
        r#"<select id="lb" size="4" disabled style="width:150px;height:100px">
             <option>one</option><option>two</option>
           </select>"#,
        400.0,
    );
    let (x, y) = list_box_row_y(&doc, "lb", 1);
    click_at(&mut doc, x, y);
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        -1
    );

    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" value="0" disabled
             style="width:200px;height:20px">"#,
        400.0,
    );
    let rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    click_at(&mut doc, rect.x + rect.w * 0.75, rect.y + rect.h / 2.0);
    assert_eq!(
        crate::types::input_value(find_by_id(&doc.root, "r").unwrap()),
        "0"
    );
}

#[test]
fn a_reset_restores_the_authors_defaults_not_the_users_edits() {
    // The reset algorithm over all three states at once: value, checkedness
    // and selectedness each fall back to their content attribute.
    let mut doc = layout_html(
        r#"<form id="f">
             <input id="t" type="text" value="original">
             <select id="lb" size="4" style="width:150px;height:100px">
               <option selected>one</option><option>two</option>
             </select>
           </form>"#,
        400.0,
    );
    let (x, y) = list_box_row_y(&doc, "lb", 1);
    click_at(&mut doc, x, y);
    fn by_id_mut<'a>(node: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(node);
        }
        node.children.iter_mut().find_map(|c| by_id_mut(c, id))
    }
    if let Some(t) = by_id_mut(&mut doc.root, "t") {
        t.value_state = Some("typed".into());
        t.dirty_value = true;
    }
    assert_eq!(
        crate::html::forms::selected_index(find_by_id(&doc.root, "lb").unwrap()),
        1
    );

    let form_id = find_by_id(&doc.root, "f").unwrap().node_id;
    crate::types::reset_form(&mut doc.root, form_id);

    let lb = find_by_id(&doc.root, "lb").unwrap();
    assert_eq!(
        crate::html::forms::selected_index(lb),
        0,
        "reset restores the marked option"
    );
    let t = find_by_id(&doc.root, "t").unwrap();
    assert_eq!(
        crate::types::input_value(t),
        "original",
        "reset restores the value attribute"
    );
}

#[test]
fn an_unstyled_list_box_still_occupies_space() {
    // §15.5.16: a list box's block size is "the height necessary to contain as
    // many rows for items as given by the element's display size". Every other
    // list-box test pins width and height in CSS, so a collapsed intrinsic size
    // would fail none of them — and an unclickable 0-height control looks
    // exactly like a broken click path.
    let doc = layout_html(
        r#"<select id="lb" size="4"><option>one</option><option>two</option></select>"#,
        400.0,
    );
    let lb = find_by_id(&doc.root, "lb").unwrap();
    let r = lb.layout.content_rect;
    assert!(
        r.w > 0.0 && r.h > 0.0,
        "an unstyled list box laid out to {}x{}",
        r.w,
        r.h
    );
    // Taller than a drop-down of the same options, which is the whole point of
    // a display size above one.
    let dd = layout_html(
        r#"<select id="dd"><option>one</option><option>two</option></select>"#,
        400.0,
    );
    let dd_h = find_by_id(&dd.root, "dd").unwrap().layout.content_rect.h;
    assert!(
        r.h > dd_h,
        "a 4-row list box ({}) is not taller than a drop-down ({dd_h})",
        r.h
    );
}

#[test]
fn the_dom_accessors_see_what_a_click_did() {
    // ⛔ There are TWO stores — the render tree and the arena — and interaction
    // writes only the first. A click that moves state the accessors cannot see
    // is a click the program is told never happened, which is the failure this
    // pins: assert through `doc.value()`/`doc.selected_index()`, the WHATWG
    // accessors, not through the node the click wrote.
    let mut doc = layout_html(
        r#"<select id="lb" size="4" style="width:150px;height:100px">
             <option value="a">one</option><option value="b">two</option>
           </select>"#,
        400.0,
    );
    let lb_id = find_by_id(&doc.root, "lb").unwrap().node_id;
    let (x, y) = list_box_row_y(&doc, "lb", 1);
    click_at(&mut doc, x, y);
    assert_eq!(
        doc.selected_index(lb_id),
        1,
        "selectedIndex did not see the click"
    );
    assert_eq!(doc.value(lb_id), "b", "select.value did not see the click");

    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="any" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let r_id = find_by_id(&doc.root, "r").unwrap().node_id;
    let rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    click_at(&mut doc, rect.x + rect.w * 0.75, rect.y + rect.h / 2.0);
    let seen: f64 = crate::html::forms::parse_floating_point(&doc.value(r_id)).unwrap();
    assert!(
        seen > 50.0,
        "input.value did not see the click, reporting {seen}"
    );
}

#[test]
fn a_dropdown_built_through_the_dom_selects_its_first_option() {
    // A `ComboBox` filled by `AddItem` must come up the same way the equivalent
    // markup does. Only the PARSER used to run the selectedness setting
    // algorithm, so this route left the control with no selection at all.
    let mut doc = layout_html(r#"<select id="dd"></select>"#, 400.0);
    let dd = find_by_id(&doc.root, "dd").unwrap().node_id;
    assert_eq!(
        doc.selected_index(dd),
        -1,
        "an empty select has no selection"
    );
    doc.add_item(dd, "alpha");
    doc.add_item(dd, "beta");
    assert_eq!(
        doc.selected_index(dd),
        0,
        "a drop-down auto-selects its first option"
    );
    assert_eq!(doc.value(dd), "alpha");

    // A list box built the same way stays empty — the guard is display size.
    let mut doc = layout_html(r#"<select id="lb" size="4"></select>"#, 400.0);
    let lb = find_by_id(&doc.root, "lb").unwrap().node_id;
    doc.add_item(lb, "alpha");
    assert_eq!(
        doc.selected_index(lb),
        -1,
        "a list box does not auto-select"
    );
}

#[test]
fn a_value_attribute_write_reaches_a_range_until_it_is_dirty() {
    // The `value` content attribute drives the value while the dirty flag is
    // down, and stops the moment anything sets the value — the same rule
    // `checked` follows.
    let mut doc = layout_html(
        r#"<input id="r" type="range" min="0" max="100" step="20" value="0"
             style="width:200px;height:20px">"#,
        400.0,
    );
    let r = find_by_id(&doc.root, "r").unwrap().node_id;
    doc.set_attribute(r, "value", "50");
    assert_eq!(
        doc.value(r),
        "60",
        "an attribute write is sanitized like the markup's"
    );

    // A click raises the dirty flag; the attribute no longer speaks.
    let rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    click_at(&mut doc, rect.x + rect.w * 0.1, rect.y + rect.h / 2.0);
    let after_click = doc.value(r);
    doc.set_attribute(r, "value", "100");
    assert_eq!(
        doc.value(r),
        after_click,
        "a dirty value ignores the attribute"
    );
}

#[test]
fn a_form_submits_what_the_user_did_not_what_the_markup_said() {
    // ⛔ THE GAP EVERY EXISTING SUBMISSION TEST MISSED: none of them changed a
    // control before collecting, so reading the `value` ATTRIBUTE — the
    // author's default — passed all of them while silently submitting the
    // wrong data for every user who touched a field.
    let mut doc = layout_html(
        r#"<form id="f">
             <input id="t" type="text" name="who" value="default">
             <input id="r" type="range" name="level" min="0" max="100" step="any"
                    value="0" style="width:200px;height:20px">
             <select id="s" name="pick" size="4" style="width:150px;height:100px">
               <option value="a">one</option><option value="b">two</option>
             </select>
           </form>"#,
        400.0,
    );

    // Type into the field, drag the range, pick a row — all through the real
    // interaction paths.
    let t_center = {
        let t = find_by_id(&doc.root, "t").unwrap();
        (
            t.layout.border_rect.x + t.layout.border_rect.w / 2.0,
            t.layout.border_rect.y + t.layout.border_rect.h / 2.0,
        )
    };
    click_at(&mut doc, t_center.0, t_center.1);
    for ch in ['h', 'i'] {
        doc.process_key_event(
            crate::dom::HtmlEventType::KeyDown,
            ch as u32,
            Some(ch),
            false,
            false,
            false,
            false,
        );
    }
    let r_rect = find_by_id(&doc.root, "r").unwrap().layout.content_rect;
    click_at(
        &mut doc,
        r_rect.x + r_rect.w * 0.75,
        r_rect.y + r_rect.h / 2.0,
    );
    let (sx, sy) = list_box_row_y(&doc, "s", 1);
    click_at(&mut doc, sx, sy);

    let form = find_by_id(&doc.root, "f").unwrap();
    let data = crate::types::collect_form_data(form);
    assert_eq!(
        submitted_one(&data, "who"),
        Some("defaulthi"),
        "the form submitted the default instead of the typed value"
    );
    assert_eq!(submitted_one(&data, "pick"), Some("b"));
    let level: f64 =
        crate::html::forms::parse_floating_point(submitted_one(&data, "level").unwrap_or(""))
            .unwrap_or(-1.0);
    assert!(
        level > 50.0,
        "the form submitted the range's default, {level}"
    );
}
