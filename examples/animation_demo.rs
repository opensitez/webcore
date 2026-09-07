/// CSS @keyframes animation showcase.
/// Demonstrates the webcore animation runtime: spin, pulse, bounce, fade,
/// colour-cycle, staggered dots, ripple, heartbeat.
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::platform::Platform;
use webcore::{load_html, Document, HtmlEventType, Renderer};

const HTML: &str = include_str!("html/animation_demo.html");

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    mouse_pos: (f32, f32),
}

impl App {
    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Animation Demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(960u32, 700u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        let doc = load_html(HTML, self.width);
        self.doc = Some(doc);
        self.window = Some(window);
        self.platform = Some(platform);
        // Kick off the first animation frame.
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _wid: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (window, platform) = match (self.window.as_ref(), self.platform.as_mut()) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };

        if self.renderer.handle_window_event(&event, self.doc.as_mut()) {
            self.request_redraw();
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.width = platform.logical_width();
                if let Some(doc) = self.doc.as_mut() {
                    self.renderer.layout_engine().layout(doc, self.width);
                }
                self.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let sf = platform.scale_factor();
                let zoom = self.renderer.zoom;
                self.mouse_pos = (position.x as f32 / sf, position.y as f32 / sf);
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (mp.0 / zoom, mp.1 / zoom + doc.scroll_y);
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
                let zoom = self.renderer.zoom;
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (mp.0 / zoom, mp.1 / zoom + doc.scroll_y);
                    if doc.process_mouse_event(etype, pt, bt) {
                        self.request_redraw();
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
                let zoom = self.renderer.zoom;
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mp.0 / zoom, mp.1 / zoom + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                self.request_redraw();
            }

            WindowEvent::RedrawRequested => {
                let doc = match self.doc.as_mut() {
                    Some(d) => d,
                    None => return,
                };
                let renderer = &mut self.renderer;

                // Re-layout every frame so tick_animations advances.
                renderer.layout_engine().layout(doc, self.width);

                platform.render(|scale, pixmap| {
                    renderer.render(doc, pixmap, scale);
                });

                // Schedule the next frame while any animation is running.
                if doc.needs_animation_frame {
                    window.request_redraw();
                }
            }

            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    // Poll keeps the loop running continuously; needed for smooth animations.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        window: None,
        platform: None,
        renderer: Renderer::new(),
        doc: None,
        width: 960.0,
        mouse_pos: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).unwrap();
}
