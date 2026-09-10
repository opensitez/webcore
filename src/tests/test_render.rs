// Pixel-level render tests for blend modes, gradients, and layout.
// ── Button background covers right padding ───────────────────────────────────
// Pixel test: the background color must appear in the right-padding zone.
#[test]
fn render_button_bg_covers_right_padding() {
    // Button with red background, 20px left/right padding.
    // After layout, the right 20px zone should be red.
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        .row { display: flex; }
        .btn { padding: 0 20px; background: red; font-size: 10pt; }
        </style>
        <div class="row">
          <div class="btn">Hi</div>
        </div>
    "#,
        200,
        40,
    );
    // The button starts at x=0. Find a non-white pixel to locate button right edge.
    // Scan row y=5 for the last red pixel.
    let y = 5u32;
    let mut last_red = None;
    for x in 0..200 {
        let (r, g, b, _a) = pixel(&pm, x, y);
        if r > 200 && g < 50 && b < 50 {
            last_red = Some(x);
        }
    }
    let last_red = last_red.expect("No red pixel found — button background not rendered");
    // Inside the right padding, a pixel 5 from the last_red edge should also be red
    // i.e., the rightmost-20px zone is all red. Test that at least 15 consecutive red
    // pixels exist before last_red.
    let mut run = 0u32;
    for x in (0..=last_red).rev() {
        let (r, g, b, _a) = pixel(&pm, x, y);
        if r > 200 && g < 50 && b < 50 {
            run += 1;
        } else {
            break;
        }
    }
    assert!(
        run >= 15,
        "Right padding area should be at least 15px red; got {} red pixels ending at x={}",
        run,
        last_red
    );
}

// ── Float:right text renders at correct position ─────────────────────────────
// Pixel test: text in a float:right span must appear on the right half.
#[test]
fn render_float_right_text_visible() {
    // White background, float:right span colored red.
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        .item { width: 200px; background: #eee; font-size: 10pt; }
        .stat { float: right; color: red; }
        </style>
        <div class="item">Left <span class="stat">Rt</span></div>
    "#,
        300,
        40,
    );
    // Expect some colored pixels on the right half (x > 100) of the div
    let mut found_right = false;
    for x in 100..200 {
        for y in 0..30u32 {
            let (r, _g, _b, a) = pixel(&pm, x, y);
            // Red text should have r component significantly higher
            if a > 10 && r > 150 {
                found_right = true;
                break;
            }
        }
        if found_right {
            break;
        }
    }
    assert!(
        found_right,
        "float:right text (red) should appear in right half of container"
    );
}

// ── Button background in graph_demo exact setup ──────────────────────────────
// Pixel test: button background (with border-radius) covers right padding
// in a border-box sidebar+content layout exactly matching graph_demo CSS.
#[test]
fn render_graph_demo_button_bg_right_padding() {
    // Matches graph_demo: * { box-sizing: border-box }, sidebar 170px,
    // content flex:1, button padding 5px 12px, border-radius 6px.
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: #0d1117; }
        .main { display: flex; }
        .sidebar { width: 170px; min-width: 170px; background: #161b22;
                   border-right: 1px solid #30363d; padding: 14px; }
        .content { flex: 1; }
        .btn-row { display: flex; gap: 8px; padding: 0 16px 12px 16px; }
        .btn { padding: 5px 12px; border-radius: 6px; font-size: 8pt;
               font-weight: 600; border: none; background: #1f6feb; color: #fff; }
        </style>
        <div class="main">
          <div class="sidebar">S</div>
          <div class="content">
            <div class="btn-row">
              <div class="btn" id="b1">All Bar</div>
              <div class="btn" id="b2">All Line</div>
            </div>
          </div>
        </div>
    "#,
        800,
        60,
    );
    // The first button starts at x = 170 (sidebar) + 16 (btn-row pad-left) = 186.
    // It should be blue (#1f6feb → r=31 g=111 b=235) across its full width.
    // Scan y=15 (inside the button, away from top/bottom padding) for blue pixels.
    let y = 15u32;
    let mut first_blue = None;
    let mut last_blue = None;
    for x in 186..600 {
        let (r, _g, b, _a) = pixel(&pm, x, y);
        // button blue: high blue, low red
        if b > 150 && r < 100 {
            if first_blue.is_none() {
                first_blue = Some(x);
            }
            last_blue = Some(x);
        } else if last_blue.is_some() {
            break; // past the first button
        }
    }
    let first = first_blue.expect("No blue pixel found — button background not rendered");
    let last = last_blue.unwrap();
    let btn_w = (last - first + 1) as f32;
    // Button should be at least content(~35px) + 12 + 12 = ~59px wide
    assert!(btn_w >= 40.0,
        "Button blue area should be >= 40px wide (includes both paddings), got {btn_w}px [{first}..{last}]");
    // The rightmost blue pixel should be at least 10px past the text start
    // (i.e., right padding is present). Text starts ~12px from first blue.
    assert!(
        (last - first) >= 30,
        "Button should span at least 30px of blue (text + both paddings), got {} [{first}..{last}]",
        last - first
    );
}

// ── Bold text background covers right padding (border-radius demo) ────────────
// Regression: measure_text_width_weighted underestimated bold text width when
// no font system was present, making content_w too small → right padding gap.
// Fix: apply 1.15x multiplier for bold/semi-bold in the approximation path.
#[test]
fn render_bold_text_bg_covers_right_padding() {
    // Mirror the "Border Radius" section from demo.html:
    // flex row of bold-text divs with blue bg + 20px padding + border-radius.
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: white; }
        .row { display: flex; gap: 16px; padding: 10px; }
        .card { background-color: #3498db; color: white;
                padding: 20px; border-radius: 8px; font-weight: bold; font-size: 10pt; }
        </style>
        <div class="row">
          <div class="card">8px radius</div>
        </div>
    "#,
        300,
        80,
    );
    // Scan row y=30 (inside the card, away from top/bottom padding rounding).
    // Find the first and last blue pixels — blue: high B, low R.
    let y = 30u32;
    let mut first_blue: Option<u32> = None;
    let mut last_blue: Option<u32> = None;
    for x in 0..300 {
        let (r, _g, b, _a) = pixel(&pm, x, y);
        if b > 150 && r < 100 {
            if first_blue.is_none() {
                first_blue = Some(x);
            }
            last_blue = Some(x);
        }
    }
    let first = first_blue.expect("No blue pixel found — bold card background not rendered");
    let last = last_blue.unwrap();
    let span = last - first + 1;
    // Card has 20px left + text ("8px radius" ~9 chars * ~7px ≈ 63px) + 20px right.
    // Total ≥ 100px. Require ≥ 80px to have headroom for font approximation variance
    // but still catch the bug (pre-fix the background was ~63px with no right padding).
    assert!(span >= 80,
        "Bold card background should span ≥ 80px (text + both 20px paddings), got {span}px [{first}..{last}]");
    // The right padding must be present: at least 10px of blue after the text.
    // We don't know the exact text width, but the blue run must extend well past
    // what the text alone would cover (without right-pad the span would be ~63px).
    assert!(span >= 90,
        "Bold card background right padding appears missing: span={span}px, expected ≥ 90px [{first}..{last}]");
}

// Pixel-level render tests for blend modes and gradients.

use super::harness::parse_and_layout;
use crate::renderer::Renderer;
use tiny_skia::Pixmap;

fn render_html(html: &str, w: u32, h: u32) -> Pixmap {
    let mut renderer = Renderer::new();
    // Use renderer.load_html so layout and rendering share the same font system,
    // ensuring background rects are sized to actual glyph widths (not approximations).
    let mut doc = renderer.load_html(html, w as f32);
    let mut pixmap = Pixmap::new(w, h).unwrap();
    renderer.render(&mut doc, &mut pixmap, 1.0);
    pixmap
}

fn pixel(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let idx = (y * pm.width() + x) as usize * 4;
    let d = pm.data();
    // tiny-skia stores premultiplied RGBA
    let a = d[idx + 3];
    if a == 0 {
        return (0, 0, 0, 0);
    }
    // un-premultiply
    let r = ((d[idx] as u32 * 255) / a as u32) as u8;
    let g = ((d[idx + 1] as u32 * 255) / a as u32) as u8;
    let b = ((d[idx + 2] as u32 * 255) / a as u32) as u8;
    (r, g, b, a)
}

