//! Browser view/widget owned by webcore.
//!
//! Hosts create a view, give it a viewport, ask it to navigate, forward raw
//! input in host coordinates, and paint it into a surface. Loading, progressive
//! resource updates, form navigation, animation frames, scroll presentation, and
//! render caching stay inside webcore.

use std::sync::{Arc, Mutex, mpsc};

use tiny_skia::{Pixmap, Transform};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};

use crate::KeyframeStop;
use crate::dom::HtmlEventType;
use crate::frame::EngineFrame;
use crate::html::resolve_url;
use crate::loading::{PageLoadEvent, PageLoadOptions, spawn_page_load};
use crate::renderer::Renderer;
use crate::types::{
    CSSCursor, DecodedBackgroundImage, Document, FormEvent, FormEventKind, WebCore,
    build_form_submit_url, collect_form_data, find_parent_form_action,
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
    scroll_priority_frame: bool,
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

#[derive(Clone, Copy, Debug, Default)]
pub struct BrowserMemoryStats {
    pub viewport_surface_bytes: usize,
    pub renderer_cached_content_surface_bytes: usize,
    pub renderer_cached_surface_bytes: usize,
    pub tile_surface_bytes: usize,
    pub tile_count: usize,
    pub display_list_commands: usize,
    pub display_list_estimated_bytes: usize,
    pub display_list_inline_bytes: usize,
    pub display_list_heap_bytes: usize,
    pub display_list_text_bytes: usize,
    pub display_list_image_bytes: usize,
    pub display_list_vector_bytes: usize,
    pub raw_resource_cache_entries: usize,
    pub raw_resource_cache_bytes: usize,
    pub parsed_css_cache_entries: usize,
    pub parsed_css_cache_bytes: usize,
    pub decoded_image_cache_entries: usize,
    pub decoded_image_cache_bytes: usize,
    pub dom_nodes: usize,
    pub image_nodes: usize,
    pub dom_estimated_bytes: usize,
    pub layout_estimated_bytes: usize,
    pub style_estimated_bytes: usize,
    pub unique_styles: usize,
    pub line_cache_estimated_bytes: usize,
    pub matched_rules_estimated_bytes: usize,
    pub stylesheet_estimated_bytes: usize,
    pub decoded_dom_image_bytes: usize,
}

fn string_bytes(value: &str) -> usize {
    value.len()
}

fn node_string_bytes(node: &WebCore) -> usize {
    let mut bytes = string_bytes(&node.tag)
        .saturating_add(string_bytes(&node.text))
        .saturating_add(string_bytes(&node.resolved_src));
    if let Some(value) = &node.value_state {
        bytes = bytes.saturating_add(string_bytes(value));
    }
    for (key, value) in node.attributes.iter() {
        bytes = bytes
            .saturating_add(string_bytes(key))
            .saturating_add(string_bytes(value));
    }
    bytes = bytes.saturating_add(
        node.data
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, String)>()),
    );
    for (key, value) in &node.data {
        bytes = bytes
            .saturating_add(string_bytes(key))
            .saturating_add(string_bytes(value));
    }
    bytes
}

fn line_cache_bytes(layout: &crate::types::LayoutBox) -> usize {
    let mut bytes = layout
        .line_cache
        .capacity()
        .saturating_mul(std::mem::size_of::<crate::types::LayoutLine>());
    for line in &layout.line_cache {
        bytes = bytes
            .saturating_add(
                line.visual_segments
                    .capacity()
                    .saturating_mul(std::mem::size_of::<crate::types::VisualSegment>()),
            )
            .saturating_add(
                line.char_x
                    .capacity()
                    .saturating_mul(std::mem::size_of::<f32>()),
            );
    }
    bytes
}

