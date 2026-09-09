//! The `Document` itself — the struct, its clone and default, and the
//! node-path lookup.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

/// The root document: box tree + stylesheet + metadata.
/// Which popup an element opens on activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Color,
    Calendar,
}

#[derive(Clone, Debug)]
pub struct SmoothScrollState {
    pub element_id: u32,
    pub start_x: f32,
    pub start_y: f32,
    pub target_x: f32,
    pub target_y: f32,
    pub start_time: std::time::Instant,
    pub duration: std::time::Duration,
}

pub struct Document {
    pub root: WebCore,
    pub stylesheet: Stylesheet,
    pub title: String,
    pub base_url: String,

    // ── Arena-based DOM (bridge period: mirrors WebCore tree) ────────────────
    /// Arena-based DOM tree with stable NodeId identity.
    /// During the bridge period, this mirrors the WebCore tree structure.
    pub arena: DomArena,
    /// Next node_id to assign (monotonically increasing counter).
    pub next_node_id: u32,
    /// Bridge lookup: set of known node_ids in the tree.
    /// O(1) node lookup index: node_id → raw pointer into the WebCore tree.
    /// A node's PATH from the root — the child index at each level.
    ///
    /// ⛔ This held `*const WebCore` and was UNSOUND. Its safety comment said
    /// the pointers were "valid because the tree hasn't been mutated since
    /// `rebuild_node_index()` was called" — an invariant nothing enforced.
    /// `append_child` pushes to `parent.children`, the `Vec` reallocates, and
    /// every cached pointer into it dangles; the next `get_box_by_id`
    /// dereferenced one. Demonstrated by comparing the cached address against
    /// a fresh walk, without dereferencing either.
    ///
    /// A path cannot dangle. A stale one leads to the wrong node or to none,
    /// and the id is re-checked on arrival, so the answer is `None` rather
    /// than undefined. `arenaplan.md` item 4 removes the need for it entirely:
    /// once the payload is in arena-indexed arrays, a `NodeId` IS the index.
    pub node_index: HashMap<u32, Vec<u32>>,
    /// Which grammar this document was built from — HTML or XML.
    ///
    /// The difference the DOM actually draws is CASE. An HTML document
    /// ASCII-lowercases tag and attribute names, so `createElement("DIV")`
    /// makes a `div` and `getAttribute("HREF")` finds `href`; an XML document
    /// is case-sensitive, where `<Rect>` and `<rect>` are two elements.
    ///
    /// Everything webcore parses is HTML, so this only becomes anything other
    /// than `Html` when a caller asks for an XML document explicitly.
    pub kind: DocumentKind,
    /// Separated layout data indexed by node_id (bridge: duplicates WebCore geometry).
    pub layout_store: crate::layout::layout_box::LayoutStore,
    /// Nodes created by dom_create_element/dom_create_text that haven't been
    /// inserted into the WebCore tree yet. Consumed by dom_append_child/dom_insert_before.
    pub pending_nodes: HashMap<u32, WebCore>,
    /// URLs from `<link rel="stylesheet" href="...">` tags in `<head>`.
    /// Populated by the parser so the host can fetch and merge external CSS.
    /// External stylesheets from `<link rel="stylesheet">` tags: (href, media).
    /// The `media` string (e.g. "print", "screen", "") is preserved so that
    /// print-only sheets can be skipped for screen rendering but kept for future
    /// print support.
    pub linked_stylesheets: Vec<(String, String)>,
    pub editor: Editor,
    /// Drawing state for the document's `<canvas>` elements, keyed by node id.
    /// The pixels stay on the element in `WebCore::image_data`; this is what
    /// persists between two calls from a page. See `canvas::CanvasSurfaces`.
    pub canvas_surfaces: crate::canvas::CanvasSurfaces,
    /// NodeId-based event system with capture/bubble phases.
    pub event_targets: crate::dom::events::EventTargetMap,
    /// Viewport scroll position in logical pixels (managed by Renderer::render).
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Active scrollbar drag state (None when not dragging).
    pub scrollbar_drag: Option<ScrollbarDrag>,
    /// Currently hovered element (node_id, 0 if none).
    pub hovered_box: u32,
    /// Suppresses the next hover change after a hover-triggered relayout.
    /// Prevents feedback loops: hover opens dropdown → layout changes →
    /// re-hit-test finds different element → dropdown closes → repeat.
    pub hover_suppress_count: u8,
    /// Currently active (pressed) element (node_id, 0 if none).
    pub active_box: u32,
    /// Currently focused element (node_id, 0 if none).
    pub focused_box: u32,
    /// Element hit on last MouseDown — used to fire Click on MouseUp if same target.
    pub mousedown_target: u32,
    /// Last click target + time for DblClick detection.
    pub last_click_target: u32,
    pub last_click_time: Option<std::time::Instant>,
    /// Drag state machine.
    pub drag_source: u32,
    pub drag_start_doc_pt: (f32, f32),
    pub drag_active: bool,
    /// Set of link hrefs the user has clicked (for :visited pseudo-class).
    pub visited_urls: std::collections::HashSet<String>,
    /// `setCustomValidity()` messages, keyed by element.
    ///
    /// Custom validity is STATE, not content — there is no attribute for it,
    /// and a page that sets one and reloads loses it. Keeping it beside the
    /// document rather than on the element is also what lets a control that
    /// was never touched cost nothing.
    pub custom_validity: HashMap<u32, String>,
    /// The `DocumentType` node's id, or 0 when the document had no doctype.
    pub doctype: u32,
    /// The rendering mode the doctype selected (HTML §13.2.6.4.1).
    ///
    /// Stored as the full tri-state even though `compatMode` collapses two of
    /// them: limited-quirks is a real mode with its own line-box rule, and a
    /// field that only ever answers through the collapse could not tell it
    /// from no-quirks.
    pub quirks: crate::html::doctype::QuirksMode,
    /// `document.characterSet`. Always the encoding the bytes were DECODED
    /// with — `parse_html` takes a Rust `&str`, which is UTF-8 by
    /// construction, and `parse_html_bytes` records what it sniffed.
    pub character_set: String,
    /// Live `TreeWalker`s and `NodeIterator`s, keyed by handle.
    ///
    /// They live on the document because an iterator's pre-removing steps have
    /// to run from inside `remove_child` — a traversal the caller owned
    /// outright could not be told the tree had changed under it.
    pub traversals: crate::dom::traversal::TraversalStore,
    /// Live `Range`s. Unlike a traversal a range holds no callback, so this
    /// one DOES survive a document clone.
    pub ranges: crate::dom::range::RangeStore,
    /// The **top layer**, bottom-first: modal dialogs and showing popovers, in
    /// the order they entered it. The ordering is what nested popovers and
    /// light dismiss need; `WebCore::top_layer_kind` is the per-node mirror
    /// the selector matcher reads, and both are written in one place.
    pub top_layer: Vec<u32>,
    /// Set while `split_text` runs. The split has its OWN range rule, and the
    /// generic insert/replace-data hooks its internals would otherwise fire
    /// would apply a second, wrong adjustment on top of it.
    pub suppress_range_updates: bool,
    /// Last known logical viewport size — kept in sync by LayoutEngine::layout.
    pub viewport_w: f32,
    pub viewport_h: f32,
    /// True when focus was moved by keyboard (Tab/Shift+Tab) — drives :focus-visible.
    pub keyboard_focus: bool,
    /// Caret blink epoch — reset on each keystroke so caret stays visible while typing.
    pub caret_blink_epoch: std::time::Instant,
    /// Currently open select dropdown (node_id, 0 if none open).
    pub open_select: u32,
    /// The element whose PICKER is open — `<input type=color>` today.
    ///
    /// The same shape `open_select` has: one node, drawn as an overlay after
    /// the page and hit-tested before anything else while it is open. A picker
    /// is user-agent chrome that appears on activation, which is exactly what
    /// the dropdown already is; there is one popup surface here and this is a
    /// second thing on it, not a new mechanism.
    pub open_picker: u32,
    /// The `<input type=range>` whose knob the pointer is holding (0 = none).
    ///
    /// A slider is the one control whose interaction is the pointer's whole
    /// PATH rather than where it landed. HTML says so in the words that
    /// distinguish its two events: "while the user is dragging the control's
    /// knob, input events would fire whenever the position changed, whereas
    /// the change event would only fire when the user let go of the knob,
    /// committing to a specific value."
    ///
    /// Held as the ELEMENT, not a flag, because a drag that has wandered off
    /// the control still belongs to it — a pointer released over the page must
    /// commit the slider it grabbed, not abandon it.
    pub dragging_range: u32,
    /// What the range held when the drag began.
    ///
    /// `change` fires on release "if the value is committed" — a press and
    /// release that moved nothing committed nothing, so this is what release
    /// compares against instead of firing unconditionally.
    pub range_drag_origin: String,
    /// Hovered option index in open dropdown (-1 = none).
    pub dropdown_hover_idx: i32,
    /// Form event callback — set by the host to handle form interactions.
    /// Called when users interact with form elements (click checkbox, type in input, etc.).
    pub on_form_event: Option<FormEventCallback>,

