//! The cascade: the public entry points AND the core walk they run.
//!
//! ⛔ `apply_cascade_inner` used to live in `cascade_incremental.rs`, which
//! left this file as three wrappers and put the actual cascade behind a name
//! that said "incremental". The incremental HOVER path is the other file's
//! subject; the walk is this one's.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Cascade ─────────────────────────────────────────────────────────────

fn normal_cascade_cmp(
    rules: &[CssRule],
    a: (u32, usize, Option<u32>),
    b: (u32, usize, Option<u32>),
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (sp_a, idx_a, prox_a) = a;
    let (sp_b, idx_b, prox_b) = b;

    let origin_a = if is_author_origin(sp_a) { 1 } else { 0 };
    let origin_b = if is_author_origin(sp_b) { 1 } else { 0 };
    match origin_a.cmp(&origin_b) {
        Ordering::Equal => {}
        other => return other,
    }

    match rules[idx_a].layer_rank.cmp(&rules[idx_b].layer_rank) {
        Ordering::Equal => {}
        other => return other,
    }

    match sp_a.cmp(&sp_b) {
        Ordering::Equal => {}
        other => return other,
    }

    // CSS Cascade 6 §4.2: Scope Proximity
    // For scoped declarations with equal specificity, the declaration with
    // the shorter proximity from the scoping root to the scoped element wins.
    if let (Some(dist_a), Some(dist_b)) = (prox_a, prox_b) {
        match dist_b.cmp(&dist_a) {
            Ordering::Equal => {}
            other => return other,
        }
    }

    // Tie-break by stylesheet source order
    idx_a.cmp(&idx_b)
}

fn important_cascade_cmp(
    rules: &[CssRule],
    a: (u32, usize, Option<u32>),
    b: (u32, usize, Option<u32>),
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (sp_a, idx_a, prox_a) = a;
    let (sp_b, idx_b, prox_b) = b;

    let layer_a = rules[idx_a].layer_rank;
    let rev_layer_a = if layer_a == u32::MAX {
        0
    } else {
        u32::MAX - layer_a
    };
    let layer_b = rules[idx_b].layer_rank;
    let rev_layer_b = if layer_b == u32::MAX {
        0
    } else {
        u32::MAX - layer_b
    };
    match rev_layer_a.cmp(&rev_layer_b) {
        Ordering::Equal => {}
        other => return other,
    }

    match sp_a.cmp(&sp_b) {
        Ordering::Equal => {}
        other => return other,
    }

    if let (Some(dist_a), Some(dist_b)) = (prox_a, prox_b) {
        match dist_b.cmp(&dist_a) {
            Ordering::Equal => {}
            other => return other,
        }
    }

    idx_a.cmp(&idx_b)
}

fn clear_inherit_tracking_for_property(inherit_props: &mut HashSet<String>, prop: &str) {
    inherit_props.remove(prop);
    let id = properties::resolve(&prop.to_ascii_lowercase());
    clear_inherit_tracking_for_id(inherit_props, id);
}

fn clear_inherit_tracking_for_id(inherit_props: &mut HashSet<String>, id: properties::PropertyId) {
    let def = property_defs::get(id);
    inherit_props.remove(def.name);
    for &longhand in def.longhands {
        inherit_props.remove(property_defs::get(longhand).name);
    }
}

fn apply_css_value_with_cascade_context(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    val: &crate::types::CssValue,
    local_vars: &HashMap<String, String>,
    parent_style: Option<&ComputedStyle>,
    revert_base: Option<&ComputedStyle>,
    revert_layer_base: &ComputedStyle,
) {
    use crate::types::CssValue;
    let name = property_defs::get(id).name;
    match val {
        CssValue::Inherit => {
            if let Some(parent) = parent_style {
                copy_property_from_style(style, parent, name);
            }
        }
        CssValue::RevertLayer => copy_property_from_style(style, revert_layer_base, name),
        CssValue::Revert => {
            if let Some(base) = revert_base {
                copy_property_from_style(style, base, name);
            } else {
                apply_css_value(style, id, &CssValue::Initial);
            }
        }
        CssValue::Raw(s) => {
            let resolved = resolve_var_references(s, local_vars);
            if resolved.trim().is_empty() && s.contains("var(") {
                return;
            }
            apply_resolved_property_with_cascade_context(
                style,
                name,
                id,
                &resolved,
                parent_style,
                revert_base,
                revert_layer_base,
            );
        }
        _ => apply_css_value(style, id, val),
    }
}

fn apply_resolved_property_with_cascade_context(
    style: &mut ComputedStyle,
    prop: &str,
    id: properties::PropertyId,
    value: &str,
    parent_style: Option<&ComputedStyle>,
    revert_base: Option<&ComputedStyle>,
    revert_layer_base: &ComputedStyle,
) {
    let trimmed = value.trim();
    if trimmed == "inherit" {
        if let Some(parent) = parent_style {
            copy_property_from_style(style, parent, prop);
        }
    } else if trimmed == "revert-layer" {
        copy_property_from_style(style, revert_layer_base, prop);
    } else if trimmed == "revert" {
        if let Some(base) = revert_base {
            copy_property_from_style(style, base, prop);
        } else {
            apply_css_value(style, id, &crate::types::CssValue::Initial);
        }
    } else {
        apply_property_by_id_str(style, id, value);
    }
}

fn apply_state_matched_rules(
    state: &mut ComputedStyle,
    matched: &mut Vec<(u32, usize, Option<u32>)>,
    stylesheet: &Stylesheet,
    local_vars: &HashMap<String, String>,
    parent_style: Option<&ComputedStyle>,
    revert_base: Option<&ComputedStyle>,
) {
    matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    let mut current_layer: Option<(bool, u32)> = None;
    let mut layer_start_style = state.clone();
    for &(sp, ri, _) in matched.iter() {
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_layer != Some(layer_key) {
            current_layer = Some(layer_key);
            layer_start_style = state.clone();
        }
        for &(id, ref val) in &rule.compiled_decls {
            apply_css_value_with_cascade_context(
                state,
                id,
                val,
                local_vars,
                parent_style,
                revert_base,
                &layer_start_style,
            );
        }
    }

    matched.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
    for author_pass in [true, false] {
        let mut current_layer: Option<(bool, u32)> = None;
        let mut layer_start_style = state.clone();
        for &(sp, ri, _) in matched.iter() {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &stylesheet.rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_layer != Some(layer_key) {
                current_layer = Some(layer_key);
                layer_start_style = state.clone();
            }
            for &(id, ref val) in &rule.compiled_important {
                apply_css_value_with_cascade_context(
                    state,
                    id,
                    val,
                    local_vars,
                    parent_style,
                    revert_base,
                    &layer_start_style,
                );
            }
        }
    }
}

/// Apply a stylesheet to all boxes in the tree (cascade + inheritance).
pub fn apply_cascade(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
) {
    apply_cascade_vp(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        0.0,
        0.0,
        0,
        false,
    );
}

/// Apply a stylesheet with viewport size and focused element for media queries and :focus selectors.
///
/// `keyboard_focus` controls whether `:focus-visible` matches: pass `true` only when
/// focus was moved by keyboard (Tab/Shift+Tab), `false` for mouse-click focus.
///
/// **Note**: call `stylesheet.rebuild_index()` before this if rules were added since last cascade.
pub fn apply_cascade_vp(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
) {
    let empty_hover = std::collections::HashSet::new();
    apply_cascade_vp_hover(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        &empty_hover,
    );
}

/// Cascade with hover chain: elements in hover_chain will match :hover pseudo-class.
///
/// When the stylesheet has more than 1000 rules, automatically uses a parallel
/// selector-matching pass (via Rayon) to speed up large pages.
pub fn apply_cascade_vp_hover(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
) {
    apply_cascade_vp_hover_target(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        hover_chain,
        0,
    );
}

pub fn apply_cascade_vp_hover_target(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    target_id: u32,
) {
    apply_cascade_vp_hover_target_url(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        hover_chain,
        target_id,
        "",
    );
}

pub fn apply_cascade_vp_hover_target_url(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
) {
    // Use parallel cascade when the stylesheet is large enough to justify the overhead.
    if stylesheet.rules.len() > 1000 {
        apply_cascade_parallel(
            root,
            stylesheet,
            parent_style,
            root_font_px,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            hover_chain,
            target_id,
            document_url,
        );
        return;
    }
    // A single Vec is reused for the entire tree traversal (push/pop per node)
    // instead of cloning the ancestor list at every level — O(depth) allocations
    // instead of O(nodes × depth).
    let mut ancestors: Vec<AncestorInfo> = Vec::new();
    let mut candidates_buf: Vec<usize> = Vec::new();
    let mut counters: HashMap<String, Vec<i32>> = HashMap::new();
    let mut share_cache: ShareCache = HashMap::new();
    apply_cascade_inner(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        &mut ancestors,
        0,
        1,
        0,
        1,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        target_id,
        document_url,
        &stylesheet.variables,
        &mut candidates_buf,
        &mut counters,
        hover_chain,
        &[],
        &[],
        &[],
        &mut share_cache,
        None,
    );
}

/// Build a ComputedStyle for a ::before/::after pseudo-element.
/// Inherits inherited properties from `base` (the originating element's style),
/// resets non-inherited properties to CSS initial values, then applies matched declarations.
/// The generated text of a `content` declaration, or `None` when it names no
/// pseudo-element at all. `content: ""` returns `Some("")`.
fn pseudo_content_value(value: &str) -> Option<String> {
    let v = value.trim();
    if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("normal") {
        return None;
    }
    Some(value.to_string())
}

fn resolve_custom_counter_style_marker(
    stylesheet: &Stylesheet,
    name: &str,
    index: i32,
) -> Option<String> {
    resolve_custom_counter_style_marker_inner(stylesheet, name, index, &mut Vec::new())
}

