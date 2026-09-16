//! Browser view/widget owned by webcore.
//!
//! Hosts create a view, give it a viewport, ask it to navigate, forward raw
//! input in host coordinates, and paint it into a surface. Loading, progressive
//! resource updates, form navigation, animation frames, scroll presentation, and
//! render caching stay inside webcore.

use std::sync::{Arc, Mutex, mpsc};

use tiny_skia::{Pixmap, Transform};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use crate::dom::HtmlEventType;
use crate::frame::EngineFrame;
use crate::html::resolve_url;
use crate::loading::{PageLoadEvent, PageLoadOptions, spawn_page_load};
use crate::renderer::Renderer;
use crate::types::{
    CSSCursor, Document, FormEvent, FormEventKind, WebCore, build_form_submit_url,
    collect_form_data, find_parent_form_action,
};

enum BrowserViewLoadResult {
    HtmlChunk {
        load_id: usize,
        url: String,
        html: String,
    },
    Complete {
        load_id: usize,
        url: String,
    },
}

/// Opaque browser-page area. This is the unit a GUI toolkit should embed.
pub struct BrowserView {
    renderer: Renderer,
    doc: Option<Document>,
    stream_frame: Option<EngineFrame>,
    streamed_html_len: usize,
    stream_paint_ready: bool,
    stream_needs_layout: bool,
    url: String,
    title: String,
    loading: bool,
    width: f32,
    height: f32,
    viewport_pixmap: Option<Pixmap>,
    options: PageLoadOptions,
    load_id: usize,
    tx: mpsc::Sender<BrowserViewLoadResult>,
    rx: mpsc::Receiver<BrowserViewLoadResult>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    pending_navigate: Arc<Mutex<Option<String>>>,
}

impl BrowserView {
    pub fn new(width: f32, height: f32, options: PageLoadOptions) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            renderer: Renderer::new(),
            doc: None,
            stream_frame: None,
            streamed_html_len: 0,
            stream_paint_ready: false,
            stream_needs_layout: false,
            url: String::new(),
            title: String::new(),
            loading: false,
            width,
            height,
            viewport_pixmap: None,
            options,
            load_id: 0,
            tx,
            rx,
            wake: None,
            pending_navigate: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_wake_callback(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }

    fn wake(&self) {
        if let Some(wake) = self.wake.as_ref() {
            wake();
        }
    }

    fn active_doc(&self) -> Option<&Document> {
        self.stream_frame
            .as_ref()
            .map(|frame| &frame.doc)
            .or(self.doc.as_ref())
    }

    fn active_doc_mut(&mut self) -> Option<&mut Document> {
        if let Some(frame) = self.stream_frame.as_mut() {
            Some(&mut frame.doc)
        } else {
            self.doc.as_mut()
        }
    }

    fn feed_streaming_chunk(&mut self, url: &str, html: &str) {
        if self.stream_frame.is_none() {
            let mut frame = EngineFrame::empty(self.width, self.height);
            frame.set_cache_dir(self.options.cache_dir.clone());
            frame.set_resource_wake(self.wake.clone());
            frame.start_streaming(url);
            self.stream_frame = Some(frame);
            self.streamed_html_len = 0;
            self.stream_paint_ready = false;
            self.stream_needs_layout = true;
        } else if self.streamed_html_len == 0
            && self
                .stream_frame
                .as_ref()
                .is_some_and(|frame| frame.doc.base_url != url)
        {
            if let Some(frame) = self.stream_frame.as_mut() {
                frame.start_streaming(url);
            }
        }
        if !html.is_empty() {
            if let Some(frame) = self.stream_frame.as_mut() {
                frame.feed_html_chunk(html.as_bytes());
                if !frame.doc.title.is_empty() {
                    self.title = frame.doc.title.clone();
                }
                self.stream_paint_ready |= streamed_tree_can_paint(&frame.doc.root);
            }
            self.streamed_html_len = self.streamed_html_len.saturating_add(html.len());
            self.stream_needs_layout = true;
        }
    }

