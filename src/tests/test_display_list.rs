//! Tests for the display list builder and replay.

use crate::frame::EngineFrame;
use crate::html::{parse_html, parse_html_with_base};
use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd};
use crate::renderer::display_list_builder::{build_display_list, build_display_list_full};
use crate::renderer::display_list_replay::{reduce_corner_radii, replay, replay_with_scroll};
use crate::types::{Color, Rect};
use crate::Renderer;

fn build(html: &str) -> (EngineFrame, DisplayList) {
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();
    let list = build_display_list(&f.doc.root, 800.0, 600.0);
    (f, list)
}

fn build_full(html: &str) -> (EngineFrame, DisplayList) {
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();
    let list = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    (f, list)
}

#[test]
fn rtl_mixed_inline_text_commands_stay_inside_line_box() {
    let (_frame, list) = build_full(
        r##"<html dir="rtl"><body style="margin:0">
             <div style="width:800px; font:16px sans-serif">
               <span>محمد بن عبد الواحد</span>
               <b>منصور السعدي</b>
               <a href="#">المقدسي</a>
             </div>
           </body></html>"##,
    );

    let text_commands: Vec<(f32, String)> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { x, text, .. } if text.trim().len() > 1 => Some((*x, text.clone())),
            _ => None,
        })
        .collect();
    assert!(
        !text_commands.is_empty(),
        "expected rendered RTL text commands"
    );
    for (x, text) in text_commands {
        assert!(
            (0.0..=800.0).contains(&x),
            "RTL text command '{text}' should start inside the 800px line box, got x={x:.1}"
        );
    }
}

#[test]
fn rtl_visual_segments_keep_geometry_after_relayout_cache_reuse() {
    let mut doc = parse_html(
        r##"<html dir="rtl"><body style="margin:0">
             <p id="t" style="width:604px;font:16px sans-serif">
               محمد بن عبد الواحد <b>منصور السعدي</b> المقدسي، سنة 569 هـ. كُنِّي بأبي عبد الله.
             </p>
           </body></html>"##,
    );
    let mut renderer = Renderer::new();
    renderer.layout_engine().layout(&mut doc, 800.0);
    renderer.layout_engine().layout(&mut doc, 800.0);

    let paragraph = crate::dom::query_selector(&doc.root, "#t").expect("paragraph should exist");
    let visual_widths: Vec<f32> = paragraph
        .layout
        .line_cache
        .iter()
        .flat_map(|line| line.visual_segments.iter().map(|seg| seg.width))
        .collect();
    assert!(
        visual_widths.iter().any(|w| *w > 1.0),
        "cached relayout must preserve RTL visual segment geometry: {visual_widths:?}"
    );
}

#[test]
fn video_with_controls_paints_media_surface() {
    let (_frame, list) =
        build(r#"<video id="movie" controls width="320" height="180" data-duration="75"></video>"#);
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::FillRect { color, .. } if color.r == 16 && color.g == 18 && color.b == 24
        )),
        "video should paint a media viewport"
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::Text { text, .. } if text == "Play"
        )),
        "video controls should paint an activation label"
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::Text { text, .. } if text == "1:15"
        )),
        "video controls should paint the media duration"
    );
}

#[test]
fn autoplay_background_video_without_frame_does_not_paint_debug_label() {
    let (_frame, list) = build(
        r#"<video id="player-bg" autoplay muted loop width="320" height="180">
             <source src="hero.mp4" type="video/mp4">
           </video>"#,
    );
    assert!(
        !list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::FillRect { color, .. } if color.r == 16 && color.g == 18 && color.b == 24
        )),
        "uncontrolled videos without decoded pixels should not cover page fallback content"
    );
    assert!(
        !list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::Text { text, .. } if text == "No video frame"
        )),
        "uncontrolled videos must not expose internal no-frame placeholders"
    );
}

#[test]
fn video_svg_poster_paints_through_native_svg_path() {
    let poster = "data:image/svg+xml,%3Csvg%20viewBox='0%200%2010%2010'%20xmlns='http://www.w3.org/2000/svg'%3E%3Crect%20width='10'%20height='10'%20fill='red'/%3E%3C/svg%3E";
    let html = format!(
        r#"<video controls width="100" height="80" poster="{}"></video>"#,
        poster
    );
    let (_frame, list) = build(&html);
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::Image { data: ImageRef::Owned(_, w, h), .. } if *w == 100 && *h == 80
        )),
        "SVG video posters should rasterize through the native SVG image path"
    );
}

#[test]
fn audio_with_controls_paints_compact_media_surface() {
    let (_frame, list) =
        build(r#"<audio id="sound" controls data-duration="9"><source src="tone.ogg"></audio>"#);
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::FillRect { color, .. } if color.r == 245 && color.g == 247 && color.b == 250
        )),
        "audio should paint a compact media surface"
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::Text { text, .. } if text == "0:09"
        )),
        "audio controls should paint the media duration"
    );
}

fn count_opaque_pixels(pixmap: &tiny_skia::Pixmap, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    let width = pixmap.width();
    let height = pixmap.height();
    let x1 = x1.min(width);
    let y1 = y1.min(height);
    let data = pixmap.data();
    let mut count = 0;
    for y in y0.min(height)..y1 {
        for x in x0.min(width)..x1 {
            let idx = ((y * width + x) * 4 + 3) as usize;
            if data.get(idx).copied().unwrap_or(0) > 0 {
                count += 1;
            }
        }
    }
    count
}

fn find_node_by_tag<'a>(node: &'a crate::WebCore, tag: &str) -> Option<&'a crate::WebCore> {
    if node.tag == tag {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_node_by_tag(child, tag))
}

#[test]
fn border_radius_overlap_uses_one_proportional_scale_factor() {
    let reduced = reduce_corner_radii(100.0, 40.0, [80.0, 40.0, 10.0, 30.0]);

    let expected = [29.09091, 14.545455, 3.6363637, 10.909091];
    for (actual, expected) in reduced.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 0.0001);
    }
}

// ── Basic commands ──────────────────────────────────────────────────────────

#[test]
fn non_empty_doc_produces_commands() {
    let (_, list) = build("<div style='background: white'>x</div>");
    assert!(!list.is_empty(), "doc with content should produce commands");
}

#[test]
fn text_children_are_not_painted_from_stale_child_geometry() {
    fn first_text_mut(n: &mut crate::WebCore) -> Option<&mut crate::WebCore> {
        if n.tag == "#text" {
            return Some(n);
        }
        for child in &mut n.children {
            if let Some(found) = first_text_mut(child) {
                return Some(found);
            }
        }
        None
    }

    let doc = parse_html(
        "<style>*{margin:0;padding:0} h3{font:40px/46px sans-serif;width:300px}</style>\
         <h3>Headline text wraps here</h3>",
    );
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();

    let parent_lines = f
        .doc
        .root
        .children
        .iter()
        .flat_map(|n| n.children.iter())
        .find(|n| n.tag == "h3")
        .expect("h3")
        .layout
        .line_cache
        .clone();
    let text = first_text_mut(&mut f.doc.root).expect("text child");
    text.layout.line_cache = parent_lines;
    for line in &mut text.layout.line_cache {
        line.y += 100_000.0;
    }
    text.layout.content_rect.y += 100_000.0;
    text.layout.border_rect.y += 100_000.0;

    let list = build_display_list(&f.doc.root, 800.0, 600.0);
    let text_ys: Vec<f32> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, y, .. } if text.contains("Headline") => Some(*y),
            _ => None,
        })
        .collect();

    assert!(
        !text_ys.is_empty(),
        "parent line cache should still paint text"
    );
    assert!(
        text_ys.iter().all(|y| *y < 1000.0),
        "stale child #text geometry must not emit offscreen text commands: {text_ys:?}"
    );
}

