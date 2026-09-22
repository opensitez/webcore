use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tiny_skia::{FillRule, Paint, PathBuilder, Rect, Stroke, Transform};
use webcore::dom;
use webcore::dom::events::ListenerOptions;
use webcore::types::{Component, WebCore};
use webcore::{BrowserView, Document, parse_markdown};

pub struct DemoLive {
    installed_file: Option<String>,
    dom_state: Arc<Mutex<DomState>>,
    ttt_state: Arc<Mutex<TicTacToeState>>,
    mine_state: Arc<Mutex<MineState>>,
    graph_state: Arc<Mutex<GraphState>>,
    kanban_state: Arc<Mutex<KanbanState>>,
    playground_state: Arc<Mutex<PlaygroundState>>,
    email_state: Arc<Mutex<EmailState>>,
    eudora_state: Arc<Mutex<EudoraState>>,
    last_markdown_source: String,
    last_dom_tick: Instant,
    seed: u32,
}

impl DemoLive {
    pub fn new() -> Self {
        Self {
            installed_file: None,
            dom_state: Arc::new(Mutex::new(DomState::new())),
            ttt_state: Arc::new(Mutex::new(TicTacToeState::default())),
            mine_state: Arc::new(Mutex::new(MineState::new())),
            graph_state: Arc::new(Mutex::new(GraphState::default())),
            kanban_state: Arc::new(Mutex::new(KanbanState::default())),
            playground_state: Arc::new(Mutex::new(PlaygroundState::default())),
            email_state: Arc::new(Mutex::new(EmailState::default())),
            eudora_state: Arc::new(Mutex::new(EudoraState::default())),
            last_markdown_source: String::new(),
            last_dom_tick: Instant::now(),
            seed: 0x1234_abcd,
        }
    }

    pub fn update(&mut self, file: Option<&str>, view: &mut BrowserView) -> bool {
        let Some(file) = file else {
            self.installed_file = None;
            return false;
        };

        let already_installed = view
            .document()
            .and_then(|doc| doc.get_attribute(doc.root.node_id, "data-demo-live-installed"))
            .is_some_and(|installed| installed == file);

        let mut changed = false;
        if self.installed_file.as_deref() != Some(file) || !already_installed {
            if self.install(file, view) {
                self.installed_file = Some(file.to_string());
                changed = true;
            }
        }

        if file == "dom.html" {
            let interval_ms = view
                .document()
                .and_then(|doc| doc.query_selector("#speed-slider").map(|id| doc.value(id)))
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| self.dom_state.lock().unwrap().interval_ms)
                .clamp(200, 3000);
            if self.last_dom_tick.elapsed() >= Duration::from_millis(interval_ms) {
                self.last_dom_tick = Instant::now();
                let mut seed = self.seed;
                if let Some(doc) = view.document_mut() {
                    dom_tick(doc, &self.dom_state, &mut seed);
                    changed = true;
                }
                self.seed = seed;
            }
        }
        if file == "markdown.html" {
            if let Some(doc) = view.document_mut() {
                if update_markdown_preview(doc, &mut self.last_markdown_source) {
                    changed = true;
                }
            }
        }

        changed
    }

    pub fn next_wake_deadline(&self, file: Option<&str>, view: &BrowserView) -> Option<Instant> {
        if file != Some("dom.html") {
            return None;
        }
        let interval_ms = view
            .document()
            .and_then(|doc| doc.query_selector("#speed-slider").map(|id| doc.value(id)))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_else(|| self.dom_state.lock().unwrap().interval_ms)
            .clamp(200, 3000);
        let interval = Duration::from_millis(interval_ms);
        Some(Instant::now() + interval.saturating_sub(self.last_dom_tick.elapsed()))
    }

    fn install(&mut self, file: &str, view: &mut BrowserView) -> bool {
        let Some(doc) = view.document_mut() else {
            return false;
        };
        if !demo_document_ready(doc, file) {
            return false;
        }
        match file {
            "calculator.html" => install_calculator(doc),
            "tictactoe.html" => install_tictactoe(doc, self.ttt_state.clone()),
            "minesweeper.html" => install_minesweeper(doc, self.mine_state.clone(), &mut self.seed),
            "dom.html" => install_dom(doc, self.dom_state.clone()),
            "graph.html" => install_graph(doc, self.graph_state.clone(), &mut self.seed),
            "markdown.html" => install_markdown(doc, &mut self.last_markdown_source),
            "email.html" => install_email(doc, self.email_state.clone()),
            "eudora.html" => install_eudora(doc, self.eudora_state.clone()),
            "print.html" => install_print(doc),
            "forms_demo.html" => install_forms(doc),
            "events.html" => install_events(doc, self.kanban_state.clone(), &mut self.seed),
            "event_playground.html" => install_event_playground(doc, self.playground_state.clone()),
            _ => return false,
        }
        doc.set_attribute(doc.root.node_id, "data-demo-live-installed", file);
        true
    }
}

pub struct GraphComponent;

impl Component for GraphComponent {
    fn measure(&self, node: &WebCore, _available_w: f32) -> (f32, f32) {
        let w = attr(node, "data-width", "340").parse().unwrap_or(340.0);
        let h = attr(node, "data-height", "190").parse().unwrap_or(190.0);
        (w, h)
    }

    fn intrinsic_width(&self, node: &WebCore) -> (f32, f32) {
        let w = attr(node, "data-width", "340").parse().unwrap_or(340.0);
        (w, w)
    }

    fn paint(
        &self,
        node: &WebCore,
        pixmap: &mut tiny_skia::Pixmap,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        scale: f32,
    ) {
        let ts = Transform::from_scale(scale, scale);
        let mut bg = Paint::default();
        bg.set_color_rgba8(22, 27, 34, 255);
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            pixmap.fill_rect(rect, &bg, ts, None);
        }

        let values = parse_values(&attr(node, "data-values", ""));
        if values.is_empty() {
            return;
        }
        let chart_type = attr(node, "data-type", "bar");
        let max = values.iter().copied().fold(0.0, f32::max).max(1.0);
        let margin = 12.0;
        let plot_w = (w - margin * 2.0).max(1.0);
        let plot_h = (h - margin * 2.0).max(1.0);
        match chart_type.as_str() {
            "line" | "area" | "scatter" => {
                let step = plot_w / (values.len().saturating_sub(1)).max(1) as f32;
                if chart_type == "area" {
                    let mut pb = PathBuilder::new();
                    pb.move_to(x + margin, y + h - margin);
                    for (i, value) in values.iter().enumerate() {
                        pb.line_to(
                            x + margin + i as f32 * step,
                            y + h - margin - (value / max) * plot_h,
                        );
                    }
                    pb.line_to(x + margin + plot_w, y + h - margin);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let mut fill = Paint::default();
                        fill.set_color_rgba8(89, 161, 79, 60);
                        pixmap.fill_path(&path, &fill, FillRule::Winding, ts, None);
                    }
                }
                let mut pb = PathBuilder::new();
                for (i, value) in values.iter().enumerate() {
                    let px = x + margin + i as f32 * step;
                    let py = y + h - margin - (value / max) * plot_h;
                    if chart_type == "scatter" {
                        let mut dot = Paint::default();
                        dot.set_color_rgba8(78, 121, 167, 220);
                        if let Some(rect) = Rect::from_xywh(px - 3.0, py - 3.0, 6.0, 6.0) {
                            pixmap.fill_rect(rect, &dot, ts, None);
                        }
                    } else if i == 0 {
                        pb.move_to(px, py);
                    } else {
                        pb.line_to(px, py);
                    }
                }
                if chart_type != "scatter" {
                    if let Some(path) = pb.finish() {
                        let mut line = Paint::default();
                        line.set_color_rgba8(89, 161, 79, 255);
                        let mut stroke = Stroke::default();
                        stroke.width = 2.0;
                        pixmap.stroke_path(&path, &line, &stroke, ts, None);
                    }
                }
            }
            "pie" | "donut" => {
                let total = values.iter().sum::<f32>().max(1.0);
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                let radius = (w.min(h) / 2.0 - 22.0).max(20.0);
                let inner = if chart_type == "donut" {
                    radius * 0.55
                } else {
                    0.0
                };
                let mut start = -90.0f32;
                let colors = [(78, 121, 167), (242, 142, 43), (89, 161, 79), (225, 87, 89)];
                for (i, value) in values.iter().enumerate() {
                    let sweep = value / total * 360.0;
                    let mut pb = PathBuilder::new();
                    let start_rad = start.to_radians();
                    if inner == 0.0 {
                        pb.move_to(cx, cy);
                    } else {
                        pb.move_to(cx + inner * start_rad.cos(), cy + inner * start_rad.sin());
                    }
                    pb.line_to(cx + radius * start_rad.cos(), cy + radius * start_rad.sin());
                    let steps = (sweep / 5.0).max(4.0) as i32;
                    for step in 1..=steps {
                        let a = (start + sweep * step as f32 / steps as f32).to_radians();
                        pb.line_to(cx + radius * a.cos(), cy + radius * a.sin());
                    }
                    if inner > 0.0 {
                        for step in (0..=steps).rev() {
                            let a = (start + sweep * step as f32 / steps as f32).to_radians();
                            pb.line_to(cx + inner * a.cos(), cy + inner * a.sin());
                        }
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let (r, g, b) = colors[i % colors.len()];
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(r, g, b, 255);
                        pixmap.fill_path(&path, &paint, FillRule::Winding, ts, None);
                    }
                    start += sweep;
                }
            }
            "gauge" => {
                let pct = ((values[0] - 95.0) / 5.0).clamp(0.0, 1.0);
                let mut track = Paint::default();
                track.set_color_rgba8(48, 54, 61, 255);
                let mut fill = Paint::default();
                fill.set_color_rgba8(63, 185, 80, 255);
                let mut stroke = Stroke::default();
                stroke.width = 10.0;
                let cx = x + w / 2.0;
                let cy = y + h / 2.0 + 25.0;
                let radius = (w.min(h) / 2.0 - 28.0).max(20.0);
                stroke_arc(pixmap, ts, cx, cy, radius, 180.0, 360.0, &track, &stroke);
                stroke_arc(
                    pixmap,
                    ts,
                    cx,
                    cy,
                    radius,
                    180.0,
                    180.0 + 180.0 * pct,
                    &fill,
                    &stroke,
                );
            }
            _ => {
                let bar_w = plot_w / values.len() as f32;
                let mut paint = Paint::default();
                paint.set_color_rgba8(78, 121, 167, 255);
                for (i, value) in values.iter().enumerate() {
                    let bh = value / max * plot_h;
                    let rect = if chart_type == "hbar" {
                        Rect::from_xywh(
                            x + margin,
                            y + margin + i as f32 * (plot_h / values.len() as f32),
                            value / max * plot_w,
                            (plot_h / values.len() as f32 - 4.0).max(1.0),
                        )
                    } else {
                        Rect::from_xywh(
                            x + margin + i as f32 * bar_w + 2.0,
                            y + h - margin - bh,
                            (bar_w - 4.0).max(1.0),
                            bh,
                        )
                    };
                    if let Some(rect) = rect {
                        pixmap.fill_rect(rect, &paint, ts, None);
                    }
                }
            }
        }
    }
}

fn stroke_arc(
    pixmap: &mut tiny_skia::Pixmap,
    ts: Transform,
    cx: f32,
    cy: f32,
    r: f32,
    start: f32,
    end: f32,
    paint: &Paint,
    stroke: &Stroke,
) {
    let mut pb = PathBuilder::new();
    let steps = ((end - start).abs() / 5.0).max(2.0) as i32;
    for i in 0..=steps {
        let a = (start + (end - start) * i as f32 / steps as f32).to_radians();
        let x = cx + r * a.cos();
        let y = cy + r * a.sin();
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, paint, stroke, ts, None);
    }
}

fn install_calculator(doc: &mut Document) {
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            let Some(id) = doc.closest(evt.target, ".btn") else {
                return;
            };
            if evt.button != 0 {
                return;
            }
            evt.prevent_default();
            let button_id = doc.get_attribute(id, "id").unwrap_or_default();
            if button_id == "clear" {
                set_text(doc, "#display", "0");
            } else if button_id == "equals" {
                let expr = text(doc, "#display");
                set_text(
                    doc,
                    "#display",
                    &eval_expression(&expr)
                        .map(|v| v.to_string())
                        .unwrap_or_else(|e| format!("err: {e}")),
                );
            } else if let Some(value) = doc.get_attribute(id, "data-value") {
                let cur = text(doc, "#display");
                let next = if cur.trim() == "0" {
                    value
                } else {
                    format!("{cur}{value}")
                };
                set_text(doc, "#display", &next);
            }
        }),
        ListenerOptions::default(),
    );
}

fn demo_document_ready(doc: &Document, file: &str) -> bool {
    let selector = match file {
        "calculator.html" => "#display",
        "tictactoe.html" => "#c0",
        "minesweeper.html" => "#r0c0",
        "dom.html" => "#speed-slider",
        "graph.html" => "graph",
        "markdown.html" => ".md-source",
        "email.html" => ".email-list",
        "eudora.html" => "#inbox-rows",
        "print.html" => "#btn-print",
        "forms_demo.html" => "#order-btn",
        "events.html" => "#body-backlog",
        "event_playground.html" => "#zone-hover",
        _ => return false,
    };
    doc.query_selector(selector).is_some()
}

#[derive(Default)]
struct TicTacToeState {
    board: [Option<char>; 9],
    x_score: u32,
    o_score: u32,
    draws: u32,
}

fn install_tictactoe(doc: &mut Document, state: Arc<Mutex<TicTacToeState>>) {
    state.lock().unwrap().board = [None; 9];
    for idx in 0..9 {
        let Some(cell) = node_id(doc, &format!("#c{idx}")) else {
            continue;
        };
        let state_cells = state.clone();
        doc.add_event_listener(
            cell,
            "click",
            Box::new(move |_evt, doc| {
                let mut st = state_cells.lock().unwrap();
                if st.board[idx].is_some() || winner(&st.board).is_some() {
                    return;
                }
                st.board[idx] = Some('X');
                mark_cell(doc, idx, 'X');
                if finish_ttt(doc, &mut st) {
                    return;
                }
                let ai = best_ai(&st.board).or_else(|| st.board.iter().position(|v| v.is_none()));
                if let Some(ai) = ai {
                    st.board[ai] = Some('O');
                    mark_cell(doc, ai, 'O');
                    if finish_ttt(doc, &mut st) {
                        return;
                    }
                }
                set_text(doc, "#status", "Your turn");
            }),
            ListenerOptions::default(),
        );
    }

    let state_reset = state.clone();
    if let Some(reset) = node_id(doc, "#reset") {
        doc.add_event_listener(
            reset,
            "click",
            Box::new(move |_evt, doc| {
                let mut st = state_reset.lock().unwrap();
                st.board = [None; 9];
                for idx in 0..9 {
                    set_text(doc, &format!("#c{idx}"), "");
                    remove_class(doc, &format!("#c{idx}"), "cell-x");
                    remove_class(doc, &format!("#c{idx}"), "cell-o");
                    remove_class(doc, &format!("#c{idx}"), "cell-win");
                }
                set_text(doc, "#status", "Your turn");
            }),
            ListenerOptions::default(),
        );
    }

    if let Some(easy) = node_id(doc, "#diff-easy") {
        doc.add_event_listener(
            easy,
            "click",
            Box::new(move |_evt, doc| {
                add_class(doc, "#diff-easy", "btn-diff-active");
                remove_class(doc, "#diff-hard", "btn-diff-active");
            }),
            ListenerOptions::default(),
        );
    }
    if let Some(hard) = node_id(doc, "#diff-hard") {
        doc.add_event_listener(
            hard,
            "click",
            Box::new(move |_evt, doc| {
                add_class(doc, "#diff-hard", "btn-diff-active");
                remove_class(doc, "#diff-easy", "btn-diff-active");
            }),
            ListenerOptions::default(),
        );
    }
}

#[derive(Clone)]
struct MineState {
    mines: [bool; 81],
    revealed: [bool; 81],
    flagged: [bool; 81],
    adj: [u8; 81],
    flag_mode: bool,
}

impl MineState {
    fn new() -> Self {
        Self {
            mines: [false; 81],
            revealed: [false; 81],
            flagged: [false; 81],
            adj: [0; 81],
            flag_mode: false,
        }
    }
}

