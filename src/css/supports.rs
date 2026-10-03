//! Declaration feature queries. A deferred (`Raw`) value is not a parse error.
//! Keep value validation separate from stylesheet storage/compilation choices.

use super::{calc, properties::PropertyId, value_parse};

pub(super) fn declaration_value(id: PropertyId, value: &str) -> bool {
    use PropertyId::*;
    let lower = value.to_ascii_lowercase();
    let v = lower.as_str();
    let keywords = |words: &str| words.split_whitespace().any(|word| word == v);
    let list = |words: &str, max: usize| keyword_list(v, words, max);
    match id {
        GridTemplateColumns | GridTemplateRows => track_list(v),
        GridAutoColumns | GridAutoRows => track_sizes(v),
        GridTemplateAreas => v == "none" || grid_areas(value),
        GridAutoFlow => list("row column dense", 2) && !(v.contains("row") && v.contains("column")),
        GridColumnStart | GridColumnEnd | GridRowStart | GridRowEnd => grid_line(value),
        GridColumn | GridRow => slash_list(value, 2, grid_line),
        GridArea => slash_list(value, 4, grid_line),
        GridTemplate => v == "none" || slash_list(v, 2, track_list),
        Display if keywords("flow-root contents") => true,
        ContainerType => keywords("normal size inline-size"),
        ContainerName => v == "none" || identifiers(value),
        Container => {
            let parts: Vec<_> = value.split('/').collect();
            parts.len() <= 2
                && declaration_value(ContainerName, parts[0].trim())
                && (parts.len() == 1 || declaration_value(ContainerType, parts[1].trim()))
        }
        Contain => {
            keywords("none strict content") || list("size inline-size layout style paint", 4)
        }
        ContainIntrinsicSize => values(v, 1, 4, |p| {
            matches!(p, "auto" | "none") || length(p, false, false)
        }),
        ContentVisibility => keywords("visible auto hidden"),
        Transform => super::transform_parse::parse_css_transform_checked(v).is_some(),
        Translate => super::transform_parse::parse_individual_translate(v).is_some(),
        Rotate => super::transform_parse::parse_individual_rotate(v).is_some(),
        Scale => super::transform_parse::parse_individual_scale(v).is_some(),
        Filter | BackdropFilter => {
            super::apply::parse_css_filter_checked(v, crate::types::Color::BLACK).is_some()
        }
        TransformBox => keywords("content-box border-box fill-box stroke-box view-box"),
        TransformStyle => keywords("flat preserve-3d"),
        BackfaceVisibility => keywords("visible hidden"),
        Perspective => v == "none" || length(v, false, false),
        TransformOrigin | PerspectiveOrigin | ObjectPosition | BackgroundPosition
        | MaskPosition => position(v),
        BackgroundPositionX | BackgroundPositionY => comma_list(v, |p| position(p)),
        AnimationDuration | TransitionDuration => comma_list(v, |p| {
            super::animation::parse_time_ms(p).is_some_and(|n| n >= 0.0)
        }),
        AnimationDelay | TransitionDelay => {
            comma_list(v, |p| super::animation::parse_time_ms(p).is_some())
        }
        AnimationTimingFunction | TransitionTimingFunction => {
            comma_list(v, |p| super::animation::parse_easing_checked(p).is_some())
        }
        AnimationIterationCount => comma_list(v, |p| {
            p == "infinite" || calc::parse_nonnegative_number(p).is_some()
        }),
        AnimationDirection => comma_list(v, |p| {
            keyword_list(p, "normal reverse alternate alternate-reverse", 1)
        }),
        AnimationFillMode => comma_list(v, |p| keyword_list(p, "none forwards backwards both", 1)),
        AnimationPlayState => comma_list(v, |p| keyword_list(p, "running paused", 1)),
        AnimationComposition => comma_list(v, |p| keyword_list(p, "replace add accumulate", 1)),
        TransitionBehavior => comma_list(v, |p| keyword_list(p, "normal allow-discrete", 1)),
        AnimationName | TransitionProperty | WillChange => {
            comma_list(value, |p| ident(p) || css_string(p))
        }
        Animation => comma_list(value, |p| {
            values(p, 1, 10, |t| {
                super::animation::parse_time_ms(t).is_some()
                    || super::animation::parse_easing_checked(t).is_some()
                    || calc::parse_nonnegative_number(t).is_some()
                    || ident(t)
                    || css_string(t)
            })
        }),
        Transition => comma_list(value, |p| {
            values(p, 1, 5, |t| {
                super::animation::parse_time_ms(t).is_some()
                    || super::animation::parse_easing_checked(t).is_some()
                    || ident(t)
            })
        }),
        ColorScheme => v == "normal" || list("light dark only", 3),
        ForcedColorAdjust => keywords("auto none preserve-parent-color"),
        ColorInterpolation => keywords("auto srgb linearrgb"),
        FontSize => value_parse::parse_font_size_checked(v).is_some(),
        FontWeight => {
            keywords("normal bold bolder lighter")
                || calc::parse_css_number(v).is_some_and(|n| (1.0..=1000.0).contains(&n))
        }
        FontFamily => comma_list(value, |p| css_string(p) || identifiers(p)),
        FontStyle => {
            keywords("normal italic oblique")
                || v.strip_prefix("oblique ")
                    .is_some_and(|p| calc::parse_math_angle_deg(p).is_some())
        }
        Font => font_shorthand(value),
        FontVariant => {
            keywords("normal none")
                || values(v, 1, 16, |p| {
                    [
                        FontVariantCaps,
                        FontVariantEastAsian,
                        FontVariantLigatures,
                        FontVariantNumeric,
                        FontVariantPosition,
                        FontVariantAlternates,
                    ]
                    .iter()
                    .any(|id| declaration_value(*id, p))
                })
        }
        FontVariantAlternates => {
            keywords("normal historical-forms")
                || values(v, 1, 6, |p| {
                    [
                        "stylistic",
                        "styleset",
                        "character-variant",
                        "swash",
                        "ornaments",
                        "annotation",
                    ]
                    .iter()
                    .any(|name| {
                        p.strip_prefix(name)
                            .and_then(|s| s.strip_prefix('('))
                            .and_then(|s| s.strip_suffix(')'))
                            .is_some_and(|s| comma_list(s, ident))
                    })
                })
        }
        FontStretch => {
            keywords(
                "normal ultra-condensed extra-condensed condensed semi-condensed semi-expanded expanded extra-expanded ultra-expanded",
            ) || v
                .strip_suffix('%')
                .is_some_and(|p| calc::parse_nonnegative_number(p).is_some())
        }
        FontFeatureSettings => {
            v == "normal" || !super::font::parse_feature_settings(value).is_empty()
        }
        FontVariationSettings => {
            v == "normal" || !super::font::parse_variation_settings(value).is_empty()
        }
        FontSynthesis => v == "none" || list("weight style small-caps position", 4),
        FontSynthesisWeight | FontSynthesisSmallCaps | FontSynthesisPosition => {
            keywords("auto none")
        }
        FontSynthesisStyle => keywords("auto none oblique-only"),
        FontVariantCaps => keywords(
            "normal small-caps all-small-caps petite-caps all-petite-caps unicase titling-caps",
        ),
        FontVariantEmoji => keywords("normal text emoji unicode"),
        FontVariantPosition => keywords("normal sub super"),
        FontVariantLigatures => {
            keywords("normal none")
                || list(
                    "common-ligatures no-common-ligatures discretionary-ligatures no-discretionary-ligatures historical-ligatures no-historical-ligatures contextual no-contextual",
                    4,
                )
        }
        FontVariantNumeric => {
            v == "normal"
                || list(
                    "lining-nums oldstyle-nums proportional-nums tabular-nums diagonal-fractions stacked-fractions ordinal slashed-zero",
                    6,
                )
        }
        FontVariantEastAsian => {
            v == "normal"
                || list(
                    "jis78 jis83 jis90 jis04 simplified traditional full-width proportional-width ruby",
                    3,
                )
        }
        TextWrap => list("wrap nowrap balance pretty stable auto", 2),
        TextDecorationLine => v == "none" || list("underline overline line-through blink", 4),
        TextDecorationStyle => keywords("solid double dotted dashed wavy"),
        TextDecorationSkipInk => keywords("auto none all"),
        TextUnderlinePosition => v == "auto" || list("from-font under left right", 2),
        TextOverflow => keywords("clip ellipsis") || css_string(value),
        TextDecoration => values(v, 1, 6, |p| {
            declaration_value(TextDecorationLine, p)
                || declaration_value(TextDecorationStyle, p)
                || color(p)
                || length(p, true, false)
        }),
        TextShadow | BoxShadow => shadow(v, id == BoxShadow),
        TextEmphasisStyle => {
            keywords("none")
                || css_string(value)
                || list("filled open dot circle double-circle triangle sesame", 2)
        }
        TextEmphasisPosition => list("over under left right", 2),
        TextEmphasis => {
            declaration_value(TextEmphasisStyle, value)
                || values(v, 1, 3, |p| {
                    declaration_value(TextEmphasisStyle, p) || color(p)
                })
        }
        TextOrientation => keywords("mixed upright sideways"),
        TextCombineUpright => keywords("none all"),
        WhiteSpace if v == "break-spaces" => true,
        OverflowWrap | WordWrap => keywords("normal anywhere break-word"),
        Hyphens => keywords("none manual auto"),
        Direction => keywords("ltr rtl"),
        ListStyleType => ident(value),
        ListStylePosition => keywords("inside outside"),
        ListStyleImage => v == "none" || super::extract_url(value).is_some(),
        ListStyle => values(value, 1, 3, |p| ident(p) || super::extract_url(p).is_some()),
        UnicodeBidi => keywords("normal embed isolate bidi-override isolate-override plaintext"),
        WritingMode => keywords("horizontal-tb vertical-rl vertical-lr sideways-rl sideways-lr"),
        AlignItems | AlignSelf | AlignContent | JustifyContent | JustifyItems | JustifySelf => {
            list(
                "normal auto stretch start end self-start self-end flex-start flex-end center left right baseline first last safe unsafe space-between space-around space-evenly legacy",
                3,
            )
        }
        RowGap | ColumnGap => v == "normal" || length(v, true, false),
        Gap => values(v, 1, 2, |p| p == "normal" || length(p, true, false)),
        Flex => {
            keywords("auto none initial")
                || values(v, 1, 3, |p| {
                    calc::parse_nonnegative_number(p).is_some() || declaration_value(FlexBasis, p)
                })
        }
        FlexFlow => values(v, 1, 2, |p| {
            declaration_value(FlexDirection, p) || declaration_value(FlexWrap, p)
        }),
        PlaceContent => values(v, 1, 4, |p| {
            declaration_value(AlignContent, p) || declaration_value(JustifyContent, p)
        }),
        PlaceItems => values(v, 1, 4, |p| {
            declaration_value(AlignItems, p) || declaration_value(JustifyItems, p)
        }),
        PlaceSelf => values(v, 1, 4, |p| {
            declaration_value(AlignSelf, p) || declaration_value(JustifySelf, p)
        }),
        BorderCollapse => keywords("collapse separate"),
        BorderSpacing => values(v, 1, 2, |p| length(p, false, false)),
        TableLayout => keywords("auto fixed"),
        CaptionSide => keywords("top bottom block-start block-end inline-start inline-end"),
        EmptyCells => keywords("show hide"),
        ObjectFit => keywords("fill contain cover none scale-down"),
        AspectRatio => {
            let ratio = v.strip_prefix("auto ").unwrap_or(v);
            v == "auto" || slash_list(ratio, 2, |p| calc::parse_nonnegative_number(p).is_some())
        }
        ImageRendering => {
            keywords("auto smooth high-quality crisp-edges pixelated -webkit-optimize-contrast")
        }
        ImageOrientation => keywords("none from-image"),
        PointerEvents => keywords(
            "auto none visiblepainted visiblefill visiblestroke visible painted fill stroke all bounding-box",
        ),
        UserSelect => keywords("auto none text all contain"),
        TouchAction => {
            keywords("auto none manipulation")
                || list(
                    "pan-x pan-left pan-right pan-y pan-up pan-down pinch-zoom",
                    3,
                )
        }
        Resize => keywords("none both horizontal vertical block inline"),
        Cursor => comma_list(v, |p| {
            keyword_list(
                p,
                "auto default none context-menu help pointer progress wait cell crosshair text vertical-text alias copy move no-drop not-allowed grab grabbing all-scroll col-resize row-resize n-resize e-resize s-resize w-resize ne-resize nw-resize se-resize sw-resize ew-resize ns-resize nesw-resize nwse-resize zoom-in zoom-out",
                1,
            ) || super::extract_url(p).is_some()
        }),
        Appearance => keywords(
            "auto none textfield menulist-button button checkbox radio menulist listbox textarea searchfield push-button square-button slider-horizontal progress-bar meter",
        ),
        FieldSizing => keywords("fixed content"),
        ScrollBehavior => keywords("auto smooth"),
        ScrollSnapType => v == "none" || list("x y block inline both mandatory proximity", 2),
        ScrollSnapAlign => list("none start end center", 2),
        ScrollSnapStop => keywords("normal always"),
        OverflowAnchor => keywords("auto none"),
        OverscrollBehavior => list("auto contain none", 2),
        OverscrollBehaviorX | OverscrollBehaviorY => keywords("auto contain none"),
        ScrollbarWidth => keywords("auto thin none"),
        ScrollbarGutter => v == "auto" || list("stable both-edges", 2),
        ScrollbarColor => v == "auto" || values(v, 2, 2, color),
        Isolation => keywords("auto isolate"),
        MixBlendMode | BackgroundBlendMode => comma_list(v, |p| {
            keyword_list(
                p,
                "normal multiply screen overlay darken lighten color-dodge color-burn hard-light soft-light difference exclusion hue saturation color luminosity plus-lighter",
                1,
            )
        }),
        InterpolateSize => false, // Keyword storage exists, intrinsic-size interpolation does not.
        ColumnCount => v == "auto" || calc::parse_positive_integer(v).is_some(),
        ColumnWidth => v == "auto" || length(v, false, false),
        Columns => values(v, 1, 2, |p| {
            declaration_value(ColumnWidth, p) || declaration_value(ColumnCount, p)
        }),
        ColumnSpan => keywords("none all"),
        ColumnFill => keywords("auto balance balance-all"),
        LineClamp => v == "none" || calc::parse_positive_integer(v).is_some(),
        Orphans | Widows => calc::parse_positive_integer(v).is_some(),
        TabSize => v.parse::<u32>().is_ok(),
        PageBreakBefore | PageBreakAfter => keywords("auto always avoid left right"),
        PageBreakInside => keywords("auto avoid"),
        BreakBefore | BreakAfter => keywords(
            "auto avoid always all avoid-page page left right recto verso avoid-column column",
        ),
        BreakInside => keywords("auto avoid avoid-page avoid-column"),
        Quotes => {
            keywords("auto none") || {
                let p = value_parse::split_css_values(value);
                !p.is_empty() && p.len() % 2 == 0 && p.iter().all(|s| css_string(s))
            }
        }
        CounterReset => super::apply::parse_counter_reset_checked(value).is_some(),
        CounterSet | CounterIncrement => {
            super::apply::parse_counter_list_checked(value, 0).is_some()
        }
        StrokeWidth => length(v, true, false),
        Fill | Stroke if v == "none" || v.starts_with("url(") => true,
        Color
        | BackgroundColor
        | Fill
        | Stroke
        | BorderTopColor
        | BorderRightColor
        | BorderBottomColor
        | BorderLeftColor
        | BorderBlockStartColor
        | BorderBlockEndColor
        | BorderInlineStartColor
        | BorderInlineEndColor
        | OutlineColor
        | TextDecorationColor
        | TextEmphasisColor
        | ColumnRuleColor => color(v),
        CaretColor => v == "auto" || color(v),
        AccentColor => false, // Registered but its application callback is a no-op.
        MinInlineSize | MinBlockSize | MaxInlineSize | MaxBlockSize => {
            keywords("auto none min-content max-content fit-content stretch")
                || length(v, true, false)
        }
        MarginBlockStart | MarginBlockEnd | MarginInlineStart | MarginInlineEnd => {
            v == "auto" || length(v, true, true)
        }
        PaddingBlockStart | PaddingBlockEnd | PaddingInlineStart | PaddingInlineEnd
        | ScrollPaddingTop | ScrollPaddingRight | ScrollPaddingBottom | ScrollPaddingLeft => {
            v == "auto" || length(v, true, false)
        }
        ScrollMarginTop | ScrollMarginRight | ScrollMarginBottom | ScrollMarginLeft => {
            length(v, false, true)
        }
        Margin | Inset => values(v, 1, 4, |p| p == "auto" || length(p, true, true)),
        MarginBlock | MarginInline | InsetBlock | InsetInline => {
            values(v, 1, 2, |p| p == "auto" || length(p, true, true))
        }
        Padding | ScrollPadding => values(v, 1, 4, |p| {
            (id == ScrollPadding && p == "auto") || length(p, true, false)
        }),
        PaddingBlock | PaddingInline => values(v, 1, 2, |p| length(p, true, false)),
        ScrollMargin => values(v, 1, 4, |p| length(p, false, true)),
        Overflow => list("visible hidden clip scroll auto", 2),
        OverflowClipMargin => super::parse_overflow_clip_margin(v).is_some(),
        BorderTopLeftRadius
        | BorderTopRightRadius
        | BorderBottomLeftRadius
        | BorderBottomRightRadius
        | BorderStartStartRadius
        | BorderStartEndRadius
        | BorderEndStartRadius
        | BorderEndEndRadius => values(v, 1, 2, |p| length(p, true, false)),
        BorderRadius => slash_list(v, 2, |p| values(p, 1, 4, |p| length(p, true, false))),
        BorderBlockStartStyle
        | BorderBlockEndStyle
        | BorderInlineStartStyle
        | BorderInlineEndStyle
        | OutlineStyle
        | ColumnRuleStyle => value_parse::try_parse_border_style(v).is_some(),
        BorderBlockStartWidth
        | BorderBlockEndWidth
        | BorderInlineStartWidth
        | BorderInlineEndWidth
        | OutlineWidth
        | ColumnRuleWidth => keywords("thin medium thick") || length(v, false, false),
        OutlineOffset => length(v, false, true),
        ShapeMargin => super::shape::valid_shape_margin(value),
        BorderWidth => values(v, 1, 4, |p| declaration_value(OutlineWidth, p)),
        BorderStyle => values(v, 1, 4, |p| {
            value_parse::try_parse_border_style(p).is_some()
        }),
        BorderColor => values(v, 1, 4, color),
        Border | BorderTop | BorderRight | BorderBottom | BorderLeft | BorderBlock
        | BorderInline | BorderBlockStart | BorderBlockEnd | BorderInlineStart
        | BorderInlineEnd | Outline | ColumnRule => values(v, 1, 3, |p| {
            declaration_value(OutlineWidth, p)
                || value_parse::try_parse_border_style(p).is_some()
                || color(p)
        }),
        BackgroundImage | MaskImage | BorderImageSource => comma_list(value, image),
        BackgroundSize | MaskSize => comma_list(v, |p| {
            matches!(p, "cover" | "contain")
                || values(p, 1, 2, |n| n == "auto" || length(n, true, false))
        }),
        BackgroundRepeat | MaskRepeat => comma_list(v, |p| {
            keyword_list(p, "repeat no-repeat repeat-x repeat-y round space", 2)
        }),
        BackgroundAttachment => comma_list(v, |p| keyword_list(p, "scroll fixed local", 1)),
        BackgroundOrigin | MaskOrigin => comma_list(v, |p| {
            keyword_list(
                p,
                "border-box padding-box content-box fill-box stroke-box view-box",
                1,
            )
        }),
        BackgroundClip | MaskClip => comma_list(v, |p| {
            keyword_list(
                p,
                "border-box padding-box content-box text fill-box stroke-box view-box no-clip",
                1,
            )
        }),
        MaskMode => comma_list(v, |p| keyword_list(p, "match-source alpha luminance", 1)),
        MaskComposite => comma_list(v, |p| keyword_list(p, "add subtract intersect exclude", 1)),
        BorderImageRepeat => list("stretch repeat round space", 2),
        BorderImageSlice => values(v, 1, 5, |p| {
            p == "fill" || calc::parse_nonnegative_number(p.trim_end_matches('%')).is_some()
        }),
        BorderImageWidth => values(v, 1, 4, |p| {
            p == "auto" || calc::parse_nonnegative_number(p).is_some() || length(p, true, false)
        }),
        BorderImageOutset => values(v, 1, 4, |p| {
            calc::parse_nonnegative_number(p).is_some() || length(p, false, false)
        }),
        BorderImage => slash_list(v, 3, |p| {
            values(p, 1, 8, |t| {
                image(t)
                    || declaration_value(BorderImageSlice, t)
                    || declaration_value(BorderImageWidth, t)
                    || declaration_value(BorderImageRepeat, t)
            })
        }),
        Background | Mask => comma_list(value, |layer| {
            let tokens = value_parse::split_css_values(layer);
            !tokens.is_empty()
                && tokens.iter().all(|t| {
                    t == "/"
                        || image(t)
                        || color(t)
                        || [
                            BackgroundRepeat,
                            BackgroundAttachment,
                            BackgroundOrigin,
                            BackgroundClip,
                            BackgroundSize,
                            BackgroundPosition,
                            MaskMode,
                            MaskComposite,
                        ]
                        .iter()
                        .any(|id| declaration_value(*id, t))
                })
        }),
        ClipPath => super::property_defs::valid_clip_path(value),
        ShapeOutside => super::shape::valid_shape_outside_non_image(value) || image(value),
        Clip => {
            v == "auto"
                || v.strip_prefix("rect(")
                    .and_then(|p| p.strip_suffix(')'))
                    .is_some_and(|p| {
                        let p = p.replace(',', " ");
                        values(&p, 4, 4, |t| t == "auto" || length(t, false, true))
                    })
        }
        MarginTrim => {
            v == "none"
                || list(
                    "block inline block-start block-end inline-start inline-end",
                    4,
                )
        }
        // These currently only retain their tokens; no used-value consumer
        // implements the feature. Registration alone must not advertise it.
        AnchorName | PositionAnchor | ViewTransitionName | AnimationTimeline | ScrollTimeline
        | OffsetPath => false,
        ZIndex if v == "auto" => true,
        Opacity => v.strip_suffix('%').map_or_else(
            || calc::parse_css_number(v).is_some(),
            |p| calc::parse_css_number(p).is_some(),
        ),
        Display
        | Position
        | Float
        | Clear
        | BoxSizing
        | OverflowX
        | OverflowY
        | Visibility
        | ZIndex
        | Width
        | Height
        | MinWidth
        | MinHeight
        | MaxWidth
        | MaxHeight
        | MarginTop
        | MarginRight
        | MarginBottom
        | MarginLeft
        | PaddingTop
        | PaddingRight
        | PaddingBottom
        | PaddingLeft
        | BorderTopWidth
        | BorderRightWidth
        | BorderBottomWidth
        | BorderLeftWidth
        | BorderTopStyle
        | BorderRightStyle
        | BorderBottomStyle
        | BorderLeftStyle
        | Top
        | Right
        | Bottom
        | Left
        | LineHeight
        | LetterSpacing
        | WordSpacing
        | TextAlign
        | TextDecorationThickness
        | TextTransform
        | TextIndent
        | TextUnderlineOffset
        | WhiteSpace
        | WordBreak
        | VerticalAlign
        | FlexDirection
        | FlexWrap
        | FlexGrow
        | FlexShrink
        | FlexBasis
        | Order
        | InlineSize
        | BlockSize
        | InsetBlockStart
        | InsetBlockEnd
        | InsetInlineStart
        | InsetInlineEnd => !matches!(
            super::pre_parse_value(id, v),
            crate::types::CssValue::Raw(_)
        ),
        Content | Unknown => false, // Content has its own structured validator.
    }
}

