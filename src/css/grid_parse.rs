//! Grid track-list parsing.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Grid Track Parsers ───────────────────────────────────────────────────────

/// Parse a single grid track size token.
pub fn parse_single_track(v: &str) -> GridTrackSize {
    let v = v.trim();
    if v == "auto" {
        return GridTrackSize::auto();
    }
    if v == "min-content" {
        return GridTrackSize {
            kind: GridTrackKind::MinContent,
            ..Default::default()
        };
    }
    if v == "max-content" {
        return GridTrackSize {
            kind: GridTrackKind::MaxContent,
            ..Default::default()
        };
    }
    if v.ends_with("fr") {
        let fr: f32 = v[..v.len() - 2].parse().unwrap_or(1.0);
        return GridTrackSize::fr(fr);
    }
    if v.ends_with('%') {
        let pct: f32 = v[..v.len() - 1].parse().unwrap_or(0.0);
        return GridTrackSize::percent(pct);
    }
    if v.ends_with("px") {
        let px: f32 = v[..v.len() - 2].parse().unwrap_or(0.0);
        return GridTrackSize::fixed(px);
    }
    if v.starts_with("calc(") {
        let len = parse_length(v);
        return GridTrackSize {
            kind: GridTrackKind::Calc,
            calc_length: Some(len),
            ..Default::default()
        };
    }
    if v.starts_with("minmax(") {
        let inner = &v[7..v.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ',').collect();
        if parts.len() == 2 {
            let min_t = parse_single_track(parts[0].trim());
            let max_t = parse_single_track(parts[1].trim());
            return GridTrackSize {
                kind: GridTrackKind::MinMax,
                value: 0.0,
                min_kind: min_t.kind,
                min_value: min_t.value,
                max_kind: max_t.kind,
                max_value: max_t.value,
                calc_length: None,
            };
        }
    }
    if v.starts_with("fit-content(") {
        let inner = &v[12..v.len() - 1];
        let t = parse_single_track(inner.trim());
        return GridTrackSize {
            kind: GridTrackKind::FitContent,
            value: t.value,
            max_kind: t.kind,
            max_value: t.value,
            ..Default::default()
        };
    }
    // unitless number → px
    if let Ok(n) = v.parse::<f32>() {
        return GridTrackSize::fixed(n);
    }
    GridTrackSize::auto()
}

/// Parse a grid-template-columns/rows value into Vec<GridTrackSize>.
/// Also extracts named grid lines into line_names: name → Vec<line_index> (0-based).
/// Handles repeat(), minmax(), fr, px, %, auto, min-content, max-content.
/// auto_repeat_cols receives any auto-fill/auto-fit tracks.
pub fn parse_track_list(v: &str, auto_repeat_cols: &mut Vec<GridTrackSize>) -> Vec<GridTrackSize> {
    let mut line_names = std::collections::HashMap::new();
    parse_track_list_with_names(v, auto_repeat_cols, &mut line_names)
}

/// Like parse_track_list but also populates a name→line-number map.
pub fn parse_track_list_with_names(
    v: &str,
    auto_repeat_cols: &mut Vec<GridTrackSize>,
    line_names: &mut std::collections::HashMap<String, Vec<usize>>,
) -> Vec<GridTrackSize> {
    if v.is_empty() {
        return Vec::new();
    }
    if v.trim() == "subgrid" {
        return vec![GridTrackSize::subgrid()];
    }
    let tokens = tokenize_track_list(v);
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].trim();
        if t.starts_with('[') && t.ends_with(']') {
            // Named line: [name1 name2 ...]
            let inner = &t[1..t.len() - 1];
            let line_idx = result.len(); // line is BEFORE the next track
            for name in inner.split_whitespace() {
                line_names
                    .entry(name.to_string())
                    .or_insert_with(Vec::new)
                    .push(line_idx);
            }
        } else if t.starts_with("repeat(") || (i + 1 < tokens.len() && t == "repeat") {
            let repeat_str = if t.starts_with("repeat(") && t.ends_with(')') {
                t.to_string()
            } else {
                t.to_string()
            };
            // Strip "repeat(" prefix and single trailing ")" — not trim which strips multiple
            let stripped = repeat_str.strip_prefix("repeat(").unwrap_or(&repeat_str);
            let inner = stripped.strip_suffix(')').unwrap_or(stripped);
            // Find top-level comma (not inside parens)
            let comma = {
                let mut depth = 0;
                let mut pos = None;
                for (i, ch) in inner.chars().enumerate() {
                    if ch == '(' {
                        depth += 1;
                    }
                    if ch == ')' {
                        depth -= 1;
                    }
                    if ch == ',' && depth == 0 {
                        pos = Some(i);
                        break;
                    }
                }
                pos.unwrap_or(0)
            };
            let count_str = inner[..comma].trim();
            let track_str = inner[comma + 1..].trim();
            let track = parse_single_track(track_str);
            if count_str == "auto-fill" || count_str == "auto-fit" {
                auto_repeat_cols.push(track.clone());
            } else {
                let count = if let Ok(n) = count_str.parse::<usize>() {
                    n
                } else {
                    // Handle calc() in repeat count, e.g. repeat(calc(5 - 1), ...)
                    let resolved = parse_length(count_str).resolve(16.0, 0.0, 16.0);
                    if resolved > 0.0 { resolved as usize } else { 1 }
                };
                for _ in 0..count {
                    result.push(track.clone());
                }
            }
        } else if !t.is_empty() {
            result.push(parse_single_track(t));
        }
        i += 1;
    }
    result
}

