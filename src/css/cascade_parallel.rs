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
use crate::css::cascade::{MatchMap, MatchSets, match_rules};
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
#[derive(Clone, Copy)]
struct MatchNode<'a>(&'a crate::types::WebCore);
unsafe impl Send for MatchNode<'_> {}
unsafe impl Sync for MatchNode<'_> {}

/// Shared sibling references for selector matching. The same read-only DOM
/// borrow invariant as `MatchNode` applies to the references in this list.
struct MatchSiblingNodes<'a>(Vec<&'a crate::types::WebCore>);
unsafe impl Send for MatchSiblingNodes<'_> {}
unsafe impl Sync for MatchSiblingNodes<'_> {}

/// One element to match, with everything the matcher needs about its position.
///
/// It borrows the node so the matcher can be handed a real `html_box`: `:has()`,
/// `:empty`, `:focus`, `:focus-within`, `:checked` and friends all read the box,
/// and a matcher without one silently answers false.
struct CascadeWorkItem<'a> {
    node: MatchNode<'a>,
    ancestors: Vec<AncestorInfo>,
    ancestor_nodes: Vec<MatchNode<'a>>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    /// The parent's element children, in order, shared by every sibling — one
    /// list per parent rather than a private copy each.
    /// `sibling_pos` is where this element sits in it, so the slice before that
    /// is what `+` and `~` look at.
    siblings: std::sync::Arc<Vec<SiblingInfo>>,
    sibling_pos: usize,
    sibling_nodes: std::sync::Arc<MatchSiblingNodes<'a>>,
    raw_child_index: usize,
}

/// Pass 1: flatten the DOM tree into a work list.
/// Each element gets its ancestor chain snapshot (needed for descendant selectors).
fn flatten_tree_for_cascade<'a>(
    node: &'a crate::types::WebCore,
    ancestors: &mut Vec<AncestorInfo>,
    ancestor_nodes: &mut Vec<MatchNode<'a>>,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
    siblings: &std::sync::Arc<Vec<SiblingInfo>>,
    sibling_pos: usize,
    sibling_nodes: &std::sync::Arc<MatchSiblingNodes<'a>>,
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
            ancestor_nodes: ancestor_nodes.clone(),
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
        attributes: std::sync::Arc::new(node.attributes.clone()),
        auto_direction: super::matching::auto_direction(node),
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        node_id: node.node_id,
        prev_siblings: std::sync::Arc::new(siblings[..sibling_pos].to_vec()),
    });
    ancestor_nodes.push(MatchNode(node));

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
                .map(SiblingInfo::from_node)
                .collect::<Vec<_>>(),
        );
        let child_nodes = std::sync::Arc::new(MatchSiblingNodes(node.children.iter().collect()));
        for (i, child) in node.children.iter().enumerate() {
            let (ci, ns) = if !child.is_element() {
                (i, n_children)
            } else {
                (elem_indices[i], n_elem_children)
            };
            flatten_tree_for_cascade(
                child,
                ancestors,
                ancestor_nodes,
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
    ancestor_nodes.pop();
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
    focus_within_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
) {
    // The work items borrow `root`, so passes 1 and 2 are scoped: the immutable
    // borrow has to end before pass 3 takes the tree mutably.
    let mut match_map = match_tree_with_ancestor_nodes(
        root,
        stylesheet,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        hover_chain,
        focus_within_chain,
        target_id,
        document_url,
    );

    let apply_started = std::time::Instant::now();
    let mut ancestors: Vec<AncestorInfo> = Vec::new();
    let mut candidates_buf: Vec<usize> = Vec::new();
    let mut counters = crate::css::cascade::CounterState::default();
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
        focus_within_chain,
        &[],
        &[],
        &[],
        &mut share_cache,
        Some(&mut match_map),
    );
    crate::profile::record(crate::profile::Phase::CascadeApply, apply_started.elapsed());
    crate::css::cascade::resolve_document_generated_content(root, stylesheet);
}

pub(crate) fn match_tree_with_ancestor_nodes(
    root: &crate::types::WebCore,
    stylesheet: &Stylesheet,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
) -> MatchMap {
    match_tree_with_ancestor_nodes_filtered(
        root,
        stylesheet,
        vw,
        vh,
        focused_box,
        keyboard_focus,
        hover_chain,
        focus_within_chain,
        target_id,
        document_url,
        None,
    )
}

