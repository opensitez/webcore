//! Tests for the display list builder and replay.

use crate::Renderer;
use crate::frame::EngineFrame;
use crate::html::{parse_html, parse_html_with_base};
use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd, PlaceholderTypography};
use crate::renderer::display_list_builder::{
    build_display_list, build_display_list_full, build_display_list_full_with_font_system,
};
use crate::renderer::display_list_replay::{
    reduce_corner_radii, replay, replay_tile_with_scroll_and_transform_overrides,
    replay_with_scroll, replay_with_scroll_and_transform_overrides, replay_with_text,
};
use crate::types::{Color, Rect, TextTransform};

fn build(html: &str) -> (EngineFrame, DisplayList) {
    let doc = parse_html(html);
    let mut f = EngineFrame::new(doc, 800.0, 600.0);
    f.update_frame();
    let list = build_display_list(&f.doc.root, 800.0, 600.0);
    (f, list)
}

#[test]
fn positioned_inline_icon_backing_uses_own_font_box_not_descendant_bounds() {
    let mut renderer = Renderer::new();
    let (ascent, descent, _) =
        crate::layout::inline_layout::font_metrics(Some(&mut renderer.font_system), "Arial", 20.0);
    for (child, line_height) in [
        ("<span style='font-size:10px'>F</span>", 8),
        ("<span style='font-size:20px'>F</span>", 20),
        (
            "<svg width='20' height='20' style='position:relative;top:1px'><rect width='20' height='20'/></svg>",
            20,
        ),
        (
            "<span style='display:inline-block;width:20px;height:40px'></span>",
            36,
        ),
    ] {
        let markup = format!(
            "<style>body{{margin:0;font:20px/{line_height}px Arial}}#icon{{position:relative}}#back{{position:absolute;top:4px;left:8px;width:12.8px;height:15.2px;background:white}}</style><div id='line'><i id='icon'>{child}<span id='back'></span></i></div>"
        );
        let doc = renderer.load_html(&markup, 200.0);
        let icon = doc
            .get_box_by_id(doc.get_element_by_id("icon").unwrap())
            .unwrap();
        let back = doc
            .get_box_by_id(doc.get_element_by_id("back").unwrap())
            .unwrap();
        let line = doc
            .get_box_by_id(doc.get_element_by_id("line").unwrap())
            .unwrap();
        let baseline = line.layout.line_cache[0].y + line.layout.line_cache[0].ascent;
        assert!(
            (icon.layout.content_rect.h - ascent - descent).abs() < 0.01,
            "inline font box must not adopt child's height: {child} {:?}",
            icon.layout.content_rect
        );
        assert!(
            (icon.layout.content_rect.y - (baseline - ascent)).abs() < 0.01,
            "inline font box must use its own baseline: {child} {:?}",
            icon.layout.content_rect
        );
        assert!((back.layout.border_rect.y - icon.layout.padding_rect.y - 4.0).abs() < 0.01);
        assert!((back.layout.border_rect.x - icon.layout.padding_rect.x - 8.0).abs() < 0.01);
    }
}

#[test]
fn native_text_selection_uses_cascade_and_paints_at_both_scales() {
    for (tag, value) in [
        ("input", "abc"),
        ("textarea", "abc\ndef"),
        ("textarea", "مرحبا"),
    ] {
        let markup = if tag == "input" {
            format!("<input id=control value='{value}'>")
        } else {
            format!("<textarea id=control>{value}</textarea>")
        };
        let (mut frame, _) = build(&format!(
            "<style>body{{margin:0}}input,textarea{{font:20px/26px Arial;width:180px;height:80px;color:black;background:white}}input::selection,textarea::selection{{background:#00ff00;color:#ff0000}}</style>{markup}"
        ));
        let id = frame.doc.get_element_by_id("control").unwrap();
        let node = frame.doc.get_box_by_id_mut(id).unwrap();
        node.input_sel_anchor = 0;
        node.input_cursor = value.chars().count();
        let list = build_display_list(&frame.doc.root, 800.0, 600.0);
        let selection = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::FormElement {
                    node_id,
                    text_selection,
                    ..
                } if *node_id == id => text_selection.as_ref(),
                _ => None,
            })
            .expect("live native selection must be recorded");
        assert_eq!(selection.range, 0..value.chars().count());
        assert_eq!(selection.background, Color::rgb(0, 255, 0));
        assert_eq!(selection.foreground, Color::rgb(255, 0, 0));
        let mut fonts = cosmic_text::FontSystem::new();
        let mut cache = cosmic_text::SwashCache::new();
        for scale in [1.0, 2.0] {
            let mut pixels =
                tiny_skia::Pixmap::new((240.0 * scale) as u32, (100.0 * scale) as u32).unwrap();
            replay_with_text(&list, &mut pixels, scale, &mut fonts, &mut cache);
            assert!(
                pixels
                    .data()
                    .chunks_exact(4)
                    .any(|px| px == [0, 255, 0, 255]),
                "{tag} selection background at {scale}x"
            );
            assert!(
                pixels
                    .data()
                    .chunks_exact(4)
                    .any(|px| px[0] > 128 && px[1] < 64 && px[2] < 64),
                "{tag} selected glyph foreground at {scale}x"
            );
        }
    }
}

#[test]
fn empty_positioned_bullet_and_wrapped_link_border() {
    for direction in ["ltr", "rtl"] {
        let (_, list) = build(&format!(
            "<style>body{{margin:40px;direction:{direction}}}li{{position:relative;width:140px;list-style:none;font:16px/26px Arial}}li::before{{content:'';position:absolute;top:8px;inset-inline-start:-16px;border:3px solid #3f3f42;border-radius:50%;background:#3f3f42}}a{{color:#222;border-bottom:1px solid #b80000;text-decoration:none}}</style><ul><li><a>Several words make this link wrap across lines</a></li></ul>"
        ));
        let bullet = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::Border {
                    rect,
                    widths,
                    colors,
                    radii,
                    radii_y,
                    ..
                } if widths[0] == 3.0 && colors[0].r == 63 => {
                    assert_eq!(*radii, [3.0; 4], "bullet horizontal radii");
                    assert_eq!(*radii_y, [3.0; 4], "bullet vertical radii");
                    Some(rect)
                }
                _ => None,
            })
            .expect("bullet border");
        assert!(
            (bullet.w - 6.0).abs() < 0.1 && (bullet.h - 6.0).abs() < 0.1,
            "{direction}: empty bullet must not stretch: {bullet:?}"
        );
        let borders: Vec<_> = list
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                PaintCmd::Border {
                    rect,
                    widths,
                    colors,
                    ..
                } if widths[2] == 1.0 && colors[2].r == 184 => Some(rect),
                _ => None,
            })
            .collect();
        assert!(
            borders.len() >= 2,
            "{direction}: each wrapped line needs its red border: {borders:?}"
        );
        assert!(
            borders.iter().all(|r| r.h <= 27.1 && r.w <= 141.0),
            "{direction}: borders must follow individual fragments: {borders:?}"
        );
    }
}

#[test]
fn auto_z_positioned_content_paints_after_earlier_fixed_layer() {
    let (_, list) = build(
        "<style>body{margin:0}.overlay{position:fixed;inset:0;background:#fff}.row{position:relative}</style><div class=overlay></div><div class=row>Message subject</div>",
    );
    let fixed_end = list
        .commands
        .iter()
        .position(|cmd| matches!(cmd, PaintCmd::EndFixedPosition))
        .expect("fixed layer");
    let subject = list
        .commands
        .iter()
        .position(
            |cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.contains("Message subject")),
        )
        .expect("positioned row text");
    assert!(
        fixed_end < subject,
        "later positioned content must paint over the fixed layer"
    );
}

#[test]
fn indented_inline_text_paints_at_the_collapsed_space_position() {
    fn word_x(html: &str) -> f32 {
        let (_, list) = build(html);
        list.commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::Text { text, x, .. } if text == "followers" => Some(*x),
                _ => None,
            })
            .expect("followers text command")
    }

    let compact = word_x(
        "<style>body{margin:0;font:14px Arial}</style><div><a><span>6</span> followers</a></div>",
    );
    let indented = word_x(
        "<style>body{margin:0;font:14px Arial}</style><div><a><span>6</span>\n          followers</a></div>",
    );
    assert!(
        (compact - indented).abs() < 1.0,
        "indentation shifted paint from {compact} to {indented}"
    );
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
    let (frame, list) = build_full(
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
        .unwrap_or_else(|| panic!(
            "positioned block ::before generated content should paint as text: {:?}; pseudo={:?}",
            list.commands,
            find_node_by_tag(&frame.doc.root, "::before").map(|n| (
                &n.text, &n.style.rare().content, &n.layout.line_cache, &n.layout.inline_runs
            ))
        ));

    let pseudo = find_node_by_tag(&frame.doc.root, "::before").unwrap();
    let text_w = pseudo
        .layout
        .line_cache
        .first()
        .expect("generated text line")
        .width;
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
fn rtl_atomic_prefix_does_not_shift_multiple_words_outside_line() {
    let (frame, list) = build_full(
        r#"<style>body{margin:0}#line{direction:rtl;width:320px;font:16px/26px Arial}#icon{display:inline-block;width:30px;height:20px}</style><div id=line><span id=icon></span>ليكون أول نوع جديد من السنانير منذ مئة سنة</div>"#,
    );
    let node = crate::dom::query_selector(&frame.doc.root, "#line").unwrap();
    let line = &node.layout.line_cache[0];
    assert!(
        line.text_x_offset.abs() < 0.1,
        "all words, not just the first, determine the RTL origin: {line:?}"
    );
    for command in &list.commands {
        if let PaintCmd::Text { x, text, .. } = command {
            assert!(
                *x >= line.x - 0.1 && *x < 320.0,
                "text outside RTL line: {text} at {x}"
            );
        }
    }
}

#[test]
fn rtl_list_bullets_paint_on_inline_start_side() {
    for kind in ["disc", "circle", "square"] {
        for direction in ["ltr", "rtl"] {
            let (frame, list) = build(&format!(
                "<style>body{{margin:40px}}li{{direction:{direction};width:200px;list-style:{kind};font:16px/26px Arial}}</style><ul><li id=item>List item</li></ul>"
            ));
            let item = crate::dom::query_selector(&frame.doc.root, "#item").unwrap();
            let line = &item.layout.line_cache[0];
            let x = list
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    PaintCmd::ListMarker { x, .. } => Some(*x),
                    _ => None,
                })
                .expect("list marker");
            assert!(
                if direction == "rtl" {
                    x > line.x + line.width
                } else {
                    x < line.x
                },
                "{direction} {kind}: marker {x} must be on inline-start side of {line:?}"
            );
        }
    }
}

#[test]
fn outside_text_markers_do_not_overlap_list_content_in_either_direction() {
    for direction in ["ltr", "rtl"] {
        for custom in ["", "li::marker{content:'LONG ';color:red}"] {
            let (frame, list) = build(&format!(
                "<style>body{{margin:80px}}ol{{margin:0;padding:0}}li{{direction:{direction};width:200px;font:16px/26px Arial}}li::marker{{color:red}}{custom}</style><ol start=123><li id=item>Item text</li></ol>"
            ));
            let item = crate::dom::query_selector(&frame.doc.root, "#item").unwrap();
            let mut pixmap = tiny_skia::Pixmap::new(400, 160).unwrap();
            let mut fonts = cosmic_text::FontSystem::new();
            let mut cache = cosmic_text::SwashCache::new();
            replay_with_text(&list, &mut pixmap, 1.0, &mut fonts, &mut cache);
            let xs: Vec<_> = pixmap
                .data()
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, p)| p[0] > 0 && p[1] == 0 && p[2] == 0 && p[3] > 0)
                .map(|(i, _)| i % 400)
                .collect();
            assert!(!xs.is_empty(), "{direction} marker must paint");
            assert!(
                if direction == "rtl" {
                    *xs.iter().min().unwrap() as f32 >= item.layout.content_rect.right()
                } else {
                    (*xs.iter().max().unwrap() as f32) < item.layout.content_rect.x
                },
                "{direction} {custom}: marker {:?}..{:?} overlaps {:?}",
                xs.iter().min(),
                xs.iter().max(),
                item.layout.content_rect
            );
        }
    }
}

#[test]
fn list_image_markers_follow_direction_and_position() {
    for rtl in [false, true] {
        for inside in [false, true] {
            let direction = if rtl { "rtl" } else { "ltr" };
            let position = if inside { "inside" } else { "outside" };
            let list = list_marker_with_example_base(&format!(
                "<style>*{{margin:0;padding:0}}body{{margin:100px}}li{{width:200px;direction:{direction};list-style-position:{position};list-style-image:url(silicon.png)}}</style><ul><li>item</li></ul>"
            ));
            let (x, width) = list
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    PaintCmd::ListMarker {
                        marker_type: 4,
                        x,
                        image: Some(image),
                        ..
                    } => {
                        let width = match image {
                            ImageRef::Owned(_, w, _) | ImageRef::Shared(_, w, _) => *w as f32,
                        };
                        Some((*x, width))
                    }
                    _ => None,
                })
                .expect("decoded marker");
            let expected = if rtl {
                300.0 - if inside { width } else { 0.0 }
            } else {
                100.0 - if inside { 0.0 } else { width }
            };
            assert!(
                (x - expected).abs() < 0.1,
                "{direction} {position}: {x} != {expected}"
            );
        }
    }
}

#[test]
fn closed_disclosure_marker_points_toward_inline_end() {
    for (direction, expected) in [("ltr", "\u{25b8}"), ("rtl", "\u{25c2}")] {
        let (_, list) = build(&format!(
            "<li style='direction:{direction};list-style-type:disclosure-closed'>Item</li>"
        ));
        assert!(list.commands.iter().any(|cmd| matches!(cmd,
            PaintCmd::ListMarker { text, .. } if text == expected
        )));
    }
}

#[test]
fn predefined_numeric_list_markers_paint_local_digits() {
    for (style, marker) in [
        ("arabic-indic", "١٢."),
        ("persian", "۱۲."),
        ("cjk-decimal", "一二、"),
        ("lower-armenian", "ժբ."),
    ] {
        let (_, list) = build(&format!(
            "<ol style='list-style-type:{style}' start='12'><li>Item</li></ol>"
        ));
        assert!(
            list.commands.iter().any(|cmd| matches!(cmd,
                PaintCmd::ListMarker { text, .. } if text == marker
            )),
            "{style} should paint {marker}"
        );
    }
}

#[test]
fn text_controls_preserve_authored_color_even_when_background_matches() {
    for markup in ["<input value='ABC'>", "<textarea>ABC</textarea>"] {
        for color in ["black", "white", "#345678"] {
            let (frame, list) = build(&format!(
                "<style>input,textarea{{appearance:none;color:{color};background:{color};width:200px;height:30px;border:0;padding:0}}</style>{markup}"
            ));
            let node = crate::dom::query_selector(&frame.doc.root, "input,textarea").unwrap();
            let painted = list
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    PaintCmd::FormElement { color, .. } => Some(*color),
                    _ => None,
                })
                .expect("form paint command");
            assert_eq!(
                painted, node.style.color,
                "paint must use the cascaded color, not a contrast heuristic: {markup} {color}"
            );
        }
    }
}

#[test]
fn text_input_values_and_placeholders_respect_rtl_start_alignment() {
    for attribute in ["value", "placeholder"] {
        for direction in ["ltr", "rtl"] {
            let (_, list) = build(&format!(
                "<style>body{{margin:0}}input{{direction:{direction};text-align:start;appearance:none;width:200px;height:30px;padding:0;border:0;color:red;background:transparent}}input::placeholder{{color:red}}</style><input {attribute}='ABC'>"
            ));
            let mut pixmap = tiny_skia::Pixmap::new(220, 40).unwrap();
            let mut fonts = cosmic_text::FontSystem::new();
            let mut cache = cosmic_text::SwashCache::new();
            replay_with_text(&list, &mut pixmap, 1.0, &mut fonts, &mut cache);
            let xs: Vec<_> = pixmap
                .data()
                .chunks_exact(4)
                .enumerate()
                .filter(|(_, p)| p[0] > 0 && p[1] == 0 && p[2] == 0 && p[3] > 0)
                .map(|(i, _)| i % 220)
                .collect();
            assert!(!xs.is_empty(), "{attribute} must paint");
            assert!(
                if direction == "rtl" {
                    *xs.iter().min().unwrap() > 150
                } else {
                    *xs.iter().max().unwrap() < 50
                },
                "{attribute} {direction} wrong text bounds: {:?}..{:?}",
                xs.iter().min(),
                xs.iter().max()
            );
        }
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
            PaintCmd::Text { text, .. } if text == "0:00 / 1:15"
        )),
        "video controls should paint the current time and media duration"
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
            PaintCmd::Text { text, .. } if text == "0:00 / 0:09"
        )),
        "audio controls should paint the current time and media duration"
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
fn inline_svg_live_paint_server_and_variable_mutations_invalidate_pixels() {
    let (mut frame, _) = build(
        r##"<svg id="icon" width="20" height="20" style="--paint:red">
          <defs>
            <linearGradient id="first"><stop stop-color="var(--paint)"/><stop offset="1" stop-color="var(--paint)"/></linearGradient>
            <linearGradient id="second"><stop stop-color="blue"/><stop offset="1" stop-color="blue"/></linearGradient>
          </defs>
          <rect id="shape" width="20" height="20" style="fill:url(#first)"/>
        </svg>"##,
    );
    let shape = frame.doc.get_element_by_id("shape").unwrap();
    let icon = frame.doc.get_element_by_id("icon").unwrap();
    for (target, property, value, expected) in [
        (icon, "style", "--paint:red", [255, 0, 0, 255]),
        (shape, "style", "fill:url(#second)", [0, 0, 255, 255]),
        (shape, "style", "fill:url(#first)", [255, 0, 0, 255]),
        (icon, "style", "--paint:lime", [0, 255, 0, 255]),
        (shape, "style", "fill:url(#missing) blue", [0, 0, 255, 255]),
        (shape, "style", "fill:url(#missing)", [0, 0, 0, 0]),
    ] {
        frame.doc.set_attribute(target, property, value);
        frame.update_frame();
        let list = build_display_list(&frame.doc.root, 800.0, 600.0);
        let (data, width, _) = first_image_data(&list).expect("mutated SVG raster");
        let offset = ((10 * width + 10) * 4) as usize;
        assert_eq!(&data[offset..offset + 4], &expected, "mutation {value}");
        let repeat = build_display_list(&frame.doc.root, 800.0, 600.0);
        let (again, _, _) = first_image_data(&repeat).unwrap();
        assert_eq!(data, again, "cached repeat after mutation {value}");
    }
}

