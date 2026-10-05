//! The top layer and the popover API — HTML §6.12, CSS Position §6.
//!
//! Split out of `api.rs`, which had grown to twenty-five `impl Document`
//! blocks. The state lives on `Document`; this is the behaviour over it.

use crate::types::Document;

// ─── The top layer, and the popover API (HTML §6.12, CSS Position §6) ───────
//
// One ordered list on the document plus a mirror field on each node, written
// by the two functions below and nowhere else — `:modal` and `:popover-open`
// read the node, and light dismiss reads the order.
//
// The light-dismiss rule is narrower than "popovers close each other", and all
// four rows were measured:
//
//   opening `auto`   → closes every other `auto` AND `hint`
//   opening `hint`   → closes other `hint`s only, and leaves `auto` alone
//   opening `manual` → closes nothing
//   `manual`         → is closed by nothing
//
// `beforetoggle` is CANCELABLE and honoured: `preventDefault()` in it stops the
// popover opening (measured — `:popover-open` stays false afterwards).

impl Document {
    pub(crate) fn pointer_root(
        &self,
        point: (f32, f32),
        button: u8,
    ) -> Option<&crate::types::WebCore> {
        let scroll = (self.scroll_x, self.scroll_y);
        for id in self.top_layer.iter().rev() {
            let Some(node) = self.find_webcore(*id) else {
                continue;
            };
            if !self.is_inert(*id)
                && crate::layout::hit_test::point_to_hit_scrolled(node, point, scroll, button)
                    .is_some()
            {
                return Some(node);
            }
        }
        self.active_modal_dialog().is_none().then_some(&self.root)
    }

    pub(crate) fn pointer_hit(
        &self,
        point: (f32, f32),
        button: u8,
    ) -> Option<crate::layout::hit_test::HitResult> {
        let hit = crate::layout::hit_test::point_to_hit_scrolled(
            self.pointer_root(point, button)?,
            point,
            (self.scroll_x, self.scroll_y),
            button,
        )?;
        (!self.is_inert(hit.node_id)).then_some(hit)
    }

    pub(crate) fn pointer_link(&self, point: (f32, f32), button: u8) -> Option<String> {
        self.pointer_hit(point, button)?;
        crate::layout::hit_test::hit_test_link_scrolled(
            self.pointer_root(point, button)?,
            point,
            (self.scroll_x, self.scroll_y),
            button,
        )
    }

    /// Put `id` into the top layer, or move it to the top if already there.
    ///
    /// The single write point for both halves of the state.
    pub fn add_to_top_layer(&mut self, id: u32, kind: crate::types::TopLayerKind) {
        if id == 0 {
            return;
        }
        self.top_layer.retain(|n| *n != id);
        self.top_layer.push(id);
        if let Some(node) = self.find_webcore_mut(id) {
            node.top_layer_kind = Some(kind);
            node.layout.layout_dirty = true;
            node.cascade_dirty = true;
        }
    }

    /// Take `id` out of the top layer. A no-op when it was not in it.
    pub fn remove_from_top_layer(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        self.top_layer.retain(|n| *n != id);
        if let Some(node) = self.find_webcore_mut(id) {
            node.top_layer_kind = None;
            node.layout.layout_dirty = true;
            node.cascade_dirty = true;
        }
    }

    /// The top layer, bottom-first.
    pub fn top_layer_nodes(&self) -> &[u32] {
        &self.top_layer
    }

    /// `element.popover` — `None` is the IDL's `null`, for an element with no
    /// `popover` attribute at all.
    ///
    /// ⛔ The invalid-value default is `"manual"`, not `"auto"`: a bare
    /// `popover` attribute is `auto` and `popover="bogus"` is `manual`
    /// (measured). The two are opposite ends of the light-dismiss rule, so
    /// collapsing them is not cosmetic.
    pub fn popover(&self, id: u32) -> Option<String> {
        let raw = self.get_attribute(id, "popover")?;
        let v = raw.to_ascii_lowercase();
        Some(match v.as_str() {
            "" | "auto" => "auto".to_string(),
            "hint" => "hint".to_string(),
            _ => "manual".to_string(),
        })
    }

