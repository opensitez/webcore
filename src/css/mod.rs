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
                    return evaluate_container_for_type_and_style(
                        &cond[..i], w, h, container_type, style,
                    ) || evaluate_container_for_type_and_style(
                        &cond[i + 1..], w, h, container_type, style,
                    );
                }
                _ => {}
            }
        }
    }

    if let Some(rest) = cond.strip_prefix("not ") {
        return !evaluate_container_for_type_and_style(rest.trim(), w, h, container_type, style);
    }
    if let Some((start, end)) = find_keyword_outside_parens(cond, "and") {
        return evaluate_container_for_type_and_style(&cond[..start], w, h, container_type, style)
            && evaluate_container_for_type_and_style(
                &cond[end..],
                w,
                h,
                container_type,
                style,
            );
    }
    if let Some((start, end)) = find_keyword_outside_parens(cond, "or") {
        return evaluate_container_for_type_and_style(&cond[..start], w, h, container_type, style)
            || evaluate_container_for_type_and_style(
                &cond[end..],
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

    if lower.starts_with("style(") {
        return style.is_some_and(|s| container_style_query_matches(inner.trim(), s));
    }

    // Legacy min-/max- syntax
    if let Some(rest) = lower.strip_prefix("min-width:") {
        if container_type == crate::types::ContainerType::Normal {
            return false;
        }
        return w >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-width:") {
        if container_type == crate::types::ContainerType::Normal {
            return false;
        }
        return w <= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("min-height:") {
        if container_type != crate::types::ContainerType::Size {
            return false;
        }
        return h >= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("max-height:") {
        if container_type != crate::types::ContainerType::Size {
            return false;
        }
        return h <= parse_media_px(rest.trim());
    }
    if let Some(rest) = lower.strip_prefix("orientation:") {
        if container_type != crate::types::ContainerType::Size {
            return false;
        }
        return match rest.trim() {
            "portrait" => h >= w,
            "landscape" => w > h,
            _ => false,
        };
    }
    if lower.contains("aspect-ratio") {
        if container_type != crate::types::ContainerType::Size || h <= 0.0 {
            return false;
        }
        let ratio = w / h;
        if let Some(rest) = lower.strip_prefix("min-aspect-ratio:") {
            return parse_container_ratio(rest).is_some_and(|value| ratio >= value);
        }
        if let Some(rest) = lower.strip_prefix("max-aspect-ratio:") {
            return parse_container_ratio(rest).is_some_and(|value| ratio <= value);
        }
        if let Some(rest) = lower.strip_prefix("aspect-ratio:") {
            return parse_container_ratio(rest).is_some_and(|value| (ratio - value).abs() < 0.0001);
        }
        if let Some(rest) = lower.strip_prefix("aspect-ratio") {
            return parse_container_ratio_comparison(ratio, rest);
        }
        let parts = lower.split_whitespace().collect::<Vec<_>>();
        if parts.len() == 5 && parts[2] == "aspect-ratio" {
            let Some(left) = parse_container_ratio(parts[0]) else { return false };
            let Some(right) = parse_container_ratio(parts[4]) else { return false };
            return compare_container_ratio(left, ratio, parts[1])
                && compare_container_ratio(ratio, right, parts[3]);
        }
        return false;
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
        if container_type == crate::types::ContainerType::Normal {
            return false;
        }
        if let Some(v) = parse_range(rest, w) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("height") {
        if container_type != crate::types::ContainerType::Size {
            return false;
        }
        if let Some(v) = parse_range(rest, h) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("inline-size") {
        if container_type == crate::types::ContainerType::Normal {
            return false;
        }
        if let Some(v) = parse_range(rest, w) {
            return v;
        }
    }
    if let Some(rest) = lower.strip_prefix("block-size") {
        if container_type != crate::types::ContainerType::Size {
            return false;
        }
        if let Some(v) = parse_range(rest, h) {
            return v;
        }
    }

    // Unknown features do not match a container.
    false
}

fn parse_container_ratio(raw: &str) -> Option<f32> {
    let raw = raw.trim();
    let (numerator, denominator) = raw.split_once('/').unwrap_or((raw, "1"));
    let numerator = numerator.trim().parse::<f32>().ok()?;
    let denominator = denominator.trim().parse::<f32>().ok()?;
    (numerator >= 0.0 && denominator > 0.0 && numerator.is_finite() && denominator.is_finite())
        .then_some(numerator / denominator)
}

fn compare_container_ratio(left: f32, right: f32, operator: &str) -> bool {
    match operator {
        "<" => left < right,
        "<=" => left <= right,
        ">" => left > right,
        ">=" => left >= right,
        "=" | ":" => (left - right).abs() < 0.0001,
        _ => false,
    }
}

fn parse_container_ratio_comparison(ratio: f32, comparison: &str) -> bool {
    for operator in [">=", "<=", ">", "<", "=", ":"] {
        if let Some(value) = comparison.trim().strip_prefix(operator) {
            return parse_container_ratio(value)
                .is_some_and(|value| compare_container_ratio(ratio, value, operator));
        }
    }
    false
}

fn container_style_query_matches(query: &str, style: &crate::types::ComputedStyle) -> bool {
    if !query.to_ascii_lowercase().starts_with("style(") || !query.ends_with(')') {
        return false;
    }
    let body = query["style(".len()..query.len() - 1].trim();
    evaluate_style_query(body, style)
}

fn evaluate_style_query(query: &str, style: &crate::types::ComputedStyle) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    if query
        .as_bytes()
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"not "))
    {
        return !evaluate_style_query(&query[4..], style);
    }
    for (keyword, all) in [("and", true), ("or", false)] {
        if let Some((start, end)) = find_keyword_outside_parens(query, keyword) {
            let left = query[..start].trim();
            let right = query[end..].trim();
            if left.starts_with('(') && right.starts_with('(') {
                return if all {
                    evaluate_style_query(left, style) && evaluate_style_query(right, style)
                } else {
                    evaluate_style_query(left, style) || evaluate_style_query(right, style)
                };
            }
        }
    }
    if query.starts_with('(') && query.ends_with(')') {
        let mut depth = 0usize;
        let wraps_all = query.bytes().enumerate().all(|(index, byte)| {
            match byte {
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            depth > 0 || index == query.len() - 1
        });
        if wraps_all && depth == 0 {
            return evaluate_style_query(&query[1..query.len() - 1], style);
        }
    }
    evaluate_style_feature(query, style)
}

fn evaluate_style_feature(body: &str, style: &crate::types::ComputedStyle) -> bool {
    if let Some(matches) = evaluate_style_range(body, style) {
        return matches;
    }
    let Some((property, value)) = body.split_once(':') else {
        return body.starts_with("--") && style.custom_props.contains_key(body);
    };
    let property = property.trim();
    let value = value.trim();
    let resolved = crate::css::resolve_var_references(value, &style.custom_props);
    let value = resolved.trim();
    if property.starts_with("--") {
        return style
            .custom_props
            .get(property)
            .is_some_and(|actual| actual.trim() == value);
    }
    match property.to_ascii_lowercase().as_str() {
        "color" => crate::css::parse_color(value).is_some_and(|c| c == style.color),
        "background-color" => {
            crate::css::parse_color(value).is_some_and(|c| c == style.background_color)
        }
        "display" => {
            let wanted = value.replace('-', "").to_ascii_lowercase();
            let actual = format!("{:?}", style.display).to_ascii_lowercase();
            actual == wanted
        }
        "font-weight" => {
            let wanted = match value.to_ascii_lowercase().as_str() {
                "normal" => Some(400),
                "bold" => Some(700),
                _ => value.parse::<u16>().ok(),
            };
            wanted.is_some_and(|wanted| style.font_weight.value() == wanted)
        }
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StyleRangeKind {
    Number,
    Percentage,
    Length,
    Angle,
    Time,
    Frequency,
    Resolution,
}

#[derive(Clone, Copy)]
struct StyleRangeValue {
    kind: StyleRangeKind,
    value: f32,
}

fn evaluate_style_range(
    body: &str,
    style: &crate::types::ComputedStyle,
) -> Option<bool> {
    let mut operands = Vec::new();
    let mut operators = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut chars = body.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '<' | '>' | '=' if depth == 0 => {
                operands.push(body[start..index].trim());
                let end = if chars.peek().is_some_and(|(_, next)| *next == '=') {
                    chars.next().unwrap().0 + 1
                } else {
                    index + 1
                };
                operators.push(&body[index..end]);
                start = end;
            }
            _ => {}
        }
    }
    if operators.is_empty() {
        return None;
    }
    operands.push(body[start..].trim());
    if operands.len() < 2 || operands.len() > 3 || operands.iter().any(|part| part.is_empty()) {
        return Some(false);
    }
    if operators.len() == 2
        && !((operators[0].starts_with('<') && operators[1].starts_with('<'))
            || (operators[0].starts_with('>') && operators[1].starts_with('>')))
    {
        return Some(false);
    }
    let Some(values) = operands
        .iter()
        .map(|part| parse_style_range_value(part, style))
        .collect::<Option<Vec<_>>>()
    else {
        return Some(false);
    };
    Some(operators.iter().enumerate().all(|(index, operator)| {
        compare_style_range_values(values[index], values[index + 1], operator)
    }))
}

fn parse_style_range_value(
    raw: &str,
    style: &crate::types::ComputedStyle,
) -> Option<StyleRangeValue> {
    let raw = raw.trim();
    let resolved = if raw.starts_with("--") && !raw.contains([' ', '(', ')', ':']) {
        style.custom_props.get(raw)?.as_str().to_string()
    } else {
        raw.to_string()
    };
    let resolved = crate::css::resolve_var_references(&resolved, &style.custom_props);
    let value = resolved.trim().to_ascii_lowercase();
    let typed = if let Ok(number) = value.parse::<f32>() {
        StyleRangeValue { kind: StyleRangeKind::Number, value: number }
    } else if let Some(number) = value.strip_suffix('%').and_then(|v| v.parse::<f32>().ok()) {
        StyleRangeValue { kind: StyleRangeKind::Percentage, value: number }
    } else {
        let units = [
            ("dppx", StyleRangeKind::Resolution, 1.0),
            ("dpcm", StyleRangeKind::Resolution, 2.54 / 96.0),
            ("dpi", StyleRangeKind::Resolution, 1.0 / 96.0),
            ("khz", StyleRangeKind::Frequency, 1000.0),
            ("hz", StyleRangeKind::Frequency, 1.0),
            ("grad", StyleRangeKind::Angle, 0.9),
            ("turn", StyleRangeKind::Angle, 360.0),
            ("deg", StyleRangeKind::Angle, 1.0),
            ("rad", StyleRangeKind::Angle, 180.0 / std::f32::consts::PI),
            ("ms", StyleRangeKind::Time, 1.0),
            ("s", StyleRangeKind::Time, 1000.0),
        ];
        if let Some((number, kind, factor)) = units.iter().find_map(|(unit, kind, factor)| {
            value.strip_suffix(unit).and_then(|v| v.parse::<f32>().ok())
                .map(|number| (number, *kind, *factor))
        }) {
            StyleRangeValue { kind, value: number * factor }
        } else if ["px", "cm", "mm", "q", "in", "pt", "pc"]
            .iter()
            .any(|unit| value.ends_with(unit))
        {
            let crate::types::CssLength::Px(px) = crate::css::parse_length_checked(&value)? else {
                return None;
            };
            StyleRangeValue { kind: StyleRangeKind::Length, value: px }
        } else {
            return None;
        }
    };
    typed.value.is_finite().then_some(typed)
}

fn compare_style_range_values(left: StyleRangeValue, right: StyleRangeValue, operator: &str) -> bool {
    let same_kind = left.kind == right.kind
        || (left.kind == StyleRangeKind::Number && left.value == 0.0 && right.kind == StyleRangeKind::Length)
        || (right.kind == StyleRangeKind::Number && right.value == 0.0 && left.kind == StyleRangeKind::Length);
    if !same_kind {
        return false;
    }
    match operator {
        "<" => left.value < right.value,
        "<=" => left.value <= right.value,
        ">" => left.value > right.value,
        ">=" => left.value >= right.value,
        "=" => (left.value - right.value).abs() < f32::EPSILON,
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
