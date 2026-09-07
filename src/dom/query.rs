//! `querySelector` / `querySelectorAll` and the selector-matching walk.
//!
//! The free helpers below the `impl` are this module's own — they were the
//! first of several such groups to accumulate in `api.rs`.

use crate::types::Document;
use crate::types::WebCore;

// ─── Query ──────────────────────────────────────────────────────────────────

impl Document {
    /// Find element by its HTML `id` attribute. Returns stable node_id.
    pub fn get_element_by_id(&self, id: &str) -> Option<u32> {
        fn walk(node: &WebCore, id: &str) -> Option<u32> {
            if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
                return Some(node.node_id);
            }
            for child in &node.children {
                if let Some(found) = walk(child, id) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.root, id)
    }

    /// Query for the first element matching a CSS selector.
    pub fn query_selector(&self, selector: &str) -> Option<u32> {
        self.run_query(selector, true).first().copied()
    }

    /// Query for all elements matching a CSS selector.
    pub fn query_selector_all(&self, selector: &str) -> Vec<u32> {
        self.run_query(selector, false)
    }

    /// Shared body of `querySelector` / `querySelectorAll`.
    ///
    /// The document element is both a CANDIDATE and an ANCESTOR, and the walk
    /// used to treat it as neither — it started at the root's children with an
    /// empty ancestor chain, so `querySelector("html")` found nothing and
    /// `querySelector("html body")` had no `html` to match against.
    fn run_query(&self, selector: &str, first_only: bool) -> Vec<u32> {
        matching_ids_from_with_state(
            &self.root,
            selector,
            first_only,
            self.fragment_target_id(),
            &self.base_url,
        )
    }
}

/// Ids of the elements in `root`'s tree (including `root`) that match
/// `selector`, in document order.
///
/// This is THE selector query for the DOM surface. `dom::query_selector` and
/// friends used to run a ten-line matcher that understood `#id`, `.class`,
/// `tag` and `*` and nothing else, silently answering "no match" for every
/// combinator, pseudo-class and attribute selector — so
/// `element.matches("div p")` was false on a `<p>` inside a `<div>`, and the
/// headless browser's `find` reported an empty result for `table tbody`.
/// One question, one answer.
pub fn matching_ids_from(root: &WebCore, selector: &str, first_only: bool) -> Vec<u32> {
    matching_ids_from_with_state(root, selector, first_only, 0, "")
}

pub fn matching_ids_from_with_state(
    root: &WebCore,
    selector: &str,
    first_only: bool,
    target_id: u32,
    document_url: &str,
) -> Vec<u32> {
    {
        let selectors = parse_comma_selectors(selector);
        let empty_hover = std::collections::HashSet::new();
        let mut results = Vec::new();
        if root.is_element() && root.node_id != 0 {
            let ctx = crate::css::MatchContext {
                focused_box: 0,
                keyboard_focus: false,
                type_child_index: 0,
                type_sibling_count: 1,
                html_box: Some(root),
                hover_chain: &empty_hover,
                element_id: root.node_id,
                scope_root_id: root.node_id,
                target_id,
                document_url,
                prev_siblings: &[],
                next_siblings: &[],
            };
            for sel in &selectors {
                if crate::css::matches_selector_with_ancestors(
                    &sel.parts,
                    &root.tag,
                    &root.attributes,
                    0,
                    1,
                    &[],
                    &ctx,
                ) {
                    results.push(root.node_id);
                    if first_only {
                        return results;
                    }
                    break;
                }
            }
        }
        let root_chain = [build_ancestor_entry(root, 0, 1, 0, 1)];
        query_walk(
            root,
            &root_chain,
            &selectors,
            &empty_hover,
            root.node_id,
            target_id,
            document_url,
            first_only,
            &mut results,
        );
        results
    }
}

/// Split comma-separated selectors and parse each one.
fn parse_comma_selectors(selector: &str) -> Vec<crate::css::CssSelector> {
    selector
        .split(',')
        .map(|s| crate::css::parse_selector(s.trim()))
        .collect()
}

/// Per-child positional facts a selector can ask about.
///
/// The cascade computes all four for every element it styles; the query walk
/// used to hardcode `type_child_index: 0, type_sibling_count: 1` and pass an
/// empty `prev_siblings`, which silently broke `:nth-of-type`, `:first-of-type`
/// and BOTH sibling combinators — `querySelectorAll("p + p")` answered 0 on a
/// document with two adjacent paragraphs.
struct ChildPositions {
    /// Index among ELEMENT siblings (what `:nth-child` counts).
    elem_index: Vec<usize>,
    elem_count: usize,
    type_index: Vec<usize>,
    type_count: Vec<usize>,
}

