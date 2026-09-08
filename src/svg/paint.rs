//! Native SVG rasterization.

use super::geometry::{
    intrinsic_size_from_markup, parse_preserve_aspect_ratio, parse_svg_length, parse_view_box,
    AlignX, AlignY, PreserveAspectRatio, SvgLength, SvgViewBox,
};
use super::{parse_svg_document, SvgDocument, SvgElementKind, SvgNode};
use crate::canvas::{
    Canvas, Font, FontStyle, FontWeight, Matrix, TextAlign, TextBaseline, TinySkiaCanvas,
};
use crate::css::{
    parse_color, parse_stylesheet, resolve_var_references, AttrOp, Combinator, CssRule,
    CssSelector, Declarations, SelectorPart,
};
use crate::types::Color;
use std::collections::HashMap;
use tiny_skia::{
    FillRule, GradientStop as SkGradientStop, LineCap, LineJoin, LinearGradient, Mask, MaskType,
    Paint, Path, PathBuilder, PathSegment, Pixmap, PixmapPaint, PixmapRef, Point as SkPoint,
    PremultipliedColorU8, RadialGradient, SpreadMode, Stroke, StrokeDash, Transform,
};

#[derive(Clone)]
enum PaintSource {
    Color(Color),
    Url(String, Option<Color>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaintOp {
    Fill,
    Stroke,
}

#[derive(Clone)]
struct PaintState {
    visible: bool,
    current_color: Color,
    fill: Option<PaintSource>,
    stroke: Option<PaintSource>,
    custom_props: HashMap<String, String>,
    font_family: String,
    font_size: f32,
    font_weight: FontWeight,
    font_style: FontStyle,
    letter_spacing: f32,
    word_spacing: f32,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    baseline_shift: f32,
    viewport_width: f32,
    viewport_height: f32,
    overflow_visible: bool,
    stroke_width: f32,
    stroke_linecap: LineCap,
    stroke_linejoin: LineJoin,
    stroke_miterlimit: f32,
    stroke_dasharray: Option<Vec<f32>>,
    stroke_dashoffset: f32,
    fill_rule: FillRule,
    paint_order: [PaintOp; 2],
    opacity: f32,
}

impl Default for PaintState {
    fn default() -> Self {
        Self {
            visible: true,
            current_color: Color::BLACK,
            fill: Some(PaintSource::Color(Color::BLACK)),
            stroke: None,
            custom_props: HashMap::new(),
            font_family: "sans-serif".to_string(),
            font_size: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            baseline_shift: 0.0,
            viewport_width: 0.0,
            viewport_height: 0.0,
            overflow_visible: false,
            stroke_width: 1.0,
            stroke_linecap: LineCap::Butt,
            stroke_linejoin: LineJoin::Miter,
            stroke_miterlimit: 4.0,
            stroke_dasharray: None,
            stroke_dashoffset: 0.0,
            fill_rule: FillRule::Winding,
            paint_order: [PaintOp::Fill, PaintOp::Stroke],
            opacity: 1.0,
        }
    }
}

pub fn rasterize_svg_to_rgba(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    let doc = parse_svg_document(svg).ok()?;
    rasterize_svg_document_to_rgba(
        &doc,
        width,
        height,
        intrinsic_size_from_markup(svg),
        Color::BLACK,
        Some(Color::BLACK),
        None,
    )
}

pub fn rasterize_svg_document_to_rgba(
    doc: &SvgDocument,
    width: u32,
    height: u32,
    intrinsic_size: (f32, f32),
    current_color: Color,
    fill: Option<Color>,
    stroke: Option<Color>,
) -> Option<Vec<u8>> {
    rasterize_svg_document_to_rgba_with_vars(
        doc,
        width,
        height,
        intrinsic_size,
        current_color,
        fill,
        stroke,
        &HashMap::new(),
    )
}

pub(crate) fn rasterize_svg_document_to_rgba_with_vars(
    doc: &SvgDocument,
    width: u32,
    height: u32,
    intrinsic_size: (f32, f32),
    current_color: Color,
    fill: Option<Color>,
    stroke: Option<Color>,
    custom_props: &HashMap<String, String>,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let mut pixmap = Pixmap::new(width, height)?;
    let (iw, ih) = intrinsic_size;
    let vb = doc
        .root
        .attr_ascii_case_insensitive("viewBox")
        .and_then(|v| parse_view_box(Some(v)));
    let transform = if let Some(vb) = vb {
        view_box_transform(
            vb,
            width as f32,
            height as f32,
            parse_preserve_aspect_ratio(
                doc.root.attr_ascii_case_insensitive("preserveAspectRatio"),
            ),
        )
    } else {
        let sx = if iw > 0.0 { width as f32 / iw } else { 1.0 };
        let sy = if ih > 0.0 { height as f32 / ih } else { 1.0 };
        Transform::from_row(sx, 0.0, 0.0, sy, 0.0, 0.0)
    };
    let mut id_map = HashMap::new();
    collect_id_nodes(&doc.root, &mut id_map);
    let styles = collect_style_rules(&doc.root);
    let mut initial_state = PaintState::default();
    initial_state.current_color = current_color;
    initial_state.fill = fill.map(PaintSource::Color);
    initial_state.stroke = stroke.map(PaintSource::Color);
    initial_state.custom_props = custom_props.clone();
    let (viewport_width, viewport_height) = viewport_user_size(vb, iw, ih, width, height);
    initial_state.viewport_width = viewport_width;
    initial_state.viewport_height = viewport_height;
    paint_node(
        &doc.root,
        &mut pixmap,
        initial_state,
        transform,
        &id_map,
        &styles,
        &mut Vec::new(),
        &mut Vec::new(),
        None,
        true,
    );
    Some(pixmap.data().to_vec())
}

pub fn rasterize_svg_intrinsic(svg: &str) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h) = intrinsic_size_from_markup(svg);
    let w = w.ceil().max(1.0) as u32;
    let h = h.ceil().max(1.0) as u32;
    rasterize_svg_to_rgba(svg, w, h).map(|rgba| (rgba, w, h))
}

fn collect_id_nodes<'a>(node: &'a SvgNode, ids: &mut HashMap<String, &'a SvgNode>) {
    if let Some(id) = node.attr("id") {
        ids.insert(id.to_string(), node);
    }
    for child in &node.children {
        collect_id_nodes(child, ids);
    }
}

fn collect_style_rules(node: &SvgNode) -> Vec<CssRule> {
    let mut rules = Vec::new();
    collect_style_rules_into(node, &mut rules);
    rules
}

fn collect_style_rules_into(node: &SvgNode, rules: &mut Vec<CssRule>) {
    if matches!(node.kind, SvgElementKind::Style) {
        if let Some(parsed) = parse_stylesheet(&node.text) {
            rules.extend(parsed);
        }
    }
    for child in &node.children {
        collect_style_rules_into(child, rules);
    }
}

