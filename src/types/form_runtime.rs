//! The form-control runtime: clicking a control, collecting and encoding a
//! form's data, and resetting it.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

/// Returns true if `node` is a focusable element (native or via tabindex/contenteditable).
/// tabindex=-1 elements return true (focusable by script/click) but are excluded from
/// the *tab* order by `collect_focusable_ordered`.
/// Handle a click on a form element: toggle checkbox, select radio, fire form events.
/// Returns Some(true) if a redraw is needed, Some(false) if handled but no redraw, None if not a form element.
pub fn handle_form_click(
    root: &mut WebCore,
    target: u32,
    callback: &mut Option<FormEventCallback>,
) -> Option<bool> {
    // Find a node by node_id in the tree (immutable)
    fn find_ref<'a>(node: &'a WebCore, t: u32) -> Option<&'a WebCore> {
        if node.node_id == t {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = find_ref(child, t) {
                return Some(found);
            }
        }
        None
    }
    // Find a node by node_id in the tree (mutable)
    fn find_mut<'a>(node: &'a mut WebCore, t: u32) -> Option<&'a mut WebCore> {
        if node.node_id == t {
            return Some(node);
        }
        for child in &mut node.children {
            if let Some(found) = find_mut(child, t) {
                return Some(found);
            }
        }
        None
    }
    fn find_label_control_id(root: &WebCore, label_id: u32) -> Option<u32> {
        fn find_by_html_id(node: &WebCore, id: &str) -> Option<u32> {
            if node.attributes.get("id").is_some_and(|value| value == id) && is_labelable(node) {
                return Some(node.node_id);
            }
            for child in &node.children {
                if let Some(found) = find_by_html_id(child, id) {
                    return Some(found);
                }
            }
            None
        }
        fn first_labelable_descendant(node: &WebCore) -> Option<u32> {
            for child in &node.children {
                if is_labelable(child) {
                    return Some(child.node_id);
                }
                if let Some(found) = first_labelable_descendant(child) {
                    return Some(found);
                }
            }
            None
        }
        let label = find_ref(root, label_id)?;
        if label.tag != "label" {
            return None;
        }
        if let Some(for_id) = label.attributes.get("for") {
            if let Some(control) = find_by_html_id(root, for_id) {
                return Some(control);
            }
        }
        first_labelable_descendant(label)
    }
    fn is_labelable(node: &WebCore) -> bool {
        matches!(
            node.tag.as_str(),
            "button" | "input" | "meter" | "output" | "progress" | "select" | "textarea"
        ) && node
            .attributes
            .get("type")
            .is_none_or(|ty| !ty.eq_ignore_ascii_case("hidden"))
    }

    // If the target is a #text node, find the parent form element instead.
    // This handles clicks on text inside <select>, <button>, etc.
    let effective_target = {
        let node = find_ref(root, target)?;
        if node.tag == "#text" {
            // Walk the tree to find the parent of this text node
            fn find_parent_id(node: &WebCore, child_id: u32) -> Option<u32> {
                for c in &node.children {
                    if c.node_id == child_id {
                        return Some(node.node_id);
                    }
                    if let Some(p) = find_parent_id(c, child_id) {
                        return Some(p);
                    }
                }
                None
            }
            find_parent_id(root, target).unwrap_or(target)
        } else {
            target
        }
    };
    let target = effective_target;

    // Disabled elements don't respond to clicks
    let target_node = find_ref(root, target)?;
    if target_node.attributes.contains_key("disabled") {
        return None;
    }

    // Read target info before mutation
    let (tag, input_type, name, id, value) = {
        let tag = target_node.tag.clone();
        let input_type = target_node
            .attributes
            .get("type")
            .map(|ty| ty.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let name = target_node
            .attributes
            .get("name")
            .cloned()
            .unwrap_or_default();
        let id = target_node
            .attributes
            .get("id")
            .cloned()
            .unwrap_or_default();
        let value = target_node
            .attributes
            .get("value")
            .cloned()
            .unwrap_or_default();
        (tag, input_type, name, id, value)
    };

    match tag.as_str() {
        "label" => {
            let control_id = find_label_control_id(root, target)?;
            if control_id == target {
                return None;
            }
            handle_form_click(root, control_id, callback)
        }
        "input" => {
            match input_type.as_str() {
                "checkbox" => {
                    let node = find_mut(root, target)?;
                    // **A click changes STATE, not markup** (HTML §4.10.5.3).
                    // This used to add and remove the `checked` ATTRIBUTE, so
                    // ticking a box edited the document and
                    // `getAttribute("checked")` answered the user's last click
                    // instead of the author's default.
                    let was_checked = node.checkedness;
                    node.checkedness = !was_checked;
                    // "must be set to true whenever the user interacts with the
                    // control in a way that changes the checkedness."
                    node.dirty_checked = true;
                    let new_checked = !was_checked;
                    if let Some(cb) = callback {
                        cb(&FormEvent {
                            tag: tag.clone(),
                            id,
                            name,
                            kind: FormEventKind::Toggle(new_checked),
                            element: target,
                        });
                    }
                    Some(true)
                }
                "radio" => {
                    // Uncheck other radios with the same name, check this one
                    if !name.is_empty() {
                        fn uncheck_radios(node: &mut WebCore, name: &str, except_id: u32) {
                            if node.tag == "input"
                                && node.attributes.get("type").map(|s| s.as_str()) == Some("radio")
                                && node.attributes.get("name").map(|s| s.as_str()) == Some(name)
                                && node.node_id != except_id
                            {
                                node.checkedness = false;
                                node.dirty_checked = true;
                            }
                            for child in &mut node.children {
                                uncheck_radios(child, name, except_id);
                            }
                        }
                        uncheck_radios(root, &name, target);
                    }
                    let node = find_mut(root, target)?;
                    node.checkedness = true;
                    node.dirty_checked = true;
                    if let Some(cb) = callback {
                        cb(&FormEvent {
                            tag: tag.clone(),
                            id,
                            name,
                            kind: FormEventKind::Change(value),
                            element: target,
                        });
                    }
                    Some(true)
                }
                "submit" | "button" | "reset" => {
                    // Reset button: reset the parent form
                    if input_type == "reset" {
                        let _form_action = find_parent_form_action(root, target);
                        // Find and reset the parent form
                        fn find_form_for_reset(node: &WebCore, target_id: u32) -> Option<u32> {
                            if node.tag == "form" {
                                fn contains(n: &WebCore, t: u32) -> bool {
                                    if n.node_id == t {
                                        return true;
                                    }
                                    n.children.iter().any(|c| contains(c, t))
                                }
                                if contains(node, target_id) {
                                    return Some(node.node_id);
                                }
                            }
                            for child in &node.children {
                                if let Some(f) = find_form_for_reset(child, target_id) {
                                    return Some(f);
                                }
                            }
                            None
                        }
                        if let Some(form_id) = find_form_for_reset(root, target) {
                            reset_form(root, form_id);
                        }
                    }
                    if let Some(cb) = callback {
                        cb(&FormEvent {
                            tag: tag.clone(),
                            id,
                            name,
                            kind: FormEventKind::Click(value),
                            element: target,
                        });
                    }
                    Some(false)
                }
                "text" | "password" | "email" | "search" | "url" | "tel" | "number" => {
                    // Text input clicked — set cursor to end of value
                    let node = find_mut(root, target)?;
                    let len = input_value(node).chars().count();
                    node.input_cursor = len;
                    node.input_sel_anchor = len;
                    Some(true)
                }
                _ => None,
            }
        }
        "button" => {
            let target_node2 = find_ref(root, target);
            let btn_type = target_node2
                .and_then(|n| n.attributes.get("type").cloned())
                .unwrap_or_else(|| "submit".to_string());
            if let Some(cb) = callback {
                let text = target_node2.map(|n| n.text.clone()).unwrap_or_default();
                cb(&FormEvent {
                    tag: tag.clone(),
                    id: id.clone(),
                    name: name.clone(),
                    kind: FormEventKind::Click(if value.is_empty() {
                        text
                    } else {
                        value.clone()
                    }),
                    element: target,
                });
                // Submit buttons trigger form submission only when they have a
                // form owner. A standalone demo toolbar `<button>` still has
                // a missing-value default type of submit, but it has no form
                // to submit and must not navigate the current document.
                if btn_type == "submit" && has_parent_form(root, target) {
                    let action = find_parent_form_action(root, target);
                    cb(&FormEvent {
                        tag: "form".into(),
                        id: String::new(),
                        name: String::new(),
                        kind: FormEventKind::Submit(action),
                        element: target,
                    });
                }
            }
            // Reset buttons reset the form
            if btn_type == "reset" {
                fn find_form_id(node: &WebCore, target_id: u32) -> Option<u32> {
                    if node.tag == "form" {
                        fn has(n: &WebCore, t: u32) -> bool {
                            if n.node_id == t {
                                return true;
                            }
                            n.children.iter().any(|c| has(c, t))
                        }
                        if has(node, target_id) {
                            return Some(node.node_id);
                        }
                    }
                    for c in &node.children {
                        if let Some(f) = find_form_id(c, target_id) {
                            return Some(f);
                        }
                    }
                    None
                }
                if let Some(fid) = find_form_id(root, target) {
                    reset_form(root, fid);
                }
            }
            Some(btn_type == "reset") // redraw if reset
        }
        // ⛔ `<select>` and `<input type=range>` are both absent on purpose:
        // where the click LANDED decides what they do, and this function is
        // handed a target without a point. Both live in `process_mouse_event`,
        // which has `doc_pt` — a list box picks a row, a range picks a value
        // along its track, and a drop-down opens its popup.
        "select" => None,
        _ => None,
    }
}

