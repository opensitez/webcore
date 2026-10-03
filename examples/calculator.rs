use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::dom::HtmlEventType;
use webcore::platform::Platform;
use webcore::types::ComponentRegistry;
use webcore::{Document, LayoutEngine, Renderer, load_html_with_registry};

const HTML: &str = include_str!("html/calculator.html");

fn relayout(doc: &mut Document, width: f32, height: f32) {
    let mut layout = LayoutEngine::new();
    layout.viewport_h = height;
    layout.layout(doc, width);
}

fn eval_expression(expr: &str) -> Result<f64, String> {
    // simple tokenizer + shunting-yard + RPN evaluation
    #[derive(Debug)]
    enum Tok {
        Num(f64),
        Op(char),
        LParen,
        RParen,
    }

    fn prec(op: char) -> i32 {
        match op {
            '+' | '-' => 1,
            '*' | '/' => 2,
            _ => 0,
        }
    }

    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    let s = expr.trim();
    while i < s.len() {
        let ch = s.as_bytes()[i] as char;
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            i += 1;
            while i < s.len() {
                let c = s.as_bytes()[i] as char;
                if c.is_ascii_digit() || c == '.' {
                    i += 1
                } else {
                    break;
                }
            }
            let num = s[start..i].parse::<f64>().map_err(|e| e.to_string())?;
            toks.push(Tok::Num(num));
            continue;
        }
        match ch {
            '+' | '-' | '*' | '/' => {
                toks.push(Tok::Op(ch));
                i += 1
            }
            '(' => {
                toks.push(Tok::LParen);
                i += 1
            }
            ')' => {
                toks.push(Tok::RParen);
                i += 1
            }
            _ => return Err(format!("unexpected '{}'", ch)),
        }
    }

    // shunting-yard
    let mut out: Vec<Tok> = Vec::new();
    let mut ops: Vec<char> = Vec::new();
    for t in toks {
        match t {
            Tok::Num(n) => out.push(Tok::Num(n)),
            Tok::Op(op) => {
                while let Some(&top) = ops.last() {
                    if top == '(' {
                        break;
                    }
                    if prec(top) >= prec(op) {
                        out.push(Tok::Op(ops.pop().unwrap()));
                    } else {
                        break;
                    }
                }
                ops.push(op);
            }
            Tok::LParen => ops.push('('),
            Tok::RParen => {
                while let Some(op) = ops.pop() {
                    if op == '(' {
                        break;
                    }
                    out.push(Tok::Op(op));
                }
            }
        }
    }
    while let Some(op) = ops.pop() {
        if op == '(' {
            return Err("mismatched parentheses".into());
        }
        out.push(Tok::Op(op));
    }

    // eval RPN
    let mut st: Vec<f64> = Vec::new();
    for t in out {
        match t {
            Tok::Num(n) => st.push(n),
            Tok::Op(op) => {
                if st.len() < 2 {
                    return Err("invalid expression".into());
                }
                let b = st.pop().unwrap();
                let a = st.pop().unwrap();
                let r = match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => return Err(format!("unknown op {}", op)),
                };
                st.push(r);
            }
            _ => {}
        }
    }
    if st.len() == 1 {
        Ok(st[0])
    } else {
        Err("invalid evaluation".into())
    }
}

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    registry: ComponentRegistry,
    expr: String,
    width: f32,
    mouse_x: f32,
    mouse_y: f32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("calculator — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(360u32, 420u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        let mut doc = load_html_with_registry(
            HTML,
            "",
            self.width,
            platform.logical_height(),
            self.registry.clone(),
        );

        // buttons
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".btn") else {
                    return;
                };
                // left click only
                if evt.button != 0 {
                    return;
                }
                // Prevent default editor behavior (caret/selection) for buttons
                evt.prevent_default();
                let cur_id = __cur;
                // Read button value first
                let val_opt = __d.get_attribute(cur_id, "data-value");
                let id = __d.get_attribute(cur_id, "id").unwrap_or_default();
                let id = id.as_str();
                // clear
                if id == "clear" {
                    if let Some(d) = __d.query_selector("#display") {
                        __d.set_text_content(d, "0");
                    }
                    return;
                }
                if id == "equals" {
                    if let Some(d) = __d.query_selector("#display") {
                        let cur = __d.text_content(d);
                        match eval_expression(&cur) {
                            Ok(v) => {
                                __d.set_text_content(d, &format!("{}", v));
                            }
                            Err(e) => {
                                __d.set_text_content(d, &format!("err: {}", e));
                            }
                        }
                    }
                    return;
                }
                // normal buttons with data-value
                if let Some(val) = val_opt {
                    if let Some(d) = __d.query_selector("#display") {
                        let cur = __d.text_content(d).trim().to_string();
                        let next = if cur == "0" {
                            val.to_string()
                        } else {
                            format!("{}{}", cur, val)
                        };
                        __d.set_text_content(d, &next);
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
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (window, platform) = match (self.window.as_ref(), self.platform.as_mut()) {
            (Some(w), Some(p)) => (w, p),
            _ => return,
        };
        match event {
            WindowEvent::CloseRequested => _event_loop.exit(),
            WindowEvent::Resized(size) => {
                platform.resize(size.width, size.height);
                self.width = platform.logical_width();
                if let Some(doc) = self.doc.as_mut() {
                    relayout(doc, self.width, platform.logical_height());
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32 / platform.scale_factor();
                self.mouse_y = position.y as f32 / platform.scale_factor();
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (self.mouse_x, self.mouse_y + doc.scroll_y);
                    if doc.process_mouse_event(HtmlEventType::MouseMove, pt, 0) {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (self.mouse_x, self.mouse_y + doc.scroll_y);
                    let kind = if state == ElementState::Pressed {
                        HtmlEventType::MouseDown
                    } else {
                        HtmlEventType::MouseUp
                    };
                    if doc.process_mouse_event(kind, pt, 0) {
                        relayout(doc, self.width, platform.logical_height());
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(doc) = self.doc.as_mut() {
                    let renderer = &mut self.renderer;
                    platform.render(|scale, pixmap| {
                        renderer.render(doc, pixmap, scale);
                    });
                }
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
        registry: ComponentRegistry::default(),
        expr: String::new(),
        width: 360.0,
        mouse_x: 0.0,
        mouse_y: 0.0,
    };
    event_loop.run_app(&mut app).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use webcore::dom;

    #[test]
    fn calculator_relayout_keeps_the_window_height() {
        let mut doc = load_html_with_registry(HTML, "", 360.0, 420.0, ComponentRegistry::default());
        relayout(&mut doc, 360.0, 420.0);

        let calc = dom::query_selector(&doc.root, ".calc").unwrap();
        assert_eq!(doc.viewport_h, 420.0);
        assert!(
            calc.layout.border_rect.y >= 0.0,
            "{:?}",
            calc.layout.border_rect
        );
        assert!(
            calc.layout.border_rect.bottom() <= 420.0,
            "{:?}",
            calc.layout.border_rect
        );
    }

    #[test]
    fn display_updates_through_document_text_content() {
        let mut doc = load_html_with_registry(HTML, "", 360.0, 420.0, ComponentRegistry::default());
        let display = doc.query_selector("#display").unwrap();
        doc.set_text_content(display, "42");
        assert_eq!(doc.text_content(display), "42");
        assert_eq!(doc.query_selector("#display"), Some(display));
    }
}