fn paint_node<'a>(
    node: &'a SvgNode,
    pixmap: &mut Pixmap,
    inherited: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
    allow_filter: bool,
) {
    let mut state = state_for_node(node, inherited, styles, ancestors);
    if !state.visible {
        return;
    }
    let mut transform = node
        .attr_ascii_case_insensitive("transform")
        .and_then(parse_transform_list)
        .map(|local| transform.pre_concat(local))
        .unwrap_or(transform);
    let mut nested_viewport_clip = None;
    if matches!(node.kind, SvgElementKind::Svg) && !ancestors.is_empty() {
        let viewport_transform = transform;
        let viewport_w = attr_length(node, "width", LengthAxis::X, &state)
            .unwrap_or(state.viewport_width)
            .max(0.0);
        let viewport_h = attr_length(node, "height", LengthAxis::Y, &state)
            .unwrap_or(state.viewport_height)
            .max(0.0);
        let x = attr_length(node, "x", LengthAxis::X, &state).unwrap_or(0.0);
        let y = attr_length(node, "y", LengthAxis::Y, &state).unwrap_or(0.0);
        state.viewport_width = viewport_w;
        state.viewport_height = viewport_h;
        if !state.overflow_visible {
            nested_viewport_clip = nested_svg_viewport_mask(
                pixmap.width(),
                pixmap.height(),
                viewport_transform,
                x,
                y,
                viewport_w,
                viewport_h,
                clip,
            );
        }
        transform = transform.pre_translate(x, y);
        if let Some(vb) = node
            .attr_ascii_case_insensitive("viewBox")
            .and_then(|value| parse_view_box(Some(value)))
        {
            transform = transform.pre_concat(view_box_transform(
                vb,
                viewport_w,
                viewport_h,
                parse_preserve_aspect_ratio(
                    node.attr_ascii_case_insensitive("preserveAspectRatio"),
                ),
            ));
        }
    }
    let local_mask = compositing_mask_for_node(
        node,
        &state,
        pixmap.width(),
        pixmap.height(),
        transform,
        ids,
        styles,
        ancestors,
        stack,
        nested_viewport_clip.as_ref().or(clip),
    );
    let active_clip = local_mask
        .as_ref()
        .or(nested_viewport_clip.as_ref())
        .or(clip);
    if allow_filter {
        if let Some(id) = node.attr("filter").and_then(parse_url_id) {
            if !stack.iter().any(|seen| seen == id) {
                if let Some(filter_node) =
                    ids.get(id).copied().filter(|node| is_svg_filter_node(node))
                {
                    let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                        return;
                    };
                    stack.push(id.to_string());
                    paint_node(
                        node,
                        &mut layer,
                        state.clone(),
                        transform,
                        ids,
                        styles,
                        ancestors,
                        stack,
                        active_clip,
                        false,
                    );
                    stack.pop();
                    apply_svg_filter(&mut layer, filter_node, &state);
                    let paint = PixmapPaint::default();
                    pixmap.draw_pixmap(
                        0,
                        0,
                        layer.as_ref(),
                        &paint,
                        Transform::identity(),
                        active_clip,
                    );
                    return;
                }
            }
        }
    }
    if matches!(
        node.kind,
        SvgElementKind::Svg | SvgElementKind::Group | SvgElementKind::Unknown(_)
    ) && state.opacity < 0.999
    {
        let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        let mut child_state = state.clone();
        child_state.opacity = 1.0;
        paint_children(
            node,
            &mut layer,
            child_state,
            transform,
            ids,
            styles,
            ancestors,
            stack,
            active_clip,
            allow_filter,
        );
        let paint = PixmapPaint {
            opacity: state.opacity.clamp(0.0, 1.0),
            blend_mode: tiny_skia::BlendMode::SourceOver,
            quality: tiny_skia::FilterQuality::Nearest,
        };
        pixmap.draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &paint,
            Transform::identity(),
            active_clip,
        );
        return;
    }
    match node.kind {
        SvgElementKind::Svg | SvgElementKind::Group | SvgElementKind::Unknown(_) => {}
        SvgElementKind::Defs | SvgElementKind::Symbol | SvgElementKind::Style => return,
        SvgElementKind::Use => {
            paint_use(
                node,
                pixmap,
                state.clone(),
                transform,
                ids,
                styles,
                ancestors,
                stack,
                active_clip,
            );
            return;
        }
        SvgElementKind::Rect => {
            if let Some(path) = rect_path(node, &state) {
                paint_path(
                    pixmap,
                    &path,
                    &state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
        }
        SvgElementKind::Circle => {
            if let Some(path) = circle_path(node, &state) {
                paint_path(
                    pixmap,
                    &path,
                    &state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
        }
        SvgElementKind::Ellipse => {
            if let Some(path) = ellipse_path(node, &state) {
                paint_path(
                    pixmap,
                    &path,
                    &state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
        }
        SvgElementKind::Line => {
            if let Some(path) = line_path(node, &state) {
                paint_path(
                    pixmap,
                    &path,
                    &state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
            paint_line_markers(
                node,
                pixmap,
                state.clone(),
                transform,
                ids,
                styles,
                ancestors,
                stack,
                active_clip,
            );
        }
        SvgElementKind::Polyline => {
            let points = points_list(node, &state);
            if let Some(path) = points_path_from_pairs(&points, false) {
                let mut line_state = state.clone();
                line_state.fill = None;
                paint_path(
                    pixmap,
                    &path,
                    &line_state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
            paint_poly_markers(
                node,
                &points,
                false,
                pixmap,
                state.clone(),
                transform,
                ids,
                styles,
                ancestors,
                stack,
                active_clip,
            );
        }
        SvgElementKind::Polygon => {
            let points = points_list(node, &state);
            if let Some(path) = points_path_from_pairs(&points, true) {
                paint_path(
                    pixmap,
                    &path,
                    &state,
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
            paint_poly_markers(
                node,
                &points,
                true,
                pixmap,
                state.clone(),
                transform,
                ids,
                styles,
                ancestors,
                stack,
                active_clip,
            );
        }
        SvgElementKind::Path => {
            if let Some(d) = node.attr("d").or_else(|| node.attr("D")) {
                if let Some(path) = parse_path_data(d) {
                    paint_path(
                        pixmap,
                        &path,
                        &state,
                        transform,
                        ids,
                        styles,
                        ancestors,
                        stack,
                        active_clip,
                    );
                }
                let (points, closed) = path_marker_points(d);
                paint_poly_markers(
                    node,
                    &points,
                    closed,
                    pixmap,
                    state.clone(),
                    transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    active_clip,
                );
            }
        }
        SvgElementKind::Image => {
            paint_image(node, pixmap, &state, transform, active_clip);
        }
        SvgElementKind::Text | SvgElementKind::Tspan | SvgElementKind::TextPath => {
            paint_text(
                node,
                pixmap,
                &state,
                transform,
                active_clip,
                ids,
                styles,
                ancestors,
            );
            return;
        }
        _ => {}
    }
    paint_children(
        node,
        pixmap,
        state,
        transform,
        ids,
        styles,
        ancestors,
        stack,
        active_clip,
        allow_filter,
    );
}

fn paint_children<'a>(
    node: &'a SvgNode,
    pixmap: &mut Pixmap,
    state: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
    allow_filter: bool,
) {
    ancestors.push(node);
    for child in &node.children {
        paint_node(
            child,
            pixmap,
            state.clone(),
            transform,
            ids,
            styles,
            ancestors,
            stack,
            clip,
            allow_filter,
        );
    }
    ancestors.pop();
}

fn paint_text<'a>(
    node: &'a SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    clip: Option<&Mask>,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
) {
    if !has_text_content(node) {
        return;
    }
    let mut cursor = TextCursor {
        x: attr_length(node, "x", LengthAxis::X, state).unwrap_or(0.0),
        y: attr_length(node, "y", LengthAxis::Y, state).unwrap_or(0.0),
    };
    cursor.x += attr_length(node, "dx", LengthAxis::X, state).unwrap_or(0.0);
    cursor.y += attr_length(node, "dy", LengthAxis::Y, state).unwrap_or(0.0);
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    if let Some(clip) = clip {
        let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        paint_text_tree(
            node,
            &mut layer,
            state,
            transform,
            ids,
            styles,
            ancestors,
            &mut cursor,
            &mut font_system,
            &mut swash_cache,
        );
        let pp = PixmapPaint::default();
        pixmap.draw_pixmap(0, 0, layer.as_ref(), &pp, Transform::identity(), Some(clip));
    } else {
        paint_text_tree(
            node,
            pixmap,
            state,
            transform,
            ids,
            styles,
            ancestors,
            &mut cursor,
            &mut font_system,
            &mut swash_cache,
        );
    }
}

struct TextCursor {
    x: f32,
    y: f32,
}

fn paint_text_tree<'a>(
    node: &'a SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    cursor: &mut TextCursor,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut cosmic_text::SwashCache,
) {
    if !node.text.is_empty() {
        let target_length = attr_length(node, "textLength", LengthAxis::X, state);
        let length_adjust = node
            .attr_ascii_case_insensitive("lengthAdjust")
            .unwrap_or("spacing")
            .trim();
        if has_text_position_lists(node) {
            let _ = paint_positioned_text_onto(
                pixmap,
                state,
                transform,
                node,
                &node.text,
                cursor,
                font_system,
                swash_cache,
            );
        } else {
            let advance = paint_text_onto(
                pixmap,
                state,
                transform,
                &node.text,
                cursor.x,
                cursor.y,
                target_length,
                length_adjust,
                font_system,
                swash_cache,
            );
            cursor.x += advance;
        }
    }

    ancestors.push(node);
    for child in &node.children {
        if !matches!(
            child.kind,
            SvgElementKind::Text | SvgElementKind::Tspan | SvgElementKind::TextPath
        ) {
            continue;
        }
        let child_state = state_for_node(child, state.clone(), styles, ancestors);
        if matches!(child.kind, SvgElementKind::TextPath) {
            cursor.x += paint_text_path(
                child,
                pixmap,
                &child_state,
                transform,
                ids,
                font_system,
                swash_cache,
            );
            continue;
        }
        let old_cursor = TextCursor {
            x: cursor.x,
            y: cursor.y,
        };
        if let Some(x) = attr_length(child, "x", LengthAxis::X, &child_state) {
            cursor.x = x;
        }
        if let Some(y) = attr_length(child, "y", LengthAxis::Y, &child_state) {
            cursor.y = y;
        }
        cursor.x += attr_length(child, "dx", LengthAxis::X, &child_state).unwrap_or(0.0);
        cursor.y += attr_length(child, "dy", LengthAxis::Y, &child_state).unwrap_or(0.0);
        paint_text_tree(
            child,
            pixmap,
            &child_state,
            transform,
            ids,
            styles,
            ancestors,
            cursor,
            font_system,
            swash_cache,
        );
        if child.attr("x").is_some() || child.attr("y").is_some() {
            cursor.y = old_cursor.y;
        }
    }
    ancestors.pop();
}

fn has_text_position_lists(node: &SvgNode) -> bool {
    ["x", "y", "dx", "dy"].iter().any(|name| {
        node.attr_ascii_case_insensitive(name)
            .is_some_and(|value| svg_text_length_list(value).len() > 1)
    })
}

fn paint_positioned_text_onto(
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    node: &SvgNode,
    text: &str,
    cursor: &mut TextCursor,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut cosmic_text::SwashCache,
) -> f32 {
    let xs = node
        .attr_ascii_case_insensitive("x")
        .map(svg_text_length_list)
        .unwrap_or_default();
    let ys = node
        .attr_ascii_case_insensitive("y")
        .map(svg_text_length_list)
        .unwrap_or_default();
    let dxs = node
        .attr_ascii_case_insensitive("dx")
        .map(svg_text_length_list)
        .unwrap_or_default();
    let dys = node
        .attr_ascii_case_insensitive("dy")
        .map(svg_text_length_list)
        .unwrap_or_default();
    let mut paint_state = state.clone();
    paint_state.text_align = TextAlign::Start;
    let mut total = 0.0;
    for (i, ch) in text.chars().enumerate() {
        if let Some(x) = xs.get(i).copied() {
            cursor.x = x;
        }
        if let Some(y) = ys.get(i).copied() {
            cursor.y = y;
        }
        cursor.x += dxs.get(i).copied().unwrap_or(0.0);
        cursor.y += dys.get(i).copied().unwrap_or(0.0);
        let s = ch.to_string();
        let advance = paint_text_onto(
            pixmap,
            &paint_state,
            transform,
            &s,
            cursor.x,
            cursor.y,
            None,
            "spacing",
            font_system,
            swash_cache,
        );
        cursor.x += advance;
        total += advance;
    }
    total
}

fn svg_text_length_list(value: &str) -> Vec<f32> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(|part| number(part))
        .collect()
}

fn paint_text_path(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    ids: &HashMap<String, &SvgNode>,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut cosmic_text::SwashCache,
) -> f32 {
    let text = collect_svg_text(node);
    if text.is_empty() {
        return 0.0;
    }
    let Some(id) = href_id(node) else {
        return 0.0;
    };
    let Some(path_node) = ids.get(id).copied() else {
        return 0.0;
    };
    let Some(data) = path_node.attr("d").or_else(|| path_node.attr("D")) else {
        return 0.0;
    };
    let Some(path) = parse_path_data(data) else {
        return 0.0;
    };
    let samples = flatten_path_points(&path);
    let total_len = path_polyline_length(&samples);
    if samples.len() < 2 || total_len <= 0.0 {
        return 0.0;
    }

    let mut canvas = TinySkiaCanvas::with_text(pixmap, font_system, swash_cache);
    canvas.set_global_alpha(state.opacity);
    canvas.set_font(&Font {
        family: state.font_family.clone(),
        size: state.font_size.max(1.0),
        weight: state.font_weight,
        style: state.font_style,
    });
    canvas.set_text_align(TextAlign::Start);
    canvas.set_text_baseline(state.text_baseline);
    let natural_advance = canvas.measure_text(&text).width;
    let mut distance = text_path_start_offset(node, state, total_len);
    distance += match state.text_align {
        TextAlign::Center => -natural_advance / 2.0,
        TextAlign::End | TextAlign::Right => -natural_advance,
        TextAlign::Start | TextAlign::Left => 0.0,
    };

    for ch in text.chars() {
        let s = ch.to_string();
        let char_advance = canvas.measure_text(&s).width;
        let mid = distance + char_advance / 2.0;
        if let Some((x, y, angle)) = point_at_path_distance(&samples, mid) {
            let local = transform
                .pre_translate(x, y + state.baseline_shift)
                .pre_rotate(angle.to_degrees())
                .pre_translate(-char_advance / 2.0, 0.0);
            canvas.set_transform(Matrix::from_tiny_skia(local));
            if let Some(color) = state.fill.as_ref().and_then(flat_paint_color) {
                canvas.set_fill_color(to_canvas_color(color));
                canvas.fill_text(&s, 0.0, 0.0);
            }
            if let Some(color) = state.stroke.as_ref().and_then(flat_paint_color) {
                if state.stroke_width > 0.0 {
                    canvas.set_stroke_color(to_canvas_color(color));
                    canvas.set_line_width(state.stroke_width);
                    canvas.stroke_text(&s, 0.0, 0.0);
                }
            }
        }
        distance += char_advance + state.letter_spacing;
        if ch.is_whitespace() {
            distance += state.word_spacing;
        }
    }
    natural_advance
}

fn collect_svg_text(node: &SvgNode) -> String {
    let mut text = node.text.clone();
    for child in &node.children {
        if matches!(
            child.kind,
            SvgElementKind::Text | SvgElementKind::Tspan | SvgElementKind::TextPath
        ) {
            text.push_str(&collect_svg_text(child));
        }
    }
    text
}

fn text_path_start_offset(node: &SvgNode, state: &PaintState, path_len: f32) -> f32 {
    let Some(raw) = node.attr_ascii_case_insensitive("startOffset") else {
        return 0.0;
    };
    match parse_svg_length(raw.trim()) {
        Some(SvgLength::Percent(v)) => path_len * v / 100.0,
        Some(SvgLength::Number(v)) | Some(SvgLength::Px(v)) => v,
        Some(SvgLength::Em(v)) | Some(SvgLength::Rem(v)) => v * state.font_size.max(1.0),
        None => number(raw).unwrap_or(0.0),
    }
}

fn paint_text_onto(
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    text: &str,
    x: f32,
    y: f32,
    target_length: Option<f32>,
    length_adjust: &str,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut cosmic_text::SwashCache,
) -> f32 {
    let mut canvas = TinySkiaCanvas::with_text(pixmap, font_system, swash_cache);
    canvas.set_transform(Matrix::from_tiny_skia(transform));
    canvas.set_global_alpha(state.opacity);
    canvas.set_font(&Font {
        family: state.font_family.clone(),
        size: state.font_size.max(1.0),
        weight: state.font_weight,
        style: state.font_style,
    });
    canvas.set_text_align(state.text_align);
    canvas.set_text_baseline(state.text_baseline);
    let advance = canvas.measure_text(text).width;
    let adjusted_advance = text_length_adjusted_advance(text, advance, target_length);
    let extra_letter_spacing = if length_adjust.eq_ignore_ascii_case("spacing")
        || length_adjust.eq_ignore_ascii_case("spacingAndGlyphs")
    {
        text_length_extra_spacing(text, advance, target_length)
    } else {
        0.0
    };
    let total_letter_spacing = state.letter_spacing + extra_letter_spacing;
    let y = y + state.baseline_shift;
    if total_letter_spacing != 0.0 || state.word_spacing != 0.0 {
        paint_spaced_text_onto(
            &mut canvas,
            state,
            text,
            x,
            y,
            adjusted_advance,
            total_letter_spacing,
        );
        return adjusted_advance;
    }
    if let Some(color) = state.fill.as_ref().and_then(flat_paint_color) {
        canvas.set_fill_color(to_canvas_color(color));
        canvas.fill_text(text, x, y);
    }
    if let Some(color) = state.stroke.as_ref().and_then(flat_paint_color) {
        if state.stroke_width > 0.0 {
            canvas.set_stroke_color(to_canvas_color(color));
            canvas.set_line_width(state.stroke_width);
            canvas.stroke_text(text, x, y);
        }
    }
    adjusted_advance
}

fn paint_spaced_text_onto(
    canvas: &mut TinySkiaCanvas<'_>,
    state: &PaintState,
    text: &str,
    x: f32,
    y: f32,
    adjusted_advance: f32,
    letter_spacing: f32,
) {
    let mut cursor = match state.text_align {
        TextAlign::Center => x - adjusted_advance / 2.0,
        TextAlign::End | TextAlign::Right => x - adjusted_advance,
        TextAlign::Start | TextAlign::Left => x,
    };
    canvas.set_text_align(TextAlign::Start);
    for ch in text.chars() {
        let s = ch.to_string();
        let char_advance = canvas.measure_text(&s).width;
        if let Some(color) = state.fill.as_ref().and_then(flat_paint_color) {
            canvas.set_fill_color(to_canvas_color(color));
            canvas.fill_text(&s, cursor, y);
        }
        if let Some(color) = state.stroke.as_ref().and_then(flat_paint_color) {
            if state.stroke_width > 0.0 {
                canvas.set_stroke_color(to_canvas_color(color));
                canvas.set_line_width(state.stroke_width);
                canvas.stroke_text(&s, cursor, y);
            }
        }
        cursor += char_advance + letter_spacing;
        if ch.is_whitespace() {
            cursor += state.word_spacing;
        }
    }
}

fn text_length_adjusted_advance(text: &str, measured: f32, target_length: Option<f32>) -> f32 {
    if text.chars().count() > 1 {
        target_length
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(measured)
    } else {
        measured
    }
}

fn text_length_extra_spacing(text: &str, measured: f32, target_length: Option<f32>) -> f32 {
    let count = text.chars().count();
    if count <= 1 {
        return 0.0;
    }
    let Some(target) = target_length.filter(|v| v.is_finite() && *v >= 0.0) else {
        return 0.0;
    };
    (target - measured) / (count - 1) as f32
}

fn has_text_content(node: &SvgNode) -> bool {
    if !node.text.is_empty() {
        return true;
    }
    for child in &node.children {
        if matches!(
            child.kind,
            SvgElementKind::Text | SvgElementKind::Tspan | SvgElementKind::TextPath
        ) && has_text_content(child)
        {
            return true;
        }
    }
    false
}

fn flat_paint_color(source: &PaintSource) -> Option<Color> {
    match source {
        PaintSource::Color(color) => Some(*color),
        PaintSource::Url(_, fallback) => *fallback,
    }
}

fn to_canvas_color(color: Color) -> crate::canvas::Color {
    crate::canvas::Color::rgba(color.r, color.g, color.b, color.a)
}

fn paint_image(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    clip: Option<&Mask>,
) {
    let href = node
        .attr("href")
        .or_else(|| node.attr("xlink:href"))
        .or_else(|| {
            node.attributes
                .iter()
                .find(|a| a.namespace.as_deref() == Some("xlink") && a.name == "href")
                .map(|a| a.value.as_str())
        });
    let Some(href) = href else {
        return;
    };
    let Some((rgba, iw, ih)) = crate::html::load_image_from_src(href, "") else {
        return;
    };
    if iw == 0 || ih == 0 {
        return;
    }
    let x = attr_length(node, "x", LengthAxis::X, state).unwrap_or(0.0);
    let y = attr_length(node, "y", LengthAxis::Y, state).unwrap_or(0.0);
    let w = attr_length(node, "width", LengthAxis::X, state).unwrap_or(iw as f32);
    let h = attr_length(node, "height", LengthAxis::Y, state).unwrap_or(ih as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let Some(src) = PixmapRef::from_bytes(&rgba, iw, ih) else {
        return;
    };
    let mut paint = PixmapPaint::default();
    paint.opacity = state.opacity.clamp(0.0, 1.0);
    let image_transform = transform
        .pre_translate(x, y)
        .pre_scale(w / iw as f32, h / ih as f32);
    pixmap.draw_pixmap(0, 0, src, &paint, image_transform, clip);
}

fn paint_use<'a>(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    let href = node
        .attr("href")
        .or_else(|| node.attr("xlink:href"))
        .or_else(|| {
            node.attributes
                .iter()
                .find(|a| a.namespace.as_deref() == Some("xlink") && a.name == "href")
                .map(|a| a.value.as_str())
        });
    let Some(id) = href.and_then(|h| h.strip_prefix('#')) else {
        return;
    };
    if stack.iter().any(|seen| seen == id) {
        return;
    }
    let Some(target) = ids.get(id).copied() else {
        return;
    };
    let transform = transform.pre_translate(
        attr_length(node, "x", LengthAxis::X, &state).unwrap_or(0.0),
        attr_length(node, "y", LengthAxis::Y, &state).unwrap_or(0.0),
    );
    stack.push(id.to_string());
    match target.kind {
        SvgElementKind::Symbol => {
            let use_w = attr_length(node, "width", LengthAxis::X, &state)
                .or_else(|| attr_length(target, "width", LengthAxis::X, &state))
                .unwrap_or(state.viewport_width);
            let use_h = attr_length(node, "height", LengthAxis::Y, &state)
                .or_else(|| attr_length(target, "height", LengthAxis::Y, &state))
                .unwrap_or(state.viewport_height);
            let mut symbol_state = state.clone();
            symbol_state.viewport_width = use_w;
            symbol_state.viewport_height = use_h;
            let symbol_transform = target
                .attr_ascii_case_insensitive("viewBox")
                .and_then(|value| parse_view_box(Some(value)))
                .filter(|_| use_w > 0.0 && use_h > 0.0)
                .map(|vb| {
                    transform.pre_concat(view_box_transform(
                        vb,
                        use_w,
                        use_h,
                        parse_preserve_aspect_ratio(
                            target.attr_ascii_case_insensitive("preserveAspectRatio"),
                        ),
                    ))
                })
                .unwrap_or(transform);
            for child in &target.children {
                paint_node(
                    child,
                    pixmap,
                    symbol_state.clone(),
                    symbol_transform,
                    ids,
                    styles,
                    ancestors,
                    stack,
                    clip,
                    true,
                );
            }
        }
        _ => paint_node(
            target, pixmap, state, transform, ids, styles, ancestors, stack, clip, true,
        ),
    }
    stack.pop();
}

fn paint_line_markers<'a>(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    let x1 = attr_length(node, "x1", LengthAxis::X, &state).unwrap_or(0.0);
    let y1 = attr_length(node, "y1", LengthAxis::Y, &state).unwrap_or(0.0);
    let x2 = attr_length(node, "x2", LengthAxis::X, &state).unwrap_or(0.0);
    let y2 = attr_length(node, "y2", LengthAxis::Y, &state).unwrap_or(0.0);
    let angle = (y2 - y1).atan2(x2 - x1).to_degrees();
    paint_marker_ref(
        node,
        "marker-start",
        x1,
        y1,
        angle,
        pixmap,
        state.clone(),
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip,
    );
    paint_marker_ref(
        node,
        "marker-end",
        x2,
        y2,
        angle,
        pixmap,
        state,
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip,
    );
}

fn paint_poly_markers<'a>(
    node: &SvgNode,
    points: &[(f32, f32)],
    closed: bool,
    pixmap: &mut Pixmap,
    state: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    if points.len() < 2 {
        return;
    }
    let first_angle = segment_angle(points[0], points[1]);
    paint_marker_ref(
        node,
        "marker-start",
        points[0].0,
        points[0].1,
        first_angle,
        pixmap,
        state.clone(),
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip,
    );
    for i in 1..points.len() - 1 {
        let in_angle = segment_angle(points[i - 1], points[i]);
        let out_angle = segment_angle(points[i], points[i + 1]);
        paint_marker_ref(
            node,
            "marker-mid",
            points[i].0,
            points[i].1,
            (in_angle + out_angle) / 2.0,
            pixmap,
            state.clone(),
            transform,
            ids,
            styles,
            ancestors,
            stack,
            clip,
        );
    }
    let last = points.len() - 1;
    let end_angle = if closed {
        segment_angle(points[last], points[0])
    } else {
        segment_angle(points[last - 1], points[last])
    };
    paint_marker_ref(
        node,
        "marker-end",
        points[last].0,
        points[last].1,
        end_angle,
        pixmap,
        state,
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip,
    );
}

fn segment_angle(from: (f32, f32), to: (f32, f32)) -> f32 {
    (to.1 - from.1).atan2(to.0 - from.0).to_degrees()
}

#[allow(clippy::too_many_arguments)]
fn paint_marker_ref<'a>(
    owner: &SvgNode,
    attr: &str,
    x: f32,
    y: f32,
    angle: f32,
    pixmap: &mut Pixmap,
    state: PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    let href = owner
        .attr(attr)
        .or_else(|| owner.attr("marker"))
        .and_then(parse_url_id);
    let Some(id) = href else {
        return;
    };
    if stack.iter().any(|seen| seen == id) {
        return;
    }
    let Some(marker) = ids.get(id).copied() else {
        return;
    };
    if !matches!(marker.kind, SvgElementKind::Marker) {
        return;
    }
    let ref_x = attr_length(marker, "refX", LengthAxis::X, &state).unwrap_or(0.0);
    let ref_y = attr_length(marker, "refY", LengthAxis::Y, &state).unwrap_or(0.0);
    let marker_angle = match marker.attr("orient").unwrap_or("0").trim() {
        "auto" | "auto-start-reverse" => angle,
        other => number(other).unwrap_or(angle),
    };
    let mut marker_transform = transform
        .pre_translate(x, y)
        .pre_rotate(marker_angle)
        .pre_translate(-ref_x, -ref_y);
    if marker
        .attr("markerUnits")
        .is_some_and(|v| v == "strokeWidth")
    {
        marker_transform = marker_transform.pre_scale(state.stroke_width, state.stroke_width);
    }
    if let Some(vb) = marker
        .attr_ascii_case_insensitive("viewBox")
        .and_then(|v| parse_view_box(Some(v)))
    {
        let marker_w =
            attr_length(marker, "markerWidth", LengthAxis::X, &state).unwrap_or(vb.width);
        let marker_h =
            attr_length(marker, "markerHeight", LengthAxis::Y, &state).unwrap_or(vb.height);
        marker_transform = marker_transform
            .pre_scale(marker_w / vb.width, marker_h / vb.height)
            .pre_translate(-vb.min_x, -vb.min_y);
    }
    stack.push(id.to_string());
    for child in &marker.children {
        paint_node(
            child,
            pixmap,
            state.clone(),
            marker_transform,
            ids,
            styles,
            ancestors,
            stack,
            clip,
            true,
        );
    }
    stack.pop();
}

fn compositing_mask_for_node<'a>(
    node: &'a SvgNode,
    state: &PaintState,
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let clip = clip_mask_for_node(node, state, width, height, transform, ids, parent);
    let mask = svg_mask_for_node(
        node,
        width,
        height,
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip.as_ref().or(parent),
    );
    mask.or(clip)
}

fn clip_mask_for_node(
    node: &SvgNode,
    state: &PaintState,
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &SvgNode>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let id = node.attr("clip-path").and_then(parse_url_id)?;
    let clip_node = ids.get(id).copied()?;
    if !matches!(clip_node.kind, SvgElementKind::ClipPath) {
        return None;
    }
    let mut mask = Mask::new(width, height)?;
    for child in &clip_node.children {
        if let Some(path) = node_path(child, state) {
            mask.fill_path(&path, FillRule::Winding, true, transform);
        }
    }
    if let Some(parent) = parent {
        for (value, parent_value) in mask.data_mut().iter_mut().zip(parent.data()) {
            *value = (*value).min(*parent_value);
        }
    }
    Some(mask)
}

fn nested_svg_viewport_mask(
    width: u32,
    height: u32,
    transform: Transform,
    x: f32,
    y: f32,
    viewport_w: f32,
    viewport_h: f32,
    parent: Option<&Mask>,
) -> Option<Mask> {
    if viewport_w <= 0.0 || viewport_h <= 0.0 {
        return None;
    }
    let rect = tiny_skia::Rect::from_xywh(x, y, viewport_w, viewport_h)?;
    let mut path = PathBuilder::new();
    path.push_rect(rect);
    let path = path.finish()?;
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(&path, FillRule::Winding, true, transform);
    if let Some(parent) = parent {
        for (value, parent_value) in mask.data_mut().iter_mut().zip(parent.data()) {
            *value = (*value).min(*parent_value);
        }
    }
    Some(mask)
}

fn svg_mask_for_node<'a>(
    node: &SvgNode,
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let id = node.attr("mask").and_then(parse_url_id)?;
    if stack.iter().any(|seen| seen == id) {
        return None;
    }
    let mask_node = ids.get(id).copied()?;
    if !matches!(mask_node.kind, SvgElementKind::Mask) {
        return None;
    }
    let mut mask_pixmap = Pixmap::new(width, height)?;
    stack.push(id.to_string());
    for child in &mask_node.children {
        paint_node(
            child,
            &mut mask_pixmap,
            PaintState::default(),
            transform,
            ids,
            styles,
            ancestors,
            stack,
            None,
            true,
        );
    }
    stack.pop();
    let mut mask = Mask::from_pixmap(mask_pixmap.as_ref(), MaskType::Luminance);
    if let Some(parent) = parent {
        for (value, parent_value) in mask.data_mut().iter_mut().zip(parent.data()) {
            *value = (*value).min(*parent_value);
        }
    }
    Some(mask)
}

fn node_path(node: &SvgNode, state: &PaintState) -> Option<Path> {
    match node.kind {
        SvgElementKind::Rect => rect_path(node, state),
        SvgElementKind::Circle => circle_path(node, state),
        SvgElementKind::Ellipse => ellipse_path(node, state),
        SvgElementKind::Line => line_path(node, state),
        SvgElementKind::Polyline => points_path_from_pairs(&points_list(node, state), false),
        SvgElementKind::Polygon => points_path_from_pairs(&points_list(node, state), true),
        SvgElementKind::Path => node
            .attr("d")
            .or_else(|| node.attr("D"))
            .and_then(parse_path_data),
        _ => None,
    }
}

fn state_for_node(
    node: &SvgNode,
    mut state: PaintState,
    styles: &[CssRule],
    ancestors: &[&SvgNode],
) -> PaintState {
    for attr in &node.attributes {
        if attr.namespace.is_none() && attr.name != "style" {
            apply_paint_attr(&mut state, &attr.name, &attr.value);
        }
    }
    let mut matches: Vec<(u32, usize, &CssRule)> = styles
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            rule_matches_svg_node(rule, node, ancestors).then_some((rule.specificity, index, rule))
        })
        .collect();
    matches.sort_by_key(|(specificity, index, _)| (*specificity, *index));
    for (_, _, rule) in &matches {
        apply_declarations(&mut state, &rule.declarations);
    }
    if let Some(style) = node.attr("style") {
        for decl in style.split(';') {
            if let Some((name, value)) = decl.split_once(':') {
                apply_paint_attr(&mut state, name.trim(), value.trim());
            }
        }
    }
    for (_, _, rule) in &matches {
        apply_declarations(&mut state, &rule.important_declarations);
    }
    state
}

