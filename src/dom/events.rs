//! NodeId-based event dispatch with capture/bubble phases.
//!
//! This is the DOM spec-compliant event system. Listeners are registered on
//! specific nodes (by node_id), and events flow through three phases:
//! 1. **Capture**: root → target (listeners with `capture: true`)
//! 2. **Target**: fire on the target node
//! 3. **Bubble**: target → root (listeners with `capture: false`)
//!
//! This coexists with the old CSS-selector-based EventListeners during migration.

use crate::dom::arena::{DomArena, NodeId};
use std::collections::HashMap;

// ─── Event Phase ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    None,
    Capture,
    Target,
    Bubble,
}

// ─── Event ──────────────────────────────────────────────────────────────────

/// DOM event — flows through capture → target → bubble phases.
#[derive(Debug)]
pub struct DomEvent {
    pub event_type: String,
    pub target: u32,         // node_id of the deepest hit element
    pub current_target: u32, // node_id of the element currently handling
    pub phase: EventPhase,
    /// Mouse/pointer position in document coordinates.
    pub client_x: f32,
    pub client_y: f32,
    /// Mouse button (0=left, 1=middle, 2=right).
    pub button: u8,
    /// Keyboard key code.
    pub key_code: u32,
    pub key: String,
    pub char_code: Option<char>,
    /// Modifier keys.
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    /// Wheel delta.
    pub delta_x: f32,
    pub delta_y: f32,
    /// Related target (e.g. for mouseenter/leave, the element being entered/left).
    pub related_target: u32,
    /// `Event.bubbles` — does this event run the bubble phase at all?
    ///
    /// Not every event does. `load`, `focus`, `mouseenter` and friends fire on
    /// the target and stop; dispatching them up the tree calls listeners a
    /// browser never would. Set from `event_defaults` by `DomEvent::new`.
    pub bubbles: bool,
    /// `Event.cancelable` — may `preventDefault()` do anything?
    pub cancelable: bool,
    /// `Event.composed` — does this event cross a shadow boundary?
    pub composed: bool,
    /// `Event.isTrusted` — true when the user agent generated it, false when a
    /// script did (DOM §2.2). `dispatch_event` from the API sets it false.
    pub is_trusted: bool,
    /// `Event.timeStamp` — milliseconds since the document began.
    pub time_stamp: f64,
    /// `Event.composedPath()` — the propagation path, target-first. Filled by
    /// dispatch; empty before and after.
    pub composed_path: Vec<u32>,

    /// `ToggleEvent.oldState` / `.newState` (HTML §6.12) — `"open"` or
    /// `"closed"`. Empty for every other event type, like the mouse and
    /// keyboard fields above, which this struct already carries for all of
    /// them rather than splitting into one type per interface.
    pub old_state: String,
    pub new_state: String,
    /// `CustomEvent.detail`.
    detail: String,
    /// `UIEvent.detail` — click count.
    ui_detail: i32,
    /// `MouseEvent.buttons` bitmask: 1 left, 2 right, 4 middle.
    buttons: u8,
    /// `MouseEvent.screenX/Y`.
    screen_x: f32,
    screen_y: f32,
    /// Page scroll at dispatch, for `pageX`/`pageY`.
    scroll_x: f32,
    scroll_y: f32,
    /// The target's padding-box origin, for `offsetX`/`offsetY`.
    target_origin: (f32, f32),
    /// `KeyboardEvent.code` — the physical key.
    code: String,
    /// `KeyboardEvent.location`.
    location: u32,
    /// `KeyboardEvent.repeat`.
    repeat: bool,
    /// `KeyboardEvent.isComposing`.
    is_composing: bool,
    /// `WheelEvent.deltaZ` and `deltaMode`.
    delta_z: f32,
    delta_mode: u32,
    /// `InputEvent.data` and `inputType`.
    data: Option<String>,
    input_type: String,
    /// Call to prevent default browser behavior.
    prevented: bool,
    /// Call to stop event from reaching further listeners.
    stopped: bool,
    /// Call to stop event from reaching listeners on the same element.
    immediate_stopped: bool,
    /// DOM §2.9's "dispatch flag". `dispatchEvent` on an event that is already
    /// being dispatched must fail rather than re-enter — a handler calling
    /// `dispatchEvent(sameEvent)` would otherwise recurse forever.
    dispatching: bool,
}

