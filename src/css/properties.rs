#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyId {
    // Display & Layout
    Display,
    Position,
    Float,
    Clear,
    BoxSizing,
    Overflow,
    OverflowX,
    OverflowY,
    Visibility,
    Opacity,
    ZIndex,

    // Sizing
    Width,
    Height,
    MinWidth,
    MinHeight,
    MaxWidth,
    MaxHeight,

    // Margin
    Margin,
    MarginTop,
    MarginRight,
    MarginBottom,
    MarginLeft,

    // Padding
    Padding,
    PaddingTop,
    PaddingRight,
    PaddingBottom,
    PaddingLeft,

    // Border width/style/color
    Border,
    BorderTop,
    BorderRight,
    BorderBottom,
    BorderLeft,
    BorderWidth,
    BorderStyle,
    BorderColor,
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
    BorderRadius,
    BorderTopLeftRadius,
    BorderTopRightRadius,
    BorderBottomLeftRadius,
    BorderBottomRightRadius,
    BorderCollapse,
    BorderSpacing,
    BorderImage,
    BorderImageSource,
    BorderImageSlice,
    BorderImageWidth,
    BorderImageOutset,
    BorderImageRepeat,

    // Position offsets
    Top,
    Right,
    Bottom,
    Left,

    // Color & Background
    Color,
    Background,
    BackgroundColor,
    Fill,
    Stroke,
    BackgroundImage,
    BackgroundSize,
    BackgroundRepeat,
    BackgroundPosition,
    BackgroundAttachment,
    BackgroundOrigin,
    BackgroundClip,
    BackgroundBlendMode,
    Mask,
    MaskImage,
    MaskMode,
    MaskRepeat,
    MaskPosition,
    MaskSize,
    MaskClip,
    MaskOrigin,
    MaskComposite,

    // Font
    Font,
    FontFamily,
    FontSize,
    FontWeight,
    FontStyle,
    FontStretch,
    FontVariant,
    FontVariantAlternates,
    FontVariantCaps,
    FontVariantEastAsian,
    FontVariantEmoji,
    FontVariantLigatures,
    FontVariantNumeric,
    FontVariantPosition,
    FontFeatureSettings,
    FontVariationSettings,
    FontSynthesis,
    FontSynthesisWeight,
    FontSynthesisStyle,
    FontSynthesisSmallCaps,
    FontSynthesisPosition,

    // Text
    LineHeight,
    LetterSpacing,
    WordSpacing,
    TextAlign,
    TextDecoration,
    TextDecorationLine,
    TextDecorationStyle,
    TextDecorationColor,
    TextDecorationThickness,
    TextDecorationSkipInk,
    TextEmphasis,
    TextEmphasisStyle,
    TextEmphasisColor,
    TextEmphasisPosition,
    TextTransform,
    TextIndent,
    TextOverflow,
    TextShadow,
    TextUnderlineOffset,
    TextUnderlinePosition,
    TextWrap,

    // Whitespace & word handling
    WhiteSpace,
    WordBreak,
    WordWrap,
    OverflowWrap,
    Hyphens,

    // Inline layout
    VerticalAlign,
    Direction,
    UnicodeBidi,
    WritingMode,
    TextOrientation,
    TextCombineUpright,

    // List
    ListStyleType,
    ListStylePosition,
    ListStyleImage,
    ListStyle,

    // Flexbox
    Flex,
    FlexFlow,
    FlexDirection,
    FlexWrap,
    FlexGrow,
    FlexShrink,
    FlexBasis,
    AlignItems,
    AlignSelf,
    AlignContent,
    JustifyContent,
    JustifyItems,
    JustifySelf,
    Order,
    Gap,
    RowGap,
    ColumnGap,

    // Grid
    GridTemplateColumns,
    GridTemplateRows,
    GridTemplateAreas,
    GridAutoColumns,
    GridAutoRows,
    GridAutoFlow,
    GridColumn,
    GridColumnStart,
    GridColumnEnd,
    GridRow,
    GridRowStart,
    GridRowEnd,
    GridArea,
    GridTemplate,

    // Table
    TableLayout,
    CaptionSide,
    EmptyCells,

    // Interaction
    Cursor,
    PointerEvents,
    UserSelect,
    TouchAction,
    Resize,

    // Transform
    Transform,
    TransformBox,
    TransformOrigin,
    TransformStyle,
    Perspective,
    PerspectiveOrigin,
    BackfaceVisibility,

    // Transition
    Transition,
    TransitionProperty,
    TransitionDuration,
    TransitionTimingFunction,
    TransitionDelay,
    TransitionBehavior,

    // Animation
    Animation,
    AnimationName,
    AnimationDuration,
    AnimationTimingFunction,
    AnimationDelay,
    AnimationIterationCount,
    AnimationDirection,
    AnimationFillMode,
    AnimationPlayState,
    AnimationComposition,
    AnimationTimeline,

    // Filters & compositing
    Filter,
    BackdropFilter,
    MixBlendMode,
    Isolation,
    Contain,
    ContentVisibility,
    ContainIntrinsicSize,
    WillChange,

    // Outline
    Outline,
    OutlineColor,
    OutlineStyle,
    OutlineWidth,
    OutlineOffset,

    // Effects
    BoxShadow,
    ClipPath,
    Clip,
    ShapeOutside,
    ShapeMargin,
    OffsetPath,

    // Generated content
    Content,
    Quotes,
    CounterIncrement,
    CounterReset,
    CounterSet,

    // Object/image
    ObjectFit,
    ObjectPosition,
    AspectRatio,
    ImageOrientation,
    ImageRendering,

    // Multi-column
    ColumnCount,
    ColumnWidth,
    Columns,
    ColumnSpan,
    ColumnRuleColor,
    ColumnRuleStyle,
    ColumnRuleWidth,
    ColumnRule,
    ColumnFill,

    // Page/break
    PageBreakBefore,
    PageBreakAfter,
    PageBreakInside,
    BreakBefore,
    BreakAfter,
    BreakInside,
    LineClamp,
    Orphans,
    Widows,

    // Scroll
    ScrollBehavior,
    ScrollSnapType,
    ScrollSnapAlign,
    ScrollSnapStop,
    OverflowAnchor,
    OverflowClipMargin,
    ScrollPadding,
    ScrollPaddingTop,
    ScrollPaddingRight,
    ScrollPaddingBottom,
    ScrollPaddingLeft,
    ScrollMargin,
    ScrollMarginTop,
    ScrollMarginRight,
    ScrollMarginBottom,
    ScrollMarginLeft,

    // Overscroll
    OverscrollBehavior,
    OverscrollBehaviorX,
    OverscrollBehaviorY,
    ScrollbarColor,
    ScrollbarWidth,
    ScrollbarGutter,
    ScrollTimeline,

    // UI
    AccentColor,
    CaretColor,
    ColorScheme,
    ForcedColorAdjust,
    Appearance,
    FieldSizing,

    // Container queries
    ContainerType,
    ContainerName,
    Container,

    // Anchor positioning / view transitions
    AnchorName,
    PositionAnchor,
    ViewTransitionName,

    // Logical sizing
    InlineSize,
    BlockSize,

    // Logical inset
    InsetBlockStart,
    InsetBlockEnd,
    InsetInlineStart,
    InsetInlineEnd,
    Inset,
    InsetBlock,
    InsetInline,
    MinInlineSize,
    MinBlockSize,
    MaxInlineSize,
    MaxBlockSize,

    // Logical margin
    MarginBlockStart,
    MarginBlockEnd,
    MarginInlineStart,
    MarginInlineEnd,
    MarginBlock,
    MarginInline,

    // Logical padding
    PaddingBlockStart,
    PaddingBlockEnd,
    PaddingInlineStart,
    PaddingInlineEnd,
    PaddingBlock,
    PaddingInline,
    BorderBlock,
    BorderBlockStart,
    BorderBlockEnd,
    BorderInline,
    BorderInlineStart,
    BorderInlineEnd,
    BorderBlockStartWidth,
    BorderBlockEndWidth,
    BorderInlineStartWidth,
    BorderInlineEndWidth,
    BorderBlockStartStyle,
    BorderBlockEndStyle,
    BorderInlineStartStyle,
    BorderInlineEndStyle,
    BorderBlockStartColor,
    BorderBlockEndColor,
    BorderInlineStartColor,
    BorderInlineEndColor,

    // Place shorthands
    PlaceContent,
    PlaceItems,
    PlaceSelf,

    // Misc
    ColorInterpolation,
    InterpolateSize,
    MarginTrim,
    TabSize,
    Rotate,
    Scale,
    Translate,

    Unknown,
}

