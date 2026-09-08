//! Data-driven CSS property definitions.
//!
//! Each CSS property is defined as a `PropertyDef` with:
//! - Identity (PropertyId, name)
//! - Metadata (inherited, shorthand longhands)
//! - Behavior (apply function, copy function)
//!
//! This replaces the giant match in apply_property_by_id with table lookup.
//! Adding a new property = one entry in `get()`.

use super::properties::PropertyId;
use crate::types::ComputedStyle;

/// Function that applies a CSS value string to a ComputedStyle field.
pub type ApplyFn = fn(style: &mut ComputedStyle, value: &str);

/// Function that copies a property from one ComputedStyle to another (for inheritance).
pub type CopyFn = fn(dst: &mut ComputedStyle, src: &ComputedStyle);

/// Definition of a single CSS property.
pub struct PropertyDef {
    pub id: PropertyId,
    pub name: &'static str,
    pub inherited: bool,
    pub apply: ApplyFn,
    pub copy: CopyFn,
    /// For shorthands: the longhand properties this expands to.
    pub longhands: &'static [PropertyId],
}

// ── No-op defaults ──────────────────────────────────────────────────────────

fn apply_noop(_: &mut ComputedStyle, _: &str) {}
fn copy_noop(_: &mut ComputedStyle, _: &ComputedStyle) {}

fn apply_content(s: &mut ComputedStyle, v: &str) {
    let v = v.trim();
    if v.eq_ignore_ascii_case("normal") || v.eq_ignore_ascii_case("none") {
        s.rare_mut().content.clear();
    } else {
        s.rare_mut().content = v.to_string();
    }
}

fn copy_content(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().content = s.rare().content.clone();
}

// ── Unknown default ─────────────────────────────────────────────────────────

static UNKNOWN_DEF: PropertyDef = PropertyDef {
    id: PropertyId::Unknown,
    name: "",
    inherited: false,
    apply: apply_noop,
    copy: copy_noop,
    longhands: &[],
};

// ── Table lookup ────────────────────────────────────────────────────────────