#[test]
fn inline_svg_css_stroke_servers_inherit_and_change_through_dom() {
    let (mut frame, _) = build(
        r##"<style>#outline { fill:none; stroke:url(#paint); stroke-width:4 }</style>
        <svg width="20" height="20">
          <defs><linearGradient id="paint"><stop stop-color="red"/><stop offset="1" stop-color="red"/></linearGradient></defs>
          <g id="outline"><rect id="shape" x="2" y="2" width="16" height="16"/></g>
        </svg>"##,
    );
    let outline = frame.doc.get_element_by_id("outline").unwrap();
    let shape = frame.doc.get_element_by_id("shape").unwrap();
    assert_eq!(
        frame.doc.computed_style_property(shape, "stroke"),
        "url(#paint)"
    );
    for (value, expected) in [
        ("stroke:url(#paint)", [255, 0, 0, 255]),
        ("stroke:blue", [0, 0, 255, 255]),
        ("stroke:url(#missing) lime", [0, 255, 0, 255]),
        ("stroke:currentColor;color:red", [255, 0, 0, 255]),
        ("stroke:none", [0, 0, 0, 0]),
    ] {
        frame.doc.set_attribute(outline, "style", value);
        frame.update_frame();
        let list = build_display_list(&frame.doc.root, 800.0, 600.0);
        let (data, width, _) = first_image_data(&list).unwrap();
        let offset = ((2 * width + 10) * 4) as usize;
        assert_eq!(
            &data[offset..offset + 4],
            &expected,
            "stroke mutation {value}"
        );
        let center = ((10 * width + 10) * 4) as usize;
        assert_eq!(data[center + 3], 0, "fill:none stays transparent");
    }
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
fn inline_svg_paints_later_group_with_inherited_fill() {
    let standalone = crate::svg::rasterize_svg_to_rgba(
        r##"<svg width="93" height="20" viewBox="0 0 93 20"><g><path d="M0 0H29V20H0Z" fill="#ff0033"/></g><g id="wordmark"><path d="M31.1484 2.09994H40.6084V18.8999H31.1484Z"/></g></svg>"##,
        93,
        20,
    )
    .expect("standalone SVG image");
    let standalone_pixel = (10 * 93 + 35) * 4;
    assert!(
        standalone[standalone_pixel + 3] > 200,
        "standalone second group should paint"
    );
    let (_, list) = build(
        r##"<div style="color:black;fill:currentcolor">
            <svg width="93" height="20" viewBox="0 0 93 20">
                <g><path d="M0 0H29V20H0Z" fill="#ff0033"/></g>
                <g id="wordmark"><path d="M31.1484 2.09994H40.6084V18.8999H31.1484Z"/></g>
            </svg>
        </div>"##,
    );
    let (data, width, _) = first_image_data(&list).expect("inline SVG image");
    let pixel = ((10 * width + 35) * 4) as usize;
    assert!(
        data[pixel] < 30 && data[pixel + 1] < 30 && data[pixel + 2] < 30 && data[pixel + 3] > 200,
        "second group should inherit black fill, got {:?}",
        &data[pixel..pixel + 4]
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
fn inline_svg_use_resolves_symbols_in_other_inline_svg() {
    let (_, list) = build(
        r##"<svg style="display:none"><defs>
        <symbol id="shared" viewBox="0 0 12 12"><rect width="12" height="12"/></symbol>
        </defs></svg><svg style="width:12px;height:12px;fill:red"><use href="#shared"/></svg>"##,
    );
    let (data, w, h) = first_image_data(&list).expect("visible SVG");
    assert_eq!((w, h), (12, 12));
    assert!(
        data.chunks_exact(4)
            .any(|pixel| pixel[0] > 200 && pixel[3] > 200)
    );
}

#[test]
fn inline_svg_css_zero_stroke_width_suppresses_inherited_outline() {
    for width in ["0", "0px", "calc(1px - 1px)"] {
        let (_, list) = build(&format!(
            "<style>svg{{stroke:red;stroke-width:{width};fill:none;width:24px;height:24px}}</style>\
             <svg viewBox='0 0 24 24'><circle cx='12' cy='12' r='8'/></svg>"
        ));
        let (data, _, _) = first_image_data(&list).expect("SVG raster");
        assert!(data.chunks_exact(4).all(|px| px[3] == 0), "{width}");
    }
}

#[test]
fn inline_svg_root_stroke_current_color_survives_dom_style() {
    let (_, list) = build(
        r#"<style>svg.search { color: rgb(164, 206, 254); }</style>
           <svg class="search" style="width:24px;height:24px" viewBox="0 0 24 24"
                fill="none" stroke="currentColor" stroke-width="2"
                stroke-linecap="round" stroke-linejoin="round">
             <circle cx="11" cy="11" r="8"/>
             <path d="m21 21-4.3-4.3"/>
           </svg>"#,
    );

    let (data, w, h) =
        first_image_data(&list).expect("inline SVG should rasterize to an image command");
    assert_eq!((w, h), (24, 24));
    let painted = data.chunks_exact(4).filter(|px| px[3] > 0).count();
    assert!(
        painted > 20,
        "stroke-only currentColor SVG should not rasterize transparent; painted={painted}"
    );
}

#[test]
fn inline_external_after_icon_sits_after_text() {
    let (frame, _list) = build(
        r#"<style>
             body { margin: 0; font: 16px/24px sans-serif; }
             a.external::after {
               content: "";
               display: inline-block;
               width: 16px;
               height: 16px;
               margin-left: 2px;
               background: currentColor;
             }
           </style>
           <a class="external">Fix internal links</a>"#,
    );

    let link = find_node_by_tag(&frame.doc.root, "a").expect("external link");
    let icon = link
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("materialized external-link pseudo icon");
    let text = link
        .children
        .iter()
        .find(|child| child.is_text_node())
        .expect("link text");
    let text_right = text.layout.margin_rect.x + text.layout.margin_rect.w;
    assert!(
        icon.layout.border_rect.x >= text_right + 1.0,
        "external-link icon paint box should be positioned after text; text={:?} icon={:?}",
        text.layout.margin_rect,
        icon.layout.margin_rect
    );
}

#[test]
fn relative_inline_external_after_icon_sits_after_text() {
    let (frame, _list) = build(
        r#"<style>
             body { margin: 0; font: 16px/24px sans-serif; }
             a.external { position: relative; padding-right: 18px; }
             a.external::after {
               content: "\200b" / "(external)";
               position: relative;
               top: 2px;
               display: inline-block;
               width: 16px;
               height: 16px;
               margin-left: 2px;
               background: currentColor;
             }
           </style>
           <a class="external">Fix internal links</a>"#,
    );

    let link = find_node_by_tag(&frame.doc.root, "a").expect("external link");
    let icon = link
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("materialized external-link pseudo icon");
    let text = link
        .children
        .iter()
        .find(|child| child.is_text_node())
        .expect("link text");
    let text_right = text.layout.margin_rect.x + text.layout.margin_rect.w;
    assert!(
        matches!(icon.style.display, crate::types::Display::InlineBlock),
        "relative inline pseudo should not be blockified"
    );
    assert!(
        icon.layout.border_rect.x >= text_right + 1.0,
        "relative external-link icon should be positioned after text; text={:?} icon={:?}",
        text.layout.margin_rect,
        icon.layout.margin_rect
    );
}

#[test]
fn absolute_inline_external_after_icon_uses_static_inline_position() {
    let (frame, _list) = build(
        r#"<style>
             body { margin: 0; font: 16px/24px sans-serif; }
             a.external { position: relative; padding-right: 18px; }
             a.external::after {
               content: "\200b" / "(external)";
               position: absolute;
               display: inline-block;
               width: 16px;
               height: 16px;
               margin-left: 2px;
               background: currentColor;
             }
           </style>
           <a class="external">Fix internal links</a>"#,
    );

    let link = find_node_by_tag(&frame.doc.root, "a").expect("external link");
    let icon = link
        .children
        .iter()
        .find(|child| child.tag == "::after")
        .expect("materialized external-link pseudo icon");
    let text = link
        .children
        .iter()
        .find(|child| child.is_text_node())
        .expect("link text");
    let text_right = text.layout.margin_rect.x + text.layout.margin_rect.w;
    assert!(
        icon.layout.border_rect.x >= text_right + 1.0,
        "absolute external-link icon should use inline static position after text; text={:?} icon={:?}",
        text.layout.margin_rect,
        icon.layout.margin_rect
    );
}

#[test]
fn inline_svg_uses_css_animated_fill_when_rasterized() {
    let mut renderer = Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
             body { margin: 0; }
             @keyframes icon-fill { from { fill: rgb(255, 0, 0); } to { fill: rgb(0, 0, 255); } }
             svg { fill: rgb(0, 0, 255); animation: icon-fill 10s linear; }
           </style>
           <svg style="width:20px;height:20px" viewBox="0 0 20 20">
             <rect x="0" y="0" width="20" height="20"/>
           </svg>"#,
        20.0,
    );
    assert!(
        !doc.animation_overrides.is_empty(),
        "CSS animation should be sampled"
    );
    let mut pixmap = tiny_skia::Pixmap::new(20, 20).unwrap();
    renderer.render(&mut doc, &mut pixmap, 1.0);
    let data = pixmap.data();
    let idx = ((10 * pixmap.width() + 10) * 4) as usize;
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
fn inline_svg_mixed_text_chunk_preserves_computed_center_and_end_alignment() {
    fn bounds(data: &[u8], width: u32) -> (usize, usize, usize, usize) {
        let mut result: Option<(usize, usize, usize, usize)> = None;
        for (index, pixel) in data.chunks_exact(4).enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let (x, y) = (index % width as usize, index / width as usize);
            result = Some(match result {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => {
                    (left.min(x), top.min(y), right.max(x), bottom.max(y))
                }
            });
        }
        result.expect("SVG text must paint")
    }

    for (alignment, expected) in [
        ("center", crate::types::TextAlign::Center),
        ("end", crate::types::TextAlign::End),
    ] {
        let render = |content: &str| {
            build(&format!(
                "<style>svg text {{text-align:{alignment};font:24px monospace;fill:black}}</style>
                <svg width='180' height='60' viewBox='0 0 180 60'>
                    <text x='100' y='40'>{content}</text>
                </svg>"
            ))
        };
        let (frame, mixed) = render("A<tspan id='mixed-span'>AA</tspan>A");
        let span = frame
            .doc
            .get_box_by_id(frame.doc.get_element_by_id("mixed-span").unwrap())
            .unwrap();
        assert_eq!(
            span.style.text_align, expected,
            "span must inherit computed {alignment}"
        );
        let (_, flat) = render("AAAA");
        let (mixed_data, mixed_width, mixed_height) = first_image_data(&mixed).unwrap();
        let (flat_data, flat_width, flat_height) = first_image_data(&flat).unwrap();
        assert_eq!((mixed_width, mixed_height), (flat_width, flat_height));
        let mixed_bounds = bounds(mixed_data, mixed_width);
        let flat_bounds = bounds(flat_data, flat_width);
        for (actual, expected) in [
            mixed_bounds.0,
            mixed_bounds.1,
            mixed_bounds.2,
            mixed_bounds.3,
        ]
        .into_iter()
        .zip([flat_bounds.0, flat_bounds.1, flat_bounds.2, flat_bounds.3])
        {
            assert!(
                actual.abs_diff(expected) <= 1,
                "{alignment}: mixed={mixed_bounds:?} flat={flat_bounds:?}"
            );
        }
    }
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
fn image_set_background_uses_selected_candidate_resolution_for_auto_size() {
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
        r#"<style>body{margin:0}</style><div id="box" style="width:250px;height:100px;background-image:image-set(url(one.png) 1x,url(two.png) 2x);background-repeat:no-repeat"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 300.0, 150.0);
    frame.update_frame();
    let node = find_by_id_mut(&mut frame.doc.root, "box").expect("box node");
    let bitmap = std::sync::Arc::new(vec![255; 200 * 100 * 4]);
    assert!(crate::images::set_decoded_bg_image_for_url_on_node(
        node,
        crate::images::DecodedImage::Raster(bitmap, 200, 100),
        "https://example.test/two.png",
        "https://example.test/page",
    ));
    assert_eq!(node.bg_image_resolution, 2.0);

    let list = build_display_list(&frame.doc.root, 300.0, 150.0);
    let (width, height) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BackgroundImage { draw_w, draw_h, .. } => Some((*draw_w, *draw_h)),
            _ => None,
        })
        .expect("background image paint command");
    assert!((width - 100.0).abs() < 0.1, "draw_w={width}");
    assert!((height - 50.0).abs() < 0.1, "draw_h={height}");
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
fn nested_opacity_emits_nested_groups_around_child_fill() {
    let (_, list) = build(
        r#"<style>*{margin:0;padding:0}.outer{position:relative;opacity:.5;width:40px;height:40px}
           .inner{opacity:.5;background:red;width:40px;height:40px}</style>
           <div class="outer"><div class="inner"></div></div>"#,
    );
    let mut depth = 0;
    let mut red_depth = None;
    let mut pushes = 0;
    let mut pops = 0;
    for cmd in &list.commands {
        match cmd {
            PaintCmd::PushOpacity { .. } => {
                depth += 1;
                pushes += 1;
            }
            PaintCmd::PopOpacity => {
                depth -= 1;
                pops += 1;
            }
            PaintCmd::FillRect { color, .. } if color.r > 200 && color.g < 50 => {
                red_depth = Some(depth);
            }
            _ => {}
        }
    }
    assert_eq!((pushes, pops, depth, red_depth), (2, 2, 0, Some(2)));
    let mut pixmap = tiny_skia::Pixmap::new(50, 50).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let pixel = &pixmap.data()[(20 * 50 + 20) * 4..][..4];
    assert!(
        pixel[0] > 0 && pixel[3] > 0,
        "nested opacity should replay its child fill, got {pixel:?}"
    );
    let mut tile = tiny_skia::Pixmap::new(512, 512).unwrap();
    replay_tile_with_scroll_and_transform_overrides(
        &list,
        &mut tile,
        1.0,
        &mut cosmic_text::FontSystem::new(),
        &mut cosmic_text::SwashCache::new(),
        0.0,
        0.0,
        0.0,
        0.0,
        None,
    );
    let tile_pixel = &tile.data()[(20 * 512 + 20) * 4..][..4];
    assert!(
        tile_pixel[0] > 0 && tile_pixel[3] > 0,
        "tile replay should retain nested opacity content, got {tile_pixel:?}"
    );
}

#[test]
fn tile_culling_retains_transformed_and_shadow_spill_pixels() {
    for style in [
        "top:100px;transform:translateY(-30px);background:red",
        "top:100px;box-shadow:0 -30px 0 red",
    ] {
        let (_, list) = build(&format!(
            "<style>body{{margin:0}}div{{position:absolute;left:10px;width:20px;height:20px;{style}}}</style><div></div>"
        ));
        let mut tile = tiny_skia::Pixmap::new(100, 90).unwrap();
        replay_tile_with_scroll_and_transform_overrides(
            &list,
            &mut tile,
            1.0,
            &mut cosmic_text::FontSystem::new(),
            &mut cosmic_text::SwashCache::new(),
            0.0,
            0.0,
            0.0,
            0.0,
            None,
        );
        let pixel = tile.pixel(20, 80).unwrap();
        assert!(
            pixel.red() > 200 && pixel.alpha() > 200,
            "paint spilling into tile lost: {style}: {pixel:?}"
        );
    }
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
fn fixed_backdrop_respects_positioned_z_order_and_viewport_scroll() {
    let (_, list) = build(
        r#"<style>
             body { margin:0; background:white; }
             #backdrop { position:fixed; z-index:1; inset:0; width:100px; height:100px; background:blue; }
             #article { position:relative; z-index:2; width:100px; height:60px; background:red; }
           </style>
           <main><div id="backdrop"></div><section id="article"></section></main>"#,
    );
    let colors: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::FillRect { color, .. } if color.a > 0 => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();
    let blue = colors
        .iter()
        .position(|c| *c == (0, 0, 255))
        .expect("fixed backdrop");
    let red = colors
        .iter()
        .position(|c| *c == (255, 0, 0))
        .expect("article");
    assert!(
        blue < red,
        "fixed backdrop must paint behind higher z-index article"
    );

    let mut fonts = cosmic_text::FontSystem::new();
    let mut cache = cosmic_text::SwashCache::new();
    let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
    replay_with_scroll(&list, &mut pixmap, 1.0, &mut fonts, &mut cache, 0.0, 40.0);
    let pixel = |x: usize, y: usize| {
        let i = (y * 100 + x) * 4;
        let bytes = pixmap.data();
        (bytes[i], bytes[i + 1], bytes[i + 2])
    };
    assert_eq!(pixel(50, 10), (255, 0, 0));
    assert_eq!(pixel(50, 80), (0, 0, 255));

    let (_, overlay) = build(
        r#"<style>
             body { margin:0; }
             #article { position:relative; z-index:2; width:100px; height:60px; background:red; }
             #overlay { position:fixed; z-index:3; inset:0; width:100px; height:100px; background:blue; }
           </style>
           <main><section id="article"></section><div id="overlay"></div></main>"#,
    );
    let colors: Vec<_> = overlay
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::FillRect { color, .. } if color.a > 0 => Some((color.r, color.g, color.b)),
            _ => None,
        })
        .collect();
    let blue = colors
        .iter()
        .position(|c| *c == (0, 0, 255))
        .expect("fixed overlay");
    let red = colors
        .iter()
        .position(|c| *c == (255, 0, 0))
        .expect("article");
    assert!(
        red < blue,
        "higher z-index fixed overlay must paint above article"
    );
}

#[test]
fn fixed_box_rasterizes_at_one_viewport_position_across_tiles() {
    let mut list = DisplayList::new();
    let fill = |rect, color| PaintCmd::FillRect {
        rect,
        color,
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    };
    list.push(fill(
        Rect::new(0.0, 0.0, 128.0, 256.0),
        Color::rgb(0, 0, 255),
    ));
    list.push(PaintCmd::BeginFixedPosition);
    list.push(fill(
        Rect::new(0.0, 0.0, 128.0, 80.0),
        Color::rgb(255, 0, 0),
    ));
    list.push(PaintCmd::EndFixedPosition);

    let mut fonts = cosmic_text::FontSystem::new();
    let mut cache = cosmic_text::SwashCache::new();
    let mut first = tiny_skia::Pixmap::new(128, 128).unwrap();
    let mut second = tiny_skia::Pixmap::new(128, 128).unwrap();
    for (pixmap, tile_y) in [(&mut first, 0.0), (&mut second, 128.0)] {
        replay_tile_with_scroll_and_transform_overrides(
            &list, pixmap, 1.0, &mut fonts, &mut cache, 0.0, tile_y, 0.0, 100.0, None,
        );
    }
    let pixel = |pixmap: &tiny_skia::Pixmap, y: usize| {
        let i = (y * 128 + 64) * 4;
        let bytes = pixmap.data();
        (bytes[i], bytes[i + 1], bytes[i + 2])
    };
    assert_eq!(pixel(&first, 10), (0, 0, 255));
    assert_eq!(pixel(&first, 110), (255, 0, 0));
    assert_eq!(pixel(&second, 10), (255, 0, 0));
    assert_eq!(pixel(&second, 110), (0, 0, 255));
}

#[test]
fn transparent_gradient_stop_keeps_opaque_neighbor_hue() {
    let (_, list) = build(
        r#"<style>
            body { margin: 0; background: white; }
            #fade { width: 100px; height: 30px;
                    background: linear-gradient(90deg, white 20%, transparent); }
           </style><div id="fade"></div>"#,
    );
    let mut pixmap = tiny_skia::Pixmap::new(100, 30).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    replay(&list, &mut pixmap, 1.0);
    let i = (15 * 100 + 70) * 4;
    assert_eq!(&pixmap.data()[i..i + 4], &[255, 255, 255, 255]);
}

#[test]
fn unavailable_icon_font_uses_missing_glyph_square() {
    let render = |content: &str, family: &str| {
        let html = format!(
            "<body style='margin:0'><div style='font-family:{family};font-size:20px;line-height:24px;color:red'>{content}</div></body>"
        );
        let (_, list) = build_full(&html);
        assert!(
            list.commands
                .iter()
                .any(|cmd| matches!(cmd, PaintCmd::Text { .. }))
        );
        let mut pixmap = tiny_skia::Pixmap::new(64, 32).unwrap();
        replay_with_text(
            &list,
            &mut pixmap,
            1.0,
            &mut cosmic_text::FontSystem::new(),
            &mut cosmic_text::SwashCache::new(),
        );
        pixmap.data().to_vec()
    };
    let missing_icon = render("\u{e90d}", "unavailable-icon-face");
    let square = render("□", "sans-serif");
    assert_eq!(missing_icon, square);
    assert!(square.iter().any(|channel| *channel != 0));
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
        matches!(cmd, PaintCmd::PushClip { radius, .. } if radius.iter().all(|r| (*r - (8200.0_f32).sqrt() * 0.25).abs() < 0.01))
    });
    assert!(has_circle_clip, "circle clip should map to a rounded clip");

    let (_, ellipse) = build(
        r#"<style>body{margin:0}</style>
           <div style="width:120px;height:80px;clip-path:ellipse(40px 20px at 50% 50%)">x</div>"#,
    );
    let has_ellipse_clip = ellipse.commands.iter().any(|cmd| {
        matches!(cmd, PaintCmd::PushClip { rect, radius, radius_y }
            if (rect.x - 20.0).abs() < 0.5
                && (rect.y - 20.0).abs() < 0.5
                && (rect.w - 80.0).abs() < 0.5
                && (rect.h - 40.0).abs() < 0.5
                && radius.iter().all(|r| (*r - 40.0).abs() < 0.5)
                && radius_y.iter().all(|r| (*r - 20.0).abs() < 0.5))
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
            .any(|cmd| matches!(cmd, PaintCmd::PushClipPath { points, .. } if points.len() == 3)),
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
fn degenerate_polygon_clip_hides_paint() {
    for shape in [
        "polygon(0 0)",
        "polygon(0 0, 0 0)",
        "polygon(0 0, 100% 100%)",
    ] {
        let (_, list) = build(&format!(
            "<style>body{{margin:0}}</style><div style='width:100px;height:100px;background:red;clip-path:{shape}'></div>"
        ));
        assert!(
            list.commands
                .iter()
                .any(|cmd| matches!(cmd, PaintCmd::PushClipPath { .. })),
            "{shape}"
        );
        let mut pixmap = tiny_skia::Pixmap::new(120, 120).unwrap();
        replay(&list, &mut pixmap, 1.0);
        assert!(
            pixmap.data().chunks_exact(4).all(|pixel| pixel[3] == 0),
            "{shape} must have no visible fill area"
        );
    }
}

#[test]
fn polygon_clip_fill_rule_agrees_between_paint_and_hit_testing() {
    let contour = "0 0, 100% 0, 100% 100%, 0 100%, 0 0, 100% 0, 100% 100%, 0 100%";
    for (rule, visible) in [("nonzero", true), ("evenodd", false)] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!(
                "<style>body{{margin:0}}#shape{{width:100px;height:100px;background:red;clip-path:polygon({rule}, {contour})}}</style><div id=shape></div>"
            ),
            800.0,
        );
        let id = doc.get_element_by_id("shape").unwrap();
        let list = build_display_list(&doc.root, 800.0, 600.0);
        assert!(list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::PushClipPath { even_odd, .. } if *even_odd == (rule == "evenodd")
        )));
        let mut pixmap = tiny_skia::Pixmap::new(120, 120).unwrap();
        replay(&list, &mut pixmap, 1.0);
        let pixel = pixmap.pixel(50, 50).unwrap();
        assert_eq!(
            pixel.red() > 200 && pixel.green() < 30,
            visible,
            "{rule} paint"
        );
        assert_eq!(
            doc.element_from_point(50.0, 50.0) == Some(id),
            visible,
            "{rule} hit"
        );
    }
}