/// Find the form element parent of a target (walks up from #text to select/input/button).
pub(crate) fn find_form_parent_id(root: &WebCore, target_id: u32) -> u32 {
    fn find_ref<'a>(node: &'a WebCore, t: u32) -> Option<&'a WebCore> {
        if node.node_id == t {
            return Some(node);
        }
        for child in &node.children {
            if let Some(f) = find_ref(child, t) {
                return Some(f);
            }
        }
        None
    }
    if let Some(node) = find_ref(root, target_id) {
        if matches!(
            node.tag.as_str(),
            "input" | "select" | "textarea" | "button"
        ) {
            return target_id;
        }
    }
    // Walk tree to find parent
    fn walk(node: &WebCore, target_id: u32) -> Option<u32> {
        for child in &node.children {
            if child.node_id == target_id {
                if matches!(
                    node.tag.as_str(),
                    "input" | "select" | "textarea" | "button" | "label"
                ) {
                    return Some(node.node_id);
                }
            }
            if let Some(p) = walk(child, target_id) {
                return Some(p);
            }
        }
        None
    }
    walk(root, target_id).unwrap_or(target_id)
}

fn has_parent_form(root: &WebCore, target_id: u32) -> bool {
    fn contains(node: &WebCore, target_id: u32) -> bool {
        node.node_id == target_id || node.children.iter().any(|child| contains(child, target_id))
    }
    fn walk(node: &WebCore, target_id: u32) -> bool {
        node.tag == "form" && node.children.iter().any(|child| contains(child, target_id))
            || node.children.iter().any(|child| walk(child, target_id))
    }
    walk(root, target_id)
}

