//! Font-family and font-settings helpers.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Font utility helpers ──────────────────────────────────────────────────────

pub(crate) fn parse_font_variant_groups(value: &str, id: properties::PropertyId) -> Option<String> {
    use properties::PropertyId::*;
    let groups: &[&[&str]] = match id {
        FontVariantNumeric => &[
            &["lining-nums", "oldstyle-nums"],
            &["proportional-nums", "tabular-nums"],
            &["diagonal-fractions", "stacked-fractions"],
            &["ordinal"],
            &["slashed-zero"],
        ],
        FontVariantLigatures => &[
            &["common-ligatures", "no-common-ligatures"],
            &["discretionary-ligatures", "no-discretionary-ligatures"],
            &["historical-ligatures", "no-historical-ligatures"],
            &["contextual", "no-contextual"],
        ],
        FontVariantEastAsian => &[
            &[
                "jis78",
                "jis83",
                "jis90",
                "jis2004",
                "simplified",
                "traditional",
            ],
            &["full-width", "proportional-width"],
            &["ruby"],
        ],
        _ => return None,
    };
    let mut rest = value;
    let mut selected = [None; 5];
    let mut standalone = None;
    let mut count = 0;
    loop {
        let (trivia, _) = super::syntax::trivia_prefix(rest);
        rest = &rest[trivia..];
        if rest.is_empty() {
            break;
        }
        let (name, consumed, false) = super::syntax::name_token(rest)? else {
            return None;
        };
        rest = &rest[consumed..];
        count += 1;
        if name.eq_ignore_ascii_case("normal")
            || (id == FontVariantLigatures && name.eq_ignore_ascii_case("none"))
        {
            standalone = Some(if name.eq_ignore_ascii_case("none") {
                "none"
            } else {
                "normal"
            });
        } else {
            let (group, keyword) = groups.iter().enumerate().find_map(|(group, words)| {
                words
                    .iter()
                    .find(|word| name.eq_ignore_ascii_case(word))
                    .map(|word| (group, *word))
            })?;
            if selected[group].replace(keyword).is_some() {
                return None;
            }
        }
    }
    if let Some(keyword) = standalone {
        return (count == 1).then(|| keyword.to_owned());
    }
    (count > 0).then(|| selected.into_iter().flatten().collect::<Vec<_>>().join(" "))
}

/// Split a CSS `font-family` value into individual family names.
/// Handles quoted names with spaces and strips surrounding quote characters.
/// `"Times New Roman", Arial, sans-serif` → `["Times New Roman", "Arial", "sans-serif"]`
pub fn split_font_families(raw: &str) -> Vec<String> {
    let mut families = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for ch in raw.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if in_quotes && c == quote_char => {
                in_quotes = false;
            }
            ',' if !in_quotes => {
                let name = current.trim().to_string();
                if !name.is_empty() {
                    families.push(name);
                }
                current.clear();
            }
            c => {
                current.push(c);
            }
        }
    }
    let name = current.trim().to_string();
    if !name.is_empty() {
        families.push(name);
    }
    families
}

/// Map CSS system-font keywords to a generic CSS family name.
/// Returns `None` for regular named fonts that should be kept as-is.
pub fn resolve_system_font_keyword(name: &str) -> Option<&'static str> {
    match name {
        "system-ui" | "-apple-system" | "BlinkMacSystemFont" | "ui-sans-serif" | "ui-rounded" => {
            Some("sans-serif")
        }
        "ui-serif" => Some("serif"),
        "ui-monospace" => Some("monospace"),
        _ => None,
    }
}

// CSS Fonts 4 absolute weight and slope ranges.
pub(crate) const MIN_FONT_WEIGHT: f32 = 1.0;
pub(crate) const MAX_FONT_WEIGHT: f32 = 1000.0;
pub(crate) const MAX_FONT_OBLIQUE_ANGLE_DEG: f32 = 90.0;

pub(crate) fn font_keyword(value: &str) -> Option<String> {
    let value = value.trim();
    let (name, end, function) = super::syntax::name_token(value)?;
    (!function && end == value.len()).then(|| name.to_ascii_lowercase())
}

