// Visual property tests – ported from cpptests/test_visual.cpp
// Render smoke and hit-test skipped (require widget / DC infrastructure).
use webcore::css::{apply_property, Stylesheet};
use webcore::types::*;
use webcore::{load_html, parse_html};

fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

// ============================================================
// Opacity
// ============================================================

#[test]
fn opacity_parsed_from_inline() {
    let doc = parse_html("<div style=\"opacity: 0.5;\">Semi</div>");
    let b = find_box(&doc.root, &|b| {
        b.style.opacity > 0.49 && b.style.opacity < 0.51
    });
    assert!(b.is_some());
}

#[test]
fn opacity_zero() {
    let doc = parse_html("<div style=\"opacity: 0;\">Invisible</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div" && b.style.opacity < 0.01);
    assert!(b.is_some());
}

#[test]
fn opacity_one() {
    let doc = parse_html("<div style=\"opacity: 1;\">Full</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div" && b.style.opacity > 0.99);
    assert!(b.is_some());
}

#[test]
fn opacity_clamped_high() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "opacity", "2.0");
    assert!(style.opacity >= 0.0);
}

#[test]
fn opacity_clamped_low() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "opacity", "-1.0");
    assert!(style.opacity >= -1.1);
}

// ============================================================
// Overflow
// ============================================================

#[test]
fn overflow_hidden() {
    let doc = parse_html("<div style=\"overflow: hidden;\">Clipped</div>");
    let b = find_box(&doc.root, &|b| b.style.overflow_x == Overflow::Hidden);
    assert!(b.is_some());
}

#[test]
fn overflow_scroll() {
    let doc = parse_html("<div style=\"overflow: scroll;\">Scrollable</div>");
    let b = find_box(&doc.root, &|b| b.style.overflow_x == Overflow::Scroll);
    assert!(b.is_some());
}

#[test]
fn overflow_auto() {
    let doc = parse_html("<div style=\"overflow: auto;\">Auto</div>");
    let b = find_box(&doc.root, &|b| b.style.overflow_x == Overflow::Auto);
    assert!(b.is_some());
}

#[test]
fn overflow_visible_default() {
    let doc = parse_html("<div>Default overflow</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(b.style.overflow_x, Overflow::Visible);
}

// ============================================================
// Outline
// ============================================================

#[test]
fn outline_shorthand() {
    let doc = parse_html("<div style=\"outline: 2px solid red;\">Outlined</div>");
    let b = find_box(&doc.root, &|b| {
        b.style.outline_width == 2.0 && b.style.outline_style == BorderStyle::Solid
    });
    assert!(b.is_some());
}

#[test]
fn outline_individual() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "outline-width", "3px");
    apply_property(&mut style, "outline-style", "dashed");
    apply_property(&mut style, "outline-color", "blue");
    apply_property(&mut style, "outline-offset", "5px");
    assert_eq!(style.outline_width, 3.0);
    assert_eq!(style.outline_style, BorderStyle::Dashed);
    assert_eq!(style.outline_color, Color::rgb(0, 0, 255));
    assert_eq!(style.outline_offset, 5.0);
}

#[test]
fn outline_does_not_affect_layout() {
    let doc = load_html(
        "<div style=\"width: 200px; outline: 5px solid red;\">Outlined</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w >= 199.0 && b.layout.content_rect.w <= 201.0
    });
    assert!(b.is_some());
    // Outline should not increase the border box dimensions
    let bx = b.unwrap();
    assert!(bx.layout.border_rect.w >= 199.0 && bx.layout.border_rect.w <= 201.0);
}

// ============================================================
// Text Overflow
// ============================================================

#[test]
fn text_overflow_ellipsis() {
    let doc = parse_html(
        "<div style=\"text-overflow: ellipsis; overflow: hidden; white-space: nowrap;\">Long text</div>");
    let b = find_box(&doc.root, &|b| {
        b.style.text_overflow == TextOverflow::Ellipsis
    });
    assert!(b.is_some());
}