fn install_minesweeper(doc: &mut Document, state: Arc<Mutex<MineState>>, seed: &mut u32) {
    new_mine_game(doc, &state, seed);
    let state_new = state.clone();
    if let Some(new_game) = node_id(doc, "#new-game") {
        doc.add_event_listener(
            new_game,
            "click",
            Box::new(move |_evt, doc| {
                let mut seed = 0x55aa_1234;
                new_mine_game(doc, &state_new, &mut seed);
            }),
            ListenerOptions::default(),
        );
    }

    let state_flag = state.clone();
    if let Some(flag_mode) = node_id(doc, "#flag-mode") {
        doc.add_event_listener(
            flag_mode,
            "click",
            Box::new(move |_evt, doc| {
                let mut st = state_flag.lock().unwrap();
                st.flag_mode = !st.flag_mode;
                if st.flag_mode {
                    add_class(doc, "#flag-mode", "btn-flag-active");
                } else {
                    remove_class(doc, "#flag-mode", "btn-flag-active");
                }
            }),
            ListenerOptions::default(),
        );
    }

    for idx in 0..81 {
        let selector = format!("#r{}c{}", idx / 9, idx % 9);
        let Some(cell) = node_id(doc, &selector) else {
            continue;
        };
        let state_cell = state.clone();
        doc.add_event_listener(
            cell,
            "click",
            Box::new(move |_evt, doc| {
                let cell_id = format!("r{}c{}", idx / 9, idx % 9);
                let mut st = state_cell.lock().unwrap();
                if st.flag_mode {
                    st.flagged[idx] = !st.flagged[idx];
                    set_text(
                        doc,
                        &format!("#{cell_id}"),
                        if st.flagged[idx] { "F" } else { "" },
                    );
                    if st.flagged[idx] {
                        add_class(doc, &format!("#{cell_id}"), "cell-flag");
                    } else {
                        remove_class(doc, &format!("#{cell_id}"), "cell-flag");
                    }
                    return;
                }
                if st.mines[idx] {
                    for i in 0..81 {
                        if st.mines[i] {
                            set_text(doc, &format!("#r{}c{}", i / 9, i % 9), "*");
                            add_class(doc, &format!("#r{}c{}", i / 9, i % 9), "cell-mine");
                        }
                    }
                    set_text(doc, "#status", "Game Over");
                    return;
                }
                reveal_mine_cell(doc, &mut st, idx);
                if (0..81).filter(|i| st.revealed[*i]).count() >= 71 {
                    set_text(doc, "#status", "You Win!");
                } else {
                    set_text(doc, "#status", "Keep going");
                }
            }),
            ListenerOptions::default(),
        );
    }
}

#[derive(Default)]
struct DomState {
    tick: i32,
    cpu: i32,
    mem: i32,
    rps: i32,
    err: f32,
    dark: bool,
    compact: bool,
    paused: bool,
    chaos: bool,
    interval_ms: u64,
    services: i32,
}

impl DomState {
    fn new() -> Self {
        Self {
            cpu: 23,
            mem: 3200,
            rps: 1420,
            err: 0.12,
            interval_ms: 1000,
            services: 4,
            ..Default::default()
        }
    }
}

fn install_dom(doc: &mut Document, state: Arc<Mutex<DomState>>) {
    dom_annotate_toolbar(doc);
    {
        let mut st = state.lock().unwrap();
        st.paused = false;
        st.chaos = false;
        render_dom_toolbar(doc, &st);
    }
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            let Some(id) = doc.closest(evt.target, ".tb-btn") else {
                return;
            };
            let action = doc.get_attribute(id, "data-dom-action").unwrap_or_default();
            let mut st = state.lock().unwrap();
            if action == "dark" {
                st.dark = !st.dark;
            } else if action == "compact" {
                st.compact = !st.compact;
            } else if action == "pause" {
                st.paused = !st.paused;
            } else if action == "chaos" {
                st.chaos = !st.chaos;
                add_alert(
                    doc,
                    if st.chaos {
                        "🔥 Chaos mode enabled"
                    } else {
                        "✅ Chaos mode disabled"
                    },
                );
            } else if action == "service" {
                st.services += 1;
                let svc = st.services;
                if let Some(tbody) = doc.query_selector("#svc-tbody") {
                    let row = doc.create_element("tr");
                    doc.append_child(tbody, row);
                    doc.set_attribute(row, "id", &format!("svc-x{svc}"));

                    let name = doc.create_element("td");
                    doc.append_child(row, name);
                    doc.set_text_content(name, "Extra Service");

                    let status = doc.create_element("td");
                    doc.append_child(row, status);
                    let badge = doc.create_element("span");
                    doc.append_child(status, badge);
                    doc.class_list_add(badge, "badge");
                    doc.class_list_add(badge, "badge-ok");
                    doc.set_text_content(badge, "HEALTHY");

                    let latency = doc.create_element("td");
                    doc.append_child(row, latency);
                    doc.set_text_content(latency, "7ms");
                }
                add_alert(doc, "✅ Added Extra Service");
            } else if action == "alerts" {
                clear_children(doc, "#alerts");
            } else if action == "feed" {
                clear_children(doc, "#activity-feed");
            }
            render_dom_toolbar(doc, &st);
        }),
        ListenerOptions::default(),
    );
}

fn dom_annotate_toolbar(doc: &mut Document) {
    for button in doc.query_selector_all(".tb-btn") {
        let label = doc.text_content(button);
        let action = if label.contains("Dark") {
            "dark"
        } else if label.contains("Compact") {
            "compact"
        } else if label.contains("Pause") || label.contains("Resume") {
            "pause"
        } else if label.contains("Chaos") || label.contains("Calm") {
            "chaos"
        } else if label.contains("Service") {
            "service"
        } else if label.contains("Alerts") {
            "alerts"
        } else if label.contains("Feed") {
            "feed"
        } else {
            ""
        };
        if !action.is_empty() {
            doc.set_attribute(button, "data-dom-action", action);
        }
    }
}

fn render_dom_toolbar(doc: &mut Document, st: &DomState) {
    set_dom_toggle(doc, "dark", st.dark, "🌙 Dark", "☀ Light");
    set_dom_toggle(doc, "compact", st.compact, "📏 Compact", "📐 Roomy");
    set_dom_toggle(doc, "pause", st.paused, "✍ Pause", "▶ Resume");
    set_dom_toggle(doc, "chaos", st.chaos, "🔥 Chaos", "🧯 Calm");
    set_root_class(doc, "dark", st.dark);
    set_root_class(doc, "compact", st.compact);
}

fn set_dom_toggle(doc: &mut Document, action: &str, active: bool, off_text: &str, on_text: &str) {
    let selector = &format!("[data-dom-action={action}]");
    if let Some(button) = doc.query_selector(selector) {
        doc.set_text_content(button, if active { on_text } else { off_text });
        if active {
            doc.class_list_add(button, "tb-active");
        } else {
            doc.class_list_remove(button, "tb-active");
        }
    }
}

fn set_root_class(doc: &mut Document, class: &str, enabled: bool) {
    if enabled {
        doc.class_list_add(doc.root.node_id, class);
    } else {
        doc.class_list_remove(doc.root.node_id, class);
    }
}

#[derive(Default)]
struct GraphState {
    clicks: u32,
    cycles: u32,
    refreshes: u32,
}

fn install_graph(doc: &mut Document, state: Arc<Mutex<GraphState>>, seed: &mut u32) {
    set_text(doc, "#status-text", "Central demo live handlers installed.");
    let root = doc.root.node_id;
    let state_graph = state.clone();
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            let mut st = state_graph.lock().unwrap();
            if let Some(id) = doc.closest(evt.target, "graph") {
                let types = [
                    "bar", "line", "area", "pie", "donut", "hbar", "scatter", "gauge",
                ];
                let current = doc
                    .get_attribute(id, "data-type")
                    .unwrap_or_else(|| "bar".to_string());
                let next = types
                    [(types.iter().position(|t| *t == current).unwrap_or(0) + 1) % types.len()];
                doc.set_attribute(id, "data-type", next);
                st.clicks += 1;
                st.cycles += 1;
                graph_status(doc, &st, &format!("Chart cycled to {next}"));
            } else if let Some(id) = doc.closest(evt.target, ".btn") {
                let button = doc.get_attribute(id, "id").unwrap_or_default();
                let mut seed = st.refreshes.wrapping_add(0x9876) as u32;
                apply_graph_button(doc, &button, &mut seed);
                st.clicks += 1;
                st.refreshes += 1;
                graph_status(doc, &st, &format!("Applied {button}"));
            } else if let Some(id) = doc.closest(evt.target, ".sb-item") {
                clear_class_prefix(doc, ".sb-item", "sb-item-active");
                add_class_id(doc, id, "sb-item-active");
                let item_id = doc.get_attribute(id, "id").unwrap_or_default();
                let mult = match item_id.as_str() {
                    "sb-home" | "sb-desktop" => 1.0,
                    "sb-products" => 0.75,
                    "sb-pricing" => 0.5,
                    "sb-blog" => 0.4,
                    "sb-docs" => 0.3,
                    "sb-about" => 0.2,
                    "sb-organic" => 1.1,
                    "sb-direct" => 0.65,
                    "sb-referral" => 0.35,
                    "sb-social" => 0.25,
                    "sb-mobile" => 0.6,
                    "sb-tablet" => 0.15,
                    _ => 1.0,
                };
                scale_graph_values(doc, mult);
                st.clicks += 1;
                graph_status(
                    doc,
                    &st,
                    &format!("Showing {item_id} at {:.0}%", mult * 100.0),
                );
            } else if let Some(id) = doc.closest(evt.target, ".kpi") {
                if doc.class_list_contains(id, "kpi-selected") {
                    doc.class_list_remove(id, "kpi-selected");
                } else {
                    doc.class_list_add(id, "kpi-selected");
                }
                st.clicks += 1;
                graph_status(doc, &st, "KPI selected");
            }
        }),
        ListenerOptions::default(),
    );
    apply_graph_button(doc, "btn-bar", seed);
}

fn install_markdown(doc: &mut Document, last_source: &mut String) {
    if let Some(source) = doc.query_selector(".md-source") {
        doc.set_attribute(source, "contenteditable", "true");
        doc.set_attribute(source, "tabindex", "0");
        doc.set_style_property(source, "caret-color", "#60a5fa");
    }
    *last_source = String::new();
    update_markdown_preview(doc, last_source);

    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            let Some(button) = doc.closest(evt.target, "button") else {
                return;
            };
            let label = doc.text_content(button);
            if label.contains("Get Markdown") {
                let bytes = markdown_source(doc).len();
                set_text(
                    doc,
                    ".toolbar-title",
                    &format!("Markdown Editor — {bytes} bytes"),
                );
            } else if label.contains("Get HTML") {
                let bytes = doc
                    .query_selector(".preview-body")
                    .map(|id| doc.inner_html(id).len())
                    .unwrap_or(0);
                set_text(
                    doc,
                    ".toolbar-title",
                    &format!("Rendered HTML — {bytes} bytes"),
                );
            } else if label.contains("Round-Trip") {
                set_text(doc, ".toolbar-title", "Round-trip preview refreshed");
                let mut last = String::new();
                update_markdown_preview(doc, &mut last);
            }
        }),
        ListenerOptions::default(),
    );
}

fn markdown_source(doc: &Document) -> String {
    doc.query_selector(".md-source")
        .map(|id| doc.text_content(id))
        .unwrap_or_default()
}

fn update_markdown_preview(doc: &mut Document, last_source: &mut String) -> bool {
    let source = markdown_source(doc);
    if source == *last_source {
        return false;
    }
    *last_source = source.clone();
    let parsed = parse_markdown(&source);
    let html = parsed
        .query_selector("body")
        .map(|body| parsed.inner_html(body))
        .unwrap_or_else(|| parsed.inner_html(parsed.root.node_id));
    if let Some(preview) = doc.query_selector(".preview-body") {
        doc.set_inner_html(preview, &html);
        true
    } else {
        false
    }
}

