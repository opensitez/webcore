//! Native SVG rasterization.

use super::geometry::{
    intrinsic_size_from_markup, parse_preserve_aspect_ratio, parse_view_box, AlignX, AlignY,
    PreserveAspectRatio,
};
use super::{parse_svg_document, SvgElementKind, SvgNode};
use crate::canvas::{
    Canvas, Font, FontStyle, FontWeight, Matrix, TextAlign, TextBaseline, TinySkiaCanvas,
};
use crate::css::{
    parse_color, parse_stylesheet, AttrOp, Combinator, CssRule, Declarations, SelectorPart,
};
use crate::types::Color;
use std::collections::HashMap;
use tiny_skia::{
    FillRule, GradientStop as SkGradientStop, LineCap, LineJoin, LinearGradient, Paint, Path,
    PathBuilder, Pixmap, PixmapPaint, PixmapRef, Point as SkPoint, RadialGradient, SpreadMode,
    Mask, MaskType, Stroke, StrokeDash, Transform,
};

#[derive(Clone)]
enum PaintSource {
    Color(Color),
    Url(String, Option<Color>),
}

#[derive(Clone)]
struct PaintState {
    visible: bool,
    current_color: Color,
    fill: Option<PaintSource>,
    stroke: Option<PaintSource>,
    font_family: String,
    font_size: f32,
    font_weight: FontWeight,
    font_style: FontStyle,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    stroke_width: f32,
    stroke_linecap: LineCap,
    stroke_linejoin: LineJoin,
    stroke_miterlimit: f32,
    stroke_dasharray: Option<Vec<f32>>,
    stroke_dashoffset: f32,
    fill_rule: FillRule,
    opacity: f32,
}

impl Default for PaintState {
    fn default() -> Self {
        Self {
            visible: true,
            current_color: Color::BLACK,
            fill: Some(PaintSource::Color(Color::BLACK)),
            stroke: None,
            font_family: "sans-serif".to_string(),
            font_size: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            stroke_width: 1.0,
            stroke_linecap: LineCap::Butt,
            stroke_linejoin: LineJoin::Miter,
            stroke_miterlimit: 4.0,
            stroke_dasharray: None,
            stroke_dashoffset: 0.0,
            fill_rule: FillRule::Winding,
            opacity: 1.0,
        }
    }
}

