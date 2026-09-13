//! `TreeWalker` and `NodeIterator` — DOM §6.
//!
//! Every expectation is a Chrome measurement (`/tmp/webcore-html/tw.html`,
//! `tw2.html`, `tw3.html`). The suite is built around the four places the two
//! objects DISAGREE, because a test that exercises only one of them cannot
//! tell them apart:
//!
//!   * the walker never returns its root, the iterator returns it first;
//!   * `REJECT` prunes a subtree for the walker and means `SKIP` for the
//!     iterator;
//!   * `previousNode` from the end differs by exactly one node, and only
//!     because `pointerBeforeReferenceNode` exists;
//!   * removal moves an iterator's reference and leaves a walker's alone.

use crate::dom::traversal::*;
use crate::html::parse_html;
use crate::types::Document;

const PAGE: &str = "<div id=root><p id=p1>t1<b id=b1>t2</b>t3</p><!--c1--><p id=p2>t4</p><span id=s1><i id=i1>t5</i></span></div>";

fn page() -> Document {
    parse_html(PAGE)
}
fn el(d: &Document, id: &str) -> u32 {
    d.get_element_by_id(id).unwrap()
}

/// A node's `id`, or its `nodeName` when it has none — the same shorthand the
/// Chrome probes printed, so the tables can be compared line for line.
fn nm(d: &Document, id: u32) -> String {
    match d.get_attribute(id, "id") {
        Some(v) if !v.is_empty() => v,
        _ => d.node_name(id),
    }
}

fn walk_all(d: &mut Document, t: u32) -> Vec<String> {
    let mut out = vec![nm(d, d.current_node(t).unwrap())];
    while let Some(n) = d.tw_next_node(t) {
        out.push(nm(d, n));
    }
    out
}

fn iter_all(d: &mut Document, t: u32) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(n) = d.ni_next_node(t) {
        out.push(nm(d, n));
    }
    out
}

// ─── whatToShow ─────────────────────────────────────────────────────────────

#[test]
fn what_to_show_selects_which_node_kinds_the_walk_returns() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    assert_eq!(walk_all(&mut d, t), ["root", "p1", "b1", "p2", "s1", "i1"]);

    let t = d.create_tree_walker(root, SHOW_COMMENT, None);
    assert_eq!(walk_all(&mut d, t), ["root", "#comment"]);

    let t = d.create_tree_walker(root, SHOW_TEXT, None);
    let texts = walk_all(&mut d, t);
    assert_eq!(
        texts[0], "root",
        "currentNode starts at the root whatever whatToShow says"
    );
    assert_eq!(texts.len(), 6, "five text nodes plus the root: {texts:?}");
}

#[test]
fn what_to_show_zero_matches_nothing() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, 0, None);
    assert_eq!(d.tw_next_node(t), None);
}

#[test]
fn the_filter_is_never_called_for_a_node_what_to_show_excluded() {
    // ⛔ The ORDER of the two checks is observable, and only by counting.
    // Chrome ran the filter for four elements and for none of the text nodes.
    use std::sync::{Arc, Mutex};
    let mut d = page();
    let root = el(&d, "root");
    let seen = Arc::new(Mutex::new(Vec::<u16>::new()));
    let rec = Arc::clone(&seen);
    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(move |doc: &Document, n: u32| {
            rec.lock().unwrap().push(doc.node_type(n));
            FILTER_ACCEPT
        })),
    );
    while d.tw_next_node(t).is_some() {}
    let kinds = seen.lock().unwrap().clone();
    assert_eq!(
        kinds.len(),
        5,
        "one call per ELEMENT below the root, got {kinds:?}"
    );
    assert!(
        kinds.iter().all(|k| *k == 1),
        "only elements reached the filter: {kinds:?}"
    );
}

// ─── the walker/iterator disagreements ──────────────────────────────────────

#[test]
fn the_walker_never_returns_its_root_and_the_iterator_returns_it_first() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    assert_eq!(
        {
            let n = d.tw_next_node(t).unwrap();
            nm(&d, n)
        },
        "p1",
        "the walker starts BELOW the root"
    );

    let it = d.create_node_iterator(root, SHOW_ELEMENT, None);
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "root",
        "the iterator returns it"
    );
}

