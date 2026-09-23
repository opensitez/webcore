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

fn for_each_webcore_mut_by_id(node: &mut WebCore, id: u32, f: &mut impl FnMut(&mut WebCore)) {
    if node.node_id == id {
        f(node);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            for_each_webcore_mut_by_id(child, id, f);
        }
    }
    for child in &mut node.children {
        for_each_webcore_mut_by_id(child, id, f);
    }
}

impl Document {
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
            loaded_linked_stylesheets: HashMap::new(),
            loaded_stylesheet_slots: HashMap::new(),
            preserve_stylesheet_document_order: true,
            editor: Editor::new(),
            canvas_surfaces: crate::canvas::CanvasSurfaces::default(),
            media_states: crate::types::MediaStateMap::new(),
            event_targets: crate::dom::events::EventTargetMap::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            scrollbar_drag: None,
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
            cascade_styles: HashMap::new(),
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
            pending_images: None,
            image_load_errors: Vec::new(),
            images_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pending_stylesheets: None,
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
            results.sort_by_key(|(idx, _, _, _)| *idx);
            for (idx, css_url, sheet, _media) in results {
                match self.loaded_stylesheet_slots.entry(idx) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().append_fragment(sheet.clone());
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(sheet.clone());
                    }
                }
                match self.loaded_linked_stylesheets.entry(css_url) {
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().append_fragment(sheet);
                    }
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(sheet);
                    }
                }
                changed = true;
            }
        } else {
            let mut processed = 0usize;
            loop {
                match rx.try_recv() {
                    Ok((_, _, sheet, _)) => {
                        self.stylesheet.append_fragment(sheet);
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
        if rebuild_from_document_order {
            self.rebuild_author_stylesheet_from_document_order();
        }
        self.stylesheet
            .resolve_variables_for_viewport(self.viewport_w, self.viewport_h);
        self.stylesheet.rebuild_index();
        self.refresh_shadow_linked_stylesheets();
        self.style_dirty = true;
        true
    }

    fn rebuild_author_stylesheet_from_document_order(&mut self) {
        self.stylesheet = crate::css::ua_stylesheet();
        for (idx, ds) in self.document_stylesheets.iter().enumerate() {
            match ds {
                DocumentStylesheet::Inline { css } => {
                    self.stylesheet.parse_and_add_with_base(css, &self.base_url);
                }
                DocumentStylesheet::Linked { href, .. } => {
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

    pub(crate) fn refresh_shadow_linked_stylesheets(&mut self) -> bool {
        fn signature(sheet: &crate::css::Stylesheet) -> (usize, usize, usize, usize) {
            (
                sheet.rules.len(),
                sheet.font_faces.len(),
                sheet.keyframes.len(),
                sheet.raw_sources.len(),
            )
        }

        fn refresh_node(
            node: &mut WebCore,
            base_url: &str,
            loaded_linked: &HashMap<String, crate::css::Stylesheet>,
        ) -> bool {
            let mut changed = false;
            if let Some(sr) = node.shadow_root.as_mut() {
                let needs_rebuild =
                    !sr.document_stylesheets.is_empty() || !sr.adopted_stylesheets.is_empty();
                if needs_rebuild {
                    let before = signature(&sr.stylesheet);
                    let mut stylesheet = crate::css::ua_stylesheet();
                    for ds in &sr.document_stylesheets {
                        match ds {
                            DocumentStylesheet::Inline { css } => {
                                stylesheet.parse_and_add_author(css);
                            }
                            DocumentStylesheet::Linked { href, .. } => {
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
                    stylesheet.rebuild_index();
                    let after = signature(&stylesheet);
                    if before != after {
                        changed = true;
                    }
                    sr.stylesheet = stylesheet;
                }
                for child in &mut sr.children {
                    changed |= refresh_node(child, base_url, loaded_linked);
                }
            }
            for child in &mut node.children {
                changed |= refresh_node(child, base_url, loaded_linked);
            }
            changed
        }

        let base_url = self.base_url.clone();
        let loaded_linked = &self.loaded_linked_stylesheets;
        refresh_node(&mut self.root, &base_url, loaded_linked)
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
            let (node_id, path, target, decoded) = match result {
                PendingImageResult::Loaded {
                    node_id,
                    path,
                    target,
                    decoded,
                    ..
                } => (node_id, path, target, decoded),
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
                        if crate::html::set_decoded_bg_image_on_node(node, decoded.clone()) {
                            loaded_target = true;
                        }
                    }
                    PendingImageTarget::BackgroundLayer(layer_index) => {
                        if crate::html::set_decoded_bg_image_layer_on_node(
                            node,
                            layer_index,
                            decoded.clone(),
                        ) {
                            loaded_target = true;
                        }
                    }
                    PendingImageTarget::Mask => {
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
            if node_id != 0 {
                for_each_webcore_mut_by_id(&mut self.root, node_id, &mut apply_to_node);
                for pending in self.pending_nodes.values_mut() {
                    for_each_webcore_mut_by_id(pending, node_id, &mut apply_to_node);
                }
            } else if let Some(node) = find_node_by_path_mut(&mut self.root, &path) {
                apply_to_node(node);
            }
            if loaded_target {
                self.image_load_errors
                    .retain(|(err_path, err_target, _, _)| {
                        err_path != &loaded_path || *err_target != loaded_target_kind
                    });
                if target_needs_relayout {
                    mark_layout_path_dirty(&mut self.root, &path);
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
                        if crate::html::expand_animated_image_to_size(
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
                        let delay = std::time::Duration::from_millis(
                            animated.frames[current].duration_ms.max(10) as u64,
                        );
                        if now.duration_since(last) >= delay {
                            let next = (current + 1) % animated.frames.len();
                            node.animated_image_frame = next;
                            node.animated_image_last_tick = Some(now);
                            node.image_data = Some(animated.frames[next].pixels.clone());
                            node.image_data_width = animated.width;
                            node.image_data_height = animated.height;
                            tick.changed_any = true;
                            let rect = node.layout.border_rect;
                            if rect.w > 0.0 && rect.h > 0.0 {
                                tick.paint_rects.push(rect);
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

    /// Compute the full scrollable extent of the document.
    /// Walks all elements and returns the maximum bottom/right edge,
    /// ignoring containers with `height: 100vh` or similar constraints.
    pub fn scroll_height(root: &WebCore) -> f32 {
        fn walk_scroll(
            node: &WebCore,
            max_bottom: &mut f32,
            is_root: bool,
            inside_svg_foreign_content: bool,
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
                        );
                    }
                    return;
                }
                // Only count if within a reasonable range (2x the current max)
                let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
                if node.layout.margin_rect.h > 0.0
                    && bottom > 0.0
                    && bottom < *max_bottom * 3.0 + 2000.0
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
                if node.layout.margin_rect.h <= 0.0 {
                    return;
                }
                let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
                if bottom > *max_bottom {
                    *max_bottom = bottom;
                }
            }
            if !is_root
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
                );
            }
        }
        let mut max_bottom = 0.0;
        walk_scroll(root, &mut max_bottom, true, false);
        max_bottom
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
            tx.send((
                i,
                format!("https://example.test/{i}.css"),
                stylesheet_with_rule(&format!(".c{i}")),
                String::new(),
            ))
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
    fn pending_stylesheet_poll_preserves_document_order_not_arrival_order() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = true;
        doc.stylesheet = crate::css::ua_stylesheet();
        doc.base_url = "https://example.test/page/".to_string();
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Inline {
                css: ".target { color: rgb(10, 0, 0) }".to_string(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Linked {
                href: "https://example.test/slow.css".to_string(),
                media: String::new(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Inline {
                css: ".target { color: rgb(20, 0, 0) }".to_string(),
            });
        doc.document_stylesheets
            .push(crate::types::DocumentStylesheet::Linked {
                href: "https://example.test/fast.css".to_string(),
                media: String::new(),
            });

        let (tx, rx) = std::sync::mpsc::channel();
        tx.send((
            3,
            "https://example.test/fast.css".to_string(),
            stylesheet_with_color(".target", "rgb(40, 0, 0)"),
            String::new(),
        ))
        .unwrap();
        tx.send((
            1,
            "https://example.test/slow.css".to_string(),
            stylesheet_with_color(".target", "rgb(30, 0, 0)"),
            String::new(),
        ))
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
        tx.send((
            1,
            "https://example.test/app.css".to_string(),
            stylesheet_with_color(".target", "rgb(20, 0, 0)"),
            String::new(),
        ))
        .unwrap();
        tx.send((
            0,
            "https://example.test/app.css".to_string(),
            stylesheet_with_color(".target", "rgb(10, 0, 0)"),
            String::new(),
        ))
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
    fn pending_stylesheet_poll_appends_live_fragments_without_rebuild_storage() {
        let mut doc = Document::new();
        doc.preserve_stylesheet_document_order = false;
        doc.stylesheet = crate::css::ua_stylesheet();
        let ua_rules = doc.stylesheet.rules.len();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..2 {
            tx.send((
                i,
                format!("https://example.test/{i}.css"),
                stylesheet_with_rule(&format!(".live{i}")),
                String::new(),
            ))
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
            tx.send((
                i,
                "https://example.test/app.css".to_string(),
                stylesheet_with_rule(&format!(".item{i}")),
                String::new(),
            ))
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
}
