//! Native `<audio>` / `<video>` control painting.
//!
//! The media state lives in `video::Document` extensions; this file owns the
//! display-list representation of controls and poster frames.

use crate::renderer::display_list::{DisplayList, ImageRef, PaintCmd, TextDecoration};
use crate::renderer::display_list_builder::object_fit_rect;
use crate::types::{Color, ComputedStyle, FontStyle, Rect, TextDecorationStyle, WebCore};

#[derive(Clone, Copy, Debug)]
pub(crate) struct MediaControlLayout {
    pub rail_y: f32,
    pub rail_h: f32,
    pub timeline_x: f32,
    pub timeline_w: f32,
}

pub(crate) fn media_control_layout(node: &WebCore) -> Option<MediaControlLayout> {
    if !node.attributes.contains_key("controls") || !matches!(node.tag.as_str(), "audio" | "video")
    {
        return None;
    }
    let cr = node.layout.content_rect;
    if cr.w <= 0.0 || cr.h <= 0.0 {
        return None;
    }
    let is_video = node.tag == "video";
    let rail_h = if is_video {
        34.0_f32.min(cr.h)
    } else {
        cr.h.min(42.0)
    };
    let rail_y = if is_video { cr.y + cr.h - rail_h } else { cr.y };
    let timeline_x = cr.x + 54.0;
    let timeline_w = (cr.w - 116.0).max(0.0);
    Some(MediaControlLayout {
        rail_y,
        rail_h,
        timeline_x,
        timeline_w,
    })
}

pub(crate) fn build_media_element(node: &WebCore, list: &mut DisplayList, sx: f32, sy: f32) {
    let cr = node.layout.content_rect;
    if cr.w <= 0.0 || cr.h <= 0.0 {
        return;
    }

    let x = cr.x - sx;
    let y = cr.y - sy;
    let rect = Rect::new(x, y, cr.w, cr.h);
    let font_px = node.style.font_size_px(16.0, 16.0).clamp(10.0, 18.0);
    let is_video = node.tag == "video";
    let controls = node.attributes.contains_key("controls");
    let poster = node
        .attributes
        .get("poster")
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let label = if is_video && poster {
        "Video"
    } else if is_video {
        "No video frame"
    } else {
        "Audio"
    };

    let has_video_pixels = node.image_data.is_some() || node.svg_document.is_some();
    if !is_video || controls || has_video_pixels {
        list.push(PaintCmd::FillRect {
            rect,
            color: if is_video {
                Color::rgba(16, 18, 24, 255)
            } else {
                Color::rgba(245, 247, 250, 255)
            },
            radius: [4.0; 4],
            radius_y: [4.0; 4],
        });
    }

    if is_video {
        paint_video_poster(node, list, sx, sy, cr, rect, font_px);
    }

    if is_video
        && controls
        && cr.h >= 70.0
        && node.image_data.is_none()
        && node.svg_document.is_none()
    {
        let text_y = y + (cr.h - font_px) * 0.5;
        emit_media_text(
            list,
            x + 12.0,
            text_y,
            label,
            font_px,
            Color::rgba(220, 226, 235, 220),
            &node.style,
        );
    }

    if controls {
        paint_media_controls(node, list, x, y, cr, font_px, is_video);
    }
}

fn paint_video_poster(
    node: &WebCore,
    list: &mut DisplayList,
    sx: f32,
    sy: f32,
    cr: Rect,
    rect: Rect,
    font_px: f32,
) {
    if let Some(ref data) = node.image_data {
        if node.image_width > 0 && node.image_height > 0 {
            let (dst, _clip) = object_fit_rect(
                &node.style,
                cr,
                node.image_width as f32,
                node.image_height as f32,
                font_px,
                16.0,
            );
            list.push(PaintCmd::PushClip {
                rect,
                radius: [4.0; 4],
                radius_y: [4.0; 4],
            });
            list.push(PaintCmd::Image {
                rect: Rect::new(dst.x - sx, dst.y - sy, dst.w, dst.h),
                data: ImageRef::Shared(data.clone(), node.image_width, node.image_height),
            });
            list.push(PaintCmd::PopClip);
        }
    } else if let Some(ref svg_doc) = node.svg_document {
        let raster_w = cr.w.round() as u32;
        let raster_h = cr.h.round() as u32;
        if raster_w > 0 && raster_h > 0 {
            if let Some(rgba) = crate::svg::rasterize_svg_document_to_rgba(
                svg_doc,
                raster_w,
                raster_h,
                (node.svg_viewbox_w, node.svg_viewbox_h),
                Color::BLACK,
                Some(Color::BLACK),
                None,
            ) {
                list.push(PaintCmd::PushClip {
                    rect,
                    radius: [4.0; 4],
                    radius_y: [4.0; 4],
                });
                list.push(PaintCmd::Image {
                    rect,
                    data: ImageRef::Owned(rgba, raster_w, raster_h),
                });
                list.push(PaintCmd::PopClip);
            }
        }
    }
}

