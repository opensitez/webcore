//! Flex container and item enums.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Flex / Grid ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

impl Default for FlexDirection {
    fn default() -> Self {
        Self::Row
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexWrap {
    Nowrap,
    Wrap,
    WrapReverse,
}

impl Default for FlexWrap {
    fn default() -> Self {
        Self::Nowrap
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignItems {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    /// `last baseline` — aligns the items' LAST baselines and packs the group
    /// against the cross-END edge (Box Alignment §4.1).
    LastBaseline,
}

impl Default for AlignItems {
    fn default() -> Self {
        Self::Stretch
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignSelf {
    Auto,
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    /// `last baseline` — see `AlignItems::LastBaseline`.
    LastBaseline,
}

impl Default for AlignSelf {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JustifyContent {
    Normal,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    /// `left` / `right` are PHYSICAL (Box Alignment §5). They do not swap with
    /// `row-reverse` the way `flex-start` / `flex-end` do, so they need their
    /// own variants rather than mapping onto the flex-relative pair.
    Left,
    Right,
}

impl Default for JustifyContent {
    fn default() -> Self {
        Self::Normal
    }
}