#[test]
fn colored_div_has_fill_rect() {
    let (_, list) =
        build(r#"<div style="background-color: red; width: 100px; height: 50px">x</div>"#);
    let has_red = list.commands.iter().any(
        |cmd| matches!(cmd, PaintCmd::FillRect { color, .. } if color.r == 255 && color.g == 0),
    );
    assert!(has_red, "red div should produce FillRect with red");
}

#[test]
fn inline_svg_uses_cascaded_fill_color_when_rasterized() {
    let (frame, list) = build(
        r#"<style>svg { color: rgb(255, 0, 0); fill: currentColor; }</style>
           <svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect x="0" y="0" width="20" height="20"/>
           </svg>"#,
    );
    let svg = find_node_by_tag(&frame.doc.root, "svg").expect("inline SVG node");
    assert!(
        svg.svg_document.is_some(),
        "inline SVG should keep its parsed native SVG tree for browser paint"
    );

    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, h) = image.expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] > 200 && data[idx + 1] < 50 && data[idx + 2] < 50 && data[idx + 3] > 200,
        "center pixel should be red from cascaded fill, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn inline_svg_uses_css_animated_fill_when_rasterized() {
    let (_, list) = build(
        r#"<style>
             @keyframes icon-fill { from { fill: rgb(255, 0, 0); } to { fill: rgb(0, 0, 255); } }
             svg { fill: rgb(0, 0, 255); animation: icon-fill 10s linear; }
           </style>
           <svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect x="0" y="0" width="20" height="20"/>
           </svg>"#,
    );

    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, h) = image.expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] > 200 && data[idx + 1] < 50 && data[idx + 2] < 50 && data[idx + 3] > 200,
        "center pixel should use animated fill at animation start, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn inline_svg_uses_native_animate_fill_when_rasterized() {
    let (_, list) = build(
        r#"<svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect x="0" y="0" width="20" height="20" fill="blue">
               <animate attributeName="fill" from="rgb(255,0,0)" to="rgb(0,0,255)" dur="10s"/>
             </rect>
           </svg>"#,
    );

    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, h) = image.expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] > 200 && data[idx + 1] < 50 && data[idx + 2] < 50 && data[idx + 3] > 200,
        "center pixel should use native SVG animate fill at animation start, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn inline_svg_resolves_inherited_custom_property_paint() {
    let (_, list) = build(
        r#"<div style="--icon-color: rgb(25, 103, 210)">
             <svg style="width:20px;height:14px;color:blue" viewBox="0 0 20 14" fill="none">
               <path d="M0 0H20V14H0Z" fill="var(--icon-color)"/>
             </svg>
           </div>"#,
    );

    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, _) = image.expect("inline SVG should rasterize to an image command");
    let idx = ((7 * w + 10) * 4) as usize;
    assert!(
        data[idx] < 50 && data[idx + 1] > 80 && data[idx + 2] > 180 && data[idx + 3] > 200,
        "SVG paint var() should resolve inherited custom properties, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn inline_svg_child_selector_style_reaches_native_paint() {
    let (_, list) = build(
        r#"<style>svg rect.badge { fill: rgb(12, 150, 90); }</style>
           <svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect class="badge" x="0" y="0" width="20" height="20" fill="red"/>
           </svg>"#,
    );

    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, h) = image.expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] < 40 && data[idx + 1] > 120 && data[idx + 2] > 70 && data[idx + 3] > 200,
        "center pixel should come from DOM-cascaded projected child style, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn decoded_svg_image_uses_parsed_native_svg_tree_when_rasterized() {
    fn find_img_mut(node: &mut crate::WebCore) -> Option<&mut crate::WebCore> {
        if node.tag == "img" {
            return Some(node);
        }
        node.children.iter_mut().find_map(find_img_mut)
    }

    let doc = parse_html(r#"<img style="width:20px;height:20px">"#);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    let img = find_img_mut(&mut frame.doc.root).expect("img node");
    crate::html::set_decoded_image_on_node(
        img,
        crate::html::DecodedImage::Svg(
            r#"<svg width="20" height="20"><style>.brand{fill:rgb(88, 0, 150)}</style><rect class="brand" width="20" height="20"/></svg>"#
                .to_string(),
            20.0,
            20.0,
        ),
    );
    assert!(
        img.svg_document.is_some(),
        "decoded SVG images should keep a parsed native SVG tree for browser paint"
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, h) = image.expect("decoded SVG image should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] > 70 && data[idx + 1] < 40 && data[idx + 2] > 120 && data[idx + 3] > 200,
        "center pixel should come from the SVG's internal CSS, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn decoded_svg_image_uses_isolated_svg_document_color() {
    fn find_img_mut(node: &mut crate::WebCore) -> Option<&mut crate::WebCore> {
        if node.tag == "img" {
            return Some(node);
        }
        node.children.iter_mut().find_map(find_img_mut)
    }

    let doc = parse_html(r#"<img style="width:20px;height:20px;color:white">"#);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    let img = find_img_mut(&mut frame.doc.root).expect("img node");
    crate::html::set_decoded_image_on_node(
        img,
        crate::html::DecodedImage::Svg(
            r#"<svg width="20" height="20" color="rgb(80, 0, 160)"><rect width="20" height="20" fill="currentColor"/></svg>"#
                .to_string(),
            20.0,
            20.0,
        ),
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        _ => None,
    });
    let (data, w, _) = image.expect("decoded SVG image should rasterize to an image command");
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] > 60 && data[idx] < 110 && data[idx + 1] < 40 && data[idx + 2] > 130,
        "SVG image should use its own document color, not embedding img color; got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn text_node_produces_text_command() {
    let (_, list) = build("<p>Hello World</p>");
    let has_text = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.contains("Hello")));
    assert!(has_text, "text should produce Text command");
}

#[test]
fn border_produces_border_command() {
    let (_, list) =
        build(r#"<div style="border: 2px solid blue; width: 100px; height: 50px">x</div>"#);
    let has_border = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::Border { widths, .. } if widths[0] > 0.0));
    assert!(has_border, "border should produce Border command");
}

#[test]
fn display_none_produces_nothing() {
    let (_, list) = build(r#"<div style="display: none; background: red">hidden</div>"#);
    let has_hidden = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.contains("hidden")));
    assert!(!has_hidden, "display:none should not produce commands");
}

// ── Clip and opacity ────────────────────────────────────────────────────────

#[test]
fn overflow_hidden_produces_clip() {
    let (_, list) =
        build(r#"<div style="overflow: hidden; width: 100px; height: 50px"><p>content</p></div>"#);
    let has_clip = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::PushClip { .. }));
    let has_pop = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::PopClip));
    assert!(
        has_clip && has_pop,
        "overflow:hidden should produce PushClip/PopClip"
    );
}

#[test]
fn opacity_produces_push_pop() {
    let (_, list) = build(r#"<div style="opacity: 0.5; width: 100px; height: 50px">semi</div>"#);
    let has_op = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::PushOpacity { alpha } if (*alpha - 0.5).abs() < 0.01));
    assert!(has_op, "opacity should produce PushOpacity");
}

#[test]
fn stacking_context_for_z_index() {
    let (_, list) =
        build(r#"<div style="position: relative; z-index: 5; width: 100px; height: 50px">z</div>"#);
    let has_ctx = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::BeginStackingContext { z_index, .. } if *z_index == 5));
    assert!(has_ctx, "z-index should create stacking context");
}

#[test]
fn explicit_zero_z_index_creates_stacking_context() {
    let (_, list) =
        build(r#"<div style="position: relative; z-index: 0; width: 100px; height: 50px">z</div>"#);
    let has_ctx = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::BeginStackingContext { z_index, .. } if *z_index == 0));
    assert!(
        has_ctx,
        "explicit z-index:0 should create a stacking context"
    );
}

#[test]
fn z_indexed_descendant_inside_plain_wrapper_competes_with_siblings() {
    let (_, list) = build(
        r#"<style>
             body { margin: 0 }
             .box { position: relative; width: 40px; height: 40px; }
             #front { z-index: 10; background-color: red; }
             #middle { z-index: 5; margin-top: -40px; background-color: blue; }
           </style>
           <div id="wrap"><div id="front" class="box"></div></div>
           <div id="middle" class="box"></div>"#,
    );
    let painted: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::FillRect { color, .. } if color.a > 0 => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();
    assert_eq!(
        painted.last().copied(),
        Some((255, 0, 0)),
        "nested z-index:10 must paint above sibling z-index:5"
    );
}

#[test]
fn clip_path_inset_and_circle_emit_display_list_clips() {
    let (_, inset) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:100px;height:80px;clip-path:inset(10px 20px 30px 5px)">x</div>"#,
    );
    let inset_clip = inset
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushClip { rect, radius, .. } if *radius == [0.0; 4] => Some(*rect),
            _ => None,
        })
        .expect("inset clip should be emitted");
    assert!((inset_clip.x - 5.0).abs() < 0.5);
    assert!((inset_clip.y - 10.0).abs() < 0.5);
    assert!((inset_clip.w - 75.0).abs() < 0.5);
    assert!((inset_clip.h - 40.0).abs() < 0.5);

    let (_, circle) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:100px;height:80px;clip-path:circle(25% at 50% 50%)">x</div>"#,
    );
    let has_circle_clip = circle.commands.iter().any(|cmd| {
        matches!(cmd, PaintCmd::PushClip { radius, .. } if radius.iter().all(|r| (*r - 20.0).abs() < 0.5))
    });
    assert!(has_circle_clip, "circle clip should map to a rounded clip");

    let (_, ellipse) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:120px;height:80px;clip-path:ellipse(40px 20px at 50% 50%)">x</div>"#,
    );
    let has_ellipse_clip = ellipse.commands.iter().any(|cmd| {
        matches!(cmd, PaintCmd::PushClip { rect, radius, .. }
            if (rect.x - 20.0).abs() < 0.5
                && (rect.y - 20.0).abs() < 0.5
                && (rect.w - 80.0).abs() < 0.5
                && (rect.h - 40.0).abs() < 0.5
                && radius.iter().all(|r| (*r - 20.0).abs() < 0.5))
    });
    assert!(
        has_ellipse_clip,
        "ellipse clip should emit a display-list clip"
    );

    let (_, polygon) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:100px;height:100px;clip-path:polygon(0 0, 100% 0, 0 100%);background:red"></div>"#,
    );
    assert!(
        polygon
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::PushClipPath { points } if points.len() == 3)),
        "polygon clip should emit a polygon display-list clip"
    );

    let mut pixmap = tiny_skia::Pixmap::new(120, 120).unwrap();
    replay(&polygon, &mut pixmap, 1.0);
    let alpha_at = |x: usize, y: usize| pixmap.data()[(y * 120 + x) * 4 + 3];
    assert!(
        alpha_at(10, 10) > 0,
        "paint inside the polygon should remain visible"
    );
    assert_eq!(
        alpha_at(90, 90),
        0,
        "paint outside the polygon should be clipped"
    );
}

#[test]
fn contain_paint_emits_a_descendant_clip() {
    let (_, list) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:20px;height:20px;contain:paint">
             <div style="width:60px;height:20px;background:red"></div>
           </div>"#,
    );
    let has_clip = list.commands.iter().any(|cmd| {
        matches!(cmd, PaintCmd::PushClip { rect, .. } if (rect.w - 20.0).abs() < 0.5 && (rect.h - 20.0).abs() < 0.5)
    });
    assert!(has_clip, "contain:paint should clip descendant paint");
}

#[test]
fn filter_drop_shadow_paints_offset_shadow_from_display_list() {
    let (_, list) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:10px;height:10px;background:black;filter:drop-shadow(10px 0 0 red)"></div>"#,
    );
    let mut pixmap = tiny_skia::Pixmap::new(40, 20).unwrap();
    replay(&list, &mut pixmap, 1.0);

    let source = pixmap.pixel(5, 5).expect("source pixel in bounds");
    assert!(
        source.alpha() > 0,
        "filtered source content should remain painted"
    );
    let shadow = pixmap.pixel(15, 5).expect("shadow pixel in bounds");
    assert!(
        shadow.red() > 0 && shadow.alpha() > 0,
        "drop-shadow should paint at the requested offset; got rgba({}, {}, {}, {})",
        shadow.red(),
        shadow.green(),
        shadow.blue(),
        shadow.alpha()
    );
}

#[test]
fn backdrop_filter_emits_a_backdrop_filter_command() {
    let (_, list) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:20px;height:20px;background:red"></div>
           <div style="position:absolute;left:0;top:0;width:20px;height:20px;backdrop-filter:grayscale(1)"></div>"#,
    );

    let command = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::BackdropFilter { rect, filters } => Some((rect, filters)),
        _ => None,
    });

    let (rect, filters) = command.expect("backdrop-filter command");
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0.0, 0.0, 20.0, 20.0));
    assert!(
        filters
            .iter()
            .any(|(kind, value, _, _, _)| *kind == 3 && (*value - 1.0).abs() < 0.001),
        "grayscale backdrop filter should use the shared filter encoding"
    );
}