fn layout_bytes(layout: &crate::types::LayoutBox) -> usize {
    std::mem::size_of::<crate::types::LayoutBox>()
        .saturating_add(line_cache_bytes(layout))
        .saturating_add(
            layout
                .inline_runs
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::types::InlineRun>()),
        )
        .saturating_add(
            layout
                .collapsed_border_segments
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::types::CollapsedBorderSegment>()),
        )
}

fn style_string_bytes(style: &crate::types::ComputedStyle) -> usize {
    let mut bytes = 0usize;
    macro_rules! add {
        ($field:ident) => {
            bytes = bytes.saturating_add(string_bytes(&style.$field));
        };
    }
    add!(font_family);
    add!(custom_list_style_type);
    add!(grid_column_start_name);
    add!(grid_column_end_name);
    add!(grid_row_start_name);
    add!(grid_row_end_name);
    add!(grid_area);
    add!(text_overflow_string);
    add!(font_variant_alternates);
    add!(font_variant_caps);
    add!(font_variant_east_asian);
    add!(font_variant_emoji);
    add!(font_variant_ligatures);
    add!(font_variant_numeric);
    add!(font_variant_position);
    add!(before_content);
    add!(after_content);
    add!(marker_content);
    add!(border_image_source);
    add!(border_image_slice);
    add!(border_image_width);
    add!(border_image_outset);
    add!(border_image_repeat);
    add!(background_image_url);
    add!(text_combine_upright);
    add!(container_name);
    add!(list_style_image);
    add!(text_decoration_skip_ink);
    add!(text_emphasis_style);
    add!(text_emphasis_position);
    add!(text_wrap);
    add!(background_blend_mode);
    add!(overflow_anchor);
    add!(overflow_clip_margin);
    add!(anchor_name);
    add!(position_anchor);
    add!(view_transition_name);
    add!(animation_timeline);
    add!(scroll_timeline);
    add!(offset_path);
    add!(scrollbar_width);
    add!(scrollbar_gutter);
    add!(appearance);
    add!(field_sizing);
    add!(interpolate_size);
    add!(margin_trim);
    add!(forced_color_adjust);
    bytes
}

fn style_bytes(style: &crate::types::ComputedStyle) -> usize {
    let mut bytes = std::mem::size_of::<crate::types::ComputedStyle>()
        .saturating_add(style_string_bytes(style))
        .saturating_add(
            style
                .box_shadow
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::types::BoxShadow>()),
        )
        .saturating_add(
            style
                .grid_col_line_names
                .capacity()
                .saturating_mul(std::mem::size_of::<(String, Vec<usize>)>()),
        )
        .saturating_add(
            style
                .grid_row_line_names
                .capacity()
                .saturating_mul(std::mem::size_of::<(String, Vec<usize>)>()),
        );
    for map in [&style.grid_col_line_names, &style.grid_row_line_names] {
        for (key, value) in map {
            bytes = bytes.saturating_add(string_bytes(key)).saturating_add(
                value
                    .capacity()
                    .saturating_mul(std::mem::size_of::<usize>()),
            );
        }
    }
    if let Some(rare) = &style.rare {
        bytes = bytes.saturating_add(std::mem::size_of_val(rare.as_ref()));
    }
    macro_rules! boxed_style {
        ($field:ident) => {
            if let Some(child) = &style.$field {
                bytes = bytes.saturating_add(style_bytes(child));
            }
        };
    }
    boxed_style!(before_style);
    boxed_style!(after_style);
    boxed_style!(selection_style);
    boxed_style!(placeholder_style);
    boxed_style!(marker_style);
    boxed_style!(backdrop_style);
    boxed_style!(file_selector_button_style);
    boxed_style!(hover_style);
    boxed_style!(active_style);
    boxed_style!(visited_style);
    bytes
}

