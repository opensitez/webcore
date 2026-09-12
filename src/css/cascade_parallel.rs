//! The parallel cascade pass.
//!
//! ⛔ THE MATCHING ONLY. This file used to carry a second copy of the whole
//! cascade — presentational attributes, `!important` ordering, the variable
//! scope, counters, pseudo-elements, shadow DOM — and the two copies drifted:
//! a large page rendered one way on load (here) and another way the moment a
//! hover re-cascaded it through `cascade.rs`. What is parallel about the
//! parallel cascade is running the SELECTORS off-thread; everything downstream
//! of "which rules matched" is `cascade::apply_cascade_inner`, once.

#![allow(unused_imports)]
use super::*;
use crate::css::cascade::{match_rules, MatchMap, MatchSets};
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Parallel Cascade ────────────────────────────────────────────────────────

/// A borrowed element the matching pass may read from any thread.
///
/// `WebCore` is not `Sync`, and the single reason is `LayoutBox`'s
/// `cached_intrinsic_w: Cell<f32>` — a layout memo. Three facts make sharing it
/// across the matching pass sound:
///
/// * `apply_cascade_parallel` holds the tree by `&mut`, so the shared reborrow
///   handed to this pass has no concurrent writer anywhere.
/// * **Selector matching reads DOM and element state — `tag`, `attributes`,
///   `children`, `text`, `node_id`, `checkedness`, `value_state`, `data`,
///   `top_layer_kind` — and never reads or writes `layout`.** That is the rule
///   this claim rests on; a matcher that reaches into `layout` voids it.
/// * Every matcher entry point takes `&WebCore`, so the cell is the only way to
///   write through one at all, and nothing on the match path touches it.
///
/// The wrapper is around the BORROW, not the work item: a future field that is
/// genuinely not `Sync` then fails to compile instead of being blessed by this.
struct MatchNode<'a>(&'a crate::types::WebCore);
unsafe impl Send for MatchNode<'_> {}
unsafe impl Sync for MatchNode<'_> {}

/// One element to match, with everything the matcher needs about its position.
///
/// It borrows the node so the matcher can be handed a real `html_box`: `:has()`,
/// `:empty`, `:focus`, `:focus-within`, `:checked` and friends all read the box,
/// and a matcher without one silently answers false.
struct CascadeWorkItem<'a> {
    node: MatchNode<'a>,
    ancestors: Vec<AncestorInfo>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    /// The parent's element children as `(tag, id, class)`, in order, shared by
    /// every sibling — one list per parent rather than a private copy each.
    /// `sibling_pos` is where this element sits in it, so the slice before that
    /// is what `+` and `~` look at.
    siblings: std::sync::Arc<Vec<(String, String, String)>>,
    sibling_pos: usize,
    sibling_nodes: std::sync::Arc<Vec<MatchNode<'a>>>,
    raw_child_index: usize,
}

