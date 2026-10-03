use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::Window;

use std::sync::Mutex;
use webcore::dom::HtmlEventType;
use webcore::platform::Platform;
use webcore::{Document, Renderer};

const HTML: &str = include_str!("html/graph.html");

#[path = "graph_demo/graph_element.rs"]
mod graph_element;


struct AppState {
    interaction_count: i32,
    cycle_count: i32,
    refresh_count: i32,
    log_counter: i32,
}

fn bump_interaction(doc: &mut Document, state: &mut AppState) {
    state.interaction_count += 1;
    if let Some(c) = doc.query_selector("#click-count") {
        doc.set_text_content(c, &state.interaction_count.to_string());
    }
    if let Some(c) = doc.query_selector("#cycle-count") {
        doc.set_text_content(c, &state.cycle_count.to_string());
    }
    if let Some(c) = doc.query_selector("#refresh-count") {
        doc.set_text_content(c, &state.refresh_count.to_string());
    }
}

fn update_status(doc: &mut Document, detail: &str) {
    if let Some(st) = doc.query_selector("#status-text") {
        doc.set_text_content(st, detail);
    }
}

fn log_event(doc: &mut Document, state: &mut AppState, etype: &str, detail: &str) {
    state.log_counter += 1;
    // Shift log lines
    for i in (2..=5).rev() {
        let src_id = format!("#log{}", i - 1);
        let dst_id = format!("#log{}", i);
        let src_text = doc.query_selector(&src_id)
            .map(|id| doc.text_content(id))
            .unwrap_or_default();
        if let Some(dst) = doc.query_selector(&dst_id) {
            doc.set_text_content(dst, &src_text);
        }
    }
    if let Some(log1) = doc.query_selector("#log1") {
        let msg = format!("{} #{} {}", etype, state.log_counter, detail);
        doc.set_text_content(log1, &msg);
    }
}

fn scale_all_charts(doc: &mut Document, mult: f32) {
    let graph_ids = doc.query_selector_all("vybe-graph");
    for gid in graph_ids {
        let vals_str = doc.get_attribute(gid, "data-values").unwrap_or_default();
        if vals_str.is_empty() {
            continue;
        }
        let new_vals: Vec<String> = vals_str
            .split(',')
            .filter_map(|s| s.trim().parse::<f32>().ok())
            .map(|v| format!("{:.0}", v * mult))
            .collect();
        doc.set_attribute(gid, "data-values", &new_vals.join(","));
    }
}

