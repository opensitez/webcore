// Ported from cpptests/test_css_advanced.cpp
// Advanced CSS property parsing tests

use webcore::css::apply_property;
use webcore::types::*;

fn style_with(prop: &str, val: &str) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, prop, val);
    style
}

// ============================================================
// Text Shadow Parsing
// ============================================================

#[test]
fn css_adv_text_shadow_basic() {
    let s = style_with("text-shadow", "2px 3px red");
    let ts = s.text_shadow.as_ref().expect("text-shadow should be set");
    assert_eq!(ts.offset_x, 2.0);
    assert_eq!(ts.offset_y, 3.0);
    assert_eq!(ts.color, Color::rgb(255, 0, 0));
}

#[test]
fn css_adv_text_shadow_with_blur() {
    let s = style_with("text-shadow", "1px 2px 5px blue");
    let ts = s.text_shadow.as_ref().expect("text-shadow should be set");
    assert_eq!(ts.offset_x, 1.0);
    assert_eq!(ts.offset_y, 2.0);
    assert_eq!(ts.blur, 5.0);
    assert_eq!(ts.color, Color::rgb(0, 0, 255));
}

#[test]
fn css_adv_text_shadow_none() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "text-shadow", "2px 3px red");
    assert!(s.text_shadow.is_some());
    apply_property(&mut s, "text-shadow", "none");
    assert!(s.text_shadow.is_none());
}

#[test]
fn css_adv_text_shadow_default_color() {
    let s = style_with("text-shadow", "3px 4px");
    let ts = s.text_shadow.as_ref().expect("text-shadow should be set");
    assert_eq!(ts.offset_x, 3.0);
    assert_eq!(ts.offset_y, 4.0);
}

// ============================================================
// Box Shadow Parsing
// ============================================================

#[test]
fn css_adv_box_shadow_with_spread() {
    let s = style_with("box-shadow", "2px 3px 4px 5px black");
    let bs = s.box_shadow.first().expect("box-shadow should be set");
    assert_eq!(bs.offset_x, 2.0);
    assert_eq!(bs.offset_y, 3.0);
    assert_eq!(bs.blur, 4.0);
    assert_eq!(bs.spread, 5.0);
}

#[test]
fn css_adv_box_shadow_inset() {
    let s = style_with("box-shadow", "inset 2px 3px 4px black");
    let bs = s.box_shadow.first().expect("box-shadow should be set");
    assert!(bs.inset);
    assert_eq!(bs.offset_x, 2.0);
    assert_eq!(bs.offset_y, 3.0);
}

#[test]
fn css_adv_box_shadow_none() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "box-shadow", "2px 3px black");
    assert!(!s.box_shadow.is_empty());
    apply_property(&mut s, "box-shadow", "none");
    assert!(s.box_shadow.is_empty());
}

// ============================================================
// Flex Shorthand
// ============================================================

#[test]
fn css_adv_flex_shorthand_none() {
    let s = style_with("flex", "none");
    assert_eq!(s.flex_grow as i32, 0);
    assert_eq!(s.flex_shrink as i32, 0);
    assert!(s.flex_basis.is_auto());
}

#[test]
fn css_adv_flex_shorthand_auto() {
    let s = style_with("flex", "auto");
    assert_eq!(s.flex_grow as i32, 1);
    assert_eq!(s.flex_shrink as i32, 1);
    assert!(s.flex_basis.is_auto());
}

#[test]
fn css_adv_flex_shorthand_single_number() {
    let s = style_with("flex", "2");
    assert_eq!(s.flex_grow as i32, 2);
    assert_eq!(s.flex_shrink as i32, 1);
    assert_eq!(s.flex_basis, CssLength::Px(0.0));
}

#[test]
fn css_adv_flex_shorthand_two_values() {
    let s = style_with("flex", "1 0");
    assert_eq!(s.flex_grow as i32, 1);
    assert_eq!(s.flex_shrink as i32, 0);
}