#[test]
fn the_root_is_never_filtered() {
    use std::sync::{Arc, Mutex};
    let mut d = page();
    let root = el(&d, "root");
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let rec = Arc::clone(&seen);
    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(move |doc: &Document, n: u32| {
            rec.lock().unwrap().push(nm(doc, n));
            FILTER_ACCEPT
        })),
    );
    d.tw_next_node(t);
    assert_eq!(
        *seen.lock().unwrap(),
        ["p1"],
        "the walk moves off the root before asking"
    );
}

#[test]
fn reject_prunes_a_subtree_for_the_walker_and_skip_does_not() {
    // The discriminating pair: `b1` is INSIDE `p1`, so it survives a SKIP and
    // disappears under a REJECT. One verdict alone cannot show this.
    let mut d = page();
    let root = el(&d, "root");
    let p1 = el(&d, "p1");

    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(
            move |_: &Document, n: u32| {
                if n == p1 { FILTER_SKIP } else { FILTER_ACCEPT }
            },
        )),
    );
    assert_eq!(walk_all(&mut d, t), ["root", "b1", "p2", "s1", "i1"]);

    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(move |_: &Document, n: u32| {
            if n == p1 {
                FILTER_REJECT
            } else {
                FILTER_ACCEPT
            }
        })),
    );
    assert_eq!(
        walk_all(&mut d, t),
        ["root", "p2", "s1", "i1"],
        "b1 went with p1"
    );
}

#[test]
fn reject_is_exactly_skip_for_an_iterator() {
    // ⛔ An iterator walks flat tree order and has no subtree to prune, so the
    // two verdicts CANNOT differ. Measured: `b1` survives rejecting `p1`.
    let mut d = page();
    let root = el(&d, "root");
    let p1 = el(&d, "p1");
    let it = d.create_node_iterator(
        root,
        SHOW_ELEMENT,
        Some(Box::new(move |_: &Document, n: u32| {
            if n == p1 {
                FILTER_REJECT
            } else {
                FILTER_ACCEPT
            }
        })),
    );
    assert_eq!(iter_all(&mut d, it), ["root", "b1", "p2", "s1", "i1"]);
}

#[test]
fn previous_node_from_the_end_differs_by_one_between_the_two_objects() {
    // The cheapest proof that `pointerBeforeReferenceNode` is real state and
    // not derived: the iterator's pointer sits AFTER its reference, so
    // stepping back lands on the reference itself.
    let mut d = page();
    let root = el(&d, "root");

    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    while d.tw_next_node(t).is_some() {}
    let mut back = Vec::new();
    while let Some(n) = d.tw_previous_node(t) {
        back.push(nm(&d, n));
    }
    assert_eq!(back, ["s1", "p2", "b1", "p1", "root"]);

    let it = d.create_node_iterator(root, SHOW_ELEMENT, None);
    while d.ni_next_node(it).is_some() {}
    let mut back = Vec::new();
    while let Some(n) = d.ni_previous_node(it) {
        back.push(nm(&d, n));
    }
    assert_eq!(
        back,
        ["i1", "s1", "p2", "b1", "p1", "root"],
        "i1 comes back first"
    );
}

// ─── the walker's navigation members ────────────────────────────────────────

#[test]
fn first_child_next_sibling_and_parent_node_move_the_current_node() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    assert_eq!(
        d.tw_first_child(t).map(|n| nm(&d, n)).as_deref(),
        Some("p1")
    );
    assert_eq!(
        d.tw_next_sibling(t).map(|n| nm(&d, n)).as_deref(),
        Some("p2")
    );
    assert_eq!(
        d.tw_parent_node(t).map(|n| nm(&d, n)).as_deref(),
        Some("root")
    );

    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    assert_eq!(d.tw_last_child(t).map(|n| nm(&d, n)).as_deref(), Some("s1"));
}

#[test]
fn a_navigation_that_finds_nothing_leaves_the_current_node_where_it_was() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    d.tw_first_child(t);
    assert_eq!(d.tw_previous_sibling(t), None);
    assert_eq!(nm(&d, d.current_node(t).unwrap()), "p1", "still on p1");

    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    assert_eq!(
        d.tw_parent_node(t),
        None,
        "the root has no parent inside the walk"
    );
    assert_eq!(nm(&d, d.current_node(t).unwrap()), "root");
}

