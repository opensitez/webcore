//! What a document still names, and what nothing does.
//!
//! ⛔ **This module enumerates. It frees nothing, on purpose.**
//!
//! A detached node is garbage only when NOTHING holds it, and webcore can see
//! only half of that: its own tree and its own id-keyed state. The other half
//! is script, which holds these ids as opaque numbers through the embedder —
//! and webcore is a standalone crate with no dependency on any runtime, so it
//! cannot look. An API that freed by default would therefore free on an
//! INCOMPLETE root set, which does not under-reclaim: it hands a live script
//! reference a dead node.
//!
//! So the shape is the safe way round. [`Document::detached_candidates`] says
//! what is unreachable from webcore's own roots; a caller that also knows what
//! script holds intersects the two and frees explicitly. An embedder that
//! knows less than everything then reclaims less than everything, which is the
//! failure a memory routine should have.

use std::collections::HashSet;

impl crate::types::Document {
    /// Every node id THIS DOCUMENT still names — the render tree plus every
    /// piece of id-keyed state.
    ///
    /// ⛔ The stores are not all keyed the same way. `TraversalStore` and
    /// `RangeStore` map a HANDLE to a value that holds the node ids, so their
    /// keys are not nodes and their values must be read; `EventTargetMap` is
    /// keyed by node. Each has a `node_ids()` that answers for itself rather
    /// than this walking their internals.
    ///
    /// ⛔ `node_index` is deliberately absent: it is a derived cache, rebuilt
    /// from the tree, so treating it as a root would make every stale entry
    /// immortal.
    pub fn retained_ids(&self) -> HashSet<u32> {
        let mut out = HashSet::new();

        fn walk(n: &crate::types::WebCore, out: &mut HashSet<u32>) {
            out.insert(n.node_id);
            for c in n.effective_children() {
                walk(c, out);
            }
            // A host's LIGHT children are still the document's, even when the
            // shadow tree is what renders — `effective_children` answers one or
            // the other, never both. A slotted child is reached twice, which a
            // set absorbs.
            if let Some(sr) = &n.shadow_root {
                // ⛔ The ShadowRoot's OWN id. It is a node in the spec — a
                // `DocumentFragment` — and `attachShadow` returns it, so a
                // caller can hold it. It is in neither child list.
                out.insert(sr.node_id);
                for c in &n.children {
                    walk(c, out);
                }
            }
        }
        walk(&self.root, &mut out);

        // Interaction and focus state.
        for id in [
            self.hovered_box,
            self.prev_hovered_box,
            self.active_box,
            self.focused_box,
            self.mousedown_target,
            self.last_click_target,
            self.drag_source,
            self.doctype,
            self.open_select,
            self.open_picker,
            self.dragging_range,
        ] {
            out.insert(id);
        }
        out.extend(self.hover_sensitive_nodes.iter().copied());
        out.extend(self.top_layer.iter().copied());
        out.extend(self.custom_validity.keys().copied());
        out.extend(self.event_targets.node_ids());
        out.extend(self.traversals.node_ids());
        out.extend(self.ranges.node_ids());
        out.extend(self.active_animations.iter().map(|a| a.element_id));
        out.extend(self.finished_animations.iter().map(|a| a.element_id));
        out.extend(self.editor.caret_box);

        // A detached node the document is still holding for re-insertion, and
        // everything under it — `remove_child` hands the node back and the
        // caller may put it anywhere.
        for node in self.pending_nodes.values() {
            walk(node, &mut out);
        }

        out.remove(&0);
        out
    }

    /// Arena nodes this document no longer names.
    ///
    /// **Not garbage — candidates.** Whether one can be freed depends on
    /// whether script still holds it, which only the embedder knows. See the
    /// module note.
    ///
    /// ⛔ The `is_alive` filter is load-bearing for the FUTURE caller, not
    /// decoration. Nothing frees today, so every slot ever allocated is alive
    /// and it looks redundant — the moment an embedder starts freeing, this is
    /// the only thing separating a reclaimed slot from a detached-but-held one.
    /// (An id is never reissued, so a dead slot stays dead.)
    pub fn detached_candidates(&self) -> Vec<u32> {
        let named = self.retained_ids();
        (1..self.arena.len() as u32)
            .filter(|id| self.arena.is_alive(crate::dom::arena::NodeId(*id)) && !named.contains(id))
            .collect()
    }
}