/// Map a CSS property name (lowercase) to its `PropertyId`.
pub fn resolve(name: &str) -> PropertyId {
    // Strip vendor prefixes first
    let stripped = if name.starts_with("-webkit-") {
        &name[8..]
    } else if name.starts_with("-moz-") {
        &name[5..]
    } else if name.starts_with("-ms-") {
        &name[4..]
    } else if name.starts_with("-o-") {
        &name[3..]
    } else {
        name
    };

    match stripped {
        // Display & Layout
        "display" => PropertyId::Display,
        "position" => PropertyId::Position,
        "float" => PropertyId::Float,
        "clear" => PropertyId::Clear,
        "box-sizing" => PropertyId::BoxSizing,
        "overflow" => PropertyId::Overflow,
        "overflow-x" => PropertyId::OverflowX,
        "overflow-y" => PropertyId::OverflowY,
        "visibility" => PropertyId::Visibility,
        "opacity" => PropertyId::Opacity,
        "z-index" => PropertyId::ZIndex,

        // Sizing
        "width" => PropertyId::Width,
        "height" => PropertyId::Height,
        "min-width" => PropertyId::MinWidth,
        "min-height" => PropertyId::MinHeight,
        "max-width" => PropertyId::MaxWidth,
        "max-height" => PropertyId::MaxHeight,

        // Margin
        "margin" => PropertyId::Margin,
        "margin-top" => PropertyId::MarginTop,
        "margin-right" => PropertyId::MarginRight,
        "margin-bottom" => PropertyId::MarginBottom,
        "margin-left" => PropertyId::MarginLeft,

        // Padding
        "padding" => PropertyId::Padding,
        "padding-top" => PropertyId::PaddingTop,
        "padding-right" => PropertyId::PaddingRight,
        "padding-bottom" => PropertyId::PaddingBottom,
        "padding-left" => PropertyId::PaddingLeft,

        // Border shorthands
        "border" => PropertyId::Border,
        "border-top" => PropertyId::BorderTop,
        "border-right" => PropertyId::BorderRight,
        "border-bottom" => PropertyId::BorderBottom,
        "border-left" => PropertyId::BorderLeft,
        "border-width" => PropertyId::BorderWidth,
        "border-style" => PropertyId::BorderStyle,
        "border-color" => PropertyId::BorderColor,

        // Border individual
        "border-top-width" => PropertyId::BorderTopWidth,
        "border-right-width" => PropertyId::BorderRightWidth,
        "border-bottom-width" => PropertyId::BorderBottomWidth,
        "border-left-width" => PropertyId::BorderLeftWidth,
        "border-top-style" => PropertyId::BorderTopStyle,
        "border-right-style" => PropertyId::BorderRightStyle,
        "border-bottom-style" => PropertyId::BorderBottomStyle,
        "border-left-style" => PropertyId::BorderLeftStyle,
        "border-top-color" => PropertyId::BorderTopColor,
        "border-right-color" => PropertyId::BorderRightColor,
        "border-bottom-color" => PropertyId::BorderBottomColor,
        "border-left-color" => PropertyId::BorderLeftColor,
        "border-radius" => PropertyId::BorderRadius,
        "border-top-left-radius" => PropertyId::BorderTopLeftRadius,
        "border-top-right-radius" => PropertyId::BorderTopRightRadius,
        "border-bottom-left-radius" => PropertyId::BorderBottomLeftRadius,
        "border-bottom-right-radius" => PropertyId::BorderBottomRightRadius,
        "border-collapse" => PropertyId::BorderCollapse,
        "border-spacing" => PropertyId::BorderSpacing,
        "border-image" => PropertyId::BorderImage,
        "border-image-source" => PropertyId::BorderImageSource,
        "border-image-slice" => PropertyId::BorderImageSlice,
        "border-image-width" => PropertyId::BorderImageWidth,
        "border-image-outset" => PropertyId::BorderImageOutset,
        "border-image-repeat" => PropertyId::BorderImageRepeat,

        // Position offsets
        "top" => PropertyId::Top,
        "right" => PropertyId::Right,
        "bottom" => PropertyId::Bottom,
        "left" => PropertyId::Left,

        // Color & Background
        "color" => PropertyId::Color,
        "background" => PropertyId::Background,
        "background-color" => PropertyId::BackgroundColor,
        "fill" => PropertyId::Fill,
        "stroke" => PropertyId::Stroke,
        "background-image" => PropertyId::BackgroundImage,
        "mask" => PropertyId::Mask,
        "mask-image" => PropertyId::MaskImage,
        "mask-mode" => PropertyId::MaskMode,
        "mask-repeat" => PropertyId::MaskRepeat,
        "mask-position" => PropertyId::MaskPosition,
        "mask-size" => PropertyId::MaskSize,
        "mask-clip" => PropertyId::MaskClip,
        "mask-origin" => PropertyId::MaskOrigin,
        "mask-composite" => PropertyId::MaskComposite,
        "background-size" => PropertyId::BackgroundSize,
        "background-repeat" => PropertyId::BackgroundRepeat,
        "background-position" => PropertyId::BackgroundPosition,
        "background-attachment" => PropertyId::BackgroundAttachment,
        "background-origin" => PropertyId::BackgroundOrigin,
        "background-clip" => PropertyId::BackgroundClip,
        "background-blend-mode" => PropertyId::BackgroundBlendMode,

        // Font
        "font" => PropertyId::Font,
        "font-family" => PropertyId::FontFamily,
        "font-size" => PropertyId::FontSize,
        "font-weight" => PropertyId::FontWeight,
        "font-style" => PropertyId::FontStyle,
        "font-stretch" => PropertyId::FontStretch,
        "font-synthesis" => PropertyId::FontSynthesis,
        "font-synthesis-weight" => PropertyId::FontSynthesisWeight,
        "font-synthesis-style" => PropertyId::FontSynthesisStyle,
        "font-synthesis-small-caps" => PropertyId::FontSynthesisSmallCaps,
        "font-synthesis-position" => PropertyId::FontSynthesisPosition,
        "font-variant" => PropertyId::FontVariant,
        "font-variant-alternates" => PropertyId::FontVariantAlternates,
        "font-variant-caps" => PropertyId::FontVariantCaps,
        "font-variant-east-asian" => PropertyId::FontVariantEastAsian,
        "font-variant-emoji" => PropertyId::FontVariantEmoji,
        "font-variant-ligatures" => PropertyId::FontVariantLigatures,
        "font-variant-numeric" => PropertyId::FontVariantNumeric,
        "font-variant-position" => PropertyId::FontVariantPosition,
        "font-feature-settings" => PropertyId::FontFeatureSettings,
        "font-variation-settings" => PropertyId::FontVariationSettings,

        // Text
        "line-height" => PropertyId::LineHeight,
        "letter-spacing" => PropertyId::LetterSpacing,
        "word-spacing" => PropertyId::WordSpacing,
        "text-align" => PropertyId::TextAlign,
        "text-decoration" => PropertyId::TextDecoration,
        "text-decoration-line" => PropertyId::TextDecorationLine,
        "text-decoration-style" => PropertyId::TextDecorationStyle,
        "text-decoration-color" => PropertyId::TextDecorationColor,
        "text-decoration-thickness" => PropertyId::TextDecorationThickness,
        "text-decoration-skip-ink" => PropertyId::TextDecorationSkipInk,
        "text-emphasis" => PropertyId::TextEmphasis,
        "text-emphasis-style" => PropertyId::TextEmphasisStyle,
        "text-emphasis-color" => PropertyId::TextEmphasisColor,
        "text-emphasis-position" => PropertyId::TextEmphasisPosition,
        "text-transform" => PropertyId::TextTransform,
        "text-indent" => PropertyId::TextIndent,
        "text-overflow" => PropertyId::TextOverflow,
        "text-shadow" => PropertyId::TextShadow,
        "text-underline-offset" => PropertyId::TextUnderlineOffset,
        "text-underline-position" => PropertyId::TextUnderlinePosition,
        "text-wrap" => PropertyId::TextWrap,

        // Whitespace
        "white-space" => PropertyId::WhiteSpace,
        "word-break" => PropertyId::WordBreak,
        "word-wrap" => PropertyId::WordWrap,
        "overflow-wrap" => PropertyId::OverflowWrap,
        "hyphens" => PropertyId::Hyphens,

        // Inline layout
        "vertical-align" => PropertyId::VerticalAlign,
        "direction" => PropertyId::Direction,
        "unicode-bidi" => PropertyId::UnicodeBidi,
        "writing-mode" => PropertyId::WritingMode,
        "text-orientation" => PropertyId::TextOrientation,
        "text-combine-upright" => PropertyId::TextCombineUpright,

        // List
        "list-style-type" => PropertyId::ListStyleType,
        "list-style-position" => PropertyId::ListStylePosition,
        "list-style-image" => PropertyId::ListStyleImage,
        "list-style" => PropertyId::ListStyle,

        // Flexbox
        "flex" => PropertyId::Flex,
        "flex-flow" => PropertyId::FlexFlow,
        "flex-direction" => PropertyId::FlexDirection,
        "flex-wrap" => PropertyId::FlexWrap,
        "flex-grow" => PropertyId::FlexGrow,
        "flex-shrink" => PropertyId::FlexShrink,
        "flex-basis" => PropertyId::FlexBasis,
        "align-items" => PropertyId::AlignItems,
        "align-self" => PropertyId::AlignSelf,
        "align-content" => PropertyId::AlignContent,
        "justify-content" => PropertyId::JustifyContent,
        "justify-items" => PropertyId::JustifyItems,
        "justify-self" => PropertyId::JustifySelf,
        "order" => PropertyId::Order,
        "gap" => PropertyId::Gap,
        "row-gap" => PropertyId::RowGap,
        "column-gap" => PropertyId::ColumnGap,

        // Grid
        "grid-template-columns" => PropertyId::GridTemplateColumns,
        "grid-template-rows" => PropertyId::GridTemplateRows,
        "grid-template-areas" => PropertyId::GridTemplateAreas,
        "grid-auto-columns" => PropertyId::GridAutoColumns,
        "grid-auto-rows" => PropertyId::GridAutoRows,
        "grid-auto-flow" => PropertyId::GridAutoFlow,
        "grid-column" => PropertyId::GridColumn,
        "grid-column-start" => PropertyId::GridColumnStart,
        "grid-column-end" => PropertyId::GridColumnEnd,
        "grid-row" => PropertyId::GridRow,
        "grid-row-start" => PropertyId::GridRowStart,
        "grid-row-end" => PropertyId::GridRowEnd,
        "grid-area" => PropertyId::GridArea,
        "grid" | "grid-template" => PropertyId::GridTemplate,

        // Table
        "table-layout" => PropertyId::TableLayout,
        "caption-side" => PropertyId::CaptionSide,
        "empty-cells" => PropertyId::EmptyCells,

        // Interaction
        "cursor" => PropertyId::Cursor,
        "pointer-events" => PropertyId::PointerEvents,
        "user-select" => PropertyId::UserSelect,
        "touch-action" => PropertyId::TouchAction,
        "resize" => PropertyId::Resize,

        // Transform
        "transform" => PropertyId::Transform,
        "transform-box" => PropertyId::TransformBox,
        "transform-origin" => PropertyId::TransformOrigin,
        "transform-style" => PropertyId::TransformStyle,
        "perspective" => PropertyId::Perspective,
        "perspective-origin" => PropertyId::PerspectiveOrigin,
        "backface-visibility" => PropertyId::BackfaceVisibility,

        // Transition
        "transition" => PropertyId::Transition,
        "transition-property" => PropertyId::TransitionProperty,
        "transition-duration" => PropertyId::TransitionDuration,
        "transition-timing-function" => PropertyId::TransitionTimingFunction,
        "transition-delay" => PropertyId::TransitionDelay,
        "transition-behavior" => PropertyId::TransitionBehavior,

        // Animation
        "animation" => PropertyId::Animation,
        "animation-name" => PropertyId::AnimationName,
        "animation-duration" => PropertyId::AnimationDuration,
        "animation-timing-function" => PropertyId::AnimationTimingFunction,
        "animation-delay" => PropertyId::AnimationDelay,
        "animation-iteration-count" => PropertyId::AnimationIterationCount,
        "animation-direction" => PropertyId::AnimationDirection,
        "animation-fill-mode" => PropertyId::AnimationFillMode,
        "animation-play-state" => PropertyId::AnimationPlayState,
        "animation-composition" => PropertyId::AnimationComposition,
        "animation-timeline" => PropertyId::AnimationTimeline,

        // Filters & compositing
        "filter" => PropertyId::Filter,
        "backdrop-filter" => PropertyId::BackdropFilter,
        "mix-blend-mode" => PropertyId::MixBlendMode,
        "isolation" => PropertyId::Isolation,
        "contain" => PropertyId::Contain,
        "content-visibility" => PropertyId::ContentVisibility,
        "contain-intrinsic-size" => PropertyId::ContainIntrinsicSize,
        "will-change" => PropertyId::WillChange,

        // Outline
        "outline" => PropertyId::Outline,
        "outline-color" => PropertyId::OutlineColor,
        "outline-style" => PropertyId::OutlineStyle,
        "outline-width" => PropertyId::OutlineWidth,
        "outline-offset" => PropertyId::OutlineOffset,

        // Effects
        "box-shadow" => PropertyId::BoxShadow,
        "clip-path" => PropertyId::ClipPath,
        "clip" => PropertyId::Clip,
        "shape-outside" => PropertyId::ShapeOutside,
        "shape-margin" => PropertyId::ShapeMargin,
        "offset-path" => PropertyId::OffsetPath,

        // Generated content
        "content" => PropertyId::Content,
        "quotes" => PropertyId::Quotes,
        "counter-increment" => PropertyId::CounterIncrement,
        "counter-reset" => PropertyId::CounterReset,
        "counter-set" => PropertyId::CounterSet,

        // Object/image
        "object-fit" => PropertyId::ObjectFit,
        "object-position" => PropertyId::ObjectPosition,
        "aspect-ratio" => PropertyId::AspectRatio,
        "image-orientation" => PropertyId::ImageOrientation,
        "image-rendering" => PropertyId::ImageRendering,

        // Multi-column
        "column-count" => PropertyId::ColumnCount,
        "column-width" => PropertyId::ColumnWidth,
        "columns" => PropertyId::Columns,
        "column-span" => PropertyId::ColumnSpan,
        "column-rule-color" => PropertyId::ColumnRuleColor,
        "column-rule-style" => PropertyId::ColumnRuleStyle,
        "column-rule-width" => PropertyId::ColumnRuleWidth,
        "column-rule" => PropertyId::ColumnRule,
        "column-fill" => PropertyId::ColumnFill,

        // Page/break
        "page-break-before" => PropertyId::PageBreakBefore,
        "page-break-after" => PropertyId::PageBreakAfter,
        "page-break-inside" => PropertyId::PageBreakInside,
        "break-before" => PropertyId::BreakBefore,
        "break-after" => PropertyId::BreakAfter,
        "break-inside" => PropertyId::BreakInside,
        "line-clamp" => PropertyId::LineClamp,
        "-webkit-line-clamp" => PropertyId::LineClamp,
        "orphans" => PropertyId::Orphans,
        "widows" => PropertyId::Widows,

        // Scroll
        "scroll-behavior" => PropertyId::ScrollBehavior,
        "scroll-snap-type" => PropertyId::ScrollSnapType,
        "scroll-snap-align" => PropertyId::ScrollSnapAlign,
        "scroll-snap-stop" => PropertyId::ScrollSnapStop,
        "overflow-anchor" => PropertyId::OverflowAnchor,
        "overflow-clip-margin" => PropertyId::OverflowClipMargin,
        "scroll-padding" => PropertyId::ScrollPadding,
        "scroll-padding-top" => PropertyId::ScrollPaddingTop,
        "scroll-padding-right" => PropertyId::ScrollPaddingRight,
        "scroll-padding-bottom" => PropertyId::ScrollPaddingBottom,
        "scroll-padding-left" => PropertyId::ScrollPaddingLeft,
        "scroll-margin" => PropertyId::ScrollMargin,
        "scroll-margin-top" => PropertyId::ScrollMarginTop,
        "scroll-margin-right" => PropertyId::ScrollMarginRight,
        "scroll-margin-bottom" => PropertyId::ScrollMarginBottom,
        "scroll-margin-left" => PropertyId::ScrollMarginLeft,

        // Overscroll
        "overscroll-behavior" => PropertyId::OverscrollBehavior,
        "overscroll-behavior-x" => PropertyId::OverscrollBehaviorX,
        "overscroll-behavior-y" => PropertyId::OverscrollBehaviorY,
        "scrollbar-color" => PropertyId::ScrollbarColor,
        "scrollbar-width" => PropertyId::ScrollbarWidth,
        "scrollbar-gutter" => PropertyId::ScrollbarGutter,
        "scroll-timeline" => PropertyId::ScrollTimeline,

        // UI
        "accent-color" => PropertyId::AccentColor,
        "caret-color" => PropertyId::CaretColor,
        "color-scheme" => PropertyId::ColorScheme,
        "forced-color-adjust" => PropertyId::ForcedColorAdjust,
        "appearance" => PropertyId::Appearance,
        "field-sizing" => PropertyId::FieldSizing,

        // Container queries
        "container-type" => PropertyId::ContainerType,
        "container-name" => PropertyId::ContainerName,
        "container" => PropertyId::Container,

        // Anchor positioning / view transitions
        "anchor-name" => PropertyId::AnchorName,
        "position-anchor" => PropertyId::PositionAnchor,
        "view-transition-name" => PropertyId::ViewTransitionName,

        // Logical sizing
        "inline-size" => PropertyId::InlineSize,
        "block-size" => PropertyId::BlockSize,
        "min-inline-size" => PropertyId::MinInlineSize,
        "min-block-size" => PropertyId::MinBlockSize,
        "max-inline-size" => PropertyId::MaxInlineSize,
        "max-block-size" => PropertyId::MaxBlockSize,

        // Logical inset
        "inset-block-start" => PropertyId::InsetBlockStart,
        "inset-block-end" => PropertyId::InsetBlockEnd,
        "inset-inline-start" => PropertyId::InsetInlineStart,
        "inset-inline-end" => PropertyId::InsetInlineEnd,
        "inset" => PropertyId::Inset,
        "inset-block" => PropertyId::InsetBlock,
        "inset-inline" => PropertyId::InsetInline,

        // Logical margin
        "margin-block-start" => PropertyId::MarginBlockStart,
        "margin-block-end" => PropertyId::MarginBlockEnd,
        "margin-inline-start" => PropertyId::MarginInlineStart,
        "margin-inline-end" => PropertyId::MarginInlineEnd,
        "margin-block" => PropertyId::MarginBlock,
        "margin-inline" => PropertyId::MarginInline,

        // Logical padding
        "padding-block-start" => PropertyId::PaddingBlockStart,
        "padding-block-end" => PropertyId::PaddingBlockEnd,
        "padding-inline-start" => PropertyId::PaddingInlineStart,
        "padding-inline-end" => PropertyId::PaddingInlineEnd,
        "padding-block" => PropertyId::PaddingBlock,
        "padding-inline" => PropertyId::PaddingInline,

        // Logical borders
        "border-block" => PropertyId::BorderBlock,
        "border-block-start" => PropertyId::BorderBlockStart,
        "border-block-end" => PropertyId::BorderBlockEnd,
        "border-inline" => PropertyId::BorderInline,
        "border-inline-start" => PropertyId::BorderInlineStart,
        "border-inline-end" => PropertyId::BorderInlineEnd,
        "border-block-start-width" => PropertyId::BorderBlockStartWidth,
        "border-block-end-width" => PropertyId::BorderBlockEndWidth,
        "border-inline-start-width" => PropertyId::BorderInlineStartWidth,
        "border-inline-end-width" => PropertyId::BorderInlineEndWidth,
        "border-block-start-style" => PropertyId::BorderBlockStartStyle,
        "border-block-end-style" => PropertyId::BorderBlockEndStyle,
        "border-inline-start-style" => PropertyId::BorderInlineStartStyle,
        "border-inline-end-style" => PropertyId::BorderInlineEndStyle,
        "border-block-start-color" => PropertyId::BorderBlockStartColor,
        "border-block-end-color" => PropertyId::BorderBlockEndColor,
        "border-inline-start-color" => PropertyId::BorderInlineStartColor,
        "border-inline-end-color" => PropertyId::BorderInlineEndColor,

        // Place shorthands
        "place-content" => PropertyId::PlaceContent,
        "place-items" => PropertyId::PlaceItems,
        "place-self" => PropertyId::PlaceSelf,

        // Misc
        "color-interpolation" => PropertyId::ColorInterpolation,
        "interpolate-size" => PropertyId::InterpolateSize,
        "margin-trim" => PropertyId::MarginTrim,
        "tab-size" => PropertyId::TabSize,
        "rotate" => PropertyId::Rotate,
        "scale" => PropertyId::Scale,
        "translate" => PropertyId::Translate,

        // HTML table attributes mapped to CSS
        "cellpadding" => PropertyId::Padding,
        "cellspacing" => PropertyId::BorderSpacing,

        _ => PropertyId::Unknown,
    }
}

