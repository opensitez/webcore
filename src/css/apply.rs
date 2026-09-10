//! Applying a parsed declaration to a `ComputedStyle`.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Property Application ─────────────────────────────────────────────────

pub fn apply_property(style: &mut ComputedStyle, prop: &str, value: &str) {
    // HTML attributes that aren't real CSS properties — handle before resolving
    match prop {
        "cellpadding" => {
            let v = value.trim();
            style.cell_padding = parse_length(v);
            return;
        }
        "cellspacing" => {
            let v = value.trim();
            style.border_spacing_h = parse_length(v);
            style.border_spacing_v = parse_length(v);
            return;
        }
        _ => {}
    }
    // CSS custom properties (--*) are not resolved by PropertyId
    let v = value.trim();
    if prop.starts_with("--") {
        style.custom_props.insert(prop.to_string(), v.to_string());
        return;
    }
    let id = properties::resolve(prop);
    apply_property_by_id(style, id, value);
}

/// Apply a pre-parsed CssValue to a computed style. This is the primary cascade
/// path — values are pre-parsed during stylesheet compilation.
pub fn apply_css_value(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    value: &crate::types::CssValue,
) {
    use crate::types::CssValue;
    match value {
        CssValue::Inherit => return,
        // `unset` behaves as `inherit` on an inherited property (the style
        // already carries the parent's value, so leaving it alone IS
        // inheriting) and as `initial` on every other. CSS Cascade 5 §7.3.
        CssValue::Unset if properties::is_inherited(id) => return,
        CssValue::Initial | CssValue::Unset => {
            note_current_color(style, id, "");
            reset_to_initial(style, id);
            return;
        }
        // These need cascade origin/layer context; handled by the cascade.
        CssValue::Revert | CssValue::RevertLayer => return,
        _ => note_specified_svg_paint(style, id),
    }
    match value {
        CssValue::Inherit
        | CssValue::Unset
        | CssValue::Initial
        | CssValue::Revert
        | CssValue::RevertLayer => {}
        CssValue::Length(l) => {
            if apply_length_value(style, id, l) {
                return;
            }
        }
        CssValue::Color(c) => {
            // A real colour landing on a property that earlier asked for
            // `currentColor` takes it back off the deferred list.
            note_current_color(style, id, "");
            if apply_color_value(style, id, c) {
                return;
            }
        }
        CssValue::Number(n) => {
            if apply_number_value(style, id, *n) {
                return;
            }
        }
        CssValue::Integer(n) => {
            if apply_integer_value(style, id, *n) {
                return;
            }
        }
        // ── Keyword values — direct assignment, no parsing ──
        CssValue::Display(d) => {
            style.display = *d;
            return;
        }
        CssValue::Position(p) => {
            style.position = *p;
            return;
        }
        CssValue::Float(f) => {
            style.float = *f;
            return;
        }
        CssValue::Clear(c) => {
            style.clear = *c;
            return;
        }
        CssValue::BoxSizing(b) => {
            style.box_sizing = *b;
            return;
        }
        CssValue::Overflow(o) => {
            use properties::PropertyId::*;
            match id {
                OverflowX => style.overflow_x = *o,
                OverflowY => style.overflow_y = *o,
                _ => {
                    style.overflow_x = *o;
                    style.overflow_y = *o;
                }
            }
            return;
        }
        CssValue::Visible(v) => {
            style.visibility = *v;
            return;
        }
        CssValue::TextAlign(a) => {
            style.text_align = *a;
            return;
        }
        CssValue::TextTransform(t) => {
            style.text_transform = *t;
            return;
        }
        CssValue::WhiteSpace(w) => {
            style.white_space = *w;
            return;
        }
        CssValue::FontWeight(w) => {
            style.font_weight = *w;
            return;
        }
        CssValue::FontStyle(s) => {
            style.font_style = *s;
            return;
        }
        CssValue::FlexDirection(d) => {
            style.flex_direction = *d;
            return;
        }
        CssValue::FlexWrap(w) => {
            style.flex_wrap = *w;
            return;
        }
        CssValue::AlignItems(a) => {
            style.align_items = *a;
            return;
        }
        CssValue::AlignSelf(a) => {
            style.align_self = *a;
            return;
        }
        CssValue::AlignContent(a) => {
            style.align_content = *a;
            return;
        }
        CssValue::JustifyContent(j) => {
            style.justify_content = *j;
            return;
        }
        CssValue::ListStyleType(l) => {
            style.list_style_type = *l;
            style.custom_list_style_type.clear();
            return;
        }
        CssValue::ListStylePosition(l) => {
            style.list_style_position = *l;
            return;
        }
        CssValue::WordBreak(w) => {
            style.word_break = *w;
            return;
        }
        CssValue::BorderStyle(bs) => {
            use crate::types::BorderStyleValue as BSV;
            let s = match bs {
                BSV::None => crate::types::BorderStyle::None,
                BSV::Hidden => crate::types::BorderStyle::Hidden,
                BSV::Solid => crate::types::BorderStyle::Solid,
                BSV::Dashed => crate::types::BorderStyle::Dashed,
                BSV::Dotted => crate::types::BorderStyle::Dotted,
                BSV::Double => crate::types::BorderStyle::Double,
                BSV::Groove => crate::types::BorderStyle::Groove,
                BSV::Ridge => crate::types::BorderStyle::Ridge,
                BSV::Inset => crate::types::BorderStyle::Inset,
                BSV::Outset => crate::types::BorderStyle::Outset,
            };
            use properties::PropertyId::*;
            match id {
                BorderTopStyle => style.border_top_style = s,
                BorderRightStyle => style.border_right_style = s,
                BorderBottomStyle => style.border_bottom_style = s,
                BorderLeftStyle => style.border_left_style = s,
                _ => {}
            }
            return;
        }
        CssValue::VerticalAlign(v) => {
            style.vertical_align = v.clone();
            return;
        }
        CssValue::Raw(s) => {
            apply_property_by_id_str(style, id, s);
            return;
        }
    }
    // Fallback: typed value wasn't handled — serialize back to string
    let s = match value {
        CssValue::Length(l) => crate::html::serializer::serialize_length(l),
        CssValue::Color(c) => format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
        CssValue::Number(n) => format!("{}", n),
        CssValue::Integer(n) => format!("{}", n),
        CssValue::Revert => "revert".to_string(),
        CssValue::RevertLayer => "revert-layer".to_string(),
        _ => return,
    };
    apply_property_by_id_str(style, id, &s);
}

/// Apply a raw string value — used for var() resolution, inline styles,
/// and properties that haven't been converted to typed CssValue yet.
/// Record — never resolve — a declaration that asks for `currentColor`.
///
/// ⛔ DEFERRED. `currentColor` computes to the element's own `color`
/// (css-color-4 §6.2), and `color` may be declared AFTER the property that
/// references it, so reading `style.color` here would take a stale value.
/// `finalize_current_color` applies the mask once the whole cascade is done.
///
/// A later declaration of the same property that is NOT `currentColor` clears
/// the bit, so the cascade's winner is the one that decides.
pub(crate) fn note_current_color(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    value: &str,
) {
    use crate::types::*;
    use properties::PropertyId as P;
    const B_ALL: u16 = CURRENT_COLOR_BORDER_TOP
        | CURRENT_COLOR_BORDER_RIGHT
        | CURRENT_COLOR_BORDER_BOTTOM
        | CURRENT_COLOR_BORDER_LEFT;
    let bit = match id {
        P::BorderTopColor | P::BorderTop => CURRENT_COLOR_BORDER_TOP,
        P::BorderRightColor | P::BorderRight => CURRENT_COLOR_BORDER_RIGHT,
        P::BorderBottomColor | P::BorderBottom => CURRENT_COLOR_BORDER_BOTTOM,
        P::BorderLeftColor | P::BorderLeft => CURRENT_COLOR_BORDER_LEFT,
        P::BorderColor | P::Border => B_ALL,
        P::OutlineColor | P::Outline => CURRENT_COLOR_OUTLINE,
        P::BackgroundColor | P::Background => CURRENT_COLOR_BACKGROUND,
        P::TextDecorationColor | P::TextDecoration => CURRENT_COLOR_TEXT_DECOR,
        P::CaretColor => CURRENT_COLOR_CARET,
        P::Fill => CURRENT_COLOR_SVG_FILL,
        P::Stroke => CURRENT_COLOR_SVG_STROKE,
        _ => return,
    };
    // A shorthand carries the colour as one component among several, so every
    // top-level token is a candidate. A token that merely CONTAINS the keyword
    // (`linear-gradient(currentColor,red)`) is not one: only a whole token is.
    let asks = value
        .split_ascii_whitespace()
        .any(|t| t.eq_ignore_ascii_case("currentcolor"));
    if asks {
        style.rare_mut().current_color_props |= bit;
    } else if style.rare().current_color_props & bit != 0 {
        style.rare_mut().current_color_props &= !bit;
    }
}