#[derive(Clone, Copy)]
enum EmailFilter {
    Mailbox(&'static str),
    Label(&'static str),
}

#[derive(Clone, Copy)]
struct EmailState {
    filter: EmailFilter,
    selected_id: &'static str,
    dark: bool,
}

impl Default for EmailState {
    fn default() -> Self {
        Self {
            filter: EmailFilter::Mailbox("Inbox"),
            selected_id: "alice",
            dark: false,
        }
    }
}

#[derive(Clone, Copy)]
struct EmailMessage {
    id: &'static str,
    mailbox: &'static str,
    sender: &'static str,
    to: &'static str,
    initials: &'static str,
    avatar_color: &'static str,
    subject: &'static str,
    date: &'static str,
    preview: &'static str,
    body_html: &'static str,
    labels: &'static [&'static str],
    unread: bool,
    starred: bool,
    attachment: bool,
    priority: &'static str,
}

const EMAIL_MESSAGES: &[EmailMessage] = &[
    EmailMessage {
        id: "alice",
        mailbox: "Inbox",
        sender: "Alice Chen",
        to: "you@example.com",
        initials: "AC",
        avatar_color: "#5b5fc7",
        subject: "Re: Project timeline update",
        date: "10:32 AM",
        preview: "Thanks for the update. I agree we should push the deadline to next Friday...",
        body_html: "<p>Hi,</p><p>Thanks for the update. I agree we should push the deadline to next Friday. I've already told the design team.</p><p>One question: should we include the new onboarding flow in this release, or defer it to v2.1?</p><p>Best,<br>Alice</p><blockquote><p style=\"font-size:9pt;opacity:0.7;\">On March 6, 2026, You wrote:</p><p>Just wanted to let you know that we're running a bit behind on the dashboard redesign.</p><p>I think we should push the milestone to next Friday.</p></blockquote>",
        labels: &["Work", "Urgent"],
        unread: true,
        starred: true,
        attachment: false,
        priority: "high",
    },
    EmailMessage {
        id: "bob",
        mailbox: "Inbox",
        sender: "Bob Martin",
        to: "you@example.com",
        initials: "BM",
        avatar_color: "#c74b50",
        subject: "Code review: PR #247",
        date: "9:15 AM",
        preview: "I left a few comments on your PR. Mostly minor stuff — naming...",
        body_html: "<p>Nice work on PR #247.</p><p>I left a few comments, mostly around naming and one edge case in the retry path.</p><p>The overall shape looks good to me.</p>",
        labels: &["Dev"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "ci",
        mailbox: "Inbox",
        sender: "CI Pipeline",
        to: "dev-team@example.com",
        initials: "CI",
        avatar_color: "#27ae60",
        subject: "Build #1842 passed",
        date: "8:44 AM",
        preview: "All 1164 tests passed. Branch: main. Duration: 2m 34s.",
        body_html: "<p>Build <code>#1842</code> passed.</p><table><tr><td>Branch</td><td>main</td></tr><tr><td>Tests</td><td>1164 passed</td></tr><tr><td>Duration</td><td>2m 34s</td></tr></table>",
        labels: &["Dev"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "carol",
        mailbox: "Inbox",
        sender: "Carol Davis",
        to: "you@example.com",
        initials: "CD",
        avatar_color: "#e67e22",
        subject: "Lunch today?",
        date: "Yesterday",
        preview: "Hey! Want to grab lunch today? I was thinking that new Thai place...",
        body_html: "<p>Hey!</p><p>Want to grab lunch today? I was thinking that new Thai place around the corner.</p><p>Carol</p>",
        labels: &["Personal"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "dave",
        mailbox: "Inbox",
        sender: "Dave Wilson",
        to: "you@example.com",
        initials: "DW",
        avatar_color: "#2980b9",
        subject: "Q1 Report draft attached",
        date: "Yesterday",
        preview: "Please find the Q1 report draft attached. Revenue up 12%...",
        body_html: "<p>Please find the Q1 report draft attached.</p><ul><li>Revenue up 12%</li><li>Gross margin improved</li><li>Forecast needs one more review</li></ul><p>Can you send notes by Thursday?</p>",
        labels: &["Finance", "Work"],
        unread: true,
        starred: false,
        attachment: true,
        priority: "",
    },
    EmailMessage {
        id: "eve",
        mailbox: "Inbox",
        sender: "Eve Thompson",
        to: "team@example.com",
        initials: "ET",
        avatar_color: "#8e44ad",
        subject: "Meeting notes - March 5",
        date: "Mar 5",
        preview: "Sprint Planning notes. Action items for Alice, Bob, You, Frank...",
        body_html: "<p>Here are the sprint planning notes.</p><h3>Action items</h3><ul><li>Alice: finalize onboarding mocks</li><li>Bob: review API retry patch</li><li>You: update release checklist</li><li>Frank: rate-limit implementation</li></ul>",
        labels: &["Team", "Work"],
        unread: false,
        starred: true,
        attachment: true,
        priority: "",
    },
    EmailMessage {
        id: "frank",
        mailbox: "Inbox",
        sender: "Frank Lee",
        to: "you@example.com",
        initials: "FL",
        avatar_color: "#16a085",
        subject: "Re: API rate limiting strategy",
        date: "Mar 5",
        preview: "I've pushed the initial implementation. Can you take a look at the...",
        body_html: "<p>I've pushed the initial implementation.</p><p>Can you take a look at the token bucket defaults? I used conservative limits until we get production numbers.</p>",
        labels: &["Work", "Dev"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "low",
    },
    EmailMessage {
        id: "grace",
        mailbox: "Inbox",
        sender: "Grace Kim",
        to: "design@example.com",
        initials: "GK",
        avatar_color: "#d35400",
        subject: "New brand guidelines v2",
        date: "Mar 4",
        preview: "Hi team, attached are the updated brand guidelines. Major changes...",
        body_html: "<p>Hi team,</p><p>Attached are the updated brand guidelines. Major changes include the revised color palette and icon spacing rules.</p><p>Grace</p>",
        labels: &["Work", "Design"],
        unread: false,
        starred: false,
        attachment: true,
        priority: "",
    },
    EmailMessage {
        id: "sent-release",
        mailbox: "Sent",
        sender: "You",
        to: "release-team@example.com",
        initials: "ME",
        avatar_color: "#0078d4",
        subject: "Release checklist updates",
        date: "Yesterday",
        preview: "I updated the checklist with QA sign-off and documentation owners...",
        body_html: "<p>I updated the checklist with QA sign-off and documentation owners.</p><p>Please review before the release sync.</p>",
        labels: &["Work", "Team"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "draft-onboarding",
        mailbox: "Drafts",
        sender: "Draft",
        to: "alice@example.com",
        initials: "DR",
        avatar_color: "#0078d4",
        subject: "Onboarding flow follow-up",
        date: "Draft",
        preview: "I think v2.1 is the safer target unless design can finish...",
        body_html: "<p>I think v2.1 is the safer target unless design can finish the empty states this week.</p>",
        labels: &["Work"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "spam-promo",
        mailbox: "Spam",
        sender: "Prize Desk",
        to: "you@example.com",
        initials: "PD",
        avatar_color: "#95a5a6",
        subject: "Final notice: claim your prize",
        date: "Mar 3",
        preview: "Congratulations, your account was selected...",
        body_html: "<p>Congratulations, your account was selected.</p><p>This message was classified as spam.</p>",
        labels: &["Urgent"],
        unread: true,
        starred: false,
        attachment: false,
        priority: "high",
    },
    EmailMessage {
        id: "trash-old",
        mailbox: "Trash",
        sender: "Old Newsletter",
        to: "you@example.com",
        initials: "ON",
        avatar_color: "#7f8c8d",
        subject: "Weekly product roundup",
        date: "Feb 28",
        preview: "Stories from around the web...",
        body_html: "<p>Stories from around the web.</p><p>This message is in Trash.</p>",
        labels: &["Personal"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
    EmailMessage {
        id: "archive-standup",
        mailbox: "Archive",
        sender: "Team Bot",
        to: "team@example.com",
        initials: "TB",
        avatar_color: "#1abc9c",
        subject: "Daily standup summary",
        date: "Feb 27",
        preview: "Yesterday: browser demos. Today: mail interactions...",
        body_html: "<p>Yesterday: browser demos.</p><p>Today: mail interactions and demo-browser polish.</p>",
        labels: &["Team"],
        unread: false,
        starred: false,
        attachment: false,
        priority: "",
    },
];

fn install_email(doc: &mut Document, state: Arc<Mutex<EmailState>>) {
    *state.lock().unwrap() = EmailState::default();
    email_annotate_navigation(doc);
    {
        let mut email_state = state.lock().unwrap();
        render_email_all(doc, &mut email_state);
    }
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            handle_email_click(doc, evt.target, &state);
        }),
        ListenerOptions::default(),
    );
}

fn email_annotate_navigation(doc: &mut Document) {
    const MAILBOXES: &[&str] = &[
        "Inbox", "Starred", "Sent", "Drafts", "Archive", "Spam", "Trash",
    ];
    for item in doc.query_selector_all(".sidebar-item") {
        let label = doc.text_content(item);
        if let Some(mailbox) = MAILBOXES.iter().find(|mailbox| label.contains(**mailbox)) {
            doc.set_attribute(item, "data-mailbox", mailbox);
        }
    }
    for item in doc.query_selector_all(".label-item") {
        let label = doc.text_content(item).trim().to_string();
        if !label.is_empty() {
            doc.set_attribute(item, "data-label", &label);
        }
    }
    for button in doc.query_selector_all("button") {
        if doc.get_attribute(button, "title").as_deref() == Some("Toggle dark mode") {
            doc.set_attribute(button, "data-email-action", "dark");
        }
    }
}

fn handle_email_click(doc: &mut Document, target: u32, state: &Arc<Mutex<EmailState>>) {
    if let Some(button) = doc.closest(target, "button") {
        if doc.get_attribute(button, "data-email-action").as_deref() == Some("dark") {
            let mut email_state = state.lock().unwrap();
            email_state.dark = !email_state.dark;
            render_email_all(doc, &mut email_state);
            return;
        }
    }
    if let Some(item) = doc.closest(target, ".sidebar-item") {
        if let Some(mailbox) = email_static_name(doc.get_attribute(item, "data-mailbox")) {
            let mut email_state = state.lock().unwrap();
            email_state.filter = EmailFilter::Mailbox(mailbox);
            email_select_first_visible(&mut email_state);
            render_email_all(doc, &mut email_state);
            return;
        }
    }
    if let Some(item) = doc.closest(target, ".label-item") {
        if let Some(label) = email_static_name(doc.get_attribute(item, "data-label")) {
            let mut email_state = state.lock().unwrap();
            email_state.filter = EmailFilter::Label(label);
            email_select_first_visible(&mut email_state);
            render_email_all(doc, &mut email_state);
            return;
        }
    }
    if let Some(item) = doc.closest(target, ".email-item") {
        if let Some(message_id) = email_static_id(doc.get_attribute(item, "data-email-id")) {
            let mut email_state = state.lock().unwrap();
            email_state.selected_id = message_id;
            render_email_all(doc, &mut email_state);
        }
    }
}

fn email_static_name(value: Option<String>) -> Option<&'static str> {
    match value.as_deref() {
        Some("Inbox") => Some("Inbox"),
        Some("Starred") => Some("Starred"),
        Some("Sent") => Some("Sent"),
        Some("Drafts") => Some("Drafts"),
        Some("Archive") => Some("Archive"),
        Some("Spam") => Some("Spam"),
        Some("Trash") => Some("Trash"),
        Some("Work") => Some("Work"),
        Some("Personal") => Some("Personal"),
        Some("Urgent") => Some("Urgent"),
        Some("Finance") => Some("Finance"),
        Some("Dev") => Some("Dev"),
        Some("Design") => Some("Design"),
        Some("Team") => Some("Team"),
        _ => None,
    }
}

fn email_static_id(value: Option<String>) -> Option<&'static str> {
    EMAIL_MESSAGES
        .iter()
        .find(|message| Some(message.id) == value.as_deref())
        .map(|message| message.id)
}

fn render_email_all(doc: &mut Document, state: &mut EmailState) {
    email_select_first_visible(state);
    render_email_theme(doc, state.dark);
    render_email_navigation(doc, state);
    render_email_list(doc, state);
    render_email_preview(doc, state);
}

fn render_email_theme(doc: &mut Document, dark: bool) {
    if dark {
        doc.class_list_add(doc.root.node_id, "dark");
        for button in doc.query_selector_all("button") {
            if doc.get_attribute(button, "data-email-action").as_deref() == Some("dark") {
                doc.set_text_content(button, "☀");
            }
        }
    } else {
        doc.class_list_remove(doc.root.node_id, "dark");
        for button in doc.query_selector_all("button") {
            if doc.get_attribute(button, "data-email-action").as_deref() == Some("dark") {
                doc.set_text_content(button, "☽");
            }
        }
    }
}

fn email_select_first_visible(state: &mut EmailState) {
    if EMAIL_MESSAGES.iter().any(|message| {
        message.id == state.selected_id && email_matches_filter(message, state.filter)
    }) {
        return;
    }
    state.selected_id = EMAIL_MESSAGES
        .iter()
        .find(|message| email_matches_filter(message, state.filter))
        .map(|message| message.id)
        .unwrap_or("alice");
}

fn render_email_navigation(doc: &mut Document, state: &EmailState) {
    for item in doc.query_selector_all(".sidebar-item") {
        doc.class_list_remove(item, "active");
        let active = match (
            state.filter,
            doc.get_attribute(item, "data-mailbox").as_deref(),
        ) {
            (EmailFilter::Mailbox(mailbox), Some(item_mailbox)) => mailbox == item_mailbox,
            _ => false,
        };
        if active {
            doc.class_list_add(item, "active");
        }
    }
    for item in doc.query_selector_all(".label-item") {
        doc.class_list_remove(item, "active");
        let active = match (
            state.filter,
            doc.get_attribute(item, "data-label").as_deref(),
        ) {
            (EmailFilter::Label(label), Some(item_label)) => label == item_label,
            _ => false,
        };
        if active {
            doc.class_list_add(item, "active");
        }
    }
}

fn render_email_list(doc: &mut Document, state: &EmailState) {
    let Some(list) = doc.query_selector(".email-list") else {
        return;
    };
    doc.set_text_content(list, "");
    let visible: Vec<&EmailMessage> = EMAIL_MESSAGES
        .iter()
        .filter(|message| email_matches_filter(message, state.filter))
        .collect();
    if visible.is_empty() {
        let empty = email_append_el(doc, list, "div", "email-item selected");
        doc.set_text_content(empty, "No messages in this view");
        return;
    }
    for (visible_idx, message) in visible.iter().enumerate() {
        render_email_row(doc, list, message, message.id == state.selected_id);
        if visible_idx + 1 < visible.len() {
            email_append_el(doc, list, "div", "email-divider");
        }
    }
}

fn render_email_row(doc: &mut Document, list: u32, message: &EmailMessage, selected: bool) {
    let row_class = if selected {
        "email-item selected"
    } else {
        "email-item"
    };
    let row = email_append_el(doc, list, "div", row_class);
    doc.set_attribute(row, "data-email-id", message.id);
    match message.priority {
        "high" => {
            email_append_el(doc, row, "span", "priority-bar priority-high");
        }
        "low" => {
            email_append_el(doc, row, "span", "priority-bar priority-low");
        }
        _ => {}
    }
    let avatar = email_append_el(doc, row, "div", "avatar");
    doc.set_attribute(
        avatar,
        "style",
        &format!("background:{};", message.avatar_color),
    );
    doc.set_text_content(avatar, message.initials);

    let inner = email_append_el(doc, row, "div", "email-item-inner");
    if message.unread {
        email_append_el(doc, inner, "span", "unread-dot");
    }
    let row1 = email_append_el(doc, inner, "div", "email-row1");
    let sender_class = if message.unread {
        "email-sender unread"
    } else {
        "email-sender"
    };
    let sender = email_append_el(doc, row1, "span", sender_class);
    doc.set_text_content(sender, message.sender);
    if message.starred {
        let star = email_append_el(doc, row1, "span", "star-icon");
        doc.set_text_content(star, "★");
    }
    if message.attachment {
        let attach = email_append_el(doc, row1, "span", "attach-icon");
        doc.set_attribute(attach, "title", "Has attachment");
        doc.set_text_content(attach, "📎");
    }
    let date = email_append_el(doc, row1, "span", "email-date");
    doc.set_text_content(date, message.date);

    let row2 = email_append_el(doc, inner, "div", "email-row2");
    let subject_class = if message.unread {
        "email-subject unread"
    } else {
        "email-subject"
    };
    let subject = email_append_el(doc, row2, "span", subject_class);
    doc.set_text_content(subject, message.subject);
    for label in message.labels {
        append_email_pill(doc, row2, label, "font-size:7.5pt;padding:1px 7px;");
    }
    let preview = email_append_el(doc, inner, "div", "email-preview");
    doc.set_text_content(preview, message.preview);
}

fn render_email_preview(doc: &mut Document, state: &EmailState) {
    let Some(message) = EMAIL_MESSAGES.iter().find(|message| {
        message.id == state.selected_id && email_matches_filter(message, state.filter)
    }) else {
        email_render_empty_preview(doc);
        return;
    };
    if let Some(badge) = doc.query_selector(".email-priority-badge") {
        match message.priority {
            "high" => {
                doc.set_style_property(badge, "display", "block");
                doc.set_text_content(badge, "HIGH PRIORITY");
            }
            "low" => {
                doc.set_style_property(badge, "display", "block");
                doc.set_text_content(badge, "LOW PRIORITY");
            }
            _ => {
                doc.set_style_property(badge, "display", "none");
                doc.set_text_content(badge, "");
            }
        }
    }
    set_text(doc, ".email-subject-title", message.subject);
    set_text(
        doc,
        ".email-meta",
        &format!("{} · to {} · {}", message.sender, message.to, message.date),
    );
    if let Some(labels) = doc.query_selector(".email-labels") {
        doc.set_text_content(labels, "");
        for label in message.labels {
            append_email_pill(doc, labels, label, "font-size:8pt;padding:2px 8px;");
        }
        if message.attachment {
            let note = email_append_el(doc, labels, "span", "pill");
            doc.set_attribute(
                note,
                "style",
                "background:rgba(136,136,136,0.15);color:#666;font-size:8pt;padding:2px 8px;",
            );
            doc.set_text_content(note, "Attachment");
        }
    }
    if let Some(body) = doc.query_selector(".email-body") {
        doc.set_inner_html(body, message.body_html);
    }
}

fn email_render_empty_preview(doc: &mut Document) {
    if let Some(badge) = doc.query_selector(".email-priority-badge") {
        doc.set_style_property(badge, "display", "none");
        doc.set_text_content(badge, "");
    }
    set_text(doc, ".email-subject-title", "No message selected");
    set_text(doc, ".email-meta", "");
    if let Some(labels) = doc.query_selector(".email-labels") {
        doc.set_text_content(labels, "");
    }
    if let Some(body) = doc.query_selector(".email-body") {
        doc.set_text_content(body, "Select a mailbox or message to preview it.");
    }
}

fn email_matches_filter(message: &EmailMessage, filter: EmailFilter) -> bool {
    match filter {
        EmailFilter::Mailbox("Starred") => message.starred,
        EmailFilter::Mailbox(mailbox) => message.mailbox == mailbox,
        EmailFilter::Label(label) => message
            .labels
            .iter()
            .any(|message_label| *message_label == label),
    }
}

fn append_email_pill(doc: &mut Document, parent: u32, label: &str, extra_style: &str) {
    let pill = email_append_el(doc, parent, "span", "pill");
    let (background, color) = email_label_style(label);
    doc.set_attribute(
        pill,
        "style",
        &format!("background:{background};color:{color};{extra_style}"),
    );
    doc.set_text_content(pill, label);
}

fn email_label_style(label: &str) -> (&'static str, &'static str) {
    match label {
        "Work" => ("rgba(91,95,199,0.15)", "#5b5fc7"),
        "Personal" => ("rgba(39,174,96,0.15)", "#27ae60"),
        "Urgent" => ("rgba(231,76,60,0.15)", "#e74c3c"),
        "Finance" => ("rgba(243,156,18,0.15)", "#f39c12"),
        "Dev" => ("rgba(52,152,219,0.15)", "#3498db"),
        "Design" => ("rgba(155,89,182,0.15)", "#9b59b6"),
        "Team" => ("rgba(26,188,156,0.15)", "#1abc9c"),
        _ => ("rgba(136,136,136,0.15)", "#666666"),
    }
}

fn email_append_el(doc: &mut Document, parent: u32, tag: &str, class_name: &str) -> u32 {
    let node = doc.create_element(tag);
    doc.append_child(parent, node);
    if !class_name.is_empty() {
        doc.set_attribute(node, "class", class_name);
    }
    node
}

#[derive(Clone, Copy)]
struct EudoraMail {
    from: &'static str,
    to: &'static str,
    cc: &'static str,
    subject: &'static str,
    date: &'static str,
    size: &'static str,
    body: &'static str,
    unread: bool,
    replied: bool,
    forwarded: bool,
    attach: bool,
    pri: &'static str,
    flagged: bool,
}

#[derive(Clone, Copy)]
struct EudoraContact {
    name: &'static str,
    email: &'static str,
    phone: &'static str,
    company: &'static str,
    title: &'static str,
    notes: &'static str,
    color: &'static str,
}

#[derive(Clone, Copy)]
struct EudoraFilter {
    name: &'static str,
    matcher: &'static str,
    action: &'static str,
    enabled: bool,
}

#[derive(Clone)]
struct EudoraState {
    in_sel: usize,
    out_sel: usize,
    contact_sel: usize,
    filter_enabled: [bool; EUDORA_FILTERS.len()],
    dark: bool,
}

impl Default for EudoraState {
    fn default() -> Self {
        Self {
            in_sel: 0,
            out_sel: 0,
            contact_sel: 0,
            filter_enabled: EUDORA_FILTERS.map(|f| f.enabled),
            dark: false,
        }
    }
}

const EUDORA_INBOX: &[EudoraMail] = &[
    EudoraMail {
        from: "Steve Dorner <sdorner@qualcomm.com>",
        to: "you@example.com",
        cc: "",
        subject: "Welcome to Eudora 2026!",
        date: "Mar 7, 2026 10:15 AM",
        size: "4K",
        body: "<p>Welcome to the new Eudora!</p><p>We've rebuilt Eudora from the ground up for 2026, keeping the spirit of the original while adding modern features.</p><ul><li>Rich HTML composition</li><li>Full dark mode support</li><li>Blazing fast search</li></ul><p>Best regards,<br>Steve</p>",
        unread: true,
        replied: false,
        forwarded: false,
        attach: false,
        pri: "highest",
        flagged: true,
    },
    EudoraMail {
        from: "Jeff Beckley <jbeckley@qualcomm.com>",
        to: "eudora-dev@list.example.com",
        cc: "sdorner@qualcomm.com; dpark@example.com",
        subject: "Re: Re: Re: Toolbar redesign proposal",
        date: "Mar 7, 2026 9:42 AM",
        size: "12K",
        body: "<p>Looks like we have consensus then. Ship it!</p><p>Jeff</p><blockquote><p>The spacing is perfect on macOS. Keyboard shortcuts in tooltips: yes, absolutely.</p></blockquote>",
        unread: true,
        replied: false,
        forwarded: false,
        attach: false,
        pri: "normal",
        flagged: false,
    },
    EudoraMail {
        from: "Dana Park <dpark@example.com>",
        to: "you@example.com",
        cc: "lwarren@design.example",
        subject: "📅 Calendar: Design Review Meeting",
        date: "Mar 7, 2026 8:00 AM",
        size: "6K",
        body: "<div class=\"cal-card\"><div class=\"cal-header\"><div class=\"cal-title\">📅 Design Review Meeting</div></div><div class=\"cal-body\"><p><b>Friday, March 14, 2026</b><br>2:00 PM – 3:30 PM</p><p>📍 Conference Room B</p><ol><li>Dashboard wireframe review</li><li>Color palette finalization</li></ol></div></div>",
        unread: true,
        replied: false,
        forwarded: false,
        attach: true,
        pri: "high",
        flagged: true,
    },
    EudoraMail {
        from: "Alan Strider <astrider@example.com>",
        to: "team@example.com",
        cc: "",
        subject: "Attachment: Q1 Budget Report",
        date: "Mar 6, 2026 4:18 PM",
        size: "842K",
        body: "<p>Hi team,</p><p>Q1 budget report attached. Engineering is under budget and server costs are down.</p>",
        unread: false,
        replied: false,
        forwarded: false,
        attach: true,
        pri: "normal",
        flagged: false,
    },
    EudoraMail {
        from: "Rick Langley <rick@example.com>",
        to: "you@example.com",
        cc: "",
        subject: "Lunch Friday?",
        date: "Mar 2, 2026 1:12 PM",
        size: "1K",
        body: "<p>Hey, want to grab lunch Friday? Thinking that new ramen place on 5th.</p><p>Rick</p>",
        unread: false,
        replied: false,
        forwarded: false,
        attach: false,
        pri: "none",
        flagged: false,
    },
];

const EUDORA_OUTBOX: &[EudoraMail] = &[
    EudoraMail {
        from: "you@example.com",
        to: "Steve Dorner <sdorner@qualcomm.com>",
        cc: "",
        subject: "Re: Welcome to Eudora 2026!",
        date: "Mar 7, 2026 10:45 AM",
        size: "3K",
        body: "<p>Thanks Steve! The new version is amazing.</p>",
        unread: false,
        replied: false,
        forwarded: false,
        attach: false,
        pri: "normal",
        flagged: false,
    },
    EudoraMail {
        from: "you@example.com",
        to: "team@example.com",
        cc: "astrider@example.com",
        subject: "Dashboard redesign - timeline update",
        date: "Mar 6, 2026 3:00 PM",
        size: "2K",
        body: "<p>Hi team,</p><p>Pushing milestone to next Friday.</p>",
        unread: false,
        replied: false,
        forwarded: false,
        attach: false,
        pri: "normal",
        flagged: false,
    },
];

const EUDORA_CONTACTS: &[EudoraContact] = &[
    EudoraContact {
        name: "Steve Dorner",
        email: "sdorner@qualcomm.com",
        phone: "(858) 555-0100",
        company: "Qualcomm",
        title: "Chief Architect",
        notes: "Creator of Eudora. Prefers plain text.",
        color: "#326ea5",
    },
    EudoraContact {
        name: "Jeff Beckley",
        email: "jbeckley@qualcomm.com",
        phone: "(858) 555-0101",
        company: "Qualcomm",
        title: "Senior Engineer",
        notes: "Toolbar and UI specialist.",
        color: "#a55532",
    },
    EudoraContact {
        name: "Dana Park",
        email: "dpark@example.com",
        phone: "(415) 555-0201",
        company: "TechStudio",
        title: "Design Lead",
        notes: "Conference co-speaker.",
        color: "#9b32a5",
    },
    EudoraContact {
        name: "Rick Langley",
        email: "rick@example.com",
        phone: "(510) 555-0300",
        company: "",
        title: "",
        notes: "College friend. Likes ramen.",
        color: "#d29922",
    },
];

const EUDORA_FILTERS: [EudoraFilter; 7] = [
    EudoraFilter {
        name: "Mailing Lists",
        matcher: "To contains @list.example",
        action: "Move to Mailing Lists",
        enabled: true,
    },
    EudoraFilter {
        name: "Newsletters",
        matcher: "From contains newsletter",
        action: "Move to Newsletters",
        enabled: true,
    },
    EudoraFilter {
        name: "Spam Keywords",
        matcher: "Subject contains winner|lottery",
        action: "Move to Junk",
        enabled: true,
    },
    EudoraFilter {
        name: "Work Priority",
        matcher: "From contains @qualcomm.com",
        action: "Set Priority: High, Flag",
        enabled: true,
    },
    EudoraFilter {
        name: "Large Attach",
        matcher: "Attachment Size > 5MB",
        action: "Add Label: Large",
        enabled: false,
    },
    EudoraFilter {
        name: "Auto-Reply OOO",
        matcher: "Subject is Out of Office",
        action: "Skip Inbox, Auto-Reply",
        enabled: true,
    },
    EudoraFilter {
        name: "Conference",
        matcher: "Subject contains conference|summit",
        action: "Move to Work/Projects",
        enabled: false,
    },
];

fn install_eudora(doc: &mut Document, state: Arc<Mutex<EudoraState>>) {
    *state.lock().unwrap() = EudoraState::default();
    render_eudora_all(doc, &state.lock().unwrap());
    let root = doc.root.node_id;
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            handle_eudora_click(doc, evt.target, &state);
        }),
        ListenerOptions::default(),
    );
}

fn handle_eudora_click(doc: &mut Document, target: u32, state: &Arc<Mutex<EudoraState>>) {
    if doc.closest(target, ".tab-close").is_some() {
        eudora_close_compose(doc);
        return;
    }
    if doc.closest(target, "#btn-dark").is_some() {
        let dark = {
            let mut st = state.lock().unwrap();
            st.dark = !st.dark;
            st.dark
        };
        eudora_toggle_dark(doc, dark);
        return;
    }
    if let Some(tab) = doc.closest(target, ".tab") {
        if let Some(name) = doc.get_attribute(tab, "data-tab") {
            eudora_switch_tab(doc, &name);
            return;
        }
    }
    if let Some(row) = doc.closest(target, "tr") {
        if let (Some(box_name), Some(idx)) = (
            doc.get_attribute(row, "data-box"),
            doc.get_attribute(row, "data-idx")
                .and_then(|idx| idx.parse::<usize>().ok()),
        ) {
            let mut st = state.lock().unwrap();
            if box_name == "in" {
                st.in_sel = idx.min(EUDORA_INBOX.len().saturating_sub(1));
                render_eudora_inbox(doc, &st);
            } else {
                st.out_sel = idx.min(EUDORA_OUTBOX.len().saturating_sub(1));
                render_eudora_outbox(doc, &st);
            }
            return;
        }
    }
    if let Some(card) = doc.closest(target, ".contact-card") {
        if let Some(idx) = doc
            .get_attribute(card, "data-contact")
            .and_then(|idx| idx.parse::<usize>().ok())
        {
            let mut st = state.lock().unwrap();
            st.contact_sel = idx.min(EUDORA_CONTACTS.len().saturating_sub(1));
            render_eudora_contacts(doc, &st);
            return;
        }
    }
    if let Some(email_btn) = doc.closest(target, ".contact-action-btn") {
        if doc.text_content(email_btn).contains("Email") {
            let idx = state.lock().unwrap().contact_sel;
            eudora_open_compose_to_contact(doc, idx);
            return;
        }
    }
    if let Some(toggle) = doc.closest(target, ".toggle-track") {
        if let Some(idx) = doc
            .get_attribute(toggle, "data-filter")
            .and_then(|idx| idx.parse::<usize>().ok())
        {
            let mut st = state.lock().unwrap();
            if let Some(enabled) = st.filter_enabled.get_mut(idx) {
                *enabled = !*enabled;
            }
            render_eudora_filters(doc, &st);
            return;
        }
    }
    if let Some(filter) = doc.closest(target, ".filter-card") {
        if let Some(idx) = doc
            .get_attribute(filter, "data-filter")
            .and_then(|idx| idx.parse::<usize>().ok())
        {
            eudora_set_status(doc, &format!("Selected filter #{}", idx + 1), "", "");
            return;
        }
    }
    if let Some(wazoo) = doc.closest(target, ".wazoo-item") {
        let label = doc.text_content(wazoo);
        if label.contains("In") {
            eudora_select_mailbox(doc, "in");
            return;
        }
        if label.contains("Out") {
            eudora_select_mailbox(doc, "out");
            return;
        }
    }
    if let Some(button) = doc.closest(target, "button") {
        let label = doc.text_content(button);
        if label.contains("Check Mail") {
            eudora_set_status(
                doc,
                "Checking mail...",
                "Connecting to mail.example.com",
                "",
            );
        } else if label.contains("New Message") {
            eudora_open_compose(doc);
        } else if label.contains("Reply") {
            let idx = state.lock().unwrap().in_sel;
            eudora_open_reply(doc, idx);
        } else if label.contains("Forward") {
            let idx = state.lock().unwrap().in_sel;
            eudora_open_forward(doc, idx);
        } else if label.contains("Trash") {
            eudora_set_status(doc, "Moved message to Trash", "", "");
        } else if label.contains("Send") {
            eudora_set_status(doc, "Message sent!", "", "");
            eudora_close_compose(doc);
        } else if label.contains("Queue") {
            eudora_set_status(doc, "Message queued in Out mailbox", "SMTP Ready", "");
            eudora_close_compose(doc);
        } else if label.contains("Discard") {
            eudora_close_compose(doc);
        }
    }
}

fn render_eudora_all(doc: &mut Document, st: &EudoraState) {
    render_eudora_inbox(doc, st);
    render_eudora_outbox(doc, st);
    render_eudora_contacts(doc, st);
    render_eudora_filters(doc, st);
}

fn render_eudora_inbox(doc: &mut Document, st: &EudoraState) {
    render_eudora_rows(doc, "#inbox-rows", EUDORA_INBOX, st.in_sel, "in");
    eudora_show_mail(doc, "in", EUDORA_INBOX, st.in_sel);
    let unread = EUDORA_INBOX.iter().filter(|mail| mail.unread).count();
    eudora_set_status(
        doc,
        &format!("In — {} messages, {unread} unread", EUDORA_INBOX.len()),
        "IMAP Idle",
        "Connected to mail.example.com",
    );
}

fn render_eudora_outbox(doc: &mut Document, st: &EudoraState) {
    render_eudora_rows(doc, "#outbox-rows", EUDORA_OUTBOX, st.out_sel, "out");
    eudora_show_mail(doc, "out", EUDORA_OUTBOX, st.out_sel);
}

fn render_eudora_rows(
    doc: &mut Document,
    tbody_selector: &str,
    mails: &[EudoraMail],
    selected_idx: usize,
    box_name: &str,
) {
    let Some(tbody) = doc.query_selector(tbody_selector) else {
        return;
    };
    doc.set_text_content(tbody, "");
    for (idx, mail) in mails.iter().enumerate() {
        let row = doc.create_element("tr");
        doc.append_child(tbody, row);
        let row_class = if idx == selected_idx {
            "selected"
        } else if mail.unread {
            "unread"
        } else {
            ""
        };
        if !row_class.is_empty() {
            doc.set_attribute(row, "class", row_class);
        }
        doc.set_attribute(row, "data-box", box_name);
        doc.set_attribute(row, "data-idx", &idx.to_string());

        eudora_append_marker_cell(doc, row, mail);
        eudora_append_priority_cell(doc, row, mail);
        eudora_append_td_text(doc, row, "col-a", if mail.attach { "📎" } else { "" });
        let flag = eudora_append_td_text(doc, row, "col-f", if mail.flagged { "⚑" } else { "" });
        doc.set_attribute(flag, "style", "color:#d23720");
        let who = if box_name == "out" {
            mail.to
        } else {
            mail.from
        };
        eudora_append_td_text(doc, row, "col-who", display_name(who));
        eudora_append_td_text(doc, row, "col-date", mail.date);
        eudora_append_td_text(doc, row, "col-size", mail.size);
        eudora_append_td_text(doc, row, "col-subj", mail.subject);
    }
}

fn eudora_append_td(doc: &mut Document, row: u32, class_name: &str) -> u32 {
    let cell = doc.create_element("td");
    doc.append_child(row, cell);
    doc.set_attribute(cell, "class", class_name);
    cell
}

fn eudora_append_td_text(doc: &mut Document, row: u32, class_name: &str, text: &str) -> u32 {
    let cell = eudora_append_td(doc, row, class_name);
    doc.set_text_content(cell, text);
    cell
}

fn eudora_append_marker_cell(doc: &mut Document, row: u32, mail: &EudoraMail) {
    let cell = eudora_append_td(doc, row, "col-s");
    if mail.unread {
        let dot = doc.create_element("span");
        doc.append_child(cell, dot);
        doc.set_attribute(dot, "class", "dot-unread");
    } else if mail.replied {
        doc.set_text_content(cell, "↩");
    } else if mail.forwarded {
        doc.set_text_content(cell, "→");
    }
}

fn eudora_append_priority_cell(doc: &mut Document, row: u32, mail: &EudoraMail) {
    let cell = eudora_append_td(doc, row, "col-p");
    match mail.pri {
        "highest" => {
            let span = doc.create_element("span");
            doc.append_child(cell, span);
            doc.set_attribute(span, "class", "pri-highest");
            doc.set_text_content(span, "▲▲");
        }
        "high" => {
            let span = doc.create_element("span");
            doc.append_child(cell, span);
            doc.set_attribute(span, "class", "pri-high");
            doc.set_text_content(span, "▲");
        }
        _ => {}
    }
}

fn eudora_show_mail(doc: &mut Document, prefix: &str, mails: &[EudoraMail], idx: usize) {
    let Some(mail) = mails.get(idx) else {
        return;
    };
    set_text(doc, &format!("#{prefix}-from"), mail.from);
    set_text(doc, &format!("#{prefix}-to"), mail.to);
    let cc_html = if mail.cc.is_empty() {
        String::new()
    } else {
        format!(
            "  <span style=\"color:#6a6560\">Cc:</span> {}",
            html_escape(mail.cc)
        )
    };
    if let Some(cc) = doc.query_selector(&format!("#{prefix}-cc-area")) {
        doc.set_inner_html(cc, &cc_html);
    }
    set_text(doc, &format!("#{prefix}-subj"), mail.subject);
    let mut meta = format!("Date: {}    Size: {}", mail.date, mail.size);
    if matches!(mail.pri, "high" | "highest") {
        meta.push_str("    Priority: HIGH");
    }
    if mail.attach {
        meta.push_str("    📎 Attachment");
    }
    set_text(doc, &format!("#{prefix}-meta"), &meta);
    if let Some(body) = doc.query_selector(&format!("#{prefix}-body")) {
        doc.set_inner_html(body, mail.body);
    }
}

fn render_eudora_contacts(doc: &mut Document, st: &EudoraState) {
    let list = EUDORA_CONTACTS
        .iter()
        .enumerate()
        .map(|(idx, contact)| {
            let initials = initials(contact.name);
            format!(
                "<div class=\"contact-card {}\" data-contact=\"{}\"><div class=\"avatar\" style=\"background:{}\">{}</div><div class=\"contact-info\"><div class=\"contact-name\">{}</div><div class=\"contact-title\">{}{}</div><div class=\"contact-email\">{}</div></div></div>",
                if idx == st.contact_sel { "selected" } else { "" },
                idx,
                contact.color,
                html_escape(&initials),
                html_escape(contact.name),
                html_escape(contact.title),
                if !contact.title.is_empty() && !contact.company.is_empty() { " • " } else { "" },
                html_escape(contact.email)
            )
        })
        .collect::<String>();
    if let Some(id) = doc.query_selector("#contact-list-inner") {
        doc.set_inner_html(id, &list);
    }
    render_eudora_contact_detail(doc, st.contact_sel);
}

fn render_eudora_contact_detail(doc: &mut Document, idx: usize) {
    let Some(contact) = EUDORA_CONTACTS.get(idx) else {
        return;
    };
    let html = format!(
        "<div class=\"contact-detail-card\"><div class=\"contact-banner\"><div class=\"contact-banner-inner\" style=\"background:{}\"></div></div><div class=\"contact-avatar-area\"><div class=\"contact-avatar-large\" style=\"background:{}\">{}</div><div class=\"contact-detail-name\">{}</div><div class=\"contact-detail-title\">{}{}</div></div><div class=\"contact-detail-body\"><hr class=\"contact-detail-divider\"><div class=\"contact-info-row\"><span class=\"contact-info-icon\">✉</span><div><div class=\"contact-info-label\">Email</div><div class=\"contact-info-value email\">{}</div></div></div><div class=\"contact-info-row\"><span class=\"contact-info-icon\">📞</span><div><div class=\"contact-info-label\">Phone</div><div class=\"contact-info-value\">{}</div></div></div><hr class=\"contact-detail-divider\"><div class=\"contact-info-label\" style=\"margin-bottom:4px;\">Notes</div><div style=\"font-size:9pt;color:#231e19\">{}</div><div class=\"contact-detail-actions\"><div class=\"contact-action-btn primary\">✉ Email</div><div class=\"contact-action-btn\">📞 Call</div><div class=\"contact-action-btn\" style=\"color:#78726c\">✏ Edit</div></div></div></div>",
        contact.color,
        contact.color,
        html_escape(&initials(contact.name)),
        html_escape(contact.name),
        html_escape(contact.title),
        if !contact.title.is_empty() && !contact.company.is_empty() {
            format!(" at {}", html_escape(contact.company))
        } else {
            String::new()
        },
        html_escape(contact.email),
        html_escape(contact.phone),
        html_escape(contact.notes)
    );
    if let Some(id) = doc.query_selector("#contact-detail-inner") {
        doc.set_inner_html(id, &html);
    }
}

fn render_eudora_filters(doc: &mut Document, st: &EudoraState) {
    let active = st.filter_enabled.iter().filter(|enabled| **enabled).count();
    set_text(
        doc,
        "#filters-meta",
        &format!(
            "{} rules • {active} active • {} disabled",
            EUDORA_FILTERS.len(),
            EUDORA_FILTERS.len() - active
        ),
    );
    let html = EUDORA_FILTERS
        .iter()
        .enumerate()
        .map(|(idx, filter)| {
            let enabled = st.filter_enabled[idx];
            let junk = filter.action.contains("Junk") || filter.action.contains("Skip");
            format!(
                "<div class=\"filter-card\" data-filter=\"{}\"><div class=\"filter-status-bar {}\"></div><div class=\"filter-toggle\"><div class=\"toggle-track {}\" data-filter=\"{}\"><div class=\"toggle-knob\"></div></div></div><div class=\"filter-body\"><div><span class=\"filter-num\">#{}</span> <span class=\"filter-name {}\">{}</span></div><div class=\"filter-row\"><span class=\"filter-pill if\">IF</span><span class=\"filter-text {}\">{}</span></div><div class=\"filter-arrow\">↓</div><div class=\"filter-row\"><span class=\"filter-pill then {}\">THEN</span><span class=\"filter-action-text {}\">{}</span></div></div><span class=\"filter-status-pill {}\">{}</span></div>",
                idx,
                if enabled { "active" } else { "inactive" },
                if enabled { "on" } else { "off" },
                idx,
                idx + 1,
                if enabled { "" } else { "inactive" },
                html_escape(filter.name),
                if enabled { "" } else { "inactive" },
                html_escape(filter.matcher),
                if junk { "junk" } else { "" },
                if enabled { "" } else { "inactive" },
                html_escape(filter.action),
                if enabled { "active" } else { "inactive" },
                if enabled { "Active" } else { "Off" }
            )
        })
        .collect::<String>();
    if let Some(id) = doc.query_selector("#filter-cards") {
        doc.set_inner_html(id, &html);
    }
}

fn eudora_switch_tab(doc: &mut Document, name: &str) {
    for tab in doc.query_selector_all(".tab") {
        doc.class_list_remove(tab, "selected");
        if doc.get_attribute(tab, "data-tab").as_deref() == Some(name) {
            doc.class_list_add(tab, "selected");
        }
    }
    for page in doc.query_selector_all(".page") {
        doc.class_list_remove(page, "active");
    }
    let page = match name {
        "in" => "#page-in",
        "out" => "#page-out",
        "contacts" => "#page-contacts",
        "filters" => "#page-filters",
        "compose" => "#page-compose",
        _ => "#page-in",
    };
    add_class(doc, page, "active");
    match name {
        "out" => eudora_set_status(
            doc,
            &format!("Out — {} messages", EUDORA_OUTBOX.len()),
            "SMTP Ready",
            "",
        ),
        "contacts" => eudora_set_status(
            doc,
            &format!("Contacts — {} people", EUDORA_CONTACTS.len()),
            "",
            "",
        ),
        "filters" => eudora_set_status(
            doc,
            &format!("Filters — {} rules", EUDORA_FILTERS.len()),
            "",
            "",
        ),
        "compose" => eudora_set_status(doc, "Composing message...", "", ""),
        _ => {
            let unread = EUDORA_INBOX.iter().filter(|mail| mail.unread).count();
            eudora_set_status(
                doc,
                &format!("In — {} messages, {unread} unread", EUDORA_INBOX.len()),
                "IMAP Idle",
                "Connected to mail.example.com",
            );
        }
    }
}

fn eudora_select_mailbox(doc: &mut Document, tab: &str) {
    for item in doc.query_selector_all(".wazoo-item") {
        doc.class_list_remove(item, "selected");
        let text = doc.text_content(item);
        if (tab == "in" && text.contains("In")) || (tab == "out" && text.contains("Out")) {
            doc.class_list_add(item, "selected");
        }
    }
    eudora_switch_tab(doc, tab);
}

fn eudora_open_compose(doc: &mut Document) {
    eudora_set_input_value(doc, "#compose-to", "");
    eudora_set_input_value(doc, "#compose-cc", "");
    eudora_set_input_value(doc, "#compose-bcc", "");
    eudora_set_input_value(doc, "#compose-subj", "");
    set_text(doc, "#compose-attach", "");
    set_text(doc, "#compose-priority", "Normal");
    set_text(doc, "#compose-tab", "✏ New Message ×");
    if let Some(body) = doc.query_selector("#compose-body") {
        doc.set_inner_html(
            body,
            "<p><br></p><p><br></p><div class=\"compose-sig\">— <br>Sent with Eudora 2026</div>",
        );
    }
    style(doc, "#compose-tab", "display", "flex");
    eudora_switch_tab(doc, "compose");
}

fn eudora_open_reply(doc: &mut Document, idx: usize) {
    let Some(mail) = EUDORA_INBOX.get(idx) else {
        return;
    };
    eudora_open_compose(doc);
    eudora_set_input_value(doc, "#compose-to", mail.from);
    eudora_set_input_value(
        doc,
        "#compose-subj",
        &format!(
            "{}{}",
            if mail.subject.starts_with("Re: ") {
                ""
            } else {
                "Re: "
            },
            mail.subject
        ),
    );
    set_text(
        doc,
        "#compose-priority",
        if matches!(mail.pri, "high" | "highest") {
            "High"
        } else {
            "Normal"
        },
    );
    set_text(doc, "#compose-tab", "↩ Reply ×");
    if let Some(body) = doc.query_selector("#compose-body") {
        doc.set_inner_html(
            body,
            &format!(
                "<p><br></p><div style=\"font-size:9pt;opacity:0.5;margin:16px 0 4px 0;\">At {}, {} wrote:</div><blockquote style=\"margin:0;padding:0 0 0 12px;border-left:3px solid #326ea5;\">{}</blockquote>",
                html_escape(mail.date),
                html_escape(mail.from),
                mail.body
            ),
        );
    }
}

fn eudora_open_forward(doc: &mut Document, idx: usize) {
    let Some(mail) = EUDORA_INBOX.get(idx) else {
        return;
    };
    eudora_open_compose(doc);
    eudora_set_input_value(
        doc,
        "#compose-subj",
        &format!(
            "{}{}",
            if mail.subject.starts_with("Fwd: ") {
                ""
            } else {
                "Fwd: "
            },
            mail.subject
        ),
    );
    set_text(doc, "#compose-tab", "→ Forward ×");
    if let Some(body) = doc.query_selector("#compose-body") {
        doc.set_inner_html(
            body,
            &format!(
                "<p><br></p><div style=\"font-size:9pt;opacity:0.5;margin:16px 0 4px 0;\">-------- Forwarded Message --------<br>From: {}<br>Date: {}</div><blockquote style=\"margin:0;padding:0 0 0 12px;border-left:3px solid #326ea5;\">{}</blockquote>",
                html_escape(mail.from),
                html_escape(mail.date),
                mail.body
            ),
        );
    }
}

fn eudora_open_compose_to_contact(doc: &mut Document, idx: usize) {
    eudora_open_compose(doc);
    if let Some(contact) = EUDORA_CONTACTS.get(idx) {
        eudora_set_input_value(doc, "#compose-to", contact.email);
    }
}

fn eudora_close_compose(doc: &mut Document) {
    style(doc, "#compose-tab", "display", "none");
    eudora_switch_tab(doc, "in");
}

fn eudora_set_status(doc: &mut Document, left: &str, center: &str, right: &str) {
    set_text(doc, "#sb-left", left);
    set_text(doc, "#sb-center", center);
    set_text(doc, "#sb-right", right);
}

fn eudora_set_input_value(doc: &mut Document, selector: &str, value: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.set_attribute(id, "value", value);
    }
}

