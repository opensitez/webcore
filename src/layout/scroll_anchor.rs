use std::sync::Arc;

use crate::types::{ComputedStyle, Display, Document, Overflow, Position, WebCore};

fn is_scroll_container(node: &WebCore) -> bool {
    matches!(
        node.style.overflow_y,
        Overflow::Auto | Overflow::Scroll | Overflow::Hidden
    )
}

fn optimal_region(
    style: &ComputedStyle,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    font_px: f32,
    root_font_px: f32,
    viewport_w: f32,
    viewport_h: f32,
) -> (f32, f32, f32, f32) {
    let resolve = |length: &crate::types::CssLength, basis| {
        length
            .resolve_vp(font_px, basis, root_font_px, viewport_w, viewport_h)
            .max(0.0)
    };
    let region_left = left + resolve(&style.scroll_padding_left, width);
    let region_top = top + resolve(&style.scroll_padding_top, height);
    let region_right =
        (left + width - resolve(&style.scroll_padding_right, width)).max(region_left);
    let region_bottom =
        (top + height - resolve(&style.scroll_padding_bottom, height)).max(region_top);
    (region_left, region_right, region_top, region_bottom)
}

#[derive(Clone, Copy)]
struct Candidate<'a> {
    node: &'a WebCore,
    parent: Option<usize>,
    clip_left: f32,
    clip_right: f32,
    clip_top: f32,
    clip_bottom: f32,
}

impl Candidate<'_> {
    fn is_viable(self, exclude_position: bool) -> bool {
        let node = self.node;
        if node.style.display == Display::None
            || node.style.overflow_anchor == "none"
            || (exclude_position
                && matches!(
                    node.style.position,
                    Position::Fixed | Position::Sticky | Position::Absolute
                ))
            || self.clip_left >= self.clip_right
            || self.clip_top >= self.clip_bottom
        {
            return false;
        }
        if node.style.display == Display::Contents {
            return true;
        }
        let rect = node.layout.border_rect;
        rect.w > 0.0
            && rect.h > 0.0
            && rect.x < self.clip_right
            && rect.x + rect.w > self.clip_left
            && rect.y < self.clip_bottom
            && rect.y + rect.h > self.clip_top
    }

    fn is_fully_visible(self) -> bool {
        let rect = self.node.layout.border_rect;
        rect.x >= self.clip_left
            && rect.x + rect.w <= self.clip_right
            && rect.y >= self.clip_top
            && rect.y + rect.h <= self.clip_bottom
    }

    fn child_clip(self) -> (f32, f32, f32, f32) {
        let node = self.node;
        let (left, right) = if node.style.overflow_x == Overflow::Clip {
            (
                self.clip_left.max(node.layout.content_rect.x),
                self.clip_right
                    .min(node.layout.content_rect.x + node.layout.content_rect.w),
            )
        } else {
            (self.clip_left, self.clip_right)
        };
        let (top, bottom) = if node.style.overflow_y == Overflow::Clip {
            (
                self.clip_top.max(node.layout.content_rect.y),
                self.clip_bottom
                    .min(node.layout.content_rect.y + node.layout.content_rect.h),
            )
        } else {
            (self.clip_top, self.clip_bottom)
        };
        (left, right, top, bottom)
    }
}

pub(super) struct ViewportAnchor {
    node_id: u32,
    old_y: f32,
    old_scroll_y: f32,
    path: Vec<(u32, Arc<ComputedStyle>)>,
}

impl ViewportAnchor {
    pub(super) fn capture(doc: &Document, viewport_w: f32, viewport_h: f32) -> Option<Self> {
        if doc.scroll_y <= 0.0
            || viewport_h <= 0.0
            || (doc.viewport_w - viewport_w).abs() > 0.5
            || (doc.viewport_h - viewport_h).abs() > 0.5
        {
            return None;
        }
        let root_font_px = doc.root.style.font_size_px(
            ComputedStyle::INITIAL_FONT_SIZE_PX,
            ComputedStyle::INITIAL_FONT_SIZE_PX,
        );
        let (left, right, top, bottom) = optimal_region(
            &doc.root.style,
            doc.scroll_x,
            doc.scroll_y,
            viewport_w,
            viewport_h,
            root_font_px,
            root_font_px,
            viewport_w,
            viewport_h,
        );
        if let Some(focused) = editable_focus(doc) {
            if let Some(anchor) =
                Self::capture_priority(&doc.root, focused, left, right, top, bottom, doc.scroll_y)
            {
                return Some(anchor);
            }
        }
        Self::capture_in(&doc.root, left, right, top, bottom, true, doc.scroll_y)
    }

