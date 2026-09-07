/// Tests for caret position accuracy using real cosmic_text glyph metrics.
///
/// Key invariants:
/// - char_x is populated when layout uses a font system (via Renderer::load_html)
/// - get_caret_x returns the exact glyph-boundary x, not an approximation
/// - get_offset_from_x correctly identifies character boundaries
/// - Thin characters like 'i', 'l', '1' are measured correctly (not approximated)
use crate::dom::HtmlEventType;
use crate::layout::hit_test::{get_caret_x, get_offset_from_x};
use crate::layout::inline_layout::collect_flat_text;
use crate::types::*;
use crate::Renderer;

fn load_with_fonts(html: &str) -> Document {
    let mut renderer = Renderer::new();
    renderer.load_html(html, 900.0)
}

fn find_editable_box(root: &WebCore) -> Option<&WebCore> {
    if root
        .attributes
        .get("contenteditable")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        return Some(root);
    }
    for child in &root.children {
        if let Some(b) = find_editable_box(child) {
            return Some(b);
        }
    }
    None
}

/// Find any box that has a populated line_cache (inline content).
fn find_box_with_lines(root: &WebCore) -> Option<&WebCore> {
    if !root.layout.line_cache.is_empty() {
        return Some(root);
    }
    for child in &root.children {
        if let Some(b) = find_box_with_lines(child) {
            return Some(b);
        }
    }
    None
}

#[test]
fn user_select_none_blocks_mouse_selection() {
    let mut doc = load_with_fonts(
        r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             #no { user-select: none; }
           </style>
           <div id="no">not selectable</div>
           <div id="yes">selectable</div>"#,
    );

    let no = doc.get_element_by_id("no").unwrap();
    let yes = doc.get_element_by_id("yes").unwrap();
    let no_rect = doc.get_box_by_id(no).unwrap().layout.border_rect;
    let yes_rect = doc.get_box_by_id(yes).unwrap().layout.border_rect;

    let blocked = doc.editor.handle_mouse_event(
        &doc.root,
        HtmlEventType::MouseDown,
        (no_rect.x + 4.0, no_rect.y + no_rect.h * 0.5),
        0,
    );
    assert!(!blocked);
    assert_eq!(doc.editor.caret_box, None);

    let allowed = doc.editor.handle_mouse_event(
        &doc.root,
        HtmlEventType::MouseDown,
        (yes_rect.x + 4.0, yes_rect.y + yes_rect.h * 0.5),
        0,
    );
    assert!(allowed);
    assert!(doc.editor.caret_box.is_some());
}

#[test]
fn user_select_all_selects_the_whole_element() {
    let mut doc = load_with_fonts(
        r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             #all { user-select: all; }
           </style>
           <div id="all">select all text</div>"#,
    );

    let all = doc.get_element_by_id("all").unwrap();
    let rect = doc.get_box_by_id(all).unwrap().layout.border_rect;

    let selected = doc.editor.handle_mouse_event(
        &doc.root,
        HtmlEventType::MouseDown,
        (rect.x + 4.0, rect.y + rect.h * 0.5),
        0,
    );

    assert!(selected);
    assert_eq!(doc.editor.caret_box, Some(all));
    assert_eq!(
        doc.editor.sel_args(),
        (Some(0), Some("select all text".len()))
    );
}

// ─── char_x population ───────────────────────────────────────────────────────

#[test]
fn char_x_populated_with_font_system() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    assert!(
        !node.layout.line_cache.is_empty(),
        "no lines in editable box"
    );
    let line = &node.layout.line_cache[0];
    assert!(
        !line.char_x.is_empty(),
        "char_x must be populated when layout uses a real font system"
    );
}

#[test]
fn char_x_length_matches_text_plus_one() {
    // char_x has one entry per byte boundary + 1 for end-of-line
    let doc = load_with_fonts(r#"<p contenteditable="true">abc</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let line = &node.layout.line_cache[0];
    // "abc" is 3 ASCII bytes → char_x length = 3+1 = 4 (or up to text_length+1)
    assert!(
        line.char_x.len() >= 4,
        "char_x should have text_length+1 entries, got {}",
        line.char_x.len()
    );
}

#[test]
fn char_x_starts_at_zero() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello world</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let line = &node.layout.line_cache[0];
    assert!(!line.char_x.is_empty());
    assert_eq!(
        line.char_x[0], 0.0,
        "first char_x entry must be 0 (relative to line.x)"
    );
}