fn resolve_custom_counter_style_marker_inner(
    stylesheet: &Stylesheet,
    name: &str,
    index: i32,
    resolving: &mut Vec<String>,
) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let rule = stylesheet
        .counter_styles
        .iter()
        .find(|rule| rule.name.eq_ignore_ascii_case(name))?;
    if resolving
        .iter()
        .any(|seen| seen.eq_ignore_ascii_case(&rule.name))
    {
        return None;
    }
    resolving.push(rule.name.clone());
    let system = counter_style_system(stylesheet, rule, resolving);
    let symbols = if system.starts_with("additive") {
        Vec::new()
    } else {
        let symbols =
            counter_style_symbols(counter_style_decl(stylesheet, rule, "symbols", resolving)?)?;
        if symbols.is_empty() {
            resolving.pop();
            return None;
        }
        symbols
    };
    let prefix = counter_style_decl(stylesheet, rule, "prefix", resolving)
        .map(|value| resolve_content_value_with_context(value, None, None))
        .unwrap_or_default();
    let suffix = counter_style_decl(stylesheet, rule, "suffix", resolving)
        .map(|value| resolve_content_value_with_context(value, None, None))
        .unwrap_or_else(|| ". ".to_string());
    let mut body = (if !counter_style_range_contains(
        counter_style_decl(stylesheet, rule, "range", resolving),
        index,
    ) {
        counter_style_fallback_body(stylesheet, rule, index, resolving)
    } else if system.starts_with("cyclic") {
        let idx = (index - 1).rem_euclid(symbols.len() as i32) as usize;
        Some(symbols[idx].clone())
    } else if system.starts_with("fixed") {
        let first = fixed_counter_first_value(&system);
        let offset = index - first;
        if offset < 0 || offset as usize >= symbols.len() {
            counter_style_fallback_body(stylesheet, rule, index, resolving)
        } else {
            Some(symbols[offset as usize].clone())
        }
    } else if system.starts_with("numeric") && symbols.len() >= 2 {
        Some(numeric_counter_symbols(index, &symbols))
    } else if system.starts_with("alphabetic") && symbols.len() >= 2 {
        Some(alphabetic_counter_symbols(index, &symbols))
    } else if system.starts_with("additive") {
        counter_style_additive_body(
            counter_style_decl(stylesheet, rule, "additive-symbols", resolving),
            index,
        )
    } else {
        let repeats = index.max(1) as usize;
        Some(symbols[0].repeat(repeats))
    })
    .unwrap_or_else(|| crate::css::format_counter_value(index, "decimal"));
    if let Some(padded) = counter_style_padded_body(
        counter_style_decl(stylesheet, rule, "pad", resolving),
        &body,
    ) {
        body = padded;
    }
    resolving.pop();
    Some(format!("{prefix}{body}{suffix}"))
}

fn counter_style_decl<'a>(
    stylesheet: &'a Stylesheet,
    rule: &'a crate::css::CounterStyleRule,
    key: &str,
    resolving: &[String],
) -> Option<&'a String> {
    if let Some(value) = rule.declarations.get(key) {
        return Some(value);
    }
    let system = rule.declarations.get("system")?.trim();
    let base = counter_style_extends_name(system)?;
    if resolving.iter().any(|seen| seen.eq_ignore_ascii_case(base)) {
        return None;
    }
    let base_rule = stylesheet
        .counter_styles
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(base))?;
    counter_style_decl(stylesheet, base_rule, key, resolving)
}

fn counter_style_system(
    stylesheet: &Stylesheet,
    rule: &crate::css::CounterStyleRule,
    resolving: &[String],
) -> String {
    let own = rule
        .declarations
        .get("system")
        .map(|value| value.trim())
        .unwrap_or("symbolic");
    let Some(base) = counter_style_extends_name(own) else {
        return own.to_ascii_lowercase();
    };
    if resolving.iter().any(|seen| seen.eq_ignore_ascii_case(base)) {
        return "symbolic".to_string();
    }
    stylesheet
        .counter_styles
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(base))
        .map(|base_rule| counter_style_system(stylesheet, base_rule, resolving))
        .unwrap_or_else(|| base.to_ascii_lowercase())
}

fn counter_style_extends_name(system: &str) -> Option<&str> {
    let mut parts = system.split_whitespace();
    if !parts.next()?.eq_ignore_ascii_case("extends") {
        return None;
    }
    parts.next().filter(|name| !name.is_empty())
}

fn counter_style_additive_body(additive: Option<&String>, index: i32) -> Option<String> {
    if index <= 0 {
        return None;
    }
    let mut remaining = index;
    let mut out = String::new();
    for part in additive?.split(',') {
        let part = part.trim();
        let split = part
            .char_indices()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))?;
        let weight = part[..split].trim().parse::<i32>().ok()?;
        if weight <= 0 {
            return None;
        }
        let symbol = resolve_content_value_with_context(part[split..].trim(), None, None);
        if symbol.is_empty() {
            return None;
        }
        while remaining >= weight {
            out.push_str(&symbol);
            remaining -= weight;
        }
    }
    (remaining == 0 && !out.is_empty()).then_some(out)
}

fn fixed_counter_first_value(system: &str) -> i32 {
    system
        .split_whitespace()
        .nth(1)
        .and_then(|part| part.parse::<i32>().ok())
        .unwrap_or(1)
}

fn counter_style_fallback_body(
    stylesheet: &Stylesheet,
    rule: &crate::css::CounterStyleRule,
    index: i32,
    resolving: &mut Vec<String>,
) -> Option<String> {
    let fallback = counter_style_decl(stylesheet, rule, "fallback", resolving)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or("decimal");
    if fallback.eq_ignore_ascii_case("decimal") {
        Some(crate::css::format_counter_value(index, "decimal"))
    } else if stylesheet
        .counter_styles
        .iter()
        .any(|candidate| candidate.name.eq_ignore_ascii_case(fallback))
    {
        resolve_custom_counter_style_marker_inner(stylesheet, fallback, index, resolving)
    } else {
        Some(crate::css::format_counter_value(index, fallback))
    }
}

fn counter_style_range_contains(range: Option<&String>, index: i32) -> bool {
    let Some(range) = range else {
        return true;
    };
    let range = range.trim();
    if range.eq_ignore_ascii_case("auto") || range.is_empty() {
        return true;
    }
    range.split(',').any(|pair| {
        let mut parts = pair.split_whitespace();
        let Some(start) = parts.next().and_then(counter_range_bound) else {
            return false;
        };
        let Some(end) = parts.next().and_then(counter_range_bound) else {
            return false;
        };
        index >= start && index <= end
    })
}

fn counter_range_bound(value: &str) -> Option<i32> {
    if value.eq_ignore_ascii_case("infinite") {
        Some(i32::MAX)
    } else if value.eq_ignore_ascii_case("-infinite") {
        Some(i32::MIN)
    } else {
        value.parse::<i32>().ok()
    }
}

fn counter_style_padded_body(pad: Option<&String>, body: &str) -> Option<String> {
    let pad = pad?.trim();
    let mut parts = pad.splitn(2, char::is_whitespace);
    let width = parts.next()?.parse::<usize>().ok()?;
    let symbol_src = parts.next()?.trim();
    let symbol = counter_style_symbols(symbol_src)
        .and_then(|mut symbols| symbols.pop())
        .filter(|symbol| !symbol.is_empty())
        .unwrap_or_else(|| resolve_content_value_with_context(symbol_src, None, None));
    let body_len = body.chars().count();
    if symbol.is_empty() || body_len >= width {
        return None;
    }
    Some(format!("{}{}", symbol.repeat(width - body_len), body))
}

fn counter_style_symbols(value: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'') else {
            break;
        };
        let mut escaped = false;
        let mut end_byte = None;
        for (idx, ch) in rest.char_indices().skip(1) {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                end_byte = Some(idx);
                break;
            }
        }
        let end = end_byte?;
        out.push(resolve_content_value_with_context(
            &rest[..=end],
            None,
            None,
        ));
        rest = &rest[end + quote.len_utf8()..];
    }
    Some(out)
}

fn numeric_counter_symbols(value: i32, symbols: &[String]) -> String {
    if value == 0 {
        return symbols[0].clone();
    }
    let negative = value < 0;
    let mut n = value.abs();
    let base = symbols.len() as i32;
    let mut parts = Vec::new();
    while n > 0 {
        parts.push(symbols[(n % base) as usize].clone());
        n /= base;
    }
    let mut out = parts.into_iter().rev().collect::<String>();
    if negative {
        out.insert(0, '-');
    }
    out
}

fn alphabetic_counter_symbols(mut value: i32, symbols: &[String]) -> String {
    if value <= 0 {
        return crate::css::format_counter_value(value, "decimal");
    }
    let base = symbols.len() as i32;
    let mut parts = Vec::new();
    while value > 0 {
        value -= 1;
        parts.push(symbols[(value % base) as usize].clone());
        value /= base;
    }
    parts.into_iter().rev().collect()
}

/// The last word on `display`, run once every declaration has been applied.
///
/// ⛔ COMPUTED-VALUE TIME, not declaration time (CSS Display 3 §2.7). A
/// blockification done inside `float`'s own applier depends on where `float`
/// sits among the declarations, so `float:left; display:inline` and
/// `display:inline; float:left` came out different — and the two cascade
/// implementations, which order matched rules differently, disagreed about the
/// same element on a re-cascade.
pub(crate) fn finalize_display(style: &mut ComputedStyle, tag: &str, has_explicit_display: bool) {
    crate::css::finalize_logical_float_clear(style);
    // A block-level element left Inline by nothing but the default takes Block.
    if matches!(style.display, Display::Inline) && !has_explicit_display {
        let should_be_block = matches!(
            tag,
            "anonymous-block"
                | "div"
                | "p"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "ul"
                | "ol"
                | "dl"
                | "dt"
                | "dd"
                | "pre"
                | "blockquote"
                | "hr"
                | "section"
                | "article"
                | "aside"
                | "nav"
                | "header"
                | "footer"
                | "main"
                | "address"
                | "figure"
                | "figcaption"
                | "details"
                | "center"
                | "form"
                | "fieldset"
                | "legend"
                | "hgroup"
                | "search"
        );
        if should_be_block {
            style.display = Display::Block;
        }
    }
    // Floated and absolutely positioned boxes are blockified.
    let out_of_flow = !matches!(style.float, Float::None)
        || matches!(style.position, Position::Absolute | Position::Fixed);
    if out_of_flow {
        blockify_out_of_flow(style);
    }
}

fn blockify_out_of_flow(style: &mut ComputedStyle) {
    style.display = match style.display {
        Display::Inline | Display::InlineBlock => Display::Block,
        Display::InlineFlex => Display::Flex,
        Display::InlineGrid => Display::Grid,
        Display::TableRow
        | Display::TableCell
        | Display::TableHeaderCell
        | Display::TableRowGroup
        | Display::TableHeaderGroup
        | Display::TableFooterGroup
        | Display::TableColumn
        | Display::TableColumnGroup
        | Display::TableCaption
        | Display::Ruby
        | Display::RubyText => Display::Block,
        other => other,
    };
}

