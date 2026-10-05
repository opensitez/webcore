//! `HTMLElement` — `inert` and the reflected content attributes
//! (HTML §3.2.6, §6.7).

use crate::types::Document;

// ─── HTMLElement: inert and the reflected content attributes ────────────────
//
// Five attributes, four different default rules — which is why there is no
// single `reflect_enumerated` helper here. All measured:
//
//   * `draggable` absent is FALSE on a `<div>` and TRUE on `<a href>`,
//     `<area href>` and `<img>`: a per-element default, reached through the
//     `auto` state that an invalid value also falls into.
//   * `spellcheck` absent is TRUE, and a child of `spellcheck=false` is
//     FALSE — inherited.
//   * `translate` absent is TRUE and also inherited, but its setter writes
//     `"yes"`/`"no"` where `draggable` and `spellcheck` write `"true"`/`"false"`.
//   * `autocapitalize` absent is the EMPTY STRING, and an unrecognised value
//     is `"sentences"` — the two differ, so "invalid behaves as absent" is
//     wrong here even though it holds for the other three.
//   * `accessKey` absent is the empty string, plainly.

impl Document {
    /// `element.inert` — the element's OWN attribute.
    ///
    /// ⛔ Not inherited, even though the EFFECT is: inside `<div inert>` a
    /// button answers `false` for `.inert` and still cannot be focused
    /// (measured). Use [`Document::is_inert`] for the effective question.
    pub fn inert(&self, id: u32) -> bool {
        self.has_attribute(id, "inert")
    }

    /// `element.inert = …`. A boolean attribute: `false` removes it, `true`
    /// sets it to the empty string.
    pub fn set_inert(&mut self, id: u32, value: bool) {
        if value {
            self.set_attribute(id, "inert", "");
        } else {
            self.remove_attribute(id, "inert");
        }
    }

    /// Is this node inert — its own attribute or any ancestor's?
    ///
    /// The same shape as `is_actually_disabled`, and for the same reason: the
    /// attribute sits on an ancestor and the question is asked of the
    /// descendant.
    pub fn is_inert(&self, id: u32) -> bool {
        if self
            .active_modal_dialog()
            .is_some_and(|modal| !self.is_descendant_of(id, modal))
        {
            return true;
        }
        let mut cur = id;
        while cur != 0 {
            if self.has_attribute(cur, "inert") {
                return true;
            }
            // A modal escapes ancestor inertness, but not its own inert attribute.
            if self.find_webcore(cur).is_some_and(|node| {
                node.top_layer_kind == Some(crate::types::TopLayerKind::ModalDialog)
            }) {
                break;
            }
            let parent = self.parent_node(cur);
            if parent == cur {
                break;
            }
            cur = parent;
        }
        false
    }

    /// `element.draggable`.
    pub fn draggable(&self, id: u32) -> bool {
        match self
            .get_attribute(id, "draggable")
            .map(|v| v.to_ascii_lowercase())
        {
            Some(v) if v == "true" => true,
            Some(v) if v == "false" => false,
            // Absent, or the `auto` state that an unrecognised value falls
            // into: the per-element default.
            _ => {
                matches!(self.tag_name(id), Some("img"))
                    || (matches!(self.tag_name(id), Some("a") | Some("area"))
                        && self.has_attribute(id, "href"))
            }
        }
    }

    /// `element.draggable = …` — writes the keyword, never removes it.
    pub fn set_draggable(&mut self, id: u32, value: bool) {
        self.set_attribute(id, "draggable", if value { "true" } else { "false" });
    }

    /// `element.spellcheck` — inherited, defaulting to true.
    pub fn spellcheck(&self, id: u32) -> bool {
        let mut cur = id;
        while cur != 0 {
            match self
                .get_attribute(cur, "spellcheck")
                .map(|v| v.to_ascii_lowercase())
            {
                Some(v) if v == "true" => return true,
                Some(v) if v == "false" => return false,
                // An unrecognised value is the `default` state, which keeps
                // asking upwards (measured: `spellcheck=bogus` answers true).
                _ => {}
            }
            let parent = self.parent_node(cur);
            if parent == cur {
                break;
            }
            cur = parent;
        }
        true
    }

    pub fn set_spellcheck(&mut self, id: u32, value: bool) {
        self.set_attribute(id, "spellcheck", if value { "true" } else { "false" });
    }

    /// `element.translate` — inherited, defaulting to true.
    pub fn translate(&self, id: u32) -> bool {
        let mut cur = id;
        while cur != 0 {
            match self
                .get_attribute(cur, "translate")
                .map(|v| v.to_ascii_lowercase())
            {
                Some(v) if v == "yes" => return true,
                Some(v) if v == "no" => return false,
                _ => {}
            }
            let parent = self.parent_node(cur);
            if parent == cur {
                break;
            }
            cur = parent;
        }
        true
    }

    /// `element.translate = …`.
    ///
    /// ⛔ Writes `"yes"`/`"no"`, not `"true"`/`"false"` — the one setter in
    /// this group with a different vocabulary from its neighbours.
    pub fn set_translate(&mut self, id: u32, value: bool) {
        self.set_attribute(id, "translate", if value { "yes" } else { "no" });
    }

    /// `element.autocapitalize`.
    ///
    /// ⛔ Absent is `""` and an UNRECOGNISED value is `"sentences"`. Treating
    /// an invalid value as absent — which is right for the three above — gives
    /// the wrong answer here.
    pub fn autocapitalize(&self, id: u32) -> String {
        let Some(raw) = self.get_attribute(id, "autocapitalize") else {
            return String::new();
        };
        let v = raw.to_ascii_lowercase();
        match v.as_str() {
            "off" | "none" | "on" | "sentences" | "words" | "characters" => v,
            _ => "sentences".to_string(),
        }
    }

    pub fn set_autocapitalize(&mut self, id: u32, value: &str) {
        self.set_attribute(id, "autocapitalize", value);
    }

    /// `element.accessKey` — the attribute verbatim, `""` when absent.
    pub fn access_key(&self, id: u32) -> String {
        self.get_attribute(id, "accesskey").unwrap_or_default()
    }

    pub fn set_access_key(&mut self, id: u32, value: &str) {
        self.set_attribute(id, "accesskey", value);
    }

    /// `element.accessKeyLabel` — the key combination the UA actually assigned.
    ///
    /// Always `""`: nothing here assigns one, and the spec's own answer for
    /// that case is the empty string. Spec-derived rather than measured — this
    /// Chrome build does not expose the property at all.
    pub fn access_key_label(&self, _id: u32) -> String {
        String::new()
    }
}