pub fn rasterize_svg_to_rgba(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    if width == 0 || height == 0 {
        return None;
    }
    let doc = parse_svg_document(svg).ok()?;
    let mut pixmap = Pixmap::new(width, height)?;
    let (iw, ih) = intrinsic_size_from_markup(svg);
    let vb = doc
        .root
        .attr_ascii_case_insensitive("viewBox")
        .and_then(|v| parse_view_box(Some(v)));
    let (origin_x, origin_y, scale_x, scale_y) = if let Some(vb) = vb {
        let viewport_w = width as f32;
        let viewport_h = height as f32;
        match parse_preserve_aspect_ratio(doc.root.attr_ascii_case_insensitive("preserveAspectRatio")) {
            PreserveAspectRatio::None => (
                -vb.min_x * viewport_w / vb.width,
                -vb.min_y * viewport_h / vb.height,
                viewport_w / vb.width,
                viewport_h / vb.height,
            ),
            PreserveAspectRatio::Meet { align_x, align_y }
            | PreserveAspectRatio::Slice { align_x, align_y } => {
                let sx = viewport_w / vb.width;
                let sy = viewport_h / vb.height;
                let scale = if matches!(
                    parse_preserve_aspect_ratio(doc.root.attr_ascii_case_insensitive("preserveAspectRatio")),
                    PreserveAspectRatio::Slice { .. }
                ) {
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
                (align_dx - vb.min_x * scale, align_dy - vb.min_y * scale, scale, scale)
            }
        }
    } else {
        let sx = if iw > 0.0 { width as f32 / iw } else { 1.0 };
        let sy = if ih > 0.0 { height as f32 / ih } else { 1.0 };
        (0.0, 0.0, sx, sy)
    };
    let transform = Transform::from_row(scale_x, 0.0, 0.0, scale_y, origin_x, origin_y);
    let mut id_map = HashMap::new();
    collect_id_nodes(&doc.root, &mut id_map);
    let styles = collect_style_rules(&doc.root);
    paint_node(
        &doc.root,
        &mut pixmap,
        PaintState::default(),
        transform,
        &id_map,
        &styles,
        &mut Vec::new(),
        &mut Vec::new(),
        None,
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
) {
    let state = state_for_node(node, inherited, styles, ancestors);
    if !state.visible {
        return;
    }
    let transform = node
        .attr_ascii_case_insensitive("transform")
        .and_then(parse_transform_list)
        .map(|local| transform.pre_concat(local))
        .unwrap_or(transform);
    let local_mask = compositing_mask_for_node(
        node,
        pixmap.width(),
        pixmap.height(),
        transform,
        ids,
        styles,
        ancestors,
        stack,
        clip,
    );
    let active_clip = local_mask.as_ref().or(clip);
    match node.kind {
        SvgElementKind::Svg
        | SvgElementKind::Group
        | SvgElementKind::Unknown(_) => {}
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
            if let Some(path) = rect_path(node) {
                paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
            }
        }
        SvgElementKind::Circle => {
            if let Some(path) = circle_path(node) {
                paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
            }
        }
        SvgElementKind::Ellipse => {
            if let Some(path) = ellipse_path(node) {
                paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
            }
        }
        SvgElementKind::Line => {
            if let Some(path) = line_path(node) {
                paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
            }
            paint_line_markers(node, pixmap, state.clone(), transform, ids, styles, ancestors, stack, active_clip);
        }
        SvgElementKind::Polyline => {
            let points = points_list(node);
            if let Some(path) = points_path_from_pairs(&points, false) {
                let mut line_state = state.clone();
                line_state.fill = None;
                paint_path(pixmap, &path, &line_state, transform, ids, styles, ancestors, stack, active_clip);
            }
            paint_poly_markers(node, &points, false, pixmap, state.clone(), transform, ids, styles, ancestors, stack, active_clip);
        }
        SvgElementKind::Polygon => {
            let points = points_list(node);
            if let Some(path) = points_path_from_pairs(&points, true) {
                paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
            }
            paint_poly_markers(node, &points, true, pixmap, state.clone(), transform, ids, styles, ancestors, stack, active_clip);
        }
        SvgElementKind::Path => {
            if let Some(d) = node.attr("d").or_else(|| node.attr("D")) {
                if let Some(path) = parse_path_data(d) {
                    paint_path(pixmap, &path, &state, transform, ids, styles, ancestors, stack, active_clip);
                }
                let (points, closed) = path_marker_points(d);
                paint_poly_markers(node, &points, closed, pixmap, state.clone(), transform, ids, styles, ancestors, stack, active_clip);
            }
        }
        SvgElementKind::Image => {
            paint_image(node, pixmap, state.opacity, transform, active_clip);
        }
        SvgElementKind::Text | SvgElementKind::Tspan => {
            paint_text(node, pixmap, &state, transform, active_clip);
            return;
        }
        _ => {}
    }
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
            active_clip,
        );
    }
    ancestors.pop();
}

fn paint_text(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    clip: Option<&Mask>,
) {
    let text = text_content(node);
    if text.is_empty() {
        return;
    }
    let x = attr_number(node, "x").unwrap_or(0.0) + attr_number(node, "dx").unwrap_or(0.0);
    let y = attr_number(node, "y").unwrap_or(0.0) + attr_number(node, "dy").unwrap_or(0.0);
    let mut font_system = cosmic_text::FontSystem::new();
    let mut swash_cache = cosmic_text::SwashCache::new();
    if let Some(clip) = clip {
        let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
            return;
        };
        paint_text_onto(&mut layer, state, transform, &text, x, y, &mut font_system, &mut swash_cache);
        let pp = PixmapPaint::default();
        pixmap.draw_pixmap(0, 0, layer.as_ref(), &pp, Transform::identity(), Some(clip));
    } else {
        paint_text_onto(pixmap, state, transform, &text, x, y, &mut font_system, &mut swash_cache);
    }
}

