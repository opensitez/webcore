//! Helpers for driving CSS animations.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Animation helpers ────────────────────────────────────────────────────

/// Extract the CSS properties that can participate in transitions from a style.
/// Values are serialised to `rgba(…)` or `Npx` strings for comparison/interpolation.
/// Find the node_id of the nearest ancestor of `target_id` that has a valid node_id.
/// Used when hit-test returns a node with node_id=0 (e.g. pseudo-elements, post-process nodes).
pub fn find_parent_node_id_by_id(root: &WebCore, target_id: u32) -> u32 {
    fn walk(node: &WebCore, target_id: u32) -> Option<u32> {
        for child in &node.children {
            if child.node_id == target_id {
                return if node.node_id != 0 {
                    Some(node.node_id)
                } else {
                    None
                };
            }
            if let Some(id) = walk(child, target_id) {
                return Some(id);
            }
        }
        None
    }
    walk(root, target_id).unwrap_or(0)
}

/// Find the node_id of an <a> element with the given href.
pub fn find_link_node_id(root: &WebCore, href: &str) -> Option<u32> {
    if root.tag == "a" && root.node_id != 0 && root.style.href == href {
        return Some(root.node_id);
    }
    for child in &root.children {
        if let Some(id) = find_link_node_id(child, href) {
            return Some(id);
        }
    }
    None
}

pub(crate) fn subtree_contains_id(node: &WebCore, target_id: u32) -> bool {
    if node.node_id == target_id {
        return true;
    }
    for child in &node.children {
        if subtree_contains_id(child, target_id) {
            return true;
        }
    }
    false
}

pub(crate) fn extract_transitionable(node: &WebCore) -> HashMap<String, String> {
    extract_transitionable_style(&node.style)
}

pub(crate) fn extract_transitionable_style(s: &ComputedStyle) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("opacity".into(), format!("{}", s.opacity));
    m.insert(
        "visibility".into(),
        format!("{:?}", s.visibility).to_ascii_lowercase(),
    );
    m.insert("display".into(), transition_display(s.display));
    m.insert(
        "content-visibility".into(),
        match s.content_visibility {
            ContentVisibility::Visible => "visible",
            ContentVisibility::Auto => "auto",
            ContentVisibility::Hidden => "hidden",
        }
        .to_string(),
    );
    m.insert("color".into(), color_to_rgba(s.color));
    m.insert("background-color".into(), color_to_rgba(s.background_color));
    if let Some(fill) = s.svg_fill {
        m.insert("fill".into(), color_to_rgba(fill));
    }
    if let Some(stroke) = s.svg_stroke {
        m.insert("stroke".into(), color_to_rgba(stroke));
    }
    m.insert("border-top-color".into(), color_to_rgba(s.border_top_color));
    m.insert(
        "border-right-color".into(),
        color_to_rgba(s.border_right_color),
    );
    m.insert(
        "border-bottom-color".into(),
        color_to_rgba(s.border_bottom_color),
    );
    m.insert(
        "border-left-color".into(),
        color_to_rgba(s.border_left_color),
    );
    m.insert("border-color".into(), color_to_rgba(s.border_top_color));
    m.insert("transform".into(), s.transform.clone());
    m.insert(
        "font-size".into(),
        format!("{}px", s.font_size_px(16.0, 16.0)),
    );
    m.insert("width".into(), transition_length(&s.width));
    m.insert("height".into(), transition_length(&s.height));
    m.insert("min-width".into(), transition_length(&s.min_width));
    m.insert("min-height".into(), transition_length(&s.min_height));
    m.insert("max-width".into(), transition_length(&s.max_width));
    m.insert("max-height".into(), transition_length(&s.max_height));
    m.insert("margin-top".into(), transition_length(&s.margin_top));
    m.insert("margin-right".into(), transition_length(&s.margin_right));
    m.insert("margin-bottom".into(), transition_length(&s.margin_bottom));
    m.insert("margin-left".into(), transition_length(&s.margin_left));
    m.insert("padding-top".into(), transition_length(&s.padding_top));
    m.insert("padding-right".into(), transition_length(&s.padding_right));
    m.insert(
        "padding-bottom".into(),
        transition_length(&s.padding_bottom),
    );
    m.insert("padding-left".into(), transition_length(&s.padding_left));
    m.insert("top".into(), transition_length(&s.top));
    m.insert("right".into(), transition_length(&s.right));
    m.insert("bottom".into(), transition_length(&s.bottom));
    m.insert("left".into(), transition_length(&s.left));
    m.insert(
        "border-top-width".into(),
        transition_length(&s.border_top_width),
    );
    m.insert(
        "border-right-width".into(),
        transition_length(&s.border_right_width),
    );
    m.insert(
        "border-bottom-width".into(),
        transition_length(&s.border_bottom_width),
    );
    m.insert(
        "border-left-width".into(),
        transition_length(&s.border_left_width),
    );
    m.insert("border-radius".into(), transition_length(&s.border_radius));
    m.insert(
        "letter-spacing".into(),
        transition_length(&s.letter_spacing),
    );
    m.insert("word-spacing".into(), transition_length(&s.word_spacing));
    m.insert("text-indent".into(), transition_length(&s.text_indent));
    m.insert("gap".into(), transition_length(&s.gap));
    m.insert("row-gap".into(), transition_length(&s.row_gap));
    m.insert("column-gap".into(), transition_length(&s.column_gap));
    m.insert("flex-basis".into(), transition_length(&s.flex_basis));
    m
}

