//! Tests for the display list builder and replay.

use crate::Renderer;
use crate::frame::EngineFrame;
use crate::html::{parse_html, parse_html_with_base};
use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd};
use crate::renderer::display_list_builder::{
    build_display_list, build_display_list_full, build_display_list_full_with_font_system,
};
use crate::renderer::display_list_replay::{
    reduce_corner_radii, replay, replay_with_scroll, replay_with_scroll_and_transform_overrides,
};
use crate::types::{Color, Rect};

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

fn first_image_data(list: &DisplayList) -> Option<(&[u8], u32, u32)> {
    list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data.as_slice(), *w, *h)),
        PaintCmd::Image {
            data: ImageRef::Shared(data, w, h),
            ..
        } => Some((data.as_ref(), *w, *h)),
        _ => None,
    })
}

#[test]
fn block_pseudo_child_and_following_text_are_painted() {
    let (frame, list) = build_full(
        r#"<body style="margin:0">
             <style>
             .card { display:flex; flex-direction:column; width:220px; font:16px/20px sans-serif; color:#202020; }
             .card::before { content:"LIVE"; display:block; width:39px; height:20px; background:#d0021b; color:white; }
             .card::after { content:attr(data-deck); display:block; font:12px/14px serif; }
             </style>
             <a class="card" data-deck="Deck text">Headline after generated label</a>
           </body>"#,
    );
    let card = find_node_by_tag(&frame.doc.root, "a").expect("card");
    let after = card
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("materialized ::after");
    assert_eq!(after.text, "Deck text");
    assert!(
        (after.style.font_size_px(16.0, 16.0) - 12.0).abs() < 0.5,
        "pseudo style should keep ::after font-size"
    );

    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::FillRect { color, .. } if color.r == 208 && color.g == 2 && color.b == 27)),
        "block-level generated pseudo-elements should paint their own box"
    );
    let headline_lines: Vec<&str> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. }
                if text.contains("Headline") || text.contains("generated") =>
            {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(
        !headline_lines.is_empty(),
        "standalone inline #text children should paint from their wrapped line cache"
    );
    assert!(
        !headline_lines
            .iter()
            .any(|text| text.contains("Headline after generated label")),
        "standalone text fallback must not paint the whole wrapped text node as one unwrapped run"
    );
    let text_dump: Vec<String> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text {
                text, font_size, ..
            } => Some(format!("{text:?}@{font_size}")),
            _ => None,
        })
        .collect();
    assert!(
        list.commands.iter().any(
            |cmd| matches!(cmd, PaintCmd::Text { text, font_size, .. } if text.contains("Deck text") && (*font_size - 12.0).abs() < 0.5)
        ),
        "block-level generated ::after text should keep its pseudo-element font; text={text_dump:?}"
    );
}

#[test]
fn absolutely_positioned_block_pseudo_text_is_painted() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <style>
             .switch { position:relative; display:block; width:45px; height:25px; }
             .switch::before {
               content:"\f186";
               position:absolute;
               left:3px;
               bottom:3px;
               width:19px;
               height:19px;
               line-height:20px;
               text-align:center;
               font-family:FontAwesome;
               background:white;
               color:#666;
             }
             </style>
             <span class="switch"></span>
           </body>"#,
    );

    let moon = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text {
                x,
                y,
                text,
                font_family,
                font_size,
                line_height,
                ..
            } if text == "\u{f186}" && font_family == "FontAwesome" => {
                Some((*x, *y, *font_size, *line_height))
            }
            _ => None,
        })
        .expect("positioned block ::before generated content should paint as text");

    let text_w = moon.2 * 0.55;
    let text_center_x = moon.0 + text_w * 0.5;
    let text_center_y = moon.1 + moon.3 * 0.5;
    assert!(
        (text_center_x - 12.5).abs() < 1.0,
        "moon icon should be horizontally centered in the 19px knob; command={moon:?}"
    );
    assert!(
        (text_center_y - 12.5).abs() < 1.0,
        "moon icon should be vertically centered in the 19px knob; command={moon:?}"
    );
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
fn flex_anchor_direct_text_child_emits_text_command() {
    let (_frame, list) = build_full(
        r#"<html dir="rtl"><body style="margin:0">
             <ul style="margin:0; padding:0; list-style:none">
               <li style="display:inline-block">
                 <a style="display:flex; flex-direction:column; justify-content:center; height:44px; padding:0 8px; color:#141414">أخبار</a>
               </li>
             </ul>
           </body></html>"#,
    );

    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "أخبار")),
        "blockified direct text children in flex anchors must paint"
    );
}

#[test]
fn empty_generated_flex_icon_contributes_declared_width() {
    let (frame, list) = build_full(
        r#"<style>
             body { margin: 0 }
             button { display: flex; align-items: center; column-gap: 2px; padding: 0; font: 16px/20px sans-serif; }
             button::after { content: ""; display: block; width: 20px; height: 20px; background: currentColor; }
           </style>
           <button id="b">HTML</button>"#,
    );
    let button_id = frame.doc.get_element_by_id("b").expect("button");
    let button = frame.doc.get_node(button_id).expect("button node");
    let after = button
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("empty generated ::after should be materialized");

    assert!(
        after.layout.content_rect.w >= 19.0,
        "empty generated icon should keep its declared width, got {:.1}",
        after.layout.content_rect.w
    );
    assert!(
        button.layout.content_rect.w >= after.layout.content_rect.w + 35.0,
        "button content width should include text plus generated icon; button={:.1} after={:.1}",
        button.layout.content_rect.w,
        after.layout.content_rect.w
    );
    assert!(
        list.commands.iter().any(
            |cmd| matches!(cmd, PaintCmd::FillRect { rect, .. } if rect.w >= 19.0 && rect.h >= 19.0)
        ),
        "generated icon should paint at its declared box size"
    );
}