/// Tokenize a track list, keeping repeat(...) and [...] as single tokens.
fn tokenize_track_list(v: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    for ch in v.chars() {
        match ch {
            '[' => {
                // If there's content before '[', push it as a separate token
                if bracket_depth == 0 && paren_depth == 0 {
                    let s = current.trim().to_string();
                    if !s.is_empty() {
                        tokens.push(s);
                    }
                    current = String::new();
                }
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                if bracket_depth > 0 {
                    bracket_depth -= 1;
                }
                current.push(ch);
                if bracket_depth == 0 && paren_depth == 0 {
                    tokens.push(current.trim().to_string());
                    current = String::new();
                }
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                }
                current.push(ch);
                if paren_depth == 0 && bracket_depth == 0 {
                    tokens.push(current.trim().to_string());
                    current = String::new();
                }
            }
            ' ' | '\t' | '\n' if paren_depth == 0 && bracket_depth == 0 => {
                let s = current.trim().to_string();
                if !s.is_empty() {
                    tokens.push(s);
                }
                current = String::new();
            }
            _ => {
                current.push(ch);
            }
        }
    }
    let s = current.trim().to_string();
    if !s.is_empty() {
        tokens.push(s);
    }
    tokens
}

/// Parse grid-template-areas string.
/// Input: `"a a b" "a a b" "c c b"` → Vec<Vec<String>>
pub fn parse_grid_template_areas(v: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    // Each quoted string is a row
    let mut rest = v.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.starts_with('"') || rest.starts_with('\'') {
            let q = rest.chars().next().unwrap();
            let end = rest[1..].find(q).unwrap_or(rest.len() - 1);
            let row_str = &rest[1..end + 1];
            let cells: Vec<String> = row_str.split_whitespace().map(|s| s.to_string()).collect();
            if !cells.is_empty() {
                rows.push(cells);
            }
            rest = &rest[end + 2..];
        } else {
            break;
        }
    }
    rows
}

/// Parse a CSS grid line value.
/// Returns: (numeric_value, named_reference)
/// numeric: positive = explicit 1-based line, 0 = auto,
/// negative > -10000 = negative line number, <= -10000 = span (encoded).
/// named: non-empty if referencing a named line like "content-start" or area "content".
pub fn parse_grid_line_named(v: &str) -> (i32, String) {
    let v = v.trim();
    if v == "auto" || v.is_empty() {
        return (0, String::new());
    }
    if v.starts_with("span ") {
        let rest = v[5..].trim();
        let n: i32 = rest.parse().unwrap_or(1);
        return (-(n + 10000), String::new());
    }
    if let Ok(n) = v.parse::<i32>() {
        return (n, String::new());
    }
    // Named line reference (e.g. "content", "content-start", "title-end")
    (0, v.to_string())
}

/// Convenience wrapper for parse_grid_line_named that discards the name.
pub fn parse_grid_line(v: &str) -> i32 {
    parse_grid_line_named(v).0
}
