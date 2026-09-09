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
        // Dispatch to listeners first so they can prevent default handling.
        let mut evt = crate::dom::HtmlEvent::new(etype);
        evt.key_code = key_code;
        evt.char_code = ch;
        evt.ctrl_key = ctrl;
        evt.shift_key = shift;
        evt.alt_key = alt;
        evt.meta_key = meta;

        let (handled, mut evt) = self.dispatch_input_event(evt);

        // Also dispatch through NodeId-based event system (capture/bubble).
        let target = if self.focused_box != 0 {
            self.focused_box
        } else {
            self.root.node_id
        };
        {
            let mut dom_evt = crate::dom::events::DomEvent::new(etype.as_str(), target);
            dom_evt.key_code = key_code;
            dom_evt.char_code = ch;
            dom_evt.ctrl_key = ctrl;
            dom_evt.shift_key = shift;
            dom_evt.alt_key = alt;
            dom_evt.meta_key = meta;
            // `KeyboardEvent.key` — the character or named key. It was left
            // empty, so a listener reading `event.key` (the member the spec
            // points every keyboard handler at) got nothing at all.
            dom_evt.key = match ch {
                Some(c) => c.to_string(),
                None => crate::dom::events::key_name_for_code(key_code).to_string(),
            };
            if self.dispatch_dom_event(&mut dom_evt) {
                // handled via new system
            }
            if dom_evt.default_prevented() {
                evt.default_prevented = true;
            }
        }

        let mut redraw = handled;

        if !evt.default_prevented {
            if self.svg_trigger_event(target, etype.as_str()) {
                redraw = true;
            }
            if etype == crate::dom::HtmlEventType::KeyDown {
                let key = ch
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| crate::dom::events::key_name_for_code(key_code).to_string());
                if self.svg_trigger_access_key(&key) {
                    redraw = true;
                }
            }

            // Check if a form input is focused — route keys there first
            let form_handled = if self.focused_box != 0
                && etype == crate::dom::HtmlEventType::KeyDown
            {
                let focused = self.get_node(self.focused_box).unwrap_or(&self.root);
                // Select: arrow up/down changes selected option
                if focused.tag == "select" && (key_code == 38 || key_code == 40) {
                    let fid = self.focused_box;
                    // Arrow keys move to the next or previous option and PICK
                    // it, which is the same algorithm a click runs — HTML lists
                    // "through a menu command, or through any other mechanism"
                    // alongside the click for exactly this reason.
                    let options = self
                        .find_webcore(fid)
                        .map(crate::html::forms::option_ids)
                        .unwrap_or_default();
                    if !options.is_empty() {
                        // With nothing selected — a list box's resting state —
                        // Down starts at the first option and Up at the last.
                        let cur = self
                            .find_webcore(fid)
                            .map(crate::html::forms::selected_index)
                            .unwrap_or(-1);
                        let new_idx = if cur < 0 {
                            if key_code == 40 {
                                0
                            } else {
                                options.len() - 1
                            }
                        } else if key_code == 40 {
                            ((cur as usize) + 1).min(options.len() - 1)
                        } else {
                            (cur as usize).saturating_sub(1)
                        };
                        let option_id = options[new_idx];
                        let new_text = self
                            .find_webcore(option_id)
                            .map(crate::html::forms::option_label)
                            .unwrap_or_default();
                        if let Some(sel) = self.find_webcore_mut(fid) {
                            let changed = crate::html::forms::pick_option(sel, option_id);
                            if let Some(tn) =
                                sel.children.iter_mut().rev().find(|c| c.tag == "#text")
                            {
                                tn.text = new_text;
                            }
                            sel.layout.layout_dirty = true;
                            if changed {
                                self.style_dirty = true;
                                self.send_select_update_notifications(fid);
                            }
                        }
                    }
                    true
                }
                // Number input: arrow up/down increments/decrements
                else if focused.tag == "input"
                    && focused.attributes.get("type").map(|s| s.as_str()) == Some("number")
                    && (key_code == 38 || key_code == 40)
                {
                    let fid = self.focused_box;
                    fn find_n<'a>(n: &'a mut WebCore, t: u32) -> Option<&'a mut WebCore> {
                        if n.node_id == t {
                            return Some(n);
                        }
                        for c in &mut n.children {
                            if let Some(r) = find_n(c, t) {
                                return Some(r);
                            }
                        }
                        None
                    }
                    if let Some(input) = find_n(&mut self.root, fid) {
                        // Read the VALUE, not the default value — an arrow key
                        // after typing used to step from whatever the markup
                        // said rather than from what the field shows.
                        let val: f64 =
                            crate::html::forms::parse_floating_point(&input_value(input))
                                .unwrap_or(0.0);
                        let step: f64 = input
                            .attributes
                            .get("step")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1.0);
                        let min: Option<f64> =
                            input.attributes.get("min").and_then(|s| s.parse().ok());
                        let max: Option<f64> =
                            input.attributes.get("max").and_then(|s| s.parse().ok());
                        let new_val = if key_code == 38 {
                            val + step
                        } else {
                            val - step
                        };
                        let new_val = if let Some(mx) = max {
                            new_val.min(mx)
                        } else {
                            new_val
                        };
                        let new_val = if let Some(mn) = min {
                            new_val.max(mn)
                        } else {
                            new_val
                        };
                        if new_val != val {
                            input.value_state =
                                Some(crate::html::forms::best_representation(new_val));
                            input.dirty_value = true;
                            input.layout.layout_dirty = true;
                        }
                    }
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
                        let changed = process_form_input_key(input, key_code, ch, ctrl, shift);
                        // Reset caret blink so it stays visible while typing
                        self.caret_blink_epoch = std::time::Instant::now();
                        if changed {
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
                // Typing went through `process_form_input_key`, which takes
                // `&mut WebCore` and so updated `value` on the render tree
                // only. Reconcile before the DOM is read. Deferred to here
                // rather than done at the call site because `input` borrows
                // `self.root` for the whole block above.
                self.sync_form_state_to_arena();
                // Typing changes `:in-range`, `:valid` and friends the same way
                // a click changes `:checked` — see the note at the click path.
                self.style_dirty = true;
            } else if {
                // ⛔ The editor mutates the render tree with no arena in
                // scope, so the DOM has to be told afterwards — see
                // `resync_subtree`.
                let mut editor = std::mem::take(&mut self.editor);
                let handled = editor.handle_key_event(&mut self.root, etype, key_code, ch, ctrl);
                self.editor = editor;
                if handled {
                    let mut root = std::mem::replace(&mut self.root, WebCore::new("#placeholder"));
                    crate::html::arena_wiring::resync_subtree(&mut self.arena, &mut root);
                    self.root = root;
                }
                handled
            } {
                redraw = true;
            }
        }

        redraw
    }
}