#[test]
fn empty_generated_flex_icon_survives_display_contents_wrapper() {
    let (frame, list) = build_full(
        r#"<style>
             body { margin: 0 }
             *,:before,:after { box-sizing: border-box; }
             mdn-dropdown { display: contents; }
             .navigation { display: flex; }
             .navigation__popup { display: block; }
             .navigation__menu { display: flex; }
             .menu { display: flex; }
             .menu__tab { display: block; }
             .menu__tab-button {
               align-items: center;
               border: 1px solid transparent;
               border-bottom: none;
               column-gap: .125rem;
               color: white;
               display: flex;
               font: inherit;
               line-height: 1.25;
               margin: 0;
               padding: .5rem .6875rem;
               white-space: nowrap;
             }
             .menu__tab-button:after,
             .menu__tab-button:before {
               background-color: currentcolor;
               height: 1.25rem;
               mask-size: cover;
               width: 1.25rem;
             }
             .menu__tab-button:after {
               content: "";
               mask-image: url('/static/client/chevron-down.svg');
             }
           </style>
           <nav class="navigation">
             <div class="navigation__popup">
               <div class="navigation__menu">
                 <nav class="menu">
                   <div class="menu__tab">
                     <mdn-dropdown><button id="b" class="menu__tab-button"><span class="menu__tab-label">HTML</span></button></mdn-dropdown>
                   </div>
                 </nav>
               </div>
             </div>
           </nav>"#,
    );
    let button_id = frame.doc.get_element_by_id("b").expect("button");
    let button = frame.doc.get_node(button_id).expect("button node");
    let after = button
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("empty generated ::after should be materialized");

    assert!(
        after.layout.content_rect.w >= 19.0 && after.layout.content_rect.h >= 19.0,
        "empty generated icon should keep declared geometry through display:contents wrapper, got {:.1}x{:.1}",
        after.layout.content_rect.w,
        after.layout.content_rect.h
    );
    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::FillRect { rect, color, .. }
                if rect.w >= 19.0 && rect.h >= 19.0 && color.a > 200)),
        "generated mask icon should paint its currentColor background into its own box"
    );
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
fn rtl_inline_atomic_box_is_placed_before_following_text_visually() {
    let mut doc = parse_html(
        r##"<html dir="rtl"><body style="margin:0">
             <div id="badge" style="direction:rtl; width:57px; font:12px/29px sans-serif">
               <svg id="play" style="display:inline-block;width:24px;height:24px" viewBox="0 0 13 13">
                 <path d="M.5.6h12v12H.5z"/>
                 <path fill="currentColor" d="M2.144.96v11.28l8.712-5.64z"/>
               </svg><time id="duration">3:20</time>
             </div>
           </body></html>"##,
    );
    let mut renderer = Renderer::new();
    renderer.layout_engine().layout(&mut doc, 800.0);

    let play = crate::dom::query_selector(&doc.root, "#play").expect("play icon");
    let duration = crate::dom::query_selector(&doc.root, "#duration").expect("duration");
    assert!(
        play.layout.border_rect.x > duration.layout.border_rect.x,
        "RTL inline layout should put the DOM-first icon on the right and the following duration on the left, play={:?} duration={:?}",
        play.layout.border_rect,
        duration.layout.border_rect
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

#[test]
fn inline_after_content_on_empty_block_paints_at_content_box() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <style>
             a { display:block; width:74px; height:28px; padding:0 8px; background:#1665cf;
                 color:white; font:700 12px/28px sans-serif; text-align:center; }
             a::after { content:"Subscribe"; }
             </style>
             <a href="/subscribe"></a>
           </body>"#,
    );
    assert!(
        list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::Text {
                    text,
                    color,
                    font_size,
                    ..
                } if text == "Subscribe" && color.r == 255 && color.g == 255 && color.b == 255 && (*font_size - 12.0).abs() < 0.5
            )
        }),
        "empty block with generated ::after text should paint; commands were {:?}",
        list.commands
    );
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
    assert_eq!(svg.style.color, Color::rgb(255, 0, 0));
    assert_eq!(svg.style.svg_fill, Some(Color::rgb(255, 0, 0)));

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
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
fn inline_svg_uses_black_current_color_when_rasterized() {
    let (_, list) = build(
        r#"<style>svg { color: rgb(0, 0, 0); fill: currentColor; }</style>
           <svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect x="0" y="0" width="20" height="20"/>
           </svg>"#,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (20, 20));
    let idx = ((10 * w + 10) * 4) as usize;
    assert!(
        data[idx] < 20 && data[idx + 1] < 20 && data[idx + 2] < 20 && data[idx + 3] > 200,
        "center pixel should be black from cascaded currentColor fill, got rgba({}, {}, {}, {})",
        data[idx],
        data[idx + 1],
        data[idx + 2],
        data[idx + 3]
    );
}

#[test]
fn inline_svg_preserves_current_color_path_over_default_fill() {
    let (_, list) = build(
        r#"<style>svg.play { color: rgb(255, 255, 255); }</style>
           <svg class="play" style="width:13px;height:13px" viewBox="0 0 13 13">
             <path d="M.5.6h12v12H.5z"/>
             <path fill="currentColor" d="M2.144.96v11.28l8.712-5.64z"/>
           </svg>"#,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (13, 13));

    let corner = ((1 * w + 1) * 4) as usize;
    assert!(
        data[corner] < 30 && data[corner + 1] < 30 && data[corner + 2] < 30,
        "square background should keep default black fill, got rgba({}, {}, {}, {})",
        data[corner],
        data[corner + 1],
        data[corner + 2],
        data[corner + 3]
    );

    let center = ((6 * w + 6) * 4) as usize;
    assert!(
        data[center] > 220 && data[center + 1] > 220 && data[center + 2] > 220,
        "triangle should use currentColor white, got rgba({}, {}, {}, {})",
        data[center],
        data[center + 1],
        data[center + 2],
        data[center + 3]
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

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
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

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
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

    let (data, w, _) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
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

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
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
fn inline_svg_text_path_uses_dom_cascaded_svg_paint() {
    let (_, list) = build(
        r##"<style>
             .mandala { color: rgb(255, 255, 255); }
             .mandala svg > text { fill: currentColor; }
             .mandala textPath[href="#circle1"] { font-size: 20px; }
           </style>
           <div class="mandala" style="background:#222">
             <svg style="width:140px;height:60px" viewBox="0 0 140 60" fill="none">
               <defs><path id="circle1" d="M10 42 H130"/></defs>
               <text><textPath href="#circle1"><tspan>MDN</tspan></textPath></text>
             </svg>
           </div>"##,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (140, 60));
    let white_pixels = data
        .chunks_exact(4)
        .filter(|px| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120)
        .count();
    assert!(
        white_pixels > 20,
        "textPath should use DOM-cascaded fill instead of root fill=none, white_pixels={white_pixels}"
    );
}

#[test]
fn inline_svg_circular_text_path_uses_dom_cascaded_svg_paint() {
    let (_, list) = build(
        r##"<style>
             .mandala { color: rgb(255, 255, 255); }
             .mandala svg > text { fill: currentColor; }
             .mandala textPath[href="#circle1"] { font-size: 20px; }
           </style>
           <div class="mandala" style="background:#222">
             <svg style="width:120px;height:120px" viewBox="0 0 120 120" fill="none">
               <defs>
                 <path id="circle1" d="M60,60 m-44,0 a44,44 0 1,1 88,0 a44,44 0 1,1 -88,0"/>
               </defs>
               <text><textPath textLength="276" href="#circle1"><tspan>MDN SVG TEXT PATH</tspan></textPath></text>
             </svg>
           </div>"##,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (120, 120));
    let white_pixels = data
        .chunks_exact(4)
        .filter(|px| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120)
        .count();
    assert!(
        white_pixels > 20,
        "circular textPath should use DOM-cascaded fill instead of root fill=none, white_pixels={white_pixels}"
    );
}

#[test]
fn inline_svg_text_path_uses_dom_cascaded_var_fill() {
    let (_, list) = build(
        r##"<style>
             .mandala { --color-border-primary: rgb(255, 255, 255); }
             .mandala svg > text { fill: var(--color-border-primary); }
             .mandala textPath[href="#circle1"] { font-size: 20px; }
           </style>
           <div class="mandala" style="background:#222">
             <svg style="width:120px;height:120px" viewBox="0 0 120 120" fill="none">
               <defs>
                 <path id="circle1" d="M60,60 m-44,0 a44,44 0 1,1 88,0 a44,44 0 1,1 -88,0"/>
               </defs>
               <text><textPath textLength="276" href="#circle1"><tspan>MDN SVG TEXT PATH</tspan></textPath></text>
             </svg>
           </div>"##,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (120, 120));
    let white_pixels = data
        .chunks_exact(4)
        .filter(|px| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120)
        .count();
    assert!(
        white_pixels > 20,
        "textPath should resolve DOM-cascaded var() fill, white_pixels={white_pixels}"
    );
}

#[test]
fn inline_svg_text_path_text_length_does_not_collapse_to_path_end() {
    let repeated = "/".repeat(120);
    let html = format!(
        r##"<style>
             .mandala {{ --color-border-primary: rgb(255, 255, 255); }}
             .mandala svg > text {{ fill: var(--color-border-primary); }}
             .mandala textPath[href="#circle1"] {{ font-size: 24px; }}
           </style>
           <div class="mandala" style="background:#222">
             <svg style="width:560px;height:560px" viewBox="50 50 575 575" fill="none">
               <defs>
                 <path id="circle1" d="M337.5,337.5 m-320,0 a320,320 0 1,1 640,0 a320,320 0 1,1 -640,0"/>
               </defs>
               <text><textPath textLength="2010" href="#circle1">{repeated}</textPath></text>
             </svg>
           </div>"##
    );
    let doc = parse_html(&html);
    let mut frame = EngineFrame::new(doc, 1366.0, 688.0);
    frame.update_frame();
    let list = build_display_list_full(
        &frame.doc.root,
        1366.0,
        688.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (560, 560));

    let white = |px: &[u8]| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120;
    let total_white = data.chunks_exact(4).filter(|px| white(px)).count();
    let right_edge_white = data
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, px)| {
            let x = (*i as u32) % w;
            x > w - 12 && white(px)
        })
        .count();
    assert!(
        total_white > 80,
        "MDN-shaped textPath should paint visible text, total_white={total_white}"
    );
    assert!(
        right_edge_white * 3 < total_white,
        "textPath glyphs should be distributed around the arc, not clamped at the right edge: right_edge_white={right_edge_white}, total_white={total_white}"
    );
}

#[test]
fn inline_svg_mdn_style_tspan_text_path_uses_dom_styles() {
    let repeated = (0..48).map(|_| "/<tspan>/</tspan>").collect::<String>();
    let html = format!(
        r##"<style>
             .homepage--dark {{ --color-border-primary: rgb(255, 255, 255); }}
             .mandala svg > text {{ fill: var(--color-border-primary); }}
             .mandala textPath[href="#circle1"] {{ font-size: 24px; }}
           </style>
           <div class="homepage--dark">
             <div class="mandala" style="background:#222">
               <svg style="width:560px;height:560px" viewBox="50 50 575 575" fill="none">
                 <defs>
                   <path id="circle1" d="M337.5,337.5 m-320,0 a320,320 0 1,1 640,0 a320,320 0 1,1 -640,0"/>
                 </defs>
                 <text dy="70" textlength="2010">
                   <textpath textlength="2010" href="#circle1">{repeated}</textpath>
                 </text>
               </svg>
             </div>
           </div>"##
    );
    let doc = parse_html(&html);
    let mut frame = EngineFrame::new(doc, 1366.0, 688.0);
    frame.update_frame();
    let list = build_display_list_full(
        &frame.doc.root,
        1366.0,
        688.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (560, 560));

    let white = |px: &[u8]| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut total_white = 0usize;
    for (i, px) in data.chunks_exact(4).enumerate() {
        if white(px) {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            total_white += 1;
        }
    }
    assert!(
        total_white > 80,
        "expected visible SVG text, total_white={total_white}"
    );
    assert!(
        min_x < 80 && max_x > 470 && min_y < 120 && max_y > 430,
        "DOM-styled MDN-like textPath should be distributed around the circle, bounds=({min_x},{min_y},{max_x},{max_y}), total_white={total_white}"
    );
}

#[test]
fn replay_paints_offset_inline_svg_inside_overflow_clip() {
    let repeated = (0..48).map(|_| "/<tspan>/</tspan>").collect::<String>();
    let html = format!(
        r##"<style>
             body {{ margin: 0; }}
             .hero {{ width: 560px; height: 320px; overflow: hidden; background: #111; }}
             .mandala {{ width: 560px; height: 560px; margin-top: -140px; }}
             .mandala svg {{ width: 560px; height: 560px; }}
             .mandala svg > text {{ fill: rgb(255, 255, 255); }}
             .mandala textPath[href="#circle1"] {{ font-size: 24px; }}
           </style>
           <div class="hero">
             <div class="mandala">
               <svg viewBox="50 50 575 575" fill="none">
                 <defs>
                   <path id="circle1" d="M337.5,337.5 m-320,0 a320,320 0 1,1 640,0 a320,320 0 1,1 -640,0"/>
                 </defs>
                 <text dy="70" textlength="2010">
                   <textpath textlength="2010" href="#circle1">{repeated}</textpath>
                 </text>
               </svg>
             </div>
           </div>"##
    );
    let doc = parse_html(&html);
    let mut frame = EngineFrame::new(doc, 1366.0, 688.0);
    frame.update_frame();
    let mut font_system = cosmic_text::FontSystem::new();
    let list = build_display_list_full_with_font_system(
        &frame.doc.root,
        1366.0,
        688.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
        Some(&mut font_system as *mut _),
    );
    let (image_data, image_w, image_h) =
        first_image_data(&list).expect("inline SVG should rasterize before replay");
    assert_eq!((image_w, image_h), (560, 560));
    let image_white = image_data
        .chunks_exact(4)
        .filter(|px| px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120)
        .count();
    assert!(
        image_white > 80,
        "inline SVG raster should contain visible text before replay, image_white={image_white}"
    );
    let mut pixmap = tiny_skia::Pixmap::new(560, 320).expect("pixmap");
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    replay(&list, &mut pixmap, 1.0);

    let mut total_white = 0usize;
    let mut min_x = 560u32;
    let mut max_x = 0u32;
    for (i, px) in pixmap.data().chunks_exact(4).enumerate() {
        if px[0] > 180 && px[1] > 180 && px[2] > 180 && px[3] > 120 {
            let x = (i as u32) % 560;
            total_white += 1;
            min_x = min_x.min(x);
            max_x = max_x.max(x);
        }
    }
    assert!(
        total_white > 40,
        "offset inline SVG should paint through its overflow clip, total_white={total_white}"
    );
    assert!(
        min_x < 120 && max_x > 440,
        "clipped offset SVG should not collapse to the right edge, bounds=({min_x},{max_x}), total_white={total_white}"
    );
}

#[test]
fn inline_svg_mdn_homepage_mandala_rings_use_dom_styles() {
    let slashes = (0..42)
        .map(|_| "/      <tspan>\n/      </tspan>\n")
        .collect::<String>();
    let pluses = (0..21)
        .map(|_| "+      <tspan>\n+      </tspan>\n")
        .collect::<String>();
    let braces = (0..12)
        .map(|_| "{      <tspan>\n}      </tspan>\n")
        .collect::<String>();
    let tags = (0..16)
        .map(|_| "<tspan>\n&lt;&gt;      </tspan>\n&lt;/&gt;      ")
        .collect::<String>();
    let dummy_rules = (0..1100)
        .map(|i| format!(".unused{i} {{ color: rgb(1, 2, 3); }}\n"))
        .collect::<String>();
    let html = format!(
        r##"<style>
             {dummy_rules}
             body {{ margin: 0; }}
             .homepage--dark {{ --color-border-primary: rgb(81, 86, 93); }}
             .hero {{ width: 560px; height: 320px; overflow: hidden; background: rgb(33, 36, 38); }}
             .mandala {{ width: 560px; height: 560px; margin-top: -140px; }}
             .mandala svg {{ width: 560px; height: 560px; }}
             .mandala svg > text {{ fill: var(--color-border-primary); }}
             .mandala textpath[href="#circle1"] {{ font-size: 24px; }}
             .mandala textpath[href="#circle2"] {{ font-size: 20.8px; }}
             .mandala textpath[href="#circle3"] {{ font-size: 19.2px; }}
             .mandala textpath[href="#circle4"] {{ font-size: 17.6px; }}
             .mandala textpath[href="#circle5"] {{ font-size: 16px; }}
           </style>
           <div class="homepage--dark">
             <div class="hero">
               <div class="mandala">
                 <svg viewbox="50 50 575 575" fill="none" xmlns="http://www.w3.org/2000/svg" class="mandala">
                   <defs>
                     <path d="M337.5,337.5 m-320,0 a320,320 0 1,1 640,0 a320,320 0 1,1 -640,0" id="circle1"/>
                     <path d="M337.5,337.5 m-280,0 a280,280 0 1,1 560,0 a280,280 0 1,1 -560,0" id="circle2"/>
                     <path d="M337.5,337.5 m-240,0 a240,240 0 1,1 480,0 a240,240 0 1,1 -480,0" id="circle3"/>
                     <path d="M337.5,337.5 m-200,0 a200,200 0 1,1 400,0 a200,200 0 1,1 -400,0" id="circle4"/>
                     <path d="M337.5,337.5 m-160,0 a160,160 0 1,1 320,0 a160,160 0 1,1 -320,0" id="circle5"/>
                   </defs>
                   <text dy="70" textlength="2010"><textpath textlength="2010" href="#circle1">{slashes}</textpath></text>
                   <text dy="70" textlength="1760"><textpath textlength="1760" href="#circle2">{pluses}</textpath></text>
                   <text dy="70" textlength="1507"><textpath textlength="1507" href="#circle3">{braces}</textpath></text>
                   <text dy="70" textlength="1257"><textpath textlength="1257" href="#circle4">../../    ../../    ../../    ../../    ../../    ../../    ../../</textpath></text>
                   <text dy="70" textlength="1005"><textpath textlength="1005" href="#circle5">{tags}</textpath></text>
                 </svg>
               </div>
             </div>
           </div>"##
    );
    let (_, list) = build(&html);
    let (image_data, image_w, image_h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image");
    assert_eq!((image_w, image_h), (560, 560));
    let mut min_x = image_w;
    let mut min_y = image_h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut colored = 0usize;
    for (i, px) in image_data.chunks_exact(4).enumerate() {
        if px[3] > 120 && px[0] > 55 && px[1] > 55 && px[2] > 55 {
            let x = (i as u32) % image_w;
            let y = (i as u32) / image_w;
            colored += 1;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    assert!(
        colored > 80,
        "DOM-styled MDN mandala should paint visible gray rings, colored={colored}"
    );
    assert!(
        min_x < 80 && max_x > 470 && min_y < 120 && max_y > 430,
        "DOM-styled MDN mandala should be distributed around the circles, bounds=({min_x},{min_y},{max_x},{max_y}), colored={colored}"
    );

    let mut pixmap = tiny_skia::Pixmap::new(560, 320).expect("pixmap");
    pixmap.fill(tiny_skia::Color::TRANSPARENT);
    replay(&list, &mut pixmap, 1.0);
    let bg = (33i16, 36i16, 38i16);
    let mut visible = 0usize;
    let mut min_visible_x = 560u32;
    let mut max_visible_x = 0u32;
    for (i, px) in pixmap.data().chunks_exact(4).enumerate() {
        let d =
            (px[0] as i16 - bg.0).abs() + (px[1] as i16 - bg.1).abs() + (px[2] as i16 - bg.2).abs();
        if px[3] > 120 && d > 20 {
            let x = (i as u32) % 560;
            visible += 1;
            min_visible_x = min_visible_x.min(x);
            max_visible_x = max_visible_x.max(x);
        }
    }
    assert!(
        visible > 40,
        "replayed clipped MDN mandala should visibly differ from its dark background, visible={visible}"
    );
    assert!(
        min_visible_x < 120 && max_visible_x > 440,
        "replayed clipped MDN mandala should remain distributed, bounds=({min_visible_x},{max_visible_x}), visible={visible}"
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

    let doc = parse_html(r#"<img src="x.svg" style="width:20px;height:20px">"#);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
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
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        PaintCmd::Image {
            data: ImageRef::Shared(data, w, h),
            ..
        } => Some((data.as_ref(), *w, *h)),
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

    let doc = parse_html(r#"<img src="x.svg" style="width:20px;height:20px;color:white">"#);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
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
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let image = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Image {
            data: ImageRef::Owned(data, w, h),
            ..
        } => Some((data, *w, *h)),
        PaintCmd::Image {
            data: ImageRef::Shared(data, w, h),
            ..
        } => Some((data.as_ref(), *w, *h)),
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
fn data_svg_background_decodes_before_first_paint() {
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        r##"
        <style>
        body { margin: 0; }
        #icon {
            width: 18px;
            height: 18px;
            background-image: url('data:image/svg+xml;utf8,<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 20 20" fill="%23000"><path d="M8 1a7 7 0 015.605 11.191l5.102 5.102-1.414 1.414-5.102-5.102A7 7 0 118 1m0 2a5 5 0 100 10A5 5 0 008 3"/></svg>');
            background-repeat: no-repeat;
            background-position: center;
            background-size: max(calc(0.875rem + 4px), 10px);
        }
        </style>
        <div id="icon"></div>
        "##,
        800.0,
    );
    let icon = crate::tests::harness::find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "icon")
    })
    .expect("icon");
    assert!(
        icon.bg_image_data.is_some(),
        "data: SVG CSS backgrounds should decode synchronously for first paint"
    );

    let list = build_display_list(&doc.root, 800.0, 600.0);
    let bg = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::BackgroundImage { draw_w, draw_h, .. } => Some((*draw_w, *draw_h)),
        _ => None,
    });
    let (draw_w, draw_h) = bg.expect("decoded background should produce a paint command");
    assert!((draw_w - 18.0).abs() < 0.5, "draw_w={draw_w}");
    assert!((draw_h - 18.0).abs() < 0.5, "draw_h={draw_h}");
}

#[test]
fn background_position_keyword_offsets_resolve_from_far_edge() {
    fn find_by_id_mut<'a>(
        node: &'a mut crate::WebCore,
        id: &str,
    ) -> Option<&'a mut crate::WebCore> {
        if node.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(node);
        }
        node.children
            .iter_mut()
            .find_map(|child| find_by_id_mut(child, id))
    }

    let doc = parse_html(
        r#"
        <style>body{margin:0}</style>
        <div id="box" style="
            width: 100px;
            height: 60px;
            background-image: url(sprite.svg);
            background-repeat: no-repeat;
            background-position: right 8px bottom 6px;
        "></div>
        "#,
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    let node = find_by_id_mut(&mut frame.doc.root, "box").expect("box node");
    node.bg_image_data = Some(std::sync::Arc::new(vec![255; 20 * 10 * 4]));
    node.bg_image_width = 20;
    node.bg_image_height = 10;
    node.bg_image_ratio_only = false;

    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let bg = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::BackgroundImage {
            pos_x,
            pos_y,
            draw_w,
            draw_h,
            ..
        } => Some((*pos_x, *pos_y, *draw_w, *draw_h)),
        _ => None,
    });
    let (x, y, w, h) = bg.expect("background image should paint");
    assert!((w - 20.0).abs() < 0.1, "draw_w={w}");
    assert!((h - 10.0).abs() < 0.1, "draw_h={h}");
    assert!(
        (x - 72.0).abs() < 0.1,
        "right 8px should paint at x=72, got {x}"
    );
    assert!(
        (y - 44.0).abs() < 0.1,
        "bottom 6px should paint at y=44, got {y}"
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

#[test]
fn will_change_transform_creates_replay_transform_slot() {
    let (_, list) =
        build(r#"<div style="will-change: transform; width: 100px; height: 50px">ticker</div>"#);
    assert!(
        list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::PushTransform {
                    transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    ..
                }
            )
        }),
        "will-change: transform should reserve a replay-time transform slot"
    );
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
fn flex_container_does_not_repaint_child_button_text_from_line_cache() {
    let (_, list) = build(
        r#"<div style="display:flex;justify-content:flex-end;width:400px">
              <button style="background:#0d6efd;color:white;padding:6px 12px">Add Tree</button>
           </div>"#,
    );
    let add_tree_texts = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.trim() == "Add Tree"))
        .count();
    assert_eq!(
        add_tree_texts, 1,
        "flex container must not also paint a stale aggregate line-cache copy of child text"
    );
}

#[test]
fn flex_end_button_background_keeps_bootstrap_padding() {
    let (_, list) = build(
        r#"<style>
             .d-flex { display:flex; }
             .justify-content-end { justify-content:flex-end; }
             .btn { display:inline-block; padding:6px 12px; border:1px solid transparent; }
             .btn-primary { color:white; background-color:#0d6efd; }
           </style>
           <div class="d-flex justify-content-end" style="width:400px">
             <button class="btn btn-primary">Add Tree</button>
           </div>"#,
    );
    let blue_button_width = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. } if *color == Color::rgb(13, 110, 253) => {
                Some(rect.w)
            }
            _ => None,
        })
        .expect("button should paint a primary background");
    assert!(
        blue_button_width >= 74.0,
        "button background should include text plus Bootstrap horizontal padding; got width {blue_button_width}"
    );
}