#[test]
fn rounded_polygon_clips_corners_in_paint_and_hit_testing() {
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0}#shape{width:100px;height:100px;background:red;clip-path:polygon(round 20px, 0 0, 100% 0, 100% 100%, 0 100%)}</style><div id=shape></div>",
        800.0,
    );
    let id = doc.get_element_by_id("shape").unwrap();
    let list = build_display_list(&doc.root, 800.0, 600.0);
    let mut pixmap = tiny_skia::Pixmap::new(120, 120).unwrap();
    replay(&list, &mut pixmap, 1.0);
    for (x, y, visible) in [(1, 1, false), (20, 5, true), (50, 50, true)] {
        let pixel = pixmap.pixel(x, y).unwrap();
        assert_eq!(
            pixel.red() > 200 && pixel.green() < 30,
            visible,
            "paint ({x},{y})"
        );
        assert_eq!(
            doc.element_from_point(x as f32, y as f32) == Some(id),
            visible,
            "hit ({x},{y})"
        );
    }
}

#[test]
fn clip_shape_pixels_and_hits_agree_on_non_square_boxes() {
    for (shape, inside, outside) in [
        ("ellipse(80px 20px at 50% 50%)", (100, 50), (40, 35)),
        ("circle(50% at 50% 50%)", (165, 50), (190, 50)),
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(&format!(
            "<html><head><style>body{{margin:0}}#shape{{width:200px;height:100px;background:red;clip-path:{shape}}}</style></head><body><div id=shape></div></body></html>"
        ), 800.0);
        let list = build_display_list(&doc.root, 800.0, 600.0);
        let id = doc.get_element_by_id("shape").unwrap();
        let mut pixmap = tiny_skia::Pixmap::new(200, 100).unwrap();
        replay(&list, &mut pixmap, 1.0);
        for (point, expected) in [(inside, true), (outside, false)] {
            let pixel = &pixmap.data()[(point.1 * 200 + point.0) * 4..][..4];
            assert_eq!(
                pixel[0] > 200 && pixel[1] < 30 && pixel[3] > 200,
                expected,
                "{shape} pixel at {point:?}: {pixel:?}"
            );
            assert_eq!(
                doc.element_from_point(point.0 as f32, point.1 as f32) == Some(id),
                expected,
                "{shape} DOM hit at {point:?}"
            );
            assert_eq!(
                crate::layout::hit_test::hit_test_box_at(
                    &doc.root,
                    (point.0 as f32, point.1 as f32),
                    0
                ) == id,
                expected,
                "{shape} layout hit at {point:?}"
            );
        }
    }
}

#[test]
fn clip_path_geometry_box_clips_paint_and_hits_at_the_same_edges() {
    for clip in [
        "content-box",
        "inset(0) content-box",
        "content-box inset(0)",
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!(
                "<style>body{{margin:0}}#shape{{width:80px;height:80px;padding:10px;border:5px solid blue;background:red;clip-path:{clip}}}</style><div id=shape></div>"
            ),
            800.0,
        );
        let id = doc.get_element_by_id("shape").unwrap();
        let mut pixmap = tiny_skia::Pixmap::new(110, 110).unwrap();
        replay(
            &build_display_list(&doc.root, 800.0, 600.0),
            &mut pixmap,
            1.0,
        );
        for (x, y, visible) in [
            (5, 5, false),
            (14, 50, false),
            (15, 50, true),
            (94, 50, true),
            (95, 50, false),
        ] {
            let pixel = pixmap.pixel(x, y).unwrap();
            assert_eq!(
                pixel.red() > 200 && pixel.green() < 30,
                visible,
                "{clip} paint at ({x},{y})"
            );
            assert_eq!(
                doc.element_from_point(x as f32, y as f32) == Some(id),
                visible,
                "{clip} hit at ({x},{y})"
            );
        }
    }
}

#[test]
fn bare_clip_path_boxes_follow_border_radii_but_explicit_inset_stays_square() {
    for (clip, corner_visible) in [
        ("border-box", false),
        ("inset(0) border-box", true),
        ("padding-box", false),
        ("inset(0) padding-box", true),
        ("content-box", false),
        ("inset(0) content-box", true),
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!("<style>body{{margin:0}}#shape{{position:relative;width:80px;height:80px;padding:10px;border:5px solid transparent;border-radius:30px;clip-path:{clip}}}#child{{position:absolute;inset:0;background:red}}</style><div id=shape><div id=child></div></div>"),
            800.0,
        );
        let child = doc.get_element_by_id("child").unwrap();
        let point = match clip.split_whitespace().last().unwrap() {
            "border-box" => (5, 5),
            "padding-box" => (6, 6),
            _ => (16, 16),
        };
        let mut pixmap = tiny_skia::Pixmap::new(110, 110).unwrap();
        replay(
            &build_display_list(&doc.root, 800.0, 600.0),
            &mut pixmap,
            1.0,
        );
        let painted = pixmap.pixel(point.0, point.1).unwrap().alpha() > 200;
        assert_eq!(painted, corner_visible, "{clip} paint at {point:?}");
        assert_eq!(
            doc.element_from_point(point.0 as f32, point.1 as f32) == Some(child),
            corner_visible,
            "{clip} hit at {point:?}"
        );
    }
}

#[test]
fn rounded_inset_clip_agrees_between_paint_and_hit_testing() {
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0}#shape{width:100px;height:100px;background:red;clip-path:inset(10px round 30px / 15px)}</style><div id=shape></div>",
        800.0,
    );
    let id = doc.get_element_by_id("shape").unwrap();
    let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
    replay(
        &build_display_list(&doc.root, 800.0, 600.0),
        &mut pixmap,
        1.0,
    );
    for (x, y, expected) in [
        (12, 12, false),
        (50, 12, true),
        (12, 50, true),
        (50, 50, true),
    ] {
        assert_eq!(
            pixmap.pixel(x, y).unwrap().alpha() > 200,
            expected,
            "paint at ({x},{y})"
        );
        assert_eq!(
            doc.element_from_point(x as f32, y as f32) == Some(id),
            expected,
            "hit at ({x},{y})"
        );
    }
}

#[test]
fn xywh_and_rect_clips_agree_between_paint_and_hit_testing() {
    for (clip, samples) in [
        (
            "XYWH(10px 20px 40px 30px round 8px)",
            [
                (11, 21, false),
                (30, 22, true),
                (30, 35, true),
                (60, 35, false),
            ],
        ),
        (
            "RECT(20px 50px 50px 10px)",
            [
                (12, 22, true),
                (30, 22, true),
                (30, 35, true),
                (60, 35, false),
            ],
        ),
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!("<style>body{{margin:0}}#shape{{width:100px;height:100px;background:red;clip-path:{clip}}}</style><div id=shape></div>"),
            800.0,
        );
        let id = doc.get_element_by_id("shape").unwrap();
        let shape = crate::tests::harness::find_box(&doc.root, &|node| node.node_id == id).unwrap();
        assert_eq!(
            shape.style.clip_path.kind,
            crate::types::ClipPathKind::Inset,
            "{clip}"
        );
        let expected = Rect::new(10.0, 20.0, 40.0, 30.0);
        assert_eq!(
            shape
                .style
                .clip_path
                .inset_rect(shape.layout.border_rect, 16.0, 16.0),
            expected,
            "{clip}"
        );
        if clip.starts_with("XYWH") {
            assert_eq!(
                shape.style.rare().clip_path_inset_round.as_ref().unwrap().0[0],
                crate::types::CssLength::Px(8.0)
            );
        }
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        replay(
            &build_display_list(&doc.root, 800.0, 600.0),
            &mut pixmap,
            1.0,
        );
        for (x, y, visible) in samples {
            assert_eq!(
                pixmap.pixel(x, y).unwrap().alpha() > 200,
                visible,
                "{clip} paint at ({x},{y})"
            );
            assert_eq!(
                doc.element_from_point(x as f32, y as f32) == Some(id),
                visible,
                "{clip} hit at ({x},{y})"
            );
        }
    }
}

#[test]
fn default_radial_clips_agree_between_paint_and_hit_testing() {
    for (clip, samples) in [
        (
            "circle()",
            [
                (10, 30, false),
                (50, 5, true),
                (50, 30, true),
                (90, 30, false),
            ],
        ),
        (
            "ellipse()",
            [
                (5, 5, false),
                (50, 5, true),
                (50, 30, true),
                (95, 55, false),
            ],
        ),
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!("<style>body{{margin:0}}#shape{{width:100px;height:60px;background:red;clip-path:{clip}}}</style><div id=shape></div>"),
            800.0,
        );
        let id = doc.get_element_by_id("shape").unwrap();
        let mut pixmap = tiny_skia::Pixmap::new(100, 60).unwrap();
        replay(
            &build_display_list(&doc.root, 800.0, 600.0),
            &mut pixmap,
            1.0,
        );
        for (x, y, visible) in samples {
            assert_eq!(
                pixmap.pixel(x, y).unwrap().alpha() > 200,
                visible,
                "{clip} paint at ({x},{y})"
            );
            assert_eq!(
                doc.element_from_point(x as f32, y as f32) == Some(id),
                visible,
                "{clip} hit at ({x},{y})"
            );
        }
    }
}

#[test]
fn inset_clip_reduces_overconstrained_percentages_proportionally() {
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0}#shape{width:100px;height:100px;background:red;clip-path:inset(75% 0 50% 0)}</style><div id=shape></div>",
        800.0,
    );
    let list = build_display_list(&doc.root, 800.0, 600.0);
    let inset = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushClip { rect, .. } if rect.w == 100.0 && rect.h == 0.0 => Some(*rect),
            _ => None,
        })
        .expect("zero-height inset clip");
    assert!((inset.y - 60.0).abs() < 0.01, "inset y = {}", inset.y);
    let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
    replay(&list, &mut pixmap, 1.0);
    assert_eq!(pixmap.pixel(50, 60).unwrap().alpha(), 0);
    assert_ne!(
        doc.element_from_point(50.0, 60.0),
        doc.get_element_by_id("shape")
    );
}

#[test]
fn css_path_clip_uses_svg_curves_and_fill_rule_for_paint_and_hits() {
    for (clip, visible_at_center) in [
        (
            "path(nonzero, 'M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z')",
            true,
        ),
        (
            "path(evenodd, 'M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z')",
            false,
        ),
    ] {
        let mut renderer = Renderer::new();
        let doc = renderer.load_html(
            &format!("<style>body{{margin:0}}#shape{{width:100px;height:100px;background:red;clip-path:{clip}}}</style><div id=shape></div>"),
            800.0,
        );
        let id = doc.get_element_by_id("shape").unwrap();
        let list = build_display_list(&doc.root, 800.0, 600.0);
        assert!(
            list.commands
                .iter()
                .any(|cmd| matches!(cmd, PaintCmd::PushClipSvgPath { .. }))
        );
        let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
        replay(&list, &mut pixmap, 1.0);
        for (x, y, expected) in [(10, 10, true), (50, 50, visible_at_center)] {
            assert_eq!(
                pixmap.pixel(x, y).unwrap().alpha() > 200,
                expected,
                "{clip} paint at ({x},{y})"
            );
            assert_eq!(
                doc.element_from_point(x as f32, y as f32) == Some(id),
                expected,
                "{clip} hit at ({x},{y})"
            );
        }
    }
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0}#shape{width:100px;height:100px;background:red;clip-path:path('M0 100 Q50 -100 100 100 Z')}</style><div id=shape></div>",
        800.0,
    );
    let mut pixmap = tiny_skia::Pixmap::new(100, 100).unwrap();
    replay(
        &build_display_list(&doc.root, 800.0, 600.0),
        &mut pixmap,
        1.0,
    );
    assert!(
        pixmap.pixel(50, 40).unwrap().alpha() > 200,
        "quadratic curve fills below its arc"
    );
    assert_eq!(
        pixmap.pixel(25, 5).unwrap().alpha(),
        0,
        "quadratic curve clips above its arc"
    );
}

