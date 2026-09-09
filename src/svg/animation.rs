//! Typed SVG animation element metadata.
//!
//! This module only models parsed animation declarations. Timeline sampling and
//! applying animated values are a later integration step.

use super::{SvgAttribute, SvgDocument, SvgElementKind, SvgNode};
use crate::css::parse_color;
use crate::svg::path::{flatten_path_points, parse_path_data};
use crate::types::WebCore;
use crate::types::{apply_easing, EasingFn};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct SvgAnimationElement {
    pub kind: SvgAnimationKind,
    pub target_attribute: Option<String>,
    pub href: Option<String>,
    pub begin: Vec<SvgAnimationTime>,
    pub dur: Option<SvgAnimationTime>,
    pub end: Vec<SvgAnimationTime>,
    pub min: Option<SvgAnimationTime>,
    pub max: Option<SvgAnimationTime>,
    pub repeat_dur: Option<SvgAnimationTime>,
    pub repeat_count: SvgRepeatCount,
    pub restart: SvgAnimationRestart,
    pub fill_mode: SvgAnimationFillMode,
    pub calc_mode: SvgCalcMode,
    pub additive: SvgAdditiveMode,
    pub accumulate: SvgAccumulateMode,
    pub key_times: Vec<f32>,
    pub key_splines: Vec<[f32; 4]>,
    pub key_points: Vec<f32>,
    pub values: SvgAnimationValues,
    pub transform_type: Option<SvgAnimateTransformType>,
    pub rotate: Option<SvgAnimateMotionRotate>,
    pub path: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAnimationKind {
    Animate,
    AnimateColor,
    AnimateTransform,
    AnimateMotion,
    Set,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SvgAnimationTime {
    Seconds(f32),
    Indefinite,
    Syncbase {
        id: String,
        event: SvgSyncbaseEvent,
        offset: f32,
    },
    Eventbase {
        id: String,
        event: String,
        offset: f32,
    },
    AccessKey {
        key: String,
        offset: f32,
    },
    Event {
        event: String,
        offset: f32,
    },
    Media,
    Wallclock(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgSyncbaseEvent {
    Begin,
    End,
    Repeat(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SvgRepeatCount {
    Once,
    Count(f32),
    Indefinite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAnimationRestart {
    Always,
    WhenNotActive,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAnimationFillMode {
    Remove,
    Freeze,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgCalcMode {
    Linear,
    Discrete,
    Paced,
    Spline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAdditiveMode {
    Replace,
    Sum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAccumulateMode {
    None,
    Sum,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgAnimationValues {
    pub from: Option<String>,
    pub to: Option<String>,
    pub by: Option<String>,
    pub values: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgAnimateTransformType {
    Translate,
    Scale,
    Rotate,
    SkewX,
    SkewY,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SvgAnimateMotionRotate {
    Auto,
    AutoReverse,
    Angle(f32),
}

pub(crate) type SvgAnimationOverride = (Vec<usize>, String, String);
pub(crate) type SvgAnimationControl = (Vec<usize>, String, f32);

pub(crate) fn parse_animation_element(node: &SvgNode) -> Option<SvgAnimationElement> {
    let kind = match node.kind {
        SvgElementKind::Animate => SvgAnimationKind::Animate,
        SvgElementKind::AnimateColor => SvgAnimationKind::AnimateColor,
        SvgElementKind::AnimateTransform => SvgAnimationKind::AnimateTransform,
        SvgElementKind::AnimateMotion => SvgAnimationKind::AnimateMotion,
        SvgElementKind::Set => SvgAnimationKind::Set,
        _ => return None,
    };

    Some(SvgAnimationElement {
        kind,
        target_attribute: node
            .attr_ascii_case_insensitive("attributeName")
            .map(str::to_string),
        href: node
            .attr_ascii_case_insensitive("href")
            .or_else(|| attr_ns(node, "xlink", "href"))
            .map(str::to_string),
        begin: parse_time_list(node.attr_ascii_case_insensitive("begin"))
            .unwrap_or_else(|| vec![SvgAnimationTime::Seconds(0.0)]),
        dur: node
            .attr_ascii_case_insensitive("dur")
            .and_then(parse_time_value),
        end: parse_time_list(node.attr_ascii_case_insensitive("end")).unwrap_or_default(),
        min: node
            .attr_ascii_case_insensitive("min")
            .and_then(parse_time_value),
        max: node
            .attr_ascii_case_insensitive("max")
            .and_then(parse_time_value),
        repeat_dur: node
            .attr_ascii_case_insensitive("repeatDur")
            .and_then(parse_time_value),
        repeat_count: parse_repeat_count(node.attr_ascii_case_insensitive("repeatCount")),
        restart: parse_restart(node.attr_ascii_case_insensitive("restart")),
        fill_mode: parse_fill_mode(node.attr_ascii_case_insensitive("fill")),
        calc_mode: parse_calc_mode_for_kind(node.attr_ascii_case_insensitive("calcMode"), kind),
        additive: parse_additive(node.attr_ascii_case_insensitive("additive")),
        accumulate: parse_accumulate(node.attr_ascii_case_insensitive("accumulate")),
        key_times: parse_number_list(node.attr_ascii_case_insensitive("keyTimes")),
        key_splines: parse_key_splines(node.attr_ascii_case_insensitive("keySplines")),
        key_points: parse_number_list(node.attr_ascii_case_insensitive("keyPoints")),
        values: SvgAnimationValues {
            from: node.attr_ascii_case_insensitive("from").map(str::to_string),
            to: node.attr_ascii_case_insensitive("to").map(str::to_string),
            by: node.attr_ascii_case_insensitive("by").map(str::to_string),
            values: parse_semicolon_list(node.attr_ascii_case_insensitive("values")),
        },
        transform_type: node
            .attr_ascii_case_insensitive("type")
            .and_then(parse_animate_transform_type),
        rotate: node
            .attr_ascii_case_insensitive("rotate")
            .and_then(parse_animate_motion_rotate),
        path: node.attr_ascii_case_insensitive("path").map(str::to_string),
    })
}

fn attr_ns<'a>(node: &'a SvgNode, namespace: &str, name: &str) -> Option<&'a str> {
    node.attributes
        .iter()
        .find(|attr| attr.namespace.as_deref() == Some(namespace) && attr.name == name)
        .map(|attr| attr.value.as_str())
}

fn parse_time_list(raw: Option<&str>) -> Option<Vec<SvgAnimationTime>> {
    let raw = raw?;
    let times: Vec<SvgAnimationTime> = raw
        .split(';')
        .filter_map(|part| parse_time_value(part.trim()))
        .collect();
    (!times.is_empty()).then_some(times)
}

fn parse_time_value(raw: &str) -> Option<SvgAnimationTime> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(syncbase) = parse_syncbase_time(value) {
        return Some(syncbase);
    }
    if let Some(access_key) = parse_access_key_time(value) {
        return Some(access_key);
    }
    if let Some(eventbase) = parse_eventbase_time(value) {
        return Some(eventbase);
    }
    if value.eq_ignore_ascii_case("media") {
        return Some(SvgAnimationTime::Media);
    }
    if let Some(wallclock) = parse_wallclock_time(value) {
        return Some(wallclock);
    }
    if value.eq_ignore_ascii_case("indefinite") {
        return Some(SvgAnimationTime::Indefinite);
    }
    if let Some(seconds) = parse_clock_seconds(value) {
        return Some(SvgAnimationTime::Seconds(seconds));
    }
    if value.ends_with("ms") {
        if let Ok(n) = value[..value.len() - 2].trim().parse::<f32>() {
            return Some(SvgAnimationTime::Seconds(n / 1000.0));
        }
    }
    if value.ends_with('s') {
        if let Ok(n) = value[..value.len() - 1].trim().parse::<f32>() {
            return Some(SvgAnimationTime::Seconds(n));
        }
    }
    if value.chars().any(|ch| ch.is_ascii_alphabetic()) {
        let (event, offset) = split_eventbase_event_and_offset(value)?;
        return Some(SvgAnimationTime::Event {
            event: event.to_string(),
            offset,
        });
    }
    value.parse::<f32>().ok().map(SvgAnimationTime::Seconds)
}

fn parse_wallclock_time(value: &str) -> Option<SvgAnimationTime> {
    let value = value.trim();
    let open = value.find('(')?;
    let close = value.rfind(')')?;
    if close <= open || !value[..open].eq_ignore_ascii_case("wallclock") {
        return None;
    }
    let clock = value[open + 1..close].trim();
    if clock.is_empty() || !value[close + 1..].trim().is_empty() {
        return None;
    }
    Some(SvgAnimationTime::Wallclock(clock.to_string()))
}

fn parse_clock_seconds(value: &str) -> Option<f32> {
    let value = value.trim();
    if value.contains(':') {
        let parts = value.split(':').collect::<Vec<_>>();
        return match parts.as_slice() {
            [minutes, seconds] => {
                let minutes = minutes.trim().parse::<f32>().ok()?;
                let seconds = seconds.trim().parse::<f32>().ok()?;
                Some(minutes * 60.0 + seconds)
            }
            [hours, minutes, seconds] => {
                let hours = hours.trim().parse::<f32>().ok()?;
                let minutes = minutes.trim().parse::<f32>().ok()?;
                let seconds = seconds.trim().parse::<f32>().ok()?;
                Some(hours * 3600.0 + minutes * 60.0 + seconds)
            }
            _ => None,
        };
    }
    for (suffix, scale) in [("min", 60.0), ("h", 3600.0)] {
        if let Some(raw) = value.strip_suffix(suffix) {
            let n = raw.trim().parse::<f32>().ok()?;
            return Some(n * scale);
        }
    }
    None
}

fn parse_access_key_time(value: &str) -> Option<SvgAnimationTime> {
    let value = value.trim();
    let open = value.find('(')?;
    let close = value.rfind(')')?;
    if close <= open || !value[..open].eq_ignore_ascii_case("accessKey") {
        return None;
    }
    let offset = parse_syncbase_offset(value[close + 1..].trim())?;
    let key = value[open + 1..close].trim();
    if key.is_empty() {
        return None;
    }
    Some(SvgAnimationTime::AccessKey {
        key: key.to_string(),
        offset,
    })
}

fn parse_syncbase_time(value: &str) -> Option<SvgAnimationTime> {
    if let Some(repeat) = parse_syncbase_repeat_time(value) {
        return Some(repeat);
    }
    for (marker, event) in [
        (".begin", SvgSyncbaseEvent::Begin),
        (".end", SvgSyncbaseEvent::End),
    ] {
        let Some(marker_pos) = value.find(marker) else {
            continue;
        };
        let id = value[..marker_pos].trim();
        if id.is_empty() {
            return None;
        }
        let rest = value[marker_pos + marker.len()..].trim();
        let offset = if rest.is_empty() {
            0.0
        } else if let Some(raw_offset) = rest.strip_prefix('+') {
            match parse_time_value(raw_offset.trim())? {
                SvgAnimationTime::Seconds(s) => s,
                _ => return None,
            }
        } else if let Some(raw_offset) = rest.strip_prefix('-') {
            match parse_time_value(raw_offset.trim())? {
                SvgAnimationTime::Seconds(s) => -s,
                _ => return None,
            }
        } else {
            return None;
        };
        return Some(SvgAnimationTime::Syncbase {
            id: id.to_string(),
            event,
            offset,
        });
    }
    None
}

fn parse_syncbase_repeat_time(value: &str) -> Option<SvgAnimationTime> {
    let marker = ".repeat(";
    let marker_pos = value.find(marker)?;
    let id = value[..marker_pos].trim();
    if id.is_empty() {
        return None;
    }
    let repeat_start = marker_pos + marker.len();
    let repeat_end = value[repeat_start..].find(')')? + repeat_start;
    let repeat = value[repeat_start..repeat_end].trim().parse::<u32>().ok()?;
    let rest = value[repeat_end + 1..].trim();
    let offset = parse_syncbase_offset(rest)?;
    Some(SvgAnimationTime::Syncbase {
        id: id.to_string(),
        event: SvgSyncbaseEvent::Repeat(repeat),
        offset,
    })
}

fn parse_syncbase_offset(rest: &str) -> Option<f32> {
    if rest.is_empty() {
        return Some(0.0);
    }
    if let Some(raw_offset) = rest.strip_prefix('+') {
        match parse_time_value(raw_offset.trim())? {
            SvgAnimationTime::Seconds(s) => Some(s),
            _ => None,
        }
    } else if let Some(raw_offset) = rest.strip_prefix('-') {
        match parse_time_value(raw_offset.trim())? {
            SvgAnimationTime::Seconds(s) => Some(-s),
            _ => None,
        }
    } else {
        None
    }
}

fn parse_eventbase_time(value: &str) -> Option<SvgAnimationTime> {
    let dot = value.find('.')?;
    let id = value[..dot].trim();
    if id.is_empty() {
        return None;
    }
    let mut id_chars = id.chars();
    let first = id_chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }
    if !id_chars.all(|ch| ch == '_' || ch == '-' || ch == ':' || ch.is_ascii_alphanumeric()) {
        return None;
    }
    let rest = value[dot + 1..].trim();
    if rest.is_empty() {
        return None;
    }
    let (event, offset) = split_eventbase_event_and_offset(rest)?;
    if event.eq_ignore_ascii_case("begin") || event.eq_ignore_ascii_case("end") {
        return None;
    }
    Some(SvgAnimationTime::Eventbase {
        id: id.to_string(),
        event: event.to_string(),
        offset,
    })
}

fn split_eventbase_event_and_offset(rest: &str) -> Option<(&str, f32)> {
    let offset_pos = rest
        .char_indices()
        .skip(1)
        .find(|(_, ch)| *ch == '+' || *ch == '-')
        .map(|(idx, _)| idx);
    let Some(offset_pos) = offset_pos else {
        return Some((rest.trim(), 0.0));
    };
    let event = rest[..offset_pos].trim();
    if event.is_empty() {
        return None;
    }
    let sign = rest.as_bytes()[offset_pos] as char;
    let raw_offset = rest[offset_pos + 1..].trim();
    let SvgAnimationTime::Seconds(mut offset) = parse_time_value(raw_offset)? else {
        return None;
    };
    if sign == '-' {
        offset = -offset;
    }
    Some((event, offset))
}

fn parse_repeat_count(raw: Option<&str>) -> SvgRepeatCount {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return SvgRepeatCount::Once;
    };
    if raw.eq_ignore_ascii_case("indefinite") {
        SvgRepeatCount::Indefinite
    } else {
        raw.parse::<f32>()
            .ok()
            .filter(|count| *count >= 0.0)
            .map(SvgRepeatCount::Count)
            .unwrap_or(SvgRepeatCount::Once)
    }
}

fn parse_restart(raw: Option<&str>) -> SvgAnimationRestart {
    match raw.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("never") => SvgAnimationRestart::Never,
        Some(value) if value.eq_ignore_ascii_case("whenNotActive") => {
            SvgAnimationRestart::WhenNotActive
        }
        _ => SvgAnimationRestart::Always,
    }
}

fn parse_fill_mode(raw: Option<&str>) -> SvgAnimationFillMode {
    match raw.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("freeze") => SvgAnimationFillMode::Freeze,
        _ => SvgAnimationFillMode::Remove,
    }
}

fn parse_calc_mode(raw: Option<&str>) -> SvgCalcMode {
    match raw.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("discrete") => SvgCalcMode::Discrete,
        Some(value) if value.eq_ignore_ascii_case("paced") => SvgCalcMode::Paced,
        Some(value) if value.eq_ignore_ascii_case("spline") => SvgCalcMode::Spline,
        _ => SvgCalcMode::Linear,
    }
}

fn parse_calc_mode_for_kind(raw: Option<&str>, kind: SvgAnimationKind) -> SvgCalcMode {
    match raw {
        Some(_) => parse_calc_mode(raw),
        None if kind == SvgAnimationKind::AnimateMotion => SvgCalcMode::Paced,
        None => SvgCalcMode::Linear,
    }
}

fn parse_additive(raw: Option<&str>) -> SvgAdditiveMode {
    match raw.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("sum") => SvgAdditiveMode::Sum,
        _ => SvgAdditiveMode::Replace,
    }
}

fn parse_accumulate(raw: Option<&str>) -> SvgAccumulateMode {
    match raw.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("sum") => SvgAccumulateMode::Sum,
        _ => SvgAccumulateMode::None,
    }
}

fn parse_number_list(raw: Option<&str>) -> Vec<f32> {
    raw.unwrap_or("")
        .split(';')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect()
}

fn parse_key_splines(raw: Option<&str>) -> Vec<[f32; 4]> {
    raw.unwrap_or("")
        .split(';')
        .filter_map(|part| {
            let nums = parse_number_tokens(part)?;
            (nums.len() == 4).then(|| [nums[0], nums[1], nums[2], nums[3]])
        })
        .collect()
}

fn parse_semicolon_list(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_animate_transform_type(raw: &str) -> Option<SvgAnimateTransformType> {
    match raw.trim() {
        value if value.eq_ignore_ascii_case("translate") => {
            Some(SvgAnimateTransformType::Translate)
        }
        value if value.eq_ignore_ascii_case("scale") => Some(SvgAnimateTransformType::Scale),
        value if value.eq_ignore_ascii_case("rotate") => Some(SvgAnimateTransformType::Rotate),
        value if value.eq_ignore_ascii_case("skewX") => Some(SvgAnimateTransformType::SkewX),
        value if value.eq_ignore_ascii_case("skewY") => Some(SvgAnimateTransformType::SkewY),
        _ => None,
    }
}

fn parse_animate_motion_rotate(raw: &str) -> Option<SvgAnimateMotionRotate> {
    let value = raw.trim();
    if value.eq_ignore_ascii_case("auto") {
        Some(SvgAnimateMotionRotate::Auto)
    } else if value.eq_ignore_ascii_case("auto-reverse") {
        Some(SvgAnimateMotionRotate::AutoReverse)
    } else {
        value.parse::<f32>().ok().map(SvgAnimateMotionRotate::Angle)
    }
}

pub(crate) fn tick_svg_animations(root: &mut WebCore, now: std::time::Instant) -> bool {
    let mut still_running = false;
    tick_svg_animations_in_node(root, now, &mut still_running);
    still_running
}

fn tick_svg_animations_in_node(
    node: &mut WebCore,
    now: std::time::Instant,
    still_running: &mut bool,
) {
    if let Some(doc) = node.svg_document.clone() {
        if document_has_svg_animations(&doc) {
            let start = *node.svg_animation_start_time.get_or_insert(now);
            let elapsed = now.duration_since(start).as_secs_f32();
            let sampled = sample_svg_animation_overrides_with_controls(
                &doc,
                elapsed,
                &node.svg_animation_controls,
                still_running,
            );
            node.svg_animation_overrides = sampled;
        } else {
            node.svg_animation_overrides.clear();
            node.svg_animation_start_time = None;
        }
    }
    for child in &mut node.children {
        tick_svg_animations_in_node(child, now, still_running);
    }
}

pub(crate) fn document_has_svg_animations(doc: &SvgDocument) -> bool {
    node_has_svg_animations(&doc.root)
}

fn node_has_svg_animations(node: &SvgNode) -> bool {
    node.animation.is_some() || node.children.iter().any(node_has_svg_animations)
}

pub(crate) fn sample_svg_animation_overrides(
    doc: &SvgDocument,
    elapsed_s: f32,
    still_running: &mut bool,
) -> Vec<SvgAnimationOverride> {
    sample_svg_animation_overrides_with_controls(doc, elapsed_s, &[], still_running)
}

pub(crate) fn sample_svg_animation_overrides_with_controls(
    doc: &SvgDocument,
    elapsed_s: f32,
    controls: &[SvgAnimationControl],
    still_running: &mut bool,
) -> Vec<SvgAnimationOverride> {
    let mut id_paths = Vec::new();
    collect_id_refs(&doc.root, &mut Vec::new(), &mut id_paths);
    let animation_defs = collect_animation_defs(&doc.root);
    let animation_paths = collect_animation_def_paths(&doc.root);

    let mut out = Vec::new();
    collect_animation_samples(
        &doc.root,
        &doc.root,
        &mut Vec::new(),
        None,
        elapsed_s,
        &id_paths,
        &animation_defs,
        &animation_paths,
        controls,
        still_running,
        &mut out,
        &mut HashMap::new(),
    );
    out
}

pub(crate) fn animation_controls_for_event(
    doc: &SvgDocument,
    event_target_path: &[usize],
    event_name: &str,
    elapsed_s: f32,
) -> Vec<SvgAnimationControl> {
    let mut id_paths = Vec::new();
    collect_id_refs(&doc.root, &mut Vec::new(), &mut id_paths);
    let mut out = Vec::new();
    collect_animation_event_controls(
        &doc.root,
        &mut Vec::new(),
        None,
        event_target_path,
        event_name,
        elapsed_s,
        &id_paths,
        &mut out,
    );
    out
}

pub(crate) fn animation_controls_for_access_key(
    doc: &SvgDocument,
    key: &str,
    elapsed_s: f32,
) -> Vec<SvgAnimationControl> {
    let mut out = Vec::new();
    collect_animation_access_key_controls(&doc.root, &mut Vec::new(), key, elapsed_s, &mut out);
    out
}

pub(crate) fn svg_document_with_animation_overrides(
    doc: &SvgDocument,
    overrides: &[SvgAnimationOverride],
) -> SvgDocument {
    let mut doc = doc.clone();
    for (path, name, value) in overrides {
        if let Some(node) = node_at_path_mut(&mut doc.root, path) {
            set_attr(node, name, value);
        }
    }
    doc
}

fn collect_id_refs(
    node: &SvgNode,
    path: &mut Vec<usize>,
    out: &mut Vec<(String, Vec<usize>, Option<String>)>,
) {
    if let Some(id) = node.attr("id") {
        out.push((
            id.to_string(),
            path.clone(),
            node.attr("d").map(str::to_string),
        ));
    }
    for (idx, child) in node.children.iter().enumerate() {
        path.push(idx);
        collect_id_refs(child, path, out);
        path.pop();
    }
}

fn collect_animation_defs(root: &SvgNode) -> HashMap<String, SvgAnimationElement> {
    let mut out = HashMap::new();
    collect_animation_defs_in_node(root, &mut out);
    out
}

fn collect_animation_defs_in_node(node: &SvgNode, out: &mut HashMap<String, SvgAnimationElement>) {
    if let (Some(id), Some(anim)) = (node.attr("id"), &node.animation) {
        out.insert(id.to_string(), anim.clone());
    }
    for child in &node.children {
        collect_animation_defs_in_node(child, out);
    }
}

fn collect_animation_def_paths(root: &SvgNode) -> HashMap<String, Vec<usize>> {
    let mut out = HashMap::new();
    collect_animation_def_paths_in_node(root, &mut Vec::new(), &mut out);
    out
}

fn collect_animation_def_paths_in_node(
    node: &SvgNode,
    path: &mut Vec<usize>,
    out: &mut HashMap<String, Vec<usize>>,
) {
    if let (Some(id), Some(_)) = (node.attr("id"), &node.animation) {
        out.insert(id.to_string(), path.clone());
    }
    for (idx, child) in node.children.iter().enumerate() {
        path.push(idx);
        collect_animation_def_paths_in_node(child, path, out);
        path.pop();
    }
}

fn collect_animation_samples(
    root: &SvgNode,
    node: &SvgNode,
    path: &mut Vec<usize>,
    parent_path: Option<Vec<usize>>,
    elapsed_s: f32,
    id_paths: &[(String, Vec<usize>, Option<String>)],
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    still_running: &mut bool,
    out: &mut Vec<SvgAnimationOverride>,
    active_values: &mut HashMap<(Vec<usize>, String), String>,
) {
    if let Some(anim) = &node.animation {
        let target_path = anim
            .href
            .as_deref()
            .and_then(|href| href.strip_prefix('#'))
            .and_then(|id| id_paths.iter().find(|(found, _, _)| found == id))
            .map(|(_, path, _)| path.clone())
            .or_else(|| parent_path.clone());
        let motion_path = resolve_animation_motion_path(anim, node, id_paths);
        let base_value = target_path.as_ref().and_then(|path| {
            let attr = animation_target_attr_name(anim)?;
            active_values
                .get(&(path.clone(), attr.clone()))
                .cloned()
                .or_else(|| {
                    node_at_path_ref(root, path)
                        .and_then(|target| animation_base_attr(target, anim))
                })
        });
        if let (Some(target_path), Some((attr, sample))) = (
            target_path,
            sample_animation_override(
                anim,
                elapsed_s,
                motion_path.as_deref(),
                base_value.as_deref(),
                animation_defs,
                animation_paths,
                controls,
                path,
            ),
        ) {
            active_values.insert((target_path.clone(), attr.clone()), sample.clone());
            out.push((target_path, attr, sample));
        }
        if animation_still_running(
            anim,
            path,
            elapsed_s,
            animation_defs,
            animation_paths,
            controls,
        ) {
            *still_running = true;
        }
    }

    for (idx, child) in node.children.iter().enumerate() {
        let parent_for_child = path.clone();
        path.push(idx);
        collect_animation_samples(
            root,
            child,
            path,
            Some(parent_for_child),
            elapsed_s,
            id_paths,
            animation_defs,
            animation_paths,
            controls,
            still_running,
            out,
            active_values,
        );
        path.pop();
    }
}

fn collect_animation_event_controls(
    node: &SvgNode,
    path: &mut Vec<usize>,
    parent_path: Option<Vec<usize>>,
    event_target_path: &[usize],
    event_name: &str,
    elapsed_s: f32,
    id_paths: &[(String, Vec<usize>, Option<String>)],
    out: &mut Vec<SvgAnimationControl>,
) {
    if let Some(anim) = &node.animation {
        let target_path = anim
            .href
            .as_deref()
            .and_then(|href| href.strip_prefix('#'))
            .and_then(|id| id_paths.iter().find(|(found, _, _)| found == id))
            .map(|(_, path, _)| path.clone())
            .or_else(|| parent_path.clone());
        for time in &anim.begin {
            if let Some(at) = animation_event_instance_time(
                time,
                target_path.as_deref(),
                event_target_path,
                event_name,
                elapsed_s,
                id_paths,
            ) {
                out.push((path.clone(), "begin".to_string(), at));
            }
        }
        for time in &anim.end {
            if let Some(at) = animation_event_instance_time(
                time,
                target_path.as_deref(),
                event_target_path,
                event_name,
                elapsed_s,
                id_paths,
            ) {
                out.push((path.clone(), "end".to_string(), at));
            }
        }
    }

    for (idx, child) in node.children.iter().enumerate() {
        let parent_for_child = path.clone();
        path.push(idx);
        collect_animation_event_controls(
            child,
            path,
            Some(parent_for_child),
            event_target_path,
            event_name,
            elapsed_s,
            id_paths,
            out,
        );
        path.pop();
    }
}

fn collect_animation_access_key_controls(
    node: &SvgNode,
    path: &mut Vec<usize>,
    key: &str,
    elapsed_s: f32,
    out: &mut Vec<SvgAnimationControl>,
) {
    if let Some(anim) = &node.animation {
        for time in &anim.begin {
            if animation_access_key_matches(time, key) {
                out.push((
                    path.clone(),
                    "begin".to_string(),
                    elapsed_s + animation_access_key_offset(time),
                ));
            }
        }
        for time in &anim.end {
            if animation_access_key_matches(time, key) {
                out.push((
                    path.clone(),
                    "end".to_string(),
                    elapsed_s + animation_access_key_offset(time),
                ));
            }
        }
    }

    for (idx, child) in node.children.iter().enumerate() {
        path.push(idx);
        collect_animation_access_key_controls(child, path, key, elapsed_s, out);
        path.pop();
    }
}

fn animation_access_key_matches(time: &SvgAnimationTime, key: &str) -> bool {
    matches!(time, SvgAnimationTime::AccessKey { key: expected, .. } if expected.eq_ignore_ascii_case(key))
}

fn animation_access_key_offset(time: &SvgAnimationTime) -> f32 {
    match time {
        SvgAnimationTime::AccessKey { offset, .. } => *offset,
        _ => 0.0,
    }
}

fn animation_event_instance_time(
    time: &SvgAnimationTime,
    animation_target_path: Option<&[usize]>,
    event_target_path: &[usize],
    event_name: &str,
    elapsed_s: f32,
    id_paths: &[(String, Vec<usize>, Option<String>)],
) -> Option<f32> {
    match time {
        SvgAnimationTime::Event { event, offset }
            if event.eq_ignore_ascii_case(event_name)
                && animation_target_path == Some(event_target_path) =>
        {
            Some(elapsed_s + offset)
        }
        SvgAnimationTime::Eventbase { id, event, offset }
            if event.eq_ignore_ascii_case(event_name)
                && id_paths.iter().any(|(found, path, _)| {
                    found == id && path.as_slice() == event_target_path
                }) =>
        {
            Some(elapsed_s + offset)
        }
        SvgAnimationTime::AccessKey { .. } => None,
        _ => None,
    }
}

fn sample_animation_override(
    anim: &SvgAnimationElement,
    elapsed_s: f32,
    motion_path: Option<&str>,
    base_value: Option<&str>,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    animation_path: &[usize],
) -> Option<(String, String)> {
    if anim.kind == SvgAnimationKind::AnimateMotion {
        return sample_motion_transform(
            anim,
            elapsed_s,
            motion_path,
            base_value,
            animation_defs,
            animation_paths,
            controls,
            animation_path,
        )
        .map(|value| ("transform".to_string(), value));
    }
    let attr = anim.target_attribute.clone()?;
    let value = sample_animation(
        anim,
        elapsed_s,
        base_value,
        animation_defs,
        animation_paths,
        controls,
        animation_path,
    )?;
    let value = if anim.kind == SvgAnimationKind::AnimateTransform {
        format_transform_animation_value(anim, &value)
    } else {
        value
    };
    let value = compose_additive_value(anim, &attr, base_value, &value);
    Some((attr, value))
}

fn sample_animation(
    anim: &SvgAnimationElement,
    elapsed_s: f32,
    base_value: Option<&str>,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    animation_path: &[usize],
) -> Option<String> {
    let begin_s = begin_seconds_for_path(
        anim,
        animation_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
    )?;
    if elapsed_s < begin_s {
        return None;
    }
    let local_s = elapsed_s - begin_s;

    if anim.kind == SvgAnimationKind::Set {
        let value = anim
            .values
            .to
            .clone()
            .or_else(|| anim.values.values.first().cloned())?;
        let active_s = set_active_duration_seconds_for_path(
            anim,
            animation_path,
            begin_s,
            animation_defs,
            animation_paths,
            controls,
        );
        if local_s > active_s {
            return (anim.fill_mode == SvgAnimationFillMode::Freeze).then_some(value);
        }
        return Some(value);
    }

    let dur_s = duration_seconds(anim)?;
    if dur_s <= 0.0 {
        return anim.values.to.clone();
    }

    let active_s = active_duration_seconds_for_path(
        anim,
        animation_path,
        begin_s,
        dur_s,
        animation_defs,
        animation_paths,
        controls,
    );
    if local_s > active_s {
        return (anim.fill_mode == SvgAnimationFillMode::Freeze)
            .then(|| {
                let final_value = sample_animation_at_progress(anim, 1.0, base_value)?;
                Some(apply_accumulate_value(
                    anim,
                    &final_value,
                    final_iteration(local_s, dur_s, active_s),
                ))
            })
            .flatten();
    }

    let progress = iteration_progress(local_s, dur_s, active_s);
    let iteration = current_iteration(local_s, dur_s, active_s);
    let value = sample_animation_at_progress(anim, progress, base_value)?;
    Some(apply_accumulate_value(anim, &value, iteration))
}

fn animation_still_running(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> bool {
    let Some(begin_s) = begin_seconds_for_path(
        anim,
        animation_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
    ) else {
        if controls.iter().any(|(path, kind, at)| {
            path.as_slice() == animation_path && kind == "begin" && *at > elapsed_s
        }) {
            return true;
        }
        if has_pending_runtime_syncbase_begin(
            anim,
            elapsed_s,
            animation_defs,
            animation_paths,
            controls,
        ) {
            return true;
        }
        return false;
    };
    if elapsed_s < begin_s {
        return true;
    }
    if anim.kind == SvgAnimationKind::Set {
        let active_s = set_active_duration_seconds_for_path(
            anim,
            animation_path,
            begin_s,
            animation_defs,
            animation_paths,
            controls,
        );
        return active_s.is_finite() && elapsed_s - begin_s <= active_s;
    }
    let Some(dur_s) = duration_seconds(anim) else {
        return false;
    };
    elapsed_s - begin_s
        <= active_duration_seconds_for_path(
            anim,
            animation_path,
            begin_s,
            dur_s,
            animation_defs,
            animation_paths,
            controls,
        )
}

fn begin_seconds_for_path(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> Option<f32> {
    let runtime_begin = runtime_begin_seconds(
        anim,
        animation_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
    );
    runtime_begin.or_else(|| begin_seconds(anim, animation_defs))
}

fn runtime_begin_seconds(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> Option<f32> {
    runtime_begin_seconds_inner(
        anim,
        animation_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
        &mut Vec::new(),
    )
}

fn runtime_begin_seconds_inner(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    stack: &mut Vec<Vec<usize>>,
) -> Option<f32> {
    if stack.iter().any(|seen| seen.as_slice() == animation_path) {
        return None;
    }
    stack.push(animation_path.to_vec());

    let mut begins: Vec<f32> = controls
        .iter()
        .filter(|(path, kind, at)| {
            path.as_slice() == animation_path && kind == "begin" && *at <= elapsed_s
        })
        .map(|(_, _, at)| *at)
        .collect();
    begins.extend(anim.begin.iter().filter_map(|time| {
        runtime_syncbase_instance_seconds(
            time,
            elapsed_s,
            animation_defs,
            animation_paths,
            controls,
            stack,
            true,
        )
    }));
    begins.sort_by(|a, b| a.total_cmp(b));

    let accepted = match anim.restart {
        SvgAnimationRestart::Always => begins.into_iter().max_by(|a, b| a.total_cmp(b)),
        SvgAnimationRestart::Never => begins.into_iter().next(),
        SvgAnimationRestart::WhenNotActive => {
            let dur_s = duration_seconds(anim);
            let mut accepted = None;
            for begin_s in begins {
                let active_at_begin = match (accepted, dur_s) {
                    (Some(prev), Some(dur_s)) => {
                        begin_s
                            <= prev
                                + active_duration_seconds_for_path(
                                    anim,
                                    animation_path,
                                    prev,
                                    dur_s,
                                    animation_defs,
                                    animation_paths,
                                    controls,
                                )
                    }
                    (Some(_), None) => true,
                    _ => false,
                };
                if !active_at_begin {
                    accepted = Some(begin_s);
                }
            }
            accepted
        }
    };

    stack.pop();
    accepted
}

fn runtime_syncbase_instance_seconds(
    time: &SvgAnimationTime,
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    stack: &mut Vec<Vec<usize>>,
    require_due: bool,
) -> Option<f32> {
    let SvgAnimationTime::Syncbase { id, event, offset } = time else {
        return None;
    };
    let referenced_path = animation_paths.get(id)?;
    let referenced = animation_defs.get(id)?;
    let begin = runtime_begin_seconds_inner(
        referenced,
        referenced_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
        stack,
    )
    .or_else(|| begin_seconds(referenced, animation_defs))?;
    let base = match event {
        SvgSyncbaseEvent::Begin => begin,
        SvgSyncbaseEvent::End => {
            let dur_s = duration_seconds(referenced)?;
            begin
                + active_duration_seconds_for_path(
                    referenced,
                    referenced_path,
                    begin,
                    dur_s,
                    animation_defs,
                    animation_paths,
                    controls,
                )
        }
        SvgSyncbaseEvent::Repeat(repeat) => {
            let dur_s = duration_seconds(referenced)?;
            let active_s = active_duration_seconds_for_path(
                referenced,
                referenced_path,
                begin,
                dur_s,
                animation_defs,
                animation_paths,
                controls,
            );
            let repeat_at = begin + dur_s * *repeat as f32;
            if repeat_at - begin > active_s {
                return None;
            }
            repeat_at
        }
    };
    let instance = base + *offset;
    (!require_due || instance <= elapsed_s).then_some(instance)
}

fn has_pending_runtime_syncbase_begin(
    anim: &SvgAnimationElement,
    elapsed_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> bool {
    anim.begin.iter().any(|time| {
        runtime_syncbase_instance_seconds(
            time,
            elapsed_s,
            animation_defs,
            animation_paths,
            controls,
            &mut Vec::new(),
            false,
        )
        .is_some_and(|begin| begin > elapsed_s)
    })
}

fn begin_seconds(
    anim: &SvgAnimationElement,
    animation_defs: &HashMap<String, SvgAnimationElement>,
) -> Option<f32> {
    begin_seconds_inner(anim, animation_defs, &mut Vec::new())
}

fn begin_seconds_inner(
    anim: &SvgAnimationElement,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    stack: &mut Vec<String>,
) -> Option<f32> {
    anim.begin
        .iter()
        .filter_map(|time| resolve_time_seconds(time, animation_defs, stack))
        .min_by(|a, b| a.total_cmp(b))
}

fn duration_seconds(anim: &SvgAnimationElement) -> Option<f32> {
    match anim.dur.as_ref()? {
        SvgAnimationTime::Seconds(s) => Some(*s),
        SvgAnimationTime::Indefinite
        | SvgAnimationTime::AccessKey { .. }
        | SvgAnimationTime::Event { .. }
        | SvgAnimationTime::Eventbase { .. }
        | SvgAnimationTime::Media
        | SvgAnimationTime::Wallclock(_)
        | SvgAnimationTime::Syncbase { .. } => None,
    }
}

fn min_seconds(anim: &SvgAnimationElement) -> Option<f32> {
    match anim.min.as_ref()? {
        SvgAnimationTime::Seconds(s) => Some(*s),
        SvgAnimationTime::Indefinite
        | SvgAnimationTime::AccessKey { .. }
        | SvgAnimationTime::Event { .. }
        | SvgAnimationTime::Eventbase { .. }
        | SvgAnimationTime::Media
        | SvgAnimationTime::Wallclock(_)
        | SvgAnimationTime::Syncbase { .. } => None,
    }
}

fn max_seconds(anim: &SvgAnimationElement) -> Option<f32> {
    match anim.max.as_ref()? {
        SvgAnimationTime::Seconds(s) => Some(*s),
        SvgAnimationTime::Indefinite
        | SvgAnimationTime::AccessKey { .. }
        | SvgAnimationTime::Event { .. }
        | SvgAnimationTime::Eventbase { .. }
        | SvgAnimationTime::Media
        | SvgAnimationTime::Wallclock(_)
        | SvgAnimationTime::Syncbase { .. } => None,
    }
}

fn first_static_end_seconds_inner(
    anim: &SvgAnimationElement,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    stack: &mut Vec<String>,
) -> Option<f32> {
    anim.end
        .iter()
        .filter_map(|time| resolve_time_seconds(time, animation_defs, stack))
        .min_by(|a, b| a.total_cmp(b))
}

fn active_duration_seconds(
    anim: &SvgAnimationElement,
    begin_s: f32,
    dur_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
) -> f32 {
    active_duration_seconds_inner(anim, begin_s, dur_s, animation_defs, &mut Vec::new())
}

fn active_duration_seconds_for_path(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    begin_s: f32,
    dur_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    _animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> f32 {
    let active = active_duration_seconds(anim, begin_s, dur_s, animation_defs);
    controls
        .iter()
        .filter(|(path, kind, at)| {
            path.as_slice() == animation_path && kind == "end" && *at >= begin_s
        })
        .map(|(_, _, at)| (*at - begin_s).max(0.0))
        .min_by(|a, b| a.total_cmp(b))
        .map(|runtime_end| active.min(runtime_end))
        .unwrap_or(active)
}

fn set_active_duration_seconds_for_path(
    anim: &SvgAnimationElement,
    animation_path: &[usize],
    begin_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
) -> f32 {
    if let Some(dur_s) = duration_seconds(anim) {
        return active_duration_seconds_for_path(
            anim,
            animation_path,
            begin_s,
            dur_s,
            animation_defs,
            animation_paths,
            controls,
        );
    }

    let static_end = first_static_end_seconds_inner(anim, animation_defs, &mut Vec::new())
        .map(|end_s| (end_s - begin_s).max(0.0));
    let runtime_end = controls
        .iter()
        .filter(|(path, kind, at)| {
            path.as_slice() == animation_path && kind == "end" && *at >= begin_s
        })
        .map(|(_, _, at)| (*at - begin_s).max(0.0))
        .min_by(|a, b| a.total_cmp(b));
    let mut active = match (static_end, runtime_end) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => f32::INFINITY,
    };
    if let Some(min_s) = min_seconds(anim) {
        active = active.max(min_s);
    }
    if let Some(max_s) = max_seconds(anim) {
        active = active.min(max_s);
    }
    active
}

fn active_duration_seconds_inner(
    anim: &SvgAnimationElement,
    begin_s: f32,
    dur_s: f32,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    stack: &mut Vec<String>,
) -> f32 {
    let repeated = match anim.repeat_count {
        SvgRepeatCount::Once => dur_s,
        SvgRepeatCount::Count(count) => dur_s * count.max(0.0),
        SvgRepeatCount::Indefinite => f32::INFINITY,
    };
    let repeated = if let Some(repeat_dur) = repeat_duration_seconds(anim) {
        repeated.min(repeat_dur)
    } else {
        repeated
    };
    let active = if let Some(end_s) = first_static_end_seconds_inner(anim, animation_defs, stack) {
        repeated.min((end_s - begin_s).max(0.0))
    } else {
        repeated
    };
    let active = if let Some(min_s) = min_seconds(anim) {
        active.max(min_s)
    } else {
        active
    };
    if let Some(max_s) = max_seconds(anim) {
        active.min(max_s)
    } else {
        active
    }
}

fn resolve_time_seconds(
    time: &SvgAnimationTime,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    stack: &mut Vec<String>,
) -> Option<f32> {
    match time {
        SvgAnimationTime::Seconds(s) => Some(*s),
        SvgAnimationTime::Indefinite
        | SvgAnimationTime::AccessKey { .. }
        | SvgAnimationTime::Event { .. }
        | SvgAnimationTime::Eventbase { .. }
        | SvgAnimationTime::Media
        | SvgAnimationTime::Wallclock(_) => None,
        SvgAnimationTime::Syncbase { id, event, offset } => {
            if stack.iter().any(|seen| seen == id) {
                return None;
            }
            let referenced = animation_defs.get(id)?;
            stack.push(id.clone());
            let base = match event {
                SvgSyncbaseEvent::Begin => begin_seconds_inner(referenced, animation_defs, stack),
                SvgSyncbaseEvent::End => {
                    let begin = begin_seconds_inner(referenced, animation_defs, stack)?;
                    let dur = duration_seconds(referenced)?;
                    Some(
                        begin
                            + active_duration_seconds_inner(
                                referenced,
                                begin,
                                dur,
                                animation_defs,
                                stack,
                            ),
                    )
                }
                SvgSyncbaseEvent::Repeat(repeat) => {
                    let begin = begin_seconds_inner(referenced, animation_defs, stack)?;
                    let dur = duration_seconds(referenced)?;
                    let active = active_duration_seconds_inner(
                        referenced,
                        begin,
                        dur,
                        animation_defs,
                        stack,
                    );
                    let repeat_at = begin + dur * *repeat as f32;
                    (repeat_at - begin <= active).then_some(repeat_at)
                }
            };
            stack.pop();
            base.map(|base| base + offset)
        }
    }
}

fn repeat_duration_seconds(anim: &SvgAnimationElement) -> Option<f32> {
    match anim.repeat_dur.as_ref()? {
        SvgAnimationTime::Seconds(s) => Some(*s),
        SvgAnimationTime::Indefinite => Some(f32::INFINITY),
        SvgAnimationTime::AccessKey { .. }
        | SvgAnimationTime::Event { .. }
        | SvgAnimationTime::Eventbase { .. }
        | SvgAnimationTime::Media
        | SvgAnimationTime::Wallclock(_)
        | SvgAnimationTime::Syncbase { .. } => None,
    }
}

fn sample_animation_at_progress(
    anim: &SvgAnimationElement,
    progress: f32,
    base_value: Option<&str>,
) -> Option<String> {
    if !anim.values.values.is_empty() {
        return sample_values_list(anim, progress);
    }
    match (&anim.values.from, &anim.values.to, &anim.values.by) {
        (Some(from), Some(to), _) => interpolate_svg_value(
            from,
            to,
            eased_segment_progress(anim, 0, progress),
            anim.calc_mode,
        ),
        (Some(from), None, Some(by)) => {
            let to = add_numeric_values(from, by)?;
            interpolate_svg_value(
                from,
                &to,
                eased_segment_progress(anim, 0, progress),
                anim.calc_mode,
            )
        }
        (None, Some(to), _) => {
            if let Some(from) = base_value {
                interpolate_svg_value(
                    from,
                    to,
                    eased_segment_progress(anim, 0, progress),
                    anim.calc_mode,
                )
            } else {
                Some(to.clone())
            }
        }
        (Some(from), None, None) => Some(from.clone()),
        (None, None, Some(by)) => {
            let from = base_value.unwrap_or("0");
            let to = add_numeric_values(from, by)?;
            interpolate_svg_value(
                from,
                &to,
                eased_segment_progress(anim, 0, progress),
                anim.calc_mode,
            )
        }
        (None, None, None) => None,
    }
}

fn sample_values_list(anim: &SvgAnimationElement, progress: f32) -> Option<String> {
    let values = &anim.values.values;
    if values.len() == 1 {
        return values.first().cloned();
    }
    if anim.calc_mode == SvgCalcMode::Discrete {
        return sample_discrete_values(values, &anim.key_times, progress);
    }
    if anim.calc_mode == SvgCalcMode::Paced {
        return sample_paced_values(values, progress);
    }
    let idx = segment_index(values.len(), &anim.key_times, progress);
    let from = values.get(idx)?;
    let to = values.get((idx + 1).min(values.len() - 1))?;
    let local_t = segment_progress(values.len(), &anim.key_times, idx, progress);
    let local_t = eased_segment_progress(anim, idx, local_t);
    interpolate_svg_value(from, to, local_t, anim.calc_mode)
}

fn sample_discrete_values(values: &[String], key_times: &[f32], progress: f32) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let progress = progress.clamp(0.0, 1.0);
    if key_times.len() == values.len() {
        let idx = key_times
            .iter()
            .enumerate()
            .rev()
            .find(|(_, key_time)| progress >= **key_time)
            .map(|(idx, _)| idx)
            .unwrap_or(0)
            .min(values.len() - 1);
        return values.get(idx).cloned();
    }
    let idx = ((progress * values.len() as f32).floor() as usize).min(values.len() - 1);
    values.get(idx).cloned()
}

fn eased_segment_progress(anim: &SvgAnimationElement, idx: usize, local_t: f32) -> f32 {
    if anim.calc_mode != SvgCalcMode::Spline {
        return local_t;
    }
    let Some([x1, y1, x2, y2]) = anim.key_splines.get(idx).copied() else {
        return local_t;
    };
    apply_easing(&EasingFn::CubicBezier(x1, y1, x2, y2), local_t).clamp(0.0, 1.0)
}

fn sample_paced_values(values: &[String], progress: f32) -> Option<String> {
    let parsed: Vec<Vec<(f32, String)>> = values
        .iter()
        .map(|value| parse_numeric_components(value))
        .collect::<Option<_>>()?;
    if parsed.len() < 2 || parsed.windows(2).any(|pair| pair[0].len() != pair[1].len()) {
        return None;
    }
    let mut distances = Vec::new();
    let mut total = 0.0;
    for pair in parsed.windows(2) {
        if pair[0]
            .iter()
            .zip(pair[1].iter())
            .any(|(a, b)| matching_numeric_suffix(&a.1, &b.1).is_none())
        {
            return None;
        }
        let distance = pair[0]
            .iter()
            .zip(pair[1].iter())
            .map(|(a, b)| (b.0 - a.0).powi(2))
            .sum::<f32>()
            .sqrt();
        distances.push(distance);
        total += distance;
    }
    if total <= 0.0 {
        return values.first().cloned();
    }
    let mut target = total * progress.clamp(0.0, 1.0);
    for (idx, distance) in distances.iter().enumerate() {
        if target <= *distance {
            let t = if *distance > 0.0 {
                target / *distance
            } else {
                0.0
            };
            return Some(
                parsed[idx]
                    .iter()
                    .zip(parsed[idx + 1].iter())
                    .map(|(a, b)| {
                        matching_numeric_suffix(&a.1, &b.1)
                            .map(|suffix| format_dimension(a.0 + (b.0 - a.0) * t, suffix))
                    })
                    .collect::<Option<Vec<_>>>()?
                    .join(" "),
            );
        }
        target -= *distance;
    }
    values.last().cloned()
}

fn segment_index(value_count: usize, key_times: &[f32], progress: f32) -> usize {
    if key_times.len() == value_count {
        for i in 0..key_times.len().saturating_sub(1) {
            if progress >= key_times[i] && progress <= key_times[i + 1] {
                return i;
            }
        }
        return value_count.saturating_sub(2);
    }
    let segment_count = value_count.saturating_sub(1).max(1);
    ((progress * segment_count as f32).floor() as usize).min(segment_count - 1)
}

fn segment_progress(value_count: usize, key_times: &[f32], idx: usize, progress: f32) -> f32 {
    if key_times.len() == value_count {
        let start = key_times[idx];
        let end = key_times[(idx + 1).min(key_times.len() - 1)];
        if end > start {
            return ((progress - start) / (end - start)).clamp(0.0, 1.0);
        }
        return 1.0;
    }
    let segment_count = value_count.saturating_sub(1).max(1) as f32;
    (progress * segment_count - idx as f32).clamp(0.0, 1.0)
}

fn interpolate_svg_value(from: &str, to: &str, t: f32, calc_mode: SvgCalcMode) -> Option<String> {
    if matches!(calc_mode, SvgCalcMode::Discrete) {
        return Some(if t < 1.0 { from } else { to }.to_string());
    }
    if let (Some(a), Some(b)) = (parse_color(from), parse_color(to)) {
        let mix = |x: u8, y: u8| x as f32 + (y as f32 - x as f32) * t;
        return Some(format!(
            "rgba({},{},{},{:.4})",
            mix(a.r, b.r).round() as u8,
            mix(a.g, b.g).round() as u8,
            mix(a.b, b.b).round() as u8,
            (a.a as f32 + (b.a as f32 - a.a as f32) * t) / 255.0
        ));
    }
    if let Some(value) = interpolate_number_lists(from, to, t) {
        return Some(value);
    }
    Some(if t < 1.0 { from } else { to }.to_string())
}

fn compose_additive_value(
    anim: &SvgAnimationElement,
    attr: &str,
    base_value: Option<&str>,
    sampled: &str,
) -> String {
    if anim.additive != SvgAdditiveMode::Sum {
        return sampled.to_string();
    }
    let Some(base) = base_value.map(str::trim).filter(|base| !base.is_empty()) else {
        return sampled.to_string();
    };
    if anim.kind == SvgAnimationKind::AnimateTransform || attr == "transform" {
        return format!("{base} {sampled}");
    }
    add_numeric_values(base, sampled).unwrap_or_else(|| sampled.to_string())
}

fn apply_accumulate_value(anim: &SvgAnimationElement, sampled: &str, iteration: u32) -> String {
    if anim.accumulate != SvgAccumulateMode::Sum || iteration == 0 {
        return sampled.to_string();
    }
    let Some(delta) = animation_delta(anim) else {
        return sampled.to_string();
    };
    let Some(offset) = scale_numeric_value(&delta, iteration as f32) else {
        return sampled.to_string();
    };
    add_numeric_values(sampled, &offset).unwrap_or_else(|| sampled.to_string())
}

fn animation_delta(anim: &SvgAnimationElement) -> Option<String> {
    if !anim.values.values.is_empty() {
        return subtract_numeric_values(anim.values.values.last()?, anim.values.values.first()?);
    }
    match (&anim.values.from, &anim.values.to, &anim.values.by) {
        (Some(from), Some(to), _) => subtract_numeric_values(to, from),
        (Some(_), None, Some(by)) | (None, None, Some(by)) => Some(by.clone()),
        _ => None,
    }
}

fn final_iteration(local_s: f32, dur_s: f32, active_s: f32) -> u32 {
    current_iteration(local_s.min(active_s), dur_s, active_s)
}

fn current_iteration(local_s: f32, dur_s: f32, active_s: f32) -> u32 {
    if dur_s <= 0.0 || !active_s.is_finite() {
        return 0;
    }
    if (local_s - active_s).abs() < 0.0001 && active_s > 0.0 {
        return ((active_s / dur_s).ceil().max(1.0) as u32).saturating_sub(1);
    }
    (local_s / dur_s).floor().max(0.0) as u32
}

fn add_numeric_values(a: &str, b: &str) -> Option<String> {
    let a = parse_numeric_components(a)?;
    let b = parse_numeric_components(b)?;
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    Some(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| {
                matching_numeric_suffix(&x.1, &y.1)
                    .map(|suffix| format_dimension(x.0 + y.0, suffix))
            })
            .collect::<Option<Vec<_>>>()?
            .join(" "),
    )
}

fn subtract_numeric_values(a: &str, b: &str) -> Option<String> {
    let a = parse_numeric_components(a)?;
    let b = parse_numeric_components(b)?;
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    Some(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| {
                matching_numeric_suffix(&x.1, &y.1)
                    .map(|suffix| format_dimension(x.0 - y.0, suffix))
            })
            .collect::<Option<Vec<_>>>()?
            .join(" "),
    )
}

fn scale_numeric_value(value: &str, factor: f32) -> Option<String> {
    let values = parse_numeric_components(value)?;
    if values.is_empty() {
        return None;
    }
    Some(
        values
            .iter()
            .map(|value| format_dimension(value.0 * factor, &value.1))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn interpolate_number_lists(from: &str, to: &str, t: f32) -> Option<String> {
    let a = parse_numeric_components(from)?;
    let b = parse_numeric_components(to)?;
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    Some(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| {
                matching_numeric_suffix(&x.1, &y.1)
                    .map(|suffix| format_dimension(x.0 + (y.0 - x.0) * t, suffix))
            })
            .collect::<Option<Vec<_>>>()?
            .join(" "),
    )
}

fn parse_number_tokens(raw: &str) -> Option<Vec<f32>> {
    parse_numeric_components(raw).map(|values| values.into_iter().map(|(value, _)| value).collect())
}

fn parse_numeric_components(raw: &str) -> Option<Vec<(f32, String)>> {
    raw.split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .filter(|part| !part.is_empty())
        .map(parse_numeric_component)
        .collect()
}

fn parse_numeric_component(part: &str) -> Option<(f32, String)> {
    let split = part
        .char_indices()
        .find(|(idx, ch)| {
            *idx > 0
                && !ch.is_ascii_digit()
                && *ch != '.'
                && *ch != 'e'
                && *ch != 'E'
                && *ch != '+'
                && *ch != '-'
        })
        .map(|(idx, _)| idx)
        .unwrap_or(part.len());
    let number = part[..split].parse::<f32>().ok()?;
    let suffix = part[split..].to_string();
    Some((number, suffix))
}

fn matching_numeric_suffix<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    if a == b {
        Some(a)
    } else if a.is_empty() {
        Some(b)
    } else if b.is_empty() {
        Some(a)
    } else {
        None
    }
}

fn format_dimension(value: f32, suffix: &str) -> String {
    format!("{}{}", format_number(value), suffix)
}

fn format_number(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 0.0001 {
        format!("{}", rounded.round() as i32)
    } else {
        format!("{rounded}")
    }
}

fn format_transform_animation_value(anim: &SvgAnimationElement, raw: &str) -> String {
    let value = raw.trim();
    if value.contains('(') {
        return value.to_string();
    }
    match anim
        .transform_type
        .unwrap_or(SvgAnimateTransformType::Translate)
    {
        SvgAnimateTransformType::Translate => format!("translate({value})"),
        SvgAnimateTransformType::Scale => format!("scale({value})"),
        SvgAnimateTransformType::Rotate => format!("rotate({value})"),
        SvgAnimateTransformType::SkewX => format!("skewX({value})"),
        SvgAnimateTransformType::SkewY => format!("skewY({value})"),
    }
}

fn resolve_animation_motion_path(
    anim: &SvgAnimationElement,
    node: &SvgNode,
    id_paths: &[(String, Vec<usize>, Option<String>)],
) -> Option<String> {
    if anim.kind != SvgAnimationKind::AnimateMotion {
        return None;
    }
    if let Some(path) = &anim.path {
        return Some(path.clone());
    }
    let href = node
        .children
        .iter()
        .find(|child| child.kind == SvgElementKind::MPath)
        .and_then(|child| {
            child
                .attr("href")
                .or_else(|| attr_ns(child, "xlink", "href"))
                .and_then(|href| href.strip_prefix('#'))
        })?;
    id_paths
        .iter()
        .find(|(id, _, _)| id == href)
        .and_then(|(_, _, d)| d.clone())
}

fn sample_motion_transform(
    anim: &SvgAnimationElement,
    elapsed_s: f32,
    motion_path: Option<&str>,
    base_value: Option<&str>,
    animation_defs: &HashMap<String, SvgAnimationElement>,
    animation_paths: &HashMap<String, Vec<usize>>,
    controls: &[SvgAnimationControl],
    animation_path: &[usize],
) -> Option<String> {
    let begin_s = begin_seconds_for_path(
        anim,
        animation_path,
        elapsed_s,
        animation_defs,
        animation_paths,
        controls,
    )?;
    if elapsed_s < begin_s {
        return None;
    }
    let dur_s = duration_seconds(anim)?;
    if dur_s <= 0.0 {
        return None;
    }
    let local_s = elapsed_s - begin_s;
    let active_s = active_duration_seconds_for_path(
        anim,
        animation_path,
        begin_s,
        dur_s,
        animation_defs,
        animation_paths,
        controls,
    );
    if local_s > active_s {
        return (anim.fill_mode == SvgAnimationFillMode::Freeze)
            .then(|| motion_transform_at_progress(anim, 1.0, motion_path, base_value))
            .flatten();
    }
    let progress = iteration_progress(local_s, dur_s, active_s);
    motion_transform_at_progress(anim, progress, motion_path, base_value)
}

fn iteration_progress(local_s: f32, dur_s: f32, active_s: f32) -> f32 {
    if dur_s <= 0.0 {
        return 1.0;
    }
    if (local_s - active_s).abs() < 0.0001 && active_s.is_finite() {
        return 1.0;
    }
    ((local_s % dur_s) / dur_s).clamp(0.0, 1.0)
}

fn motion_transform_at_progress(
    anim: &SvgAnimationElement,
    progress: f32,
    motion_path: Option<&str>,
    base_value: Option<&str>,
) -> Option<String> {
    let motion_progress = motion_path_progress(anim, progress);
    let (x, y, angle) = if let Some(path) = motion_path.or(anim.path.as_deref()) {
        sample_motion_path(path, motion_progress)?
    } else if !anim.values.values.is_empty() {
        let value = sample_values_list(anim, motion_progress)?;
        let nums = parse_number_tokens(&value)?;
        (*nums.first()?, *nums.get(1).unwrap_or(&0.0), 0.0)
    } else {
        return None;
    };

    let mut transform = format!("translate({} {})", format_number(x), format_number(y));
    if let Some(rotate) = &anim.rotate {
        let degrees = match rotate {
            SvgAnimateMotionRotate::Auto => angle,
            SvgAnimateMotionRotate::AutoReverse => angle + 180.0,
            SvgAnimateMotionRotate::Angle(deg) => *deg,
        };
        transform.push_str(&format!(" rotate({})", format_number(degrees)));
    }
    Some(compose_additive_value(
        anim,
        "transform",
        base_value,
        &transform,
    ))
}

fn motion_path_progress(anim: &SvgAnimationElement, progress: f32) -> f32 {
    if anim.key_points.len() < 2 {
        return progress;
    }
    let point_count = anim.key_points.len();
    let idx = segment_index(point_count, &anim.key_times, progress);
    let from = anim.key_points[idx];
    let to = anim.key_points[(idx + 1).min(point_count - 1)];
    let local_t = segment_progress(point_count, &anim.key_times, idx, progress);
    let local_t = eased_segment_progress(anim, idx, local_t);
    (from + (to - from) * local_t).clamp(0.0, 1.0)
}

fn sample_motion_path(path: &str, progress: f32) -> Option<(f32, f32, f32)> {
    let parsed = parse_path_data(path)?;
    let points = flatten_path_points(&parsed);
    if points.len() < 2 {
        return None;
    }
    let mut segments = Vec::new();
    let mut total = 0.0;
    for pair in points.windows(2) {
        let (x1, y1) = pair[0];
        let (x2, y2) = pair[1];
        let len = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
        if len > 0.0 {
            segments.push((x1, y1, x2, y2, len));
            total += len;
        }
    }
    if total <= 0.0 {
        return None;
    }
    let mut target = total * progress.clamp(0.0, 1.0);
    for (x1, y1, x2, y2, len) in segments {
        if target <= len {
            let t = if len > 0.0 { target / len } else { 0.0 };
            let x = x1 + (x2 - x1) * t;
            let y = y1 + (y2 - y1) * t;
            let angle = (y2 - y1).atan2(x2 - x1).to_degrees();
            return Some((x, y, angle));
        }
        target -= len;
    }
    let (x1, y1) = *points.get(points.len().saturating_sub(2))?;
    let (x2, y2) = *points.last()?;
    Some((x2, y2, (y2 - y1).atan2(x2 - x1).to_degrees()))
}

fn node_at_path_mut<'a>(node: &'a mut SvgNode, path: &[usize]) -> Option<&'a mut SvgNode> {
    let mut current = node;
    for idx in path {
        current = current.children.get_mut(*idx)?;
    }
    Some(current)
}

fn node_at_path_ref<'a>(node: &'a SvgNode, path: &[usize]) -> Option<&'a SvgNode> {
    let mut current = node;
    for idx in path {
        current = current.children.get(*idx)?;
    }
    Some(current)
}

fn animation_base_attr(node: &SvgNode, anim: &SvgAnimationElement) -> Option<String> {
    let attr = animation_target_attr_name(anim)?;
    node.attr(&attr).map(str::to_string)
}

fn animation_target_attr_name(anim: &SvgAnimationElement) -> Option<String> {
    let attr = if anim.kind == SvgAnimationKind::AnimateMotion {
        "transform"
    } else {
        anim.target_attribute.as_deref()?
    };
    Some(attr.to_string())
}

fn set_attr(node: &mut SvgNode, name: &str, value: &str) {
    if let Some(attr) = node
        .attributes
        .iter_mut()
        .find(|attr| attr.namespace.is_none() && attr.name == name)
    {
        attr.value = value.to_string();
    } else {
        node.attributes.push(SvgAttribute {
            namespace: None,
            name: name.to_string(),
            value: value.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::parse_svg_document;

    #[test]
    fn samples_basic_animate_fill() {
        let doc = parse_svg_document(
            r#"<svg><rect id="r" fill="red"><animate attributeName="fill" from="red" to="blue" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].0, vec![0]);
        assert_eq!(overrides[0].1, "fill");
        assert!(
            overrides[0].2.starts_with("rgba(128,0,128"),
            "expected mid red/blue fill, got {}",
            overrides[0].2
        );
    }

    #[test]
    fn animate_color_uses_existing_color_interpolation() {
        let doc = parse_svg_document(
            r#"<svg><rect fill="red"><animateColor attributeName="fill" from="red" to="blue" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        assert_eq!(
            doc.root.children[0].children[0]
                .animation
                .as_ref()
                .unwrap()
                .kind,
            SvgAnimationKind::AnimateColor
        );

        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert!(
            overrides[0].2.starts_with("rgba(128,0,128"),
            "expected midpoint purple fill, got {}",
            overrides[0].2
        );
    }

    #[test]
    fn parses_smil_clock_values() {
        assert_eq!(
            parse_time_value("2min"),
            Some(SvgAnimationTime::Seconds(120.0))
        );
        assert_eq!(
            parse_time_value("1.5h"),
            Some(SvgAnimationTime::Seconds(5400.0))
        );
        assert_eq!(
            parse_time_value("02:03"),
            Some(SvgAnimationTime::Seconds(123.0))
        );
        assert_eq!(
            parse_time_value("01:02:03.5"),
            Some(SvgAnimationTime::Seconds(3723.5))
        );
    }

    #[test]
    fn parses_smil_clock_offsets() {
        assert_eq!(
            parse_time_value("button.click+2min"),
            Some(SvgAnimationTime::Eventbase {
                id: "button".to_string(),
                event: "click".to_string(),
                offset: 120.0,
            })
        );
        assert_eq!(
            parse_time_value("accessKey(a)-00:02"),
            Some(SvgAnimationTime::AccessKey {
                key: "a".to_string(),
                offset: -2.0,
            })
        );
    }

    #[test]
    fn parses_media_and_wallclock_timing_as_typed_values() {
        assert_eq!(parse_time_value("media"), Some(SvgAnimationTime::Media));
        assert_eq!(
            parse_time_value("wallclock(2026-09-09T12:00:00Z)"),
            Some(SvgAnimationTime::Wallclock(
                "2026-09-09T12:00:00Z".to_string()
            ))
        );
        assert_eq!(
            parse_time_value("load"),
            Some(SvgAnimationTime::Event {
                event: "load".to_string(),
                offset: 0.0,
            })
        );
    }

    #[test]
    fn set_animation_removes_after_duration_by_default() {
        let doc = parse_svg_document(
            r#"<svg><rect><set attributeName="visibility" to="hidden" dur="1s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let active = sample_svg_animation_overrides(&doc, 0.5, &mut running);
        assert!(running);
        assert_eq!(
            active,
            vec![(vec![0], "visibility".to_string(), "hidden".to_string())]
        );

        running = false;
        let after = sample_svg_animation_overrides(&doc, 1.5, &mut running);
        assert!(!running);
        assert!(after.is_empty());
    }

    #[test]
    fn set_animation_freezes_after_duration_when_requested() {
        let doc = parse_svg_document(
            r#"<svg><rect><set attributeName="visibility" to="hidden" dur="1s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let after = sample_svg_animation_overrides(&doc, 1.5, &mut running);
        assert!(!running);
        assert_eq!(
            after,
            vec![(vec![0], "visibility".to_string(), "hidden".to_string())]
        );
    }

    #[test]
    fn set_animation_respects_runtime_end_control() {
        let doc = parse_svg_document(
            r#"<svg><rect><set attributeName="visibility" to="hidden" begin="click" end="mouseout"/></rect></svg>"#,
        )
        .unwrap();
        let controls = vec![
            (vec![0, 0], "begin".to_string(), 1.0),
            (vec![0, 0], "end".to_string(), 2.0),
        ];
        let mut running = false;
        let active =
            sample_svg_animation_overrides_with_controls(&doc, 1.5, &controls, &mut running);
        assert!(running);
        assert_eq!(
            active,
            vec![(vec![0], "visibility".to_string(), "hidden".to_string())]
        );

        running = false;
        let after =
            sample_svg_animation_overrides_with_controls(&doc, 2.5, &controls, &mut running);
        assert!(!running);
        assert!(after.is_empty());
    }

    #[test]
    fn samples_href_target_and_freezes_after_end() {
        let doc = parse_svg_document(
            r##"<svg><circle id="dot" cx="0"/><animate href="#dot" attributeName="cx" values="0;10;20" keyTimes="0;0.5;1" dur="2s" fill="freeze"/></svg>"##,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "cx".to_string(), "20".to_string())]
        );
    }

    #[test]
    fn repeat_count_keeps_animation_running_until_active_duration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="1s" repeatCount="2"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.25, &mut running);
        assert!(running);
        assert_eq!(overrides[0].2, "2.5");
    }

    #[test]
    fn animate_transform_wraps_sampled_values() {
        let doc = parse_svg_document(
            r#"<svg><rect><animateTransform attributeName="transform" type="rotate" values="0;90" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "transform".to_string(), "rotate(45)".to_string())]
        );
    }

    #[test]
    fn animate_motion_samples_path_as_transform() {
        let doc = parse_svg_document(
            r#"<svg><rect><animateMotion path="M 0 0 L 20 0" dur="2s" rotate="auto"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(
                vec![0],
                "transform".to_string(),
                "translate(10 0) rotate(0)".to_string()
            )]
        );
    }

    #[test]
    fn animate_motion_defaults_to_paced_calc_mode() {
        let doc = parse_svg_document(
            r#"<svg><rect><animateMotion values="0 0;100 0;110 0" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(
                vec![0],
                "transform".to_string(),
                "translate(55 0)".to_string()
            )]
        );
    }

    #[test]
    fn animate_motion_resolves_mpath_reference() {
        let doc = parse_svg_document(
            r##"<svg><defs><path id="track" d="M 0 0 L 0 20"/></defs><rect><animateMotion dur="2s" rotate="auto"><mpath href="#track"/></animateMotion></rect></svg>"##,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(
                vec![1],
                "transform".to_string(),
                "translate(0 10) rotate(90)".to_string()
            )]
        );
    }

    #[test]
    fn explicit_end_clamps_active_duration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="2s" end="1s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.5, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "10".to_string())]
        );
    }

    #[test]
    fn key_splines_ease_segment_progress() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;100" dur="2s" calcMode="spline" keySplines="0.42 0 1 1"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        let value = overrides[0].2.parse::<f32>().unwrap();
        assert!(
            value > 25.0 && value < 40.0,
            "ease-in midpoint should be behind linear midpoint, got {value}"
        );
    }

    #[test]
    fn paced_values_follow_numeric_distance() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;100;110" dur="2s" calcMode="paced"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(overrides[0].2, "55");
    }

    #[test]
    fn paced_values_preserve_matching_length_units() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0px;100px;110px" dur="2s" calcMode="paced"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(overrides[0].2, "55px");
    }

    #[test]
    fn discrete_values_use_equal_duration_buckets() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="visibility" values="hidden;visible" calcMode="discrete" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let first = sample_svg_animation_overrides(&doc, 0.75, &mut running);
        assert!(running);
        assert_eq!(
            first,
            vec![(vec![0], "visibility".to_string(), "hidden".to_string())]
        );

        running = false;
        let second = sample_svg_animation_overrides(&doc, 1.25, &mut running);
        assert!(running);
        assert_eq!(
            second,
            vec![(vec![0], "visibility".to_string(), "visible".to_string())]
        );
    }

    #[test]
    fn discrete_values_respect_key_times() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="display" values="none;inline;block" calcMode="discrete" keyTimes="0;0.25;0.75" dur="4s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let first = sample_svg_animation_overrides(&doc, 0.5, &mut running);
        assert!(running);
        assert_eq!(
            first,
            vec![(vec![0], "display".to_string(), "none".to_string())]
        );

        running = false;
        let second = sample_svg_animation_overrides(&doc, 2.0, &mut running);
        assert!(running);
        assert_eq!(
            second,
            vec![(vec![0], "display".to_string(), "inline".to_string())]
        );

        running = false;
        let third = sample_svg_animation_overrides(&doc, 3.5, &mut running);
        assert!(running);
        assert_eq!(
            third,
            vec![(vec![0], "display".to_string(), "block".to_string())]
        );
    }

    #[test]
    fn to_animation_interpolates_from_underlying_value() {
        let doc = parse_svg_document(
            r#"<svg><rect x="4"><animate attributeName="x" to="10" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(overrides, vec![(vec![0], "x".to_string(), "7".to_string())]);
    }

    #[test]
    fn interpolates_matching_length_units() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" from="10px" to="20px" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "15px".to_string())]
        );
    }

    #[test]
    fn interpolates_matching_percent_units() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="width" values="0%;100%" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "width".to_string(), "50%".to_string())]
        );
    }

    #[test]
    fn additive_transform_composes_with_base_transform() {
        let doc = parse_svg_document(
            r#"<svg><rect transform="translate(5 0)"><animateTransform attributeName="transform" type="translate" values="0 0;10 0" dur="2s" additive="sum"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(
                vec![0],
                "transform".to_string(),
                "translate(5 0) translate(5 0)".to_string()
            )]
        );
    }

    #[test]
    fn accumulate_adds_completed_iteration_delta() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="1s" repeatCount="3" accumulate="sum"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.5, &mut running);
        assert!(running);
        assert_eq!(overrides[0].2, "15");
    }

    #[test]
    fn repeat_dur_clamps_active_duration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="1s" repeatCount="indefinite" repeatDur="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 2.5, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "10".to_string())]
        );
    }

    #[test]
    fn syncbase_begin_uses_referenced_animation_end() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" dur="1s"/><animate attributeName="y" values="0;10" begin="a.end+0.5s" dur="1s"/></rect></svg>"##,
        )
        .unwrap();

        let mut running = false;
        let before = sample_svg_animation_overrides(&doc, 1.25, &mut running);
        assert_eq!(before.len(), 0);
        assert!(running);

        running = false;
        let during = sample_svg_animation_overrides(&doc, 2.0, &mut running);
        assert!(running);
        assert_eq!(during, vec![(vec![0], "y".to_string(), "5".to_string())]);
    }

    #[test]
    fn syncbase_repeat_event_starts_from_referenced_iteration() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" dur="1s" repeatCount="4"/><animate attributeName="y" values="0;10" begin="a.repeat(2)+0.5s" dur="1s"/></rect></svg>"##,
        )
        .unwrap();

        let mut running = false;
        let before = sample_svg_animation_overrides(&doc, 2.25, &mut running);
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].1, "x");
        assert!(running);

        running = false;
        let during = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(running);
        assert_eq!(during.len(), 2);
        assert_eq!(during[1], (vec![0], "y".to_string(), "5".to_string()));
    }

    #[test]
    fn runtime_syncbase_begin_starts_from_referenced_runtime_begin() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" begin="click" dur="2s"/><animate attributeName="y" values="0;10" begin="a.begin+0.5s" dur="2s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = vec![(vec![0, 0], "begin".to_string(), 1.0)];

        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 1.75, &controls, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![
                (vec![0], "x".to_string(), "3.75".to_string()),
                (vec![0], "y".to_string(), "1.25".to_string()),
            ]
        );
    }

    #[test]
    fn runtime_syncbase_end_starts_from_referenced_runtime_end() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" begin="click" dur="2s"/><animate attributeName="y" values="0;10" begin="a.end+0.5s" dur="2s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = vec![(vec![0, 0], "begin".to_string(), 1.0)];

        let mut running = false;
        let before =
            sample_svg_animation_overrides_with_controls(&doc, 3.25, &controls, &mut running);
        assert_eq!(before.len(), 0);
        assert!(running);

        running = false;
        let during =
            sample_svg_animation_overrides_with_controls(&doc, 3.75, &controls, &mut running);
        assert!(running);
        assert_eq!(during, vec![(vec![0], "y".to_string(), "1.25".to_string())]);
    }

    #[test]
    fn runtime_syncbase_repeat_starts_from_referenced_runtime_iteration() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" begin="click" dur="1s" repeatCount="4"/><animate attributeName="y" values="0;10" begin="a.repeat(2)+0.5s" dur="1s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = vec![(vec![0, 0], "begin".to_string(), 1.0)];

        let mut running = false;
        let before =
            sample_svg_animation_overrides_with_controls(&doc, 3.25, &controls, &mut running);
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].1, "x");
        assert!(running);

        running = false;
        let during =
            sample_svg_animation_overrides_with_controls(&doc, 3.75, &controls, &mut running);
        assert!(running);
        assert_eq!(during.len(), 2);
        assert_eq!(during[1], (vec![0], "y".to_string(), "2.5".to_string()));
    }

    #[test]
    fn animate_motion_key_points_remap_path_progress() {
        let doc = parse_svg_document(
            r#"<svg><rect><animateMotion path="M 0 0 L 100 0" dur="2s" keyPoints="0;0.25;1" keyTimes="0;0.5;1"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(
                vec![0],
                "transform".to_string(),
                "translate(25 0)".to_string()
            )]
        );
    }

    #[test]
    fn min_extends_active_duration_before_freeze() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="1s" min="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.5, &mut running);
        assert!(running);
        assert_eq!(overrides, vec![(vec![0], "x".to_string(), "5".to_string())]);
    }

    #[test]
    fn max_clamps_repeated_active_duration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="1s" repeatCount="5" max="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 2.5, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "10".to_string())]
        );
    }

    #[test]
    fn from_by_interpolates_to_from_plus_by() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" from="10" by="20" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "20".to_string())]
        );
    }

    #[test]
    fn by_only_interpolates_from_underlying_value() {
        let doc = parse_svg_document(
            r#"<svg><rect x="5"><animate attributeName="x" by="20" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "15".to_string())]
        );
    }

    #[test]
    fn by_only_freeze_preserves_underlying_value() {
        let doc = parse_svg_document(
            r#"<svg><rect x="5"><animate attributeName="x" by="20" dur="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "25".to_string())]
        );
    }

    #[test]
    fn from_by_freeze_uses_from_plus_by() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" from="10" by="20" dur="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "30".to_string())]
        );
    }

    #[test]
    fn to_animation_freeze_ends_at_to_value() {
        let doc = parse_svg_document(
            r#"<svg><rect x="5"><animate attributeName="x" to="20" dur="2s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "20".to_string())]
        );
    }

    #[test]
    fn accumulate_repeat_boundary_uses_last_completed_iteration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" from="0" to="10" dur="1s" repeatCount="2" accumulate="sum" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let at_end = sample_svg_animation_overrides(&doc, 2.0, &mut running);
        assert!(running);
        assert_eq!(at_end, vec![(vec![0], "x".to_string(), "20".to_string())]);

        running = false;
        let frozen = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(frozen, vec![(vec![0], "x".to_string(), "20".to_string())]);
    }

    #[test]
    fn accumulate_from_by_uses_by_as_iteration_delta() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" from="10" by="5" dur="1s" repeatCount="2" accumulate="sum" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let frozen = sample_svg_animation_overrides(&doc, 3.0, &mut running);
        assert!(!running);
        assert_eq!(frozen, vec![(vec![0], "x".to_string(), "20".to_string())]);
    }

    #[test]
    fn stacked_additive_animations_compose_in_document_order() {
        let doc = parse_svg_document(
            r#"<svg><rect x="5"><animate attributeName="x" from="0" to="10" dur="2s" additive="sum"/><animate attributeName="x" from="0" to="20" dur="2s" additive="sum"/></rect></svg>"#,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(running);
        assert_eq!(overrides.len(), 2);
        assert_eq!(overrides[0].2, "10");
        assert_eq!(overrides[1].2, "20");
    }

    #[test]
    fn syncbase_cycle_does_not_overflow() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate id="a" attributeName="x" values="0;10" begin="b.end" dur="1s"/><animate id="b" attributeName="y" values="0;10" begin="a.end" dur="1s"/></rect></svg>"##,
        )
        .unwrap();
        let mut running = false;
        let overrides = sample_svg_animation_overrides(&doc, 1.0, &mut running);
        assert!(!running);
        assert!(overrides.is_empty());
    }

    #[test]
    fn runtime_begin_control_starts_event_animation() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" begin="click" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let controls = vec![(vec![0, 0], "begin".to_string(), 1.0)];
        let mut running = false;
        let before =
            sample_svg_animation_overrides_with_controls(&doc, 0.5, &controls, &mut running);
        assert!(before.is_empty());

        running = false;
        let during =
            sample_svg_animation_overrides_with_controls(&doc, 2.0, &controls, &mut running);
        assert!(running);
        assert_eq!(during, vec![(vec![0], "x".to_string(), "5".to_string())]);
    }

    #[test]
    fn runtime_event_offset_delays_matching_animation() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" begin="click+0.5s" dur="2s"/></rect></svg>"#,
        )
        .unwrap();
        let controls = animation_controls_for_event(&doc, &[0], "click", 1.0);
        assert_eq!(controls, vec![(vec![0, 0], "begin".to_string(), 1.5)]);
    }

    #[test]
    fn eventbase_begin_uses_referenced_target_event_with_offset() {
        let doc = parse_svg_document(
            r##"<svg><rect id="button"/><circle id="dot" cx="0"><animate attributeName="cx" values="0;10" begin="button.click+0.5s" dur="2s"/></circle></svg>"##,
        )
        .unwrap();
        let controls = animation_controls_for_event(&doc, &[0], "click", 1.0);
        assert_eq!(controls, vec![(vec![1, 0], "begin".to_string(), 1.5)]);

        let mut running = false;
        let before =
            sample_svg_animation_overrides_with_controls(&doc, 1.25, &controls, &mut running);
        assert!(before.is_empty());
        assert!(running);

        running = false;
        let during =
            sample_svg_animation_overrides_with_controls(&doc, 2.5, &controls, &mut running);
        assert!(running);
        assert_eq!(during, vec![(vec![1], "cx".to_string(), "5".to_string())]);
    }

    #[test]
    fn eventbase_negative_offset_can_start_animation_before_event_time() {
        let doc = parse_svg_document(
            r##"<svg><rect id="button"/><circle id="dot" cx="0"><animate attributeName="cx" values="0;10" begin="button.click-0.5s" dur="2s"/></circle></svg>"##,
        )
        .unwrap();
        let controls = animation_controls_for_event(&doc, &[0], "click", 1.0);
        assert_eq!(controls, vec![(vec![1, 0], "begin".to_string(), 0.5)]);

        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 1.0, &controls, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![1], "cx".to_string(), "2.5".to_string())]
        );
    }

    #[test]
    fn access_key_begin_starts_matching_animation() {
        let doc = parse_svg_document(
            r##"<svg><rect x="0"><animate attributeName="x" values="0;10" begin="accessKey(m)" dur="2s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = animation_controls_for_access_key(&doc, "m", 1.0);
        assert_eq!(controls, vec![(vec![0, 0], "begin".to_string(), 1.0)]);

        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 2.0, &controls, &mut running);
        assert!(running);
        assert_eq!(overrides, vec![(vec![0], "x".to_string(), "5".to_string())]);
    }

    #[test]
    fn access_key_matching_is_ascii_case_insensitive() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate attributeName="x" values="0;10" begin="accessKey(M)" dur="2s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = animation_controls_for_access_key(&doc, "m", 1.0);
        assert_eq!(controls, vec![(vec![0, 0], "begin".to_string(), 1.0)]);
    }

    #[test]
    fn access_key_offset_delays_matching_animation() {
        let doc = parse_svg_document(
            r##"<svg><rect><animate attributeName="x" values="0;10" begin="accessKey(m)+0.25s" dur="2s"/></rect></svg>"##,
        )
        .unwrap();
        let controls = animation_controls_for_access_key(&doc, "m", 1.0);
        assert_eq!(controls, vec![(vec![0, 0], "begin".to_string(), 1.25)]);
    }

    #[test]
    fn restart_never_ignores_later_runtime_begins() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" begin="click" dur="2s" restart="never"/></rect></svg>"#,
        )
        .unwrap();
        let controls = vec![
            (vec![0, 0], "begin".to_string(), 1.0),
            (vec![0, 0], "begin".to_string(), 3.0),
        ];
        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 3.5, &controls, &mut running);
        assert!(!running);
        assert!(overrides.is_empty());
    }

    #[test]
    fn restart_when_not_active_accepts_only_inactive_runtime_begins() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" begin="click" dur="2s" restart="whenNotActive"/></rect></svg>"#,
        )
        .unwrap();
        let controls = vec![
            (vec![0, 0], "begin".to_string(), 1.0),
            (vec![0, 0], "begin".to_string(), 2.0),
            (vec![0, 0], "begin".to_string(), 4.0),
        ];
        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 4.5, &controls, &mut running);
        assert!(running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "2.5".to_string())]
        );
    }

    #[test]
    fn runtime_end_control_clamps_active_duration() {
        let doc = parse_svg_document(
            r#"<svg><rect><animate attributeName="x" values="0;10" dur="4s" fill="freeze"/></rect></svg>"#,
        )
        .unwrap();
        let controls = vec![(vec![0, 0], "end".to_string(), 1.0)];
        let mut running = false;
        let overrides =
            sample_svg_animation_overrides_with_controls(&doc, 2.0, &controls, &mut running);
        assert!(!running);
        assert_eq!(
            overrides,
            vec![(vec![0], "x".to_string(), "10".to_string())]
        );
    }
}
