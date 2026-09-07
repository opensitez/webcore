//! CSS animation and transition types.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Animation / Transition types ─────────────────────────────────────────

/// A single keyframe stop inside a `@keyframes` block.
#[derive(Clone, Debug)]
pub struct KeyframeStop {
    /// Progress point in the animation (0.0 = `from` / `0%`, 1.0 = `to` / `100%`).
    pub offset: f32,
    /// CSS property/value pairs declared at this stop.
    pub properties: Vec<(String, String)>,
}

/// CSS easing function (timing function).
#[derive(Clone, Debug, PartialEq)]
pub enum EasingFn {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f32, f32, f32, f32),
    StepStart,
    StepEnd,
    Steps(u32, StepPosition),
    /// `linear()` — css-easing-2 §2.1. A piecewise-linear curve given as
    /// (input progress, output progress) control points, already normalised
    /// and sorted by input.
    LinearPoints(Vec<(f32, f32)>),
}

/// The step position of a `steps()` easing — css-easing-2 §2.3.
///
/// ⛔ FOUR values, not a boolean. `jump-none` and `jump-both` change the number
/// of JUMPS as well as where they land (`steps-1` and `steps+1` respectively),
/// so neither can be expressed as "is it jump-start". A `jump-none` sprite
/// animation must reach its last frame; folded onto jump-end it never does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StepPosition {
    JumpStart,
    JumpEnd,
    JumpNone,
    JumpBoth,
}
impl Default for EasingFn {
    fn default() -> Self {
        Self::Ease
    }
}

/// CSS `animation-direction` values.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum AnimDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

/// CSS `animation-fill-mode` values.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum FillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// CSS `animation-composition` values.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum AnimationComposition {
    #[default]
    Replace,
    Add,
    Accumulate,
}

/// A fully parsed CSS `animation` shorthand or sub-property group.
#[derive(Clone, Debug)]
pub struct ParsedAnimation {
    pub name: String,
    pub duration_ms: f32,
    pub delay_ms: f32,
    pub timing_fn: EasingFn,
    /// `f32::INFINITY` for `animation-iteration-count: infinite`.
    pub iteration_count: f32,
    pub direction: AnimDirection,
    pub fill_mode: FillMode,
    pub play_state_paused: bool,
    pub composition: AnimationComposition,
}

/// A fully parsed CSS `transition` shorthand or sub-property group.
#[derive(Clone, Debug)]
pub struct ParsedTransition {
    pub property: String,
    pub duration_ms: f32,
    pub delay_ms: f32,
    pub timing_fn: EasingFn,
    pub allow_discrete: bool,
}

/// Runtime state for one active CSS animation on one element.
#[derive(Clone, Debug)]
pub struct AnimState {
    /// The WebCore raw pointer, stored as `usize` for Hash/Eq.
    pub element_id: u32,
    pub animation: ParsedAnimation,
    pub start_time: std::time::Instant,
    pub last_iteration_event: u32,
}

/// Runtime state for one active CSS transition on one property of one element.
#[derive(Clone, Debug)]
pub struct TransitionState {
    pub property: String,
    pub from_value: String,
    pub to_value: String,
    pub reversing_adjusted_start_value: String,
    pub reversing_shortening_factor: f32,
    pub start_time: std::time::Instant,
    pub duration_ms: f32,
    pub delay_ms: f32,
    pub timing_fn: EasingFn,
    pub allow_discrete: bool,
}