fn paint_media_controls(
    node: &WebCore,
    list: &mut DisplayList,
    x: f32,
    y: f32,
    cr: Rect,
    font_px: f32,
    is_video: bool,
) {
    let Some(control) = media_control_layout(node) else {
        return;
    };
    let rail_y = control.rail_y - node.layout.content_rect.y + y;
    list.push(PaintCmd::FillRect {
        rect: Rect::new(x, rail_y, cr.w, control.rail_h),
        color: if is_video {
            Color::rgba(0, 0, 0, 185)
        } else {
            Color::rgba(231, 236, 243, 255)
        },
        radius: if is_video { [0.0; 4] } else { [4.0; 4] },
        radius_y: if is_video { [0.0; 4] } else { [4.0; 4] },
    });

    emit_media_text(
        list,
        x + 10.0,
        rail_y + (control.rail_h - font_px) * 0.5,
        if node.media_paused { "Play" } else { "Pause" },
        font_px,
        if is_video { Color::WHITE } else { Color::BLACK },
        &node.style,
    );

    let timeline_x = control.timeline_x - node.layout.content_rect.x + x;
    if control.timeline_w > 8.0 {
        paint_media_timeline(
            node,
            list,
            timeline_x,
            rail_y,
            control.rail_h,
            control.timeline_w,
            is_video,
        );
    }

    let duration = node
        .media_duration
        .or_else(|| markup_duration(node))
        .map(format_media_time)
        .unwrap_or_else(|| "--:--".to_string());
    emit_media_text(
        list,
        (x + cr.w - 52.0).max(x + 4.0),
        rail_y + (control.rail_h - font_px) * 0.5,
        &duration,
        font_px,
        if is_video { Color::WHITE } else { Color::BLACK },
        &node.style,
    );
}

fn paint_media_timeline(
    node: &WebCore,
    list: &mut DisplayList,
    timeline_x: f32,
    rail_y: f32,
    rail_h: f32,
    timeline_w: f32,
    is_video: bool,
) {
    let timeline_y = rail_y + rail_h * 0.5 - 2.0;
    list.push(PaintCmd::FillRect {
        rect: Rect::new(timeline_x, timeline_y, timeline_w, 4.0),
        color: if is_video {
            Color::rgba(180, 188, 198, 150)
        } else {
            Color::rgba(110, 120, 135, 170)
        },
        radius: [2.0; 4],
        radius_y: [2.0; 4],
    });
    if let Some(duration) = node.media_duration {
        if duration > 0.0 {
            let progress = (node.media_current_time / duration).clamp(0.0, 1.0);
            list.push(PaintCmd::FillRect {
                rect: Rect::new(
                    timeline_x,
                    timeline_y,
                    (timeline_w * progress).max(4.0),
                    4.0,
                ),
                color: Color::rgba(255, 255, 255, 230),
                radius: [2.0; 4],
                radius_y: [2.0; 4],
            });
        }
    } else {
        list.push(PaintCmd::FillRect {
            rect: Rect::new(timeline_x, timeline_y, 4.0, 4.0),
            color: Color::rgba(255, 255, 255, 230),
            radius: [2.0; 4],
            radius_y: [2.0; 4],
        });
    }
}

fn markup_duration(node: &WebCore) -> Option<f32> {
    node.attributes
        .get("data-duration")
        .or_else(|| node.attributes.get("duration"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn emit_media_text(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    text: &str,
    font_px: f32,
    color: Color,
    base: &ComputedStyle,
) {
    if text.is_empty() || font_px <= 0.0 {
        return;
    }
    let fp = font_px.max(1.0);
    let letter_sp = base.letter_spacing.resolve(fp, 0.0, 16.0);
    let word_sp = base.word_spacing.resolve(fp, 0.0, 16.0);
    let deco_t = base.text_decoration_thickness.resolve(fp, 0.0, 16.0);
    let underline_offset = base.text_underline_offset.resolve(fp, 0.0, 16.0);
    list.push(PaintCmd::Text {
        x,
        y,
        text: text.to_string(),
        font_family: base.font_family.clone(),
        font_size: fp,
        font_weight: base.font_weight.value(),
        font_style: match base.font_style {
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
            _ => 0,
        },
        font_stretch: base.font_stretch,
        line_height: fp * 1.2,
        color,
        decoration: TextDecoration {
            underline: base.text_decoration.underline,
            overline: base.text_decoration.overline,
            strikethrough: base.text_decoration.strikethrough,
            color: base.text_decoration_color.unwrap_or(color),
            style: match base.text_decoration_style {
                TextDecorationStyle::Double => 1,
                TextDecorationStyle::Dotted => 2,
                TextDecorationStyle::Dashed => 3,
                TextDecorationStyle::Wavy => 4,
                _ => 0,
            },
            thickness: if deco_t > 0.0 { deco_t } else { 1.0 },
            underline_offset,
            underline_position: base.text_underline_position,
            skip_ink: !base.text_decoration_skip_ink.eq_ignore_ascii_case("none"),
        },
        letter_spacing: letter_sp,
        word_spacing: word_sp,
        small_caps: base.small_caps,
    });
}

fn format_media_time(seconds: f32) -> String {
    let total = seconds.round().max(0.0) as u32;
    format!("{}:{:02}", total / 60, total % 60)
}
