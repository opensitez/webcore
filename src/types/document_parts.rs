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