    fn install_form_navigation_handler(&mut self, base_url: &str) {
        let pending_navigate = self.pending_navigate.clone();
        let wake = self.wake.clone();
        let base_url = base_url.to_string();
        let handler = Box::new(move |event: &FormEvent| {
            if let FormEventKind::Submit(action) = &event.kind {
                let target = if action.is_empty() {
                    base_url.clone()
                } else {
                    resolve_url(action, &base_url)
                };
                *pending_navigate.lock().unwrap() = Some(target);
                if let Some(wake) = wake.as_ref() {
                    wake();
                }
            }
        });
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.doc.on_form_event = Some(handler);
        } else if let Some(doc) = self.doc.as_mut() {
            doc.on_form_event = Some(handler);
        }
    }

    fn layout_active(&mut self) -> bool {
        if let Some(frame) = self.stream_frame.as_mut() {
            let engine = self.renderer.layout_engine();
            engine.viewport_h = self.height;
            frame.doc.style_dirty = true;
            engine.layout(&mut frame.doc, self.width);
            self.renderer.invalidate_display_list();
            self.stream_needs_layout = false;
            return true;
        }
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let engine = self.renderer.layout_engine();
        engine.viewport_h = self.height;
        engine.layout(doc, self.width);
        self.renderer.invalidate_display_list();
        true
    }

    fn ensure_streamed_paint_layout(&mut self) {
        if !self.stream_paint_ready {
            return;
        }
        let Some(frame) = self.stream_frame.as_mut() else {
            return;
        };
        let root = &frame.doc.root.layout;
        let needs_layout = frame.doc.style_dirty
            || frame.doc.root.layout.layout_dirty
            || !root.margin_rect.w.is_finite()
            || !root.margin_rect.h.is_finite()
            || root.margin_rect.w <= 0.0
            || root.margin_rect.h <= 0.0;
        if !needs_layout {
            self.stream_needs_layout = false;
            return;
        }
        let engine = self.renderer.layout_engine();
        engine.viewport_h = self.height;
        engine.layout(&mut frame.doc, self.width);
        self.renderer.invalidate_display_list();
        self.stream_needs_layout = false;
    }

    fn update_streamed_frame_before_paint(&mut self) -> bool {
        if !self.stream_paint_ready || !self.stream_needs_layout {
            return false;
        }
        self.layout_active()
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn set_options(&mut self, options: PageLoadOptions) {
        self.options = options;
    }

    pub fn document(&self) -> Option<&Document> {
        self.active_doc()
    }

    pub fn document_mut(&mut self) -> Option<&mut Document> {
        self.active_doc_mut()
    }

    pub fn document_and_renderer_mut(&mut self) -> Option<(&mut Document, &mut Renderer)> {
        if let Some(frame) = self.stream_frame.as_mut() {
            return Some((&mut frame.doc, &mut self.renderer));
        }
        let doc = self.doc.as_mut()?;
        Some((doc, &mut self.renderer))
    }

    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        if let Some(frame) = self.stream_frame.as_mut() {
            self.renderer
                .handle_window_event(event, Some(&mut frame.doc));
        } else {
            self.renderer.handle_window_event(event, self.doc.as_mut());
        }
    }

    pub fn is_shift_held(&self) -> bool {
        self.renderer.is_shift_held()
    }

    pub fn zoom(&self) -> f32 {
        self.renderer.zoom
    }

    pub fn invalidate_display(&mut self) {
        self.renderer.invalidate_display_list();
        self.invalidate_backing();
        self.wake();
    }

    pub fn set_inspect_mode(&mut self, on: bool) -> bool {
        let Some(doc) = self.active_doc_mut() else {
            return false;
        };
        doc.style_dirty = true;
        doc.stylesheet.inspect_mode = on;
        let _ = doc;
        self.layout_active();
        self.invalidate_backing();
        self.wake();
        true
    }

    pub fn relayout(&mut self) -> bool {
        if !self.layout_active() {
            return false;
        }
        self.invalidate_backing();
        self.wake();
        true
    }

    pub fn benchmark_progressive_layout(&mut self) -> Option<(f64, f64)> {
        if self.stream_frame.is_some() {
            return None;
        }
        let Some(doc) = self.doc.as_mut() else {
            return None;
        };
        let width = self.width;
        fn mark_dirty(node: &mut WebCore) {
            node.layout.layout_dirty = true;
            for child in &mut node.children {
                mark_dirty(child);
            }
        }

        let engine = self.renderer.layout_engine();
        mark_dirty(&mut doc.root);
        let t0 = std::time::Instant::now();
        engine.layout(doc, width);
        let full_ms = t0.elapsed().as_micros() as f64 / 1000.0;

        mark_dirty(&mut doc.root);
        let t1 = std::time::Instant::now();
        let _more = engine.layout_above_fold(doc, width);
        let above_ms = t1.elapsed().as_micros() as f64 / 1000.0;
        engine.layout_remainder(doc, width);
        self.renderer.invalidate_display_list();
        self.invalidate_backing();
        self.wake();
        Some((full_ms, above_ms))
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        if (self.width - width).abs() < 0.5 && (self.height - height).abs() < 0.5 {
            return;
        }
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.invalidate_backing();
        self.layout_active();
        self.wake();
    }

    pub fn navigate(&mut self, url: String) {
        self.url = url.clone();
        self.title = "Loading...".to_string();
        self.loading = true;
        self.doc = None;
        self.stream_frame = Some(EngineFrame::empty(self.width, self.height));
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.set_cache_dir(self.options.cache_dir.clone());
            frame.set_resource_wake(self.wake.clone());
            frame.start_streaming(&url);
        }
        self.streamed_html_len = 0;
        self.stream_paint_ready = false;
        self.stream_needs_layout = true;
        self.invalidate_backing();
        self.load_id = self.load_id.wrapping_add(1);
        let load_id = self.load_id;
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        let loader_wake = wake.clone();
        let mut load_options = self.options.clone();
        load_options.emit_preview = true;
        spawn_page_load(url, load_options, move |event| match event {
            PageLoadEvent::Chunk { url, html } => {
                let _ = tx.send(BrowserViewLoadResult::HtmlChunk { load_id, url, html });
                if let Some(wake) = loader_wake.as_ref() {
                    wake();
                }
            }
            PageLoadEvent::Complete { url } => {
                let _ = tx.send(BrowserViewLoadResult::Complete { load_id, url });
                if let Some(wake) = loader_wake.as_ref() {
                    wake();
                }
            }
        });
        if let Some(wake) = wake.as_ref() {
            wake();
        }
    }

    pub fn load_until_ready(&mut self, url: String, timeout: std::time::Duration) -> bool {
        self.navigate(url);
        let deadline = std::time::Instant::now() + timeout;
        let mut changed = false;
        loop {
            changed |= self.poll();
            if self.active_doc().is_some() && !self.loading {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return changed;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut completed_url = None::<String>;
        while let Ok(result) = self.rx.try_recv() {
            match result {
                BrowserViewLoadResult::HtmlChunk { load_id, url, html }
                    if load_id == self.load_id =>
                {
                    self.url = url.clone();
                    self.feed_streaming_chunk(&url, &html);
                    self.loading = true;
                    changed = true;
                }
                BrowserViewLoadResult::Complete { load_id, url } if load_id == self.load_id => {
                    completed_url = Some(url);
                }
                _ => {}
            }
        }
        if let Some(url) = completed_url {
            self.url = url.clone();
            if let Some(frame) = self.stream_frame.as_mut() {
                frame.finish_loading();
                self.stream_paint_ready = true;
                self.stream_needs_layout = true;
                self.title = if frame.doc.title.is_empty() {
                    url.split('/')
                        .filter(|part| !part.is_empty())
                        .next_back()
                        .unwrap_or("Untitled")
                        .to_string()
                } else {
                    frame.doc.title.clone()
                };
            } else {
                self.title = url
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .next_back()
                    .unwrap_or("Untitled")
                    .to_string();
            }
            self.loading = false;
            self.install_form_navigation_handler(&url);
            changed = true;
        }
        if changed {
            self.invalidate_backing();
            self.wake();
        }
        changed
    }

    pub fn drain_pending_navigation(&mut self) -> bool {
        let target = self.pending_navigate.lock().unwrap().take();
        if let Some(url) = target {
            self.navigate(url);
            true
        } else {
            false
        }
    }

    pub fn drive_idle(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let changed = self.poll();
        let nav_changed = self.drain_pending_navigation();
        let stream_layout_changed = self.update_streamed_frame_before_paint();
        let needs_redraw = if let Some(frame) = self.stream_frame.as_mut() {
            self.renderer.drive_document_idle(
                event_loop,
                Some(&mut frame.doc),
                self.width,
                self.height,
            )
        } else {
            self.renderer.drive_document_idle(
                event_loop,
                self.doc.as_mut(),
                self.width,
                self.height,
            )
        };
        if needs_redraw {
            self.invalidate_backing();
        }
        changed || nav_changed || stream_layout_changed || needs_redraw
    }

    pub fn paint_into(&mut self, target: &mut Pixmap, x: i32, y: i32, scale: f32) {
        self.ensure_streamed_paint_layout();
        let width_px = target.width().max(1);
        let height_px = target.height().max(1);
        let view_w = ((self.width * scale).ceil() as u32).max(1).min(width_px);
        let view_h = ((self.height * scale).ceil() as u32).max(1).min(height_px);
        if x == 0 && y == 0 && target.width() == view_w && target.height() == view_h {
            if let Some(frame) = self.stream_frame.as_mut() {
                if self.stream_paint_ready {
                    self.renderer.render(&mut frame.doc, target, scale);
                } else {
                    fill_placeholder(target, x, y, view_w, view_h);
                }
            } else if let Some(doc) = self.doc.as_mut() {
                self.renderer.render(doc, target, scale);
            } else {
                fill_placeholder(target, x, y, view_w, view_h);
            }
        } else {
            let needs_pixmap = self
                .viewport_pixmap
                .as_ref()
                .is_none_or(|pm| pm.width() != view_w || pm.height() != view_h);
            if needs_pixmap {
                self.viewport_pixmap = Pixmap::new(view_w, view_h);
            }
            if let Some(viewport) = self.viewport_pixmap.as_mut() {
                if let Some(frame) = self.stream_frame.as_mut() {
                    if self.stream_paint_ready {
                        self.renderer.render(&mut frame.doc, viewport, scale);
                        blit_viewport_from_backing(viewport, target, x, y, 0, view_w, view_h);
                    } else {
                        fill_placeholder(target, x, y, view_w, view_h);
                    }
                } else if let Some(doc) = self.doc.as_mut() {
                    self.renderer.render(doc, viewport, scale);
                    blit_viewport_from_backing(viewport, target, x, y, 0, view_w, view_h);
                } else {
                    fill_placeholder(target, x, y, view_w, view_h);
                }
            } else {
                fill_placeholder(target, x, y, view_w, view_h);
            }
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> bool {
        let width = self.width;
        let height = self.height;
        let Some(doc) = self.active_doc_mut() else {
            return false;
        };
        let old_scroll_y = doc.scroll_y;
        if doc.process_scrollbar_event(HtmlEventType::MouseMove, x, y, width, height)
            && (doc.scroll_y - old_scroll_y).abs() >= 0.5
        {
            self.wake();
            return true;
        }
        doc.process_mouse_event(HtmlEventType::MouseMove, (x, y + doc.scroll_y), 0);
        let needs_style = doc.hover_changed
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc.hover_sensitive_nodes.contains(&doc.prev_hovered_box));
        if !needs_style && doc.hover_changed {
            doc.hover_changed = false;
            doc.prev_hovered_box = doc.hovered_box;
        }
        needs_style
    }

    pub fn handle_mouse_button(&mut self, kind: HtmlEventType, x: f32, y: f32, button: u8) -> bool {
        let width = self.width;
        let height = self.height;
        let Some(doc) = self.active_doc_mut() else {
            return false;
        };
        let old_scroll_y = doc.scroll_y;
        if doc.process_scrollbar_event(kind, x, y, width, height)
            && (doc.scroll_y - old_scroll_y).abs() >= 0.5
        {
            self.wake();
            return true;
        }
        let changed = doc.process_mouse_event(kind, (x, y + doc.scroll_y), button);
        if matches!(kind, HtmlEventType::MouseUp) {
            self.handle_activation_at(x, y);
        }
        changed
    }

    pub fn handle_wheel(&mut self, dx: f32, dy: f32) -> bool {
        let height = self.height;
        let Some(doc) = self.active_doc_mut() else {
            return false;
        };
        let max_y = (Document::scroll_height(&doc.root) - height).max(0.0);
        let old_y = doc.scroll_y;
        doc.scroll_x = (doc.scroll_x + dx).max(0.0);
        doc.scroll_y = (doc.scroll_y + dy).clamp(0.0, max_y);
        if (doc.scroll_y - old_y).abs() >= 0.5 {
            self.wake();
            true
        } else {
            false
        }
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.handle_wheel(dx, dy)
    }

    pub fn scroll_to(&mut self, x: f32, y: f32) -> bool {
        let height = self.height;
        let Some(doc) = self.active_doc_mut() else {
            return false;
        };
        let max_y = (Document::scroll_height(&doc.root) - height).max(0.0);
        let old = (doc.scroll_x, doc.scroll_y);
        doc.scroll_x = x.max(0.0);
        doc.scroll_y = y.clamp(0.0, max_y);
        if (doc.scroll_y - old.1).abs() >= 0.5 || (doc.scroll_x - old.0).abs() >= 0.5 {
            self.wake();
            true
        } else {
            false
        }
    }

    pub fn scroll_y(&self) -> f32 {
        self.active_doc().map(|doc| doc.scroll_y).unwrap_or(0.0)
    }

    pub fn handle_key(
        &mut self,
        event_type: HtmlEventType,
        key_code: u32,
        ch: Option<char>,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> bool {
        if matches!(event_type, HtmlEventType::KeyDown) {
            if key_code == 9 {
                let moved = self.active_doc_mut().is_some_and(|doc| {
                    if shift {
                        doc.focus_prev()
                    } else {
                        doc.focus_next()
                    }
                });
                if moved {
                    self.invalidate_backing();
                }
                return moved;
            }
            if key_code == 13 && self.submit_focused_text_control() {
                return true;
            }
        }
        let changed = self.active_doc_mut().is_some_and(|doc| {
            doc.process_key_event(event_type, key_code, ch, ctrl, shift, alt, meta)
        });
        if changed {
            self.invalidate_backing();
        }
        changed
    }

    pub fn cursor_at(&self, x: f32, y: f32) -> CSSCursor {
        self.active_doc()
            .and_then(|doc| {
                crate::layout::hit_test::point_to_hit(&doc.root, (x, y + doc.scroll_y), 0)
            })
            .and_then(|hit| self.active_doc()?.get_box_by_id(hit.node_id))
            .map(|node| node.style.cursor)
            .unwrap_or(CSSCursor::Auto)
    }

    fn handle_activation_at(&mut self, x: f32, y: f32) {
        let view_url = self.url.clone();
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        let current_url = if doc.base_url.is_empty() {
            view_url.clone()
        } else {
            doc.base_url.clone()
        };
        let pt = (x, y + doc.scroll_y);
        if let Some(href) = crate::layout::hit_test::hit_test_link(&doc.root, pt, 0) {
            self.navigate(resolve_browser_target(&href, &current_url, &view_url));
            return;
        }
        if let Some(hit) = crate::layout::hit_test::point_to_hit(&doc.root, pt, 0) {
            let Some(node) = doc.get_box_by_id(hit.node_id) else {
                return;
            };
            let form_action = find_parent_form_action(&doc.root, hit.node_id);
            if matches!(node.tag.as_str(), "button" | "input") {
                let input_type = node
                    .attributes
                    .get("type")
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if node.tag == "button" || input_type == "submit" {
                    let target = if form_action.is_empty() {
                        current_url.clone()
                    } else {
                        resolve_browser_target(&form_action, &current_url, &view_url)
                    };
                    let data = find_containing_form(&doc.root, hit.node_id)
                        .map(collect_form_data)
                        .unwrap_or_default();
                    let url = build_form_submit_url(&target, "get", &data);
                    self.navigate(url);
                }
            }
        }
    }

    fn submit_focused_text_control(&mut self) -> bool {
        let Some(doc) = self.active_doc() else {
            return false;
        };
        let focused = doc.focused_box;
        if focused == 0 {
            return false;
        }
        let Some(node) = doc.get_box_by_id(focused) else {
            return false;
        };
        if node.tag != "input" {
            return false;
        }
        let input_type = node
            .attributes
            .get("type")
            .map(|s| s.as_str())
            .unwrap_or("text");
        if !matches!(input_type, "text" | "password" | "email" | "search") {
            return false;
        }
        let action = find_parent_form_action(&doc.root, focused);
        let data = find_containing_form(&doc.root, focused)
            .map(collect_form_data)
            .unwrap_or_default();
        let target = if action.is_empty() {
            self.url.clone()
        } else {
            let base_url = if doc.base_url.is_empty() {
                self.url.as_str()
            } else {
                doc.base_url.as_str()
            };
            resolve_browser_target(&action, base_url, &self.url)
        };
        let url = build_form_submit_url(&target, "get", &data);
        self.navigate(url);
        true
    }

    fn invalidate_backing(&mut self) {
        // BrowserView does not keep a second full viewport cache. Renderer owns
        // the retained surface/display-list cache so scroll, fixed overlays,
        // and scrollbar chrome stay in one presentation model.
    }
}

fn find_containing_form(root: &WebCore, target_id: u32) -> Option<&WebCore> {
    fn contains(node: &WebCore, target_id: u32) -> bool {
        node.node_id == target_id || node.children.iter().any(|child| contains(child, target_id))
    }

    if root.tag == "form" && contains(root, target_id) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(form) = find_containing_form(child, target_id) {
            return Some(form);
        }
    }
    None
}

fn streamed_tree_can_paint(root: &WebCore) -> bool {
    root.tag.eq_ignore_ascii_case("body")
        || root.children.iter().any(|child| {
            child.tag.eq_ignore_ascii_case("body")
                || child.tag.eq_ignore_ascii_case("main")
                || streamed_tree_can_paint(child)
        })
}

fn resolve_browser_target(raw: &str, document_base_url: &str, view_url: &str) -> String {
    let base = if document_base_url.is_empty() {
        view_url
    } else {
        document_base_url
    };
    resolve_url(raw, base)
}

fn fill_placeholder(target: &mut Pixmap, x: i32, y: i32, w: u32, h: u32) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(tiny_skia::Color::from_rgba8(26, 26, 29, 255));
    if let Some(rect) = tiny_skia::Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
        target.fill_rect(rect, &paint, Transform::identity(), None);
    }
}

