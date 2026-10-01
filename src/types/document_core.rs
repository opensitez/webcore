//! `Document` construction, the node index, and node lookup.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::arena::DomArena;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

fn union_rect(a: Rect, b: Rect) -> Rect {
    if a.w <= 0.0 || a.h <= 0.0 {
        return b;
    }
    if b.w <= 0.0 || b.h <= 0.0 {
        return a;
    }
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.w).max(b.x + b.w);
    let y2 = (a.y + a.h).max(b.y + b.h);
    Rect::new(x1, y1, x2 - x1, y2 - y1)
}

fn for_each_webcore_mut_by_id(
    node: &mut WebCore,
    id: u32,
    f: &mut impl FnMut(&mut WebCore),
) -> bool {
    let mut found = false;
    if node.node_id == id {
        f(node);
        found = true;
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            found |= for_each_webcore_mut_by_id(child, id, f);
        }
    }
    for child in &mut node.children {
        found |= for_each_webcore_mut_by_id(child, id, f);
    }
    found
}

fn update_image_target_in_root(
    root: &mut WebCore,
    node_id: u32,
    path: &[usize],
    update: &mut impl FnMut(&mut WebCore),
) -> bool {
    if node_id == 0 {
        if let Some(node) = find_node_by_path_mut(root, path) {
            update(node);
            true
        } else {
            false
        }
    } else if let Some(node) = find_node_by_path_mut(root, path)
        && node.node_id == node_id
    {
        update(node);
        true
    } else {
        for_each_webcore_mut_by_id(root, node_id, update)
    }
}

fn find_child_path_by_id(node: &WebCore, id: u32, path: &mut Vec<usize>) -> bool {
    if node.node_id == id {
        return true;
    }
    for (index, child) in node.children.iter().enumerate() {
        path.push(index);
        if find_child_path_by_id(child, id, path) {
            return true;
        }
        path.pop();
    }
    false
}

fn mark_image_layout_path_dirty(root: &mut WebCore, node_id: u32, path: &[usize]) {
    if node_id == 0 || find_node_by_path_mut(root, path).is_some_and(|node| node.node_id == node_id)
    {
        mark_layout_path_dirty(root, path);
    } else {
        let mut current_path = Vec::new();
        if find_child_path_by_id(root, node_id, &mut current_path) {
            mark_layout_path_dirty(root, &current_path);
        } else {
            mark_layout_node_dirty(root);
        }
    }
}

impl Document {
    pub(crate) fn note_scroll_action(&mut self, id: u32) {
        let serial = self.scroll_action_serial.entry(id).or_default();
        *serial = serial.wrapping_add(1);
    }

    pub fn new() -> Self {
        Self {
            root: WebCore::new("html"),
            stylesheet: Stylesheet::default(),
            title: String::new(),
            arena: DomArena::new(),
            next_node_id: 1, // 0 = NodeId::NONE (reserved)
            node_index: HashMap::new(),
            kind: DocumentKind::Html,
            layout_store: crate::layout::layout_box::LayoutStore::new(),
            pending_nodes: HashMap::new(),
            base_url: String::new(),
            linked_stylesheets: Vec::new(),
            document_stylesheets: Vec::new(),
            inline_stylesheet_cache: HashMap::new(),
            dynamic_style_slots: HashMap::new(),
            loaded_linked_stylesheets: HashMap::new(),
            loaded_stylesheet_slots: HashMap::new(),
            preserve_stylesheet_document_order: true,
            editor: Editor::new(),
            canvas_surfaces: crate::canvas::CanvasSurfaces::default(),
            media_states: crate::types::MediaStateMap::new(),
            event_targets: crate::dom::events::EventTargetMap::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_action_serial: HashMap::new(),
            scrollbar_drag: None,
            resize_drag: None,
            hovered_box: 0,
            hover_suppress_count: 0,
            active_box: 0,
            focused_box: 0,
            mousedown_target: 0,
            last_click_target: 0,
            last_click_time: None,
            drag_source: 0,
            drag_start_doc_pt: (0.0, 0.0),
            drag_active: false,
            visited_urls: std::collections::HashSet::new(),
            custom_validity: HashMap::new(),
            doctype: 0,
            quirks: crate::html::doctype::QuirksMode::Quirks,
            character_set: "UTF-8".to_string(),
            traversals: crate::dom::traversal::TraversalStore::new(),
            ranges: crate::dom::range::RangeStore::new(),
            top_layer: Vec::new(),
            suppress_range_updates: false,
            viewport_w: 0.0,
            viewport_h: 0.0,
            device_pixel_ratio: 1.0,
            keyboard_focus: false,
            caret_blink_epoch: std::time::Instant::now(),
            open_select: 0,
            open_picker: 0,
            dropdown_hover_idx: -1,
            // Transient interaction state, like the two popups beside it: a
            // fresh document is holding nothing.
            dragging_range: 0,
            range_drag_origin: String::new(),
            on_form_event: None,
            on_navigate: None,
            on_title_change: None,
            on_dom_mutation: None,
            on_visibility_change: None,
            active_animations: Vec::new(),
            transition_states: HashMap::new(),
            prev_styles: HashMap::new(),
            transition_style_refs: HashMap::new(),
            animation_overrides: HashMap::new(),
            needs_animation_frame: false,
            smooth_scrolls: Vec::new(),
            hover_changed: false,
            hover_sensitive_nodes: HashSet::new(),
            // A new document has never had the author/UA cascade applied.
            // Reused renderers keep layout-engine cache state across pages, so
            // the document itself must force its first cascade even when the
            // viewport is unchanged and no media query boundary moved.
            style_dirty: true,
            prev_hovered_box: 0,
            pending_announcements: Vec::new(),
            live_region_snapshots: HashMap::new(),
            live_regions_initialized: false,
            layout_generation: 0,
            scroll_height_cache: std::cell::Cell::new(None),
            scroll_width_cache: std::cell::Cell::new(None),
            pending_images: None,
            image_load_errors: Vec::new(),
            images_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pending_stylesheets: None,
            pending_stylesheet_base: None,
        }
    }

    pub(crate) fn has_dirty_layout(&self) -> bool {
        fn walk(node: &WebCore) -> bool {
            node.layout.layout_dirty
                || node.has_dirty_layout_descendant
                || node.children.iter().any(walk)
                || node
                    .shadow_root
                    .as_ref()
                    .is_some_and(|shadow| shadow.children.iter().any(walk))
        }

        walk(&self.root)
    }

    pub fn animation_overrides_for(&self, element_id: u32) -> Option<&[(String, String)]> {
        self.animation_overrides.get(&element_id).map(Vec::as_slice)
    }

    /// Poll for linked stylesheets that arrived from background fetch threads.
    /// Returns true if the stylesheet changed and the caller should re-layout.
    pub fn poll_pending_stylesheets(&mut self) -> bool {
        self.poll_pending_stylesheets_budgeted(usize::MAX, std::time::Duration::from_secs(60))
    }

    pub(crate) fn add_streamed_inline_stylesheet(&mut self, css: String, media: String) -> bool {
        let source: std::sync::Arc<str> = css.into();
        let slot = self.document_stylesheets.len();
        let active = crate::css::evaluate_media(&media, self.viewport_w, self.viewport_h);
        self.document_stylesheets.push(DocumentStylesheet::Inline {
            css: source.clone(),
            media,
        });
        if active {
            let sheet = crate::parsed_inline_stylesheet(&source, &self.base_url);
            self.stylesheet.append_fragment((*sheet).clone());
            self.inline_stylesheet_cache.insert(
                slot,
                CachedInlineStylesheet {
                    source,
                    base_url: self.base_url.clone(),
                    sheet,
                },
            );
        }
        active
    }

    /// Bring DOM-created `<style>` elements into the author cascade after a
    /// tree or text mutation. Parser-created sheets already have their slots.
    pub(crate) fn sync_dynamic_style_sheets(&mut self) {
        let ids: Vec<u32> = self.dynamic_style_slots.keys().copied().collect();
        let mut changed = false;
        for id in ids {
            let connected = self.is_connected(id);
            let css = if connected {
                self.text_content(id)
            } else {
                String::new()
            };
            let media = if connected {
                self.get_attribute(id, "media").unwrap_or_default()
            } else {
                String::new()
            };
            let slot = match self.dynamic_style_slots[&id] {
                Some(slot) => slot,
                None if connected => {
                    let slot = self.document_stylesheets.len();
                    self.dynamic_style_slots.insert(id, Some(slot));
                    self.document_stylesheets.push(DocumentStylesheet::Inline {
                        css: std::sync::Arc::from(""),
                        media: String::new(),
                    });
                    slot
                }
                None => continue,
            };
            let current = &self.document_stylesheets[slot];
            if matches!(current, DocumentStylesheet::Inline { css: old_css, media: old_media }
                if old_css.as_ref() == css && old_media == &media)
            {
                continue;
            }
            self.document_stylesheets[slot] = DocumentStylesheet::Inline {
                css: css.into(),
                media,
            };
            changed = true;
        }
        if changed {
            self.rebuild_author_stylesheet_from_document_order();
            self.stylesheet
                .resolve_variables_for_viewport(self.viewport_w, self.viewport_h);
            self.stylesheet.rebuild_index();
            self.style_dirty = true;
        }
    }

