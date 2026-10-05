//! Keyboard input.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

impl Document {
    /// High-level keyboard event entry point.
    pub fn process_key_event(
        &mut self,
        etype: crate::dom::HtmlEventType,
        key_code: u32,
        ch: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> bool {
        self.process_key_event_with_native_navigation(
            etype,
            key_code,
            ch,
            ctrl,
            shift,
            alt,
            meta,
            &mut |_, _, _| false,
        )
    }

    pub(crate) fn process_key_event_with_native_navigation(
        &mut self,
        etype: crate::dom::HtmlEventType,
        key_code: u32,
        ch: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
        navigate: &mut dyn FnMut(&mut WebCore, u32, bool) -> bool,
    ) -> bool {
        let (handled, evt) =
            self.dispatch_keyboard_input(etype, key_code, ch, ctrl, shift, alt, meta);
        self.process_keyboard_defaults_with_native_navigation(evt, handled, navigate)
    }

    pub(crate) fn dispatch_keyboard_input(
        &mut self,
        etype: crate::dom::HtmlEventType,
        key_code: u32,
        ch: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> (bool, crate::dom::HtmlEvent) {
        self.keyboard_activation_target = None;
        let mut evt = crate::dom::HtmlEvent::new(etype);
        evt.key_code = key_code;
        evt.char_code = ch;
        evt.ctrl_key = ctrl;
        evt.shift_key = shift;
        evt.alt_key = alt;
        evt.meta_key = meta;

        evt.target = if self.focused_box != 0 {
            self.focused_box
        } else {
            self.root.node_id
        };
        self.dispatch_input_event(evt)
    }

    pub(crate) fn process_keyboard_defaults_with_native_navigation(
        &mut self,
        evt: crate::dom::HtmlEvent,
        handled: bool,
        navigate: &mut dyn FnMut(&mut WebCore, u32, bool) -> bool,
    ) -> bool {
        let etype = evt.event_type;
        let key_code = evt.key_code;
        let ch = evt.char_code;
        let (ctrl, shift, alt, meta) = (evt.ctrl_key, evt.shift_key, evt.alt_key, evt.meta_key);
        let target = evt.target;
        let mut redraw = handled;
        let space_release = if etype == HtmlEventType::KeyUp && key_code == 32 {
            let pressed = std::mem::take(&mut self.keyboard_space_target);
            if self.active_box == pressed && pressed != 0 {
                self.active_box = 0;
                self.style_dirty = true;
                redraw = true;
            }
            pressed == target && pressed != 0
        } else {
            false
        };

        if !evt.default_prevented {
            if etype == crate::dom::HtmlEventType::KeyDown
                && key_code == 27
                && self.open_picker != 0
            {
                self.open_picker = 0;
                self.picker_calendar = None;
                self.picker_time = None;
                return true;
            }
            if etype == crate::dom::HtmlEventType::KeyDown
                && self.open_picker != 0
                && self.picker_time.is_some()
                && !ctrl
                && !meta
                && !alt
            {
                if key_code == 13 {
                    let value = self.time_picker_value(self.open_picker).unwrap();
                    if self.picker_value_allowed(self.open_picker, &value) {
                        self.commit_picker_value(self.open_picker, &value);
                        self.open_picker = 0;
                        self.picker_time = None;
                    }
                    return true;
                }
                if key_code == 9 {
                    self.open_picker = 0;
                    self.picker_time = None;
                } else if let Some(draft) = self.picker_time.as_mut() {
                    match key_code {
                        37 => draft.move_active(-1),
                        39 => draft.move_active(1),
                        38 => draft.adjust(draft.active, 1),
                        40 => draft.adjust(draft.active, -1),
                        36 => draft.set_endpoint(false),
                        35 => draft.set_endpoint(true),
                        _ => {
                            if let Some(digit) = ch.and_then(|ch| ch.to_digit(10)) {
                                draft.digit(digit);
                            }
                        }
                    }
                    return true;
                }
            }
            if etype == crate::dom::HtmlEventType::KeyDown && (ctrl || meta) && !alt {
                if let Some(key) = ch.or_else(|| char::from_u32(key_code)) {
                    if let Some(changed) = self.clipboard_shortcut(key) {
                        return changed || redraw;
                    }
                }
            }
            let (enter_activates, space_activates) = self
                .get_node(target)
                .map(|node| match node.tag.as_str() {
                    "a" | "area" => (node.attributes.contains_key("href"), false),
                    "button" => (true, true),
                    "input" => match self.input_type(target).as_str() {
                        "button" | "submit" | "reset" | "image" => (true, true),
                        "checkbox" | "radio" => (false, true),
                        _ => (false, false),
                    },
                    _ => (false, false),
                })
                .unwrap_or((false, false));
            if !ctrl
                && !meta
                && !alt
                && self.focused_box == target
                && !self.is_actually_disabled(target)
                && !self.is_inert(target)
            {
                if space_activates && etype == HtmlEventType::KeyDown && key_code == 32 {
                    self.keyboard_space_target = target;
                    self.active_box = target;
                    self.style_dirty = true;
                    return true;
                }
                if enter_activates && etype == HtmlEventType::KeyDown && key_code == 13
                    || space_activates && space_release
                {
                    let mut click = crate::dom::HtmlEvent::new(HtmlEventType::Click);
                    click.target = target;
                    let (handled, click) = self.dispatch_input_event(click);
                    if !click.default_prevented
                        && self.get_node(target).is_some()
                        && !self.is_actually_disabled(target)
                        && !self.is_inert(target)
                    {
                        self.keyboard_activation_target = Some(target);
                        self.activate_control_click(target);
                    }
                    return handled || redraw || !click.default_prevented;
                }
            }
            if etype == crate::dom::HtmlEventType::KeyDown
                && key_code == 27
                && self.open_select == 0
                && self.open_picker == 0
            {
                if let Some(modal) = self.active_modal_dialog() {
                    if self
                        .get_attribute(modal, "closedby")
                        .is_none_or(|value| !value.eq_ignore_ascii_case("none"))
                    {
                        return self.request_close_dialog(modal) || redraw;
                    }
                }
            }
            if etype == crate::dom::HtmlEventType::KeyDown
                && self.open_select != 0
                && matches!(key_code, 13 | 27 | 9)
            {
                self.open_select = 0;
                self.dropdown_hover_idx = -1;
                self.dropdown_scroll = 0.0;
                if key_code != 9 {
                    return true;
                }
                redraw = true;
            }
            if self.svg_trigger_event(target, etype.as_str()) {
                redraw = true;
            }
            if etype == crate::dom::HtmlEventType::KeyDown {
                if self.focused_box != 0
                    && self.is_media_element(self.focused_box)
                    && matches!(key_code, 13 | 32)
                {
                    if self.media_toggle_playback(self.focused_box) {
                        redraw = true;
                    }
                }
                let key = ch
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| crate::dom::events::key_name_for_code(key_code).to_string());
                if self.svg_trigger_access_key(&key) {
                    redraw = true;
                }
            }

            // Check if a form input is focused — route keys there first
            let mut form_value_changed = false;
            let form_handled = if self.focused_box != 0
                && etype == crate::dom::HtmlEventType::KeyDown
                && !self.is_actually_disabled(self.focused_box)
                && !self.is_inert(self.focused_box)
            {
                let focused = self.get_node(self.focused_box).unwrap_or(&self.root);
                let select_all = focused.tag == "select"
                    && crate::html::forms::is_multiple(focused)
                    && (ctrl || meta)
                    && (key_code == 65 || ch.is_some_and(|ch| ch == 'a' || ch == 'A'));
                // Navigate the enabled options, retaining their DOM indices.
                if focused.tag == "select"
                    && (matches!(key_code, 35 | 36 | 38 | 40)
                        || select_all
                        || (key_code == 32 && crate::html::forms::is_list_box(focused)))
                {
                    let fid = self.focused_box;
                    let multiple = crate::html::forms::is_multiple(focused);
                    let listbox = crate::html::forms::is_list_box(focused);
                    // Keep the active row distinct from the first selected row,
                    // so successive Shift arrows extend the same range.
                    let mut options = Vec::new();
                    if let Some(select) = self.find_webcore(fid) {
                        if !self.is_actually_disabled(fid) {
                            let mut index = 0;
                            crate::html::forms::for_each_option(
                                select,
                                &mut |option, group_disabled| {
                                    if !crate::html::forms::option_is_disabled(
                                        option,
                                        group_disabled,
                                    ) {
                                        options.push((index, option.node_id));
                                    }
                                    index += 1;
                                },
                            );
                        }
                    }
                    if !options.is_empty() {
                        // With nothing selected — a list box's resting state —
                        // Down starts at the first option and Up at the last.
                        let cur = self
                            .listbox_active_options
                            .get(&fid)
                            .and_then(|id| {
                                options
                                    .iter()
                                    .find(|(_, option_id)| id == option_id)
                                    .map(|(index, _)| *index)
                            })
                            .unwrap_or_else(|| {
                                self.find_webcore(fid)
                                    .map(crate::html::forms::selected_index)
                                    .unwrap_or(-1)
                            });
                        let next = match key_code {
                            32 => options.iter().find(|(index, _)| *index == cur),
                            36 => options.first(),
                            35 => options.last(),
                            40 => options.iter().find(|(index, _)| *index > cur),
                            _ if cur < 0 => options.last(),
                            _ => options.iter().rev().find(|(index, _)| *index < cur),
                        };
                        let next = if select_all {
                            self.listbox_anchors.insert(fid, options[0].1);
                            options.last()
                        } else {
                            next
                        };
                        if let Some(&(_, option_id)) = next {
                            let move_only = listbox
                                && multiple
                                && (ctrl || meta)
                                && !shift
                                && !select_all
                                && key_code != 32;
                            let changed = if move_only {
                                self.listbox_active_options.insert(fid, option_id);
                                false
                            } else if listbox {
                                self.select_list_box_option(
                                    fid,
                                    option_id,
                                    multiple && key_code == 32 && (ctrl || meta),
                                    multiple && (shift || select_all),
                                )
                            } else {
                                let changed = self.find_webcore_mut(fid).is_some_and(|sel| {
                                    crate::html::forms::pick_option(sel, option_id)
                                });
                                if changed {
                                    self.style_dirty = true;
                                    self.send_select_update_notifications(fid);
                                }
                                changed
                            };
                            if changed {
                                form_value_changed = true;
                            }
                            if listbox {
                                self.reveal_list_box_option(fid, option_id);
                            }
                        }
                        self.reveal_select_popup_selection();
                    }
                    true
                }
                // Number input: arrow up/down increments/decrements
                else if focused.tag == "input"
                    && focused.attributes.get("type").map(|s| s.as_str()) == Some("number")
                    && (key_code == 38 || key_code == 40)
                {
                    form_value_changed = self.step_number_input(self.focused_box, key_code == 38);
                    true
                } else if is_text_input(focused) {
                    // Find the focused node mutably and process the key
                    let fid = self.focused_box;
                    fn find_input<'a>(n: &'a mut WebCore, t: u32) -> Option<&'a mut WebCore> {
                        if n.node_id == t {
                            return Some(n);
                        }
                        for c in &mut n.children {
                            if let Some(r) = find_input(c, t) {
                                return Some(r);
                            }
                        }
                        None
                    }
                    if let Some(input) = find_input(&mut self.root, fid) {
                        let old_value = input_value(input);
                        let changed = if input.tag == "textarea"
                            && matches!(key_code, 38 | 40)
                            && !ctrl
                            && !meta
                            && !alt
                        {
                            navigate(input, key_code, shift)
                        } else {
                            process_form_input_key(input, key_code, ch, ctrl || meta, shift)
                        };
                        form_value_changed = old_value != input_value(input);
                        // Reset caret blink so it stays visible while typing
                        self.caret_blink_epoch = std::time::Instant::now();
                        self.editor.caret_visible = true;
                        self.editor.last_blink = self.caret_blink_epoch;
                        if form_value_changed {
                            // Fire form event callback
                            if let Some(ref mut cb) = self.on_form_event {
                                cb(&FormEvent {
                                    tag: input.tag.clone(),
                                    id: input.attributes.get("id").cloned().unwrap_or_default(),
                                    name: input.attributes.get("name").cloned().unwrap_or_default(),
                                    kind: FormEventKind::Input(input_value(input)),
                                    element: fid,
                                });
                            }
                        }
                        changed
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if form_handled {
                redraw = true;
                if form_value_changed {
                    // Typing changes `:in-range`, `:valid` and friends the same way
                    // a click changes `:checked` — see the note at the click path.
                    self.style_dirty = true;
                }
            } else if {
                if !editor_fallback_may_edit(&self.root, &self.editor, self.focused_box) {
                    false
                } else {
                    // ⛔ The editor mutates the render tree with no arena in
                    // scope, so the DOM has to be told afterwards — see
                    // `resync_subtree`.
                    let mut editor = std::mem::take(&mut self.editor);
                    let handled = editor.handle_key_event_with_modifiers(
                        &mut self.root,
                        etype,
                        key_code,
                        ch,
                        ctrl || meta,
                        shift,
                    );
                    self.editor = editor;
                    // Horizontal navigation changes only Editor state, not nodes.
                    if handled && !matches!(key_code, 37 | 39) {
                        let mut root =
                            std::mem::replace(&mut self.root, WebCore::new("#placeholder"));
                        crate::html::arena_wiring::resync_subtree(&mut self.arena, &mut root);
                        self.root = root;
                    }
                    handled
                }
            } {
                redraw = true;
            }
        }

        redraw
    }

    pub(crate) fn step_number_input(&mut self, id: u32, up: bool) -> bool {
        if self.is_actually_disabled(id) {
            return false;
        }
        let changed = {
            let Some(input) = self.find_webcore_mut(id) else {
                return false;
            };
            if input.attributes.contains_key("disabled")
                || input.attributes.contains_key("readonly")
            {
                return false;
            }
            let old = crate::html::forms::parse_floating_point(&input_value(input)).unwrap_or(0.0);
            let step = input
                .attributes
                .get("step")
                .and_then(|value| value.parse::<f64>().ok())
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(1.0);
            let min = input
                .attributes
                .get("min")
                .and_then(|value| crate::html::forms::parse_floating_point(value));
            let max = input
                .attributes
                .get("max")
                .and_then(|value| crate::html::forms::parse_floating_point(value));
            let mut next = old + if up { step } else { -step };
            if let Some(max) = max {
                next = next.min(max);
            }
            if let Some(min) = min {
                next = next.max(min);
            }
            if next == old {
                None
            } else {
                let value = crate::html::forms::best_representation(next);
                input.value_state = Some(value.clone());
                input.dirty_value = true;
                input.layout.layout_dirty = true;
                Some((
                    input.attributes.get("id").cloned().unwrap_or_default(),
                    input.attributes.get("name").cloned().unwrap_or_default(),
                    value,
                ))
            }
        };
        if let Some((field_id, name, value)) = changed {
            if let Some(callback) = &mut self.on_form_event {
                callback(&FormEvent {
                    tag: "input".to_string(),
                    id: field_id,
                    name,
                    kind: FormEventKind::Input(value),
                    element: id,
                });
            }
            true
        } else {
            false
        }
    }
}

fn editor_fallback_may_edit(root: &WebCore, editor: &Editor, focused_id: u32) -> bool {
    if focused_id != 0 && is_native_form_control_descendant(root, focused_id) {
        return false;
    }
    editor
        .caret_info()
        .map(|(caret_id, _)| {
            !editor.read_only || crate::dom::is_in_contenteditable_by_id(root, caret_id)
        })
        .unwrap_or(false)
}

fn is_native_form_control_descendant(root: &WebCore, id: u32) -> bool {
    fn walk(node: &WebCore, id: u32, in_control: bool) -> Option<bool> {
        let owns_control = matches!(
            node.tag.as_str(),
            "input" | "select" | "textarea" | "button" | "option"
        );
        if node.node_id == id {
            return Some(in_control || owns_control);
        }
        let next_in_control = in_control || owns_control;
        for child in &node.children {
            if let Some(found) = walk(child, id, next_in_control) {
                return Some(found);
            }
        }
        None
    }
    walk(root, id, false).unwrap_or(false)
}
