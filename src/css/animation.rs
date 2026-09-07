//! The animation and transition shorthand parsers.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Animation / transition shorthand parsers ─────────────────────────────────

/// Parse an `animation-timing-function` value into an `EasingFn`.
pub fn parse_easing(s: &str) -> EasingFn {
    let s = s.trim();
    match s {
        "linear" => EasingFn::Linear,
        "ease" => EasingFn::Ease,
        "ease-in" => EasingFn::EaseIn,
        "ease-out" => EasingFn::EaseOut,
        "ease-in-out" => EasingFn::EaseInOut,
        "step-start" => EasingFn::StepStart,
        "step-end" => EasingFn::StepEnd,
        s if s.starts_with("cubic-bezier(") => {
            let inner = s
                .strip_prefix("cubic-bezier(")
                .unwrap_or("")
                .strip_suffix(')')
                .unwrap_or("");
            let parts: Vec<f32> = inner
                .split(',')
                .filter_map(|p| p.trim().parse().ok())
                .collect();
            if parts.len() == 4 {
                EasingFn::CubicBezier(parts[0], parts[1], parts[2], parts[3])
            } else {
                EasingFn::Ease
            }
        }
        s if s.starts_with("steps(") => {
            let inner = s
                .strip_prefix("steps(")
                .unwrap_or("")
                .strip_suffix(')')
                .unwrap_or("");
            let mut it = inner.splitn(2, ',');
            let count = it
                .next()
                .and_then(|p| p.trim().parse::<u32>().ok())
                .unwrap_or(1);
            // css-easing-2 §2.3 — four step positions. `start`/`end` are the
            // level-1 spellings of `jump-start`/`jump-end`.
            let pos = match it.next().map(|p| p.trim().to_ascii_lowercase()).as_deref() {
                Some("start") | Some("jump-start") => StepPosition::JumpStart,
                Some("jump-none") => StepPosition::JumpNone,
                Some("jump-both") => StepPosition::JumpBoth,
                _ => StepPosition::JumpEnd,
            };
            EasingFn::Steps(count, pos)
        }
        // `linear()` — css-easing-2 §2.1. Unrecognised, it fell through every
        // arm of the animation parser and was taken for the ANIMATION NAME.
        s if s.starts_with("linear(") => {
            let inner = s
                .strip_prefix("linear(")
                .unwrap_or("")
                .strip_suffix(')')
                .unwrap_or("");
            match parse_linear_points(inner) {
                Some(pts) => EasingFn::LinearPoints(pts),
                None => EasingFn::Linear,
            }
        }
        _ => EasingFn::Ease,
    }
}

/// Parse an `animation` shorthand value (comma-separated list of animations).
pub fn parse_animation_shorthand(s: &str) -> Vec<ParsedAnimation> {
    // ⛔ Split on TOP-LEVEL commas only. A bare `split(',')` cut inside the
    // easing function: `animation: spin 1s cubic-bezier(.4,0,.2,1) infinite`
    // became two animations, `infinite` landed on the junk one, and the real
    // animation ran exactly one iteration (css-animations-1 §4.9,
    // css-syntax-3 §5.4.9).
    crate::css::value_parse::split_top_level_commas(s)
        .into_iter()
        .filter_map(|part| parse_single_animation(part.trim()))
        .collect()
}

fn parse_single_animation(s: &str) -> Option<ParsedAnimation> {
    let mut name = String::new();
    let mut duration_ms = 0.0f32;
    let mut delay_ms = 0.0f32;
    let mut timing_fn = EasingFn::Ease;
    let mut iteration_count = 1.0f32;
    let mut direction = AnimDirection::Normal;
    let mut fill_mode = FillMode::None;
    let mut play_state_paused = false;
    let mut composition = AnimationComposition::Replace;
    let mut got_duration = false;

    for tok in tokenize_anim(s) {
        if tok.is_empty() {
            continue;
        }
        if let Some(ms) = parse_time_ms(&tok) {
            if !got_duration {
                duration_ms = ms;
                got_duration = true;
            } else {
                delay_ms = ms;
            }
            continue;
        }
        if is_timing_fn(&tok) {
            timing_fn = parse_easing(&tok);
            continue;
        }
        match tok.as_str() {
            "normal" => {
                direction = AnimDirection::Normal;
                continue;
            }
            "reverse" => {
                direction = AnimDirection::Reverse;
                continue;
            }
            "alternate" => {
                direction = AnimDirection::Alternate;
                continue;
            }
            "alternate-reverse" => {
                direction = AnimDirection::AlternateReverse;
                continue;
            }
            "none" => {
                fill_mode = FillMode::None;
                continue;
            }
            "forwards" => {
                fill_mode = FillMode::Forwards;
                continue;
            }
            "backwards" => {
                fill_mode = FillMode::Backwards;
                continue;
            }
            "both" => {
                fill_mode = FillMode::Both;
                continue;
            }
            "running" => {
                play_state_paused = false;
                continue;
            }
            "paused" => {
                play_state_paused = true;
                continue;
            }
            "infinite" => {
                iteration_count = f32::INFINITY;
                continue;
            }
            "replace" => {
                composition = AnimationComposition::Replace;
                continue;
            }
            "add" => {
                composition = AnimationComposition::Add;
                continue;
            }
            "accumulate" => {
                composition = AnimationComposition::Accumulate;
                continue;
            }
            _ => {}
        }
        if let Ok(n) = tok.parse::<f32>() {
            iteration_count = n;
            continue;
        }
        if name.is_empty() {
            name = tok.clone();
        }
    }

    if name.is_empty() || name == "none" {
        return None;
    }
    Some(ParsedAnimation {
        name,
        duration_ms,
        delay_ms,
        timing_fn,
        iteration_count,
        direction,
        fill_mode,
        play_state_paused,
        composition,
    })
}

