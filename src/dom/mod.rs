//! DOM manipulation, events API, and editing utilities.
//!
//! This module ports the C++ `wxHtmlEditWidget` DOM/events/editing API to Rust.

/// The HTML namespace — DOM §1.5. An element created without a namespace in an
/// HTML document is in it, which is why `namespace: None` counts as HTML.
///
/// Named here because two rules turn on it: `nodeName` uppercases only for HTML
/// elements, and `createElementNS` with this URI is an ordinary HTML element.
pub const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

pub mod api;
pub mod arena;
pub mod attr_nodes;
pub mod attrs;
pub mod canvas_api;
pub mod computed_style;
pub mod dialog;
pub mod document_meta;
pub mod event_handlers;
pub mod events;
pub mod form_association;
pub mod html_element;
pub mod query;
pub mod range;
pub mod reclaim;
pub mod reflect;
pub mod registry;
pub mod select;
pub mod selection;
pub mod tables;
pub mod token_list;
pub mod token_list_api;
pub mod top_layer;
pub mod traversal;
pub mod url;
pub mod validation_api;
pub mod xml;

// Re-exported at `dom::` so the path matches the other engine's:
// `webcore::dom::new_document` and `vybe_widgets::dom::new_document` are the
// same call under a different browser.
pub use registry::{
    close_document, is_open, new_document, new_xml_document, with_document, DocumentId,
};

use crate::css::apply_property;
use crate::layout::hit_test::point_to_hit;
use crate::types::{
    Color, CssLength, Display, Document, FontStyle, FontWeight, Position, UserSelect, WebCore,
};
use std::time::{Duration, Instant};

const CARET_BLINK_MS: u64 = 500;

// ─── Event types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlEventType {
    // Mouse
    Click,
    DblClick,
    MouseDown,
    MouseUp,
    MouseMove,
    /// Fires when cursor enters element or any descendant (bubbles). Mirror of CSS :hover.
    MouseOver,
    /// Fires when cursor leaves element or any descendant (bubbles).
    MouseOut,
    /// Fires when cursor enters element boundary (does not bubble).
    MouseEnter,
    /// Fires when cursor leaves element boundary (does not bubble).
    MouseLeave,
    ContextMenu,
    // Pointer (unified mouse/touch/stylus — fired alongside mouse events on desktop)
    PointerDown,
    PointerUp,
    PointerMove,
    PointerCancel,
    PointerEnter,
    PointerLeave,
    PointerOver,
    PointerOut,
    // Drag
    DragStart,
    Drag,
    DragEnter,
    DragOver,
    DragLeave,
    Drop,
    DragEnd,
    // Keyboard
    KeyDown,
    KeyUp,
    KeyPress,
    // Wheel / scroll
    /// Raw wheel input from mouse or trackpad (fires before Scroll).
    Wheel,
    Scroll,
    // Content / form / focus
    Input,
    Change,
    /// Fires on the focused element (does not bubble).
    Focus,
    /// Fires on the focused element when it loses focus (does not bubble).
    Blur,
    /// Bubbling version of Focus.
    FocusIn,
    /// Bubbling version of Blur.
    FocusOut,
    SelectionChange,
    // Document / window lifecycle
    /// Fires on the document root after parsing and layout complete.
    DOMContentLoaded,
    /// Fires on the document root when the document is about to be replaced or the window closed.
    Unload,
    /// Fires on the document root when the viewport is resized.
    Resize,
}

impl HtmlEventType {
    /// DOM event type string (e.g. "click", "mousedown", "keydown").
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::DblClick => "dblclick",
            Self::MouseDown => "mousedown",
            Self::MouseUp => "mouseup",
            Self::MouseMove => "mousemove",
            Self::MouseOver => "mouseover",
            Self::MouseOut => "mouseout",
            Self::MouseEnter => "mouseenter",
            Self::MouseLeave => "mouseleave",
            Self::ContextMenu => "contextmenu",
            Self::PointerDown => "pointerdown",
            Self::PointerUp => "pointerup",
            Self::PointerMove => "pointermove",
            Self::PointerCancel => "pointercancel",
            Self::PointerEnter => "pointerenter",
            Self::PointerLeave => "pointerleave",
            Self::PointerOver => "pointerover",
            Self::PointerOut => "pointerout",
            Self::DragStart => "dragstart",
            Self::Drag => "drag",
            Self::DragEnter => "dragenter",
            Self::DragOver => "dragover",
            Self::DragLeave => "dragleave",
            Self::Drop => "drop",
            Self::DragEnd => "dragend",
            Self::KeyDown => "keydown",
            Self::KeyUp => "keyup",
            Self::KeyPress => "keypress",
            Self::Wheel => "wheel",
            Self::Scroll => "scroll",
            Self::Input => "input",
            Self::Change => "change",
            Self::Focus => "focus",
            Self::Blur => "blur",
            Self::FocusIn => "focusin",
            Self::FocusOut => "focusout",
            Self::SelectionChange => "selectionchange",
            Self::DOMContentLoaded => "DOMContentLoaded",
            Self::Unload => "unload",
            Self::Resize => "resize",
        }
    }
}

// ─── HtmlEvent ────────────────────────────────────────────────────────────────

pub struct HtmlEvent {
    pub event_type: HtmlEventType,
    /// Stable node_id of the deepest box hit.
    pub target: u32,
    /// Stable node_id of the current listener's element.
    pub current_target: u32,
    /// Position in window coordinates.
    pub client_pos: (f32, f32),
    /// Position in document coordinates.
    pub doc_pos: (f32, f32),
    /// Mouse button: 0=left, 1=middle, 2=right.
    pub button: u8,
    pub key_code: u32,
    pub char_code: Option<char>,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    /// Wheel scroll delta in logical pixels (positive = scroll down/right).
    pub delta_x: f32,
    pub delta_y: f32,
    /// Source node_id for drag events.
    pub drag_source: u32,
    /// Related target node_id (e.g. focus/blur counterpart).
    pub related_target: u32,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}

impl HtmlEvent {
    pub fn new(event_type: HtmlEventType) -> Self {
        Self {
            event_type,
            target: 0,
            current_target: 0,
            client_pos: (0.0, 0.0),
            doc_pos: (0.0, 0.0),
            button: 0,
            key_code: 0,
            char_code: None,
            ctrl_key: false,
            shift_key: false,
            alt_key: false,
            meta_key: false,
            delta_x: 0.0,
            delta_y: 0.0,
            drag_source: 0,
            related_target: 0,
            default_prevented: false,
            propagation_stopped: false,
        }
    }

    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }
}

// ─── Event listener registry ──────────────────────────────────────────────────