    fn capture_priority(
        root: &WebCore,
        focused: u32,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        scroll_y: f32,
    ) -> Option<Self> {
        if root.node_id == focused {
            return None;
        }
        let mut candidates = vec![Candidate {
            node: root,
            parent: None,
            clip_left: left,
            clip_right: right,
            clip_top: top,
            clip_bottom: bottom,
        }];
        let mut pending = vec![0usize];
        while let Some(index) = pending.pop() {
            let candidate = candidates[index];
            let node = candidate.node;
            if !candidate.is_viable(index != 0) {
                continue;
            }
            if node.node_id == focused {
                let mut anchor = Self::capture_in(
                    node,
                    candidate.clip_left,
                    candidate.clip_right,
                    candidate.clip_top,
                    candidate.clip_bottom,
                    true,
                    scroll_y,
                )?;
                let mut path = Vec::new();
                let mut parent = candidate.parent;
                while let Some(parent_index) = parent {
                    let ancestor = &candidates[parent_index];
                    path.push((ancestor.node.node_id, ancestor.node.style.clone()));
                    parent = ancestor.parent;
                }
                path.reverse();
                path.append(&mut anchor.path);
                anchor.path = path;
                return Some(anchor);
            }
            if index != 0 && is_scroll_container(node) {
                continue;
            }
            let (clip_left, clip_right, clip_top, clip_bottom) = candidate.child_clip();
            for child in node.children.iter().rev() {
                let child_index = candidates.len();
                candidates.push(Candidate {
                    node: child,
                    parent: Some(index),
                    clip_left,
                    clip_right,
                    clip_top,
                    clip_bottom,
                });
                pending.push(child_index);
            }
        }
        None
    }

    fn capture_in(
        root: &WebCore,
        left: f32,
        right: f32,
        top: f32,
        bottom: f32,
        include_root: bool,
        scroll_y: f32,
    ) -> Option<Self> {
        let mut candidates = vec![Candidate {
            node: root,
            parent: None,
            clip_left: left,
            clip_right: right,
            clip_top: top,
            clip_bottom: bottom,
        }];
        let mut pending = vec![(0usize, false)];
        while let Some((index, fallback)) = pending.pop() {
            let candidate = candidates[index];
            let node = candidate.node;
            if fallback {
                if node.node_id != 0 && !node.is_text_node() {
                    return Some(Self::from_candidate(&candidates, index, scroll_y));
                }
                continue;
            }
            if !candidate.is_viable(include_root || index != 0) {
                continue;
            }
            let contents = node.style.display == Display::Contents;
            if !contents
                && candidate.is_fully_visible()
                && (include_root || index != 0)
                && node.node_id != 0
                && !node.is_text_node()
            {
                return Some(Self::from_candidate(&candidates, index, scroll_y));
            }
            if !contents && (include_root || index != 0) {
                pending.push((index, true));
            }
            if index != 0 && is_scroll_container(node) {
                continue;
            }
            let (clip_left, clip_right, clip_top, clip_bottom) = candidate.child_clip();
            for child in node.children.iter().rev() {
                let child_index = candidates.len();
                candidates.push(Candidate {
                    node: child,
                    parent: Some(index),
                    clip_left,
                    clip_right,
                    clip_top,
                    clip_bottom,
                });
                pending.push((child_index, false));
            }
        }
        None
    }

