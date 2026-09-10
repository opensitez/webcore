//! Native SVG rasterization.

use super::geometry::{
    intrinsic_size_from_markup, parse_preserve_aspect_ratio, parse_svg_length, parse_view_box,
    AlignX, AlignY, PreserveAspectRatio, SvgLength, SvgViewBox,
};
use super::path::{
    flatten_path_points, number, number_list, parse_path_data, parse_transform_list,
    path_marker_subpaths, path_polyline_length, point_at_path_distance,
};
use super::{parse_svg_document, SvgDocument, SvgElementKind, SvgNode};
use crate::canvas::{
    Canvas, Font, FontStyle, FontWeight, Matrix, TextAlign, TextBaseline, TinySkiaCanvas,
};
use crate::css::{
    parse_color, parse_stylesheet, resolve_var_references, AttrOp, Combinator, CssRule,
    CssSelector, Declarations, SelectorPart,
};
use crate::svg::animation::{WEBCORE_ANIMATED_ATTR_NS, WEBCORE_ANIMATED_ATTR_PREFIX};
use crate::svg::condition;
use crate::types::{
    Color, Direction, Overflow, WebCore, SPECIFIED_SVG_FILL, SPECIFIED_SVG_STROKE,
};
use std::borrow::Cow;
use std::collections::HashMap;
use tiny_skia::{
    FillRule, GradientStop as SkGradientStop, LineCap, LineJoin, LinearGradient, Mask, MaskType,
    Paint, Path, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Point as SkPoint,
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
    direction: Direction,
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
    non_scaling_stroke: bool,
    fill_rule: FillRule,
    clip_rule: FillRule,
    mask_type: MaskType,
    stop_color: Color,
    stop_opacity: f32,
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
            direction: Direction::LTR,
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
            non_scaling_stroke: false,
            fill_rule: FillRule::Winding,
            clip_rule: FillRule::Winding,
            mask_type: MaskType::Luminance,
            stop_color: Color::BLACK,
            stop_opacity: 1.0,
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
    rasterize_svg_document_to_rgba_with_dom(
        doc,
        width,
        height,
        intrinsic_size,
        current_color,
        fill,
        stroke,
        custom_props,
        None,
    )
}

