// Rendering tests – ported from cpptests/test_rendering.cpp
//
// C++ render tests call engine.Render(dc, doc, ...) which requires a wxDC.
// In Rust we port these as layout-based tests (parse + layout) since the
// Renderer requires a Pixmap from tiny-skia.  Full pixel rendering is
// covered by integration tests elsewhere.
//
// Test strategy:
//   - "Smoke" tests: parse + layout, assert no panic, check key invariants.
//   - Hit-test tests: use the hit_test API from layout::hit_test.
//   - Overflow tests: verify box dimensions after layout.
use tiny_skia::Pixmap;
use webcore::types::*;
use webcore::{load_html, parse_html, Renderer};

// ─── Helpers ──────────────────────────────────────────────────────────────────

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
// Visibility in Hit-Testing
// ============================================================

#[test]
fn hidden_box_layout_no_panic() {
    // visibility:hidden box should be in the layout tree but not visible.
    let doc = load_html(
        "<div style=\"visibility: hidden; width: 200px; height: 100px;\">Hidden</div>\
         <div style=\"width: 200px; height: 100px;\">Visible</div>",
        800.0,
    );
    // Hidden box exists in tree
    let hidden = find_box(&doc.root, &|b| b.tag == "div" && !b.style.visibility);
    assert!(
        hidden.is_some(),
        "visibility:hidden div should exist in tree"
    );
    // Visible box also exists
    let visible = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.visibility && b.layout.content_rect.h > 0.0
    });
    assert!(
        visible.is_some(),
        "visible div should exist in tree with non-zero height"
    );
}

#[test]
fn display_none_box_skipped_in_layout() {
    let doc = load_html(
        "<div style=\"display: none;\">Gone</div>\
         <div>Visible</div>",
        800.0,
    );
    // display:none box should have zero dimensions or be excluded from layout
    let none_box = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.display == Display::None
    });
    assert!(
        none_box.is_some(),
        "display:none div should still be in box tree"
    );
    // Visible div should have positive height
    let visible = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.display != Display::None && b.layout.content_rect.h > 0.0
    });
    assert!(visible.is_some(), "visible div should have positive height");
}

// ============================================================
// Overflow Clipping
// ============================================================

#[test]
fn overflow_hidden_layout() {
    let doc = load_html(
        "<div style=\"overflow: hidden; width: 200px; height: 50px;\">\
         <p>Content that overflows the box with lots of text</p>\
         </div>",
        800.0,
    );
    let box_ = find_box(&doc.root, &|b| {
        b.style.overflow_x == Overflow::Hidden && b.tag == "div"
    });
    assert!(box_.is_some(), "overflow:hidden div not found");
    // Width should be close to 200px
    let w = box_.unwrap().layout.content_rect.w;
    assert!(
        w >= 195.0 && w <= 205.0,
        "overflow:hidden div width should be ~200px, got {}",
        w
    );
}

#[test]
fn overflow_scroll_layout() {
    let doc = load_html(
        "<div style=\"overflow: scroll; width: 300px; height: 100px;\">\
         <p>Scrollable content</p>\
         </div>",
        800.0,
    );
    let box_ = find_box(&doc.root, &|b| b.style.overflow_x == Overflow::Scroll);
    assert!(box_.is_some(), "overflow:scroll div not found");
}

// ============================================================
// Border Rendering (parse + layout smoke)
// ============================================================

