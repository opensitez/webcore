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

/// A conjunction of media query lists. Each list retains its own comma/OR
/// semantics; nested `@media`, link media, and imports add another constraint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum MediaConditions {
    #[default]
    Any,
    Query(String),
    All(Box<[String]>),
}

impl MediaConditions {
    pub(crate) fn with_query(&self, query: &str) -> Self {
        let query = query.trim();
        if query.is_empty() || query.eq_ignore_ascii_case("all") {
            return self.clone();
        }
        match self {
            Self::Any => Self::Query(query.to_string()),
            Self::Query(previous) => Self::All(vec![previous.clone(), query.to_string()].into()),
            Self::All(previous) => {
                let mut queries = previous.to_vec();
                queries.push(query.to_string());
                Self::All(queries.into_boxed_slice())
            }
        }
    }

    pub(crate) fn and(&self, other: &Self) -> Self {
        match other {
            Self::Any => self.clone(),
            Self::Query(query) => self.with_query(query),
            Self::All(queries) => queries.iter().fold(self.clone(), |conditions, query| {
                conditions.with_query(query)
            }),
        }
    }

    pub fn matches(&self, vw: f32, vh: f32) -> bool {
        match self {
            Self::Any => true,
            Self::Query(query) => evaluate_media(query, vw, vh),
            Self::All(queries) => queries.iter().all(|query| evaluate_media(query, vw, vh)),
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Any)
    }

    pub fn contains(&self, text: &str) -> bool {
        match self {
            Self::Any => false,
            Self::Query(query) => query.contains(text),
            Self::All(queries) => queries.iter().any(|query| query.contains(text)),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Any => String::new(),
            Self::Query(query) => query.clone(),
            Self::All(queries) => queries.join(" AND "),
        }
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        match self {
            Self::Any => 0,
            Self::Query(query) => query.capacity(),
            Self::All(queries) => queries
                .iter()
                .map(|query| std::mem::size_of::<String>() + query.capacity())
                .sum(),
        }
    }
}

/// Evaluate a CSS @media condition string.
/// Returns true if the condition matches the given viewport dimensions.
/// `condition` is the full text after "@media" (trimmed).
pub fn evaluate_media(condition: &str, vw: f32, vh: f32) -> bool {
    evaluate_media_value(condition, vw, vh).unwrap_or(false)
}

