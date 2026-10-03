use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::{Document, Renderer, load_html};

const HTML: &str = include_str!("html/events.html");

const COLS: &[(&str, &str)] = &[
    ("col-backlog", "body-backlog"),
    ("col-todo", "body-todo"),
    ("col-progress", "body-progress"),
    ("col-review", "body-review"),
    ("col-done", "body-done"),
];

struct DragState {
    /// Card id being dragged, empty when idle.
    source_id: String,
    /// Title text of card being dragged.
    source_title: String,
    /// Mouse position when drag was initiated.
    start_pos: (f32, f32),
    /// Whether the drag threshold has been crossed.
    active: bool,
    /// Column body id we are currently hovering over, if any.
    target_body: Option<String>,
}

#[derive(Default)]
struct DomPointerState {
    pressed_card: Option<(String, String)>,
    hovered_body: Option<String>,
    suppress_click: bool,
}

impl DragState {
    fn idle() -> Self {
        Self {
            source_id: String::new(),
            source_title: String::new(),
            start_pos: (0.0, 0.0),
            active: false,
            target_body: None,
        }
    }
    fn has_source(&self) -> bool {
        !self.source_id.is_empty()
    }
}

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    mouse_pos: (f32, f32),
    drag: DragState,
    pointer: Arc<Mutex<DomPointerState>>,
    mouse_down: bool,
    /// Ghost overlay position during drag (logical coords), drawn after display list.
    ghost_pos: Option<(f32, f32)>,
}

