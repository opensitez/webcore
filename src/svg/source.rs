//! SVG source assembly and intrinsic sizing helpers.

use crate::dom::attrs::AttrMap;

/// Parse an SVG length that is definite at parse time.
///
/// SVG presentation attributes accept unitless lengths as px, and `px` lengths
/// are directly usable. Contextual lengths like `%`/`em` must be left to CSS
/// layout instead of being coerced by their numeric prefix.
pub(crate) fn parse_svg_length_px(s: &str) -> Option<f32> {
    let token = s
        .trim()
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()
        .unwrap_or("");
    if token.is_empty() || token.ends_with('%') {
        return None;
    }
    let num_end = token
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        .last()
        .map(|(idx, c)| idx + c.len_utf8())
        .unwrap_or(0);
    if num_end == 0 {
        return None;
    }
    let (num, unit) = token.split_at(num_end);
    if !unit.is_empty() && !unit.eq_ignore_ascii_case("px") {
        return None;
    }
    let px = num.parse::<f32>().ok()?;
    if px.is_finite() && px > 0.0 {
        Some(px)
    } else {
        None
    }
}

/// Parse definite pixels from a string like "20px", "512", "20px;height:10px".
pub(crate) fn parse_px(s: &str) -> Option<u32> {
    parse_svg_length_px(s).map(|n| n.round() as u32)
}

/// Parse a viewBox attribute value "min-x min-y width height" -> (width, height).
pub(crate) fn parse_viewbox_value(val: Option<&str>) -> Option<(u32, u32)> {
    let val = val?;
    let parts: Vec<f32> = val
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 4 {
        Some((parts[2].round() as u32, parts[3].round() as u32))
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InlineSvgSource {
    pub markup: String,
    pub viewbox_w: f32,
    pub viewbox_h: f32,
    pub explicit_w: Option<u32>,
    pub explicit_h: Option<u32>,
}

pub(crate) fn build_inline_svg_source(attrs: &AttrMap, body: &str) -> InlineSvgSource {
    let mut svg_tag = String::from("<svg");
    for (k, v) in attrs.iter() {
        svg_tag.push_str(&format!(" {}=\"{}\"", k, v));
    }
    if !svg_tag.contains("xmlns=") {
        svg_tag.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    if body.contains("xlink:") && !svg_tag.contains("xmlns:xlink") {
        svg_tag.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    }
    svg_tag.push('>');

    let vb_str = attrs.get("viewBox").or_else(|| attrs.get("viewbox"));
    let (viewbox_w, viewbox_h) = parse_viewbox_value(vb_str.map(|s| s.as_str()))
        .map(|(w, h)| (w as f32, h as f32))
        .unwrap_or((0.0, 0.0));

    InlineSvgSource {
        markup: format!("{}{}</svg>", svg_tag, body),
        viewbox_w,
        viewbox_h,
        explicit_w: attrs.get("width").and_then(|s| parse_px(s)),
        explicit_h: attrs.get("height").and_then(|s| parse_px(s)),
    }
}