/// `bubbles`, `cancelable` and `composed` for an event type (HTML §4.11 and the
/// UI Events spec).
///
/// These are per-TYPE facts, not per-dispatch choices: `click` bubbles and is
/// cancelable, `load` is neither, `focus` is neither but `focusin` bubbles.
/// Getting them wrong is not cosmetic — a non-bubbling event dispatched up the
/// tree runs listeners a browser never runs.
pub fn event_defaults(event_type: &str) -> (bool, bool, bool) {
    match event_type {
        // Mouse and pointer — bubble, cancelable, composed.
        "click" | "auxclick" | "dblclick" | "mousedown" | "mouseup" | "mousemove" | "mouseover"
        | "mouseout" | "contextmenu" | "wheel" | "pointerdown" | "pointerup" | "pointermove"
        | "pointerover" | "pointerout" | "pointercancel" | "keydown" | "keyup" | "keypress"
        | "beforeinput" | "compositionstart" | "compositionupdate" | "compositionend" | "cut"
        | "copy" | "paste" | "dragstart" | "drag" | "dragenter" | "dragover" | "dragleave"
        | "drop" | "submit" | "reset" | "beforetoggle" => (true, true, true),
        // Bubble, not cancelable.
        "input" | "change" | "select" | "focusin" | "focusout" | "dragend" | "scrollend"
        | "formdata" | "slotchange" => (true, false, true),
        // Fire on the target only, not cancelable.
        "load" | "unload" | "error" | "abort" | "focus" | "blur" | "mouseenter" | "mouseleave"
        | "pointerenter" | "pointerleave" | "scroll" | "resize" | "toggle" | "invalid"
        | "cancel" | "close" | "canplay" | "canplaythrough" | "durationchange" | "emptied"
        | "ended" | "loadeddata" | "loadedmetadata" | "loadstart" | "pause" | "play"
        | "playing" | "progress" | "ratechange" | "seeked" | "seeking" | "stalled" | "suspend"
        | "timeupdate" | "volumechange" | "waiting" => (false, false, false),
        // `beforeunload` is the odd one: does not bubble, IS cancelable.
        "beforeunload" => (false, true, false),
        // Window/document lifecycle — bubble, not cancelable.
        "hashchange" | "popstate" | "pagehide" | "pageshow" | "offline" | "online"
        | "languagechange" | "storage" | "message" | "messageerror" | "afterprint"
        | "beforeprint" | "visibilitychange" | "readystatechange" | "DOMContentLoaded" => {
            (true, false, false)
        }
        // Animations and transitions bubble and are cancelable.
        "animationstart" | "animationend" | "animationiteration" | "transitionstart"
        | "transitionend" | "transitionrun" | "transitioncancel" => (true, true, true),
        // Anything else, including custom events, defaults to the DOM's own
        // defaults: an event created with no options bubbles nowhere.
        _ => (false, false, false),
    }
}

impl DomEvent {
    /// An event the USER AGENT fires: flags come from the per-type table and
    /// `isTrusted` is true. For a script-constructed event use
    /// [`DomEvent::new_script`] or [`DomEvent::new_with_flags`] — those default
    /// to not bubbling, which is what `new Event(type)` does in a browser.
    pub fn new(event_type: impl Into<String>, target: u32) -> Self {
        let ty: String = event_type.into();
        let (bubbles, cancelable, composed) = event_defaults(&ty);
        Self {
            event_type: ty,
            target,
            current_target: 0,
            phase: EventPhase::None,
            client_x: 0.0,
            client_y: 0.0,
            button: 0,
            key_code: 0,
            key: String::new(),
            char_code: None,
            ctrl_key: false,
            shift_key: false,
            alt_key: false,
            meta_key: false,
            delta_x: 0.0,
            delta_y: 0.0,
            related_target: 0,
            bubbles,
            cancelable,
            composed,
            is_trusted: true,
            time_stamp: time_origin_elapsed_ms(),
            composed_path: Vec::new(),
            old_state: String::new(),
            new_state: String::new(),
            detail: String::new(),
            ui_detail: 0,
            buttons: 0,
            screen_x: 0.0,
            screen_y: 0.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            target_origin: (0.0, 0.0),
            code: String::new(),
            location: 0,
            repeat: false,
            is_composing: false,
            delta_z: 0.0,
            delta_mode: 0,
            data: None,
            input_type: String::new(),
            prevented: false,
            stopped: false,
            immediate_stopped: false,
            dispatching: false,
        }
    }

    /// `new Event(type)` — the script constructor with no options.
    ///
    /// **All three flags default to FALSE, whatever the type is.** The per-type
    /// table only describes events the USER AGENT fires; a script-constructed
    /// `new Event("click")` does not bubble unless the caller asks for it.
    /// Confirmed against Chrome. Use [`DomEvent::new`] for a UA-fired event and
    /// this for anything a script makes.
    pub fn new_script(event_type: impl Into<String>, target: u32) -> Self {
        Self::new_with_flags(event_type, target, false, false, false)
    }

    /// `new Event(type, { bubbles, cancelable, composed })` — a script-created
    /// event, which is never trusted and takes its flags from the caller
    /// rather than from the type table.
    pub fn new_with_flags(
        event_type: impl Into<String>,
        target: u32,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
    ) -> Self {
        let mut e = Self::new(event_type, target);
        e.bubbles = bubbles;
        e.cancelable = cancelable;
        e.composed = composed;
        e.is_trusted = false;
        e
    }

