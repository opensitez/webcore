//! Plain-text clipboard default actions shared by browser chrome and page controls.

use super::events::DomEvent;
use crate::{Document, WebCore};
use std::sync::Mutex;

// Keep the native owner alive, particularly for X11 selection ownership.
static SYSTEM_CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_system_clipboard<T>(
    operation: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, arboard::Error> {
    let mut owner = SYSTEM_CLIPBOARD
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if owner.is_none() {
        *owner = Some(arboard::Clipboard::new()?);
    }
    operation(owner.as_mut().expect("initialized clipboard"))
}

impl Document {
    fn clipboard_target(&self) -> u32 {
        if self.focused_box != 0 {
            self.focused_box
        } else {
            self.editor.caret_box.unwrap_or(self.root.node_id)
        }
    }

    /// The selected displayed plain text. Password controls never expose their value.
    pub fn selected_text(&self) -> Option<String> {
        if let Some(node) = self
            .get_node(self.focused_box)
            .filter(|node| crate::types::is_text_input(node))
        {
            if self.is_actually_disabled(node.node_id)
                || self.is_inert(node.node_id)
                || node
                    .attributes
                    .get("type")
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("password"))
            {
                return None;
            }
            let start = node.input_cursor.min(node.input_sel_anchor);
            let end = node.input_cursor.max(node.input_sel_anchor);
            return (start < end).then(|| {
                crate::types::input_value(node)
                    .chars()
                    .skip(start)
                    .take(end - start)
                    .collect()
            });
        }
        let mut text = String::new();
        let mut previous = None;
        for (id, start, end) in self.editor.selection_segments(&self.root) {
            let Some(node) = self.get_node(id) else {
                continue;
            };
            let flat = crate::layout::inline_layout::collect_flat_text(node);
            let Some(part) = flat.get(start..end) else {
                continue;
            };
            if previous.is_some_and(|previous| previous != id) {
                text.push('\n');
            }
            text.push_str(part);
            previous = Some(id);
        }
        (!text.is_empty()).then_some(text)
    }

    fn clipboard_may_edit(&self) -> bool {
        let id = self.clipboard_target();
        if self.is_actually_disabled(id) || self.is_inert(id) {
            return false;
        }
        if let Some(node) = self
            .get_node(id)
            .filter(|node| crate::types::is_text_input(node))
        {
            return !node.attributes.contains_key("readonly");
        }
        self.editor.caret_info().is_some_and(|(id, _)| {
            (!self.editor.read_only || super::is_in_contenteditable_by_id(&self.root, id))
                && (!self.editor.has_selection()
                    || self
                        .editor
                        .selection_segments(&self.root)
                        .iter()
                        .all(|(owner, _, _)| *owner == id))
        })
    }

    fn clipboard_replace(&mut self, text: &str, input_type: &str) -> bool {
        if !self.clipboard_may_edit() {
            return false;
        }
        let target = self.clipboard_target();
        let mut before = DomEvent::new("beforeinput", target);
        before.set_input_type(input_type);
        before.set_data((input_type == "insertFromPaste").then(|| text.to_string()));
        self.dispatch_dom_event(&mut before);
        if before.default_prevented() {
            return false;
        }
        if self.clipboard_target() != target || !self.clipboard_may_edit() {
            return false;
        }
        let native = self
            .get_node(target)
            .is_some_and(crate::types::is_text_input);
        let changed = if native {
            self.find_webcore_mut(target)
                .is_some_and(|node| crate::types::replace_form_input_selection(node, text))
        } else {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let mut editor = std::mem::take(&mut self.editor);
            let mut lines = normalized.split('\n');
            editor.insert_text(&mut self.root, lines.next().unwrap_or_default());
            for line in lines {
                editor.insert_br(&mut self.root);
                editor.insert_text(&mut self.root, line);
            }
            self.editor = editor;
            let mut root = std::mem::replace(&mut self.root, WebCore::new("#placeholder"));
            crate::html::arena_wiring::resync_subtree(&mut self.arena, &mut root);
            self.root = root;
            true
        };
        if !changed {
            return false;
        }
        self.editor.caret_visible = true;
        self.editor.last_blink = std::time::Instant::now();
        let mut input = DomEvent::new("input", target);
        input.set_input_type(input_type);
        input.set_data((input_type == "insertFromPaste").then(|| text.to_string()));
        self.dispatch_dom_event(&mut input);
        if native {
            if let Some(node) = self.get_node(target) {
                let event = crate::types::FormEvent {
                    tag: node.tag.clone(),
                    id: node.attributes.get("id").cloned().unwrap_or_default(),
                    name: node.attributes.get("name").cloned().unwrap_or_default(),
                    kind: crate::types::FormEventKind::Input(crate::types::input_value(node)),
                    element: target,
                };
                if let Some(callback) = &mut self.on_form_event {
                    callback(&event);
                }
            }
        }
        true
    }

    /// Apply one plain-text paste transaction, not one keyboard/layout update per character.
    pub fn paste_text(&mut self, text: &str) -> bool {
        let mut event = DomEvent::new("paste", self.clipboard_target());
        event.set_clipboard_data(ClipboardData::read_only(text));
        self.dispatch_dom_event(&mut event);
        if event.default_prevented() || text.is_empty() {
            return false;
        }
        self.clipboard_replace(text, "insertFromPaste")
    }

    pub(crate) fn copy_text_to(&mut self, cut: bool, mut write: impl FnMut(&str) -> bool) -> bool {
        let mut event = DomEvent::new(if cut { "cut" } else { "copy" }, self.clipboard_target());
        event.set_clipboard_data(ClipboardData::default());
        self.dispatch_dom_event(&mut event);
        let text = if event.default_prevented() {
            event.clipboard_data().and_then(|data| data.text.clone())
        } else {
            self.selected_text()
        };
        let Some(text) = text else {
            return false;
        };
        if !write(&text) {
            return false;
        }
        cut && !event.default_prevented() && self.clipboard_replace("", "deleteByCut")
    }

    /// Default platform clipboard shortcuts. Keydown cancellation is checked by the caller.
    pub(crate) fn clipboard_shortcut(&mut self, key: char) -> Option<bool> {
        match key.to_ascii_lowercase() {
            'c' | 'x' => Some(self.copy_text_to(key.eq_ignore_ascii_case(&'x'), |text| {
                with_system_clipboard(|clipboard| clipboard.set_text(text)).is_ok()
            })),
            'v' => Some(
                with_system_clipboard(|clipboard| clipboard.get_text())
                    .is_ok_and(|text| self.paste_text(&text)),
            ),
            _ => None,
        }
    }
}

