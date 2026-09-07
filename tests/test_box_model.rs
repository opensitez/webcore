// Ported from cpptests/test_box_model.cpp
// Box model: margin, padding, border, box-sizing, width/height, colors.

use webcore::css::apply_property;
use webcore::types::*;
use webcore::{load_html, parse_html};

fn style_with(prop: &str, val: &str) -> ComputedStyle {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, prop, val);
    style
}

fn parse_and_layout(html: &str, vw: f32) -> Document {
    load_html(html, vw)
}

fn find_box<'a, F: Fn(&WebCore) -> bool>(root: &'a WebCore, pred: &F) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(b) = find_box(child, pred) {
            return Some(b);
        }
    }
    None
}

// ============================================================
// Margin Tests
// ============================================================

#[test]
fn box_model_margin_shorthand_four() {
    let s = style_with("margin", "10px 20px 30px 40px");
    assert_eq!(s.margin_top, CssLength::Px(10.0));
    assert_eq!(s.margin_right, CssLength::Px(20.0));
    assert_eq!(s.margin_bottom, CssLength::Px(30.0));
    assert_eq!(s.margin_left, CssLength::Px(40.0));
}

#[test]
fn box_model_margin_shorthand_two() {
    let s = style_with("margin", "10px 20px");
    assert_eq!(s.margin_top, CssLength::Px(10.0));
    assert_eq!(s.margin_right, CssLength::Px(20.0));
    assert_eq!(s.margin_bottom, CssLength::Px(10.0));
    assert_eq!(s.margin_left, CssLength::Px(20.0));
}

#[test]
fn box_model_margin_shorthand_one() {
    let s = style_with("margin", "15px");
    assert_eq!(s.margin_top, CssLength::Px(15.0));
    assert_eq!(s.margin_right, CssLength::Px(15.0));
    assert_eq!(s.margin_bottom, CssLength::Px(15.0));
    assert_eq!(s.margin_left, CssLength::Px(15.0));
}

#[test]
fn box_model_margin_individual() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin-top", "5px");
    apply_property(&mut s, "margin-right", "10px");
    apply_property(&mut s, "margin-bottom", "15px");
    apply_property(&mut s, "margin-left", "20px");
    assert_eq!(s.margin_top, CssLength::Px(5.0));
    assert_eq!(s.margin_right, CssLength::Px(10.0));
    assert_eq!(s.margin_bottom, CssLength::Px(15.0));
    assert_eq!(s.margin_left, CssLength::Px(20.0));
}

#[test]
fn box_model_margin_auto_center() {
    let doc = parse_and_layout(
        "<div style='width: 200px; margin-left: auto; margin-right: auto;'>Centered</div>",
        800.0,
    );
    // Find div with width ≈ 200
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with width 200 not found");
    let div = div.unwrap();
    // Centered: x ≈ (800 - 200) / 2 = 300 (accounting for body UA margin)
    assert!(
        div.layout.content_rect.x > 250.0 && div.layout.content_rect.x < 350.0,
        "centered div should be near x=300, got x={}",
        div.layout.content_rect.x
    );
}

#[test]
fn box_model_margin_auto_left_only() {
    let doc = parse_and_layout(
        "<div style='width: 200px; margin-left: auto;'>Right</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with width 200 not found");
    // margin-left:auto pushes element to the right (x > 500)
    assert!(
        div.unwrap().layout.content_rect.x > 500.0,
        "margin-left:auto should push div right, got x={}",
        div.unwrap().layout.content_rect.x
    );
}

#[test]
fn box_model_margin_auto_does_not_affect_defaults() {
    let doc = parse_and_layout("<div style='width: 200px;'>Not centered</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with width 200 not found");
    // Without auto margins, div is at the left (small x, allowing body margin ~8px)
    assert!(
        div.unwrap().layout.content_rect.x < 50.0,
        "div without auto margin should be near left, got x={}",
        div.unwrap().layout.content_rect.x
    );
}