#[test]
fn inline_block_badge_paints_its_background_bubble() {
    let (_, list) = build(
        r#"<span style="display:inline-block;background:rgb(248,249,250);color:rgb(33,37,41);padding:6px 12px;border-radius:999px">2542 members</span>"#,
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::FillRect { color, radius, .. }
                if *color == Color::rgb(248, 249, 250) && radius.iter().any(|r| *r > 0.0)
        )),
        "inline-block badge background should paint as a rounded bubble"
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
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(html, 800.0);

    // Without hover
    let list_no_hover = build_display_list_full(
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

    let btn_id = doc.get_element_by_id("btn").unwrap();
    let rect = doc.get_bounding_client_rect(btn_id).unwrap();
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseMove,
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
        0,
    );
    renderer.layout_engine().layout(&mut doc, 800.0);

    // With hover applied by cascade/layout.
    let list_hover = build_display_list_full(
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

#[test]
fn hover_inherited_text_color_repaints_from_current_source_style() {
    let html = r#"<html><head><style>
        .btn {
            color: #7c6af7;
            background: transparent;
            width: 100px;
            height: 40px;
        }
        .btn:hover {
            color: #fff;
            background: #7c6af7;
        }
    </style></head><body>
        <div class="btn" id="btn"><span>Outline</span></div>
    </body></html>"#;
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(html, 800.0);

    let btn_id = doc.get_element_by_id("btn").unwrap();
    let rect = doc.get_bounding_client_rect(btn_id).unwrap();
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseMove,
        (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
        0,
    );
    renderer.layout_engine().layout(&mut doc, 800.0);

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

    let outline_color = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { text, color, .. } if text == "Outline" => Some(*color),
        _ => None,
    });
    assert_eq!(
        outline_color,
        Some(Color::rgb(255, 255, 255)),
        "hover text paint must use current inherited source-node color"
    );
}