fn keyword_list(v: &str, words: &str, max: usize) -> bool {
    let tokens: Vec<_> = v.split_whitespace().collect();
    !tokens.is_empty()
        && tokens.len() <= max
        && tokens
            .iter()
            .all(|token| words.split_whitespace().any(|word| word == *token))
}

fn values(v: &str, min: usize, max: usize, check: impl Fn(&str) -> bool) -> bool {
    let tokens = value_parse::split_css_values(v);
    (min..=max).contains(&tokens.len()) && tokens.iter().all(|token| check(token))
}

fn comma_list(v: &str, check: impl Fn(&str) -> bool) -> bool {
    value_parse::split_top_level_commas(v)
        .iter()
        .all(|part| !part.trim().is_empty() && check(part.trim()))
}

fn slash_list(v: &str, max: usize, check: impl Fn(&str) -> bool) -> bool {
    let parts: Vec<_> = v.split('/').collect();
    parts.len() <= max
        && parts
            .iter()
            .all(|part| !part.trim().is_empty() && check(part.trim()))
}

fn length(v: &str, percent: bool, negative: bool) -> bool {
    if v.parse::<f32>().is_ok_and(|n| n != 0.0)
        || (!percent && v.ends_with('%'))
        || (!negative && v.starts_with('-'))
    {
        return false;
    }
    value_parse::parse_length_checked(v).is_some_and(|n| {
        !matches!(
            n,
            crate::types::CssLength::Auto
                | crate::types::CssLength::None
                | crate::types::CssLength::MinContent
                | crate::types::CssLength::MaxContent
                | crate::types::CssLength::FitContent
        )
    })
}

