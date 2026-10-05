//! `@font-face` declarations.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── @font-face declaration ───────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontFaceSourceKind {
    Url(String),
    Local(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFaceSource {
    pub kind: FontFaceSourceKind,
    pub formats: Vec<String>,
    pub techs: Vec<String>,
}

fn normalize_font_range(value: &str, parse: fn(&str) -> Option<f32>, unit: &str) -> Option<String> {
    if super::font::font_keyword(value).as_deref() == Some("auto") {
        return Some("auto".into());
    }
    let components = super::syntax::split_component_values(value);
    if !(1..=2).contains(&components.len()) {
        return None;
    }
    let first = parse(components[0])?;
    let mut values = vec![first];
    if components.len() == 2 {
        values.push(parse(components[1])?);
        values.sort_by(f32::total_cmp);
    }
    Some(
        values
            .into_iter()
            .map(|number| super::calc::serialize_math_literal(number, unit))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

pub(crate) fn normalize_font_style(value: &str, descriptor: bool) -> Option<String> {
    let components = super::syntax::split_component_values(value);
    let keyword = super::font::font_keyword(components.first()?)?;
    if !descriptor && matches!(keyword.as_str(), "left" | "right") {
        return None;
    }
    if matches!(keyword.as_str(), "normal" | "italic" | "left" | "right")
        || (descriptor && keyword == "auto")
    {
        return (components.len() == 1).then_some(keyword);
    }
    let max_components = if descriptor { 3 } else { 2 };
    if keyword != "oblique" || components.len() > max_components {
        return None;
    }
    let mut angles = Vec::new();
    for component in &components[1..] {
        let number = super::calc::parse_css_angle_deg(component)?;
        let limit = super::font::MAX_FONT_OBLIQUE_ANGLE_DEG;
        let angle = if super::calc::is_math_function(component) {
            number.clamp(-limit, limit)
        } else if (-limit..=limit).contains(&number) {
            number
        } else {
            return None;
        };
        angles.push(angle);
    }
    angles.sort_by(f32::total_cmp);
    let mut normalized = keyword;
    for angle in angles {
        normalized.push(' ');
        normalized.push_str(&super::calc::serialize_math_literal(angle, "deg"));
    }
    Some(normalized)
}

fn normalize_font_metric(value: &str, allow_normal: bool) -> Option<String> {
    if allow_normal && super::font::font_keyword(value).as_deref() == Some("normal") {
        return Some("normal".into());
    }
    super::font::parse_nonnegative_font_percentage(value)
        .map(|number| super::calc::serialize_math_literal(number, "%"))
}

fn normalize_font_language(value: &str) -> Option<String> {
    if super::font::font_keyword(value).as_deref() == Some("normal") {
        return Some("normal".into());
    }
    let (_, rest) = super::apply::consume_css_string(value)?;
    rest.trim().is_empty().then(|| value.trim().to_string())
}

pub(crate) fn parse_font_face_body(body: &str) -> Option<FontFaceDecl> {
    let (declarations, _) = super::parse_declarations_important(body);
    let mut face = FontFaceDecl::default();
    for (property, value) in &declarations {
        match property.as_str() {
            "font-family" => {
                if let Some(family) = parse_font_face_family(value) {
                    face.family = family;
                }
            }
            "src" => {
                let sources: Vec<_> = parse_font_face_sources(value)
                    .into_iter()
                    .filter(font_source_supported)
                    .collect();
                if !sources.is_empty() {
                    face.sources = sources;
                    face.src = value.clone();
                }
            }
            "font-weight" => {
                if let Some(weight) =
                    normalize_font_range(value, super::font::parse_absolute_font_weight_number, "")
                {
                    face.weight = Some(weight);
                }
            }
            "font-style" => {
                if let Some(style) = normalize_font_style(value, true) {
                    face.style = Some(style);
                }
            }
            "font-width" | "font-stretch" => {
                if let Some(width) = normalize_font_range(value, super::font::parse_font_width, "%")
                {
                    face.stretch = Some(width);
                }
            }
            "font-display" => {
                if let Some(display) = parse_font_display(value) {
                    face.display = Some(display);
                }
            }
            "unicode-range" => {
                if parse_unicode_ranges_checked(value).is_some() {
                    face.unicode_range = Some(value.clone());
                }
            }
            "size-adjust" => {
                if let Some(metric) = normalize_font_metric(value, false) {
                    face.size_adjust = Some(metric);
                }
            }
            "ascent-override" | "descent-override" | "line-gap-override" => {
                if let Some(metric) = normalize_font_metric(value, true) {
                    let field = match property.as_str() {
                        "ascent-override" => &mut face.ascent_override,
                        "descent-override" => &mut face.descent_override,
                        _ => &mut face.line_gap_override,
                    };
                    *field = Some(metric);
                }
            }
            "font-feature-settings" => {
                if super::font::parse_feature_settings_checked(value).is_some() {
                    face.feature_settings = Some(value.clone());
                }
            }
            "font-variation-settings" => {
                if super::font::parse_variation_settings_checked(value).is_some() {
                    face.variation_settings = Some(value.clone());
                }
            }
            "font-language-override" => {
                if let Some(language) = normalize_font_language(value) {
                    face.language_override = Some(language);
                }
            }
            _ => {}
        }
    }
    (!face.family.is_empty() && !face.sources.is_empty()).then_some(face)
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
    let normalized = super::syntax::normalize_input(src);
    let comments = normalized
        .contains("/*")
        .then(|| super::syntax::normalize_comments(&normalized));
    super::value_parse::split_top_level_commas(comments.as_deref().unwrap_or(&normalized))
        .into_iter()
        .filter_map(parse_font_face_source)
        .collect()
}

pub(crate) fn supports_font_format(format: &str) -> bool {
    matches!(
        format.to_ascii_lowercase().as_str(),
        "woff2"
            | "woff"
            | "embedded-opentype"
            | "eot"
            | "opentype"
            | "truetype"
            | "collection"
            | "font/woff2"
            | "font/woff"
            | "application/vnd.ms-fontobject"
            | "font/otf"
            | "font/ttf"
    )
}

pub(crate) fn supports_font_tech(tech: &str) -> bool {
    matches!(
        tech.to_ascii_lowercase().as_str(),
        "features-opentype" | "features-aat" | "variations" | "variations-opentype"
    )
}

pub(crate) fn font_source_supported(source: &FontFaceSource) -> bool {
    (source.formats.is_empty()
        || source
            .formats
            .iter()
            .any(|format| supports_font_format(format)))
        && source.techs.iter().all(|tech| supports_font_tech(tech))
}

pub fn unicode_range_intersects_text(range: Option<&str>, text: &str) -> bool {
    let Some(range) = range else {
        return true;
    };
    let range = range.trim();
    if range.is_empty() || text.is_empty() {
        return true;
    }
    let ranges = parse_unicode_ranges(range);
    if ranges.is_empty() {
        return true;
    }
    text.chars().any(|ch| {
        let cp = ch as u32;
        ranges.iter().any(|(start, end)| cp >= *start && cp <= *end)
    })
}

pub(crate) struct UnicodeTextCoverage {
    words: Vec<u64>,
    empty: bool,
}

impl UnicodeTextCoverage {
    pub(crate) fn new(text: &str) -> Self {
        let mut words = vec![0; 0x110000 / 64];
        for ch in text.chars() {
            let cp = ch as usize;
            words[cp / 64] |= 1 << (cp % 64);
        }
        Self {
            words,
            empty: text.is_empty(),
        }
    }

    pub(crate) fn intersects(&self, range: Option<&str>) -> bool {
        let Some(range) = range else {
            return true;
        };
        if self.empty || range.trim().is_empty() {
            return true;
        }
        let ranges = parse_unicode_ranges(range);
        if ranges.is_empty() {
            return true;
        }
        ranges.into_iter().any(|(start, end)| {
            let start = start.min(0x10ffff) as usize;
            let end = end.min(0x10ffff) as usize;
            if start > end {
                return false;
            }
            let first = start / 64;
            let last = end / 64;
            (first..=last).any(|word| {
                let low = if word == first {
                    u64::MAX << (start % 64)
                } else {
                    u64::MAX
                };
                let high = if word == last {
                    u64::MAX >> (63 - end % 64)
                } else {
                    u64::MAX
                };
                self.words[word] & low & high != 0
            })
        })
    }
}

pub fn parse_font_display(value: &str) -> Option<String> {
    let keyword = descriptor_ident(value.trim())?;
    matches!(
        keyword.as_str(),
        "auto" | "block" | "swap" | "fallback" | "optional"
    )
    .then_some(keyword)
}

pub(crate) fn unicode_range_intersects_latin(range: Option<&str>) -> bool {
    let Some(range) = range else {
        return true;
    };
    let ranges = parse_unicode_ranges(range);
    ranges.is_empty() || ranges.iter().any(|(start, _end)| *start <= 0x00ff)
}

fn parse_unicode_ranges(range: &str) -> Vec<(u32, u32)> {
    parse_unicode_ranges_checked(range).unwrap_or_default()
}

pub(crate) fn parse_unicode_ranges_checked(range: &str) -> Option<Vec<(u32, u32)>> {
    range
        .split(',')
        .map(|part| parse_unicode_range_part(part.trim()))
        .collect()
}

fn parse_unicode_range_part(part: &str) -> Option<(u32, u32)> {
    let body = part
        .strip_prefix("U+")
        .or_else(|| part.strip_prefix("u+"))?;
    if body.is_empty() {
        return None;
    }
    if body.contains('?') {
        let wildcard = body.find('?')?;
        if !body[wildcard..].bytes().all(|byte| byte == b'?') {
            return None;
        }
        let mut start = String::with_capacity(body.len());
        let mut end = String::with_capacity(body.len());
        for ch in body.chars() {
            if ch == '?' {
                start.push('0');
                end.push('F');
            } else if ch.is_ascii_hexdigit() {
                start.push(ch);
                end.push(ch);
            } else {
                return None;
            }
        }
        return parse_codepoint(&start).zip(parse_codepoint(&end));
    }
    if let Some((start, end)) = body.split_once('-') {
        let start = parse_codepoint(start)?;
        let end = parse_codepoint(end)?;
        if start <= end {
            Some((start, end))
        } else {
            None
        }
    } else {
        parse_codepoint(body).map(|cp| (cp, cp))
    }
}

fn parse_codepoint(hex: &str) -> Option<u32> {
    if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let cp = u32::from_str_radix(hex, 16).ok()?;
    (cp <= 0x10ffff).then_some(cp)
}

fn parse_font_face_source(source: &str) -> Option<FontFaceSource> {
    let source = source.trim();
    let (kind, rest) = if let Some((value, end)) = super::apply::parse_url_function(source) {
        (FontFaceSourceKind::Url(value), &source[end..])
    } else if let Some((value, rest)) = consume_function(source, "local") {
        if !rest.trim().is_empty() {
            return None;
        }
        return Some(FontFaceSource {
            kind: FontFaceSourceKind::Local(parse_font_face_family(value)?),
            formats: Vec::new(),
            techs: Vec::new(),
        });
    } else {
        return None;
    };
    let mut formats = Vec::new();
    let mut techs = Vec::new();
    let mut rest = rest.trim();
    if let Some((value, next)) = consume_function(rest, "format") {
        let value = value.trim();
        let format = if let Some((string, tail)) = super::apply::consume_css_string(value) {
            if !tail.trim().is_empty() {
                return None;
            }
            string.to_ascii_lowercase()
        } else {
            let format = descriptor_ident(value)?;
            if !matches!(
                format.as_str(),
                "collection"
                    | "embedded-opentype"
                    | "opentype"
                    | "svg"
                    | "truetype"
                    | "woff"
                    | "woff2"
            ) {
                return None;
            }
            format
        };
        match format.as_str() {
            "woff2-variations"
            | "woff-variations"
            | "truetype-variations"
            | "opentype-variations" => {
                formats.push(format.trim_end_matches("-variations").to_string());
                techs.push("variations".to_string());
            }
            _ => formats.push(format),
        }
        rest = next.trim();
    }
    if let Some((value, next)) = consume_function(rest, "tech") {
        for item in super::value_parse::split_top_level_commas(value) {
            let tech = descriptor_ident(item.trim())?;
            if !matches!(
                tech.as_str(),
                "features-opentype"
                    | "features-aat"
                    | "features-graphite"
                    | "color-colrv0"
                    | "color-colrv1"
                    | "color-svg"
                    | "color-sbix"
                    | "color-cbdt"
                    | "variations"
                    | "palettes"
                    | "incremental"
            ) {
                return None;
            }
            techs.push(tech);
        }
        rest = next.trim();
    }
    if !rest.is_empty() {
        return None;
    }
    Some(FontFaceSource {
        kind,
        formats,
        techs,
    })
}

fn consume_function<'a>(src: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let src = src.trim_start();
    let (actual, start, function) = super::syntax::name_token(src)?;
    if !function || !actual.eq_ignore_ascii_case(name) {
        return None;
    }
    let (body, end) = super::syntax::function_body(src, start)?;
    Some((body, &src[end..]))
}

fn descriptor_ident(value: &str) -> Option<String> {
    let (name, end, function) = super::syntax::name_token(value)?;
    (!function && end == value.len()).then(|| name.to_ascii_lowercase())
}

pub(crate) fn parse_font_face_family(value: &str) -> Option<String> {
    let value = value.trim();
    if let Some((family, rest)) = super::apply::consume_css_string(value) {
        return rest.trim().is_empty().then_some(family);
    }
    let mut names = Vec::new();
    for token in super::syntax::split_component_values(value) {
        let (name, end, function) = super::syntax::name_token(token)?;
        if function
            || end != token.len()
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "initial"
                    | "inherit"
                    | "unset"
                    | "revert"
                    | "revert-layer"
                    | "default"
                    | "serif"
                    | "sans-serif"
                    | "monospace"
                    | "cursive"
                    | "fantasy"
                    | "system-ui"
                    | "ui-serif"
                    | "ui-sans-serif"
                    | "ui-monospace"
                    | "ui-rounded"
                    | "math"
                    | "emoji"
                    | "fangsong"
            )
        {
            return None;
        }
        names.push(name.into_owned());
    }
    (!names.is_empty()).then(|| names.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_face_numeric_descriptors_normalize_math_ranges_and_aliases() {
        let face = parse_font_face_body(
            r#"
            font-family: Example; src: url(example.woff2);
            font-weight: 800 300;
            font-style: o\62 lique 30deg -30deg;
            font-stretch: condensed;
            font-width: min(75%, 90%) calc(100% + 25%);
            size-adjust: calc(80% + 20%);
            ascent-override: max(70%, calc(40% + 50%));
            descent-override: calc(-10%);
            line-gap-override: n\6f rmal;
            font-language-override: "TRK";
        "#,
        )
        .unwrap();
        assert_eq!(face.weight.as_deref(), Some("300 800"));
        assert_eq!(face.style.as_deref(), Some("oblique -30deg 30deg"));
        assert_eq!(face.stretch.as_deref(), Some("75% 125%"));
        assert_eq!(face.size_adjust.as_deref(), Some("100%"));
        assert_eq!(face.ascent_override.as_deref(), Some("90%"));
        assert_eq!(face.descent_override.as_deref(), Some("0%"));
        assert_eq!(face.line_gap_override.as_deref(), Some("normal"));
        assert_eq!(face.language_override.as_deref(), Some(r#""TRK""#));
        let clamped = parse_font_face_body(
            r#"font-family: Example; src: url(example.woff2);
            font-weight: calc(2000) calc(0); font-width: calc(-10%);
            font-style: oblique calc(180deg);"#,
        )
        .unwrap();
        assert_eq!(clamped.weight.as_deref(), Some("1 1000"));
        assert_eq!(clamped.stretch.as_deref(), Some("0%"));
        assert_eq!(clamped.style.as_deref(), Some("oblique 90deg"));
    }

    #[test]
    fn font_face_invalid_numeric_descriptors_preserve_valid_values() {
        let face = parse_font_face_body(
            r#"font-family: Example; src: url(example.woff2);
            font-weight: bold; font-weight: bolder; font-weight: 1001; font-weight: 400px;
            font-style: italic; font-style: normal 20deg; font-style: oblique 91deg;
            font-width: expanded; font-stretch: -1%; font-width: 100;
            size-adjust: 120%; size-adjust: normal; size-adjust: -10%;
            ascent-override: 90%; ascent-override: 90px;
            descent-override: 20%; descent-override: 20% 30%;
            line-gap-override: normal; line-gap-override: calc(10px);
            font-language-override: "ENG"; font-language-override: ENG;
            font-language-override: "TRK" garbage;
        "#,
        )
        .unwrap();
        assert_eq!(face.weight.as_deref(), Some("700"));
        assert_eq!(face.style.as_deref(), Some("italic"));
        assert_eq!(face.stretch.as_deref(), Some("125%"));
        assert_eq!(face.size_adjust.as_deref(), Some("120%"));
        assert_eq!(face.ascent_override.as_deref(), Some("90%"));
        assert_eq!(face.descent_override.as_deref(), Some("20%"));
        assert_eq!(face.line_gap_override.as_deref(), Some("normal"));
        assert_eq!(face.language_override.as_deref(), Some(r#""ENG""#));
    }

    #[test]
    fn font_face_unicode_ranges_validate_the_complete_descriptor() {
        assert_eq!(
            parse_unicode_ranges_checked("U+4??, u+1000-10FFFF"),
            Some(vec![(0x400, 0x4ff), (0x1000, 0x10ffff)])
        );
        for range in [
            "U+?A",
            "U+41, bad",
            "U+41,",
            "U+ 41",
            "U+40 - 50",
            "U+50-40",
            "U+110000",
            "U+??????",
        ] {
            assert!(parse_unicode_ranges_checked(range).is_none(), "{range}");
        }
        let mut faces = Vec::new();
        super::super::extract_font_faces(
            r#"@font-face {
            font-family: Example; src: url(example.woff2);
            unicode-range: U+41-5A; unicode-range: U+41, invalid;
            font-display: s\77 ap; font-display: invalid;
        }"#,
            &mut faces,
        );
        assert_eq!(faces[0].unicode_range.as_deref(), Some("U+41-5A"));
        assert_eq!(faces[0].display.as_deref(), Some("swap"));
    }

    #[test]
    fn font_face_sources_use_css_tokens_and_recover_by_candidate() {
        let sources = parse_font_face_sources(
            r#"ééé, url(a.woff2) garbage,
            local("Arial") format("woff2"), url(a.woff2) format("woff2", "woff"),
            url(a.woff2) tech(variations) format("woff2"),
            url(a.woff2) tech(unknown), url(a.woff2) format ("woff2"),
            u\72 l("f\6f nt.woff2") f\6f rmat(woff2) tech(variations),
            l\6f cal(My\20 Font), url(legacy.woff2) format("woff2-variations")"#,
        );
        assert_eq!(sources.len(), 3);
        assert_eq!(
            sources[0].kind,
            FontFaceSourceKind::Url("font.woff2".into())
        );
        assert_eq!(sources[0].formats, vec!["woff2"]);
        assert_eq!(sources[0].techs, vec!["variations"]);
        assert_eq!(sources[1].kind, FontFaceSourceKind::Local("My Font".into()));
        assert_eq!(sources[2].formats, vec!["woff2"]);
        assert_eq!(sources[2].techs, vec!["variations"]);
        assert_eq!(
            parse_font_face_sources("url(font.woff2)/**/format(woff2)").len(),
            1
        );
        assert!(
            parse_font_face_sources("local(serif), local(12Invalid), local(Arial, Helvetica)")
                .is_empty()
        );
    }

    #[test]
    fn font_face_invalid_descriptors_preserve_valid_fallbacks() {
        let mut faces = Vec::new();
        super::super::extract_font_faces(
            r#"@font-face {
            font-family: Good Font; font-family: serif;
            src: url(good.woff2); src: url(bad.woff2) garbage;
            src: url(unsupported.woff2) tech(color-COLRv1);
        }
        @font-face { font-family: NoSource; }
        @font-face { src: url(no-family.woff2); }
        @font-face { font-family: Invalid; src: ééé; }"#,
            &mut faces,
        );
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].family, "Good Font");
        assert_eq!(
            faces[0].sources[0].kind,
            FontFaceSourceKind::Url("good.woff2".into())
        );
    }
}
