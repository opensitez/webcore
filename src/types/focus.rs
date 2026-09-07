//! Focus movement — Tab and Shift+Tab.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

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
        let focusable: Vec<u32> = positive.into_iter().map(|(p, _)| p).chain(normal).collect();
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
                    if i == 0 {
                        focusable.len() - 1
                    } else {
                        i - 1
                    }
                } else {
                    if i + 1 >= focusable.len() {
                        0
                    } else {
                        i + 1
                    }
                }
            }
        };
        let new_focus = focusable[next];
        let old_focus = self.focused_box;
        if new_focus == old_focus {
            return false;
        }

        self.keyboard_focus = true;
        self.focused_box = new_focus;
        if old_focus != 0 {
            let mut e = HtmlEvent::new(HtmlEventType::Blur);
            e.target = old_focus;
            e.related_target = new_focus;
            self.dispatch_input_event(e);
            let mut e = HtmlEvent::new(HtmlEventType::FocusOut);
            e.target = old_focus;
            e.related_target = new_focus;
            self.dispatch_input_event(e);
        }
        if new_focus != 0 {
            let mut e = HtmlEvent::new(HtmlEventType::Focus);
            e.target = new_focus;
            e.related_target = old_focus;
            self.dispatch_input_event(e);
            let mut e = HtmlEvent::new(HtmlEventType::FocusIn);
            e.target = new_focus;
            e.related_target = old_focus;
            self.dispatch_input_event(e);
        }
        self.stylesheet.rebuild_index();
        self.hovered_box = 0;
        self.active_box = 0;
        let target_id = self.fragment_target_id();
        crate::css::apply_cascade_vp_hover_target_url(
            &mut self.root,
            &self.stylesheet,
            None,
            16.0,
            self.viewport_w,
            self.viewport_h,
            self.focused_box,
            true,
            &std::collections::HashSet::new(),
            target_id,
            &self.base_url,
        );
        true
    }
}