    /// `preventDefault()` — only does anything on a CANCELABLE event
    /// (DOM §2.3). Calling it on `load` or `mouseenter` is a no-op in a
    /// browser, and treating it as one here keeps `defaultPrevented` honest.
    pub fn prevent_default(&mut self) {
        if self.cancelable {
            self.prevented = true;
        }
    }
    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }
    pub fn stop_immediate_propagation(&mut self) {
        self.stopped = true;
        self.immediate_stopped = true;
    }
    pub fn default_prevented(&self) -> bool {
        self.prevented
    }
    pub fn propagation_stopped(&self) -> bool {
        self.stopped
    }

    /// `Event.bubbles`.
    pub fn bubbles(&self) -> bool {
        self.bubbles
    }
    /// `Event.cancelable`.
    pub fn cancelable(&self) -> bool {
        self.cancelable
    }
    /// `Event.composed`.
    pub fn composed(&self) -> bool {
        self.composed
    }
    /// `Event.isTrusted` — true only for events the user agent generated.
    pub fn is_trusted(&self) -> bool {
        self.is_trusted
    }
    /// `Event.timeStamp` — milliseconds since the engine's time origin.
    pub fn time_stamp(&self) -> f64 {
        self.time_stamp
    }

    /// Is this event mid-dispatch? `dispatchEvent` refuses to re-enter it.
    pub fn is_dispatching(&self) -> bool {
        self.dispatching
    }

    /// `Event.eventPhase` — the numeric constant DOM §2.2 defines.
    pub fn event_phase(&self) -> u16 {
        match self.phase {
            EventPhase::None => 0,
            EventPhase::Capture => 1,
            EventPhase::Target => 2,
            EventPhase::Bubble => 3,
        }
    }

    /// `Event.composedPath()` — target first, root last.
    pub fn composed_path(&self) -> &[u32] {
        &self.composed_path
    }

    /// `Event.srcElement` — the legacy alias for `target`, still shipped.
    pub fn src_element(&self) -> u32 {
        self.target
    }

    /// `Event.cancelBubble` — legacy alias for "propagation stopped".
    pub fn cancel_bubble(&self) -> bool {
        self.stopped
    }
    pub fn set_cancel_bubble(&mut self, v: bool) {
        if v {
            self.stopped = true;
        }
    }

    /// `Event.returnValue` — legacy inverse of `defaultPrevented`.
    pub fn return_value(&self) -> bool {
        !self.prevented
    }
    pub fn set_return_value(&mut self, v: bool) {
        if !v {
            self.prevent_default();
        }
    }

    /// `Event.initEvent(type, bubbles, cancelable)` — legacy initialiser. Does
    /// nothing once the event is being dispatched, per DOM §2.2.
    pub fn init_event(&mut self, event_type: &str, bubbles: bool, cancelable: bool) {
        if self.phase != EventPhase::None {
            return;
        }
        self.event_type = event_type.to_string();
        self.bubbles = bubbles;
        self.cancelable = cancelable;
        self.prevented = false;
        self.stopped = false;
        self.immediate_stopped = false;
        self.is_trusted = false;
    }
}

// ─── Event Handler ──────────────────────────────────────────────────────────

/// A listener callback.
///
/// It receives the event AND the document, because a listener that cannot
/// touch the DOM is not what `addEventListener` means — a browser handler
/// queries, sets attributes, inserts and removes nodes. The engine hands the
/// document over for the duration of the call by moving the listener map out
/// of it first, so there is no aliasing and no interior-mutability trick at the
/// call site.
pub type EventHandler = Box<dyn FnMut(&mut DomEvent, &mut crate::types::Document) + Send + Sync>;

/// The options half of `addEventListener(type, cb, options)` (DOM §2.7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListenerOptions {
    /// Fire during the capture phase instead of the bubble phase.
    pub capture: bool,
    /// Remove the listener after it fires once.
    pub once: bool,
    /// The listener will not call `preventDefault()`; if it does, the call is
    /// ignored. Lets a scroller run without waiting on the handler.
    pub passive: bool,
}

impl ListenerOptions {
    pub fn capture(capture: bool) -> Self {
        Self {
            capture,
            ..Self::default()
        }
    }
}

struct ListenerEntry {
    id: u32,
    event_type: String,
    /// In a cell so dispatch can call it through `&self`: one listener firing
    /// while another is on the stack is ordinary, and each has its own cell.
    handler: std::cell::RefCell<EventHandler>,
    options: ListenerOptions,
    /// Set when `once` has fired, or when an `AbortSignal` aborted it. Swept
    /// after dispatch — a listener cannot be removed mid-dispatch without
    /// disturbing the iteration the spec defines over a SNAPSHOT.
    removed: std::cell::Cell<bool>,
}

// ─── Event Target Map ───────────────────────────────────────────────────────

/// Manages event listeners registered on specific nodes.
pub struct EventTargetMap {
    /// Listeners keyed by node_id.
    listeners: HashMap<u32, Vec<ListenerEntry>>,
    next_id: u32,
    /// The event handler slots — `(node_id, "onclick")` → the listener id it
    /// currently holds. HTML §8.1.7.2 gives each handler ONE slot, so setting
    /// it twice must leave one listener; this is what makes that true.
    handler_slots: HashMap<(u32, String), u32>,
}

impl Default for EventTargetMap {
    fn default() -> Self {
        Self::new()
    }
}

impl EventTargetMap {
    pub fn new() -> Self {
        Self {
            listeners: HashMap::new(),
            next_id: 1,
            handler_slots: HashMap::new(),
        }
    }

