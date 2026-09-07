use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use std::sync::Mutex;
use webcore::dom::{self, HtmlEventType};
use webcore::platform::Platform;
use webcore::types::Component;
use webcore::{Document, Renderer, WebCore};

const HTML: &str = include_str!("html/graph.html");

fn get_attr(node: &WebCore, key: &str, def: &str) -> String {
    node.attributes
        .get(key)
        .cloned()
        .unwrap_or_else(|| def.to_string())
}

fn parse_csv(s: &str) -> Vec<f32> {
    s.split(',')
        .filter_map(|x| x.trim().parse::<f32>().ok())
        .collect()
}

/// Graph custom component — implements the Component trait for full layout participation.
struct GraphComponent;

impl Component for GraphComponent {
    fn measure(&self, node: &WebCore, _available_w: f32) -> (f32, f32) {
        let w = get_attr(node, "data-width", "340")
            .parse::<f32>()
            .unwrap_or(340.0);
        let h = get_attr(node, "data-height", "190")
            .parse::<f32>()
            .unwrap_or(190.0);
        (w, h)
    }

    fn intrinsic_width(&self, node: &WebCore) -> (f32, f32) {
        let w = get_attr(node, "data-width", "340")
            .parse::<f32>()
            .unwrap_or(340.0);
        (w, w) // fixed size: min == max
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
        use tiny_skia::*;
        let mut paint = Paint::default();
        paint.set_color_rgba8(22, 27, 34, 255);
        let ts = Transform::from_scale(scale, scale);
        if let Some(rect) = Rect::from_xywh(x, y, w, h) {
            pixmap.fill_rect(rect, &paint, ts, None);
        }

        let chart_type = get_attr(node, "data-type", "bar");
        let values = parse_csv(&get_attr(node, "data-values", ""));
        if values.is_empty() {
            return;
        }

        let max_val = values.iter().copied().fold(0.0f32, f32::max).max(1.0);
        let n = values.len();
        let margin = 10.0;
        let plot_w = w - 2.0 * margin;
        let plot_h = h - 2.0 * margin;

        match chart_type.as_str() {
            "bar" => {
                let bar_w = plot_w / n as f32;
                for (i, &v) in values.iter().enumerate() {
                    let bh = (v / max_val) * plot_h;
                    let bx = x + margin + i as f32 * bar_w;
                    let by = y + h - margin - bh;
                    let mut p = Paint::default();
                    p.set_color_rgba8(78, 121, 167, 255);
                    if let Some(r) = Rect::from_xywh(bx + 2.0, by, bar_w - 4.0, bh) {
                        pixmap.fill_rect(r, &p, ts, None);
                    }
                }
            }
            "line" | "area" => {
                let step = plot_w / (n - 1).max(1) as f32;
                if chart_type == "area" {
                    let mut pb = PathBuilder::new();
                    pb.move_to(x + margin, y + h - margin);
                    for (i, &v) in values.iter().enumerate() {
                        let px = x + margin + i as f32 * step;
                        let py = y + h - margin - (v / max_val) * plot_h;
                        pb.line_to(px, py);
                    }
                    pb.line_to(x + margin + plot_w, y + h - margin);
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let mut p_fill = Paint::default();
                        p_fill.set_color_rgba8(89, 161, 79, 50);
                        pixmap.fill_path(&path, &p_fill, FillRule::Winding, ts, None);
                    }
                }

                let mut pb = PathBuilder::new();
                for (i, &v) in values.iter().enumerate() {
                    let px = x + margin + i as f32 * step;
                    let py = y + h - margin - (v / max_val) * plot_h;
                    if i == 0 {
                        pb.move_to(px, py);
                    } else {
                        pb.line_to(px, py);
                    }
                }
                if let Some(path) = pb.finish() {
                    let mut p = Paint::default();
                    p.set_color_rgba8(89, 161, 79, 255);
                    let mut stroke = Stroke::default();
                    stroke.width = 2.0;
                    pixmap.stroke_path(&path, &p, &stroke, ts, None);
                }
            }
            "pie" | "donut" => {
                let donut = chart_type == "donut";
                let total: f32 = values.iter().copied().sum::<f32>().abs().max(1.0);
                let cx = x + w / 2.0;
                let cy = y + h / 2.0 + 10.0;
                let r = (w.min(h) / 2.0 - 30.0).max(20.0);
                let ir = if donut { r * 0.55 } else { 0.0 };
                let mut sa = -90.0f32;
                let colors = [(78, 121, 167), (242, 142, 43), (89, 161, 79), (225, 87, 89)];
                for (i, &v) in values.iter().enumerate() {
                    let sw = (v / total) * 360.0;
                    if sw < 0.5 {
                        sa += sw;
                        continue;
                    }
                    let mut pb = PathBuilder::new();
                    let start_rad = sa.to_radians();
                    let _end_rad = (sa + sw).to_radians();

                    if !donut {
                        pb.move_to(cx, cy);
                    } else {
                        pb.move_to(cx + ir * start_rad.cos(), cy + ir * start_rad.sin());
                    }

                    pb.line_to(cx + r * start_rad.cos(), cy + r * start_rad.sin());
                    let steps = (sw / 5.0).max(5.0) as i32;
                    for s in 1..=steps {
                        let a = (sa + sw * (s as f32 / steps as f32)).to_radians();
                        pb.line_to(cx + r * a.cos(), cy + r * a.sin());
                    }

                    if donut {
                        for s in (0..=steps).rev() {
                            let a = (sa + sw * (s as f32 / steps as f32)).to_radians();
                            pb.line_to(cx + ir * a.cos(), cy + ir * a.sin());
                        }
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        let (rc, gc, bc) = colors[i % colors.len()];
                        let mut p = Paint::default();
                        p.set_color_rgba8(rc, gc, bc, 255);
                        pixmap.fill_path(&path, &p, FillRule::Winding, ts, None);
                        let mut p_border = Paint::default();
                        p_border.set_color_rgba8(22, 27, 34, 255);
                        let mut stroke = Stroke::default();
                        stroke.width = 2.0;
                        pixmap.stroke_path(&path, &p_border, &stroke, ts, None);
                    }
                    sa += sw;
                }
            }
            "hbar" => {
                let bar_h = plot_h / n as f32;
                for (i, &v) in values.iter().enumerate() {
                    let bw = (v / max_val) * plot_w;
                    let bx = x + margin;
                    let by = y + margin + i as f32 * bar_h;
                    let mut p = Paint::default();
                    p.set_color_rgba8(176, 122, 161, 255);
                    if let Some(r) = Rect::from_xywh(bx, by + 2.0, bw, bar_h - 4.0) {
                        pixmap.fill_rect(r, &p, ts, None);
                    }
                }
            }
            "scatter" => {
                let step = plot_w / (n - 1).max(1) as f32;
                for (i, &v) in values.iter().enumerate() {
                    let px = x + margin + i as f32 * step;
                    let py = y + h - margin - (v / max_val) * plot_h;
                    let mut p = Paint::default();
                    p.set_color_rgba8(78, 121, 167, 100);
                    if let Some(r) = Rect::from_xywh(px - 6.0, py - 6.0, 12.0, 12.0) {
                        pixmap.fill_rect(r, &p, ts, None);
                    }
                    p.set_color_rgba8(78, 121, 167, 255);
                    if let Some(r) = Rect::from_xywh(px - 3.0, py - 3.0, 6.0, 6.0) {
                        pixmap.fill_rect(r, &p, ts, None);
                    }
                }
            }
            "gauge" => {
                let pct = (values[0] - 95.0) / 5.0;
                let pct = pct.clamp(0.0, 1.0);
                let cx = x + w / 2.0;
                let cy = y + h / 2.0 + 20.0;
                let r = (w.min(h) / 2.0 - 30.0).max(20.0);

                let mut pb_track = PathBuilder::new();
                for i in 0..=36 {
                    let a = (180.0 + i as f32 * 5.0).to_radians();
                    if i == 0 {
                        pb_track.move_to(cx + r * a.cos(), cy + r * a.sin());
                    } else {
                        pb_track.line_to(cx + r * a.cos(), cy + r * a.sin());
                    }
                }
                if let Some(path) = pb_track.finish() {
                    let mut p = Paint::default();
                    p.set_color_rgba8(40, 45, 55, 255);
                    let mut stroke = Stroke::default();
                    stroke.width = 10.0;
                    stroke.line_cap = LineCap::Round;
                    pixmap.stroke_path(&path, &p, &stroke, ts, None);
                }

                let mut pb_fill = PathBuilder::new();
                let steps = (pct * 36.0) as i32;
                for i in 0..=steps {
                    let a = (180.0 + i as f32 * 5.0).to_radians();
                    if i == 0 {
                        pb_fill.move_to(cx + r * a.cos(), cy + r * a.sin());
                    } else {
                        pb_fill.line_to(cx + r * a.cos(), cy + r * a.sin());
                    }
                }
                if let Some(path) = pb_fill.finish() {
                    let mut p = Paint::default();
                    p.set_color_rgba8(63, 185, 80, 255);
                    let mut stroke = Stroke::default();
                    stroke.width = 10.0;
                    stroke.line_cap = LineCap::Round;
                    pixmap.stroke_path(&path, &p, &stroke, ts, None);
                }
            }
            _ => {}
        }
    }

    fn accessibility_role(&self) -> &str {
        "img"
    }

    fn accessibility_label(&self, node: &WebCore) -> Option<String> {
        let chart_type = get_attr(node, "data-type", "chart");
        Some(format!("{} chart", chart_type))
    }
}

