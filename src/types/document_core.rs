//! `Document` construction, the node index, and node lookup.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::arena::DomArena;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

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
            style_dirty: false,
            prev_hovered_box: 0,
            pending_announcements: Vec::new(),
            live_region_snapshots: HashMap::new(),
            live_regions_initialized: false,
            layout_generation: 0,
            pending_images: None,
            images_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
    /// Poll for images that arrived from background fetch threads.
    /// Returns `true` if any new images were loaded (caller should re-layout).
    pub fn poll_pending_images(&mut self) -> bool {
        let rx = match self.pending_images.as_ref() {
            Some(rx) => rx,
            None => return false,
        };
        let mut loaded_any = false;
        while let Ok((path, target, decoded)) = rx.try_recv() {
            let mut loaded_target = false;
            if let Some(node) = find_node_by_path_mut(&mut self.root, &path) {
                match target {
                    PendingImageTarget::Element => {
                        crate::html::set_decoded_image_on_node(node, decoded);
                        loaded_target = true;
                    }
                    PendingImageTarget::Background => {
                        if let Some((data, w, h)) = crate::html::decoded_image_pixels(decoded) {
                            node.bg_image_data = Some(std::sync::Arc::new(data));
                            node.bg_image_width = w;
                            node.bg_image_height = h;
                            loaded_target = true;
                        }
                    }
                    PendingImageTarget::Mask => {
                        if let Some((data, w, h)) = crate::html::decoded_image_pixels(decoded) {
                            node.mask_image_data = Some(std::sync::Arc::new(data));
                            node.mask_image_width = w;
                            node.mask_image_height = h;
                            loaded_target = true;
                        }
                    }
                }
            }
            if loaded_target {
                mark_layout_path_dirty(&mut self.root, &path);
                loaded_any = true;
            }
        }
        if self
            .images_in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            self.pending_images = None;
        }
        loaded_any
    }

    /// Advance animated image frames. Returns true when pixels changed and the
    /// caller should repaint.
    pub fn tick_animated_images(&mut self, now: std::time::Instant) -> bool {
        fn tick_node(node: &mut WebCore, now: std::time::Instant) -> bool {
            let mut changed = false;
            if let Some(animated) = node.animated_image.as_ref() {
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
                        changed = true;
                    } else if node.animated_image_last_tick.is_none() {
                        node.animated_image_last_tick = Some(now);
                    }
                }
            }
            for child in &mut node.children {
                changed |= tick_node(child, now);
            }
            changed
        }

        tick_node(&mut self.root, now)
    }

    pub fn has_animated_images(&self) -> bool {
        has_animated_images(&self.root)
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
        for child in &root.children {
            Self::walk_all(child, f);
        }
    }

    pub fn walk_all_mut<F: FnMut(&mut WebCore)>(root: &mut WebCore, f: &mut F) {
        f(root);
        for child in &mut root.children {
            Self::walk_all_mut(child, f);
        }
    }

    /// Compute the full scrollable extent of the document.
    /// Walks all elements and returns the maximum bottom/right edge,
    /// ignoring containers with `height: 100vh` or similar constraints.
    pub fn scroll_height(root: &WebCore) -> f32 {
        fn walk_scroll(node: &WebCore, max_bottom: &mut f32, is_root: bool) {
            if matches!(node.style.display, Display::None) {
                return;
            }
            // Fixed elements don't contribute to scroll height
            if matches!(node.style.position, Position::Fixed) {
                return;
            }
            // Skip zero-size nodes (not yet laid out or collapsed)
            if node.layout.margin_rect.w == 0.0 && node.layout.margin_rect.h == 0.0 {
                return;
            }
            // Absolute elements contribute only if they're within the document flow area
            // (some abs elements are positioned far off-screen as accessibility hacks)
            if matches!(node.style.position, Position::Absolute) {
                // Only count if within a reasonable range (2x the current max)
                let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
                if bottom > 0.0 && bottom < *max_bottom * 3.0 + 2000.0 {
                    if bottom > *max_bottom {
                        *max_bottom = bottom;
                    }
                }
                // Don't recurse into abs children — they position relative to their CB
                return;
            }
            let bottom = node.layout.margin_rect.y + node.layout.margin_rect.h;
            if bottom > *max_bottom {
                *max_bottom = bottom;
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
                walk_scroll(child, max_bottom, false);
            }
        }
        let mut max_bottom = root.layout.margin_rect.h;
        walk_scroll(root, &mut max_bottom, true);
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

fn has_animated_images(node: &WebCore) -> bool {
    node.animated_image
        .as_ref()
        .is_some_and(|animated| animated.frames.len() > 1)
        || node.children.iter().any(has_animated_images)
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
