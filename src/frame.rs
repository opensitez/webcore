//! Frame loop — the engine's self-contained update cycle.
//!
//! `EngineFrame` is the primary API for both **browser mode** and **app engine mode**.
//! The host should never need to call cascade/layout/paint manually — just feed
//! content and events, and the engine handles everything internally.
//!
//! ## Browser mode
//! ```ignore
//! let mut frame = EngineFrame::new(800.0, 600.0);
//! frame.load_html("<h1>Hello</h1>");
//! // ... on vsync:
//! if frame.update_frame() {
//!     renderer.render(&frame.doc, &mut pixmap, scale);
//! }
//! ```
//!
//! ## App engine mode
//! ```ignore
//! let mut frame = EngineFrame::new(800.0, 600.0);
//! frame.load_html("<div id='root'></div>");
//! let root = frame.query_selector("#root").unwrap();
//! frame.set_inner_html(root, "<button>Click me</button>");
//! // ... on vsync:
//! if frame.update_frame() {
//!     renderer.render(&frame.doc, &mut pixmap, scale);
//! }
//! ```
//!
//! ## Event handling
//! ```ignore
//! frame.mouse_move(x, y);       // hover tracking
//! frame.mouse_event(Click, pt, 0); // click
//! frame.scroll(0.0, -30.0);     // scroll
//! frame.resize(1024.0, 768.0);  // viewport change
//! frame.key_input("a");         // text input
//! ```
//!
//! The host only needs to:
//! 1. Create an EngineFrame
//! 2. Set content (load_html, DOM API, or navigate)
//! 3. Forward input events
//! 4. Call update_frame() on vsync — it returns true when pixels changed
//! 5. Paint using renderer.render() when update_frame() returns true

use crate::layout::LayoutEngine;
use crate::types::Document;

/// Callbacks the engine fires to notify the host of state changes.
/// The host implements this trait — the engine calls it, never the other way around.
///
/// All methods have default no-op implementations so the host only needs to
/// override what it cares about.
pub trait EngineCallbacks {
    /// First paint is ready (above-fold content laid out and display list built).
    fn on_first_paint(&mut self) {}
    /// Document fully loaded and laid out (all resources fetched).
    fn on_load_complete(&mut self) {}
    /// Document scroll extent changed — update scrollbar.
    fn on_scroll_height_changed(&mut self, _height: f32) {}
    /// Title changed (`<title>` element parsed or updated).
    fn on_title_changed(&mut self, _title: &str) {}
    /// Navigation requested (link clicked, form submitted).
    fn on_navigate(&mut self, _url: &str) {}
    /// Layout completed — for benchmarking / profiling.
    fn on_layout_complete(&mut self, _duration_ms: f32) {}
    /// Cursor style should change (pointer, text, default, etc.).
    fn on_cursor_changed(&mut self, _cursor: crate::types::CSSCursor) {}
}

/// No-op callbacks — used when host doesn't register any.
struct NoopCallbacks;
impl EngineCallbacks for NoopCallbacks {}

/// The self-contained engine. Wraps Document + LayoutEngine into a frame-based
/// update cycle. The host feeds content and events; the engine handles
/// cascade, layout, and display list internally.
pub struct EngineFrame {
    pub doc: Document,
    pub engine: LayoutEngine,
    /// Whether any DOM mutation or style change occurred since last frame.
    needs_style: bool,
    /// Whether layout needs to run (set when style changes geometry-affecting properties).
    needs_layout: bool,
    /// Whether the display needs redrawing (set after any visible change).
    needs_paint: bool,
    /// Viewport dimensions.
    viewport_w: f32,
    viewport_h: f32,
    /// Last reported scroll height — used to detect changes for callback.
    last_scroll_height: f32,
    /// Whether initial content has been loaded (for on_first_paint).
    first_paint_done: bool,
    /// Stateful HTML tokenizer/parser for progressive chunked loading.
    streaming_parser: Option<crate::html::streaming::StreamingParser>,
    /// Stylesheet results discovered while streaming HTML.
    stylesheet_tx: Option<std::sync::mpsc::Sender<crate::types::PendingStylesheetResult>>,
    scheduled_stylesheets: std::collections::HashSet<String>,
    image_tx: Option<std::sync::mpsc::Sender<crate::types::PendingImageResult>>,
    scheduled_images: std::collections::HashSet<String>,
    cache_dir: Option<String>,
    resource_wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    /// Host callbacks (boxed trait object).
    callbacks: Box<dyn EngineCallbacks>,
}

impl EngineFrame {
    /// Create from an existing Document.
    pub fn new(doc: Document, viewport_w: f32, viewport_h: f32) -> Self {
        let mut engine = LayoutEngine::new();
        engine.viewport_w = viewport_w;
        engine.viewport_h = viewport_h;
        Self {
            doc,
            engine,
            needs_style: true,
            needs_layout: true,
            needs_paint: true,
            viewport_w,
            viewport_h,
            last_scroll_height: 0.0,
            first_paint_done: false,
            streaming_parser: None,
            stylesheet_tx: None,
            scheduled_stylesheets: std::collections::HashSet::new(),
            image_tx: None,
            scheduled_images: std::collections::HashSet::new(),
            cache_dir: None,
            resource_wake: None,
            callbacks: Box::new(NoopCallbacks),
        }
    }

    /// Create an empty engine — content is set via `load_html()` or DOM API.
    pub fn empty(viewport_w: f32, viewport_h: f32) -> Self {
        let doc = crate::html::parse_html("<html><head></head><body></body></html>");
        Self::new(doc, viewport_w, viewport_h)
    }

    /// Register host callbacks. The engine pushes events — the host never polls.
    pub fn set_callbacks(&mut self, callbacks: impl EngineCallbacks + 'static) {
        self.callbacks = Box::new(callbacks);
    }

    pub fn set_cache_dir(&mut self, cache_dir: Option<String>) {
        self.cache_dir = cache_dir;
    }

    pub fn set_resource_wake(&mut self, wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
        self.resource_wake = wake;
    }

