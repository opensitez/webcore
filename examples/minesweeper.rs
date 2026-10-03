/// Port of wxhtmledit/examples/minesweeper_demo.cpp
/// Minesweeper game board (static visual; no game logic).
use std::sync::{Arc, Mutex};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use rand::Rng;
use webcore::dom::HtmlEventType;
use webcore::platform::Platform;
use webcore::{Document, LayoutEngine, Renderer, load_html};

const HTML: &str = include_str!("html/minesweeper.html");

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
    rows: usize,
    cols: usize,
    mines: usize,
    flag_mode: bool,
    mine_grid: [bool; 81],
    revealed: [bool; 81],
    flagged: [bool; 81],
    adj: [u8; 81],
}

fn rc_to_idx(r: usize, c: usize) -> usize {
    r * 9 + c
}

fn parse_id_to_idx(id: &str) -> Option<usize> {
    // id format r{row}c{col}
    if !id.starts_with('r') {
        return None;
    }
    let parts: Vec<&str> = id[1..].split('c').collect();
    if parts.len() != 2 {
        return None;
    }
    let r = parts[0].parse::<usize>().ok()?;
    let c = parts[1].parse::<usize>().ok()?;
    Some(rc_to_idx(r, c))
}

fn new_game(state: &mut AppState, doc: &mut Document) {
    // reset
    state.mine_grid = [false; 81];
    state.revealed = [false; 81];
    state.flagged = [false; 81];
    state.adj = [0; 81];
    // place mines
    let mut rng = rand::thread_rng();
    let mut placed = 0;
    while placed < state.mines {
        let i = rng.gen_range(0..(state.rows * state.cols));
        if !state.mine_grid[i] {
            state.mine_grid[i] = true;
            placed += 1;
        }
    }
    // compute adjacencies
    for r in 0..state.rows {
        for c in 0..state.cols {
            let idx = rc_to_idx(r, c);
            if state.mine_grid[idx] {
                continue;
            }
            let mut count = 0u8;
            for dr in [-1i32, 0, 1].iter() {
                for dc in [-1i32, 0, 1].iter() {
                    if *dr == 0 && *dc == 0 {
                        continue;
                    }
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if nr >= 0 && nr < state.rows as i32 && nc >= 0 && nc < state.cols as i32 {
                        let ni = rc_to_idx(nr as usize, nc as usize);
                        if state.mine_grid[ni] {
                            count += 1;
                        }
                    }
                }
            }
            state.adj[idx] = count;
        }
    }
    // update DOM: clear cells and mine count
    for r in 0..state.rows {
        for c in 0..state.cols {
            let id = format!("r{}c{}", r, c);
            if let Some(cell) = doc.query_selector(&format!("#{}", id)) {
                doc.set_text_content(cell, "");
                doc.class_list_remove(cell, "cell-revealed");
                doc.class_list_remove(cell, "cell-mine");
                doc.class_list_remove(cell, "cell-flag");
                for n in 1..=8 {
                    doc.class_list_remove(cell, &format!("cell-{}", n));
                }
            }
        }
    }
    if let Some(mc) = doc.query_selector("#mine-count") {
        doc.set_text_content(mc, &state.mines.to_string());
    }
    if let Some(st) = doc.query_selector("#status") {
        doc.set_text_content(st, "Click to start");
    }
}

fn reveal_recursive(state: &mut AppState, doc: &mut Document, idx: usize) {
    if state.revealed[idx] || state.flagged[idx] {
        return;
    }
    state.revealed[idx] = true;
    if let Some(cell) = doc.query_selector(&format!("#r{}c{}", idx / 9, idx % 9)) {
        doc.class_list_add(cell, "cell-revealed");
        if state.mine_grid[idx] {
            doc.class_list_add(cell, "cell-mine");
            doc.set_text_content(cell, "*");
            return;
        }
        let a = state.adj[idx];
        if a > 0 {
            doc.set_text_content(cell, &a.to_string());
            doc.class_list_add(cell, &format!("cell-{}", a));
        }
    }
    if state.adj[idx] == 0 {
        let r = idx / 9;
        let c = idx % 9;
        for dr in [-1i32, 0, 1].iter() {
            for dc in [-1i32, 0, 1].iter() {
                if *dr == 0 && *dc == 0 {
                    continue;
                }
                let nr = r as i32 + dr;
                let nc = c as i32 + dc;
                if nr >= 0 && nr < state.rows as i32 && nc >= 0 && nc < state.cols as i32 {
                    let ni = rc_to_idx(nr as usize, nc as usize);
                    if !state.revealed[ni] {
                        reveal_recursive(state, doc, ni);
                    }
                }
            }
        }
    }
}