// `None` is the Media Queries "unknown" value. It is not ordinary false:
// negating an unknown feature must not make an unsupported query match.
fn evaluate_media_value(condition: &str, vw: f32, vh: f32) -> Option<bool> {
    let cond = condition.trim();
    if cond.is_empty() {
        return Some(true);
    }

    // ⛔ `only` is a no-op qualifier — `only print` IS `print` (Media Queries
    // §3). Leaving it attached meant the string matched no known media type
    // and fell through to the permissive default, so a print stylesheet
    // written `@media only print` (or `<link media="only print">`) applied to
    // the SCREEN: `display: block` everywhere, columns and floats dropped,
    // navigation hidden. A page styled that way renders as one long column,
    // exactly as if it had been printed.
    let cond = match cond.len() >= 5 && cond.as_bytes()[..5].eq_ignore_ascii_case(b"only ") {
        true => cond[5..].trim_start(),
        false => cond,
    };
    if cond.is_empty() {
        return Some(true);
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
            return Some(
                evaluate_media(left.trim(), vw, vh) || evaluate_media(right.trim(), vw, vh),
            );
        }
    }

    // A media-type modifier negates the whole query. A media-condition `not`
    // takes exactly one parenthesized term; combining it requires grouping.
    if cond.len() >= 4 && cond.as_bytes()[..4].eq_ignore_ascii_case(b"not ") {
        let rest = cond[4..].trim();
        if rest.starts_with('(') {
            if find_keyword_outside_parens(rest, "and").is_some()
                || find_keyword_outside_parens(rest, "or").is_some()
            {
                return None;
            }
        } else if find_keyword_outside_parens(rest, "or").is_some() {
            return None;
        }
        return evaluate_media_value(rest, vw, vh).map(|value| !value);
    }

    let and = find_keyword_outside_parens(cond, "and");
    let or = find_keyword_outside_parens(cond, "or");
    if and.is_some() && or.is_some() {
        return None;
    }

    // A media type may lead an `and` chain; later terms must be parenthesized.
    if let Some((start, end)) = and {
        let left = &cond[..start];
        let right = &cond[end..];
        if left.trim().is_empty() || !right.trim().starts_with('(') {
            return None;
        }
        let left = evaluate_media_value(left.trim(), vw, vh);
        if left == Some(false) {
            return Some(false);
        }
        return match (left, evaluate_media_value(right.trim(), vw, vh)) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        };
    }

    // `or` joins media conditions, never bare media types.
    if let Some((start, end)) = or {
        let left = &cond[..start];
        let right = &cond[end..];
        if !left.trim().starts_with('(') || !right.trim().starts_with('(') {
            return None;
        }
        let left = evaluate_media_value(left.trim(), vw, vh);
        if left == Some(true) {
            return Some(true);
        }
        return match (left, evaluate_media_value(right.trim(), vw, vh)) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
    }

    // Named media types (no parens)
    if !cond.starts_with('(') {
        return Some(match cond.to_ascii_lowercase().as_str() {
            "screen" | "all" => true,
            // Everything that is not a screen. The deprecated types are listed
            // because they must not match either — a `<link media="handheld">`
            // sheet applying to a desktop render is the same failure as the
            // print one.
            "print" | "speech" | "aural" | "braille" | "embossed" | "handheld" | "projection"
            | "tty" | "tv" => false,
            // Media Queries 4: an unknown media type never matches.
            _ => false,
        });
    }

    if cond.starts_with('(') && cond.ends_with(')') {
        let inner = cond[1..cond.len() - 1].trim();
        let is_parenthesized_logical = inner.starts_with('(')
            || (inner.len() >= 4 && inner.as_bytes()[..4].eq_ignore_ascii_case(b"not "))
            || find_keyword_outside_parens(inner, "and").is_some()
            || find_keyword_outside_parens(inner, "or").is_some();
        if is_parenthesized_logical {
            return evaluate_media_value(inner, vw, vh);
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

    fn media_query_px(value: &str) -> Option<f32> {
        let value = value.trim();
        if value.parse::<f32>().is_ok_and(|number| number != 0.0) {
            return None;
        }
        let length = parse_length_checked(value)?;
        if matches!(
            length,
            CssLength::Auto
                | CssLength::None
                | CssLength::Content
                | CssLength::MinContent
                | CssLength::MaxContent
                | CssLength::FitContent
                | CssLength::FitContentArg(_)
                | CssLength::Stretch
                | CssLength::Percent(_)
        ) {
            return None;
        }
        let resolved = parse_media_px(value);
        resolved.is_finite().then_some(resolved)
    }

    fn media_dimension(feature: &str, width: f32, height: f32) -> Option<f32> {
        match feature {
            "width" | "inline-size" => Some(width),
            "height" | "block-size" => Some(height),
            _ => None,
        }
    }

    if let Some(rest) = lower.strip_prefix("min-width:") {
        return media_query_px(rest).map(|value| vw >= value);
    }
    if let Some(rest) = lower.strip_prefix("max-width:") {
        return media_query_px(rest).map(|value| vw <= value);
    }
    if let Some(rest) = lower.strip_prefix("min-height:") {
        return media_query_px(rest).map(|value| vh >= value);
    }
    if let Some(rest) = lower.strip_prefix("max-height:") {
        return media_query_px(rest).map(|value| vh <= value);
    }
    if let Some(rest) = lower.strip_prefix("orientation:") {
        return match rest.trim() {
            "landscape" => Some(vw > vh),
            "portrait" => Some(vh >= vw),
            _ => None,
        };
    }
    if let Some(rest) = lower.strip_prefix("prefers-color-scheme:") {
        return match rest.trim() {
            "light" => Some(color_scheme_preference() == ColorSchemePreference::Light),
            "dark" => Some(color_scheme_preference() == ColorSchemePreference::Dark),
            _ => None,
        };
    }
    if let Some(rest) = lower.strip_prefix("hover:") {
        return match rest.trim() {
            "hover" => Some(true),
            "none" => Some(false),
            _ => None,
        };
    }
    if let Some(rest) = lower.strip_prefix("pointer:") {
        return match rest.trim() {
            "fine" => Some(true),
            "coarse" | "none" => Some(false),
            _ => None,
        };
    }
    // ⛔ ANSWER THE PREFERENCE FEATURES. Falling through to the fail-open
    // default made a feature match BOTH of its mutually exclusive values, so
    // `(prefers-reduced-motion: reduce)` and `(no-preference)` both applied and
    // source order decided. This engine has no OS preference channel, so it
    // reports the defaults of an ordinary desktop UA — which is a real answer,
    // not a guess, and stops the self-contradiction.
    for (feature, matching, allowed) in [
        (
            "prefers-reduced-motion:",
            "no-preference",
            &["no-preference", "reduce"][..],
        ),
        (
            "prefers-contrast:",
            "no-preference",
            &["no-preference", "more", "less", "custom"],
        ),
        (
            "prefers-reduced-transparency:",
            "no-preference",
            &["no-preference", "reduce"],
        ),
        (
            "prefers-reduced-data:",
            "no-preference",
            &["no-preference", "reduce"],
        ),
        ("forced-colors:", "none", &["none", "active"]),
        ("inverted-colors:", "none", &["none", "inverted"]),
        ("any-hover:", "hover", &["none", "hover"]),
        ("any-pointer:", "fine", &["none", "coarse", "fine"]),
        ("scripting:", "none", &["none", "initial-only", "enabled"]),
        ("update:", "fast", &["none", "slow", "fast"]),
    ] {
        if let Some(rest) = lower.strip_prefix(feature) {
            let value = rest.trim();
            return allowed.contains(&value).then_some(value == matching);
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
        return media_dpi(rest).map(|d| 96.0 >= d);
    }
    if let Some(rest) = lower.strip_prefix("max-resolution:") {
        return media_dpi(rest).map(|d| 96.0 <= d);
    }
    if let Some(rest) = lower.strip_prefix("resolution:") {
        return media_dpi(rest).map(|d| (d - 96.0).abs() < 0.5);
    }

    if let Some(range) = split_range_comparisons(lower) {
        if !range.valid {
            return None;
        }
        let operands = range.operands();
        let operators = range.operators();
        if operands.len() == 2 {
            if let Some(dim) = media_dimension(operands[0], vw, vh) {
                return media_query_px(operands[1])
                    .map(|value| compare_container_size(dim, value, operators[0]));
            }
            if let Some(dim) = media_dimension(operands[1], vw, vh) {
                return media_query_px(operands[0])
                    .map(|value| compare_container_size(value, dim, operators[0]));
            }
        } else if operands.len() == 3 {
            if let Some(dim) = media_dimension(operands[1], vw, vh) {
                let ordered = (operators[0].starts_with('<') && operators[1].starts_with('<'))
                    || (operators[0].starts_with('>') && operators[1].starts_with('>'));
                return media_query_px(operands[0])
                    .zip(media_query_px(operands[2]))
                    .map(|(left, right)| {
                        ordered
                            && compare_container_size(left, dim, operators[0])
                            && compare_container_size(dim, right, operators[1])
                    });
            }
        }
        return None;
    }
    for (feature, dim) in [
        ("width", vw),
        ("height", vh),
        ("inline-size", vw),
        ("block-size", vh),
    ] {
        if let Some(value) = lower
            .strip_prefix(feature)
            .and_then(|rest| rest.strip_prefix(':'))
        {
            return media_query_px(value).map(|value| (dim - value).abs() < 0.5);
        }
    }

    // Unknown features stay unknown through negation and logical combinations.
    None
}

/// Find an `and`/`or` token outside parentheses. Parentheses delimit tokens
/// even without intervening whitespace, as in `(width: 1px)and (height: 1px)`.
pub(crate) fn find_keyword_outside_parens(s: &str, keyword: &str) -> Option<(usize, usize)> {
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
                let end = i + kw.len();
                let left_boundary =
                    i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b')';
                let right_boundary =
                    end == bytes.len() || bytes[end].is_ascii_whitespace() || bytes[end] == b'(';
                if depth == 0
                    && bytes.len() - i >= kw.len()
                    && left_boundary
                    && right_boundary
                    && bytes[i..i + kw.len()].eq_ignore_ascii_case(kw)
                {
                    return Some((i, end));
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