fn apply_declarations(state: &mut PaintState, declarations: &Declarations) {
    for (name, value) in declarations {
        if name.starts_with("--") {
            state.custom_props.insert(name.clone(), value.clone());
            continue;
        }
        apply_paint_attr(state, name, value);
    }
}

fn rule_matches_svg_node(rule: &CssRule, node: &SvgNode, ancestors: &[&SvgNode]) -> bool {
    rule.selectors
        .iter()
        .filter(|selector| selector.valid)
        .any(|selector| selector_matches_svg_selector(selector, node, ancestors))
}

fn selector_matches_svg_selector(
    selector: &CssSelector,
    node: &SvgNode,
    ancestors: &[&SvgNode],
) -> bool {
    selector.valid && selector_matches_svg_parts(&selector.parts, node, ancestors)
}

fn selector_matches_svg_parts(
    parts: &[SelectorPart],
    node: &SvgNode,
    ancestors: &[&SvgNode],
) -> bool {
    let mut groups: Vec<&[SelectorPart]> = Vec::new();
    let mut combinators: Vec<&Combinator> = Vec::new();
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if let SelectorPart::Combinator(combinator) = part {
            groups.push(&parts[start..i]);
            combinators.push(combinator);
            start = i + 1;
        }
    }
    groups.push(&parts[start..]);
    if groups.is_empty()
        || !simple_selector_group_matches(groups[groups.len() - 1], node, ancestors)
    {
        return false;
    }

    let mut current = node;
    let mut ancestor_limit = ancestors.len();
    for idx in (0..groups.len().saturating_sub(1)).rev() {
        let combinator = combinators
            .get(idx)
            .copied()
            .unwrap_or(&Combinator::Descendant);
        match combinator {
            Combinator::Child => {
                if ancestor_limit == 0
                    || !simple_selector_group_matches(
                        groups[idx],
                        ancestors[ancestor_limit - 1],
                        &ancestors[..ancestor_limit - 1],
                    )
                {
                    return false;
                }
                current = ancestors[ancestor_limit - 1];
                ancestor_limit -= 1;
            }
            Combinator::Descendant => {
                let Some(found) = ancestors[..ancestor_limit].iter().rposition(|ancestor| {
                    simple_selector_group_matches(
                        groups[idx],
                        ancestor,
                        &ancestors[..ancestor_limit],
                    )
                }) else {
                    return false;
                };
                current = ancestors[found];
                ancestor_limit = found;
            }
            Combinator::AdjacentSibling => {
                let Some(parent) = ancestor_limit.checked_sub(1).map(|i| ancestors[i]) else {
                    return false;
                };
                let Some(current_index) = svg_child_index(parent, current) else {
                    return false;
                };
                if current_index == 0 {
                    return false;
                }
                let sibling = &parent.children[current_index - 1];
                if !simple_selector_group_matches(
                    groups[idx],
                    sibling,
                    &ancestors[..ancestor_limit],
                ) {
                    return false;
                }
                current = sibling;
            }
            Combinator::GeneralSibling => {
                let Some(parent) = ancestor_limit.checked_sub(1).map(|i| ancestors[i]) else {
                    return false;
                };
                let Some(current_index) = svg_child_index(parent, current) else {
                    return false;
                };
                let Some(found) = parent.children[..current_index]
                    .iter()
                    .rposition(|sibling| {
                        simple_selector_group_matches(
                            groups[idx],
                            sibling,
                            &ancestors[..ancestor_limit],
                        )
                    })
                else {
                    return false;
                };
                current = &parent.children[found];
            }
            Combinator::Column => return false,
        }
    }
    true
}

fn svg_child_index(parent: &SvgNode, child: &SvgNode) -> Option<usize> {
    parent
        .children
        .iter()
        .position(|candidate| std::ptr::eq(candidate, child))
}