#[test]
fn box_model_margin_percent() {
    let doc = parse_and_layout("<div style='margin-left: 10%;'>Offset</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && matches!(b.style.margin_left, CssLength::Percent(_))
    });
    assert!(div.is_some(), "div with percent margin not found");
    // 10% of 800 = 80px margin-left; resolved_margin_left should be ~80
    let ml = div.unwrap().layout.resolved_margin_left;
    assert!(
        ml >= 70.0 && ml <= 90.0,
        "10% margin-left on 800px viewport should resolve to ~80px, got {}",
        ml
    );
}

// ============================================================
// Padding Tests
// ============================================================

#[test]
fn box_model_padding_shorthand_four() {
    let s = style_with("padding", "10px 20px 30px 40px");
    assert_eq!(s.padding_top, CssLength::Px(10.0));
    assert_eq!(s.padding_right, CssLength::Px(20.0));
    assert_eq!(s.padding_bottom, CssLength::Px(30.0));
    assert_eq!(s.padding_left, CssLength::Px(40.0));
}

#[test]
fn box_model_padding_shorthand_one() {
    let s = style_with("padding", "15px");
    assert_eq!(s.padding_top, CssLength::Px(15.0));
    assert_eq!(s.padding_right, CssLength::Px(15.0));
    assert_eq!(s.padding_bottom, CssLength::Px(15.0));
    assert_eq!(s.padding_left, CssLength::Px(15.0));
}

#[test]
fn box_model_padding_individual() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "padding-top", "5px");
    apply_property(&mut s, "padding-right", "10px");
    apply_property(&mut s, "padding-bottom", "15px");
    apply_property(&mut s, "padding-left", "20px");
    assert_eq!(s.padding_top, CssLength::Px(5.0));
    assert_eq!(s.padding_right, CssLength::Px(10.0));
    assert_eq!(s.padding_bottom, CssLength::Px(15.0));
    assert_eq!(s.padding_left, CssLength::Px(20.0));
}

#[test]
fn box_model_padding_affects_layout() {
    let doc = parse_and_layout(
        "<div style='width: 200px; padding: 20px;'>Padded</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with content_rect.w ≈ 200 not found");
    let div = div.unwrap();
    // paddingRect.w = content (200) + padding-left (20) + padding-right (20) = 240
    assert!(
        div.layout.padding_rect.w >= 235.0 && div.layout.padding_rect.w <= 245.0,
        "padding_rect.w should be ~240, got {}",
        div.layout.padding_rect.w
    );
}

// ============================================================
// Border Tests
// ============================================================

#[test]
fn box_model_border_shorthand() {
    let s = style_with("border", "2px solid red");
    assert_eq!(s.border_top_width, CssLength::Px(2.0));
    assert_eq!(s.border_right_width, CssLength::Px(2.0));
    assert_eq!(s.border_bottom_width, CssLength::Px(2.0));
    assert_eq!(s.border_left_width, CssLength::Px(2.0));
    assert_eq!(s.border_top_style, BorderStyle::Solid);
    assert_eq!(s.border_top_color, Color::rgb(255, 0, 0));
}

#[test]
fn box_model_border_individual_sides() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "border-top", "1px solid black");
    apply_property(&mut s, "border-bottom", "3px dashed blue");
    assert_eq!(s.border_top_width, CssLength::Px(1.0));
    assert_eq!(s.border_top_style, BorderStyle::Solid);
    assert_eq!(s.border_bottom_width, CssLength::Px(3.0));
    assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
}

#[test]
fn box_model_border_width_only() {
    let s = style_with("border-width", "4px");
    assert_eq!(s.border_top_width, CssLength::Px(4.0));
    assert_eq!(s.border_right_width, CssLength::Px(4.0));
    assert_eq!(s.border_bottom_width, CssLength::Px(4.0));
    assert_eq!(s.border_left_width, CssLength::Px(4.0));
}

