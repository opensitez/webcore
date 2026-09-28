//! Native shell for an embedded Webcore document.
//!
//! The browser owns the OS window, input translation and presentation. An
//! embedding runtime supplies only document painting and guest callbacks.

use std::sync::Arc;

use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::platform::Platform;
use crate::ui_events::{self, UiEvent};

pub trait EmbeddedPage {
    fn title(&self) -> String;
    fn resize(&mut self, width: f32, height: f32);
    fn paint(&mut self, pixmap: &mut Pixmap, scale: f32);
    fn input(&mut self, event: &UiEvent);
    fn tick(&mut self);
}

struct Shell<P> {
    page: P,
    width: u32,
    height: u32,
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    pointer: PhysicalPosition<f64>,
    buttons: i32,
}

impl<P: EmbeddedPage> ApplicationHandler for Shell<P> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(event_loop.create_window(
            Window::default_attributes()
                .with_title(self.page.title())
                .with_inner_size(LogicalSize::new(self.width, self.height))
        ).expect("Failed to create Webcore window"));
        let platform = Platform::new_windowed(window.clone());
        self.page.resize(platform.logical_width(), platform.logical_height());
        window.request_redraw();
        self.window = Some(window);
        self.platform = Some(platform);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(platform) = self.platform.as_mut() {
                    platform.resize(size.width, size.height);
                    self.page.resize(platform.logical_width(), platform.logical_height());
                }
                if let Some(window) = self.window.as_ref() { window.request_redraw(); }
            }
            WindowEvent::RedrawRequested => {
                if let Some(platform) = self.platform.as_mut() {
                    platform.render(|scale, pixmap| self.page.paint(pixmap, scale));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = position;
                let scale = self.platform.as_ref().map(Platform::scale_factor).unwrap_or(1.0);
                self.dispatch(UiEvent {
                    kind: "mousemove".into(),
                    client_x: (position.x / f64::from(scale)) as i32,
                    client_y: (position.y / f64::from(scale)) as i32,
                    buttons: self.buttons,
                    ..UiEvent::default()
                });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (button, mask) = match button {
                    MouseButton::Left => (0, 1),
                    MouseButton::Middle => (1, 4),
                    MouseButton::Right => (2, 2),
                    _ => (0, 0),
                };
                if state == ElementState::Pressed { self.buttons |= mask; }
                else { self.buttons &= !mask; }
                let scale = self.platform.as_ref().map(Platform::scale_factor).unwrap_or(1.0);
                self.dispatch(UiEvent {
                    kind: if state == ElementState::Pressed { "mousedown" } else { "mouseup" }.into(),
                    client_x: (self.pointer.x / f64::from(scale)) as i32,
                    client_y: (self.pointer.y / f64::from(scale)) as i32,
                    button, buttons: self.buttons,
                    ..UiEvent::default()
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let delta_y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -(y as f64) * 100.0,
                    MouseScrollDelta::PixelDelta(p) => -p.y,
                };
                self.dispatch(UiEvent {
                    kind: "wheel".into(),
                    delta_y,
                    client_x: self.pointer.x as i32,
                    client_y: self.pointer.y as i32,
                    buttons: self.buttons,
                    ..UiEvent::default()
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let (key, code, key_code) = key_fields(&event.logical_key);
                self.dispatch(UiEvent {
                    kind: if event.state == ElementState::Pressed { "keydown" } else { "keyup" }.into(),
                    key, code, key_code, repeat: event.repeat,
                    ..UiEvent::default()
                });
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.page.tick();
        if let Some(window) = self.window.as_ref() {
            let title = self.page.title();
            if window.title() != title { window.set_title(&title); }
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(16)
        ));
    }
}

impl<P: EmbeddedPage> Shell<P> {
    fn dispatch(&mut self, event: UiEvent) {
        ui_events::push(event.clone());
        self.page.input(&event);
        if let Some(window) = self.window.as_ref() { window.request_redraw(); }
    }
}

pub fn run<P: EmbeddedPage>(width: u32, height: u32, page: P) {
    let event_loop = EventLoop::new().expect("Failed to create Webcore event loop");
    let mut shell = Shell {
        page, width, height, window: None, platform: None,
        pointer: PhysicalPosition::new(0.0, 0.0), buttons: 0,
    };
    event_loop.run_app(&mut shell).expect("Webcore event loop failed");
}

fn key_fields(key: &Key) -> (String, String, i32) {
    match key {
        Key::Character(text) => {
            let Some(c) = text.chars().next() else { return Default::default(); };
            let lower = c.to_ascii_lowercase();
            let code = match lower {
                'a'..='z' => format!("Key{}", lower.to_ascii_uppercase()),
                '0'..='9' => format!("Digit{lower}"),
                ' ' => "Space".into(),
                _ => String::new(),
            };
            let key_code = if lower.is_ascii_alphabetic() { lower.to_ascii_uppercase() as i32 } else { lower as i32 };
            (c.to_string(), code, key_code)
        }
        Key::Named(named) => {
            let (name, code) = match named {
                NamedKey::Enter => ("Enter", 13),
                NamedKey::Escape => ("Escape", 27),
                NamedKey::Backspace => ("Backspace", 8),
                NamedKey::Tab => ("Tab", 9),
                NamedKey::Space => (" ", 32),
                NamedKey::ArrowLeft => ("ArrowLeft", 37),
                NamedKey::ArrowUp => ("ArrowUp", 38),
                NamedKey::ArrowRight => ("ArrowRight", 39),
                NamedKey::ArrowDown => ("ArrowDown", 40),
                NamedKey::Delete => ("Delete", 46),
                _ => ("", 0),
            };
            (name.into(), name.into(), code)
        }
        _ => Default::default(),
    }
}