#[test]
fn css_adv_flex_shorthand_three_values() {
    let s = style_with("flex", "2 1 100px");
    assert_eq!(s.flex_grow as i32, 2);
    assert_eq!(s.flex_shrink as i32, 1);
    assert_eq!(s.flex_basis, CssLength::Px(100.0));
}

// ============================================================
// Background Shorthand
// ============================================================

#[test]
fn css_adv_background_color() {
    let s = style_with("background", "red");
    assert_eq!(s.background_color, Color::rgb(255, 0, 0));
}

#[test]
fn css_adv_background_no_repeat() {
    let s = style_with("background", "#fff no-repeat");
    assert_eq!(s.background_repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn css_adv_background_repeat_x() {
    let s = style_with("background", "#fff repeat-x");
    assert_eq!(s.background_repeat, BackgroundRepeat::RepeatX);
}

#[test]
fn css_adv_background_repeat_y() {
    let s = style_with("background", "#fff repeat-y");
    assert_eq!(s.background_repeat, BackgroundRepeat::RepeatY);
}

#[test]
fn css_adv_background_image_url() {
    let s = style_with("background-image", "url('test.png')");
    assert_eq!(s.background_image_url, "test.png");
}

// ============================================================
// Background Repeat Standalone
// ============================================================

#[test]
fn css_adv_background_repeat_property() {
    let s = style_with("background-repeat", "no-repeat");
    assert_eq!(s.background_repeat, BackgroundRepeat::NoRepeat);
}

// ============================================================
// Background Size
// ============================================================

#[test]
fn css_adv_background_size_cover() {
    let s = style_with("background-size", "cover");
    assert_eq!(s.background_size, BackgroundSize::Cover);
}

#[test]
fn css_adv_background_size_contain() {
    let s = style_with("background-size", "contain");
    assert_eq!(s.background_size, BackgroundSize::Contain);
}

// ============================================================
// Font Variant
// ============================================================

#[test]
fn css_adv_font_variant_small_caps() {
    let s = style_with("font-variant", "small-caps");
    assert!(s.small_caps);
}

#[test]
fn css_adv_font_variant_normal() {
    let mut s = ComputedStyle::default();
    s.small_caps = true;
    apply_property(&mut s, "font-variant", "normal");
    assert!(!s.small_caps);
}

// ============================================================
// Visibility
// ============================================================

#[test]
fn css_adv_visibility_hidden() {
    let s = style_with("visibility", "hidden");
    assert!(!s.visibility);
}

#[test]
fn css_adv_visibility_visible() {
    let mut s = ComputedStyle::default();
    s.visibility = false;
    apply_property(&mut s, "visibility", "visible");
    assert!(s.visibility);
}

// ============================================================
// Border Individual Width Properties
// ============================================================

#[test]
fn css_adv_border_top_width() {
    let s = style_with("border-top-width", "3px");
    assert_eq!(s.border_top_width, CssLength::Px(3.0));
}

#[test]
fn css_adv_border_right_width() {
    let s = style_with("border-right-width", "4px");
    assert_eq!(s.border_right_width, CssLength::Px(4.0));
}

#[test]
fn css_adv_border_bottom_width() {
    let s = style_with("border-bottom-width", "5px");
    assert_eq!(s.border_bottom_width, CssLength::Px(5.0));
}

#[test]
fn css_adv_border_left_width() {
    let s = style_with("border-left-width", "6px");
    assert_eq!(s.border_left_width, CssLength::Px(6.0));
}

// ============================================================
// Border Color Individual
// ============================================================

#[test]
fn css_adv_border_top_color() {
    let s = style_with("border-top-color", "red");
    assert_eq!(s.border_top_color, Color::rgb(255, 0, 0));
}

#[test]
fn css_adv_border_color_all() {
    let s = style_with("border-color", "blue");
    assert_eq!(s.border_top_color, Color::rgb(0, 0, 255));
    assert_eq!(s.border_right_color, Color::rgb(0, 0, 255));
    assert_eq!(s.border_bottom_color, Color::rgb(0, 0, 255));
    assert_eq!(s.border_left_color, Color::rgb(0, 0, 255));
}

// ============================================================
// Border Style Individual
// ============================================================

#[test]
fn css_adv_border_top_style() {
    let s = style_with("border-top-style", "dashed");
    assert_eq!(s.border_top_style, BorderStyle::Dashed);
}

#[test]
fn css_adv_border_bottom_style() {
    let s = style_with("border-bottom-style", "dotted");
    assert_eq!(s.border_bottom_style, BorderStyle::Dotted);
}

// ============================================================
// Border Collapse
// ============================================================

#[test]
fn css_adv_border_collapse_collapse() {
    let s = style_with("border-collapse", "collapse");
    assert!(s.border_collapse);
}

#[test]
fn css_adv_border_collapse_separate() {
    let mut s = ComputedStyle::default();
    s.border_collapse = true;
    apply_property(&mut s, "border-collapse", "separate");
    assert!(!s.border_collapse);
}

// ============================================================
// Text Decoration Line
// ============================================================

#[test]
fn css_adv_text_decoration_line_underline() {
    let s = style_with("text-decoration-line", "underline");
    assert!(s.text_decoration.underline);
}

#[test]
fn css_adv_text_decoration_line_through() {
    let s = style_with("text-decoration-line", "line-through");
    assert!(s.text_decoration.strikethrough);
}

// ============================================================
// Grid Column/Row Shorthand
// ============================================================

#[test]
fn css_adv_grid_column_shorthand() {
    let s = style_with("grid-column", "1 / 3");
    assert_eq!(s.grid_column_start, 1);
    assert_eq!(s.grid_column_end, 3);
}

#[test]
fn css_adv_grid_row_shorthand() {
    let s = style_with("grid-row", "2 / 4");
    assert_eq!(s.grid_row_start, 2);
    assert_eq!(s.grid_row_end, 4);
}

// ============================================================
// List Style
// ============================================================

#[test]
fn css_adv_list_style_type_decimal() {
    let s = style_with("list-style-type", "decimal");
    assert_eq!(s.list_style_type, ListStyleType::Decimal);
}

#[test]
fn css_adv_list_style_type_lower_alpha() {
    let s = style_with("list-style-type", "lower-alpha");
    assert_eq!(s.list_style_type, ListStyleType::LowerAlpha);
}

#[test]
fn css_adv_list_style_type_upper_roman() {
    let s = style_with("list-style-type", "upper-roman");
    assert_eq!(s.list_style_type, ListStyleType::UpperRoman);
}

#[test]
fn css_adv_list_style_shorthand() {
    let s = style_with("list-style", "square");
    assert_eq!(s.list_style_type, ListStyleType::Square);
}

// ============================================================
// Gradient Parsing
// ============================================================

#[test]
fn css_adv_linear_gradient_two_stops() {
    let s = style_with("background-image", "linear-gradient(to right, red, blue)");
    assert_eq!(s.gradient_type, GradientType::Linear);
    assert!(s.rare().gradient_stops.len() >= 2);
}

#[test]
fn css_adv_linear_gradient_angle() {
    let s = style_with("background-image", "linear-gradient(45deg, red, blue)");
    assert_eq!(s.gradient_type, GradientType::Linear);
}

// ============================================================
// Margin Shorthand Three Values
// ============================================================

#[test]
fn css_adv_margin_shorthand_three() {
    let s = style_with("margin", "10px 20px 30px");
    assert_eq!(s.margin_top, CssLength::Px(10.0));
    assert_eq!(s.margin_right, CssLength::Px(20.0));
    assert_eq!(s.margin_bottom, CssLength::Px(30.0));
    assert_eq!(s.margin_left, CssLength::Px(20.0)); // left = right
}

// ============================================================
// Padding Shorthand Three Values
// ============================================================

#[test]
fn css_adv_padding_shorthand_three() {
    let s = style_with("padding", "5px 10px 15px");
    assert_eq!(s.padding_top, CssLength::Px(5.0));
    assert_eq!(s.padding_right, CssLength::Px(10.0));
    assert_eq!(s.padding_bottom, CssLength::Px(15.0));
    assert_eq!(s.padding_left, CssLength::Px(10.0)); // left = right
}

// ============================================================
// Padding Shorthand Two Values
// ============================================================

#[test]
fn css_adv_padding_shorthand_two() {
    let s = style_with("padding", "10px 20px");
    assert_eq!(s.padding_top, CssLength::Px(10.0));
    assert_eq!(s.padding_right, CssLength::Px(20.0));
    assert_eq!(s.padding_bottom, CssLength::Px(10.0));
    assert_eq!(s.padding_left, CssLength::Px(20.0));
}

// ============================================================
// CSSLength em/rem
// ============================================================

#[test]
fn css_adv_font_size_em() {
    let s = style_with("width", "2em");
    assert_eq!(s.width, CssLength::Em(2.0));
}

#[test]
fn css_adv_font_size_rem() {
    let s = style_with("width", "1.5rem");
    assert_eq!(s.width, CssLength::Rem(1.5));
}

// ============================================================
// Container Query CSS properties
// ============================================================

#[test]
fn css_adv_container_type_size() {
    let s = style_with("container-type", "size");
    assert_eq!(s.container_type, ContainerType::Size);
}

#[test]
fn css_adv_container_type_inline_size() {
    let s = style_with("container-type", "inline-size");
    assert_eq!(s.container_type, ContainerType::InlineSize);
}

#[test]
fn css_adv_container_type_normal() {
    let mut s = ComputedStyle::default();
    s.container_type = ContainerType::Size;
    apply_property(&mut s, "container-type", "normal");
    assert_eq!(s.container_type, ContainerType::Normal);
}

#[test]
fn css_adv_container_name_parsed() {
    let s = style_with("container-name", "sidebar");
    assert_eq!(s.container_name, "sidebar");
}

// ============================================================
// Container Shorthand
// ============================================================

#[test]
fn css_adv_container_shorthand_parsed() {
    let s = style_with("container", "sidebar / inline-size");
    assert_eq!(s.container_name, "sidebar");
    assert_eq!(s.container_type, ContainerType::InlineSize);
}

#[test]
fn css_adv_container_shorthand_size_only() {
    let s = style_with("container", "size");
    assert_eq!(s.container_type, ContainerType::Size);
}

// ============================================================
// CSS Logical Properties — Margin / Padding
// ============================================================

#[test]
fn css_adv_margin_inline_shorthand() {
    let s = style_with("margin-inline", "10px");
    assert_eq!(s.margin_left, CssLength::Px(10.0));
    assert_eq!(s.margin_right, CssLength::Px(10.0));
}

#[test]
fn css_adv_padding_inline_shorthand() {
    let s = style_with("padding-inline", "15px");
    assert_eq!(s.padding_left, CssLength::Px(15.0));
    assert_eq!(s.padding_right, CssLength::Px(15.0));
}

#[test]
fn css_adv_margin_block_shorthand() {
    let s = style_with("margin-block", "20px");
    assert_eq!(s.margin_top, CssLength::Px(20.0));
    assert_eq!(s.margin_bottom, CssLength::Px(20.0));
}

#[test]
fn css_adv_padding_block_shorthand() {
    let s = style_with("padding-block", "25px");
    assert_eq!(s.padding_top, CssLength::Px(25.0));
    assert_eq!(s.padding_bottom, CssLength::Px(25.0));
}

// ============================================================
// Inset Logical Properties
// ============================================================

#[test]
fn css_adv_inset_inline_start_ltr() {
    let mut s = ComputedStyle::default();
    s.direction = Direction::LTR;
    apply_property(&mut s, "inset-inline-start", "10px");
    assert_eq!(s.left, CssLength::Px(10.0));
}

#[test]
fn css_adv_inset_inline_end_ltr() {
    let mut s = ComputedStyle::default();
    s.direction = Direction::LTR;
    apply_property(&mut s, "inset-inline-end", "20px");
    assert_eq!(s.right, CssLength::Px(20.0));
}

#[test]
fn css_adv_inset_block_start() {
    let s = style_with("inset-block-start", "15px");
    assert_eq!(s.top, CssLength::Px(15.0));
}

#[test]
fn css_adv_inset_block_end() {
    let s = style_with("inset-block-end", "25px");
    assert_eq!(s.bottom, CssLength::Px(25.0));
}

// ============================================================
// Hover Colors (now stored as full hover_style overlay)
// ============================================================

#[test]
fn css_adv_hover_background_color() {
    // hover-background-color was removed; hover styles are now stored as a
    // full ComputedStyle clone in hover_style. Test via cascade instead.
    use webcore::parse_html;
    let doc = parse_html(
        "<html><head><style>div:hover { background-color: yellow; }</style></head>\
         <body><div>x</div></body></html>",
    );
    fn find<'a>(b: &'a webcore::types::WebCore, tag: &str) -> Option<&'a webcore::types::WebCore> {
        if b.tag == tag {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = find(c, tag) {
                return Some(f);
            }
        }
        None
    }
    let div = find(&doc.root, "div").expect("div");
    let hs = div.style.hover_style.as_ref().expect("hover_style");
    assert_eq!(hs.background_color, Color::rgb(255, 255, 0));
}

