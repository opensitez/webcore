/// Port of wxhtmledit/examples/markdown_demo.cpp
/// Split-pane Markdown editor in a single window:
///   Left half  — raw Markdown source (editable, monospace)
///   Right half — live rendered preview (read-only)
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use webcore::platform::Platform;
use webcore::{load_html, parse_markdown, Document, HtmlEventType, Renderer};

// ── Sample markdown ──────────────────────────────────────────────────────────

const SAMPLE_MD: &str = concat!(
    "# Markdown Editor\n\n",
    "This is a **live preview** of your Markdown content.\n\n",
    "## Features\n\n",
    "- **Bold**, *italic*, and ~~strikethrough~~\n",
    "- `Inline code` and code blocks\n",
    "- [Links](https://example.com) and images\n",
    "- Ordered and unordered lists\n",
    "- Tables with alignment\n",
    "- Blockquotes\n",
    "- Headings (ATX and Setext)\n\n",
    "## Code Block\n\n",
    "```cpp\n",
    "#include <iostream>\n\n",
    "int main() {\n",
    "    std::cout << \"Hello, Markdown!\" << std::endl;\n",
    "    return 0;\n",
    "}\n",
    "```\n\n",
    "## Table\n\n",
    "| Feature       | Status    | Notes          |\n",
    "|:--------------|:---------:|---------------:|\n",
    "| Parsing       | Done      | Round-trip     |\n",
    "| Serialization | Done      | Preserves style|\n",
    "| Editing       | Live      | Real-time      |\n\n",
    "## Blockquote\n\n",
    "> The best way to predict the future\n",
    "> is to invent it.\n",
    ">\n",
    "> — Alan Kay\n\n",
    "---\n\n",
    "### Ordered List\n\n",
    "1. First item\n",
    "2. Second item\n",
    "3. Third item\n\n",
    "Try editing the Markdown on the left!\n",
);

// ── Simple editable HTML for the source pane ─────────────────────────────────