fn simple_selector_group_matches(
    parts: &[SelectorPart],
    node: &SvgNode,
    ancestors: &[&SvgNode],
) -> bool {
    for part in parts {
        match part {
            SelectorPart::Tag(tag) => {
                if tag != "*" && !svg_tag_name(node).eq_ignore_ascii_case(tag) {
                    return false;
                }
            }
            SelectorPart::Id(id) => {
                if node.attr("id") != Some(id.as_str()) {
                    return false;
                }
            }
            SelectorPart::Class(class) => {
                if !node
                    .attr("class")
                    .is_some_and(|classes| classes.split_ascii_whitespace().any(|c| c == class))
                {
                    return false;
                }
            }
            SelectorPart::Universal => {}
            SelectorPart::Attribute {
                name,
                op,
                value,
                case_sensitive,
            } => {
                if !attribute_selector_matches(node, name, op, value, *case_sensitive) {
                    return false;
                }
            }
            SelectorPart::Not(inner) => {
                if selector_matches_svg_selector(inner, node, ancestors) {
                    return false;
                }
            }
            SelectorPart::Is(list) | SelectorPart::Where(list) => {
                if !list
                    .iter()
                    .any(|selector| selector_matches_svg_selector(selector, node, ancestors))
                {
                    return false;
                }
            }
            SelectorPart::PseudoClass(name) => {
                if !svg_structural_pseudo_matches(name, node, ancestors) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn svg_structural_pseudo_matches(name: &str, node: &SvgNode, ancestors: &[&SvgNode]) -> bool {
    let name = name.trim();
    match name {
        "root" => ancestors.is_empty(),
        "empty" => node.children.is_empty() && node.text.trim().is_empty(),
        "first-child" => svg_child_position(node, ancestors)
            .is_some_and(|(index, count)| count > 0 && index == 0),
        "last-child" => svg_child_position(node, ancestors)
            .is_some_and(|(index, count)| count > 0 && index + 1 == count),
        "only-child" => svg_child_position(node, ancestors).is_some_and(|(_, count)| count == 1),
        "first-of-type" => svg_type_child_position(node, ancestors)
            .is_some_and(|(index, count)| count > 0 && index == 0),
        "last-of-type" => svg_type_child_position(node, ancestors)
            .is_some_and(|(index, count)| count > 0 && index + 1 == count),
        "only-of-type" => {
            svg_type_child_position(node, ancestors).is_some_and(|(_, count)| count == 1)
        }
        _ => svg_nth_pseudo_matches(name, node, ancestors),
    }
}

fn svg_nth_pseudo_matches(name: &str, node: &SvgNode, ancestors: &[&SvgNode]) -> bool {
    let Some((pseudo, formula)) = name.split_once('(') else {
        return false;
    };
    let formula = formula.trim_end_matches(')').trim();
    let Some((index, count)) = (match pseudo.trim() {
        "nth-child" => svg_child_position(node, ancestors),
        "nth-last-child" => svg_child_position(node, ancestors)
            .map(|(index, count)| (count.saturating_sub(index), count)),
        "nth-of-type" => svg_type_child_position(node, ancestors),
        "nth-last-of-type" => svg_type_child_position(node, ancestors)
            .map(|(index, count)| (count.saturating_sub(index), count)),
        _ => None,
    }) else {
        return false;
    };
    count > 0 && nth_formula_matches(formula, index + 1)
}

fn svg_child_position(node: &SvgNode, ancestors: &[&SvgNode]) -> Option<(usize, usize)> {
    let parent = ancestors.last().copied()?;
    let index = svg_child_index(parent, node)?;
    Some((index, parent.children.len()))
}

fn svg_type_child_position(node: &SvgNode, ancestors: &[&SvgNode]) -> Option<(usize, usize)> {
    let parent = ancestors.last().copied()?;
    let tag = svg_tag_name(node);
    let mut index = None;
    let mut count = 0usize;
    for child in &parent.children {
        if svg_tag_name(child).eq_ignore_ascii_case(tag) {
            if std::ptr::eq(child, node) {
                index = Some(count);
            }
            count += 1;
        }
    }
    index.map(|index| (index, count))
}

fn nth_formula_matches(formula: &str, position: usize) -> bool {
    let formula = formula.trim().to_ascii_lowercase().replace(' ', "");
    if formula == "odd" {
        return position % 2 == 1;
    }
    if formula == "even" {
        return position % 2 == 0;
    }
    if let Ok(n) = formula.parse::<isize>() {
        return n > 0 && position == n as usize;
    }
    let Some(n_pos) = formula.find('n') else {
        return false;
    };
    let a_text = &formula[..n_pos];
    let b_text = &formula[n_pos + 1..];
    let a = match a_text {
        "" | "+" => 1,
        "-" => -1,
        other => other.parse::<isize>().unwrap_or(0),
    };
    let b = if b_text.is_empty() {
        0
    } else {
        b_text.parse::<isize>().unwrap_or(0)
    };
    let pos = position as isize;
    if a == 0 {
        return pos == b;
    }
    let delta = pos - b;
    delta % a == 0 && delta / a >= 0
}

fn attribute_selector_matches(
    node: &SvgNode,
    name: &str,
    op: &AttrOp,
    expected: &str,
    case_sensitive: Option<bool>,
) -> bool {
    let Some(actual) = node.attr(name) else {
        return false;
    };
    let insensitive = case_sensitive == Some(false);
    let actual_owned;
    let expected_owned;
    let (actual, expected) = if insensitive {
        actual_owned = actual.to_ascii_lowercase();
        expected_owned = expected.to_ascii_lowercase();
        (actual_owned.as_str(), expected_owned.as_str())
    } else {
        (actual, expected)
    };
    match op {
        AttrOp::Exists => true,
        AttrOp::Eq => actual == expected,
        AttrOp::Contains => actual.contains(expected),
        AttrOp::StartsWith => actual.starts_with(expected),
        AttrOp::EndsWith => actual.ends_with(expected),
        AttrOp::Includes => actual.split_ascii_whitespace().any(|part| part == expected),
        AttrOp::DashMatch => {
            actual == expected
                || actual
                    .strip_prefix(expected)
                    .is_some_and(|rest| rest.starts_with('-'))
        }
    }
}

fn svg_tag_name(node: &SvgNode) -> &str {
    match &node.kind {
        SvgElementKind::Svg => "svg",
        SvgElementKind::Group => "g",
        SvgElementKind::Defs => "defs",
        SvgElementKind::Symbol => "symbol",
        SvgElementKind::Use => "use",
        SvgElementKind::Path => "path",
        SvgElementKind::Rect => "rect",
        SvgElementKind::Circle => "circle",
        SvgElementKind::Ellipse => "ellipse",
        SvgElementKind::Line => "line",
        SvgElementKind::Polyline => "polyline",
        SvgElementKind::Polygon => "polygon",
        SvgElementKind::Text => "text",
        SvgElementKind::Tspan => "tspan",
        SvgElementKind::TextPath => "textPath",
        SvgElementKind::Image => "image",
        SvgElementKind::LinearGradient => "linearGradient",
        SvgElementKind::RadialGradient => "radialGradient",
        SvgElementKind::ClipPath => "clipPath",
        SvgElementKind::Mask => "mask",
        SvgElementKind::Pattern => "pattern",
        SvgElementKind::Marker => "marker",
        SvgElementKind::Style => "style",
        SvgElementKind::Unknown(name) => name,
    }
}

fn apply_paint_attr(state: &mut PaintState, name: &str, value: &str) {
    match name {
        "fill" => {
            state.fill = parse_svg_paint(value, state.current_color, &state.custom_props);
        }
        "stroke" => state.stroke = parse_svg_paint(value, state.current_color, &state.custom_props),
        "color" => {
            if let Some(color) = parse_svg_color(value, &state.custom_props) {
                state.current_color = color;
            }
        }
        "display" if value.trim().eq_ignore_ascii_case("none") => {
            state.visible = false;
        }
        "visibility" => {
            let value = value.trim();
            if value.eq_ignore_ascii_case("hidden") || value.eq_ignore_ascii_case("collapse") {
                state.visible = false;
            } else if value.eq_ignore_ascii_case("visible") {
                state.visible = true;
            }
        }
        "overflow" => {
            state.overflow_visible = value.trim().eq_ignore_ascii_case("visible");
        }
        "stroke-width" => {
            if let Some(v) = resolve_svg_length(value, LengthAxis::Normalized, state) {
                state.stroke_width = v.max(0.0);
            }
        }
        "stroke-linecap" => {
            state.stroke_linecap = match value.trim() {
                "round" => LineCap::Round,
                "square" => LineCap::Square,
                _ => LineCap::Butt,
            };
        }
        "stroke-linejoin" => {
            state.stroke_linejoin = match value.trim() {
                "round" => LineJoin::Round,
                "bevel" => LineJoin::Bevel,
                "miter-clip" | "miterclip" => LineJoin::MiterClip,
                _ => LineJoin::Miter,
            };
        }
        "stroke-miterlimit" => {
            if let Some(v) = number(value) {
                state.stroke_miterlimit = v.max(1.0);
            }
        }
        "stroke-dasharray" => {
            let value = value.trim();
            if value.eq_ignore_ascii_case("none") {
                state.stroke_dasharray = None;
            } else {
                let values = number_list(value);
                if !values.is_empty()
                    && values.iter().all(|v| *v >= 0.0)
                    && values.iter().any(|v| *v > 0.0)
                {
                    state.stroke_dasharray = Some(if values.len() % 2 == 1 {
                        values.iter().chain(values.iter()).copied().collect()
                    } else {
                        values
                    });
                }
            }
        }
        "stroke-dashoffset" => {
            if let Some(v) = number(value) {
                state.stroke_dashoffset = v;
            }
        }
        "fill-rule" => {
            state.fill_rule = if value.trim().eq_ignore_ascii_case("evenodd") {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
        }
        "paint-order" => {
            state.paint_order = parse_paint_order(value);
        }
        "opacity" => {
            if let Some(v) = number(value) {
                state.opacity = v.clamp(0.0, 1.0);
            }
        }
        "fill-opacity" => {
            if let (Some(fill), Some(v)) = (state.fill.take(), number(value)) {
                state.fill = Some(with_source_alpha(fill, v));
            }
        }
        "stroke-opacity" => {
            if let (Some(stroke), Some(v)) = (state.stroke.take(), number(value)) {
                state.stroke = Some(with_source_alpha(stroke, v));
            }
        }
        "font-size" => {
            if let Some(v) = resolve_svg_length(value, LengthAxis::Y, state) {
                state.font_size = v.max(1.0);
            }
        }
        "font-family" => {
            let family = value
                .split(',')
                .next()
                .unwrap_or(value)
                .trim()
                .trim_matches(|c| c == '"' || c == '\'');
            if !family.is_empty() {
                state.font_family = family.to_string();
            }
        }
        "font-weight" => {
            let value = value.trim();
            state.font_weight = if value.eq_ignore_ascii_case("bold")
                || value.parse::<u16>().is_ok_and(|weight| weight >= 600)
            {
                FontWeight::Bold
            } else {
                FontWeight::Normal
            };
        }
        "font-style" => {
            state.font_style = if value.trim().eq_ignore_ascii_case("italic")
                || value.trim().eq_ignore_ascii_case("oblique")
            {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            };
        }
        "letter-spacing" => {
            let value = value.trim();
            state.letter_spacing = if value.eq_ignore_ascii_case("normal") {
                0.0
            } else {
                resolve_svg_length(value, LengthAxis::X, state).unwrap_or(state.letter_spacing)
            };
        }
        "word-spacing" => {
            let value = value.trim();
            state.word_spacing = if value.eq_ignore_ascii_case("normal") {
                0.0
            } else {
                resolve_svg_length(value, LengthAxis::X, state).unwrap_or(state.word_spacing)
            };
        }
        "text-anchor" => {
            state.text_align = match value.trim() {
                "middle" => TextAlign::Center,
                "end" => TextAlign::End,
                _ => TextAlign::Start,
            };
        }
        "dominant-baseline" | "alignment-baseline" => {
            state.text_baseline = match value.trim() {
                "text-before-edge" | "hanging" => TextBaseline::Hanging,
                "middle" | "central" => TextBaseline::Middle,
                "text-after-edge" | "ideographic" => TextBaseline::Ideographic,
                _ => TextBaseline::Alphabetic,
            };
        }
        "baseline-shift" => {
            let value = value.trim();
            state.baseline_shift = if value.eq_ignore_ascii_case("sub") {
                state.font_size * 0.2
            } else if value.eq_ignore_ascii_case("super") {
                -state.font_size * 0.35
            } else {
                resolve_svg_length(value, LengthAxis::Y, state).unwrap_or(0.0)
            };
        }
        _ => {}
    }
}

fn parse_svg_paint(
    value: &str,
    current_color: Color,
    custom_props: &HashMap<String, String>,
) -> Option<PaintSource> {
    let resolved;
    let value = if value.contains("var(") {
        resolved = resolve_var_references(value, custom_props);
        resolved.trim()
    } else {
        value.trim()
    };
    if value.eq_ignore_ascii_case("none") {
        None
    } else if value.eq_ignore_ascii_case("currentColor") {
        Some(PaintSource::Color(current_color))
    } else if let Some((id, fallback)) = parse_paint_url(value, current_color, custom_props) {
        Some(PaintSource::Url(id.to_string(), fallback))
    } else {
        parse_svg_color(value, custom_props).map(PaintSource::Color)
    }
}

fn parse_svg_color(value: &str, custom_props: &HashMap<String, String>) -> Option<Color> {
    if value.contains("var(") {
        let resolved = resolve_var_references(value, custom_props);
        parse_color(resolved.trim())
    } else {
        parse_color(value)
    }
}

fn is_svg_filter_node(node: &SvgNode) -> bool {
    svg_tag_name(node).eq_ignore_ascii_case("filter")
}

fn apply_svg_filter(pixmap: &mut Pixmap, filter: &SvgNode, state: &PaintState) {
    let source = pixmap.to_owned();
    let mut current = source.to_owned();
    let mut results: HashMap<String, Pixmap> = HashMap::new();
    results.insert("SourceGraphic".to_string(), source.to_owned());
    results.insert("SourceAlpha".to_string(), source_alpha_pixmap(&source));

    for child in &filter.children {
        let tag = svg_tag_name(child);
        let mut next = filter_input(child, &current, &results);
        if tag.eq_ignore_ascii_case("feGaussianBlur") {
            let std_dev = filter_std_deviation(child).unwrap_or(0.0);
            crate::canvas::effects::blur_pixmap(&mut next, std_dev);
        } else if tag.eq_ignore_ascii_case("feOffset") {
            let dx = attr_length(child, "dx", LengthAxis::X, state).unwrap_or(0.0);
            let dy = attr_length(child, "dy", LengthAxis::Y, state).unwrap_or(0.0);
            offset_pixmap(&mut next, dx, dy);
        } else if tag.eq_ignore_ascii_case("feDropShadow") {
            let dx = attr_length(child, "dx", LengthAxis::X, state).unwrap_or(2.0);
            let dy = attr_length(child, "dy", LengthAxis::Y, state).unwrap_or(2.0);
            let std_dev = filter_std_deviation(child).unwrap_or(2.0);
            crate::canvas::effects::drop_shadow(
                &mut next,
                dx,
                dy,
                std_dev,
                flood_color(child, state),
            );
        } else if tag.eq_ignore_ascii_case("feFlood") {
            next = flood_pixmap(pixmap.width(), pixmap.height(), flood_color(child, state));
        } else if tag.eq_ignore_ascii_case("feComposite") {
            let second = filter_input_named(
                child.attr("in2").unwrap_or("SourceGraphic"),
                &source,
                &current,
                &results,
            );
            next =
                composite_filter_pixmaps(&next, &second, child.attr("operator").unwrap_or("over"));
        } else if tag.eq_ignore_ascii_case("feBlend") {
            let second = filter_input_named(
                child.attr("in2").unwrap_or("SourceGraphic"),
                &source,
                &current,
                &results,
            );
            next = blend_filter_pixmaps(&second, &next, child.attr("mode").unwrap_or("normal"));
        } else if tag.eq_ignore_ascii_case("feColorMatrix") {
            next = color_matrix_filter_pixmap(
                &next,
                child.attr("type").unwrap_or("matrix"),
                child.attr("values").unwrap_or(""),
            );
        } else if tag.eq_ignore_ascii_case("feComponentTransfer") {
            next = component_transfer_filter_pixmap(&next, child);
        } else if tag.eq_ignore_ascii_case("feMorphology") {
            next = morphology_filter_pixmap(&next, child);
        } else if tag.eq_ignore_ascii_case("feMerge") {
            let Some(mut merged) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                continue;
            };
            let mut saw_node = false;
            for merge_node in &child.children {
                if !svg_tag_name(merge_node).eq_ignore_ascii_case("feMergeNode") {
                    continue;
                }
                saw_node = true;
                let input = filter_input_named(
                    merge_node.attr("in").unwrap_or("SourceGraphic"),
                    &source,
                    &current,
                    &results,
                );
                composite_source_over(&mut merged, &input);
            }
            if saw_node {
                next = merged;
            }
        } else {
            continue;
        }

        if let Some(name) = child.attr("result").filter(|name| !name.trim().is_empty()) {
            results.insert(name.trim().to_string(), next.to_owned());
        }
        current = next;
    }
    *pixmap = current;
}

fn filter_input(node: &SvgNode, current: &Pixmap, results: &HashMap<String, Pixmap>) -> Pixmap {
    node.attr("in")
        .and_then(|name| results.get(name.trim()))
        .unwrap_or(current)
        .to_owned()
}

fn filter_input_named(
    name: &str,
    source: &Pixmap,
    current: &Pixmap,
    results: &HashMap<String, Pixmap>,
) -> Pixmap {
    let name = name.trim();
    if name == "SourceGraphic" {
        source.to_owned()
    } else {
        results.get(name).unwrap_or(current).to_owned()
    }
}

fn flood_color(node: &SvgNode, state: &PaintState) -> Color {
    let mut color = node
        .attr("flood-color")
        .and_then(|value| {
            if value.trim().eq_ignore_ascii_case("currentColor") {
                Some(state.current_color)
            } else {
                parse_svg_color(value, &state.custom_props)
            }
        })
        .unwrap_or(Color::BLACK);
    if let Some(opacity) = node.attr("flood-opacity").and_then(number) {
        color = with_alpha(color, opacity);
    }
    color
}

fn flood_pixmap(width: u32, height: u32, color: Color) -> Pixmap {
    let mut pixmap = Pixmap::new(width, height).expect("filter pixmap dimensions are valid");
    let premul = PremultipliedColorU8::from_rgba(color.r, color.g, color.b, color.a)
        .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
    for px in pixmap.pixels_mut() {
        *px = premul;
    }
    pixmap
}

fn source_alpha_pixmap(source: &Pixmap) -> Pixmap {
    let mut alpha =
        Pixmap::new(source.width(), source.height()).expect("source dimensions are valid");
    for (dst, src) in alpha.pixels_mut().iter_mut().zip(source.pixels()) {
        let a = src.alpha();
        *dst = PremultipliedColorU8::from_rgba(a, a, a, a).unwrap();
    }
    alpha
}

fn composite_source_over(dst: &mut Pixmap, src: &Pixmap) {
    for (d, s) in dst.pixels_mut().iter_mut().zip(src.pixels()) {
        *d = source_over_pixel(*d, *s);
    }
}

fn composite_filter_pixmaps(input: &Pixmap, input2: &Pixmap, operator: &str) -> Pixmap {
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    for ((dst, a), b) in out
        .pixels_mut()
        .iter_mut()
        .zip(input.pixels())
        .zip(input2.pixels())
    {
        let aa = a.alpha() as u32;
        let ba = b.alpha() as u32;
        let inv_aa = 255 - aa;
        let inv_ba = 255 - ba;
        let (r, g, bl, alpha) = match operator.trim() {
            "in" => (
                a.red() as u32 * ba / 255,
                a.green() as u32 * ba / 255,
                a.blue() as u32 * ba / 255,
                aa * ba / 255,
            ),
            "out" => (
                a.red() as u32 * inv_ba / 255,
                a.green() as u32 * inv_ba / 255,
                a.blue() as u32 * inv_ba / 255,
                aa * inv_ba / 255,
            ),
            "atop" => (
                a.red() as u32 * ba / 255 + b.red() as u32 * inv_aa / 255,
                a.green() as u32 * ba / 255 + b.green() as u32 * inv_aa / 255,
                a.blue() as u32 * ba / 255 + b.blue() as u32 * inv_aa / 255,
                ba,
            ),
            "xor" => (
                a.red() as u32 * inv_ba / 255 + b.red() as u32 * inv_aa / 255,
                a.green() as u32 * inv_ba / 255 + b.green() as u32 * inv_aa / 255,
                a.blue() as u32 * inv_ba / 255 + b.blue() as u32 * inv_aa / 255,
                aa * inv_ba / 255 + ba * inv_aa / 255,
            ),
            _ => (
                a.red() as u32 + b.red() as u32 * inv_aa / 255,
                a.green() as u32 + b.green() as u32 * inv_aa / 255,
                a.blue() as u32 + b.blue() as u32 * inv_aa / 255,
                aa + ba * inv_aa / 255,
            ),
        };
        *dst = premul_channels_to_pixel(
            r.min(255) as u8,
            g.min(255) as u8,
            bl.min(255) as u8,
            alpha.min(255) as u8,
        );
    }
    out
}

fn component_transfer_filter_pixmap(input: &Pixmap, node: &SvgNode) -> Pixmap {
    let funcs = component_transfer_funcs(node);
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    for (dst, src) in out.pixels_mut().iter_mut().zip(input.pixels()) {
        let (r, g, b, a) = pixel_unpremul_rgba(*src);
        let channel = |value: u8, func: Option<&ComponentTransferFunc>| -> f32 {
            let value = value as f32 / 255.0;
            func.map(|func| func.apply(value)).unwrap_or(value)
        };
        *dst = premul_from_unit_rgba(
            channel(r, funcs.r.as_ref()),
            channel(g, funcs.g.as_ref()),
            channel(b, funcs.b.as_ref()),
            channel(a, funcs.a.as_ref()),
        );
    }
    out
}

#[derive(Default)]
struct ComponentTransferFuncs {
    r: Option<ComponentTransferFunc>,
    g: Option<ComponentTransferFunc>,
    b: Option<ComponentTransferFunc>,
    a: Option<ComponentTransferFunc>,
}

#[derive(Clone, Debug)]
struct ComponentTransferFunc {
    kind: String,
    table_values: Vec<f32>,
    slope: f32,
    intercept: f32,
    amplitude: f32,
    exponent: f32,
    offset: f32,
}

impl ComponentTransferFunc {
    fn from_node(node: &SvgNode) -> Self {
        Self {
            kind: node
                .attr("type")
                .unwrap_or("identity")
                .trim()
                .to_ascii_lowercase(),
            table_values: node
                .attr("tableValues")
                .map(number_list)
                .unwrap_or_default(),
            slope: node.attr("slope").and_then(number).unwrap_or(1.0),
            intercept: node.attr("intercept").and_then(number).unwrap_or(0.0),
            amplitude: node.attr("amplitude").and_then(number).unwrap_or(1.0),
            exponent: node.attr("exponent").and_then(number).unwrap_or(1.0),
            offset: node.attr("offset").and_then(number).unwrap_or(0.0),
        }
    }

    fn apply(&self, value: f32) -> f32 {
        match self.kind.as_str() {
            "table" => table_transfer(value, &self.table_values, false),
            "discrete" => table_transfer(value, &self.table_values, true),
            "linear" => self.slope * value + self.intercept,
            "gamma" => self.amplitude * value.max(0.0).powf(self.exponent) + self.offset,
            _ => value,
        }
        .clamp(0.0, 1.0)
    }
}

fn component_transfer_funcs(node: &SvgNode) -> ComponentTransferFuncs {
    let mut funcs = ComponentTransferFuncs::default();
    for child in &node.children {
        let func = ComponentTransferFunc::from_node(child);
        match svg_tag_name(child) {
            tag if tag.eq_ignore_ascii_case("feFuncR") => funcs.r = Some(func),
            tag if tag.eq_ignore_ascii_case("feFuncG") => funcs.g = Some(func),
            tag if tag.eq_ignore_ascii_case("feFuncB") => funcs.b = Some(func),
            tag if tag.eq_ignore_ascii_case("feFuncA") => funcs.a = Some(func),
            _ => {}
        }
    }
    funcs
}

fn table_transfer(value: f32, table: &[f32], discrete: bool) -> f32 {
    if table.is_empty() {
        return value;
    }
    if table.len() == 1 {
        return table[0];
    }
    let scaled = value.clamp(0.0, 1.0) * (table.len() - 1) as f32;
    let i = scaled.floor() as usize;
    if discrete {
        return table[i.min(table.len() - 1)];
    }
    if i >= table.len() - 1 {
        return table[table.len() - 1];
    }
    let t = scaled - i as f32;
    table[i] + (table[i + 1] - table[i]) * t
}

fn morphology_filter_pixmap(input: &Pixmap, node: &SvgNode) -> Pixmap {
    let radii = node
        .attr("radius")
        .map(number_list)
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![0.0]);
    let rx = radii[0].max(0.0).round() as i32;
    let ry = radii.get(1).copied().unwrap_or(radii[0]).max(0.0).round() as i32;
    if rx == 0 && ry == 0 {
        return input.to_owned();
    }
    let dilate = !node
        .attr("operator")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("erode"));
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let width = input.width() as i32;
    let height = input.height() as i32;
    for y in 0..height {
        for x in 0..width {
            let mut r = if dilate { 0u8 } else { 255u8 };
            let mut g = r;
            let mut b = r;
            let mut a = r;
            for sy in (y - ry).max(0)..=(y + ry).min(height - 1) {
                for sx in (x - rx).max(0)..=(x + rx).min(width - 1) {
                    let px = input.pixel(sx as u32, sy as u32).unwrap();
                    if dilate {
                        r = r.max(px.red());
                        g = g.max(px.green());
                        b = b.max(px.blue());
                        a = a.max(px.alpha());
                    } else {
                        r = r.min(px.red());
                        g = g.min(px.green());
                        b = b.min(px.blue());
                        a = a.min(px.alpha());
                    }
                }
            }
            *out.pixel_mut(x as u32, y as u32).unwrap() = premul_channels_to_pixel(r, g, b, a);
        }
    }
    out
}

fn blend_filter_pixmaps(backdrop: &Pixmap, source: &Pixmap, mode: &str) -> Pixmap {
    let mut out =
        Pixmap::new(source.width(), source.height()).expect("filter dimensions are valid");
    for ((dst, b), s) in out
        .pixels_mut()
        .iter_mut()
        .zip(backdrop.pixels())
        .zip(source.pixels())
    {
        let (br, bg, bb, ba) = pixel_unpremul_rgba(*b);
        let (sr, sg, sb, sa) = pixel_unpremul_rgba(*s);
        let blend = |bc: u8, sc: u8| -> u8 {
            let b = bc as u32;
            let s = sc as u32;
            match mode.trim() {
                "multiply" => (b * s / 255) as u8,
                "screen" => (255 - (255 - b) * (255 - s) / 255) as u8,
                "darken" => bc.min(sc),
                "lighten" => bc.max(sc),
                _ => sc,
            }
        };
        let blended = Color::rgba(blend(br, sr), blend(bg, sg), blend(bb, sb), sa);
        let mut src_layer =
            PremultipliedColorU8::from_rgba(blended.r, blended.g, blended.b, blended.a)
                .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
        if sa < 255 && ba > 0 {
            src_layer = source_over_pixel(*b, src_layer);
        }
        *dst = src_layer;
    }
    out
}

fn source_over_pixel(dst: PremultipliedColorU8, src: PremultipliedColorU8) -> PremultipliedColorU8 {
    let sa = src.alpha() as u32;
    if sa == 0 {
        return dst;
    }
    let da = dst.alpha() as u32;
    let inv_sa = 255 - sa;
    let r = (src.red() as u32 + dst.red() as u32 * inv_sa / 255).min(255) as u8;
    let g = (src.green() as u32 + dst.green() as u32 * inv_sa / 255).min(255) as u8;
    let b = (src.blue() as u32 + dst.blue() as u32 * inv_sa / 255).min(255) as u8;
    let a = (sa + da * inv_sa / 255).min(255) as u8;
    premul_channels_to_pixel(r, g, b, a)
}

fn color_matrix_filter_pixmap(input: &Pixmap, ty: &str, values: &str) -> Pixmap {
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let nums = number_list(values);
    for (dst, src) in out.pixels_mut().iter_mut().zip(input.pixels()) {
        let (r, g, b, a) = pixel_unpremul_rgba(*src);
        let (rf, gf, bf, af) = (
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );
        let next = match ty.trim() {
            "saturate" => {
                let s = nums.first().copied().unwrap_or(1.0);
                let ir = 0.213 + 0.787 * s;
                let ig = 0.715 - 0.715 * s;
                let ib = 0.072 - 0.072 * s;
                let jr = 0.213 - 0.213 * s;
                let jg = 0.715 + 0.285 * s;
                let jb = 0.072 - 0.072 * s;
                let kr = 0.213 - 0.213 * s;
                let kg = 0.715 - 0.715 * s;
                let kb = 0.072 + 0.928 * s;
                (
                    ir * rf + ig * gf + ib * bf,
                    jr * rf + jg * gf + jb * bf,
                    kr * rf + kg * gf + kb * bf,
                    af,
                )
            }
            "luminanceToAlpha" => (0.0, 0.0, 0.0, 0.2126 * rf + 0.7152 * gf + 0.0722 * bf),
            _ if nums.len() >= 20 => (
                nums[0] * rf + nums[1] * gf + nums[2] * bf + nums[3] * af + nums[4],
                nums[5] * rf + nums[6] * gf + nums[7] * bf + nums[8] * af + nums[9],
                nums[10] * rf + nums[11] * gf + nums[12] * bf + nums[13] * af + nums[14],
                nums[15] * rf + nums[16] * gf + nums[17] * bf + nums[18] * af + nums[19],
            ),
            _ => (rf, gf, bf, af),
        };
        *dst = premul_from_unit_rgba(next.0, next.1, next.2, next.3);
    }
    out
}

fn pixel_unpremul_rgba(px: PremultipliedColorU8) -> (u8, u8, u8, u8) {
    let a = px.alpha();
    if a == 0 {
        return (0, 0, 0, 0);
    }
    let unpremul = |c: u8| -> u8 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
    (
        unpremul(px.red()),
        unpremul(px.green()),
        unpremul(px.blue()),
        a,
    )
}

fn premul_channels_to_pixel(r: u8, g: u8, b: u8, a: u8) -> PremultipliedColorU8 {
    PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a)
        .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap())
}

