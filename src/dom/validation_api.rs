//! Constraint validation — HTML §4.10.19.

use crate::html::validity::ValidityState;
use crate::types::Document;

// ─── Constraint validation — HTML §4.10.19 ──────────────────────────────────
impl Document {
    /// `element.willValidate` — whether the element is a candidate for
    /// constraint validation at all.
    ///
    /// This is the member that decides everything else: a barred element is
    /// always `valid`, always has an empty `validationMessage`, and always
    /// passes `checkValidity()`, no matter what its attributes say. Chrome on
    /// `<input required readonly value="">` answers `willValidate=false` and
    /// `valid=true` — the `required` is simply not in force.
    pub fn will_validate(&self, id: u32) -> bool {
        let Some(tag) = self.tag_name(id) else {
            return false;
        };
        // Listed but never candidates.
        if matches!(tag, "output" | "fieldset" | "object") {
            return false;
        }
        if !matches!(tag, "input" | "select" | "textarea" | "button") {
            return false;
        }

        if self.is_actually_disabled(id) {
            return false;
        }
        // A control inside a `<datalist>` is a suggestion, not an entry.
        let mut cursor = self.parent_element(id);
        while let Some(node) = cursor {
            if self.tag_name(node) == Some("datalist") {
                return false;
            }
            cursor = self.parent_element(node);
        }
        match tag {
            "input" => {
                if crate::html::validity::input_type_is_barred(&self.input_type(id)) {
                    return false;
                }
                // `readonly` bars only the types it applies to; a checkbox is
                // not made read-only by the attribute.
                !(self.has_attribute(id, "readonly") && self.readonly_applies(id))
            }
            "textarea" => !self.has_attribute(id, "readonly"),
            "button" => self.button_type(id) == "submit",
            _ => true,
        }
    }

    /// `element.validity`.
    pub fn validity(&self, id: u32) -> ValidityState {
        let mut v = ValidityState::default();
        if !self.will_validate(id) {
            return v;
        }
        let Some(tag) = self.tag_name(id) else {
            return v;
        };

        v.custom_error = self.custom_validity.get(&id).is_some_and(|m| !m.is_empty());
        if tag == "button" {
            return v;
        }

        let value = self.value(id);
        let input_type = if tag == "input" {
            self.input_type(id)
        } else {
            String::new()
        };

        // ── valueMissing ──
        if self.has_attribute(id, "required") {
            v.value_missing = match tag {
                "select" => self.select_has_no_placeholder_selection(id),
                "textarea" => value.is_empty(),
                _ => match input_type.as_str() {
                    "checkbox" => !self.checked(id),
                    // A radio group is satisfied by ANY checked member.
                    "radio" => !self.radio_group_has_a_checked_member(id),
                    "file" => value.is_empty(),
                    _ => value.is_empty(),
                },
            };
        }

        // Every constraint below is on the VALUE, and an empty value has
        // nothing to violate — that is why `required` is the only one that
        // fires on an empty field.
        if value.is_empty() {
            return v;
        }

        // ── typeMismatch ──
        v.type_mismatch = match input_type.as_str() {
            "email" => {
                if self.has_attribute(id, "multiple") {
                    value
                        .split(',')
                        .any(|a| !crate::html::validity::is_valid_email(a.trim()))
                } else {
                    !crate::html::validity::is_valid_email(&value)
                }
            }
            "url" => !crate::html::validity::is_valid_url(value.trim()),
            _ => false,
        };

        // ── patternMismatch ──
        if let Some(pattern) = self.get_attribute(id, "pattern") {
            if PATTERN_TYPES.contains(&input_type.as_str()) {
                // An unparseable pattern is not a violation: HTML says a
                // pattern that fails to compile is ignored.
                v.pattern_mismatch =
                    crate::html::validity::pattern_matches(&pattern, &value) == Some(false);
            }
        }

        // ── tooLong / tooShort ──
        //
        // ⛔ Both apply only to a value the USER edited. Chrome on
        // `<input maxlength=3 value="abcdef">` answers VALID — the dirty value
        // flag is part of the constraint, not an implementation shortcut.
        let length_constrained =
            tag == "textarea" || crate::html::forms::supports_text_length_constraints(&input_type);
        if length_constrained && self.value_is_dirty(id) {
            let len = value.encode_utf16().count() as i64;
            if let Some(max) = self.numeric_attribute(id, "maxlength") {
                v.too_long = len > max as i64;
            }
            if let Some(min) = self.numeric_attribute(id, "minlength") {
                v.too_short = len < min as i64;
            }
        }

        // ── rangeUnderflow / rangeOverflow / stepMismatch / badInput ──
        if NUMERIC_TYPES.contains(&input_type.as_str()) {
            match crate::html::forms::parse_floating_point(&value) {
                None => v.bad_input = true,
                Some(n) => {
                    let min = self
                        .get_attribute(id, "min")
                        .and_then(|s| crate::html::forms::parse_floating_point(&s));
                    let max = self
                        .get_attribute(id, "max")
                        .and_then(|s| crate::html::forms::parse_floating_point(&s));
                    if let Some(min) = min {
                        v.range_underflow = n < min;
                    }
                    if let Some(max) = max {
                        v.range_overflow = n > max;
                    }

                    let step = match self.get_attribute(id, "step") {
                        Some(s) if s.trim().eq_ignore_ascii_case("any") => None,
                        Some(s) => crate::html::forms::parse_floating_point(&s)
                            .filter(|s| *s > 0.0)
                            .or(Some(1.0)),
                        None => Some(1.0),
                    };
                    if let Some(step) = step {
                        let base = min.unwrap_or(0.0);
                        let offset = (n - base) / step;
                        // A float division never lands exactly on an integer,
                        // so the test is "within a rounding error of one".
                        v.step_mismatch = (offset - offset.round()).abs() > 1e-9;
                    }
                }
            }
        }
        if let Some(state) = crate::html::temporal::constraints(
            &input_type,
            &value,
            self.get_attribute(id, "min").as_deref(),
            self.get_attribute(id, "max").as_deref(),
            self.get_attribute(id, "step").as_deref(),
            self.get_attribute(id, "value").as_deref(),
        ) {
            v.range_underflow = state.underflow;
            v.range_overflow = state.overflow;
            v.step_mismatch = state.step_mismatch;
        }
        v
    }

