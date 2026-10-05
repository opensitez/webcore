//! `HTMLDialogElement` — HTML §4.11.4.
//!
//! Modality is TOP-LAYER MEMBERSHIP, which lives in `top_layer.rs`; the UA
//! sheet's `dialog:modal` rule does the layout. Nothing here writes a style.

use crate::types::Document;

#[derive(Clone, Default)]
pub(crate) struct DialogState {
    previous_focus: u32,
    return_value: String,
}

// ─── HTMLDialogElement ──────────────────────────────────────────────────────
//
// HTML §4.11.4. Openness is the `open` CONTENT ATTRIBUTE — the IDL property
// reflects it, so there is no separate "is it showing" flag to drift out of
// step, and markup that arrives with `<dialog open>` is already open without
// anyone calling `show()`.
//
// `display` and the non-modal `position` now come from the UA stylesheet
// (`dialog:not([open]) { display: none }` and the `dialog` block in
// `css::UA_CSS`), which is where the spec puts them. This file used to write
// both as INLINE styles because the sheet had no `dialog` entry — that made a
// dialog that had never been opened render in flow, and made an opened one
// immune to the author's own `display`, since an inline style beats every
// rule. Setting the attribute is the whole of `show()`; the cascade does the
// rest.
//
impl Document {
    /// `dialog.show()` / `dialog.showModal()`.
    pub fn show_dialog(&mut self, id: u32, modal: bool) {
        if self.tag_name(id) != Some("dialog")
            || self.dialog_open(id)
            || !self.is_connected(id)
            || self.popover_open(id)
        {
            return;
        }
        if !self.fire_before_toggle(id, "closed", "open") {
            return;
        }
        if !self.is_connected(id) || self.dialog_open(id) || self.popover_open(id) {
            return;
        }
        let previous_focus = self.focused_box;
        self.dialog_states.entry(id).or_default().previous_focus = previous_focus;
        self.set_attribute(id, "open", "");
        // ⛔ The `position: fixed` INLINE style that used to live here is gone.
        // A modal is in the TOP LAYER, and the UA sheet's `dialog:modal` rule
        // does the layout — which means an author's own `position` can now
        // beat it, exactly as it already could for a non-modal dialog.
        if modal {
            self.add_to_top_layer(id, crate::types::TopLayerKind::ModalDialog);
            self.open_select = 0;
            self.open_picker = 0;
        }
        let focus = self.find_webcore(id).and_then(|dialog| {
            fn candidate(
                doc: &Document,
                node: &crate::types::WebCore,
                autofocus: bool,
            ) -> Option<u32> {
                if (!autofocus || node.attributes.contains_key("autofocus"))
                    && crate::types::is_focusable_node(node)
                    && !doc.is_inert(node.node_id)
                    && !doc.is_actually_disabled(node.node_id)
                {
                    return Some(node.node_id);
                }
                node.children
                    .iter()
                    .find_map(|child| candidate(doc, child, autofocus))
            }
            candidate(self, dialog, true).or_else(|| candidate(self, dialog, false))
        });
        if let Some(focus) =
            focus.filter(|target| !self.is_inert(*target) && !self.is_actually_disabled(*target))
        {
            self.focus(focus);
        } else {
            // Dialog focusing steps allow focusing the dialog itself without tabindex.
            self.focused_box = if self.is_inert(id) { 0 } else { id };
        }
    }

    /// `dialog.close()`.
    pub fn close_dialog(&mut self, id: u32) {
        if self.tag_name(id) != Some("dialog") || !self.dialog_open(id) {
            return;
        }
        self.remove_attribute(id, "open");
        // Leaving the top layer is the whole of it: the `position` override a
        // modal used to carry was an inline style, and the UA sheet's
        // `dialog:modal` rule stops applying by itself.
        self.remove_from_top_layer(id);
        let previous = self
            .dialog_states
            .get(&id)
            .map_or(0, |state| state.previous_focus);
        if self.is_descendant_of(self.focused_box, id) {
            self.focused_box = 0;
            self.focus(previous);
        }
    }

