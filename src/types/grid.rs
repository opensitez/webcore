//! Box sizing, align-content and grid track types.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── New types for BoxSizing, AlignContent, Grid ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}
impl Default for BoxSizing {
    fn default() -> Self {
        Self::ContentBox
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlignContent {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}
impl Default for AlignContent {
    fn default() -> Self {
        Self::Stretch
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridAutoFlow {
    Row,
    RowDense,
    Column,
    ColumnDense,
}
impl Default for GridAutoFlow {
    fn default() -> Self {
        Self::Row
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GridTrackKind {
    Fixed,
    Percent,
    Fractional,
    Auto,
    MinMax,
    MinContent,
    MaxContent,
    FitContent,
    Subgrid,
    Calc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GridTrackSize {
    pub kind: GridTrackKind,
    pub value: f32,
    pub min_kind: GridTrackKind,
    pub min_value: f32,
    pub min_calc_length: Option<CssLength>,
    pub max_kind: GridTrackKind,
    pub max_value: f32,
    pub max_calc_length: Option<CssLength>,
    /// For `Calc` kind: the full CssLength for deferred resolution.
    pub calc_length: Option<CssLength>,
}

impl Default for GridTrackSize {
    fn default() -> Self {
        Self {
            kind: GridTrackKind::Auto,
            value: 0.0,
            min_kind: GridTrackKind::Auto,
            min_value: 0.0,
            min_calc_length: None,
            max_kind: GridTrackKind::Auto,
            max_value: 0.0,
            max_calc_length: None,
            calc_length: None,
        }
    }
}

impl GridTrackSize {
    pub fn fixed(px: f32) -> Self {
        Self {
            kind: GridTrackKind::Fixed,
            value: px,
            ..Default::default()
        }
    }
    pub fn percent(pct: f32) -> Self {
        Self {
            kind: GridTrackKind::Percent,
            value: pct,
            ..Default::default()
        }
    }
    pub fn fr(fr: f32) -> Self {
        Self {
            kind: GridTrackKind::Fractional,
            value: fr,
            ..Default::default()
        }
    }
    pub fn auto() -> Self {
        Self::default()
    }
    pub fn subgrid() -> Self {
        Self {
            kind: GridTrackKind::Subgrid,
            ..Default::default()
        }
    }
    pub fn is_auto(&self) -> bool {
        self.kind == GridTrackKind::Auto
    }
    pub fn is_none(&self) -> bool {
        self.kind == GridTrackKind::Auto && self.value == 0.0
    }
    pub fn is_subgrid(&self) -> bool {
        self.kind == GridTrackKind::Subgrid
    }
}
