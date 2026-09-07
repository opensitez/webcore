//! CSS: selectors, the parser, the cascade and the UA stylesheet.
//!
//! ⛔ This file DECLARES and RE-EXPORTS. It used to hold 7,766 of the
//! folder's 10,718 lines — the folder existed and `mod.rs` absorbed
//! everything anyway, which is exactly the failure `dom/api.rs` had.
//! Every call site says `crate::css::X`, so the glob re-exports below
//! keep ONE path to each item rather than scattering module paths
//! through the crate.

// ─── Container Query Evaluation ──────────────────────────────────────────────

/// Evaluate a `@container` condition string against known container dimensions.
///
/// Supports:
/// - Legacy syntax: `(min-width: Xpx)`, `(max-width: Xpx)`, `(min-height: Xpx)`, `(max-height: Xpx)`
/// - Modern range syntax: `(width > Xpx)`, `(width >= Xpx)`, `(width < Xpx)`, `(width <= Xpx)`
/// - Logical: `and`, `or`, `not`
pub fn evaluate_container(condition: &str, w: f32, h: f32) -> bool {
    evaluate_container_for_type(condition, w, h, crate::types::ContainerType::Size)
}

pub(crate) fn evaluate_container_for_type(
    condition: &str,
    w: f32,
    h: f32,
    container_type: crate::types::ContainerType,
) -> bool {
    evaluate_container_for_type_and_style(condition, w, h, container_type, None)
}

pub(crate) fn evaluate_container_for_type_and_style(
    condition: &str,
    w: f32,
    h: f32,
    container_type: crate::types::ContainerType,
    style: Option<&crate::types::ComputedStyle>,
) -> bool {
    let cond = condition.trim();
    if cond.is_empty() {
        return true;
    }

    // Comma = OR at top level
    {
        let mut depth = 0usize;
        let bytes = cond.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                }
                b',' if depth == 0 => {
                    return evaluate_container_for_type(&cond[..i], w, h, container_type)
                        || evaluate_container_for_type(&cond[i + 1..], w, h, container_type);
                }
                _ => {}
            }
        }
    }

    if let Some(rest) = cond.strip_prefix("not ") {
        return !evaluate_container_for_type_and_style(rest.trim(), w, h, container_type, style);
    }
    if let Some(idx) = find_keyword_outside_parens(cond, " and ") {
        return evaluate_container_for_type_and_style(&cond[..idx], w, h, container_type, style)
            && evaluate_container_for_type_and_style(
                &cond[idx + 5..],
                w,
                h,
                container_type,
                style,
            );
    }
    if let Some(idx) = find_keyword_outside_parens(cond, " or ") {
        return evaluate_container_for_type_and_style(&cond[..idx], w, h, container_type, style)
            || evaluate_container_for_type_and_style(
                &cond[idx + 4..],
                w,
                h,
                container_type,
                style,
            );
    }

    // Strip outer parens
    let inner = if cond.starts_with('(') && cond.ends_with(')') {
        &cond[1..cond.len() - 1]
    } else {
        cond
    };
    let lower = inner.to_ascii_lowercase();
    let lower = lower.trim();

    // So `vw`/`vh` inside the query mean the viewport being queried.
    crate::css::media_query::set_media_viewport(w, h);

    // Legacy min-/max- syntax
    if let Some(rest) = lower.strip_prefix("min-width:") {
        return w >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-width:") {
        return w <= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("min-height:") {
        if container_type == crate::types::ContainerType::InlineSize {
            return false;
        }
        return h >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-height:") {
        if container_type == crate::types::ContainerType::InlineSize {
            return false;
        }
        return h <= parse_media_px(rest.trim());
    }

    // Modern range syntax: `width >= 300px`, `width > 300px`, etc.
    fn parse_range(expr: &str, dim: f32) -> Option<bool> {
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
        if let Some(v) = parse_range(rest, w) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("height") {
        if container_type == crate::types::ContainerType::InlineSize {
            return false;
        }
        if let Some(v) = parse_range(rest, h) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("inline-size") {
        if let Some(v) = parse_range(rest, w) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("block-size") {
        if container_type == crate::types::ContainerType::InlineSize {
            return false;
        }
        if let Some(v) = parse_range(rest, h) {
            return v;
        }
    }

    if lower.starts_with("style(") {
        return style.is_some_and(|s| container_style_query_matches(lower, s));
    }

    // Unknown — fail-open
    true
}

fn container_style_query_matches(lower: &str, style: &crate::types::ComputedStyle) -> bool {
    let Some(body) = lower
        .strip_prefix("style(")
        .and_then(|s| s.strip_suffix(')'))
    else {
        return false;
    };
    let body = body.trim();
    let Some((property, value)) = body.split_once(':') else {
        return false;
    };
    let property = property.trim();
    let value = value.trim();
    match property {
        "color" => crate::css::parse_color(value).is_some_and(|c| c == style.color),
        "background-color" => {
            crate::css::parse_color(value).is_some_and(|c| c == style.background_color)
        }
        "display" => {
            let wanted = value.replace('-', "");
            let actual = format!("{:?}", style.display).to_ascii_lowercase();
            actual == wanted
        }
        "font-weight" => value
            .parse::<u16>()
            .map(|wanted| style.font_weight.value() == wanted)
            .unwrap_or(false),
        _ => false,
    }
}

pub mod animation;
pub mod apply;
pub mod calc;
pub mod cascade;
pub mod cascade_incremental;
pub mod cascade_parallel;
pub mod color_spaces;
pub mod container;
pub mod font;
pub mod font_face;
pub mod grid_parse;
pub mod inherit;
pub mod keyframes;
pub mod matching;
pub mod media_query;
pub mod parser;
pub mod properties;
pub mod property_defs;
pub mod rule;
pub mod selector;
pub mod stylesheet;
pub mod transform_parse;
pub mod ua_sheet;
pub mod value_parse;

pub use animation::*;
pub use apply::*;
pub use calc::*;
pub use cascade::*;
pub use cascade_incremental::*;
pub use cascade_parallel::*;
pub use container::*;
pub use font::*;
pub use font_face::*;
pub use grid_parse::*;
pub use inherit::*;
pub use keyframes::*;
pub use matching::*;
pub use media_query::*;
pub use parser::*;
pub use rule::*;
pub use selector::*;
pub use stylesheet::*;
pub use transform_parse::*;
pub use ua_sheet::*;
pub use value_parse::*;