    /// Poll pending stylesheet work. A zero `max_time` means "drain everything
    /// already queued without waiting", which is the browser-frame path: worker
    /// threads parse/fetch independently, and the UI coalesces all currently
    /// available changes into a single cascade/layout pass.
    pub fn poll_pending_stylesheets_budgeted(
        &mut self,
        max_sheets: usize,
        max_time: std::time::Duration,
    ) -> bool {
        let Some(rx) = self.pending_stylesheets.take() else {
            return false;
        };
        if max_sheets == 0 {
            self.pending_stylesheets = Some(rx);
            return false;
        }
        let rebuild_from_document_order =
            self.preserve_stylesheet_document_order && !self.document_stylesheets.is_empty();
        let start = std::time::Instant::now();
        let time_limited = !max_time.is_zero();
        let mut disconnected = false;
        let mut changed = false;
        let mut rebuild_needed = false;
        let mut variables_changed = false;
        let mut changed_urls = HashSet::new();
        if rebuild_from_document_order {
            let mut results = Vec::new();
            loop {
                match rx.try_recv() {
                    Ok(item) => results.push(item),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
                if results.len() >= max_sheets || (time_limited && start.elapsed() >= max_time) {
                    break;
                }
            }
            results.sort_by_key(|update| update.slot);
            for update in results {
                let crate::types::PendingStylesheetResult {
                    slot: idx,
                    url: css_url,
                    sheet,
                    kind,
                    ..
                } = update;
                let replace = kind == crate::types::StylesheetUpdateKind::Replace;
                changed_urls.insert(css_url.clone());
                variables_changed |= sheet.may_change_root_variables();
                if replace {
                    variables_changed |= self
                        .loaded_stylesheet_slots
                        .get(&idx)
                        .is_some_and(crate::css::Stylesheet::may_change_root_variables);
                }
                let append_in_order = !replace
                    && !rebuild_needed
                    && self.can_append_linked_stylesheet_fragment(idx, &css_url);
                if append_in_order {
                    self.stylesheet.append_fragment(sheet.clone());
                } else {
                    rebuild_needed = true;
                }
                if replace {
                    self.loaded_stylesheet_slots.insert(idx, sheet.clone());
                    self.loaded_linked_stylesheets.insert(css_url, sheet);
                } else {
                    self.loaded_stylesheet_slots
                        .entry(idx)
                        .and_modify(|existing| existing.append_fragment(sheet.clone()))
                        .or_insert_with(|| sheet.clone());
                    self.loaded_linked_stylesheets
                        .entry(css_url)
                        .and_modify(|existing| existing.append_fragment(sheet.clone()))
                        .or_insert(sheet);
                }
                changed = true;
            }
        } else {
            let mut processed = 0usize;
            let mut last_slot = self.loaded_stylesheet_slots.keys().copied().max();
            loop {
                match rx.try_recv() {
                    Ok(update) => {
                        let crate::types::PendingStylesheetResult {
                            slot, sheet, kind, ..
                        } = update;
                        if self.pending_stylesheet_base.is_none() {
                            self.pending_stylesheet_base = Some(self.stylesheet.clone());
                        }
                        let replace = kind == crate::types::StylesheetUpdateKind::Replace;
                        if replace {
                            variables_changed |= self
                                .loaded_stylesheet_slots
                                .get(&slot)
                                .is_some_and(crate::css::Stylesheet::may_change_root_variables);
                        }
                        rebuild_needed |= replace || last_slot.is_some_and(|last| slot < last);
                        last_slot = Some(last_slot.map_or(slot, |last| last.max(slot)));
                        variables_changed |= sheet.may_change_root_variables();
                        if !rebuild_needed {
                            self.stylesheet.append_fragment(sheet.clone());
                        }
                        if replace {
                            self.loaded_stylesheet_slots.insert(slot, sheet);
                        } else {
                            self.loaded_stylesheet_slots
                                .entry(slot)
                                .and_modify(|existing| existing.append_fragment(sheet.clone()))
                                .or_insert(sheet);
                        }
                        processed += 1;
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
                if processed >= max_sheets || (time_limited && start.elapsed() >= max_time) {
                    break;
                }
            }
        }
        if !disconnected {
            self.pending_stylesheets = Some(rx);
        }
        if !changed {
            return false;
        }
        if rebuild_from_document_order && rebuild_needed {
            self.rebuild_author_stylesheet_from_document_order();
        } else if rebuild_needed {
            if let Some(base) = &self.pending_stylesheet_base {
                self.stylesheet = base.clone();
                let mut slots: Vec<_> = self.loaded_stylesheet_slots.iter().collect();
                slots.sort_by_key(|(idx, _)| **idx);
                for (_, sheet) in slots {
                    self.stylesheet.append_fragment(sheet.clone());
                }
            }
        }
        if rebuild_needed || variables_changed {
            self.stylesheet
                .resolve_variables_for_viewport(self.viewport_w, self.viewport_h);
        }
        self.stylesheet.rebuild_index();
        if !changed_urls.is_empty() {
            self.refresh_shadow_linked_stylesheets_for(&changed_urls);
        }
        self.style_dirty = true;
        true
    }

    fn can_append_linked_stylesheet_fragment(&self, idx: usize, css_url: &str) -> bool {
        let Some(DocumentStylesheet::Linked { href, media }) = self.document_stylesheets.get(idx)
        else {
            return false;
        };
        if !crate::css::evaluate_media(media, self.viewport_w, self.viewport_h)
            || crate::html::resolve_url(href, &self.base_url) != css_url
        {
            return false;
        }
        for (later_idx, later) in self.document_stylesheets.iter().enumerate() {
            if let DocumentStylesheet::Linked { href, .. } = later {
                if later_idx != idx && crate::html::resolve_url(href, &self.base_url) == css_url {
                    return false;
                }
            }
            if later_idx <= idx {
                continue;
            }
            match later {
                DocumentStylesheet::Inline { media, .. }
                    if crate::css::evaluate_media(media, self.viewport_w, self.viewport_h) =>
                {
                    return false;
                }
                DocumentStylesheet::Linked { href, media }
                    if crate::css::evaluate_media(media, self.viewport_w, self.viewport_h)
                        && (self.loaded_stylesheet_slots.contains_key(&later_idx)
                            || self
                                .loaded_linked_stylesheets
                                .contains_key(&crate::html::resolve_url(href, &self.base_url))) =>
                {
                    return false;
                }
                _ => {}
            }
        }
        true
    }

    fn rebuild_author_stylesheet_from_document_order(&mut self) {
        self.stylesheet = crate::css::ua_stylesheet();
        for (idx, ds) in self.document_stylesheets.iter().enumerate() {
            match ds {
                DocumentStylesheet::Inline { css, media } => {
                    if !crate::css::evaluate_media(media, self.viewport_w, self.viewport_h) {
                        continue;
                    }
                    let cached = self
                        .inline_stylesheet_cache
                        .get(&idx)
                        .is_some_and(|cached| {
                            cached.base_url == self.base_url
                                && (std::sync::Arc::ptr_eq(&cached.source, css)
                                    || cached.source.as_ref() == css.as_ref())
                        });
                    if !cached {
                        let sheet = crate::parsed_inline_stylesheet(css, &self.base_url);
                        self.inline_stylesheet_cache.insert(
                            idx,
                            CachedInlineStylesheet {
                                source: css.clone(),
                                base_url: self.base_url.clone(),
                                sheet,
                            },
                        );
                    }
                    self.stylesheet
                        .append_fragment((*self.inline_stylesheet_cache[&idx].sheet).clone());
                }
                DocumentStylesheet::Linked { href, media } => {
                    if !crate::css::evaluate_media(media, self.viewport_w, self.viewport_h) {
                        continue;
                    }
                    let abs = crate::html::resolve_url(href, &self.base_url);
                    if let Some(sheet) = self
                        .loaded_stylesheet_slots
                        .get(&idx)
                        .or_else(|| self.loaded_linked_stylesheets.get(&abs))
                    {
                        self.stylesheet.append_fragment(sheet.clone());
                    }
                }
            }
        }
    }

    pub(crate) fn reevaluate_stylesheet_media(&mut self, viewport_w: f32, viewport_h: f32) {
        self.viewport_w = viewport_w;
        self.viewport_h = viewport_h;
        if self.preserve_stylesheet_document_order {
            self.rebuild_author_stylesheet_from_document_order();
            self.stylesheet.rebuild_index();
        }
        self.stylesheet
            .resolve_variables_for_viewport(viewport_w, viewport_h);
        self.refresh_shadow_linked_stylesheets();
        self.style_dirty = true;
    }

    pub(crate) fn refresh_shadow_linked_stylesheets(&mut self) -> bool {
        self.refresh_shadow_linked_stylesheets_matching(None)
    }

    fn refresh_shadow_linked_stylesheets_for(&mut self, urls: &HashSet<String>) -> bool {
        self.refresh_shadow_linked_stylesheets_matching(Some(urls))
    }

    fn refresh_shadow_linked_stylesheets_matching(
        &mut self,
        changed_urls: Option<&HashSet<String>>,
    ) -> bool {
        fn signature(sheet: &crate::css::Stylesheet) -> (usize, usize, usize, usize, usize) {
            (
                sheet.rules.len(),
                sheet.font_faces.len(),
                sheet.keyframes.len(),
                sheet.counter_styles.len(),
                sheet.source_count,
            )
        }

        fn refresh_node(
            node: &mut WebCore,
            base_url: &str,
            loaded_linked: &HashMap<String, crate::css::Stylesheet>,
            viewport_w: f32,
            viewport_h: f32,
            changed_urls: Option<&HashSet<String>>,
        ) -> bool {
            let mut changed = false;
            if let Some(sr) = node.shadow_root.as_mut() {
                let needs_rebuild = if let Some(urls) = changed_urls {
                    sr.document_stylesheets.iter().any(|sheet| {
                        matches!(sheet, DocumentStylesheet::Linked { href, .. }
                            if urls.contains(&crate::html::resolve_url(href, base_url)))
                    })
                } else {
                    !sr.document_stylesheets.is_empty() || !sr.adopted_stylesheets.is_empty()
                };
                if needs_rebuild {
                    let before = signature(&sr.stylesheet);
                    let mut stylesheet = crate::css::ua_stylesheet();
                    for ds in &sr.document_stylesheets {
                        match ds {
                            DocumentStylesheet::Inline { css, media } => {
                                if !crate::css::evaluate_media(media, viewport_w, viewport_h) {
                                    continue;
                                }
                                stylesheet.parse_and_add_author(css);
                            }
                            DocumentStylesheet::Linked { href, media } => {
                                if !crate::css::evaluate_media(media, viewport_w, viewport_h) {
                                    continue;
                                }
                                let abs = crate::html::resolve_url(href, base_url);
                                if let Some(sheet) =
                                    loaded_linked.get(&abs).or_else(|| loaded_linked.get(href))
                                {
                                    stylesheet.append_fragment(sheet.clone());
                                }
                            }
                        }
                    }
                    for css in &sr.adopted_stylesheets {
                        stylesheet.parse_and_add_author(css);
                    }
                    stylesheet.resolve_variables_for_viewport(viewport_w, viewport_h);
                    stylesheet.rebuild_index();
                    let after = signature(&stylesheet);
                    if before != after || sr.stylesheet.counter_styles != stylesheet.counter_styles
                    {
                        changed = true;
                    }
                    sr.stylesheet = stylesheet;
                }
                for child in &mut sr.children {
                    changed |= refresh_node(
                        child,
                        base_url,
                        loaded_linked,
                        viewport_w,
                        viewport_h,
                        changed_urls,
                    );
                }
            }
            for child in &mut node.children {
                changed |= refresh_node(
                    child,
                    base_url,
                    loaded_linked,
                    viewport_w,
                    viewport_h,
                    changed_urls,
                );
            }
            changed
        }

        let base_url = self.base_url.clone();
        let loaded_linked = &self.loaded_linked_stylesheets;
        refresh_node(
            &mut self.root,
            &base_url,
            loaded_linked,
            self.viewport_w,
            self.viewport_h,
            changed_urls,
        )
    }

    /// Poll for images that arrived from background fetch threads.
    /// Returns whether pixels changed and whether intrinsic sizing changed
    /// enough to require layout. Most late image arrivals should repaint only:
    /// dimensions are often already known from attributes, srcset hints, or SVG
    /// metadata, and relaying out the entire document for those is brutal on
    /// image-heavy pages.
    pub fn poll_pending_images_detailed(&mut self) -> PendingImagePoll {
        self.poll_pending_images_budgeted(usize::MAX, std::time::Duration::from_secs(60))
    }

    /// Poll image decode/fetch results. A zero `max_time` drains the currently
    /// queued results without blocking; resource workers keep decoding in the
    /// background and the next frame picks up whatever arrived meanwhile.
    pub fn poll_pending_images_budgeted(
        &mut self,
        max_images: usize,
        max_time: std::time::Duration,
    ) -> PendingImagePoll {
        let rx = match self.pending_images.take() {
            Some(rx) => rx,
            None => return PendingImagePoll::default(),
        };
        if max_images == 0 {
            self.pending_images = Some(rx);
            return PendingImagePoll::default();
        }
        let mut poll = PendingImagePoll::default();
        let start = std::time::Instant::now();
        let time_limited = !max_time.is_zero();
        let mut processed = 0usize;
        let mut queue_drained = false;
        loop {
            let Ok(result) = rx.try_recv() else {
                queue_drained = true;
                break;
            };
            let (node_id, path, target, url, decoded) = match result {
                PendingImageResult::Dimensions {
                    node_id,
                    path,
                    target,
                    width,
                    height,
                } => {
                    if target == PendingImageTarget::Element {
                        let mut needs_relayout = false;
                        let mut update = |node: &mut WebCore| {
                            if node.image_width != width || node.image_height != height {
                                node.image_width = width;
                                node.image_height = height;
                                needs_relayout |=
                                    node.style.width.is_auto() || node.style.height.is_auto();
                            }
                        };
                        let root_updated = update_image_target_in_root(
                            &mut self.root,
                            node_id,
                            &path,
                            &mut update,
                        );
                        if node_id != 0 {
                            for pending in self.pending_nodes.values_mut() {
                                for_each_webcore_mut_by_id(pending, node_id, &mut update);
                            }
                        }
                        if needs_relayout {
                            if root_updated {
                                mark_image_layout_path_dirty(&mut self.root, node_id, &path);
                            }
                            poll.needs_relayout = true;
                            poll.loaded_any = true;
                        }
                    }
                    processed += 1;
                    if processed >= max_images || (time_limited && start.elapsed() >= max_time) {
                        break;
                    }
                    continue;
                }
                PendingImageResult::Loaded {
                    node_id,
                    path,
                    target,
                    url,
                    decoded,
                } => (node_id, path, target, url, decoded),
                PendingImageResult::Failed {
                    node_id,
                    path,
                    target,
                    url,
                    error,
                } => {
                    self.image_load_errors.push((path, target, url, error));
                    let _ = node_id;
                    processed += 1;
                    if processed >= max_images || (time_limited && start.elapsed() >= max_time) {
                        break;
                    }
                    continue;
                }
            };
            let mut loaded_target = false;
            let mut target_needs_relayout = false;
            let mut paint_rect = None;
            let loaded_path = path.clone();
            let loaded_target_kind = target;
            let base_url = self.base_url.clone();
            let device_pixel_ratio = self.device_pixel_ratio;
            let mut apply_to_node = |node: &mut WebCore| {
                paint_rect = Some(match paint_rect {
                    Some(existing) => union_rect(existing, node.layout.border_rect),
                    None => node.layout.border_rect,
                });
                match target {
                    PendingImageTarget::Element | PendingImageTarget::ElementFallback => {
                        if matches!(target, PendingImageTarget::ElementFallback)
                            && node.image_data.is_some()
                        {
                            return;
                        }
                        let old_size = (node.image_width, node.image_height);
                        let intrinsic_size_controls_layout =
                            node.style.width.is_auto() || node.style.height.is_auto();
                        crate::html::set_decoded_image_on_node(node, decoded.clone());
                        target_needs_relayout |= intrinsic_size_controls_layout
                            && old_size != (node.image_width, node.image_height);
                        loaded_target = true;
                    }
                    PendingImageTarget::Background => {
                        let selected = node.style.background_image_url_for_dpr(device_pixel_ratio);
                        let expected = crate::html::resolve_url(&selected, &base_url);
                        if selected.is_empty()
                            || (url != expected
                                && (node.bg_image_data.is_some()
                                    || node.style.rare().background_image_set_source.is_none()))
                        {
                            return;
                        }
                        if crate::html::set_decoded_bg_image_for_url_on_node(
                            node,
                            decoded.clone(),
                            &url,
                            &base_url,
                        ) {
                            loaded_target = true;
                        }
                    }
                    PendingImageTarget::BackgroundLayer(layer_index) => {
                        let Some(layer) = node
                            .style
                            .rare()
                            .additional_background_layers
                            .get(layer_index)
                        else {
                            return;
                        };
                        let selected = layer.image_url_for_dpr(device_pixel_ratio);
                        let expected = crate::html::resolve_url(&selected, &base_url);
                        let loaded = node
                            .additional_bg_images
                            .get(layer_index)
                            .and_then(|image| image.as_ref())
                            .is_some();
                        if selected.is_empty()
                            || (url != expected && (loaded || layer.image_set_source.is_none()))
                        {
                            return;
                        }
                        if crate::html::set_decoded_bg_image_layer_for_url_on_node(
                            node,
                            layer_index,
                            decoded.clone(),
                            &url,
                            &base_url,
                        ) {
                            loaded_target = true;
                        }
                    }
                    PendingImageTarget::Mask => {
                        let selected = node.style.mask_image_url_for_dpr(device_pixel_ratio);
                        let expected = crate::html::resolve_url(&selected, &base_url);
                        if selected.is_empty()
                            || (url != expected
                                && (node.mask_image_data.is_some()
                                    || node.style.rare().mask_image_set_source.is_none()))
                        {
                            return;
                        }
                        if let Some((data, w, h)) =
                            crate::html::decoded_image_pixels_arc(decoded.clone())
                        {
                            node.mask_image_data = Some(data);
                            node.mask_image_width = w;
                            node.mask_image_height = h;
                            loaded_target = true;
                        }
                    }
                }
            };
            let root_updated =
                update_image_target_in_root(&mut self.root, node_id, &path, &mut apply_to_node);
            if node_id != 0 {
                for pending in self.pending_nodes.values_mut() {
                    for_each_webcore_mut_by_id(pending, node_id, &mut apply_to_node);
                }
            }
            if loaded_target {
                self.image_load_errors
                    .retain(|(err_path, err_target, _, _)| {
                        err_path != &loaded_path || *err_target != loaded_target_kind
                    });
                if target_needs_relayout {
                    if root_updated {
                        mark_image_layout_path_dirty(&mut self.root, node_id, &path);
                    }
                    poll.needs_relayout = true;
                } else if let Some(rect) = paint_rect
                    && rect.w > 0.0
                    && rect.h > 0.0
                {
                    poll.paint_rects.push(rect);
                }
                poll.loaded_any = true;
            }
            processed += 1;
            if processed >= max_images || (time_limited && start.elapsed() >= max_time) {
                break;
            }
        }
        if !queue_drained
            || self
                .images_in_flight
                .load(std::sync::atomic::Ordering::SeqCst)
                != 0
        {
            self.pending_images = Some(rx);
        }
        poll
    }

    /// Compatibility wrapper for callers that only need a yes/no signal.
    pub fn poll_pending_images(&mut self) -> bool {
        self.poll_pending_images_detailed().loaded_any
    }

    /// Advance animated image frames. Returns true when pixels changed and the
    /// caller should repaint.
    pub fn tick_animated_images(&mut self, now: std::time::Instant) -> bool {
        self.tick_animated_images_detailed(now).changed_any
    }

    pub fn tick_animated_images_detailed(&mut self, now: std::time::Instant) -> AnimatedImageTick {
        self.tick_animated_images_in_viewport_detailed(now, 0.0, f32::INFINITY)
    }

    /// Advance animated images that intersect the current viewport. Offscreen
    /// animations should not force full-window repaints while the user is not
    /// looking at them.
    pub fn tick_animated_images_in_viewport(
        &mut self,
        now: std::time::Instant,
        scroll_y: f32,
        viewport_h: f32,
    ) -> bool {
        self.tick_animated_images_in_viewport_detailed(now, scroll_y, viewport_h)
            .changed_any
    }

    pub fn tick_animated_images_in_viewport_detailed(
        &mut self,
        now: std::time::Instant,
        scroll_y: f32,
        viewport_h: f32,
    ) -> AnimatedImageTick {
        fn tick_node(
            node: &mut WebCore,
            now: std::time::Instant,
            scroll_y: f32,
            viewport_h: f32,
            inherited_visible: bool,
            clip_top: f32,
            clip_bottom: f32,
            tick: &mut AnimatedImageTick,
        ) {
            let visible = inherited_visible && !animated_image_hidden_by_style(node);
            if !visible {
                collapse_animated_subtree(node);
                return;
            }
            let intersects =
                animated_image_intersects_clip(node, scroll_y, viewport_h, clip_top, clip_bottom);
            if intersects {
                if let Some(animated) = node.animated_image.as_mut() {
                    if animated.source_bytes.is_some() && !animated.fully_decoded {
                        let target_width = node.layout.border_rect.w.ceil().max(1.0) as u32;
                        let target_height = node.layout.border_rect.h.ceil().max(1.0) as u32;
                        if crate::html::poll_animated_image_expansion(
                            animated,
                            target_width,
                            target_height,
                        ) {
                            node.animated_image_frame = 0;
                            node.animated_image_last_tick = Some(now);
                            node.image_data =
                                animated.frames.first().map(|frame| frame.pixels.clone());
                            node.image_data_width = animated.width;
                            node.image_data_height = animated.height;
                            tick.changed_any = true;
                            let rect = node.layout.border_rect;
                            if rect.w > 0.0 && rect.h > 0.0 {
                                tick.paint_rects.push(rect);
                            }
                        }
                    }
                    if animated.frames.len() > 1 {
                        let current = node
                            .animated_image_frame
                            .min(animated.frames.len().saturating_sub(1));
                        let last = node.animated_image_last_tick.unwrap_or(now);
                        if let Some((next, frame_start)) =
                            advance_animated_image_frame(animated, current, last, now)
                        {
                            node.animated_image_frame = next;
                            node.animated_image_last_tick = Some(frame_start);
                            if next != current {
                                node.image_data = Some(animated.frames[next].pixels.clone());
                                node.image_data_width = animated.width;
                                node.image_data_height = animated.height;
                                tick.changed_any = true;
                                let rect = node.layout.border_rect;
                                if rect.w > 0.0 && rect.h > 0.0 {
                                    tick.paint_rects.push(rect);
                                }
                            }
                        } else if node.animated_image_last_tick.is_none() {
                            node.animated_image_last_tick = Some(now);
                        }
                    }
                }
            } else if collapse_node_animation(node) {
                tick.changed_any = true;
            }
            let (child_clip_top, child_clip_bottom) =
                animated_child_clip(node, scroll_y, viewport_h, clip_top, clip_bottom);
            if child_clip_bottom < child_clip_top {
                return;
            }
            for child in &mut node.children {
                tick_node(
                    child,
                    now,
                    scroll_y,
                    viewport_h,
                    visible,
                    child_clip_top,
                    child_clip_bottom,
                    tick,
                );
            }
        }

        let clip_top = scroll_y;
        let clip_bottom = scroll_y + viewport_h;
        let mut tick = AnimatedImageTick::default();
        tick_node(
            &mut self.root,
            now,
            scroll_y,
            viewport_h,
            true,
            clip_top,
            clip_bottom,
            &mut tick,
        );
        tick
    }

    pub fn has_animated_images(&self) -> bool {
        has_animated_images(&self.root)
    }

    pub fn has_visible_animated_images(&self, scroll_y: f32, viewport_h: f32) -> bool {
        has_visible_animated_images(&self.root, scroll_y, viewport_h)
    }

    pub fn next_visible_animated_image_deadline(
        &self,
        now: std::time::Instant,
        scroll_y: f32,
        viewport_h: f32,
    ) -> Option<std::time::Instant> {
        next_visible_animated_image_deadline(&self.root, now, scroll_y, viewport_h)
    }

    /// Rebuild the O(1) node index by walking the tree and storing pointers.
    /// Called after layout (tree structure is stable until next mutation).
    pub fn rebuild_node_index(&mut self) {
        self.node_index.clear();
        fn collect(node: &WebCore, path: &mut Vec<u32>, map: &mut HashMap<u32, Vec<u32>>) {
            if node.node_id != 0 {
                map.insert(node.node_id, path.clone());
            }
            for (i, child) in node.children.iter().enumerate() {
                path.push(i as u32);
                collect(child, path, map);
                path.pop();
            }
        }
        let mut path = Vec::new();
        collect(&self.root, &mut path, &mut self.node_index);
    }

    /// Backward-compat alias.
    pub fn rebuild_node_map(&mut self) {
        self.rebuild_node_index();
    }

    /// O(1) node lookup by node_id. Uses the cached pointer index.
    /// Falls back to tree walk if index is empty (not yet built).
    #[inline]
    pub fn get_box_by_id(&self, node_id: u32) -> Option<&WebCore> {
        if node_id == 0 {
            return None;
        }
        // Follow the cached path. O(depth) rather than O(1), and no `unsafe`:
        // a stale path leads somewhere wrong, which the id check below
        // rejects, where a stale POINTER was undefined behaviour.
        if let Some(path) = self.node_index.get(&node_id) {
            let mut cur = &self.root;
            let mut ok = true;
            for step in path {
                match cur.children.get(*step as usize) {
                    Some(next) => cur = next,
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && cur.node_id == node_id {
                return Some(cur);
            }
        }
        // Fallback: tree walk (index not built yet)
        fn walk(node: &WebCore, id: u32) -> Option<&WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            if let Some(shadow) = node.shadow_root.as_ref() {
                for child in &shadow.children {
                    if let Some(f) = walk(child, id) {
                        return Some(f);
                    }
                }
            }
            for child in &node.children {
                if let Some(f) = walk(child, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&self.root, node_id)
    }

    /// Same as get_box_by_id — O(1) when index is built.
    #[inline]
    pub fn get_node(&self, node_id: u32) -> Option<&WebCore> {
        self.get_box_by_id(node_id)
    }

    /// O(1) mutable node lookup via tree walk (arena stores clones, not references).
    /// For mutable access, we must use the tree since the arena is a snapshot.
    pub fn get_box_by_id_mut(&mut self, node_id: u32) -> Option<&mut WebCore> {
        if node_id == 0 {
            return None;
        }
        fn walk(node: &mut WebCore, id: u32) -> Option<&mut WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            if let Some(shadow) = node.shadow_root.as_mut() {
                for child in &mut shadow.children {
                    if let Some(f) = walk(child, id) {
                        return Some(f);
                    }
                }
            }
            for child in &mut node.children {
                if let Some(f) = walk(child, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&mut self.root, node_id)
    }

    /// Allocate the next node_id (for dynamically created nodes outside the parser).
    pub fn alloc_node_id(&mut self) -> u32 {
        let id = self.next_node_id;
        self.next_node_id += 1;
        id
    }

    /// Walk all boxes in depth-first order.
    pub fn walk_all<F: FnMut(&WebCore)>(root: &WebCore, f: &mut F) {
        f(root);
        if let Some(shadow) = root.shadow_root.as_ref() {
            for child in &shadow.children {
                Self::walk_all(child, f);
            }
        }
        for child in &root.children {
            Self::walk_all(child, f);
        }
    }

    pub fn walk_all_mut<F: FnMut(&mut WebCore)>(root: &mut WebCore, f: &mut F) {
        f(root);
        if let Some(shadow) = root.shadow_root.as_mut() {
            for child in &mut shadow.children {
                Self::walk_all_mut(child, f);
            }
        }
        for child in &mut root.children {
            Self::walk_all_mut(child, f);
        }
    }

    pub(crate) fn viewport_overflow_body(root: &WebCore) -> Option<u32> {
        let html = if root.tag == "html" {
            root
        } else {
            root.effective_children()
                .iter()
                .find(|child| child.tag == "html")?
        };
        if html.style.overflow_x != Overflow::Visible || html.style.overflow_y != Overflow::Visible
        {
            return None;
        }
        html.effective_children()
            .iter()
            .find(|child| child.tag == "body")
            .map(|body| body.node_id)
    }

    /// Compute the full scrollable extent of the document.
    /// Walks all elements and returns the maximum bottom/right edge,
    /// ignoring containers with `height: 100vh` or similar constraints.
    pub fn scroll_height(root: &WebCore) -> f32 {
        fn walk_scroll(
            node: &WebCore,
            max_bottom: &mut f32,
            is_root: bool,
            inside_svg_foreign_content: bool,
            viewport_overflow_body: Option<u32>,
        ) {
            if inside_svg_foreign_content {
                return;
            }
            if matches!(node.style.display, Display::None) {
                return;
            }
            if crate::layout::is_layout_inert_svg_node(node) {
                return;
            }
            // Fixed elements don't contribute to scroll height
            if matches!(node.style.position, Position::Fixed) {
                return;
            }
            let viewport_overflow = viewport_overflow_body == Some(node.node_id);
            // Absolute elements contribute only if they're within the document flow area
            // (some abs elements are positioned far off-screen as accessibility hacks)
            let contributes_own_scroll_extent = !matches!(node.style.display, Display::Contents)
                && !(node.is_text_node() && node.text.trim().is_empty());

            if matches!(node.style.position, Position::Absolute) {
                if !contributes_own_scroll_extent {
                    for child in &node.children {
                        walk_scroll(
                            child,
                            max_bottom,
                            false,
                            node.tag == "svg"
                                && crate::layout::is_svg_foreign_content_tag(&child.tag),
                            viewport_overflow_body,
                        );
                    }
                    return;
                }
                // A tall positioned page starting near the origin contributes its full height.
                // The top-edge limit only excludes far-offscreen accessibility content.
                let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
                if node.layout.margin_rect.h > 0.0
                    && bottom > 0.0
                    && node.layout.margin_rect.y < *max_bottom * 3.0 + 2000.0
                {
                    if bottom > *max_bottom {
                        *max_bottom = bottom;
                    }
                } else {
                    return;
                }
            } else
            // The root box height is overwritten with the previous scroll
            // extent after layout, so counting it here creates a ratchet:
            // pages can grow but never shrink when display/content collapses.
            // Use descendants to compute the natural document extent instead.
            if !is_root && contributes_own_scroll_extent {
                if node.layout.margin_rect.h <= 0.0 && !viewport_overflow {
                    return;
                }
                let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
                if bottom > *max_bottom {
                    *max_bottom = bottom;
                }
            }
            if !is_root
                && !viewport_overflow
                && matches!(
                    node.style.overflow_y,
                    Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
                )
            {
                return;
            }
            for child in &node.children {
                walk_scroll(
                    child,
                    max_bottom,
                    false,
                    node.tag == "svg" && crate::layout::is_svg_foreign_content_tag(&child.tag),
                    viewport_overflow_body,
                );
            }
        }
        let mut max_bottom = 0.0;
        walk_scroll(
            root,
            &mut max_bottom,
            true,
            false,
            Self::viewport_overflow_body(root),
        );
        max_bottom
    }

    pub fn cached_scroll_height(&self) -> f32 {
        if !self.style_dirty
            && !self.root.layout.layout_dirty
            && !self.root.has_dirty_layout_descendant
            && let Some((generation, height)) = self.scroll_height_cache.get()
            && generation == self.layout_generation
        {
            return height;
        }
        let height = Self::scroll_height(&self.root);
        if !self.style_dirty
            && !self.root.layout.layout_dirty
            && !self.root.has_dirty_layout_descendant
        {
            self.scroll_height_cache
                .set(Some((self.layout_generation, height)));
        }
        height
    }

    pub fn scroll_width(root: &WebCore) -> f32 {
        let mut right = root.layout.margin_rect.w;
        let mut pending = vec![(root, true, false)];
        while let Some((node, is_root, inside_svg_foreign_content)) = pending.pop() {
            if inside_svg_foreign_content
                || matches!(node.style.display, Display::None)
                || crate::layout::is_layout_inert_svg_node(node)
                || matches!(node.style.position, Position::Fixed)
            {
                continue;
            }
            let contributes = !matches!(node.style.display, Display::Contents)
                && !(node.is_text_node() && node.text.trim().is_empty());
            if contributes && !is_root {
                let rect = node.layout.margin_rect;
                let edge = rect.x + rect.w;
                if matches!(node.style.position, Position::Absolute) {
                    if rect.w > 0.0 && edge > 0.0 && edge < right * 3.0 + 2000.0 {
                        right = right.max(edge);
                    } else {
                        continue;
                    }
                } else if rect.w > 0.0 {
                    right = right.max(edge);
                }
            }
            if !is_root
                && matches!(
                    node.style.overflow_x,
                    Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
                )
            {
                continue;
            }
            pending.extend(node.children.iter().map(|child| {
                (
                    child,
                    false,
                    node.tag == "svg" && crate::layout::is_svg_foreign_content_tag(&child.tag),
                )
            }));
        }
        right
    }

    pub fn cached_scroll_width(&self) -> f32 {
        if !self.style_dirty
            && !self.root.layout.layout_dirty
            && !self.root.has_dirty_layout_descendant
            && let Some((generation, width)) = self.scroll_width_cache.get()
            && generation == self.layout_generation
        {
            return width;
        }
        let width = Self::scroll_width(&self.root);
        if !self.style_dirty
            && !self.root.layout.layout_dirty
            && !self.root.has_dirty_layout_descendant
        {
            self.scroll_width_cache
                .set(Some((self.layout_generation, width)));
        }
        width
    }

    pub fn viewport_y_scroll_locked(&self) -> bool {
        fn locks(style: &ComputedStyle) -> bool {
            matches!(style.overflow_y, Overflow::Hidden | Overflow::Clip)
        }
        if locks(&self.root.style) {
            return true;
        }
        self.root
            .children
            .iter()
            .find(|child| child.tag == "body")
            .map(|body| locks(&body.style))
            .unwrap_or(false)
    }
}

fn advance_animated_image_frame(
    animated: &crate::html::AnimatedImage,
    current: usize,
    last: std::time::Instant,
    now: std::time::Instant,
) -> Option<(usize, std::time::Instant)> {
    let frame_count = animated.frames.len();
    if frame_count < 2 {
        return None;
    }
    let cycle_ms: u64 = animated
        .frames
        .iter()
        .map(|frame| frame.duration_ms.max(10) as u64)
        .sum();
    let elapsed_ms = now.saturating_duration_since(last).as_millis();
    let cycle_ms = cycle_ms as u128;
    let mut remaining_ms = elapsed_ms % cycle_ms;
    let mut next = current;
    let mut advanced = elapsed_ms >= cycle_ms;
    for _ in 0..frame_count {
        let delay = animated.frames[next].duration_ms.max(10) as u128;
        if remaining_ms < delay {
            break;
        }
        remaining_ms -= delay;
        next = (next + 1) % frame_count;
        advanced = true;
    }
    advanced.then(|| {
        (
            next,
            now - std::time::Duration::from_millis(remaining_ms as u64),
        )
    })
}

fn collapse_node_animation(node: &mut WebCore) -> bool {
    let Some(animated) = node.animated_image.as_mut() else {
        return false;
    };
    if !crate::html::collapse_animated_image(animated, node.animated_image_frame) {
        return false;
    }
    node.animated_image_frame = 0;
    node.animated_image_last_tick = None;
    node.image_data = animated.frames.first().map(|frame| frame.pixels.clone());
    node.image_data_width = animated.width;
    node.image_data_height = animated.height;
    true
}

fn collapse_animated_subtree(node: &mut WebCore) {
    collapse_node_animation(node);
    for child in &mut node.children {
        collapse_animated_subtree(child);
    }
}

fn has_animated_images(node: &WebCore) -> bool {
    node.animated_image
        .as_ref()
        .is_some_and(|animated| animated.can_animate())
        || node.children.iter().any(has_animated_images)
}

fn has_visible_animated_images(node: &WebCore, scroll_y: f32, viewport_h: f32) -> bool {
    fn walk(
        node: &WebCore,
        scroll_y: f32,
        viewport_h: f32,
        inherited_visible: bool,
        clip_top: f32,
        clip_bottom: f32,
    ) -> bool {
        let visible = inherited_visible && !animated_image_hidden_by_style(node);
        if !visible {
            return false;
        }
        if animated_image_intersects_clip(node, scroll_y, viewport_h, clip_top, clip_bottom)
            && node
                .animated_image
                .as_ref()
                .is_some_and(|animated| animated.can_animate())
        {
            return true;
        }
        let (child_clip_top, child_clip_bottom) =
            animated_child_clip(node, scroll_y, viewport_h, clip_top, clip_bottom);
        if child_clip_bottom < child_clip_top {
            return false;
        }
        node.children.iter().any(|child| {
            walk(
                child,
                scroll_y,
                viewport_h,
                visible,
                child_clip_top,
                child_clip_bottom,
            )
        })
    }

    walk(
        node,
        scroll_y,
        viewport_h,
        true,
        scroll_y,
        scroll_y + viewport_h,
    )
}

fn next_visible_animated_image_deadline(
    node: &WebCore,
    now: std::time::Instant,
    scroll_y: f32,
    viewport_h: f32,
) -> Option<std::time::Instant> {
    fn walk(
        node: &WebCore,
        now: std::time::Instant,
        scroll_y: f32,
        viewport_h: f32,
        inherited_visible: bool,
        clip_top: f32,
        clip_bottom: f32,
        best: &mut Option<std::time::Instant>,
    ) {
        let visible = inherited_visible && !animated_image_hidden_by_style(node);
        if !visible {
            return;
        }
        if animated_image_intersects_clip(node, scroll_y, viewport_h, clip_top, clip_bottom)
            && let Some(animated) = node.animated_image.as_ref()
            && animated.can_animate()
        {
            let current = node
                .animated_image_frame
                .min(animated.frames.len().saturating_sub(1));
            let delay = std::time::Duration::from_millis(
                animated.frames[current].duration_ms.max(10) as u64,
            );
            let due = node
                .animated_image_last_tick
                .map(|last| last + delay)
                .unwrap_or(now);
            if best.is_none_or(|existing| due < existing) {
                *best = Some(due);
            }
        }
        let (child_clip_top, child_clip_bottom) =
            animated_child_clip(node, scroll_y, viewport_h, clip_top, clip_bottom);
        if child_clip_bottom < child_clip_top {
            return;
        }
        for child in &node.children {
            walk(
                child,
                now,
                scroll_y,
                viewport_h,
                visible,
                child_clip_top,
                child_clip_bottom,
                best,
            );
        }
    }

    let mut best = None;
    walk(
        node,
        now,
        scroll_y,
        viewport_h,
        true,
        scroll_y,
        scroll_y + viewport_h,
        &mut best,
    );
    best
}

fn animated_image_hidden_by_style(node: &WebCore) -> bool {
    matches!(node.style.display, Display::None)
        || !node.style.visibility
        || node.style.opacity <= 0.0
}

fn animated_image_intersects_clip(
    node: &WebCore,
    scroll_y: f32,
    viewport_h: f32,
    clip_top: f32,
    clip_bottom: f32,
) -> bool {
    let rect = node.layout.margin_rect;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return viewport_h.is_infinite();
    }
    let (top, bottom) = animated_vertical_interval(node, scroll_y);
    let viewport_top = scroll_y;
    let viewport_bottom = scroll_y + viewport_h;
    bottom >= viewport_top && top <= viewport_bottom && bottom >= clip_top && top <= clip_bottom
}

fn animated_child_clip(
    node: &WebCore,
    scroll_y: f32,
    _viewport_h: f32,
    clip_top: f32,
    clip_bottom: f32,
) -> (f32, f32) {
    if matches!(
        node.style.overflow_y,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    ) {
        let content = node.layout.content_rect;
        if content.h <= 0.0 {
            return (1.0, 0.0);
        }
        let (top, bottom) = if matches!(node.style.position, Position::Fixed) {
            (scroll_y + content.y, scroll_y + content.y + content.h)
        } else {
            (content.y, content.y + content.h)
        };
        (clip_top.max(top), clip_bottom.min(bottom))
    } else {
        (clip_top, clip_bottom)
    }
}

fn animated_vertical_interval(node: &WebCore, scroll_y: f32) -> (f32, f32) {
    let rect = node.layout.margin_rect;
    if matches!(node.style.position, Position::Fixed) {
        (scroll_y + rect.y, scroll_y + rect.y + rect.h)
    } else {
        (rect.y, rect.y + rect.h)
    }
}

fn mark_layout_path_dirty(node: &mut WebCore, path: &[usize]) -> bool {
    if path.is_empty() {
        mark_layout_node_dirty(node);
        return true;
    }
    let Some(child) = node.children.get_mut(path[0]) else {
        return false;
    };
    if mark_layout_path_dirty(child, &path[1..]) {
        node.layout.cached_intrinsic_w.set(f32::NAN);
        node.layout.intrinsic_dirty = true;
        node.has_dirty_layout_descendant = true;
        return true;
    }
    false
}

fn mark_layout_node_dirty(node: &mut WebCore) {
    node.layout.layout_dirty = true;
    node.layout.cached_intrinsic_w.set(f32::NAN);
    node.layout.intrinsic_dirty = true;
    node.has_dirty_layout_descendant = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dom_created_style_element_updates_and_detaches_author_rules() {
        let mut doc = crate::load_html(
            "<html><body><button class='key'>7</button></body></html>",
            800.0,
        );
        let base_rules = doc.stylesheet.rules.len();
        let body = doc.body().unwrap();
        let style = doc.create_element("style");
        let text = doc.create_text_node(".key { width: 60px; }");
        doc.append_child(style, text);
        assert!(!doc.stylesheet.rules.iter().any(|rule| {
            rule.declarations
                .get("width")
                .is_some_and(|value| value == "60px")
        }));

        doc.append_child(body, style);
        assert_eq!(doc.stylesheet.rules.len(), base_rules + 1);
        assert!(doc.stylesheet.rules.iter().any(|rule| {
            rule.declarations
                .get("width")
                .is_some_and(|value| value == "60px")
        }));

        doc.set_text_content(style, ".key { width: 80px; }");
        assert_eq!(doc.stylesheet.rules.len(), base_rules + 1);
        assert!(doc.stylesheet.rules.iter().any(|rule| {
            rule.declarations
                .get("width")
                .is_some_and(|value| value == "80px")
        }));
        assert!(!doc.stylesheet.rules.iter().any(|rule| {
            rule.declarations
                .get("width")
                .is_some_and(|value| value == "60px")
        }));

        doc.remove_child(style);
        assert_eq!(doc.stylesheet.rules.len(), base_rules);
    }

    fn stylesheet_with_rule(selector: &str) -> crate::css::Stylesheet {
        let mut sheet = crate::css::Stylesheet::default();
        sheet.parse_and_add_author(&format!("{selector} {{ color: red }}"));
        sheet
    }

    fn stylesheet_with_color(selector: &str, color: &str) -> crate::css::Stylesheet {
        let mut sheet = crate::css::Stylesheet::default();
        sheet.parse_and_add_author(&format!("{selector} {{ color: {color} }}"));
        sheet
    }

    #[test]
    fn document_order_excludes_nonmatching_linked_media() {
        let mut doc = Document::new();
        doc.viewport_w = 800.0;
        doc.viewport_h = 600.0;
        doc.document_stylesheets = vec![
            DocumentStylesheet::Linked {
                href: "screen.css".into(),
                media: "screen".into(),
            },
            DocumentStylesheet::Linked {
                href: "print.css".into(),
                media: "print".into(),
            },
        ];
        doc.loaded_stylesheet_slots
            .insert(0, stylesheet_with_color(".screen-only", "red"));
        doc.loaded_stylesheet_slots
            .insert(1, stylesheet_with_color(".print-only", "blue"));
        doc.rebuild_author_stylesheet_from_document_order();
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|r| r.declarations.get("color").is_some_and(|v| v == "red"))
        );
        assert!(
            !doc.stylesheet
                .rules
                .iter()
                .any(|r| r.declarations.get("color").is_some_and(|v| v == "blue"))
        );
        assert!(doc.loaded_stylesheet_slots.contains_key(&1));
    }

    #[test]
    fn pending_stylesheet_poll_respects_the_document_order_budget() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Linked {
                href: "https://example.test/0.css".to_string(),
                media: String::new(),
            });
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..3 {
            tx.send(
                (
                    i,
                    format!("https://example.test/{i}.css"),
                    stylesheet_with_rule(&format!(".c{i}")),
                    String::new(),
                )
                    .into(),
            )
            .unwrap();
        }
        doc.pending_stylesheets = Some(rx);

        assert!(doc.poll_pending_stylesheets_budgeted(1, std::time::Duration::from_secs(1)));
        assert_eq!(doc.loaded_linked_stylesheets.len(), 1);
        assert!(
            doc.pending_stylesheets.is_some(),
            "remaining stylesheet fragments should stay queued for later frames"
        );

        assert!(doc.poll_pending_stylesheets_budgeted(1, std::time::Duration::from_secs(1)));
        assert_eq!(doc.loaded_linked_stylesheets.len(), 2);
    }

    #[test]
    fn inline_stylesheet_media_tracks_viewport_when_rebuilt() {
        let mut doc = Document::new();
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Inline {
                css: ".wide-only { color: red }".into(),
                media: "(min-width: 600px)".to_string(),
            });
        doc.viewport_w = 800.0;
        doc.viewport_h = 600.0;
        doc.rebuild_author_stylesheet_from_document_order();
        let wide_rules = doc.stylesheet.rules.len();

        doc.viewport_w = 400.0;
        doc.rebuild_author_stylesheet_from_document_order();
        assert_eq!(doc.stylesheet.rules.len() + 1, wide_rules);
    }

    #[test]
    fn inline_stylesheet_parse_is_reused_and_invalidated_by_source_or_base() {
        let mut doc = Document::new();
        doc.base_url = "https://example.test/one/".into();
        doc.document_stylesheets.push(DocumentStylesheet::Inline {
            css: ".target { background-image: url(icon.svg) }".into(),
            media: String::new(),
        });
        doc.rebuild_author_stylesheet_from_document_order();
        let first = doc.inline_stylesheet_cache[&0].sheet.clone();
        doc.rebuild_author_stylesheet_from_document_order();
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &doc.inline_stylesheet_cache[&0].sheet
        ));