/// Resolve every flow-relative box declaration onto its physical side, once,
/// after the cascade — css-logical-1 §4, css-writing-modes-4 §6.4.
///
/// The declarations are replayed IN ORDER, so a later one still wins over an
/// earlier one; what changes is that both are mapped with the element's final
/// `direction` and `writing-mode` instead of whatever those held at the moment
/// each was applied.
pub fn finalize_logical(style: &mut ComputedStyle) {
    use crate::types::*;
    finalize_logical_float_clear(style);
    if style.rare().logical_box.is_empty() && style.rare().logical_borders.is_empty() {
        return;
    }
    let decls = std::mem::take(&mut style.rare_mut().logical_box);
    let wm = style.writing_mode;
    let dir = style.direction;
    let i_start = inline_start_side(wm, dir);
    let i_end = opposite(i_start);
    let b_start = block_start_side(wm);
    let b_end = opposite(b_start);
    let horizontal_inline = inline_axis_is_horizontal(wm);

    for (slot, len) in decls {
        match slot {
            LogicalSlot::MarginInlineStart => set_margin(style, i_start, len),
            LogicalSlot::MarginInlineEnd => set_margin(style, i_end, len),
            LogicalSlot::MarginBlockStart => set_margin(style, b_start, len),
            LogicalSlot::MarginBlockEnd => set_margin(style, b_end, len),
            LogicalSlot::PaddingInlineStart => set_padding(style, i_start, len),
            LogicalSlot::PaddingInlineEnd => set_padding(style, i_end, len),
            LogicalSlot::PaddingBlockStart => set_padding(style, b_start, len),
            LogicalSlot::PaddingBlockEnd => set_padding(style, b_end, len),
            LogicalSlot::InsetInlineStart => set_inset(style, i_start, len),
            LogicalSlot::InsetInlineEnd => set_inset(style, i_end, len),
            LogicalSlot::InsetBlockStart => set_inset(style, b_start, len),
            LogicalSlot::InsetBlockEnd => set_inset(style, b_end, len),
            LogicalSlot::BorderInlineStartWidth => set_border_width(style, i_start, len),
            LogicalSlot::BorderInlineEndWidth => set_border_width(style, i_end, len),
            LogicalSlot::BorderBlockStartWidth => set_border_width(style, b_start, len),
            LogicalSlot::BorderBlockEndWidth => set_border_width(style, b_end, len),
            // A size on the inline axis is the WIDTH only while that axis is
            // horizontal; in a vertical writing mode it is the height.
            LogicalSlot::InlineSize => {
                if horizontal_inline {
                    style.width = len
                } else {
                    style.height = len
                }
            }
            LogicalSlot::BlockSize => {
                if horizontal_inline {
                    style.height = len
                } else {
                    style.width = len
                }
            }
            LogicalSlot::MinInlineSize => {
                if horizontal_inline {
                    style.min_width = len
                } else {
                    style.min_height = len
                }
            }
            LogicalSlot::MinBlockSize => {
                if horizontal_inline {
                    style.min_height = len
                } else {
                    style.min_width = len
                }
            }
            LogicalSlot::MaxInlineSize => {
                if horizontal_inline {
                    style.max_width = len
                } else {
                    style.max_height = len
                }
            }
            LogicalSlot::MaxBlockSize => {
                if horizontal_inline {
                    style.max_height = len
                } else {
                    style.max_width = len
                }
            }
        }
    }
    let borders = std::mem::take(&mut style.rare_mut().logical_borders);
    for decl in borders {
        let side = match decl.slot {
            LogicalBorderSlot::InlineStart => i_start,
            LogicalBorderSlot::InlineEnd => i_end,
            LogicalBorderSlot::BlockStart => b_start,
            LogicalBorderSlot::BlockEnd => b_end,
        };
        if let Some(width) = decl.width {
            set_border_width(style, side, width);
        }
        if let Some(border_style) = decl.style {
            set_border_style(style, side, border_style);
        }
        if let Some(color) = decl.color {
            set_border_color(style, side, color);
        }
    }
}

pub fn finalize_logical_float_clear(style: &mut ComputedStyle) {
    use crate::types::{Clear, Float, PhysicalSide};

    let inline_start = inline_start_side(style.writing_mode, style.direction);
    let inline_end = opposite(inline_start);
    style.float = match (style.float, inline_start, inline_end) {
        (Float::InlineStart, PhysicalSide::Left, _) => Float::Left,
        (Float::InlineStart, PhysicalSide::Right, _) => Float::Right,
        (Float::InlineEnd, _, PhysicalSide::Left) => Float::Left,
        (Float::InlineEnd, _, PhysicalSide::Right) => Float::Right,
        (Float::InlineStart | Float::InlineEnd, _, _) => Float::None,
        (other, _, _) => other,
    };
    style.clear = match (style.clear, inline_start, inline_end) {
        (Clear::InlineStart, PhysicalSide::Left, _) => Clear::Left,
        (Clear::InlineStart, PhysicalSide::Right, _) => Clear::Right,
        (Clear::InlineEnd, _, PhysicalSide::Left) => Clear::Left,
        (Clear::InlineEnd, _, PhysicalSide::Right) => Clear::Right,
        (Clear::InlineStart | Clear::InlineEnd, _, _) => Clear::None,
        (other, _, _) => other,
    };
}

fn set_margin(s: &mut ComputedStyle, side: crate::types::PhysicalSide, l: CssLength) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.margin_top = l,
        P::Right => s.margin_right = l,
        P::Bottom => s.margin_bottom = l,
        P::Left => s.margin_left = l,
    }
}
fn set_padding(s: &mut ComputedStyle, side: crate::types::PhysicalSide, l: CssLength) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.padding_top = l,
        P::Right => s.padding_right = l,
        P::Bottom => s.padding_bottom = l,
        P::Left => s.padding_left = l,
    }
}
fn set_inset(s: &mut ComputedStyle, side: crate::types::PhysicalSide, l: CssLength) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.top = l,
        P::Right => s.right = l,
        P::Bottom => s.bottom = l,
        P::Left => s.left = l,
    }
}
fn set_border_width(s: &mut ComputedStyle, side: crate::types::PhysicalSide, l: CssLength) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.border_top_width = l,
        P::Right => s.border_right_width = l,
        P::Bottom => s.border_bottom_width = l,
        P::Left => s.border_left_width = l,
    }
}
fn set_border_style(s: &mut ComputedStyle, side: crate::types::PhysicalSide, v: BorderStyle) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.border_top_style = v,
        P::Right => s.border_right_style = v,
        P::Bottom => s.border_bottom_style = v,
        P::Left => s.border_left_style = v,
    }
}
fn set_border_color(s: &mut ComputedStyle, side: crate::types::PhysicalSide, c: Color) {
    use crate::types::PhysicalSide as P;
    match side {
        P::Top => s.border_top_color = c,
        P::Right => s.border_right_color = c,
        P::Bottom => s.border_bottom_color = c,
        P::Left => s.border_left_color = c,
    }
}

/// Copy the element's computed `color` into every property that asked for
/// `currentColor`. Called once, after the whole cascade.
pub fn finalize_current_color(style: &mut ComputedStyle) {
    use crate::types::*;
    let mask = style.rare().current_color_props;
    if mask == 0 {
        return;
    }
    let c = style.color;
    if mask & CURRENT_COLOR_BORDER_TOP != 0 {
        style.border_top_color = c;
    }
    if mask & CURRENT_COLOR_BORDER_RIGHT != 0 {
        style.border_right_color = c;
    }
    if mask & CURRENT_COLOR_BORDER_BOTTOM != 0 {
        style.border_bottom_color = c;
    }
    if mask & CURRENT_COLOR_BORDER_LEFT != 0 {
        style.border_left_color = c;
    }
    if mask & CURRENT_COLOR_OUTLINE != 0 {
        style.outline_color = c;
    }
    if mask & CURRENT_COLOR_BACKGROUND != 0 {
        style.background_color = c;
    }
    if mask & CURRENT_COLOR_TEXT_DECOR != 0 {
        style.text_decoration_color = Some(c);
    }
    if mask & CURRENT_COLOR_CARET != 0 {
        style.caret_color = Some(c);
    }
    if mask & CURRENT_COLOR_SVG_FILL != 0 {
        style.svg_fill = Some(c);
    }
    if mask & CURRENT_COLOR_SVG_STROKE != 0 {
        style.svg_stroke = Some(c);
    }
    // The mask has done its job. Clearing it keeps it from riding along into a
    // style that inherits from this one.
    style.rare_mut().current_color_props = 0;
}