fn color(v: &str) -> bool {
    if let Some(inner) = v
        .strip_prefix("light-dark(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let parts = value_parse::split_top_level_commas(inner);
        return parts.len() == 2 && parts.iter().all(|p| color(p.trim()));
    }
    value_parse::parse_color(v).is_some()
}

fn position(v: &str) -> bool {
    values(v, 1, 4, |p| {
        keyword_list(p, "left right top bottom center", 1) || length(p, true, true)
    })
}

fn ident(v: &str) -> bool {
    let mut chars = v.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '-' || c == '\\')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '\\'))
}

fn identifiers(v: &str) -> bool {
    !v.is_empty() && v.split_whitespace().all(ident)
}
fn css_string(v: &str) -> bool {
    super::apply::consume_css_string(v).is_some_and(|(_, rest)| rest.trim().is_empty())
}

fn grid_line(v: &str) -> bool {
    if v == "auto" {
        return true;
    }
    let parts = value_parse::split_css_values(v);
    !parts.is_empty()
        && parts.len() <= 3
        && parts
            .iter()
            .all(|p| ident(p) || calc::parse_css_integer(p).is_some_and(|n| n != 0))
}

fn font_shorthand(v: &str) -> bool {
    if matches!(
        v,
        "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar"
    ) {
        return true;
    }
    let parts = value_parse::split_css_values(v);
    let Some(size) = parts.iter().position(|p| {
        value_parse::parse_font_size_checked(p.split('/').next().unwrap_or("")).is_some()
    }) else {
        return false;
    };
    size + 1 < parts.len()
        && parts[..size].iter().all(|p| {
            [
                PropertyId::FontStyle,
                PropertyId::FontWeight,
                PropertyId::FontStretch,
                PropertyId::FontVariantCaps,
            ]
            .iter()
            .any(|id| declaration_value(*id, p))
        })
}

