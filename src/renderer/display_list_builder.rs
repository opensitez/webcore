//! Display list builder — walks the box tree and records paint commands.
//!
//! Uses EXACT positions from the layout engine. Never approximates.
//! Faithfully ports the render_box logic from mod.rs into PaintCmd recording.

use super::display_list::{DisplayList, ImageRef, PaintCmd, TextDecoration};
use crate::types::{
    BackgroundClip, BackgroundRepeat, BackgroundSize, BorderStyle, ClipPathKind, Color,
    ComputedStyle, ContentVisibility, CssLength, Direction, Display, FontStyle,
    GradientRadialShape, GradientRadialSize, GradientType, ListStylePosition, ListStyleType,
    MixBlendMode, Overflow, Position, Resize, TextAlign, TextDecorationStyle, TextOverflow,
    TextTransform, WhiteSpace,
};
use crate::types::{Rect, WebCore};

struct BackgroundImagePaint<'a> {
    data: std::sync::Arc<Vec<u8>>,
    image_width: u32,
    image_height: u32,
    ratio_only: bool,
    size: BackgroundSize,
    size_w: &'a CssLength,
    size_h: &'a CssLength,
    position_x: &'a CssLength,
    position_y: &'a CssLength,
    repeat: BackgroundRepeat,
}

fn push_background_image_paint(
    list: &mut DisplayList,
    layer: BackgroundImagePaint<'_>,
    font_px: f32,
    root_font_px: f32,
    bg_origin_rect: Rect,
    bg_clip_rect: Rect,
    radii: [f32; 4],
    radii_y: [f32; 4],
    blend_mode: u8,
) {
    if layer.image_width == 0 || layer.image_height == 0 {
        return;
    }
    let iw = layer.image_width as f32;
    let ih = layer.image_height as f32;
    let ow = bg_origin_rect.w;
    let oh = bg_origin_rect.h;

    let (draw_w, draw_h) = match layer.size {
        BackgroundSize::Cover => {
            let scale = (ow / iw).max(oh / ih);
            (iw * scale, ih * scale)
        }
        BackgroundSize::Contain => {
            let scale = (ow / iw).min(oh / ih);
            (iw * scale, ih * scale)
        }
        BackgroundSize::Explicit => {
            let w_auto = layer.size_w.is_auto();
            let h_auto = layer.size_h.is_auto();
            let explicit_w = (!w_auto).then(|| layer.size_w.resolve(font_px, ow, root_font_px));
            let explicit_h = (!h_auto).then(|| layer.size_h.resolve(font_px, oh, root_font_px));
            match (explicit_w, explicit_h) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w * ih / iw),
                (None, Some(h)) => (h * iw / ih, h),
                (None, None) => (iw, ih),
            }
        }
        BackgroundSize::Auto => {
            if layer.ratio_only && ow > 0.0 && oh > 0.0 {
                let scale = (ow / iw).min(oh / ih);
                (iw * scale, ih * scale)
            } else {
                (iw, ih)
            }
        }
    };

    let pos_x = bg_origin_rect.x + layer.position_x.resolve(font_px, ow - draw_w, root_font_px);
    let pos_y = bg_origin_rect.y + layer.position_y.resolve(font_px, oh - draw_h, root_font_px);
    let size_mode = match layer.size {
        BackgroundSize::Auto => 0u8,
        BackgroundSize::Cover => 1,
        BackgroundSize::Contain => 2,
        BackgroundSize::Explicit => 3,
    };
    let (repeat_x_mode, repeat_y_mode) = layer.repeat.axis_modes();

    list.push(PaintCmd::BackgroundImage {
        container: bg_origin_rect,
        clip: bg_clip_rect,
        data: ImageRef::Shared(layer.data, layer.image_width, layer.image_height),
        size_mode,
        draw_w,
        draw_h,
        pos_x,
        pos_y,
        repeat_x_mode,
        repeat_y_mode,
        radii,
        radii_y,
        blend_mode,
    });
}

fn background_gradient_rect(
    size: BackgroundSize,
    size_w: &CssLength,
    size_h: &CssLength,
    position_x: &CssLength,
    position_y: &CssLength,
    font_px: f32,
    root_font_px: f32,
    bg_origin_rect: Rect,
) -> Rect {
    let ow = bg_origin_rect.w;
    let oh = bg_origin_rect.h;
    let (draw_w, draw_h) = match size {
        BackgroundSize::Cover | BackgroundSize::Contain | BackgroundSize::Auto => (ow, oh),
        BackgroundSize::Explicit => {
            let w_auto = size_w.is_auto();
            let h_auto = size_h.is_auto();
            let draw_w = if w_auto {
                ow
            } else {
                size_w.resolve(font_px, ow, root_font_px)
            };
            let draw_h = if h_auto {
                oh
            } else {
                size_h.resolve(font_px, oh, root_font_px)
            };
            (draw_w, draw_h)
        }
    };
    let x = bg_origin_rect.x + position_x.resolve(font_px, ow - draw_w, root_font_px);
    let y = bg_origin_rect.y + position_y.resolve(font_px, oh - draw_h, root_font_px);
    Rect::new(x, y, draw_w.max(0.0), draw_h.max(0.0))
}

fn root_font_size_px(root: &WebCore) -> f32 {
    let initial = ComputedStyle::INITIAL_FONT_SIZE_PX;
    root.style.font_size_px(initial, initial)
}

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
        sticky_scroll_container: None,
        sticky_containing_block: None,
        hovered_id: 0,
        active_id: 0,
        visited_hrefs: &visited,
        base_url: "",
        clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        paint_clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        suppress_deferred_z_descendants: false,
        font_system: None,
        transform_ctx: crate::types::TransformCtx {
            // The root box's font size IS the root font size — `rem`.
            font_px: root_font_size_px(root),
            root_font_px: root_font_size_px(root),
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
    build_display_list_full_with_font_system(
        root,
        viewport_w,
        viewport_h,
        scroll_x,
        scroll_y,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        None,
    )
}

pub fn build_display_list_full_with_font_system(
    root: &WebCore,
    viewport_w: f32,
    viewport_h: f32,
    scroll_x: f32,
    scroll_y: f32,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &std::collections::HashSet<String>,
    base_url: &str,
    font_system: Option<*mut cosmic_text::FontSystem>,
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
        sticky_scroll_container: None,
        sticky_containing_block: None,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        paint_clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        suppress_deferred_z_descendants: false,
        font_system,
        transform_ctx: crate::types::TransformCtx {
            // The root box's font size IS the root font size — `rem`.
            font_px: root_font_size_px(root),
            root_font_px: root_font_size_px(root),
            viewport_w,
            viewport_h,
        },
    };
    let mut list = DisplayList::new();
    build_for_box(root, &mut list, &ctx);

    list
}

/// Build a display list for a scroll-local paint band.
///
/// Commands remain in document coordinates so replay still scroll-translates the
/// list. The band only limits which boxes emit paint commands; child traversal is
/// preserved for overflow-visible and positioned descendants.
pub fn build_display_list_viewport(
    root: &WebCore,
    viewport_w: f32,
    viewport_h: f32,
    scroll_x: f32,
    scroll_y: f32,
    paint_top: f32,
    paint_bottom: f32,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &std::collections::HashSet<String>,
    base_url: &str,
) -> DisplayList {
    build_display_list_viewport_with_font_system(
        root,
        viewport_w,
        viewport_h,
        scroll_x,
        scroll_y,
        paint_top,
        paint_bottom,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        None,
    )
}

pub fn build_display_list_viewport_with_font_system(
    root: &WebCore,
    viewport_w: f32,
    viewport_h: f32,
    scroll_x: f32,
    scroll_y: f32,
    paint_top: f32,
    paint_bottom: f32,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &std::collections::HashSet<String>,
    base_url: &str,
    font_system: Option<*mut cosmic_text::FontSystem>,
) -> DisplayList {
    let doc_h = crate::types::Document::scroll_height(root).max(viewport_h);
    let paint_top = paint_top.max(0.0);
    let paint_bottom = paint_bottom.max(paint_top).min(doc_h.max(viewport_h));
    let paint_clip = Rect::new(
        0.0,
        paint_top,
        viewport_w,
        (paint_bottom - paint_top).max(0.0),
    );
    let ctx = BuildContext {
        scroll_x: 0.0,
        scroll_y: 0.0,
        sticky_scroll_x: scroll_x,
        sticky_scroll_y: scroll_y,
        sticky_scroll_container: None,
        sticky_containing_block: None,
        hovered_id,
        active_id,
        visited_hrefs,
        base_url,
        clip: Rect::new(0.0, 0.0, viewport_w, doc_h),
        paint_clip,
        suppress_deferred_z_descendants: false,
        font_system,
        transform_ctx: crate::types::TransformCtx {
            font_px: root_font_size_px(root),
            root_font_px: root_font_size_px(root),
            viewport_w,
            viewport_h,
        },
    };
    let mut list = DisplayList::new();
    build_for_box(root, &mut list, &ctx);

    list
}

#[derive(Clone, Copy, Debug)]
struct StickyScrollContainer {
    /// In display list coordinates (before this container's own internal scroll).
    scrollport: Rect,
}

#[inline]
fn is_scroll_container(style: &ComputedStyle) -> bool {
    matches!(
        style.overflow_x,
        Overflow::Hidden | Overflow::Scroll | Overflow::Auto
    ) || matches!(
        style.overflow_y,
        Overflow::Hidden | Overflow::Scroll | Overflow::Auto
    )
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
    sticky_scroll_container: Option<StickyScrollContainer>,
    sticky_containing_block: Option<Rect>,
    hovered_id: u32,
    active_id: u32,
    visited_hrefs: &'a std::collections::HashSet<String>,
    base_url: &'a str,
    clip: Rect,
    paint_clip: Rect,
    suppress_deferred_z_descendants: bool,
    font_system: Option<*mut cosmic_text::FontSystem>,
    /// What a `transform` needs to resolve `vw`/`vh` and `rem`. Carried on the
    /// context because the element's own box is not enough: a transform length
    /// can name the viewport.
    transform_ctx: crate::types::TransformCtx,
}

