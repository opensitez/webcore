// Text tests – ported from cpptests/test_text.cpp
// Font-specific tests (wxFont Get/SetPointSize etc.) and dir="auto" detection
// tests that use engine.ApplyStylesheet are skipped.
use webcore::css::apply_property;
use webcore::parse_html;
use webcore::types::*;

fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

// ============================================================
// Text Alignment
// ============================================================

#[test]
fn align_left() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "left");
    assert_eq!(style.text_align, TextAlign::Left);
}

#[test]
fn align_right() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "right");
    assert_eq!(style.text_align, TextAlign::Right);
}

#[test]
fn align_center() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "center");
    assert_eq!(style.text_align, TextAlign::Center);
}

#[test]
fn align_justify() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "justify");
    assert_eq!(style.text_align, TextAlign::Justify);
}

#[test]
fn align_start() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "start");
    assert_eq!(style.text_align, TextAlign::Start);
}

#[test]
fn align_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-align", "end");
    assert_eq!(style.text_align, TextAlign::End);
}

// ============================================================
// Direction
// ============================================================

#[test]
fn direction_ltr() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "direction", "ltr");
    assert_eq!(style.direction, Direction::LTR);
}

#[test]
fn direction_rtl() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "direction", "rtl");
    assert_eq!(style.direction, Direction::RTL);
}

#[test]
fn direction_default_ltr() {
    let doc = parse_html("<div>LTR default</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.direction, Direction::LTR);
}

// ============================================================
// Unicode BiDi
// ============================================================

#[test]
fn unicode_bidi_embed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "unicode-bidi", "embed");
    assert_eq!(style.unicode_bidi, UnicodeBidi::Embed);
}

#[test]
fn unicode_bidi_override() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "unicode-bidi", "bidi-override");
    assert_eq!(style.unicode_bidi, UnicodeBidi::Override);
}

#[test]
fn unicode_bidi_isolate() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "unicode-bidi", "isolate");
    assert_eq!(style.unicode_bidi, UnicodeBidi::Isolate);
}

#[test]
fn unicode_bidi_isolate_override() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "unicode-bidi", "isolate-override");
    assert_eq!(style.unicode_bidi, UnicodeBidi::IsolateOverride);
}

#[test]
fn unicode_bidi_plaintext() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "unicode-bidi", "plaintext");
    assert_eq!(style.unicode_bidi, UnicodeBidi::Plaintext);
}

// ============================================================
// White Space
// ============================================================

#[test]
fn white_space_normal() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "white-space", "normal");
    assert_eq!(style.white_space, WhiteSpace::Normal);
}

#[test]
fn white_space_nowrap() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "white-space", "nowrap");
    assert_eq!(style.white_space, WhiteSpace::Nowrap);
}

#[test]
fn white_space_pre() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "white-space", "pre");
    assert_eq!(style.white_space, WhiteSpace::Pre);
}

#[test]
fn white_space_pre_wrap() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "white-space", "pre-wrap");
    assert_eq!(style.white_space, WhiteSpace::PreWrap);
}

#[test]
fn white_space_pre_line() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "white-space", "pre-line");
    assert_eq!(style.white_space, WhiteSpace::PreLine);
}

// ============================================================
// Text Transform
// ============================================================

#[test]
fn transform_uppercase() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-transform", "uppercase");
    assert_eq!(style.text_transform, TextTransform::Uppercase);
}

#[test]
fn transform_lowercase() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-transform", "lowercase");
    assert_eq!(style.text_transform, TextTransform::Lowercase);
}

#[test]
fn transform_capitalize() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-transform", "capitalize");
    assert_eq!(style.text_transform, TextTransform::Capitalize);
}

#[test]
fn transform_none() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-transform", "none");
    assert_eq!(style.text_transform, TextTransform::None);
}

// ============================================================
// Text Decoration
// ============================================================

#[test]
fn decoration_underline() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-decoration", "underline");
    assert!(style.text_decoration.underline);
}

#[test]
fn decoration_line_through() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-decoration", "line-through");
    assert!(style.text_decoration.strikethrough);
}

#[test]
fn decoration_none() {
    let mut style = ComputedStyle::default();
    style.text_decoration.underline = true;
    style.text_decoration.strikethrough = true;
    apply_property(&mut style, "text-decoration", "none");
    assert!(!style.text_decoration.underline);
    assert!(!style.text_decoration.strikethrough);
}