fn make_source_html(md: &str) -> String {
    let escaped = md
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!DOCTYPE html><html><head><style>
  body {{ margin: 0; background: #f5f5f5; color: #1a1a1a; }}
  pre  {{ font-family: monospace; font-size: 13px; padding: 12px;
          white-space: pre-wrap; word-wrap: break-word; margin: 0; }}
</style></head><body><pre contenteditable="true">{}</pre></body></html>"#,
        escaped
    )
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,

    /// Left pane: editable markdown source
    src_doc: Option<Document>,

    /// Right pane: rendered markdown preview
    prev_doc: Option<Document>,

    width: f32, // total logical window width
    height: f32,
    scale: f32,
    mouse_x: f32,
    mouse_y: f32,
}

impl App {
    /// Left pane occupies [0, split_x), right pane [split_x, width)
    fn split_x(&self) -> f32 {
        (self.width * 0.45).floor()
    }

    fn request_redraw(&self) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn is_in_left_pane(&self, mouse_x: f32) -> bool {
        mouse_x / self.scale < self.split_x()
    }

    /// Rebuild the preview document from the current source text.
    fn update_preview(&mut self) {
        let markdown = if let Some(doc) = self.src_doc.as_ref() {
            doc.root.text_content()
        } else {
            return;
        };
        let pane_w = self.width - self.split_x();
        let mut doc = parse_markdown(&markdown);
        self.renderer.layout_engine().layout(&mut doc, pane_w);
        doc.editor.read_only = true;
        self.prev_doc = Some(doc);
    }

    /// Render both panes into the window's pixmap by rendering into two
    /// temporary sub-pixmaps and blitting them side-by-side.
    fn render_split(&mut self, scale: f32, main: &mut Pixmap) {
        let split_x = self.split_x();
        let pane_w_left = split_x;
        let pane_w_right = self.width - split_x;

        let pw_left = (pane_w_left * scale).round() as u32;
        let pw_right = (pane_w_right * scale).round() as u32;
        let ph = main.height();

        // Left sub-pixmap
        if let Some(mut left_pm) = Pixmap::new(pw_left.max(1), ph.max(1)) {
            if let Some(doc) = self.src_doc.as_mut() {
                self.renderer.render(doc, &mut left_pm, scale);
            }
            blit(&left_pm, main, 0, 0);
        }

        // Right sub-pixmap
        if let Some(mut right_pm) = Pixmap::new(pw_right.max(1), ph.max(1)) {
            if let Some(doc) = self.prev_doc.as_mut() {
                self.renderer.render(doc, &mut right_pm, scale);
            }
            blit(&right_pm, main, pw_left, 0);
        }

        // Draw a 2-pixel divider between panes
        let div_x = pw_left as usize;
        let stride = main.width() as usize;
        let sep = tiny_skia::ColorU8::from_rgba(80, 80, 100, 255).premultiply();
        let div_pixels = main.pixels_mut();
        for row in 0..ph as usize {
            for dx in 0..2usize {
                let col = div_x + dx;
                if col < stride {
                    div_pixels[row * stride + col] = sep;
                }
            }
        }
    }
}

/// Copy all pixels from `src` into `dst` at offset (`dst_x`, `dst_y`).
fn blit(src: &Pixmap, dst: &mut Pixmap, dst_x: u32, dst_y: u32) {
    let src_w = src.width() as usize;
    let src_h = src.height() as usize;
    let dst_w = dst.width() as usize;
    let dst_h = dst.height() as usize;
    let src_pixels = src.pixels();
    let dst_pixels = dst.pixels_mut();
    for row in 0..src_h {
        let dst_row = dst_y as usize + row;
        if dst_row >= dst_h {
            break;
        }
        let src_base = row * src_w;
        let dst_base = dst_row * dst_w + dst_x as usize;
        let copy_w = src_w.min(dst_w.saturating_sub(dst_x as usize));
        dst_pixels[dst_base..dst_base + copy_w]
            .copy_from_slice(&src_pixels[src_base..src_base + copy_w]);
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

// ── ApplicationHandler ────────────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Markdown Editor — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1100u32, 750u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.scale = platform.scale_factor();
        self.width = platform.logical_width();
        self.height = platform.logical_height();
        // Sync renderer DPI scale so fill_char_x_for_line shapes at the same
        // physical-pixel size as draw_text_run.  Without this, on HiDPI displays
        // char_x positions are computed at scale=1.0 but text is rendered at 2.0,
        // causing click-to-caret mapping to be off by several characters.
        self.renderer.set_scale(self.scale);

        // Source pane
        let src_html = make_source_html(SAMPLE_MD);
        let src_w = self.split_x();
        let mut src_doc = load_html(&src_html, src_w);
        self.renderer.layout_engine().layout(&mut src_doc, src_w);
        self.src_doc = Some(src_doc);

        // Preview pane
        let prev_w = self.width - self.split_x();
        let mut prev_doc = parse_markdown(SAMPLE_MD);
        self.renderer.layout_engine().layout(&mut prev_doc, prev_w);
        prev_doc.editor.read_only = true;
        self.prev_doc = Some(prev_doc);

        self.window = Some(window);
        self.platform = Some(platform);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        if self.platform.is_none() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Focused(focused) => {
                if let Some(doc) = self.src_doc.as_mut() {
                    doc.editor.has_focus = focused;
                    self.request_redraw();
                }
            }

            WindowEvent::Resized(size) => {
                let (new_scale, new_w, new_h) = {
                    let p = self.platform.as_mut().unwrap();
                    p.resize(size.width, size.height);
                    (p.scale_factor(), p.logical_width(), p.logical_height())
                };
                self.scale = new_scale;
                self.width = new_w;
                self.height = new_h;
                self.renderer.set_scale(new_scale);

                let src_w = self.split_x();
                let prev_w = self.width - src_w;
                let mut engine = self.renderer.layout_engine();
                if let Some(doc) = self.src_doc.as_mut() {
                    engine.layout(doc, src_w);
                }
                if let Some(doc) = self.prev_doc.as_mut() {
                    engine.layout(doc, prev_w);
                }
                self.request_redraw();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self.platform.as_ref().unwrap().scale_factor();
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / scale,
                };
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                let pane_off = self.split_x();
                if self.is_in_left_pane(self.mouse_x) {
                    if let Some(doc) = self.src_doc.as_mut() {
                        let doc_pt = (mx / sc, my / sc + doc.scroll_y);
                        doc.process_wheel_event(doc_pt, dy);
                    }
                } else {
                    if let Some(doc) = self.prev_doc.as_mut() {
                        let doc_pt = ((mx / sc) - pane_off, my / sc + doc.scroll_y);
                        doc.process_wheel_event(doc_pt, dy);
                    }
                }
                self.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32;
                self.mouse_y = position.y as f32;
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                let (sx, sy) = (mx / sc, my / sc);
                let split_x = self.split_x();
                let pane_w_right = self.width - split_x;
                if self.is_in_left_pane(self.mouse_x) {
                    if let Some(doc) = self.src_doc.as_mut() {
                        let sb = doc.process_scrollbar_event(
                            HtmlEventType::MouseMove,
                            sx,
                            sy,
                            split_x,
                            self.height,
                        );
                        if sb {
                            self.request_redraw();
                        } else {
                            let pt = (sx, sy + doc.scroll_y);
                            if doc.process_mouse_event(HtmlEventType::MouseMove, pt, 0) {
                                self.request_redraw();
                            }
                        }
                    }
                } else if let Some(doc) = self.prev_doc.as_mut() {
                    let lx = sx - split_x;
                    if doc.process_scrollbar_event(
                        HtmlEventType::MouseMove,
                        lx,
                        sy,
                        pane_w_right,
                        self.height,
                    ) {
                        self.request_redraw();
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
                let (mx, my, sc) = (self.mouse_x, self.mouse_y, self.scale);
                let (sx, sy) = (mx / sc, my / sc);
                let split_x = self.split_x();
                let pane_w_right = self.width - split_x;
                if state == ElementState::Pressed {
                    if self.is_in_left_pane(self.mouse_x) {
                        if let Some(doc) = self.src_doc.as_mut() {
                            let sb = doc.process_scrollbar_event(
                                HtmlEventType::MouseDown,
                                sx,
                                sy,
                                split_x,
                                self.height,
                            );
                            if !sb {
                                let pt = (sx, sy + doc.scroll_y);
                                doc.process_mouse_event(HtmlEventType::MouseDown, pt, bt);
                            }
                            self.request_redraw();
                        }
                    } else if let Some(doc) = self.prev_doc.as_mut() {
                        let lx = sx - split_x;
                        doc.process_scrollbar_event(
                            HtmlEventType::MouseDown,
                            lx,
                            sy,
                            pane_w_right,
                            self.height,
                        );
                        self.request_redraw();
                    }
                } else {
                    if self.is_in_left_pane(self.mouse_x) {
                        if let Some(doc) = self.src_doc.as_mut() {
                            doc.process_scrollbar_event(
                                HtmlEventType::MouseUp,
                                sx,
                                sy,
                                split_x,
                                self.height,
                            );
                            let pt = (sx, sy + doc.scroll_y);
                            doc.process_mouse_event(HtmlEventType::MouseUp, pt, bt);
                            self.request_redraw();
                        }
                    } else if let Some(doc) = self.prev_doc.as_mut() {
                        let lx = sx - split_x;
                        doc.process_scrollbar_event(
                            HtmlEventType::MouseUp,
                            lx,
                            sy,
                            pane_w_right,
                            self.height,
                        );
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
                let src_w = self.split_x();
                let changed = if let Some(doc) = self.src_doc.as_mut() {
                    doc.process_key_event(
                        HtmlEventType::KeyDown,
                        key_code,
                        ch,
                        false,
                        false,
                        false,
                        false,
                    )
                } else {
                    false
                };

                if changed {
                    let mut engine = self.renderer.layout_engine();
                    if let Some(doc) = self.src_doc.as_mut() {
                        engine.layout(doc, src_w);
                    }
                    self.request_redraw();
                    self.update_preview();
                }
            }

            WindowEvent::RedrawRequested => {
                // Render both panes into a temp pixmap, then blit to the platform surface.
                // This avoids double-borrowing self while platform.render holds its closure.
                let scale = self.scale;
                let w_phys = (self.width * scale).round() as u32;
                let h_phys = (self.height * scale).round() as u32;
                if let Some(mut main_pm) = Pixmap::new(w_phys.max(1), h_phys.max(1)) {
                    self.render_split(scale, &mut main_pm);
                    // Now borrow platform briefly
                    if let Some(platform) = self.platform.as_mut() {
                        let src_pixels = main_pm.pixels().to_vec(); // copy to owned
                        platform.render(|_s, pixmap| {
                            let dst = pixmap.pixels_mut();
                            let n = dst.len().min(src_pixels.len());
                            dst[..n].copy_from_slice(&src_pixels[..n]);
                        });
                    }
                }
                if let Some(doc) = self.src_doc.as_ref() {
                    event_loop
                        .set_control_flow(ControlFlow::WaitUntil(doc.editor.next_blink_deadline()));
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {
        if let Some(doc) = self.src_doc.as_mut() {
            if doc.editor.has_focus && doc.editor.blink_update() {
                self.request_redraw();
            }
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
        src_doc: None,
        prev_doc: None,
        width: 1100.0,
        height: 750.0,
        scale: 1.0,
        mouse_x: 0.0,
        mouse_y: 0.0,
    };
    event_loop.run_app(&mut app).unwrap();
}