/// Find the action URL of the nearest ancestor <form> element.
pub fn find_parent_form_action(root: &WebCore, target_id: u32) -> String {
    fn walk(node: &WebCore, target_id: u32) -> Option<String> {
        for child in &node.children {
            if child.node_id == target_id {
                if node.tag == "form" {
                    return Some(node.attributes.get("action").cloned().unwrap_or_default());
                }
                return None; // found target but parent isn't form — caller keeps looking
            }
            if let Some(action) = walk(child, target_id) {
                return Some(action);
            }
            // Check if child contains target and this node is a form
            if node.tag == "form" {
                fn contains(node: &WebCore, target_id: u32) -> bool {
                    if node.node_id == target_id {
                        return true;
                    }
                    node.children.iter().any(|c| contains(c, target_id))
                }
                if contains(child, target_id) {
                    return Some(node.attributes.get("action").cloned().unwrap_or_default());
                }
            }
        }
        None
    }
    walk(root, target_id).unwrap_or_default()
}

/// **Constructing the entry list** (HTML §4.10.21.4) for a `<form>`.
///
/// A LIST of name/value entries in tree order, not a map. HTML appends one
/// entry per contributing control and never says two entries may not share a
/// name — which is the whole shape of a `multiple` select and of a checkbox
/// group, both of which submit several values under one name. Returned as a
/// map, the last write silently won and every value but one vanished at the
/// point of submission, where nothing downstream could tell it had happened.
///
/// The rules each control follows, and where each one is written, stay exactly
/// where they were; this is only the container being able to hold the answer.
/// - Text/password/hidden/email/…: the control's VALUE, not its attribute
/// - Checkbox / radio: only when checked; `"on"` when no value is given
/// - Select: one entry per selected, non-disabled option
/// - Textarea: its value
/// - Disabled elements, and everything inside them, contribute nothing
/// - Elements without a name contribute nothing
pub fn collect_form_data(form: &WebCore) -> Vec<(String, String)> {
    let mut data = Vec::new();
    collect_form_data_inner(form, &mut data);
    data
}

