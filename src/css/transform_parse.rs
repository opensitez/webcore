//! Parsing `transform` and `transform-origin`.
//!
//! ⛔ Value PARSING, which had ended up in the file that APPLIES values.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use std::collections::{HashMap, HashSet};

/// Parse a CSS `transform` value string into a `CssTransform`.
pub fn parse_css_transform(v: &str) -> crate::types::CssTransform {
    parse_css_transform_checked(v).unwrap_or_default()
}

/// Parse a CSS `transform` value, rejecting the whole declaration if any
/// transform function is unknown or malformed.
pub fn parse_css_transform_checked(v: &str) -> Option<crate::types::CssTransform> {
    use crate::types::{CssTransform, TransformOp};
    let mut ops = Vec::new();
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some(CssTransform::default());
    }
    if v.is_empty() { return None; }
    // Simple tokenizer: split on function calls like "translate(10px, 20px) rotate(45deg)"
    let mut rest = v;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        // Find function name up to '('
        let paren_pos = match rest.find('(') {
            Some(p) => p,
            None => return None,
        };
        let func = rest[..paren_pos].trim().to_ascii_lowercase();
        let after_paren = &rest[paren_pos..];
        let (args_str, after_func) = consume_parenthesized(after_paren)?;
        rest = after_func;

        let raw = split_transform_args(args_str);
        let count = match func.as_str() {
            "translate" | "scale" | "skew" => 1..=2,
            "translate3d" | "scale3d" => 3..=3,
            "matrix" => 6..=6,
            "matrix3d" => 16..=16,
            _ => 1..=1,
        };
        if !count.contains(&raw.len()) { return None; }
        let len = |i: usize, percent: bool| parse_transform_length(raw[i], percent);
        let ang = |i: usize| {
            let value = raw[i];
            // Transform functions, unlike the rotate property, allow literal zero.
            if value.parse::<f32>().is_ok_and(|n| n == 0.0) { Some(0.0) }
            else { parse_transform_angle(value) }
        };
        let scale = |i: usize| parse_transform_scale(raw[i]);
        let number = |i: usize| super::calc::parse_css_number(raw[i]);

        match func.as_str() {
            "translate" => ops.push(TransformOp::Translate(len(0, true)?,
                if raw.len() == 2 { len(1, true)? } else { CssLength::Zero })),
            "translatex" => ops.push(TransformOp::TranslateX(len(0, true)?)),
            "translatey" => ops.push(TransformOp::TranslateY(len(0, true)?)),
            "translate3d" => {
                len(2, false)?;
                ops.push(TransformOp::Translate(len(0, true)?, len(1, true)?));
            }
            "translatez" => { len(0, false)?; }
            "scale" => ops.push(TransformOp::Scale(scale(0)?,
                if raw.len() == 2 { scale(1)? } else { scale(0)? })),
            "scalex" => ops.push(TransformOp::ScaleX(scale(0)?)),
            "scaley" => ops.push(TransformOp::ScaleY(scale(0)?)),
            "scale3d" => {
                scale(2)?;
                ops.push(TransformOp::Scale(scale(0)?, scale(1)?));
            }
            "scalez" => { scale(0)?; }
            "rotate" | "rotatez" => ops.push(TransformOp::Rotate(ang(0)?)),
            "rotatex" | "rotatey" => { ang(0)?; }
            "skew" => {
                let x = ang(0)?.to_radians().tan();
                let y = if raw.len() == 2 { ang(1)?.to_radians().tan() } else { 0.0 };
                ops.push(TransformOp::Matrix(1.0, y, x, 1.0, 0.0, 0.0));
            }
            "skewx" => ops.push(TransformOp::SkewX(ang(0)?)),
            "skewy" => ops.push(TransformOp::SkewY(ang(0)?)),
            "matrix" => ops.push(TransformOp::Matrix(
                number(0)?, number(1)?, number(2)?, number(3)?, number(4)?, number(5)?,
            )),
            "matrix3d" => {
                for i in 0..16 { number(i)?; }
                ops.push(TransformOp::Matrix(
                    number(0)?, number(1)?, number(4)?, number(5)?, number(12)?, number(13)?,
                ));
            }
            "perspective" => { len(0, false)?; }
            _ => return None,
        }
    }
    Some(CssTransform { ops })
}

fn parse_transform_length(value: &str, percentage: bool) -> Option<CssLength> {
    let length = crate::css::value_parse::parse_length_checked(value)?;
    if matches!(length, CssLength::Auto | CssLength::None | CssLength::MinContent
        | CssLength::MaxContent | CssLength::FitContent | CssLength::FitContentArg(_)
        | CssLength::Content | CssLength::Stretch)
        || value.parse::<f32>().is_ok_and(|number| number != 0.0)
        || (!percentage && length.has_percentage()) {
        return None;
    }
    Some(length)
}

fn parse_transform_angle(value: &str) -> Option<f32> {
    if super::calc::is_math_function(value) {
        return super::calc::parse_math_angle_deg(value);
    }
    let lower = value.to_ascii_lowercase();
    let number = ["turn", "grad", "rad", "deg"].iter()
        .find_map(|unit| lower.strip_suffix(unit))?;
    number.parse::<f32>().ok().filter(|n| n.is_finite())?;
    super::calc::parse_math_angle_deg(&format!("calc({value})"))
}