fn paint_text_onto(
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    text: &str,
    x: f32,
    y: f32,
    font_system: &mut cosmic_text::FontSystem,
    swash_cache: &mut cosmic_text::SwashCache,
) {
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
}

fn text_content(node: &SvgNode) -> String {
    let mut out = String::new();
    collect_text_content(node, &mut out);
    out
}

fn collect_text_content(node: &SvgNode, out: &mut String) {
    out.push_str(&node.text);
    for child in &node.children {
        if matches!(child.kind, SvgElementKind::Text | SvgElementKind::Tspan) {
            collect_text_content(child, out);
        }
    }
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
    opacity: f32,
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
    let x = attr_number(node, "x").unwrap_or(0.0);
    let y = attr_number(node, "y").unwrap_or(0.0);
    let w = attr_number(node, "width").unwrap_or(iw as f32);
    let h = attr_number(node, "height").unwrap_or(ih as f32);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let Some(src) = PixmapRef::from_bytes(&rgba, iw, ih) else {
        return;
    };
    let mut paint = PixmapPaint::default();
    paint.opacity = opacity.clamp(0.0, 1.0);
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
    let transform = transform
        .pre_translate(attr_number(node, "x").unwrap_or(0.0), attr_number(node, "y").unwrap_or(0.0));
    stack.push(id.to_string());
    match target.kind {
        SvgElementKind::Symbol => {
            for child in &target.children {
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
                );
            }
        }
        _ => paint_node(
            target, pixmap, state, transform, ids, styles, ancestors, stack, clip,
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
    let x1 = attr_number(node, "x1").unwrap_or(0.0);
    let y1 = attr_number(node, "y1").unwrap_or(0.0);
    let x2 = attr_number(node, "x2").unwrap_or(0.0);
    let y2 = attr_number(node, "y2").unwrap_or(0.0);
    let angle = (y2 - y1).atan2(x2 - x1).to_degrees();
    paint_marker_ref(node, "marker-start", x1, y1, angle, pixmap, state.clone(), transform, ids, styles, ancestors, stack, clip);
    paint_marker_ref(node, "marker-end", x2, y2, angle, pixmap, state, transform, ids, styles, ancestors, stack, clip);
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
    paint_marker_ref(node, "marker-start", points[0].0, points[0].1, first_angle, pixmap, state.clone(), transform, ids, styles, ancestors, stack, clip);
    for i in 1..points.len() - 1 {
        let in_angle = segment_angle(points[i - 1], points[i]);
        let out_angle = segment_angle(points[i], points[i + 1]);
        paint_marker_ref(node, "marker-mid", points[i].0, points[i].1, (in_angle + out_angle) / 2.0, pixmap, state.clone(), transform, ids, styles, ancestors, stack, clip);
    }
    let last = points.len() - 1;
    let end_angle = if closed {
        segment_angle(points[last], points[0])
    } else {
        segment_angle(points[last - 1], points[last])
    };
    paint_marker_ref(node, "marker-end", points[last].0, points[last].1, end_angle, pixmap, state, transform, ids, styles, ancestors, stack, clip);
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
    let ref_x = attr_number(marker, "refX").unwrap_or(0.0);
    let ref_y = attr_number(marker, "refY").unwrap_or(0.0);
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
    if let Some(vb) = marker.attr_ascii_case_insensitive("viewBox").and_then(|v| parse_view_box(Some(v))) {
        let marker_w = attr_number(marker, "markerWidth").unwrap_or(vb.width);
        let marker_h = attr_number(marker, "markerHeight").unwrap_or(vb.height);
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
        );
    }
    stack.pop();
}

