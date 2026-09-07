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
        self.doc = crate::load_html_with_registry(
            html,
            base_url,
            self.viewport_w,
            self.viewport_h,
            self.engine.component_registry.clone(),
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
        // 1. Poll for async images/fonts
        if self.doc.poll_pending_images() {
            self.needs_layout = true;
            self.needs_paint = true;
        }
        self.engine.poll_pending_fonts();

        // 2. Check if hover changed (set by process_mouse_event)
        if self.doc.hover_changed {
            self.needs_style = true;
            self.needs_layout = true;
            self.needs_paint = true;
        }

        // 3. Check for running animations
        if self.doc.needs_animation_frame {
            self.needs_style = true;
            self.needs_layout = true;
            self.needs_paint = true;
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
            self.needs_style = false;
            self.needs_layout = false;
            self.needs_paint = true;

            // Notify host of layout completion
            let duration_ms = t0.elapsed().as_secs_f32() * 1000.0;
            self.callbacks.on_layout_complete(duration_ms);

            // Check if scroll height changed — notify host for scrollbar update
            let sh = crate::types::Document::scroll_height(&self.doc.root)
                .max(self.doc.root.layout.margin_rect.h);
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
        let doc_h = crate::types::Document::scroll_height(&self.doc.root)
            .max(self.doc.root.layout.margin_rect.h);
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
            .max(self.doc.root.layout.margin_rect.h)
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
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Streaming — progressive HTML loading for browser mode
    // ═══════════════════════════════════════════════════════════════════════════

    /// Start loading HTML progressively via streaming parser.
    /// Feed chunks with `feed_html_chunk()`, finalize with `finish_loading()`.
    pub fn start_streaming(&mut self, base_url: &str) {
        self.doc = crate::html::parse_html("<html><head></head><body></body></html>");
        self.doc.base_url = base_url.to_string();
        self.needs_style = true;
        self.needs_layout = true;
        self.needs_paint = true;
    }

    /// Feed a chunk of HTML to the streaming parser.
    /// Returns resource hints (URLs to fetch in parallel).
    pub fn feed_html_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Vec<(String, crate::html::streaming::ResourceKind)> {
        use crate::html::streaming::DomMutation;

        let mut parser = crate::html::streaming::StreamingParser::new(&self.doc.base_url);
        let mutations = parser.feed(chunk);

        let mut resource_hints = Vec::new();

        for mutation in &mutations {
            match mutation {
                DomMutation::AddStylesheet { css, .. } => {
                    self.doc.stylesheet.parse_and_add(css);
                    self.mark_style_dirty();
                    self.engine.invalidate_cascade();
                }
                DomMutation::TitleChanged { title } => {
                    self.callbacks.on_title_changed(title);
                }
                DomMutation::ResourceHint { kind, url } => {
                    resource_hints.push((url.clone(), kind.clone()));
                }
                _ => {}
            }
        }

        if !mutations.is_empty() {
            self.mark_style_dirty();
        }

        resource_hints
    }

    /// Signal that all HTML data has been received.
    pub fn finish_loading(&mut self) {
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
