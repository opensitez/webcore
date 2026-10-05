//! Browser view/widget owned by webcore.
//!
//! Hosts create a view, give it a viewport, ask it to navigate, forward raw
//! input in host coordinates, and paint it into a surface. Loading, progressive
//! resource updates, form navigation, animation frames, scroll presentation, and
//! render caching stay inside webcore.

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use tiny_skia::{Pixmap, Transform};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};

use crate::dom::HtmlEventType;
use crate::frame::EngineFrame;
use crate::html::resolve_url;
use crate::loading::{PageLoadEvent, PageLoadOptions, spawn_page_load};
use crate::renderer::Renderer;
use crate::types::{
    CSSCursor, DecodedBackgroundImage, Document, FormEvent, FormEventKind, FormSubmitter, WebCore,
    build_form_submit_url, collect_form_data_with_submitter, encode_form_urlencoded,
    find_form_parent_id, find_parent_form_action, form_owner_id, submitter_form_method,
};

use crate::scheduling::ANIMATION_FRAME_INTERVAL;
const BACKGROUND_MEDIA_INTERVAL: Duration = Duration::from_millis(250);

enum BrowserViewLoadResult {
    HtmlChunk {
        load_id: usize,
        url: String,
        html: String,
    },
    Complete {
        load_id: usize,
        url: String,
        html: String,
    },
}

fn next_frame_deadline(previous: Option<Instant>, now: Instant, interval: Duration) -> Instant {
    let Some(previous) = previous else {
        return now + interval;
    };
    if previous > now && previous <= now + interval {
        return previous;
    }
    if previous > now {
        return now + interval;
    }
    let missed = now.duration_since(previous).as_nanos() / interval.as_nanos();
    previous + interval * (missed.min((u32::MAX - 1) as u128) as u32 + 1)
}

/// Opaque browser-page area. This is the unit a GUI toolkit should embed.
pub struct BrowserView {
    renderer: Renderer,
    doc: Option<crate::browser::BrowserDocument>,
    stream_frame: Option<EngineFrame>,
    history_cache: std::collections::VecDeque<CachedHistoryPage>,
    history_cache_bytes: usize,
    current_history_id: Option<u64>,
    same_document_history: Vec<SameDocumentEntry>,
    fragment_scroll_pending: bool,
    pending_hash_events: Vec<(String, String)>,
    streamed_html_len: usize,
    stream_paint_ready: bool,
    stream_needs_layout: bool,
    stream_layout_committed: bool,
    html_parse_backlog: bool,
    interaction_layout_pending: bool,
    native_caret_reveal_pending: bool,
    url: String,
    title: String,
    loading: bool,
    scroll_priority_frame: bool,
    pointer_position: (f32, f32),
    native_selection_drag: Option<(u32, usize)>,
    next_frame_deadline: Option<Instant>,
    last_stream_frame_update: Option<Instant>,
    width: f32,
    height: f32,
    viewport_pixmap: Option<Pixmap>,
    options: PageLoadOptions,
    load_id: usize,
    document_navigation_pending: bool,
    tx: mpsc::Sender<BrowserViewLoadResult>,
    rx: mpsc::Receiver<BrowserViewLoadResult>,
    deferred_load_result: Option<BrowserViewLoadResult>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    pending_navigate: Arc<Mutex<Option<String>>>,
}

const HISTORY_CACHE_MAX_PAGES: usize = 2;
const HISTORY_CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;

struct CachedHistoryPage {
    id: u64,
    title: String,
    frame: EngineFrame,
    streamed_html_len: usize,
    bytes: usize,
    entries: Vec<SameDocumentEntry>,
}

