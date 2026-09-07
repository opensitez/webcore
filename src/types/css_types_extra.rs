//! Further CSS value types.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── New CSS types ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UserSelect {
    Auto,
    None,
    Text,
    All,
    Contain,
}
impl Default for UserSelect {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resize {
    None,
    Both,
    Horizontal,
    Vertical,
}
impl Default for Resize {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundClip {
    BorderBox,
    PaddingBox,
    ContentBox,
    Text,
}
impl Default for BackgroundClip {
    fn default() -> Self {
        Self::BorderBox
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundAttachment {
    Scroll,
    Fixed,
    Local,
}
impl Default for BackgroundAttachment {
    fn default() -> Self {
        Self::Scroll
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollBehavior {
    Auto,
    Smooth,
}
impl Default for ScrollBehavior {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextDecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}
impl Default for TextDecorationStyle {
    fn default() -> Self {
        Self::Solid
    }
}

/// Scroll-container axis for `scroll-snap-type`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ScrollSnapAxis {
    #[default]
    None,
    X,
    Y,
    Both,
    Block,
    Inline,
}

/// Combined snap-type carrying both axis and strictness.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ScrollSnapType {
    pub axis: ScrollSnapAxis,
    /// `true` = mandatory (always snap), `false` = proximity (snap only when close).
    pub mandatory: bool,
}

impl ScrollSnapType {
    pub fn none() -> Self {
        Self {
            axis: ScrollSnapAxis::None,
            mandatory: false,
        }
    }
    pub fn snaps_y(self) -> bool {
        matches!(
            self.axis,
            ScrollSnapAxis::Y | ScrollSnapAxis::Both | ScrollSnapAxis::Block
        )
    }
    pub fn snaps_x(self) -> bool {
        matches!(
            self.axis,
            ScrollSnapAxis::X | ScrollSnapAxis::Both | ScrollSnapAxis::Inline
        )
    }
    pub fn is_enabled(self) -> bool {
        self.axis != ScrollSnapAxis::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ScrollSnapAlign {
    #[default]
    None,
    Start,
    End,
    Center,
}

/// Controls scroll chaining when a scroll container reaches its boundary.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum OverscrollBehavior {
    /// Default: propagate scroll to the parent scroll container.
    #[default]
    Auto,
    /// Don't chain — swallow the scroll delta at the boundary.
    Contain,
    /// Same as Contain for chaining (no bounce effects either).
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MixBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
impl Default for MixBlendMode {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnicodeBidi {
    Normal,
    Embed,
    Override,
    Isolate,
    IsolateOverride,
    Plaintext,
}
impl Default for UnicodeBidi {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CSSCursor {
    Auto,
    Default,
    Pointer,
    Text,
    Move,
    Crosshair,
    Wait,
    Help,
    NotAllowed,
    Grab,
    Grabbing,
    Copy,
    Cell,
    ContextMenu,
    AllScroll,
    ZoomIn,
    ZoomOut,
    ColResize,
    RowResize,
    NResize,
    EResize,
    SResize,
    WResize,
    NEResize,
    NWResize,
    SEResize,
    SWResize,
    None,
}
impl Default for CSSCursor {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// `break-before`/`break-after` — css-break-3 §3.
///
/// `Column` is its own value, NOT a synonym for `Always`: `always` breaks at
/// the nearest fragmentation boundary of any kind, while `column` breaks only
/// a column. Both force a column break inside a multicol container, which is
/// the only fragmentation this engine performs.
pub enum BreakValue {
    Auto,
    Always,
    Avoid,
    Left,
    Right,
    Column,
}
impl Default for BreakValue {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BreakInside {
    Auto,
    Avoid,
}
impl Default for BreakInside {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WritingMode {
    HorizontalTB,
    VerticalRL,
    VerticalLR,
    SidewaysRL,
    SidewaysLR,
}
impl Default for WritingMode {
    fn default() -> Self {
        Self::HorizontalTB
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextOrientation {
    Mixed,
    Upright,
    Sideways,
}
impl Default for TextOrientation {
    fn default() -> Self {
        Self::Mixed
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CaptionSide {
    Top,
    Bottom,
    BlockStart,
    BlockEnd,
    InlineStart,
    InlineEnd,
}
impl Default for CaptionSide {
    fn default() -> Self {
        Self::Top
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hyphens {
    None,
    Manual,
    Auto,
}
impl Default for Hyphens {
    fn default() -> Self {
        Self::Manual
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvents {
    Auto,
    None,
    VisiblePainted,
    VisibleFill,
    VisibleStroke,
    Visible,
    Painted,
    Fill,
    Stroke,
    All,
}
impl Default for PointerEvents {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContainerType {
    Normal,
    Size,
    InlineSize,
}
impl Default for ContainerType {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClipPath {
    pub kind: ClipPathKind,
    // inset(top right bottom left)
    pub inset_top: CssLength,
    pub inset_right: CssLength,
    pub inset_bottom: CssLength,
    pub inset_left: CssLength,
    // circle(r at cx cy) / ellipse(rx ry at cx cy)
    pub circle_radius: CssLength,
    pub ellipse_rx: CssLength,
    pub ellipse_ry: CssLength,
    pub center_x: CssLength,
    pub center_y: CssLength,
    // polygon points
    pub points: Vec<(CssLength, CssLength)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ClipPathKind {
    #[default]
    None,
    Inset,
    Circle,
    Ellipse,
    Polygon,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        Self {
            rare: None,
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            z_index: 0,
            z_index_is_auto: true,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,

            box_sizing: BoxSizing::ContentBox,
            width: CssLength::Auto,
            height: CssLength::Auto,
            // `auto`, which is the CSS initial value. It computes to 0 in
            // normal flow, but on a flex item it means the content-based
            // minimum (Flexbox §4.5) — defaulting to `Zero` made that branch
            // dead, so a flex item would shrink to nothing.
            min_width: CssLength::Auto,
            max_width: CssLength::None,
            min_height: CssLength::Auto,
            max_height: CssLength::None,

            margin_top: CssLength::Zero,
            margin_right: CssLength::Zero,
            margin_bottom: CssLength::Zero,
            margin_left: CssLength::Zero,

            padding_top: CssLength::Zero,
            padding_right: CssLength::Zero,
            padding_bottom: CssLength::Zero,
            padding_left: CssLength::Zero,

            // ⛔ `medium`, which CSS Backgrounds 3 §4.3 pins at exactly 3px —
            // not 0. Safe because the USED width is zeroed whenever the
            // matching `border-*-style` is `none` (`layout/mod.rs`), which is
            // itself the initial value, so an element with no border declared
            // still occupies no border space.
            //
            // It matters for `border-style: solid` with no width given, and
            // for anything that resets a width to its initial value.
            border_top_width: CssLength::Px(3.0),
            border_right_width: CssLength::Px(3.0),
            border_bottom_width: CssLength::Px(3.0),
            border_left_width: CssLength::Px(3.0),

            border_top_style: BorderStyle::None,
            border_right_style: BorderStyle::None,
            border_bottom_style: BorderStyle::None,
            border_left_style: BorderStyle::None,

            border_top_color: Color::BLACK,
            border_right_color: Color::BLACK,
            border_bottom_color: Color::BLACK,
            border_left_color: Color::BLACK,

            border_radius: CssLength::Zero,

            top: CssLength::Auto,
            right: CssLength::Auto,
            bottom: CssLength::Auto,
            left: CssLength::Auto,

            color: Color::BLACK,
            background_color: Color::TRANSPARENT,
            svg_fill: Some(Color::BLACK),
            svg_stroke: None,
            font_family: String::from("sans-serif"),
            font_size: CssLength::Px(16.0),
            font_weight: FontWeight::Normal,
            relative_font_weight_base: None,
            font_style: FontStyle::Normal,
            line_height: CssLength::Em(1.2),
            letter_spacing: CssLength::Zero,
            word_spacing: CssLength::Zero,
            text_align: TextAlign::Start,
            vertical_align: VerticalAlign::Baseline,
            text_decoration: TextDecoration::default(),
            text_indent: CssLength::Zero,
            white_space: WhiteSpace::Normal,
            text_transform: TextTransform::None,
            word_break: WordBreak::Normal,
            overflow_wrap: OverflowWrap::Normal,
            direction: Direction::LTR,

            list_style_type: ListStyleType::None,
            custom_list_style_type: String::new(),
            list_style_position: ListStylePosition::Outside,
            list_index: 0,

            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Nowrap,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            align_self: AlignSelf::Auto,
            align_content: AlignContent::Stretch,
            align_safety: 0,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: CssLength::Auto,
            order: 0,
            gap: CssLength::Zero,
            row_gap: CssLength::Zero,
            // ⛔ `normal`, not zero. `column-gap`'s initial value is `normal`,
            // which in a MULTI-COLUMN container computes to 1em
            // (css-multicol-1 §4.2); in flex and grid it resolves to 0, which
            // is what `res_len` gives for `Auto`. Storing `Zero` made an unset
            // gap indistinguishable from an author's `column-gap: 0`, so every
            // multicol block rendered with its columns touching.
            column_gap: CssLength::Auto,

            grid_auto_columns: GridTrackSize::default(),
            grid_auto_rows: GridTrackSize::default(),
            grid_col_line_names: std::collections::HashMap::new(),
            grid_row_line_names: std::collections::HashMap::new(),
            subgrid_columns: false,
            subgrid_rows: false,
            grid_column_start: 0,
            grid_column_end: 0,
            grid_row_start: 0,
            grid_row_end: 0,
            grid_column_start_name: String::new(),
            grid_column_end_name: String::new(),
            grid_row_start_name: String::new(),
            grid_row_end_name: String::new(),
            grid_area: String::new(),
            grid_auto_flow: GridAutoFlow::Row,
            justify_items: AlignItems::Stretch,
            justify_self: AlignSelf::Auto,

            opacity: 1.0,
            visibility: true,
            box_shadow: Vec::new(),

            gradient_type: GradientType::None,
            gradient_angle: 180.0,
            gradient_direction: GradientDirection::Angle(180.0),
            gradient_radial_shape: GradientRadialShape::Ellipse,
            gradient_radial_size: GradientRadialSize::FarthestCorner,
            gradient_radial_radius_x: CssLength::Auto,
            gradient_radial_radius_y: CssLength::Auto,
            gradient_radial_position_x: CssLength::Percent(50.0),
            gradient_radial_position_y: CssLength::Percent(50.0),

            background_size: BackgroundSize::Auto,
            background_size_w: CssLength::Auto,
            background_size_h: CssLength::Auto,
            background_position_x: CssLength::Zero,
            background_position_y: CssLength::Zero,
            background_repeat: BackgroundRepeat::Repeat,

            outline_width: 0.0,
            outline_style: BorderStyle::None,
            outline_color: Color::BLACK,
            outline_offset: 0.0,

            text_overflow: TextOverflow::Clip,
            text_overflow_string: String::new(),
            text_shadow: None,
            small_caps: false,
            font_variant_alternates: String::from("normal"),
            font_variant_caps: String::from("normal"),
            font_variant_east_asian: String::from("normal"),
            font_variant_emoji: String::from("normal"),
            font_variant_ligatures: String::from("normal"),
            font_variant_numeric: String::from("normal"),
            font_variant_position: String::from("normal"),

            object_fit: ObjectFit::Fill,

            before_content: String::new(),
            after_content: String::new(),
            marker_content: String::new(),
            before_style: None,
            after_style: None,
            selection_style: None,
            placeholder_style: None,
            marker_style: None,
            backdrop_style: None,

            caret_color: None,
            scrollbar_thumb_color: None,
            scrollbar_track_color: None,

            border_collapse: false,
            border_spacing_h: CssLength::Zero,
            border_spacing_v: CssLength::Zero,
            border_image_source: String::from("none"),
            border_image_slice: String::from("100%"),
            border_image_width: String::from("1"),
            border_image_outset: String::from("0"),
            border_image_repeat: String::from("stretch"),
            caption_side: CaptionSide::Top,
            empty_cells_hide: false,
            table_layout_fixed: false,
            cell_padding: CssLength::Auto,

            border_top_left_radius: CssLength::Zero,
            border_top_right_radius: CssLength::Zero,
            border_bottom_left_radius: CssLength::Zero,
            border_bottom_right_radius: CssLength::Zero,
            border_top_left_radius_y: CssLength::Zero,
            border_top_right_radius_y: CssLength::Zero,
            border_bottom_right_radius_y: CssLength::Zero,
            border_bottom_left_radius_y: CssLength::Zero,

            background_image_url: String::new(),

            unicode_bidi: UnicodeBidi::Normal,
            writing_mode: WritingMode::HorizontalTB,
            text_orientation: TextOrientation::Mixed,
            text_combine_upright: String::from("none"),

            cursor: CSSCursor::Auto,

            break_before: BreakValue::Auto,
            break_after: BreakValue::Auto,
            break_inside: BreakInside::Auto,

            tab_size: 8,
            hyphens: Hyphens::Manual,
            widows: 2,
            orphans: 2,

            clip_path: ClipPath::default(),
            clip_rect: None,

            pointer_events: PointerEvents::Auto,

            container_name: String::new(),
            container_type: ContainerType::Normal,

            hover_style: None,
            active_style: None,
            visited_style: None,

            list_style_image: String::new(),

            object_position_x: CssLength::Percent(50.0),
            object_position_y: CssLength::Percent(50.0),

            aspect_ratio: None,

            text_decoration_color: None,
            text_decoration_style: TextDecorationStyle::Solid,
            text_decoration_thickness: CssLength::Auto,
            text_decoration_skip_ink: String::from("auto"),
            text_emphasis_style: String::from("none"),
            text_emphasis_color: None,
            text_emphasis_position: String::from("over right"),
            text_underline_position: TextUnderlinePosition::Auto,
            text_wrap: String::from("wrap"),
            background_blend_mode: String::from("normal"),
            overflow_anchor: String::from("auto"),
            overflow_clip_margin: String::from("0px"),
            anchor_name: String::from("none"),
            position_anchor: String::from("auto"),
            view_transition_name: String::from("none"),
            animation_timeline: String::from("auto"),
            scroll_timeline: String::from("none"),
            offset_path: String::from("none"),
            scrollbar_width: String::from("auto"),
            scrollbar_gutter: String::from("auto"),
            appearance: String::from("auto"),
            field_sizing: String::from("fixed"),
            interpolate_size: String::from("numeric-only"),
            margin_trim: String::from("none"),
            forced_color_adjust: String::from("auto"),

            scroll_snap_type: ScrollSnapType::none(),
            scroll_snap_align: ScrollSnapAlign::None,
            scroll_snap_stop: String::from("normal"),
            overscroll_behavior_x: OverscrollBehavior::Auto,
            overscroll_behavior_y: OverscrollBehavior::Auto,

            contain_layout: false,
            contain_paint: false,
            contain_size: false,
            content_visibility: ContentVisibility::Visible,
            contain_intrinsic_width: CssLength::None,
            contain_intrinsic_height: CssLength::None,

            will_change_transform: false,

            scroll_padding_top: CssLength::Zero,
            scroll_padding_right: CssLength::Zero,
            scroll_padding_bottom: CssLength::Zero,
            scroll_padding_left: CssLength::Zero,
            scroll_margin_top: CssLength::Zero,
            scroll_margin_right: CssLength::Zero,
            scroll_margin_bottom: CssLength::Zero,
            scroll_margin_left: CssLength::Zero,

            user_select: UserSelect::Auto,
            resize: Resize::None,

            shape_outside: String::from("none"),
            shape_margin: CssLength::Zero,

            background_clip: BackgroundClip::BorderBox,
            background_origin: BackgroundClip::PaddingBox,
            background_attachment: BackgroundAttachment::Scroll,

            column_count: None,
            column_width: CssLength::Auto,
            column_rule_width: CssLength::Px(0.0),
            column_rule_style: BorderStyle::None,
            column_rule_color: Color::BLACK,
            column_fill: true,
            column_span_all: false,
            line_clamp: None,

            transform: String::new(),

            css_translate: CssTransform::default(),
            css_rotate: CssTransform::default(),
            css_scale: CssTransform::default(),
            css_transform: CssTransform::default(),
            transform_box: String::from("border-box"),
            transform_style_3d: String::from("flat"),
            perspective: String::from("none"),
            perspective_origin: String::from("50% 50%"),
            backface_visibility: String::from("visible"),
            css_filter: CssFilters::default(),

            text_underline_offset: CssLength::Auto,

            will_change: String::new(),

            scroll_behavior: ScrollBehavior::Auto,
            isolation: false,
            mix_blend_mode: MixBlendMode::Normal,
            color_scheme: "normal".to_string(),

            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            counter_set: Vec::new(),

            font_stretch: 100.0,
            font_synthesis_weight: true,
            font_synthesis_style: true,
            font_synthesis_small_caps: true,
            font_synthesis_position: true,

            custom_props: HashMap::new(),

            href: String::new(),
        }
    }
}

impl ComputedStyle {
    /// Inherit inheritable properties from a parent style.
    /// Copy inherited properties from parent using the property table.
    pub fn inherit_from(&mut self, parent: &ComputedStyle) {
        for &id in crate::css::property_defs::INHERITED_IDS {
            let def = crate::css::property_defs::get(id);
            (def.copy)(self, parent);
        }
    }

    pub fn is_block_level(&self) -> bool {
        matches!(
            self.display,
            Display::Block
                | Display::Flex
                | Display::Grid
                | Display::Table
                | Display::ListItem
                | Display::FlowRoot
        )
    }

    pub fn is_inline_level(&self) -> bool {
        matches!(
            self.display,
            Display::Inline
                | Display::InlineBlock
                | Display::InlineFlex
                | Display::InlineGrid
                | Display::Ruby
                | Display::RubyText
        )
    }

    pub fn is_positioned(&self) -> bool {
        !matches!(self.position, Position::Static)
    }

    pub fn establishes_bfc(&self) -> bool {
        self.is_positioned()
            || !matches!(self.float, Float::None)
            || matches!(
                self.overflow_x,
                Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
            )
            || matches!(
                self.overflow_y,
                Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
            )
            || matches!(
                self.display,
                Display::Flex
                    | Display::Grid
                    | Display::InlineFlex
                    | Display::InlineGrid
                    | Display::InlineBlock
                    | Display::Table
            )
    }

    /// Resolved font size in px (needs parent px for em/%, root px for rem).
    pub fn font_size_px(&self, parent_px: f32, root_px: f32) -> f32 {
        // For font-size, `%` is relative to the parent font size, not the containing block width.
        // Pass parent_px as both the em base and the percentage base.
        self.font_size
            .resolve(parent_px, parent_px, root_px)
            .max(1.0)
    }
}
