//! Text-input state: the selection direction, the value, and the key handler
//! that edits both.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

fn adjacent_grapheme_offset(value: &str, cursor: usize, forward: bool) -> usize {
    let byte = value
        .char_indices()
        .nth(cursor)
        .map_or(value.len(), |(at, _)| at);
    let boundary = crate::dom::adjacent_grapheme_boundary(value, byte, forward);
    value[..boundary].chars().count()
}

/// Returns true if this element is a text-editable form input.
/// What put this element in the **top layer** (CSS Position §6, HTML §4.11.4
/// and §6.12).
///
/// The top layer is one ordered list on `Document`; this is the per-node half,
/// written ONLY by `add_to_top_layer`/`remove_from_top_layer` so the two
/// cannot drift. It is what makes `:modal` and `:popover-open` answerable —
/// before it, modality was "did someone write `position: fixed`", which no
/// selector can ask about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopLayerKind {
    /// A `<dialog>` opened with `showModal()`. Matches `:modal`.
    ModalDialog,
    /// A showing popover. Matches `:popover-open`.
    Popover,
}

/// `selectionDirection` — the third state that the cursor/anchor pair cannot
/// hold (HTML §4.10.19.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionDirection {
    /// The platform default: a selection with no direction of its own.
    #[default]
    None,
    Forward,
    Backward,
}

impl SelectionDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            SelectionDirection::None => "none",
            SelectionDirection::Forward => "forward",
            SelectionDirection::Backward => "backward",
        }
    }

    /// Parse the IDL value. An unrecognised string is **not** an error — the
    /// IDL type is a plain `DOMString`, so `selectionDirection = "bogus"`
    /// stores the platform default rather than throwing (measured: Chrome
    /// answers `"none"`).
    pub fn parse(value: &str) -> SelectionDirection {
        match value {
            "forward" => SelectionDirection::Forward,
            "backward" => SelectionDirection::Backward,
            _ => SelectionDirection::None,
        }
    }
}

/// Does the **text selection API** apply to this control (HTML §4.10.19.3)?
///
/// ⛔ Not [`is_text_input`], and not `ValueMode::Value` either. `number`,
/// `date`, `range` and `color` all hold a value and all accept typing, and
/// Chrome answers `null` for `selectionStart` on every one of them: the
/// selection API is defined only over `text`, `search`, `url`, `tel` and
/// `password`, plus `<textarea>`. An input with no `type` — or an unrecognised
/// one — is in the Text state, so it is supported (measured).
pub fn selection_api_applies(node: &WebCore) -> bool {
    match node.tag.as_str() {
        "textarea" => true,
        "input" => {
            let t = node
                .attributes
                .get("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".into());
            match t.as_str() {
                "text" | "search" | "url" | "tel" | "password" => true,
                // Every OTHER named state is unsupported; anything the parser
                // does not recognise falls back to Text, which is.
                "hidden" | "email" | "number" | "date" | "month" | "week" | "time"
                | "datetime-local" | "range" | "color" | "checkbox" | "radio" | "file"
                | "submit" | "image" | "reset" | "button" => false,
                _ => true,
            }
        }
        _ => false,
    }
}

pub fn is_text_input(node: &WebCore) -> bool {
    match node.tag.as_str() {
        "textarea" => true,
        "input" => {
            let t = node
                .attributes
                .get("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            matches!(
                t.as_str(),
                "text"
                    | "password"
                    | "email"
                    | "search"
                    | "url"
                    | "tel"
                    | "number"
                    | "date"
                    | "month"
                    | "week"
                    | "time"
                    | "datetime-local"
            )
        }
        _ => false,
    }
}

pub(crate) fn input_uses_minimum_normal_line_height(input_type: &str) -> bool {
    ["text", "search", "password", "email", "url", "tel"]
        .iter()
        .any(|kind| input_type.eq_ignore_ascii_case(kind))
}

/// A form control's **value** (HTML §4.10.18.1).
///
/// The single read point for every consumer — the paint path, form submission,
/// the key handler. It answers the VALUE, which is `value_state` once anything
/// has set it and the `value` content attribute (the default value) until then.
pub fn input_value(node: &WebCore) -> String {
    // The PRESENCE of a state string decides, not `dirty_value`. The two are
    // not the same question: sanitization seeds a value with the dirty flag
    // still down, and a control set to the empty string holds the empty string
    // — `Some("")` must not fall back to the default the way `None` does.
    // `dirty_value` answers a different question, for the reset algorithm.
    if let Some(v) = &node.value_state {
        return v.clone();
    }
    if node.tag == "textarea" {
        // Textarea value is in child text nodes
        node.children
            .iter()
            .filter(|c| c.tag == "#text")
            .map(|c| c.text.as_str())
            .collect()
    } else {
        node.attributes.get("value").cloned().unwrap_or_default()
    }
}