#[derive(Clone)]
struct SameDocumentEntry {
    id: u64,
    url: String,
    scroll: (f32, f32),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrowserMemoryStats {
    pub history_cache_entries: usize,
    pub history_cache_bytes: usize,
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

fn fallback_title_from_url(url: &str) -> String {
    url.split('/')
        .filter(|part| !part.is_empty())
        .next_back()
        .unwrap_or("Untitled")
        .to_string()
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
    bytes = bytes.saturating_add(style.text_overflow.heap_bytes());
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

fn style_bytes(
    style: &crate::types::ComputedStyle,
    seen_custom_props: &mut std::collections::HashSet<usize>,
) -> usize {
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
    let props_ptr = std::sync::Arc::as_ptr(&style.custom_props) as usize;
    if seen_custom_props.insert(props_ptr) {
        bytes = bytes
            .saturating_add(std::mem::size_of::<usize>() * 2)
            .saturating_add(std::mem::size_of::<std::collections::HashMap<String, String>>())
            .saturating_add(
                style
                    .custom_props
                    .capacity()
                    .saturating_mul(std::mem::size_of::<(String, String)>() + 1),
            );
        for (name, value) in style.custom_props.iter() {
            bytes = bytes
                .saturating_add(string_bytes(name))
                .saturating_add(string_bytes(value));
        }
    }
    if let Some(rare) = &style.rare {
        bytes = bytes.saturating_add(std::mem::size_of_val(rare.as_ref()));
    }
    macro_rules! boxed_style {
        ($field:ident) => {
            if let Some(child) = &style.$field {
                bytes = bytes.saturating_add(style_bytes(child, seen_custom_props));
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
    boxed_style!(details_content_style);
    boxed_style!(spelling_error_style);
    boxed_style!(grammar_error_style);
    boxed_style!(first_line_style);
    boxed_style!(first_letter_style);
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
    for (name, value) in &sheet.variables {
        bytes = bytes
            .saturating_add(string_bytes(name))
            .saturating_add(string_bytes(value));
    }
    bytes = bytes.saturating_add(sheet.keyframes_heap_bytes());
    for layer in &sheet.layer_order {
        bytes = bytes.saturating_add(string_bytes(layer));
    }
    for (layer, condition) in &sheet.layer_declarations {
        bytes = bytes
            .saturating_add(string_bytes(layer))
            .saturating_add(condition.heap_bytes());
    }
    for rule in &sheet.rules {
        bytes = bytes
            .saturating_add(string_bytes(&rule.layer))
            .saturating_add(rule.media_condition.heap_bytes())
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
    pub fn new(width: f32, height: f32, mut options: PageLoadOptions) -> Self {
        crate::css::initialize_ua_stylesheet();
        let (tx, rx) = mpsc::channel();
        ensure_cookie_jar(&mut options);
        Self {
            renderer: Renderer::new(),
            doc: None,
            stream_frame: None,
            history_cache: std::collections::VecDeque::new(),
            history_cache_bytes: 0,
            current_history_id: None,
            same_document_history: Vec::new(),
            fragment_scroll_pending: false,
            pending_hash_events: Vec::new(),
            streamed_html_len: 0,
            stream_paint_ready: false,
            stream_needs_layout: false,
            stream_layout_committed: false,
            html_parse_backlog: false,
            interaction_layout_pending: false,
            native_caret_reveal_pending: false,
            url: String::new(),
            title: String::new(),
            loading: false,
            scroll_priority_frame: false,
            pointer_position: (0.0, 0.0),
            native_selection_drag: None,
            next_frame_deadline: None,
            last_stream_frame_update: None,
            width,
            height,
            viewport_pixmap: None,
            options,
            load_id: 0,
            tx,
            rx,
            deferred_load_result: None,
            wake: None,
            pending_navigate: Arc::new(Mutex::new(None)),
            document_navigation_pending: false,
        }
    }

    pub fn set_wake_callback(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Some(Arc::new(wake));
    }

    /// Attach an existing live DOM without serializing or replacing its nodes.
    pub fn attach_document(&mut self, document: crate::browser::BrowserDocument) {
        self.native_selection_drag = None;
        self.load_id = self.load_id.wrapping_add(1);
        self.stream_frame = None;
        self.history_cache.clear();
        self.history_cache_bytes = 0;
        self.loading = false;
        {
            let doc = document.read();
            self.url = doc.base_url.clone();
            self.title = doc.title.clone();
        }
        self.doc = Some(document);
        self.layout_active();
        self.invalidate_backing();
        self.wake();
    }

    pub fn register_trait_component(
        &mut self,
        tag: &str,
        component: impl crate::types::Component + 'static,
    ) {
        self.renderer.register_trait_component(tag, component);
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.engine.component_registry = self.renderer.component_registry.clone();
        }
    }

    fn wake(&self) {
        if let Some(wake) = self.wake.as_ref() {
            wake();
        }
    }

    fn active_doc(&self) -> Option<crate::browser::DocumentRead<'_>> {
        if let Some(frame) = self.stream_frame.as_ref() {
            Some(crate::browser::DocumentRead::Borrowed(&frame.doc))
        } else {
            self.doc.as_ref().map(|document| document.read())
        }
    }

    fn flush_dirty_active_layout(&mut self) -> bool {
        if self
            .active_doc()
            .is_some_and(|doc| doc.style_dirty || doc.has_dirty_layout())
        {
            self.layout_active()
        } else {
            false
        }
    }

    fn queue_interaction_layout(&mut self) {
        if self
            .active_doc()
            .is_some_and(|doc| doc.style_dirty || doc.hover_changed || doc.has_dirty_layout())
        {
            self.interaction_layout_pending = true;
            if self.stream_frame.is_some() {
                self.stream_needs_layout = true;
            }
        }
    }

    fn active_doc_mut(&mut self) -> Option<crate::browser::DocumentWrite<'_>> {
        if let Some(frame) = self.stream_frame.as_mut() {
            Some(crate::browser::DocumentWrite::Borrowed(&mut frame.doc))
        } else {
            self.doc.as_ref().map(|document| document.write())
        }
    }

    fn feed_streaming_chunk(&mut self, url: &str, html: &str) {
        if self.stream_frame.is_none() {
            let mut frame = EngineFrame::empty(self.width, self.height);
            frame.engine.component_registry = self.renderer.component_registry.clone();
            frame.set_cache_dir(self.options.cache_dir.clone());
            frame.set_resource_wake(self.wake.clone());
            frame.start_streaming(url);
            self.stream_frame = Some(frame);
            self.streamed_html_len = 0;
            self.stream_paint_ready = false;
            self.stream_needs_layout = true;
            self.stream_layout_committed = false;
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
        let _ = base_url;
        let handler = Box::new(move |event: &FormEvent| {
            if let FormEventKind::Submit(action) = &event.kind {
                let _ = action;
            }
        });
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.doc.on_form_event = Some(handler);
        } else if let Some(document) = self.doc.as_ref() {
            let mut doc = document.write();
            doc.on_form_event = Some(handler);
        }
    }

    fn layout_active(&mut self) -> bool {
        self.wire_streamed_font_resources();
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.set_viewport(self.width, self.height);
            let scroll_priority = std::mem::take(&mut self.scroll_priority_frame);
            let update = frame.update_frame_detailed_with_scroll_priority(scroll_priority);
            let resource_visible = self
                .renderer
                .invalidate_resource_paint_rects(&update.resource_paint_rects);
            let mut visual_changed = update.changed || resource_visible;
            if update.paint_only_display_list_rebuild {
                let image_visible = self
                    .renderer
                    .invalidate_paint_rects(update.paint_rects.iter().copied());
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
                ) || resource_visible
                    || non_transform_visible
                    || image_visible;
                if non_transform_visible || image_visible {
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
        let Some(document) = self.doc.as_ref() else {
            return false;
        };
        let mut doc = document.write();
        let engine = self.renderer.layout_engine();
        engine.viewport_h = self.height;
        engine.layout(&mut doc, self.width);
        self.renderer.invalidate_display_list();
        self.interaction_layout_pending = false;
        true
    }

    fn update_streamed_frame_before_paint(&mut self) -> bool {
        if !self.stream_paint_ready {
            return false;
        }
        self.last_stream_frame_update = Some(Instant::now());
        self.wire_streamed_font_resources();
        let Some(frame) = self.stream_frame.as_mut() else {
            return false;
        };
        frame.set_viewport(self.width, self.height);
        let scroll_priority = std::mem::take(&mut self.scroll_priority_frame);
        let update = frame.update_frame_detailed_with_scroll_priority(scroll_priority);
        let resource_visible = self
            .renderer
            .invalidate_resource_paint_rects(&update.resource_paint_rects);
        let mut visual_changed = update.changed || resource_visible;
        if update.paint_only_display_list_rebuild {
            let image_visible = self
                .renderer
                .invalidate_paint_rects(update.paint_rects.iter().copied());
            let non_transform_visible = self
                .renderer
                .invalidate_non_transform_animation_paint_rects(
                    &frame.doc,
                    self.width,
                    self.height,
                );
            visual_changed =
                self.renderer
                    .invalidate_animation_paint_rects(&frame.doc, self.width, self.height)
                    || resource_visible
                    || non_transform_visible
                    || image_visible;
            if non_transform_visible || image_visible {
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
        self.stream_layout_committed = true;
        self.interaction_layout_pending = false;
        visual_changed
    }

    fn ensure_streamed_layout_current(&mut self) -> bool {
        let needs_update = self.stream_paint_ready
            && self
                .stream_frame
                .as_ref()
                .is_some_and(|frame| self.stream_needs_layout || frame.needs_render());
        if needs_update {
            self.update_streamed_frame_before_paint()
        } else {
            self.wire_streamed_font_resources();
            false
        }
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
        let mut options = options;
        if options.cookie_jar.is_none() {
            options.cookie_jar = self.options.cookie_jar.clone();
        }
        ensure_cookie_jar(&mut options);
        self.options = options;
    }

    pub fn document(&self) -> Option<crate::browser::DocumentRead<'_>> {
        self.active_doc()
    }

    pub fn document_mut(&mut self) -> Option<crate::browser::DocumentWrite<'_>> {
        self.active_doc_mut()
    }

    pub fn set_external_video_overlay(&mut self, id: u32, enabled: bool) -> bool {
        let Some((mut doc, renderer)) = self.document_and_renderer_current_mut() else {
            return false;
        };
        let Some(node) = doc.find_webcore_mut(id) else {
            return false;
        };
        if node.tag != "video" || node.external_video_overlay == enabled {
            return false;
        }
        node.external_video_overlay = enabled;
        renderer.invalidate_display_list();
        true
    }

    pub fn document_and_renderer_mut(
        &mut self,
    ) -> Option<(crate::browser::DocumentWrite<'_>, &mut Renderer)> {
        self.ensure_streamed_layout_current();
        self.document_and_renderer_current_mut()
    }

    /// Access the already-updated page after `drive_idle`, without starting another frame update.
    pub fn document_and_renderer_current_mut(
        &mut self,
    ) -> Option<(crate::browser::DocumentWrite<'_>, &mut Renderer)> {
        if let Some(frame) = self.stream_frame.as_mut() {
            return Some((
                crate::browser::DocumentWrite::Borrowed(&mut frame.doc),
                &mut self.renderer,
            ));
        }
        let document = self.doc.as_ref()?;
        Some((document.write(), &mut self.renderer))
    }

    pub fn memory_stats(&self) -> BrowserMemoryStats {
        let renderer = self.renderer.memory_stats();
        let raw = crate::loading::raw_resource_cache_stats();
        let parsed_css = crate::parsed_css_cache_stats();
        let decoded = crate::decoded_image_cache_stats();
        let mut stats = BrowserMemoryStats {
            history_cache_entries: self.history_cache.len(),
            history_cache_bytes: self.history_cache_bytes,
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
            let mut seen_custom_props = std::collections::HashSet::<usize>::new();
            stats.stylesheet_estimated_bytes = stylesheet_bytes(&doc.stylesheet);
            for cached in doc.inline_stylesheet_cache.values() {
                stats.stylesheet_estimated_bytes = stats
                    .stylesheet_estimated_bytes
                    .saturating_add(stylesheet_bytes(&cached.sheet))
                    .saturating_add(cached.source.len())
                    .saturating_add(cached.base_url.capacity());
            }
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
                        .saturating_add(style_bytes(&node.style, &mut seen_custom_props));
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
                if let Some(masks) = node.mask_images.as_ref() {
                    for mask in masks.first.iter().chain(masks.additional.iter().flatten()) {
                        add_arc_bytes_ref(
                            &mut seen,
                            &mut stats.decoded_dom_image_bytes,
                            &mask.data,
                        );
                    }
                }
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
            let mut doc = self.doc.as_ref().map(|document| document.write());
            self.renderer.handle_window_event(event, doc.as_deref_mut());
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
        let Some(mut doc) = self.active_doc_mut() else {
            return false;
        };
        doc.style_dirty = true;
        doc.stylesheet.inspect_mode = on;
        drop(doc);
        self.layout_active();
        self.invalidate_backing();
        self.wake();
        true
    }

    pub fn relayout(&mut self) -> bool {
        if self
            .stream_frame
            .as_ref()
            .is_some_and(|frame| !self.stream_needs_layout && !frame.needs_render())
        {
            return false;
        }
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
        let Some(document) = self.doc.as_ref() else {
            return None;
        };
        let mut doc = document.write();
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
        engine.layout(&mut doc, width);
        let full_ms = t0.elapsed().as_micros() as f64 / 1000.0;

        mark_dirty(&mut doc.root);
        let t1 = std::time::Instant::now();
        let _more = engine.layout_above_fold(&mut doc, width);
        let above_ms = t1.elapsed().as_micros() as f64 / 1000.0;
        engine.layout_remainder(&mut doc, width);
        drop(doc);
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

    pub(crate) fn viewport_size(&self) -> (f32, f32) {
        (self.width, self.height)
    }

    pub fn navigate(&mut self, url: String) {
        self.document_navigation_pending = true;
        if self.navigate_fragment(&url, false) {
            return;
        }
        self.navigate_with_options(url, self.options.clone());
    }

    fn remember_current_history_entry(&mut self) {
        let Some(id) = self.current_history_id else {
            return;
        };
        let scroll = self
            .active_doc()
            .map(|doc| (doc.scroll_x, doc.scroll_y))
            .unwrap_or_default();
        let entry = SameDocumentEntry {
            id,
            url: self.url.clone(),
            scroll,
        };
        if let Some(old) = self
            .same_document_history
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            *old = entry;
        } else {
            self.same_document_history.push(entry);
        }
    }

    fn navigate_fragment(&mut self, url: &str, traversal: bool) -> bool {
        let (Ok(mut current), Ok(mut next)) =
            (reqwest::Url::parse(&self.url), reqwest::Url::parse(url))
        else {
            return false;
        };
        if !traversal && next.fragment().is_none() {
            return false;
        }
        let hash_changed = current.fragment() != next.fragment();
        current.set_fragment(None);
        next.set_fragment(None);
        if current != next || self.active_doc().is_none() {
            return false;
        }
        self.remember_current_history_entry();
        if hash_changed {
            self.pending_hash_events
                .push((self.url.clone(), url.to_string()));
        }
        self.url = url.to_string();
        if let Some(mut doc) = self.active_doc_mut() {
            doc.navigation_url = Some(url.to_string());
            doc.style_dirty = true;
        }
        self.fragment_scroll_pending = true;
        self.queue_interaction_layout();
        self.wake();
        true
    }

    fn apply_pending_fragment_scroll(&mut self) {
        if !self.fragment_scroll_pending {
            return;
        }
        let loading = self.loading;
        let Some(mut doc) = self.active_doc_mut() else {
            return;
        };
        let fragment = crate::dom::url::parse(doc.document_uri(), None)
            .map(|url| crate::dom::url::decode_fragment(&url.fragment))
            .unwrap_or_default();
        let target = doc.fragment_target_id();
        let found = target != 0 || fragment.is_empty() || fragment.eq_ignore_ascii_case("top");
        if target != 0 {
            doc.scroll_into_view(target);
        } else if found {
            doc.scroll_x = 0.0;
            doc.scroll_y = 0.0;
        }
        let max_y = (doc.cached_scroll_height() - doc.viewport_h).max(0.0);
        doc.scroll_y = doc.scroll_y.min(max_y);
        drop(doc);
        if found || !loading {
            self.fragment_scroll_pending = false;
        }
    }

    fn dispatch_pending_hash_events(&mut self) -> bool {
        let events = std::mem::take(&mut self.pending_hash_events);
        if events.is_empty() {
            return false;
        }
        if let Some(mut doc) = self.active_doc_mut() {
            for (old, new) in events {
                let mut event = crate::dom::events::DomEvent::new("hashchange", doc.root.node_id);
                event.old_url = old;
                event.new_url = new;
                doc.dispatch_dom_event(&mut event);
            }
        }
        self.queue_interaction_layout();
        true
    }

    /// Navigate to a history entry. Restoring a cached entry preserves its DOM,
    /// computed styles, layout, form state, and scroll position.
    pub fn navigate_history(&mut self, url: String, entry_id: u64, restore: bool) {
        self.document_navigation_pending = false;
        let entry = self
            .same_document_history
            .iter()
            .find(|entry| entry.id == entry_id && entry.url == url)
            .cloned();
        if restore && entry.is_some() && self.navigate_fragment(&url, true) {
            self.current_history_id = Some(entry_id);
            if let Some(mut doc) = self.active_doc_mut() {
                let scroll = entry.unwrap().scroll;
                doc.scroll_x = scroll.0;
                doc.scroll_y = scroll.1;
            }
            self.fragment_scroll_pending = false;
            return;
        }
        if !restore
            && self.current_history_id != Some(entry_id)
            && self.navigate_fragment(&url, false)
        {
            self.current_history_id = Some(entry_id);
            return;
        }
        self.cache_current_history_page();
        if restore {
            if let Some(index) = self.history_cache.iter().position(|page| {
                page.id == entry_id || page.entries.iter().any(|entry| entry.id == entry_id)
            }) {
                let page = self
                    .history_cache
                    .remove(index)
                    .expect("cached history entry");
                self.history_cache_bytes = self.history_cache_bytes.saturating_sub(page.bytes);
                self.load_id = self.load_id.wrapping_add(1);
                self.pending_hash_events.clear();
                let scroll = page
                    .entries
                    .iter()
                    .find(|entry| entry.id == entry_id)
                    .map(|entry| entry.scroll);
                self.url = url.clone();
                self.title = page.title;
                self.streamed_html_len = page.streamed_html_len;
                self.stream_frame = Some(page.frame);
                if let Some(frame) = self.stream_frame.as_mut() {
                    frame.resume_media_playback();
                }
                self.doc = None;
                self.current_history_id = Some(entry_id);
                self.same_document_history = page.entries;
                if let Some(frame) = self.stream_frame.as_mut() {
                    frame.doc.navigation_url = Some(url);
                    frame.doc.style_dirty = true;
                    if let Some(scroll) = scroll {
                        frame.doc.scroll_x = scroll.0;
                        frame.doc.scroll_y = scroll.1;
                    }
                }
                self.loading = false;
                self.stream_paint_ready = true;
                self.stream_needs_layout = true;
                self.stream_layout_committed = true;
                self.html_parse_backlog = false;
                self.interaction_layout_pending = true;
                self.native_caret_reveal_pending = false;
                self.deferred_load_result = None;
                self.renderer.invalidate_display_list();
                self.invalidate_backing();
                self.wake();
                return;
            }
        }
        self.history_cache.retain(|page| {
            page.id != entry_id && !page.entries.iter().any(|entry| entry.id == entry_id)
        });
        self.history_cache_bytes = self.history_cache.iter().map(|page| page.bytes).sum();
        self.navigate_with_options(url, self.options.clone());
        self.current_history_id = Some(entry_id);
    }

    /// Associate a navigation initiated by the document, such as a form submit,
    /// with the history entry assigned by the embedding browser.
    pub fn set_history_entry(&mut self, entry_id: u64) {
        self.current_history_id = Some(entry_id);
    }

    pub(crate) fn take_document_navigation(&mut self) -> bool {
        std::mem::take(&mut self.document_navigation_pending)
    }

    fn cache_current_history_page(&mut self) {
        self.remember_current_history_entry();
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.stop_media_playback();
        }
        let Some(id) = self.current_history_id.take() else {
            return;
        };
        if self.loading || !self.stream_layout_committed || self.stream_frame.is_none() {
            return;
        }
        let stats = self.memory_stats();
        let bytes = stats
            .dom_estimated_bytes
            .saturating_add(stats.layout_estimated_bytes)
            .saturating_add(stats.style_estimated_bytes)
            .saturating_add(stats.line_cache_estimated_bytes)
            .saturating_add(stats.stylesheet_estimated_bytes)
            .saturating_add(stats.decoded_dom_image_bytes)
            .saturating_add(
                self.same_document_history.capacity() * std::mem::size_of::<SameDocumentEntry>(),
            )
            .saturating_add(
                self.same_document_history
                    .iter()
                    .map(|entry| entry.url.capacity())
                    .sum::<usize>(),
            );
        if bytes > HISTORY_CACHE_MAX_BYTES {
            return;
        }
        self.history_cache.retain(|page| page.id != id);
        self.history_cache_bytes = self.history_cache.iter().map(|page| page.bytes).sum();
        let page = CachedHistoryPage {
            id,
            title: self.title.clone(),
            frame: self.stream_frame.take().expect("completed history frame"),
            streamed_html_len: self.streamed_html_len,
            bytes,
            entries: std::mem::take(&mut self.same_document_history),
        };
        self.history_cache_bytes = self.history_cache_bytes.saturating_add(bytes);
        self.history_cache.push_back(page);
        while self.history_cache.len() > HISTORY_CACHE_MAX_PAGES
            || self.history_cache_bytes > HISTORY_CACHE_MAX_BYTES
        {
            if let Some(oldest) = self.history_cache.pop_front() {
                self.history_cache_bytes = self.history_cache_bytes.saturating_sub(oldest.bytes);
            }
        }
    }

    fn navigate_with_options(&mut self, url: String, options: PageLoadOptions) {
        self.cache_current_history_page();
        self.same_document_history.clear();
        self.fragment_scroll_pending = reqwest::Url::parse(&url)
            .ok()
            .is_some_and(|url| url.fragment().is_some());
        self.pending_hash_events.clear();
        self.url = url.clone();
        self.title = "Loading...".to_string();
        self.loading = true;
        self.doc = None;
        self.stream_frame = Some(EngineFrame::empty(self.width, self.height));
        if let Some(frame) = self.stream_frame.as_mut() {
            frame.engine.component_registry = self.renderer.component_registry.clone();
            frame.set_cache_dir(self.options.cache_dir.clone());
            frame.set_resource_wake(self.wake.clone());
            frame.start_streaming(&url);
        }
        self.streamed_html_len = 0;
        self.stream_paint_ready = false;
        self.stream_needs_layout = true;
        self.stream_layout_committed = false;
        self.html_parse_backlog = false;
        self.interaction_layout_pending = false;
        self.native_caret_reveal_pending = false;
        self.deferred_load_result = None;
        self.invalidate_backing();
        self.load_id = self.load_id.wrapping_add(1);
        let load_id = self.load_id;
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        let loader_wake = wake.clone();
        let mut load_options = options;
        load_options.emit_preview = true;
        spawn_page_load(url, load_options, move |event| match event {
            PageLoadEvent::Chunk { url, html } => {
                let _ = tx.send(BrowserViewLoadResult::HtmlChunk { load_id, url, html });
                if let Some(wake) = loader_wake.as_ref() {
                    wake();
                }
            }
            PageLoadEvent::Complete { url, html } => {
                let _ = tx.send(BrowserViewLoadResult::Complete { load_id, url, html });
                if let Some(wake) = loader_wake.as_ref() {
                    wake();
                }
            }
        });
        if let Some(wake) = wake.as_ref() {
            wake();
        }
    }

    fn navigate_form_submission(
        &mut self,
        target: String,
        method: &str,
        data: Vec<(String, String)>,
    ) {
        self.document_navigation_pending = true;
        let (url, options) = self.form_submission_request(target, method, data);
        self.navigate_with_options(url, options);
    }

    fn form_submission_request(
        &self,
        target: String,
        method: &str,
        data: Vec<(String, String)>,
    ) -> (String, PageLoadOptions) {
        if method.eq_ignore_ascii_case("post") {
            let mut options = self.options.clone();
            options.request_method = "POST".to_string();
            options.request_body = Some(encode_form_urlencoded(&data).into_bytes());
            options.request_content_type = Some("application/x-www-form-urlencoded".to_string());
            (target, options)
        } else {
            let url = build_form_submit_url(&target, method, &data);
            (url, self.options.clone())
        }
    }

    pub fn load_until_ready(&mut self, url: String, timeout: std::time::Duration) -> bool {
        self.navigate(url);
        let deadline = std::time::Instant::now() + timeout;
        let mut changed = false;
        loop {
            changed |= self.poll();
            if self.active_doc().is_some() && !self.loading {
                self.stream_paint_ready = true;
                self.stream_needs_layout = true;
                self.ensure_streamed_layout_current();
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
        let mut completed = None::<(String, String)>;
        let start = std::time::Instant::now();
        let mut html_chunks = 0usize;
        let mut budget_exhausted = false;
        loop {
            let result = self
                .deferred_load_result
                .take()
                .or_else(|| self.rx.try_recv().ok());
            let Some(result) = result else {
                break;
            };
            if html_chunks > 0 && start.elapsed() >= std::time::Duration::from_millis(16) {
                self.deferred_load_result = Some(result);
                budget_exhausted = true;
                break;
            }
            match result {
                BrowserViewLoadResult::HtmlChunk { load_id, url, html }
                    if load_id == self.load_id =>
                {
                    if self
                        .active_doc()
                        .is_none_or(|doc| doc.navigation_url.is_none())
                    {
                        self.url = url.clone();
                    }
                    self.feed_streaming_chunk(&url, &html);
                    self.loading = true;
                    html_chunks += 1;
                    changed = true;
                }
                BrowserViewLoadResult::Complete { load_id, url, html }
                    if load_id == self.load_id =>
                {
                    completed = Some((url, html));
                }
                _ => {}
            }
        }
        self.html_parse_backlog = budget_exhausted;
        if let Some((url, _html)) = completed {
            if self
                .active_doc()
                .is_none_or(|doc| doc.navigation_url.is_none())
            {
                self.url = url.clone();
            }
            if let Some(frame) = self.stream_frame.as_mut() {
                frame.finish_loading();
                self.stream_paint_ready = true;
                self.stream_needs_layout = true;
            }
            let title = self.active_doc().map(|doc| doc.title.clone());
            self.title = title
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| fallback_title_from_url(&url));
            self.loading = false;
            self.html_parse_backlog = false;
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
        let hash_changed = self.dispatch_pending_hash_events();
        if let Some(document) = self.doc.as_ref() {
            self.title = document.read().title.clone();
        }
        if let Some(document) = self.doc.as_ref() {
            if document.take_resource_changes() {
                crate::restart_async_image_fetches(&mut document.write());
            }
        }
        let scroll_priority = self.scroll_priority_frame;
        let changed = if scroll_priority { false } else { self.poll() };
        let nav_changed = self.drain_pending_navigation();
        let deferred_update =
            !scroll_priority && !self.defer_queued_html_layout() && !self.stream_frame_update_due();
        let stream_layout_changed =
            if scroll_priority || self.defer_queued_html_layout() || deferred_update {
                false
            } else {
                self.update_streamed_frame_before_paint()
            };
        let needs_redraw = if self.stream_frame.is_some() {
            let (stream_needs_wake, stream_needs_redraw) = self.stream_idle_state();
            if scroll_priority {
                self.next_frame_deadline = None;
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(1),
                ));
            } else if stream_needs_wake || deferred_update {
                let deadline = next_frame_deadline(
                    self.next_frame_deadline,
                    Instant::now(),
                    self.stream_frame_interval(),
                );
                self.next_frame_deadline = Some(deadline);
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            } else {
                self.next_frame_deadline = None;
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            scroll_priority || stream_layout_changed || (stream_needs_redraw && !deferred_update)
        } else {
            let mut doc = self.doc.as_ref().map(|document| document.write());
            self.renderer.drive_document_idle(
                event_loop,
                doc.as_deref_mut(),
                self.width,
                self.height,
            )
        };
        if needs_redraw {
            self.invalidate_backing();
        }
        hash_changed || changed || nav_changed || stream_layout_changed || needs_redraw
    }

    #[cfg(test)]
    fn drive_idle_for_test(&mut self) -> bool {
        let hash_changed = self.dispatch_pending_hash_events();
        let scroll_priority = self.scroll_priority_frame;
        let changed = if scroll_priority { false } else { self.poll() };
        let nav_changed = self.drain_pending_navigation();
        let deferred_update =
            !scroll_priority && !self.defer_queued_html_layout() && !self.stream_frame_update_due();
        let stream_layout_changed =
            if scroll_priority || self.defer_queued_html_layout() || deferred_update {
                false
            } else {
                self.update_streamed_frame_before_paint()
            };
        let needs_redraw = if self.stream_frame.is_some() {
            let (_, stream_needs_redraw) = self.stream_idle_state();
            scroll_priority || stream_layout_changed || (stream_needs_redraw && !deferred_update)
        } else {
            false
        };
        if needs_redraw {
            self.invalidate_backing();
        }
        hash_changed || changed || nav_changed || stream_layout_changed || needs_redraw
    }

    fn stream_idle_state(&self) -> (bool, bool) {
        let Some(frame) = self.stream_frame.as_ref() else {
            return (false, false);
        };
        let pending_resources = frame.doc.pending_images.is_some()
            || frame.doc.pending_stylesheets.is_some()
            || frame.engine.has_pending_fonts();
        let frame_needs_work = frame.needs_render();
        let frame_needs_redraw = frame.needs_visible_render();
        let has_animations = frame.has_animations();
        let needs_wake = self.loading
            || pending_resources
            || frame_needs_work
            || has_animations
            || self.stream_needs_layout;
        let needs_redraw =
            !self.defer_queued_html_layout() && (frame_needs_redraw || self.stream_needs_layout);
        (needs_wake, needs_redraw)
    }

    fn stream_frame_update_due(&self) -> bool {
        let Some(frame) = self.stream_frame.as_ref() else {
            return true;
        };
        self.stream_needs_layout
            || self.interaction_layout_pending
            || frame.needs_immediate_update()
            || self.last_stream_frame_update.is_none_or(|last| {
                Instant::now().saturating_duration_since(last) >= self.stream_frame_interval()
            })
    }

    fn stream_frame_interval(&self) -> Duration {
        let Some(frame) = self.stream_frame.as_ref() else {
            return ANIMATION_FRAME_INTERVAL;
        };
        if self.loading
            || frame.doc.pending_images.is_some()
            || frame.doc.pending_stylesheets.is_some()
            || frame.engine.has_pending_fonts()
            || !frame.media_only_idle()
        {
            ANIMATION_FRAME_INTERVAL
        } else {
            BACKGROUND_MEDIA_INTERVAL
        }
    }

    fn defer_queued_html_layout(&self) -> bool {
        self.html_parse_backlog && self.stream_layout_committed && !self.interaction_layout_pending
    }

    pub fn paint_into(&mut self, target: &mut Pixmap, x: i32, y: i32, scale: f32) {
        if self.stream_frame.is_none() && self.interaction_layout_pending {
            self.layout_active();
        }
        if let Some(frame) = self.stream_frame.as_mut() {
            if frame.set_device_pixel_ratio(scale) {
                self.wake();
            }
        }
        // Painting must not synchronously drain network/resources, but it does
        // have to consume already-queued interaction style work. Otherwise a
        // hover/focus change can set stream_needs_layout and then render the
        // stale display list forever until an unrelated resource tick happens.
        if self.stream_needs_layout
            && !self.scroll_priority_frame
            && !self.defer_queued_html_layout()
        {
            self.ensure_streamed_layout_current();
        }
        self.apply_pending_fragment_scroll();
        if std::mem::take(&mut self.native_caret_reveal_pending) {
            if let Some(frame) = self.stream_frame.as_mut() {
                self.renderer.reveal_native_control_caret(&mut frame.doc);
            } else if let Some(document) = self.doc.as_ref() {
                self.renderer
                    .reveal_native_control_caret(&mut document.write());
            }
        }
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
            } else if let Some(document) = self.doc.as_ref() {
                self.renderer.render(&mut document.write(), target, scale);
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
                } else if let Some(document) = self.doc.as_ref() {
                    self.renderer.render(&mut document.write(), viewport, scale);
                    blit_viewport_from_backing(viewport, target, x, y, 0, view_w, view_h);
                } else {
                    fill_placeholder(target, x, y, view_w, view_h);
                }
            } else {
                fill_placeholder(target, x, y, view_w, view_h);
            }
        }
        crate::profile::finish_scroll_paint();
        self.scroll_priority_frame = false;
    }

    pub fn handle_mouse_move(&mut self, x: f32, y: f32) -> bool {
        self.pointer_position = (x, y);
        let width = self.width;
        let height = self.height;
        let (mut redraw, needs_style) = {
            let Some(mut doc) = self.active_doc_mut() else {
                return false;
            };
            let old_scroll_y = doc.scroll_y;
            if doc.process_scrollbar_event(HtmlEventType::MouseMove, x, y, width, height) {
                let scrolled = (doc.scroll_y - old_scroll_y).abs() >= 0.5;
                drop(doc);
                if scrolled {
                    crate::profile::mark_scroll_input();
                    self.scroll_priority_frame = true;
                }
                self.flush_dirty_active_layout();
                self.invalidate_backing();
                self.wake();
                return true;
            }
            let doc_pt = (x, y + doc.scroll_y);
            let mut redraw = doc.process_mouse_event(HtmlEventType::MouseMove, doc_pt, 0);
            redraw |= doc.process_mouse_event(HtmlEventType::PointerMove, doc_pt, 0);
            let needs_style = doc.hover_changed
                && crate::css::hover_change_requires_style(
                    &doc.root,
                    &doc.stylesheet,
                    doc.prev_hovered_box,
                    doc.hovered_box,
                    &doc.hover_sensitive_nodes,
                );
            if !needs_style && doc.hover_changed {
                doc.hover_changed = false;
                doc.prev_hovered_box = doc.hovered_box;
            }
            (redraw, needs_style)
        };
        if let Some((id, anchor)) = self.native_selection_drag
            && let Some((mut doc, renderer)) = self.document_and_renderer_current_mut()
            && doc.focused_box == id
        {
            redraw |= renderer.place_native_control_caret(&mut doc, (x, y), Some(anchor));
        }
        if needs_style {
            self.queue_interaction_layout();
            self.invalidate_backing();
            self.wake();
        } else if redraw {
            self.queue_interaction_layout();
            self.invalidate_backing();
            self.wake();
        }
        redraw || needs_style
    }

    pub fn handle_mouse_button(&mut self, kind: HtmlEventType, x: f32, y: f32, button: u8) -> bool {
        self.handle_mouse_button_with_modifiers(kind, x, y, button, false, false, false, false)
    }

    pub fn handle_mouse_button_with_modifiers(
        &mut self,
        kind: HtmlEventType,
        x: f32,
        y: f32,
        button: u8,
        ctrl: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> bool {
        self.pointer_position = (x, y);
        let width = self.width;
        let height = self.height;
        let drag_anchor = self.native_selection_drag;
        let Some(mut doc) = self.active_doc_mut() else {
            return false;
        };
        let old_scroll_y = doc.scroll_y;
        let selection_release = matches!(kind, HtmlEventType::MouseUp)
            && doc.editor.mouse_down
            && doc.editor.has_selection();
        let selection_anchor = drag_anchor.or_else(|| {
            shift
                .then(|| {
                    doc.get_node(doc.focused_box)
                        .map(|node| (doc.focused_box, node.input_sel_anchor))
                })
                .flatten()
        });
        if doc.process_scrollbar_event(kind, x, y, width, height) {
            let scrolled = (doc.scroll_y - old_scroll_y).abs() >= 0.5;
            drop(doc);
            if scrolled {
                crate::profile::mark_scroll_input();
                self.scroll_priority_frame = true;
            }
            self.flush_dirty_active_layout();
            self.invalidate_backing();
            self.wake();
            return true;
        }
        let doc_pt = (x, y + doc.scroll_y);
        let popup_input = doc.open_select != 0 || doc.open_picker != 0;
        let mut changed =
            doc.process_mouse_event_with_modifiers(kind, doc_pt, button, ctrl, shift, alt, meta);
        let pointer_kind = match kind {
            HtmlEventType::MouseDown => Some(HtmlEventType::PointerDown),
            HtmlEventType::MouseUp => Some(HtmlEventType::PointerUp),
            _ => None,
        };
        if let Some(pointer_kind) = pointer_kind
            && !popup_input
        {
            changed |= doc.process_mouse_event_with_modifiers(
                pointer_kind,
                doc_pt,
                button,
                ctrl,
                shift,
                alt,
                meta,
            );
        }
        if !popup_input && button == 2 && matches!(kind, HtmlEventType::MouseUp) {
            changed |= doc.process_mouse_event(HtmlEventType::ContextMenu, doc_pt, button);
        }
        let activation = if matches!(kind, HtmlEventType::MouseUp) {
            doc.pointer_activation_target.take()
        } else {
            None
        };
        drop(doc);
        let mut next_drag = None;
        if !popup_input
            && button == 0
            && matches!(kind, HtmlEventType::MouseDown | HtmlEventType::MouseUp)
            && let Some((mut doc, renderer)) = self.document_and_renderer_current_mut()
        {
            let anchor = selection_anchor
                .filter(|(id, _)| *id == doc.focused_box)
                .map(|(_, anchor)| anchor);
            let placed = renderer.place_native_control_caret(&mut doc, (x, y), anchor);
            changed |= placed;
            next_drag = placed.then(|| {
                let id = doc.focused_box;
                (id, doc.get_node(id).unwrap().input_sel_anchor)
            });
        }
        if button == 0 && matches!(kind, HtmlEventType::MouseDown) {
            self.native_selection_drag = next_drag;
        }
        if button == 0 && matches!(kind, HtmlEventType::MouseUp) {
            self.native_selection_drag = None;
        }
        if button == 0
            && !popup_input
            && !selection_release
            && matches!(kind, HtmlEventType::MouseUp)
            && activation.is_some()
        {
            self.handle_activation_target(activation.unwrap(), Some((x, y)));
        }
        if changed {
            self.queue_interaction_layout();
            self.invalidate_backing();
            self.wake();
        }
        changed
    }

    pub fn handle_wheel(&mut self, dx: f32, dy: f32) -> bool {
        let pointer = self.pointer_position;
        let Some(mut doc) = self.active_doc_mut() else {
            return false;
        };
        let doc_point = (pointer.0 + doc.scroll_x, pointer.1 + doc.scroll_y);
        let mut wheel = crate::dom::HtmlEvent::new(HtmlEventType::Wheel);
        wheel.client_pos = pointer;
        wheel.doc_pos = doc_point;
        wheel.delta_x = dx;
        wheel.delta_y = dy;
        wheel.target = doc.hovered_box;
        let popup_wheel = doc
            .select_popup()
            .is_some_and(|popup| popup.contains(doc_point));
        let (mut changed, wheel) = if popup_wheel {
            (false, wheel)
        } else {
            doc.dispatch_input_event(wheel)
        };
        let old = (doc.scroll_x, doc.scroll_y);
        let scrolled = !wheel.default_prevented && doc.process_wheel_event_xy(doc_point, -dx, -dy);
        let inner_scroll = !popup_wheel && scrolled && (doc.scroll_x, doc.scroll_y) == old;
        drop(doc);
        if scrolled {
            crate::profile::mark_scroll_input();
            self.scroll_priority_frame = true;
            self.wake();
            changed = true;
        }
        if inner_scroll {
            // Inner scroll offsets are recorded in paint commands, unlike the
            // viewport offset. Re-record, retaining unchanged paint segments.
            self.renderer.invalidate_display_list();
        }
        if changed {
            if self
                .active_doc()
                .is_some_and(|doc| doc.style_dirty || doc.has_dirty_layout())
            {
                self.stream_needs_layout = true;
            }
            self.invalidate_backing();
        } else {
            return false;
        }
        true
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        self.handle_wheel(dx, dy)
    }

    pub fn scroll_to(&mut self, x: f32, y: f32) -> bool {
        let height = self.height;
        let Some(mut doc) = self.active_doc_mut() else {
            return false;
        };
        let max_y = (doc.cached_scroll_height() - height).max(0.0);
        let old = (doc.scroll_x, doc.scroll_y);
        doc.scroll_x = x.max(0.0);
        doc.scroll_y = y.clamp(0.0, max_y);
        let changed = (doc.scroll_y - old.1).abs() >= 0.5 || (doc.scroll_x - old.0).abs() >= 0.5;
        drop(doc);
        if changed {
            crate::profile::mark_scroll_input();
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
        let Some((handled, evt)) = self.active_doc_mut().map(|mut doc| {
            doc.dispatch_keyboard_input(event_type, key_code, ch, ctrl, shift, alt, meta)
        }) else {
            return false;
        };
        if !evt.default_prevented && matches!(event_type, HtmlEventType::KeyDown) {
            if key_code == 9 {
                let moved = self.active_doc_mut().is_some_and(|mut doc| {
                    doc.open_picker = 0;
                    doc.picker_calendar = None;
                    doc.picker_time = None;
                    if shift {
                        doc.focus_prev()
                    } else {
                        doc.focus_next()
                    }
                });
                if moved || handled {
                    self.queue_interaction_layout();
                    self.invalidate_backing();
                    self.wake();
                }
                return moved || handled;
            }
            if key_code == 13 && self.submit_focused_text_control() {
                return true;
            }
        }
        let changed =
            self.document_and_renderer_current_mut()
                .is_some_and(|(mut doc, renderer)| {
                    if !matches!(key_code, 38 | 40) {
                        renderer.reset_native_vertical_goal();
                    }
                    let initial = crate::ComputedStyle::INITIAL_FONT_SIZE_PX;
                    let root_font = doc.root.style.font_size_px(initial, initial);
                    doc.process_keyboard_defaults_with_native_navigation(
                        evt,
                        handled,
                        &mut |node, key, extend| {
                            renderer.move_native_control_vertically(node, root_font, key, extend)
                        },
                    )
                });
        let activation = self
            .active_doc_mut()
            .and_then(|mut doc| doc.keyboard_activation_target.take());
        if let Some(target) = activation {
            self.handle_activation_target(target, None);
        }
        if changed {
            self.queue_interaction_layout();
            self.native_caret_reveal_pending = true;
            self.invalidate_backing();
            self.wake();
        }
        changed
    }

    pub fn cursor_at(&self, x: f32, y: f32) -> CSSCursor {
        if let Some(doc) = self.active_doc() {
            let resize_axes = doc
                .resize_drag
                .as_ref()
                .map(|drag| drag.axes)
                .or_else(|| doc.resize_grip_at(x, y).map(|drag| drag.axes));
            if let Some(axes) = resize_axes {
                return match axes {
                    (true, true) => CSSCursor::SEResize,
                    (true, false) => CSSCursor::EResize,
                    (false, true) => CSSCursor::SResize,
                    (false, false) => CSSCursor::Auto,
                };
            }
        }
        self.active_doc()
            .and_then(|doc| doc.pointer_hit((x + doc.scroll_x, y + doc.scroll_y), 0))
            .and_then(|hit| {
                self.active_doc()?
                    .get_box_by_id(hit.node_id)
                    .map(|node| node.style.cursor)
            })
            .unwrap_or(CSSCursor::Auto)
    }

    fn handle_activation_target(&mut self, target_id: u32, point: Option<(f32, f32)>) {
        let view_url = self.url.clone();
        let pending_navigate = self.pending_navigate.clone();
        let Some(mut doc) = self.active_doc_mut() else {
            return;
        };
        let current_url = if doc.base_url.is_empty() {
            view_url.clone()
        } else {
            doc.base_url.clone()
        };
        let mut ancestor = target_id;
        let mut href = None;
        while ancestor != 0 {
            let Some(node) = doc.get_node(ancestor) else {
                break;
            };
            if matches!(node.tag.as_str(), "a" | "area") {
                href = node.attributes.get("href").cloned();
                break;
            }
            ancestor = doc.parent_node(ancestor);
        }
        if let Some(href) = href {
            doc.visited_urls.insert(href.clone());
            drop(doc);
            self.navigate(resolve_browser_target(&href, &current_url, &view_url));
            return;
        }
        {
            let control_id = find_form_parent_id(&doc.root, target_id);
            if doc.is_actually_disabled(control_id) || doc.is_inert(control_id) {
                return;
            }
            let Some(node) = doc.get_box_by_id(control_id) else {
                return;
            };
            if matches!(node.tag.as_str(), "button" | "input") {
                let input_type = node
                    .attributes
                    .get("type")
                    .map(|s| s.trim().to_ascii_lowercase())
                    .unwrap_or_else(|| {
                        if node.tag == "button" {
                            "submit".to_string()
                        } else {
                            "text".to_string()
                        }
                    });
                let is_submit = if node.tag == "button" {
                    !matches!(input_type.as_str(), "button" | "reset")
                } else {
                    matches!(input_type.as_str(), "submit" | "image")
                };
                if is_submit {
                    let Some(form_id) = form_owner_id(&doc.root, control_id) else {
                        return;
                    };
                    if !validate_user_form_submission(&mut doc, form_id, control_id) {
                        *pending_navigate.lock().unwrap() = None;
                        drop(doc);
                        self.queue_interaction_layout();
                        self.native_caret_reveal_pending = true;
                        self.wake();
                        return;
                    }
                    let mut submit_event = crate::dom::events::DomEvent::new("submit", form_id);
                    doc.dispatch_dom_event(&mut submit_event);
                    if submit_event.default_prevented() {
                        *pending_navigate.lock().unwrap() = None;
                        return;
                    }
                    let action = find_parent_form_action(&doc.root, control_id);
                    let method = submitter_form_method(&doc.root, control_id);
                    if method == "dialog" {
                        doc.submit_dialog_form(form_id, control_id);
                        return;
                    }
                    let target = if action.is_empty() {
                        current_url.clone()
                    } else {
                        resolve_browser_target(&action, &current_url, &view_url)
                    };
                    let rect = doc.get_box_by_id(control_id).unwrap().layout.content_rect;
                    let image_coordinates = point
                        .map(|(x, y)| {
                            (
                                (x + doc.scroll_x - rect.x).max(0.0).floor() as u32,
                                (y + doc.scroll_y - rect.y).max(0.0).floor() as u32,
                            )
                        })
                        .unwrap_or((0, 0));
                    let data = collect_form_data_with_submitter(
                        &doc.root,
                        form_id,
                        Some(FormSubmitter {
                            node_id: control_id,
                            image_coordinates,
                        }),
                    );
                    *pending_navigate.lock().unwrap() = None;
                    drop(doc);
                    self.navigate_form_submission(target, &method, data);
                }
            }
        }
    }

    fn submit_focused_text_control(&mut self) -> bool {
        let view_url = self.url.clone();
        let pending_navigate = self.pending_navigate.clone();
        let submit = {
            let Some(mut doc) = self.active_doc_mut() else {
                return false;
            };
            let focused = doc.focused_box;
            if doc.open_picker != 0 || doc.open_select != 0 {
                return false;
            }
            if focused == 0 {
                return false;
            }
            let Some(node) = doc.get_box_by_id(focused) else {
                return false;
            };
            if node.tag != "input" {
                return false;
            }
            if doc.is_actually_disabled(focused) || doc.is_inert(focused) {
                return false;
            }
            let input_type = doc.input_type(focused);
            if !matches!(
                input_type.as_str(),
                "text"
                    | "password"
                    | "email"
                    | "search"
                    | "url"
                    | "tel"
                    | "number"
                    | "date"
                    | "month"
                    | "week"
                    | "time"
                    | "datetime-local"
            ) {
                return false;
            }
            let Some(form_id) = doc.form_owner(focused) else {
                return false;
            };
            let Some(target) = doc.implicit_submission_target(form_id) else {
                return true;
            };
            let submitter = (target != form_id).then_some(target);
            if let Some(submitter) = submitter {
                let mut click = crate::dom::events::DomEvent::new("click", submitter);
                click.is_trusted = true;
                doc.dispatch_dom_event(&mut click);
                if click.default_prevented()
                    || doc.is_actually_disabled(submitter)
                    || doc.is_inert(submitter)
                    || doc.form_owner(submitter) != Some(form_id)
                {
                    *pending_navigate.lock().unwrap() = None;
                    return true;
                }
            }
            if !validate_user_form_submission(&mut doc, form_id, submitter.unwrap_or(0)) {
                *pending_navigate.lock().unwrap() = None;
                drop(doc);
                self.queue_interaction_layout();
                self.native_caret_reveal_pending = true;
                self.wake();
                return true;
            }
            let mut submit_event = crate::dom::events::DomEvent::new("submit", form_id);
            doc.dispatch_dom_event(&mut submit_event);
            if submit_event.default_prevented() {
                *pending_navigate.lock().unwrap() = None;
                return true;
            }
            let action = find_parent_form_action(&doc.root, target);
            let method = submitter_form_method(&doc.root, target);
            if method == "dialog" {
                doc.submit_dialog_form(form_id, target);
                return true;
            }
            let data = collect_form_data_with_submitter(
                &doc.root,
                form_id,
                submitter.map(|node_id| FormSubmitter {
                    node_id,
                    image_coordinates: (0, 0),
                }),
            );
            let target = if action.is_empty() {
                view_url.clone()
            } else {
                let base_url = if doc.base_url.is_empty() {
                    view_url.as_str()
                } else {
                    doc.base_url.as_str()
                };
                resolve_browser_target(&action, base_url, &view_url)
            };
            (target, method, data)
        };
        *self.pending_navigate.lock().unwrap() = None;
        let (target, method, data) = submit;
        self.navigate_form_submission(target, &method, data);
        true
    }

    fn invalidate_backing(&mut self) {
        // BrowserView does not keep a second full viewport cache. Renderer owns
        // the retained surface/display-list cache so scroll, fixed overlays,
        // and scrollbar chrome stay in one presentation model.
    }
}

fn validate_user_form_submission(doc: &mut Document, form_id: u32, submitter: u32) -> bool {
    let skip = doc
        .get_box_by_id(form_id)
        .is_some_and(|form| form.attributes.contains_key("novalidate"))
        || doc.get_box_by_id(submitter).is_some_and(|control| {
            let is_submitter = control.tag == "button"
                || control.tag == "input"
                    && control.attributes.get("type").is_some_and(|kind| {
                        matches!(
                            kind.trim().to_ascii_lowercase().as_str(),
                            "submit" | "image"
                        )
                    });
            is_submitter && control.attributes.contains_key("formnovalidate")
        });
    skip || doc.report_validity(form_id)
}

fn ensure_cookie_jar(options: &mut PageLoadOptions) {
    if options.cookie_jar.is_none() {
        options.cookie_jar = Some(Arc::new(Mutex::new(crate::loading::CookieJar::default())));
    }
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
    let base_url = crate::dom::url::parse(base, None);
    crate::dom::url::parse(raw, base_url.as_ref())
        .map(|url| url.href())
        .unwrap_or_else(|| resolve_url(raw, base))
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
    if src_x == 0 && dst_x == 0 && bytes == backing_stride && bytes == target_stride {
        let src_off = src_start_y * backing_stride;
        let dst_off = dst_start_y * target_stride;
        let len = copy_h as usize * bytes;
        dst[dst_off..dst_off + len].copy_from_slice(&src[src_off..src_off + len]);
        return;
    }
    for row in 0..copy_h as usize {
        let src_off = (src_start_y + row) * backing_stride + src_x * 4;
        let dst_off = (dst_start_y + row) * target_stride + dst_x * 4;
        dst[dst_off..dst_off + bytes].copy_from_slice(&src[src_off..src_off + bytes]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::collect_form_data_for_form;

    #[test]
    fn textarea_enter_reveals_caret_without_scrolling_the_page() {
        let doc = crate::parse_html(
            "<body style='margin:0'><textarea id=t style='width:250px;height:60px;font:20px/30px sans-serif'>a</textarea></body>",
        );
        let handle = crate::browser::BrowserDocument::new(doc);
        let mut view = BrowserView::new(400.0, 240.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        let id = handle.read().get_element_by_id("t").unwrap();
        let rect = handle.read().get_node(id).unwrap().layout.content_rect;
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(kind, rect.x + 1.0, rect.y + 15.0, 0);
        }
        for _ in 0..6 {
            assert!(view.handle_key(HtmlEventType::KeyDown, 13, None, false, false, false, false));
        }
        assert!(view.native_caret_reveal_pending);
        let mut target = Pixmap::new(400, 240).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        let doc = handle.read();
        let node = doc.get_node(id).unwrap();
        assert!(node.layout.scroll_top > 0.0);
        assert!(
            (node.layout.scroll_top - (node.layout.scroll_height - node.layout.content_rect.h))
                .abs()
                < 1.0,
            "the trailing caret line must fit the content viewport: {:?}",
            node.layout
        );
        assert_eq!(doc.scroll_y, 0.0);
    }

    #[test]
    fn native_text_pointer_places_caret_on_clicked_line_and_rtl_edge() {
        for scale in [1.0, 2.0] {
            let doc = crate::parse_html(
                "<body style='margin:0'><textarea id=t style='display:block;width:250px;height:100px;font:20px/30px sans-serif'>abc\ndef</textarea><input id=r dir=rtl value='مرحبا' style='display:block;width:250px;height:40px;font:20px/30px sans-serif'></body>",
            );
            let handle = crate::browser::BrowserDocument::new(doc);
            let mut view = BrowserView::new(400.0, 240.0, PageLoadOptions::default());
            view.attach_document(handle.clone());
            let mut pixels = Pixmap::new((400.0 * scale) as u32, (240.0 * scale) as u32).unwrap();
            view.paint_into(&mut pixels, 0, 0, scale);
            for (name, index) in [("t", 4), ("r", 0)] {
                let id = handle.read().get_element_by_id(name).unwrap();
                let rect = handle.read().get_node(id).unwrap().layout.content_rect;
                let point = if name == "t" {
                    (rect.x + 1.0, rect.y + 45.0)
                } else {
                    (rect.right() - 1.0, rect.y + rect.h / 2.0)
                };
                for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                    view.handle_mouse_button(kind, point.0, point.1, 0);
                }
                let doc = handle.read();
                assert_eq!(
                    doc.get_node(id).unwrap().input_cursor,
                    index,
                    "{name} at {scale}x"
                );
            }
        }
    }

    #[test]
    fn time_picker_enter_commits_without_submitting_its_form() {
        let handle = crate::browser::BrowserDocument::new(crate::parse_html(
            "<form action='https://example.com/submitted'><input id=p type=time value=12:30><button>Submit</button></form>",
        ));
        let mut view = BrowserView::new(400.0, 240.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        let id = handle.read().get_element_by_id("p").unwrap();
        let rect = handle.read().get_node(id).unwrap().layout.border_rect;
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(kind, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 0);
        }
        assert_eq!(handle.read().open_picker, id);
        view.handle_key(HtmlEventType::KeyDown, 38, None, false, false, false, false);
        view.handle_key(HtmlEventType::KeyDown, 13, None, false, false, false, false);
        assert_eq!(handle.read().value(id), "13:30");
        assert_eq!(handle.read().open_picker, 0);
        assert!(!view.url().contains("submitted"));
        assert!(!view.is_loading());
    }

    #[test]
    fn temporal_caret_hit_uses_the_painted_label_inset_at_each_scale() {
        for scale in [1.0, 1.5, 2.0] {
            for (kind, value) in [
                ("date", "2026-10-05"),
                ("month", "2026-10"),
                ("week", "2026-W41"),
                ("time", "12:30"),
                ("datetime-local", "2026-10-05T12:30"),
            ] {
                let handle = crate::browser::BrowserDocument::new(crate::parse_html(&format!(
                    "<input id=p type={kind} value='{value}' style='font:10px/20px monospace;width:240px'>"
                )));
                let mut view = BrowserView::new(400.0, 240.0, PageLoadOptions::default());
                view.attach_document(handle.clone());
                let mut pixels =
                    Pixmap::new((400.0 * scale) as u32, (240.0 * scale) as u32).unwrap();
                view.paint_into(&mut pixels, 0, 0, scale);
                let id = handle.read().get_element_by_id("p").unwrap();
                let rect = handle.read().get_node(id).unwrap().layout.content_rect;
                // The visible first character starts four CSS pixels inside
                // the temporal label, not at the unadorned content edge.
                view.handle_mouse_button(
                    HtmlEventType::MouseDown,
                    rect.x + crate::widgets::DateField::TEXT_INSET_PX,
                    rect.y + rect.h / 2.0,
                    0,
                );
                assert_eq!(
                    handle.read().get_node(id).unwrap().input_cursor,
                    0,
                    "{kind} at {scale}x"
                );
            }
        }
    }

    #[test]
    fn native_picker_surface_does_not_activate_a_link_underneath() {
        let doc = crate::parse_html(
            "<body style='margin:0'><input id=p type=month value=2026-08><a href=about:blank style='display:block;height:250px'>Under the picker</a></body>",
        );
        let handle = crate::browser::BrowserDocument::new(doc);
        let mut view = BrowserView::new(400.0, 400.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        let id = handle.read().get_element_by_id("p").unwrap();
        let rect = handle.read().get_node(id).unwrap().layout.border_rect;
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(kind, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0, 0);
        }
        let (x, y, w, _) = handle.read().picker_rect(id).unwrap();
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(
                kind,
                x + w - crate::widgets::Calendar::CELL / 2.0,
                y + crate::widgets::Calendar::CELL / 2.0,
                0,
            );
        }
        assert_eq!(handle.read().open_picker, id);
        assert_eq!(handle.read().focused_box, id);
        assert_eq!(handle.read().picker_month(id).0, 2027);
        assert_eq!(view.url(), "");
        assert!(!view.loading);
    }

    #[test]
    fn native_listbox_pointer_preserves_modifiers_after_scroll() {
        let doc = crate::parse_html(
            "<body style='margin:0'><select id=s multiple size=3 style='width:150px'><option id=a>A</option><option id=b>B</option><option id=c>C</option><option id=d>D</option><option id=e>E</option><option id=f>F</option></select></body>",
        );
        let handle = crate::browser::BrowserDocument::new(doc);
        let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        let id = handle.read().get_element_by_id("s").unwrap();
        let rect = handle.read().get_node(id).unwrap().layout.content_rect;
        let row = crate::html::forms::list_box_row_height(
            handle
                .read()
                .get_node(id)
                .unwrap()
                .style
                .font_size_px(16.0, 16.0),
        );
        let mut click = |index: usize, shift: bool, meta: bool, offset: f32| {
            let y =
                rect.y + crate::html::forms::LIST_BOX_PADDING + row * (index as f32 + 0.5) - offset;
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button_with_modifiers(
                    kind,
                    rect.x + 5.0,
                    y,
                    0,
                    false,
                    shift,
                    false,
                    meta,
                );
            }
        };
        click(0, false, false, 0.0);
        click(2, true, false, 0.0);
        assert_eq!(handle.read().query_selector_all("option:checked").len(), 3);
        click(1, false, true, 0.0);
        assert_eq!(handle.read().query_selector_all("option:checked").len(), 2);
        drop(click);
        handle
            .write()
            .process_wheel_event((rect.x + 5.0, rect.y + 5.0), -row * 3.0);
        let offset = handle.read().get_node(id).unwrap().layout.scroll_top;
        assert!(offset > 0.0);
        let y = rect.y + crate::html::forms::LIST_BOX_PADDING + row * 4.5 - offset;
        assert_eq!(
            handle
                .read()
                .pointer_hit((rect.x + 5.0, y), 0)
                .map(|hit| hit.node_id),
            Some(id),
            "scrolled control must retain its pointer target; rect={rect:?}, row={row}, offset={offset}, y={y}"
        );
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(kind, rect.x + 5.0, y, 0);
        }
        let doc = handle.read();
        assert_eq!(
            doc.query_selector_all("option:checked"),
            [doc.get_element_by_id("e").unwrap()],
            "scrolled row click: target={}, focus={}, drag={}, scroll={}, rect={:?}",
            doc.mousedown_target,
            doc.focused_box,
            doc.drag_active,
            doc.get_node(id).unwrap().layout.scroll_top,
            doc.get_node(id).unwrap().layout.content_rect,
        );
    }

    #[test]
    fn attached_document_keeps_nodes_listeners_and_host_mutations() {
        let mut doc = crate::parse_html(
            "<body style='margin:0'><button id='button'>Click</button><div id='status'>Before</div></body>",
        );
        let button = doc.get_element_by_id("button").unwrap();
        let status = doc.get_element_by_id("status").unwrap();
        let clicks = Arc::new(Mutex::new(0));
        let observed = clicks.clone();
        doc.add_event_listener(
            button,
            "click",
            Box::new(move |_, _| {
                *observed.lock().unwrap() += 1;
            }),
            crate::dom::events::ListenerOptions::default(),
        );
        let handle = crate::browser::BrowserDocument::new(doc);
        let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        let rect = handle.read().get_node(button).unwrap().layout.border_rect;
        let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        view.handle_mouse_button(HtmlEventType::MouseDown, x, y, 0);
        view.handle_mouse_button(HtmlEventType::MouseUp, x, y, 0);
        assert_eq!(*clicks.lock().unwrap(), 1);
        handle.write().set_text_content(status, "After");
        view.relayout();
        assert_eq!(view.document().unwrap().text_content(status), "After");
        assert_eq!(
            view.document().unwrap().get_element_by_id("button"),
            Some(button)
        );
        let mut target = Pixmap::new(320, 200).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(target.data().chunks_exact(4).any(|pixel| pixel[3] != 0));
    }

    #[test]
    fn keyboard_control_edits_coalesce_layout_until_paint() {
        for tag in ["input", "textarea"] {
            let doc = crate::parse_html(&format!(
                "<body><{tag} id=entry style='width:100px;height:40px'></{tag}></body>"
            ));
            let entry = doc.get_element_by_id("entry").unwrap();
            let handle = crate::browser::BrowserDocument::new(doc);
            let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
            view.attach_document(handle.clone());
            handle.write().focus(entry);
            view.relayout();
            let generation = handle.read().layout_generation;
            for ch in "abcdef".chars() {
                view.handle_key(
                    HtmlEventType::KeyDown,
                    ch as u32,
                    Some(ch),
                    false,
                    false,
                    false,
                    false,
                );
                assert_eq!(
                    handle.read().layout_generation,
                    generation,
                    "{tag}: input must not force layout"
                );
            }
            assert_eq!(
                crate::types::input_value(handle.read().get_node(entry).unwrap()),
                "abcdef"
            );
            assert!(view.native_caret_reveal_pending);
            let mut pixels = Pixmap::new(320, 200).unwrap();
            view.paint_into(&mut pixels, 0, 0, 1.0);
            assert!(!view.native_caret_reveal_pending);
            assert!(!view.interaction_layout_pending);
        }
    }

    #[test]
    fn attached_document_keyboard_events_edit_once_and_keep_modifiers() {
        let mut doc = crate::parse_html("<body><input id='entry' value=''></body>");
        let entry = doc.get_element_by_id("entry").unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        for kind in ["keydown", "keypress", "keyup"] {
            let events = observed.clone();
            doc.add_event_listener(
                entry,
                kind,
                Box::new(move |event, _| {
                    events.lock().unwrap().push((
                        event.event_type.clone(),
                        event.key.clone(),
                        event.shift_key,
                    ));
                }),
                crate::dom::events::ListenerOptions::default(),
            );
        }
        let handle = crate::browser::BrowserDocument::new(doc);
        let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
        view.attach_document(handle.clone());
        handle.write().focus(entry);
        for event_type in [
            HtmlEventType::KeyDown,
            HtmlEventType::KeyPress,
            HtmlEventType::KeyUp,
        ] {
            view.handle_key(event_type, 65, Some('A'), false, true, false, false);
        }
        let doc = handle.read();
        assert_eq!(crate::types::input_value(doc.get_node(entry).unwrap()), "A");
        assert_eq!(
            *observed.lock().unwrap(),
            vec![
                ("keydown".into(), "A".into(), true),
                ("keypress".into(), "A".into(), true),
                ("keyup".into(), "A".into(), true),
            ],
        );
    }

    #[test]
    fn viewport_bulk_copy_matches_clipped_row_copy() {
        let mut source = Pixmap::new(8, 9).unwrap();
        for (index, pixel) in source.data_mut().chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[index as u8, index as u8 * 3, index as u8 * 2, 255]);
        }
        for (width, x, y, source_y) in [(8, 0, 2, 1), (8, 0, -2, 1), (10, 2, 1, 2), (8, -2, 0, 0)] {
            let mut expected = Pixmap::new(width, 12).unwrap();
            expected.fill(tiny_skia::Color::from_rgba8(17, 19, 23, 255));
            let mut actual = expected.clone();
            for row in 0..8i32 {
                for column in 0..8i32 {
                    let (dst_x, dst_y) = (x + column, y + row);
                    let src_y = source_y + row;
                    if dst_x >= 0 && dst_x < width as i32 && dst_y >= 0 && dst_y < 12 && src_y < 9 {
                        let src = (src_y as usize * 8 + column as usize) * 4;
                        let dst = (dst_y as usize * width as usize + dst_x as usize) * 4;
                        expected.data_mut()[dst..dst + 4]
                            .copy_from_slice(&source.data()[src..src + 4]);
                    }
                }
            }
            blit_viewport_from_backing(&source, &mut actual, x, y, source_y as u32, 8, 8);
            assert_eq!(
                actual.data(),
                expected.data(),
                "width={width} offset=({x},{y})"
            );
        }
    }

    #[test]
    fn history_restores_page_state_without_reloading() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let first_url = "about:history-first";
        view.url = first_url.into();
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame
            .as_mut()
            .unwrap()
            .start_streaming(first_url);
        view.feed_streaming_chunk(first_url, "<body><p>First page</p></body>");
        view.stream_frame.as_mut().unwrap().finish_loading();
        view.update_streamed_frame_before_paint();
        view.loading = false;
        view.current_history_id = Some(1);
        view.stream_frame.as_mut().unwrap().doc.scroll_y = 42.0;

        view.navigate_history("about:history-second".into(), 2, false);
        assert_eq!(view.history_cache.len(), 1);
        view.navigate_history(first_url.into(), 1, true);

        assert_eq!(view.url, first_url);
        assert!(!view.loading);
        assert_eq!(view.stream_frame.as_ref().unwrap().doc.scroll_y, 42.0);
        assert!(
            view.stream_frame
                .as_ref()
                .unwrap()
                .doc
                .root
                .text_content()
                .contains("First page")
        );
    }

