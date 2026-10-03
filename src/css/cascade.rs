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

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub(super) struct CustomDeclaration<'a> {
    name: &'a str,
    value: &'a str,
    author_origin: bool,
    layer_rank: u32,
    inline: bool,
    important: bool,
}

impl<'a> CustomDeclaration<'a> {
    pub(super) fn rule(name: &'a str, value: &'a str, rule: &CssRule, specificity: u32, important: bool) -> Self {
        Self {
            name,
            value,
            author_origin: is_author_origin(specificity),
            layer_rank: rule.layer_rank,
            inline: false,
            important,
        }
    }

    fn inline(name: &'a str, value: &'a str, important: bool) -> Self {
        Self {
            name,
            value,
            author_origin: true,
            layer_rank: u32::MAX,
            inline: true,
            important,
        }
    }

    fn same_layer(self, other: Self) -> bool {
        self.author_origin == other.author_origin
            && self.inline == other.inline
            && (self.inline || self.layer_rank == other.layer_rank)
    }
}

#[cfg(test)]
fn custom_declaration_value<'a>(
    declarations: &'a [CustomDeclaration<'a>],
    index: usize,
) -> Option<&'a str> {
    let mut index = index;
    loop {
        let current = declarations[index];
        let keyword = current.value.trim();
        let layer_only = if keyword.eq_ignore_ascii_case("revert") {
            false
        } else if keyword.eq_ignore_ascii_case("revert-layer") {
            true
        } else {
            return Some(current.value);
        };
        index = previous_custom_declaration(declarations, index, layer_only)?;
    }
}

fn previous_custom_declaration(
    declarations: &[CustomDeclaration<'_>],
    index: usize,
    layer_only: bool,
) -> Option<usize> {
    let current = declarations[index];
    (0..index).rev().find(|&candidate_index| {
        let candidate = declarations[candidate_index];
        candidate.name == current.name
            && if layer_only {
                !candidate.same_layer(current)
            } else {
                candidate.author_origin != current.author_origin
            }
    })
}

struct CascadedCustomProperties<'a> {
    declarations: &'a [CustomDeclaration<'a>],
    inherited: &'a HashMap<String, String>,
    winners: HashMap<&'a str, usize>,
    memo: HashMap<String, Option<String>>,
    stack: Vec<String>,
    cyclic: HashSet<String>,
}

impl CascadedCustomProperties<'_> {
    fn resolve(&mut self, name: &str) -> Option<String> {
        if let Some(value) = self.memo.get(name) {
            return value.clone();
        }
        let Some(&index) = self.winners.get(name) else {
            return self.inherited.get(name).cloned();
        };
        if let Some(first) = self.stack.iter().position(|entry| entry == name) {
            self.cyclic.extend(self.stack[first..].iter().cloned());
            return None;
        }
        if self.stack.len() >= 128 {
            return None;
        }
        self.stack.push(name.to_string());
        let value = self.resolve_candidate(index);
        self.stack.pop();
        let value = if self.cyclic.contains(name) {
            None
        } else {
            value
        };
        self.memo.insert(name.to_string(), value.clone());
        value
    }

    fn resolve_candidate(&mut self, mut index: usize) -> Option<String> {
        loop {
            let declaration = self.declarations[index];
            let value = if contains_var_function(declaration.value) {
                super::animation::substitute_custom_value_with_lookup(
                    declaration.value,
                    &mut |name| self.resolve(name),
                    0,
                )?
            } else {
                declaration.value.to_string()
            };
            let keyword = value.trim();
            if keyword.eq_ignore_ascii_case("initial") {
                return None;
            }
            if keyword.eq_ignore_ascii_case("inherit") || keyword.eq_ignore_ascii_case("unset") {
                return self.inherited.get(declaration.name).cloned();
            }
            if keyword.eq_ignore_ascii_case("revert") {
                let Some(previous) = previous_custom_declaration(self.declarations, index, false) else {
                    return self.inherited.get(declaration.name).cloned();
                };
                index = previous;
            } else if keyword.eq_ignore_ascii_case("revert-layer") {
                let Some(previous) = previous_custom_declaration(self.declarations, index, true) else {
                    return self.inherited.get(declaration.name).cloned();
                };
                index = previous;
            } else {
                return Some(value);
            }
        }
    }
}

