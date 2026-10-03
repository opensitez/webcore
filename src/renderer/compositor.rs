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

use super::display_list::{DisplayList, PaintCmd};
use super::tiles::TileManager;
use crate::types::{Rect, WebCore};

pub struct PaintSegment {
    pub list: DisplayList,
    pub fixed: bool,
    pub backdrop_dependent: bool,
    pub tiles: TileManager,
    pub fixed_surface: Option<FixedSurface>,
}

impl PaintSegment {
    pub fn has_animated_transform(
        &self,
        overrides: &std::collections::HashMap<u32, [f32; 6]>,
    ) -> bool {
        self.list.commands.iter().any(|cmd| {
            matches!(cmd, PaintCmd::PushTransform { node_id, .. } if overrides.contains_key(node_id))
        })
    }
}

/// A viewport layer cropped to the pixels that can affect compositing.
pub struct FixedSurface {
    pub image: tiny_skia::Pixmap,
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    viewport_width: u32,
    viewport_height: u32,
}

impl FixedSurface {
    pub fn from_viewport(surface: tiny_skia::Pixmap, scale: f32) -> Self {
        let viewport_width = surface.width();
        let viewport_height = surface.height();
        let mut left = viewport_width;
        let mut top = viewport_height;
        let mut right = 0;
        let mut bottom = 0;
        for (index, pixel) in surface.data().chunks_exact(4).enumerate() {
            if pixel[3] != 0 {
                let x = index as u32 % viewport_width;
                let y = index as u32 / viewport_width;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
        if right == viewport_width && bottom == viewport_height && left == 0 && top == 0 {
            return Self {
                image: surface,
                x: 0,
                y: 0,
                scale,
                viewport_width,
                viewport_height,
            };
        }
        if right <= left || bottom <= top {
            return Self {
                image: tiny_skia::Pixmap::new(1, 1).expect("one-pixel fixed surface"),
                x: 0,
                y: 0,
                scale,
                viewport_width,
                viewport_height,
            };
        }
        let width = right - left;
        let height = bottom - top;
        let mut image = tiny_skia::Pixmap::new(width, height).expect("bounded fixed surface");
        let row_bytes = width as usize * 4;
        for row in 0..height as usize {
            let src = ((top as usize + row) * viewport_width as usize + left as usize) * 4;
            let dst = row * row_bytes;
            image.data_mut()[dst..dst + row_bytes]
                .copy_from_slice(&surface.data()[src..src + row_bytes]);
        }
        Self {
            image,
            x: left as i32,
            y: top as i32,
            scale,
            viewport_width,
            viewport_height,
        }
    }

    pub fn matches(&self, width: u32, height: u32, scale: f32) -> bool {
        self.viewport_width == width
            && self.viewport_height == height
            && (self.scale - scale).abs() < 0.001
    }

    pub fn composite(&self, target: &mut tiny_skia::Pixmap) {
        target.draw_pixmap(
            self.x,
            self.y,
            self.image.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
}

/// Ordered document and viewport paint layers. Splitting at fixed-position
/// blocks preserves stacking order while document layers retain raster tiles
/// across scrolls.
pub struct PaintSegments {
    pub segments: Vec<PaintSegment>,
}

impl PaintSegments {
    pub fn from_display_list(list: &DisplayList, viewport_w: f32, doc_h: f32) -> Option<Self> {
        let mut inside_fixed = false;
        let mut has_fixed = false;
        let mut effect_depth = 0i32;
        let mut clips = Vec::new();
        for cmd in &list.commands {
            match cmd {
                PaintCmd::BeginFixedPosition => {
                    if inside_fixed
                        || effect_depth != 0
                        || clips.iter().any(|rect: &Rect| {
                            rect.x > 0.0
                                || rect.y > 0.0
                                || rect.right() < viewport_w
                                || rect.bottom() + 0.5 < doc_h
                        })
                    {
                        return None;
                    }
                    inside_fixed = true;
                    has_fixed = true;
                }
                PaintCmd::EndFixedPosition => {
                    if !inside_fixed {
                        return None;
                    }
                    inside_fixed = false;
                }
                _ if inside_fixed => {}
                PaintCmd::PushClip { rect, .. } => clips.push(*rect),
                PaintCmd::PopClip => {
                    clips.pop();
                }
                PaintCmd::PushClipPath { .. }
                | PaintCmd::PushClipSvgPath { .. }
                | PaintCmd::PushTransform { .. }
                | PaintCmd::PushOpacity { .. }
                | PaintCmd::PushFilter { .. }
                | PaintCmd::PushMask { .. }
                | PaintCmd::PushMaskGroup { .. }
                | PaintCmd::PushBlendMode { .. }
                | PaintCmd::PushTextGradient { .. } => effect_depth += 1,
                PaintCmd::PopTransform
                | PaintCmd::PopOpacity
                | PaintCmd::PopFilter
                | PaintCmd::PopMask
                | PaintCmd::PopBlendMode
                | PaintCmd::PopTextGradient => effect_depth -= 1,
                _ => {}
            }
        }
        if !has_fixed || inside_fixed || effect_depth != 0 {
            return None;
        }

        let mut kinds = vec![false];
        let mut owners = Vec::with_capacity(list.commands.len());
        let mut owner = 0usize;
        for cmd in &list.commands {
            match cmd {
                PaintCmd::BeginFixedPosition => {
                    kinds.push(true);
                    owner = kinds.len() - 1;
                    owners.push(Some(owner));
                }
                PaintCmd::EndFixedPosition => {
                    owners.push(Some(owner));
                    kinds.push(false);
                    owner = kinds.len() - 1;
                }
                _ if kinds[owner] || !paint_segment_structure(cmd) => owners.push(Some(owner)),
                _ => owners.push(None),
            }
        }
        let mut segments: Vec<_> = kinds
            .into_iter()
            .map(|fixed| PaintSegment {
                list: DisplayList::new(),
                fixed,
                backdrop_dependent: false,
                tiles: TileManager::new(),
                fixed_surface: None,
            })
            .collect();
        for (cmd, owner) in list.commands.iter().zip(owners) {
            if let Some(index) = owner {
                segments[index].list.push(cmd.clone());
            } else {
                for segment in &mut segments {
                    segment.list.push(cmd.clone());
                }
            }
        }
        segments.retain(|segment| {
            segment
                .list
                .commands
                .iter()
                .any(|cmd| !paint_segment_structure(cmd))
        });
        for segment in &mut segments {
            segment.backdrop_dependent = segment.list.commands.iter().any(|cmd| {
                matches!(
                    cmd,
                    PaintCmd::BackdropFilter { .. } | PaintCmd::PushBlendMode { .. }
                )
            });
        }
        Some(Self { segments })
    }

    pub fn invalidate_all(&mut self) {
        for segment in &mut self.segments {
            if segment.fixed {
                segment.fixed_surface = None;
            } else {
                segment.tiles.invalidate_all();
            }
        }
    }

    pub fn invalidate_rect(&mut self, rect: &Rect) {
        for segment in &mut self.segments {
            if segment.fixed {
                segment.fixed_surface = None;
            } else {
                segment.tiles.invalidate_rect(rect);
            }
        }
    }

    /// Reuse identical paint layers without retaining stale backdrop effects.
    pub fn retain_unchanged_rasters(&mut self, previous: Self) -> usize {
        if self.segments.len() != previous.segments.len()
            || self
                .segments
                .iter()
                .zip(&previous.segments)
                .any(|(new, old)| new.fixed != old.fixed)
        {
            return 0;
        }

        let mut retained = 0;
        let mut backdrop_changed = false;
        for (new, old) in self.segments.iter_mut().zip(previous.segments) {
            let unchanged = new.list.commands == old.list.commands;
            if unchanged && !(new.backdrop_dependent && backdrop_changed) {
                new.tiles = old.tiles;
                new.fixed_surface = old.fixed_surface;
                retained += 1;
            } else {
                if !new.fixed
                    && !(new.backdrop_dependent && backdrop_changed)
                    && let Some(damage) = simple_paint_damage(&old.list, &new.list, old.tiles.scale)
                {
                    new.tiles = old.tiles;
                    for rect in damage {
                        new.tiles.invalidate_rect(&rect);
                    }
                    retained += 1;
                }
                backdrop_changed = true;
            }
        }
        retained
    }
}

/// Only paint commands with bounded damage can retain tiles outside their
/// changed bounds. Transforms and backdrop-dependent effects take the full path.
fn simple_paint_damage(old: &DisplayList, new: &DisplayList, scale: f32) -> Option<Vec<Rect>> {
    if old.commands.len() != new.commands.len() {
        return inserted_or_removed_paint_damage(old, new, scale);
    }
    let mut damage = Vec::new();
    for (before, after) in old.commands.iter().zip(&new.commands) {
        if before == after {
            if matches!(
                before,
                PaintCmd::PushTransform { .. }
                    | PaintCmd::PushFilter { .. }
                    | PaintCmd::PushMask { .. }
                    | PaintCmd::PushMaskGroup { .. }
                    | PaintCmd::PushBlendMode { .. }
                    | PaintCmd::PushTextGradient { .. }
                    | PaintCmd::BackdropFilter { .. }
            ) {
                return None;
            }
            continue;
        }
        damage.push(bounded_command_damage(before, scale)?);
        damage.push(bounded_command_damage(after, scale)?);
    }
    Some(damage)
}

fn inserted_or_removed_paint_damage(
    old: &DisplayList,
    new: &DisplayList,
    scale: f32,
) -> Option<Vec<Rect>> {
    let unsafe_effect = |cmd: &PaintCmd| {
        matches!(
            cmd,
            PaintCmd::PushTransform { .. }
                | PaintCmd::PushFilter { .. }
                | PaintCmd::PushMask { .. }
                | PaintCmd::PushMaskGroup { .. }
                | PaintCmd::PushBlendMode { .. }
                | PaintCmd::PushTextGradient { .. }
                | PaintCmd::BackdropFilter { .. }
        )
    };
    if old.commands.iter().chain(&new.commands).any(unsafe_effect) {
        return None;
    }

    let prefix = old
        .commands
        .iter()
        .zip(&new.commands)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old.commands[prefix..]
        .iter()
        .rev()
        .zip(new.commands[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_changed = &old.commands[prefix..old.commands.len() - suffix];
    let new_changed = &new.commands[prefix..new.commands.len() - suffix];
    let mut damage = Vec::with_capacity(old_changed.len() + new_changed.len());
    for cmd in old_changed.iter().chain(new_changed) {
        damage.push(bounded_command_damage(cmd, scale)?);
    }
    Some(damage)
}

fn bounded_command_damage(cmd: &PaintCmd, scale: f32) -> Option<Rect> {
    let (rect, inflate) = match cmd {
        PaintCmd::FillRect { rect, .. } => (*rect, 1.0),
        PaintCmd::Border { rect, .. }
        | PaintCmd::BorderImage { rect, .. }
        | PaintCmd::Image { rect, .. }
        | PaintCmd::ResizeGrip { rect, .. } => (*rect, 2.0),
        PaintCmd::Gradient { clip, .. } | PaintCmd::BackgroundImage { clip, .. } => (*clip, 2.0),
        PaintCmd::Outline { rect, width, .. } => (*rect, width.max(0.0) * 0.5 + 2.0),
        PaintCmd::BoxShadow {
            rect,
            offset_x,
            offset_y,
            blur,
            spread,
            inset,
            ..
        } => (
            box_shadow_damage_rect(*rect, *offset_x, *offset_y, *blur, *spread, *inset, scale),
            1.0,
        ),
        _ => return None,
    };
    Some(Rect::new(
        rect.x - inflate,
        rect.y - inflate,
        rect.w + inflate * 2.0,
        rect.h + inflate * 2.0,
    ))
}

fn box_shadow_damage_rect(
    rect: Rect,
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    inset: bool,
    scale: f32,
) -> Rect {
    if inset {
        return rect;
    }
    // Replay allocates a blurred shadow with 4 * blur + 4 device pixels of
    // padding. Include the original box because replay clears its interior.
    let blur_pad = (blur.max(0.0) * 4.0 + 4.0) / scale.max(0.001);
    let reach = spread.abs() + blur_pad;
    let left = rect.x.min(rect.x + offset_x) - reach;
    let top = rect.y.min(rect.y + offset_y) - reach;
    let right = rect.right().max(rect.right() + offset_x) + reach;
    let bottom = rect.bottom().max(rect.bottom() + offset_y) + reach;
    Rect::new(left, top, right - left, bottom - top)
}

fn paint_segment_structure(cmd: &PaintCmd) -> bool {
    matches!(
        cmd,
        PaintCmd::BeginFixedPosition
            | PaintCmd::EndFixedPosition
            | PaintCmd::PushClip { .. }
            | PaintCmd::PopClip
            | PaintCmd::PushClipPath { .. }
            | PaintCmd::PushClipSvgPath { .. }
            | PaintCmd::PushTransform { .. }
            | PaintCmd::PopTransform
            | PaintCmd::PushOpacity { .. }
            | PaintCmd::PopOpacity
            | PaintCmd::PushFilter { .. }
            | PaintCmd::PopFilter
            | PaintCmd::PushMask { .. }
            | PaintCmd::PushMaskGroup { .. }
            | PaintCmd::PopMask
            | PaintCmd::PushBlendMode { .. }
            | PaintCmd::PopBlendMode
            | PaintCmd::PushTextGradient { .. }
            | PaintCmd::PopTextGradient
            | PaintCmd::BeginStackingContext { .. }
            | PaintCmd::EndStackingContext
    )
}

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
    fn fixed_segment_ignores_unrelated_transform_animation() {
        let mut segment = PaintSegment {
            list: DisplayList::new(),
            fixed: true,
            backdrop_dependent: false,
            tiles: TileManager::new(),
            fixed_surface: None,
        };
        segment.list.push(PaintCmd::PushTransform {
            node_id: 7,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        });
        segment.list.push(PaintCmd::PopTransform);

        let mut overrides = std::collections::HashMap::new();
        overrides.insert(8, [1.0, 0.0, 0.0, 1.0, 10.0, 0.0]);
        assert!(!segment.has_animated_transform(&overrides));
        overrides.insert(7, [1.0, 0.0, 0.0, 1.0, 20.0, 0.0]);
        assert!(segment.has_animated_transform(&overrides));
    }

    #[test]
    fn cropped_fixed_surface_composites_identically_to_full_viewport() {
        let mut source = tiny_skia::Pixmap::new(100, 60).unwrap();
        for y in 8..18usize {
            for x in 30..50usize {
                let offset = (y * 100 + x) * 4;
                source.data_mut()[offset..offset + 4].copy_from_slice(&[80, 20, 10, 128]);
            }
        }
        let mut expected = tiny_skia::Pixmap::new(100, 60).unwrap();
        expected.fill(tiny_skia::Color::from_rgba8(10, 30, 90, 255));
        let mut actual = expected.clone();
        expected.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
        let retained = FixedSurface::from_viewport(source, 2.0);
        assert_eq!((retained.x, retained.y), (30, 8));
        assert_eq!((retained.image.width(), retained.image.height()), (20, 10));
        assert!(retained.matches(100, 60, 2.0));
        assert!(!retained.matches(100, 60, 1.0));
        retained.composite(&mut actual);
        assert_eq!(actual.data(), expected.data());

        let empty = FixedSurface::from_viewport(tiny_skia::Pixmap::new(100, 60).unwrap(), 1.0);
        assert_eq!((empty.image.width(), empty.image.height()), (1, 1));
        let before = actual.clone();
        empty.composite(&mut actual);
        assert_eq!(actual.data(), before.data());
    }

    fn two_layer_paint_list(
        document_color: crate::types::Color,
        fixed_color: crate::types::Color,
    ) -> DisplayList {
        let mut list = DisplayList::new();
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 200.0, 100.0),
            color: document_color,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            color: fixed_color,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);
        list
    }

    #[test]
    fn rebuilt_paint_segments_retain_only_unchanged_rasters() {
        use crate::types::Color;

        let list = two_layer_paint_list(Color::WHITE, Color::rgb(0, 0, 255));
        let mut previous = PaintSegments::from_display_list(&list, 200.0, 100.0).unwrap();
        let document = previous
            .segments
            .iter_mut()
            .find(|segment| !segment.fixed)
            .unwrap();
        document
            .tiles
            .update_viewport(Rect::new(0.0, 0.0, 100.0, 100.0), 1.0);
        assert!(document.tiles.ensure_tile(0, 0));
        document.tiles.mark_clean(0, 0);
        previous
            .segments
            .iter_mut()
            .find(|segment| segment.fixed)
            .unwrap()
            .fixed_surface = Some(FixedSurface::from_viewport(
            tiny_skia::Pixmap::new(100, 100).unwrap(),
            1.0,
        ));

        let changed_fixed = two_layer_paint_list(Color::WHITE, Color::rgb(255, 0, 0));
        let mut rebuilt = PaintSegments::from_display_list(&changed_fixed, 200.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        let document = rebuilt
            .segments
            .iter()
            .find(|segment| !segment.fixed)
            .unwrap();
        assert!(!document.tiles.tiles.get(&(0, 0)).unwrap().dirty);
        assert!(
            rebuilt
                .segments
                .iter()
                .find(|segment| segment.fixed)
                .unwrap()
                .fixed_surface
                .is_none()
        );

        let changed_document = two_layer_paint_list(Color::rgb(0, 255, 0), Color::rgb(255, 0, 0));
        let mut rebuilt_again =
            PaintSegments::from_display_list(&changed_document, 200.0, 100.0).unwrap();
        rebuilt_again.retain_unchanged_rasters(rebuilt);
        assert!(
            rebuilt_again
                .segments
                .iter()
                .find(|segment| !segment.fixed)
                .unwrap()
                .tiles
                .tiles
                .get(&(0, 0))
                .unwrap()
                .dirty
        );
    }

    #[test]
    fn local_fill_change_keeps_distant_document_tiles_clean() {
        use crate::types::Color;

        let mut list = DisplayList::new();
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1200.0, 100.0),
            color: Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::FillRect {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0),
            color: Color::rgb(0, 0, 255),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::rgb(0, 0, 0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);
        let mut previous = PaintSegments::from_display_list(&list, 1200.0, 100.0).unwrap();
        let document = previous
            .segments
            .iter_mut()
            .find(|segment| !segment.fixed)
            .unwrap();
        for tx in [0, 1] {
            document.tiles.ensure_tile(tx, 0);
            document.tiles.mark_clean(tx, 0);
        }
        if let PaintCmd::FillRect { color, .. } = &mut list.commands[1] {
            *color = Color::rgb(255, 0, 0);
        }
        let mut rebuilt = PaintSegments::from_display_list(&list, 1200.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        let document = rebuilt
            .segments
            .iter()
            .find(|segment| !segment.fixed)
            .unwrap();
        assert!(document.tiles.tiles.get(&(0, 0)).unwrap().dirty);
        assert!(!document.tiles.tiles.get(&(1, 0)).unwrap().dirty);
    }

    #[test]
    fn bounded_change_inside_unchanged_opacity_group_keeps_distant_tiles() {
        use crate::types::Color;

        let mut list = DisplayList::new();
        list.push(PaintCmd::PushOpacity { alpha: 0.5 });
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1200.0, 100.0),
            color: Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::FillRect {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0),
            color: Color::rgb(0, 0, 255),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::PopOpacity);
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color::BLACK,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);
        let original = list.clone();
        let mut previous = PaintSegments::from_display_list(&list, 1200.0, 100.0).unwrap();
        let document = previous
            .segments
            .iter_mut()
            .find(|segment| !segment.fixed)
            .unwrap();
        for tx in [0, 1] {
            document.tiles.ensure_tile(tx, 0);
            document.tiles.mark_clean(tx, 0);
        }

        if let PaintCmd::FillRect { color, .. } = &mut list.commands[2] {
            *color = Color::rgb(255, 0, 0);
        }
        let mut rebuilt = PaintSegments::from_display_list(&list, 1200.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        let document = rebuilt
            .segments
            .iter()
            .find(|segment| !segment.fixed)
            .unwrap();
        assert!(document.tiles.tiles.get(&(0, 0)).unwrap().dirty);
        assert!(!document.tiles.tiles.get(&(1, 0)).unwrap().dirty);

        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        for (tile_x, changed) in [(0.0, true), (512.0, false)] {
            let mut before = tiny_skia::Pixmap::new(512, 512).unwrap();
            let mut after = tiny_skia::Pixmap::new(512, 512).unwrap();
            crate::renderer::display_list_replay::replay_tile_with_scroll_and_transform_overrides(
                &original,
                &mut before,
                1.0,
                &mut fonts,
                &mut glyphs,
                tile_x,
                0.0,
                0.0,
                0.0,
                None,
            );
            crate::renderer::display_list_replay::replay_tile_with_scroll_and_transform_overrides(
                &list,
                &mut after,
                1.0,
                &mut fonts,
                &mut glyphs,
                tile_x,
                0.0,
                0.0,
                0.0,
                None,
            );
            assert_eq!(before.data() != after.data(), changed, "tile x={tile_x}");
        }

        let mut changed_alpha = list.clone();
        changed_alpha.commands[0] = PaintCmd::PushOpacity { alpha: 0.8 };
        assert!(simple_paint_damage(&list, &changed_alpha, 1.0).is_none());

        let mut inserted = list.clone();
        inserted.commands.insert(
            3,
            PaintCmd::FillRect {
                rect: Rect::new(70.0, 10.0, 20.0, 20.0),
                color: Color::BLACK,
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
        );
        let damage = simple_paint_damage(&list, &inserted, 1.0).unwrap();
        assert_eq!(damage.len(), 1);
        assert!(damage[0].right() < 512.0);
    }

    #[test]
    fn transformed_paint_change_does_not_retain_tiles_by_untransformed_bounds() {
        use crate::types::Color;

        let mut before = DisplayList::new();
        before.push(PaintCmd::PushTransform {
            node_id: 1,
            transform: [1.0, 0.0, 0.0, 1.0, 700.0, 0.0],
        });
        before.push(PaintCmd::FillRect {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0),
            color: Color::rgb(0, 0, 255),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        before.push(PaintCmd::PopTransform);
        let mut after = DisplayList::new();
        after.commands = before.commands.clone();
        if let PaintCmd::FillRect { color, .. } = &mut after.commands[1] {
            *color = Color::rgb(255, 0, 0);
        }
        assert!(simple_paint_damage(&before, &after, 1.0).is_none());
    }

    #[test]
    fn local_shadow_change_invalidates_blur_reach_but_retains_distant_tiles() {
        use crate::types::Color;

        let mut list = DisplayList::new();
        list.push(PaintCmd::BoxShadow {
            rect: Rect::new(505.0, 20.0, 20.0, 20.0),
            color: Color::rgb(0, 0, 255),
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 3.0,
            spread: 0.0,
            inset: false,
            radii: [0.0; 4],
            radii_y: [0.0; 4],
        });
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);
        let mut previous = PaintSegments::from_display_list(&list, 1600.0, 100.0).unwrap();
        let document = previous
            .segments
            .iter_mut()
            .find(|segment| !segment.fixed)
            .unwrap();
        document
            .tiles
            .update_viewport(Rect::new(0.0, 0.0, 1600.0, 100.0), 1.0);
        for tx in 0..3 {
            document.tiles.ensure_tile(tx, 0);
            document.tiles.mark_clean(tx, 0);
        }
        if let PaintCmd::BoxShadow { color, .. } = &mut list.commands[0] {
            *color = Color::rgb(255, 0, 0);
        }
        let mut rebuilt = PaintSegments::from_display_list(&list, 1600.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        let document = rebuilt
            .segments
            .iter()
            .find(|segment| !segment.fixed)
            .unwrap();
        assert!(document.tiles.tiles.get(&(0, 0)).unwrap().dirty);
        assert!(document.tiles.tiles.get(&(1, 0)).unwrap().dirty);
        assert!(!document.tiles.tiles.get(&(2, 0)).unwrap().dirty);
    }

    #[test]
    fn inserted_bounded_paint_keeps_unaffected_document_tiles() {
        use crate::types::Color;

        let mut list = DisplayList::new();
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1600.0, 100.0),
            color: Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color::BLACK,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);
        let mut previous = PaintSegments::from_display_list(&list, 1600.0, 100.0).unwrap();
        let document = previous
            .segments
            .iter_mut()
            .find(|segment| !segment.fixed)
            .unwrap();
        document
            .tiles
            .update_viewport(Rect::new(0.0, 0.0, 1600.0, 100.0), 1.0);
        for tx in 0..3 {
            document.tiles.ensure_tile(tx, 0);
            document.tiles.mark_clean(tx, 0);
        }

        list.commands.insert(
            1,
            PaintCmd::FillRect {
                rect: Rect::new(600.0, 10.0, 20.0, 20.0),
                color: Color::rgb(255, 0, 0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
        );
        let mut rebuilt = PaintSegments::from_display_list(&list, 1600.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        let document = rebuilt
            .segments
            .iter()
            .find(|segment| !segment.fixed)
            .unwrap();
        assert!(!document.tiles.tiles.get(&(0, 0)).unwrap().dirty);
        assert!(document.tiles.tiles.get(&(1, 0)).unwrap().dirty);
        assert!(!document.tiles.tiles.get(&(2, 0)).unwrap().dirty);
    }

    #[test]
    fn structural_damage_rejects_effect_layers_and_unbounded_commands() {
        use crate::types::Color;

        let fill = PaintCmd::FillRect {
            rect: Rect::new(600.0, 10.0, 20.0, 20.0),
            color: Color::rgb(255, 0, 0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        };
        let mut before = DisplayList::new();
        before.push(PaintCmd::PushTransform {
            node_id: 1,
            transform: [1.0, 0.0, 0.0, 1.0, 500.0, 0.0],
        });
        before.push(PaintCmd::PopTransform);
        let mut after = DisplayList::new();
        after.commands = before.commands.clone();
        after.commands.insert(1, fill.clone());
        assert!(simple_paint_damage(&before, &after, 1.0).is_none());

        before.commands.clear();
        before.push(PaintCmd::BackdropFilter {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            filters: Vec::new(),
        });
        after.commands = before.commands.clone();
        after.push(fill);
        assert!(simple_paint_damage(&before, &after, 1.0).is_none());
    }

    #[test]
    fn removed_bounded_paint_inside_unchanged_clip_has_local_damage() {
        use crate::types::Color;

        let mut before = DisplayList::new();
        before.push(PaintCmd::PushClip {
            rect: Rect::new(0.0, 0.0, 800.0, 100.0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        before.push(PaintCmd::FillRect {
            rect: Rect::new(550.0, 10.0, 20.0, 20.0),
            color: Color::BLACK,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        before.push(PaintCmd::PopClip);
        let mut after = DisplayList::new();
        after.commands = vec![before.commands[0].clone(), before.commands[2].clone()];
        let damage = simple_paint_damage(&before, &after, 1.0).expect("bounded removal");
        assert_eq!(damage.len(), 1);
        assert!(damage[0].x <= 550.0 && damage[0].right() >= 570.0);
        assert!(damage[0].right() < 600.0);
    }

    #[test]
    fn replacing_fill_with_clipped_gradient_retains_distant_tiles() {
        use crate::types::{Color, GradientDirection};

        let mut before = DisplayList::new();
        before.push(PaintCmd::FillRect {
            rect: Rect::new(600.0, 10.0, 30.0, 30.0),
            color: Color::BLACK,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        let mut after = DisplayList::new();
        after.push(PaintCmd::Gradient {
            rect: Rect::new(0.0, 0.0, 1200.0, 100.0),
            clip: Rect::new(600.0, 10.0, 30.0, 30.0),
            repeat_x_mode: 0,
            repeat_y_mode: 0,
            gradient_type: 1,
            angle: 0.0,
            direction: GradientDirection::default(),
            radial_center_x: 0.5,
            radial_center_y: 0.5,
            radial_radius_x: 0.5,
            radial_radius_y: 0.5,
            stops: vec![(Color::WHITE, 0.0), (Color::BLACK, 1.0)],
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            opacity: 1.0,
            blend_mode: 0,
        });
        let damage = simple_paint_damage(&before, &after, 1.0).expect("bounded replacement");
        assert_eq!(damage.len(), 2);
        assert!(
            damage
                .iter()
                .all(|rect| rect.x > 500.0 && rect.right() < 700.0)
        );
    }

    #[test]
    fn inserted_background_image_uses_paint_clip_not_positioning_area() {
        use crate::renderer::display_list::ImageRef;

        let before = DisplayList::new();
        let mut after = DisplayList::new();
        after.push(PaintCmd::BackgroundImage {
            container: Rect::new(0.0, 0.0, 1200.0, 100.0),
            clip: Rect::new(600.0, 10.0, 30.0, 30.0),
            data: ImageRef::Owned(vec![255, 255, 255, 255], 1, 1),
            size_mode: 0,
            draw_w: 1.0,
            draw_h: 1.0,
            pos_x: 0.0,
            pos_y: 0.0,
            repeat_x_mode: 1,
            repeat_y_mode: 1,
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            blend_mode: 0,
        });
        let damage = simple_paint_damage(&before, &after, 1.0).expect("bounded insertion");
        assert_eq!(damage.len(), 1);
        assert!(damage[0].x > 500.0 && damage[0].right() < 700.0);
    }

    #[test]
    fn outline_damage_includes_stroke_outside_border_box() {
        let damage = bounded_command_damage(
            &PaintCmd::Outline {
                rect: Rect::new(600.0, 10.0, 30.0, 30.0),
                width: 12.0,
                color: crate::types::Color::BLACK,
                style: 1,
                offset: 0.0,
                radii: [0.0; 4],
                radii_y: [0.0; 4],
            },
            1.0,
        )
        .unwrap();
        assert!(damage.x <= 594.0 && damage.right() >= 636.0);
    }

    #[test]
    fn changed_shadow_damage_includes_old_and_new_extents_at_device_scale() {
        let old = box_shadow_damage_rect(
            Rect::new(100.0, 10.0, 20.0, 20.0),
            0.0,
            0.0,
            4.0,
            0.0,
            false,
            2.0,
        );
        let new = box_shadow_damage_rect(
            Rect::new(700.0, 10.0, 20.0, 20.0),
            0.0,
            0.0,
            4.0,
            0.0,
            false,
            2.0,
        );
        assert!(old.x <= 90.0 && old.right() >= 130.0);
        assert!(new.x <= 690.0 && new.right() >= 730.0);
    }

    #[test]
    fn changed_backdrop_invalidates_an_unchanged_dependent_segment() {
        use crate::types::Color;

        let mut list = two_layer_paint_list(Color::WHITE, Color::rgb(0, 0, 255));
        list.push(PaintCmd::BackdropFilter {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            filters: vec![(0, 4.0, 0.0, 0.0, Color::WHITE)],
        });
        let mut previous = PaintSegments::from_display_list(&list, 200.0, 100.0).unwrap();
        let dependent = previous.segments.last_mut().unwrap();
        assert!(dependent.backdrop_dependent);
        dependent
            .tiles
            .update_viewport(Rect::new(0.0, 0.0, 100.0, 100.0), 1.0);
        dependent.tiles.ensure_tile(0, 0);
        dependent.tiles.mark_clean(0, 0);

        if let PaintCmd::FillRect { color, .. } = &mut list.commands[0] {
            *color = Color::rgb(255, 0, 0);
        }
        let mut rebuilt = PaintSegments::from_display_list(&list, 200.0, 100.0).unwrap();
        rebuilt.retain_unchanged_rasters(previous);
        assert!(rebuilt.segments.last().unwrap().tiles.tiles.is_empty());
    }

    #[test]
    fn empty_fixed_layers_do_not_create_paint_segments() {
        let mut list = DisplayList::new();
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 200.0, 100.0),
            color: crate::types::Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::EndFixedPosition);
        list.push(PaintCmd::BeginFixedPosition);
        list.push(PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            color: crate::types::Color::WHITE,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        list.push(PaintCmd::EndFixedPosition);

        let segments = PaintSegments::from_display_list(&list, 200.0, 100.0).unwrap();
        assert_eq!(segments.segments.len(), 2);
        assert_eq!(
            segments
                .segments
                .iter()
                .filter(|segment| segment.fixed)
                .count(),
            1
        );
    }

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
