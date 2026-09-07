// Port of LayoutText.cpp
//
// Provides BiDi resolution, text measurement, and line-breaking logic.
// Uses the `unicode-bidi` crate for UAX#9 algorithm.

use crate::layout::inline_layout::{measure_text_width, InlineItem, InlineItemKind};
use crate::types::*;
use unicode_bidi::{BidiClass, BidiInfo, Level};

// ─── BiDi: paragraph direction detection ─────────────────────────────────────

/// Detect the paragraph base direction from the first strong character.
/// Mirrors LayoutEngine::DetectParagraphDirection in C++.
pub fn detect_paragraph_direction(text: &str) -> Direction {
    // Use unicode-bidi's auto-detection (scans for first strong char)
    let bidi = BidiInfo::new(text, None);
    if let Some(para) = bidi.paragraphs.first() {
        if para.level.is_rtl() {
            return Direction::RTL;
        }
    }
    Direction::LTR
}

pub fn first_strong_direction(text: &str) -> Option<Direction> {
    for ch in text.chars() {
        match unicode_bidi::bidi_class(ch) {
            BidiClass::L => return Some(Direction::LTR),
            BidiClass::R | BidiClass::AL => return Some(Direction::RTL),
            _ => {}
        }
    }
    None
}

/// Detect direction for a slice of text by byte range.
pub fn detect_direction_in_range(text: &str, start: usize, length: usize) -> Direction {
    let end = (start + length).min(text.len());
    detect_paragraph_direction(&text[start..end])
}

// ─── BiDi: resolve visual segments for a line ─────────────────────────────────