/// Event handler callback. Receives the event and a mutable reference to the
/// DOM root so handlers can query/mutate the tree without unsafe pointer casts.
// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Find a node by node_id in the tree (immutable).
fn find_node_ref(node: &WebCore, id: u32) -> Option<&WebCore> {
    if node.node_id == id {
        return Some(node);
    }
    for child in &node.children {
        if let Some(f) = find_node_ref(child, id) {
            return Some(f);
        }
    }
    None
}

/// Build path of node_ids `[root, ..., parent, target]` from root down to target.
fn collect_id_path(node: &WebCore, target_id: u32, path: &mut Vec<u32>) -> bool {
    path.push(node.node_id);
    if node.node_id == target_id {
        return true;
    }
    for child in &node.children {
        if collect_id_path(child, target_id, path) {
            return true;
        }
    }
    path.pop();
    false
}

fn user_select_allows_selection(root: &WebCore, target_id: u32) -> bool {
    let mut path = Vec::new();
    if !collect_id_path(root, target_id, &mut path) {
        return true;
    }
    path.into_iter()
        .filter_map(|id| find_node_ref(root, id))
        .all(|node| node.style.user_select != UserSelect::None)
}

fn nearest_user_select_all_node(root: &WebCore, target_id: u32) -> Option<&WebCore> {
    let mut path = Vec::new();
    if !collect_id_path(root, target_id, &mut path) {
        return None;
    }
    path.into_iter()
        .rev()
        .filter_map(|id| find_node_ref(root, id))
        .find(|node| node.style.user_select == UserSelect::All)
}

/// Simple CSS selector matching: `tag`, `#id`, `.class`, `*`.
pub fn matches_simple_selector(b: &WebCore, selector: &str) -> bool {
    if selector == "*" {
        return true;
    }
    if let Some(id_sel) = selector.strip_prefix('#') {
        return b.attributes.get("id").map(|s| s == id_sel).unwrap_or(false);
    }
    if let Some(cls_sel) = selector.strip_prefix('.') {
        return b
            .attributes
            .get("class")
            .map(|s| s.split_whitespace().any(|c| c == cls_sel))
            .unwrap_or(false);
    }
    b.tag == selector
}

// ─── DOM class manipulation ───────────────────────────────────────────────────

/// Add a CSS class to a box.
pub fn add_class(b: &mut WebCore, cls: &str) {
    if has_class(b, cls) {
        return;
    }
    let entry = b.attributes.entry_or_default("class");
    if entry.is_empty() {
        *entry = cls.to_string();
    } else {
        entry.push(' ');
        entry.push_str(cls);
    }
}

/// Remove a CSS class from a box.
pub fn remove_class(b: &mut WebCore, cls: &str) {
    if let Some(val) = b.attributes.get_mut("class") {
        let new: Vec<&str> = val.split_whitespace().filter(|&c| c != cls).collect();
        *val = new.join(" ");
    }
}

/// Toggle a CSS class on a box.
pub fn toggle_class(b: &mut WebCore, cls: &str) {
    if has_class(b, cls) {
        remove_class(b, cls);
    } else {
        add_class(b, cls);
    }
}

/// Returns true if the box has the given CSS class.
pub fn has_class(b: &WebCore, cls: &str) -> bool {
    b.attributes
        .get("class")
        .map(|s| s.split_whitespace().any(|c| c == cls))
        .unwrap_or(false)
}

// ─── DOM attribute manipulation ───────────────────────────────────────────────

/// Set an attribute on a box.  Handles `id`, `class`, `style`, `href` specially.
pub fn set_attribute(b: &mut WebCore, attr: &str, value: &str) {
    match attr {
        "id" => {
            b.attributes.insert("id", value);
        }
        "class" => {
            b.attributes.insert("class", value);
        }
        "style" => {
            apply_inline_style_str(b, value);
        }
        _ => {
            b.attributes.insert(attr, value);
        }
    }
}

/// Get an attribute from a box.  Returns `None` if not present.
pub fn get_attribute<'a>(b: &'a WebCore, attr: &str) -> Option<&'a str> {
    match attr {
        "tag" => Some(b.tag.as_str()),
        _ => b.attributes.get(attr).map(|s| s.as_str()),
    }
}

/// Remove an attribute from a box.
pub fn remove_attribute(b: &mut WebCore, attr: &str) {
    match attr {
        "id" => {
            b.attributes.remove("id");
        }
        "class" => {
            b.attributes.remove("class");
        }
        _ => {
            b.attributes.remove(attr);
        }
    }
}

// ─── Custom data ─────────────────────────────────────────────────────────────

pub fn set_data(b: &mut WebCore, key: &str, value: &str) {
    b.data.insert(key.to_string(), value.to_string());
}

pub fn get_data<'a>(b: &'a WebCore, key: &str) -> Option<&'a str> {
    b.data.get(key).map(|s| s.as_str())
}

pub fn has_data(b: &WebCore, key: &str) -> bool {
    b.data.contains_key(key)
}

pub fn remove_data(b: &mut WebCore, key: &str) {
    b.data.remove(key);
}

// ─── Visibility ───────────────────────────────────────────────────────────────

/// Hide a box (sets `display: none`).
pub fn hide(b: &mut WebCore) {
    std::sync::Arc::make_mut(&mut b.style).display = Display::None;
}

/// Show a box (restores block display if hidden).
pub fn show(b: &mut WebCore) {
    if b.style.display == Display::None {
        std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
    }
}

pub fn is_visible(b: &WebCore) -> bool {
    b.style.display != Display::None
}

// ─── Inline style property ───────────────────────────────────────────────────

/// Apply a single CSS property to a box's computed style.
/// Also persists the value in the `style` attribute so that a CSS re-cascade
/// (e.g. after class toggling) does not overwrite the change.
pub fn set_style_property(b: &mut WebCore, prop: &str, value: &str) {
    apply_property(std::sync::Arc::make_mut(&mut b.style), prop, value);
    let style_str = b.attributes.entry_or_default("style");
    upsert_style_attr_prop(style_str, prop, value);
    mark_layout_dirty(b);
}

/// Upsert a single `prop: value` declaration inside an inline style string.
fn upsert_style_attr_prop(style_str: &mut String, prop: &str, value: &str) {
    let prop_lc = prop.trim().to_ascii_lowercase();
    let mut replaced = false;
    let rebuilt: String = crate::css::parser::split_declarations(style_str)
        .into_iter()
        .filter_map(|decl| {
            let t = decl.trim();
            if t.is_empty() {
                return None;
            }
            if let Some(colon) = t.find(':') {
                if t[..colon].trim().to_ascii_lowercase() == prop_lc {
                    replaced = true;
                    return Some(format!("{}: {}", prop, value));
                }
            }
            Some(t.to_string())
        })
        .collect::<Vec<_>>()
        .join("; ");
    if replaced {
        *style_str = rebuilt;
    } else if style_str.is_empty() {
        *style_str = format!("{}: {}", prop, value);
    } else {
        style_str.push_str(&format!("; {}: {}", prop, value));
    }
}