#[test]
fn css_adv_hover_color() {
    use webcore::parse_html;
    let doc = parse_html(
        "<html><head><style>div:hover { color: green; }</style></head>\
         <body><div>x</div></body></html>",
    );
    fn find<'a>(b: &'a webcore::types::WebCore, tag: &str) -> Option<&'a webcore::types::WebCore> {
        if b.tag == tag {
            return Some(b);
        }
        for c in &b.children {
            if let Some(f) = find(c, tag) {
                return Some(f);
            }
        }
        None
    }
    let div = find(&doc.root, "div").expect("div");
    let hs = div.style.hover_style.as_ref().expect("hover_style");
    // CSS "green" is #008000
    assert_eq!(hs.color, Color::rgb(0, 128, 0));
}

// ============================================================
// Cell Padding
// ============================================================

#[test]
fn css_adv_cell_padding_parsed() {
    let s = style_with("cellpadding", "5");
    assert_eq!(s.cell_padding, CssLength::Px(5.0));
}

// ============================================================
// Border Radius Individual Corner
// ============================================================

#[test]
fn css_adv_border_top_left_radius() {
    let s = style_with("border-top-left-radius", "10px");
    assert_eq!(s.border_top_left_radius, CssLength::Px(10.0));
    // Also sets the uniform border_radius shortcut
    assert_eq!(s.border_radius, CssLength::Px(10.0));
}