        doc.document_stylesheets[0] = DocumentStylesheet::Inline {
            css: ".updated { background-image: url(icon.svg) }".into(),
            media: String::new(),
        };
        doc.rebuild_author_stylesheet_from_document_order();
        let second = doc.inline_stylesheet_cache[&0].sheet.clone();
        assert!(!std::sync::Arc::ptr_eq(&first, &second));
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".updated")
        );
        assert!(
            !doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".target")
        );

        doc.base_url = "https://example.test/two/".into();
        doc.rebuild_author_stylesheet_from_document_order();
        assert!(!std::sync::Arc::ptr_eq(
            &second,
            &doc.inline_stylesheet_cache[&0].sheet
        ));
        assert!(doc.stylesheet.rules.iter().any(|rule| {
            rule.declarations
                .iter()
                .any(|(_, value)| value.contains("https://example.test/two/icon.svg"))
        }));
    }

    #[test]
    fn inline_stylesheet_parse_is_reused_across_documents() {
        let css: std::sync::Arc<str> = ".revisited { background-image: url(icon.svg) }".into();
        let make_doc = || {
            let mut doc = Document::new();
            doc.base_url = "https://example.test/revisited/".into();
            doc.document_stylesheets.push(DocumentStylesheet::Inline {
                css: css.clone(),
                media: String::new(),
            });
            doc.rebuild_author_stylesheet_from_document_order();
            doc
        };
        let first = make_doc();
        let second = make_doc();
        assert!(std::sync::Arc::ptr_eq(
            &first.inline_stylesheet_cache[&0].sheet,
            &second.inline_stylesheet_cache[&0].sheet,
        ));
    }

    #[test]
    fn initial_load_reuses_parsed_inline_stylesheet_across_documents() {
        let html = "<style>.shared { color: red }</style><div class='shared'>hello</div>";
        let first = crate::load_html_with_base(html, "https://example.test/page", 800.0, 600.0);
        let second = crate::load_html_with_base(html, "https://example.test/page", 800.0, 600.0);
        assert!(std::sync::Arc::ptr_eq(
            &first.inline_stylesheet_cache[&0].sheet,
            &second.inline_stylesheet_cache[&0].sheet,
        ));
        assert!(
            second
                .stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".shared")
        );
    }

    #[test]
    fn pending_stylesheet_poll_preserves_document_order_not_arrival_order() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.stylesheet = crate::css::ua_stylesheet();
        doc.base_url = "https://example.test/page/".to_string();
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Inline {
                css: ".target { color: rgb(10, 0, 0) }".into(),
                media: String::new(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Linked {
                href: "https://example.test/slow.css".to_string(),
                media: String::new(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Inline {
                css: ".target { color: rgb(20, 0, 0) }".into(),
                media: String::new(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Linked {
                href: "https://example.test/fast.css".to_string(),
                media: String::new(),
            });

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            (
                3,
                "https://example.test/fast.css".to_string(),
                stylesheet_with_color(".target", "rgb(40, 0, 0)"),
                String::new(),
            )
                .into(),
        )
        .unwrap();
        tx.send(
            (
                1,
                "https://example.test/slow.css".to_string(),
                stylesheet_with_color(".target", "rgb(30, 0, 0)"),
                String::new(),
            )
                .into(),
        )
        .unwrap();
        doc.pending_stylesheets = Some(rx);

        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::from_secs(1)));

        let target_rules: Vec<_> = doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector == ".target")
            .collect();
        assert_eq!(target_rules.len(), 4);
        let reds: Vec<_> = target_rules
            .iter()
            .map(|rule| {
                rule.declarations
                    .iter()
                    .find(|decl| decl.0 == "color")
                    .map(|decl| decl.1.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();
        assert_eq!(
            reds,
            vec![
                "rgb(10, 0, 0)",
                "rgb(30, 0, 0)",
                "rgb(20, 0, 0)",
                "rgb(40, 0, 0)"
            ],
            "parallel stylesheet completion must fill source-order slots"
        );
    }

    #[test]
    fn pending_stylesheet_poll_preserves_repeated_href_slots() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.stylesheet = crate::css::ua_stylesheet();
        doc.base_url = "https://example.test/".to_string();
        for _ in 0..2 {
            doc.document_stylesheets
                .push(crate::types::DocumentStylesheet::Linked {
                    href: "https://example.test/app.css".to_string(),
                    media: String::new(),
                });
        }

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            (
                1,
                "https://example.test/app.css".to_string(),
                stylesheet_with_color(".target", "rgb(20, 0, 0)"),
                String::new(),
            )
                .into(),
        )
        .unwrap();
        tx.send(
            (
                0,
                "https://example.test/app.css".to_string(),
                stylesheet_with_color(".target", "rgb(10, 0, 0)"),
                String::new(),
            )
                .into(),
        )
        .unwrap();
        doc.pending_stylesheets = Some(rx);

        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::from_secs(1)));
        let reds: Vec<_> = doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector == ".target")
            .filter_map(|rule| {
                rule.declarations
                    .iter()
                    .find(|decl| decl.0 == "color")
                    .map(|decl| decl.1.as_str())
            })
            .collect();
        assert_eq!(reds, vec!["rgb(10, 0, 0)", "rgb(20, 0, 0)"]);
    }

    #[test]
    fn pending_stylesheet_replacement_discards_partial_rules() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.stylesheet = crate::css::ua_stylesheet();
        doc.base_url = "https://example.test/".into();
        doc.document_stylesheets.push(DocumentStylesheet::Linked {
            href: "app.css".into(),
            media: String::new(),
        });
        let url = "https://example.test/app.css".to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_stylesheets = Some(rx);
        tx.send(crate::types::PendingStylesheetResult::fragment(
            0,
            url.clone(),
            stylesheet_with_color(".target", "red"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        tx.send(crate::types::PendingStylesheetResult::replace(
            0,
            url,
            stylesheet_with_color(".target", "blue"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        let colors: Vec<_> = doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector == ".target")
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["blue"]);
    }

    #[test]
    fn unordered_pending_stylesheet_replacement_discards_partial_rules() {
        let mut doc = Document::new();
        doc.stylesheet = crate::css::ua_stylesheet();
        let url = "https://example.test/app.css".to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_stylesheets = Some(rx);
        tx.send(crate::types::PendingStylesheetResult::fragment(
            0,
            url.clone(),
            stylesheet_with_color(".target", "red"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        tx.send(crate::types::PendingStylesheetResult::replace(
            0,
            url,
            stylesheet_with_color(".target", "blue"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        let colors: Vec<_> = doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector == ".target")
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["blue"]);
    }

    #[test]
    fn tail_stylesheet_fragments_keep_existing_rule_storage() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.stylesheet = crate::css::ua_stylesheet();
        doc.base_url = "https://example.test/".into();
        doc.document_stylesheets.push(DocumentStylesheet::Linked {
            href: "tail.css".into(),
            media: String::new(),
        });
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_stylesheets = Some(rx);
        let url = "https://example.test/tail.css".to_string();
        tx.send(
            (
                0,
                url.clone(),
                stylesheet_with_rule(".first"),
                String::new(),
            )
                .into(),
        )
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        let first = doc
            .stylesheet
            .rules
            .iter()
            .find(|rule| rule.original_selector == ".first")
            .unwrap();
        let first_selector_storage = first.original_selector.as_ptr();

        tx.send((0, url, stylesheet_with_rule(".second"), String::new()).into())
            .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        let first = doc
            .stylesheet
            .rules
            .iter()
            .find(|rule| rule.original_selector == ".first")
            .unwrap();
        assert_eq!(first.original_selector.as_ptr(), first_selector_storage);
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".second")
        );
    }

    #[test]
    fn streamed_root_variables_keep_importance_across_unrelated_fragments() {
        let mut doc = Document::new();
        doc.stylesheet = crate::css::ua_stylesheet();
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_stylesheets = Some(rx);
        let send = |css: &str| {
            let mut sheet = crate::css::Stylesheet::default();
            sheet.parse_and_add_author(css);
            tx.send((0, String::new(), sheet, String::new()).into())
                .unwrap();
        };

        send(":root { --brand: red !important }");
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        assert_eq!(
            doc.stylesheet.variables.get("--brand").map(String::as_str),
            Some("red")
        );

        send(".unrelated { color: blue }");
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        assert_eq!(
            doc.stylesheet.variables.get("--brand").map(String::as_str),
            Some("red")
        );

        send("html { --brand: green; --accent: blue }");
        assert!(doc.poll_pending_stylesheets_budgeted(8, std::time::Duration::ZERO));
        assert_eq!(
            doc.stylesheet.variables.get("--brand").map(String::as_str),
            Some("red")
        );
        assert_eq!(
            doc.stylesheet.variables.get("--accent").map(String::as_str),
            Some("blue")
        );
    }

    #[test]
    fn stylesheet_tail_append_rejects_later_inline_css_and_repeated_href() {
        let mut doc = Document::new();
        doc.base_url = "https://example.test/".into();
        doc.document_stylesheets = vec![
            DocumentStylesheet::Linked {
                href: "app.css".into(),
                media: String::new(),
            },
            DocumentStylesheet::Inline {
                css: "p { color: blue }".into(),
                media: String::new(),
            },
        ];
        assert!(!doc.can_append_linked_stylesheet_fragment(0, "https://example.test/app.css"));
        doc.document_stylesheets.pop();
        assert!(doc.can_append_linked_stylesheet_fragment(0, "https://example.test/app.css"));
        doc.document_stylesheets.push(DocumentStylesheet::Linked {
            href: "app.css".into(),
            media: String::new(),
        });
        assert!(!doc.can_append_linked_stylesheet_fragment(0, "https://example.test/app.css"));
    }

    #[test]
    fn pending_stylesheet_poll_appends_live_fragments_without_rebuild_storage() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = false;
        doc.stylesheet = crate::css::ua_stylesheet();
        let ua_rules = doc.stylesheet.rules.len();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..2 {
            tx.send(
                (
                    i,
                    format!("https://example.test/{i}.css"),
                    stylesheet_with_rule(&format!(".live{i}")),
                    String::new(),
                )
                    .into(),
            )
            .unwrap();
        }
        doc.pending_stylesheets = Some(rx);

        assert!(doc.poll_pending_stylesheets_budgeted(2, std::time::Duration::from_secs(1)));
        assert!(
            doc.stylesheet.rules.len() >= ua_rules + 2,
            "live fragments should append directly to active stylesheet"
        );
        assert!(
            doc.loaded_linked_stylesheets.is_empty(),
            "live mode should not fill rebuild storage for every CSS fragment"
        );
    }

    #[test]
    fn pending_stylesheet_live_poll_handles_large_fragment_batches_in_place() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = false;
        doc.stylesheet = crate::css::ua_stylesheet();
        let ua_rules = doc.stylesheet.rules.len();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..512 {
            tx.send(
                (
                    i,
                    "https://example.test/app.css".to_string(),
                    stylesheet_with_rule(&format!(".item{i}")),
                    String::new(),
                )
                    .into(),
            )
            .unwrap();
        }
        doc.pending_stylesheets = Some(rx);

        assert!(doc.poll_pending_stylesheets_budgeted(512, std::time::Duration::from_secs(1)));
        assert!(
            doc.stylesheet.rules.len() >= ua_rules + 512,
            "all live fragments should append directly to the active stylesheet"
        );
        assert!(doc.loaded_linked_stylesheets.is_empty());
    }

    #[test]
    fn unrelated_link_update_does_not_rebuild_shadow_stylesheet() {
        fn shadow_sheet(doc: &mut Document, host_id: u32) -> &mut crate::css::Stylesheet {
            &mut crate::dom::find_box_mut(&mut doc.root, host_id)
                .expect("host box")
                .shadow_root
                .as_mut()
                .expect("shadow root")
                .stylesheet
        }

        let mut doc = crate::html::parse_html(
            "<x-host id='host'><template shadowrootmode='open'>\
             <link rel='stylesheet' href='shadow.css'><span class='inside'>x</span>\
             </template></x-host>",
        );
        doc.base_url = "https://example.test/page/".to_string();
        doc.preserve_stylesheet_document_order = true;
        doc.document_stylesheets.push(DocumentStylesheet::Linked {
            href: "other.css".to_string(),
            media: String::new(),
        });
        let host_id = doc.get_element_by_id("host").expect("shadow host");
        shadow_sheet(&mut doc, host_id).parse_and_add_author(".sentinel { color: red }");

        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_stylesheets = Some(rx);
        tx.send(PendingStylesheetResult::fragment(
            0,
            "https://example.test/page/other.css".to_string(),
            stylesheet_with_rule(".other"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(1, std::time::Duration::ZERO));
        assert!(
            shadow_sheet(&mut doc, host_id)
                .rules
                .iter()
                .any(|r| r.original_selector == ".sentinel")
        );

        tx.send(PendingStylesheetResult::fragment(
            1,
            "https://example.test/page/shadow.css".to_string(),
            stylesheet_with_rule(".inside"),
            String::new(),
        ))
        .unwrap();
        assert!(doc.poll_pending_stylesheets_budgeted(1, std::time::Duration::ZERO));
        let rules = &shadow_sheet(&mut doc, host_id).rules;
        assert!(rules.iter().any(|r| r.original_selector == ".inside"));
        assert!(!rules.iter().any(|r| r.original_selector == ".sentinel"));
    }

    #[test]
    fn pending_image_poll_respects_the_live_budget() {
        let mut doc = Document::new();
        for _ in 0..3 {
            doc.root.children.push(WebCore::new("img"));
        }
        let decoded =
            crate::html::DecodedImage::Raster(std::sync::Arc::new(vec![255, 0, 0, 255]), 1, 1);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..3 {
            tx.send(PendingImageResult::Loaded {
                node_id: 0,
                path: vec![i],
                target: PendingImageTarget::Element,
                url: format!("memory:{i}"),
                decoded: decoded.clone(),
            })
            .unwrap();
        }
        doc.pending_images = Some(rx);

        let first = doc.poll_pending_images_budgeted(1, std::time::Duration::from_secs(1));
        assert!(first.loaded_any);
        assert_eq!(doc.root.children[0].image_width, 1);
        assert_eq!(doc.root.children[1].image_width, 0);
        assert!(
            doc.pending_images.is_some(),
            "remaining image results should stay queued for later frames"
        );

        let second = doc.poll_pending_images_budgeted(1, std::time::Duration::from_secs(1));
        assert!(second.loaded_any);
        assert_eq!(doc.root.children[1].image_width, 1);
        assert_eq!(doc.root.children[2].image_width, 0);
    }

    #[test]
    fn image_results_use_verified_path_and_relocate_stale_node_ids() {
        let mut doc = Document::new();
        let mut first = WebCore::new("div");
        first.node_id = 11;
        first.layout.layout_dirty = false;
        let mut image = WebCore::new("img");
        image.node_id = 12;
        image.layout.layout_dirty = false;
        doc.root.children.push(first);
        doc.root.children.push(image);
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_images = Some(rx);

        tx.send(PendingImageResult::Dimensions {
            node_id: 12,
            path: vec![1],
            target: PendingImageTarget::Element,
            width: 20,
            height: 10,
        })
        .unwrap();
        assert!(
            doc.poll_pending_images_budgeted(1, std::time::Duration::ZERO)
                .needs_relayout
        );
        assert_eq!(doc.root.children[1].image_width, 20);
        assert!(!doc.root.children[0].layout.layout_dirty);

        doc.root.children[1].image_width = 0;
        doc.root.children[1].image_height = 0;
        doc.root.children[1].layout.layout_dirty = false;
        tx.send(PendingImageResult::Loaded {
            node_id: 12,
            path: vec![0],
            target: PendingImageTarget::Element,
            url: "memory:image".into(),
            decoded: crate::html::DecodedImage::Raster(
                std::sync::Arc::new(vec![255, 0, 0, 255]),
                1,
                1,
            ),
        })
        .unwrap();
        assert!(
            doc.poll_pending_images_budgeted(1, std::time::Duration::ZERO)
                .needs_relayout
        );
        assert_eq!(doc.root.children[0].image_width, 0);
        assert_eq!(doc.root.children[1].image_width, 1);
        assert!(
            !doc.root.children[0].layout.layout_dirty,
            "stale path must not dirty the wrong sibling"
        );
        assert!(
            doc.root.children[1].layout.layout_dirty,
            "relocated image must dirty its current path"
        );
    }

    #[test]
    fn image_set_keeps_low_density_pixels_until_high_density_arrives() {
        let mut doc = Document::new();
        doc.base_url = "https://example.test/".into();
        let mut node = WebCore::new("div");
        crate::css::apply_property(
            std::sync::Arc::make_mut(&mut node.style),
            "background-image",
            "image-set(url(one.png) 1x, url(two.png) 2x)",
        );
        doc.root.children.push(node);
        let (tx, rx) = std::sync::mpsc::channel();
        doc.pending_images = Some(rx);
        let loaded = |url: &str, red: u8| PendingImageResult::Loaded {
            node_id: 0,
            path: vec![0],
            target: PendingImageTarget::Background,
            url: url.to_string(),
            decoded: crate::html::DecodedImage::Raster(
                std::sync::Arc::new(vec![red, 0, 0, 255]),
                1,
                1,
            ),
        };

        tx.send(loaded("https://example.test/one.png", 40)).unwrap();
        assert!(
            doc.poll_pending_images_budgeted(1, std::time::Duration::ZERO)
                .loaded_any
        );
        assert_eq!(doc.root.children[0].bg_image_data.as_ref().unwrap()[0], 40);

        doc.device_pixel_ratio = 2.0;
        assert_eq!(doc.root.children[0].bg_image_data.as_ref().unwrap()[0], 40);
        tx.send(loaded("https://example.test/two.png", 90)).unwrap();
        tx.send(loaded("https://example.test/one.png", 40)).unwrap();
        assert!(
            doc.poll_pending_images_budgeted(1, std::time::Duration::ZERO)
                .loaded_any
        );
        assert_eq!(doc.root.children[0].bg_image_data.as_ref().unwrap()[0], 90);
        assert!(
            !doc.poll_pending_images_budgeted(1, std::time::Duration::ZERO)
                .loaded_any
        );
        assert_eq!(doc.root.children[0].bg_image_data.as_ref().unwrap()[0], 90);
    }
}