fn blit_viewport_from_backing(
    backing: &Pixmap,
    target: &mut Pixmap,
    dst_x: i32,
    dst_y: i32,
    src_y: u32,
    width: u32,
    height: u32,
) {
    let max_w = width.min(backing.width()).min(target.width());
    if max_w == 0 || height == 0 || src_y >= backing.height() {
        return;
    }
    let backing_stride = backing.width() as usize * 4;
    let target_stride = target.width() as usize * 4;
    let copy_rows = height.min(backing.height().saturating_sub(src_y));
    let src_x_skip = if dst_x < 0 { (-dst_x) as u32 } else { 0 };
    let dst_x = dst_x.max(0) as u32;
    let dst_y_skip = if dst_y < 0 { (-dst_y) as u32 } else { 0 };
    let dst_y = dst_y.max(0) as u32;
    if src_x_skip >= max_w || dst_x >= target.width() || dst_y >= target.height() {
        return;
    }
    let copy_w = max_w
        .saturating_sub(src_x_skip)
        .min(target.width().saturating_sub(dst_x));
    let copy_h = copy_rows
        .saturating_sub(dst_y_skip)
        .min(target.height().saturating_sub(dst_y));
    if copy_w == 0 || copy_h == 0 {
        return;
    }
    let src_x = src_x_skip as usize;
    let dst_x = dst_x as usize;
    let src_start_y = src_y.saturating_add(dst_y_skip) as usize;
    let dst_start_y = dst_y as usize;
    let bytes = copy_w as usize * 4;
    let src = backing.data();
    let dst = target.data_mut();
    for row in 0..copy_h as usize {
        let src_off = (src_start_y + row) * backing_stride + src_x * 4;
        let dst_off = (dst_start_y + row) * target_stride + dst_x * 4;
        dst[dst_off..dst_off + bytes].copy_from_slice(&src[src_off..src_off + bytes]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_view_feeds_html_chunks_into_streaming_frame() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        let first =
            "<!doctype html><html><head><style>p{color:red}</style></head><body><p id='hello'>";
        view.feed_streaming_chunk(base, first);
        assert!(
            view.document()
                .unwrap()
                .get_element_by_id("hello")
                .is_some()
        );
        assert_eq!(view.streamed_html_len, first.len());

        let second = "Hello</p>";
        view.feed_streaming_chunk(base, second);
        let doc = view.document().unwrap();
        let node_id = doc.get_element_by_id("hello").unwrap();
        let node = doc.get_box_by_id(node_id).unwrap();
        assert_eq!(node.children.len(), 1);
        assert_eq!(node.children[0].text, "Hello");
        assert_eq!(view.streamed_html_len, first.len() + second.len());
    }

    #[test]
    fn browser_view_does_not_paint_head_only_stream_as_a_page() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><title>T</title><style>body{margin:0}</style>",
        );
        assert!(
            !view.stream_paint_ready,
            "head-only streamed chunks should preload resources, not paint as malformed content"
        );

        view.feed_streaming_chunk(base, "</head><body><p>Ready</p>");
        assert!(view.stream_paint_ready);
    }

    #[test]
    fn browser_view_lays_out_streamed_body_before_first_paint() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><style>body{margin:0}p{display:block;margin:0;height:24px}</style></head><body><p>Ready</p>",
        );

        assert!(view.stream_paint_ready);
        let before = view
            .stream_frame
            .as_ref()
            .unwrap()
            .doc
            .root
            .layout
            .margin_rect
            .h;
        assert!(before <= 0.0 || before.is_nan());

        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);

        let after = view
            .stream_frame
            .as_ref()
            .unwrap()
            .doc
            .root
            .layout
            .margin_rect
            .h;
        assert!(
            after > 0.0,
            "first streamed paint must consume UA/inline/current CSS through layout before rendering"
        );
    }

    #[test]
    fn browser_view_batches_streamed_chunks_into_one_layout_before_paint() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><style>body{margin:0}p{display:block;height:20px}</style></head><body>",
        );
        view.feed_streaming_chunk(base, "<p>A</p><p>B</p>");

        assert!(view.stream_needs_layout);
        assert!(view.update_streamed_frame_before_paint());
        assert!(!view.stream_needs_layout);

        let height_after = view
            .stream_frame
            .as_ref()
            .unwrap()
            .doc
            .root
            .layout
            .margin_rect
            .h;
        assert!(height_after > 0.0);

        assert!(
            !view.update_streamed_frame_before_paint(),
            "no new streamed DOM/style work should mean no surprise second layout"
        );
    }

    #[test]
    fn streamed_frame_inherits_browser_wake_callback() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        view.set_wake_callback(|| {});

        view.feed_streaming_chunk("https://example.test/", "<html><body>ready");

        assert!(
            view.stream_frame
                .as_ref()
                .is_some_and(|frame| frame.has_resource_wake()),
            "resource workers must wake the browser view instead of waiting for timer/input polling"
        );
    }

    #[test]
    fn streamed_picture_resolves_without_final_tree_pass() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/articles/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        view.feed_streaming_chunk(
            base,
            r#"<html><body><picture><source media="(min-width: 1px)" srcset="https://cdn.example.test/wide.webp 1x"><img id="hero" src="fallback.jpg"></picture>"#,
        );

        let doc = view.document().unwrap();
        let hero_id = doc.get_element_by_id("hero").unwrap();
        let hero = doc.get_box_by_id(hero_id).unwrap();
        assert_eq!(hero.resolved_src, "https://cdn.example.test/wide.webp");
    }

    #[test]
    fn first_streamed_chunk_can_correct_redirected_base_url() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame
            .as_mut()
            .unwrap()
            .start_streaming("https://example.test/requested/");

        view.feed_streaming_chunk(
            "https://cdn.example.test/final/",
            r#"<html><body><img id="hero" src="image.png">"#,
        );

        let doc = view.document().unwrap();
        assert_eq!(doc.base_url, "https://cdn.example.test/final/");
        let hero_id = doc.get_element_by_id("hero").unwrap();
        let hero = doc.get_box_by_id(hero_id).unwrap();
        assert_eq!(
            hero.resolved_src,
            "https://cdn.example.test/final/image.png"
        );
    }

    #[test]
    fn browser_navigation_resolves_targets_against_document_base() {
        let base = "https://www.bbc.co.uk/news/";
        let view_url = "https://www.bbc.co.uk/";

        assert_eq!(
            resolve_browser_target("/sport", base, view_url),
            "https://www.bbc.co.uk/sport"
        );
        assert_eq!(
            resolve_browser_target("articles/c123", base, view_url),
            "https://www.bbc.co.uk/news/articles/c123"
        );
        assert_eq!(
            resolve_browser_target("//static.files.bbci.co.uk/account", base, view_url),
            "https://static.files.bbci.co.uk/account"
        );
        assert_eq!(
            resolve_browser_target("", base, view_url),
            "https://www.bbc.co.uk/news/"
        );
    }

    #[test]
    fn browser_navigation_falls_back_to_view_url_without_document_base() {
        assert_eq!(
            resolve_browser_target("next", "", "https://example.test/dir/page.html"),
            "https://example.test/dir/next"
        );
    }
}