struct AppState {
    interaction_count: i32,
    cycle_count: i32,
    refresh_count: i32,
    log_counter: i32,
}

fn bump_interaction(root: &mut WebCore, state: &mut AppState) {
    state.interaction_count += 1;
    if let Some(c) = dom::query_selector_mut(root, "#click-count") {
        dom::set_text_content(c, &state.interaction_count.to_string());
    }
    if let Some(c) = dom::query_selector_mut(root, "#cycle-count") {
        dom::set_text_content(c, &state.cycle_count.to_string());
    }
    if let Some(c) = dom::query_selector_mut(root, "#refresh-count") {
        dom::set_text_content(c, &state.refresh_count.to_string());
    }
}

fn update_status(root: &mut WebCore, detail: &str) {
    if let Some(st) = dom::query_selector_mut(root, "#status-text") {
        dom::set_text_content(st, detail);
    }
}

fn log_event(root: &mut WebCore, state: &mut AppState, etype: &str, detail: &str) {
    state.log_counter += 1;
    // Shift log lines
    for i in (2..=5).rev() {
        let src_id = format!("#log{}", i - 1);
        let dst_id = format!("#log{}", i);
        let src_text = dom::query_selector(root, &src_id)
            .map(|b| dom::get_text_content(b))
            .unwrap_or_default();
        if let Some(dst) = dom::query_selector_mut(root, &dst_id) {
            dom::set_text_content(dst, &src_text);
        }
    }
    if let Some(log1) = dom::query_selector_mut(root, "#log1") {
        let msg = format!("{} #{} {}", etype, state.log_counter, detail);
        dom::set_text_content(log1, &msg);
    }
}

