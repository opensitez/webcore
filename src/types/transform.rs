//! `transform` values.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Transform ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssTransform {
    pub ops: Vec<TransformOp>,
}

/// What a transform needs to resolve its lengths: the element's own font size
/// and the root's (for `em`/`rem`) and the viewport (for `vw`/`vh`). The
/// reference box comes separately, as the rect passed alongside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformCtx {
    pub font_px: f32,
    pub root_font_px: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
}
impl Default for TransformCtx {
    fn default() -> Self {
        Self {
            font_px: 16.0,
            root_font_px: 16.0,
            viewport_w: 0.0,
            viewport_h: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TransformOp {
    // ⛔ LENGTHS, not resolved pixels. A `translate()` argument is a
    // `<length-percentage>`, and its percentage refers to the REFERENCE BOX
    // (width for X, height for Y) — css-transforms-1 §transform-property — so
    // it cannot be resolved at parse time, when no box exists. Resolving with
    // a zero containing size and a zero viewport made `translate(-50%, -50%)`
    // move the element by exactly (0, 0), which mis-places every dialog,
    // tooltip and hero that centres itself with the standard idiom.
    Translate(CssLength, CssLength),
    TranslateX(CssLength),
    TranslateY(CssLength),
    Scale(f32, f32),
    ScaleX(f32),
    ScaleY(f32),
    Rotate(f32),                          // degrees
    SkewX(f32),                           // degrees
    SkewY(f32),                           // degrees
    Matrix(f32, f32, f32, f32, f32, f32), // a b c d e f
}