#[test]
fn box_model_border_style_only() {
    let s = style_with("border-style", "dotted");
    assert_eq!(s.border_top_style, BorderStyle::Dotted);
}

#[test]
fn box_model_border_affects_layout() {
    let doc = parse_and_layout(
        "<div style='width: 200px; border: 5px solid black;'>Bordered</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with content_rect.w ≈ 200 not found");
    let div = div.unwrap();
    // border_rect.w = content (200) + border-left (5) + border-right (5) = 210
    assert!(
        div.layout.border_rect.w >= 205.0 && div.layout.border_rect.w <= 215.0,
        "border_rect.w should be ~210, got {}",
        div.layout.border_rect.w
    );
}

// ============================================================
// Box Sizing
// ============================================================

#[test]
fn box_model_content_box_default() {
    let doc = parse_and_layout(
        "<div style='width: 200px; padding: 20px; border: 5px solid black;'>Content-box</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.box_sizing == BoxSizing::ContentBox
    });
    assert!(div.is_some(), "content-box div not found");
    let div = div.unwrap();
    assert!(
        (div.layout.content_rect.w - 200.0).abs() < 2.0,
        "content_rect.w should be 200, got {}",
        div.layout.content_rect.w
    );
    // borderRect = 200 + 40 (padding) + 10 (border) = 250
    assert!(
        div.layout.border_rect.w >= 245.0 && div.layout.border_rect.w <= 255.0,
        "border_rect.w should be ~250, got {}",
        div.layout.border_rect.w
    );
}

#[test]
fn box_model_border_box_sizing() {
    let doc = parse_and_layout(
        "<div style='box-sizing: border-box; width: 200px; padding: 20px; border: 5px solid black;'>Border-box</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.box_sizing == BoxSizing::BorderBox
    });
    assert!(div.is_some(), "border-box div not found");
    let div = div.unwrap();
    // border_rect.w should be 200
    assert!(
        div.layout.border_rect.w >= 195.0 && div.layout.border_rect.w <= 205.0,
        "border_rect.w should be ~200 with border-box, got {}",
        div.layout.border_rect.w
    );
    // content_rect.w = 200 - 40 (padding) - 10 (border) = 150
    assert!(
        div.layout.content_rect.w >= 145.0 && div.layout.content_rect.w <= 155.0,
        "content_rect.w should be ~150 with border-box, got {}",
        div.layout.content_rect.w
    );
}

// ============================================================
// Width / Height
// ============================================================

#[test]
fn box_model_explicit_width() {
    let doc = parse_and_layout("<div style='width: 300px;'>Fixed</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.w - 300.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with content_rect.w ≈ 300 not found");
}

#[test]
fn box_model_percentage_width() {
    let doc = parse_and_layout("<div style='width: 50%;'>Half</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.layout.content_rect.w > 350.0 && b.layout.content_rect.w < 450.0
    });
    assert!(div.is_some(), "div with width ≈ 50% of 800 not found");
}

#[test]
fn box_model_auto_width_fills_container() {
    let doc = parse_and_layout("<div>Full width</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.layout.content_rect.w > 700.0
    });
    assert!(div.is_some(), "div should fill container (w > 700)");
}

#[test]
fn box_model_min_width() {
    let doc = parse_and_layout(
        "<div style='width: 50px; min-width: 200px;'>Min</div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.min_width == CssLength::Px(200.0)
    });
    assert!(div.is_some(), "div with min-width: 200px not found");
    assert!(
        div.unwrap().layout.content_rect.w >= 200.0,
        "div with min-width: 200px should be at least 200 wide"
    );
}

#[test]
fn box_model_max_width() {
    let doc = parse_and_layout("<div style='max-width: 300px;'>Max</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && b.style.max_width != CssLength::None
    });
    assert!(div.is_some(), "div with max-width not found");
    assert!(
        div.unwrap().layout.content_rect.w <= 305.0,
        "div with max-width: 300px should be at most ~305 wide"
    );
}