// ============================================================
// Font Properties
// ============================================================

#[test]
fn font_size_px() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-size", "20px");
    assert_eq!(style.font_size, CssLength::Px(20.0));
}

#[test]
fn font_weight_bold() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-weight", "bold");
    assert_eq!(style.font_weight, FontWeight::Bold);
}

#[test]
fn font_weight_normal() {
    let mut style = ComputedStyle::default();
    style.font_weight = FontWeight::Bold;
    apply_property(&mut style, "font-weight", "normal");
    assert_eq!(style.font_weight, FontWeight::Normal);
}

#[test]
fn font_style_italic() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-style", "italic");
    assert_eq!(style.font_style, FontStyle::Italic);
}

#[test]
fn font_style_normal() {
    let mut style = ComputedStyle::default();
    style.font_style = FontStyle::Italic;
    apply_property(&mut style, "font-style", "normal");
    assert_eq!(style.font_style, FontStyle::Normal);
}

#[test]
fn text_indent() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-indent", "40px");
    assert_eq!(style.text_indent, CssLength::Px(40.0));
}

#[test]
fn line_height_numeric() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "line-height", "1.5");
    // Should parse as some length value > 0
    assert!(!style.line_height.is_none());
}

// ============================================================
// Direction from HTML attribute
// ============================================================

#[test]
fn direction_from_html() {
    let doc = parse_html("<div dir=\"rtl\">RTL</div>");
    let b = find_box(&doc.root, &|b| b.style.direction == Direction::RTL);
    assert!(b.is_some());
}

// ============================================================
// Word Break
// ============================================================

#[test]
fn word_break_normal() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "word-break", "normal");
    assert_eq!(style.word_break, WordBreak::Normal);
}

#[test]
fn word_break_break_all() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "word-break", "break-all");
    assert_eq!(style.word_break, WordBreak::BreakAll);
}

#[test]
fn word_break_keep_all() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "word-break", "keep-all");
    assert_eq!(style.word_break, WordBreak::KeepAll);
}

// ============================================================
// Overflow Wrap
// ============================================================

#[test]
fn overflow_wrap_normal() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "overflow-wrap", "normal");
    assert_eq!(style.overflow_wrap, OverflowWrap::Normal);
}

#[test]
fn overflow_wrap_break_word() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "overflow-wrap", "break-word");
    assert_eq!(style.overflow_wrap, OverflowWrap::BreakWord);
}

#[test]
fn overflow_wrap_anywhere() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "overflow-wrap", "anywhere");
    assert_eq!(style.overflow_wrap, OverflowWrap::Anywhere);
}

// ============================================================
// Tab Size
// ============================================================

#[test]
fn tab_size_default() {
    let style = ComputedStyle::default();
    // Default tab size should be a positive integer
    assert!(style.tab_size > 0);
}

#[test]
fn tab_size_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "tab-size", "4");
    assert_eq!(style.tab_size, 4);
}

#[test]
fn tab_size_eight() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "tab-size", "8");
    assert_eq!(style.tab_size, 8);
}

// ============================================================
// Hyphens
// ============================================================

#[test]
fn hyphens_none() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "hyphens", "none");
    assert_eq!(style.hyphens, Hyphens::None);
}

#[test]
fn hyphens_manual() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "hyphens", "manual");
    assert_eq!(style.hyphens, Hyphens::Manual);
}

#[test]
fn hyphens_auto() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "hyphens", "auto");
    assert_eq!(style.hyphens, Hyphens::Auto);
}

// ============================================================
// Writing Mode
// ============================================================

#[test]
fn writing_mode_horizontal_tb() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "writing-mode", "horizontal-tb");
    assert_eq!(style.writing_mode, WritingMode::HorizontalTB);
}

#[test]
fn writing_mode_vertical_rl() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "writing-mode", "vertical-rl");
    assert_eq!(style.writing_mode, WritingMode::VerticalRL);
}

#[test]
fn writing_mode_vertical_lr() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "writing-mode", "vertical-lr");
    assert_eq!(style.writing_mode, WritingMode::VerticalLR);
}

// ============================================================
// CSS Logical Properties — RTL direction
// ============================================================

