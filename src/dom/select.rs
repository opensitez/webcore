//! `HTMLSelectElement` and `HTMLOptionElement` — HTML §4.10.7–§4.10.10.

use crate::types::Document;

// ─── HTMLSelectElement / HTMLOptionElement ──────────────────────────────────
//
// The items of a `<select>` are its `<option>` CHILDREN — HTML has no separate
// item list, and webcore's own renderer already reads them that way (it walks
// `sel.children` collecting `option` and flattening `optgroup`). So these are
// ordinary tree operations, and an item added here is one the renderer draws
// and the serializer round-trips, with nothing to keep in sync.
//
// Selection lives on the OPTIONS, as `selectedness` — the state HTML defines
// and the state webcore's mouse and keyboard paths write. Reading and writing
// THAT is what makes a programmatic selection and a user's click mean the same
// thing. `html::forms` holds the algorithms over it.

impl Document {
    /// Option node_ids in tree order, flattening `<optgroup>` — HTML counts
    /// options through a group, not around it, so `selectedIndex` does too.
    fn option_ids(&self, select: u32) -> Vec<u32> {
        // Delegates rather than walking again: two copies of "which options
        // does this select have" drift, and this one already differed from the
        // spec's — it descended into a NESTED `<optgroup>`, which the list of
        // options excludes.
        self.find_webcore(select)
            .map(crate::html::forms::option_ids)
            .unwrap_or_default()
    }

    /// `select.options.length`.
    pub fn item_count(&self, select: u32) -> usize {
        self.option_ids(select).len()
    }

    /// `select.add(new Option(text))` — append an `<option>` carrying `text`.
    pub fn add_item(&mut self, select: u32, text: &str) {
        if select == 0 {
            return;
        }
        let option = self.create_element("option");
        let label = self.create_text_node(text);
        self.append_child(option, label);
        self.append_child(select, option);
        self.notify_select_changed(select);
    }

    /// `select.remove(index)`. Out of range removes nothing, as the IDL says.
    pub fn remove_item(&mut self, select: u32, index: usize) {
        if let Some(&id) = self.option_ids(select).get(index) {
            self.remove_child(id);
            // Removing the selected option leaves a drop-down with none, and
            // the algorithm is what picks its replacement.
            self.notify_select_changed(select);
        }
    }

    /// Drop every option. `select.length = 0` in the IDL.
    pub fn clear_items(&mut self, select: u32) {
        for id in self.option_ids(select) {
            self.remove_child(id);
        }
        self.notify_select_changed(select);
    }

    /// The label of the option at `index` — its text content.
    pub fn item_text(&self, select: u32, index: usize) -> String {
        match self.option_ids(select).get(index) {
            Some(&id) => self.text_content(id).trim().to_string(),
            None => String::new(),
        }
    }

    pub fn set_item_text(&mut self, select: u32, index: usize, text: &str) {
        if let Some(&id) = self.option_ids(select).get(index) {
            self.set_text_content(id, text);
            // A closed drop-down shows the LABEL, so renaming the selected
            // option has to move the shown text with it.
            self.notify_select_changed(select);
        }
    }

    /// `select.selectedIndex` — "the index of the first option element in the
    /// list of options that has its selectedness set to true. If there isn't
    /// one, then it is −1."
    ///
    /// ⛔ −1 is not only the empty-select answer. A LIST BOX rests with nothing
    /// selected, because the selectedness setting algorithm auto-selects only
    /// at a display size of 1. This used to fall back to 0 whenever any option
    /// existed, so a fresh list box claimed a selection it did not have.
    pub fn selected_index(&self, select: u32) -> i32 {
        self.find_webcore(select)
            .map(crate::html::forms::selected_index)
            .unwrap_or(-1)
    }

    /// `select.selectedIndex = i`.
    ///
    /// Writes SELECTEDNESS — the same state a click and an arrow key write, so
    /// a programmatic selection and a user's are indistinguishable afterwards.
    ///
    /// "Set the selectedness of all the option elements ... to false, and then
    /// set the selectedness of the option element ... with index `index` to
    /// true", and the IDL setter raises DIRTINESS, which is what stops a
    /// subsequent form reset from being a no-op.
    ///
    /// ⛔ It does NOT touch the `selected` content attribute. That attribute is
    /// `defaultSelected` — the author's default and the reset target — and
    /// writing it here is why a reset used to restore the last programmatic
    /// selection instead of the markup's.
    pub fn set_selected_index(&mut self, select: u32, index: i32) {
        let options = crate::html::forms::option_ids(match self.find_webcore(select) {
            Some(n) => n,
            None => return,
        });
        // A negative index — or any out-of-range one — means "nothing
        // selected", which selectedness can now actually express.
        let chosen = usize::try_from(index)
            .ok()
            .filter(|i| *i < options.len())
            .map(|i| options[i]);
        if let Some(sel) = self.find_webcore_mut(select) {
            crate::html::forms::for_each_option_mut(sel, &mut |option, _| {
                option.selectedness = Some(option.node_id) == chosen;
                option.dirty_selectedness = true;
            });
            sel.layout.layout_dirty = true;
        }
        // A closed drop-down shows a child text node rather than its options,
        // so the shown label has to follow the selection. `option:checked` is a
        // selector, so this is a style change too.
        if let Some(sel) = self.find_webcore_mut(select) {
            crate::html::forms::refresh_select_display_text(sel);
        }
        self.style_dirty = true;
    }