/// Get the property definition for a PropertyId.
/// Returns a static reference — zero allocation, zero-cost lookup.
/// The compiler optimizes the match to a jump table.
pub fn get(id: PropertyId) -> &'static PropertyDef {
    use PropertyId::*;
    match id {
        // ── Sizing ──
        Width => &PropertyDef {
            id: Width,
            name: "width",
            inherited: false,
            apply: apply_width,
            copy: copy_width,
            longhands: &[],
        },
        Height => &PropertyDef {
            id: Height,
            name: "height",
            inherited: false,
            apply: apply_height,
            copy: copy_height,
            longhands: &[],
        },
        MinWidth => &PropertyDef {
            id: MinWidth,
            name: "min-width",
            inherited: false,
            apply: apply_min_width,
            copy: copy_min_width,
            longhands: &[],
        },
        MinHeight => &PropertyDef {
            id: MinHeight,
            name: "min-height",
            inherited: false,
            apply: apply_min_height,
            copy: copy_min_height,
            longhands: &[],
        },
        MaxWidth => &PropertyDef {
            id: MaxWidth,
            name: "max-width",
            inherited: false,
            apply: apply_max_width,
            copy: copy_max_width,
            longhands: &[],
        },
        MaxHeight => &PropertyDef {
            id: MaxHeight,
            name: "max-height",
            inherited: false,
            apply: apply_max_height,
            copy: copy_max_height,
            longhands: &[],
        },

        // ── Margin ──
        MarginTop => &PropertyDef {
            id: MarginTop,
            name: "margin-top",
            inherited: false,
            apply: apply_margin_top,
            copy: copy_margin_top,
            longhands: &[],
        },
        MarginRight => &PropertyDef {
            id: MarginRight,
            name: "margin-right",
            inherited: false,
            apply: apply_margin_right,
            copy: copy_margin_right,
            longhands: &[],
        },
        MarginBottom => &PropertyDef {
            id: MarginBottom,
            name: "margin-bottom",
            inherited: false,
            apply: apply_margin_bottom,
            copy: copy_margin_bottom,
            longhands: &[],
        },
        MarginLeft => &PropertyDef {
            id: MarginLeft,
            name: "margin-left",
            inherited: false,
            apply: apply_margin_left,
            copy: copy_margin_left,
            longhands: &[],
        },
        Margin => &PropertyDef {
            id: Margin,
            name: "margin",
            inherited: false,
            apply: apply_margin,
            copy: copy_noop,
            longhands: &[MarginTop, MarginRight, MarginBottom, MarginLeft],
        },

        // ── Padding ──
        PaddingTop => &PropertyDef {
            id: PaddingTop,
            name: "padding-top",
            inherited: false,
            apply: apply_padding_top,
            copy: copy_padding_top,
            longhands: &[],
        },
        PaddingRight => &PropertyDef {
            id: PaddingRight,
            name: "padding-right",
            inherited: false,
            apply: apply_padding_right,
            copy: copy_padding_right,
            longhands: &[],
        },
        PaddingBottom => &PropertyDef {
            id: PaddingBottom,
            name: "padding-bottom",
            inherited: false,
            apply: apply_padding_bottom,
            copy: copy_padding_bottom,
            longhands: &[],
        },
        PaddingLeft => &PropertyDef {
            id: PaddingLeft,
            name: "padding-left",
            inherited: false,
            apply: apply_padding_left,
            copy: copy_padding_left,
            longhands: &[],
        },
        Padding => &PropertyDef {
            id: Padding,
            name: "padding",
            inherited: false,
            apply: apply_padding,
            copy: copy_noop,
            longhands: &[PaddingTop, PaddingRight, PaddingBottom, PaddingLeft],
        },

        // ── Color ──
        Color => &PropertyDef {
            id: Color,
            name: "color",
            inherited: true,
            apply: apply_color,
            copy: copy_color,
            longhands: &[],
        },
        BackgroundColor => &PropertyDef {
            id: BackgroundColor,
            name: "background-color",
            inherited: false,
            apply: apply_background_color,
            copy: copy_background_color,
            longhands: &[],
        },
        Fill => &PropertyDef {
            id: Fill,
            name: "fill",
            inherited: true,
            apply: apply_svg_fill,
            copy: copy_svg_fill,
            longhands: &[],
        },
        Stroke => &PropertyDef {
            id: Stroke,
            name: "stroke",
            inherited: true,
            apply: apply_svg_stroke,
            copy: copy_svg_stroke,
            longhands: &[],
        },
        Opacity => &PropertyDef {
            id: Opacity,
            name: "opacity",
            inherited: false,
            apply: apply_opacity,
            copy: copy_opacity,
            longhands: &[],
        },

        // ── Font (inherited) ──
        FontSize => &PropertyDef {
            id: FontSize,
            name: "font-size",
            inherited: true,
            apply: apply_font_size,
            copy: copy_font_size,
            longhands: &[],
        },
        FontFamily => &PropertyDef {
            id: FontFamily,
            name: "font-family",
            inherited: true,
            apply: apply_font_family,
            copy: copy_font_family,
            longhands: &[],
        },
        FontWeight => &PropertyDef {
            id: FontWeight,
            name: "font-weight",
            inherited: true,
            apply: apply_font_weight,
            copy: copy_font_weight,
            longhands: &[],
        },
        FontStyle => &PropertyDef {
            id: FontStyle,
            name: "font-style",
            inherited: true,
            apply: apply_font_style,
            copy: copy_font_style,
            longhands: &[],
        },
        Font => &PropertyDef {
            id: Font,
            name: "font",
            inherited: true,
            apply: apply_font,
            copy: copy_noop,
            longhands: &[FontFamily, FontSize, FontWeight, FontStyle, LineHeight],
        },
        FontVariationSettings => &PropertyDef {
            id: FontVariationSettings,
            name: "font-variation-settings",
            inherited: true,
            apply: apply_font_variation_settings,
            copy: copy_font_variation_settings,
            longhands: &[],
        },
        FontFeatureSettings => &PropertyDef {
            id: FontFeatureSettings,
            name: "font-feature-settings",
            inherited: true,
            apply: apply_font_feature_settings,
            copy: copy_font_feature_settings,
            longhands: &[],
        },
        FontVariant => &PropertyDef {
            id: FontVariant,
            name: "font-variant",
            inherited: true,
            apply: apply_font_variant,
            copy: copy_font_variant,
            longhands: &[
                FontVariantAlternates,
                FontVariantCaps,
                FontVariantEastAsian,
                FontVariantEmoji,
                FontVariantLigatures,
                FontVariantNumeric,
                FontVariantPosition,
            ],
        },
        FontVariantAlternates => &PropertyDef {
            id: FontVariantAlternates,
            name: "font-variant-alternates",
            inherited: true,
            apply: apply_font_variant_alternates,
            copy: copy_font_variant_alternates,
            longhands: &[],
        },
        FontVariantCaps => &PropertyDef {
            id: FontVariantCaps,
            name: "font-variant-caps",
            inherited: true,
            apply: apply_font_variant_caps,
            copy: copy_font_variant_caps,
            longhands: &[],
        },
        FontVariantEastAsian => &PropertyDef {
            id: FontVariantEastAsian,
            name: "font-variant-east-asian",
            inherited: true,
            apply: apply_font_variant_east_asian,
            copy: copy_font_variant_east_asian,
            longhands: &[],
        },
        FontVariantEmoji => &PropertyDef {
            id: FontVariantEmoji,
            name: "font-variant-emoji",
            inherited: true,
            apply: apply_font_variant_emoji,
            copy: copy_font_variant_emoji,
            longhands: &[],
        },
        FontVariantLigatures => &PropertyDef {
            id: FontVariantLigatures,
            name: "font-variant-ligatures",
            inherited: true,
            apply: apply_font_variant_ligatures,
            copy: copy_font_variant_ligatures,
            longhands: &[],
        },
        FontVariantNumeric => &PropertyDef {
            id: FontVariantNumeric,
            name: "font-variant-numeric",
            inherited: true,
            apply: apply_font_variant_numeric,
            copy: copy_font_variant_numeric,
            longhands: &[],
        },
        FontVariantPosition => &PropertyDef {
            id: FontVariantPosition,
            name: "font-variant-position",
            inherited: true,
            apply: apply_font_variant_position,
            copy: copy_font_variant_position,
            longhands: &[],
        },
        FontStretch => &PropertyDef {
            id: FontStretch,
            name: "font-stretch",
            inherited: true,
            apply: apply_font_stretch,
            copy: copy_font_stretch,
            longhands: &[],
        },
        FontSynthesis => &PropertyDef {
            id: FontSynthesis,
            name: "font-synthesis",
            inherited: true,
            apply: apply_font_synthesis,
            copy: copy_noop,
            longhands: &[
                FontSynthesisWeight,
                FontSynthesisStyle,
                FontSynthesisSmallCaps,
                FontSynthesisPosition,
            ],
        },
        FontSynthesisWeight => &PropertyDef {
            id: FontSynthesisWeight,
            name: "font-synthesis-weight",
            inherited: true,
            apply: apply_font_synthesis_weight,
            copy: copy_font_synthesis_weight,
            longhands: &[],
        },
        FontSynthesisStyle => &PropertyDef {
            id: FontSynthesisStyle,
            name: "font-synthesis-style",
            inherited: true,
            apply: apply_font_synthesis_style,
            copy: copy_font_synthesis_style,
            longhands: &[],
        },
        FontSynthesisSmallCaps => &PropertyDef {
            id: FontSynthesisSmallCaps,
            name: "font-synthesis-small-caps",
            inherited: true,
            apply: apply_font_synthesis_small_caps,
            copy: copy_font_synthesis_small_caps,
            longhands: &[],
        },
        FontSynthesisPosition => &PropertyDef {
            id: FontSynthesisPosition,
            name: "font-synthesis-position",
            inherited: true,
            apply: apply_font_synthesis_position,
            copy: copy_font_synthesis_position,
            longhands: &[],
        },
        LineHeight => &PropertyDef {
            id: LineHeight,
            name: "line-height",
            inherited: true,
            apply: apply_line_height,
            copy: copy_line_height,
            longhands: &[],
        },

        // ── Text (inherited) ──
        TextAlign => &PropertyDef {
            id: TextAlign,
            name: "text-align",
            inherited: true,
            apply: apply_text_align,
            copy: copy_text_align,
            longhands: &[],
        },
        TextTransform => &PropertyDef {
            id: TextTransform,
            name: "text-transform",
            inherited: true,
            apply: apply_text_transform,
            copy: copy_text_transform,
            longhands: &[],
        },
        TextIndent => &PropertyDef {
            id: TextIndent,
            name: "text-indent",
            inherited: true,
            apply: apply_text_indent,
            copy: copy_text_indent,
            longhands: &[],
        },
        LetterSpacing => &PropertyDef {
            id: LetterSpacing,
            name: "letter-spacing",
            inherited: true,
            apply: apply_letter_spacing,
            copy: copy_letter_spacing,
            longhands: &[],
        },
        WordSpacing => &PropertyDef {
            id: WordSpacing,
            name: "word-spacing",
            inherited: true,
            apply: apply_word_spacing,
            copy: copy_word_spacing,
            longhands: &[],
        },
        WhiteSpace => &PropertyDef {
            id: WhiteSpace,
            name: "white-space",
            inherited: true,
            apply: apply_white_space,
            copy: copy_white_space,
            longhands: &[],
        },
        Direction => &PropertyDef {
            id: Direction,
            name: "direction",
            inherited: true,
            apply: apply_direction,
            copy: copy_direction,
            longhands: &[],
        },
        Visibility => &PropertyDef {
            id: Visibility,
            name: "visibility",
            inherited: true,
            apply: apply_visibility,
            copy: copy_visibility,
            longhands: &[],
        },
        Cursor => &PropertyDef {
            id: Cursor,
            name: "cursor",
            inherited: true,
            apply: apply_cursor,
            copy: copy_cursor,
            longhands: &[],
        },

        // ── Display & Layout ──
        Display => &PropertyDef {
            id: Display,
            name: "display",
            inherited: false,
            apply: apply_display,
            copy: copy_display,
            longhands: &[],
        },
        Position => &PropertyDef {
            id: Position,
            name: "position",
            inherited: false,
            apply: apply_position,
            copy: copy_position,
            longhands: &[],
        },
        ZIndex => &PropertyDef {
            id: ZIndex,
            name: "z-index",
            inherited: false,
            apply: apply_z_index,
            copy: copy_z_index,
            longhands: &[],
        },
        Float => &PropertyDef {
            id: Float,
            name: "float",
            inherited: false,
            apply: apply_float,
            copy: copy_float,
            longhands: &[],
        },
        Clear => &PropertyDef {
            id: Clear,
            name: "clear",
            inherited: false,
            apply: apply_clear,
            copy: copy_clear,
            longhands: &[],
        },
        BoxSizing => &PropertyDef {
            id: BoxSizing,
            name: "box-sizing",
            inherited: false,
            apply: apply_box_sizing,
            copy: copy_box_sizing,
            longhands: &[],
        },
        OverflowX => &PropertyDef {
            id: OverflowX,
            name: "overflow-x",
            inherited: false,
            apply: apply_overflow_x,
            copy: copy_overflow_x,
            longhands: &[],
        },
        OverflowY => &PropertyDef {
            id: OverflowY,
            name: "overflow-y",
            inherited: false,
            apply: apply_overflow_y,
            copy: copy_overflow_y,
            longhands: &[],
        },
        Overflow => &PropertyDef {
            id: Overflow,
            name: "overflow",
            inherited: false,
            apply: apply_overflow,
            copy: copy_noop,
            longhands: &[OverflowX, OverflowY],
        },

        // ── Position offsets ──
        Top => &PropertyDef {
            id: Top,
            name: "top",
            inherited: false,
            apply: apply_top,
            copy: copy_top,
            longhands: &[],
        },
        Right => &PropertyDef {
            id: Right,
            name: "right",
            inherited: false,
            apply: apply_right,
            copy: copy_right,
            longhands: &[],
        },
        Bottom => &PropertyDef {
            id: Bottom,
            name: "bottom",
            inherited: false,
            apply: apply_bottom,
            copy: copy_bottom,
            longhands: &[],
        },
        Left => &PropertyDef {
            id: Left,
            name: "left",
            inherited: false,
            apply: apply_left,
            copy: copy_left,
            longhands: &[],
        },

        // ── Border ──
        Border => &PropertyDef {
            id: Border,
            name: "border",
            inherited: false,
            apply: apply_border,
            copy: copy_noop,
            longhands: &[
                BorderTopWidth,
                BorderRightWidth,
                BorderBottomWidth,
                BorderLeftWidth,
                BorderTopStyle,
                BorderRightStyle,
                BorderBottomStyle,
                BorderLeftStyle,
                BorderTopColor,
                BorderRightColor,
                BorderBottomColor,
                BorderLeftColor,
            ],
        },
        BorderWidth => &PropertyDef {
            id: BorderWidth,
            name: "border-width",
            inherited: false,
            apply: apply_border_width,
            copy: copy_noop,
            longhands: &[
                BorderTopWidth,
                BorderRightWidth,
                BorderBottomWidth,
                BorderLeftWidth,
            ],
        },
        BorderStyle => &PropertyDef {
            id: BorderStyle,
            name: "border-style",
            inherited: false,
            apply: apply_border_style_sh,
            copy: copy_noop,
            longhands: &[
                BorderTopStyle,
                BorderRightStyle,
                BorderBottomStyle,
                BorderLeftStyle,
            ],
        },
        BorderColor => &PropertyDef {
            id: BorderColor,
            name: "border-color",
            inherited: false,
            apply: apply_border_color_sh,
            copy: copy_noop,
            longhands: &[
                BorderTopColor,
                BorderRightColor,
                BorderBottomColor,
                BorderLeftColor,
            ],
        },
        BorderTopWidth => &PropertyDef {
            id: BorderTopWidth,
            name: "border-top-width",
            inherited: false,
            apply: apply_border_top_width,
            copy: copy_border_top_width,
            longhands: &[],
        },
        BorderRightWidth => &PropertyDef {
            id: BorderRightWidth,
            name: "border-right-width",
            inherited: false,
            apply: apply_border_right_width,
            copy: copy_border_right_width,
            longhands: &[],
        },
        BorderBottomWidth => &PropertyDef {
            id: BorderBottomWidth,
            name: "border-bottom-width",
            inherited: false,
            apply: apply_border_bottom_width,
            copy: copy_border_bottom_width,
            longhands: &[],
        },
        BorderLeftWidth => &PropertyDef {
            id: BorderLeftWidth,
            name: "border-left-width",
            inherited: false,
            apply: apply_border_left_width,
            copy: copy_border_left_width,
            longhands: &[],
        },
        BorderTopStyle => &PropertyDef {
            id: BorderTopStyle,
            name: "border-top-style",
            inherited: false,
            apply: apply_border_top_style,
            copy: copy_border_top_style,
            longhands: &[],
        },
        BorderRightStyle => &PropertyDef {
            id: BorderRightStyle,
            name: "border-right-style",
            inherited: false,
            apply: apply_border_right_style,
            copy: copy_border_right_style,
            longhands: &[],
        },
        BorderBottomStyle => &PropertyDef {
            id: BorderBottomStyle,
            name: "border-bottom-style",
            inherited: false,
            apply: apply_border_bottom_style,
            copy: copy_border_bottom_style,
            longhands: &[],
        },
        BorderLeftStyle => &PropertyDef {
            id: BorderLeftStyle,
            name: "border-left-style",
            inherited: false,
            apply: apply_border_left_style,
            copy: copy_border_left_style,
            longhands: &[],
        },
        BorderTopColor => &PropertyDef {
            id: BorderTopColor,
            name: "border-top-color",
            inherited: false,
            apply: apply_border_top_color,
            copy: copy_border_top_color,
            longhands: &[],
        },
        BorderRightColor => &PropertyDef {
            id: BorderRightColor,
            name: "border-right-color",
            inherited: false,
            apply: apply_border_right_color,
            copy: copy_border_right_color,
            longhands: &[],
        },
        BorderBottomColor => &PropertyDef {
            id: BorderBottomColor,
            name: "border-bottom-color",
            inherited: false,
            apply: apply_border_bottom_color,
            copy: copy_border_bottom_color,
            longhands: &[],
        },
        BorderLeftColor => &PropertyDef {
            id: BorderLeftColor,
            name: "border-left-color",
            inherited: false,
            apply: apply_border_left_color,
            copy: copy_border_left_color,
            longhands: &[],
        },
        BorderTop => &PropertyDef {
            id: BorderTop,
            name: "border-top",
            inherited: false,
            apply: apply_border_top_sh,
            copy: copy_noop,
            longhands: &[BorderTopWidth, BorderTopStyle, BorderTopColor],
        },
        BorderRight => &PropertyDef {
            id: BorderRight,
            name: "border-right",
            inherited: false,
            apply: apply_border_right_sh,
            copy: copy_noop,
            longhands: &[BorderRightWidth, BorderRightStyle, BorderRightColor],
        },
        BorderBottom => &PropertyDef {
            id: BorderBottom,
            name: "border-bottom",
            inherited: false,
            apply: apply_border_bottom_sh,
            copy: copy_noop,
            longhands: &[BorderBottomWidth, BorderBottomStyle, BorderBottomColor],
        },
        BorderLeft => &PropertyDef {
            id: BorderLeft,
            name: "border-left",
            inherited: false,
            apply: apply_border_left_sh,
            copy: copy_noop,
            longhands: &[BorderLeftWidth, BorderLeftStyle, BorderLeftColor],
        },

        // ── Border radius ──
        BorderRadius => &PropertyDef {
            id: BorderRadius,
            name: "border-radius",
            inherited: false,
            apply: apply_border_radius,
            copy: copy_noop,
            longhands: &[
                BorderTopLeftRadius,
                BorderTopRightRadius,
                BorderBottomRightRadius,
                BorderBottomLeftRadius,
            ],
        },
        BorderTopLeftRadius => &PropertyDef {
            id: BorderTopLeftRadius,
            name: "border-top-left-radius",
            inherited: false,
            apply: apply_border_top_left_radius,
            copy: copy_border_top_left_radius,
            longhands: &[],
        },
        BorderTopRightRadius => &PropertyDef {
            id: BorderTopRightRadius,
            name: "border-top-right-radius",
            inherited: false,
            apply: apply_border_top_right_radius,
            copy: copy_border_top_right_radius,
            longhands: &[],
        },
        BorderBottomLeftRadius => &PropertyDef {
            id: BorderBottomLeftRadius,
            name: "border-bottom-left-radius",
            inherited: false,
            apply: apply_border_bottom_left_radius,
            copy: copy_border_bottom_left_radius,
            longhands: &[],
        },
        BorderBottomRightRadius => &PropertyDef {
            id: BorderBottomRightRadius,
            name: "border-bottom-right-radius",
            inherited: false,
            apply: apply_border_bottom_right_radius,
            copy: copy_border_bottom_right_radius,
            longhands: &[],
        },

        BorderImage => &PropertyDef {
            id: BorderImage,
            name: "border-image",
            inherited: false,
            apply: apply_border_image,
            copy: copy_noop,
            longhands: &[
                BorderImageSource,
                BorderImageSlice,
                BorderImageWidth,
                BorderImageOutset,
                BorderImageRepeat,
            ],
        },
        BorderImageSource => &PropertyDef {
            id: BorderImageSource,
            name: "border-image-source",
            inherited: false,
            apply: apply_border_image_source,
            copy: copy_border_image_source,
            longhands: &[],
        },
        BorderImageSlice => &PropertyDef {
            id: BorderImageSlice,
            name: "border-image-slice",
            inherited: false,
            apply: apply_border_image_slice,
            copy: copy_border_image_slice,
            longhands: &[],
        },
        BorderImageWidth => &PropertyDef {
            id: BorderImageWidth,
            name: "border-image-width",
            inherited: false,
            apply: apply_border_image_width,
            copy: copy_border_image_width,
            longhands: &[],
        },
        BorderImageOutset => &PropertyDef {
            id: BorderImageOutset,
            name: "border-image-outset",
            inherited: false,
            apply: apply_border_image_outset,
            copy: copy_border_image_outset,
            longhands: &[],
        },
        BorderImageRepeat => &PropertyDef {
            id: BorderImageRepeat,
            name: "border-image-repeat",
            inherited: false,
            apply: apply_border_image_repeat,
            copy: copy_border_image_repeat,
            longhands: &[],
        },

        // ── Table ──
        BorderCollapse => &PropertyDef {
            id: BorderCollapse,
            name: "border-collapse",
            inherited: true,
            apply: apply_border_collapse,
            copy: copy_border_collapse,
            longhands: &[],
        },
        BorderSpacing => &PropertyDef {
            id: BorderSpacing,
            name: "border-spacing",
            inherited: true,
            apply: apply_border_spacing,
            copy: copy_border_spacing,
            longhands: &[],
        },
        CaptionSide => &PropertyDef {
            id: CaptionSide,
            name: "caption-side",
            inherited: true,
            apply: apply_caption_side,
            copy: copy_caption_side,
            longhands: &[],
        },
        EmptyCells => &PropertyDef {
            id: EmptyCells,
            name: "empty-cells",
            inherited: true,
            apply: apply_empty_cells,
            copy: copy_empty_cells,
            longhands: &[],
        },
        TableLayout => &PropertyDef {
            id: TableLayout,
            name: "table-layout",
            inherited: false,
            apply: apply_table_layout,
            copy: copy_table_layout,
            longhands: &[],
        },

        // ── Vertical align ──
        VerticalAlign => &PropertyDef {
            id: VerticalAlign,
            name: "vertical-align",
            inherited: false,
            apply: apply_vertical_align,
            copy: copy_vertical_align,
            longhands: &[],
        },

        // ── Text decoration ──
        TextDecoration => &PropertyDef {
            id: TextDecoration,
            name: "text-decoration",
            inherited: false,
            apply: apply_text_decoration,
            copy: copy_text_decoration,
            longhands: &[TextDecorationLine, TextDecorationStyle, TextDecorationColor],
        },
        TextDecorationLine => &PropertyDef {
            id: TextDecorationLine,
            name: "text-decoration-line",
            inherited: false,
            apply: apply_text_decoration_line,
            copy: copy_text_decoration_line,
            longhands: &[],
        },
        TextDecorationColor => &PropertyDef {
            id: TextDecorationColor,
            name: "text-decoration-color",
            inherited: false,
            apply: apply_text_decoration_color,
            copy: copy_text_decoration_color,
            longhands: &[],
        },
        TextDecorationStyle => &PropertyDef {
            id: TextDecorationStyle,
            name: "text-decoration-style",
            inherited: false,
            apply: apply_text_decoration_style_fn,
            copy: copy_text_decoration_style,
            longhands: &[],
        },
        TextDecorationThickness => &PropertyDef {
            id: TextDecorationThickness,
            name: "text-decoration-thickness",
            inherited: false,
            apply: apply_text_decoration_thickness,
            copy: copy_text_decoration_thickness,
            longhands: &[],
        },
        TextDecorationSkipInk => &PropertyDef {
            id: TextDecorationSkipInk,
            name: "text-decoration-skip-ink",
            inherited: true,
            apply: apply_text_decoration_skip_ink,
            copy: copy_text_decoration_skip_ink,
            longhands: &[],
        },
        TextEmphasis => &PropertyDef {
            id: TextEmphasis,
            name: "text-emphasis",
            inherited: true,
            apply: apply_text_emphasis,
            copy: copy_noop,
            longhands: &[TextEmphasisStyle, TextEmphasisColor],
        },
        TextEmphasisStyle => &PropertyDef {
            id: TextEmphasisStyle,
            name: "text-emphasis-style",
            inherited: true,
            apply: apply_text_emphasis_style,
            copy: copy_text_emphasis_style,
            longhands: &[],
        },
        TextEmphasisColor => &PropertyDef {
            id: TextEmphasisColor,
            name: "text-emphasis-color",
            inherited: true,
            apply: apply_text_emphasis_color,
            copy: copy_text_emphasis_color,
            longhands: &[],
        },
        TextEmphasisPosition => &PropertyDef {
            id: TextEmphasisPosition,
            name: "text-emphasis-position",
            inherited: true,
            apply: apply_text_emphasis_position,
            copy: copy_text_emphasis_position,
            longhands: &[],
        },
        TextUnderlineOffset => &PropertyDef {
            id: TextUnderlineOffset,
            name: "text-underline-offset",
            inherited: true,
            apply: apply_text_underline_offset,
            copy: copy_text_underline_offset,
            longhands: &[],
        },
        TextUnderlinePosition => &PropertyDef {
            id: TextUnderlinePosition,
            name: "text-underline-position",
            inherited: true,
            apply: apply_text_underline_position,
            copy: copy_text_underline_position,
            longhands: &[],
        },
        TextWrap => &PropertyDef {
            id: TextWrap,
            name: "text-wrap",
            inherited: true,
            apply: apply_text_wrap,
            copy: copy_text_wrap,
            longhands: &[],
        },
        TextOverflow => &PropertyDef {
            id: TextOverflow,
            name: "text-overflow",
            inherited: false,
            apply: apply_text_overflow,
            copy: copy_text_overflow,
            longhands: &[],
        },
        TextShadow => &PropertyDef {
            id: TextShadow,
            name: "text-shadow",
            inherited: true,
            apply: apply_text_shadow,
            copy: copy_text_shadow,
            longhands: &[],
        },

        // ── Word / overflow-wrap ──
        WordBreak => &PropertyDef {
            id: WordBreak,
            name: "word-break",
            inherited: true,
            apply: apply_word_break,
            copy: copy_word_break,
            longhands: &[],
        },
        OverflowWrap => &PropertyDef {
            id: OverflowWrap,
            name: "overflow-wrap",
            inherited: true,
            apply: apply_overflow_wrap,
            copy: copy_overflow_wrap,
            longhands: &[],
        },
        WordWrap => &PropertyDef {
            id: WordWrap,
            name: "word-wrap",
            inherited: true,
            apply: apply_overflow_wrap,
            copy: copy_overflow_wrap,
            longhands: &[],
        },

        // ── List style ──
        ListStyleType => &PropertyDef {
            id: ListStyleType,
            name: "list-style-type",
            inherited: true,
            apply: apply_list_style_type,
            copy: copy_list_style_type,
            longhands: &[],
        },
        ListStylePosition => &PropertyDef {
            id: ListStylePosition,
            name: "list-style-position",
            inherited: true,
            apply: apply_list_style_position,
            copy: copy_list_style_position,
            longhands: &[],
        },
        ListStyleImage => &PropertyDef {
            id: ListStyleImage,
            name: "list-style-image",
            inherited: true,
            apply: apply_list_style_image,
            copy: copy_list_style_image,
            longhands: &[],
        },
        ListStyle => &PropertyDef {
            id: ListStyle,
            name: "list-style",
            inherited: true,
            apply: apply_list_style,
            copy: copy_noop,
            longhands: &[ListStyleType, ListStylePosition, ListStyleImage],
        },

        // ── Flexbox ──
        FlexDirection => &PropertyDef {
            id: FlexDirection,
            name: "flex-direction",
            inherited: false,
            apply: apply_flex_direction,
            copy: copy_flex_direction,
            longhands: &[],
        },
        FlexWrap => &PropertyDef {
            id: FlexWrap,
            name: "flex-wrap",
            inherited: false,
            apply: apply_flex_wrap,
            copy: copy_flex_wrap,
            longhands: &[],
        },
        FlexGrow => &PropertyDef {
            id: FlexGrow,
            name: "flex-grow",
            inherited: false,
            apply: apply_flex_grow,
            copy: copy_flex_grow,
            longhands: &[],
        },
        FlexShrink => &PropertyDef {
            id: FlexShrink,
            name: "flex-shrink",
            inherited: false,
            apply: apply_flex_shrink,
            copy: copy_flex_shrink,
            longhands: &[],
        },
        FlexBasis => &PropertyDef {
            id: FlexBasis,
            name: "flex-basis",
            inherited: false,
            apply: apply_flex_basis,
            copy: copy_flex_basis,
            longhands: &[],
        },
        Flex => &PropertyDef {
            id: Flex,
            name: "flex",
            inherited: false,
            apply: apply_flex,
            copy: copy_noop,
            longhands: &[FlexGrow, FlexShrink, FlexBasis],
        },
        FlexFlow => &PropertyDef {
            id: FlexFlow,
            name: "flex-flow",
            inherited: false,
            apply: apply_flex_flow,
            copy: copy_noop,
            longhands: &[FlexDirection, FlexWrap],
        },
        Order => &PropertyDef {
            id: Order,
            name: "order",
            inherited: false,
            apply: apply_order,
            copy: copy_order,
            longhands: &[],
        },
        JustifyContent => &PropertyDef {
            id: JustifyContent,
            name: "justify-content",
            inherited: false,
            apply: apply_justify_content,
            copy: copy_justify_content,
            longhands: &[],
        },
        AlignItems => &PropertyDef {
            id: AlignItems,
            name: "align-items",
            inherited: false,
            apply: apply_align_items,
            copy: copy_align_items,
            longhands: &[],
        },
        AlignSelf => &PropertyDef {
            id: AlignSelf,
            name: "align-self",
            inherited: false,
            apply: apply_align_self,
            copy: copy_align_self,
            longhands: &[],
        },
        AlignContent => &PropertyDef {
            id: AlignContent,
            name: "align-content",
            inherited: false,
            apply: apply_align_content,
            copy: copy_align_content,
            longhands: &[],
        },
        JustifyItems => &PropertyDef {
            id: JustifyItems,
            name: "justify-items",
            inherited: false,
            apply: apply_justify_items,
            copy: copy_justify_items,
            longhands: &[],
        },
        JustifySelf => &PropertyDef {
            id: JustifySelf,
            name: "justify-self",
            inherited: false,
            apply: apply_justify_self,
            copy: copy_justify_self,
            longhands: &[],
        },
        Gap => &PropertyDef {
            id: Gap,
            name: "gap",
            inherited: false,
            apply: apply_gap,
            copy: copy_noop,
            longhands: &[RowGap, ColumnGap],
        },
        RowGap => &PropertyDef {
            id: RowGap,
            name: "row-gap",
            inherited: false,
            apply: apply_row_gap,
            copy: copy_row_gap,
            longhands: &[],
        },
        ColumnGap => &PropertyDef {
            id: ColumnGap,
            name: "column-gap",
            inherited: false,
            apply: apply_column_gap,
            copy: copy_column_gap,
            longhands: &[],
        },

        // ── Grid ──
        GridTemplateColumns => &PropertyDef {
            id: GridTemplateColumns,
            name: "grid-template-columns",
            inherited: false,
            apply: apply_grid_template_columns,
            copy: copy_grid_template_columns,
            longhands: &[],
        },
        GridTemplateRows => &PropertyDef {
            id: GridTemplateRows,
            name: "grid-template-rows",
            inherited: false,
            apply: apply_grid_template_rows,
            copy: copy_grid_template_rows,
            longhands: &[],
        },
        GridTemplateAreas => &PropertyDef {
            id: GridTemplateAreas,
            name: "grid-template-areas",
            inherited: false,
            apply: apply_grid_template_areas,
            copy: copy_grid_template_areas,
            longhands: &[],
        },
        GridAutoColumns => &PropertyDef {
            id: GridAutoColumns,
            name: "grid-auto-columns",
            inherited: false,
            apply: apply_grid_auto_columns,
            copy: copy_grid_auto_columns,
            longhands: &[],
        },
        GridAutoRows => &PropertyDef {
            id: GridAutoRows,
            name: "grid-auto-rows",
            inherited: false,
            apply: apply_grid_auto_rows,
            copy: copy_grid_auto_rows,
            longhands: &[],
        },
        GridAutoFlow => &PropertyDef {
            id: GridAutoFlow,
            name: "grid-auto-flow",
            inherited: false,
            apply: apply_grid_auto_flow,
            copy: copy_grid_auto_flow,
            longhands: &[],
        },
        GridColumn => &PropertyDef {
            id: GridColumn,
            name: "grid-column",
            inherited: false,
            apply: apply_grid_column,
            copy: copy_noop,
            longhands: &[GridColumnStart, GridColumnEnd],
        },
        GridRow => &PropertyDef {
            id: GridRow,
            name: "grid-row",
            inherited: false,
            apply: apply_grid_row,
            copy: copy_noop,
            longhands: &[GridRowStart, GridRowEnd],
        },
        GridColumnStart => &PropertyDef {
            id: GridColumnStart,
            name: "grid-column-start",
            inherited: false,
            apply: apply_grid_column_start,
            copy: copy_grid_column_start,
            longhands: &[],
        },
        GridColumnEnd => &PropertyDef {
            id: GridColumnEnd,
            name: "grid-column-end",
            inherited: false,
            apply: apply_grid_column_end,
            copy: copy_grid_column_end,
            longhands: &[],
        },
        GridRowStart => &PropertyDef {
            id: GridRowStart,
            name: "grid-row-start",
            inherited: false,
            apply: apply_grid_row_start,
            copy: copy_grid_row_start,
            longhands: &[],
        },
        GridRowEnd => &PropertyDef {
            id: GridRowEnd,
            name: "grid-row-end",
            inherited: false,
            apply: apply_grid_row_end,
            copy: copy_grid_row_end,
            longhands: &[],
        },
        GridArea => &PropertyDef {
            id: GridArea,
            name: "grid-area",
            inherited: false,
            apply: apply_grid_area,
            copy: copy_noop,
            longhands: &[GridRowStart, GridColumnStart, GridRowEnd, GridColumnEnd],
        },
        GridTemplate => &PropertyDef {
            id: GridTemplate,
            name: "grid-template",
            inherited: false,
            apply: apply_grid_template,
            copy: copy_noop,
            longhands: &[GridTemplateRows, GridTemplateColumns, GridTemplateAreas],
        },

        // ── Background ──
        Background => &PropertyDef {
            id: Background,
            name: "background",
            inherited: false,
            apply: apply_background,
            copy: copy_noop,
            longhands: &[
                BackgroundColor,
                BackgroundImage,
                BackgroundPosition,
                BackgroundSize,
                BackgroundRepeat,
                BackgroundAttachment,
                BackgroundOrigin,
                BackgroundClip,
            ],
        },
        BackgroundImage => &PropertyDef {
            id: BackgroundImage,
            name: "background-image",
            inherited: false,
            apply: apply_background_image,
            copy: copy_background_image,
            longhands: &[],
        },
        BackgroundSize => &PropertyDef {
            id: BackgroundSize,
            name: "background-size",
            inherited: false,
            apply: apply_background_size,
            copy: copy_background_size,
            longhands: &[],
        },
        BackgroundPosition => &PropertyDef {
            id: BackgroundPosition,
            name: "background-position",
            inherited: false,
            apply: apply_background_position,
            copy: copy_background_position,
            longhands: &[],
        },
        BackgroundRepeat => &PropertyDef {
            id: BackgroundRepeat,
            name: "background-repeat",
            inherited: false,
            apply: apply_background_repeat,
            copy: copy_background_repeat,
            longhands: &[],
        },
        BackgroundClip => &PropertyDef {
            id: BackgroundClip,
            name: "background-clip",
            inherited: false,
            apply: apply_background_clip,
            copy: copy_background_clip,
            longhands: &[],
        },
        BackgroundOrigin => &PropertyDef {
            id: BackgroundOrigin,
            name: "background-origin",
            inherited: false,
            apply: apply_background_origin,
            copy: copy_background_origin,
            longhands: &[],
        },
        BackgroundAttachment => &PropertyDef {
            id: BackgroundAttachment,
            name: "background-attachment",
            inherited: false,
            apply: apply_background_attachment,
            copy: copy_background_attachment,
            longhands: &[],
        },
        BackgroundBlendMode => &PropertyDef {
            id: BackgroundBlendMode,
            name: "background-blend-mode",
            inherited: false,
            apply: apply_background_blend_mode,
            copy: copy_background_blend_mode,
            longhands: &[],
        },

        // ── Mask ──
        Mask => &PropertyDef {
            id: Mask,
            name: "mask",
            inherited: false,
            apply: apply_mask,
            copy: copy_noop,
            longhands: &[
                MaskImage,
                MaskMode,
                MaskRepeat,
                MaskPosition,
                MaskSize,
                MaskClip,
                MaskOrigin,
                MaskComposite,
            ],
        },
        MaskImage => &PropertyDef {
            id: MaskImage,
            name: "mask-image",
            inherited: false,
            apply: apply_mask_image,
            copy: copy_mask_image,
            longhands: &[],
        },
        MaskMode => &PropertyDef {
            id: MaskMode,
            name: "mask-mode",
            inherited: false,
            apply: apply_mask_mode,
            copy: copy_mask_mode,
            longhands: &[],
        },
        MaskRepeat => &PropertyDef {
            id: MaskRepeat,
            name: "mask-repeat",
            inherited: false,
            apply: apply_mask_repeat,
            copy: copy_mask_repeat,
            longhands: &[],
        },
        MaskPosition => &PropertyDef {
            id: MaskPosition,
            name: "mask-position",
            inherited: false,
            apply: apply_mask_position,
            copy: copy_mask_position,
            longhands: &[],
        },
        MaskSize => &PropertyDef {
            id: MaskSize,
            name: "mask-size",
            inherited: false,
            apply: apply_mask_size,
            copy: copy_mask_size,
            longhands: &[],
        },
        MaskClip => &PropertyDef {
            id: MaskClip,
            name: "mask-clip",
            inherited: false,
            apply: apply_mask_clip,
            copy: copy_mask_clip,
            longhands: &[],
        },
        MaskOrigin => &PropertyDef {
            id: MaskOrigin,
            name: "mask-origin",
            inherited: false,
            apply: apply_mask_origin,
            copy: copy_mask_origin,
            longhands: &[],
        },
        MaskComposite => &PropertyDef {
            id: MaskComposite,
            name: "mask-composite",
            inherited: false,
            apply: apply_mask_composite,
            copy: copy_mask_composite,
            longhands: &[],
        },

        // ── Outline ──
        Outline => &PropertyDef {
            id: Outline,
            name: "outline",
            inherited: false,
            apply: apply_outline,
            copy: copy_noop,
            longhands: &[OutlineWidth, OutlineStyle, OutlineColor],
        },
        OutlineStyle => &PropertyDef {
            id: OutlineStyle,
            name: "outline-style",
            inherited: false,
            apply: apply_outline_style,
            copy: copy_outline_style,
            longhands: &[],
        },
        OutlineColor => &PropertyDef {
            id: OutlineColor,
            name: "outline-color",
            inherited: false,
            apply: apply_outline_color,
            copy: copy_outline_color,
            longhands: &[],
        },
        OutlineWidth => &PropertyDef {
            id: OutlineWidth,
            name: "outline-width",
            inherited: false,
            apply: apply_outline_width,
            copy: copy_outline_width,
            longhands: &[],
        },
        OutlineOffset => &PropertyDef {
            id: OutlineOffset,
            name: "outline-offset",
            inherited: false,
            apply: apply_outline_offset,
            copy: copy_outline_offset,
            longhands: &[],
        },

        // ── Box shadow ──
        BoxShadow => &PropertyDef {
            id: BoxShadow,
            name: "box-shadow",
            inherited: false,
            apply: apply_box_shadow,
            copy: copy_box_shadow,
            longhands: &[],
        },

        // ── Pointer events ──
        PointerEvents => &PropertyDef {
            id: PointerEvents,
            name: "pointer-events",
            inherited: true,
            apply: apply_pointer_events,
            copy: copy_pointer_events,
            longhands: &[],
        },

        // ── User interaction ──
        UserSelect => &PropertyDef {
            id: UserSelect,
            name: "user-select",
            inherited: false,
            apply: apply_user_select,
            copy: copy_user_select,
            longhands: &[],
        },
        Resize => &PropertyDef {
            id: Resize,
            name: "resize",
            inherited: false,
            apply: apply_resize,
            copy: copy_resize,
            longhands: &[],
        },

        // ── Object fit/position ──
        ObjectFit => &PropertyDef {
            id: ObjectFit,
            name: "object-fit",
            inherited: false,
            apply: apply_object_fit,
            copy: copy_object_fit,
            longhands: &[],
        },
        ObjectPosition => &PropertyDef {
            id: ObjectPosition,
            name: "object-position",
            inherited: false,
            apply: apply_object_position,
            copy: copy_object_position,
            longhands: &[],
        },
        AspectRatio => &PropertyDef {
            id: AspectRatio,
            name: "aspect-ratio",
            inherited: false,
            apply: apply_aspect_ratio,
            copy: copy_aspect_ratio,
            longhands: &[],
        },

        // ── Transform / filter ──
        Transform => &PropertyDef {
            id: Transform,
            name: "transform",
            inherited: false,
            apply: apply_transform,
            copy: copy_transform,
            longhands: &[],
        },
        TransformBox => &PropertyDef {
            id: TransformBox,
            name: "transform-box",
            inherited: false,
            apply: apply_transform_box,
            copy: copy_transform_box,
            longhands: &[],
        },
        TransformOrigin => &PropertyDef {
            id: TransformOrigin,
            name: "transform-origin",
            inherited: false,
            apply: apply_transform_origin,
            copy: copy_transform_origin,
            longhands: &[],
        },
        Filter => &PropertyDef {
            id: Filter,
            name: "filter",
            inherited: false,
            apply: apply_filter,
            copy: copy_filter,
            longhands: &[],
        },
        BackdropFilter => &PropertyDef {
            id: BackdropFilter,
            name: "backdrop-filter",
            inherited: false,
            apply: apply_backdrop_filter,
            copy: copy_backdrop_filter,
            longhands: &[],
        },
        // Accepted but not implemented
        TransformStyle => &PropertyDef {
            id: TransformStyle,
            name: "transform-style",
            inherited: false,
            apply: apply_transform_style_3d,
            copy: copy_transform_style_3d,
            longhands: &[],
        },
        Perspective => &PropertyDef {
            id: Perspective,
            name: "perspective",
            inherited: false,
            apply: apply_perspective,
            copy: copy_perspective,
            longhands: &[],
        },
        PerspectiveOrigin => &PropertyDef {
            id: PerspectiveOrigin,
            name: "perspective-origin",
            inherited: false,
            apply: apply_perspective_origin,
            copy: copy_perspective_origin,
            longhands: &[],
        },
        BackfaceVisibility => &PropertyDef {
            id: BackfaceVisibility,
            name: "backface-visibility",
            inherited: false,
            apply: apply_backface_visibility,
            copy: copy_backface_visibility,
            longhands: &[],
        },

        // ── Transition / animation ──
        Transition => &PropertyDef {
            id: Transition,
            name: "transition",
            inherited: false,
            apply: apply_transition,
            copy: copy_transition,
            longhands: &[],
        },
        TransitionProperty => &PropertyDef {
            id: TransitionProperty,
            name: "transition-property",
            inherited: false,
            apply: apply_transition_property,
            copy: copy_transition,
            longhands: &[],
        },
        TransitionDuration => &PropertyDef {
            id: TransitionDuration,
            name: "transition-duration",
            inherited: false,
            apply: apply_transition_duration,
            copy: copy_transition,
            longhands: &[],
        },
        TransitionTimingFunction => &PropertyDef {
            id: TransitionTimingFunction,
            name: "transition-timing-function",
            inherited: false,
            apply: apply_transition_timing_function,
            copy: copy_transition,
            longhands: &[],
        },
        TransitionDelay => &PropertyDef {
            id: TransitionDelay,
            name: "transition-delay",
            inherited: false,
            apply: apply_transition_delay,
            copy: copy_transition,
            longhands: &[],
        },
        TransitionBehavior => &PropertyDef {
            id: TransitionBehavior,
            name: "transition-behavior",
            inherited: false,
            apply: apply_transition_behavior,
            copy: copy_transition,
            longhands: &[],
        },
        Animation => &PropertyDef {
            id: Animation,
            name: "animation",
            inherited: false,
            apply: apply_animation,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationName => &PropertyDef {
            id: AnimationName,
            name: "animation-name",
            inherited: false,
            apply: apply_animation_name,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationDuration => &PropertyDef {
            id: AnimationDuration,
            name: "animation-duration",
            inherited: false,
            apply: apply_animation_duration,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationTimingFunction => &PropertyDef {
            id: AnimationTimingFunction,
            name: "animation-timing-function",
            inherited: false,
            apply: apply_animation_timing_function,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationDelay => &PropertyDef {
            id: AnimationDelay,
            name: "animation-delay",
            inherited: false,
            apply: apply_animation_delay,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationIterationCount => &PropertyDef {
            id: AnimationIterationCount,
            name: "animation-iteration-count",
            inherited: false,
            apply: apply_animation_iteration_count,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationDirection => &PropertyDef {
            id: AnimationDirection,
            name: "animation-direction",
            inherited: false,
            apply: apply_animation_direction,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationFillMode => &PropertyDef {
            id: AnimationFillMode,
            name: "animation-fill-mode",
            inherited: false,
            apply: apply_animation_fill_mode,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationPlayState => &PropertyDef {
            id: AnimationPlayState,
            name: "animation-play-state",
            inherited: false,
            apply: apply_animation_play_state,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationComposition => &PropertyDef {
            id: AnimationComposition,
            name: "animation-composition",
            inherited: false,
            apply: apply_animation_composition,
            copy: copy_animation,
            longhands: &[],
        },
        AnimationTimeline => &PropertyDef {
            id: AnimationTimeline,
            name: "animation-timeline",
            inherited: false,
            apply: apply_animation_timeline,
            copy: copy_animation_timeline,
            longhands: &[],
        },
        WillChange => &PropertyDef {
            id: WillChange,
            name: "will-change",
            inherited: false,
            apply: apply_will_change,
            copy: copy_will_change,
            longhands: &[],
        },

        // ── Unicode-bidi & writing ──
        UnicodeBidi => &PropertyDef {
            id: UnicodeBidi,
            name: "unicode-bidi",
            inherited: false,
            apply: apply_unicode_bidi,
            copy: copy_unicode_bidi,
            longhands: &[],
        },
        WritingMode => &PropertyDef {
            id: WritingMode,
            name: "writing-mode",
            inherited: true,
            apply: apply_writing_mode,
            copy: copy_writing_mode,
            longhands: &[],
        },
        TextOrientation => &PropertyDef {
            id: TextOrientation,
            name: "text-orientation",
            inherited: true,
            apply: apply_text_orientation,
            copy: copy_text_orientation,
            longhands: &[],
        },
        TextCombineUpright => &PropertyDef {
            id: TextCombineUpright,
            name: "text-combine-upright",
            inherited: true,
            apply: apply_text_combine_upright,
            copy: copy_text_combine_upright,
            longhands: &[],
        },

        // ── Hyphens / tab-size / text extras ──
        TabSize => &PropertyDef {
            id: TabSize,
            name: "tab-size",
            inherited: true,
            apply: apply_tab_size,
            copy: copy_tab_size,
            longhands: &[],
        },
        Hyphens => &PropertyDef {
            id: Hyphens,
            name: "hyphens",
            inherited: true,
            apply: apply_hyphens,
            copy: copy_hyphens,
            longhands: &[],
        },
        Widows => &PropertyDef {
            id: Widows,
            name: "widows",
            inherited: true,
            apply: apply_widows,
            copy: copy_widows,
            longhands: &[],
        },
        Orphans => &PropertyDef {
            id: Orphans,
            name: "orphans",
            inherited: true,
            apply: apply_orphans,
            copy: copy_orphans,
            longhands: &[],
        },

        // ── Scrollbar & caret ──
        ScrollbarColor => &PropertyDef {
            id: ScrollbarColor,
            name: "scrollbar-color",
            inherited: false,
            apply: apply_scrollbar_color,
            copy: copy_scrollbar_color,
            longhands: &[],
        },
        ScrollbarWidth => &PropertyDef {
            id: ScrollbarWidth,
            name: "scrollbar-width",
            inherited: false,
            apply: apply_scrollbar_width,
            copy: copy_scrollbar_width,
            longhands: &[],
        },
        ScrollbarGutter => &PropertyDef {
            id: ScrollbarGutter,
            name: "scrollbar-gutter",
            inherited: false,
            apply: apply_scrollbar_gutter,
            copy: copy_scrollbar_gutter,
            longhands: &[],
        },
        ScrollTimeline => &PropertyDef {
            id: ScrollTimeline,
            name: "scroll-timeline",
            inherited: false,
            apply: apply_scroll_timeline,
            copy: copy_scroll_timeline,
            longhands: &[],
        },
        CaretColor => &PropertyDef {
            id: CaretColor,
            name: "caret-color",
            inherited: true,
            apply: apply_caret_color,
            copy: copy_caret_color,
            longhands: &[],
        },

        // ── Quotes ──
        Quotes => &PropertyDef {
            id: Quotes,
            name: "quotes",
            inherited: true,
            apply: apply_quotes,
            copy: copy_quotes,
            longhands: &[],
        },

        // ── Container queries ──
        ContainerType => &PropertyDef {
            id: ContainerType,
            name: "container-type",
            inherited: false,
            apply: apply_container_type,
            copy: copy_container_type,
            longhands: &[],
        },
        ContainerName => &PropertyDef {
            id: ContainerName,
            name: "container-name",
            inherited: false,
            apply: apply_container_name,
            copy: copy_container_name,
            longhands: &[],
        },
        Container => &PropertyDef {
            id: Container,
            name: "container",
            inherited: false,
            apply: apply_container,
            copy: copy_noop,
            longhands: &[ContainerType, ContainerName],
        },
        AnchorName => &PropertyDef {
            id: AnchorName,
            name: "anchor-name",
            inherited: false,
            apply: apply_anchor_name,
            copy: copy_anchor_name,
            longhands: &[],
        },
        PositionAnchor => &PropertyDef {
            id: PositionAnchor,
            name: "position-anchor",
            inherited: false,
            apply: apply_position_anchor,
            copy: copy_position_anchor,
            longhands: &[],
        },
        ViewTransitionName => &PropertyDef {
            id: ViewTransitionName,
            name: "view-transition-name",
            inherited: false,
            apply: apply_view_transition_name,
            copy: copy_view_transition_name,
            longhands: &[],
        },

        // ── Clip ──
        Clip => &PropertyDef {
            id: Clip,
            name: "clip",
            inherited: false,
            apply: apply_clip,
            copy: copy_clip,
            longhands: &[],
        },
        ClipPath => &PropertyDef {
            id: ClipPath,
            name: "clip-path",
            inherited: false,
            apply: apply_clip_path,
            copy: copy_clip_path,
            longhands: &[],
        },
        ShapeOutside => &PropertyDef {
            id: ShapeOutside,
            name: "shape-outside",
            inherited: false,
            apply: apply_shape_outside,
            copy: copy_shape_outside,
            longhands: &[],
        },
        ShapeMargin => &PropertyDef {
            id: ShapeMargin,
            name: "shape-margin",
            inherited: false,
            apply: apply_shape_margin,
            copy: copy_shape_margin,
            longhands: &[],
        },
        OffsetPath => &PropertyDef {
            id: OffsetPath,
            name: "offset-path",
            inherited: false,
            apply: apply_offset_path,
            copy: copy_offset_path,
            longhands: &[],
        },

        // ── Break / page-break ──
        BreakBefore => &PropertyDef {
            id: BreakBefore,
            name: "break-before",
            inherited: false,
            apply: apply_break_before,
            copy: copy_break_before,
            longhands: &[],
        },
        BreakAfter => &PropertyDef {
            id: BreakAfter,
            name: "break-after",
            inherited: false,
            apply: apply_break_after,
            copy: copy_break_after,
            longhands: &[],
        },
        BreakInside => &PropertyDef {
            id: BreakInside,
            name: "break-inside",
            inherited: false,
            apply: apply_break_inside,
            copy: copy_break_inside,
            longhands: &[],
        },
        LineClamp => &PropertyDef {
            id: LineClamp,
            name: "line-clamp",
            inherited: false,
            apply: apply_line_clamp,
            copy: copy_line_clamp,
            longhands: &[],
        },
        PageBreakBefore => &PropertyDef {
            id: PageBreakBefore,
            name: "page-break-before",
            inherited: false,
            apply: apply_break_before,
            copy: copy_break_before,
            longhands: &[],
        },
        PageBreakAfter => &PropertyDef {
            id: PageBreakAfter,
            name: "page-break-after",
            inherited: false,
            apply: apply_break_after,
            copy: copy_break_after,
            longhands: &[],
        },
        PageBreakInside => &PropertyDef {
            id: PageBreakInside,
            name: "page-break-inside",
            inherited: false,
            apply: apply_break_inside,
            copy: copy_break_inside,
            longhands: &[],
        },

        // ── Multi-column ──
        ColumnCount => &PropertyDef {
            id: ColumnCount,
            name: "column-count",
            inherited: false,
            apply: apply_column_count,
            copy: copy_column_count,
            longhands: &[],
        },
        ColumnWidth => &PropertyDef {
            id: ColumnWidth,
            name: "column-width",
            inherited: false,
            apply: apply_column_width,
            copy: copy_column_width,
            longhands: &[],
        },
        Columns => &PropertyDef {
            id: Columns,
            name: "columns",
            inherited: false,
            apply: apply_columns,
            copy: copy_noop,
            longhands: &[ColumnCount, ColumnWidth],
        },
        ColumnRule => &PropertyDef {
            id: ColumnRule,
            name: "column-rule",
            inherited: false,
            apply: apply_column_rule,
            copy: copy_noop,
            longhands: &[ColumnRuleWidth, ColumnRuleStyle, ColumnRuleColor],
        },
        ColumnRuleWidth => &PropertyDef {
            id: ColumnRuleWidth,
            name: "column-rule-width",
            inherited: false,
            apply: apply_column_rule_width,
            copy: copy_column_rule_width,
            longhands: &[],
        },
        ColumnRuleStyle => &PropertyDef {
            id: ColumnRuleStyle,
            name: "column-rule-style",
            inherited: false,
            apply: apply_column_rule_style,
            copy: copy_column_rule_style,
            longhands: &[],
        },
        ColumnRuleColor => &PropertyDef {
            id: ColumnRuleColor,
            name: "column-rule-color",
            inherited: false,
            apply: apply_column_rule_color,
            copy: copy_column_rule_color,
            longhands: &[],
        },
        ColumnFill => &PropertyDef {
            id: ColumnFill,
            name: "column-fill",
            inherited: false,
            apply: apply_column_fill,
            copy: copy_column_fill,
            longhands: &[],
        },
        ColumnSpan => &PropertyDef {
            id: ColumnSpan,
            name: "column-span",
            inherited: false,
            apply: apply_column_span,
            copy: copy_column_span,
            longhands: &[],
        },

        // ── Counter ──
        CounterReset => &PropertyDef {
            id: CounterReset,
            name: "counter-reset",
            inherited: false,
            apply: apply_counter_reset,
            copy: copy_counter_reset,
            longhands: &[],
        },
        CounterIncrement => &PropertyDef {
            id: CounterIncrement,
            name: "counter-increment",
            inherited: false,
            apply: apply_counter_increment,
            copy: copy_counter_increment,
            longhands: &[],
        },
        CounterSet => &PropertyDef {
            id: CounterSet,
            name: "counter-set",
            inherited: false,
            apply: apply_counter_set,
            copy: copy_counter_set,
            longhands: &[],
        },

        // ── Misc ──
        ScrollBehavior => &PropertyDef {
            id: ScrollBehavior,
            name: "scroll-behavior",
            inherited: false,
            apply: apply_scroll_behavior,
            copy: copy_scroll_behavior,
            longhands: &[],
        },
        OverflowAnchor => &PropertyDef {
            id: OverflowAnchor,
            name: "overflow-anchor",
            inherited: false,
            apply: apply_overflow_anchor,
            copy: copy_overflow_anchor,
            longhands: &[],
        },
        OverflowClipMargin => &PropertyDef {
            id: OverflowClipMargin,
            name: "overflow-clip-margin",
            inherited: false,
            apply: apply_overflow_clip_margin,
            copy: copy_overflow_clip_margin,
            longhands: &[],
        },
        OverscrollBehavior => &PropertyDef {
            id: OverscrollBehavior,
            name: "overscroll-behavior",
            inherited: false,
            apply: apply_overscroll_behavior,
            copy: copy_noop,
            longhands: &[OverscrollBehaviorX, OverscrollBehaviorY],
        },
        OverscrollBehaviorX => &PropertyDef {
            id: OverscrollBehaviorX,
            name: "overscroll-behavior-x",
            inherited: false,
            apply: apply_overscroll_behavior_x,
            copy: copy_overscroll_behavior_x,
            longhands: &[],
        },
        OverscrollBehaviorY => &PropertyDef {
            id: OverscrollBehaviorY,
            name: "overscroll-behavior-y",
            inherited: false,
            apply: apply_overscroll_behavior_y,
            copy: copy_overscroll_behavior_y,
            longhands: &[],
        },
        Isolation => &PropertyDef {
            id: Isolation,
            name: "isolation",
            inherited: false,
            apply: apply_isolation,
            copy: copy_isolation,
            longhands: &[],
        },
        MixBlendMode => &PropertyDef {
            id: MixBlendMode,
            name: "mix-blend-mode",
            inherited: false,
            apply: apply_mix_blend_mode,
            copy: copy_mix_blend_mode,
            longhands: &[],
        },
        InterpolateSize => &PropertyDef {
            id: InterpolateSize,
            name: "interpolate-size",
            inherited: true,
            apply: apply_interpolate_size,
            copy: copy_interpolate_size,
            longhands: &[],
        },
        MarginTrim => &PropertyDef {
            id: MarginTrim,
            name: "margin-trim",
            inherited: false,
            apply: apply_margin_trim,
            copy: copy_margin_trim,
            longhands: &[],
        },

        // ── Containment ──
        Contain => &PropertyDef {
            id: Contain,
            name: "contain",
            inherited: false,
            apply: apply_contain,
            copy: copy_contain,
            longhands: &[],
        },
        ContentVisibility => &PropertyDef {
            id: ContentVisibility,
            name: "content-visibility",
            inherited: false,
            apply: apply_content_visibility,
            copy: copy_content_visibility,
            longhands: &[],
        },
        ContainIntrinsicSize => &PropertyDef {
            id: ContainIntrinsicSize,
            name: "contain-intrinsic-size",
            inherited: false,
            apply: apply_contain_intrinsic_size,
            copy: copy_contain_intrinsic_size,
            longhands: &[],
        },

        // ── Scroll snap ──
        ScrollSnapType => &PropertyDef {
            id: ScrollSnapType,
            name: "scroll-snap-type",
            inherited: false,
            apply: apply_scroll_snap_type,
            copy: copy_scroll_snap_type,
            longhands: &[],
        },
        ScrollSnapAlign => &PropertyDef {
            id: ScrollSnapAlign,
            name: "scroll-snap-align",
            inherited: false,
            apply: apply_scroll_snap_align,
            copy: copy_scroll_snap_align,
            longhands: &[],
        },
        ScrollPadding => &PropertyDef {
            id: ScrollPadding,
            name: "scroll-padding",
            inherited: false,
            apply: apply_scroll_padding,
            copy: copy_noop,
            longhands: &[
                ScrollPaddingTop,
                ScrollPaddingRight,
                ScrollPaddingBottom,
                ScrollPaddingLeft,
            ],
        },
        ScrollPaddingTop => &PropertyDef {
            id: ScrollPaddingTop,
            name: "scroll-padding-top",
            inherited: false,
            apply: apply_scroll_padding_top,
            copy: copy_scroll_padding_top,
            longhands: &[],
        },
        ScrollPaddingRight => &PropertyDef {
            id: ScrollPaddingRight,
            name: "scroll-padding-right",
            inherited: false,
            apply: apply_scroll_padding_right,
            copy: copy_scroll_padding_right,
            longhands: &[],
        },
        ScrollPaddingBottom => &PropertyDef {
            id: ScrollPaddingBottom,
            name: "scroll-padding-bottom",
            inherited: false,
            apply: apply_scroll_padding_bottom,
            copy: copy_scroll_padding_bottom,
            longhands: &[],
        },
        ScrollPaddingLeft => &PropertyDef {
            id: ScrollPaddingLeft,
            name: "scroll-padding-left",
            inherited: false,
            apply: apply_scroll_padding_left,
            copy: copy_scroll_padding_left,
            longhands: &[],
        },
        ScrollMargin => &PropertyDef {
            id: ScrollMargin,
            name: "scroll-margin",
            inherited: false,
            apply: apply_scroll_margin,
            copy: copy_noop,
            longhands: &[
                ScrollMarginTop,
                ScrollMarginRight,
                ScrollMarginBottom,
                ScrollMarginLeft,
            ],
        },
        ScrollMarginTop => &PropertyDef {
            id: ScrollMarginTop,
            name: "scroll-margin-top",
            inherited: false,
            apply: apply_scroll_margin_top,
            copy: copy_scroll_margin_top,
            longhands: &[],
        },
        ScrollMarginRight => &PropertyDef {
            id: ScrollMarginRight,
            name: "scroll-margin-right",
            inherited: false,
            apply: apply_scroll_margin_right,
            copy: copy_scroll_margin_right,
            longhands: &[],
        },
        ScrollMarginBottom => &PropertyDef {
            id: ScrollMarginBottom,
            name: "scroll-margin-bottom",
            inherited: false,
            apply: apply_scroll_margin_bottom,
            copy: copy_scroll_margin_bottom,
            longhands: &[],
        },
        ScrollMarginLeft => &PropertyDef {
            id: ScrollMarginLeft,
            name: "scroll-margin-left",
            inherited: false,
            apply: apply_scroll_margin_left,
            copy: copy_scroll_margin_left,
            longhands: &[],
        },
        ScrollSnapStop => &PropertyDef {
            id: ScrollSnapStop,
            name: "scroll-snap-stop",
            inherited: false,
            apply: apply_scroll_snap_stop,
            copy: copy_scroll_snap_stop,
            longhands: &[],
        },

        // ── Logical properties ──
        MarginBlock => &PropertyDef {
            id: MarginBlock,
            name: "margin-block",
            inherited: false,
            apply: apply_margin_block,
            copy: copy_noop,
            longhands: &[MarginTop, MarginBottom],
        },
        MarginBlockStart => &PropertyDef {
            id: MarginBlockStart,
            name: "margin-block-start",
            inherited: false,
            apply: apply_margin_block_start,
            copy: copy_margin_top,
            longhands: &[],
        },
        MarginBlockEnd => &PropertyDef {
            id: MarginBlockEnd,
            name: "margin-block-end",
            inherited: false,
            apply: apply_margin_block_end,
            copy: copy_margin_bottom,
            longhands: &[],
        },
        MarginInline => &PropertyDef {
            id: MarginInline,
            name: "margin-inline",
            inherited: false,
            apply: apply_margin_inline,
            copy: copy_noop,
            longhands: &[MarginLeft, MarginRight],
        },
        MarginInlineStart => &PropertyDef {
            id: MarginInlineStart,
            name: "margin-inline-start",
            inherited: false,
            apply: apply_margin_inline_start,
            copy: copy_margin_left,
            longhands: &[],
        },
        MarginInlineEnd => &PropertyDef {
            id: MarginInlineEnd,
            name: "margin-inline-end",
            inherited: false,
            apply: apply_margin_inline_end,
            copy: copy_margin_right,
            longhands: &[],
        },
        PaddingBlock => &PropertyDef {
            id: PaddingBlock,
            name: "padding-block",
            inherited: false,
            apply: apply_padding_block,
            copy: copy_noop,
            longhands: &[PaddingTop, PaddingBottom],
        },
        PaddingBlockStart => &PropertyDef {
            id: PaddingBlockStart,
            name: "padding-block-start",
            inherited: false,
            apply: apply_padding_block_start,
            copy: copy_padding_top,
            longhands: &[],
        },
        PaddingBlockEnd => &PropertyDef {
            id: PaddingBlockEnd,
            name: "padding-block-end",
            inherited: false,
            apply: apply_padding_block_end,
            copy: copy_padding_bottom,
            longhands: &[],
        },
        PaddingInline => &PropertyDef {
            id: PaddingInline,
            name: "padding-inline",
            inherited: false,
            apply: apply_padding_inline,
            copy: copy_noop,
            longhands: &[PaddingLeft, PaddingRight],
        },
        PaddingInlineStart => &PropertyDef {
            id: PaddingInlineStart,
            name: "padding-inline-start",
            inherited: false,
            apply: apply_padding_inline_start,
            copy: copy_padding_left,
            longhands: &[],
        },
        PaddingInlineEnd => &PropertyDef {
            id: PaddingInlineEnd,
            name: "padding-inline-end",
            inherited: false,
            apply: apply_padding_inline_end,
            copy: copy_padding_right,
            longhands: &[],
        },
        BorderBlock => &PropertyDef {
            id: BorderBlock,
            name: "border-block",
            inherited: false,
            apply: apply_border_block,
            copy: copy_noop,
            longhands: &[
                BorderTopWidth,
                BorderBottomWidth,
                BorderTopStyle,
                BorderBottomStyle,
                BorderTopColor,
                BorderBottomColor,
            ],
        },
        BorderBlockStart => &PropertyDef {
            id: BorderBlockStart,
            name: "border-block-start",
            inherited: false,
            apply: apply_border_block_start,
            copy: copy_noop,
            longhands: &[BorderTopWidth, BorderTopStyle, BorderTopColor],
        },
        BorderBlockEnd => &PropertyDef {
            id: BorderBlockEnd,
            name: "border-block-end",
            inherited: false,
            apply: apply_border_block_end,
            copy: copy_noop,
            longhands: &[BorderBottomWidth, BorderBottomStyle, BorderBottomColor],
        },
        BorderInline => &PropertyDef {
            id: BorderInline,
            name: "border-inline",
            inherited: false,
            apply: apply_border_inline,
            copy: copy_noop,
            longhands: &[
                BorderLeftWidth,
                BorderRightWidth,
                BorderLeftStyle,
                BorderRightStyle,
                BorderLeftColor,
                BorderRightColor,
            ],
        },
        BorderInlineStart => &PropertyDef {
            id: BorderInlineStart,
            name: "border-inline-start",
            inherited: false,
            apply: apply_border_inline_start,
            copy: copy_noop,
            longhands: &[BorderLeftWidth, BorderLeftStyle, BorderLeftColor],
        },
        BorderInlineEnd => &PropertyDef {
            id: BorderInlineEnd,
            name: "border-inline-end",
            inherited: false,
            apply: apply_border_inline_end,
            copy: copy_noop,
            longhands: &[BorderRightWidth, BorderRightStyle, BorderRightColor],
        },
        BorderBlockStartWidth => &PropertyDef {
            id: BorderBlockStartWidth,
            name: "border-block-start-width",
            inherited: false,
            apply: apply_border_block_start_width,
            copy: copy_border_top_width,
            longhands: &[],
        },
        BorderBlockEndWidth => &PropertyDef {
            id: BorderBlockEndWidth,
            name: "border-block-end-width",
            inherited: false,
            apply: apply_border_block_end_width,
            copy: copy_border_bottom_width,
            longhands: &[],
        },
        BorderInlineStartWidth => &PropertyDef {
            id: BorderInlineStartWidth,
            name: "border-inline-start-width",
            inherited: false,
            apply: apply_border_inline_start_width,
            copy: copy_border_left_width,
            longhands: &[],
        },
        BorderInlineEndWidth => &PropertyDef {
            id: BorderInlineEndWidth,
            name: "border-inline-end-width",
            inherited: false,
            apply: apply_border_inline_end_width,
            copy: copy_border_right_width,
            longhands: &[],
        },
        BorderBlockStartStyle => &PropertyDef {
            id: BorderBlockStartStyle,
            name: "border-block-start-style",
            inherited: false,
            apply: apply_border_block_start_style,
            copy: copy_border_top_style,
            longhands: &[],
        },
        BorderBlockEndStyle => &PropertyDef {
            id: BorderBlockEndStyle,
            name: "border-block-end-style",
            inherited: false,
            apply: apply_border_block_end_style,
            copy: copy_border_bottom_style,
            longhands: &[],
        },
        BorderInlineStartStyle => &PropertyDef {
            id: BorderInlineStartStyle,
            name: "border-inline-start-style",
            inherited: false,
            apply: apply_border_inline_start_style,
            copy: copy_border_left_style,
            longhands: &[],
        },
        BorderInlineEndStyle => &PropertyDef {
            id: BorderInlineEndStyle,
            name: "border-inline-end-style",
            inherited: false,
            apply: apply_border_inline_end_style,
            copy: copy_border_right_style,
            longhands: &[],
        },
        BorderBlockStartColor => &PropertyDef {
            id: BorderBlockStartColor,
            name: "border-block-start-color",
            inherited: false,
            apply: apply_border_block_start_color,
            copy: copy_border_top_color,
            longhands: &[],
        },
        BorderBlockEndColor => &PropertyDef {
            id: BorderBlockEndColor,
            name: "border-block-end-color",
            inherited: false,
            apply: apply_border_block_end_color,
            copy: copy_border_bottom_color,
            longhands: &[],
        },
        BorderInlineStartColor => &PropertyDef {
            id: BorderInlineStartColor,
            name: "border-inline-start-color",
            inherited: false,
            apply: apply_border_inline_start_color,
            copy: copy_border_left_color,
            longhands: &[],
        },
        BorderInlineEndColor => &PropertyDef {
            id: BorderInlineEndColor,
            name: "border-inline-end-color",
            inherited: false,
            apply: apply_border_inline_end_color,
            copy: copy_border_right_color,
            longhands: &[],
        },
        InsetBlockStart => &PropertyDef {
            id: InsetBlockStart,
            name: "inset-block-start",
            inherited: false,
            apply: apply_inset_block_start,
            copy: copy_top,
            longhands: &[],
        },
        InsetBlockEnd => &PropertyDef {
            id: InsetBlockEnd,
            name: "inset-block-end",
            inherited: false,
            apply: apply_inset_block_end,
            copy: copy_bottom,
            longhands: &[],
        },
        InsetInlineStart => &PropertyDef {
            id: InsetInlineStart,
            name: "inset-inline-start",
            inherited: false,
            apply: apply_inset_inline_start,
            copy: copy_left,
            longhands: &[],
        },
        InsetInlineEnd => &PropertyDef {
            id: InsetInlineEnd,
            name: "inset-inline-end",
            inherited: false,
            apply: apply_inset_inline_end,
            copy: copy_right,
            longhands: &[],
        },
        Inset => &PropertyDef {
            id: Inset,
            name: "inset",
            inherited: false,
            apply: apply_inset,
            copy: copy_noop,
            longhands: &[Top, Right, Bottom, Left],
        },
        InsetBlock => &PropertyDef {
            id: InsetBlock,
            name: "inset-block",
            inherited: false,
            apply: apply_inset_block,
            copy: copy_noop,
            longhands: &[Top, Bottom],
        },
        InsetInline => &PropertyDef {
            id: InsetInline,
            name: "inset-inline",
            inherited: false,
            apply: apply_inset_inline,
            copy: copy_noop,
            longhands: &[Left, Right],
        },

        // ── Place shorthands ──
        PlaceSelf => &PropertyDef {
            id: PlaceSelf,
            name: "place-self",
            inherited: false,
            apply: apply_place_self,
            copy: copy_noop,
            longhands: &[AlignSelf, JustifySelf],
        },
        PlaceItems => &PropertyDef {
            id: PlaceItems,
            name: "place-items",
            inherited: false,
            apply: apply_place_items,
            copy: copy_noop,
            longhands: &[AlignItems, JustifyItems],
        },
        PlaceContent => &PropertyDef {
            id: PlaceContent,
            name: "place-content",
            inherited: false,
            apply: apply_place_content,
            copy: copy_noop,
            longhands: &[AlignContent, JustifyContent],
        },

        // ── Appearance / color-scheme / accent-color ──
        Appearance => &PropertyDef {
            id: Appearance,
            name: "appearance",
            inherited: false,
            apply: apply_appearance,
            copy: copy_appearance,
            longhands: &[],
        },
        FieldSizing => &PropertyDef {
            id: FieldSizing,
            name: "field-sizing",
            inherited: false,
            apply: apply_field_sizing,
            copy: copy_field_sizing,
            longhands: &[],
        },
        ColorScheme => &PropertyDef {
            id: ColorScheme,
            name: "color-scheme",
            inherited: true,
            apply: apply_color_scheme,
            copy: copy_color_scheme,
            longhands: &[],
        },
        ForcedColorAdjust => &PropertyDef {
            id: ForcedColorAdjust,
            name: "forced-color-adjust",
            inherited: true,
            apply: apply_forced_color_adjust,
            copy: copy_forced_color_adjust,
            longhands: &[],
        },
        ColorInterpolation => &PropertyDef {
            id: ColorInterpolation,
            name: "color-interpolation",
            inherited: true,
            apply: apply_noop,
            copy: copy_noop,
            longhands: &[],
        },
        AccentColor => &PropertyDef {
            id: AccentColor,
            name: "accent-color",
            inherited: true,
            apply: apply_accent_color,
            copy: copy_noop,
            longhands: &[],
        },

        // ── Image rendering ──
        ImageRendering => &PropertyDef {
            id: ImageRendering,
            name: "image-rendering",
            inherited: true,
            apply: apply_noop,
            copy: copy_noop,
            longhands: &[],
        },
        ImageOrientation => &PropertyDef {
            id: ImageOrientation,
            name: "image-orientation",
            inherited: true,
            apply: apply_noop,
            copy: copy_noop,
            longhands: &[],
        },

        // ── Touch / interaction ──
        TouchAction => &PropertyDef {
            id: TouchAction,
            name: "touch-action",
            inherited: false,
            apply: apply_noop,
            copy: copy_noop,
            longhands: &[],
        },

        // ── Logical sizing ──
        InlineSize => &PropertyDef {
            id: InlineSize,
            name: "inline-size",
            inherited: false,
            apply: apply_inline_size,
            copy: copy_width,
            longhands: &[],
        },
        BlockSize => &PropertyDef {
            id: BlockSize,
            name: "block-size",
            inherited: false,
            apply: apply_block_size,
            copy: copy_height,
            longhands: &[],
        },
        MinInlineSize => &PropertyDef {
            id: MinInlineSize,
            name: "min-inline-size",
            inherited: false,
            apply: apply_min_inline_size,
            copy: copy_min_width,
            longhands: &[],
        },
        MinBlockSize => &PropertyDef {
            id: MinBlockSize,
            name: "min-block-size",
            inherited: false,
            apply: apply_min_block_size,
            copy: copy_min_height,
            longhands: &[],
        },
        MaxInlineSize => &PropertyDef {
            id: MaxInlineSize,
            name: "max-inline-size",
            inherited: false,
            apply: apply_max_inline_size,
            copy: copy_max_width,
            longhands: &[],
        },
        MaxBlockSize => &PropertyDef {
            id: MaxBlockSize,
            name: "max-block-size",
            inherited: false,
            apply: apply_max_block_size,
            copy: copy_max_height,
            longhands: &[],
        },

        // ── Content (generated) ──
        Content => &PropertyDef {
            id: Content,
            name: "content",
            inherited: false,
            apply: apply_content,
            copy: copy_content,
            longhands: &[],
        },

        // ── Individual transform properties (CSS Transforms Level 2) ──
        Rotate => &PropertyDef {
            id: Rotate,
            name: "rotate",
            inherited: false,
            apply: apply_individual_rotate,
            copy: copy_individual_rotate,
            longhands: &[],
        },
        Scale => &PropertyDef {
            id: Scale,
            name: "scale",
            inherited: false,
            apply: apply_individual_scale,
            copy: copy_individual_scale,
            longhands: &[],
        },
        Translate => &PropertyDef {
            id: Translate,
            name: "translate",
            inherited: false,
            apply: apply_individual_translate,
            copy: copy_individual_translate,
            longhands: &[],
        },

        // ── Default ──
        _ => &UNKNOWN_DEF,
    }
}

/// Collect all inherited property IDs for use in inherit_from.
pub const INHERITED_IDS: &[PropertyId] = &[
    PropertyId::Color,
    PropertyId::FontSize,
    PropertyId::FontFamily,
    PropertyId::FontWeight,
    PropertyId::FontStyle,
    PropertyId::LineHeight,
    PropertyId::TextAlign,
    PropertyId::TextTransform,
    PropertyId::TextIndent,
    PropertyId::LetterSpacing,
    PropertyId::WordSpacing,
    PropertyId::WhiteSpace,
    PropertyId::Direction,
    PropertyId::Visibility,
    PropertyId::Cursor,
    PropertyId::ListStyleType,
    PropertyId::ListStylePosition,
    PropertyId::BorderCollapse,
    PropertyId::BorderSpacing,
    PropertyId::CaptionSide,
    PropertyId::EmptyCells,
    PropertyId::WordBreak,
    PropertyId::OverflowWrap,
    PropertyId::WritingMode,
    PropertyId::TextOrientation,
    PropertyId::TextCombineUpright,
    PropertyId::TabSize,
    PropertyId::Hyphens,
    PropertyId::Orphans,
    PropertyId::Widows,
    PropertyId::PointerEvents,
    PropertyId::CaretColor,
    PropertyId::Quotes,
    PropertyId::TextShadow,
    PropertyId::FontVariationSettings,
    PropertyId::FontFeatureSettings,
    PropertyId::FontVariant,
    PropertyId::FontVariantAlternates,
    PropertyId::FontVariantCaps,
    PropertyId::FontVariantEastAsian,
    PropertyId::FontVariantEmoji,
    PropertyId::FontVariantLigatures,
    PropertyId::FontVariantNumeric,
    PropertyId::FontVariantPosition,
    PropertyId::FontStretch,
    PropertyId::FontSynthesisWeight,
    PropertyId::FontSynthesisStyle,
    PropertyId::FontSynthesisSmallCaps,
    PropertyId::FontSynthesisPosition,
    PropertyId::TextUnderlineOffset,
    PropertyId::TextUnderlinePosition,
    PropertyId::TextDecorationSkipInk,
    PropertyId::TextEmphasisStyle,
    PropertyId::TextEmphasisColor,
    PropertyId::TextEmphasisPosition,
    PropertyId::TextWrap,
    PropertyId::InterpolateSize,
    PropertyId::ColorScheme,
    PropertyId::ForcedColorAdjust,
];

// ═══════════════════════════════════════════════════════════════════════════════
// Apply functions — one per property, extracted from the giant match.
// Each takes `(style, value_str)` and sets the appropriate field.
// ═══════════════════════════════════════════════════════════════════════════════

use super::{parse_color, parse_font_size_checked, parse_length, parse_length_checked};
use crate::types::*;

fn apply_keyword_list(field: &mut String, v: &str, allowed: &[&str]) {
    let value = v.trim();
    if !value.is_empty()
        && value
            .split_whitespace()
            .all(|tok| allowed.iter().any(|kw| *kw == tok))
    {
        *field = value.to_string();
    }
}

fn apply_comma_keyword_list(field: &mut String, v: &str, allowed: &[&str]) {
    let value = v.trim();
    if !value.is_empty()
        && value.split(',').all(|part| {
            let part = part.trim();
            !part.is_empty() && allowed.iter().any(|kw| *kw == part)
        })
    {
        *field = value.to_string();
    }
}

// ── Sizing ──────────────────────────────────────────────────────────────────

// The sizing properties DROP a value they cannot parse rather than falling
// back to `auto` — an invalid declaration leaves the cascade's winner in place
// (css-syntax-3 §9).
fn apply_width(s: &mut ComputedStyle, v: &str) {
    if let Some(l) = parse_length_checked(v) {
        s.width = l;
    }
}
fn apply_height(s: &mut ComputedStyle, v: &str) {
    if let Some(l) = parse_length_checked(v) {
        s.height = l;
    }
}
fn apply_min_width(s: &mut ComputedStyle, v: &str) {
    if let Some(l) = parse_length_checked(v) {
        s.min_width = l;
    }
}
fn apply_min_height(s: &mut ComputedStyle, v: &str) {
    if let Some(l) = parse_length_checked(v) {
        s.min_height = l;
    }
}
fn apply_max_width(s: &mut ComputedStyle, v: &str) {
    if v.trim() == "none" {
        s.max_width = CssLength::None;
    } else if let Some(l) = parse_length_checked(v) {
        s.max_width = l;
    }
}
fn apply_max_height(s: &mut ComputedStyle, v: &str) {
    if v.trim() == "none" {
        s.max_height = CssLength::None;
    } else if let Some(l) = parse_length_checked(v) {
        s.max_height = l;
    }
}

fn copy_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.width = s.width.clone();
}
fn copy_height(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.height = s.height.clone();
}
fn copy_min_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.min_width = s.min_width.clone();
}
fn copy_min_height(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.min_height = s.min_height.clone();
}
fn copy_max_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.max_width = s.max_width.clone();
}
fn copy_max_height(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.max_height = s.max_height.clone();
}

// ── Margin ──────────────────────────────────────────────────────────────────

fn apply_margin_top(s: &mut ComputedStyle, v: &str) {
    s.margin_top = parse_length(v);
}
fn apply_margin_right(s: &mut ComputedStyle, v: &str) {
    s.margin_right = parse_length(v);
}
fn apply_margin_bottom(s: &mut ComputedStyle, v: &str) {
    s.margin_bottom = parse_length(v);
}
fn apply_margin_left(s: &mut ComputedStyle, v: &str) {
    s.margin_left = parse_length(v);
}

fn apply_margin(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.margin_top,
        &mut s.margin_right,
        &mut s.margin_bottom,
        &mut s.margin_left,
        parse_length,
    );
}

fn copy_margin_top(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.margin_top = s.margin_top.clone();
}
fn copy_margin_right(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.margin_right = s.margin_right.clone();
}
fn copy_margin_bottom(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.margin_bottom = s.margin_bottom.clone();
}
fn copy_margin_left(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.margin_left = s.margin_left.clone();
}

// ── Padding ─────────────────────────────────────────────────────────────────

fn apply_padding_top(s: &mut ComputedStyle, v: &str) {
    s.padding_top = parse_length(v);
}
fn apply_padding_right(s: &mut ComputedStyle, v: &str) {
    s.padding_right = parse_length(v);
}
fn apply_padding_bottom(s: &mut ComputedStyle, v: &str) {
    s.padding_bottom = parse_length(v);
}
fn apply_padding_left(s: &mut ComputedStyle, v: &str) {
    s.padding_left = parse_length(v);
}

fn apply_padding(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.padding_top,
        &mut s.padding_right,
        &mut s.padding_bottom,
        &mut s.padding_left,
        parse_length,
    );
}

fn copy_padding_top(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.padding_top = s.padding_top.clone();
}
fn copy_padding_right(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.padding_right = s.padding_right.clone();
}
fn copy_padding_bottom(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.padding_bottom = s.padding_bottom.clone();
}
fn copy_padding_left(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.padding_left = s.padding_left.clone();
}

// ── Color / Background ──────────────────────────────────────────────────────

fn apply_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.color = c;
    }
}
fn apply_background_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.background_color = c;
    }
}
fn apply_svg_fill(s: &mut ComputedStyle, v: &str) {
    if v.trim().eq_ignore_ascii_case("none") {
        s.svg_fill = None;
    } else if let Some(c) = parse_color(v) {
        s.svg_fill = Some(c);
    }
}
fn apply_svg_stroke(s: &mut ComputedStyle, v: &str) {
    if v.trim().eq_ignore_ascii_case("none") {
        s.svg_stroke = None;
    } else if let Some(c) = parse_color(v) {
        s.svg_stroke = Some(c);
    }
}
fn apply_opacity(s: &mut ComputedStyle, v: &str) {
    // css-color-4 §14: an `<alpha-value>` is a `<number>` OR a `<percentage>`
    // — the two are equivalent, `50%` is `0.5` — and either may arrive inside
    // a `calc()`. A bare `parse::<f32>()` rejects both of the latter forms and
    // would silently leave the element fully opaque.
    let v = v.trim();
    let op = if let Some(p) = v.strip_suffix('%') {
        p.trim().parse::<f32>().ok().map(|n| n / 100.0)
    } else if let Ok(n) = v.parse::<f32>() {
        Some(n)
    } else {
        match crate::css::parse_length(v) {
            CssLength::Percent(p) => Some(p / 100.0),
            CssLength::Px(n) => Some(n),
            _ => None,
        }
    };
    s.opacity = op.unwrap_or(1.0).clamp(0.0, 1.0);
}

