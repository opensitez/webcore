//! Table structure normalization — HTML §13.2.6.4.9 "in table".

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Table structure (HTML §13.2.6.4.9 "in table") ─────────────────────────

/// May this node sit directly inside a `<table>`?
///
/// The spec's "in table" insertion mode handles exactly these; anything else is
/// foster-parented out. Whitespace-only text is kept — it is the newline
/// between `<table>` and `<tr>` that every hand-written table has, and moving
/// it would put a stray text node before every table on the web.
fn allowed_in_table(node: &WebCore) -> bool {
    match node.tag.as_str() {
        "caption" | "colgroup" | "col" | "thead" | "tbody" | "tfoot" | "tr" | "form" | "script"
        | "template" | "style" | "#comment" => true,
        // A bare cell is table content: the row it needs is IMPLIED, not
        // missing (see `wrap_bare_cells_in_row`). Fostering it out instead
        // emptied `<table><td>x</td></table>` of everything it had.
        "td" | "th" => true,
        "#text" => node.text.trim().is_empty(),
        _ => false,
    }
}

/// Give bare `<td>`/`<th>` children of a table an implied `<tr>`.
///
/// `<table><td>x</td></table>` is `table > tbody > tr > td` in every browser —
/// the "in table" mode inserts the row the author left out, the same way it
/// inserts the `<tbody>`. Without it the cells were not table content, so they
/// were foster-parented out and the table came back empty.
fn wrap_bare_cells_in_row(table: &mut WebCore) {
    if !table
        .children
        .iter()
        .any(|c| c.tag == "td" || c.tag == "th")
    {
        return;
    }
    let children = std::mem::take(&mut table.children);
    let mut out: Vec<WebCore> = Vec::new();
    let mut row: Option<WebCore> = None;
    for child in children {
        if child.tag == "td" || child.tag == "th" {
            let r = row.get_or_insert_with(|| {
                let mut n = WebCore::new("tr");
                apply_property(
                    std::sync::Arc::make_mut(&mut n.style),
                    "display",
                    default_display("tr"),
                );
                n
            });
            r.children.push(child);
        } else {
            if let Some(r) = row.take() {
                out.push(r);
            }
            out.push(child);
        }
    }
    if let Some(r) = row.take() {
        out.push(r);
    }
    table.children = out;
}

/// Group a table's stray `<tr>` children into implied `<tbody>` elements.
///
/// `<table><tr>` parses to `table > tbody > tr` in every browser: the tree
/// builder inserts the `<tbody>` the author left out. Without it the DOM is a
/// shape no real page ever sees — `table > tr` — so `tbody` selectors,
/// `:nth-child`, and `HTMLTableElement.tBodies` all disagree with a browser.
///
/// Consecutive runs are grouped into ONE `<tbody>`, and a run is not broken by
/// the whitespace between rows, so `<table>\n<tr>…\n<tr>…\n</table>` yields a
/// single tbody like it should rather than one per row.
fn group_rows_into_tbody(table: &mut WebCore) {
    wrap_bare_cells_in_row(table);
    if !table.children.iter().any(|c| c.tag == "tr") {
        return;
    }
    let children = std::mem::take(&mut table.children);
    let mut out: Vec<WebCore> = Vec::new();
    let mut current: Option<WebCore> = None;
    // Whitespace held back, so it only joins a run that actually continues.
    let mut pending_ws: Vec<WebCore> = Vec::new();
    for child in children {
        let is_ws = child.is_text_node() && child.text.trim().is_empty();
        if child.tag == "tr" {
            let tbody = current.get_or_insert_with(|| {
                let mut b = WebCore::new("tbody");
                apply_property(
                    std::sync::Arc::make_mut(&mut b.style),
                    "display",
                    default_display("tbody"),
                );
                b
            });
            tbody.children.append(&mut pending_ws);
            tbody.children.push(child);
        } else if is_ws && current.is_some() {
            pending_ws.push(child);
        } else {
            if let Some(tbody) = current.take() {
                out.push(tbody);
            }
            out.append(&mut pending_ws);
            out.push(child);
        }
    }
    if let Some(tbody) = current.take() {
        out.push(tbody);
    }
    out.append(&mut pending_ws);
    table.children = out;
}

fn group_cols_into_colgroup(table: &mut WebCore) {
    if !table.children.iter().any(|child| child.tag == "col") {
        return;
    }
    let children = std::mem::take(&mut table.children);
    let mut out = Vec::with_capacity(children.len());
    let mut current: Option<WebCore> = None;
    let mut pending_ws = Vec::new();
    for child in children {
        if child.tag == "col" {
            let group = current.get_or_insert_with(|| {
                let mut group = WebCore::new("colgroup");
                apply_property(
                    std::sync::Arc::make_mut(&mut group.style),
                    "display",
                    default_display("colgroup"),
                );
                group
            });
            group.children.append(&mut pending_ws);
            group.children.push(child);
        } else if child.is_text_node() && child.text.trim().is_empty() && current.is_some() {
            pending_ws.push(child);
        } else {
            if let Some(group) = current.take() {
                out.push(group);
            }
            out.append(&mut pending_ws);
            out.push(child);
        }
    }
    if let Some(group) = current {
        out.push(group);
    }
    out.append(&mut pending_ws);
    table.children = out;
}