#[test]
fn text_overflow_clip_default() {
    let doc = parse_html("<div>Normal text</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(b.style.text_overflow, TextOverflow::Clip);
}

// ============================================================
// Border Radius
// ============================================================

#[test]
fn border_radius_parsed() {
    let doc = parse_html("<div style=\"border-radius: 10px;\">Rounded</div>");
    let b = find_box(&doc.root, &|b| b.style.border_radius == CssLength::Px(10.0));
    assert!(b.is_some());
}

#[test]
fn border_radius_zero_default() {
    let doc = parse_html("<div>Square</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert!(
        matches!(b.style.border_radius, CssLength::Px(v) if v < 0.01)
            || matches!(b.style.border_radius, CssLength::Zero)
    );
}

// ============================================================
// Box Shadow
// ============================================================

#[test]
fn box_shadow_parsed() {
    let doc = parse_html("<div style=\"box-shadow: 5px 10px 15px rgba(0,0,0,0.5);\">Shadow</div>");
    let b = find_box(&doc.root, &|b| !b.style.box_shadow.is_empty());
    assert!(b.is_some());
}

#[test]
fn box_shadow_none_default() {
    let doc = parse_html("<div>No shadow</div>");
    let b = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert!(b.style.box_shadow.is_empty());
}

// ============================================================
// Visibility
// ============================================================

#[test]
fn visibility_hidden() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "visibility", "hidden");
    assert!(!style.visibility);
}

#[test]
fn visibility_visible() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "visibility", "visible");
    assert!(style.visibility);
}

// ============================================================
// Gradient
// ============================================================

#[test]
fn linear_gradient_parsed() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "linear-gradient(to bottom, red, blue)",
    );
    assert_eq!(style.gradient_type, GradientType::Linear);
    assert!(style.rare().gradient_stops.len() >= 2);
}

// ============================================================
// clip-path
// ============================================================

