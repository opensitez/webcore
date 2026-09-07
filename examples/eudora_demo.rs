/// Port of wxhtmledit/examples/eudora_demo.cpp
/// Complex UI showcase with sidebar and main content.
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::platform::Platform;
use webcore::{load_html, Document, HtmlEventType, LayoutEngine, Renderer};

const HTML: &str = include_str!("html/eudora.html");

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    height: f32,
    mouse_pos: (f32, f32),
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("eudora_demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1200u32, 900u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        let mut doc = load_html(HTML, self.width);
        self.renderer.layout_engine().layout(&mut doc, self.width);
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
                self.height = platform.logical_height();
                if let Some(doc) = self.doc.as_mut() {
                    self.renderer.layout_engine().layout(doc, self.width);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let sf = platform.scale_factor();
                let (sx, sy) = (position.x as f32 / sf, position.y as f32 / sf);
                self.mouse_pos = (sx, sy);
                if let Some(doc) = self.doc.as_mut() {
                    if doc.process_scrollbar_event(
                        HtmlEventType::MouseMove,
                        sx,
                        sy,
                        self.width,
                        self.height,
                    ) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let bt = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => 0,
                };
                let (sx, sy) = self.mouse_pos;
                if state == ElementState::Pressed {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.process_scrollbar_event(
                            HtmlEventType::MouseDown,
                            sx,
                            sy,
                            self.width,
                            self.height,
                        );
                        window.request_redraw();
                    }
                } else {
                    if let Some(doc) = self.doc.as_mut() {
                        doc.process_scrollbar_event(
                            HtmlEventType::MouseUp,
                            sx,
                            sy,
                            self.width,
                            self.height,
                        );
                        let _ = bt;
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / platform.scale_factor()
                    }
                };
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mp.0, mp.1 + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                window.request_redraw();
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
            }
            _ => {}
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
        width: 1200.0,
        height: 900.0,
        mouse_pos: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).unwrap();
}