#[test]
fn css_path_clip_uses_reference_box_origin_and_clips_positioned_descendants() {
    let mut renderer = Renderer::new();
    let doc = renderer.load_html(
        "<style>body{margin:0}#shape{position:relative;margin:20px;width:100px;height:100px;clip-path:path('M0 0 H50 V100 H0 Z')}#child{position:absolute;inset:0;background:red}</style><div id=shape><div id=child></div></div>",
        800.0,
    );
    let child = doc.get_element_by_id("child").unwrap();
    let mut pixmap = tiny_skia::Pixmap::new(160, 160).unwrap();
    replay(
        &build_display_list(&doc.root, 800.0, 600.0),
        &mut pixmap,
        1.0,
    );
    for (x, y, visible) in [
        (30, 30, true),
        (60, 60, true),
        (90, 30, false),
        (10, 30, false),
    ] {
        assert_eq!(
            pixmap.pixel(x, y).unwrap().alpha() > 200,
            visible,
            "paint at ({x},{y})"
        );
        assert_eq!(
            doc.element_from_point(x as f32, y as f32) == Some(child),
            visible,
            "hit at ({x},{y})"
        );
    }
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
        PaintCmd::BackdropFilter { rect, filters, .. } => Some((rect, filters)),
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
fn backdrop_filter_uses_rounded_border_box() {
    let (_, list) = build(
        r#"<style>body{margin:0}</style><div style="width:20px;height:20px;padding:4px;border:2px solid red;border-radius:8px;backdrop-filter:invert(1)"></div>"#,
    );
    let (rect, radii) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BackdropFilter { rect, radii, .. } => Some((rect, radii)),
            _ => None,
        })
        .unwrap();
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (0.0, 0.0, 32.0, 32.0));
    assert!(radii[0] >= 7.5);

    let list = DisplayList {
        commands: vec![
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                color: Color::rgb(255, 0, 0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::BackdropFilter {
                rect: Rect::new(5.0, 5.0, 20.0, 20.0),
                radii: [8.0; 4],
                radii_y: [8.0; 4],
                filters: vec![(5, 1.0, 0.0, 0.0, Color::BLACK)],
            },
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(40, 40).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let pixel = |x: usize, y: usize| &pixmap.data()[(y * 40 + x) * 4..][..4];
    assert_eq!(pixel(5, 5)[0], 255, "rounded corner retains original red");
    assert_eq!(pixel(15, 15)[0], 0, "center receives inverted backdrop");
}

#[test]
fn backdrop_filter_samples_transformed_position() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 40.0, 30.0),
                color: Color::rgb(255, 0, 0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PushTransform {
                node_id: 1,
                transform: [1.0, 0.0, 0.0, 1.0, 10.0, 0.0],
            },
            PaintCmd::BackdropFilter {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                radii: [0.0; 4],
                radii_y: [0.0; 4],
                filters: vec![(5, 1.0, 0.0, 0.0, Color::BLACK)],
            },
            PaintCmd::PopTransform,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(40, 30).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let red_at = |x: usize| pixmap.data()[(5 * 40 + x) * 4];
    assert_eq!(red_at(5), 255);
    assert_eq!(red_at(15), 0);
}

#[test]
fn backdrop_filter_respects_parent_clip() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 40.0, 20.0),
                color: Color::rgb(255, 0, 0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PushClip {
                rect: Rect::new(10.0, 0.0, 10.0, 20.0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::BackdropFilter {
                rect: Rect::new(5.0, 0.0, 20.0, 20.0),
                radii: [0.0; 4],
                radii_y: [0.0; 4],
                filters: vec![(5, 1.0, 0.0, 0.0, Color::BLACK)],
            },
            PaintCmd::PopClip,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(40, 20).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let red_at = |x: usize| pixmap.data()[(10 * 40 + x) * 4];
    assert_eq!(red_at(7), 255);
    assert_eq!(red_at(15), 0);
    assert_eq!(red_at(23), 255);
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
fn opaque_one_pixel_repeated_background_respects_scroll_and_clip() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BackgroundImage {
        container: Rect::new(10.0, 100.0, 20.0, 20.0),
        clip: Rect::new(10.0, 100.0, 20.0, 20.0),
        data: ImageRef::Owned(vec![255, 0, 0, 255], 1, 1),
        size_mode: 0,
        draw_w: 1.0,
        draw_h: 1.0,
        pos_x: 10.4,
        pos_y: 100.4,
        repeat_x_mode: 1,
        repeat_y_mode: 1,
        radii: [4.0; 4],
        radii_y: [4.0; 4],
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
    assert_eq!(pixmap.pixel(20, 60).unwrap().red(), 255);
    assert_eq!(pixmap.pixel(20, 60).unwrap().green(), 0);
    assert_eq!(pixmap.pixel(10, 50).unwrap().green(), 255);
    assert_eq!(pixmap.pixel(30, 60).unwrap().green(), 255);
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
        text_indent: 0.0,
        text_align: crate::types::TextAlign::Start,
        direction: crate::types::Direction::LTR,
        placeholder_color: Color::rgba(0, 0, 0, 128),
        placeholder_typography: None,
        value_typography: None,
        file_button_color: Color::BLACK,
        file_button_background: Color::TRANSPARENT,
        file_button_font_size: 16.0,
        file_button_font_weight: 400,
        file_button_font_family: "Arial".to_string(),
        checked: true,
        value: String::new(),
        placeholder: String::new(),
        input_cursor: 0,
        text_selection: None,
        appearance_none: false,
        vertical: false,
        options: Vec::new(),
        selected: -1,
        selected_all: Vec::new(),
        content_scroll: (0.0, 0.0),
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
fn live_control_value_retains_computed_typography_without_mutating_value() {
    let (_, list) = build(
        r#"<input value="Hello world" style="width:220px;height:60px;color:red;font:italic 700 20px/12px serif;font-stretch:150%;letter-spacing:3px;word-spacing:7px;text-transform:uppercase;text-decoration:underline;text-shadow:3px 3px 0 blue">"#,
    );
    let styled = list
        .commands
        .iter()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .unwrap()
        .clone();
    let PaintCmd::FormElement {
        value,
        value_typography: Some(typography),
        ..
    } = &styled
    else {
        panic!("computed value typography must be retained")
    };
    assert_eq!(
        value, "Hello world",
        "text-transform must not mutate the live form value"
    );
    assert_eq!(typography.font_style, 1);
    assert_eq!(typography.font_weight, 700);
    assert_eq!(typography.font_stretch, 150.0);
    assert_eq!(typography.letter_spacing, 3.0);
    assert_eq!(typography.word_spacing, 7.0);
    assert_eq!(
        typography.line_height, 12.0,
        "authored line-height below the em must not be clamped"
    );
    assert!(!typography.normal_line_height);
    assert!(typography.decoration.underline);
    assert!(typography.shadow.is_some());
    assert_eq!(typography.text_transform, TextTransform::Uppercase);
    for scale in [1.0, 2.0] {
        let paint = |command: PaintCmd| {
            let mut display_list = DisplayList::new();
            display_list.push(command);
            let mut pixels =
                tiny_skia::Pixmap::new((280.0 * scale) as u32, (100.0 * scale) as u32).unwrap();
            let mut fonts = cosmic_text::FontSystem::new();
            let mut glyphs = cosmic_text::SwashCache::new();
            replay_with_text(&display_list, &mut pixels, scale, &mut fonts, &mut glyphs);
            pixels
        };
        let mut plain = styled.clone();
        if let PaintCmd::FormElement {
            value_typography, ..
        } = &mut plain
        {
            *value_typography = None;
        }
        assert_ne!(
            paint(styled.clone()).data(),
            paint(plain).data(),
            "computed value typography must affect paint at scale {scale}"
        );
    }
}

#[test]
fn placeholder_typography_changes_placeholder_paint_but_not_input_value() {
    let (_, list) = build(
        r#"<input type="text" placeholder="MMMM" style="width:200px;height:50px;font-size:10px">"#,
    );
    let command = list
        .commands
        .iter()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .expect("form paint command")
        .clone();
    let mut styled = command.clone();
    if let PaintCmd::FormElement {
        placeholder_typography,
        ..
    } = &mut styled
    {
        *placeholder_typography = Some(PlaceholderTypography {
            layout: None,
            opacity: 1.0,
            font_size: 28.0,
            font_weight: 700,
            font_style: 1,
            font_family: "serif".to_string(),
            font_stretch: 100.0,
            normal_line_height: false,
            line_height: 34.0,
            letter_spacing: 2.0,
            word_spacing: 0.0,
            text_transform: TextTransform::None,
            decoration: Default::default(),
            shadow: None,
        });
    }
    let paint = |command: PaintCmd| {
        let mut display_list = DisplayList::new();
        display_list.push(command);
        let mut pixmap = tiny_skia::Pixmap::new(240, 80).unwrap();
        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        replay_with_text(&display_list, &mut pixmap, 1.0, &mut fonts, &mut glyphs);
        pixmap
    };
    let base_pixels = paint(command.clone());
    let styled_pixels = paint(styled.clone());
    assert_ne!(
        base_pixels.data(),
        styled_pixels.data(),
        "placeholder font must affect paint"
    );

    let mut value_base = command;
    let mut value_styled = styled;
    if let PaintCmd::FormElement { value, .. } = &mut value_base {
        *value = "MMMM".to_string();
    }
    if let PaintCmd::FormElement { value, .. } = &mut value_styled {
        *value = "MMMM".to_string();
    }
    assert_eq!(
        paint(value_base).data(),
        paint(value_styled).data(),
        "value must keep the input font"
    );
}

#[test]
fn zero_opacity_placeholder_paints_no_text_or_shadow() {
    let (_, mut list) = build(
        r#"<style>input::placeholder { opacity: 0; text-shadow: 2px 2px red; }</style>
           <input type="email" placeholder="you@domain.com" style="width:200px;height:50px">"#,
    );
    let command = list
        .commands
        .iter_mut()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .expect("form paint command");
    let PaintCmd::FormElement {
        placeholder,
        placeholder_typography,
        ..
    } = command
    else {
        unreachable!()
    };
    assert_eq!(placeholder_typography.as_ref().unwrap().opacity, 0.0);
    assert_eq!(placeholder.as_str(), "you@domain.com");
    let mut no_placeholder = list.clone();
    if let Some(PaintCmd::FormElement { placeholder, .. }) = no_placeholder
        .commands
        .iter_mut()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
    {
        placeholder.clear();
    }
    let paint = |list: &DisplayList| {
        let mut pixmap = tiny_skia::Pixmap::new(240, 80).unwrap();
        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        replay_with_text(list, &mut pixmap, 1.0, &mut fonts, &mut glyphs);
        pixmap
    };
    assert_eq!(paint(&list).data(), paint(&no_placeholder).data());
}

#[test]
fn placeholder_transform_and_stretch_reach_paint_without_changing_value() {
    let (_, list) = build(
        r#"<style>input::placeholder { text-transform: uppercase; font-stretch: 150%; }</style>
           <input type="text" placeholder="Hello" style="width:200px;height:50px">"#,
    );
    let styled = list
        .commands
        .iter()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .expect("form paint command")
        .clone();
    let mut plain = styled.clone();
    if let PaintCmd::FormElement {
        placeholder_typography: Some(typography),
        ..
    } = &mut plain
    {
        assert_eq!(typography.text_transform, TextTransform::Uppercase);
        assert_eq!(typography.font_stretch, 150.0);
        typography.text_transform = TextTransform::None;
        typography.font_stretch = 100.0;
    } else {
        panic!("placeholder pseudo style must reach the form paint command");
    }
    let paint = |command: PaintCmd| {
        let mut display_list = DisplayList::new();
        display_list.push(command);
        let mut pixmap = tiny_skia::Pixmap::new(240, 80).unwrap();
        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        replay_with_text(&display_list, &mut pixmap, 1.0, &mut fonts, &mut glyphs);
        pixmap
    };
    assert_ne!(paint(styled.clone()).data(), paint(plain.clone()).data());
    let mut value_styled = styled;
    let mut value_plain = plain;
    if let PaintCmd::FormElement { value, .. } = &mut value_styled {
        *value = "Hello".into();
    }
    if let PaintCmd::FormElement { value, .. } = &mut value_plain {
        *value = "Hello".into();
    }
    assert_eq!(paint(value_styled).data(), paint(value_plain).data());
}

#[test]
fn placeholder_decoration_paints_without_decorating_input_value() {
    let (_, list) = build(
        r#"<style>input::placeholder { text-decoration: underline; text-decoration-color: red; }</style>
           <input type="text" placeholder="Hello" style="width:200px;height:50px">"#,
    );
    let styled = list
        .commands
        .iter()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .expect("form paint command")
        .clone();
    let mut plain = styled.clone();
    if let PaintCmd::FormElement {
        placeholder_typography: Some(typography),
        ..
    } = &mut plain
    {
        assert!(typography.decoration.underline);
        typography.decoration = Default::default();
    } else {
        panic!("placeholder pseudo style must reach the form paint command");
    }
    let paint = |command: PaintCmd| {
        let mut display_list = DisplayList::new();
        display_list.push(command);
        let mut pixmap = tiny_skia::Pixmap::new(240, 80).unwrap();
        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        replay_with_text(&display_list, &mut pixmap, 1.0, &mut fonts, &mut glyphs);
        pixmap
    };
    assert_ne!(paint(styled.clone()).data(), paint(plain.clone()).data());
    let mut value_styled = styled;
    let mut value_plain = plain;
    if let PaintCmd::FormElement { value, .. } = &mut value_styled {
        *value = "Hello".into();
    }
    if let PaintCmd::FormElement { value, .. } = &mut value_plain {
        *value = "Hello".into();
    }
    assert_eq!(paint(value_styled).data(), paint(value_plain).data());
}

#[test]
fn placeholder_blurred_shadow_paints_without_shadowing_input_value() {
    let (_, list) = build(
        r#"<style>input::placeholder { text-shadow: 4px 3px 2px red; }</style>
           <input type="text" placeholder="Hello" style="width:200px;height:50px">"#,
    );
    let styled = list
        .commands
        .iter()
        .find(|command| matches!(command, PaintCmd::FormElement { .. }))
        .expect("form paint command")
        .clone();
    let mut plain = styled.clone();
    if let PaintCmd::FormElement {
        placeholder_typography: Some(typography),
        ..
    } = &mut plain
    {
        assert!(
            typography
                .shadow
                .as_ref()
                .is_some_and(|shadow| shadow.blur > 0.0)
        );
        typography.shadow = None;
    } else {
        panic!("placeholder pseudo style must reach the form paint command");
    }
    let paint = |command: PaintCmd| {
        let mut display_list = DisplayList::new();
        display_list.push(command);
        let mut pixmap = tiny_skia::Pixmap::new(240, 80).unwrap();
        let mut fonts = cosmic_text::FontSystem::new();
        let mut glyphs = cosmic_text::SwashCache::new();
        replay_with_text(&display_list, &mut pixmap, 1.0, &mut fonts, &mut glyphs);
        pixmap
    };
    assert_ne!(paint(styled.clone()).data(), paint(plain.clone()).data());
    let mut value_styled = styled;
    let mut value_plain = plain;
    if let PaintCmd::FormElement { value, .. } = &mut value_styled {
        *value = "Hello".into();
    }
    if let PaintCmd::FormElement { value, .. } = &mut value_plain {
        *value = "Hello".into();
    }
    assert_eq!(paint(value_styled).data(), paint(value_plain).data());
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
        letter_spacing: 0.0,
        word_spacing: 0.0,
        small_caps: false,
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
    assert!(
        list.has_scroll_dependent_sticky,
        "display lists with sticky content must be rebuilt when scroll changes"
    );
}

#[test]
fn empty_sticky_subtree_does_not_invalidate_scroll_tiles() {
    let html = r#"
        <style>body { margin: 0; } #sticky { position: sticky; top: 40px; z-index: 10; }</style>
        <div style="height: 900px; background: white"></div>
        <div id="sticky"><div id="empty"></div></div>
    "#;
    let (_frame, list) = build_full(html);
    assert!(
        !list.has_scroll_dependent_sticky,
        "an empty sticky stacking context cannot change the painted scroll tiles"
    );
}

#[test]
fn empty_fixed_subtree_does_not_force_scroll_rasterization() {
    let html = r#"
        <style>body { margin: 0; } #fixed { position: fixed; bottom: 0; right: 0; z-index: 10; }</style>
        <div style="height: 900px; background: white"></div>
        <div id="fixed"><div id="empty"></div></div>
    "#;
    let (_frame, list) = build_full(html);
    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BeginFixedPosition)),
        "an empty fixed subtree cannot change the painted scroll tiles"
    );

    let (_frame, visible) = build_full(&html.replace(
        "<div id=\"empty\"></div>",
        "<div id=\"empty\" style=\"background: red; width: 20px; height: 20px\"></div>",
    ));
    assert!(
        visible
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::BeginFixedPosition)),
        "visible fixed content must remain anchored to the viewport"
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

#[test]
fn deferred_sticky_uses_its_parent_containing_block() {
    let doc = parse_html(
        r#"<style>
            html, body { margin: 0; height: 600px; }
            #spacer { height: 500px; }
            #wrapper { height: 300px; padding-top: 200px; }
            #sticky { position: sticky; top: 24px; z-index: 400;
                      width: 40px; height: 40px; background: red; }
        </style>
        <div id="spacer"></div>
        <div id="wrapper"><div id="sticky"></div></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    frame.doc.root.layout.content_rect.h = 600.0;
    frame.doc.root.layout.padding_rect.h = 600.0;
    frame.doc.root.layout.border_rect.h = 600.0;
    let expected_y = frame
        .doc
        .get_element_by_id("sticky")
        .and_then(|id| frame.doc.get_node(id))
        .expect("sticky node")
        .layout
        .border_rect
        .y;
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
    let painted_y = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. }
                if color.r == 255 && color.g == 0 && color.b == 0 =>
            {
                Some(rect.y)
            }
            _ => None,
        })
        .expect("sticky background");
    assert!(
        (painted_y - expected_y).abs() < 0.1,
        "painted at {painted_y}, laid out at {expected_y}"
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
fn body_canvas_color_precedes_negative_z_descendant() {
    let (_, list) = build_full(
        r#"<html><head><style>
          html, body { margin: 0; }
          body { background: white; }
          #video { position: relative; z-index: -1; width: 200px; height: 100px; background: red; }
        </style></head><body><div id="video"></div></body></html>"#,
    );
    let white = list.commands.iter().position(|command| {
        matches!(command, PaintCmd::FillRect { color, .. } if *color == Color::rgb(255, 255, 255))
    }).unwrap();
    let red = list.commands.iter().position(|command| {
        matches!(command, PaintCmd::FillRect { color, .. } if *color == Color::rgb(255, 0, 0))
    }).unwrap();
    assert!(
        white < red,
        "body canvas color must be behind negative-z content"
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

#[test]
fn box_shadow_uses_border_edge_outside_and_padding_edge_inside() {
    let (_, list) = build(
        "<body style='margin:0'><div style='position:absolute;left:40px;top:40px;\
         width:100px;height:80px;border:10px solid blue;border-radius:20px;\
         box-shadow:0 0 0 3px red,inset 0 0 0 3px green'></div></body>",
    );
    let shadows: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::BoxShadow {
                rect,
                inset,
                radii,
                radii_y,
                ..
            } => Some((rect, inset, radii, radii_y)),
            _ => None,
        })
        .collect();
    assert_eq!(shadows.len(), 2);
    for (rect, inset, radii, radii_y) in shadows {
        let (expected_rect, expected_radius) = if *inset {
            (Rect::new(50.0, 50.0, 100.0, 80.0), 10.0)
        } else {
            (Rect::new(40.0, 40.0, 120.0, 100.0), 20.0)
        };
        assert_eq!(*rect, expected_rect, "wrong shadow edge for inset={inset}");
        assert_eq!(*radii, [expected_radius; 4]);
        assert_eq!(*radii_y, [expected_radius; 4]);
    }
}

#[test]
fn bordered_box_shadows_paint_on_their_respective_sides() {
    let doc = parse_html(
        "<body style='margin:0;background:white'><div style='position:absolute;left:40px;top:40px;\
         width:100px;height:80px;border:10px solid blue;border-radius:20px;background:white;\
         box-shadow:0 0 0 3px red,inset 0 0 0 3px green'></div></body>",
    );
    let mut frame = EngineFrame::new(doc, 200.0, 180.0);
    frame.update_frame();
    let mut pixels = tiny_skia::Pixmap::new(200, 180).unwrap();
    Renderer::new().render(&mut frame.doc, &mut pixels, 1.0);
    let outer = pixels.pixel(38, 90).unwrap();
    let border = pixels.pixel(45, 90).unwrap();
    let inner = pixels.pixel(51, 90).unwrap();
    let center = pixels.pixel(100, 90).unwrap();
    assert!(
        outer.red() > 200 && outer.green() < 40 && outer.blue() < 40,
        "outer shadow missed the border edge: {outer:?}"
    );
    assert!(
        border.blue() > 200 && border.red() < 40,
        "border was overwritten by a shadow: {border:?}"
    );
    assert!(
        inner.green() > 80 && inner.red() < 80,
        "inset shadow missed the padding edge: {inner:?}"
    );
    assert!(
        center.red() > 240 && center.green() > 240 && center.blue() > 240,
        "inset shadow filled the center: {center:?}"
    );
}

#[test]
fn inset_shadow_offset_moves_hole_without_flattening_its_corner() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BoxShadow {
        rect: Rect::new(40.0, 40.0, 100.0, 100.0),
        color: Color::BLACK,
        offset_x: 20.0,
        offset_y: 0.0,
        blur: 0.0,
        spread: 0.0,
        inset: true,
        radii: [30.0; 4],
        radii_y: [30.0; 4],
    });
    let mut pixels = tiny_skia::Pixmap::new(180, 180).unwrap();
    replay(&list, &mut pixels, 1.0);
    assert!(
        pixels.pixel(65, 48).unwrap().alpha() > 200,
        "offset flattened the inset shadow's upper-left corner"
    );
    assert_eq!(
        pixels.pixel(100, 80).unwrap().alpha(),
        0,
        "shadow filled its translated hole"
    );
}

#[test]
fn inset_shadow_spread_and_offset_translate_the_same_hole() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BoxShadow {
        rect: Rect::new(40.0, 40.0, 100.0, 100.0),
        color: Color::BLACK,
        offset_x: 20.0,
        offset_y: 0.0,
        blur: 0.0,
        spread: 10.0,
        inset: true,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
    });
    let mut pixels = tiny_skia::Pixmap::new(180, 180).unwrap();
    replay(&list, &mut pixels, 1.0);
    assert!(
        pixels.pixel(65, 90).unwrap().alpha() > 200,
        "positive offset and spread did not widen the left shadow"
    );
    assert_eq!(
        pixels.pixel(135, 90).unwrap().alpha(),
        0,
        "positive offset incorrectly left a right-side shadow"
    );
    assert!(
        pixels.pixel(90, 45).unwrap().alpha() > 200,
        "spread did not paint the top shadow"
    );
}

#[test]
fn background_clip_uses_the_corresponding_corner_radius() {
    for (clip, x, y) in [("padding-box", 58, 51), ("content-box", 61, 61)] {
        let doc = parse_html(&format!(
            "<body style='margin:0;background:white'><div style='position:absolute;left:40px;top:40px;\
             width:100px;height:80px;border:10px solid transparent;padding:10px;\
             border-radius:20px;background:red;background-clip:{clip}'></div></body>"
        ));
        let mut frame = EngineFrame::new(doc, 200.0, 180.0);
        frame.update_frame();
        let mut pixels = tiny_skia::Pixmap::new(200, 180).unwrap();
        Renderer::new().render(&mut frame.doc, &mut pixels, 1.0);
        let hit = pixels.pixel(x, y).unwrap();
        assert!(
            hit.red() > 200 && hit.green() < 40 && hit.blue() < 40,
            "{clip} used the border-box corner radius at ({x}, {y}): {hit:?}"
        );
        let outside = pixels.pixel(x - 10, y - 10).unwrap();
        assert!(
            outside.red() > 240 && outside.green() > 240 && outside.blue() > 240,
            "{clip} painted outside its clipping edge: {outside:?}"
        );
    }
}

#[test]
fn gradient_background_uses_its_clip_box_radius() {
    let (_, list) = build(
        "<body style='margin:0'><div style='width:100px;height:80px;border:10px solid transparent;\
         padding:10px;border-radius:20px;background-image:linear-gradient(red,blue);\
         background-clip:padding-box'></div></body>",
    );
    let (clip, radii, radii_y) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Gradient {
                clip,
                radii,
                radii_y,
                ..
            } => Some((clip, radii, radii_y)),
            _ => None,
        })
        .expect("gradient paint command");
    assert_eq!(*clip, Rect::new(10.0, 10.0, 120.0, 100.0));
    assert_eq!(*radii, [10.0; 4]);
    assert_eq!(*radii_y, [10.0; 4]);
}

#[test]
fn image_background_uses_its_clip_box_radius() {
    let doc = parse_html(
        "<body style='margin:0'><div id='box' style='width:100px;height:80px;\
         border:10px solid transparent;padding:10px;border-radius:20px;\
         background-image:url(sprite.png);background-clip:content-box'></div></body>",
    );
    let mut frame = EngineFrame::new(doc, 200.0, 180.0);
    frame.update_frame();
    let box_node = crate::dom::query_selector_mut(&mut frame.doc.root, "#box").unwrap();
    box_node.bg_image_data = Some(std::sync::Arc::new(vec![255; 4]));
    box_node.bg_image_width = 1;
    box_node.bg_image_height = 1;
    box_node.bg_image_ratio_only = false;
    let list = build_display_list(&frame.doc.root, 200.0, 180.0);
    let (clip, radii, radii_y) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BackgroundImage {
                clip,
                radii,
                radii_y,
                ..
            } => Some((clip, radii, radii_y)),
            _ => None,
        })
        .expect("background image paint command");
    assert_eq!(*clip, Rect::new(20.0, 20.0, 100.0, 80.0));
    assert_eq!(*radii, [0.0; 4]);
    assert_eq!(*radii_y, [0.0; 4]);
}

// ── Pixel-level rendering tests ─────────────────────────────────────────────

#[test]
fn blurred_outer_shadow_does_not_fill_transparent_border_box() {
    for extra in ["", "transform:translateX(1px)", "border-radius:20px"] {
        let doc = parse_html(&format!(
            "<body style='margin:0;background:white'><div style='position:absolute;left:40px;top:40px;width:100px;height:80px;box-shadow:0 0 8px black;{extra}'></div></body>"
        ));
        let mut frame = EngineFrame::new(doc, 200.0, 160.0);
        frame.update_frame();
        let mut pixmap = tiny_skia::Pixmap::new(200, 160).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        crate::Renderer::new().render(&mut frame.doc, &mut pixmap, 1.0);
        let inside = pixmap.pixel(90, 80).unwrap();
        let outside = pixmap.pixel(37, 80).unwrap();
        assert_eq!(inside.red(), 255, "shadow filled interior: {extra}");
        assert!(outside.red() < 250, "outer shadow missing: {extra}");
    }
}

