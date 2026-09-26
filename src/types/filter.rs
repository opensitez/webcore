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

impl CssFilters {
    /// Resolved filter lengths serialize in pixels, so explicit inheritance
    /// cannot reinterpret the parent's relative units in the child's context.
    pub(crate) fn to_css(&self) -> String {
        if self.ops.is_empty() { return "none".into(); }
        self.ops.iter().map(|op| match op {
            FilterOp::Blur(v) => format!("blur({v}px)"),
            FilterOp::Brightness(v) => format!("brightness({v})"),
            FilterOp::Contrast(v) => format!("contrast({v})"),
            FilterOp::Grayscale(v) => format!("grayscale({v})"),
            FilterOp::HueRotate(v) => format!("hue-rotate({v}deg)"),
            FilterOp::Invert(v) => format!("invert({v})"),
            FilterOp::Opacity(v) => format!("opacity({v})"),
            FilterOp::Saturate(v) => format!("saturate({v})"),
            FilterOp::Sepia(v) => format!("sepia({v})"),
            FilterOp::DropShadow {dx, dy, blur, color} => {
                let color = if color.a == 255 {
                    format!("rgb({}, {}, {})", color.r, color.g, color.b)
                } else {
                    format!("rgba({}, {}, {}, {})", color.r, color.g, color.b, color.a as f32 / 255.0)
                };
                format!("drop-shadow({color} {dx}px {dy}px {blur}px)")
            }
        }).collect::<Vec<_>>().join(" ")
    }
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