#[test]
fn animated_inherited_text_color_repaints_from_scoped_override() {
    let html = r#"<div id="btn" style="color:#7c6af7"><span>Outline</span></div>"#;
    let mut frame = EngineFrame::new(parse_html(html), 800.0, 600.0);
    frame.update_frame();

    let btn_id = frame.doc.get_element_by_id("btn").unwrap();
    frame.doc.animation_overrides.insert(
        btn_id,
        vec![("color".to_string(), "rgb(255,255,255)".to_string())],
    );
    let overrides = frame.doc.animation_overrides.clone();
    let restore = crate::css::apply_animation_overrides_scoped(&mut frame.doc.root, &overrides);
    let list = build_display_list_full(
        &frame.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    crate::css::restore_animation_overrides(&mut frame.doc.root, restore);

    let outline_color = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { text, color, .. } if text == "Outline" => Some(*color),
        _ => None,
    });
    assert_eq!(
        outline_color,
        Some(Color::rgb(255, 255, 255)),
        "animated inherited color must repaint cached inline text runs"
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
fn replay_transforms_background_image_clip_with_the_image() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::PushTransform {
        node_id: 9,
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, -9.0],
    });
    list.push(PaintCmd::BackgroundImage {
        container: Rect::new(20.0, 20.0, 18.0, 18.0),
        clip: Rect::new(20.0, 20.0, 18.0, 18.0),
        data: ImageRef::Owned(vec![255, 0, 0, 255], 1, 1),
        size_mode: 3,
        draw_w: 18.0,
        draw_h: 18.0,
        pos_x: 20.0,
        pos_y: 20.0,
        repeat_x_mode: 0,
        repeat_y_mode: 0,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
        blend_mode: 0,
    });
    list.push(PaintCmd::PopTransform);

    let mut pixmap = tiny_skia::Pixmap::new(80, 80).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    replay(&list, &mut pixmap, 1.0);

    let top = pixmap
        .pixel(25, 12)
        .expect("sample inside transformed image");
    assert_eq!(
        (top.red(), top.green(), top.blue(), top.alpha()),
        (255, 0, 0, 255),
        "background-image clip must move with a CSS transform"
    );

    let old_clip_only = pixmap
        .pixel(25, 35)
        .expect("sample below transformed image");
    assert_eq!(
        (
            old_clip_only.red(),
            old_clip_only.green(),
            old_clip_only.blue(),
            old_clip_only.alpha()
        ),
        (255, 255, 255, 255),
        "the untransformed clip area must not mask/draw stale pixels"
    );
}