fn blockify_flex_or_grid_item(style: &mut ComputedStyle) {
    style.display = match style.display {
        Display::Inline | Display::InlineBlock => Display::Block,
        Display::InlineFlex => Display::Flex,
        Display::InlineGrid => Display::Grid,
        Display::TableRow
        | Display::TableCell
        | Display::TableHeaderCell
        | Display::TableRowGroup
        | Display::TableHeaderGroup
        | Display::TableFooterGroup
        | Display::TableColumn
        | Display::TableColumnGroup
        | Display::TableCaption
        | Display::Ruby
        | Display::RubyText => Display::Block,
        other => other,
    };
}

pub(crate) fn build_pseudo_style_shared(
    matched: &mut Vec<(u32, usize, Option<u32>)>,
    base: &ComputedStyle,
    vars: &HashMap<String, String>,
    _attrs: &crate::dom::attrs::AttrMap,
    rules: &[CssRule],
) -> Option<(Option<String>, Box<ComputedStyle>)> {
    if matched.is_empty() {
        return None;
    }
    // CSS Cascade order for normal declarations is origin, then layer, then
    // specificity/scope proximity/source order. Keeping origin first prevents a UA unlayered
    // rule from beating a layered author rule.
    matched.sort_by(|&a, &b| normal_cascade_cmp(rules, a, b));
    let mut ps = ComputedStyle::default();
    ps.inherit_from(base);
    ps.relative_font_weight_base = Some(base.font_weight);
    if ps.href.is_empty() && !base.href.is_empty() {
        ps.href = base.href.clone();
    }
    if base.text_decoration.underline {
        ps.text_decoration.underline = true;
    }
    if ps.text_decoration_color.is_none() {
        ps.text_decoration_color = base.text_decoration_color;
    }
    if ps.text_decoration_thickness.is_auto() {
        ps.text_decoration_thickness = base.text_decoration_thickness.clone();
    }
    // **`content` decides whether the pseudo-element exists at all**
    // (css-pseudo-4 §2.1): `none` — which is what `normal` computes to here,
    // and what an absent declaration leaves — generates nothing. `""` is a
    // real, empty pseudo-element, so the two cannot collapse to one string.
    let mut content_value: Option<String> = None;
    let pseudo_revert_base = ps.clone();
    let mut current_normal_layer: Option<(bool, u32)> = None;
    let mut normal_layer_start_style = ps.clone();
    for &(sp, ri, _) in matched.iter() {
        let rule = &rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_normal_layer != Some(layer_key) {
            current_normal_layer = Some(layer_key);
            normal_layer_start_style = ps.clone();
        }
        for (prop, val) in &rule.declarations {
            let resolved = resolve_var_references(val, vars);
            if prop == "content" {
                content_value = pseudo_content_value(&resolved);
            } else {
                let id = properties::resolve(prop);
                apply_resolved_property_with_cascade_context(
                    &mut ps,
                    prop,
                    id,
                    &resolved,
                    Some(base),
                    Some(&pseudo_revert_base),
                    &normal_layer_start_style,
                );
            }
        }
    }
    // `!important` reverses the origin order (CSS Cascade §6.3): author first,
    // then UA, so a UA `!important` on a pseudo-element still wins.
    for author_pass in [true, false] {
        let mut important_matched = matched.clone();
        important_matched.sort_by(|&a, &b| important_cascade_cmp(rules, a, b));
        let mut current_important_layer: Option<(bool, u32)> = None;
        let mut important_layer_start_style = ps.clone();
        for &(sp, ri, _) in important_matched.iter() {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_important_layer != Some(layer_key) {
                current_important_layer = Some(layer_key);
                important_layer_start_style = ps.clone();
            }
            for (prop, val) in &rule.important_declarations {
                let resolved = resolve_var_references(val, vars);
                if prop == "content" {
                    content_value = pseudo_content_value(&resolved);
                } else {
                    let id = properties::resolve(prop);
                    apply_resolved_property_with_cascade_context(
                        &mut ps,
                        prop,
                        id,
                        &resolved,
                        Some(base),
                        Some(&pseudo_revert_base),
                        &important_layer_start_style,
                    );
                }
            }
        }
    }
    Some((content_value, Box::new(ps)))
}

/// Create or update the `::before` / `::after` child boxes.
///
/// ⛔ A FUNCTION, not the block it used to be. Its locals — two `WebCore`
/// pseudo-element boxes and their style clones — were living in
/// `apply_cascade_inner`'s frame ACROSS the recursive call, because a
/// debug build does not reuse stack slots between sibling scopes. Only a
/// real function boundary pops them (`arenaplan.md` item 3).
pub(crate) fn build_pseudo_element_boxes(root: &mut crate::types::WebCore) {
    let is_grid_or_flex = matches!(
        root.style.display,
        Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
    );
    let has_pseudo_containing_box = !matches!(root.style.display, Display::Inline);
    let before_is_positioned = root.style.before_style.as_ref().map_or(false, |ps| {
        matches!(ps.position, Position::Absolute | Position::Fixed)
    });
    let before_is_block = root
        .style
        .before_style
        .as_ref()
        .map_or(false, |ps| has_pseudo_containing_box && ps.is_block_level());
    // `before_style` is Some only when `content` generated the pseudo-element,
    // so it — not the generated TEXT, which is empty for `content: ""` — is
    // what says the box may exist.
    let before_generated = root.style.before_style.is_some();
    if before_generated && (is_grid_or_flex || before_is_positioned || before_is_block) {
        let existing = root.children.iter().position(|c| c.tag == "::before");
        let mut pseudo_box = crate::types::WebCore::new("::before");
        pseudo_box.text = root.style.before_content.clone();
        pseudo_box.tag = "::before".to_string();
        if let Some(ref ps) = root.style.before_style {
            pseudo_box.style = std::sync::Arc::new(*ps.clone());
        }
        if pseudo_box.style.is_positioned() {
            blockify_out_of_flow(std::sync::Arc::make_mut(&mut pseudo_box.style));
        }
        if is_grid_or_flex
            && !pseudo_box.style.is_positioned()
            && matches!(pseudo_box.style.display, Display::Inline)
        {
            std::sync::Arc::make_mut(&mut pseudo_box.style).display = Display::Block;
        }
        if let Some(idx) = existing {
            root.children[idx] = pseudo_box;
        } else {
            root.children.insert(0, pseudo_box);
        }
        std::sync::Arc::make_mut(&mut root.style).before_content = String::new();
    } else {
        if let Some(idx) = root.children.iter().position(|c| c.tag == "::before") {
            root.children.remove(idx);
        }
    }
    let after_is_positioned = root.style.after_style.as_ref().map_or(false, |ps| {
        matches!(ps.position, Position::Absolute | Position::Fixed)
    });
    let after_is_block = root
        .style
        .after_style
        .as_ref()
        .map_or(false, |ps| has_pseudo_containing_box && ps.is_block_level());
    let after_generated = root.style.after_style.is_some();
    if after_generated && (is_grid_or_flex || after_is_positioned || after_is_block) {
        let existing = root.children.iter().position(|c| c.tag == "::after");
        let mut pseudo_box = crate::types::WebCore::new("::after");
        pseudo_box.text = root.style.after_content.clone();
        pseudo_box.tag = "::after".to_string();
        if let Some(ref ps) = root.style.after_style {
            pseudo_box.style = std::sync::Arc::new(*ps.clone());
        }
        if pseudo_box.style.is_positioned() {
            blockify_out_of_flow(std::sync::Arc::make_mut(&mut pseudo_box.style));
        }
        if is_grid_or_flex
            && !pseudo_box.style.is_positioned()
            && matches!(pseudo_box.style.display, Display::Inline)
        {
            std::sync::Arc::make_mut(&mut pseudo_box.style).display = Display::Block;
        }
        if let Some(idx) = existing {
            root.children[idx] = pseudo_box;
        } else {
            root.children.push(pseudo_box);
        }
        std::sync::Arc::make_mut(&mut root.style).after_content = String::new();
    } else {
        if let Some(idx) = root.children.iter().position(|c| c.tag == "::after") {
            root.children.remove(idx);
        }
    }
}

/// A style-sharing cache for one cascade run, spanning the WHOLE document.
///
/// ⛔ The cache used to live inside `cascade_children`, so it only ever shared
/// between SIBLINGS — which is why the measured sharing on demo.html was 2.9%
/// while `arenaplan.md` quotes a 5-12x DOCUMENT-WIDE distinct-style ratio. The
/// two are different questions.
///
/// The key is `(parent style identity, tag, attributes)`. The parent's identity
/// is what makes a document-wide cache SOUND: two elements share only if their
/// parents already shared a style, which by induction means their whole
/// ancestor chains are selector-equivalent — so a descendant selector cannot
/// tell them apart. The base case is the root, whose style is unique.
///
/// Item 1 is what made this cheap: a parent style is an `Arc` now, so its
/// identity is a pointer rather than a deep comparison.
pub(crate) type ShareCache = HashMap<(usize, String, String), std::sync::Arc<ComputedStyle>>;

/// Every rule that matched one element, bucketed by what it styles.
///
/// ⛔ ONE shape for both cascades. The parallel pass computes these off-thread
/// and hands them to `apply_cascade_inner`, which otherwise computes them
/// itself — so there is a single matcher, a single set of buckets, and no way
/// for the two paths to disagree about which rules apply to an element.
#[derive(Clone, Default)]
pub(crate) struct MatchSets {
    pub matched: Vec<(u32, usize, Option<u32>)>,
    pub hover_matched: Vec<(u32, usize, Option<u32>)>,
    pub active_matched: Vec<(u32, usize, Option<u32>)>,
    pub visited_matched: Vec<(u32, usize, Option<u32>)>,
    pub before_matched: Vec<(u32, usize, Option<u32>)>,
    pub after_matched: Vec<(u32, usize, Option<u32>)>,
    pub selection_matched: Vec<(u32, usize, Option<u32>)>,
    pub placeholder_matched: Vec<(u32, usize, Option<u32>)>,
    pub marker_matched: Vec<(u32, usize, Option<u32>)>,
    pub backdrop_matched: Vec<(u32, usize, Option<u32>)>,
}