#[test]
fn first_child_descends_through_a_skip_and_steps_over_a_reject() {
    let mut d = page();
    let root = el(&d, "root");
    let p1 = el(&d, "p1");

    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(
            move |_: &Document, n: u32| {
                if n == p1 { FILTER_SKIP } else { FILTER_ACCEPT }
            },
        )),
    );
    assert_eq!(
        d.tw_first_child(t).map(|n| nm(&d, n)).as_deref(),
        Some("b1")
    );

    let t = d.create_tree_walker(
        root,
        SHOW_ELEMENT,
        Some(Box::new(move |_: &Document, n: u32| {
            if n == p1 {
                FILTER_REJECT
            } else {
                FILTER_ACCEPT
            }
        })),
    );
    assert_eq!(
        d.tw_first_child(t).map(|n| nm(&d, n)).as_deref(),
        Some("p2")
    );
}

#[test]
fn a_root_with_no_matching_descendants_yields_nothing() {
    let mut d = page();
    let p2 = el(&d, "p2");
    let t = d.create_tree_walker(p2, SHOW_ELEMENT, None);
    assert_eq!(d.tw_next_node(t), None);
    let t = d.create_tree_walker(p2, SHOW_TEXT, None);
    assert_eq!(
        d.tw_first_child(t).map(|n| d.node_type(n)),
        Some(3),
        "its text node"
    );
}

#[test]
fn the_walk_stops_at_the_root_rather_than_escaping_it() {
    let mut d = page();
    let s1 = el(&d, "s1");
    let t = d.create_tree_walker(s1, SHOW_ELEMENT, None);
    assert_eq!(d.tw_next_node(t).map(|n| nm(&d, n)).as_deref(), Some("i1"));
    assert_eq!(d.tw_next_node(t), None, "p2 is outside this root");
}

#[test]
fn first_child_stops_at_the_node_the_walk_started_from() {
    // ⛔ A mutation found this: dropping the "do not climb past the start"
    // guard left `firstChild()` escaping into the NEXT subtree. Chrome
    // answers null and leaves `currentNode` where it was — the walk is over
    // p1's children, and p2 is not one of them.
    let mut d = deep2();
    let r = el(&d, "r");
    let p1 = el(&d, "p1");
    let b1 = el(&d, "b1");
    for last in [false, true] {
        let t = d.create_tree_walker(
            r,
            SHOW_ELEMENT,
            Some(Box::new(move |_: &Document, n: u32| {
                if n == b1 {
                    FILTER_REJECT
                } else {
                    FILTER_ACCEPT
                }
            })),
        );
        d.set_current_node(t, p1);
        let got = if last {
            d.tw_last_child(t)
        } else {
            d.tw_first_child(t)
        };
        assert_eq!(got, None, "last={last}");
        assert_eq!(d.current_node(t), Some(p1), "and currentNode did not move");
    }
}

#[test]
fn first_child_through_a_skip_still_stops_at_the_start() {
    // The SKIP road to the same edge: b1 and i1 both skipped, so descending
    // finds nothing and the climb back out must not leave p1.
    let mut d = deep2();
    let r = el(&d, "r");
    let p1 = el(&d, "p1");
    let b1 = el(&d, "b1");
    let i1 = el(&d, "i1");
    let t = d.create_tree_walker(
        r,
        SHOW_ELEMENT,
        Some(Box::new(move |_: &Document, n: u32| {
            if n == b1 || n == i1 {
                FILTER_SKIP
            } else {
                FILTER_ACCEPT
            }
        })),
    );
    d.set_current_node(t, p1);
    assert_eq!(d.tw_first_child(t), None);
    assert_eq!(d.current_node(t), Some(p1));
}

// ─── the iterator's reference pointer ───────────────────────────────────────