fn eudora_toggle_dark(doc: &mut Document, dark: bool) {
    set_text(doc, "#btn-dark", if dark { "☀" } else { "☽" });
    set_root_class(doc, "dark", dark);
    if let Some(style) = doc.query_selector("#eudora-dark-style") {
        doc.remove(style);
    }
}

fn display_name(addr: &str) -> &str {
    addr.split('<').next().unwrap_or(addr).trim()
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|part| part.chars().next())
        .take(2)
        .collect()
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn install_print(doc: &mut Document) {
    let root = doc.root.node_id;
    apply_print_state(doc);
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            if doc.closest(evt.target, "#btn-print").is_some() {
                set_text(
                    doc,
                    "#notice",
                    "Print output is not implemented yet; preview refreshed.",
                );
            } else if doc.closest(evt.target, "#btn-preview").is_some()
                || doc.closest(evt.target, "#btn-settings").is_some()
            {
                set_text(doc, "#notice", "Print preview refreshed.");
            } else if doc.closest(evt.target, "#lbl-header").is_some()
                && doc.closest(evt.target, "#chk-header").is_none()
            {
                toggle_checked(doc, "#chk-header");
            } else if doc.closest(evt.target, "#lbl-footer").is_some()
                && doc.closest(evt.target, "#chk-footer").is_none()
            {
                toggle_checked(doc, "#chk-footer");
            }
            apply_print_state(doc);
        }),
        ListenerOptions::default(),
    );
}

