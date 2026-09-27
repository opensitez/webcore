//! `@keyframes` extraction.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── @keyframes extraction ────────────────────────────────────────────────────

/// Parse all `@keyframes` (and `@-webkit-keyframes`) blocks from a CSS string.
pub fn extract_keyframes(css: &str) -> HashMap<String, Vec<KeyframeStop>> {
    let cleaned = strip_css_comments(css);
    extract_keyframes_cleaned(&cleaned)
}

pub(crate) fn extract_keyframes_cleaned(css: &str) -> HashMap<String, Vec<KeyframeStop>> {
    let mut out: HashMap<String, Vec<KeyframeStop>> = HashMap::new();
    let mut s = css.trim();

    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }

        if !s.starts_with('@') {
            // Skip regular rules without recursing
            if let Some(brace) = s.find('{') {
                let (_, rest) = consume_block(&s[brace..]);
                s = rest;
            } else {
                break;
            }
            continue;
        }

        // Only lowercase a small prefix to identify the @-rule type
        let prefix = &s[..s.len().min(30)];
        let at_lower: String = prefix.to_ascii_lowercase();

        // Handle no-block @ rules
        if at_lower.starts_with("@import") || at_lower.starts_with("@charset") {
            if let Some(semi) = s.find(';') {
                s = &s[semi + 1..];
            } else {
                break;
            }
            continue;
        }

        let brace = match s.find('{') {
            Some(p) => p,
            None => {
                if let Some(semi) = s.find(';') {
                    s = &s[semi + 1..];
                } else {
                    break;
                }
                continue;
            }
        };
        let at_header = s[..brace].trim();
        let rest_from_brace = &s[brace..];
        let (inner_block, after_block) = consume_block(rest_from_brace);

        if at_lower.starts_with("@keyframes") || at_lower.starts_with("@-webkit-keyframes") {
            let prefix_len = if at_lower.starts_with("@-webkit-keyframes") {
                "@-webkit-keyframes".len()
            } else {
                "@keyframes".len()
            };
            let name = at_header[prefix_len..].trim().to_string();
            if !name.is_empty() {
                out.insert(name, parse_keyframe_stops(inner_block));
            }
        } else if at_lower.starts_with("@media")
            || at_lower.starts_with("@container")
            || at_lower.starts_with("@layer")
        {
            // Recurse for nested @keyframes (rare but spec-valid)
            out.extend(extract_keyframes_cleaned(inner_block));
        } else if at_lower.starts_with("@supports") {
            let condition = at_header["@supports".len()..].trim();
            if crate::css::parser::supports_condition_matches(condition) {
                out.extend(extract_keyframes_cleaned(inner_block));
            }
        }

        s = after_block;
    }
    out
}

/// Parse the body of a `@keyframes` block into a sorted list of stops.
fn parse_keyframe_stops(block: &str) -> Vec<KeyframeStop> {
    let mut stops: Vec<KeyframeStop> = Vec::new();
    let mut s = block.trim();

    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }

        let brace = match s.find('{') {
            Some(p) => p,
            None => break,
        };
        let selector = s[..brace].trim();
        let (decl_block, rest) = consume_block(&s[brace..]);
        s = rest;

        let (props, _) = parse_declarations_important(decl_block);
        let timing_fn = props.get("animation-timing-function")
            .and_then(|value| super::animation::parse_easing_checked(value));
        let prop_vec: Vec<(String, String)> = props
            .iter()
            .filter_map(|(k, v)| {
                if k == "animation-timing-function" {
                    return None;
                }
                // Normalize color values to rgba() so interpolation works.
                let is_color_prop = matches!(
                    k.as_str(),
                    "color"
                        | "background-color"
                        | "border-color"
                        | "border-top-color"
                        | "border-right-color"
                        | "border-bottom-color"
                        | "border-left-color"
                        | "outline-color"
                        | "fill"
                        | "stroke"
                );
                if is_color_prop {
                    if let Some(c) = parse_color(&v) {
                        return Some((
                            k.clone(),
                            format!(
                                "rgba({},{},{},{})",
                                c.r,
                                c.g,
                                c.b,
                                (c.a as f32 / 255.0 * 1000.0).round() / 1000.0
                            ),
                        ));
                    }
                }
                Some((k.clone(), v.clone()))
            })
            .collect();

        for sel in selector.split(',') {
            let sel = sel.trim();
            let Some(offset) = keyframe_selector_offset(sel) else {
                continue;
            };
            merge_keyframe_stop(&mut stops, offset, &prop_vec, timing_fn.as_ref());
        }
    }

    stops.sort_by(|a, b| {
        a.offset
            .partial_cmp(&b.offset)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    stops
}

fn keyframe_selector_offset(sel: &str) -> Option<f32> {
    let offset = match sel {
        "from" => 0.0,
        "to" => 1.0,
        s if s.ends_with('%') => {
            let pct = s[..s.len() - 1].trim().parse::<f32>().ok()?;
            if !(0.0..=100.0).contains(&pct) {
                return None;
            }
            pct / 100.0
        }
        _ => return None,
    };
    Some(offset)
}

fn merge_keyframe_stop(
    stops: &mut Vec<KeyframeStop>,
    offset: f32,
    properties: &[(String, String)],
    timing_fn: Option<&EasingFn>,
) {
    if let Some(stop) = stops
        .iter_mut()
        .find(|s| (s.offset - offset).abs() < 0.0001)
    {
        if let Some(timing_fn) = timing_fn { stop.timing_fn = Some(timing_fn.clone()); }
        for (name, value) in properties {
            if let Some((_, existing)) = stop.properties.iter_mut().find(|(k, _)| k == name) {
                *existing = value.clone();
            } else {
                stop.properties.push((name.clone(), value.clone()));
            }
        }
    } else {
        stops.push(KeyframeStop {
            offset,
            properties: properties.to_vec(),
            timing_fn: timing_fn.cloned(),
        });
    }
}