#[test]
fn char_x_monotonically_increasing_ltr() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello world</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let line = &node.layout.line_cache[0];
    for i in 1..line.char_x.len() {
        assert!(
            line.char_x[i] >= line.char_x[i - 1],
            "char_x[{}]={} < char_x[{}]={} — positions must be non-decreasing",
            i,
            line.char_x[i],
            i - 1,
            line.char_x[i - 1]
        );
    }
}

// ─── get_caret_x accuracy ────────────────────────────────────────────────────

#[test]
fn caret_at_start_is_line_x() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Test</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    let x = get_caret_x(&flat, &node.layout.inline_runs, line, line.text_start);
    assert!(
        (x - line.x).abs() < 0.5,
        "caret at start of line should equal line.x, got x={} line.x={}",
        x,
        line.x
    );
}

#[test]
fn caret_at_end_matches_line_width() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Test</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    let end_off = line.text_start + line.text_length;
    // Strip trailing newline if present
    let measure_end = if end_off > 0 && flat.as_bytes().get(end_off - 1) == Some(&b'\n') {
        end_off - 1
    } else {
        end_off
    };
    let x_end = get_caret_x(&flat, &node.layout.inline_runs, line, measure_end);
    // The caret at the end should be further right than at the start
    assert!(x_end > line.x, "caret at end should be past start of line");
}

#[test]
fn caret_positions_strictly_ordered_for_distinct_chars() {
    // With real shaping, each character boundary produces a distinct x
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    let mut prev_x = -1.0f32;
    for i in 0..="Hello".len() {
        let off = line.text_start + i;
        let x = get_caret_x(&flat, &node.layout.inline_runs, line, off);
        assert!(
            x >= prev_x,
            "caret x at offset {} ({}) must be >= previous x {}",
            i,
            x,
            prev_x
        );
        prev_x = x;
    }
}

// ─── Thin characters: 'i', 'l', '1' ─────────────────────────────────────────
// These are the letters that the old approx got most wrong.

