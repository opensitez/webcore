use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::WebCore;
use webcore::{load_html, Document, LayoutEngine, Renderer};

const HTML: &str = include_str!("html/event_playground.html");

fn find_node(node: &WebCore, id: u32) -> Option<&WebCore> {
    if node.node_id == id {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, id) {
            return Some(found);
        }
    }
    None
}

// ── Shared event log buffer ────────────────────────────────────────────────────

/// Category for color-coding log entries
#[derive(Clone, Copy)]
enum EvCat {
    Mouse,
    Pointer,
    Focus,
    Key,
    Wheel,
    Drag,
    Life,
}

impl EvCat {
    fn css_class(self) -> &'static str {
        match self {
            EvCat::Mouse => "log-tag-mouse",
            EvCat::Pointer => "log-tag-pointer",
            EvCat::Focus => "log-tag-focus",
            EvCat::Key => "log-tag-key",
            EvCat::Wheel => "log-tag-wheel",
            EvCat::Drag => "log-tag-drag",
            EvCat::Life => "log-tag-life",
        }
    }
    fn stat_id(self) -> &'static str {
        match self {
            EvCat::Mouse => "stat-mouse",
            EvCat::Pointer => "stat-pointer",
            EvCat::Focus => "stat-focus",
            EvCat::Key => "stat-key",
            EvCat::Wheel => "stat-wheel",
            EvCat::Drag => "stat-drag",
            EvCat::Life => "stat-life",
        }
    }
}

struct LogEntry {
    cat: EvCat,
    tag: String,
    body: String,
}

struct SharedState {
    log: VecDeque<LogEntry>,
    counts: [u32; 7], // mouse, pointer, focus, key, wheel, drag, life
    dirty: bool,
    // extra state for zones
    wheel_count: u32,
    /// Exponentially-decayed accumulator for the bar. Decays toward 0 each event.
    wheel_pos: f32,
}

impl SharedState {
    fn new() -> Self {
        Self {
            log: VecDeque::with_capacity(21),
            counts: [0u32; 7],
            dirty: false,
            wheel_count: 0,
            wheel_pos: 0.0,
        }
    }

    fn push(&mut self, cat: EvCat, tag: &str, body: &str) {
        if self.log.len() >= 20 {
            self.log.pop_back();
        }
        self.log.push_front(LogEntry {
            cat,
            tag: tag.to_string(),
            body: body.to_string(),
        });
        let idx = match cat {
            EvCat::Mouse => 0,
            EvCat::Pointer => 1,
            EvCat::Focus => 2,
            EvCat::Key => 3,
            EvCat::Wheel => 4,
            EvCat::Drag => 5,
            EvCat::Life => 6,
        };
        self.counts[idx] += 1;
        self.dirty = true;
    }
}