    /// `element.validationMessage` — empty when the element is valid or barred.
    ///
    /// The wording is implementation-defined; the spec asks only for a
    /// suitably descriptive message. A custom message wins over every built-in
    /// one, which is what makes `setCustomValidity` useful.
    pub fn validation_message(&self, id: u32) -> String {
        if !self.will_validate(id) {
            return String::new();
        }
        let v = self.validity(id);
        if v.valid() {
            return String::new();
        }
        if v.custom_error {
            return self.custom_validity.get(&id).cloned().unwrap_or_default();
        }
        let tag = self.tag_name(id).unwrap_or("");
        if v.value_missing {
            return match tag {
                "select" => "Please select an item in the list.".into(),
                _ => "Please fill out this field.".into(),
            };
        }
        if v.type_mismatch {
            return match self.input_type(id).as_str() {
                "email" => "Please enter an email address.".into(),
                _ => "Please enter a URL.".into(),
            };
        }
        if v.pattern_mismatch {
            return "Please match the requested format.".into();
        }
        if v.too_long {
            return "Please shorten this text.".into();
        }
        if v.too_short {
            return "Please lengthen this text.".into();
        }
        if v.range_underflow {
            let min = self.get_attribute(id, "min").unwrap_or_default();
            return format!("Value must be greater than or equal to {}.", min);
        }
        if v.range_overflow {
            let max = self.get_attribute(id, "max").unwrap_or_default();
            return format!("Value must be less than or equal to {}.", max);
        }
        if v.step_mismatch {
            return "Please enter a valid value.".into();
        }
        if v.bad_input {
            return "Please enter a number.".into();
        }
        String::new()
    }

    /// `element.setCustomValidity(message)`. An empty message clears it, which
    /// is the only way to clear it.
    pub fn set_custom_validity(&mut self, id: u32, message: &str) {
        if message.is_empty() {
            self.custom_validity.remove(&id);
        } else {
            self.custom_validity.insert(id, message.to_string());
        }
    }

    /// `element.checkValidity()` — and on a `<form>`, the static validation of
    /// every control it owns.
    ///
    /// Fires an `invalid` event at each element that fails, as the spec
    /// requires. That event is what a page listens for to render its own
    /// messages, so a `checkValidity` that only returned a bool would be
    /// answering the question and skipping the mechanism.
    pub fn check_validity(&mut self, id: u32) -> bool {
        if self.tag_name(id) == Some("form") {
            let mut all_valid = true;
            for control in self.form_elements(id) {
                if !self.check_validity(control) {
                    all_valid = false;
                }
            }
            return all_valid;
        }
        if !self.will_validate(id) {
            return true;
        }
        if self.validity(id).valid() {
            return true;
        }
        self.fire_invalid_event(id);
        false
    }