fn copy_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.color = s.color;
}
fn copy_background_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_color = s.background_color;
}
fn copy_svg_fill(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.svg_fill = s.svg_fill;
}
fn copy_svg_stroke(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.svg_stroke = s.svg_stroke;
}
fn copy_opacity(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.opacity = s.opacity;
}

// ── Font ────────────────────────────────────────────────────────────────────

fn apply_font_size(s: &mut ComputedStyle, v: &str) {
    if let Some(size) = parse_font_size_checked(v) {
        s.font_size = size;
    }
}
fn apply_font_family(s: &mut ComputedStyle, v: &str) {
    s.font_family = super::split_font_families(v).join(", ");
}
fn apply_font_weight(s: &mut ComputedStyle, v: &str) {
    let lower = v.trim().to_ascii_lowercase();
    s.font_weight = match lower.as_str() {
        "normal" => FontWeight::Normal,
        "bold" => FontWeight::Bold,
        "bolder" => relative_bolder(s.relative_font_weight_base.unwrap_or(s.font_weight)),
        "lighter" => relative_lighter(s.relative_font_weight_base.unwrap_or(s.font_weight)),
        _ => parse_absolute_font_weight(&lower).unwrap_or(s.font_weight),
    };
}

fn parse_absolute_font_weight(v: &str) -> Option<FontWeight> {
    let n = v.parse::<u16>().ok()?;
    if (1..=1000).contains(&n) {
        Some(FontWeight::Value(n))
    } else {
        None
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
fn apply_font_style(s: &mut ComputedStyle, v: &str) {
    let first = v.split_whitespace().next().unwrap_or(v);
    s.font_style = match first {
        "italic" => FontStyle::Italic,
        "oblique" => FontStyle::Oblique,
        _ => FontStyle::Normal,
    };
}
fn apply_font(s: &mut ComputedStyle, v: &str) {
    super::apply_font_shorthand(s, v);
}
fn apply_font_variation_settings(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().font_variation_settings = super::parse_variation_settings(v);
    // ⛔ Cloned: `&s.rare()` borrows the whole style for the loop, and the
    // body writes to it. The list is empty for 99.3% of elements.
    for (tag, val) in s.rare().font_variation_settings.clone() {
        if tag == "wght" {
            s.font_weight = FontWeight::Value(val as u16);
        }
    }
}
fn apply_font_feature_settings(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().font_feature_settings = super::parse_feature_settings(v);
    for (tag, val) in s.rare().font_feature_settings.clone() {
        if tag == "smcp" {
            s.small_caps = val != 0;
        }
    }
}
fn apply_font_variant(s: &mut ComputedStyle, v: &str) {
    s.small_caps = v
        .split_whitespace()
        .any(|tok| tok == "small-caps" || tok == "all-small-caps");
    s.font_variant_caps = if s.small_caps {
        String::from("small-caps")
    } else {
        String::from("normal")
    };
}
fn apply_font_variant_caps(s: &mut ComputedStyle, v: &str) {
    let value = normalize_font_variant_longhand(v);
    s.small_caps = matches!(
        v.split_whitespace().next(),
        Some("small-caps" | "all-small-caps")
    );
    s.font_variant_caps = value;
}
fn apply_font_variant_alternates(s: &mut ComputedStyle, v: &str) {
    s.font_variant_alternates = normalize_font_variant_longhand(v);
}
fn apply_font_variant_east_asian(s: &mut ComputedStyle, v: &str) {
    s.font_variant_east_asian = normalize_font_variant_longhand(v);
}
fn apply_font_variant_emoji(s: &mut ComputedStyle, v: &str) {
    s.font_variant_emoji = normalize_font_variant_longhand(v);
}
fn apply_font_variant_ligatures(s: &mut ComputedStyle, v: &str) {
    s.font_variant_ligatures = normalize_font_variant_longhand(v);
}
fn apply_font_variant_numeric(s: &mut ComputedStyle, v: &str) {
    s.font_variant_numeric = normalize_font_variant_longhand(v);
}
fn apply_font_variant_position(s: &mut ComputedStyle, v: &str) {
    s.font_variant_position = normalize_font_variant_longhand(v);
}
fn normalize_font_variant_longhand(v: &str) -> String {
    let value = v.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        String::from("normal")
    } else {
        value
    }
}
fn apply_font_stretch(s: &mut ComputedStyle, v: &str) {
    s.font_stretch = match v {
        "ultra-condensed" => 50.0,
        "extra-condensed" => 62.5,
        "condensed" => 75.0,
        "semi-condensed" => 87.5,
        "normal" => 100.0,
        "semi-expanded" => 112.5,
        "expanded" => 125.0,
        "extra-expanded" => 150.0,
        "ultra-expanded" => 200.0,
        s if s.ends_with('%') => s[..s.len() - 1].parse().unwrap_or(100.0),
        _ => 100.0,
    };
}
fn apply_font_synthesis(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none" {
        s.font_synthesis_weight = false;
        s.font_synthesis_style = false;
        s.font_synthesis_small_caps = false;
        s.font_synthesis_position = false;
        return;
    }
    if value == "auto" {
        s.font_synthesis_weight = true;
        s.font_synthesis_style = true;
        s.font_synthesis_small_caps = true;
        s.font_synthesis_position = true;
        return;
    }

    let mut seen = false;
    let mut weight = false;
    let mut style = false;
    let mut small_caps = false;
    let mut position = false;
    for tok in value.split_whitespace() {
        match tok {
            "weight" => {
                weight = true;
                seen = true;
            }
            "style" => {
                style = true;
                seen = true;
            }
            "small-caps" => {
                small_caps = true;
                seen = true;
            }
            "position" => {
                position = true;
                seen = true;
            }
            _ => return,
        }
    }
    if seen {
        s.font_synthesis_weight = weight;
        s.font_synthesis_style = style;
        s.font_synthesis_small_caps = small_caps;
        s.font_synthesis_position = position;
    }
}
fn apply_font_synthesis_weight(s: &mut ComputedStyle, v: &str) {
    if let Some(enabled) = parse_font_synthesis_longhand(v) {
        s.font_synthesis_weight = enabled;
    }
}
fn apply_font_synthesis_style(s: &mut ComputedStyle, v: &str) {
    if let Some(enabled) = parse_font_synthesis_longhand(v) {
        s.font_synthesis_style = enabled;
    }
}
fn apply_font_synthesis_small_caps(s: &mut ComputedStyle, v: &str) {
    if let Some(enabled) = parse_font_synthesis_longhand(v) {
        s.font_synthesis_small_caps = enabled;
    }
}
fn apply_font_synthesis_position(s: &mut ComputedStyle, v: &str) {
    if let Some(enabled) = parse_font_synthesis_longhand(v) {
        s.font_synthesis_position = enabled;
    }
}
fn parse_font_synthesis_longhand(v: &str) -> Option<bool> {
    match v.trim() {
        "auto" => Some(true),
        "none" => Some(false),
        _ => None,
    }
}
fn apply_line_height(s: &mut ComputedStyle, v: &str) {
    s.line_height = super::parse_line_height(v);
}