/// Apply a `key: val; key: val` style string to a box.
/// Also persists each property in the `style` attribute so re-cascade is lossless.
pub fn apply_inline_style_str(b: &mut WebCore, css: &str) {
    for decl in crate::css::parser::split_declarations(css) {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let prop = decl[..colon].trim();
            let val = decl[colon + 1..].trim();
            if !prop.is_empty() && !val.is_empty() {
                apply_property(std::sync::Arc::make_mut(&mut b.style), prop, val);
                let style_str = b.attributes.entry_or_default("style");
                upsert_style_attr_prop(style_str, prop, val);
            }
        }
    }
}

// ─── Query selector ───────────────────────────────────────────────────────────

// All four go through `api::matching_ids_from`, the one complete selector
// query. They used to run `matches_simple_selector` per node, which understood
// `#id`, `.class`, `tag` and `*` and answered "no" to everything else — so
// `query_selector_all(root, "table tbody")` returned nothing on a document
// that had one, and the headless browser's `find` command inherited that.

/// Returns the first box matching the selector, searching depth-first.
pub fn query_selector<'a>(root: &'a WebCore, selector: &str) -> Option<&'a WebCore> {
    let id = *crate::dom::query::matching_ids_from(root, selector, true).first()?;
    fn find<'a>(node: &'a WebCore, id: u32) -> Option<&'a WebCore> {
        if node.node_id == id {
            return Some(node);
        }
        node.children.iter().find_map(|c| find(c, id))
    }
    find(root, id)
}

/// Mutable version of `query_selector`.
pub fn query_selector_mut<'a>(root: &'a mut WebCore, selector: &str) -> Option<&'a mut WebCore> {
    let id = *crate::dom::query::matching_ids_from(root, selector, true).first()?;
    find_box_mut(root, id)
}

/// Returns all boxes matching the selector, in document order.
pub fn query_selector_all<'a>(root: &'a WebCore, selector: &str) -> Vec<&'a WebCore> {
    let ids: std::collections::HashSet<u32> =
        crate::dom::query::matching_ids_from(root, selector, false)
            .into_iter()
            .collect();
    let mut out = Vec::new();
    fn collect<'a>(
        node: &'a WebCore,
        ids: &std::collections::HashSet<u32>,
        out: &mut Vec<&'a WebCore>,
    ) {
        if node.node_id != 0 && ids.contains(&node.node_id) {
            out.push(node);
        }
        for child in &node.children {
            collect(child, ids, out);
        }
    }
    collect(root, &ids, &mut out);
    out
}

/// Returns node_ids of all boxes matching the selector.
/// Callers use `find_box_mut(root, id)` to get `&mut` references one at a time.
pub fn query_selector_all_ids(root: &WebCore, selector: &str) -> Vec<u32> {
    crate::dom::query::matching_ids_from(root, selector, false)
}

// ─── Tree traversal ───────────────────────────────────────────────────────────

pub fn get_first_child(b: &WebCore) -> Option<&WebCore> {
    b.children.first()
}

pub fn get_last_child(b: &WebCore) -> Option<&WebCore> {
    b.children.last()
}

// ─── DOM tree mutation ────────────────────────────────────────────────────────

/// Append `child` as the last child of `parent`, in the RENDER tree.
///
/// Order here is the `Vec`'s order, and that is the whole of it: the DOM's
/// answer to `appendChild` comes from `DomArena`, which `Document::append_child`
/// drives. This is the box that gets laid out.
pub fn append_child(parent: &mut WebCore, child: WebCore) {
    parent.children.push(child);
    mark_layout_dirty(parent);
}

/// Insert `new_node` after the child with `reference_id`, in the RENDER tree.
///
/// Position comes from the `Vec` — there is no other order here. The DOM's
/// `insertBefore`/`after` are `Document`'s, and go through `DomArena`.
pub fn insert_after(parent: &mut WebCore, reference_id: u32, new_node: WebCore) -> bool {
    match parent
        .children
        .iter()
        .position(|c| c.node_id == reference_id)
    {
        Some(idx) => {
            parent.children.insert(idx + 1, new_node);
            true
        }
        None => false,
    }
}

/// Remove the child identified by node_id from `parent`, returning it.
pub fn remove_child(parent: &mut WebCore, node_id: u32) -> Option<WebCore> {
    let idx = parent.children.iter().position(|c| c.node_id == node_id)?;
    Some(parent.children.remove(idx))
}
/// Deep-clone an element (WebCore implements Clone).
pub fn clone_element(b: &WebCore) -> WebCore {
    b.clone()
}

/// Create a new element with the given tag name.
pub fn create_element(tag: &str) -> WebCore {
    WebCore::new(tag)
}

// ─── Dirty flag helpers ───────────────────────────────────────────────────────

/// Mark a node as needing re-layout. Call after any mutation that changes
/// geometry (text content, style, children added/removed).
pub fn mark_layout_dirty(node: &mut WebCore) {
    node.layout.layout_dirty = true;
    node.layout.line_cache.clear();
}

/// Mark a node as dirty AND propagate `has_dirty_descendant` up from a child
/// to the root. Call on the root after marking a descendant dirty.
pub fn propagate_dirty_to_root(root: &mut WebCore, target_id: u32) -> bool {
    if root.node_id == target_id {
        root.layout.layout_dirty = true;
        return true;
    }
    for child in &mut root.children {
        if propagate_dirty_to_root(child, target_id) {
            root.has_dirty_descendant = true;
            return true;
        }
    }
    false
}

// ─── Text content ─────────────────────────────────────────────────────────────

/// Get the concatenated text content of a box and all descendants.
pub fn get_text_content(b: &WebCore) -> String {
    b.text_content()
}

/// Replace all children with a single `#text` child containing the given text.
pub fn set_text_content(b: &mut WebCore, text: &str) {
    // If there's already exactly one #text child, just update its text in place.
    if b.children.len() == 1 && b.children[0].tag == "#text" {
        b.children[0].text = text.to_string();
        b.children[0].layout.line_cache.clear();
        b.layout.line_cache.clear();
        mark_layout_dirty(b);
        return;
    }
    b.children.clear();
    b.layout.inline_runs.clear();
    b.layout.line_cache.clear();
    let mut tn = WebCore::new("#text");
    tn.text = text.to_string();
    // Inherit style from parent so text rendering picks up font/color.
    tn.style = b.style.clone();
    std::sync::Arc::make_mut(&mut tn.style).display = Display::Inline; // #text nodes must remain inline
    std::sync::Arc::make_mut(&mut tn.style).hover_style = None;
    std::sync::Arc::make_mut(&mut tn.style).active_style = None;
    std::sync::Arc::make_mut(&mut tn.style).visited_style = None;
    b.children.push(tn);
    mark_layout_dirty(b);
}

