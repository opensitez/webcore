//! A laid-out line of inline content.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Layout Line ──────────────────────────────────────────────────────────────

/// Result of line-breaking for a line in inline content.
#[derive(Clone, Debug, Default)]
pub struct LayoutLine {
    pub text_start: usize,
    pub text_length: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub ascent: f32,
    pub descent: f32,
    pub extra_space_per_word: f32, // for text-align: justify
    /// X offset from `self.x` where text content actually starts.
    /// Non-zero when atomic inline items (e.g. checkbox, image) precede text on this line.
    pub text_x_offset: f32,
    /// BiDi visual segments in visual order. Empty = pure LTR, use logical order.
    pub visual_segments: Vec<VisualSegment>,
    /// Per-character-boundary x positions relative to `self.x + text_x_offset`, in logical pixels.
    pub char_x: Vec<f32>,
    /// True when this is the final visible line of a clamped block and later
    /// line content was omitted.
    pub has_clamped_continuation: bool,
    /// Fingerprint of everything `char_x` was shaped from — the line's text,
    /// the styles of the runs covering it, the justification spacing and the
    /// device scale.
    ///
    /// ⛔ Filling `char_x` re-shapes the line through cosmic-text and was the
    /// single largest cost in a layout (72,805 profile samples on Wikipedia),
    /// repeated on every layout even when nothing about the line had changed.
    /// The existing early-stop could not help: it needs `old_line_idx > 0` and
    /// copies the whole TAIL, so the first line is always re-shaped and a block
    /// of one or two lines never benefits. `0` means "not computed".
    pub char_x_key: u64,
}