pub(crate) fn parse_absolute_font_weight_number(value: &str) -> Option<f32> {
    match font_keyword(value).as_deref() {
        Some("normal") => return Some(FontWeight::Normal.value() as f32),
        Some("bold") => return Some(FontWeight::Bold.value() as f32),
        _ => {}
    }
    let number = super::calc::parse_css_number(value)?;
    if super::calc::is_math_function(value.trim()) {
        Some(number.clamp(MIN_FONT_WEIGHT, MAX_FONT_WEIGHT))
    } else {
        (MIN_FONT_WEIGHT..=MAX_FONT_WEIGHT)
            .contains(&number)
            .then_some(number)
    }
}

pub(crate) fn parse_absolute_font_weight(value: &str) -> Option<FontWeight> {
    match font_keyword(value).as_deref() {
        Some("normal") => Some(FontWeight::Normal),
        Some("bold") => Some(FontWeight::Bold),
        _ => parse_absolute_font_weight_number(value)
            .map(|number| FontWeight::Value(number.round() as u16)),
    }
}

pub(crate) fn parse_font_width(value: &str) -> Option<f32> {
    // Keyword percentages are specified by CSS Fonts, not inferred from font data.
    let keyword = match font_keyword(value).as_deref() {
        Some("ultra-condensed") => Some(50.0),
        Some("extra-condensed") => Some(62.5),
        Some("condensed") => Some(75.0),
        Some("semi-condensed") => Some(87.5),
        Some("normal") => Some(100.0),
        Some("semi-expanded") => Some(112.5),
        Some("expanded") => Some(125.0),
        Some("extra-expanded") => Some(150.0),
        Some("ultra-expanded") => Some(200.0),
        _ => None,
    };
    keyword.or_else(|| parse_nonnegative_font_percentage(value))
}

pub(crate) fn parse_nonnegative_font_percentage(value: &str) -> Option<f32> {
    let number = super::calc::parse_css_percentage(value)?;
    if super::calc::is_math_function(value.trim()) {
        Some(number.max(0.0))
    } else {
        (number >= 0.0).then_some(number)
    }
}

/// Parse a CSS `font-variation-settings` value into a list of `(axis-tag, value)` pairs.
/// Accepts `normal` (returns empty) or `"wght" 700, "wdth" 75`.
pub fn parse_variation_settings(v: &str) -> Vec<(String, f32)> {
    parse_variation_settings_checked(v).unwrap_or_default()
}

/// Parse a CSS `font-feature-settings` value into `(feature-tag, value)` pairs.
/// Accepts `normal` (empty), `"kern"` (= 1), `"liga" on`, `"liga" off`, `"calt" 2`.
pub fn parse_feature_settings(v: &str) -> Vec<(String, u32)> {
    parse_feature_settings_checked(v).unwrap_or_default()
}

fn settings_keyword(value: &str, keyword: &str) -> bool {
    let value = value.trim();
    super::syntax::name_token(value).is_some_and(|(name, end, function)| {
        !function && end == value.len() && name.eq_ignore_ascii_case(keyword)
    })
}