fn premul_from_unit_rgba(r: f32, g: f32, b: f32, a: f32) -> PremultipliedColorU8 {
    let alpha = a.clamp(0.0, 1.0);
    let byte = |v: f32| -> u8 { (v.clamp(0.0, 1.0) * alpha * 255.0).round() as u8 };
    let alpha_byte = (alpha * 255.0).round() as u8;
    PremultipliedColorU8::from_rgba(byte(r), byte(g), byte(b), alpha_byte)
        .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap())
}

fn filter_std_deviation(node: &SvgNode) -> Option<f32> {
    let value = node.attr("stdDeviation")?;
    number_list(value)
        .into_iter()
        .next()
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn offset_pixmap(pixmap: &mut Pixmap, dx: f32, dy: f32) {
    if !dx.is_finite() || !dy.is_finite() {
        return;
    }
    let Some(mut out) = Pixmap::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    out.draw_pixmap(
        dx.round() as i32,
        dy.round() as i32,
        pixmap.as_ref(),
        &paint,
        Transform::identity(),
        None,
    );
    *pixmap = out;
}

fn parse_url_id(value: &str) -> Option<&str> {
    let value = value.trim();
    let inside = value.strip_prefix("url(")?.strip_suffix(')')?.trim();
    let inside = inside.trim_matches(|c| c == '"' || c == '\'');
    inside.strip_prefix('#')
}

fn parse_paint_url<'a>(
    value: &'a str,
    current_color: Color,
    custom_props: &HashMap<String, String>,
) -> Option<(&'a str, Option<Color>)> {
    let value = value.trim();
    let rest = value.strip_prefix("url(")?;
    let close = rest.find(')')?;
    let inside = rest[..close].trim().trim_matches(|c| c == '"' || c == '\'');
    let id = inside.strip_prefix('#')?;
    let fallback = rest[close + 1..].trim();
    let fallback = (!fallback.is_empty())
        .then(|| parse_svg_color(fallback, custom_props))
        .flatten()
        .or_else(|| {
            fallback
                .eq_ignore_ascii_case("currentColor")
                .then_some(current_color)
        });
    Some((id, fallback))
}

fn parse_paint_order(value: &str) -> [PaintOp; 2] {
    let mut order = [PaintOp::Fill, PaintOp::Stroke];
    let mut seen_fill = false;
    let mut seen_stroke = false;
    let mut next = 0usize;
    for token in value.split_ascii_whitespace() {
        match token {
            "normal" => return [PaintOp::Fill, PaintOp::Stroke],
            "fill" if !seen_fill && next < 2 => {
                order[next] = PaintOp::Fill;
                seen_fill = true;
                next += 1;
            }
            "stroke" if !seen_stroke && next < 2 => {
                order[next] = PaintOp::Stroke;
                seen_stroke = true;
                next += 1;
            }
            "markers" => {}
            _ => {}
        }
    }
    if !seen_fill && next < 2 {
        order[next] = PaintOp::Fill;
        next += 1;
    }
    if !seen_stroke && next < 2 {
        order[next] = PaintOp::Stroke;
    }
    order
}

fn with_source_alpha(source: PaintSource, alpha: f32) -> PaintSource {
    match source {
        PaintSource::Color(color) => PaintSource::Color(with_alpha(color, alpha)),
        PaintSource::Url(id, fallback) => {
            PaintSource::Url(id, fallback.map(|color| with_alpha(color, alpha)))
        }
    }
}

fn with_alpha(mut color: Color, alpha: f32) -> Color {
    color.a = ((color.a as f32) * alpha.clamp(0.0, 1.0)).round() as u8;
    color
}

fn paint_path<'a>(
    pixmap: &mut Pixmap,
    path: &Path,
    state: &PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    for op in state.paint_order {
        match op {
            PaintOp::Fill => paint_path_fill(
                pixmap, path, state, transform, ids, styles, ancestors, stack, clip,
            ),
            PaintOp::Stroke => paint_path_stroke(
                pixmap, path, state, transform, ids, styles, ancestors, stack, clip,
            ),
        }
    }
}

fn paint_path_fill<'a>(
    pixmap: &mut Pixmap,
    path: &Path,
    state: &PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    let Some(source) = &state.fill else {
        return;
    };
    with_paint_source(
        source,
        state,
        path,
        ids,
        styles,
        ancestors,
        stack,
        |paint| {
            pixmap.fill_path(path, paint, state.fill_rule, transform, clip);
        },
    );
}

fn paint_path_stroke<'a>(
    pixmap: &mut Pixmap,
    path: &Path,
    state: &PaintState,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    clip: Option<&Mask>,
) {
    let Some(source) = &state.stroke else {
        return;
    };
    if state.stroke_width <= 0.0 {
        return;
    }
    let mut stroke = Stroke::default();
    stroke.width = state.stroke_width;
    stroke.line_cap = state.stroke_linecap;
    stroke.line_join = state.stroke_linejoin;
    stroke.miter_limit = state.stroke_miterlimit;
    stroke.dash = state
        .stroke_dasharray
        .clone()
        .and_then(|dash| StrokeDash::new(dash, state.stroke_dashoffset));
    with_paint_source(
        source,
        state,
        path,
        ids,
        styles,
        ancestors,
        stack,
        |paint| {
            pixmap.stroke_path(path, paint, &stroke, transform, clip);
        },
    );
}

fn with_paint_source<'a>(
    source: &PaintSource,
    state: &PaintState,
    path: &Path,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    draw: impl FnOnce(&Paint),
) {
    let mut paint = Paint::default();
    match source {
        PaintSource::Color(color) => {
            paint.set_color(apply_opacity(*color, state.opacity).to_tiny_skia());
            draw(&paint);
        }
        PaintSource::Url(id, fallback) => {
            if let Some(pattern) = ids
                .get(id)
                .copied()
                .filter(|node| matches!(node.kind, SvgElementKind::Pattern))
            {
                if let Some(tile) =
                    render_pattern_tile(pattern, state, ids, styles, ancestors, stack)
                {
                    let Some(src) = PixmapRef::from_bytes(tile.data(), tile.width(), tile.height())
                    else {
                        paint.set_color(
                            apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity)
                                .to_tiny_skia(),
                        );
                        draw(&paint);
                        return;
                    };
                    paint.shader = tiny_skia::Pattern::new(
                        src,
                        SpreadMode::Repeat,
                        tiny_skia::FilterQuality::Nearest,
                        state.opacity.clamp(0.0, 1.0),
                        pattern_transform(pattern, state),
                    );
                    draw(&paint);
                } else {
                    paint.set_color(
                        apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity)
                            .to_tiny_skia(),
                    );
                    draw(&paint);
                }
            } else if let Some(shader) = ids
                .get(id)
                .and_then(|node| gradient_shader(node, state.opacity, path, ids))
            {
                paint.shader = shader;
                draw(&paint);
            } else {
                paint.set_color(
                    apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity).to_tiny_skia(),
                );
                draw(&paint);
            }
        }
    }
}

fn render_pattern_tile<'a>(
    pattern: &'a SvgNode,
    inherited: &PaintState,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
) -> Option<Pixmap> {
    let width = attr_length(pattern, "width", LengthAxis::X, inherited)
        .unwrap_or(0.0)
        .ceil()
        .max(1.0) as u32;
    let height = attr_length(pattern, "height", LengthAxis::Y, inherited)
        .unwrap_or(0.0)
        .ceil()
        .max(1.0) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let mut tile = Pixmap::new(width, height)?;
    let mut transform = Transform::identity();
    if let Some(vb) = pattern
        .attr_ascii_case_insensitive("viewBox")
        .and_then(|v| parse_view_box(Some(v)))
    {
        transform = transform
            .pre_scale(width as f32 / vb.width, height as f32 / vb.height)
            .pre_translate(-vb.min_x, -vb.min_y);
    }
    for child in &pattern.children {
        paint_node(
            child,
            &mut tile,
            inherited.clone(),
            transform,
            ids,
            styles,
            ancestors,
            stack,
            None,
            true,
        );
    }
    Some(tile)
}

