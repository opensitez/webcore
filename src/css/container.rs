//! Container queries — evaluation and the container cascade pass.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Container Cascade Pass ───────────────────────────────────────────────────

/// An entry on the container ancestor stack built during `apply_container_cascade_tree`.
#[derive(Clone)]
pub struct ContainerEntry {
    pub width: f32,
    pub height: f32,
    pub container_type: crate::types::ContainerType,
    pub name: String,
    pub style: std::sync::Arc<crate::types::ComputedStyle>,
}

pub(crate) struct AppliedContainerStyle {
    base: std::sync::Arc<ComputedStyle>,
    rules: Vec<usize>,
}

struct ContainerConditionBranch<'a> {
    name: &'a str,
    condition: &'a str,
    required_type: Option<crate::types::ContainerType>,
}

pub(crate) fn parse_container_branch_header(header: &str) -> (&str, &str) {
    let header = header.trim();
    if header.starts_with('(') {
        return ("", header);
    }
    let end = header
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || *ch == '(')
        .map(|(index, _)| index)
        .unwrap_or(header.len());
    let first = &header[..end];
    if first.eq_ignore_ascii_case("style")
        || first.eq_ignore_ascii_case("not")
        || first.eq_ignore_ascii_case("and")
        || first.eq_ignore_ascii_case("or")
    {
        return ("", header);
    }
    let condition = header[end..].trim();
    if condition.is_empty() {
        (first, "")
    } else {
        (first, condition)
    }
}

fn query_container_type(condition: &str) -> Option<crate::types::ContainerType> {
    let lower = condition.to_ascii_lowercase();
    let mut outside_style = String::with_capacity(lower.len());
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("style(") {
        outside_style.push_str(&rest[..start]);
        let body = &rest[start + "style(".len()..];
        let mut depth = 1usize;
        let mut end = body.len();
        for (index, ch) in body.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &body[end..];
    }
    outside_style.push_str(rest);
    if ["height", "block-size", "aspect-ratio", "orientation"]
        .iter()
        .any(|feature| outside_style.contains(feature))
    {
        Some(crate::types::ContainerType::Size)
    } else if ["width", "inline-size"]
        .iter()
        .any(|feature| outside_style.contains(feature))
    {
        Some(crate::types::ContainerType::InlineSize)
    } else {
        None
    }
}