fn collect_form_data_inner(node: &WebCore, data: &mut Vec<(String, String)>) {
    if node.attributes.contains_key("disabled") {
        return;
    }
    let name = match node.attributes.get("name") {
        Some(n) if !n.is_empty() => n.clone(),
        _ => {
            // No name — recurse into children but don't collect this node
            for child in &node.children {
                collect_form_data_inner(child, data);
            }
            return;
        }
    };
    match node.tag.as_str() {
        "input" => {
            let input_type = node
                .attributes
                .get("type")
                .map(|s| s.as_str())
                .unwrap_or("text");
            match input_type {
                "checkbox" => {
                    // What gets SUBMITTED is the current checkedness, not the
                    // author's default — a box the user unticked must not be
                    // in the form data because the markup still says `checked`.
                    if node.checkedness {
                        let val = node
                            .attributes
                            .get("value")
                            .cloned()
                            .unwrap_or_else(|| "on".to_string());
                        data.push((name, val));
                    }
                }
                "radio" => {
                    if node.checkedness {
                        let val = node.attributes.get("value").cloned().unwrap_or_default();
                        data.push((name, val));
                    }
                }
                "submit" | "button" | "reset" | "image" => {
                    // Submit buttons are not included in form data by default
                }
                "file" => {
                    // File inputs would need special handling — skip for now
                }
                _ => {
                    // ⛔ The VALUE. Reading the `value` ATTRIBUTE here meant a
                    // form submitted the author's default instead of what the
                    // user typed — and every existing test passed, because none
                    // of them types before collecting.
                    data.push((name, input_value(node)));
                }
            }
        }
        "select" => {
            // "For each option element ... whose selectedness is true and that
            // is not disabled, append an entry" — SELECTEDNESS, so what is
            // submitted is what the user picked rather than what the markup
            // defaulted to, and a control with nothing selected contributes
            // nothing at all.
            //
            // One entry PER selected option, which is how a `multiple` select
            // submits several values under one name.
            for option in crate::html::forms::list_of_options(node) {
                if option.selectedness && !option.attributes.contains_key("disabled") {
                    data.push((name.clone(), crate::html::forms::option_value(option)));
                }
            }
        }
        "textarea" => {
            let val = input_value(node);
            data.push((name, val));
        }
        _ => {
            for child in &node.children {
                collect_form_data_inner(child, data);
            }
        }
    }
}

/// Reset all form fields inside a <form> to their default values.
/// Text inputs reset to their original value attribute (from defaultValue).
/// Checkboxes/radios reset to their initial checked state.
/// Selects reset to the initially selected option.
pub fn reset_form(root: &mut WebCore, form_id: u32) {
    fn find_mut<'a>(n: &'a mut WebCore, t: u32) -> Option<&'a mut WebCore> {
        if n.node_id == t {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, t) {
                return Some(r);
            }
        }
        None
    }
    if let Some(form) = find_mut(root, form_id) {
        reset_form_inner(form);
    }
}

