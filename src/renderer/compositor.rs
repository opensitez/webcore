//! Compositor Layer Tree — separates painting from compositing.
//!
//! Elements with transform, opacity, position:fixed, overflow:scroll, or
//! will-change get their own compositing layer. The compositor handles:
//! - Scroll: update layer offset, composite (no repaint)
//! - Transform: update layer transform, composite (no repaint)
//! - Opacity: update layer opacity, composite (no repaint)
//!
//! This means scrolling is always instant — we just move pre-rasterized
//! tiles around. Only content changes trigger rasterization.

use crate::types::{Rect, WebCore};

/// Unique layer identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LayerId(pub u32);

/// A compositing layer — owns a portion of the display that can be
/// independently scrolled, transformed, or opacity-adjusted.
#[derive(Clone, Debug)]
pub struct CompositorLayer {
    pub id: LayerId,
    /// DOM node that created this layer (for hit-testing).
    pub node_id: u32,
    /// Bounds in parent layer coordinates.
    pub bounds: Rect,
    /// Scroll offset — applied to all content in this layer.
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// Maximum scroll extent.
    pub scroll_width: f32,
    pub scroll_height: f32,
    /// Transform relative to parent layer (2D affine: [a, b, c, d, e, f]).
    pub transform: [f32; 6],
    /// Opacity (0.0 = transparent, 1.0 = opaque).
    pub opacity: f32,
    /// Whether this layer's content needs re-rasterization.
    pub needs_raster: bool,
    /// Whether this layer clips its children (overflow:hidden/scroll/auto).
    pub clips: bool,
    /// Child layers (painted on top of this layer).
    pub children: Vec<LayerId>,
    /// Reason this layer was created (for debugging).
    pub reason: LayerReason,
}

/// Why a layer was created — for debugging and optimization.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerReason {
    /// Root layer (the document).
    Root,
    /// position:fixed — stays in place during scroll.
    Fixed,
    /// overflow:scroll/auto — content scrolls independently.
    ScrollContainer,
    /// CSS transform (non-identity).
    Transform,
    /// opacity < 1.0.
    Opacity,
    /// will-change hint.
    WillChange,
    /// CSS filter.
    Filter,
}

impl CompositorLayer {
    pub fn new(id: LayerId, node_id: u32, bounds: Rect, reason: LayerReason) -> Self {
        Self {
            id,
            node_id,
            bounds,
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_width: 0.0,
            scroll_height: 0.0,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], // identity
            opacity: 1.0,
            needs_raster: true,
            clips: false,
            children: Vec::new(),
            reason,
        }
    }

    /// Is this the identity transform?
    pub fn has_transform(&self) -> bool {
        let [a, b, c, d, e, f] = self.transform;
        (a - 1.0).abs() > 0.001
            || b.abs() > 0.001
            || c.abs() > 0.001
            || (d - 1.0).abs() > 0.001
            || e.abs() > 0.001
            || f.abs() > 0.001
    }

    /// Is this layer scrollable?
    pub fn is_scrollable(&self) -> bool {
        self.scroll_height > self.bounds.h || self.scroll_width > self.bounds.w
    }
}

/// The compositor manages all layers and handles compositing.
#[derive(Clone, Debug)]
pub struct Compositor {
    /// All layers, indexed by LayerId.
    layers: Vec<CompositorLayer>,
    /// Root layer (always layers[0]).
    root: LayerId,
    /// Next layer ID to assign.
    next_id: u32,
}

impl Compositor {
    pub fn new() -> Self {
        let root = CompositorLayer::new(
            LayerId(0),
            0,
            Rect::new(0.0, 0.0, 0.0, 0.0),
            LayerReason::Root,
        );
        Self {
            layers: vec![root],
            root: LayerId(0),
            next_id: 1,
        }
    }

    /// Build the layer tree from the DOM after layout.
    /// Walks the DOM tree and creates layers for elements that need them.
    pub fn build_layers(&mut self, root: &WebCore, viewport_w: f32, viewport_h: f32) {
        self.layers.clear();
        self.next_id = 0;

        // Root layer covers the viewport
        let root_layer = self.alloc_layer(
            0,
            Rect::new(0.0, 0.0, viewport_w, viewport_h),
            LayerReason::Root,
        );
        self.root = root_layer;

        // Walk DOM and create child layers
        self.build_layers_walk(root, root_layer);
    }