fn transition_display(display: Display) -> String {
    match display {
        Display::None => "none",
        Display::Block => "block",
        Display::Inline => "inline",
        Display::InlineBlock => "inline-block",
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid => "grid",
        Display::InlineGrid => "inline-grid",
        Display::Table => "table",
        Display::TableRow => "table-row",
        Display::TableCell | Display::TableHeaderCell => "table-cell",
        Display::TableRowGroup => "table-row-group",
        Display::TableHeaderGroup => "table-header-group",
        Display::TableFooterGroup => "table-footer-group",
        Display::TableColumnGroup => "table-column-group",
        Display::TableColumn => "table-column",
        Display::TableCaption => "table-caption",
        Display::ListItem => "list-item",
        Display::Ruby => "ruby",
        Display::RubyText => "ruby-text",
        Display::FlowRoot => "flow-root",
        Display::Contents => "contents",
    }
    .to_string()
}

fn transition_length(v: &CssLength) -> String {
    match v {
        CssLength::Px(n) => format!("{n}px"),
        CssLength::Em(n) => format!("{n}em"),
        CssLength::Rem(n) => format!("{n}rem"),
        CssLength::Percent(n) => format!("{n}%"),
        CssLength::Vw(n) => format!("{n}vw"),
        CssLength::Vh(n) => format!("{n}vh"),
        CssLength::Vmin(n) => format!("{n}vmin"),
        CssLength::Vmax(n) => format!("{n}vmax"),
        CssLength::Zero => "0px".to_string(),
        CssLength::Auto => "auto".to_string(),
        CssLength::None => "none".to_string(),
        CssLength::Content => "content".to_string(),
        CssLength::MinContent => "min-content".to_string(),
        CssLength::MaxContent => "max-content".to_string(),
        CssLength::FitContent => "fit-content".to_string(),
        CssLength::FitContentArg(_)
        | CssLength::Calc(_)
        | CssLength::CalcExpr(_)
        | CssLength::Min(_)
        | CssLength::Max(_)
        | CssLength::Clamp(_) => {
            format!("{}px", v.resolve_vp(16.0, 0.0, 16.0, 0.0, 0.0))
        }
    }
}

fn color_to_rgba(c: Color) -> String {
    format!("rgba({},{},{},{:.4})", c.r, c.g, c.b, c.a as f32 / 255.0)
}