#[test]
fn svg_stroke_current_color_does_not_get_filled() {
    let pm = render_html(
        r#"
        <style>
          body { margin: 0; background: white; }
          svg { color: rgb(0, 128, 0); }
        </style>
        <svg width="80" height="40" viewBox="0 0 80 40">
          <path d="M5 5 L75 5 L75 35 Z" stroke="currentColor" stroke-width="4"/>
        </svg>
        "#,
        90,
        50,
    );

    let (sr, sg, sb, _) = pixel(&pm, 40, 5);
    assert!(
        sg > sr && sg > sb,
        "stroke should resolve currentColor through SVG rasterization; got rgb({sr}, {sg}, {sb})"
    );

    let (ir, ig, ib, _) = pixel(&pm, 60, 20);
    assert!(
        ir > 240 && ig > 240 && ib > 240,
        "stroke-only SVG path must not receive an injected fill; got rgb({ir}, {ig}, {ib})"
    );
}

#[test]
fn box_shadow_blur_softens_outside_the_shadow_rect() {
    let pm = render_html(
        r#"
        <style>
          body { margin: 0; background: white; }
          #box { margin: 40px; width: 40px; height: 40px; background: white; box-shadow: 0 0 12px black; }
        </style>
        <div id="box"></div>
        "#,
        140,
        140,
    );
    let (r, g, b, _a) = pixel(&pm, 35, 60);

    assert!(
        r < 250 && g < 250 && b < 250,
        "blurred box-shadow should paint soft pixels outside the box edge; got rgb({r}, {g}, {b})"
    );
}

#[test]
fn text_shadow_blur_softens_outside_the_glyphs() {
    let pm = render_html(
        r#"
        <style>
          body { margin: 0; background: white; }
          #text { margin-left: 24px; margin-top: 16px; color: transparent; text-shadow: 0 0 8px black; font: 48px/56px sans-serif; }
        </style>
        <div id="text">MM</div>
        "#,
        180,
        100,
    );

    let mut found_soft_shadow = false;
    for x in 12..22 {
        for y in 25..70 {
            let (r, g, b, _a) = pixel(&pm, x, y);
            if r < 250 && g < 250 && b < 250 {
                found_soft_shadow = true;
                break;
            }
        }
        if found_soft_shadow {
            break;
        }
    }

    assert!(
        found_soft_shadow,
        "blurred text-shadow should paint soft pixels outside the glyph origin"
    );
}

#[test]
fn render_contenteditable_selection_uses_selection_background() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
           body { margin: 0; background: white; }
           #ed { font: 20px/24px sans-serif; color: black; }
           #ed::selection { background-color: rgb(255, 0, 0); color: white; }
           </style>
           <div id="ed" contenteditable>hello world</div>"#,
        240.0,
    );
    let ed = doc.get_element_by_id("ed").unwrap();
    doc.editor.caret_box = Some(ed);
    doc.editor.caret_local = 5;
    doc.editor.sel_start = 0;
    doc.editor.sel_end = 5;
    doc.editor.caret_visible = false;

    let line = doc
        .get_box_by_id(ed)
        .unwrap()
        .layout
        .line_cache
        .first()
        .cloned()
        .expect("contenteditable text should have a line");
    let x = (line.x + line.char_x.get(2).copied().unwrap_or(20.0)).round() as u32;
    let y = (line.y + line.height / 2.0).round() as u32;

    let mut pixmap = Pixmap::new(240, 80).unwrap();
    renderer.render(&mut doc, &mut pixmap, 1.0);
    let (r, g, b, a) = pixel(&pixmap, x, y);

    assert!(
        a > 0 && r > 200 && g < 80 && b < 80,
        "::selection background should paint red at ({x},{y}), got rgba({r},{g},{b},{a})"
    );
}

#[test]
fn render_contenteditable_selection_uses_selection_foreground() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
           body { margin: 0; background: white; }
           #ed { font: 48px/56px sans-serif; color: black; }
           #ed::selection { background-color: black; color: rgb(0, 255, 0); }
           </style>
           <div id="ed" contenteditable>MMMM</div>"#,
        240.0,
    );
    let ed = doc.get_element_by_id("ed").unwrap();
    doc.editor.caret_box = Some(ed);
    doc.editor.caret_local = 4;
    doc.editor.sel_start = 0;
    doc.editor.sel_end = 4;
    doc.editor.caret_visible = false;

    let mut pixmap = Pixmap::new(240, 90).unwrap();
    renderer.render(&mut doc, &mut pixmap, 1.0);

    let mut green_pixels = 0usize;
    for y in 0..70 {
        for x in 0..180 {
            let (r, g, b, a) = pixel(&pixmap, x, y);
            if a > 0 && g > 160 && r < 80 && b < 80 {
                green_pixels += 1;
            }
        }
    }

    assert!(
        green_pixels > 20,
        "::selection foreground should repaint selected text green; found {green_pixels} green pixels"
    );
}

#[test]
fn element_scrollbar_color_paints_track_and_thumb() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        #scroller {
            width: 40px;
            height: 40px;
            overflow-y: scroll;
            scrollbar-color: rgb(255, 0, 0) rgb(0, 255, 0);
            background: white;
        }
        #content { height: 120px; }
        </style>
        <div id="scroller"><div id="content"></div></div>
    "#,
        80,
        80,
    );

    let (tr, tg, tb, _) = pixel(&pm, 35, 30);
    assert!(
        tg > 180 && tr < 80 && tb < 80,
        "scrollbar track should use scrollbar-color track, got rgba({tr},{tg},{tb},_)"
    );

    let (r, g, b, _) = pixel(&pm, 35, 5);
    assert!(
        r > 180 && g < 80 && b < 80,
        "scrollbar thumb should use scrollbar-color thumb, got rgba({r},{g},{b},_)"
    );
}

#[test]
fn horizontal_element_scrollbar_color_paints_track_and_thumb() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        #scroller {
            width: 40px;
            height: 40px;
            overflow-x: scroll;
            overflow-y: hidden;
            white-space: nowrap;
            scrollbar-color: rgb(255, 0, 0) rgb(0, 255, 0);
            background: white;
        }
        #content { display: inline-block; width: 120px; height: 20px; }
        </style>
        <div id="scroller"><div id="content"></div></div>
    "#,
        80,
        80,
    );

    let (tr, tg, tb, _) = pixel(&pm, 30, 35);
    assert!(
        tg > 180 && tr < 80 && tb < 80,
        "horizontal scrollbar track should use scrollbar-color track, got rgba({tr},{tg},{tb},_)"
    );

    let (r, g, b, _) = pixel(&pm, 5, 35);
    assert!(
        r > 180 && g < 80 && b < 80,
        "horizontal scrollbar thumb should use scrollbar-color thumb, got rgba({r},{g},{b},_)"
    );
}

// ── Flex nav: li items inside a flex ul must not overlap ──────────────────────

#[test]
fn layout_flex_nav_li_items_no_overlap() {
    // A flex <ul> with <li> items: each li must start after the previous one ends.
    // Historically, compute_intrinsic_width treated list-item children as block children
    // (taking max width), which caused the outer flex to assign too-small a width to the
    // ul, which then squished all li items to near-zero causing visual overlap.
    use super::harness::find_box;
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .nav { display: flex; width: 600px; }
        .links { display: flex; gap: 20px; list-style: none; }
        </style>
        <nav class="nav">
          <ul class="links">
            <li>World</li>
            <li>Politics</li>
            <li>Science</li>
          </ul>
        </nav>
    "#,
        600.0,
    );

    // Collect li border_rect x positions
    let mut li_boxes: Vec<f32> = Vec::new();
    fn collect_li(node: &crate::types::WebCore, out: &mut Vec<f32>) {
        if node.tag == "li" {
            out.push(node.layout.border_rect.x);
        }
        for ch in &node.children {
            collect_li(ch, out);
        }
    }
    collect_li(&doc.root, &mut li_boxes);

    assert_eq!(
        li_boxes.len(),
        3,
        "expected 3 li boxes, got {}",
        li_boxes.len()
    );
    // Each li must start strictly after the previous one (no overlap)
    assert!(
        li_boxes[1] > li_boxes[0] + 5.0,
        "Politics li should start after World li; World.x={} Politics.x={}",
        li_boxes[0],
        li_boxes[1]
    );
    assert!(
        li_boxes[2] > li_boxes[1] + 5.0,
        "Science li should start after Politics li; Politics.x={} Science.x={}",
        li_boxes[1],
        li_boxes[2]
    );
}