    fn fragment_test_view() -> BrowserView {
        let url = "https://example.test/page";
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.url = url.into();
        view.stream_frame = Some(EngineFrame::empty(360.0, 180.0));
        view.stream_frame.as_mut().unwrap().start_streaming(url);
        view.feed_streaming_chunk(url, "<style>:target{color:red}</style><body><input id=value value=default><input id=check type=checkbox><div style='height:400px'></div><h2 id=target>Destination</h2><a name=legacy>Legacy</a><div id='café'>Encoded</div><div style='height:200px'></div></body>");
        view.stream_frame.as_mut().unwrap().finish_loading();
        view.update_streamed_frame_before_paint();
        view.loading = false;
        view.current_history_id = Some(1);
        view
    }

    #[test]
    fn fragment_navigation_preserves_live_document_and_applies_target_style() {
        let mut view = fragment_test_view();
        let doc = &mut view.stream_frame.as_mut().unwrap().doc;
        let input = doc.get_element_by_id("value").unwrap();
        let check = doc.get_element_by_id("check").unwrap();
        doc.set_value(input, "live");
        doc.get_box_by_id_mut(check).unwrap().checkedness = true;
        doc.base_url = "https://cdn.example.test/resources/".into();
        let load_id = view.load_id;
        view.navigate("https://example.test/page#target".into());
        assert_eq!(view.load_id, load_id);
        assert!(!view.loading);
        assert!(view.history_cache.is_empty());
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        let doc = view.document().unwrap();
        assert_eq!(doc.get_element_by_id("value"), Some(input));
        assert_eq!(
            crate::types::input_value(doc.get_node(input).unwrap()),
            "live"
        );
        assert!(doc.get_node(check).unwrap().checkedness);
        assert_eq!(doc.base_url, "https://cdn.example.test/resources/");
        assert_eq!(doc.location_component("hash"), "#target");
        assert_eq!(doc.document_uri(), "https://example.test/page#target");
        assert!(doc.scroll_y > 0.0);
        let target = doc.fragment_target_id();
        assert_eq!(
            doc.get_node(target).unwrap().style.color,
            crate::Color::rgb(255, 0, 0)
        );
    }