/// Pass 1: flatten the DOM tree into a work list.
/// Each element gets its ancestor chain snapshot (needed for descendant selectors).
fn flatten_tree_for_cascade<'a>(
    node: &'a crate::types::WebCore,
    ancestors: &mut Vec<AncestorInfo>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    siblings: &std::sync::Arc<Vec<(String, String, String)>>,
    sibling_pos: usize,
    sibling_nodes: &std::sync::Arc<Vec<MatchNode<'a>>>,
    raw_child_index: usize,
    out: &mut Vec<CascadeWorkItem<'a>>,
) {
    if ancestors.len() >= MAX_CASCADE_DEPTH {
        return;
    }
    // Non-elements and pseudo-elements never match a selector.
    if !node.is_element() || node.tag == "::before" || node.tag == "::after" {
        return;
    }
    // Results are keyed by `node_id`; a box with no DOM node behind it has none,
    // so it is left to the apply walk to match inline.
    if node.node_id != 0 {
        out.push(CascadeWorkItem {
            node: MatchNode(node),
            ancestors: ancestors.clone(),
            child_index,
            sibling_count,
            type_child_index,
            type_sibling_count,
            siblings: siblings.clone(),
            sibling_pos,
            sibling_nodes: sibling_nodes.clone(),
            raw_child_index,
        });
    }

    ancestors.push(AncestorInfo {
        tag: node.tag.clone(),
        attributes: node.attributes.clone(),
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        node_id: node.node_id,
    });

    let n_children = node.children.len();
    if n_children > 0 {
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
        let n_elem_children = node.children.iter().filter(|c| c.is_element()).count();
        let mut elem_pos = 0usize;
        let elem_indices: Vec<usize> = node
            .children
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

        // Built once for the whole sibling row: `+` and `~` read a prefix of it.
        let child_siblings = std::sync::Arc::new(
            node.children
                .iter()
                .filter(|c| c.is_element())
                .map(|c| {
                    (
                        c.tag.clone(),
                        c.attributes.get("id").cloned().unwrap_or_default(),
                        c.attributes.get("class").cloned().unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>(),
        );
        let child_nodes =
            std::sync::Arc::new(node.children.iter().map(MatchNode).collect::<Vec<_>>());
        for (i, child) in node.children.iter().enumerate() {
            let (ci, ns) = if !child.is_element() {
                (i, n_children)
            } else {
                (elem_indices[i], n_elem_children)
            };
            flatten_tree_for_cascade(
                child,
                ancestors,
                ci,
                ns,
                type_counts[i],
                type_totals[i],
                &child_siblings,
                elem_indices[i],
                &child_nodes,
                i,
                out,
            );
        }
    }

    ancestors.pop();
}

/// Parallel cascade: match every element's selectors off-thread, then run the
/// ordinary cascade walk with the answers already in hand.
///
/// 1. Flatten the DOM into a work list with ancestor snapshots (sequential)
/// 2. Match selectors via Rayon (parallel)
/// 3. `apply_cascade_inner` — the same walk the small-sheet path runs
pub fn apply_cascade_parallel(
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
    // The work items borrow `root`, so passes 1 and 2 are scoped: the immutable
    // borrow has to end before pass 3 takes the tree mutably.
    let match_map: MatchMap = {
        let mut work_items: Vec<CascadeWorkItem> = Vec::new();
        let mut ancestors: Vec<AncestorInfo> = Vec::new();
        let no_siblings = std::sync::Arc::new(Vec::new());
        let no_sibling_nodes = std::sync::Arc::new(Vec::new());
        flatten_tree_for_cascade(
            root,
            &mut ancestors,
            0,
            1,
            0,
            1,
            &no_siblings,
            0,
            &no_sibling_nodes,
            0,
            &mut work_items,
        );

        work_items
            .par_iter()
            .map(|item| {
                let mut candidates_buf: Vec<usize> = Vec::new();
                let next_nodes: Vec<&crate::types::WebCore> =
                    if item.raw_child_index + 1 < item.sibling_nodes.len() {
                        item.sibling_nodes[item.raw_child_index + 1..]
                            .iter()
                            .map(|m| m.0)
                            .collect()
                    } else {
                        Vec::new()
                    };
                let sets = match_rules(
                    item.node.0,
                    stylesheet,
                    &item.ancestors,
                    item.child_index,
                    item.sibling_count,
                    item.type_child_index,
                    item.type_sibling_count,
                    vw,
                    vh,
                    focused_box,
                    keyboard_focus,
                    hover_chain,
                    target_id,
                    document_url,
                    &item.siblings[..item.sibling_pos],
                    &item.siblings[item.sibling_pos.saturating_add(1).min(item.siblings.len())..],
                    &next_nodes,
                    &mut candidates_buf,
                );
                (item.node.0.node_id, sets)
            })
            .collect()
    };

    let mut ancestors: Vec<AncestorInfo> = Vec::new();
    let mut candidates_buf: Vec<usize> = Vec::new();
    let mut counters: HashMap<String, Vec<i32>> = HashMap::new();
    let mut share_cache = crate::css::cascade::ShareCache::new();
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
        Some(&match_map),
    );
}