// ── Absolute positioned child height from inset: 0 ───────────────────────────

#[test]
fn layout_abs_inset_zero_fills_parent_height() {
    // position:absolute; inset:0 must give the child the same height as the parent.
    use super::harness::find_box;
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .parent { width: 200px; height: 100px; position: relative; }
        .child  { position: absolute; inset: 0; }
        </style>
        <div class="parent"><div class="child"></div></div>
    "#,
        200.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "child")
            .unwrap_or(false)
    })
    .expect("child not found");
    assert!(
        (child.layout.border_rect.w - 200.0).abs() < 1.0,
        "inset:0 child width should be 200, got {}",
        child.layout.border_rect.w
    );
    assert!(
        (child.layout.border_rect.h - 100.0).abs() < 1.0,
        "inset:0 child height should be 100, got {}",
        child.layout.border_rect.h
    );
}

// ── Blend mode: solid colors ──────────────────────────────────────────────────

#[test]
fn render_blend_multiply_solid_colors() {
    // Red stage (255,0,0) + blue overlay (inset:0) with multiply → near-black
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 100px; height: 100px; background: #ff0000; position: relative; }
        .overlay { position: absolute; inset: 0; background: #0000ff; mix-blend-mode: multiply; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        100,
        100,
    );
    let (r, g, b, _) = pixel(&pm, 50, 50);
    // multiply(red, blue) = (255*0/255, 0*0/255, 0*255/255) = (0,0,0) → black
    assert!(
        r < 20,
        "multiply red*blue should give near-black red channel, got {r}"
    );
    assert!(
        b < 20,
        "multiply red*blue should give near-black blue channel, got {b}"
    );
}

#[test]
fn render_blend_screen_solid_colors() {
    // Red stage (255,0,0) + blue overlay (inset:0) with screen → bright magenta
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 100px; height: 100px; background: #ff0000; position: relative; }
        .overlay { position: absolute; inset: 0; background: #0000ff; mix-blend-mode: screen; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        100,
        100,
    );
    let (r, g, b, _) = pixel(&pm, 50, 50);
    // screen(red, blue) = 1-(1-1)*(1-0)=1 for R; 1-(1-0)*(1-1)=1 for B → magenta (255,0,255)
    assert!(
        r > 200,
        "screen red*blue should give bright red channel, got {r}"
    );
    assert!(
        b > 200,
        "screen red*blue should give bright blue channel, got {b}"
    );
}

#[test]
fn render_blend_normal_vs_multiply_differ() {
    // Verify that multiply and normal produce different pixels
    let normal_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 100px; height: 100px; background: #ff6600; position: relative; }
        .overlay { position: absolute; inset: 0; background: #0066ff; mix-blend-mode: normal; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        100,
        100,
    );
    let multiply_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 100px; height: 100px; background: #ff6600; position: relative; }
        .overlay { position: absolute; inset: 0; background: #0066ff; mix-blend-mode: multiply; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        100,
        100,
    );
    let (nr, ng, nb, _) = pixel(&normal_pm, 50, 50);
    let (mr, mg, mb, _) = pixel(&multiply_pm, 50, 50);
    // normal shows the blue overlay; multiply: orange*blue = much darker
    assert_ne!((nr, nb), (mr, mb),
        "normal and multiply should produce different pixels; normal=({nr},{ng},{nb}) multiply=({mr},{mg},{mb})");
    let normal_luma = nr as u32 + ng as u32 + nb as u32;
    let multiply_luma = mr as u32 + mg as u32 + mb as u32;
    assert!(multiply_luma < normal_luma,
        "multiply should be darker than normal; normal_luma={normal_luma} multiply_luma={multiply_luma}");
}

#[test]
fn background_blend_mode_multiplies_gradient_over_background_color() {
    let normal_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box {
            width: 80px;
            height: 80px;
            background-color: #ff0000;
            background-image: linear-gradient(#0000ff, #0000ff);
            background-blend-mode: normal;
        }
        </style>
        <div class="box"></div>
    "#,
        100,
        100,
    );
    let multiply_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .box {
            width: 80px;
            height: 80px;
            background-color: #ff0000;
            background-image: linear-gradient(#0000ff, #0000ff);
            background-blend-mode: multiply;
        }
        </style>
        <div class="box"></div>
    "#,
        100,
        100,
    );

    let (nr, ng, nb, _) = pixel(&normal_pm, 40, 40);
    let (mr, mg, mb, _) = pixel(&multiply_pm, 40, 40);
    assert!(
        nb > 200 && nr < 40 && ng < 40,
        "normal background blend should paint the blue gradient, got ({nr},{ng},{nb})"
    );
    assert!(
        mr < 40 && mg < 40 && mb < 40,
        "multiply should blend the blue gradient with the red background to black, got ({mr},{mg},{mb})"
    );
}

#[test]
fn text_decoration_skip_ink_auto_leaves_descender_gap() {
    let auto_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        p {
            color: black;
            font-size: 40px;
            line-height: 1;
            text-decoration: underline;
            text-decoration-color: red;
            text-decoration-thickness: 4px;
            text-decoration-skip-ink: auto;
        }
        </style>
        <p>ag</p>
    "#,
        120,
        70,
    );
    let none_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        p {
            color: black;
            font-size: 40px;
            line-height: 1;
            text-decoration: underline;
            text-decoration-color: red;
            text-decoration-thickness: 4px;
            text-decoration-skip-ink: none;
        }
        </style>
        <p>ag</p>
    "#,
        120,
        70,
    );

    let red_pixels = |pm: &Pixmap| {
        let mut count = 0usize;
        for y in 30..60 {
            for x in 35..80 {
                let (r, g, b, _) = pixel(pm, x, y);
                if r > 200 && g < 80 && b < 80 {
                    count += 1;
                }
            }
        }
        count
    };
    let auto_red = red_pixels(&auto_pm);
    let none_red = red_pixels(&none_pm);
    assert!(
        none_red > auto_red + 20,
        "skip-ink:none should paint more continuous underline pixels under descenders; auto={auto_red}, none={none_red}"
    );
}

#[test]
fn blend_mode_composites_into_enclosing_filter_layer() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: #ff0000; }
        .group { width: 80px; height: 80px; background: #00ff00; filter: brightness(1); position: relative; }
        .blend { position: absolute; inset: 0; background: #0000ff; mix-blend-mode: multiply; }
        </style>
        <div class="group"><div class="blend"></div></div>
    "#,
        100,
        100,
    );
    let (r, g, b, _) = pixel(&pm, 40, 40);
    assert!(
        r < 30 && g < 30 && b < 30,
        "blue multiply should blend inside the green filtered group, got ({r},{g},{b})"
    );
}

#[test]
fn same_element_filter_runs_before_mix_blend_mode() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: #ff0000; }
        .box { width: 80px; height: 80px; background: #0000ff; filter: grayscale(1); mix-blend-mode: screen; }
        </style>
        <div class="box"></div>
    "#,
        100,
        100,
    );
    let (r, g, b, _) = pixel(&pm, 40, 40);
    assert!(
        r > 220 && (5..=60).contains(&g) && (5..=60).contains(&b),
        "filter should grayscale blue to its low luminance before screen blending with red, got ({r},{g},{b})"
    );
}

// ── Blend mode: with radial gradient ─────────────────────────────────────────