#[test]
fn modal_dialog_backdrop_paints_viewport_fill() {
    let mut r = crate::Renderer::new();
    let mut doc = r.load_html(
        r#"<style>
           dialog::backdrop { background-color: rgba(0, 0, 0, 0.5); }
           </style>
           <dialog id="modal">Hi</dialog>"#,
        800.0,
    );
    let modal = doc.get_element_by_id("modal").unwrap();
    doc.show_dialog(modal, true);
    doc.recascade();
    let modal_node = doc
        .find_webcore(modal)
        .expect("modal node should remain in the render tree");
    assert!(
        modal_node.style.backdrop_style.is_some(),
        "dialog::backdrop should cascade onto the modal dialog"
    );

    let list = build_display_list_full(
        &doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    let commands: Vec<_> = list
        .commands
        .iter()
        .chain(list.fixed_commands.iter())
        .collect();
    assert!(
        commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::FillRect {
                rect,
                color,
                radius,
                radius_y,
            } if *rect == Rect::new(0.0, 0.0, 800.0, 600.0)
                && color.r == 0
                && color.g == 0
                && color.b == 0
                && (120..=135).contains(&color.a)
                && *radius == [0.0; 4]
                && *radius_y == [0.0; 4]
        )),
        "modal dialog should emit a viewport-sized ::backdrop fill"
    );
}

#[test]
fn filter_will_change_isolation_and_blend_create_stacking_contexts() {
    for style in [
        "filter: blur(0px)",
        "will-change: transform",
        "isolation: isolate",
        "mix-blend-mode: multiply",
    ] {
        let (_, list) = build(&format!(
            r#"<div style="{style}; width: 100px; height: 50px">stack</div>"#
        ));
        let has_ctx = list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BeginStackingContext { .. }));
        assert!(has_ctx, "{style} should create a stacking context");
    }
}

// ── Inline content with line_cache ──────────────────────────────────────────

#[test]
fn inline_text_produces_text_commands() {
    let (_, list) = build(
        r#"<p style="width: 200px">This is a paragraph with some text content that should wrap.</p>"#,
    );
    let text_cmds: Vec<_> = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::Text { .. }))
        .collect();
    assert!(
        !text_cmds.is_empty(),
        "paragraph text should produce Text commands"
    );
}

#[test]
fn styled_spans_produce_separate_runs() {
    let html =
        r#"<p><span style="color: red">Red</span> <span style="color: blue">Blue</span></p>"#;
    let (_, list) = build(html);
    let text_cmds: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| {
            if let PaintCmd::Text { text, color, .. } = cmd {
                Some((text.clone(), *color))
            } else {
                None
            }
        })
        .collect();
    // Should have at least text commands
    assert!(
        !text_cmds.is_empty(),
        "styled spans should produce text commands"
    );
}

// ── Hover style switching ───────────────────────────────────────────────────

#[test]
fn hover_style_applied_in_display_list() {
    let html = r#"<html><head><style>
        .btn { background-color: gray; width: 100px; height: 40px; }
        .btn:hover { background-color: red; }
    </style></head><body>
        <div class="btn" id="btn">Click</div>
    </body></html>"#;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();

    let btn_id = f.doc.get_element_by_id("btn").unwrap();

    // Without hover
    let list_no_hover = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    // With hover on button
    let list_hover = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        btn_id,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    // Count red FillRects in each
    let red_no_hover = list_no_hover
        .commands
        .iter()
        .filter(
            |cmd| matches!(cmd, PaintCmd::FillRect { color, .. } if color.r > 200 && color.g < 50),
        )
        .count();
    let red_hover = list_hover
        .commands
        .iter()
        .filter(
            |cmd| matches!(cmd, PaintCmd::FillRect { color, .. } if color.r > 200 && color.g < 50),
        )
        .count();

    assert!(
        red_hover > red_no_hover,
        "hover should add a red background: no_hover={} hover={}",
        red_no_hover,
        red_hover
    );
}

// ── Scrolling ───────────────────────────────────────────────────────────────

/// ⛔ The display list is SCROLL-INDEPENDENT — the scroll is applied at replay.
///
/// This test used to assert the opposite: that building at scroll 100 moved
/// the rect up by 100. That was the old contract, and it is why scrolling cost
/// seconds — a list with the offset baked in is only valid at that one offset,
/// so the page could only move by rebuilding the whole document's list. The
/// list is now built in DOCUMENT coordinates and translated by
/// `replay_with_scroll`, which is what lets one build serve every scroll
/// position.
///
/// The scroll argument survives for `position: sticky` alone — the one scheme
/// whose position really is a function of the scroll.
#[test]
fn the_display_list_is_scroll_independent() {
    let html = r#"<div style="background: blue; width: 100px; height: 50px; position: absolute; top: 200px; left: 100px">x</div>"#;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();

    let unscrolled = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    let scrolled = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        100.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    fn find_blue_y(list: &DisplayList) -> Option<f32> {
        for cmd in &list.commands {
            if let PaintCmd::FillRect { rect, color, .. } = cmd {
                if color.b > 200 && color.r < 50 {
                    return Some(rect.y);
                }
            }
        }
        None
    }
    let y1 = find_blue_y(&unscrolled).expect("the blue box is painted");
    let y2 = find_blue_y(&scrolled).expect("the blue box is painted");

    assert!(
        (y1 - y2).abs() < 0.01,
        "the same box must build to the same document position whatever the \
         scroll — got {y1} and {y2}; a scroll-dependent list cannot be cached"
    );
    assert!(
        (y1 - 200.0).abs() < 2.0,
        "and that position is the DOCUMENT one"
    );
}

#[test]
fn replay_scrolls_background_image_clips_with_the_image() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BackgroundImage {
        container: Rect::new(10.0, 100.0, 20.0, 20.0),
        clip: Rect::new(10.0, 100.0, 20.0, 20.0),
        data: ImageRef::Owned(vec![255, 0, 0, 255], 1, 1),
        size_mode: 3,
        draw_w: 20.0,
        draw_h: 20.0,
        pos_x: 10.0,
        pos_y: 100.0,
        repeat_x_mode: 0,
        repeat_y_mode: 0,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
        blend_mode: 0,
    });

    let mut pixmap = tiny_skia::Pixmap::new(80, 80).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    replay_with_scroll(
        &list,
        &mut pixmap,
        1.0,
        &mut font_system,
        &mut swash_cache,
        0.0,
        50.0,
    );

    let red = pixmap.pixel(15, 55).expect("sample inside scrolled image");
    assert_eq!(
        (red.red(), red.green(), red.blue(), red.alpha()),
        (255, 0, 0, 255),
        "background-image and its clip mask must scroll together"
    );

    let white = pixmap.pixel(15, 35).expect("sample above scrolled image");
    assert_eq!(
        (white.red(), white.green(), white.blue(), white.alpha()),
        (255, 255, 255, 255),
        "background-image must remain clipped after scroll"
    );
}

#[test]
fn replay_scrolls_form_element_content() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::FormElement {
        tag: "input".to_string(),
        input_type: "checkbox".to_string(),
        rect: Rect::new(10.0, 100.0, 20.0, 20.0),
        node_id: 1,
        attributes: Vec::new(),
        font_size: 16.0,
        font_weight: 400,
        font_family: "Arial".to_string(),
        color: Color::BLACK,
        placeholder_color: Color::rgba(0, 0, 0, 128),
        checked: true,
        value: String::new(),
        placeholder: String::new(),
        input_cursor: 0,
        appearance_none: false,
        vertical: false,
        options: Vec::new(),
        selected: -1,
        selected_all: Vec::new(),
    });

    let mut pixmap = tiny_skia::Pixmap::new(80, 80).unwrap();
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    replay_with_scroll(
        &list,
        &mut pixmap,
        1.0,
        &mut font_system,
        &mut swash_cache,
        0.0,
        50.0,
    );

    let painted_in_scrolled_position = count_opaque_pixels(&pixmap, 8, 48, 34, 74);
    let painted_at_unscrolled_position = count_opaque_pixels(&pixmap, 8, 0, 34, 38);
    assert!(
        painted_in_scrolled_position > 0,
        "form element content should paint at its scrolled viewport position"
    );
    assert_eq!(
        painted_at_unscrolled_position, 0,
        "form element content must not stay fixed while the page scrolls"
    );
}

#[test]
fn replay_scrolls_text_shadow_with_text() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::TextShadow {
        x: 10.0,
        y: 100.0,
        text: "Welcome".to_string(),
        font_family: "Arial".to_string(),
        font_size: 24.0,
        font_weight: 700,
        font_style: 0,
        font_stretch: 100.0,
        line_height: 30.0,
        color: Color::rgba(0, 0, 0, 255),
        blur: 0.0,
    });

    let mut pixmap = tiny_skia::Pixmap::new(180, 100).unwrap();
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    replay_with_scroll(
        &list,
        &mut pixmap,
        1.0,
        &mut font_system,
        &mut swash_cache,
        0.0,
        50.0,
    );

    let scrolled_band = count_opaque_pixels(&pixmap, 0, 45, 170, 90);
    let fixed_band = count_opaque_pixels(&pixmap, 0, 0, 170, 40);
    assert!(
        scrolled_band > 0,
        "text shadow should move with page scroll"
    );
    assert_eq!(
        fixed_band, 0,
        "text shadow must not remain at the unscrolled document position"
    );
}

#[test]
fn sticky_right_bottom_insets_clamp_to_viewport_edges() {
    let html = r#"
        <div style="height: 200px"></div>
        <div style="position: sticky; right: 15px; bottom: 10px; margin-left: 200px; width: 20px; height: 20px; background-color: red"></div>
    "#;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 100.0, 100.0);
    f.update_frame();
    let list = build_display_list_full(
        &f.doc.root,
        100.0,
        100.0,
        0.0,
        130.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    let red = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. } if color.r == 255 && color.g == 0 => Some(*rect),
            _ => None,
        })
        .expect("sticky box should paint a red background");

    assert!(
        (red.x - 65.0).abs() < 0.01,
        "right:15 should clamp the 20px box to x=65 in a 100px viewport, got {}",
        red.x
    );
    assert!(
        (red.y - 200.0).abs() < 0.01,
        "bottom:10 should keep the box above the viewport bottom when scrolled, got {}",
        red.y
    );
}

#[test]
fn sticky_position_creates_a_stacking_context() {
    let html = r#"
        <style>
          body { margin: 0; }
          #sticky { position: sticky; top: 0; width: 20px; height: 20px; background: red; }
        </style>
        <div id="sticky"></div>
    "#;
    let (_f, list) = build_full(html);

    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BeginStackingContext { .. })),
        "position: sticky should establish a stacking context"
    );
}

