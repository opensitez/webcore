//! Re-running the cascade and flushing layout.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

impl Document {
    /// Re-apply the CSS cascade to the entire document tree.
    /// Call this after mutating class attributes (e.g. toggling dark mode) so
    /// that `ComputedStyle` on every box is updated before the next layout pass.
    /// Resets hover/active pointers since box addresses may change after re-layout.
    pub fn recascade(&mut self) {
        // Invalidate hover/active pointers — raw pointers may alias differently
        // after WebCore trees are rebuilt or re-allocated during parsing.
        self.hovered_box = 0;
        self.active_box = 0;
        self.stylesheet.rebuild_index();
        let target_id = self.fragment_target_id();
        crate::css::apply_cascade_vp_hover_target_url(
            &mut self.root,
            &self.stylesheet,
            None,
            16.0,
            self.viewport_w,
            self.viewport_h,
            self.focused_box,
            self.keyboard_focus,
            &std::collections::HashSet::new(),
            target_id,
            &self.base_url,
        );
    }

    /// Re-apply cascade with an explicit focused element node_id.
    pub fn recascade_with_focus(&mut self, focused: u32) {
        self.focused_box = focused;
        self.hovered_box = 0;
        self.active_box = 0;
        self.stylesheet.rebuild_index();
        let target_id = self.fragment_target_id();
        crate::css::apply_cascade_vp_hover_target_url(
            &mut self.root,
            &self.stylesheet,
            None,
            16.0,
            self.viewport_w,
            self.viewport_h,
            self.focused_box,
            self.keyboard_focus,
            &std::collections::HashSet::new(),
            target_id,
            &self.base_url,
        );
    }

    /// **Flush pending style and layout**, so a geometry question is answered
    /// about the tree as it is now.
    ///
    /// CSSOM View defines its geometry on BOXES, and a node inserted or
    /// restyled since the last layout does not have one yet. A browser hides
    /// that by flushing on demand — every geometry attribute is specified to
    /// return a box, and returning one means having laid it out. Here layout
    /// ran only in the paint path, so a program that appended a control and
    /// asked for its rect in the same turn was told 0×0, and the real answer
    /// arrived a frame later with nobody left to receive it.
    ///
    /// The width is the one the document was last laid out against — the
    /// viewport its shell gave it. A document that has never been laid out has
    /// no containing block to measure against, and no box to flush to, so it is
    /// left exactly as it is rather than measured against a guess.
    pub fn flush_layout(&mut self) {
        let width = self.root.layout.last_containing_width;
        if width <= 0.0 {
            return;
        }
        self.recascade();
        LayoutEngine::new().layout(self, width);
    }
}