#[test]
fn replay_can_apply_transform_animation_without_rebuilding_display_list() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::PushTransform {
        node_id: 7,
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    });
    list.push(PaintCmd::FillRect {
        rect: Rect::new(10.0, 10.0, 20.0, 20.0),
        color: Color::rgb(255, 0, 0),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    });
    list.push(PaintCmd::PopTransform);

    let mut pixmap = tiny_skia::Pixmap::new(80, 50).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    let overrides = std::collections::HashMap::from([(7, [1.0, 0.0, 0.0, 1.0, 30.0, 0.0])]);

    replay_with_scroll_and_transform_overrides(
        &list,
        &mut pixmap,
        1.0,
        &mut font_system,
        &mut swash_cache,
        0.0,
        0.0,
        &overrides,
    );

    let moved = pixmap.pixel(45, 15).expect("sample moved box");
    assert_eq!(
        (moved.red(), moved.green(), moved.blue(), moved.alpha()),
        (255, 0, 0, 255),
        "animated transform override should move cached display-list content"
    );
    let old = pixmap.pixel(15, 15).expect("sample old box position");
    assert_eq!(
        (old.red(), old.green(), old.blue(), old.alpha()),
        (255, 255, 255, 255),
        "old position should not repaint when transform override is applied"
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
        file_button_color: Color::BLACK,
        file_button_background: Color::TRANSPARENT,
        file_button_font_size: 16.0,
        file_button_font_weight: 400,
        file_button_font_family: "Arial".to_string(),
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

#[test]
fn positioned_negative_z_before_background_paints() {
    let (_, list) = build_full(
        r#"<html><head><style>
          html, body { margin: 0; padding: 0; }
          #bar { position: relative; z-index: 0; width: 400px; height: 40px; margin-left: 200px; }
          #bar::before {
            content: "";
            position: absolute;
            z-index: -1;
            top: 0;
            height: 40px;
            left: 50%;
            width: 100vw;
            transform: translateX(-50%);
            background: rgb(184, 0, 0);
          }
        </style></head><body><div id="bar"></div></body></html>"#,
    );

    let red_generated_backgrounds = list
        .commands
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                PaintCmd::FillRect { rect, color, .. }
                    if color.r == 184 && color.g == 0 && color.b == 0 && rect.w >= 790.0
            )
        })
        .count();
    assert!(
        red_generated_backgrounds >= 1,
        "positioned generated backgrounds with negative z-index must be painted"
    );
    assert_eq!(
        red_generated_backgrounds, 1,
        "positioned generated backgrounds must not be replayed by every ancestor"
    );
}

#[test]
fn positioned_after_with_inset_and_bottom_border_paints() {
    let (_, list) = build_full(
        r#"<html><head><style>
          html, body { margin: 0; padding: 0; }
          #bar { position: relative; width: 240px; height: 44px; }
          #bar::after {
            content: "";
            position: absolute;
            left: 0;
            right: 0;
            bottom: 0;
            border-bottom: 0.0625rem solid #E6E8EA;
          }
        </style></head><body><nav id="bar"></nav></body></html>"#,
    );

    let border = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Border {
            rect,
            widths,
            colors,
            ..
        } if widths[2] > 0.0 && colors[2].r == 230 && colors[2].g == 232 => Some((*rect, *widths)),
        _ => None,
    });
    let (rect, widths) = border.expect("absolute ::after bottom border should paint");
    assert!(
        rect.w >= 239.0 && rect.h >= 1.0 && widths[2] >= 1.0,
        "absolute ::after border should span the parent width, got rect={rect:?} widths={widths:?}"
    );
}

#[test]
fn rtl_flex_nav_intrinsic_width_keeps_inline_block_items_on_one_line() {
    let doc = parse_html(
        r#"<html dir="rtl"><body style="margin:0"><header style="direction:rtl"><div style="display:flex;justify-content:space-between;align-items:stretch;width:1280px;background:#b80000"><div id="left" style="width:900px;height:44px"></div><div id="menu"><ul id="nav" style="list-style:none;margin:0;padding:0;position:relative;overflow:hidden"><li style="display:inline-block;position:relative;margin-inline-end:0"><a style="display:flex;flex-direction:column;justify-content:center;height:44px;padding:0 8px;font-size:16px;line-height:24px">الرئيسية</a></li><li style="display:inline-block;position:relative;margin-inline-end:0"><a style="display:flex;flex-direction:column;justify-content:center;height:44px;padding:0 8px;font-size:16px;line-height:24px">أخبار</a></li><li style="display:inline-block;position:relative;margin-inline-end:0"><a style="display:flex;flex-direction:column;justify-content:center;height:44px;padding:0 8px;font-size:16px;line-height:24px">رياضة</a></li></ul></div></div></header></body></html>"#,
    );
    let mut frame = EngineFrame::new(doc, 1280.0, 900.0);
    frame.update_frame();
    let nav = crate::dom::query_selector(&frame.doc.root, "#nav").expect("nav should exist");

    assert_eq!(
        nav.layout.line_cache.len(),
        1,
        "RTL inline-block menu items should fit on one line, got lines={:?}",
        nav.layout.line_cache
    );
    assert!(
        nav.layout.border_rect.h <= 50.0,
        "single-row nav should stay around 44px high, got {:?}",
        nav.layout.border_rect
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
fn render_display_list_text_clipped_by_overflow_hidden() {
    let doc = parse_html(
        r#"<body style="margin: 0; background: white"><div style="position: absolute; left: 100px; top: 20px; width: 100px; height: 40px; overflow: hidden;"><div style="transform: translateX(-60px); white-space: nowrap; font-size: 20px; color: black;">Long scrolling text across the container</div></div></body>"#,
    );
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();

    let mut pixmap = tiny_skia::Pixmap::new(400, 300).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pixmap, 1.0);

    let data = pixmap.data();
    // Pixels to the left of x = 100 must NOT contain text pixels (must stay white: 255, 255, 255)
    for y in 20..50 {
        for x in 0..95 {
            let idx = (y * 400 + x) as usize * 4;
            let (r, g, b) = (data[idx], data[idx + 1], data[idx + 2]);
            assert!(
                r > 250 && g > 250 && b > 250,
                "pixel at ({x}, {y}) was ({r}, {g}, {b}), text leaked outside overflow:hidden clip!"
            );
        }
    }

    // Pixels inside the clip (x in 100..200) MUST contain text pixels
    let mut has_dark_inside = false;
    for y in 20..50 {
        for x in 100..200 {
            let idx = (y * 400 + x) as usize * 4;
            if data[idx] < 100 && data[idx + 1] < 100 && data[idx + 2] < 100 {
                has_dark_inside = true;
                break;
            }
        }
        if has_dark_inside {
            break;
        }
    }
    assert!(
        has_dark_inside,
        "text inside overflow:hidden clip should be visible"
    );
}

#[test]
fn replay_clip_inside_transform_moves_with_transformed_content() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::PushTransform {
        node_id: 1,
        transform: [1.0, 0.0, 0.0, 1.0, -80.0, 0.0],
    });
    list.push(PaintCmd::PushClip {
        rect: Rect::new(100.0, 20.0, 60.0, 40.0),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    });
    list.push(PaintCmd::FillRect {
        rect: Rect::new(100.0, 20.0, 60.0, 40.0),
        color: Color::rgba(255, 0, 0, 255),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    });
    list.push(PaintCmd::PopClip);
    list.push(PaintCmd::PopTransform);

    let mut pixmap = tiny_skia::Pixmap::new(200, 100).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    replay(&list, &mut pixmap, 1.0);

    let data = pixmap.data();
    let moved_idx = (30 * 200 + 30) * 4;
    assert_eq!(
        &data[moved_idx..moved_idx + 4],
        &[255, 0, 0, 255],
        "transformed clip should expose the transformed fill"
    );
    let unmoved_idx = (30 * 200 + 110) * 4;
    assert_eq!(
        &data[unmoved_idx..unmoved_idx + 4],
        &[255, 255, 255, 255],
        "clip must not stay behind in the untransformed location"
    );
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
fn padded_inline_background_paints_the_inline_box_not_only_glyphs() {
    let (_, list) = build(
        r#"<body style="margin:0; font:16px/20px sans-serif">
             <div style="width:300px">
               <span style="display:inline; background:#202122; color:white; padding:0 8px">100+</span>
             </div>
           </body>"#,
    );

    let (text_idx, text_x) = list
        .commands
        .iter()
        .enumerate()
        .find_map(|cmd| match cmd {
            (idx, PaintCmd::Text { text, x, .. }) if text.contains("100+") => Some((idx, *x)),
            _ => None,
        })
        .expect("text should paint");
    let (bg_idx, bg) = list
        .commands
        .iter()
        .enumerate()
        .filter_map(|cmd| match cmd {
            (idx, PaintCmd::FillRect { rect, color, .. })
                if color.r == 32 && color.g == 33 && color.b == 34 =>
            {
                Some((idx, *rect))
            }
            _ => None,
        })
        .max_by(|(_, a), (_, b)| a.w.partial_cmp(&b.w).unwrap())
        .expect("inline background should paint");

    assert!(
        bg_idx < text_idx,
        "padded inline background must paint before text; bg@{bg_idx}, text@{text_idx}"
    );
    assert!(
        bg.x <= text_x - 7.5,
        "left inline padding should be painted; bg={bg:?}, text_x={text_x}"
    );
    assert!(
        bg.x + bg.w >= text_x + 37.5,
        "inline background should include right padding beyond the text run, bg={bg:?}, text_x={text_x}"
    );
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
            n.image_data_width = iw;
            n.image_data_height = ih;
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
        200,
        100,
    );
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
        200,
        100,
    );
    let r = image_rect(&list).expect("an image was painted");
    // contain → scale = min(100/200, 100/100) = 0.5 → 100x50, centred vertically.
    assert_eq!((r.w, r.h), (100.0, 50.0), "got {}x{}", r.w, r.h);
    assert_eq!(r.y, 25.0, "centred vertically");
}