#[test]
fn sticky_inside_overflow_scroll_container_uses_container_scrollport() {
    let html = r#"
        <style>
          body { margin: 0; }
          #scroller {
            overflow: scroll;
            width: 200px;
            height: 200px;
            margin-top: 100px;
          }
          #spacer {
            height: 100px;
          }
          #sticky {
            position: sticky;
            top: 10px;
            width: 50px;
            height: 30px;
            background-color: rgb(255, 0, 0);
          }
          #content {
            height: 800px;
          }
        </style>
        <div id="scroller">
          <div id="spacer"></div>
          <div id="sticky"></div>
          <div id="content"></div>
        </div>
    "#;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 800.0);
    f.update_frame();

    let scroller_id = f.doc.get_element_by_id("scroller").expect("scroller");
    f.doc.element_scroll_to(scroller_id, 0.0, 150.0);

    let list = build_display_list_full(
        &f.doc.root,
        800.0,
        800.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    let red = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("sticky box should paint a red background");

    assert!(
        (red.y - 110.0).abs() < 0.1,
        "sticky element should stick to container scrollport top (100 + 10 = 110), got {}",
        red.y
    );
}

#[test]
fn sticky_clamped_to_containing_block_bottom() {
    let html = r#"
        <style>
          body { margin: 0; }
          #section {
            height: 300px;
            margin-top: 100px;
            background-color: rgb(0, 0, 255);
          }
          #sticky {
            position: sticky;
            top: 10px;
            width: 50px;
            height: 40px;
            background-color: rgb(255, 0, 0);
          }
          #rest {
            height: 1000px;
          }
        </style>
        <div id="section">
          <div id="sticky"></div>
        </div>
        <div id="rest"></div>
    "#;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();

    let list = build_display_list_full(
        &f.doc.root,
        800.0,
        600.0,
        0.0,
        500.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    let red = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("sticky box should paint a red background");

    assert!(
        (red.y - 360.0).abs() < 0.1,
        "sticky element should be clamped to containing block bottom (400 - 40 = 360), got {}",
        red.y
    );
}

// ── Pseudo-elements ─────────────────────────────────────────────────────────

#[test]
fn before_after_pseudo_elements() {
    let html = r#"<html><head><style>
        .with-before::before { content: ">>"; color: red; }
    </style></head><body>
        <p class="with-before">Text</p>
    </body></html>"#;
    let (_, list) = build(html);
    let text_cmds: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        text_cmds.iter().any(|text| text.contains(">>")),
        "::before should paint generated text, got {text_cmds:?}"
    );
}

// ── Box shadow ──────────────────────────────────────────────────────────────

#[test]
fn box_shadow_produces_command() {
    let (_, list) =
        build(r#"<div style="box-shadow: 5px 5px 10px black; width: 100px; height: 50px">x</div>"#);
    let has_shadow = list
        .commands
        .iter()
        .any(|cmd| matches!(cmd, PaintCmd::BoxShadow { .. }));
    assert!(has_shadow, "box-shadow should produce BoxShadow command");
}

#[test]
fn multiple_box_shadows_produce_separate_commands() {
    let (_, list) = build(
        r#"<div style="box-shadow: 2px 0 0 red, inset 0 3px 0 blue; width: 100px; height: 50px">x</div>"#,
    );
    let shadows: Vec<&PaintCmd> = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::BoxShadow { .. }))
        .collect();
    assert_eq!(
        shadows.len(),
        2,
        "comma-separated box-shadow layers must not collapse into one shadow"
    );
    assert!(
        shadows
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BoxShadow { inset: false, .. })),
        "outer shadow layer is preserved"
    );
    assert!(
        shadows
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BoxShadow { inset: true, .. })),
        "inset shadow layer is preserved"
    );
}

// ── Pixel-level rendering tests ─────────────────────────────────────────────

#[test]
fn render_display_list_produces_colored_pixels() {
    let doc = parse_html(
        r#"
        <div style="background: red; width: 100px; height: 50px; position: absolute; left: 10px; top: 10px">x</div>
        <div style="background: blue; width: 200px; height: 100px; position: absolute; left: 150px; top: 10px">x</div>
    "#,
    );
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();

    let mut pixmap = tiny_skia::Pixmap::new(400, 300).unwrap();
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pixmap, 1.0);

    let data = pixmap.data();
    // Red at (50, 25)
    let idx = (25 * 400 + 50) as usize * 4;
    assert!(
        data[idx] > 200 && data[idx + 1] < 50,
        "pixel (50,25) should be red: ({},{},{})",
        data[idx],
        data[idx + 1],
        data[idx + 2]
    );
    // Blue at (200, 50)
    let idx = (50 * 400 + 200) as usize * 4;
    assert!(
        data[idx + 2] > 200 && data[idx] < 50,
        "pixel (200,50) should be blue: ({},{},{})",
        data[idx],
        data[idx + 1],
        data[idx + 2]
    );
}

#[test]
fn render_display_list_text_produces_dark_pixels() {
    let doc = parse_html(
        r#"<p style="color: black; font-size: 20px; position: absolute; left: 10px; top: 10px">Hello World</p>"#,
    );
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();

    let mut pixmap = tiny_skia::Pixmap::new(400, 300).unwrap();
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pixmap, 1.0);

    let data = pixmap.data();
    let mut has_dark = false;
    // Scan a wider area — absolute positioning + font metrics may shift text
    for y in 0..150 {
        for x in 0..300 {
            let idx = (y * 400 + x) as usize * 4;
            if data[idx] < 100 && data[idx + 1] < 100 && data[idx + 2] < 100 {
                has_dark = true;
                break;
            }
        }
        if has_dark {
            break;
        }
    }
    assert!(has_dark, "text should produce dark pixels");
}

#[test]
fn render_display_list_border_visible() {
    let doc = parse_html(
        r#"<div style="border: 3px solid green; width: 100px; height: 50px; position: absolute; left: 50px; top: 50px">x</div>"#,
    );
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();

    let mut pixmap = tiny_skia::Pixmap::new(400, 300).unwrap();
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pixmap, 1.0);

    let data = pixmap.data();
    // Top border at (100, 50) should be greenish
    let idx = (50 * 400 + 100) as usize * 4;
    assert!(
        data[idx + 1] > 100,
        "top border should have green: g={}",
        data[idx + 1]
    );
}

// ── Command ordering ────────────────────────────────────────────────────────

#[test]
fn parent_background_before_child() {
    let (_, list) = build(
        r#"
        <div style="background: blue; padding: 10px">
            <p style="background: red">text</p>
        </div>
    "#,
    );
    let mut blue_idx = None;
    let mut red_idx = None;
    for (i, cmd) in list.commands.iter().enumerate() {
        if let PaintCmd::FillRect { color, .. } = cmd {
            if color.b == 255 && color.r == 0 && blue_idx.is_none() {
                blue_idx = Some(i);
            }
            if color.r == 255 && color.b == 0 && red_idx.is_none() {
                red_idx = Some(i);
            }
        }
    }
    if let (Some(b), Some(r)) = (blue_idx, red_idx) {
        assert!(b < r, "parent paints before child: blue@{} red@{}", b, r);
    }
}

#[test]
fn command_count_scales_with_elements() {
    let small = build("<div>one</div>").1;
    let big = build("<div><p>a</p><p>b</p><p>c</p><p>d</p><p>e</p></div>").1;
    assert!(
        big.len() > small.len(),
        "more elements = more commands: {} vs {}",
        big.len(),
        small.len()
    );
}

// ── No crash tests ──────────────────────────────────────────────────────────

#[test]
fn image_without_data_no_crash() {
    let (_, list) = build("<div><img src='x.png' width='100' height='50'></div>");
    let _ = list.len();
}

#[test]
fn deeply_nested_no_crash() {
    let html = "<div>".repeat(10) + "deep" + &"</div>".repeat(10);
    let (_, list) = build(&html);
    assert!(!list.is_empty());
}

#[test]
fn empty_text_no_crash() {
    let (_, list) = build("<p></p><p>   </p><p>\n</p>");
    let _ = list.len();
}

// ── object-fit / object-position (css-images-3 §5.5) ────────────────────────

/// Give every `<img>` a natural size, then rebuild the list.
fn build_with_image(html: &str, iw: u32, ih: u32) -> DisplayList {
    let mut doc = parse_html(html);
    fn seed(n: &mut crate::types::WebCore, iw: u32, ih: u32) {
        if n.tag == "img" {
            n.image_width = iw;
            n.image_height = ih;
            n.image_data = Some(std::sync::Arc::new(vec![255u8; (iw * ih * 4) as usize]));
        }
        for c in &mut n.children {
            seed(c, iw, ih);
        }
    }
    seed(&mut doc.root, iw, ih);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();
    build_display_list(&f.doc.root, 800.0, 600.0)
}

fn image_rect(list: &DisplayList) -> Option<crate::types::Rect> {
    list.commands.iter().find_map(|c| match c {
        PaintCmd::Image { rect, .. } => Some(*rect),
        _ => None,
    })
}

#[test]
fn replaced_image_border_radius_clips_painted_bitmap() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{display:block;width:40px;height:40px;border-radius:20px}</style><img src=x>",
        2,
        2,
    );
    let mut pixmap = tiny_skia::Pixmap::new(50, 50).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let pixel = |x: usize, y: usize| {
        let i = (y * 50 + x) * 4;
        Color::rgba(data[i], data[i + 1], data[i + 2], data[i + 3])
    };

    assert!(
        pixel(0, 0).a < 32,
        "the rounded image corner must be clipped, got {:?}",
        pixel(0, 0)
    );
    assert!(
        pixel(20, 20).a > 220,
        "the center of the image must remain painted, got {:?}",
        pixel(20, 20)
    );
}

/// **`object-fit` decides the drawn size of a replaced element** (css-images-3
/// §5.5). It was parsed into `ComputedStyle` and never read by the paint path,
/// so every image was stretched to its content box — the `fill` behaviour —
/// whatever the author asked for.
#[test]
fn object_fit_cover_preserves_the_aspect_ratio() {
    // 200x100 natural, shown in a 100x100 box.
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px;object-fit:cover}</style><img src=x>",
        200, 100);
    let r = image_rect(&list).expect("an image was painted");
    // cover → scale = max(100/200, 100/100) = 1 → 200x100, centred horizontally.
    assert_eq!(
        (r.w, r.h),
        (200.0, 100.0),
        "cover must keep the 2:1 ratio, got {}x{}",
        r.w,
        r.h
    );
    assert_eq!(r.x, -50.0, "centred by the default object-position: 50%");
}

