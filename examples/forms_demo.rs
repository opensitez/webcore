//! forms_demo — Interactive pizza ordering app using webcore.
//!
//! Uses the same event system as graph_demo and event_playground:
//! - `renderer.handle_window_event()` for all input routing
//! - `doc.add_event_listener()` for click handlers
//! - `dom::set_text_content()` / `dom::set_attribute()` to update DOM
//!
//! Usage:
//!   cargo run --example forms_demo

use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::WebCore;
use webcore::{load_html, LayoutEngine, Renderer};

const HTML: &str = include_str!("html/forms_demo.html");

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<webcore::Document>,
    mouse: (f32, f32),
    width: f32,
}

fn update_summary(root: &mut WebCore) {
    // Read form values and update the summary section
    let name = dom::query_selector(root, "#name")
        .and_then(|n| n.attributes.get("value").cloned())
        .unwrap_or_default();
    let phone = dom::query_selector(root, "#phone")
        .and_then(|n| n.attributes.get("value").cloned())
        .unwrap_or_default();

    // Size from radio
    let size = ["small", "medium", "large"]
        .iter()
        .find(|s| {
            dom::query_selector(root, &format!("input[value={}]", s))
                .map(|n| n.attributes.contains_key("checked"))
                .unwrap_or(false)
        })
        .unwrap_or(&"medium");
    let price = match *size {
        "small" => 9.99f32,
        "large" => 19.99,
        _ => 14.99,
    };

    // Toppings
    let mut toppings = Vec::new();
    let mut topping_price = 0.0f32;
    for id in &[
        "pep", "mush", "onion", "saus", "pepper", "olive", "cheese", "jala",
    ] {
        let sel = format!("#{}", id);
        if let Some(n) = dom::query_selector(root, &sel) {
            if n.attributes.contains_key("checked") {
                if let Some(label) = n.attributes.get("data-label") {
                    toppings.push(label.clone());
                }
                if let Some(p) = n.attributes.get("data-price") {
                    topping_price += p.parse::<f32>().unwrap_or(0.0);
                }
            }
        }
    }

    let total = price + topping_price;
    let topping_str = if toppings.is_empty() {
        "None".into()
    } else {
        toppings.join(", ")
    };

    if let Some(el) = dom::query_selector_mut(root, "#sum-name") {
        dom::set_text_content(el, if name.is_empty() { "—" } else { &name });
    }
    if let Some(el) = dom::query_selector_mut(root, "#sum-size") {
        dom::set_text_content(el, &format!("{} (${:.2})", size, price));
    }
    if let Some(el) = dom::query_selector_mut(root, "#sum-toppings") {
        dom::set_text_content(el, &topping_str);
    }
    if let Some(el) = dom::query_selector_mut(root, "#sum-total") {
        dom::set_text_content(el, &format!("${:.2}", total));
    }
    if let Some(el) = dom::query_selector_mut(root, "#order-btn") {
        dom::set_attribute(el, "value", &format!("Place Order — ${:.2}", total));
    }
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            platform: None,
            renderer: Renderer::new(),
            doc: None,
            mouse: (0.0, 0.0),
            width: 860.0,
        }
    }
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            el.create_window(
                Window::default_attributes()
                    .with_title("Pizza Builder — webcore Forms Demo")
                    .with_inner_size(winit::dpi::LogicalSize::new(860u32, 900u32)),
            )
            .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();

        let mut doc = load_html(HTML, self.width);

        // ── Wire up events using the library event system ────────────────

        // Checkboxes: toggle and update summary
        for id in &[
            "pep", "mush", "onion", "saus", "pepper", "olive", "cheese", "jala",
        ] {
            let sel = format!("#{}", id);
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, sel.as_str()) else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    // Toggle is already handled by process_mouse_event
                    update_summary(root);
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
        }

        // Radio buttons: update summary on size change
        for val in &["small", "medium", "large"] {
            let sel = format!("input[value={}]", val);
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, sel.as_str()) else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    update_summary(root);
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
        }

        // Order button
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#order-btn") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                if let Some(el) = dom::query_selector_mut(root, "#status") {
                    dom::set_text_content(el, "Order placed! Thank you!");
                }
                if let Some(el) = dom::query_selector_mut(root, "#progress") {
                    dom::set_attribute(el, "value", "1");
                }
                eprintln!("🍕 ORDER PLACED!");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // Reset button
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#reset-btn") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                // Clear text inputs
                for id in &["name", "phone"] {
                    if let Some(el) = dom::query_selector_mut(root, &format!("#{}", id)) {
                        dom::set_attribute(el, "value", "");
                    }
                }
                // Uncheck all toppings
                for id in &[
                    "pep", "mush", "onion", "saus", "pepper", "olive", "cheese", "jala",
                ] {
                    if let Some(el) = dom::query_selector_mut(root, &format!("#{}", id)) {
                        el.attributes.remove("checked");
                    }
                }
                if let Some(el) = dom::query_selector_mut(root, "#status") {
                    dom::set_text_content(el, "Order reset. Start fresh!");
                }
                if let Some(el) = dom::query_selector_mut(root, "#progress") {
                    dom::set_attribute(el, "value", "0");
                }
                update_summary(root);
                eprintln!("🔄 Order reset");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // Initial summary
        update_summary(&mut doc.root);

        self.doc = Some(doc);
        self.window = Some(window);
        self.platform = Some(platform);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _wid: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (window, platform) = match (&self.window, &mut self.platform) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };

        // Let the renderer handle built-in events (hover, click dispatch, focus, scroll)
        self.renderer.handle_window_event(&event, self.doc.as_mut());

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.width = platform.logical_width();
                if let Some(doc) = self.doc.as_mut() {
                    LayoutEngine::new().layout(doc, self.width);
                }
                window.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let sf = platform.scale_factor();
                self.mouse = (position.x as f32 / sf, position.y as f32 / sf);
                if let Some(doc) = self.doc.as_mut() {
                    let w = self.width;
                    let ch = platform.logical_height();
                    doc.process_scrollbar_event(
                        HtmlEventType::MouseMove,
                        self.mouse.0,
                        self.mouse.1,
                        w,
                        ch,
                    );
                }
                window.request_redraw();
            }

            WindowEvent::MouseInput { .. } => {
                // handle_window_event already dispatches mouse events (click, focus, etc.)
                window.request_redraw();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(doc) = self.doc.as_mut() {
                    let sf = platform.scale_factor();
                    let dy = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => -y * 40.0,
                        winit::event::MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / sf,
                    };
                    let pt = (self.mouse.0 + doc.scroll_x, self.mouse.1 + doc.scroll_y);
                    doc.process_wheel_event(pt, dy);
                }
                window.request_redraw();
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    if let PhysicalKey::Code(code) = key_event.physical_key {
                        let kc = match code {
                            KeyCode::Escape => 27,
                            KeyCode::Enter => 13,
                            KeyCode::Tab => 9,
                            KeyCode::Backspace => 8,
                            KeyCode::Delete => 46,
                            KeyCode::Space => 32,
                            KeyCode::ArrowLeft => 37,
                            KeyCode::ArrowUp => 38,
                            KeyCode::ArrowRight => 39,
                            KeyCode::ArrowDown => 40,
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
                            // Extract the actual character typed
                            let ch = key_event.text.as_ref().and_then(|s| s.chars().next());
                            let etype = match key_event.state {
                                ElementState::Pressed => HtmlEventType::KeyDown,
                                ElementState::Released => HtmlEventType::KeyUp,
                            };
                            if doc.process_key_event(etype, kc, ch, false, false, false, false) {
                                LayoutEngine::new().layout(doc, self.width);
                                window.request_redraw();
                            }
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let Some(doc) = self.doc.as_mut() {
                    platform.render(|scale, pixmap| {
                        self.renderer.render(doc, pixmap, scale);
                    });
                }
            }

            _ => {}
        }
    }
}

fn main() {
    eprintln!("🍕 Pizza Builder — webcore Forms Demo");
    eprintln!("   Click checkboxes and radio buttons");
    eprintln!("   Click text fields to type");
    eprintln!("   Click 'Place Order' to submit");
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run");
}