    /// Every NODE this map keeps a reference to.
    ///
    /// `listeners` IS keyed by node id, and `handler_slots`' key is
    /// `(node_id, handler_name)` — its VALUE is a listener id and is not a node.
    /// Both halves are here because a node can hold an `onclick` slot with no
    /// entry in `listeners`.
    pub fn node_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.listeners
            .keys()
            .copied()
            .chain(self.handler_slots.keys().map(|(n, _)| *n))
    }

    /// Fold listeners registered DURING a dispatch back in.
    ///
    /// `Document::dispatch_event` moves this map out of the document so a
    /// handler can be given `&mut Document`; anything the handler registers
    /// lands in the document's fresh map and is merged here afterwards.
    /// Without it, `addEventListener` called from inside a handler would be
    /// silently dropped.
    pub fn merge_from(&mut self, other: EventTargetMap) {
        for (node, entries) in other.listeners {
            self.listeners.entry(node).or_default().extend(entries);
        }
        for (key, id) in other.handler_slots {
            self.handler_slots.insert(key, id);
        }
        self.next_id = self.next_id.max(other.next_id);
    }

    /// Register an event listener on a node. Returns a listener ID for removal.
    pub fn add_event_listener(
        &mut self,
        node_id: u32,
        event_type: &str,
        handler: EventHandler,
        capture: bool,
    ) -> u32 {
        self.add_event_listener_with(
            node_id,
            event_type,
            handler,
            ListenerOptions::capture(capture),
        )
    }

    /// `addEventListener(type, callback, options)`.
    ///
    /// Returns a listener id for removal. Note the spec's dedup rule — the same
    /// (type, callback, capture) triple registers ONCE — cannot be enforced
    /// here: two `Box<dyn Fn>` values have no equality, so "the same callback"
    /// is not a question this API can ask. Callers that need it must hold the
    /// id and not register twice.
    pub fn add_event_listener_with(
        &mut self,
        node_id: u32,
        event_type: &str,
        handler: EventHandler,
        options: ListenerOptions,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners
            .entry(node_id)
            .or_default()
            .push(ListenerEntry {
                id,
                event_type: event_type.to_string(),
                handler: std::cell::RefCell::new(handler),
                options,
                removed: std::cell::Cell::new(false),
            });
        id
    }

    /// Remove a listener by its ID.
    pub fn remove_event_listener(&mut self, listener_id: u32) {
        for entries in self.listeners.values_mut() {
            entries.retain(|e| e.id != listener_id);
        }
        // Remove empty entries
        self.listeners.retain(|_, v| !v.is_empty());
        self.handler_slots.retain(|_, id| *id != listener_id);
    }

    /// Remove all listeners on a specific node.
    pub fn remove_all_listeners(&mut self, node_id: u32) {
        self.listeners.remove(&node_id);
        self.handler_slots.retain(|(n, _), _| *n != node_id);
    }

    /// `el.onclick = handler` — set an event handler IDL attribute.
    ///
    /// Replaces whatever the slot held, which is the whole difference between
    /// a handler and `addEventListener`. `handler_name` is an `on*` name;
    /// anything else is rejected, because a browser has no such attribute.
    /// Handlers always run in the BUBBLE phase (HTML §8.1.7.2).
    pub fn set_event_handler(
        &mut self,
        node_id: u32,
        handler_name: &str,
        handler: EventHandler,
    ) -> Option<u32> {
        let event_type = crate::dom::event_handlers::event_type_for_handler(handler_name)?;
        let key = (node_id, handler_name.to_ascii_lowercase());
        if let Some(old_id) = self.handler_slots.remove(&key) {
            self.remove_event_listener(old_id);
        }
        let id =
            self.add_event_listener_with(node_id, &event_type, handler, ListenerOptions::default());
        self.handler_slots.insert(key, id);
        Some(id)
    }

    /// `el.onclick = null` — clear the slot. True if one was set.
    pub fn remove_event_handler(&mut self, node_id: u32, handler_name: &str) -> bool {
        let key = (node_id, handler_name.to_ascii_lowercase());
        match self.handler_slots.remove(&key) {
            Some(id) => {
                self.remove_event_listener(id);
                true
            }
            None => false,
        }
    }

    /// Is this handler slot set? (`el.onclick !== null`.)
    pub fn has_event_handler(&self, node_id: u32, handler_name: &str) -> bool {
        self.handler_slots
            .contains_key(&(node_id, handler_name.to_ascii_lowercase()))
    }

    /// Every handler name currently set on a node.
    pub fn event_handler_names(&self, node_id: u32) -> Vec<String> {
        let mut v: Vec<String> = self
            .handler_slots
            .keys()
            .filter(|(n, _)| *n == node_id)
            .map(|(_, name)| name.clone())
            .collect();
        v.sort();
        v
    }

    /// Check if any listeners are registered.
    pub fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }

    /// Dispatch an event through the arena-based DOM tree with capture → target → bubble.
    /// Returns true if any handler was called.
    pub fn dispatch_event(
        &self,
        arena: &DomArena,
        event: &mut DomEvent,
        doc: &mut crate::types::Document,
    ) -> bool {
        if event.target == 0 {
            return false;
        }
        let path = arena.ancestor_chain(NodeId(event.target));
        let path_root_to_target: Vec<u32> = path.iter().rev().map(|id| id.0).collect();
        self.dispatch_with_path(event, &path_root_to_target, doc)
    }

    /// Dispatch an event through the WebCore tree with capture → target → bubble.
    /// Uses the WebCore tree for the ancestor path (no DomArena needed).
    pub fn dispatch_on_tree(
        &self,
        root: &crate::types::WebCore,
        event: &mut DomEvent,
        doc: &mut crate::types::Document,
    ) -> bool {
        if event.target == 0 {
            return false;
        }
        let path = Self::propagation_path(root, event.target);
        self.dispatch_with_path_retargeting(event, &path, doc)
    }

    /// The propagation path, root-first, with the SHADOW HOST each node sits
    /// under (0 for the light tree).
    ///
    /// The walk descends into shadow roots, which it did not before: a node
    /// inside a shadow tree was unreachable, so an event targeting one found no
    /// path and never dispatched at all.
    pub(crate) fn propagation_path(root: &crate::types::WebCore, target: u32) -> Vec<(u32, u32)> {
        fn walk(
            node: &crate::types::WebCore,
            target: u32,
            host: u32,
            path: &mut Vec<(u32, u32)>,
        ) -> bool {
            path.push((node.node_id, host));
            if node.node_id == target {
                return true;
            }
            // A shadow root's children belong to the SHADOW tree, so everything
            // inside it records this node as its host.
            if let Some(sr) = &node.shadow_root {
                for child in &sr.children {
                    if walk(child, target, node.node_id, path) {
                        return true;
                    }
                }
            }
            for child in &node.children {
                if walk(child, target, host, path) {
                    return true;
                }
            }
            path.pop();
            false
        }
        let mut path = Vec::new();
        walk(root, target, 0, &mut path);
        if path.is_empty() {
            return path;
        }
        // WINDOW and DOCUMENT sit above the document element in the propagation
        // path — that is why `composedPath()` on a click reports six entries in
        // a browser and reported four here, and why
        // `document.addEventListener("click", ..)` never fired: the walk
        // started at `<html>` and neither was ever a current target.
        let mut full = vec![(WINDOW_TARGET, 0u32), (DOCUMENT_TARGET, 0u32)];
        full.extend(path);
        full
    }

    /// Dispatch with shadow retargeting (DOM §2.9).
    ///
    /// A listener OUTSIDE a shadow tree must not see a node inside it as the
    /// target — it sees the HOST. Without that, shadow encapsulation leaks: a
    /// document-level click handler would be handed an internal node it has no
    /// business knowing about. A non-`composed` event does not cross the
    /// boundary at all.
    fn dispatch_with_path_retargeting(
        &self,
        event: &mut DomEvent,
        path: &[(u32, u32)],
        doc: &mut crate::types::Document,
    ) -> bool {
        let target_host = path.last().map(|(_, h)| *h).unwrap_or(0);
        // Not composed and the target is inside a shadow tree: the event stays
        // in that tree, so trim the path to the shadow subtree.
        let visible: Vec<(u32, u32)> = if target_host != 0 && !event.composed {
            path.iter()
                .copied()
                .filter(|(_, h)| *h == target_host)
                .collect()
        } else {
            path.to_vec()
        };
        let ids: Vec<u32> = visible.iter().map(|(id, _)| *id).collect();
        let original_target = event.target;
        // Retargeting is per NODE, so it is applied by `fire_listeners` through
        // this map rather than once for the whole dispatch.
        let retarget: std::collections::HashMap<u32, u32> = visible
            .iter()
            .map(|(id, host)| {
                let seen = if *host == target_host {
                    original_target
                } else if target_host != 0 {
                    target_host
                } else {
                    original_target
                };
                (*id, seen)
            })
            .collect();
        let handled = self.dispatch_with_path_inner(event, &ids, doc, &retarget);
        event.target = original_target;
        handled
    }

    pub(crate) fn dispatch_tree_path(
        &self,
        event: &mut DomEvent,
        path: &[(u32, u32)],
        doc: &mut crate::types::Document,
    ) -> bool {
        self.dispatch_with_path_retargeting(event, path, doc)
    }

    /// Dispatch along a root-to-target path the caller already collected.
    pub fn dispatch_path(
        &self,
        event: &mut DomEvent,
        path_root_to_target: &[u32],
        doc: &mut crate::types::Document,
    ) -> bool {
        if event.target == 0 || path_root_to_target.is_empty() {
            return false;
        }
        self.dispatch_with_path(event, path_root_to_target, doc)
    }

    /// Core dispatch logic — given a root-to-target path, run capture → target → bubble.
    fn dispatch_with_path(
        &self,
        event: &mut DomEvent,
        path_root_to_target: &[u32],
        doc: &mut crate::types::Document,
    ) -> bool {
        self.dispatch_with_path_inner(
            event,
            path_root_to_target,
            doc,
            &std::collections::HashMap::new(),
        )
    }

    fn dispatch_with_path_inner(
        &self,
        event: &mut DomEvent,
        path_root_to_target: &[u32],
        doc: &mut crate::types::Document,
        retarget: &std::collections::HashMap<u32, u32>,
    ) -> bool {
        event.dispatching = true;
        // `composedPath()` is TARGET FIRST, root last — the reverse of the walk
        // order. Available to every listener, in every phase.
        event.composed_path = path_root_to_target.iter().rev().copied().collect();
        // The node the target phase fires on, taken from the PATH.
        //
        // Not from `event.target`: retargeting rewrites that field per listener,
        // so by the time the capture phase has run over the ancestors it holds
        // the shadow HOST, and the target phase would fire on the host instead
        // of on the node the event was dispatched at.
        let target_node = match path_root_to_target.last() {
            Some(id) => *id,
            None => {
                event.dispatching = false;
                return false;
            }
        };

        let mut any_handled = false;

        // ── Phase 1: Capture (root → target, excluding target) ──
        event.phase = EventPhase::Capture;
        for &node_id in &path_root_to_target[..path_root_to_target.len().saturating_sub(1)] {
            event.current_target = node_id;
            if self.fire_listeners(node_id, event, Some(true), doc, retarget) {
                any_handled = true;
            }
            if event.stopped {
                event.dispatching = false;
                return any_handled;
            }
        }

        // ── Phase 2: Target ──
        // The target sits at the END of the capture traversal and the START of
        // the bubble one, so its CAPTURE listeners fire before its BUBBLE
        // listeners regardless of the order they were registered in. Verified
        // against Chrome: a capture listener added second still runs before a
        // bubble listener added first.
        //
        // Both run even when the event does not bubble — `bubbles` gates the
        // ancestor traversal below, not the target itself. That is why a
        // `load` listener on the target fires and one on its parent does not.
        event.phase = EventPhase::Target;
        event.current_target = target_node;
        if self.fire_listeners(target_node, event, Some(true), doc, retarget) {
            any_handled = true;
        }
        if !event.immediate_stopped {
            if self.fire_listeners(target_node, event, Some(false), doc, retarget) {
                any_handled = true;
            }
        }
        if event.stopped {
            event.dispatching = false;
            return any_handled;
        }

        // ── Phase 3: Bubble (target → root, excluding target) ──
        // Only if the event bubbles. `load`, `focus` and `mouseenter` do not,
        // and running this phase for them called listeners a browser never
        // would.
        if event.bubbles {
            event.phase = EventPhase::Bubble;
            let ancestor_count = path_root_to_target.len().saturating_sub(1);
            for i in (0..ancestor_count).rev() {
                let node_id = path_root_to_target[i];
                event.current_target = node_id;
                if self.fire_listeners(node_id, event, Some(false), doc, retarget) {
                    any_handled = true;
                }
                if event.stopped {
                    event.dispatching = false;
                    return any_handled;
                }
            }
        }

        event.phase = EventPhase::None;
        event.current_target = 0;
        event.dispatching = false;
        any_handled
    }

    /// Fire listeners on a node. `capture_phase` of `None` means the target
    /// phase, where the flag does not select and everything fires in order.
    fn fire_listeners(
        &self,
        node_id: u32,
        event: &mut DomEvent,
        capture_phase: Option<bool>,
        doc: &mut crate::types::Document,
        retarget: &std::collections::HashMap<u32, u32>,
    ) -> bool {
        // What this listener is allowed to see as `event.target`.
        if let Some(seen) = retarget.get(&node_id) {
            event.target = *seen;
        }
        let entries = match self.listeners.get(&node_id) {
            Some(e) => e,
            None => return false,
        };
        let mut any = false;
        for entry in entries {
            if entry.removed.get() {
                continue;
            }
            if entry.event_type != event.event_type {
                continue;
            }
            if let Some(want) = capture_phase {
                if entry.options.capture != want {
                    continue;
                }
            }
            // `once` is marked BEFORE the call: a handler that dispatches the
            // same event type re-enters here, and the spec has already removed
            // the listener by then.
            if entry.options.once {
                entry.removed.set(true);
            }
            if entry.options.passive {
                // A passive listener may not cancel the event. Rather than
                // trust it, the flag is restored after the call.
                let was = event.prevented;
                (entry.handler.borrow_mut())(event, doc);
                event.prevented = was;
            } else {
                (entry.handler.borrow_mut())(event, doc);
            }
            any = true;
            if event.immediate_stopped {
                break;
            }
        }
        any
    }

    /// Drop listeners that `once` or an abort retired. Call after dispatch —
    /// removing them during it would disturb the iteration.
    pub fn sweep_removed(&mut self) {
        for entries in self.listeners.values_mut() {
            entries.retain(|e| !e.removed.get());
        }
        self.listeners.retain(|_, v| !v.is_empty());
    }
}