/// Parse a `transition` shorthand value (comma-separated list of transitions).
pub fn parse_transition_shorthand(s: &str) -> Vec<ParsedTransition> {
    // Top-level commas only — see `parse_animation_shorthand`. This turned
    // `transition: transform .3s cubic-bezier(.4, 0, .2, 1)` into four
    // transitions with the easing degraded to `ease`.
    crate::css::value_parse::split_top_level_commas(s)
        .into_iter()
        .filter_map(|part| parse_single_transition(part.trim()))
        .collect()
}

fn parse_single_transition(s: &str) -> Option<ParsedTransition> {
    let mut property = String::new();
    let mut duration_ms = 0.0f32;
    let mut delay_ms = 0.0f32;
    let mut timing_fn = EasingFn::Ease;
    let mut got_duration = false;
    let mut allow_discrete = false;

    for tok in tokenize_anim(s) {
        if tok.is_empty() {
            continue;
        }
        if tok == "allow-discrete" {
            allow_discrete = true;
            continue;
        }
        if tok == "normal" {
            continue;
        }
        if let Some(ms) = parse_time_ms(&tok) {
            if !got_duration {
                duration_ms = ms;
                got_duration = true;
            } else {
                delay_ms = ms;
            }
            continue;
        }
        if is_timing_fn(&tok) {
            timing_fn = parse_easing(&tok);
            continue;
        }
        if property.is_empty() {
            property = tok.clone();
        }
    }

    if property.is_empty() {
        property = "all".to_string();
    }
    if property == "none" {
        return None;
    }
    Some(ParsedTransition {
        property,
        duration_ms,
        delay_ms,
        timing_fn,
        allow_discrete,
    })
}