type Shared = Arc<Mutex<SharedState>>;

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    mouse_pos: (f32, f32),
    shared: Shared,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Event Playground — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1200u32, 800u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();

        let mut doc = load_html(HTML, self.width);

        // ── Register all event listeners ──────────────────────────────────────

        let shared = self.shared.clone();

        // --- MouseOver ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "mouseover",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".hover-box") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::add_class(t, "hover-box-active");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                if let Some(el) = dom::query_selector_mut(root, "#hover-status") {
                    dom::set_text_content(el, &format!("MouseOver: #{}", id));
                }
                s.lock().unwrap().push(
                    EvCat::Mouse,
                    "MouseOver",
                    &format!(
                        "#{} at ({:.0},{:.0})",
                        id,
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- MouseOut ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "mouseout",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".hover-box") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::remove_class(t, "hover-box-active");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                if let Some(el) = dom::query_selector_mut(root, "#hover-status") {
                    dom::set_text_content(el, &format!("MouseOut: #{}", id));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Mouse, "MouseOut", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- MouseEnter ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "mouseenter",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".hover-box") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = find_node(root, cur_id)
                    .and_then(|t| t.attributes.get("id").cloned())
                    .unwrap_or_default();
                s.lock()
                    .unwrap()
                    .push(EvCat::Mouse, "MouseEnter", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- MouseLeave ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "mouseleave",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".hover-box") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = find_node(root, cur_id)
                    .and_then(|t| t.attributes.get("id").cloned())
                    .unwrap_or_default();
                s.lock()
                    .unwrap()
                    .push(EvCat::Mouse, "MouseLeave", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- MouseMove (on hover zone) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "mousemove",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#zone-hover") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                s.lock().unwrap().push(
                    EvCat::Mouse,
                    "MouseMove",
                    &format!(
                        "({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Click ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-click") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#click-status") {
                    dom::set_text_content(el, "Click fired!");
                }
                s.lock().unwrap().push(EvCat::Mouse, "Click", "#btn-click");
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- DblClick ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "dblclick",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-dblclick") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#click-status") {
                    dom::set_text_content(el, "DblClick fired!");
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Mouse, "DblClick", "#btn-dblclick");
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- ContextMenu ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "contextmenu",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-ctx") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#click-status") {
                    dom::set_text_content(el, "ContextMenu fired!");
                }
                s.lock().unwrap().push(
                    EvCat::Mouse,
                    "ContextMenu",
                    &format!(
                        "#btn-ctx at ({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- DragStart (on drag cards) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "dragstart",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".drag-card") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::add_class(t, "drag-card-active");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                if let Some(el) = dom::query_selector_mut(root, "#drag-status") {
                    dom::set_text_content(el, &format!("Dragging #{}", id));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Drag, "DragStart", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Drag ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "drag",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".drag-card") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = find_node(root, cur_id)
                    .and_then(|t| t.attributes.get("id").cloned())
                    .unwrap_or_default();
                s.lock().unwrap().push(
                    EvCat::Drag,
                    "Drag",
                    &format!(
                        "#{} at ({:.0},{:.0})",
                        id,
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- DragEnd ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "dragend",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".drag-card") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::remove_class(t, "drag-card-active");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                if let Some(el) = dom::query_selector_mut(root, "#drag-status") {
                    dom::set_text_content(el, &format!("DragEnd #{}", id));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Drag, "DragEnd", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- KeyDown ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "keydown",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "body") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let kc = evt.key_code;
                let key_name = key_code_name(kc);
                if let Some(el) = dom::query_selector_mut(root, "#key-display") {
                    dom::set_text_content(el, &format!("Key: {} (code {})", key_name, kc));
                    dom::add_class(el, "key-display-active");
                }
                if let Some(el) = dom::query_selector_mut(root, "#key-status") {
                    dom::set_text_content(el, &format!("KeyDown: {}", key_name));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Key, "KeyDown", &format!("{} ({})", key_name, kc));
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- KeyUp ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "keyup",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "body") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let kc = evt.key_code;
                let key_name = key_code_name(kc);
                if let Some(el) = dom::query_selector_mut(root, "#key-display") {
                    dom::remove_class(el, "key-display-active");
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Key, "KeyUp", &format!("{} ({})", key_name, kc));
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Wheel (on wheel zone) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "wheel",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#zone-wheel") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let dx = evt.delta_x;
                let dy = evt.delta_y;
                let mut st = s.lock().unwrap();
                st.wheel_count += 1;
                let count = st.wheel_count;
                // Exponential decay: blends new delta in, old value fades out each event.
                // Caps per-event contribution so momentum bursts don't instantly saturate.
                st.wheel_pos = st.wheel_pos * 0.75 + dy.clamp(-60.0, 60.0) * 0.5;
                let pos = st.wheel_pos;
                st.push(EvCat::Wheel, "Wheel", &format!("dx={:.1} dy={:.1}", dx, dy));
                drop(st);
                if let Some(el) = dom::query_selector_mut(root, "#wheel-count") {
                    dom::set_text_content(el, &count.to_string());
                }
                if let Some(el) = dom::query_selector_mut(root, "#wheel-delta-label") {
                    dom::set_text_content(el, &format!("dx={:.1}  dy={:.1}", dx, dy));
                }
                // Bar centred at 50%: scroll down → right, scroll up → left.
                // Decays back to centre when scrolling stops.
                let bar_pct = (50.0 + pos.clamp(-50.0, 50.0)).clamp(0.0, 100.0) as u32;
                if let Some(el) = dom::query_selector_mut(root, "#wheel-bar") {
                    dom::set_style_property(el, "width", &format!("{}%", bar_pct));
                }
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Focus (on focus items) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "focus",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".focus-item") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::add_class(t, "focus-item-focused");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                let dot_id = format!("#{}", id.replace("focus-item", "focus-dot"));
                if let Some(dot) = dom::query_selector_mut(root, &dot_id) {
                    dom::add_class(dot, "focus-dot-on");
                }
                if let Some(el) = dom::query_selector_mut(root, "#focus-status") {
                    dom::set_text_content(el, &format!("Focus: #{}", id));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Focus, "Focus", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Blur (on focus items) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "blur",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".focus-item") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        dom::remove_class(t, "focus-item-focused");
                        t.attributes.get("id").cloned().unwrap_or_default()
                    })
                    .unwrap_or_default();
                let dot_id = format!("#{}", id.replace("focus-item", "focus-dot"));
                if let Some(dot) = dom::query_selector_mut(root, &dot_id) {
                    dom::remove_class(dot, "focus-dot-on");
                }
                if let Some(el) = dom::query_selector_mut(root, "#focus-status") {
                    dom::set_text_content(el, &format!("Blur: #{}", id));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Focus, "Blur", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- FocusIn (on focus items) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "focusin",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".focus-item") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = find_node(root, cur_id)
                    .and_then(|t| t.attributes.get("id").cloned())
                    .unwrap_or_default();
                s.lock()
                    .unwrap()
                    .push(EvCat::Focus, "FocusIn", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- FocusOut (on focus items) ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "focusout",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".focus-item") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let id = find_node(root, cur_id)
                    .and_then(|t| t.attributes.get("id").cloned())
                    .unwrap_or_default();
                s.lock()
                    .unwrap()
                    .push(EvCat::Focus, "FocusOut", &format!("#{}", id));
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- PointerDown ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "pointerdown",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#pointer-canvas") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#pointer-status") {
                    dom::set_text_content(
                        el,
                        &format!(
                            "PointerDown at ({:.0},{:.0})",
                            (evt.client_x, evt.client_y).0,
                            (evt.client_x, evt.client_y).1
                        ),
                    );
                }
                s.lock().unwrap().push(
                    EvCat::Pointer,
                    "PointerDown",
                    &format!(
                        "({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- PointerUp ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "pointerup",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#pointer-canvas") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#pointer-status") {
                    dom::set_text_content(
                        el,
                        &format!(
                            "PointerUp at ({:.0},{:.0})",
                            (evt.client_x, evt.client_y).0,
                            (evt.client_x, evt.client_y).1
                        ),
                    );
                }
                s.lock().unwrap().push(
                    EvCat::Pointer,
                    "PointerUp",
                    &format!(
                        "({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- PointerMove ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "pointermove",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#pointer-canvas") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cx = (evt.client_x, evt.client_y).0;
                let cy = (evt.client_x, evt.client_y).1;
                // Find canvas rect first, then update dot
                let (canvas_x, canvas_y) = {
                    let canvas = dom::query_selector_mut(root, "#pointer-canvas");
                    canvas
                        .map(|c| (c.layout.border_rect.x, c.layout.border_rect.y))
                        .unwrap_or((0.0, 0.0))
                };
                let rel_x = (cx - canvas_x - 7.0).max(0.0);
                let rel_y = (cy - canvas_y - 7.0).max(0.0);
                if let Some(dot) = dom::query_selector_mut(root, "#pointer-dot") {
                    dom::set_style_property(dot, "left", &format!("{}px", rel_x as u32));
                    dom::set_style_property(dot, "top", &format!("{}px", rel_y as u32));
                }
                if let Some(el) = dom::query_selector_mut(root, "#pointer-status") {
                    dom::set_text_content(el, &format!("PointerMove ({:.0},{:.0})", cx, cy));
                }
                s.lock().unwrap().push(
                    EvCat::Pointer,
                    "PointerMove",
                    &format!("({:.0},{:.0})", cx, cy),
                );
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- PointerOver ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "pointerover",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#zone-pointer") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                s.lock().unwrap().push(
                    EvCat::Pointer,
                    "PointerOver",
                    &format!(
                        "({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- PointerOut ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "pointerout",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#zone-pointer") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                s.lock().unwrap().push(
                    EvCat::Pointer,
                    "PointerOut",
                    &format!(
                        "({:.0},{:.0})",
                        (evt.client_x, evt.client_y).0,
                        (evt.client_x, evt.client_y).1
                    ),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- Resize ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "resize",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "body") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let w = (evt.client_x, evt.client_y).0 as u32;
                let h = (evt.client_x, evt.client_y).1 as u32;
                if let Some(el) = dom::query_selector_mut(root, "#viewport-size") {
                    dom::set_text_content(el, &format!("{}x{}", w, h));
                }
                s.lock()
                    .unwrap()
                    .push(EvCat::Life, "Resize", &format!("{}x{}", w, h));
                let _ = root;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // --- DOMContentLoaded ---
        let s = shared.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "DOMContentLoaded",
            Box::new(move |_evt, _d: &mut webcore::Document| {
                s.lock()
                    .unwrap()
                    .push(EvCat::Life, "DOMContentLoaded", "document ready");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        self.doc = Some(doc);
        self.window = Some(window);
        self.platform = Some(platform);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (window, platform) = match (self.window.as_ref(), self.platform.as_mut()) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };

        // Let the renderer handle built-in events (zoom, scroll, hover, etc.)
        self.renderer.handle_window_event(&event, self.doc.as_mut());

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.width = platform.logical_width();
                if let Some(doc) = self.doc.as_mut() {
                    LayoutEngine::new().layout(doc, self.width);
                    // Update viewport display
                    let w = size.width;
                    let h = size.height;
                    if let Some(el) = dom::query_selector_mut(&mut doc.root, "#viewport-size") {
                        dom::set_text_content(el, &format!("{}x{}", w, h));
                    }
                    self.shared.lock().unwrap().push(
                        EvCat::Life,
                        "Resize",
                        &format!("{}x{}", w, h),
                    );
                }
                window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                // handle_window_event already dispatched MouseMove + hover.
                // Just track mouse_pos for scrollbar hit-testing.
                let sf = platform.scale_factor();
                self.mouse_pos = (position.x as f32 / sf, position.y as f32 / sf);
                if let Some(doc) = self.doc.as_mut() {
                    let (mx, my) = self.mouse_pos;
                    let (sx, sy) = (mx, my);
                    if doc.process_scrollbar_event(
                        HtmlEventType::MouseMove,
                        sx,
                        sy,
                        self.width,
                        platform.logical_height(),
                    ) {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // handle_window_event already dispatched Wheel event.
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / platform.scale_factor()
                    }
                };
                if let Some(doc) = self.doc.as_mut() {
                    let mp = self.mouse_pos;
                    let doc_pt = (mp.0, mp.1 + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                window.request_redraw();
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // handle_window_event already dispatched MouseDown/MouseUp/Click/PointerDown/PointerUp.
                // We only need to handle scrollbars and ContextMenu here.
                let (mx, my) = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mx, my + doc.scroll_y);
                    match (state, button) {
                        (ElementState::Pressed, MouseButton::Left) => {
                            let sb = doc.process_scrollbar_event(
                                HtmlEventType::MouseDown,
                                mx,
                                my,
                                self.width,
                                platform.logical_height(),
                            );
                            if sb {
                                window.request_redraw();
                            }
                        }
                        (ElementState::Released, MouseButton::Left) => {
                            doc.process_scrollbar_event(
                                HtmlEventType::MouseUp,
                                mx,
                                my,
                                self.width,
                                platform.logical_height(),
                            );
                            window.request_redraw();
                        }
                        (ElementState::Pressed, MouseButton::Right) => {
                            doc.process_mouse_event(HtmlEventType::ContextMenu, doc_pt, 2);
                            window.request_redraw();
                        }
                        _ => {}
                    }
                    LayoutEngine::new().layout(doc, self.width);
                    window.request_redraw();
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    let kc = match code {
                        KeyCode::Escape => 27,
                        KeyCode::Enter => 13,
                        KeyCode::Tab => 9,
                        KeyCode::Delete => 46,
                        KeyCode::Backspace => 8,
                        KeyCode::ArrowUp => 38,
                        KeyCode::ArrowDown => 40,
                        KeyCode::ArrowLeft => 37,
                        KeyCode::ArrowRight => 39,
                        KeyCode::Space => 32,
                        KeyCode::KeyA => 65,
                        KeyCode::KeyB => 66,
                        KeyCode::KeyC => 67,
                        KeyCode::KeyD => 68,
                        KeyCode::KeyE => 69,
                        KeyCode::KeyF => 70,
                        KeyCode::KeyG => 71,
                        KeyCode::KeyH => 72,
                        KeyCode::KeyI => 73,
                        KeyCode::KeyJ => 74,
                        KeyCode::KeyK => 75,
                        KeyCode::KeyL => 76,
                        KeyCode::KeyM => 77,
                        KeyCode::KeyN => 78,
                        KeyCode::KeyO => 79,
                        KeyCode::KeyP => 80,
                        KeyCode::KeyQ => 81,
                        KeyCode::KeyR => 82,
                        KeyCode::KeyS => 83,
                        KeyCode::KeyT => 84,
                        KeyCode::KeyU => 85,
                        KeyCode::KeyV => 86,
                        KeyCode::KeyW => 87,
                        KeyCode::KeyX => 88,
                        KeyCode::KeyY => 89,
                        KeyCode::KeyZ => 90,
                        KeyCode::Digit0 => 48,
                        KeyCode::Digit1 => 49,
                        KeyCode::Digit2 => 50,
                        KeyCode::Digit3 => 51,
                        KeyCode::Digit4 => 52,
                        KeyCode::Digit5 => 53,
                        KeyCode::Digit6 => 54,
                        KeyCode::Digit7 => 55,
                        KeyCode::Digit8 => 56,
                        KeyCode::Digit9 => 57,
                        _ => 0,
                    };
                    if kc != 0 {
                        let etype = match state {
                            ElementState::Pressed => HtmlEventType::KeyDown,
                            ElementState::Released => HtmlEventType::KeyUp,
                        };
                        if doc.process_key_event(etype, kc, None, false, false, false, false) {
                            LayoutEngine::new().layout(doc, self.width);
                            window.request_redraw();
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                // Drain the shared log into the DOM before rendering
                {
                    let width = self.width;
                    if let Some(doc) = self.doc.as_mut() {
                        let mut st = self.shared.lock().unwrap();
                        if st.dirty {
                            st.dirty = false;
                            let entries: Vec<(EvCat, String, String)> = st
                                .log
                                .iter()
                                .map(|e| (e.cat, e.tag.clone(), e.body.clone()))
                                .collect();
                            let counts = st.counts;
                            drop(st);

                            let root = &mut doc.root;
                            for (i, (cat, tag, body)) in entries.iter().enumerate() {
                                let sel = format!("#log-{}", i);
                                if let Some(el) = dom::query_selector_mut(root, &sel) {
                                    dom::set_text_content(el, &format!("[{}] {}", tag, body));
                                    dom::remove_class(el, "log-tag-mouse");
                                    dom::remove_class(el, "log-tag-pointer");
                                    dom::remove_class(el, "log-tag-focus");
                                    dom::remove_class(el, "log-tag-key");
                                    dom::remove_class(el, "log-tag-wheel");
                                    dom::remove_class(el, "log-tag-drag");
                                    dom::remove_class(el, "log-tag-life");
                                    dom::add_class(el, cat.css_class());
                                }
                            }
                            for i in entries.len()..20 {
                                let sel = format!("#log-{}", i);
                                if let Some(el) = dom::query_selector_mut(root, &sel) {
                                    dom::set_text_content(el, "...");
                                }
                            }
                            let stat_ids = [
                                ("stat-mouse", 0usize),
                                ("stat-pointer", 1),
                                ("stat-focus", 2),
                                ("stat-key", 3),
                                ("stat-wheel", 4),
                                ("stat-drag", 5),
                                ("stat-life", 6),
                            ];
                            for (id, idx) in &stat_ids {
                                let sel = format!("#{}", id);
                                if let Some(el) = dom::query_selector_mut(root, &sel) {
                                    dom::set_text_content(el, &counts[*idx].to_string());
                                }
                            }
                            let total: u32 = counts.iter().sum();
                            if let Some(el) = dom::query_selector_mut(root, "#stat-total") {
                                dom::set_text_content(el, &total.to_string());
                            }
                            LayoutEngine::new().layout(doc, width);
                        }
                    }
                }

                if let Some(doc) = self.doc.as_mut() {
                    let renderer = &mut self.renderer;
                    platform.render(|scale, pixmap| {
                        renderer.render(doc, pixmap, scale);
                    });
                }
            }

            _ => {}
        }

        // If the shared state is dirty, request a redraw to flush log to DOM
        if self.shared.lock().unwrap().dirty {
            window.request_redraw();
        }
    }
}

// ── Key code name helper ───────────────────────────────────────────────────────

fn key_code_name(kc: u32) -> String {
    match kc {
        8 => "Backspace".to_string(),
        9 => "Tab".to_string(),
        13 => "Enter".to_string(),
        27 => "Escape".to_string(),
        32 => "Space".to_string(),
        37 => "ArrowLeft".to_string(),
        38 => "ArrowUp".to_string(),
        39 => "ArrowRight".to_string(),
        40 => "ArrowDown".to_string(),
        46 => "Delete".to_string(),
        48..=57 => format!("{}", (kc - 48) as u8 as char),
        65..=90 => format!("{}", (kc as u8) as char),
        _ => format!("Key({})", kc),
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    let shared = Arc::new(Mutex::new(SharedState::new()));

    let mut app = App {
        window: None,
        platform: None,
        renderer: Renderer::new(),
        doc: None,
        width: 1200.0,
        mouse_pos: (0.0, 0.0),
        shared,
    };
    event_loop.run_app(&mut app).unwrap();
}
