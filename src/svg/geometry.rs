//! SVG geometry and intrinsic sizing helpers.

use super::{SvgDocument, parse_svg_document};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgLength {
    Number(f32),
    Px(f32),
    Percent(f32),
    Em(f32),
    Rem(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SvgViewBox {
    pub min_x: f32,
    pub min_y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreserveAspectRatio {
    None,
    Meet { align_x: AlignX, align_y: AlignY },
    Slice { align_x: AlignX, align_y: AlignY },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignX {
    Min,
    Mid,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignY {
    Min,
    Mid,
    Max,
}

impl Default for PreserveAspectRatio {
    fn default() -> Self {
        Self::Meet {
            align_x: AlignX::Mid,
            align_y: AlignY::Mid,
        }
    }
}

pub fn intrinsic_size_from_markup(svg: &str) -> (f32, f32) {
    parse_svg_document(svg)
        .ok()
        .and_then(|doc| intrinsic_size(&doc))
        .unwrap_or((0.0, 0.0))
}

pub fn intrinsic_size(doc: &SvgDocument) -> Option<(f32, f32)> {
    let root = &doc.root;
    let width = root
        .attr_ascii_case_insensitive("width")
        .and_then(parse_svg_length)
        .and_then(definite_intrinsic_length);
    let height = root
        .attr_ascii_case_insensitive("height")
        .and_then(parse_svg_length)
        .and_then(definite_intrinsic_length);

    match (width, height) {
        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some((w, h)),
        _ => parse_view_box(root.attr_ascii_case_insensitive("viewBox"))
            .filter(|vb| vb.width > 0.0 && vb.height > 0.0)
            .map(|vb| (vb.width, vb.height)),
    }
}

/// Returns true if the SVG has a viewBox (an intrinsic aspect ratio) but no explicit
/// width and height attributes.
pub fn has_ratio_only(doc: &SvgDocument) -> bool {
    let root = &doc.root;
    let width = root
        .attr_ascii_case_insensitive("width")
        .and_then(parse_svg_length)
        .and_then(definite_intrinsic_length);
    let height = root
        .attr_ascii_case_insensitive("height")
        .and_then(parse_svg_length)
        .and_then(definite_intrinsic_length);
    let has_explicit_dims = width.is_some() && height.is_some();
    let has_viewbox = parse_view_box(root.attr_ascii_case_insensitive("viewBox"))
        .map(|vb| vb.width > 0.0 && vb.height > 0.0)
        .unwrap_or(false);
    !has_explicit_dims && has_viewbox
}

pub fn has_ratio_only_from_markup(svg: &str) -> bool {
    parse_svg_document(svg)
        .ok()
        .map(|doc| has_ratio_only(&doc))
        .unwrap_or(false)
}

pub fn parse_svg_length(input: &str) -> Option<SvgLength> {
    let token = input
        .trim()
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()
        .unwrap_or("");
    if token.is_empty() {
        return None;
    }

    if let Ok(value) = token.parse::<f32>() {
        return value.is_finite().then_some(SvgLength::Number(value));
    }
    use crate::types::CssLength;
    match crate::css::parse_length_checked(token)? {
        CssLength::Zero => Some(SvgLength::Number(0.0)),
        CssLength::Px(value) if value.is_finite() => Some(SvgLength::Px(value)),
        CssLength::Percent(value) if value.is_finite() => Some(SvgLength::Percent(value)),
        CssLength::Em(value) if value.is_finite() => Some(SvgLength::Em(value)),
        CssLength::Rem(value) if value.is_finite() => Some(SvgLength::Rem(value)),
        _ => None,
    }
}

pub fn definite_intrinsic_length(length: SvgLength) -> Option<f32> {
    match length {
        SvgLength::Number(v) | SvgLength::Px(v) if v > 0.0 => Some(v),
        _ => None,
    }
}

pub fn parse_view_box(value: Option<&str>) -> Option<SvgViewBox> {
    let value = value?;
    let parts: Vec<f32> = value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() == 4 {
        Some(SvgViewBox {
            min_x: parts[0],
            min_y: parts[1],
            width: parts[2],
            height: parts[3],
        })
    } else {
        None
    }
}

pub fn parse_preserve_aspect_ratio(value: Option<&str>) -> PreserveAspectRatio {
    let Some(value) = value else {
        return PreserveAspectRatio::default();
    };
    let mut parts = value.split_whitespace();
    let Some(align) = parts.next() else {
        return PreserveAspectRatio::default();
    };
    if align.eq_ignore_ascii_case("none") {
        return PreserveAspectRatio::None;
    }

    let (align_x, align_y) = match align {
        "xMinYMin" => (AlignX::Min, AlignY::Min),
        "xMidYMin" => (AlignX::Mid, AlignY::Min),
        "xMaxYMin" => (AlignX::Max, AlignY::Min),
        "xMinYMid" => (AlignX::Min, AlignY::Mid),
        "xMidYMid" => (AlignX::Mid, AlignY::Mid),
        "xMaxYMid" => (AlignX::Max, AlignY::Mid),
        "xMinYMax" => (AlignX::Min, AlignY::Max),
        "xMidYMax" => (AlignX::Mid, AlignY::Max),
        "xMaxYMax" => (AlignX::Max, AlignY::Max),
        _ => return PreserveAspectRatio::default(),
    };

    match parts.next() {
        Some(mode) if mode.eq_ignore_ascii_case("slice") => {
            PreserveAspectRatio::Slice { align_x, align_y }
        }
        _ => PreserveAspectRatio::Meet { align_x, align_y },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_reuse_css_scientific_notation_and_absolute_units() {
        assert_eq!(parse_svg_length("1e2"), Some(SvgLength::Number(100.0)));
        for token in ["1in", "2.54cm", "25.4mm", "101.6Q", "72pt", "6pc", "9.6e1PX"] {
            let SvgLength::Px(value) = parse_svg_length(token).unwrap() else { panic!("{token}") };
            assert!((value - 96.0).abs() < 0.0001, "{token}: {value}");
            assert!((super::super::source::parse_svg_length_px(token).unwrap() - 96.0).abs() < 0.0001);
        }
        assert_eq!(parse_svg_length("1e1%"), Some(SvgLength::Percent(10.0)));
        assert_eq!(parse_svg_length("1e-1em"), Some(SvgLength::Em(0.1)));
        for token in ["NaN", "inf", "1e99", "1e99px", "1garbage", "1e+px"] {
            assert_eq!(parse_svg_length(token), None, "{token}");
        }
        assert_eq!(super::super::source::parse_svg_length_px("100%"), None);
        assert_eq!(super::super::source::parse_svg_length_px("2em"), None);
        assert_eq!(intrinsic_size_from_markup("<svg width='1in' height='1e1px'/>") , (96.0, 10.0));
    }

    #[test]
    fn intrinsic_size_uses_definite_width_and_height() {
        assert_eq!(
            intrinsic_size_from_markup(r#"<svg width="24px" height="16"></svg>"#),
            (24.0, 16.0)
        );
    }

    #[test]
    fn intrinsic_size_ignores_contextual_lengths_and_uses_viewbox() {
        assert_eq!(
            intrinsic_size_from_markup(
                r#"<svg width="100%" height="2em" viewBox="0 0 30 20"></svg>"#
            ),
            (30.0, 20.0)
        );
    }

    #[test]
    fn preserve_aspect_ratio_defaults_to_xmidymid_meet() {
        assert_eq!(
            parse_preserve_aspect_ratio(None),
            PreserveAspectRatio::Meet {
                align_x: AlignX::Mid,
                align_y: AlignY::Mid
            }
        );
        assert_eq!(
            parse_preserve_aspect_ratio(Some("xMaxYMin slice")),
            PreserveAspectRatio::Slice {
                align_x: AlignX::Max,
                align_y: AlignY::Min
            }
        );
    }
}