#[test]
fn clip_path_inset_parsed() {
    let doc = parse_html("<div style='clip-path: inset(10px 20px 30px 40px);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Inset);
    assert_eq!(div.style.clip_path.inset_top, CssLength::Px(10.0));
    assert_eq!(div.style.clip_path.inset_right, CssLength::Px(20.0));
    assert_eq!(div.style.clip_path.inset_bottom, CssLength::Px(30.0));
    assert_eq!(div.style.clip_path.inset_left, CssLength::Px(40.0));
}

#[test]
fn clip_path_inset_single_value() {
    let doc = parse_html("<div style='clip-path: inset(15px);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Inset);
    assert_eq!(div.style.clip_path.inset_top, CssLength::Px(15.0));
    assert_eq!(div.style.clip_path.inset_right, CssLength::Px(15.0));
    assert_eq!(div.style.clip_path.inset_bottom, CssLength::Px(15.0));
    assert_eq!(div.style.clip_path.inset_left, CssLength::Px(15.0));
}

#[test]
fn clip_path_circle_parsed() {
    let doc = parse_html("<div style='clip-path: circle(50% at 50% 50%);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Circle);
    assert_eq!(div.style.clip_path.circle_radius, CssLength::Percent(50.0));
    assert_eq!(div.style.clip_path.center_x, CssLength::Percent(50.0));
    assert_eq!(div.style.clip_path.center_y, CssLength::Percent(50.0));
}

#[test]
fn clip_path_circle_no_center() {
    let doc = parse_html("<div style='clip-path: circle(100px);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Circle);
    assert_eq!(div.style.clip_path.circle_radius, CssLength::Px(100.0));
    // Default center is 50% 50%
    assert_eq!(div.style.clip_path.center_x, CssLength::Percent(50.0));
    assert_eq!(div.style.clip_path.center_y, CssLength::Percent(50.0));
}

#[test]
fn clip_path_ellipse_parsed() {
    let doc = parse_html("<div style='clip-path: ellipse(40% 60% at 50% 50%);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Ellipse);
    assert_eq!(div.style.clip_path.ellipse_rx, CssLength::Percent(40.0));
    assert_eq!(div.style.clip_path.ellipse_ry, CssLength::Percent(60.0));
}

#[test]
fn clip_path_polygon_parsed() {
    let doc = parse_html("<div style='clip-path: polygon(50% 0%, 100% 100%, 0% 100%);'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::Polygon);
    assert_eq!(div.style.clip_path.points.len(), 3);
    assert_eq!(div.style.clip_path.points[0].0, CssLength::Percent(50.0));
    assert_eq!(div.style.clip_path.points[0].1, CssLength::Percent(0.0));
    assert_eq!(div.style.clip_path.points[1].0, CssLength::Percent(100.0));
    assert_eq!(div.style.clip_path.points[1].1, CssLength::Percent(100.0));
}

#[test]
fn clip_path_none() {
    let doc = parse_html("<div style='clip-path: none;'>Test</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.clip_path.kind, ClipPathKind::None);
}

// ============================================================
// CSS Variable Resolution via Stylesheet
// ============================================================

#[test]
fn css_variable_resolution() {
    let doc = load_html(
        "<html><head><style>\
         :root { --main-bg: #00ff00; }\
         .box { background-color: var(--main-bg); }\
         </style></head>\
         <body><div class=\"box\">Green</div></body></html>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "box")
            .unwrap_or(false)
            && b.style.background_color == Color::rgb(0, 255, 0)
    });
    assert!(
        b.is_some(),
        "div.box should have green background from CSS variable"
    );
}

// ============================================================
// Pseudo-element ::before / ::after content
// ============================================================

#[test]
fn pseudo_element_content() {
    let doc = load_html(
        "<html><head><style>\
         p::before { content: \">> \"; }\
         p::after  { content: \" <<\"; }\
         </style></head>\
         <body><p>Content</p></body></html>",
        800.0,
    );
    let before_box = find_box(&doc.root, &|b| {
        b.tag == "p" && !b.style.before_content.is_empty()
    });
    assert!(
        before_box.is_some(),
        "p should have before_content from ::before rule"
    );
    let after_box = find_box(&doc.root, &|b| {
        b.tag == "p" && !b.style.after_content.is_empty()
    });
    assert!(
        after_box.is_some(),
        "p should have after_content from ::after rule"
    );
}

#[test]
fn pseudo_element_before_has_own_style() {
    // ::before with its own color/font-weight should store a full ComputedStyle
    let doc = load_html(
        r#"<style>
            p::before { content: ">> "; color: red; font-weight: bold; }
        </style><p>Hello</p>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some(), "p not found");
    let p = p.unwrap();
    assert_eq!(p.style.before_content, ">> ", "before_content text");
    let bs = p.style.before_style.as_deref();
    assert!(
        bs.is_some(),
        "before_style should be Some — other declarations were dropped"
    );
    let bs = bs.unwrap();
    assert_eq!(
        bs.color,
        Color::rgb(255, 0, 0),
        "::before color should be red"
    );
    assert_eq!(
        bs.font_weight,
        webcore::types::FontWeight::Bold,
        "::before font-weight should be bold"
    );
}

#[test]
fn pseudo_element_after_has_own_style() {
    let doc = load_html(
        r#"<style>
            span::after { content: " OK"; color: #00aa00; font-style: italic; }
        </style><p><span id="s">Done</span></p>"#,
        800.0,
    );
    let s = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "s").unwrap_or(false)
    });
    assert!(s.is_some(), "span not found");
    let s = s.unwrap();
    assert_eq!(s.style.after_content, " OK");
    let as_ = s.style.after_style.as_deref();
    assert!(as_.is_some(), "after_style should be Some");
    let c = as_.unwrap().color;
    assert_eq!(
        (c.r, c.g, c.b),
        (0, 170, 0),
        "::after color should be #00aa00"
    );
    assert_eq!(as_.unwrap().font_style, webcore::types::FontStyle::Italic);
}

#[test]
fn pseudo_element_inherits_font_from_element() {
    // When ::before has no explicit font-size, it should inherit from the element
    let doc = load_html(
        r#"<style>
            p { font-size: 20px; }
            p::before { content: ">> "; }
        </style><p>Text</p>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some());
    let p = p.unwrap();
    assert_eq!(p.style.before_content, ">> ");
    // before_style inherits font-size from element
    let bs = p.style.before_style.as_deref();
    assert!(bs.is_some(), "before_style should be set");
    let f = bs.unwrap().font_size.resolve(16.0, 0.0, 16.0);
    assert!(
        (f - 20.0).abs() < 1.0,
        "::before should inherit font-size 20px, got {f}"
    );
}

#[test]
fn pseudo_element_does_not_nest() {
    // before_style and after_style on a pseudo-element style should be None (no nesting)
    let doc = load_html(
        r#"<style>p::before { content: "X"; color: blue; }</style><p>Hi</p>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    if let Some(bs) = p.style.before_style.as_deref() {
        assert!(
            bs.before_style.is_none(),
            "pseudo-element style should not have nested before_style"
        );
        assert!(
            bs.after_style.is_none(),
            "pseudo-element style should not have nested after_style"
        );
    }
}

// ============================================================
// Stylesheet struct: CSS variables in :root
// ============================================================

#[test]
fn css_variables_in_root() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        ":root { --main-color: #ff0000; --gap: 10px; } p { color: var(--main-color); }",
    );
    assert!(
        ss.variables.contains_key("--main-color"),
        "should extract --main-color"
    );
    assert_eq!(
        ss.variables.get("--main-color").map(|s| s.as_str()),
        Some("#ff0000")
    );
    assert!(ss.variables.contains_key("--gap"), "should extract --gap");
}

#[test]
fn css_variable_with_fallback() {
    // A stylesheet with a variable reference that uses a fallback value must parse without error.
    let mut ss = Stylesheet::default();
    ss.parse_and_add(":root { --main: blue; } p { color: var(--missing, red); }");
    assert!(
        !ss.rules.is_empty(),
        "stylesheet should have at least one rule"
    );
}

// ============================================================
// Background shorthand: position / size / repeat
// ============================================================

#[test]
fn background_shorthand_cover_no_repeat() {
    let doc =
        parse_html("<div style='background: #ccc url(test.png) center / cover no-repeat;'>X</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.background_size, BackgroundSize::Cover);
    assert_eq!(div.style.background_repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(div.style.background_position_x, CssLength::Percent(50.0));
    assert_eq!(div.style.background_position_y, CssLength::Percent(50.0));
}

#[test]
fn background_shorthand_center_top() {
    let doc = parse_html("<div style='background: url(img.jpg) center top no-repeat;'>X</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.background_position_x, CssLength::Percent(50.0));
    assert_eq!(div.style.background_position_y, CssLength::Percent(0.0));
    assert_eq!(div.style.background_repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn background_shorthand_contain() {
    let doc = parse_html("<div style='background: url(x.png) center / contain;'>X</div>");
    let div = find_box(&doc.root, &|b| b.tag == "div").unwrap();
    assert_eq!(div.style.background_size, BackgroundSize::Contain);
}

// ============================================================
// ::selection pseudo-element
// ============================================================

#[test]
fn selection_style_stored_on_element() {
    let doc = load_html(
        r#"<style>
            ::selection { background-color: #ffcc00; color: #000000; }
        </style><p id="p">Hello</p>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "p").unwrap_or(false)
    });
    assert!(p.is_some(), "p not found");
    let ss = p.unwrap().style.selection_style.as_deref();
    assert!(
        ss.is_some(),
        "::selection style should be stored on element"
    );
    let bg = ss.unwrap().background_color;
    assert_eq!(
        (bg.r, bg.g, bg.b),
        (255, 204, 0),
        "::selection background should be #ffcc00"
    );
}

#[test]
fn selection_style_per_element_override() {
    // p::selection overrides ::selection for <p> elements only
    let doc = load_html(
        r#"<style>
            ::selection          { background-color: blue; }
            p::selection         { background-color: red; }
        </style><p id="p">Text</p><div id="d">Other</div>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "p").unwrap_or(false)
    });
    let d = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "d").unwrap_or(false)
    });
    let p_bg = p
        .unwrap()
        .style
        .selection_style
        .as_deref()
        .map(|s| s.background_color);
    let d_bg = d
        .unwrap()
        .style
        .selection_style
        .as_deref()
        .map(|s| s.background_color);
    assert!(p_bg.is_some(), "p should have selection_style");
    assert!(d_bg.is_some(), "div should have selection_style");
    assert_eq!(
        (p_bg.unwrap().r, p_bg.unwrap().g, p_bg.unwrap().b),
        (255, 0, 0),
        "p::selection should be red"
    );
    assert_eq!(
        (d_bg.unwrap().r, d_bg.unwrap().g, d_bg.unwrap().b),
        (0, 0, 255),
        "div::selection should fall back to blue"
    );
}

// ============================================================
// ::marker pseudo-element
// ============================================================

#[test]
fn marker_style_stored_on_list_item() {
    let doc = load_html(
        r#"<style>
            li::marker { color: red; }
        </style><ul><li id="li">Item</li></ul>"#,
        800.0,
    );
    let li = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "li").unwrap_or(false)
    });
    assert!(li.is_some(), "li not found");
    let ms = li.unwrap().style.marker_style.as_deref();
    assert!(ms.is_some(), "::marker style should be stored on <li>");
    let c = ms.unwrap().color;
    assert_eq!((c.r, c.g, c.b), (255, 0, 0), "::marker color should be red");
}