#[test]
fn the_iterator_pointer_starts_before_its_reference_and_flips_on_the_first_step() {
    let mut d = page();
    let root = el(&d, "root");
    let it = d.create_node_iterator(root, SHOW_ELEMENT, None);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "root");
    assert_eq!(d.pointer_before_reference_node(it), Some(true));
    d.ni_next_node(it);
    assert_eq!(
        nm(&d, d.reference_node(it).unwrap()),
        "root",
        "still the root"
    );
    assert_eq!(
        d.pointer_before_reference_node(it),
        Some(false),
        "the pointer moved past it"
    );
}

#[test]
fn detach_does_nothing_at_all() {
    let mut d = page();
    let root = el(&d, "root");
    let it = d.create_node_iterator(root, SHOW_ELEMENT, None);
    d.traversal_detach(it);
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "root",
        "still works afterwards"
    );
}

// ─── removal: the pre-removing steps ────────────────────────────────────────

const DEEP: &str =
    "<div id=r><p id=p1><b id=b1><i id=i1></i></b></p><p id=p2></p><p id=p3></p></div>";

fn deep() -> Document {
    parse_html(DEEP)
}

/// Two sibling subtrees, so a removal has a PREVIOUS SIBLING with descendants
/// of its own — the shape that separates "the parent" from "the previous
/// sibling's deepest descendant".
const DEEP2: &str = "<div id=r><p id=p1><b id=b1><i id=i1></i></b></p><p id=p2><b id=b2><i id=i2></i></b></p><p id=p3></p></div>";

fn deep2() -> Document {
    parse_html(DEEP2)
}

#[test]
fn removing_an_ancestor_of_the_reference_moves_it_back_past_the_subtree() {
    // Measured: reference `b1`, remove `p1` → reference `r`, pointer after it,
    // next node `p2`. The rule is INCLUSIVE ANCESTOR, not "was the reference".
    let mut d = deep();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    d.ni_next_node(it);
    d.ni_next_node(it);
    d.ni_next_node(it); // r, p1, b1
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "b1");
    let p1 = el(&d, "p1");
    d.remove_child(p1);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "r");
    assert_eq!(d.pointer_before_reference_node(it), Some(false));
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "p2"
    );
}

#[test]
fn removing_a_deep_reference_walks_all_the_way_out_of_the_removed_subtree() {
    let mut d = deep();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    for _ in 0..4 {
        d.ni_next_node(it);
    } // r, p1, b1, i1
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "i1");
    let p1 = el(&d, "p1");
    d.remove_child(p1);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "r");
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "p2"
    );
}

#[test]
fn removing_a_node_that_is_not_an_ancestor_leaves_the_reference_alone() {
    // Both directions: a PRECEDING sibling and a FOLLOWING one. This is the
    // pair that separates the real rule from "if anything was removed, back up".
    let mut d = deep();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    for _ in 0..5 {
        d.ni_next_node(it);
    } // r, p1, b1, i1, p2
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "p2");
    let p1 = el(&d, "p1");
    d.remove_child(p1);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "p2", "untouched");
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "p3"
    );

    let mut d = deep();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    d.ni_next_node(it);
    d.ni_next_node(it); // r, p1
    let p3 = el(&d, "p3");
    d.remove_child(p3);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "p1");
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "b1"
    );
}

#[test]
fn with_the_pointer_before_the_reference_the_removal_moves_it_forward_instead() {
    // ⛔ The other branch entirely. Measured: reference `p1` with the pointer
    // BEFORE it, remove `p1` → reference `p2` with the pointer STILL before,
    // so the next step returns `p2` itself rather than what follows it.
    let mut d = deep();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    d.ni_next_node(it);
    d.ni_next_node(it);
    d.ni_previous_node(it);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "p1");
    assert_eq!(d.pointer_before_reference_node(it), Some(true));
    let p1 = el(&d, "p1");
    d.remove_child(p1);
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "p2");
    assert_eq!(d.pointer_before_reference_node(it), Some(true));
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "p2"
    );
}