pub fn apply_property_by_id_str(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    value: &str,
) {
    let v = value.trim();
    if v == "inherit" {
        return;
    }
    note_specified_svg_paint(style, id);
    note_current_color(style, id, v);
    // Same rule as the typed path above — see `apply_css_value`.
    if v == "unset" && properties::is_inherited(id) {
        return;
    }
    if matches!(v, "initial" | "unset" | "revert" | "revert-layer") {
        reset_to_initial(style, id);
        return;
    }
    (property_defs::get(id).apply)(style, v);
}

/// Reset a property to its initial value, descending into a SHORTHAND's
/// longhands.
///
/// ⛔ A shorthand owns no storage, so its `copy` is a no-op — resetting it
/// through `copy` silently did NOTHING. `flex: initial` left the item's grow,
/// shrink and basis exactly as they were instead of returning them to
/// `0 1 auto`. Only the longhands hold the values, so only they can be reset.
fn reset_to_initial(style: &mut ComputedStyle, id: properties::PropertyId) {
    let def = property_defs::get(id);
    if !def.longhands.is_empty() {
        for &lh in def.longhands {
            reset_to_initial(style, lh);
        }
        return;
    }
    let default_style = ComputedStyle::default();
    (def.copy)(style, &default_style);
}

fn note_specified_svg_paint(style: &mut ComputedStyle, id: properties::PropertyId) {
    use properties::PropertyId::*;
    match id {
        Fill => style.rare_mut().specified_svg_paint_props |= SPECIFIED_SVG_FILL,
        Stroke => style.rare_mut().specified_svg_paint_props |= SPECIFIED_SVG_STROKE,
        _ => {}
    }
}

/// Backward-compat wrapper — accepts &str directly.
pub fn apply_property_by_id(style: &mut ComputedStyle, id: properties::PropertyId, value: &str) {
    apply_property_by_id_str(style, id, value);
}

// ── Typed value application (fast paths for pre-parsed values) ────────────

fn apply_length_value(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    l: &crate::types::CssLength,
) -> bool {
    use properties::PropertyId::*;
    match id {
        Width => style.width = l.clone(),
        Height => style.height = l.clone(),
        MinWidth => style.min_width = l.clone(),
        MinHeight => style.min_height = l.clone(),
        MaxWidth => style.max_width = l.clone(),
        MaxHeight => style.max_height = l.clone(),
        MarginTop => style.margin_top = l.clone(),
        MarginRight => style.margin_right = l.clone(),
        MarginBottom => style.margin_bottom = l.clone(),
        MarginLeft => style.margin_left = l.clone(),
        PaddingTop => style.padding_top = l.clone(),
        PaddingRight => style.padding_right = l.clone(),
        PaddingBottom => style.padding_bottom = l.clone(),
        PaddingLeft => style.padding_left = l.clone(),
        BorderTopWidth => style.border_top_width = l.clone(),
        BorderRightWidth => style.border_right_width = l.clone(),
        BorderBottomWidth => style.border_bottom_width = l.clone(),
        BorderLeftWidth => style.border_left_width = l.clone(),
        Top => style.top = l.clone(),
        Right => style.right = l.clone(),
        Bottom => style.bottom = l.clone(),
        Left => style.left = l.clone(),
        FlexBasis => style.flex_basis = l.clone(),
        // FontSize is NOT handled here — it needs special em/rem resolution at cascade time
        LineHeight => style.line_height = l.clone(),
        LetterSpacing => style.letter_spacing = l.clone(),
        WordSpacing => style.word_spacing = l.clone(),
        TextIndent => style.text_indent = l.clone(),
        Gap => {
            style.row_gap = l.clone();
            style.column_gap = l.clone();
            style.gap = l.clone();
        }
        RowGap => {
            style.row_gap = l.clone();
            style.gap = l.clone();
        }
        ColumnGap => style.column_gap = l.clone(),
        OutlineWidth => {
            if let crate::types::CssLength::Px(px) = l {
                style.outline_width = *px;
            }
        }
        OutlineOffset => {
            if let crate::types::CssLength::Px(px) = l {
                style.outline_offset = *px;
            }
        }
        TextDecorationThickness => style.text_decoration_thickness = l.clone(),
        TextUnderlineOffset => style.text_underline_offset = l.clone(),
        _ => return false, // Unhandled — caller falls back to string path
    }
    true
}

fn apply_color_value(
    style: &mut ComputedStyle,
    id: properties::PropertyId,
    c: &crate::types::Color,
) -> bool {
    use properties::PropertyId::*;
    match id {
        Color => style.color = *c,
        BackgroundColor => style.background_color = *c,
        BorderTopColor => style.border_top_color = *c,
        BorderRightColor => style.border_right_color = *c,
        BorderBottomColor => style.border_bottom_color = *c,
        BorderLeftColor => style.border_left_color = *c,
        OutlineColor => style.outline_color = *c,
        CaretColor => style.caret_color = Some(*c),
        Fill => style.svg_fill = Some(*c),
        Stroke => style.svg_stroke = Some(*c),
        _ => return false,
    }
    true
}

fn apply_number_value(style: &mut ComputedStyle, id: properties::PropertyId, n: f32) -> bool {
    use properties::PropertyId::*;
    match id {
        Opacity => style.opacity = n.clamp(0.0, 1.0),
        FlexGrow => style.flex_grow = n,
        FlexShrink => style.flex_shrink = n,
        FontStretch => style.font_stretch = n,
        _ => return false,
    }
    true
}

fn apply_integer_value(style: &mut ComputedStyle, id: properties::PropertyId, n: i32) -> bool {
    use properties::PropertyId::*;
    match id {
        ZIndex => {
            style.z_index = n;
            style.z_index_is_auto = false;
        }
        Order => style.order = n,
        _ => return false,
    }
    true
}

/// Parse a CSS `filter` value string into `CssFilters`.
pub fn parse_css_filter(v: &str) -> crate::types::CssFilters {
    parse_css_filter_with_current_color(v, crate::types::Color::BLACK)
}

pub fn parse_css_filter_with_current_color(
    v: &str,
    current_color: crate::types::Color,
) -> crate::types::CssFilters {
    use crate::types::{CssFilters, FilterOp};
    fn filter_number(arg: &str, default: f32) -> f32 {
        let arg = arg.trim();
        if arg.is_empty() {
            return default;
        }
        if let Some(n) = arg.strip_suffix('%') {
            n.trim().parse::<f32>().unwrap_or(default) / 100.0
        } else {
            arg.parse::<f32>().unwrap_or(default)
        }
    }
    fn clamped_unit(arg: &str, default: f32) -> f32 {
        filter_number(arg, default).clamp(0.0, 1.0)
    }
    fn filter_length_px(arg: &str, default: f32) -> f32 {
        let arg = arg.trim();
        if arg.is_empty() {
            return default;
        }
        crate::css::value_parse::parse_length(arg).resolve_vp(16.0, 0.0, 16.0, 800.0, 600.0)
    }
    fn filter_angle_deg(arg: &str, default: f32) -> f32 {
        let arg = arg.trim();
        if arg.is_empty() {
            return default;
        }
        let lower = arg.to_ascii_lowercase();
        let num = |suffix: &str| {
            lower
                .trim_end_matches(suffix)
                .parse::<f32>()
                .unwrap_or(default)
        };
        if lower.ends_with("turn") {
            num("turn") * 360.0
        } else if lower.ends_with("grad") {
            num("grad") * 0.9
        } else if lower.ends_with("rad") {
            num("rad") * 180.0 / std::f32::consts::PI
        } else if lower.ends_with("deg") {
            num("deg")
        } else {
            lower.parse::<f32>().unwrap_or(default)
        }
    }
    let mut ops = Vec::new();
    if v.trim() == "none" {
        return CssFilters::default();
    }
    let mut rest = v.trim();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let paren_pos = match rest.find('(') {
            Some(p) => p,
            None => break,
        };
        let func = rest[..paren_pos].trim().to_ascii_lowercase();
        let after_paren = &rest[paren_pos + 1..];
        let close = after_paren.find(')').unwrap_or(after_paren.len());
        let arg_str = after_paren[..close].trim();
        rest = if close + 1 < after_paren.len() {
            &after_paren[close + 1..]
        } else {
            ""
        };
        match func.as_str() {
            "blur" => ops.push(FilterOp::Blur(filter_length_px(arg_str, 0.0))),
            "brightness" => ops.push(FilterOp::Brightness(filter_number(arg_str, 1.0).max(0.0))),
            "contrast" => ops.push(FilterOp::Contrast(filter_number(arg_str, 1.0).max(0.0))),
            "grayscale" => ops.push(FilterOp::Grayscale(clamped_unit(arg_str, 1.0))),
            "hue-rotate" => ops.push(FilterOp::HueRotate(filter_angle_deg(arg_str, 0.0))),
            "invert" => ops.push(FilterOp::Invert(clamped_unit(arg_str, 1.0))),
            "opacity" => ops.push(FilterOp::Opacity(clamped_unit(arg_str, 1.0))),
            "saturate" => ops.push(FilterOp::Saturate(filter_number(arg_str, 1.0).max(0.0))),
            "sepia" => ops.push(FilterOp::Sepia(clamped_unit(arg_str, 1.0))),
            "drop-shadow" => {
                let parts: Vec<&str> = arg_str.split_whitespace().collect();
                let mut lengths = Vec::new();
                let mut color = None;
                for part in parts {
                    if color.is_none() {
                        if let Some(c) = parse_color(part) {
                            color = Some(c);
                            continue;
                        }
                    }
                    lengths.push(filter_length_px(part, 0.0));
                }
                if lengths.len() < 2 || lengths.len() > 3 {
                    return CssFilters::default();
                }
                ops.push(FilterOp::DropShadow {
                    dx: lengths[0],
                    dy: lengths[1],
                    blur: lengths.get(2).copied().unwrap_or(0.0),
                    color: color.unwrap_or(current_color),
                });
            }
            _ => return CssFilters::default(),
        }
    }
    CssFilters { ops }
}