#[test]
fn outer_shadow_tile_culling_preserves_edge_and_clears_interior() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BoxShadow {
        rect: Rect::new(100.0, 100.0, 800.0, 800.0),
        color: Color::rgba(0, 0, 0, 255),
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 20.0,
        spread: 0.0,
        inset: false,
        radii: [20.0; 4],
        radii_y: [20.0; 4],
    });
    let mut fonts = cosmic_text::FontSystem::new();
    let mut glyphs = cosmic_text::SwashCache::new();
    for scale in [1.0, 2.0] {
        let side = (256.0 * scale) as u32;
        let mut interior = tiny_skia::Pixmap::new(side, side).unwrap();
        replay_tile_with_scroll_and_transform_overrides(
            &list,
            &mut interior,
            scale,
            &mut fonts,
            &mut glyphs,
            300.0,
            300.0,
            0.0,
            0.0,
            None,
        );
        assert!(
            interior.data().chunks_exact(4).all(|pixel| pixel[3] == 0),
            "interior tile contains shadow at scale {scale}"
        );

        let mut edge = tiny_skia::Pixmap::new(side, side).unwrap();
        replay_tile_with_scroll_and_transform_overrides(
            &list,
            &mut edge,
            scale,
            &mut fonts,
            &mut glyphs,
            0.0,
            300.0,
            0.0,
            0.0,
            None,
        );
        assert!(
            edge.pixel((95.0 * scale) as u32, (100.0 * scale) as u32)
                .is_some_and(|pixel| pixel.alpha() > 0),
            "edge tile lost shadow at scale {scale}"
        );
        assert_eq!(
            edge.pixel((150.0 * scale) as u32, (100.0 * scale) as u32)
                .unwrap()
                .alpha(),
            0,
            "edge tile painted border-box interior at scale {scale}"
        );
    }
}

#[test]
fn transformed_inset_shadow_blurs_and_stays_inside_border_box() {
    for (transform, edge, center, outside) in [
        (
            [1.0, 0.0, 0.0, 1.0, 40.0, 0.0],
            (72, 70),
            (120, 70),
            (65, 70),
        ),
        (
            [1.5, 0.0, 0.0, 1.5, 20.0, 0.0],
            (68, 105),
            (130, 105),
            (60, 105),
        ),
        (
            [0.0, 1.0, -1.0, 0.0, 180.0, 0.0],
            (72, 80),
            (110, 80),
            (65, 80),
        ),
    ] {
        let mut list = DisplayList::new();
        list.push(PaintCmd::PushTransform {
            node_id: 1,
            transform,
        });
        list.push(PaintCmd::BoxShadow {
            rect: Rect::new(30.0, 30.0, 100.0, 80.0),
            color: Color::BLACK,
            offset_x: 0.0,
            offset_y: 0.0,
            blur: 8.0,
            spread: 0.0,
            inset: true,
            radii: [12.0; 4],
            radii_y: [12.0; 4],
        });
        list.push(PaintCmd::PopTransform);
        let mut pixels = tiny_skia::Pixmap::new(240, 190).unwrap();
        replay(&list, &mut pixels, 1.0);
        let edge_alpha = pixels.pixel(edge.0, edge.1).unwrap().alpha();
        let center_alpha = pixels.pixel(center.0, center.1).unwrap().alpha();
        let outside_alpha = pixels.pixel(outside.0, outside.1).unwrap().alpha();
        assert!(
            edge_alpha > center_alpha + 20,
            "transformed inset shadow lost its blur: edge {edge_alpha}, center {center_alpha}"
        );
        assert_eq!(
            outside_alpha, 0,
            "transformed inset shadow escaped its border box"
        );
    }
}

#[test]
fn outer_shadow_blur_has_consistent_css_size_at_hidpi() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::BoxShadow {
        rect: Rect::new(50.0, 50.0, 100.0, 100.0),
        color: Color::BLACK,
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 8.0,
        spread: 0.0,
        inset: false,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
    });
    let mut alphas = Vec::new();
    for scale in [1.0, 2.0] {
        let mut pixels =
            tiny_skia::Pixmap::new((200.0 * scale) as u32, (200.0 * scale) as u32).unwrap();
        replay(&list, &mut pixels, scale);
        alphas.push(
            pixels
                .pixel((45.0 * scale) as u32, (100.0 * scale) as u32)
                .unwrap()
                .alpha(),
        );
    }
    assert!(alphas[0] > 0, "outer shadow is missing");
    assert!(
        (alphas[0] as i16 - alphas[1] as i16).abs() <= 25,
        "outer shadow changed CSS blur width with device scale: {alphas:?}"
    );
}

#[test]
fn outer_shadow_spread_keeps_small_corners_sharp() {
    for blur in [0.0, 4.0] {
        let mut list = DisplayList::new();
        list.push(PaintCmd::BoxShadow {
            rect: Rect::new(50.0, 50.0, 100.0, 100.0),
            color: Color::BLACK,
            offset_x: 0.0,
            offset_y: 0.0,
            blur,
            spread: 20.0,
            inset: false,
            radii: [2.0; 4],
            radii_y: [2.0; 4],
        });
        let mut pixels = tiny_skia::Pixmap::new(200, 200).unwrap();
        replay(&list, &mut pixels, 1.0);
        let near_corner = pixels.pixel(42, 31).unwrap().alpha();
        assert!(
            near_corner > 70,
            "small border radius was exaggerated by spread with blur {blur}: alpha {near_corner}"
        );
        assert_eq!(
            pixels.pixel(60, 60).unwrap().alpha(),
            0,
            "outer shadow painted inside the border box with blur {blur}"
        );
    }
}

#[test]
fn clipped_outer_shadow_blurs_before_clipping() {
    let shadow = PaintCmd::BoxShadow {
        rect: Rect::new(50.0, 50.0, 60.0, 60.0),
        color: Color::BLACK,
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 14.0,
        spread: 0.0,
        inset: false,
        radii: [0.0; 4],
        radii_y: [0.0; 4],
    };
    let mut unclipped = DisplayList::new();
    unclipped.push(shadow.clone());
    let mut clipped = DisplayList::new();
    clipped.push(PaintCmd::PushClip {
        rect: Rect::new(75.0, 0.0, 75.0, 150.0),
        radius: [0.0; 4],
        radius_y: [0.0; 4],
    });
    clipped.push(shadow);
    clipped.push(PaintCmd::PopClip);
    let mut full_pixels = tiny_skia::Pixmap::new(160, 160).unwrap();
    let mut clipped_pixels = tiny_skia::Pixmap::new(160, 160).unwrap();
    replay(&unclipped, &mut full_pixels, 1.0);
    replay(&clipped, &mut clipped_pixels, 1.0);
    for y in 35..50 {
        for x in 80..100 {
            let actual = clipped_pixels.pixel(x, y).unwrap().alpha();
            let expected = full_pixels.pixel(x, y).unwrap().alpha();
            assert!(
                (actual as i16 - expected as i16).abs() <= 2,
                "clip changed the shadow blur at ({x}, {y}): {actual} vs {expected}"
            );
        }
    }
    assert_eq!(clipped_pixels.pixel(70, 40).unwrap().alpha(), 0);
}

#[test]
fn transformed_clipped_outer_shadow_preserves_blur_and_bounds() {
    let shadow = PaintCmd::BoxShadow {
        rect: Rect::new(40.0, 40.0, 60.0, 60.0),
        color: Color::BLACK,
        offset_x: 0.0,
        offset_y: 0.0,
        blur: 12.0,
        spread: 0.0,
        inset: false,
        radii: [10.0; 4],
        radii_y: [10.0; 4],
    };
    let make_list = |with_clip: bool| {
        let mut list = DisplayList::new();
        list.push(PaintCmd::PushTransform {
            node_id: 1,
            transform: [1.25, 0.0, 0.0, 1.25, 25.0, 10.0],
        });
        if with_clip {
            list.push(PaintCmd::PushClip {
                rect: Rect::new(55.0, 0.0, 65.0, 120.0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            });
        }
        list.push(shadow.clone());
        if with_clip {
            list.push(PaintCmd::PopClip);
        }
        list.push(PaintCmd::PopTransform);
        list
    };
    let full = make_list(false);
    let clipped = make_list(true);
    let mut full_pixels = tiny_skia::Pixmap::new(190, 170).unwrap();
    let mut clipped_pixels = tiny_skia::Pixmap::new(190, 170).unwrap();
    replay(&full, &mut full_pixels, 1.0);
    replay(&clipped, &mut clipped_pixels, 1.0);
    assert!(
        full_pixels.pixel(110, 55).unwrap().alpha() > 0,
        "transformed outer shadow did not paint outside its border box"
    );
    for y in 52..60 {
        for x in 105..120 {
            let actual = clipped_pixels.pixel(x, y).unwrap().alpha();
            let expected = full_pixels.pixel(x, y).unwrap().alpha();
            assert!(
                (actual as i16 - expected as i16).abs() <= 2,
                "transformed clip changed the shadow at ({x}, {y}): {actual} vs {expected}"
            );
        }
    }
    assert_eq!(clipped_pixels.pixel(75, 55).unwrap().alpha(), 0);
}

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
fn transformed_repeating_gradient_intersects_its_rounded_and_ancestor_clips() {
    let (_, list) = build(
        "<style>body{margin:0}.outer{position:relative;overflow:hidden;\
         margin:40px;width:100px;height:80px}.tile{position:absolute;left:0;top:0;\
         width:60px;height:60px;border-radius:16px;transform:translateX(80px);\
         background-image:linear-gradient(red,red);background-size:20px 20px}</style>\
         <main><div class='outer'><div class='tile'></div></div></main>",
    );
    for scale in [1.0, 2.0] {
        let mut pixels =
            tiny_skia::Pixmap::new((240.0 * scale) as u32, (160.0 * scale) as u32).unwrap();
        pixels.fill(tiny_skia::Color::WHITE);
        replay(&list, &mut pixels, scale);
        for (x, y, color) in [
            (130.0, 70.0, (255, 0, 0)),
            (150.0, 70.0, (255, 255, 255)),
            (121.0, 41.0, (255, 255, 255)),
        ] {
            let p = pixels
                .pixel((x * scale) as u32, (y * scale) as u32)
                .unwrap();
            assert_eq!(
                (p.red(), p.green(), p.blue()),
                color,
                "scale={scale}, at ({x}, {y})"
            );
        }
    }
}

#[test]
fn absolute_paint_bypasses_only_clips_between_it_and_its_containing_block() {
    let (_, list) = build(
        "<style>body{margin:0}.outer{position:relative;width:200px;height:80px;\
         margin:40px;overflow:hidden}.intermediate{width:20px;height:20px;overflow:hidden}\
         .child{position:absolute;left:100px;top:0;width:140px;height:40px;\
         background:red}</style><main><div class='outer'><div class='intermediate'>\
         <div class='child'></div></div></div></main>",
    );
    let mut pixels = tiny_skia::Pixmap::new(320, 160).unwrap();
    pixels.fill(tiny_skia::Color::WHITE);
    replay(&list, &mut pixels, 1.0);
    let visible = pixels.pixel(150, 50).unwrap();
    assert_eq!(
        (visible.red(), visible.green(), visible.blue()),
        (255, 0, 0)
    );
    let clipped = pixels.pixel(250, 50).unwrap();
    assert_eq!(
        (clipped.red(), clipped.green(), clipped.blue()),
        (255, 255, 255)
    );
}

#[test]
fn animated_gradient_keeps_its_ancestor_overflow_clip() {
    let (_, list) = build(
        "<style>body{margin:0}.box{position:relative;overflow:hidden;border-radius:16px;\
         width:200px;height:80px;margin:40px;background:#e9eaf2}.shine{position:absolute;\
         inset:0;transform:translateX(-40px)}.shine::before{content:'';display:block;\
         width:40px;height:100%;background:linear-gradient(to right,transparent,red)}</style>\
         <main><section><div class='box'><div class='shine'></div></div></section></main>",
    );
    let node_id = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushTransform { node_id, .. } => Some(*node_id),
            _ => None,
        })
        .unwrap();
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    for scale in [1.0, 2.0] {
        for translation in [-40.0, -20.0, 80.0, 190.0, 200.0] {
            let mut pixmap =
                tiny_skia::Pixmap::new((320.0 * scale) as u32, (160.0 * scale) as u32).unwrap();
            pixmap.fill(tiny_skia::Color::WHITE);
            let overrides = std::collections::HashMap::from([(
                node_id,
                [1.0, 0.0, 0.0, 1.0, translation, 0.0],
            )]);
            replay_with_scroll_and_transform_overrides(
                &list,
                &mut pixmap,
                scale,
                &mut font_system,
                &mut swash_cache,
                0.0,
                0.0,
                &overrides,
            );
            for (x, y) in [(30.0, 70.0), (250.0, 70.0), (41.0, 41.0)] {
                let pixel = pixmap
                    .pixel((x * scale) as u32, (y * scale) as u32)
                    .unwrap();
                assert_eq!(
                    (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
                    (255, 255, 255, 255),
                    "shine escaped clip: scale={scale}, translation={translation}, at ({x}, {y})"
                );
            }
        }
    }
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
    list.commands.iter().rev().find_map(|cmd| match cmd {
        PaintCmd::Gradient { stops, .. } => Some(stops.clone()),
        _ => None,
    })
}

#[test]
fn gradient_lengths_resolve_against_used_image_geometry() {
    let cases = [
        (
            "width:200px",
            "linear-gradient(to right,red 40px,green,blue 80%)",
            vec![0.2, 0.5, 0.8],
        ),
        (
            "width:100px",
            "linear-gradient(to right,red 40px,green,blue 80%)",
            vec![0.4, 0.6, 0.8],
        ),
        (
            "width:100px",
            "linear-gradient(to right,red 100px,blue 50%)",
            vec![1.0, 1.0],
        ),
        (
            "width:400px",
            "linear-gradient(to right,red 100px,blue 50%)",
            vec![0.25, 0.5],
        ),
        (
            "width:200px",
            "linear-gradient(to right,red 20px 60px,blue)",
            vec![0.1, 0.3, 1.0],
        ),
        (
            "width:200px",
            "linear-gradient(to right,red calc(10% + 20px),blue)",
            vec![0.2, 1.0],
        ),
        (
            "width:200px;font-size:20px",
            "linear-gradient(to right,red 2em,blue)",
            vec![0.2, 1.0],
        ),
        (
            "width:200px;background-size:100px 100px;background-repeat:no-repeat",
            "linear-gradient(to right,red 20px,blue)",
            vec![0.2, 1.0],
        ),
        (
            "width:200px",
            "radial-gradient(ellipse 100px 50px,red 25px,blue)",
            vec![0.25, 1.0],
        ),
        (
            "width:100px",
            "linear-gradient(45deg,red 50px,blue)",
            vec![50.0 / (100.0_f32 * 2.0_f32.sqrt()), 1.0],
        ),
    ];
    for (geometry, image, expected) in cases {
        let (_, list) = build(&format!(
            "<style>body{{margin:0}}div{{height:100px;{geometry};background-image:{image}}}</style><div></div>"
        ));
        let actual = gradient_stops(&list).expect(image);
        assert_eq!(actual.len(), expected.len(), "{image}");
        for ((_, actual), expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 0.001,
                "{geometry}; {image}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn layered_gradient_lengths_use_each_layers_geometry() {
    let (_, list) = build(
        "<style>body{margin:0}div{width:200px;height:100px;background-image:linear-gradient(to right,red 20px,blue),linear-gradient(to right,green 20px,blue);background-size:100px 100px,200px 100px;background-repeat:no-repeat}</style><div></div>",
    );
    let positions: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Gradient { stops, .. } => Some(stops[0].1),
            _ => None,
        })
        .collect();
    assert_eq!(positions.len(), 2);
    assert!((positions[0] - 0.1).abs() < 0.001);
    assert!((positions[1] - 0.2).abs() < 0.001);
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

    let (first_rect, first_clip, _, _, first_blend) = paints[1];
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

    let (second_rect, second_clip, _, _, second_blend) = paints[0];
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

    let (first_rect, first_clip, first_repeat_x, first_repeat_y) = paints[1];
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

    let (second_rect, _, second_repeat_x, second_repeat_y) = paints[0];
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
        rx > 0.0 && rx < 0.001,
        "zero-width ellipse uses a tiny positive x radius, got {rx}"
    );
    assert!(
        ry > 10_000.0,
        "zero-width precedence gives the ellipse a very large y radius, got {ry}"
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
fn radial_gradient_subpixel_radii_reach_paint_geometry() {
    for (descriptor, expected) in [
        ("circle .25px", (0.25_f32, 0.25_f32)),
        ("ellipse .25px .75px", (0.25, 0.75)),
        ("ellipse .75px .25px", (0.75, 0.25)),
        ("circle closest-side", (0.25, 0.25)),
        ("ellipse closest-side", (0.25, 0.75)),
        (
            "circle closest-corner",
            (0.25_f32.hypot(0.75), 0.25_f32.hypot(0.75)),
        ),
        (
            "ellipse closest-corner",
            (
                0.25 * std::f32::consts::SQRT_2,
                0.75 * std::f32::consts::SQRT_2,
            ),
        ),
    ] {
        let (_, list) = build(&format!(
            "<div style=\"width:100px;height:50px;background:radial-gradient({descriptor} at .25px .75px,red,blue)\"></div>"
        ));
        let (_, _, rx, ry) = radial_gradient_geometry(&list).expect("radial gradient was painted");
        assert!(
            (rx - expected.0).abs() < 0.001 && (ry - expected.1).abs() < 0.001,
            "{descriptor}: radii=({rx},{ry}), expected={expected:?}"
        );
    }
}

#[test]
fn radial_gradient_outside_centers_measure_extended_box_edges() {
    for (cx, cy) in [
        (-20.0_f32, 30.0_f32),
        (120.0, 30.0),
        (50.0, -20.0),
        (50.0, 80.0),
        (-20.0, -20.0),
        (120.0, 80.0),
    ] {
        let near_x = cx.abs().min((100.0 - cx).abs());
        let far_x = cx.abs().max((100.0 - cx).abs());
        let near_y = cy.abs().min((60.0 - cy).abs());
        let far_y = cy.abs().max((60.0 - cy).abs());
        for shape in ["circle", "ellipse"] {
            for (extent, dx, dy, corner) in [
                ("closest-side", near_x, near_y, false),
                ("farthest-side", far_x, far_y, false),
                ("closest-corner", near_x, near_y, true),
                ("farthest-corner", far_x, far_y, true),
            ] {
                let (_, list) = build(&format!(
                    "<div style=\"width:100px;height:60px;\
                     background:radial-gradient({shape} {extent} at {cx}px {cy}px,red,blue)\"></div>"
                ));
                let (actual_cx, actual_cy, rx, ry) = radial_gradient_geometry(&list).unwrap();
                let (expected_rx, expected_ry) = if shape == "circle" {
                    let radius = if corner {
                        dx.hypot(dy)
                    } else if extent == "closest-side" {
                        dx.min(dy)
                    } else {
                        dx.max(dy)
                    };
                    (radius, radius)
                } else {
                    let factor = if corner {
                        std::f32::consts::SQRT_2
                    } else {
                        1.0
                    };
                    (dx * factor, dy * factor)
                };
                assert!(
                    (actual_cx - cx).abs() < 0.01
                        && (actual_cy - cy).abs() < 0.01
                        && (rx - expected_rx).abs() < 0.01
                        && (ry - expected_ry).abs() < 0.01,
                    "{shape} {extent} at ({cx},{cy}): actual=({actual_cx},{actual_cy},{rx},{ry}), expected radii=({expected_rx},{expected_ry})"
                );
            }
        }
    }
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
fn multiple_background_layers_paint_back_to_front() {
    let (_, list) = build(
        r#"<div style="width:100px;height:50px;background-image:linear-gradient(red,red),linear-gradient(green,green),linear-gradient(blue,blue)"></div>"#,
    );
    let painted_colors: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Gradient { stops, .. } => stops.first().map(|stop| stop.0),
            _ => None,
        })
        .collect();
    assert_eq!(painted_colors.len(), 3);
    assert!(painted_colors[0].b > painted_colors[0].r);
    assert!(painted_colors[1].g > painted_colors[1].r);
    assert!(painted_colors[2].r > painted_colors[2].b);

    let mut pixmap = tiny_skia::Pixmap::new(120, 80).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let i = (25 * 120 + 50) * 4;
    let pixel = &pixmap.data()[i..i + 4];
    assert!(
        pixel[0] > pixel[2],
        "the first red layer should cover blue: {pixel:?}"
    );
}

