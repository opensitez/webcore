/// Port of wxhtmledit/examples/contenteditable_demo.cpp
/// A form/settings UI with specific fields marked as contenteditable.
/// The HTML already has contenteditable="true" on the field divs;
/// the rest of the document is read-only.
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use webcore::platform::Platform;
use webcore::{Document, HtmlEventType, Renderer};

const HTML: &str = include_str!("html/contenteditable.html");

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
                        .with_title("Contenteditable Demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(800u32, 700u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.scale = platform.scale_factor();
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        let mut doc = self.renderer.load_html(HTML, self.width);
        // The document is read-only by default; individual fields have
        // contenteditable="true" which the HTML parser marks as editable.
        doc.editor.read_only = true;
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
        let (_window, platform) = match (self.window.as_ref(), self.platform.as_mut()) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(focused) => {
                if let Some(doc) = self.doc.as_mut() {
                    doc.editor.has_focus = focused;
                    self.request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.scale = platform.scale_factor();
                self.width = platform.logical_width();
                self.height = platform.logical_height();
                let mut engine = self.renderer.layout_engine();
                if let Some(doc) = self.doc.as_mut() {
                    engine.layout(doc, self.width);
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
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (mx / sc, my / sc + doc.scroll_y);
                    if doc.process_mouse_event(HtmlEventType::MouseMove, pt, 0) {
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let etype = if state == ElementState::Pressed {
                    HtmlEventType::MouseDown
                } else {
                    HtmlEventType::MouseUp
                };
                let bt = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => 0,
                };
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (mx / sc, my / sc + doc.scroll_y);
                    if doc.process_mouse_event(etype, pt, bt) {
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key_code = winit_key_to_code(&event.logical_key);
                let ch = match &event.logical_key {
                    Key::Character(s) => s.chars().next(),
                    Key::Named(NamedKey::Space) => Some(' '),
                    _ => None,
                };
                let mut engine = self.renderer.layout_engine();
                if let Some(doc) = self.doc.as_mut() {
                    if doc.process_key_event(
                        HtmlEventType::KeyDown,
                        key_code,
                        ch,
                        false,
                        false,
                        false,
                        false,
                    ) {
                        if ch.is_some() && key_code >= 32 {
                            engine.layout_no_cascade(doc, self.width);
                        } else {
                            engine.layout(doc, self.width);
                        }
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let doc = match self.doc.as_mut() {
                    Some(d) => d,
                    None => return,
                };
                let renderer = &mut self.renderer;
                platform.render(|scale, pixmap| {
                    renderer.render(doc, pixmap, scale);
                });
                event_loop
                    .set_control_flow(ControlFlow::WaitUntil(doc.editor.next_blink_deadline()));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(doc) = self.doc.as_mut() {
            if doc.editor.has_focus && doc.editor.blink_update() {
                self.request_redraw();
            }
        }
    }
}

impl App {
    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

fn winit_key_to_code(key: &Key) -> u32 {
    match key {
        Key::Named(NamedKey::Enter) => 13,
        Key::Named(NamedKey::Backspace) => 8,
        Key::Named(NamedKey::Delete) => 46,
        Key::Named(NamedKey::ArrowLeft) => 37,
        Key::Named(NamedKey::ArrowRight) => 39,
        Key::Named(NamedKey::Tab) => 9,
        Key::Character(s) => s.chars().next().map(|c| c as u32).unwrap_or(0),
        _ => 0,
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
        width: 800.0,
        height: 700.0,
        scale: 1.0,
        mouse_x: 0.0,
        mouse_y: 0.0,
    };
    event_loop.run_app(&mut app).unwrap();
}
