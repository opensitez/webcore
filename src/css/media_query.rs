//! Media query evaluation.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSchemePreference {
    Light,
    Dark,
}

static COLOR_SCHEME_PREFERENCE: AtomicU8 = AtomicU8::new(0);

pub fn set_color_scheme_preference(pref: ColorSchemePreference) {
    COLOR_SCHEME_PREFERENCE.store(
        match pref {
            ColorSchemePreference::Light => 0,
            ColorSchemePreference::Dark => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn color_scheme_preference() -> ColorSchemePreference {
    match COLOR_SCHEME_PREFERENCE.load(Ordering::Relaxed) {
        1 => ColorSchemePreference::Dark,
        _ => ColorSchemePreference::Light,
    }
}

// ─── Media Query Evaluator ───────────────────────────────────────────────────

/// Evaluate a CSS @media condition string.
/// Returns true if the condition matches the given viewport dimensions.
/// `condition` is the full text after "@media" (trimmed).
pub fn evaluate_media(condition: &str, vw: f32, vh: f32) -> bool {
    let cond = condition.trim();
    if cond.is_empty() {
        return true;
    }

    // ⛔ `only` is a no-op qualifier — `only print` IS `print` (Media Queries
    // §3). Leaving it attached meant the string matched no known media type
    // and fell through to the permissive default, so a print stylesheet
    // written `@media only print` (or `<link media="only print">`) applied to
    // the SCREEN: `display: block` everywhere, columns and floats dropped,
    // navigation hidden. A page styled that way renders as one long column,
    // exactly as if it had been printed.
    let cond = match cond.len() >= 5 && cond[..5].eq_ignore_ascii_case("only ") {
        true => cond[5..].trim_start(),
        false => cond,
    };
    if cond.is_empty() {
        return true;
    }

    // Handle comma-separated list at top level (OR semantics)
    // We first split on `and`/`or` outside parens, then check named types.
    // But comma is always OR at the top level.
    {
        let mut depth = 0usize;
        let bytes = cond.as_bytes();
        let mut comma_pos: Option<usize> = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                b',' if depth == 0 => {
                    comma_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }
        if let Some(pos) = comma_pos {
            let left = &cond[..pos];
            let right = &cond[pos + 1..];
            return evaluate_media(left.trim(), vw, vh) || evaluate_media(right.trim(), vw, vh);
        }
    }

    // Handle `not` prefix (before `and`/`or` splitting)
    // `not` folds case like every other CSS keyword.
    if cond.len() >= 4 && cond.as_bytes()[..4].eq_ignore_ascii_case(b"not ") {
        return !evaluate_media(cond[4..].trim(), vw, vh);
    }

    // Handle `and` combinator outside parens
    if let Some(idx) = find_keyword_outside_parens(cond, " and ") {
        let left = &cond[..idx];
        let right = &cond[idx + 5..];
        return evaluate_media(left.trim(), vw, vh) && evaluate_media(right.trim(), vw, vh);
    }

    // Handle `or` combinator outside parens
    if let Some(idx) = find_keyword_outside_parens(cond, " or ") {
        let left = &cond[..idx];
        let right = &cond[idx + 4..];
        return evaluate_media(left.trim(), vw, vh) || evaluate_media(right.trim(), vw, vh);
    }

    // Named media types (no parens)
    if !cond.starts_with('(') {
        return match cond.to_ascii_lowercase().as_str() {
            "screen" | "all" => true,
            // Everything that is not a screen. The deprecated types are listed
            // because they must not match either — a `<link media="handheld">`
            // sheet applying to a desktop render is the same failure as the
            // print one.
            "print" | "speech" | "aural" | "braille" | "embossed" | "handheld" | "projection"
            | "tty" | "tv" => false,
            // Anything unrecognised is more likely a parse artefact than a
            // real media type, so it stays permissive rather than silently
            // dropping a stylesheet.
            _ => true,
        };
    }

    if cond.starts_with('(') && cond.ends_with(')') {
        let inner = cond[1..cond.len() - 1].trim();
        let is_parenthesized_logical = inner.starts_with('(')
            || (inner.len() >= 4 && inner.as_bytes()[..4].eq_ignore_ascii_case(b"not "))
            || find_keyword_outside_parens(inner, " and ").is_some()
            || find_keyword_outside_parens(inner, " or ").is_some();
        if is_parenthesized_logical {
            return evaluate_media(inner, vw, vh);
        }
    }

    // Strip outer parens for feature queries
    let inner = if cond.starts_with('(') && cond.ends_with(')') {
        &cond[1..cond.len() - 1]
    } else {
        cond
    };
    let lower = inner.to_ascii_lowercase();
    let lower = lower.trim();

    if let Some(rest) = lower.strip_prefix("min-width:") {
        return vw >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-width:") {
        return vw <= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("min-height:") {
        return vh >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-height:") {
        return vh <= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("orientation:") {
        return match rest.trim() {
            "landscape" => vw > vh,
            "portrait" => vh >= vw,
            _ => true,
        };
    }
    if let Some(rest) = lower.strip_prefix("prefers-color-scheme:") {
        return match rest.trim() {
            "light" => color_scheme_preference() == ColorSchemePreference::Light,
            "dark" => color_scheme_preference() == ColorSchemePreference::Dark,
            _ => false,
        };
    }
    if let Some(rest) = lower.strip_prefix("hover:") {
        return match rest.trim() {
            "hover" => true,
            "none" => false,
            _ => true,
        };
    }
    if let Some(rest) = lower.strip_prefix("pointer:") {
        return match rest.trim() {
            "fine" => true,
            "coarse" | "none" => false,
            _ => true,
        };
    }
    // ⛔ ANSWER THE PREFERENCE FEATURES. Falling through to the fail-open
    // default made a feature match BOTH of its mutually exclusive values, so
    // `(prefers-reduced-motion: reduce)` and `(no-preference)` both applied and
    // source order decided. This engine has no OS preference channel, so it
    // reports the defaults of an ordinary desktop UA — which is a real answer,
    // not a guess, and stops the self-contradiction.
    for (feature, matching) in [
        ("prefers-reduced-motion:", "no-preference"),
        ("prefers-contrast:", "no-preference"),
        ("prefers-reduced-transparency:", "no-preference"),
        ("prefers-reduced-data:", "no-preference"),
        ("forced-colors:", "none"),
        ("inverted-colors:", "none"),
        ("any-hover:", "hover"),
        ("any-pointer:", "fine"),
        ("scripting:", "none"),
        ("update:", "fast"),
    ] {
        if let Some(rest) = lower.strip_prefix(feature) {
            return rest.trim() == matching;
        }
    }

    // `<number>dppx | <number>x | <number>dpi | <number>dpcm`, against a 1x /
    // 96dpi device. Trimming the unit off without converting it made every
    // `dppx` query mis-answer — `2dppx` parsed as 0 and matched everything.
    fn media_dpi(v: &str) -> Option<f32> {
        let v = v.trim();
        for (unit, per_unit) in [("dppx", 96.0f32), ("dpcm", 2.54), ("dpi", 1.0), ("x", 96.0)] {
            if let Some(n) = v.strip_suffix(unit) {
                return n.trim().parse::<f32>().ok().map(|n| n * per_unit);
            }
        }
        None
    }
    if let Some(rest) = lower.strip_prefix("min-resolution:") {
        return media_dpi(rest).map(|d| 96.0 >= d).unwrap_or(false);
    }
    if let Some(rest) = lower.strip_prefix("max-resolution:") {
        return media_dpi(rest).map(|d| 96.0 <= d).unwrap_or(false);
    }
    if let Some(rest) = lower.strip_prefix("resolution:") {
        return media_dpi(rest)
            .map(|d| (d - 96.0).abs() < 0.5)
            .unwrap_or(false);
    }

    fn parse_media_two_sided_range(expr: &str, feature: &str, dim: f32) -> Option<bool> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 || parts[2] != feature {
            return None;
        }
        let left = parse_media_px(parts[0]);
        let right = parse_media_px(parts[4]);
        let left_ok = match parts[1] {
            "<" => left < dim,
            "<=" => left <= dim,
            ">" => left > dim,
            ">=" => left >= dim,
            _ => return None,
        };
        let right_ok = match parts[3] {
            "<" => dim < right,
            "<=" => dim <= right,
            ">" => dim > right,
            ">=" => dim >= right,
            _ => return None,
        };
        Some(left_ok && right_ok)
    }
    if let Some(v) = parse_media_two_sided_range(lower, "width", vw) {
        return v;
    }
    if let Some(v) = parse_media_two_sided_range(lower, "height", vh) {
        return v;
    }
    if let Some(v) = parse_media_two_sided_range(lower, "inline-size", vw) {
        return v;
    }
    if let Some(v) = parse_media_two_sided_range(lower, "block-size", vh) {
        return v;
    }

    // Modern range syntax: `width >= 300px`, `width > 300px`, etc.
    fn parse_media_range(expr: &str, dim: f32) -> Option<bool> {
        let e = expr.trim();
        if let Some(rest) = e.strip_prefix(">=") {
            return Some(dim >= parse_media_px(rest.trim()));
        }
        if let Some(rest) = e.strip_prefix("<=") {
            return Some(dim <= parse_media_px(rest.trim()));
        }
        if let Some(rest) = e.strip_prefix('>') {
            return Some(dim > parse_media_px(rest.trim()));
        }
        if let Some(rest) = e.strip_prefix('<') {
            return Some(dim < parse_media_px(rest.trim()));
        }
        if let Some(rest) = e.strip_prefix(':') {
            return Some((dim - parse_media_px(rest.trim())).abs() < 0.5);
        }
        None
    }
    if let Some(rest) = lower.strip_prefix("width") {
        if let Some(v) = parse_media_range(rest, vw) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("height") {
        if let Some(v) = parse_media_range(rest, vh) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("inline-size") {
        if let Some(v) = parse_media_range(rest, vw) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("block-size") {
        if let Some(v) = parse_media_range(rest, vh) {
            return v;
        }
    }

    // Unknown feature — fail closed. A parenthesized media feature is a real
    // feature query, not a media type; treating it as true makes mutually
    // exclusive unknown branches both match.
    false
}

/// Find byte index of `keyword` in `s` where it is not inside parentheses.
pub(crate) fn find_keyword_outside_parens(s: &str, keyword: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let kw = keyword.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i + kw.len() <= bytes.len() {
        match bytes[i] {
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                if depth > 0 {
                    depth -= 1;
                }
                i += 1;
            }
            _ => {
                // ⛔ ASCII CASE-INSENSITIVE. CSS keywords fold case, and an
                // uppercase `AND` slipped past this, past the `(` check, and
                // into the permissive media-type default — so
                // `@media screen AND (min-width: 500px)` matched at every
                // width and desktop-only rules applied on mobile.
                if depth == 0
                    && bytes.len() - i >= kw.len()
                    && bytes[i..i + kw.len()].eq_ignore_ascii_case(kw)
                {
                    return Some(i);
                }
                i += 1;
            }
        }
    }
    None
}

/// A length in a media query, in px.
///
/// ⛔ This was a THIRD private unit table — `px`, `em`, and a bare `parse()`
/// for everything else. A bare parse of `"40rem"` FAILS, giving 0, and
/// `(min-width: 40rem)` with a threshold of 0 matches every viewport. Every
/// rem-based breakpoint — which is what Bootstrap and Tailwind emit — was
/// therefore always on. Measured: `(min-width: 4000rem)` (64000px) matched a
/// 1200px viewport; Chrome correctly does not.
///
/// `parse_length` is the single unit definition. Relative units in a media
/// query resolve against the INITIAL font size, not any element's — Media
/// Queries 4 §1.3 — so both `em` and `rem` are 16px here.
pub(crate) fn parse_media_px(s: &str) -> f32 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    crate::css::value_parse::parse_length(s).resolve_vp(16.0, 0.0, 16.0, viewport().0, viewport().1)
}

thread_local! {
    /// The viewport the media query is being evaluated against, so `vw`/`vh`
    /// inside one mean what they say.
    static MQ_VIEWPORT: std::cell::Cell<(f32, f32)> = std::cell::Cell::new((0.0, 0.0));
}

fn viewport() -> (f32, f32) {
    MQ_VIEWPORT.with(|v| v.get())
}

/// Record the viewport for the duration of a media-query evaluation.
pub(crate) fn set_media_viewport(w: f32, h: f32) {
    MQ_VIEWPORT.with(|v| v.set((w, h)));
}