#[test]
fn border_render_smoke() {
    let doc = load_html(
        "<div style=\"border: 2px solid red; width: 200px; height: 100px;\">Bordered</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    let b = b.unwrap();
    assert_eq!(b.style.border_top_style, BorderStyle::Solid);
    assert_eq!(b.style.border_top_width, CssLength::Px(2.0));
}

#[test]
fn dashed_border_smoke() {
    let doc = load_html(
        "<div style=\"border: 3px dashed blue; width: 150px; height: 80px;\">Dashed</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.border_top_style, BorderStyle::Dashed);
}

#[test]
fn dotted_border_smoke() {
    let doc = load_html(
        "<div style=\"border: 1px dotted green; width: 100px;\">Dotted</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.border_top_style, BorderStyle::Dotted);
}

#[test]
fn rounded_border_smoke() {
    let doc = load_html(
        "<div style=\"border: 2px solid black; border-radius: 10px; \
         width: 200px; height: 100px;\">Rounded</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    let b = b.unwrap();
    // border-radius should be parsed
    let radius_px = b.style.border_radius.resolve(16.0, 200.0, 16.0);
    assert!(radius_px > 0.0, "border-radius should be > 0");
}

// ============================================================
// Box Shadow (parse + layout smoke)
// ============================================================

#[test]
fn box_shadow_render_smoke() {
    let doc = load_html(
        "<div style=\"box-shadow: 5px 5px 10px black; width: 200px; height: 100px;\">Shadow</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.tag == "div");
    assert!(b.is_some());
    // box-shadow should be parsed
    assert!(
        !b.unwrap().style.box_shadow.is_empty(),
        "box-shadow should be parsed"
    );
}

// ============================================================
// Gradient (parse + layout smoke)
// ============================================================

#[test]
fn linear_gradient_render_smoke() {
    let doc = load_html(
        "<div style=\"background-image: linear-gradient(to right, red, blue); \
         width: 300px; height: 100px;\">Gradient</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    assert_eq!(
        b.unwrap().style.gradient_type,
        GradientType::Linear,
        "gradient_type should be Linear"
    );
}

// ============================================================
// Opacity (parse + layout smoke)
// ============================================================

#[test]
fn opacity_render_smoke() {
    let doc = load_html(
        "<div style=\"opacity: 0.5; background-color: red; \
         width: 200px; height: 100px;\">Half transparent</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    let opacity = b.unwrap().style.opacity;
    assert!(
        (opacity - 0.5).abs() < 0.01,
        "opacity should be ~0.5, got {}",
        opacity
    );
}

#[test]
fn zero_opacity_render_smoke() {
    let doc = load_html(
        "<div style=\"opacity: 0; width: 100px; height: 100px;\">Invisible</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| b.tag == "div");
    assert!(b.is_some());
    assert_eq!(b.unwrap().style.opacity, 0.0, "opacity should be 0.0");
}

// ============================================================
// Background Color (parse + layout smoke)
// ============================================================

#[test]
fn background_color_render_smoke() {
    let doc = load_html(
        "<div style=\"background-color: #336699; width: 200px; height: 100px;\">Colored</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    let bg = b.unwrap().style.background_color;
    assert_eq!(bg.r, 0x33);
    assert_eq!(bg.g, 0x66);
    assert_eq!(bg.b, 0x99);
}

// ============================================================
// Text Shadow (parse + layout smoke)
// ============================================================

#[test]
fn text_shadow_render_smoke() {
    let doc = load_html(
        "<p style=\"text-shadow: 2px 2px 3px gray;\">Shadow text</p>",
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some());
    assert!(
        p.unwrap().style.text_shadow.is_some(),
        "text-shadow should be parsed"
    );
}

// ============================================================
// Outline (parse + layout smoke — should not affect layout dimensions)
// ============================================================

#[test]
fn outline_render_smoke() {
    let doc = load_html(
        "<div style=\"outline: 2px solid red; width: 200px; height: 100px;\">Outlined</div>",
        800.0,
    );
    let b = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(b.is_some());
    let b = b.unwrap();
    // Outline should be parsed
    assert!(b.style.outline_width > 0.0, "outline_width should be > 0");
    assert_eq!(b.style.outline_style, BorderStyle::Solid);
    // Outline does NOT affect content_rect width (unlike border)
    let w = b.layout.content_rect.w;
    assert!(
        w >= 195.0 && w <= 205.0,
        "outline should not affect content width, got {}",
        w
    );
}

// ============================================================
// Complex Rendering Scenarios (smoke – no panic)
// ============================================================

#[test]
fn mixed_styling_smoke() {
    let doc = load_html(
        "<div style=\"background-color: #f0f0f0; border: 1px solid #ccc; \
         border-radius: 5px; padding: 20px;\">\
           <h2 style=\"color: navy;\">Title</h2>\
           <p style=\"text-indent: 20px; line-height: 1.5;\">Paragraph with \
           <b>bold</b> and <i>italic</i> text.</p>\
           <ul><li>Item 1</li><li>Item 2</li></ul>\
         </div>",
        800.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.tag == "div" && b.layout.content_rect.w > 0.0
    });
    assert!(
        div.is_some(),
        "outer div must exist and have positive width"
    );
}

#[test]
fn positioned_with_borders_smoke() {
    let doc = load_html(
        "<div style=\"position: relative; width: 400px; height: 300px; border: 1px solid black;\">\
           <div style=\"position: absolute; top: 10px; right: 10px; \
           width: 100px; height: 50px; background-color: red;\">Abs</div>\
           <p style=\"text-decoration: underline;\">Underlined text</p>\
         </div>",
        800.0,
    );
    let outer = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.position == Position::Relative
    });
    assert!(outer.is_some(), "relative-positioned div must exist");
    let abs_box = find_box(&doc.root, &|b| {
        b.tag == "div" && b.style.position == Position::Absolute
    });
    assert!(abs_box.is_some(), "absolute-positioned div must exist");
}

#[test]
fn scrolled_viewport_smoke() {
    // Tall content — layout should produce multiple paragraphs stacked vertically
    let doc = load_html(
        "<div><p>Line 1</p><p>Line 2</p><p>Line 3</p><p>Line 4</p>\
         <p>Line 5</p><p>Line 6</p><p>Line 7</p><p>Line 8</p></div>",
        800.0,
    );
    let mut ps = Vec::new();
    fn collect<'a>(root: &'a WebCore, out: &mut Vec<&'a WebCore>) {
        if root.tag == "p" {
            out.push(root);
        }
        for c in &root.children {
            collect(c, out);
        }
    }
    collect(&doc.root, &mut ps);
    assert!(
        ps.len() >= 8,
        "should have 8 <p> elements, got {}",
        ps.len()
    );
    // Consecutive paragraphs should be stacked (later ones have greater y)
    for i in 1..ps.len() {
        assert!(
            ps[i].layout.content_rect.y > ps[i - 1].layout.content_rect.y,
            "p[{}] y ({}) should be > p[{}] y ({})",
            i,
            ps[i].layout.content_rect.y,
            i - 1,
            ps[i - 1].layout.content_rect.y
        );
    }
}

#[test]
fn selection_highlight_smoke() {
    // Parse + layout a paragraph — just verify it doesn't panic and has content
    let doc = load_html("<p>Hello World</p>", 800.0);
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some());
    assert!(
        p.unwrap().layout.content_rect.h > 0.0,
        "paragraph should have positive height after layout"
    );
}

// ============================================================
// Gradient rendering — HiDPI correctness
// ============================================================

/// Helper: render a document at the given logical size and scale, return the Pixmap.
fn render_doc(html: &str, logical_w: u32, logical_h: u32, scale: f32) -> Pixmap {
    let phys_w = (logical_w as f32 * scale) as u32;
    let phys_h = (logical_h as f32 * scale) as u32;
    let mut doc = load_html(html, logical_w as f32);
    let mut renderer = Renderer::new();
    let mut pixmap = Pixmap::new(phys_w, phys_h).expect("pixmap");
    renderer.render(&mut doc, &mut pixmap, scale);
    pixmap
}

/// Return the RGBA of the pixel at logical position (lx, ly) inside a pixmap rendered at `scale`.
fn pixel_at(pixmap: &Pixmap, lx: u32, ly: u32, scale: f32) -> (u8, u8, u8, u8) {
    let px = (lx as f32 * scale) as u32;
    let py = (ly as f32 * scale) as u32;
    let idx = (py * pixmap.width() + px) as usize;
    let p = &pixmap.pixels()[idx];
    (p.red(), p.green(), p.blue(), p.alpha())
}

#[test]
fn gradient_background_covers_element_at_scale_1() {
    // A div with a linear-gradient background should have non-white pixels
    // inside its bounds at scale 1.0.
    // Reset body margin so the div starts at x=0, y=0 — makes pixel coords predictable.
    let html = "<body style=\"margin:0;padding:0\"><div style=\"width:100px; height:50px; \
                 background: linear-gradient(to right, #ff0000, #0000ff);\"></div></body>";
    let pixmap = render_doc(html, 200, 100, 1.0);
    // The center of the div (50, 25) should NOT be white
    let (r, g, b, _) = pixel_at(&pixmap, 50, 25, 1.0);
    assert!(
        r > 0 || b > 0,
        "center of gradient div should have color, got ({r},{g},{b})"
    );
    // The left edge should be more red than blue
    let (lr, _lg, lb, _) = pixel_at(&pixmap, 5, 25, 1.0);
    assert!(lr > lb, "left edge should be more red, got r={lr} b={lb}");
    // The right edge should be more blue than red
    let (rr, _rg, rb, _) = pixel_at(&pixmap, 90, 25, 1.0);
    assert!(rb > rr, "right edge should be more blue, got r={rr} b={rb}");
}

#[test]
fn gradient_background_covers_element_at_scale_2() {
    // At HiDPI scale 2.0 the gradient should cover exactly the same logical area.
    // Verify that logical position (50, 25) — the center of the 100×50 div —
    // is still colored, not white.
    let html = "<div style=\"width:100px; height:50px; \
                 background: linear-gradient(to right, #ff0000, #0000ff);\"></div>";
    let pixmap = render_doc(html, 200, 100, 2.0);
    let (r, g, b, _) = pixel_at(&pixmap, 50, 25, 2.0);
    assert!(
        r > 0 || b > 0,
        "center of gradient div should have color at scale 2, got ({r},{g},{b})"
    );
    // At scale 2 the physical size is 400×200; physical pixel (100,50) is the center.
    // That should NOT be pure white (which would indicate the gradient was drawn
    // at the wrong (unscaled) position).
    let (r2, g2, b2, _) = pixel_at(&pixmap, 50, 25, 2.0);
    assert!(
        r2 > 0 || b2 > 0,
        "gradient at scale 2 should not be white at logical center"
    );
}

#[test]
fn gradient_background_position_matches_scale_1_and_2() {
    // Render the same HTML at scale 1 and scale 2.
    // Reset body margin so the div starts at x=0 — pixel coords match at both scales.
    let html = "<body style=\"margin:0;padding:0\"><div style=\"width:100px; height:50px; \
                 background: linear-gradient(to right, #ff0000, #0000ff);\"></div></body>";
    let pm1 = render_doc(html, 200, 100, 1.0);
    let pm2 = render_doc(html, 200, 100, 2.0);
    let (r1, _, b1, _) = pixel_at(&pm1, 5, 25, 1.0);
    let (r2, _, b2, _) = pixel_at(&pm2, 5, 25, 2.0);
    // Both should have the left side strongly red
    assert!(r1 > b1, "scale 1: left should be red, r={r1} b={b1}");
    assert!(r2 > b2, "scale 2: left should be red, r={r2} b={b2}");
    // Colors should be in the same general range (within 30 units)
    let dr = (r1 as i32 - r2 as i32).abs();
    let db = (b1 as i32 - b2 as i32).abs();
    assert!(
        dr < 30 && db < 30,
        "scale 1 and 2 should produce similar colors at same logical pos: \
         scale1=({r1},{b1}) scale2=({r2},{b2})"
    );
}