/// Precomputed match results, keyed by `node_id`.
///
/// ⛔ `node_id`, not a path through `children`. A path is invalidated the moment
/// `build_pseudo_element_boxes` inserts a `::before` at index 0 during the apply
/// walk: every later sibling then reads its neighbour's rules, and the last one
/// reads none at all. `node_id` is stable across that insertion.
pub(crate) type MatchMap = HashMap<u32, MatchSets>;

/// Run the selectors of `stylesheet` against one element.
///
/// The only place a selector is tested during a cascade. `candidates_buf` is a
/// scratch Vec the caller owns so the walk allocates once, not once per node.
pub(crate) fn match_rules(
    node: &crate::types::WebCore,
    stylesheet: &Stylesheet,
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
    prev_siblings: &[(String, String, String)],
    next_siblings: &[(String, String, String)],
    next_sibling_nodes: &[&crate::types::WebCore],
    candidates_buf: &mut Vec<usize>,
) -> MatchSets {
    // ⛔ `html_box` is the element itself, always. `:has()`, `:empty`, `:focus`,
    // `:focus-within`, `:modal`, `:popover-open`, `:checked`, `:indeterminate`
    // and `:placeholder-shown` all read the BOX; without one they answer false
    // or fall back to the content attribute, which is a different page.
    let match_ctx = MatchContext {
        focused_box,
        keyboard_focus,
        type_child_index,
        type_sibling_count,
        html_box: Some(node),
        hover_chain,
        element_id: node.node_id,
        scope_root_id: 0,
        target_id,
        document_url,
        prev_siblings,
        next_siblings,
        next_sibling_nodes,
    };

    let mut sets = MatchSets::default();

    // The selector index narrows the candidates instead of scanning every rule.
    let id = node.attributes.get("id").map(|s| s.as_str());
    let class_attr = node
        .attributes
        .get("class")
        .map(|s| s.as_str())
        .unwrap_or("");
    let classes: Vec<&str> = class_attr.split_whitespace().collect();
    stylesheet.candidate_rules(&node.tag, id, &classes, candidates_buf);

    for &rule_idx in candidates_buf.iter() {
        let rule = &stylesheet.rules[rule_idx];
        // Rules whose @media condition does not match the viewport are not in
        // the cascade at all.
        if !rule.media_condition.is_empty() && !evaluate_media(&rule.media_condition, vw, vh) {
            continue;
        }
        // Container rules need layout context — a post-layout pass applies them.
        if !rule.container_condition.is_empty() {
            continue;
        }
        let Some(scope_proximity) = rule_matches_scope(
            rule,
            node,
            ancestors,
            child_index,
            sibling_count,
            &match_ctx,
        ) else {
            continue;
        };
        for sel in &rule.selectors {
            // Per-selector state flags are precomputed; nothing is scanned here.
            let has_hover = sel.has_hover;
            let has_active = sel.has_active;
            let has_visited = sel.has_visited;

            if (has_hover || has_active || has_visited)
                && rule.pseudo_element == PseudoElement::None
            {
                if matches_selector_with_ancestors(
                    &sel.base_parts,
                    &node.tag,
                    &node.attributes,
                    child_index,
                    sibling_count,
                    ancestors,
                    &match_ctx,
                ) {
                    if has_hover {
                        sets.hover_matched
                            .push((rule.specificity, rule_idx, scope_proximity));
                    }
                    if has_active {
                        sets.active_matched
                            .push((rule.specificity, rule_idx, scope_proximity));
                    }
                    if has_visited {
                        sets.visited_matched
                            .push((rule.specificity, rule_idx, scope_proximity));
                    }
                    // With a hover chain live, the FULL selector is tested too:
                    // a `:hover` rule that matches now applies as a normal rule,
                    // so it can change layout (`display: block` on a menu).
                    if has_hover
                        && !hover_chain.is_empty()
                        && sel.matches_with_ancestors_ctx(
                            node,
                            child_index,
                            sibling_count,
                            ancestors,
                            &match_ctx,
                        )
                    {
                        sets.matched
                            .push((rule.specificity, rule_idx, scope_proximity));
                    }
                    break;
                }
                continue;
            }
            if sel.matches_with_ancestors_ctx(
                node,
                child_index,
                sibling_count,
                ancestors,
                &match_ctx,
            ) {
                match rule.pseudo_element {
                    PseudoElement::Before => {
                        sets.before_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::After => {
                        sets.after_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::Selection => {
                        sets.selection_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::Placeholder => {
                        sets.placeholder_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::Marker => {
                        sets.marker_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::Backdrop => {
                        sets.backdrop_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::None => {
                        sets.matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::Ignored => {}
                }
                break;
            }
        }
    }
    sets
}

fn element_matches_scope_selector(
    sel: &CssSelector,
    idx: usize,
    node: &WebCore,
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    match_ctx: &MatchContext<'_>,
) -> bool {
    if idx == ancestors.len() {
        sel.matches_with_ancestors_ctx(node, child_index, sibling_count, ancestors, match_ctx)
    } else {
        selector_matches_ancestor(sel, &ancestors[idx], &ancestors[..idx], match_ctx)
    }
}

fn limit_matches_between(
    limit_sel: &CssSelector,
    from_idx: usize,
    to_node: bool,
    node: &WebCore,
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    match_ctx: &MatchContext<'_>,
) -> bool {
    for i in (from_idx + 1)..ancestors.len() {
        if selector_matches_ancestor(limit_sel, &ancestors[i], &ancestors[..i], match_ctx) {
            return true;
        }
    }
    if to_node {
        if limit_sel.matches_with_ancestors_ctx(
            node,
            child_index,
            sibling_count,
            ancestors,
            match_ctx,
        ) {
            return true;
        }
    }
    false
}

fn rule_matches_scope(
    rule: &CssRule,
    node: &WebCore,
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    match_ctx: &MatchContext<'_>,
) -> Option<Option<u32>> {
    use crate::css::rule::ScopeFrame;

    let scopes: Vec<ScopeFrame> = if !rule.scopes.is_empty() {
        rule.scopes.clone()
    } else if rule.scope_selector.is_some() {
        vec![ScopeFrame {
            root: rule.scope_selector.clone(),
            limit: rule.scope_limit_selector.clone(),
        }]
    } else {
        return Some(None);
    };

    let n = scopes.len();
    let innermost = &scopes[n - 1];

    for idx in (0..=ancestors.len()).rev() {
        let root_matches = match &innermost.root {
            Some(sel) => element_matches_scope_selector(
                sel,
                idx,
                node,
                ancestors,
                child_index,
                sibling_count,
                match_ctx,
            ),
            None => true,
        };
        if !root_matches {
            continue;
        }

        if let Some(limit_sel) = &innermost.limit {
            if limit_matches_between(
                limit_sel,
                idx,
                true,
                node,
                ancestors,
                child_index,
                sibling_count,
                match_ctx,
            ) {
                continue;
            }
        }

        let mut curr_idx = idx;
        let mut outer_ok = true;
        for k in (0..n - 1).rev() {
            let outer_frame = &scopes[k];
            let mut found_outer = None;
            for parent_idx in (0..=curr_idx).rev() {
                let m = match &outer_frame.root {
                    Some(sel) => element_matches_scope_selector(
                        sel,
                        parent_idx,
                        node,
                        ancestors,
                        child_index,
                        sibling_count,
                        match_ctx,
                    ),
                    None => true,
                };
                if m {
                    if let Some(limit_sel) = &outer_frame.limit {
                        if limit_matches_between(
                            limit_sel,
                            parent_idx,
                            true,
                            node,
                            ancestors,
                            child_index,
                            sibling_count,
                            match_ctx,
                        ) {
                            continue;
                        }
                    }
                    found_outer = Some(parent_idx);
                    break;
                }
            }
            if let Some(p_idx) = found_outer {
                curr_idx = p_idx;
            } else {
                outer_ok = false;
                break;
            }
        }

        if outer_ok {
            let distance = (ancestors.len() - idx) as u32;
            return Some(Some(distance));
        }
    }

    None
}

fn selector_matches_ancestor(
    selector: &CssSelector,
    ancestor: &AncestorInfo,
    ancestors_above: &[AncestorInfo],
    match_ctx: &MatchContext<'_>,
) -> bool {
    let ancestor_ctx = MatchContext {
        focused_box: match_ctx.focused_box,
        keyboard_focus: match_ctx.keyboard_focus,
        type_child_index: ancestor.type_child_index,
        type_sibling_count: ancestor.type_sibling_count,
        html_box: None,
        hover_chain: match_ctx.hover_chain,
        element_id: ancestor.node_id,
        scope_root_id: match_ctx.scope_root_id,
        target_id: match_ctx.target_id,
        document_url: match_ctx.document_url,
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    matches_selector_with_ancestors(
        &selector.parts,
        &ancestor.tag,
        &ancestor.attributes,
        ancestor.child_index,
        ancestor.sibling_count,
        ancestors_above,
        &ancestor_ctx,
    )
}

pub(crate) fn apply_cascade_inner(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
    // Mutable: we push this element's info before recursing and pop after.
    // One Vec is reused for the entire tree — no per-node heap allocation.
    ancestors: &mut Vec<AncestorInfo>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    target_id: u32,
    document_url: &str,
    inherited_vars: &HashMap<String, String>,
    candidates_buf: &mut Vec<usize>,
    counters: &mut HashMap<String, Vec<i32>>,
    hover_chain: &std::collections::HashSet<u32>,
    prev_siblings: &[(String, String, String)],
    next_siblings: &[(String, String, String)],
    next_sibling_nodes: &[&crate::types::WebCore],
    share_cache: &mut ShareCache,
    // Selector matches computed off-thread by the parallel pass, keyed by
    // `node_id`. `None`, or a miss, means match inline — never "no rules".
    precomputed: Option<&MatchMap>,
) {
    // Guard against stack overflow on deeply nested DOMs.
    if ancestors.len() >= MAX_CASCADE_DEPTH {
        // Just inherit from parent and stop — the page may render slightly wrong
        // at extreme depth, but won't crash.
        if let Some(p) = parent_style {
            std::sync::Arc::make_mut(&mut root.style).inherit_from(p);
        }
        return;
    }

    // Text and comment nodes are not elements — they inherit from their
    // parent but must never match CSS selectors (including `*`).
    if !root.is_element() {
        let saved_display = root.style.display;
        if let Some(p) = parent_style {
            std::sync::Arc::make_mut(&mut root.style).inherit_from(p);
        }
        let display = if root.tag == "#text" {
            Display::Inline
        } else {
            saved_display
        };
        std::sync::Arc::make_mut(&mut root.style).display = display;
        return;
    }

    // Synthetic ::before/::after children already have their computed pseudo
    // style set from the originating element. Recascading them as normal
    // children would overwrite pseudo-specific font/color/content rules.
    if root.tag == "::before" || root.tag == "::after" {
        return;
    }

    // ⚠ A style-sharing stub stood here: four bindings feeding an EMPTY `if`,
    // whose own comment said the sharing "actually happens in cascade_children
    // where we have access to the sibling WebCore objects". It computed a class
    // string and a hover lookup on every element and did nothing with them.
    // Removed rather than annotated — a reader greps `class_attr` and lands on
    // machinery that never ran.

    // Start with default style and inherit from parent
    let mut style = ComputedStyle::default();
    if let Some(p) = parent_style {
        style.inherit_from(p);
        style.relative_font_weight_base = Some(p.font_weight);
    }
    // Selector matching — the SAME function the parallel pass runs, so a
    // precomputed result and an inline one can never disagree.
    let precomputed_here = precomputed
        .filter(|_| root.node_id != 0)
        .and_then(|m| m.get(&root.node_id));
    let sets = match precomputed_here {
        Some(sets) => sets.clone(),
        // ⛔ A miss MATCHES, it does not mean "no rules". An element the parallel
        // pass never saw — a box with no DOM node behind it, or a shadow subtree,
        // which is matched against its own scoped sheet — has to be cascaded, and
        // handing it an empty result renders it unstyled with nothing to show for it.
        None => match_rules(
            root,
            stylesheet,
            ancestors,
            child_index,
            sibling_count,
            type_child_index,
            type_sibling_count,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            hover_chain,
            target_id,
            document_url,
            prev_siblings,
            next_siblings,
            next_sibling_nodes,
            candidates_buf,
        ),
    };
    let MatchSets {
        mut matched,
        mut hover_matched,
        mut active_matched,
        mut visited_matched,
        mut before_matched,
        mut after_matched,
        mut selection_matched,
        mut placeholder_matched,
        mut marker_matched,
        mut backdrop_matched,
    } = sets;
    matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    // Build variable scope: inherited from parent + any --custom-properties from matched rules.
    // Only clone the map when new custom properties are actually defined — most elements
    // don't define any, so we avoid O(vars) cloning at every node.
    let has_new_vars = matched.iter().any(|(_, ri, _)| {
        stylesheet.rules[*ri]
            .declarations
            .keys()
            .any(|p| p.starts_with("--"))
            || stylesheet.rules[*ri]
                .important_declarations
                .keys()
                .any(|p| p.starts_with("--"))
    });
    // Also check inline style for custom properties — these must be available
    // during var() resolution of stylesheet rules on the same element.
    let inline_decls = root
        .attributes
        .get("style")
        .cloned()
        .map(|s| parse_declarations_important(&s));
    let has_inline_vars = inline_decls
        .as_ref()
        .map(|(n, _)| n.keys().any(|p| p.starts_with("--")))
        .unwrap_or(false);

    let local_vars_owned = if has_new_vars || has_inline_vars {
        let mut vars = inherited_vars.clone();
        for &(_, ri, _) in &matched {
            for (prop, val) in &stylesheet.rules[ri].declarations {
                if prop.starts_with("--") {
                    vars.insert(prop.clone(), val.clone());
                }
            }
            for (prop, val) in &stylesheet.rules[ri].important_declarations {
                if prop.starts_with("--") {
                    vars.insert(prop.clone(), val.clone());
                }
            }
        }
        // Inline custom properties override stylesheet ones (higher specificity)
        if let Some((ref n, _)) = inline_decls {
            for (prop, val) in n {
                if prop.starts_with("--") {
                    vars.insert(prop.clone(), val.clone());
                }
            }
        }
        pre_resolve_variables(&mut vars);
        Some(vars)
    } else {
        None
    };
    let local_vars: &HashMap<String, String> = local_vars_owned.as_ref().unwrap_or(inherited_vars);
    // Track properties whose highest-specificity declaration is `inherit`.
    // After all rules are applied, these properties are reset to the parent's value.
    let mut inherit_props: HashSet<String> = HashSet::new();
    let has_vars = !local_vars.is_empty();
    let mut pre_author_normal_style: Option<ComputedStyle> = None;
    let mut current_normal_layer: Option<(bool, u32)> = None;
    let mut normal_layer_start_style = style.clone();
    let mut hints_applied = false;
    for &(sp, ri, _) in &matched {
        if is_author_origin(sp) && pre_author_normal_style.is_none() {
            apply_presentational_hints(&mut style, root, ancestors);
            hints_applied = true;
            pre_author_normal_style = Some(style.clone());
        }
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_normal_layer != Some(layer_key) {
            current_normal_layer = Some(layer_key);
            normal_layer_start_style = style.clone();
        }
        let revert_base = if is_author_origin(sp) {
            pre_author_normal_style.as_ref()
        } else {
            None
        };
        let revert_layer_base = &normal_layer_start_style;
        // Fast path: use pre-compiled declarations (PropertyId dispatch, no string matching).
        // Only fall back to raw declarations when var() resolution is needed.
        if has_vars && rule.has_var_refs {
            // Slow path: var() references need string-based resolution
            for (prop, val) in &rule.declarations {
                if prop.starts_with("--") {
                    continue;
                }
                let resolved = resolve_var_references(val, &local_vars);
                if resolved.trim().is_empty() && val.contains("var(") {
                    continue;
                }
                let trimmed = resolved.trim();
                if trimmed == "inherit" {
                    inherit_props.insert(prop.to_string());
                } else if trimmed == "revert-layer" {
                    copy_property_from_style(&mut style, revert_layer_base, prop);
                } else if trimmed == "revert" {
                    if let Some(base) = revert_base {
                        copy_property_from_style(&mut style, base, prop);
                    } else {
                        apply_property(&mut style, prop, "initial");
                    }
                } else {
                    clear_inherit_tracking_for_property(&mut inherit_props, prop);
                    apply_property(&mut style, prop, &resolved);
                }
            }
        } else {
            // Fast path: no var() — use compiled declarations directly
            for &(id, ref val) in &rule.compiled_decls {
                if matches!(val, crate::types::CssValue::Inherit) {
                    let name = property_defs::get(id).name;
                    inherit_props.insert(name.to_string());
                } else if matches!(val, crate::types::CssValue::RevertLayer) {
                    let name = property_defs::get(id).name;
                    copy_property_from_style(&mut style, revert_layer_base, name);
                } else if matches!(val, crate::types::CssValue::Revert) {
                    let name = property_defs::get(id).name;
                    if let Some(base) = revert_base {
                        copy_property_from_style(&mut style, base, name);
                    } else {
                        apply_css_value(&mut style, id, &crate::types::CssValue::Initial);
                    }
                } else if let crate::types::CssValue::Raw(s) = val {
                    // Raw values may contain var() even when has_vars is false
                    // (the rule has var refs but no variables are defined in scope).
                    // Resolve var() with empty vars — triggers fallback values.
                    if s.contains("var(") {
                        let resolved = resolve_var_references(s, &local_vars);
                        if !resolved.trim().is_empty() {
                            let trimmed = resolved.trim();
                            let name = property_defs::get(id).name;
                            if trimmed == "inherit" {
                                inherit_props.insert(name.to_string());
                            } else if trimmed == "revert-layer" {
                                copy_property_from_style(&mut style, revert_layer_base, name);
                            } else if trimmed == "revert" {
                                if let Some(base) = revert_base {
                                    copy_property_from_style(&mut style, base, name);
                                } else {
                                    apply_css_value(
                                        &mut style,
                                        id,
                                        &crate::types::CssValue::Initial,
                                    );
                                }
                            } else {
                                clear_inherit_tracking_for_id(&mut inherit_props, id);
                                apply_property_by_id_str(&mut style, id, &resolved);
                            }
                        }
                    } else {
                        let trimmed = s.trim();
                        if trimmed == "inherit" {
                            inherit_props.insert(property_defs::get(id).name.to_string());
                        } else if trimmed == "revert-layer" {
                            let name = property_defs::get(id).name;
                            copy_property_from_style(&mut style, revert_layer_base, name);
                        } else if trimmed == "revert" {
                            let name = property_defs::get(id).name;
                            if let Some(base) = revert_base {
                                copy_property_from_style(&mut style, base, name);
                            } else {
                                apply_css_value(&mut style, id, &crate::types::CssValue::Initial);
                            }
                        } else {
                            clear_inherit_tracking_for_id(&mut inherit_props, id);
                            apply_css_value(&mut style, id, val);
                        }
                    }
                } else {
                    clear_inherit_tracking_for_id(&mut inherit_props, id);
                    apply_css_value(&mut style, id, val);
                }
            }
        }
    }
    if !hints_applied {
        apply_presentational_hints(&mut style, root, ancestors);
    }

    apply_form_sizing_hints_after_ua(&mut style, root, &stylesheet.rules, &matched);

    // Second pass: `!important`, in CSS Cascade §6.3 order — which REVERSES the
    // origin ranking. A UA `!important` beats an author `!important`, so the UA
    // rules are applied LAST here even though they were applied first above.
    // `matched` is sorted by boosted specificity, so filtering on origin keeps
    // specificity order within each pass.
    //
    // Applying `matched` in one sweep let a page write
    // `input[type=hidden] { display: block !important }` and reveal a hidden
    // field — Chrome answers `display: none` there, and now so does this.
    let mut important_matched = matched.clone();
    important_matched.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
    for author_pass in [true, false] {
        let mut current_important_layer: Option<(bool, u32)> = None;
        let mut important_layer_start_style = style.clone();
        for &(sp, ri, _) in &important_matched {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &stylesheet.rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_important_layer != Some(layer_key) {
                current_important_layer = Some(layer_key);
                important_layer_start_style = style.clone();
            }
            let revert_base = if is_author_origin(sp) {
                pre_author_normal_style.as_ref()
            } else {
                None
            };
            if has_vars && rule.has_var_refs {
                for (prop, val) in &rule.important_declarations {
                    if prop.starts_with("--") {
                        continue;
                    }
                    let resolved = resolve_var_references(val, &local_vars);
                    if resolved.trim().is_empty() && val.contains("var(") {
                        continue;
                    }
                    let id = properties::resolve(prop);
                    apply_resolved_property_with_cascade_context(
                        &mut style,
                        prop,
                        id,
                        &resolved,
                        parent_style,
                        revert_base,
                        &important_layer_start_style,
                    );
                }
            } else {
                for &(id, ref val) in &rule.compiled_important {
                    apply_css_value_with_cascade_context(
                        &mut style,
                        id,
                        val,
                        &local_vars,
                        parent_style,
                        revert_base,
                        &important_layer_start_style,
                    );
                }
            }
        }
    }
    // Re-apply parent values for properties whose winning declaration was `inherit`.
    if !inherit_props.is_empty() {
        if let Some(p) = parent_style {
            for prop in &inherit_props {
                copy_property_from_parent(&mut style, p, prop);
            }
        }
    }
    // Hover style — clone the base style and overlay all matched hover declarations.
    if !hover_matched.is_empty() {
        let mut hs = style.clone();
        apply_state_matched_rules(
            &mut hs,
            &mut hover_matched,
            stylesheet,
            &local_vars,
            parent_style,
            pre_author_normal_style.as_ref(),
        );
        // Prevent infinite nesting: state styles don't carry their own state overrides.
        hs.hover_style = None;
        hs.active_style = None;
        hs.visited_style = None;
        style.hover_style = Some(Box::new(hs));
    }
    // Active style — clone the base style and overlay all matched active declarations.
    if !active_matched.is_empty() {
        let mut as_ = style.clone();
        apply_state_matched_rules(
            &mut as_,
            &mut active_matched,
            stylesheet,
            &local_vars,
            parent_style,
            pre_author_normal_style.as_ref(),
        );
        as_.hover_style = None;
        as_.active_style = None;
        as_.visited_style = None;
        style.active_style = Some(Box::new(as_));
    }
    // Visited style — clone the base style and overlay all matched visited declarations.
    if !visited_matched.is_empty() {
        let mut vs = style.clone();
        apply_state_matched_rules(
            &mut vs,
            &mut visited_matched,
            stylesheet,
            &local_vars,
            parent_style,
            pre_author_normal_style.as_ref(),
        );
        vs.hover_style = None;
        vs.active_style = None;
        vs.visited_style = None;
        style.visited_style = Some(Box::new(vs));
    }

    // Apply inline style attribute (normal declarations).
    // Custom properties were already merged into local_vars above.
    // Also collect inline hover-* properties for building hover_style.
    let mut inline_hover_props: Vec<(String, String)> = Vec::new();
    let (_inline_normal, inline_important) = if let Some((n, i)) = inline_decls {
        for (prop, val) in &n {
            if prop.starts_with("--") {
                continue;
            }
            // Inline hover-* properties: hover-background-color → background-color on hover
            if let Some(real_prop) = prop.strip_prefix("hover-") {
                let resolved = resolve_var_references(val, local_vars);
                inline_hover_props.push((real_prop.to_string(), resolved));
                continue;
            }
            let resolved = resolve_var_references(val, local_vars);
            if resolved.trim() == "inherit" {
                if let Some(p) = parent_style {
                    copy_property_from_parent(&mut style, p, prop);
                }
            } else if matches!(resolved.trim(), "revert" | "revert-layer") {
                if let Some(base) = &pre_author_normal_style {
                    copy_property_from_style(&mut style, base, prop);
                } else {
                    apply_property(&mut style, prop, "initial");
                }
            } else {
                apply_property(&mut style, prop, &resolved);
            }
        }
        (n, i)
    } else {
        (Declarations::new(), Declarations::new())
    };
    // Merge inline hover-* properties into hover_style
    if !inline_hover_props.is_empty() {
        let mut hs = if let Some(existing) = style.hover_style.take() {
            *existing
        } else {
            style.clone()
        };
        for (prop, val) in &inline_hover_props {
            apply_property(&mut hs, prop, val);
        }
        hs.hover_style = None;
        hs.active_style = None;
        hs.visited_style = None;
        style.hover_style = Some(Box::new(hs));
    }

    // `!important`, in CSS Cascade §6.4.1 order — bottom to top:
    //   author sheet important → inline important → UA important.
    // The style attribute is author origin, so it outranks author RULES but
    // still loses to the UA sheet's `!important`; the UA pass therefore comes
    // last, not first.
    let mut current_author_important_layer: Option<u32> = None;
    let mut author_important_layer_start_style = style.clone();
    for &(sp, ri, _) in &important_matched {
        if !is_author_origin(sp) {
            continue;
        }
        let rule = &stylesheet.rules[ri];
        if current_author_important_layer != Some(rule.layer_rank) {
            current_author_important_layer = Some(rule.layer_rank);
            author_important_layer_start_style = style.clone();
        }
        let revert_base = pre_author_normal_style.as_ref();
        for &(id, ref val) in &stylesheet.rules[ri].compiled_important {
            apply_css_value_with_cascade_context(
                &mut style,
                id,
                val,
                &local_vars,
                parent_style,
                revert_base,
                &author_important_layer_start_style,
            );
        }
    }
    let inline_important_start_style = style.clone();
    for (prop, val) in &inline_important {
        let resolved = resolve_var_references(val, &local_vars);
        let id = properties::resolve(prop);
        apply_resolved_property_with_cascade_context(
            &mut style,
            prop,
            id,
            &resolved,
            parent_style,
            pre_author_normal_style.as_ref(),
            &inline_important_start_style,
        );
    }
    let mut current_ua_important_layer: Option<u32> = None;
    let mut ua_important_layer_start_style = style.clone();
    for &(sp, ri, _) in &important_matched {
        if is_author_origin(sp) {
            continue;
        }
        let rule = &stylesheet.rules[ri];
        if current_ua_important_layer != Some(rule.layer_rank) {
            current_ua_important_layer = Some(rule.layer_rank);
            ua_important_layer_start_style = style.clone();
        }
        for &(id, ref val) in &stylesheet.rules[ri].compiled_important {
            apply_css_value_with_cascade_context(
                &mut style,
                id,
                val,
                &local_vars,
                parent_style,
                None,
                &ua_important_layer_start_style,
            );
        }
    }

    // Capture href from attributes (non-standard CSS, but useful for our editor)
    if let Some(href) = root.attributes.get("href") {
        style.href = href.clone();
    }

    // Resolve relative font size to absolute Px for inheritance parity
    let parent_font_px = parent_style
        .map(|p| p.font_size_px(root_font_px, root_font_px))
        .unwrap_or(root_font_px);
    let font_px = style.font_size_px(parent_font_px, root_font_px);
    style.font_size = CssLength::Px(font_px);

    // If this is the root element (<html>), its computed font-size becomes the
    // new root font-size used for `rem` resolution in all descendants.
    // e.g. `html { font-size: 62.5% }` → 1rem = 10px instead of 16px.
    let root_font_px = if root.tag.eq_ignore_ascii_case("html") {
        font_px
    } else {
        root_font_px
    };

    // Preserve list_index: set by the HTML parser (ol counter), not by CSS.
    // The fresh ComputedStyle defaults list_index=0, so carry the old value forward.
    style.list_index = root.style.list_index;
    // Preserve the resolved custom-property scope on computed style. Paint-time
    // consumers such as inline SVG need these inherited variables after cascade.
    style.custom_props = local_vars.clone();
    let has_explicit_display = matched.iter().any(|&(_, ri, _)| {
        stylesheet.rules[ri]
            .declarations
            .iter()
            .any(|(k, _)| k == "display")
    });
    finalize_display(&mut style, &root.tag, has_explicit_display);
    if parent_style.is_some_and(|parent| {
        matches!(
            parent.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        )
    }) {
        blockify_flex_or_grid_item(&mut style);
    }
    // `currentColor` resolves against this element's own `color`, which is only
    // final now — css-color-4 §6.2.
    crate::css::finalize_current_color(&mut style);
    // Flow-relative box properties map onto physical sides using the FINAL
    // `direction` and `writing-mode` — css-logical-1 §4.
    crate::css::finalize_logical(&mut style);
    // ⛔ MOVED, not cloned. This was a full 2.3 KB `ComputedStyle` copy per
    // element — and it kept the local alive across the recursive call below,
    // where it is the single biggest thing in the stack frame.
    //
    // ⛔ NOT interned. Giving byte-identical styles the same `Arc` was built
    // and MEASURED: it took demo.html from 1,099 distinct styles to 979 (11%)
    // and more than doubled cascade+layout time, 2.46 s to 5.80 s. The
    // `arenaplan.md` 5-12x figure is a measurement artifact — that plan's own
    // footnote says the serializer it counted with "emits a subset of
    // properties, so styles differing in an unserialized property collide".
    // Compared losslessly the styles on a real page are nearly all distinct.
    root.style = std::sync::Arc::new(style);
    // Store matched CSS rules for inspector (only when enabled).
    if stylesheet.inspect_mode {
        root.matched_rules.clear();
        for &(sp, ri, _) in &matched {
            let rule = &stylesheet.rules[ri];
            root.matched_rules.push(crate::types::MatchedRule {
                selector: rule.original_selector.clone(),
                declarations: rule
                    .declarations
                    .iter()
                    .map(|(k, v)| (k.clone(), resolve_var_references(v, &local_vars)))
                    .collect(),
                specificity: sp,
                source: if ri < 50 {
                    "ua".to_string()
                } else {
                    rule.media_condition.clone()
                },
                layer: rule.layer.clone(),
                layer_rank: rule.layer_rank,
            });
        }
    }
    // Mark dirty so the layout subtree pruning (in layout_box_with_fc) knows to
    // re-layout this element.  Cleared by the individual layout algorithms after
    // they have computed the final geometry.
    root.layout.layout_dirty = true;

    // <form> inside table elements: browsers treat it as transparent (display:contents)
    // so it doesn't break table row grouping. Check if any ancestor is a table element.
    if root.tag == "form" {
        let in_table = ancestors
            .iter()
            .any(|a| matches!(a.tag.as_str(), "table" | "thead" | "tbody" | "tfoot" | "tr"));
        if in_table {
            std::sync::Arc::make_mut(&mut root.style).display = Display::Contents;
        }
    }

    // Build full ComputedStyle for ::before / ::after pseudo-elements.
    // Each inherits from the element's computed style, then has its own declarations applied.
    // ── CSS counters: reset, increment, then resolve counter() in content ──
    // Track which counters were reset at this level so we can pop them later.
    let mut counters_pushed: Vec<String> = Vec::new();
    fn increment_counter(
        counters: &mut HashMap<String, Vec<i32>>,
        counters_pushed: &mut Vec<String>,
        name: &str,
        delta: i32,
    ) {
        let stack = counters.entry(name.to_string()).or_insert_with(|| {
            counters_pushed.push(name.to_string());
            vec![0]
        });
        if let Some(top) = stack.last_mut() {
            *top += delta;
        }
    }
    fn set_counter(
        counters: &mut HashMap<String, Vec<i32>>,
        counters_pushed: &mut Vec<String>,
        name: &str,
        value: i32,
    ) {
        let stack = counters.entry(name.to_string()).or_insert_with(|| {
            counters_pushed.push(name.to_string());
            vec![0]
        });
        if let Some(top) = stack.last_mut() {
            *top = value;
        }
    }
    for (name, val) in &root.style.counter_reset {
        counters
            .entry(name.clone())
            .or_insert_with(Vec::new)
            .push(*val);
        counters_pushed.push(name.clone());
    }
    // `ol` implicitly resets the `list-item` counter
    if root.tag == "ol" && root.style.counter_reset.is_empty() {
        counters
            .entry("list-item".to_string())
            .or_insert_with(Vec::new)
            .push(0);
        counters_pushed.push("list-item".to_string());
    }
    for (name, val) in &root.style.counter_increment {
        increment_counter(counters, &mut counters_pushed, name, *val);
    }
    for (name, val) in &root.style.counter_set {
        set_counter(counters, &mut counters_pushed, name, *val);
    }
    // `li` implicitly increments the `list-item` counter
    if root.tag == "li" && root.style.counter_increment.is_empty() {
        increment_counter(counters, &mut counters_pushed, "list-item", 1);
    }
    if root.tag == "li" {
        if let Some(value) = counters
            .get("list-item")
            .and_then(|stack| stack.last())
            .copied()
        {
            std::sync::Arc::make_mut(&mut root.style).list_index = value;
        }
        if let Some(marker) = resolve_custom_counter_style_marker(
            stylesheet,
            &root.style.custom_list_style_type,
            root.style.list_index,
        ) {
            std::sync::Arc::make_mut(&mut root.style).marker_content = marker;
        }
    }

    if let Some((Some(txt), ps)) = build_pseudo_style_shared(
        &mut before_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        // ::before may carry counter-increment/counter-reset — apply before resolving content
        for (name, val) in &ps.counter_reset {
            counters
                .entry(name.clone())
                .or_insert_with(Vec::new)
                .push(*val);
            counters_pushed.push(name.clone());
        }
        for (name, val) in &ps.counter_increment {
            increment_counter(counters, &mut counters_pushed, name, *val);
        }
        for (name, val) in &ps.counter_set {
            set_counter(counters, &mut counters_pushed, name, *val);
        }
        let resolved_content = resolve_content_value_with_context(
            &txt,
            Some(&root.attributes),
            Some(&root.style.rare().quotes),
        );
        std::sync::Arc::make_mut(&mut root.style).before_content =
            resolve_counters_in_content(&resolved_content, counters);
        std::sync::Arc::make_mut(&mut root.style).before_style = Some(ps);
    }
    if let Some((Some(txt), ps)) = build_pseudo_style_shared(
        &mut after_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        for (name, val) in &ps.counter_reset {
            counters
                .entry(name.clone())
                .or_insert_with(Vec::new)
                .push(*val);
            counters_pushed.push(name.clone());
        }
        for (name, val) in &ps.counter_increment {
            increment_counter(counters, &mut counters_pushed, name, *val);
        }
        for (name, val) in &ps.counter_set {
            set_counter(counters, &mut counters_pushed, name, *val);
        }
        let resolved_content = resolve_content_value_with_context(
            &txt,
            Some(&root.attributes),
            Some(&root.style.rare().quotes),
        );
        std::sync::Arc::make_mut(&mut root.style).after_content =
            resolve_counters_in_content(&resolved_content, counters);
        std::sync::Arc::make_mut(&mut root.style).after_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut selection_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).selection_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut placeholder_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).placeholder_style = Some(ps);
    }
    if let Some((txt, ps)) = build_pseudo_style_shared(
        &mut marker_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        if let Some(txt) = txt {
            let resolved_content = resolve_content_value_with_context(
                &txt,
                Some(&root.attributes),
                Some(&root.style.rare().quotes),
            );
            std::sync::Arc::make_mut(&mut root.style).marker_content =
                resolve_counters_in_content(&resolved_content, counters);
        }
        std::sync::Arc::make_mut(&mut root.style).marker_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut backdrop_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).backdrop_style = Some(ps);
    }

    {
        let authored_content = root.style.rare().content.clone();
        if !authored_content.is_empty() {
            let resolved = resolve_content_value_with_context(
                &authored_content,
                Some(&root.attributes),
                Some(&root.style.rare().quotes),
            );
            std::sync::Arc::make_mut(&mut root.style).rare_mut().content =
                resolve_counters_in_content(&resolved, counters);
        }
    }

    build_pseudo_element_boxes(root);

    ancestors.push(AncestorInfo {
        tag: root.tag.clone(),
        attributes: root.attributes.clone(),
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        node_id: root.node_id,
    });

    // Helper: cascade a list of children with a given stylesheet
    fn cascade_children(
        children: &mut [crate::types::WebCore],
        stylesheet: &Stylesheet,
        // ⛔ The `Arc`, not a `&ComputedStyle`: the share key needs the
        // parent's IDENTITY, and that is the pointer.
        parent_style: &std::sync::Arc<ComputedStyle>,
        root_font_px: f32,
        ancestors: &mut Vec<AncestorInfo>,
        vw: f32,
        vh: f32,
        focused_box: u32,
        keyboard_focus: bool,
        target_id: u32,
        document_url: &str,
        inherited_vars: &HashMap<String, String>,
        candidates_buf: &mut Vec<usize>,
        counters: &mut HashMap<String, Vec<i32>>,
        hover_chain: &std::collections::HashSet<u32>,
        share_cache: &mut ShareCache,
        precomputed: Option<&MatchMap>,
    ) {
        let n_children = children.len();
        if n_children == 0 {
            return;
        }
        let child_tags: Vec<String> = children
            .iter()
            .map(|c| c.tag.to_ascii_lowercase())
            .collect();
        let mut type_running: HashMap<&str, usize> = HashMap::new();
        let type_counts: Vec<usize> = child_tags
            .iter()
            .map(|tag| {
                let slot = type_running.entry(tag.as_str()).or_insert(0);
                let idx = *slot;
                *slot += 1;
                idx
            })
            .collect();
        let type_totals: Vec<usize> = child_tags
            .iter()
            .map(|tag| *type_running.get(tag.as_str()).unwrap_or(&0))
            .collect();
        let n_elem_children = children.iter().filter(|c| c.is_element()).count();
        let mut elem_pos = 0usize;
        let elem_indices: Vec<usize> = children
            .iter()
            .map(|c| {
                if !c.is_element() {
                    0
                } else {
                    let p = elem_pos;
                    elem_pos += 1;
                    p
                }
            })
            .collect();
        // One sibling row feeds both left-looking combinators and
        // right-looking `:nth-last-child(... of S)`.
        let sibling_records: Vec<(String, String, String)> = children
            .iter()
            .filter(|c| c.is_element())
            .map(|c| {
                (
                    c.tag.clone(),
                    c.attributes.get("id").cloned().unwrap_or_default(),
                    c.attributes.get("class").cloned().unwrap_or_default(),
                )
            })
            .collect();
        // ⛔ The cache is the caller's now, spanning the whole document —
        // see `ShareCache`. A per-parent one could only ever share between
        // siblings, which measured 2.9% on demo.html.
        let parent_id = std::sync::Arc::as_ptr(parent_style) as usize;
        for i in 0..n_children {
            let (_before, rest) = children.split_at_mut(i);
            let (child, after) = rest.split_first_mut().unwrap();
            let after_nodes: Vec<&crate::types::WebCore> = after.iter().collect();
            if matches!(child.tag.as_str(), "::before" | "::after") {
                if matches!(
                    parent_style.display,
                    Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
                ) {
                    blockify_flex_or_grid_item(std::sync::Arc::make_mut(&mut child.style));
                }
                child.layout.layout_dirty = true;
                continue;
            }

            let (ci, ns) = if !child.is_element() {
                (i, n_children)
            } else {
                (elem_indices[i], n_elem_children)
            };
            let sibling_pos = elem_indices[i];
            let (prev_for_child, next_for_child) = if child.is_element() {
                (
                    &sibling_records[..sibling_pos],
                    &sibling_records[sibling_pos.saturating_add(1)..],
                )
            } else {
                (&[][..], &[][..])
            };

            // Try style sharing before full cascade.
            //
            // ⛔ The key is the element's WHOLE attribute list, not just its
            // class. `i[data-x] { … }` was silently dropped: the two `<i>`
            // hashed the same and the second took the first one's style.
            // Confirmed to be sharing, not missing attribute-selector support,
            // by running the fixture with `can_share` forced false.
            //
            // The attributes ARE the element as far as a selector is concerned
            // — anything else is a hole waiting for the next selector form.
            // ⛔ …and the element's BOX STATE, which no attribute records.
            // `:modal`, `:checked`, `:focus`, `:indeterminate` and `:in-range`
            // all match on it, so two elements with the same tag and the same
            // attributes are still not interchangeable. Two `<dialog open>`s,
            // one `show()`n and one `showModal()`ed, hashed the same and the
            // modal took the plain one's `position`.
            let child_class = {
                let mut parts: Vec<String> = child
                    .attributes
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                parts.sort();
                parts.push(child.selector_state_key(focused_box));
                parts.join("\u{1}")
            };
            let can_share = child.is_element()
                && child.tag != "::before" && child.tag != "::after"
                && !child.attributes.contains_key("id")
                && !child.attributes.contains_key("style")
                && !hover_chain.contains(&child.node_id)
                // ⛔ The share key is `(tag, class)` and says nothing about
                // sibling POSITION. With `i + i { … }` or `li:nth-child(2)`
                // in the sheet, two same-key siblings are NOT interchangeable
                // — the second was handed the first one's style and the rule
                // vanished. Verified both ways: the test goes green with
                // sharing off and red with it on.
                && !stylesheet.has_sibling_sensitive_rules
                && child.children.is_empty(); // only for leaf elements (no pseudo-elements to worry about)
            let share_key = (parent_id, child.tag.clone(), child_class.clone());

            if can_share {
                if let Some(cached) = share_cache.get(&share_key) {
                    // ⛔ THE point of item 1: a shared style is a refcount
                    // bump, not a 2.3 KB memcpy. `cached` is already an `Arc`.
                    child.style = cached.clone();
                    continue;
                }
            }

            apply_cascade_inner(
                child,
                stylesheet,
                Some(parent_style),
                root_font_px,
                ancestors,
                ci,
                ns,
                type_counts[i],
                type_totals[i],
                vw,
                vh,
                focused_box,
                keyboard_focus,
                target_id,
                document_url,
                inherited_vars,
                candidates_buf,
                counters,
                hover_chain,
                prev_for_child,
                next_for_child,
                &after_nodes,
                share_cache,
                precomputed,
            );
            if matches!(
                parent_style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            ) {
                blockify_flex_or_grid_item(std::sync::Arc::make_mut(&mut child.style));
            }
            // Cache style for sharing with future siblings
            if can_share && !share_cache.contains_key(&share_key) {
                share_cache.insert(share_key, child.style.clone());
            }
        }
    }

    // Shadow DOM: cascade shadow children with the shadow's scoped stylesheet,
    // and also cascade light DOM children with the document stylesheet.
    // CSS custom properties cross the shadow boundary via inherited_vars.
    // ⛔ The parent style for the children comes from the ARC now, not from a
    // 2.3 KB local held alive across the recursion. This is the frame shrink:
    // what crosses the recursive call is a pointer.
    let parent_for_children = root.style.clone();
    if root.shadow_root.is_some() {
        // Take shadow root temporarily to satisfy borrow checker
        let mut sr = root.shadow_root.take().unwrap();
        sr.stylesheet.rebuild_index();
        // `:host` — the shadow stylesheet styling its own host. The matcher
        // cannot answer it (it has no idea whose shadow tree a rule came from),
        // so it is applied HERE, where the host and its shadow stylesheet are
        // both in hand. `:host` rules were returning false unconditionally,
        // which made the single most common shadow-CSS rule inert.
        apply_host_rules(root, &sr.stylesheet, &local_vars, ancestors);
        // ⛔ `None`, not the map: pass 1 walks the LIGHT tree only, and the map
        // is keyed globally by `node_id`. A shadow child that happened to carry
        // an id from the light pass would be handed rules matched against the
        // DOCUMENT sheet instead of its own scoped one.
        cascade_children(
            &mut sr.children,
            &sr.stylesheet,
            &parent_for_children,
            root_font_px,
            ancestors,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            target_id,
            document_url,
            &local_vars,
            candidates_buf,
            counters,
            hover_chain,
            share_cache,
            None,
        );
        root.shadow_root = Some(sr);
        // Also cascade light DOM children (they need document styles for ::slotted)
        cascade_children(
            &mut root.children,
            stylesheet,
            &parent_for_children,
            root_font_px,
            ancestors,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            target_id,
            document_url,
            &local_vars,
            candidates_buf,
            counters,
            hover_chain,
            share_cache,
            precomputed,
        );
    } else {
        cascade_children(
            &mut root.children,
            stylesheet,
            &parent_for_children,
            root_font_px,
            ancestors,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            target_id,
            document_url,
            &local_vars,
            candidates_buf,
            counters,
            hover_chain,
            share_cache,
            precomputed,
        );
    }

    ancestors.pop();

    // Pop counters that were reset at this level
    for name in counters_pushed.iter().rev() {
        if let Some(stack) = counters.get_mut(name) {
            stack.pop();
            if stack.is_empty() {
                counters.remove(name);
            }
        }
    }
}

fn apply_form_sizing_hints_after_ua(
    style: &mut ComputedStyle,
    root: &crate::types::WebCore,
    rules: &[CssRule],
    matched: &[(u32, usize, Option<u32>)],
) {
    let author_declares = |property: &str| {
        matched.iter().any(|(sp, ri, _)| {
            is_author_origin(*sp) && rules[*ri].declarations.contains_key(property)
        })
    };

    match root.tag.as_str() {
        "input" => {
            if !author_declares("width") {
                if let Some(size) = root.attributes.get("size") {
                    if let Ok(chars) = size.trim().parse::<f32>() {
                        if chars > 0.0 {
                            apply_property(style, "width", &format!("{}em", chars * 0.6 + 0.5));
                        }
                    }
                }
            }
        }
        "textarea" => {
            if !author_declares("height") {
                if let Some(rows) = root.attributes.get("rows") {
                    if let Ok(rows) = rows.trim().parse::<f32>() {
                        if rows > 0.0 {
                            apply_property(style, "height", &format!("{}em", rows * 1.4));
                        }
                    }
                }
            }
            if !author_declares("width") {
                if let Some(cols) = root.attributes.get("cols") {
                    if let Ok(cols) = cols.trim().parse::<f32>() {
                        if cols > 0.0 {
                            apply_property(style, "width", &format!("{}em", cols * 0.6));
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn apply_presentational_hints(
    style: &mut ComputedStyle,
    root: &crate::types::WebCore,
    ancestors: &[AncestorInfo],
) {
    for (attr, val) in root.attributes.iter() {
        match attr.as_str() {
            "align" => match val.as_str() {
                "center" => apply_property(style, "text-align", "center"),
                "right" => apply_property(style, "text-align", "right"),
                "left" => apply_property(style, "text-align", "left"),
                _ => {}
            },
            "valign" => apply_property(style, "vertical-align", val),
            "multiple" if root.tag == "select" && !root.attributes.contains_key("size") => {
                apply_property(style, "height", &format!("{}em", 4.0 * 1.2 + 0.5));
            }
            "bgcolor" => apply_property(style, "background-color", val),
            "color" | "text" => apply_property(style, "color", val),
            "face" => apply_property(style, "font-family", val),
            "size" => match root.tag.as_str() {
                "font" => {
                    let px: f32 = match val.trim() {
                        "1" => 10.0,
                        "2" => 13.0,
                        "3" => 16.0,
                        "4" => 18.0,
                        "5" => 24.0,
                        "6" => 32.0,
                        "7" => 48.0,
                        v => v.parse::<f32>().unwrap_or(16.0),
                    };
                    apply_property(style, "font-size", &format!("{}px", px));
                }
                "select" => {
                    let rows = val.trim().parse::<f32>().unwrap_or(1.0).max(1.0);
                    let height = if rows > 1.0 { rows * 1.2 + 0.5 } else { 2.2 };
                    apply_property(style, "height", &format!("{height}em"));
                }
                "input" => {
                    if let Ok(chars) = val.trim().parse::<f32>() {
                        if chars > 0.0 {
                            apply_property(style, "width", &format!("{}ch", chars));
                        }
                    }
                }
                _ => {}
            },
            "rows" if root.tag == "textarea" => {
                if let Ok(rows) = val.trim().parse::<f32>() {
                    if rows > 0.0 {
                        apply_property(style, "height", &format!("{}em", rows * 1.4));
                    }
                }
            }
            "cols" if root.tag == "textarea" => {
                if let Ok(cols) = val.trim().parse::<f32>() {
                    if cols > 0.0 {
                        apply_property(style, "width", &format!("{}em", cols * 0.6));
                    }
                }
            }
            "width" => {
                let clean = val.trim().trim_end_matches(';').trim();
                if clean.ends_with('%') {
                    apply_property(style, "width", clean);
                } else {
                    let num = clean.strip_suffix("px").unwrap_or(clean).trim();
                    if let Ok(n) = num.parse::<f32>() {
                        apply_property(style, "width", &format!("{}px", n));
                    }
                }
            }
            "height" => {
                let clean = val.trim().trim_end_matches(';').trim();
                if clean.ends_with('%') {
                    apply_property(style, "height", clean);
                } else {
                    let num = clean.strip_suffix("px").unwrap_or(clean).trim();
                    if let Ok(n) = num.parse::<f32>() {
                        apply_property(style, "height", &format!("{}px", n));
                    }
                }
            }
            "border" if root.tag == "table" => {
                if let Ok(w) = val.parse::<f32>() {
                    if w > 0.0 {
                        apply_property(style, "border", &format!("{}px solid", w));
                        apply_property(style, "border-collapse", "collapse");
                    } else {
                        apply_property(style, "border", "0px solid transparent");
                    }
                }
            }
            "cellspacing" => {
                if let Ok(n) = val.parse::<f32>() {
                    apply_property(style, "border-spacing", &format!("{}px", n));
                } else if val.ends_with("px") {
                    apply_property(style, "border-spacing", val);
                }
            }
            "cellpadding" => apply_property(style, "cellpadding", val),
            "dir" => match val.to_ascii_lowercase().as_str() {
                "rtl" => apply_property(style, "direction", "rtl"),
                "ltr" => apply_property(style, "direction", "ltr"),
                "auto" => {
                    let text = collect_text_for_dir_auto(root);
                    if let Some(dir) = crate::layout::text::first_strong_direction(&text) {
                        match dir {
                            Direction::RTL => apply_property(style, "direction", "rtl"),
                            Direction::LTR => apply_property(style, "direction", "ltr"),
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    if matches!(root.tag.as_str(), "td" | "th") {
        let has_table_border = ancestors.iter().rev().any(|a| {
            a.tag == "table"
                && a.attributes
                    .get("border")
                    .and_then(|v| v.parse::<f32>().ok())
                    .map_or(false, |n| n > 0.0)
        });
        if has_table_border {
            apply_property(style, "border", "1px solid");
        }
    }
}

fn collect_text_for_dir_auto(node: &WebCore) -> String {
    let mut out = String::new();
    collect_text_for_dir_auto_inner(node, &mut out);
    out
}

fn collect_text_for_dir_auto_inner(node: &WebCore, out: &mut String) {
    if matches!(node.tag.as_str(), "script" | "style") {
        return;
    }
    if node.tag != "#comment" && !node.text.is_empty() {
        out.push_str(&node.text);
    }
    for child in &node.children {
        collect_text_for_dir_auto_inner(child, out);
    }
}