    fn build_layers_walk(&mut self, node: &WebCore, parent_layer: LayerId) {
        use crate::types::*;

        if matches!(node.style.display, Display::None) {
            return;
        }

        let needs_layer = self.needs_own_layer(node);

        let current_layer = if let Some(reason) = needs_layer {
            let layer = self.alloc_layer(node.node_id, node.layout.border_rect, reason);

            // Set layer properties from node style
            if let Some(l) = self.get_mut(layer) {
                l.opacity = node.style.opacity;
                l.clips = matches!(
                    node.style.overflow_x,
                    Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
                ) || matches!(
                    node.style.overflow_y,
                    Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
                );

                if l.clips {
                    l.scroll_width = node.layout.scroll_width;
                    l.scroll_height = node.layout.scroll_height;
                }

                // Parse transform
                if !node.style.transform.is_empty() {
                    // Basic transform support — extract translate/scale
                    // Full matrix parsing would go here
                    l.transform = parse_transform_basic(
                        &node.style.transform,
                        node.layout.border_rect.w,
                        node.layout.border_rect.h,
                    );
                }
            }

            // Add as child of parent
            if let Some(parent) = self.get_mut(parent_layer) {
                parent.children.push(layer);
            }
            layer
        } else {
            parent_layer
        };

        // Recurse into children
        for child in &node.children {
            self.build_layers_walk(child, current_layer);
        }
    }

    /// Determine if a node needs its own compositing layer.
    fn needs_own_layer(&self, node: &WebCore) -> Option<LayerReason> {
        use crate::types::*;

        // position:fixed — always gets own layer
        if node.style.position == Position::Fixed {
            return Some(LayerReason::Fixed);
        }

        // overflow:scroll/auto — scroll container
        if matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto)
            || matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto)
        {
            return Some(LayerReason::ScrollContainer);
        }

        // CSS transform
        if !node.style.transform.is_empty() {
            return Some(LayerReason::Transform);
        }

        // opacity < 1.0
        if node.style.opacity < 0.999 {
            return Some(LayerReason::Opacity);
        }

        // CSS filter
        if !node.style.rare().filter.is_empty() {
            return Some(LayerReason::Filter);
        }

        None
    }

    /// Allocate a new layer.
    fn alloc_layer(&mut self, node_id: u32, bounds: Rect, reason: LayerReason) -> LayerId {
        let id = LayerId(self.next_id);
        self.next_id += 1;
        self.layers
            .push(CompositorLayer::new(id, node_id, bounds, reason));
        id
    }

    /// Get a layer by ID.
    pub fn get(&self, id: LayerId) -> Option<&CompositorLayer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// Get a mutable layer by ID.
    pub fn get_mut(&mut self, id: LayerId) -> Option<&mut CompositorLayer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Get the root layer.
    pub fn root_layer(&self) -> &CompositorLayer {
        self.get(self.root).unwrap()
    }

    /// Number of layers.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Update scroll offset for a layer. Returns true if changed.
    /// This is a compositor-only operation — no layout or paint needed.
    pub fn scroll_layer(&mut self, layer_id: LayerId, dx: f32, dy: f32) -> bool {
        if let Some(layer) = self.get_mut(layer_id) {
            let max_x = (layer.scroll_width - layer.bounds.w).max(0.0);
            let max_y = (layer.scroll_height - layer.bounds.h).max(0.0);
            let new_x = (layer.scroll_x + dx).max(0.0).min(max_x);
            let new_y = (layer.scroll_y + dy).max(0.0).min(max_y);
            if (new_x - layer.scroll_x).abs() > 0.01 || (new_y - layer.scroll_y).abs() > 0.01 {
                layer.scroll_x = new_x;
                layer.scroll_y = new_y;
                return true;
            }
        }
        false
    }

    /// Update opacity for a layer. Returns true if changed.
    /// Compositor-only — no repaint needed.
    pub fn set_layer_opacity(&mut self, layer_id: LayerId, opacity: f32) -> bool {
        if let Some(layer) = self.get_mut(layer_id) {
            if (layer.opacity - opacity).abs() > 0.001 {
                layer.opacity = opacity;
                return true;
            }
        }
        false
    }

    /// Find the layer containing a document-space point.
    /// Used for scroll routing — determines which scroll container handles a scroll event.
    pub fn hit_test_layer(&self, doc_x: f32, doc_y: f32) -> LayerId {
        self.hit_test_walk(self.root, doc_x, doc_y)
    }

    fn hit_test_walk(&self, layer_id: LayerId, x: f32, y: f32) -> LayerId {
        if let Some(layer) = self.get(layer_id) {
            // Check children in reverse order (topmost first)
            for &child_id in layer.children.iter().rev() {
                let result = self.hit_test_walk(child_id, x, y);
                if result != self.root {
                    return result;
                }
            }
            // Check this layer
            let lx = x - layer.bounds.x + layer.scroll_x;
            let ly = y - layer.bounds.y + layer.scroll_y;
            if lx >= 0.0 && ly >= 0.0 && lx <= layer.bounds.w && ly <= layer.bounds.h {
                if layer.is_scrollable() {
                    return layer_id;
                }
            }
        }
        self.root
    }

    /// Mark all layers as needing re-rasterization (after layout change).
    pub fn invalidate_all(&mut self) {
        for layer in &mut self.layers {
            layer.needs_raster = true;
        }
    }

    /// Mark a specific layer as needing re-rasterization.
    pub fn invalidate_layer(&mut self, layer_id: LayerId) {
        if let Some(layer) = self.get_mut(layer_id) {
            layer.needs_raster = true;
        }
    }
}