fn register_card_press(doc: &mut Document, pointer: Arc<Mutex<DomPointerState>>) {
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "mousedown",
        Box::new(move |evt, doc: &mut Document| {
            if evt.button != 0 {
                return;
            }
            let pressed = doc.closest(evt.target, ".card").and_then(|node_id| {
                let id = doc.get_attribute(node_id, "id")?;
                let title = doc.query_selector(&format!("#{id} .card-title"))
                    .map(|title| doc.text_content(title)).unwrap_or_default();
                Some((id, title))
            });
            pointer.lock().unwrap().pressed_card = pressed;
        }),
        webcore::dom::events::ListenerOptions::default(),
    );
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("events_demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1000u32, 800u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();

        let mut doc = load_html(HTML, self.width);

        let __root = doc.root.node_id;
        register_card_press(&mut doc, self.pointer.clone());
        let pointer = self.pointer.clone();
        doc.add_event_listener(
            __root,
            "mousemove",
            Box::new(move |evt, doc: &mut Document| {
                let body = doc.closest(evt.target, ".column-body").and_then(|node_id| {
                    doc.get_attribute(node_id, "id")
                }).or_else(|| {
                    let node_id = doc.closest(evt.target, ".column")?;
                    let id = doc.get_attribute(node_id, "id")?;
                    id.strip_prefix("col-").map(|suffix| format!("body-{suffix}"))
                });
                pointer.lock().unwrap().hovered_body = body;
            }),
            webcore::dom::events::ListenerOptions::default(),
        );
        let pointer = self.pointer.clone();
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                if pointer.lock().unwrap().suppress_click {
                    return;
                }
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".card") else {
                    return;
                };
                let cur_id = __cur;
                // Deselect all cards first
                deselect_all(__d);
                __d.class_list_add(cur_id, "card-selected");
                let id_str = __d.get_attribute(cur_id, "id").unwrap_or_default();
                let title = __d.query_selector(&format!("#{id_str} .card-title"))
                    .map(|title| __d.text_content(title)).unwrap_or_default();
                if !id_str.is_empty() {
                    if let Some(info) = __d.query_selector("#selected-info") {
                        __d.set_text_content(info, &format!("{} ({})", title, id_str));
                    }
                }
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

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.width = platform.logical_width();
                if let Some(doc) = self.doc.as_mut() {
                    self.renderer.layout_engine().layout(doc, self.width);
                }
                window.request_redraw();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / platform.scale_factor()
                    }
                };
                if let Some(doc) = self.doc.as_mut() {
                    let mp = self.mouse_pos;
                    doc.process_wheel_event((mp.0, mp.1 + doc.scroll_y), dy);
                }
                window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let sf = platform.scale_factor();
                let mx = position.x as f32 / sf;
                let my = position.y as f32 / sf;
                self.mouse_pos = (mx, my);

                if let Some(doc) = self.doc.as_mut() {
                    self.pointer.lock().unwrap().hovered_body = None;
                    let doc_pt = (mx, my + doc.scroll_y);
                    if doc.process_mouse_event(HtmlEventType::MouseMove, doc_pt, 0) {
                        window.request_redraw();
                    }
                }

                if self.mouse_down && self.drag.has_source() {
                    let dx = mx - self.drag.start_pos.0;
                    let dy = my - self.drag.start_pos.1;

                    if !self.drag.active && (dx * dx + dy * dy).sqrt() > 5.0 {
                        // Cross drag threshold — start the drag
                        self.drag.active = true;
                        eprintln!(
                            "[DRAG] threshold crossed, starting drag of {}",
                            self.drag.source_id
                        );
                        if let Some(doc) = self.doc.as_mut() {
                            drag_start(doc, &self.drag.source_id, &self.drag.source_title);
                            doc.style_dirty = true;
                            self.renderer.layout_engine().layout(doc, self.width);
                        }
                    }

                    if self.drag.active {
                        if let Some(doc) = self.doc.as_mut() {
                            let doc_y = my + doc.scroll_y;
                            let new_target = self.pointer.lock().unwrap().hovered_body.clone();

                            if new_target != self.drag.target_body {
                                update_drop_highlights(doc, &new_target);
                                self.drag.target_body = new_target;
                                doc.style_dirty = true;
                                self.renderer.layout_engine().layout(doc, self.width);
                            }

                            // Just store the ghost position — we'll draw it as an
                            // overlay in RedrawRequested, no display list rebuild needed.
                            self.ghost_pos = Some((mx + 12.0, doc_y + 8.0));
                        }
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_pos;
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => {
                        self.mouse_down = true;
                        if let Some(doc) = self.doc.as_mut() {
                            self.pointer.lock().unwrap().pressed_card = None;
                            let doc_pt = (mx, my + doc.scroll_y);
                            doc.process_mouse_event(HtmlEventType::MouseDown, doc_pt, 0);
                            let pressed = self.pointer.lock().unwrap().pressed_card.take();
                            if let Some((id, title)) = pressed {
                                eprintln!("[DRAG] source={} title={}", id, title);
                                self.drag = DragState {
                                    source_id: id,
                                    source_title: title,
                                    start_pos: (mx, my),
                                    active: false,
                                    target_body: None,
                                };
                            }
                            self.renderer.layout_engine().layout(doc, self.width);
                            window.request_redraw();
                        }
                    }

                    (ElementState::Released, MouseButton::Left) => {
                        self.mouse_down = false;
                        eprintln!(
                            "[DRAG] released, active={} has_source={} target={:?}",
                            self.drag.active,
                            self.drag.has_source(),
                            self.drag.target_body
                        );
                        if self.drag.active {
                            // Complete the drop
                            if let Some(doc) = self.doc.as_mut() {
                                self.pointer.lock().unwrap().suppress_click = true;
                                let doc_pt = (mx, my + doc.scroll_y);
                                doc.process_mouse_event(HtmlEventType::MouseUp, doc_pt, 0);
                                self.pointer.lock().unwrap().suppress_click = false;
                                let dropped =
                                    if let Some(ref body_id) = self.drag.target_body.clone() {
                                        eprintln!(
                                            "[DRAG] dropping {} onto {}",
                                            self.drag.source_id, body_id
                                        );
                                        drop_card(doc, &self.drag.source_id, body_id)
                                    } else {
                                        eprintln!("[DRAG] no target body, cancelling");
                                        false
                                    };
                                drag_end(doc, &self.drag.source_id, dropped);
                                doc.style_dirty = true;
                                self.renderer.layout_engine().layout(doc, self.width);
                                window.request_redraw();
                            }
                            self.drag = DragState::idle();
                            self.ghost_pos = None;
                        } else if self.drag.has_source() {
                            self.drag = DragState::idle();
                            self.ghost_pos = None;
                            if let Some(doc) = self.doc.as_mut() {
                                let doc_pt = (mx, my + doc.scroll_y);
                                if doc.process_mouse_event(HtmlEventType::MouseUp, doc_pt, 0) {
                                    doc.style_dirty = true;
                                    self.renderer.layout_engine().layout(doc, self.width);
                                    window.request_redraw();
                                }
                            }
                        } else if let Some(doc) = self.doc.as_mut() {
                            let doc_pt = (mx, my + doc.scroll_y);
                            if doc.process_mouse_event(HtmlEventType::MouseUp, doc_pt, 0) {
                                self.renderer.layout_engine().layout(doc, self.width);
                                window.request_redraw();
                            }
                        }
                    }

                    (state, MouseButton::Right) => {
                        if let Some(doc) = self.doc.as_mut() {
                            let doc_pt = (mx, my + doc.scroll_y);
                            let kind = if state == ElementState::Pressed {
                                HtmlEventType::MouseDown
                            } else {
                                HtmlEventType::MouseUp
                            };
                            let mut changed = doc.process_mouse_event(kind, doc_pt, 2);
                            if state == ElementState::Released {
                                changed |= doc.process_mouse_event(HtmlEventType::ContextMenu, doc_pt, 2);
                            }
                            if changed {
                                self.renderer.layout_engine().layout(doc, self.width);
                                window.request_redraw();
                            }
                        }
                    }

                    _ => {}
                }
            }

            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
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
                        _ => 0,
                    };
                    if kc != 0
                        && doc.process_key_event(
                            HtmlEventType::KeyDown,
                            kc,
                            None,
                            false,
                            false,
                            false,
                            false,
                        )
                    {
                        self.renderer.layout_engine().layout(doc, self.width);
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(doc) = self.doc.as_mut() {
                    let ghost_pos = self.ghost_pos;
                    let renderer = &mut self.renderer;
                    platform.render(|scale, pixmap| {
                        renderer.render(doc, pixmap, scale);
                        // Draw ghost overlay directly on pixmap — no display list rebuild
                        if let Some((gx, gy)) = ghost_pos {
                            if let Some(ghost) = dom::query_selector(&doc.root, "#drag-ghost") {
                                let w = ghost.layout.border_rect.w;
                                let h = ghost.layout.border_rect.h;
                                let sx = doc.scroll_x;
                                let sy = doc.scroll_y;
                                let px = ((gx - sx) * scale) as i32;
                                let py = ((gy - sy) * scale) as i32;
                                let pw = (w * scale) as i32;
                                let ph = (h * scale) as i32;
                                let pix_w = pixmap.width() as i32;
                                let pix_h = pixmap.height() as i32;
                                // Semi-transparent card background
                                let pixels = pixmap.pixels_mut();
                                for dy in 0..ph {
                                    let y = py + dy;
                                    if y < 0 || y >= pix_h {
                                        continue;
                                    }
                                    for dx in 0..pw {
                                        let x = px + dx;
                                        if x < 0 || x >= pix_w {
                                            continue;
                                        }
                                        let idx = (y * pix_w + x) as usize;
                                        if idx < pixels.len() {
                                            // Blend: 80% opacity dark card
                                            let dst = pixels[idx];
                                            let a = 200u32;
                                            let ia = 255 - a;
                                            let r =
                                                (30 * a / 255 + dst.red() as u32 * ia / 255) as u8;
                                            let g = (36 * a / 255 + dst.green() as u32 * ia / 255)
                                                as u8;
                                            let b =
                                                (42 * a / 255 + dst.blue() as u32 * ia / 255) as u8;
                                            let na = (a + dst.alpha() as u32 * ia / 255) as u8;
                                            if let Some(p) =
                                                tiny_skia::PremultipliedColorU8::from_rgba(
                                                    r, g, b, na,
                                                )
                                            {
                                                pixels[idx] = p;
                                            }
                                        }
                                    }
                                }
                                // Draw ghost title text
                                if let Some(title) = dom::query_selector(&doc.root, "#ghost-title")
                                {
                                    let text = dom::get_text_content(title);
                                    if !text.is_empty() {
                                        // Title rendered by the display list at ghost's layout position;
                                        // the overlay rect is enough visual feedback for now.
                                    }
                                }
                            }
                        }
                    });
                }
            }

            _ => {}
        }
    }
}

