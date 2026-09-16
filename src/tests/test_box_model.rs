// Ported from tests/test_box_model.cpp

use super::harness::*;
use crate::css::apply_property;
use crate::types::*;

// ── Margin Tests ──────────────────────────────────────────────────────────────

#[test]
fn boxmodel_margin_shorthand_four() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin", "10px 20px 30px 40px");
    assert_eq!(s.margin_top.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.margin_right.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(s.margin_bottom.resolve(16.0, 0.0, 16.0), 30.0);
    assert_eq!(s.margin_left.resolve(16.0, 0.0, 16.0), 40.0);
}

#[test]
fn boxmodel_margin_shorthand_two() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin", "10px 20px");
    assert_eq!(s.margin_top.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.margin_right.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(s.margin_bottom.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.margin_left.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn boxmodel_margin_shorthand_one() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin", "15px");
    assert_eq!(s.margin_top.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.margin_right.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.margin_bottom.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.margin_left.resolve(16.0, 0.0, 16.0), 15.0);
}

#[test]
fn boxmodel_margin_individual() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "margin-top", "5px");
    apply_property(&mut s, "margin-right", "10px");
    apply_property(&mut s, "margin-bottom", "15px");
    apply_property(&mut s, "margin-left", "20px");
    assert_eq!(s.margin_top.resolve(16.0, 0.0, 16.0), 5.0);
    assert_eq!(s.margin_right.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.margin_bottom.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.margin_left.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn boxmodel_margin_auto_center() {
    let doc = parse_and_layout(
        r#"<div style="width: 200px; margin-left: auto; margin-right: auto;">Centered</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 5.0
    });
    assert!(b.is_some(), "div with width=200 not found");
    let x = b.unwrap().layout.content_rect.x;
    assert!(
        x > 250.0 && x < 350.0,
        "expected centered x ~300, got {}",
        x
    );
}

#[test]
fn boxmodel_margin_auto_left_only() {
    let doc = parse_and_layout(
        r#"<div style="width: 200px; margin-left: auto;">Right</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 5.0
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.x > 500.0);
}

#[test]
fn boxmodel_margin_auto_does_not_affect_defaults() {
    let doc = parse_and_layout(r#"<div style="width: 200px;">Not centered</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 5.0
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.x < 50.0);
}

#[test]
fn boxmodel_margin_percent() {
    let doc = parse_and_layout(r#"<div style="margin-left: 10%;">Offset</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && matches!(b.style.margin_left, CssLength::Percent(_))
    });
    assert!(b.is_some());
    let x = b.unwrap().layout.content_rect.x;
    assert!(x >= 70.0 && x <= 90.0, "expected ~80, got {}", x);
}

// ── Padding Tests ─────────────────────────────────────────────────────────────

#[test]
fn boxmodel_padding_shorthand_four() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "padding", "10px 20px 30px 40px");
    assert_eq!(s.padding_top.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.padding_right.resolve(16.0, 0.0, 16.0), 20.0);
    assert_eq!(s.padding_bottom.resolve(16.0, 0.0, 16.0), 30.0);
    assert_eq!(s.padding_left.resolve(16.0, 0.0, 16.0), 40.0);
}

#[test]
fn boxmodel_padding_shorthand_one() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "padding", "15px");
    assert_eq!(s.padding_top.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.padding_right.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.padding_bottom.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.padding_left.resolve(16.0, 0.0, 16.0), 15.0);
}

#[test]
fn boxmodel_padding_individual() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "padding-top", "5px");
    apply_property(&mut s, "padding-right", "10px");
    apply_property(&mut s, "padding-bottom", "15px");
    apply_property(&mut s, "padding-left", "20px");
    assert_eq!(s.padding_top.resolve(16.0, 0.0, 16.0), 5.0);
    assert_eq!(s.padding_right.resolve(16.0, 0.0, 16.0), 10.0);
    assert_eq!(s.padding_bottom.resolve(16.0, 0.0, 16.0), 15.0);
    assert_eq!(s.padding_left.resolve(16.0, 0.0, 16.0), 20.0);
}

#[test]
fn boxmodel_padding_affects_layout() {
    let doc = parse_and_layout(
        r#"<div style="width: 200px; padding: 20px;">Padded</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 5.0
    });
    assert!(b.is_some());
    let pw = b.unwrap().layout.padding_rect.w;
    assert!(
        pw >= 235.0 && pw <= 245.0,
        "expected padding_rect.w ~240, got {}",
        pw
    );
}