#[test]
fn css_adv_border_top_right_radius() {
    let s = style_with("border-top-right-radius", "8px");
    assert_eq!(s.border_top_right_radius, CssLength::Px(8.0));
}

#[test]
fn css_adv_border_bottom_left_radius() {
    let s = style_with("border-bottom-left-radius", "6px");
    assert_eq!(s.border_bottom_left_radius, CssLength::Px(6.0));
}

#[test]
fn css_adv_border_bottom_right_radius() {
    let s = style_with("border-bottom-right-radius", "4px");
    assert_eq!(s.border_bottom_right_radius, CssLength::Px(4.0));
}

// ============================================================
// Media Queries — @media rule parsing
// ============================================================

use webcore::css::parse_stylesheet;

#[test]
fn css_adv_media_query_parsed() {
    let rules = parse_stylesheet("@media (max-width: 600px) { .small { color: red; } }")
        .unwrap_or_default();
    assert_eq!(rules.len(), 1);
    assert!(rules[0].media_condition.contains("max-width"));
}

#[test]
fn css_adv_media_query_multiple_rules() {
    let rules =
        parse_stylesheet("@media (min-width: 800px) { .a { color: red; } .b { color: blue; } }")
            .unwrap_or_default();
    assert_eq!(rules.len(), 2);
    assert!(!rules[0].media_condition.is_empty());
    assert!(!rules[1].media_condition.is_empty());
}