/// The **reset algorithm** for one control (HTML §4.10.23).
///
/// Every arm is now the same sentence: drop the STATE, clear its dirty flag,
/// and let the content attribute speak again. Nothing is copied anywhere,
/// because the default was never overwritten in the first place.
fn reset_form_inner(node: &mut WebCore) {
    match node.tag.as_str() {
        "input" => {
            let input_type = node
                .attributes
                .get("type")
                .cloned()
                .unwrap_or_else(|| "text".to_string());
            match input_type.as_str() {
                "checkbox" | "radio" => {
                    // Verbatim: "set its ... dirty checkedness flag back to
                    // false, ... set the checkedness of the element to true if
                    // the element has a `checked` content attribute and false
                    // if it does not".
                    node.checkedness = node.attributes.contains_key("checked");
                    node.dirty_checked = false;
                }
                // Buttons and file inputs have no resettable value here; every
                // other state carries one.
                "submit" | "reset" | "button" | "image" | "hidden" | "file" => {}
                _ => {
                    // "Set the dirty value flag to false", after which the
                    // value falls back to the `value` content attribute on its
                    // own — dropping the state IS the reset.
                    //
                    // ⛔ This used to read a `defaultValue` ATTRIBUTE, which is
                    // not a content attribute at all but the IDL name FOR
                    // `value`. Nothing ever wrote it, so the fallback did the
                    // work and reset restored the field to whatever the user
                    // had last typed — the identical bug `defaultChecked` had.
                    node.value_state = None;
                    node.dirty_value = false;
                    node.input_cursor = 0;
                    node.input_sel_anchor = 0;
                    // "Invoke the value sanitization algorithm, if the type
                    // attribute's current state defines one" — the reset
                    // algorithm's own last step, and the reason a range does
                    // not come back holding a step-mismatched default.
                    crate::html::forms::seed_input_value(node);
                }
            }
        }
        "textarea" => {
            // A `<textarea>`'s default value is its CHILD TEXT, so the same
            // move restores it: typing no longer edits those children.
            node.value_state = None;
            node.dirty_value = false;
            node.input_cursor = 0;
            node.input_sel_anchor = 0;
        }
        "select" => {
            // "Set the selectedness of all the option elements ... to true if
            // the option element has a `selected` attribute, and false
            // otherwise; set the dirtiness of all ... to false; and then have
            // the select element run the selectedness setting algorithm."
            crate::html::forms::reset_select(node);
        }
        _ => {
            for child in &mut node.children {
                reset_form_inner(child);
            }
        }
    }
}

/// Encode form data as application/x-www-form-urlencoded string.
pub fn encode_form_urlencoded(data: &[(String, String)]) -> String {
    // ⛔ IN ENTRY ORDER, not sorted. HTML runs the serializer over "a list of
    // name-value pairs", and a list's order is part of the answer: a server
    // reading repeated names sees them in the order the controls appear.
    // Sorting was here to make a `HashMap`'s arbitrary iteration order
    // repeatable for tests — the wrong fix for a container that could not hold
    // the data in the first place.
    data.iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// Build the submission URL for a form.
/// GET: appends encoded data as query string.
/// POST: returns action URL unchanged (data goes in body).
pub fn build_form_submit_url(action: &str, method: &str, data: &[(String, String)]) -> String {
    if method.eq_ignore_ascii_case("post") {
        action.to_string()
    } else {
        let encoded = encode_form_urlencoded(data);
        if encoded.is_empty() {
            action.to_string()
        } else {
            let sep = if action.contains('?') { "&" } else { "?" };
            format!("{}{}{}", action, sep, encoded)
        }
    }
}

/// Apply autofocus: find the first element with the `autofocus` attribute and focus it.
pub fn apply_autofocus(doc: &mut Document) {
    fn find_autofocus(node: &WebCore) -> Option<u32> {
        if node.attributes.contains_key("autofocus") && is_focusable_node(node) {
            return Some(node.node_id);
        }
        for child in &node.children {
            if let Some(id) = find_autofocus(child) {
                return Some(id);
            }
        }
        None
    }
    if let Some(id) = find_autofocus(&doc.root) {
        doc.focused_box = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn standalone_button_does_not_submit_current_page() {
        let mut doc = crate::parse_html("<button id='dark'>☽</button>");
        let button = doc.get_element_by_id("dark").unwrap();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = events.clone();
        let mut callback: Option<FormEventCallback> = Some(Box::new(move |event| {
            let name = match &event.kind {
                FormEventKind::Click(_) => "click",
                FormEventKind::Submit(_) => "submit",
                _ => "other",
            };
            captured.lock().unwrap().push(name.to_string());
        }));

        assert!(handle_form_click(&mut doc.root, button, &mut callback).is_some());
        assert_eq!(events.lock().unwrap().as_slice(), ["click"]);
    }

    #[test]
    fn submit_button_inside_form_still_submits() {
        let mut doc =
            crate::parse_html("<form action='/send'><button id='send'>Send</button></form>");
        let button = doc.get_element_by_id("send").unwrap();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured = events.clone();
        let mut callback: Option<FormEventCallback> = Some(Box::new(move |event| {
            let name = match &event.kind {
                FormEventKind::Click(_) => "click".to_string(),
                FormEventKind::Submit(action) => format!("submit:{action}"),
                _ => "other".to_string(),
            };
            captured.lock().unwrap().push(name);
        }));

        assert!(handle_form_click(&mut doc.root, button, &mut callback).is_some());
        assert_eq!(events.lock().unwrap().as_slice(), ["click", "submit:/send"]);
    }
}