fn reveal_all_mines(state: &AppState, doc: &mut Document) {
    for i in 0..(state.rows * state.cols) {
        if state.mine_grid[i] {
            if let Some(c) = doc.query_selector(&format!("#r{}c{}", i / 9, i % 9)) {
                doc.class_list_add(c, "cell-mine");
                doc.set_text_content(c, "*");
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("minesweeper — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(500u32, 700u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        self.doc = Some(load_html(HTML, self.width));
        // Register event handlers and initialize game
        if let Some(doc) = self.doc.as_mut() {
            let state = self.state.clone();
            // New game
            let state2 = state.clone();
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(_) = __d.closest(evt.target, "#new-game") else {
                        return;
                    };

                    if evt.button != 0 {
                        return;
                    }
                    let mut st = state2.lock().unwrap();
                    new_game(&mut st, __d);
                }),
                webcore::dom::events::ListenerOptions::default(),
            );

            // Flag mode toggle
            let state3 = state.clone();
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(_) = __d.closest(evt.target, "#flag-mode") else {
                        return;
                    };

                    if evt.button != 0 {
                        return;
                    }
                    let mut st = state3.lock().unwrap();
                    st.flag_mode = !st.flag_mode;
                    if let Some(btn) = __d.query_selector("#flag-mode") {
                        if st.flag_mode {
                            __d.class_list_add(btn, "btn-flag-active");
                        } else {
                            __d.class_list_remove(btn, "btn-flag-active");
                        }
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );

            // Cell click
            let state4 = state.clone();
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

                    if evt.button != 0 {
                        return;
                    }
                    let id = __d.get_attribute(__cur, "id").unwrap_or_default();
                    if let Some(idx) = parse_id_to_idx(&id) {
                        let mut st = state4.lock().unwrap();
                        if st.flag_mode {
                            st.flagged[idx] = !st.flagged[idx];
                            if let Some(c) = __d.query_selector(&format!("#{}", id)) {
                                if st.flagged[idx] {
                                    __d.class_list_add(c, "cell-flag");
                                    __d.set_text_content(c, "F");
                                } else {
                                    __d.class_list_remove(c, "cell-flag");
                                    __d.set_text_content(c, "");
                                }
                            }
                            return;
                        }
                        // reveal
                        if st.revealed[idx] {
                            return;
                        }
                        if st.mine_grid[idx] {
                            // reveal mine -> game over
                            reveal_all_mines(&st, __d);
                            if let Some(s) = __d.query_selector("#status") {
                                __d.set_text_content(s, "Game Over");
                            }
                            return;
                        }
                        reveal_recursive(&mut st, __d, idx);
                        // check win
                        let mut revealed_count = 0;
                        for i in 0..(st.rows * st.cols) {
                            if st.revealed[i] {
                                revealed_count += 1;
                            }
                        }
                        if revealed_count >= (st.rows * st.cols - st.mines) {
                            if let Some(s) = __d.query_selector("#status") {
                                __d.set_text_content(s, "You Win!");
                            }
                        }
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );

            // Right-click/context menu toggles flag as well
            let state5 = state.clone();
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "contextmenu",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, ".cell") else {
                        return;
                    };
                    let id = __d.get_attribute(__cur, "id").unwrap_or_default();
                    if let Some(idx) = parse_id_to_idx(&id) {
                        let mut st = state5.lock().unwrap();
                        st.flagged[idx] = !st.flagged[idx];
                        if let Some(c) = __d.query_selector(&format!("#{}", id)) {
                            if st.flagged[idx] {
                                __d.class_list_add(c, "cell-flag");
                                __d.set_text_content(c, "F");
                            } else {
                                __d.class_list_remove(c, "cell-flag");
                                __d.set_text_content(c, "");
                            }
                        }
                    }
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
        }
        // perform initial layout and start a new game so hit-testing works
        if let Some(doc) = self.doc.as_mut() {
            LayoutEngine::new().layout(doc, self.width);
            let mut st = self.state.lock().unwrap();
            new_game(&mut st, doc);
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
                    LayoutEngine::new().layout(doc, self.width);
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
                if let Some(doc) = self.doc.as_mut() {
                    let pt = (self.mouse_pos.0, self.mouse_pos.1 + doc.scroll_y);
                    if doc.process_mouse_event(HtmlEventType::MouseMove, pt, 0) {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::MouseInput {
                state,
                button,
                ..
            } => {
                if let Some(doc) = self.doc.as_mut() {
                    let btn = match button {
                        winit::event::MouseButton::Left => 0,
                        winit::event::MouseButton::Middle => 1,
                        winit::event::MouseButton::Right => 2,
                        _ => return,
                    };
                    let kind = if state == winit::event::ElementState::Pressed {
                        HtmlEventType::MouseDown
                    } else {
                        HtmlEventType::MouseUp
                    };
                    let pt = (self.mouse_pos.0, self.mouse_pos.1 + doc.scroll_y);
                    let mut changed = doc.process_mouse_event(kind, pt, btn);
                    if btn == 2 && state == winit::event::ElementState::Released {
                        changed |= doc.process_mouse_event(HtmlEventType::ContextMenu, pt, btn);
                    }
                    if changed {
                        LayoutEngine::new().layout(doc, self.width);
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
            rows: 9,
            cols: 9,
            mines: 10,
            flag_mode: false,
            mine_grid: [false; 81],
            revealed: [false; 81],
            flagged: [false; 81],
            adj: [0; 81],
        })),
        mouse_pos: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).unwrap();
}