fn pattern_transform(pattern: &SvgNode, state: &PaintState) -> Transform {
    let local = pattern
        .attr("patternTransform")
        .and_then(parse_transform_list)
        .unwrap_or_else(Transform::identity);
    local.pre_translate(
        -attr_length(pattern, "x", LengthAxis::X, state).unwrap_or(0.0),
        -attr_length(pattern, "y", LengthAxis::Y, state).unwrap_or(0.0),
    )
}

fn gradient_shader(
    node: &SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &SvgNode>,
) -> Option<tiny_skia::Shader<'static>> {
    match node.kind {
        SvgElementKind::LinearGradient => linear_gradient_shader(node, opacity, path, ids),
        SvgElementKind::RadialGradient => radial_gradient_shader(node, opacity, path, ids),
        _ => None,
    }
}

fn linear_gradient_shader(
    node: &SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &SvgNode>,
) -> Option<tiny_skia::Shader<'static>> {
    let bounds = path.bounds();
    let user_space =
        inherited_gradient_attr(node, "gradientUnits", ids).is_some_and(|v| v == "userSpaceOnUse");
    let x1 = gradient_coord(
        node,
        ids,
        "x1",
        if user_space { 0.0 } else { bounds.left() },
        bounds.width(),
        user_space,
        0.0,
    );
    let y1 = gradient_coord(
        node,
        ids,
        "y1",
        if user_space { 0.0 } else { bounds.top() },
        bounds.height(),
        user_space,
        0.0,
    );
    let x2 = gradient_coord(
        node,
        ids,
        "x2",
        if user_space { 0.0 } else { bounds.left() },
        bounds.width(),
        user_space,
        if user_space { 0.0 } else { 1.0 },
    );
    let y2 = gradient_coord(
        node,
        ids,
        "y2",
        if user_space { 0.0 } else { bounds.top() },
        bounds.height(),
        user_space,
        0.0,
    );
    LinearGradient::new(
        SkPoint::from_xy(x1, y1),
        SkPoint::from_xy(x2, y2),
        gradient_stops(node, opacity, ids),
        gradient_spread(node, ids),
        gradient_transform(node, ids),
    )
}

fn radial_gradient_shader(
    node: &SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &SvgNode>,
) -> Option<tiny_skia::Shader<'static>> {
    let bounds = path.bounds();
    let user_space =
        inherited_gradient_attr(node, "gradientUnits", ids).is_some_and(|v| v == "userSpaceOnUse");
    let cx = gradient_coord(
        node,
        ids,
        "cx",
        if user_space { 0.0 } else { bounds.left() },
        bounds.width(),
        user_space,
        0.5,
    );
    let cy = gradient_coord(
        node,
        ids,
        "cy",
        if user_space { 0.0 } else { bounds.top() },
        bounds.height(),
        user_space,
        0.5,
    );
    let fx = gradient_coord(
        node,
        ids,
        "fx",
        if user_space { 0.0 } else { bounds.left() },
        bounds.width(),
        user_space,
        if user_space { cx } else { 0.5 },
    );
    let fy = gradient_coord(
        node,
        ids,
        "fy",
        if user_space { 0.0 } else { bounds.top() },
        bounds.height(),
        user_space,
        if user_space { cy } else { 0.5 },
    );
    let r = gradient_length(
        node,
        ids,
        "r",
        bounds.width().max(bounds.height()),
        user_space,
        if user_space { 0.0 } else { 0.5 },
    );
    RadialGradient::new(
        SkPoint::from_xy(fx, fy),
        0.0,
        SkPoint::from_xy(cx, cy),
        r.max(0.0),
        gradient_stops(node, opacity, ids),
        gradient_spread(node, ids),
        gradient_transform(node, ids),
    )
}

fn gradient_spread(node: &SvgNode, ids: &HashMap<String, &SvgNode>) -> SpreadMode {
    match inherited_gradient_attr(node, "spreadMethod", ids).as_deref() {
        Some("reflect") => SpreadMode::Reflect,
        Some("repeat") => SpreadMode::Repeat,
        _ => SpreadMode::Pad,
    }
}

fn gradient_transform(node: &SvgNode, ids: &HashMap<String, &SvgNode>) -> Transform {
    inherited_gradient_attr(node, "gradientTransform", ids)
        .as_deref()
        .and_then(parse_transform_list)
        .unwrap_or_else(Transform::identity)
}

fn gradient_coord(
    node: &SvgNode,
    ids: &HashMap<String, &SvgNode>,
    name: &str,
    base: f32,
    span: f32,
    user_space: bool,
    default: f32,
) -> f32 {
    let value = inherited_gradient_attr(node, name, ids)
        .as_deref()
        .and_then(percent_or_number)
        .unwrap_or(default);
    if user_space {
        value
    } else {
        base + value * span
    }
}

fn gradient_length(
    node: &SvgNode,
    ids: &HashMap<String, &SvgNode>,
    name: &str,
    span: f32,
    user_space: bool,
    default: f32,
) -> f32 {
    let value = inherited_gradient_attr(node, name, ids)
        .as_deref()
        .and_then(percent_or_number)
        .unwrap_or(default);
    if user_space {
        value
    } else {
        value * span
    }
}

fn percent_or_number(value: &str) -> Option<f32> {
    let trimmed = value.trim();
    if let Some(percent) = trimmed.strip_suffix('%') {
        number(percent).map(|v| v / 100.0)
    } else {
        number(trimmed)
    }
}

fn gradient_stops(
    node: &SvgNode,
    opacity: f32,
    ids: &HashMap<String, &SvgNode>,
) -> Vec<SkGradientStop> {
    let mut stops = Vec::new();
    collect_gradient_stops(node, opacity, ids, &mut stops, &mut Vec::new());
    stops
}

fn collect_gradient_stops(
    node: &SvgNode,
    opacity: f32,
    ids: &HashMap<String, &SvgNode>,
    stops: &mut Vec<SkGradientStop>,
    stack: &mut Vec<String>,
) {
    for child in &node.children {
        if svg_tag_name(child).eq_ignore_ascii_case("stop") {
            let offset = percent_or_number(child.attr("offset").unwrap_or("0")).unwrap_or(0.0);
            let mut color = child
                .attr("stop-color")
                .and_then(parse_color)
                .unwrap_or(Color::BLACK);
            if let Some(style) = child.attr("style") {
                for decl in style.split(';') {
                    if let Some((name, value)) = decl.split_once(':') {
                        let name = name.trim();
                        let value = value.trim();
                        if name == "stop-color" {
                            if let Some(parsed) = parse_color(value) {
                                color = parsed;
                            }
                        } else if name == "stop-opacity" {
                            if let Some(alpha) = number(value) {
                                color = with_alpha(color, alpha);
                            }
                        }
                    }
                }
            }
            if let Some(alpha) = child.attr("stop-opacity").and_then(number) {
                color = with_alpha(color, alpha);
            }
            stops.push(SkGradientStop::new(
                offset,
                apply_opacity(color, opacity).to_tiny_skia(),
            ));
        }
    }
    if !stops.is_empty() {
        return;
    }
    let Some(id) = href_id(node) else {
        return;
    };
    if stack.iter().any(|seen| seen == id) {
        return;
    }
    let Some(parent) = ids.get(id).copied() else {
        return;
    };
    stack.push(id.to_string());
    collect_gradient_stops(parent, opacity, ids, stops, stack);
    stack.pop();
}

fn inherited_gradient_attr(
    node: &SvgNode,
    name: &str,
    ids: &HashMap<String, &SvgNode>,
) -> Option<String> {
    if let Some(value) = node.attr(name) {
        return Some(value.to_string());
    }
    let id = href_id(node)?;
    ids.get(id)
        .and_then(|parent| inherited_gradient_attr(parent, name, ids))
}

fn href_id(node: &SvgNode) -> Option<&str> {
    node.attr("href")
        .or_else(|| node.attr("xlink:href"))
        .or_else(|| {
            node.attributes
                .iter()
                .find(|a| a.namespace.as_deref() == Some("xlink") && a.name == "href")
                .map(|a| a.value.as_str())
        })
        .and_then(|href| href.strip_prefix('#'))
}

fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = ((color.a as f32) * opacity.clamp(0.0, 1.0)).round() as u8;
    color
}

fn viewport_user_size(
    view_box: Option<SvgViewBox>,
    intrinsic_w: f32,
    intrinsic_h: f32,
    raster_w: u32,
    raster_h: u32,
) -> (f32, f32) {
    if let Some(vb) = view_box {
        return (vb.width.max(0.0), vb.height.max(0.0));
    }
    (
        intrinsic_w.max(raster_w as f32).max(1.0),
        intrinsic_h.max(raster_h as f32).max(1.0),
    )
}

fn view_box_transform(
    vb: SvgViewBox,
    viewport_w: f32,
    viewport_h: f32,
    preserve_aspect_ratio: PreserveAspectRatio,
) -> Transform {
    if vb.width <= 0.0 || vb.height <= 0.0 || viewport_w <= 0.0 || viewport_h <= 0.0 {
        return Transform::identity();
    }
    match preserve_aspect_ratio {
        PreserveAspectRatio::None => {
            let sx = viewport_w / vb.width;
            let sy = viewport_h / vb.height;
            Transform::from_row(sx, 0.0, 0.0, sy, -vb.min_x * sx, -vb.min_y * sy)
        }
        PreserveAspectRatio::Meet { align_x, align_y }
        | PreserveAspectRatio::Slice { align_x, align_y } => {
            let sx = viewport_w / vb.width;
            let sy = viewport_h / vb.height;
            let scale = if matches!(preserve_aspect_ratio, PreserveAspectRatio::Slice { .. }) {
                sx.max(sy)
            } else {
                sx.min(sy)
            };
            let content_w = vb.width * scale;
            let content_h = vb.height * scale;
            let extra_x = viewport_w - content_w;
            let extra_y = viewport_h - content_h;
            let align_dx = match align_x {
                AlignX::Min => 0.0,
                AlignX::Mid => extra_x / 2.0,
                AlignX::Max => extra_x,
            };
            let align_dy = match align_y {
                AlignY::Min => 0.0,
                AlignY::Mid => extra_y / 2.0,
                AlignY::Max => extra_y,
            };
            Transform::from_row(
                scale,
                0.0,
                0.0,
                scale,
                align_dx - vb.min_x * scale,
                align_dy - vb.min_y * scale,
            )
        }
    }
}

#[derive(Clone, Copy)]
enum LengthAxis {
    X,
    Y,
    Normalized,
}

fn attr_length(node: &SvgNode, name: &str, axis: LengthAxis, state: &PaintState) -> Option<f32> {
    node.attr_ascii_case_insensitive(name)
        .and_then(|value| resolve_svg_length(value, axis, state))
}

fn resolve_svg_length(value: &str, axis: LengthAxis, state: &PaintState) -> Option<f32> {
    match parse_svg_length(value)? {
        SvgLength::Number(v) | SvgLength::Px(v) => Some(v),
        SvgLength::Percent(v) => Some(v / 100.0 * length_reference(axis, state)),
        SvgLength::Em(v) | SvgLength::Rem(v) => Some(v * state.font_size.max(1.0)),
    }
    .filter(|v| v.is_finite())
}

fn length_reference(axis: LengthAxis, state: &PaintState) -> f32 {
    match axis {
        LengthAxis::X => state.viewport_width.max(1.0),
        LengthAxis::Y => state.viewport_height.max(1.0),
        LengthAxis::Normalized => ((state.viewport_width.powi(2) + state.viewport_height.powi(2))
            / 2.0)
            .sqrt()
            .max(1.0),
    }
}

fn rect_path(node: &SvgNode, state: &PaintState) -> Option<Path> {
    let x = attr_length(node, "x", LengthAxis::X, state).unwrap_or(0.0);
    let y = attr_length(node, "y", LengthAxis::Y, state).unwrap_or(0.0);
    let w = attr_length(node, "width", LengthAxis::X, state)?;
    let h = attr_length(node, "height", LengthAxis::Y, state)?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut b = PathBuilder::new();
    let mut rx = attr_length(node, "rx", LengthAxis::X, state);
    let mut ry = attr_length(node, "ry", LengthAxis::Y, state);
    if rx.is_none() {
        rx = ry;
    }
    if ry.is_none() {
        ry = rx;
    }
    let rx = rx.unwrap_or(0.0).abs().min(w / 2.0);
    let ry = ry.unwrap_or(0.0).abs().min(h / 2.0);
    if rx == 0.0 || ry == 0.0 {
        b.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
    } else {
        rounded_rect_path(&mut b, x, y, w, h, rx, ry);
    }
    b.finish()
}

fn rounded_rect_path(b: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) {
    const K: f32 = 0.552_284_8;
    let right = x + w;
    let bottom = y + h;
    b.move_to(x + rx, y);
    b.line_to(right - rx, y);
    b.cubic_to(
        right - rx + rx * K,
        y,
        right,
        y + ry - ry * K,
        right,
        y + ry,
    );
    b.line_to(right, bottom - ry);
    b.cubic_to(
        right,
        bottom - ry + ry * K,
        right - rx + rx * K,
        bottom,
        right - rx,
        bottom,
    );
    b.line_to(x + rx, bottom);
    b.cubic_to(
        x + rx - rx * K,
        bottom,
        x,
        bottom - ry + ry * K,
        x,
        bottom - ry,
    );
    b.line_to(x, y + ry);
    b.cubic_to(x, y + ry - ry * K, x + rx - rx * K, y, x + rx, y);
    b.close();
}

fn circle_path(node: &SvgNode, state: &PaintState) -> Option<Path> {
    let cx = attr_length(node, "cx", LengthAxis::X, state).unwrap_or(0.0);
    let cy = attr_length(node, "cy", LengthAxis::Y, state).unwrap_or(0.0);
    let r = attr_length(node, "r", LengthAxis::Normalized, state)?;
    let mut b = PathBuilder::new();
    b.push_circle(cx, cy, r);
    b.finish()
}

fn ellipse_path(node: &SvgNode, state: &PaintState) -> Option<Path> {
    let cx = attr_length(node, "cx", LengthAxis::X, state).unwrap_or(0.0);
    let cy = attr_length(node, "cy", LengthAxis::Y, state).unwrap_or(0.0);
    let rx = attr_length(node, "rx", LengthAxis::X, state)?;
    let ry = attr_length(node, "ry", LengthAxis::Y, state)?;
    let mut b = PathBuilder::new();
    b.push_oval(tiny_skia::Rect::from_xywh(
        cx - rx,
        cy - ry,
        rx * 2.0,
        ry * 2.0,
    )?);
    b.finish()
}

fn line_path(node: &SvgNode, state: &PaintState) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(
        attr_length(node, "x1", LengthAxis::X, state).unwrap_or(0.0),
        attr_length(node, "y1", LengthAxis::Y, state).unwrap_or(0.0),
    );
    b.line_to(
        attr_length(node, "x2", LengthAxis::X, state).unwrap_or(0.0),
        attr_length(node, "y2", LengthAxis::Y, state).unwrap_or(0.0),
    );
    b.finish()
}

fn points_list(node: &SvgNode, _state: &PaintState) -> Vec<(f32, f32)> {
    let Some(raw) = node.attr("points") else {
        return Vec::new();
    };
    let values = number_list(raw);
    values
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

fn points_path_from_pairs(points: &[(f32, f32)], close: bool) -> Option<Path> {
    if points.len() < 2 {
        return None;
    }
    let mut b = PathBuilder::new();
    b.move_to(points[0].0, points[0].1);
    for (x, y) in &points[1..] {
        b.line_to(*x, *y);
    }
    if close {
        b.close();
    }
    b.finish()
}

fn flatten_path_points(path: &Path) -> Vec<(f32, f32)> {
    let mut points = Vec::new();
    let mut start = (0.0f32, 0.0f32);
    let mut current = (0.0f32, 0.0f32);
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                start = (p.x, p.y);
                current = start;
                points.push(current);
            }
            PathSegment::LineTo(p) => {
                current = (p.x, p.y);
                points.push(current);
            }
            PathSegment::QuadTo(c, p) => {
                let end = (p.x, p.y);
                for (_, to) in flatten_quad_points(current, (c.x, c.y), end) {
                    points.push(to);
                }
                current = end;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let end = (p.x, p.y);
                for (_, to) in flatten_cubic_points(current, (c1.x, c1.y), (c2.x, c2.y), end) {
                    points.push(to);
                }
                current = end;
            }
            PathSegment::Close => {
                current = start;
                points.push(current);
            }
        }
    }
    points
}