fn copy_font_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_size = s.font_size.clone();
}
fn copy_font_family(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_family = s.font_family.clone();
}
fn copy_font_weight(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_weight = s.font_weight;
}
fn copy_font_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_style = s.font_style;
}
fn copy_line_height(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.line_height = s.line_height.clone();
}
fn copy_font_variation_settings(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().font_variation_settings = s.rare().font_variation_settings.clone();
}
fn copy_font_feature_settings(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().font_feature_settings = s.rare().font_feature_settings.clone();
    d.small_caps = s.small_caps;
}
fn copy_font_variant(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.small_caps = s.small_caps;
    d.font_variant_caps = s.font_variant_caps.clone();
}
fn copy_font_variant_caps(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.small_caps = s.small_caps;
    d.font_variant_caps = s.font_variant_caps.clone();
}
fn copy_font_variant_alternates(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_alternates = s.font_variant_alternates.clone();
}
fn copy_font_variant_east_asian(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_east_asian = s.font_variant_east_asian.clone();
}
fn copy_font_variant_emoji(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_emoji = s.font_variant_emoji.clone();
}
fn copy_font_variant_ligatures(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_ligatures = s.font_variant_ligatures.clone();
}
fn copy_font_variant_numeric(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_numeric = s.font_variant_numeric.clone();
}
fn copy_font_variant_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_variant_position = s.font_variant_position.clone();
}
fn copy_font_stretch(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_stretch = s.font_stretch;
}
fn copy_font_synthesis_weight(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_synthesis_weight = s.font_synthesis_weight;
}
fn copy_font_synthesis_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_synthesis_style = s.font_synthesis_style;
}
fn copy_font_synthesis_small_caps(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_synthesis_small_caps = s.font_synthesis_small_caps;
}
fn copy_font_synthesis_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.font_synthesis_position = s.font_synthesis_position;
}

// ── Text ────────────────────────────────────────────────────────────────────

fn apply_text_align(s: &mut ComputedStyle, v: &str) {
    s.text_align = match v {
        "right" => TextAlign::Right,
        "center" => TextAlign::Center,
        "justify" => TextAlign::Justify,
        "end" => TextAlign::End,
        "start" => TextAlign::Start,
        _ => TextAlign::Left,
    };
}
fn apply_text_transform(s: &mut ComputedStyle, v: &str) {
    s.text_transform = match v {
        "uppercase" => TextTransform::Uppercase,
        "lowercase" => TextTransform::Lowercase,
        "capitalize" => TextTransform::Capitalize,
        "full-width" => TextTransform::FullWidth,
        "full-size-kana" => TextTransform::FullSizeKana,
        "math-auto" => TextTransform::MathAuto,
        _ => TextTransform::None,
    };
}
fn apply_text_indent(s: &mut ComputedStyle, v: &str) {
    s.text_indent = parse_length(v);
}
fn apply_letter_spacing(s: &mut ComputedStyle, v: &str) {
    s.letter_spacing = parse_length(v);
}
fn apply_word_spacing(s: &mut ComputedStyle, v: &str) {
    s.word_spacing = parse_length(v);
}
fn apply_white_space(s: &mut ComputedStyle, v: &str) {
    s.white_space = match v {
        "nowrap" => WhiteSpace::Nowrap,
        "pre" => WhiteSpace::Pre,
        "pre-wrap" => WhiteSpace::PreWrap,
        "pre-line" => WhiteSpace::PreLine,
        _ => WhiteSpace::Normal,
    };
}
fn apply_direction(s: &mut ComputedStyle, v: &str) {
    s.direction = match v {
        "rtl" => Direction::RTL,
        _ => Direction::LTR,
    };
}
fn apply_visibility(s: &mut ComputedStyle, v: &str) {
    s.visibility = v != "hidden" && v != "collapse";
}
fn apply_cursor(s: &mut ComputedStyle, v: &str) {
    let keyword = v.split(',').last().unwrap_or(v).trim();
    s.cursor = match keyword {
        "pointer" => CSSCursor::Pointer,
        "text" => CSSCursor::Text,
        "default" => CSSCursor::Default,
        "move" => CSSCursor::Move,
        "not-allowed" => CSSCursor::NotAllowed,
        "grab" => CSSCursor::Grab,
        "grabbing" => CSSCursor::Grabbing,
        "copy" => CSSCursor::Copy,
        "cell" => CSSCursor::Cell,
        "context-menu" => CSSCursor::ContextMenu,
        "all-scroll" => CSSCursor::AllScroll,
        "zoom-in" => CSSCursor::ZoomIn,
        "zoom-out" => CSSCursor::ZoomOut,
        "col-resize" => CSSCursor::ColResize,
        "row-resize" => CSSCursor::RowResize,
        "ew-resize" => CSSCursor::ColResize,
        "ns-resize" => CSSCursor::RowResize,
        "crosshair" => CSSCursor::Crosshair,
        "help" => CSSCursor::Help,
        "wait" => CSSCursor::Wait,
        "progress" => CSSCursor::Wait,
        "none" => CSSCursor::None,
        "n-resize" => CSSCursor::NResize,
        "e-resize" => CSSCursor::EResize,
        "s-resize" => CSSCursor::SResize,
        "w-resize" => CSSCursor::WResize,
        "ne-resize" => CSSCursor::NEResize,
        "nw-resize" => CSSCursor::NWResize,
        "se-resize" => CSSCursor::SEResize,
        "sw-resize" => CSSCursor::SWResize,
        "nesw-resize" => CSSCursor::NEResize,
        "nwse-resize" => CSSCursor::NWResize,
        _ => CSSCursor::Auto,
    };
}

fn copy_text_align(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_align = s.text_align;
}
fn copy_text_transform(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_transform = s.text_transform;
}
fn copy_text_indent(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_indent = s.text_indent.clone();
}
fn copy_letter_spacing(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.letter_spacing = s.letter_spacing.clone();
}
fn copy_word_spacing(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.word_spacing = s.word_spacing.clone();
}
fn copy_white_space(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.white_space = s.white_space;
}
fn copy_direction(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.direction = s.direction;
}
fn copy_visibility(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.visibility = s.visibility;
}
fn copy_cursor(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.cursor = s.cursor;
}

// ── Display & Layout ────────────────────────────────────────────────────────

fn apply_display(s: &mut ComputedStyle, v: &str) {
    s.display = match v {
        "none" => Display::None,
        "block" => Display::Block,
        "inline" => Display::Inline,
        "inline-block" => Display::InlineBlock,
        "flex" => Display::Flex,
        "inline-flex" => Display::InlineFlex,
        "grid" => Display::Grid,
        "inline-grid" => Display::InlineGrid,
        "table" => Display::Table,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "table-caption" => Display::TableCaption,
        "table-column" => Display::TableColumn,
        "table-column-group" => Display::TableColumnGroup,
        "table-header-group" => Display::TableHeaderGroup,
        "table-footer-group" => Display::TableFooterGroup,
        "table-row-group" => Display::TableRowGroup,
        "list-item" => Display::ListItem,
        "flow-root" => Display::FlowRoot,
        "contents" => Display::Contents,
        "ruby" => Display::Ruby,
        "ruby-text" => Display::RubyText,
        _ => Display::Inline,
    };
}
fn apply_position(s: &mut ComputedStyle, v: &str) {
    s.position = match v {
        "static" => Position::Static,
        "relative" => Position::Relative,
        "absolute" => Position::Absolute,
        "fixed" => Position::Fixed,
        "sticky" => Position::Sticky,
        _ => Position::Static,
    };
}
fn apply_z_index(s: &mut ComputedStyle, v: &str) {
    if v.eq_ignore_ascii_case("auto") {
        s.z_index = 0;
        s.z_index_is_auto = true;
    } else if let Ok(n) = v.parse() {
        s.z_index = n;
        s.z_index_is_auto = false;
    }
}
fn apply_float(s: &mut ComputedStyle, v: &str) {
    s.float = match v {
        "left" => Float::Left,
        "right" => Float::Right,
        "inline-start" => Float::InlineStart,
        "inline-end" => Float::InlineEnd,
        _ => Float::None,
    };
    // Blockification is settled once, after the whole declaration block, by
    // `finalize_display` — doing it here would depend on declaration order.
}
fn apply_clear(s: &mut ComputedStyle, v: &str) {
    s.clear = match v {
        "left" => Clear::Left,
        "right" => Clear::Right,
        "both" => Clear::Both,
        "inline-start" => Clear::InlineStart,
        "inline-end" => Clear::InlineEnd,
        _ => Clear::None,
    };
}
fn apply_box_sizing(s: &mut ComputedStyle, v: &str) {
    s.box_sizing = match v {
        "border-box" => BoxSizing::BorderBox,
        _ => BoxSizing::ContentBox,
    };
}
fn apply_overflow_x(s: &mut ComputedStyle, v: &str) {
    s.overflow_x = super::parse_overflow(v);
}
fn apply_overflow_y(s: &mut ComputedStyle, v: &str) {
    s.overflow_y = super::parse_overflow(v);
}
fn apply_overflow(s: &mut ComputedStyle, v: &str) {
    let ov = super::parse_overflow(v);
    s.overflow_x = ov;
    s.overflow_y = ov;
}

fn copy_display(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.display = s.display;
}
fn copy_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.position = s.position;
}
fn copy_z_index(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.z_index = s.z_index;
    d.z_index_is_auto = s.z_index_is_auto;
}
fn copy_float(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.float = s.float;
}
fn copy_clear(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.clear = s.clear;
}
fn copy_box_sizing(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.box_sizing = s.box_sizing;
}
fn copy_overflow_x(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overflow_x = s.overflow_x;
}
fn copy_overflow_y(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overflow_y = s.overflow_y;
}

// ── Position offsets ────────────────────────────────────────────────────────

fn apply_top(s: &mut ComputedStyle, v: &str) {
    s.top = parse_length(v);
}
fn apply_right(s: &mut ComputedStyle, v: &str) {
    s.right = parse_length(v);
}
fn apply_bottom(s: &mut ComputedStyle, v: &str) {
    s.bottom = parse_length(v);
}
fn apply_left(s: &mut ComputedStyle, v: &str) {
    s.left = parse_length(v);
}

fn copy_top(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.top = s.top.clone();
}
fn copy_right(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.right = s.right.clone();
}
fn copy_bottom(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.bottom = s.bottom.clone();
}
fn copy_left(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.left = s.left.clone();
}

// ── Border ──────────────────────────────────────────────────────────────────

fn apply_border(s: &mut ComputedStyle, v: &str) {
    reset_border_side(
        &mut s.border_top_width,
        &mut s.border_top_style,
        &mut s.border_top_color,
    );
    reset_border_side(
        &mut s.border_right_width,
        &mut s.border_right_style,
        &mut s.border_right_color,
    );
    reset_border_side(
        &mut s.border_bottom_width,
        &mut s.border_bottom_style,
        &mut s.border_bottom_color,
    );
    reset_border_side(
        &mut s.border_left_width,
        &mut s.border_left_style,
        &mut s.border_left_color,
    );
    super::apply_border_shorthand(s, v);
}
fn reset_border_side(width: &mut CssLength, style: &mut BorderStyle, color: &mut Color) {
    *width = CssLength::Px(3.0);
    *style = BorderStyle::None;
    *color = Color::BLACK;
}
fn apply_border_width(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.border_top_width,
        &mut s.border_right_width,
        &mut s.border_bottom_width,
        &mut s.border_left_width,
        parse_length,
    );
}
fn apply_border_style_sh(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.split_whitespace().collect();
    match parts.len() {
        1 => {
            let bs = super::parse_border_style(parts[0]);
            s.border_top_style = bs;
            s.border_right_style = bs;
            s.border_bottom_style = bs;
            s.border_left_style = bs;
        }
        2 => {
            let tb = super::parse_border_style(parts[0]);
            let rl = super::parse_border_style(parts[1]);
            s.border_top_style = tb;
            s.border_bottom_style = tb;
            s.border_right_style = rl;
            s.border_left_style = rl;
        }
        3 => {
            s.border_top_style = super::parse_border_style(parts[0]);
            let rl = super::parse_border_style(parts[1]);
            s.border_right_style = rl;
            s.border_left_style = rl;
            s.border_bottom_style = super::parse_border_style(parts[2]);
        }
        4 => {
            s.border_top_style = super::parse_border_style(parts[0]);
            s.border_right_style = super::parse_border_style(parts[1]);
            s.border_bottom_style = super::parse_border_style(parts[2]);
            s.border_left_style = super::parse_border_style(parts[3]);
        }
        _ => {}
    }
}
fn apply_border_color_sh(s: &mut ComputedStyle, v: &str) {
    // Split by whitespace, but be careful with color functions like rgb(...)
    // Use the same comma/space-aware splitting as other shorthands
    let parts: Vec<&str> = super::split_shorthand_values(v);
    match parts.len() {
        1 => {
            let bc = parse_color(parts[0]).unwrap_or(Color::BLACK);
            s.border_top_color = bc;
            s.border_right_color = bc;
            s.border_bottom_color = bc;
            s.border_left_color = bc;
        }
        2 => {
            let tb = parse_color(parts[0]).unwrap_or(Color::BLACK);
            let rl = parse_color(parts[1]).unwrap_or(Color::BLACK);
            s.border_top_color = tb;
            s.border_bottom_color = tb;
            s.border_right_color = rl;
            s.border_left_color = rl;
        }
        3 => {
            s.border_top_color = parse_color(parts[0]).unwrap_or(Color::BLACK);
            let rl = parse_color(parts[1]).unwrap_or(Color::BLACK);
            s.border_right_color = rl;
            s.border_left_color = rl;
            s.border_bottom_color = parse_color(parts[2]).unwrap_or(Color::BLACK);
        }
        4 => {
            s.border_top_color = parse_color(parts[0]).unwrap_or(Color::BLACK);
            s.border_right_color = parse_color(parts[1]).unwrap_or(Color::BLACK);
            s.border_bottom_color = parse_color(parts[2]).unwrap_or(Color::BLACK);
            s.border_left_color = parse_color(parts[3]).unwrap_or(Color::BLACK);
        }
        _ => {}
    }
}
fn apply_border_top_width(s: &mut ComputedStyle, v: &str) {
    s.border_top_width = parse_length(v);
}
fn apply_border_right_width(s: &mut ComputedStyle, v: &str) {
    s.border_right_width = parse_length(v);
}
fn apply_border_bottom_width(s: &mut ComputedStyle, v: &str) {
    s.border_bottom_width = parse_length(v);
}
fn apply_border_left_width(s: &mut ComputedStyle, v: &str) {
    s.border_left_width = parse_length(v);
}
fn apply_border_top_style(s: &mut ComputedStyle, v: &str) {
    s.border_top_style = super::parse_border_style(v);
}
fn apply_border_right_style(s: &mut ComputedStyle, v: &str) {
    s.border_right_style = super::parse_border_style(v);
}
fn apply_border_bottom_style(s: &mut ComputedStyle, v: &str) {
    s.border_bottom_style = super::parse_border_style(v);
}
fn apply_border_left_style(s: &mut ComputedStyle, v: &str) {
    s.border_left_style = super::parse_border_style(v);
}
fn apply_border_top_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.border_top_color = c;
    }
}
fn apply_border_right_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.border_right_color = c;
    }
}
fn apply_border_bottom_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.border_bottom_color = c;
    }
}
fn apply_border_left_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.border_left_color = c;
    }
}

fn apply_border_top_sh(s: &mut ComputedStyle, v: &str) {
    reset_border_side(
        &mut s.border_top_width,
        &mut s.border_top_style,
        &mut s.border_top_color,
    );
    super::apply_border_side_shorthand(
        v,
        &mut s.border_top_width,
        &mut s.border_top_style,
        &mut s.border_top_color,
    );
}
fn apply_border_right_sh(s: &mut ComputedStyle, v: &str) {
    reset_border_side(
        &mut s.border_right_width,
        &mut s.border_right_style,
        &mut s.border_right_color,
    );
    super::apply_border_side_shorthand(
        v,
        &mut s.border_right_width,
        &mut s.border_right_style,
        &mut s.border_right_color,
    );
}
fn apply_border_bottom_sh(s: &mut ComputedStyle, v: &str) {
    reset_border_side(
        &mut s.border_bottom_width,
        &mut s.border_bottom_style,
        &mut s.border_bottom_color,
    );
    super::apply_border_side_shorthand(
        v,
        &mut s.border_bottom_width,
        &mut s.border_bottom_style,
        &mut s.border_bottom_color,
    );
}
fn apply_border_left_sh(s: &mut ComputedStyle, v: &str) {
    reset_border_side(
        &mut s.border_left_width,
        &mut s.border_left_style,
        &mut s.border_left_color,
    );
    super::apply_border_side_shorthand(
        v,
        &mut s.border_left_width,
        &mut s.border_left_style,
        &mut s.border_left_color,
    );
}

fn copy_border_top_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_top_width = s.border_top_width.clone();
}
fn copy_border_right_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_right_width = s.border_right_width.clone();
}
fn copy_border_bottom_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_bottom_width = s.border_bottom_width.clone();
}
fn copy_border_left_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_left_width = s.border_left_width.clone();
}
fn copy_border_top_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_top_style = s.border_top_style;
}
fn copy_border_right_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_right_style = s.border_right_style;
}
fn copy_border_bottom_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_bottom_style = s.border_bottom_style;
}
fn copy_border_left_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_left_style = s.border_left_style;
}
fn copy_border_top_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_top_color = s.border_top_color;
}
fn copy_border_right_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_right_color = s.border_right_color;
}
fn copy_border_bottom_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_bottom_color = s.border_bottom_color;
}
fn copy_border_left_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_left_color = s.border_left_color;
}

// ── Border radius ───────────────────────────────────────────────────────────

fn apply_border_radius(s: &mut ComputedStyle, v: &str) {
    let (horizontal, vertical) = v.split_once('/').unwrap_or((v, v));
    let [tl, tr, br, bl] = parse_radius_set(horizontal);
    let [tl_y, tr_y, br_y, bl_y] = parse_radius_set(vertical);
    s.border_radius = tl.clone();
    s.border_top_left_radius = tl;
    s.border_top_right_radius = tr;
    s.border_bottom_right_radius = br;
    s.border_bottom_left_radius = bl;
    s.border_top_left_radius_y = tl_y;
    s.border_top_right_radius_y = tr_y;
    s.border_bottom_right_radius_y = br_y;
    s.border_bottom_left_radius_y = bl_y;
}
fn apply_border_top_left_radius(s: &mut ComputedStyle, v: &str) {
    let (x, y) = parse_radius_pair(v);
    s.border_top_left_radius = x;
    s.border_top_left_radius_y = y;
    s.border_radius = s.border_top_left_radius.clone();
}
fn apply_border_top_right_radius(s: &mut ComputedStyle, v: &str) {
    let (x, y) = parse_radius_pair(v);
    s.border_top_right_radius = x;
    s.border_top_right_radius_y = y;
}
fn apply_border_bottom_left_radius(s: &mut ComputedStyle, v: &str) {
    let (x, y) = parse_radius_pair(v);
    s.border_bottom_left_radius = x;
    s.border_bottom_left_radius_y = y;
}
fn apply_border_bottom_right_radius(s: &mut ComputedStyle, v: &str) {
    let (x, y) = parse_radius_pair(v);
    s.border_bottom_right_radius = x;
    s.border_bottom_right_radius_y = y;
}

fn copy_border_top_left_radius(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_top_left_radius = s.border_top_left_radius.clone();
    d.border_top_left_radius_y = s.border_top_left_radius_y.clone();
    d.border_radius = s.border_radius.clone();
}
fn copy_border_top_right_radius(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_top_right_radius = s.border_top_right_radius.clone();
    d.border_top_right_radius_y = s.border_top_right_radius_y.clone();
}
fn copy_border_bottom_left_radius(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_bottom_left_radius = s.border_bottom_left_radius.clone();
    d.border_bottom_left_radius_y = s.border_bottom_left_radius_y.clone();
}
fn copy_border_bottom_right_radius(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_bottom_right_radius = s.border_bottom_right_radius.clone();
    d.border_bottom_right_radius_y = s.border_bottom_right_radius_y.clone();
}