#[test]
fn render_blend_multiply_gradient_overlay() {
    // Linear base + radial warm overlay (inset:0) with multiply → center darker than normal
    let normal_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 200px; height: 100px; background: linear-gradient(90deg, #1d4ed8, #be185d);
                 position: relative; }
        .overlay { position: absolute; inset: 0;
                   background: radial-gradient(circle at 50% 50%, #fbbf24 0%, #f97316 60%, transparent 100%);
                   mix-blend-mode: normal; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        200,
        100,
    );
    let multiply_pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .stage { width: 200px; height: 100px; background: linear-gradient(90deg, #1d4ed8, #be185d);
                 position: relative; }
        .overlay { position: absolute; inset: 0;
                   background: radial-gradient(circle at 50% 50%, #fbbf24 0%, #f97316 60%, transparent 100%);
                   mix-blend-mode: multiply; }
        </style>
        <div class="stage"><div class="overlay"></div></div>
    "#,
        200,
        100,
    );
    let (nr, ng, nb, _) = pixel(&normal_pm, 100, 50);
    let (mr, mg, mb, _) = pixel(&multiply_pm, 100, 50);
    assert!(
        mr != nr || mg != ng || mb != nb,
        "multiply and normal should differ at center; normal=({nr},{ng},{nb}) multiply=({mr},{mg},{mb})"
    );
    let normal_luma = nr as u32 + ng as u32 + nb as u32;
    let multiply_luma = mr as u32 + mg as u32 + mb as u32;
    assert!(multiply_luma < normal_luma,
        "multiply should produce darker result than normal; normal_luma={normal_luma} multiply_luma={multiply_luma}");
}

// ── Sticky positioning inside a scrollable div ────────────────────────────────

#[test]
fn layout_sticky_inside_scrollable_div() {
    use super::harness::find_box;
    // A sticky header inside a fixed-height overflow:scroll container.
    // The sticky element should NOT move past clip.y (the top of the div in screen space).
    // Previously the threshold was 0 (top of screen) instead of clip.y, so sticky never
    // kicked in for elements inside a div.
    //
    // We verify layout: the sticky-header exists and has a valid position inside the container.
    let doc = super::harness::parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .container { height: 200px; overflow-y: scroll; position: relative; width: 300px; }
        .sticky-hdr { position: sticky; top: 0; height: 30px; background: red; }
        .content    { height: 600px; }
        </style>
        <div class="container">
          <div class="sticky-hdr">Header</div>
          <div class="content"></div>
        </div>
    "#,
        300.0,
    );
    let hdr = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "sticky-hdr")
            .unwrap_or(false)
    })
    .expect("sticky-hdr not found");
    let container = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "container")
            .unwrap_or(false)
    })
    .expect("container not found");
    // Header must be inside the container vertically
    assert!(
        hdr.layout.border_rect.y >= container.layout.border_rect.y,
        "sticky-hdr should be at or below container top; hdr.y={} container.y={}",
        hdr.layout.border_rect.y,
        container.layout.border_rect.y
    );
    assert!(
        (hdr.layout.border_rect.h - 30.0).abs() < 1.0,
        "sticky-hdr height should be 30, got {}",
        hdr.layout.border_rect.h
    );
}

// ── inline-block in flex: background must cover padding ──────────────────────

#[test]
fn layout_inline_block_in_flex_padding_covered() {
    use super::harness::find_box;
    // An inline-block button with horizontal padding inside a flex row nested in a two-column
    // layout (sidebar + content), mirroring graph.html structure.
    // border_rect.w must equal content_rect.w + left_padding + right_padding (24px total).
    // Bug: background (border_rect) was smaller than content+padding, clipping right padding,
    // caused by the outer flex over-shrinking the content column.
    let doc = super::harness::parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .main    { display: flex; }
        .sidebar { width: 170px; min-width: 170px; }
        .content { flex: 1; min-width: 0; }
        .btn-row { display: flex; flex-wrap: wrap; gap: 8px; }
        .btn     { display: inline-block; padding: 5px 12px; background: blue; }
        </style>
        <div class="main">
          <div class="sidebar">Sidebar</div>
          <div class="content">
            <div class="btn-row">
              <span class="btn">All Bar</span>
              <span class="btn">All Line</span>
            </div>
          </div>
        </div>
    "#,
        800.0,
    );
    let btn = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "btn")
            .unwrap_or(false)
    })
    .expect("btn not found");
    let pad_total = 12.0 + 12.0; // padding-left + padding-right
    let expected_border_w = btn.layout.content_rect.w + pad_total;
    assert!(
        (btn.layout.border_rect.w - expected_border_w).abs() < 1.5,
        "btn border_rect.w should be content_rect.w + 24 = {expected_border_w}, got {}; \
         content_rect.w={}",
        btn.layout.border_rect.w,
        btn.layout.content_rect.w
    );
    // content must be non-trivial (text was measured)
    assert!(
        btn.layout.content_rect.w > 5.0,
        "btn content_rect.w should be > 5 (text width), got {}",
        btn.layout.content_rect.w
    );
    // sidebar must keep its min-width (not be over-shrunk by flex)
    let sidebar = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "sidebar")
            .unwrap_or(false)
    })
    .expect("sidebar not found");
    assert!(
        sidebar.layout.border_rect.w >= 169.0,
        "sidebar must not shrink below min-width 170; got {}",
        sidebar.layout.border_rect.w
    );
}

// ── float:right inside a block renders to the right ──────────────────────────

#[test]
fn layout_float_right_appears_on_right() {
    use super::harness::find_box;
    // A float:right element inside a fixed-width block should be positioned
    // at the right edge of the parent's content area.
    // Bug: float:right elements inside sb-item were not visible at all.
    let doc = super::harness::parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .item  { width: 170px; padding: 5px 8px; }
        .stat  { float: right; }
        </style>
        <div class="item">/home <span class="stat">4,231</span></div>
    "#,
        200.0,
    );
    let stat = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "stat")
            .unwrap_or(false)
    })
    .expect("stat not found");
    let item = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "item")
            .unwrap_or(false)
    })
    .expect("item not found");
    // The float must be to the right of the midpoint of the content area
    let content_mid = item.layout.content_rect.x + item.layout.content_rect.w / 2.0;
    assert!(
        stat.layout.border_rect.x > content_mid,
        "float:right stat should be in right half; stat.x={} content_mid={} item.content={:?}",
        stat.layout.border_rect.x,
        content_mid,
        item.layout.content_rect
    );
    // Float right edge should be near the content right edge
    let stat_right = stat.layout.border_rect.x + stat.layout.border_rect.w;
    let content_right = item.layout.content_rect.x + item.layout.content_rect.w;
    assert!((stat_right - content_right).abs() < 2.0,
        "float:right right edge should align with content right; stat_right={stat_right} content_right={content_right}");
}

#[test]
fn debug_graph_sidebar_and_button() {
    use super::harness::{find_all_boxes, find_box, parse_and_layout};
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; font-size: 10pt; }
        body { background: #0d1117; }
        .main { display: flex; }
        .sidebar { background: #161b22; border-right: 1px solid #30363d;
                   padding: 14px; width: 170px; min-width: 170px; }
        .sidebar h3 { font-size: 9pt; margin: 0 0 8px 0; }
        .sb-item { padding: 5px 8px; margin-bottom: 2px; border-radius: 6px; font-size: 8pt; }
        .sb-item .sstat { float: right; }
        .content { flex: 1; min-width: 0; }
        .btn-row { display: flex; gap: 8px; padding: 0 16px 12px 16px; flex-wrap: wrap; }
        .btn { padding: 5px 12px; border-radius: 6px; font-size: 8pt;
               font-weight: 600; display: inline-block; background: blue; }
        </style>
        <div class="main">
          <div class="sidebar">
            <h3>Pages</h3>
            <div class="sb-item" id="sb-home">/home <span class="sstat">4,231</span></div>
            <div class="sb-item" id="sb-products">/products <span class="sstat">2,847</span></div>
          </div>
          <div class="content">
            <div class="btn-row">
              <span class="btn" id="btn-bar">All Bar</span>
              <span class="btn" id="btn-line">All Line</span>
            </div>
          </div>
        </div>
    "#,
        1024.0,
    );

    // Check sidebar width
    let sidebar = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "sidebar")
            .unwrap_or(false)
    })
    .unwrap();
    eprintln!("sidebar border_rect={:?}", sidebar.layout.border_rect);

    // Check sstat float positions
    let sstats = find_all_boxes(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "sstat")
            .unwrap_or(false)
    });
    for s in &sstats {
        eprintln!(
            "sstat border_rect={:?} margin_rect={:?}",
            s.layout.border_rect, s.layout.margin_rect
        );
    }

    // Check buttons
    let btns = find_all_boxes(
        &doc.root,
        &|b| matches!(b.attributes.get("class"), Some(c) if c.contains("btn")),
    );
    for b in &btns {
        eprintln!(
            "btn '{}' border_rect={:?} content_rect={:?}",
            b.attributes.get("id").unwrap_or(&String::new()),
            b.layout.border_rect,
            b.layout.content_rect
        );
    }
}