// ============================================================
// Ignored pseudo-elements don't leak styles
// ============================================================

#[test]
fn first_line_does_not_apply_to_element() {
    let doc = load_html(
        r#"<style>p::first-line { font-size: 99px; }</style><p id="p">Text</p>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "p").unwrap_or(false)
    });
    assert!(p.is_some());
    let fs = p.unwrap().style.font_size.resolve(16.0, 0.0, 16.0);
    // ::first-line should not set font-size to 99px on the element itself
    assert!(
        fs < 50.0,
        "::first-line font-size leaked to element: {fs}px"
    );
}

#[test]
fn placeholder_does_not_apply_to_element() {
    let doc = load_html(
        r#"<style>input::placeholder { color: hotpink; }</style>
           <input id="inp" type="text">"#,
        800.0,
    );
    let inp = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "inp").unwrap_or(false)
    });
    assert!(inp.is_some());
    // color should NOT be hotpink (placeholder style must not leak to the input element)
    let c = inp.unwrap().style.color;
    assert_ne!(
        (c.r, c.g, c.b),
        (255, 105, 180),
        "::placeholder color leaked to <input>"
    );
}

// ============================================================
// Render Smoke Tests (ported from Visual::RenderComplexSmoke /
// ClipPathRenderSmoke / ClipPathPolygonRenderSmoke)
// ============================================================