/// Plain-text ClipboardEvent data. Paste data is read-only; copy/cut handlers can replace it.
#[derive(Clone, Debug, Default)]
pub struct ClipboardData {
    text: Option<String>,
    read_only: bool,
}

impl ClipboardData {
    fn read_only(text: &str) -> Self {
        Self {
            text: Some(text.to_string()),
            read_only: true,
        }
    }
    pub fn get_data(&self, format: &str) -> String {
        if format.eq_ignore_ascii_case("text/plain") || format.eq_ignore_ascii_case("text") {
            self.text.clone().unwrap_or_default()
        } else {
            String::new()
        }
    }
    pub fn set_data(&mut self, format: &str, text: &str) -> bool {
        if self.read_only
            || !(format.eq_ignore_ascii_case("text/plain") || format.eq_ignore_ascii_case("text"))
        {
            return false;
        }
        self.text = Some(text.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn input(html: &str) -> (Document, u32) {
        let mut doc = crate::parse_html(html);
        let id = doc.get_element_by_id("t").unwrap();
        doc.focused_box = id;
        (doc, id)
    }

    #[test]
    fn clipboard_paste_replaces_unicode_selection_in_one_transaction() {
        let (mut doc, id) = input("<input id=t value='a😀bc'>");
        doc.set_selection_range(id, 1, 4, None);
        let events = Arc::new(Mutex::new(Vec::new()));
        for kind in ["paste", "beforeinput", "input"] {
            let events = events.clone();
            doc.add_event_listener(
                id,
                kind,
                Box::new(move |event, _| {
                    events
                        .lock()
                        .unwrap()
                        .push((kind, event.input_type().to_string()));
                    if kind == "paste" {
                        assert_eq!(
                            event.clipboard_data().unwrap().get_data("text/plain"),
                            "é界"
                        );
                        assert!(
                            !event
                                .clipboard_data_mut()
                                .unwrap()
                                .set_data("text/plain", "changed")
                        );
                    }
                }),
                Default::default(),
            );
        }
        assert!(doc.paste_text("é界"));
        assert_eq!(doc.value(id), "aé界c");
        assert_eq!(doc.selection_start(id), Some(3));
        assert_eq!(doc.selection_end(id), Some(3));
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                ("paste", "".into()),
                ("beforeinput", "insertFromPaste".into()),
                ("input", "insertFromPaste".into())
            ]
        );
        assert_eq!(doc.get_attribute(id, "value").as_deref(), Some("a😀bc"));
    }