/// Find the two surrounding keyframe stops for `t` and return interpolated properties.
pub(crate) fn interpolate_keyframe_stops(stops: &[KeyframeStop], t: f32) -> Vec<(String, String)> {
    if stops.is_empty() {
        return Vec::new();
    }
    if stops.len() == 1 {
        return stops[0].properties.clone();
    }

    // Find surrounding stops.
    let (from, to, local_t) = if t <= stops[0].offset {
        (&stops[0], &stops[0], 0.0f32)
    } else if t >= stops[stops.len() - 1].offset {
        let last = &stops[stops.len() - 1];
        (last, last, 1.0f32)
    } else {
        let mut fi = 0usize;
        for i in 0..stops.len() - 1 {
            if t >= stops[i].offset && t <= stops[i + 1].offset {
                fi = i;
                break;
            }
        }
        let ti = fi + 1;
        let range = stops[ti].offset - stops[fi].offset;
        let lt = if range > 1e-6 {
            (t - stops[fi].offset) / range
        } else {
            0.0
        };
        (&stops[fi], &stops[ti], lt)
    };

    let mut props = Vec::new();
    for (prop, _) in &from.properties {
        if !props.iter().any(|p: &String| p == prop) {
            props.push(prop.clone());
        }
    }
    for (prop, _) in &to.properties {
        if !props.iter().any(|p| p == prop) {
            props.push(prop.clone());
        }
    }

    let mut result = Vec::new();
    for prop in props {
        let from_val = from
            .properties
            .iter()
            .find(|(p, _)| p == &prop)
            .map(|(_, v)| v.as_str());
        let to_val = to
            .properties
            .iter()
            .find(|(p, _)| p == &prop)
            .map(|(_, v)| v.as_str());
        let fallback = from_val.or(to_val).unwrap_or("");
        result.push((
            prop.clone(),
            interpolate_property_value(
                &prop,
                from_val.unwrap_or(fallback),
                to_val.unwrap_or(fallback),
                local_t,
            ),
        ));
    }
    result
}

/// Interpolate a named CSS property between two value strings.
pub(crate) fn interpolate_property_value(prop: &str, from: &str, to: &str, t: f32) -> String {
    if prop == "visibility" && (from == "visible" || to == "visible") {
        if t > 0.0 && t < 1.0 {
            return "visible".to_string();
        }
    }
    interpolate_value(from, to, t)
}

pub(crate) fn is_discrete_transition_property(prop: &str) -> bool {
    matches!(prop, "display" | "content-visibility")
}

pub(crate) fn discrete_transition_value(prop: &str, from: &str, to: &str, t: f32) -> String {
    match prop {
        "display" => {
            if from == "none" && to != "none" {
                to.to_string()
            } else if to == "none" && from != "none" {
                if t >= 1.0 { to } else { from }.to_string()
            } else if t < 0.5 {
                from.to_string()
            } else {
                to.to_string()
            }
        }
        "content-visibility" => {
            if from == "hidden" && to != "hidden" {
                to.to_string()
            } else if to == "hidden" && from != "hidden" {
                if t >= 1.0 { to } else { from }.to_string()
            } else if t < 0.5 {
                from.to_string()
            } else {
                to.to_string()
            }
        }
        _ => {
            if t < 0.5 {
                from.to_string()
            } else {
                to.to_string()
            }
        }
    }
}

/// Interpolate between two CSS value strings.
/// Handles `rgba(…)` colors and strings containing numbers.
pub(crate) fn interpolate_value(from: &str, to: &str, t: f32) -> String {
    if let Some(c) = interpolate_color(from, to, t) {
        return c;
    }
    // For transforms, if one side is empty/none, synthesize the identity form.
    let (from, to) = if from.is_empty() || from == "none" {
        (transform_identity(to).into(), to.to_string())
    } else if to.is_empty() || to == "none" {
        (from.to_string(), transform_identity(from).into())
    } else {
        (from.to_string(), to.to_string())
    };
    interpolate_numeric(&from, &to, t)
}