#[test]
fn multilayer_background_options_survive_stylesheet_declaration_order() {
    let images = "background-image:linear-gradient(red,red),linear-gradient(green,green),linear-gradient(blue,blue);";
    let options = "background-size:cover,contain,12px 8px;background-position:left top,center center,right bottom;background-repeat:repeat-x,no-repeat,round;background-origin:border-box,content-box,padding-box;background-clip:content-box,padding-box,border-box;background-blend-mode:multiply,screen,overlay;";
    let html = |style: String| {
        format!("<body style='margin:0'><div style='width:100px;height:50px;{style}'></div></body>")
    };
    let (_, before) = build(&html(format!("{options}{images}")));
    let (_, after) = build(&html(format!("{images}{options}")));
    let before_paints = gradient_paints_with_blend(&before);
    let after_paints = gradient_paints_with_blend(&after);
    assert_eq!(before_paints.len(), 3);
    assert_eq!(before_paints, after_paints);
    assert_eq!(
        before_paints
            .iter()
            .map(|paint| paint.4)
            .collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
}

#[test]
fn shorter_background_lists_cycle_through_four_painted_layers() {
    let images = "background-image:linear-gradient(red,red),linear-gradient(green,green),linear-gradient(blue,blue),linear-gradient(yellow,yellow);";
    let options = "background-size:cover,contain;background-position:left top,right bottom;background-repeat:repeat-x,no-repeat;background-origin:border-box,content-box;background-clip:content-box,padding-box;background-blend-mode:multiply,screen;";
    let html = |style: String| {
        format!("<body style='margin:0'><div style='width:100px;height:50px;{style}'></div></body>")
    };
    let (_, before) = build(&html(format!("{options}{images}")));
    let (_, after) = build(&html(format!("{images}{options}")));
    let before_paints = gradient_paints_with_blend(&before);
    let after_paints = gradient_paints_with_blend(&after);
    assert_eq!(before_paints.len(), 4);
    assert_eq!(before_paints, after_paints);
    assert_eq!(
        before_paints
            .iter()
            .map(|paint| paint.4)
            .collect::<Vec<_>>(),
        vec![2, 1, 2, 1]
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
        resolution: 1.0,
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
    let image_index = list
        .commands
        .iter()
        .position(|cmd| matches!(cmd, PaintCmd::BackgroundImage { .. }))
        .unwrap();
    let gradient_index = list
        .commands
        .iter()
        .position(|cmd| matches!(cmd, PaintCmd::Gradient { .. }))
        .unwrap();
    assert!(
        image_index < gradient_index,
        "the second URL layer must paint behind the first gradient"
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
fn background_blend_isolation_waits_for_paintable_layers() {
    use crate::renderer::display_list_builder::build_display_list_full;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>body{margin:0}#box{width:40px;height:40px;background:red;
            background-image:none,url('pending.png');background-blend-mode:normal,multiply}</style>
            <div id='box'></div>"#,
        100.0,
    );
    let build = |doc: &crate::Document| {
        build_display_list_full(
            &doc.root,
            100.0,
            100.0,
            0.0,
            0.0,
            0,
            0,
            &std::collections::HashSet::new(),
            "",
        )
    };
    let list = build(&doc);
    assert!(
        !list
            .commands
            .iter()
            .any(|command| matches!(command, PaintCmd::PushOpacity { .. }))
    );
    let id = doc.get_element_by_id("box").unwrap();
    doc.get_box_by_id_mut(id).unwrap().additional_bg_images =
        vec![Some(crate::types::DecodedBackgroundImage {
            data: std::sync::Arc::new(vec![0, 0, 255, 255]),
            width: 1,
            height: 1,
            ratio_only: false,
            resolution: 1.0,
        })];
    let list = build(&doc);
    assert_eq!(
        list.commands
            .iter()
            .filter(|command| matches!(command, PaintCmd::PushOpacity { alpha } if *alpha == 1.0))
            .count(),
        1
    );
    assert_eq!(
        list.commands
            .iter()
            .filter(|command| matches!(command, PaintCmd::PopOpacity))
            .count(),
        1
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
        text_indent: 0.0,
        text_align: crate::types::TextAlign::Start,
        direction: crate::types::Direction::LTR,
        placeholder_color: Color::rgba(0, 0, 0, 128),
        placeholder_typography: None,
        value_typography: None,
        file_button_color: Color::BLACK,
        file_button_background: Color::TRANSPARENT,
        file_button_font_size: 16.0,
        file_button_font_weight: 400,
        file_button_font_family: "Arial".to_string(),
        checked: true,
        value: String::new(),
        placeholder: String::new(),
        input_cursor: 0,
        text_selection: None,
        appearance_none: true,
        vertical: false,
        options: Vec::new(),
        selected: -1,
        selected_all: Vec::new(),
        content_scroll: (0.0, 0.0),
    });

    let mut pixmap = tiny_skia::Pixmap::new(24, 24).unwrap();
    replay(&list, &mut pixmap, 1.0);
    assert!(
        pixmap.data().chunks_exact(4).all(|px| px[3] == 0),
        "appearance:none should leave native checkbox chrome unpainted"
    );
}

#[test]
fn form_labels_respect_text_indent_and_control_clipping() {
    let mut list = DisplayList::new();
    list.push(PaintCmd::FormElement {
        tag: "input".to_string(),
        input_type: "submit".to_string(),
        rect: Rect::new(10.0, 10.0, 45.0, 30.0),
        node_id: 1,
        attributes: Vec::new(),
        font_size: 14.0,
        font_weight: 400,
        font_family: "Arial".to_string(),
        color: Color::BLACK,
        text_indent: -1000.0,
        text_align: crate::types::TextAlign::Start,
        direction: crate::types::Direction::LTR,
        placeholder_color: Color::BLACK,
        placeholder_typography: None,
        value_typography: None,
        file_button_color: Color::BLACK,
        file_button_background: Color::TRANSPARENT,
        file_button_font_size: 14.0,
        file_button_font_weight: 400,
        file_button_font_family: "Arial".to_string(),
        checked: false,
        value: "Go".to_string(),
        placeholder: String::new(),
        input_cursor: 0,
        text_selection: None,
        appearance_none: true,
        vertical: false,
        options: Vec::new(),
        selected: -1,
        selected_all: Vec::new(),
        content_scroll: (0.0, 0.0),
    });
    let mut pixmap = tiny_skia::Pixmap::new(160, 50).unwrap();
    pixmap.fill(tiny_skia::Color::WHITE);
    let mut fonts = cosmic_text::FontSystem::new();
    let mut cache = cosmic_text::SwashCache::new();
    crate::renderer::display_list_replay::replay_with_text(
        &list,
        &mut pixmap,
        1.0,
        &mut fonts,
        &mut cache,
    );
    assert!(pixmap.data().chunks_exact(4).all(|px| px[0] == 255));

    if let PaintCmd::FormElement {
        tag,
        input_type,
        value,
        text_indent,
        ..
    } = &mut list.commands[0]
    {
        *tag = "select".to_string();
        input_type.clear();
        *value = "All Departments".to_string();
        *text_indent = 0.0;
    }
    pixmap.fill(tiny_skia::Color::WHITE);
    crate::renderer::display_list_replay::replay_with_text(
        &list,
        &mut pixmap,
        1.0,
        &mut fonts,
        &mut cache,
    );
    for y in 0..50usize {
        for x in 55..160usize {
            assert_eq!(
                pixmap.data()[(y * 160 + x) * 4],
                255,
                "select text escaped at {x},{y}"
            );
        }
    }
}

#[test]
fn select_default_label_paints_at_device_scale() {
    let (_, list) = build(
        r#"<select style="position:absolute;left:200px;top:40px;width:150px;height:40px;appearance:none;border:0;background:transparent;color:black"><option>All Categories</option></select>"#,
    );
    let command = list
        .commands
        .iter()
        .find(|cmd| matches!(cmd, PaintCmd::FormElement { tag, .. } if tag == "select"))
        .unwrap()
        .clone();
    assert!(
        matches!(&command, PaintCmd::FormElement { value, selected: 0, .. } if value == "All Categories")
    );
    let mut isolated = DisplayList::new();
    isolated.push(command);
    let mut fonts = cosmic_text::FontSystem::new();
    let mut cache = cosmic_text::SwashCache::new();
    for scale in [1.0, 2.0] {
        let mut pixels =
            tiny_skia::Pixmap::new((500.0 * scale) as u32, (150.0 * scale) as u32).unwrap();
        pixels.fill(tiny_skia::Color::WHITE);
        replay_with_text(&isolated, &mut pixels, scale, &mut fonts, &mut cache);
        assert!(
            pixels.data().chunks_exact(4).any(|p| p[0] < 128),
            "selected label missing at scale {scale}"
        );
        let mut clipped = DisplayList::new();
        clipped.push(PaintCmd::PushClip {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
        clipped.push(isolated.commands[0].clone());
        clipped.push(PaintCmd::PopClip);
        pixels.fill(tiny_skia::Color::WHITE);
        replay_with_text(&clipped, &mut pixels, scale, &mut fonts, &mut cache);
        assert!(
            pixels.data().chunks_exact(4).all(|p| p[0] == 255),
            "selected label escaped ancestor clip at scale {scale}"
        );
    }
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

#[test]
fn overflow_clip_margin_paint_uses_selected_visual_box() {
    for (value, expected) in [
        ("content-box 5px", "content"),
        ("padding-box 5px", "padding"),
        ("border-box -5px", "border"),
    ] {
        let (frame, list) = build(&format!(
            "<style>html,body{{margin:0}}</style><div id=clip style='width:100px;height:100px;padding:10px;border:10px solid black;overflow:clip;overflow-clip-margin:{value}'></div>"
        ));
        let node = crate::dom::query_selector(&frame.doc.root, "#clip").unwrap();
        let base = match expected {
            "content" => node.layout.content_rect,
            "padding" => node.layout.padding_rect,
            _ => node.layout.border_rect,
        };
        let offset = if value.ends_with("-5px") { -5.0 } else { 5.0 };
        let clip = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::PushClip { rect, .. } => Some(*rect),
                _ => None,
            })
            .expect("overflow clip");
        assert_eq!(
            clip,
            Rect::new(
                base.x - offset,
                base.y - offset,
                base.w + 2.0 * offset,
                base.h + 2.0 * offset
            ),
            "{value}"
        );
    }
}

#[test]
fn overflow_clip_margin_adjusts_rounded_clip_radii() {
    for (value, expected_radius) in [
        ("border-box 5px", 25.0),
        ("padding-box 5px", 15.0),
        ("content-box 5px", 5.0),
    ] {
        let (_, list) = build(&format!(
            "<style>html,body{{margin:0}}</style><div style='width:100px;height:100px;padding:10px;border:10px solid black;border-radius:20px;overflow:clip;overflow-clip-margin:{value}'></div>"
        ));
        let (rx, ry) = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::PushClip {
                    radius, radius_y, ..
                } => Some((*radius, *radius_y)),
                _ => None,
            })
            .expect("rounded overflow clip");
        assert_eq!(rx, [expected_radius; 4], "{value}");
        assert_eq!(ry, [expected_radius; 4], "{value}");
    }
}

#[test]
fn overflow_clip_margin_rounded_edge_clips_child_pixels() {
    let (_, list) = build(
        "<style>html,body{margin:0}</style><div style='width:100px;height:100px;padding:10px;border:10px solid transparent;border-radius:20px;overflow:clip;overflow-clip-margin:content-box 5px'><div style='position:relative;left:-30px;top:-30px;width:150px;height:150px;background:blue'></div></div>",
    );
    let mut pixels = tiny_skia::Pixmap::new(160, 160).unwrap();
    replay(&list, &mut pixels, 1.0);
    assert_eq!(pixels.pixel(15, 15).unwrap().alpha(), 0);
    assert!(pixels.pixel(20, 16).unwrap().alpha() > 0);
    assert!(pixels.pixel(50, 50).unwrap().alpha() > 0);
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

#[test]
fn inset_box_shadow_respects_rounded_corners() {
    let (_frame, list) = build(
        "<style>*{margin:0;padding:0}div{width:84px;height:32px;\
         border-radius:16px;box-shadow:inset 0 0 0 1px black}</style><div></div>",
    );
    let mut pixmap = tiny_skia::Pixmap::new(90, 40).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha = |x, y| pixmap.pixel(x, y).unwrap().alpha();
    assert_eq!(
        alpha(0, 0),
        0,
        "the outer pill corner must stay transparent"
    );
    assert!(
        alpha(42, 0) > 150,
        "the top edge must retain its inset outline"
    );
    assert_eq!(
        alpha(42, 16),
        0,
        "the center must not be filled by the shadow"
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
fn text_overflow_mixed_styled_runs_emits_one_end_marker() {
    let (_, list) = build(
        "<style>*{margin:0;padding:0}div{width:75px;font:20px monospace;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}b{color:red}</style><div>abcd<b>efghijklmnop</b></div>",
    );
    let markers = list
        .commands
        .iter()
        .filter(|cmd| match cmd {
            PaintCmd::Text { text, .. } => text.contains('…'),
            _ => false,
        })
        .count();
    assert_eq!(
        markers, 1,
        "one clipped line must paint one end marker: {:?}",
        list.commands
    );
}

#[test]
fn text_overflow_keeps_complete_graphemes_inside_marker_budget() {
    use unicode_segmentation::UnicodeSegmentation;
    for source in ["abcdefghijk", "a\u{301}a\u{301}a\u{301}a\u{301}a\u{301}"] {
        for width in [40.0, 55.0] {
            let (_, list) = build(&format!(
                "<style>*{{margin:0;padding:0}}div{{width:{width}px;font:20px monospace;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}</style><div>{source}</div>"
            ));
            let text = list
                .commands
                .iter()
                .find_map(|cmd| match cmd {
                    PaintCmd::Text { text, .. } if text.ends_with('…') => Some(text.as_str()),
                    _ => None,
                })
                .expect("ellipsis command");
            let prefix = text.strip_suffix('…').unwrap();
            assert!(
                prefix.is_empty()
                    || source
                        .grapheme_indices(true)
                        .any(|(offset, cluster)| offset + cluster.len() == prefix.len()),
                "ellipsis split a grapheme: {text:?}"
            );
            let measured = crate::layout::inline_layout::measure_text_width_weighted(
                text,
                20.0,
                None,
                crate::types::FontWeight::Normal,
                crate::types::FontStyle::Normal,
                1.0,
                "monospace",
                1.0,
            );
            assert!(
                measured <= width + 0.1,
                "ellipsis clipped at {width}px: {text:?} occupies {measured}px"
            );
        }
    }
}

#[test]
fn text_overflow_reserves_the_markers_letter_spacing() {
    let (_, list) = build(
        "<style>*{margin:0;padding:0}div{width:80px;font:20px monospace;letter-spacing:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}</style><div>abcdefghijk</div>",
    );
    let text = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } if text.ends_with('…') => Some(text.as_str()),
            _ => None,
        })
        .expect("ellipsis command");
    let measured = crate::layout::inline_layout::measure_text_width_weighted(
        text,
        20.0,
        None,
        crate::types::FontWeight::Normal,
        crate::types::FontStyle::Normal,
        1.0,
        "monospace",
        1.0,
    ) + text.chars().count().saturating_sub(1) as f32 * 4.0;
    assert!(
        measured <= 80.1,
        "tracking clips the ellipsis: {text:?} occupies {measured}px"
    );

    let mut renderer = Renderer::new();
    let width = crate::layout::inline_layout::measure_text_width_weighted(
        "abcd…",
        20.0,
        Some(&mut renderer.font_system),
        crate::types::FontWeight::Normal,
        crate::types::FontStyle::Normal,
        1.0,
        "monospace",
        1.0,
    ) + 4.0 * 4.0
        + 0.5;
    let doc = renderer.load_html(&format!("<style>*{{margin:0;padding:0}}div{{width:{width}px;font:20px monospace;letter-spacing:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}</style><div>abcdefghijk</div>"), 800.0);
    let list = build_display_list_full_with_font_system(
        &doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
        Some(&mut renderer.font_system as *mut _),
    );
    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "abcd…")),
        "must not reserve unused tracking after the last marker glyph"
    );
}

#[test]
fn empty_text_overflow_string_clips_at_grapheme_boundaries_without_ellipsis() {
    use unicode_segmentation::UnicodeSegmentation;
    let source = "a\u{301}a\u{301}a\u{301}a\u{301}a\u{301}";
    let (_, list) = build(&format!(
        "<style>*{{margin:0;padding:0}}div{{width:40px;font:20px monospace;white-space:nowrap;overflow:hidden;text-overflow:\"\"}}</style><div>{source}</div>"
    ));
    let text = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .expect("text command");
    assert!(
        !text.contains('…'),
        "empty marker must not become ellipsis: {text:?}"
    );
    assert!(
        text.len() < source.len(),
        "must truncate rather than clip mid-glyph"
    );
    assert!(
        source
            .grapheme_indices(true)
            .any(|(offset, cluster)| offset + cluster.len() == text.len())
    );
}

#[test]
fn rtl_single_value_text_overflow_marks_the_physical_left_edge() {
    let (_, list) = build(
        "<style>*{margin:0;padding:0}div{direction:rtl;width:45px;font:20px sans-serif;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}</style><div>مرحبا بالعالم الجميل</div>",
    );
    let marker = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text == "…" => Some(*x),
            _ => None,
        })
        .expect("RTL overflow marker");
    assert!(
        (marker - 0.0).abs() < 0.1,
        "marker should sit at the physical left edge: {marker}"
    );
    assert!(
        !list.commands.iter().any(
            |cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.ends_with('…') && text != "…")
        ),
        "single-value RTL text-overflow must not append a right-edge marker"
    );
}

#[test]
fn scrolled_two_value_text_overflow_marks_the_fixed_left_edge() {
    let mut frame = EngineFrame::new(
        parse_html(
            "<style>*{margin:0;padding:0}div{width:50px;font:20px monospace;white-space:nowrap;overflow:auto;text-overflow:'<' clip}</style><div id=box>abcdefghijklmnop</div>",
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let box_node = crate::dom::query_selector_mut(&mut frame.doc.root, "#box").unwrap();
    assert!(box_node.layout.scroll_width > box_node.layout.content_rect.w);
    box_node.layout.scroll_left = 30.0;
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let marker_x = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text == "<" => Some(*x),
            _ => None,
        })
        .expect("scrolled left-edge marker");
    assert!(
        (marker_x - 0.0).abs() < 0.1,
        "left marker must remain at the box edge: {marker_x}"
    );
}

#[test]
fn scrolled_two_value_text_overflow_marks_both_physical_edges() {
    let mut frame = EngineFrame::new(
        parse_html(
            "<style>*{margin:0;padding:0}div{width:50px;font:20px monospace;white-space:nowrap;overflow:auto;text-overflow:'<' '>'}</style><div id=box>abcdefghijklmnop</div>",
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let box_node = crate::dom::query_selector_mut(&mut frame.doc.root, "#box").unwrap();
    assert!(box_node.layout.scroll_width > box_node.layout.content_rect.w);
    box_node.layout.scroll_left = 30.0;
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let markers: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text == "<" || text == ">" => {
                Some((text.as_str(), *x))
            }
            _ => None,
        })
        .collect();
    assert!(
        markers
            .iter()
            .any(|&(marker, x)| marker == "<" && x.abs() < 0.1),
        "left marker should stay at the left edge: {markers:?}"
    );
    assert!(
        markers
            .iter()
            .any(|&(marker, x)| marker == ">" && x > 30.0 && x < 50.0),
        "right marker should stay at the right edge: {markers:?}"
    );

    let box_node = crate::dom::query_selector_mut(&mut frame.doc.root, "#box").unwrap();
    box_node.layout.scroll_left = box_node.layout.scroll_width - box_node.layout.content_rect.w;
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "<"))
    );
    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == ">")),
        "right marker must disappear once the right edge is fully visible"
    );
}

#[test]
fn left_text_overflow_marker_reserves_its_letter_spacing() {
    fn clipped_text_start(spacing: u32) -> f32 {
        let mut frame = EngineFrame::new(
            parse_html(&format!(
                "<style>*{{margin:0;padding:0}}div{{width:50px;font:20px monospace;letter-spacing:{spacing}px;white-space:nowrap;overflow:auto;text-overflow:'<<' clip}}</style><div id=box>abcdefghijklmnop</div>"
            )),
            800.0,
            600.0,
        );
        frame.update_frame();
        crate::dom::query_selector_mut(&mut frame.doc.root, "#box")
            .unwrap()
            .layout
            .scroll_left = 30.0;
        let list = build_display_list(&frame.doc.root, 800.0, 600.0);
        assert!(
            list.commands
                .iter()
                .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "<<"))
        );
        list.commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::PushClip { rect, .. } if rect.x > 0.0 && rect.w < 50.0 => Some(rect.x),
                _ => None,
            })
            .expect("left marker text clip")
    }

    let plain = clipped_text_start(0);
    let spaced = clipped_text_start(4);
    assert!(
        (spaced - plain - 4.0).abs() < 0.1,
        "marker tracking must reserve one extra 4px gap: {plain} -> {spaced}"
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
        Some("b) "),
        "fallback representation must not receive the original style's padding"
    );
}