    #[test]
    fn clipboard_paste_normalizes_newlines_and_obeys_utf16_maxlength() {
        let (mut doc, id) = input("<textarea id=t maxlength=5>ab</textarea>");
        doc.select(id);
        assert!(doc.paste_text("😀\r\n界Zextra"));
        assert_eq!(doc.value(id), "😀\n界Z");
        assert_eq!(doc.selection_start(id), Some(5));
        let (mut doc, id) = input("<input id=t maxlength=3>");
        assert!(doc.paste_text("a\r\nb😀"));
        assert_eq!(doc.value(id), "ab");
    }

    #[test]
    fn clipboard_copy_readonly_and_protect_disabled_inert_password() {
        for attrs in ["readonly", "disabled", "inert", "type=password"] {
            let (mut doc, id) = input(&format!("<input id=t value=secret {attrs}>"));
            doc.select(id);
            assert_eq!(
                doc.selected_text(),
                (attrs == "readonly").then(|| "secret".to_string())
            );
            if attrs != "type=password" {
                assert!(!doc.paste_text("changed"));
            }
            assert_eq!(doc.value(id), "secret");
        }
    }

    #[test]
    fn clipboard_paste_and_beforeinput_cancellation_preserve_value_and_range() {
        for kind in ["paste", "beforeinput"] {
            let (mut doc, id) = input("<input id=t value=abcd>");
            doc.set_selection_range(id, 1, 3, None);
            doc.add_event_listener(
                id,
                kind,
                Box::new(|event, _| event.prevent_default()),
                Default::default(),
            );
            assert!(!doc.paste_text("replacement"));
            assert_eq!(doc.value(id), "abcd");
            assert_eq!(
                (doc.selection_start(id), doc.selection_end(id)),
                (Some(1), Some(3))
            );
        }
    }

    #[test]
    fn clipboard_cut_writes_before_mutation_and_failed_write_preserves_selection() {
        let (mut doc, id) = input("<input id=t value=abcd>");
        doc.set_selection_range(id, 1, 3, None);
        assert!(!doc.copy_text_to(true, |_| false));
        assert_eq!(doc.value(id), "abcd");
        assert!(doc.copy_text_to(true, |text| {
            assert_eq!(text, "bc");
            true
        }));
        assert_eq!(doc.value(id), "ad");
        assert_eq!(doc.selection_start(id), Some(1));
    }