fn matched_rules_bytes(node: &WebCore) -> usize {
    let mut bytes = node
        .matched_rules
        .capacity()
        .saturating_mul(std::mem::size_of::<crate::types::MatchedRule>());
    for rule in &node.matched_rules {
        bytes = bytes
            .saturating_add(string_bytes(&rule.selector))
            .saturating_add(string_bytes(&rule.source))
            .saturating_add(string_bytes(&rule.layer))
            .saturating_add(
                rule.declarations
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(String, String)>()),
            );
        for (name, value) in &rule.declarations {
            bytes = bytes
                .saturating_add(string_bytes(name))
                .saturating_add(string_bytes(value));
        }
    }
    bytes
}

fn stylesheet_bytes(sheet: &crate::css::Stylesheet) -> usize {
    let mut bytes = std::mem::size_of::<crate::css::Stylesheet>()
        .saturating_add(
            sheet
                .rules
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::css::CssRule>()),
        )
        .saturating_add(
            sheet
                .font_faces
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::css::FontFaceDecl>()),
        )
        .saturating_add(
            sheet
                .page_rules
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::css::PageRule>()),
        )
        .saturating_add(
            sheet
                .counter_styles
                .capacity()
                .saturating_mul(std::mem::size_of::<crate::css::CounterStyleRule>()),
        );
    for source in &sheet.raw_sources {
        bytes = bytes.saturating_add(string_bytes(source));
    }
    for (name, value) in &sheet.variables {
        bytes = bytes
            .saturating_add(string_bytes(name))
            .saturating_add(string_bytes(value));
    }
    for (name, stops) in &sheet.keyframes {
        bytes = bytes.saturating_add(string_bytes(name)).saturating_add(
            stops
                .capacity()
                .saturating_mul(std::mem::size_of::<KeyframeStop>()),
        );
    }
    for layer in &sheet.layer_order {
        bytes = bytes.saturating_add(string_bytes(layer));
    }
    for rule in &sheet.rules {
        bytes = bytes
            .saturating_add(string_bytes(&rule.layer))
            .saturating_add(string_bytes(&rule.media_condition))
            .saturating_add(string_bytes(&rule.container_condition))
            .saturating_add(string_bytes(&rule.container_name))
            .saturating_add(string_bytes(&rule.original_selector))
            .saturating_add(
                rule.selectors
                    .capacity()
                    .saturating_mul(std::mem::size_of::<crate::css::CssSelector>()),
            )
            .saturating_add(
                rule.compiled_decls
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(
                        crate::css::properties::PropertyId,
                        crate::types::CssValue,
                    )>()),
            )
            .saturating_add(rule.compiled_important.capacity().saturating_mul(
                std::mem::size_of::<(crate::css::properties::PropertyId, crate::types::CssValue)>(),
            ))
            .saturating_add(
                rule.scopes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<crate::css::ScopeFrame>()),
            );
        for (name, value) in rule
            .declarations
            .iter()
            .chain(rule.important_declarations.iter())
        {
            bytes = bytes
                .saturating_add(string_bytes(name))
                .saturating_add(string_bytes(value));
        }
    }
    bytes
}

fn add_arc_bytes(
    seen: &mut std::collections::HashSet<usize>,
    total: &mut usize,
    data: &Option<Arc<Vec<u8>>>,
) {
    let Some(data) = data.as_ref() else {
        return;
    };
    let ptr = Arc::as_ptr(data) as usize;
    if seen.insert(ptr) {
        *total = total.saturating_add(data.len());
    }
}