#[test]
fn object_fit_none_uses_the_natural_size() {
    let list = build_with_image(
        "<style>*{margin:0;padding:0} img{width:100px;height:100px;object-fit:none}</style><img src=x>",
        200,
        100,
    );
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
        200,
        100,
    );
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

fn gradient_paints(list: &DisplayList) -> Vec<(Rect, Rect, u8, u8)> {
    list.commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Gradient {
                rect,
                clip,
                repeat_x_mode,
                repeat_y_mode,
                ..
            } => Some((*rect, *clip, *repeat_x_mode, *repeat_y_mode)),
            _ => None,
        })
        .collect()
}

fn gradient_paints_with_blend(list: &DisplayList) -> Vec<(Rect, Rect, u8, u8, u8)> {
    list.commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Gradient {
                rect,
                clip,
                repeat_x_mode,
                repeat_y_mode,
                blend_mode,
                ..
            } => Some((*rect, *clip, *repeat_x_mode, *repeat_y_mode, *blend_mode)),
            _ => None,
        })
        .collect()
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
fn additional_background_layers_use_their_own_origin_clip_and_blend() {
    let (_, list) = build(
        r#"<body style="margin:0">
             <div style="width:100px;height:50px;padding:10px;border:5px solid transparent;
                         background-image:linear-gradient(red, red), linear-gradient(blue, blue);
                         background-origin:border-box, content-box;
                         background-clip:border-box, content-box;
                         background-blend-mode:multiply, screen"></div>
           </body>"#,
    );
    let paints = gradient_paints_with_blend(&list);
    assert_eq!(paints.len(), 2, "expected two gradients, got {paints:?}");

    let (first_rect, first_clip, _, _, first_blend) = paints[0];
    assert_eq!(first_blend, 1, "primary layer should use multiply");
    assert!(
        first_rect.x.abs() < 0.01
            && first_rect.y.abs() < 0.01
            && (first_rect.w - 130.0).abs() < 0.01
            && (first_rect.h - 80.0).abs() < 0.01,
        "primary origin should be the border box, got {first_rect:?}"
    );
    assert!(
        first_clip.x.abs() < 0.01
            && first_clip.y.abs() < 0.01
            && (first_clip.w - 130.0).abs() < 0.01
            && (first_clip.h - 80.0).abs() < 0.01,
        "primary clip should be the border box, got {first_clip:?}"
    );

    let (second_rect, second_clip, _, _, second_blend) = paints[1];
    assert_eq!(second_blend, 2, "second layer should use screen");
    assert!(
        (second_rect.x - 15.0).abs() < 0.01
            && (second_rect.y - 15.0).abs() < 0.01
            && (second_rect.w - 100.0).abs() < 0.01
            && (second_rect.h - 50.0).abs() < 0.01,
        "second layer origin should be the content box, got {second_rect:?}"
    );
    assert!(
        (second_clip.x - 15.0).abs() < 0.01
            && (second_clip.y - 15.0).abs() < 0.01
            && (second_clip.w - 100.0).abs() < 0.01
            && (second_clip.h - 50.0).abs() < 0.01,
        "second layer clip should be the content box, got {second_clip:?}"
    );
}

#[test]
fn gradient_background_respects_size_and_position_for_underlines() {
    let (_, list) = build(
        r#"<body style="margin:0">
             <div style="width:100px;height:35px;
                         background-image:linear-gradient(to right, black 0, black 33%, transparent 33%, transparent 66%, black 66%, black 100%);
                         background-repeat:no-repeat;
                         background-size:400% 2px;
                         background-position:100% 100%"></div>
           </body>"#,
    );
    let paints = gradient_paints(&list);
    assert_eq!(paints.len(), 1, "expected one gradient, got {paints:?}");
    let (rect, clip, repeat_x, repeat_y) = paints[0];

    assert_eq!((repeat_x, repeat_y), (0, 0));
    assert!(
        (clip.w - 100.0).abs() < 0.01 && (clip.h - 35.0).abs() < 0.01,
        "the gradient clips to the element box, got {clip:?}"
    );
    assert!(
        (rect.w - 400.0).abs() < 0.01 && (rect.h - 2.0).abs() < 0.01,
        "background-size should produce a 400px by 2px image, got {rect:?}"
    );
    assert!(
        (rect.x + 300.0).abs() < 0.01 && (rect.y - 33.0).abs() < 0.01,
        "background-position:100% 100% should align the sized gradient at the lower right, got {rect:?}"
    );
}

#[test]
fn background_shorthand_gradient_keeps_size_position_and_repeat() {
    let (_, list) = build(
        r#"<body style="margin:0">
             <div style="width:100px;height:35px;
                         background:linear-gradient(to right, black, transparent) 100% 100% / 400% 2px no-repeat"></div>
           </body>"#,
    );
    let paints = gradient_paints(&list);
    assert_eq!(paints.len(), 1, "expected one gradient, got {paints:?}");
    let (rect, _, repeat_x, repeat_y) = paints[0];

    assert_eq!((repeat_x, repeat_y), (0, 0));
    assert!(
        (rect.w - 400.0).abs() < 0.01 && (rect.h - 2.0).abs() < 0.01,
        "background shorthand should keep / background-size, got {rect:?}"
    );
    assert!(
        (rect.x + 300.0).abs() < 0.01 && (rect.y - 33.0).abs() < 0.01,
        "background shorthand should keep background-position, got {rect:?}"
    );
}