/// Populate `line.visual_segments` with BiDi-reordered runs.
/// If the line is pure LTR, `visual_segments` is cleared (renderer uses logical order).
/// Mirrors LayoutEngine::ResolveBidiLine in C++.
/// Uses bidi.levels directly (NOT reorder_line which returns Cow<str>).
pub fn resolve_bidi_line(
    text: &str,
    line: &mut LayoutLine,
    para_dir: Direction,
    unicode_bidi: UnicodeBidi,
) {
    let byte_start = line.text_start;
    let byte_end = byte_start + line.text_length;
    if byte_end > text.len() || line.text_length == 0 {
        line.visual_segments.clear();
        return;
    }
    if !text.is_char_boundary(byte_start) || !text.is_char_boundary(byte_end) {
        line.visual_segments.clear();
        return;
    }
    let line_text_raw = &text[byte_start..byte_end];
    // Normalize raw newlines (from HTML source formatting) to spaces so that
    // unicode-bidi doesn't treat them as paragraph separators (bidi type B),
    // which would produce incorrect embedding levels for subsequent RTL characters.
    let line_text_owned: String;
    let line_text: &str = if line_text_raw.contains('\n') || line_text_raw.contains('\r') {
        line_text_owned = line_text_raw
            .chars()
            .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
            .collect();
        &line_text_owned
    } else {
        line_text_raw
    };
    if matches!(
        unicode_bidi,
        UnicodeBidi::Override | UnicodeBidi::IsolateOverride
    ) {
        resolve_bidi_override_line(line_text, line, byte_start, para_dir);
        return;
    }
    let para_dir = if unicode_bidi == UnicodeBidi::Plaintext {
        detect_paragraph_direction(line_text)
    } else {
        para_dir
    };
    let para_level = Some(match para_dir {
        Direction::LTR => Level::ltr(),
        Direction::RTL => Level::rtl(),
    });
    let bidi = BidiInfo::new(line_text, para_level);
    if bidi.paragraphs.is_empty() {
        line.visual_segments.clear();
        return;
    }
    // Fast path: no RTL and paragraph is LTR
    let has_rtl = bidi.levels.iter().any(|l| l.is_rtl());
    if !has_rtl && para_dir == Direction::LTR {
        line.visual_segments.clear();
        return;
    }

    // char→byte table
    let char_bytes: Vec<usize> = line_text.char_indices().map(|(b, _)| b).collect();
    let char_count = char_bytes.len();

    // Build level runs: group consecutive chars by embedding level
    #[derive(Clone)]
    struct LevelRun {
        start: usize,
        end: usize,
        level: u8,
    }
    let mut runs: Vec<LevelRun> = Vec::new();
    if char_count > 0 && !bidi.levels.is_empty() {
        let mut run_start = 0usize;
        let mut cur_lv = bidi.levels[0].number();
        for ci in 1..=char_count {
            let next_lv = if ci < bidi.levels.len() {
                bidi.levels[ci].number()
            } else {
                cur_lv
            };
            if ci == char_count || next_lv != cur_lv {
                runs.push(LevelRun {
                    start: run_start,
                    end: ci,
                    level: cur_lv,
                });
                run_start = ci;
                cur_lv = next_lv;
            }
        }
    }

    // UAX#9 L2: reverse runs from max level down to min odd level
    if !runs.is_empty() {
        let max_lv = runs.iter().map(|r| r.level).max().unwrap_or(0);
        let min_odd = runs
            .iter()
            .map(|r| r.level)
            .filter(|&n| n & 1 == 1)
            .min()
            .unwrap_or(max_lv | 1);
        for threshold in (min_odd..=max_lv).rev() {
            let mut i = 0;
            while i < runs.len() {
                if runs[i].level >= threshold {
                    let mut j = i + 1;
                    while j < runs.len() && runs[j].level >= threshold {
                        j += 1;
                    }
                    runs[i..j].reverse();
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
    }

    line.visual_segments.clear();
    for run in &runs {
        let seg_byte_start = if run.start < char_count {
            char_bytes[run.start]
        } else {
            line_text.len()
        };
        let seg_byte_end = if run.end < char_count {
            char_bytes[run.end]
        } else {
            line_text.len()
        };
        if seg_byte_end <= seg_byte_start {
            continue;
        }
        line.visual_segments.push(VisualSegment {
            logical_start: byte_start + seg_byte_start,
            length: seg_byte_end - seg_byte_start,
            level: run.level,
            x: 0.0,
            width: 0.0,
        });
    }
}

fn resolve_bidi_override_line(
    line_text: &str,
    line: &mut LayoutLine,
    byte_start: usize,
    para_dir: Direction,
) {
    line.visual_segments.clear();

    if para_dir == Direction::LTR {
        return;
    }

    let mut chars: Vec<(usize, usize)> = line_text
        .char_indices()
        .map(|(idx, ch)| (idx, idx + ch.len_utf8()))
        .collect();
    chars.reverse();
    for (start, end) in chars {
        if end <= start {
            continue;
        }
        line.visual_segments.push(VisualSegment {
            logical_start: byte_start + start,
            length: end - start,
            level: 1,
            x: 0.0,
            width: 0.0,
        });
    }
}

// ─── Break opportunities ──────────────────────────────────────────────────────

/// Return true if it is valid to break a line after `ch`.
/// Mirrors LayoutEngine::IsBreakableAfter in C++.
pub fn is_breakable_after(ch: char) -> bool {
    match ch {
        ' ' | '\t' | '-' => true,
        '\u{00AD}' => true,                              // soft hyphen
        '\u{2013}' | '\u{2014}' => true,                 // en-dash, em-dash
        c if c >= '\u{3000}' && c <= '\u{9FFF}' => true, // CJK / ideographic
        c if c >= '\u{F900}' && c <= '\u{FAFF}' => true, // CJK compatibility
        _ => false,
    }
}

// ─── Check for <br> at an inline item position ───────────────────────────────

/// Return true if `items[idx]` is a forced line break.
/// In the Rust system <br> emits `InlineItemKind::Break` directly.
pub fn is_break_item(items: &[InlineItem], idx: usize) -> bool {
    idx < items.len() && matches!(items[idx].kind, InlineItemKind::Break)
}

// ─── Text measurement ─────────────────────────────────────────────────────────

/// Measure the advance width of a text segment at a given font size.
///
/// For now uses the same character-width approximation as `measure_text_width`
/// in inline_layout.rs, plus letter-spacing.  cosmic-text integration would
/// give pixel-perfect results but requires a running FontSystem.
pub fn measure_text(
    text: &str,
    start: usize,
    length: usize,
    font_px: f32,
    letter_spacing_px: f32,
    font_system: Option<&mut cosmic_text::FontSystem>,
) -> f32 {
    if length == 0 {
        return 0.0;
    }
    let end = (start + length).min(text.len());
    // Ensure valid UTF-8 boundary
    let end = floor_char_boundary(text, end);
    if end <= start {
        return 0.0;
    }
    let snippet = &text[start..end];
    let char_count = snippet.chars().count();
    measure_text_width(snippet, font_px, font_system) + letter_spacing_px * char_count as f32
}

/// Measure a rendered text run with CSS Text spacing applied.
pub fn measure_text_with_spacing(
    text: &str,
    font_px: f32,
    letter_spacing_px: f32,
    word_spacing_px: f32,
    font_system: Option<&mut cosmic_text::FontSystem>,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let char_count = text.chars().count() as f32;
    let word_separators = text.chars().filter(|c| c.is_whitespace()).count() as f32;
    measure_text_width(text, font_px, font_system)
        + letter_spacing_px * char_count
        + word_spacing_px * word_separators
}

/// Advance width of a single character including letter-spacing.
/// `tab_size` is the CSS tab-size property (number of space widths per tab, default 8).
pub fn measure_char_width(ch: char, font_px: f32, letter_spacing_px: f32) -> f32 {
    measure_char_width_ts(ch, font_px, letter_spacing_px, 8)
}

pub fn measure_char_width_ts(ch: char, font_px: f32, letter_spacing_px: f32, tab_size: i32) -> f32 {
    let base = font_px * 0.55;
    let space_w = base * 0.35;
    let advance = if ch == '\t' {
        space_w * (tab_size.max(1)) as f32
    } else if "iIlj1!|:;,.'`".contains(ch) {
        base * 0.45
    } else if "mwMW".contains(ch) {
        base * 1.20
    } else if ch == ' ' {
        space_w
    } else {
        base
    };
    advance + letter_spacing_px
}

// ─── Line breaking ────────────────────────────────────────────────────────────

/// Find the byte offset within `text` where the line should break so that
/// the content from `line_start` fits within `available_width`.
///
/// Returns a byte offset in `[line_start, line_end]`.
/// Mirrors LayoutEngine::FindLineBreak in C++.
pub fn find_line_break(
    text: &str,
    line_start: usize,
    line_end: usize,
    available_width: f32,
    font_px: f32,
    letter_spacing: f32,
    word_break: WordBreak,
    overflow_wrap: OverflowWrap,
) -> usize {
    if line_start >= line_end {
        return line_end;
    }

    let end = line_end.min(text.len());
    let end = floor_char_boundary(text, end);
    if end <= line_start {
        return line_end;
    }

    // Fast path: everything fits
    let total_w = measure_text(
        text,
        line_start,
        end - line_start,
        font_px,
        letter_spacing,
        None,
    );
    if total_w <= available_width {
        return line_end;
    }

    // Find first character position that exceeds the available width
    let snippet = &text[line_start..end];
    let mut running_w = 0.0f32;
    let mut overflow_byte = end; // default: overflow at the very end

    for (byte_off, ch) in snippet.char_indices() {
        let cw = measure_char_width(ch, font_px, letter_spacing);
        if running_w + cw > available_width {
            overflow_byte = line_start + byte_off;
            break;
        }
        running_w += cw;
    }

    // word-break: break-all — any character is a valid break point
    if word_break == WordBreak::BreakAll {
        return overflow_byte.max(advance_one_char(text, line_start));
    }

    // Normal: scan backwards from overflow point for the last break opportunity
    let pre_overflow = &text[line_start..overflow_byte];
    let mut break_after_byte: Option<usize> = None;

    for (byte_off, ch) in pre_overflow.char_indices().rev() {
        if is_breakable_after(ch) {
            // break *after* this char
            break_after_byte = Some(line_start + byte_off + ch.len_utf8());
            break;
        }
    }

    if let Some(bp) = break_after_byte {
        return bp;
    }

    // No normal break found — check overflow-wrap / word-break policies
    if overflow_wrap == OverflowWrap::BreakWord
        || overflow_wrap == OverflowWrap::Anywhere
        || word_break == WordBreak::BreakWord
    {
        return overflow_byte.max(advance_one_char(text, line_start));
    }

    // word-break: keep-all — let the line overflow rather than break mid-word
    if word_break == WordBreak::KeepAll {
        return line_end;
    }

    // Default fallback: force break at the overflow character
    overflow_byte.max(advance_one_char(text, line_start))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Return the byte offset one UTF-8 character past `start` in `text`.
fn advance_one_char(text: &str, start: usize) -> usize {
    if start >= text.len() {
        return text.len();
    }
    let ch_len = text[start..]
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1);
    start + ch_len
}

/// Move `idx` back to the nearest UTF-8 character boundary.
pub fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}