/// Given a CSS transform string like `rotate(180deg)`, return the identity form
/// with the same function and matching zero-ish arguments: `rotate(0deg)`.
fn transform_identity(transform: &str) -> String {
    let s = transform.trim();
    if s.is_empty() || s == "none" {
        return String::new();
    }

    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let func = rest[..open].trim();
        if func.is_empty() {
            break;
        }
        let mut depth = 0usize;
        let mut close = None;
        for (i, ch) in rest[open..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else { break };
        let inner = &rest[open + 1..close];
        let identity_val = if func.starts_with("scale") { "1" } else { "0" };
        let args: Vec<String> = split_transform_args(inner)
            .into_iter()
            .map(|arg| format!("{}{}", identity_val, transform_arg_unit(arg)))
            .collect();
        out.push(format!("{}({})", func, args.join(", ")));
        rest = rest[close + 1..].trim_start();
    }
    out.join(" ")
}

fn split_transform_args(inner: &str) -> Vec<&str> {
    let comma_parts: Vec<&str> = inner
        .split(',')
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .collect();
    if comma_parts.len() > 1 {
        comma_parts
    } else {
        inner.split_whitespace().collect()
    }
}

fn transform_arg_unit(arg: &str) -> &str {
    let arg = arg.trim();
    let mut seen_digit = false;
    for (idx, ch) in arg.char_indices() {
        if ch == '+' || ch == '-' || ch == '.' || ch.is_ascii_digit() {
            if ch.is_ascii_digit() {
                seen_digit = true;
            }
            continue;
        }
        if seen_digit {
            return &arg[idx..];
        }
    }
    ""
}

fn interpolate_color(from: &str, to: &str, t: f32) -> Option<String> {
    let (fr, fg, fb, fa) = parse_rgba(from)?;
    let (tr, tg, tb, ta) = parse_rgba(to)?;
    let a = lerp(fa, ta, t);
    let pr = lerp(fr * fa, tr * ta, t);
    let pg = lerp(fg * fa, tg * ta, t);
    let pb = lerp(fb * fa, tb * ta, t);
    let unpremul = |c: f32| {
        if a <= 0.0 {
            0
        } else {
            (c / a).round().clamp(0.0, 255.0) as u8
        }
    };
    let r = unpremul(pr);
    let g = unpremul(pg);
    let b = unpremul(pb);
    Some(format!("rgba({},{},{},{:.4})", r, g, b, a))
}

fn parse_rgba(s: &str) -> Option<(f32, f32, f32, f32)> {
    let s = s.trim();
    let inner = s.strip_prefix("rgba(")?.strip_suffix(')')?;
    let mut it = inner.split(',');
    let r = it.next()?.trim().parse::<f32>().ok()?;
    let g = it.next()?.trim().parse::<f32>().ok()?;
    let b = it.next()?.trim().parse::<f32>().ok()?;
    let a = it.next()?.trim().parse::<f32>().ok()?;
    Some((r, g, b, a))
}

/// Interpolate by extracting all decimal numbers from both strings and lerping them.
fn interpolate_numeric(from: &str, to: &str, t: f32) -> String {
    let from_nums = extract_nums(from);
    let to_nums = extract_nums(to);

    if from_nums.is_empty() || from_nums.len() != to_nums.len() {
        return if t < 0.5 {
            from.to_string()
        } else {
            to.to_string()
        };
    }

    let mut result = from.to_string();
    // Replace in reverse order so byte offsets remain valid.
    for ((start, end, fv), (_, to_end, tv)) in from_nums.iter().zip(to_nums.iter()).rev() {
        let v = lerp(*fv, *tv, t);
        let mut s = if v == v.floor() && v.abs() < 1e9 {
            format!("{}", v as i64)
        } else {
            format!("{:.4}", v)
        };
        let from_unit = numeric_unit_after(from, *end);
        let to_unit = numeric_unit_after(to, *to_end);
        if from_unit.is_empty() && !to_unit.is_empty() && fv.abs() < 1e-6 {
            s.push_str(to_unit);
        }
        result.replace_range(start..end, &s);
    }
    result
}

fn numeric_unit_after(s: &str, end: usize) -> &str {
    let bytes = s.as_bytes();
    let mut i = end;
    while i < bytes.len() {
        let ch = bytes[i];
        if ch == b'%' || ch.is_ascii_alphabetic() {
            i += 1;
        } else {
            break;
        }
    }
    &s[end..i]
}