fn container_condition_matches(
    branches: &[ContainerConditionBranch<'_>],
    containers: &[ContainerEntry],
) -> bool {
    branches.iter().any(|branch| {
        let container = containers.iter().rev().find(|container| {
            (branch.name.is_empty() || container.name == branch.name)
                && match branch.required_type {
                    Some(crate::types::ContainerType::Size) => {
                        container.container_type == crate::types::ContainerType::Size
                    }
                    Some(crate::types::ContainerType::InlineSize) => {
                        container.container_type != crate::types::ContainerType::Normal
                    }
                    _ => true,
                }
        });
        container.is_some_and(|container| {
            evaluate_container_for_type_and_style(
                branch.condition,
                container.width,
                container.height,
                container.container_type,
                Some(&container.style),
            )
        })
    })
}

/// Walk `node` and all its descendants applying any `@container` rules whose
/// condition matches the nearest container ancestor in `container_stack`.
///
/// This is called as a post-layout pass (after box sizes are known) so that
/// container dimensions are available for condition evaluation.
///
/// Returns `true` if any styles were changed (used to decide whether a
/// second layout pass is needed).
pub fn apply_container_cascade_tree(
    node: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    container_stack: &[ContainerEntry],
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
) -> bool {
    let mut applied_rules = HashMap::new();
    apply_container_cascade_tree_with_state(
        node,
        stylesheet,
        container_stack,
        ancestors,
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        root_font_px,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        &mut applied_rules,
    )
}

pub(crate) fn apply_container_cascade_tree_with_state(
    node: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    container_stack: &[ContainerEntry],
    ancestors: &[AncestorInfo],
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    applied_rules: &mut HashMap<u32, AppliedContainerStyle>,
) -> bool {
    // Create owned Vecs once at the top level; the recursive inner function
    // reuses them via push/pop so no per-node heap allocation is needed.
    let mut cs = container_stack.to_vec();
    let mut anc = ancestors.to_vec();
    let conditions = stylesheet
        .rules
        .iter()
        .map(|rule| {
            std::iter::once(&rule.container_condition)
                .chain(&rule.nested_container_conditions)
                .filter(|header| !header.is_empty())
                .map(|header| {
                    crate::css::value_parse::split_top_level_commas(header)
                        .into_iter()
                        .map(|branch| {
                            let (name, condition) = parse_container_branch_header(branch);
                            ContainerConditionBranch {
                                name,
                                condition,
                                required_type: query_container_type(condition),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    apply_container_cascade_inner(
        node,
        stylesheet,
        &mut cs,
        &mut anc,
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        root_font_px,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        applied_rules,
        &conditions,
    )
}

fn apply_container_cascade_inner(
    node: &mut crate::types::WebCore,
    stylesheet: &Stylesheet,
    container_stack: &mut Vec<ContainerEntry>,
    ancestors: &mut Vec<AncestorInfo>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    root_font_px: f32,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    applied_rules: &mut HashMap<u32, AppliedContainerStyle>,
    conditions: &[Vec<Vec<ContainerConditionBranch<'_>>>],
) -> bool {
    use crate::types::ContainerType;

    let mut changed = false;

    // Apply matching container rules to this element
    if !container_stack.is_empty() {
        let empty_hover = std::collections::HashSet::new();
        let empty_focus = std::collections::HashSet::new();
        let match_ctx = MatchContext {
            focused_box,
            keyboard_focus,
            type_child_index,
            type_sibling_count,
            html_box: Some(node),
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
        let mut cont_matched: Vec<(usize, u32, Declarations)> = Vec::new();
        for (rule_index, rule) in stylesheet.rules.iter().enumerate() {
            if rule.container_condition.is_empty() {
                continue;
            }
            if !rule.media_condition.is_empty() && !evaluate_media(&rule.media_condition, vw, vh) {
                continue;
            }
            if !conditions[rule_index]
                .iter()
                .all(|branches| container_condition_matches(branches, container_stack))
            {
                continue;
            }
            // Full selector matching (same logic as apply_cascade_inner)
            let has_hover = rule.selectors.iter().any(|s| {
                s.parts
                    .iter()
                    .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "hover"))
            });
            let has_active = rule.selectors.iter().any(|s| {
                s.parts
                    .iter()
                    .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "active"))
            });
            if has_hover || has_active {
                continue;
            } // state pseudo-class rules are handled separately
            for sel in &rule.selectors {
                if sel.matches_with_ancestors_ctx(
                    node,
                    child_index,
                    sibling_count,
                    ancestors,
                    &match_ctx,
                ) {
                    if rule.pseudo_element == PseudoElement::None {
                        let mut merged = rule.declarations.clone();
                        for (k, v) in &rule.important_declarations {
                            merged.insert(k.clone(), v.clone());
                        }
                        cont_matched.push((rule_index, rule.specificity, merged));
                    }
                    break;
                }
            }
        }
        cont_matched.sort_by_key(|(_, specificity, _)| *specificity);
        let matched_rules = cont_matched
            .iter()
            .map(|(index, _, _)| *index)
            .collect::<Vec<_>>();
        if !matched_rules.is_empty() || applied_rules.contains_key(&node.node_id) {
            let entry = applied_rules
                .entry(node.node_id)
                .or_insert_with(|| AppliedContainerStyle {
                    base: node.style.clone(),
                    rules: Vec::new(),
                });
            if entry.rules != matched_rules {
                let mut candidate = entry.base.clone();
                for (_, _, decls) in &cont_matched {
                    for (prop, val) in decls {
                        let resolved = resolve_var_references(val, &stylesheet.variables);
                        apply_property(std::sync::Arc::make_mut(&mut candidate), prop, &resolved);
                    }
                }
                entry.rules = matched_rules;
                if candidate.as_ref() != node.style.as_ref() {
                    node.style = candidate;
                    node.layout.layout_dirty = true;
                    changed = true;
                }
            }
        }
    }

    // Update container stack: if this element is a container, push it
    // Push this element as a container ancestor (if it qualifies), recurse, pop.
    let pushed_container = !node.tag.starts_with('#');
    if pushed_container {
        container_stack.push(ContainerEntry {
            width: node.layout.content_rect.w,
            height: node.layout.content_rect.h,
            container_type: node.style.container_type,
            name: node.style.container_name.clone(),
            style: node.style.clone(),
        });
    }

    let n_children = node.children.len();
    if n_children == 0 {
        if pushed_container {
            container_stack.pop();
        }
        return changed;
    }

    // Push this element as an ancestor for children (mirrors apply_cascade_inner).
	    ancestors.push(AncestorInfo {
	        tag: node.tag.clone(),
	        attributes: node.attributes.clone(),
	        child_index,
	        sibling_count,
	        type_child_index,
	        type_sibling_count,
	        node_id: node.node_id,
	        prev_siblings: Vec::new(),
	    });

    // O(n) type counting (was O(n²) with per-child filter passes).
    let child_tags: Vec<String> = node
        .children
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

    for (i, child) in node.children.iter_mut().enumerate() {
        let c = apply_container_cascade_inner(
            child,
            stylesheet,
            container_stack,
            ancestors,
            i,
            n_children,
            type_counts[i],
            type_totals[i],
            root_font_px,
            vw,
            vh,
            focused_box,
            keyboard_focus,
            applied_rules,
            conditions,
        );
        if c {
            changed = true;
        }
    }

    // If any descendant changed, mark this node dirty too.
    // This prevents the layout subtree pruning from skipping an ancestor whose
    // content width is unchanged while a child still needs re-layout.
    if changed {
        node.layout.layout_dirty = true;
    }

    ancestors.pop();
    if pushed_container {
        container_stack.pop();
    }
    changed
}
