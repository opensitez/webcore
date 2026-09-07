use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::{load_html, Document, LayoutEngine, Renderer, WebCore};

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
    mouse_down: bool,
    /// Ghost overlay position during drag (logical coords), drawn after display list.
    ghost_pos: Option<(f32, f32)>,
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

        // Card selection on click (only fires when not dragging — handled below)
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(|evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".card") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                // Deselect all cards first
                deselect_all(root);
                // Select this card via node_id lookup
                if let Some(target_mut) = dom::find_box_mut(root, cur_id) {
                    dom::add_class(target_mut, "card-selected");
                }
                // Re-lookup for id/title (borrow ended)
                let (title, id_str) = {
                    let target = dom::find_box_mut(root, cur_id);
                    match target {
                        Some(t) => {
                            let title = get_text_of_class(t, "card-title");
                            let id = t.attributes.get("id").cloned().unwrap_or_default();
                            (title, id)
                        }
                        None => return,
                    }
                };
                if !id_str.is_empty() {
                    if let Some(info) = dom::query_selector_mut(root, "#selected-info") {
                        dom::set_text_content(info, &format!("{} ({})", title, id_str));
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
                            let scroll_y = doc.scroll_y;
                            let doc_y = my + scroll_y;

                            // Find which column body we're hovering over
                            let new_target = find_target_body(&doc.root, mx, doc_y);

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
                } else if !self.mouse_down {
                    // Normal hover — let process_mouse_event handle MouseEnter/Leave
                    if let Some(doc) = self.doc.as_mut() {
                        let doc_pt = (mx, my + doc.scroll_y);
                        if doc.process_mouse_event(HtmlEventType::MouseMove, doc_pt, 0) {
                            window.request_redraw();
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_pos;
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Left) => {
                        self.mouse_down = true;
                        if let Some(doc) = self.doc.as_mut() {
                            let doc_pt = (mx, my + doc.scroll_y);
                            // Check if we pressed on a card
                            let card_id = hit_card_id(&doc.root, doc_pt);
                            eprintln!(
                                "[DRAG] pressed at {:?} scroll_y={} card_id={:?}",
                                doc_pt, doc.scroll_y, card_id
                            );
                            if let Some(id) = card_id {
                                let title = {
                                    let card = dom::query_selector(&doc.root, &format!("#{}", id));
                                    card.map(|c| get_text_of_class(c, "card-title"))
                                        .unwrap_or_default()
                                };
                                eprintln!("[DRAG] source={} title={}", id, title);
                                self.drag = DragState {
                                    source_id: id,
                                    source_title: title,
                                    start_pos: (mx, my),
                                    active: false,
                                    target_body: None,
                                };
                            } else {
                                // Clicked outside a card — clear selection
                                if doc.process_mouse_event(HtmlEventType::MouseDown, doc_pt, 0) {
                                    self.renderer.layout_engine().layout(doc, self.width);
                                    window.request_redraw();
                                }
                            }
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
                            // Short press = click — fire click event on the card
                            self.drag = DragState::idle();
                            self.ghost_pos = None;
                            if let Some(doc) = self.doc.as_mut() {
                                let doc_pt = (mx, my + doc.scroll_y);
                                if doc.process_mouse_event(HtmlEventType::Click, doc_pt, 0) {
                                    doc.style_dirty = true;
                                    self.renderer.layout_engine().layout(doc, self.width);
                                    window.request_redraw();
                                }
                            }
                        }
                    }

                    (ElementState::Pressed, MouseButton::Right) => {
                        if let Some(doc) = self.doc.as_mut() {
                            let doc_pt = (mx, my + doc.scroll_y);
                            if doc.process_mouse_event(HtmlEventType::ContextMenu, doc_pt, 2) {
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

/// Find the id of the top-most card under `doc_pt`, if any.
fn hit_card_id(root: &WebCore, doc_pt: (f32, f32)) -> Option<String> {
    use webcore::layout::hit_test::point_to_hit;
    let hit = point_to_hit(root, doc_pt, 0)?;
    fn find_node<'a>(node: &'a WebCore, id: u32) -> Option<&'a WebCore> {
        if node.node_id == id {
            return Some(node);
        }
        for c in &node.children {
            if let Some(f) = find_node(c, id) {
                return Some(f);
            }
        }
        None
    }
    let hit_node = find_node(root, hit.node_id);
    eprintln!(
        "[HIT] node_id={} tag={} class={} rect={:?}",
        hit.node_id,
        hit_node.map(|n| n.tag.as_str()).unwrap_or("?"),
        hit_node
            .and_then(|n| n.attributes.get("class"))
            .map(|s| s.as_str())
            .unwrap_or(""),
        hit_node.map(|n| (
            n.layout.border_rect.x,
            n.layout.border_rect.y,
            n.layout.border_rect.w,
            n.layout.border_rect.h
        )),
    );
    let result = find_card_ancestor(root, hit.node_id);
    eprintln!("[HIT] card_ancestor={:?}", result);
    result
}

/// Recursively search for the node matching `target_id` and return its nearest
/// card ancestor's id. Walks the tree depth-first; when the target is found,
/// propagates a sentinel upward so ancestor nodes can check if they are a card.
fn find_card_ancestor(root: &WebCore, target_id: u32) -> Option<String> {
    /// Returns `Some(card_id)` if target_id is found in this subtree and an
    /// ancestor with class "card" exists. Returns `Some("")` as a sentinel
    /// meaning "found the target but no card ancestor yet".
    fn walk(node: &WebCore, target_id: u32) -> Option<String> {
        if node.node_id == target_id {
            // Found the target — check if this node itself is a card
            if dom::has_class(node, "card") {
                return node.attributes.get("id").cloned().or(Some(String::new()));
            }
            // Signal "found" so ancestors can check themselves
            return Some(String::new());
        }
        for child in &node.children {
            if let Some(id) = walk(child, target_id) {
                if !id.is_empty() {
                    // Already found a card — propagate the id
                    return Some(id);
                }
                // Child subtree contains target but no card yet — check this node
                if dom::has_class(node, "card") {
                    return node.attributes.get("id").cloned().or(Some(String::new()));
                }
                // Keep propagating the sentinel
                return Some(String::new());
            }
        }
        None
    }
    walk(root, target_id).filter(|id| !id.is_empty())
}

/// Find which column body id contains the point `(mx, doc_y)`.
fn find_target_body(root: &WebCore, mx: f32, doc_y: f32) -> Option<String> {
    for &(_, body_id) in COLS {
        if let Some(body) = dom::query_selector(root, &format!("#{}", body_id)) {
            let r = body.layout.border_rect;
            // Use column x-range but full vertical extent of body
            if mx >= r.x && mx < r.x + r.w && doc_y >= r.y && doc_y < r.y + r.h {
                return Some(body_id.to_string());
            }
        }
    }
    // Fallback: match by column header area too (use col-* instead of body-*)
    for &(col_id, body_id) in COLS {
        if let Some(col) = dom::query_selector(root, &format!("#{}", col_id)) {
            let r = col.layout.border_rect;
            if mx >= r.x && mx < r.x + r.w && doc_y >= r.y && doc_y < r.y + r.h {
                return Some(body_id.to_string());
            }
        }
    }
    None
}

/// Called when drag threshold is crossed. Shows ghost and banner, marks card as dragging.
fn drag_start(doc: &mut Document, card_id: &str, card_title: &str) {
    let root = &mut doc.root;

    // Mark source card as dragging
    if let Some(card) = dom::query_selector_mut(root, &format!("#{}", card_id)) {
        dom::add_class(card, "card-dragging");
    }

    // Show drag banner
    if let Some(banner) = dom::query_selector_mut(root, "#drag-banner") {
        dom::add_class(banner, "drag-banner-visible");
    }
    if let Some(el) = dom::query_selector_mut(root, "#drag-title") {
        dom::set_text_content(el, card_title);
    }

    // Show and configure drag ghost
    if let Some(ghost) = dom::query_selector_mut(root, "#drag-ghost") {
        dom::add_class(ghost, "drag-ghost-visible");
    }
    if let Some(el) = dom::query_selector_mut(root, "#ghost-title") {
        dom::set_text_content(el, card_title);
    }

    // Show all drop placeholders
    for &(_, body_id) in COLS {
        let sel = format!("#ph-{}", &body_id["body-".len()..]);
        if let Some(ph) = dom::query_selector_mut(root, &sel) {
            dom::set_style_property(ph, "display", "block");
        }
    }
}

/// Update which column is highlighted as the current drop target.
fn update_drop_highlights(doc: &mut Document, new_target: &Option<String>) {
    let root = &mut doc.root;
    for &(col_id, _) in COLS {
        if let Some(col) = dom::query_selector_mut(root, &format!("#{}", col_id)) {
            dom::remove_class(col, "column-drop-active");
        }
    }
    if let Some(body_id) = new_target {
        // Derive col id from body id: "body-backlog" → "col-backlog"
        let col_id = format!("col-{}", &body_id["body-".len()..]);
        if let Some(col) = dom::query_selector_mut(root, &format!("#{}", col_id)) {
            dom::add_class(col, "column-drop-active");
        }
        if let Some(el) = dom::query_selector_mut(root, "#drag-target-col") {
            let col_name = col_id.replacen("col-", "→ ", 1);
            dom::set_text_content(el, &col_name);
        }
    } else {
        if let Some(el) = dom::query_selector_mut(root, "#drag-target-col") {
            dom::set_text_content(el, "");
        }
    }
}

/// Move the card to the target column body. Returns true on success.
fn drop_card(doc: &mut Document, card_id: &str, target_body_id: &str) -> bool {
    let root = &mut doc.root;

    // Find which body currently contains the card
    let src_body_id = {
        let mut found: Option<String> = None;
        for &(_, body_id) in COLS {
            if let Some(body) = dom::query_selector(root, &format!("#{}", body_id)) {
                if body
                    .children
                    .iter()
                    .any(|c| c.attributes.get("id").map(|s| s.as_str()) == Some(card_id))
                {
                    found = Some(body_id.to_string());
                    break;
                }
            }
        }
        match found {
            Some(id) => id,
            None => return false,
        }
    };

    // Don't drop onto the same column
    if src_body_id == target_body_id {
        return false;
    }

    // Remove card from source body by node_id
    let card = {
        let src_body = match dom::query_selector_mut(root, &format!("#{}", src_body_id)) {
            Some(b) => b,
            None => return false,
        };
        let card_node_id = match src_body
            .children
            .iter()
            .find(|c| c.attributes.get("id").map(|s| s.as_str()) == Some(card_id))
            .map(|c| c.node_id)
        {
            Some(id) => id,
            None => return false,
        };
        match dom::remove_child(src_body, card_node_id) {
            Some(c) => c,
            None => return false,
        }
    };

    // Append to target body after the placeholder (first child)
    match dom::query_selector_mut(root, &format!("#{}", target_body_id)) {
        Some(target_body) => {
            let placeholder_id = target_body.children.first().map(|c| c.node_id);
            if let Some(ph_id) = placeholder_id {
                dom::insert_after(target_body, ph_id, card);
            } else {
                dom::append_child(target_body, card);
            }
        }
        None => return false,
    }

    // Remove card-dragging class (card is now in new location)
    if let Some(card) = dom::query_selector_mut(root, &format!("#{}", card_id)) {
        dom::remove_class(card, "card-dragging");
    }

    true
}

/// Clean up after drag ends (success or cancelled).
fn drag_end(doc: &mut Document, card_id: &str, _dropped: bool) {
    let root = &mut doc.root;

    // Remove dragging style from card (in case drop was cancelled)
    if let Some(card) = dom::query_selector_mut(root, &format!("#{}", card_id)) {
        dom::remove_class(card, "card-dragging");
    }

    // Hide ghost and banner
    if let Some(ghost) = dom::query_selector_mut(root, "#drag-ghost") {
        dom::remove_class(ghost, "drag-ghost-visible");
    }
    if let Some(banner) = dom::query_selector_mut(root, "#drag-banner") {
        dom::remove_class(banner, "drag-banner-visible");
    }

    // Remove all column highlights
    for &(col_id, _) in COLS {
        if let Some(col) = dom::query_selector_mut(root, &format!("#{}", col_id)) {
            dom::remove_class(col, "column-drop-active");
        }
    }

    // Hide all placeholders
    for &(_, body_id) in COLS {
        let ph_suffix = &body_id["body-".len()..];
        if let Some(ph) = dom::query_selector_mut(root, &format!("#ph-{}", ph_suffix)) {
            dom::set_style_property(ph, "display", "none");
        }
    }
}

/// Deselect all cards.
fn deselect_all(root: &mut WebCore) {
    fn walk(node: &mut WebCore) {
        if dom::has_class(node, "card") {
            dom::remove_class(node, "card-selected");
        }
        for child in node.children.iter_mut() {
            walk(child);
        }
    }
    walk(root);
}

/// Get the text content of the first descendant with `class_name`.
fn get_text_of_class<'a>(node: &'a WebCore, class_name: &str) -> String {
    fn walk<'a>(node: &'a WebCore, class_name: &str) -> Option<&'a WebCore> {
        if dom::has_class(node, class_name) {
            return Some(node);
        }
        for child in &node.children {
            if let Some(b) = walk(child, class_name) {
                return Some(b);
            }
        }
        None
    }
    walk(node, class_name)
        .map(|b| dom::get_text_content(b))
        .unwrap_or_default()
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
        mouse_down: false,
        ghost_pos: None,
    };
    event_loop.run_app(&mut app).unwrap();
}
