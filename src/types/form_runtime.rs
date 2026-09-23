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

    // Disabled elements don't respond to clicks, including controls disabled
    // by a containing fieldset except through its first legend.
    let target_node = find_ref(root, target)?;
    if is_actually_disabled(root, target) {
        return None;
    }

    // Read target info before mutation
    let (tag, input_type, name, id, value) = {
        let tag = target_node.tag.clone();
        let input_type = normalize_input_type(
            target_node
                .attributes
                .get("type")
                .map(|ty| ty.as_str())
                .unwrap_or("text"),
        );
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
                        let owner = form_owner_id(root, target);
                        let same_owner_ids = radio_ids_with_owner(root, owner);
                        fn uncheck_radios_by_id(
                            node: &mut WebCore,
                            name: &str,
                            ids: &HashSet<u32>,
                            except_id: u32,
                        ) {
                            if ids.contains(&node.node_id)
                                && node.attributes.get("name").map(|s| s.as_str()) == Some(name)
                                && node.node_id != except_id
                            {
                                node.checkedness = false;
                                node.dirty_checked = true;
                            }
                            for child in &mut node.children {
                                uncheck_radios_by_id(child, name, ids, except_id);
                            }
                        }
                        uncheck_radios_by_id(root, &name, &same_owner_ids, target);
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
                "submit" | "button" | "reset" | "image" => {
                    // Reset button: reset the parent form
                    if input_type == "reset" {
                        if let Some(form_id) = form_owner_id(root, target) {
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
                        if matches!(input_type.as_str(), "submit" | "image")
                            && form_owner_id(root, target).is_some()
                        {
                            let action = submitter_form_action(root, target);
                            cb(&FormEvent {
                                tag: "form".into(),
                                id: String::new(),
                                name: String::new(),
                                kind: FormEventKind::Submit(action),
                                element: target,
                            });
                        }
                    }
                    Some(input_type == "reset")
                }
                "text" | "password" | "email" | "search" | "url" | "tel" | "number" | "date"
                | "month" | "week" | "time" | "datetime-local" => {
                    // Text input clicked — set cursor to end of value
                    let node = find_mut(root, target)?;
                    let len = input_value(node).chars().count();
                    node.input_cursor = len;
                    node.input_sel_anchor = len;
                    Some(true)
                }
                "hidden" => None,
                "range" | "color" | "file" => None,
                _ => {
                    // Unknown input types are in the Text state.
                    let node = find_mut(root, target)?;
                    let len = input_value(node).chars().count();
                    node.input_cursor = len;
                    node.input_sel_anchor = len;
                    Some(true)
                }
            }
        }
        "button" => {
            let target_node2 = find_ref(root, target);
            let btn_type = normalize_button_type(
                target_node2
                    .and_then(|n| n.attributes.get("type").cloned())
                    .as_deref()
                    .unwrap_or("submit"),
            );
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
                if btn_type == "submit" && form_owner_id(root, target).is_some() {
                    let action = submitter_form_action(root, target);
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
                if let Some(fid) = form_owner_id(root, target) {
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

fn normalize_input_type(raw: &str) -> String {
    let ty = raw.trim().to_ascii_lowercase();
    match ty.as_str() {
        "hidden" | "text" | "search" | "tel" | "url" | "email" | "password" | "date" | "month"
        | "week" | "time" | "datetime-local" | "number" | "range" | "color" | "checkbox"
        | "radio" | "file" | "submit" | "image" | "reset" | "button" => ty,
        _ => "text".to_string(),
    }
}

fn normalize_button_type(raw: &str) -> String {
    let ty = raw.trim().to_ascii_lowercase();
    match ty.as_str() {
        "button" | "reset" | "submit" => ty,
        _ => "submit".to_string(),
    }
}

fn form_associated(node: &WebCore) -> bool {
    matches!(
        node.tag.as_str(),
        "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea" | "img"
    )
}

fn listed_element(node: &WebCore) -> bool {
    matches!(
        node.tag.as_str(),
        "button" | "fieldset" | "input" | "object" | "output" | "select" | "textarea"
    )
}

fn find_node(root: &WebCore, target_id: u32) -> Option<&WebCore> {
    if root.node_id == target_id {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_node(child, target_id) {
            return Some(found);
        }
    }
    None
}

fn find_node_mut(root: &mut WebCore, target_id: u32) -> Option<&mut WebCore> {
    if root.node_id == target_id {
        return Some(root);
    }
    for child in &mut root.children {
        if let Some(found) = find_node_mut(child, target_id) {
            return Some(found);
        }
    }
    None
}

fn find_by_html_id<'a>(root: &'a WebCore, html_id: &str) -> Option<&'a WebCore> {
    if root
        .attributes
        .get("id")
        .is_some_and(|value| value == html_id)
    {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_by_html_id(child, html_id) {
            return Some(found);
        }
    }
    None
}

fn parent_id(root: &WebCore, target_id: u32) -> Option<u32> {
    for child in &root.children {
        if child.node_id == target_id {
            return Some(root.node_id);
        }
        if let Some(found) = parent_id(child, target_id) {
            return Some(found);
        }
    }
    None
}

fn ancestor_form_id(root: &WebCore, target_id: u32) -> Option<u32> {
    let mut cursor = parent_id(root, target_id);
    while let Some(id) = cursor {
        let node = find_node(root, id)?;
        if node.tag == "form" {
            return Some(id);
        }
        cursor = parent_id(root, id);
    }
    None
}

pub fn form_owner_id(root: &WebCore, target_id: u32) -> Option<u32> {
    let node = find_node(root, target_id)?;
    if !form_associated(node) {
        return None;
    }
    if let Some(form_id) = node.attributes.get("form") {
        if let Some(form) = find_by_html_id(root, form_id) {
            if form.tag == "form" {
                return Some(form.node_id);
            }
        }
    }
    ancestor_form_id(root, target_id)
}

fn first_legend_child_id(fieldset: &WebCore) -> Option<u32> {
    fieldset
        .children
        .iter()
        .find(|child| child.tag == "legend")
        .map(|child| child.node_id)
}

fn is_actually_disabled(root: &WebCore, target_id: u32) -> bool {
    if find_node(root, target_id).is_some_and(|node| node.attributes.contains_key("disabled")) {
        return true;
    }
    let mut child_id = target_id;
    let mut cursor = parent_id(root, target_id);
    while let Some(id) = cursor {
        let Some(node) = find_node(root, id) else {
            return false;
        };
        if node.tag == "fieldset" && node.attributes.contains_key("disabled") {
            if first_legend_child_id(node) != Some(child_id) {
                return true;
            }
        }
        child_id = id;
        cursor = parent_id(root, id);
    }
    false
}

fn radio_ids_with_owner(root: &WebCore, owner: Option<u32>) -> HashSet<u32> {
    let mut ids = HashSet::new();
    fn walk(root: &WebCore, node: &WebCore, owner: Option<u32>, ids: &mut HashSet<u32>) {
        if node.tag == "input"
            && normalize_input_type(
                node.attributes
                    .get("type")
                    .map(|s| s.as_str())
                    .unwrap_or("text"),
            ) == "radio"
            && form_owner_id(root, node.node_id) == owner
        {
            ids.insert(node.node_id);
        }
        for child in &node.children {
            walk(root, child, owner, ids);
        }
    }
    walk(root, root, owner, &mut ids);
    ids
}

fn submitter_form_action(root: &WebCore, submitter_id: u32) -> String {
    let Some(form_id) = form_owner_id(root, submitter_id) else {
        return String::new();
    };
    if let Some(submitter) = find_node(root, submitter_id) {
        if let Some(action) = submitter.attributes.get("formaction") {
            return action.clone();
        }
    }
    find_node(root, form_id)
        .and_then(|form| form.attributes.get("action").cloned())
        .unwrap_or_default()
}

pub fn submitter_form_method(root: &WebCore, submitter_id: u32) -> String {
    let Some(form_id) = form_owner_id(root, submitter_id) else {
        return "get".to_string();
    };
    let raw = find_node(root, submitter_id)
        .and_then(|submitter| submitter.attributes.get("formmethod"))
        .or_else(|| find_node(root, form_id).and_then(|form| form.attributes.get("method")))
        .map(|method| method.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "get".to_string());
    match raw.as_str() {
        "post" | "dialog" => raw,
        _ => "get".to_string(),
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

/// Find the action URL of a submitter's form owner.
pub fn find_parent_form_action(root: &WebCore, target_id: u32) -> String {
    submitter_form_action(root, target_id)
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
    collect_form_data_inner(form, form, &mut data);
    data
}

pub fn collect_form_data_for_form(root: &WebCore, form_id: u32) -> Vec<(String, String)> {
    let mut data = Vec::new();
    let mut seen = HashSet::new();
    if let Some(form) = find_node(root, form_id) {
        collect_form_descendant_data(root, form, &mut seen, &mut data);
    }
    collect_explicit_form_data(root, root, form_id, &mut seen, &mut data);
    collect_following_orphan_form_data(root, form_id, &mut seen, &mut data);
    data
}

fn collect_form_descendant_data(
    root: &WebCore,
    node: &WebCore,
    seen: &mut HashSet<u32>,
    data: &mut Vec<(String, String)>,
) {
    if listed_element(node)
        && !(node.tag == "input"
            && normalize_input_type(
                node.attributes
                    .get("type")
                    .map(|s| s.as_str())
                    .unwrap_or("text"),
            ) == "image")
        && !is_actually_disabled(root, node.node_id)
        && seen.insert(node.node_id)
    {
        append_successful_control(node, data);
    }
    for child in &node.children {
        collect_form_descendant_data(root, child, seen, data);
    }
}

fn collect_explicit_form_data(
    root: &WebCore,
    node: &WebCore,
    form_id: u32,
    seen: &mut HashSet<u32>,
    data: &mut Vec<(String, String)>,
) {
    if listed_element(node)
        && !seen.contains(&node.node_id)
        && node.attributes.contains_key("form")
        && form_owner_id(root, node.node_id) == Some(form_id)
        && !is_actually_disabled(root, node.node_id)
    {
        seen.insert(node.node_id);
        append_successful_control(node, data);
    }
    for child in &node.children {
        collect_explicit_form_data(root, child, form_id, seen, data);
    }
}

fn collect_following_orphan_form_data(
    root: &WebCore,
    form_id: u32,
    seen: &mut HashSet<u32>,
    data: &mut Vec<(String, String)>,
) {
    fn walk(
        root: &WebCore,
        node: &WebCore,
        form_id: u32,
        after_form: &mut bool,
        seen: &mut HashSet<u32>,
        data: &mut Vec<(String, String)>,
    ) {
        if node.node_id == form_id {
            *after_form = true;
        } else if *after_form && node.tag == "form" {
            *after_form = false;
        } else if *after_form
            && listed_element(node)
            && !seen.contains(&node.node_id)
            && node.attributes.get("form").is_none()
            && ancestor_form_id(root, node.node_id).is_none()
            && !is_actually_disabled(root, node.node_id)
        {
            seen.insert(node.node_id);
            append_successful_control(node, data);
        }

        for child in &node.children {
            walk(root, child, form_id, after_form, seen, data);
        }
    }

    let mut after_form = false;
    walk(root, root, form_id, &mut after_form, seen, data);
}

fn collect_form_data_inner(node: &WebCore, root: &WebCore, data: &mut Vec<(String, String)>) {
    if listed_element(node) && !is_actually_disabled(root, node.node_id) {
        append_successful_control(node, data);
    }
    for child in &node.children {
        collect_form_data_inner(child, root, data);
    }
}

fn append_successful_control(node: &WebCore, data: &mut Vec<(String, String)>) {
    if matches!(
        node.tag.as_str(),
        "button" | "fieldset" | "object" | "output"
    ) {
        return;
    }
    let name = match node.attributes.get("name") {
        Some(n) if !n.is_empty() => n.clone(),
        _ => return,
    };
    match node.tag.as_str() {
        "input" => {
            let input_type = normalize_input_type(
                node.attributes
                    .get("type")
                    .map(|s| s.as_str())
                    .unwrap_or("text"),
            );
            match input_type.as_str() {
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
                        let val = node
                            .attributes
                            .get("value")
                            .cloned()
                            .unwrap_or_else(|| "on".to_string());
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
            crate::html::forms::for_each_option(node, &mut |option, group_disabled| {
                if option.selectedness
                    && !crate::html::forms::option_is_disabled(option, group_disabled)
                {
                    data.push((name.clone(), crate::html::forms::option_value(option)));
                }
            });
        }
        "textarea" => {
            let val = input_value(node);
            data.push((name, val));
        }
        _ => {}
    }
}

/// Reset all form fields inside a <form> to their default values.
/// Text inputs reset to their original value attribute (from defaultValue).
/// Checkboxes/radios reset to their initial checked state.
/// Selects reset to the initially selected option.
pub fn reset_form(root: &mut WebCore, form_id: u32) {
    let control_ids = form_control_ids(root, form_id);
    for control_id in control_ids {
        if let Some(control) = find_node_mut(root, control_id) {
            reset_control(control);
        }
    }
}

fn form_control_ids(root: &WebCore, form_id: u32) -> Vec<u32> {
    let mut ids = Vec::new();
    fn walk(root: &WebCore, node: &WebCore, form_id: u32, ids: &mut Vec<u32>) {
        if listed_element(node)
            && !(node.tag == "input"
                && normalize_input_type(
                    node.attributes
                        .get("type")
                        .map(|s| s.as_str())
                        .unwrap_or("text"),
                ) == "image")
            && form_owner_id(root, node.node_id) == Some(form_id)
        {
            ids.push(node.node_id);
        }
        for child in &node.children {
            walk(root, child, form_id, ids);
        }
    }
    walk(root, root, form_id, &mut ids);
    ids
}

/// The **reset algorithm** for one control (HTML §4.10.23).
///
/// Every arm is now the same sentence: drop the STATE, clear its dirty flag,
/// and let the content attribute speak again. Nothing is copied anywhere,
/// because the default was never overwritten in the first place.
fn reset_control(node: &mut WebCore) {
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
        _ => {}
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
