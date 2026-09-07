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
    // Create owned Vecs once at the top level; the recursive inner function
    // reuses them via push/pop so no per-node heap allocation is needed.
    let mut cs = container_stack.to_vec();
    let mut anc = ancestors.to_vec();
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
) -> bool {
    use crate::types::ContainerType;

    let mut changed = false;

    // Apply matching container rules to this element
    if !container_stack.is_empty() {
        let empty_hover = std::collections::HashSet::new();
        let match_ctx = MatchContext {
            focused_box,
            keyboard_focus,
            type_child_index,
            type_sibling_count,
            html_box: Some(node),
            hover_chain: &empty_hover,
            element_id: node.node_id,
            scope_root_id: 0,
            target_id: 0,
            document_url: "",
            prev_siblings: &[],
            next_siblings: &[],
        };
        let mut cont_matched: Vec<(u32, Declarations)> = Vec::new();
        for rule in &stylesheet.rules {
            if rule.container_condition.is_empty() {
                continue;
            }
            if !rule.media_condition.is_empty() && !evaluate_media(&rule.media_condition, vw, vh) {
                continue;
            }
            // Find nearest container that matches the rule's name
            let ctx = if rule.container_name.is_empty() {
                container_stack.last()
            } else {
                container_stack
                    .iter()
                    .rev()
                    .find(|c| c.name == rule.container_name)
            };
            let ctx = match ctx {
                Some(c) => c,
                None => continue,
            };
            if !evaluate_container_for_type_and_style(
                &rule.container_condition,
                ctx.width,
                ctx.height,
                ctx.container_type,
                Some(&ctx.style),
            ) {
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
                        cont_matched.push((rule.specificity, merged));
                    }
                    break;
                }
            }
        }
        if !cont_matched.is_empty() {
            changed = true;
            cont_matched.sort_by_key(|(sp, _)| *sp);
            for (_, decls) in &cont_matched {
                for (prop, val) in decls {
                    let resolved = resolve_var_references(val, &stylesheet.variables);
                    apply_property(std::sync::Arc::make_mut(&mut node.style), prop, &resolved);
                }
            }
            // Mark layout dirty so the subtree pruning doesn't suppress the
            // geometry changes caused by these newly applied container rules.
            node.layout.layout_dirty = true;
        }
    }

    // Update container stack: if this element is a container, push it
    // Push this element as a container ancestor (if it qualifies), recurse, pop.
    let pushed_container = !matches!(node.style.container_type, ContainerType::Normal);
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
