//! Display list builder — walks the box tree and records paint commands.
//!
//! Uses EXACT positions from the layout engine. Never approximates.
//! Faithfully ports the render_box logic from mod.rs into PaintCmd recording.

use super::display_list::{DisplayList, ImageRef, PaintCmd, TextDecoration};
use crate::types::{
    BackgroundClip, BackgroundSize, ClipPathKind, Color, ComputedStyle, ContentVisibility, Display,
    FontStyle, GradientRadialShape, GradientRadialSize, GradientType, ListStylePosition,
    ListStyleType, MixBlendMode, Overflow, Position, Resize, TextDecorationStyle, TextOverflow,
    TextTransform, WhiteSpace,
};
use crate::types::{Rect, WebCore};

/// Build a display list from a laid-out box tree.
pub fn build_display_list(root: &WebCore, viewport_w: f32, viewport_h: f32) -> DisplayList {
    let visited = std::collections::HashSet::new();
    // Use full document extent as clip — viewport culling is done at replay time.
    // Building with viewport clip causes scrolled-to content to be missing.
    let doc_h = crate::types::Document::scroll_height(root).max(viewport_h);
    let ctx = BuildContext {
        scroll_x: 0.0,
        scroll_y: 0.0,
        sticky_scroll_x: 0.0,
        sticky_scroll_y: 0.0,
        hovered_id: 0,
        active_id: 0,
        visited_hrefs: &visited,
        base_url: "",
        clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        suppress_deferred_z_descendants: false,
        transform_ctx: crate::types::TransformCtx {
            // The root box's font size IS the root font size — `rem`.
            font_px: root.style.font_size_px(16.0, 16.0),
            root_font_px: root.style.font_size_px(16.0, 16.0),
            viewport_w,
            viewport_h,
        },
    };
    let mut list = DisplayList::new();
    build_for_box(root, &mut list, &ctx);
    list
}

/// Build with full context (scroll, hover, etc.).
pub fn build_display_list_full(
    root: &WebCore,
    viewport_w: f32,
    viewport_h: f32,
    scroll_x: f32,
    scroll_y: f32,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &std::collections::HashSet<String>,
    base_url: &str,
) -> DisplayList {
    let doc_h = crate::types::Document::scroll_height(root).max(viewport_h);
    let ctx = BuildContext {
        // ⛔ The list is built in DOCUMENT coordinates so replay can translate
        // it to any scroll position. The caller's scroll is kept only for
        // `position: sticky`, whose position genuinely depends on it.
        scroll_x: 0.0,
        scroll_y: 0.0,
        sticky_scroll_x: scroll_x,
        sticky_scroll_y: scroll_y,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        suppress_deferred_z_descendants: false,
        transform_ctx: crate::types::TransformCtx {
            // The root box's font size IS the root font size — `rem`.
            font_px: root.style.font_size_px(16.0, 16.0),
            root_font_px: root.style.font_size_px(16.0, 16.0),
            viewport_w,
            viewport_h,
        },
    };
    let mut list = DisplayList::new();
    build_for_box(root, &mut list, &ctx);

    // Fixed elements: rendered at viewport position (already scroll=0)
    let fixed_ctx = BuildContext {
        scroll_x: 0.0,
        scroll_y: 0.0,
        sticky_scroll_x: 0.0,
        sticky_scroll_y: 0.0,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        clip: Rect::new(0.0, 0.0, viewport_w, viewport_h),
        suppress_deferred_z_descendants: false,
        transform_ctx: ctx.transform_ctx,
    };
    let mut fixed_ids = Vec::new();
    collect_fixed_elements(root, &mut fixed_ids);
    for fid in fixed_ids {
        fn find_node(node: &WebCore, id: u32) -> Option<&WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = find_node(child, id) {
                    return Some(found);
                }
            }
            None
        }
        if let Some(node) = find_node(root, fid) {
            // Into a list of its own — see `DisplayList::fixed_commands`.
            let mut fixed = DisplayList::new();
            build_for_box(node, &mut fixed, &fixed_ctx);
            list.fixed_commands.extend(fixed.commands);
        }
    }

    list
}

#[derive(Clone, Copy)]
struct BuildContext<'a> {
    scroll_x: f32,
    scroll_y: f32,
    /// The live scroll offset, for `position: sticky` ONLY.
    ///
    /// ⛔ `scroll_x/y` are 0 now — the list is built in DOCUMENT coordinates so
    /// one cached list can serve every scroll position. Sticky is the one
    /// scheme whose position genuinely depends on the scroll, so it needs the
    /// real value, and a page containing one has to rebuild its list when the
    /// scroll changes (see `Renderer::render`).
    sticky_scroll_x: f32,
    sticky_scroll_y: f32,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &'a std::collections::HashSet<String>,
    base_url: &'a str,
    clip: Rect,
    suppress_deferred_z_descendants: bool,
    /// What a `transform` needs to resolve `vw`/`vh` and `rem`. Carried on the
    /// context because the element's own box is not enough: a transform length
    /// can name the viewport.
    transform_ctx: crate::types::TransformCtx,
}

