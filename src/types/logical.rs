//! Flow-relative (logical) box properties and their mapping onto physical
//! sides — css-logical-1 §4 and css-writing-modes-4 §6.4.

use super::*;

/// A flow-relative box property, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalSlot {
    MarginPhysical(PhysicalSide),
    PaddingPhysical(PhysicalSide),
    MarginInlineStart,
    MarginInlineEnd,
    MarginBlockStart,
    MarginBlockEnd,
    PaddingInlineStart,
    PaddingInlineEnd,
    PaddingBlockStart,
    PaddingBlockEnd,
    InsetInlineStart,
    InsetInlineEnd,
    InsetBlockStart,
    InsetBlockEnd,
    BorderInlineStartWidth,
    BorderInlineEndWidth,
    BorderBlockStartWidth,
    BorderBlockEndWidth,
    InlineSize,
    BlockSize,
    MinInlineSize,
    MinBlockSize,
    MaxInlineSize,
    MaxBlockSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalBorderSlot {
    InlineStart,
    InlineEnd,
    BlockStart,
    BlockEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalCornerSlot {
    StartStart,
    StartEnd,
    EndStart,
    EndEnd,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogicalBorderValue {
    pub slot: LogicalBorderSlot,
    pub width: Option<CssLength>,
    pub style: Option<BorderStyle>,
    pub color: Option<Color>,
}

/// A physical side of the box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalSide {
    Top,
    Right,
    Bottom,
    Left,
}

/// The physical side the inline-start edge names.
///
/// css-writing-modes-4 §6.4: the inline axis is horizontal in a horizontal
/// writing mode and vertical in a vertical one, and `direction` picks which
/// end of that axis is the start.
pub fn inline_start_side(wm: WritingMode, dir: Direction) -> PhysicalSide {
    let rtl = dir == Direction::RTL;
    match wm {
        WritingMode::HorizontalTB => {
            if rtl {
                PhysicalSide::Right
            } else {
                PhysicalSide::Left
            }
        }
        WritingMode::VerticalRL
        | WritingMode::VerticalLR
        | WritingMode::SidewaysRL
        | WritingMode::SidewaysLR => {
            if rtl {
                PhysicalSide::Bottom
            } else {
                PhysicalSide::Top
            }
        }
    }
}

/// The physical side the block-start edge names. `direction` has no say in it.
pub fn block_start_side(wm: WritingMode) -> PhysicalSide {
    match wm {
        WritingMode::HorizontalTB => PhysicalSide::Top,
        WritingMode::VerticalRL | WritingMode::SidewaysRL => PhysicalSide::Right,
        WritingMode::VerticalLR | WritingMode::SidewaysLR => PhysicalSide::Left,
    }
}

pub fn opposite(side: PhysicalSide) -> PhysicalSide {
    match side {
        PhysicalSide::Top => PhysicalSide::Bottom,
        PhysicalSide::Bottom => PhysicalSide::Top,
        PhysicalSide::Left => PhysicalSide::Right,
        PhysicalSide::Right => PhysicalSide::Left,
    }
}

/// True when the inline axis runs horizontally, i.e. `inline-size` is `width`.
pub fn inline_axis_is_horizontal(wm: WritingMode) -> bool {
    matches!(wm, WritingMode::HorizontalTB)
}