#[test]
fn custom_counter_style_fallback_uses_its_representation_not_its_marker() {
    let (frame, _) = build(
        r#"
        <style>
        @counter-style fallback-symbol {
            system: fixed 1;
            symbols: "F";
            pad: 2 "0";
            prefix: "X";
            suffix: "Y";
        }
        @counter-style primary-symbol {
            system: fixed 2;
            symbols: "P";
            fallback: fallback-symbol;
            pad: 5 "Z";
            prefix: "[";
            suffix: "]";
        }
        li { list-style-type: primary-symbol; }
        </style>
        <ol><li id=first>first</li></ol>
    "#,
    );
    let item = crate::tests::harness::find_box(&frame.doc.root, &|node| {
        node.tag == "li"
            && node
                .attributes
                .get("id")
                .is_some_and(|value| value == "first")
    })
    .unwrap();
    assert_eq!(item.style.marker_content, "[0F]");
}

#[test]
fn custom_counter_style_negative_sign_and_auto_ranges_follow_the_system() {
    let (frame, _) = build(
        r#"
        <style>
        @counter-style binary-sign {
            system: numeric;
            symbols: "0" "1";
            negative: "(" ")";
            pad: 4 "0";
            suffix: " ";
        }
        @counter-style alpha-sign {
            system: alphabetic;
            symbols: "a" "b";
            negative: "(" ")";
            suffix: " ";
        }
        @counter-style cyclic-sign {
            system: cyclic;
            symbols: "A" "B";
            negative: "(" ")";
            suffix: " ";
        }
        @counter-style repeated {
            system: symbolic;
            symbols: "R";
            suffix: " ";
        }
        ol { counter-reset: list-item -3; }
        #numeric { list-style-type: binary-sign; }
        #alpha { list-style-type: alpha-sign; counter-set: list-item -2; }
        #cyclic { list-style-type: cyclic-sign; counter-set: list-item -2; }
        #huge { list-style-type: repeated; counter-set: list-item 1000000000; }
        </style>
        <ol><li id=numeric>numeric</li><li id=alpha>alpha</li><li id=cyclic>cyclic</li><li id=huge>huge</li></ol>
    "#,
    );
    let marker = |id: &str| {
        crate::tests::harness::find_box(&frame.doc.root, &|node| {
            node.tag == "li" && node.attributes.get("id").is_some_and(|value| value == id)
        })
        .map(|node| node.style.marker_content.clone())
    };
    assert_eq!(marker("numeric").as_deref(), Some("(0010) "));
    assert_eq!(marker("alpha").as_deref(), Some("-2 "));
    assert_eq!(marker("cyclic").as_deref(), Some("B "));
    assert_eq!(marker("huge").as_deref(), Some("1000000000 "));
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
    let list = list_marker_with_example_base(
        r#"<ul><li style="list-style-image:url(silicon.png); width:100px">item</li></ul>"#,
    );
    let marker = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::ListMarker {
            marker_type, text, ..
        } if *marker_type == 4 => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(
        marker,
        Some("silicon.png"),
        "list-style-image should survive marker generation as an image marker"
    );
}

#[test]
fn list_style_shorthand_url_emits_image_marker_command() {
    let list = list_marker_with_example_base(
        r#"<ul><li style="list-style:url(silicon.png); width:100px">item</li></ul>"#,
    );
    assert!(
        list.commands.iter().any(|cmd| matches!(
            cmd,
            PaintCmd::ListMarker {
                marker_type: 4,
                text,
                ..
            } if text == "silicon.png"
        )),
        "list-style shorthand URL should feed list-style-image"
    );
}

fn list_marker_with_example_base(html: &str) -> DisplayList {
    let base = format!("{}/examples/", env!("CARGO_MANIFEST_DIR"));
    let doc = parse_html_with_base(html, &base);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    build_display_list_full(
        &frame.doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        &base,
    )
}

#[test]
fn missing_list_style_image_uses_list_style_type_fallback() {
    let (_, list) = build(
        r#"<ul><li style="list-style-image:url(missing-marker.png);list-style-type:square">item</li></ul>"#,
    );
    assert!(
        list.commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::ListMarker { marker_type: 2, .. }))
    );
    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::ListMarker { marker_type: 4, .. }))
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
            text_align: crate::types::TextAlign::Left,
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
        has_scroll_dependent_sticky: false,
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
fn list_style_image_marker_preserves_intrinsic_aspect_ratio() {
    let paint_list = DisplayList {
        commands: vec![PaintCmd::ListMarker {
            marker_type: 4,
            text_align: crate::types::TextAlign::Left,
            x: 4.0,
            y: 4.0,
            size: 8.0,
            color: Color::rgba(0, 0, 0, 255),
            text: String::new(),
            image: Some(ImageRef::Owned(vec![255, 0, 0, 255].repeat(8 * 4), 8, 4)),
            font_family: String::new(),
            font_size: 16.0,
            font_weight: 400,
            font_style: 0,
            line_height: 20.0,
        }],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(24, 24).unwrap();
    replay(&paint_list, &mut pixmap, 1.0);
    let pixel = |x: usize, y: usize| &pixmap.data()[(y * 24 + x) * 4..][..4];
    assert!(pixel(10, 6)[0] > 200, "intrinsic marker width should paint");
    assert_eq!(
        pixel(10, 10)[3],
        0,
        "intrinsic marker height should not stretch"
    );
}

#[test]
fn circle_list_marker_paints_a_hollow_circle() {
    let list = DisplayList {
        commands: vec![PaintCmd::ListMarker {
            marker_type: 1,
            text_align: crate::types::TextAlign::Left,
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
        has_scroll_dependent_sticky: false,
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
                no_clip: false,
                origin: Rect::new(0.0, 0.0, 20.0, 10.0),
                tile: Rect::new(0.0, 0.0, 20.0, 10.0),
                data: ImageRef::Owned(
                    vec![
                        255, 255, 255, 255, // left half visible
                        0, 0, 0, 0, // right half transparent by alpha
                    ],
                    2,
                    1,
                ),
                luminance: false,
                repeat_x_mode: 0,
                repeat_y_mode: 0,
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
        has_scroll_dependent_sticky: false,
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
                no_clip: false,
                origin: Rect::new(0.0, 0.0, 10.0, 10.0),
                tile: Rect::new(0.0, 0.0, 10.0, 10.0),
                data: ImageRef::Owned(vec![0, 0, 0, 255], 1, 1),
                luminance: false,
                repeat_x_mode: 0,
                repeat_y_mode: 0,
            },
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::rgba(32, 33, 34, 255),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PopMask,
        ],
        has_scroll_dependent_sticky: false,
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
fn mask_layer_luminance_multiplies_color_by_alpha() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushMask {
                rect: Rect::new(0.0, 0.0, 30.0, 10.0),
                no_clip: false,
                origin: Rect::new(0.0, 0.0, 30.0, 10.0),
                tile: Rect::new(0.0, 0.0, 30.0, 10.0),
                data: ImageRef::Owned(
                    vec![0, 0, 0, 255, 128, 128, 128, 128, 128, 128, 128, 255],
                    3,
                    1,
                ),
                luminance: true,
                repeat_x_mode: 0,
                repeat_y_mode: 0,
            },
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 30.0, 10.0),
                color: Color::rgba(200, 40, 20, 255),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PopMask,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(30, 10).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha_at = |x: usize| pixmap.data()[(5 * 30 + x) * 4 + 3];
    assert_eq!(alpha_at(5), 0);
    assert!((alpha_at(15) as i32 - 128).abs() <= 1);
    assert!((alpha_at(25) as i32 - 128).abs() <= 1);
}

#[test]
fn mask_mode_luminance_reaches_display_list() {
    let doc = parse_html(
        r#"<div id="masked" style="width:30px;height:10px;mask-image:url(mask.png);mask-mode:luminance;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 100.0, 50.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default)).set(
        0,
        crate::types::DecodedMaskImage {
            data: std::sync::Arc::new(vec![255, 255, 255, 255]),
            width: 1,
            height: 1,
            resolution: 1.0,
        },
    );
    let list = build_display_list(&frame.doc.root, 100.0, 50.0);
    assert!(list.commands.iter().any(|cmd| matches!(
        cmd,
        PaintCmd::PushMask {
            luminance: true,
            ..
        }
    )));
}

#[test]
fn mask_size_position_repeat_and_clip_reach_pixels() {
    let commands = vec![
        PaintCmd::PushMask {
            rect: Rect::new(0.0, 0.0, 18.0, 10.0),
            no_clip: false,
            origin: Rect::new(0.0, 0.0, 20.0, 10.0),
            tile: Rect::new(15.0, 0.0, 5.0, 10.0),
            data: ImageRef::Owned(vec![255, 255, 255, 255], 1, 1),
            luminance: false,
            repeat_x_mode: 0,
            repeat_y_mode: 0,
        },
        PaintCmd::FillRect {
            rect: Rect::new(0.0, 0.0, 20.0, 10.0),
            color: Color::rgb(200, 30, 10),
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        },
        PaintCmd::PopMask,
    ];
    let list = DisplayList {
        commands,
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(20, 10).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha = |x: usize| pixmap.data()[(5 * 20 + x) * 4 + 3];
    assert_eq!(alpha(14), 0, "outside the positioned mask tile");
    assert_eq!(alpha(16), 255, "inside the positioned mask tile");
    assert_eq!(alpha(19), 0, "outside mask-clip");
}

#[test]
fn intrinsic_mask_repeats_its_source_pixels_without_stretching() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushMask {
                rect: Rect::new(0.0, 0.0, 8.0, 4.0),
                no_clip: false,
                origin: Rect::new(0.0, 0.0, 8.0, 4.0),
                tile: Rect::new(0.0, 0.0, 2.0, 1.0),
                data: ImageRef::Owned(vec![255, 255, 255, 255, 0, 0, 0, 0], 2, 1),
                luminance: false,
                repeat_x_mode: 1,
                repeat_y_mode: 1,
            },
            PaintCmd::FillRect {
                rect: Rect::new(0.0, 0.0, 8.0, 4.0),
                color: Color::rgb(200, 30, 10),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::PopMask,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(8, 4).unwrap();
    replay(&list, &mut pixmap, 1.0);
    for y in 0..4 {
        for x in 0..8 {
            assert_eq!(
                pixmap.data()[(y * 8 + x) * 4 + 3],
                if x % 2 == 0 { 255 } else { 0 },
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn mask_no_clip_can_reveal_paint_outside_the_border_box() {
    for no_clip in [false, true] {
        let list = DisplayList {
            commands: vec![
                PaintCmd::PushMask {
                    rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                    no_clip,
                    origin: Rect::new(0.0, 0.0, 10.0, 10.0),
                    tile: Rect::new(12.0, 0.0, 4.0, 10.0),
                    data: ImageRef::Owned(vec![255, 255, 255, 255], 1, 1),
                    luminance: false,
                    repeat_x_mode: 0,
                    repeat_y_mode: 0,
                },
                PaintCmd::FillRect {
                    rect: Rect::new(12.0, 0.0, 4.0, 10.0),
                    color: Color::rgb(200, 30, 10),
                    radius: [0.0; 4],
                    radius_y: [0.0; 4],
                },
                PaintCmd::PopMask,
            ],
            has_scroll_dependent_sticky: false,
            fixed_commands: Vec::new(),
        };
        let mut pixmap = tiny_skia::Pixmap::new(20, 10).unwrap();
        replay(&list, &mut pixmap, 1.0);
        assert_eq!(
            pixmap.data()[(5 * 20 + 14) * 4 + 3],
            if no_clip { 255 } else { 0 }
        );
    }
}

#[test]
fn mask_geometry_uses_css_boxes_and_authored_image_size() {
    let doc = parse_html(
        r#"<div id="masked" style="width:40px;height:20px;padding:4px;border:2px solid black;mask-image:url(mask.png);mask-origin:content-box;mask-clip:padding-box;mask-size:10px 5px;mask-position:right bottom;mask-repeat:no-repeat;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 100.0, 60.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default)).set(
        0,
        crate::types::DecodedMaskImage {
            data: std::sync::Arc::new(vec![255, 255, 255, 255]),
            width: 1,
            height: 1,
            resolution: 1.0,
        },
    );
    let list = build_display_list(&frame.doc.root, 100.0, 60.0);
    let PaintCmd::PushMask {
        rect,
        origin,
        tile,
        repeat_x_mode,
        repeat_y_mode,
        ..
    } = list
        .commands
        .iter()
        .find(|cmd| matches!(cmd, PaintCmd::PushMask { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert!((origin.w - 40.0).abs() < 0.01);
    assert!((origin.h - 20.0).abs() < 0.01);
    assert!((rect.w - 48.0).abs() < 0.01);
    assert!((rect.h - 28.0).abs() < 0.01);
    assert!((tile.x - (origin.x + origin.w - 10.0)).abs() < 0.01);
    assert!((tile.y - (origin.y + origin.h - 5.0)).abs() < 0.01);
    assert!((tile.w - 10.0).abs() < 0.01);
    assert!((tile.h - 5.0).abs() < 0.01);
    assert_eq!((*repeat_x_mode, *repeat_y_mode), (0, 0));
}

#[test]
fn mask_fill_box_maps_differently_for_origin_and_clip_on_html() {
    let doc = parse_html(
        r#"<div id="masked" style="width:40px;height:20px;padding:4px;border:2px solid black;mask-image:url(mask.png);mask-origin:fill-box;mask-clip:fill-box;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 100.0, 60.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default)).set(
        0,
        crate::types::DecodedMaskImage {
            data: std::sync::Arc::new(vec![255, 255, 255, 255]),
            width: 1,
            height: 1,
            resolution: 1.0,
        },
    );
    let list = build_display_list(&frame.doc.root, 100.0, 60.0);
    let PaintCmd::PushMask { origin, rect, .. } = list
        .commands
        .iter()
        .find(|cmd| matches!(cmd, PaintCmd::PushMask { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert!((origin.w - 52.0).abs() < 0.01);
    assert!((origin.h - 32.0).abs() < 0.01);
    assert!((rect.w - 40.0).abs() < 0.01);
    assert!((rect.h - 20.0).abs() < 0.01);
}

#[test]
fn mask_shorthand_no_clip_reaches_display_list() {
    let doc = parse_html(
        r#"<div id="masked" style="width:20px;height:10px;mask:url(mask.png) left top / 5px 5px no-repeat content-box no-clip;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 60.0, 40.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default)).set(
        0,
        crate::types::DecodedMaskImage {
            data: std::sync::Arc::new(vec![255, 255, 255, 255]),
            width: 1,
            height: 1,
            resolution: 1.0,
        },
    );
    let list = build_display_list(&frame.doc.root, 60.0, 40.0);
    assert!(list.commands.iter().any(|cmd| matches!(
        cmd,
        PaintCmd::PushMask { no_clip: true, tile, .. }
            if (tile.w - 5.0).abs() < 0.01 && (tile.h - 5.0).abs() < 0.01
    )));
}

#[test]
fn image_set_mask_uses_selected_candidate_resolution_for_auto_size() {
    let doc = parse_html(
        r#"<div id="masked" style="width:250px;height:100px;mask-image:image-set(url(one.png) 1x,url(two.png) 2x);mask-repeat:no-repeat;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 300.0, 150.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    let bitmap = std::sync::Arc::new(vec![255; 200 * 100 * 4]);
    assert!(crate::images::set_decoded_mask_image_for_url_on_node(
        node,
        crate::images::DecodedImage::Raster(bitmap, 200, 100),
        "https://example.test/two.png",
        "https://example.test/page",
    ));
    assert_eq!(
        node.mask_images
            .as_ref()
            .unwrap()
            .first
            .as_ref()
            .unwrap()
            .resolution,
        2.0
    );
    let list = build_display_list(&frame.doc.root, 300.0, 150.0);
    let tile = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushMask { tile, .. } => Some(*tile),
            _ => None,
        })
        .expect("mask paint command");
    assert!((tile.w - 100.0).abs() < 0.1, "width={}", tile.w);
    assert!((tile.h - 50.0).abs() < 0.1, "height={}", tile.h);
}

#[test]
fn mask_image_none_does_not_paint_retained_decoded_pixels() {
    let doc = parse_html(
        r#"<div id="masked" style="width:20px;height:10px;mask-image:url(mask.png);background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 60.0, 40.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default)).set(
        0,
        crate::types::DecodedMaskImage {
            data: std::sync::Arc::new(vec![255, 255, 255, 255]),
            width: 1,
            height: 1,
            resolution: 1.0,
        },
    );
    assert!(
        build_display_list(&frame.doc.root, 60.0, 40.0)
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::PushMask { .. }))
    );
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut node.style),
        "mask-image",
        "none",
    );
    assert!(
        build_display_list(&frame.doc.root, 60.0, 40.0)
            .commands
            .iter()
            .all(|cmd| !matches!(cmd, PaintCmd::PushMask { .. }))
    );
}

#[test]
fn mask_images_keep_none_layers_and_repeat_geometry_values() {
    let doc = parse_html(
        r#"<div id="masked" style="width:20px;height:10px;mask-image:none,url(second.png),url(third.png);mask-size:4px 5px,6px 7px;mask-repeat:no-repeat;background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 60.0, 40.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    let images = std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default));
    for index in 1..=2 {
        images.set(
            index,
            crate::types::DecodedMaskImage {
                data: std::sync::Arc::new(vec![255, 255, 255, 255]),
                width: 1,
                height: 1,
                resolution: 1.0,
            },
        );
    }
    let list = build_display_list(&frame.doc.root, 60.0, 40.0);
    let layers = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::PushMaskGroup { layers } => Some(layers),
            _ => None,
        })
        .unwrap();
    assert_eq!(layers.len(), 3);
    assert!(layers[0].data.is_none());
    assert_eq!((layers[1].tile.w, layers[1].tile.h), (6.0, 7.0));
    assert_eq!((layers[2].tile.w, layers[2].tile.h), (4.0, 5.0));
    assert_eq!(layers[2].repeat_x_mode, 0);
}

#[test]
fn all_none_mask_layers_leave_content_visible() {
    let (_, list) =
        build(r#"<div style="width:20px;height:10px;background:red;mask-image:none,none"></div>"#);
    assert!(
        list.commands
            .iter()
            .all(|cmd| !matches!(cmd, PaintCmd::PushMaskGroup { .. }))
    );
    assert!(list.commands.iter().any(|cmd| matches!(cmd,
        PaintCmd::FillRect { color, .. } if color.r == 255 && color.g == 0 && color.b == 0
    )));
}

#[test]
fn changed_mask_source_does_not_paint_old_decoded_pixels() {
    let doc = parse_html(
        r#"<div id="masked" style="width:20px;height:10px;mask-image:url(old.png);background:red"></div>"#,
    );
    let mut frame = EngineFrame::new(doc, 60.0, 40.0);
    frame.update_frame();
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    let image = crate::types::DecodedMaskImage {
        data: std::sync::Arc::new(vec![255, 255, 255, 255]),
        width: 1,
        height: 1,
        resolution: 1.0,
    };
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default))
        .set_with_source(0, image.clone(), "old.png".to_string());
    assert!(
        build_display_list(&frame.doc.root, 60.0, 40.0)
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::PushMask { .. }))
    );
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut node.style),
        "mask-image",
        "url(new.png)",
    );
    assert!(
        build_display_list(&frame.doc.root, 60.0, 40.0)
            .commands
            .iter()
            .all(|cmd| !matches!(cmd, PaintCmd::PushMask { .. }))
    );
    let node = crate::dom::query_selector_mut(&mut frame.doc.root, "#masked").unwrap();
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default))
        .set_with_source(0, image, "new.png".to_string());
    assert!(
        build_display_list(&frame.doc.root, 60.0, 40.0)
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::PushMask { .. }))
    );
}