/// Process a key event on a focused form input. Returns true if the value changed.
fn form_input_maxlength(node: &WebCore) -> Option<usize> {
    node.attributes
        .get("maxlength")
        .filter(|_| {
            node.tag == "textarea"
                || crate::html::forms::supports_text_length_constraints(
                    node.attributes.get("type").map_or("text", String::as_str),
                )
        })
        .and_then(|value| crate::html::forms::parse_non_negative_integer(value))
        .map(|length| length as usize)
}

pub(crate) fn replace_form_input_selection(node: &mut WebCore, replacement: &str) -> bool {
    if !is_text_input(node)
        || node.attributes.contains_key("disabled")
        || node.attributes.contains_key("readonly")
    {
        return false;
    }
    let mut value = input_value(node);
    let len = value.chars().count();
    let start = node.input_cursor.min(node.input_sel_anchor).min(len);
    let end = node.input_cursor.max(node.input_sel_anchor).min(len);
    let byte_start = value
        .char_indices()
        .nth(start)
        .map_or(value.len(), |(at, _)| at);
    let byte_end = value
        .char_indices()
        .nth(end)
        .map_or(value.len(), |(at, _)| at);
    let retained_units =
        value[..byte_start].encode_utf16().count() + value[byte_end..].encode_utf16().count();
    let mut available =
        form_input_maxlength(node).map_or(usize::MAX, |limit| limit.saturating_sub(retained_units));
    let normalized = replacement.replace("\r\n", "\n").replace('\r', "\n");
    let mut inserted = String::new();
    for ch in normalized.chars() {
        if node.tag != "textarea" && ch == '\n' {
            continue;
        }
        if ch.len_utf16() > available {
            break;
        }
        inserted.push(ch);
        available -= ch.len_utf16();
    }
    if start == end && inserted.is_empty() {
        return false;
    }
    value.replace_range(byte_start..byte_end, &inserted);
    node.input_cursor = start + inserted.chars().count();
    node.input_sel_anchor = node.input_cursor;
    node.input_sel_direction = SelectionDirection::None;
    node.value_state = Some(value);
    node.dirty_value = true;
    if node.tag == "textarea" || node.style.field_sizing.eq_ignore_ascii_case("content") {
        node.layout.layout_dirty = true;
    }
    true
}