fn add_arc_bytes_ref(
    seen: &mut std::collections::HashSet<usize>,
    total: &mut usize,
    data: &Arc<Vec<u8>>,
) {
    let ptr = Arc::as_ptr(data) as usize;
    if seen.insert(ptr) {
        *total = total.saturating_add(data.len());
    }
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
            scroll_priority_frame: false,
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
        self.wire_streamed_font_resources();
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.set_viewport(self.width, self.height);
            let scroll_priority = std::mem::take(&mut self.scroll_priority_frame);
            let update = frame.update_frame_detailed_with_scroll_priority(scroll_priority);
            let mut visual_changed = update.changed;
            if update.paint_only_display_list_rebuild {
                let non_transform_visible = self
                    .renderer
                    .invalidate_non_transform_animation_paint_rects(
                        &frame.doc,
                        self.width,
                        self.height,
                    );
                visual_changed = self.renderer.invalidate_animation_paint_rects(
                    &frame.doc,
                    self.width,
                    self.height,
                ) || non_transform_visible;
                if non_transform_visible {
                    self.renderer.invalidate_paint_only_display_list();
                }
            } else if update.rebuild_display_list {
                self.renderer.invalidate_display_list();
            } else if update.changed {
                visual_changed = self.renderer.invalidate_animation_paint_rects(
                    &frame.doc,
                    self.width,
                    self.height,
                );
            }
            self.stream_needs_layout = false;
            return visual_changed;
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
        if !self.stream_needs_layout {
            return;
        }
        self.wire_streamed_font_resources();
        let Some(frame) = self.stream_frame.as_mut() else {
            return;
        };
        frame.set_viewport(self.width, self.height);
        let scroll_priority = std::mem::take(&mut self.scroll_priority_frame);
        let update = frame.update_frame_detailed_with_scroll_priority(scroll_priority);
        let mut visual_changed = update.changed;
        if update.paint_only_display_list_rebuild {
            let non_transform_visible =
                self.renderer
                    .invalidate_non_transform_animation_paint_rects(
                        &frame.doc,
                        self.width,
                        self.height,
                    );
            visual_changed = self
                .renderer
                .invalidate_animation_paint_rects(&frame.doc, self.width, self.height)
                || non_transform_visible;
            if non_transform_visible {
                self.renderer.invalidate_paint_only_display_list();
            }
        } else if update.rebuild_display_list {
            self.renderer.invalidate_display_list();
        } else if update.changed {
            visual_changed =
                self.renderer
                    .invalidate_animation_paint_rects(&frame.doc, self.width, self.height);
        }
        self.stream_needs_layout = false;
        let _ = visual_changed;
    }

    fn update_streamed_frame_before_paint(&mut self) -> bool {
        if !self.stream_paint_ready {
            return false;
        }
        self.wire_streamed_font_resources();
        let Some(frame) = self.stream_frame.as_mut() else {
            return false;
        };
        frame.set_viewport(self.width, self.height);
        let scroll_priority = std::mem::take(&mut self.scroll_priority_frame);
        let update = frame.update_frame_detailed_with_scroll_priority(scroll_priority);
        let mut visual_changed = update.changed;
        if update.paint_only_display_list_rebuild {
            let non_transform_visible =
                self.renderer
                    .invalidate_non_transform_animation_paint_rects(
                        &frame.doc,
                        self.width,
                        self.height,
                    );
            visual_changed = self
                .renderer
                .invalidate_animation_paint_rects(&frame.doc, self.width, self.height)
                || non_transform_visible;
            if non_transform_visible {
                self.renderer.invalidate_paint_only_display_list();
            }
        } else if update.rebuild_display_list {
            self.renderer.invalidate_display_list();
        } else if update.changed {
            visual_changed =
                self.renderer
                    .invalidate_animation_paint_rects(&frame.doc, self.width, self.height);
        }
        self.stream_needs_layout = false;
        visual_changed
    }

    fn wire_streamed_font_resources(&mut self) {
        let cache_dir = self.options.cache_dir.clone();
        let font_system = &mut self.renderer.font_system as *mut _;
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.engine.font_system = Some(font_system);
            frame.engine.resource_cache_dir = cache_dir.clone();
        }
        self.renderer.layout_engine().resource_cache_dir = cache_dir;
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

    pub fn memory_stats(&self) -> BrowserMemoryStats {
        let renderer = self.renderer.memory_stats();
        let raw = crate::loading::raw_resource_cache_stats();
        let parsed_css = crate::parsed_css_cache_stats();
        let decoded = crate::decoded_image_cache_stats();
        let mut stats = BrowserMemoryStats {
            viewport_surface_bytes: self
                .viewport_pixmap
                .as_ref()
                .map(|surface| surface.data().len())
                .unwrap_or(0),
            renderer_cached_content_surface_bytes: renderer.cached_content_surface_bytes,
            renderer_cached_surface_bytes: renderer.cached_surface_bytes,
            tile_surface_bytes: renderer.tile_surface_bytes,
            tile_count: renderer.tile_count,
            display_list_commands: renderer.display_list_commands,
            display_list_estimated_bytes: renderer.display_list_estimated_bytes,
            display_list_inline_bytes: renderer.display_list_inline_bytes,
            display_list_heap_bytes: renderer.display_list_heap_bytes,
            display_list_text_bytes: renderer.display_list_text_bytes,
            display_list_image_bytes: renderer.display_list_image_bytes,
            display_list_vector_bytes: renderer.display_list_vector_bytes,
            raw_resource_cache_entries: raw.entries,
            raw_resource_cache_bytes: raw.bytes,
            parsed_css_cache_entries: parsed_css.entries,
            parsed_css_cache_bytes: parsed_css.bytes,
            decoded_image_cache_entries: decoded.entries,
            decoded_image_cache_bytes: decoded.bytes,
            ..Default::default()
        };
        if let Some(doc) = self.active_doc() {
            let mut seen = std::collections::HashSet::<usize>::new();
            let mut seen_styles = std::collections::HashSet::<usize>::new();
            stats.stylesheet_estimated_bytes = stylesheet_bytes(&doc.stylesheet);
            Document::walk_all(&doc.root, &mut |node| {
                stats.dom_nodes += 1;
                if node.tag.eq_ignore_ascii_case("img") {
                    stats.image_nodes += 1;
                }
                stats.dom_estimated_bytes = stats
                    .dom_estimated_bytes
                    .saturating_add(std::mem::size_of::<WebCore>())
                    .saturating_add(node_string_bytes(node))
                    .saturating_add(
                        node.children
                            .capacity()
                            .saturating_mul(std::mem::size_of::<WebCore>()),
                    )
                    .saturating_add(
                        node.additional_bg_images
                            .capacity()
                            .saturating_mul(std::mem::size_of::<Option<DecodedBackgroundImage>>()),
                    )
                    .saturating_add(
                        node.svg_animation_overrides
                            .capacity()
                            .saturating_mul(std::mem::size_of::<(Vec<usize>, String, String)>()),
                    )
                    .saturating_add(
                        node.svg_animation_controls
                            .capacity()
                            .saturating_mul(std::mem::size_of::<(Vec<usize>, String, f32)>()),
                    );
                stats.layout_estimated_bytes = stats
                    .layout_estimated_bytes
                    .saturating_add(layout_bytes(&node.layout));
                stats.line_cache_estimated_bytes = stats
                    .line_cache_estimated_bytes
                    .saturating_add(line_cache_bytes(&node.layout));
                stats.matched_rules_estimated_bytes = stats
                    .matched_rules_estimated_bytes
                    .saturating_add(matched_rules_bytes(node));
                let style_ptr = std::sync::Arc::as_ptr(&node.style) as usize;
                if seen_styles.insert(style_ptr) {
                    stats.unique_styles += 1;
                    stats.style_estimated_bytes = stats
                        .style_estimated_bytes
                        .saturating_add(style_bytes(&node.style));
                }
                add_arc_bytes(
                    &mut seen,
                    &mut stats.decoded_dom_image_bytes,
                    &node.image_data,
                );
                if let Some(animated) = node.animated_image.as_ref() {
                    for frame in &animated.frames {
                        add_arc_bytes_ref(
                            &mut seen,
                            &mut stats.decoded_dom_image_bytes,
                            &frame.pixels,
                        );
                    }
                    if let Some(bytes) = animated.source_bytes.as_ref() {
                        add_arc_bytes_ref(&mut seen, &mut stats.decoded_dom_image_bytes, bytes);
                    }
                }
                add_arc_bytes(
                    &mut seen,
                    &mut stats.decoded_dom_image_bytes,
                    &node.bg_image_data,
                );
                add_arc_bytes(
                    &mut seen,
                    &mut stats.decoded_dom_image_bytes,
                    &node.mask_image_data,
                );
                for layer in &node.additional_bg_images {
                    if let Some(layer) = layer.as_ref() {
                        add_arc_bytes_ref(
                            &mut seen,
                            &mut stats.decoded_dom_image_bytes,
                            &layer.data,
                        );
                    }
                }
            });
        }
        stats
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
        let start = std::time::Instant::now();
        let mut html_chunks = 0usize;
        let mut budget_exhausted = false;
        loop {
            if html_chunks >= 8
                || (html_chunks > 0 && start.elapsed() >= std::time::Duration::from_millis(16))
            {
                budget_exhausted = true;
                break;
            }
            let Ok(result) = self.rx.try_recv() else {
                break;
            };
            match result {
                BrowserViewLoadResult::HtmlChunk { load_id, url, html }
                    if load_id == self.load_id =>
                {
                    self.url = url.clone();
                    self.feed_streaming_chunk(&url, &html);
                    self.loading = true;
                    html_chunks += 1;
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
        if budget_exhausted {
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
        let needs_redraw = if self.stream_frame.is_some() {
            let (stream_needs_wake, stream_needs_redraw) = self.stream_idle_state();
            if stream_needs_wake {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    std::time::Instant::now() + std::time::Duration::from_millis(16),
                ));
            } else {
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            stream_layout_changed || stream_needs_redraw
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

    fn stream_idle_state(&self) -> (bool, bool) {
        let Some(frame) = self.stream_frame.as_ref() else {
            return (false, false);
        };
        let pending_resources = frame.doc.pending_images.is_some()
            || frame.doc.pending_stylesheets.is_some()
            || frame.engine.has_pending_fonts();
        let frame_needs_render = frame.needs_render();
        let has_animations = frame.has_animations();
        let needs_wake = self.loading
            || pending_resources
            || frame_needs_render
            || has_animations
            || self.stream_needs_layout;
        let needs_redraw = frame_needs_render || has_animations || self.stream_needs_layout;
        (needs_wake, needs_redraw)
    }

    pub fn paint_into(&mut self, target: &mut Pixmap, x: i32, y: i32, scale: f32) {
        self.wire_streamed_font_resources();
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
            self.scroll_priority_frame = true;
            self.wake();
            return true;
        }
        let redraw = doc.process_mouse_event(HtmlEventType::MouseMove, (x, y + doc.scroll_y), 0);
        let needs_style = doc.hover_changed
            && (doc.hover_sensitive_nodes.contains(&doc.hovered_box)
                || doc.hover_sensitive_nodes.contains(&doc.prev_hovered_box));
        if !needs_style && doc.hover_changed {
            doc.hover_changed = false;
            doc.prev_hovered_box = doc.hovered_box;
        }
        if needs_style {
            self.renderer.invalidate_display_list();
            self.invalidate_backing();
            self.wake();
        } else if redraw {
            self.invalidate_backing();
            self.wake();
        }
        redraw || needs_style
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
            self.scroll_priority_frame = true;
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
            self.scroll_priority_frame = true;
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
            self.scroll_priority_frame = true;
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
    fn browser_view_first_paint_drives_frame_resource_scheduler() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><style>.logo{display:block;width:16px;height:16px;background-image:url(/logo.png)}</style></head><body><div class='logo'></div>",
        );

        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);

        assert!(
            view.stream_frame
                .as_ref()
                .unwrap()
                .doc
                .pending_images
                .is_some(),
            "BrowserView must let EngineFrame discover CSS background dependencies"
        );
    }

    #[test]
    fn browser_view_paint_does_not_poll_streamed_resource_queues_after_layout() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body><p>Ready</p>");

        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(!view.stream_needs_layout);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut sheet = crate::css::Stylesheet::default();
        sheet.parse_and_add_author("p{color:red}");
        tx.send((
            0,
            "https://example.test/late.css".to_string(),
            sheet,
            String::new(),
        ))
        .unwrap();
        view.stream_frame.as_mut().unwrap().doc.pending_stylesheets = Some(rx);

        view.paint_into(&mut target, 0, 0, 1.0);

        assert!(
            view.stream_frame
                .as_ref()
                .unwrap()
                .doc
                .pending_stylesheets
                .is_some(),
            "paint must not drain async resources; BrowserView::drive_idle owns frame updates"
        );
        assert!(
            view.update_streamed_frame_before_paint(),
            "the browser idle/update loop should consume pending resources"
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
    fn browser_view_relayout_is_noop_when_streamed_frame_is_clean() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><style>body{margin:0}p{display:block;height:20px}</style></head><body><p>A</p>",
        );
        assert!(view.update_streamed_frame_before_paint());
        assert!(!view.stream_needs_layout);
        assert!(!view.stream_frame.as_ref().unwrap().doc.style_dirty);

        assert!(
            !view.relayout(),
            "a clean streamed frame should not force a full cascade/layout"
        );
    }

    #[test]
    fn pending_stream_resources_wake_without_forcing_repaint() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body><p>Ready</p>");
        assert!(view.update_streamed_frame_before_paint());
        assert!(!view.stream_needs_layout);

        let (_tx, rx) = std::sync::mpsc::channel();
        view.stream_frame.as_mut().unwrap().doc.pending_stylesheets = Some(rx);

        let (needs_wake, needs_redraw) = view.stream_idle_state();
        assert!(needs_wake, "pending resources should keep the browser loop alive");
        assert!(
            !needs_redraw,
            "queued resources alone should not discard a valid painted frame"
        );
    }

    #[test]
    fn browser_view_poll_budgets_streamed_html_chunks() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/".to_string();
        view.load_id = 42;

        for idx in 0..12 {
            view.tx
                .send(BrowserViewLoadResult::HtmlChunk {
                    load_id: 42,
                    url: base.clone(),
                    html: if idx == 0 {
                        "<!doctype html><body><p>0</p>".to_string()
                    } else {
                        format!("<p>{idx}</p>")
                    },
                })
                .unwrap();
        }

        let first_len = "<!doctype html><body><p>0</p>".len();
        let total_len = first_len
            + (1..12)
                .map(|idx| format!("<p>{idx}</p>").len())
                .sum::<usize>();

        assert!(view.poll());
        assert!(
            view.streamed_html_len < total_len,
            "one UI tick should ingest a bounded batch, not the whole queued document"
        );

        assert!(view.poll());
        assert!(
            view.streamed_html_len > first_len,
            "the next browser tick should continue streaming the remaining chunks"
        );
        while view.streamed_html_len < total_len {
            assert!(view.poll());
        }
        assert_eq!(view.streamed_html_len, total_len);
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
    fn streamed_picture_source_dimensions_override_fallback_image_hint() {
        let mut view = BrowserView::new(800.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/articles/";
        view.stream_frame = Some(EngineFrame::empty(800.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);

        view.feed_streaming_chunk(
            base,
            r#"<html><body><picture><source media="(min-width: 500px)" srcset="wide.svg" width="84" height="29"><img id="badge" src="small.svg" width="25" height="25"></picture>"#,
        );

        let doc = view.document().unwrap();
        let badge_id = doc.get_element_by_id("badge").unwrap();
        let badge = doc.get_box_by_id(badge_id).unwrap();
        assert_eq!(badge.resolved_src, "https://example.test/articles/wide.svg");
        assert_eq!(badge.selected_source_width, Some(84));
        assert_eq!(badge.selected_source_height, Some(29));
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