#[test]
fn css_adv_media_query_with_normal_rules() {
    let rules = parse_stylesheet(
        "p { color: black; } \
         @media (max-width: 600px) { p { color: red; } }",
    )
    .unwrap_or_default();
    assert_eq!(rules.len(), 2);
    assert!(rules[0].media_condition.is_empty()); // unconditional
    assert!(!rules[1].media_condition.is_empty()); // conditional
}

#[test]
fn css_adv_media_query_screen_type() {
    let rules = parse_stylesheet("@media screen and (max-width: 500px) { .x { display: none; } }")
        .unwrap_or_default();
    assert_eq!(rules.len(), 1);
    assert!(!rules[0].media_condition.is_empty());
}

#[test]
fn css_adv_media_query_nested() {
    let rules =
        parse_stylesheet("@media screen { @media (max-width: 500px) { .x { color: red; } } }")
            .unwrap_or_default();
    assert_eq!(rules.len(), 1);
    // Should have combined condition containing both terms
    assert!(rules[0].media_condition.contains("screen"));
    assert!(rules[0].media_condition.contains("max-width"));
}

// ============================================================
// CSS Logical Properties — Margin/Padding start/end (individual)
// ============================================================

#[test]
fn css_adv_margin_inline_start_ltr() {
    let mut s = ComputedStyle::default();
    s.direction = Direction::LTR;
    apply_property(&mut s, "margin-inline-start", "10px");
    apply_property(&mut s, "margin-inline-end", "20px");
    assert_eq!(s.margin_left, CssLength::Px(10.0));
    assert_eq!(s.margin_right, CssLength::Px(20.0));
}