impl std::fmt::Debug for EventTargetMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventTargetMap")
            .field(
                "listener_count",
                &self.listeners.values().map(|v| v.len()).sum::<usize>(),
            )
            .finish()
    }
}

/// Milliseconds since the engine's time origin — what `Event.timeStamp` reports.
///
/// A browser measures from the DOCUMENT's time origin; this measures from the
/// first event constructed in the process, which gives the same thing that
/// matters: a monotonic, comparable stamp. It was hard-coded to 0.0, so every
/// event claimed to have happened at the same instant and no handler could
/// order or rate-limit anything.
fn time_origin_elapsed_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// The `Document` node.
///
/// The tree's root `WebCore` IS the `<html>` element, so there is no node for
/// the document itself — the only `NodeType::Document` in the arena is the dead
/// sentinel in slot 0. A reserved id gives the document an identity the DOM can
/// name without restructuring the tree: `getRootNode()`, `ownerDocument`,
/// `documentElement`, `parentNode` of `<html>` and `nodeType == 9` all need one.
pub const DOCUMENT_TARGET: u32 = u32::MAX - 1;

/// Is this the `Document` rather than a node in the tree?
pub fn is_document_target(id: u32) -> bool {
    id == DOCUMENT_TARGET
}

/// The `Window` event target.
///
/// `Window` is an `EventTarget` but not a node, so it needs an id that no node
/// can ever have. `onload`, `onresize`, `onscroll`, `onpopstate`,
/// `onhashchange` and the rest of `WindowEventHandlers` are registered against
/// this. Without it those 18 handler names resolved and could be set, and
/// nothing ever fired them.
pub const WINDOW_TARGET: u32 = u32::MAX;

