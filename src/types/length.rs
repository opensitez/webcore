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
    /// Query-container units retain their axis until layout selects an eligible ancestor.
    Cqw(f32),
    Cqh(f32),
    Cqi(f32),
    Cqb(f32),
    Cqmin(f32),
    Cqmax(f32),
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
    /// CSS Sizing 4 `stretch`: the margin box fills the containing block.
    Stretch,
    /// `fit-content(<length-percentage>)` — the FUNCTION form, css-sizing-3
    /// §6.1, which is `max(min-content, min(max-content, <argument>))`. It
    /// carries an argument, so it cannot share the `FitContent` keyword's
    /// variant; like the keywords it is not a length, and a consumer that
    /// cannot measure content falls back to `auto`.
    FitContentArg(Box<CssLength>),
    None,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QueryContainerSizes {
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub inline: Option<f32>,
    pub block: Option<f32>,
    pub fallback_vertical: bool,
}

/// Expression node for calc() with nested min/max/clamp.
#[derive(Clone, Debug, PartialEq)]
pub enum CalcNode {
    Value(CssLength),
    Scalar(f32, CalcScalarUnit),
    Add(Box<CalcNode>, Box<CalcNode>),
    Sub(Box<CalcNode>, Box<CalcNode>),
    Mul(Box<CalcNode>, f32),
    Div(Box<CalcNode>, f32),
    Product(Box<CalcNode>, Box<CalcNode>),
    Quotient(Box<CalcNode>, Box<CalcNode>),
    Function(CssMathFunction, Vec<CalcNode>),
}

/// Canonical units for non-length calculation literals; evaluation uses their
/// canonical numeric values, while serialization retains their CSS types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalcScalarUnit { Number, Radians, Seconds, Hertz, Dppx, Percent }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CssRoundingStrategy { Nearest, Up, Down, ToZero }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CssMathFunction {
    Min, Max, Clamp, Round(CssRoundingStrategy), Mod, Rem, Abs, Sign,
    Sin, Cos, Tan, Asin, Acos, Atan, Atan2, Pow, Sqrt, Hypot, Log, Exp,
}

impl CssMathFunction {
    fn evaluate(self, args: &[f32]) -> f32 {
        use CssMathFunction::*;
        if args.iter().any(|v| v.is_nan()) { return f32::NAN; }
        let a = args[0];
        let b = args.get(1).copied().unwrap_or(1.0);
        match self {
            Min => args.iter().copied().fold(f32::INFINITY, f32::min),
            Max => args.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            Clamp => a.max(b.min(args[2])),
            Abs => a.abs(),
            Sign => if a == 0.0 { a } else { a.signum() },
            Sin => a.sin(), Cos => a.cos(), Tan => a.tan(),
            Asin => a.asin(), Acos => a.acos(), Atan => a.atan(),
            Atan2 => a.atan2(b), Pow => a.powf(b), Sqrt => a.sqrt(),
            Hypot => args.iter().copied().fold(0.0, f32::hypot),
            Log => if args.len() == 1 { a.ln() } else { a.log(b) },
            Exp => a.exp(),
            Round(strategy) => {
                let step = b.abs();
                if step == 0.0 || (a.is_infinite() && step.is_infinite()) { return f32::NAN; }
                if a.is_infinite() { return a; }
                if step.is_infinite() {
                    return match strategy {
                        CssRoundingStrategy::Up if a > 0.0 => f32::INFINITY,
                        CssRoundingStrategy::Down if a < 0.0 => f32::NEG_INFINITY,
                        _ => 0.0f32.copysign(a),
                    };
                }
                if a % step == 0.0 { return a; }
                let quotient = a / step;
                let rounded = match strategy {
                    CssRoundingStrategy::Nearest => (quotient + 0.5).floor(),
                    CssRoundingStrategy::Up => quotient.ceil(),
                    CssRoundingStrategy::Down => quotient.floor(),
                    CssRoundingStrategy::ToZero => quotient.trunc(),
                };
                if rounded == 0.0 { 0.0f32.copysign(a) } else { rounded * step }
            }
            Mod | Rem => {
                if b == 0.0 || a.is_infinite() { return f32::NAN; }
                if b.is_infinite() {
                    return if self == Mod && a.is_sign_negative() != b.is_sign_negative() { f32::NAN } else { a };
                }
                let remainder = a % b;
                if self == Rem { remainder }
                else if remainder == 0.0 { 0.0f32.copysign(b) }
                else if remainder.is_sign_negative() != b.is_sign_negative() { remainder + b }
                else { remainder }
            }
        }
    }
}

#[cfg(test)]
mod math_edge_tests {
    use super::*;

    #[test]
    fn comparisons_preserve_css_signed_zero_order() {
        for args in [[0.0, -0.0], [-0.0, 0.0]] {
            assert!(CssMathFunction::Min.evaluate(&args).is_sign_negative());
            assert!(!CssMathFunction::Max.evaluate(&args).is_sign_negative());
        }
        assert!(!CssMathFunction::Clamp.evaluate(&[0.0, -0.0, 1.0]).is_sign_negative());
        assert!(CssMathFunction::Clamp.evaluate(&[-1.0, 0.0, -0.0]).is_sign_negative());
    }