    fn from_candidate(candidates: &[Candidate<'_>], index: usize, scroll_y: f32) -> Self {
        let node = candidates[index].node;
        let mut path = Vec::new();
        let mut current = Some(index);
        while let Some(index) = current {
            let candidate = &candidates[index];
            path.push((candidate.node.node_id, candidate.node.style.clone()));
            current = candidate.parent;
        }
        path.reverse();
        Self {
            node_id: node.node_id,
            old_y: node.layout.border_rect.y,
            old_scroll_y: scroll_y,
            path,
        }
    }

    pub(super) fn adjust(self, doc: &mut Document, viewport_h: f32) {
        if (doc.scroll_y - self.old_scroll_y).abs() > 0.01
            || doc.scroll_y <= 0.0
            || !self.path_is_valid(doc)
        {
            return;
        }
        let Some(delta) = self.movement(doc) else {
            return;
        };
        let max_scroll = (Document::scroll_height(&doc.root) - viewport_h).max(0.0);
        let next = (doc.scroll_y + delta).clamp(0.0, max_scroll);
        if (next - doc.scroll_y).abs() >= 0.01 {
            doc.scroll_y = next;
            doc.fire_window_event("scroll");
        }
    }

    fn path_is_valid(&self, doc: &Document) -> bool {
        for (id, old_style) in &self.path {
            let current = if *id == 0 {
                &doc.root
            } else if let Some(node) = doc.get_box_by_id(*id) {
                node
            } else {
                return false;
            };
            if current.style.display == Display::None
                || current.style.overflow_anchor == "none"
                || anchoring_suppressed_by_style(old_style, &current.style)
            {
                return false;
            }
        }
        true
    }

    fn movement(&self, doc: &Document) -> Option<f32> {
        let anchor = doc.get_box_by_id(self.node_id)?;
        let delta = anchor.layout.border_rect.y - self.old_y;
        if !delta.is_finite() || delta.abs() < 0.01 {
            return None;
        }
        Some(delta)
    }
}

struct ElementAnchor {
    scroller_id: u32,
    old_content_y: f32,
    anchor: ViewportAnchor,
}

pub(super) struct ElementAnchors(Vec<ElementAnchor>);

impl ElementAnchors {
    pub(super) fn capture(doc: &Document) -> Self {
        let mut anchors = Vec::new();
        let focused = editable_focus(doc);
        let root_font_px = doc.root.style.font_size_px(
            ComputedStyle::INITIAL_FONT_SIZE_PX,
            ComputedStyle::INITIAL_FONT_SIZE_PX,
        );
        let focus_ancestors = focused
            .and_then(|id| doc.node_index.get(&id).map(|path| (id, path)))
            .and_then(|(id, path)| {
                let mut node = &doc.root;
                let mut ancestors = Vec::with_capacity(path.len());
                for step in path {
                    ancestors.push(node.node_id);
                    node = node.children.get(*step as usize)?;
                }
                (node.node_id == id).then_some(ancestors)
            });
        let mut pending = vec![(&doc.root, ComputedStyle::INITIAL_FONT_SIZE_PX)];
        while let Some((node, parent_font_px)) = pending.pop() {
            if node.style.display == Display::None {
                continue;
            }
            let font_px = node.style.font_size_px(parent_font_px, root_font_px);
            if node.node_id != 0
                && is_scroll_container(node)
                && node.style.overflow_anchor != "none"
                && node.layout.scroll_top > 0.0
                && node.layout.content_rect.h > 0.0
            {
                let (left, right, top, bottom) = optimal_region(
                    &node.style,
                    node.layout.content_rect.x + node.layout.scroll_left,
                    node.layout.content_rect.y + node.layout.scroll_top,
                    node.layout.content_rect.w,
                    node.layout.content_rect.h,
                    font_px,
                    root_font_px,
                    doc.viewport_w,
                    doc.viewport_h,
                );
                let priority_focus = focused.filter(|_| {
                    focus_ancestors
                        .as_ref()
                        .is_none_or(|ancestors| ancestors.contains(&node.node_id))
                });
                let anchor = priority_focus
                    .and_then(|id| {
                        ViewportAnchor::capture_priority(
                            node,
                            id,
                            left,
                            right,
                            top,
                            bottom,
                            node.layout.scroll_top,
                        )
                    })
                    .or_else(|| {
                        ViewportAnchor::capture_in(
                            node,
                            left,
                            right,
                            top,
                            bottom,
                            false,
                            node.layout.scroll_top,
                        )
                    });
                if let Some(anchor) = anchor {
                    anchors.push(ElementAnchor {
                        scroller_id: node.node_id,
                        old_content_y: node.layout.content_rect.y,
                        anchor,
                    });
                }
            }
            pending.extend(node.children.iter().map(|child| (child, font_px)));
        }
        Self(anchors)
    }

