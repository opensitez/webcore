mod platform;

use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use platform::Platform;
use webcore::{Document, HtmlEventType, Renderer, load_html_with_base};

const DEMO_HTML: &str = r##"<!DOCTYPE html>
<html>
<head>
  <style>
    body { font-family: sans-serif; margin: 16px; color: #222; background: #fff; }
    h1   { color: #2c5aa0; font-size: 2em; margin-bottom: 8px; }
    h2   { color: #444; font-size: 1.4em; margin: 16px 0 4px; border-bottom: 2px solid #ddd; }
    p    { margin: 8px 0; line-height: 1.6; }
    .card {
      border: 1px solid #ccc; border-radius: 6px;
      padding: 12px 16px; margin: 12px 0;
      background: #f9f9f9;
    }
    .flex-row { display: flex; gap: 12px; margin: 12px 0; }
    .box {
      flex: 1; padding: 12px; border-radius: 4px;
      background: #e8f0fe; border: 1px solid #8ab4f8;
    }
    ul   { padding-left: 2em; }
    code { background: #eee; padding: 1px 4px; border-radius: 3px; font-family: monospace; }
    a    { color: #1a73e8; }
    strong { font-weight: bold; }
    em     { font-style: italic; }
    table  { border-collapse: collapse; width: 100%; margin: 12px 0; }
    th, td { border: 1px solid #ccc; padding: 6px 12px; }
    th     { background: #f0f0f0; font-weight: bold; }
    blockquote {
      border-left: 4px solid #8ab4f8; margin: 12px 0;
      padding: 8px 16px; background: #f0f5ff; color: #555;
    }
  </style>
</head>
<body>
  <h1>webcore — Rust HTML/CSS Renderer</h1>
  <p contenteditable="true">A port of <strong>wxhtmledit</strong> to Rust, using <em>tiny-skia</em> for rendering
     and <em>winit</em> for windowing. Click here to edit.</p>

  <div class="card">
    <h2>Features</h2>
    <ul>
      <li>Full CSS cascade with specificity</li>
      <li>Block, inline, <strong>flexbox</strong>, and <strong>CSS grid</strong> layout</li>
      <li>Table layout with colspan/rowspan</li>
      <li>Bidirectional text (LTR/RTL)</li>
      <li>Font rendering via <code>cosmic-text</code></li>
      <li>Border, background, border-radius, shadows</li>
      <li>List markers (disc, decimal, alpha, roman)</li>
      <li>Pseudo-classes and attribute selectors</li>
      <li>HTML entities and whitespace collapsing</li>
    </ul>
  </div>

  <h2>Flexbox Demo</h2>
  <div class="flex-row">
    <div class="box"><strong>Column A</strong><br/>flex-grow: 1</div>
    <div class="box"><strong>Column B</strong><br/>flex-grow: 1</div>
    <div class="box"><strong>Column C</strong><br/>flex-grow: 1</div>
  </div>

  <h2>Typography</h2>
  <p>Normal text, <strong>bold</strong>, <em>italic</em>, <strong><em>bold italic</em></strong>,
     <code>monospace code</code>, <a href="#">link</a>.</p>

  <blockquote>
    "The goal is to have a complete, fast HTML/CSS rendering engine in pure Rust."
  </blockquote>

  <h2>Table</h2>
  <table>
    <tr><th>Property</th><th>C++ (wxhtmledit)</th><th>Rust (webcore)</th></tr>
    <tr><td>Language</td><td>C++17</td><td>Rust 2021</td></tr>
    <tr><td>Rendering</td><td>wxWidgets DC</td><td>tiny-skia</td></tr>
    <tr><td>Windowing</td><td>wxWidgets</td><td>winit</td></tr>
    <tr><td>Text</td><td>wxFont</td><td>cosmic-text</td></tr>
    <tr><td>Layout</td><td>Custom engine</td><td>Ported engine</td></tr>
  </table>

  <p style="color:#888; font-size:0.85em;">webcore v0.1.0 — built with tiny-skia + winit + cosmic-text</p>
</body>
</html>
"##;

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
    initial_html: String,
    base_url: String,
    /// Receiver for a Document being loaded on a background thread.
    pending_doc: Option<std::sync::mpsc::Receiver<Document>>,
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
                        .with_title("webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(900u32, 700u32)),
                )
                .expect("Failed to create window"),
        );

        let platform = Platform::new_windowed(window.clone());
        self.scale = platform.scale_factor();
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        self.renderer.set_scale(self.scale);

        // Load content on a background thread so the window shows immediately.
        // If `base` is a URL/file URL and `html` is empty, let webcore own the
        // progressive fetch path instead of blocking before the event loop.
        let html = std::mem::take(&mut self.initial_html);
        let base = std::mem::take(&mut self.base_url);
        let w = self.width;
        let h = self.height;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (html, base) = if html.is_empty() && !base.is_empty() {
                match webcore::loading::load_document_progressive(
                    &base,
                    &webcore::PageLoadOptions {
                        preview_interval_bytes: usize::MAX / 4,
                        ..Default::default()
                    },
                    |_, _| {},
                ) {
                    Ok((html, final_url)) => (html, final_url),
                    Err(err) => (
                        format!("<h2>Failed to load {base}</h2><pre>{err}</pre>"),
                        base,
                    ),
                }
            } else {
                (html, base)
            };
            let doc = load_html_with_base(&html, &base, w, h);
            let _ = tx.send(doc);
        });
        self.pending_doc = Some(rx);

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
        // Built-in zoom + pan: pinch, PanGesture, Ctrl+Wheel, Ctrl+=/−/0.
        if self.renderer.handle_window_event(&event, self.doc.as_mut()) {
            self.request_redraw();
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
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
                self.renderer.set_scale(self.scale);
                if let Some(doc) = self.doc.as_mut() {
                    self.renderer.layout_engine().layout(doc, self.width);
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 20.0, y * 20.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => (
                        p.x as f32 / platform.scale_factor(),
                        p.y as f32 / platform.scale_factor(),
                    ),
                };
                let zoom = self.renderer.zoom;
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mx / sc / zoom + doc.scroll_x, my / sc / zoom + doc.scroll_y);
                    doc.process_wheel_event_xy(doc_pt, dx, dy);
                }
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32;
                self.mouse_y = position.y as f32;
                let zoom = self.renderer.zoom;
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                let (sx, sy) = (mx / sc, my / sc);
                if let Some(doc) = self.doc.as_mut() {
                    let sb = doc.process_scrollbar_event(
                        HtmlEventType::MouseMove,
                        sx,
                        sy,
                        self.width,
                        self.height,
                    );
                    if sb {
                        self.request_redraw();
                    } else {
                        let pt = (sx / zoom, sy / zoom + doc.scroll_y);
                        if doc.process_mouse_event(HtmlEventType::MouseMove, pt, 0) {
                            self.request_redraw();
                        }
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
                let zoom = self.renderer.zoom;
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                let (sx, sy) = (mx / sc, my / sc);
                if state == ElementState::Pressed {
                    if let Some(doc) = self.doc.as_mut() {
                        let sb = doc.process_scrollbar_event(
                            HtmlEventType::MouseDown,
                            sx,
                            sy,
                            self.width,
                            self.height,
                        );
                        if !sb {
                            let pt = (sx / zoom, sy / zoom + doc.scroll_y);
                            doc.process_mouse_event(HtmlEventType::MouseDown, pt, bt);
                        }
                        self.request_redraw();
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
                        let pt = (sx / zoom, sy / zoom + doc.scroll_y);
                        doc.process_mouse_event(HtmlEventType::MouseUp, pt, bt);
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key_code = winit_key_to_code(&event.logical_key);
                let ch = match &event.logical_key {
                    Key::Character(s) => s.chars().next(),
                    _ => None,
                };
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
                        let engine = self.renderer.layout_engine();
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

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        // Check if the background document load has finished.
        if let Some(rx) = self.pending_doc.as_ref() {
            if let Ok(mut doc) = rx.try_recv() {
                // Re-layout with the renderer's font system for accurate glyph widths.
                self.renderer.layout_engine().viewport_h = self.height;
                self.renderer.layout_engine().layout(&mut doc, self.width);
                self.doc = Some(doc);
                self.pending_doc = None;
                self.request_redraw();
            }
        }

        let mut needs_redraw = false;
        let mut has_pending = self.pending_doc.is_some();
        if let Some(doc) = self.doc.as_mut() {
            if doc.editor.has_focus && doc.editor.blink_update() {
                needs_redraw = true;
            }
            // Poll for async resources that arrived from background threads.
            let mut needs_relayout = false;
            if doc.poll_pending_stylesheets() {
                self.renderer.layout_engine().invalidate_cascade();
                doc.style_dirty = true;
                needs_relayout = true;
                needs_redraw = true;
                self.renderer.invalidate_display_list();
            }
            let image_poll =
                doc.poll_pending_images_budgeted(32, std::time::Duration::from_millis(8));
            if image_poll.loaded_any {
                needs_relayout |= image_poll.needs_relayout;
                needs_redraw = true;
                if image_poll.needs_relayout {
                    self.renderer.invalidate_display_list();
                } else {
                    self.renderer.invalidate_paint_rects(image_poll.paint_rects);
                }
            }
            if self
                .renderer
                .layout_engine()
                .poll_pending_fonts_budgeted(8, std::time::Duration::from_millis(8))
            {
                self.renderer.layout_engine().invalidate_cascade();
                doc.style_dirty = true;
                needs_relayout = true;
                self.renderer.invalidate_display_list();
            }
            if needs_relayout {
                self.renderer.layout_engine().layout(doc, self.width);
                needs_redraw = true;
            }
            // Keep polling while resources are still in flight.
            has_pending = has_pending
                || doc.pending_images.is_some()
                || doc.pending_stylesheets.is_some()
                || self.renderer.layout_engine().has_pending_fonts();
        }
        if needs_redraw {
            self.request_redraw();
        }
        // Use short poll interval while async resources are in flight,
        // otherwise wait for events normally.
        if has_pending {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(50),
            ));
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
        Key::Character(s) => s.chars().next().map(|c| c as u32).unwrap_or(0),
        _ => 0,
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    let (initial_html, base_url) = if let Some(ref path) = arg {
        if path.starts_with("http://") || path.starts_with("https://") {
            (String::new(), path.clone())
        } else {
            let file_url = std::fs::canonicalize(path)
                .ok()
                .map(|path| format!("file://{}", path.display()))
                .unwrap_or_else(|| format!("file://{path}"));
            (String::new(), file_url)
        }
    } else {
        (DEMO_HTML.to_string(), String::new())
    };

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App {
        window: None,
        platform: None,
        renderer: Renderer::new(),
        doc: None,
        width: 900.0,
        height: 700.0,
        scale: 1.0,
        mouse_x: 0.0,
        mouse_y: 0.0,
        pending_doc: None,
        initial_html,
        base_url,
    };
    event_loop.run_app(&mut app).expect("Failed to run app");
}
