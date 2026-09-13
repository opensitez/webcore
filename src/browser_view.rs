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
use crate::html::resolve_url;
use crate::loading::{PageLoadOptions, PageSession, PageSessionEvent};
use crate::renderer::Renderer;
use crate::types::{
    CSSCursor, Document, FormEvent, FormEventKind, WebCore, build_form_submit_url,
    collect_form_data, find_parent_form_action,
};

enum BrowserViewLoadResult {
    Page {
        load_id: usize,
        url: String,
        doc: Box<Document>,
        preview: bool,
    },
}

/// Opaque browser-page area. This is the unit a GUI toolkit should embed.
pub struct BrowserView {
    renderer: Renderer,
    doc: Option<Document>,
    url: String,
    title: String,
    loading: bool,
    width: f32,
    height: f32,
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
            url: String::new(),
            title: String::new(),
            loading: false,
            width,
            height,
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
        self.doc.as_ref()
    }

    pub fn document_mut(&mut self) -> Option<&mut Document> {
        self.doc.as_mut()
    }

    pub fn document_and_renderer_mut(&mut self) -> Option<(&mut Document, &mut Renderer)> {
        let doc = self.doc.as_mut()?;
        Some((doc, &mut self.renderer))
    }

    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        self.renderer.handle_window_event(event, self.doc.as_mut());
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
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        doc.style_dirty = true;
        doc.stylesheet.inspect_mode = on;
        let engine = self.renderer.layout_engine();
        engine.viewport_h = self.height;
        engine.layout(doc, self.width);
        self.renderer.invalidate_display_list();
        self.invalidate_backing();
        self.wake();
        true
    }

    pub fn relayout(&mut self) -> bool {
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let engine = self.renderer.layout_engine();
        engine.viewport_h = self.height;
        engine.layout(doc, self.width);
        self.renderer.invalidate_display_list();
        self.invalidate_backing();
        self.wake();
        true
    }

    pub fn benchmark_progressive_layout(&mut self) -> Option<(f64, f64)> {
        let Some(doc) = self.doc.as_mut() else {
            return None;
        };
        fn mark_dirty(node: &mut WebCore) {
            node.layout.layout_dirty = true;
            for child in &mut node.children {
                mark_dirty(child);
            }
        }

        let engine = self.renderer.layout_engine();
        mark_dirty(&mut doc.root);
        let t0 = std::time::Instant::now();
        engine.layout(doc, self.width);
        let full_ms = t0.elapsed().as_micros() as f64 / 1000.0;

        mark_dirty(&mut doc.root);
        let t1 = std::time::Instant::now();
        let _more = engine.layout_above_fold(doc, self.width);
        let above_ms = t1.elapsed().as_micros() as f64 / 1000.0;
        engine.layout_remainder(doc, self.width);
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
        if let Some(doc) = self.doc.as_mut() {
            let engine = self.renderer.layout_engine();
            engine.viewport_h = self.height;
            engine.layout(doc, self.width);
            self.renderer.invalidate_display_list();
        }
        self.wake();
    }

    pub fn navigate(&mut self, url: String) {
        self.url = url.clone();
        self.title = "Loading...".to_string();
        self.loading = true;
        self.doc = None;
        self.invalidate_backing();
        self.load_id = self.load_id.wrapping_add(1);
        let load_id = self.load_id;
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        let session = PageSession::new(self.options.clone());
        session.navigate(url, self.width, self.height, move |event| match event {
            PageSessionEvent::Page { url, doc, preview } => {
                let _ = tx.send(BrowserViewLoadResult::Page {
                    load_id,
                    url,
                    doc,
                    preview,
                });
                if let Some(wake) = wake.as_ref() {
                    wake();
                }
            }
        });
    }

    pub fn load_until_ready(&mut self, url: String, timeout: std::time::Duration) -> bool {
        self.navigate(url);
        let deadline = std::time::Instant::now() + timeout;
        let mut changed = false;
        loop {
            changed |= self.poll();
            if self.doc.is_some() && !self.loading {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return changed;
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    pub fn poll(&mut self) -> bool {
        let mut pending = Vec::new();
        while let Ok(result) = self.rx.try_recv() {
            pending.push(result);
        }
        let mut latest: Option<BrowserViewLoadResult> = None;
        for result in pending {
            match &result {
                BrowserViewLoadResult::Page { load_id, .. } if *load_id == self.load_id => {
                    latest = Some(result);
                }
                _ => {}
            }
        }
        let Some(result) = latest else {
            return false;
        };
        match result {
            BrowserViewLoadResult::Page {
                url,
                mut doc,
                preview,
                ..
            } => {
                self.url = url.clone();
                self.title = if doc.title.is_empty() {
                    url.split('/')
                        .filter(|part| !part.is_empty())
                        .last()
                        .unwrap_or("Untitled")
                        .to_string()
                } else {
                    doc.title.clone()
                };
                self.loading = preview;
                let pending_navigate = self.pending_navigate.clone();
                let wake = self.wake.clone();
                let base_url = url.clone();
                doc.on_form_event = Some(Box::new(move |event: &FormEvent| {
                    if let FormEventKind::Submit(action) = &event.kind {
                        let target = if action.is_empty() {
                            base_url.clone()
                        } else {
                            resolve_url(&base_url, action)
                        };
                        *pending_navigate.lock().unwrap() = Some(target);
                        if let Some(wake) = wake.as_ref() {
                            wake();
                        }
                    }
                }));
                self.doc = Some(*doc);
                self.renderer.invalidate_display_list();
                self.invalidate_backing();
                self.wake();
                true
            }
        }
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
        let needs_redraw = self.renderer.drive_document_idle(
            event_loop,
            self.doc.as_mut(),
            self.width,
            self.height,
        );
        if needs_redraw {
            self.invalidate_backing();
        }
        changed || nav_changed || needs_redraw
    }

    pub fn paint_into(&mut self, target: &mut Pixmap, x: i32, y: i32, scale: f32) {
        let width_px = target.width().max(1);
        let height_px = target.height().max(1);
        let view_w = ((self.width * scale).ceil() as u32).max(1).min(width_px);
        let view_h = ((self.height * scale).ceil() as u32).max(1).min(height_px);
        let Some(doc) = self.doc.as_mut() else {
            fill_placeholder(target, x, y, view_w, view_h);
            return;
        };
        if x == 0 && y == 0 && target.width() == view_w && target.height() == view_h {
            self.renderer.render(doc, target, scale);
        } else if let Some(mut viewport) = Pixmap::new(view_w, view_h) {
            self.renderer.render(doc, &mut viewport, scale);
            blit_viewport_from_backing(&viewport, target, x, y, 0, view_w, view_h);
        }
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> bool {
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let old_scroll_y = doc.scroll_y;
        if doc.process_scrollbar_event(HtmlEventType::MouseMove, x, y, self.width, self.height)
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
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let old_scroll_y = doc.scroll_y;
        if doc.process_scrollbar_event(kind, x, y, self.width, self.height)
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
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let max_y = (Document::scroll_height(&doc.root) - self.height).max(0.0);
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
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        let max_y = (Document::scroll_height(&doc.root) - self.height).max(0.0);
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
        self.doc.as_ref().map(|doc| doc.scroll_y).unwrap_or(0.0)
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
                let moved = self.doc.as_mut().is_some_and(|doc| {
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
        let changed = self.doc.as_mut().is_some_and(|doc| {
            doc.process_key_event(event_type, key_code, ch, ctrl, shift, alt, meta)
        });
        if changed {
            self.invalidate_backing();
        }
        changed
    }

    pub fn cursor_at(&self, x: f32, y: f32) -> CSSCursor {
        self.doc
            .as_ref()
            .and_then(|doc| {
                crate::layout::hit_test::point_to_hit(&doc.root, (x, y + doc.scroll_y), 0)
            })
            .and_then(|hit| self.doc.as_ref()?.get_box_by_id(hit.node_id))
            .map(|node| node.style.cursor)
            .unwrap_or(CSSCursor::Auto)
    }

    fn handle_activation_at(&mut self, x: f32, y: f32) {
        let Some(doc) = self.doc.as_mut() else {
            return;
        };
        let pt = (x, y + doc.scroll_y);
        if let Some(href) = crate::layout::hit_test::hit_test_link(&doc.root, pt, 0) {
            self.navigate(resolve_url(&self.url, &href));
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
                        self.url.clone()
                    } else {
                        resolve_url(&self.url, &form_action)
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
        let Some(doc) = self.doc.as_ref() else {
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
            resolve_url(&self.url, &action)
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