    /// `element.reportValidity()`.
    ///
    /// Same answer as `checkValidity`, plus "report the problems to the user".
    /// Reporting is the host's job — there is no built-in bubble here — so the
    /// element is focused, which is the part of the spec's reporting steps a
    /// layout engine owns.
    pub fn report_validity(&mut self, id: u32) -> bool {
        let valid = self.check_validity(id);
        if !valid {
            let target = if self.tag_name(id) == Some("form") {
                self.form_elements(id)
                    .into_iter()
                    .find(|c| self.will_validate(*c) && !self.validity(*c).valid())
            } else {
                Some(id)
            };
            if let Some(t) = target {
                self.focus(t);
            }
        }
        valid
    }

    fn fire_invalid_event(&mut self, id: u32) {
        // `invalid` does not bubble and IS cancelable (HTML §4.10.19.3).
        let mut event = crate::dom::events::DomEvent::new("invalid", id);
        self.dispatch_event(&mut event);
    }

    /// Disabled, or inside a disabled `<fieldset>` that is not shielding it
    /// through the fieldset's first `<legend>`.
    pub(crate) fn is_actually_disabled(&self, id: u32) -> bool {
        if !matches!(
            self.tag_name(id),
            Some("input" | "button" | "select" | "textarea" | "fieldset" | "option" | "optgroup")
        ) {
            return false;
        }
        if self.has_attribute(id, "disabled") {
            return true;
        }
        let mut child = id;
        let mut cursor = self.parent_element(id);
        while let Some(node) = cursor {
            if self.tag_name(node) == Some("fieldset") && self.has_attribute(node, "disabled") {
                // The first `<legend>` of a disabled fieldset is NOT disabled.
                let first_legend = self
                    .children(node)
                    .into_iter()
                    .find(|c| self.tag_name(*c) == Some("legend"));
                if first_legend != Some(child) {
                    return true;
                }
            }
            child = node;
            cursor = self.parent_element(node);
        }
        false
    }

    /// Whether `readonly` has any effect on this input's type.
    fn readonly_applies(&self, id: u32) -> bool {
        matches!(
            self.input_type(id).as_str(),
            "text"
                | "search"
                | "url"
                | "tel"
                | "email"
                | "password"
                | "date"
                | "month"
                | "week"
                | "time"
                | "datetime-local"
                | "number"
        )
    }

    /// A `<select required>` is missing a value when its selected option is
    /// the placeholder — the first option, with an empty value, when the
    /// select is not `multiple` and has no `size` above one.
    fn select_has_no_placeholder_selection(&self, id: u32) -> bool {
        let value = self.value(id);
        value.is_empty()
    }

    fn radio_group_has_a_checked_member(&self, id: u32) -> bool {
        let name = self.get_attribute(id, "name").unwrap_or_default();
        if name.is_empty() {
            return self.checked(id);
        }
        let owner = self.form_owner(id);
        let mut any = false;
        self.walk_tree(self.root.node_id, &mut |doc, node| {
            if any {
                return;
            }
            if doc.tag_name(node) != Some("input") {
                return;
            }
            if doc.input_type(node) != "radio" {
                return;
            }
            if doc.get_attribute(node, "name").unwrap_or_default() != name {
                return;
            }
            if doc.form_owner(node) != owner {
                return;
            }
            if doc.checked(node) {
                any = true;
            }
        });
        any
    }

    fn numeric_attribute(&self, id: u32, name: &str) -> Option<u32> {
        self.get_attribute(id, name)
            .and_then(|v| crate::html::forms::parse_non_negative_integer(&v))
    }

    fn value_is_dirty(&self, id: u32) -> bool {
        self.find_webcore(id).is_some_and(|n| n.dirty_value)
    }
}

/// The input types `pattern` applies to (HTML §4.10.5.3.6).
const PATTERN_TYPES: &[&str] = &["text", "search", "url", "tel", "email", "password"];

/// The input types whose value is a NUMBER, for range and step constraints.
///
/// The date and time types also have `min`/`max`/`step`, and they belong here
/// — but their value is a date string, not a float, so putting them in this
/// list today would make every well-formed `2026-08-29` a `badInput`. They
/// join when there is a date parser to convert them; leaving them out costs a
/// missing constraint, putting them in would cost a wrong one.
const NUMERIC_TYPES: &[&str] = &["number", "range"];