// ── Gradient opacity is applied ───────────────────────────────────────────────

/// Regression: draw_gradient ignored node.style.opacity, so `opacity` animation
/// had no visual effect on elements with gradient backgrounds (fade animation bug).
#[test]
fn render_gradient_opacity_applied() {
    // A solid-red gradient at opacity:0.5 over white body.
    // The center pixel should appear as a half-alpha blend (~pink), not full red.
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: white; }
        .box { width: 100px; height: 100px;
               background: linear-gradient(red, red);
               opacity: 0.5; }
        </style>
        <div class="box"></div>
    "#,
        150,
        120,
    );
    let (r, g, b, _) = pixel(&pm, 50, 50);
    // red at 50% opacity over white → composited ~(255, 127, 127)
    assert!(
        r > 200,
        "red channel should be high (blend of red+white), got {r}"
    );
    assert!(
        g > 80,
        "green channel should be elevated by white blend (opacity=0.5), got {g}"
    );
    assert!(
        b > 80,
        "blue channel should be elevated by white blend (opacity=0.5), got {b}"
    );
}

#[test]
fn render_opacity_composites_descendants_as_one_group() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: white; }
        .group { position: relative; width: 100px; height: 100px; opacity: 0.5; }
        .a, .b { position: absolute; top: 0; width: 70px; height: 100px; }
        .a { left: 0; background: red; }
        .b { left: 30px; background: blue; }
        </style>
        <div class="group"><div class="a"></div><div class="b"></div></div>
    "#,
        120,
        120,
    );

    let (_, overlap_g, overlap_b, _) = pixel(&pm, 50, 50);
    assert!(
        overlap_g > 105 && overlap_b > 220,
        "opacity must composite the children as one group over white, got g={overlap_g} b={overlap_b}"
    );
}

// ── Absolute all-auto insets inside flex container: positioned inside container ──

/// Regression: position:absolute children with all insets auto inside a flex container
/// were placed at document origin (0,0) instead of following the static position.
/// After the fix they should stay inside their containing flex box.
#[test]
fn layout_abs_all_auto_in_flex_inside_container() {
    use super::harness::find_box;
    let doc = super::harness::parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { padding-top: 80px; }
        .wrap { position: relative; display: flex;
                align-items: center; justify-content: center;
                width: 100px; height: 100px; }
        .ring { position: absolute; width: 60px; height: 60px; }
        </style>
        <div class="wrap"><div class="ring"></div></div>
    "#,
        300.0,
    );
    let wrap = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "wrap")
            .unwrap_or(false)
    })
    .expect("wrap not found");
    let ring = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "ring")
            .unwrap_or(false)
    })
    .expect("ring not found");
    // ring must be inside wrap (not at document origin ~0,0)
    assert!(
        ring.layout.border_rect.y >= wrap.layout.border_rect.y - 1.0,
        "ring y ({}) should be >= wrap y ({}) — ring should not be at document origin",
        ring.layout.border_rect.y,
        wrap.layout.border_rect.y
    );
    assert!(
        ring.layout.border_rect.x >= wrap.layout.border_rect.x - 1.0,
        "ring x ({}) should be >= wrap x ({})",
        ring.layout.border_rect.x,
        wrap.layout.border_rect.x
    );
}

/// Absolute child with all-auto insets inside a centered flex container should
/// be centered (follow the static position from justify-content/align-items).
#[test]
fn layout_abs_all_auto_in_flex_centered() {
    use super::harness::find_box;
    let doc = super::harness::parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .wrap { position: relative; display: flex;
                align-items: center; justify-content: center;
                width: 200px; height: 200px; }
        .child { position: absolute; width: 40px; height: 40px; }
        </style>
        <div class="wrap"><div class="child"></div></div>
    "#,
        300.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "child")
            .unwrap_or(false)
    })
    .expect("child not found");
    // Container 200x200 at origin, child 40x40 → centered at (80, 80)
    assert!(
        (child.layout.border_rect.x - 80.0).abs() < 2.0,
        "abs child x should be ~80 (centered), got {}",
        child.layout.border_rect.x
    );
    assert!(
        (child.layout.border_rect.y - 80.0).abs() < 2.0,
        "abs child y should be ~80 (centered), got {}",
        child.layout.border_rect.y
    );
}

#[test]
fn absolute_static_position_uses_inline_flow_x_when_horizontal_insets_auto() {
    use super::harness::find_box;

    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { font: 16px/20px monospace; }
        .wrap { position: relative; width: 300px; height: 80px; }
        .abs { position: absolute; width: 20px; height: 20px; }
        </style>
        <div class="wrap"><span id="lead">abcd</span><span class="abs"></span></div>
    "#,
        320.0,
    );
    let lead = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "lead")
    })
    .expect("lead not found");
    let abs = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .is_some_and(|class| class == "abs")
    })
    .expect("abs not found");

    let expected_x = lead.layout.margin_rect.x + lead.layout.margin_rect.w;
    assert!(
        (abs.layout.border_rect.x - expected_x).abs() < 2.0,
        "absolute all-auto x should follow its inline static position: got {}, expected {}",
        abs.layout.border_rect.x,
        expected_x
    );
}

#[test]
fn absolute_all_auto_fallback_uses_rtl_containing_block_end() {
    use super::harness::find_box;

    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .wrap { position: relative; direction: rtl; width: 240px; height: 120px; }
        .abs { position: absolute; bottom: 0; width: 48px; height: 48px; }
        </style>
        <div class="wrap"><div class="abs"></div></div>
    "#,
        320.0,
    );
    let abs = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .is_some_and(|class| class == "abs")
    })
    .expect("abs not found");

    assert!(
        (abs.layout.border_rect.x - 192.0).abs() < 1.0,
        "absolute all-auto fallback in RTL should use the containing block end, got {}",
        abs.layout.border_rect.x
    );
}

#[test]
fn absolute_over_constrained_horizontal_insets_follow_direction() {
    use super::harness::find_box;

    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        .wrap { position: relative; width: 200px; height: 60px; }
        .box { position: absolute; left: 20px; right: 30px; width: 40px; height: 20px; }
        #rtl { direction: rtl; }
        </style>
        <div id="ltr" class="wrap"><div id="a" class="box"></div></div>
        <div id="rtl" class="wrap"><div id="b" class="box"></div></div>
    "#,
        260.0,
    );
    let a = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "a")
    })
    .expect("ltr abs not found");
    let b = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "b")
    })
    .expect("rtl abs not found");

    assert!(
        (a.layout.border_rect.x - 20.0).abs() < 1.0,
        "LTR over-constrained abs should honor left, got {}",
        a.layout.border_rect.x
    );
    assert!(
        (b.layout.border_rect.x - 130.0).abs() < 1.0,
        "RTL over-constrained abs should honor right, got {}",
        b.layout.border_rect.x
    );
}

// ── Border-radius per-side arc: top-only border on a circle ──────────────────

/// Regression: spinner (.spinner { border-radius:50%; border-top:3px solid purple; })
/// rendered a visible border only in the screen-space top half (via clip mask),
/// not as a proper arc path. After the fix, arc paths are used so rotation works.
/// This test verifies the arc shape: border pixels should appear at the TOP of
/// the circle and NOT at the BOTTOM center.
#[test]
fn render_border_radius_top_only_renders_top_arc() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { background: #000; }
        .spinner {
          width: 80px; height: 80px; border-radius: 50%;
          border-top:    4px solid #fff;
          border-right:  4px solid transparent;
          border-bottom: 4px solid transparent;
          border-left:   4px solid transparent;
        }
        </style>
        <div class="spinner"></div>
    "#,
        200,
        200,
    );

    // Top center of the spinner (x=40, y≈2) should have a white arc pixel
    let top_has_border = (2u32..10).any(|y| {
        let (r, g, b, a) = pixel(&pm, 40, y);
        a > 30 && r > 100 && g > 100 && b > 100
    });
    assert!(
        top_has_border,
        "top center of spinner should have a white border arc pixel"
    );

    // Bottom center (x=40, y≈76) should have NO border (transparent → black background)
    let bottom_has_border = (70u32..80).any(|y| {
        let (r, g, b, a) = pixel(&pm, 40, y);
        a > 30 && r > 100 && g > 100 && b > 100
    });
    assert!(
        !bottom_has_border,
        "bottom center of spinner should be black (no border-bottom), but found border pixels"
    );
}