    // ── Engine callbacks ─────────────────────────────────────────────────────
    /// Called when a link is clicked (href). Return `true` to handle navigation,
    /// `false` to let the engine follow the link.
    pub on_navigate: Option<Box<dyn FnMut(&str) -> bool + Send>>,
    /// Called when the document title changes (e.g. via `<title>` or DOM mutation).
    pub on_title_change: Option<Box<dyn FnMut(&str) + Send>>,
    /// Called after any DOM mutation (node added/removed/attribute changed).
    /// The argument is the node_id of the mutated node.
    pub on_dom_mutation: Option<Box<dyn FnMut(u32) + Send>>,
    /// Called when a node becomes visible in the viewport (intersection observer pattern).
    pub on_visibility_change: Option<Box<dyn FnMut(u32, bool) + Send>>,

    // ── CSS animation / transition runtime ────────────────────────────────────
    /// All currently running CSS animations (one entry per animation per element).
    pub active_animations: Vec<AnimState>,
    /// Per-element active transitions, keyed by WebCore pointer (as usize).
    pub(crate) transition_states: HashMap<u32, Vec<TransitionState>>,
    /// Previous transitionable style values per element, for change detection.
    pub(crate) prev_styles: HashMap<u32, HashMap<String, String>>,
    /// Clean cascade-time style snapshot, keyed by element pointer.
    /// Populated when the cascade runs; never mutated by animation overrides.
    /// Used by sync_transitions so hover-out correctly reads the base (not overridden) values.
    pub(crate) cascade_styles: HashMap<u32, HashMap<String, String>>,
    /// Interpolated CSS property overrides produced by `tick_animations`.
    /// Applied on top of the cascade result before geometry runs.
    pub(crate) animation_overrides: HashMap<u32, Vec<(String, String)>>,
    /// Set by `tick_animations`; tells the host to request another render frame.
    pub needs_animation_frame: bool,
    /// Active `scroll-behavior:smooth` element scrolls.
    pub(crate) smooth_scrolls: Vec<SmoothScrollState>,
    /// Set when `hovered_box` changes; cleared by `layout()` after running `sync_transitions`.
    pub hover_changed: bool,
    /// Node IDs of elements that have hover-dependent CSS rules.
    /// Only these need re-cascade on hover change. Populated during full cascade.
    pub hover_sensitive_nodes: HashSet<u32>,
    /// Set by DOM API mutations to force a full cascade on next layout.
    pub style_dirty: bool,
    /// Previous hover target — used to compute the diff for incremental cascade.
    pub prev_hovered_box: u32,