fn compositing_mask_for_node<'a>(
    node: &'a SvgNode,
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let clip = clip_mask_for_node(node, width, height, transform, ids, parent);
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
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &SvgNode>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let id = node
        .attr("clip-path")
        .and_then(parse_url_id)?;
    let clip_node = ids.get(id).copied()?;
    if !matches!(clip_node.kind, SvgElementKind::ClipPath) {
        return None;
    }
    let mut mask = Mask::new(width, height)?;
    for child in &clip_node.children {
        if let Some(path) = node_path(child) {
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

fn node_path(node: &SvgNode) -> Option<Path> {
    match node.kind {
        SvgElementKind::Rect => rect_path(node),
        SvgElementKind::Circle => circle_path(node),
        SvgElementKind::Ellipse => ellipse_path(node),
        SvgElementKind::Line => line_path(node),
        SvgElementKind::Polyline => points_path_from_pairs(&points_list(node), false),
        SvgElementKind::Polygon => points_path_from_pairs(&points_list(node), true),
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
        apply_paint_attr(state, name, value);
    }
}

fn rule_matches_svg_node(rule: &CssRule, node: &SvgNode, ancestors: &[&SvgNode]) -> bool {
    rule.selectors
        .iter()
        .filter(|selector| selector.valid)
        .any(|selector| selector_matches_svg_parts(&selector.parts, node, ancestors))
}

fn selector_matches_svg_parts(parts: &[SelectorPart], node: &SvgNode, ancestors: &[&SvgNode]) -> bool {
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
    if groups.is_empty() || !simple_selector_group_matches(groups[groups.len() - 1], node) {
        return false;
    }

    let mut ancestor_limit = ancestors.len();
    for idx in (0..groups.len().saturating_sub(1)).rev() {
        let combinator = combinators.get(idx).copied().unwrap_or(&Combinator::Descendant);
        match combinator {
            Combinator::Child => {
                if ancestor_limit == 0
                    || !simple_selector_group_matches(groups[idx], ancestors[ancestor_limit - 1])
                {
                    return false;
                }
                ancestor_limit -= 1;
            }
            Combinator::Descendant => {
                let Some(found) = ancestors[..ancestor_limit]
                    .iter()
                    .rposition(|ancestor| simple_selector_group_matches(groups[idx], ancestor))
                else {
                    return false;
                };
                ancestor_limit = found;
            }
            _ => return false,
        }
    }
    true
}

fn simple_selector_group_matches(parts: &[SelectorPart], node: &SvgNode) -> bool {
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
            _ => return false,
        }
    }
    true
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
        AttrOp::DashMatch => actual == expected || actual.strip_prefix(expected).is_some_and(|rest| rest.starts_with('-')),
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
            state.fill = parse_svg_paint(value, state.current_color);
        }
        "stroke" => state.stroke = parse_svg_paint(value, state.current_color),
        "color" => {
            if let Some(color) = parse_color(value) {
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
        "stroke-width" => {
            if let Some(v) = number(value) {
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
                if !values.is_empty() && values.iter().all(|v| *v >= 0.0) && values.iter().any(|v| *v > 0.0) {
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
            if let Some(v) = number(value) {
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
        _ => {}
    }
}

fn parse_svg_paint(value: &str, current_color: Color) -> Option<PaintSource> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        None
    } else if value.eq_ignore_ascii_case("currentColor") {
        Some(PaintSource::Color(current_color))
    } else if let Some(id) = parse_url_id(value) {
        Some(PaintSource::Url(id.to_string(), None))
    } else {
        parse_color(value).map(PaintSource::Color)
    }
}

fn parse_url_id(value: &str) -> Option<&str> {
    let value = value.trim();
    let inside = value.strip_prefix("url(")?.strip_suffix(')')?.trim();
    let inside = inside.trim_matches(|c| c == '"' || c == '\'');
    inside.strip_prefix('#')
}

fn with_source_alpha(source: PaintSource, alpha: f32) -> PaintSource {
    match source {
        PaintSource::Color(color) => PaintSource::Color(with_alpha(color, alpha)),
        PaintSource::Url(id, fallback) => PaintSource::Url(id, fallback.map(|color| with_alpha(color, alpha))),
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
    if let Some(source) = &state.fill {
        with_paint_source(source, state, path, ids, styles, ancestors, stack, |paint| {
            pixmap.fill_path(path, paint, state.fill_rule, transform, clip);
        });
    }
    if let Some(source) = &state.stroke {
        if state.stroke_width > 0.0 {
            let mut stroke = Stroke::default();
            stroke.width = state.stroke_width;
            stroke.line_cap = state.stroke_linecap;
            stroke.line_join = state.stroke_linejoin;
            stroke.miter_limit = state.stroke_miterlimit;
            stroke.dash = state
                .stroke_dasharray
                .clone()
                .and_then(|dash| StrokeDash::new(dash, state.stroke_dashoffset));
            with_paint_source(source, state, path, ids, styles, ancestors, stack, |paint| {
                pixmap.stroke_path(path, paint, &stroke, transform, clip);
            });
        }
    }
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
                if let Some(tile) = render_pattern_tile(pattern, state, ids, styles, ancestors, stack) {
                    let Some(src) = PixmapRef::from_bytes(tile.data(), tile.width(), tile.height()) else {
                        paint.set_color(apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity).to_tiny_skia());
                        draw(&paint);
                        return;
                    };
                    paint.shader = tiny_skia::Pattern::new(
                        src,
                        SpreadMode::Repeat,
                        tiny_skia::FilterQuality::Nearest,
                        state.opacity.clamp(0.0, 1.0),
                        pattern_transform(pattern),
                    );
                    draw(&paint);
                } else {
                    paint.set_color(apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity).to_tiny_skia());
                    draw(&paint);
                }
            } else if let Some(shader) = ids.get(id).and_then(|node| gradient_shader(node, state.opacity, path, ids)) {
                paint.shader = shader;
                draw(&paint);
            } else {
                paint.set_color(apply_opacity(fallback.unwrap_or(Color::BLACK), state.opacity).to_tiny_skia());
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
    let width = attr_number(pattern, "width").unwrap_or(0.0).ceil().max(1.0) as u32;
    let height = attr_number(pattern, "height").unwrap_or(0.0).ceil().max(1.0) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let mut tile = Pixmap::new(width, height)?;
    let mut transform = Transform::identity();
    if let Some(vb) = pattern.attr_ascii_case_insensitive("viewBox").and_then(|v| parse_view_box(Some(v))) {
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
        );
    }
    Some(tile)
}