pub(crate) fn rasterize_svg_document_to_rgba_with_dom(
    doc: &SvgDocument,
    width: u32,
    height: u32,
    intrinsic_size: (f32, f32),
    current_color: Color,
    fill: Option<Color>,
    stroke: Option<Color>,
    custom_props: &HashMap<String, String>,
    dom_root: Option<&WebCore>,
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
        dom_root,
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
    dom_node: Option<&WebCore>,
) {
    let mut state = state_for_node(node, inherited, styles, ancestors, dom_node);
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
                        dom_node,
                    );
                    stack.pop();
                    apply_svg_filter(&mut layer, filter_node, node, &state, transform);
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
    if matches!(node.kind, SvgElementKind::Switch) {
        if state.opacity < 0.999 {
            let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
                return;
            };
            let mut child_state = state.clone();
            child_state.opacity = 1.0;
            paint_switch_child(
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
                dom_node,
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
        } else {
            paint_switch_child(
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
                dom_node,
            );
        }
        return;
    }
    if matches!(
        node.kind,
        SvgElementKind::Svg
            | SvgElementKind::Group
            | SvgElementKind::Anchor
            | SvgElementKind::Stop
            | SvgElementKind::Unknown(_)
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
            dom_node,
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
        SvgElementKind::Svg
        | SvgElementKind::Group
        | SvgElementKind::Anchor
        | SvgElementKind::Stop
        | SvgElementKind::Unknown(_) => {}
        SvgElementKind::Defs
        | SvgElementKind::Symbol
        | SvgElementKind::View
        | SvgElementKind::Cursor
        | SvgElementKind::Title
        | SvgElementKind::Desc
        | SvgElementKind::Metadata
        | SvgElementKind::Script
        | SvgElementKind::Animate
        | SvgElementKind::AnimateColor
        | SvgElementKind::AnimateTransform
        | SvgElementKind::AnimateMotion
        | SvgElementKind::MPath
        | SvgElementKind::Set
        | SvgElementKind::Filter
        | SvgElementKind::FeGaussianBlur
        | SvgElementKind::FeOffset
        | SvgElementKind::FeDropShadow
        | SvgElementKind::FeFlood
        | SvgElementKind::FeComposite
        | SvgElementKind::FeBlend
        | SvgElementKind::FeColorMatrix
        | SvgElementKind::FeComponentTransfer
        | SvgElementKind::FeFuncR
        | SvgElementKind::FeFuncG
        | SvgElementKind::FeFuncB
        | SvgElementKind::FeFuncA
        | SvgElementKind::FeMorphology
        | SvgElementKind::FeMerge
        | SvgElementKind::FeMergeNode
        | SvgElementKind::FeImage
        | SvgElementKind::FeTile
        | SvgElementKind::FeConvolveMatrix
        | SvgElementKind::FeDisplacementMap
        | SvgElementKind::Style => return,
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
                for subpath in path_marker_subpaths(d) {
                    paint_poly_markers(
                        node,
                        &subpath.points,
                        subpath.closed,
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
        }
        SvgElementKind::ForeignObject => {
            paint_foreign_object(node, pixmap, &state, transform, active_clip);
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
        dom_node,
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
    dom_node: Option<&WebCore>,
) {
    ancestors.push(node);
    for (index, child) in node.children.iter().enumerate() {
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
            dom_node.and_then(|dom| svg_dom_child(dom, index)),
        );
    }
    ancestors.pop();
}

fn paint_switch_child<'a>(
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
    dom_node: Option<&WebCore>,
) {
    ancestors.push(node);
    if let Some((index, child)) = node
        .children
        .iter()
        .enumerate()
        .find(|(_, child)| condition::switch_accepts(child))
    {
        paint_node(
            child,
            pixmap,
            state,
            transform,
            ids,
            styles,
            ancestors,
            stack,
            clip,
            allow_filter,
            dom_node.and_then(|dom| svg_dom_child(dom, index)),
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
        let child_state = state_for_node(child, state.clone(), styles, ancestors, None);
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
    let text = svg_directional_text(text, state.direction);
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
    let text = svg_directional_text(&text, state.direction);
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
    let side_right = node
        .attr_ascii_case_insensitive("side")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("right"));
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
    let start_offset = text_path_start_offset(node, state, total_len);
    let method_stretch = node
        .attr_ascii_case_insensitive("method")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("stretch"));
    let target_length = attr_length(node, "textLength", LengthAxis::X, state)
        .or_else(|| method_stretch.then_some((total_len - start_offset).max(0.0)));
    let length_adjust = node
        .attr_ascii_case_insensitive("lengthAdjust")
        .unwrap_or("spacing")
        .trim();
    let extra_letter_spacing = if length_adjust.eq_ignore_ascii_case("spacing")
        || length_adjust.eq_ignore_ascii_case("spacingAndGlyphs")
    {
        text_length_extra_spacing(&text, natural_advance, target_length)
    } else {
        0.0
    };
    let total_letter_spacing = state.letter_spacing + extra_letter_spacing;
    let mut distance = start_offset;
    distance += match state.text_align {
        TextAlign::Center => -natural_advance / 2.0,
        TextAlign::End | TextAlign::Right => -natural_advance,
        TextAlign::Start | TextAlign::Left => 0.0,
    };

    for ch in text.chars() {
        let s = ch.to_string();
        let char_advance = canvas.measure_text(&s).width;
        let mid = distance + char_advance / 2.0;
        if let Some((mut x, mut y, angle)) = point_at_path_distance(&samples, mid) {
            if side_right {
                let offset = state.font_size.max(1.0);
                x += -angle.sin() * offset;
                y += angle.cos() * offset;
            }
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
        distance += char_advance + total_letter_spacing;
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

fn svg_directional_text(text: &str, direction: Direction) -> Cow<'_, str> {
    if text.is_empty() || matches!(direction, Direction::LTR) {
        return Cow::Borrowed(text);
    }
    let info = unicode_bidi::BidiInfo::new(text, Some(unicode_bidi::Level::rtl()));
    let Some(para) = info.paragraphs.first() else {
        return Cow::Borrowed(text);
    };
    Cow::Owned(info.reorder_line(para, para.range.clone()).into_owned())
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
    let text = svg_directional_text(text, state.direction);
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
    let advance = canvas.measure_text(&text).width;
    let adjusted_advance = text_length_adjusted_advance(&text, advance, target_length);
    let extra_letter_spacing = if length_adjust.eq_ignore_ascii_case("spacing")
        || length_adjust.eq_ignore_ascii_case("spacingAndGlyphs")
    {
        text_length_extra_spacing(&text, advance, target_length)
    } else {
        0.0
    };
    let total_letter_spacing = state.letter_spacing + extra_letter_spacing;
    let y = y + state.baseline_shift;
    if total_letter_spacing != 0.0 || state.word_spacing != 0.0 {
        paint_spaced_text_onto(
            &mut canvas,
            state,
            &text,
            x,
            y,
            adjusted_advance,
            total_letter_spacing,
        );
        return adjusted_advance;
    }
    if let Some(color) = state.fill.as_ref().and_then(flat_paint_color) {
        canvas.set_fill_color(to_canvas_color(color));
        canvas.fill_text(&text, x, y);
    }
    if let Some(color) = state.stroke.as_ref().and_then(flat_paint_color) {
        if state.stroke_width > 0.0 {
            canvas.set_stroke_color(to_canvas_color(color));
            canvas.set_line_width(state.stroke_width);
            canvas.stroke_text(&text, x, y);
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

fn paint_foreign_object(
    node: &SvgNode,
    pixmap: &mut Pixmap,
    state: &PaintState,
    transform: Transform,
    clip: Option<&Mask>,
) {
    let x = attr_length(node, "x", LengthAxis::X, state).unwrap_or(0.0);
    let y = attr_length(node, "y", LengthAxis::Y, state).unwrap_or(0.0);
    let w = attr_length(node, "width", LengthAxis::X, state).unwrap_or(0.0);
    let h = attr_length(node, "height", LengthAxis::Y, state).unwrap_or(0.0);
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let raster_w = w.ceil().max(1.0) as u32;
    let raster_h = h.ceil().max(1.0) as u32;
    let Some(mut layer) = Pixmap::new(raster_w, raster_h) else {
        return;
    };
    let fragment = foreign_object_html_fragment(node);
    let html = format!(
        r#"<!doctype html><html><head><style>html,body{{margin:0;padding:0;background:transparent;overflow:hidden;}}</style></head><body>{fragment}</body></html>"#
    );
    let mut doc = crate::load_html_vp(&html, w, h);
    doc.scroll_x = 0.0;
    doc.scroll_y = 0.0;
    let mut renderer = crate::Renderer::new();
    renderer.render(&mut doc, &mut layer, 1.0);
    let mut paint = PixmapPaint::default();
    paint.opacity = state.opacity.clamp(0.0, 1.0);
    pixmap.draw_pixmap(
        0,
        0,
        layer.as_ref(),
        &paint,
        transform.pre_translate(x, y),
        clip,
    );
}

fn foreign_object_html_fragment(node: &SvgNode) -> String {
    let mut out = String::new();
    out.push_str(&escape_html_text(&node.text));
    for child in &node.children {
        serialize_svg_subtree_as_markup(child, &mut out);
    }
    out
}

fn serialize_svg_subtree_as_markup(node: &SvgNode, out: &mut String) {
    let tag = svg_tag_name(node);
    out.push('<');
    out.push_str(tag);
    for attr in &node.attributes {
        out.push(' ');
        if let Some(namespace) = attr.namespace.as_deref().filter(|ns| !ns.is_empty()) {
            out.push_str(namespace);
            out.push(':');
        }
        out.push_str(&attr.name);
        out.push_str("=\"");
        out.push_str(&escape_html_attr(&attr.value));
        out.push('"');
    }
    out.push('>');
    out.push_str(&escape_html_text(&node.text));
    for child in &node.children {
        serialize_svg_subtree_as_markup(child, out);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn escape_html_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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
    let image_transform = transform.pre_translate(x, y).pre_concat(view_box_transform(
        SvgViewBox {
            min_x: 0.0,
            min_y: 0.0,
            width: iw as f32,
            height: ih as f32,
        },
        w,
        h,
        parse_preserve_aspect_ratio(node.attr_ascii_case_insensitive("preserveAspectRatio")),
    ));
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
                    None,
                );
            }
        }
        _ => paint_node(
            target, pixmap, state, transform, ids, styles, ancestors, stack, clip, true, None,
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
            average_marker_angle(in_angle, out_angle),
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

fn average_marker_angle(a: f32, b: f32) -> f32 {
    let ar = a.to_radians();
    let br = b.to_radians();
    let x = ar.cos() + br.cos();
    let y = ar.sin() + br.sin();
    if x.abs() < f32::EPSILON && y.abs() < f32::EPSILON {
        b
    } else {
        y.atan2(x).to_degrees()
    }
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
    let marker_w = attr_length(marker, "markerWidth", LengthAxis::X, &state).unwrap_or(3.0);
    let marker_h = attr_length(marker, "markerHeight", LengthAxis::Y, &state).unwrap_or(3.0);
    let ref_x = marker_ref_length(marker, "refX", LengthAxis::X, marker_w, &state).unwrap_or(0.0);
    let ref_y = marker_ref_length(marker, "refY", LengthAxis::Y, marker_h, &state).unwrap_or(0.0);
    let marker_angle = match marker.attr("orient").unwrap_or("0").trim() {
        "auto" => angle,
        "auto-start-reverse" if attr == "marker-start" => angle + 180.0,
        "auto-start-reverse" => angle,
        other => number(other).unwrap_or(angle),
    };
    let mut marker_transform = transform
        .pre_translate(x, y)
        .pre_rotate(marker_angle)
        .pre_translate(-ref_x, -ref_y);
    if marker.attr("markerUnits").unwrap_or("strokeWidth") == "strokeWidth" {
        marker_transform = marker_transform.pre_scale(state.stroke_width, state.stroke_width);
    }
    if let Some(vb) = marker
        .attr_ascii_case_insensitive("viewBox")
        .and_then(|v| parse_view_box(Some(v)))
    {
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
            None,
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
    let clip = clip_mask_for_node(
        node, state, width, height, transform, ids, styles, ancestors, parent,
    );
    let mask = svg_mask_for_node(
        node,
        state,
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

fn clip_mask_for_node<'a>(
    node: &SvgNode,
    state: &PaintState,
    width: u32,
    height: u32,
    transform: Transform,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    parent: Option<&Mask>,
) -> Option<Mask> {
    let id = node.attr("clip-path").and_then(parse_url_id)?;
    let clip_node = ids.get(id).copied()?;
    if !matches!(clip_node.kind, SvgElementKind::ClipPath) {
        return None;
    }
    let mut mask = Mask::new(width, height)?;
    let clip_transform = if clip_node
        .attr("clipPathUnits")
        .is_some_and(|value| value == "objectBoundingBox")
    {
        let target_path = node_path(node, state)?;
        let bounds = target_path.bounds();
        transform
            .pre_translate(bounds.left(), bounds.top())
            .pre_scale(bounds.width(), bounds.height())
    } else {
        transform
    };
    ancestors.push(clip_node);
    for child in &clip_node.children {
        let child_state = state_for_node(child, state.clone(), styles, ancestors, None);
        if let Some(path) = node_path(child, &child_state) {
            mask.fill_path(&path, child_state.clip_rule, true, clip_transform);
        }
    }
    ancestors.pop();
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
    let id = node.attr("mask").and_then(parse_url_id)?;
    if stack.iter().any(|seen| seen == id) {
        return None;
    }
    let mask_node = ids.get(id).copied()?;
    if !matches!(mask_node.kind, SvgElementKind::Mask) {
        return None;
    }
    let mut mask_pixmap = Pixmap::new(width, height)?;
    ancestors.push(mask_node);
    let mut mask_state = state_for_node(mask_node, PaintState::default(), styles, ancestors, None);
    let mask_transform = if mask_node
        .attr("maskContentUnits")
        .is_some_and(|value| value == "objectBoundingBox")
    {
        let target_path = node_path(node, state)?;
        let bounds = target_path.bounds();
        mask_state.viewport_width = 1.0;
        mask_state.viewport_height = 1.0;
        transform
            .pre_translate(bounds.left(), bounds.top())
            .pre_scale(bounds.width(), bounds.height())
    } else {
        transform
    };
    stack.push(id.to_string());
    for child in &mask_node.children {
        paint_node(
            child,
            &mut mask_pixmap,
            mask_state.clone(),
            mask_transform,
            ids,
            styles,
            ancestors,
            stack,
            None,
            true,
            None,
        );
    }
    stack.pop();
    ancestors.pop();
    let mut mask = Mask::from_pixmap(mask_pixmap.as_ref(), mask_state.mask_type);
    if let Some(region) = mask_region_for_node(mask_node, node, state, width, height, transform) {
        for (value, region_value) in mask.data_mut().iter_mut().zip(region.data()) {
            *value = (*value).min(*region_value);
        }
    }
    if let Some(parent) = parent {
        for (value, parent_value) in mask.data_mut().iter_mut().zip(parent.data()) {
            *value = (*value).min(*parent_value);
        }
    }
    Some(mask)
}

fn mask_region_for_node(
    mask_node: &SvgNode,
    target_node: &SvgNode,
    target_state: &PaintState,
    width: u32,
    height: u32,
    transform: Transform,
) -> Option<Mask> {
    let uses_object_bbox = !mask_node
        .attr("maskUnits")
        .is_some_and(|value| value == "userSpaceOnUse");
    let (x, y, w, h, region_transform) = if uses_object_bbox {
        let target_path = node_path(target_node, target_state)?;
        let bounds = target_path.bounds();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return None;
        }
        let mut state = target_state.clone();
        state.viewport_width = 1.0;
        state.viewport_height = 1.0;
        (
            attr_length(mask_node, "x", LengthAxis::X, &state).unwrap_or(-0.1),
            attr_length(mask_node, "y", LengthAxis::Y, &state).unwrap_or(-0.1),
            attr_length(mask_node, "width", LengthAxis::X, &state).unwrap_or(1.2),
            attr_length(mask_node, "height", LengthAxis::Y, &state).unwrap_or(1.2),
            transform
                .pre_translate(bounds.left(), bounds.top())
                .pre_scale(bounds.width(), bounds.height()),
        )
    } else {
        (
            attr_length(mask_node, "x", LengthAxis::X, target_state)
                .unwrap_or(-0.1 * target_state.viewport_width.max(1.0)),
            attr_length(mask_node, "y", LengthAxis::Y, target_state)
                .unwrap_or(-0.1 * target_state.viewport_height.max(1.0)),
            attr_length(mask_node, "width", LengthAxis::X, target_state)
                .unwrap_or(1.2 * target_state.viewport_width.max(1.0)),
            attr_length(mask_node, "height", LengthAxis::Y, target_state)
                .unwrap_or(1.2 * target_state.viewport_height.max(1.0)),
            transform,
        )
    };
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rect = tiny_skia::Rect::from_xywh(x, y, w, h)?;
    let mut path = PathBuilder::new();
    path.push_rect(rect);
    let path = path.finish()?;
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(&path, FillRule::Winding, true, region_transform);
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
    dom_node: Option<&WebCore>,
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
    if let Some(dom) = dom_node {
        apply_dom_computed_style(&mut state, node, dom, ancestors.is_empty());
    }
    apply_animated_paint_attrs(&mut state, node);
    state
}

fn apply_dom_computed_style(state: &mut PaintState, _svg: &SvgNode, node: &WebCore, is_root: bool) {
    let style = node.style.as_ref();
    let font_px = style.font_size_px(state.font_size, 16.0);
    let specified_svg_paint = style.rare().specified_svg_paint_props;
    state.visible = state.visible && style.visibility;
    state.current_color = style.color;
    if specified_svg_paint & SPECIFIED_SVG_FILL != 0 {
        if let Some(fill) = style.svg_fill {
            state.fill = Some(PaintSource::Color(fill));
        } else {
            state.fill = None;
        }
    } else if is_root && style.svg_fill.is_none() {
        state.fill = None;
    }
    if specified_svg_paint & SPECIFIED_SVG_STROKE != 0 {
        if let Some(stroke) = style.svg_stroke {
            state.stroke = Some(PaintSource::Color(stroke));
        } else {
            state.stroke = None;
        }
    } else if is_root && style.svg_stroke.is_none() {
        state.stroke = None;
    }
    state.custom_props = style.custom_props.clone();
    state.font_family = style.font_family.clone();
    state.font_size = font_px;
    state.font_weight = if style.font_weight.is_bold() {
        FontWeight::Bold
    } else {
        FontWeight::Normal
    };
    state.font_style = match style.font_style {
        crate::types::FontStyle::Italic | crate::types::FontStyle::Oblique => FontStyle::Italic,
        crate::types::FontStyle::Normal => FontStyle::Normal,
    };
    state.letter_spacing = style.letter_spacing.resolve(font_px, font_px, 16.0);
    state.word_spacing = style.word_spacing.resolve(font_px, font_px, 16.0);
    state.text_align = match style.text_align {
        crate::types::TextAlign::Left => TextAlign::Left,
        crate::types::TextAlign::Right => TextAlign::Right,
        crate::types::TextAlign::Center => TextAlign::Center,
        crate::types::TextAlign::End => TextAlign::End,
        crate::types::TextAlign::Justify | crate::types::TextAlign::Start => TextAlign::Start,
    };
    state.direction = style.direction;
    state.opacity *= style.opacity.clamp(0.0, 1.0);
    state.overflow_visible = matches!(style.overflow_x, Overflow::Visible)
        && matches!(style.overflow_y, Overflow::Visible);
}

fn apply_animated_paint_attrs(state: &mut PaintState, node: &SvgNode) {
    for marker in node.attributes.iter().filter(|attr| {
        attr.namespace.as_deref() == Some(WEBCORE_ANIMATED_ATTR_NS)
            && attr.name.starts_with(WEBCORE_ANIMATED_ATTR_PREFIX)
    }) {
        let name = &marker.name[WEBCORE_ANIMATED_ATTR_PREFIX.len()..];
        if let Some(value) = node.attr(name).map(str::to_string) {
            apply_paint_attr(state, name, &value);
        }
    }
}

fn svg_dom_child(parent: &WebCore, svg_child_index: usize) -> Option<&WebCore> {
    let parent_path = parent.svg_tree_path.as_ref()?;
    parent.children.iter().find(|child| {
        child.svg_tree_path.as_ref().is_some_and(|path| {
            path.len() == parent_path.len() + 1
                && path.starts_with(parent_path)
                && path.last() == Some(&svg_child_index)
        })
    })
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
        SvgElementKind::Title => "title",
        SvgElementKind::Desc => "desc",
        SvgElementKind::Metadata => "metadata",
        SvgElementKind::Anchor => "a",
        SvgElementKind::ForeignObject => "foreignObject",
        SvgElementKind::Image => "image",
        SvgElementKind::LinearGradient => "linearGradient",
        SvgElementKind::RadialGradient => "radialGradient",
        SvgElementKind::ClipPath => "clipPath",
        SvgElementKind::Mask => "mask",
        SvgElementKind::Filter => "filter",
        SvgElementKind::FeGaussianBlur => "feGaussianBlur",
        SvgElementKind::FeOffset => "feOffset",
        SvgElementKind::FeDropShadow => "feDropShadow",
        SvgElementKind::FeFlood => "feFlood",
        SvgElementKind::FeComposite => "feComposite",
        SvgElementKind::FeBlend => "feBlend",
        SvgElementKind::FeColorMatrix => "feColorMatrix",
        SvgElementKind::FeComponentTransfer => "feComponentTransfer",
        SvgElementKind::FeFuncR => "feFuncR",
        SvgElementKind::FeFuncG => "feFuncG",
        SvgElementKind::FeFuncB => "feFuncB",
        SvgElementKind::FeFuncA => "feFuncA",
        SvgElementKind::FeMorphology => "feMorphology",
        SvgElementKind::FeMerge => "feMerge",
        SvgElementKind::FeMergeNode => "feMergeNode",
        SvgElementKind::FeImage => "feImage",
        SvgElementKind::FeTile => "feTile",
        SvgElementKind::FeConvolveMatrix => "feConvolveMatrix",
        SvgElementKind::FeDisplacementMap => "feDisplacementMap",
        SvgElementKind::Pattern => "pattern",
        SvgElementKind::Marker => "marker",
        SvgElementKind::Stop => "stop",
        SvgElementKind::Switch => "switch",
        SvgElementKind::View => "view",
        SvgElementKind::Cursor => "cursor",
        SvgElementKind::Style => "style",
        SvgElementKind::Script => "script",
        SvgElementKind::Animate => "animate",
        SvgElementKind::AnimateColor => "animateColor",
        SvgElementKind::AnimateTransform => "animateTransform",
        SvgElementKind::AnimateMotion => "animateMotion",
        SvgElementKind::MPath => "mpath",
        SvgElementKind::Set => "set",
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
        "vector-effect" => {
            state.non_scaling_stroke = value.trim().eq_ignore_ascii_case("non-scaling-stroke");
        }
        "fill-rule" => {
            state.fill_rule = if value.trim().eq_ignore_ascii_case("evenodd") {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
        }
        "clip-rule" => {
            state.clip_rule = if value.trim().eq_ignore_ascii_case("evenodd") {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
        }
        "mask-type" => {
            state.mask_type = if value.trim().eq_ignore_ascii_case("alpha") {
                MaskType::Alpha
            } else {
                MaskType::Luminance
            };
        }
        "stop-color" => {
            if let Some(color) = parse_svg_color(value, &state.custom_props) {
                state.stop_color = color;
            }
        }
        "stop-opacity" => {
            if let Some(v) = number(value) {
                state.stop_opacity = v.clamp(0.0, 1.0);
            }
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
        "direction" => {
            state.direction = match value.trim().to_ascii_lowercase().as_str() {
                "rtl" => Direction::RTL,
                _ => Direction::LTR,
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

fn apply_svg_filter(
    pixmap: &mut Pixmap,
    filter: &SvgNode,
    target_node: &SvgNode,
    state: &PaintState,
    transform: Transform,
) {
    let source = pixmap.to_owned();
    let mut current = source.to_owned();
    let mut results: HashMap<String, Pixmap> = HashMap::new();
    results.insert("SourceGraphic".to_string(), source.to_owned());
    results.insert("SourceAlpha".to_string(), source_alpha_pixmap(&source));
    let primitive_state = filter_primitive_state(filter, target_node, state);

    for child in &filter.children {
        let tag = svg_tag_name(child);
        let mut next = filter_input(child, &current, &results);
        if tag.eq_ignore_ascii_case("feGaussianBlur") {
            let std_dev = filter_std_deviation(child).unwrap_or(0.0);
            crate::canvas::effects::blur_pixmap(&mut next, std_dev);
        } else if tag.eq_ignore_ascii_case("feOffset") {
            let dx = attr_length(child, "dx", LengthAxis::X, &primitive_state).unwrap_or(0.0);
            let dy = attr_length(child, "dy", LengthAxis::Y, &primitive_state).unwrap_or(0.0);
            offset_pixmap(&mut next, dx, dy);
        } else if tag.eq_ignore_ascii_case("feDropShadow") {
            let dx = attr_length(child, "dx", LengthAxis::X, &primitive_state).unwrap_or(2.0);
            let dy = attr_length(child, "dy", LengthAxis::Y, &primitive_state).unwrap_or(2.0);
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
            next = composite_filter_pixmaps(&next, &second, child);
        } else if tag.eq_ignore_ascii_case("feBlend") {
            let second = filter_input_named(
                child.attr("in2").unwrap_or("SourceGraphic"),
                &source,
                &current,
                &results,
            );
            next = blend_filter_pixmaps(&second, &next, child.attr("mode").unwrap_or("normal"));
        } else if tag.eq_ignore_ascii_case("feDisplacementMap") {
            let map = filter_input_named(
                child.attr("in2").unwrap_or("SourceGraphic"),
                &source,
                &current,
                &results,
            );
            next = displacement_map_filter_pixmap(&next, &map, child);
        } else if tag.eq_ignore_ascii_case("feColorMatrix") {
            next = color_matrix_filter_pixmap(
                &next,
                child.attr("type").unwrap_or("matrix"),
                child.attr("values").unwrap_or(""),
            );
        } else if tag.eq_ignore_ascii_case("feConvolveMatrix") {
            next = convolve_matrix_filter_pixmap(&next, child);
        } else if tag.eq_ignore_ascii_case("feComponentTransfer") {
            next = component_transfer_filter_pixmap(&next, child);
        } else if tag.eq_ignore_ascii_case("feMorphology") {
            next = morphology_filter_pixmap(&next, child, &primitive_state);
        } else if tag.eq_ignore_ascii_case("feImage") {
            next = image_filter_pixmap(child, &primitive_state, pixmap.width(), pixmap.height())
                .unwrap_or_else(|| Pixmap::new(pixmap.width(), pixmap.height()).unwrap_or(next));
        } else if tag.eq_ignore_ascii_case("feTile") {
            next = tile_filter_pixmap(&next);
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
    if let Some(region) = filter_region_for_node(
        filter,
        target_node,
        state,
        pixmap.width(),
        pixmap.height(),
        transform,
    ) {
        apply_alpha_mask_to_pixmap(pixmap, region.data());
    }
}

fn filter_primitive_state(
    filter: &SvgNode,
    target_node: &SvgNode,
    target_state: &PaintState,
) -> PaintState {
    if !filter
        .attr("primitiveUnits")
        .is_some_and(|value| value == "objectBoundingBox")
    {
        return target_state.clone();
    }
    let Some(target_path) = node_path(target_node, target_state) else {
        return target_state.clone();
    };
    let bounds = target_path.bounds();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return target_state.clone();
    }
    let mut state = target_state.clone();
    state.viewport_width = bounds.width();
    state.viewport_height = bounds.height();
    state
}

fn filter_region_for_node(
    filter: &SvgNode,
    target_node: &SvgNode,
    target_state: &PaintState,
    width: u32,
    height: u32,
    transform: Transform,
) -> Option<Mask> {
    let uses_object_bbox = !filter
        .attr("filterUnits")
        .is_some_and(|value| value == "userSpaceOnUse");
    let (x, y, w, h, region_transform) = if uses_object_bbox {
        let target_path = node_path(target_node, target_state)?;
        let bounds = target_path.bounds();
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return None;
        }
        let mut unit_state = target_state.clone();
        unit_state.viewport_width = 1.0;
        unit_state.viewport_height = 1.0;
        (
            attr_length(filter, "x", LengthAxis::X, &unit_state).unwrap_or(-0.1),
            attr_length(filter, "y", LengthAxis::Y, &unit_state).unwrap_or(-0.1),
            attr_length(filter, "width", LengthAxis::X, &unit_state).unwrap_or(1.2),
            attr_length(filter, "height", LengthAxis::Y, &unit_state).unwrap_or(1.2),
            transform
                .pre_translate(bounds.left(), bounds.top())
                .pre_scale(bounds.width(), bounds.height()),
        )
    } else {
        (
            attr_length(filter, "x", LengthAxis::X, target_state)
                .unwrap_or(-0.1 * target_state.viewport_width.max(1.0)),
            attr_length(filter, "y", LengthAxis::Y, target_state)
                .unwrap_or(-0.1 * target_state.viewport_height.max(1.0)),
            attr_length(filter, "width", LengthAxis::X, target_state)
                .unwrap_or(1.2 * target_state.viewport_width.max(1.0)),
            attr_length(filter, "height", LengthAxis::Y, target_state)
                .unwrap_or(1.2 * target_state.viewport_height.max(1.0)),
            transform,
        )
    };
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rect = tiny_skia::Rect::from_xywh(x, y, w, h)?;
    let mut path = PathBuilder::new();
    path.push_rect(rect);
    let path = path.finish()?;
    let mut mask = Mask::new(width, height)?;
    mask.fill_path(&path, FillRule::Winding, true, region_transform);
    Some(mask)
}

fn apply_alpha_mask_to_pixmap(pixmap: &mut Pixmap, mask: &[u8]) {
    for (px, mask_alpha) in pixmap.pixels_mut().iter_mut().zip(mask) {
        let alpha = px.alpha() as u32 * *mask_alpha as u32 / 255;
        if alpha == 0 {
            *px = PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
            continue;
        }
        let scale = alpha as f32 / px.alpha().max(1) as f32;
        let r = (px.red() as f32 * scale).round().clamp(0.0, 255.0) as u8;
        let g = (px.green() as f32 * scale).round().clamp(0.0, 255.0) as u8;
        let b = (px.blue() as f32 * scale).round().clamp(0.0, 255.0) as u8;
        *px = PremultipliedColorU8::from_rgba(r, g, b, alpha as u8)
            .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
    }
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

fn composite_filter_pixmaps(input: &Pixmap, input2: &Pixmap, node: &SvgNode) -> Pixmap {
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let operator = node.attr("operator").unwrap_or("over").trim();
    if operator == "arithmetic" {
        return arithmetic_composite_filter_pixmaps(
            input,
            input2,
            (
                node.attr("k1").and_then(number).unwrap_or(0.0),
                node.attr("k2").and_then(number).unwrap_or(0.0),
                node.attr("k3").and_then(number).unwrap_or(0.0),
                node.attr("k4").and_then(number).unwrap_or(0.0),
            ),
        );
    }
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
        let (r, g, bl, alpha) = match operator {
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

fn arithmetic_composite_filter_pixmaps(
    input: &Pixmap,
    input2: &Pixmap,
    coeffs: (f32, f32, f32, f32),
) -> Pixmap {
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let (k1, k2, k3, k4) = coeffs;
    for ((dst, a), b) in out
        .pixels_mut()
        .iter_mut()
        .zip(input.pixels())
        .zip(input2.pixels())
    {
        let (ar, ag, ab, aa) = pixel_unpremul_rgba(*a);
        let (br, bg, bb, ba) = pixel_unpremul_rgba(*b);
        let arithmetic = |ca: u8, cb: u8| -> f32 {
            let ca = ca as f32 / 255.0;
            let cb = cb as f32 / 255.0;
            k1 * ca * cb + k2 * ca + k3 * cb + k4
        };
        *dst = premul_from_unit_rgba(
            arithmetic(ar, br),
            arithmetic(ag, bg),
            arithmetic(ab, bb),
            arithmetic(aa, ba),
        );
    }
    out
}

fn displacement_map_filter_pixmap(input: &Pixmap, map: &Pixmap, node: &SvgNode) -> Pixmap {
    let scale = node.attr("scale").and_then(number).unwrap_or(0.0);
    if scale == 0.0 {
        return input.to_owned();
    }
    let x_channel = node.attr("xChannelSelector").unwrap_or("A").trim();
    let y_channel = node.attr("yChannelSelector").unwrap_or("A").trim();
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let width = input.width() as i32;
    let height = input.height() as i32;
    for y in 0..height {
        for x in 0..width {
            let map_px = map
                .pixel(x as u32, y as u32)
                .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
            let dx = (filter_channel(map_px, x_channel) - 0.5) * scale;
            let dy = (filter_channel(map_px, y_channel) - 0.5) * scale;
            let sx = (x as f32 + dx).round() as i32;
            let sy = (y as f32 + dy).round() as i32;
            let index = (y as u32 * input.width() + x as u32) as usize;
            out.pixels_mut()[index] = if sx >= 0 && sy >= 0 && sx < width && sy < height {
                input
                    .pixel(sx as u32, sy as u32)
                    .unwrap_or_else(|| PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap())
            } else {
                PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap()
            };
        }
    }
    out
}

fn filter_channel(px: PremultipliedColorU8, channel: &str) -> f32 {
    let (r, g, b, a) = pixel_unpremul_rgba(px);
    let value = match channel {
        "R" | "r" => r,
        "G" | "g" => g,
        "B" | "b" => b,
        _ => a,
    };
    value as f32 / 255.0
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

fn convolve_matrix_filter_pixmap(input: &Pixmap, node: &SvgNode) -> Pixmap {
    let kernel = node
        .attr("kernelMatrix")
        .map(number_list)
        .unwrap_or_default();
    let Some((order_x, order_y)) = convolve_order(node, kernel.len()) else {
        return input.to_owned();
    };
    if kernel.len() != order_x * order_y {
        return input.to_owned();
    }
    let sum: f32 = kernel.iter().sum();
    let divisor = node
        .attr("divisor")
        .and_then(number)
        .filter(|v| v.is_finite() && *v != 0.0)
        .unwrap_or_else(|| if sum != 0.0 { sum } else { 1.0 });
    let bias = node.attr("bias").and_then(number).unwrap_or(0.0);
    let target_x = node
        .attr("targetX")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value < order_x)
        .unwrap_or(order_x / 2);
    let target_y = node
        .attr("targetY")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value < order_y)
        .unwrap_or(order_y / 2);
    let preserve_alpha = node
        .attr("preserveAlpha")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"));
    let edge_mode = node.attr("edgeMode").unwrap_or("duplicate").trim();
    let mut out = Pixmap::new(input.width(), input.height()).expect("filter dimensions are valid");
    let width = input.width() as i32;
    let height = input.height() as i32;

    for y in 0..height {
        for x in 0..width {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let mut a = 0.0f32;
            for ky in 0..order_y {
                for kx in 0..order_x {
                    let sx = x + kx as i32 - target_x as i32;
                    let sy = y + ky as i32 - target_y as i32;
                    let Some(px) = convolve_sample(input, sx, sy, edge_mode) else {
                        continue;
                    };
                    let (pr, pg, pb, pa) = pixel_unpremul_rgba(px);
                    let weight = kernel[ky * order_x + kx];
                    r += pr as f32 * weight;
                    g += pg as f32 * weight;
                    b += pb as f32 * weight;
                    a += pa as f32 * weight;
                }
            }
            let src_alpha = input
                .pixel(x as u32, y as u32)
                .map(|px| px.alpha())
                .unwrap_or(0);
            let alpha = if preserve_alpha {
                src_alpha as f32 / 255.0
            } else {
                a / divisor / 255.0 + bias
            };
            let index = (y as u32 * input.width() + x as u32) as usize;
            out.pixels_mut()[index] = premul_from_unit_rgba(
                r / divisor / 255.0 + bias,
                g / divisor / 255.0 + bias,
                b / divisor / 255.0 + bias,
                alpha,
            );
        }
    }
    out
}

fn convolve_order(node: &SvgNode, kernel_len: usize) -> Option<(usize, usize)> {
    if let Some(order) = node.attr("order") {
        let values: Vec<usize> = order
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.trim().is_empty())
            .filter_map(|part| part.trim().parse::<usize>().ok())
            .collect();
        return match values.as_slice() {
            [one] if *one > 0 => Some((*one, *one)),
            [x, y] if *x > 0 && *y > 0 => Some((*x, *y)),
            _ => None,
        };
    }
    let side = (kernel_len as f32).sqrt() as usize;
    if side > 0 && side * side == kernel_len {
        Some((side, side))
    } else {
        None
    }
}

fn convolve_sample(
    input: &Pixmap,
    x: i32,
    y: i32,
    edge_mode: &str,
) -> Option<PremultipliedColorU8> {
    let width = input.width() as i32;
    let height = input.height() as i32;
    let (x, y) = if x >= 0 && y >= 0 && x < width && y < height {
        (x, y)
    } else if edge_mode.eq_ignore_ascii_case("wrap") {
        (x.rem_euclid(width), y.rem_euclid(height))
    } else if edge_mode.eq_ignore_ascii_case("none") {
        return Some(PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
    } else {
        (x.clamp(0, width - 1), y.clamp(0, height - 1))
    };
    input.pixel(x as u32, y as u32)
}

fn morphology_filter_pixmap(input: &Pixmap, node: &SvgNode, state: &PaintState) -> Pixmap {
    let radii = node
        .attr("radius")
        .map(|value| svg_filter_radius_list(value, state))
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
            let index = (y as u32 * input.width() + x as u32) as usize;
            out.pixels_mut()[index] = premul_channels_to_pixel(r, g, b, a);
        }
    }
    out
}

fn svg_filter_radius_list(value: &str, state: &PaintState) -> Vec<f32> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.trim().is_empty())
        .enumerate()
        .filter_map(|(index, part)| {
            let axis = if index == 0 {
                LengthAxis::X
            } else {
                LengthAxis::Y
            };
            resolve_svg_length(part, axis, state).or_else(|| number(part))
        })
        .collect()
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
    let ty = ty.trim().to_ascii_lowercase();
    for (dst, src) in out.pixels_mut().iter_mut().zip(input.pixels()) {
        let (r, g, b, a) = pixel_unpremul_rgba(*src);
        let (rf, gf, bf, af) = (
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        );
        let next = match ty.as_str() {
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
            "huerotate" => {
                let angle = nums.first().copied().unwrap_or(0.0).to_radians();
                let cos = angle.cos();
                let sin = angle.sin();
                let ir = 0.213 + cos * 0.787 - sin * 0.213;
                let ig = 0.715 - cos * 0.715 - sin * 0.715;
                let ib = 0.072 - cos * 0.072 + sin * 0.928;
                let jr = 0.213 - cos * 0.213 + sin * 0.143;
                let jg = 0.715 + cos * 0.285 + sin * 0.140;
                let jb = 0.072 - cos * 0.072 - sin * 0.283;
                let kr = 0.213 - cos * 0.213 - sin * 0.787;
                let kg = 0.715 - cos * 0.715 + sin * 0.715;
                let kb = 0.072 + cos * 0.928 + sin * 0.072;
                (
                    ir * rf + ig * gf + ib * bf,
                    jr * rf + jg * gf + jb * bf,
                    kr * rf + kg * gf + kb * bf,
                    af,
                )
            }
            "luminancetoalpha" => (0.0, 0.0, 0.0, 0.2126 * rf + 0.7152 * gf + 0.0722 * bf),
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

fn tile_filter_pixmap(input: &Pixmap) -> Pixmap {
    let Some((left, top, right, bottom)) = pixmap_alpha_bounds(input) else {
        return input.to_owned();
    };
    let tile_w = right - left + 1;
    let tile_h = bottom - top + 1;
    if tile_w == 0 || tile_h == 0 {
        return input.to_owned();
    }
    let Some(mut tile) = Pixmap::new(tile_w, tile_h) else {
        return input.to_owned();
    };
    let input_w = input.width() as usize;
    let tile_w_usize = tile_w as usize;
    for y in 0..tile_h as usize {
        for x in 0..tile_w_usize {
            let src = input.pixels()[(top as usize + y) * input_w + left as usize + x];
            tile.pixels_mut()[y * tile_w_usize + x] = src;
        }
    }

    let Some(mut out) = Pixmap::new(input.width(), input.height()) else {
        return input.to_owned();
    };
    let paint = PixmapPaint {
        opacity: 1.0,
        blend_mode: tiny_skia::BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Nearest,
    };
    let x_start = -((left as i32).rem_euclid(tile_w as i32));
    let y_start = -((top as i32).rem_euclid(tile_h as i32));
    let mut y = y_start;
    while y < input.height() as i32 {
        let mut x = x_start;
        while x < input.width() as i32 {
            out.draw_pixmap(x, y, tile.as_ref(), &paint, Transform::identity(), None);
            x += tile_w as i32;
        }
        y += tile_h as i32;
    }
    out
}

fn image_filter_pixmap(
    node: &SvgNode,
    state: &PaintState,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    let href = svg_href_value(node)?;
    let (rgba, iw, ih) = crate::html::load_image_from_src(href, "")?;
    if iw == 0 || ih == 0 {
        return None;
    }
    let src = PixmapRef::from_bytes(&rgba, iw, ih)?;
    let mut out = Pixmap::new(width, height)?;
    let x = attr_length(node, "x", LengthAxis::X, state).unwrap_or(0.0);
    let y = attr_length(node, "y", LengthAxis::Y, state).unwrap_or(0.0);
    let w = attr_length(node, "width", LengthAxis::X, state).unwrap_or(iw as f32);
    let h = attr_length(node, "height", LengthAxis::Y, state).unwrap_or(ih as f32);
    if w <= 0.0 || h <= 0.0 {
        return Some(out);
    }
    let mut paint = PixmapPaint::default();
    paint.opacity = state.opacity.clamp(0.0, 1.0);
    let image_transform = Transform::from_translate(x, y).pre_concat(view_box_transform(
        SvgViewBox {
            min_x: 0.0,
            min_y: 0.0,
            width: iw as f32,
            height: ih as f32,
        },
        w,
        h,
        parse_preserve_aspect_ratio(node.attr_ascii_case_insensitive("preserveAspectRatio")),
    ));
    out.draw_pixmap(0, 0, src, &paint, image_transform, None);
    Some(out)
}

fn pixmap_alpha_bounds(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    let mut left = pixmap.width();
    let mut top = pixmap.height();
    let mut right = 0;
    let mut bottom = 0;
    let width = pixmap.width() as usize;
    for (idx, px) in pixmap.pixels().iter().enumerate() {
        if px.alpha() == 0 {
            continue;
        }
        let x = (idx % width) as u32;
        let y = (idx / width) as u32;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    (left <= right && top <= bottom).then_some((left, top, right, bottom))
}

fn svg_href_value(node: &SvgNode) -> Option<&str> {
    node.attr("href")
        .or_else(|| node.attr("xlink:href"))
        .or_else(|| {
            node.attributes
                .iter()
                .find(|a| a.namespace.as_deref() == Some("xlink") && a.name == "href")
                .map(|a| a.value.as_str())
        })
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
    let stroke_scale = if state.non_scaling_stroke {
        transform_scale(transform)
    } else {
        1.0
    };
    let mut stroke = Stroke::default();
    stroke.width = state.stroke_width / stroke_scale;
    stroke.line_cap = state.stroke_linecap;
    stroke.line_join = state.stroke_linejoin;
    stroke.miter_limit = state.stroke_miterlimit;
    stroke.dash = state.stroke_dasharray.clone().and_then(|dash| {
        StrokeDash::new(
            dash.into_iter().map(|value| value / stroke_scale).collect(),
            state.stroke_dashoffset / stroke_scale,
        )
    });
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

fn transform_scale(transform: Transform) -> f32 {
    let sx = (transform.sx * transform.sx + transform.ky * transform.ky).sqrt();
    let sy = (transform.kx * transform.kx + transform.sy * transform.sy).sqrt();
    ((sx + sy) / 2.0).max(0.0001)
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
                    render_pattern_tile(pattern, state, path, ids, styles, ancestors, stack)
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
                        pattern_transform(pattern, state, path),
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
                .and_then(|node| gradient_shader(node, state.opacity, path, ids, styles, ancestors))
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
    path: &Path,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stack: &mut Vec<String>,
) -> Option<Pixmap> {
    let bounds = path.bounds();
    let pattern_units_bbox = !pattern
        .attr("patternUnits")
        .is_some_and(|value| value == "userSpaceOnUse");
    let mut unit_state = inherited.clone();
    if pattern_units_bbox {
        unit_state.viewport_width = 1.0;
        unit_state.viewport_height = 1.0;
    }
    let mut width = attr_length(pattern, "width", LengthAxis::X, &unit_state).unwrap_or(0.0);
    let mut height = attr_length(pattern, "height", LengthAxis::Y, &unit_state).unwrap_or(0.0);
    if pattern_units_bbox {
        width *= bounds.width();
        height *= bounds.height();
    }
    let width = width.ceil().max(1.0) as u32;
    let height = height.ceil().max(1.0) as u32;
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
    } else if pattern
        .attr("patternContentUnits")
        .is_some_and(|value| value == "objectBoundingBox")
    {
        let mut content_state = inherited.clone();
        content_state.viewport_width = bounds.width();
        content_state.viewport_height = bounds.height();
        for child in &pattern.children {
            paint_node(
                child,
                &mut tile,
                content_state.clone(),
                transform,
                ids,
                styles,
                ancestors,
                stack,
                None,
                true,
                None,
            );
        }
        return Some(tile);
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
            None,
        );
    }
    Some(tile)
}

fn pattern_transform(pattern: &SvgNode, state: &PaintState, path: &Path) -> Transform {
    let local = pattern
        .attr("patternTransform")
        .and_then(parse_transform_list)
        .unwrap_or_else(Transform::identity);
    let bounds = path.bounds();
    let pattern_units_bbox = !pattern
        .attr("patternUnits")
        .is_some_and(|value| value == "userSpaceOnUse");
    let mut unit_state = state.clone();
    if pattern_units_bbox {
        unit_state.viewport_width = 1.0;
        unit_state.viewport_height = 1.0;
    }
    let mut x = attr_length(pattern, "x", LengthAxis::X, &unit_state).unwrap_or(0.0);
    let mut y = attr_length(pattern, "y", LengthAxis::Y, &unit_state).unwrap_or(0.0);
    if pattern_units_bbox {
        x = bounds.left() + x * bounds.width();
        y = bounds.top() + y * bounds.height();
    }
    local.pre_translate(x, y)
}

fn gradient_shader<'a>(
    node: &'a SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
) -> Option<tiny_skia::Shader<'static>> {
    match node.kind {
        SvgElementKind::LinearGradient => {
            linear_gradient_shader(node, opacity, path, ids, styles, ancestors)
        }
        SvgElementKind::RadialGradient => {
            radial_gradient_shader(node, opacity, path, ids, styles, ancestors)
        }
        _ => None,
    }
}

fn linear_gradient_shader<'a>(
    node: &'a SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
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
        gradient_stops(node, opacity, ids, styles, ancestors),
        gradient_spread(node, ids),
        gradient_transform(node, ids),
    )
}

fn radial_gradient_shader<'a>(
    node: &'a SvgNode,
    opacity: f32,
    path: &Path,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
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
        gradient_stops(node, opacity, ids, styles, ancestors),
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

fn gradient_stops<'a>(
    node: &'a SvgNode,
    opacity: f32,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
) -> Vec<SkGradientStop> {
    let mut stops = Vec::new();
    collect_gradient_stops(
        node,
        opacity,
        ids,
        styles,
        ancestors,
        &mut stops,
        &mut Vec::new(),
    );
    stops
}

fn collect_gradient_stops<'a>(
    node: &'a SvgNode,
    opacity: f32,
    ids: &HashMap<String, &'a SvgNode>,
    styles: &[CssRule],
    ancestors: &mut Vec<&'a SvgNode>,
    stops: &mut Vec<SkGradientStop>,
    stack: &mut Vec<String>,
) {
    ancestors.push(node);
    let gradient_state = state_for_node(node, PaintState::default(), styles, ancestors, None);
    for child in &node.children {
        if svg_tag_name(child).eq_ignore_ascii_case("stop") {
            let offset = percent_or_number(child.attr("offset").unwrap_or("0")).unwrap_or(0.0);
            let stop_state = state_for_node(child, gradient_state.clone(), styles, ancestors, None);
            let color = with_alpha(stop_state.stop_color, stop_state.stop_opacity);
            stops.push(SkGradientStop::new(
                offset,
                apply_opacity(color, opacity).to_tiny_skia(),
            ));
        }
    }
    if !stops.is_empty() {
        ancestors.pop();
        return;
    }
    let Some(id) = href_id(node) else {
        ancestors.pop();
        return;
    };
    if stack.iter().any(|seen| seen == id) {
        ancestors.pop();
        return;
    }
    let Some(parent) = ids.get(id).copied() else {
        ancestors.pop();
        return;
    };
    stack.push(id.to_string());
    collect_gradient_stops(parent, opacity, ids, styles, ancestors, stops, stack);
    stack.pop();
    ancestors.pop();
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

fn marker_ref_length(
    marker: &SvgNode,
    name: &str,
    axis: LengthAxis,
    marker_size: f32,
    state: &PaintState,
) -> Option<f32> {
    let value = marker.attr_ascii_case_insensitive(name)?.trim();
    match value {
        "left" | "top" => Some(0.0),
        "center" => Some(marker_size / 2.0),
        "right" | "bottom" => Some(marker_size),
        percent if percent.ends_with('%') => number(percent).map(|v| v / 100.0 * marker_size),
        _ => resolve_svg_length(value, axis, state),
    }
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
    fn svg_directional_text_reorders_rtl_runs() {
        let text = "abc אבג";
        let ltr = svg_directional_text(text, Direction::LTR);
        let rtl = svg_directional_text(text, Direction::RTL);
        assert_eq!(ltr.as_ref(), text);
        assert_ne!(rtl.as_ref(), text);
        assert_eq!(rtl.chars().count(), text.chars().count());
    }

    #[test]
    fn native_rasterizer_paints_rect() {
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
    fn native_rasterizer_paints_filled_child_under_group_clip_when_root_fill_none() {
        let data = rasterize_svg_to_rgba(
            r##"<svg fill="none" viewBox="0 0 20 10">
                <g clip-path="url(#a)">
                    <path fill="#000" d="M0 0h20v10H0z"/>
                </g>
                <defs><clipPath id="a"><path fill="#fff" d="M0 0h20v10H0z"/></clipPath></defs>
            </svg>"##,
            20,
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
    fn native_rasterizer_honors_non_scaling_stroke() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="100" height="100" viewBox="0 0 10 10"><line x1="1" y1="5" x2="9" y2="5" stroke="black" stroke-width="2" vector-effect="non-scaling-stroke"/></svg>"#,
            100,
            100,
        )
        .unwrap();
        assert!(painted_at(&data, 100, 50, 50));
        assert!(!painted_at(&data, 100, 50, 42));
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
    fn native_rasterizer_preserves_image_aspect_ratio_by_default() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="20"><image width="20" height="20" href="data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMiIgaGVpZ2h0PSIxIj48cmVjdCB3aWR0aD0iMiIgaGVpZ2h0PSIxIiBmaWxsPSJibGFjayIvPjwvc3ZnPg=="/></svg>"##,
            20,
            20,
        )
        .unwrap();
        assert_eq!(alpha_at(&data, 20, 10, 2), 0);
        assert!(painted_at(&data, 20, 10, 10));
        assert_eq!(alpha_at(&data, 20, 10, 18), 0);
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
    fn native_rasterizer_applies_stylesheet_rules_to_gradient_stops() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <style>.hot { stop-color: rgb(255, 0, 0); stop-opacity: 1 }</style>
                <defs><linearGradient id="g"><stop class="hot" offset="0%"/><stop offset="100%" stop-color="blue"/></linearGradient></defs>
                <rect width="20" height="10" fill="url(#g)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 1, 5);
        assert!(
            r > 150 && g < 80 && b < 120 && a > 200,
            "gradient stop pixel was {r},{g},{b},{a}"
        );
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
    fn native_rasterizer_switch_paints_first_supported_child() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="20">
                <switch>
                    <rect width="20" height="20" fill="red" requiredExtensions="https://example.invalid/svg-ext"/>
                    <rect width="20" height="20" fill="rgb(0, 180, 0)"/>
                    <rect width="20" height="20" fill="blue"/>
                </switch>
            </svg>"#,
            20,
            20,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 10, 10);
        assert!(
            r < 60 && g > 120 && b < 80 && a > 200,
            "switch should skip unsupported child and paint first supported child, got {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_switch_honors_system_language() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="20">
                <switch>
                    <rect width="20" height="20" fill="red" systemLanguage="fr"/>
                    <rect width="20" height="20" fill="black" systemLanguage="en-US"/>
                </switch>
            </svg>"#,
            20,
            20,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 10, 10);
        assert!(
            r < 30 && g < 30 && b < 30 && a > 200,
            "switch should choose matching language child, got {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_does_not_paint_view_elements() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="20" height="20">
                <view id="detail" viewBox="0 0 10 10">
                    <rect width="20" height="20" fill="black"/>
                </view>
                <cursor id="cursor-resource">
                    <rect width="20" height="20" fill="black"/>
                </cursor>
            </svg>"#,
            20,
            20,
        )
        .unwrap();
        assert!(!painted_at(&data, 20, 10, 10));
    }

    #[test]
    fn native_rasterizer_paints_foreign_object_html() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="30" height="20">
                <foreignObject x="5" y="4" width="20" height="10">
                    <div xmlns="http://www.w3.org/1999/xhtml" style="width:20px;height:10px;background:#000"></div>
                </foreignObject>
            </svg>"#,
            30,
            20,
        )
        .unwrap();
        assert!(!painted_at(&data, 30, 2, 5));
        assert!(painted_at(&data, 30, 10, 8));
        assert!(!painted_at(&data, 30, 27, 8));
    }

    #[test]
    fn native_rasterizer_paints_anchor_children_but_not_metadata() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="30" height="10">
                <title><rect width="10" height="10" fill="red"/></title>
                <desc><rect width="10" height="10" fill="red"/></desc>
                <metadata><rect width="10" height="10" fill="red"/></metadata>
                <a href="#target"><rect x="12" width="10" height="10" fill="black"/></a>
            </svg>"##,
            30,
            10,
        )
        .unwrap();
        assert!(!painted_at(&data, 30, 5, 5));
        assert!(painted_at(&data, 30, 16, 5));
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
    fn native_rasterizer_paints_path_without_viewbox_using_default_fill() {
        let data = rasterize_svg_to_rgba(
            r#"<svg width="24" height="24">
                <path d="M9.021 1.811l-.525.525c.938.938 1.5 2.25 1.5 3.675s-.563 2.738-1.5 3.675l.525.525c1.05-1.087 1.725-2.55 1.725-4.2s-.675-3.112-1.725-4.2z"/>
                <path d="M10.596.199l-.525.562c1.35 1.35 2.175 3.225 2.175 5.25s-.825 3.9-2.175 5.25l.525.525c1.5-1.462 2.4-3.525 2.4-5.775s-.9-4.312-2.4-5.812zM6.996 1.511l-2.25 2.25H.996v4.5h3.75l2.25 2.25z"/>
              </svg>"#,
            24,
            24,
        )
        .expect("speaker SVG should rasterize");
        let painted = data
            .chunks_exact(4)
            .filter(|px| px[3] > 0 && px[0] < 80 && px[1] < 80 && px[2] < 80)
            .count();
        assert!(painted > 20, "speaker path should produce dark pixels");
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
    fn native_rasterizer_applies_text_path_side_right() {
        let left = rasterize_svg_to_rgba(
            r##"<svg width="130" height="70">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline">I</textPath></text>
            </svg>"##,
            130,
            70,
        )
        .unwrap();
        let right = rasterize_svg_to_rgba(
            r##"<svg width="130" height="70">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline" side="right">I</textPath></text>
            </svg>"##,
            130,
            70,
        )
        .unwrap();
        let left_bounds = painted_bounds(&left, 130).expect("left textPath bounds");
        let right_bounds = painted_bounds(&right, 130).expect("right textPath bounds");
        assert!(
            right_bounds.1 > left_bounds.1 + 12,
            "side=right should move text to the right side of the path, left={left_bounds:?} right={right_bounds:?}"
        );
    }

    #[test]
    fn native_rasterizer_applies_text_path_text_length_spacing() {
        let normal = rasterize_svg_to_rgba(
            r##"<svg width="130" height="40">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline">II</textPath></text>
            </svg>"##,
            130,
            40,
        )
        .unwrap();
        let adjusted = rasterize_svg_to_rgba(
            r##"<svg width="130" height="40">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline" textLength="80">II</textPath></text>
            </svg>"##,
            130,
            40,
        )
        .unwrap();
        let normal_bounds = painted_bounds(&normal, 130).expect("normal textPath bounds");
        let adjusted_bounds = painted_bounds(&adjusted, 130).expect("adjusted textPath bounds");
        assert!(
            adjusted_bounds.2 > normal_bounds.2 + 30,
            "textPath textLength should widen painted text, normal={normal_bounds:?} adjusted={adjusted_bounds:?}"
        );
    }

    #[test]
    fn native_rasterizer_applies_text_path_method_stretch() {
        let normal = rasterize_svg_to_rgba(
            r##"<svg width="130" height="40">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline">II</textPath></text>
            </svg>"##,
            130,
            40,
        )
        .unwrap();
        let stretched = rasterize_svg_to_rgba(
            r##"<svg width="130" height="40">
                <defs><path id="baseline" d="M10 28 H120"/></defs>
                <text fill="black" font-size="20"><textPath href="#baseline" method="stretch">II</textPath></text>
            </svg>"##,
            130,
            40,
        )
        .unwrap();
        let normal_bounds = painted_bounds(&normal, 130).expect("normal textPath bounds");
        let stretched_bounds = painted_bounds(&stretched, 130).expect("stretched textPath bounds");
        assert!(
            stretched_bounds.2 > normal_bounds.2 + 50,
            "method=stretch should distribute text over the path, normal={normal_bounds:?} stretched={stretched_bounds:?}"
        );
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
    fn native_rasterizer_applies_evenodd_clip_rule() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="20">
                <defs><clipPath id="c"><path clip-rule="evenodd" d="M1 1 H19 V19 H1 Z M6 6 H14 V14 H6 Z"/></clipPath></defs>
                <rect width="20" height="20" fill="black" clip-path="url(#c)"/>
            </svg>"##,
            20,
            20,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 3, 3));
        assert_eq!(alpha_at(&data, 20, 10, 10), 0);
    }

    #[test]
    fn native_rasterizer_applies_object_bounding_box_clip_path_units() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><clipPath id="c" clipPathUnits="objectBoundingBox">
                    <rect x="0" y="0" width="0.5" height="1"/>
                </clipPath></defs>
                <rect x="4" y="2" width="12" height="8" fill="black" clip-path="url(#c)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 6));
        assert!(!painted_at(&data, 24, 13, 6));
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
    fn native_rasterizer_applies_alpha_mask_type() {
        let alpha_mask = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><mask id="m" mask-type="alpha"><rect width="10" height="10" fill="black"/></mask></defs><rect width="20" height="10" fill="red" mask="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        let luminance_mask = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10"><defs><mask id="m"><rect width="10" height="10" fill="black"/></mask></defs><rect width="20" height="10" fill="red" mask="url(#m)"/></svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&alpha_mask, 20, 5, 5));
        assert_eq!(alpha_at(&luminance_mask, 20, 5, 5), 0);
    }

    #[test]
    fn native_rasterizer_applies_object_bounding_box_mask_content_units() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><mask id="m" maskContentUnits="objectBoundingBox">
                    <rect x="0" y="0" width="50%" height="100%" fill="white"/>
                </mask></defs>
                <rect x="4" y="2" width="12" height="8" fill="black" mask="url(#m)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 6));
        assert!(!painted_at(&data, 24, 13, 6));
    }

    #[test]
    fn native_rasterizer_clips_mask_to_user_space_region() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><mask id="m" maskUnits="userSpaceOnUse" x="4" y="2" width="6" height="8">
                    <rect width="24" height="12" fill="white"/>
                </mask></defs>
                <rect width="24" height="12" fill="black" mask="url(#m)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 6));
        assert!(!painted_at(&data, 24, 13, 6));
    }

    #[test]
    fn native_rasterizer_clips_mask_to_object_bounding_box_region() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><mask id="m" x="0" y="0" width="50%" height="100%">
                    <rect width="24" height="12" fill="white"/>
                </mask></defs>
                <rect x="4" y="2" width="12" height="8" fill="black" mask="url(#m)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 6));
        assert!(!painted_at(&data, 24, 13, 6));
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
    fn native_rasterizer_reverses_auto_start_reverse_marker_start() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="10">
                <defs><marker id="m" markerWidth="4" markerHeight="4" refX="0" refY="2" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
                    <path d="M0 0 L4 2 L0 4 Z" fill="black"/>
                </marker></defs>
                <line x1="10" y1="5" x2="18" y2="5" stroke="none" marker-start="url(#m)"/>
            </svg>"##,
            24,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 5));
        assert!(!painted_at(&data, 24, 13, 5));
    }

    #[test]
    fn native_rasterizer_defaults_marker_units_to_stroke_width() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><marker id="m" refX="0" refY="1">
                    <rect width="2" height="2" fill="black"/>
                </marker></defs>
                <line x1="4" y1="6" x2="12" y2="6" stroke="none" stroke-width="3" marker-end="url(#m)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 16, 6));
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
    fn marker_mid_angle_average_handles_180_wraparound() {
        let angle = average_marker_angle(170.0, -170.0).abs();
        assert!(
            angle > 170.0,
            "mid marker angle should follow the long-axis bisector, got {angle}"
        );
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
    fn native_rasterizer_keeps_path_markers_inside_each_subpath() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><marker id="m" markerWidth="2" markerHeight="2" refX="1" refY="1" markerUnits="userSpaceOnUse"><rect width="2" height="2" fill="black"/></marker></defs>
                <path d="M2 3 L6 3 M18 3 L22 3" fill="none" stroke="none" marker-end="url(#m)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 6, 3));
        assert!(painted_at(&data, 24, 22, 3));
    }

    #[test]
    fn native_rasterizer_resolves_marker_ref_keyword_positions() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="14" height="8">
                <defs><marker id="m" markerWidth="6" markerHeight="4" refX="center" refY="center" markerUnits="userSpaceOnUse">
                    <rect width="6" height="4" fill="black"/>
                </marker></defs>
                <line x1="8" y1="4" x2="8" y2="4" stroke="none" marker-end="url(#m)"/>
            </svg>"##,
            14,
            8,
        )
        .unwrap();
        assert!(painted_at(&data, 14, 5, 3));
        assert!(painted_at(&data, 14, 10, 5));
        assert!(!painted_at(&data, 14, 11, 6));
    }

    #[test]
    fn native_rasterizer_resolves_marker_ref_percentages_against_marker_viewport() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="40" height="20">
                <defs><marker id="m" markerWidth="10" markerHeight="10" refX="50%" refY="50%" markerUnits="userSpaceOnUse">
                    <rect width="10" height="10" fill="black"/>
                </marker></defs>
                <line x1="20" y1="10" x2="20" y2="10" stroke="none" marker-end="url(#m)"/>
            </svg>"##,
            40,
            20,
        )
        .unwrap();
        assert!(painted_at(&data, 40, 15, 5));
        assert!(painted_at(&data, 40, 24, 14));
        assert!(!painted_at(&data, 40, 9, 10));
        assert!(!painted_at(&data, 40, 31, 10));
    }

    #[test]
    fn native_rasterizer_resolves_pattern_paint_server() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="12" height="6"><defs><pattern id="p" patternUnits="userSpaceOnUse" width="4" height="4"><rect width="2" height="4" fill="black"/></pattern></defs><rect width="12" height="6" fill="url(#p)"/></svg>"##,
            12,
            6,
        )
        .unwrap();
        assert!(painted_at(&data, 12, 0, 2));
        assert!(!painted_at(&data, 12, 3, 2));
        assert!(painted_at(&data, 12, 4, 2));
    }

    #[test]
    fn native_rasterizer_resolves_object_bounding_box_pattern_units() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><pattern id="p" patternUnits="objectBoundingBox" patternContentUnits="objectBoundingBox" width="50%" height="100%">
                    <rect width="25%" height="100%" fill="black"/>
                </pattern></defs>
                <rect x="4" y="2" width="12" height="8" fill="url(#p)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 5, 6));
        assert!(!painted_at(&data, 24, 8, 6));
        assert!(painted_at(&data, 24, 11, 6));
    }

    #[test]
    fn native_rasterizer_applies_basic_svg_blur_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="30" height="20"><defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="30" height="20"><feGaussianBlur stdDeviation="2"/></filter></defs><rect x="12" y="7" width="4" height="4" fill="black" filter="url(#f)"/></svg>"##,
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
            r##"<svg width="30" height="20"><defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="30" height="20"><feDropShadow dx="8" dy="0" stdDeviation="0" flood-color="black"/></filter></defs><rect x="4" y="6" width="6" height="6" fill="red" filter="url(#f)"/></svg>"##,
            30,
            20,
        )
        .unwrap();
        assert!(painted_at(&data, 30, 6, 8));
        assert!(painted_at(&data, 30, 14, 8));
    }

    #[test]
    fn native_rasterizer_clips_filter_to_user_space_region() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="10" height="12">
                    <feFlood flood-color="black"/>
                </filter></defs>
                <rect width="24" height="12" fill="red" filter="url(#f)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 5, 6));
        assert!(!painted_at(&data, 24, 15, 6));
    }

    #[test]
    fn native_rasterizer_clips_filter_to_object_bounding_box_region() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><filter id="f" x="0" y="0" width="50%" height="100%">
                    <feFlood flood-color="black"/>
                </filter></defs>
                <rect x="4" y="2" width="12" height="8" fill="red" filter="url(#f)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(painted_at(&data, 24, 7, 6));
        assert!(!painted_at(&data, 24, 13, 6));
    }

    #[test]
    fn native_rasterizer_resolves_object_bounding_box_filter_primitive_units() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="24" height="12">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="24" height="12" primitiveUnits="objectBoundingBox">
                    <feOffset dx="50%" dy="0"/>
                </filter></defs>
                <rect x="2" y="3" width="8" height="4" fill="black" filter="url(#f)"/>
            </svg>"##,
            24,
            12,
        )
        .unwrap();
        assert!(!painted_at(&data, 24, 3, 5));
        assert!(painted_at(&data, 24, 7, 5));
        assert!(!painted_at(&data, 24, 14, 5));
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
    fn native_rasterizer_evaluates_svg_arithmetic_composite_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feFlood flood-color="rgb(0, 0, 255)" result="blue"/>
                    <feComposite in="SourceGraphic" in2="blue" operator="arithmetic" k2="0.5" k3="0.5"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 0, 0)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r > 90 && r < 180 && g < 40 && b > 90 && b < 180 && a > 200,
            "arithmetic composite pixel was {r},{g},{b},{a}"
        );
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
    fn native_rasterizer_evaluates_svg_tile_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="12" height="8">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="12" height="8">
                    <feTile/>
                </filter></defs>
                <rect x="2" y="2" width="3" height="2" fill="black" filter="url(#f)"/>
            </svg>"##,
            12,
            8,
        )
        .unwrap();
        assert!(painted_at(&data, 12, 2, 2));
        assert!(painted_at(&data, 12, 7, 3));
        assert!(painted_at(&data, 12, 10, 5));
    }

    #[test]
    fn native_rasterizer_evaluates_svg_image_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="14" height="8">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="14" height="8">
                    <feImage x="6" y="3" width="4" height="2" href="data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMiIgaGVpZ2h0PSIxIj48cmVjdCB3aWR0aD0iMiIgaGVpZ2h0PSIxIiBmaWxsPSJibGFjayIvPjwvc3ZnPg=="/>
                </filter></defs>
                <rect x="1" y="1" width="2" height="2" fill="red" filter="url(#f)"/>
            </svg>"##,
            14,
            8,
        )
        .unwrap();
        assert_eq!(alpha_at(&data, 14, 2, 2), 0);
        assert!(painted_at(&data, 14, 7, 4));
    }

    #[test]
    fn native_rasterizer_evaluates_svg_convolve_matrix_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="9" height="7">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="9" height="7">
                    <feConvolveMatrix order="3" kernelMatrix="1 1 1 1 1 1 1 1 1" divisor="9"/>
                </filter></defs>
                <rect x="4" y="3" width="1" height="1" fill="black" filter="url(#f)"/>
            </svg>"##,
            9,
            7,
        )
        .unwrap();
        assert!(alpha_at(&data, 9, 4, 3) > 20);
        assert!(alpha_at(&data, 9, 3, 3) > 20);
        assert_eq!(alpha_at(&data, 9, 1, 1), 0);
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
    fn native_rasterizer_evaluates_svg_displacement_map_filter() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="10" height="7">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="10" height="7">
                    <feFlood flood-color="rgb(255, 128, 128)" result="map"/>
                    <feDisplacementMap in="SourceGraphic" in2="map" scale="2" xChannelSelector="R" yChannelSelector="G"/>
                </filter></defs>
                <rect x="4" y="3" width="2" height="1" fill="black" filter="url(#f)"/>
            </svg>"##,
            10,
            7,
        )
        .unwrap();
        assert!(painted_at(&data, 10, 3, 3));
        assert!(!painted_at(&data, 10, 5, 3));
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
    fn native_rasterizer_evaluates_svg_color_matrix_hue_rotate() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feColorMatrix type="hueRotate" values="120"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 0, 0)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r < 120 && g > 80 && b < 120 && a > 200,
            "hue-rotate pixel was {r},{g},{b},{a}"
        );
    }

    #[test]
    fn native_rasterizer_evaluates_svg_color_matrix_luminance_to_alpha() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f">
                    <feColorMatrix type="luminanceToAlpha"/>
                </filter></defs>
                <rect x="2" y="2" width="8" height="6" fill="rgb(255, 255, 255)" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        let (r, g, b, a) = rgba_at(&data, 20, 5, 5);
        assert!(
            r < 20 && g < 20 && b < 20 && a > 200,
            "luminance-to-alpha pixel was {r},{g},{b},{a}"
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

    #[test]
    fn native_rasterizer_evaluates_svg_morphology_dilate() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f" filterUnits="userSpaceOnUse" x="0" y="0" width="20" height="10"><feMorphology operator="dilate" radius="2"/></filter></defs>
                <rect x="8" y="4" width="2" height="2" fill="black" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        assert!(painted_at(&data, 20, 8, 4));
        assert!(painted_at(&data, 20, 6, 4));
        assert!(painted_at(&data, 20, 10, 4));
    }

    #[test]
    fn native_rasterizer_evaluates_svg_morphology_erode() {
        let data = rasterize_svg_to_rgba(
            r##"<svg width="20" height="10">
                <defs><filter id="f"><feMorphology operator="erode" radius="1"/></filter></defs>
                <rect x="6" y="2" width="8" height="6" fill="black" filter="url(#f)"/>
            </svg>"##,
            20,
            10,
        )
        .unwrap();
        assert_eq!(alpha_at(&data, 20, 6, 4), 0);
        assert!(painted_at(&data, 20, 8, 4));
    }

    #[test]
    fn native_rasterizer_paints_external_wordmark_svg_shape() {
        let data = rasterize_svg_to_rgba(
            r##"<svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 140 22">
                <g clip-path="url(#a)">
                    <path fill="#0e65c0" d="M118 0h18v18h-18z"/>
                    <path fill="#000" d="M0 .5h18v17H0zM24 .5h18v17H24zM48 .5h18v17H48zM72 .5h18v17H72zM96 .5h18v17H96z"/>
                </g>
                <defs><clipPath id="a"><path fill="#fff" d="M0 0h140v21.42H0z"/></clipPath></defs>
            </svg>"##,
            140,
            22,
        )
        .unwrap();
        assert!(painted_at(&data, 140, 8, 8), "black wordmark path missing");
        let (r, g, b, a) = rgba_at(&data, 140, 125, 8);
        assert!(
            r < 40 && g > 70 && b > 140 && a > 200,
            "blue wordmark path was {r},{g},{b},{a}"
        );
    }
}