fn randomize_all_charts(doc: &mut Document) {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let graph_ids = doc.query_selector_all("vybe-graph");
    for gid in graph_ids {
        let vals_str = doc.get_attribute(gid, "data-values").unwrap_or_default();
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
        doc.set_attribute(gid, "data-values", &new_vals.join(","));
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

        let mut doc = self.renderer.load_html_vp(HTML, self.width, 860.0);
        doc.define_custom_element("vybe-graph", graph_element::GraphElement)
            .expect("graph definition");

        let state = self.state.clone();

        // Interactivity: Cycle chart type on click using library event system
        let __root = doc.root.node_id;
        doc.add_event_listener(
            __root,
            "click",
            Box::new(move |evt, __d: &mut webcore::Document| {
                // Delegation, the way a page writes it: one listener, then
                // `closest()` to find which matching element was hit.
                let Some(__cur) = __d.closest(evt.target, "vybe-graph") else {
                    return;
                };
                let cur_id = __cur;
                let types = [
                    "bar", "line", "area", "pie", "donut", "hbar", "scatter", "gauge",
                ];
                let cur_type = __d.get_attribute(cur_id, "data-type").unwrap_or("bar".to_string());
                let elem_id = __d.get_attribute(cur_id, "id").unwrap_or("?".to_string());
                let idx = types.iter().position(|t| *t == cur_type).unwrap_or(0);
                let next = types[(idx + 1) % types.len()];
                __d.set_attribute(cur_id, "data-type", next);

                let mut st = state.lock().unwrap();
                st.cycle_count += 1;
                bump_interaction(__d, &mut st);
                update_status(__d, &format!("Cycled {} to {}", elem_id, next));
                log_event(
                    __d,
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
                    let graph_ids = __d.query_selector_all("vybe-graph");
                    for gid in graph_ids {
                        __d.set_attribute(gid, "data-type", t);
                    }
                    let mut st = state.lock().unwrap();
                    bump_interaction(__d, &mut st);
                    update_status(__d, &format!("All charts set to {}", t));
                    log_event(__d, &mut st, "CLICK", &format!("btn -> all = {}", t));
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
                randomize_all_charts(__d);
                let mut st = state.lock().unwrap();
                st.refresh_count += 1;
                bump_interaction(__d, &mut st);
                update_status(__d, "All chart data randomized!");
                log_event(__d, &mut st, "CLICK", "btn-rand -> randomized all data");
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
                scale_all_charts(__d, 1.1);
                let mut st = state.lock().unwrap();
                bump_interaction(__d, &mut st);
                update_status(__d, "All values grew +10%");
                log_event(__d, &mut st, "CLICK", "btn-grow -> +10%");
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
                scale_all_charts(__d, 0.9);
                let mut st = state.lock().unwrap();
                bump_interaction(__d, &mut st);
                update_status(__d, "All values shrank -10%");
                log_event(__d, &mut st, "CLICK", "btn-shrink -> -10%");
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
                let cur_id = __cur;
                let all_ids = __d.query_selector_all(".sb-item");
                for bid in all_ids {
                    __d.class_list_remove(bid, "sb-item-active");
                }
                __d.class_list_add(cur_id, "sb-item-active");

                let id = __d.get_attribute(cur_id, "id").unwrap_or_default();
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
                scale_all_charts(__d, mult);

                let mut st = state.lock().unwrap();
                bump_interaction(__d, &mut st);
                update_status(__d, &format!("Showing data for {}", id));
                log_event(
                    __d,
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
                __d.class_list_toggle(__cur, "kpi-selected");
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
                            if doc.process_mouse_event(HtmlEventType::MouseUp, doc_pt, 0) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_data_changes_through_document_attributes() {
        let mut doc = Renderer::new().load_html_vp(HTML, 1100.0, 860.0);
        doc.define_custom_element("vybe-graph", graph_element::GraphElement)
            .unwrap();
        let mut graph_ids = Vec::new();
        Document::walk_all(&doc.root, &mut |node| {
            if node.tag == "vybe-graph" { graph_ids.push(node.node_id); }
        });
        assert!(!graph_ids.is_empty(), "the graph element is absent from the document tree");
        assert!(graph_ids.iter().all(|id| *id != 0), "graph nodes lack DOM ids: {graph_ids:?}");
        assert!(doc.query_selector_all("*").contains(&graph_ids[0]), "graph node is absent from universal selection");
        let graph = doc.query_selector("vybe-graph").unwrap();
        let canvas = doc.child_nodes(graph).into_iter()
            .find(|&id| doc.tag_name(id) == Some("canvas"))
            .expect("upgraded graph has a canvas");
        let pixels = doc.get_node(canvas).unwrap().image_data.as_ref().unwrap();
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] == 78 && pixel[1] == 121));
        let before = pixels.clone();
        doc.set_attribute(graph, "data-type", "pie");
        let after = doc.get_node(canvas).unwrap().image_data.as_ref().unwrap();
        assert_ne!(&before, after, "observed attribute did not redraw the canvas");
        let original = doc.get_attribute(graph, "data-values").unwrap();
        let first: f32 = original.split(',').next().unwrap().parse().unwrap();

        scale_all_charts(&mut doc, 2.0);

        let updated = doc.get_attribute(graph, "data-values").unwrap();
        assert_eq!(updated.split(',').next().unwrap().parse::<f32>().unwrap(), (first * 2.0).round());
    }
}
