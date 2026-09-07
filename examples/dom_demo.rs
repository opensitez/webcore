use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::{load_html, Document, LayoutEngine, Renderer, WebCore};

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
        if let Some(b) = dom::query_selector_mut(&mut doc.root, "#mem-bar") {
            dom::set_style_property(b, "width", &format!("{}%", mem_pct));
            dom::set_style_property(b, "background", mem_color);
        }
        set_text(doc, "#mem-big", &format!("{} / 8192 MB", st.mem));
        set_text(doc, "#mem-label", &format!("{}% used", mem_pct));
        set_text(doc, "#disk-val", &format!("{} MB/s", st.disk));
        if let Some(b) = dom::query_selector_mut(&mut doc.root, "#disk-bar") {
            dom::set_style_property(b, "width", &format!("{}%", (st.disk * 100) / 500));
        }
        set_text(doc, "#net-val", &format!("{} Mbps", st.net));
        if let Some(b) = dom::query_selector_mut(&mut doc.root, "#net-bar") {
            dom::set_style_property(b, "width", &format!("{}%", (st.net * 100) / 1000));
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

            if let Some(b) = dom::query_selector_mut(&mut doc.root, &bid) {
                dom::set_text_content(b, badge_text);
                if badge_ok {
                    dom::add_class(b, "badge-ok");
                    dom::remove_class(b, "badge-warn");
                    dom::remove_class(b, "badge-err");
                }
                if badge_warn {
                    dom::add_class(b, "badge-warn");
                    dom::remove_class(b, "badge-ok");
                    dom::remove_class(b, "badge-err");
                }
                if badge_err {
                    dom::add_class(b, "badge-err");
                    dom::remove_class(b, "badge-ok");
                    dom::remove_class(b, "badge-warn");
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
            if let Some(feed) = dom::query_selector_mut(&mut doc.root, "#activity-feed") {
                while feed.children.len() >= 10 {
                    if let Some(first_id) = dom::get_first_child(feed).map(|c| c.node_id) {
                        dom::remove_child(feed, first_id);
                    } else {
                        break;
                    }
                }
                let mut item = dom::create_element("div");
                dom::add_class(&mut item, "feed-item");
                dom::set_text_content(&mut item, &msg);
                dom::append_child(feed, item);
            }
        }

        if let Some(alerts) = dom::query_selector_mut(&mut doc.root, "#alerts") {
            if alerts.children.len() > 5 {
                if let Some(first_id) = dom::get_first_child(alerts).map(|c| c.node_id) {
                    dom::remove_child(alerts, first_id);
                }
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
            let root = &mut __d.root;
            let _ = &root;
            // left-click only
            if evt.button != 0 { return; }
            // Prevent default editor behavior for toolbar actions
            evt.prevent_default();
            // Debug: log hit/test info
            let doc_pos = (evt.page_x(), evt.page_y());
            let btn_ptr = evt.target;
            let cur_ptr = __cur;
            fn find_node_ref(node: &WebCore, id: u32) -> Option<&WebCore> {
                if node.node_id == id { return Some(node); }
                for c in &node.children { if let Some(f) = find_node_ref(c, id) { return Some(f); } }
                None
            }
            let (btn_text, id, cls) = find_node_ref(root, cur_ptr)
                .map(|cur_node| (
                    dom::get_text_content(cur_node).trim().to_string(),
                    cur_node.attributes.get("id").cloned().unwrap_or_default(),
                    cur_node.attributes.get("class").cloned().unwrap_or_default(),
                ))
                .unwrap_or_default();
            eprintln!("[DOM_DBG] click button target={} current_target={} id='{}' class='{}' btn_text='{}' doc_pos={:?} button={}", btn_ptr, cur_ptr, id, cls, btn_text, doc_pos, evt.button);
            let mut st = state.write().unwrap();

            if btn_text.contains("Dark") {
                st.dark_mode = !st.dark_mode;
                // Mutation of DOM from event handler is allowed via unsafe for now
                if st.dark_mode { dom::add_class(root, "dark"); eprintln!("[DOM_DBG] dark ON"); }
                else { dom::remove_class(root, "dark"); eprintln!("[DOM_DBG] dark OFF"); }
                // layout will be done by caller after event processing
            } else if btn_text.contains("Compact") {
                st.compact = !st.compact;
                if st.compact { dom::add_class(root, "compact"); eprintln!("[DOM_DBG] compact ON"); }
                else { dom::remove_class(root, "compact"); eprintln!("[DOM_DBG] compact OFF"); }
                // layout will be done by caller after event processing
            } else if btn_text.contains("Pause") {
                st.paused = !st.paused;
            } else if btn_text.contains("Chaos") {
                st.chaos_mode = !st.chaos_mode;
                if st.chaos_mode {
                    let ts_str = time_str(st.tick);
                    let msg = format!("🔥 [{}] CHAOS MODE ENGAGED — brace yourself", ts_str);
                    if let Some(alerts) = dom::query_selector_mut(root, "#alerts") {
                        let mut a = dom::create_element("div");
                        dom::add_class(&mut a, "alert"); dom::add_class(&mut a, "alert-warn");
                        dom::set_text_content(&mut a, &msg);
                        dom::append_child(alerts, a);
                    }
                    // layout will be done by caller after event processing
                    eprintln!("[DOM_DBG] chaos ON - alert appended");
                }
            } else if btn_text.contains("Service") {
                st.svc_count += 1;
                let svc_count = st.svc_count;
                    if let Some(tbody) = dom::query_selector_mut(root, "#svc-tbody") {
                    const NAMES: &[&str] = &["🔴 Redis", "📦 Kafka", "🌐 Nginx", "🔍 Elastic", "📊 Prometheus", "🔐 Vault", "🗺 Consul", "📉 Grafana", "🔭 Jaeger", "📁 MinIO"];
                    let idx = ((svc_count - 5) % 10) as usize;
                    let mut row = dom::create_element("tr");
                    dom::set_attribute(&mut row, "id", &format!("svc-x{}", svc_count));
                    let mut td1 = dom::create_element("td"); dom::set_text_content(&mut td1, NAMES[idx]);
                    let mut td2 = dom::create_element("td");
                    let mut badge = dom::create_element("span"); dom::add_class(&mut badge, "badge"); dom::add_class(&mut badge, "badge-ok");
                    dom::set_attribute(&mut badge, "id", &format!("svc-x{}-badge", svc_count)); dom::set_text_content(&mut badge, "HEALTHY");
                    dom::append_child(&mut td2, badge);
                    let mut td3 = dom::create_element("td"); dom::set_attribute(&mut td3, "id", &format!("svc-x{}-lat", svc_count));
                    dom::set_text_content(&mut td3, &format!("{}ms", 2 + rand_range(0, 19)));
                    dom::append_child(&mut row, td1); dom::append_child(&mut row, td2); dom::append_child(&mut row, td3);
                    dom::append_child(tbody, row);
                    eprintln!("[DOM_DBG] service row added id=svc-x{}", svc_count);
                    // layout will be done by caller after event processing
                }
            } else if btn_text.contains("Alerts") {
                 if let Some(alerts) = dom::query_selector_mut(root, "#alerts") {
                     while !alerts.children.is_empty() {
                         let fid = dom::get_first_child(alerts).map(|c| c.node_id).unwrap();
                         dom::remove_child(alerts, fid);
                     }
                     eprintln!("[DOM_DBG] alerts cleared");
                    // layout will be done by caller after event processing
                 }
            } else if btn_text.contains("Feed") {
                 if let Some(feed) = dom::query_selector_mut(root, "#activity-feed") {
                     while !feed.children.is_empty() {
                         let fid = dom::get_first_child(feed).map(|c| c.node_id).unwrap();
                         dom::remove_child(feed, fid);
                     }
                     eprintln!("[DOM_DBG] feed cleared");
                     // layout will be done by caller after event processing
                 }
            }
        }), webcore::dom::events::ListenerOptions::default());
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn set_text(doc: &mut Document, selector: &str, text: &str) {
    if let Some(b) = dom::query_selector_mut(&mut doc.root, selector) {
        dom::set_text_content(b, text);
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
    if let Some(alerts) = dom::query_selector_mut(&mut doc.root, "#alerts") {
        let mut a = dom::create_element("div");
        dom::add_class(&mut a, "alert");
        dom::add_class(&mut a, cls);
        dom::set_text_content(&mut a, text);
        dom::append_child(alerts, a);
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
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let b_idx = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => 3,
                };
                if let Some(doc) = self.doc.as_mut() {
                    if doc.process_mouse_event(
                        HtmlEventType::Click,
                        (self.mouse_pos.0, self.mouse_pos.1 + doc.scroll_y),
                        b_idx,
                    ) {
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