    // ── aria-live region machinery ─────────────────────────────────────────────
    /// Announcements queued since the last call to `take_announcements()`.
    pub pending_announcements: Vec<Announcement>,
    /// Text-content snapshots for each aria-live region, keyed by WebCore pointer.
    /// Updated every layout pass to detect content changes.
    pub(crate) live_region_snapshots: HashMap<u32, String>,
    /// `false` until the first `check_live_regions()` call.
    /// On the very first pass, only assertive regions announce their initial content;
    /// polite regions are silently initialised so they don't flood the user on load.
    pub(crate) live_regions_initialized: bool,

    /// Monotonically increasing counter bumped after every layout pass.
    /// Used by the Renderer to detect when the display list cache is stale.
    pub layout_generation: u64,

    // ── Async image loading ─────────────────────────────────────────────────
    /// Receiver for images arriving from background fetch threads.
    /// Each message is (node_path, decoded_rgba, width, height).
    pub pending_images: Option<std::sync::mpsc::Receiver<(Vec<usize>, crate::html::DecodedImage)>>,
    /// Number of image fetches still in flight.
    pub images_in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Document {
    // ── Node index (node_id → pointer for O(1) lookup) ───────────────────────

    /// High-level mouse event entry point.
    /// The palette geometry of an open picker — one place, so the hit test and
    /// the paint cannot disagree about where the swatches are.
    ///
    /// Returns the popup's origin and cell size. `None` when the element has no
    /// laid-out box, which is also when there is nothing to click.
    /// The `<details>` a click belongs to, when the click is on its `<summary>`.
    ///
    /// Walks UP from the hit node, because a click lands on whatever is
    /// innermost — the text inside the summary, or an element the author put
    /// there — and the summary itself is often not what was hit.
    pub(crate) fn summary_details(&self, hit: u32) -> Option<u32> {
        // ⛔ Walks the RENDER TREE, not the arena. A hit id comes from hit
        // testing over boxes, and `Document::parent_node` is the ARENA's
        // parent — it asserts on an id the arena does not hold, which took the
        // whole process down on the first click of a submit button. Every
        // other click-path walk here (`find_form_parent_id`) goes over boxes
        // for the same reason.
        fn walk<'a>(node: &'a WebCore, hit: u32, chain: &mut Vec<&'a WebCore>) -> bool {
            chain.push(node);
            if node.node_id == hit {
                return true;
            }
            for child in &node.children {
                if walk(child, hit, chain) {
                    return true;
                }
            }
            chain.pop();
            false
        }
        let mut chain = Vec::new();
        if !walk(&self.root, hit, &mut chain) {
            return None;
        }
        // Innermost first: the click lands on the text inside the summary, or
        // on whatever the author put there, rather than on the summary itself.
        for i in (0..chain.len()).rev() {
            if chain[i].tag == "summary" {
                return chain
                    .get(i.checked_sub(1)?)
                    .filter(|p| p.tag == "details")
                    .map(|p| p.node_id);
            }
        }
        None
    }