/// Split an animation/transition shorthand token (handles `cubic-bezier(…)` as one token).
fn tokenize_anim(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ' ' | '\t' if depth == 0 => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

pub fn parse_time_ms(s: &str) -> Option<f32> {
    if let Some(ms) = s.strip_suffix("ms") {
        ms.trim().parse::<f32>().ok()
    } else if let Some(sec) = s.strip_suffix('s') {
        sec.trim().parse::<f32>().ok().map(|v| v * 1000.0)
    } else {
        None
    }
}

fn is_timing_fn(s: &str) -> bool {
    matches!(
        s,
        "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"
    ) || s.starts_with("cubic-bezier(")
        || s.starts_with("steps(")
        || s.starts_with("linear(")
}

/// The control points of a `linear()` easing — css-easing-2 §2.1.
///
/// Each entry is `<number> <percentage>{0,2}`: the output progress, then zero,
/// one or two explicit input positions (two is shorthand for repeating the
/// output at both). Points with no position are spread evenly between the
/// positions that have one, and the list is made non-decreasing in input.
fn parse_linear_points(inner: &str) -> Option<Vec<(f32, f32)>> {
    let mut pts: Vec<(f32, Option<f32>)> = Vec::new();
    for entry in inner.split(',') {
        let mut toks = entry.split_whitespace();
        let out: f32 = toks.next()?.parse().ok()?;
        let mut had_pos = false;
        for t in toks {
            let p = t.strip_suffix('%')?.parse::<f32>().ok()? / 100.0;
            pts.push((out, Some(p)));
            had_pos = true;
        }
        if !had_pos {
            pts.push((out, None));
        }
    }
    if pts.len() < 2 {
        return None;
    }
    // The first and last points anchor the curve when they carry no position.
    if pts[0].1.is_none() {
        pts[0].1 = Some(0.0);
    }
    let last = pts.len() - 1;
    if pts[last].1.is_none() {
        pts[last].1 = Some(1.0);
    }
    // Positions must not decrease.
    let mut running = 0.0f32;
    for p in pts.iter_mut() {
        if let Some(v) = p.1 {
            running = running.max(v);
            p.1 = Some(running);
        }
    }
    // Spread each unpositioned run evenly between its positioned neighbours.
    let mut i = 0;
    while i < pts.len() {
        if pts[i].1.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1; // always positioned: index 0 is
        let mut end = i; // anchored above
        while pts[end].1.is_none() {
            end += 1;
        }
        let (a, b) = (pts[start].1.unwrap(), pts[end].1.unwrap());
        let n = (end - start) as f32;
        for (k, j) in (start + 1..end).enumerate() {
            pts[j].1 = Some(a + (b - a) * (k as f32 + 1.0) / n);
        }
        i = end;
    }
    Some(
        pts.into_iter()
            .map(|(o, p)| (p.unwrap_or(0.0), o))
            .collect(),
    )
}

/// Extract CSS custom properties (--name: value) from `:root { }` blocks.
/// Extract CSS custom properties (--*) from rule blocks.
/// Collects from any rule block, since custom properties can be set on any element
/// and are inherited. This matches the common patterns: `:root`, `html`, `html.class`,
/// `body`, and element-level overrides.
pub(crate) fn extract_root_variables_cleaned(css: &str, vars: &mut HashMap<String, String>) {
    extract_root_variables_inner(css, vars);
}

fn extract_root_variables_inner(css: &str, vars: &mut HashMap<String, String>) {
    extract_root_variables_vp(css, vars, 0.0, 0.0);
}

pub(crate) fn extract_root_variables_vp(
    css: &str,
    vars: &mut HashMap<String, String>,
    vw: f32,
    vh: f32,
) {
    let mut s = css;
    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        if s.starts_with('@') {
            let prefix = &s[..s.len().min(30)];
            let lower: String = prefix.to_ascii_lowercase();
            if lower.starts_with("@media") {
                if let Some(brace) = s.find('{') {
                    // Only extract variables from matching media queries
                    // (when viewport is known, i.e. vw > 0).
                    let condition = s[6..brace].trim();
                    let matches = vw == 0.0 || evaluate_media(condition, vw, vh);
                    let (block, rest) = consume_block(&s[brace..]);
                    if matches {
                        extract_root_variables_vp(&block, vars, vw, vh);
                    }
                    s = rest;
                } else {
                    break;
                }
                continue;
            }
            // @layer, @supports: recurse into their blocks to find :root variables
            if lower.starts_with("@layer") || lower.starts_with("@supports") {
                if let Some(brace) = s.find('{') {
                    let (block, rest) = consume_block(&s[brace..]);
                    extract_root_variables_vp(&block, vars, vw, vh);
                    s = rest;
                } else {
                    break;
                }
                continue;
            }
            // Other @-rules (@keyframes, @font-face, etc.): skip
            if let Some(brace) = s.find('{') {
                let (_, rest) = consume_block(&s[brace..]);
                s = rest;
            } else {
                break;
            }
            continue;
        }
        // Skip stray closing braces (from minified CSS where blocks run together)
        if s.starts_with('}') {
            s = &s[1..];
            continue;
        }
        if let Some(brace) = s.find('{') {
            let selector = s[..brace].trim();
            let (block, rest) = consume_block(&s[brace..]);
            // Only extract custom properties from :root and html selectors
            // (universal scope). Selector-specific variables are handled
            // by the per-element cascade via inherited_vars.
            let sel_lower = selector.to_ascii_lowercase();
            let is_root = sel_lower.split(',').any(|s| {
                let s = s.trim();
                s == ":root"
                    || s == "html"
                    || s == "*"
                    || s.starts_with(":root ")
                    || s.starts_with(":root,")
                    || s.starts_with("html ")
                    || s.starts_with("html,")
                    || s.starts_with("html[")
            });
            if is_root && block.contains("--") {
                for decl in block.split(';') {
                    let decl = decl.trim();
                    if let Some(colon) = decl.find(':') {
                        let prop = decl[..colon].trim();
                        if prop.starts_with("--") {
                            let val = decl[colon + 1..].trim().to_string();
                            // Don't overwrite a non-empty value with an empty one
                            // (prevents dark-mode selectors from clobbering light-mode defaults)
                            if !val.is_empty() || !vars.contains_key(prop) {
                                vars.insert(prop.to_string(), val);
                            }
                        }
                    }
                }
            }
            s = rest;
        } else {
            break;
        }
    }
}