fn parse_radius_set(input: &str) -> [CssLength; 4] {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let first = parts.first().copied().unwrap_or("0");
    [
        parse_length(first),
        parse_length(parts.get(1).copied().unwrap_or(first)),
        parse_length(parts.get(2).copied().unwrap_or(first)),
        parse_length(
            parts
                .get(3)
                .copied()
                .unwrap_or(parts.get(1).copied().unwrap_or(first)),
        ),
    ]
}

fn parse_radius_pair(input: &str) -> (CssLength, CssLength) {
    let mut parts = input.split_whitespace();
    let x = parse_length(parts.next().unwrap_or("0"));
    let y = parts.next().map(parse_length).unwrap_or_else(|| x.clone());
    (x, y)
}

// ── Border image ────────────────────────────────────────────────────────────

fn is_border_image_repeat_keyword(v: &str) -> bool {
    matches!(v, "stretch" | "repeat" | "round" | "space")
}

fn apply_border_image_source(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none"
        || value.starts_with("url(")
        || value.starts_with("image-set(")
        || value.starts_with("-webkit-image-set(")
        || value.ends_with("-gradient)")
    {
        s.border_image_source = value.to_string();
    }
}

fn apply_border_image_slice(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if !value.is_empty() {
        s.border_image_slice = value.to_string();
    }
}

fn apply_border_image_width(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if !value.is_empty() {
        s.border_image_width = value.to_string();
    }
}

fn apply_border_image_outset(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if !value.is_empty() {
        s.border_image_outset = value.to_string();
    }
}

fn apply_border_image_repeat(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.border_image_repeat,
        v,
        &["stretch", "repeat", "round", "space"],
    );
}

fn apply_border_image(s: &mut ComputedStyle, v: &str) {
    s.border_image_source = String::from("none");
    s.border_image_slice = String::from("100%");
    s.border_image_width = String::from("1");
    s.border_image_outset = String::from("0");
    s.border_image_repeat = String::from("stretch");

    let mut slash_parts = v.split('/').map(str::trim);
    let before_slash = slash_parts.next().unwrap_or("");
    let width = slash_parts.next();
    let outset = slash_parts.next();
    let mut repeat_tokens = Vec::new();

    if let Some(width) = width.filter(|part| !part.is_empty()) {
        let width_tokens: Vec<&str> = width
            .split_whitespace()
            .filter(|token| {
                if is_border_image_repeat_keyword(token) {
                    repeat_tokens.push(*token);
                    false
                } else {
                    true
                }
            })
            .collect();
        if !width_tokens.is_empty() {
            apply_border_image_width(s, &width_tokens.join(" "));
        }
    }
    if let Some(outset) = outset.filter(|part| !part.is_empty()) {
        let outset_tokens: Vec<&str> = outset
            .split_whitespace()
            .filter(|token| {
                if is_border_image_repeat_keyword(token) {
                    repeat_tokens.push(*token);
                    false
                } else {
                    true
                }
            })
            .collect();
        if !outset_tokens.is_empty() {
            apply_border_image_outset(s, &outset_tokens.join(" "));
        }
    }

    let mut slice_tokens = Vec::new();
    for token in before_slash.split_whitespace() {
        if token == "none" || token.contains('(') {
            apply_border_image_source(s, token);
        } else if is_border_image_repeat_keyword(token) {
            repeat_tokens.push(token);
        } else {
            slice_tokens.push(token);
        }
    }
    if !slice_tokens.is_empty() {
        apply_border_image_slice(s, &slice_tokens.join(" "));
    }
    if !repeat_tokens.is_empty() {
        apply_border_image_repeat(s, &repeat_tokens.join(" "));
    }
}

fn copy_border_image_source(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_image_source = s.border_image_source.clone();
}
fn copy_border_image_slice(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_image_slice = s.border_image_slice.clone();
}
fn copy_border_image_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_image_width = s.border_image_width.clone();
}
fn copy_border_image_outset(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_image_outset = s.border_image_outset.clone();
}
fn copy_border_image_repeat(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_image_repeat = s.border_image_repeat.clone();
}

// ── Table ───────────────────────────────────────────────────────────────────

fn apply_border_collapse(s: &mut ComputedStyle, v: &str) {
    s.border_collapse = v == "collapse";
}
fn apply_border_spacing(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.split_whitespace().collect();
    s.border_spacing_h = parse_length(parts.first().copied().unwrap_or("0"));
    s.border_spacing_v = parse_length(
        parts
            .get(1)
            .copied()
            .unwrap_or(parts.first().copied().unwrap_or("0")),
    );
}
fn apply_caption_side(s: &mut ComputedStyle, v: &str) {
    s.caption_side = match v {
        "bottom" => CaptionSide::Bottom,
        "block-start" => CaptionSide::BlockStart,
        "block-end" => CaptionSide::BlockEnd,
        "inline-start" => CaptionSide::InlineStart,
        "inline-end" => CaptionSide::InlineEnd,
        _ => CaptionSide::Top,
    };
}
fn apply_empty_cells(s: &mut ComputedStyle, v: &str) {
    s.empty_cells_hide = v == "hide";
}
fn apply_table_layout(s: &mut ComputedStyle, v: &str) {
    s.table_layout_fixed = v == "fixed";
}

fn copy_border_collapse(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_collapse = s.border_collapse;
}
fn copy_border_spacing(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.border_spacing_h = s.border_spacing_h.clone();
    d.border_spacing_v = s.border_spacing_v.clone();
}
fn copy_caption_side(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.caption_side = s.caption_side;
}
fn copy_empty_cells(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.empty_cells_hide = s.empty_cells_hide;
}
fn copy_table_layout(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.table_layout_fixed = s.table_layout_fixed;
}

// ── Vertical align ──────────────────────────────────────────────────────────

fn apply_vertical_align(s: &mut ComputedStyle, v: &str) {
    s.vertical_align = match v {
        "top" => VerticalAlign::Top,
        "middle" => VerticalAlign::Middle,
        "bottom" => VerticalAlign::Bottom,
        "text-top" => VerticalAlign::TextTop,
        "text-bottom" => VerticalAlign::TextBottom,
        "sub" => VerticalAlign::Sub,
        "super" => VerticalAlign::Super,
        _ => parse_length_checked(v)
            .map(VerticalAlign::Length)
            .unwrap_or(VerticalAlign::Baseline),
    };
}
fn copy_vertical_align(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.vertical_align = s.vertical_align.clone();
}

// ── Text decoration ─────────────────────────────────────────────────────────

fn apply_text_decoration(s: &mut ComputedStyle, v: &str) {
    s.text_decoration.underline = v.contains("underline");
    s.text_decoration.overline = v.contains("overline");
    s.text_decoration.strikethrough = v.contains("line-through");
    s.text_decoration_style = if v.contains("double") {
        TextDecorationStyle::Double
    } else if v.contains("dotted") {
        TextDecorationStyle::Dotted
    } else if v.contains("dashed") {
        TextDecorationStyle::Dashed
    } else if v.contains("wavy") {
        TextDecorationStyle::Wavy
    } else {
        TextDecorationStyle::Solid
    };
    for token in v.split_whitespace() {
        if let Some(c) = parse_color(token) {
            s.text_decoration_color = Some(c);
            break;
        }
    }
}
fn apply_text_decoration_line(s: &mut ComputedStyle, v: &str) {
    s.text_decoration.underline = v.contains("underline");
    s.text_decoration.overline = v.contains("overline");
    s.text_decoration.strikethrough = v.contains("line-through");
}
fn apply_text_decoration_color(s: &mut ComputedStyle, v: &str) {
    s.text_decoration_color = parse_color(v);
}
fn apply_text_decoration_style_fn(s: &mut ComputedStyle, v: &str) {
    s.text_decoration_style = match v {
        "double" => TextDecorationStyle::Double,
        "dotted" => TextDecorationStyle::Dotted,
        "dashed" => TextDecorationStyle::Dashed,
        "wavy" => TextDecorationStyle::Wavy,
        _ => TextDecorationStyle::Solid,
    };
}
fn apply_text_decoration_thickness(s: &mut ComputedStyle, v: &str) {
    s.text_decoration_thickness = parse_length(v);
}
fn apply_text_decoration_skip_ink(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.text_decoration_skip_ink, v, &["auto", "none", "all"]);
}
fn copy_text_decoration(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration = s.text_decoration;
    d.text_decoration_color = s.text_decoration_color;
    d.text_decoration_style = s.text_decoration_style;
}
fn apply_text_emphasis(s: &mut ComputedStyle, v: &str) {
    s.text_emphasis_style = String::from("none");
    s.text_emphasis_color = None;
    let mut rest = v.trim().to_string();
    if let Some(start) = rest.find("rgb(").or_else(|| rest.find("rgba(")) {
        if let Some(end) = rest[start..].find(')') {
            let color_text = &rest[start..start + end + 1];
            s.text_emphasis_color = parse_color(color_text);
            rest.replace_range(start..start + end + 1, " ");
        }
    }
    let mut style_tokens = Vec::new();
    for token in rest.split_whitespace() {
        if let Some(color) = parse_color(token) {
            s.text_emphasis_color = Some(color);
        } else {
            style_tokens.push(token);
        }
    }
    if !style_tokens.is_empty() {
        apply_text_emphasis_style(s, &style_tokens.join(" "));
    }
}
fn apply_text_emphasis_style(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none"
        || value == "filled"
        || value == "open"
        || value == "dot"
        || value == "circle"
        || value == "double-circle"
        || value == "triangle"
        || value == "sesame"
        || value.starts_with('"')
        || value.starts_with('\'')
        || value.split_whitespace().all(|tok| {
            matches!(
                tok,
                "filled" | "open" | "dot" | "circle" | "double-circle" | "triangle" | "sesame"
            )
        })
    {
        s.text_emphasis_style = value.to_string();
    }
}
fn apply_text_emphasis_color(s: &mut ComputedStyle, v: &str) {
    s.text_emphasis_color = parse_color(v);
}
fn apply_text_emphasis_position(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.text_emphasis_position,
        v,
        &["over", "under", "right", "left"],
    );
}
fn apply_text_underline_offset(s: &mut ComputedStyle, v: &str) {
    s.text_underline_offset = parse_length(v);
}
fn apply_text_underline_position(s: &mut ComputedStyle, v: &str) {
    s.text_underline_position = if v.split_whitespace().any(|tok| tok == "under") {
        TextUnderlinePosition::Under
    } else if v.split_whitespace().any(|tok| tok == "left") {
        TextUnderlinePosition::Left
    } else if v.split_whitespace().any(|tok| tok == "right") {
        TextUnderlinePosition::Right
    } else if v.split_whitespace().any(|tok| tok == "from-font") {
        TextUnderlinePosition::FromFont
    } else {
        TextUnderlinePosition::Auto
    };
}
fn apply_text_overflow(s: &mut ComputedStyle, v: &str) {
    let tokens = split_text_overflow_tokens(v);
    if tokens.is_empty() || tokens.len() > 2 {
        s.text_overflow = TextOverflow::Clip;
        s.text_overflow_string.clear();
        return;
    }

    let end_marker = tokens.last().map(String::as_str).unwrap_or("clip");
    match end_marker {
        "ellipsis" => {
            s.text_overflow = TextOverflow::Ellipsis;
            s.text_overflow_string.clear();
        }
        "clip" => {
            s.text_overflow = TextOverflow::Clip;
            s.text_overflow_string.clear();
        }
        marker if is_quoted_css_string(marker) => {
            s.text_overflow = TextOverflow::Ellipsis;
            s.text_overflow_string = crate::css::resolve_content_value(marker);
        }
        _ => {
            s.text_overflow = TextOverflow::Clip;
            s.text_overflow_string.clear();
        }
    }
}

fn split_text_overflow_tokens(v: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = v.trim().chars().peekable();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let mut token = String::new();
            token.push(chars.next().unwrap());
            let mut escaped = false;
            for c in chars.by_ref() {
                token.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == quote {
                    break;
                }
            }
            tokens.push(token);
        } else {
            let mut token = String::new();
            while let Some(c) = chars.peek().copied() {
                if c.is_whitespace() {
                    break;
                }
                token.push(c);
                chars.next();
            }
            tokens.push(token);
        }
    }
    tokens
}

fn is_quoted_css_string(v: &str) -> bool {
    (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))
}
fn apply_text_wrap(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.text_wrap,
        v,
        &["wrap", "nowrap", "balance", "pretty", "stable"],
    );
}
fn apply_text_shadow(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.text_shadow = None;
    } else {
        let ts = super::parse_shadow_value(v);
        s.text_shadow = Some(TextShadow {
            offset_x: ts.0,
            offset_y: ts.1,
            blur: ts.2,
            color: ts.3,
        });
    }
}

fn copy_text_decoration_line(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration = s.text_decoration;
}
fn copy_text_decoration_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration_color = s.text_decoration_color;
}
fn copy_text_decoration_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration_style = s.text_decoration_style;
}
fn copy_text_decoration_thickness(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration_thickness = s.text_decoration_thickness.clone();
}
fn copy_text_decoration_skip_ink(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_decoration_skip_ink = s.text_decoration_skip_ink.clone();
}
fn copy_text_emphasis_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_emphasis_style = s.text_emphasis_style.clone();
}
fn copy_text_emphasis_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_emphasis_color = s.text_emphasis_color;
}
fn copy_text_emphasis_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_emphasis_position = s.text_emphasis_position.clone();
}
fn copy_text_underline_offset(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_underline_offset = s.text_underline_offset.clone();
}
fn copy_text_underline_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_underline_position = s.text_underline_position;
}
fn copy_text_wrap(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_wrap = s.text_wrap.clone();
}
fn copy_text_overflow(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_overflow = s.text_overflow;
    d.text_overflow_string = s.text_overflow_string.clone();
}
fn copy_text_shadow(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_shadow = s.text_shadow.clone();
}

// ── Word / overflow-wrap ────────────────────────────────────────────────────

fn apply_word_break(s: &mut ComputedStyle, v: &str) {
    s.word_break = match v {
        "break-all" => WordBreak::BreakAll,
        "keep-all" => WordBreak::KeepAll,
        "break-word" => WordBreak::BreakWord,
        _ => WordBreak::Normal,
    };
}
fn apply_overflow_wrap(s: &mut ComputedStyle, v: &str) {
    s.overflow_wrap = match v {
        "break-word" => OverflowWrap::BreakWord,
        "anywhere" => OverflowWrap::Anywhere,
        _ => OverflowWrap::Normal,
    };
}

fn copy_word_break(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.word_break = s.word_break;
}
fn copy_overflow_wrap(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overflow_wrap = s.overflow_wrap;
}

// ── List style ──────────────────────────────────────────────────────────────

fn apply_list_style_type(s: &mut ComputedStyle, v: &str) {
    s.custom_list_style_type.clear();
    s.list_style_type = match v {
        "none" => ListStyleType::None,
        "disc" => ListStyleType::Disc,
        "circle" => ListStyleType::Circle,
        "square" => ListStyleType::Square,
        "decimal" => ListStyleType::Decimal,
        "decimal-leading-zero" => ListStyleType::DecimalLeadingZero,
        "lower-alpha" => ListStyleType::LowerAlpha,
        "upper-alpha" => ListStyleType::UpperAlpha,
        "lower-latin" => ListStyleType::LowerLatin,
        "upper-latin" => ListStyleType::UpperLatin,
        "lower-roman" => ListStyleType::LowerRoman,
        "upper-roman" => ListStyleType::UpperRoman,
        "lower-greek" => ListStyleType::LowerGreek,
        "armenian" => ListStyleType::Armenian,
        "georgian" => ListStyleType::Georgian,
        "hebrew" => ListStyleType::Hebrew,
        "hiragana" => ListStyleType::Hiragana,
        "katakana" => ListStyleType::Katakana,
        "hiragana-iroha" => ListStyleType::HiraganaIroha,
        "katakana-iroha" => ListStyleType::KatakanaIroha,
        "cjk-decimal" => ListStyleType::CjkDecimal,
        _ => {
            if is_custom_ident(v) {
                s.custom_list_style_type = v.to_string();
                ListStyleType::None
            } else {
                ListStyleType::None
            }
        }
    };
}
fn apply_list_style_position(s: &mut ComputedStyle, v: &str) {
    s.list_style_position = if v == "inside" {
        ListStylePosition::Inside
    } else {
        ListStylePosition::Outside
    };
}
fn apply_list_style_image(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.list_style_image = String::new();
    } else if let Some(url) = super::extract_url(v) {
        s.list_style_image = url;
    }
}
fn apply_list_style(s: &mut ComputedStyle, v: &str) {
    s.custom_list_style_type.clear();
    s.list_style_type = ListStyleType::Disc;
    s.list_style_position = ListStylePosition::Outside;
    s.list_style_image.clear();
    if let Some(url) = super::extract_url(v) {
        s.list_style_image = url;
    }
    let v_no_url = if let Some(start) = v.find("url(") {
        let mut depth = 0;
        let mut end = v.len();
        for (i, ch) in v[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        format!("{} {}", &v[..start], &v[end..])
    } else {
        v.to_string()
    };
    for token in v_no_url.split_whitespace() {
        match token {
            "inside" | "outside" => apply_list_style_position(s, token),
            "none" => {
                s.list_style_type = ListStyleType::None;
                s.custom_list_style_type.clear();
                s.list_style_image.clear();
            }
            other => apply_list_style_type(s, other),
        }
    }
}

fn copy_list_style_type(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.list_style_type = s.list_style_type;
    d.custom_list_style_type = s.custom_list_style_type.clone();
}
fn copy_list_style_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.list_style_position = s.list_style_position;
}
fn copy_list_style_image(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.list_style_image = s.list_style_image.clone();
}

// ── Flexbox ─────────────────────────────────────────────────────────────────

fn apply_flex_direction(s: &mut ComputedStyle, v: &str) {
    s.flex_direction = match v {
        "row-reverse" => FlexDirection::RowReverse,
        "column" => FlexDirection::Column,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => FlexDirection::Row,
    };
}
fn apply_flex_wrap(s: &mut ComputedStyle, v: &str) {
    s.flex_wrap = match v {
        "wrap" => FlexWrap::Wrap,
        "wrap-reverse" => FlexWrap::WrapReverse,
        _ => FlexWrap::Nowrap,
    };
}
fn apply_flex_grow(s: &mut ComputedStyle, v: &str) {
    s.flex_grow = v.parse().unwrap_or(0.0);
}
fn apply_flex_shrink(s: &mut ComputedStyle, v: &str) {
    s.flex_shrink = v.parse().unwrap_or(1.0);
}
fn apply_flex_basis(s: &mut ComputedStyle, v: &str) {
    // `content` is legal on `flex-basis` alone (Flexbox §7.2.3), so it is read
    // here rather than in the shared length parser.
    s.flex_basis = if v.trim().eq_ignore_ascii_case("content") {
        CssLength::Content
    } else {
        parse_length(v)
    };
}
fn apply_order(s: &mut ComputedStyle, v: &str) {
    s.order = v.parse().unwrap_or(0);
}

fn apply_flex(s: &mut ComputedStyle, v: &str) {
    // Flexbox §7: the components may appear in any order — two numbers give
    // grow then shrink, and anything else is the basis. Reading the basis only
    // out of the THIRD slot dropped it from every two-value form, so
    // `flex: 1 30%` silently kept whatever basis the element already had.
    if v.trim().eq_ignore_ascii_case("none") {
        s.flex_grow = 0.0;
        s.flex_shrink = 0.0;
        s.flex_basis = CssLength::Auto;
        return;
    }
    let mut grow: Option<f32> = None;
    let mut shrink: Option<f32> = None;
    let mut basis: Option<CssLength> = None;
    for tok in super::split_css_shorthand_values(v) {
        let t = tok.as_str();
        if let Ok(n) = t.parse::<f32>() {
            // A bare number is a flex factor — unless both are already taken,
            // in which case it is the basis (`flex: 1 1 0`).
            if grow.is_none() {
                grow = Some(n);
            } else if shrink.is_none() {
                shrink = Some(n);
            } else if basis.is_none() {
                basis = Some(CssLength::Zero);
            }
            continue;
        }
        if basis.is_none() {
            basis = Some(if t.eq_ignore_ascii_case("content") {
                CssLength::Content
            } else {
                parse_length(t)
            });
        }
    }
    // Omitted components take the shorthand's own defaults, which are not the
    // properties' initial values: an absent basis is 0, not `auto`.
    s.flex_grow = grow.unwrap_or(1.0);
    s.flex_shrink = shrink.unwrap_or(1.0);
    s.flex_basis = basis.unwrap_or(CssLength::Zero);
}
fn apply_flex_flow(s: &mut ComputedStyle, v: &str) {
    for tok in v.split_whitespace() {
        match tok {
            "row" => {
                s.flex_direction = FlexDirection::Row;
            }
            "row-reverse" => {
                s.flex_direction = FlexDirection::RowReverse;
            }
            "column" => {
                s.flex_direction = FlexDirection::Column;
            }
            "column-reverse" => {
                s.flex_direction = FlexDirection::ColumnReverse;
            }
            "nowrap" => {
                s.flex_wrap = FlexWrap::Nowrap;
            }
            "wrap" => {
                s.flex_wrap = FlexWrap::Wrap;
            }
            "wrap-reverse" => {
                s.flex_wrap = FlexWrap::WrapReverse;
            }
            _ => {}
        }
    }
}
/// Split an alignment value into its `<overflow-position>` and the position
/// keyword, per the Box Alignment grammar. `first baseline` and `last baseline`
/// collapse to the baseline keywords the callers below understand.
///
/// The parsers used to match the whole declaration as one string, so every
/// two-word form in the grammar — `safe center`, `unsafe flex-end`,
/// `first baseline`, `last baseline` — missed and silently took the property's
/// initial value instead.
fn split_alignment(v: &str) -> (&str, bool) {
    let v = v.trim();
    let mut safe = false;
    let mut rest = v;
    if let Some(r) = v.strip_prefix("safe ") {
        safe = true;
        rest = r.trim();
    } else if let Some(r) = v.strip_prefix("unsafe ") {
        safe = false;
        rest = r.trim();
    }
    let rest = match rest {
        "first baseline" => "baseline",
        "last baseline" => "last-baseline",
        other => other,
    };
    (rest, safe)
}

/// Set or clear one of the `align_safety` bits.
fn set_safety(s: &mut ComputedStyle, bit: u8, safe: bool) {
    if safe {
        s.align_safety |= bit;
    } else {
        s.align_safety &= !bit;
    }
}

pub const SAFETY_JUSTIFY_CONTENT: u8 = 1;
pub const SAFETY_ALIGN_CONTENT: u8 = 2;
pub const SAFETY_ALIGN_ITEMS: u8 = 4;
pub const SAFETY_ALIGN_SELF: u8 = 8;

fn apply_justify_content(s: &mut ComputedStyle, v: &str) {
    let (v, safe) = split_alignment(v);
    set_safety(s, SAFETY_JUSTIFY_CONTENT, safe);
    s.justify_content = match v {
        "flex-end" | "end" | "self-end" => JustifyContent::FlexEnd,
        "center" => JustifyContent::Center,
        "space-between" => JustifyContent::SpaceBetween,
        "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly,
        // Physical, and so immune to `row-reverse`.
        "left" => JustifyContent::Left,
        "right" => JustifyContent::Right,
        _ => JustifyContent::FlexStart,
    };
}
fn apply_align_items(s: &mut ComputedStyle, v: &str) {
    let (v, safe) = split_alignment(v);
    set_safety(s, SAFETY_ALIGN_ITEMS, safe);
    s.align_items = match v {
        "flex-start" | "start" | "self-start" => AlignItems::FlexStart,
        "flex-end" | "end" | "self-end" => AlignItems::FlexEnd,
        "center" => AlignItems::Center,
        "baseline" => AlignItems::Baseline,
        "last-baseline" => AlignItems::LastBaseline,
        _ => AlignItems::Stretch,
    };
}
fn apply_align_self(s: &mut ComputedStyle, v: &str) {
    let (v, safe) = split_alignment(v);
    set_safety(s, SAFETY_ALIGN_SELF, safe);
    s.align_self = match v {
        "flex-start" | "start" | "self-start" => AlignSelf::FlexStart,
        "flex-end" | "end" | "self-end" => AlignSelf::FlexEnd,
        "center" => AlignSelf::Center,
        "baseline" => AlignSelf::Baseline,
        "last-baseline" => AlignSelf::LastBaseline,
        "stretch" => AlignSelf::Stretch,
        _ => AlignSelf::Auto,
    };
}
fn apply_align_content(s: &mut ComputedStyle, v: &str) {
    let (v, safe) = split_alignment(v);
    set_safety(s, SAFETY_ALIGN_CONTENT, safe);
    s.align_content = match v {
        "flex-start" | "start" | "baseline" | "last-baseline" => AlignContent::FlexStart,
        "flex-end" | "end" => AlignContent::FlexEnd,
        "center" => AlignContent::Center,
        "space-between" => AlignContent::SpaceBetween,
        "space-around" => AlignContent::SpaceAround,
        "space-evenly" => AlignContent::SpaceEvenly,
        _ => AlignContent::Stretch,
    };
}
fn apply_justify_items(s: &mut ComputedStyle, v: &str) {
    let (v, _) = split_alignment(v);
    s.justify_items = match v {
        "flex-start" | "start" | "self-start" | "left" => AlignItems::FlexStart,
        "flex-end" | "end" | "self-end" | "right" => AlignItems::FlexEnd,
        "center" => AlignItems::Center,
        "baseline" => AlignItems::Baseline,
        "last-baseline" => AlignItems::LastBaseline,
        _ => AlignItems::Stretch,
    };
}
fn apply_justify_self(s: &mut ComputedStyle, v: &str) {
    let (v, _) = split_alignment(v);
    s.justify_self = match v {
        "flex-start" | "start" | "self-start" | "left" => AlignSelf::FlexStart,
        "flex-end" | "end" | "self-end" | "right" => AlignSelf::FlexEnd,
        "center" => AlignSelf::Center,
        "baseline" => AlignSelf::Baseline,
        "last-baseline" => AlignSelf::LastBaseline,
        "stretch" => AlignSelf::Stretch,
        _ => AlignSelf::Auto,
    };
}
fn apply_gap(s: &mut ComputedStyle, v: &str) {
    // `gap: <row> <column>` — the one-value form applies to both. Parsing the
    // whole declaration as a single length made `gap: 10px 30px` unparseable,
    // so both gaps fell back to their initial value.
    let toks = super::split_css_shorthand_values(v);
    let row = parse_length(toks.first().map(|t| t.as_str()).unwrap_or(v));
    let col = match toks.get(1) {
        Some(t) => parse_length(t.as_str()),
        None => row.clone(),
    };
    s.row_gap = row.clone();
    s.column_gap = col;
    s.gap = row;
}
fn apply_row_gap(s: &mut ComputedStyle, v: &str) {
    s.row_gap = parse_length(v);
}
fn apply_column_gap(s: &mut ComputedStyle, v: &str) {
    s.column_gap = parse_length(v);
}

fn copy_flex_direction(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.flex_direction = s.flex_direction;
}
fn copy_flex_wrap(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.flex_wrap = s.flex_wrap;
}
fn copy_flex_grow(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.flex_grow = s.flex_grow;
}
fn copy_flex_shrink(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.flex_shrink = s.flex_shrink;
}
fn copy_flex_basis(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.flex_basis = s.flex_basis.clone();
}
fn copy_order(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.order = s.order;
}
fn copy_justify_content(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.justify_content = s.justify_content;
}
fn copy_align_items(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.align_items = s.align_items;
}
fn copy_align_self(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.align_self = s.align_self;
}
fn copy_align_content(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.align_content = s.align_content;
}
fn copy_justify_items(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.justify_items = s.justify_items;
}
fn copy_justify_self(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.justify_self = s.justify_self;
}
fn copy_row_gap(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.row_gap = s.row_gap.clone();
    d.gap = s.gap.clone();
}
fn copy_column_gap(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_gap = s.column_gap.clone();
}

// ── Grid ────────────────────────────────────────────────────────────────────

