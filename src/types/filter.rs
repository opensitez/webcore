//! `filter` values.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Filter ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CssFilters {
    pub ops: Vec<FilterOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterOp {
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Grayscale(f32),
    HueRotate(f32),
    Invert(f32),
    Opacity(f32),
    Saturate(f32),
    Sepia(f32),
    DropShadow {
        dx: f32,
        dy: f32,
        blur: f32,
        color: Color,
    },
}