/// Returns true if the property is inherited by default per the CSS spec.
pub fn is_inherited(id: PropertyId) -> bool {
    matches!(
        id,
        PropertyId::Color
            | PropertyId::FontFamily
            | PropertyId::FontSize
            | PropertyId::FontWeight
            | PropertyId::FontStyle
            | PropertyId::FontStretch
            | PropertyId::FontVariant
            | PropertyId::FontVariantAlternates
            | PropertyId::FontVariantCaps
            | PropertyId::FontVariantEastAsian
            | PropertyId::FontVariantEmoji
            | PropertyId::FontVariantLigatures
            | PropertyId::FontVariantNumeric
            | PropertyId::FontVariantPosition
            | PropertyId::FontFeatureSettings
            | PropertyId::FontVariationSettings
            | PropertyId::FontSynthesis
            | PropertyId::FontSynthesisWeight
            | PropertyId::FontSynthesisStyle
            | PropertyId::FontSynthesisSmallCaps
            | PropertyId::FontSynthesisPosition
            | PropertyId::Font
            | PropertyId::LineHeight
            | PropertyId::LetterSpacing
            | PropertyId::WordSpacing
            | PropertyId::TextAlign
            | PropertyId::TextIndent
            | PropertyId::TextTransform
            | PropertyId::TextDecoration
            | PropertyId::TextDecorationLine
            | PropertyId::TextDecorationStyle
            | PropertyId::TextDecorationColor
            | PropertyId::TextDecorationThickness
            | PropertyId::TextDecorationSkipInk
            | PropertyId::TextEmphasis
            | PropertyId::TextEmphasisStyle
            | PropertyId::TextEmphasisColor
            | PropertyId::TextEmphasisPosition
            | PropertyId::TextUnderlineOffset
            | PropertyId::TextUnderlinePosition
            | PropertyId::TextShadow
            | PropertyId::TextWrap
            | PropertyId::WhiteSpace
            | PropertyId::WordBreak
            | PropertyId::WordWrap
            | PropertyId::OverflowWrap
            | PropertyId::Hyphens
            | PropertyId::Direction
            | PropertyId::WritingMode
            | PropertyId::TextOrientation
            | PropertyId::TextCombineUpright
            | PropertyId::InterpolateSize
            | PropertyId::Visibility
            | PropertyId::Cursor
            | PropertyId::ListStyleType
            | PropertyId::ListStylePosition
            | PropertyId::ListStyleImage
            | PropertyId::ListStyle
            | PropertyId::Quotes
            | PropertyId::Orphans
            | PropertyId::Widows
            | PropertyId::TabSize
            | PropertyId::BorderCollapse
            | PropertyId::BorderSpacing
            | PropertyId::CaptionSide
            | PropertyId::EmptyCells
            | PropertyId::ColorScheme
    )
}