fn install_forms(doc: &mut Document) {
    update_form_summary(doc);
    let root = doc.root.node_id;

    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            if doc.closest(evt.target, "#order-btn").is_some() {
                set_text(doc, "#status", "Order placed! Thank you!");
                doc.query_selector("#progress")
                    .map(|id| doc.set_attribute(id, "value", "1"));
            } else if doc.closest(evt.target, "#reset-btn").is_some() {
                for id in ["#name", "#phone", "#notes"] {
                    doc.query_selector(id)
                        .map(|node| doc.set_attribute(node, "value", ""));
                }
                for id in TOPPING_IDS {
                    doc.query_selector(&format!("#{id}"))
                        .map(|node| doc.remove_attribute(node, "checked"));
                }
                doc.query_selector("input[value=medium]")
                    .map(|id| doc.set_attribute(id, "checked", ""));
                doc.query_selector("#progress")
                    .map(|id| doc.set_attribute(id, "value", "0"));
                set_text(doc, "#status", "Order reset. Start fresh!");
            }
            update_form_summary(doc);
        }),
        ListenerOptions::default(),
    );

    doc.add_event_listener(
        root,
        "input",
        Box::new(move |_evt, doc| {
            update_form_summary(doc);
        }),
        ListenerOptions::default(),
    );

    doc.add_event_listener(
        root,
        "change",
        Box::new(move |_evt, doc| {
            update_form_summary(doc);
        }),
        ListenerOptions::default(),
    );
}

#[derive(Default)]
struct KanbanState {
    selected: Option<String>,
    drag: Option<KanbanDrag>,
    next_card: u32,
}

struct KanbanDrag {
    card_id: String,
    title: String,
    start: (f32, f32),
    active: bool,
    target_body: Option<String>,
}

const KANBAN_COLS: &[(&str, &str)] = &[
    ("col-backlog", "body-backlog"),
    ("col-todo", "body-todo"),
    ("col-progress", "body-progress"),
    ("col-review", "body-review"),
    ("col-done", "body-done"),
];