/// Is this the `Window` target rather than a node?
pub fn is_window_target(id: u32) -> bool {
    id == WINDOW_TARGET
}

/// `KeyboardEvent.key` for a key that produces no character.
///
/// UI Events §6.3.3 gives every non-printing key a NAME — `Enter`, `ArrowLeft`,
/// `Escape` — and `key` is the member the spec points handlers at. A keycode
/// alone is not an answer any listener can use.
pub fn key_name_for_code(key_code: u32) -> &'static str {
    match key_code {
        8 => "Backspace",
        9 => "Tab",
        13 => "Enter",
        16 => "Shift",
        17 => "Control",
        18 => "Alt",
        19 => "Pause",
        20 => "CapsLock",
        27 => "Escape",
        32 => " ",
        33 => "PageUp",
        34 => "PageDown",
        35 => "End",
        36 => "Home",
        37 => "ArrowLeft",
        38 => "ArrowUp",
        39 => "ArrowRight",
        40 => "ArrowDown",
        45 => "Insert",
        46 => "Delete",
        91 | 93 => "Meta",
        112 => "F1",
        113 => "F2",
        114 => "F3",
        115 => "F4",
        116 => "F5",
        117 => "F6",
        118 => "F7",
        119 => "F8",
        120 => "F9",
        121 => "F10",
        122 => "F11",
        123 => "F12",
        144 => "NumLock",
        145 => "ScrollLock",
        // The spec's value for a key with no other name.
        _ => "Unidentified",
    }
}

