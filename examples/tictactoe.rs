/// Port of wxhtmledit/examples/tictactoe_demo.cpp
/// Tic-Tac-Toe game board (static visual; no game logic).
use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

// Returns Some((winner_char, [a,b,c])) if a winner found, otherwise None.
fn check_winner(board: &[Option<char>; 9]) -> Option<(char, [usize; 3])> {
    let lines: [[usize; 3]; 8] = [
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8], // rows
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8], // cols
        [0, 4, 8],
        [2, 4, 6], // diags
    ];
    for &ln in &lines {
        if let (Some(a), Some(b), Some(c)) = (board[ln[0]], board[ln[1]], board[ln[2]]) {
            if a == b && b == c {
                return Some((a, ln));
            }
        }
    }
    None
}

fn is_draw(board: &[Option<char>; 9]) -> bool {
    board.iter().all(|c| c.is_some()) && check_winner(board).is_none()
}

fn minimax(board: &mut [Option<char>; 9], depth: i32, is_max: bool) -> i32 {
    if let Some((w, _)) = check_winner(board) {
        return match w {
            'O' => 10 - depth,
            'X' => depth - 10,
            _ => 0,
        };
    }
    if is_draw(board) {
        return 0;
    }

    if is_max {
        let mut best = -1000;
        for i in 0..9 {
            if board[i].is_none() {
                board[i] = Some('O');
                let val = minimax(board, depth + 1, false);
                board[i] = None;
                best = best.max(val);
            }
        }
        best
    } else {
        let mut best = 1000;
        for i in 0..9 {
            if board[i].is_none() {
                board[i] = Some('X');
                let val = minimax(board, depth + 1, true);
                board[i] = None;
                best = best.min(val);
            }
        }
        best
    }
}

fn best_ai_move(board: &mut [Option<char>; 9]) -> Option<usize> {
    let mut best_val = -10000;
    let mut best_move: Option<usize> = None;
    for i in 0..9 {
        if board[i].is_none() {
            board[i] = Some('O');
            let move_val = minimax(board, 0, false);
            board[i] = None;
            if move_val > best_val {
                best_val = move_val;
                best_move = Some(i);
            }
        }
    }
    best_move
}
use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::{Document, Renderer, WebCore};

const HTML: &str = include_str!("html/tictactoe.html");

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    state: Arc<Mutex<AppState>>,
    mouse_pos: (f32, f32),
}

struct AppState {
    next_x: bool,
    score_x: i32,
    score_o: i32,
    score_d: i32,
    board: [Option<char>; 9],
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("tictactoe — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(500u32, 700u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        self.doc = Some(self.renderer.load_html_vp(HTML, self.width, 700.0));
        // Register simple click handlers for cells and controls
        if let Some(doc) = self.doc.as_mut() {
            let state = self.state.clone();
            // cell clicks: human plays X, then AI (O) via minimax
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, ".cell") else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    let cur_id = __cur;
                    let id = dom::find_box_mut(root, cur_id)
                        .and_then(|t| dom::get_attribute(t, "id").map(|s| s.to_string()))
                        .unwrap_or_default();
                    let idx = id
                        .strip_prefix('c')
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);
                    let mut st = state.lock().unwrap();
                    if st.board[idx].is_some() {
                        return;
                    }
                    // Human move
                    st.board[idx] = Some('X');
                    if let Some(cell) = dom::query_selector_mut(root, &format!("#{}", id)) {
                        dom::set_text_content(cell, "X");
                        dom::add_class(cell, "cell-x");
                    }
                    if let Some((w, line)) = check_winner(&st.board) {
                        for &i in &line {
                            if let Some(c) = dom::query_selector_mut(root, &format!("#c{}", i)) {
                                dom::add_class(c, "cell-win");
                            }
                        }
                        if let Some(s) = dom::query_selector_mut(root, "#status") {
                            dom::set_text_content(s, &format!("{} wins!", w));
                        }
                        return;
                    }
                    if is_draw(&st.board) {
                        if let Some(s) = dom::query_selector_mut(root, "#status") {
                            dom::set_text_content(s, "Draw");
                        }
                        return;
                    }
                    // AI move
                    if let Some(ai_idx) = best_ai_move(&mut st.board) {
                        st.board[ai_idx] = Some('O');
                        if let Some(cell) = dom::query_selector_mut(root, &format!("#c{}", ai_idx))
                        {
                            dom::set_text_content(cell, "O");
                            dom::add_class(cell, "cell-o");
                        }
                        if let Some((w2, line2)) = check_winner(&st.board) {
                            for &i in &line2 {
                                if let Some(c) = dom::query_selector_mut(root, &format!("#c{}", i))
                                {
                                    dom::add_class(c, "cell-win");
                                }
                            }
                            if let Some(s) = dom::query_selector_mut(root, "#status") {
                                dom::set_text_content(s, &format!("{} wins!", w2));
                            }
                            return;
                        }
                        if is_draw(&st.board) {
                            if let Some(s) = dom::query_selector_mut(root, "#status") {
                                dom::set_text_content(s, "Draw");
                            }
                            return;
                        }
                    }
                    if let Some(s) = dom::query_selector_mut(root, "#status") {
                        dom::set_text_content(s, "Your turn");
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );

            // Reset button
            let state = self.state.clone();
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, "#reset") else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    let mut st = state.lock().unwrap();
                    for i in 0..9 {
                        if let Some(c) = dom::query_selector_mut(root, &format!("#c{}", i)) {
                            dom::set_text_content(c, "");
                            dom::remove_class(c, "cell-x");
                            dom::remove_class(c, "cell-o");
                            dom::remove_class(c, "cell-win");
                        }
                    }
                    st.next_x = true;
                    if let Some(s) = dom::query_selector_mut(root, "#status") {
                        dom::set_text_content(s, "Your turn");
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );

            // Difficulty toggles
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, "#diff-easy") else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    if let Some(e) = dom::query_selector_mut(root, "#diff-easy") {
                        dom::add_class(e, "btn-diff-active");
                    }
                    if let Some(e) = dom::query_selector_mut(root, "#diff-hard") {
                        dom::remove_class(e, "btn-diff-active");
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, "#diff-hard") else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    if let Some(e) = dom::query_selector_mut(root, "#diff-hard") {
                        dom::add_class(e, "btn-diff-active");
                    }
                    if let Some(e) = dom::query_selector_mut(root, "#diff-easy") {
                        dom::remove_class(e, "btn-diff-active");
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
        }
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
                if let Some(doc) = self.doc.as_mut() {
                    self.renderer.layout_engine().layout(doc, self.width);
                }
                window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mp.0, mp.1 + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (
                    position.x as f32 / platform.scale_factor(),
                    position.y as f32 / platform.scale_factor(),
                );
            }
            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    if doc.process_mouse_event(
                        HtmlEventType::Click,
                        (self.mouse_pos.0, self.mouse_pos.1 + doc.scroll_y),
                        0,
                    ) {
                        self.renderer.layout_engine().layout(doc, self.width);
                        window.request_redraw();
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
        width: 500.0,
        state: Arc::new(Mutex::new(AppState {
            next_x: true,
            score_x: 0,
            score_o: 0,
            score_d: 0,
            board: Default::default(),
        })),
        mouse_pos: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).unwrap();
}
