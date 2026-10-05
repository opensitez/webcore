//! `@keyframes` extraction.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct KeyframeRule {
    pub name: String,
    pub stops: Arc<Vec<KeyframeStop>>,
    pub media_condition: MediaConditions,
    pub layer: String,
    pub author_origin: bool,
}

// ─── @keyframes extraction ────────────────────────────────────────────────────

/// Parse all `@keyframes` (and `@-webkit-keyframes`) blocks from a CSS string.
pub fn extract_keyframes(css: &str) -> HashMap<String, Vec<KeyframeStop>> {
    let cleaned = strip_css_comments(css);
    extract_keyframes_cleaned(&cleaned)
}

pub(crate) fn extract_keyframes_cleaned(css: &str) -> HashMap<String, Vec<KeyframeStop>> {
    let mut out: HashMap<String, Vec<KeyframeStop>> = HashMap::new();
    for rule in super::parser::global_rule_blocks(css) {
        if rule.name.as_ref().is_some_and(|name| {
            name.eq_ignore_ascii_case("keyframes") || name.eq_ignore_ascii_case("-webkit-keyframes")
        }) {
            if let Some(name) = keyframes_name(rule.prelude) {
                out.insert(name, parse_keyframe_stops(rule.body));
            }
        }
    }
    out
}

pub(super) fn keyframes_name(prelude: &str) -> Option<String> {
    if matches!(prelude.as_bytes().first(), Some(b'\'' | b'"')) {
        let (name, rest) = super::apply::consume_css_string(prelude)?;
        return (!name.is_empty() && rest.trim().is_empty()).then_some(name);
    }
    let (name, consumed, function) = super::syntax::name_token(prelude)?;
    if function
        || consumed != prelude.len()
        || [
            "none",
            "initial",
            "inherit",
            "unset",
            "revert",
            "revert-layer",
            "default",
        ]
        .iter()
        .any(|keyword| name.eq_ignore_ascii_case(keyword))
    {
        return None;
    }
    Some(name.into_owned())
}

/// Parse the body of a `@keyframes` block into a sorted list of stops.
pub(super) fn parse_keyframe_stops(block: &str) -> Vec<KeyframeStop> {
    let mut stops: Vec<KeyframeStop> = Vec::new();
    for rule in super::parser::rule_blocks(block) {
        if rule.name.is_some() {
            continue;
        }
        let Some(offsets) = rule
            .prelude
            .split(',')
            .map(|selector| keyframe_selector_offset(selector.trim()))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };

        let (props, _) = parse_declarations_important(rule.body);
        let timing_fn = props
            .get("animation-timing-function")
            .or_else(|| props.get("-webkit-animation-timing-function"))
            .and_then(|value| super::animation::parse_easing_checked(value));
        let prop_vec: Vec<(String, String)> = props
            .iter()
            .filter_map(|(k, v)| {
                if matches!(
                    k.as_str(),
                    "animation-timing-function" | "-webkit-animation-timing-function"
                ) {
                    return None;
                }
                if matches!(
                    k.strip_prefix("-webkit-").unwrap_or(k),
                    "animation"
                        | "animation-name"
                        | "animation-duration"
                        | "animation-delay"
                        | "animation-iteration-count"
                        | "animation-direction"
                        | "animation-fill-mode"
                        | "animation-play-state"
                ) {
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

        for offset in offsets {
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
    if let Some((word, consumed, false)) = super::syntax::name_token(sel) {
        if consumed != sel.len() {
            return None;
        }
        return if word.eq_ignore_ascii_case("from") {
            Some(0.0)
        } else if word.eq_ignore_ascii_case("to") {
            Some(1.0)
        } else {
            None
        };
    }
    let (pct, unit, consumed) = super::syntax::numeric_token(sel)?;
    (unit == "%" && consumed == sel.len() && (0.0..=100.0).contains(&pct)).then_some(pct / 100.0)
}

fn merge_keyframe_stop(
    stops: &mut Vec<KeyframeStop>,
    offset: f32,
    properties: &[(String, String)],
    timing_fn: Option<&EasingFn>,
) {
    if let Some(stop) = stops.iter_mut().find(|s| s.offset == offset) {
        if let Some(timing_fn) = timing_fn {
            stop.timing_fn = Some(timing_fn.clone());
        }
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