    #[test]
    fn fragment_history_shares_live_state_and_restores_cached_document_group() {
        let mut view = fragment_test_view();
        let input = view.document().unwrap().get_element_by_id("value").unwrap();
        view.navigate_history("https://example.test/page#target".into(), 2, false);
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        let target_scroll = view.document().unwrap().scroll_y;
        view.stream_frame
            .as_mut()
            .unwrap()
            .doc
            .set_value(input, "shared");
        view.navigate_history("https://example.test/page".into(), 1, true);
        assert_eq!(view.document().unwrap().scroll_y, 0.0);
        assert_eq!(
            crate::types::input_value(view.document().unwrap().get_node(input).unwrap()),
            "shared"
        );
        view.navigate_history("https://example.test/page#target".into(), 2, true);
        assert_eq!(view.document().unwrap().scroll_y, target_scroll);
        view.navigate_history("about:other-page".into(), 3, false);
        assert_eq!(view.history_cache.len(), 1);
        view.navigate_history("https://example.test/page".into(), 1, true);
        assert_eq!(view.url(), "https://example.test/page");
        assert!(!view.loading);
        assert_eq!(
            crate::types::input_value(view.document().unwrap().get_node(input).unwrap()),
            "shared"
        );
        assert_eq!(view.document().unwrap().scroll_y, 0.0);
        view.navigate_history("https://example.test/page#target".into(), 2, true);
        assert_eq!(view.document().unwrap().scroll_y, target_scroll);
        assert!(view.history_cache.is_empty());
    }

