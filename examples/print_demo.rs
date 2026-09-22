/// Port of wxhtmledit/examples/print_demo.cpp
/// Print/preview demo.
///
/// Note: print and print-preview are not implemented in the Rust port.
/// This demo shows the document content with visual A4 page-break markers
/// so the layout can be inspected.  The toolbar buttons display an
/// informational notice instead of printing.
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use webcore::dom;
use webcore::dom::events::ListenerOptions;
use webcore::platform::Platform;
use webcore::{Document, LayoutEngine, Renderer, load_html};

const HTML: &str = include_str!("html/print.html");

// A4 page height in CSS pixels at 96 dpi ≈ 1122 px logical
const A4_PAGE_HEIGHT_PX: f32 = 1122.0;

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    height: f32,
    scale: f32,
    mouse_x: f32,
    mouse_y: f32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Print Demo (preview only) — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(900u32, 780u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.scale = platform.scale_factor();
        self.renderer.set_scale(self.scale);
        self.width = platform.logical_width();
        self.height = platform.logical_height();

        let mut doc = load_html(HTML, self.width);
        doc.editor.read_only = true;
        install_print_demo_handlers(&mut doc);
        apply_print_demo_state(&mut doc);
        LayoutEngine::new().layout(&mut doc, self.width);
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
        let renderer_handled = match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::ModifiersChanged(_) => {
                self.renderer.handle_window_event(&event, self.doc.as_mut())
            }
            _ => false,
        };
        if renderer_handled {
            if let Some(doc) = self.doc.as_mut() {
                apply_print_demo_state(doc);
                LayoutEngine::new().layout(doc, self.width);
            }
            self.request_redraw();
        }
        let (_window, platform) = match (self.window.as_ref(), self.platform.as_mut()) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.scale = platform.scale_factor();
                self.renderer.set_scale(self.scale);
                self.width = platform.logical_width();
                self.height = platform.logical_height();
                if let Some(doc) = self.doc.as_mut() {
                    LayoutEngine::new().layout(doc, self.width);
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / platform.scale_factor()
                    }
                };
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mx / sc, my / sc + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32;
                self.mouse_y = position.y as f32;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Released && button == MouseButton::Left {
                    self.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if renderer_handled {
                    return;
                }
                // Space / Page-Down: scroll one page; Escape: exit
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => event_loop.exit(),
                    Key::Named(NamedKey::Space) | Key::Named(NamedKey::PageDown) => {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.scroll_y += self.height;
                        }
                        self.request_redraw();
                    }
                    Key::Named(NamedKey::PageUp) => {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.scroll_y -= self.height;
                        }
                        self.request_redraw();
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.scroll_y += 40.0;
                        }
                        self.request_redraw();
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        if let Some(doc) = self.doc.as_mut() {
                            doc.scroll_y -= 40.0;
                        }
                        self.request_redraw();
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let doc = match self.doc.as_mut() {
                    Some(d) => d,
                    None => return,
                };
                let width = self.width;
                let renderer = &mut self.renderer;
                platform.render(|scale, pixmap| {
                    renderer.render(doc, pixmap, scale);
                    draw_page_breaks(pixmap, scale, doc.scroll_y, width, A4_PAGE_HEIGHT_PX);
                });
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}
}

impl App {
    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

/// Draw thin red horizontal lines at A4 page boundaries onto the pixmap.
fn draw_page_breaks(
    pixmap: &mut tiny_skia::Pixmap,
    scale: f32,
    scroll_y: f32,
    _width: f32,
    page_height: f32,
) {
    let ph = pixmap.height() as f32;

    // First page boundary visible in the current scroll window
    let first_boundary = (scroll_y / page_height).ceil() * page_height;
    let mut y_logical = first_boundary;

    let pix_w = pixmap.width();
    let pix_h = pixmap.height();
    let red = tiny_skia::ColorU8::from_rgba(220, 50, 50, 180).premultiply();
    let pixels = pixmap.pixels_mut();
    while y_logical < scroll_y + ph / scale {
        let y_physical = (y_logical - scroll_y) * scale;
        if y_physical >= 0.0 && y_physical < ph {
            let y0 = y_physical.floor() as u32;
            let y1 = (y0 + 1).min(pix_h - 1);
            for row in y0..=y1 {
                let base = (row * pix_w) as usize;
                for col in 0..pix_w as usize {
                    pixels[base + col] = red;
                }
            }
        }
        y_logical += page_height;
    }
}

fn install_print_demo_handlers(doc: &mut Document) {
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc: &mut Document| {
            if doc.closest(evt.target, "#btn-print").is_some() {
                set_notice(
                    doc,
                    "Print output is not implemented yet; showing the current preview settings.",
                );
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#btn-preview").is_some() {
                set_notice(doc, "Print preview refreshed.");
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#btn-settings").is_some() {
                set_notice(
                    doc,
                    "Preview settings are live: header, footer, and scale update here.",
                );
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#chk-header").is_some() {
                set_notice(doc, "Preview options updated.");
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#chk-footer").is_some() {
                set_notice(doc, "Preview options updated.");
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#lbl-header").is_some() {
                toggle_checked(doc, "#chk-header");
                set_notice(doc, "Preview options updated.");
                apply_print_demo_state(doc);
                return;
            }
            if doc.closest(evt.target, "#lbl-footer").is_some() {
                toggle_checked(doc, "#chk-footer");
                set_notice(doc, "Preview options updated.");
                apply_print_demo_state(doc);
            }
        }),
        ListenerOptions::default(),
    );
}

fn apply_print_demo_state(doc: &mut Document) {
    let header_checked = selector_checked(doc, "#chk-header");
    let footer_checked = selector_checked(doc, "#chk-footer");
    let scale_pct = doc
        .query_selector("#scale-input")
        .map(|id| doc.value(id))
        .and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(100.0)
        .clamp(25.0, 400.0);

    if let Some(id) = doc.query_selector("#page-header") {
        doc.set_style_property(id, "display", if header_checked { "block" } else { "none" });
    }
    if let Some(id) = doc.query_selector("#page-footer") {
        doc.set_style_property(id, "display", if footer_checked { "block" } else { "none" });
    }
    if let Some(id) = doc.query_selector("#doc-content") {
        doc.set_style_property(
            id,
            "font-size",
            &format!("{:.2}px", 16.0 * scale_pct / 100.0),
        );
    }
    if let Some(id) = doc.query_selector("#scale-input") {
        let normalized = format!("{scale_pct:.0}");
        if doc.value(id) != normalized {
            doc.set_value(id, &normalized);
        }
    }
}

fn selector_checked(doc: &Document, selector: &str) -> bool {
    doc.query_selector(selector)
        .and_then(|id| doc.get_attribute(id, "checked"))
        .is_some()
}

fn toggle_checked(doc: &mut Document, selector: &str) {
    if let Some(id) = doc.query_selector(selector) {
        if doc.get_attribute(id, "checked").is_some() {
            doc.remove_attribute(id, "checked");
        } else {
            doc.set_attribute(id, "checked", "");
        }
    }
}

fn set_notice(doc: &mut Document, text: &str) {
    if let Some(id) = doc.query_selector("#notice") {
        if let Some(node) = dom::find_box_mut(&mut doc.root, id) {
            dom::set_text_content(node, text);
        }
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
        width: 900.0,
        height: 780.0,
        scale: 1.0,
        mouse_x: 0.0,
        mouse_y: 0.0,
    };
    event_loop.run_app(&mut app).unwrap();
}