// ── Border Tests ──────────────────────────────────────────────────────────────

#[test]
fn boxmodel_border_shorthand() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "border", "2px solid red");
    assert_eq!(s.border_top_width.resolve(16.0, 0.0, 16.0), 2.0);
    assert_eq!(s.border_top_style, BorderStyle::Solid);
    assert_eq!(s.border_top_color, Color::rgb(255, 0, 0));
    assert_eq!(s.border_right_width.resolve(16.0, 0.0, 16.0), 2.0);
    assert_eq!(s.border_bottom_width.resolve(16.0, 0.0, 16.0), 2.0);
    assert_eq!(s.border_left_width.resolve(16.0, 0.0, 16.0), 2.0);
}

#[test]
fn boxmodel_border_width_only() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "border-width", "4px");
    assert_eq!(s.border_top_width.resolve(16.0, 0.0, 16.0), 4.0);
    assert_eq!(s.border_right_width.resolve(16.0, 0.0, 16.0), 4.0);
    assert_eq!(s.border_bottom_width.resolve(16.0, 0.0, 16.0), 4.0);
    assert_eq!(s.border_left_width.resolve(16.0, 0.0, 16.0), 4.0);
}

#[test]
fn boxmodel_border_style_only() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "border-style", "dotted");
    assert_eq!(s.border_top_style, BorderStyle::Dotted);
}

#[test]
fn boxmodel_border_individual_sides() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "border-top", "1px solid black");
    apply_property(&mut s, "border-bottom", "3px dashed blue");
    assert_eq!(s.border_top_width.resolve(16.0, 0.0, 16.0), 1.0);
    assert_eq!(s.border_top_style, BorderStyle::Solid);
    assert_eq!(s.border_bottom_width.resolve(16.0, 0.0, 16.0), 3.0);
    assert_eq!(s.border_bottom_style, BorderStyle::Dashed);
}

#[test]
fn boxmodel_border_affects_layout() {
    let doc = parse_and_layout(
        r#"<div style="width: 200px; border: 5px solid black;">Bordered</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 200.0).abs() < 5.0
    });
    assert!(b.is_some());
    let bw = b.unwrap().layout.border_rect.w;
    assert!(
        bw >= 205.0 && bw <= 215.0,
        "expected border_rect.w ~210, got {}",
        bw
    );
}

// ── Box Sizing ────────────────────────────────────────────────────────────────

#[test]
fn boxmodel_content_box_default() {
    let doc = parse_and_layout(
        r#"<div style="width: 200px; padding: 20px; border: 5px solid black;">Content-box</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.box_sizing == BoxSizing::ContentBox
    });
    assert!(b.is_some(), "div should default to content-box sizing");
    let bx = b.unwrap();
    assert!(
        (bx.layout.content_rect.w - 200.0).abs() < 5.0,
        "content width should be 200"
    );
    // borderRect = 200 (content) + 40 (padding) + 10 (border) = 250
    assert!(
        bx.layout.border_rect.w >= 245.0 && bx.layout.border_rect.w <= 255.0,
        "expected border_rect.w ~250, got {}",
        bx.layout.border_rect.w
    );
}

#[test]
fn boxmodel_border_box_sizing() {
    let doc = parse_and_layout(
        r#"<div style="box-sizing: border-box; width: 200px; padding: 20px; border: 5px solid black;">Border-box</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.box_sizing == BoxSizing::BorderBox
    });
    assert!(b.is_some(), "div should have border-box sizing");
    let bx = b.unwrap();
    // borderRect should be 200
    assert!(
        bx.layout.border_rect.w >= 195.0 && bx.layout.border_rect.w <= 205.0,
        "expected border_rect.w ~200, got {}",
        bx.layout.border_rect.w
    );
    // contentWidth = 200 - 40 (padding) - 10 (border) = 150
    assert!(
        bx.layout.content_rect.w >= 145.0 && bx.layout.content_rect.w <= 155.0,
        "expected content_rect.w ~150, got {}",
        bx.layout.content_rect.w
    );
}

// ── Width / Height ────────────────────────────────────────────────────────────

#[test]
fn boxmodel_explicit_width() {
    let doc = parse_and_layout(r#"<div style="width: 300px;">Fixed</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.w - 300.0).abs() < 5.0
    });
    assert!(b.is_some());
}

