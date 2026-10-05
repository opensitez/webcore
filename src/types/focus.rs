//! Focus movement — Tab and Shift+Tab.

use super::{Document, collect_focusable_ordered};
use crate::dom::{HtmlEvent, HtmlEventType};

impl Document {
    /// Move keyboard focus to the next focusable element (Tab key).
    pub fn focus_next(&mut self) -> bool {
        self.shift_tab_focus(false)
    }

    /// Move keyboard focus to the previous focusable element (Shift+Tab).
    pub fn focus_prev(&mut self) -> bool {
        self.shift_tab_focus(true)
    }

    fn shift_tab_focus(&mut self, reverse: bool) -> bool {
        // Build the tab order: elements with explicit tabindex > 0 come first
        // (sorted ascending), then native-focusable and tabindex=0 in document order.
        // Elements with tabindex=-1 are excluded (focusable by script, not keyboard).
        let mut positive: Vec<(u32, i32)> = Vec::new();
        let mut normal: Vec<u32> = Vec::new();
        collect_focusable_ordered(&self.root, &mut positive, &mut normal);
        positive.sort_by_key(|&(_, idx)| idx);
        let focusable: Vec<u32> = positive
            .into_iter()
            .map(|(p, _)| p)
            .chain(normal)
            .filter(|id| !self.is_actually_disabled(*id) && !self.is_inert(*id))
            .collect();
        if focusable.is_empty() {
            return false;
        }

        let current = self.focused_box;
        let pos = focusable.iter().position(|&p| p == current);
        let next = match pos {
            None => {
                if reverse {
                    focusable.len() - 1
                } else {
                    0
                }
            }
            Some(i) => {
                if reverse {
                    if i == 0 { focusable.len() - 1 } else { i - 1 }
                } else {
                    if i + 1 >= focusable.len() { 0 } else { i + 1 }
                }
            }
        };
        let new_focus = focusable[next];
        self.set_focus_target(new_focus, true)
    }

    /// Focus state/events are immediate; style work is coalesced by the owner.
    pub(crate) fn set_focus_target(&mut self, new_focus: u32, keyboard: bool) -> bool {
        let old_focus = self.focused_box;
        let modality_changed = self.keyboard_focus != keyboard;
        self.keyboard_focus = keyboard;
        if old_focus == new_focus {
            self.style_dirty |= modality_changed;
            return modality_changed;
        }
        self.focused_box = 0;
        if self.keyboard_space_target != 0 {
            if self.active_box == self.keyboard_space_target {
                self.active_box = 0;
            }
            self.keyboard_space_target = 0;
        }
        self.style_dirty = true;
        if old_focus != 0 {
            self.svg_trigger_event(old_focus, "blur");
            let mut e = HtmlEvent::new(HtmlEventType::Blur);
            e.target = old_focus;
            e.related_target = new_focus;
            self.dispatch_input_event(e);
            if self.focused_box != 0 {
                return true;
            }
            self.svg_trigger_event(old_focus, "focusout");
            let mut e = HtmlEvent::new(HtmlEventType::FocusOut);
            e.target = old_focus;
            e.related_target = new_focus;
            self.dispatch_input_event(e);
            if self.focused_box != 0 {
                return true;
            }
        }
        if new_focus != 0
            && (self.get_node(new_focus).is_none()
                || self.is_actually_disabled(new_focus)
                || self.is_inert(new_focus))
        {
            return true;
        }
        self.focused_box = new_focus;
        if new_focus != 0 {
            self.svg_trigger_event(new_focus, "focus");
            let mut e = HtmlEvent::new(HtmlEventType::Focus);
            e.target = new_focus;
            e.related_target = old_focus;
            self.dispatch_input_event(e);
            if self.focused_box != new_focus {
                return true;
            }
            self.svg_trigger_event(new_focus, "focusin");
            let mut e = HtmlEvent::new(HtmlEventType::FocusIn);
            e.target = new_focus;
            e.related_target = old_focus;
            self.dispatch_input_event(e);
        }
        true
    }
}