// ── CSS scale() transform applies to text (heartbeat fix) ────────────────────

/// Regression: CSS `transform: scale()` was applied to backgrounds/borders but
/// NOT to text content — draw_inline_content bypassed elem_ts entirely.
/// After the fix, text is rendered to a temp pixmap and composited with the
/// CSS transform, so a scale(2) element has text that spans twice as many pixels.
#[test]
fn render_css_scale_transform_affects_text() {
    // Red text "I" at scale(1) — measure its pixel width.
    let pm_normal = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        .t { font-size: 20px; color: red; display: inline-block; }
        </style>
        <div class="t">I</div>
    "#,
        200,
        60,
    );

    // Same text at scale(2) — should span approximately twice as many red pixels.
    let pm_scaled = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        .t { font-size: 20px; color: red; display: inline-block;
             transform: scale(2); transform-origin: 0 0; }
        </style>
        <div class="t">I</div>
    "#,
        200,
        60,
    );

    // Count non-white pixels in top row band (y=5..30) for both renders.
    fn count_red_pixels(pm: &tiny_skia::Pixmap, y_range: std::ops::Range<u32>) -> u32 {
        let mut n = 0u32;
        for y in y_range {
            for x in 0..pm.width() {
                let (r, g, b, a) = {
                    let idx = (y * pm.width() + x) as usize * 4;
                    let d = pm.data();
                    let a = d[idx + 3];
                    if a == 0 {
                        (0u8, 0u8, 0u8, 0u8)
                    } else {
                        let r = ((d[idx] as u32 * 255) / a as u32) as u8;
                        let g = ((d[idx + 1] as u32 * 255) / a as u32) as u8;
                        let b = ((d[idx + 2] as u32 * 255) / a as u32) as u8;
                        (r, g, b, a)
                    }
                };
                if a > 10 && r > 150 && g < 100 && b < 100 {
                    n += 1;
                }
            }
        }
        n
    }

    let normal_red = count_red_pixels(&pm_normal, 5..30);
    let scaled_red = count_red_pixels(&pm_scaled, 5..55);

    assert!(
        normal_red > 0,
        "baseline render should have some red text pixels"
    );
    assert!(scaled_red > normal_red,
        "scale(2) text should cover more pixels than scale(1); normal={normal_red} scaled={scaled_red}");
}

#[test]
fn debug_sidebar_box_sizing() {
    use super::harness::{find_box, parse_and_layout};
    use crate::types::BoxSizing;
    let doc = parse_and_layout(
        r#"
        <style>
        * { box-sizing: border-box; }
        .sidebar { padding: 14px; width: 170px; border-right: 1px solid red; }
        </style>
        <div style="display:flex;">
          <div class="sidebar">Sidebar</div>
          <div>Content</div>
        </div>
    "#,
        800.0,
    );
    let sidebar = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "sidebar")
            .unwrap_or(false)
    })
    .unwrap();
    eprintln!(
        "sidebar box_sizing={:?} border_rect.w={} content_rect.w={}",
        sidebar.style.box_sizing, sidebar.layout.border_rect.w, sidebar.layout.content_rect.w
    );
    assert!(
        matches!(sidebar.style.box_sizing, BoxSizing::BorderBox),
        "sidebar should have box-sizing:border-box from * rule"
    );
    assert!(
        (sidebar.layout.border_rect.w - 170.0).abs() < 1.0,
        "sidebar border_rect.w should be 170 (border-box), got {}",
        sidebar.layout.border_rect.w
    );
}

/// Flex-column with align-items:center must shrink auto-width children to their
/// intrinsic width. If the h2 fills the full container it will be left-aligned;
/// if it shrinks to text width it will be centered (non-zero cross_pos offset).
#[test]
fn layout_flex_column_center_shrinks_child_to_intrinsic_width() {
    use super::harness::{find_box, parse_and_layout};
    // 400px-wide column, centered. The h2 text "Hi" is much narrower than 400px.
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        .col { display: flex; flex-direction: column; align-items: center; width: 400px; }
        h2 { font-size: 16px; }
        </style>
        <div class="col"><h2>Hi</h2></div>
    "#,
        800.0,
    );
    let col = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "col")
            .unwrap_or(false)
    })
    .unwrap();
    let h2 = find_box(col, &|b| b.tag == "h2").unwrap();
    // With centering, h2 must be narrower than the 400px container and have
    // a non-zero left offset (margin_rect.x > col.layout.content_rect.x).
    assert!(
        h2.layout.margin_rect.w < 380.0,
        "h2 should shrink to text width, not fill 400px; got w={}",
        h2.layout.margin_rect.w
    );
    assert!(
        h2.layout.margin_rect.x > col.layout.content_rect.x + 1.0,
        "h2 should be shifted right (centered); x={} col.layout.content_rect.x={}",
        h2.layout.margin_rect.x,
        col.layout.content_rect.x
    );
}

// ── linear-gradient geometry (css-images-3 §3.4.1) ───────────────────────────

/// **The gradient line runs corner to corner, through the centre of the box.**
/// Its length is `|W·sin A| + |H·cos A|` and its midpoint is the box centre, so
/// a hard 50% stop paints as the line through the centre perpendicular to the
/// gradient direction. For `45deg` — up and to the right — that boundary has
/// slope +1 in screen coordinates, with the first colour below-left of it.
#[test]
fn render_linear_gradient_45deg_line_crosses_the_centre() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px;
               background: linear-gradient(45deg, #ff0000 0%, #ff0000 50%,
                                                  #0000ff 50%, #0000ff 100%); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let is_red = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        r > 200 && g < 60 && b < 60
    };
    let is_blue = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        b > 200 && r < 60 && g < 60
    };
    // Centre (100, 20); boundary points (75,-5)…(135,55). Sampled well clear of it.
    assert!(is_red(75, 10), "(75,10) is below-left of the boundary");
    assert!(is_blue(115, 10), "(115,10) is above-right of the boundary");
    assert!(is_red(105, 30), "(105,30) is below-left of the boundary");
    assert!(is_blue(135, 30), "(135,30) is above-right of the boundary");
}

/// The same box at 135deg — down and to the right — mirrors it.
#[test]
fn render_linear_gradient_135deg_line_crosses_the_centre() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px;
               background: linear-gradient(135deg, #ff0000 0%, #ff0000 50%,
                                                   #0000ff 50%, #0000ff 100%); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let is_red = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        r > 200 && g < 60 && b < 60
    };
    let is_blue = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        b > 200 && r < 60 && g < 60
    };
    assert!(is_red(75, 30), "(75,30) is above-left of the boundary");
    assert!(is_blue(115, 30), "(115,30) is below-right of the boundary");
    assert!(is_red(105, 10), "(105,10) is above-left of the boundary");
    assert!(is_blue(135, 10), "(135,10) is below-right of the boundary");
}

#[test]
fn render_linear_gradient_corner_keyword_uses_box_aspect_ratio() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px;
               background: linear-gradient(to top right, #ff0000 0%, #ff0000 50%,
                                                          #0000ff 50%, #0000ff 100%); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let is_red = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        r > 200 && g < 60 && b < 60
    };
    let is_blue = |x, y| {
        let (r, g, b, _) = pixel(&pm, x, y);
        b > 200 && r < 60 && g < 60
    };
    assert!(
        is_red(90, 30),
        "wide-box corner gradient keeps lower-left red"
    );
    assert!(
        is_blue(110, 10),
        "wide-box corner gradient keeps upper-right blue"
    );
}