/// Valid parents for an element that only belongs inside a table.
/// `None` when the tag is not table-only and may appear anywhere.
fn table_part_parents(tag: &str) -> Option<&'static [&'static str]> {
    match tag {
        "caption" | "colgroup" | "thead" | "tbody" | "tfoot" => Some(&["table"]),
        "tr" => Some(&["table", "thead", "tbody", "tfoot"]),
        "td" | "th" => Some(&["tr"]),
        "col" => Some(&["colgroup"]),
        _ => None,
    }
}

/// Drop table parts that are not inside a table, keeping their content.
///
/// `<div><td>orphan</td></div>` has no table anywhere, and the "in body"
/// insertion mode ignores a `<td>` start tag outright — so a browser keeps the
/// text and no cell. We were building the element, which put a `display:
/// table-cell` box in the middle of a block flow and made `querySelector("td")`
/// answer on a document with no table in it.
pub(crate) fn unwrap_misplaced_table_parts(node: &mut WebCore) {
    if node.tag == "template" {
        return;
    }
    for child in &mut node.children {
        unwrap_misplaced_table_parts(child);
    }
    let parent_tag = node.tag.clone();
    // Index-based and IN PLACE. A `WebCore` is ~4KB, so moving one into a local
    // costs 4KB of STACK per recursion level — a page 80 elements deep
    // overflowed the stack before it finished parsing. Nothing here holds a
    // node by value.
    let mut i = 0;
    while i < node.children.len() {
        let misplaced = table_part_parents(&node.children[i].tag)
            .map(|ok| !ok.contains(&parent_tag.as_str()))
            .unwrap_or(false);
        if misplaced {
            // The element is ignored and its CONTENT takes its place — then the
            // loop re-examines that content against this parent, because
            // promoting a `<tr>`'s children leaves `<td>`s somewhere that
            // cannot hold them either. `i` deliberately does not advance.
            let promoted = std::mem::take(&mut node.children[i].children);
            node.children.splice(i..=i, promoted);
        } else {
            i += 1;
        }
    }
}

/// Move a table-level `<form>`'s children out into the table.
///
/// HTML §13.2.6.4.9 keeps the `<form>` (the form element pointer is set) but
/// inserts nothing into it — the rows that follow are inserted into the TABLE.
/// Chrome gives `table > [form, tbody > tr]`; we were nesting the rows inside
/// the form, which put a block box between the table and its rows.
fn hoist_table_form_children(table: &mut WebCore) {
    if !table
        .children
        .iter()
        .any(|c| c.tag == "form" && !c.children.is_empty())
    {
        return;
    }
    let children = std::mem::take(&mut table.children);
    let mut out = Vec::with_capacity(children.len());
    for mut child in children {
        if child.tag == "form" {
            let inner = std::mem::take(&mut child.children);
            out.push(child);
            out.extend(inner);
        } else {
            out.push(child);
        }
    }
    table.children = out;
}

/// Apply the table fix-ups to `node`'s subtree.
///
/// Foster parenting first: content that may not sit in a table is moved out to
/// just BEFORE the table, in order, as a sibling. `<div><table>stray<tr>…`
/// becomes `<div>stray<table><tbody><tr>…` — which is what Chrome produces, and
/// why the text used to render inside the table box instead of above it.
pub(crate) fn normalize_tables(node: &mut WebCore) {
    for child in &mut node.children {
        normalize_tables(child);
    }
    if !node.children.iter().any(|c| c.tag == "table") {
        return;
    }
    // In place, by index: a `WebCore` is ~4KB and this recurses once per level,
    // so moving nodes through locals put kilobytes on the stack per element and
    // a deep page overflowed before it finished parsing.
    let mut i = 0;
    while i < node.children.len() {
        if node.children[i].tag != "table" {
            i += 1;
            continue;
        }
        // Foster parenting: content that may not sit in a table moves out to
        // just BEFORE the table, in order, as a sibling.
        let mut fostered: Vec<WebCore> = Vec::new();
        let mut k = 0;
        while k < node.children[i].children.len() {
            if allowed_in_table(&node.children[i].children[k]) {
                k += 1;
            } else {
                fostered.push(node.children[i].children.remove(k));
            }
        }
        hoist_table_form_children(&mut node.children[i]);
        group_cols_into_colgroup(&mut node.children[i]);
        group_rows_into_tbody(&mut node.children[i]);
        let moved = fostered.len();
        node.children.splice(i..i, fostered);
        i += moved + 1;
    }
}