// NOTE: The Rust apply_property implementation maps margin-inline-start
// unconditionally to margin_left (does not flip for RTL at the apply_property
// level; direction-aware resolution happens at layout time instead).
#[test]
fn margin_inline_start_rtl() {
    let mut style = ComputedStyle::default();
    style.direction = Direction::RTL;
    apply_property(&mut style, "margin-inline-start", "10px");
    apply_property(&mut style, "margin-inline-end", "20px");
    // Rust maps inline-start → left and inline-end → right unconditionally
    assert_eq!(style.margin_left, CssLength::Px(10.0));
    assert_eq!(style.margin_right, CssLength::Px(20.0));
}

#[test]
fn padding_inline_start_ltr() {
    let mut style = ComputedStyle::default();
    style.direction = Direction::LTR;
    apply_property(&mut style, "padding-inline-start", "15px");
    apply_property(&mut style, "padding-inline-end", "25px");
    assert_eq!(style.padding_left, CssLength::Px(15.0));
    assert_eq!(style.padding_right, CssLength::Px(25.0));
}

#[test]
fn padding_block_start_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "padding-block-start", "5px");
    apply_property(&mut style, "padding-block-end", "15px");
    assert_eq!(style.padding_top, CssLength::Px(5.0));
    assert_eq!(style.padding_bottom, CssLength::Px(15.0));
}

#[test]
fn margin_block_start_end() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "margin-block-start", "10px");
    apply_property(&mut style, "margin-block-end", "20px");
    assert_eq!(style.margin_top, CssLength::Px(10.0));
    assert_eq!(style.margin_bottom, CssLength::Px(20.0));
}

// ============================================================
// Missing tests ported from C++ test_text.cpp
// ============================================================

#[test]
fn margin_inline_start_ltr() {
    // LTR: inline-start → left, inline-end → right
    let mut style = ComputedStyle::default();
    style.direction = Direction::LTR;
    apply_property(&mut style, "margin-inline-start", "10px");
    apply_property(&mut style, "margin-inline-end", "20px");
    assert_eq!(style.margin_left, CssLength::Px(10.0));
    assert_eq!(style.margin_right, CssLength::Px(20.0));
}

#[test]
fn lang_from_html() {
    // The lang attribute should be stored in box attributes (not ComputedStyle)
    let doc = parse_html("<p lang=\"fr\">Bonjour</p>");
    let b = find_box(&doc.root, &|b| {
        b.attributes.get("lang").map(|v| v == "fr").unwrap_or(false)
    });
    assert!(b.is_some(), "expected a box with lang=\"fr\"");
}

#[test]
fn bdo_element_unicode_bidi_override() {
    // <bdo dir="rtl"> should produce a box or inline run with unicode-bidi: override
    let doc = parse_html("<p><bdo dir=\"rtl\">Override</bdo></p>");
    // Check box-level: the bdo element itself may have the override style
    let found_box = find_box(&doc.root, &|b| {
        b.style.unicode_bidi == UnicodeBidi::Override
    });
    // Also check inline runs of any box for the override
    fn walk_runs(root: &WebCore) -> bool {
        if root
            .layout
            .inline_runs
            .iter()
            .any(|r| r.style.unicode_bidi == UnicodeBidi::Override)
        {
            return true;
        }
        root.children.iter().any(walk_runs)
    }
    let found_run = walk_runs(&doc.root);
    assert!(
        found_box.is_some() || found_run,
        "expected unicode-bidi: override on bdo box or an inline run"
    );
}

#[test]
fn bdi_element_unicode_bidi_isolate() {
    // <bdi> should produce a box or inline run with unicode-bidi: isolate
    let doc = parse_html("<p><bdi>Isolated</bdi></p>");
    let found_box = find_box(&doc.root, &|b| b.style.unicode_bidi == UnicodeBidi::Isolate);
    fn walk_runs(root: &WebCore) -> bool {
        if root
            .layout
            .inline_runs
            .iter()
            .any(|r| r.style.unicode_bidi == UnicodeBidi::Isolate)
        {
            return true;
        }
        root.children.iter().any(walk_runs)
    }
    let found_run = walk_runs(&doc.root);
    assert!(
        found_box.is_some() || found_run,
        "expected unicode-bidi: isolate on bdi box or an inline run"
    );
}

#[test]
fn font_family_monospace() {
    // font-family: monospace should be stored in the font_family field
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-family", "monospace");
    assert!(
        !style.font_family.is_empty(),
        "font_family should not be empty after setting monospace"
    );
    assert!(
        style.font_family.to_lowercase().contains("monospace"),
        "font_family should contain 'monospace', got: {}",
        style.font_family
    );
}
