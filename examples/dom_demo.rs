use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::dom::HtmlEventType;
use webcore::platform::Platform;
use webcore::{Document, LayoutEngine, Renderer, load_html};

const HTML: &str = include_str!("html/dom.html");

// ── Sparkline ────────────────────────────────────────────────────────────────

fn sparkline(data: &[i32], max_val: i32) -> String {
    const BARS: &[char] = &[' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    data.iter()
        .map(|&v| {
            let i = ((v * 7) / max_val.max(1)).clamp(0, 7) as usize;
            BARS[i]
        })
        .collect()
}

fn time_str(tick_secs: i32) -> String {
    let s = tick_secs % 60;
    let m = (tick_secs / 60) % 60;
    let h = (tick_secs / 3600) % 24;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

// ── Dashboard state ───────────────────────────────────────────────────────────

#[derive(Default)]
struct State {
    tick: i32,
    cpu: i32,
    mem: i32,
    rps: i32,
    disk: i32,
    net: i32,
    err_rate: f32,
    prev_cpu: i32,
    prev_mem: i32,
    prev_rps: i32,
    prev_err: f32,
    cpu_history: Vec<i32>,
    rps_history: Vec<i32>,
    svc_count: i32,
    chaos_mode: bool,
    paused: bool,
    dark_mode: bool,
    compact: bool,
    base_lat: [i32; 4],
    interval_ms: u64,
}

// ── App ───────────────────────────────────────────────────────────────────────

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    width: f32,
    state: Arc<RwLock<State>>,
    mouse_pos: (f32, f32),
    pending_mouse_move: Option<(f32, f32)>,
    next_tick: Instant,
}

impl App {
    fn new() -> Self {
        let mut st = State::default();
        st.cpu = 23;
        st.mem = 3200;
        st.rps = 1420;
        st.disk = 45;
        st.net = 120;
        st.err_rate = 0.12;
        st.prev_cpu = 23;
        st.prev_mem = 3200;
        st.prev_rps = 1420;
        st.prev_err = 0.12;
        st.svc_count = 4;
        st.base_lat = [12, 3, 1, 5];
        st.interval_ms = 1000;

        Self {
            window: None,
            platform: None,
            renderer: Renderer::new(),
            doc: None,
            width: 1000.0,
            state: Arc::new(RwLock::new(st)),
            mouse_pos: (0.0, 0.0),
            pending_mouse_move: None,
            next_tick: Instant::now(),
        }
    }

    fn do_tick(&mut self) {
        let mut st = self.state.write().unwrap();
        if st.paused {
            return;
        }
        st.tick += 1;

        st.prev_cpu = st.cpu;
        st.prev_mem = st.mem;
        st.prev_rps = st.rps;
        st.prev_err = st.err_rate;

        let sw = if st.chaos_mode { 25 } else { 7 };
        st.cpu = (st.cpu + rand_range(-sw, sw)).clamp(3, 99);
        let msw = if st.chaos_mode { 500 } else { 90 };
        st.mem = (st.mem + rand_range(-msw, msw)).clamp(800, 7900);
        let rsw = if st.chaos_mode { 800 } else { 150 };
        st.rps = (st.rps + rand_range(-rsw, rsw)).clamp(100, 6000);
        st.disk = (st.disk + rand_range(-30, 30)).clamp(5, 500);
        st.net = (st.net + rand_range(-50, 50)).clamp(10, 1000);
        let esw = if st.chaos_mode { 0.5 } else { 0.05 };
        st.err_rate = (st.err_rate + (rand_range(-50, 50) as f32) * (esw / 50.0)).clamp(0.0, 8.0);

        let cpu = st.cpu;
        st.cpu_history.push(cpu);
        if st.cpu_history.len() > 60 {
            st.cpu_history.remove(0);
        }
        let rps = st.rps;
        st.rps_history.push(rps);
        if st.rps_history.len() > 60 {
            st.rps_history.remove(0);
        }

        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };

        set_text(doc, "#cpu-val", &format!("{}%", st.cpu));
        set_text(doc, "#mem-val", &format!("{} MB", st.mem));
        set_text(doc, "#req-val", &format!("{}", st.rps));
        set_text(doc, "#err-val", &format!("{:.2}%", st.err_rate));
        set_change(doc, "#cpu-chg", st.cpu, st.prev_cpu);
        set_change(doc, "#mem-chg", st.mem, st.prev_mem);
        set_change(doc, "#req-chg", st.rps, st.prev_rps);
        set_change_f(doc, "#err-chg", st.err_rate, st.prev_err);

        let uptime = {
            let s = st.tick;
            let d = s / 86400;
            let s = s % 86400;
            let h = s / 3600;
            let s = s % 3600;
            let m = s / 60;
            let s = s % 60;
            format!("Uptime: {}d {}h {}m {}s", d, h, m, s)
        };
        set_text(doc, "#clock", &uptime);

        let cpu_sl = sparkline(&st.cpu_history, 100);
        set_text(doc, "#cpu-chart", &cpu_sl);
        let (cpu_min, cpu_max) = min_max(&st.cpu_history);
        set_text(
            doc,
            "#cpu-meta",
            &format!("min {}%  max {}%  now {}%", cpu_min, cpu_max, st.cpu),
        );

        let rps_sl = sparkline(&st.rps_history, 6000);
        set_text(doc, "#rps-chart", &rps_sl);
        let (rps_min, rps_max) = min_max(&st.rps_history);
        set_text(
            doc,
            "#rps-meta",
            &format!("min {}  max {}  now {} req/s", rps_min, rps_max, st.rps),
        );

        let mem_pct = (st.mem * 100) / 8192;
        let mem_color = if mem_pct > 85 {
            "#ef4444"
        } else if mem_pct > 65 {
            "#f59e0b"
        } else {
            "#10b981"
        };
        if let Some(b) = doc.query_selector("#mem-bar") {
            doc.set_style_property(b, "width", &format!("{}%", mem_pct));
            doc.set_style_property(b, "background", mem_color);
        }
        set_text(doc, "#mem-big", &format!("{} / 8192 MB", st.mem));
        set_text(doc, "#mem-label", &format!("{}% used", mem_pct));
        set_text(doc, "#disk-val", &format!("{} MB/s", st.disk));
        if let Some(b) = doc.query_selector("#disk-bar") {
            doc.set_style_property(b, "width", &format!("{}%", (st.disk * 100) / 500));
        }
        set_text(doc, "#net-val", &format!("{} Mbps", st.net));
        if let Some(b) = doc.query_selector("#net-bar") {
            doc.set_style_property(b, "width", &format!("{}%", (st.net * 100) / 1000));
        }

        let svc_ids = ["api", "db", "cache", "queue"];
        let svc_names = ["API Gateway", "Database", "Cache", "Msg Queue"];
        let thr = if st.chaos_mode { 20 } else { 3 };
        let wthr = if st.chaos_mode { 35 } else { 8 };

        for i in 0..4 {
            let bid = format!("#svc-{}-badge", svc_ids[i]);
            let lid = format!("#svc-{}-lat", svc_ids[i]);
            let roll = rand_range(0, 99);
            let latency = st.base_lat[i] + rand_range(0, 9);
            let (badge_text, badge_ok, badge_warn, badge_err, final_lat) = if roll < thr {
                ("DOWN", false, false, true, 999)
            } else if roll < wthr {
                ("WARN", false, true, false, latency * 5)
            } else {
                ("OK", true, false, false, latency)
            };

            if let Some(b) = doc.query_selector(&bid) {
                doc.set_text_content(b, badge_text);
                let class = if badge_ok { "badge-ok" } else if badge_warn { "badge-warn" } else { "badge-err" };
                for candidate in ["badge-ok", "badge-warn", "badge-err"] {
                    if candidate == class { doc.class_list_add(b, candidate); }
                    else { doc.class_list_remove(b, candidate); }
                }
            }
            set_text(doc, &lid, &format!("{}ms", final_lat));

            if badge_err {
                let ts_str = time_str(st.tick);
                let msg = format!("🚨 [{}] {} is DOWN!", ts_str, svc_names[i]);
                add_alert(doc, "alert-crit", &msg);
            }
        }

        if st.tick % 3 == 0 {
            const EVENTS: &[&str] = &[
                "🚀 Deployment completed",
                "📈 Auto-scaler: 3 -> 4 replicas",
                "🔒 SSL certificate renewed",
                "💾 DB backup done (2.3 GB)",
                "⚡ Cache cleared /api/v2/*",
                "🛡 Rate limit: 203.0.113.42",
                "✅ Health check passed",
                "🔄 Config reload: feature-flags",
                "👤 New user #14,203",
                "📡 Webhook delivered",
                "📁 Log rotation: 340 MB archived",
                "🧹 Job 'cleanup-sessions' done",
            ];
            let ts_str = time_str(st.tick);
            let ev = EVENTS[(st.tick as usize / 3) % EVENTS.len()];
            let msg = format!("{}  {}", ts_str, ev);
            if let Some(feed) = doc.query_selector("#activity-feed") {
                while doc.children(feed).len() >= 10 {
                    if let Some(first_id) = doc.first_child(feed) { doc.remove_child(first_id); }
                    else { break; }
                }
                let item = doc.create_element("div");
                doc.class_list_add(item, "feed-item");
                doc.set_text_content(item, &msg);
                doc.append_child(feed, item);
            }
        }

        if let Some(alerts) = doc.query_selector("#alerts") {
            if doc.children(alerts).len() > 5 {
                if let Some(first_id) = doc.first_child(alerts) { doc.remove_child(first_id); }
            }
        }
        if st.err_rate > 3.0 && st.tick % 5 == 0 {
            let ts_str = time_str(st.tick);
            let msg = format!(
                "⚠ [{}] Error rate {:.1}% — threshold exceeded",
                ts_str, st.err_rate
            );
            add_alert(doc, "alert-warn", &msg);
        }

        LayoutEngine::new().layout(doc, self.width);
    }

    fn setup_events(&mut self) {
        let doc = match self.doc.as_mut() {
            Some(d) => d,
            None => return,
        };
        let state = self.state.clone();
        // no captures needed — layout will be requested by process_mouse_event

        // Toolbar buttons: match by class to avoid catching other buttons in the
        // document. Prevent default editor behavior for toolbar actions.
        let __root = doc.root.node_id;
        doc.add_event_listener(__root, "click", Box::new(move |evt, __d: &mut webcore::Document| {
            // Delegation, the way a page writes it: one listener, then
            // `closest()` to find which matching element was hit.
            let Some(__cur) = __d.closest(evt.target, ".tb-btn") else { return };
            // left-click only
            if evt.button != 0 { return; }
            // Prevent default editor behavior for toolbar actions
            evt.prevent_default();
            let action = __d.get_attribute(__cur, "data-dom-action").unwrap_or_default();
            let mut st = state.write().unwrap();

            if action == "dark" {
                st.dark_mode = !st.dark_mode;
                if let Some(root) = __d.document_element() {
                    if st.dark_mode { __d.class_list_add(root, "dark"); }
                    else { __d.class_list_remove(root, "dark"); }
                }
            } else if action == "compact" {
                st.compact = !st.compact;
                if let Some(root) = __d.document_element() {
                    if st.compact { __d.class_list_add(root, "compact"); }
                    else { __d.class_list_remove(root, "compact"); }
                }
            } else if action == "pause" {
                st.paused = !st.paused;
            } else if action == "chaos" {
                st.chaos_mode = !st.chaos_mode;
                if st.chaos_mode {
                    let ts_str = time_str(st.tick);
                    let msg = format!("🔥 [{}] CHAOS MODE ENGAGED — brace yourself", ts_str);
                    add_alert(__d, "alert-warn", &msg);
                }
            } else if action == "service" {
                st.svc_count += 1;
                let svc_count = st.svc_count;
                    if let Some(tbody) = __d.query_selector("#svc-tbody") {
                    const NAMES: &[&str] = &["🔴 Redis", "📦 Kafka", "🌐 Nginx", "🔍 Elastic", "📊 Prometheus", "🔐 Vault", "🗺 Consul", "📉 Grafana", "🔭 Jaeger", "📁 MinIO"];
                    let idx = ((svc_count - 5) % 10) as usize;
                    let row = __d.create_element("tr");
                    __d.set_attribute(row, "id", &format!("svc-x{}", svc_count));
                    let td1 = __d.create_element("td"); __d.set_text_content(td1, NAMES[idx]);
                    let td2 = __d.create_element("td");
                    let badge = __d.create_element("span"); __d.class_list_add(badge, "badge"); __d.class_list_add(badge, "badge-ok");
                    __d.set_attribute(badge, "id", &format!("svc-x{}-badge", svc_count)); __d.set_text_content(badge, "HEALTHY");
                    __d.append_child(td2, badge);
                    let td3 = __d.create_element("td"); __d.set_attribute(td3, "id", &format!("svc-x{}-lat", svc_count));
                    __d.set_text_content(td3, &format!("{}ms", 2 + rand_range(0, 19)));
                    __d.append_child(row, td1); __d.append_child(row, td2); __d.append_child(row, td3);
                    __d.append_child(tbody, row);
                }
            } else if action == "alerts" {
                 if let Some(alerts) = __d.query_selector("#alerts") {
                     while let Some(first) = __d.first_child(alerts) { __d.remove_child(first); }
                 }
            } else if action == "feed" {
                 if let Some(feed) = __d.query_selector("#activity-feed") {
                     while let Some(first) = __d.first_child(feed) { __d.remove_child(first); }
                 }
            }
            let pressed = match action.as_str() {
                "dark" => Some(st.dark_mode),
                "compact" => Some(st.compact),
                "pause" => Some(st.paused),
                "chaos" => Some(st.chaos_mode),
                _ => None,
            };
            if let Some(pressed) = pressed {
                __d.set_attribute(__cur, "aria-pressed", if pressed { "true" } else { "false" });
                if pressed {
                    __d.class_list_add(__cur, "tb-active");
                } else {
                    __d.class_list_remove(__cur, "tb-active");
                }
            }
        }), webcore::dom::events::ListenerOptions::default());
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn set_text(doc: &mut Document, selector: &str, text: &str) {
    if let Some(b) = doc.query_selector(selector) {
        doc.set_text_content(b, text);
    }
}

fn set_change(doc: &mut Document, selector: &str, cur: i32, prev: i32) {
    let d = cur - prev;
    let s = if d > 0 {
        format!("+{}", d)
    } else if d < 0 {
        format!("{}", d)
    } else {
        "--".into()
    };
    set_text(doc, selector, &s);
}

fn set_change_f(doc: &mut Document, selector: &str, cur: f32, prev: f32) {
    let d = cur - prev;
    let s = if d > 0.005 {
        format!("+{:.2}%", d)
    } else if d < -0.005 {
        format!("{:.2}%", d)
    } else {
        "stable".into()
    };
    set_text(doc, selector, &s);
}

fn add_alert(doc: &mut Document, cls: &str, text: &str) {
    if let Some(alerts) = doc.query_selector("#alerts") {
        let a = doc.create_element("div");
        doc.class_list_add(a, "alert");
        doc.class_list_add(a, cls);
        doc.set_text_content(a, text);
        doc.append_child(alerts, a);
    }
}

fn min_max(v: &[i32]) -> (i32, i32) {
    if v.is_empty() {
        return (0, 0);
    }
    (*v.iter().min().unwrap(), *v.iter().max().unwrap())
}

static RAND_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(12345);
fn rand_range(lo: i32, hi: i32) -> i32 {
    use std::sync::atomic::Ordering;
    let mut s = RAND_STATE.load(Ordering::Relaxed);
    s ^= s << 13;
    s ^= s >> 7;
    s ^= s << 17;
    RAND_STATE.store(s, Ordering::Relaxed);
    lo + ((s as i32).unsigned_abs() as i32 % (hi - lo + 1).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use webcore::dom::events::DomEvent;
    use webcore::types::Color;

    #[test]
    fn toolbar_clicks_mutate_the_document() {
        let mut app = App::new();
        app.doc = Some(load_html(HTML, app.width));
        app.setup_events();
        let doc = app.doc.as_mut().unwrap();
        let buttons = doc.query_selector_all(".tb-btn");
        let root = doc.document_element().unwrap();

        doc.dispatch_dom_event(&mut DomEvent::new("click", buttons[0]));
        assert!(doc.class_list_contains(root, "dark"));

        let tbody = doc.query_selector("#svc-tbody").unwrap();
        let before = doc.child_nodes(tbody).len();
        doc.dispatch_dom_event(&mut DomEvent::new("click", buttons[4]));
        assert_eq!(doc.child_nodes(tbody).len(), before + 1);
    }

    #[test]
    fn active_toolbar_hover_keeps_readable_colors() {
        let mut doc = load_html(HTML, 1000.0);
        let button = doc.query_selector(".tb-btn").unwrap();
        doc.class_list_add(button, "tb-active");
        LayoutEngine::new().layout(&mut doc, 1000.0);
        let rect = doc.get_node(button).unwrap().layout.border_rect;
        doc.process_mouse_event(
            HtmlEventType::MouseMove,
            (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            0,
        );
        LayoutEngine::new().layout(&mut doc, 1000.0);
        let style = &doc.get_node(button).unwrap().style;
        assert_eq!(style.background_color, Color::rgb(55, 48, 163));
        assert_eq!(style.color, Color::rgb(255, 255, 255));
    }
}

// ── winit ApplicationHandler ─────────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("dom_demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1000u32, 800u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();
        self.doc = Some(load_html(HTML, self.width));
        self.window = Some(window);
        self.platform = Some(platform);
        self.next_tick =
            Instant::now() + Duration::from_millis(self.state.read().unwrap().interval_ms);
        self.setup_events();
    }

    fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if let (Some(point), Some(doc)) = (self.pending_mouse_move.take(), self.doc.as_mut()) {
            let point = (point.0, point.1 + doc.scroll_y);
            let changed = doc.process_mouse_event(HtmlEventType::MouseMove, point, 0)
                | doc.process_mouse_event(HtmlEventType::PointerMove, point, 0);
            if changed {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }
        let now = Instant::now();
        if now >= self.next_tick {
            self.do_tick();
            let interval = self.state.read().unwrap().interval_ms;
            self.next_tick = now + Duration::from_millis(interval);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
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
            WindowEvent::CursorMoved { position, .. } => {
                let scale = platform.scale_factor();
                self.mouse_pos = (position.x as f32 / scale, position.y as f32 / scale);
                self.pending_mouse_move = Some(self.mouse_pos);
            }
            WindowEvent::MouseInput {
                state,
                button,
                ..
            } => {
                let b_idx = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => return,
                };
                if let Some(doc) = self.doc.as_mut() {
                    let kind = if state == ElementState::Pressed {
                        HtmlEventType::MouseDown
                    } else {
                        HtmlEventType::MouseUp
                    };
                    let pt = (self.mouse_pos.0, self.mouse_pos.1 + doc.scroll_y);
                    let mut changed = doc.process_mouse_event(kind, pt, b_idx);
                    if b_idx == 2 && state == ElementState::Released {
                        changed |= doc.process_mouse_event(HtmlEventType::ContextMenu, pt, b_idx);
                    }
                    if changed {
                        window.request_redraw();
                    }
                }
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
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