    #[test]
    fn rounding_finite_values_does_not_overflow_intermediate_quotients() {
        for strategy in [CssRoundingStrategy::Nearest, CssRoundingStrategy::Up,
            CssRoundingStrategy::Down, CssRoundingStrategy::ToZero] {
            for value in [1.0, -1.0] {
                assert_eq!(CssMathFunction::Round(strategy).evaluate(&[value, 3.0e-39]), value);
            }
        }
    }
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
        self.resolve_query_vp(
            parent_font_px,
            containing_px,
            root_font_px,
            vw,
            vh,
            QueryContainerSizes::default(),
        )
    }

    pub fn resolve_query_vp(
        &self,
        parent_font_px: f32,
        containing_px: f32,
        root_font_px: f32,
        vw: f32,
        vh: f32,
        query: QueryContainerSizes,
    ) -> f32 {
        match self {
            CalcNode::Scalar(value, _) => *value,
            CalcNode::Value(v) => {
                v.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
            }
            CalcNode::Add(a, b) => {
                a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
                    + b.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
            }
            CalcNode::Sub(a, b) => {
                a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
                    - b.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
            }
            CalcNode::Mul(a, f) => {
                a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query) * f
            }
            CalcNode::Div(a, f) => {
                if *f != 0.0 {
                    a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
                        / f
                } else {
                    0.0
                }
            }
            CalcNode::Product(a, b) => a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
                * b.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query),
            CalcNode::Quotient(a, b) => a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
                / b.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query),
            CalcNode::Function(function, args) => function.evaluate(&args.iter().map(|a|
                a.resolve_query_vp(parent_font_px, containing_px, root_font_px, vw, vh, query)
            ).collect::<Vec<_>>()),
        }
    }

    pub fn has_percentage(&self) -> bool {
        match self {
            CalcNode::Scalar(_, _) => false,
            CalcNode::Value(v) => v.has_percentage(),
            CalcNode::Add(a, b) | CalcNode::Sub(a, b) => a.has_percentage() || b.has_percentage(),
            CalcNode::Mul(a, _) | CalcNode::Div(a, _) => a.has_percentage(),
            CalcNode::Product(a, b) | CalcNode::Quotient(a, b) => a.has_percentage() || b.has_percentage(),
            CalcNode::Function(_, args) => args.iter().any(CalcNode::has_percentage),
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
        self.resolve_query_vp(
            parent_font_px,
            containing_px,
            root_font_px,
            viewport_w,
            viewport_h,
            QueryContainerSizes::default(),
        )
    }

    pub fn resolve_query_vp(
        &self,
        parent_font_px: f32,
        containing_px: f32,
        root_font_px: f32,
        viewport_w: f32,
        viewport_h: f32,
        query: QueryContainerSizes,
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
            CssLength::Cqw(v) => v / 100.0 * query.width.unwrap_or(viewport_w),
            CssLength::Cqh(v) => v / 100.0 * query.height.unwrap_or(viewport_h),
            CssLength::Cqi(v) => v / 100.0 * query.inline.unwrap_or(if query.fallback_vertical { viewport_h } else { viewport_w }),
            CssLength::Cqb(v) => v / 100.0 * query.block.unwrap_or(if query.fallback_vertical { viewport_w } else { viewport_h }),
            CssLength::Cqmin(v) => {
                let inline = query.inline.unwrap_or(if query.fallback_vertical { viewport_h } else { viewport_w });
                let block = query.block.unwrap_or(if query.fallback_vertical { viewport_w } else { viewport_h });
                v / 100.0 * inline.min(block)
            }
            CssLength::Cqmax(v) => {
                let inline = query.inline.unwrap_or(if query.fallback_vertical { viewport_h } else { viewport_w });
                let block = query.block.unwrap_or(if query.fallback_vertical { viewport_w } else { viewport_h });
                v / 100.0 * inline.max(block)
            }
            CssLength::Calc(c) => {
                c[0] / 100.0 * containing_px
                    + c[1]
                    + c[2] * parent_font_px
                    + c[3] * root_font_px
                    + c[4] / 100.0 * viewport_w
                    + c[5] / 100.0 * viewport_h
            }
            CssLength::CalcExpr(node) => node.resolve_query_vp(
                parent_font_px,
                containing_px,
                root_font_px,
                viewport_w,
                viewport_h,
                query,
            ),
            CssLength::Min(vals) => vals
                .iter()
                .map(|v| {
                    v.resolve_query_vp(
                        parent_font_px,
                        containing_px,
                        root_font_px,
                        viewport_w,
                        viewport_h,
                        query,
                    )
                })
                .fold(f32::INFINITY, f32::min),
            CssLength::Max(vals) => vals
                .iter()
                .map(|v| {
                    v.resolve_query_vp(
                        parent_font_px,
                        containing_px,
                        root_font_px,
                        viewport_w,
                        viewport_h,
                        query,
                    )
                })
                .fold(f32::NEG_INFINITY, f32::max),
            CssLength::Clamp(parts) => {
                let (min, val, max) = (&parts[0], &parts[1], &parts[2]);
                let min_v = min.resolve_query_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                    query,
                );
                let val_v = val.resolve_query_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                    query,
                );
                let max_v = max.resolve_query_vp(
                    parent_font_px,
                    containing_px,
                    root_font_px,
                    viewport_w,
                    viewport_h,
                    query,
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
            | CssLength::Stretch
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
            CssLength::Stretch => true,
            CssLength::Calc(c) => c[0] != 0.0,
            CssLength::CalcExpr(node) => node.has_percentage(),
            CssLength::Min(vals) | CssLength::Max(vals) => vals.iter().any(|v| v.has_percentage()),
            CssLength::Clamp(parts) => parts.iter().any(|v| v.has_percentage()),
            CssLength::FitContentArg(arg) => arg.has_percentage(),
            _ => false,
        }
    }
}