#[test]
fn object_fit_contain_fits_inside_the_box() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px;object-fit:contain}</style><img src=x>",
        200, 100);
    let r = image_rect(&list).expect("an image was painted");
    // contain → scale = min(100/200, 100/100) = 0.5 → 100x50, centred vertically.
    assert_eq!((r.w, r.h), (100.0, 50.0), "got {}x{}", r.w, r.h);
    assert_eq!(r.y, 25.0, "centred vertically");
}

#[test]
fn object_fit_none_uses_the_natural_size() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px;object-fit:none}</style><img src=x>",
        200, 100);
    let r = image_rect(&list).expect("an image was painted");
    assert_eq!((r.w, r.h), (200.0, 100.0), "got {}x{}", r.w, r.h);
}

/// `fill` is the initial value and must still stretch to the box.
#[test]
fn object_fit_fill_is_the_default_and_stretches() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px}</style><img src=x>",
        200,
        100,
    );
    let r = image_rect(&list).expect("an image was painted");
    assert_eq!((r.w, r.h), (100.0, 100.0), "got {}x{}", r.w, r.h);
}

/// `object-position` places the object in the box.
#[test]
fn object_position_places_the_object() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px;object-fit:contain;object-position:left top}</style><img src=x>",
        200, 100);
    let r = image_rect(&list).expect("an image was painted");
    assert_eq!(
        (r.x, r.y),
        (0.0, 0.0),
        "left top pins to the origin, got {},{}",
        r.x,
        r.y
    );
}

#[test]
fn object_position_edge_offsets_place_the_painted_object() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:200px;height:100px;object-fit:contain;object-position:right 10px bottom 20px}</style><img src=x>",
        100,
        100,
    );
    let r = image_rect(&list).expect("an image was painted");
    assert_eq!(
        (r.x, r.y, r.w, r.h),
        (90.0, -20.0, 100.0, 100.0),
        "right/bottom edge offsets place the object inside the free space"
    );
}

// ── Gradient parsing: colour-stop fixup and layers (css-images-3 §4.3.1) ─────

fn gradient_stops(list: &DisplayList) -> Option<Vec<(crate::types::Color, f32)>> {
    list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Gradient { stops, .. } => Some(stops.clone()),
        _ => None,
    })
}

fn gradient_rect(list: &DisplayList) -> Option<crate::types::Rect> {
    list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Gradient { rect, .. } => Some(*rect),
        _ => None,
    })
}

fn radial_gradient_geometry(list: &DisplayList) -> Option<(f32, f32, f32, f32)> {
    list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Gradient {
            radial_center_x,
            radial_center_y,
            radial_radius_x,
            radial_radius_y,
            ..
        } => Some((
            *radial_center_x,
            *radial_center_y,
            *radial_radius_x,
            *radial_radius_y,
        )),
        _ => None,
    })
}

fn fill_rect_of(list: &DisplayList, r: u8, g: u8, b: u8) -> Option<crate::types::Rect> {
    list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::FillRect { rect, color, .. } if color.r == r && color.g == g && color.b == b => {
            Some(*rect)
        }
        _ => None,
    })
}

#[test]
fn gradient_background_repeat_preserves_space_and_round_modes() {
    let (_, list) = build(
        r#"
        <style>
        .box {
            width: 80px;
            height: 40px;
            background-image: linear-gradient(red, red);
            background-repeat: space round;
        }
        </style>
        <div class=box></div>
    "#,
    );
    let modes = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Gradient {
            repeat_x_mode,
            repeat_y_mode,
            ..
        } => Some((*repeat_x_mode, *repeat_y_mode)),
        _ => None,
    });

    assert_eq!(modes, Some((2, 3)));
}

#[test]
fn a_second_background_layer_does_not_corrupt_the_first() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:linear-gradient(red,blue),linear-gradient(lime,black)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(
        stops.len(),
        2,
        "the first layer has two stops, got {stops:?}"
    );
    assert_eq!(
        (stops[0].0.r, stops[0].0.b),
        (255, 0),
        "first stop is red, got {:?}",
        stops[0].0
    );
    assert_eq!(
        (stops[1].0.r, stops[1].0.b),
        (0, 255),
        "second stop is blue, got {:?}",
        stops[1].0
    );
    assert!(
        (stops[1].1 - 1.0).abs() < 0.001,
        "second stop sits at 100%, got {}",
        stops[1].1
    );
}

#[test]
fn a_radial_gradient_keeps_a_positioned_first_stop() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(red 10%, blue 90%)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(stops.len(), 2, "both stops survive, got {stops:?}");
    assert_eq!(
        (stops[0].0.r, stops[0].0.b),
        (255, 0),
        "first stop is red, got {:?}",
        stops[0].0
    );
    assert!(
        (stops[0].1 - 0.10).abs() < 0.001,
        "first stop sits at 10%, got {}",
        stops[0].1
    );
    assert!(
        (stops[1].1 - 0.90).abs() < 0.001,
        "second stop sits at 90%, got {}",
        stops[1].1
    );
}

#[test]
fn a_radial_gradient_descriptor_does_not_eat_a_stop() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(circle at 50% 50%, rgba(255,0,0,1) 0%, rgb(0,0,255) 100%)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(
        stops.len(),
        2,
        "the descriptor is dropped and both stops parse, got {stops:?}"
    );
    assert_eq!(
        (stops[0].0.r, stops[0].0.b),
        (255, 0),
        "first stop is red, got {:?}",
        stops[0].0
    );
    assert_eq!(
        (stops[1].0.r, stops[1].0.b),
        (0, 255),
        "second stop is blue, got {:?}",
        stops[1].0
    );
}

#[test]
fn radial_gradient_descriptor_sets_center_and_ellipse_radii() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(ellipse closest-side at right bottom, red, blue)">x</div>"#,
    );
    let (cx, cy, rx, ry) = radial_gradient_geometry(&list).expect("a radial gradient was painted");
    assert!((cx - 100.0).abs() < 0.001, "right center x, got {cx}");
    assert!((cy - 50.0).abs() < 0.001, "bottom center y, got {cy}");
    assert!(
        rx <= 1.0,
        "closest-side x radius clamps to the nearest side, got {rx}"
    );
    assert!(
        ry <= 1.0,
        "closest-side y radius clamps to the nearest side, got {ry}"
    );
}

#[test]
fn radial_gradient_explicit_ellipse_radii_reach_paint_geometry() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(ellipse 20px 10px at 25px 30px, red, blue)">x</div>"#,
    );
    let (cx, cy, rx, ry) = radial_gradient_geometry(&list).expect("a radial gradient was painted");
    assert!((cx - 25.0).abs() < 0.001, "explicit center x, got {cx}");
    assert!((cy - 30.0).abs() < 0.001, "explicit center y, got {cy}");
    assert!((rx - 20.0).abs() < 0.001, "explicit x radius, got {rx}");
    assert!((ry - 10.0).abs() < 0.001, "explicit y radius, got {ry}");
}

#[test]
fn radial_gradient_explicit_circle_radius_stays_circular() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(circle 25px at center, red, blue)">x</div>"#,
    );
    let (_, _, rx, ry) = radial_gradient_geometry(&list).expect("a radial gradient was painted");
    assert!((rx - 25.0).abs() < 0.001, "circle x radius, got {rx}");
    assert!((ry - 25.0).abs() < 0.001, "circle y radius, got {ry}");
}

#[test]
fn radial_gradient_farthest_corner_and_closest_side_sizing() {
    // Circle closest-side at 20px 30px in 100px x 60px box:
    // left=20, right=80, top=30, bottom=30 => min is 20.
    let (_, list1) = build(
        r#"<div style="width:100px;height:60px;background:radial-gradient(circle closest-side at 20px 30px, red, blue)">x</div>"#,
    );
    let (_, _, rx1, ry1) = radial_gradient_geometry(&list1).expect("list1 gradient");
    assert!(
        (rx1 - 20.0).abs() < 0.01,
        "closest-side circle radius 20, got {rx1}"
    );
    assert!(
        (ry1 - 20.0).abs() < 0.01,
        "closest-side circle radius 20, got {ry1}"
    );

    // Ellipse farthest-corner at 0 0 in 100px x 50px box:
    // dx = 100, dy = 50 => rx = 100 * sqrt(2) ≈ 141.42, ry = 50 * sqrt(2) ≈ 70.71.
    let (_, list2) = build(
        r#"<div style="width:100px;height:50px;background:radial-gradient(ellipse farthest-corner at left top, red, blue)">x</div>"#,
    );
    let (_, _, rx2, ry2) = radial_gradient_geometry(&list2).expect("list2 gradient");
    assert!(
        (rx2 - 100.0 * std::f32::consts::SQRT_2).abs() < 0.1,
        "ellipse rx passes through corner: {rx2}"
    );
    assert!(
        (ry2 - 50.0 * std::f32::consts::SQRT_2).abs() < 0.1,
        "ellipse ry passes through corner: {ry2}"
    );
}

#[test]
fn multiple_background_gradient_layers_are_painted() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background-image:linear-gradient(red, blue), radial-gradient(circle, green, yellow)">x</div>"#,
    );
    let gradient_count = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::Gradient { .. }))
        .count();
    assert_eq!(
        gradient_count, 2,
        "both gradient layers should be painted, got {gradient_count}"
    );
}

#[test]
fn a_stop_before_its_predecessor_is_clamped_up_to_it() {
    // css-images-3 §4.3.1 fixup step 3.
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:linear-gradient(red 60%, blue 20%, lime 90%)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(stops.len(), 3, "three stops, got {stops:?}");
    assert!(
        (stops[1].1 - 0.60).abs() < 0.001,
        "20% clamps up to the preceding 60%, got {}",
        stops[1].1
    );
    assert!(
        (stops[2].1 - 0.90).abs() < 0.001,
        "90% is untouched, got {}",
        stops[2].1
    );
}

#[test]
fn unpositioned_stops_space_between_positioned_neighbours() {
    // css-images-3 §4.3.1 fixup step 4 — the spec's own example 2.
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:linear-gradient(red 40%, white, black, blue)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(stops.len(), 4, "four stops, got {stops:?}");
    for (i, want) in [0.40f32, 0.60, 0.80, 1.0].iter().enumerate() {
        assert!(
            (stops[i].1 - want).abs() < 0.001,
            "stop {i} sits at {want}, got {} (all: {stops:?})",
            stops[i].1
        );
    }
}