// ─── Editing: toggle formatting on selection ──────────────────────────────────

/// Describes a text range within a single `WebCore` (local byte offsets).
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

/// Toggle `font-weight: bold` on the inline runs that overlap `range`.
pub fn toggle_bold(b: &mut WebCore, range: &TextRange) {
    let was_bold = b
        .layout
        .inline_runs
        .iter()
        .filter(|r| r.text_offset < range.end && r.text_offset + r.length > range.start)
        .all(|r| r.style.font_weight.is_bold());

    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.font_weight = if was_bold {
                FontWeight::Normal
            } else {
                FontWeight::Bold
            };
        }
    }
}

/// Toggle `font-style: italic` on the inline runs that overlap `range`.
pub fn toggle_italic(b: &mut WebCore, range: &TextRange) {
    let was_italic = b
        .layout
        .inline_runs
        .iter()
        .filter(|r| r.text_offset < range.end && r.text_offset + r.length > range.start)
        .all(|r| r.style.font_style == FontStyle::Italic);

    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.font_style = if was_italic {
                FontStyle::Normal
            } else {
                FontStyle::Italic
            };
        }
    }
}

/// Toggle `text-decoration: underline` on overlapping runs.
pub fn toggle_underline(b: &mut WebCore, range: &TextRange) {
    let was_underline = b
        .layout
        .inline_runs
        .iter()
        .filter(|r| r.text_offset < range.end && r.text_offset + r.length > range.start)
        .all(|r| r.style.text_decoration.underline);

    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.text_decoration.underline = !was_underline;
        }
    }
}

/// Toggle `text-decoration: line-through` on overlapping runs.
pub fn toggle_strikethrough(b: &mut WebCore, range: &TextRange) {
    let was = b
        .layout
        .inline_runs
        .iter()
        .filter(|r| r.text_offset < range.end && r.text_offset + r.length > range.start)
        .all(|r| r.style.text_decoration.strikethrough);

    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.text_decoration.strikethrough = !was;
        }
    }
}

/// Set font size (in px) on overlapping runs.
pub fn set_font_size(b: &mut WebCore, range: &TextRange, size_px: f32) {
    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.font_size = CssLength::Px(size_px);
        }
    }
}

/// Set font family on overlapping runs.
pub fn set_font_family(b: &mut WebCore, range: &TextRange, family: &str) {
    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.font_family = family.to_string();
        }
    }
}

/// Set text color on overlapping runs.
pub fn set_text_color(b: &mut WebCore, range: &TextRange, color: Color) {
    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.color = color;
        }
    }
}

/// Set background color on overlapping runs.
pub fn set_bg_color(b: &mut WebCore, range: &TextRange, color: Color) {
    for run in &mut b.layout.inline_runs {
        if run.text_offset < range.end && run.text_offset + run.length > range.start {
            run.style.background_color = color;
        }
    }
}

// ─── Undo/Redo ────────────────────────────────────────────────────────────────

/// A snapshot for undo/redo (clones the whole document tree).
pub struct UndoEntry {
    /// Serialized snapshot (caret/selection positions baked in as metadata).
    pub doc: Document,
    pub caret_pos: usize,
    pub sel_start: usize,
    pub sel_end: usize,
}

/// Simple undo/redo stack (max 500 entries).
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Snapshot the current document state before a mutation.
    pub fn push(&mut self, doc: Document, caret_pos: usize, sel_start: usize, sel_end: usize) {
        self.undo.push(UndoEntry {
            doc,
            caret_pos,
            sel_start,
            sel_end,
        });
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undo: moves current doc → redo, returns previous entry.
    pub fn undo(
        &mut self,
        current_doc: Document,
        current_caret: usize,
        current_sel_start: usize,
        current_sel_end: usize,
    ) -> Option<UndoEntry> {
        let entry = self.undo.pop()?;
        self.redo.push(UndoEntry {
            doc: current_doc,
            caret_pos: current_caret,
            sel_start: current_sel_start,
            sel_end: current_sel_end,
        });
        Some(entry)
    }

    /// Redo: moves current doc → undo, returns next entry.
    pub fn redo(
        &mut self,
        current_doc: Document,
        current_caret: usize,
        current_sel_start: usize,
        current_sel_end: usize,
    ) -> Option<UndoEntry> {
        let entry = self.redo.pop()?;
        self.undo.push(UndoEntry {
            doc: current_doc,
            caret_pos: current_caret,
            sel_start: current_sel_start,
            sel_end: current_sel_end,
        });
        Some(entry)
    }
}

// ─── Editor ──────────────────────────────────────────────────────────────────