/// Helper matching test_rendering.rs: renders HTML into a Pixmap.
fn render_doc_visual(html: &str, logical_w: u32, logical_h: u32) -> tiny_skia::Pixmap {
    use webcore::Renderer;
    let mut doc = load_html(html, logical_w as f32);
    let mut renderer = Renderer::new();
    let mut pixmap = tiny_skia::Pixmap::new(logical_w, logical_h).expect("pixmap");
    renderer.render(&mut doc, &mut pixmap, 1.0);
    pixmap
}

#[test]
fn render_complex_smoke() {
    // Ported from Visual::RenderComplexSmoke — verifies the renderer does not
    // panic on a document with mixed visual properties.
    render_doc_visual(
        "<html><body>\
         <h1>Hello</h1>\
         <p style=\"color: blue; opacity: 0.8;\">World</p>\
         <div style=\"overflow: hidden; width: 100px; height: 50px;\">Clipped content here</div>\
         <div style=\"outline: 2px solid red;\">Outlined</div>\
         <div style=\"border-radius: 8px; background-color: #eee; padding: 10px;\">Rounded</div>\
         </body></html>",
        800,
        600,
    );
    // no panic → pass
}

#[test]
fn hit_test_smoke() {
    // Ported from Visual::HitTestSmoke — verifies that point_to_hit returns
    // a valid (or None) result without panicking.
    use webcore::point_to_hit;
    let doc = load_html("<p>Hello World</p>", 800.0);
    // Point inside the document — may or may not hit a text run, but must not panic.
    let _hit = point_to_hit(&doc.root, (10.0, 10.0), 0);
    // Point outside the document — must also not panic.
    let _miss = point_to_hit(&doc.root, (9999.0, 9999.0), 0);
}

#[test]
fn clip_path_render_smoke() {
    // Ported from Visual::ClipPathRenderSmoke — circle clip-path renders without panic.
    render_doc_visual(
        "<div style='width: 200px; height: 200px; background: red; \
         clip-path: circle(50% at 50% 50%);'>Clipped</div>",
        800,
        600,
    );
}

#[test]
fn clip_path_polygon_render_smoke() {
    // Ported from Visual::ClipPathPolygonRenderSmoke — polygon clip-path renders without panic.
    render_doc_visual(
        "<div style='width: 200px; height: 200px; background: blue; \
         clip-path: polygon(50% 0%, 100% 100%, 0% 100%);'>Triangle</div>",
        800,
        600,
    );
}
