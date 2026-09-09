//! Mouse input: hit-testing a point and dispatching the events it implies.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

impl Document {
    /// A pointer event with no modifier keys held.
    ///
    /// The plain spelling, kept because almost every caller has no modifiers to
    /// report and a `false, false, false, false` tail at each of them says
    /// nothing. `process_mouse_event_with_modifiers` is the full event.
    pub fn process_mouse_event(
        &mut self,
        etype: crate::dom::HtmlEventType,
        doc_pt: (f32, f32),
        button: u8,
    ) -> bool {
        self.process_mouse_event_with_modifiers(etype, doc_pt, button, false, false, false, false)
    }

    /// A pointer event, with the modifier keys that were held.
    ///
    /// Modifiers are part of a pointer event, not decoration: HTML's list box
    /// asks the user agent to "allow the user to request that the option whose
    /// selectedness is true, if any, be unselected", and every platform spells
    /// that request as a modified click. Without them the control had a correct
    /// algorithm and no way to reach it.
    ///
    /// Four bools rather than a struct, matching `process_key_event`, which
    /// already carries its modifiers this way — one convention for both halves
    /// of the input surface.
    pub fn process_mouse_event_with_modifiers(
        &mut self,
        etype: crate::dom::HtmlEventType,
        doc_pt: (f32, f32),
        button: u8,
        ctrl: bool,
        _shift: bool,
        _alt: bool,
        meta: bool,
    ) -> bool {
        // Ctrl on the platforms that use it, ⌘ on the one that does not — the
        // same pair every other modified gesture answers to.
        let unselect_request = ctrl || meta;
        use crate::dom::{HtmlEvent, HtmlEventType};
        // client_pos = screen-space logical coordinates (doc coords minus scroll).
        let client_pos = (doc_pt.0, doc_pt.1 - self.scroll_y);

        let mut evt = HtmlEvent::new(etype);
        evt.doc_pos = doc_pt;
        evt.client_pos = client_pos;
        evt.button = button;
        let hit_result = crate::layout::hit_test::point_to_hit(&self.root, doc_pt, button);
        let mut hit_node_id: u32 = hit_result.as_ref().map(|h| h.node_id).unwrap_or(0);
        // For inline links: check if the hit point is inside an inline run
        // with an href. If so, find the ancestor <a> element for hover styling.
        if let Some(ref hr) = hit_result {
            if let Some(hit_box) = self.get_node(hr.node_id) {
                for run in &hit_box.layout.inline_runs {
                    if hr.local_offset >= run.text_offset
                        && hr.local_offset < run.text_offset + run.length
                    {
                        if !run.style.href.is_empty() {
                            if let Some(link_id) = find_link_node_id(&self.root, &run.style.href) {
                                hit_node_id = link_id;
                            }
                        }
                        break;
                    }
                }
            }
        }
        evt.target = hit_node_id;

        let mut redraw = false;
        match etype {
            HtmlEventType::MouseMove => {
                if hit_node_id != 0 && self.svg_trigger_event(hit_node_id, etype.as_str()) {
                    redraw = true;
                }
                // A held knob follows the pointer, wherever it has got to —
                // including outside the control, which is why this is keyed on
                // the element being HELD and not on what the move hit.
                if self.dragging_range != 0 {
                    let range_id = self.dragging_range;
                    if self.drag_range_to(range_id, doc_pt) {
                        redraw = true;
                    }
                }
                // After a hover-triggered relayout (e.g. dropdown opens), the
                // layout changes and re-hit-testing at the same mouse position may
                // find a different element, causing a feedback loop
                // (open → re-hit → close → re-hit → open …).
                // Suppress one hover change after each hover-triggered relayout.
                if self.hover_suppress_count > 0 {
                    self.hover_suppress_count -= 1;
                } else if self.hovered_box != hit_node_id {
                    self.hovered_box = hit_node_id;
                    self.hover_changed = true;
                    redraw = true;
                }
                // Track hover over open dropdown
                if self.open_select != 0 {
                    let open_sel_id = self.open_select;
                    let sel = match self.get_node(open_sel_id) {
                        Some(s) => s,
                        None => {
                            self.open_select = 0;
                            return redraw;
                        }
                    };
                    let dropdown_y = sel.layout.border_rect.y + sel.layout.border_rect.h;
                    let font_px = sel.style.font_size_px(16.0, 16.0);
                    let item_h = font_px * 1.8;
                    let group_h = font_px * 1.5;
                    let mut y_acc = 0.0f32;
                    let mut new_hover: i32 = -1;
                    let mut opt_i = 0usize;
                    let rel_y = doc_pt.1 - dropdown_y - 4.0;
                    for child in &sel.children {
                        if child.tag == "option" {
                            if rel_y >= y_acc && rel_y < y_acc + item_h {
                                new_hover = opt_i as i32;
                            }
                            y_acc += item_h;
                            opt_i += 1;
                        } else if child.tag == "optgroup" {
                            y_acc += group_h;
                            for gc in &child.children {
                                if gc.tag == "option" {
                                    if rel_y >= y_acc && rel_y < y_acc + item_h {
                                        new_hover = opt_i as i32;
                                    }
                                    y_acc += item_h;
                                    opt_i += 1;
                                }
                            }
                        }
                    }
                    if new_hover != self.dropdown_hover_idx {
                        self.dropdown_hover_idx = new_hover;
                        redraw = true;
                    }
                }
                // Drag: if mouse button held and moved past threshold, fire DragStart/Drag.
                if self.drag_source != 0 {
                    let dx = doc_pt.0 - self.drag_start_doc_pt.0;
                    let dy = doc_pt.1 - self.drag_start_doc_pt.1;
                    if !self.drag_active && (dx * dx + dy * dy) > 25.0 {
                        // DragStart
                        self.drag_active = true;
                        let mut e = HtmlEvent::new(HtmlEventType::DragStart);
                        e.target = self.drag_source;
                        e.doc_pos = self.drag_start_doc_pt;
                        e.client_pos = (
                            self.drag_start_doc_pt.0,
                            self.drag_start_doc_pt.1 - self.scroll_y,
                        );
                        if self.dispatch_input_event(e).0 {
                            redraw = true;
                        }
                    }
                    if self.drag_active {
                        let mut e = HtmlEvent::new(HtmlEventType::Drag);
                        e.target = self.drag_source;
                        e.doc_pos = doc_pt;
                        e.client_pos = client_pos;
                        if self.dispatch_input_event(e).0 {
                            redraw = true;
                        }
                    }
                }
            }
            HtmlEventType::MouseDown | HtmlEventType::PointerDown => {
                if hit_node_id != 0 && self.svg_trigger_event(hit_node_id, etype.as_str()) {
                    redraw = true;
                }
                if self.active_box != hit_node_id {
                    self.active_box = hit_node_id;
                    redraw = true;
                }
                if etype == HtmlEventType::MouseDown {
                    self.mousedown_target = hit_node_id;
                    // Arm drag state machine.
                    self.drag_source = hit_node_id;
                    self.drag_start_doc_pt = doc_pt;
                    self.drag_active = false;
                }
                // Focus change on click.
                // Only interactive (focusable) elements receive focus on click.
                // Clicking a non-focusable element blurs the current focus.
                if etype == HtmlEventType::MouseDown {
                    // Walk up from hit target to find the nearest focusable ancestor
                    let focus_target_id = if hit_node_id != 0 {
                        if let Some(hit) = self.get_node(hit_node_id) {
                            if is_focusable_node(hit) {
                                hit_node_id
                            } else {
                                find_form_parent_id(&self.root, hit_node_id)
                            }
                        } else {
                            0u32
                        }
                    } else {
                        0u32
                    };
                    let click_focusable = focus_target_id != 0
                        && self
                            .get_node(focus_target_id)
                            .map(|fp| is_focusable_node(fp))
                            .unwrap_or(false);
                    let new_focus = if click_focusable {
                        focus_target_id
                    } else {
                        0u32
                    };
                    if self.focused_box != new_focus {
                        let old_focus = self.focused_box;
                        self.keyboard_focus = false;
                        self.focused_box = new_focus;
                        if old_focus != 0 {
                            self.svg_trigger_event(old_focus, "blur");
                            let mut e = HtmlEvent::new(HtmlEventType::Blur);
                            e.target = old_focus;
                            e.related_target = new_focus;
                            self.dispatch_input_event(e);
                            self.svg_trigger_event(old_focus, "focusout");
                            let mut e = HtmlEvent::new(HtmlEventType::FocusOut);
                            e.target = old_focus;
                            e.related_target = new_focus;
                            self.dispatch_input_event(e);
                        }
                        if new_focus != 0 {
                            self.svg_trigger_event(new_focus, "focus");
                            let mut e = HtmlEvent::new(HtmlEventType::Focus);
                            e.target = new_focus;
                            e.related_target = old_focus;
                            self.dispatch_input_event(e);
                            self.svg_trigger_event(new_focus, "focusin");
                            let mut e = HtmlEvent::new(HtmlEventType::FocusIn);
                            e.target = new_focus;
                            e.related_target = old_focus;
                            self.dispatch_input_event(e);
                        }
                        // Always recascade when focus changes so :focus/:focus-visible update.
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
                            false,
                            &std::collections::HashSet::new(),
                            target_id,
                            &self.base_url,
                        );
                        redraw = true;
                    }
                }
                // **Grabbing a slider's knob.** A range is driven from the
                // PRESS, unlike every other control here, because its
                // interaction continues while the pointer moves — see
                // `Document::dragging_range`. The press itself already moves
                // the value: pressing anywhere on the track jumps the knob
                // there, which is what makes the first `input` fire before any
                // movement at all.
                let range_id = find_form_parent_id(&self.root, hit_node_id);
                if self.is_range_input(range_id)
                    && !self
                        .get_node(range_id)
                        .map(|n| n.attributes.contains_key("disabled"))
                        .unwrap_or(true)
                {
                    self.range_drag_origin = self
                        .find_webcore(range_id)
                        .map(input_value)
                        .unwrap_or_default();
                    self.dragging_range = range_id;
                    if self.drag_range_to(range_id, doc_pt) {
                        redraw = true;
                    }
                }
            }
            HtmlEventType::MouseUp | HtmlEventType::PointerUp => {
                if hit_node_id != 0 && self.svg_trigger_event(hit_node_id, etype.as_str()) {
                    redraw = true;
                }
                // Letting go of a knob. FIRST, so the last position the
                // pointer reached is the value that gets committed — and
                // before any of the click routing below, which must not see a
                // range at all.
                if self.dragging_range != 0 {
                    let range_id = self.dragging_range;
                    if self.drag_range_to(range_id, doc_pt) {
                        redraw = true;
                    }
                    self.commit_range_drag();
                }
                if self.active_box != 0 {
                    self.active_box = 0;
                    redraw = true;
                }
                if etype == HtmlEventType::MouseUp {
                    // DragEnd if drag was active; save flag before resetting.
                    let was_dragging = self.drag_active;
                    if was_dragging {
                        let mut e = HtmlEvent::new(HtmlEventType::DragEnd);
                        e.target = self.drag_source;
                        e.doc_pos = doc_pt;
                        e.client_pos = client_pos;
                        if self.dispatch_input_event(e).0 {
                            redraw = true;
                        }
                    }
                    self.drag_source = 0;
                    self.drag_active = false;

                    // ⛔ **An open popup takes the click FIRST.**
                    //
                    // It is drawn over the page and is not in the tree, so the
                    // hit test below finds whatever happens to be UNDER it —
                    // and finds nothing at all where no element lies, which is
                    // most of the page. Gating a popup's click on that dropped
                    // every pick that landed past the end of the content.
                    //
                    // Either outcome closes it: a swatch picks, anywhere else
                    // dismisses, and neither reaches the page beneath.
                    if self.open_picker != 0 {
                        let picker_id = self.open_picker;
                        // What a pick MEANS depends on the control: a swatch is
                        // a colour, a cell is a date. Both write a value in the
                        // format that control's spec requires, and both close.
                        let picked = match self.picker_kind(picker_id) {
                            Some(PickerKind::Calendar) => self
                                .calendar_hit(picker_id, doc_pt)
                                .map(|(y, m, d)| crate::widgets::to_date_value(y, m, d)),
                            _ => self
                                .picker_hit(picker_id, doc_pt)
                                .map(crate::widgets::to_simple_colour),
                        };
                        if let Some(value) = picked {
                            self.set_value(picker_id, &value);
                            let (id, name) = self
                                .find_webcore(picker_id)
                                .map(|n| {
                                    (
                                        n.attributes.get("id").cloned().unwrap_or_default(),
                                        n.attributes.get("name").cloned().unwrap_or_default(),
                                    )
                                })
                                .unwrap_or_default();
                            if let Some(ref mut cb) = self.on_form_event {
                                cb(&FormEvent {
                                    tag: "input".to_string(),
                                    id,
                                    name,
                                    kind: FormEventKind::Change(value),
                                    element: picker_id,
                                });
                            }
                        }
                        self.open_picker = 0;
                        self.mousedown_target = 0;
                        return true;
                    }
                    // Click only if no drag occurred and released on same element as pressed.
                    //
                    // ⚠ An OPEN DROPDOWN relaxes the first half, for the reason
                    // the picker above is handled before this gate at all: the
                    // list is drawn over the page and is not in the tree, so a
                    // row that happens to hang past the end of the content had
                    // nothing under it, `hit_node_id` was 0, and the pick was
                    // dropped. It worked only where an element lay beneath.
                    //
                    // The branch it guards reads the click's Y against the
                    // select's own geometry and never consults `hit_node_id`,
                    // so letting it through costs nothing when the list is up.
                    if (hit_node_id != 0 && hit_node_id == self.mousedown_target
                        || self.open_select != 0)
                        && !was_dragging
                    {
                        let mut click = HtmlEvent::new(HtmlEventType::Click);
                        click.target = hit_node_id;
                        click.doc_pos = doc_pt;
                        click.client_pos = client_pos;
                        click.button = button;
                        if self.dispatch_input_event(click).0 {
                            redraw = true;
                        }

                        // Form element interactions
                        // The second half of the popup rule: an OPEN DROPDOWN
                        // is handled in here, and this gate excluded it for the
                        // same reason the outer one did — a row with no element
                        // beneath it has `hit_node_id == 0`. The form-click call
                        // still needs a real node and keeps its own check.
                        if (hit_node_id != 0 || self.open_select != 0) && button == 0 {
                            let form_click = (hit_node_id != 0)
                                .then(|| {
                                    handle_form_click(
                                        &mut self.root,
                                        hit_node_id,
                                        &mut self.on_form_event,
                                    )
                                })
                                .flatten();
                            // **EVERY element gets a `click`, not just the form
                            // controls.** `handle_form_click` answers `None` for
                            // anything it does not recognise, and that was the
                            // end of the road: a listener on a `<td>`, a `<div>`
                            // or an `<li>` was registered, was reachable, and
                            // never fired — so a composed control could be built
                            // out of ordinary elements and could not be clicked.
                            // UI Events puts `click` on the element the pointer
                            // pressed and released over, whatever it is.
                            if form_click.is_none() && hit_node_id != 0 {
                                self.fire_element_click(hit_node_id);
                            }
                            if let Some(form_redraw) = form_click {
                                if form_redraw {
                                    redraw = true;
                                }
                                // `handle_form_click` takes `&mut WebCore`, so it
                                // wrote `checked` to the render tree only. Push it
                                // into the arena before anything reads the DOM
                                // through the WHATWG accessors — see
                                // `Document::sync_form_state_to_arena`.
                                self.sync_form_state_to_arena();
                                // **A state change is a STYLE change.** The
                                // cascade is cached, so `:checked` (and
                                // `:checked + label`, and every rule keyed off
                                // it) keeps whatever it computed BEFORE the
                                // click until something says otherwise. Ticking
                                // a box changed the state, painted the tick and
                                // left the styling on the previous frame's
                                // answer.
                                self.style_dirty = true;
                            }
                            // **Activating a `<summary>` toggles its `<details>`**
                            // (HTML §4.11.1). The summary already draws a pointer
                            // cursor and a disclosure marker, so the control looked
                            // interactive and did nothing — the cursor promised an
                            // interaction that was never wired.
                            if hit_node_id != 0 && button == 0 {
                                if let Some(details_id) = self.summary_details(hit_node_id) {
                                    let open = self
                                        .find_webcore(details_id)
                                        .map(|n| n.attributes.contains_key("open"))
                                        .unwrap_or(false);
                                    if open {
                                        self.remove_attribute(details_id, "open");
                                    } else {
                                        self.set_attribute(details_id, "open", "");
                                    }
                                    // `details:not([open])` is a SELECTOR, so what
                                    // is shown is a cascade decision — the same
                                    // reason ticking a checkbox marks style dirty.
                                    self.style_dirty = true;
                                    redraw = true;
                                }
                            }

                            // Handle select dropdown
                            if self.open_select != 0 {
                                // Collect options from DOM children
                                let sel = self.get_node(self.open_select).unwrap();
                                let font_px = sel.style.font_size_px(16.0, 16.0);
                                let item_h = font_px * 1.8;
                                let group_h = font_px * 1.5;

                                // Count items (options + optgroups) for height
                                let mut opt_texts: Vec<String> = Vec::new();
                                let mut opt_values: Vec<String> = Vec::new();
                                let mut total_h = 8.0f32; // padding
                                for child in &sel.children {
                                    if child.tag == "option" {
                                        let txt: String = child
                                            .children
                                            .iter()
                                            .filter(|c| c.tag == "#text")
                                            .map(|c| c.text.as_str())
                                            .collect();
                                        let val = child
                                            .attributes
                                            .get("value")
                                            .cloned()
                                            .unwrap_or_else(|| txt.clone());
                                        opt_texts.push(txt.trim().to_string());
                                        opt_values.push(val.trim().to_string());
                                        total_h += item_h;
                                    } else if child.tag == "optgroup" {
                                        total_h += group_h;
                                        for gc in &child.children {
                                            if gc.tag == "option" {
                                                let txt: String = gc
                                                    .children
                                                    .iter()
                                                    .filter(|c| c.tag == "#text")
                                                    .map(|c| c.text.as_str())
                                                    .collect();
                                                let val = gc
                                                    .attributes
                                                    .get("value")
                                                    .cloned()
                                                    .unwrap_or_else(|| txt.clone());
                                                opt_texts.push(txt.trim().to_string());
                                                opt_values.push(val.trim().to_string());
                                                total_h += item_h;
                                            }
                                        }
                                    }
                                }

                                let dropdown_y =
                                    sel.layout.border_rect.y + sel.layout.border_rect.h;
                                let popup_w = sel.layout.border_rect.w.max(150.0);
                                let click_y = doc_pt.1;
                                let click_x = doc_pt.0;

                                if click_y >= dropdown_y
                                    && click_y < dropdown_y + total_h
                                    && click_x >= sel.layout.border_rect.x
                                    && click_x < sel.layout.border_rect.x + popup_w
                                {
                                    // Determine which option was clicked
                                    let rel_y = click_y - dropdown_y - 4.0;
                                    let mut y_acc = 0.0f32;
                                    let mut clicked_opt: Option<usize> = None;
                                    let mut opt_i = 0usize;
                                    for child in &sel.children {
                                        if child.tag == "option" {
                                            if rel_y >= y_acc && rel_y < y_acc + item_h {
                                                clicked_opt = Some(opt_i);
                                                break;
                                            }
                                            y_acc += item_h;
                                            opt_i += 1;
                                        } else if child.tag == "optgroup" {
                                            y_acc += group_h;
                                            for gc in &child.children {
                                                if gc.tag == "option" {
                                                    if rel_y >= y_acc && rel_y < y_acc + item_h {
                                                        clicked_opt = Some(opt_i);
                                                        break;
                                                    }
                                                    y_acc += item_h;
                                                    opt_i += 1;
                                                }
                                            }
                                            if clicked_opt.is_some() {
                                                break;
                                            }
                                        }
                                    }

                                    if let Some(opt_idx) = clicked_opt {
                                        let sel_id = self.open_select;
                                        let new_text =
                                            opt_texts.get(opt_idx).cloned().unwrap_or_default();
                                        // The option's node_id, so the pick runs
                                        // over the spec's own list of options
                                        // rather than this popup's parallel
                                        // walk — the two counted optgroups the
                                        // same way, but only one of them is the
                                        // definition.
                                        let option_id = self
                                            .find_webcore(sel_id)
                                            .map(crate::html::forms::option_ids)
                                            .and_then(|ids| ids.get(opt_idx).copied());
                                        if let (Some(option_id), Some(sel_mut)) =
                                            (option_id, self.find_webcore_mut(sel_id))
                                        {
                                            let changed =
                                                crate::html::forms::pick_option(sel_mut, option_id);
                                            // The drop-down's shown text is a
                                            // child text node rather than a
                                            // repaint of the options.
                                            if let Some(tn) = sel_mut
                                                .children
                                                .iter_mut()
                                                .rev()
                                                .find(|c| c.tag == "#text")
                                            {
                                                tn.text = new_text;
                                            }
                                            sel_mut.layout.layout_dirty = true;
                                            if changed {
                                                // `option:checked` is a selector.
                                                self.style_dirty = true;
                                                self.send_select_update_notifications(sel_id);
                                            }
                                        }
                                    }
                                    self.open_select = 0;
                                    redraw = true;
                                } else {
                                    self.open_select = 0;
                                    redraw = true;
                                }
                            } else {
                                // Check if clicking a select to open it
                                let effective_id = find_form_parent_id(&self.root, hit_node_id);
                                let is_select = self
                                    .get_node(effective_id)
                                    .map(|n| n.tag == "select")
                                    .unwrap_or(false);
                                // ⛔ A LIST BOX HAS NO POPUP. Its rows are drawn
                                // inside the control, so a click picks one
                                // directly — routing it through `open_select`
                                // opened a phantom list over the page and
                                // selected nothing, ever.
                                let list_box = is_select
                                    && self
                                        .get_node(effective_id)
                                        .map(crate::html::forms::is_list_box)
                                        .unwrap_or(false);
                                if list_box {
                                    // ⛔ NO click event here. Every element gets
                                    // one from `fire_element_click` above —
                                    // `handle_form_click` returns `None` for a
                                    // `<select>`, so the generic path already
                                    // covered it. Firing a second would report
                                    // two clicks for one press.
                                    if self.click_list_box_row(
                                        effective_id,
                                        doc_pt.1,
                                        unselect_request,
                                    ) {
                                        // Selectedness is a SELECTOR
                                        // (`option:checked`), so the cascade has
                                        // to be told, exactly as ticking a
                                        // checkbox does.
                                        self.style_dirty = true;
                                        redraw = true;
                                    }
                                } else if is_select {
                                    self.open_select = effective_id;
                                    redraw = true;
                                } else if self.is_range_input(effective_id) {
                                    // ⛔ NOTHING on release. A range is the one
                                    // control driven from the press, because a
                                    // drag is a press, a path and a release —
                                    // handling it here as well would move the
                                    // knob a second time to wherever the
                                    // pointer happened to end.
                                } else if self.picker_kind(effective_id).is_some() {
                                    // Activating the control opens its picker —
                                    // HTML leaves each picker's FORM to the
                                    // user agent and says only that one is
                                    // offered.
                                    self.open_picker = effective_id;
                                    redraw = true;
                                }
                            }
                        }

                        // DblClick: same target within 400 ms.
                        let now = std::time::Instant::now();
                        let is_dbl = self.last_click_target == hit_node_id
                            && self
                                .last_click_time
                                .map(|t| t.elapsed().as_millis() < 400)
                                .unwrap_or(false);
                        if is_dbl {
                            let mut dbl = HtmlEvent::new(HtmlEventType::DblClick);
                            dbl.target = hit_node_id;
                            dbl.doc_pos = doc_pt;
                            dbl.client_pos = client_pos;
                            dbl.button = button;
                            if self.dispatch_input_event(dbl).0 {
                                redraw = true;
                            }
                            // Reset so triple-click doesn't re-trigger.
                            self.last_click_target = 0;
                            self.last_click_time = None;
                        } else {
                            self.last_click_target = hit_node_id;
                            self.last_click_time = Some(now);
                        }
                    }
                    self.mousedown_target = 0;
                    // Track visited links + fire on_navigate callback.
                    if button == 0 {
                        if let Some(href) =
                            crate::layout::hit_test::hit_test_link(&self.root, doc_pt, button)
                        {
                            self.visited_urls.insert(href.clone());
                            if let Some(ref mut cb) = self.on_navigate {
                                cb(&href);
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        let (handled, mut evt) = self.dispatch_input_event(evt);
        if handled {
            redraw = true;
        }

        // Also dispatch through the NodeId-based event system (capture/bubble).
        if evt.target != 0 {
            let mut dom_evt = crate::dom::events::DomEvent::new(etype.as_str(), evt.target);
            dom_evt.client_x = client_pos.0;
            dom_evt.client_y = client_pos.1;
            dom_evt.button = button;
            // The MODIFIERS travel with the event. They were left at their
            // defaults, so `event.ctrlKey` was false in every listener however
            // the user actually clicked — a ctrl-click was indistinguishable
            // from a plain one.
            dom_evt.ctrl_key = evt.ctrl_key;
            dom_evt.shift_key = evt.shift_key;
            dom_evt.alt_key = evt.alt_key;
            dom_evt.meta_key = evt.meta_key;
            dom_evt.related_target = evt.related_target;
            if self.dispatch_dom_event(&mut dom_evt) {
                redraw = true;
            }
            // A listener that cancelled the event must stop the default
            // action, exactly as one registered the older way does.
            if dom_evt.default_prevented() {
                evt.default_prevented = true;
            }
        }

        // Only perform editor/default behavior if not prevented by handlers.
        if !evt.default_prevented {
            if self
                .editor
                .handle_mouse_event(&self.root, etype, doc_pt, button)
            {
                redraw = true;
            }
        }

        // Full cascade + layout only when event handlers or editor logic changed
        // DOM state (class toggles, etc.), not merely for hover/active pointer updates.
        if handled {
            let width = self.root.layout.last_containing_width.max(0.0);
            self.recascade();
            LayoutEngine::new().layout(self, width);
        }

        redraw
    }

    /// Dispatch `MouseOver` on the new hover target and `MouseOut` on the previous one.
    /// Called from the renderer on every `CursorMoved` event.
    /// Returns `true` if listeners were fired (caller should redraw).
    pub fn dispatch_over_out(&mut self, doc_pt: (f32, f32)) -> bool {
        use crate::dom::{HtmlEvent, HtmlEventType};
        let client_pos = (doc_pt.0, doc_pt.1 - self.scroll_y);
        let new_id: u32 = crate::layout::hit_test::point_to_hit(&self.root, doc_pt, 0)
            .map(|h| h.node_id)
            .unwrap_or(0);
        let old_id = self.hovered_box;
        if new_id == old_id {
            return false;
        }

        let mut redraw = false;
        macro_rules! ev {
            ($t:expr_2021, $tgt:expr_2021, $rel:expr_2021, $bubble:expr_2021) => {{
                let mut e = HtmlEvent::new($t);
                e.target = $tgt;
                e.related_target = $rel;
                e.doc_pos = doc_pt;
                e.client_pos = client_pos;
                // `bubbles` is a property of the event TYPE, so the same call
                // serves both: `mouseenter`/`mouseleave` fire on the target and
                // stop, `mouseover`/`mouseout` propagate.
                let _ = $bubble;
                self.dispatch_input_event(e).0
            }};
        }
        if old_id != 0 {
            if ev!(HtmlEventType::MouseOut, old_id, new_id, true) {
                redraw = true;
            }
            if ev!(HtmlEventType::MouseLeave, old_id, new_id, false) {
                redraw = true;
            }
            ev!(HtmlEventType::PointerOut, old_id, new_id, true);
            ev!(HtmlEventType::PointerLeave, old_id, new_id, false);
        }
        if new_id != 0 {
            if ev!(HtmlEventType::MouseOver, new_id, old_id, true) {
                redraw = true;
            }
            if ev!(HtmlEventType::MouseEnter, new_id, old_id, false) {
                redraw = true;
            }
            ev!(HtmlEventType::PointerOver, new_id, old_id, true);
            ev!(HtmlEventType::PointerEnter, new_id, old_id, false);
        }

        // Also dispatch through NodeId-based event system
        if old_id != 0 {
            if self.svg_trigger_event(old_id, "mouseout") {
                redraw = true;
            }
            if self.svg_trigger_event(old_id, "mouseleave") {
                redraw = true;
            }
            if self.svg_trigger_event(old_id, "pointerout") {
                redraw = true;
            }
            if self.svg_trigger_event(old_id, "pointerleave") {
                redraw = true;
            }
            let mut e = crate::dom::events::DomEvent::new("mouseout", old_id);
            e.related_target = new_id;
            e.client_x = client_pos.0;
            e.client_y = client_pos.1;
            self.dispatch_dom_event(&mut e);
        }
        if new_id != 0 {
            if self.svg_trigger_event(new_id, "mouseover") {
                redraw = true;
            }
            if self.svg_trigger_event(new_id, "mouseenter") {
                redraw = true;
            }
            if self.svg_trigger_event(new_id, "pointerover") {
                redraw = true;
            }
            if self.svg_trigger_event(new_id, "pointerenter") {
                redraw = true;
            }
            let mut e = crate::dom::events::DomEvent::new("mouseover", new_id);
            e.related_target = old_id;
            e.client_x = client_pos.0;
            e.client_y = client_pos.1;
            self.dispatch_dom_event(&mut e);
        }
        redraw
    }
}