fn settings_tag(entry: &str) -> Option<(String, &str)> {
    let (tag, rest) = super::apply::consume_css_string(entry.trim())?;
    (tag.len() == 4 && tag.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
        .then_some((tag, rest.trim()))
}

pub(crate) fn parse_variation_settings_checked(value: &str) -> Option<Vec<(String, f32)>> {
    if settings_keyword(value, "normal") {
        return Some(Vec::new());
    }
    super::value_parse::split_top_level_commas(value)
        .into_iter()
        .map(|entry| {
            let (tag, rest) = settings_tag(entry)?;
            Some((tag, super::calc::parse_css_number(rest)?))
        })
        .collect()
}

pub(crate) fn parse_feature_settings_checked(value: &str) -> Option<Vec<(String, u32)>> {
    if settings_keyword(value, "normal") {
        return Some(Vec::new());
    }
    super::value_parse::split_top_level_commas(value)
        .into_iter()
        .map(|entry| {
            let (tag, rest) = settings_tag(entry)?;
            let number = if rest.is_empty() || settings_keyword(rest, "on") {
                1
            } else if settings_keyword(rest, "off") {
                0
            } else if let Ok(integer) = rest.parse::<u32>() {
                integer
            } else {
                // Calculations clamp to the descriptor's nonnegative range.
                let integer = super::calc::parse_css_integer(rest)?;
                if rest.parse::<i32>().is_ok() {
                    return None;
                }
                integer.max(0) as u32
            };
            Some((tag, number))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_weight_and_width_share_checked_application_and_supports() {
        use super::super::properties::PropertyId;
        use crate::types::CssValue;
        let weight = super::super::rule::pre_parse_value(PropertyId::FontWeight, "calc(400 + 150)");
        assert!(matches!(
            weight,
            CssValue::FontWeight(FontWeight::Value(550))
        ));
        let width = super::super::rule::pre_parse_value(PropertyId::FontStretch, "calc(80% + 15%)");
        assert!(matches!(width, CssValue::Number(number) if number == 95.0));
        let mut style = ComputedStyle::default();
        super::super::apply_css_value(&mut style, PropertyId::FontWeight, &weight);
        super::super::apply_css_value(&mut style, PropertyId::FontStretch, &width);
        super::super::apply_property(&mut style, "font-weight", "1001");
        super::super::apply_property(&mut style, "font-width", "-10%");
        assert_eq!(style.font_weight.value(), 550);
        assert_eq!(style.font_stretch, 95.0);
        super::super::apply_property(&mut style, "font-style", "italic");
        super::super::apply_property(&mut style, "font-style", "oblique 100deg");
        assert_eq!(style.font_style, FontStyle::Italic);
        for condition in [
            "(font-width: calc(80% + 15%))",
            "(font-weight: calc(400 + 150))",
            r"(font-style: o\62 lique 20deg)",
        ] {
            assert!(
                super::super::parser::supports_condition_matches(condition),
                "{condition}"
            );
        }
        for condition in [
            "(font-width: -10%)",
            "(font-weight: 1001)",
            "(font-style: oblique 100deg)",
        ] {
            assert!(
                !super::super::parser::supports_condition_matches(condition),
                "{condition}"
            );
        }
    }

    #[test]
    fn invalid_font_settings_preserve_previous_property_and_descriptor_values() {
        let mut style = ComputedStyle::default();
        super::super::apply_property(&mut style, "font-feature-settings", r#""kern" 0"#);
        super::super::apply_property(&mut style, "font-feature-settings", r#""kern" garbage"#);
        assert_eq!(style.rare().font_feature_settings, vec![("kern".into(), 0)]);
        super::super::apply_property(&mut style, "font-variation-settings", r#""wdth" 80"#);
        super::super::apply_property(
            &mut style,
            "font-variation-settings",
            r#""wdth" 80, invalid"#,
        );
        assert_eq!(
            style.rare().font_variation_settings,
            vec![("wdth".into(), 80.0)]
        );

        let mut faces = Vec::new();
        super::super::extract_font_faces(
            r#"@font-face {
            font-family: Test; src: url(test.woff2);
            font-feature-settings: "kern" 0;
            font-feature-settings: "kern" invalid;
            font-variation-settings: "wdth" 80;
            font-variation-settings: "abc" 20;
        }"#,
            &mut faces,
        );
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].feature_settings.as_deref(), Some(r#""kern" 0"#));
        assert_eq!(faces[0].variation_settings.as_deref(), Some(r#""wdth" 80"#));
    }

    #[test]
    fn font_settings_validate_complete_lists_and_decoded_tags() {
        assert_eq!(
            parse_feature_settings_checked(r#""\6b ern" OFF, "liga" calc(1 + 1)"#),
            Some(vec![("kern".into(), 0), ("liga".into(), 2)])
        );
        assert_eq!(
            parse_variation_settings_checked(r#""wght" calc(400 + 50), "wdth" min(80, 90)"#),
            Some(vec![("wght".into(), 450.0), ("wdth".into(), 80.0)])
        );
        assert_eq!(parse_feature_settings_checked(r"n\6f rmal"), Some(vec![]));
        for value in [
            r#""kern" nonsense"#,
            r#""kern" -1"#,
            r#""kern" 1.5"#,
            r#""abc" 1"#,
            r#""éabc" 1"#,
            r#""kern" 1,"#,
            r#""kern" 1, invalid"#,
        ] {
            assert!(parse_feature_settings_checked(value).is_none(), "{value}");
        }
        for value in [
            r#""wght" NaN"#,
            r#""wght" 1px"#,
            r#""wght" 1,"#,
            r#""abc" 1"#,
            r#""wght" 1, bad"#,
        ] {
            assert!(parse_variation_settings_checked(value).is_none(), "{value}");
        }
    }
}