/// Expand `var()` references within the variable map itself so all values are concrete.
/// Handles chains (--a: var(--b), --b: 1rem) and circular refs (invalid/empty).
pub(crate) fn pre_resolve_variables(vars: &mut HashMap<String, String>) {
    // Handle csstools light-dark() polyfill: in light mode (our default),
    // the toggle variables should be empty so fallback (light) values are used.
    // The polyfill sets --csstools-color-scheme--light: initial in light mode,
    // which makes --csstools-light-dark-toggle--N invalid → fallback kicks in.
    // We simulate this by removing the toggle variables entirely.
    let toggle_keys: Vec<String> = vars
        .keys()
        .filter(|k| k.starts_with("--csstools-light-dark-toggle-"))
        .cloned()
        .collect();
    for key in &toggle_keys {
        vars.remove(key);
    }

    let keys: Vec<String> = vars.keys().cloned().collect();
    let max_passes = keys.len().min(50);
    for _ in 0..max_passes {
        let mut changed = false;
        let snapshot = vars.clone();
        for key in &keys {
            if let Some(val) = vars.get(key) {
                if val.contains("var(") {
                    let resolved = resolve_var_pass(val, &snapshot);
                    if resolved != *val {
                        vars.insert(key.clone(), resolved);
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    // Final pass: any still-unresolved custom property is cyclic/invalid.
    // The fallback inside the custom property's own `var()` is not taken; a
    // consuming declaration can still use its own fallback when it sees the
    // variable is invalid.
    let keys: Vec<String> = vars.keys().cloned().collect();
    for key in &keys {
        if let Some(val) = vars.get(key) {
            let mut resolved = val.clone();
            if resolved.contains("var(") {
                resolved.clear();
            }
            // Resolve light-dark() → use light value
            if resolved.contains("light-dark(") {
                if let Some(start) = resolved.find("light-dark(") {
                    let inner = &resolved[start + 11..];
                    if let Some(comma) = inner.find(',') {
                        if let Some(end) = inner.rfind(')') {
                            let light_val = inner[..comma].trim().to_string();
                            resolved =
                                format!("{}{}{}", &resolved[..start], light_val, &inner[end + 1..]);
                        }
                    }
                }
            }
            if resolved != *val {
                vars.insert(key.clone(), resolved);
            }
        }
    }
}

/// Extract @font-face declarations from a CSS string.
pub fn extract_font_faces(css: &str, faces: &mut Vec<FontFaceDecl>) {
    let cleaned = strip_css_comments(css);
    extract_font_faces_cleaned(&cleaned, faces);
}

/// Split CSS declarations on `;`, but skip `;` inside parentheses (for data URIs).
fn split_declarations_paren_aware(s: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ';' if depth == 0 => {
                result.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < s.len() {
        result.push(&s[start..]);
    }
    result
}

pub(crate) fn extract_font_faces_cleaned(css: &str, faces: &mut Vec<FontFaceDecl>) {
    let mut s = css;
    loop {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        // Search for @font-face case-insensitively without lowercasing entire string
        let pos = match find_case_insensitive(s, "@font-face") {
            Some(p) => p,
            None => break,
        };
        s = &s[pos + 10..];
        s = s.trim_start();
        if !s.starts_with('{') {
            continue;
        }
        let (block, rest) = consume_block(s);
        s = rest;

        // Parse declarations — split on `;` outside parentheses (to avoid
        // splitting inside `url(data:...;base64,...)`)
        let mut face = FontFaceDecl::default();
        for decl in split_declarations_paren_aware(block) {
            let decl = decl.trim();
            if let Some(colon) = decl.find(':') {
                let prop = decl[..colon].trim().to_ascii_lowercase();
                let value = decl[colon + 1..].trim().to_string();
                match prop.as_str() {
                    "font-family" => {
                        face.family = value.trim_matches('"').trim_matches('\'').to_string();
                    }
                    "src" => {
                        face.sources = crate::css::font_face::parse_font_face_sources(&value);
                        face.src = value;
                    }
                    "font-weight" => {
                        face.weight = Some(value);
                    }
                    "font-style" => {
                        face.style = Some(value);
                    }
                    "font-stretch" => {
                        face.stretch = Some(value);
                    }
                    "font-display" => {
                        face.display = Some(value);
                    }
                    "unicode-range" => {
                        face.unicode_range = Some(value);
                    }
                    "size-adjust" => {
                        face.size_adjust = Some(value);
                    }
                    "ascent-override" => {
                        face.ascent_override = Some(value);
                    }
                    "descent-override" => {
                        face.descent_override = Some(value);
                    }
                    "line-gap-override" => {
                        face.line_gap_override = Some(value);
                    }
                    "font-feature-settings" => {
                        face.feature_settings = Some(value);
                    }
                    "font-variation-settings" => {
                        face.variation_settings = Some(value);
                    }
                    "font-language-override" => {
                        face.language_override = Some(value);
                    }
                    _ => {}
                }
            }
        }
        if !face.family.is_empty() || !face.src.is_empty() {
            faces.push(face);
        }
    }
}