// ─── UIEvent / MouseEvent / KeyboardEvent / WheelEvent (UI Events) ──────────
//
// These interfaces live in the UI Events spec rather than DOM or HTML, so they
// are not in the IDL ledger built from the WHATWG specs — but they are the
// members every listener actually reads, and a wrong `buttons` or a missing
// `getModifierState` is as broken as a missing `onclick`.
//
// One flat struct carries them all, because this engine has no interface
// hierarchy to model and a `MouseEvent` is a `DomEvent` with the mouse fields
// filled. What matters is that the ACCESSORS have the spec's names and the
// spec's semantics.

impl DomEvent {
    // ── UIEvent ──

    /// `UIEvent.detail` — for a click, the click count (1 single, 2 double).
    /// Zero for events that do not count.
    pub fn ui_detail(&self) -> i32 {
        self.ui_detail
    }
    pub fn set_ui_detail(&mut self, n: i32) {
        self.ui_detail = n;
    }

    // ── MouseEvent ──

    /// `MouseEvent.clientX` / `clientY` — viewport coordinates.
    pub fn client_x(&self) -> f32 {
        self.client_x
    }
    pub fn client_y(&self) -> f32 {
        self.client_y
    }

    /// `MouseEvent.pageX` / `pageY` — document coordinates: the viewport
    /// position plus how far the page is scrolled.
    pub fn page_x(&self) -> f32 {
        self.client_x + self.scroll_x
    }
    pub fn page_y(&self) -> f32 {
        self.client_y + self.scroll_y
    }

    /// `MouseEvent.screenX` / `screenY`.
    pub fn screen_x(&self) -> f32 {
        self.screen_x
    }
    pub fn screen_y(&self) -> f32 {
        self.screen_y
    }

    /// `MouseEvent.offsetX` / `offsetY` — relative to the target's padding box.
    pub fn offset_x(&self) -> f32 {
        self.client_x - self.target_origin.0
    }
    pub fn offset_y(&self) -> f32 {
        self.client_y - self.target_origin.1
    }

    /// `MouseEvent.button` — which button CHANGED state: 0 left, 1 middle,
    /// 2 right. Meaningless on `mousemove`, where nothing changed.
    pub fn button(&self) -> u8 {
        self.button
    }

    /// `MouseEvent.buttons` — a BITMASK of which buttons are currently held:
    /// 1 left, 2 right, 4 middle. A different question from `button`, and the
    /// one to ask during a drag. Note bit 2 is RIGHT and bit 4 is MIDDLE —
    /// the order is not the same as `button`'s numbering.
    pub fn buttons(&self) -> u8 {
        self.buttons
    }
    pub fn set_buttons(&mut self, mask: u8) {
        self.buttons = mask;
    }

    /// `MouseEvent.relatedTarget` — 0 when there is none.
    pub fn related_target(&self) -> u32 {
        self.related_target
    }

    // ── Modifier keys, shared by mouse and keyboard events ──

    pub fn ctrl_key(&self) -> bool {
        self.ctrl_key
    }
    pub fn shift_key(&self) -> bool {
        self.shift_key
    }
    pub fn alt_key(&self) -> bool {
        self.alt_key
    }
    pub fn meta_key(&self) -> bool {
        self.meta_key
    }

    /// `getModifierState(key)` — UI Events §5.2. Recognises the modifier key
    /// VALUES the spec lists, not arbitrary names; an unknown name is false.
    pub fn get_modifier_state(&self, key: &str) -> bool {
        match key {
            "Control" => self.ctrl_key,
            "Shift" => self.shift_key,
            "Alt" => self.alt_key,
            "Meta" => self.meta_key,
            "AltGraph" | "CapsLock" | "Fn" | "FnLock" | "Hyper" | "NumLock" | "ScrollLock"
            | "Super" | "Symbol" | "SymbolLock" => false,
            _ => false,
        }
    }

    // ── KeyboardEvent ──

    /// `KeyboardEvent.key` — the character or named key produced, e.g. `a`,
    /// `A`, `Enter`, `ArrowLeft`.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// `KeyboardEvent.code` — the PHYSICAL key, e.g. `KeyA`, independent of
    /// layout and of shift. `key` and `code` differ on any non-US layout.
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn set_code(&mut self, code: impl Into<String>) {
        self.code = code.into();
    }

    /// `KeyboardEvent.location` — 0 standard, 1 left, 2 right, 3 numpad.
    pub fn location(&self) -> u32 {
        self.location
    }
    pub fn set_location(&mut self, loc: u32) {
        self.location = loc;
    }

    /// `KeyboardEvent.repeat` — held down and auto-repeating.
    pub fn repeat(&self) -> bool {
        self.repeat
    }
    pub fn set_repeat(&mut self, r: bool) {
        self.repeat = r;
    }

    /// `KeyboardEvent.isComposing` — inside an IME composition session.
    pub fn is_composing(&self) -> bool {
        self.is_composing
    }
    pub fn set_is_composing(&mut self, c: bool) {
        self.is_composing = c;
    }

    // ── WheelEvent ──

    /// `WheelEvent.deltaX` / `deltaY` / `deltaZ`.
    pub fn delta_x(&self) -> f32 {
        self.delta_x
    }
    pub fn delta_y(&self) -> f32 {
        self.delta_y
    }
    pub fn delta_z(&self) -> f32 {
        self.delta_z
    }

    /// `WheelEvent.deltaMode` — 0 pixels, 1 lines, 2 pages.
    pub fn delta_mode(&self) -> u32 {
        self.delta_mode
    }
    pub fn set_delta_mode(&mut self, m: u32) {
        self.delta_mode = m;
    }

    // ── InputEvent ──