fn flatten_quad_points(
    p0: (f32, f32),
    c: (f32, f32),
    p1: (f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let mut prev = p0;
    for i in 1..=16 {
        let t = i as f32 / 16.0;
        let mt = 1.0 - t;
        let next = (
            mt * mt * p0.0 + 2.0 * mt * t * c.0 + t * t * p1.0,
            mt * mt * p0.1 + 2.0 * mt * t * c.1 + t * t * p1.1,
        );
        out.push((prev, next));
        prev = next;
    }
    out
}

fn flatten_cubic_points(
    p0: (f32, f32),
    c1: (f32, f32),
    c2: (f32, f32),
    p1: (f32, f32),
) -> Vec<((f32, f32), (f32, f32))> {
    let mut out = Vec::new();
    let mut prev = p0;
    for i in 1..=24 {
        let t = i as f32 / 24.0;
        let mt = 1.0 - t;
        let next = (
            mt.powi(3) * p0.0
                + 3.0 * mt.powi(2) * t * c1.0
                + 3.0 * mt * t.powi(2) * c2.0
                + t.powi(3) * p1.0,
            mt.powi(3) * p0.1
                + 3.0 * mt.powi(2) * t * c1.1
                + 3.0 * mt * t.powi(2) * c2.1
                + t.powi(3) * p1.1,
        );
        out.push((prev, next));
        prev = next;
    }
    out
}

fn path_polyline_length(points: &[(f32, f32)]) -> f32 {
    points
        .windows(2)
        .map(|pair| segment_length(pair[0], pair[1]))
        .sum()
}

fn point_at_path_distance(points: &[(f32, f32)], distance: f32) -> Option<(f32, f32, f32)> {
    let mut remaining = distance.max(0.0);
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let len = segment_length(from, to);
        if len <= f32::EPSILON {
            continue;
        }
        if remaining <= len {
            let t = remaining / len;
            let x = from.0 + (to.0 - from.0) * t;
            let y = from.1 + (to.1 - from.1) * t;
            return Some((x, y, (to.1 - from.1).atan2(to.0 - from.0)));
        }
        remaining -= len;
    }
    points.windows(2).last().and_then(|pair| {
        let from = pair[0];
        let to = pair[1];
        let len = segment_length(from, to);
        (len > f32::EPSILON).then(|| (to.0, to.1, (to.1 - from.1).atan2(to.0 - from.0)))
    })
}

fn segment_length(from: (f32, f32), to: (f32, f32)) -> f32 {
    ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt()
}