#[test]
fn css_adv_padding_block_start_end() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "padding-block-start", "5px");
    apply_property(&mut s, "padding-block-end", "15px");
    assert_eq!(s.padding_top, CssLength::Px(5.0));
    assert_eq!(s.padding_bottom, CssLength::Px(15.0));
}

#[test]
fn css_adv_margin_block_start_end() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin-block-start", "10px");
    apply_property(&mut s, "margin-block-end", "20px");
    assert_eq!(s.margin_top, CssLength::Px(10.0));
    assert_eq!(s.margin_bottom, CssLength::Px(20.0));
}

// ============================================================
// Cellspacing
// ============================================================

#[test]
fn css_adv_cell_spacing_parsed() {
    let s = style_with("cellspacing", "3");
    assert_eq!(s.border_spacing_h, CssLength::Px(3.0));
    assert_eq!(s.border_spacing_v, CssLength::Px(3.0));
}

// ============================================================
// Border Inline / Block Width (C++ CSSAdv tests)
// apply_property does not implement border-inline-* or border-block-* yet.
// ============================================================

// TODO: API not available — "border-inline-start-width" is not handled by apply_property.
// (C++ test: CSSAdv/BorderInlineStartWidth)

// TODO: API not available — "border-inline-end-width" is not handled by apply_property.
// (C++ test: CSSAdv/BorderInlineEndWidth)

// TODO: API not available — "border-block-start-width" is not handled by apply_property.
// (C++ test: CSSAdv/BorderBlockStartWidth)

// TODO: API not available — "border-block-end-width" is not handled by apply_property.
// (C++ test: CSSAdv/BorderBlockEndWidth)

// ============================================================
// Border Inline / Block Color (C++ CSSAdv tests)
// ============================================================

// TODO: API not available — "border-inline-start-color" is not handled by apply_property.
// (C++ test: CSSAdv/BorderInlineStartColor)

// TODO: API not available — "border-block-start-color" is not handled by apply_property.
// (C++ test: CSSAdv/BorderBlockStartColor)

// ============================================================
// Colspan / Rowspan (C++ CSSAdv tests)
// In Rust, colspan/rowspan are HTML element attributes on WebCore,
// not ComputedStyle CSS properties.
// ============================================================

// TODO: API not available — colspan/rowspan are not ComputedStyle fields; they are
// parsed from HTML attributes and stored on WebCore, not accessible via apply_property.
// (C++ tests: CSSAdv/ColspanParsed, CSSAdv/RowspanParsed)