/// Resolve `var(--name)` and `var(--name, fallback)` references in a CSS value.
/// Variables in the map are pre-resolved by `pre_resolve_variables`, so one pass suffices.
/// Any still-unresolved var() (unknown custom property with no fallback) is dropped.
pub fn resolve_var_references(val: &str, variables: &HashMap<String, String>) -> String {
    if !val.contains("var(") {
        return val.to_string();
    }
    let mut result = resolve_var_pass(val, variables);
    // Iterate to resolve chained vars (var(--a) → var(--b) → value).
    // Max 10 iterations to prevent infinite loops from circular refs.
    for _ in 0..10 {
        if !result.contains("var(") {
            return result;
        }
        let next = resolve_var_pass(&result, variables);
        if next == result {
            break;
        } // no progress — unresolvable
        result = next;
    }
    // Drop any remaining unresolved var() by substituting with fallback or "".
    if result.contains("var(") {
        resolve_var_pass(&result, &HashMap::new())
    } else {
        result
    }
}

pub(crate) fn resolve_var_pass(val: &str, variables: &HashMap<String, String>) -> String {
    if !val.contains("var(") {
        return val.to_string();
    }
    let mut out = String::new();
    let mut rest = val;
    while !rest.is_empty() {
        if let Some(start) = rest.find("var(") {
            out.push_str(&rest[..start]);
            rest = &rest[start + 4..]; // skip "var("
                                       // find matching closing paren
            let mut depth = 1usize;
            let mut end = 0;
            let bytes = rest.as_bytes();
            while end < bytes.len() {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            let inner = &rest[..end];
            rest = if end < rest.len() {
                &rest[end + 1..]
            } else {
                ""
            };
            // inner = "--name" or "--name, fallback"
            let (name, fallback) = if let Some(comma) = inner.find(',') {
                (inner[..comma].trim(), Some(inner[comma + 1..].trim()))
            } else {
                (inner.trim(), None)
            };
            if let Some(resolved) = variables.get(name) {
                if resolved.is_empty() {
                    if let Some(fb) = fallback {
                        out.push_str(fb);
                    }
                } else {
                    out.push_str(resolved);
                }
            } else if let Some(fb) = fallback {
                out.push_str(fb);
            }
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}

/// Resolve a CSS `content` property value string to a displayable string.
/// Handles string literals, open-quote/close-quote, and discards complex expressions.
/// Process CSS Unicode escapes in a string: `\e001` → U+E001, `\A` → newline, etc.
fn unescape_css_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // Collect up to 6 hex digits
            let mut hex = String::new();
            while hex.len() < 6 {
                match chars.peek() {
                    Some(ch) if ch.is_ascii_hexdigit() => {
                        hex.push(*ch);
                        chars.next();
                    }
                    _ => break,
                }
            }
            if !hex.is_empty() {
                // Optional trailing whitespace is consumed after hex escape
                if let Some(&' ') = chars.peek() {
                    chars.next();
                }
                if let Ok(cp) = u32::from_str_radix(&hex, 16) {
                    if let Some(uc) = char::from_u32(cp) {
                        out.push(uc);
                        continue;
                    }
                }
                // Invalid code point — output replacement character
                out.push('\u{FFFD}');
            } else if let Some(next) = chars.next() {
                // Escaped literal character (e.g. \\ → \, \" → ")
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn resolve_content_value(v: &str) -> String {
    resolve_content_value_with_context(v, None, None)
}

pub(crate) fn resolve_content_value_with_context(
    v: &str,
    attrs: Option<&crate::dom::attrs::AttrMap>,
    quotes: Option<&[String]>,
) -> String {
    let v = v.trim();
    match v {
        "none" | "normal" => String::new(),
        "open-quote" => quote_string(quotes, true),
        "close-quote" => quote_string(quotes, false),
        "no-open-quote" | "no-close-quote" => String::new(),
        _ => {
            // Quoted string: "text" or 'text'
            if (v.starts_with('"') && v.ends_with('"'))
                || (v.starts_with('\'') && v.ends_with('\''))
            {
                return unescape_css_string(&v[1..v.len() - 1]);
            }
            // Multiple tokens (e.g. '"foo" open-quote'): concatenate resolved parts
            let mut out = String::new();
            let mut rest = v;
            while !rest.is_empty() {
                rest = rest.trim_start();
                if rest.starts_with('"') || rest.starts_with('\'') {
                    let q = &rest[..1];
                    if let Some(end) = rest[1..].find(q) {
                        out.push_str(&unescape_css_string(&rest[1..end + 1]));
                        rest = &rest[end + 2..];
                    } else {
                        out.push_str(&unescape_css_string(&rest[1..]));
                        break;
                    }
                } else {
                    // keyword/function token
                    let end = content_token_end(rest);
                    let tok = &rest[..end];
                    match tok {
                        "open-quote" => out.push_str(&quote_string(quotes, true)),
                        "close-quote" => out.push_str(&quote_string(quotes, false)),
                        "no-open-quote" | "no-close-quote" => {}
                        _ => {
                            if (tok.starts_with("counter(") || tok.starts_with("counters("))
                                && tok.ends_with(')')
                            {
                                out.push('\x01');
                                out.push_str(tok);
                                out.push('\x01');
                            } else if tok.starts_with("attr(") && tok.ends_with(')') {
                                if let Some(attrs) = attrs {
                                    out.push_str(&resolve_attr_content(tok, attrs));
                                }
                            }
                        }
                    }
                    rest = &rest[end..];
                }
            }
            out
        }
    }
}

fn quote_string(quotes: Option<&[String]>, open: bool) -> String {
    quotes
        .and_then(|items| items.get(if open { 0 } else { 1 }))
        .cloned()
        .unwrap_or_else(|| if open { "\u{201C}" } else { "\u{201D}" }.to_string())
}

fn resolve_attr_content(tok: &str, attrs: &crate::dom::attrs::AttrMap) -> String {
    let inner = tok
        .strip_prefix("attr(")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or("")
        .trim();
    let args = split_counter_function_args(inner);
    let Some(name_arg) = args.first() else {
        return String::new();
    };
    let name = name_arg.split_whitespace().next().unwrap_or("").trim();
    if name.is_empty() {
        return String::new();
    }
    if let Some(value) = attrs.get(name) {
        return value.clone();
    }
    args.get(1)
        .map(|fallback| unquote_content_arg(fallback.trim()))
        .unwrap_or_default()
}

fn content_token_end(rest: &str) -> usize {
    let mut chars = rest.char_indices();
    let Some((_, first)) = chars.next() else {
        return 0;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '-') {
        return rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    }
    let bytes = rest.as_bytes();
    let mut ident_end = first.len_utf8();
    while ident_end < bytes.len() {
        let b = bytes[ident_end];
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            ident_end += 1;
        } else {
            break;
        }
    }
    if bytes.get(ident_end) != Some(&b'(') {
        return rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    }

    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escape = false;
    for (i, c) in rest.char_indices().skip_while(|(i, _)| *i < ident_end) {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + c.len_utf8();
                }
            }
            _ => {}
        }
    }
    rest.len()
}