fn measure_paint_text_width(
    ctx: &BuildContext<'_>,
    text: &str,
    font_px: f32,
    weight: crate::types::FontWeight,
    style: crate::types::FontStyle,
    font_family: &str,
    font_stretch: f32,
) -> f32 {
    let font_system = ctx.font_system.map(|ptr| unsafe { &mut *ptr });
    crate::layout::inline_layout::measure_text_width_weighted(
        text,
        font_px,
        font_system,
        weight,
        style,
        1.0,
        font_family,
        font_stretch,
    )
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

fn inline_has_non_empty_text(node: &WebCore) -> bool {
    if node.tag == "#text" && !node.text.trim().is_empty() {
        return true;
    }
    for c in &node.children {
        if inline_has_non_empty_text(c) {
            return true;
        }
    }
    false
}

fn inline_box_has_edges(node: &WebCore) -> bool {
    node.layout.padding_rect.w > node.layout.content_rect.w + 0.5
        || node.layout.padding_rect.h > node.layout.content_rect.h + 0.5
        || node.layout.border_rect.w > node.layout.padding_rect.w + 0.5
        || node.layout.border_rect.h > node.layout.padding_rect.h + 0.5
}

fn direct_child_element_has_owned_inline_text_paint(node: &WebCore) -> bool {
    for child in node.effective_children() {
        if child.style.display == Display::None || child.style.position == Position::Fixed {
            continue;
        }
        if child.tag != "#text" && !child.layout.line_cache.is_empty() {
            return true;
        }
    }
    false
}

fn background_clip_rect_for_node(node: &WebCore, sx: f32, sy: f32) -> Rect {
    match node.style.background_clip {
        BackgroundClip::ContentBox => {
            let c = node.layout.content_rect;
            Rect::new(c.x - sx, c.y - sy, c.w, c.h)
        }
        BackgroundClip::PaddingBox | BackgroundClip::Text => {
            let p = node.layout.padding_rect;
            Rect::new(p.x - sx, p.y - sy, p.w, p.h)
        }
        BackgroundClip::BorderBox => {
            let b = node.layout.border_rect;
            Rect::new(b.x - sx, b.y - sy, b.w, b.h)
        }
    }
}

fn resolved_border_radii_for_node(node: &WebCore, root_font_px: f32) -> ([f32; 4], [f32; 4]) {
    let p = node.layout.padding_rect;
    let font_px = node.style.font_size_px(root_font_px, root_font_px).max(1.0);
    let radii = [
        node.style
            .border_top_left_radius
            .resolve(font_px, p.w, root_font_px),
        node.style
            .border_top_right_radius
            .resolve(font_px, p.w, root_font_px),
        node.style
            .border_bottom_right_radius
            .resolve(font_px, p.w, root_font_px),
        node.style
            .border_bottom_left_radius
            .resolve(font_px, p.w, root_font_px),
    ];
    let radii_y = [
        node.style
            .border_top_left_radius_y
            .resolve(font_px, p.h, root_font_px),
        node.style
            .border_top_right_radius_y
            .resolve(font_px, p.h, root_font_px),
        node.style
            .border_bottom_right_radius_y
            .resolve(font_px, p.h, root_font_px),
        node.style
            .border_bottom_left_radius_y
            .resolve(font_px, p.h, root_font_px),
    ];
    (radii, radii_y)
}

fn push_inline_descendant_box_backgrounds(
    node: &WebCore,
    list: &mut DisplayList,
    sx: f32,
    sy: f32,
    paint_clip: Rect,
    root_font_px: f32,
) {
    for child in node.effective_children() {
        if matches!(child.style.display, Display::None) || !child.style.visibility {
            continue;
        }

        let is_inline_text_box = child.style.is_inline_level()
            && !child.is_image_element()
            && inline_has_non_empty_text(child);
        if is_inline_text_box
            && inline_box_has_edges(child)
            && child.style.background_color.a > 0
            && child.style.background_clip != BackgroundClip::Text
            && rect_intersects(background_clip_rect_for_node(child, sx, sy), paint_clip)
        {
            let opacity = child.style.opacity;
            let raw = child.style.background_color;
            let color = Color::rgba(raw.r, raw.g, raw.b, ((raw.a as f32) * opacity) as u8);
            let (radii, radii_y) = resolved_border_radii_for_node(child, root_font_px);
            list.push(PaintCmd::FillRect {
                rect: background_clip_rect_for_node(child, sx, sy),
                color,
                radius: radii,
                radius_y: radii_y,
            });
        }

        if child.style.is_inline_level() || matches!(child.style.display, Display::Contents) {
            push_inline_descendant_box_backgrounds(child, list, sx, sy, paint_clip, root_font_px);
        }
    }
}

#[inline]
fn rect_intersects(a: Rect, b: Rect) -> bool {
    a.right() >= b.x && a.x <= b.right() && a.bottom() >= b.y && a.y <= b.bottom()
}

fn inline_relative_visual_offset(root: &WebCore, path: &[usize], root_font_px: f32) -> (f32, f32) {
    let mut cur = root;
    let mut dx = 0.0;
    let mut dy = 0.0;
    for &idx in path {
        let Some(next) = cur.children.get(idx) else {
            break;
        };
        cur = next;
        if cur.style.position != Position::Relative {
            continue;
        }
        let font_px = cur.style.font_size_px(root_font_px, root_font_px);
        let containing_w = cur.layout.content_rect.w.max(root.layout.content_rect.w);
        if !cur.style.left.is_auto() {
            dx += cur.style.left.resolve(font_px, containing_w, root_font_px);
        } else if !cur.style.right.is_auto() {
            dx -= cur.style.right.resolve(font_px, containing_w, root_font_px);
        }
        if !cur.style.top.is_auto() {
            dy += cur.style.top.resolve(font_px, containing_w, root_font_px);
        } else if !cur.style.bottom.is_auto() {
            dy -= cur
                .style
                .bottom
                .resolve(font_px, containing_w, root_font_px);
        }
    }
    (dx, dy)
}

fn node_at_relative_path<'a>(root: &'a WebCore, path: &[usize]) -> Option<&'a WebCore> {
    let mut cur = root;
    for &idx in path {
        cur = cur.children.get(idx)?;
    }
    Some(cur)
}

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

    if node.tag == "#text" {
        build_laid_out_text_node(node, list, ctx, ctx.scroll_x, ctx.scroll_y, ctx.paint_clip);
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
            || bx > ctx.paint_clip.right()
            || by > ctx.paint_clip.bottom()
        {
            return;
        }
    }

    let pr = node.layout.padding_rect;
    let px = pr.x - sx;
    let py = pr.y - sy;
    let pw = pr.w;
    let ph = pr.h;
    let font_px = node.style.font_size_px(ctx.transform_ctx.root_font_px, ctx.transform_ctx.root_font_px);
    let paint_self = rect_intersects(
        Rect::new(
            br.x - sx - 256.0,
            br.y - sy - 256.0,
            br.w + 512.0,
            br.h + 512.0,
        ),
        ctx.paint_clip,
    );

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
        .resolve(font_px, pr.w, ctx.transform_ctx.root_font_px);
    let r_tr = node
        .style
        .border_top_right_radius
        .resolve(font_px, pr.w, ctx.transform_ctx.root_font_px);
    let r_br = node
        .style
        .border_bottom_right_radius
        .resolve(font_px, pr.w, ctx.transform_ctx.root_font_px);
    let r_bl = node
        .style
        .border_bottom_left_radius
        .resolve(font_px, pr.w, ctx.transform_ctx.root_font_px);
    let r_tl_y = node
        .style
        .border_top_left_radius_y
        .resolve(font_px, pr.h, ctx.transform_ctx.root_font_px);
    let r_tr_y = node
        .style
        .border_top_right_radius_y
        .resolve(font_px, pr.h, ctx.transform_ctx.root_font_px);
    let r_br_y = node
        .style
        .border_bottom_right_radius_y
        .resolve(font_px, pr.h, ctx.transform_ctx.root_font_px);
    let r_bl_y = node
        .style
        .border_bottom_left_radius_y
        .resolve(font_px, pr.h, ctx.transform_ctx.root_font_px);
    let radii_arr = [r_tl, r_tr, r_br, r_bl];
    let radii_y_arr = [r_tl_y, r_tr_y, r_br_y, r_bl_y];

    // ── Hover / active / visited check ───────────────────────────────────────
    // Hover is applied by the cascade/layout pass. Applying `hover_style` again
    // here makes paint disagree with layout: geometry is computed from one style
    // while text/background are drawn from another, which shows up as growing
    // link text and hover backgrounds bleeding into neighbouring boxes.
    let is_hovered = false;
    let is_active =
        ctx.active_id != 0 && node.style.active_style.is_some() && subtree_has(node, ctx.active_id);
    let is_visited = node.style.visited_style.is_some()
        && !node.style.href.is_empty()
        && ctx.visited_hrefs.contains(&node.style.href);

    let eff_style: &ComputedStyle = if is_active {
        node.style.active_style.as_deref().unwrap_or(&node.style)
    } else if is_visited {
        node.style.visited_style.as_deref().unwrap_or(&node.style)
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
        list.has_scroll_dependent_sticky = true;
        let (viewport_left, viewport_top, viewport_right, viewport_bottom) =
            if let Some(sc) = ctx.sticky_scroll_container {
                (
                    sc.scrollport.x,
                    sc.scrollport.y,
                    sc.scrollport.right(),
                    sc.scrollport.bottom(),
                )
            } else {
                let vl = ctx.sticky_scroll_x + ctx.clip.x;
                let vt = ctx.sticky_scroll_y + ctx.clip.y;
                (
                    vl,
                    vt,
                    vl + ctx.transform_ctx.viewport_w,
                    vt + ctx.transform_ctx.viewport_h,
                )
            };

        let top_val = node
            .style
            .top
            .resolve(font_px, viewport_bottom - viewport_top, ctx.transform_ctx.root_font_px);
        let left_val = node
            .style
            .left
            .resolve(font_px, viewport_right - viewport_left, ctx.transform_ctx.root_font_px);
        let bottom_val = node
            .style
            .bottom
            .resolve(font_px, viewport_bottom - viewport_top, ctx.transform_ctx.root_font_px);
        let right_val = node
            .style
            .right
            .resolve(font_px, viewport_right - viewport_left, ctx.transform_ctx.root_font_px);
        let nat_x = pr.x - sx;
        let nat_y = pr.y - sy;

        let has_left = !node.style.left.is_auto();
        let has_right = !node.style.right.is_auto();
        let has_top = !node.style.top.is_auto();
        let has_bottom = !node.style.bottom.is_auto();

        let mut cx = nat_x;
        let v_w = viewport_right - viewport_left;
        if has_left && has_right {
            if pw + left_val + right_val > v_w {
                if node.style.direction == Direction::RTL {
                    cx = cx.min(viewport_right - right_val - pw);
                } else {
                    cx = cx.max(viewport_left + left_val);
                }
            } else {
                if node.style.direction == Direction::RTL {
                    cx = cx.min(viewport_right - right_val - pw);
                    cx = cx.max(viewport_left + left_val);
                } else {
                    cx = cx.max(viewport_left + left_val);
                    cx = cx.min(viewport_right - right_val - pw);
                }
            }
        } else if has_left {
            cx = cx.max(viewport_left + left_val);
        } else if has_right {
            cx = cx.min(viewport_right - right_val - pw);
        }

        let mut cy = nat_y;
        let v_h = viewport_bottom - viewport_top;
        if has_top && has_bottom {
            if ph + top_val + bottom_val > v_h {
                cy = cy.max(viewport_top + top_val);
            } else {
                cy = cy.max(viewport_top + top_val);
                cy = cy.min(viewport_bottom - bottom_val - ph);
            }
        } else if has_top {
            cy = cy.max(viewport_top + top_val);
        } else if has_bottom {
            cy = cy.min(viewport_bottom - bottom_val - ph);
        }

        // Containing block clamp
        if let Some(cb) = ctx.sticky_containing_block {
            let cb_left = cb.x - sx;
            let cb_top = cb.y - sy;
            let cb_right = cb_left + cb.w;
            let cb_bottom = cb_top + cb.h;

            if cb.w > 0.0 {
                if cb_right - cb_left >= pw {
                    cx = cx.clamp(cb_left, cb_right - pw);
                } else if node.style.direction == Direction::RTL {
                    cx = cb_right - pw;
                } else {
                    cx = cb_left;
                }
            }

            if cb.h > 0.0 {
                if cb_bottom - cb_top >= ph {
                    cy = cy.clamp(cb_top, cb_bottom - ph);
                } else {
                    cy = cb_top;
                }
            }
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

    let mask_image_requested = !eff_style.rare().mask_image_url.trim().is_empty();
    let has_mask_layer = node.mask_image_data.is_some()
        && node.mask_image_width > 0
        && node.mask_image_height > 0
        && pw > 0.0
        && ph > 0.0;
    if paint_self && mask_image_requested && !has_mask_layer {
        return;
    }

    // ── Stacking context ─────────────────────────────────────────────────────
    let blend = blend_mode_to_u8(eff_style.mix_blend_mode);
    let stacking = (eff_style.is_positioned() && !eff_style.z_index_is_auto)
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
    //
    // Paint commands below already fold `eff_style.opacity` into their color
    // alpha. Emitting an additional full-viewport opacity layer here made every
    // semi-transparent element allocate and composite a whole pixmap during
    // replay, and also applied opacity twice.

    // ── Blend mode ───────────────────────────────────────────────────────────
    if blend != 0 {
        list.push(PaintCmd::PushBlendMode { mode: blend });
    }

    // ── CSS transform ────────────────────────────────────────────────────────
    // Use the element's DOCUMENT position (pr.x, pr.y) for the transform origin,
    // not the scroll-adjusted position (px, py). The scroll offset is applied
    // separately by the replay's global transform. This prevents transforms from
    // shifting when the user scrolls.
    let has_transform = !eff_style.css_transform.ops.is_empty() || eff_style.will_change_transform;
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
            node_id: node.node_id,
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
        if paint_self {
            list.push(PaintCmd::BackdropFilter {
                rect: Rect::new(px, py, pw, ph),
                filters: encode_filter_ops(&backdrop_filters),
            });
        }
    }

    // ── CSS filters ───────────────────────────────────────────────────────────
    let has_filter = !eff_style.css_filter.ops.is_empty();
    if has_filter {
        let filters = encode_filter_ops(&eff_style.css_filter);
        list.push(PaintCmd::PushFilter { filters });
    }

    let clip_path_rect = clip_path_rect(eff_style, node.layout.border_rect, sx, sy, font_px, ctx.transform_ctx.root_font_px);
    let clip_path_polygon =
        clip_path_polygon_points(eff_style, node.layout.border_rect, sx, sy, font_px, ctx.transform_ctx.root_font_px);
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
        if paint_self && !bs.inset {
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
            BackgroundClip::PaddingBox | BackgroundClip::Text => Rect::new(px, py, pw, ph),
            BackgroundClip::BorderBox => Rect::new(br.x - eff_sx, br.y - eff_sy, br.w, br.h),
        }
    };
    let bg_clip_rect = background_box(eff_style.background_clip);
    let bg_origin_rect = background_box(eff_style.background_origin);
    let raw_bg = eff_style.background_color;
    let clipped_background_color = Color::rgba(
        raw_bg.r,
        raw_bg.g,
        raw_bg.b,
        (raw_bg.a as f32 * eff_style.opacity) as u8,
    );
    // `background-repeat` decides, per axis, whether the image (or gradient)
    // tiles out of the positioning area to cover the painting area.
    let (bg_repeat_x_mode, bg_repeat_y_mode) = node.style.background_repeat.axis_modes();

    // ── (b) Background color (opacity applied to alpha) ──────────────────────
    {
        let is_inline_with_text = node.style.is_inline_level()
            && !node.is_image_element()
            && inline_has_non_empty_text(node);
        let opacity = eff_style.opacity;
        if raw_bg.a > 0
            && !is_inline_with_text
            && eff_style.background_clip != BackgroundClip::Text
        {
            let alpha = ((raw_bg.a as f32) * opacity) as u8;
            let bg = Color::rgba(raw_bg.r, raw_bg.g, raw_bg.b, alpha);
            if paint_self {
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
    }

    // ── (c) Gradient background ──────────────────────────────────────────────
    let mut has_text_gradient = false;
    if paint_self
        && node.style.gradient_type != GradientType::None
        && node.style.rare().gradient_stops.len() >= 2
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
        let gradient_rect = background_gradient_rect(
            node.style.background_size,
            &node.style.background_size_w,
            &node.style.background_size_h,
            &node.style.background_position_x,
            &node.style.background_position_y,
            font_px,
            ctx.transform_ctx.root_font_px,
            bg_origin_rect,
        );
        let radial_center_x = node.style.gradient_radial_position_x.resolve(
            font_px,
            gradient_rect.w,
            ctx.transform_ctx.root_font_px,
        );
        let radial_center_y = node.style.gradient_radial_position_y.resolve(
            font_px,
            gradient_rect.h,
            ctx.transform_ctx.root_font_px,
        );
        let (radial_radius_x, radial_radius_y) = radial_gradient_used_radii(
            &node.style,
            gradient_rect.w,
            gradient_rect.h,
            radial_center_x,
            radial_center_y,
            font_px,
            ctx.transform_ctx.root_font_px,
        );
        if eff_style.background_clip == BackgroundClip::Text {
            has_text_gradient = true;
            list.push(PaintCmd::PushTextGradient {
                rect: gradient_rect,
                background_color: clipped_background_color,
                gradient_type: grad_type_u8,
                angle: node.style.gradient_angle,
                direction: node.style.gradient_direction,
                radial_center_x,
                radial_center_y,
                radial_radius_x,
                radial_radius_y,
                stops,
            });
        } else {
            list.push(PaintCmd::Gradient {
                rect: gradient_rect,
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
    }
    if paint_self
        && eff_style.background_clip == BackgroundClip::Text
        && !has_text_gradient
        && clipped_background_color.a > 0
    {
        has_text_gradient = true;
        list.push(PaintCmd::PushTextGradient {
            rect: bg_clip_rect,
            background_color: Color::TRANSPARENT,
            gradient_type: 1,
            angle: 90.0,
            direction: crate::types::GradientDirection::Angle(90.0),
            radial_center_x: 0.0,
            radial_center_y: 0.0,
            radial_radius_x: 1.0,
            radial_radius_y: 1.0,
            stops: vec![(clipped_background_color, 0.0), (clipped_background_color, 1.0)],
        });
    }

    // ── (b2) Additional background gradient layers ───────────────────────────
    if paint_self {
        for layer in &eff_style.rare().additional_background_layers {
            if layer.gradient_type != GradientType::None && layer.gradient_stops.len() >= 2 {
                let opacity = eff_style.opacity;
                let grad_type_u8 = match layer.gradient_type {
                    GradientType::Linear => 1u8,
                    GradientType::Radial => 2u8,
                    GradientType::None => 0u8,
                };
                let stops: Vec<(Color, f32)> = layer
                    .gradient_stops
                    .iter()
                    .map(|s| {
                        let a = ((s.color.a as f32) * opacity) as u8;
                        (Color::rgba(s.color.r, s.color.g, s.color.b, a), s.position)
                    })
                    .collect();
                let layer_clip_rect = background_box(layer.clip);
                let layer_origin_rect = background_box(layer.origin);
                let gradient_rect = background_gradient_rect(
                    layer.size,
                    &layer.size_w,
                    &layer.size_h,
                    &layer.position_x,
                    &layer.position_y,
                    font_px,
                    ctx.transform_ctx.root_font_px,
                    layer_origin_rect,
                );
                let radial_center_x = layer.gradient_radial_position_x.resolve(
                    font_px,
                    gradient_rect.w,
                    ctx.transform_ctx.root_font_px,
                );
                let radial_center_y = layer.gradient_radial_position_y.resolve(
                    font_px,
                    gradient_rect.h,
                    ctx.transform_ctx.root_font_px,
                );
                let mut temp_style = ComputedStyle::default();
                temp_style.gradient_radial_shape = layer.gradient_radial_shape;
                temp_style.gradient_radial_size = layer.gradient_radial_size;
                temp_style.gradient_radial_radius_x = layer.gradient_radial_radius_x.clone();
                temp_style.gradient_radial_radius_y = layer.gradient_radial_radius_y.clone();
                let (radial_radius_x, radial_radius_y) = radial_gradient_used_radii(
                    &temp_style,
                    gradient_rect.w,
                    gradient_rect.h,
                    radial_center_x,
                    radial_center_y,
                    font_px,
                    ctx.transform_ctx.root_font_px,
                );
                let (layer_repeat_x_mode, layer_repeat_y_mode) = layer.repeat.axis_modes();
                list.push(PaintCmd::Gradient {
                    rect: gradient_rect,
                    clip: layer_clip_rect,
                    repeat_x_mode: layer_repeat_x_mode,
                    repeat_y_mode: layer_repeat_y_mode,
                    gradient_type: grad_type_u8,
                    angle: layer.gradient_angle,
                    direction: layer.gradient_direction,
                    radial_center_x,
                    radial_center_y,
                    radial_radius_x,
                    radial_radius_y,
                    stops,
                    radii: radii_arr,
                    radii_y: radii_y_arr,
                    opacity,
                    blend_mode: background_blend_mode_to_u8(&layer.blend_mode),
                });
            }
        }
    }

    // ── (d) Background image ─────────────────────────────────────────────────
    if paint_self && let Some(ref bg_data) = node.bg_image_data {
        push_background_image_paint(
            list,
            BackgroundImagePaint {
                data: bg_data.clone(),
                image_width: node.bg_image_width,
                image_height: node.bg_image_height,
                ratio_only: node.bg_image_ratio_only,
                size: node.style.background_size,
                size_w: &node.style.background_size_w,
                size_h: &node.style.background_size_h,
                position_x: &node.style.background_position_x,
                position_y: &node.style.background_position_y,
                repeat: node.style.background_repeat,
            },
            font_px,
            ctx.transform_ctx.root_font_px,
            bg_origin_rect,
            bg_clip_rect,
            radii_arr,
            radii_y_arr,
            background_blend_mode_to_u8(&eff_style.background_blend_mode),
        );
    }

    if paint_self {
        for (layer_index, layer) in eff_style
            .rare()
            .additional_background_layers
            .iter()
            .enumerate()
        {
            let Some(Some(bg_image)) = node.additional_bg_images.get(layer_index) else {
                continue;
            };
            let layer_clip_rect = background_box(layer.clip);
            let layer_origin_rect = background_box(layer.origin);
            push_background_image_paint(
                list,
                BackgroundImagePaint {
                    data: bg_image.data.clone(),
                    image_width: bg_image.width,
                    image_height: bg_image.height,
                    ratio_only: bg_image.ratio_only,
                    size: layer.size,
                    size_w: &layer.size_w,
                    size_h: &layer.size_h,
                    position_x: &layer.position_x,
                    position_y: &layer.position_y,
                    repeat: layer.repeat,
                },
                font_px,
                ctx.transform_ctx.root_font_px,
                layer_origin_rect,
                layer_clip_rect,
                radii_arr,
                radii_y_arr,
                background_blend_mode_to_u8(&layer.blend_mode),
            );
        }
    }

    // ── (e) Inset box-shadow ─────────────────────────────────────────────────
    for bs in &eff_style.box_shadow {
        if paint_self && bs.inset {
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
        if paint_self && !node.layout.collapsed_border_segments.is_empty() {
            for segment in &node.layout.collapsed_border_segments {
                if segment.width <= 0.0 || segment.style == BorderStyle::None || segment.color.a == 0 {
                    continue;
                }
                let rect = Rect::new(
                    segment.rect.x - eff_sx,
                    segment.rect.y - eff_sy,
                    segment.rect.w,
                    segment.rect.h,
                );
                let mut widths = [0.0; 4];
                let mut colors = [Color::TRANSPARENT; 4];
                let mut styles = [0; 4];
                let side = if segment.axis == 0 { 0 } else { 3 };
                widths[side] = segment.width;
                colors[side] = segment.color;
                styles[side] = bstyle(segment.style);
                list.push(PaintCmd::Border {
                    rect,
                    widths,
                    colors,
                    styles,
                    radii: [0.0; 4],
                    radii_y: [0.0; 4],
                    opacity: eff_style.opacity,
                });
            }
        }
        if paint_self
            && node.layout.collapsed_border_segments.is_empty()
            && bw.iter().any(|&w| w > 0.0)
        {
            let bx = br.x - eff_sx;
            let by = br.y - eff_sy;
            let border_rect = Rect::new(bx, by, br.w, br.h);
            let border_image_outsets = border_image_side_values(
                &eff_style.border_image_outset,
                bw,
                border_rect,
                font_px,
                false,
                ctx.transform_ctx.root_font_px,
            );
            let border_image_rect = Rect::new(
                border_rect.x - border_image_outsets[3],
                border_rect.y - border_image_outsets[0],
                border_rect.w + border_image_outsets[1] + border_image_outsets[3],
                border_rect.h + border_image_outsets[0] + border_image_outsets[2],
            );
            let border_image_widths = border_image_side_values(
                &eff_style.border_image_width,
                bw,
                border_image_rect,
                font_px,
                true,
                ctx.transform_ctx.root_font_px,
            );
            let mut painted_border_image = false;
            if let Some(src) = crate::css::extract_url(&eff_style.border_image_source) {
                if let Some((data, w, h)) =
                    crate::html::load_paint_image_from_src(&src, ctx.base_url)
                {
                    if w > 0 && h > 0 {
                        let (repeat_x_mode, repeat_y_mode) =
                            border_image_repeat_modes(&eff_style.border_image_repeat);
                        list.push(PaintCmd::BorderImage {
                            rect: border_image_rect,
                            widths: border_image_widths,
                            slices: border_image_slices(
                                &eff_style.border_image_slice,
                                w as f32,
                                h as f32,
                            ),
                            repeat_x_mode,
                            repeat_y_mode,
                            fill_center: eff_style
                                .border_image_slice
                                .split_whitespace()
                                .any(|part| part.eq_ignore_ascii_case("fill")),
                            data: ImageRef::Shared(data, w, h),
                        });
                        painted_border_image = true;
                    }
                }
            }
            if !painted_border_image
                && paint_gradient_border_image_fallback(
                    list,
                    border_image_rect,
                    border_image_widths,
                    &eff_style.border_image_source,
                    eff_style.opacity,
                )
            {
                painted_border_image = true;
            }
            if !painted_border_image {
                let mut border_widths = bw;
                let border_colors = [
                    eff_style.border_top_color,
                    eff_style.border_right_color,
                    eff_style.border_bottom_color,
                    eff_style.border_left_color,
                ];
                let mut border_styles = [
                    bstyle(eff_style.border_top_style),
                    bstyle(eff_style.border_right_style),
                    bstyle(eff_style.border_bottom_style),
                    bstyle(eff_style.border_left_style),
                ];
                for side in 0..4 {
                    if border_widths[side] <= 0.0 || border_styles[side] == 0 {
                        border_widths[side] = 0.0;
                        border_styles[side] = 0;
                    }
                }
                if border_widths
                    .iter()
                    .enumerate()
                    .any(|(side, width)| *width > 0.0 && border_colors[side].a > 0)
                {
                    list.push(PaintCmd::Border {
                        rect: border_rect,
                        widths: border_widths,
                        colors: border_colors,
                        styles: border_styles,
                        radii: radii_arr,
                        radii_y: radii_y_arr,
                        opacity: eff_style.opacity,
                    });
                }
            }
        }
    }

    // ── (g) Outline ──────────────────────────────────────────────────────────
    if paint_self
        && eff_style.outline_width > 0.0
        && eff_style.outline_style != crate::types::BorderStyle::None
    {
        let ofs = eff_style.outline_offset;
        let ow = eff_style.outline_width;
        let radius_delta = ofs + ow;
        let rx = br.x - eff_sx - ofs - ow;
        let ry = br.y - eff_sy - ofs - ow;
        let rw = br.w + 2.0 * (ofs + ow);
        let rh = br.h + 2.0 * (ofs + ow);
        let outline_radii = radii_arr.map(|r| (r + radius_delta).max(0.0));
        let outline_radii_y = radii_y_arr.map(|r| (r + radius_delta).max(0.0));
        list.push(PaintCmd::Outline {
            rect: Rect::new(rx, ry, rw, rh),
            width: ow,
            color: eff_style.outline_color,
            style: bstyle(eff_style.outline_style),
            offset: ofs,
            radii: outline_radii,
            radii_y: outline_radii_y,
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
        resolve_overflow_clip_margin(eff_style, font_px, node.layout.padding_rect.w, ctx.transform_ctx.root_font_px);
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

    let is_sc = is_scroll_container(eff_style);
    let sc_container = if is_sc {
        Some(StickyScrollContainer {
            scrollport: Rect::new(pr.x - eff_sx, pr.y - eff_sy, pr.w, pr.h),
        })
    } else {
        ctx.sticky_scroll_container
    };

    let cb_rect = sticky_containing_block_for_children(node, ctx.sticky_containing_block);

    let suppress_z = ctx.suppress_deferred_z_descendants && !stacking;

    let child_ctx = BuildContext {
        scroll_x: child_sx,
        scroll_y: child_sy,
        sticky_scroll_x: ctx.sticky_scroll_x,
        sticky_scroll_y: ctx.sticky_scroll_y,
        sticky_scroll_container: sc_container,
        sticky_containing_block: cb_rect,
        hovered_id: ctx.hovered_id,
        active_id: ctx.active_id,
        visited_hrefs: ctx.visited_hrefs,
        base_url: ctx.base_url,
        clip: child_clip,
        paint_clip: ctx.paint_clip,
        suppress_deferred_z_descendants: suppress_z,
        font_system: ctx.font_system,
        transform_ctx: ctx.transform_ctx,
    };

    let contents_visible = !matches!(eff_style.content_visibility, ContentVisibility::Hidden);
    if contents_visible {
        // ── (i) Negative z-index children (paint behind text) ────────────────
        if !suppress_z {
            let eff_children = node.effective_children();
            let mut negative_z = Vec::new();
            for child in eff_children {
                collect_explicit_z_descendants(
                    child,
                    child_ctx.sticky_containing_block,
                    &mut negative_z,
                );
            }
            negative_z.retain(|(c, _)| c.style.z_index < 0 && !c.style.z_index_is_auto);
            negative_z.sort_by_key(|(c, _)| c.style.z_index);
            let mut z_ctx = child_ctx;
            z_ctx.suppress_deferred_z_descendants = false;
            for (child, containing_block) in negative_z {
                z_ctx.sticky_containing_block = containing_block;
                build_positioned_box(child, list, &z_ctx);
            }
        }

        // ── (j) ::before pseudo-element (fallback when line_cache is empty) ────
        if !node.style.before_content.is_empty()
            && node.layout.line_cache.is_empty()
            && !matches!(node.style.display, Display::Inline)
        {
            emit_generated_pseudo_content(
                list,
                node,
                &node.style.before_content,
                node.style.before_style.as_deref().unwrap_or(&node.style),
                eff_sx,
                eff_sy,
                ctx.transform_ctx.root_font_px,
            );
        }

        // ── (k) Inline text content (line_cache) ─────────────────────────────
        //
        // Flex/grid containers usually lay out children as independent items,
        // but some inline descendant trees are represented by the container's
        // own line cache. Suppress the container cache only when a direct child
        // element has its own line cache to paint; otherwise real page text can
        // disappear while avoiding duplicate button/link labels.
        let is_flex_or_grid = matches!(
            node.style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        );
        let paints_own_inline_text =
            !is_flex_or_grid || !direct_child_element_has_owned_inline_text_paint(node);
        if paint_self && paints_own_inline_text && !node.layout.line_cache.is_empty() {
            push_inline_descendant_box_backgrounds(
                node,
                list,
                child_sx,
                child_sy,
                ctx.paint_clip,
                ctx.transform_ctx.root_font_px,
            );
            build_inline_text(
                node,
                eff_style,
                list,
                ctx,
                child_sx,
                child_sy,
                is_hovered,
                is_active,
                ctx.paint_clip,
            );
        }
        if paint_self
            && node.is_pseudo_element()
            && !node.text.is_empty()
            && node.layout.line_cache.is_empty()
        {
            emit_generated_pseudo_content(list, node, &node.text, &node.style, child_sx, child_sy, ctx.transform_ctx.root_font_px);
        }

        // ── (l) ::after pseudo-element (fallback when line_cache is empty) ─────
        if !node.style.after_content.is_empty()
            && node.layout.line_cache.is_empty()
            && !matches!(node.style.display, Display::Inline)
        {
            let mut after_offset = 0.0f32;
            if !node.style.before_content.is_empty() {
                let ps = node.style.before_style.as_deref().unwrap_or(&node.style);
                let fpx = ps.font_size_px(ctx.transform_ctx.root_font_px, ctx.transform_ctx.root_font_px).max(1.0);
                after_offset = node.style.before_content.chars().count() as f32 * fpx * 0.55 + 6.0;
            }
            emit_generated_pseudo_content_offset(
                list,
                node,
                &node.style.after_content,
                node.style.after_style.as_deref().unwrap_or(&node.style),
                eff_sx,
                eff_sy,
                after_offset,
                ctx.transform_ctx.root_font_px,
            );
        }

        // ── (m) List markers ─────────────────────────────────────────────────
        if paint_self
            && node.style.display == Display::ListItem
            && !node.layout.line_cache.is_empty()
        {
            build_list_marker(node, list, ctx, eff_sx, eff_sy);
        }

        // ── (n) HR ───────────────────────────────────────────────────────────
        if paint_self && node.tag == "hr" {
            let cr = node.layout.border_rect;
            let y_hr = cr.y + cr.h / 2.0 - eff_sy;
            list.push(PaintCmd::HorizontalRule {
                x1: cr.x - eff_sx,
                y1: y_hr,
                x2: cr.right() - eff_sx,
            });
        }

        // ── (o) Form elements (content only — box decoration handled by CSS steps above)
        if paint_self {
            build_form_element(node, list, eff_sx, eff_sy, ctx.transform_ctx.root_font_px);
        }

        // ── (p) Image / SVG / Canvas / Media ────────────────────────────────
        //
        // `<canvas>` joins `<img>` here because by this point it IS one: a canvas
        // keeps its bitmap in `image_data` exactly as a decoded image does, and
        // both are premultiplied RGBA, which is what `PixmapRef::from_bytes` at
        // replay reads. Everything the 2D context drew is already in those bytes,
        // so painting a canvas is painting its bitmap and nothing else — which is
        // also what the spec says a canvas is.
        if paint_self && (node.tag == "video" || node.tag == "audio") {
            crate::video::build_media_element(node, list, eff_sx, eff_sy);
        }

        if paint_self && (node.is_image_element() || node.tag == "svg" || node.tag == "canvas") {
            if let Some(ref data) = node.image_data {
                if node.image_width > 0 && node.image_height > 0 {
                    let cr = node.layout.content_rect;
                    let (dst, clip) = object_fit_rect(
                        &node.style,
                        cr,
                        node.image_width as f32,
                        node.image_height as f32,
                        font_px,
                        ctx.transform_ctx.root_font_px,
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
                        data: ImageRef::Shared(
                            data.clone(),
                            node.image_data_width.max(1),
                            node.image_data_height.max(1),
                        ),
                    });
                    if clip || clips_radius {
                        list.push(PaintCmd::PopClip);
                    }
                }
            } else if node.tag == "svg" || (node.is_image_element() && node.svg_document.is_some())
            {
                // SVG: rasterize the parsed native tree on demand at the
                // layout-determined size.
                if node.svg_document.is_some() {
                    let cr = node.layout.content_rect;
                    if cr.w > 0.0 && cr.h > 0.0 {
                        let raster_w = cr.w.round() as u32;
                        let raster_h = cr.h.round() as u32;
                        if raster_w > 0 && raster_h > 0 {
                            let c = node.style.color;
                            let rgba = if let Some(ref doc) = node.svg_document {
                                let sampled_overrides;
                                let overrides = if node.svg_animation_overrides.is_empty() {
                                    let mut running = false;
                                    sampled_overrides =
                                        crate::svg::animation::sample_svg_animation_overrides(
                                            doc,
                                            0.0,
                                            &mut running,
                                        );
                                    sampled_overrides.as_slice()
                                } else {
                                    node.svg_animation_overrides.as_slice()
                                };
                                let animated_doc;
                                let doc = if overrides.is_empty() {
                                    doc
                                } else {
                                    animated_doc =
                                        crate::svg::svg_document_with_animation_overrides(
                                            doc, overrides,
                                        );
                                    &animated_doc
                                };
                                let (current_color, fill, stroke) = if node.tag == "svg" {
                                    let specified_svg_paint =
                                        node.style.rare().specified_svg_paint_props;
                                    let fill = (specified_svg_paint
                                        & crate::types::SPECIFIED_SVG_FILL
                                        != 0)
                                        .then_some(node.style.svg_fill)
                                        .flatten();
                                    let stroke = (specified_svg_paint
                                        & crate::types::SPECIFIED_SVG_STROKE
                                        != 0)
                                        .then_some(node.style.svg_stroke)
                                        .flatten();
                                    (c, fill, stroke)
                                } else {
                                    (
                                        crate::types::Color::BLACK,
                                        Some(crate::types::Color::BLACK),
                                        None,
                                    )
                                };
                                if node.tag == "svg" {
                                    crate::svg::rasterize_svg_document_to_rgba_with_dom(
                                        doc,
                                        raster_w,
                                        raster_h,
                                        (node.svg_viewbox_w, node.svg_viewbox_h),
                                        current_color,
                                        fill,
                                        stroke,
                                        &node.style.custom_props,
                                        Some(node),
                                    )
                                } else {
                                    crate::svg::rasterize_svg_document_to_rgba(
                                        doc,
                                        raster_w,
                                        raster_h,
                                        (node.svg_viewbox_w, node.svg_viewbox_h),
                                        current_color,
                                        fill,
                                        stroke,
                                    )
                                }
                            } else {
                                None
                            };
                            if let Some(rgba) = rgba {
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
                                    data: ImageRef::Shared(
                                        std::sync::Arc::new(rgba),
                                        raster_w,
                                        raster_h,
                                    ),
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
            // Fixed boxes are deferred with other positioned descendants.
        {
            let eff_children = node.effective_children();
            let is_renderable = |c: &WebCore| -> bool {
                !matches!(c.style.display, Display::None)
                    && (c.tag != "#text"
                        || (!c.text.trim().is_empty()
                            && !matches!(node.style.display, Display::Inline)
                            && (matches!(
                                node.style.display,
                                Display::Flex
                                    | Display::InlineFlex
                                    | Display::Grid
                                    | Display::InlineGrid
                            ) || node.layout.line_cache.is_empty())))
                    && (c.tag != "::before" || c.style.is_positioned() || c.style.is_block_level())
                    && (c.tag != "::after" || c.style.is_positioned() || c.style.is_block_level())
                    && c.style.position != Position::Fixed
            };

            let mut deferred_z = Vec::new();
            if !suppress_z {
                for child in eff_children {
                    collect_explicit_z_descendants(
                        child,
                        child_ctx.sticky_containing_block,
                        &mut deferred_z,
                    );
                }
            }

            let mut normal_ctx = child_ctx;
            normal_ctx.suppress_deferred_z_descendants = suppress_z || !deferred_z.is_empty();

            if !matches!(node.tag.as_str(), "input" | "select" | "textarea" | "progress" | "meter") {
                for child in eff_children {
                    if is_renderable(child) {
                        build_for_box(child, list, &normal_ctx);
                    }
                }
            }

            deferred_z.retain(|(c, _)| {
                (is_renderable(c) || c.style.position == Position::Fixed)
                    && (c.style.z_index_is_auto || c.style.z_index >= 0)
            });
            deferred_z.sort_by_key(|(c, _)| {
                if c.style.z_index_is_auto {
                    0
                } else {
                    c.style.z_index
                }
            });
            let mut z_ctx = child_ctx;
            z_ctx.suppress_deferred_z_descendants = false;
            for (child, containing_block) in deferred_z {
                z_ctx.sticky_containing_block = containing_block;
                build_positioned_box(child, list, &z_ctx);
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
    if has_text_gradient {
        list.push(PaintCmd::PopTextGradient);
    }
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
    match style.gradient_radial_size {
        GradientRadialSize::ClosestSide => match style.gradient_radial_shape {
            GradientRadialShape::Circle => {
                let r = left.min(right).min(top.min(bottom)).max(1.0);
                (r, r)
            }
            GradientRadialShape::Ellipse => (left.min(right).max(1.0), top.min(bottom).max(1.0)),
        },
        GradientRadialSize::FarthestSide => match style.gradient_radial_shape {
            GradientRadialShape::Circle => {
                let r = left.max(right).max(top.max(bottom)).max(1.0);
                (r, r)
            }
            GradientRadialShape::Ellipse => (left.max(right).max(1.0), top.max(bottom).max(1.0)),
        },
        GradientRadialSize::ClosestCorner => {
            let dx = left.min(right);
            let dy = top.min(bottom);
            match style.gradient_radial_shape {
                GradientRadialShape::Circle => {
                    let r = dx.hypot(dy).max(1.0);
                    (r, r)
                }
                GradientRadialShape::Ellipse => (
                    (dx * std::f32::consts::SQRT_2).max(1.0),
                    (dy * std::f32::consts::SQRT_2).max(1.0),
                ),
            }
        }
        GradientRadialSize::FarthestCorner => {
            let dx = left.max(right);
            let dy = top.max(bottom);
            match style.gradient_radial_shape {
                GradientRadialShape::Circle => {
                    let r = dx.hypot(dy).max(1.0);
                    (r, r)
                }
                GradientRadialShape::Ellipse => (
                    (dx * std::f32::consts::SQRT_2).max(1.0),
                    (dy * std::f32::consts::SQRT_2).max(1.0),
                ),
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Inline text using EXACT layout positions
// ═══════════════════════════════════════════════════════════════════════════════

fn build_inline_text(
    node: &WebCore,
    eff_style: &ComputedStyle,
    list: &mut DisplayList,
    ctx: &BuildContext<'_>,
    sx: f32,
    sy: f32,
    _is_hovered: bool,
    _is_active: bool,
    paint_clip: Rect,
) {
    let flat = crate::layout::inline_layout::collect_flat_text(node);
    if flat.is_empty() {
        return;
    }

    let opacity = eff_style.opacity;
    let root_font_px = ctx.transform_ctx.root_font_px;
    let fallback_font_px = node.style.font_size_px(root_font_px, root_font_px).max(1.0);
    let fallback_letter_spc = node
        .style
        .letter_spacing
        .resolve(fallback_font_px, 0.0, root_font_px);
    let fallback_word_spc = node
        .style
        .word_spacing
        .resolve(fallback_font_px, 0.0, root_font_px);

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
            .resolve(fallback_font_px, node.layout.content_rect.w, root_font_px);
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
        if ly + line.height < paint_clip.y || ly > paint_clip.bottom() {
            continue;
        }

        // ── Build chunks from inline_runs / visual_segments ──────────────
        struct Chunk {
            s: usize,
            e: usize,
            run_idx: Option<usize>,
            rtl: bool,
            visual_x: Option<f32>,
            visual_w: f32,
            starts_visual_segment: bool,
        }
        let mut chunks: Vec<Chunk> = Vec::new();

        let has_rtl_visual_segments = line.visual_segments.iter().any(|vs| (vs.level & 1) != 0);
        let touched_inline_runs = node
            .layout
            .inline_runs
            .iter()
            .filter(|run| run.text_offset < line_end && run.text_offset + run.length > line_start)
            .count();
        let use_bidi_visual_segments = has_rtl_visual_segments
            && touched_inline_runs <= 1
            && !node.layout.inline_runs.is_empty();
        if use_bidi_visual_segments {
            // BiDi: use visual segment order
            for vs in &line.visual_segments {
                let seg_s = vs.logical_start;
                let seg_e = vs.logical_start + vs.length;
                let is_rtl = (vs.level & 1) != 0;
                if is_rtl {
                    let mut s = seg_s;
                    let mut e = seg_e.min(flat.len());
                    while s < e && matches!(flat.as_bytes()[s], b' ' | b'\t' | b'\n' | b'\r') {
                        s += 1;
                    }
                    while e > s && matches!(flat.as_bytes()[e - 1], b' ' | b'\t' | b'\n' | b'\r') {
                        e -= 1;
                    }
                    if s < e {
                        chunks.push(Chunk {
                            s,
                            e,
                            run_idx: None,
                            rtl: true,
                            visual_x: Some(vs.x),
                            visual_w: vs.width,
                            starts_visual_segment: true,
                        });
                    }
                    continue;
                }
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
                            visual_x: None,
                            visual_w: 0.0,
                            starts_visual_segment: false,
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
                for chunk in &mut seg_chunks {
                    chunk.visual_x = Some(vs.x);
                    chunk.visual_w = vs.width;
                }
                if let Some(first) = seg_chunks.first_mut() {
                    first.starts_visual_segment = true;
                }
                chunks.extend(seg_chunks);
            }
        } else if node.layout.inline_runs.is_empty() {
            chunks.push(Chunk {
                s: line_start,
                e: line_end,
                run_idx: None,
                rtl: false,
                visual_x: None,
                visual_w: 0.0,
                starts_visual_segment: false,
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
                        visual_x: None,
                        visual_w: 0.0,
                        starts_visual_segment: false,
                    });
                }
            }
        }

        let mut cursor_x = lx + line.text_x_offset;
        let mut previous_collapsible_space = false;
        let mut previous_logical_end: Option<usize> = None;
        let mut first_letter_painted = false;

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let s = floor_cb(&flat, chunk.s);
            let e = floor_cb(&flat, chunk.e);
            if e <= s {
                continue;
            }

            let (run_style, run_font_px, run_letter_spc, run_word_spc, _run_extra) =
                if let Some(ri) = chunk.run_idx {
                    let run = &node.layout.inline_runs[ri];
                    let fp = run.style.font_size_px(fallback_font_px, root_font_px).max(1.0);
                    let ls = run.style.letter_spacing.resolve(fp, 0.0, root_font_px);
                    let ws = run.style.word_spacing.resolve(fp, 0.0, root_font_px);
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
            let paint_style_ref: &ComputedStyle = chunk
                .run_idx
                .and_then(|ri| node.layout.inline_runs.get(ri))
                .and_then(|run| node_at_relative_path(node, &run.path))
                .map(|source| source.style.as_ref())
                .unwrap_or(style_ref);
            let (relative_dx, relative_dy) = chunk
                .run_idx
                .and_then(|ri| node.layout.inline_runs.get(ri))
                .map(|run| inline_relative_visual_offset(node, &run.path, root_font_px))
                .unwrap_or((0.0, 0.0));
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
            let mut leading_draw_advance = 0.0f32;
            if !chunk.rtl && collapses_spaces_for_paint(style_ref.white_space) {
                if let Some(visible_start) = draw_text
                    .char_indices()
                    .find_map(|(idx, ch)| (!ch.is_whitespace()).then_some(idx))
                {
                    if visible_start > 0 {
                        let leading = &draw_text[..visible_start];
                        if previous_logical_end.is_some() {
                            leading_draw_advance = measure_paint_text_width(
                                ctx,
                                leading,
                                run_font_px,
                                style_ref.font_weight,
                                style_ref.font_style,
                                &style_ref.font_family,
                                style_ref.font_stretch,
                            ) + run_letter_spc * leading.chars().count() as f32
                                + run_word_spc * leading.chars().filter(|&c| c == ' ').count() as f32
                                + line.extra_space_per_word
                                    * leading.chars().filter(|&c| c == ' ').count() as f32;
                        }
                        draw_text = draw_text[visible_start..].to_string();
                    }
                } else if !draw_text.is_empty() {
                    draw_text.clear();
                }
            }
            if !chunk.rtl
                && collapses_spaces_for_paint(style_ref.white_space)
                && should_insert_missing_inline_gap(&draw_text)
                && let Some(prev_end) = previous_logical_end
            {
                let gap_start = floor_cb(&flat, prev_end.min(flat.len()));
                if gap_start < s {
                    let gap = &flat[gap_start..s];
                    if !gap.is_empty() && gap.chars().all(char::is_whitespace) {
                        let char_gap = if !line.char_x.is_empty() {
                            let start_off = gap_start.saturating_sub(line_start);
                            let end_off = s.saturating_sub(line_start);
                            if start_off < line.char_x.len() && end_off < line.char_x.len() {
                                let start = line.char_x[start_off];
                                let end = line.char_x[end_off];
                                if start.is_finite() && end.is_finite() {
                                    (end - start).max(0.0)
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        let gap_w = if char_gap > 0.1 {
                            char_gap
                        } else {
                            measure_paint_text_width(
                                ctx,
                                " ",
                                run_font_px,
                                style_ref.font_weight,
                                style_ref.font_style,
                                &style_ref.font_family,
                                style_ref.font_stretch,
                            ) + run_letter_spc
                                + run_word_spc
                                + line.extra_space_per_word
                        };
                        cursor_x += gap_w;
                    }
                }
            }

            let run_line_h = if line.height > 0.0 {
                line.height
            } else {
                style_ref
                    .line_height
                    .resolve(run_font_px, 0.0, root_font_px)
                    .max(run_font_px * 1.2)
            };

            // Use char_x for exact x position if available.
            // For RTL chunks, char_x byte offsets don't correspond to visual
            // position (logical byte 0 of Arabic maps to the rightmost glyph).
            // Use cursor_x instead, which advances in visual order.
            let char_x_start_end = if !chunk.rtl && !line.char_x.is_empty() {
                let start_off = s.saturating_sub(line_start);
                let end_off = e.saturating_sub(line_start);
                if start_off < line.char_x.len() && end_off < line.char_x.len() {
                    let start = line.char_x[start_off];
                    let end = line.char_x[end_off];
                    if start.is_finite() && end.is_finite() && end > start {
                        Some((start, end))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if chunk.starts_visual_segment {
                if let Some(visual_x) = chunk.visual_x {
                    cursor_x = lx + line.text_x_offset + visual_x;
                }
            }

            let measured_advance = measure_paint_text_width(
                ctx,
                &draw_text,
                run_font_px,
                style_ref.font_weight,
                style_ref.font_style,
                &style_ref.font_family,
                style_ref.font_stretch,
            ) + run_letter_spc * draw_text.chars().count() as f32
                + run_word_spc * draw_text.chars().filter(|&c| c == ' ').count() as f32;

            let layout_advance = if !line.char_x.is_empty() {
                let start_off = s.saturating_sub(line_start);
                let end_off = e.saturating_sub(line_start);
                if start_off < line.char_x.len() && end_off < line.char_x.len() {
                    let advance = (line.char_x[end_off] - line.char_x[start_off]).abs();
                    if advance > 0.0 && advance.is_finite() {
                        Some(advance)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let mut chunk_advance = layout_advance.unwrap_or(measured_advance);
            if chunk.rtl {
                if let Some(visual_x) = chunk.visual_x {
                    let segment_right = lx + line.text_x_offset + visual_x + chunk.visual_w;
                    let remaining = (segment_right - cursor_x).max(0.0);
                    if remaining.is_finite() && remaining > 0.0 {
                        chunk_advance = chunk_advance.min(remaining);
                    }
                }
            };

            if draw_text.is_empty() {
                if previous_logical_end.is_some() {
                    cursor_x += chunk_advance;
                    previous_logical_end = Some(e);
                }
                continue;
            }

            let x_pos = if let Some((start, _)) = char_x_start_end {
                lx + line.text_x_offset + start
            } else {
                cursor_x
            };
            let v_shift = vertical_align_y_shift(&style_ref.vertical_align, run_font_px);
            let is_vertical = !crate::types::inline_axis_is_horizontal(node.style.writing_mode);
            let (x_pos, y_pos) = if is_vertical {
                (x_pos + v_shift, ly)
            } else {
                (x_pos, ly + v_shift)
            };
            let x_pos = x_pos + leading_draw_advance + relative_dx;
            let y_pos = y_pos + relative_dy;
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
                let marker_width = measure_paint_text_width(
                    ctx,
                    overflow_marker,
                    run_font_px,
                    style_ref.font_weight,
                    style_ref.font_style,
                    &style_ref.font_family,
                    style_ref.font_stretch,
                );
                let budget = (available - marker_width).max(0.0);
                let full_width = measure_paint_text_width(
                    ctx,
                    &draw_text,
                    run_font_px,
                    style_ref.font_weight,
                    style_ref.font_style,
                    &style_ref.font_family,
                    style_ref.font_stretch,
                );
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
                            let width = measure_paint_text_width(
                                ctx,
                                &candidate,
                                run_font_px,
                                style_ref.font_weight,
                                style_ref.font_style,
                                &style_ref.font_family,
                                style_ref.font_stretch,
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
            let run_background = if paint_style_ref.background_color.a > 0 {
                paint_style_ref.background_color
            } else {
                style_ref.background_color
            };
            if run_background.a > 0
                && style_ref.background_clip != BackgroundClip::Text
                && paint_style_ref.background_clip != BackgroundClip::Text
            {
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
                    color: run_background,
                    radius: [0.0; 4],
                    radius_y: [0.0; 4],
                });
            }

            let run_color = paint_style_ref.color;
            let alpha = ((run_color.a as f32) * opacity) as u8;
            let text_color = Color::rgba(run_color.r, run_color.g, run_color.b, alpha);

            // Text shadow
            if let Some(ref ts) = style_ref.text_shadow {
                let shadow_alpha = ((ts.color.a as f32) * opacity).round().clamp(0.0, 255.0) as u8;
                let shadow_color = Color::rgba(ts.color.r, ts.color.g, ts.color.b, shadow_alpha);
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
                    color: shadow_color,
                    blur: ts.blur,
                    letter_spacing: run_letter_spc,
                    word_spacing: run_word_spc,
                    small_caps: style_ref.small_caps,
                });
            }

            // Main text
            let deco_t = style_ref
                .text_decoration_thickness
                .resolve(run_font_px, 0.0, root_font_px);
            let underline_offset = style_ref
                .text_underline_offset
                .resolve(run_font_px, 0.0, root_font_px);
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

            let mut emit_text_run = |text: String,
                                     x: f32,
                                     style: &ComputedStyle,
                                     font_px: f32,
                                     line_h: f32,
                                     color: Color,
                                     letter_spacing: f32,
                                     word_spacing: f32| {
                list.push(PaintCmd::Text {
                    x,
                    y: y_pos,
                    text,
                    font_family: style.font_family.clone(),
                    font_size: font_px,
                    font_weight: style.font_weight.value(),
                    font_style: match style.font_style {
                        FontStyle::Italic => 1,
                        FontStyle::Oblique => 2,
                        _ => 0,
                    },
                    font_stretch: style.font_stretch,
                    line_height: line_h,
                    color,
                    decoration: TextDecoration {
                        underline: style.text_decoration.underline,
                        overline: style.text_decoration.overline,
                        strikethrough: style.text_decoration.strikethrough,
                        color: style.text_decoration_color.unwrap_or(color),
                        style: match style.text_decoration_style {
                            TextDecorationStyle::Double => 1,
                            TextDecorationStyle::Dotted => 2,
                            TextDecorationStyle::Dashed => 3,
                            TextDecorationStyle::Wavy => 4,
                            _ => 0,
                        },
                        thickness: if deco_t > 0.0 {
                            deco_t
                        } else {
                            (font_px / 12.0).max(1.0)
                        },
                        underline_offset,
                        underline_position: style.text_underline_position,
                        skip_ink: !style.text_decoration_skip_ink.eq_ignore_ascii_case("none"),
                    },
                    letter_spacing,
                    word_spacing,
                    small_caps: style.small_caps,
                });
            };

            if !first_letter_painted
                && !chunk.rtl
                && let Some(first_style) = node.style.first_letter_style.as_deref()
                && let Some((first_start, first_ch)) =
                    draw_text.char_indices().find(|(_, ch)| !ch.is_whitespace())
            {
                first_letter_painted = true;
                let first_end = first_start + first_ch.len_utf8();
                let leading = &draw_text[..first_start];
                let first = &draw_text[first_start..first_end];
                let trailing = &draw_text[first_end..];
                let mut letter_x = x_pos;
                if !leading.is_empty() {
                    emit_text_run(
                        leading.to_string(),
                        x_pos,
                        style_ref,
                        run_font_px,
                        run_line_h,
                        text_color,
                        letter_sp,
                        run_word_spc,
                    );
                    letter_x += measure_paint_text_width(
                        ctx,
                        leading,
                        run_font_px,
                        style_ref.font_weight,
                        style_ref.font_style,
                        &style_ref.font_family,
                        style_ref.font_stretch,
                    ) + letter_sp * leading.chars().count() as f32
                        + run_word_spc * leading.chars().filter(|&c| c == ' ').count() as f32;
                }
                let first_font_px = first_style
                    .font_size_px(fallback_font_px, root_font_px)
                    .max(1.0);
                let first_line_h = first_style
                    .line_height
                    .resolve(first_font_px, 0.0, root_font_px)
                    .max(first_font_px * 1.2);
                let first_color = first_style.color;
                let first_alpha = ((first_color.a as f32) * opacity) as u8;
                let first_text_color =
                    Color::rgba(first_color.r, first_color.g, first_color.b, first_alpha);
                let first_letter_sp = first_style
                    .letter_spacing
                    .resolve(first_font_px, 0.0, root_font_px);
                let first_word_sp = first_style
                    .word_spacing
                    .resolve(first_font_px, 0.0, root_font_px);
                emit_text_run(
                    first.to_string(),
                    letter_x,
                    first_style,
                    first_font_px,
                    first_line_h,
                    first_text_color,
                    first_letter_sp,
                    first_word_sp,
                );
                if !trailing.is_empty() {
                    let first_w = measure_paint_text_width(
                        ctx,
                        first,
                        first_font_px,
                        first_style.font_weight,
                        first_style.font_style,
                        &first_style.font_family,
                        first_style.font_stretch,
                    ) + first_letter_sp;
                    emit_text_run(
                        trailing.to_string(),
                        letter_x + first_w,
                        style_ref,
                        run_font_px,
                        run_line_h,
                        text_color,
                        letter_sp,
                        run_word_spc,
                    );
                }
            } else {
                emit_text_run(
                    draw_text.clone(),
                    x_pos,
                    style_ref,
                    run_font_px,
                    run_line_h,
                    text_color,
                    letter_sp,
                    run_word_spc,
                );
            }

            // Advance cursor using char_x if available and sane. Some relayout
            // paths can leave byte-offset caret positions collapsed at a run
            // boundary; trusting those positions paints all chunks at one x.
            if chunk.rtl {
                cursor_x += chunk_advance;
            } else if let Some((_, end)) = char_x_start_end {
                let next = lx + line.text_x_offset + end;
                if next > cursor_x + 0.5 {
                    cursor_x = next;
                } else {
                    cursor_x += chunk_advance;
                }
                cursor_x = cursor_x.max(x_pos + measured_advance);
            } else {
                cursor_x += chunk_advance;
                cursor_x = cursor_x.max(x_pos + measured_advance);
            }
            previous_logical_end = Some(e);
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
        .map(|s| s.font_size_px(ctx.transform_ctx.root_font_px, ctx.transform_ctx.root_font_px))
        .unwrap_or_else(|| node.style.font_size_px(ctx.transform_ctx.root_font_px, ctx.transform_ctx.root_font_px));
    let fallback_line_y = node.layout.content_rect.y;
    let fallback_line_h = node
        .style
        .line_height
        .resolve(font_px, font_px, ctx.transform_ctx.root_font_px)
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
                .resolve(font_px, font_px, ctx.transform_ctx.root_font_px)
                .max(font_px * 1.2)
        })
        .unwrap_or_else(|| {
            node.style
                .line_height
                .resolve(font_px, font_px, ctx.transform_ctx.root_font_px)
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
        let image =
            crate::html::load_paint_image_from_src(&node.style.list_style_image, ctx.base_url)
                .map(|(data, w, h)| ImageRef::Shared(data, w, h));
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
        ListStyleType::DisclosureOpen | ListStyleType::DisclosureClosed => {
            let mx = if inside {
                line_x - sx
            } else {
                line_x - sx - 4.0
            };
            let my = line_y - sy;
            let text = if matches!(node.style.list_style_type, ListStyleType::DisclosureOpen) {
                "\u{25be}"
            } else {
                "\u{25b8}"
            };
            list.push(PaintCmd::ListMarker {
                marker_type: 3,
                x: mx,
                y: my,
                size: 0.0,
                color: c,
                text: text.to_string(),
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

fn build_form_element(node: &WebCore, list: &mut DisplayList, sx: f32, sy: f32, root_font_px: f32) {
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
    let font_px = node.style.font_size_px(root_font_px, root_font_px).max(1.0);
    let mut value_color = node.style.color;
    let background = node.style.background_color;
    if matches!(tag, "input" | "textarea")
        && value_color.a == 255
        && background.a == 255
        && (value_color.r, value_color.g, value_color.b)
            == (background.r, background.g, background.b)
    {
        let brightness = 299u32 * background.r as u32
            + 587u32 * background.g as u32
            + 114u32 * background.b as u32;
        value_color = if brightness < 128_000 {
            Color::WHITE
        } else {
            Color::BLACK
        };
    }
    value_color.a = ((value_color.a as f32) * node.style.opacity).round() as u8;
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
        color: value_color,
        text_indent: node.style.text_indent.resolve_vp(
            font_px,
            cr.w,
            font_px,
            0.0,
            0.0,
        ),
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
        file_button_color: node
            .style
            .file_selector_button_style
            .as_ref()
            .map(|s| s.color)
            .unwrap_or(node.style.color),
        file_button_background: node
            .style
            .file_selector_button_style
            .as_ref()
            .map(|s| s.background_color)
            .unwrap_or(Color::TRANSPARENT),
        file_button_font_size: node
            .style
            .file_selector_button_style
            .as_ref()
            .map(|s| s.font_size_px(font_px, font_px))
            .unwrap_or(font_px),
        file_button_font_weight: node
            .style
            .file_selector_button_style
            .as_ref()
            .map(|s| s.font_weight.value())
            .unwrap_or_else(|| node.style.font_weight.value()),
        file_button_font_family: node
            .style
            .file_selector_button_style
            .as_ref()
            .map(|s| s.font_family.clone())
            .unwrap_or_else(|| node.style.font_family.clone()),
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

fn build_laid_out_text_node(
    node: &WebCore,
    list: &mut DisplayList,
    ctx: &BuildContext<'_>,
    sx: f32,
    sy: f32,
    paint_clip: Rect,
) {
    if !node.layout.line_cache.is_empty() {
        build_inline_text(
            node,
            &node.style,
            list,
            ctx,
            sx,
            sy,
            false,
            false,
            paint_clip,
        );
        return;
    }

    let text = if matches!(
        node.style.white_space,
        WhiteSpace::Normal | WhiteSpace::Nowrap
    ) {
        node.text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        node.text.clone()
    };
    if text.trim().is_empty() {
        return;
    }

    let font_px = node.style.font_size_px(ctx.transform_ctx.root_font_px, ctx.transform_ctx.root_font_px).max(1.0);
    let line_h = node
        .style
        .line_height
        .resolve(font_px, 0.0, ctx.transform_ctx.root_font_px)
        .max(font_px * 1.2);
    let y = node.layout.content_rect.y - sy;
    if y + line_h < paint_clip.y || y > paint_clip.bottom() {
        return;
    }
    emit_text(
        list,
        node.layout.content_rect.x - sx,
        y,
        &apply_text_transform(&text, node.style.text_transform),
        &node.style,
        font_px,
        line_h,
        ctx.transform_ctx.root_font_px,
    );
}

fn emit_generated_pseudo_content(
    list: &mut DisplayList,
    node: &WebCore,
    text: &str,
    style: &ComputedStyle,
    sx: f32,
    sy: f32,
    root_font_px: f32,
) {
    emit_generated_pseudo_content_offset(list, node, text, style, sx, sy, 0.0, root_font_px);
}

fn emit_generated_pseudo_content_offset(
    list: &mut DisplayList,
    node: &WebCore,
    text: &str,
    style: &ComputedStyle,
    sx: f32,
    sy: f32,
    offset_x: f32,
    root_font_px: f32,
) {
    if text.is_empty() {
        return;
    }
    let font_px = style.font_size_px(node.style.font_size_px(root_font_px, root_font_px), root_font_px).max(1.0);
    let line_h = style
        .line_height
        .resolve(font_px, 0.0, root_font_px)
        .max(font_px * 1.2);
    let mut x = node.layout.content_rect.x - sx + offset_x;
    let text_w = text.chars().count() as f32 * font_px * 0.55;
    match style.text_align {
        TextAlign::Center => {
            x += ((node.layout.content_rect.w - text_w) * 0.5).max(0.0);
        }
        TextAlign::Right | TextAlign::End if !matches!(style.direction, Direction::RTL) => {
            x += (node.layout.content_rect.w - text_w).max(0.0);
        }
        TextAlign::Left | TextAlign::Start if matches!(style.direction, Direction::RTL) => {
            x += (node.layout.content_rect.w - text_w).max(0.0);
        }
        _ => {}
    }
    let y = node.layout.content_rect.y - sy + (node.layout.content_rect.h - line_h) * 0.5;
    emit_text(list, x, y, text, style, font_px, line_h, root_font_px);
}

fn emit_text(
    list: &mut DisplayList,
    x: f32,
    y: f32,
    text: &str,
    style: &ComputedStyle,
    font_px: f32,
    line_h: f32,
    root_font_px: f32,
) {
    if text.is_empty() || font_px <= 0.0 {
        return;
    }
    let fp = font_px.max(1.0);
    let lh = if line_h > 0.0 { line_h } else { fp * 1.2 };
    let letter_sp = style.letter_spacing.resolve(fp, 0.0, root_font_px);
    let word_sp = style.word_spacing.resolve(fp, 0.0, root_font_px);
    let deco_t = style.text_decoration_thickness.resolve(fp, 0.0, root_font_px);
    let underline_offset = style.text_underline_offset.resolve(fp, 0.0, root_font_px);
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

fn resolve_overflow_clip_margin(style: &ComputedStyle, font_px: f32, reference: f32, root_font_px: f32) -> f32 {
    style
        .overflow_clip_margin
        .split_whitespace()
        .find_map(crate::css::parse_length_checked)
        .map(|length| length.resolve(font_px, reference, root_font_px).max(0.0))
        .unwrap_or(0.0)
}

fn clip_path_rect(
    style: &ComputedStyle,
    border_rect: Rect,
    scroll_x: f32,
    scroll_y: f32,
    font_px: f32,
    root_font_px: f32,
) -> Option<(Rect, [f32; 4])> {
    match style.clip_path.kind {
        ClipPathKind::Inset => {
            let top = style
                .clip_path
                .inset_top
                .resolve(font_px, border_rect.h, root_font_px);
            let right = style
                .clip_path
                .inset_right
                .resolve(font_px, border_rect.w, root_font_px);
            let bottom = style
                .clip_path
                .inset_bottom
                .resolve(font_px, border_rect.h, root_font_px);
            let left = style
                .clip_path
                .inset_left
                .resolve(font_px, border_rect.w, root_font_px);
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
                .resolve(font_px, reference, root_font_px)
                .max(0.0);
            let cx = border_rect.x
                + style
                    .clip_path
                    .center_x
                    .resolve(font_px, border_rect.w, root_font_px);
            let cy = border_rect.y
                + style
                    .clip_path
                    .center_y
                    .resolve(font_px, border_rect.h, root_font_px);
            Some((
                Rect::new(cx - r - scroll_x, cy - r - scroll_y, r * 2.0, r * 2.0),
                [r; 4],
            ))
        }
        ClipPathKind::Ellipse => {
            let rx = style
                .clip_path
                .ellipse_rx
                .resolve(font_px, border_rect.w, root_font_px)
                .max(0.0);
            let ry = style
                .clip_path
                .ellipse_ry
                .resolve(font_px, border_rect.h, root_font_px)
                .max(0.0);
            let cx = border_rect.x
                + style
                    .clip_path
                    .center_x
                    .resolve(font_px, border_rect.w, root_font_px);
            let cy = border_rect.y
                + style
                    .clip_path
                    .center_y
                    .resolve(font_px, border_rect.h, root_font_px);
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
    root_font_px: f32,
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
                    border_rect.x + x.resolve(font_px, border_rect.w, root_font_px) - scroll_x,
                    border_rect.y + y.resolve(font_px, border_rect.h, root_font_px) - scroll_y,
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

fn paint_gradient_border_image_fallback(
    list: &mut DisplayList,
    rect: Rect,
    widths: [f32; 4],
    source: &str,
    opacity: f32,
) -> bool {
    let src = source.trim();
    if src.eq_ignore_ascii_case("none") || !src.to_ascii_lowercase().contains("gradient(") {
        return false;
    }

    let Some(color) = gradient_border_image_color(src) else {
        return true;
    };
    if color.a == 0 {
        return true;
    }
    let color = Color::rgba(
        color.r,
        color.g,
        color.b,
        ((color.a as f32) * opacity.clamp(0.0, 1.0)).round() as u8,
    );

    if widths[0] > 0.0 {
        list.push(PaintCmd::FillRect {
            rect: Rect::new(rect.x, rect.y, rect.w, widths[0]),
            color,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
    }
    if widths[2] > 0.0 {
        list.push(PaintCmd::FillRect {
            rect: Rect::new(rect.x, rect.y + rect.h - widths[2], rect.w, widths[2]),
            color,
            radius: [0.0; 4],
            radius_y: [0.0; 4],
        });
    }
    true
}

fn gradient_border_image_color(source: &str) -> Option<Color> {
    for token in source.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        let token = token
            .trim()
            .trim_matches(|ch| ch == '(' || ch == ')' || ch == ';');
        if let Some(color) = crate::css::parse_color(token) {
            if color.a != 0 {
                return Some(color);
            }
        }
    }
    None
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
                vals.push((image_h * percent / 100.0, image_w * percent / 100.0));
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
    let left = pick(3)
        .or_else(|| pick(1))
        .or_else(|| pick(0))
        .unwrap_or((image_h, image_w))
        .1;
    [
        top.clamp(0.0, image_h),
        right.clamp(0.0, image_w),
        bottom.clamp(0.0, image_h),
        left.clamp(0.0, image_w),
    ]
}

fn border_image_repeat_modes(value: &str) -> (u8, u8) {
    fn mode(token: &str) -> u8 {
        match token.to_ascii_lowercase().as_str() {
            "repeat" => 1,
            "space" => 2,
            "round" => 3,
            _ => 0,
        }
    }

    let tokens: Vec<&str> = value.split_whitespace().collect();
    let horizontal = tokens.first().copied().map(mode).unwrap_or(0);
    let vertical = tokens.get(1).copied().map(mode).unwrap_or(horizontal);
    (horizontal, vertical)
}

fn border_image_side_values(
    value: &str,
    border_widths: [f32; 4],
    rect: Rect,
    font_px: f32,
    width_value: bool,
    root_font_px: f32,
) -> [f32; 4] {
    let tokens: Vec<&str> = value
        .split_whitespace()
        .filter(|token| !token.eq_ignore_ascii_case("auto"))
        .collect();
    if tokens.is_empty() {
        return if width_value { border_widths } else { [0.0; 4] };
    }
    let refs = [rect.h, rect.w, rect.h, rect.w];
    let parse_side = |token: &str, side: usize| -> f32 {
        if let Ok(multiplier) = token.parse::<f32>() {
            return (border_widths[side] * multiplier).max(0.0);
        }
        crate::css::parse_length_checked(token)
            .map(|length| length.resolve(font_px, refs[side], root_font_px).max(0.0))
            .unwrap_or_else(|| {
                if width_value {
                    border_widths[side]
                } else {
                    0.0
                }
            })
    };
    let pick = |idx: usize| -> &str {
        tokens
            .get(idx)
            .copied()
            .or_else(|| {
                if idx == 2 {
                    tokens.first().copied()
                } else {
                    None
                }
            })
            .or_else(|| {
                if idx == 3 {
                    tokens.get(1).copied().or_else(|| tokens.first().copied())
                } else {
                    None
                }
            })
            .unwrap_or(tokens[0])
    };
    [
        parse_side(pick(0), 0),
        parse_side(pick(1), 1),
        parse_side(pick(2), 2),
        parse_side(pick(3), 3),
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

pub(crate) fn apply_text_transform(text: &str, tt: TextTransform) -> String {
    match tt {
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => capitalize_words(text),
        TextTransform::FullWidth => text.chars().map(to_full_width_char).collect(),
        TextTransform::FullSizeKana => text.chars().map(to_full_size_kana).collect(),
        TextTransform::MathAuto => to_math_auto(text),
        TextTransform::None => text.to_owned(),
    }
}

pub(crate) fn to_full_size_kana(ch: char) -> char {
    match ch {
        // Hiragana small characters -> full-size
        'ぁ' => 'あ',
        'ぃ' => 'い',
        'ぅ' => 'う',
        'ぇ' => 'え',
        'ぉ' => 'お',
        'っ' => 'つ',
        'ゃ' => 'や',
        'ゅ' => 'ゆ',
        'ょ' => 'よ',
        'ゎ' => 'わ',
        'ゕ' => 'か',
        'ゖ' => 'け',
        // Katakana small characters -> full-size
        'ァ' => 'ア',
        'ィ' => 'イ',
        'ゥ' => 'ウ',
        'ェ' => 'エ',
        'ォ' => 'オ',
        'ッ' => 'ツ',
        'ャ' => 'ヤ',
        'ュ' => 'ユ',
        'ョ' => 'ヨ',
        'ヮ' => 'ワ',
        'ヵ' => 'カ',
        'ヶ' => 'ケ',
        // Katakana phonetic extensions
        'ㇰ' => 'ク',
        'ㇱ' => 'シ',
        'ㇲ' => 'ス',
        'ㇳ' => 'ト',
        'ㇴ' => 'ヌ',
        'ㇵ' => 'ハ',
        'ㇶ' => 'ヒ',
        'ㇷ' => 'フ',
        'ㇸ' => 'ヘ',
        'ㇹ' => 'ホ',
        'ㇺ' => 'ム',
        'ㇻ' => 'ラ',
        'ㇼ' => 'リ',
        'ㇽ' => 'ル',
        'ㇾ' => 'レ',
        'ㇿ' => 'ロ',
        // Halfwidth Katakana small characters
        'ｧ' => 'ｱ',
        'ｨ' => 'ｲ',
        'ｩ' => 'ｳ',
        'ｪ' => 'ｴ',
        'ｫ' => 'ｵ',
        'ｬ' => 'ﾔ',
        'ｭ' => 'ﾕ',
        'ｮ' => 'ﾖ',
        'ｯ' => 'ﾂ',
        _ => ch,
    }
}

pub(crate) fn to_math_auto(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut chars = text.chars().peekable();
    let mut alphabetic_run = Vec::new();

    while let Some(ch) = chars.next() {
        if is_math_letter(ch) {
            alphabetic_run.push(ch);
            if chars
                .peek()
                .map_or(true, |next_ch| !is_math_letter(*next_ch))
            {
                if alphabetic_run.len() == 1 {
                    out.push(to_math_italic(alphabetic_run[0]));
                } else {
                    for r in &alphabetic_run {
                        out.push(*r);
                    }
                }
                alphabetic_run.clear();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_math_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
        || ('\u{0391}'..='\u{03A9}').contains(&ch)
        || ('\u{03B1}'..='\u{03C9}').contains(&ch)
        || ch == '∂'
}

fn to_math_italic(ch: char) -> char {
    match ch {
        'a' => '\u{1D44E}',
        'b' => '\u{1D44F}',
        'c' => '\u{1D450}',
        'd' => '\u{1D451}',
        'e' => '\u{1D452}',
        'f' => '\u{1D453}',
        'g' => '\u{1D454}',
        'h' => '\u{210E}', // Planck constant h
        'i' => '\u{1D456}',
        'j' => '\u{1D457}',
        'k' => '\u{1D458}',
        'l' => '\u{1D459}',
        'm' => '\u{1D45A}',
        'n' => '\u{1D45B}',
        'o' => '\u{1D45C}',
        'p' => '\u{1D45D}',
        'q' => '\u{1D45E}',
        'r' => '\u{1D45F}',
        's' => '\u{1D460}',
        't' => '\u{1D461}',
        'u' => '\u{1D462}',
        'v' => '\u{1D463}',
        'w' => '\u{1D464}',
        'x' => '\u{1D465}',
        'y' => '\u{1D466}',
        'z' => '\u{1D467}',
        'A'..='Z' => char::from_u32(ch as u32 - 'A' as u32 + 0x1D434).unwrap_or(ch),
        'α'..='ω' => char::from_u32(ch as u32 - 0x03B1 + 0x1D6C2).unwrap_or(ch),
        '∂' => '\u{1D6DB}',
        'Α'..='Ω' => char::from_u32(ch as u32 - 0x0391 + 0x1D6A8).unwrap_or(ch),
        _ => ch,
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
        if open { 0xfe46 } else { 0xfe45 }
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("double-circle"))
    {
        0x25ce
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("triangle"))
    {
        if open { 0x25b3 } else { 0x25b2 }
    } else if value
        .split_whitespace()
        .any(|tok| tok.eq_ignore_ascii_case("circle"))
    {
        if open { 0x25cb } else { 0x25cf }
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

fn should_insert_missing_inline_gap(text: &str) -> bool {
    let Some(ch) = text.chars().find(|ch| !ch.is_whitespace()) else {
        return false;
    };
    !matches!(
        ch,
        ',' | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
            | '›'
            | '»'
            | '、'
            | '。'
            | '，'
            | '؟'
            | '؛'
    )
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

fn build_positioned_box(node: &WebCore, list: &mut DisplayList, ctx: &BuildContext<'_>) {
    if node.style.position != Position::Fixed {
        build_for_box(node, list, ctx);
        return;
    }
    let viewport_w = ctx.transform_ctx.viewport_w;
    let viewport_h = ctx.transform_ctx.viewport_h;
    let fixed_ctx = BuildContext {
        scroll_x: 0.0,
        scroll_y: 0.0,
        sticky_scroll_x: 0.0,
        sticky_scroll_y: 0.0,
        sticky_scroll_container: None,
        sticky_containing_block: None,
        clip: Rect::new(0.0, 0.0, viewport_w, viewport_h),
        paint_clip: Rect::new(0.0, 0.0, viewport_w, viewport_h),
        suppress_deferred_z_descendants: false,
        ..*ctx
    };
    list.push(PaintCmd::BeginFixedPosition);
    build_for_box(node, list, &fixed_ctx);
    list.push(PaintCmd::EndFixedPosition);
}

fn creates_stacking_context(node: &WebCore) -> bool {
    let eff_style = &node.style;
    (eff_style.is_positioned() && !eff_style.z_index_is_auto)
        || eff_style.opacity < 1.0
        || !eff_style.css_transform.ops.is_empty()
        || !eff_style.css_filter.ops.is_empty()
        || !eff_style.rare().backdrop_filter.is_empty()
        || eff_style.will_change_transform
        || eff_style.isolation
        || blend_mode_to_u8(eff_style.mix_blend_mode) != 0
        || matches!(eff_style.position, Position::Fixed | Position::Sticky)
}

fn is_explicit_z_positioned(node: &WebCore) -> bool {
    node.style.position == Position::Fixed
        || node.style.position == Position::Absolute
        || (node.style.is_positioned() && !node.style.z_index_is_auto)
}

fn sticky_containing_block_for_children(node: &WebCore, inherited: Option<Rect>) -> Option<Rect> {
    if matches!(node.style.display, Display::Inline | Display::Contents | Display::None) {
        return inherited;
    }
    let mut rect = if node.layout.content_rect.w > 0.0 || node.layout.content_rect.h > 0.0 {
        node.layout.content_rect
    } else if node.layout.padding_rect.w > 0.0 || node.layout.padding_rect.h > 0.0 {
        node.layout.padding_rect
    } else {
        node.layout.border_rect
    };
    if is_scroll_container(&node.style) {
        rect.w = rect.w.max(node.layout.scroll_width);
        rect.h = rect.h.max(node.layout.scroll_height);
    }
    Some(rect)
}

fn collect_explicit_z_descendants<'a>(
    node: &'a WebCore,
    containing_block: Option<Rect>,
    out: &mut Vec<(&'a WebCore, Option<Rect>)>,
) {
    if matches!(node.style.display, Display::None) {
        return;
    }
    if is_explicit_z_positioned(node) {
        out.push((node, containing_block));
        return;
    }
    if creates_stacking_context(node) {
        return;
    }
    if node.tag == "::before" || node.tag == "::after" {
        return;
    }
    let child_containing_block = sticky_containing_block_for_children(node, containing_block);
    for child in node.effective_children() {
        collect_explicit_z_descendants(child, child_containing_block, out);
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
pub(crate) fn object_fit_rect(
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