/// Returns true if the property is a shorthand that expands into longhands.
pub fn is_shorthand(id: PropertyId) -> bool {
    matches!(
        id,
        PropertyId::Margin
            | PropertyId::Padding
            | PropertyId::Border
            | PropertyId::BorderTop
            | PropertyId::BorderRight
            | PropertyId::BorderBottom
            | PropertyId::BorderLeft
            | PropertyId::BorderWidth
            | PropertyId::BorderStyle
            | PropertyId::BorderColor
            | PropertyId::BorderRadius
            | PropertyId::Background
            | PropertyId::Font
            | PropertyId::Flex
            | PropertyId::FlexFlow
            | PropertyId::ListStyle
            | PropertyId::TextDecoration
            | PropertyId::Outline
            | PropertyId::Transition
            | PropertyId::Animation
            | PropertyId::GridTemplate
            | PropertyId::GridArea
            | PropertyId::GridColumn
            | PropertyId::GridRow
            | PropertyId::Gap
            | PropertyId::Columns
            | PropertyId::ColumnRule
            | PropertyId::PlaceContent
            | PropertyId::PlaceItems
            | PropertyId::PlaceSelf
            | PropertyId::Inset
            | PropertyId::InsetBlock
            | PropertyId::InsetInline
            | PropertyId::MarginBlock
            | PropertyId::MarginInline
            | PropertyId::PaddingBlock
            | PropertyId::PaddingInline
            | PropertyId::ScrollPadding
            | PropertyId::ScrollMargin
            | PropertyId::OverscrollBehavior
            | PropertyId::Overflow
            | PropertyId::Container
    )
}