#[test]
fn the_reference_lands_on_the_previous_siblings_deepest_descendant() {
    // ⛔ A mutation found this too: answering the PARENT is right only when
    // there is no previous sibling. Measured: removing `p2` with reference
    // `i2` lands on `i1` — the last inclusive descendant of `p1` — not on `r`.
    let mut d = deep2();
    let r = el(&d, "r");
    let it = d.create_node_iterator(r, SHOW_ELEMENT, None);
    for _ in 0..7 {
        d.ni_next_node(it);
    } // r, p1, b1, i1, p2, b2, i2
    assert_eq!(nm(&d, d.reference_node(it).unwrap()), "i2");
    let p2 = el(&d, "p2");
    d.remove_child(p2);
    assert_eq!(
        nm(&d, d.reference_node(it).unwrap()),
        "i1",
        "not the parent r"
    );
    assert_eq!(d.pointer_before_reference_node(it), Some(false));
    assert_eq!(
        {
            let n = d.ni_next_node(it).unwrap();
            nm(&d, n)
        },
        "p3"
    );
}

#[test]
fn a_tree_walker_is_not_told_about_removals() {
    // Measured: the walker's currentNode stays on the DETACHED node and
    // `nextNode()` then answers null. A walker has no pre-removing steps —
    // giving it the iterator's would be inventing behaviour.
    let mut d = deep();
    let r = el(&d, "r");
    let t = d.create_tree_walker(r, SHOW_ELEMENT, None);
    d.tw_next_node(t);
    let p1 = el(&d, "p1");
    assert_eq!(d.current_node(t), Some(p1));
    d.remove_child(p1);
    assert_eq!(
        d.current_node(t),
        Some(p1),
        "still pointing at the detached node"
    );
}

// ─── the handle's own surface ───────────────────────────────────────────────

#[test]
fn the_handle_reports_what_it_was_created_with() {
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT | SHOW_TEXT, None);
    assert_eq!(d.traversal_root(t), Some(root));
    assert_eq!(d.traversal_what_to_show(t), Some(SHOW_ELEMENT | SHOW_TEXT));
    assert!(!d.traversal_has_filter(t));
    let t2 = d.create_tree_walker(
        root,
        SHOW_ALL,
        Some(Box::new(|_: &Document, _| FILTER_ACCEPT)),
    );
    assert!(d.traversal_has_filter(t2));
    assert_ne!(t, t2, "each handle is distinct");
}

#[test]
fn current_node_can_be_set_outside_the_root() {
    // Chrome allows it and carries on from there.
    let mut d = page();
    let root = el(&d, "root");
    let b1 = el(&d, "b1");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    d.set_current_node(t, b1);
    assert_eq!(d.tw_next_node(t).map(|n| nm(&d, n)).as_deref(), Some("p2"));
}

#[test]
fn a_cloned_document_carries_no_traversals() {
    // ⛔ A filter is a `Box<dyn FnMut>` and does not clone, so the store starts
    // empty — the same trade `event_targets` makes. Pinned because "the filter
    // silently vanished" is otherwise invisible.
    let mut d = page();
    let root = el(&d, "root");
    let t = d.create_tree_walker(root, SHOW_ELEMENT, None);
    let clone = d.clone();
    assert_eq!(
        clone.traversal_root(t),
        None,
        "the handle names nothing in the clone"
    );
    assert_eq!(
        d.traversal_root(t),
        Some(root),
        "and still works in the original"
    );
}

#[test]
fn a_parsed_comment_is_a_comment_node_not_an_element() {
    // ⛔ Found through `SHOW_COMMENT`, not by looking. `new_box` special-cased
    // `#text` alone, so every comment the PARSER built was an arena Element:
    // `nodeType` answered 1 and `nodeName` came back `"#COMMENT"`, uppercased
    // by the element rule. Chrome: `[8, "#comment", "c"]`.
    let d = parse_html("<div id=h><!--c--></div>");
    let host = d.get_element_by_id("h").unwrap();
    let comment = d.child_nodes(host)[0];
    assert_eq!(d.node_type(comment), 8);
    assert_eq!(
        d.node_name(comment),
        "#comment",
        "not uppercased — it is not an element"
    );
    assert_eq!(d.text_data(comment), "c", "and it kept its data");

    // The created form was always right; both roads must now agree.
    let mut d2 = parse_html("<div id=h></div>");
    let made = d2.create_comment("c");
    assert_eq!(d2.node_type(made), d.node_type(comment));
    assert_eq!(d2.node_name(made), d.node_name(comment));
}
