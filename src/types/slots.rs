//! Assigning light-DOM children to shadow-tree slots.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

/// Resolve `<slot>` elements in a shadow tree by projecting light DOM children into them.
pub(crate) fn resolve_slots_inner(
    host_node: Option<&WebCore>,
    shadow_children: &mut Vec<WebCore>,
    light_children: &[WebCore],
    shadow_stylesheet: Option<&Stylesheet>,
) {
    let mut candidates = Vec::new();
    for child in shadow_children.iter_mut() {
        if child.tag == "slot" {
            let slot_name = child.attributes.get("name").cloned().unwrap_or_default();
            let mut projected: Vec<WebCore> = if slot_name.is_empty() {
                // Default slot: all light children without a `slot` attribute
                // Slottables are elements and non-blank text. A comment is
                // neither, so it is not projected.
                light_children
                    .iter()
                    .filter(|lc| {
                        !lc.attributes.contains_key("slot") && lc.is_element()
                            || (lc.is_text_node()
                                && !lc.text.trim().is_empty()
                                && !lc.attributes.contains_key("slot"))
                    })
                    .cloned()
                    .collect()
            } else {
                // Named slot: light children with matching `slot` attribute
                light_children
                    .iter()
                    .filter(|lc| {
                        lc.attributes
                            .get("slot")
                            .map(|s| s == &slot_name)
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            };
            if !projected.is_empty() {
                let slot_for_selector = child.clone();
                for node in &mut projected {
                    mark_projected_slot_subtree(node);
                    if let Some(sheet) = shadow_stylesheet {
                        crate::css::cascade::apply_host_projected_rules_to_projected(
                            node,
                            host_node,
                            sheet,
                            Some(&child.style),
                            0.0,
                            0.0,
                            &mut candidates,
                        );
                        crate::css::cascade::apply_slotted_rules_to_projected(
                            node,
                            Some(&slot_for_selector),
                            sheet,
                            Some(&child.style),
                            0.0,
                            0.0,
                            &mut candidates,
                        );
                    }
                }
                child.children = projected;
            }
            // If no matches, keep slot's own children as fallback
        } else {
            // Recurse into shadow tree children to find nested slots
            resolve_slots_inner(
                host_node,
                &mut child.children,
                light_children,
                shadow_stylesheet,
            );
            // Also recurse into shadow roots of nested shadow hosts
            let nested_host = child.clone();
            if let Some(ref mut sr) = child.shadow_root {
                resolve_slots_inner(
                    Some(&nested_host),
                    &mut sr.children,
                    &nested_host.children,
                    Some(&sr.stylesheet),
                );
            }
        }
    }
}

pub(crate) const PROJECTED_SLOT_MARKER: &str = "__webcore_projected_slot";

pub(crate) fn is_projected_slot_subtree(node: &WebCore) -> bool {
    node.data.contains_key(PROJECTED_SLOT_MARKER)
}

fn mark_projected_slot_subtree(node: &mut WebCore) {
    node.data
        .insert(PROJECTED_SLOT_MARKER.to_string(), "1".to_string());
    for child in &mut node.children {
        mark_projected_slot_subtree(child);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            mark_projected_slot_subtree(child);
        }
    }
}