    #[test]
    fn fragment_hash_events_are_queued_with_url_payloads() {
        let mut view = fragment_test_view();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observed = seen.clone();
        let doc = &mut view.stream_frame.as_mut().unwrap().doc;
        doc.add_event_listener(
            doc.root.node_id,
            "hashchange",
            Box::new(move |event, _| {
                assert!(event.is_trusted);
                observed
                    .lock()
                    .unwrap()
                    .push((event.old_url.clone(), event.new_url.clone()));
            }),
            crate::dom::events::ListenerOptions::default(),
        );
        view.navigate("https://example.test/page#legacy".into());
        assert!(seen.lock().unwrap().is_empty());
        view.drive_idle_for_test();
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            &[(
                "https://example.test/page".to_string(),
                "https://example.test/page#legacy".to_string()
            )]
        );
        view.navigate("https://example.test/page#legacy".into());
        view.drive_idle_for_test();
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_ne!(view.document().unwrap().fragment_target_id(), 0);
        view.navigate("https://example.test/page#caf%C3%A9".into());
        assert_eq!(
            view.document().unwrap().fragment_target_id(),
            view.document().unwrap().get_element_by_id("café").unwrap()
        );
    }

    #[test]
    fn late_streaming_results_do_not_undo_fragment_navigation() {
        let mut view = fragment_test_view();
        view.loading = true;
        let load_id = view.load_id;
        view.navigate("https://example.test/page#target".into());
        view.tx
            .send(BrowserViewLoadResult::Complete {
                load_id,
                url: "https://example.test/page".into(),
                html: String::new(),
            })
            .unwrap();
        view.poll();
        assert_eq!(view.url(), "https://example.test/page#target");
        assert_eq!(view.load_id, load_id);
        assert!(!view.loading);
    }

    #[test]
    fn fragment_scroll_waits_for_streamed_target_without_restarting_load() {
        let url = "https://example.test/stream";
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.url = url.into();
        view.loading = true;
        view.stream_frame = Some(EngineFrame::empty(360.0, 180.0));
        view.stream_frame.as_mut().unwrap().start_streaming(url);
        view.feed_streaming_chunk(
            url,
            "<!doctype html><body><p>First chunk</p><div style='height:400px'></div>",
        );
        view.update_streamed_frame_before_paint();
        let load_id = view.load_id;
        view.navigate("https://example.test/stream#late".into());
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        assert!(view.fragment_scroll_pending);
        view.feed_streaming_chunk(
            url,
            "<h2 id=late>Late target</h2><div style='height:200px'></div></body>",
        );
        view.update_streamed_frame_before_paint();
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        assert!(!view.fragment_scroll_pending);
        assert!(view.document().unwrap().scroll_y > 0.0);
        assert!(view.loading);
        assert_eq!(view.load_id, load_id);
    }

    #[test]
    fn streamed_idle_scheduler_skips_redundant_updates_but_not_dirty_frames() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body><p>Ready</p>");
        assert!(view.update_streamed_frame_before_paint());
        view.last_stream_frame_update = Some(Instant::now() + Duration::from_secs(1));
        assert!(!view.stream_frame_update_due());

        view.stream_frame.as_mut().unwrap().mark_paint_dirty();
        assert!(!view.stream_frame_update_due());

        view.stream_frame.as_mut().unwrap().mark_style_dirty();
        assert!(view.stream_frame_update_due());
    }

    #[test]
    fn video_overlay_toggle_does_not_advance_the_frame_clock() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body><video id=clip></video>");
        assert!(view.update_streamed_frame_before_paint());
        let id = view.document().unwrap().get_element_by_id("clip").unwrap();
        let updated_at = view.last_stream_frame_update;
        view.stream_frame
            .as_mut()
            .unwrap()
            .doc
            .needs_animation_frame = true;

        assert!(view.set_external_video_overlay(id, true));
        assert_eq!(view.last_stream_frame_update, updated_at);
        assert!(!view.set_external_video_overlay(id, true));
    }

    #[test]
    fn animation_deadlines_do_not_add_paint_time_to_each_frame() {
        let start = Instant::now();
        let interval = Duration::from_nanos(16_666_667);
        let first = next_frame_deadline(None, start, interval);
        assert_eq!(first, start + interval);
        assert_eq!(
            next_frame_deadline(Some(first), start + Duration::from_millis(8), interval),
            first
        );
        assert_eq!(
            next_frame_deadline(Some(first), first + Duration::from_millis(8), interval),
            start + interval * 2,
        );
        assert_eq!(
            next_frame_deadline(Some(first), first + Duration::from_millis(40), interval),
            start + interval * 4,
        );
        assert_eq!(
            next_frame_deadline(
                Some(start + BACKGROUND_MEDIA_INTERVAL),
                start,
                ANIMATION_FRAME_INTERVAL,
            ),
            start + ANIMATION_FRAME_INTERVAL,
            "a visible animation must not inherit a distant background-media deadline"
        );
    }

    #[test]
    fn decoded_media_arrival_bypasses_background_deadline() {
        let mut view = BrowserView::new(320.0, 180.0, PageLoadOptions::default());
        let doc = crate::html::parse_html("<video id='clip' src='clip.mp4'></video>");
        let mut frame = EngineFrame::new(doc, 320.0, 180.0);
        let id = frame.doc.get_element_by_id("clip").unwrap();
        assert!(frame.doc.media_play(id));
        assert!(frame.update_frame());
        frame.doc.get_box_by_id_mut(id).unwrap().layout.border_rect =
            crate::types::Rect::new(0.0, 3000.0, 80.0, 60.0);
        view.stream_frame = Some(frame);
        view.last_stream_frame_update = Some(Instant::now());
        assert_eq!(view.stream_frame_interval(), BACKGROUND_MEDIA_INTERVAL);
        assert!(!view.stream_frame_update_due());

        view.stream_frame
            .as_ref()
            .unwrap()
            .signal_video_update_for_test();
        assert!(
            !view.stream_frame_update_due(),
            "offscreen decoded frames should wait for the background media tick"
        );
        view.stream_frame
            .as_mut()
            .unwrap()
            .doc
            .get_box_by_id_mut(id)
            .unwrap()
            .layout
            .border_rect = crate::types::Rect::new(0.0, 0.0, 80.0, 60.0);
        assert!(
            view.stream_frame_update_due(),
            "a visible decoded frame is urgent"
        );
        view.stream_frame
            .as_mut()
            .unwrap()
            .doc
            .get_box_by_id_mut(id)
            .unwrap()
            .layout
            .border_rect = crate::types::Rect::new(0.0, 3000.0, 80.0, 60.0);
        view.stream_frame
            .as_ref()
            .unwrap()
            .signal_urgent_video_update_for_test();
        assert!(view.stream_frame_update_due());
    }

    #[test]
    fn file_document_relative_links_stay_file_urls() {
        let target = resolve_browser_target(
            "overflow.html",
            "file:///tmp/webcore/examples/html/demo.html",
            "file:///tmp/webcore/examples/html/demo.html",
        );
        assert_eq!(target, "file:///tmp/webcore/examples/html/overflow.html");

        let explicit = resolve_browser_target(
            "file:///tmp/webcore/examples/html/forms_demo.html",
            "file:///tmp/webcore/examples/html/demo.html",
            "file:///tmp/webcore/examples/html/demo.html",
        );
        assert_eq!(
            explicit,
            "file:///tmp/webcore/examples/html/forms_demo.html"
        );
    }

    #[test]
    fn right_mouse_button_dispatches_contextmenu_with_button_two() {
        let seen = Arc::new(Mutex::new(None::<u8>));
        let mut doc = crate::parse_html(
            r#"<html><body style="margin:0"><div id="target" style="width:120px;height:80px"></div></body></html>"#,
        );
        let target = doc.get_element_by_id("target").unwrap();
        let seen_listener = seen.clone();
        doc.add_event_listener(
            target,
            "contextmenu",
            Box::new(move |evt, _doc| {
                *seen_listener.lock().unwrap() = Some(evt.button);
            }),
            crate::dom::events::ListenerOptions::default(),
        );

        let mut view = BrowserView::new(240.0, 160.0, PageLoadOptions::default());
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        view.layout_active();
        let rect = view
            .doc
            .as_ref()
            .unwrap()
            .read()
            .get_box_by_id(target)
            .unwrap()
            .layout
            .border_rect;
        let x = rect.x + rect.w * 0.5;
        let y = rect.y + rect.h * 0.5;

        view.handle_mouse_button(HtmlEventType::MouseDown, x, y, 2);
        view.handle_mouse_button(HtmlEventType::MouseUp, x, y, 2);

        assert_eq!(*seen.lock().unwrap(), Some(2));
    }

    #[test]
    fn resize_grip_drag_reflows_without_clicking_element() {
        let clicks = Arc::new(Mutex::new(0usize));
        let mut doc = crate::parse_html(
            r#"<html><body style="margin:0"><div id="box" style="width:80px;height:60px;overflow:hidden;resize:both"></div></body></html>"#,
        );
        let id = doc.get_element_by_id("box").unwrap();
        let observed = clicks.clone();
        doc.add_event_listener(
            id,
            "click",
            Box::new(move |_, _| *observed.lock().unwrap() += 1),
            crate::dom::events::ListenerOptions::default(),
        );
        let mut view = BrowserView::new(240.0, 160.0, PageLoadOptions::default());
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        view.layout_active();
        let rect = view
            .doc
            .as_ref()
            .unwrap()
            .read()
            .get_node(id)
            .unwrap()
            .layout
            .border_rect;
        let press = (rect.x + rect.w - 5.0, rect.y + rect.h - 5.0);
        assert_eq!(view.cursor_at(press.0, press.1), CSSCursor::SEResize);
        view.handle_mouse_button(HtmlEventType::MouseDown, press.0, press.1, 0);
        view.handle_mouse_move(press.0 + 20.0, press.1 + 10.0);
        view.handle_mouse_button(HtmlEventType::MouseUp, press.0 + 20.0, press.1 + 10.0, 0);

        let doc = view.doc.as_ref().unwrap().read();
        let resized = doc.get_node(id).unwrap().layout.border_rect;
        assert!(
            (resized.w - rect.w - 20.0).abs() < 1.0,
            "width: {resized:?}"
        );
        assert!(
            (resized.h - rect.h - 10.0).abs() < 1.0,
            "height: {resized:?}"
        );
        assert_eq!(*clicks.lock().unwrap(), 0);
    }

    #[test]
    fn implicit_submission_uses_default_button_and_blocking_fields() {
        for (markup, expected) in [
            (
                "<form id=f action='/submit'><input id=t name=q value=hello><button name=go value=first>Go</button><button name=go value=second>Other</button></form>",
                "https://example.test/submit?q=hello&go=first",
            ),
            (
                "<button form=f name=go value=external formaction='/other'>Go</button><form id=f action='/submit'><input id=t name=q value=hello><button>Internal</button></form>",
                "https://example.test/other?go=external&q=hello",
            ),
            (
                "<form id=f action='/submit'><input id=t name=q value=hello><input type=image name=map><button>Other</button></form>",
                "https://example.test/submit?q=hello&map.x=0&map.y=0",
            ),
            (
                "<form id=f action='/submit'><input id=t name=q value=hello><button disabled>Blocked</button><button>Other</button></form>",
                "https://example.test/form",
            ),
            (
                "<form id=f action='/submit'><input id=t name=q value=hello><input name=second></form>",
                "https://example.test/form",
            ),
            (
                "<form id=f action='/submit'><input id=t name=q value=hello formaction='/invalid' formmethod=post></form>",
                "https://example.test/submit?q=hello",
            ),
            (
                "<form id=f action='/submit'><input id=t name=q required><button formnovalidate name=go value=unchecked>Go</button></form>",
                "https://example.test/submit?q=&go=unchecked",
            ),
        ] {
            let mut doc = crate::parse_html(markup);
            let text = doc.get_element_by_id("t").unwrap();
            doc.focus(text);
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            assert!(view.submit_focused_text_control(), "{markup}");
            assert_eq!(view.url, expected, "{markup}");
        }
    }

    #[test]
    fn textarea_placeholder_soft_wrap_uses_control_font_without_pseudo_rule() {
        let doc = crate::parse_html(
            "<body style='background:white'><textarea id=t placeholder='alpha beta gamma delta epsilon zeta' style='width:95px;height:120px;border:0;padding:0;color:black;background:white;font:20px/30px sans-serif'></textarea></body>",
        );
        let id = doc.get_element_by_id("t").unwrap();
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        let rect = view
            .document()
            .unwrap()
            .get_node(id)
            .unwrap()
            .layout
            .content_rect;
        let mut target = Pixmap::new(360, 180).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        let second_line_has_text = (rect.y as u32 + 35..rect.y as u32 + 55).any(|y| {
            (rect.x as u32 + 2..(rect.x + rect.w) as u32 - 2).any(|x| {
                let offset = (y * target.width() + x) as usize * 4;
                let pixel = &target.data()[offset..offset + 4];
                pixel[0] < 200 && pixel[1] < 200 && pixel[2] < 200
            })
        });
        assert!(
            second_line_has_text,
            "default placeholder must paint its second visual row"
        );
    }

    #[test]
    fn textarea_soft_wrapping_publishes_scroll_extent_and_reveals_caret() {
        for scale in [1.0, 2.0] {
            let doc = crate::parse_html(
                "<textarea id=t style='width:95px;height:35px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon zeta</textarea>",
            );
            let id = doc.get_element_by_id("t").unwrap();
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.attach_document(crate::browser::BrowserDocument::new(doc));
            {
                let mut doc = view.doc.as_ref().unwrap().write();
                let node = doc.get_node(id).unwrap();
                assert!(
                    node.layout.scroll_height > 90.0,
                    "wrapped extent: {:?}",
                    node.layout
                );
                doc.focus(id);
            }
            view.handle_key(HtmlEventType::KeyDown, 35, None, false, false, false, false);
            let mut target = Pixmap::new((360.0 * scale) as u32, (180.0 * scale) as u32).unwrap();
            view.paint_into(&mut target, 0, 0, scale);
            let doc = view.document().unwrap();
            let node = doc.get_node(id).unwrap();
            assert_eq!(
                node.input_cursor,
                crate::types::input_value(node).chars().count()
            );
            assert!(
                node.layout.scroll_top > 0.0,
                "wrapped caret reveal, scale={scale}"
            );
        }
    }

    #[test]
    fn textarea_wrap_off_reveals_horizontal_caret() {
        let doc = crate::parse_html(
            "<textarea id=t wrap=off style='width:95px;height:60px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon zeta</textarea>",
        );
        let id = doc.get_element_by_id("t").unwrap();
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        view.doc.as_ref().unwrap().write().focus(id);
        view.handle_key(HtmlEventType::KeyDown, 35, None, false, false, false, false);
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        assert!(
            view.document()
                .unwrap()
                .get_node(id)
                .unwrap()
                .layout
                .scroll_left
                > 0.0
        );
        view.handle_key(HtmlEventType::KeyDown, 36, None, false, false, false, false);
        view.paint_into(&mut Pixmap::new(360, 180).unwrap(), 0, 0, 1.0);
        assert_eq!(
            view.document()
                .unwrap()
                .get_node(id)
                .unwrap()
                .layout
                .scroll_left,
            0.0
        );
    }

    #[test]
    fn content_sized_textarea_uses_wrapped_rows_and_height_limits() {
        let mut renderer = crate::Renderer::new();
        let doc = renderer.load_html(
            "<textarea id=grow style='field-sizing:content;max-width:95px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon zeta</textarea><textarea id=limit style='field-sizing:content;max-width:95px;max-height:60px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon zeta</textarea>",
            360.0,
        );
        let grow = doc
            .get_node(doc.get_element_by_id("grow").unwrap())
            .unwrap();
        assert!(grow.layout.content_rect.w <= 95.0);
        assert!(grow.layout.content_rect.h >= 90.0);
        let limit = doc
            .get_node(doc.get_element_by_id("limit").unwrap())
            .unwrap();
        assert!(limit.layout.border_rect.h <= 60.0);
        assert!(limit.layout.scroll_height > limit.layout.content_rect.h);
    }

    #[test]
    fn textarea_wrapping_respects_author_break_policy() {
        let mut renderer = crate::Renderer::new();
        for (policy, wraps) in [
            ("", true),
            ("overflow-wrap:normal", false),
            ("overflow-wrap:normal;word-break:break-all", true),
            ("white-space:pre;word-break:break-all", false),
        ] {
            let doc = renderer.load_html(&format!(
                "<textarea id=t style='width:95px;height:60px;font:20px/30px sans-serif;{policy}'>ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz</textarea>"
            ), 360.0);
            let node = doc.get_node(doc.get_element_by_id("t").unwrap()).unwrap();
            assert_eq!(
                node.layout.scroll_height > node.layout.content_rect.h,
                wraps,
                "{policy}"
            );
            assert_eq!(
                node.layout.scroll_width > node.layout.content_rect.w,
                !wraps,
                "{policy}"
            );
        }
    }

    #[test]
    fn textarea_wrap_off_is_a_cascadable_ua_default() {
        let mut renderer = crate::Renderer::new();
        let doc = renderer.load_html(
            "<textarea id=off wrap=off style='width:95px;height:60px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon</textarea><textarea id=override wrap=off style='white-space:pre-wrap;width:95px;height:60px;font:20px/30px sans-serif'>alpha beta gamma delta epsilon</textarea>",
            360.0,
        );
        let off = doc.get_node(doc.get_element_by_id("off").unwrap()).unwrap();
        assert_eq!(off.style.white_space, crate::types::WhiteSpace::Pre);
        assert!(off.layout.scroll_width > off.layout.content_rect.w);
        assert_eq!(off.layout.scroll_height, off.layout.content_rect.h);
        let wrapped = doc
            .get_node(doc.get_element_by_id("override").unwrap())
            .unwrap();
        assert!(wrapped.layout.scroll_height > wrapped.layout.content_rect.h);
    }

    #[test]
    fn textarea_system_colors_are_cascadable_ua_defaults() {
        let mut renderer = crate::Renderer::new();
        let doc = renderer.load_html(
            "<body style='background:black;color:red'><textarea id=default></textarea><textarea id=authored style='background:transparent;color:inherit'></textarea>",
            360.0,
        );
        let default = doc
            .get_node(doc.get_element_by_id("default").unwrap())
            .unwrap();
        assert_eq!(
            default.style.background_color,
            crate::types::Color::rgb(255, 255, 255)
        );
        assert_eq!(default.style.color, crate::types::Color::rgb(0, 0, 0));
        let authored = doc
            .get_node(doc.get_element_by_id("authored").unwrap())
            .unwrap();
        assert_eq!(authored.style.background_color.a, 0);
        assert_eq!(authored.style.color, crate::types::Color::rgb(255, 0, 0));
    }

    #[test]
    fn keyboard_cancellation_precedes_tab_and_implicit_submission() {
        for (code, name) in [(9, "Tab"), (13, "Enter")] {
            let mut doc = crate::parse_html(
                "<form action='/submit'><input id=t name=q value=hello><button>Go</button></form>",
            );
            let id = doc.get_element_by_id("t").unwrap();
            let keys = Arc::new(Mutex::new(0));
            let observed = keys.clone();
            doc.add_event_listener(
                id,
                "keydown",
                Box::new(move |event, _| {
                    assert_eq!(event.key, name);
                    *observed.lock().unwrap() += 1;
                    event.prevent_default();
                }),
                crate::dom::events::ListenerOptions::default(),
            );
            doc.focus(id);
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.handle_key(
                HtmlEventType::KeyDown,
                code,
                None,
                false,
                false,
                false,
                false,
            );
            assert_eq!(*keys.lock().unwrap(), 1);
            assert_eq!(view.url, "https://example.test/form");
            assert_eq!(view.document().unwrap().focused_box, id);
        }
    }

    #[test]
    fn keyboard_enter_activates_links_and_image_submitters() {
        for (markup, target) in [
            (
                "<a id=go href='#next'>Next</a><div id=next>Destination</div>",
                "https://example.test/form#next",
            ),
            (
                "<form action='/submit'><input id=go type=image name=pic></form>",
                "https://example.test/submit?pic.x=0&pic.y=0",
            ),
            (
                "<form action='/submit'><button id=go name=action value=go>Go</button></form>",
                "https://example.test/submit?action=go",
            ),
        ] {
            let mut doc = crate::parse_html(markup);
            let id = doc.get_element_by_id("go").unwrap();
            doc.focus(id);
            let count = Arc::new(Mutex::new(0));
            let observed = count.clone();
            doc.add_event_listener(
                id,
                "click",
                Box::new(move |event, _| {
                    assert!(event.is_trusted);
                    *observed.lock().unwrap() += 1;
                }),
                crate::dom::events::ListenerOptions::default(),
            );
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            assert!(view.handle_key(HtmlEventType::KeyDown, 13, None, false, false, false, false));
            assert_eq!(view.url, target, "{markup}");
            assert_eq!(*count.lock().unwrap(), 1);
        }
    }

    #[test]
    fn keyboard_space_activates_once_on_release_and_shows_pressed_state() {
        let mut doc = crate::parse_html("<input id=go type=checkbox>");
        let id = doc.get_element_by_id("go").unwrap();
        doc.focus(id);
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        for _ in 0..3 {
            assert!(view.handle_key(
                HtmlEventType::KeyDown,
                32,
                Some(' '),
                false,
                false,
                false,
                false
            ));
            assert!(!view.document().unwrap().get_node(id).unwrap().checkedness);
            assert_eq!(view.document().unwrap().active_box, id);
        }
        assert!(view.handle_key(
            HtmlEventType::KeyUp,
            32,
            Some(' '),
            false,
            false,
            false,
            false
        ));
        assert!(view.document().unwrap().get_node(id).unwrap().checkedness);
        assert_eq!(view.document().unwrap().active_box, 0);
        view.handle_key(
            HtmlEventType::KeyUp,
            32,
            Some(' '),
            false,
            false,
            false,
            false,
        );
        assert!(view.document().unwrap().get_node(id).unwrap().checkedness);
    }

    #[test]
    fn keyboard_activation_honors_cancellation_and_disabled_state() {
        for (markup, canceled) in [
            ("<a id=go href='/next'>Next</a>", "click"),
            ("<a id=go href='/next'>Next</a>", "keydown"),
            (
                "<form action='/submit'><button id=go>Go</button></form>",
                "click",
            ),
            (
                "<form action='/submit'><button id=go disabled>Go</button></form>",
                "",
            ),
            ("<div inert><a id=go href='/next'>Next</a></div>", ""),
        ] {
            let mut doc = crate::parse_html(markup);
            let id = doc.get_element_by_id("go").unwrap();
            doc.focus(id);
            if !canceled.is_empty() {
                doc.add_event_listener(
                    id,
                    canceled,
                    Box::new(|event, _| event.prevent_default()),
                    crate::dom::events::ListenerOptions::default(),
                );
            }
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.handle_key(HtmlEventType::KeyDown, 13, None, false, false, false, false);
            assert_eq!(view.url, "https://example.test/form", "{markup} {canceled}");
        }
    }

    #[test]
    fn keyboard_space_cancellation_and_focus_changes_disarm_activation() {
        for cancel in ["keydown", "keyup", "click", "blur"] {
            let mut doc =
                crate::parse_html("<input id=go type=checkbox><button id=other>Other</button>");
            let id = doc.get_element_by_id("go").unwrap();
            let other = doc.get_element_by_id("other").unwrap();
            doc.focus(id);
            if cancel != "blur" {
                doc.add_event_listener(
                    id,
                    cancel,
                    Box::new(|event, _| event.prevent_default()),
                    crate::dom::events::ListenerOptions::default(),
                );
            }
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.handle_key(
                HtmlEventType::KeyDown,
                32,
                Some(' '),
                false,
                false,
                false,
                false,
            );
            if cancel == "blur" {
                let mut doc = view.active_doc_mut().unwrap();
                doc.focus(other);
                doc.focus(id);
            }
            view.handle_key(
                HtmlEventType::KeyUp,
                32,
                Some(' '),
                false,
                false,
                false,
                false,
            );
            assert!(
                !view.document().unwrap().get_node(id).unwrap().checkedness,
                "{cancel}"
            );
            assert_eq!(view.document().unwrap().keyboard_space_target, 0);
        }
    }

    #[test]
    fn implicit_submission_default_click_can_cancel_before_validation() {
        let mut doc = crate::parse_html(
            "<form action='/submit'><input id=t name=q required><button id=go>Go</button></form>",
        );
        let text = doc.get_element_by_id("t").unwrap();
        let go = doc.get_element_by_id("go").unwrap();
        let clicks = Arc::new(Mutex::new(0));
        let observed = clicks.clone();
        doc.add_event_listener(
            go,
            "click",
            Box::new(move |event, _| {
                *observed.lock().unwrap() += 1;
                assert!(event.is_trusted);
                event.prevent_default();
            }),
            crate::dom::events::ListenerOptions::default(),
        );
        doc.focus(text);
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.url = "https://example.test/form".into();
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        assert!(view.submit_focused_text_control());
        assert_eq!(*clicks.lock().unwrap(), 1);
        assert_eq!(view.url, "https://example.test/form");
        assert!(!view.interaction_layout_pending);
    }

    #[test]
    fn browser_view_submission_validates_before_submit_event() {
        for (form_attrs, button_attrs, should_submit) in [
            ("", "", false),
            ("novalidate", "", true),
            ("", "formnovalidate", true),
        ] {
            let mut doc = crate::parse_html(&format!(
                "<body style='margin:0'><form id=f action='/submit' {form_attrs}><input id=required required name=value><button id=go {button_attrs}>Submit</button></form></body>"
            ));
            let form = doc.get_element_by_id("f").unwrap();
            let required = doc.get_element_by_id("required").unwrap();
            let go = doc.get_element_by_id("go").unwrap();
            let submits = Arc::new(Mutex::new(0));
            let count = submits.clone();
            doc.add_event_listener(
                form,
                "submit",
                Box::new(move |_, _| {
                    *count.lock().unwrap() += 1;
                }),
                crate::dom::events::ListenerOptions::default(),
            );
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.layout_active();
            let rect = view
                .doc
                .as_ref()
                .unwrap()
                .read()
                .get_node(go)
                .unwrap()
                .layout
                .border_rect;
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button(kind, rect.x + 4.0, rect.y + 4.0, 0);
            }
            assert_eq!(*submits.lock().unwrap(), usize::from(should_submit));
            if should_submit {
                assert_eq!(view.url, "https://example.test/submit?value=");
            } else {
                assert_eq!(view.url, "https://example.test/form");
                assert_eq!(view.doc.as_ref().unwrap().read().focused_box, required);
                assert!(view.interaction_layout_pending);
            }
        }
    }

    #[test]
    fn browser_view_disabled_submitters_do_not_navigate() {
        for markup in [
            "<button id=go disabled>Submit</button>",
            "<input id=go type=submit disabled value=Submit>",
            "<input id=go type=image disabled width=40 height=30 alt=Submit>",
            "<fieldset disabled><button id=go>Submit</button></fieldset>",
        ] {
            let doc = crate::parse_html(&format!(
                "<body style='margin:0'><form action='/submitted'>{markup}</form></body>"
            ));
            let id = doc.get_element_by_id("go").unwrap();
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.layout_active();
            let rect = view
                .doc
                .as_ref()
                .unwrap()
                .read()
                .get_node(id)
                .unwrap()
                .layout
                .border_rect;
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button(kind, rect.x + 4.0, rect.y + 4.0, 0);
            }
            assert_eq!(view.url, "https://example.test/form", "{markup}");
        }
    }

    #[test]
    fn pointer_move_listener_updates_are_committed_at_paint() {
        let mut doc = crate::parse_html(
            "<body><button id=go type=button>Update</button><div id=status>Before</div></body>",
        );
        let id = doc.get_element_by_id("go").unwrap();
        let status = doc.get_element_by_id("status").unwrap();
        doc.add_event_listener(
            id,
            "mousemove",
            Box::new(move |_, doc| doc.set_text_content(status, "After")),
            crate::dom::events::ListenerOptions::default(),
        );
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        let rect = view
            .document()
            .unwrap()
            .get_node(id)
            .unwrap()
            .layout
            .border_rect;
        let generation = view.document().unwrap().layout_generation;
        for offset in 1..5 {
            assert!(view.handle_mouse_move(rect.x + offset as f32, rect.y + 4.0));
            assert_eq!(view.document().unwrap().layout_generation, generation);
        }
        assert_eq!(view.document().unwrap().text_content(status), "After");
        assert!(view.interaction_layout_pending);
        let mut target = Pixmap::new(360, 180).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(view.document().unwrap().layout_generation > generation);
        assert!(!view.interaction_layout_pending);
    }

    #[test]
    fn pointer_listener_updates_coalesce_layout_until_paint() {
        let mut doc = crate::parse_html(
            "<body><button id=go type=button>Update</button><div id=status>Before</div></body>",
        );
        let id = doc.get_element_by_id("go").unwrap();
        let status = doc.get_element_by_id("status").unwrap();
        doc.add_event_listener(
            id,
            "mouseup",
            Box::new(move |_, doc| doc.set_text_content(status, "After")),
            crate::dom::events::ListenerOptions::default(),
        );
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        let rect = view
            .document()
            .unwrap()
            .get_node(id)
            .unwrap()
            .layout
            .border_rect;
        let generation = view.document().unwrap().layout_generation;
        for _ in 0..4 {
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button(kind, rect.x + 4.0, rect.y + 4.0, 0);
            }
            assert_eq!(view.document().unwrap().layout_generation, generation);
            assert_eq!(view.document().unwrap().text_content(status), "After");
        }
        let mut target = Pixmap::new(360, 180).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(view.document().unwrap().layout_generation > generation);
    }

    #[test]
    fn tab_bursts_coalesce_focus_geometry_and_preserve_pointer_hover() {
        let doc = crate::parse_html(
            "<style>button:focus{width:120px;outline:3px solid red}button:hover{background:blue}</style><button id=a>A</button><button id=b>B</button>",
        );
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.attach_document(crate::browser::BrowserDocument::new(doc));
        let a = view.document().unwrap().get_element_by_id("a").unwrap();
        let rect = view
            .document()
            .unwrap()
            .get_node(a)
            .unwrap()
            .layout
            .border_rect;
        view.handle_mouse_move(rect.x + 2.0, rect.y + 2.0);
        let mut target = Pixmap::new(360, 180).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        let generation = view.document().unwrap().layout_generation;
        let hover = view.document().unwrap().hovered_box;
        for _ in 0..3 {
            assert!(view.handle_key(HtmlEventType::KeyDown, 9, None, false, false, false, false));
            assert_eq!(view.document().unwrap().layout_generation, generation);
            assert_eq!(view.document().unwrap().hovered_box, hover);
        }
        assert_eq!(view.document().unwrap().focused_box, a);
        view.paint_into(&mut target, 0, 0, 1.0);
        let doc = view.document().unwrap();
        assert_eq!(doc.layout_generation, generation + 1);
        let node = doc.get_node(a).unwrap();
        assert_eq!(node.style.outline_color, crate::Color::rgb(255, 0, 0));
        assert_eq!(node.style.background_color, crate::Color::rgb(0, 0, 255));
        assert!((node.layout.border_rect.w - 120.0).abs() < 0.1);
    }

    #[test]
    fn browser_view_canceled_click_does_not_activate_links_or_submitters() {
        for markup in [
            "<a id=go href='/next'>Link</a>",
            "<form action='/next'><button id=go>Submit</button></form>",
            "<form action='/next'><input id=go type=image width=40 height=30></form>",
        ] {
            let mut doc = crate::parse_html(&format!("<body style='margin:0'>{markup}</body>"));
            let id = doc.get_element_by_id("go").unwrap();
            doc.add_event_listener(
                id,
                "click",
                Box::new(|event, _| event.prevent_default()),
                crate::dom::events::ListenerOptions::default(),
            );
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.layout_active();
            let rect = view
                .document()
                .unwrap()
                .get_node(id)
                .unwrap()
                .layout
                .border_rect;
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button(kind, rect.x + 4.0, rect.y + 4.0, 0);
            }
            assert_eq!(view.url, "https://example.test/form", "{markup}");
            assert!(view.document().unwrap().visited_urls.is_empty());
        }
    }

    #[test]
    fn browser_view_release_without_matching_press_does_not_activate() {
        for markup in [
            "<a id=go href='/next'>Link</a>",
            "<form action='/next'><input id=go type=image width=40 height=30></form>",
        ] {
            let doc = crate::parse_html(&format!("<body style='margin:0'>{markup}</body>"));
            let id = doc.get_element_by_id("go").unwrap();
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.layout_active();
            let rect = view
                .document()
                .unwrap()
                .get_node(id)
                .unwrap()
                .layout
                .border_rect;
            view.handle_mouse_button(HtmlEventType::MouseDown, 350.0, 170.0, 0);
            view.handle_mouse_button(HtmlEventType::MouseUp, rect.x + 4.0, rect.y + 4.0, 0);
            assert_eq!(view.url, "https://example.test/form", "{markup}");
            assert!(view.document().unwrap().visited_urls.is_empty());
        }
    }

    #[test]
    fn browser_view_secondary_click_does_not_activate_links_or_submitters() {
        for markup in [
            "<a id=go href='/next'>Link</a>",
            "<form action='/next'><button id=go>Submit</button></form>",
        ] {
            let doc = crate::parse_html(&format!("<body style='margin:0'>{markup}</body>"));
            let id = doc.get_element_by_id("go").unwrap();
            let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
            view.url = "https://example.test/form".into();
            view.doc = Some(crate::browser::BrowserDocument::new(doc));
            view.layout_active();
            let rect = view
                .doc
                .as_ref()
                .unwrap()
                .read()
                .get_node(id)
                .unwrap()
                .layout
                .border_rect;
            for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
                view.handle_mouse_button(kind, rect.x + 4.0, rect.y + 4.0, 2);
            }
            assert_eq!(view.url, "https://example.test/form", "{markup}");
        }
    }

    #[test]
    fn browser_view_image_submit_uses_content_box_css_coordinates() {
        let doc = crate::parse_html(
            "<body style='margin:0'><form action='/submit'><input id=go type=image name=map style='width:40px;height:30px;border:2px solid black;padding:3px'></form></body>",
        );
        let id = doc.get_element_by_id("go").unwrap();
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.url = "https://example.test/form".into();
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        view.layout_active();
        let rect = view
            .doc
            .as_ref()
            .unwrap()
            .read()
            .get_node(id)
            .unwrap()
            .layout
            .content_rect;
        for kind in [HtmlEventType::MouseDown, HtmlEventType::MouseUp] {
            view.handle_mouse_button(kind, rect.x + 8.75, rect.y + 6.25, 0);
        }
        assert_eq!(view.url, "https://example.test/submit?map.x=8&map.y=6");
    }

    #[test]
    fn browser_view_submit_button_navigates_with_successful_controls() {
        let doc = crate::parse_html(
            r#"<html><body style="margin:0">
                <form action="https://example.test/login" method="get">
                    <input id="user" name="user" value="Youness">
                    <button id="submit" type="submit">Go</button>
                </form>
            </body></html>"#,
        );
        let submit_id = doc.get_element_by_id("submit").unwrap();

        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        view.url = "https://example.test/form".to_string();
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        view.layout_active();

        let rect = view
            .doc
            .as_ref()
            .unwrap()
            .read()
            .get_box_by_id(submit_id)
            .unwrap()
            .layout
            .border_rect;
        let x = rect.x + rect.w * 0.5;
        let y = rect.y + rect.h * 0.5;

        view.handle_mouse_button(HtmlEventType::MouseDown, x, y, 0);
        view.handle_mouse_button(HtmlEventType::MouseUp, x, y, 0);

        assert_eq!(view.url, "https://example.test/login?user=Youness");
    }

    #[test]
    fn browser_view_post_submit_sends_successful_controls_in_body() {
        let view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        let (url, options) = view.form_submission_request(
            "https://example.test/submit".to_string(),
            "post",
            vec![
                ("csrf_token".to_string(), "abc 123".to_string()),
                ("login".to_string(), "youness".to_string()),
                ("password".to_string(), "secret".to_string()),
            ],
        );

        assert_eq!(url, "https://example.test/submit");
        assert_eq!(options.request_method, "POST");
        assert_eq!(
            options.request_content_type.as_deref(),
            Some("application/x-www-form-urlencoded")
        );
        assert_eq!(
            options.request_body.as_deref(),
            Some("csrf_token=abc+123&login=youness&password=secret".as_bytes())
        );
    }

    #[test]
    fn browser_view_collects_hidden_csrf_control_from_form_dom() {
        let doc = crate::parse_html(
            r#"<form id="login" method="post">
                <input type="hidden" name="csrf_token" value="abc 123">
                <input name="login" value="youness">
                <button type="submit">Connecter</button>
            </form>"#,
        );
        let form_id = doc.get_element_by_id("login").unwrap();

        let data = collect_form_data_for_form(&doc.root, form_id);

        assert_eq!(
            data,
            vec![
                ("csrf_token".to_string(), "abc 123".to_string()),
                ("login".to_string(), "youness".to_string()),
            ]
        );
    }

    #[test]
    fn browser_view_collects_table_login_controls_with_hidden_csrf() {
        let doc = crate::parse_html(
            r#"<table>
                <form id="login" method="post">
                    <input type="hidden" name="csrf_token" value="abc123">
                    <tr>
                        <td><label>Login</label></td>
                        <td><input name="username" value="admin"></td>
                    </tr>
                    <tr>
                        <td><label>Password</label></td>
                        <td><input type="password" name="password" value="admin"></td>
                    </tr>
                    <tr>
                        <td colspan="2"><button type="submit">Connecter</button></td>
                    </tr>
                </form>
            </table>"#,
        );
        let form_id = doc.get_element_by_id("login").unwrap();

        let data = collect_form_data_for_form(&doc.root, form_id);

        assert_eq!(
            data,
            vec![
                ("csrf_token".to_string(), "abc123".to_string()),
                ("username".to_string(), "admin".to_string()),
                ("password".to_string(), "admin".to_string()),
            ]
        );
    }

    #[test]
    fn browser_view_set_options_preserves_cookie_jar_by_default() {
        let mut view = BrowserView::new(360.0, 180.0, PageLoadOptions::default());
        let jar = view.options.cookie_jar.clone().expect("browser cookie jar");

        view.set_options(PageLoadOptions {
            cache_dir: Some("cache".to_string()),
            ..Default::default()
        });

        let after = view
            .options
            .cookie_jar
            .clone()
            .expect("preserved cookie jar");
        assert!(Arc::ptr_eq(&jar, &after));
    }

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

        assert!(
            view.update_streamed_frame_before_paint(),
            "browser update/idle should prepare the streamed frame before paint"
        );
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
            "first streamed update must consume UA/inline/current CSS through layout before rendering"
        );
    }

    #[test]
    fn browser_view_update_drives_frame_resource_scheduler() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            "<!doctype html><html><head><style>.logo{display:block;width:16px;height:16px;background-image:url(/logo.png)}</style></head><body><div class='logo'></div>",
        );

        assert!(
            view.update_streamed_frame_before_paint(),
            "browser update/idle should discover CSS background dependencies"
        );

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
        assert!(view.update_streamed_frame_before_paint());
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(!view.stream_needs_layout);

        let (tx, rx) = std::sync::mpsc::channel();
        let mut sheet = crate::css::Stylesheet::default();
        sheet.parse_and_add_author("p{color:red}");
        tx.send(
            (
                0,
                "https://example.test/late.css".to_string(),
                sheet,
                String::new(),
            )
                .into(),
        )
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
    fn browser_view_paint_consumes_streamed_hover_style_work() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            r#"
            <!doctype html><html><head><style>
              body { margin: 0; }
              mdn-dropdown { display: contents; }
              .menu { display: flex; }
              .menu__tab-button {
                display: flex;
                align-items: center;
                padding: 8px 12px;
                background: rgb(0, 0, 0);
                color: white;
              }
              .menu__tab-button:is([aria-expanded=true], :hover) {
                background: rgb(0, 96, 223);
              }
            </style></head><body>
              <nav class="menu">
                <mdn-dropdown>
                  <button id="tab" class="menu__tab-button">HTML</button>
                </mdn-dropdown>
              </nav>
            "#,
        );
        assert!(view.update_streamed_frame_before_paint());

        let tab_id = view.document().unwrap().get_element_by_id("tab").unwrap();
        let rect = view
            .document()
            .unwrap()
            .get_node(tab_id)
            .unwrap()
            .layout
            .border_rect;
        assert!(view.handle_mouse_move(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5));
        assert!(
            view.stream_needs_layout,
            "streamed hover should queue style/layout work for the browser frame"
        );

        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(
            !view.stream_needs_layout,
            "paint should consume queued interaction layout without polling resources"
        );
        let document = view.document().unwrap();
        let tab = document.get_node(tab_id).unwrap();
        assert_eq!(tab.style.background_color.r, 0);
        assert_eq!(tab.style.background_color.g, 96);
        assert_eq!(tab.style.background_color.b, 223);
    }

    #[test]
    fn browser_view_popup_pick_does_not_activate_covered_link_or_pointer_listener() {
        let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(320.0, 200.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base,
            "<body style='margin:0'><a id='covered' href='https://example.test/wrong' style='position:absolute;top:0;left:0;width:320px;height:140px'>Covered link</a>\
             <select id='s' style='position:absolute;top:160px;width:160px'><option>A</option><option>B</option><option>C</option></select></body>");
        assert!(view.update_streamed_frame_before_paint());
        let calls = Arc::new(Mutex::new(0));
        let observed = calls.clone();
        let (point, id) = {
            let mut doc = view.active_doc_mut().unwrap();
            let covered = doc.get_element_by_id("covered").unwrap();
            doc.add_event_listener(
                covered,
                "pointerup",
                Box::new(move |_, _| {
                    *observed.lock().unwrap() += 1;
                }),
                crate::dom::events::ListenerOptions::default(),
            );
            let id = doc.get_element_by_id("s").unwrap();
            doc.open_select = id;
            let popup = doc.select_popup().unwrap();
            let row = &popup.rows[1];
            (
                (
                    popup.rect.x + 10.0,
                    popup.rect.y + 4.0 + row.top + row.height / 2.0,
                ),
                id,
            )
        };
        let before_url = view.url.clone();
        view.handle_mouse_button(HtmlEventType::MouseDown, point.0, point.1, 0);
        view.handle_mouse_button(HtmlEventType::MouseUp, point.0, point.1, 0);
        assert_eq!(view.document().unwrap().selected_index(id), 1);
        assert_eq!(
            view.url, before_url,
            "popup must not navigate the covered link"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            0,
            "popup must not dispatch pointerup beneath itself"
        );
    }

    #[test]
    fn browser_view_select_popup_wheel_repaints_without_page_layout() {
        let mut view = BrowserView::new(320.0, 200.0, PageLoadOptions::default());
        let base = "https://example.test/";
        let options: String = (0..30)
            .map(|i| format!("<option>Item {i}</option>"))
            .collect();
        view.stream_frame = Some(EngineFrame::empty(320.0, 200.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, &format!(
            "<body style='margin:0'><select id='s' style='position:absolute;top:160px;width:160px'>{options}</select></body>"));
        assert!(view.update_streamed_frame_before_paint());
        let mut target = Pixmap::new(320, 200).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        let (point, generation) = {
            let mut doc = view.active_doc_mut().unwrap();
            doc.open_select = doc.get_element_by_id("s").unwrap();
            let popup = doc.select_popup().unwrap();
            (
                (popup.rect.x + 20.0, popup.rect.y + 20.0),
                doc.layout_generation,
            )
        };
        view.handle_mouse_move(point.0, point.1);
        assert!(view.handle_wheel(0.0, 80.0));
        view.paint_into(&mut target, 0, 0, 1.0);
        let doc = view.document().unwrap();
        assert_eq!(doc.dropdown_scroll, 80.0);
        assert_eq!(doc.scroll_y, 0.0);
        assert_eq!(
            doc.layout_generation, generation,
            "popup wheel must not relayout the page"
        );
    }

    #[test]
    fn browser_view_wheel_scrolls_inner_container_without_layout() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body style='margin:0'><div id='mail' style='width:100px;height:100px;overflow:auto'><div style='position:relative;height:100px;background:red'></div><div style='position:relative;height:100px;background:blue'></div></div></body>");
        assert!(view.update_streamed_frame_before_paint());
        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert_eq!(target.pixel(50, 50).unwrap().red(), 255);
        let generation = view.document().unwrap().layout_generation;
        view.handle_mouse_move(50.0, 50.0);
        assert!(view.handle_wheel(0.0, 60.0));
        let doc = view.document().unwrap();
        let id = doc.get_element_by_id("mail").unwrap();
        assert_eq!(doc.get_node(id).unwrap().layout.scroll_top, 60.0);
        assert_eq!(doc.scroll_y, 0.0);
        assert_eq!(doc.layout_generation, generation);
        drop(doc);
        view.paint_into(&mut target, 0, 0, 1.0);
        assert_eq!(target.pixel(50, 50).unwrap().blue(), 255);
        let outside = target.pixel(50, 150).unwrap();
        assert_eq!(
            (outside.red(), outside.green(), outside.blue()),
            (255, 255, 255),
            "deferred positioned rows must remain clipped to the scrollport"
        );
        assert_eq!(view.document().unwrap().layout_generation, generation);
    }

    #[test]
    fn browser_view_trackpad_scrolls_both_axes_of_inner_container() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body style='margin:0'><div id='pane' style='width:100px;height:100px;overflow:auto'><div style='width:300px;height:300px;background:red'></div></div></body>");
        assert!(view.update_streamed_frame_before_paint());
        let generation = view.document().unwrap().layout_generation;
        view.handle_mouse_move(50.0, 50.0);
        assert!(view.handle_wheel(40.0, 60.0));
        let doc = view.document().unwrap();
        let pane = doc
            .get_node(doc.get_element_by_id("pane").unwrap())
            .unwrap();
        assert_eq!(pane.layout.scroll_left, 40.0);
        assert_eq!(pane.layout.scroll_top, 60.0);
        assert_eq!(doc.scroll_x, 0.0);
        assert_eq!(doc.scroll_y, 0.0);
        assert_eq!(doc.layout_generation, generation);
    }

    #[test]
    fn browser_view_scroll_priority_defers_streamed_resource_update() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            "<!doctype html><body><div style='height:2000px'>Ready</div>",
        );
        assert!(view.update_streamed_frame_before_paint());
        assert!(view.handle_wheel(0.0, 80.0));

        let (tx, rx) = std::sync::mpsc::channel();
        let mut sheet = crate::css::Stylesheet::default();
        sheet.parse_and_add_author("div{color:red}");
        tx.send(
            (
                0,
                "https://example.test/late.css".to_string(),
                sheet,
                String::new(),
            )
                .into(),
        )
        .unwrap();
        view.stream_frame.as_mut().unwrap().doc.pending_stylesheets = Some(rx);

        let changed = view.drive_idle_for_test();
        assert!(changed, "scroll-priority idle should request a redraw");
        assert!(
            view.stream_frame
                .as_ref()
                .unwrap()
                .doc
                .pending_stylesheets
                .is_some(),
            "scroll-priority idle must not drain resource queues before presenting scroll"
        );
    }

    #[test]
    fn browser_view_wheel_does_not_synchronously_flush_dirty_stream_layout() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            "<!doctype html><body><div style='height:2000px'>Ready</div>",
        );
        assert!(view.update_streamed_frame_before_paint());

        let doc = &mut view.stream_frame.as_mut().unwrap().doc;
        doc.style_dirty = true;
        view.stream_needs_layout = false;

        assert!(view.handle_wheel(0.0, 80.0));
        let doc = &view.stream_frame.as_ref().unwrap().doc;
        assert!(
            doc.style_dirty,
            "wheel scrolling must not run a blocking recascade/layout inline"
        );
        assert!(
            view.stream_needs_layout,
            "dirty work should be scheduled for the browser frame loop"
        );

        assert!(view.drive_idle_for_test());
        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(
            view.stream_frame.as_ref().unwrap().doc.style_dirty,
            "the scroll presentation must not consume queued style/layout work"
        );
        assert!(!view.scroll_priority_frame);
        assert!(view.drive_idle_for_test());
        assert!(
            !view.stream_frame.as_ref().unwrap().doc.style_dirty,
            "the following frame should process the queued work"
        );
    }

    #[test]
    fn browser_view_scroll_priority_defers_loader_chunks() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/".to_string();
        view.load_id = 7;
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(&base);
        view.feed_streaming_chunk(
            &base,
            "<!doctype html><body><div style='height:2000px'>Ready</div>",
        );
        assert!(view.update_streamed_frame_before_paint());
        let before_len = view.streamed_html_len;
        assert!(view.handle_wheel(0.0, 80.0));

        view.tx
            .send(BrowserViewLoadResult::HtmlChunk {
                load_id: 7,
                url: base,
                html: "<p>late</p>".to_string(),
            })
            .unwrap();

        let changed = view.drive_idle_for_test();
        assert!(changed, "scroll-priority idle should request a redraw");
        assert_eq!(
            view.streamed_html_len, before_len,
            "scroll-priority idle must present scroll before ingesting queued HTML"
        );

        // Scroll priority lasts until presentation, not just one idle callback.
        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(view.drive_idle_for_test());
        assert!(
            view.streamed_html_len > before_len,
            "the idle turn after presenting scroll must resume HTML ingestion"
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
        let doc = &view.stream_frame.as_ref().unwrap().doc;
        let paragraph_id = doc.query_selector("p").unwrap();
        assert!(
            doc.node_index.contains_key(&paragraph_id),
            "streamed nodes should be indexed before layout"
        );
        assert_eq!(doc.get_box_by_id(paragraph_id).unwrap().tag, "p");
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
        assert!(
            needs_wake,
            "pending resources should keep the browser loop alive"
        );
        assert!(
            !needs_redraw,
            "queued resources alone should not discard a valid painted frame"
        );
    }

    #[test]
    fn streamed_animation_clock_wakes_without_unconditional_repaint() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(base, "<!doctype html><body><p>Ready</p>");
        assert!(view.update_streamed_frame_before_paint());
        assert!(!view.stream_needs_layout);

        view.stream_frame
            .as_mut()
            .unwrap()
            .doc
            .needs_animation_frame = true;

        let (needs_wake, needs_redraw) = view.stream_idle_state();
        assert!(
            needs_wake,
            "active animations should keep the browser clock alive"
        );
        assert!(
            !needs_redraw,
            "the animation clock alone should not repaint a clean streamed frame"
        );
    }

    #[test]
    fn streamed_svg_animation_repaints_only_its_box() {
        let mut view = BrowserView::new(240.0, 160.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(240.0, 160.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            r#"<!doctype html><body style="margin:0">
            <svg width="40" height="40"><rect width="40" height="40" fill="red">
            <animate attributeName="fill" values="red;blue" dur="2s" repeatCount="indefinite"/>
            </rect></svg><div style="height:80px;background:green">Static content</div></body>"#,
        );
        view.stream_frame.as_mut().unwrap().finish_loading();
        assert!(view.update_streamed_frame_before_paint());
        let mut target = Pixmap::new(240, 160).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        let before = target.data().to_vec();
        fn advance(node: &mut crate::types::WebCore) {
            if node.svg_animation_start_time.is_some() {
                node.svg_animation_start_time =
                    Some(std::time::Instant::now() - std::time::Duration::from_secs(1));
            }
            for child in &mut node.children {
                advance(child);
            }
        }
        advance(&mut view.stream_frame.as_mut().unwrap().doc.root);
        assert!(view.update_streamed_frame_before_paint());
        assert!(view.renderer.paint_only_display_list_dirty_for_test());
        assert_eq!(view.renderer.dirty_paint_rect_count_for_test(), 1);
        view.paint_into(&mut target, 0, 0, 1.0);
        let center = (20 * 240 + 20) * 4;
        assert_ne!(
            &before[center..center + 4],
            &target.data()[center..center + 4]
        );
        assert_eq!(
            &before[60 * 240 * 4..],
            &target.data()[60 * 240 * 4..],
            "unchanged content below the animated SVG must retain its pixels"
        );
    }

    #[test]
    fn browser_view_poll_coalesces_ready_html_chunks() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/".to_string();
        view.load_id = 42;

        for idx in 0..12 {
            let html = if idx == 0 {
                "<!doctype html><body><p>0</p>".to_string()
            } else {
                format!("<p>{idx}</p>")
            };
            view.tx
                .send(BrowserViewLoadResult::HtmlChunk {
                    load_id: 42,
                    url: base.clone(),
                    html,
                })
                .unwrap();
        }

        let first_len = "<!doctype html><body><p>0</p>".len();
        let total_len = first_len
            + (1..12)
                .map(|idx| format!("<p>{idx}</p>").len())
                .sum::<usize>();

        let mut polls = 0;
        while view.streamed_html_len < total_len && polls < 12 {
            assert!(view.poll());
            assert!(view.stream_needs_layout);
            polls += 1;
        }
        assert_eq!(
            view.streamed_html_len, total_len,
            "ready chunks should reach layout within bounded parser turns"
        );
        assert!(view.update_streamed_frame_before_paint());
        assert!(!view.stream_needs_layout);
    }

    #[test]
    fn queued_html_defers_repeat_layout_but_not_first_paint_or_interaction() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        view.html_parse_backlog = true;
        assert!(!view.defer_queued_html_layout());
        view.stream_layout_committed = true;
        assert!(view.defer_queued_html_layout());
        view.interaction_layout_pending = true;
        assert!(!view.defer_queued_html_layout());
        view.interaction_layout_pending = false;
        view.html_parse_backlog = false;
        assert!(!view.defer_queued_html_layout());
    }

    #[test]
    fn browser_view_feeds_delta_chunks_without_chopping_tag_boundaries() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";

        view.feed_streaming_chunk(base, "<html><body><svg viewB");
        view.feed_streaming_chunk(
            base,
            r#"ox="0 0 24 24"><path d="M0 0h24v24H0z"/></svg><main><article>Post</article></main>"#,
        );

        let html = crate::html::serialize_html(&view.stream_frame.as_ref().unwrap().doc);
        assert!(
            html.contains("<article>Post</article>"),
            "delta chunks must advance the streamed body: {html}"
        );
        assert_eq!(
            html.matches("ox=&quot;0 0 24 24&quot;&gt;").count(),
            0,
            "split attributes must remain inside the tag, not leak as text: {html}"
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
    fn browser_view_mouse_move_recascades_ancestor_hover_dropdowns() {
        fn find<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
            if node.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(node);
            }
            node.children.iter().find_map(|child| find(child, id))
        }

        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let doc = view.renderer.load_html(
            r#"<style>
               body { margin: 0 }
               ul, li { margin: 0; padding: 0 }
               li { display: block; width: 180px; height: 40px }
               a { display: block; width: 180px; height: 40px }
               .panel { display: block; position: absolute; width: 200px; max-height: 0; overflow: hidden }
               .panel p { height: 60px; margin: 0 }
               li:hover .panel { max-height: 80px }
               </style>
               <ul><li id="item"><a id="trigger">Products</a><div id="panel" class="panel"><p>Firefox</p></div></li></ul>"#,
            480.0,
        );
        view.doc = Some(crate::browser::BrowserDocument::new(doc));

        let trigger_id = view
            .document()
            .unwrap()
            .get_element_by_id("trigger")
            .expect("trigger");
        let trigger_rect = view
            .document()
            .unwrap()
            .get_bounding_client_rect(trigger_id)
            .expect("trigger rect");
        assert!(
            find(&view.document().unwrap().root, "panel")
                .unwrap()
                .layout
                .border_rect
                .h
                < 1.0
        );

        assert!(
            view.handle_mouse_move(trigger_rect.x + 8.0, trigger_rect.y + 8.0),
            "moving over a child of an ancestor-hover trigger must request style work"
        );
        view.relayout();

        assert!(
            find(&view.document().unwrap().root, "panel")
                .unwrap()
                .layout
                .border_rect
                .h
                > 50.0,
            "real BrowserView mouse movement must open li:hover descendant panels"
        );
    }

    #[test]
    fn browser_view_mouse_move_prepares_hover_transition_frame_before_redraw() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let doc = view.renderer.load_html(
            r#"<style>
               body { margin: 0 }
               #outline {
                 display: block;
                 width: 120px;
                 height: 40px;
                 background-color: transparent;
                 color: #7c6af7;
                 transition: background-color 250ms linear, color 250ms linear;
               }
               #outline:hover {
                 background-color: #7c6af7;
                 color: white;
               }
               </style>
               <div id="outline">Outline</div>"#,
            480.0,
        );
        view.doc = Some(crate::browser::BrowserDocument::new(doc));
        let generation = view.document().unwrap().layout_generation;

        assert!(
            view.handle_mouse_move(10.0, 10.0),
            "hovering a transitioning element must request redraw"
        );
        assert!(view.interaction_layout_pending);
        assert_eq!(
            view.document().unwrap().layout_generation,
            generation,
            "hover event handlers should queue, not synchronously execute, layout"
        );
        let mut pixels = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut pixels, 0, 0, 1.0);
        assert!(!view.interaction_layout_pending);

        let doc = view.document().unwrap();
        let id = doc.get_element_by_id("outline").expect("outline");
        assert!(
            !doc.hover_changed,
            "BrowserView should consume hover style work at the requested paint boundary"
        );
        let states = doc
            .transition_states
            .get(&id)
            .expect("hover transition states");
        assert!(
            states
                .iter()
                .any(|state| state.property == "background-color"
                    && state.to_value == "rgba(124,106,247,1.0000)"),
            "background transition should target hovered purple"
        );
        assert!(
            states
                .iter()
                .any(|state| state.property == "color"
                    && state.to_value == "rgba(255,255,255,1.0000)"),
            "text color transition should target hovered white"
        );
        let first_frame = doc
            .animation_overrides
            .get(&id)
            .expect("first transition frame");
        assert!(
            first_frame
                .iter()
                .any(|(prop, value)| prop == "color" && value == "rgba(124,106,247,1.0000)"),
            "first requested redraw should paint the transition start, not stale hover/fallback text"
        );
    }

    #[test]
    fn streamed_browser_view_mouse_move_dirties_renderer_after_hover_transition_layout() {
        let mut view = BrowserView::new(480.0, 320.0, PageLoadOptions::default());
        let base = "https://example.test/";
        view.stream_frame = Some(EngineFrame::empty(480.0, 320.0));
        view.stream_frame.as_mut().unwrap().start_streaming(base);
        view.feed_streaming_chunk(
            base,
            r#"<html><head><style>
               body { margin: 0 }
               #outline {
                 display: block;
                 width: 120px;
                 height: 40px;
                 background-color: transparent;
                 color: #7c6af7;
                 transition: background-color 250ms linear, color 250ms linear;
               }
               #outline:hover {
                 background-color: #7c6af7;
                 color: white;
               }
               </style></head><body><div id="outline">Outline</div>"#,
        );
        view.stream_paint_ready = true;
        view.stream_needs_layout = true;
        assert!(view.update_streamed_frame_before_paint());

        let mut target = Pixmap::new(480, 320).unwrap();
        view.paint_into(&mut target, 0, 0, 1.0);
        assert!(
            !view.renderer.display_list_dirty_for_test(),
            "initial paint should consume the display-list dirty flag"
        );

        assert!(
            view.handle_mouse_move(10.0, 10.0),
            "hovering a streamed transitioning element must request redraw"
        );
        assert!(
            view.stream_needs_layout,
            "streamed hover should queue style/layout work for the browser frame loop"
        );
        assert!(
            view.update_streamed_frame_before_paint(),
            "the browser frame loop should consume queued hover style work"
        );

        let doc = view.document().unwrap();
        let id = doc.get_element_by_id("outline").expect("outline");
        assert!(
            doc.transition_states.contains_key(&id),
            "streamed hover should create transition states during the next frame update"
        );
        assert!(
            view.renderer.display_list_dirty_for_test(),
            "streamed hover layout must dirty the renderer so the requested redraw cannot reuse stale paint"
        );
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

        assert!(!view.take_document_navigation());

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
    fn browser_navigation_preserves_php_query_links_in_same_directory() {
        let base = "http://localhost/genie/";
        let view_url = "http://localhost/genie/";

        assert_eq!(
            resolve_browser_target("index.php?page=individuals", base, view_url),
            "http://localhost/genie/index.php?page=individuals"
        );
        assert_eq!(
            resolve_browser_target("?page=individuals", base, view_url),
            "http://localhost/genie/?page=individuals"
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
