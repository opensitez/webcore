//! Syntax shared by shape feature queries, cascade validation, and float layout.

use super::{property_defs, value_parse};
use crate::types::CssLength;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShapeBox {
    Margin,
    Border,
    HalfBorder,
    Padding,
    Content,
}

pub(crate) fn split_shape_at(inner: &str) -> (Option<&str>, Option<&str>) {
    let trimmed = inner.trim();
    let lower = trimmed.to_ascii_lowercase();
    let split = if lower.starts_with("at ") {
        Some((0, 3))
    } else {
        lower.find(" at ").map(|index| (index, index + 4))
    };
    if let Some((before_end, after_start)) = split {
        let before = trimmed[..before_end].trim();
        let after = trimmed[after_start..].trim();
        (if before.is_empty() { None } else { Some(before) }, Some(after))
    } else {
        (if trimmed.is_empty() { None } else { Some(trimmed) }, None)
    }
}

pub(crate) fn shape_box_kind(value: &str) -> Option<ShapeBox> {
    Some(match value.to_ascii_lowercase().as_str() {
        "margin-box" => ShapeBox::Margin,
        "border-box" => ShapeBox::Border,
        "half-border-box" => ShapeBox::HalfBorder,
        "padding-box" => ShapeBox::Padding,
        "content-box" => ShapeBox::Content,
        _ => return None,
    })
}

fn shape_box(value: &str) -> bool {
    shape_box_kind(value).is_some()
}

fn shape_length(value: &str, negative: bool) -> bool {
    let value = value.trim();
    if value.parse::<f32>().is_ok_and(|number| number != 0.0)
        || (!negative && value.starts_with('-'))
    {
        return false;
    }
    value_parse::parse_length_checked(value).is_some_and(|length| {
        !length.is_auto()
            && !matches!(length, CssLength::None | CssLength::Stretch | CssLength::Content)
    })
}

pub(crate) fn valid_shape_margin(value: &str) -> bool {
    shape_length(value, false)
}

pub(crate) fn valid_basic_shape(value: &str) -> bool {
    let Some((name, inner)) = value.trim().split_once('(') else {
        return false;
    };
    let Some(inner) = inner.strip_suffix(')') else {
        return false;
    };
    match name.to_ascii_lowercase().as_str() {
        "circle" | "ellipse" => {
            let (radii, position) = split_shape_at(inner);
            if position.is_some_and(|position| {
                property_defs::parse_background_position_pair(position).is_none()
            }) {
                return false;
            }
            let radii = value_parse::split_css_values(radii.unwrap_or(""));
            let keyword = |value: &str| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "closest-side" | "farthest-side" | "closest-corner" | "farthest-corner"
                )
            };
            if name.eq_ignore_ascii_case("circle") {
                radii.len() <= 1
                    && radii
                        .first()
                        .is_none_or(|radius| keyword(radius) || shape_length(radius, false))
            } else {
                match radii.as_slice() {
                    [] => true,
                    [radius] => keyword(radius),
                    [horizontal, vertical] => {
                        shape_length(horizontal, false) && shape_length(vertical, false)
                    }
                    _ => false,
                }
            }
        }
        "inset" => {
            let tokens = value_parse::split_css_values(inner);
            let round = tokens
                .iter()
                .position(|token| token.eq_ignore_ascii_case("round"))
                .unwrap_or(tokens.len());
            (1..=4).contains(&round)
                && tokens[..round].iter().all(|token| shape_length(token, true))
                && (round == tokens.len()
                    || (round + 1 < tokens.len()
                        && super::supports::declaration_value(
                            super::properties::PropertyId::BorderRadius,
                            &tokens[round + 1..].join(" "),
                        )))
        }
        "polygon" => {
            let parts = value_parse::split_top_level_commas(inner);
            let first_is_rule = parts
                .first()
                .is_some_and(|part| matches!(part.trim(), "evenodd" | "nonzero"));
            let coordinates = &parts[usize::from(first_is_rule)..];
            !coordinates.is_empty()
                && coordinates.iter().all(|part| {
                    let tokens = value_parse::split_css_values(part);
                    tokens.len() == 2 && tokens.iter().all(|token| shape_length(token, true))
                })
        }
        _ => false,
    }
}

pub(crate) fn valid_shape_outside_non_image(value: &str) -> bool {
    if value.trim().eq_ignore_ascii_case("none") {
        return true;
    }
    let tokens = value_parse::split_css_values(value);
    match tokens.as_slice() {
        [one] => shape_box(one) || valid_basic_shape(one),
        [first, second] => {
            (shape_box(first) && valid_basic_shape(second))
                || (valid_basic_shape(first) && shape_box(second))
        }
        _ => false,
    }
}