pub(crate) fn image(v: &str) -> bool {
    if v == "none"
        || super::extract_url(v).is_some()
        || super::property_defs::extract_image_set_url_for_device_pixel_ratio(v, 1.0).is_some()
    {
        return true;
    }
    let mut style = crate::types::ComputedStyle::default();
    super::apply::apply_gradient(&mut style, v);
    style.rare().gradient_stops.len() >= 2
}

fn shadow(v: &str, box_shadow: bool) -> bool {
    v == "none"
        || comma_list(v, |part| {
            let mut lengths = 0;
            let mut colors = 0;
            let mut inset = 0;
            for p in value_parse::split_css_values(part) {
                if length(&p, false, true) {
                    lengths += 1;
                } else if color(&p) {
                    colors += 1;
                } else if box_shadow && p == "inset" {
                    inset += 1;
                } else {
                    return false;
                }
            }
            (2..=if box_shadow { 4 } else { 3 }).contains(&lengths) && colors <= 1 && inset <= 1
        })
}

fn grid_areas(v: &str) -> bool {
    let mut rest = v.trim();
    while !rest.is_empty() {
        let Some((_, tail)) = super::apply::consume_css_string(rest) else {
            return false;
        };
        rest = tail.trim_start();
    }
    let rows = super::grid_parse::parse_grid_template_areas(v);
    !rows.is_empty() && !rows[0].is_empty() && rows.iter().all(|row| row.len() == rows[0].len())
}