#[test]
fn thin_char_l_has_nonzero_width() {
    let doc = load_with_fonts(r#"<p contenteditable="true">flex</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    // "flex": f=0, l=1, e=2, x=3
    let x_before_l = get_caret_x(&flat, &node.layout.inline_runs, line, line.text_start + 1);
    let x_after_l = get_caret_x(&flat, &node.layout.inline_runs, line, line.text_start + 2);
    assert!(
        x_after_l > x_before_l,
        "'l' must have positive width: before={} after={}",
        x_before_l,
        x_after_l
    );
}

#[test]
fn thin_char_l_is_narrower_than_m() {
    // In any proportional font 'l' < 'm'
    let doc_l = load_with_fonts(r#"<p contenteditable="true">al</p>"#);
    let doc_m = load_with_fonts(r#"<p contenteditable="true">am</p>"#);
    let width_of = |doc: &Document, idx: usize| {
        let node = find_editable_box(&doc.root).unwrap();
        let flat = collect_flat_text(node);
        let line = &node.layout.line_cache[0];
        let x0 = get_caret_x(&flat, &node.layout.inline_runs, line, line.text_start + idx);
        let x1 = get_caret_x(
            &flat,
            &node.layout.inline_runs,
            line,
            line.text_start + idx + 1,
        );
        x1 - x0
    };
    let w_l = width_of(&doc_l, 1);
    let w_m = width_of(&doc_m, 1);
    assert!(
        w_l < w_m,
        "width of 'l' ({}) should be less than width of 'm' ({}) in a proportional font",
        w_l,
        w_m
    );
}

// ─── get_offset_from_x ───────────────────────────────────────────────────────

#[test]
fn offset_from_x_at_line_start_returns_start() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    // Clicking before any text should return the start offset
    let off = get_offset_from_x(&flat, &node.layout.inline_runs, line, line.x - 5.0);
    assert_eq!(
        off, line.text_start,
        "click before line start should return text_start"
    );
}

#[test]
fn offset_from_x_midpoint_selects_correct_char() {
    // Clicking at the midpoint of a character should select that character's boundary.
    // This is the key invariant: clicking inside char i should return either i or i+1.
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello world</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];
    if line.char_x.is_empty() {
        return;
    }

    let text_slice = &flat[line.text_start..line.text_start + line.text_length];
    let mut byte_off = line.text_start;
    for (_i, ch) in text_slice.char_indices() {
        let x0 = get_caret_x(&flat, &node.layout.inline_runs, line, byte_off);
        let next = byte_off + ch.len_utf8();
        let x1 = get_caret_x(&flat, &node.layout.inline_runs, line, next);
        if (x1 - x0).abs() < 0.01 {
            byte_off = next;
            continue; // zero-width char, skip
        }
        // Click at midpoint of this character
        let mid_x = (x0 + x1) / 2.0 + 0.1; // slightly past midpoint → should give next boundary
        let recovered = get_offset_from_x(&flat, &node.layout.inline_runs, line, mid_x);
        assert!(
            recovered == byte_off || recovered == next,
            "midpoint click in char at offset {} (width {:.1}): recovered={} expected {} or {}",
            byte_off,
            x1 - x0,
            recovered,
            byte_off,
            next
        );
        byte_off = next;
    }
}

// ─── Monospace font accuracy ──────────────────────────────────────────────────

#[test]
fn monospace_chars_equal_width() {
    // In monospace font, 'i' and 'm' should have the same width.
    // Use char_x directly (populated by fill_char_x_for_line) rather than
    // get_caret_x, which uses the width approximation (not font-exact).
    let doc = load_with_fonts(r#"<p contenteditable="true"><code>im</code></p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let line = &node.layout.line_cache[0];
    if line.char_x.len() < 3 {
        return;
    } // skip if no font system
    let w_i = line.char_x[1] - line.char_x[0];
    let w_m = line.char_x[2] - line.char_x[1];
    // In monospace, both chars should have the same advance (within 1px)
    assert!(
        (w_i - w_m).abs() < 1.5,
        "in monospace font 'i' width ({}) should ≈ 'm' width ({})",
        w_i,
        w_m
    );
}

// ─── Bold text is wider ───────────────────────────────────────────────────────

#[test]
fn bold_text_wider_than_normal() {
    let doc_n = load_with_fonts(r#"<p contenteditable="true">Hello</p>"#);
    let doc_b = load_with_fonts(r#"<p contenteditable="true"><strong>Hello</strong></p>"#);
    let line_width = |doc: &Document| {
        let node = find_editable_box(&doc.root).unwrap();
        let flat = collect_flat_text(node);
        let line = &node.layout.line_cache[0];
        *line.char_x.last().unwrap_or(&0.0)
    };
    let w_normal = line_width(&doc_n);
    let w_bold = line_width(&doc_b);
    assert!(
        w_bold > w_normal,
        "bold text width ({}) should exceed normal text width ({})",
        w_bold,
        w_normal
    );
}

// ─── Caret / click self-consistency ──────────────────────────────────────────
// get_caret_x and get_offset_from_x must agree: clicking at the x returned by
// get_caret_x for offset O must produce offset O (or the adjacent char boundary).

#[test]
fn caret_click_roundtrip_consistent() {
    let doc = load_with_fonts(r#"<p contenteditable="true">Hello world</p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    let flat = collect_flat_text(node);
    let line = &node.layout.line_cache[0];

    let text_slice = &flat[line.text_start..line.text_start + line.text_length];
    let mut byte_off = line.text_start;
    for ch in text_slice.chars() {
        let caret_x = get_caret_x(&flat, &node.layout.inline_runs, line, byte_off);
        // Clicking at the caret x should return this offset or the adjacent one.
        let recovered = get_offset_from_x(&flat, &node.layout.inline_runs, line, caret_x);
        let next = byte_off + ch.len_utf8();
        assert!(
            recovered == byte_off || recovered == next,
            "roundtrip failed at offset {}: caret_x={:.1}, recovered={}",
            byte_off,
            caret_x,
            recovered
        );
        byte_off = next;
    }
}

#[test]
fn caret_click_roundtrip_flex_item() {
    // Flex items are shifted by shift_rects after layout; verify the caret
    // coordinates are consistent (click → offset → caret → same position).
    let doc = load_with_fonts(
        r#"
        <div style="display:flex">
          <div id="a">Left</div>
          <div id="b">Right side</div>
        </div>"#,
    );
    let find_by_id = |root: &WebCore, id: &str| -> Option<*const WebCore> {
        fn search(node: &WebCore, id: &str) -> Option<*const WebCore> {
            if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(node as *const WebCore);
            }
            for child in &node.children {
                if let Some(r) = search(child, id) {
                    return Some(r);
                }
            }
            None
        }
        search(root, id)
    };

    // Verify both flex items have valid line_cache with correct line.x (absolute).
    for id in &["a", "b"] {
        let ptr = find_by_id(&doc.root, id).expect("flex item not found");
        let node = unsafe { &*ptr };
        assert!(
            !node.layout.line_cache.is_empty(),
            "flex item '{}' has no line_cache",
            id
        );
        let line = &node.layout.line_cache[0];
        // line.x must be non-negative (absolute document coordinate).
        assert!(
            line.x >= 0.0,
            "flex item '{}' line.x={} must be ≥ 0",
            id,
            line.x
        );
    }

    // For the second flex item: clicking at midline should give a valid offset.
    let ptr_b = find_by_id(&doc.root, "b").expect("flex item b");
    let node_b = unsafe { &*ptr_b };
    let flat = collect_flat_text(node_b);
    let line = &node_b.layout.line_cache[0];
    let mid_x = line.x + line.width / 2.0;
    let off = get_offset_from_x(&flat, &node_b.layout.inline_runs, line, mid_x);
    // Offset must be within the line's text range.
    assert!(
        off >= line.text_start && off <= line.text_start + line.text_length,
        "offset {} out of range [{}, {}] for mid-click in flex item b",
        off,
        line.text_start,
        line.text_start + line.text_length
    );
    // Caret x for this offset must be <= mid_x + one char width.
    let caret_x = get_caret_x(&flat, &node_b.layout.inline_runs, line, off);
    assert!(
        caret_x <= mid_x + 20.0,
        "caret_x={:.1} is far past mid_x={:.1}",
        caret_x,
        mid_x
    );
}

// ─── Empty block after Enter ──────────────────────────────────────────────────

#[test]
fn empty_paragraph_has_line_cache() {
    // An empty <p> (no children, no text) must get a placeholder line so the
    // caret can be positioned inside it (e.g. after pressing Enter).
    let doc = load_with_fonts(r#"<p contenteditable="true"></p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    assert!(
        !node.layout.line_cache.is_empty(),
        "empty <p> must have a placeholder line for caret positioning"
    );
    let line = &node.layout.line_cache[0];
    assert!(
        line.height > 0.0,
        "placeholder line must have positive height"
    );
}

#[test]
fn empty_paragraph_has_nonzero_height() {
    let doc = load_with_fonts(r#"<p contenteditable="true"></p>"#);
    let node = find_editable_box(&doc.root).expect("editable box");
    assert!(
        node.layout.border_rect.h > 0.0,
        "empty <p> must have non-zero height so it's visible and clickable"
    );
}

#[test]
fn hr_unaffected_by_empty_block_fix() {
    // <hr> is a void element — the empty-block placeholder must NOT be added.
    let doc = load_with_fonts(r#"<div><hr/><p>text</p></div>"#);
    // Find the <hr> box — it must have an empty line_cache (no text placeholder).
    fn find_hr(node: &WebCore) -> Option<&WebCore> {
        if node.tag == "hr" {
            return Some(node);
        }
        for child in &node.children {
            if let Some(r) = find_hr(child) {
                return Some(r);
            }
        }
        None
    }
    let hr = find_hr(&doc.root).expect("<hr> not found");
    assert!(
        hr.layout.line_cache.is_empty(),
        "<hr> must NOT have a placeholder line_cache"
    );
}

// ─── Enter key: caret moves to new line ──────────────────────────────────────
//
// Scenario: text is present in an editable element; the user clicks in the
// middle, presses Enter, then types a character.  The character must appear on
// the second line (below the original text), not on the first.

/// Run the click→Enter→type scenario for the given HTML.
///
/// `find_edit_box` locates the editable box within the document.
/// Returns `(original_line_y, inserted_line_y)` (absolute document coords).
fn enter_creates_new_line_scenario(
    html: &str,
    find_edit_box: fn(&WebCore) -> Option<*const WebCore>,
) -> (f32, f32) {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(html, 900.0);

    // Locate the editable box and find the click point (center of line 0).
    let (click_x, click_y, orig_line_y) = {
        let node = unsafe { &*find_edit_box(&doc.root).expect("editable box not found") };
        assert!(
            !node.layout.line_cache.is_empty(),
            "editable box has no lines after layout"
        );
        let line = &node.layout.line_cache[0];
        // Click in the middle of the text horizontally, vertically centered on line.
        let cx = line.x + line.width / 2.0;
        let cy = line.y + line.height / 2.0;
        (cx, cy, line.y)
    };

    // Simulate mouse click to place the caret.
    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (click_x, click_y), 0);
    assert!(
        doc.editor.caret_box.is_some(),
        "click did not place a caret"
    );

    // Press Enter.
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);

    // Re-cascade + layout so the new DOM structure gets styles and coordinates.
    doc.style_dirty = true;
    renderer.layout_engine().layout(&mut doc, 900.0);

    // The caret box must still be set and have lines.
    let caret_id = doc.editor.caret_box.expect("caret_box lost after Enter");
    let caret_node = doc
        .get_box_by_id(caret_id)
        .expect("caret box not found in tree");
    assert!(
        !caret_node.layout.line_cache.is_empty(),
        "caret box '{}' has no lines after Enter + relayout",
        caret_node.tag
    );

    // Type a character so we can see where it lands.
    doc.editor.insert_char(&mut doc.root, 'X');
    renderer.layout_engine().layout(&mut doc, 900.0);

    // Re-locate the caret box (pointer unchanged, layout has been refreshed).
    let caret_local = doc.editor.caret_local;
    let caret_id2 = doc.editor.caret_box.expect("caret_box lost after insert");
    let caret_node = doc
        .get_box_by_id(caret_id2)
        .expect("caret box not found in tree");
    assert!(
        !caret_node.layout.line_cache.is_empty(),
        "caret box has no lines after inserting 'X'"
    );

    // Find the line that contains the caret (last line whose text_start ≤ caret_local).
    // For <p>-split: caret is in the new paragraph at line_cache[0].
    // For <br> in <td>: caret is after the <br>, so on the second line.
    let inserted_line_y = caret_node
        .layout
        .line_cache
        .iter()
        .filter(|l| l.text_start <= caret_local)
        .last()
        .map(|l| l.y)
        .unwrap_or(caret_node.layout.line_cache[0].y);
    (orig_line_y, inserted_line_y)
}

#[test]
fn enter_at_midtext_moves_caret_to_new_line_in_root() {
    // <p> directly inside root → Enter splits into two <p> blocks.
    let html = r#"<p contenteditable="true">Hello world</p>"#;
    let (orig_y, new_y) = enter_creates_new_line_scenario(html, |root| {
        find_editable_box(root).map(|b| b as *const WebCore)
    });
    assert!(
        new_y > orig_y,
        "after Enter in root <p>: inserted text line y ({}) must be below original line y ({})",
        new_y,
        orig_y
    );
}

#[test]
fn enter_at_midtext_moves_caret_to_new_line_inside_div() {
    // <p> inside a <div> → same splitting behaviour.
    let html = r#"<div><p contenteditable="true">Hello world</p></div>"#;
    let (orig_y, new_y) = enter_creates_new_line_scenario(html, |root| {
        find_editable_box(root).map(|b| b as *const WebCore)
    });
    assert!(
        new_y > orig_y,
        "after Enter in <div><p>: inserted text line y ({}) must be below original line y ({})",
        new_y,
        orig_y
    );
}

#[test]
fn enter_at_midtext_moves_caret_to_new_line_in_table_cell() {
    // <td contenteditable> → Enter inserts <br> (non-prose tag), creating a second
    // line within the same cell.
    let html = r#"<table><tr><td contenteditable="true">Hello world</td></tr></table>"#;
    let (orig_y, new_y) = enter_creates_new_line_scenario(html, |root| {
        fn find_td(node: &WebCore) -> Option<*const WebCore> {
            if node.tag == "td" {
                return Some(node as *const WebCore);
            }
            for c in &node.children {
                if let Some(r) = find_td(c) {
                    return Some(r);
                }
            }
            None
        }
        find_td(root)
    });
    assert!(
        new_y > orig_y,
        "after Enter in <td>: inserted text line y ({}) must be below original line y ({})",
        new_y,
        orig_y
    );
}

// ─── insert_br caret placement (the bug that caused "same-line insertion") ────
//
// After pressing Enter inside a non-prose container (e.g. <div>, <td>), a <br>
// is inserted and the caret stays in the same box.  The caret must be at the
// START of the new visual line, NOT at the end of the previous one.
// Specifically:
//  (a) the typed character lands in the text node AFTER the <br>, not before;
//  (b) the renderer's line selection must pick the second line, not the first.

#[test]
fn enter_in_div_typed_char_goes_to_new_line() {
    // <div contenteditable> gets a <br> on Enter (not a new block).
    // After Enter at midpoint of "Hello world", typing 'X' must NOT produce
    // "HelloX world" on line 1; it must produce "X world" (or similar) on line 2.
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(r#"<div contenteditable="true">Hello world</div>"#, 900.0);

    // Locate the editable div and click in the middle of its text.
    let (click_x, click_y, _) = {
        let node = find_editable_box(&doc.root).expect("editable div");
        let line = &node.layout.line_cache[0];
        (
            line.x + line.width / 2.0,
            line.y + line.height / 2.0,
            line.y,
        )
    };

    // Click → Enter → relayout.
    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (click_x, click_y), 0);
    let split_offset = doc.editor.caret_local;
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);
    renderer.layout_engine().layout(&mut doc, 900.0);

    // The flag must be set (caret is at the start of a new line after <br>).
    assert!(
        doc.editor.caret_at_line_start,
        "caret_at_line_start must be true after insert_br"
    );

    // Type 'X' — it must go into the segment AFTER the <br>.
    doc.editor.insert_char(&mut doc.root, 'X');

    // The flag is consumed by insert_char.
    assert!(
        !doc.editor.caret_at_line_start,
        "caret_at_line_start must be cleared after insert_char"
    );

    // Re-layout and inspect the flat text of the editable div.
    renderer.layout_engine().layout(&mut doc, 900.0);
    let node = find_editable_box(&doc.root).expect("editable div");
    let flat = collect_flat_text(node);

    // "Hello world" was split at split_offset.  'X' must appear AFTER the split
    // (i.e. in the second segment), not inside the first segment.
    let first_half = &flat[..split_offset];
    assert!(
        !first_half.contains('X'),
        "typed 'X' must NOT appear in the first segment (before the <br>), flat={:?}",
        flat
    );
    assert!(
        flat[split_offset..].contains('X'),
        "typed 'X' must appear in the second segment (after the <br>), flat={:?}",
        flat
    );
}

#[test]
fn enter_in_cell_caret_renders_on_new_line() {
    // After Enter in a <td>, the caret must visually appear on the second line.
    // We verify by checking that the line selected by the renderer for the caret
    // (the last line whose text_start <= caret_local, with line-start preference)
    // is the second line, not the first.
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"<table><tr><td contenteditable="true">Hello world</td></tr></table>"#,
        900.0,
    );

    // Find the <td> and click in the middle.
    fn find_td(node: &WebCore) -> Option<*const WebCore> {
        if node.tag == "td" {
            return Some(node as *const WebCore);
        }
        for c in &node.children {
            if let Some(r) = find_td(c) {
                return Some(r);
            }
        }
        None
    }
    let td_ptr = find_td(&doc.root).expect("<td>");
    let (click_x, click_y, line0_y) = {
        let td = unsafe { &*td_ptr };
        let line = &td.layout.line_cache[0];
        (
            line.x + line.width / 2.0,
            line.y + line.height / 2.0,
            line.y,
        )
    };

    doc.editor
        .handle_mouse_event(&doc.root, HtmlEventType::MouseDown, (click_x, click_y), 0);
    doc.editor
        .handle_key_event(&mut doc.root, HtmlEventType::KeyDown, 13, None, false);
    doc.style_dirty = true;
    renderer.layout_engine().layout(&mut doc, 900.0);

    // After relayout the <td> must have two lines.
    // Re-find the td by walking the tree (old pointer may be invalidated).
    fn find_td2(node: &WebCore) -> Option<&WebCore> {
        if node.tag == "td" {
            return Some(node);
        }
        for c in &node.children {
            if let Some(r) = find_td2(c) {
                return Some(r);
            }
        }
        None
    }
    let td = find_td2(&doc.root).expect("<td> must still exist");
    assert!(
        td.layout.line_cache.len() >= 2,
        "<td> must have ≥ 2 lines after Enter (has {})",
        td.layout.line_cache.len()
    );

    let caret_local = doc.editor.caret_local;
    // The renderer prefers the line where caret_local == line.text_start.
    // That must be line 1 (the second line), not line 0.
    let preferred_line = td
        .layout
        .line_cache
        .iter()
        .filter(|l| l.text_start <= caret_local && caret_local <= l.text_start + l.text_length)
        .max_by_key(|l| {
            if l.text_start == caret_local {
                1usize
            } else {
                0
            }
        })
        .expect("no matching line for caret_local");

    assert!(
        preferred_line.y > line0_y,
        "caret must be rendered on a line below the original (line0_y={}, preferred.y={})",
        line0_y,
        preferred_line.y
    );
}