// ── Drag helpers ──────────────────────────────────────────────────────────────

/// Called when drag threshold is crossed. Shows ghost and banner, marks card as dragging.
fn drag_start(doc: &mut Document, card_id: &str, card_title: &str) {
    // Mark source card as dragging
    if let Some(card) = doc.query_selector(&format!("#{}", card_id)) {
        doc.class_list_add(card, "card-dragging");
    }

    // Show drag banner
    if let Some(banner) = doc.query_selector("#drag-banner") {
        doc.class_list_add(banner, "drag-banner-visible");
    }
    if let Some(el) = doc.query_selector("#drag-title") {
        doc.set_text_content(el, card_title);
    }

    // Show and configure drag ghost
    if let Some(ghost) = doc.query_selector("#drag-ghost") {
        doc.class_list_add(ghost, "drag-ghost-visible");
    }
    if let Some(el) = doc.query_selector("#ghost-title") {
        doc.set_text_content(el, card_title);
    }

    // Show all drop placeholders
    for &(_, body_id) in COLS {
        let sel = format!("#ph-{}", &body_id["body-".len()..]);
        if let Some(ph) = doc.query_selector(&sel) {
            doc.set_style_property(ph, "display", "block");
        }
    }
}

/// Update which column is highlighted as the current drop target.
fn update_drop_highlights(doc: &mut Document, new_target: &Option<String>) {
    for &(col_id, _) in COLS {
        if let Some(col) = doc.query_selector(&format!("#{}", col_id)) {
            doc.class_list_remove(col, "column-drop-active");
        }
    }
    if let Some(body_id) = new_target {
        // Derive col id from body id: "body-backlog" → "col-backlog"
        let col_id = format!("col-{}", &body_id["body-".len()..]);
        if let Some(col) = doc.query_selector(&format!("#{}", col_id)) {
            doc.class_list_add(col, "column-drop-active");
        }
        if let Some(el) = doc.query_selector("#drag-target-col") {
            let col_name = col_id.replacen("col-", "→ ", 1);
            doc.set_text_content(el, &col_name);
        }
    } else {
        if let Some(el) = doc.query_selector("#drag-target-col") {
            doc.set_text_content(el, "");
        }
    }
}