#[test]
fn boxmodel_percentage_width() {
    let doc = parse_and_layout(r#"<div style="width: 50%;">Half</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 350.0 && b.layout.content_rect.w < 450.0
    });
    assert!(b.is_some());
}

#[test]
fn boxmodel_auto_width_fills_container() {
    let doc = parse_and_layout(r#"<div>Full width</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 700.0
    });
    assert!(b.is_some());
}

#[test]
fn boxmodel_min_width() {
    let doc = parse_and_layout(
        r#"<div style="width: 50px; min-width: 200px;">Min</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && matches!(b.style.min_width, CssLength::Px(v) if (v - 200.0).abs() < 1.0)
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.w >= 200.0);
}

#[test]
fn boxmodel_max_width() {
    let doc = parse_and_layout(r#"<div style="max-width: 300px;">Max</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && !b.style.max_width.is_none()
    });
    assert!(b.is_some());
    assert!(b.unwrap().layout.content_rect.w <= 305.0);
}

#[test]
fn boxmodel_explicit_height() {
    let doc = parse_and_layout(r#"<div style="height: 100px;">Tall</div>"#, 800.0);
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && (b.layout.content_rect.h - 100.0).abs() < 5.0
    });
    assert!(b.is_some());
}

// ── Colors ────────────────────────────────────────────────────────────────────

#[test]
fn boxmodel_named_colors() {
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
fn boxmodel_hex_colors_3() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "color", "#f00");
    assert_eq!(s.color, Color::rgb(255, 0, 0));
}

#[test]
fn boxmodel_hex_colors_6() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "color", "#00ff00");
    assert_eq!(s.color, Color::rgb(0, 255, 0));
}

#[test]
fn boxmodel_rgb_colors() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "color", "rgb(128, 64, 32)");
    assert_eq!(s.color, Color::rgb(128, 64, 32));
}

#[test]
fn boxmodel_rgba_colors() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "color", "rgba(255, 0, 0, 0.5)");
    assert_eq!(s.color.r, 255);
    assert_eq!(s.color.g, 0);
    assert_eq!(s.color.b, 0);
}

#[test]
fn boxmodel_background_color() {
    let mut s = ComputedStyle::default();
    apply_property(&mut s, "background-color", "#336699");
    assert_eq!(s.background_color, Color::rgb(0x33, 0x66, 0x99));
}

#[test]
fn boxmodel_background_color_from_html() {
    let doc = parse(r#"<div style="background-color: yellow;">Yellow</div>"#);
    let b = find_box(&doc.root, &|b| {
        b.style.background_color == Color::rgb(255, 255, 0)
    });
    assert!(b.is_some());
}

#[test]
fn boxmodel_color_inheritance_via_stylesheet() {
    let doc = parse_and_layout(
        r#"<html><head><style>.parent { color: red; }</style></head>
           <body><div class="parent"><p>Inherits red</p></div></body></html>"#,
        800.0,
    );
    let parent = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "parent")
            .unwrap_or(false)
            && b.style.color == Color::rgb(255, 0, 0)
    });
    assert!(parent.is_some(), "parent div with red color not found");
    let child = find_box(&doc.root, &|b| {
        b.tag == "p" && b.style.color == Color::rgb(255, 0, 0)
    });
    assert!(child.is_some(), "child p should inherit red");
}

// ── Display: none ─────────────────────────────────────────────────────────────

#[test]
fn boxmodel_display_none() {
    let doc = parse_and_layout(
        r#"<div style="display: none;">Hidden</div><div>Visible</div>"#,
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.style.display == Display::None);
    assert!(b.is_some());
    assert_eq!(b.unwrap().layout.content_rect.w, 0.0);
    assert_eq!(b.unwrap().layout.content_rect.h, 0.0);
}

#[test]
fn boxmodel_display_none_clears_descendant_layout() {
    let doc = parse_and_layout(
        r#"<div id="hidden" style="display:none">
             <header style="position:fixed; width:400px; height:40px">
               <a style="display:block; width:120px; height:20px">Hidden</a>
             </header>
           </div>
           <p>Visible</p>"#,
        800.0,
    );
    let hidden = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "hidden")
    })
    .unwrap();
    assert_eq!(hidden.layout.content_rect, Rect::default());
    let header = find_box(&doc.root, &|b| b.tag == "header").unwrap();
    assert_eq!(header.layout.content_rect, Rect::default());
    let link = find_box(&doc.root, &|b| b.tag == "a").unwrap();
    assert_eq!(link.layout.content_rect, Rect::default());
}