    #[cfg(test)]
    pub(crate) fn has_resource_wake(&self) -> bool {
        self.resource_wake.is_some()
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Content loading — the host just says "here's HTML" or "navigate to URL"
    // ═══════════════════════════════════════════════════════════════════════════

    /// Load HTML content. Parses, fetches external CSS, cascades, and lays out.
    /// This is the primary way to set content in browser mode.
    pub fn load_html(&mut self, html: &str) {
        self.load_html_with_base(html, "");
    }

    /// Load HTML with a base URL for resolving relative links and resources.
    pub fn load_html_with_base(&mut self, html: &str, base_url: &str) {
        if !self.doc.fire_window_event("beforeunload") {
            return;
        }
        self.doc.svg_trigger_projected_tree_event("unload");
        self.doc.fire_window_event("unload");
        self.doc = crate::load_html_reusing_with_stylesheet_loader_and_wait(
            html,
            base_url,
            self.viewport_w,
            self.viewport_h,
            self.engine.component_registry.clone(),
            None,
            None,
            std::time::Duration::ZERO,
        );
        self.first_paint_done = false;
        self.needs_style = true;
        self.needs_layout = true;
        self.needs_paint = true;
        self.engine.invalidate_cascade();
        // HTML §7.9: `DOMContentLoaded` once the document is parsed, then
        // `load` once it and its resources are ready. Both fired nowhere
        // before, so `window.onload` — the single most-used handler on the
        // web — never ran.
        self.doc.fire_window_event("DOMContentLoaded");
        self.doc.fire_window_event("load");
    }

    /// Set the full body HTML content (keeps existing <head> stylesheets).
    /// Lighter than load_html for app-style content updates.
    pub fn set_body_html(&mut self, html: &str) {
        if let Some(body_id) = self.doc.query_selector("body") {
            self.doc.set_inner_html(body_id, html);
            self.mark_style_dirty();
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Frame update — the core of the engine
    // ═══════════════════════════════════════════════════════════════════════════

    /// Run one frame of the engine update cycle.
    /// Returns `true` if the screen needs redrawing.
    ///
    /// Call this on every vsync (60fps), or after any event/mutation.
    /// It does the **minimum work needed**:
    /// - Nothing changed → returns false immediately (0ms).
    /// - Only hover changed → incremental cascade + layout on ~10 nodes.
    /// - DOM was mutated → cascade + layout on dirty subtrees.
    /// - Viewport resized → full cascade + layout.
    /// - Only scrolled → returns true (repaint only, no layout).
    pub fn update_frame(&mut self) -> bool {
        // 1. Poll for async stylesheets/images/fonts
        if self
            .doc
            .poll_pending_stylesheets_budgeted(32, std::time::Duration::from_millis(6))
        {
            self.needs_style = true;
            self.needs_layout = true;
            self.needs_paint = true;
        }
        let image_poll = self
            .doc
            .poll_pending_images_budgeted(32, std::time::Duration::from_millis(8));
        if image_poll.loaded_any {
            self.needs_layout |= image_poll.needs_relayout;
            self.needs_paint = true;
        }
        if self
            .engine
            .poll_pending_fonts_budgeted(usize::MAX, std::time::Duration::ZERO)
        {
            self.doc.style_dirty = true;
            self.needs_style = true;
            self.needs_layout = true;
            self.needs_paint = true;
        }

        if self.doc.tick_animated_images_in_viewport(
            std::time::Instant::now(),
            self.doc.scroll_y,
            self.viewport_h,
        ) {
            self.needs_paint = true;
        }

        // 2. Check if hover changed (set by process_mouse_event)
        if self.doc.hover_changed {
            self.needs_style = true;
            self.needs_layout = true;
            self.needs_paint = true;
        }

        // 3. Check for running animations
        if self.doc.needs_animation_frame {
            self.doc.tick_animations(std::time::Instant::now());
            let animation_needs_layout = self.doc.animation_overrides.values().any(|props| {
                crate::types::animation_runtime::animation_properties_affect_layout(props)
            });
            if animation_needs_layout {
                self.needs_style = true;
                self.needs_layout = true;
                self.needs_paint = true;
            } else if !self.doc.animation_overrides.is_empty() {
                self.needs_paint = true;
            }
        }

        // 4. Style + Layout (batched — all mutations since last frame processed at once)
        if self.needs_style || self.needs_layout {
            let t0 = std::time::Instant::now();
            if self.needs_style {
                self.engine.layout(&mut self.doc, self.viewport_w);
            } else {
                self.engine
                    .layout_no_cascade(&mut self.doc, self.viewport_w);
            }
            self.schedule_unscheduled_document_images();
            self.needs_style = false;
            self.needs_layout = false;
            self.needs_paint = true;

            // Notify host of layout completion
            let duration_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.callbacks.on_layout_complete(duration_ms);

            // Check if scroll height changed — notify host for scrollbar update
            let sh = crate::types::Document::scroll_height(&self.doc.root);
            if (sh - self.last_scroll_height).abs() > 1.0 {
                self.last_scroll_height = sh;
                self.callbacks.on_scroll_height_changed(sh);
            }

            // First paint callback
            if !self.first_paint_done {
                self.first_paint_done = true;
                self.callbacks.on_first_paint();
            }
        }

        // 5. Paint flag
        if self.needs_paint {
            self.needs_paint = false;
            return true;
        }

        false
    }

    /// Check if the engine needs a repaint without consuming the flag.
    pub fn needs_render(&self) -> bool {
        self.needs_paint
            || self.needs_style
            || self.needs_layout
            || self.doc.hover_changed
            || self.doc.needs_animation_frame
            || self
                .doc
                .has_visible_animated_images(self.doc.scroll_y, self.viewport_h)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Input events — host forwards raw events, engine handles everything
    // ═══════════════════════════════════════════════════════════════════════════

    /// Set the viewport size. Triggers re-cascade + re-layout on next frame.
    pub fn set_viewport(&mut self, w: f32, h: f32) {
        if (w - self.viewport_w).abs() > 0.5 || (h - self.viewport_h).abs() > 0.5 {
            self.viewport_w = w;
            self.viewport_h = h;
            self.engine.viewport_w = w;
            self.engine.viewport_h = h;
            // `window.onresize`. It resolved as a handler name and nothing ever
            // fired it, so a page that laid itself out on resize never ran.
            self.doc.fire_window_event("resize");
            self.needs_style = true; // media queries may change
            self.needs_layout = true;
            self.needs_paint = true;
        }
    }

    /// Alias for set_viewport.
    pub fn resize(&mut self, w: f32, h: f32) {
        self.set_viewport(w, h);
    }

    /// Scroll by delta. No layout needed — just repaint with new offset.
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        let doc_h = crate::types::Document::scroll_height(&self.doc.root);
        let doc_w = self.doc.root.layout.margin_rect.w;
        let view_h = self.viewport_h;
        let view_w = self.viewport_w;

        let new_y = if self.doc.viewport_y_scroll_locked() {
            self.doc.scroll_y
        } else {
            (self.doc.scroll_y + dy)
                .max(0.0)
                .min((doc_h - view_h).max(0.0))
        };
        let new_x = (self.doc.scroll_x + dx)
            .max(0.0)
            .min((doc_w - view_w).max(0.0));

        if (new_y - self.doc.scroll_y).abs() > 0.01 || (new_x - self.doc.scroll_x).abs() > 0.01 {
            self.doc.scroll_y = new_y;
            self.doc.scroll_x = new_x;
            // `window.onscroll` — fired AFTER the offset moves, so a handler
            // reading the scroll position sees the new one.
            self.doc.fire_window_event("scroll");
            self.needs_paint = true; // repaint only, no layout
        }
    }

    /// Set scroll position absolutely.
    pub fn scroll_to(&mut self, x: f32, y: f32) {
        if self
            .doc
            .viewport_scroll_to(x, y, self.viewport_w, self.viewport_h)
        {
            self.needs_paint = true;
        }
    }

    /// Mouse moved to document-space coordinates. Handles hover tracking.
    pub fn mouse_move(&mut self, doc_x: f32, doc_y: f32) {
        let new_hovered =
            crate::layout::hit_test::hit_test_box_at(&self.doc.root, (doc_x, doc_y), 0);
        if new_hovered != self.doc.hovered_box {
            self.doc.hovered_box = new_hovered;
            self.doc.hover_changed = true;
        }
    }

    /// Process a mouse event (click, mousedown, mouseup) and mark dirty if needed.
    pub fn mouse_event(
        &mut self,
        etype: crate::dom::HtmlEventType,
        doc_pt: (f32, f32),
        button: u8,
    ) -> bool {
        let redraw = self.doc.process_mouse_event(etype, doc_pt, button);
        if redraw {
            self.needs_paint = true;
            if self.doc.hover_changed {
                self.needs_style = true;
                self.needs_layout = true;
            }
        }
        redraw
    }

    /// Get the total scroll height of the document.
    pub fn scroll_height(&self) -> f32 {
        crate::types::Document::scroll_height(&self.doc.root)
    }

    /// Get the current scroll position.
    pub fn scroll_position(&self) -> (f32, f32) {
        (self.doc.scroll_x, self.doc.scroll_y)
    }

    /// Get the viewport dimensions.
    pub fn viewport(&self) -> (f32, f32) {
        (self.viewport_w, self.viewport_h)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Dirty tracking — mark what changed
    // ═══════════════════════════════════════════════════════════════════════════

    /// Mark that the DOM or styles have changed and need re-cascade.
    pub fn mark_style_dirty(&mut self) {
        self.doc.style_dirty = true;
        self.needs_style = true;
        self.needs_layout = true;
        self.needs_paint = true;
    }

    /// Mark that layout needs to run (e.g. after text content change).
    pub fn mark_layout_dirty(&mut self) {
        self.needs_layout = true;
        self.needs_paint = true;
    }

    /// Mark that the display needs redrawing (e.g. after cursor blink).
    pub fn mark_paint_dirty(&mut self) {
        self.needs_paint = true;
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // DOM API — mutations are queued, layout runs on next update_frame()
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get the root element's node ID (the <html> element).
    pub fn root_id(&self) -> u32 {
        self.doc.root.node_id
    }

    /// Query for an element by CSS selector. Returns node ID if found.
    pub fn query_selector(&self, selector: &str) -> Option<u32> {
        self.doc.query_selector(selector)
    }

    /// Create an element and mark style dirty.
    pub fn create_element(&mut self, tag: &str) -> u32 {
        let id = self.doc.create_element(tag);
        self.mark_style_dirty();
        id
    }

    /// Create a text node and mark style dirty.
    pub fn create_text(&mut self, text: &str) -> u32 {
        let id = self.doc.create_text_node(text);
        self.mark_style_dirty();
        id
    }

    /// Append child and mark dirty.
    pub fn append_child(&mut self, parent: u32, child: u32) {
        self.doc.append_child(parent, child);
        self.mark_style_dirty();
    }

    /// Remove child and mark dirty.
    pub fn remove_child(&mut self, child: u32) {
        self.doc.remove_child(child);
        self.mark_style_dirty();
    }

    /// Set attribute and mark dirty.
    pub fn set_attribute(&mut self, id: u32, key: &str, val: &str) {
        self.doc.set_attribute(id, key, val);
        self.mark_style_dirty();
    }

    /// Toggle class and mark dirty.
    pub fn toggle_class(&mut self, id: u32, class: &str) -> bool {
        let result = self.doc.class_list_toggle(id, class);
        self.mark_style_dirty();
        result
    }

    /// Add a CSS class.
    pub fn add_class(&mut self, id: u32, class: &str) {
        if let Some(node) = self.doc.get_box_by_id_mut(id) {
            crate::dom::add_class(node, class);
            self.mark_style_dirty();
        }
    }

    /// Remove a CSS class.
    pub fn remove_class(&mut self, id: u32, class: &str) {
        if let Some(node) = self.doc.get_box_by_id_mut(id) {
            crate::dom::remove_class(node, class);
            self.mark_style_dirty();
        }
    }

    /// Set inline style property and mark dirty.
    pub fn set_style(&mut self, id: u32, prop: &str, val: &str) {
        self.doc.set_style_property(id, prop, val);
        self.mark_style_dirty();
    }

    /// Set text content and mark dirty.
    pub fn set_text_content(&mut self, id: u32, text: &str) {
        self.doc.set_text_content(id, text);
        self.mark_style_dirty();
    }

    /// Set inner HTML and mark dirty.
    pub fn set_inner_html(&mut self, id: u32, html: &str) {
        self.doc.set_inner_html(id, html);
        self.mark_style_dirty();
    }

    /// Add a CSS stylesheet (like injecting a <style> tag).
    pub fn add_stylesheet(&mut self, css: &str) {
        self.doc.stylesheet.parse_and_add(css);
        self.mark_style_dirty();
        self.engine.invalidate_cascade();
    }

    /// Set a CSS variable on the root element.
    pub fn set_css_var(&mut self, name: &str, value: &str) {
        std::sync::Arc::make_mut(&mut self.doc.root.style)
            .custom_props
            .insert(name.to_string(), value.to_string());
        self.mark_style_dirty();
        self.engine.invalidate_cascade();
    }

    /// Apply a theme (set of CSS variables) on :root.
    pub fn set_theme(&mut self, vars: &[(&str, &str)]) {
        for &(name, value) in vars {
            std::sync::Arc::make_mut(&mut self.doc.root.style)
                .custom_props
                .insert(name.to_string(), value.to_string());
        }
        self.mark_style_dirty();
        self.engine.invalidate_cascade();
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Event API — host registers callbacks, engine dispatches
    // ═══════════════════════════════════════════════════════════════════════════

    /// Register an event listener on a node. Returns listener ID.
    pub fn on(
        &mut self,
        node_id: u32,
        event_type: &str,
        handler: crate::dom::events::EventHandler,
    ) -> u32 {
        self.doc
            .event_targets
            .add_event_listener(node_id, event_type, handler, false)
    }

    /// Register a capture-phase event listener. Returns listener ID.
    pub fn on_capture(
        &mut self,
        node_id: u32,
        event_type: &str,
        handler: crate::dom::events::EventHandler,
    ) -> u32 {
        self.doc
            .event_targets
            .add_event_listener(node_id, event_type, handler, true)
    }

    /// Remove an event listener by ID.
    pub fn off(&mut self, listener_id: u32) {
        self.doc.event_targets.remove_event_listener(listener_id);
    }

    /// Dispatch a DOM event through capture → target → bubble.
    pub fn dispatch_event(&mut self, event: &mut crate::dom::events::DomEvent) -> bool {
        self.doc.dispatch_dom_event(event)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Query API — read computed state
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get the computed bounding rectangle of an element (document coordinates).
    pub fn get_bounding_rect(&self, id: u32) -> Option<crate::types::Rect> {
        self.doc.get_node(id).map(|n| n.layout.border_rect)
    }

    /// Get the text content of an element.
    pub fn get_text_content(&self, id: u32) -> Option<String> {
        self.doc
            .get_node(id)
            .map(|n| crate::dom::get_text_content(n))
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, id: u32, key: &str) -> Option<String> {
        self.doc
            .get_node(id)
            .and_then(|n| n.attributes.get(key).cloned())
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Focus & Keyboard — the engine handles tab order, focus events, key routing
    // ═══════════════════════════════════════════════════════════════════════════

    /// Move focus to the next focusable element (Tab).
    /// Returns true if focus changed.
    pub fn focus_next(&mut self) -> bool {
        let changed = self.doc.focus_next();
        if changed {
            self.mark_style_dirty(); // :focus styles may change
        }
        changed
    }

    /// Move focus to the previous focusable element (Shift+Tab).
    pub fn focus_prev(&mut self) -> bool {
        let changed = self.doc.focus_prev();
        if changed {
            self.mark_style_dirty();
        }
        changed
    }

    /// Focus a specific element by node ID.
    pub fn focus(&mut self, node_id: u32) {
        if self.doc.focused_box != node_id {
            let old = self.doc.focused_box;
            self.doc.focused_box = node_id;
            self.doc.keyboard_focus = true;
            // Fire blur on old, focus on new
            if old != 0 {
                let mut e = crate::dom::HtmlEvent::new(crate::dom::HtmlEventType::Blur);
                e.target = old;
                e.related_target = node_id;
                self.doc.dispatch_input_event(e);
            }
            if node_id != 0 {
                let mut e = crate::dom::HtmlEvent::new(crate::dom::HtmlEventType::Focus);
                e.target = node_id;
                e.related_target = old;
                self.doc.dispatch_input_event(e);
            }
            self.mark_style_dirty();
        }
    }

    /// Remove focus from the currently focused element.
    pub fn blur(&mut self) {
        self.focus(0);
    }

    /// Get the currently focused element's node ID (0 = none).
    pub fn focused(&self) -> u32 {
        self.doc.focused_box
    }

    /// Handle a keyboard event. Routes to the focused element.
    /// Returns true if the event was handled (consumed).
    pub fn key_down(&mut self, key: &str, modifiers: u8) -> bool {
        // Tab / Shift+Tab → focus navigation
        if key == "Tab" {
            let shift = modifiers & 1 != 0;
            return if shift {
                self.focus_prev()
            } else {
                self.focus_next()
            };
        }

        // Escape → blur
        if key == "Escape" {
            self.blur();
            return true;
        }

        // Enter/Space → activate focused element (click)
        if (key == "Enter" || key == " ") && self.doc.focused_box != 0 {
            let focused = self.doc.focused_box;
            let node = self.doc.get_node(focused);
            if let Some(n) = node {
                let pt = (n.layout.content_rect.x + 1.0, n.layout.content_rect.y + 1.0);
                self.doc
                    .process_mouse_event(crate::dom::HtmlEventType::Click, pt, 0);
                self.mark_style_dirty();
                return true;
            }
        }

        // Route to focused element's event handler
        if self.doc.focused_box != 0 {
            // Check if focused element is a custom component
            if let Some(node) = self.doc.get_node(self.doc.focused_box) {
                let tag = node.tag.clone();
                if let Some(component) = self.engine.component_registry.get_component(&tag) {
                    let event = crate::types::ComponentEvent::KeyDown {
                        key: key.to_string(),
                        modifiers,
                    };
                    if let Some(node_mut) = self.doc.get_box_by_id_mut(self.doc.focused_box) {
                        if component.handle_event(node_mut, &event) {
                            self.mark_paint_dirty();
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    /// Handle text input (from IME or direct typing).
    pub fn text_input(&mut self, text: &str) -> bool {
        if self.doc.focused_box != 0 {
            if let Some(node) = self.doc.get_node(self.doc.focused_box) {
                let tag = node.tag.clone();
                if let Some(component) = self.engine.component_registry.get_component(&tag) {
                    let event = crate::types::ComponentEvent::TextInput {
                        text: text.to_string(),
                    };
                    if let Some(node_mut) = self.doc.get_box_by_id_mut(self.doc.focused_box) {
                        if component.handle_event(node_mut, &event) {
                            self.mark_paint_dirty();
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Accessibility — the engine provides an a11y tree for screen readers
    // ═══════════════════════════════════════════════════════════════════════════

    /// Get accessibility announcements (from aria-live regions).
    /// Call after update_frame() to get pending announcements.
    pub fn take_announcements(&mut self) -> Vec<crate::types::Announcement> {
        self.doc.take_announcements()
    }

    /// Check if there are pending accessibility announcements.
    pub fn has_announcements(&self) -> bool {
        !self.doc.pending_announcements.is_empty()
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Animation API — programmatic animations + CSS transition integration
    // ═══════════════════════════════════════════════════════════════════════════

    /// Start a CSS transition on a property. The engine interpolates from the
    /// current value to the target over the given duration.
    ///
    /// For compositor properties (transform, opacity), this is GPU-only — no
    /// layout or repaint needed. For layout properties (width, height, margin),
    /// this triggers incremental relayout each frame.
    pub fn animate(
        &mut self,
        node_id: u32,
        property: &str,
        target_value: &str,
        duration_ms: f32,
        easing: crate::types::EasingFn,
    ) {
        // Check if this is a compositor-only property
        let is_compositor = matches!(property, "transform" | "opacity" | "filter");

        if let Some(node) = self.doc.get_box_by_id_mut(node_id) {
            // Get current value as string for interpolation start point
            let current = match property {
                "opacity" => format!("{}", node.style.opacity),
                "transform" => node.style.transform.clone(),
                _ => String::new(),
            };

            // Create a transition state
            let transition = crate::types::TransitionState {
                property: property.to_string(),
                from_value: current.clone(),
                to_value: target_value.to_string(),
                reversing_adjusted_start_value: current,
                reversing_shortening_factor: 1.0,
                start_time: std::time::Instant::now(),
                duration_ms,
                delay_ms: 0.0,
                timing_fn: easing,
                allow_discrete: false,
            };

            // Add to document's active transitions
            self.doc
                .transition_states
                .entry(node_id)
                .or_insert_with(Vec::new)
                .push(transition);

            self.doc.needs_animation_frame = true;
        }

        if is_compositor {
            self.mark_paint_dirty(); // compositor-only: just repaint
        } else {
            self.mark_style_dirty(); // layout property: full cascade + layout
        }
    }

    /// Check if any animations are currently running.
    pub fn has_animations(&self) -> bool {
        self.doc.needs_animation_frame
            || !self.doc.active_animations.is_empty()
            || !self.doc.transition_states.is_empty()
            || self
                .doc
                .has_visible_animated_images(self.doc.scroll_y, self.viewport_h)
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Streaming — progressive HTML loading for browser mode
    // ═══════════════════════════════════════════════════════════════════════════

    /// Start loading HTML progressively via streaming parser.
    /// Feed chunks with `feed_html_chunk()`, finalize with `finish_loading()`.
    pub fn start_streaming(&mut self, base_url: &str) {
        self.doc = crate::html::parse_html("<html></html>");
        self.doc.root.children.clear();
        self.doc.rebuild_node_index();
        self.doc.base_url = base_url.to_string();
        self.doc.stylesheet = crate::css::ua_stylesheet();
        self.doc.preserve_stylesheet_document_order = true;
        let mut parser = crate::html::streaming::StreamingParser::new(base_url);
        parser.set_root_child_count(self.doc.root.children.len());
        self.streaming_parser = Some(parser);
        self.stylesheet_tx = None;
        self.scheduled_stylesheets.clear();
        self.image_tx = None;
        self.scheduled_images.clear();
        self.doc.pending_images = None;
        self.doc.images_in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.needs_style = true;
        self.needs_layout = true;
        self.needs_paint = true;
    }

    fn ensure_stylesheet_sender(
        &mut self,
    ) -> std::sync::mpsc::Sender<crate::types::PendingStylesheetResult> {
        if let Some(tx) = &self.stylesheet_tx {
            return tx.clone();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.doc.pending_stylesheets = Some(rx);
        self.stylesheet_tx = Some(tx.clone());
        tx
    }

    fn schedule_streamed_stylesheet(&mut self, slot_idx: usize, url: String) {
        let scheduled_key = format!("{slot_idx}\n{url}");
        if !self.scheduled_stylesheets.insert(scheduled_key) {
            return;
        }
        let tx = self.ensure_stylesheet_sender();
        let cache_dir = self.cache_dir.clone();
        let wake = self.resource_wake.clone();
        crate::spawn_css_resource_task(move || {
            let cache_key = format!("{url}\n");
            let loader: crate::StylesheetLoader = std::sync::Arc::new({
                let cache_dir = cache_dir.clone();
                move |css_url| crate::fetch_text_resource(css_url, cache_dir.as_deref())
            });
            let streaming_loader: crate::StreamingStylesheetLoader = std::sync::Arc::new({
                let cache_dir = cache_dir.clone();
                move |css_url, emit| {
                    crate::fetch_text_resource_streaming(css_url, cache_dir.as_deref(), emit)
                }
            });
            let loaded = crate::load_stylesheet_cached(
                cache_key,
                url.clone(),
                String::new(),
                loader,
                Some(streaming_loader),
                true,
                |sheet| {
                    let _ = tx.send((slot_idx, url.clone(), sheet, String::new()));
                    if let Some(wake) = wake.as_ref() {
                        wake();
                    }
                },
            );
            if loaded.emitted_fragments == 0 {
                let _ = tx.send((slot_idx, url, loaded.sheet, String::new()));
                if let Some(wake) = wake.as_ref() {
                    wake();
                }
            }
        });
    }

    fn ensure_image_sender(&mut self) -> std::sync::mpsc::Sender<crate::types::PendingImageResult> {
        if let Some(tx) = &self.image_tx {
            return tx.clone();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.doc.pending_images = Some(rx);
        self.image_tx = Some(tx.clone());
        tx
    }

    fn schedule_streamed_image(
        &mut self,
        path: Vec<usize>,
        target: crate::types::PendingImageTarget,
        url: String,
    ) {
        let node_id = crate::types::find_node_by_path_mut(&mut self.doc.root, &path)
            .map(|node| node.node_id)
            .unwrap_or(0);
        let key = format!("{target:?}:{path:?}:{url}");
        if url.is_empty() || !self.scheduled_images.insert(key) {
            return;
        }
        let tx = self.ensure_image_sender();
        let in_flight = self.doc.images_in_flight.clone();
        let cache_dir = self.cache_dir.clone();
        let wake = self.resource_wake.clone();
        in_flight.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        crate::spawn_image_resource_task(move || {
            let loader = cache_dir.map(|cache_dir| {
                std::sync::Arc::new(move |src: &str| {
                    let bytes = crate::loading::cached_fetch_bytes(src, &cache_dir)?;
                    crate::html::decode_image_bytes_ex(&bytes).ok_or_else(|| {
                        format!("unsupported image bytes: {} bytes from {src}", bytes.len())
                    })
                })
                    as std::sync::Arc<
                        dyn Fn(&str) -> Result<crate::html::DecodedImage, String>
                            + Send
                            + Sync
                            + 'static,
                    >
            });
            let result = crate::cached_decoded_image_result(&url, loader.as_deref());
            let event = match result {
                Ok(decoded) => crate::types::PendingImageResult::Loaded {
                    node_id,
                    path,
                    target,
                    url,
                    decoded,
                },
                Err(error) => crate::types::PendingImageResult::Failed {
                    node_id,
                    path,
                    target,
                    url,
                    error,
                },
            };
            let _ = tx.send(event);
            in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            if let Some(wake) = wake.as_ref() {
                wake();
            }
        });
    }

    fn schedule_streamed_element_image(
        &mut self,
        tag: &str,
        attributes: &crate::dom::attrs::AttrMap,
        path: Vec<usize>,
    ) {
        let requests = if tag == "video" {
            attributes
                .get("poster")
                .map(|poster| {
                    vec![(
                        crate::types::PendingImageTarget::Element,
                        crate::html::resolve_url(poster, &self.doc.base_url),
                    )]
                })
                .unwrap_or_default()
        } else if tag == "img" {
            let mut requests = Vec::new();
            if let Some(src) = find_node_by_path(&self.doc.root, &path)
                .and_then(crate::html::image_fallback_source)
                .or_else(|| crate::html::image_fallback_source_attrs(attributes))
            {
                requests.push((
                    crate::types::PendingImageTarget::ElementFallback,
                    crate::html::resolve_url(src, &self.doc.base_url),
                ));
            }
            find_node_by_path(&self.doc.root, &path)
                .and_then(|node| (!node.resolved_src.is_empty()).then(|| node.resolved_src.clone()))
                .or_else(|| {
                    crate::html::image_srcset_source_attrs(attributes)
                        .and_then(|srcset| {
                            crate::html::parse_srcset_url_for(
                                srcset,
                                attributes.get("sizes").map(String::as_str),
                                self.viewport_w,
                                self.viewport_h,
                                1.0,
                            )
                        })
                        .map(|candidate| crate::html::resolve_url(&candidate, &self.doc.base_url))
                })
                .map(|preferred| {
                    if !requests.iter().any(|(_, url)| url == &preferred) {
                        requests.push((crate::types::PendingImageTarget::Element, preferred));
                    }
                });
            requests
        } else {
            Vec::new()
        };
        for (target, url) in requests {
            self.schedule_streamed_image(path.clone(), target, url);
        }
    }

    fn apply_streamed_image_dimension_hints(&mut self, path: &[usize]) {
        let Some(node) = crate::types::find_node_by_path_mut(&mut self.doc.root, path) else {
            return;
        };
        if node.tag != "img" {
            return;
        }
        if let Some(w) = node
            .attributes
            .get("width")
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
        {
            node.image_width = w;
        }
        if let Some(h) = node
            .attributes
            .get("height")
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
        {
            node.image_height = h;
        }
    }

    fn resolve_streamed_image_source(&mut self, path: &[usize]) {
        let base = self.doc.base_url.clone();
        if let Some(parent_path) = path.split_last().map(|(_, parent)| parent)
            && let Some(parent) =
                crate::types::find_node_by_path_mut(&mut self.doc.root, parent_path)
            && parent.tag == "picture"
        {
            crate::html::resolve_picture_source(parent, &base, self.viewport_w, self.viewport_h);
            return;
        }
        if let Some(node) = crate::types::find_node_by_path_mut(&mut self.doc.root, path) {
            crate::html::resolve_img_source(node, &base, self.viewport_w, self.viewport_h);
        }
    }

    fn schedule_unscheduled_document_images(&mut self) {
        fn collect(
            node: &crate::types::WebCore,
            base_url: &str,
            viewport_w: f32,
            viewport_h: f32,
            path: &mut Vec<usize>,
            out: &mut Vec<(Vec<usize>, crate::types::PendingImageTarget, String)>,
        ) {
            if node.is_image_element() && node.image_data.is_none() {
                if !node.resolved_src.is_empty() {
                    out.push((
                        path.clone(),
                        crate::types::PendingImageTarget::Element,
                        node.resolved_src.clone(),
                    ));
                } else if let Some(src) = crate::html::image_fallback_source(node) {
                    out.push((
                        path.clone(),
                        crate::types::PendingImageTarget::ElementFallback,
                        crate::html::resolve_url(src, base_url),
                    ));
                } else if let Some(srcset) = crate::html::image_srcset_source(node)
                    && let Some(candidate) = crate::html::parse_srcset_url_for(
                        srcset,
                        node.attributes.get("sizes").map(String::as_str),
                        viewport_w,
                        viewport_h,
                        1.0,
                    )
                {
                    out.push((
                        path.clone(),
                        crate::types::PendingImageTarget::Element,
                        crate::html::resolve_url(&candidate, base_url),
                    ));
                }
            } else if node.tag == "video"
                && node.image_data.is_none()
                && let Some(poster) = node.attributes.get("poster")
            {
                out.push((
                    path.clone(),
                    crate::types::PendingImageTarget::Element,
                    crate::html::resolve_url(poster, base_url),
                ));
            }
            if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
                out.push((
                    path.clone(),
                    crate::types::PendingImageTarget::Background,
                    crate::html::resolve_url(&node.style.background_image_url, base_url),
                ));
            }
            if node.mask_image_data.is_none() && !node.style.rare().mask_image_url.is_empty() {
                out.push((
                    path.clone(),
                    crate::types::PendingImageTarget::Mask,
                    crate::html::resolve_url(&node.style.rare().mask_image_url, base_url),
                ));
            }
            for (idx, child) in node.children.iter().enumerate() {
                path.push(idx);
                collect(child, base_url, viewport_w, viewport_h, path, out);
                path.pop();
            }
        }

        let mut requests = Vec::new();
        collect(
            &self.doc.root,
            &self.doc.base_url,
            self.viewport_w,
            self.viewport_h,
            &mut Vec::new(),
            &mut requests,
        );
        for (path, target, url) in requests {
            self.schedule_streamed_image(path, target, url);
        }
    }

    /// Feed a chunk of HTML to the streaming parser.
    /// Returns resource hints (URLs to fetch in parallel).
    pub fn feed_html_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Vec<(String, crate::html::streaming::ResourceKind)> {
        use crate::html::streaming::DomMutation;

        let parser = self.streaming_parser.get_or_insert_with(|| {
            crate::html::streaming::StreamingParser::new(&self.doc.base_url)
        });
        let mutations = parser.feed(chunk);

        let mut resource_hints = Vec::new();

        for mutation in &mutations {
            match mutation {
                DomMutation::InsertElement {
                    parent_path,
                    tag,
                    attributes,
                } => {
                    if let Some(parent_id) = node_id_at_path(&self.doc.root, parent_path) {
                        let child_id = self.doc.create_element(tag);
                        self.doc.append_child(parent_id, child_id);
                        for (name, value) in attributes {
                            self.doc.set_attribute(child_id, name, value);
                        }
                        let base_url = self.doc.base_url.clone();
                        if let Some(node) = self.doc.find_webcore_mut(child_id) {
                            crate::html::parser::HtmlParser::post_process_node(node, &base_url);
                        }
                        if tag == "img" || tag == "video" {
                            let mut element_path = parent_path.clone();
                            if let Some(child_index) = self
                                .doc
                                .get_node(parent_id)
                                .and_then(|n| n.children.len().checked_sub(1))
                            {
                                element_path.push(child_index);
                                if tag == "img" {
                                    self.resolve_streamed_image_source(&element_path);
                                    self.apply_streamed_image_dimension_hints(&element_path);
                                }
                                self.schedule_streamed_element_image(tag, attributes, element_path);
                            }
                        }
                    }
                }
                DomMutation::AppendText { parent_path, text } => {
                    if !text.is_empty()
                        && let Some(parent_id) = node_id_at_path(&self.doc.root, parent_path)
                    {
                        let child_id = self.doc.create_text_node(text);
                        self.doc.append_child(parent_id, child_id);
                    }
                }
                DomMutation::AddStylesheet { css, .. } => {
                    self.doc
                        .document_stylesheets
                        .push(crate::types::DocumentStylesheet::Inline { css: css.clone() });
                    self.doc
                        .stylesheet
                        .parse_and_add_with_base(&css, &self.doc.base_url);
                    self.doc.stylesheet.rebuild_index();
                    self.mark_style_dirty();
                    self.engine.invalidate_cascade();
                }
                DomMutation::TitleChanged { title } => {
                    self.callbacks.on_title_changed(title);
                }
                DomMutation::ResourceHint { kind, url } => {
                    if matches!(kind, crate::html::streaming::ResourceKind::Stylesheet) {
                        self.doc
                            .linked_stylesheets
                            .push((url.clone(), String::new()));
                        let slot_idx = self.doc.document_stylesheets.len();
                        self.doc.document_stylesheets.push(
                            crate::types::DocumentStylesheet::Linked {
                                href: url.clone(),
                                media: String::new(),
                            },
                        );
                        self.schedule_streamed_stylesheet(slot_idx, url.clone());
                    }
                    resource_hints.push((url.clone(), kind.clone()));
                }
                _ => {}
            }
        }

        if !mutations.is_empty() {
            materialize_streamed_inline_svgs(&mut self.doc.root);
            self.mark_style_dirty();
        }

        resource_hints
    }

    /// Signal that all HTML data has been received.
    pub fn finish_loading(&mut self) {
        if let Some(mut parser) = self.streaming_parser.take() {
            for mutation in parser.finish() {
                match mutation {
                    crate::html::streaming::DomMutation::InsertElement {
                        parent_path,
                        tag,
                        attributes,
                    } => {
                        if let Some(parent_id) = node_id_at_path(&self.doc.root, &parent_path) {
                            let child_id = self.doc.create_element(&tag);
                            self.doc.append_child(parent_id, child_id);
                            for (name, value) in &attributes {
                                self.doc.set_attribute(child_id, &name, &value);
                            }
                            let base_url = self.doc.base_url.clone();
                            if let Some(node) = self.doc.find_webcore_mut(child_id) {
                                crate::html::parser::HtmlParser::post_process_node(node, &base_url);
                            }
                            let mut element_path = parent_path.clone();
                            let child_index =
                                self.doc.get_node(parent_id).map(|n| n.children.len());
                            if let Some(child_index) =
                                child_index.and_then(|len| len.checked_sub(1))
                            {
                                element_path.push(child_index);
                                if tag == "img" || tag == "video" {
                                    if tag == "img" {
                                        self.resolve_streamed_image_source(&element_path);
                                        self.apply_streamed_image_dimension_hints(&element_path);
                                    }
                                    self.schedule_streamed_element_image(
                                        &tag,
                                        &attributes,
                                        element_path,
                                    );
                                }
                            }
                        }
                    }
                    crate::html::streaming::DomMutation::AppendText { parent_path, text } => {
                        if !text.is_empty()
                            && let Some(parent_id) = node_id_at_path(&self.doc.root, &parent_path)
                        {
                            let child_id = self.doc.create_text_node(&text);
                            self.doc.append_child(parent_id, child_id);
                        }
                    }
                    crate::html::streaming::DomMutation::AddStylesheet { css, .. } => {
                        let css_text = css.clone();
                        self.doc
                            .document_stylesheets
                            .push(crate::types::DocumentStylesheet::Inline { css });
                        self.doc
                            .stylesheet
                            .parse_and_add_with_base(&css_text, &self.doc.base_url);
                        self.doc.stylesheet.rebuild_index();
                        self.mark_style_dirty();
                        self.engine.invalidate_cascade();
                    }
                    crate::html::streaming::DomMutation::TitleChanged { title } => {
                        self.callbacks.on_title_changed(&title);
                    }
                    crate::html::streaming::DomMutation::ResourceHint { kind, url } => {
                        if matches!(kind, crate::html::streaming::ResourceKind::Stylesheet) {
                            self.doc
                                .linked_stylesheets
                                .push((url.clone(), String::new()));
                            let slot_idx = self.doc.document_stylesheets.len();
                            self.doc.document_stylesheets.push(
                                crate::types::DocumentStylesheet::Linked {
                                    href: url.clone(),
                                    media: String::new(),
                                },
                            );
                            self.schedule_streamed_stylesheet(slot_idx, url);
                        }
                    }
                    _ => {}
                }
            }
        }
        materialize_streamed_inline_svgs(&mut self.doc.root);
        post_process_streamed_tree(&mut self.doc.root, &self.doc.base_url);
        crate::html::number_lists(&mut self.doc.root);
        self.schedule_unscheduled_document_images();
        self.stylesheet_tx = None;
        self.image_tx = None;
        self.mark_style_dirty();
        self.callbacks.on_load_complete();
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Performance — measure and report rendering pipeline timing
    // ═══════════════════════════════════════════════════════════════════════════

    /// Enable performance tracking. Call before update_frame().
    pub fn enable_perf(&mut self) {
        crate::layout::perf::enable();
    }

    /// Disable performance tracking.
    pub fn disable_perf(&mut self) {
        crate::layout::perf::disable();
    }

    /// Get performance counters from the last update_frame().
    pub fn perf_counters(&self) -> crate::layout::perf::PerfCounters {
        crate::layout::perf::counters()
    }

    /// Print a perf summary to stderr.
    pub fn print_perf(&self) {
        let c = crate::layout::perf::counters();
        if c.layout_calls > 0 || c.layout_ms > 0.0 {
            eprintln!("[perf] {}", c.summary());
        }
    }
}

fn node_id_at_path(root: &crate::types::WebCore, path: &[usize]) -> Option<u32> {
    find_node_by_path(root, path)
        .map(|node| node.node_id)
        .filter(|id| *id != 0)
}

fn find_node_by_path<'a>(
    root: &'a crate::types::WebCore,
    path: &[usize],
) -> Option<&'a crate::types::WebCore> {
    let mut node = root;
    for &idx in path {
        node = node.children.get(idx)?;
    }
    Some(node)
}

fn materialize_streamed_inline_svgs(node: &mut crate::types::WebCore) {
    if node.tag == "svg" && !node.children.is_empty() {
        let source = crate::svg::build_inline_svg_source_from_node(node);
        node.svg_document = crate::svg::parse_svg_document(&source.markup).ok();
        node.svg_viewbox_w = source.viewbox_w;
        node.svg_viewbox_h = source.viewbox_h;
        crate::css::apply_property(
            std::sync::Arc::make_mut(&mut node.style),
            "display",
            "inline-block",
        );
        if let Some(w) = source.explicit_w {
            crate::css::apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "width",
                &format!("{}px", w),
            );
        }
        if let Some(h) = source.explicit_h {
            crate::css::apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "height",
                &format!("{}px", h),
            );
        }
    }
    for child in &mut node.children {
        materialize_streamed_inline_svgs(child);
    }
}

fn post_process_streamed_tree(node: &mut crate::types::WebCore, base_url: &str) {
    for child in &mut node.children {
        post_process_streamed_tree(child, base_url);
    }
    crate::html::parser::HtmlParser::post_process_node(node, base_url);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_frame_keeps_parser_state_across_chunks() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(b"<html><head><style>.a{color:");
        let before = frame.doc.stylesheet.rules.len();
        frame.feed_html_chunk(b"red}</style></head><body>");
        frame.finish_loading();
        assert!(
            frame.doc.stylesheet.rules.len() > before,
            "split inline style should parse after later chunk arrives"
        );
        assert!(
            frame
                .doc
                .document_stylesheets
                .iter()
                .any(|sheet| matches!(sheet, crate::types::DocumentStylesheet::Inline { css } if css.contains(".a"))),
            "streaming should register inline stylesheets on the document as they arrive"
        );
    }

    #[test]
    fn streaming_frame_applies_element_and_text_mutations() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(b"<main><h1>Hello");
        frame.feed_html_chunk(b" stream</h1></main>");
        frame.finish_loading();
        let text = crate::dom::get_text_content(&frame.doc.root);
        assert!(text.contains("Hellostream"), "streamed text was {text:?}");
    }

    #[test]
    fn streaming_frame_adopts_the_document_html_element() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(b"<html><head></head><body><p>Hi</p></body></html>");
        frame.finish_loading();

        assert_eq!(frame.doc.root.tag, "html");
        assert!(
            frame
                .doc
                .root
                .children
                .iter()
                .any(|child| child.tag == "head"),
            "streamed head should be a child of the document root"
        );
        assert!(
            frame
                .doc
                .root
                .children
                .iter()
                .any(|child| child.tag == "body"),
            "streamed body should be a child of the document root"
        );
        assert!(
            !frame
                .doc
                .root
                .children
                .iter()
                .any(|child| child.tag == "html"),
            "streaming must not nest a second html element"
        );
    }

    #[test]
    fn streaming_frame_keeps_void_head_elements_from_swallowing_body() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br#"<html><head><meta charset="utf-8"><link rel="stylesheet" href="/app.css"></head><body><main><h1>Real</h1></main></body></html>"#,
        );
        frame.finish_loading();

        let head = frame
            .doc
            .root
            .children
            .iter()
            .find(|child| child.tag == "head")
            .expect("streamed document should have a head");
        assert!(
            head.children
                .iter()
                .filter(|child| matches!(child.tag.as_str(), "meta" | "link"))
                .all(|child| child.children.is_empty()),
            "void head elements must not become insertion parents"
        );

        let body = frame
            .doc
            .root
            .children
            .iter()
            .find(|child| child.tag == "body")
            .expect("streamed document should have a body");
        assert!(
            body.children.iter().any(|child| child.tag == "main"),
            "visible body content should remain under body"
        );
    }

    #[test]
    fn streaming_frame_schedules_stylesheet_without_host_babysitting() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(br#"<html><head><link rel="stylesheet" href="/app.css"></head>"#);

        assert!(
            frame.doc.pending_stylesheets.is_some(),
            "stylesheet discovery should wire directly into document pending CSS"
        );
        assert!(
            frame
                .scheduled_stylesheets
                .iter()
                .any(|key| key.contains("https://example.test/app.css"))
        );
        assert!(
            frame
                .doc
                .linked_stylesheets
                .iter()
                .any(|(href, _)| href == "https://example.test/app.css"),
            "streaming stylesheet links should be visible through document resource bookkeeping"
        );
    }

    #[test]
    fn streaming_frame_finish_releases_pending_stylesheet_channel() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        let _tx = frame.ensure_stylesheet_sender();
        assert!(frame.doc.pending_stylesheets.is_some());

        frame.finish_loading();
        assert!(
            frame.stylesheet_tx.is_none(),
            "completed streaming frames should not keep a CSS sender alive forever"
        );
        drop(_tx);
        assert!(
            !frame
                .doc
                .poll_pending_stylesheets_budgeted(usize::MAX, std::time::Duration::ZERO)
        );
        assert!(
            frame.doc.pending_stylesheets.is_none(),
            "once the sender is gone and the queue is drained, idle must not see CSS as pending"
        );
    }

    #[test]
    fn update_frame_polls_streamed_stylesheets_in_a_bounded_batch() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..64 {
            let mut sheet = crate::css::Stylesheet::default();
            sheet.parse_and_add_author(&format!(".stream-batch-{i} {{ color: red }}"));
            tx.send((
                i,
                format!("https://example.test/{i}.css"),
                sheet,
                String::new(),
            ))
            .unwrap();
        }
        frame.doc.pending_stylesheets = Some(rx);

        assert!(frame.update_frame());
        let consumed = frame
            .doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector.starts_with(".stream-batch-"))
            .count();
        assert_eq!(
            consumed, 32,
            "active frames should not drain every pending stylesheet fragment in one UI tick"
        );
        assert!(
            frame.doc.pending_stylesheets.is_some(),
            "remaining stylesheet fragments should stay queued for following ticks"
        );
    }

    #[test]
    fn streamed_css_background_images_are_scheduled_after_cascade() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br#"<html><head><style>.logo{background-image:url(/logo.png);width:16px;height:16px}</style></head><body><div class="logo"></div></body></html>"#,
        );
        frame.finish_loading();

        assert!(
            frame.update_frame(),
            "first frame should cascade streamed CSS"
        );
        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("Background")
                    && key.contains("https://example.test/logo.png")),
            "streamed CSS-created background dependencies must enter the image loader"
        );
    }

    #[test]
    fn streamed_finish_runs_full_parser_node_post_processing() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br#"<html><body><input type="submit"><img data-src="hero.png" width="80" height="40"></body></html>"#,
        );
        frame.finish_loading();

        let submit = frame.doc.query_selector("input").unwrap();
        let submit = frame.doc.get_node(submit).unwrap();
        assert_eq!(
            submit.children.first().map(|child| child.text.as_str()),
            Some("Submit"),
            "streamed controls should get the same parser normalization as full parse"
        );

        let img = frame.doc.query_selector("img").unwrap();
        let img = frame.doc.get_node(img).unwrap();
        assert_eq!(img.resolved_src, "https://example.test/hero.png");
        assert_eq!((img.image_width, img.image_height), (80, 40));
    }

    #[test]
    fn streaming_frame_schedules_images_without_host_babysitting() {
        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br#"<html><body><img src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='2' height='2'/%3E"></body>"#,
        );

        assert!(
            frame.doc.pending_images.is_some(),
            "streamed image discovery should wire directly into document pending images"
        );
        assert_eq!(frame.scheduled_images.len(), 1);
    }

    #[test]
    fn streaming_frame_schedules_normalized_srcset_candidate() {
        let mut frame = EngineFrame::empty(800.0, 600.0);
        frame.start_streaming("https://example.test/news/");
        frame.feed_html_chunk(
            br#"<body><img src="small.webp" srcset="small.webp 320w, large.webp 900w" sizes="700px">"#,
        );

        let img = frame.doc.query_selector("img").unwrap();
        let img = frame.doc.get_node(img).unwrap();
        assert_eq!(img.resolved_src, "https://example.test/news/large.webp");
        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("https://example.test/news/large.webp")),
            "streaming image loader should fetch the selected responsive candidate"
        );
        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("https://example.test/news/small.webp")),
            "streaming image loader should also fetch the fallback candidate for progressive paint"
        );
    }

    #[test]
    fn streaming_frame_schedules_lazy_data_src_image() {
        let mut frame = EngineFrame::empty(800.0, 600.0);
        frame.start_streaming("https://example.test/news/");
        frame.feed_html_chunk(
            br#"<body><img loading="lazy" data-src="post.webp" width="320" height="180">"#,
        );

        let img = frame.doc.query_selector("img").unwrap();
        let img = frame.doc.get_node(img).unwrap();
        assert_eq!(img.resolved_src, "https://example.test/news/post.webp");
        assert_eq!((img.image_width, img.image_height), (320, 180));
        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("https://example.test/news/post.webp")),
            "streaming image loader should schedule lazy/deferred image sources"
        );
    }

    #[test]
    fn streaming_frame_schedules_lazy_data_srcset_candidate() {
        let mut frame = EngineFrame::empty(800.0, 600.0);
        frame.start_streaming("https://example.test/news/");
        frame.feed_html_chunk(
            br#"<body><img data-src="tiny.webp" data-srcset="tiny.webp 320w, hero.webp 900w" sizes="700px">"#,
        );

        let img = frame.doc.query_selector("img").unwrap();
        let img = frame.doc.get_node(img).unwrap();
        assert_eq!(img.resolved_src, "https://example.test/news/hero.webp");
        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("https://example.test/news/hero.webp")),
            "lazy responsive image candidates should use the same resolver as srcset"
        );
    }

    #[test]
    fn final_image_sweep_schedules_lazy_srcset_without_src() {
        let mut frame = EngineFrame::empty(800.0, 600.0);
        frame.start_streaming("https://example.test/news/");
        let body = frame.doc.create_element("body");
        frame.doc.append_child(frame.doc.root.node_id, body);
        let img = frame.doc.create_element("img");
        frame.doc.append_child(body, img);
        frame
            .doc
            .set_attribute(img, "data-srcset", "tiny.webp 320w, hero.webp 900w");
        frame.doc.set_attribute(img, "sizes", "700px");

        frame.schedule_unscheduled_document_images();

        assert!(
            frame
                .scheduled_images
                .iter()
                .any(|key| key.contains("https://example.test/news/hero.webp")),
            "final image sweep should not miss lazy responsive images with no src"
        );
    }

    #[test]
    fn streaming_frame_materializes_inline_svg_after_full_subtree_arrives() {
        fn find_svg(node: &crate::types::WebCore) -> Option<&crate::types::WebCore> {
            if node.tag == "svg" {
                return Some(node);
            }
            node.children.iter().find_map(find_svg)
        }

        let mut frame = EngineFrame::empty(320.0, 240.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br##"<html><body><svg width="16" height="16" viewBox="0 0 16 16"><path d="M0 0H16V16H0Z" fill="#6001d2"/></svg>"##,
        );
        frame.finish_loading();

        let svg = find_svg(&frame.doc.root).expect("streamed svg should be in the document");
        assert!(
            svg.svg_document.is_some(),
            "streamed inline SVG should be converted to the same renderable SVG document as the full parser"
        );
        assert_eq!(svg.svg_viewbox_w, 16.0);
        assert_eq!(svg.svg_viewbox_h, 16.0);
    }

    #[test]
    fn streaming_frame_paints_inline_svg_before_final_load() {
        let mut frame = EngineFrame::empty(64.0, 64.0);
        frame.start_streaming("https://example.test/");
        frame.feed_html_chunk(
            br##"<html><body style="margin:0"><svg width="16" height="16" viewBox="0 0 16 16"><path d="M0 0H16V16H0Z" fill="#6001d2"/></svg>"##,
        );

        assert!(
            frame.update_frame(),
            "streamed inline SVG insertion should schedule a paint"
        );
        let list =
            crate::renderer::display_list_builder::build_display_list(&frame.doc.root, 64.0, 64.0);
        let image = list.commands.iter().find_map(|cmd| match cmd {
            crate::renderer::display_list::PaintCmd::Image {
                data: crate::renderer::display_list::ImageRef::Owned(data, w, h),
                ..
            } => Some((data, *w, *h)),
            _ => None,
        });
        let (data, w, h) = image.expect("streamed inline SVG should rasterize to an image command");
        assert_eq!((w, h), (16, 16));
        let idx = ((8 * w + 8) * 4) as usize;
        assert!(
            data[idx] > 70 && data[idx + 1] < 40 && data[idx + 2] > 150 && data[idx + 3] > 200,
            "center pixel should come from streamed inline SVG paint, got rgba({}, {}, {}, {})",
            data[idx],
            data[idx + 1],
            data[idx + 2],
            data[idx + 3]
        );
    }
}