    /// `element.popover = …`. `None` removes the attribute.
    pub fn set_popover(&mut self, id: u32, value: Option<&str>) {
        match value {
            Some(v) => self.set_attribute(id, "popover", v),
            None => self.remove_attribute(id, "popover"),
        }
    }

    /// Is this element currently showing as a popover?
    pub fn popover_open(&self, id: u32) -> bool {
        self.find_webcore(id).and_then(|n| n.top_layer_kind)
            == Some(crate::types::TopLayerKind::Popover)
    }

    /// `element.showPopover()`.
    ///
    /// `false` stands for both throws: `NotSupportedError` when the element is
    /// not a popover, and `InvalidStateError` when it is not connected. Showing
    /// one that is ALREADY showing is neither — it succeeds and changes
    /// nothing (measured in isolation).
    pub fn show_popover(&mut self, id: u32) -> bool {
        let Some(state) = self.popover(id) else {
            return false;
        };
        if !self.is_connected(id) {
            return false;
        }
        if self.popover_open(id) {
            return true;
        }
        // Cancelable, and honoured.
        if !self.fire_before_toggle(id, "closed", "open") {
            return false;
        }
        self.light_dismiss_for(&state, id);
        self.add_to_top_layer(id, crate::types::TopLayerKind::Popover);
        true
    }

    /// `element.hidePopover()`. Hiding one that is already hidden succeeds.
    pub fn hide_popover(&mut self, id: u32) -> bool {
        if self.popover(id).is_none() {
            return false;
        }
        if !self.is_connected(id) {
            return false;
        }
        if !self.popover_open(id) {
            return true;
        }
        if !self.fire_before_toggle(id, "open", "closed") {
            return false;
        }
        self.remove_from_top_layer(id);
        true
    }

    /// `element.togglePopover(force)` — returns whether it ends up SHOWING.
    pub fn toggle_popover(&mut self, id: u32, force: Option<bool>) -> bool {
        let want = force.unwrap_or(!self.popover_open(id));
        if want {
            self.show_popover(id);
        } else {
            self.hide_popover(id);
        }
        self.popover_open(id)
    }

    /// Close the popovers that opening a `state` popover dismisses.
    fn light_dismiss_for(&mut self, state: &str, opening: u32) {
        // `manual` dismisses nothing, and nothing dismisses a `manual`.
        let closes: &[&str] = match state {
            "auto" => &["auto", "hint"],
            "hint" => &["hint"],
            _ => return,
        };
        // The opener is not in the list yet — `show_popover` dismisses BEFORE
        // it adds, and returns early for one that is already showing. A guard
        // for `node == opening` here would be unreachable, which a mutation
        // confirmed.
        let _ = opening;
        let open: Vec<u32> = self.top_layer.clone();
        for node in open {
            if self.find_webcore(node).and_then(|n| n.top_layer_kind)
                != Some(crate::types::TopLayerKind::Popover)
            {
                continue;
            }
            let Some(other) = self.popover(node) else {
                continue;
            };
            if closes.contains(&other.as_str()) {
                self.hide_popover(node);
            }
        }
    }

    /// Fire `beforetoggle`. Returns false when a listener cancelled it.
    ///
    /// ⛔ The spec also queues a `toggle` event after the change. There is no
    /// task queue here — the same trade `fire_invalid_event` and the `select`
    /// event already make — and firing `toggle` synchronously would put it
    /// BEFORE the state change that `beforetoggle` is defined to precede. It
    /// is not fired at all rather than fired at the wrong moment.
    pub(crate) fn fire_before_toggle(&mut self, id: u32, old_state: &str, new_state: &str) -> bool {
        let mut event = crate::dom::events::DomEvent::new("beforetoggle", id);
        event.cancelable = true;
        event.old_state = old_state.to_string();
        event.new_state = new_state.to_string();
        self.dispatch_event(&mut event);
        !event.default_prevented()
    }
}
