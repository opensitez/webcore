//! `CssLength` and the calc node.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Length ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum CssLength {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    /// Viewport-width percentage (1vw = 1% of viewport width).
    Vw(f32),
    /// Viewport-height percentage (1vh = 1% of viewport height).
    Vh(f32),
    /// `vmin` — 1% of the SMALLER viewport axis (CSS Values 4 §6.1.2).
    ///
    /// ⛔ Its own variant because it is not expressible as `Vw` or `Vh`: which
    /// axis it follows depends on the viewport's shape at resolve time. Both
    /// `vmin` and `vmax` used to parse to `Vw`, commented "approx" — which is
    /// simply the wrong axis on any landscape viewport, the common case.
    Vmin(f32),
    /// `vmax` — 1% of the LARGER viewport axis.
    Vmax(f32),
    // ── The four rare variants below are BOXED, and the reason is size ──
    // `CssLength` appears 53 times in `ComputedStyle`, so its width dominates:
    // an inline `Calc([f32; 6])` (24 bytes) or a three-Box `Clamp` (24 bytes)
    // made every length 32 bytes and `ComputedStyle` 3352. Every element owns
    // one, so a 100k-node page carried ~335 MB of style — and the cascade
    // recurses with several of them live per frame, which is what limited
    // nesting depth. Boxing the rare shapes costs one allocation on the few
    // lengths that use them and takes the common ones to 16 bytes.
    /// `calc()` — linear combination [percent, px, em, rem, vw, vh].
    Calc(Box<[f32; 6]>),
    /// `calc()` with non-linear parts (min/max nested inside calc).
    CalcExpr(Box<CalcNode>),
    /// `min()` — resolves to the smallest value.
    Min(Box<Vec<CssLength>>),
    /// `max()` — resolves to the largest value.
    Max(Box<Vec<CssLength>>),
    /// `clamp(min, val, max)` — resolves to val clamped between min and max.
    Clamp(Box<[CssLength; 3]>),
    Auto,
    Zero,
    /// `content` — size from the content, ignoring any specified size. Legal on
    /// `flex-basis` only (Flexbox §7.2.3); it is not a length and resolves to
    /// nothing, so the consumer has to branch on it.
    Content,
    /// The intrinsic sizing keywords (CSS Sizing §5). Like `content` they are
    /// not lengths — a consumer that cannot measure content treats them as
    /// `auto`, which is what `is_auto` reports, and one that CAN measure
    /// matches the variant first.
    MinContent,
    MaxContent,
    FitContent,
    /// `fit-content(<length-percentage>)` — the FUNCTION form, css-sizing-3
    /// §6.1, which is `max(min-content, min(max-content, <argument>))`. It
    /// carries an argument, so it cannot share the `FitContent` keyword's
    /// variant; like the keywords it is not a length, and a consumer that
    /// cannot measure content falls back to `auto`.
    FitContentArg(Box<CssLength>),
    None,
}

/// Expression node for calc() with nested min/max/clamp.
#[derive(Clone, Debug, PartialEq)]
pub enum CalcNode {
    Value(CssLength),
    Add(Box<CalcNode>, Box<CalcNode>),
    Sub(Box<CalcNode>, Box<CalcNode>),
    Mul(Box<CalcNode>, f32),
    Div(Box<CalcNode>, f32),
}

impl CalcNode {
    pub fn resolve_vp(
        &self,
        parent_font_px: f32,
        containing_px: f32,
        root_font_px: f32,
        vw: f32,
        vh: f32,
    ) -> f32 {
        match self {
            CalcNode::Value(v) => v.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh),
            CalcNode::Add(a, b) => {
                a.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh)
                    + b.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh)
            }
            CalcNode::Sub(a, b) => {
                a.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh)
                    - b.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh)
            }
            CalcNode::Mul(a, f) => {
                a.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh) * f
            }
            CalcNode::Div(a, f) => {
                if *f != 0.0 {
                    a.resolve_vp(parent_font_px, containing_px, root_font_px, vw, vh) / f
                } else {
                    0.0
                }
            }
        }
    }

    pub fn has_percentage(&self) -> bool {
        match self {
            CalcNode::Value(v) => v.has_percentage(),
            CalcNode::Add(a, b) | CalcNode::Sub(a, b) => a.has_percentage() || b.has_percentage(),
            CalcNode::Mul(a, _) | CalcNode::Div(a, _) => a.has_percentage(),
        }
    }
}