fn apply_grid_template_columns(s: &mut ComputedStyle, v: &str) {
    let mut names = std::collections::HashMap::new();
    let tracks =
        super::parse_track_list_with_names(v, &mut s.rare_mut().auto_repeat_columns, &mut names);
    if tracks.first().map(|t| t.is_subgrid()).unwrap_or(false) {
        s.subgrid_columns = true;
        s.rare_mut().grid_template_columns = Vec::new();
    } else {
        s.subgrid_columns = false;
        s.rare_mut().grid_template_columns = tracks;
    }
    s.grid_col_line_names = names;
}
fn apply_grid_template_rows(s: &mut ComputedStyle, v: &str) {
    let mut dummy = Vec::new();
    let mut names = std::collections::HashMap::new();
    let tracks = super::parse_track_list_with_names(v, &mut dummy, &mut names);
    if tracks.first().map(|t| t.is_subgrid()).unwrap_or(false) {
        s.subgrid_rows = true;
        s.rare_mut().grid_template_rows = Vec::new();
    } else {
        s.subgrid_rows = false;
        s.rare_mut().grid_template_rows = tracks;
    }
    s.grid_row_line_names = names;
}
fn apply_grid_template_areas(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().grid_template_areas = super::parse_grid_template_areas(v);
}
fn apply_grid_auto_columns(s: &mut ComputedStyle, v: &str) {
    s.grid_auto_columns = super::parse_single_track(v);
}
fn apply_grid_auto_rows(s: &mut ComputedStyle, v: &str) {
    s.grid_auto_rows = super::parse_single_track(v);
}
fn apply_grid_auto_flow(s: &mut ComputedStyle, v: &str) {
    s.grid_auto_flow = match v {
        "column" => GridAutoFlow::Column,
        "row dense" => GridAutoFlow::RowDense,
        "column dense" => GridAutoFlow::ColumnDense,
        _ => GridAutoFlow::Row,
    };
}
fn apply_grid_column(s: &mut ComputedStyle, v: &str) {
    if let Some(slash) = v.find('/') {
        let (sv, sn) = super::parse_grid_line_named(v[..slash].trim());
        let (ev, en) = super::parse_grid_line_named(v[slash + 1..].trim());
        s.grid_column_start = sv;
        s.grid_column_end = ev;
        s.grid_column_start_name = sn;
        s.grid_column_end_name = en;
    } else {
        let (val, name) = super::parse_grid_line_named(v);
        s.grid_column_start = val;
        s.grid_column_end = 0;
        if !name.is_empty() {
            s.grid_column_start_name = format!("{}-start", name);
            s.grid_column_end_name = format!("{}-end", name);
        } else {
            s.grid_column_start_name = String::new();
            s.grid_column_end_name = String::new();
        }
    }
}
fn apply_grid_row(s: &mut ComputedStyle, v: &str) {
    if let Some(slash) = v.find('/') {
        let (sv, sn) = super::parse_grid_line_named(v[..slash].trim());
        let (ev, en) = super::parse_grid_line_named(v[slash + 1..].trim());
        s.grid_row_start = sv;
        s.grid_row_end = ev;
        s.grid_row_start_name = sn;
        s.grid_row_end_name = en;
    } else {
        let (val, name) = super::parse_grid_line_named(v);
        s.grid_row_start = val;
        s.grid_row_end = 0;
        if !name.is_empty() {
            s.grid_row_start_name = format!("{}-start", name);
            s.grid_row_end_name = format!("{}-end", name);
        } else {
            s.grid_row_start_name = String::new();
            s.grid_row_end_name = String::new();
        }
    }
}
fn apply_grid_column_start(s: &mut ComputedStyle, v: &str) {
    let (val, name) = super::parse_grid_line_named(v);
    s.grid_column_start = val;
    s.grid_column_start_name = name;
}
fn apply_grid_column_end(s: &mut ComputedStyle, v: &str) {
    let (val, name) = super::parse_grid_line_named(v);
    s.grid_column_end = val;
    s.grid_column_end_name = name;
}
fn apply_grid_row_start(s: &mut ComputedStyle, v: &str) {
    let (val, name) = super::parse_grid_line_named(v);
    s.grid_row_start = val;
    s.grid_row_start_name = name;
}
fn apply_grid_row_end(s: &mut ComputedStyle, v: &str) {
    let (val, name) = super::parse_grid_line_named(v);
    s.grid_row_end = val;
    s.grid_row_end_name = name;
}
fn apply_grid_area(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.splitn(4, '/').collect();
    if parts.len() == 4 {
        let rs = super::parse_grid_line(parts[0].trim());
        let cs = super::parse_grid_line(parts[1].trim());
        let re = super::parse_grid_line(parts[2].trim());
        let ce = super::parse_grid_line(parts[3].trim());
        if rs != 0 || cs != 0 || re != 0 || ce != 0 {
            s.grid_row_start = rs;
            s.grid_column_start = cs;
            s.grid_row_end = re;
            s.grid_column_end = ce;
        } else {
            s.grid_area = v.to_string();
        }
    } else {
        s.grid_area = v.to_string();
    }
}
fn apply_grid_template(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.rare_mut().grid_template_rows.clear();
        s.rare_mut().grid_template_columns.clear();
        s.subgrid_columns = false;
        s.subgrid_rows = false;
    } else if let Some(slash) = v.find('/') {
        let rows_part = v[..slash].trim();
        let cols_part = v[slash + 1..].trim();
        apply_grid_template_rows(s, rows_part);
        apply_grid_template_columns(s, cols_part);
    }
}

fn copy_grid_template_columns(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().grid_template_columns = s.rare().grid_template_columns.clone();
    d.rare_mut().auto_repeat_columns = s.rare().auto_repeat_columns.clone();
    d.grid_col_line_names = s.grid_col_line_names.clone();
    d.subgrid_columns = s.subgrid_columns;
}
fn copy_grid_template_rows(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().grid_template_rows = s.rare().grid_template_rows.clone();
    d.grid_row_line_names = s.grid_row_line_names.clone();
    d.subgrid_rows = s.subgrid_rows;
}
fn copy_grid_template_areas(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().grid_template_areas = s.rare().grid_template_areas.clone();
}
fn copy_grid_auto_columns(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_auto_columns = s.grid_auto_columns.clone();
}
fn copy_grid_auto_rows(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_auto_rows = s.grid_auto_rows.clone();
}
fn copy_grid_auto_flow(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_auto_flow = s.grid_auto_flow;
}
fn copy_grid_column_start(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_column_start = s.grid_column_start;
    d.grid_column_start_name = s.grid_column_start_name.clone();
}
fn copy_grid_column_end(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_column_end = s.grid_column_end;
    d.grid_column_end_name = s.grid_column_end_name.clone();
}
fn copy_grid_row_start(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_row_start = s.grid_row_start;
    d.grid_row_start_name = s.grid_row_start_name.clone();
}
fn copy_grid_row_end(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.grid_row_end = s.grid_row_end;
    d.grid_row_end_name = s.grid_row_end_name.clone();
}

// ── Background ──────────────────────────────────────────────────────────────

fn apply_background(s: &mut ComputedStyle, v: &str) {
    let layers = crate::css::value_parse::split_top_level_commas(v);
    if layers.len() > 1 {
        reset_background_fields(s);

        let mut selected = None;
        let mut final_color = Color::TRANSPARENT;
        for layer in &layers {
            let mut parsed = ComputedStyle::default();
            apply_background_single_layer(&mut parsed, layer.trim());
            if layer_has_background_image(&parsed) && selected.is_none() {
                selected = Some(parsed.clone());
            }
            final_color = background_layer_color(layer.trim()).unwrap_or(parsed.background_color);
        }

        if let Some(parsed) = selected {
            copy_background_layer_fields(s, &parsed);
        }
        s.background_color = final_color;
        return;
    }

    apply_background_single_layer(s, v);
}

fn reset_background_fields(s: &mut ComputedStyle) {
    s.background_color = Color::TRANSPARENT;
    s.background_image_url.clear();
    s.gradient_type = GradientType::None;
    s.gradient_angle = 180.0;
    s.gradient_direction = GradientDirection::Angle(180.0);
    s.gradient_radial_shape = GradientRadialShape::Ellipse;
    s.gradient_radial_size = GradientRadialSize::FarthestCorner;
    s.gradient_radial_radius_x = CssLength::Auto;
    s.gradient_radial_radius_y = CssLength::Auto;
    s.gradient_radial_position_x = CssLength::Percent(50.0);
    s.gradient_radial_position_y = CssLength::Percent(50.0);
    s.rare_mut().gradient_stops.clear();
    s.background_position_x = CssLength::Zero;
    s.background_position_y = CssLength::Zero;
    s.background_size = BackgroundSize::Auto;
    s.background_size_w = CssLength::Auto;
    s.background_size_h = CssLength::Auto;
    s.background_repeat = BackgroundRepeat::Repeat;
    s.background_attachment = BackgroundAttachment::Scroll;
    s.background_origin = BackgroundClip::PaddingBox;
    s.background_clip = BackgroundClip::BorderBox;
}

fn layer_has_background_image(s: &ComputedStyle) -> bool {
    !s.background_image_url.is_empty() || s.gradient_type != GradientType::None
}

fn copy_background_layer_fields(dst: &mut ComputedStyle, src: &ComputedStyle) {
    dst.background_image_url = src.background_image_url.clone();
    dst.gradient_type = src.gradient_type;
    dst.gradient_angle = src.gradient_angle;
    dst.gradient_direction = src.gradient_direction;
    dst.gradient_radial_shape = src.gradient_radial_shape;
    dst.gradient_radial_size = src.gradient_radial_size;
    dst.gradient_radial_radius_x = src.gradient_radial_radius_x.clone();
    dst.gradient_radial_radius_y = src.gradient_radial_radius_y.clone();
    dst.gradient_radial_position_x = src.gradient_radial_position_x.clone();
    dst.gradient_radial_position_y = src.gradient_radial_position_y.clone();
    dst.rare_mut().gradient_stops = src.rare().gradient_stops.clone();
    dst.background_position_x = src.background_position_x.clone();
    dst.background_position_y = src.background_position_y.clone();
    dst.background_size = src.background_size;
    dst.background_size_w = src.background_size_w.clone();
    dst.background_size_h = src.background_size_h.clone();
    dst.background_repeat = src.background_repeat;
    dst.background_attachment = src.background_attachment;
    dst.background_origin = src.background_origin;
    dst.background_clip = src.background_clip;
}

fn background_layer_color(layer: &str) -> Option<Color> {
    let without_url = if let Some(start) = layer.find("url(") {
        let mut depth = 0;
        let mut end = layer.len();
        for (i, ch) in layer[start..].char_indices() {
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
        }
        format!("{} {}", &layer[..start], &layer[end..])
    } else {
        layer.to_string()
    };

    split_top_level_whitespace(&without_url)
        .into_iter()
        .filter_map(parse_color)
        .next()
}

fn apply_background_single_layer(s: &mut ComputedStyle, v: &str) {
    reset_background_fields(s);
    // Handle gradient functions first
    if v.to_ascii_lowercase().contains("gradient") {
        super::apply_gradient(s, v);
        return;
    }
    // Extract url() first
    if let Some(url) = super::extract_url(v) {
        s.background_image_url = url;
    }
    // Strip url(...) from value before splitting
    let v_no_url = if let Some(start) = v.find("url(") {
        let depth_start = start;
        let mut depth = 0;
        let mut end = v.len();
        for (i, ch) in v[depth_start..].char_indices() {
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end = depth_start + i + 1;
                    break;
                }
            }
        }
        format!("{} {}", &v[..depth_start], &v[end..])
    } else {
        v.to_string()
    };
    let v_rest = v_no_url.trim();
    let (pos_part, size_part) = if let Some(slash) = find_top_level_char(v_rest, '/') {
        (&v_rest[..slash], Some(&v_rest[slash + 1..]))
    } else {
        (v_rest, None)
    };
    if let Some(size_str) = size_part {
        let size_tokens = background_size_tokens(size_str);
        match size_tokens.first().copied().unwrap_or("auto") {
            "cover" => {
                s.background_size = BackgroundSize::Cover;
                s.background_size_w = CssLength::Auto;
                s.background_size_h = CssLength::Auto;
            }
            "contain" => {
                s.background_size = BackgroundSize::Contain;
                s.background_size_w = CssLength::Auto;
                s.background_size_h = CssLength::Auto;
            }
            _ => {
                s.background_size = BackgroundSize::Explicit;
                s.background_size_w = parse_length(size_tokens.first().copied().unwrap_or("auto"));
                s.background_size_h = size_tokens
                    .get(1)
                    .map(|tok| parse_length(tok))
                    .unwrap_or(CssLength::Auto);
            }
        }
        let repeat_tokens: Vec<&str> = split_top_level_whitespace(size_str)
            .into_iter()
            .skip(1)
            .filter(|tok| is_background_repeat_token(tok))
            .collect();
        apply_background_repeat_tokens(s, &repeat_tokens);
        let misc_tokens: Vec<&str> = split_top_level_whitespace(size_str)
            .into_iter()
            .skip(1)
            .collect();
        apply_background_misc_tokens(s, &misc_tokens);
    }
    let mut pos_tokens: Vec<String> = Vec::new();
    let mut repeat_tokens: Vec<&str> = Vec::new();
    let mut misc_tokens: Vec<&str> = Vec::new();
    for token in split_top_level_whitespace(pos_part) {
        let token_lower = token.to_ascii_lowercase();
        match token_lower.as_str() {
            "none" => {
                s.background_image_url.clear();
            }
            "no-repeat" | "repeat-x" | "repeat-y" | "repeat" | "space" | "round" => {
                repeat_tokens.push(token);
            }
            "scroll" | "fixed" | "local" | "border-box" | "padding-box" | "content-box"
            | "text" => {
                misc_tokens.push(token);
            }
            "left" | "center" | "right" | "top" | "bottom" => {
                pos_tokens.push(token_lower);
            }
            _ => {
                if let Some(c) = parse_color(token) {
                    s.background_color = c;
                } else if is_background_position_length_token(token) {
                    pos_tokens.push(token.to_string());
                }
            }
        }
    }
    apply_background_repeat_tokens(s, &repeat_tokens);
    apply_background_misc_tokens(s, &misc_tokens);
    if !pos_tokens.is_empty() {
        let mut x_set = false;
        let mut y_set = false;
        for tok in &pos_tokens {
            match tok.as_str() {
                "left" => {
                    s.background_position_x = CssLength::Percent(0.0);
                    x_set = true;
                }
                "right" => {
                    s.background_position_x = CssLength::Percent(100.0);
                    x_set = true;
                }
                "top" => {
                    s.background_position_y = CssLength::Percent(0.0);
                    y_set = true;
                }
                "bottom" => {
                    s.background_position_y = CssLength::Percent(100.0);
                    y_set = true;
                }
                "center" => {
                    if !x_set {
                        s.background_position_x = CssLength::Percent(50.0);
                        x_set = true;
                    } else if !y_set {
                        s.background_position_y = CssLength::Percent(50.0);
                        y_set = true;
                    }
                }
                other => {
                    let l = parse_length(other);
                    if !x_set {
                        s.background_position_x = l;
                        x_set = true;
                    } else if !y_set {
                        s.background_position_y = l;
                        y_set = true;
                    }
                }
            }
        }
        if x_set && !y_set {
            s.background_position_y = CssLength::Percent(50.0);
        }
    }
}

fn find_top_level_char(s: &str, needle: char) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            c if c == needle && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn split_top_level_whitespace(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => {
                if start.is_none() {
                    start = Some(i);
                }
                depth += 1;
            }
            ')' => {
                if start.is_none() {
                    start = Some(i);
                }
                depth = depth.saturating_sub(1);
            }
            c if c.is_whitespace() && depth == 0 => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            _ => {
                if start.is_none() {
                    start = Some(i);
                }
            }
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

fn is_background_position_length_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    parse_length_checked(token).is_some()
        || lower.starts_with("calc(")
        || lower.starts_with("min(")
        || lower.starts_with("max(")
        || lower.starts_with("clamp(")
}

fn background_size_tokens(value: &str) -> Vec<&str> {
    split_top_level_whitespace(value)
        .into_iter()
        .take_while(|tok| {
            let lower = tok.to_ascii_lowercase();
            lower == "auto"
                || lower == "cover"
                || lower == "contain"
                || parse_length_checked(tok).is_some()
        })
        .take(2)
        .collect()
}

fn apply_background_misc_tokens(s: &mut ComputedStyle, tokens: &[&str]) {
    let mut boxes = Vec::new();
    for token in tokens {
        match *token {
            "scroll" | "fixed" | "local" => apply_background_attachment(s, token),
            "border-box" | "padding-box" | "content-box" | "text" => boxes.push(*token),
            _ => {}
        }
    }
    if let Some(first) = boxes.first().copied() {
        apply_background_origin(s, first);
        apply_background_clip(s, boxes.get(1).copied().unwrap_or(first));
    }
}

fn apply_background_image(s: &mut ComputedStyle, v: &str) {
    let lower = v.to_ascii_lowercase();
    if lower.contains("gradient") {
        super::apply_gradient(s, v);
    } else if lower.trim() == "none" {
        s.background_image_url.clear();
    } else if let Some(url) = extract_image_set_url(v) {
        s.background_image_url = url;
    } else if let Some(url) = super::extract_url(v) {
        s.background_image_url = url;
    }
}

#[derive(Clone)]
struct ImageSetCandidate {
    url: String,
    resolution: f32,
}

fn extract_image_set_url(v: &str) -> Option<String> {
    let value = v.trim();
    let lower = value.to_ascii_lowercase();
    let inner = if lower.starts_with("image-set(") && value.ends_with(')') {
        &value["image-set(".len()..value.len() - 1]
    } else if lower.starts_with("-webkit-image-set(") && value.ends_with(')') {
        &value["-webkit-image-set(".len()..value.len() - 1]
    } else {
        return None;
    };

    let mut candidates = Vec::new();
    for candidate in super::split_top_level_commas(inner) {
        if let Some(parsed) = parse_image_set_candidate(candidate.trim()) {
            candidates.push(parsed);
        }
    }

    const DEVICE_PIXEL_RATIO: f32 = 1.0;
    candidates
        .iter()
        .filter(|candidate| candidate.resolution >= DEVICE_PIXEL_RATIO)
        .min_by(|a, b| a.resolution.total_cmp(&b.resolution))
        .or_else(|| {
            candidates
                .iter()
                .max_by(|a, b| a.resolution.total_cmp(&b.resolution))
        })
        .map(|candidate| candidate.url.clone())
}

fn parse_image_set_candidate(candidate: &str) -> Option<ImageSetCandidate> {
    let mut url = super::extract_url(candidate).or_else(|| extract_css_string_prefix(candidate))?;
    url = url.trim().to_string();
    if url.is_empty() || !image_set_candidate_type_is_supported(candidate) {
        return None;
    }
    let resolution = candidate
        .split_whitespace()
        .filter_map(parse_image_set_resolution_descriptor)
        .next()
        .unwrap_or(1.0);
    Some(ImageSetCandidate { url, resolution })
}

fn extract_css_string_prefix(value: &str) -> Option<String> {
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &value[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn parse_image_set_resolution_descriptor(token: &str) -> Option<f32> {
    let token = token.trim().trim_end_matches(',');
    let lower = token.to_ascii_lowercase();
    if let Some(value) = lower.strip_suffix('x') {
        return value.parse::<f32>().ok().filter(|v| *v > 0.0);
    }
    if let Some(value) = lower.strip_suffix("dppx") {
        return value.parse::<f32>().ok().filter(|v| *v > 0.0);
    }
    if let Some(value) = lower.strip_suffix("dpi") {
        return value
            .parse::<f32>()
            .ok()
            .map(|v| v / 96.0)
            .filter(|v| *v > 0.0);
    }
    if let Some(value) = lower.strip_suffix("dpcm") {
        return value
            .parse::<f32>()
            .ok()
            .map(|v| v / 37.795_276)
            .filter(|v| *v > 0.0);
    }
    None
}

fn image_set_candidate_type_is_supported(candidate: &str) -> bool {
    let lower = candidate.to_ascii_lowercase();
    let Some(type_start) = lower.find("type(") else {
        return true;
    };
    let after_type = &candidate[type_start + "type(".len()..];
    let Some(type_end) = after_type.find(')') else {
        return false;
    };
    let mime = after_type[..type_end]
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase();
    matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/bmp" | "image/svg+xml"
    )
}
fn apply_background_size(s: &mut ComputedStyle, v: &str) {
    let tokens = background_size_tokens(v);
    match tokens.first().map(|tok| tok.to_ascii_lowercase()) {
        Some(keyword) if keyword == "cover" => {
            s.background_size = BackgroundSize::Cover;
            s.background_size_w = CssLength::Auto;
            s.background_size_h = CssLength::Auto;
        }
        Some(keyword) if keyword == "contain" => {
            s.background_size = BackgroundSize::Contain;
            s.background_size_w = CssLength::Auto;
            s.background_size_h = CssLength::Auto;
        }
        Some(keyword) if keyword == "auto" && tokens.len() == 1 => {
            s.background_size = BackgroundSize::Auto;
            s.background_size_w = CssLength::Auto;
            s.background_size_h = CssLength::Auto;
        }
        _ => {
            s.background_size = BackgroundSize::Explicit;
            s.background_size_w = parse_length(tokens.first().copied().unwrap_or("auto"));
            s.background_size_h = tokens
                .get(1)
                .map(|tok| parse_length(tok))
                .unwrap_or(CssLength::Auto);
        }
    }
}
fn apply_background_position(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.split_whitespace().collect();
    let x_str = parts.first().copied().unwrap_or("0%");
    s.background_position_x = match x_str.to_ascii_lowercase().as_str() {
        "left" => CssLength::Percent(0.0),
        "center" => CssLength::Percent(50.0),
        "right" => CssLength::Percent(100.0),
        _ => parse_length(x_str),
    };
    let y_str = parts.get(1).copied().unwrap_or("center");
    s.background_position_y = match y_str.to_ascii_lowercase().as_str() {
        "top" => CssLength::Percent(0.0),
        "center" => CssLength::Percent(50.0),
        "bottom" => CssLength::Percent(100.0),
        _ => parse_length(y_str),
    };
}
fn apply_background_repeat(s: &mut ComputedStyle, v: &str) {
    let normalized = v.to_ascii_lowercase();
    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    apply_background_repeat_tokens(s, &tokens);
}

fn is_background_repeat_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "repeat" | "space" | "round" | "no-repeat" | "repeat-x" | "repeat-y"
    )
}

fn background_repeat_axis(token: &str) -> Option<BackgroundRepeatAxis> {
    match token.to_ascii_lowercase().as_str() {
        "repeat" => Some(BackgroundRepeatAxis::Repeat),
        "space" => Some(BackgroundRepeatAxis::Space),
        "round" => Some(BackgroundRepeatAxis::Round),
        "no-repeat" => Some(BackgroundRepeatAxis::NoRepeat),
        _ => None,
    }
}

fn apply_background_repeat_tokens(s: &mut ComputedStyle, tokens: &[&str]) {
    let Some(first) = tokens.first().copied() else {
        return;
    };
    let first_lower = first.to_ascii_lowercase();
    let second_lower = tokens.get(1).map(|tok| tok.to_ascii_lowercase());
    s.background_repeat = match (first_lower.as_str(), second_lower.as_deref()) {
        ("repeat-x", _) => BackgroundRepeat::RepeatX,
        ("repeat-y", _) => BackgroundRepeat::RepeatY,
        (_, Some(second)) => match (
            background_repeat_axis(first_lower.as_str()),
            background_repeat_axis(second),
        ) {
            (Some(x), Some(y)) => BackgroundRepeat::TwoValue(x, y),
            _ => s.background_repeat,
        },
        ("repeat", None) => BackgroundRepeat::Repeat,
        ("no-repeat", None) => BackgroundRepeat::NoRepeat,
        ("space", None) => BackgroundRepeat::Space,
        ("round", None) => BackgroundRepeat::Round,
        _ => s.background_repeat,
    };
}
fn apply_background_clip(s: &mut ComputedStyle, v: &str) {
    s.background_clip = match v.to_ascii_lowercase().as_str() {
        "padding-box" => BackgroundClip::PaddingBox,
        "content-box" => BackgroundClip::ContentBox,
        "text" => BackgroundClip::Text,
        _ => BackgroundClip::BorderBox,
    };
}
fn apply_background_origin(s: &mut ComputedStyle, v: &str) {
    s.background_origin = match v.to_ascii_lowercase().as_str() {
        "border-box" => BackgroundClip::BorderBox,
        "content-box" => BackgroundClip::ContentBox,
        _ => BackgroundClip::PaddingBox,
    };
}
fn apply_background_attachment(s: &mut ComputedStyle, v: &str) {
    s.background_attachment = match v.to_ascii_lowercase().as_str() {
        "fixed" => BackgroundAttachment::Fixed,
        "local" => BackgroundAttachment::Local,
        _ => BackgroundAttachment::Scroll,
    };
}
fn apply_background_blend_mode(s: &mut ComputedStyle, v: &str) {
    apply_comma_keyword_list(
        &mut s.background_blend_mode,
        v,
        &[
            "normal",
            "multiply",
            "screen",
            "overlay",
            "darken",
            "lighten",
            "color-dodge",
            "color-burn",
            "hard-light",
            "soft-light",
            "difference",
            "exclusion",
            "hue",
            "saturation",
            "color",
            "luminosity",
        ],
    );
}

fn copy_background_image(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_image_url = s.background_image_url.clone();
    d.gradient_type = s.gradient_type;
    d.gradient_angle = s.gradient_angle;
    d.gradient_direction = s.gradient_direction;
    d.gradient_radial_shape = s.gradient_radial_shape;
    d.gradient_radial_size = s.gradient_radial_size;
    d.gradient_radial_radius_x = s.gradient_radial_radius_x.clone();
    d.gradient_radial_radius_y = s.gradient_radial_radius_y.clone();
    d.gradient_radial_position_x = s.gradient_radial_position_x.clone();
    d.gradient_radial_position_y = s.gradient_radial_position_y.clone();
    d.rare_mut().gradient_stops = s.rare().gradient_stops.clone();
}
fn copy_background_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_size = s.background_size;
    d.background_size_w = s.background_size_w.clone();
    d.background_size_h = s.background_size_h.clone();
}
fn copy_background_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_position_x = s.background_position_x.clone();
    d.background_position_y = s.background_position_y.clone();
}
fn copy_background_repeat(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_repeat = s.background_repeat;
}
fn copy_background_clip(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_clip = s.background_clip;
}
fn copy_background_origin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_origin = s.background_origin;
}
fn copy_background_attachment(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_attachment = s.background_attachment;
}
fn copy_background_blend_mode(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.background_blend_mode = s.background_blend_mode.clone();
}

fn apply_mask(s: &mut ComputedStyle, v: &str) {
    apply_mask_initials(s);
    let (before, after) = v.split_once('/').unwrap_or((v, ""));
    let mut tokens = super::split_shorthand_values(before);
    let mut after_tokens = super::split_shorthand_values(after);
    if let Some(size) = after_tokens.first() {
        apply_mask_size(s, size);
        for token in after_tokens.drain(1..) {
            tokens.push(token);
        }
    }
    for token in tokens {
        match token {
            "none" => s.rare_mut().mask_image_url.clear(),
            "repeat" | "repeat-x" | "repeat-y" | "no-repeat" | "space" | "round" => {
                apply_mask_repeat(s, &token)
            }
            "alpha" | "luminance" | "match-source" => apply_mask_mode(s, &token),
            "add" | "subtract" | "intersect" | "exclude" => apply_mask_composite(s, &token),
            "border-box" | "padding-box" | "content-box" | "fill-box" | "stroke-box"
            | "view-box" | "no-clip" => {
                if token == "no-clip" || !s.rare().mask_origin.is_empty() {
                    apply_mask_clip(s, &token);
                } else {
                    apply_mask_origin(s, &token);
                }
            }
            _ if token.starts_with("url(") => apply_mask_image(s, &token),
            _ => {
                if !token.is_empty() {
                    apply_mask_position(s, &token);
                }
            }
        }
    }
}

fn apply_mask_initials(s: &mut ComputedStyle) {
    let rare = s.rare_mut();
    rare.mask_image_url.clear();
    rare.mask_mode.clear();
    rare.mask_repeat.clear();
    rare.mask_position.clear();
    rare.mask_size.clear();
    rare.mask_clip.clear();
    rare.mask_origin.clear();
    rare.mask_composite.clear();
}

