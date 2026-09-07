//! `LayoutBox` — geometry computed by the layout pass.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Layout Box (geometry computed by the layout pass) ───────────────────────

/// Layout-only data for a box. Separated from DOM data so that each pipeline
/// stage owns its own data — better cache behavior, independent invalidation,
/// and the ability to have multiple layout views from one DOM.
#[derive(Clone, Debug)]
pub struct LayoutBox {
    // Box model geometry (set by layout pass)
    pub content_rect: Rect,
    pub padding_rect: Rect,
    pub border_rect: Rect,
    pub margin_rect: Rect,
    pub baseline: f32,

    // Cached line breaks for inline content
    pub line_cache: Vec<LayoutLine>,

    // Inline runs (set by CSS cascade pass)
    pub inline_runs: Vec<InlineRun>,

    // Collapsed margin pass-through (set by block layout)
    pub collapsed_margin_top: f32,
    pub collapsed_margin_bottom: f32,

    // Scroll extent (set by layout)
    pub scroll_height: f32,
    pub scroll_width: f32,
    pub scroll_top: f32,
    pub scroll_left: f32,

    /// Static x position for absolutely positioned elements (set during parent layout).
    pub abs_static_x: Option<f32>,
    /// Static y position for absolutely positioned elements (set during parent layout).
    pub abs_static_y: Option<f32>,

    // Dirty flags for incremental layout
    pub layout_dirty: bool,
    /// Intrinsic sizes need recomputation (propagates up to auto-width parents).
    pub intrinsic_dirty: bool,
    /// Paint-only change (color/background) — skip layout, just repaint.
    pub paint_dirty: bool,
    pub last_containing_width: f32,

    // Resolved box-model cache (set by layout, read by parent layout)
    pub resolved_margin_top: f32,
    pub resolved_margin_right: f32,
    pub resolved_margin_bottom: f32,
    pub resolved_margin_left: f32,
    pub resolved_border_top: f32,
    pub resolved_border_right: f32,
    pub resolved_border_bottom: f32,
    pub resolved_border_left: f32,
    pub resolved_pad_top: f32,
    pub resolved_pad_right: f32,
    pub resolved_pad_bottom: f32,
    pub resolved_pad_left: f32,
    pub resolved_content_width: f32,

    /// Cached intrinsic (max-content) width — `NAN` means not yet computed.
    pub cached_intrinsic_w: std::cell::Cell<f32>,
}

impl Default for LayoutBox {
    fn default() -> Self {
        Self {
            content_rect: Rect::default(),
            padding_rect: Rect::default(),
            border_rect: Rect::default(),
            margin_rect: Rect::default(),
            baseline: 0.0,
            line_cache: Vec::new(),
            inline_runs: Vec::new(),
            collapsed_margin_top: 0.0,
            collapsed_margin_bottom: 0.0,
            scroll_height: 0.0,
            scroll_width: 0.0,
            scroll_top: 0.0,
            scroll_left: 0.0,
            abs_static_x: None,
            abs_static_y: None,
            layout_dirty: false,
            intrinsic_dirty: false,
            paint_dirty: false,
            last_containing_width: 0.0,
            resolved_margin_top: 0.0,
            resolved_margin_right: 0.0,
            resolved_margin_bottom: 0.0,
            resolved_margin_left: 0.0,
            resolved_border_top: 0.0,
            resolved_border_right: 0.0,
            resolved_border_bottom: 0.0,
            resolved_border_left: 0.0,
            resolved_pad_top: 0.0,
            resolved_pad_right: 0.0,
            resolved_pad_bottom: 0.0,
            resolved_pad_left: 0.0,
            resolved_content_width: 0.0,
            cached_intrinsic_w: std::cell::Cell::new(f32::NAN),
        }
    }
}