pub fn process_form_input_key(
    node: &mut WebCore,
    key_code: u32,
    ch: Option<char>,
    ctrl: bool,
    shift: bool,
) -> bool {
    if !is_text_input(node) {
        return false;
    }
    // Disabled elements don't accept any input
    if node.attributes.contains_key("disabled") {
        return false;
    }
    // Readonly elements allow cursor movement but not content changes
    let is_readonly = node.attributes.contains_key("readonly");
    let is_textarea = node.tag == "textarea";

    let mut value = input_value(node);
    let len = value.chars().count();
    let cursor = node.input_cursor.min(len);
    let anchor = node.input_sel_anchor.min(len);
    let has_selection = cursor != anchor;
    let sel_start = cursor.min(anchor);
    let sel_end = cursor.max(anchor);
    let mut new_cursor = cursor;
    let mut changed = false;
    let maxlength = form_input_maxlength(node);
    // Selection offsets are scalar indices; HTML length limits use UTF-16 units.
    let insertion_fits = |inserted: char| {
        maxlength.is_none_or(|limit| {
            value.encode_utf16().count()
                - value
                    .chars()
                    .skip(sel_start)
                    .take(sel_end - sel_start)
                    .map(char::len_utf16)
                    .sum::<usize>()
                + inserted.len_utf16()
                <= limit
        })
    };
    let insertion_allowed = match (key_code, ch) {
        (13, _) => insertion_fits('\n'),
        (_, Some(ch)) => insertion_fits(ch),
        _ => true,
    };

    // Ctrl+A: select all
    if ctrl && (key_code == 65 || ch == Some('a') || ch == Some('A')) {
        node.input_sel_anchor = 0;
        node.input_cursor = len;
        node.input_sel_direction = SelectionDirection::Forward;
        return true; // cursor moved, no content change
    }
    if is_readonly
        && (matches!(key_code, 8 | 13 | 46) || ch.is_some_and(|ch| !ch.is_control() && !ctrl))
    {
        return false;
    }

    if !matches!(key_code, 8 | 13 | 35..=37 | 39 | 46)
        && !ch.is_some_and(|ch| !ch.is_control() && !ctrl)
    {
        return false;
    }

    // Helper: delete selected range
    let delete_selection = |value: &mut String, sel_s: usize, sel_e: usize| -> usize {
        let byte_s = value
            .char_indices()
            .nth(sel_s)
            .map(|(i, _)| i)
            .unwrap_or(value.len());
        let byte_e = value
            .char_indices()
            .nth(sel_e)
            .map(|(i, _)| i)
            .unwrap_or(value.len());
        value.replace_range(byte_s..byte_e, "");
        sel_s
    };

    match key_code {
        8 => {
            // Backspace
            if !is_readonly {
                if has_selection {
                    new_cursor = delete_selection(&mut value, sel_start, sel_end);
                    changed = true;
                } else if cursor > 0 {
                    let previous = adjacent_grapheme_offset(&value, cursor, false);
                    let byte_pos = value
                        .char_indices()
                        .nth(previous)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let byte_end = value
                        .char_indices()
                        .nth(cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(value.len());
                    value.replace_range(byte_pos..byte_end, "");
                    new_cursor = previous;
                    changed = true;
                }
            }
        }
        46 => {
            // Delete
            if !is_readonly {
                if has_selection {
                    new_cursor = delete_selection(&mut value, sel_start, sel_end);
                    changed = true;
                } else if cursor < len {
                    let byte_pos = value
                        .char_indices()
                        .nth(cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(value.len());
                    let byte_end = value
                        .char_indices()
                        .nth(adjacent_grapheme_offset(&value, cursor, true))
                        .map(|(i, _)| i)
                        .unwrap_or(value.len());
                    value.replace_range(byte_pos..byte_end, "");
                    changed = true;
                }
            }
        }
        37 => {
            // Left arrow
            if has_selection && !shift {
                new_cursor = sel_start;
            } else if cursor > 0 {
                new_cursor = adjacent_grapheme_offset(&value, cursor, false);
            }
        }
        39 => {
            // Right arrow
            if has_selection && !shift {
                new_cursor = sel_end;
            } else if cursor < len {
                new_cursor = adjacent_grapheme_offset(&value, cursor, true);
            }
        }
        36 => {
            // Home
            new_cursor = if is_textarea && !ctrl {
                value
                    .chars()
                    .take(cursor)
                    .enumerate()
                    .filter_map(|(i, ch)| (ch == '\n').then_some(i + 1))
                    .last()
                    .unwrap_or(0)
            } else {
                0
            };
        }
        35 => {
            // End
            new_cursor = if is_textarea && !ctrl {
                cursor
                    + value
                        .chars()
                        .skip(cursor)
                        .position(|ch| ch == '\n')
                        .unwrap_or(len - cursor)
            } else {
                len
            };
        }
        13 => {
            // Enter
            if is_textarea && !is_readonly {
                if has_selection {
                    new_cursor = delete_selection(&mut value, sel_start, sel_end);
                    changed = true;
                }
                if insertion_allowed {
                    let byte_pos = value
                        .char_indices()
                        .nth(new_cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(value.len());
                    value.insert(byte_pos, '\n');
                    new_cursor += 1;
                    changed = true;
                }
            }
        }
        _ => {
            // Character input
            if let Some(c) = ch {
                if !c.is_control() && !is_readonly && !ctrl {
                    // Delete selection first if any
                    if has_selection {
                        new_cursor = delete_selection(&mut value, sel_start, sel_end);
                        changed = true;
                    }
                    if insertion_allowed {
                        let byte_pos = value
                            .char_indices()
                            .nth(new_cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(value.len());
                        value.insert(byte_pos, c);
                        new_cursor += 1;
                        changed = true;
                    }
                }
            }
        }
    }

    node.input_cursor = new_cursor;
    let extending = shift && matches!(key_code, 35..=37 | 39);
    node.input_sel_anchor = if extending { anchor } else { new_cursor };
    node.input_sel_direction = if node.input_cursor < node.input_sel_anchor {
        SelectionDirection::Backward
    } else if node.input_cursor > node.input_sel_anchor {
        SelectionDirection::Forward
    } else {
        SelectionDirection::None
    };

    if changed {
        // Typing sets the VALUE and raises the dirty value flag (HTML
        // §4.10.18.1). It does not touch the `value` attribute, nor a
        // `<textarea>`'s child text — those ARE the default value, which is
        // what a form reset restores and what the serializer round-trips.
        node.value_state = Some(value);
        node.dirty_value = true;
        if is_textarea || node.style.field_sizing.eq_ignore_ascii_case("content") {
            node.layout.layout_dirty = true;
        }
    }

    changed || key_code == 37 || key_code == 39 || key_code == 36 || key_code == 35
}