    pub(super) fn adjust(self, doc: &mut Document) {
        for entry in self.0 {
            if !entry.anchor.path_is_valid(doc) {
                continue;
            }
            let Some(scroller) = doc.get_box_by_id(entry.scroller_id) else {
                continue;
            };
            if !is_scroll_container(scroller) || scroller.style.overflow_anchor == "none" {
                continue;
            }
            let current_content_y = scroller.layout.content_rect.y;
            let max_scroll =
                (scroller.layout.scroll_height - scroller.layout.content_rect.h).max(0.0);
            let Some(anchor) = doc.get_box_by_id(entry.anchor.node_id) else {
                continue;
            };
            let old_relative_y = entry.anchor.old_y - entry.old_content_y;
            let new_relative_y = anchor.layout.border_rect.y - current_content_y;
            let delta = new_relative_y - old_relative_y;
            if !delta.is_finite() || delta.abs() < 0.01 {
                continue;
            }
            let next = (entry.anchor.old_scroll_y + delta).clamp(0.0, max_scroll);
            let Some(scroller) = doc.get_box_by_id_mut(entry.scroller_id) else {
                continue;
            };
            if (next - scroller.layout.scroll_top).abs() >= 0.01 {
                scroller.layout.scroll_top = next;
                let mut event = crate::dom::events::DomEvent::new("scroll", entry.scroller_id);
                doc.dispatch_dom_event(&mut event);
            }
        }
    }
}

fn editable_focus(doc: &Document) -> Option<u32> {
    let node = doc.get_box_by_id(doc.focused_box)?;
    let editable = match node.tag.as_str() {
        "textarea" => true,
        "input" => node.attributes.get("type").is_none_or(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "text" | "search" | "tel" | "url" | "email" | "password" | "number"
            )
        }),
        _ => node.attributes.get("contenteditable").is_some_and(|value| {
            value.is_empty()
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("plaintext-only")
        }),
    };
    editable.then_some(node.node_id)
}