#[test]
fn a_double_position_stop_expands_to_two_stops() {
    // css-images-3 §4.3.1: <linear-color-stop> = <color> <length-percentage>{1,2}
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background:linear-gradient(red 10% 20%, blue)">x</div>"#,
    );
    let stops = gradient_stops(&list).expect("a gradient was painted");
    assert_eq!(
        stops.len(),
        3,
        "the shorthand expands to two red stops plus blue, got {stops:?}"
    );
    assert!(
        (stops[0].1 - 0.10).abs() < 0.001,
        "first red at 10%, got {}",
        stops[0].1
    );
    assert!(
        (stops[1].1 - 0.20).abs() < 0.001,
        "second red at 20%, got {}",
        stops[1].1
    );
    assert_eq!(
        (stops[1].0.r, stops[1].0.b),
        (255, 0),
        "the second stop repeats the colour, got {:?}",
        stops[1].0
    );
}

// ── background-clip / background-origin (css-backgrounds-3 §3.7, §3.6) ──────

#[test]
fn the_background_colour_fills_the_border_box_by_default() {
    // background-clip's initial value is border-box, so the colour bleeds under
    // a transparent border instead of stopping at the padding edge.
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style><div style="width:100px;height:50px;border:10px solid transparent;background-color:red">x</div>"#,
    );
    let r = fill_rect_of(&list, 255, 0, 0).expect("a red background was painted");
    assert_eq!(
        (r.w, r.h),
        (120.0, 70.0),
        "the painting area is the border box, got {}x{}",
        r.w,
        r.h
    );
}

#[test]
fn background_clip_padding_box_stops_at_the_padding_edge() {
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style><div style="width:100px;height:50px;border:10px solid transparent;background-color:red;background-clip:padding-box">x</div>"#,
    );
    let r = fill_rect_of(&list, 255, 0, 0).expect("a red background was painted");
    assert_eq!(
        (r.w, r.h),
        (100.0, 50.0),
        "the painting area is the padding box, got {}x{}",
        r.w,
        r.h
    );
}

#[test]
fn background_clip_content_box_stops_at_the_content_edge() {
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style><div style="width:100px;height:50px;padding:10px;background-color:red;background-clip:content-box">x</div>"#,
    );
    let r = fill_rect_of(&list, 255, 0, 0).expect("a red background was painted");
    assert_eq!(
        (r.w, r.h),
        (100.0, 50.0),
        "the painting area is the content box, got {}x{}",
        r.w,
        r.h
    );
}

#[test]
fn a_gradient_is_positioned_in_the_padding_box_by_default() {
    // background-origin's initial value is padding-box — the border box is the
    // CLIP, not the positioning area.
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style><div style="width:100px;height:50px;border:10px solid transparent;background:linear-gradient(red,blue)">x</div>"#,
    );
    let r = gradient_rect(&list).expect("a gradient was painted");
    assert_eq!(
        (r.w, r.h),
        (100.0, 50.0),
        "the positioning area is the padding box, got {}x{}",
        r.w,
        r.h
    );
}

#[test]
fn background_origin_border_box_sizes_the_gradient_to_the_border_box() {
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style><div style="width:100px;height:50px;border:10px solid transparent;background:linear-gradient(red,blue);background-origin:border-box">x</div>"#,
    );
    let r = gradient_rect(&list).expect("a gradient was painted");
    assert_eq!(
        (r.w, r.h),
        (120.0, 70.0),
        "the positioning area is the border box, got {}x{}",
        r.w,
        r.h
    );
}

#[test]
fn a_gradient_repeats_under_a_transparent_border() {
    // The gradient image is the size of the POSITIONING area (padding box) and
    // tiles across the PAINTING area (border box), so the strip under a
    // transparent border shows the next repetition — not a gap.
    // Chrome renders (60,10) as rgb(24,0,230) and (60,130) as rgb(228,0,26)
    // for this fixture.
    let doc = parse_html(
        r#"<style>body{margin:0}</style>
        <div style="position:absolute;left:0;top:0;width:100px;height:100px;
                    border:20px solid transparent;
                    background:linear-gradient(to bottom,#ff0000,#0000ff)"></div>"#,
    );
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();
    let mut pixmap = tiny_skia::Pixmap::new(400, 300).unwrap();
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pixmap, 1.0);
    let data = pixmap.data();
    let px = |x: usize, y: usize| {
        let i = (y * 400 + x) * 4;
        (data[i], data[i + 1], data[i + 2])
    };
    let top = px(60, 10);
    assert!(
        top.2 > 180 && top.0 < 80,
        "the top border strip repeats the END of the gradient, got {top:?}"
    );
    let bottom = px(60, 130);
    assert!(
        bottom.0 > 180 && bottom.2 < 80,
        "the bottom border strip repeats the START of the gradient, got {bottom:?}"
    );
    let inside = px(60, 25);
    assert!(
        inside.0 > 180 && inside.2 < 80,
        "the top of the padding box is still the first stop, got {inside:?}"
    );
}

#[test]
fn background_repeat_space_distributes_tiles_with_gaps() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BackgroundImage {
        container: Rect::new(0.0, 0.0, 25.0, 10.0),
        clip: Rect::new(0.0, 0.0, 25.0, 10.0),
        data: ImageRef::Owned(vec![255, 0, 0, 255], 1, 1),
        size_mode: 3,
        draw_w: 10.0,
        draw_h: 10.0,
        pos_x: 0.0,
        pos_y: 0.0,
        repeat_x_mode: 2,
        repeat_y_mode: 0,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
        blend_mode: 0,
    });

    let mut pixmap = tiny_skia::Pixmap::new(25, 10).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let pixel = |x: usize, y: usize| {
        let i = (y * 25 + x) * 4;
        Color::rgba(data[i], data[i + 1], data[i + 2], data[i + 3])
    };

    assert_eq!(pixel(2, 5), Color::rgb(255, 0, 0));
    assert_eq!(
        pixel(12, 5).a,
        0,
        "space repeat should leave distributed space between whole tiles"
    );
    assert_eq!(pixel(17, 5), Color::rgb(255, 0, 0));
}

#[test]
fn background_image_blend_mode_multiplies_with_existing_backdrop() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::FillRect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        color: Color::rgba(255, 0, 0, 255),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    });
    list.push(PaintCmd::BackgroundImage {
        container: Rect::new(0.0, 0.0, 10.0, 10.0),
        clip: Rect::new(0.0, 0.0, 10.0, 10.0),
        data: ImageRef::Owned(vec![0, 0, 255, 255], 1, 1),
        size_mode: 3,
        draw_w: 10.0,
        draw_h: 10.0,
        pos_x: 0.0,
        pos_y: 0.0,
        repeat_x_mode: 0,
        repeat_y_mode: 0,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
        blend_mode: 1,
    });

    let mut pixmap = tiny_skia::Pixmap::new(10, 10).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let i = (5 * 10 + 5) * 4;
    let (r, g, b) = (data[i], data[i + 1], data[i + 2]);
    assert!(
        r < 40 && g < 40 && b < 40,
        "multiply-blended blue image over red should paint black, got ({r},{g},{b})"
    );
}

#[test]
fn appearance_none_checkbox_does_not_paint_native_chrome() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::FormElement {
        tag: "input".to_string(),
        input_type: "checkbox".to_string(),
        rect: Rect::new(0.0, 0.0, 20.0, 20.0),
        node_id: 1,
        attributes: Vec::new(),
        font_size: 16.0,
        font_weight: 400,
        font_family: "Arial".to_string(),
        color: Color::BLACK,
        placeholder_color: Color::rgba(0, 0, 0, 128),
        checked: true,
        value: String::new(),
        placeholder: String::new(),
        input_cursor: 0,
        appearance_none: true,
        vertical: false,
        options: Vec::new(),
        selected: -1,
        selected_all: Vec::new(),
    });

    let mut pixmap = tiny_skia::Pixmap::new(24, 24).unwrap();
    replay(&list, &mut pixmap, 1.0);
    assert!(
        pixmap.data().chunks_exact(4).all(|px| px[3] == 0),
        "appearance:none should leave native checkbox chrome unpainted"
    );
}

#[test]
fn overflow_clip_margin_expands_the_paint_clip_rect() {
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}</style>
           <div style="width:100px;height:50px;overflow:clip;overflow-clip-margin:8px">
             clipped
           </div>"#,
    );
    let clip = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushClip { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("overflow clip");

    assert_eq!(clip.x, -8.0);
    assert_eq!(clip.y, -8.0);
    assert_eq!(clip.w, 116.0);
    assert_eq!(clip.h, 66.0);
}

// ── border-radius per corner (css-borders-4) ────────────────────────────────

fn first_radii(list: &DisplayList) -> Option<[f32; 4]> {
    list.commands.iter().find_map(|c| match c {
        PaintCmd::FillRect { radius, .. } => Some(*radius),
        _ => None,
    })
}

fn first_radii_y(list: &DisplayList) -> Option<[f32; 4]> {
    list.commands.iter().find_map(|c| match c {
        PaintCmd::FillRect { radius_y, .. } => Some(*radius_y),
        _ => None,
    })
}

/// **Each corner keeps its own radius.** `border_radius` is only a mirror of
/// the top-left longhand, not a "the shorthand was used" flag, but the painter
/// treated any non-zero top-left as "apply this to all four corners". The
/// top-rounded card — `border-radius: 16px 16px 0 0` — came out rounded on all
/// four, and the same corrupted array feeds the border stroke and the
/// overflow clip.
#[test]
fn border_radius_keeps_each_corner_distinct() {
    let (_f, list) = build(
        "<style>* { margin:0; padding:0 }\
         div { width:200px; height:100px; background:red;\
               border-radius: 16px 16px 0 0 }</style><div></div>",
    );
    let r = first_radii(&list).expect("a background rect was painted");
    assert_eq!(r, [16.0, 16.0, 0.0, 0.0], "got {r:?}");
}

/// A single-value shorthand still rounds every corner.
#[test]
fn border_radius_shorthand_still_rounds_all_corners() {
    let (_f, list) = build(
        "<style>* { margin:0; padding:0 }\
         div { width:200px; height:100px; background:red; border-radius: 12px }</style><div></div>",
    );
    let r = first_radii(&list).expect("a background rect was painted");
    assert_eq!(r, [12.0, 12.0, 12.0, 12.0], "got {r:?}");
}

#[test]
fn border_radius_slash_keeps_elliptical_corner_radii() {
    let (_f, list) = build(
        "<style>* { margin:0; padding:0 }\
         div { width:200px; height:100px; background:red;\
               border-radius: 100px / 40px }</style><div></div>",
    );
    let rx = first_radii(&list).expect("a background rect was painted");
    let ry = first_radii_y(&list).expect("a background rect was painted");
    assert_eq!(rx, [100.0, 100.0, 100.0, 100.0], "got {rx:?}");
    assert_eq!(ry, [40.0, 40.0, 40.0, 40.0], "got {ry:?}");
}