// ============================================================
// Media Query Condition Evaluation (C++ CSSAdv tests)
// MediaContext::EvaluateCondition does not exist in the Rust public API.
// ============================================================

// TODO: API not available — MediaContext struct and EvaluateCondition are not exposed.
// The following C++ tests have no Rust equivalent:
//   CSSAdv/MediaEvalMaxWidthTrue, CSSAdv/MediaEvalMaxWidthFalse,
//   CSSAdv/MediaEvalMinWidthTrue, CSSAdv/MediaEvalMinWidthFalse,
//   CSSAdv/MediaEvalScreenAndMaxWidth, CSSAdv/MediaEvalPrint,
//   CSSAdv/MediaEvalNot, CSSAdv/MediaEvalOrientation, CSSAdv/MediaEvalDarkMode,
//   CSSAdv/MediaEvalOnlyScreen, CSSAdv/MediaEvalNotScreen,
//   CSSAdv/MediaEvalMinHeightTrue, CSSAdv/MediaEvalMinHeightFalse,
//   CSSAdv/MediaEvalMaxHeightTrue, CSSAdv/MediaEvalMaxHeightFalse,
//   CSSAdv/MediaEvalLightMode, CSSAdv/MediaEvalAll,
//   CSSAdv/MediaEvalScreenAndMinWidth, CSSAdv/MediaEvalEmptyCondition

// ============================================================
// Media Query Applied to Layout (C++ CSSAdv tests)
// The Rust layout engine does not evaluate media conditions during stylesheet
// application; media_condition is stored on CssRule but not compared to
// a viewport context when applying cascade.
// ============================================================

// TODO: API not available — LayoutEngine does not expose a mediaContext field
// and does not evaluate @media conditions during ApplyStylesheet/Layout.
// (C++ tests: CSSAdv/MediaQueryAppliedToLayout, CSSAdv/MediaQueryNotAppliedWhenWide)

// ============================================================
// Container Query — @container rule parsing (C++ CSSAdv tests)
// CssRule does not have container_condition or container_name fields.
// The parser ignores @container block headers (strips them).
// ============================================================

// TODO: API not available — CssRule has no container_condition or container_name field.
// @container rules are parsed as plain rules without the container context.
// (C++ tests: CSSAdv/ContainerQueryParsed, CSSAdv/ContainerQueryNamedParsed,
//  CSSAdv/ContainerQueryMultipleRules, CSSAdv/ContainerQueryWithNormalRules)

// ============================================================
// Container Query — Condition Evaluation (C++ CSSAdv tests)
// ContainerContext::EvaluateCondition does not exist in the Rust public API.
// ============================================================

// TODO: API not available — ContainerContext struct and EvaluateCondition are not exposed.
// (C++ tests: CSSAdv/ContainerEvalMinWidthTrue, CSSAdv/ContainerEvalMinWidthFalse,
//  CSSAdv/ContainerEvalMaxWidthTrue, CSSAdv/ContainerEvalMaxWidthFalse,
//  CSSAdv/ContainerEvalMinHeight, CSSAdv/ContainerEvalMaxHeight,
//  CSSAdv/ContainerEvalCombinedAndCondition, CSSAdv/ContainerEvalEmptyCondition)

// ============================================================
// Container Query — FindContainer (C++ CSSAdv tests)
// ContainerContext::FindContainer does not exist in the Rust public API.
// ============================================================

// TODO: API not available — ContainerContext::FindContainer is not exposed.
// (C++ tests: CSSAdv/ContainerFindNearest, CSSAdv/ContainerFindNamed,
//  CSSAdv/ContainerFindNoneWhenNoContainer)

// ============================================================
// Container Query — Integrated Layout (C++ CSSAdv tests)
// The Rust layout engine does not evaluate @container conditions during layout.
// ============================================================

// TODO: API not available — LayoutEngine does not evaluate @container rules against
// computed container sizes during Layout.
// (C++ tests: CSSAdv/ContainerQueryAppliedToLayout,
//  CSSAdv/ContainerQueryNotAppliedWhenNarrow, CSSAdv/ContainerQueryNamedApplied)