fn pattern_transform(pattern: &SvgNode) -> Transform {
    let local = pattern
        .attr("patternTransform")
        .and_then(parse_transform_list)
        .unwrap_or_else(Transform::identity);
    local.pre_translate(
        -attr_number(pattern, "x").unwrap_or(0.0),
        -attr_number(pattern, "y").unwrap_or(0.0),
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
    let user_space = inherited_gradient_attr(node, "gradientUnits", ids)
        .is_some_and(|v| v == "userSpaceOnUse");
    let x1 = gradient_coord(node, ids, "x1", if user_space { 0.0 } else { bounds.left() }, bounds.width(), user_space, 0.0);
    let y1 = gradient_coord(node, ids, "y1", if user_space { 0.0 } else { bounds.top() }, bounds.height(), user_space, 0.0);
    let x2 = gradient_coord(node, ids, "x2", if user_space { 0.0 } else { bounds.left() }, bounds.width(), user_space, if user_space { 0.0 } else { 1.0 });
    let y2 = gradient_coord(node, ids, "y2", if user_space { 0.0 } else { bounds.top() }, bounds.height(), user_space, 0.0);
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
    let user_space = inherited_gradient_attr(node, "gradientUnits", ids)
        .is_some_and(|v| v == "userSpaceOnUse");
    let cx = gradient_coord(node, ids, "cx", if user_space { 0.0 } else { bounds.left() }, bounds.width(), user_space, 0.5);
    let cy = gradient_coord(node, ids, "cy", if user_space { 0.0 } else { bounds.top() }, bounds.height(), user_space, 0.5);
    let fx = gradient_coord(node, ids, "fx", if user_space { 0.0 } else { bounds.left() }, bounds.width(), user_space, if user_space { cx } else { 0.5 });
    let fy = gradient_coord(node, ids, "fy", if user_space { 0.0 } else { bounds.top() }, bounds.height(), user_space, if user_space { cy } else { 0.5 });
    let r = gradient_length(node, ids, "r", bounds.width().max(bounds.height()), user_space, if user_space { 0.0 } else { 0.5 });
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
            stops.push(SkGradientStop::new(offset, apply_opacity(color, opacity).to_tiny_skia()));
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

fn rect_path(node: &SvgNode) -> Option<Path> {
    let x = attr_number(node, "x").unwrap_or(0.0);
    let y = attr_number(node, "y").unwrap_or(0.0);
    let w = attr_number(node, "width")?;
    let h = attr_number(node, "height")?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let mut b = PathBuilder::new();
    let mut rx = attr_number(node, "rx");
    let mut ry = attr_number(node, "ry");
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
    b.cubic_to(right - rx + rx * K, y, right, y + ry - ry * K, right, y + ry);
    b.line_to(right, bottom - ry);
    b.cubic_to(right, bottom - ry + ry * K, right - rx + rx * K, bottom, right - rx, bottom);
    b.line_to(x + rx, bottom);
    b.cubic_to(x + rx - rx * K, bottom, x, bottom - ry + ry * K, x, bottom - ry);
    b.line_to(x, y + ry);
    b.cubic_to(x, y + ry - ry * K, x + rx - rx * K, y, x + rx, y);
    b.close();
}

fn circle_path(node: &SvgNode) -> Option<Path> {
    let cx = attr_number(node, "cx").unwrap_or(0.0);
    let cy = attr_number(node, "cy").unwrap_or(0.0);
    let r = attr_number(node, "r")?;
    let mut b = PathBuilder::new();
    b.push_circle(cx, cy, r);
    b.finish()
}

fn ellipse_path(node: &SvgNode) -> Option<Path> {
    let cx = attr_number(node, "cx").unwrap_or(0.0);
    let cy = attr_number(node, "cy").unwrap_or(0.0);
    let rx = attr_number(node, "rx")?;
    let ry = attr_number(node, "ry")?;
    let mut b = PathBuilder::new();
    b.push_oval(tiny_skia::Rect::from_xywh(cx - rx, cy - ry, rx * 2.0, ry * 2.0)?);
    b.finish()
}

fn line_path(node: &SvgNode) -> Option<Path> {
    let mut b = PathBuilder::new();
    b.move_to(attr_number(node, "x1").unwrap_or(0.0), attr_number(node, "y1").unwrap_or(0.0));
    b.line_to(attr_number(node, "x2").unwrap_or(0.0), attr_number(node, "y2").unwrap_or(0.0));
    b.finish()
}

fn points_list(node: &SvgNode) -> Vec<(f32, f32)> {
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
                let Some((x, y)) = p.pair(relative) else { break };
                p.x = x;
                p.y = y;
                p.sx = x;
                p.sy = y;
                points.push((x, y));
                p.cmd = if relative { 'l' } else { 'L' };
                p.clear_controls();
            }
            'L' => {
                let Some((x, y)) = p.pair(relative) else { break };
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
                let Some((x, y)) = p.pair(relative) else { break };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'S' | 'Q' => {
                if p.pair(relative).is_none() {
                    break;
                }
                let Some((x, y)) = p.pair(relative) else { break };
                p.x = x;
                p.y = y;
                points.push((x, y));
                p.clear_controls();
            }
            'T' => {
                let Some((x, y)) = p.pair(relative) else { break };
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
                let Some((x, y)) = p.pair(relative) else { break };
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

fn attr_number(node: &SvgNode, name: &str) -> Option<f32> {
    node.attr_ascii_case_insensitive(name).and_then(number)
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
            "matrix" if args.len() >= 6 => Transform::from_row(args[0], args[1], args[2], args[3], args[4], args[5]),
            "translate" if !args.is_empty() => Transform::from_translate(args[0], args.get(1).copied().unwrap_or(0.0)),
            "scale" if !args.is_empty() => Transform::from_scale(args[0], args.get(1).copied().unwrap_or(args[0])),
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
    if rx == 0.0 || ry == 0.0 || ((x1 - x2).abs() < f32::EPSILON && (y1 - y2).abs() < f32::EPSILON) {
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
    let coef = sign * ((rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2) / denom).max(0.0).sqrt();
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

    let segments = (delta.abs() / (std::f32::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
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
    if ux * vy - uy * vx < 0.0 { -angle } else { angle }
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
}