pub(super) fn resolve_cascaded_custom_properties(
    declarations: &[CustomDeclaration<'_>],
    inherited: &HashMap<String, String>,
    vars: &mut HashMap<String, String>,
) {
    let mut winners = HashMap::new();
    for (index, declaration) in declarations.iter().enumerate() {
        winners.insert(declaration.name, index);
    }
    let names: Vec<_> = winners.keys().copied().collect();
    let mut resolver = CascadedCustomProperties {
        declarations,
        inherited,
        winners,
        memo: HashMap::new(),
        stack: Vec::new(),
        cyclic: HashSet::new(),
    };
    for name in names {
        if let Some(value) = resolver.resolve(name) {
            vars.insert(name.to_string(), value);
        } else {
            vars.remove(name);
        }
    }
}

#[cfg(test)]
#[test]
fn custom_property_rollback_crosses_many_layers_without_recursion() {
    let mut declarations = vec![CustomDeclaration {
        name: "--ink",
        value: "red",
        author_origin: true,
        layer_rank: 0,
        inline: false,
        important: false,
    }];
    declarations.extend((1..=2048).map(|layer_rank| CustomDeclaration {
        name: "--ink",
        value: "revert-layer",
        author_origin: true,
        layer_rank,
        inline: false,
        important: false,
    }));
    assert_eq!(custom_declaration_value(&declarations, 2048), Some("red"));
}

pub(super) fn normal_cascade_cmp(
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

pub(super) fn important_cascade_cmp(
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

fn track_inherit_for_id(inherit_props: &mut HashSet<String>, id: properties::PropertyId) {
    let def = property_defs::get(id);
    if def.longhands.is_empty() {
        inherit_props.insert(def.name.to_string());
    } else {
        for &longhand in def.longhands {
            inherit_props.insert(property_defs::get(longhand).name.to_string());
        }
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
            let resolved =
                resolve_var_references_for_color_scheme(s, local_vars, &style.color_scheme);
            if contains_var_function(s) && (resolved.trim().is_empty() || contains_var_function(&resolved)) {
                if properties::is_inherited(id) {
                    if let Some(parent) = parent_style {
                        copy_property_from_style(style, parent, name);
                    } else {
                        apply_css_value(style, id, &CssValue::Initial);
                    }
                } else {
                    apply_css_value(style, id, &CssValue::Initial);
                }
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
    if trimmed.eq_ignore_ascii_case("inherit") {
        if let Some(parent) = parent_style {
            copy_property_from_style(style, parent, prop);
        }
    } else if trimmed.eq_ignore_ascii_case("revert-layer") {
        copy_property_from_style(style, revert_layer_base, prop);
    } else if trimmed.eq_ignore_ascii_case("revert") {
        if let Some(base) = revert_base {
            copy_property_from_style(style, base, prop);
        } else {
            apply_css_value(style, id, &crate::types::CssValue::Initial);
        }
    } else {
        apply_property_by_id_str(style, id, value);
    }
}

fn prescan_color_scheme(
    base: &ComputedStyle,
    rules: &[CssRule],
    matched: &[(u32, usize, Option<u32>)],
    local_vars: &HashMap<String, String>,
    important: bool,
    author_pass: Option<bool>,
) -> String {
    let mut probe: Option<ComputedStyle> = None;
    for &(sp, ri, _) in matched {
        if let Some(author) = author_pass {
            if is_author_origin(sp) != author {
                continue;
            }
        }
        let rule = &rules[ri];
        if important {
            if let Some(val) = rule.important_declarations.get("color-scheme") {
                let current = probe.as_ref().unwrap_or(base);
                let resolved =
                    resolve_var_references_for_color_scheme(val, local_vars, &current.color_scheme);
                if !resolved.trim().is_empty() && !contains_var_function(&resolved) {
                    apply_property(
                        probe.get_or_insert_with(|| base.clone()),
                        "color-scheme",
                        &resolved,
                    );
                }
                continue;
            }
            if let Some((_, val)) = rule
                .compiled_important
                .iter()
                .find(|(id, _)| *id == properties::PropertyId::ColorScheme)
            {
                apply_css_value(
                    probe.get_or_insert_with(|| base.clone()),
                    properties::PropertyId::ColorScheme,
                    val,
                );
            }
        } else {
            if let Some(val) = rule.declarations.get("color-scheme") {
                let current = probe.as_ref().unwrap_or(base);
                let resolved =
                    resolve_var_references_for_color_scheme(val, local_vars, &current.color_scheme);
                if !resolved.trim().is_empty() && !contains_var_function(&resolved) {
                    apply_property(
                        probe.get_or_insert_with(|| base.clone()),
                        "color-scheme",
                        &resolved,
                    );
                }
                continue;
            }
            if let Some((_, val)) = rule
                .compiled_decls
                .iter()
                .find(|(id, _)| *id == properties::PropertyId::ColorScheme)
            {
                apply_css_value(
                    probe.get_or_insert_with(|| base.clone()),
                    properties::PropertyId::ColorScheme,
                    val,
                );
            }
        }
    }
    probe.as_ref().unwrap_or(base).color_scheme.clone()
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
    let has_state_vars = matched
        .iter()
        .any(|(_, ri, _)| stylesheet.rules[*ri].has_custom_properties);
    let mut state_vars_owned = has_state_vars.then(|| local_vars.clone());
    if let Some(vars) = state_vars_owned.as_mut() {
        let mut declarations = Vec::new();
        for &(sp, ri, _) in matched.iter() {
            let rule = &stylesheet.rules[ri];
            for (prop, value) in &rule.declarations {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::rule(prop, value, rule, sp, false));
                }
            }
        }
        matched.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
        for author_pass in [true, false] {
            for &(sp, ri, _) in matched.iter() {
                if is_author_origin(sp) != author_pass {
                    continue;
                }
                let rule = &stylesheet.rules[ri];
                for (prop, value) in &rule.important_declarations {
                    if prop.starts_with("--") {
                        declarations.push(CustomDeclaration::rule(prop, value, rule, sp, true));
                    }
                }
            }
        }
        resolve_cascaded_custom_properties(&declarations, local_vars, vars);
        matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    }
    let local_vars = state_vars_owned.as_ref().unwrap_or(local_vars);
    let needs_layer_snapshot = matched
        .iter()
        .any(|(_, ri, _)| stylesheet.rules[*ri].has_revert_value)
        || (matched
            .iter()
            .any(|(_, ri, _)| stylesheet.rules[*ri].has_var_refs)
            && local_vars
                .values()
                .any(|value| value_mentions_revert(value)));
    let mut current_layer: Option<(bool, u32)> = None;
    let mut layer_start_style = state.clone();
    for &(sp, ri, _) in matched.iter() {
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_layer != Some(layer_key) {
            current_layer = Some(layer_key);
            if needs_layer_snapshot {
                layer_start_style = state.clone();
            }
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
        for &(sp, ri, _) in matched.iter() {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &stylesheet.rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_layer != Some(layer_key) {
                current_layer = Some(layer_key);
                if needs_layer_snapshot {
                    layer_start_style = state.clone();
                }
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
    if let Some(vars) = state_vars_owned {
        state.custom_props = std::sync::Arc::new(vars);
    }
}

fn projected_rule_targets_assigned_node(sel: &CssSelector) -> bool {
    fn part_has_slot_attr(part: &SelectorPart) -> bool {
        match part {
            SelectorPart::Attribute { name, .. } => name.eq_ignore_ascii_case("slot"),
            SelectorPart::Not(sel) => sel.parts.iter().any(part_has_slot_attr),
            SelectorPart::Is(list) | SelectorPart::Where(list) | SelectorPart::Has(list) => list
                .iter()
                .any(|sel| sel.parts.iter().any(part_has_slot_attr)),
            _ => false,
        }
    }

    sel.parts
        .iter()
        .any(|part| matches!(part, SelectorPart::Combinator(_)))
        && sel.parts.iter().any(part_has_slot_attr)
}

pub(crate) fn projected_ancestor_info(node: &WebCore) -> AncestorInfo {
    AncestorInfo {
        tag: node.tag.clone(),
        attributes: std::sync::Arc::new(node.attributes.clone()),
        auto_direction: super::matching::auto_direction(node),
        child_index: 0,
        sibling_count: 1,
        type_child_index: 0,
        type_sibling_count: 1,
        node_id: node.node_id,
        prev_siblings: std::sync::Arc::new(Vec::new()),
    }
}

pub(crate) fn apply_host_projected_rules_to_projected(
    node: &mut crate::types::WebCore,
    host_node: Option<&AncestorInfo>,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    vw: f32,
    vh: f32,
    candidates_buf: &mut Vec<usize>,
) {
    let Some(host_node) = host_node else {
        return;
    };
    if !node.is_element() || !crate::types::is_projected_slot_subtree(node) {
        return;
    }

    let empty_hover = std::collections::HashSet::new();
    let empty_focus = std::collections::HashSet::new();
    apply_host_projected_rules_with_ancestors(
        node,
        std::slice::from_ref(host_node),
        stylesheet,
        parent_style,
        vw,
        vh,
        0,
        false,
        &empty_hover,
        &empty_focus,
        "",
        candidates_buf,
    );
}

#[allow(clippy::too_many_arguments)]
fn apply_host_projected_rules_with_ancestors(
    node: &mut crate::types::WebCore,
    ancestors: &[AncestorInfo],
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    document_url: &str,
    candidates_buf: &mut Vec<usize>,
) {
    if ancestors.is_empty() || !node.is_element() || !crate::types::is_projected_slot_subtree(node)
    {
        return;
    }

    let id = node.attributes.get("id").map(|s| s.as_str());
    let class_attr = node
        .attributes
        .get("class")
        .map(|s| s.as_str())
        .unwrap_or("");
    stylesheet.candidate_rules(&node.tag, id, class_attr.split_whitespace(), candidates_buf);
    for (rule_idx, rule) in stylesheet.rules.iter().enumerate() {
        if rule
            .selectors
            .iter()
            .any(projected_rule_targets_assigned_node)
            && !candidates_buf.contains(&rule_idx)
        {
            candidates_buf.push(rule_idx);
        }
    }

    let ctx = MatchContext {
        focused_box,
        keyboard_focus,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: Some(node),
        ancestor_nodes: &[],
        hover_chain,
        focus_within_chain,
        element_id: node.node_id,
        scope_root_id: 0,
        target_id: 0,
        document_url,
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    let mut matched = Vec::new();
    for &rule_idx in candidates_buf.iter() {
        let rule = &stylesheet.rules[rule_idx];
        if rule.is_slotted || rule.pseudo_element != PseudoElement::None {
            continue;
        }
        if !rule.media_condition.matches(vw, vh) {
            continue;
        }
        if !rule.container_condition.is_empty() {
            continue;
        }
        let Some(scope_proximity) = rule_matches_scope(rule, node, &ancestors, 0, 1, &ctx) else {
            continue;
        };
        if rule.selectors.iter().any(|sel| {
            projected_rule_targets_assigned_node(sel)
                && sel.matches_with_ancestors_ctx(node, 0, 1, &ancestors, &ctx)
        }) {
            matched.push((rule.specificity, rule_idx, scope_proximity));
        }
    }
    candidates_buf.clear();
    if matched.is_empty() {
        return;
    }

    matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    let mut style = (*node.style).clone();
    let mut local_vars = stylesheet.variables.clone();
    local_vars.extend(
        style
            .custom_props
            .iter()
            .map(|(k, v)| (k.clone(), v.clone())),
    );
    let has_vars = !local_vars.is_empty();
    let mut current_layer: Option<(bool, u32)> = None;
    let mut layer_start_style = style.clone();
    for &(sp, ri, _) in &matched {
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_layer != Some(layer_key) {
            current_layer = Some(layer_key);
            layer_start_style = style.clone();
        }
        if has_vars && rule.has_var_refs {
            for (prop, val) in &rule.declarations {
                if prop.starts_with("--") {
                    continue;
                }
                let resolved =
                    resolve_var_references_for_color_scheme(val, &local_vars, &style.color_scheme);
                if contains_var_function(val) && (resolved.trim().is_empty() || contains_var_function(&resolved))
                {
                    continue;
                }
                let id = properties::resolve(prop);
                apply_resolved_property_with_cascade_context(
                    &mut style,
                    prop,
                    id,
                    &resolved,
                    parent_style,
                    None,
                    &layer_start_style,
                );
            }
        } else {
            for &(id, ref val) in &rule.compiled_decls {
                apply_css_value_with_cascade_context(
                    &mut style,
                    id,
                    val,
                    &local_vars,
                    parent_style,
                    None,
                    &layer_start_style,
                );
            }
        }
    }

    let mut important = matched;
    important.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
    for author_pass in [true, false] {
        let mut current_layer: Option<(bool, u32)> = None;
        let mut layer_start_style = style.clone();
        for &(sp, ri, _) in &important {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &stylesheet.rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_layer != Some(layer_key) {
                current_layer = Some(layer_key);
                layer_start_style = style.clone();
            }
            if has_vars && rule.has_var_refs {
                for (prop, val) in &rule.important_declarations {
                    if prop.starts_with("--") {
                        continue;
                    }
                    let resolved = resolve_var_references_for_color_scheme(
                        val,
                        &local_vars,
                        &style.color_scheme,
                    );
                    if contains_var_function(val)
                        && (resolved.trim().is_empty() || contains_var_function(&resolved))
                    {
                        continue;
                    }
                    let id = properties::resolve(prop);
                    apply_resolved_property_with_cascade_context(
                        &mut style,
                        prop,
                        id,
                        &resolved,
                        parent_style,
                        None,
                        &layer_start_style,
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
                        None,
                        &layer_start_style,
                    );
                }
            }
        }
    }

    let old_display = node.style.display;
    let new_display = style.display;
    node.style = std::sync::Arc::new(style);
    node.layout.layout_dirty = true;
    if old_display != new_display
        && matches!(old_display, Display::None) != matches!(new_display, Display::None)
    {
        mark_layout_subtree_dirty(node);
    }
}

pub(crate) fn apply_slotted_rules_to_projected(
    node: &mut crate::types::WebCore,
    slot_node: Option<&crate::types::WebCore>,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    vw: f32,
    vh: f32,
    candidates_buf: &mut Vec<usize>,
) {
    if !node.is_element() || !crate::types::is_projected_slot_subtree(node) {
        return;
    }

    let id = node.attributes.get("id").map(|s| s.as_str());
    let class_attr = node
        .attributes
        .get("class")
        .map(|s| s.as_str())
        .unwrap_or("");
    stylesheet.candidate_rules(&node.tag, id, class_attr.split_whitespace(), candidates_buf);

    let empty_hover = std::collections::HashSet::new();
    let empty_focus = std::collections::HashSet::new();
    let ctx = MatchContext {
        focused_box: 0,
        keyboard_focus: false,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: Some(node),
        ancestor_nodes: &[],
        hover_chain: &empty_hover,
        focus_within_chain: &empty_focus,
        element_id: node.node_id,
        scope_root_id: 0,
        target_id: 0,
        document_url: "",
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    let mut matched = Vec::new();
    for &rule_idx in candidates_buf.iter() {
        let rule = &stylesheet.rules[rule_idx];
        if !rule.is_slotted {
            continue;
        }
        if let Some(slot_selector) = &rule.slotted_slot_selector {
            let Some(slot_node) = slot_node else {
                continue;
            };
            let slot_ctx = MatchContext {
                focused_box: 0,
                keyboard_focus: false,
                type_child_index: 0,
                type_sibling_count: 1,
                html_box: Some(slot_node),
                ancestor_nodes: &[],
                hover_chain: &empty_hover,
                focus_within_chain: &empty_focus,
                element_id: slot_node.node_id,
                scope_root_id: 0,
                target_id: 0,
                document_url: "",
                prev_siblings: &[],
                next_siblings: &[],
                next_sibling_nodes: &[],
            };
            if !slot_selector.matches_with_ancestors_ctx(slot_node, 0, 1, &[], &slot_ctx) {
                continue;
            }
        }
        if !rule.media_condition.matches(vw, vh) {
            continue;
        }
        if !rule.container_condition.is_empty() {
            continue;
        }
        if rule
            .selectors
            .iter()
            .any(|sel| sel.matches_with_ancestors_ctx(node, 0, 1, &[], &ctx))
        {
            matched.push((rule.specificity, rule_idx, None));
        }
    }
    candidates_buf.clear();
    if matched.is_empty() {
        return;
    }

    matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    let mut style = (*node.style).clone();
    let local_vars = style.custom_props.clone();
    let has_vars = !local_vars.is_empty();
    let mut current_layer: Option<(bool, u32)> = None;
    let mut layer_start_style = style.clone();
    for &(sp, ri, _) in &matched {
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_layer != Some(layer_key) {
            current_layer = Some(layer_key);
            layer_start_style = style.clone();
        }
        if has_vars && rule.has_var_refs {
            for (prop, val) in &rule.declarations {
                if prop.starts_with("--") {
                    continue;
                }
                let resolved =
                    resolve_var_references_for_color_scheme(val, &local_vars, &style.color_scheme);
                if contains_var_function(val) && (resolved.trim().is_empty() || contains_var_function(&resolved))
                {
                    continue;
                }
                let id = properties::resolve(prop);
                apply_resolved_property_with_cascade_context(
                    &mut style,
                    prop,
                    id,
                    &resolved,
                    parent_style,
                    None,
                    &layer_start_style,
                );
            }
        } else {
            for &(id, ref val) in &rule.compiled_decls {
                apply_css_value_with_cascade_context(
                    &mut style,
                    id,
                    val,
                    &local_vars,
                    parent_style,
                    None,
                    &layer_start_style,
                );
            }
        }
    }

    let mut important = matched;
    important.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
    for author_pass in [true, false] {
        let mut current_layer: Option<(bool, u32)> = None;
        let mut layer_start_style = style.clone();
        for &(sp, ri, _) in &important {
            if is_author_origin(sp) != author_pass {
                continue;
            }
            let rule = &stylesheet.rules[ri];
            let layer_key = (is_author_origin(sp), rule.layer_rank);
            if current_layer != Some(layer_key) {
                current_layer = Some(layer_key);
                layer_start_style = style.clone();
            }
            if has_vars && rule.has_var_refs {
                for (prop, val) in &rule.important_declarations {
                    if prop.starts_with("--") {
                        continue;
                    }
                    let resolved = resolve_var_references_for_color_scheme(
                        val,
                        &local_vars,
                        &style.color_scheme,
                    );
                    if contains_var_function(val)
                        && (resolved.trim().is_empty() || contains_var_function(&resolved))
                    {
                        continue;
                    }
                    let id = properties::resolve(prop);
                    apply_resolved_property_with_cascade_context(
                        &mut style,
                        prop,
                        id,
                        &resolved,
                        parent_style,
                        None,
                        &layer_start_style,
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
                        None,
                        &layer_start_style,
                    );
                }
            }
        }
    }

    let old_display = node.style.display;
    let new_display = style.display;
    node.style = std::sync::Arc::new(style);
    node.layout.layout_dirty = true;
    if old_display != new_display
        && matches!(old_display, Display::None) != matches!(new_display, Display::None)
    {
        mark_layout_subtree_dirty(node);
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
    let focus_within_chain = crate::css::build_hover_chain(root, focused_box);
    // Use parallel cascade when the stylesheet is large enough to justify the overhead.
    if (stylesheet.rules.len() > 1000
        && std::env::var_os("WEBCORE_DISABLE_PARALLEL_CASCADE").is_none()
        || stylesheet.has_ancestor_has_rules)
        && !tree_has_duplicate_node_ids(root)
    {
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
            &focus_within_chain,
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
    let mut counters = CounterState::default();
    let mut share_cache = ShareCache::new();
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
        &focus_within_chain,
        &[],
        &[],
        &[],
        &mut share_cache,
        None,
    );
    resolve_document_generated_content(root, stylesheet);
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

fn find_counter_style<'a>(
    stylesheet: &'a Stylesheet,
    name: &str,
) -> Option<&'a crate::css::CounterStyleRule> {
    let (vw, vh) = stylesheet.counter_style_viewport;
    stylesheet
        .counter_styles
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.name == name && rule.media_condition.matches(vw, vh))
        .max_by_key(|(source_order, rule)| {
            (
                rule.author_origin,
                stylesheet.layer_rank(&rule.layer),
                *source_order,
            )
        })
        .map(|(_, rule)| rule)
}

// CSS Counter Styles 3 permits fallback for representations longer than 60 codepoints.
const MAX_COUNTER_REPRESENTATION_CODEPOINTS: usize = 60;

fn resolve_custom_counter_style_marker_inner(
    stylesheet: &Stylesheet,
    name: &str,
    index: i32,
    resolving: &mut Vec<String>,
) -> Option<String> {
    let rule = find_counter_style(stylesheet, name)?;
    let prefix = counter_style_decl(stylesheet, rule, "prefix", resolving)
        .map(|value| resolve_content_value_with_context(value, None, None))
        .unwrap_or_default();
    let suffix = counter_style_decl(stylesheet, rule, "suffix", resolving)
        .map(|value| resolve_content_value_with_context(value, None, None))
        .unwrap_or_else(|| ". ".to_string());
    let body =
        resolve_custom_counter_style_representation_inner(stylesheet, name, index, resolving)?;
    Some(format!("{prefix}{body}{suffix}"))
}

fn resolve_custom_counter_style_representation_inner(
    stylesheet: &Stylesheet,
    name: &str,
    index: i32,
    resolving: &mut Vec<String>,
) -> Option<String> {
    let rule = find_counter_style(stylesheet, name)?;
    if resolving.iter().any(|seen| seen == &rule.name) {
        return None;
    }
    resolving.push(rule.name.clone());
    let system = counter_style_system(stylesheet, rule, resolving);
    let symbols = if system.starts_with("additive") {
        Vec::new()
    } else {
        let symbols = counter_style_decl(stylesheet, rule, "symbols", resolving)
            .and_then(|value| counter_style_symbols(value));
        let Some(symbols) = symbols else {
            resolving.pop();
            return None;
        };
        if symbols.is_empty() {
            resolving.pop();
            return None;
        }
        symbols
    };
    let uses_negative_sign = index < 0
        && (system.starts_with("symbolic")
            || system.starts_with("alphabetic")
            || system.starts_with("numeric")
            || system.starts_with("additive"));
    let algorithm_value = if uses_negative_sign {
        i64::from(index).abs()
    } else {
        i64::from(index)
    };
    let representation = if !counter_style_range_contains(
        counter_style_decl(stylesheet, rule, "range", resolving),
        &system,
        index,
    ) {
        None
    } else if system.starts_with("cyclic") {
        let idx = (i64::from(index) - 1).rem_euclid(symbols.len() as i64) as usize;
        Some(symbols[idx].clone())
    } else if system.starts_with("fixed") {
        let first = fixed_counter_first_value(&system);
        let offset = i64::from(index) - i64::from(first);
        if offset < 0 || offset as usize >= symbols.len() {
            None
        } else {
            Some(symbols[offset as usize].clone())
        }
    } else if system.starts_with("numeric") && symbols.len() >= 2 {
        Some(numeric_counter_symbols(algorithm_value, &symbols))
    } else if system.starts_with("alphabetic") && symbols.len() >= 2 {
        alphabetic_counter_symbols(algorithm_value, &symbols)
    } else if system.starts_with("additive") {
        counter_style_additive_body(
            counter_style_decl(stylesheet, rule, "additive-symbols", resolving),
            algorithm_value,
        )
    } else {
        let repeats = usize::try_from(algorithm_value).unwrap_or(0);
        (repeats > 0
            && symbols[0].chars().count().saturating_mul(repeats)
                <= MAX_COUNTER_REPRESENTATION_CODEPOINTS)
            .then(|| symbols[0].repeat(repeats))
    };
    let representation = representation.and_then(|mut body| {
        match counter_style_padded_body(
            counter_style_decl(stylesheet, rule, "pad", resolving),
            &body,
        ) {
            Ok(Some(padded)) => body = padded,
            Ok(None) => {}
            Err(()) => return None,
        }
        if uses_negative_sign {
            let negative = counter_style_decl(stylesheet, rule, "negative", resolving)
                .and_then(|value| counter_style_symbols(value));
            let before = negative
                .as_ref()
                .and_then(|symbols| symbols.first())
                .map(String::as_str)
                .unwrap_or("-");
            let after = negative
                .as_ref()
                .and_then(|symbols| symbols.get(1))
                .map(String::as_str)
                .unwrap_or("");
            body = format!("{before}{body}{after}");
        }
        Some(body)
    });
    let Some(body) = representation else {
        let fallback = counter_style_fallback_body(stylesheet, rule, index, resolving)
            .unwrap_or_else(|| crate::css::format_counter_value(index, "decimal"));
        resolving.pop();
        return Some(fallback);
    };
    resolving.pop();
    Some(body)
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
    if resolving.iter().any(|seen| seen == base) {
        return None;
    }
    let base_rule = find_counter_style(stylesheet, base)?;
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
    if resolving.iter().any(|seen| seen == base) {
        return "symbolic".to_string();
    }
    find_counter_style(stylesheet, base)
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

fn counter_style_additive_body(additive: Option<&String>, index: i64) -> Option<String> {
    if index < 0 {
        return None;
    }
    let mut remaining = index;
    let mut out = String::new();
    for part in additive?.split(',') {
        let part = part.trim();
        let split = part
            .char_indices()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))?;
        let weight = part[..split].trim().parse::<i64>().ok()?;
        if weight < 0 {
            return None;
        }
        let symbol = resolve_content_value_with_context(part[split..].trim(), None, None);
        if symbol.is_empty() {
            return None;
        }
        if index == 0 && weight == 0 {
            return Some(symbol);
        }
        if weight == 0 {
            continue;
        }
        if symbol
            .chars()
            .count()
            .saturating_mul((remaining / weight) as usize)
            > MAX_COUNTER_REPRESENTATION_CODEPOINTS
        {
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
    if fallback == "decimal" {
        Some(crate::css::format_counter_value(index, "decimal"))
    } else if find_counter_style(stylesheet, fallback).is_some() {
        resolve_custom_counter_style_representation_inner(stylesheet, fallback, index, resolving)
    } else {
        Some(crate::css::format_counter_value(index, fallback))
    }
}

fn counter_style_range_contains(range: Option<&String>, system: &str, index: i32) -> bool {
    let Some(range) = range else {
        return counter_style_auto_range_contains(system, index);
    };
    let range = range.trim();
    if range.eq_ignore_ascii_case("auto") || range.is_empty() {
        return counter_style_auto_range_contains(system, index);
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

fn counter_style_auto_range_contains(system: &str, index: i32) -> bool {
    if system.starts_with("alphabetic") || system.starts_with("symbolic") {
        index >= 1
    } else if system.starts_with("additive") {
        index >= 0
    } else {
        true
    }
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

fn counter_style_padded_body(pad: Option<&String>, body: &str) -> Result<Option<String>, ()> {
    let Some(pad) = pad else { return Ok(None) };
    let pad = pad.trim();
    let mut parts = pad.splitn(2, char::is_whitespace);
    let Some(width) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
        return Ok(None);
    };
    let Some(symbol_src) = parts.next().map(str::trim) else {
        return Ok(None);
    };
    let symbol = counter_style_symbols(symbol_src)
        .and_then(|mut symbols| symbols.pop())
        .filter(|symbol| !symbol.is_empty())
        .unwrap_or_else(|| resolve_content_value_with_context(symbol_src, None, None));
    let body_len = body.chars().count();
    if symbol.is_empty() || body_len >= width {
        return Ok(None);
    }
    if symbol
        .chars()
        .count()
        .saturating_mul(width - body_len)
        .saturating_add(body_len)
        > MAX_COUNTER_REPRESENTATION_CODEPOINTS
    {
        return Err(());
    }
    Ok(Some(format!("{}{}", symbol.repeat(width - body_len), body)))
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

fn numeric_counter_symbols(value: i64, symbols: &[String]) -> String {
    if value == 0 {
        return symbols[0].clone();
    }
    let mut n = value;
    let base = symbols.len() as i64;
    let mut parts = Vec::new();
    while n > 0 {
        parts.push(symbols[(n % base) as usize].clone());
        n /= base;
    }
    parts.into_iter().rev().collect()
}

fn alphabetic_counter_symbols(mut value: i64, symbols: &[String]) -> Option<String> {
    if value <= 0 {
        return None;
    }
    let base = symbols.len() as i64;
    let mut parts = Vec::new();
    while value > 0 {
        value -= 1;
        parts.push(symbols[(value % base) as usize].clone());
        value /= base;
    }
    Some(parts.into_iter().rev().collect())
}

#[cfg(test)]
mod counter_style_tests {
    use super::*;

    #[test]
    fn conditional_and_layered_counter_styles_follow_the_cascade() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author(
            r#"
            @layer early, late;
            @layer late {
                @counter-style badge { system: cyclic; symbols: "L"; suffix: "."; }
            }
            @layer early {
                @counter-style badge { system: cyclic; symbols: "E"; suffix: "."; }
            }
            @supports (unknown-property: impossible) {
                @counter-style badge { system: cyclic; symbols: "X"; suffix: "."; }
            }
            @media (min-width: 700px) {
                @layer late {
                    @counter-style badge { system: cyclic; symbols: "W"; suffix: "."; }
                }
            }
            "#,
        );
        sheet.resolve_variables_for_viewport(600.0, 800.0);
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("L.")
        );
        sheet.resolve_variables_for_viewport(800.0, 800.0);
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("W.")
        );

        sheet.parse_and_add_author(
            "@counter-style badge { system: cyclic; symbols: 'U'; suffix: '.'; }",
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("U.")
        );
    }

    #[test]
    fn linked_counter_styles_keep_link_media_and_author_origin() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add("@counter-style badge { system: cyclic; symbols: 'UA'; }");
        sheet.parse_and_add_with_base_media_conditions(
            "@layer linked { @counter-style badge { system: cyclic; symbols: 'LINK'; } }",
            "https://example.test/site.css",
            &crate::css::MediaConditions::default().with_query("(min-width: 700px)"),
        );
        sheet.resolve_variables_for_viewport(600.0, 800.0);
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("UA. ")
        );
        sheet.resolve_variables_for_viewport(800.0, 800.0);
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("LINK. ")
        );
    }

    #[test]
    fn custom_names_are_case_sensitive_and_later_definitions_replace_earlier_ones() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add(
            r##"
            @counter-style Badge { system: cyclic; symbols: "A"; suffix: "!"; }
            @counter-style badge { system: cyclic; symbols: "b"; suffix: "?"; }
            @counter-style Badge { system: cyclic; symbols: "C"; suffix: "#"; }
            @counter-style limited {
                system: fixed 2;
                symbols: "L";
                fallback: badge;
                suffix: ")";
            }
        "##,
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "Badge", 1).as_deref(),
            Some("C#")
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "badge", 1).as_deref(),
            Some("b?")
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "BADGE", 1),
            None
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "limited", 1).as_deref(),
            Some("b)")
        );
    }

    #[test]
    fn negative_minimum_zero_and_large_repetitions_use_bounded_representations() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add(
            r#"
            @counter-style binary { system: numeric; symbols: "0" "1"; suffix: " "; }
            @counter-style tally { system: additive; additive-symbols: 5 "V", 1 "I", 0 "Z"; }
            @counter-style repeated { system: symbolic; symbols: "R"; }
            @counter-style padded { system: cyclic; symbols: "X"; pad: 1000000000 "0"; }
        "#,
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "binary", i32::MIN),
            Some(format!("-{:b} ", i32::MIN.unsigned_abs()))
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "tally", 0).as_deref(),
            Some("Z. ")
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "repeated", 60),
            Some(format!("{}. ", "R".repeat(60)))
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "repeated", 61).as_deref(),
            Some("61. ")
        );
        assert_eq!(
            resolve_custom_counter_style_marker(&sheet, "padded", 1).as_deref(),
            Some("1. ")
        );
    }
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