    #[test]
    fn clipboard_copy_handler_can_override_text_without_cutting() {
        let (mut doc, id) = input("<input id=t value=abcd>");
        doc.select(id);
        doc.add_event_listener(
            id,
            "cut",
            Box::new(|event, _| {
                event
                    .clipboard_data_mut()
                    .unwrap()
                    .set_data("text/plain", "override");
                event.prevent_default();
            }),
            Default::default(),
        );
        let mut copied = String::new();
        assert!(!doc.copy_text_to(true, |text| {
            copied = text.into();
            true
        }));
        assert_eq!(copied, "override");
        assert_eq!(doc.value(id), "abcd");
    }

    #[test]
    fn clipboard_page_copy_uses_visible_intervals_and_block_boundaries() {
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(
            "<p id=a>first<span style='user-select:none'>excluded</span>last</p><p id=b>next</p>",
            400.0,
        );
        let a = doc.get_element_by_id("a").unwrap();
        let b = doc.get_element_by_id("b").unwrap();
        let points = [a, b].map(|id| {
            let line = &doc.get_node(id).unwrap().layout.line_cache[0];
            (line.x, line.y + line.height / 2.0)
        });
        doc.editor.handle_mouse_event(
            &doc.root,
            super::super::HtmlEventType::MouseDown,
            points[0],
            0,
        );
        let line = &doc.get_node(b).unwrap().layout.line_cache[0];
        let end = (line.x + line.width, points[1].1);
        doc.editor
            .handle_mouse_event(&doc.root, super::super::HtmlEventType::MouseMove, end, 0);
        assert_eq!(doc.selected_text().as_deref(), Some("firstlast\nnext"));
        assert!(!doc.paste_text("must not edit page"));
    }

    #[test]
    fn clipboard_keydown_cancellation_blocks_the_default_action() {
        let (mut doc, id) = input("<input id=t value=abcd>");
        doc.select(id);
        doc.add_event_listener(
            id,
            "keydown",
            Box::new(|event, _| event.prevent_default()),
            Default::default(),
        );
        let pastes = Arc::new(Mutex::new(0));
        let count = pastes.clone();
        doc.add_event_listener(
            id,
            "paste",
            Box::new(move |_, _| *count.lock().unwrap() += 1),
            Default::default(),
        );
        doc.process_key_event(
            super::super::HtmlEventType::KeyDown,
            86,
            Some('v'),
            false,
            false,
            false,
            true,
        );
        assert_eq!(*pastes.lock().unwrap(), 0);
        assert_eq!(doc.value(id), "abcd");
    }

    #[test]
    fn clipboard_beforeinput_focus_change_does_not_edit_a_different_control() {
        let (mut doc, id) = input("<input id=t value=first><input id=other value=second>");
        let other = doc.get_element_by_id("other").unwrap();
        doc.select(id);
        doc.add_event_listener(
            id,
            "beforeinput",
            Box::new(move |_, doc| {
                doc.focused_box = other;
            }),
            Default::default(),
        );
        assert!(!doc.paste_text("replacement"));
        assert_eq!(doc.value(id), "first");
        assert_eq!(doc.value(other), "second");
    }

    #[test]
    fn clipboard_contenteditable_paste_batches_text_and_retains_line_breaks() {
        let mut renderer = crate::Renderer::new();
        let mut doc =
            renderer.load_html("<div id=t contenteditable=plaintext-only>abcd</div>", 400.0);
        let id = doc.get_element_by_id("t").unwrap();
        doc.focused_box = id;
        doc.editor.set_caret_from_hit(id, 1, false);
        doc.editor.set_caret_from_hit(id, 3, true);
        assert!(doc.paste_text("XY\nZ"));
        assert_eq!(doc.text_content(id), "aXYZd");
        assert_eq!(doc.query_selector_all("#t br").len(), 1);
        renderer.layout_engine().layout(&mut doc, 400.0);
        assert_eq!(doc.get_node(id).unwrap().layout.line_cache.len(), 2);
    }
}