impl Default for Compositor {
    fn default() -> Self {
        Self::new()
    }
}

/// Basic transform parsing — extracts translate and scale from CSS transform string.
fn parse_transform_basic(transform: &str, w: f32, h: f32) -> [f32; 6] {
    let mut result = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]; // identity

    for part in transform.split(')') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(args) = part
            .strip_prefix("translate(")
            .or_else(|| part.strip_prefix("translateX("))
        {
            let vals: Vec<f32> = args
                .split(',')
                .enumerate()
                .filter_map(|(axis, s)| length(s, if axis == 0 { w } else { h }))
                .collect();
            if !vals.is_empty() {
                result[4] += vals[0];
            }
            if vals.len() > 1 {
                result[5] += vals[1];
            }
        } else if let Some(args) = part.strip_prefix("translateY(") {
            if let Some(v) = length(args, h) {
                result[5] += v;
            }
        } else if let Some(args) = part.strip_prefix("scale(") {
            let vals: Vec<f32> = args
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if !vals.is_empty() {
                result[0] *= vals[0];
                result[3] *= vals.get(1).copied().unwrap_or(vals[0]);
            }
        }
    }

    result
}

/// One `<length-percentage>` from a transform function.
///
/// **A percentage translate resolves against the element's OWN box** — the
/// reference box, per css-transforms-1 §3: `translateX(50%)` is half this
/// element's width, not half the viewport's. That is what the `w`/`h`
/// parameters are for, and this is the piece that was missing: they were
/// accepted, passed down from the layer builder, and never read, so every
/// percentage translate silently parsed as nothing and the element did not
/// move at all.
fn length(text: &str, reference: f32) -> Option<f32> {
    let text = text.trim();
    match text.strip_suffix('%') {
        Some(pct) => pct
            .trim()
            .parse::<f32>()
            .ok()
            .map(|p| p / 100.0 * reference),
        None => text.trim_end_matches("px").trim().parse::<f32>().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_html;

    #[test]
    fn a_percentage_translate_resolves_against_the_elements_own_box() {
        // css-transforms-1 §3 — the reference box is this element's border box.
        let t = parse_transform_basic("translate(50%, 25%)", 200.0, 80.0);
        assert_eq!(t[4], 100.0);
        assert_eq!(t[5], 20.0);
        // Pixels keep working, and the two spellings can be mixed.
        let t = parse_transform_basic("translate(10px, 50%)", 200.0, 80.0);
        assert_eq!(t[4], 10.0);
        assert_eq!(t[5], 40.0);
    }

    #[test]
    fn compositor_basic() {
        let comp = Compositor::new();
        assert_eq!(comp.layer_count(), 1); // root layer
    }

    #[test]
    fn compositor_builds_layers() {
        let doc = load_html(
            concat!(
                "<div style='position:fixed;top:0;left:0;width:100px;height:50px'>Fixed</div>",
                "<div style='overflow:auto;height:200px'><div style='height:1000px'>Scroll</div></div>",
                "<div style='opacity:0.5'>Semi</div>",
            ),
            800.0,
        );
        let mut comp = Compositor::new();
        comp.build_layers(&doc.root, 800.0, 600.0);
        // Should have: root + fixed + scroll + opacity = 4 layers
        assert!(
            comp.layer_count() >= 4,
            "expected >=4 layers, got {}",
            comp.layer_count()
        );
    }

    #[test]
    fn compositor_scroll() {
        let mut comp = Compositor::new();
        let layer = comp.alloc_layer(
            1,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            LayerReason::ScrollContainer,
        );
        if let Some(l) = comp.get_mut(layer) {
            l.scroll_height = 500.0;
        }
        assert!(comp.scroll_layer(layer, 0.0, 50.0));
        assert_eq!(comp.get(layer).unwrap().scroll_y, 50.0);
        // Clamp to max
        comp.scroll_layer(layer, 0.0, 500.0);
        assert_eq!(comp.get(layer).unwrap().scroll_y, 400.0); // 500 - 100
    }

    #[test]
    fn parse_transform_translate() {
        let t = parse_transform_basic("translate(10px, 20px)", 100.0, 100.0);
        assert_eq!(t[4], 10.0);
        assert_eq!(t[5], 20.0);
    }

    #[test]
    fn parse_transform_scale() {
        let t = parse_transform_basic("scale(2)", 100.0, 100.0);
        assert_eq!(t[0], 2.0);
        assert_eq!(t[3], 2.0);
    }
}