/// Counters and quotes follow document order independently of style matching.
/// Clean siblings can change when an earlier element's counter operations change.
pub(crate) fn resolve_document_generated_content(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
) {
    let _profile = crate::profile::is_enabled()
        .then(|| crate::profile::span(crate::profile::Phase::CascadeGeneratedContent));
    let mut pending = vec![&*root];
    let mut automatic = false;
    while let Some(node) = pending.pop() {
        if node.style.display == Display::None {
            continue;
        }
        automatic = std::iter::once(&*node.style)
            .chain(node.style.before_style.as_deref())
            .chain(node.style.after_style.as_deref())
            .chain(node.style.marker_style.as_deref())
            .any(|style| {
                style
                    .counter_reset
                    .iter()
                    .any(|reset| reset.value.is_none() && !reset.html_list_start)
            });
        if automatic {
            break;
        }
        pending.extend(node.children.iter());
        if let Some(shadow) = &node.shadow_root {
            pending.extend(shadow.children.iter());
        }
    }
    let initial_values = if automatic {
        replay_document_generated_content(root, stylesheet, Vec::new(), true)
    } else {
        Vec::new()
    };
    replay_document_generated_content(root, stylesheet, initial_values, false);
}

fn replay_document_generated_content(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    initial_values: Vec<i32>,
    collecting: bool,
) -> Vec<i32> {
    fn html_list_initial(node: &crate::types::WebCore) -> Option<i32> {
        if !node
            .style
            .counter_reset
            .iter()
            .any(|reset| reset.html_list_start)
        {
            return None;
        }
        let mut count = 0i32;
        let mut pending: Vec<_> = node.children.iter().collect();
        while let Some(child) = pending.pop() {
            if child.style.display == Display::None {
                continue;
            }
            if matches!(child.tag.as_str(), "ol" | "ul" | "menu") {
                continue;
            }
            if child.tag == "li" {
                count = count.saturating_add(1);
            }
            pending.extend(child.children.iter());
        }
        // The shared counter machinery decrements before painting the first item.
        Some(count.saturating_add(1))
    }
    fn dirty(layout: &mut crate::types::LayoutBox, descendants: &mut bool) {
        layout.layout_dirty = true;
        layout.intrinsic_dirty = true;
        layout.paint_dirty = true;
        layout.line_cache.clear();
        layout.cached_intrinsic_w.set(f32::NAN);
        *descendants = true;
    }
    fn expand(
        owner: &mut std::sync::Arc<ComputedStyle>,
        pseudo: &str,
        depth: &mut usize,
        counters: &mut CounterState,
    ) -> bool {
        let style = match pseudo {
            "::before" => owner.before_style.as_deref(),
            "::after" => owner.after_style.as_deref(),
            _ => owner.marker_style.as_deref(),
        };
        let Some(style) = style else {
            return false;
        };
        if style.display == Display::None {
            return false;
        }
        counters.apply_element(style);
        if counters.collecting {
            return false;
        }
        if style.rare().content_template.is_empty() {
            return false;
        }
        let text = render_counter_content(
            &style.rare().content_template,
            style.rare().quotes.as_deref(),
            depth,
            &counters.values,
        );
        {
            let current = match pseudo {
                "::before" => &owner.before_content,
                "::after" => &owner.after_content,
                _ => &owner.marker_content,
            };
            if *current == text {
                return false;
            }
            let style = std::sync::Arc::make_mut(owner);
            match pseudo {
                "::before" => style.before_content = text,
                "::after" => style.after_content = text,
                _ => style.marker_content = text,
            }
        }
        true
    }
    fn anonymous(node: &crate::types::WebCore) -> bool {
        matches!(
            node.tag.as_str(),
            "anonymous-block" | "anonymous-table" | "anonymous-table-row" | "anonymous-table-cell"
        )
    }
    fn materialized_pseudos(node: &crate::types::WebCore) -> (bool, bool) {
        let mut found = (false, false);
        if node.style.before_style.is_none() && node.style.after_style.is_none() {
            return found;
        }
        let mut pending: Vec<_> = node.children.iter().collect();
        while let Some(child) = pending.pop() {
            match child.tag.as_str() {
                "::before" => found.0 = true,
                "::after" => found.1 = true,
                _ if anonymous(child) => pending.extend(child.children.iter()),
                _ => {}
            }
        }
        found
    }
    enum Visit<'a> {
        Enter(&'a mut crate::types::WebCore),
        Exit {
            style: &'a mut std::sync::Arc<ComputedStyle>,
            layout: &'a mut crate::types::LayoutBox,
            descendants: &'a mut bool,
            materialized_after: bool,
            revision: usize,
            contained_depth: Option<usize>,
            anonymous: bool,
        },
    }
    let mut work = vec![Visit::Enter(root)];
    let mut depth = 0;
    let mut revision = 0;
    let mut counters = CounterState {
        collecting,
        initial_values,
        ..CounterState::default()
    };
    while let Some(visit) = work.pop() {
        match visit {
            Visit::Enter(node) => {
                if node.style.display == Display::None {
                    continue;
                }
                if matches!(node.tag.as_str(), "::before" | "::after") {
                    counters.apply_element(&node.style);
                    let template = &node.style.rare().content_template;
                    if !collecting && !template.is_empty() {
                        let text = render_counter_content(
                            template,
                            node.style.rare().quotes.as_deref(),
                            &mut depth,
                            &counters.values,
                        );
                        // Flex/grid pseudo-elements own an anonymous text item.
                        let target = if node.text.is_empty()
                            && node.children.first().is_some_and(|c| c.tag == "#text")
                        {
                            &mut node.children[0]
                        } else {
                            &mut *node
                        };
                        if target.text != text {
                            target.text = text;
                            dirty(&mut target.layout, &mut target.has_dirty_layout_descendant);
                            dirty(&mut node.layout, &mut node.has_dirty_layout_descendant);
                            revision += 1;
                        }
                    }
                    continue;
                }
                let initial_revision = revision;
                let anonymous = anonymous(node);
                let (materialized_before, materialized_after) = if anonymous {
                    (false, false)
                } else {
                    materialized_pseudos(node)
                };
                if !anonymous {
                    counters.apply_element_with_list_start(&node.style, html_list_initial(node));
                }
                if !collecting && !anonymous && node.style.display == Display::ListItem {
                    let value = counters
                        .values
                        .get("list-item")
                        .and_then(|v| v.last())
                        .copied()
                        .unwrap_or(0);
                    let marker = resolve_custom_counter_style_marker(
                        stylesheet,
                        &node.style.custom_list_style_type,
                        value,
                    );
                    if node.style.list_index != value
                        || marker
                            .as_ref()
                            .is_some_and(|m| *m != node.style.marker_content)
                    {
                        let style = std::sync::Arc::make_mut(&mut node.style);
                        style.list_index = value;
                        if let Some(marker) = marker {
                            style.marker_content = marker;
                        }
                        revision += 1;
                    }
                }
                let contained_depth = (!anonymous
                    && node.style.contain_style
                    && node.style.display != Display::Contents)
                    .then_some(depth);
                if !anonymous {
                    counters.enter_children();
                }
                if contained_depth.is_some() {
                    counters.enter_containment();
                }
                if !anonymous {
                    revision += usize::from(expand(
                        &mut node.style,
                        "::marker",
                        &mut depth,
                        &mut counters,
                    ));
                }
                if !anonymous && !materialized_before {
                    revision += usize::from(expand(
                        &mut node.style,
                        "::before",
                        &mut depth,
                        &mut counters,
                    ));
                }
                let base = work.len();
                let mut before_shadow = None;
                for child in node.children.iter_mut().rev() {
                    if node.shadow_root.is_some() && child.tag == "::before" {
                        before_shadow = Some(child);
                    } else if node.shadow_root.is_none() || child.tag == "::after" {
                        work.push(Visit::Enter(child));
                    }
                }
                if let Some(shadow) = &mut node.shadow_root {
                    for child in shadow.children.iter_mut().rev() {
                        work.push(Visit::Enter(child));
                    }
                }
                if let Some(before) = before_shadow {
                    work.push(Visit::Enter(before));
                }
                // Keep the owner's exit below its children without retaining a
                // recursive stack frame or borrowing the whole owner twice.
                work.insert(
                    base,
                    Visit::Exit {
                        style: &mut node.style,
                        layout: &mut node.layout,
                        descendants: &mut node.has_dirty_layout_descendant,
                        materialized_after,
                        revision: initial_revision,
                        contained_depth,
                        anonymous,
                    },
                );
            }
            Visit::Exit {
                style,
                layout,
                descendants,
                materialized_after,
                revision: initial_revision,
                contained_depth,
                anonymous,
            } => {
                if !anonymous {
                    if !materialized_after {
                        revision +=
                            usize::from(expand(style, "::after", &mut depth, &mut counters));
                    }
                    counters.exit_children();
                }
                if let Some(entry_depth) = contained_depth {
                    depth = entry_depth;
                    counters.exit_containment();
                }
                if revision != initial_revision {
                    dirty(layout, descendants);
                }
            }
        }
    }
    counters
        .automatic
        .iter()
        .map(|initial| {
            initial
                .total
                .saturating_add(initial.last_increment_negated)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
        })
        .collect()
}