/// High-level editor state and operations.
#[derive(Debug, Clone)]
pub struct Editor {
    pub caret_box: Option<u32>,
    pub caret_local: usize,
    pub sel_anchor: usize,
    pub sel_start: usize,
    pub sel_end: usize,
    pub caret_visible: bool,
    pub last_blink: Instant,
    pub mouse_down: bool,
    pub has_focus: bool,
    pub read_only: bool,
    /// Set to `true` immediately after `insert_br` so that:
    /// (a) rendering prefers the start of the next line over the end of
    ///     the previous one when the caret sits at an exact line boundary,
    /// (b) `insert_char` skips past the pre-`<br>` text node and inserts
    ///     into the text node that follows the `<br>`.
    /// Cleared by any other caret movement or character insertion.
    pub caret_at_line_start: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            caret_box: None,
            caret_local: 0,
            sel_anchor: 0,
            sel_start: 0,
            sel_end: 0,
            caret_visible: true,
            last_blink: Instant::now(),
            mouse_down: false,
            has_focus: false,
            read_only: false,
            caret_at_line_start: false,
        }
    }
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_selection(&self) -> bool {
        self.sel_start < self.sel_end
    }

    pub fn caret_info(&self) -> Option<(u32, usize)> {
        self.caret_box.map(|id| (id, self.caret_local))
    }

    pub fn sel_args(&self) -> (Option<usize>, Option<usize>) {
        if self.has_selection() {
            (Some(self.sel_start), Some(self.sel_end))
        } else {
            (None, None)
        }
    }

    pub fn set_caret_from_hit(&mut self, node_id: u32, local: usize, extend: bool) {
        if !extend {
            self.sel_anchor = local;
            self.sel_start = local;
            self.sel_end = local;
        }
        self.caret_box = Some(node_id);
        self.caret_local = local;
        if extend && self.caret_box == Some(node_id) {
            self.sel_start = self.sel_anchor.min(local);
            self.sel_end = self.sel_anchor.max(local);
        }
        self.caret_visible = true;
        self.last_blink = Instant::now();
    }

    pub fn move_left(&mut self, flat: &str, extend: bool) {
        let pos = self.caret_local;
        if !extend && self.has_selection() {
            let new = self.sel_start;
            self.collapse_to(new);
            return;
        }
        let new = prev_char_boundary(flat, pos);
        self.move_to(new, extend);
    }

    pub fn move_right(&mut self, flat: &str, extend: bool) {
        let pos = self.caret_local;
        if !extend && self.has_selection() {
            let new = self.sel_end;
            self.collapse_to(new);
            return;
        }
        let new = next_char_boundary(flat, pos);
        self.move_to(new, extend);
    }

    pub fn move_to(&mut self, new_pos: usize, extend: bool) {
        self.caret_local = new_pos;
        if !extend {
            self.sel_anchor = new_pos;
            self.sel_start = new_pos;
            self.sel_end = new_pos;
        } else {
            self.sel_start = self.sel_anchor.min(new_pos);
            self.sel_end = self.sel_anchor.max(new_pos);
        }
        self.caret_visible = true;
        self.last_blink = Instant::now();
    }

    pub fn collapse_to(&mut self, pos: usize) {
        self.caret_local = pos;
        self.sel_anchor = pos;
        self.sel_start = pos;
        self.sel_end = pos;
        self.caret_visible = true;
        self.last_blink = Instant::now();
        self.caret_at_line_start = false;
    }

    fn select_entire_node(&mut self, node: &WebCore) {
        let flat = crate::layout::inline_layout::collect_flat_text(node);
        self.caret_box = Some(node.node_id);
        self.caret_local = flat.len();
        self.sel_anchor = 0;
        self.sel_start = 0;
        self.sel_end = flat.len();
        self.caret_visible = true;
        self.last_blink = Instant::now();
        self.caret_at_line_start = false;
    }

    pub fn blink_update(&mut self) -> bool {
        if self.last_blink.elapsed() >= Duration::from_millis(CARET_BLINK_MS) {
            self.caret_visible = !self.caret_visible;
            self.last_blink = Instant::now();
            true
        } else {
            false
        }
    }

    pub fn next_blink_deadline(&self) -> Instant {
        self.last_blink + Duration::from_millis(CARET_BLINK_MS)
    }

    /// Primary mouse event handler for selection/caret.
    pub fn handle_mouse_event(
        &mut self,
        root: &WebCore,
        etype: HtmlEventType,
        doc_pt: (f32, f32),
        button: u8,
    ) -> bool {
        match etype {
            HtmlEventType::MouseDown => {
                self.mouse_down = true;
                self.has_focus = true;
                if let Some(hit) = point_to_hit(root, doc_pt, button) {
                    if !user_select_allows_selection(root, hit.node_id) {
                        self.mouse_down = false;
                        return false;
                    }
                    if let Some(all_node) = nearest_user_select_all_node(root, hit.node_id) {
                        self.select_entire_node(all_node);
                        return true;
                    }
                    self.set_caret_from_hit(hit.node_id, hit.local_offset, false);
                    return true;
                }
            }
            HtmlEventType::MouseMove => {
                if self.mouse_down {
                    if let Some(hit) = point_to_hit(root, doc_pt, button) {
                        if !user_select_allows_selection(root, hit.node_id) {
                            return false;
                        }
                        if let Some(all_node) = nearest_user_select_all_node(root, hit.node_id) {
                            self.select_entire_node(all_node);
                            return true;
                        }
                        if self.caret_box == Some(hit.node_id) {
                            self.caret_local = hit.local_offset;
                            self.sel_start = self.sel_anchor.min(hit.local_offset);
                            self.sel_end = self.sel_anchor.max(hit.local_offset);
                            self.caret_visible = true;
                            return true;
                        }
                    }
                }
            }
            HtmlEventType::MouseUp => {
                self.mouse_down = false;
            }
            _ => {}
        }
        false
    }

    /// Primary keyboard event handler. Returns true if redraw needed.
    pub fn handle_key_event(
        &mut self,
        root: &mut WebCore,
        etype: HtmlEventType,
        key_code: u32,
        ch: Option<char>,
        _ctrl: bool,
    ) -> bool {
        if self.read_only {
            // Allow editing inside contenteditable="true" elements even when the document is read-only.
            // The caret_box may be a child text node, so we walk the tree from root to check ancestry.
            let is_editable = self
                .caret_box
                .map(|id| is_in_contenteditable_by_id(root, id))
                .unwrap_or(false);
            if !is_editable {
                return false;
            }
        }
        if etype != HtmlEventType::KeyDown && etype != HtmlEventType::KeyPress {
            return false;
        }

        let caret_id = match self.caret_box {
            Some(id) => id,
            None => return false,
        };

        match key_code {
            37 => {
                // ArrowLeft
                if let Some(b) = find_box_mut(root, caret_id) {
                    let flat = crate::layout::inline_layout::collect_flat_text(b);
                    self.move_left(&flat, false);
                    return true;
                }
                return false;
            }
            39 => {
                // ArrowRight
                if let Some(b) = find_box_mut(root, caret_id) {
                    let flat = crate::layout::inline_layout::collect_flat_text(b);
                    self.move_right(&flat, false);
                    return true;
                }
                return false;
            }
            8 => {
                // Backspace
                self.delete_selection_or_before(root);
                return true;
            }
            46 => {
                // Delete
                self.delete_selection_or_at(root);
                return true;
            }
            13 => {
                // Enter — split the current block into two siblings
                self.insert_newline(root);
                return true;
            }
            _ => {
                if let Some(c) = ch {
                    if !c.is_control() {
                        self.insert_char(root, c);
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn insert_char(&mut self, root: &mut WebCore, ch: char) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        // Consume the line-start flag before any mutation (collapse_to will clear it too).
        let at_line_start = self.caret_at_line_start;
        if let Some(container) = find_box_mut(root, caret_nid) {
            if self.has_selection() {
                let s = self.sel_start;
                let e = self.sel_end;
                delete_range_full(container, s, e);
                self.collapse_to(s);
            }
            // When the caret was placed at the start of a new visual line (right after
            // a <br>), use strict mode: text nodes at their exact end are skipped so
            // that the character lands in the node *after* the <br>, not before it.
            let result = if at_line_start {
                find_node_offset_after_br(container, self.caret_local)
            } else {
                find_node_offset_mut(container, self.caret_local)
            };
            match result {
                Ok((leaf, local)) => {
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    leaf.text.insert_str(local, s);
                    self.caret_local += s.len();
                    self.collapse_to(self.caret_local);
                }
                Err(_) => {
                    let ins = self.caret_local.min(container.text.len());
                    container.text.insert(ins, ch);
                    self.caret_local += ch.len_utf8();
                    self.collapse_to(self.caret_local);
                }
            }
            container.layout.layout_dirty = true;
        }
    }

    pub fn delete_selection_or_before(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        if let Some(node) = find_box_mut(root, caret_nid) {
            if self.has_selection() {
                let s = self.sel_start;
                let e = self.sel_end;
                delete_range_full(node, s, e);
                self.collapse_to(s);
                node.layout.layout_dirty = true;
            } else if self.caret_local > 0 {
                let flat = crate::layout::inline_layout::collect_flat_text(node);
                let new_off = prev_char_boundary(&flat, self.caret_local);
                delete_range_full(node, new_off, self.caret_local);
                self.collapse_to(new_off);
                node.layout.layout_dirty = true;
            }
        }
    }

    pub fn delete_selection_or_at(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        if let Some(node) = find_box_mut(root, caret_nid) {
            if self.has_selection() {
                let s = self.sel_start;
                let e = self.sel_end;
                delete_range_full(node, s, e);
                self.collapse_to(s);
                node.layout.layout_dirty = true;
            } else {
                let flat = crate::layout::inline_layout::collect_flat_text(node);
                if self.caret_local < flat.len() {
                    let next_off = next_char_boundary(&flat, self.caret_local);
                    delete_range_full(node, self.caret_local, next_off);
                    self.collapse_to(self.caret_local);
                    node.layout.layout_dirty = true;
                }
            }
        }
    }

    /// Split the current block element at the caret position, creating a new sibling block.
    /// Uses the DOM API (create_element, insert_after, set_text_content) instead of
    /// direct Vec manipulation.
    pub fn insert_newline(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };

        // Delete selection first.
        if self.has_selection() {
            let (s, e) = (self.sel_start, self.sel_end);
            if let Some(b) = find_box_mut(root, caret_nid) {
                delete_range_full(b, s, e);
            }
            self.collapse_to(s);
        }

        // Peek at the tag.
        let block_tag = match find_box_mut(root, caret_nid) {
            Some(b) => b.tag.clone(),
            None => return,
        };

        // Non-prose containers get a <br> instead.
        if !is_prose_tag(&block_tag) {
            self.insert_br(root);
            return;
        }

        let split_at = self.caret_local;

        // Get text after the split point, then remove it from the current block.
        let (tag, after_text) = match find_box_mut(root, caret_nid) {
            Some(b) => {
                let flat = crate::layout::inline_layout::collect_flat_text(b);
                let split = split_at.min(flat.len());
                let after = flat[split..].to_string();
                let flen = flat.len();
                delete_range_full(b, split, flen);
                mark_layout_dirty(b);
                (b.tag.clone(), after)
            }
            None => return,
        };

        // Create new sibling block via DOM API, inheriting style from the original.
        let new_tag = if is_block_tag(&tag) {
            tag.as_str()
        } else {
            "p"
        };
        let mut new_block = create_element(new_tag);
        // Copy the computed style from the source block so the new block inherits
        // font, color, etc. without needing a full cascade.
        if let Some(src) = find_box_mut(root, caret_nid) {
            new_block.style = src.style.clone();
            std::sync::Arc::make_mut(&mut new_block.style).hover_style = None;
            std::sync::Arc::make_mut(&mut new_block.style).active_style = None;
            std::sync::Arc::make_mut(&mut new_block.style).visited_style = None;
        }
        mark_layout_dirty(&mut new_block);

        // Add text content to the new block.
        if !after_text.is_empty() {
            set_text_content(&mut new_block, &after_text);
        }
        let new_block_id = new_block.node_id;

        // Insert after the current block using the parent.
        if let Some(parent) = find_parent_mut_by_id(root, caret_nid) {
            insert_after(parent, caret_nid, new_block);
            mark_layout_dirty(parent);
        }

        // Move caret to start of new block.
        self.caret_box = Some(new_block_id);
        self.collapse_to(0);
    }

    /// Insert a `<br>` at the caret position (soft line break within the current block).
    /// Insert a `<br>` at the caret position. Uses DOM API.
    pub fn insert_br(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };

        if self.has_selection() {
            let (s, e) = (self.sel_start, self.sel_end);
            if let Some(b) = find_box_mut(root, caret_nid) {
                delete_range_full(b, s, e);
            }
            self.collapse_to(s);
        }

        let caret = self.caret_local;

        // Find the leaf text node and split at the caret position.
        let leaf_nid;
        let local_off;
        {
            let container = match find_box_mut(root, caret_nid) {
                Some(c) => c,
                None => return,
            };
            match find_node_offset_mut(container, caret) {
                Ok((leaf, local)) => {
                    leaf_nid = leaf.node_id;
                    local_off = local;
                }
                Err(_) => {
                    // Caret is past end — append <br> via DOM API
                    let br = create_element("br");
                    append_child(container, br);
                    mark_layout_dirty(container);
                    self.collapse_to(caret);
                    return;
                }
            }
        }

        // Split the text node at the caret and insert <br> between the halves.
        split_node_with_br(root, leaf_nid, local_off);
        if let Some(b) = find_box_mut(root, caret_nid) {
            mark_layout_dirty(b);
        }
        self.collapse_to(caret);
        self.caret_at_line_start = true;
    }

    /// Toggle the current block between a plain block (`<p>`) and a list item (`<ul><li>`).
    pub fn toggle_bullet_list(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };

        let is_li = find_box_mut(root, caret_nid)
            .map(|b| b.tag == "li")
            .unwrap_or(false);

        if is_li {
            // Unwrap: convert <li> back to <p>, remove <ul> wrapper if now empty.
            if let Some(ul) = find_parent_mut_by_id(root, caret_nid) {
                if ul.tag == "ul" || ul.tag == "ol" {
                    let ul_nid = ul.node_id;
                    if let Some(li_idx) = ul.children.iter().position(|c| c.node_id == caret_nid) {
                        ul.children[li_idx].tag = "p".to_string();
                        apply_property(
                            std::sync::Arc::make_mut(&mut ul.children[li_idx].style),
                            "display",
                            "block",
                        );
                        self.caret_box = Some(ul.children[li_idx].node_id);

                        // If the list now has only this one element, unwrap the <ul>
                        if ul.children.len() == 1 {
                            if let Some(gp) = find_parent_mut_by_id(root, ul_nid) {
                                if let Some(ul_idx) =
                                    gp.children.iter().position(|c| c.node_id == ul_nid)
                                {
                                    let mut ul_box = gp.children.remove(ul_idx);
                                    let p_box = ul_box.children.remove(0);
                                    gp.children.insert(ul_idx, p_box);

                                    self.caret_box = Some(gp.children[ul_idx].node_id);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Wrap block in <ul><li>.
            if let Some(parent) = find_parent_mut_by_id(root, caret_nid) {
                if let Some(idx) = parent.children.iter().position(|c| c.node_id == caret_nid) {
                    let block = parent.children.remove(idx);
                    let mut li = WebCore::new("li");
                    apply_property(
                        std::sync::Arc::make_mut(&mut li.style),
                        "display",
                        "list-item",
                    );
                    li.text = block.text;
                    li.children = block.children;
                    li.layout.inline_runs = block.layout.inline_runs;
                    let mut ul = WebCore::new("ul");
                    apply_property(std::sync::Arc::make_mut(&mut ul.style), "display", "block");
                    ul.children.push(li);
                    parent.children.insert(idx, ul);

                    self.caret_box = Some(parent.children[idx].children[0].node_id);
                }
            }
        }
    }

    /// Increase the left indent of the current block by `step_px` pixels.
    pub fn increase_indent(&mut self, root: &mut WebCore, step_px: f32) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        if let Some(b) = find_box_mut(root, caret_nid) {
            let current = match b.style.margin_left {
                CssLength::Px(v) => v,
                _ => 0.0,
            };
            let new_val = format!("{}px", current + step_px);
            apply_property(
                std::sync::Arc::make_mut(&mut b.style),
                "margin-left",
                &new_val,
            );
            let style_str = b.attributes.entry_or_default("style");
            upsert_style_attr_prop(style_str, "margin-left", &new_val);
        }
    }

    /// Decrease the left indent of the current block by `step_px` pixels (minimum 0).
    pub fn decrease_indent(&mut self, root: &mut WebCore, step_px: f32) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        if let Some(b) = find_box_mut(root, caret_nid) {
            let current = match b.style.margin_left {
                CssLength::Px(v) => v,
                _ => 0.0,
            };
            let new_val = format!("{}px", (current - step_px).max(0.0));
            apply_property(
                std::sync::Arc::make_mut(&mut b.style),
                "margin-left",
                &new_val,
            );
            let style_str = b.attributes.entry_or_default("style");
            upsert_style_attr_prop(style_str, "margin-left", &new_val);
        }
    }

    /// Wrap the current block in a `<blockquote>`.
    pub fn increase_quote_level(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };
        if let Some(parent) = find_parent_mut_by_id(root, caret_nid) {
            if let Some(idx) = parent.children.iter().position(|c| c.node_id == caret_nid) {
                let block = parent.children.remove(idx);
                let mut bq = WebCore::new("blockquote");
                apply_property(std::sync::Arc::make_mut(&mut bq.style), "display", "block");
                apply_property(
                    std::sync::Arc::make_mut(&mut bq.style),
                    "margin-left",
                    "40px",
                );
                bq.children.push(block);
                parent.children.insert(idx, bq);

                self.caret_box = Some(parent.children[idx].children[0].node_id);
            }
        }
    }

    /// Unwrap one level of `<blockquote>` around the current block.
    pub fn decrease_quote_level(&mut self, root: &mut WebCore) {
        let caret_nid = match self.caret_box {
            Some(id) => id,
            None => return,
        };

        // Confirm the immediate parent is a <blockquote>.
        let bq_nid = match find_parent_mut_by_id(root, caret_nid) {
            Some(p) if p.tag == "blockquote" => p.node_id,
            _ => return,
        };

        if let Some(gp) = find_parent_mut_by_id(root, bq_nid) {
            if let Some(bq_idx) = gp.children.iter().position(|c| c.node_id == bq_nid) {
                let bq_box = gp.children.remove(bq_idx);
                let n = bq_box.children.len();
                for (i, child) in bq_box.children.into_iter().enumerate() {
                    gp.children.insert(bq_idx + i, child);
                }
                if n > 0 {
                    // Point to the first extracted child (closest to original caret position).

                    self.caret_box = Some(gp.children[bq_idx].node_id);
                }
            }
        }
    }
}

// ─── Internal Editor helpers ──────────────────────────────────────────────────

fn prev_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    idx -= 1;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx + 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Returns true if `tag` is a block-level element that the editor treats as a paragraph unit.
fn is_block_tag(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "blockquote"
            | "pre"
            | "address"
            | "article"
            | "aside"
            | "section"
            | "header"
            | "footer"
            | "main"
            | "figure"
            | "figcaption"
            | "dt"
            | "dd"
    )
}

/// Tags where Enter creates a new sibling of the same type (paragraph splitting).
/// Structural containers — `div`, table cells, flex/grid children, `blockquote`,
/// sectioning elements, etc. — are intentionally excluded: Enter inserts `<br>` there.
fn is_prose_tag(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "dt" | "dd" | "pre"
    )
}

pub fn find_box_mut<'a>(root: &'a mut WebCore, node_id: u32) -> Option<&'a mut WebCore> {
    if root.node_id == node_id {
        return Some(root);
    }
    for child in &mut root.children {
        if let Some(b) = find_box_mut(child, node_id) {
            return Some(b);
        }
    }
    None
}