/// Resolve counter() and counters() function calls in ::before/::after content.
pub(crate) fn resolve_counters_in_content(
    content: &str,
    counters: &HashMap<String, Vec<i32>>,
) -> String {
    if !content.contains('\x01') {
        return content.to_string();
    }
    // Placeholder \x01counter(name)\x01 was inserted by resolve_content_value
    let mut out = String::new();
    let mut rest = content;
    while let Some(start) = rest.find('\x01') {
        out.push_str(&rest[..start]);
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('\x01') {
            let func = &rest[..end];
            rest = &rest[end + 1..];
            if let Some(inner) = func
                .strip_prefix("counter(")
                .and_then(|s| s.strip_suffix(')'))
            {
                let mut parts = inner.splitn(2, ',');
                let name = parts.next().unwrap_or("").trim();
                let style = parts.next().map(str::trim).unwrap_or("decimal");
                let val = counters
                    .get(name)
                    .and_then(|s| s.last())
                    .copied()
                    .unwrap_or(0);
                out.push_str(&format_counter_value(val, style));
            } else if let Some(inner) = func
                .strip_prefix("counters(")
                .and_then(|s| s.strip_suffix(')'))
            {
                let parts = split_counter_function_args(inner);
                let name = parts.get(0).map(|s| s.trim()).unwrap_or("");
                let sep = parts
                    .get(1)
                    .map(|s| unquote_content_arg(s.trim()))
                    .unwrap_or_default();
                let style = parts.get(2).map(|s| s.trim()).unwrap_or("decimal");
                if let Some(stack) = counters.get(name) {
                    let values: Vec<String> = stack
                        .iter()
                        .map(|value| format_counter_value(*value, style))
                        .collect();
                    out.push_str(&values.join(&sep));
                }
            } else {
                out.push_str(func);
            }
        }
    }
    out.push_str(rest);
    out
}

fn split_counter_function_args(inner: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut escape = false;
    for (i, c) in inner.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            ',' => {
                args.push(inner[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim());
    args
}

fn unquote_content_arg(arg: &str) -> String {
    if ((arg.starts_with('"') && arg.ends_with('"'))
        || (arg.starts_with('\'') && arg.ends_with('\'')))
        && arg.len() >= 2
    {
        unescape_css_string(&arg[1..arg.len() - 1])
    } else {
        arg.to_string()
    }
}

pub(crate) fn format_counter_value(value: i32, style: &str) -> String {
    match style.trim().to_ascii_lowercase().as_str() {
        "decimal-leading-zero" => {
            if (0..10).contains(&value) {
                format!("0{value}")
            } else if (-9..0).contains(&value) {
                format!("-0{}", value.abs())
            } else {
                value.to_string()
            }
        }
        "lower-alpha" | "lower-latin" => alpha_counter(value, false),
        "upper-alpha" | "upper-latin" => alpha_counter(value, true),
        "lower-roman" => roman_counter(value, false),
        "upper-roman" => roman_counter(value, true),
        "lower-greek" => greek_counter(value),
        "cjk-decimal" => cjk_decimal_counter(value),
        "armenian" => armenian_counter(value),
        "georgian" => georgian_counter(value),
        "hebrew" => hebrew_counter(value),
        "hiragana" => kana_counter(value, HIRAGANA_GOJUON),
        "katakana" => kana_counter(value, KATAKANA_GOJUON),
        "hiragana-iroha" => kana_counter(value, HIRAGANA_IROHA),
        "katakana-iroha" => kana_counter(value, KATAKANA_IROHA),
        _ => value.to_string(),
    }
}

fn alpha_counter(mut value: i32, uppercase: bool) -> String {
    if value <= 0 {
        return value.to_string();
    }
    let mut out = Vec::new();
    while value > 0 {
        value -= 1;
        let base = if uppercase { b'A' } else { b'a' };
        out.push((base + (value % 26) as u8) as char);
        value /= 26;
    }
    out.iter().rev().collect()
}

fn roman_counter(value: i32, uppercase: bool) -> String {
    if value <= 0 || value >= 4000 {
        return value.to_string();
    }
    let mut n = value;
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    if uppercase {
        out
    } else {
        out.to_ascii_lowercase()
    }
}

fn additive_counter(value: i32, table: &[(i32, &str)]) -> String {
    if value <= 0 {
        return value.to_string();
    }
    let mut n = value;
    let mut out = String::new();
    for (amount, symbol) in table {
        while n >= *amount {
            out.push_str(symbol);
            n -= *amount;
        }
    }
    if n == 0 {
        out
    } else {
        value.to_string()
    }
}

fn armenian_counter(value: i32) -> String {
    additive_counter(
        value,
        &[
            (9000, "Ք"),
            (8000, "Փ"),
            (7000, "Ւ"),
            (6000, "Ց"),
            (5000, "Ր"),
            (4000, "Տ"),
            (3000, "Վ"),
            (2000, "Ս"),
            (1000, "Ռ"),
            (900, "Ջ"),
            (800, "Պ"),
            (700, "Չ"),
            (600, "Ո"),
            (500, "Շ"),
            (400, "Ն"),
            (300, "Յ"),
            (200, "Մ"),
            (100, "Ճ"),
            (90, "Ղ"),
            (80, "Ձ"),
            (70, "Հ"),
            (60, "Կ"),
            (50, "Ծ"),
            (40, "Խ"),
            (30, "Լ"),
            (20, "Ի"),
            (10, "Ժ"),
            (9, "Թ"),
            (8, "Ը"),
            (7, "Է"),
            (6, "Զ"),
            (5, "Ե"),
            (4, "Դ"),
            (3, "Գ"),
            (2, "Բ"),
            (1, "Ա"),
        ],
    )
}

fn georgian_counter(value: i32) -> String {
    additive_counter(
        value,
        &[
            (10000, "ჵ"),
            (9000, "ჰ"),
            (8000, "ჯ"),
            (7000, "ჴ"),
            (6000, "ხ"),
            (5000, "ჭ"),
            (4000, "წ"),
            (3000, "ძ"),
            (2000, "ც"),
            (1000, "ჩ"),
            (900, "შ"),
            (800, "ყ"),
            (700, "ღ"),
            (600, "ქ"),
            (500, "ფ"),
            (400, "უ"),
            (300, "ტ"),
            (200, "ს"),
            (100, "რ"),
            (90, "ჟ"),
            (80, "პ"),
            (70, "ო"),
            (60, "ჲ"),
            (50, "ნ"),
            (40, "მ"),
            (30, "ლ"),
            (20, "კ"),
            (10, "ი"),
            (9, "თ"),
            (8, "ჱ"),
            (7, "ზ"),
            (6, "ვ"),
            (5, "ე"),
            (4, "დ"),
            (3, "გ"),
            (2, "ბ"),
            (1, "ა"),
        ],
    )
}

fn hebrew_counter(value: i32) -> String {
    match value {
        15 => "ט״ו".to_string(),
        16 => "ט״ז".to_string(),
        _ => additive_counter(
            value,
            &[
                (400, "ת"),
                (300, "ש"),
                (200, "ר"),
                (100, "ק"),
                (90, "צ"),
                (80, "פ"),
                (70, "ע"),
                (60, "ס"),
                (50, "נ"),
                (40, "מ"),
                (30, "ל"),
                (20, "כ"),
                (10, "י"),
                (9, "ט"),
                (8, "ח"),
                (7, "ז"),
                (6, "ו"),
                (5, "ה"),
                (4, "ד"),
                (3, "ג"),
                (2, "ב"),
                (1, "א"),
            ],
        ),
    }
}

fn kana_counter(value: i32, symbols: &[&str]) -> String {
    if value <= 0 {
        return value.to_string();
    }
    let base = symbols.len() as i32;
    let mut n = value;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(symbols[(n % base) as usize]);
        n /= base;
    }
    out.into_iter().rev().collect()
}

const HIRAGANA_GOJUON: &[&str] = &[
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た",
    "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み",
    "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん",
];

const KATAKANA_GOJUON: &[&str] = &[
    "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ",
    "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ",
    "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン",
];

const HIRAGANA_IROHA: &[&str] = &[
    "い", "ろ", "は", "に", "ほ", "へ", "と", "ち", "り", "ぬ", "る", "を", "わ", "か", "よ", "た",
    "れ", "そ", "つ", "ね", "な", "ら", "む", "う", "ゐ", "の", "お", "く", "や", "ま", "け", "ふ",
    "こ", "え", "て", "あ", "さ", "き", "ゆ", "め", "み", "し", "ゑ", "ひ", "も", "せ", "す",
];

const KATAKANA_IROHA: &[&str] = &[
    "イ", "ロ", "ハ", "ニ", "ホ", "ヘ", "ト", "チ", "リ", "ヌ", "ル", "ヲ", "ワ", "カ", "ヨ", "タ",
    "レ", "ソ", "ツ", "ネ", "ナ", "ラ", "ム", "ウ", "ヰ", "ノ", "オ", "ク", "ヤ", "マ", "ケ", "フ",
    "コ", "エ", "テ", "ア", "サ", "キ", "ユ", "メ", "ミ", "シ", "ヱ", "ヒ", "モ", "セ", "ス",
];

/// Parse CSS counter list: "name1 3 name2 name3 -1".
///
/// The default value is property-specific: counter-increment defaults omitted
/// integers to 1, while counter-reset and counter-set default them to 0.
pub fn parse_counter_list_with_default(v: &str, default_value: i32) -> Vec<(String, i32)> {
    if v == "none" {
        return Vec::new();
    }
    let mut result = Vec::new();
    let toks: Vec<&str> = v.split_whitespace().collect();
    let mut i = 0;
    while i < toks.len() {
        let name = toks[i].to_string();
        i += 1;
        let val = if i < toks.len() {
            if let Ok(n) = toks[i].parse::<i32>() {
                i += 1;
                n
            } else {
                default_value
            }
        } else {
            default_value
        };
        result.push((name, val));
    }
    result
}

pub fn parse_counter_list(v: &str) -> Vec<(String, i32)> {
    parse_counter_list_with_default(v, 1)
}

/// Split a shorthand value into top-level tokens, respecting parentheses.
/// E.g. "rgb(200,200,200) red" → ["rgb(200,200,200)", "red"]
pub fn split_shorthand_values(v: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (i, ch) in v.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                if start.is_none() {
                    start = Some(i);
                }
            }
            ')' => {
                depth = depth.saturating_sub(1);
            }
            _ if ch.is_whitespace() && depth == 0 => {
                if let Some(s) = start {
                    parts.push(&v[s..i]);
                    start = None;
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(i);
                }
            }
        }
    }
    if let Some(s) = start {
        parts.push(&v[s..]);
    }
    parts
}