fn install_events(doc: &mut Document, state: Arc<Mutex<KanbanState>>, seed: &mut u32) {
    {
        let mut st = state.lock().unwrap();
        st.drag = None;
        st.selected = None;
        st.next_card = 13 + (next_rand(seed) % 1000);
    }
    update_kanban_counts(doc);
    let root = doc.root.node_id;

    let state_down = state.clone();
    doc.add_event_listener(
        root,
        "mousedown",
        Box::new(move |evt, doc| {
            if evt.button != 0 {
                return;
            }
            if let Some(card) = doc.closest(evt.target, ".card") {
                let card_id = doc.get_attribute(card, "id").unwrap_or_default();
                let title = card_title(doc, card).unwrap_or_else(|| card_id.clone());
                state_down.lock().unwrap().drag = Some(KanbanDrag {
                    card_id,
                    title,
                    start: (evt.client_x, evt.client_y),
                    active: false,
                    target_body: None,
                });
                evt.prevent_default();
            } else {
                select_kanban_card(doc, None);
                state_down.lock().unwrap().selected = None;
            }
        }),
        ListenerOptions::default(),
    );

    let state_move = state.clone();
    doc.add_event_listener(
        root,
        "mousemove",
        Box::new(move |evt, doc| {
            let mut st = state_move.lock().unwrap();
            let Some(drag) = st.drag.as_mut() else {
                set_text(
                    doc,
                    "#mouse-pos",
                    &format!("{:.0},{:.0}", evt.client_x, evt.client_y),
                );
                return;
            };
            let dx = evt.client_x - drag.start.0;
            let dy = evt.client_y - drag.start.1;
            if !drag.active && (dx * dx + dy * dy).sqrt() > 5.0 {
                drag.active = true;
                kanban_drag_start(doc, &drag.card_id, &drag.title);
                log_kanban(doc, "DRAG", &format!("Started {}", drag.title));
            }
            if drag.active {
                let target = find_kanban_target(doc, evt.client_x, evt.client_y);
                if target != drag.target_body {
                    drag.target_body = target.clone();
                    update_kanban_drop_highlights(doc, &target);
                }
                style(
                    doc,
                    "#drag-ghost",
                    "left",
                    &format!("{}px", evt.client_x + 12.0),
                );
                style(
                    doc,
                    "#drag-ghost",
                    "top",
                    &format!("{}px", evt.client_y + 8.0),
                );
                evt.prevent_default();
            }
        }),
        ListenerOptions::default(),
    );

    let state_up = state.clone();
    doc.add_event_listener(
        root,
        "mouseup",
        Box::new(move |evt, doc| {
            if evt.button != 0 {
                return;
            }
            let drag = state_up.lock().unwrap().drag.take();
            let Some(drag) = drag else {
                return;
            };
            if drag.active {
                let dropped = drag
                    .target_body
                    .as_deref()
                    .is_some_and(|target| move_kanban_card(doc, &drag.card_id, target));
                kanban_drag_end(doc, &drag.card_id);
                update_kanban_counts(doc);
                log_kanban(
                    doc,
                    if dropped { "DROP" } else { "CANCEL" },
                    &format!(
                        "{} {}",
                        drag.title,
                        if dropped { "moved" } else { "stayed" }
                    ),
                );
                evt.prevent_default();
            } else {
                select_kanban_card(doc, Some(&drag.card_id));
                state_up.lock().unwrap().selected = Some(drag.card_id.clone());
                log_kanban(doc, "SELECT", &drag.title);
            }
        }),
        ListenerOptions::default(),
    );

    let state_click = state.clone();
    doc.add_event_listener(
        root,
        "click",
        Box::new(move |evt, doc| {
            if doc.closest(evt.target, "#btn-add").is_some() {
                let mut st = state_click.lock().unwrap();
                let id = format!("card-{}", st.next_card);
                st.next_card += 1;
                append_kanban_card(doc, "body-backlog", &id, "New task", "feature", "med");
                update_kanban_counts(doc);
                log_kanban(doc, "ADD", "New task added");
            } else if doc.closest(evt.target, "#btn-delete").is_some()
                || doc.closest(evt.target, "#ctx-delete").is_some()
            {
                let selected = state_click.lock().unwrap().selected.clone();
                if let Some(card_id) = selected {
                    doc.query_selector(&format!("#{card_id}"))
                        .map(|id| doc.remove(id));
                    state_click.lock().unwrap().selected = None;
                    set_text(doc, "#selected-info", "none");
                    update_kanban_counts(doc);
                    log_kanban(doc, "DELETE", &card_id);
                }
            } else if doc.closest(evt.target, "#btn-left").is_some()
                || doc.closest(evt.target, "#ctx-move-left").is_some()
            {
                move_selected_kanban(doc, &state_click, -1);
            } else if doc.closest(evt.target, "#btn-right").is_some()
                || doc.closest(evt.target, "#ctx-move-right").is_some()
            {
                move_selected_kanban(doc, &state_click, 1);
            } else if doc.closest(evt.target, "#btn-search").is_some() {
                highlight_kanban_bugs(doc);
                log_kanban(doc, "SEARCH", "Bug cards highlighted");
            } else if doc.closest(evt.target, "#btn-shuffle").is_some() {
                shuffle_kanban(doc);
                update_kanban_counts(doc);
                log_kanban(doc, "SHUFFLE", "Cards redistributed");
            } else if doc.closest(evt.target, "#ctx-expand").is_some() {
                toggle_kanban_selected_class(doc, &state_click, "card-expanded");
            }
        }),
        ListenerOptions::default(),
    );

    let state_key = state.clone();
    doc.add_event_listener(
        root,
        "keydown",
        Box::new(move |evt, doc| {
            let key = if evt.key.is_empty() {
                key_name(evt.key_code)
            } else {
                evt.key.clone()
            };
            set_text(doc, "#key-display", &key);
            match key.to_ascii_lowercase().as_str() {
                "n" => {
                    let mut st = state_key.lock().unwrap();
                    let id = format!("card-{}", st.next_card);
                    st.next_card += 1;
                    append_kanban_card(
                        doc,
                        "body-backlog",
                        &id,
                        "New keyboard task",
                        "feature",
                        "med",
                    );
                    update_kanban_counts(doc);
                    log_kanban(doc, "KEY", "N added a task");
                }
                "delete" => {
                    if let Some(card_id) = state_key.lock().unwrap().selected.clone() {
                        doc.query_selector(&format!("#{card_id}"))
                            .map(|id| doc.remove(id));
                        state_key.lock().unwrap().selected = None;
                        set_text(doc, "#selected-info", "none");
                        update_kanban_counts(doc);
                    }
                }
                "escape" => {
                    select_kanban_card(doc, None);
                    state_key.lock().unwrap().selected = None;
                }
                "arrowleft" => move_selected_kanban(doc, &state_key, -1),
                "arrowright" => move_selected_kanban(doc, &state_key, 1),
                _ => {}
            }
        }),
        ListenerOptions::default(),
    );
}

#[derive(Default)]
struct PlaygroundState {
    counts: [u32; 7],
    log: VecDeque<PlaygroundLog>,
    wheel_count: u32,
    wheel_pos: f32,
    dragging: Option<String>,
}

struct PlaygroundLog {
    class: &'static str,
    tag: String,
    body: String,
}

fn install_event_playground(doc: &mut Document, state: Arc<Mutex<PlaygroundState>>) {
    {
        let mut st = state.lock().unwrap();
        *st = PlaygroundState::default();
        push_playground_log_locked(&mut st, 6, "DOMContentLoaded", "document ready");
        render_playground_state(doc, &st);
    }
    for id in doc.query_selector_all(".focus-item") {
        doc.set_attribute(id, "tabindex", "0");
    }
    let root = doc.root.node_id;

    for (event_type, index, selector, status_selector) in [
        ("mouseover", 0, ".hover-box", "#hover-status"),
        ("mouseout", 0, ".hover-box", "#hover-status"),
        ("mousemove", 0, ".hover-box", "#hover-status"),
        ("click", 0, ".click-btn", "#click-status"),
        ("dblclick", 0, ".click-btn", "#click-status"),
        ("contextmenu", 0, ".click-btn", "#click-status"),
        ("pointerdown", 1, "#pointer-canvas", "#pointer-status"),
        ("pointerup", 1, "#pointer-canvas", "#pointer-status"),
        ("pointermove", 1, "#pointer-canvas", "#pointer-status"),
        ("focus", 2, ".focus-item", "#focus-status"),
        ("blur", 2, ".focus-item", "#focus-status"),
        ("keydown", 3, "body", "#key-status"),
        ("wheel", 4, "#zone-wheel", "#wheel-delta-label"),
        ("mousedown", 5, ".drag-card", "#drag-status"),
        ("mousemove", 5, "#zone-drag", "#drag-status"),
        ("mouseup", 5, "#zone-drag", "#drag-status"),
    ] {
        let selector = selector.to_string();
        let status_selector = status_selector.to_string();
        let state_event = state.clone();
        doc.add_event_listener(
            root,
            event_type,
            Box::new(move |evt, doc| {
                if doc.closest(evt.target, &selector).is_none()
                    && !(selector == "body" && evt.event_type == "keydown")
                {
                    return;
                }
                handle_playground_event(doc, evt, &state_event, index, &selector, &status_selector);
            }),
            ListenerOptions::default(),
        );
    }
}

fn dom_tick(doc: &mut Document, state: &Arc<Mutex<DomState>>, seed: &mut u32) {
    let mut st = state.lock().unwrap();
    st.interval_ms = doc
        .query_selector("#speed-slider")
        .map(|id| doc.value(id))
        .and_then(|v| v.parse().ok())
        .unwrap_or(st.interval_ms);
    if st.paused {
        return;
    }
    st.tick += 1;
    let swing = if st.chaos { 25 } else { 7 };
    st.cpu = (st.cpu + rand_range(seed, -swing, swing)).clamp(3, 99);
    st.mem = (st.mem + rand_range(seed, -120, 120)).clamp(800, 7900);
    st.rps = (st.rps + rand_range(seed, -180, 180)).clamp(100, 6000);
    st.err = (st.err + rand_range(seed, -10, 10) as f32 / 100.0).clamp(0.0, 8.0);
    set_text(doc, "#cpu-val", &format!("{}%", st.cpu));
    set_text(doc, "#mem-val", &format!("{} MB", st.mem));
    set_text(doc, "#req-val", &st.rps.to_string());
    set_text(doc, "#err-val", &format!("{:.2}%", st.err));
    set_text(doc, "#clock", &format!("Uptime: 0d 0h 0m {}s", st.tick));
    set_text(doc, "#mem-big", &format!("{} / 8192 MB", st.mem));
    let mem_pct = st.mem * 100 / 8192;
    set_text(doc, "#mem-label", &format!("{mem_pct}% used"));
    style(doc, "#mem-bar", "width", &format!("{mem_pct}%"));
    if st.tick % 3 == 0 {
        prepend_feed(
            doc,
            &format!("{:02}:{:02}:{:02}  Live DOM update", 0, 0, st.tick % 60),
        );
    }
    if st.chaos && st.tick % 5 == 0 {
        add_alert(doc, "⚠ Error threshold exceeded");
    }
}

fn new_mine_game(doc: &mut Document, state: &Arc<Mutex<MineState>>, seed: &mut u32) {
    let mut st = state.lock().unwrap();
    *st = MineState::new();
    let mut placed = 0;
    while placed < 10 {
        let idx = (next_rand(seed) as usize) % 81;
        if !st.mines[idx] {
            st.mines[idx] = true;
            placed += 1;
        }
    }
    for idx in 0..81 {
        let r = idx / 9;
        let c = idx % 9;
        let mut adj = 0;
        for dr in -1..=1 {
            for dc in -1..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let rr = r as i32 + dr;
                let cc = c as i32 + dc;
                if (0..9).contains(&rr)
                    && (0..9).contains(&cc)
                    && st.mines[(rr as usize) * 9 + cc as usize]
                {
                    adj += 1;
                }
            }
        }
        st.adj[idx] = adj;
        let selector = format!("#r{r}c{c}");
        set_text(doc, &selector, "");
        for cls in [
            "cell-revealed",
            "cell-mine",
            "cell-flag",
            "cell-1",
            "cell-2",
            "cell-3",
            "cell-4",
            "cell-5",
            "cell-6",
            "cell-7",
            "cell-8",
        ] {
            remove_class(doc, &selector, cls);
        }
    }
    set_text(doc, "#mine-count", "10");
    set_text(doc, "#status", "Click to start");
}

fn reveal_mine_cell(doc: &mut Document, st: &mut MineState, idx: usize) {
    if idx >= 81 || st.revealed[idx] || st.flagged[idx] {
        return;
    }
    st.revealed[idx] = true;
    let selector = format!("#r{}c{}", idx / 9, idx % 9);
    add_class(doc, &selector, "cell-revealed");
    let adj = st.adj[idx];
    if adj > 0 {
        set_text(doc, &selector, &adj.to_string());
        add_class(doc, &selector, &format!("cell-{adj}"));
        return;
    }
    let r = idx / 9;
    let c = idx % 9;
    for dr in -1..=1 {
        for dc in -1..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let rr = r as i32 + dr;
            let cc = c as i32 + dc;
            if (0..9).contains(&rr) && (0..9).contains(&cc) {
                reveal_mine_cell(doc, st, rr as usize * 9 + cc as usize);
            }
        }
    }
}

fn finish_ttt(doc: &mut Document, st: &mut TicTacToeState) -> bool {
    if let Some((winner, line)) = winner(&st.board) {
        for idx in line {
            add_class(doc, &format!("#c{idx}"), "cell-win");
        }
        if winner == 'X' {
            st.x_score += 1;
        } else {
            st.o_score += 1;
        }
        set_text(doc, "#score-x", &st.x_score.to_string());
        set_text(doc, "#score-o", &st.o_score.to_string());
        set_text(doc, "#status", &format!("{winner} wins!"));
        true
    } else if st.board.iter().all(Option::is_some) {
        st.draws += 1;
        set_text(doc, "#score-d", &st.draws.to_string());
        set_text(doc, "#status", "Draw");
        true
    } else {
        false
    }
}

fn mark_cell(doc: &mut Document, idx: usize, value: char) {
    set_text(doc, &format!("#c{idx}"), &value.to_string());
    add_class(
        doc,
        &format!("#c{idx}"),
        if value == 'X' { "cell-x" } else { "cell-o" },
    );
}

fn winner(board: &[Option<char>; 9]) -> Option<(char, [usize; 3])> {
    for line in [
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8],
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8],
        [0, 4, 8],
        [2, 4, 6],
    ] {
        if let Some(value) = board[line[0]] {
            if board[line[1]] == Some(value) && board[line[2]] == Some(value) {
                return Some((value, line));
            }
        }
    }
    None
}

fn best_ai(board: &[Option<char>; 9]) -> Option<usize> {
    for i in 0..9 {
        if board[i].is_none() {
            let mut probe = *board;
            probe[i] = Some('O');
            if winner(&probe).is_some() {
                return Some(i);
            }
        }
    }
    for i in 0..9 {
        if board[i].is_none() {
            let mut probe = *board;
            probe[i] = Some('X');
            if winner(&probe).is_some() {
                return Some(i);
            }
        }
    }
    [4, 0, 2, 6, 8, 1, 3, 5, 7]
        .into_iter()
        .find(|i| board[*i].is_none())
}

fn apply_graph_button(doc: &mut Document, button: &str, seed: &mut u32) {
    let graph_ids = doc.query_selector_all("graph");
    for id in graph_ids {
        match button {
            "btn-line" => doc.set_attribute(id, "data-type", "line"),
            "btn-pie" => doc.set_attribute(id, "data-type", "pie"),
            "btn-area" => doc.set_attribute(id, "data-type", "area"),
            "btn-scatter" => doc.set_attribute(id, "data-type", "scatter"),
            "btn-rand" => {
                let len = parse_values(&doc.get_attribute(id, "data-values").unwrap_or_default())
                    .len()
                    .max(1);
                let values = (0..len)
                    .map(|_| (20 + (next_rand(seed) % 90)).to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                doc.set_attribute(id, "data-values", &values);
            }
            "btn-grow" | "btn-shrink" => {
                let factor = if button == "btn-grow" { 1.1 } else { 0.9 };
                let values =
                    parse_values(&doc.get_attribute(id, "data-values").unwrap_or_default())
                        .into_iter()
                        .map(|v| format!("{:.0}", v * factor))
                        .collect::<Vec<_>>()
                        .join(",");
                doc.set_attribute(id, "data-values", &values);
            }
            _ => doc.set_attribute(id, "data-type", "bar"),
        }
    }
}

fn scale_graph_values(doc: &mut Document, factor: f32) {
    for id in doc.query_selector_all("graph") {
        let values = parse_values(&doc.get_attribute(id, "data-values").unwrap_or_default())
            .into_iter()
            .map(|v| format!("{:.0}", v * factor))
            .collect::<Vec<_>>()
            .join(",");
        if !values.is_empty() {
            doc.set_attribute(id, "data-values", &values);
        }
    }
}

fn graph_status(doc: &mut Document, st: &GraphState, detail: &str) {
    set_text(doc, "#click-count", &st.clicks.to_string());
    set_text(doc, "#cycle-count", &st.cycles.to_string());
    set_text(doc, "#refresh-count", &st.refreshes.to_string());
    set_text(doc, "#status-text", detail);
    for n in (2..=5).rev() {
        let prev = text(doc, &format!("#log{}", n - 1));
        set_text(doc, &format!("#log{n}"), &prev);
    }
    set_text(doc, "#log1", detail);
}

fn apply_print_state(doc: &mut Document) {
    let header = checked(doc, "#chk-header");
    let footer = checked(doc, "#chk-footer");
    let scale = doc
        .query_selector("#scale-input")
        .map(|id| doc.value(id))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(100.0)
        .clamp(25.0, 400.0);
    doc.query_selector("#page-header")
        .map(|id| doc.set_style_property(id, "display", if header { "block" } else { "none" }));
    doc.query_selector("#page-footer")
        .map(|id| doc.set_style_property(id, "display", if footer { "block" } else { "none" }));
    doc.query_selector("#doc-content").map(|id| {
        doc.set_style_property(id, "font-size", &format!("{:.2}px", 16.0 * scale / 100.0))
    });
}

fn toggle_checked(doc: &mut Document, selector: &str) {
    if let Some(id) = doc.query_selector(selector) {
        if doc.get_attribute(id, "checked").is_some() {
            doc.remove_attribute(id, "checked");
        } else {
            doc.set_attribute(id, "checked", "");
        }
    }
}

fn checked(doc: &Document, selector: &str) -> bool {
    doc.query_selector(selector)
        .and_then(|id| doc.get_attribute(id, "checked"))
        .is_some()
}

fn clear_class_prefix(doc: &mut Document, selector: &str, class: &str) {
    for id in doc.query_selector_all(selector) {
        remove_class_id(doc, id, class);
    }
}

fn add_class_id(doc: &mut Document, id: u32, class: &str) {
    doc.class_list_add(id, class);
}

fn remove_class_id(doc: &mut Document, id: u32, class: &str) {
    doc.class_list_remove(id, class);
}

fn set_text(doc: &mut Document, selector: &str, value: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.set_text_content(id, value);
    }
}

