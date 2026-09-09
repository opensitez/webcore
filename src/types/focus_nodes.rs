//! Which nodes can take focus, and in what order.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

pub fn is_focusable_node(node: &WebCore) -> bool {
    if matches!(node.style.display, Display::None) {
        return false;
    }
    if !node.style.visibility {
        return false;
    }
    let tag = node.tag.as_str();
    matches!(tag, "button" | "input" | "textarea" | "select")
        || (tag == "a" && node.attributes.contains_key("href"))
        || (matches!(tag, "audio" | "video") && node.attributes.contains_key("controls"))
        || node.attributes.get("tabindex")
            .and_then(|v| v.parse::<i32>().ok())
            .is_some()                          // any explicit tabindex (incl. -1)
        || node.attributes.get("contenteditable")
            .map(|v| v == "true" || v == "")
            .unwrap_or(false)
}

/// Walk the box tree and split focusable elements into two buckets for tab ordering:
/// - `positive`: elements with explicit `tabindex > 0`, paired with their index value
/// - `normal`:   native-focusable elements and `tabindex=0` elements, in document order
///
/// Elements with `tabindex=-1` are excluded (programmatically focusable only).
pub(crate) fn collect_focusable_ordered(
    node: &WebCore,
    positive: &mut Vec<(u32, i32)>,
    normal: &mut Vec<u32>,
) {
    if matches!(node.style.display, Display::None) {
        return;
    }
    if !node.style.visibility {
        return;
    }
    let tag = node.tag.as_str();

    let tabindex = node
        .attributes
        .get("tabindex")
        .and_then(|v| v.parse::<i32>().ok());

    // Determine whether this element is in the tab order.
    let native = matches!(tag, "button" | "input" | "textarea" | "select")
        || (tag == "a" && node.attributes.contains_key("href"))
        || (matches!(tag, "audio" | "video") && node.attributes.contains_key("controls"))
        || node
            .attributes
            .get("contenteditable")
            .map(|v| v == "true" || v == "")
            .unwrap_or(false);

    match tabindex {
        Some(n) if n > 0 => positive.push((node.node_id, n)),
        Some(0) => normal.push(node.node_id),
        Some(_) => {} // tabindex < 0: excluded from tab order
        None if native => normal.push(node.node_id),
        None => {}
    }

    for child in &node.children {
        collect_focusable_ordered(child, positive, normal);
    }
}