fn apply_mask_image(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.rare_mut().mask_image_url.clear();
    } else if let Some(url) = super::extract_url(v) {
        s.rare_mut().mask_image_url = url;
    }
}
fn copy_mask_image(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_image_url = s.rare().mask_image_url.clone();
}
fn apply_mask_mode(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.rare_mut().mask_mode,
        v,
        &["alpha", "luminance", "match-source"],
    );
}
fn copy_mask_mode(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_mode = s.rare().mask_mode.clone();
}
fn apply_mask_repeat(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.rare_mut().mask_repeat,
        v,
        &[
            "repeat",
            "repeat-x",
            "repeat-y",
            "no-repeat",
            "space",
            "round",
        ],
    );
}
fn copy_mask_repeat(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_repeat = s.rare().mask_repeat.clone();
}
fn apply_mask_position(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().mask_position = v.to_string();
}
fn copy_mask_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_position = s.rare().mask_position.clone();
}
fn apply_mask_size(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().mask_size = v.to_string();
}
fn copy_mask_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_size = s.rare().mask_size.clone();
}
fn apply_mask_clip(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.rare_mut().mask_clip,
        v,
        &[
            "border-box",
            "padding-box",
            "content-box",
            "fill-box",
            "stroke-box",
            "view-box",
            "no-clip",
        ],
    );
}
fn copy_mask_clip(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_clip = s.rare().mask_clip.clone();
}
fn apply_mask_origin(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.rare_mut().mask_origin,
        v,
        &[
            "border-box",
            "padding-box",
            "content-box",
            "fill-box",
            "stroke-box",
            "view-box",
        ],
    );
}
fn copy_mask_origin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_origin = s.rare().mask_origin.clone();
}
fn apply_mask_composite(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.rare_mut().mask_composite,
        v,
        &["add", "subtract", "intersect", "exclude"],
    );
}
fn copy_mask_composite(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().mask_composite = s.rare().mask_composite.clone();
}

// ── Outline ─────────────────────────────────────────────────────────────────

fn apply_outline(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.outline_style = BorderStyle::None;
        s.outline_width = 0.0;
    } else {
        for tok in super::split_shorthand_values(v) {
            match tok {
                "solid" => {
                    s.outline_style = BorderStyle::Solid;
                }
                "dashed" => {
                    s.outline_style = BorderStyle::Dashed;
                }
                "dotted" => {
                    s.outline_style = BorderStyle::Dotted;
                }
                "double" => {
                    s.outline_style = BorderStyle::Double;
                }
                "inset" => {
                    s.outline_style = BorderStyle::Inset;
                }
                "outset" => {
                    s.outline_style = BorderStyle::Outset;
                }
                "groove" => {
                    s.outline_style = BorderStyle::Groove;
                }
                "ridge" => {
                    s.outline_style = BorderStyle::Ridge;
                }
                "none" => {
                    s.outline_style = BorderStyle::None;
                }
                _ => {
                    if let CssLength::Px(w) = parse_length(tok) {
                        s.outline_width = w;
                    } else if let Some(c) = parse_color(tok) {
                        s.outline_color = c;
                    }
                }
            }
        }
    }
}
fn apply_outline_style(s: &mut ComputedStyle, v: &str) {
    s.outline_style = super::parse_border_style(v);
}
fn apply_outline_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.outline_color = c;
    }
}
fn apply_outline_width(s: &mut ComputedStyle, v: &str) {
    if let CssLength::Px(w) = parse_length(v) {
        s.outline_width = w;
    }
}
fn apply_outline_offset(s: &mut ComputedStyle, v: &str) {
    if let CssLength::Px(w) = parse_length(v) {
        s.outline_offset = w;
    }
}

fn copy_outline_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.outline_style = s.outline_style;
}
fn copy_outline_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.outline_color = s.outline_color;
}
fn copy_outline_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.outline_width = s.outline_width;
}
fn copy_outline_offset(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.outline_offset = s.outline_offset;
}

// ── Box shadow ──────────────────────────────────────────────────────────────

fn apply_box_shadow(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.box_shadow.clear();
    } else {
        s.box_shadow = super::split_top_level_commas(v)
            .into_iter()
            .filter_map(|layer| {
                let layer = layer.trim();
                if layer.is_empty() || layer.eq_ignore_ascii_case("none") {
                    return None;
                }
                let (ox, oy, blur, color) = super::parse_shadow_value(layer);
                let toks: Vec<&str> = layer.split_whitespace().collect();
                let nums: Vec<f32> = toks
                    .iter()
                    .filter_map(|t| {
                        let c = t.trim_start_matches('-').chars().next()?;
                        if c.is_ascii_digit() || c == '.' {
                            t.trim_end_matches("px").parse().ok()
                        } else {
                            None
                        }
                    })
                    .collect();
                let spread = nums.get(3).copied().unwrap_or(0.0);
                let inset = toks.iter().any(|t| t.eq_ignore_ascii_case("inset"));
                Some(BoxShadow {
                    offset_x: ox,
                    offset_y: oy,
                    blur,
                    spread,
                    color,
                    inset,
                })
            })
            .collect();
    }
}
fn copy_box_shadow(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.box_shadow = s.box_shadow.clone();
}

// ── Pointer events ──────────────────────────────────────────────────────────

fn apply_pointer_events(s: &mut ComputedStyle, v: &str) {
    s.pointer_events = match v {
        "none" => PointerEvents::None,
        "visiblePainted" => PointerEvents::VisiblePainted,
        "visibleFill" => PointerEvents::VisibleFill,
        "visibleStroke" => PointerEvents::VisibleStroke,
        "visible" => PointerEvents::Visible,
        "painted" => PointerEvents::Painted,
        "fill" => PointerEvents::Fill,
        "stroke" => PointerEvents::Stroke,
        "all" => PointerEvents::All,
        _ => PointerEvents::Auto,
    };
}
fn copy_pointer_events(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.pointer_events = s.pointer_events;
}

// ── User interaction ────────────────────────────────────────────────────────

fn apply_user_select(s: &mut ComputedStyle, v: &str) {
    s.user_select = match v {
        "none" => UserSelect::None,
        "text" => UserSelect::Text,
        "all" => UserSelect::All,
        "contain" => UserSelect::Contain,
        _ => UserSelect::Auto,
    };
}
fn apply_resize(s: &mut ComputedStyle, v: &str) {
    s.resize = match v {
        "both" => Resize::Both,
        "horizontal" => Resize::Horizontal,
        "vertical" => Resize::Vertical,
        _ => Resize::None,
    };
}

fn copy_user_select(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.user_select = s.user_select;
}
fn copy_resize(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.resize = s.resize;
}

// ── Object fit/position ─────────────────────────────────────────────────────

fn apply_object_fit(s: &mut ComputedStyle, v: &str) {
    s.object_fit = match v.to_ascii_lowercase().as_str() {
        "contain" => ObjectFit::Contain,
        "cover" => ObjectFit::Cover,
        "none" => ObjectFit::None,
        "scale-down" => ObjectFit::ScaleDown,
        _ => ObjectFit::Fill,
    };
}
fn apply_object_position(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.split_whitespace().collect();
    let (x, y) = parse_object_position_tokens(&parts);
    s.object_position_x = x;
    s.object_position_y = y;
}

fn parse_object_position_tokens(parts: &[&str]) -> (CssLength, CssLength) {
    match parts {
        [] => (CssLength::Percent(50.0), CssLength::Percent(50.0)),
        [one] => match position_axis(*one) {
            Some(PositionAxis::Horizontal(x)) => (x, CssLength::Percent(50.0)),
            Some(PositionAxis::Vertical(y)) => (CssLength::Percent(50.0), y),
            Some(PositionAxis::Center) => (CssLength::Percent(50.0), CssLength::Percent(50.0)),
            None => (parse_length(one), CssLength::Percent(50.0)),
        },
        [first, second] => {
            let a = position_axis(first);
            let b = position_axis(second);
            match (a, b) {
                (Some(PositionAxis::Horizontal(x)), Some(PositionAxis::Vertical(y)))
                | (Some(PositionAxis::Vertical(y)), Some(PositionAxis::Horizontal(x))) => (x, y),
                (Some(PositionAxis::Horizontal(x)), Some(PositionAxis::Center)) => {
                    (x, CssLength::Percent(50.0))
                }
                (Some(PositionAxis::Center), Some(PositionAxis::Horizontal(x))) => {
                    (x, CssLength::Percent(50.0))
                }
                (Some(PositionAxis::Vertical(y)), Some(PositionAxis::Center)) => {
                    (CssLength::Percent(50.0), y)
                }
                (Some(PositionAxis::Center), Some(PositionAxis::Vertical(y))) => {
                    (CssLength::Percent(50.0), y)
                }
                (Some(PositionAxis::Horizontal(x)), None) => (x, parse_length(second)),
                (Some(PositionAxis::Vertical(y)), None) => (parse_length(second), y),
                (None, Some(PositionAxis::Horizontal(x))) => (x, parse_length(first)),
                (None, Some(PositionAxis::Vertical(y))) => (parse_length(first), y),
                _ => (parse_length(first), parse_length(second)),
            }
        }
        [a, b, c] | [a, b, c, ..] => {
            if let Some((x, consumed)) = edge_position_component(parts, true) {
                if let Some((y, _)) = edge_position_component(&parts[consumed..], false) {
                    return (x, y);
                }
            }
            if let Some((y, consumed)) = edge_position_component(parts, false) {
                if let Some((x, _)) = edge_position_component(&parts[consumed..], true) {
                    return (x, y);
                }
            }
            parse_object_position_tokens(&[*a, *b, *c][..2])
        }
    }
}

#[derive(Clone)]
enum PositionAxis {
    Horizontal(CssLength),
    Vertical(CssLength),
    Center,
}

fn position_axis(token: &str) -> Option<PositionAxis> {
    match token.to_ascii_lowercase().as_str() {
        "left" => Some(PositionAxis::Horizontal(CssLength::Percent(0.0))),
        "right" => Some(PositionAxis::Horizontal(CssLength::Percent(100.0))),
        "top" => Some(PositionAxis::Vertical(CssLength::Percent(0.0))),
        "bottom" => Some(PositionAxis::Vertical(CssLength::Percent(100.0))),
        "center" => Some(PositionAxis::Center),
        _ => None,
    }
}

fn edge_position_component(parts: &[&str], horizontal: bool) -> Option<(CssLength, usize)> {
    let edge = parts.first()?.to_ascii_lowercase();
    let is_start = if horizontal {
        edge == "left"
    } else {
        edge == "top"
    };
    let is_end = if horizontal {
        edge == "right"
    } else {
        edge == "bottom"
    };
    if !is_start && !is_end {
        return None;
    }

    let offset = parts.get(1).copied();
    let value = match (is_start, offset) {
        (_, None) => {
            if is_start {
                CssLength::Percent(0.0)
            } else {
                CssLength::Percent(100.0)
            }
        }
        (true, Some(offset)) => parse_length(offset),
        (false, Some(offset)) => parse_length(&format!("calc(100% - {offset})")),
    };
    Some((value, if offset.is_some() { 2 } else { 1 }))
}
fn apply_aspect_ratio(s: &mut ComputedStyle, v: &str) {
    // css-sizing-4 §4: the grammar is `auto || <ratio>`, so the `auto` keyword
    // may sit BESIDE a ratio — `aspect-ratio: auto 16/9` — where it means
    // "prefer a replaced element's own intrinsic ratio, and use this one
    // otherwise". Matching the whole value against `"auto"` left `"auto 16"`
    // to be read as a number, which failed and fell back to 1: 1/9, not 16/9.
    let rest = v
        .split_whitespace()
        .filter(|t| !t.eq_ignore_ascii_case("auto"))
        .collect::<Vec<_>>()
        .join(" ");
    let rest = rest.trim();
    if rest.is_empty() {
        s.aspect_ratio = None;
        return;
    }
    if let Some(slash) = rest.find('/') {
        let w: f32 = rest[..slash].trim().parse().unwrap_or(1.0);
        let h: f32 = rest[slash + 1..].trim().parse().unwrap_or(1.0);
        if h > 0.0 {
            s.aspect_ratio = Some(w / h);
        }
    } else if let Ok(n) = rest.parse::<f32>() {
        if n > 0.0 {
            s.aspect_ratio = Some(n);
        }
    }
}

fn copy_object_fit(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.object_fit = s.object_fit;
}
fn copy_object_position(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.object_position_x = s.object_position_x.clone();
    d.object_position_y = s.object_position_y.clone();
}
fn copy_aspect_ratio(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.aspect_ratio = s.aspect_ratio;
}

// ── Transform / filter ──────────────────────────────────────────────────────

fn apply_transform(s: &mut ComputedStyle, v: &str) {
    if let Some(transform) = super::parse_css_transform_checked(v) {
        s.transform = if v.trim().eq_ignore_ascii_case("none") {
            String::new()
        } else {
            v.to_string()
        };
        s.css_transform = transform;
    }
}
fn apply_individual_translate(s: &mut ComputedStyle, v: &str) {
    if let Some(transform) = super::parse_individual_translate(v) {
        s.css_translate = transform;
    }
}
fn apply_individual_rotate(s: &mut ComputedStyle, v: &str) {
    if let Some(transform) = super::parse_individual_rotate(v) {
        s.css_rotate = transform;
    }
}
fn apply_individual_scale(s: &mut ComputedStyle, v: &str) {
    if let Some(transform) = super::parse_individual_scale(v) {
        s.css_scale = transform;
    }
}
fn apply_transform_box(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.transform_box,
        v,
        &[
            "content-box",
            "border-box",
            "fill-box",
            "stroke-box",
            "view-box",
        ],
    );
}
fn apply_transform_origin(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().transform_origin = Some(super::parse_transform_origin(v));
}
fn apply_transform_style_3d(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.transform_style_3d, v, &["flat", "preserve-3d"]);
}
fn apply_perspective(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none" || parse_length_checked(value).is_some() {
        s.perspective = value.to_string();
    }
}
fn is_perspective_origin_component(v: &str) -> bool {
    matches!(v, "left" | "center" | "right" | "top" | "bottom") || parse_length_checked(v).is_some()
}
fn apply_perspective_origin(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    let parts: Vec<&str> = value.split_whitespace().collect();
    if !parts.is_empty()
        && parts.len() <= 2
        && parts
            .iter()
            .all(|part| is_perspective_origin_component(part))
    {
        s.perspective_origin = value.to_string();
    }
}
fn apply_backface_visibility(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.backface_visibility, v, &["visible", "hidden"]);
}
fn apply_filter(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().filter = v.to_string();
    s.css_filter = super::parse_css_filter_with_current_color(v, s.color);
}
fn apply_backdrop_filter(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().backdrop_filter = v.to_string();
}

fn copy_transform(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.transform = s.transform.clone();
    d.css_transform = s.css_transform.clone();
}
fn copy_individual_translate(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.css_translate = s.css_translate.clone();
}
fn copy_individual_rotate(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.css_rotate = s.css_rotate.clone();
}
fn copy_individual_scale(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.css_scale = s.css_scale.clone();
}
fn copy_transform_box(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.transform_box = s.transform_box.clone();
}
fn copy_transform_origin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().transform_origin = s.rare().transform_origin.clone();
}
fn copy_transform_style_3d(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.transform_style_3d = s.transform_style_3d.clone();
}
fn copy_perspective(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.perspective = s.perspective.clone();
}
fn copy_perspective_origin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.perspective_origin = s.perspective_origin.clone();
}
fn copy_backface_visibility(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.backface_visibility = s.backface_visibility.clone();
}
fn copy_filter(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().filter = s.rare().filter.clone();
    d.css_filter = s.css_filter.clone();
}
fn copy_backdrop_filter(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().backdrop_filter = s.rare().backdrop_filter.clone();
}

// ── Transition / animation ──────────────────────────────────────────────────

fn apply_transition(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().transitions = super::parse_transition_shorthand(v);
}
fn ensure_transition(s: &mut ComputedStyle) {
    if s.rare().transitions.is_empty() {
        s.rare_mut().transitions.push(ParsedTransition {
            property: "all".to_string(),
            duration_ms: 0.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Ease,
            allow_discrete: false,
        });
    }
}

fn default_transition() -> ParsedTransition {
    ParsedTransition {
        property: "all".to_string(),
        duration_ms: 0.0,
        delay_ms: 0.0,
        timing_fn: EasingFn::Ease,
        allow_discrete: false,
    }
}

fn transition_list_values(v: &str) -> Vec<&str> {
    super::split_top_level_commas(v)
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn resize_transitions(s: &mut ComputedStyle, len: usize) {
    let len = len.max(1);
    let transitions = &mut s.rare_mut().transitions;
    if transitions.is_empty() {
        transitions.push(default_transition());
    }
    while transitions.len() < len {
        let next = transitions
            .last()
            .cloned()
            .unwrap_or_else(default_transition);
        transitions.push(next);
    }
    transitions.truncate(len);
}

fn apply_transition_property(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values
        .iter()
        .any(|value| value.eq_ignore_ascii_case("none"))
    {
        s.rare_mut().transitions.clear();
        return;
    }
    resize_transitions(s, values.len());
    for (idx, tr) in s.rare_mut().transitions.iter_mut().enumerate() {
        tr.property = values.get(idx).copied().unwrap_or("all").to_string();
    }
}
fn apply_transition_duration(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_transition(s);
        return;
    }
    resize_transitions(s, s.rare().transitions.len().max(values.len()));
    for (idx, tr) in s.rare_mut().transitions.iter_mut().enumerate() {
        let value = values[idx % values.len()];
        tr.duration_ms = super::parse_time_ms(value).unwrap_or(0.0);
    }
}
fn apply_transition_timing_function(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_transition(s);
        return;
    }
    resize_transitions(s, s.rare().transitions.len().max(values.len()));
    for (idx, tr) in s.rare_mut().transitions.iter_mut().enumerate() {
        tr.timing_fn = super::parse_easing(values[idx % values.len()]);
    }
}
fn apply_transition_delay(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_transition(s);
        return;
    }
    resize_transitions(s, s.rare().transitions.len().max(values.len()));
    for (idx, tr) in s.rare_mut().transitions.iter_mut().enumerate() {
        tr.delay_ms = super::parse_time_ms(values[idx % values.len()]).unwrap_or(0.0);
    }
}
fn apply_transition_behavior(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_transition(s);
        return;
    }
    resize_transitions(s, s.rare().transitions.len().max(values.len()));
    for (idx, tr) in s.rare_mut().transitions.iter_mut().enumerate() {
        tr.allow_discrete = values[idx % values.len()] == "allow-discrete";
    }
}

fn apply_animation(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().animations = super::parse_animation_shorthand(v);
}
fn ensure_animation(s: &mut ComputedStyle) {
    if s.rare().animations.is_empty() {
        s.rare_mut().animations.push(default_animation());
    }
}

fn default_animation() -> ParsedAnimation {
    ParsedAnimation {
        name: String::new(),
        duration_ms: 0.0,
        delay_ms: 0.0,
        timing_fn: EasingFn::Ease,
        iteration_count: 1.0,
        direction: AnimDirection::Normal,
        fill_mode: FillMode::None,
        play_state_paused: false,
        composition: AnimationComposition::Replace,
    }
}

fn resize_animations(s: &mut ComputedStyle, len: usize) {
    let len = len.max(1);
    let animations = &mut s.rare_mut().animations;
    if animations.is_empty() {
        animations.push(default_animation());
    }
    while animations.len() < len {
        let next = animations.last().cloned().unwrap_or_else(default_animation);
        animations.push(next);
    }
    animations.truncate(len);
}

fn parse_animation_iteration_count(value: &str) -> f32 {
    if value.trim() == "infinite" {
        f32::INFINITY
    } else {
        value.trim().parse().unwrap_or(1.0)
    }
}

fn parse_animation_direction(value: &str) -> AnimDirection {
    match value.trim() {
        "reverse" => AnimDirection::Reverse,
        "alternate" => AnimDirection::Alternate,
        "alternate-reverse" => AnimDirection::AlternateReverse,
        _ => AnimDirection::Normal,
    }
}

fn parse_animation_fill_mode(value: &str) -> FillMode {
    match value.trim() {
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => FillMode::None,
    }
}

fn parse_animation_composition(value: &str) -> AnimationComposition {
    match value.trim() {
        "add" => AnimationComposition::Add,
        "accumulate" => AnimationComposition::Accumulate,
        _ => AnimationComposition::Replace,
    }
}

fn apply_animation_name(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    resize_animations(s, values.len());
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.name = values.get(idx).copied().unwrap_or("").to_string();
    }
}
fn apply_animation_duration(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.duration_ms = super::parse_time_ms(values[idx % values.len()]).unwrap_or(0.0);
    }
}
fn apply_animation_timing_function(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.timing_fn = super::parse_easing(values[idx % values.len()]);
    }
}
fn apply_animation_delay(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.delay_ms = super::parse_time_ms(values[idx % values.len()]).unwrap_or(0.0);
    }
}
fn apply_animation_iteration_count(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.iteration_count = parse_animation_iteration_count(values[idx % values.len()]);
    }
}
fn apply_animation_direction(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.direction = parse_animation_direction(values[idx % values.len()]);
    }
}
fn apply_animation_fill_mode(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.fill_mode = parse_animation_fill_mode(values[idx % values.len()]);
    }
}
fn apply_animation_play_state(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.play_state_paused = values[idx % values.len()] == "paused";
    }
}
fn apply_animation_composition(s: &mut ComputedStyle, v: &str) {
    let values = transition_list_values(v);
    if values.is_empty() {
        ensure_animation(s);
        return;
    }
    resize_animations(s, s.rare().animations.len().max(values.len()));
    for (idx, anim) in s.rare_mut().animations.iter_mut().enumerate() {
        anim.composition = parse_animation_composition(values[idx % values.len()]);
    }
}
fn apply_will_change(s: &mut ComputedStyle, v: &str) {
    s.will_change = v.to_string();
    s.will_change_transform = v.contains("transform");
}

fn copy_transition(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().transitions = s.rare().transitions.clone();
}
fn copy_animation(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().animations = s.rare().animations.clone();
}
fn copy_will_change(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.will_change = s.will_change.clone();
    d.will_change_transform = s.will_change_transform;
}

// ── Unicode-bidi & writing ──────────────────────────────────────────────────

fn apply_unicode_bidi(s: &mut ComputedStyle, v: &str) {
    s.unicode_bidi = match v {
        "normal" => UnicodeBidi::Normal,
        "embed" => UnicodeBidi::Embed,
        "bidi-override" => UnicodeBidi::Override,
        "isolate" => UnicodeBidi::Isolate,
        "isolate-override" => UnicodeBidi::IsolateOverride,
        "plaintext" => UnicodeBidi::Plaintext,
        _ => UnicodeBidi::Normal,
    };
}
fn apply_writing_mode(s: &mut ComputedStyle, v: &str) {
    s.writing_mode = match v {
        "vertical-rl" => WritingMode::VerticalRL,
        "vertical-lr" => WritingMode::VerticalLR,
        "sideways-rl" => WritingMode::SidewaysRL,
        "sideways-lr" => WritingMode::SidewaysLR,
        _ => WritingMode::HorizontalTB,
    };
}
fn apply_text_orientation(s: &mut ComputedStyle, v: &str) {
    s.text_orientation = match v {
        "mixed" => TextOrientation::Mixed,
        "upright" => TextOrientation::Upright,
        "sideways" => TextOrientation::Sideways,
        _ => return,
    };
}
fn apply_text_combine_upright(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none" || value == "all" || value == "digits" {
        s.text_combine_upright = value.to_string();
        return;
    }
    if let Some(rest) = value.strip_prefix("digits ") {
        if rest.parse::<u8>().is_ok_and(|n| (2..=4).contains(&n)) {
            s.text_combine_upright = value.to_string();
        }
    }
}

fn copy_unicode_bidi(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.unicode_bidi = s.unicode_bidi;
}
fn copy_writing_mode(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.writing_mode = s.writing_mode;
}
fn copy_text_orientation(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_orientation = s.text_orientation;
}
fn copy_text_combine_upright(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.text_combine_upright = s.text_combine_upright.clone();
}

// ── Hyphens / tab-size / text extras ────────────────────────────────────────

fn apply_tab_size(s: &mut ComputedStyle, v: &str) {
    s.tab_size = v.parse().unwrap_or(8);
}
fn apply_hyphens(s: &mut ComputedStyle, v: &str) {
    s.hyphens = match v {
        "none" => Hyphens::None,
        "manual" => Hyphens::Manual,
        "auto" => Hyphens::Auto,
        _ => Hyphens::Manual,
    };
}
fn apply_widows(s: &mut ComputedStyle, v: &str) {
    if let Ok(n) = v.parse() {
        s.widows = n;
    }
}
fn apply_orphans(s: &mut ComputedStyle, v: &str) {
    if let Ok(n) = v.parse() {
        s.orphans = n;
    }
}

fn copy_tab_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.tab_size = s.tab_size;
}
fn copy_hyphens(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.hyphens = s.hyphens;
}
fn copy_widows(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.widows = s.widows;
}
fn copy_orphans(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.orphans = s.orphans;
}

// ── Scrollbar & caret ───────────────────────────────────────────────────────

fn apply_scrollbar_color(s: &mut ComputedStyle, v: &str) {
    if v == "auto" {
        s.scrollbar_thumb_color = None;
        s.scrollbar_track_color = None;
        return;
    }
    if let Some(idx) = super::find_split_space(v) {
        let thumb = v[..idx].trim();
        let track = v[idx + 1..].trim();
        if let (Some(thumb), Some(track)) = (parse_color(thumb), parse_color(track)) {
            s.scrollbar_thumb_color = Some(thumb);
            s.scrollbar_track_color = Some(track);
        }
    }
}
fn apply_scrollbar_width(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.scrollbar_width, v, &["auto", "thin", "none"]);
}
fn apply_scrollbar_gutter(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.scrollbar_gutter,
        v,
        &["auto", "stable", "both-edges"],
    );
}
fn apply_caret_color(s: &mut ComputedStyle, v: &str) {
    s.caret_color = if v == "auto" { None } else { parse_color(v) };
}

fn copy_scrollbar_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scrollbar_thumb_color = s.scrollbar_thumb_color;
    d.scrollbar_track_color = s.scrollbar_track_color;
}
fn copy_scrollbar_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scrollbar_width = s.scrollbar_width.clone();
}
fn copy_scrollbar_gutter(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scrollbar_gutter = s.scrollbar_gutter.clone();
}
fn copy_caret_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.caret_color = s.caret_color;
}

// ── Quotes ──────────────────────────────────────────────────────────────────