/// Axis-aligned gradients, which already worked, must keep working.
#[test]
fn render_linear_gradient_axis_aligned_still_spans_the_box() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px;
               background: linear-gradient(90deg, #ff0000 0%, #ff0000 50%,
                                                  #0000ff 50%, #0000ff 100%); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let (r, _, b, _) = pixel(&pm, 20, 20);
    assert!(r > 200 && b < 60, "left half is the first colour");
    let (r2, _, b2, _) = pixel(&pm, 180, 20);
    assert!(b2 > 200 && r2 < 60, "right half is the last colour");
}

/// **With no direction the gradient goes to the bottom and keeps every stop.**
/// The first component was read as a direction unconditionally, so
/// `linear-gradient(red, blue)` lost `red` and painted a flat blue.
#[test]
fn render_linear_gradient_without_a_direction_keeps_the_first_stop() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px; background: linear-gradient(#ff0000, #0000ff); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let (r, _, b, _) = pixel(&pm, 100, 2);
    assert!(r > 180 && b < 80, "top is red, got #{r:02x}..{b:02x}");
    let (r2, _, b2, _) = pixel(&pm, 100, 37);
    assert!(
        b2 > 180 && r2 < 80,
        "bottom is blue, got #{r2:02x}..{b2:02x}"
    );
}

/// A stop written as `rgb(…)` carries commas of its own; splitting the stop list
/// on every comma tore it into fragments that parsed as nothing.
#[test]
fn render_linear_gradient_accepts_functional_colour_stops() {
    let pm = render_html(
        r#"
        <style>* { margin:0; padding:0 }
        body { background: white }
        .bar { width: 200px; height: 40px;
               background: linear-gradient(90deg, rgb(255, 0, 0) 0%, rgb(255, 0, 0) 50%,
                                                  rgb(0, 0, 255) 50%, rgb(0, 0, 255) 100%); }
        </style><div class="bar"></div>
    "#,
        200,
        40,
    );
    let (r, _, b, _) = pixel(&pm, 20, 20);
    assert!(r > 200 && b < 60, "left half is red");
    let (r2, _, b2, _) = pixel(&pm, 180, 20);
    assert!(b2 > 200 && r2 < 60, "right half is blue");
}

// ── Intrinsic text width must agree with line breaking ───────────────────────

/// **A box sized to its own max-content must not wrap.** The two measurements
/// have to be the same number: `max_content_width` measures the text, and the
/// line breaker then decides where it fits. On fr.wikipedia the header links
/// came out ~8px narrower than the line they had to hold, so every one of them
/// broke onto a second line and pushed the page down.
#[test]
fn a_flex_item_sized_to_max_content_does_not_wrap_its_text() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"
        <style>* { margin: 0; padding: 0 }
        .row { display: flex; width: 600px }
        #b { white-space: nowrap }
        </style>
        <div class="row"><div id="a">Faire un don</div></div>
        <div class="row"><div id="b">Faire un don</div></div>
    "#,
        800.0,
    );
    let mut pm = tiny_skia::Pixmap::new(800, 200).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    let find = |id: &str| {
        fn walk<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
            if n.attributes.get("id").map(String::as_str) == Some(id) {
                return Some(n);
            }
            for c in &n.children {
                if let Some(f) = walk(c, id) {
                    return Some(f);
                }
            }
            None
        }
        walk(&doc.root, id).unwrap().layout.margin_rect
    };
    let (a, b) = (find("a"), find("b"));
    assert_eq!(
        a.h, b.h,
        "the auto-width item wrapped: {}x{} vs nowrap {}x{}",
        a.w, a.h, b.w, b.h
    );
    assert!(
        (a.w - b.w).abs() < 0.5,
        "max-content width {} != single-line width {}",
        a.w,
        b.w
    );
}

/// **Collapsible white space at the end of a block generates no line box.**
/// CSS Text 3 §4.1.3 removes a collapsible space that ends a line, and CSS 2.1
/// §9.4.2 treats a line box holding nothing else as not existing. The markup
/// `<li><a>…</a>\n</li>` is everywhere — fr.wikipedia's header links are
/// exactly that — and the stray newline gave every one of them a second, empty
/// line, doubling its height and pushing the page down.
#[test]
fn trailing_collapsible_whitespace_adds_no_line_box() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        "<style>* { margin: 0; padding: 0 } ul { list-style: none }</style>\
         <ul><li id=\"li_ws\"><a><span>Faire un don</span></a>\n</li>\
             <li id=\"li_no\"><a><span>Faire un don</span></a></li></ul>\
         <div id=\"div_ws\"><a><span>Faire un don</span></a>\n</div>\
         <div id=\"div_no\"><a><span>Faire un don</span></a></div>",
        800.0,
    );
    let mut pm = tiny_skia::Pixmap::new(800, 300).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    fn walk<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = walk(c, id) {
                return Some(f);
            }
        }
        None
    }
    let h = |id: &str| walk(&doc.root, id).unwrap().layout.margin_rect.h;
    assert_eq!(
        h("li_ws"),
        h("li_no"),
        "the newline gave the list item a second line: {} vs {}",
        h("li_ws"),
        h("li_no")
    );
    assert_eq!(
        h("div_ws"),
        h("div_no"),
        "the newline gave the block a second line: {} vs {}",
        h("div_ws"),
        h("div_no")
    );
}

/// **A border with no style occupies no space** (CSS Backgrounds §4.3: a
/// `border-style: none` border has a used width of 0). `border-width` computes
/// to `medium` — 3px — by default, and the inline-box decoration read that
/// width without consulting the style, so every nested inline box added 3px to
/// its first line and 3px to its last. fr.wikipedia's header links
/// ("Faire un don", "Créer un compte") were pushed past their own flex base
/// and broke onto a second line.
#[test]
fn a_nested_inline_box_adds_no_phantom_border_to_its_line() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        "<style>* { margin: 0; padding: 0 } \
         .list { display: flex; list-style: none; font-size: 14px; font-family: sans-serif } \
         .list li { display: list-item }</style>\
         <ul class=\"list\">\
           <li id=\"nested\"><a><span>Faire un don</span></a></li>\
           <li id=\"bare\">Faire un don</li>\
         </ul>",
        1280.0,
    );
    let mut pm = tiny_skia::Pixmap::new(1280, 200).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    fn walk<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = walk(c, id) {
                return Some(f);
            }
        }
        None
    }
    let line_w = |id: &str| {
        walk(&doc.root, id)
            .unwrap()
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .fold(0.0f32, f32::max)
    };
    let boxes = |id: &str| walk(&doc.root, id).unwrap().layout.line_cache.len();
    let (nested, bare) = (
        walk(&doc.root, "nested").unwrap(),
        walk(&doc.root, "bare").unwrap(),
    );

    assert_eq!(boxes("bare"), 1, "the bare text fits on one line");
    assert_eq!(
        boxes("nested"),
        1,
        "wrapping the same text in <a><span> broke it onto {} lines (widths {:?}, box {})",
        boxes("nested"),
        nested
            .layout
            .line_cache
            .iter()
            .map(|l| l.width)
            .collect::<Vec<_>>(),
        nested.layout.content_rect.w
    );
    assert!(
        (line_w("nested") - line_w("bare")).abs() < 0.5,
        "the nested line is {} wide, the bare one {}",
        line_w("nested"),
        line_w("bare")
    );
    assert!(
        (nested.layout.margin_rect.w - bare.layout.margin_rect.w).abs() < 0.5,
        "flex bases differ: nested {} vs bare {}",
        nested.layout.margin_rect.w,
        bare.layout.margin_rect.w
    );
}

/// **Measuring a string whole must agree with measuring its words and the
/// spaces between them.** The line breaker sums per-word advances plus a
/// measured `" "`, while `max_content_width` shapes the collapsed string in one
/// call. When the two disagree, a box built from one is the wrong size for the
/// line built by the other.
#[test]
fn whole_string_and_word_by_word_measurement_agree() {
    let mut renderer = Renderer::new();
    let _doc = renderer.load_html("<div>x</div>", 800.0);
    let engine = renderer.layout_engine();
    let w = |t: &str| {
        engine.measure_text_cached(
            t,
            14.0,
            crate::types::FontWeight::Normal,
            crate::types::FontStyle::Normal,
            "sans-serif",
        )
    };
    let space = w(" ");
    let parts = w("Faire") + space + w("un") + space + w("don");
    let whole = w("Faire un don");
    assert!(space > 0.5, "a space has a width: {space}");
    assert!(
        (parts - whole).abs() < 0.5,
        "word-by-word {parts} != whole-string {whole} (space={space}, \
         Faire={}, un={}, don={})",
        w("Faire"),
        w("un"),
        w("don")
    );
}