    pub(crate) fn picker_rect(&self, id: u32) -> Option<(f32, f32, f32, f32)> {
        let node = self.find_webcore(id)?;
        let br = node.layout.border_rect;
        // Below the control, as the dropdown opens below its select.
        let (w, h) = match self.picker_kind(id) {
            Some(PickerKind::Calendar) => (
                crate::widgets::Calendar::width(),
                crate::widgets::Calendar::height(),
            ),
            _ => {
                let cols = crate::widgets::PALETTE_COLUMNS as f32;
                let rows = (crate::widgets::PALETTE.len() as f32 / cols).ceil();
                let cell = crate::widgets::PALETTE_CELL;
                (cols * cell, rows * cell)
            }
        };
        Some((br.x, br.y + br.h, w, h))
    }

    /// Which picker an element opens, if any — the one place that decides, so
    /// the geometry, the paint and the hit test cannot disagree.
    pub(crate) fn picker_kind(&self, id: u32) -> Option<PickerKind> {
        let node = self.find_webcore(id)?;
        if node.tag != "input" {
            return None;
        }
        match node
            .attributes
            .get("type")?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "color" => Some(PickerKind::Color),
            // `month` and `week` open a calendar too in a browser, but they
            // pick a MONTH and a WEEK, not a day — a day grid would write a
            // value their format cannot hold. Until each has its own grid,
            // only `date` opens one.
            "date" => Some(PickerKind::Calendar),
            _ => None,
        }
    }

    /// A click on a LIST BOX row, at document y `click_y`. Returns whether the
    /// selection moved.
    ///
    /// A list box draws its own rows and has no popup, so this is the whole
    /// interaction — the drop-down's `open_select` state machine is never
    /// involved. Which algorithm runs depends on the control:
    ///
    /// * `multiple` — **toggle** the row (HTML §4.10.7: "the user agent should
    ///   allow the user to toggle the selectedness of the option elements").
    ///   Toggling on a plain click is the only way to reach a multi-selection
    ///   at a seam with no modifier keys, and it is what the
    ///   `CheckedListBox` this renders for does anyway.
    /// * single-select — **pick an option**, the algorithm a drop-down runs.
    ///
    /// `unselect_request` is the third case, and it is the one HTML words as a
    /// request rather than a click: "if the multiple attribute is absent and
    /// the element's display size is greater than 1, then the user agent should
    /// also allow the user to request that the option whose selectedness is
    /// true, if any, be unselected."
    ///
    /// A SINGLE-SELECT list box only. A drop-down has no such affordance (its
    /// display size is 1) and a `multiple` list box already reaches an empty
    /// selection by toggling, so binding it there would be a second way to do
    /// one thing. The gesture is the platform's — ctrl/⌘-click on the row that
    /// is already selected — which is why it arrives as an answered question
    /// rather than being decided here.
    pub(crate) fn click_list_box_row(
        &mut self,
        select_id: u32,
        click_y: f32,
        unselect_request: bool,
    ) -> bool {
        let Some(select) = self.find_webcore(select_id) else {
            return false;
        };
        if select.attributes.contains_key("disabled") {
            return false;
        }
        let content = select.layout.content_rect;
        let font_px = select.style.font_size_px(16.0, 16.0).max(1.0);
        let options = crate::html::forms::option_ids(select);
        let Some(row) = crate::html::forms::list_box_row_at(
            content.y,
            content.h,
            font_px,
            options.len(),
            click_y,
        ) else {
            return false;
        };
        let option_id = options[row];

        let Some(select_mut) = self.find_webcore_mut(select_id) else {
            return false;
        };
        let multiple = crate::html::forms::is_multiple(&*select_mut);
        // "Upon this request being conveyed to the user agent, and before the
        // relevant user interaction event is queued (e.g. before the click
        // event), the user agent must set the selectedness of that option
        // element to false, set its dirtiness to true, and then send select
        // update notifications." Only the option that IS selected can be
        // unselected, so a request on any other row is an ordinary pick.
        let already_selected = crate::html::forms::list_of_options(&*select_mut)
            .into_iter()
            .any(|o| o.node_id == option_id && o.selectedness);
        let changed = if unselect_request && !multiple && already_selected {
            crate::html::forms::unselect_option(select_mut, option_id)
        } else if multiple {
            crate::html::forms::toggle_option(select_mut, option_id)
        } else {
            crate::html::forms::pick_option(select_mut, option_id)
        };
        if changed {
            select_mut.layout.layout_dirty = true;
            self.send_select_update_notifications(select_id);
        }
        changed
    }

    /// A click along a RANGE control's track, at document point `doc_pt`.
    /// Returns whether the value moved.
    ///
    /// `widgets::Slider` already owned the inverse of its own paint geometry —
    /// the thumb-radius inset at each end, and the axis a vertical writing mode
    /// turns — in `set_from_pointer`. Nothing had ever called it, so every
    /// trackbar and scrollbar was decorative. Driving the widget rather than
    /// re-deriving the mapping is what keeps the thumb under the pointer.
    ///
    /// The number that comes back is then put through the control's own step
    /// and bounds, because HTML enforces them "even during user input" — a
    /// click three-fifths along a `step=20` control lands on a multiple of 20,
    /// not on 60.4.
    ///
    /// ⛔ A CLICK, not a drag: the track jumps to the point rather than paging
    /// toward it. Both are user-agent choices; this is the one browsers make.
    /// Dragging needs `mouse_move` wired to the same path.
    pub(crate) fn drag_range_to(&mut self, input_id: u32, doc_pt: (f32, f32)) -> bool {
        let Some(input) = self.find_webcore(input_id) else {
            return false;
        };
        // Mutability (HTML §4.10.18.2). `readonly` does NOT apply to a range,
        // so `disabled` is the whole test.
        if input.attributes.contains_key("disabled") {
            return false;
        }
        let rect = input.layout.content_rect;
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return false;
        }
        let min = crate::html::forms::range_minimum(input);
        let max = crate::html::forms::range_maximum(input);
        let current = input_value(input);
        let current_num = crate::html::forms::parse_floating_point(&current).unwrap_or(min);

        let mut slider = crate::widgets::Slider::new(min as f32, max as f32, current_num as f32);
        slider.width = rect.w;
        slider.height = rect.h;
        slider.vertical = !matches!(input.style.writing_mode, WritingMode::HorizontalTB);
        slider.mouse_down(doc_pt.0 - rect.x, doc_pt.1 - rect.y);
        let picked = slider.actual_value() as f64;

        // "User agents must not allow the user to set the value to a string
        // that is not a valid floating-point number", and the range and step
        // constraints hold throughout — so the pointer's answer goes through
        // the same sanitization the markup's did.
        let mut value = picked;
        if value < min {
            value = min;
        }
        if value > max && max >= min {
            value = max;
        }
        let snapped = crate::html::forms::snap_to_step(input, value);
        let text = crate::html::forms::best_representation(snapped);
        if text == current {
            return false;
        }

        let id = input.attributes.get("id").cloned().unwrap_or_default();
        let name = input.attributes.get("name").cloned().unwrap_or_default();
        if let Some(input_mut) = self.find_webcore_mut(input_id) {
            input_mut.value_state = Some(text.clone());
            input_mut.dirty_value = true;
            input_mut.layout.layout_dirty = true;
        }
        // `input` ALONE. Moving the knob is not committing to a value — that
        // is what release means, and `commit_range_drag` is where `change`
        // fires. Firing both here made every pixel of a drag look like a
        // finished decision.
        if let Some(ref mut cb) = self.on_form_event {
            cb(&FormEvent {
                tag: "input".into(),
                id,
                name,
                kind: FormEventKind::Input(text),
                element: input_id,
            });
        }
        true
    }

    /// Let go of the knob: fire `change` if the drag actually moved the value,
    /// and stop holding the control.
    ///
    /// Guarded on the value rather than on the drag having happened, because
    /// "the change event fires when the value is committed" — a press and
    /// release that moved nothing committed nothing, and a slider that
    /// announced a change every time it was merely touched would be lying to
    /// every handler counting them.
    pub(crate) fn commit_range_drag(&mut self) -> bool {
        let input_id = std::mem::replace(&mut self.dragging_range, 0);
        let origin = std::mem::take(&mut self.range_drag_origin);
        if input_id == 0 {
            return false;
        }
        let Some(input) = self.find_webcore(input_id) else {
            return false;
        };
        let text = input_value(input);
        if text == origin {
            return false;
        }
        let id = input.attributes.get("id").cloned().unwrap_or_default();
        let name = input.attributes.get("name").cloned().unwrap_or_default();
        if let Some(ref mut cb) = self.on_form_event {
            cb(&FormEvent {
                tag: "input".into(),
                id,
                name,
                kind: FormEventKind::Change(text),
                element: input_id,
            });
        }
        true
    }

    /// Whether a node is an `<input type=range>`.
    pub(crate) fn is_range_input(&self, id: u32) -> bool {
        self.find_webcore(id)
            .map(|n| {
                n.tag == "input"
                    && n.attributes
                        .get("type")
                        .map(|t| t.trim().eq_ignore_ascii_case("range"))
                        .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    /// **Send select update notifications** (HTML §4.10.7): fire `input`, then
    /// `change`.
    ///
    /// ⛔ Both, in that order. The drop-down path fired `change` alone, so a
    /// program listening for `input` — which is what a live-updating handler
    /// listens for — never heard a `<select>` at all.
    pub(crate) fn send_select_update_notifications(&mut self, select_id: u32) {
        let Some(select) = self.find_webcore(select_id) else {
            return;
        };
        let value = crate::html::forms::select_value(select);
        let id = select.attributes.get("id").cloned().unwrap_or_default();
        let name = select.attributes.get("name").cloned().unwrap_or_default();
        if let Some(ref mut cb) = self.on_form_event {
            for kind in [
                FormEventKind::Input(value.clone()),
                FormEventKind::Change(value),
            ] {
                cb(&FormEvent {
                    tag: "select".into(),
                    id: id.clone(),
                    name: name.clone(),
                    kind,
                    element: select_id,
                });
            }
        }
    }

    /// A click on an element that is not a form control.
    ///
    /// UI Events puts `click` on whatever the pointer pressed and released
    /// over. `handle_form_click` only knows the controls, so everything else —
    /// a `<td>`, a `<div>`, an `<li>` — had a listener that could be registered
    /// and never fired. A control composed out of ordinary elements, which is
    /// what a calendar's day grid is, could be built and could not be used.
    ///
    /// The event carries the element's `id` and `name` like any other, and its
    /// text as the value, so a handler reads the same shape whatever it is on.
    pub(crate) fn fire_element_click(&mut self, node_id: u32) {
        // A text box is not an event target. The click belongs to the ELEMENT
        // that owns the text — which is what hitting a word means, and what a
        // browser reports. Returning early instead made a click that landed on
        // a cell's digits fire nothing, so the control worked in the middle of
        // a cell and not on its text.
        let mut node_id = node_id;
        for _ in 0..4 {
            match self.find_webcore(node_id) {
                Some(n) if n.tag.starts_with('#') => node_id = self.parent_node(node_id),
                _ => break,
            }
        }
        let Some(node) = self.find_webcore(node_id) else {
            return;
        };
        if node.tag.starts_with('#') {
            return;
        }
        let tag = node.tag.clone();
        let id = node.attributes.get("id").cloned().unwrap_or_default();
        let name = node.attributes.get("name").cloned().unwrap_or_default();
        let text = node.text.clone();
        self.svg_trigger_event(node_id, "click");
        if let Some(ref mut cb) = self.on_form_event {
            cb(&FormEvent {
                tag,
                id,
                name,
                kind: FormEventKind::Click(text),
                element: node_id,
            });
        }
    }

    /// The month a date picker is showing: the element's own value, or the
    /// current month when it has none — which is what a browser opens on.
    pub(crate) fn picker_month(&self, id: u32) -> (i32, u32, Option<u32>) {
        let value = self.find_webcore(id).map(input_value).unwrap_or_default();
        match crate::widgets::parse_date(&value) {
            Some((y, m, d)) => (y, m, Some(d)),
            // No date library here, and none needed: an empty control opens on
            // a fixed, obviously-neutral month rather than pretending to know
            // today. The value it writes is a real date either way.
            None => (2026, 1, None),
        }
    }

    /// Which palette colour a point lands on, if any.
    /// Which day an open calendar's point lands on, and the month it belongs to.
    pub(crate) fn calendar_hit(&self, id: u32, doc_pt: (f32, f32)) -> Option<(i32, u32, u32)> {
        let (x, y, w, h) = self.picker_rect(id)?;
        if doc_pt.0 < x || doc_pt.0 >= x + w || doc_pt.1 < y || doc_pt.1 >= y + h {
            return None;
        }
        let (year, month, _) = self.picker_month(id);
        let day = crate::widgets::Calendar::day_at(
            (doc_pt.0 - x, doc_pt.1 - y),
            crate::widgets::first_weekday(year, month),
            crate::widgets::days_in_month(year, month),
        )?;
        Some((year, month, day))
    }

    pub(crate) fn picker_hit(&self, id: u32, doc_pt: (f32, f32)) -> Option<(u8, u8, u8)> {
        let (x, y, w, h) = self.picker_rect(id)?;
        if doc_pt.0 < x || doc_pt.0 >= x + w || doc_pt.1 < y || doc_pt.1 >= y + h {
            return None;
        }
        let cell = crate::widgets::PALETTE_CELL;
        let col = ((doc_pt.0 - x) / cell) as usize;
        let row = ((doc_pt.1 - y) / cell) as usize;
        crate::widgets::PALETTE
            .get(row * crate::widgets::PALETTE_COLUMNS + col)
            .copied()
    }

    // ── aria-live ──────────────────────────────────────────────────────────────

    // ── CSS Animation / Transition runtime ────────────────────────────────────
}

impl Clone for Document {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            stylesheet: self.stylesheet.clone(),
            title: self.title.clone(),
            base_url: self.base_url.clone(),
            arena: DomArena::new(), // cloned docs get fresh arena (rebuilt on demand)
            next_node_id: self.next_node_id,
            node_index: HashMap::new(), // rebuilt on demand
            // A copy of an XML document is still an XML document — carried
            // over rather than reset, or the copy would start folding names
            // the original does not.
            kind: self.kind,
            layout_store: crate::layout::layout_box::LayoutStore::new(),
            pending_nodes: HashMap::new(),
            linked_stylesheets: self.linked_stylesheets.clone(),
            editor: self.editor.clone(),
            // The canvas BITMAPS come along inside `root.clone()`, because
            // they live on the elements. The drawing STATE does not: a copy of
            // a document starts from the default context, the same way it
            // starts with no event listeners.
            canvas_surfaces: crate::canvas::CanvasSurfaces::default(),
            event_targets: crate::dom::events::EventTargetMap::new(), // listeners not cloned
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            scrollbar_drag: self.scrollbar_drag.clone(),
            hovered_box: self.hovered_box,
            hover_suppress_count: self.hover_suppress_count,
            active_box: self.active_box,
            focused_box: self.focused_box,
            mousedown_target: self.mousedown_target,
            last_click_target: self.last_click_target,
            last_click_time: self.last_click_time,
            drag_source: self.drag_source,
            drag_start_doc_pt: self.drag_start_doc_pt,
            drag_active: self.drag_active,
            visited_urls: self.visited_urls.clone(),
            custom_validity: self.custom_validity.clone(),
            doctype: self.doctype,
            quirks: self.quirks,
            character_set: self.character_set.clone(),
            // Filters are `Box<dyn FnMut>` and do not clone, the same reason
            // `event_targets` starts empty above.
            traversals: crate::dom::traversal::TraversalStore::new(),
            ranges: self.ranges.clone(),
            top_layer: self.top_layer.clone(),
            suppress_range_updates: false,
            viewport_w: self.viewport_w,
            viewport_h: self.viewport_h,
            keyboard_focus: self.keyboard_focus,
            caret_blink_epoch: std::time::Instant::now(),
            open_select: 0,
            open_picker: 0,
            dropdown_hover_idx: -1,
            // Transient interaction state, like the two popups beside it: a
            // fresh document is holding nothing.
            dragging_range: 0,
            range_drag_origin: String::new(),
            active_animations: self.active_animations.clone(),
            transition_states: self.transition_states.clone(),
            prev_styles: self.prev_styles.clone(),
            cascade_styles: self.cascade_styles.clone(),
            animation_overrides: self.animation_overrides.clone(),
            needs_animation_frame: self.needs_animation_frame,
            smooth_scrolls: self.smooth_scrolls.clone(),
            hover_changed: self.hover_changed,
            hover_sensitive_nodes: self.hover_sensitive_nodes.clone(),
            style_dirty: self.style_dirty,
            prev_hovered_box: self.prev_hovered_box,
            pending_announcements: self.pending_announcements.clone(),
            live_region_snapshots: self.live_region_snapshots.clone(),
            live_regions_initialized: self.live_regions_initialized,
            layout_generation: self.layout_generation,
            // Async image state is not cloned — cloned docs start with no pending fetches.
            pending_images: None,
            images_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            on_form_event: None,
            on_navigate: None,
            on_title_change: None,
            on_dom_mutation: None,
            on_visibility_change: None, // callbacks not cloned
        }
    }
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("title", &self.title)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: Document contains raw pointers (hovered_box, active_box, etc.) that are
// only used on the main thread. When sent across threads (e.g. background loading),
// these pointers are always null. The receiver must not dereference them until
// re-established on the owning thread.
unsafe impl Send for Document {}

pub(crate) fn find_node_by_path_mut<'a>(
    root: &'a mut WebCore,
    path: &[usize],
) -> Option<&'a mut WebCore> {
    let mut node = root;
    for &idx in path {
        if idx >= node.children.len() {
            return None;
        }
        node = &mut node.children[idx];
    }
    Some(node)
}