/// identity `[1, 0, 0, 1, 0, 0]` — the percentage translation is exactly zero.
#[test]
fn translate_percentages_resolve_against_the_reference_box() {
    // transform-origin 0 0 so the origin round-trip contributes exactly zero
    // and this test isolates the argument resolution.
    let m = tx_matrix(
        &[
            ("transform-origin", "0 0"),
            ("transform", "translate(-50%, -50%)"),
        ],
        200.0,
        100.0,
    );
    assert!(
        close6(m, [1.0, 0.0, 0.0, 1.0, -100.0, -50.0], 0.01),
        "translate(-50%,-50%) on a 200x100 box is translate(-100px,-50px); got {m:?}"
    );

    let m = tx_matrix(
        &[
            ("font-size", "40px"),
            ("transform-origin", "0 0"),
            ("transform", "translateX(2em)"),
        ],
        200.0,
        100.0,
    );
    assert!(
        (m[4] - 80.0).abs() < 0.01,
        "translateX(2em) at font-size:40px is 80px, not a hardcoded 16px basis; got e={}",
        m[4]
    );
}

/// — the box moves right instead of down.
#[test]
fn transform_functions_are_post_multiplied() {
    let m = tx_matrix(
        &[
            ("transform-origin", "0 0"),
            ("transform", "rotate(90deg) translateX(100px)"),
        ],
        100.0,
        100.0,
    );
    assert!(
        close6(m, [0.0, 1.0, -1.0, 0.0, 0.0, 100.0], 0.001),
        "rotate(90deg) translateX(100px) is R*T = [0,1,-1,0,0,100] — the translation \
         must be rotated into the new coordinate system; got {m:?}"
    );

    let m = tx_matrix(
        &[
            ("transform-origin", "0 0"),
            ("transform", "rotate(90deg) scale(2, 1)"),
        ],
        100.0,
        100.0,
    );
    assert!(
        close6(m, [0.0, 2.0, -1.0, 0.0, 0.0, 0.0], 0.001),
        "rotate(90deg) scale(2,1) is R*S = [0,2,-1,0,0,0] — scale must multiply all \
         four of a,b,c,d, not only a and d; got {m:?}"
    );
}

/// is 200x too far out, exactly `rect.w * 10.0`.
#[test]
fn transform_origin_lengths_are_offsets_not_fractions() {
    // `10px 10px` on a 200x200 box: origin (10,10) -> e = f = 20.
    // The bug reads 10.0 as a fraction: ox = 200*10 = 2000 -> e = 4000.
    let m = tx_matrix(
        &[
            ("transform-origin", "10px 10px"),
            ("transform", "rotate(180deg)"),
        ],
        200.0,
        200.0,
    );
    assert!(
        close6(m, [-1.0, 0.0, 0.0, -1.0, 20.0, 20.0], 0.01),
        "transform-origin:10px 10px is the point (10,10), so rotate(180deg) is \
         [-1,0,0,-1,20,20]; got {m:?}"
    );

    // A single `top` keyword: vertical 0%, horizontal stays center (50%).
    // Origin (100, 0) on a 200x200 box -> e = 200, f = 0.
    let m = tx_matrix(
        &[("transform-origin", "top"), ("transform", "rotate(180deg)")],
        200.0,
        200.0,
    );
    assert!(
        close6(m, [-1.0, 0.0, 0.0, -1.0, 200.0, 0.0], 0.01),
        "transform-origin:top is `center top` — origin (100,0) on a 200x200 box; got {m:?}"
    );

    // A non-px length unit is still a length: 1rem = 16px at the default root
    // font size, so origin (16,16) -> e = f = 32. The bug's `_ =>` arm returns
    // the 0.5 fraction, putting the origin at the centre.
    let m = tx_matrix(
        &[
            ("transform-origin", "1rem 1rem"),
            ("transform", "rotate(180deg)"),
        ],
        200.0,
        200.0,
    );
    assert!(
        close6(m, [-1.0, 0.0, 0.0, -1.0, 32.0, 32.0], 0.01),
        "transform-origin:1rem 1rem is the point (16,16); got {m:?}"
    );
}

#[test]
fn transform_box_selects_border_or_content_reference_box() {
    fn first_transform(list: &DisplayList) -> [f32; 6] {
        list.commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::PushTransform { transform } => Some(*transform),
                _ => None,
            })
            .expect("transform command")
    }

    let (_, border_list) = build(
        r#"<body style="margin:0">
            <div style="width:100px;height:100px;border-left:20px solid black;
                        transform-origin:50% 50%;transform-box:border-box;
                        transform:rotate(180deg)"></div>
        </body>"#,
    );
    let (_, content_list) = build(
        r#"<body style="margin:0">
            <div style="width:100px;height:100px;border-left:20px solid black;
                        transform-origin:50% 50%;transform-box:content-box;
                        transform:rotate(180deg)"></div>
        </body>"#,
    );

    let border = first_transform(&border_list);
    let content = first_transform(&content_list);
    assert!(
        close6(border, [-1.0, 0.0, 0.0, -1.0, 120.0, 100.0], 0.5),
        "border-box origin should use the 120px border box, got {border:?}"
    );
    assert!(
        close6(content, [-1.0, 0.0, 0.0, -1.0, 140.0, 100.0], 0.5),
        "content-box origin should use the content rect shifted by the border, got {content:?}"
    );
}

#[test]
fn text_overflow_ellipsis_truncates_display_text() {
    let mut frame = crate::EngineFrame::new(
        crate::parse_html(
            r#"
        <style>
        * { margin: 0; padding: 0; }
        div {
            width: 48px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            font-size: 20px;
        }
        </style>
        <div>abcdef ghi</div>
    "#,
        ),
        240.0,
        80.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 240.0, 80.0);

    let text = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { text, .. } if text.contains('…') => Some(text.as_str()),
        _ => None,
    });

    assert!(
        text.is_some(),
        "text-overflow: ellipsis should emit truncated text with an ellipsis; commands were {:?}",
        list.commands
    );
}

#[test]
fn text_overflow_two_value_custom_marker_truncates_display_text() {
    let mut frame = crate::EngineFrame::new(
        crate::parse_html(
            r#"
        <style>
        * { margin: 0; padding: 0; }
        div {
            width: 48px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: clip "--";
            font-size: 20px;
        }
        </style>
        <div>abcdef ghi</div>
    "#,
        ),
        240.0,
        80.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 240.0, 80.0);

    let text = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { text, .. } if text.contains("--") => Some(text.as_str()),
        _ => None,
    });

    assert!(
        text.is_some(),
        "text-overflow two-value syntax should emit truncated text with the end marker; commands were {:?}",
        list.commands
    );
}

#[test]
fn marker_content_overrides_list_style_marker_text() {
    let mut frame = crate::EngineFrame::new(
        crate::parse_html(
            r#"
        <style>
        li { list-style-type: decimal; }
        li::marker { content: ">> "; color: red; }
        </style>
        <ol><li>item</li></ol>
    "#,
        ),
        240.0,
        80.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 240.0, 80.0);

    let marker = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::ListMarker { text, color, .. } if text == ">> " => Some(*color),
        _ => None,
    });

    assert_eq!(
        marker,
        Some(crate::types::Color::rgb(255, 0, 0)),
        "::marker content should replace generated list-style marker text"
    );
}

#[test]
fn custom_counter_style_paints_list_marker() {
    let (frame, list) = build(
        r#"
        <style>
        @counter-style thumbs {
            system: cyclic;
            symbols: "\1F44D";
            suffix: " ";
        }
        li { list-style-type: thumbs; }
        </style>
        <ol><li>item</li></ol>
    "#,
    );
    assert_eq!(frame.doc.stylesheet.counter_styles.len(), 1);
    let li =
        crate::tests::harness::find_box(&frame.doc.root, &|node| node.tag == "li").expect("li box");
    assert_eq!(li.style.custom_list_style_type, "thumbs");
    assert_eq!(li.style.marker_content, "👍 ");

    let marker = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::ListMarker { text, .. } if text == "👍 " => Some(text.as_str()),
        _ => None,
    });

    assert_eq!(
        marker,
        Some("👍 "),
        "@counter-style should resolve custom list-style markers before paint"
    );
}

#[test]
fn custom_counter_style_applies_range_fixed_start_pad_and_fallback() {
    let (frame, _) = build(
        r#"
        <style>
        @counter-style limited {
            system: fixed 0;
            symbols: "Z" "O";
            range: 0 1;
            fallback: lower-alpha;
            pad: 2 "0";
            suffix: ") ";
        }
        ol { counter-reset: list-item -1; }
        li { list-style-type: limited; }
        </style>
        <ol><li id=zero>zero</li><li id=one>one</li><li id=two>two</li></ol>
    "#,
    );
    let marker = |id: &str| {
        crate::tests::harness::find_box(&frame.doc.root, &|node| {
            node.tag == "li" && node.attributes.get("id").is_some_and(|value| value == id)
        })
        .map(|node| node.style.marker_content.clone())
    };

    assert_eq!(marker("zero").as_deref(), Some("0Z) "));
    assert_eq!(marker("one").as_deref(), Some("0O) "));
    assert_eq!(
        marker("two").as_deref(),
        Some("0b) "),
        "out-of-range values should use the declared fallback style and then apply pad"
    );
}

#[test]
fn custom_counter_style_extends_inherits_base_descriptors() {
    let (frame, _) = build(
        r#"
        <style>
        @counter-style base-dots {
            system: cyclic;
            symbols: "A" "B";
            suffix: ". ";
        }
        @counter-style loud-dots {
            system: extends base-dots;
            suffix: "! ";
        }
        li { list-style-type: loud-dots; }
        </style>
        <ol><li id=first>first</li><li id=second>second</li></ol>
    "#,
    );
    let marker = |id: &str| {
        crate::tests::harness::find_box(&frame.doc.root, &|node| {
            node.tag == "li" && node.attributes.get("id").is_some_and(|value| value == id)
        })
        .map(|node| node.style.marker_content.clone())
    };

    assert_eq!(marker("first").as_deref(), Some("A! "));
    assert_eq!(marker("second").as_deref(), Some("B! "));
}