fn text(doc: &Document, selector: &str) -> String {
    doc.query_selector(selector)
        .map(|id| doc.text_content(id))
        .unwrap_or_default()
}

fn style(doc: &mut Document, selector: &str, prop: &str, value: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.set_style_property(id, prop, value);
    }
}

fn add_class(doc: &mut Document, selector: &str, class: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.class_list_add(id, class);
    }
}

fn remove_class(doc: &mut Document, selector: &str, class: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.class_list_remove(id, class);
    }
}

fn node_id(doc: &Document, selector: &str) -> Option<u32> {
    dom::query_selector(&doc.root, selector).map(|node| node.node_id)
}

fn clear_children(doc: &mut Document, selector: &str) {
    if let Some(id) = doc.query_selector(selector) {
        doc.set_text_content(id, "");
    }
}

fn prepend_feed(doc: &mut Document, message: &str) {
    if let Some(feed) = doc.query_selector("#activity-feed") {
        let item = doc.create_element("div");
        if let Some(first) = doc.child_nodes(feed).first().copied() {
            doc.insert_before(feed, item, first);
        } else {
            doc.append_child(feed, item);
        }
        doc.class_list_add(item, "feed-item");
        doc.set_text_content(item, message);
        for child in doc.child_nodes(feed).into_iter().skip(10) {
            doc.remove_child(child);
        }
    }
}

fn add_alert(doc: &mut Document, message: &str) {
    if let Some(alerts) = doc.query_selector("#alerts") {
        let item = doc.create_element("div");
        doc.append_child(alerts, item);
        doc.class_list_add(item, "alert");
        doc.class_list_add(item, "alert-warn");
        doc.set_text_content(item, message);
    }
}

const TOPPING_IDS: &[&str] = &[
    "pep", "mush", "onion", "saus", "pepper", "olive", "cheese", "jala",
];

fn update_form_summary(doc: &mut Document) {
    let name = doc
        .query_selector("#name")
        .map(|id| doc.value(id))
        .unwrap_or_default();
    let size = ["small", "medium", "large"]
        .into_iter()
        .find(|value| {
            doc.query_selector(&format!("input[value={value}]"))
                .is_some_and(|id| doc.get_attribute(id, "checked").is_some())
        })
        .unwrap_or("medium");
    let base = match size {
        "small" => 9.99,
        "large" => 19.99,
        _ => 14.99,
    };
    let mut toppings = Vec::new();
    let mut extras = 0.0;
    for id in TOPPING_IDS {
        let selector = format!("#{id}");
        let Some(node) = doc.query_selector(&selector) else {
            continue;
        };
        if doc.get_attribute(node, "checked").is_none() {
            continue;
        }
        if let Some(label) = doc.get_attribute(node, "data-label") {
            toppings.push(label);
        }
        extras += doc
            .get_attribute(node, "data-price")
            .and_then(|price| price.parse::<f32>().ok())
            .unwrap_or(0.0);
    }
    let total = base + extras;
    set_text(
        doc,
        "#sum-name",
        if name.trim().is_empty() { "—" } else { &name },
    );
    set_text(doc, "#sum-size", &format!("{size} (${base:.2})"));
    let topping_text = if toppings.is_empty() {
        "None".to_string()
    } else {
        toppings.join(", ")
    };
    set_text(doc, "#sum-toppings", &topping_text);
    set_text(doc, "#sum-total", &format!("${total:.2}"));
    doc.query_selector("#order-btn")
        .map(|id| doc.set_attribute(id, "value", &format!("Place Order — ${total:.2}")));
}

fn select_kanban_card(doc: &mut Document, card_id: Option<&str>) {
    for id in doc.query_selector_all(".card") {
        remove_class_id(doc, id, "card-selected");
    }
    if let Some(card_id) = card_id {
        add_class(doc, &format!("#{card_id}"), "card-selected");
        let title = doc
            .query_selector(&format!("#{card_id}"))
            .and_then(|id| card_title(doc, id))
            .unwrap_or_else(|| card_id.to_string());
        set_text(doc, "#selected-info", &format!("{title} ({card_id})"));
        enable_kanban_action_buttons(doc, true);
    } else {
        set_text(doc, "#selected-info", "none");
        enable_kanban_action_buttons(doc, false);
    }
}

fn enable_kanban_action_buttons(doc: &mut Document, enabled: bool) {
    for selector in ["#btn-left", "#btn-right", "#btn-delete"] {
        if enabled {
            remove_class(doc, selector, "btn-disabled");
        } else {
            add_class(doc, selector, "btn-disabled");
        }
    }
}

fn card_title(doc: &Document, card: u32) -> Option<String> {
    fn walk(node: &WebCore) -> Option<String> {
        if dom::has_class(node, "card-title") {
            return Some(dom::get_text_content(node));
        }
        node.children.iter().find_map(walk)
    }
    doc.get_node(card).and_then(walk)
}

fn find_kanban_target(doc: &Document, x: f32, y: f32) -> Option<String> {
    for (col_id, body_id) in KANBAN_COLS {
        for selector in [format!("#{body_id}"), format!("#{col_id}")] {
            let Some(id) = doc.query_selector(&selector) else {
                continue;
            };
            let Some(node) = doc.get_node(id) else {
                continue;
            };
            let r = node.layout.border_rect;
            if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                return Some((*body_id).to_string());
            }
        }
    }
    None
}

fn kanban_drag_start(doc: &mut Document, card_id: &str, title: &str) {
    add_class(doc, &format!("#{card_id}"), "card-dragging");
    add_class(doc, "#drag-banner", "drag-banner-visible");
    add_class(doc, "#drag-ghost", "drag-ghost-visible");
    set_text(doc, "#drag-title", title);
    set_text(doc, "#ghost-title", title);
    for (_, body_id) in KANBAN_COLS {
        style(
            doc,
            &format!("#ph-{}", &body_id["body-".len()..]),
            "display",
            "block",
        );
    }
}

fn kanban_drag_end(doc: &mut Document, card_id: &str) {
    remove_class(doc, &format!("#{card_id}"), "card-dragging");
    remove_class(doc, "#drag-banner", "drag-banner-visible");
    remove_class(doc, "#drag-ghost", "drag-ghost-visible");
    update_kanban_drop_highlights(doc, &None);
    for (_, body_id) in KANBAN_COLS {
        style(
            doc,
            &format!("#ph-{}", &body_id["body-".len()..]),
            "display",
            "none",
        );
    }
}

fn update_kanban_drop_highlights(doc: &mut Document, target: &Option<String>) {
    for (col_id, _) in KANBAN_COLS {
        remove_class(doc, &format!("#{col_id}"), "column-drop-active");
    }
    if let Some(body_id) = target {
        let col_id = format!("col-{}", &body_id["body-".len()..]);
        add_class(doc, &format!("#{col_id}"), "column-drop-active");
        set_text(
            doc,
            "#drag-target-col",
            &format!("→ {}", &body_id["body-".len()..]),
        );
    } else {
        set_text(doc, "#drag-target-col", "");
    }
}

fn move_kanban_card(doc: &mut Document, card_id: &str, target_body: &str) -> bool {
    let Some(card_node) = doc.query_selector(&format!("#{card_id}")) else {
        return false;
    };
    let current = KANBAN_COLS
        .iter()
        .find(|(_, body_id)| {
            doc.query_selector(&format!("#{body_id}"))
                .is_some_and(|body_node| doc.contains(body_node, card_node))
        })
        .map(|(_, body_id)| *body_id);
    if current == Some(target_body) {
        return false;
    }
    doc.remove_child(card_node);
    if let Some(target) = doc.query_selector(&format!("#{target_body}")) {
        doc.append_child(target, card_node);
        remove_class(doc, &format!("#{card_id}"), "card-dragging");
        true
    } else {
        false
    }
}

fn move_selected_kanban(doc: &mut Document, state: &Arc<Mutex<KanbanState>>, delta: isize) {
    let selected = state.lock().unwrap().selected.clone();
    let Some(card_id) = selected else {
        return;
    };
    let Some(card_node) = doc.query_selector(&format!("#{card_id}")) else {
        return;
    };
    let Some(index) = KANBAN_COLS.iter().position(|(_, body_id)| {
        doc.query_selector(&format!("#{body_id}"))
            .is_some_and(|body| doc.contains(body, card_node))
    }) else {
        return;
    };
    let next = (index as isize + delta).clamp(0, KANBAN_COLS.len() as isize - 1) as usize;
    if next != index && move_kanban_card(doc, &card_id, KANBAN_COLS[next].1) {
        update_kanban_counts(doc);
        log_kanban(doc, "MOVE", &format!("{card_id} moved"));
    }
}

fn toggle_kanban_selected_class(doc: &mut Document, state: &Arc<Mutex<KanbanState>>, class: &str) {
    let selected = state.lock().unwrap().selected.clone();
    if let Some(card_id) = selected {
        if let Some(id) = doc.query_selector(&format!("#{card_id}")) {
            if doc.class_list_contains(id, class) {
                doc.class_list_remove(id, class);
            } else {
                doc.class_list_add(id, class);
            }
        }
    }
}

fn append_kanban_card(
    doc: &mut Document,
    body_id: &str,
    id: &str,
    title: &str,
    tag: &str,
    priority: &str,
) {
    let card = doc.create_element("div");
    doc.set_attribute(card, "id", id);
    doc.set_attribute(card, "class", &format!("card priority-{priority}"));
    let title_el = doc.create_element("div");
    doc.set_attribute(title_el, "class", "card-title");
    doc.set_text_content(title_el, title);
    let meta = doc.create_element("div");
    doc.set_attribute(meta, "class", "card-meta");
    let tag_el = doc.create_element("span");
    doc.set_attribute(tag_el, "class", &format!("card-tag tag-{tag}"));
    doc.set_text_content(tag_el, tag);
    let prio_el = doc.create_element("span");
    doc.set_text_content(prio_el, priority);
    let desc = doc.create_element("div");
    doc.set_attribute(desc, "class", "card-desc");
    doc.set_text_content(desc, "Created from the centralized demo browser.");
    doc.append_child(meta, tag_el);
    doc.append_child(meta, prio_el);
    doc.append_child(card, title_el);
    doc.append_child(card, meta);
    doc.append_child(card, desc);
    if let Some(body) = doc.query_selector(&format!("#{body_id}")) {
        doc.append_child(body, card);
    }
}

fn update_kanban_counts(doc: &mut Document) {
    let mut total = 0usize;
    for (_, body_id) in KANBAN_COLS {
        let count = doc
            .query_selector(&format!("#{body_id}"))
            .and_then(|id| doc.get_node(id))
            .map(|body| {
                body.children
                    .iter()
                    .filter(|child| dom::has_class(child, "card"))
                    .count()
            })
            .unwrap_or(0);
        total += count;
        set_text(
            doc,
            &format!("#cnt-{}", &body_id["body-".len()..]),
            &count.to_string(),
        );
    }
    set_text(doc, "#total-count", &total.to_string());
}

fn highlight_kanban_bugs(doc: &mut Document) {
    for id in doc.query_selector_all(".card") {
        if doc.get_node(id).is_some_and(|node| {
            dom::get_text_content(node)
                .to_ascii_lowercase()
                .contains("bug")
        }) {
            doc.class_list_add(id, "card-selected");
        } else {
            doc.class_list_remove(id, "card-selected");
        }
    }
}

fn shuffle_kanban(doc: &mut Document) {
    let cards = doc.query_selector_all(".card");
    for (i, card) in cards.into_iter().enumerate() {
        let body = KANBAN_COLS[i % KANBAN_COLS.len()].1;
        doc.remove_child(card);
        if let Some(target) = doc.query_selector(&format!("#{body}")) {
            doc.append_child(target, card);
        }
    }
}

fn log_kanban(doc: &mut Document, event_type: &str, message: &str) {
    for n in (2..=5).rev() {
        let prev = text(doc, &format!("#log-{}", n - 1));
        set_text(doc, &format!("#log-{n}"), &prev);
    }
    set_text(doc, "#log-1", &format!("[now] {event_type} {message}"));
    set_text(doc, "#last-action", message);
}

fn key_name(key_code: u32) -> String {
    match key_code {
        8 => "Backspace",
        9 => "Tab",
        13 => "Enter",
        27 => "Escape",
        32 => "Space",
        37 => "ArrowLeft",
        39 => "ArrowRight",
        46 => "Delete",
        code if (32..=126).contains(&code) => {
            return char::from_u32(code).unwrap_or('?').to_string();
        }
        _ => "Key",
    }
    .to_string()
}

fn handle_playground_event(
    doc: &mut Document,
    evt: &mut webcore::dom::events::DomEvent,
    state: &Arc<Mutex<PlaygroundState>>,
    index: usize,
    selector: &str,
    status_selector: &str,
) {
    let label = match evt.event_type.as_str() {
        "keydown" => {
            let key = if evt.key.is_empty() {
                key_name(evt.key_code)
            } else {
                evt.key.clone()
            };
            set_text(doc, "#key-display", &key);
            format!("KeyDown {key}")
        }
        "wheel" => {
            let mut st = state.lock().unwrap();
            st.wheel_count += 1;
            st.wheel_pos = st.wheel_pos * 0.75 + evt.delta_y.clamp(-60.0, 60.0) * 0.5;
            set_text(doc, "#wheel-count", &st.wheel_count.to_string());
            let bar_pct = (50.0 + st.wheel_pos.clamp(-50.0, 50.0)).clamp(0.0, 100.0) as u32;
            style(doc, "#wheel-bar", "width", &format!("{bar_pct}%"));
            format!("Wheel dx={:.1} dy={:.1}", evt.delta_x, evt.delta_y)
        }
        "pointermove" => {
            if let Some(canvas) = doc.closest(evt.target, "#pointer-canvas") {
                if let Some(node) = doc.get_node(canvas) {
                    let r = node.layout.border_rect;
                    style(
                        doc,
                        "#pointer-dot",
                        "left",
                        &format!("{}px", (evt.client_x - r.x - 7.0).max(0.0)),
                    );
                    style(
                        doc,
                        "#pointer-dot",
                        "top",
                        &format!("{}px", (evt.client_y - r.y - 7.0).max(0.0)),
                    );
                }
            }
            format!("PointerMove ({:.0},{:.0})", evt.client_x, evt.client_y)
        }
        "mouseover" => {
            if let Some(id) = doc.closest(evt.target, selector) {
                add_class_id(doc, id, "hover-box-active");
            }
            "MouseOver".to_string()
        }
        "mouseout" => {
            if let Some(id) = doc.closest(evt.target, selector) {
                remove_class_id(doc, id, "hover-box-active");
            }
            "MouseOut".to_string()
        }
        "focus" => {
            if let Some(id) = doc.closest(evt.target, ".focus-item") {
                add_class_id(doc, id, "focus-item-focused");
                let focus_id = doc.get_attribute(id, "id").unwrap_or_default();
                add_class(
                    doc,
                    &format!("#{}", focus_id.replace("focus-item", "focus-dot")),
                    "focus-dot-on",
                );
            }
            "Focus".to_string()
        }
        "blur" => {
            if let Some(id) = doc.closest(evt.target, ".focus-item") {
                remove_class_id(doc, id, "focus-item-focused");
                let focus_id = doc.get_attribute(id, "id").unwrap_or_default();
                remove_class(
                    doc,
                    &format!("#{}", focus_id.replace("focus-item", "focus-dot")),
                    "focus-dot-on",
                );
            }
            "Blur".to_string()
        }
        "mousedown" => {
            if let Some(id) = doc.closest(evt.target, ".drag-card") {
                let drag_id = doc.get_attribute(id, "id").unwrap_or_default();
                add_class_id(doc, id, "drag-card-dragging");
                state.lock().unwrap().dragging = Some(drag_id.clone());
                format!("DragStart {drag_id}")
            } else {
                "MouseDown".to_string()
            }
        }
        "mousemove" => {
            if state.lock().unwrap().dragging.is_some() {
                format!("Drag ({:.0},{:.0})", evt.client_x, evt.client_y)
            } else {
                "MouseMove".to_string()
            }
        }
        "mouseup" => {
            let dragging = state.lock().unwrap().dragging.take();
            for id in doc.query_selector_all(".drag-card") {
                remove_class_id(doc, id, "drag-card-dragging");
            }
            format!("DragEnd {}", dragging.unwrap_or_default())
        }
        other => other.to_string(),
    };
    set_text(doc, status_selector, &label);
    let mut st = state.lock().unwrap();
    push_playground_log_locked(
        &mut st,
        index,
        &label,
        &format!("({:.0},{:.0})", evt.client_x, evt.client_y),
    );
    render_playground_state(doc, &st);
}

