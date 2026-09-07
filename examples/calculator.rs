use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::types::ComponentRegistry;
use webcore::{load_html_with_registry, Document, LayoutEngine, Renderer};

const HTML: &str = include_str!("html/calculator.html");

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
        let mut doc = load_html_with_registry(HTML, "", self.width, 420.0, self.registry.clone());

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
                let root = &mut __d.root;
                let _ = &root;
                // left click only
                if evt.button != 0 {
                    return;
                }
                // Prevent default editor behavior (caret/selection) for buttons
                evt.prevent_default();
                let cur_id = __cur;
                // Read button value first
                let val_opt = dom::find_box_mut(root, cur_id)
                    .and_then(|t| dom::get_attribute(t, "data-value").map(|s| s.to_string()));
                let id = dom::find_box_mut(root, cur_id)
                    .and_then(|t| dom::get_attribute(t, "id").map(|s| s.to_string()))
                    .unwrap_or_default();
                let id = id.as_str();
                // clear
                if id == "clear" {
                    if let Some(d) = dom::query_selector_mut(root, "#display") {
                        dom::set_text_content(d, "0");
                    }
                    return;
                }
                if id == "equals" {
                    if let Some(d) = dom::query_selector(root, "#display") {
                        let cur = dom::get_text_content(d);
                        match eval_expression(&cur) {
                            Ok(v) => {
                                if let Some(dd) = dom::query_selector_mut(root, "#display") {
                                    dom::set_text_content(dd, &format!("{}", v));
                                }
                            }
                            Err(e) => {
                                if let Some(dd) = dom::query_selector_mut(root, "#display") {
                                    dom::set_text_content(dd, &format!("err: {}", e));
                                }
                            }
                        }
                    }
                    return;
                }
                // normal buttons with data-value
                if let Some(val) = val_opt {
                    if let Some(d) = dom::query_selector_mut(root, "#display") {
                        let cur = dom::get_text_content(d).trim().to_string();
                        let next = if cur == "0" {
                            val.to_string()
                        } else {
                            format!("{}{}", cur, val)
                        };
                        eprintln!(
                            "[CALC_DBG] before set cur='{}' val='{}' next='{}'",
                            cur, val, next
                        );
                        dom::set_text_content(d, &next);
                        // Read back and log result
                        let after = dom::get_text_content(d);
                        eprintln!("[CALC_DBG] after set display='{}'", after);
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
                    LayoutEngine::new().layout(doc, self.width);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_x = position.x as f32 / platform.scale_factor();
                self.mouse_y = position.y as f32 / platform.scale_factor();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (self.mouse_x, self.mouse_y + doc.scroll_y);
                    if doc.process_mouse_event(HtmlEventType::Click, pt, 0) {
                        LayoutEngine::new().layout(doc, self.width);
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