    pub fn dialog_return_value(&self, id: u32) -> String {
        self.dialog_states
            .get(&id)
            .map(|state| state.return_value.clone())
            .unwrap_or_default()
    }

    pub fn set_dialog_return_value(&mut self, id: u32, value: &str) {
        if self.tag_name(id) == Some("dialog") {
            self.dialog_states.entry(id).or_default().return_value = value.to_owned();
        }
    }

    /// `requestClose()` fires a cancelable cancel event before closing.
    pub fn request_close_dialog(&mut self, id: u32) -> bool {
        if !self.dialog_open(id) || self.tag_name(id) != Some("dialog") {
            return false;
        }
        let mut event = crate::dom::events::DomEvent::new("cancel", id);
        event.bubbles = false;
        event.cancelable = true;
        self.dispatch_dom_event(&mut event);
        if event.default_prevented() {
            return false;
        }
        self.close_dialog(id);
        true
    }

    pub(crate) fn submit_dialog_form(&mut self, form: u32, submitter: u32) {
        let mut ancestor = self.parent_node(form);
        while ancestor != 0 {
            if self.tag_name(ancestor) == Some("dialog") {
                if let Some(value) = self.get_attribute(submitter, "value") {
                    self.set_dialog_return_value(ancestor, &value);
                }
                self.close_dialog(ancestor);
                return;
            }
            let parent = self.parent_node(ancestor);
            if parent == ancestor {
                break;
            }
            ancestor = parent;
        }
    }

    /// Declarative dialog invokers use the same API as an embedded host.
    pub(crate) fn activate_dialog_command(&mut self, source: u32) -> bool {
        if self.tag_name(source) != Some("button")
            || self.is_actually_disabled(source)
            || self.is_inert(source)
        {
            return false;
        }
        if crate::types::form_owner_id(&self.root, source).is_some()
            && self.button_type(source) != "button"
        {
            return false;
        }
        let Some(target) = self
            .get_attribute(source, "commandfor")
            .and_then(|target| self.get_element_by_id(&target))
        else {
            return false;
        };
        if self.tag_name(target) != Some("dialog") {
            return false;
        }
        let command = self
            .get_attribute(source, "command")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !matches!(command.as_str(), "show-modal" | "close" | "request-close") {
            return false;
        }
        let mut event = crate::dom::events::DomEvent::new("command", target);
        event.command = command.clone();
        event.source = source;
        event.cancelable = true;
        self.dispatch_dom_event(&mut event);
        if event.default_prevented() || !self.is_connected(target) || self.popover_open(target) {
            return true;
        }
        match command.as_str() {
            "show-modal" => self.show_dialog(target, true),
            "close" => {
                if self.dialog_open(target) {
                    if let Some(value) = self.get_attribute(source, "value") {
                        self.set_dialog_return_value(target, &value);
                    }
                    self.close_dialog(target);
                }
            }
            "request-close" => {
                // A canceled request must not change returnValue.
                let value = self.get_attribute(source, "value");
                if self.request_close_dialog(target) {
                    if let Some(value) = value {
                        self.set_dialog_return_value(target, &value);
                    }
                }
            }
            _ => unreachable!(),
        }
        true
    }

    pub(crate) fn active_modal_dialog(&self) -> Option<u32> {
        self.top_layer.iter().rev().copied().find(|id| {
            self.dialog_open(*id)
                && self.find_webcore(*id).is_some_and(|node| {
                    node.top_layer_kind == Some(crate::types::TopLayerKind::ModalDialog)
                })
        })
    }

    pub(crate) fn is_descendant_of(&self, mut id: u32, ancestor: u32) -> bool {
        while id != 0 {
            if id == ancestor {
                return true;
            }
            let parent = self.parent_node(id);
            if parent == id {
                break;
            }
            id = parent;
        }
        false
    }

    /// `dialog.open` — reflects the content attribute, per the IDL.
    pub fn dialog_open(&self, id: u32) -> bool {
        id != 0 && self.get_attribute(id, "open").is_some()
    }
}