fn anchoring_suppressed_by_style(old: &ComputedStyle, new: &ComputedStyle) -> bool {
    old.position != new.position
        || old.top != new.top
        || old.right != new.right
        || old.bottom != new.bottom
        || old.left != new.left
        || old.margin_top != new.margin_top
        || old.margin_right != new.margin_right
        || old.margin_bottom != new.margin_bottom
        || old.margin_left != new.margin_left
        || old.padding_top != new.padding_top
        || old.padding_right != new.padding_right
        || old.padding_bottom != new.padding_bottom
        || old.padding_left != new.padding_left
        || old.width != new.width
        || old.height != new.height
        || old.min_width != new.min_width
        || old.max_width != new.max_width
        || old.min_height != new.min_height
        || old.max_height != new.max_height
        || old.transform != new.transform
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutEngine;

    fn page(root_style: &str) -> (LayoutEngine, Document, u32, u32) {
        let html = format!(
            "<style>html,body{{margin:0}}html{{{root_style}}}\
             #before{{height:100px}}#anchor{{height:100px}}#after{{height:1200px}}</style>\
             <div id='before'></div><div id='anchor'></div><div id='after'></div>"
        );
        let mut doc = crate::html::parse_html(&html);
        let before = doc.query_selector("#before").unwrap();
        let anchor = doc.query_selector("#anchor").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 200.0;
        engine.layout(&mut doc, 400.0);
        (engine, doc, before, anchor)
    }

    #[test]
    fn growth_above_the_viewport_keeps_the_anchor_in_place() {
        let (mut engine, mut doc, before, anchor) = page("");
        doc.scroll_y = 120.0;
        let old_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y - doc.scroll_y;
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        let new_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y - doc.scroll_y;
        assert!((new_view_y - old_view_y).abs() < 0.5);
        assert!((doc.scroll_y - 180.0).abs() < 0.5);
    }

    #[test]
    fn overflow_anchor_none_disables_viewport_adjustment() {
        let (mut engine, mut doc, before, _) = page("overflow-anchor:none");
        doc.scroll_y = 120.0;
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.scroll_y - 120.0).abs() < 0.5);
    }

    #[test]
    fn changing_the_anchor_geometry_suppresses_adjustment() {
        let (mut engine, mut doc, before, anchor) = page("");
        doc.scroll_y = 120.0;
        doc.set_style_property(before, "height", "160px");
        doc.set_style_property(anchor, "height", "140px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.scroll_y - 120.0).abs() < 0.5);
    }

    #[test]
    fn excluded_subtree_is_not_selected_as_anchor() {
        let (_, mut doc, _, anchor) = page("");
        doc.scroll_y = 120.0;
        doc.set_style_property(anchor, "overflow-anchor", "none");
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 200.0;
        engine.layout(&mut doc, 400.0);
        let selected = ViewportAnchor::capture(&doc, 400.0, 200.0).unwrap();
        assert_ne!(selected.node_id, anchor);
    }

    #[test]
    fn text_growth_above_viewport_anchors_no_cascade_layout() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#before{width:100px;font-size:20px;line-height:20px}\
             #anchor{height:100px}#after{height:1200px}</style>\
             <div id='before'>Short</div><div id='anchor'></div><div id='after'></div>",
        );
        let before = doc.query_selector("#before").unwrap();
        let anchor = doc.query_selector("#anchor").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 200.0;
        engine.layout(&mut doc, 400.0);
        doc.scroll_y = 30.0;
        let old_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y - doc.scroll_y;
        doc.set_text_content(
            before,
            "Short text grows across several wrapped lines above us",
        );
        engine.layout_no_cascade(&mut doc, 400.0);
        let new_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y - doc.scroll_y;
        assert!((new_view_y - old_view_y).abs() < 0.5);
        assert!(doc.scroll_y > 30.0);
    }

    fn nested_page(scroller_style: &str) -> (LayoutEngine, Document, u32, u32, u32, u32) {
        let html = format!(
            "<style>html,body{{margin:0}}#above{{height:100px}}\
             #scroller{{height:150px;width:200px;overflow:auto;{scroller_style}}}\
             #before{{height:100px}}#anchor{{height:100px}}#after{{height:500px}}\
             #tail{{height:1000px}}</style>\
             <div id='above'></div><div id='scroller'>\
             <div id='before'></div><div id='anchor'></div><div id='after'></div>\
             </div><div id='tail'></div>"
        );
        let mut doc = crate::html::parse_html(&html);
        let above = doc.query_selector("#above").unwrap();
        let scroller = doc.query_selector("#scroller").unwrap();
        let before = doc.query_selector("#before").unwrap();
        let anchor = doc.query_selector("#anchor").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 300.0;
        engine.layout(&mut doc, 400.0);
        doc.element_scroll_to(scroller, 0.0, 120.0);
        (engine, doc, above, scroller, before, anchor)
    }

    #[test]
    fn growth_inside_scroller_preserves_visible_child() {
        let (mut engine, mut doc, _, scroller, before, anchor) = nested_page("");
        let old_top = doc.element_scroll_top(scroller);
        let old_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y
            - doc.get_box_by_id(scroller).unwrap().layout.content_rect.y
            - old_top;
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        let new_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y
            - doc.get_box_by_id(scroller).unwrap().layout.content_rect.y
            - doc.element_scroll_top(scroller);
        assert!((new_view_y - old_view_y).abs() < 0.5);
        assert!((doc.element_scroll_top(scroller) - 180.0).abs() < 0.5);
        assert_eq!(doc.scroll_y, 0.0);
    }

    #[test]
    fn moving_the_whole_scroller_does_not_change_its_scroll_top() {
        let (mut engine, mut doc, above, scroller, _, _) = nested_page("");
        doc.set_style_property(above, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 120.0).abs() < 0.5);
    }

    #[test]
    fn scroll_container_overflow_anchor_none_opts_out() {
        let (mut engine, mut doc, _, scroller, before, _) = nested_page("overflow-anchor:none");
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 120.0).abs() < 0.5);
    }

    #[test]
    fn nested_growth_does_not_move_the_viewport() {
        let (mut engine, mut doc, _, scroller, before, _) = nested_page("");
        doc.scroll_y = 50.0;
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.scroll_y - 50.0).abs() < 0.5);
        assert!((doc.element_scroll_top(scroller) - 180.0).abs() < 0.5);
    }

    #[test]
    fn positioned_scroller_still_anchors_its_own_contents() {
        let (mut engine, mut doc, _, scroller, before, _) =
            nested_page("position:absolute;top:100px");
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 180.0).abs() < 0.5);
    }

    #[test]
    fn programmatically_scrolled_hidden_overflow_survives_relayout() {
        let (mut engine, mut doc, _, scroller, before, _) = nested_page("overflow:hidden");
        assert!((doc.element_scroll_top(scroller) - 120.0).abs() < 0.5);
        doc.set_style_property(before, "height", "160px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 180.0).abs() < 0.5);
    }

    #[test]
    fn nested_text_growth_anchors_no_cascade_layout() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#scroller{height:120px;width:200px;overflow:auto}\
             #before{width:100px;font-size:20px;line-height:20px}\
             #anchor{height:100px}#after{height:500px}</style>\
             <div id='scroller'><div id='before'>Short</div>\
             <div id='anchor'></div><div id='after'></div></div>",
        );
        let scroller = doc.query_selector("#scroller").unwrap();
        let before = doc.query_selector("#before").unwrap();
        let anchor = doc.query_selector("#anchor").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 300.0;
        engine.layout(&mut doc, 400.0);
        doc.element_scroll_to(scroller, 0.0, 30.0);
        let old_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y
            - doc.get_box_by_id(scroller).unwrap().layout.content_rect.y
            - doc.element_scroll_top(scroller);
        doc.set_text_content(
            before,
            "Short text grows across several wrapped lines above us",
        );
        engine.layout_no_cascade(&mut doc, 400.0);
        let new_view_y = doc.get_box_by_id(anchor).unwrap().layout.border_rect.y
            - doc.get_box_by_id(scroller).unwrap().layout.content_rect.y
            - doc.element_scroll_top(scroller);
        assert!((new_view_y - old_view_y).abs() < 0.5);
        assert!(doc.element_scroll_top(scroller) > 30.0);
    }

    #[test]
    fn clipped_descendant_cannot_become_viewport_anchor() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#clip{height:50px;overflow:clip}\
             #hidden{position:relative;top:60px;height:100px}\
             #tail{height:1000px}</style>\
             <div id='clip'><div id='hidden'></div></div><div id='tail'></div>",
        );
        let clip = doc.query_selector("#clip").unwrap();
        let hidden = doc.query_selector("#hidden").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 100.0;
        engine.layout(&mut doc, 400.0);
        doc.scroll_y = 30.0;
        let selected = ViewportAnchor::capture(&doc, 400.0, 100.0).unwrap();
        assert_eq!(selected.node_id, clip);
        assert_ne!(selected.node_id, hidden);
    }

    #[test]
    fn horizontally_offscreen_candidate_is_not_selected() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#off{position:relative;left:600px;width:100px;height:50px}\
             #visible{width:100px;height:100px}#tail{height:1000px}</style>\
             <div id='off'></div><div id='visible'></div><div id='tail'></div>",
        );
        let off = doc.query_selector("#off").unwrap();
        let visible = doc.query_selector("#visible").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 100.0;
        engine.layout(&mut doc, 400.0);
        doc.scroll_y = 30.0;
        let selected = ViewportAnchor::capture(&doc, 400.0, 100.0).unwrap();
        assert_ne!(selected.node_id, off);
        assert_eq!(selected.node_id, visible);
    }

    #[test]
    fn editable_focus_takes_priority_over_earlier_visible_box() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#before{height:50px}#after{height:1000px}</style>\
             <div id='before'></div><input id='focus'><div id='after'></div>",
        );
        let before = doc.query_selector("#before").unwrap();
        let focus = doc.query_selector("#focus").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 100.0;
        engine.layout(&mut doc, 400.0);
        doc.scroll_y = 20.0;
        doc.focused_box = before;
        let non_editable = ViewportAnchor::capture(&doc, 400.0, 100.0).unwrap();
        assert_ne!(non_editable.node_id, focus);
        doc.focused_box = focus;
        let selected = ViewportAnchor::capture(&doc, 400.0, 100.0).unwrap();
        assert_eq!(selected.node_id, focus);
        doc.set_style_property(before, "height", "80px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.scroll_y - 50.0).abs() < 0.5);
    }

    #[test]
    fn editable_focus_inside_nested_scroller_takes_priority() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#scroller{height:100px;width:200px;overflow:auto}\
             #before{height:100px}#after{height:600px}</style>\
             <div id='scroller'><div id='before'></div><input id='focus'>\
             <div id='after'></div></div>",
        );
        let scroller = doc.query_selector("#scroller").unwrap();
        let before = doc.query_selector("#before").unwrap();
        let focus = doc.query_selector("#focus").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 300.0;
        engine.layout(&mut doc, 400.0);
        doc.element_scroll_to(scroller, 0.0, 80.0);
        doc.focused_box = focus;
        let anchors = ElementAnchors::capture(&doc);
        assert_eq!(anchors.0.len(), 1);
        assert_eq!(anchors.0[0].anchor.node_id, focus);
        doc.set_style_property(before, "height", "130px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 110.0).abs() < 0.5);
    }

    #[test]
    fn viewport_scroll_padding_changes_anchor_selection() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}html{scroll-padding-top:40px}\
             #before{height:60px}#visible{height:20px}#after{height:1000px}</style>\
             <div id='before'></div><div id='visible'></div><div id='after'></div>",
        );
        let before = doc.query_selector("#before").unwrap();
        let visible = doc.query_selector("#visible").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 100.0;
        engine.layout(&mut doc, 400.0);
        doc.scroll_y = 30.0;
        assert_eq!(
            ViewportAnchor::capture(&doc, 400.0, 100.0).unwrap().node_id,
            visible
        );
        doc.set_style_property(before, "height", "90px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.scroll_y - 60.0).abs() < 0.5);
    }

    #[test]
    fn nested_scroll_padding_percent_changes_anchor_selection() {
        let mut doc = crate::html::parse_html(
            "<style>html,body{margin:0}#scroller{height:100px;width:200px;overflow:auto;\
             scroll-padding-top:40%}#before{height:60px}#visible{height:20px}\
             #after{height:500px}</style><div id='scroller'>\
             <div id='before'></div><div id='visible'></div><div id='after'></div></div>",
        );
        let scroller = doc.query_selector("#scroller").unwrap();
        let before = doc.query_selector("#before").unwrap();
        let visible = doc.query_selector("#visible").unwrap();
        let mut engine = LayoutEngine::new();
        engine.viewport_h = 300.0;
        engine.layout(&mut doc, 400.0);
        doc.element_scroll_to(scroller, 0.0, 30.0);
        let anchors = ElementAnchors::capture(&doc);
        assert_eq!(anchors.0.len(), 1);
        assert_eq!(anchors.0[0].anchor.node_id, visible);
        doc.set_style_property(before, "height", "90px");
        engine.layout(&mut doc, 400.0);
        assert!((doc.element_scroll_top(scroller) - 60.0).abs() < 0.5);
    }
}