#[test]
fn mask_composite_uses_front_layer_operator() {
    use crate::renderer::display_list::MaskPaintLayer;
    let rgba = |alpha: u8| ImageRef::Owned(vec![alpha, alpha, alpha, alpha], 1, 1);
    let layer = |alpha: u8, composite: u8| MaskPaintLayer {
        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        no_clip: false,
        origin: Rect::new(0.0, 0.0, 1.0, 1.0),
        tile: Rect::new(0.0, 0.0, 1.0, 1.0),
        data: Some(rgba(alpha)),
        luminance: false,
        repeat_x_mode: 0,
        repeat_y_mode: 0,
        composite,
    };
    for (operator, expected) in [(0, 208), (1, 144), (2, 48), (3, 160)] {
        let list = DisplayList {
            commands: vec![
                PaintCmd::PushMaskGroup {
                    layers: vec![layer(192, operator), layer(64, 1)],
                },
                PaintCmd::FillRect {
                    rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                    color: Color::rgba(255, 0, 0, 255),
                    radius: [0.0; 4],
                    radius_y: [0.0; 4],
                },
                PaintCmd::PopMask,
            ],
            has_scroll_dependent_sticky: false,
            fixed_commands: Vec::new(),
        };
        let mut pixmap = tiny_skia::Pixmap::new(1, 1).unwrap();
        replay(&list, &mut pixmap, 1.0);
        assert!(
            (pixmap.data()[3] as i32 - expected).abs() <= 1,
            "operator {operator}"
        );
    }
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
        has_scroll_dependent_sticky: false,
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
        has_scroll_dependent_sticky: false,
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
fn border_image_fill_center_repeats_on_both_axes() {
    let mut data = vec![0; 4 * 4 * 4];
    for (x, y, color) in [
        (1, 1, [255, 0, 0, 255]),
        (2, 1, [0, 255, 0, 255]),
        (1, 2, [0, 0, 255, 255]),
        (2, 2, [255, 255, 0, 255]),
    ] {
        data[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].copy_from_slice(&color);
    }
    let list = DisplayList {
        commands: vec![PaintCmd::BorderImage {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            widths: [1.0; 4],
            slices: [1.0; 4],
            repeat_x_mode: 1,
            repeat_y_mode: 1,
            fill_center: true,
            data: ImageRef::Owned(data, 4, 4),
        }],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(10, 10).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let rgba = pixmap.data();
    let pixel = |x: usize, y: usize| &rgba[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
    assert_eq!(
        pixel(4, 4),
        pixel(6, 6),
        "the center image must tile on both axes"
    );
    assert!(pixel(4, 4)[3] > 200, "the filled center must be painted");
}

#[test]
fn border_image_tiles_visible_region_behind_translation() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushTransform {
                node_id: 1,
                transform: [1.0, 0.0, 0.0, 1.0, -5000.0, 0.0],
            },
            PaintCmd::BorderImage {
                rect: Rect::new(5000.0, 0.0, 10_000.0, 1000.0),
                widths: [1.0; 4],
                slices: [1.0; 4],
                repeat_x_mode: 1,
                repeat_y_mode: 1,
                fill_center: true,
                data: ImageRef::Owned([255, 0, 0, 255].repeat(9), 3, 3),
            },
            PaintCmd::PopTransform,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(60, 40).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let painted = pixmap.pixel(30, 20).unwrap();
    assert!(painted.red() > 240 && painted.alpha() > 240);
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
fn border_image_math_and_env_slices_reach_paint_geometry() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <div style="width:40px;height:20px;border:4px solid transparent;
                         border-image-source:url('data:image/svg+xml,%3Csvg%20viewBox=%220%200%203%203%22%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3Crect%20width=%223%22%20height=%223%22%20fill=%22red%22/%3E%3C/svg%3E');
                         border-image-slice:calc(1 + 1) env(--slice, calc(50%)) fill"></div>
           </body>"#,
    );
    let slices = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BorderImage { slices, .. } => Some(*slices),
            _ => None,
        })
        .expect("expected border-image display command");
    assert_eq!(slices, [2.0, 1.5, 2.0, 1.5]);
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
fn border_image_auto_width_uses_corresponding_natural_slice() {
    let (_frame, list) = build_full(
        r#"<body style="margin:0">
             <div style="width:40px;height:20px;border:4px solid transparent;
                         border-image-source:url('data:image/svg+xml,%3Csvg%20viewBox=%220%200%203%203%22%20xmlns=%22http://www.w3.org/2000/svg%22%3E%3Crect%20width=%223%22%20height=%223%22%20fill=%22red%22/%3E%3C/svg%3E');
                         border-image-slice:1;
                         border-image-width:auto 7px auto 9px"></div>
           </body>"#,
    );
    let widths = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BorderImage { widths, .. } => Some(*widths),
            _ => None,
        })
        .expect("expected border-image command");
    assert_eq!(widths, [1.0, 7.0, 1.0, 9.0]);
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
        has_scroll_dependent_sticky: false,
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
fn straight_border_segments_paint_dashed_dotted_and_double_styles() {
    let render = |style: u8, horizontal: bool| {
        let rect = if horizontal {
            Rect::new(2.0, 2.0, 40.0, 6.0)
        } else {
            Rect::new(2.0, 2.0, 6.0, 40.0)
        };
        let side = if horizontal { 0 } else { 3 };
        let mut widths = [0.0; 4];
        let mut colors = [Color::TRANSPARENT; 4];
        let mut styles = [0; 4];
        widths[side] = 6.0;
        colors[side] = Color::rgba(0, 180, 0, 255);
        styles[side] = style;
        let list = DisplayList {
            commands: vec![PaintCmd::Border {
                rect,
                widths,
                colors,
                styles,
                radii: [0.0; 4],
                radii_y: [0.0; 4],
                opacity: 1.0,
            }],
            has_scroll_dependent_sticky: false,
            fixed_commands: Vec::new(),
        };
        let mut pixmap = tiny_skia::Pixmap::new(48, 48).unwrap();
        replay(&list, &mut pixmap, 1.0);
        pixmap
    };
    let alpha =
        |pixmap: &tiny_skia::Pixmap, x: usize, y: usize| pixmap.data()[(y * 48 + x) * 4 + 3];

    let dashed = render(2, true);
    assert!(alpha(&dashed, 10, 5) > 200);
    assert_eq!(alpha(&dashed, 23, 5), 0);
    let vertical_dash = render(2, false);
    assert!(alpha(&vertical_dash, 5, 10) > 200);
    assert_eq!(alpha(&vertical_dash, 5, 23), 0);

    let dotted = render(3, true);
    assert!(alpha(&dotted, 5, 5) > 200);
    assert_eq!(alpha(&dotted, 11, 5), 0);

    let double = render(4, true);
    assert!(alpha(&double, 10, 3) > 200);
    assert_eq!(alpha(&double, 10, 5), 0);
    assert!(alpha(&double, 10, 7) > 200);
}

#[test]
fn ordinary_patterned_borders_preserve_gaps_on_all_sides_and_scales() {
    for scale in [1.0, 2.0] {
        for radius in [0.0, 16.0] {
            for style in [2, 3] {
                let list = DisplayList {
                    commands: vec![PaintCmd::Border {
                        rect: Rect::new(8.0, 8.0, 96.0, 64.0),
                        widths: [6.0; 4],
                        colors: [Color::rgba(0, 180, 0, 255); 4],
                        styles: [style; 4],
                        radii: [radius; 4],
                        radii_y: [radius; 4],
                        opacity: 1.0,
                    }],
                    has_scroll_dependent_sticky: false,
                    fixed_commands: Vec::new(),
                };
                let mut pixmap =
                    tiny_skia::Pixmap::new((120.0 * scale) as u32, (88.0 * scale) as u32).unwrap();
                replay(&list, &mut pixmap, scale);
                let alpha = |x: f32, y: f32| {
                    let x = (x * scale) as usize;
                    let y = (y * scale) as usize;
                    pixmap.data()[(y * pixmap.width() as usize + x) * 4 + 3]
                };
                for side in 0..4 {
                    let values: Vec<_> = (26..54)
                        .map(|at| match side {
                            0 => alpha(at as f32, 11.0),
                            1 => alpha(101.0, at as f32),
                            2 => alpha(at as f32, 69.0),
                            _ => alpha(11.0, at as f32),
                        })
                        .collect();
                    assert!(
                        values.iter().any(|alpha| *alpha > 200),
                        "paint: {style}/{radius}/{scale}/{side}"
                    );
                    assert!(
                        values.iter().any(|alpha| *alpha == 0),
                        "gaps: {style}/{radius}/{scale}/{side}"
                    );
                }
                assert_eq!(alpha(56.0, 40.0), 0, "border must not fill its interior");
                assert_eq!(alpha(5.0, 40.0), 0, "border must not escape its outer box");
            }
        }
    }
}

#[test]
fn mixed_border_patterns_do_not_turn_solid_or_paint_missing_sides() {
    let list = DisplayList {
        commands: vec![PaintCmd::Border {
            rect: Rect::new(8.0, 8.0, 96.0, 64.0),
            widths: [6.0, 6.0, 6.0, 0.0],
            colors: [Color::rgba(0, 180, 0, 255); 4],
            styles: [2, 3, 1, 0],
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            opacity: 1.0,
        }],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(120, 88).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha = |x: usize, y: usize| pixmap.data()[(y * 120 + x) * 4 + 3];
    for side in [0, 1] {
        let values: Vec<_> = (26..54)
            .map(|at| {
                if side == 0 {
                    alpha(at, 11)
                } else {
                    alpha(101, at)
                }
            })
            .collect();
        assert!(values.iter().any(|alpha| *alpha > 200));
        assert!(values.iter().any(|alpha| *alpha == 0));
    }
    for x in 26..54 {
        assert!(alpha(x, 69) > 200);
    }
    assert_eq!(alpha(9, 40), 0);
}

#[test]
fn uniform_double_borders_keep_two_stripes_and_a_gap() {
    for scale in [1.0, 2.0] {
        for radius in [0.0, 16.0] {
            let list = DisplayList {
                commands: vec![PaintCmd::Border {
                    rect: Rect::new(8.0, 8.0, 96.0, 64.0),
                    widths: [6.0; 4],
                    colors: [Color::rgba(0, 180, 0, 255); 4],
                    styles: [4; 4],
                    radii: [radius; 4],
                    radii_y: [radius; 4],
                    opacity: 1.0,
                }],
                has_scroll_dependent_sticky: false,
                fixed_commands: Vec::new(),
            };
            let mut pixmap =
                tiny_skia::Pixmap::new((120.0 * scale) as u32, (88.0 * scale) as u32).unwrap();
            replay(&list, &mut pixmap, scale);
            let alpha = |x: f32, y: f32| {
                pixmap.data()[(((y * scale) as usize * pixmap.width() as usize)
                    + (x * scale) as usize)
                    * 4
                    + 3]
            };
            for (x, y) in [
                (48.0, 9.0),
                (48.0, 13.0),
                (48.0, 67.0),
                (48.0, 71.0),
                (9.0, 40.0),
                (13.0, 40.0),
                (99.0, 40.0),
                (103.0, 40.0),
            ] {
                assert!(alpha(x, y) > 200, "stripe: {radius}/{scale}/{x}/{y}");
            }
            for (x, y) in [
                (48.0, 11.0),
                (48.0, 69.0),
                (11.0, 40.0),
                (101.0, 40.0),
                (48.0, 40.0),
                (5.0, 40.0),
            ] {
                assert_eq!(alpha(x, y), 0, "gap: {radius}/{scale}/{x}/{y}");
            }
        }
    }
}

#[test]
fn patterned_border_replay_preserves_parent_clip_and_opacity() {
    let list = DisplayList {
        commands: vec![
            PaintCmd::PushClip {
                rect: Rect::new(0.0, 0.0, 48.0, 88.0),
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            },
            PaintCmd::Border {
                rect: Rect::new(8.0, 8.0, 96.0, 64.0),
                widths: [6.0; 4],
                colors: [Color::rgba(0, 180, 0, 255); 4],
                styles: [3; 4],
                radii: [16.0; 4],
                radii_y: [16.0; 4],
                opacity: 0.5,
            },
            PaintCmd::PopClip,
        ],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(120, 88).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha = |x: usize, y: usize| pixmap.data()[(y * 120 + x) * 4 + 3];
    assert!((8..48).any(|x| alpha(x, 11) > 100));
    for y in 0..88 {
        for x in 0..120 {
            assert!(alpha(x, y) <= 128, "opacity at {x},{y}");
            if x >= 48 {
                assert_eq!(alpha(x, y), 0, "parent clip at {x},{y}");
            }
        }
    }
}

#[test]
fn collapsed_table_winning_double_border_reaches_replay() {
    let (_, list) = build(
        "<style>body{margin:0}table{border-collapse:collapse}</style>\
         <table><tr><td style='width:40px;height:24px;border-right:6px double green'>A</td>\
         <td style='width:40px;border-left:1px solid red'>B</td></tr></table>",
    );
    assert!(list.commands.iter().any(|command| matches!(command,
        PaintCmd::Border { widths, colors, styles, .. }
            if widths[3] == 6.0 && styles[3] == 4 && colors[3].g > 0
    )));
}

#[test]
fn straight_border_segments_shade_raised_and_sunken_styles() {
    let render = |style: u8| {
        let list = DisplayList {
            commands: vec![PaintCmd::Border {
                rect: Rect::new(2.0, 2.0, 20.0, 6.0),
                widths: [6.0, 0.0, 0.0, 0.0],
                colors: [
                    Color::rgba(100, 100, 100, 255),
                    Color::TRANSPARENT,
                    Color::TRANSPARENT,
                    Color::TRANSPARENT,
                ],
                styles: [style, 0, 0, 0],
                radii: [0.0; 4],
                radii_y: [0.0; 4],
                opacity: 1.0,
            }],
            has_scroll_dependent_sticky: false,
            fixed_commands: Vec::new(),
        };
        let mut pixmap = tiny_skia::Pixmap::new(24, 12).unwrap();
        replay(&list, &mut pixmap, 1.0);
        let green = |y: usize| pixmap.data()[(y * 24 + 10) * 4 + 1];
        (green(3), green(6))
    };
    let groove = render(5);
    let ridge = render(6);
    assert!(groove.0 < groove.1);
    assert!(ridge.0 > ridge.1);
    assert!(render(7).0 < render(8).0);
}

#[test]
fn ordinary_square_bands_preserve_unequal_widths_and_shading_orientation() {
    for scale in [1.0, 2.0] {
        for style in 4..=8 {
            let list = DisplayList {
                commands: vec![PaintCmd::Border {
                    rect: Rect::new(8.0, 8.0, 96.0, 64.0),
                    widths: [6.0, 12.0, 9.0, 3.0],
                    colors: [Color::rgba(100, 100, 100, 255); 4],
                    styles: [style; 4],
                    radii: [0.0; 4],
                    radii_y: [0.0; 4],
                    opacity: 1.0,
                }],
                has_scroll_dependent_sticky: false,
                fixed_commands: Vec::new(),
            };
            let mut pixmap =
                tiny_skia::Pixmap::new((120.0 * scale) as u32, (88.0 * scale) as u32).unwrap();
            replay(&list, &mut pixmap, scale);
            let pixel = |x: f32, y: f32| {
                let offset =
                    (((y * scale) as usize * pixmap.width() as usize) + (x * scale) as usize) * 4;
                (pixmap.data()[offset], pixmap.data()[offset + 3])
            };
            let outer = [(48.0, 8.0), (103.0, 40.0), (48.0, 71.0), (8.0, 40.0)];
            let inner = [(48.0, 13.0), (93.0, 40.0), (48.0, 63.0), (10.0, 40.0)];
            for side in 0..4 {
                let near = pixel(outer[side].0, outer[side].1);
                let far = pixel(inner[side].0, inner[side].1);
                assert_eq!(near.1, 255, "outer alpha: {style}/{side}/{scale}");
                assert_eq!(far.1, 255, "inner alpha: {style}/{side}/{scale}");
                if style == 4 {
                    assert_eq!(near.0, 100);
                    assert_eq!(far.0, 100);
                } else {
                    let light = (style == 6 || style == 8) == matches!(side, 0 | 3);
                    assert_eq!(near.0, if light { 162 } else { 60 });
                    assert_eq!(
                        far.0,
                        if matches!(style, 5 | 6) {
                            if light { 60 } else { 162 }
                        } else {
                            near.0
                        }
                    );
                }
            }
            if style == 4 {
                for (x, y) in [(48.0, 11.0), (98.0, 40.0), (48.0, 67.0), (9.0, 40.0)] {
                    assert_eq!(pixel(x, y).1, 0, "double gap: {scale}/{x}/{y}");
                }
            }
            assert_eq!(pixel(48.0, 40.0).1, 0);
            assert_eq!(pixel(5.0, 40.0).1, 0);
        }
    }
}

#[test]
fn transparent_border_sides_shape_css_triangle() {
    let (_, built) = build(
        r#"<style>
          body { margin: 0; }
          .arrow::after { content: ""; position: absolute; left: 4px; top: 4px;
            width: 0; height: 0; border-top: 6px solid transparent;
            border-bottom: 6px solid transparent; border-left: 12px solid #666; }
        </style><div class="arrow"></div>"#,
    );
    assert!(built.commands.iter().any(|cmd| {
        matches!(cmd, PaintCmd::Border { widths, colors, .. }
            if widths[0] == 6.0 && widths[2] == 6.0 && widths[3] == 12.0
                && colors[0].a == 0 && colors[2].a == 0)
    }));

    let list = DisplayList {
        commands: vec![PaintCmd::Border {
            rect: Rect::new(4.0, 4.0, 12.0, 12.0),
            widths: [6.0, 0.0, 6.0, 12.0],
            colors: [
                Color::rgba(0, 0, 0, 0),
                Color::rgba(0, 0, 0, 0),
                Color::rgba(0, 0, 0, 0),
                Color::rgba(102, 102, 102, 255),
            ],
            styles: [1, 0, 1, 1],
            radii: [0.0; 4],
            radii_y: [0.0; 4],
            opacity: 1.0,
        }],
        has_scroll_dependent_sticky: false,
        fixed_commands: Vec::new(),
    };
    let mut pixmap = tiny_skia::Pixmap::new(20, 20).unwrap();
    replay(&list, &mut pixmap, 1.0);
    let alpha = |x: usize, y: usize| pixmap.data()[(y * 20 + x) * 4 + 3];
    assert!(alpha(6, 10) > 200, "base of triangle should paint");
    assert_eq!(alpha(14, 5), 0, "triangle corner should stay transparent");
}

#[test]
fn rounded_border_only_triangle_remains_visible() {
    let (_, list) = build(
        "<style>body{margin:0}div::before{content:'';position:absolute;left:10px;top:10px;width:0;height:0;border-right:40px solid #ffcc00;border-bottom:40px solid transparent;border-top-right-radius:12px}</style><div></div>",
    );
    let mut pixmap = tiny_skia::Pixmap::new(80, 80).unwrap();
    replay(&list, &mut pixmap, 1.0);
    assert!(
        pixmap.pixel(44, 30).unwrap().alpha() > 200,
        "yellow wedge must paint"
    );
    assert_eq!(
        pixmap.pixel(15, 45).unwrap().alpha(),
        0,
        "transparent wedge"
    );
    assert_eq!(pixmap.pixel(49, 10).unwrap().alpha(), 0, "rounded corner");
}

#[test]
fn transparent_borders_do_not_enter_the_display_list() {
    let html = r#"
        <div id="menu" style="
            width: 120px;
            height: 32px;
            border: 1px solid transparent;
            border-bottom: none;
            background: #123456;
        "></div>
    "#;
    let doc = parse_html(html);
    let mut frame = EngineFrame::new(doc, 320.0, 200.0);
    frame.update_frame();

    let list = build_display_list_full(
        &frame.doc.root,
        320.0,
        200.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Border { .. })),
        "transparent author borders must affect layout but not paint artifacts"
    );
}

#[test]
fn pending_css_mask_does_not_paint_unmasked_background() {
    let html = r#"
        <div style="
            width: 24px;
            height: 24px;
            background: white;
            mask-image: url('/missing-icon.svg');
            mask-size: contain;
        "></div>
    "#;
    let doc = parse_html(html);
    let mut frame = EngineFrame::new(doc, 120.0, 80.0);
    frame.update_frame();

    let list = build_display_list_full(
        &frame.doc.root,
        120.0,
        80.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
    );

    assert!(
        !list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::FillRect { rect, color, .. }
                    if rect.w == 24.0 && rect.h == 24.0 && color.a > 0
            )
        }),
        "a pending CSS mask must be transparent instead of flashing a raw rectangle"
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