fn push_playground_log_locked(st: &mut PlaygroundState, index: usize, tag: &str, body: &str) {
    let class = match index {
        0 => "log-tag-mouse",
        1 => "log-tag-pointer",
        2 => "log-tag-focus",
        3 => "log-tag-key",
        4 => "log-tag-wheel",
        5 => "log-tag-drag",
        _ => "log-tag-life",
    };
    if let Some(count) = st.counts.get_mut(index) {
        *count += 1;
    }
    st.log.push_front(PlaygroundLog {
        class,
        tag: tag.to_string(),
        body: body.to_string(),
    });
    st.log.truncate(20);
}

fn render_playground_state(doc: &mut Document, st: &PlaygroundState) {
    for (selector, count) in [
        ("#stat-mouse", st.counts[0]),
        ("#stat-pointer", st.counts[1]),
        ("#stat-focus", st.counts[2]),
        ("#stat-key", st.counts[3]),
        ("#stat-wheel", st.counts[4]),
        ("#stat-drag", st.counts[5]),
        ("#stat-life", st.counts[6]),
    ] {
        set_text(doc, selector, &count.to_string());
    }
    set_text(
        doc,
        "#stat-total",
        &st.counts.iter().sum::<u32>().to_string(),
    );
    for i in 0..20 {
        let selector = format!("#log-{i}");
        if let Some(entry) = st.log.get(i) {
            set_text(doc, &selector, &format!("[{}] {}", entry.tag, entry.body));
            if let Some(id) = doc.query_selector(&selector) {
                doc.set_attribute(id, "class", "log-line");
                doc.set_attribute(id, "data-kind", entry.class);
            }
        }
    }
}

fn attr(node: &WebCore, key: &str, default: &str) -> String {
    node.attributes
        .get(key)
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn parse_values(raw: &str) -> Vec<f32> {
    raw.split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect()
}

fn next_rand(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    *seed
}

fn rand_range(seed: &mut u32, min: i32, max: i32) -> i32 {
    min + (next_rand(seed) % (max - min + 1) as u32) as i32
}

fn eval_expression(expr: &str) -> Result<f64, String> {
    let mut values = Vec::<f64>::new();
    let mut ops = Vec::<char>::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            values.push(expr[start..i].parse::<f64>().map_err(|e| e.to_string())?);
        } else if ch == '(' {
            ops.push(ch);
            i += 1;
        } else if ch == ')' {
            while ops.last().copied().is_some_and(|op| op != '(') {
                apply_op(&mut values, ops.pop().unwrap())?;
            }
            if ops.pop() != Some('(') {
                return Err("mismatched parens".to_string());
            }
            i += 1;
        } else if "+-*/".contains(ch) {
            while ops
                .last()
                .copied()
                .is_some_and(|op| precedence(op) >= precedence(ch))
            {
                apply_op(&mut values, ops.pop().unwrap())?;
            }
            ops.push(ch);
            i += 1;
        } else {
            return Err(format!("unexpected {ch}"));
        }
    }
    while let Some(op) = ops.pop() {
        apply_op(&mut values, op)?;
    }
    values
        .pop()
        .filter(|_| values.is_empty())
        .ok_or_else(|| "invalid expression".to_string())
}

fn precedence(op: char) -> i32 {
    match op {
        '+' | '-' => 1,
        '*' | '/' => 2,
        _ => 0,
    }
}

fn apply_op(values: &mut Vec<f64>, op: char) -> Result<(), String> {
    let b = values.pop().ok_or_else(|| "missing rhs".to_string())?;
    let a = values.pop().ok_or_else(|| "missing lhs".to_string())?;
    values.push(match op {
        '+' => a + b,
        '-' => a - b,
        '*' => a * b,
        '/' => a / b,
        _ => return Err("bad op".to_string()),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use webcore::dom::events::DomEvent;
    use webcore::parse_html;

    fn painted_text(doc: &Document, selector: &str) -> String {
        dom::query_selector(&doc.root, selector)
            .map(dom::get_text_content)
            .unwrap_or_default()
    }

    #[test]
    fn tictactoe_click_listener_marks_a_cell() {
        let mut doc = parse_html(include_str!("html/tictactoe.html"));
        assert!(demo_document_ready(&doc, "tictactoe.html"));
        install_tictactoe(&mut doc, Arc::new(Mutex::new(TicTacToeState::default())));
        let target = node_id(&doc, "#c0").unwrap();
        let mut event = DomEvent::new("click", target);
        doc.dispatch_dom_event(&mut event);
        assert_eq!(painted_text(&doc, "#c0"), "X");
    }

    #[test]
    fn minesweeper_click_listener_reveals_or_flags_a_cell() {
        let mut doc = parse_html(include_str!("html/minesweeper.html"));
        assert!(demo_document_ready(&doc, "minesweeper.html"));
        let state = Arc::new(Mutex::new(MineState::new()));
        let mut seed = 1;
        install_minesweeper(&mut doc, state, &mut seed);
        let flag = node_id(&doc, "#flag-mode").unwrap();
        let mut event = DomEvent::new("click", flag);
        doc.dispatch_dom_event(&mut event);
        let cell = node_id(&doc, "#r0c0").unwrap();
        let mut event = DomEvent::new("click", cell);
        doc.dispatch_dom_event(&mut event);
        assert_eq!(painted_text(&doc, "#r0c0"), "F");
    }

    #[test]
    fn delegated_root_listener_can_use_closest_with_hit_test_ids() {
        let mut doc = parse_html(include_str!("html/tictactoe.html"));
        let root = doc.root.node_id;
        doc.add_event_listener(
            root,
            "click",
            Box::new(|evt, doc| {
                if doc.closest(evt.target, ".cell").is_some() {
                    set_text(doc, "#status", "delegated");
                }
            }),
            ListenerOptions::default(),
        );
        let target = node_id(&doc, "#c0").unwrap();
        let mut event = DomEvent::new("click", target);
        doc.dispatch_dom_event(&mut event);
        assert_eq!(painted_text(&doc, "#status"), "delegated");
    }

    #[test]
    fn dom_tick_marks_document_dirty_without_hover() {
        let mut doc = parse_html(include_str!("html/dom.html"));
        let state = Arc::new(Mutex::new(DomState::new()));
        let mut seed = 1;
        doc.style_dirty = false;

        dom_tick(&mut doc, &state, &mut seed);

        assert_ne!(painted_text(&doc, "#clock"), "Uptime: 0d 0h 0m 0s");
        assert!(
            doc.style_dirty,
            "DOM dashboard mutations should dirty style/layout for idle relayout"
        );
    }

    #[test]
    fn dom_dark_mode_uses_document_mutation_api() {
        let mut doc = parse_html(include_str!("html/dom.html"));
        install_dom(&mut doc, Arc::new(Mutex::new(DomState::new())));
        doc.style_dirty = false;

        let target = node_id(&doc, ".tb-toggle").unwrap();
        let mut event = DomEvent::new("click", target);
        doc.dispatch_dom_event(&mut event);

        assert!(doc.class_list_contains(doc.root.node_id, "dark"));
        assert!(
            doc.style_dirty,
            "dark mode class changes should invalidate style instead of raw-patching the tree"
        );
    }

    #[test]
    fn dom_demo_install_starts_unpaused() {
        let mut doc = parse_html(include_str!("html/dom.html"));
        let state = Arc::new(Mutex::new(DomState {
            paused: true,
            chaos: true,
            ..DomState::new()
        }));

        install_dom(&mut doc, state);

        let pause = node_id(&doc, "[data-dom-action=pause]").unwrap();
        assert!(!doc.class_list_contains(pause, "tb-active"));
        assert!(doc.text_content(pause).contains("Pause"));
        let chaos = node_id(&doc, "[data-dom-action=chaos]").unwrap();
        assert!(!doc.class_list_contains(chaos, "tb-active"));
        assert!(doc.text_content(chaos).contains("Chaos"));
    }

    #[test]
    fn dom_toolbar_buttons_toggle_visible_state() {
        let mut doc = parse_html(include_str!("html/dom.html"));
        let state = Arc::new(Mutex::new(DomState::new()));
        install_dom(&mut doc, state);

        let compact = node_id(&doc, "[data-dom-action=compact]").unwrap();
        let mut event = DomEvent::new("click", compact);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(doc.root.node_id, "compact"));
        let compact = node_id(&doc, "[data-dom-action=compact]").unwrap();
        assert!(doc.class_list_contains(compact, "tb-active"));
        assert!(doc.text_content(compact).contains("Roomy"));

        let pause = node_id(&doc, "[data-dom-action=pause]").unwrap();
        let mut event = DomEvent::new("click", pause);
        doc.dispatch_dom_event(&mut event);
        let pause = node_id(&doc, "[data-dom-action=pause]").unwrap();
        assert!(doc.class_list_contains(pause, "tb-active"));
        assert!(doc.text_content(pause).contains("Resume"));

        let chaos = node_id(&doc, "[data-dom-action=chaos]").unwrap();
        let mut event = DomEvent::new("click", chaos);
        doc.dispatch_dom_event(&mut event);
        let chaos = node_id(&doc, "[data-dom-action=chaos]").unwrap();
        assert!(doc.class_list_contains(chaos, "tb-active"));
        assert!(
            doc.text_content(node_id(&doc, "#alerts").unwrap())
                .contains("Chaos")
        );

        let service = node_id(&doc, "[data-dom-action=service]").unwrap();
        let before = doc.query_selector_all("tr").len();
        let mut event = DomEvent::new("click", service);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.query_selector_all("tr").len() > before);

        let alerts = node_id(&doc, "[data-dom-action=alerts]").unwrap();
        let mut event = DomEvent::new("click", alerts);
        doc.dispatch_dom_event(&mut event);
        assert_eq!(doc.text_content(node_id(&doc, "#alerts").unwrap()), "");

        let feed = node_id(&doc, "[data-dom-action=feed]").unwrap();
        let mut event = DomEvent::new("click", feed);
        doc.dispatch_dom_event(&mut event);
        assert_eq!(
            doc.text_content(node_id(&doc, "#activity-feed").unwrap()),
            ""
        );
    }

    #[test]
    fn graph_click_updates_event_status() {
        let mut doc = parse_html(include_str!("html/graph.html"));
        let state = Arc::new(Mutex::new(GraphState::default()));
        let mut seed = 1;
        install_graph(&mut doc, state, &mut seed);
        doc.style_dirty = false;

        let target = node_id(&doc, "graph").unwrap();
        let mut event = DomEvent::new("click", target);
        doc.dispatch_dom_event(&mut event);

        assert!(painted_text(&doc, "#status-text").contains("Chart cycled"));
        assert_eq!(painted_text(&doc, "#click-count"), "1");
        assert!(doc.style_dirty);
    }

    #[test]
    fn graph_sidebar_and_kpi_are_live() {
        let mut doc = parse_html(include_str!("html/graph.html"));
        let state = Arc::new(Mutex::new(GraphState::default()));
        let mut seed = 1;
        install_graph(&mut doc, state, &mut seed);

        let sidebar = node_id(&doc, "#sb-products").unwrap();
        let mut event = DomEvent::new("click", sidebar);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(sidebar, "sb-item-active"));
        assert!(painted_text(&doc, "#status-text").contains("sb-products"));

        let kpi = node_id(&doc, "#kpi-users").unwrap();
        let mut event = DomEvent::new("click", kpi);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(kpi, "kpi-selected"));
    }

    #[test]
    fn markdown_demo_updates_preview_from_source() {
        let mut doc = parse_html(include_str!("html/markdown.html"));
        let mut last_source = String::new();
        install_markdown(&mut doc, &mut last_source);

        let source = node_id(&doc, ".md-source").unwrap();
        doc.set_text_content(source, "# Changed\n\nThis is **new**.");
        assert!(update_markdown_preview(&mut doc, &mut last_source));

        assert!(
            doc.text_content(node_id(&doc, ".preview-body").unwrap())
                .contains("Changed")
        );
        assert!(
            doc.text_content(node_id(&doc, ".preview-body").unwrap())
                .contains("new")
        );
    }

    #[test]
    fn email_demo_selects_mailboxes_labels_and_messages() {
        let mut doc = parse_html(include_str!("html/email.html"));
        let state = Arc::new(Mutex::new(EmailState::default()));
        install_email(&mut doc, state);

        assert!(
            doc.text_content(node_id(&doc, ".email-list").unwrap())
                .contains("Project timeline")
        );

        let sent = doc
            .query_selector_all(".sidebar-item")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-mailbox").as_deref() == Some("Sent"))
            .unwrap();
        let mut event = DomEvent::new("click", sent);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(sent, "active"));
        assert!(
            doc.text_content(node_id(&doc, ".email-list").unwrap())
                .contains("Release checklist")
        );
        assert!(
            doc.text_content(node_id(&doc, ".email-subject-title").unwrap())
                .contains("Release checklist")
        );

        let work = doc
            .query_selector_all(".label-item")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-label").as_deref() == Some("Work"))
            .unwrap();
        let mut event = DomEvent::new("click", work);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(work, "active"));

        let dave = doc
            .query_selector_all(".email-item")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-email-id").as_deref() == Some("dave"))
            .unwrap();
        let mut event = DomEvent::new("click", dave);
        doc.dispatch_dom_event(&mut event);
        let selected_dave = doc
            .query_selector_all(".email-item")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-email-id").as_deref() == Some("dave"))
            .unwrap();
        assert!(doc.class_list_contains(selected_dave, "selected"));
        assert!(
            doc.text_content(node_id(&doc, ".email-subject-title").unwrap())
                .contains("Q1 Report")
        );

        let dark = doc
            .query_selector_all("button")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-email-action").as_deref() == Some("dark"))
            .unwrap();
        let mut event = DomEvent::new("click", dark);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(doc.root.node_id, "dark"));
        assert!(doc.text_content(dark).contains("☀"));
        assert!(
            doc.text_content(node_id(&doc, ".email-list").unwrap())
                .contains("Q1 Report")
        );
    }

    #[test]
    fn eudora_demo_renders_and_handles_core_actions() {
        let mut doc = parse_html(include_str!("html/eudora.html"));
        let state = Arc::new(Mutex::new(EudoraState::default()));
        install_eudora(&mut doc, state);

        let inbox_rows = doc
            .query_selector_all("tr")
            .into_iter()
            .filter(|id| doc.get_attribute(*id, "data-box").as_deref() == Some("in"))
            .count();
        assert_eq!(inbox_rows, EUDORA_INBOX.len());
        assert!(
            doc.text_content(node_id(&doc, "#inbox-rows").unwrap())
                .contains("Welcome")
        );

        let out_tab = doc
            .query_selector_all(".tab")
            .into_iter()
            .find(|id| doc.get_attribute(*id, "data-tab").as_deref() == Some("out"))
            .unwrap();
        let mut event = DomEvent::new("click", out_tab);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(node_id(&doc, "#page-out").unwrap(), "active"));

        let new_message = doc
            .query_selector_all("button")
            .into_iter()
            .find(|id| doc.text_content(*id).contains("New Message"))
            .unwrap();
        let mut event = DomEvent::new("click", new_message);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(node_id(&doc, "#page-compose").unwrap(), "active"));

        let dark = node_id(&doc, "#btn-dark").unwrap();
        let mut event = DomEvent::new("click", dark);
        doc.dispatch_dom_event(&mut event);
        assert!(doc.class_list_contains(doc.root.node_id, "dark"));
        assert!(doc.text_content(dark).contains("☀"));
        let eudora_html = include_str!("html/eudora.html");
        assert!(eudora_html.contains(".dark #toolbar"));
        assert!(eudora_html.contains(".dark .tab:hover:not(.selected)"));
        assert!(eudora_html.contains(".dark table.msg-list td"));
        assert!(eudora_html.contains(".dark .contact-title"));
        assert!(eudora_html.contains(".dark .filter-action-text"));
    }
}