fn scale_all_charts(root: &mut WebCore, mult: f32) {
    let graph_ids = dom::query_selector_all_ids(root, "graph");
    for gid in graph_ids {
        let g = dom::find_box_mut(root, gid).unwrap();
        let vals_str = dom::get_attribute(g, "data-values")
            .unwrap_or("")
            .to_string();
        if vals_str.is_empty() {
            continue;
        }
        let new_vals: Vec<String> = vals_str
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .map(|v| format!("{:.0}", v * mult))
            .collect();
        dom::set_attribute(g, "data-values", &new_vals.join(","));
    }
}

fn randomize_all_charts(root: &mut WebCore) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let graph_ids = dom::query_selector_all_ids(root, "graph");
    for gid in graph_ids {
        let g = dom::find_box_mut(root, gid).unwrap();
        let vals_str = dom::get_attribute(g, "data-values")
            .unwrap_or("")
            .to_string();
        if vals_str.is_empty() {
            continue;
        }
        let new_vals: Vec<String> = vals_str
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .map(|v| {
                let factor: f32 = rng.gen_range(0.6..1.4);
                format!("{:.0}", (v * factor).max(1.0))
            })
            .collect();
        dom::set_attribute(g, "data-values", &new_vals.join(","));
    }
}

struct App {
    window: Option<Arc<Window>>,
    platform: Option<Platform>,
    renderer: Renderer,
    doc: Option<Document>,
    state: Arc<Mutex<AppState>>,
    width: f32,
    mouse_pos: (f32, f32),
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("graph_demo — webcore")
                        .with_inner_size(winit::dpi::LogicalSize::new(1100u32, 860u32)),
                )
                .unwrap(),
        );
        let platform = Platform::new_windowed(window.clone());
        self.width = platform.logical_width();

        self.renderer
            .register_trait_component("graph", GraphComponent);
        let mut doc = self.renderer.load_html_vp(HTML, self.width, 860.0);

        let state = self.state.clone();

        // Interactivity: Cycle chart type on click using library event system
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "graph") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let types = [
                    "bar", "line", "area", "pie", "donut", "hbar", "scatter", "gauge",
                ];
                let (cur_type, elem_id) = dom::find_box_mut(root, cur_id)
                    .map(|t| {
                        (
                            t.attributes
                                .get("data-type")
                                .cloned()
                                .unwrap_or("bar".to_string()),
                            dom::get_attribute(t, "id").unwrap_or("?").to_string(),
                        )
                    })
                    .unwrap_or(("bar".to_string(), "?".to_string()));
                let idx = types.iter().position(|t| *t == cur_type).unwrap_or(0);
                let next = types[(idx + 1) % types.len()];
                if let Some(target_mut) = dom::find_box_mut(root, cur_id) {
                    dom::set_attribute(target_mut, "data-type", next);
                    target_mut.layout.layout_dirty = true;
                }

                let mut st = state.lock().unwrap();
                st.cycle_count += 1;
                bump_interaction(root, &mut st);
                update_status(root, &format!("Cycled {} to {}", elem_id, next));
                log_event(
                    root,
                    &mut st,
                    "CLICK",
                    &format!("graph#{} -> {}", elem_id, next),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // All Bar/Line/etc buttons
        let types = [
            ("bar", "#btn-bar"),
            ("line", "#btn-line"),
            ("pie", "#btn-pie"),
            ("area", "#btn-area"),
            ("scatter", "#btn-scatter"),
        ];
        for (t, id) in types {
            let state = self.state.clone();
            let __root = doc.root.node_id;
            doc.add_event_listener(
                __root,
                "click",
                Box::new(move |evt, __d: &mut webcore::Document| {
                    // Delegation, the way a page writes it: one listener, then
                    // `closest()` to find which matching element was hit.
                    let Some(__cur) = __d.closest(evt.target, id) else {
                        return;
                    };
                    let root = &mut __d.root;
                    let _ = &root;
                    let graph_ids = dom::query_selector_all_ids(root, "graph");
                    for gid in graph_ids {
                        let g = dom::find_box_mut(root, gid).unwrap();
                        dom::set_attribute(g, "data-type", t);
                    }
                    let mut st = state.lock().unwrap();
                    bump_interaction(root, &mut st);
                    update_status(root, &format!("All charts set to {}", t));
                    log_event(root, &mut st, "CLICK", &format!("btn -> all = {}", t));
                }),
                webcore::dom::events::ListenerOptions::default(),
            );
        }

        // Randomize
        let state = self.state.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-rand") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                randomize_all_charts(root);
                let mut st = state.lock().unwrap();
                st.refresh_count += 1;
                bump_interaction(root, &mut st);
                update_status(root, "All chart data randomized!");
                log_event(root, &mut st, "CLICK", "btn-rand -> randomized all data");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // Grow
        let state = self.state.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-grow") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                scale_all_charts(root, 1.1);
                let mut st = state.lock().unwrap();
                bump_interaction(root, &mut st);
                update_status(root, "All values grew +10%");
                log_event(root, &mut st, "CLICK", "btn-grow -> +10%");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // Shrink
        let state = self.state.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "#btn-shrink") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                scale_all_charts(root, 0.9);
                let mut st = state.lock().unwrap();
                bump_interaction(root, &mut st);
                update_status(root, "All values shrank -10%");
                log_event(root, &mut st, "CLICK", "btn-shrink -> -10%");
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // Sidebar selection
        let state = self.state.clone();
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".sb-item") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                let all_ids = dom::query_selector_all_ids(root, ".sb-item");
                for bid in all_ids {
                    if let Some(b) = dom::find_box_mut(root, bid) {
                        dom::remove_class(b, "sb-item-active");
                    }
                }
                if let Some(target_mut) = dom::find_box_mut(root, cur_id) {
                    dom::add_class(target_mut, "sb-item-active");
                }

                let id = dom::find_box_mut(root, cur_id)
                    .and_then(|t| dom::get_attribute(t, "id").map(|s| s.to_string()))
                    .unwrap_or_default();
                let id = id.as_str();
                let mult = match id {
                    "sb-home" => 1.0,
                    "sb-products" => 0.75,
                    "sb-pricing" => 0.5,
                    "sb-blog" => 0.4,
                    "sb-docs" => 0.3,
                    "sb-about" => 0.2,
                    "sb-organic" => 1.1,
                    "sb-direct" => 0.65,
                    "sb-referral" => 0.35,
                    "sb-social" => 0.25,
                    "sb-desktop" => 1.0,
                    "sb-mobile" => 0.6,
                    "sb-tablet" => 0.15,
                    _ => 1.0,
                };
                scale_all_charts(root, mult);

                let mut st = state.lock().unwrap();
                bump_interaction(root, &mut st);
                update_status(root, &format!("Showing data for {}", id));
                log_event(
                    root,
                    &mut st,
                    "NAV",
                    &format!("{} (scale={:.0}%)", id, mult * 100.0),
                );
            }),
            webcore::dom::events::ListenerOptions::default(),
        );

        // KPI selection
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, ".kpi") else {
                    return;
                };
                let root = &mut __d.root;
                let _ = &root;
                let cur_id = __cur;
                if let Some(target_mut) = dom::find_box_mut(root, cur_id) {
                    dom::toggle_class(target_mut, "kpi-selected");
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
                    let mut engine = self.renderer.layout_engine();
                    engine.layout(doc, self.width);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (
                    position.x as f32 / platform.scale_factor(),
                    position.y as f32 / platform.scale_factor(),
                );
                let zoom = self.renderer.zoom;
                if let Some(doc) = self.doc.as_mut() {
                    let (sx, sy) = self.mouse_pos;
                    let pt = (sx / zoom, sy / zoom + doc.scroll_y);
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
                let zoom = self.renderer.zoom;
                let (sx, sy) = self.mouse_pos;
                let pt = (sx / zoom, sy / zoom);
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (pt.0, pt.1 + doc.scroll_y);
                    match state {
                        ElementState::Pressed => {
                            doc.process_mouse_event(HtmlEventType::MouseDown, doc_pt, 0);
                        }
                        ElementState::Released => {
                            doc.process_mouse_event(HtmlEventType::MouseUp, doc_pt, 0);
                            if doc.process_mouse_event(HtmlEventType::Click, doc_pt, 0) {
                                // Click handlers change attributes/text. The engine
                                // decides what needs relayout vs repaint.
                                let mut engine = self.renderer.layout_engine();
                                engine.layout(doc, self.width);
                            }
                        }
                        _ => {}
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 20.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        p.y as f32 / platform.scale_factor()
                    }
                };
                let mp = self.mouse_pos;
                if let Some(doc) = self.doc.as_mut() {
                    let doc_pt = (mp.0, mp.1 + doc.scroll_y);
                    doc.process_wheel_event(doc_pt, dy);
                }
                window.request_redraw();
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
        width: 1100.0,
        state: Arc::new(Mutex::new(AppState {
            interaction_count: 0,
            cycle_count: 0,
            refresh_count: 0,
            log_counter: 0,
        })),
        mouse_pos: (0.0, 0.0),
    };
    event_loop.run_app(&mut app).unwrap();
}