/// **The measuring and painting font resolvers must agree on generic family
/// names.** Sizing goes through `resolve_css_family`, painting through the
/// cheaper `css_family_to_cosmic`. They disagreed about `system-ui`: the first
/// maps it to the sans-serif generic, the second passed it through as a face
/// NAME, so a box was measured with one font and painted with another.
#[test]
fn the_two_font_resolvers_agree_on_generic_families() {
    use cosmic_text::Family;
    let same = |a: &Family, b: &Family| format!("{a:?}") == format!("{b:?}");
    let renderer = Renderer::new();
    let fs = &renderer.font_system;
    for stack in [
        "system-ui",
        "sans-serif",
        "serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui, sans-serif",
    ] {
        let painted = crate::layout::inline_layout::css_family_to_cosmic(stack);
        let resolved = crate::layout::inline_layout::resolve_css_family(fs, stack);
        let measured = resolved.as_family();
        assert!(
            same(&painted, &measured),
            "{stack:?}: painting picks {painted:?}, measuring picks {measured:?}"
        );
    }
}

// ── word-spacing and letter-spacing are MEASURED, not just painted ───────────
//
// css-text-3 §8.1 (word-spacing) and §8.2 (letter-spacing): the extra spacing
// is part of the text's advance width. Layout sums the inline items to get a
// shrink-to-fit width and to choose wrap points, so spacing that is missing
// from the item advances sizes the box for narrower text than gets painted —
// which clips the last character inside `overflow: hidden`.

#[test]
fn word_spacing_widens_the_shrink_to_fit_box() {
    use super::harness::find_box;
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        span { display: inline-block; font-size: 16px; }
        .ws { word-spacing: 10px; }
        </style>
        <div><span id="plain">a b c</span><span id="ws" class="ws">a b c</span></div>
    "#,
        800.0,
    );
    let w = |id: &str| {
        find_box(&doc.root, &|n| {
            n.attributes.get("id").map(|c| c == id).unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("#{id} not found"))
        .layout
        .content_rect
        .w
    };
    let (plain, ws) = (w("plain"), w("ws"));
    // Two word separators in "a b c" → exactly two extra 10px gaps.
    assert!(
        (ws - plain - 20.0).abs() < 0.5,
        "word-spacing:10px over two separators must widen the box by 20px; \
         plain={plain} word-spaced={ws}"
    );
}

#[test]
fn letter_spacing_widens_the_shrink_to_fit_box() {
    use super::harness::find_box;
    let doc = parse_and_layout(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        span { display: inline-block; font-size: 16px; }
        .ls { letter-spacing: 4px; }
        </style>
        <div><span id="plain">abcde</span><span id="ls" class="ls">abcde</span></div>
    "#,
        800.0,
    );
    let w = |id: &str| {
        find_box(&doc.root, &|n| {
            n.attributes.get("id").map(|c| c == id).unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("#{id} not found"))
        .layout
        .content_rect
        .w
    };
    let (plain, ls) = (w("plain"), w("ls"));
    // Tracking follows every one of the five letters, the last included.
    assert!(
        (ls - plain - 20.0).abs() < 0.5,
        "letter-spacing:4px over five letters must widen the box by 20px; \
         plain={plain} tracked={ls}"
    );
}

#[test]
fn letter_spacing_moves_the_wrap_point() {
    use super::harness::find_box;
    // Same text, same box width: without tracking both words share one line,
    // with tracking the second no longer fits. A spacing that never reached
    // the item advances could not change the break.
    let html = |ls: &str| {
        format!(
            r#"
        <style>
        * {{ margin: 0; padding: 0; }}
        .b {{ width: 120px; font-size: 16px; letter-spacing: {ls}; }}
        </style>
        <div class="b">aaaaa bbbbb</div>
    "#
        )
    };
    let lines = |src: &str| {
        let doc = parse_and_layout(src, 400.0);
        find_box(&doc.root, &|n| {
            n.attributes.get("class").map(|c| c == "b").unwrap_or(false)
        })
        .expect(".b not found")
        .layout
        .line_cache
        .len()
    };
    let tight = lines(&html("0"));
    let tracked = lines(&html("6px"));
    assert_eq!(
        tight, 1,
        "without tracking the two words fit on one line, got {tight}"
    );
    assert_eq!(tracked, 2,
        "letter-spacing:6px over 11 characters adds 66px and must force a second line, got {tracked}");
}

#[test]
fn letter_spacing_reaches_painted_glyph_positions() {
    fn red_text_bounds(pm: &tiny_skia::Pixmap) -> Option<(u32, u32)> {
        let mut first = None;
        let mut last = None;
        for y in 0..80u32 {
            for x in 0..240u32 {
                let (r, g, b, a) = pixel(pm, x, y);
                if a > 10 && r > 130 && g < 90 && b < 90 {
                    first.get_or_insert(x);
                    last = Some(x);
                }
            }
        }
        first.zip(last)
    }

    let html = |spacing: &str| {
        format!(
            r#"
        <style>
        * {{ margin: 0; padding: 0; }}
        body {{ background: white; }}
        div {{ width: 220px; font-size: 32px; color: red; letter-spacing: {spacing}; }}
        </style>
        <div>abcd</div>
    "#
        )
    };

    let plain = render_html(&html("0"), 240, 80);
    let spaced = render_html(&html("8px"), 240, 80);
    let (plain_first, plain_last) = red_text_bounds(&plain).expect("plain text painted");
    let (spaced_first, spaced_last) = red_text_bounds(&spaced).expect("spaced text painted");
    let plain_width = plain_last.saturating_sub(plain_first);
    let spaced_width = spaced_last.saturating_sub(spaced_first);

    assert!(
        spaced_width > plain_width + 16,
        "letter-spacing should widen the painted glyph bounds; plain={plain_width} spaced={spaced_width}"
    );
}

#[test]
fn word_spacing_reaches_painted_word_positions() {
    fn red_text_bounds(pm: &tiny_skia::Pixmap) -> Option<(u32, u32)> {
        let mut first = None;
        let mut last = None;
        for y in 0..80u32 {
            for x in 0..260u32 {
                let (r, g, b, a) = pixel(pm, x, y);
                if a > 10 && r > 130 && g < 90 && b < 90 {
                    first.get_or_insert(x);
                    last = Some(x);
                }
            }
        }
        first.zip(last)
    }

    let html = |spacing: &str| {
        format!(
            r#"
        <style>
        * {{ margin: 0; padding: 0; }}
        body {{ background: white; }}
        div {{ width: 240px; font-size: 32px; color: red; word-spacing: {spacing}; }}
        </style>
        <div>a b c</div>
    "#
        )
    };

    let plain = render_html(&html("0"), 260, 80);
    let spaced = render_html(&html("24px"), 260, 80);
    let (plain_first, plain_last) = red_text_bounds(&plain).expect("plain text painted");
    let (spaced_first, spaced_last) = red_text_bounds(&spaced).expect("spaced text painted");
    let plain_width = plain_last.saturating_sub(plain_first);
    let spaced_width = spaced_last.saturating_sub(spaced_first);

    assert!(
        spaced_width > plain_width + 18,
        "word-spacing should widen the painted word bounds; plain={plain_width} spaced={spaced_width}"
    );
}

#[test]
fn flex_container_direct_text_child_paints() {
    let pm = render_html(
        r#"
        <style>
        * { margin: 0; padding: 0; }
        body { background: white; }
        h2 { display: flex; align-items: center; height: 46px; font-size: 18px; color: rgb(35,42,49); }
        </style>
        <h2>Top Stories</h2>
    "#,
        180,
        80,
    );

    let mut dark_pixels = 0usize;
    for y in 0..70u32 {
        for x in 0..170u32 {
            let (r, g, b, a) = pixel(&pm, x, y);
            if a > 0 && r < 90 && g < 100 && b < 110 {
                dark_pixels += 1;
            }
        }
    }

    assert!(
        dark_pixels > 20,
        "direct text child of a flex container should paint; dark pixels={dark_pixels}"
    );
}