fn parse_transform_scale(value: &str) -> Option<f32> {
    super::calc::parse_css_number(value)
        .or_else(|| value.strip_suffix('%')?.parse::<f32>().ok().map(|n| n / 100.0))
        .or_else(|| super::calc::parse_math_alpha(value))
        .filter(|n| n.is_finite())
}

pub fn parse_individual_translate(v: &str) -> Option<crate::types::CssTransform> {
    let values = individual_transform_values(v)?;
    if values.is_empty() { return Some(CssTransform::default()); }
    if values.len() > 3 { return None; }
    let mut lengths = Vec::new();
    for (axis, value) in values.iter().enumerate() {
        lengths.push(parse_transform_length(value, axis != 2)?);
    }
    Some(CssTransform { ops: vec![TransformOp::Translate(
        lengths[0].clone(), lengths.get(1).cloned().unwrap_or(CssLength::Zero),
    )] })
}

pub fn parse_individual_rotate(v: &str) -> Option<crate::types::CssTransform> {
    let values = individual_transform_values(v)?;
    if values.is_empty() { return Some(CssTransform::default()); }
    if values.len() != 1 { return None; }
    let value = values[0];
    let angle = parse_transform_angle(value)?;
    Some(CssTransform { ops: vec![TransformOp::Rotate(angle)] })
}

pub fn parse_individual_scale(v: &str) -> Option<crate::types::CssTransform> {
    let values = individual_transform_values(v)?;
    if values.is_empty() { return Some(CssTransform::default()); }
    if values.len() > 3 { return None; }
    let scales: Option<Vec<_>> = values.iter().map(|value| parse_transform_scale(value)).collect();
    let scales = scales?;
    Some(CssTransform { ops: vec![TransformOp::Scale(
        scales[0], scales.get(1).copied().unwrap_or(scales[0]),
    )] })
}

fn individual_transform_values(v: &str) -> Option<Vec<&str>> {
    let value = v.trim();
    if value.eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    if value.is_empty() {
        return None;
    }
    Some(split_top_level_whitespace(value))
}

/// Parse a CSS `transform-origin` into a pair of lengths from the reference
/// box's top-left corner — css-transforms-1 §transform-origin.
///
/// ⛔ NOT a 0..1 fraction. A percentage IS a fraction of the box, but a
/// `<length>` is a fixed offset: this returned the raw number for `px`, which
/// the matrix builder then multiplied by the box size, so `transform-origin:
/// 10px 10px` on a 200px box put the origin 2000px outside it — a `rotate()`
/// flung the element off-screen. Keeping the `CssLength` makes both kinds
/// resolve correctly against the same containing size (the box's own).
pub fn parse_transform_origin(v: &str) -> (CssLength, CssLength) {
    // css-transforms-1 §transform-origin: `left`/`right` name the HORIZONTAL
    // axis and `top`/`bottom` the vertical one whichever position they appear
    // in, while a length or `center` is positional — x first, then y. Treating
    // every first token as x made `transform-origin: top` mean `left`.
    let mut x: Option<CssLength> = None;
    let mut y: Option<CssLength> = None;
    let mut positional: Vec<CssLength> = Vec::new();
    // A third component is the z offset, which this engine does not use.
    for tok in split_top_level_whitespace(v).into_iter().take(2) {
        match tok.to_ascii_lowercase().as_str() {
            "left" => x = Some(CssLength::Percent(0.0)),
            "right" => x = Some(CssLength::Percent(100.0)),
            "top" => y = Some(CssLength::Percent(0.0)),
            "bottom" => y = Some(CssLength::Percent(100.0)),
            "center" => positional.push(CssLength::Percent(50.0)),
            _ => positional.push(match crate::css::value_parse::parse_length_checked(tok) {
                Some(l) if !l.is_auto() => l,
                _ => CssLength::Percent(50.0),
            }),
        }
    }
    for p in positional {
        if x.is_none() {
            x = Some(p);
        } else if y.is_none() {
            y = Some(p);
        }
    }
    (
        x.unwrap_or(CssLength::Percent(50.0)),
        y.unwrap_or(CssLength::Percent(50.0)),
    )
}

pub(crate) fn consume_parenthesized(src: &str) -> Option<(&str, &str)> {
    if !src.starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
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
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((&src[1..i], &src[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn split_transform_args(src: &str) -> Vec<&str> {
    super::value_parse::split_top_level_commas(src).into_iter().map(str::trim).collect()
}

fn split_top_level_whitespace(src: &str) -> Vec<&str> {
    split_top_level(src, char::is_whitespace)
}

fn split_top_level<F>(src: &str, mut is_separator: F) -> Vec<&str>
where
    F: FnMut(char) -> bool,
{
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote = None;
    let mut start: Option<usize> = None;
    let mut escape = false;
    for (i, ch) in src.char_indices() {
        if start.is_none() && !is_separator(ch) {
            start = Some(i);
        }
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
            _ if depth == 0 && is_separator(ch) => {
                if let Some(s) = start.take() {
                    if s < i {
                        out.push(src[s..i].trim());
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        let tail = src[s..].trim();
        if !tail.is_empty() {
            out.push(tail);
        }
    }
    out
}