fn resolve_generated_content(
    text: &str,
    attrs: &crate::dom::attrs::AttrMap,
    style: &mut ComputedStyle,
    counters: &HashMap<String, Vec<i32>>,
) -> String {
    let parts = parse_content_parts(text, Some(attrs));
    let resolved = render_counter_content(&parts, style.rare().quotes.as_deref(), &mut 0, counters);
    if parts.iter().any(|part| match part {
        crate::types::GeneratedContentPart::Quote { .. } => true,
        crate::types::GeneratedContentPart::Text(text) => text.contains('\x01'),
    }) {
        style.rare_mut().content_template = parts;
    }
    resolved
}

fn render_counter_content(
    template: &[crate::types::GeneratedContentPart],
    quotes: Option<&[String]>,
    depth: &mut usize,
    counters: &HashMap<String, Vec<i32>>,
) -> String {
    let mut out = String::new();
    for part in template {
        match part {
            crate::types::GeneratedContentPart::Text(text) if text.contains('\x01') => {
                out.push_str(&resolve_counters_in_content(text, counters));
            }
            crate::types::GeneratedContentPart::Text(text) => out.push_str(text),
            crate::types::GeneratedContentPart::Quote { .. } => {
                out.push_str(&render_content_parts(
                    std::slice::from_ref(part),
                    quotes,
                    depth,
                ));
            }
        }
    }
    out
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
            let resolved = resolve_var_references_for_color_scheme(val, vars, &ps.color_scheme);
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
                let resolved = resolve_var_references_for_color_scheme(val, vars, &ps.color_scheme);
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
    // Pseudo-elements use this detached cascade path, so they need the same
    // post-cascade fixups normal elements receive below. Without this,
    // `background-color: currentColor` on generated mask icons computes as
    // transparent, leaving menus/search controls with empty icon boxes until a
    // later recascade happens to repaint them.
    crate::css::finalize_current_color(&mut ps);
    crate::css::finalize_logical(&mut ps);
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
    fn wrap_layout_item_pseudo_text(pseudo_box: &mut crate::types::WebCore) {
        if (pseudo_box.text.is_empty() && pseudo_box.style.rare().content_template.is_empty())
            || !matches!(
                pseudo_box.style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            )
        {
            return;
        }
        let mut text_box = crate::types::WebCore::new("#text");
        text_box.node_id = crate::dom::arena::next_shadow_node_id();
        text_box.text = std::mem::take(&mut pseudo_box.text);
        let mut text_style = ComputedStyle::default();
        text_style.inherit_from(&pseudo_box.style);
        text_box.style = std::sync::Arc::new(text_style);
        pseudo_box.children.push(text_box);
    }

    fn mark_pseudo_layout_dirty(node: &mut crate::types::WebCore) {
        node.layout.layout_dirty = true;
        node.layout.intrinsic_dirty = true;
        node.layout.paint_dirty = true;
        node.has_dirty_layout_descendant = true;
        node.layout.cached_intrinsic_w.set(f32::NAN);
    }

    fn preserve_loaded_pseudo_resources(
        pseudo_box: &mut crate::types::WebCore,
        existing: Option<&crate::types::WebCore>,
    ) {
        let Some(existing) = existing else {
            return;
        };
        fn background_layer_urls_match(
            a: &[crate::types::BackgroundLayer],
            b: &[crate::types::BackgroundLayer],
        ) -> bool {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(left, right)| left.image_url == right.image_url)
        }
        if pseudo_box.style.background_image_url == existing.style.background_image_url {
            pseudo_box.bg_image_data = existing.bg_image_data.clone();
            pseudo_box.bg_image_width = existing.bg_image_width;
            pseudo_box.bg_image_height = existing.bg_image_height;
            pseudo_box.bg_image_ratio_only = existing.bg_image_ratio_only;
            pseudo_box.bg_image_resolution = existing.bg_image_resolution;
        }
        if background_layer_urls_match(
            &pseudo_box.style.rare().additional_background_layers,
            &existing.style.rare().additional_background_layers,
        ) {
            pseudo_box.additional_bg_images = existing.additional_bg_images.clone();
        }
        if pseudo_box.style.rare().mask_image_url == existing.style.rare().mask_image_url
            && pseudo_box.style.rare().additional_mask_images
                == existing.style.rare().additional_mask_images
        {
            pseudo_box.mask_images = existing.mask_images.clone();
        }
    }

    let is_grid_or_flex = matches!(
        root.style.display,
        Display::Grid | Display::InlineGrid | Display::Flex | Display::InlineFlex
    );
    let has_block_children = (root.style.before_style.is_some()
        || root.style.after_style.is_some())
        && crate::layout::inline_layout::has_in_flow_block_children(root);
    let pseudo_needs_inline_box = |style: Option<&Box<ComputedStyle>>| {
        style.as_ref().is_some_and(|ps| {
            matches!(
                ps.display,
                Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
            )
        })
    };
    let pseudo_needs_block_box =
        |style: Option<&Box<ComputedStyle>>| style.as_ref().is_some_and(|ps| ps.is_block_level());
    let before_is_positioned = root.style.before_style.as_ref().map_or(false, |ps| {
        matches!(ps.position, Position::Absolute | Position::Fixed)
    });
    let before_is_block = pseudo_needs_block_box(root.style.before_style.as_ref());
    // `before_style` is Some only when `content` generated the pseudo-element,
    // so it — not the generated TEXT, which is empty for `content: ""` — is
    // what says the box may exist.
    let before_generated = root.style.before_style.is_some();
    let before_is_atomic_inline = pseudo_needs_inline_box(root.style.before_style.as_ref());
    if before_generated
        && (is_grid_or_flex
            || has_block_children
            || before_is_positioned
            || before_is_block
            || before_is_atomic_inline)
    {
        let existing = root.children.iter().position(|c| c.tag == "::before");
        let existing_node = existing.and_then(|idx| root.children.get(idx));
        let existing_node_id = existing_node.map(|node| node.node_id).filter(|id| *id != 0);
        let mut pseudo_box = crate::types::WebCore::new("::before");
        pseudo_box.node_id =
            existing_node_id.unwrap_or_else(crate::dom::arena::next_shadow_node_id);
        pseudo_box.text = root.style.before_content.clone();
        pseudo_box.tag = "::before".to_string();
        if let Some(ref ps) = root.style.before_style {
            pseudo_box.style = std::sync::Arc::new(*ps.clone());
        }
        if matches!(
            pseudo_box.style.position,
            Position::Absolute | Position::Fixed
        ) {
            blockify_out_of_flow(std::sync::Arc::make_mut(&mut pseudo_box.style));
        }
        if is_grid_or_flex
            && !pseudo_box.style.is_positioned()
            && matches!(pseudo_box.style.display, Display::Inline)
        {
            std::sync::Arc::make_mut(&mut pseudo_box.style).display = Display::Block;
        }
        wrap_layout_item_pseudo_text(&mut pseudo_box);
        preserve_loaded_pseudo_resources(&mut pseudo_box, existing_node);
        mark_pseudo_layout_dirty(&mut pseudo_box);
        if let Some(idx) = existing {
            root.children.remove(idx);
        }
        root.children.insert(0, pseudo_box);
        mark_pseudo_layout_dirty(root);
        std::sync::Arc::make_mut(&mut root.style).before_content = String::new();
    } else {
        if let Some(idx) = root.children.iter().position(|c| c.tag == "::before") {
            root.children.remove(idx);
            mark_pseudo_layout_dirty(root);
        }
    }
    let after_is_positioned = root.style.after_style.as_ref().map_or(false, |ps| {
        matches!(ps.position, Position::Absolute | Position::Fixed)
    });
    let after_is_block = pseudo_needs_block_box(root.style.after_style.as_ref());
    let after_generated = root.style.after_style.is_some();
    let after_is_atomic_inline = pseudo_needs_inline_box(root.style.after_style.as_ref());
    if after_generated
        && (is_grid_or_flex
            || has_block_children
            || after_is_positioned
            || after_is_block
            || after_is_atomic_inline)
    {
        let existing = root.children.iter().position(|c| c.tag == "::after");
        let existing_node = existing.and_then(|idx| root.children.get(idx));
        let existing_node_id = existing_node.map(|node| node.node_id).filter(|id| *id != 0);
        let mut pseudo_box = crate::types::WebCore::new("::after");
        pseudo_box.node_id =
            existing_node_id.unwrap_or_else(crate::dom::arena::next_shadow_node_id);
        pseudo_box.text = root.style.after_content.clone();
        pseudo_box.tag = "::after".to_string();
        if let Some(ref ps) = root.style.after_style {
            pseudo_box.style = std::sync::Arc::new(*ps.clone());
        }
        if matches!(
            pseudo_box.style.position,
            Position::Absolute | Position::Fixed
        ) {
            blockify_out_of_flow(std::sync::Arc::make_mut(&mut pseudo_box.style));
        }
        if is_grid_or_flex
            && !pseudo_box.style.is_positioned()
            && matches!(pseudo_box.style.display, Display::Inline)
        {
            std::sync::Arc::make_mut(&mut pseudo_box.style).display = Display::Block;
        }
        wrap_layout_item_pseudo_text(&mut pseudo_box);
        preserve_loaded_pseudo_resources(&mut pseudo_box, existing_node);
        mark_pseudo_layout_dirty(&mut pseudo_box);
        if let Some(idx) = existing {
            root.children.remove(idx);
        }
        root.children.push(pseudo_box);
        mark_pseudo_layout_dirty(root);
        std::sync::Arc::make_mut(&mut root.style).after_content = String::new();
    } else {
        if let Some(idx) = root.children.iter().position(|c| c.tag == "::after") {
            root.children.remove(idx);
            mark_pseudo_layout_dirty(root);
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
pub(crate) struct ShareCache {
    styles: HashMap<(usize, String, String), std::sync::Arc<ComputedStyle>>,
    variable_scopes: HashMap<(usize, u64), Vec<VariableScopeEntry>>,
    variable_scope_count: usize,
}

struct VariableScopeEntry {
    declarations: Vec<OwnedCustomDeclaration>,
    _parent_scope: std::sync::Arc<HashMap<String, String>>,
    scope: std::sync::Arc<HashMap<String, String>>,
}

#[derive(PartialEq, Eq)]
struct OwnedCustomDeclaration {
    name: String,
    value: String,
    author_origin: bool,
    layer_rank: u32,
    inline: bool,
    important: bool,
}

impl OwnedCustomDeclaration {
    fn from_borrowed(declaration: &CustomDeclaration<'_>) -> Self {
        Self {
            name: declaration.name.to_owned(),
            value: declaration.value.to_owned(),
            author_origin: declaration.author_origin,
            layer_rank: declaration.layer_rank,
            inline: declaration.inline,
            important: declaration.important,
        }
    }

    fn matches(&self, declaration: &CustomDeclaration<'_>) -> bool {
        self.name == declaration.name
            && self.value == declaration.value
            && self.author_origin == declaration.author_origin
            && self.layer_rank == declaration.layer_rank
            && self.inline == declaration.inline
            && self.important == declaration.important
    }
}

impl ShareCache {
    pub(crate) fn new() -> Self {
        Self {
            styles: HashMap::new(),
            variable_scopes: HashMap::new(),
            variable_scope_count: 0,
        }
    }

    fn get(&self, key: &(usize, String, String)) -> Option<&std::sync::Arc<ComputedStyle>> {
        self.styles.get(key)
    }

    fn contains_key(&self, key: &(usize, String, String)) -> bool {
        self.styles.contains_key(key)
    }

    fn insert(&mut self, key: (usize, String, String), style: std::sync::Arc<ComputedStyle>) {
        self.styles.insert(key, style);
    }

    fn variable_scope_key(
        inherited: &HashMap<String, String>,
        declarations: &[CustomDeclaration<'_>],
    ) -> (usize, u64) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        declarations.hash(&mut hasher);
        (inherited as *const _ as usize, hasher.finish())
    }

    fn find_variable_scope(
        &self,
        key: (usize, u64),
        declarations: &[CustomDeclaration<'_>],
    ) -> Option<std::sync::Arc<HashMap<String, String>>> {
        self.variable_scopes.get(&key)?.iter().find_map(|entry| {
            (entry.declarations.len() == declarations.len()
                && entry
                    .declarations
                    .iter()
                    .zip(declarations)
                    .all(|(owned, borrowed)| owned.matches(borrowed)))
            .then(|| entry.scope.clone())
        })
    }

    fn insert_variable_scope(
        &mut self,
        key: (usize, u64),
        declarations: &[CustomDeclaration<'_>],
        parent_scope: std::sync::Arc<HashMap<String, String>>,
        scope: std::sync::Arc<HashMap<String, String>>,
    ) {
        const MAX_SCOPES: usize = 4096;
        if self.variable_scope_count >= MAX_SCOPES {
            return;
        }
        self.variable_scopes
            .entry(key)
            .or_default()
            .push(VariableScopeEntry {
                declarations: declarations
                    .iter()
                    .map(OwnedCustomDeclaration::from_borrowed)
                    .collect(),
                _parent_scope: parent_scope,
                scope,
            });
        self.variable_scope_count += 1;
    }
}

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
    pub file_selector_button_matched: Vec<(u32, usize, Option<u32>)>,
    pub details_content_matched: Vec<(u32, usize, Option<u32>)>,
    pub spelling_error_matched: Vec<(u32, usize, Option<u32>)>,
    pub grammar_error_matched: Vec<(u32, usize, Option<u32>)>,
    pub first_line_matched: Vec<(u32, usize, Option<u32>)>,
    pub first_letter_matched: Vec<(u32, usize, Option<u32>)>,
}

/// Precomputed match results, keyed by `node_id`.
///
/// ⛔ `node_id`, not a path through `children`. A path is invalidated the moment
/// `build_pseudo_element_boxes` inserts a `::before` at index 0 during the apply
/// walk: every later sibling then reads its neighbour's rules, and the last one
/// reads none at all. `node_id` is stable across that insertion.
pub(crate) type MatchMap = HashMap<u32, MatchSets>;

/// Debug-only view of what the cascade matcher sees for one live element.
///
/// This deliberately calls the same `match_rules` entry point used by both the
/// serial and parallel cascades, so browser inspection can distinguish "selector
/// matching failed" from "the matched declarations were not applied" without
/// reimplementing selector behavior in the example app.
#[derive(Clone, Debug, Default)]
pub struct CssMatchDebugReport {
    pub node_id: u32,
    pub tag: String,
    pub id: String,
    pub class_attr: String,
    pub candidate_rule_indices: Vec<usize>,
    pub matched_rule_indices: Vec<usize>,
    pub hover_rule_indices: Vec<usize>,
    pub active_rule_indices: Vec<usize>,
    pub visited_rule_indices: Vec<usize>,
    pub before_rule_indices: Vec<usize>,
    pub after_rule_indices: Vec<usize>,
}

/// Run the normal cascade matcher against the element with `target_node_id`.
pub fn debug_match_report_for_node(
    root: &crate::types::WebCore,
    stylesheet: &Stylesheet,
    target_node_id: u32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    fragment_target_id: u32,
    document_url: &str,
) -> Option<CssMatchDebugReport> {
    fn walk(
        node: &crate::types::WebCore,
        stylesheet: &Stylesheet,
        target_node_id: u32,
        vw: f32,
        vh: f32,
        focused_box: u32,
        keyboard_focus: bool,
        hover_chain: &std::collections::HashSet<u32>,
        focus_within_chain: &std::collections::HashSet<u32>,
        fragment_target_id: u32,
        document_url: &str,
        ancestors: &mut Vec<AncestorInfo>,
        child_index: usize,
        sibling_count: usize,
        type_child_index: usize,
        type_sibling_count: usize,
        prev_siblings: &[SiblingInfo],
        next_siblings: &[SiblingInfo],
        next_sibling_nodes: &[&crate::types::WebCore],
    ) -> Option<CssMatchDebugReport> {
        if node.node_id == target_node_id {
            let id = node.attributes.get("id").map(|s| s.as_str());
            let class_attr = node
                .attributes
                .get("class")
                .map(|s| s.as_str())
                .unwrap_or("");
            let mut candidates = Vec::new();
            stylesheet.candidate_rules(
                &node.tag,
                id,
                class_attr.split_whitespace(),
                &mut candidates,
            );
            let mut scratch = Vec::new();
            let sets = match_rules(
                node,
                stylesheet,
                ancestors,
                &[],
                child_index,
                sibling_count,
                type_child_index,
                type_sibling_count,
                vw,
                vh,
                focused_box,
                keyboard_focus,
                hover_chain,
                focus_within_chain,
                fragment_target_id,
                document_url,
                prev_siblings,
                next_siblings,
                next_sibling_nodes,
                &mut scratch,
                None,
            );
            return Some(CssMatchDebugReport {
                node_id: node.node_id,
                tag: node.tag.clone(),
                id: id.unwrap_or("").to_string(),
                class_attr: class_attr.to_string(),
                candidate_rule_indices: candidates,
                matched_rule_indices: sets.matched.iter().map(|(_, idx, _)| *idx).collect(),
                hover_rule_indices: sets.hover_matched.iter().map(|(_, idx, _)| *idx).collect(),
                active_rule_indices: sets.active_matched.iter().map(|(_, idx, _)| *idx).collect(),
                visited_rule_indices: sets
                    .visited_matched
                    .iter()
                    .map(|(_, idx, _)| *idx)
                    .collect(),
                before_rule_indices: sets.before_matched.iter().map(|(_, idx, _)| *idx).collect(),
                after_rule_indices: sets.after_matched.iter().map(|(_, idx, _)| *idx).collect(),
            });
        }
        if ancestors.len() >= MAX_CASCADE_DEPTH || !node.is_element() {
            return None;
        }

        ancestors.push(AncestorInfo {
            tag: node.tag.clone(),
            attributes: std::sync::Arc::new(node.attributes.clone()),
            auto_direction: super::matching::auto_direction(node),
            child_index,
            sibling_count,
            type_child_index,
            type_sibling_count,
            node_id: node.node_id,
            prev_siblings: std::sync::Arc::new(prev_siblings.to_vec()),
        });

        let children = node.effective_children();
        let n_children = children.len();
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
        let child_siblings = children
            .iter()
            .filter(|c| c.is_element())
            .map(SiblingInfo::from_node)
            .collect::<Vec<_>>();
        let child_nodes = children.iter().collect::<Vec<_>>();
        for (i, child) in children.iter().enumerate() {
            let (ci, ns) = if !child.is_element() {
                (i, n_children)
            } else {
                (elem_indices[i], n_elem_children)
            };
            let prev_elem = if child.is_element() {
                &child_siblings[..elem_indices[i]]
            } else {
                &[]
            };
            let next_elem = if child.is_element() {
                &child_siblings[elem_indices[i].saturating_add(1)..]
            } else {
                &[]
            };
            let next_nodes = if i + 1 < child_nodes.len() {
                &child_nodes[i + 1..]
            } else {
                &[]
            };
            if let Some(report) = walk(
                child,
                stylesheet,
                target_node_id,
                vw,
                vh,
                focused_box,
                keyboard_focus,
                hover_chain,
                focus_within_chain,
                fragment_target_id,
                document_url,
                ancestors,
                ci,
                ns,
                type_counts[i],
                type_totals[i],
                prev_elem,
                next_elem,
                next_nodes,
            ) {
                ancestors.pop();
                return Some(report);
            }
        }
        ancestors.pop();
        None
    }

    let mut ancestors = Vec::new();
    let focus_within_chain = crate::css::build_hover_chain(root, focused_box);
    walk(
        root,
        stylesheet,
        target_node_id,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        hover_chain,
        &focus_within_chain,
        fragment_target_id,
        document_url,
        &mut ancestors,
        0,
        1,
        0,
        1,
        &[],
        &[],
        &[],
    )
}

/// Run the selectors of `stylesheet` against one element.
///
/// The only place a selector is tested during a cascade. `candidates_buf` is a
/// scratch Vec the caller owns so the walk allocates once, not once per node.
pub(crate) fn match_rules(
    node: &crate::types::WebCore,
    stylesheet: &Stylesheet,
    ancestors: &[AncestorInfo],
    ancestor_nodes: &[&crate::types::WebCore],
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
    prev_siblings: &[SiblingInfo],
    next_siblings: &[SiblingInfo],
    next_sibling_nodes: &[&crate::types::WebCore],
    candidates_buf: &mut Vec<usize>,
    media_matches: Option<&[bool]>,
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
        ancestor_nodes,
        hover_chain,
        focus_within_chain,
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
    stylesheet.candidate_rules(&node.tag, id, class_attr.split_whitespace(), candidates_buf);

    for &rule_idx in candidates_buf.iter() {
        let rule = &stylesheet.rules[rule_idx];
        // Rules whose @media condition does not match the viewport are not in
        // the cascade at all.
        if !media_matches.map_or_else(
            || rule.media_condition.matches(vw, vh),
            |matches| matches[rule_idx],
        ) {
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
                    PseudoElement::FileSelectorButton => sets.file_selector_button_matched.push((
                        rule.specificity,
                        rule_idx,
                        scope_proximity,
                    )),
                    PseudoElement::DetailsContent => sets.details_content_matched.push((
                        rule.specificity,
                        rule_idx,
                        scope_proximity,
                    )),
                    PseudoElement::SpellingError => sets.spelling_error_matched.push((
                        rule.specificity,
                        rule_idx,
                        scope_proximity,
                    )),
                    PseudoElement::GrammarError => sets.grammar_error_matched.push((
                        rule.specificity,
                        rule_idx,
                        scope_proximity,
                    )),
                    PseudoElement::FirstLine => {
                        sets.first_line_matched
                            .push((rule.specificity, rule_idx, scope_proximity))
                    }
                    PseudoElement::FirstLetter => sets.first_letter_matched.push((
                        rule.specificity,
                        rule_idx,
                        scope_proximity,
                    )),
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
        ancestor_nodes: &[],
        hover_chain: match_ctx.hover_chain,
        focus_within_chain: match_ctx.focus_within_chain,
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

fn layout_affecting_style_changed(old: &ComputedStyle, new: &ComputedStyle) -> bool {
    old.display != new.display
        || old.position != new.position
        || old.float != new.float
        || old.clear != new.clear
        || old.box_sizing != new.box_sizing
        || old.width != new.width
        || old.height != new.height
        || old.min_width != new.min_width
        || old.max_width != new.max_width
        || old.min_height != new.min_height
        || old.max_height != new.max_height
        || old.margin_top != new.margin_top
        || old.margin_right != new.margin_right
        || old.margin_bottom != new.margin_bottom
        || old.margin_left != new.margin_left
        || old.padding_top != new.padding_top
        || old.padding_right != new.padding_right
        || old.padding_bottom != new.padding_bottom
        || old.padding_left != new.padding_left
        || old.border_top_width != new.border_top_width
        || old.border_right_width != new.border_right_width
        || old.border_bottom_width != new.border_bottom_width
        || old.border_left_width != new.border_left_width
        || old.border_top_style != new.border_top_style
        || old.border_right_style != new.border_right_style
        || old.border_bottom_style != new.border_bottom_style
        || old.border_left_style != new.border_left_style
        || old.top != new.top
        || old.right != new.right
        || old.bottom != new.bottom
        || old.left != new.left
        || old.font_family != new.font_family
        || old.font_size != new.font_size
        || old.font_weight != new.font_weight
        || old.font_style != new.font_style
        || old.line_height != new.line_height
        || old.letter_spacing != new.letter_spacing
        || old.word_spacing != new.word_spacing
        || old.text_align != new.text_align
        || old.vertical_align != new.vertical_align
        || old.text_indent != new.text_indent
        || old.white_space != new.white_space
        || old.text_transform != new.text_transform
        || old.word_break != new.word_break
        || old.overflow_wrap != new.overflow_wrap
        || old.direction != new.direction
        || old.flex_direction != new.flex_direction
        || old.flex_wrap != new.flex_wrap
        || old.justify_content != new.justify_content
        || old.align_items != new.align_items
        || old.align_self != new.align_self
        || old.align_content != new.align_content
        || old.flex_grow != new.flex_grow
        || old.flex_shrink != new.flex_shrink
        || old.flex_basis != new.flex_basis
        || old.order != new.order
        || old.gap != new.gap
        || old.row_gap != new.row_gap
        || old.column_gap != new.column_gap
        || old.grid_column_start != new.grid_column_start
        || old.grid_column_end != new.grid_column_end
        || old.grid_row_start != new.grid_row_start
        || old.grid_row_end != new.grid_row_end
        || old.justify_items != new.justify_items
        || old.justify_self != new.justify_self
        || old.visibility != new.visibility
}

pub(crate) fn mark_layout_subtree_dirty(node: &mut crate::types::WebCore) {
    node.layout.layout_dirty = true;
    node.layout.intrinsic_dirty = true;
    node.layout.line_cache.clear();
    node.layout.cached_intrinsic_w.set(f32::NAN);
    node.has_dirty_layout_descendant = true;
    for child in &mut node.children {
        mark_layout_subtree_dirty(child);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            mark_layout_subtree_dirty(child);
        }
    }
}

fn tree_has_duplicate_node_ids(root: &crate::types::WebCore) -> bool {
    fn walk(node: &crate::types::WebCore, seen: &mut HashSet<u32>) -> bool {
        if node.node_id != 0 && !seen.insert(node.node_id) {
            return true;
        }
        for child in &node.children {
            if walk(child, seen) {
                return true;
            }
        }
        if let Some(shadow) = node.shadow_root.as_ref() {
            for child in &shadow.children {
                if walk(child, seen) {
                    return true;
                }
            }
        }
        false
    }

    let mut seen = HashSet::new();
    walk(root, &mut seen)
}

#[derive(Default)]
struct AutomaticCounterInitial {
    total: i64,
    last_increment_negated: i64,
    stopped: bool,
}

#[derive(Clone, Copy, Default)]
struct CounterScope {
    reversed: bool,
    automatic: Option<usize>,
}

#[derive(Default)]
pub(crate) struct CounterState {
    values: HashMap<String, Vec<i32>>,
    // Outer instances remain readable; only instances above this depth may
    // be changed by descendants of a style-containment boundary.
    boundaries: Vec<HashMap<String, usize>>,
    sibling_scopes: Vec<HashMap<String, CounterScope>>,
    collecting: bool,
    next_automatic: usize,
    initial_values: Vec<i32>,
    automatic: Vec<AutomaticCounterInitial>,
}

impl CounterState {
    fn enter_containment(&mut self) {
        self.boundaries.push(
            self.values
                .iter()
                .map(|(name, stack)| (name.clone(), stack.len()))
                .collect(),
        );
    }

    fn exit_containment(&mut self) {
        self.boundaries.pop().expect("balanced counter containment");
    }

    fn ensure_scope(&mut self) {
        if self.sibling_scopes.is_empty() {
            self.sibling_scopes.push(HashMap::new());
        }
    }

    fn enter_children(&mut self) {
        self.ensure_scope();
        self.sibling_scopes.push(HashMap::new());
    }

    fn exit_children(&mut self) {
        let names = self.sibling_scopes.pop().expect("balanced counter scope");
        for (name, _) in names {
            if let Some(stack) = self.values.get_mut(&name) {
                stack.pop();
                if stack.is_empty() {
                    self.values.remove(&name);
                }
            }
        }
    }

    fn reset(&mut self, name: &str, value: Option<i32>, reversed: bool) {
        self.ensure_scope();
        let automatic = value.is_none().then(|| {
            let id = self.next_automatic;
            self.next_automatic += 1;
            if self.collecting {
                self.automatic.push(AutomaticCounterInitial::default());
            }
            id
        });
        let value = value.unwrap_or_else(|| {
            automatic
                .and_then(|id| self.initial_values.get(id).copied())
                .unwrap_or(0)
        });
        let fresh = self
            .sibling_scopes
            .last_mut()
            .unwrap()
            .insert(
                name.to_owned(),
                CounterScope {
                    reversed,
                    automatic,
                },
            )
            .is_none();
        let stack = self.values.entry(name.to_owned()).or_default();
        // Same-level resets replace a sibling's instance, not an ancestor's.
        if !fresh {
            stack.pop();
        }
        stack.push(value);
    }

    fn writable_counter(&mut self, name: &str) -> (&mut i32, bool) {
        self.ensure_scope();
        let boundary_depth = self
            .boundaries
            .last()
            .and_then(|b| b.get(name))
            .copied()
            .unwrap_or(0);
        let stack = self.values.entry(name.to_owned()).or_default();
        let created = stack.len() <= boundary_depth;
        if created {
            stack.push(0);
            self.sibling_scopes
                .last_mut()
                .unwrap()
                .insert(name.to_owned(), CounterScope::default());
        }
        (stack.last_mut().expect("counter instantiated"), created)
    }

    fn record_initial_operation(&mut self, name: &str, increment: i64, set: Option<i32>) {
        if !self.collecting {
            return;
        }
        let Some(id) = self
            .sibling_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .and_then(|scope| scope.automatic)
        else {
            return;
        };
        let initial = &mut self.automatic[id];
        if initial.stopped {
            return;
        }
        if increment != 0 {
            initial.last_increment_negated = -increment;
        }
        if let Some(value) = set {
            initial.total = initial.total.saturating_add(i64::from(value));
            initial.stopped = true;
        } else {
            initial.total = initial.total.saturating_sub(increment);
        }
    }

    fn apply_element(&mut self, style: &ComputedStyle) {
        self.apply_element_with_list_start(style, None);
    }

    fn apply_element_with_list_start(
        &mut self,
        style: &ComputedStyle,
        html_list_start: Option<i32>,
    ) {
        // Published CSS Lists 3 inheritance: parent counters take priority over
        // same-name instances from a sibling. Containment-local instances are
        // the writable scope root, so they continue through that subtree.
        if let Some(scope) = self.sibling_scopes.last_mut() {
            scope.retain(|name, _| {
                let Some(stack) = self.values.get_mut(name) else {
                    return false;
                };
                let contained_root = self
                    .boundaries
                    .last()
                    .and_then(|b| b.get(name))
                    .is_some_and(|depth| *depth + 1 == stack.len());
                if stack.len() > 1 && !contained_root {
                    stack.pop();
                    false
                } else {
                    true
                }
            });
        }
        for reset in &style.counter_reset {
            let value = if reset.html_list_start {
                html_list_start.or(reset.value)
            } else {
                reset.value
            };
            self.reset(&reset.name, value, reset.reversed);
        }
        for (name, delta) in &style.counter_increment {
            let (value, _) = self.writable_counter(name);
            *value = value.saturating_add(*delta);
        }
        if self.collecting {
            for (index, (name, _)) in style.counter_increment.iter().enumerate() {
                if style.counter_increment[..index]
                    .iter()
                    .any(|(previous, _)| previous == name)
                {
                    continue;
                }
                let increment = style
                    .counter_increment
                    .iter()
                    .filter(|(other, _)| other == name)
                    .fold(0i64, |sum, (_, delta)| {
                        sum.saturating_add(i64::from(*delta))
                    });
                let set = style
                    .counter_set
                    .iter()
                    .rev()
                    .find(|(other, _)| other == name)
                    .map(|(_, value)| *value);
                self.record_initial_operation(name, increment, set);
            }
        }
        if style.display == Display::ListItem
            && !style
                .counter_increment
                .iter()
                .any(|(name, _)| name == "list-item")
        {
            let reversed = self
                .sibling_scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get("list-item"))
                .is_some_and(|scope| scope.reversed);
            let (value, created) = self.writable_counter("list-item");
            let increment = if reversed && !created { -1 } else { 1 };
            *value = value.saturating_add(increment);
            let set = style
                .counter_set
                .iter()
                .rev()
                .find(|(name, _)| name == "list-item")
                .map(|(_, value)| *value);
            self.record_initial_operation("list-item", i64::from(increment), set);
        }
        for (name, new_value) in &style.counter_set {
            *self.writable_counter(name).0 = *new_value;
        }
        if self.collecting {
            for (index, (name, value)) in style.counter_set.iter().enumerate() {
                if style.counter_set[index + 1..]
                    .iter()
                    .any(|(other, _)| other == name)
                    || style
                        .counter_increment
                        .iter()
                        .any(|(other, _)| other == name)
                    || (name == "list-item" && style.display == Display::ListItem)
                {
                    continue;
                }
                self.record_initial_operation(name, 0, Some(*value));
            }
        }
    }
}

struct CascadedNodeState {
    root_font_px: f32,
    local_vars: Option<std::sync::Arc<HashMap<String, String>>>,
    counter_containment: bool,
    pending_after: Option<String>,
}

// Keep the per-element style temporaries out of the recursive traversal frame.
#[inline(never)]
fn apply_cascade_node(
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
    counters: &mut CounterState,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    prev_siblings: &[SiblingInfo],
    next_siblings: &[SiblingInfo],
    next_sibling_nodes: &[&crate::types::WebCore],
    share_cache: &mut ShareCache,
    // Selector matches computed off-thread by the parallel pass, keyed by
    // `node_id`. `None`, or a miss, means match inline — never "no rules".
    precomputed: Option<&mut MatchMap>,
) -> Option<CascadedNodeState> {
    // Guard against stack overflow on deeply nested DOMs.
    if ancestors.len() >= MAX_CASCADE_DEPTH {
        // Just inherit from parent and stop — the page may render slightly wrong
        // at extreme depth, but won't crash.
        if let Some(p) = parent_style {
            std::sync::Arc::make_mut(&mut root.style).inherit_from(p);
        }
        return None;
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
        return None;
    }

    if root.parser_suppressed {
        std::sync::Arc::make_mut(&mut root.style).display = Display::None;
        return None;
    }

    // Synthetic ::before/::after children already have their computed pseudo
    // style set from the originating element. Recascading them as normal
    // children would overwrite pseudo-specific font/color/content rules.
    if root.tag == "::before" || root.tag == "::after" {
        return None;
    }

    let profile_style_started = crate::profile::is_enabled().then(std::time::Instant::now);

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
    if let Some(started) = profile_style_started {
        crate::profile::record(crate::profile::Phase::CascadeApplyInit, started.elapsed());
    }
    let profile_matches_started = crate::profile::is_enabled().then(std::time::Instant::now);
    // Selector matching — the SAME function the parallel pass runs, so a
    // precomputed result and an inline one can never disagree.
    let precomputed_here = precomputed
        .filter(|_| root.node_id != 0)
        .and_then(|m| m.remove(&root.node_id));
    let sets = match precomputed_here {
        Some(sets) => sets,
        // ⛔ A miss MATCHES, it does not mean "no rules". An element the parallel
        // pass never saw — a box with no DOM node behind it, or a shadow subtree,
        // which is matched against its own scoped sheet — has to be cascaded, and
        // handing it an empty result renders it unstyled with nothing to show for it.
        None => match_rules(
            root,
            stylesheet,
            ancestors,
            &[],
            child_index,
            sibling_count,
            type_child_index,
            type_sibling_count,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            hover_chain,
            focus_within_chain,
            target_id,
            document_url,
            prev_siblings,
            next_siblings,
            next_sibling_nodes,
            candidates_buf,
            None,
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
        mut file_selector_button_matched,
        mut details_content_matched,
        mut spelling_error_matched,
        mut grammar_error_matched,
        mut first_line_matched,
        mut first_letter_matched,
    } = sets;
    matched.sort_by(|&a, &b| normal_cascade_cmp(&stylesheet.rules, a, b));
    if let Some(started) = profile_matches_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyMatches,
            started.elapsed(),
        );
    }
    let profile_variables_started = crate::profile::is_enabled().then(std::time::Instant::now);
    // Build variable scope: inherited from parent + any --custom-properties from matched rules.
    // Only clone the map when new custom properties are actually defined — most elements
    // don't define any, so we avoid O(vars) cloning at every node.
    let mut has_new_vars = false;
    let mut has_revert_rule = false;
    let mut has_var_ref_rule = false;
    for &(_, ri, _) in &matched {
        let rule = &stylesheet.rules[ri];
        has_new_vars |= rule.has_custom_properties;
        has_revert_rule |= rule.has_revert_value;
        has_var_ref_rule |= rule.has_var_refs;
    }
    // Also check inline style for custom properties — these must be available
    // during var() resolution of stylesheet rules on the same element.
    let profile_inline_parse_started = crate::profile::is_enabled().then(std::time::Instant::now);
    let inline_decls = root
        .attributes
        .get("style")
        .map(|s| parse_declarations_important(s));
    if let Some(started) = profile_inline_parse_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyInlineParse,
            started.elapsed(),
        );
    }
    let has_inline_vars = inline_decls.as_ref().is_some_and(|(normal, important)| {
        normal
            .keys()
            .chain(important.keys())
            .any(|p| p.starts_with("--"))
    });

    let local_vars_owned = if has_new_vars || has_inline_vars {
        let mut declarations = Vec::new();
        for &(sp, ri, _) in &matched {
            let rule = &stylesheet.rules[ri];
            for (prop, val) in &rule.declarations {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::rule(prop, val, rule, sp, false));
                }
            }
        }
        if let Some((normal, _)) = &inline_decls {
            for (prop, val) in normal {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::inline(prop, val, false));
                }
            }
        }
        let mut important_custom_rules: Vec<_> = matched
            .iter()
            .copied()
            .filter(|&(_, ri, _)| {
                stylesheet.rules[ri]
                    .important_declarations
                    .keys()
                    .any(|prop| prop.starts_with("--"))
            })
            .collect();
        important_custom_rules.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
        for &(sp, ri, _) in &important_custom_rules {
            if !is_author_origin(sp) {
                continue;
            }
            for (prop, val) in &stylesheet.rules[ri].important_declarations {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::rule(
                        prop,
                        val,
                        &stylesheet.rules[ri],
                        sp,
                        true,
                    ));
                }
            }
        }
        if let Some((_, important)) = &inline_decls {
            for (prop, val) in important {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::inline(prop, val, true));
                }
            }
        }
        for &(sp, ri, _) in &important_custom_rules {
            if is_author_origin(sp) {
                continue;
            }
            for (prop, val) in &stylesheet.rules[ri].important_declarations {
                if prop.starts_with("--") {
                    declarations.push(CustomDeclaration::rule(
                        prop,
                        val,
                        &stylesheet.rules[ri],
                        sp,
                        true,
                    ));
                }
            }
        }
        let inherited_scope = parent_style
            .map(|parent| &parent.custom_props)
            .filter(|scope| std::ptr::eq(std::sync::Arc::as_ref(*scope), inherited_vars));
        let scope_key =
            inherited_scope.map(|_| ShareCache::variable_scope_key(inherited_vars, &declarations));
        if declarations.iter().all(|declaration| {
            !declaration.value.contains('(')
                && inherited_vars
                    .get(declaration.name)
                    .is_some_and(|inherited| inherited == declaration.value)
        }) {
            None
        } else if let Some(scope) =
            scope_key.and_then(|key| share_cache.find_variable_scope(key, &declarations))
        {
            Some(scope)
        } else {
            let profile_var_clone_started =
                crate::profile::is_enabled().then(std::time::Instant::now);
            let mut vars = inherited_vars.clone();
            if let Some(started) = profile_var_clone_started {
                crate::profile::record(
                    crate::profile::Phase::CascadeApplyVarClone,
                    started.elapsed(),
                );
            }
            let profile_var_resolve_started =
                crate::profile::is_enabled().then(std::time::Instant::now);
            resolve_cascaded_custom_properties(&declarations, inherited_vars, &mut vars);
            if let Some(started) = profile_var_resolve_started {
                crate::profile::record(
                    crate::profile::Phase::CascadeApplyVarResolve,
                    started.elapsed(),
                );
            }
            let scope = std::sync::Arc::new(vars);
            if let (Some(key), Some(parent_scope)) = (scope_key, inherited_scope) {
                share_cache.insert_variable_scope(
                    key,
                    &declarations,
                    parent_scope.clone(),
                    scope.clone(),
                );
            }
            Some(scope)
        }
    } else {
        None
    };
    let local_vars: &HashMap<String, String> =
        local_vars_owned.as_deref().unwrap_or(inherited_vars);
    if let Some(started) = profile_variables_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyVarScope,
            started.elapsed(),
        );
    }
    let profile_var_checks_started = crate::profile::is_enabled().then(std::time::Instant::now);
    // Track properties whose highest-specificity declaration is `inherit`.
    // After all rules are applied, these properties are reset to the parent's value.
    let mut inherit_props: HashSet<String> = HashSet::new();
    let has_vars = !local_vars.is_empty();
    let needs_revert_snapshot = has_revert_rule
        || inline_decls.as_ref().is_some_and(|(normal, important)| {
            normal
                .values()
                .chain(important.values())
                .any(|value| value_mentions_revert(value))
        })
        || (has_vars
            && has_var_ref_rule
            && local_vars
                .values()
                .any(|value| value_mentions_revert(value)));
    let mut pre_author_normal_style: Option<ComputedStyle> = None;
    let mut current_normal_layer: Option<(bool, u32)> = None;
    let mut normal_layer_start_style = needs_revert_snapshot.then(|| style.clone());
    let mut hints_applied = false;
    if let Some(started) = profile_var_checks_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyVarChecks,
            started.elapsed(),
        );
    }
    let profile_color_scheme_started = crate::profile::is_enabled().then(std::time::Instant::now);
    let normal_color_scheme =
        prescan_color_scheme(&style, &stylesheet.rules, &matched, local_vars, false, None);
    style.color_scheme = normal_color_scheme;
    if let Some(started) = profile_color_scheme_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyColorScheme,
            started.elapsed(),
        );
    }
    if let Some(started) = profile_variables_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyVariables,
            started.elapsed(),
        );
    }
    if let Some(started) = profile_style_started {
        crate::profile::record(crate::profile::Phase::CascadeApplySetup, started.elapsed());
    }
    let profile_rules_started = crate::profile::is_enabled().then(std::time::Instant::now);
    for &(sp, ri, _) in &matched {
        if is_author_origin(sp) && !hints_applied {
            apply_presentational_hints(&mut style, root, ancestors);
            hints_applied = true;
            pre_author_normal_style = needs_revert_snapshot.then(|| style.clone());
        }
        let rule = &stylesheet.rules[ri];
        let layer_key = (is_author_origin(sp), rule.layer_rank);
        if current_normal_layer != Some(layer_key) {
            current_normal_layer = Some(layer_key);
            if needs_revert_snapshot {
                normal_layer_start_style = Some(style.clone());
            }
        }
        let revert_base = if is_author_origin(sp) {
            pre_author_normal_style.as_ref()
        } else {
            None
        };
        let revert_layer_base = normal_layer_start_style
            .as_ref()
            .unwrap_or(root.style.as_ref());
        // Fast path: use pre-compiled declarations (PropertyId dispatch, no string matching).
        // Only fall back to raw declarations when var() resolution is needed.
        if has_vars && rule.has_var_refs {
            if let Some(val) = rule.declarations.get("color-scheme") {
                let resolved = if value_needs_substitution(val) {
                    std::borrow::Cow::Owned(resolve_var_references_for_color_scheme(
                        val,
                        local_vars,
                        &style.color_scheme,
                    ))
                } else {
                    std::borrow::Cow::Borrowed(val.as_str())
                };
                if !resolved.trim().is_empty() && !contains_var_function(&resolved) {
                    clear_inherit_tracking_for_property(&mut inherit_props, "color-scheme");
                    apply_property(&mut style, "color-scheme", &resolved);
                }
            }
            // Slow path: var() references need string-based resolution
            for (prop, val) in &rule.declarations {
                if prop.starts_with("--") {
                    continue;
                }
                let resolved = if value_needs_substitution(val) {
                    std::borrow::Cow::Owned(resolve_var_references_for_color_scheme(
                        val,
                        local_vars,
                        &style.color_scheme,
                    ))
                } else {
                    std::borrow::Cow::Borrowed(val.as_str())
                };
                if contains_var_function(val) && (resolved.trim().is_empty() || contains_var_function(&resolved))
                {
                    continue;
                }
                let trimmed = resolved.trim();
                if trimmed.eq_ignore_ascii_case("inherit") {
                    track_inherit_for_id(&mut inherit_props, properties::resolve(prop));
                } else if trimmed.eq_ignore_ascii_case("revert-layer") {
                    copy_property_from_style(&mut style, revert_layer_base, prop);
                } else if trimmed.eq_ignore_ascii_case("revert") {
                    if let Some(base) = revert_base {
                        copy_property_from_style(&mut style, base, prop);
                    } else {
                        apply_property(&mut style, prop, "initial");
                    }
                } else {
                    if prop == "color-scheme" {
                        continue;
                    }
                    clear_inherit_tracking_for_property(&mut inherit_props, prop);
                    apply_property(&mut style, prop, &resolved);
                }
            }
        } else {
            // Fast path: no var() — use compiled declarations directly
            if let Some(&(id, ref val)) = rule
                .compiled_decls
                .iter()
                .find(|(id, _)| *id == properties::PropertyId::ColorScheme)
            {
                apply_css_value_with_cascade_context(
                    &mut style,
                    id,
                    val,
                    local_vars,
                    parent_style,
                    revert_base,
                    revert_layer_base,
                );
            }
            for &(id, ref val) in &rule.compiled_decls {
                if id == properties::PropertyId::ColorScheme {
                    continue;
                }
                if matches!(val, crate::types::CssValue::Inherit) {
                    track_inherit_for_id(&mut inherit_props, id);
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
                    if value_needs_substitution(s) {
                        let resolved = resolve_var_references_for_color_scheme(
                            s,
                            local_vars,
                            &style.color_scheme,
                        );
                        if !resolved.trim().is_empty() && !contains_var_function(&resolved) {
                            let trimmed = resolved.trim();
                            let name = property_defs::get(id).name;
                            if trimmed.eq_ignore_ascii_case("inherit") {
                                track_inherit_for_id(&mut inherit_props, id);
                            } else if trimmed.eq_ignore_ascii_case("revert-layer") {
                                copy_property_from_style(&mut style, revert_layer_base, name);
                            } else if trimmed.eq_ignore_ascii_case("revert") {
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
                        if trimmed.eq_ignore_ascii_case("inherit") {
                            track_inherit_for_id(&mut inherit_props, id);
                        } else if trimmed.eq_ignore_ascii_case("revert-layer") {
                            let name = property_defs::get(id).name;
                            copy_property_from_style(&mut style, revert_layer_base, name);
                        } else if trimmed.eq_ignore_ascii_case("revert") {
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
    let mut important_matched: Vec<_> = matched
        .iter()
        .copied()
        .filter(|(_, ri, _)| {
            let rule = &stylesheet.rules[*ri];
            !rule.compiled_important.is_empty() || !rule.important_declarations.is_empty()
        })
        .collect();
    important_matched.sort_by(|&a, &b| important_cascade_cmp(&stylesheet.rules, a, b));
    if !important_matched.is_empty() {
        for author_pass in [true, false] {
            let important_color_scheme = prescan_color_scheme(
                &style,
                &stylesheet.rules,
                &important_matched,
                local_vars,
                true,
                Some(author_pass),
            );
            style.color_scheme = important_color_scheme;
            let mut current_important_layer: Option<(bool, u32)> = None;
            let mut important_layer_start_style = needs_revert_snapshot.then(|| style.clone());
            for &(sp, ri, _) in &important_matched {
                if is_author_origin(sp) != author_pass {
                    continue;
                }
                let rule = &stylesheet.rules[ri];
                let layer_key = (is_author_origin(sp), rule.layer_rank);
                if current_important_layer != Some(layer_key) {
                    current_important_layer = Some(layer_key);
                    if needs_revert_snapshot {
                        important_layer_start_style = Some(style.clone());
                    }
                }
                let important_layer_base = important_layer_start_style
                    .as_ref()
                    .unwrap_or(root.style.as_ref());
                let revert_base = if is_author_origin(sp) {
                    pre_author_normal_style.as_ref()
                } else {
                    None
                };
                if has_vars && rule.has_var_refs {
                    if let Some(val) = rule.important_declarations.get("color-scheme") {
                        let resolved = resolve_var_references_for_color_scheme(
                            val,
                            local_vars,
                            &style.color_scheme,
                        );
                        if !resolved.trim().is_empty() && !contains_var_function(&resolved) {
                            apply_resolved_property_with_cascade_context(
                                &mut style,
                                "color-scheme",
                                properties::PropertyId::ColorScheme,
                                &resolved,
                                parent_style,
                                revert_base,
                                important_layer_base,
                            );
                        }
                    }
                    for (prop, val) in &rule.important_declarations {
                        if prop.starts_with("--") {
                            continue;
                        }
                        let resolved = resolve_var_references_for_color_scheme(
                            val,
                            local_vars,
                            &style.color_scheme,
                        );
                        if contains_var_function(val)
                            && (resolved.trim().is_empty() || contains_var_function(&resolved))
                        {
                            continue;
                        }
                        if prop == "color-scheme" {
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
                            important_layer_base,
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
                            important_layer_base,
                        );
                    }
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
                let resolved =
                    resolve_var_references_for_color_scheme(val, local_vars, &style.color_scheme);
                if contains_var_function(val) && contains_var_function(&resolved) {
                    continue;
                }
                inline_hover_props.push((real_prop.to_string(), resolved));
                continue;
            }
            let resolved =
                resolve_var_references_for_color_scheme(val, local_vars, &style.color_scheme);
            if contains_var_function(val) && (resolved.trim().is_empty() || contains_var_function(&resolved)) {
                continue;
            } else if resolved.trim().eq_ignore_ascii_case("inherit") {
                if let Some(p) = parent_style {
                    copy_property_from_parent(&mut style, p, prop);
                }
            } else if resolved.trim().eq_ignore_ascii_case("revert")
                || resolved.trim().eq_ignore_ascii_case("revert-layer")
            {
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
    if !important_matched.is_empty() {
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
    }
    if !inline_important.is_empty() {
        let inline_important_start_style = style.clone();
        for (prop, val) in &inline_important {
            let resolved =
                resolve_var_references_for_color_scheme(val, local_vars, &style.color_scheme);
            if contains_var_function(val) && (resolved.trim().is_empty() || contains_var_function(&resolved)) {
                continue;
            }
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
    }
    if !important_matched.is_empty() {
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
    }
    if let Some(started) = profile_rules_started {
        crate::profile::record(crate::profile::Phase::CascadeApplyRules, started.elapsed());
    }
    let profile_finalize_started = crate::profile::is_enabled().then(std::time::Instant::now);
    // Capture href from attributes (non-standard CSS, but useful for our editor)
    if let Some(href) = root.attributes.get("href") {
        style.href = href.clone();
    }

    // Resolve relative font size to absolute Px for inheritance parity
    let parent_font_px = parent_style
        .map(|p| p.font_size_px(root_font_px, root_font_px))
        .unwrap_or(root_font_px);
    let font_px = style
        .font_size
        .resolve_vp(parent_font_px, parent_font_px, root_font_px, vw, vh)
        .max(1.0);
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
    style.custom_props = local_vars_owned
        .as_ref()
        .cloned()
        .or_else(|| parent_style.map(|parent| parent.custom_props.clone()))
        .unwrap_or_else(|| std::sync::Arc::new(inherited_vars.clone()));
    let local_vars = local_vars_owned.as_deref().unwrap_or(inherited_vars);
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
    // Filter lengths and currentColor depend on the final element context.
    // Keep the computed text too: explicit inheritance copies computed values.
    crate::css::finalize_filter_values(&mut style, &|length| {
        length.resolve_vp(font_px, 0.0, root_font_px, vw, vh)
    });
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
    let style_changed = root.style.as_ref() != &style;
    let old_display = root.style.display;
    let new_display = style.display;
    if style_changed {
        root.style = std::sync::Arc::new(style);
    }
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
                    rule.media_condition.label()
                },
                layer: rule.layer.clone(),
                layer_rank: rule.layer_rank,
            });
        }
    }
    // Mark dirty so the layout subtree pruning (in layout_box_with_fc) knows to
    // re-layout this element.  Cleared by the individual layout algorithms after
    // they have computed the final geometry.
    if style_changed || root.layout.last_containing_width <= 0.0 {
        root.layout.layout_dirty = true;
    }
    if old_display != new_display
        && matches!(old_display, Display::None) != matches!(new_display, Display::None)
    {
        mark_layout_subtree_dirty(root);
    }

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
    // Element-created instances live through following siblings. Descendant
    // instances are removed when this element's child scope finishes.
    if let Some(started) = profile_style_started {
        crate::profile::record(crate::profile::Phase::CascadeApplyStyle, started.elapsed());
    }
    if let Some(started) = profile_finalize_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyFinalize,
            started.elapsed(),
        );
    }
    let profile_counters_started = crate::profile::is_enabled().then(std::time::Instant::now);
    counters.apply_element(&root.style);
    if root.style.display == Display::ListItem {
        if let Some(value) = counters
            .values
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

    let counter_containment = root.style.contain_style
        && !matches!(root.style.display, Display::None | Display::Contents);
    counters.enter_children();
    if counter_containment {
        counters.enter_containment();
    }

    if let Some((Some(txt), mut ps)) = build_pseudo_style_shared(
        &mut before_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        counters.apply_element(&ps);
        let resolved_content =
            resolve_generated_content(&txt, &root.attributes, &mut ps, &counters.values);
        std::sync::Arc::make_mut(&mut root.style).before_content = resolved_content;
        std::sync::Arc::make_mut(&mut root.style).before_style = Some(ps);
    }
    let mut pending_after = None;
    if let Some((Some(txt), ps)) = build_pseudo_style_shared(
        &mut after_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        pending_after = Some(txt);
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
    if let Some((txt, mut ps)) = build_pseudo_style_shared(
        &mut marker_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        if let Some(txt) = txt {
            // `quotes` is not an applicable marker-box property; use the
            // originating element's inherited quote pairs (CSS Lists 3).
            ps.rare_mut().quotes = root.style.rare().quotes.clone();
            let resolved_content =
                resolve_generated_content(&txt, &root.attributes, &mut ps, &counters.values);
            std::sync::Arc::make_mut(&mut root.style).marker_content = resolved_content;
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
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut file_selector_button_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).file_selector_button_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut details_content_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).details_content_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut spelling_error_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).spelling_error_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut grammar_error_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).grammar_error_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut first_line_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).first_line_style = Some(ps);
    }
    if let Some((_, ps)) = build_pseudo_style_shared(
        &mut first_letter_matched,
        &root.style,
        &local_vars,
        &root.attributes,
        &stylesheet.rules,
    ) {
        std::sync::Arc::make_mut(&mut root.style).first_letter_style = Some(ps);
    }

    {
        let authored_content = root.style.rare().content.clone();
        if !authored_content.is_empty() {
            let resolved = resolve_content_value_with_context(
                &authored_content,
                Some(&root.attributes),
                root.style.rare().quotes.as_deref(),
            );
            std::sync::Arc::make_mut(&mut root.style).rare_mut().content =
                resolve_counters_in_content(&resolved, &counters.values);
        }
    }

    {
        let style = std::sync::Arc::make_mut(&mut root.style);
        for pseudo in [
            &mut style.before_style,
            &mut style.after_style,
            &mut style.selection_style,
            &mut style.placeholder_style,
            &mut style.marker_style,
            &mut style.backdrop_style,
            &mut style.file_selector_button_style,
            &mut style.details_content_style,
            &mut style.spelling_error_style,
            &mut style.grammar_error_style,
            &mut style.first_line_style,
            &mut style.first_letter_style,
        ]
        .into_iter()
        .flatten()
        {
            let pseudo_font = pseudo
                .font_size
                .resolve_vp(font_px, font_px, root_font_px, vw, vh);
            crate::css::finalize_filter_values(pseudo, &|length| {
                length.resolve_vp(pseudo_font, 0.0, root_font_px, vw, vh)
            });
        }
    }
    if let Some(started) = profile_counters_started {
        crate::profile::record(
            crate::profile::Phase::CascadeApplyCounters,
            started.elapsed(),
        );
    }
    Some(CascadedNodeState {
        root_font_px,
        local_vars: local_vars_owned,
        counter_containment,
        pending_after,
    })
}

pub(crate) fn apply_cascade_inner(
    root: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    parent_style: Option<&ComputedStyle>,
    root_font_px: f32,
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
    counters: &mut CounterState,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    prev_siblings: &[SiblingInfo],
    next_siblings: &[SiblingInfo],
    next_sibling_nodes: &[&crate::types::WebCore],
    share_cache: &mut ShareCache,
    mut precomputed: Option<&mut MatchMap>,
) {
    let Some(state) = apply_cascade_node(
        root,
        stylesheet,
        parent_style,
        root_font_px,
        ancestors,
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
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
        focus_within_chain,
        prev_siblings,
        next_siblings,
        next_sibling_nodes,
        share_cache,
        precomputed.as_deref_mut(),
    ) else {
        return;
    };
    let CascadedNodeState {
        root_font_px,
        local_vars,
        counter_containment,
        pending_after,
    } = state;
    let local_vars = local_vars.as_deref().unwrap_or(inherited_vars);

    let has_descendants = !root.children.is_empty() || root.shadow_root.is_some();
    if has_descendants {
        ancestors.push(AncestorInfo {
            tag: root.tag.clone(),
            attributes: std::sync::Arc::new(root.attributes.clone()),
            auto_direction: super::matching::auto_direction(root),
            child_index,
            sibling_count,
            type_child_index,
            type_sibling_count,
            node_id: root.node_id,
            prev_siblings: std::sync::Arc::new(prev_siblings.to_vec()),
        });
    }

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
        counters: &mut CounterState,
        hover_chain: &std::collections::HashSet<u32>,
        focus_within_chain: &std::collections::HashSet<u32>,
        share_cache: &mut ShareCache,
        mut precomputed: Option<&mut MatchMap>,
        projected_document_stylesheet: Option<&Stylesheet>,
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
        let sibling_records: Vec<SiblingInfo> = children
            .iter()
            .filter(|c| c.is_element())
            .map(SiblingInfo::from_node)
            .collect();
        // ⛔ The cache is the caller's now, spanning the whole document —
        // see `ShareCache`. A per-parent one could only ever share between
        // siblings, which measured 2.9% on demo.html.
        // Scope sharing to the actual parent node, not to the parent's computed
        // style pointer. Different parents can legitimately share the same
        // ComputedStyle Arc while living under different ancestor chains; a
        // descendant selector such as `.section .item` can then match under one
        // parent and not the other. Sharing across those parents hands later
        // nodes a cached style from the wrong selector context, which makes
        // loaded CSS appear to vanish on large streamed pages.
        let parent_node_id = ancestors
            .last()
            .map(|ancestor| ancestor.node_id)
            .unwrap_or(0);
        let parent_id = parent_node_id as usize;
        for i in 0..n_children {
            let (_before, rest) = children.split_at_mut(i);
            let (child, after) = rest.split_first_mut().unwrap();
            let is_projected_child = crate::types::is_projected_slot_subtree(child);
            let cascade_stylesheet = if is_projected_child {
                projected_document_stylesheet.unwrap_or(stylesheet)
            } else {
                stylesheet
            };
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
            let share_eligible = child.is_element()
                // This cache hit replaces the whole node visit, not just its
                // declaration calculation. Descendants still require cascade.
                && child.children.is_empty()
                && child.shadow_root.is_none()
                && !is_projected_child
                && parent_node_id != 0
                && !child.attributes.contains_key("id")
                && !child.attributes.contains_key("style")
                && !hover_chain.contains(&child.node_id);
            let share_key = if share_eligible {
                let class_attr = child
                    .attributes
                    .get("class")
                    .map(|s| s.as_str())
                    .unwrap_or("");
                cascade_stylesheet.candidate_rules(
                    &child.tag,
                    None,
                    class_attr.split_whitespace(),
                    candidates_buf,
                );
                let sibling_sensitive = cascade_stylesheet.has_sibling_sensitive_rules
                    && cascade_stylesheet.candidate_rules_are_sibling_sensitive(candidates_buf);
                let context_sensitive =
                    cascade_stylesheet.candidate_rules_need_selector_context(candidates_buf);
                // Sibling and ancestor-sensitive selectors make otherwise
                // identical elements unsafe to share.
                if sibling_sensitive || context_sensitive {
                    None
                } else {
                    let mut parts: Vec<String> = child
                        .attributes
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    parts.sort();
                    parts.push(child.selector_state_key(focused_box));
                    Some((parent_id, child.tag.clone(), parts.join("\u{1}")))
                }
            } else {
                None
            };

            if let Some(share_key) = share_key.as_ref() {
                if let Some(cached) = share_cache.get(share_key) {
                    // ⛔ THE point of item 1: a shared style is a refcount
                    // bump, not a 2.3 KB memcpy. `cached` is already an `Arc`.
                    let old_display = child.style.display;
                    let new_display = cached.display;
                    if layout_affecting_style_changed(&child.style, cached) {
                        child.layout.layout_dirty = true;
                        child.layout.intrinsic_dirty = true;
                        child.layout.line_cache.clear();
                    }
                    child.style = cached.clone();
                    if old_display != new_display
                        && matches!(old_display, Display::None)
                            != matches!(new_display, Display::None)
                    {
                        mark_layout_subtree_dirty(child);
                    }
                    continue;
                }
            }

            // Parallel matching already captured the sibling context for this
            // node. Only misses need a fresh list of following sibling nodes.
            let has_precomputed_match = !is_projected_child
                && child.node_id != 0
                && precomputed
                    .as_ref()
                    .is_some_and(|matches| matches.contains_key(&child.node_id));
            let after_nodes: Vec<&crate::types::WebCore> = if has_precomputed_match {
                Vec::new()
            } else {
                after.iter().collect()
            };
            apply_cascade_inner(
                child,
                cascade_stylesheet,
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
                focus_within_chain,
                prev_for_child,
                next_for_child,
                &after_nodes,
                share_cache,
                if is_projected_child {
                    None
                } else {
                    precomputed.as_deref_mut()
                },
            );
            if matches!(
                parent_style.display,
                Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
            ) {
                blockify_flex_or_grid_item(std::sync::Arc::make_mut(&mut child.style));
            }
            if is_projected_child {
                apply_host_projected_rules_with_ancestors(
                    child,
                    ancestors,
                    stylesheet,
                    Some(parent_style),
                    vw,
                    vh,
                    focused_box,
                    keyboard_focus,
                    hover_chain,
                    focus_within_chain,
                    document_url,
                    candidates_buf,
                );
                apply_slotted_rules_to_projected(
                    child,
                    None,
                    stylesheet,
                    Some(parent_style),
                    vw,
                    vh,
                    candidates_buf,
                );
            }
            // Cache style for sharing with future siblings
            if let Some(share_key) = share_key
                && !share_cache.contains_key(&share_key)
                && child.style.counter_reset.is_empty()
                && child.style.counter_increment.is_empty()
                && child.style.counter_set.is_empty()
                && child.style.display != Display::ListItem
                && child.style.before_style.is_none()
                && child.style.after_style.is_none()
            {
                share_cache.insert(share_key, child.style.clone());
            }
        }
    }

    // Shadow DOM: cascade light DOM children with the document stylesheet, then
    // cascade shadow children with the shadow's scoped stylesheet.
    //
    // Slot projection clones light DOM children into shadow `<slot>` nodes
    // before layout. Those projected nodes must carry their document-cascaded
    // styles; otherwise slotted dropdown/menu content has matched author rules
    // in the light tree but zero/default boxes in the composed tree.
    // CSS custom properties cross the shadow boundary via inherited_vars.
    // ⛔ The parent style for the children comes from the ARC now, not from a
    // 2.3 KB local held alive across the recursion. This is the frame shrink:
    // what crosses the recursive call is a pointer.
    let parent_for_children = root.style.clone();
    if root.shadow_root.is_some() {
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
            focus_within_chain,
            share_cache,
            precomputed,
            None,
        );
        // Take shadow root temporarily to satisfy borrow checker
        let mut sr = root.shadow_root.take().unwrap();
        sr.stylesheet.inspect_mode = stylesheet.inspect_mode;
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
        let mut shadow_ancestors = ancestors.clone();
        shadow_ancestors.push(projected_ancestor_info(root));
        cascade_children(
            &mut sr.children,
            &sr.stylesheet,
            &parent_for_children,
            root_font_px,
            &mut shadow_ancestors,
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
            focus_within_chain,
            share_cache,
            None,
            Some(stylesheet),
        );
        root.shadow_root = Some(sr);
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
            focus_within_chain,
            share_cache,
            precomputed,
            None,
        );
    }

    if has_descendants {
        ancestors.pop();
    }

    if let Some(content) = pending_after {
        let style = std::sync::Arc::make_mut(&mut root.style);
        if let Some(ps) = style.after_style.as_mut() {
            counters.apply_element(ps);
            style.after_content =
                resolve_generated_content(&content, &root.attributes, ps, &counters.values);
        }
    }

    // Child display values must be resolved before choosing whether generated
    // inline content needs anonymous line boxes around block children.
    build_pseudo_element_boxes(root);

    counters.exit_children();
    if counter_containment {
        counters.exit_containment();
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
            "value" if root.tag == "li" => {
                if let Some(value) = crate::html::forms::parse_integer(val) {
                    let value = value.clamp(i32::MIN as i64, i32::MAX as i64);
                    apply_property(style, "counter-set", &format!("list-item {value}"));
                }
            }
            "start" if root.tag == "ol" => {
                if let Some(value) = crate::html::forms::parse_integer(val) {
                    let reversed = root.attributes.contains_key("reversed");
                    let value = if reversed {
                        value.saturating_add(1)
                    } else {
                        value.saturating_sub(1)
                    }
                    .clamp(i32::MIN as i64, i32::MAX as i64);
                    let name = if reversed {
                        "reversed(list-item)"
                    } else {
                        "list-item"
                    };
                    apply_property(style, "counter-reset", &format!("{name} {value}"));
                }
            }
            "reversed" if root.tag == "ol" => {
                let start = root
                    .attributes
                    .get("start")
                    .and_then(|value| crate::html::forms::parse_integer(value));
                if start.is_none() {
                    apply_property(style, "counter-reset", "reversed(list-item)");
                    if let Some(reset) = style.counter_reset.first_mut() {
                        reset.html_list_start = true;
                    }
                }
            }
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
                    if rows > 1.0 {
                        let height = rows * 1.2 + 0.5;
                        apply_property(style, "height", &format!("{height}em"));
                    }
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
            "width" if crate::html::supports_dimension_presentational_hint(&root.tag, "width") => {
                let selected_source_width;
                let clean = if root.tag == "img" {
                    if let Some(w) = root.selected_source_width {
                        selected_source_width = w.to_string();
                        selected_source_width.as_str()
                    } else {
                        val.trim().trim_end_matches(';').trim()
                    }
                } else {
                    val.trim().trim_end_matches(';').trim()
                };
                if clean.ends_with('%') {
                    apply_property(style, "width", clean);
                } else {
                    let num = clean.strip_suffix("px").unwrap_or(clean).trim();
                    if let Ok(n) = num.parse::<f32>() {
                        apply_property(style, "width", &format!("{}px", n));
                    }
                }
            }
            "height"
                if crate::html::supports_dimension_presentational_hint(&root.tag, "height") =>
            {
                let selected_source_height;
                let clean = if root.tag == "img" {
                    if let Some(h) = root.selected_source_height {
                        selected_source_height = h.to_string();
                        selected_source_height.as_str()
                    } else {
                        val.trim().trim_end_matches(';').trim()
                    }
                } else {
                    val.trim().trim_end_matches(';').trim()
                };
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
                    match crate::layout::text::html_auto_direction(root) {
                        Direction::RTL => apply_property(style, "direction", "rtl"),
                        Direction::LTR => apply_property(style, "direction", "ltr"),
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    if root.tag == "bdi" && !root.attributes.get("dir").is_some_and(|dir| {
        dir.eq_ignore_ascii_case("ltr") || dir.eq_ignore_ascii_case("rtl") || dir.eq_ignore_ascii_case("auto")
    }) {
        match super::matching::auto_direction(root).unwrap_or(Direction::LTR) {
            Direction::RTL => apply_property(style, "direction", "rtl"),
            Direction::LTR => apply_property(style, "direction", "ltr"),
        }
    }
    if super::matching::default_ltr_telephone(&root.tag, &root.attributes) {
        apply_property(style, "direction", "ltr");
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