fn apply_quotes(s: &mut ComputedStyle, v: &str) {
    s.rare_mut().quotes.clear();
    if v != "none" && v != "auto" {
        let bytes = v.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let q = bytes[i];
                i += 1;
                let start = i;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
                s.rare_mut().quotes.push(v[start..i].to_string());
                if i < bytes.len() {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }
}

fn copy_quotes(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.rare_mut().quotes = s.rare().quotes.clone();
}

// ── Container queries ───────────────────────────────────────────────────────

fn apply_container_type(s: &mut ComputedStyle, v: &str) {
    s.container_type = match v {
        "size" => ContainerType::Size,
        "inline-size" => ContainerType::InlineSize,
        _ => ContainerType::Normal,
    };
}
fn apply_container_name(s: &mut ComputedStyle, v: &str) {
    s.container_name = v.to_string();
}
fn apply_container(s: &mut ComputedStyle, v: &str) {
    if let Some(slash) = v.find('/') {
        s.container_name = v[..slash].trim().to_string();
        apply_container_type(s, v[slash + 1..].trim());
    } else {
        match v {
            "size" | "inline-size" => apply_container_type(s, v),
            _ => s.container_name = v.to_string(),
        }
    }
}

fn copy_container_type(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.container_type = s.container_type;
}
fn copy_container_name(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.container_name = s.container_name.clone();
}

// ── Clip ────────────────────────────────────────────────────────────────────

fn apply_clip(s: &mut ComputedStyle, v: &str) {
    if v == "auto" || v == "none" {
        s.clip_rect = None;
    } else if let Some(inner) = v.strip_prefix("rect(").and_then(|sv| sv.strip_suffix(')')) {
        let parts: Vec<&str> = if inner.contains(',') {
            inner.split(',').map(|sv| sv.trim()).collect()
        } else {
            inner.split_whitespace().collect()
        };
        if parts.len() == 4 {
            let parse_val = |sv: &str| -> f32 {
                let sv = sv.trim();
                if sv == "auto" {
                    return f32::MAX;
                }
                if let Some(px) = sv.strip_suffix("px") {
                    return px.trim().parse().unwrap_or(0.0);
                }
                sv.parse().unwrap_or(0.0)
            };
            s.clip_rect = Some([
                parse_val(parts[0]),
                parse_val(parts[1]),
                parse_val(parts[2]),
                parse_val(parts[3]),
            ]);
        }
    }
}
fn apply_clip_path(s: &mut ComputedStyle, v: &str) {
    if v == "none" {
        s.clip_path = ClipPath::default();
    } else if v.starts_with("inset(") {
        let inner = v[6..v.len().saturating_sub(1)].trim();
        s.clip_path = ClipPath::default();
        s.clip_path.kind = ClipPathKind::Inset;
        let pts: Vec<&str> = inner.split_whitespace().collect();
        s.clip_path.inset_top = parse_length(pts.first().copied().unwrap_or("0"));
        s.clip_path.inset_right = parse_length(
            pts.get(1)
                .copied()
                .unwrap_or(pts.first().copied().unwrap_or("0")),
        );
        s.clip_path.inset_bottom = parse_length(
            pts.get(2)
                .copied()
                .unwrap_or(pts.first().copied().unwrap_or("0")),
        );
        s.clip_path.inset_left = parse_length(
            pts.get(3).copied().unwrap_or(
                pts.get(1)
                    .copied()
                    .unwrap_or(pts.first().copied().unwrap_or("0")),
            ),
        );
    } else if v.starts_with("circle(") {
        let inner = v[7..v.len().saturating_sub(1)].trim();
        s.clip_path = ClipPath::default();
        s.clip_path.kind = ClipPathKind::Circle;
        if let Some(at) = inner.find(" at ") {
            s.clip_path.circle_radius = parse_length(&inner[..at]);
            let center: Vec<&str> = inner[at + 4..].split_whitespace().collect();
            s.clip_path.center_x = parse_length(center.first().copied().unwrap_or("50%"));
            s.clip_path.center_y = parse_length(
                center
                    .get(1)
                    .copied()
                    .unwrap_or(center.first().copied().unwrap_or("50%")),
            );
        } else {
            s.clip_path.circle_radius = parse_length(inner);
            s.clip_path.center_x = CssLength::Percent(50.0);
            s.clip_path.center_y = CssLength::Percent(50.0);
        }
    } else if v.starts_with("ellipse(") {
        let inner = v[8..v.len().saturating_sub(1)].trim();
        s.clip_path = ClipPath::default();
        s.clip_path.kind = ClipPathKind::Ellipse;
        let (radii, center) = if let Some(at) = inner.find(" at ") {
            (&inner[..at], Some(&inner[at + 4..]))
        } else {
            (inner, None)
        };
        let rv: Vec<&str> = radii.split_whitespace().collect();
        s.clip_path.ellipse_rx = parse_length(rv.first().copied().unwrap_or("50%"));
        s.clip_path.ellipse_ry = parse_length(
            rv.get(1)
                .copied()
                .unwrap_or(rv.first().copied().unwrap_or("50%")),
        );
        if let Some(c) = center {
            let cv: Vec<&str> = c.split_whitespace().collect();
            s.clip_path.center_x = parse_length(cv.first().copied().unwrap_or("50%"));
            s.clip_path.center_y = parse_length(
                cv.get(1)
                    .copied()
                    .unwrap_or(cv.first().copied().unwrap_or("50%")),
            );
        } else {
            s.clip_path.center_x = CssLength::Percent(50.0);
            s.clip_path.center_y = CssLength::Percent(50.0);
        }
    } else if v.starts_with("polygon(") {
        let inner = v[8..v.len().saturating_sub(1)].trim();
        s.clip_path = ClipPath::default();
        s.clip_path.kind = ClipPathKind::Polygon;
        for pair in inner.split(',') {
            let pts: Vec<&str> = pair.trim().split_whitespace().collect();
            if pts.len() >= 2 {
                s.clip_path
                    .points
                    .push((parse_length(pts[0]), parse_length(pts[1])));
            }
        }
    }
}

fn copy_clip(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.clip_rect = s.clip_rect;
}
fn copy_clip_path(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.clip_path = s.clip_path.clone();
}

fn apply_shape_outside(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if !value.is_empty() {
        s.shape_outside = value.to_string();
    }
}

fn copy_shape_outside(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.shape_outside = s.shape_outside.clone();
}

fn apply_shape_margin(s: &mut ComputedStyle, v: &str) {
    s.shape_margin = parse_length(v);
}

fn copy_shape_margin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.shape_margin = s.shape_margin.clone();
}

// ── Break / page-break ──────────────────────────────────────────────────────

fn apply_break_before(s: &mut ComputedStyle, v: &str) {
    s.break_before = match v {
        "always" | "page" => BreakValue::Always,
        "column" => BreakValue::Column,
        "avoid" => BreakValue::Avoid,
        "left" => BreakValue::Left,
        "right" => BreakValue::Right,
        _ => BreakValue::Auto,
    };
}
fn apply_break_after(s: &mut ComputedStyle, v: &str) {
    s.break_after = match v {
        "always" | "page" => BreakValue::Always,
        "column" => BreakValue::Column,
        "avoid" => BreakValue::Avoid,
        "left" => BreakValue::Left,
        "right" => BreakValue::Right,
        _ => BreakValue::Auto,
    };
}
fn apply_break_inside(s: &mut ComputedStyle, v: &str) {
    s.break_inside = if v == "avoid" {
        BreakInside::Avoid
    } else {
        BreakInside::Auto
    };
}
fn apply_line_clamp(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "none" {
        s.line_clamp = None;
    } else if let Ok(lines) = value.parse::<u32>() {
        if lines > 0 {
            s.line_clamp = Some(lines);
        }
    }
}

fn copy_break_before(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.break_before = s.break_before;
}
fn copy_break_after(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.break_after = s.break_after;
}
fn copy_break_inside(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.break_inside = s.break_inside;
}
fn copy_line_clamp(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.line_clamp = s.line_clamp;
}

// ── Multi-column ────────────────────────────────────────────────────────────

fn apply_column_count(s: &mut ComputedStyle, v: &str) {
    s.column_count = if v == "auto" { None } else { v.parse().ok() };
}
fn apply_column_width(s: &mut ComputedStyle, v: &str) {
    s.column_width = parse_length(v);
}
fn apply_columns(s: &mut ComputedStyle, v: &str) {
    for tok in v.split_whitespace() {
        if let Ok(n) = tok.parse::<i32>() {
            s.column_count = Some(n);
        } else {
            s.column_width = parse_length(tok);
        }
    }
}
fn apply_column_rule(s: &mut ComputedStyle, v: &str) {
    super::apply_border_side_shorthand(
        v,
        &mut s.column_rule_width,
        &mut s.column_rule_style,
        &mut s.column_rule_color,
    );
}
fn apply_column_rule_width(s: &mut ComputedStyle, v: &str) {
    s.column_rule_width = parse_length(v);
}
fn apply_column_rule_style(s: &mut ComputedStyle, v: &str) {
    s.column_rule_style = super::parse_border_style(v);
}
fn apply_column_rule_color(s: &mut ComputedStyle, v: &str) {
    if let Some(c) = parse_color(v) {
        s.column_rule_color = c;
    }
}
fn apply_column_fill(s: &mut ComputedStyle, v: &str) {
    s.column_fill = v == "balance";
}
fn apply_column_span(s: &mut ComputedStyle, v: &str) {
    s.column_span_all = v == "all";
}

fn copy_column_count(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_count = s.column_count;
}
fn copy_column_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_width = s.column_width.clone();
}
fn copy_column_rule_width(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_rule_width = s.column_rule_width.clone();
}
fn copy_column_rule_style(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_rule_style = s.column_rule_style;
}
fn copy_column_rule_color(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_rule_color = s.column_rule_color;
}
fn copy_column_fill(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_fill = s.column_fill;
}
fn copy_column_span(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.column_span_all = s.column_span_all;
}

// ── Counter ─────────────────────────────────────────────────────────────────

fn apply_counter_reset(s: &mut ComputedStyle, v: &str) {
    s.counter_reset = super::parse_counter_list_with_default(v, 0);
}
fn apply_counter_increment(s: &mut ComputedStyle, v: &str) {
    s.counter_increment = super::parse_counter_list(v);
}
fn apply_counter_set(s: &mut ComputedStyle, v: &str) {
    s.counter_set = super::parse_counter_list_with_default(v, 0);
}

fn copy_counter_reset(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.counter_reset = s.counter_reset.clone();
}
fn copy_counter_increment(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.counter_increment = s.counter_increment.clone();
}
fn copy_counter_set(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.counter_set = s.counter_set.clone();
}

// ── Misc ────────────────────────────────────────────────────────────────────

fn apply_scroll_behavior(s: &mut ComputedStyle, v: &str) {
    s.scroll_behavior = if v == "smooth" {
        ScrollBehavior::Smooth
    } else {
        ScrollBehavior::Auto
    };
}
fn apply_overflow_anchor(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.overflow_anchor, v, &["auto", "none"]);
}
fn apply_overflow_clip_margin(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "content-box" || value == "padding-box" || value == "border-box" {
        s.overflow_clip_margin = value.to_string();
        return;
    }

    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() <= 2
        && !parts.is_empty()
        && parts.iter().all(|part| {
            *part == "content-box"
                || *part == "padding-box"
                || *part == "border-box"
                || parse_length_checked(part).is_some()
        })
        && parts
            .iter()
            .any(|part| parse_length_checked(part).is_some())
    {
        s.overflow_clip_margin = value.to_string();
    }
}
fn apply_overscroll_behavior(s: &mut ComputedStyle, v: &str) {
    let val = super::parse_overscroll(v.split_whitespace().next().unwrap_or("auto"));
    s.overscroll_behavior_x = val;
    s.overscroll_behavior_y = val;
}
fn apply_overscroll_behavior_x(s: &mut ComputedStyle, v: &str) {
    s.overscroll_behavior_x = super::parse_overscroll(v);
}
fn apply_overscroll_behavior_y(s: &mut ComputedStyle, v: &str) {
    s.overscroll_behavior_y = super::parse_overscroll(v);
}
fn apply_isolation(s: &mut ComputedStyle, v: &str) {
    s.isolation = v == "isolate";
}
fn apply_mix_blend_mode(s: &mut ComputedStyle, v: &str) {
    s.mix_blend_mode = match v {
        "multiply" => MixBlendMode::Multiply,
        "screen" => MixBlendMode::Screen,
        "overlay" => MixBlendMode::Overlay,
        "darken" => MixBlendMode::Darken,
        "lighten" => MixBlendMode::Lighten,
        "color-dodge" => MixBlendMode::ColorDodge,
        "color-burn" => MixBlendMode::ColorBurn,
        "hard-light" => MixBlendMode::HardLight,
        "soft-light" => MixBlendMode::SoftLight,
        "difference" => MixBlendMode::Difference,
        "exclusion" => MixBlendMode::Exclusion,
        "hue" => MixBlendMode::Hue,
        "saturation" => MixBlendMode::Saturation,
        "color" => MixBlendMode::Color,
        "luminosity" => MixBlendMode::Luminosity,
        _ => MixBlendMode::Normal,
    };
}
fn apply_interpolate_size(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.interpolate_size,
        v,
        &["numeric-only", "allow-keywords"],
    );
}
fn apply_margin_trim(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.margin_trim,
        v,
        &[
            "none",
            "block",
            "block-start",
            "block-end",
            "inline",
            "inline-start",
            "inline-end",
        ],
    );
}
fn apply_custom_ident_or_none(field: &mut String, v: &str) {
    let value = v.trim();
    if value == "none"
        || (!value.is_empty()
            && value
                .split(',')
                .all(|part| part.trim().starts_with("--") || is_custom_ident(part.trim())))
    {
        *field = value.to_string();
    }
}
fn apply_raw_or_keyword(field: &mut String, v: &str, allowed_keywords: &[&str]) {
    let value = v.trim();
    if value.is_empty() {
        return;
    }
    if allowed_keywords.iter().any(|kw| *kw == value)
        || value.contains('(')
        || value.starts_with("--")
        || is_custom_ident(value)
    {
        *field = value.to_string();
    }
}
fn is_custom_ident(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic() || first == '-')
        && !matches!(
            value,
            "initial" | "inherit" | "unset" | "revert" | "revert-layer"
        )
}
fn apply_anchor_name(s: &mut ComputedStyle, v: &str) {
    apply_custom_ident_or_none(&mut s.anchor_name, v);
}
fn apply_position_anchor(s: &mut ComputedStyle, v: &str) {
    apply_raw_or_keyword(&mut s.position_anchor, v, &["auto"]);
}
fn apply_view_transition_name(s: &mut ComputedStyle, v: &str) {
    apply_custom_ident_or_none(&mut s.view_transition_name, v);
}
fn apply_animation_timeline(s: &mut ComputedStyle, v: &str) {
    apply_raw_or_keyword(&mut s.animation_timeline, v, &["auto", "none"]);
}
fn apply_scroll_timeline(s: &mut ComputedStyle, v: &str) {
    apply_raw_or_keyword(&mut s.scroll_timeline, v, &["none"]);
}
fn apply_offset_path(s: &mut ComputedStyle, v: &str) {
    apply_raw_or_keyword(&mut s.offset_path, v, &["none"]);
}
fn apply_field_sizing(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.field_sizing, v, &["fixed", "content"]);
}
fn apply_appearance(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(
        &mut s.appearance,
        v,
        &[
            "auto",
            "none",
            "textfield",
            "menulist-button",
            "button",
            "checkbox",
            "radio",
            "searchfield",
            "textarea",
            "meter",
            "progress-bar",
        ],
    );
}

fn copy_scroll_behavior(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_behavior = s.scroll_behavior;
}
fn copy_overflow_anchor(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overflow_anchor = s.overflow_anchor.clone();
}
fn copy_overflow_clip_margin(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overflow_clip_margin = s.overflow_clip_margin.clone();
}
fn copy_overscroll_behavior_x(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overscroll_behavior_x = s.overscroll_behavior_x;
}
fn copy_overscroll_behavior_y(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.overscroll_behavior_y = s.overscroll_behavior_y;
}
fn copy_isolation(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.isolation = s.isolation;
}
fn copy_mix_blend_mode(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.mix_blend_mode = s.mix_blend_mode;
}
fn copy_interpolate_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.interpolate_size = s.interpolate_size.clone();
}
fn copy_margin_trim(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.margin_trim = s.margin_trim.clone();
}
fn copy_anchor_name(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.anchor_name = s.anchor_name.clone();
}
fn copy_position_anchor(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.position_anchor = s.position_anchor.clone();
}
fn copy_view_transition_name(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.view_transition_name = s.view_transition_name.clone();
}
fn copy_animation_timeline(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.animation_timeline = s.animation_timeline.clone();
}
fn copy_scroll_timeline(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_timeline = s.scroll_timeline.clone();
}
fn copy_offset_path(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.offset_path = s.offset_path.clone();
}
fn copy_field_sizing(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.field_sizing = s.field_sizing.clone();
}
fn copy_appearance(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.appearance = s.appearance.clone();
}
fn copy_forced_color_adjust(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.forced_color_adjust = s.forced_color_adjust.clone();
}

fn apply_color_scheme(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if value == "normal"
        || value == "light"
        || value == "dark"
        || value == "light dark"
        || value == "dark light"
        || value == "only light"
        || value == "only dark"
    {
        s.color_scheme = value.to_string();
    }
}

fn copy_color_scheme(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.color_scheme = s.color_scheme.clone();
}

fn apply_forced_color_adjust(s: &mut ComputedStyle, v: &str) {
    let value = v.trim();
    if matches!(value, "auto" | "none" | "preserve-parent-color") {
        s.forced_color_adjust = value.to_string();
    }
}

// ── Containment ─────────────────────────────────────────────────────────────

fn apply_contain(s: &mut ComputedStyle, v: &str) {
    let is_strict = v == "strict";
    let is_content = v == "content";
    s.contain_layout = v.contains("layout") || is_strict || is_content;
    s.contain_paint = v.contains("paint") || is_strict || is_content;
    s.contain_size = v.contains("size") || is_strict;
}

fn copy_contain(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.contain_layout = s.contain_layout;
    d.contain_paint = s.contain_paint;
    d.contain_size = s.contain_size;
}

fn apply_content_visibility(s: &mut ComputedStyle, v: &str) {
    s.content_visibility = match v.trim() {
        "visible" => ContentVisibility::Visible,
        "auto" => ContentVisibility::Auto,
        "hidden" => ContentVisibility::Hidden,
        _ => return,
    };
}

fn copy_content_visibility(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.content_visibility = s.content_visibility;
}

fn apply_contain_intrinsic_size(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.split_whitespace().collect();
    let (width, height) = match parts.as_slice() {
        [] => return,
        ["none"] => (CssLength::None, CssLength::None),
        [one] => match parse_length_checked(one) {
            Some(length) => (length.clone(), length),
            None => return,
        },
        [first, second] => {
            let first = if *first == "none" {
                CssLength::None
            } else {
                match parse_length_checked(first) {
                    Some(length) => length,
                    None => return,
                }
            };
            let second = if *second == "none" {
                CssLength::None
            } else {
                match parse_length_checked(second) {
                    Some(length) => length,
                    None => return,
                }
            };
            (first, second)
        }
        _ => return,
    };
    s.contain_intrinsic_width = width;
    s.contain_intrinsic_height = height;
}

fn copy_contain_intrinsic_size(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.contain_intrinsic_width = s.contain_intrinsic_width.clone();
    d.contain_intrinsic_height = s.contain_intrinsic_height.clone();
}

// ── Scroll snap ─────────────────────────────────────────────────────────────

fn apply_scroll_snap_type(s: &mut ComputedStyle, v: &str) {
    let mut words = v.split_whitespace();
    let axis = match words.next().unwrap_or("none") {
        "x" => ScrollSnapAxis::X,
        "y" => ScrollSnapAxis::Y,
        "both" => ScrollSnapAxis::Both,
        "block" => ScrollSnapAxis::Block,
        "inline" => ScrollSnapAxis::Inline,
        _ => ScrollSnapAxis::None,
    };
    let mandatory = matches!(words.next().unwrap_or("proximity"), "mandatory");
    s.scroll_snap_type = ScrollSnapType { axis, mandatory };
}
fn apply_scroll_snap_align(s: &mut ComputedStyle, v: &str) {
    s.scroll_snap_align = match v.split_whitespace().next().unwrap_or("none") {
        "start" => ScrollSnapAlign::Start,
        "end" => ScrollSnapAlign::End,
        "center" => ScrollSnapAlign::Center,
        _ => ScrollSnapAlign::None,
    };
}
fn apply_scroll_snap_stop(s: &mut ComputedStyle, v: &str) {
    apply_keyword_list(&mut s.scroll_snap_stop, v, &["normal", "always"]);
}
fn apply_scroll_padding(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.scroll_padding_top,
        &mut s.scroll_padding_right,
        &mut s.scroll_padding_bottom,
        &mut s.scroll_padding_left,
        parse_length,
    );
}
fn apply_scroll_padding_top(s: &mut ComputedStyle, v: &str) {
    s.scroll_padding_top = parse_length(v);
}
fn apply_scroll_padding_right(s: &mut ComputedStyle, v: &str) {
    s.scroll_padding_right = parse_length(v);
}
fn apply_scroll_padding_bottom(s: &mut ComputedStyle, v: &str) {
    s.scroll_padding_bottom = parse_length(v);
}
fn apply_scroll_padding_left(s: &mut ComputedStyle, v: &str) {
    s.scroll_padding_left = parse_length(v);
}
fn apply_scroll_margin(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.scroll_margin_top,
        &mut s.scroll_margin_right,
        &mut s.scroll_margin_bottom,
        &mut s.scroll_margin_left,
        parse_length,
    );
}
fn apply_scroll_margin_top(s: &mut ComputedStyle, v: &str) {
    s.scroll_margin_top = parse_length(v);
}
fn apply_scroll_margin_right(s: &mut ComputedStyle, v: &str) {
    s.scroll_margin_right = parse_length(v);
}
fn apply_scroll_margin_bottom(s: &mut ComputedStyle, v: &str) {
    s.scroll_margin_bottom = parse_length(v);
}
fn apply_scroll_margin_left(s: &mut ComputedStyle, v: &str) {
    s.scroll_margin_left = parse_length(v);
}

fn copy_scroll_snap_type(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_snap_type = s.scroll_snap_type;
}
fn copy_scroll_snap_align(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_snap_align = s.scroll_snap_align;
}
fn copy_scroll_snap_stop(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_snap_stop = s.scroll_snap_stop.clone();
}
fn copy_scroll_padding_top(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_padding_top = s.scroll_padding_top.clone();
}
fn copy_scroll_padding_right(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_padding_right = s.scroll_padding_right.clone();
}
fn copy_scroll_padding_bottom(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_padding_bottom = s.scroll_padding_bottom.clone();
}
fn copy_scroll_padding_left(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_padding_left = s.scroll_padding_left.clone();
}
fn copy_scroll_margin_top(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_margin_top = s.scroll_margin_top.clone();
}
fn copy_scroll_margin_right(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_margin_right = s.scroll_margin_right.clone();
}
fn copy_scroll_margin_bottom(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_margin_bottom = s.scroll_margin_bottom.clone();
}
fn copy_scroll_margin_left(d: &mut ComputedStyle, s: &ComputedStyle) {
    d.scroll_margin_left = s.scroll_margin_left.clone();
}

// ── Logical properties ──────────────────────────────────────────────────────

// ── Flow-relative box properties ────────────────────────────────────────────
//
// ⛔ RECORDED, not mapped. Which physical side `inline-start` names depends on
// the element's COMPUTED `direction` and `writing-mode`, so mapping here read
// whatever those held mid-cascade: `margin-inline-start` ignored `direction`
// outright (Tailwind's `ms-*`/`me-*` mirrored wrongly on every RTL page), and
// `inset-inline-start` answered differently depending on whether `direction`
// was declared above or below it. `finalize_logical` replays these in order,
// once, with the final values.
fn note_logical(s: &mut ComputedStyle, slot: LogicalSlot, v: &str) {
    let l = parse_length(v);
    s.rare_mut().logical_box.push((slot, l));
}
fn apply_margin_block(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginBlockStart, v);
    note_logical(s, LogicalSlot::MarginBlockEnd, v);
}
fn apply_margin_block_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginBlockStart, v);
}
fn apply_margin_block_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginBlockEnd, v);
}
fn apply_margin_inline(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginInlineStart, v);
    note_logical(s, LogicalSlot::MarginInlineEnd, v);
}
fn apply_margin_inline_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginInlineStart, v);
}
fn apply_margin_inline_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MarginInlineEnd, v);
}
fn apply_padding_block(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingBlockStart, v);
    note_logical(s, LogicalSlot::PaddingBlockEnd, v);
}
fn apply_padding_block_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingBlockStart, v);
}
fn apply_padding_block_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingBlockEnd, v);
}
fn apply_padding_inline(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingInlineStart, v);
    note_logical(s, LogicalSlot::PaddingInlineEnd, v);
}
fn apply_padding_inline_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingInlineStart, v);
}
fn apply_padding_inline_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::PaddingInlineEnd, v);
}

fn note_logical_border(
    s: &mut ComputedStyle,
    slot: LogicalBorderSlot,
    width: Option<CssLength>,
    style: Option<BorderStyle>,
    color: Option<Color>,
) {
    s.rare_mut().logical_borders.push(LogicalBorderValue {
        slot,
        width,
        style,
        color,
    });
}

fn note_logical_border_width(s: &mut ComputedStyle, slot: LogicalBorderSlot, v: &str) {
    note_logical_border(s, slot, Some(parse_length(v)), None, None);
}

fn note_logical_border_style(s: &mut ComputedStyle, slot: LogicalBorderSlot, v: &str) {
    note_logical_border(s, slot, None, Some(super::parse_border_style(v)), None);
}

fn note_logical_border_color(s: &mut ComputedStyle, slot: LogicalBorderSlot, v: &str) {
    if let Some(color) = parse_color(v) {
        note_logical_border(s, slot, None, None, Some(color));
    }
}

fn note_logical_border_shorthand(s: &mut ComputedStyle, slot: LogicalBorderSlot, v: &str) {
    let mut width = CssLength::Px(3.0);
    let mut style = BorderStyle::None;
    let mut color = Color::BLACK;
    super::apply_border_side_shorthand(v, &mut width, &mut style, &mut color);
    note_logical_border(s, slot, Some(width), Some(style), Some(color));
}

fn apply_border_inline_start(s: &mut ComputedStyle, v: &str) {
    note_logical_border_shorthand(s, LogicalBorderSlot::InlineStart, v);
}
fn apply_border_inline_end(s: &mut ComputedStyle, v: &str) {
    note_logical_border_shorthand(s, LogicalBorderSlot::InlineEnd, v);
}
fn apply_border_block_start(s: &mut ComputedStyle, v: &str) {
    note_logical_border_shorthand(s, LogicalBorderSlot::BlockStart, v);
}
fn apply_border_block_end(s: &mut ComputedStyle, v: &str) {
    note_logical_border_shorthand(s, LogicalBorderSlot::BlockEnd, v);
}
fn apply_border_inline(s: &mut ComputedStyle, v: &str) {
    apply_border_inline_start(s, v);
    apply_border_inline_end(s, v);
}
fn apply_border_block(s: &mut ComputedStyle, v: &str) {
    apply_border_block_start(s, v);
    apply_border_block_end(s, v);
}
fn apply_border_inline_start_width(s: &mut ComputedStyle, v: &str) {
    note_logical_border_width(s, LogicalBorderSlot::InlineStart, v);
}
fn apply_border_inline_end_width(s: &mut ComputedStyle, v: &str) {
    note_logical_border_width(s, LogicalBorderSlot::InlineEnd, v);
}
fn apply_border_block_start_width(s: &mut ComputedStyle, v: &str) {
    note_logical_border_width(s, LogicalBorderSlot::BlockStart, v);
}
fn apply_border_block_end_width(s: &mut ComputedStyle, v: &str) {
    note_logical_border_width(s, LogicalBorderSlot::BlockEnd, v);
}
fn apply_border_inline_start_style(s: &mut ComputedStyle, v: &str) {
    note_logical_border_style(s, LogicalBorderSlot::InlineStart, v);
}
fn apply_border_inline_end_style(s: &mut ComputedStyle, v: &str) {
    note_logical_border_style(s, LogicalBorderSlot::InlineEnd, v);
}
fn apply_border_block_start_style(s: &mut ComputedStyle, v: &str) {
    note_logical_border_style(s, LogicalBorderSlot::BlockStart, v);
}
fn apply_border_block_end_style(s: &mut ComputedStyle, v: &str) {
    note_logical_border_style(s, LogicalBorderSlot::BlockEnd, v);
}
fn apply_border_inline_start_color(s: &mut ComputedStyle, v: &str) {
    note_logical_border_color(s, LogicalBorderSlot::InlineStart, v);
}
fn apply_border_inline_end_color(s: &mut ComputedStyle, v: &str) {
    note_logical_border_color(s, LogicalBorderSlot::InlineEnd, v);
}
fn apply_border_block_start_color(s: &mut ComputedStyle, v: &str) {
    note_logical_border_color(s, LogicalBorderSlot::BlockStart, v);
}
fn apply_border_block_end_color(s: &mut ComputedStyle, v: &str) {
    note_logical_border_color(s, LogicalBorderSlot::BlockEnd, v);
}

fn apply_inset_block_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::InsetBlockStart, v);
}
fn apply_inset_block_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::InsetBlockEnd, v);
}
fn apply_inset_inline_start(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::InsetInlineStart, v);
}
fn apply_inset_inline_end(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::InsetInlineEnd, v);
}
fn apply_inline_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::InlineSize, v);
}
fn apply_block_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::BlockSize, v);
}
fn apply_min_inline_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MinInlineSize, v);
}
fn apply_min_block_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MinBlockSize, v);
}
fn apply_max_inline_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MaxInlineSize, v);
}
fn apply_max_block_size(s: &mut ComputedStyle, v: &str) {
    note_logical(s, LogicalSlot::MaxBlockSize, v);
}
fn apply_inset(s: &mut ComputedStyle, v: &str) {
    super::apply_shorthand_4(
        v,
        &mut s.top,
        &mut s.right,
        &mut s.bottom,
        &mut s.left,
        parse_length,
    );
}
fn apply_inset_block(s: &mut ComputedStyle, v: &str) {
    let l = parse_length(v);
    s.top = l.clone();
    s.bottom = l;
}
fn apply_inset_inline(s: &mut ComputedStyle, v: &str) {
    let l = parse_length(v);
    s.left = l.clone();
    s.right = l;
}

// ── Place shorthands ────────────────────────────────────────────────────────

fn apply_place_self(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.splitn(2, ' ').collect();
    apply_align_self(s, parts.first().copied().unwrap_or(v));
    apply_justify_self(s, parts.get(1).copied().unwrap_or(v));
}
fn apply_place_items(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.splitn(2, ' ').collect();
    apply_align_items(s, parts.first().copied().unwrap_or(v));
    apply_justify_items(s, parts.get(1).copied().unwrap_or(v));
}
fn apply_place_content(s: &mut ComputedStyle, v: &str) {
    let parts: Vec<&str> = v.splitn(2, ' ').collect();
    apply_align_content(s, parts.first().copied().unwrap_or(v));
    apply_justify_content(s, parts.get(1).copied().unwrap_or(v));
}

// ── Accent color ────────────────────────────────────────────────────────────

fn apply_accent_color(_s: &mut ComputedStyle, _v: &str) {
    // ⛔ DOES NOTHING, deliberately. `accent-color` tints a control's own
    // accent — a checkbox tick, a radio dot, a range thumb — and there is no
    // field for it yet. Assigning it into `background_color` painted a solid
    // block over the control and fought a real `background-color` in the same
    // rule, which is worse than not supporting the property at all.
}