#[test]
fn background_shorthand_multiple_gradient_layers_keep_independent_options() {
    let (_, list) = build(
        r#"<body style="margin:0">
             <div style="width:100px;height:50px;
                         background:
                           linear-gradient(red, red) 100% 100% / 400% 2px no-repeat,
                           linear-gradient(blue, blue) 0 0 / 10px 4px repeat-x"></div>
           </body>"#,
    );
    let paints = gradient_paints(&list);
    assert_eq!(paints.len(), 2, "expected two gradients, got {paints:?}");

    let (first_rect, first_clip, first_repeat_x, first_repeat_y) = paints[0];
    assert_eq!((first_repeat_x, first_repeat_y), (0, 0));
    assert!(
        (first_clip.w - 100.0).abs() < 0.01 && (first_clip.h - 50.0).abs() < 0.01,
        "first layer clips to the element box, got {first_clip:?}"
    );
    assert!(
        (first_rect.w - 400.0).abs() < 0.01
            && (first_rect.h - 2.0).abs() < 0.01
            && (first_rect.x + 300.0).abs() < 0.01
            && (first_rect.y - 48.0).abs() < 0.01,
        "first layer should keep its underline geometry, got {first_rect:?}"
    );

    let (second_rect, _, second_repeat_x, second_repeat_y) = paints[1];
    assert_eq!((second_repeat_x, second_repeat_y), (1, 0));
    assert!(
        (second_rect.w - 10.0).abs() < 0.01
            && (second_rect.h - 4.0).abs() < 0.01
            && second_rect.x.abs() < 0.01
            && second_rect.y.abs() < 0.01,
        "second layer should keep its own tile geometry, got {second_rect:?}"
    );
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
fn additional_background_url_layer_after_gradient_is_painted() {
    fn find_by_id_mut<'a>(
        node: &'a mut crate::WebCore,
        id: &str,
    ) -> Option<&'a mut crate::WebCore> {
        if node.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(node);
        }
        node.children
            .iter_mut()
            .find_map(|child| find_by_id_mut(child, id))
    }

    let doc = parse_html(
        r#"<div id="sprite" style="width:22px;height:22px;background-image:linear-gradient(transparent,transparent),url(sprite.svg);background-position:0 -747px;background-repeat:no-repeat"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 200.0, 120.0);
    frame.update_frame();
    let node = find_by_id_mut(&mut frame.doc.root, "sprite").expect("sprite node");
    assert_eq!(
        node.style.rare().additional_background_layers.len(),
        1,
        "url layer after transparent gradient should be retained as an additional layer"
    );
    node.additional_bg_images = vec![Some(crate::types::DecodedBackgroundImage {
        data: std::sync::Arc::new(vec![255; 32 * 800 * 4]),
        width: 32,
        height: 800,
        ratio_only: false,
    })];

    let list = build_display_list_full(
        &frame.doc.root,
        200.0,
        120.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );
    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BackgroundImage {
            container,
            draw_w,
            draw_h,
            pos_y,
            ..
        }
                if (*draw_w - 32.0).abs() < 0.1
                    && (*draw_h - 800.0).abs() < 0.1
                    && (*pos_y - (container.y - 747.0)).abs() < 0.1)),
        "second background URL layer should paint with sprite background-position"
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
        file_button_color: Color::BLACK,
        file_button_background: Color::TRANSPARENT,
        file_button_font_size: 16.0,
        file_button_font_weight: 400,
        file_button_font_family: "Arial".to_string(),
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

#[test]
fn border_radius_fifty_percent_paints_as_circle() {
    let (_f, list) = build(
        "<style>* { margin:0; padding:0 }\
         div { width:48px; height:48px; background:red; border-radius:50% }</style><div></div>",
    );
    let mut pixmap = tiny_skia::Pixmap::new(60, 60).unwrap();
    replay(&list, &mut pixmap, 1.0);

    let alpha_at = |x: u32, y: u32| pixmap.pixel(x, y).map(|p| p.alpha()).unwrap_or(0);
    assert!(
        alpha_at(24, 24) > 200,
        "center of circular background should be filled"
    );
    assert_eq!(
        alpha_at(0, 0),
        0,
        "corner outside circular background should stay transparent"
    );
    let outside_arc_alpha = alpha_at(4, 8);
    assert!(
        outside_arc_alpha < 32,
        "50% border-radius should follow a circular arc, not a quadratic squircle; alpha={outside_arc_alpha}"
    );
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
                PaintCmd::PushTransform { transform, .. } => Some(*transform),
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
fn collapsed_table_borders_resolve_per_spanning_edge_segment() {
    let (_, list) = build(
        r#"
        <style>
        body { margin: 0; }
        table { border-collapse: collapse; border-spacing: 0; }
        td { padding: 0; width: 40px; height: 30px; border: 0; }
        #span { border-right: 4px solid red; }
        #top { border-left: 8px solid blue; }
        #bottom { border-left: 1px solid green; }
        </style>
        <table>
          <tr><td id="span" rowspan="2"></td><td id="top"></td></tr>
          <tr><td id="bottom"></td></tr>
        </table>
    "#,
    );

    let mut has_blue_segment = false;
    let mut has_red_segment = false;
    let mut has_green_segment = false;
    for cmd in &list.commands {
        if let PaintCmd::Border { widths, colors, .. } = cmd {
            if widths.iter().any(|w| *w > 0.0) && colors.contains(&Color::rgb(0, 0, 255)) {
                has_blue_segment = true;
            }
            if widths.iter().any(|w| *w > 0.0) && colors.contains(&Color::rgb(255, 0, 0)) {
                has_red_segment = true;
            }
            if widths.iter().any(|w| *w > 0.0) && colors.contains(&Color::rgb(0, 128, 0)) {
                has_green_segment = true;
            }
        }
    }

    assert!(
        has_blue_segment,
        "top internal segment should use the wider blue border"
    );
    assert!(
        has_red_segment,
        "bottom internal segment should use the spanning red border"
    );
    assert!(
        !has_green_segment,
        "weaker green border must lose only its own segment, not overwrite the whole spanning edge"
    );
}

#[test]
fn collapsed_table_borders_follow_late_layout_shifts() {
    let (frame, list) = build(
        r#"
        <style>
        body { margin: 0; }
        .wrap { display: flex; padding-left: 200px; }
        table { border-collapse: collapse; border-spacing: 0; }
        td { padding: 0; width: 40px; height: 30px; border: 1px solid #a2a9b1; }
        </style>
        <div class="wrap">
          <table><tr><td id="cell"></td><td></td></tr><tr><td></td><td></td></tr></table>
        </div>
    "#,
    );

    let cell = crate::tests::harness::find_box(&frame.doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "cell")
    })
    .expect("cell");
    let cell_left = cell.layout.border_rect.x;

    let left_border_x = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Border {
                rect,
                widths,
                colors,
                ..
            } if widths[3] > 0.0 && colors[3] == Color::rgb(162, 169, 177) => Some(rect.x),
            _ => None,
        })
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .expect("collapsed left border should paint");

    assert!(
        (left_border_x - (cell_left - 0.5)).abs() < 0.75,
        "collapsed border should move with shifted cell: border_x={left_border_x}, cell_left={cell_left}"
    );
}

#[test]
fn auto_table_keeps_columns_at_min_content_even_when_container_is_narrow() {
    let (frame, _list) = build(
        r#"
        <style>
        body { margin: 0; }
        .wrap { width: 120px; }
        table { border-collapse: collapse; }
        td { padding: 0; border: 1px solid #a2a9b1; white-space: normal; }
        .nowrap { white-space: nowrap; }
        </style>
        <div class="wrap">
          <table><tr><td><span class="nowrap">abc def ghi</span></td><td><span class="nowrap">jkl mno pqr</span></td></tr></table>
        </div>
        "#,
    );

    let table = find_node_by_tag(&frame.doc.root, "table").expect("table");
    assert!(
        table.layout.border_rect.w > 150.0,
        "auto table should overflow the container rather than squeeze columns below min-content: {:?}",
        table.layout.border_rect
    );
    let cells = crate::tests::harness::find_all_boxes(&frame.doc.root, &|node| node.tag == "td");
    assert_eq!(cells.len(), 2);
    assert!(
        cells[0].layout.border_rect.w > 70.0 && cells[1].layout.border_rect.w > 70.0,
        "nowrap inline descendants should contribute their full unbreakable width to each column: {:?} {:?}",
        cells[0].layout.border_rect,
        cells[1].layout.border_rect
    );
}

#[test]
fn direct_text_around_inline_links_keeps_collapsed_space_advances() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <p style="margin:0;font:16px sans-serif">A <a>B</a> C</p>
           </body>"#,
    );

    let mut painted = Vec::new();
    for cmd in &list.commands {
        if let PaintCmd::Text { text, x, .. } = cmd {
            painted.push((text.clone(), *x));
        }
    }
    let first = painted
        .iter()
        .find(|(text, _)| text.contains('A'))
        .expect("first A")
        .1;
    let link = painted
        .iter()
        .find(|(text, _)| text == "B")
        .expect("link B")
        .1;
    let trailing = painted
        .iter()
        .rev()
        .find(|(text, _)| text.contains('C'))
        .expect("trailing C")
        .1;
    let first_gap = link - first;
    let second_gap = trailing - link;
    assert!(
        first_gap > 11.0 && second_gap > 11.0,
        "spaces between direct text/link/direct text fragments should advance paint positions: {painted:?}"
    );
}

