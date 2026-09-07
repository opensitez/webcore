//! `@font-face` declarations.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── @font-face declaration ───────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct FontFaceDecl {
    pub family: String,
    pub src: String,
    pub sources: Vec<FontFaceSource>,
    pub weight: Option<String>,
    pub style: Option<String>,
    pub stretch: Option<String>,
    pub display: Option<String>,
    pub unicode_range: Option<String>,
    pub size_adjust: Option<String>,
    pub ascent_override: Option<String>,
    pub descent_override: Option<String>,
    pub line_gap_override: Option<String>,
    pub feature_settings: Option<String>,
    pub variation_settings: Option<String>,
    pub language_override: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontFaceSourceKind {
    Url(String),
    Local(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFaceSource {
    pub kind: FontFaceSourceKind,
    pub formats: Vec<String>,
    pub techs: Vec<String>,
}

/// Extract a file path from a CSS url("...") or local("...") value.
pub fn extract_url_path(src: &str) -> String {
    let src = src.trim();
    // Strip url("...") or url('...')
    let inner = if let Some(s) = src.strip_prefix("url(") {
        s.trim_end_matches(')')
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
    } else if let Some(s) = src.strip_prefix("local(") {
        s.trim_end_matches(')')
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
    } else {
        src.trim_matches('"').trim_matches('\'')
    };
    inner.to_string()
}

pub fn parse_font_face_sources(src: &str) -> Vec<FontFaceSource> {
    split_font_sources(src)
        .into_iter()
        .filter_map(parse_font_face_source)
        .collect()
}

fn parse_font_face_source(source: &str) -> Option<FontFaceSource> {
    let source = source.trim();
    let (kind, rest) = if let Some((value, rest)) = consume_function(source, "url") {
        (
            FontFaceSourceKind::Url(unquote_css_string(value.trim())),
            rest,
        )
    } else if let Some((value, rest)) = consume_function(source, "local") {
        (
            FontFaceSourceKind::Local(unquote_css_string(value.trim())),
            rest,
        )
    } else {
        return None;
    };
    let mut formats = Vec::new();
    let mut techs = Vec::new();
    let mut rest = rest.trim();
    while !rest.is_empty() {
        if let Some((value, next)) = consume_function(rest, "format") {
            formats.extend(
                split_font_descriptor_list(value)
                    .into_iter()
                    .map(|v| unquote_css_string(v.trim()).to_ascii_lowercase()),
            );
            rest = next.trim();
        } else if let Some((value, next)) = consume_function(rest, "tech") {
            techs.extend(
                split_font_descriptor_list(value)
                    .into_iter()
                    .map(|v| v.trim().to_ascii_lowercase()),
            );
            rest = next.trim();
        } else {
            break;
        }
    }
    Some(FontFaceSource {
        kind,
        formats,
        techs,
    })
}

fn split_font_sources(src: &str) -> Vec<&str> {
    split_top_level(src, ',')
}

fn split_font_descriptor_list(src: &str) -> Vec<&str> {
    split_top_level(src, ',')
}

fn split_top_level(src: &str, separator: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut start = 0usize;
    let mut escape = false;
    for (i, ch) in src.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == separator && depth == 0 => {
                out.push(src[start..i].trim());
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = src[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn consume_function<'a>(src: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let src = src.trim_start();
    if src.len() < name.len() || !src[..name.len()].eq_ignore_ascii_case(name) {
        return None;
    }
    let after_name = src[name.len()..].trim_start();
    if !after_name.starts_with('(') {
        return None;
    }
    let mut quote = None;
    let mut escape = false;
    let mut depth = 0usize;
    for (i, ch) in after_name.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&after_name[1..i], &after_name[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn unquote_css_string(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let first = value.as_bytes()[0];
        let last = value.as_bytes()[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\'", "'");
        }
    }
    value.to_string()
}
