//! Document-adjacent types.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Document ─────────────────────────────────────────────────────────────────

pub use crate::css::Stylesheet;
use crate::dom::{Editor, HtmlEvent, HtmlEventType};
use crate::layout::LayoutEngine;

pub const CSS_RESIZE_GRIP_PX: f32 = 12.0;

/// Active scrollbar drag state (set by `process_scrollbar_event`).
#[derive(Debug, Clone)]
pub struct ScrollbarDrag {
    /// Kind of scrollbar being dragged.
    pub kind: ScrollbarDragKind,
    /// Screen Y at the start of the drag.
    pub start_mouse_y: f32,
    /// Screen X at the start of the drag.
    pub start_mouse_x: f32,
    /// Scroll position at the start of the drag.
    pub start_scroll: f32,
    /// Pixels of scroll per pixel of mouse movement.
    pub scroll_per_px: f32,
}

/// User-initiated CSS resize, anchored to the size and pointer position at press.
#[derive(Debug, Clone)]
pub struct ResizeDrag {
    pub node_id: u32,
    pub axes: (bool, bool),
    pub start_mouse: (f32, f32),
    pub start_size: (f32, f32),
}

/// Which scrollbar is being dragged.
#[derive(Debug, Clone)]
pub enum ScrollbarDragKind {
    /// The viewport (document-level) vertical scrollbar.
    Viewport,
    /// A per-element vertical scrollbar; the element is identified by its stable node_id.
    ElementVertical(u32),
    /// A per-element horizontal scrollbar; the element is identified by its stable node_id.
    ElementHorizontal(u32),
}