#[test]
fn right_float_line_breaks_before_entering_float_edge() {
    let (frame, _list) = build(
        r#"
        <style>
        body { margin: 0; font: 16px sans-serif; }
        .wrap { width: 520px; }
        .float { float: right; width: 180px; height: 120px; }
        p { margin: 0; }
        </style>
        <div class="wrap">
          <div class="float"></div>
          <p id="p">alpha beta gamma delta epsilon zeta eta theta iota kappa lambda</p>
        </div>
        "#,
    );

    let para = crate::tests::harness::find_box(&frame.doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "p")
    })
    .expect("paragraph");
    let float = crate::tests::harness::find_box(&frame.doc.root, &|node| {
        node.attributes
            .get("class")
            .is_some_and(|class| class == "float")
    })
    .expect("float");
    let float_left = float.layout.margin_rect.x;

    for line in &para.layout.line_cache {
        assert!(
            line.x + line.width <= float_left + 0.05,
            "line must not overlap right float: line={:?}, float_left={float_left}",
            line
        );
    }
}

#[test]
fn disclosure_open_and_closed_markers_paint_distinct_glyphs() {
    let (_, list) = build(
        r#"
        <style>
        #closed { list-style-type: disclosure-closed; }
        #open { list-style-type: disclosure-open; }
        </style>
        <ul><li id="closed">closed</li><li id="open">open</li></ul>
    "#,
    );

    let markers: Vec<&str> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::ListMarker { text, .. } if text == "\u{25b8}" || text == "\u{25be}" => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect();

    assert!(
        markers.contains(&"\u{25b8}") && markers.contains(&"\u{25be}"),
        "open and closed disclosure markers should paint different glyphs; got {:?}",
        markers
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
                        0, 0, 0, 0, // right half transparent by alpha
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
fn mask_layer_uses_alpha_for_black_svg_icons() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushMask {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: ImageRef::Owned(vec![0, 0, 0, 255], 1, 1),
            },
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::rgba(32, 33, 34, 255),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PopMask,
        ],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(12, 12).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    assert!(
        data[(5 * 12 + 5) * 4 + 3] > 200,
        "opaque black mask pixels should reveal the masked fill"
    );
}

#[test]
fn border_image_paints_only_the_border_ring() {
    let list = DisplayList {
        commands: vec![PaintCmd::BorderImage {
            rect: Rect::new(2.0, 2.0, 20.0, 20.0),
            widths: [4.0, 4.0, 4.0, 4.0],
            slices: [1.0, 1.0, 1.0, 1.0],
            repeat_x_mode: 0,
            repeat_y_mode: 0,
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
fn border_image_repeat_tiles_edge_segments() {
    let mut data = vec![0; 5 * 3 * 4];
    let top_middle_red = (1usize) * 4;
    data[top_middle_red] = 255;
    data[top_middle_red + 3] = 255;
    let list = DisplayList {
        commands: vec![PaintCmd::BorderImage {
            rect: Rect::new(0.0, 0.0, 11.0, 3.0),
            widths: [1.0, 1.0, 0.0, 1.0],
            slices: [1.0, 1.0, 1.0, 1.0],
            repeat_x_mode: 1,
            repeat_y_mode: 0,
            fill_center: false,
            data: ImageRef::Owned(data, 5, 3),
        }],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(12, 4).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let pixels = pixmap.data();
    let red_at = |x: usize, y: usize| pixels[(y * 12 + x) * 4];

    assert!(
        red_at(7, 0) > 200,
        "repeat should restart the top edge source pattern across the border"
    );
}

#[test]
fn border_image_repeat_modes_reach_display_list() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <div style="width:40px;height:20px;border:4px solid transparent;
                         border-image-source:url('data:image/svg+xml,%3Csvg%20viewBox=%220%200%203%203%22%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3Crect%20width=%223%22%20height=%223%22%20fill=%22red%22/%3E%3C/svg%3E');
                         border-image-slice:1;
                         border-image-repeat:round space"></div>
           </body>"#,
    );

    let border_image = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BorderImage {
                repeat_x_mode,
                repeat_y_mode,
                ..
            } => Some((*repeat_x_mode, *repeat_y_mode)),
            _ => None,
        })
        .expect("expected border-image display command");

    assert_eq!(border_image, (3, 2));
}

#[test]
fn border_image_width_and_outset_reach_display_list_geometry() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <div style="width:40px;height:20px;border:2px solid transparent;
                         border-image-source:url('data:image/svg+xml,%3Csvg%20viewBox=%220%200%203%203%22%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3Crect%20width=%223%22%20height=%223%22%20fill=%22red%22/%3E%3C/svg%3E');
                         border-image-slice:1;
                         border-image-width:8px 6px 4px 2px;
                         border-image-outset:3px 5px"></div>
           </body>"#,
    );

    let (rect, widths) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BorderImage { rect, widths, .. } => Some((*rect, *widths)),
            _ => None,
        })
        .expect("expected border-image display command");

    assert_eq!(widths, [8.0, 6.0, 4.0, 2.0]);
    assert_eq!(rect.x, -5.0);
    assert_eq!(rect.y, -3.0);
    assert_eq!(rect.w, 54.0);
    assert_eq!(rect.h, 30.0);
}

#[test]
fn gradient_border_image_does_not_fall_back_to_black_box_border() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <a style="display:block;width:100px;height:20px;color:black;
                       border-style:solid;
                       border-top-width:2px;
                       border-bottom-width:2px;
                       border-image-slice:2;
                       border-image-source:linear-gradient(90deg, transparent 10px, #ededf0 10px, #ededf0 90px, transparent 90px)">
               row
             </a>
           </body>"#,
    );

    assert!(
        list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::FillRect { rect, color, .. }
                    if rect.h == 2.0 && color.r == 237 && color.g == 237 && color.b == 240
            )
        }),
        "gradient border-image should paint separator strips"
    );
    assert!(
        !list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::Border { widths, colors, .. }
                    if (widths[1] > 0.0 || widths[3] > 0.0)
                        && colors.iter().any(|color| *color == Color::BLACK)
            )
        }),
        "gradient border-image must not fall back to black side borders"
    );
}

#[test]
fn zero_width_border_sides_do_not_paint_rectangles() {
    let list = DisplayList {
        commands: vec![PaintCmd::Border {
            rect: Rect::new(2.0, 2.0, 20.0, 20.0),
            widths: [1.0, 0.0, 0.0, 0.0],
            colors: [
                Color::rgba(0, 200, 0, 255),
                Color::rgba(0, 0, 0, 255),
                Color::rgba(0, 0, 0, 255),
                Color::rgba(0, 0, 0, 255),
            ],
            styles: [1, 1, 1, 1],
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            opacity: 1.0,
        }],
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(28, 28).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let data = pixmap.data();
    let pixel = |x: usize, y: usize| {
        let i = (y * 28 + x) * 4;
        (data[i], data[i + 1], data[i + 2], data[i + 3])
    };

    assert!(pixel(10, 2).1 > 150, "the non-zero top border should paint");
    assert_eq!(
        pixel(21, 10).3,
        0,
        "the zero-width right border must not paint its black color"
    );
    assert_eq!(
        pixel(10, 21).3,
        0,
        "the zero-width bottom border must not paint its black color"
    );
    assert_eq!(
        pixel(2, 10).3,
        0,
        "the zero-width left border must not paint its black color"
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

#[test]
fn test_inline_element_background_does_not_paint_over_text() {
    let html = r##"<!DOCTYPE html>
    <html>
    <head>
    <style>
    :root {
        --tertiary-bg-color: #811818;
        --tertiary-text-color: #ffffff;
    }
    .gallery-item-title, .gallery-item-title a {
        font-size: 1.2em;
        color: var(--tertiary-text-color);
        background-color: var(--tertiary-bg-color);
        text-decoration: none;
    }
    </style>
    </head>
    <body>
    <div class="gallery-item-title"><a href="#">Poet</a></div>
    </body>
    </html>"##;
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 400.0, 300.0);
    f.update_frame();
    let list = build_display_list(&f.doc.root, 400.0, 300.0);
    let fill_rect_count = list
        .commands
        .iter()
        .filter(|cmd| matches!(cmd, PaintCmd::FillRect { .. }))
        .count();
    assert_eq!(
        fill_rect_count, 2,
        "only parent block and inline run backgrounds should be painted"
    );

    let mut pm = tiny_skia::Pixmap::new(400, 300).unwrap();
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut f.doc, &mut pm, 1.0);

    // Verify there are white text pixels (Poet) inside the red box at (8..60, 8..36)
    let mut found_white_pixel = false;
    for y in 8..36 {
        for x in 8..60 {
            let p = pm.pixel(x, y).unwrap();
            if p.red() > 200 && p.green() > 200 && p.blue() > 200 {
                found_white_pixel = true;
                break;
            }
        }
        if found_white_pixel {
            break;
        }
    }
    assert!(
        found_white_pixel,
        "Expected white text pixels for 'Poet' inside the red box, but found none!"
    );
}