fn track(v: &str) -> bool {
    if matches!(v, "auto" | "min-content" | "max-content") || length(v, true, false) {
        return true;
    }
    if let Some(n) = v.strip_suffix("fr") {
        return calc::parse_nonnegative_number(n).is_some();
    }
    if let Some(inner) = v.strip_prefix("minmax(").and_then(|v| v.strip_suffix(')')) {
        let parts = value_parse::split_top_level_commas(inner);
        return parts.len() == 2
            && !parts[0].trim().ends_with("fr")
            && parts.iter().all(|p| track(p.trim()));
    }
    v.strip_prefix("fit-content(")
        .and_then(|v| v.strip_suffix(')'))
        .is_some_and(|p| length(p.trim(), true, false))
}

fn track_sizes(v: &str) -> bool {
    values(v, 1, usize::MAX, track)
}

fn track_list(v: &str) -> bool {
    if matches!(v, "none" | "subgrid") {
        return true;
    }
    // Use the grid tokenizer so spaces inside functions and line-name lists
    // cannot be mistaken for track boundaries.
    let tokens = super::grid_parse::tokenize_track_list(v);
    let mut has_track = false;
    for t in &tokens {
        if let Some(names) = t.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            if !names.is_empty() && !identifiers(names) {
                return false;
            }
        } else if let Some(inner) = t.strip_prefix("repeat(").and_then(|t| t.strip_suffix(')')) {
            let parts = value_parse::split_top_level_commas(inner);
            if parts.len() != 2
                || !(matches!(parts[0].trim(), "auto-fill" | "auto-fit")
                    || calc::parse_positive_integer(parts[0].trim()).is_some())
                || parts[1].contains("repeat(")
                || !track_list(parts[1].trim())
            {
                return false;
            }
            has_track = true;
        } else if track(t) {
            has_track = true;
        } else {
            return false;
        }
    }
    has_track
}