/// Find the direct parent of a node by node_id.
pub fn find_parent_mut_by_id<'a>(root: &'a mut WebCore, child_id: u32) -> Option<&'a mut WebCore> {
    if root.children.iter().any(|c| c.node_id == child_id) {
        return Some(root);
    }
    for child in &mut root.children {
        if let Some(p) = find_parent_mut_by_id(child, child_id) {
            return Some(p);
        }
    }
    None
}

/// Resolves a global offset (from collect_flat_text) to a specific leaf node and local offset.
fn find_node_offset_mut(
    node: &mut WebCore,
    mut offset: usize,
) -> Result<(&mut WebCore, usize), usize> {
    if node.is_text_node() {
        if offset <= node.text.len() {
            return Ok((node, offset));
        } else {
            return Err(offset - node.text.len());
        }
    }
    if matches!(node.style.display, Display::None) {
        return Err(offset);
    }
    if node.tag == "br" {
        return Err(offset);
    }

    if !node.text.is_empty() {
        if offset <= node.text.len() {
            return Ok((node, offset));
        } else {
            offset -= node.text.len();
        }
    }

    for child in &mut node.children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        match find_node_offset_mut(child, offset) {
            Ok(res) => return Ok(res),
            Err(rem) => offset = rem,
        }
    }
    Err(offset)
}