fn encode_filter_ops(filters: &crate::types::CssFilters) -> Vec<(u8, f32, f32, f32, Color)> {
    use crate::types::FilterOp;
    filters
        .ops
        .iter()
        .map(|op| match op {
            FilterOp::Blur(v) => (0, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Brightness(v) => (1, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Contrast(v) => (2, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Grayscale(v) => (3, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::HueRotate(v) => (4, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Invert(v) => (5, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Opacity(v) => (6, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Saturate(v) => (7, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::Sepia(v) => (8, *v, 0.0, 0.0, Color::TRANSPARENT),
            FilterOp::DropShadow {
                dx,
                dy,
                blur,
                color,
            } => (9, *blur, *dx, *dy, *color),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main entry: build_for_box — mirrors render_box in mod.rs exactly
// ═══════════════════════════════════════════════════════════════════════════════

fn build_for_box(node: &WebCore, list: &mut DisplayList, ctx: &BuildContext) {
    // ── Early exits (same as render_box) ─────────────────────────────────────
    if matches!(node.style.display, Display::None) {
        return;
    }
    if !node.style.visibility {
        return;
    }
    if node.style.opacity <= 0.0 {
        return;
    }
    if ctx.suppress_deferred_z_descendants && is_explicit_z_positioned(node) {
        return;
    }

    // Display::Contents — skip the box itself, render children only
    if matches!(node.style.display, Display::Contents) {
        for child in node.effective_children() {
            build_for_box(child, list, ctx);
        }
        return;
    }

    let sx = ctx.scroll_x;
    let sy = ctx.scroll_y;
    let br = node.layout.border_rect;

    // ── Viewport culling ─────────────────────────────────────────────────────
    // Only cull elements that clip their children (overflow != visible).
    // Elements with overflow:visible may have children that extend beyond
    // the element's border_rect (e.g. height:100vh wrapper with overflowing content).
    let clips_children = matches!(
        node.style.overflow_x,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    ) || matches!(
        node.style.overflow_y,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    );
    if clips_children
        && matches!(node.style.position, Position::Static | Position::Relative)
        && !node.style.is_inline_level()
        && !matches!(node.style.display, Display::Contents)
    {
        let bx = br.x - sx;
        let by = br.y - sy;
        if bx + br.w < ctx.clip.x
            || by + br.h < ctx.clip.y
            || bx > ctx.clip.right()
            || by > ctx.clip.bottom()
        {
            return;
        }
    }

    let pr = node.layout.padding_rect;
    let px = pr.x - sx;
    let py = pr.y - sy;
    let pw = pr.w;
    let ph = pr.h;
    let font_px = node.style.font_size_px(16.0, 16.0);

    if is_laid_out_text_node(node) {
        let cr = node.layout.content_rect;
        let line_h = node
            .style
            .line_height
            .resolve(font_px, 0.0, 16.0)
            .max(font_px * 1.2);
        let y = cr.y - sy + ((cr.h - line_h).max(0.0) * 0.5);
        emit_text(list, cr.x - sx, y, &node.text, &node.style, font_px, line_h);
        return;
    }

    // ── Border radii, per corner ─────────────────────────────────────────────
    //
    // ⛔ THE FOUR LONGHANDS, always. `border_radius` is not a "the shorthand was
    // used" flag — it is a mirror of the top-left longhand, which both the
    // shorthand and `border-top-left-radius` write. Preferring it whenever it
    // was non-zero stamped the top-left value onto all four corners, so
    // `border-radius: 16px 16px 0 0` — the top-rounded card — came out rounded
    // all round, and the same corrupted array feeds the border stroke and the
    // overflow clip.
    let r_tl = node
        .style
        .border_top_left_radius
        .resolve(font_px, pr.w, 16.0);
    let r_tr = node
        .style
        .border_top_right_radius
        .resolve(font_px, pr.w, 16.0);
    let r_br = node
        .style
        .border_bottom_right_radius
        .resolve(font_px, pr.w, 16.0);
    let r_bl = node
        .style
        .border_bottom_left_radius
        .resolve(font_px, pr.w, 16.0);
    let r_tl_y = node
        .style
        .border_top_left_radius_y
        .resolve(font_px, pr.h, 16.0);
    let r_tr_y = node
        .style
        .border_top_right_radius_y
        .resolve(font_px, pr.h, 16.0);
    let r_br_y = node
        .style
        .border_bottom_right_radius_y
        .resolve(font_px, pr.h, 16.0);
    let r_bl_y = node
        .style
        .border_bottom_left_radius_y
        .resolve(font_px, pr.h, 16.0);
    let radii_arr = [r_tl, r_tr, r_br, r_bl];
    let radii_y_arr = [r_tl_y, r_tr_y, r_br_y, r_bl_y];

    // ── Hover / active / visited check ───────────────────────────────────────
    let is_hovered = ctx.hovered_id != 0
        && node.style.hover_style.is_some()
        && subtree_has(node, ctx.hovered_id);
    let is_active =
        ctx.active_id != 0 && node.style.active_style.is_some() && subtree_has(node, ctx.active_id);
    let is_visited = node.style.visited_style.is_some()
        && !node.style.href.is_empty()
        && ctx.visited_hrefs.contains(&node.style.href);

    let eff_style: &ComputedStyle = if is_active {
        node.style.active_style.as_deref().unwrap_or(&node.style)
    } else if is_visited {
        node.style.visited_style.as_deref().unwrap_or(&node.style)
    } else if is_hovered {
        node.style.hover_style.as_deref().unwrap_or(&node.style)
    } else {
        &node.style
    };

    if node.top_layer_kind == Some(crate::types::TopLayerKind::ModalDialog) {
        if let Some(backdrop_style) = node.style.backdrop_style.as_deref() {
            let color = backdrop_style.background_color;
            if color.a > 0 {
                list.push(PaintCmd::FillRect {
                    rect: Rect::new(
                        ctx.sticky_scroll_x,
                        ctx.sticky_scroll_y,
                        ctx.transform_ctx.viewport_w,
                        ctx.transform_ctx.viewport_h,
                    ),
                    color,
                    radius: [0.0; 4],
                    radius_y: [0.0; 4],
                });
            }
        }
    }

    // ── Sticky positioning ───────────────────────────────────────────────────
    let (px, py) = if node.style.position == Position::Sticky {
        let top_val = node.style.top.resolve(font_px, ctx.clip.h, 16.0);
        let left_val = node.style.left.resolve(font_px, ctx.clip.w, 16.0);
        let bottom_val = node.style.bottom.resolve(font_px, ctx.clip.h, 16.0);
        let right_val = node.style.right.resolve(font_px, ctx.clip.w, 16.0);
        let nat_x = pr.x - sx;
        let nat_y = pr.y - sy;
        let viewport_left = ctx.sticky_scroll_x + ctx.clip.x;
        let viewport_top = ctx.sticky_scroll_y + ctx.clip.y;
        let viewport_right = viewport_left + ctx.transform_ctx.viewport_w;
        let viewport_bottom = viewport_top + ctx.transform_ctx.viewport_h;

        let mut cx = nat_x;
        if !node.style.left.is_auto() {
            cx = cx.max(viewport_left + left_val);
        }
        if !node.style.right.is_auto() {
            cx = cx.min(viewport_right - right_val - pw);
        }

        let mut cy = nat_y;
        if !node.style.top.is_auto() {
            cy = cy.max(viewport_top + top_val);
        }
        if !node.style.bottom.is_auto() {
            cy = cy.min(viewport_bottom - bottom_val - ph);
        }
        (cx, cy)
    } else {
        (px, py)
    };

    // Effective scroll offsets accounting for sticky clamping
    let eff_sx = pr.x - px;
    let eff_sy = pr.y - py;

    // ── Legacy clip: rect(top, right, bottom, left) ──────────────────────────
    if let Some(cr) = node.style.clip_rect {
        let clip_right = if cr[1] == f32::MAX { pw } else { cr[1] };
        let clip_bottom = if cr[2] == f32::MAX { ph } else { cr[2] };
        let cw = clip_right - cr[3];
        let ch = clip_bottom - cr[0];
        if cw <= 0.0 || ch <= 0.0 {
            return;
        }
    }

    // ── Stacking context ─────────────────────────────────────────────────────
    let blend = blend_mode_to_u8(eff_style.mix_blend_mode);
    let stacking = is_explicit_z_positioned(node)
        || eff_style.opacity < 1.0
        || !eff_style.css_transform.ops.is_empty()
        || !eff_style.css_filter.ops.is_empty()
        || !eff_style.rare().backdrop_filter.is_empty()
        || eff_style.will_change_transform
        || eff_style.isolation
        || blend != 0
        || matches!(eff_style.position, Position::Fixed | Position::Sticky);
    if stacking {
        list.push(PaintCmd::BeginStackingContext {
            node_id: node.node_id,
            z_index: eff_style.z_index,
        });
    }

    // ── Opacity ──────────────────────────────────────────────────────────────
    if eff_style.opacity < 1.0 {
        list.push(PaintCmd::PushOpacity {
            alpha: eff_style.opacity,
        });
    }

    // ── Blend mode ───────────────────────────────────────────────────────────
    if blend != 0 {
        list.push(PaintCmd::PushBlendMode { mode: blend });
    }

    // ── CSS transform ────────────────────────────────────────────────────────
    // Use the element's DOCUMENT position (pr.x, pr.y) for the transform origin,
    // not the scroll-adjusted position (px, py). The scroll offset is applied
    // separately by the replay's global transform. This prevents transforms from
    // shifting when the user scrolls.
    let has_transform = !eff_style.css_transform.ops.is_empty();
    if has_transform {
        let source_rect = match eff_style.transform_box.as_str() {
            "content-box" => node.layout.content_rect,
            "padding-box" => node.layout.padding_rect,
            _ => node.layout.border_rect,
        };
        let tr_rect = Rect::new(
            source_rect.x - sx,
            source_rect.y - sy,
            source_rect.w,
            source_rect.h,
        );
        list.push(PaintCmd::PushTransform {
            transform: compute_transform_matrix(
                eff_style,
                &tr_rect,
                &crate::types::TransformCtx {
                    font_px: eff_style.font_size_px(
                        ctx.transform_ctx.root_font_px,
                        ctx.transform_ctx.root_font_px,
                    ),
                    ..ctx.transform_ctx
                },
            ),
        });
    }

    // Form elements (input, select, button, textarea, etc.) are rendered entirely
    // by the FormElement paint command. Skip CSS background/border/text to avoid
    // double rendering.
    let _is_form_element = matches!(
        node.tag.as_str(),
        "input" | "textarea" | "select" | "button" | "progress" | "meter"
    );

    let backdrop_filters = crate::css::parse_css_filter_with_current_color(
        &eff_style.rare().backdrop_filter,
        eff_style.color,
    );
    if !backdrop_filters.ops.is_empty() {
        list.push(PaintCmd::BackdropFilter {
            rect: Rect::new(px, py, pw, ph),
            filters: encode_filter_ops(&backdrop_filters),
        });
    }

    // ── CSS filters ───────────────────────────────────────────────────────────
    let has_filter = !eff_style.css_filter.ops.is_empty();
    if has_filter {
        let filters = encode_filter_ops(&eff_style.css_filter);
        list.push(PaintCmd::PushFilter { filters });
    }

    let clip_path_rect = clip_path_rect(eff_style, node.layout.border_rect, sx, sy, font_px);
    let clip_path_polygon =
        clip_path_polygon_points(eff_style, node.layout.border_rect, sx, sy, font_px);
    if let Some((rect, radius)) = clip_path_rect {
        list.push(PaintCmd::PushClip {
            rect,
            radius,
            radius_y: radius,
        });
    } else if let Some(points) = clip_path_polygon.as_ref() {
        list.push(PaintCmd::PushClipPath {
            points: points.clone(),
        });
    }

    // ── (a) Outer box-shadow ─────────────────────────────────────────────────
    for bs in &eff_style.box_shadow {
        if !bs.inset {
            list.push(PaintCmd::BoxShadow {
                rect: Rect::new(px, py, pw, ph),
                color: bs.color,
                offset_x: bs.offset_x,
                offset_y: bs.offset_y,
                blur: bs.blur,
                spread: bs.spread,
                inset: false,
                radii: radii_arr,
                radii_y: radii_y_arr,
            });
        }
    }

    let has_mask_layer = node.mask_image_data.is_some()
        && node.mask_image_width > 0
        && node.mask_image_height > 0
        && pw > 0.0
        && ph > 0.0;
    if has_mask_layer {
        if let Some(mask_data) = node.mask_image_data.as_ref() {
            list.push(PaintCmd::PushMask {
                rect: Rect::new(px, py, pw, ph),
                data: ImageRef::Shared(
                    mask_data.clone(),
                    node.mask_image_width,
                    node.mask_image_height,
                ),
            });
        }
    }

    // Form elements: CSS background/border/padding renders normally (below).
    // The FormElement command only draws the control CONTENT (value text,
    // placeholder, checkbox mark, radio dot, etc.) — not the box decoration.

    // ── Background painting and positioning areas ────────────────────────────
    // `background-clip` bounds what is PAINTED — initially the border box, so a
    // background bleeds under a transparent or gapped border instead of stopping
    // at the padding edge. `background-origin` is the area an image or gradient
    // is POSITIONED and sized in — initially the padding box. The two are
    // different boxes and must be resolved separately (css-backgrounds-3 §3.6,
    // §3.7).
    let background_box = |which: BackgroundClip| -> Rect {
        match which {
            BackgroundClip::ContentBox => {
                let c = node.layout.content_rect;
                Rect::new(c.x - eff_sx, c.y - eff_sy, c.w, c.h)
            }
            // `text` clips to the glyphs themselves, which needs a glyph mask
            // the painter does not build. The padding box is the smallest area
            // that always lies inside the intended one, so an unsupported
            // `text` clip paints no more than it does without the property.
            BackgroundClip::PaddingBox | BackgroundClip::Text => Rect::new(px, py, pw, ph),
            BackgroundClip::BorderBox => Rect::new(br.x - eff_sx, br.y - eff_sy, br.w, br.h),
        }
    };
    let bg_clip_rect = background_box(eff_style.background_clip);
    let bg_origin_rect = background_box(eff_style.background_origin);
    // `background-repeat` decides, per axis, whether the image (or gradient)
    // tiles out of the positioning area to cover the painting area.
    let (bg_repeat_x_mode, bg_repeat_y_mode) = node.style.background_repeat.axis_modes();

    // ── (b) Background color (opacity applied to alpha) ──────────────────────
    {
        let raw_bg = eff_style.background_color;
        let opacity = eff_style.opacity;
        if raw_bg.a > 0 {
            let alpha = ((raw_bg.a as f32) * opacity) as u8;
            let bg = Color::rgba(raw_bg.r, raw_bg.g, raw_bg.b, alpha);
            list.push(PaintCmd::FillRect {
                // A colour has no image to position, so `background-origin`
                // does not apply to it — only the painting area does.
                rect: bg_clip_rect,
                color: bg,
                radius: radii_arr,
                radius_y: radii_y_arr,
            });
        }
    }

    // ── (c) Gradient background ──────────────────────────────────────────────
    if node.style.gradient_type != GradientType::None && node.style.rare().gradient_stops.len() >= 2
    {
        let opacity = eff_style.opacity;
        let grad_type_u8 = match node.style.gradient_type {
            GradientType::Linear => 1u8,
            GradientType::Radial => 2u8,
            GradientType::None => 0u8,
        };
        let stops: Vec<(Color, f32)> = node
            .style
            .rare()
            .gradient_stops
            .iter()
            .map(|s| {
                let a = ((s.color.a as f32) * opacity) as u8;
                (Color::rgba(s.color.r, s.color.g, s.color.b, a), s.position)
            })
            .collect();
        let radial_center_x = node.style.gradient_radial_position_x.resolve(
            font_px,
            bg_origin_rect.w,
            ctx.transform_ctx.root_font_px,
        );
        let radial_center_y = node.style.gradient_radial_position_y.resolve(
            font_px,
            bg_origin_rect.h,
            ctx.transform_ctx.root_font_px,
        );
        let (radial_radius_x, radial_radius_y) = radial_gradient_used_radii(
            &node.style,
            bg_origin_rect.w,
            bg_origin_rect.h,
            radial_center_x,
            radial_center_y,
            font_px,
            ctx.transform_ctx.root_font_px,
        );
        list.push(PaintCmd::Gradient {
            rect: bg_origin_rect,
            clip: bg_clip_rect,
            repeat_x_mode: bg_repeat_x_mode,
            repeat_y_mode: bg_repeat_y_mode,
            gradient_type: grad_type_u8,
            angle: node.style.gradient_angle,
            direction: node.style.gradient_direction,
            radial_center_x,
            radial_center_y,
            radial_radius_x,
            radial_radius_y,
            stops,
            radii: radii_arr,
            radii_y: radii_y_arr,
            opacity,
            blend_mode: background_blend_mode_to_u8(&eff_style.background_blend_mode),
        });
    }

    // ── (d) Background image ─────────────────────────────────────────────────
    if let Some(ref bg_data) = node.bg_image_data {
        if node.bg_image_width > 0 && node.bg_image_height > 0 {
            let iw = node.bg_image_width as f32;
            let ih = node.bg_image_height as f32;
            // An image is sized and placed in the POSITIONING area
            // (`background-origin`), then clipped to the painting area.
            let ow = bg_origin_rect.w;
            let oh = bg_origin_rect.h;

            // Compute drawn image dimensions based on background-size
            let (draw_w, draw_h) = match node.style.background_size {
                BackgroundSize::Cover => {
                    let scale = (ow / iw).max(oh / ih);
                    (iw * scale, ih * scale)
                }
                BackgroundSize::Contain => {
                    let scale = (ow / iw).min(oh / ih);
                    (iw * scale, ih * scale)
                }
                BackgroundSize::Explicit => {
                    let w_auto = node.style.background_size_w.is_auto();
                    let h_auto = node.style.background_size_h.is_auto();
                    let explicit_w =
                        (!w_auto).then(|| node.style.background_size_w.resolve(font_px, ow, 16.0));
                    let explicit_h =
                        (!h_auto).then(|| node.style.background_size_h.resolve(font_px, oh, 16.0));
                    let (w, h) = match (explicit_w, explicit_h) {
                        (Some(w), Some(h)) => (w, h),
                        (Some(w), None) => (w, w * ih / iw),
                        (None, Some(h)) => (h * iw / ih, h),
                        (None, None) => (iw, ih),
                    };
                    (w, h)
                }
                BackgroundSize::Auto => (iw, ih),
            };

            let pos_x = bg_origin_rect.x
                + node
                    .style
                    .background_position_x
                    .resolve(font_px, ow - draw_w, 16.0);
            let pos_y = bg_origin_rect.y
                + node
                    .style
                    .background_position_y
                    .resolve(font_px, oh - draw_h, 16.0);

            let size_mode = match node.style.background_size {
                BackgroundSize::Auto => 0u8,
                BackgroundSize::Cover => 1,
                BackgroundSize::Contain => 2,
                BackgroundSize::Explicit => 3,
            };

            list.push(PaintCmd::BackgroundImage {
                container: bg_origin_rect,
                clip: bg_clip_rect,
                data: ImageRef::Shared(bg_data.clone(), node.bg_image_width, node.bg_image_height),
                size_mode,
                draw_w,
                draw_h,
                pos_x,
                pos_y,
                repeat_x_mode: bg_repeat_x_mode,
                repeat_y_mode: bg_repeat_y_mode,
                radii: radii_arr,
                radii_y: radii_y_arr,
                blend_mode: background_blend_mode_to_u8(&eff_style.background_blend_mode),
            });
        }
    }

    // ── (e) Inset box-shadow ─────────────────────────────────────────────────
    for bs in &eff_style.box_shadow {
        if bs.inset {
            list.push(PaintCmd::BoxShadow {
                rect: Rect::new(px, py, pw, ph),
                color: bs.color,
                offset_x: bs.offset_x,
                offset_y: bs.offset_y,
                blur: bs.blur,
                spread: bs.spread,
                inset: true,
                radii: radii_arr,
                radii_y: radii_y_arr,
            });
        }
    }

    // ── (f) Borders ──────────────────────────────────────────────────────────
    // render_box calls draw_borders_masked with eff_sx, eff_sy
    {
        let bw = [
            node.layout.resolved_border_top,
            node.layout.resolved_border_right,
            node.layout.resolved_border_bottom,
            node.layout.resolved_border_left,
        ];
        if bw.iter().any(|&w| w > 0.0) {
            let bx = br.x - eff_sx;
            let by = br.y - eff_sy;
            let border_rect = Rect::new(bx, by, br.w, br.h);
            let mut painted_border_image = false;
            if let Some(src) = crate::css::extract_url(&eff_style.border_image_source) {
                if let Some((data, w, h)) = crate::html::load_image_from_src(&src, ctx.base_url) {
                    if w > 0 && h > 0 {
                        list.push(PaintCmd::BorderImage {
                            rect: border_rect,
                            widths: bw,
                            slices: border_image_slices(
                                &eff_style.border_image_slice,
                                w as f32,
                                h as f32,
                            ),
                            fill_center: eff_style
                                .border_image_slice
                                .split_whitespace()
                                .any(|part| part.eq_ignore_ascii_case("fill")),
                            data: ImageRef::Owned(data, w, h),
                        });
                        painted_border_image = true;
                    }
                }
            }
            if !painted_border_image {
                list.push(PaintCmd::Border {
                    rect: border_rect,
                    widths: bw,
                    colors: [
                        eff_style.border_top_color,
                        eff_style.border_right_color,
                        eff_style.border_bottom_color,
                        eff_style.border_left_color,
                    ],
                    styles: [
                        bstyle(eff_style.border_top_style),
                        bstyle(eff_style.border_right_style),
                        bstyle(eff_style.border_bottom_style),
                        bstyle(eff_style.border_left_style),
                    ],
                    radii: radii_arr,
                    radii_y: radii_y_arr,
                });
            }
        }
    }

    // ── (g) Outline ──────────────────────────────────────────────────────────
    if eff_style.outline_width > 0.0 && eff_style.outline_style != crate::types::BorderStyle::None {
        let ofs = eff_style.outline_offset;
        let ow = eff_style.outline_width;
        let rx = br.x - eff_sx - ofs - ow;
        let ry = br.y - eff_sy - ofs - ow;
        let rw = br.w + 2.0 * (ofs + ow);
        let rh = br.h + 2.0 * (ofs + ow);
        list.push(PaintCmd::Outline {
            rect: Rect::new(rx, ry, rw, rh),
            width: ow,
            color: eff_style.outline_color,
            style: bstyle(eff_style.outline_style),
            offset: ofs,
        });
    }

    // ── (h) Overflow clip setup ──────────────────────────────────────────────
    let overflow_clips = eff_style.contain_paint
        || matches!(
            node.style.overflow_x,
            Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
        )
        || matches!(
            node.style.overflow_y,
            Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
        );
    let overflow_clip_margin =
        resolve_overflow_clip_margin(eff_style, font_px, node.layout.padding_rect.w);
    let overflow_clip_rect = Rect::new(
        px - overflow_clip_margin,
        py - overflow_clip_margin,
        pw + 2.0 * overflow_clip_margin,
        ph + 2.0 * overflow_clip_margin,
    );
    if overflow_clips {
        list.push(PaintCmd::PushClip {
            rect: overflow_clip_rect,
            radius: radii_arr,
            radius_y: radii_y_arr,
        });
    }

    // Tighter clip rect for children when overflow is clipping
    let child_clip = if overflow_clips {
        let cx1 = overflow_clip_rect.x.max(ctx.clip.x);
        let cy1 = overflow_clip_rect.y.max(ctx.clip.y);
        let cx2 = overflow_clip_rect.right().min(ctx.clip.right());
        let cy2 = overflow_clip_rect.bottom().min(ctx.clip.bottom());
        Rect::new(cx1, cy1, (cx2 - cx1).max(0.0), (cy2 - cy1).max(0.0))
    } else {
        ctx.clip
    };

    // Per-element scroll: children are shifted by the element's scroll
    let child_sx = eff_sx + node.layout.scroll_left;
    let child_sy = eff_sy + node.layout.scroll_top;

    let child_ctx = BuildContext {
        scroll_x: child_sx,
        scroll_y: child_sy,
        sticky_scroll_x: ctx.sticky_scroll_x,
        sticky_scroll_y: ctx.sticky_scroll_y,
        hovered_id: ctx.hovered_id,
        active_id: ctx.active_id,
        visited_hrefs: ctx.visited_hrefs,
        base_url: ctx.base_url,
        clip: child_clip,
        suppress_deferred_z_descendants: ctx.suppress_deferred_z_descendants,
        transform_ctx: ctx.transform_ctx,
    };

    let contents_visible = !matches!(eff_style.content_visibility, ContentVisibility::Hidden);
    if contents_visible {
        // ── (i) Negative z-index children (paint behind text) ────────────────
        {
            let eff_children = node.effective_children();
            let mut negative_z = Vec::new();
            for child in eff_children {
                collect_explicit_z_descendants(child, &mut negative_z);
            }
            negative_z.retain(|c| c.style.z_index < 0);
            negative_z.sort_by_key(|c| c.style.z_index);
            for child in negative_z {
                build_for_box(child, list, &child_ctx);
            }
        }

        // ── (j) ::before pseudo-element (inline text content) ────────────────
        if !node.style.before_content.is_empty() && !node.layout.line_cache.is_empty() {
            let first = &node.layout.line_cache[0];
            let tx = first.x - eff_sx;
            let ty = first.y - eff_sy;
            let ps = node.style.before_style.as_deref().unwrap_or(&node.style);
            let ps_font_px = {
                let f = ps.font_size.resolve(font_px, 0.0, 16.0);
                if f > 0.0 {
                    f
                } else {
                    font_px
                }
            };
            let line_h = ps
                .line_height
                .resolve(ps_font_px, 0.0, 16.0)
                .max(ps_font_px * 1.2);
            emit_text(
                list,
                tx,
                ty,
                &node.style.before_content,
                ps,
                ps_font_px,
                line_h,
            );
        }

        // ── (k) Inline text content (line_cache) ─────────────────────────────
        if !node.layout.line_cache.is_empty() {
            build_inline_text(
                node, eff_style, list, child_sx, child_sy, is_hovered, is_active,
            );
        }

        // ── (l) ::after pseudo-element ───────────────────────────────────────
        if !node.style.after_content.is_empty() && !node.layout.line_cache.is_empty() {
            let last = &node.layout.line_cache[node.layout.line_cache.len() - 1];
            let tx = last.x - eff_sx + last.width;
            let ty = last.y - eff_sy;
            let ps = node.style.after_style.as_deref().unwrap_or(&node.style);
            let ps_font_px = {
                let f = ps.font_size.resolve(font_px, 0.0, 16.0);
                if f > 0.0 {
                    f
                } else {
                    font_px
                }
            };
            let line_h = ps
                .line_height
                .resolve(ps_font_px, 0.0, 16.0)
                .max(ps_font_px * 1.2);
            emit_text(
                list,
                tx,
                ty,
                &node.style.after_content,
                ps,
                ps_font_px,
                line_h,
            );
        }

        // ── (m) List markers ─────────────────────────────────────────────────
        if node.style.display == Display::ListItem && !node.layout.line_cache.is_empty() {
            build_list_marker(node, list, ctx, eff_sx, eff_sy);
        }

        // ── (n) HR ───────────────────────────────────────────────────────────
        if node.tag == "hr" {
            let cr = node.layout.border_rect;
            let y_hr = cr.y + cr.h / 2.0 - eff_sy;
            list.push(PaintCmd::HorizontalRule {
                x1: cr.x - eff_sx,
                y1: y_hr,
                x2: cr.right() - eff_sx,
            });
        }

        // ── (o) Form elements (content only — box decoration handled by CSS steps above)
        build_form_element(node, list, eff_sx, eff_sy);

        // ── (p) Image / SVG / Canvas ─────────────────────────────────────────
        //
        // `<canvas>` joins `<img>` here because by this point it IS one: a canvas
        // keeps its bitmap in `image_data` exactly as a decoded image does, and
        // both are premultiplied RGBA, which is what `PixmapRef::from_bytes` at
        // replay reads. Everything the 2D context drew is already in those bytes,
        // so painting a canvas is painting its bitmap and nothing else — which is
        // also what the spec says a canvas is.
        if node.is_image_element() || node.tag == "svg" || node.tag == "canvas" {
            if let Some(ref data) = node.image_data {
                if node.image_width > 0 && node.image_height > 0 {
                    let cr = node.layout.content_rect;
                    let (dst, clip) = object_fit_rect(
                        &node.style,
                        cr,
                        node.image_width as f32,
                        node.image_height as f32,
                        font_px,
                        16.0,
                    );
                    let clips_radius =
                        radii_arr.iter().any(|r| *r > 0.5) || radii_y_arr.iter().any(|r| *r > 0.5);
                    if clip || clips_radius {
                        list.push(PaintCmd::PushClip {
                            rect: Rect::new(cr.x - eff_sx, cr.y - eff_sy, cr.w, cr.h),
                            radius: if clips_radius { radii_arr } else { [0.0; 4] },
                            radius_y: if clips_radius { radii_y_arr } else { [0.0; 4] },
                        });
                    }
                    list.push(PaintCmd::Image {
                        rect: Rect::new(dst.x - eff_sx, dst.y - eff_sy, dst.w, dst.h),
                        data: ImageRef::Shared(data.clone(), node.image_width, node.image_height),
                    });
                    if clip || clips_radius {
                        list.push(PaintCmd::PopClip);
                    }
                }
            } else if node.tag == "svg" || (node.is_image_element() && node.svg_markup.is_some()) {
                // SVG: rasterize from svg_markup on demand (inline <svg> or <img src="*.svg">)
                if let Some(ref markup) = node.svg_markup {
                    let cr = node.layout.content_rect;
                    if cr.w > 0.0 && cr.h > 0.0 {
                        let raster_w = cr.w.round() as u32;
                        let raster_h = cr.h.round() as u32;
                        if raster_w > 0 && raster_h > 0 {
                            // Inject inherited CSS color for currentColor support
                            let c = node.style.color;
                            let color_hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
                            let colored = crate::svg::prepare_svg_for_rasterization(
                                markup,
                                &color_hex,
                                node.style.svg_fill,
                                node.style.svg_stroke,
                            );
                            if let Some(rgba) =
                                crate::svg::rasterize_svg_to_rgba(&colored, raster_w, raster_h)
                            {
                                let clips_radius = radii_arr.iter().any(|r| *r > 0.5)
                                    || radii_y_arr.iter().any(|r| *r > 0.5);
                                if clips_radius {
                                    list.push(PaintCmd::PushClip {
                                        rect: Rect::new(cr.x - eff_sx, cr.y - eff_sy, cr.w, cr.h),
                                        radius: radii_arr,
                                        radius_y: radii_y_arr,
                                    });
                                }
                                list.push(PaintCmd::Image {
                                    rect: Rect::new(cr.x - eff_sx, cr.y - eff_sy, cr.w, cr.h),
                                    data: ImageRef::Owned(rgba, raster_w, raster_h),
                                });
                                if clips_radius {
                                    list.push(PaintCmd::PopClip);
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── (q) Children: normal flow, then explicit z-index descendants ────
        // Skip ::before/::after (handled as inline text in steps j/l above).
        // Skip position:fixed (rendered in separate overlay pass).
        {
            let eff_children = node.effective_children();
            let is_renderable = |c: &WebCore| -> bool {
                !matches!(c.style.display, Display::None)
                    && (c.tag != "#text" || is_laid_out_text_node(c))
                    && c.tag != "::before"
                    && c.tag != "::after"
                    && c.style.position != Position::Fixed
            };

            let mut deferred_z: Vec<&WebCore> = Vec::new();
            for child in eff_children {
                collect_explicit_z_descendants(child, &mut deferred_z);
            }

            let mut normal_ctx = child_ctx;
            normal_ctx.suppress_deferred_z_descendants = !deferred_z.is_empty();

            for child in eff_children {
                if is_renderable(child) {
                    build_for_box(child, list, &normal_ctx);
                }
            }

            deferred_z.retain(|c| is_renderable(c) && c.style.z_index >= 0);
            deferred_z.sort_by_key(|c| c.style.z_index);
            for child in deferred_z {
                build_for_box(child, list, &child_ctx);
            }
        }
    }

    build_element_scrollbar(node, eff_style, list, eff_sx, eff_sy);

    if eff_style.resize != Resize::None
        && !matches!(
            (eff_style.overflow_x, eff_style.overflow_y),
            (Overflow::Visible, Overflow::Visible)
        )
    {
        let mode = match eff_style.resize {
            Resize::Both => 1,
            Resize::Horizontal => 2,
            Resize::Vertical => 3,
            Resize::None => 0,
        };
        list.push(PaintCmd::ResizeGrip {
            rect: Rect::new(px, py, pw, ph),
            color: Color::rgba(0, 0, 0, 120),
            mode,
        });
    }

    // ── Pop in reverse order ─────────────────────────────────────────────────
    if overflow_clips {
        list.push(PaintCmd::PopClip);
    }
    if clip_path_rect.is_some() || clip_path_polygon.is_some() {
        list.push(PaintCmd::PopClip);
    }
    if has_mask_layer {
        list.push(PaintCmd::PopMask);
    }
    if has_filter {
        list.push(PaintCmd::PopFilter);
    }
    if has_transform {
        list.push(PaintCmd::PopTransform);
    }
    if blend != 0 {
        list.push(PaintCmd::PopBlendMode);
    }
    if eff_style.opacity < 1.0 {
        list.push(PaintCmd::PopOpacity);
    }
    if stacking {
        list.push(PaintCmd::EndStackingContext);
    }
    // TODO: clip-path masks — no PaintCmd variant yet
}

fn build_element_scrollbar(
    node: &WebCore,
    style: &ComputedStyle,
    list: &mut DisplayList,
    sx: f32,
    sy: f32,
) {
    let cr = node.layout.content_rect;
    let pr = node.layout.padding_rect;
    let scrollbar_w = style.scrollbar_width_px();
    let show_vertical = matches!(style.overflow_y, Overflow::Scroll)
        || (matches!(style.overflow_y, Overflow::Auto) && node.layout.scroll_height > cr.h);
    let show_horizontal = matches!(style.overflow_x, Overflow::Scroll)
        || (matches!(style.overflow_x, Overflow::Auto) && node.layout.scroll_width > cr.w);

    if scrollbar_w <= 0.0 {
        return;
    }

    let thumb_col = style
        .scrollbar_thumb_color
        .unwrap_or(Color::rgba(128, 128, 128, 160));
    let track_col = style
        .scrollbar_track_color
        .unwrap_or(Color::rgba(128, 128, 128, 40));

    if show_vertical && node.layout.scroll_height > cr.h {
        let track_h = cr.h.max(0.0);
        if track_h > 0.0 {
            let thumb_h = (track_h * track_h / node.layout.scroll_height)
                .max(20.0)
                .min(track_h);
            let max_scroll = (node.layout.scroll_height - cr.h).max(0.0);
            let thumb_y = if max_scroll > 0.0 && track_h > thumb_h {
                node.layout.scroll_top * (track_h - thumb_h) / max_scroll
            } else {
                0.0
            };
            let track_x = pr.x - sx + pr.w - scrollbar_w;
            let track_y = cr.y - sy;

            list.push(PaintCmd::FillRect {
                rect: Rect::new(track_x, track_y, scrollbar_w, track_h),
                color: track_col,
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            });
            list.push(PaintCmd::FillRect {
                rect: Rect::new(
                    track_x + 1.0,
                    track_y + thumb_y + 1.0,
                    (scrollbar_w - 2.0).max(1.0),
                    (thumb_h - 2.0).max(1.0),
                ),
                color: thumb_col,
                radius: [3.0; 4],
                radius_y: [3.0; 4],
            });
        }
    }

    if show_horizontal && node.layout.scroll_width > cr.w {
        let track_w = (cr.w
            - if show_vertical && node.layout.scroll_height > cr.h {
                scrollbar_w
            } else {
                0.0
            })
        .max(0.0);
        if track_w > 0.0 {
            let thumb_w = (track_w * cr.w / node.layout.scroll_width)
                .max(20.0)
                .min(track_w);
            let max_scroll = (node.layout.scroll_width - cr.w).max(0.0);
            let thumb_x = if max_scroll > 0.0 && track_w > thumb_w {
                node.layout.scroll_left * (track_w - thumb_w) / max_scroll
            } else {
                0.0
            };
            let track_x = cr.x - sx;
            let track_y = pr.y - sy + pr.h - scrollbar_w;

            list.push(PaintCmd::FillRect {
                rect: Rect::new(track_x, track_y, track_w, scrollbar_w),
                color: track_col,
                radius: [0.0; 4],
                radius_y: [0.0; 4],
            });
            list.push(PaintCmd::FillRect {
                rect: Rect::new(
                    track_x + thumb_x + 1.0,
                    track_y + 1.0,
                    (thumb_w - 2.0).max(1.0),
                    (scrollbar_w - 2.0).max(1.0),
                ),
                color: thumb_col,
                radius: [3.0; 4],
                radius_y: [3.0; 4],
            });
        }
    }
}

fn radial_gradient_used_radii(
    style: &ComputedStyle,
    w: f32,
    h: f32,
    cx: f32,
    cy: f32,
    font_px: f32,
    root_font_px: f32,
) -> (f32, f32) {
    if !style.gradient_radial_radius_x.is_auto() {
        let rx = style
            .gradient_radial_radius_x
            .resolve(font_px, w, root_font_px);
        let ry = style
            .gradient_radial_radius_y
            .resolve(font_px, h, root_font_px);
        return match style.gradient_radial_shape {
            GradientRadialShape::Circle => {
                let r = rx.max(ry).max(1.0);
                (r, r)
            }
            GradientRadialShape::Ellipse => (rx.max(1.0), ry.max(1.0)),
        };
    }
    let left = cx.max(0.0);
    let right = (w - cx).max(0.0);
    let top = cy.max(0.0);
    let bottom = (h - cy).max(0.0);
    let (rx, ry) = match style.gradient_radial_size {
        GradientRadialSize::ClosestSide => (left.min(right), top.min(bottom)),
        GradientRadialSize::FarthestSide => (left.max(right), top.max(bottom)),
        GradientRadialSize::ClosestCorner => {
            let r = left.min(right).hypot(top.min(bottom));
            (r, r)
        }
        GradientRadialSize::FarthestCorner => match style.gradient_radial_shape {
            GradientRadialShape::Circle => {
                let r = left.max(right).hypot(top.max(bottom));
                (r, r)
            }
            GradientRadialShape::Ellipse => (left.max(right), top.max(bottom)),
        },
    };
    match style.gradient_radial_shape {
        GradientRadialShape::Circle => {
            let r = rx.max(ry).max(1.0);
            (r, r)
        }
        GradientRadialShape::Ellipse => (rx.max(1.0), ry.max(1.0)),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Inline text using EXACT layout positions
// ═══════════════════════════════════════════════════════════════════════════════

fn build_inline_text(
    node: &WebCore,
    eff_style: &ComputedStyle,
    list: &mut DisplayList,
    sx: f32,
    sy: f32,
    is_hovered: bool,
    is_active: bool,
) {
    let mut flat = String::new();
    collect_flat_text(node, &mut flat);
    if flat.is_empty() {
        return;
    }

    let opacity = eff_style.opacity;
    let fallback_font_px = node.style.font_size_px(16.0, 16.0).max(1.0);
    let fallback_letter_spc = node
        .style
        .letter_spacing
        .resolve(fallback_font_px, 0.0, 16.0);
    let fallback_word_spc = node.style.word_spacing.resolve(fallback_font_px, 0.0, 16.0);

    // When overflow is clipping and text-indent pushes content far outside the
    // element's box, skip all text rendering — the clip would hide it anyway and
    // this avoids emitting paint commands for offscreen text.
    let overflow_clips = matches!(
        node.style.overflow_x,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    ) || matches!(
        node.style.overflow_y,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    );
    if overflow_clips {
        let ti = node
            .style
            .text_indent
            .resolve(fallback_font_px, node.layout.content_rect.w, 16.0);
        if ti < -(node.layout.content_rect.w + 100.0) {
            return;
        }
    }

    for line in &node.layout.line_cache {
        let line_start = floor_cb(&flat, line.text_start.min(flat.len()));
        let line_end = floor_cb(&flat, (line.text_start + line.text_length).min(flat.len()));
        if line_start >= line_end {
            continue;
        }
        if flat[line_start..line_end].trim().is_empty() {
            continue;
        }

        // EXACT positions from layout
        let lx = line.x - sx;
        let ly = line.y - sy;

        // ── Build chunks from inline_runs / visual_segments ──────────────
        struct Chunk {
            s: usize,
            e: usize,
            run_idx: Option<usize>,
            rtl: bool,
        }
        let mut chunks: Vec<Chunk> = Vec::new();

        if !line.visual_segments.is_empty() && !node.layout.inline_runs.is_empty() {
            // BiDi: use visual segment order
            for vs in &line.visual_segments {
                let seg_s = vs.logical_start;
                let seg_e = vs.logical_start + vs.length;
                let is_rtl = (vs.level & 1) != 0;
                let mut seg_chunks: Vec<Chunk> = Vec::new();
                for (ri, run) in node.layout.inline_runs.iter().enumerate() {
                    let rs = run.text_offset;
                    let re = rs + run.length;
                    let cs = seg_s.max(rs);
                    let ce = seg_e.min(re);
                    if cs < ce {
                        seg_chunks.push(Chunk {
                            s: cs,
                            e: ce,
                            run_idx: Some(ri),
                            rtl: is_rtl,
                        });
                    }
                }
                if is_rtl {
                    // Trim leading whitespace from last chunk (becomes visual-first after reversal)
                    if let Some(last) = seg_chunks.last_mut() {
                        while last.s < last.e
                            && last.s < flat.len()
                            && matches!(flat.as_bytes()[last.s], b' ' | b'\t' | b'\n' | b'\r')
                        {
                            last.s += 1;
                        }
                    }
                    seg_chunks.reverse();
                }
                chunks.extend(seg_chunks);
            }
        } else if node.layout.inline_runs.is_empty() {
            chunks.push(Chunk {
                s: line_start,
                e: line_end,
                run_idx: None,
                rtl: false,
            });
        } else {
            for (ri, run) in node.layout.inline_runs.iter().enumerate() {
                let cs = line_start.max(run.text_offset);
                let ce = line_end.min(run.text_offset + run.length);
                if cs < ce {
                    chunks.push(Chunk {
                        s: cs,
                        e: ce,
                        run_idx: Some(ri),
                        rtl: false,
                    });
                }
            }
        }

        let mut cursor_x = lx + line.text_x_offset;
        let mut previous_collapsible_space = false;

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let s = floor_cb(&flat, chunk.s);
            let e = floor_cb(&flat, chunk.e);
            if e <= s {
                continue;
            }

            let (run_style, run_font_px, run_letter_spc, run_word_spc, _run_extra) =
                if let Some(ri) = chunk.run_idx {
                    let run = &node.layout.inline_runs[ri];
                    let fp = run.style.font_size_px(16.0, 16.0).max(1.0);
                    let ls = run.style.letter_spacing.resolve(fp, 0.0, 16.0);
                    let ws = run.style.word_spacing.resolve(fp, 0.0, 16.0);
                    (Some(&run.style), fp, ls, ws, line.extra_space_per_word)
                } else {
                    (
                        None,
                        fallback_font_px,
                        fallback_letter_spc,
                        fallback_word_spc,
                        line.extra_space_per_word,
                    )
                };

            let style_ref: &ComputedStyle = run_style.unwrap_or(&node.style);
            let seg_text = &flat[s..e];

            // Normalize raw newlines to spaces
            let seg_text_clean: String;
            let seg_text_for_draw: &str = if seg_text.contains('\n') || seg_text.contains('\r') {
                seg_text_clean = seg_text
                    .chars()
                    .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
                    .collect();
                &seg_text_clean
            } else {
                seg_text
            };
            let collapsed_text;
            let text_for_transform = if collapses_spaces_for_paint(style_ref.white_space) {
                let (collapsed, ended_with_space) =
                    collapse_spaces_for_paint(seg_text_for_draw, previous_collapsible_space);
                previous_collapsible_space = ended_with_space;
                collapsed_text = collapsed;
                collapsed_text.as_str()
            } else {
                previous_collapsible_space = false;
                seg_text_for_draw
            };
            let mut draw_text = apply_text_transform(text_for_transform, style_ref.text_transform);
            if draw_text.is_empty() {
                continue;
            }

            let run_line_h = style_ref
                .line_height
                .resolve(run_font_px, 0.0, 16.0)
                .max(run_font_px * 1.2);

            // Use char_x for exact x position if available.
            // For RTL chunks, char_x byte offsets don't correspond to visual
            // position (logical byte 0 of Arabic maps to the rightmost glyph).
            // Use cursor_x instead, which advances in visual order.
            let x_pos = if chunk.rtl {
                cursor_x
            } else if !line.char_x.is_empty() {
                let char_offset = s - line_start;
                if char_offset < line.char_x.len() {
                    lx + line.text_x_offset + line.char_x[char_offset]
                } else {
                    cursor_x
                }
            } else {
                cursor_x
            };
            let y_pos = ly + vertical_align_y_shift(&style_ref.vertical_align, run_font_px);
            let is_final_chunk = chunk_idx + 1 == chunks.len();
            let line_clamp_marker = line.has_clamped_continuation && is_final_chunk;
            let overflow_marker = if line_clamp_marker {
                "…"
            } else {
                match node.style.text_overflow {
                    TextOverflow::Ellipsis if node.style.text_overflow_string.is_empty() => "…",
                    TextOverflow::Ellipsis => node.style.text_overflow_string.as_str(),
                    TextOverflow::Clip => "",
                }
            };
            if !overflow_marker.is_empty()
                && (overflow_clips || line_clamp_marker)
                && !chunk.rtl
                && (line.width > node.layout.content_rect.w || line_clamp_marker)
            {
                let content_right = node.layout.content_rect.x - sx + node.layout.content_rect.w;
                if x_pos >= content_right {
                    continue;
                }
                let available = (content_right - x_pos).max(0.0);
                let marker_width = crate::layout::inline_layout::measure_text_width(
                    overflow_marker,
                    run_font_px,
                    None,
                );
                let budget = (available - marker_width).max(0.0);
                let full_width =
                    crate::layout::inline_layout::measure_text_width(&draw_text, run_font_px, None);
                if line_clamp_marker && full_width <= budget {
                    draw_text.push_str(overflow_marker);
                } else {
                    let start_off = s - line_start;
                    let end_off = e - line_start;
                    if !line.char_x.is_empty()
                        && end_off < line.char_x.len()
                        && start_off < line.char_x.len()
                    {
                        let base_x = line.char_x[start_off];
                        let mut cut = s;
                        for (rel, ch) in flat[s..e].char_indices() {
                            let off = start_off + rel;
                            if off < line.char_x.len() && line.char_x[off] - base_x <= budget {
                                cut = s + rel + ch.len_utf8();
                            } else {
                                break;
                            }
                        }
                        if cut < e || line_clamp_marker {
                            let cut = floor_cb(&flat, cut);
                            draw_text = format!(
                                "{}{}",
                                apply_text_transform(&flat[s..cut], style_ref.text_transform),
                                overflow_marker
                            );
                        }
                    } else {
                        let mut cut = s;
                        for (rel, ch) in flat[s..e].char_indices() {
                            let next = s + rel + ch.len_utf8();
                            let candidate =
                                apply_text_transform(&flat[s..next], style_ref.text_transform);
                            let width = crate::layout::inline_layout::measure_text_width(
                                &candidate,
                                run_font_px,
                                None,
                            );
                            if width <= budget {
                                cut = next;
                            } else {
                                break;
                            }
                        }
                        if cut < e || line_clamp_marker {
                            draw_text = format!(
                                "{}{}",
                                apply_text_transform(&flat[s..cut], style_ref.text_transform),
                                overflow_marker
                            );
                        }
                    }
                }
            }

            // Run background color
            if style_ref.background_color.a > 0 {
                let run_w = if !line.char_x.is_empty() {
                    let start_off = s - line_start;
                    let end_off = e - line_start;
                    let x_start = if start_off < line.char_x.len() {
                        line.char_x[start_off]
                    } else {
                        0.0
                    };
                    let x_end = if end_off < line.char_x.len() {
                        line.char_x[end_off]
                    } else if !line.char_x.is_empty() {
                        *line.char_x.last().unwrap()
                    } else {
                        0.0
                    };
                    (x_end - x_start).abs()
                } else {
                    // Fallback only when no char_x
                    draw_text.len() as f32 * run_font_px * 0.6
                };
                list.push(PaintCmd::FillRect {
                    rect: Rect::new(x_pos, y_pos, run_w, line.height),
                    color: style_ref.background_color,
                    radius: [0.0; 4],
                    radius_y: [0.0; 4],
                });
            }

            // Text color: use effective style color when run inherits from node
            let run_color = if std::ptr::eq(style_ref as *const _, node.style.as_ref() as *const _)
                || ((is_hovered || is_active) && style_ref.color == node.style.color)
            {
                eff_style.color
            } else {
                style_ref.color
            };
            let alpha = ((run_color.a as f32) * opacity) as u8;
            let text_color = Color::rgba(run_color.r, run_color.g, run_color.b, alpha);

            // Text shadow
            if let Some(ref ts) = style_ref.text_shadow {
                list.push(PaintCmd::TextShadow {
                    x: x_pos + ts.offset_x,
                    y: y_pos + ts.offset_y,
                    text: draw_text.clone(),
                    font_family: style_ref.font_family.clone(),
                    font_size: run_font_px,
                    font_weight: style_ref.font_weight.value(),
                    font_style: match style_ref.font_style {
                        FontStyle::Italic => 1,
                        FontStyle::Oblique => 2,
                        _ => 0,
                    },
                    font_stretch: style_ref.font_stretch,
                    line_height: run_line_h,
                    color: ts.color,
                    blur: ts.blur,
                });
            }

            // Main text
            let effective_font_style = style_ref.font_style;
            let deco_t = style_ref
                .text_decoration_thickness
                .resolve(run_font_px, 0.0, 16.0);
            let underline_offset = style_ref
                .text_underline_offset
                .resolve(run_font_px, 0.0, 16.0);
            let letter_sp = run_letter_spc;

            emit_text_emphasis_marks(
                list,
                x_pos,
                y_pos,
                &draw_text,
                style_ref,
                run_font_px,
                run_line_h,
                text_color,
                letter_sp,
                run_word_spc,
            );

            list.push(PaintCmd::Text {
                x: x_pos,
                y: y_pos,
                text: draw_text.clone(),
                font_family: style_ref.font_family.clone(),
                font_size: run_font_px,
                font_weight: style_ref.font_weight.value(),
                font_style: match effective_font_style {
                    FontStyle::Italic => 1,
                    FontStyle::Oblique => 2,
                    _ => 0,
                },
                font_stretch: style_ref.font_stretch,
                line_height: run_line_h,
                color: text_color,
                decoration: TextDecoration {
                    underline: style_ref.text_decoration.underline,
                    overline: style_ref.text_decoration.overline,
                    strikethrough: style_ref.text_decoration.strikethrough,
                    color: style_ref.text_decoration_color.unwrap_or(text_color),
                    style: match style_ref.text_decoration_style {
                        TextDecorationStyle::Double => 1,
                        TextDecorationStyle::Dotted => 2,
                        TextDecorationStyle::Dashed => 3,
                        TextDecorationStyle::Wavy => 4,
                        _ => 0,
                    },
                    thickness: if deco_t > 0.0 {
                        deco_t
                    } else {
                        (run_font_px / 12.0).max(1.0)
                    },
                    underline_offset,
                    underline_position: style_ref.text_underline_position,
                    skip_ink: !style_ref
                        .text_decoration_skip_ink
                        .eq_ignore_ascii_case("none"),
                },
                letter_spacing: letter_sp,
                word_spacing: run_word_spc,
                small_caps: style_ref.small_caps,
            });

            // Advance cursor using char_x if available
            if chunk.rtl {
                // For RTL chunks, advance cursor_x by the visual segment width
                let start_off = s.saturating_sub(line_start);
                let end_off = e.saturating_sub(line_start);
                if start_off < line.char_x.len() && end_off < line.char_x.len() {
                    cursor_x += (line.char_x[end_off] - line.char_x[start_off]).abs();
                }
            } else if !line.char_x.is_empty() {
                let end_off = e - line_start;
                if end_off < line.char_x.len() {
                    cursor_x = lx + line.text_x_offset + line.char_x[end_off];
                }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// List marker
// ═══════════════════════════════════════════════════════════════════════════════

fn build_list_marker(
    node: &WebCore,
    list: &mut DisplayList,
    ctx: &BuildContext<'_>,
    sx: f32,
    sy: f32,
) {
    // Skip marker entirely when list-style-type is None
    if matches!(node.style.list_style_type, ListStyleType::None)
        && node.style.marker_content.is_empty()
        && node.style.list_style_image.is_empty()
    {
        return;
    }
    let ms = node.style.marker_style.as_deref();
    let font_px = ms
        .map(|s| s.font_size_px(16.0, 16.0))
        .unwrap_or_else(|| node.style.font_size_px(16.0, 16.0));
    let fallback_line_y = node.layout.content_rect.y;
    let fallback_line_h = node
        .style
        .line_height
        .resolve(font_px, font_px, 16.0)
        .max(font_px * 1.2);
    let (line_x, line_y, line_h) = match node.layout.line_cache.first() {
        Some(l) => (l.x, l.y, l.height),
        None => (node.layout.content_rect.x, fallback_line_y, fallback_line_h),
    };
    let inside = node.style.list_style_position == ListStylePosition::Inside;
    let c = ms.map(|s| s.color).unwrap_or(node.style.color);
    let marker_family = ms
        .map(|s| s.font_family.clone())
        .unwrap_or_else(|| node.style.font_family.clone());
    let marker_weight = ms
        .map(|s| s.font_weight)
        .unwrap_or(node.style.font_weight)
        .value();
    let marker_style = match ms.map(|s| s.font_style).unwrap_or(node.style.font_style) {
        FontStyle::Italic => 1,
        FontStyle::Oblique => 2,
        _ => 0,
    };
    let marker_line_height = ms
        .map(|s| {
            s.line_height
                .resolve(font_px, font_px, 16.0)
                .max(font_px * 1.2)
        })
        .unwrap_or_else(|| {
            node.style
                .line_height
                .resolve(font_px, font_px, 16.0)
                .max(font_px * 1.2)
        });
    if !node.style.marker_content.is_empty() {
        let mx = if inside {
            line_x - sx
        } else {
            line_x - sx - 4.0
        };
        list.push(PaintCmd::ListMarker {
            marker_type: 3,
            x: mx,
            y: line_y - sy,
            size: 0.0,
            color: c,
            text: node.style.marker_content.clone(),
            image: None,
            font_family: marker_family.clone(),
            font_size: font_px,
            font_weight: marker_weight,
            font_style: marker_style,
            line_height: marker_line_height,
        });
        return;
    }

    if !node.style.list_style_image.is_empty() {
        let mx = if inside {
            line_x - sx
        } else {
            line_x - sx - font_px
        };
        let image = crate::html::load_image_from_src(&node.style.list_style_image, ctx.base_url)
            .map(|(data, w, h)| ImageRef::Owned(data, w, h));
        list.push(PaintCmd::ListMarker {
            marker_type: 4,
            x: mx,
            y: line_y - sy,
            size: font_px,
            color: c,
            text: node.style.list_style_image.clone(),
            image,
            font_family: marker_family.clone(),
            font_size: font_px,
            font_weight: marker_weight,
            font_style: marker_style,
            line_height: fallback_line_h,
        });
        return;
    }

    match node.style.list_style_type {
        ListStyleType::Disc => {
            let bx = if inside {
                line_x - sx + 4.0
            } else {
                line_x - sx - 10.0
            };
            let by = line_y - sy + line_h / 2.0;
            list.push(PaintCmd::ListMarker {
                marker_type: 0,
                x: bx,
                y: by,
                size: 3.0,
                color: c,
                text: String::new(),
                image: None,
                font_family: marker_family.clone(),
                font_size: font_px,
                font_weight: marker_weight,
                font_style: marker_style,
                line_height: marker_line_height,
            });
        }
        ListStyleType::Circle => {
            let bx = if inside {
                line_x - sx + 4.0
            } else {
                line_x - sx - 10.0
            };
            let by = line_y - sy + line_h / 2.0;
            list.push(PaintCmd::ListMarker {
                marker_type: 1,
                x: bx,
                y: by,
                size: 3.0,
                color: c,
                text: String::new(),
                image: None,
                font_family: marker_family.clone(),
                font_size: font_px,
                font_weight: marker_weight,
                font_style: marker_style,
                line_height: marker_line_height,
            });
        }
        ListStyleType::Square => {
            let bx = if inside {
                line_x - sx + 4.0
            } else {
                line_x - sx - 10.0
            };
            let by = line_y - sy + line_h / 2.0;
            list.push(PaintCmd::ListMarker {
                marker_type: 2,
                x: bx,
                y: by,
                size: 6.0,
                color: c,
                text: String::new(),
                image: None,
                font_family: marker_family.clone(),
                font_size: font_px,
                font_weight: marker_weight,
                font_style: marker_style,
                line_height: marker_line_height,
            });
        }
        ListStyleType::Decimal
        | ListStyleType::DecimalLeadingZero
        | ListStyleType::LowerAlpha
        | ListStyleType::UpperAlpha
        | ListStyleType::LowerLatin
        | ListStyleType::UpperLatin
        | ListStyleType::LowerRoman
        | ListStyleType::UpperRoman
        | ListStyleType::LowerGreek
        | ListStyleType::Armenian
        | ListStyleType::Georgian
        | ListStyleType::Hebrew
        | ListStyleType::Hiragana
        | ListStyleType::Katakana
        | ListStyleType::HiraganaIroha
        | ListStyleType::KatakanaIroha
        | ListStyleType::CjkDecimal => {
            let marker = format_list_marker(node.style.list_style_type, node.style.list_index);
            let mx = if inside {
                line_x - sx
            } else {
                // marker_w not available here (we don't have font shaping).
                // Use an approximate offset that matches the render_box convention:
                // mx = first_line.x - sx - marker_w - 4.0
                // Since we can't measure, emit marker text and let replay handle positioning.
                line_x - sx - 4.0
            };
            let my = line_y - sy;
            list.push(PaintCmd::ListMarker {
                marker_type: 3,
                x: mx,
                y: my,
                size: 0.0,
                color: c,
                text: marker,
                image: None,
                font_family: marker_family.clone(),
                font_size: font_px,
                font_weight: marker_weight,
                font_style: marker_style,
                line_height: marker_line_height,
            });
        }
        ListStyleType::Disclosure => {
            let mx = if inside {
                line_x - sx
            } else {
                line_x - sx - 4.0
            };
            let my = line_y - sy;
            list.push(PaintCmd::ListMarker {
                marker_type: 3,
                x: mx,
                y: my,
                size: 0.0,
                color: c,
                text: "\u{25b8}".to_string(),
                image: None,
                font_family: marker_family.clone(),
                font_size: font_px,
                font_weight: marker_weight,
                font_style: marker_style,
                line_height: marker_line_height,
            });
        }
        ListStyleType::None => {}
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Form element
// ═══════════════════════════════════════════════════════════════════════════════

fn build_form_element(node: &WebCore, list: &mut DisplayList, sx: f32, sy: f32) {
    let tag = node.tag.as_str();
    let input_type = node
        .attributes
        .get("type")
        .map(|s| s.as_str())
        .unwrap_or("text");

    let is_form = match tag {
        "input" | "textarea" | "select" | "button" | "progress" | "meter" => true,
        _ => false,
    };
    if !is_form {
        return;
    }

    let cr = node.layout.content_rect;
    let font_px = node.style.font_size_px(16.0, 16.0).max(1.0);
    let value = if tag == "select" {
        // The shown text is "the label of an option of which selectedness is
        // set to true" (HTML §15.5.16) — SELECTEDNESS, not the `selected`
        // attribute, which is only the default. Reading the attribute meant a
        // drop-down the user had changed kept painting the author's choice.
        //
        // With nothing selected there is nothing to show, which is the normal
        // resting state of a list box. The old fallback to the first option
        // drew a label for a selection that did not exist.
        crate::html::forms::list_of_options(node)
            .iter()
            .find(|o| o.selectedness)
            .map(|opt| crate::html::forms::option_label(opt))
            .unwrap_or_default()
    } else {
        crate::types::input_value(node)
    };
    let placeholder = node
        .attributes
        .get("placeholder")
        .cloned()
        .unwrap_or_default();
    // What is PAINTED is the live checkedness, not the author's default —
    // otherwise a box the user ticked draws empty, and one they unticked keeps
    // its tick because the markup still says `checked`.
    let checked = node.checkedness;

    let attrs: Vec<(String, String)> = node
        .attributes
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // A `<select>` carries its option labels and which of them are selected: a
    // list box paints the options itself, and only the element knows them.
    //
    // `selected` is the FIRST selected option, for the single-select highlight;
    // `selected_all` is every one, which is what a `multiple` list box needs
    // and what a single index could never express.
    let (options, selected, selected_all) = if tag == "select" {
        let opts = crate::html::forms::list_of_options(node);
        let labels: Vec<String> = opts
            .iter()
            .map(|o| crate::html::forms::option_label(o))
            .collect();
        let all: Vec<bool> = opts.iter().map(|o| o.selectedness).collect();
        let first = all.iter().position(|s| *s).map(|i| i as i32).unwrap_or(-1);
        (labels, first, all)
    } else {
        (Vec::new(), -1, Vec::new())
    };

    list.push(PaintCmd::FormElement {
        tag: tag.to_string(),
        input_type: input_type.to_string(),
        rect: Rect::new(cr.x - sx, cr.y - sy, cr.w, cr.h),
        node_id: node.node_id,
        attributes: attrs,
        font_size: font_px,
        font_weight: node.style.font_weight.value(),
        font_family: node.style.font_family.clone(),
        color: node.style.color,
        placeholder_color: node
            .style
            .placeholder_style
            .as_ref()
            .map(|s| s.color)
            .unwrap_or_else(|| {
                let mut c = node.style.color;
                c.a = (c.a as f32 * 0.5) as u8;
                c
            }),
        checked,
        value,
        placeholder,
        input_cursor: node.input_cursor,
        appearance_none: node.style.appearance == "none",
        vertical: !matches!(
            node.style.writing_mode,
            crate::types::WritingMode::HorizontalTB
        ),
        options,
        selected,
        selected_all,
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn emit_text(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    text: &str,
    style: &ComputedStyle,
    font_px: f32,
    line_h: f32,
) {
    if text.is_empty() || font_px <= 0.0 {
        return;
    }
    let fp = font_px.max(1.0);
    let lh = if line_h > 0.0 { line_h } else { fp * 1.2 };
    let letter_sp = style.letter_spacing.resolve(fp, 0.0, 16.0);
    let word_sp = style.word_spacing.resolve(fp, 0.0, 16.0);
    let deco_t = style.text_decoration_thickness.resolve(fp, 0.0, 16.0);
    let underline_offset = style.text_underline_offset.resolve(fp, 0.0, 16.0);
    list.push(PaintCmd::Text {
        x,
        y,
        text: text.to_string(),
        font_family: style.font_family.clone(),
        font_size: fp,
        font_weight: style.font_weight.value(),
        font_style: match style.font_style {
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
            _ => 0,
        },
        font_stretch: style.font_stretch,
        line_height: lh,
        color: style.color,
        decoration: TextDecoration {
            underline: style.text_decoration.underline,
            overline: style.text_decoration.overline,
            strikethrough: style.text_decoration.strikethrough,
            color: style.text_decoration_color.unwrap_or(style.color),
            style: match style.text_decoration_style {
                TextDecorationStyle::Double => 1,
                TextDecorationStyle::Dotted => 2,
                TextDecorationStyle::Dashed => 3,
                TextDecorationStyle::Wavy => 4,
                _ => 0,
            },
            thickness: if deco_t > 0.0 { deco_t } else { 1.0 },
            underline_offset,
            underline_position: style.text_underline_position,
            skip_ink: !style.text_decoration_skip_ink.eq_ignore_ascii_case("none"),
        },
        letter_spacing: letter_sp,
        word_spacing: word_sp,
        small_caps: style.small_caps,
    });
}

fn is_laid_out_text_node(node: &WebCore) -> bool {
    node.tag == "#text"
        && !node.text.is_empty()
        && !node.text.chars().all(|c| c.is_ascii_whitespace())
        && node.layout.content_rect.w > 0.0
        && node.layout.content_rect.h > 0.0
}

fn resolve_overflow_clip_margin(style: &ComputedStyle, font_px: f32, reference: f32) -> f32 {
    style
        .overflow_clip_margin
        .split_whitespace()
        .find_map(crate::css::parse_length_checked)
        .map(|length| length.resolve(font_px, reference, 16.0).max(0.0))
        .unwrap_or(0.0)
}

fn clip_path_rect(
    style: &ComputedStyle,
    border_rect: Rect,
    scroll_x: f32,
    scroll_y: f32,
    font_px: f32,
) -> Option<(Rect, [f32; 4])> {
    match style.clip_path.kind {
        ClipPathKind::Inset => {
            let top = style
                .clip_path
                .inset_top
                .resolve(font_px, border_rect.h, 16.0);
            let right = style
                .clip_path
                .inset_right
                .resolve(font_px, border_rect.w, 16.0);
            let bottom = style
                .clip_path
                .inset_bottom
                .resolve(font_px, border_rect.h, 16.0);
            let left = style
                .clip_path
                .inset_left
                .resolve(font_px, border_rect.w, 16.0);
            let w = (border_rect.w - left - right).max(0.0);
            let h = (border_rect.h - top - bottom).max(0.0);
            Some((
                Rect::new(
                    border_rect.x + left - scroll_x,
                    border_rect.y + top - scroll_y,
                    w,
                    h,
                ),
                [0.0; 4],
            ))
        }
        ClipPathKind::Circle => {
            let reference = border_rect.w.min(border_rect.h);
            let r = style
                .clip_path
                .circle_radius
                .resolve(font_px, reference, 16.0)
                .max(0.0);
            let cx = border_rect.x
                + style
                    .clip_path
                    .center_x
                    .resolve(font_px, border_rect.w, 16.0);
            let cy = border_rect.y
                + style
                    .clip_path
                    .center_y
                    .resolve(font_px, border_rect.h, 16.0);
            Some((
                Rect::new(cx - r - scroll_x, cy - r - scroll_y, r * 2.0, r * 2.0),
                [r; 4],
            ))
        }
        ClipPathKind::Ellipse => {
            let rx = style
                .clip_path
                .ellipse_rx
                .resolve(font_px, border_rect.w, 16.0)
                .max(0.0);
            let ry = style
                .clip_path
                .ellipse_ry
                .resolve(font_px, border_rect.h, 16.0)
                .max(0.0);
            let cx = border_rect.x
                + style
                    .clip_path
                    .center_x
                    .resolve(font_px, border_rect.w, 16.0);
            let cy = border_rect.y
                + style
                    .clip_path
                    .center_y
                    .resolve(font_px, border_rect.h, 16.0);
            Some((
                Rect::new(cx - rx - scroll_x, cy - ry - scroll_y, rx * 2.0, ry * 2.0),
                [rx.min(ry); 4],
            ))
        }
        _ => None,
    }
}

fn clip_path_polygon_points(
    style: &ComputedStyle,
    border_rect: Rect,
    scroll_x: f32,
    scroll_y: f32,
    font_px: f32,
) -> Option<Vec<(f32, f32)>> {
    if style.clip_path.kind != ClipPathKind::Polygon || style.clip_path.points.len() < 3 {
        return None;
    }
    Some(
        style
            .clip_path
            .points
            .iter()
            .map(|(x, y)| {
                (
                    border_rect.x + x.resolve(font_px, border_rect.w, 16.0) - scroll_x,
                    border_rect.y + y.resolve(font_px, border_rect.h, 16.0) - scroll_y,
                )
            })
            .collect(),
    )
}

/// An `<option>`'s label — HTML §4.11.3.5: "the value of the option element's
/// `label` attribute, if there is one, or else the option element's DESCENDANT
/// TEXT CONTENT, with ASCII whitespace stripped and collapsed."
///
/// The descendant part is what was missing: this collected the option's DIRECT
/// `#text` children only, so a label wrapped in any element — which is what a
/// framework that nests a text widget produces — rendered as an empty entry.
/// The wrapper is invalid markup on the author's side and a browser still shows
/// the word, because the label is defined over descendants.
pub(crate) fn option_label(opt: &WebCore) -> String {
    if let Some(label) = opt.attributes.get("label") {
        return label.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    let mut text = String::new();
    descendant_text(opt, &mut text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every text node under `node`, in tree order — DOM's "descendant text
/// content". Unlike `collect_flat_text` this does NOT filter by `display`: the
/// label of an option is defined over the tree, not over what is rendered.
fn descendant_text(node: &WebCore, out: &mut String) {
    if node.tag == "#text" {
        out.push_str(&node.text);
    }
    for child in &node.children {
        descendant_text(child, out);
    }
}

fn collect_flat_text(node: &WebCore, out: &mut String) {
    let generated_content = node.style.rare().content.as_str();
    if node.tag == "#text" {
        if generated_content.is_empty() {
            out.push_str(&node.text);
        } else {
            out.push_str(generated_content);
        }
        return;
    }
    if !generated_content.is_empty() {
        out.push_str(generated_content);
        return;
    }
    for child in &node.children {
        if child.tag == "br" {
            out.push('\n');
        } else if matches!(child.style.display, Display::Inline | Display::None)
            || child.tag == "#text"
        {
            collect_flat_text(child, out);
        }
    }
}

fn subtree_has(node: &WebCore, id: u32) -> bool {
    if node.node_id == id {
        return true;
    }
    node.children.iter().any(|c| subtree_has(c, id))
}

fn floor_cb(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn bstyle(s: crate::types::BorderStyle) -> u8 {
    use crate::types::BorderStyle::*;
    match s {
        None => 0,
        Solid => 1,
        Dashed => 2,
        Dotted => 3,
        Double => 4,
        Groove => 5,
        Ridge => 6,
        Inset => 7,
        Outset => 8,
        _ => 0,
    }
}

fn border_image_slices(value: &str, image_w: f32, image_h: f32) -> [f32; 4] {
    let mut vals = Vec::new();
    for part in value.split_whitespace() {
        if part.eq_ignore_ascii_case("fill") {
            continue;
        }
        let p = part.trim();
        if let Some(raw) = p.strip_suffix('%') {
            if let Ok(percent) = raw.parse::<f32>() {
                vals.push((
                    image_h * percent / 100.0,
                    image_w * percent / 100.0,
                ));
            }
        } else if let Ok(px) = p.parse::<f32>() {
            vals.push((px, px));
        } else if let Some(px) = p.strip_suffix("px").and_then(|n| n.parse::<f32>().ok()) {
            vals.push((px, px));
        }
    }
    let pick = |idx: usize| vals.get(idx).copied();
    let top = pick(0).unwrap_or((image_h, image_w)).0;
    let right = pick(1).or_else(|| pick(0)).unwrap_or((image_h, image_w)).1;
    let bottom = pick(2).or_else(|| pick(0)).unwrap_or((image_h, image_w)).0;
    let left = pick(3).or_else(|| pick(1)).or_else(|| pick(0)).unwrap_or((image_h, image_w)).1;
    [
        top.clamp(0.0, image_h),
        right.clamp(0.0, image_w),
        bottom.clamp(0.0, image_h),
        left.clamp(0.0, image_w),
    ]
}

fn blend_mode_to_u8(m: MixBlendMode) -> u8 {
    use MixBlendMode::*;
    match m {
        Normal => 0,
        Multiply => 1,
        Screen => 2,
        Overlay => 3,
        Darken => 4,
        Lighten => 5,
        ColorDodge => 6,
        ColorBurn => 7,
        HardLight => 8,
        SoftLight => 9,
        Difference => 10,
        Exclusion => 11,
        Hue => 12,
        Saturation => 13,
        Color => 14,
        Luminosity => 15,
    }
}

fn background_blend_mode_to_u8(modes: &str) -> u8 {
    match modes.split(',').next().unwrap_or("normal").trim() {
        "multiply" => 1,
        "screen" => 2,
        "overlay" => 3,
        "darken" => 4,
        "lighten" => 5,
        "color-dodge" => 6,
        "color-burn" => 7,
        "hard-light" => 8,
        "soft-light" => 9,
        "difference" => 10,
        "exclusion" => 11,
        "hue" => 12,
        "saturation" => 13,
        "color" => 14,
        "luminosity" => 15,
        _ => 0,
    }
}

/// The 2D affine matrix a `transform` resolves to, as `[a, b, c, d, e, f]`.
///
/// Public because CSSOM asks the same question: `getComputedStyle().transform`
/// serializes exactly this matrix.
pub fn compute_transform_matrix(
    style: &ComputedStyle,
    rect: &Rect,
    ctx: &crate::types::TransformCtx,
) -> [f32; 6] {
    let [a, b, c, d, e, f] = compute_transform_matrix_raw(style, rect.w, rect.h, ctx);
    // css-transforms-1 §transform-rendering: the matrix is applied about the
    // transform origin — translate(origin) · M · translate(-origin).
    let half = crate::types::CssLength::Percent(50.0);
    let (ox_len, oy_len) = match &style.rare().transform_origin {
        Some((x, y)) => (x.clone(), y.clone()),
        None => (half.clone(), half),
    };
    let ox = rect.x + ox_len.resolve(ctx.font_px, rect.w, ctx.root_font_px);
    let oy = rect.y + oy_len.resolve(ctx.font_px, rect.h, ctx.root_font_px);
    [
        a,
        b,
        c,
        d,
        e + ox - (a * ox + c * oy),
        f + oy - (b * ox + d * oy),
    ]
}

/// The transformation matrix M itself, with NO `transform-origin` applied.
///
/// This is what CSSOM serializes for `getComputedStyle().transform`: the origin
/// is a separate property and is not baked into the reported matrix. Painting
/// wants the sandwiched form, which is `compute_transform_matrix`.
///
/// `ref_w`/`ref_h` are the reference box, which percentages in `translate()`
/// resolve against — width for the X component, height for the Y.
pub fn compute_transform_matrix_raw(
    style: &ComputedStyle,
    ref_w: f32,
    ref_h: f32,
    ctx: &crate::types::TransformCtx,
) -> [f32; 6] {
    use crate::types::TransformOp;
    let (mut a, mut b, mut c, mut d, mut e, mut f) =
        (1.0f32, 0.0f32, 0.0f32, 1.0f32, 0.0f32, 0.0f32);
    let rx = |l: &crate::types::CssLength| {
        l.resolve_vp(
            ctx.font_px,
            ref_w,
            ctx.root_font_px,
            ctx.viewport_w,
            ctx.viewport_h,
        )
    };
    let ry = |l: &crate::types::CssLength| {
        l.resolve_vp(
            ctx.font_px,
            ref_h,
            ctx.root_font_px,
            ctx.viewport_w,
            ctx.viewport_h,
        )
    };
    // ⛔ Each function POST-multiplies the matrix built so far — css-transforms-1
    // §transform-rendering step 3. `translate` used to do `e += tx; f += ty` and
    // `scale` `a *= sx; d *= sy`, both of which ignore the matrix to their left:
    // `rotate(90deg) translateX(100px)` moved the box RIGHT instead of DOWN, and
    // `rotate(45deg) scale(2)` scaled along the un-rotated axes and sheared it.
    for op in style
        .css_translate
        .ops
        .iter()
        .chain(style.css_rotate.ops.iter())
        .chain(style.css_scale.ops.iter())
        .chain(style.css_transform.ops.iter())
    {
        match op {
            TransformOp::Translate(tx, ty) => {
                let (tx, ty) = (rx(tx), ry(ty));
                e += a * tx + c * ty;
                f += b * tx + d * ty;
            }
            TransformOp::TranslateX(tx) => {
                let tx = rx(tx);
                e += a * tx;
                f += b * tx;
            }
            TransformOp::TranslateY(ty) => {
                let ty = ry(ty);
                e += c * ty;
                f += d * ty;
            }
            TransformOp::Scale(sx, sy) => {
                a *= sx;
                b *= sx;
                c *= sy;
                d *= sy;
            }
            TransformOp::ScaleX(sx) => {
                a *= sx;
                b *= sx;
            }
            TransformOp::ScaleY(sy) => {
                c *= sy;
                d *= sy;
            }
            TransformOp::Rotate(deg) => {
                let rad = deg * std::f32::consts::PI / 180.0;
                let (cos, sin) = (rad.cos(), rad.sin());
                let (na, nb) = (a * cos + c * sin, b * cos + d * sin);
                let (nc, nd) = (a * -sin + c * cos, b * -sin + d * cos);
                a = na;
                b = nb;
                c = nc;
                d = nd;
            }
            TransformOp::SkewX(deg) => {
                let t = (deg * std::f32::consts::PI / 180.0).tan();
                c += a * t;
                d += b * t;
            }
            TransformOp::SkewY(deg) => {
                let t = (deg * std::f32::consts::PI / 180.0).tan();
                a += c * t;
                b += d * t;
            }
            TransformOp::Matrix(m0, m1, m2, m3, m4, m5) => {
                let (na, nb, nc, nd) = (
                    a * m0 + c * m1,
                    b * m0 + d * m1,
                    a * m2 + c * m3,
                    b * m2 + d * m3,
                );
                let (ne, nf) = (a * m4 + c * m5 + e, b * m4 + d * m5 + f);
                a = na;
                b = nb;
                c = nc;
                d = nd;
                e = ne;
                f = nf;
            }
        }
    }
    [a, b, c, d, e, f]
}

fn apply_text_transform(text: &str, tt: TextTransform) -> String {
    match tt {
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => capitalize_words(text),
        TextTransform::FullWidth => text.chars().map(to_full_width_char).collect(),
        TextTransform::FullSizeKana | TextTransform::MathAuto => text.to_owned(),
        TextTransform::None => text.to_owned(),
    }
}

fn to_full_width_char(ch: char) -> char {
    match ch {
        ' ' => char::from_u32(0x3000).unwrap_or(ch),
        '!'..='~' => char::from_u32(ch as u32 - 0x21 + 0xff01).unwrap_or(ch),
        _ => ch,
    }
}

fn emit_text_emphasis_marks(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    text: &str,
    style: &ComputedStyle,
    font_px: f32,
    line_h: f32,
    fallback_color: Color,
    letter_spacing: f32,
    word_spacing: f32,
) {
    let Some(mark) = text_emphasis_mark(&style.text_emphasis_style) else {
        return;
    };
    let mark_count = text.chars().filter(|ch| !ch.is_whitespace()).count();
    if mark_count == 0 {
        return;
    }

    let mark_text = mark.repeat(mark_count);
    let mark_font_px = (font_px * 0.5).max(1.0);
    let mark_y = if style
        .text_emphasis_position
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("under"))
    {
        y + line_h * 0.55
    } else {
        y - font_px * 0.45
    };
    let color = style.text_emphasis_color.unwrap_or(fallback_color);

    list.push(PaintCmd::Text {
        x,
        y: mark_y,
        text: mark_text,
        font_family: style.font_family.clone(),
        font_size: mark_font_px,
        font_weight: style.font_weight.value(),
        font_style: match style.font_style {
            FontStyle::Italic => 1,
            FontStyle::Oblique => 2,
            _ => 0,
        },
        font_stretch: style.font_stretch,
        line_height: mark_font_px,
        color,
        decoration: TextDecoration {
            underline: false,
            overline: false,
            strikethrough: false,
            color,
            style: 0,
            thickness: 1.0,
            underline_offset: 0.0,
            underline_position: style.text_underline_position,
            skip_ink: true,
        },
        letter_spacing,
        word_spacing,
        small_caps: false,
    });
}

fn text_emphasis_mark(style: &str) -> Option<String> {
    let value = style.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(custom) = quoted_text(value) {
        return if custom.is_empty() {
            None
        } else {
            Some(custom.to_string())
        };
    }

    let open = value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("open"));
    let mark = if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("sesame"))
    {
        if open {
            0xfe46
        } else {
            0xfe45
        }
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("double-circle"))
    {
        0x25ce
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("triangle"))
    {
        if open {
            0x25b3
        } else {
            0x25b2
        }
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("circle"))
    {
        if open {
            0x25cb
        } else {
            0x25cf
        }
    } else if open {
        0x25e6
    } else {
        0x2022
    };
    Some(char::from_u32(mark).unwrap_or('*').to_string())
}

fn quoted_text(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        Some(&value[1..value.len() - 1])
    } else {
        None
    }
}

fn collapses_spaces_for_paint(white_space: WhiteSpace) -> bool {
    matches!(
        white_space,
        WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine
    )
}

fn collapse_spaces_for_paint(text: &str, mut previous_space: bool) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !previous_space {
                out.push(' ');
            }
            previous_space = true;
        } else {
            out.push(ch);
            previous_space = false;
        }
    }
    (out, previous_space)
}

fn vertical_align_y_shift(vertical_align: &crate::types::VerticalAlign, font_px: f32) -> f32 {
    let shift =
        crate::layout::inline_layout::vertical_align_shift(vertical_align, font_px, font_px);
    match vertical_align {
        crate::types::VerticalAlign::Super => -shift,
        crate::types::VerticalAlign::Sub => shift,
        crate::types::VerticalAlign::TextTop | crate::types::VerticalAlign::TextBottom => -shift,
        crate::types::VerticalAlign::Length(_) => -shift,
        _ => 0.0,
    }
}

fn capitalize_words(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_space = true;
    for c in text.chars() {
        if prev_space && c.is_alphabetic() {
            for uc in c.to_uppercase() {
                result.push(uc);
            }
        } else {
            result.push(c);
        }
        prev_space = c.is_whitespace();
    }
    result
}

fn format_list_marker(lst: ListStyleType, index: i32) -> String {
    let style = match lst {
        ListStyleType::Decimal => "decimal",
        ListStyleType::DecimalLeadingZero => "decimal-leading-zero",
        ListStyleType::LowerAlpha | ListStyleType::LowerLatin => "lower-alpha",
        ListStyleType::UpperAlpha | ListStyleType::UpperLatin => "upper-alpha",
        ListStyleType::LowerRoman => "lower-roman",
        ListStyleType::UpperRoman => "upper-roman",
        ListStyleType::LowerGreek => "lower-greek",
        ListStyleType::CjkDecimal => "cjk-decimal",
        ListStyleType::Armenian => "armenian",
        ListStyleType::Georgian => "georgian",
        ListStyleType::Hebrew => "hebrew",
        ListStyleType::Hiragana => "hiragana",
        ListStyleType::Katakana => "katakana",
        ListStyleType::HiraganaIroha => "hiragana-iroha",
        ListStyleType::KatakanaIroha => "katakana-iroha",
        _ => return String::new(),
    };
    format!("{}.", crate::css::format_counter_value(index, style))
}

fn collect_fixed_elements(node: &WebCore, out: &mut Vec<u32>) {
    if node.style.position == Position::Fixed && node.node_id != 0 {
        out.push(node.node_id);
    }
    for child in &node.children {
        collect_fixed_elements(child, out);
    }
}

fn is_explicit_z_positioned(node: &WebCore) -> bool {
    node.style.is_positioned()
        && !node.style.z_index_is_auto
        && node.style.position != Position::Fixed
}

fn collect_explicit_z_descendants<'a>(node: &'a WebCore, out: &mut Vec<&'a WebCore>) {
    if matches!(node.style.display, Display::None)
        || node.tag == "::before"
        || node.tag == "::after"
        || node.style.position == Position::Fixed
    {
        return;
    }
    if is_explicit_z_positioned(node) {
        out.push(node);
        return;
    }
    for child in node.effective_children() {
        collect_explicit_z_descendants(child, out);
    }
}

/// The concrete object rect for a replaced element (css-images-3 §5.5):
/// `object-fit` chooses the drawn size from the natural size and the content
/// box, `object-position` places it inside that box. Returns the destination
/// rect and whether it overflows the box (so the caller clips).
///
/// ⛔ Both properties were parsed into `ComputedStyle` and read by nobody, so
/// every replaced element painted stretched to its content box — the `fill`
/// behaviour — whatever the author asked for.
fn object_fit_rect(
    style: &crate::types::ComputedStyle,
    cr: Rect,
    iw: f32,
    ih: f32,
    font_px: f32,
    root_font_px: f32,
) -> (Rect, bool) {
    use crate::types::ObjectFit;
    if iw <= 0.0 || ih <= 0.0 || cr.w <= 0.0 || cr.h <= 0.0 {
        return (cr, false);
    }
    let contain = (cr.w / iw).min(cr.h / ih);
    let cover = (cr.w / iw).max(cr.h / ih);
    let (ow, oh) = match style.object_fit {
        ObjectFit::Fill => (cr.w, cr.h),
        ObjectFit::Contain => (iw * contain, ih * contain),
        ObjectFit::Cover => (iw * cover, ih * cover),
        ObjectFit::None => (iw, ih),
        // `scale-down` is the SMALLER of `none` and `contain`.
        ObjectFit::ScaleDown => {
            if contain >= 1.0 {
                (iw, ih)
            } else {
                (iw * contain, ih * contain)
            }
        }
    };
    // A percentage aligns that fraction of the object with the same fraction of
    // the box, so it distributes the free space — which is negative when the
    // object is larger, and correctly shifts it left/up.
    let place = |len: &crate::types::CssLength, free: f32, extent: f32| -> f32 {
        match len {
            crate::types::CssLength::Percent(p) => free * (p / 100.0),
            other @ (crate::types::CssLength::Calc(_) | crate::types::CssLength::CalcExpr(_)) => {
                other.resolve(font_px, free, root_font_px)
            }
            other => other.resolve(font_px, extent, root_font_px),
        }
    };
    let x = cr.x + place(&style.object_position_x, cr.w - ow, cr.w);
    let y = cr.y + place(&style.object_position_y, cr.h - oh, cr.h);
    let overflows = ow > cr.w + 0.01 || oh > cr.h + 0.01;
    (Rect::new(x, y, ow, oh), overflows)
}