    /// `InputEvent.data` — the characters inserted, if any.
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }
    pub fn set_data(&mut self, d: Option<String>) {
        self.data = d;
    }

    /// `InputEvent.inputType` — e.g. `insertText`, `deleteContentBackward`.
    pub fn input_type(&self) -> &str {
        &self.input_type
    }
    pub fn set_input_type(&mut self, t: impl Into<String>) {
        self.input_type = t.into();
    }

    /// Set the target's padding-box origin, so `offsetX`/`offsetY` can be
    /// answered. Called by whatever builds the event from a hit test.
    pub fn set_target_origin(&mut self, x: f32, y: f32) {
        self.target_origin = (x, y);
    }

    /// Set the page scroll offset, so `pageX`/`pageY` can be answered.
    pub fn set_scroll_offset(&mut self, x: f32, y: f32) {
        self.scroll_x = x;
        self.scroll_y = y;
    }

    /// Set `screenX`/`screenY`.
    pub fn set_screen_pos(&mut self, x: f32, y: f32) {
        self.screen_x = x;
        self.screen_y = y;
    }
}

// ─── AbortController / AbortSignal (DOM §3.2) ───────────────────────────────

/// `AbortSignal` — the read side of an `AbortController`.
///
/// Shared by handle rather than by value: `signal.aborted` has to answer true
/// in every holder the moment `abort()` is called, so a copy would be a
/// different signal wearing the same name.
#[derive(Debug, Clone, Default)]
pub struct AbortSignal {
    inner: std::rc::Rc<std::cell::RefCell<AbortState>>,
}

#[derive(Debug, Default)]
struct AbortState {
    aborted: bool,
    reason: String,
    /// Set by `AbortSignal::timeout`.
    deadline: Option<std::time::Instant>,
    /// Set by `AbortSignal::any` — this signal is aborted if any source is.
    sources: Vec<AbortSignal>,
}

impl AbortSignal {
    /// `signal.aborted`.
    pub fn aborted(&self) -> bool {
        if self.inner.borrow().aborted {
            return true;
        }
        if let Some(deadline) = self.inner.borrow().deadline {
            if std::time::Instant::now() >= deadline {
                return true;
            }
        }
        self.inner.borrow().sources.iter().any(|s| s.aborted())
    }

    /// `signal.reason` — empty until aborted. The spec's default reason is an
    /// `AbortError` DOMException; with no exception type here it is the name.
    pub fn reason(&self) -> String {
        let st = self.inner.borrow();
        if st.aborted {
            return st.reason.clone();
        }
        if let Some(deadline) = st.deadline {
            if std::time::Instant::now() >= deadline {
                return "TimeoutError".to_string();
            }
        }
        drop(st);
        for s in &self.inner.borrow().sources {
            if s.aborted() {
                return s.reason();
            }
        }
        String::new()
    }

    /// `signal.throwIfAborted()` — `Err(reason)` when aborted, since this
    /// engine has no exceptions to throw.
    pub fn throw_if_aborted(&self) -> Result<(), String> {
        if self.aborted() {
            Err(self.reason())
        } else {
            Ok(())
        }
    }

    /// `AbortSignal.timeout(ms)` — a signal that aborts after a deadline.
    ///
    /// The deadline is checked when the signal is READ rather than fired by a
    /// timer: this engine has no task queue to schedule on, and a signal whose
    /// only observable effect is `aborted`/`reason` is fully described by
    /// checking the clock at the point someone asks.
    pub fn timeout(ms: u64) -> Self {
        let s = Self::default();
        s.inner.borrow_mut().deadline =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
        s
    }

    /// `AbortSignal.any(signals)` — aborts when any of them has.
    pub fn any(signals: &[AbortSignal]) -> Self {
        let out = Self::default();
        out.inner.borrow_mut().sources = signals.to_vec();
        out
    }

    /// `AbortSignal.abort(reason)` — a signal that is already aborted.
    pub fn already_aborted(reason: &str) -> Self {
        let s = Self::default();
        s.inner.borrow_mut().aborted = true;
        s.inner.borrow_mut().reason = if reason.is_empty() {
            "AbortError".to_string()
        } else {
            reason.to_string()
        };
        s
    }
}

/// `AbortController` — the write side.
#[derive(Debug, Default)]
pub struct AbortController {
    signal: AbortSignal,
}

impl AbortController {
    pub fn new() -> Self {
        Self::default()
    }

    /// `controller.signal`.
    pub fn signal(&self) -> AbortSignal {
        self.signal.clone()
    }

    /// `controller.abort(reason)`. Aborting twice is a no-op — the first
    /// reason stands, per DOM §3.2.
    pub fn abort(&self, reason: &str) {
        let mut st = self.signal.inner.borrow_mut();
        if st.aborted {
            return;
        }
        st.aborted = true;
        st.reason = if reason.is_empty() {
            "AbortError".to_string()
        } else {
            reason.to_string()
        };
    }
}

// ─── CustomEvent (DOM §2.4) ─────────────────────────────────────────────────

impl DomEvent {
    /// `new CustomEvent(type, { detail })`.
    pub fn new_custom(
        event_type: impl Into<String>,
        target: u32,
        detail: impl Into<String>,
    ) -> Self {
        let mut e = Self::new_with_flags(event_type, target, false, false, false);
        e.detail = detail.into();
        e
    }

    /// `CustomEvent.detail`. A string rather than an arbitrary value: this
    /// engine has no script value type to carry.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// `CustomEvent.initCustomEvent(...)` — legacy, and like `initEvent` it
    /// does nothing once dispatch has begun.
    pub fn init_custom_event(
        &mut self,
        event_type: &str,
        bubbles: bool,
        cancelable: bool,
        detail: &str,
    ) {
        if self.phase != EventPhase::None {
            return;
        }
        self.init_event(event_type, bubbles, cancelable);
        self.detail = detail.to_string();
    }
}