impl Default for CssLength {
    fn default() -> Self {
        Self::Auto
    }
}

impl CssLength {
    pub fn resolve(&self, parent_font_px: f32, containing_px: f32, root_font_px: f32) -> f32 {
        self.resolve_vp(parent_font_px, containing_px, root_font_px, 0.0, 0.0)
    }

    /// Resolve with explicit viewport dimensions for `vw`/`vh`.
    pub fn resolve_vp(
        &self,
        parent_font_px: f32,
        containing_px: f32,
        root_font_px: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> f32 {
        match self {
            CssLength::Px(v) => *v,
            CssLength::Em(v) => v * parent_font_px,
            CssLength::Rem(v) => v * root_font_px,
            CssLength::Percent(v) => v / 100.0 * containing_px,
            CssLength::Vw(v) => v / 100.0 * viewport_w,
            CssLength::Vh(v) => v / 100.0 * viewport_h,
            CssLength::Vmin(v) => v / 100.0 * viewport_w.min(viewport_h),
            CssLength::Vmax(v) => v / 100.0 * viewport_w.max(viewport_h),
            CssLength::Calc(c) => {
                c[0] / 100.0 * containing_px
                    + c[1]
                    + c[2] * parent_font_px
                    + c[3] * root_font_px
                    + c[4] / 100.0 * viewport_w
                    + c[5] / 100.0 * viewport_h
            }
            CssLength::CalcExpr(node) => node.resolve_vp(
                parent_font_px,
                containing_px,
                root_font_px,
                viewport_w,
                viewport_h,
            ),
            CssLength::Min(vals) => vals
                .iter()
                .map(|v| {
                    v.resolve_vp(
                        parent_font_px,
                        containing_px,
                        root_font_px,
                        viewport_w,
                        viewport_h,
                    )
                })
                .fold(f32::INFINITY, f32::min),
            CssLength::Max(vals) => vals
                .iter()
                .map(|v| {
                    v.resolve_vp(
                        parent_font_px,
                        containing_px,
                        root_font_px,
                        viewport_w,
                        viewport_h,
                    )
                })
                .fold(f32::NEG_INFINITY, f32::max),
            CssLength::Clamp(parts) => {
                let (min, val, max) = (&parts[0], &parts[1], &parts[2]);
                let min_v = min.resolve_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                );
                let val_v = val.resolve_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                );
                let max_v = max.resolve_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                );
                val_v.max(min_v).min(max_v)
            }
            CssLength::Auto => 0.0,
            // Not lengths. The flex algorithm reads these before it ever asks
            // for a resolved value.
            CssLength::Content => 0.0,
            CssLength::MinContent
            | CssLength::MaxContent
            | CssLength::FitContent
            | CssLength::FitContentArg(_) => 0.0,
            CssLength::Zero => 0.0,
            CssLength::None => 0.0,
        }
    }

    /// ⛔ Reports `true` for the intrinsic keywords as well. They are not
    /// lengths, and every caller that cannot measure content — block, table and
    /// inline layout — has to fall back to automatic sizing rather than resolve
    /// them to zero. A caller that CAN measure matches the variant before
    /// asking this.
    pub fn is_auto(&self) -> bool {
        matches!(
            self,
            CssLength::Auto
                | CssLength::MinContent
                | CssLength::MaxContent
                | CssLength::FitContent
                | CssLength::FitContentArg(_)
        )
    }
    /// The intrinsic sizing keyword this length names, if it is one.
    pub fn intrinsic(&self) -> Option<CssLength> {
        match self {
            CssLength::MinContent
            | CssLength::MaxContent
            | CssLength::FitContent
            | CssLength::FitContentArg(_) => Some(self.clone()),
            _ => None,
        }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, CssLength::None)
    }

    /// Reports `true` if this length directly contains or resolves against a percentage of the containing block.
    pub fn has_percentage(&self) -> bool {
        match self {
            CssLength::Percent(_) => true,
            CssLength::Calc(c) => c[0] != 0.0,
            CssLength::CalcExpr(node) => node.has_percentage(),
            CssLength::Min(vals) | CssLength::Max(vals) => vals.iter().any(|v| v.has_percentage()),
            CssLength::Clamp(parts) => parts.iter().any(|v| v.has_percentage()),
            CssLength::FitContentArg(arg) => arg.has_percentage(),
            _ => false,
        }
    }
}