pub fn apply_shorthand_4<F: Fn(&str) -> CssLength>(
    v: &str,
    top: &mut CssLength,
    right: &mut CssLength,
    bottom: &mut CssLength,
    left: &mut CssLength,
    parse: F,
) {
    // Split on whitespace but keep calc()/clamp()/var() intact
    let parts = split_css_values(v);
    match parts.len() {
        1 => {
            let x = parse(&parts[0]);
            *top = x.clone();
            *right = x.clone();
            *bottom = x.clone();
            *left = x;
        }
        2 => {
            let tb = parse(&parts[0]);
            let rl = parse(&parts[1]);
            *top = tb.clone();
            *bottom = tb;
            *right = rl.clone();
            *left = rl;
        }
        3 => {
            *top = parse(&parts[0]);
            let rl = parse(&parts[1]);
            *right = rl.clone();
            *left = rl;
            *bottom = parse(&parts[2]);
        }
        4 => {
            *top = parse(&parts[0]);
            *right = parse(&parts[1]);
            *bottom = parse(&parts[2]);
            *left = parse(&parts[3]);
        }
        _ => {}
    }
}

pub fn apply_border_shorthand(style: &mut ComputedStyle, v: &str) {
    // border: <width> <style> <color>
    // Split on whitespace but keep calc()/var() expressions intact
    for part in split_css_values(v).iter() {
        let part = part.as_str();
        if let Some(bs) = try_parse_border_style(part) {
            style.border_top_style = bs;
            style.border_right_style = bs;
            style.border_bottom_style = bs;
            style.border_left_style = bs;
        } else if let Some(c) = parse_color(part) {
            style.border_top_color = c;
            style.border_right_color = c;
            style.border_bottom_color = c;
            style.border_left_color = c;
        } else {
            let w = parse_length(part);
            // `parse_length` reports an unrecognised token as `auto`; the
            // intrinsic keywords are equally not border widths.
            if !w.is_auto() {
                style.border_top_width = w.clone();
                style.border_right_width = w.clone();
                style.border_bottom_width = w.clone();
                style.border_left_width = w;
            }
        }
    }
}

pub fn apply_border_side_shorthand(
    v: &str,
    width: &mut CssLength,
    style: &mut BorderStyle,
    color: &mut Color,
) {
    for part in split_css_values(v).iter() {
        let part = part.as_str();
        if let Some(bs) = try_parse_border_style(part) {
            *style = bs;
        } else if let Some(c) = parse_color(part) {
            *color = c;
        } else {
            let w = parse_length(part);
            // `parse_length` reports an unrecognised token as `auto`; the
            // intrinsic keywords are equally not border widths.
            if !w.is_auto() {
                *width = w;
            }
        }
    }
}

pub fn extract_url(v: &str) -> Option<String> {
    let lower = v.to_lowercase();
    let start = lower.find("url(")?;
    let inner = v[start + 4..].trim();
    let inner = inner.trim_start_matches('"').trim_start_matches('\'');
    let end = inner.find(|c| c == ')' || c == '"' || c == '\'')?;
    Some(inner[..end].to_string())
}

/// Resolve all `url()` references in CSS text relative to the CSS file's URL.
/// This ensures that `url('../image.jpg')` in an external stylesheet is resolved
/// relative to the stylesheet, not the HTML document.
pub fn resolve_css_urls(css: &str, css_base_url: &str) -> String {
    if css_base_url.is_empty() || !css_base_url.contains("://") {
        return css.to_string();
    }
    // Find the directory of the CSS file URL
    let css_dir = if let Some(last_slash) = css_base_url.rfind('/') {
        &css_base_url[..=last_slash]
    } else {
        css_base_url
    };

    let mut result = String::with_capacity(css.len());
    let mut remaining = css;
    while let Some(url_start) = remaining.to_lowercase().find("url(") {
        // Copy everything before url(
        result.push_str(&remaining[..url_start]);
        let after_url = &remaining[url_start + 4..];
        let _inner = after_url.trim_start();
        // Find the closing )
        let mut depth = 1;
        let mut end_idx = 0;
        for (i, ch) in after_url.char_indices() {
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end_idx = i;
                    break;
                }
            }
        }
        if end_idx == 0 {
            // Malformed — just copy as-is
            result.push_str(&remaining[url_start..]);
            break;
        }
        let url_content = after_url[..end_idx].trim();
        let url_content = url_content.trim_matches('"').trim_matches('\'');

        // Only resolve relative URLs (not absolute, data:, or already-resolved)
        let resolved = if url_content.contains("://")
            || url_content.starts_with("data:")
            || url_content.starts_with('/')
        {
            url_content.to_string()
        } else {
            format!("{}{}", css_dir, url_content)
        };

        result.push_str(&format!("url('{}')", resolved));
        remaining = &after_url[end_idx + 1..];
    }
    result.push_str(remaining);
    result
}

/// Parse a CSS shadow value: `offset_x offset_y [blur] [color]`
/// Returns (offset_x, offset_y, blur_radius, color).
pub fn parse_shadow_value(v: &str) -> (f32, f32, f32, Color) {
    let mut nums: Vec<f32> = Vec::new();
    let mut color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    // Use paren-aware splitting so rgba(r, g, b, a) stays as one token
    let tokens = split_paren_aware(v);
    for tok in &tokens {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(c) = parse_color(t) {
            color = c;
        } else if let Ok(n) = t.trim_end_matches("px").parse::<f32>() {
            nums.push(n);
        }
    }
    let ox = nums.first().copied().unwrap_or(0.0);
    let oy = nums.get(1).copied().unwrap_or(0.0);
    let blur = nums.get(2).copied().unwrap_or(0.0);
    (ox, oy, blur, color)
}

/// Split a string on whitespace, but keep parenthesized groups together.
/// e.g. "2px 2px rgba(0, 0, 0, 0.5)" → ["2px", "2px", "rgba(0, 0, 0, 0.5)"]
fn split_paren_aware(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(c);
            }
            ' ' | '\t' if depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Find the byte index of a space that is not nested inside parentheses.