fn child_positions(children: &[WebCore]) -> ChildPositions {
    let mut running: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let tags: Vec<String> = children
        .iter()
        .map(|c| c.tag.to_ascii_lowercase())
        .collect();
    let type_index: Vec<usize> = tags
        .iter()
        .map(|t| {
            let slot = running.entry(t.clone()).or_insert(0);
            let i = *slot;
            *slot += 1;
            i
        })
        .collect();
    let type_count: Vec<usize> = tags.iter().map(|t| *running.get(t).unwrap_or(&0)).collect();
    let mut pos = 0usize;
    let elem_index: Vec<usize> = children
        .iter()
        .map(|c| {
            if c.is_element() {
                let p = pos;
                pos += 1;
                p
            } else {
                0
            }
        })
        .collect();
    ChildPositions {
        elem_index,
        elem_count: pos,
        type_index,
        type_count,
    }
}

/// Build ancestor info for the current node.
fn build_ancestor_entry(
    node: &WebCore,
    child_index: usize,
    sibling_count: usize,
    type_child_index: usize,
    type_sibling_count: usize,
) -> crate::css::AncestorInfo {
    crate::css::AncestorInfo {
        tag: node.tag.clone(),
        attributes: node.attributes.clone(),
        child_index,
        sibling_count,
        type_child_index,
        type_sibling_count,
        node_id: node.node_id,
    }
}

/// Walk `node`'s subtree testing every ELEMENT against `selectors`.
///
/// `parent_ancestors` must already contain `node` itself — `run_query` seeds it
/// with the document element so `html body` has an `html` to match against.
///
/// Document order, depth-first — `first_only` stops at the first hit, which is
/// what `querySelector` returns.
fn query_walk(
    node: &WebCore,
    parent_ancestors: &[crate::css::AncestorInfo],
    selectors: &[crate::css::CssSelector],
    hover_chain: &std::collections::HashSet<u32>,
    scope_root_id: u32,
    target_id: u32,
    document_url: &str,
    first_only: bool,
    results: &mut Vec<u32>,
) -> bool {
    let pos = child_positions(&node.children);
    // `+` and `~` look BACKWARDS, so this accumulates as the walk moves right.
    let mut prev_siblings: Vec<(String, String, String)> = Vec::new();
    let sibling_records: Vec<(String, String, String)> = node
        .children
        .iter()
        .filter(|c| c.is_element())
        .map(|c| {
            (
                c.tag.to_ascii_lowercase(),
                c.attributes.get("id").cloned().unwrap_or_default(),
                c.attributes.get("class").cloned().unwrap_or_default(),
            )
        })
        .collect();

    for (i, child) in node.children.iter().enumerate() {
        if !child.is_element() || child.node_id == 0 {
            continue;
        }

        let ctx = crate::css::MatchContext {
            focused_box: 0,
            keyboard_focus: false,
            type_child_index: pos.type_index[i],
            type_sibling_count: pos.type_count[i],
            html_box: Some(child),
            hover_chain,
            element_id: child.node_id,
            scope_root_id,
            target_id,
            document_url,
            prev_siblings: &prev_siblings,
            next_siblings: &sibling_records[pos.elem_index[i].saturating_add(1)..],
        };

        for sel in selectors {
            if crate::css::matches_selector_with_ancestors(
                &sel.parts,
                &child.tag,
                &child.attributes,
                pos.elem_index[i],
                pos.elem_count,
                parent_ancestors,
                &ctx,
            ) {
                results.push(child.node_id);
                if first_only {
                    return true;
                }
                break; // one hit per element, however many alternatives matched
            }
        }

        let mut child_ancestors = parent_ancestors.to_vec();
        child_ancestors.push(build_ancestor_entry(
            child,
            pos.elem_index[i],
            pos.elem_count,
            pos.type_index[i],
            pos.type_count[i],
        ));
        if query_walk(
            child,
            &child_ancestors,
            selectors,
            hover_chain,
            scope_root_id,
            target_id,
            document_url,
            first_only,
            results,
        ) {
            return true;
        }

        prev_siblings.push((
            child.tag.to_ascii_lowercase(),
            child.attributes.get("id").cloned().unwrap_or_default(),
            child.attributes.get("class").cloned().unwrap_or_default(),
        ));
    }
    false
}