#[test]
fn custom_counter_style_additive_symbols_paint_marker() {
    let (frame, _) = build(
        r#"
        <style>
        @counter-style tally {
            system: additive;
            additive-symbols: 5 "V", 1 "I";
            suffix: " ";
        }
        li { list-style-type: tally; }
        </style>
        <ol><li id=one>one</li><li id=two>two</li><li id=three>three</li><li id=four>four</li><li id=five>five</li><li id=six>six</li></ol>
    "#,
    );
    let marker = |id: &str| {
        crate::tests::harness::find_box(&frame.doc.root, &|node| {
            node.tag == "li" && node.attributes.get("id").is_some_and(|value| value == id)
        })
        .map(|node| node.style.marker_content.clone())
    };

    assert_eq!(marker("four").as_deref(), Some("IIII "));
    assert_eq!(marker("six").as_deref(), Some("VI "));
}

#[test]
fn marker_uses_its_own_font_fields() {
    let (_, list) = build(
        r#"<style>
            li { font-weight: 400; font-style: normal; font-size: 16px; }
            li::marker { content: "x"; font-weight: 700; font-style: italic; font-size: 24px; }
        </style><ol><li>item</li></ol>"#,
    );
    let marker = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::ListMarker {
                text,
                font_size,
                font_weight,
                font_style,
                ..
            } if text == "x" => Some((*font_size, *font_weight, *font_style)),
            _ => None,
        })
        .expect("marker command");

    assert_eq!(marker.0, 24.0);
    assert_eq!(marker.1, 700);
    assert_eq!(marker.2, 1);
}

#[test]
fn transform_translate_post_multiplies_existing_rotation() {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "transform", "rotate(90deg) translateX(80px)");
    let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
        &style,
        100.0,
        100.0,
        &crate::types::TransformCtx {
            font_px: 16.0,
            root_font_px: 16.0,
            viewport_w: 800.0,
            viewport_h: 600.0,
        },
    );

    assert!(
        matrix[4].abs() < 0.01,
        "translated x should be rotated into y, got matrix {matrix:?}"
    );
    assert!(
        (matrix[5] - 80.0).abs() < 0.01,
        "translateX after rotate(90deg) should move down by 80px, got matrix {matrix:?}"
    );
}

#[test]
fn transform_parser_maps_representable_3d_functions_to_2d_matrix() {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(
        &mut style,
        "transform",
        "translate3d(10px, 20px, 30px) rotateZ(90deg) scale3d(2, 3, 4)",
    );
    assert_eq!(
        style.css_transform.ops.len(),
        3,
        "representable 3D transform functions should survive parsing"
    );

    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(
        &mut style,
        "transform",
        "matrix3d(1, 2, 0, 0, 3, 4, 0, 0, 0, 0, 1, 0, 5, 6, 0, 1)",
    );
    let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
        &style,
        100.0,
        100.0,
        &crate::types::TransformCtx {
            font_px: 16.0,
            root_font_px: 16.0,
            viewport_w: 800.0,
            viewport_h: 600.0,
        },
    );
    assert_eq!(matrix, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
}

#[test]
fn individual_transform_properties_compose_before_transform() {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "transform", "translateX(5px)");
    crate::css::apply_property(&mut style, "scale", "2 3");
    crate::css::apply_property(&mut style, "rotate", "90deg");
    crate::css::apply_property(&mut style, "translate", "10px 20px");

    let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
        &style,
        100.0,
        100.0,
        &crate::types::TransformCtx {
            font_px: 16.0,
            root_font_px: 16.0,
            viewport_w: 800.0,
            viewport_h: 600.0,
        },
    );

    assert!(
        matrix[0].abs() < 0.01 && (matrix[1] - 2.0).abs() < 0.01,
        "rotate then scale should compose through the matrix, got {matrix:?}"
    );
    assert!(
        (matrix[2] + 3.0).abs() < 0.01 && matrix[3].abs() < 0.01,
        "scale should follow individual rotate in CSS transform order, got {matrix:?}"
    );
    assert!(
        (matrix[4] - 10.0).abs() < 0.01 && (matrix[5] - 30.0).abs() < 0.01,
        "individual transforms must compose before the transform shorthand, got {matrix:?}"
    );
}

#[test]
fn list_style_image_emits_image_marker_command() {
    let (_, list) =
        build(r#"<ul><li style="list-style-image:url(marker.svg); width:100px">item</li></ul>"#);
    let marker = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::ListMarker {
            marker_type, text, ..
        } if *marker_type == 4 => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(
        marker,
        Some("marker.svg"),
        "list-style-image should survive marker generation as an image marker"
    );
}

#[test]
fn list_style_shorthand_url_emits_image_marker_command() {
    let (_, list) =
        build(r#"<ul><li style="list-style:url(icon.png); width:100px">item</li></ul>"#);
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::ListMarker {
                marker_type: 4,
                text,
                ..
            } if text == "icon.png"
        )),
        "list-style shorthand URL should feed list-style-image"
    );
}

#[test]
fn list_style_image_marker_decodes_and_paints_resolved_image() {
    let base = format!("{}/examples/", env!("CARGO_MANIFEST_DIR"));
    let doc = parse_html_with_base(
        r#"<style>*{margin:0;padding:0} li{list-style-position:inside;list-style-image:url(silicon.png);font-size:16px;line-height:20px}</style><ul><li>item</li></ul>"#,
        &base,
    );
    let mut frame = EngineFrame::new(doc, 80.0, 40.0);
    frame.update_frame();
    let list = build_display_list_full(
        &frame.doc.root,
        80.0,
        40.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        &base,
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::ListMarker {
                marker_type: 4,
                image: Some(ImageRef::Owned(_, _, _) | ImageRef::Shared(_, _, _)),
                ..
            }
        )),
        "list-style-image should decode into marker image pixels"
    );

    let paint_list = DisplayList {
        commands: vec![PaintCmd::ListMarker {
            marker_type: 4,
            x: 4.0,
            y: 4.0,
            size: 12.0,
            color: Color::rgba(0, 0, 0, 255),
            text: String::new(),
            image: Some(ImageRef::Owned(vec![255, 0, 0, 255], 1, 1)),
            font_family: String::new(),
            font_size: 16.0,
            font_weight: 400,
            font_style: 0,
            line_height: 20.0,
        }],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(24, 24).unwrap();
    replay(&paint_list, &mut pixmap, 1.0);
    let painted_red = pixmap
        .data()
        .chunks_exact(4)
        .any(|px| px[0] > 200 && px[1] < 80 && px[2] < 80 && px[3] > 200);
    assert!(painted_red, "image marker replay should paint its bitmap");
}

#[test]
fn circle_list_marker_paints_a_hollow_circle() {
    let list = DisplayList {
        commands: vec![PaintCmd::ListMarker {
            marker_type: 1,
            x: 12.0,
            y: 12.0,
            size: 6.0,
            color: Color::rgba(0, 0, 0, 255),
            text: String::new(),
            image: None,
            font_family: String::new(),
            font_size: 16.0,
            font_weight: 400,
            font_style: 0,
            line_height: 20.0,
        }],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(30, 30).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let alpha_at = |x: usize, y: usize| data[(y * 30 + x) * 4 + 3];

    assert!(
        alpha_at(12, 6) > 0 || alpha_at(12, 18) > 0,
        "circle marker should paint the stroked rim"
    );
    assert_eq!(
        alpha_at(12, 12),
        0,
        "list-style-type: circle should not fill the marker center"
    );
}

#[test]
fn mask_layer_applies_to_nested_paint_commands() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushMask {
                rect: Rect::new(0.0, 0.0, 20.0, 10.0),
                data: ImageRef::Owned(
                    vec![
                        255, 255, 255, 255, // left half visible
                        0, 0, 0, 255, // right half transparent by luminance
                    ],
                    2,
                    1,
                ),
            },
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 20.0, 10.0),
                color: Color::rgba(255, 0, 0, 255),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::FillRect {
                rect: Rect::new(12.0, 0.0, 8.0, 10.0),
                color: Color::rgba(0, 0, 255, 255),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PopMask,
        ],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(24, 12).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let alpha_at = |x: usize, y: usize| data[(y * 24 + x) * 4 + 3];

    assert!(alpha_at(5, 5) > 200, "opaque mask half should show content");
    assert_eq!(
        alpha_at(15, 5),
        0,
        "transparent mask half should hide later nested paint commands"
    );
}

#[test]
fn border_image_paints_only_the_border_ring() {
    let list = DisplayList {
        commands: vec![PaintCmd::BorderImage {
            rect: Rect::new(2.0, 2.0, 20.0, 20.0),
            widths: [4.0, 4.0, 4.0, 4.0],
            slices: [1.0, 1.0, 1.0, 1.0],
            fill_center: false,
            data: ImageRef::Owned([0, 220, 0, 255].repeat(9), 3, 3),
        }],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(24, 24).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let alpha_at = |x: usize, y: usize| data[(y * 24 + x) * 4 + 3];
    let green_at = |x: usize, y: usize| data[(y * 24 + x) * 4 + 1];

    assert!(alpha_at(4, 4) > 200 && green_at(4, 4) > 180);
    assert_eq!(
        alpha_at(12, 12),
        0,
        "border-image paint must not fill the content box"
    );
}

#[test]
fn resize_overflow_box_emits_resize_grip() {
    let (_, list) = build(
        r#"<div id="a" style="width:80px;height:60px;overflow:auto;resize:both"></div>
           <div id="b" style="width:80px;height:60px;overflow:visible;resize:both"></div>"#,
    );

    let grips = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::ResizeGrip { .. }))
        .count();
    assert_eq!(
        grips, 1,
        "only a resizable non-visible-overflow box should emit a resize affordance"
    );
}

/// Build a `ComputedStyle` from declarations and ask for the matrix the
/// element's `transform` resolves to, against an explicit reference box.
/// Mirrors what `build_for_box` does at its `PushTransform` site.
fn tx_matrix(decls: &[(&str, &str)], w: f32, h: f32) -> [f32; 6] {
    let mut s = crate::types::ComputedStyle::default();
    for (p, v) in decls {
        crate::css::apply_property(&mut s, p, v);
    }
    let ctx = crate::types::TransformCtx {
        font_px: s.font_size_px(16.0, 16.0),
        root_font_px: 16.0,
        viewport_w: 1000.0,
        viewport_h: 800.0,
    };
    crate::renderer::display_list_builder::compute_transform_matrix(
        &s,
        &crate::types::Rect::new(0.0, 0.0, w, h),
        &ctx,
    )
}

fn close6(got: [f32; 6], want: [f32; 6], eps: f32) -> bool {
    got.iter()
        .zip(want.iter())
        .all(|(a, b)| (a - b).abs() <= eps)
}