pub fn find_split_space(v: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in v.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ' ' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Split on the commas that separate a function's arguments, leaving the ones
/// inside a nested function — `rgb(255, 0, 0)` — alone.
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => {
                out.push(s[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[start..].to_string());
    out
}

fn greek_counter(mut value: i32) -> String {
    if value <= 0 {
        return value.to_string();
    }
    const DIGITS: [char; 24] = [
        'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ',
        'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
    ];
    let mut out = Vec::new();
    while value > 0 {
        value -= 1;
        out.push(DIGITS[(value % DIGITS.len() as i32) as usize]);
        value /= DIGITS.len() as i32;
    }
    out.iter().rev().collect()
}

fn cjk_decimal_counter(value: i32) -> String {
    let digits = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    let mut out = String::new();
    if value < 0 {
        out.push('-');
    }
    for ch in value.abs().to_string().chars() {
        if let Some(digit) = ch.to_digit(10) {
            out.push(digits[digit as usize]);
        }
    }
    out
}

/// Byte index of the first space outside any parentheses, which is where a
/// colour stop's position begins.
fn find_top_level_space(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            c if c.is_whitespace() && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// The `<angle>` or `to <side>` that may open a `linear-gradient()`.
/// `None` when the component is not a direction at all — the gradient then
/// runs to the bottom and this component is a colour stop.
fn parse_gradient_direction(dir: &str) -> Option<GradientDirection> {
    let dir = dir.trim().to_ascii_lowercase();
    for (unit, per_turn) in [
        ("deg", 360.0f32),
        ("grad", 400.0),
        ("rad", std::f32::consts::TAU),
        ("turn", 1.0),
    ] {
        if let Some(num) = dir.strip_suffix(unit) {
            if let Ok(n) = num.trim().parse::<f32>() {
                return Some(GradientDirection::Angle(n * 360.0 / per_turn));
            }
        }
    }
    let sides = dir.strip_prefix("to ")?;
    // Side keywords may be written in either order.
    let mut top = false;
    let mut bottom = false;
    let mut left = false;
    let mut right = false;
    for word in sides.split_whitespace() {
        match word {
            "top" => top = true,
            "bottom" => bottom = true,
            "left" => left = true,
            "right" => right = true,
            _ => return None,
        }
    }
    match (top, bottom, left, right) {
        (true, false, false, false) => Some(GradientDirection::Angle(0.0)),
        (false, false, false, true) => Some(GradientDirection::Angle(90.0)),
        (false, true, false, false) => Some(GradientDirection::Angle(180.0)),
        (false, false, true, false) => Some(GradientDirection::Angle(270.0)),
        (true, false, false, true) => Some(GradientDirection::Corner { x: 1, y: -1 }),
        (false, true, false, true) => Some(GradientDirection::Corner { x: 1, y: 1 }),
        (false, true, true, false) => Some(GradientDirection::Corner { x: -1, y: 1 }),
        (true, false, true, false) => Some(GradientDirection::Corner { x: -1, y: -1 }),
        _ => None,
    }
}

/// The body of a function starting at `open` — the byte index of its `(` —
/// without the parentheses.
///
/// Trimming a trailing `)` instead would swallow the closing paren of a nested
/// `rgb(...)`, so the matching paren is counted.
fn function_body(s: &str, open: usize) -> &str {
    let mut depth = 0usize;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return &s[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    &s[open + 1..]
}

/// A colour stop as authored. `pos` is `None` when the position was omitted —
/// the fixup rules (css-images-3 §4.3.1) assign it.
struct RawStop {
    color: Color,
    pos: Option<f32>,
}

/// A stop position as a fraction of the gradient line.
///
/// Only `<percentage>` resolves here: a `<length>` is measured along the
/// gradient line, whose used length is a layout result that the cascade has
/// not reached yet, so the stop is left unpositioned and spaced by fixup.
fn parse_stop_position(p: &str) -> Option<f32> {
    p.trim()
        .strip_suffix('%')?
        .trim()
        .parse::<f32>()
        .ok()
        .map(|n| n / 100.0)
}

/// Parse one `<linear-color-stop>` = `<color> <length-percentage>{0,2}`,
/// appending the stops it yields.
///
/// The two-position shorthand is the same colour twice: `red 10% 20%` is a
/// solid red band from 10% to 20%.
fn push_color_stop(component: &str, out: &mut Vec<RawStop>) {
    let c = component.trim();
    if c.is_empty() {
        return;
    }
    // The colour may itself be a function, so its end is the first space
    // OUTSIDE any parentheses.
    let split = find_top_level_space(c).unwrap_or(c.len());
    let color = match parse_color(&c[..split]) {
        Some(c) => c,
        None => return,
    };
    let rest = c.get(split..).map(str::trim).unwrap_or("");
    if rest.is_empty() {
        out.push(RawStop { color, pos: None });
        return;
    }
    for tok in rest.split_whitespace().take(2) {
        out.push(RawStop {
            color,
            pos: parse_stop_position(tok),
        });
    }
}

/// Resolve every stop position, in the four ordered steps of css-images-3
/// §4.3.1 "Color Stop Fixup". Afterwards every stop has a definite position
/// and the list is in ascending order.
fn fixup_color_stops(stops: &mut [RawStop]) {
    if stops.is_empty() {
        return;
    }
    // 1 & 2: the outermost stops anchor the ends of the gradient line.
    if stops[0].pos.is_none() {
        stops[0].pos = Some(0.0);
    }
    let last = stops.len() - 1;
    if stops[last].pos.is_none() {
        stops[last].pos = Some(1.0);
    }
    // 3: a stop may not precede one declared before it — clamp it up to the
    // largest position seen so far.
    let mut running = f32::NEG_INFINITY;
    for s in stops.iter_mut() {
        if let Some(p) = s.pos {
            if p < running {
                s.pos = Some(running);
            } else {
                running = p;
            }
        }
    }
    // 4: each run of unpositioned stops spreads evenly between the positioned
    // stops around it — not evenly across the whole list.
    let mut i = 0;
    while i < stops.len() {
        if stops[i].pos.is_some() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = start;
        while end < stops.len() && stops[end].pos.is_none() {
            end += 1;
        }
        // Steps 1 and 2 positioned both ends, so a run always has a positioned
        // neighbour on each side.
        let before = stops[start - 1].pos.unwrap_or(0.0);
        let after = stops.get(end).and_then(|s| s.pos).unwrap_or(1.0);
        let gaps = (end - start + 1) as f32;
        for (k, idx) in (start..end).enumerate() {
            stops[idx].pos = Some(before + (after - before) * (k as f32 + 1.0) / gaps);
        }
        i = end;
    }
}

pub fn apply_gradient(style: &mut ComputedStyle, v: &str) {
    // `background` and `background-image` take a comma-separated list of
    // LAYERS, and only one gradient fits in `ComputedStyle`, so the first layer
    // carrying one wins. The later layers must stay out of it: their stops are
    // not part of this gradient's colour stop list.
    let layers = split_top_level_commas(v);
    let layer = match layers
        .iter()
        .find(|l| l.to_ascii_lowercase().contains("gradient"))
    {
        Some(l) => l.clone(),
        None => return,
    };
    // `to_ascii_lowercase` keeps byte offsets, so an index found in it indexes
    // the original.
    let lower = layer.to_ascii_lowercase();
    let (kind, name_at) = if let Some(i) = lower.find("linear-gradient") {
        (GradientType::Linear, i)
    } else if let Some(i) = lower.find("radial-gradient") {
        (GradientType::Radial, i)
    } else {
        return;
    };
    let open = match layer[name_at..].find('(') {
        Some(i) => name_at + i,
        None => return,
    };
    let inner = function_body(&layer, open);
    let mut args = split_top_level_commas(inner);
    if args.is_empty() {
        return;
    }

    style.gradient_type = kind;
    match kind {
        GradientType::Linear => {
            // **The direction is OPTIONAL** (css-images-3 §3.4.1). Only consume
            // the first component when it really is one; otherwise it is a
            // colour stop and belongs to the stop list.
            let angle = parse_gradient_direction(args[0].trim());
            if angle.is_some() {
                args.remove(0);
            }
            style.gradient_direction = angle.unwrap_or_default();
            style.gradient_angle = match style.gradient_direction {
                GradientDirection::Angle(angle) => angle,
                GradientDirection::Corner { x, y } => match (x, y) {
                    (1, -1) => 45.0,
                    (1, 1) => 135.0,
                    (-1, 1) => 225.0,
                    _ => 315.0,
                },
            };
        }
        GradientType::Radial => {
            // The first component is the optional
            // `[<shape> || <size>] [at <position>]` descriptor exactly when it
            // is not a colour stop. The test is on the component's COLOUR part,
            // since a positioned first stop (`red 10%`) is not a `<color>` on
            // its own and would otherwise be eaten as a descriptor.
            let first = args[0].trim();
            let split = find_top_level_space(first).unwrap_or(first.len());
            if parse_color(&first[..split]).is_none() {
                apply_radial_gradient_descriptor(style, first);
                args.remove(0);
            } else {
                reset_radial_gradient_descriptor(style);
            }
        }
        GradientType::None => {}
    }

    let mut raw: Vec<RawStop> = Vec::with_capacity(args.len());
    for component in &args {
        push_color_stop(component, &mut raw);
    }
    fixup_color_stops(&mut raw);
    let stops = &mut style.rare_mut().gradient_stops;
    stops.clear();
    stops.extend(raw.iter().map(|s| GradientStop {
        color: s.color,
        position: s.pos.unwrap_or(0.0),
    }));
}

fn reset_radial_gradient_descriptor(style: &mut ComputedStyle) {
    style.gradient_radial_shape = GradientRadialShape::Ellipse;
    style.gradient_radial_size = GradientRadialSize::FarthestCorner;
    style.gradient_radial_radius_x = CssLength::Auto;
    style.gradient_radial_radius_y = CssLength::Auto;
    style.gradient_radial_position_x = CssLength::Percent(50.0);
    style.gradient_radial_position_y = CssLength::Percent(50.0);
}

fn apply_radial_gradient_descriptor(style: &mut ComputedStyle, descriptor: &str) {
    reset_radial_gradient_descriptor(style);
    let lower = descriptor.to_ascii_lowercase();
    let (before_at, after_at) = match lower.find(" at ") {
        Some(at) => (&descriptor[..at], Some(&descriptor[at + 4..])),
        None => (descriptor, None),
    };

    let mut radii = Vec::new();
    for token in before_at.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "circle" => style.gradient_radial_shape = GradientRadialShape::Circle,
            "ellipse" => style.gradient_radial_shape = GradientRadialShape::Ellipse,
            "closest-side" => style.gradient_radial_size = GradientRadialSize::ClosestSide,
            "farthest-side" => style.gradient_radial_size = GradientRadialSize::FarthestSide,
            "closest-corner" => style.gradient_radial_size = GradientRadialSize::ClosestCorner,
            "farthest-corner" => style.gradient_radial_size = GradientRadialSize::FarthestCorner,
            _ => {
                let len = parse_length(token);
                if !len.is_auto() {
                    radii.push(len);
                }
            }
        }
    }
    if let Some(rx) = radii.first() {
        style.gradient_radial_radius_x = rx.clone();
        style.gradient_radial_radius_y = radii.get(1).cloned().unwrap_or_else(|| rx.clone());
    }

    if let Some(position) = after_at {
        let (x, y) = parse_radial_position(position);
        style.gradient_radial_position_x = x;
        style.gradient_radial_position_y = y;
    }
}

fn parse_radial_position(position: &str) -> (CssLength, CssLength) {
    let mut x = CssLength::Percent(50.0);
    let mut y = CssLength::Percent(50.0);
    let mut x_set = false;
    let mut y_set = false;

    for token in position.split_whitespace() {
        match token.to_ascii_lowercase().as_str() {
            "left" => {
                x = CssLength::Percent(0.0);
                x_set = true;
            }
            "right" => {
                x = CssLength::Percent(100.0);
                x_set = true;
            }
            "top" => {
                y = CssLength::Percent(0.0);
                y_set = true;
            }
            "bottom" => {
                y = CssLength::Percent(100.0);
                y_set = true;
            }
            "center" => {
                if !x_set {
                    x = CssLength::Percent(50.0);
                    x_set = true;
                } else if !y_set {
                    y = CssLength::Percent(50.0);
                    y_set = true;
                }
            }
            _ => {
                if let Some(length) = parse_length_checked(token) {
                    if !x_set {
                        x = length;
                        x_set = true;
                    } else if !y_set {
                        y = length;
                        y_set = true;
                    }
                }
            }
        }
    }
    (x, y)
}

/// Return true if a token looks like a font-size value (keyword or length unit).
fn is_font_size_token(tok: &str) -> bool {
    parse_font_size_checked(tok).is_some()
}

pub fn apply_font_shorthand(style: &mut ComputedStyle, v: &str) {
    // CSS font shorthand: [style] [variant] [weight] [stretch] size[/line-height] family-list
    // System font keywords (single-token):
    let keyword = v.trim().to_ascii_lowercase();
    if matches!(
        keyword.as_str(),
        "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar"
    ) {
        return; // Use UA defaults; no overrides.
    }

    let inherited_font_weight = style.relative_font_weight_base.unwrap_or(style.font_weight);
    let defaults = ComputedStyle::default();
    style.font_style = defaults.font_style;
    style.small_caps = defaults.small_caps;
    style.font_variant_alternates = defaults.font_variant_alternates;
    style.font_variant_caps = defaults.font_variant_caps;
    style.font_variant_east_asian = defaults.font_variant_east_asian;
    style.font_variant_emoji = defaults.font_variant_emoji;
    style.font_variant_ligatures = defaults.font_variant_ligatures;
    style.font_variant_numeric = defaults.font_variant_numeric;
    style.font_variant_position = defaults.font_variant_position;
    style.font_weight = defaults.font_weight;
    style.font_stretch = defaults.font_stretch;
    style.line_height = defaults.line_height;
    style.rare_mut().font_feature_settings.clear();
    style.rare_mut().font_variation_settings.clear();

    // Tokenise the shorthand, respecting quoted strings in the family part.
    // Split by whitespace for the pre-family part; the family is everything after size.
    let v = v.trim();
    let mut size_found_at: Option<usize> = None; // byte offset in v
    let mut byte_pos = 0usize;

    // Walk tokens to find the font-size token (first length/keyword that can be a size).
    for tok in v.split_whitespace() {
        // Handle "size/line-height" as a single token.
        let size_tok = if tok.contains('/') {
            tok.splitn(2, '/').next().unwrap_or(tok)
        } else {
            tok
        };

        if is_font_size_token(size_tok) {
            // Parse size (and optional /line-height).
            if tok.contains('/') {
                let mut parts = tok.splitn(2, '/');
                if let Some(size) = parse_font_size_checked(parts.next().unwrap_or("")) {
                    style.font_size = size;
                }
                if let Some(lh) = parts.next() {
                    style.line_height = parse_line_height(lh);
                }
            } else {
                if let Some(size) = parse_font_size_checked(tok) {
                    style.font_size = size;
                }
            }
            size_found_at = Some(byte_pos + tok.len());
            break;
        }

        // Pre-size tokens: style / variant / weight.
        let tok_lower = tok.to_ascii_lowercase();
        match tok_lower.as_str() {
            "italic" | "oblique" => {
                style.font_style = if tok_lower == "italic" {
                    FontStyle::Italic
                } else {
                    FontStyle::Oblique
                };
            }
            "small-caps" => {
                style.small_caps = true;
                style.font_variant_caps = String::from("small-caps");
            }
            "bold" => {
                style.font_weight = FontWeight::Bold;
            }
            "bolder" => {
                style.font_weight = relative_bolder(inherited_font_weight);
            }
            "lighter" => {
                style.font_weight = relative_lighter(inherited_font_weight);
            }
            "ultra-condensed" => {
                style.font_stretch = 50.0;
            }
            "extra-condensed" => {
                style.font_stretch = 62.5;
            }
            "condensed" => {
                style.font_stretch = 75.0;
            }
            "semi-condensed" => {
                style.font_stretch = 87.5;
            }
            "semi-expanded" => {
                style.font_stretch = 112.5;
            }
            "expanded" => {
                style.font_stretch = 125.0;
            }
            "extra-expanded" => {
                style.font_stretch = 150.0;
            }
            "ultra-expanded" => {
                style.font_stretch = 200.0;
            }
            "normal" => {}
            s if parse_absolute_font_weight(s).is_some() => {
                style.font_weight = parse_absolute_font_weight(s).unwrap();
            }
            _ => {}
        }
        byte_pos += tok.len() + 1; // +1 for the space
    }

    // Everything after the size (and optional /lh) token is the font-family list.
    if let Some(after) = size_found_at {
        let family_part = v[after..].trim();
        // Strip any leading /line-height that wasn't part of the size token.
        let family_part = if family_part.starts_with('/') {
            let rest = family_part[1..].trim();
            // The line-height value is the next whitespace-separated token.
            let mut it = rest.splitn(2, char::is_whitespace);
            let lh_tok = it.next().unwrap_or("");
            style.line_height = parse_line_height(lh_tok);
            it.next().unwrap_or("").trim()
        } else {
            family_part
        };
        if !family_part.is_empty() {
            style.font_family = split_font_families(family_part).join(", ");
        }
    }
}

fn relative_bolder(base: FontWeight) -> FontWeight {
    match base.value() {
        0..=349 => FontWeight::Value(400),
        350..=549 => FontWeight::Value(700),
        _ => FontWeight::Value(900),
    }
}

fn relative_lighter(base: FontWeight) -> FontWeight {
    match base.value() {
        0..=549 => FontWeight::Value(100),
        550..=749 => FontWeight::Value(400),
        _ => FontWeight::Value(700),
    }
}

fn parse_absolute_font_weight(v: &str) -> Option<FontWeight> {
    let n = v.parse::<u16>().ok()?;
    if (1..=1000).contains(&n) {
        Some(FontWeight::Value(n))
    } else {
        None
    }
}