    /// `element.value` — which means three different things, exactly as HTML
    /// says it does.
    ///
    /// A `<textarea>`'s value is its text content; a `<select>`'s is the VALUE
    /// OF ITS SELECTED OPTION (falling back to that option's label, per
    /// `option.value`); everything else reads the `value` attribute. webcore's
    /// `input_value` covers the first and last but answers a `<select>` from a
    /// `value` attribute a select does not have.
    pub fn value(&self, id: u32) -> String {
        if id == 0 {
            return String::new();
        }
        let Some(node) = self.find_webcore(id) else {
            return String::new();
        };
        let tag = node.tag.clone();
        let input_mode = crate::html::forms::value_mode(node);
        match tag.as_str() {
            // The VALUE, which is the raw value once anything has set it and
            // the child text (the default value) until then.
            "textarea" => self
                .find_webcore(id)
                .map(crate::types::input_value)
                .unwrap_or_default(),
            "select" => self
                .find_webcore(id)
                .map(crate::html::forms::select_value)
                .unwrap_or_default(),
            // ⛔ WHICH MODE the `value` IDL attribute is in decides where to
            // read from (HTML §4.10.5.4). A checkbox's value is its `value`
            // ATTRIBUTE, defaulting to `"on"` — its STATE is checkedness. Only
            // a mode-`Value` control holds a value of its own.
            "input" if input_mode != crate::html::forms::ValueMode::Value => {
                match self.get_attribute(id, "value") {
                    Some(v) => v,
                    None if input_mode == crate::html::forms::ValueMode::DefaultOn => {
                        "on".to_string()
                    }
                    None => String::new(),
                }
            }
            // Everything else answers with its VALUE. Reading the `value`
            // ATTRIBUTE here made `input.value` report the author's default
            // rather than what the control holds — so a field the user had
            // typed into read back empty.
            _ => self
                .find_webcore(id)
                .map(crate::types::input_value)
                .unwrap_or_default(),
        }
    }

    /// `element.value = v`, the setter half of the above.
    pub fn set_value(&mut self, id: u32, value: &str) {
        if id == 0 {
            return;
        }
        let tag = self
            .find_webcore(id)
            .map(|n| n.tag.clone())
            .unwrap_or_default();
        match tag.as_str() {
            // The IDL setter sets the VALUE and raises the dirty value flag
            // (HTML §4.10.5.4) — it does not rewrite the markup, which is what
            // `set_text_content` and `set_attribute` were doing here. A reset
            // after a programmatic write used to restore the written value.
            "textarea" => self.set_control_value(id, value),
            // Assigning to `select.value` selects the option WITH that value —
            // it does not store a string. An unmatched value selects nothing.
            "select" => {
                let options = self.option_ids(id);
                let found = options.iter().position(|o| {
                    let v = self
                        .get_attribute(*o, "value")
                        .unwrap_or_else(|| self.text_content(*o).trim().to_string());
                    v == value
                });
                self.set_selected_index(id, found.map_or(-1, |i| i as i32));
            }
            _ => self.set_control_value(id, value),
        }
    }

    /// The `value` IDL setter, per the control's MODE (HTML §4.10.5.4).
    ///
    /// ⛔ Only mode `Value` writes the value state. A checkbox, a radio and a
    /// button write the `value` CONTENT ATTRIBUTE — that attribute IS their
    /// value, so routing them through the state made `checkbox.value = "true"`
    /// read back as `"on"`.
    fn set_control_value(&mut self, id: u32, value: &str) {
        let mode = match self.find_webcore(id) {
            Some(n) => crate::html::forms::value_mode(n),
            None => return,
        };
        // "If the new value is different from the old, move the text entry
        // cursor to the end and unselect" (HTML §4.10.5.4). Assigning the SAME
        // string leaves the selection where it was — measured both ways:
        // a (2,4) selection survives `value = "abcdef"` on a value that already
        // reads `"abcdef"`, and collapses to `[6,6]` when it does not.
        let value_changed = self.value(id) != value;
        if mode != crate::html::forms::ValueMode::Value {
            self.set_attribute(id, "value", value);
            return;
        }
        if let Some(node) = self.find_webcore_mut(id) {
            node.value_state = Some(value.to_string());
            node.dirty_value = true;
            node.layout.layout_dirty = true;
            // "Invoke the value sanitization algorithm, if the element's type
            // attribute's current state defines one" — a range assigned an
            // off-step number snaps, exactly as it does from the markup.
            let sanitized = {
                let is_range = node
                    .attributes
                    .get("type")
                    .map(|t| t.trim().eq_ignore_ascii_case("range"))
                    .unwrap_or(false);
                if is_range {
                    Some(crate::html::forms::best_representation(
                        crate::html::forms::sanitize_range_value(node, value),
                    ))
                } else {
                    None
                }
            };
            if let Some(v) = sanitized {
                node.value_state = Some(v);
            }
        }
        if value_changed {
            let end = self.value(id).chars().count();
            if let Some(node) = self.find_webcore_mut(id) {
                node.input_cursor = end;
                node.input_sel_anchor = end;
                node.input_sel_direction = crate::types::SelectionDirection::None;
            }
        }
        self.style_dirty = true;
    }
}