/// `contenteditable="true"`.  Used to allow key events inside contenteditable
/// elements when the document-level editor is otherwise read-only.
pub fn is_in_contenteditable_by_id(node: &WebCore, target_id: u32) -> bool {
    if node.node_id == target_id {
        return node
            .attributes
            .get("contenteditable")
            .map(|v| v.is_empty() || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    }
    let editable_root = node
        .attributes
        .get("contenteditable")
        .map(|v| v.is_empty() || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if editable_root && node_contains_id(node, target_id) {
        return true;
    }
    for child in &node.children {
        if is_in_contenteditable_by_id(child, target_id) {
            return true;
        }
    }
    false
}

fn node_contains_id(node: &WebCore, target_id: u32) -> bool {
    if node.node_id == target_id {
        return true;
    }
    for child in &node.children {
        if node_contains_id(child, target_id) {
            return true;
        }
    }
    false
}

/// Like `find_node_offset_mut` but uses strict `<` for text nodes so that a
/// caret sitting exactly at the end of a text node (e.g. just before a `<br>`)
/// falls through to the next sibling.  Used by `insert_char` when
/// `caret_at_line_start` is true — i.e. after `insert_br` placed the caret at
/// the logical start of the new visual line.
fn find_node_offset_after_br(
    node: &mut WebCore,
    mut offset: usize,
) -> Result<(&mut WebCore, usize), usize> {
    if node.is_text_node() {
        // Strict: offset must be *inside* the text (< not <=).
        // offset == text.len() falls through so the caller can try the next sibling.
        if offset < node.text.len() {
            return Ok((node, offset));
        } else {
            return Err(offset - node.text.len());
        }
    }
    if matches!(node.style.display, Display::None) {
        return Err(offset);
    }
    if node.tag == "br" {
        return Err(offset);
    }

    if !node.text.is_empty() {
        if offset < node.text.len() {
            return Ok((node, offset));
        } else {
            offset -= node.text.len();
        }
    }

    for child in &mut node.children {
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        match find_node_offset_after_br(child, offset) {
            Ok(res) => return Ok(res),
            Err(rem) => offset = rem,
        }
    }
    Err(offset)
}

/// Delete bytes `[start, end)` from the flat text of `container`.
/// Handles ranges that span multiple child text nodes (fixes the single-node bug).
fn delete_range_full(container: &mut WebCore, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let mut pos = 0usize;
    delete_flat_range(container, &mut pos, start, end);
}

fn delete_flat_range(node: &mut WebCore, pos: &mut usize, start: usize, end: usize) {
    if *pos >= end {
        return;
    }

    if node.is_text_node() {
        let orig_len = node.text.len();
        let node_end = *pos + orig_len;
        if *pos < end && node_end > start {
            let local_s = if start > *pos { start - *pos } else { 0 };
            let local_e = if end < node_end { end - *pos } else { orig_len };
            if local_s < local_e {
                node.text.drain(local_s..local_e);
            }
        }
        *pos = node_end;
        return;
    }

    if matches!(node.style.display, Display::None) || node.tag == "br" {
        return;
    }

    if !node.text.is_empty() {
        let orig_len = node.text.len();
        let node_end = *pos + orig_len;
        if *pos < end && node_end > start {
            let local_s = if start > *pos { start - *pos } else { 0 };
            let local_e = if end < node_end { end - *pos } else { orig_len };
            if local_s < local_e {
                node.text.drain(local_s..local_e);
            }
        }
        *pos = node_end;
    }

    for child in &mut node.children {
        if *pos >= end {
            break;
        }
        if matches!(child.style.position, Position::Absolute | Position::Fixed) {
            continue;
        }
        delete_flat_range(child, pos, start, end);
    }
}

/// Split the text node at `leaf_nid` at `local_off`, inserting a `<br>` between the halves.
fn split_node_with_br(root: &mut WebCore, leaf_nid: u32, local_off: usize) {
    split_node_with_br_impl(root, leaf_nid, local_off);
}

fn split_node_with_br_impl(node: &mut WebCore, leaf_nid: u32, local_off: usize) -> bool {
    // Case A: the leaf is a direct *text-node* child of this node.
    // Only apply when the child is a pure text node (#text).  Element nodes
    // that happen to carry `.text` (e.g. <td>) must NOT be removed from their
    // parent — they are handled by Case B via recursion.
    if let Some(idx) = node.children.iter().position(|c| c.node_id == leaf_nid) {
        if node.children[idx].is_text_node() {
            let text = node.children[idx].text.clone();
            let old_id = node.children[idx].node_id;
            let split = local_off.min(text.len());
            let before = text[..split].to_string();
            let after = text[split..].to_string();

            // Remove the original leaf, then insert [before, br, after] via DOM API.
            remove_child(node, old_id);
            // Build the replacement nodes
            let mut new_children = Vec::new();
            if !before.is_empty() {
                let mut bn = WebCore::new("#text");
                bn.text = before;
                new_children.push(bn);
            }
            new_children.push(WebCore::new("br"));
            if !after.is_empty() {
                let mut an = WebCore::new("#text");
                an.text = after;
                new_children.push(an);
            }
            // Insert at the position where the old node was
            // Reversed and all inserted at `idx`, so each one lands in front
            // of the last — the index is not needed and never was.
            for child in new_children.into_iter().rev() {
                node.children.insert(idx, child);
            }
            return true;
        }
    }

    // Case B: the leaf is this node itself (has its own .text, no children)
    if node.node_id == leaf_nid && !node.text.is_empty() {
        let split = local_off.min(node.text.len());
        let after = node.text[split..].to_string();
        node.text = node.text[..split].to_string();
        let mut after_node = WebCore::new("#text");
        after_node.text = after;
        node.children.insert(0, after_node);
        node.children.insert(0, WebCore::new("br"));
        return true;
    }

    // Recurse
    for child in &mut node.children {
        if split_node_with_br_impl(child, leaf_nid, local_off) {
            return true;
        }
    }
    false
}

// ─── Block-level insertion helpers ────────────────────────────────────────────

/// Insert a `<hr>` element after the block that currently contains the caret.
pub fn insert_hr(editor: &Editor, root: &mut WebCore) {
    let caret_id = match editor.caret_box {
        Some(id) => id,
        None => return,
    };
    if let Some(parent) = find_parent_mut_by_id(root, caret_id) {
        if let Some(idx) = parent.children.iter().position(|c| c.node_id == caret_id) {
            let mut hr = WebCore::new("hr");
            apply_property(std::sync::Arc::make_mut(&mut hr.style), "display", "block");
            parent.children.insert(idx + 1, hr);
        }
    }
}