pub(crate) fn match_tree_with_ancestor_nodes_filtered(
    root: &crate::types::WebCore,
    stylesheet: &Stylesheet,
    vw: f32,
    vh: f32,
    focused_box: u32,
    keyboard_focus: bool,
    hover_chain: &std::collections::HashSet<u32>,
    focus_within_chain: &std::collections::HashSet<u32>,
    target_id: u32,
    document_url: &str,
    match_nodes: Option<&std::collections::HashSet<u32>>,
) -> MatchMap {
    let flatten_started = std::time::Instant::now();
    let mut work_items: Vec<CascadeWorkItem> = Vec::new();
    let mut ancestors: Vec<AncestorInfo> = Vec::new();
    let mut ancestor_nodes = Vec::new();
    let no_siblings = std::sync::Arc::new(Vec::new());
    let no_sibling_nodes = std::sync::Arc::new(MatchSiblingNodes(Vec::new()));
    flatten_tree_for_cascade(
        root,
        &mut ancestors,
        &mut ancestor_nodes,
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
    crate::profile::record(
        crate::profile::Phase::CascadeFlatten,
        flatten_started.elapsed(),
    );

    let mut media_cache = HashMap::new();
    let media_matches: Vec<bool> = stylesheet
        .rules
        .iter()
        .map(|rule| {
            let condition = &rule.media_condition;
            condition.is_empty()
                || *media_cache
                    .entry(condition)
                    .or_insert_with(|| condition.matches(vw, vh))
        })
        .collect();
    let match_started = std::time::Instant::now();
    let matches = work_items
        .par_iter()
        .filter(|item| match_nodes.is_none_or(|ids| ids.contains(&item.node.0.node_id)))
        .map(|item| {
            let mut candidates_buf: Vec<usize> = Vec::new();
            let next_nodes = item
                .sibling_nodes
                .0
                .get(item.raw_child_index + 1..)
                .unwrap_or(&[]);
            let sets = match_rules(
                item.node.0,
                stylesheet,
                &item.ancestors,
                &item
                    .ancestor_nodes
                    .iter()
                    .map(|node| node.0)
                    .collect::<Vec<_>>(),
                item.child_index,
                item.sibling_count,
                item.type_child_index,
                item.type_sibling_count,
                vw,
                vh,
                focused_box,
                keyboard_focus,
                hover_chain,
                focus_within_chain,
                target_id,
                document_url,
                &item.siblings[..item.sibling_pos],
                &item.siblings[item.sibling_pos.saturating_add(1).min(item.siblings.len())..],
                next_nodes,
                &mut candidates_buf,
                Some(&media_matches),
            );
            (item.node.0.node_id, sets)
        })
        .collect();
    crate::profile::record(crate::profile::Phase::CascadeMatch, match_started.elapsed());
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattened_siblings_share_one_read_only_node_list() {
        let doc = crate::parse_html(
            "<div><span id='first'></span><span id='second'></span><span id='third'></span></div>",
        );
        let mut work_items = Vec::new();
        flatten_tree_for_cascade(
            &doc.root,
            &mut Vec::new(),
            &mut Vec::new(),
            0,
            1,
            0,
            1,
            &std::sync::Arc::new(Vec::new()),
            0,
            &std::sync::Arc::new(MatchSiblingNodes(Vec::new())),
            0,
            &mut work_items,
        );
        let find = |id| {
            work_items
                .iter()
                .find(|item| {
                    item.node
                        .0
                        .attributes
                        .get("id")
                        .is_some_and(|value| value == id)
                })
                .unwrap()
        };
        let first = find("first");
        let second = find("second");
        let third = find("third");
        assert!(std::sync::Arc::ptr_eq(
            &first.sibling_nodes,
            &second.sibling_nodes
        ));
        assert!(std::sync::Arc::ptr_eq(
            &second.sibling_nodes,
            &third.sibling_nodes
        ));
        assert_eq!(
            first.sibling_nodes.0[first.raw_child_index + 1]
                .attributes
                .get("id")
                .map(String::as_str),
            Some("second"),
        );
    }

    #[test]
    fn parallel_media_matching_follows_each_viewport() {
        let mut doc = crate::parse_html("<p class='target'>Text</p>");
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author(
            "@media (min-width: 600px) { .target { color: red } } \
             @media (max-width: 599px) { .target { color: blue } }",
        );
        sheet.rebuild_index();
        let empty = HashSet::new();
        let mut apply = |width| {
            apply_cascade_parallel(
                &mut doc.root,
                &sheet,
                None,
                16.0,
                width,
                600.0,
                0,
                false,
                &empty,
                &empty,
                0,
                "",
            );
            crate::dom::query_selector(&doc.root, ".target")
                .unwrap()
                .style
                .color
        };
        assert_eq!(apply(800.0), Color::rgb(255, 0, 0));
        assert_eq!(apply(400.0), Color::rgb(0, 0, 255));
    }
}