/// Move the card to the target column body. Returns true on success.
fn drop_card(doc: &mut Document, card_id: &str, target_body_id: &str) -> bool {
    let Some(card) = doc.query_selector(&format!("#{card_id}")) else { return false; };
    let Some(source_body) = doc.parent_element(card) else { return false; };
    let Some(target_body) = doc.query_selector(&format!("#{target_body_id}")) else { return false; };
    if source_body == target_body {
        return false;
    }
    doc.append_child(target_body, card);
    doc.class_list_remove(card, "card-dragging");

    true
}

/// Clean up after drag ends (success or cancelled).
fn drag_end(doc: &mut Document, card_id: &str, _dropped: bool) {
    // Remove dragging style from card (in case drop was cancelled)
    if let Some(card) = doc.query_selector(&format!("#{}", card_id)) {
        doc.class_list_remove(card, "card-dragging");
    }

    // Hide ghost and banner
    if let Some(ghost) = doc.query_selector("#drag-ghost") {
        doc.class_list_remove(ghost, "drag-ghost-visible");
    }
    if let Some(banner) = doc.query_selector("#drag-banner") {
        doc.class_list_remove(banner, "drag-banner-visible");
    }

    // Remove all column highlights
    for &(col_id, _) in COLS {
        if let Some(col) = doc.query_selector(&format!("#{}", col_id)) {
            doc.class_list_remove(col, "column-drop-active");
        }
    }

    // Hide all placeholders
    for &(_, body_id) in COLS {
        let ph_suffix = &body_id["body-".len()..];
        if let Some(ph) = doc.query_selector(&format!("#ph-{}", ph_suffix)) {
            doc.set_style_property(ph, "display", "none");
        }
    }
}

/// Deselect all cards.
fn deselect_all(doc: &mut Document) {
    for card in doc.query_selector_all(".card") {
        doc.class_list_remove(card, "card-selected");
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        window: None,
        platform: None,
        renderer: Renderer::new(),
        doc: None,
        width: 1000.0,
        mouse_pos: (0.0, 0.0),
        drag: DragState::idle(),
        pointer: Arc::new(Mutex::new(DomPointerState::default())),
        mouse_down: false,
        ghost_pos: None,
    };
    event_loop.run_app(&mut app).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_press_uses_bubbling_dom_event_target() {
        let mut doc = load_html(HTML, 1000.0);
        webcore::LayoutEngine::new().layout(&mut doc, 1000.0);
        let card = dom::query_selector(&doc.root, "#card-1").unwrap();
        let rect = card.layout.border_rect;
        let point = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let pointer = Arc::new(Mutex::new(DomPointerState::default()));
        register_card_press(&mut doc, pointer.clone());

        doc.process_mouse_event(HtmlEventType::MouseDown, point, 0);

        let pressed = pointer.lock().unwrap().pressed_card.clone();
        assert_eq!(pressed.as_ref().map(|(id, _)| id.as_str()), Some("card-1"));
        assert_eq!(pressed.as_ref().map(|(_, title)| title.as_str()), Some("Update dependencies"));
    }

    #[test]
    fn drop_card_reparents_through_document_append_child() {
        let mut doc = load_html(HTML, 1000.0);
        let card = doc.query_selector("#card-1").unwrap();
        let destination = doc.query_selector("#body-done").unwrap();
        assert!(drop_card(&mut doc, "card-1", "body-done"));
        assert_eq!(doc.parent_element(card), Some(destination));
        assert_eq!(doc.query_selector("#body-done #card-1"), Some(card));
    }
}