/// Extract `(start_byte, end_byte, value)` for every number in `s`.
fn extract_nums(s: &str) -> Vec<(usize, usize, f32)> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let neg = bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit();
        if bytes[i].is_ascii_digit() || neg {
            let start = i;
            if neg {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if let Ok(v) = s[start..i].parse::<f32>() {
                result.push((start, i, v));
            }
        } else {
            i += 1;
        }
    }
    result
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Apply an easing function to a linear progress value in 0.0..=1.0.
pub(crate) fn apply_easing(easing: &EasingFn, t: f32) -> f32 {
    match easing {
        EasingFn::Linear => t,
        EasingFn::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
        EasingFn::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
        EasingFn::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
        EasingFn::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
        EasingFn::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, t),
        // `step-start` IS `steps(1, jump-start)` and `step-end` is
        // `steps(1, jump-end)` — css-easing-2 §2.3. Written out separately
        // they drifted: `step-start` returned 0 at t=0, which is step-END's
        // answer, so the first frame of every step-start animation was wrong.
        EasingFn::StepStart => steps_easing(1, StepPosition::JumpStart, t),
        EasingFn::StepEnd => steps_easing(1, StepPosition::JumpEnd, t),
        EasingFn::Steps(n, pos) => steps_easing(*n, *pos, t),
        EasingFn::LinearPoints(pts) => linear_points_easing(pts, t),
    }
}

/// A `steps()` easing — css-easing-2 §2.3's own algorithm.
///
/// The output is `current step / jumps`, where the number of JUMPS depends on
/// the position: `steps` for jump-start/jump-end, `steps - 1` for jump-none
/// and `steps + 1` for jump-both. That is why the position cannot be a
/// boolean: it changes the denominator, not only the offset.
fn steps_easing(steps: u32, pos: StepPosition, t: f32) -> f32 {
    let steps = steps.max(1) as i64;
    let mut current = (t * steps as f32).floor() as i64;
    if matches!(pos, StepPosition::JumpStart | StepPosition::JumpBoth) {
        current += 1;
    }
    if t >= 0.0 && current < 0 {
        current = 0;
    }
    let jumps = match pos {
        StepPosition::JumpNone => steps - 1,
        StepPosition::JumpBoth => steps + 1,
        _ => steps,
    };
    if jumps <= 0 {
        return 0.0;
    }
    if t <= 1.0 && current > jumps {
        current = jumps;
    }
    current as f32 / jumps as f32
}

/// A `linear()` easing — css-easing-2 §2.1: linear interpolation between the
/// control points, clamped to the ends.
fn linear_points_easing(pts: &[(f32, f32)], t: f32) -> f32 {
    if pts.is_empty() {
        return t;
    }
    if t <= pts[0].0 {
        return pts[0].1;
    }
    let last = pts.len() - 1;
    if t >= pts[last].0 {
        return pts[last].1;
    }
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if t >= x0 && t <= x1 {
            if (x1 - x0).abs() < f32::EPSILON {
                return y1;
            }
            return y0 + (y1 - y0) * (t - x0) / (x1 - x0);
        }
    }
    pts[last].1
}

/// CSS cubic-bezier evaluation via Newton-Raphson.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let mut u = t;
    for _ in 0..8 {
        let bx = bcoord(x1, x2, u) - t;
        let db = bderiv(x1, x2, u);
        if db.abs() < 1e-6 {
            break;
        }
        u = (u - bx / db).clamp(0.0, 1.0);
    }
    bcoord(y1, y2, u)
}

fn bcoord(p1: f32, p2: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t2 * p2 + t3
}