#[test]
fn box_model_explicit_height() {
    let doc = parse_and_layout("<div style='height: 100px;'>Tall</div>", 800.0);
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "div" && (b.layout.content_rect.h - 100.0).abs() < 2.0
    });
    assert!(div.is_some(), "div with height ≈ 100px not found");
}

// ============================================================
// Colors
// ============================================================

#[test]
fn box_model_named_colors() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "color", "red");
    assert_eq!(s.color, Color::rgb(255, 0, 0));
    apply_property(&mut s, "color", "blue");
    assert_eq!(s.color, Color::rgb(0, 0, 255));
    apply_property(&mut s, "color", "green");
    assert_eq!(s.color, Color::rgb(0, 128, 0));
    apply_property(&mut s, "color", "white");
    assert_eq!(s.color, Color::rgb(255, 255, 255));
    apply_property(&mut s, "color", "black");
    assert_eq!(s.color, Color::rgb(0, 0, 0));
}

#[test]
fn box_model_hex_colors_3() {
    let s = style_with("color", "#f00");
    assert_eq!(s.color, Color::rgb(255, 0, 0));
}

#[test]
fn box_model_hex_colors_6() {
    let s = style_with("color", "#00ff00");
    assert_eq!(s.color, Color::rgb(0, 255, 0));
}

#[test]
fn box_model_rgb_colors() {
    let s = style_with("color", "rgb(128, 64, 32)");
    assert_eq!(s.color, Color::rgb(128, 64, 32));
}

#[test]
fn box_model_rgba_colors() {
    let s = style_with("color", "rgba(255, 0, 0, 0.5)");
    assert_eq!(s.color.r, 255);
    assert_eq!(s.color.g, 0);
    assert_eq!(s.color.b, 0);
}

#[test]
fn box_model_background_color() {
    let s = style_with("background-color", "#336699");
    assert_eq!(s.background_color, Color::rgb(0x33, 0x66, 0x99));
}

#[test]
fn box_model_background_color_from_html() {
    let doc = parse_html("<div style='background-color: yellow;'>Yellow</div>");
    let div = find_box(&doc.root, &|b: &WebCore| {
        b.style.background_color == Color::rgb(255, 255, 0)
    });
    assert!(div.is_some(), "div with yellow background not found");
}

#[test]
fn box_model_color_inheritance_via_stylesheet() {
    let doc = parse_and_layout(
        "<html><head><style>.parent { color: red; }</style></head>\
         <body><div class='parent'><p>Inherits red</p></div></body></html>",
        800.0,
    );
    // Parent div should have red color applied
    let parent = find_box(&doc.root, &|b: &WebCore| {
        b.style.color == Color::rgb(255, 0, 0)
            && b.attributes
                .get("class")
                .map(|v| v == "parent")
                .unwrap_or(false)
    });
    assert!(parent.is_some(), "parent div with red color not found");
    // Child p should inherit red from parent
    let child = find_box(&doc.root, &|b: &WebCore| {
        b.tag == "p" && b.style.color == Color::rgb(255, 0, 0)
    });
    assert!(child.is_some(), "child p should inherit red color");
}

// ============================================================
// Display: none
// ============================================================

#[test]
fn box_model_display_none() {
    let doc = parse_and_layout(
        "<div style='display: none;'>Hidden</div><div>Visible</div>",
        800.0,
    );
    let hidden = find_box(&doc.root, &|b: &WebCore| b.style.display == Display::None);
    assert!(hidden.is_some(), "display:none div should be in tree");
    let hidden = hidden.unwrap();
    assert_eq!(
        hidden.layout.content_rect.w, 0.0,
        "display:none box should have zero width"
    );
    assert_eq!(
        hidden.layout.content_rect.h, 0.0,
        "display:none box should have zero height"
    );
}