fn path_marker_points(data: &str) -> (Vec<(f32, f32)>, bool) {
    let mut p = PathDataParser {
        data,
        pos: 0,
        cmd: 'M',
        x: 0.0,
        y: 0.0,
        sx: 0.0,
        sy: 0.0,
        last_cubic_ctrl: None,
        last_quad_ctrl: None,
    };
    let mut points = Vec::new();
    let mut closed = false;
    while p.skip_separators() {
        if let Some(c) = p.peek_cmd() {
            p.cmd = c;
            p.pos += c.len_utf8();
        }
        let relative = p.cmd.is_ascii_lowercase();
        match p.cmd.to_ascii_uppercase() {
            'M' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                p.sx = x;
                p.sy = y;
                points.push((x, y));
                p.cmd = if relative { 'l' } else { 'L' };
                p.clear_controls();
            }
            'L' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'H' => {
                let Some(mut x) = p.num() else { break };
                if relative {
                    x += p.x;
                }
                p.x = x;
                points.push((p.x, p.y));
                p.clear_controls();
            }
            'V' => {
                let Some(mut y) = p.num() else { break };
                if relative {
                    y += p.y;
                }
                p.y = y;
                points.push((p.x, p.y));
                p.clear_controls();
            }
            'C' => {
                if p.pair(relative).is_none() || p.pair(relative).is_none() {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'S' | 'Q' => {
                if p.pair(relative).is_none() {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'T' => {
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'A' => {
                if p.num().is_none()
                    || p.num().is_none()
                    || p.num().is_none()
                    || p.flag().is_none()
                    || p.flag().is_none()
                {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else {
                    break;
                };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'Z' => {
                points.push((p.sx, p.sy));
                p.x = p.sx;
                p.y = p.sy;
                closed = true;
                p.clear_controls();
            }
            _ => break,
        }
    }
    (points, closed)
}

fn number(value: &str) -> Option<f32> {
    let token = value
        .trim()
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()
        .unwrap_or("");
    let end = token
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E'))
        .last()
        .map(|(idx, c)| idx + c.len_utf8())
        .unwrap_or(0);
    if end == 0 {
        return None;
    }
    token[..end].parse::<f32>().ok().filter(|v| v.is_finite())
}

fn number_list(value: &str) -> Vec<f32> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(number)
        .collect()
}

fn parse_transform_list(value: &str) -> Option<Transform> {
    let mut rest = value.trim();
    let mut transform = Transform::identity();
    while !rest.is_empty() {
        let open = rest.find('(')?;
        let name = rest[..open].trim();
        let close_rel = rest[open + 1..].find(')')?;
        let args = number_list(&rest[open + 1..open + 1 + close_rel]);
        let local = match name {
            "matrix" if args.len() >= 6 => {
                Transform::from_row(args[0], args[1], args[2], args[3], args[4], args[5])
            }
            "translate" if !args.is_empty() => {
                Transform::from_translate(args[0], args.get(1).copied().unwrap_or(0.0))
            }
            "scale" if !args.is_empty() => {
                Transform::from_scale(args[0], args.get(1).copied().unwrap_or(args[0]))
            }
            "rotate" if args.len() >= 3 => Transform::from_rotate_at(args[0], args[1], args[2]),
            "rotate" if !args.is_empty() => Transform::from_rotate(args[0]),
            "skewX" if !args.is_empty() => Transform::from_skew(args[0].to_radians().tan(), 0.0),
            "skewY" if !args.is_empty() => Transform::from_skew(0.0, args[0].to_radians().tan()),
            _ => return None,
        };
        transform = transform.pre_concat(local);
        rest = rest[open + 1 + close_rel + 1..].trim_start();
    }
    Some(transform)
}

fn parse_path_data(data: &str) -> Option<Path> {
    let mut p = PathDataParser {
        data,
        pos: 0,
        cmd: 'M',
        x: 0.0,
        y: 0.0,
        sx: 0.0,
        sy: 0.0,
        last_cubic_ctrl: None,
        last_quad_ctrl: None,
    };
    let mut b = PathBuilder::new();
    while p.skip_separators() {
        if let Some(c) = p.peek_cmd() {
            p.cmd = c;
            p.pos += c.len_utf8();
        }
        let relative = p.cmd.is_ascii_lowercase();
        match p.cmd.to_ascii_uppercase() {
            'M' => {
                let (x, y) = p.pair(relative)?;
                b.move_to(x, y);
                p.x = x;
                p.y = y;
                p.sx = x;
                p.sy = y;
                p.cmd = if relative { 'l' } else { 'L' };
                p.clear_controls();
            }
            'L' => {
                let (x, y) = p.pair(relative)?;
                b.line_to(x, y);
                p.x = x;
                p.y = y;
                p.clear_controls();
            }
            'H' => {
                let mut x = p.num()?;
                if relative {
                    x += p.x;
                }
                b.line_to(x, p.y);
                p.x = x;
                p.clear_controls();
            }
            'V' => {
                let mut y = p.num()?;
                if relative {
                    y += p.y;
                }
                b.line_to(p.x, y);
                p.y = y;
                p.clear_controls();
            }
            'C' => {
                let (x1, y1) = p.pair(relative)?;
                let (x2, y2) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.cubic_to(x1, y1, x2, y2, x, y);
                p.x = x;
                p.y = y;
                p.last_cubic_ctrl = Some((x2, y2));
                p.last_quad_ctrl = None;
            }
            'S' => {
                let (x1, y1) = p
                    .last_cubic_ctrl
                    .map(|(cx, cy)| (p.x * 2.0 - cx, p.y * 2.0 - cy))
                    .unwrap_or((p.x, p.y));
                let (x2, y2) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.cubic_to(x1, y1, x2, y2, x, y);
                p.x = x;
                p.y = y;
                p.last_cubic_ctrl = Some((x2, y2));
                p.last_quad_ctrl = None;
            }
            'Q' => {
                let (x1, y1) = p.pair(relative)?;
                let (x, y) = p.pair(relative)?;
                b.quad_to(x1, y1, x, y);
                p.x = x;
                p.y = y;
                p.last_quad_ctrl = Some((x1, y1));
                p.last_cubic_ctrl = None;
            }
            'T' => {
                let (x1, y1) = p
                    .last_quad_ctrl
                    .map(|(qx, qy)| (p.x * 2.0 - qx, p.y * 2.0 - qy))
                    .unwrap_or((p.x, p.y));
                let (x, y) = p.pair(relative)?;
                b.quad_to(x1, y1, x, y);
                p.x = x;
                p.y = y;
                p.last_quad_ctrl = Some((x1, y1));
                p.last_cubic_ctrl = None;
            }
            'A' => {
                let rx = p.num()?.abs();
                let ry = p.num()?.abs();
                let angle = p.num()?;
                let large_arc = p.flag()?;
                let sweep = p.flag()?;
                let (x, y) = p.pair(relative)?;
                arc_to_cubic(&mut b, p.x, p.y, rx, ry, angle, large_arc, sweep, x, y);
                p.x = x;
                p.y = y;
                p.clear_controls();
            }
            'Z' => {
                b.close();
                p.x = p.sx;
                p.y = p.sy;
                p.clear_controls();
            }
            _ => return None,
        }
    }
    b.finish()
}

struct PathDataParser<'a> {
    data: &'a str,
    pos: usize,
    cmd: char,
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    last_cubic_ctrl: Option<(f32, f32)>,
    last_quad_ctrl: Option<(f32, f32)>,
}

impl<'a> PathDataParser<'a> {
    fn skip_separators(&mut self) -> bool {
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == ',' {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        self.pos < self.data.len()
    }

    fn peek_cmd(&self) -> Option<char> {
        self.peek().filter(|c| c.is_ascii_alphabetic())
    }

    fn peek(&self) -> Option<char> {
        self.data[self.pos..].chars().next()
    }

    fn pair(&mut self, relative: bool) -> Option<(f32, f32)> {
        let mut x = self.num()?;
        let mut y = self.num()?;
        if relative {
            x += self.x;
            y += self.y;
        }
        Some((x, y))
    }

    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        match self.peek()? {
            '0' => {
                self.pos += 1;
                Some(false)
            }
            '1' => {
                self.pos += 1;
                Some(true)
            }
            _ => None,
        }
    }

    fn clear_controls(&mut self) {
        self.last_cubic_ctrl = None;
        self.last_quad_ctrl = None;
    }

    fn num(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.pos;
        let mut saw_digit = false;
        let mut saw_exp = false;
        while let Some(c) = self.peek() {
            let ok = if c.is_ascii_digit() {
                saw_digit = true;
                true
            } else if matches!(c, '+' | '-') {
                self.pos == start || saw_exp
            } else if c == '.' {
                true
            } else if matches!(c, 'e' | 'E') {
                saw_exp = true;
                true
            } else {
                false
            };
            if !ok {
                break;
            }
            if saw_exp && !matches!(c, 'e' | 'E') {
                saw_exp = false;
            }
            self.pos += c.len_utf8();
        }
        if !saw_digit || self.pos == start {
            return None;
        }
        self.data[start..self.pos].parse::<f32>().ok()
    }
}

fn arc_to_cubic(
    b: &mut PathBuilder,
    x1: f32,
    y1: f32,
    mut rx: f32,
    mut ry: f32,
    x_axis_rotation: f32,
    large_arc: bool,
    sweep: bool,
    x2: f32,
    y2: f32,
) {
    if rx == 0.0 || ry == 0.0 || ((x1 - x2).abs() < f32::EPSILON && (y1 - y2).abs() < f32::EPSILON)
    {
        b.line_to(x2, y2);
        return;
    }

    let phi = x_axis_rotation.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    let lambda = x1p.powi(2) / rx.powi(2) + y1p.powi(2) / ry.powi(2);
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
    }

    let rx2 = rx.powi(2);
    let ry2 = ry.powi(2);
    let x1p2 = x1p.powi(2);
    let y1p2 = y1p.powi(2);
    let denom = rx2 * y1p2 + ry2 * x1p2;
    if denom == 0.0 {
        b.line_to(x2, y2);
        return;
    }
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let coef = sign
        * ((rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2) / denom)
            .max(0.0)
            .sqrt();
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    let theta1 = angle_between(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut delta = angle_between(
        (x1p - cxp) / rx,
        (y1p - cyp) / ry,
        (-x1p - cxp) / rx,
        (-y1p - cyp) / ry,
    );
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }

    let segments = (delta.abs() / (std::f32::consts::FRAC_PI_2))
        .ceil()
        .max(1.0) as usize;
    let step = delta / segments as f32;
    for i in 0..segments {
        let t1 = theta1 + i as f32 * step;
        arc_segment_to_cubic(b, cx, cy, rx, ry, cos_phi, sin_phi, t1, t1 + step);
    }
}

fn angle_between(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    let angle = (dot / len).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        -angle
    } else {
        angle
    }
}

fn arc_segment_to_cubic(
    b: &mut PathBuilder,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
    t1: f32,
    t2: f32,
) {
    let delta = t2 - t1;
    let alpha = (4.0 / 3.0) * (delta / 4.0).tan();
    let (sin_t1, cos_t1) = t1.sin_cos();
    let (sin_t2, cos_t2) = t2.sin_cos();
    let p1 = (cos_t1 - alpha * sin_t1, sin_t1 + alpha * cos_t1);
    let p2 = (cos_t2 + alpha * sin_t2, sin_t2 - alpha * cos_t2);
    let p = (cos_t2, sin_t2);
    let map = |x: f32, y: f32| {
        (
            cx + rx * x * cos_phi - ry * y * sin_phi,
            cy + rx * x * sin_phi + ry * y * cos_phi,
        )
    };
    let (x1, y1) = map(p1.0, p1.1);
    let (x2, y2) = map(p2.0, p2.1);
    let (x, y) = map(p.0, p.1);
    b.cubic_to(x1, y1, x2, y2, x, y);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_painted_pixel(data: &[u8]) -> bool {
        data.chunks_exact(4).any(|p| p[3] != 0)
    }

    fn painted_at(data: &[u8], width: usize, x: usize, y: usize) -> bool {
        let i = (y * width + x) * 4;
        data.get(i + 3).copied().unwrap_or(0) != 0
    }

    fn alpha_at(data: &[u8], width: usize, x: usize, y: usize) -> u8 {
        let i = (y * width + x) * 4;
        data.get(i + 3).copied().unwrap_or(0)
    }

    fn rgba_at(data: &[u8], width: usize, x: usize, y: usize) -> (u8, u8, u8, u8) {
        let i = (y * width + x) * 4;
        (
            data.get(i).copied().unwrap_or(0),
            data.get(i + 1).copied().unwrap_or(0),
            data.get(i + 2).copied().unwrap_or(0),
            data.get(i + 3).copied().unwrap_or(0),
        )
    }

    fn painted_in(data: &[u8], width: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> bool {
        for y in y0..y1 {
            for x in x0..x1 {
                if painted_at(data, width, x, y) {
                    return true;
                }
            }
        }
        false
    }

    fn painted_bounds(data: &[u8], width: usize) -> Option<(usize, usize, usize, usize)> {
        let height = data.len() / 4 / width;
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        for y in 0..height {
            for x in 0..width {
                if painted_at(data, width, x, y) {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        (min_x != usize::MAX).then_some((min_x, min_y, max_x, max_y))
    }

    #[test]
    fn native_rasterizer_paints_rect_without_resvg() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#,
            10,
            10,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_paints_basic_path() {
        let data = rasterize_svg_to_rgba(
            r##"<svg viewBox="0 0 10 10"><path d="M1 1 L9 1 L9 9 Z" fill="#00ff00"/></svg>"##,
            10,
            10,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_applies_group_transform() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="10"><g transform="translate(10,0)"><rect width="5" height="5"/></g></svg>"#,
            20,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 20, 2, 2));
        assert!(painted_at(&data, 20, 12, 2));
    }

    #[test]
    fn native_rasterizer_composites_group_opacity_as_one_layer() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="10"><g opacity="0.5"><rect x="2" y="2" width="8" height="6" fill="black"/><rect x="6" y="2" width="8" height="6" fill="black"/></g></svg>"#,
            20,
            10,
        )
        .unwrap();
        assert!((110..=145).contains(&alpha_at(&data, 20, 7, 5)));
    }

    #[test]
    fn native_rasterizer_resolves_percentage_shape_lengths_against_viewport() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="10"><rect width="50%" height="100%" fill="black"/></svg>"#,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 5, 5));
        assert!(!painted_at(&data, 20, 15, 5));
    }

    #[test]
    fn native_rasterizer_applies_nested_svg_viewport_and_viewbox() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="30" height="10"><svg x="10" y="0" width="10" height="10" viewBox="0 0 20 20"><rect width="40" height="20" fill="black"/></svg></svg>"#,
            30,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 30, 5, 5));
        assert!(painted_at(&data, 30, 12, 5));
        assert!(!painted_at(&data, 30, 22, 5));
    }

    #[test]
    fn native_rasterizer_honors_nested_svg_overflow_visible() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="30" height="10"><svg x="10" y="0" width="10" height="10" overflow="visible"><rect width="20" height="10" fill="black"/></svg></svg>"#,
            30,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 30, 5, 5));
        assert!(painted_at(&data, 30, 12, 5));
        assert!(painted_at(&data, 30, 22, 5));
    }

    #[test]
    fn native_rasterizer_paints_smooth_curves_and_arcs() {
        let data = rasterize_svg_to_rgba(
            r#"<svg viewBox="0 0 20 20"><path d="M2 10 C4 2 8 2 10 10 S16 18 18 10 M2 18 A4 4 0 0 1 10 18" fill="none" stroke="black" stroke-width="1"/></svg>"#,
            20,
            20,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_accepts_stroke_presentation_attributes() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="20"><path d="M2 2 L18 2 L18 18" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="2 2"/></svg>"#,
            20,
            20,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_resolves_defs_use_references() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><rect id="r" width="5" height="5" fill="#000"/></defs><use href="#r" x="10"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 20, 2, 2));
        assert!(painted_at(&data, 20, 12, 2));
    }

    #[test]
    fn native_rasterizer_scales_symbol_use_viewbox_to_use_viewport() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><symbol id="s" viewBox="0 0 100 50"><rect width="100" height="50" fill="black"/></symbol></defs><use href="#s" x="5" y="2" width="10" height="5"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 20, 3, 4));
        assert!(painted_at(&data, 20, 10, 4));
        assert!(!painted_at(&data, 20, 17, 4));
    }

    #[test]
    fn native_rasterizer_resolves_linear_gradient_paint() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><linearGradient id="g"><stop offset="0%" stop-color="red"/><stop offset="100%" stop-color="blue"/></linearGradient></defs><rect width="20" height="10" fill="url(#g)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_resolves_radial_gradient_paint() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="20"><defs><radialGradient id="g"><stop offset="0%" stop-color="white"/><stop offset="100%" stop-color="black"/></radialGradient></defs><circle cx="10" cy="10" r="8" fill="url(#g)"/></svg>"##,
            20,
            20,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_applies_internal_style_rules() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="10" height="10"><style>.hot { fill: red; }</style><rect class="hot" width="10" height="10"/></svg>"#,
            10,
            10,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_matches_svg_functional_selector_pseudos() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="30" height="10"><style>rect:not(.skip){fill:black}:is(circle,.target){fill:black}</style><rect class="skip" width="10" height="10" fill="none"/><rect x="10" width="10" height="10" fill="none"/><circle class="target" cx="25" cy="5" r="5" fill="none"/></svg>"#,
            30,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 30, 5, 5));
        assert!(painted_at(&data, 30, 15, 5));
        assert!(painted_at(&data, 30, 25, 5));
    }

    #[test]
    fn native_rasterizer_matches_svg_sibling_selectors() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="40" height="10"><style>.start + rect{fill:black}.start ~ circle{fill:black}</style><rect class="start" width="10" height="10" fill="none"/><rect x="10" width="10" height="10" fill="none"/><rect x="20" width="10" height="10" fill="none"/><circle cx="35" cy="5" r="5" fill="none"/></svg>"#,
            40,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 40, 5, 5));
        assert!(painted_at(&data, 40, 15, 5));
        assert!(!painted_at(&data, 40, 25, 5));
        assert!(painted_at(&data, 40, 35, 5));
    }

    #[test]
    fn native_rasterizer_matches_svg_structural_pseudos() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="60" height="10"><style>rect:first-of-type{fill:black}rect:nth-child(4){fill:black}circle:last-of-type{fill:black}</style><rect width="10" height="10" fill="none"/><circle cx="15" cy="5" r="5" fill="none"/><rect x="20" width="10" height="10" fill="none"/><circle cx="35" cy="5" r="5" fill="none"/><rect x="40" width="10" height="10" fill="none"/></svg>"#,
            60,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 60, 5, 5));
        assert!(!painted_at(&data, 60, 15, 5));
        assert!(painted_at(&data, 60, 25, 5));
        assert!(painted_at(&data, 60, 35, 5));
        assert!(!painted_at(&data, 60, 45, 5));
    }

    #[test]
    fn native_rasterizer_paints_text_with_svg_font_attrs() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="80" height="30"><text x="4" y="22" fill="black" font-size="20" font-weight="700">SVG</text></svg>"#,
            80,
            30,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_positions_nested_tspan_text() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="80" height="24"><style>tspan.hot{fill:black}</style><text x="2" y="16" fill="black" font-size="14">A<tspan class="hot" x="42" dy="0">B</tspan></text></svg>"#,
            80,
            24,
        )
        .unwrap();
        assert!(painted_in(&data, 80, 1, 4, 18, 20));
        assert!(!painted_in(&data, 80, 24, 4, 36, 20));
        assert!(painted_in(&data, 80, 40, 4, 58, 20));
    }

    #[test]
    fn native_rasterizer_applies_tspan_baseline_shift() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="80" height="36"><text x="4" y="30" fill="black" font-size="20">A<tspan x="42" baseline-shift="-14">B</tspan></text></svg>"#,
            80,
            36,
        )
        .unwrap();
        assert!(painted_in(&data, 80, 4, 16, 24, 35));
        assert!(painted_in(&data, 80, 42, 2, 62, 20));
    }

    #[test]
    fn native_rasterizer_applies_text_length_spacing() {
        let normal = rasterize_svg_to_rgba(
            r#"<svg width="120" height="34"><text x="4" y="26" fill="black" font-size="24">II</text></svg>"#,
            120,
            34,
        )
        .unwrap();
        let adjusted = rasterize_svg_to_rgba(
            r#"<svg width="120" height="34"><text x="4" y="26" fill="black" font-size="24" textLength="70">II</text></svg>"#,
            120,
            34,
        )
        .unwrap();
        let normal_bounds = painted_bounds(&normal, 120).expect("normal text bounds");
        let adjusted_bounds = painted_bounds(&adjusted, 120).expect("textLength text bounds");
        assert!(
            adjusted_bounds.2 > normal_bounds.2 + 30,
            "textLength spacing should widen painted text, normal={normal_bounds:?} adjusted={adjusted_bounds:?}"
        );
    }

    #[test]
    fn native_rasterizer_applies_svg_letter_spacing() {
        let normal = rasterize_svg_to_rgba(
            r#"<svg width="90" height="34"><text x="4" y="26" fill="black" font-size="24">II</text></svg>"#,
            90,
            34,
        )
        .unwrap();
        let spaced = rasterize_svg_to_rgba(
            r#"<svg width="90" height="34"><text x="4" y="26" fill="black" font-size="24" letter-spacing="18">II</text></svg>"#,
            90,
            34,
        )
        .unwrap();
        let normal_bounds = painted_bounds(&normal, 90).expect("normal text bounds");
        let spaced_bounds = painted_bounds(&spaced, 90).expect("spaced text bounds");
        assert!(
            spaced_bounds.2 > normal_bounds.2 + 10,
            "letter-spacing should widen painted text, normal={normal_bounds:?} spaced={spaced_bounds:?}"
        );
    }

    #[test]
    fn native_rasterizer_uses_paint_server_fallback_color() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="10" height="10"><rect width="10" height="10" fill="url(#missing) rgb(255, 0, 0)"/></svg>"##,
            10,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 10, 5, 5);
        assert!(r > 200 && g < 50 && b < 50 && a > 200);
    }

    #[test]
    fn native_rasterizer_applies_paint_order_for_fill_and_stroke() {
        let normal = rasterize_svg_to_rgba(
            r#"<svg width="40" height="40"><rect x="10" y="10" width="20" height="20" fill="blue" stroke="red" stroke-width="12"/></svg>"#,
            40,
            40,
        )
        .unwrap();
        let stroke_under_fill = rasterize_svg_to_rgba(
            r#"<svg width="40" height="40"><rect x="10" y="10" width="20" height="20" fill="blue" stroke="red" stroke-width="12" paint-order="stroke fill"/></svg>"#,
            40,
            40,
        )
        .unwrap();
        let (nr, ng, nb, _) = rgba_at(&normal, 40, 11, 20);
        let (or, og, ob, _) = rgba_at(&stroke_under_fill, 40, 11, 20);
        assert!(
            nr > 150 && ng < 80 && nb < 120,
            "normal pixel was {nr},{ng},{nb}"
        );
        assert!(
            ob > 150 && or < 120 && og < 120,
            "ordered pixel was {or},{og},{ob}"
        );
    }

    #[test]
    fn native_rasterizer_paints_text_path() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="120" height="40">
                <defs><path id="baseline" d="M10 28 H110"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline" startOffset="10">SVG</textPath></text>
            </svg>"##,
            120,
            40,
        )
        .unwrap();
        assert!(painted_in(&data, 120, 15, 10, 75, 34));
        assert!(!painted_in(&data, 120, 15, 0, 75, 8));
    }

    #[test]
    fn native_rasterizer_applies_per_character_text_position_lists() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="100" height="30"><text x="4 50" y="24 24" fill="black" font-size="20">AB</text></svg>"#,
            100,
            30,
        )
        .unwrap();
        assert!(painted_in(&data, 100, 2, 8, 25, 29));
        assert!(!painted_in(&data, 100, 28, 8, 45, 29));
        assert!(painted_in(&data, 100, 48, 8, 75, 29));
    }

    #[test]
    fn native_rasterizer_resolves_svg_var_fallback_paint() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10" fill="none"><path d="M0 0H20V10H0Z" fill="var(--brand, #7D2EFF)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 5, 5));
    }

    #[test]
    fn native_rasterizer_resolves_inherited_gradient_stops() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><linearGradient id="base"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><linearGradient id="child" href="#base" x1="0" x2="100%" gradientTransform="translate(0 0)" spreadMethod="repeat"/></defs><rect width="20" height="10" fill="url(#child)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(has_painted_pixel(&data));
    }

    #[test]
    fn native_rasterizer_applies_clip_path() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><clipPath id="c"><rect width="5" height="10"/></clipPath></defs><rect width="20" height="10" clip-path="url(#c)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 2, 2));
        assert!(!painted_at(&data, 20, 12, 2));
    }

    #[test]
    fn native_rasterizer_applies_luminance_mask() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><mask id="m"><rect width="5" height="10" fill="white"/></mask></defs><rect width="20" height="10" mask="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 2, 2));
        assert!(!painted_at(&data, 20, 12, 2));
    }

    #[test]
    fn native_rasterizer_paints_line_marker_end() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><marker id="m" markerWidth="4" markerHeight="4" refX="0" refY="2"><rect width="4" height="4" fill="black"/></marker></defs><line x1="1" y1="5" x2="12" y2="5" stroke="black" marker-end="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 13, 5));
    }

    #[test]
    fn native_rasterizer_paints_line_marker_start() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><marker id="m" markerWidth="4" markerHeight="4" refX="4" refY="2"><rect width="4" height="4" fill="black"/></marker></defs><line x1="8" y1="5" x2="16" y2="5" stroke="black" marker-start="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 5, 5));
    }

    #[test]
    fn native_rasterizer_paints_polyline_marker_mid() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="16"><defs><marker id="m" markerWidth="4" markerHeight="4" refX="2" refY="2"><rect width="4" height="4" fill="black"/></marker></defs><polyline points="3,12 12,4 21,12" fill="none" stroke="black" marker-mid="url(#m)"/></svg>"##,
            24,
            16,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 12, 4));
    }

    #[test]
    fn native_rasterizer_paints_path_marker_end() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><marker id="m" markerWidth="4" markerHeight="4" refX="0" refY="2"><rect width="4" height="4" fill="black"/></marker></defs><path d="M1 5 C4 2 8 2 12 5" fill="none" stroke="black" marker-end="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 13, 5));
    }

    #[test]
    fn native_rasterizer_resolves_pattern_paint_server() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="12" height="6"><defs><pattern id="p" width="4" height="4"><rect width="2" height="4" fill="black"/></pattern></defs><rect width="12" height="6" fill="url(#p)"/></svg>"##,
            12,
            6,
        )
        .unwrap();
        assert!(painted_at(&data, 12, 0, 2));
        assert!(!painted_at(&data, 12, 3, 2));
        assert!(painted_at(&data, 12, 4, 2));
    }

    #[test]
    fn native_rasterizer_applies_basic_svg_blur_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="30" height="20"><defs><filter id="f"><feGaussianBlur stdDeviation="2"/></filter></defs><rect x="12" y="7" width="4" height="4" fill="black" filter="url(#f)"/></svg>"##,
            30,
            20,
        )
        .unwrap();
        assert!(painted_at(&data, 30, 13, 8));
        assert!(painted_at(&data, 30, 9, 8));
    }

    #[test]
    fn native_rasterizer_applies_basic_svg_drop_shadow_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="30" height="20"><defs><filter id="f"><feDropShadow dx="8" dy="0" stdDeviation="0" flood-color="black"/></filter></defs><rect x="4" y="6" width="6" height="6" fill="red" filter="url(#f)"/></svg>"##,
            30,
            20,
        )
        .unwrap();
        assert!(painted_at(&data, 30, 6, 8));
        assert!(painted_at(&data, 30, 14, 8));
    }

    #[test]
    fn native_rasterizer_evaluates_svg_flood_and_composite_filter_results() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feFlood flood-color="red" result="paint"/>
                    <feComposite in="paint" in2="SourceGraphic" operator="in"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="blue" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r > 150 && g < 80 && b < 80 && a > 150,
            "composited pixel was {r},{g},{b},{a}"
        );
        assert_eq!(alpha_at(&data, 20, 15, 5), 0);
    }

    #[test]
    fn native_rasterizer_evaluates_svg_merge_filter_nodes() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feFlood flood-color="red" result="paint"/>
                    <feMerge>
                        <feMergeNode in="paint"/>
                        <feMergeNode in="SourceGraphic"/>
                    </feMerge>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="blue" fill-opacity="0.5" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, _, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r > 40 && b > 100 && a > 120,
            "merged rect pixel was {r},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_evaluates_svg_blend_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feFlood flood-color="rgb(0, 0, 255)" result="blue"/>
                    <feBlend in="SourceGraphic" in2="blue" mode="multiply"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 0, 0)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r < 80 && g < 80 && b < 80 && a > 200,
            "multiply blend pixel was {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_evaluates_svg_color_matrix_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feColorMatrix type="matrix" values="0 0 1 0 0  0 1 0 0 0  1 0 0 0 0  0 0 0 1 0"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 0, 0)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r < 80 && g < 80 && b > 150 && a > 200,
            "color-matrix pixel was {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_evaluates_svg_component_transfer_linear() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feComponentTransfer>
                        <feFuncR type="linear" slope="0" intercept="0"/>
                        <feFuncB type="linear" slope="0" intercept="1"/>
                    </feComponentTransfer>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 0, 0)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r < 80 && g < 80 && b > 150 && a > 200,
            "component-transfer pixel was {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_evaluates_svg_component_transfer_table_alpha() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feComponentTransfer>
                        <feFuncA type="table" tableValues="0 0.5"/>
                    </feComponentTransfer>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="black" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (_, _, _, a) = rgba_at(&data, 20, 5, 5);
        assert!((90..170).contains(&a), "alpha was {a}");
    }
}