fn bderiv(p1: f32, p2: f32, t: f32) -> f32 {
    3.0 * (1.0 - t) * (1.0 - t) * p1 + 6.0 * (1.0 - t) * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

impl Document {
    /// Dispatch a DOM event from inside the engine.
    ///
    /// Moves the listener map out so handlers can be given `&mut Document`,
    /// merges back anything they registered, and sweeps `once` listeners.
    /// Every engine dispatch goes through here so none of them can forget a
    /// step.
    pub fn dispatch_dom_event(&mut self, event: &mut crate::dom::events::DomEvent) -> bool {
        if event.target == 0 {
            return false;
        }
        // DOM §2.9's dispatch flag: an event already in flight cannot be
        // dispatched again. Without the guard a handler that re-dispatches its
        // own event recurses until the stack runs out.
        if event.is_dispatching() {
            return false;
        }
        // `Window` is an EventTarget but not a node, so it has no tree path —
        // the event fires on it directly.
        if crate::dom::events::is_window_target(event.target) {
            let map = std::mem::take(&mut self.event_targets);
            let handled = map.dispatch_path(event, &[crate::dom::events::WINDOW_TARGET], self);
            let added = std::mem::replace(&mut self.event_targets, map);
            self.event_targets.merge_from(added);
            self.event_targets.sweep_removed();
            return handled;
        }
        let map = std::mem::take(&mut self.event_targets);
        let root = std::mem::replace(&mut self.root, WebCore::new("#placeholder"));
        let handled = map.dispatch_on_tree(&root, event, self);
        self.root = root;
        let added = std::mem::replace(&mut self.event_targets, map);
        self.event_targets.merge_from(added);
        self.event_targets.sweep_removed();
        handled
    }
}

impl Document {
    /// The `Window` event target — `window.addEventListener(...)`.
    pub fn window_target(&self) -> u32 {
        crate::dom::events::WINDOW_TARGET
    }

    /// Fire a window-level event: `load`, `resize`, `scroll`, `popstate`,
    /// `hashchange`, `beforeunload` and the rest of `WindowEventHandlers`.
    ///
    /// Returns false if a handler cancelled it, which is what `beforeunload`
    /// and `unload` are asked for.
    pub fn fire_window_event(&mut self, event_type: &str) -> bool {
        let mut e =
            crate::dom::events::DomEvent::new(event_type, crate::dom::events::WINDOW_TARGET);
        self.dispatch_dom_event(&mut e);
        !e.default_prevented()
    }
}

impl Document {
    /// Dispatch an engine input event to BOTH listener systems.
    ///
    /// `HtmlEvent` is the engine's internal input record and `DomEvent` is the
    /// WHATWG one; every input has to reach both or half the listeners on a
    /// page never run. Only `click`, `mouseover`, `mouseout` and the keyboard
    /// types were bridged, so `addEventListener("input")`, `"change"`,
    /// `"submit"`, `"focus"` and `"blur"` — among the most-used handlers on the
    /// web — were registered, stored, and never called.
    ///
    /// Returns whether anything handled it. A DOM listener that cancels the
    /// event cancels the default action too.
    pub fn dispatch_input_event(
        &mut self,
        mut evt: crate::dom::HtmlEvent,
    ) -> (bool, crate::dom::HtmlEvent) {
        let handled = false;
        if evt.target == 0 {
            return (handled, evt);
        }
        let mut dom = crate::dom::events::DomEvent::new(evt.event_type.as_str(), evt.target);
        dom.client_x = evt.client_pos.0;
        dom.client_y = evt.client_pos.1;
        dom.button = evt.button;
        dom.key_code = evt.key_code;
        dom.char_code = evt.char_code;
        dom.ctrl_key = evt.ctrl_key;
        dom.shift_key = evt.shift_key;
        dom.alt_key = evt.alt_key;
        dom.meta_key = evt.meta_key;
        dom.delta_x = evt.delta_x;
        dom.delta_y = evt.delta_y;
        dom.related_target = evt.related_target;
        dom.key = match evt.char_code {
            Some(c) => c.to_string(),
            None => crate::dom::events::key_name_for_code(evt.key_code).to_string(),
        };
        dom.set_scroll_offset(self.scroll_x, self.scroll_y);
        let dom_handled = self.dispatch_dom_event(&mut dom);
        if dom.default_prevented() {
            evt.default_prevented = true;
        }
        (handled || dom_handled, evt)
    }
}
