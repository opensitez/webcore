//! `TinySkiaCanvas` — `Canvas` impl that paints onto a `tiny_skia::Pixmap`.
//!
//! This is the live render backend. The form's render loop wraps the
//! current frame's pixmap into a `TinySkiaCanvas` and either lets user
//! paint code call methods directly OR replays a `RecordingCanvas` onto
//! it (the standard path — recording is the persistent state).
//!
//! ## State management
//!
//! Canvas paint state (fill colour, stroke colour, line width, transform,
//! etc.) is non-trivial because tiny-skia doesn't carry it across calls
//! the way HTML5 canvas does. We track it explicitly in a
//! [`PaintState`] struct, mutated by the trait's `set_*` methods and
//! consulted by every `fill` / `stroke` call.
//!
//! `save` / `restore` push/pop the state stack. The trait's transform
//! ops compose into a `tiny_skia::Transform` carried in the current
//! state.
//!
//! Path building uses a `tiny_skia::PathBuilder` accumulated between
//! `begin_path` and `fill` / `stroke` calls. After painting, the
//! builder is reset for the next sub-path.
//!
//! ## Text rendering
//!
//! Text is rendered through `cosmic_text` (the same engine the rest of
//! the toolkit uses). The font cache is shared across calls via a
//! `&mut FontSystem` borrowed from the caller's
//! [`super::TextContext`]. For now, `fill_text` / `stroke_text` route
//! through `super::super::ide_text::draw_text` which is the existing
//! cosmic-text wrapper. (Stroke-text is approximated as fill-text
//! pending a real outline-stroke implementation.)
//!
//! ## Image rendering
//!
//! `draw_image` builds a `tiny_skia::Pixmap` from the image's RGBA
//! buffer and blits it onto the target with the appropriate scale
//! transform.

use tiny_skia::{
    Color as TsColor, FillRule, FilterQuality, GradientStop, LineCap as TsLineCap,
    LineJoin as TsLineJoin, LinearGradient, Mask, Paint, Path, PathBuilder, PathSegment, Pixmap,
    PixmapPaint, PixmapRef, Point as TsPoint, RadialGradient, SpreadMode, Stroke as TsStroke,
    Transform,
};

use cosmic_text::{
    Attrs, Buffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping, SwashCache,
};

use super::{
    Canvas, Color, Font, FontStyle, FontWeight, GradientKind, Image, LineCap, LineJoin,
    Paint as CanvasPaint, Repetition,
};

/// `Canvas` impl that paints into a `tiny_skia::Pixmap`.
///
/// Holds a mutable borrow of the target pixmap for its lifetime, plus
/// the current paint state. Constructed each frame by the form's render
/// loop and dropped at end-of-frame.
///
/// Optionally borrows a `FontSystem + SwashCache` from the caller to
/// support text rendering. When no font system is provided (e.g. when
/// constructed inline by tests that don't need text), `fill_text` /
/// `stroke_text` are no-ops. The form's render loop always provides one
/// — `RenderContext` already carries them — so live rendering of
/// recordings with text just works.
pub struct TinySkiaCanvas<'a> {
    pixmap: &'a mut Pixmap,
    state: PaintState,
    state_stack: Vec<PaintFrame>,
    path: PathBuilder,
    text_ctx: Option<TextCtx<'a>>,
    /// Active clip mask (if `clip` was called). Combined with the
    /// current transform when issuing draw ops. None = no clipping.
    clip_mask: Option<Mask>,
}

/// One entry on the `save`/`restore` stack — paint state + a snapshot
/// of the active clip mask. Cloning the `Mask` is the only way to
/// preserve clipping across `save`/`restore`; tiny-skia masks are just
/// alpha buffers, so the clone is a `Vec<u8>` copy of pixmap-sized
/// bytes — fine for typical UI usage.
#[derive(Clone)]
struct PaintFrame {
    state: PaintState,
    clip_mask: Option<Mask>,
}

/// Everything a `<canvas>` context keeps between two calls from a page.
///
/// The bitmap is deliberately NOT in here. A canvas element owns its pixels
/// for as long as it exists and they are reached by other code — the display
/// list paints them, `getImageData` reads them — so they stay on the element
/// and this carries only what the drawing state machine needs:
/// [HTML §4.12.5.1.2](https://html.spec.whatwg.org/multipage/canvas.html#drawing-state)'s
/// drawing state, the `save`/`restore` stack it pushes onto, the current
/// default path, and the clipping region.
///
/// `Default` is a context that has never been drawn to, which is also exactly
/// what `reset()` and a `width`/`height` assignment must produce.
#[derive(Default)]
pub struct CanvasState {
    state: PaintState,
    state_stack: Vec<PaintFrame>,
    path: PathBuilder,
    clip_mask: Option<Mask>,
}

impl CanvasState {
    /// Set how many device pixels one canvas coordinate unit covers.
    ///
    /// Called by whoever owns the bitmap, when it allocates one bigger than the
    /// coordinate space so a HiDPI display gets real pixels instead of an
    /// upscale. Everything above stays in CSS pixels: drawing, hit testing and
    /// `getTransform` all behave as if the surface were 1x.
    ///
    /// Sets the CURRENT transform too, because a scale that only took effect
    /// after the next `resetTransform()` would apply to some drawings and not
    /// others. Callers change this when they reallocate the bitmap, and a
    /// reallocation resets the drawing state anyway.
    ///
    /// A non-positive or non-finite scale is ignored: it would make the matrix
    /// singular, and every hit test inverts it.
    pub fn set_device_scale(&mut self, scale: f32) {
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        self.state.base = Transform::from_scale(scale, scale);
        self.state.transform = self.state.base;
    }

    /// The device scale in effect — 1.0 on an ordinary surface.
    pub fn device_scale(&self) -> f32 {
        self.state.base.sx
    }
}

/// Borrowed cosmic-text resources used for text rendering. Shared with
/// the rest of the toolkit via `RenderContext::font_system /
/// swash_cache`. Optional so callers that don't need text don't have
/// to set up cosmic-text.
struct TextCtx<'a> {
    font_system: &'a mut FontSystem,
    swash_cache: &'a mut SwashCache,
}

#[derive(Clone, Debug)]
pub struct PaintState {
    pub fill: CanvasPaint,
    pub stroke: CanvasPaint,
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub global_alpha: f32,
    pub font: Font,
    pub transform: Transform,
    /// **Device pixels per canvas coordinate unit, applied under everything.**
    ///
    /// A backing store bigger than the coordinate space is how a canvas stays
    /// sharp on a HiDPI display: the bitmap is allocated at `rect * dpr` and
    /// this scales drawing up to fill it, so a page keeps writing CSS pixels
    /// and gets device pixels. It is exactly the `ctx.scale(dpr, dpr)` idiom a
    /// page would otherwise have to write itself, moved under the API.
    ///
    /// It sits UNDER the page's transform, which is what makes it invisible:
    /// `resetTransform()` comes back to this rather than to the identity, and
    /// `getTransform()` reports the page's own matrix with this divided out. A
    /// device scale a page could see would be a device scale a page could
    /// destroy — one `resetTransform()` and every later drawing would land at
    /// half size on a 2x display.
    pub base: Transform,
    pub dash_intervals: Vec<f32>,
    pub dash_offset: f32,
    pub image_smoothing: bool,
    pub text_align: super::TextAlign,
    pub text_baseline: super::TextBaseline,
    pub shadow: super::Shadow,
    pub composite: super::CompositeOp,
    pub smoothing_quality: super::SmoothingQuality,
    /// `filter`, as the CSS source string. `"none"` is the initial value and
    /// the spec's own spelling for "no filter", so it is stored rather than
    /// represented as an absence.
    pub filter: String,
    pub direction: super::Direction,
    pub lang: String,
    pub letter_spacing: String,
    pub word_spacing: String,
    pub font_kerning: super::FontKerning,
    pub font_stretch: super::FontStretch,
    pub font_variant_caps: super::FontVariantCaps,
    pub text_rendering: super::TextRendering,
}

impl Default for PaintState {
    fn default() -> Self {
        Self {
            fill: CanvasPaint::Color(Color::BLACK),
            stroke: CanvasPaint::Color(Color::BLACK),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            global_alpha: 1.0,
            font: Font::default(),
            transform: Transform::identity(),
            base: Transform::identity(),
            dash_intervals: Vec::new(),
            dash_offset: 0.0,
            // HTML5 canvas defaults `imageSmoothingEnabled` to true.
            image_smoothing: true,
            // …and `textAlign` to `start`, `textBaseline` to `alphabetic`.
            text_align: super::TextAlign::default(),
            text_baseline: super::TextBaseline::default(),
            // `shadowColor` starts transparent black, which is what makes the
            // other three shadow attributes inert until one is set.
            shadow: super::Shadow::default(),
            composite: super::CompositeOp::SourceOver,
            smoothing_quality: super::SmoothingQuality::Low,
            filter: "none".to_string(),
            direction: super::Direction::Inherit,
            lang: "inherit".to_string(),
            letter_spacing: "0px".to_string(),
            word_spacing: "0px".to_string(),
            font_kerning: super::FontKerning::Auto,
            font_stretch: super::FontStretch::Normal,
            font_variant_caps: super::FontVariantCaps::Normal,
            text_rendering: super::TextRendering::Auto,
        }
    }
}

impl<'a> TinySkiaCanvas<'a> {
    /// Wrap a pixmap as a canvas. The canvas borrows the pixmap until
    /// it's dropped. Text rendering is disabled (no font system).
    pub fn new(pixmap: &'a mut Pixmap) -> Self {
        Self {
            pixmap,
            state: PaintState::default(),
            state_stack: Vec::new(),
            path: PathBuilder::new(),
            text_ctx: None,
            clip_mask: None,
        }
    }

    /// Wrap a pixmap as a canvas with text rendering enabled. The
    /// `FontSystem` and `SwashCache` are borrowed for the lifetime of
    /// the canvas. The form's render loop constructs canvases this way
    /// using `RenderContext::font_system / swash_cache`.
    pub fn with_text(
        pixmap: &'a mut Pixmap,
        font_system: &'a mut FontSystem,
        swash_cache: &'a mut SwashCache,
    ) -> Self {
        Self {
            pixmap,
            state: PaintState::default(),
            state_stack: Vec::new(),
            path: PathBuilder::new(),
            text_ctx: Some(TextCtx {
                font_system,
                swash_cache,
            }),
            clip_mask: None,
        }
    }

    /// Resume a `<canvas>` context that was suspended between operations.
    ///
    /// **Why this exists.** A page reaches the canvas one call at a time —
    /// `ctx.fillStyle = "red"` and `ctx.fillRect(…)` are two separate trips
    /// across the host boundary, and the seam hands over one op per trip. A
    /// canvas built fresh for each of them would start from
    /// `PaintState::default()` every time, so the fill colour set by the first
    /// call would be gone by the second and every drawing would come out
    /// black. The bitmap is not the only thing that persists between calls;
    /// the *context state* does too, and HTML §4.12.5.1.2 is explicit that it
    /// survives until reset.
    ///
    /// So a caller holding a `<canvas>` element keeps a [`CanvasState`]
    /// alongside the pixels and threads it back in here. What is carried is
    /// four fields and no pixels: `resume`/[`suspend`](Self::suspend) MOVE
    /// them, so a round trip is a handful of pointer swaps and not a copy of
    /// the surface.
    pub fn resume(
        pixmap: &'a mut Pixmap,
        saved: CanvasState,
        text: Option<(&'a mut FontSystem, &'a mut SwashCache)>,
    ) -> Self {
        Self {
            pixmap,
            state: saved.state,
            state_stack: saved.state_stack,
            path: saved.path,
            text_ctx: text.map(|(font_system, swash_cache)| TextCtx {
                font_system,
                swash_cache,
            }),
            clip_mask: saved.clip_mask,
        }
    }

    /// Detach everything that has to outlive this operation, releasing the
    /// borrow on the pixmap and the fonts. The counterpart to
    /// [`resume`](Self::resume).
    pub fn suspend(self) -> CanvasState {
        CanvasState {
            state: self.state,
            state_stack: self.state_stack,
            path: self.path,
            clip_mask: self.clip_mask,
        }
    }

    // An inherent `measure_text` returning `(width, height)` used to live here.
    //
    // It is gone because the `Canvas` trait now has one returning the spec's
    // full `TextMetrics`, and two methods of the same name is worse than
    // either: an inherent method WINS over a trait method at every call site,
    // so the narrower answer would have shadowed the complete one silently.
    // `(width, height)` is recoverable from the metrics — `width`, and the two
    // `font_bounding_box` fields — so nothing was lost with it.

    /// Build a `tiny_skia::Paint` from the current fill style and global alpha.
    ///
    /// A free function over the STATE rather than a method on `self`, and the
    /// lifetime is why: a pattern's shader borrows the image bytes, so the
    /// paint cannot be `'static`. Borrowing only `state` lets the call sites
    /// hold the paint and `&mut self.pixmap` at once — disjoint fields, which
    /// a method taking `&self` would have merged into one borrow of the whole
    /// canvas.
    fn fill_paint(state: &PaintState) -> Paint<'_> {
        let mut p = Paint::default();
        apply_canvas_paint(&mut p, &state.fill, state.global_alpha);
        p.anti_alias = true;
        // `globalCompositeOperation` belongs on every fill and stroke, so it is
        // set in the two paint builders rather than at the call sites — there
        // are a dozen of those and one that forgot would silently paint
        // source-over while the attribute said otherwise.
        p.blend_mode = state.composite.to_tiny_skia();
        p
    }

    /// Build a `tiny_skia::Paint` + `Stroke` from the current stroke
    /// style, line width, caps, joins, miter limit, dash, and global alpha.
    fn stroke_paint(state: &PaintState) -> (Paint<'_>, TsStroke) {
        let mut p = Paint::default();
        apply_canvas_paint(&mut p, &state.stroke, state.global_alpha);
        p.anti_alias = true;
        p.blend_mode = state.composite.to_tiny_skia();
        let mut stroke = TsStroke {
            width: state.line_width,
            line_cap: line_cap_to_ts(state.line_cap),
            line_join: line_join_to_ts(state.line_join),
            miter_limit: state.miter_limit,
            ..TsStroke::default()
        };
        if !state.dash_intervals.is_empty() {
            stroke.dash =
                tiny_skia::StrokeDash::new(state.dash_intervals.clone(), state.dash_offset);
        }
        (p, stroke)
    }

    /// Take the current path builder and replace it with an empty one.
    /// Used by `fill` / `stroke` after they've consumed the path.
    /// The current path, as a finished `tiny_skia::Path`.
    ///
    /// **Borrows it; does not consume it.** §4.12.5 resets the current path in
    /// exactly one place — `beginPath()` — so `fill()` must leave it standing.
    /// This used to take the path out, which meant `rect(); fill(); stroke();`
    /// filled and then stroked NOTHING: the second call found an empty path and
    /// drew nothing at all, silently. `rect(); fill(); stroke();` is how a page
    /// draws an outlined box, so that was most of them.
    fn current_path(&self) -> Option<Path> {
        self.path.clone().finish()
    }

    /// Fill or stroke an explicit path, leaving the current path alone.
    ///
    /// The body `fill_with_rule` and `stroke` share, lifted so the `Path2D`
    /// overloads can reach it with a path of their own.
    fn paint_path(&mut self, path: &Path, rule: super::FillRule, as_stroke: bool) {
        let path = path.clone();
        self.with_effects(move |target, state, clip| {
            if as_stroke {
                let (paint, stroke) = Self::stroke_paint(state);
                target.stroke_path(&path, &paint, &stroke, state.transform, clip);
            } else {
                let paint = Self::fill_paint(state);
                target.fill_path(&path, &paint, rule.to_tiny_skia(), state.transform, clip);
            }
        });
    }

    /// Intersect the clip with an explicit path, leaving the current path
    /// alone. The body `clip_with_rule` shares with `clip_path`.
    fn clip_to(&mut self, path: &Path, rule: super::FillRule) {
        let ts_rule = rule.to_tiny_skia();
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        let Some(mut mask) = Mask::new(w, h) else {
            return;
        };
        mask.fill_path(path, ts_rule, true, self.state.transform);
        if let Some(existing) = self.clip_mask.as_mut() {
            existing.intersect_path(path, ts_rule, true, self.state.transform);
        } else {
            self.clip_mask = Some(mask);
        }
    }
}

impl<'a> Canvas for TinySkiaCanvas<'a> {
    /// The rasteriser's own paint state — the attribute getters read through
    /// this, so what a page reads back is the state the next drawing will use.
    fn drawing_state(&self) -> &super::DrawingState {
        &self.state
    }

    /// `fillStyle = "..."` — parsed by the ENGINE's own CSS colour parser, so
    /// every form the engine understands is understood here too.
    fn set_fill_style_css(&mut self, css: &str) {
        if let Some(c) = parse_canvas_color(css) {
            self.state.fill = super::Paint::Color(c);
        }
    }

    fn set_stroke_style_css(&mut self, css: &str) {
        if let Some(c) = parse_canvas_color(css) {
            self.state.stroke = super::Paint::Color(c);
        }
    }

    fn set_font_css(&mut self, css: &str) {
        if let Some(f) = parse_canvas_font(css) {
            self.state.font = f;
        }
    }

    fn set_shadow_color_css(&mut self, css: &str) {
        if let Some(c) = parse_canvas_color(css) {
            self.state.shadow.color = c;
        }
    }

    // ─── Paint state ────────────────────────────────────────────────────

    fn set_fill_color(&mut self, color: Color) {
        self.state.fill = CanvasPaint::Color(color);
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.state.stroke = CanvasPaint::Color(color);
    }
    fn set_fill_paint(&mut self, paint: &CanvasPaint) {
        self.state.fill = paint.clone();
    }
    fn set_stroke_paint(&mut self, paint: &CanvasPaint) {
        self.state.stroke = paint.clone();
    }
    fn set_line_width(&mut self, width: f32) {
        self.state.line_width = width.max(0.0);
    }
    fn set_line_cap(&mut self, cap: LineCap) {
        self.state.line_cap = cap;
    }
    fn set_line_join(&mut self, join: LineJoin) {
        self.state.line_join = join;
    }
    fn set_miter_limit(&mut self, limit: f32) {
        self.state.miter_limit = limit.max(1.0);
    }
    fn set_global_alpha(&mut self, alpha: f32) {
        self.state.global_alpha = alpha.clamp(0.0, 1.0);
    }

    fn set_image_smoothing(&mut self, enabled: bool) {
        self.state.image_smoothing = enabled;
    }
    fn set_text_align(&mut self, align: super::TextAlign) {
        self.state.text_align = align;
    }
    fn set_text_baseline(&mut self, baseline: super::TextBaseline) {
        self.state.text_baseline = baseline;
    }
    fn set_font(&mut self, font: &Font) {
        self.state.font = font.clone();
    }
    fn set_line_dash(&mut self, intervals: &[f32]) {
        // A rejected list leaves the previous pattern in force rather than
        // clearing it — see `normalize_dash` for which lists are rejected and
        // why an odd-length one comes back doubled.
        if let Some(dash) = super::normalize_dash(intervals) {
            self.state.dash_intervals = dash;
        }
    }
    fn set_line_dash_offset(&mut self, offset: f32) {
        self.state.dash_offset = offset;
    }

    // ─── Path building ──────────────────────────────────────────────────

    fn begin_path(&mut self) {
        self.path = PathBuilder::new();
    }

    fn close_path(&mut self) {
        self.path.close();
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x, y);
    }

    fn quadratic_curve_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.quad_to(cx, cy, x, y);
    }

    fn bezier_curve_to(&mut self, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32) {
        self.path.cubic_to(cx1, cy1, cx2, cy2, x, y);
    }

    fn arc(&mut self, x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool) {
        // Polyline approximation. tiny-skia doesn't have a native arc
        // primitive — we sample N segments along the angular range. 32
        // segments is enough for visual smoothness up to ~100px radius.
        let segments = 32usize;
        let mut a = start;
        let total = if ccw {
            -(start - end).abs()
        } else {
            (end - start).abs()
        };
        let step = total / segments as f32;

        // First point — move_to so we don't connect to whatever was last.
        let (sx, sy) = (x + r * a.cos(), y + r * a.sin());
        if self.path.is_empty() {
            self.path.move_to(sx, sy);
        } else {
            self.path.line_to(sx, sy);
        }
        for _ in 0..segments {
            a += step;
            let (px, py) = (x + r * a.cos(), y + r * a.sin());
            self.path.line_to(px, py);
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.path.move_to(x, y);
        self.path.line_to(x + w, y);
        self.path.line_to(x + w, y + h);
        self.path.line_to(x, y + h);
        self.path.close();
    }

    fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        // Cubic-bezier approximation of an ellipse — same magic
        // constant trick as super::super::circle_path.
        let kx = rx * 0.5522848;
        let ky = ry * 0.5522848;
        self.path.move_to(x, y - ry);
        self.path
            .cubic_to(x + kx, y - ry, x + rx, y - ky, x + rx, y);
        self.path
            .cubic_to(x + rx, y + ky, x + kx, y + ry, x, y + ry);
        self.path
            .cubic_to(x - kx, y + ry, x - rx, y + ky, x - rx, y);
        self.path
            .cubic_to(x - rx, y - ky, x - kx, y - ry, x, y - ry);
        self.path.close();
    }

    // ─── Drawing ────────────────────────────────────────────────────────

    fn fill(&mut self) {
        self.fill_with_rule(super::FillRule::NonZero);
    }

    fn fill_with_rule(&mut self, rule: super::FillRule) {
        if let Some(path) = self.current_path() {
            self.paint_path(&path, rule, false);
        }
    }

    fn stroke(&mut self) {
        if let Some(path) = self.current_path() {
            self.paint_path(&path, super::FillRule::NonZero, true);
        }
    }

    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(path) = pb.finish() {
            self.with_effects(|target, state, clip| {
                let paint = Self::fill_paint(state);
                target.fill_path(&path, &paint, FillRule::Winding, state.transform, clip);
            });
        }
    }

    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(path) = pb.finish() {
            self.with_effects(|target, state, clip| {
                let (paint, stroke) = Self::stroke_paint(state);
                target.stroke_path(&path, &paint, &stroke, state.transform, clip);
            });
        }
    }

    fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        // Fill with transparent black, replacing whatever's there.
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + w, y);
        pb.line_to(x + w, y + h);
        pb.line_to(x, y + h);
        pb.close();
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.set_color_rgba8(0, 0, 0, 0);
            paint.blend_mode = tiny_skia::BlendMode::Source;
            let mask = self.clip_mask.as_ref();
            self.pixmap
                .fill_path(&path, &paint, FillRule::Winding, self.state.transform, mask);
        }
    }

    fn fill_text(&mut self, text: &str, x: f32, y: f32) {
        // Real text rendering through cosmic-text. Requires a font
        // context — without one, this is a no-op (used by tests that
        // construct a TinySkiaCanvas without text support).
        // **Taken out of `self`, not borrowed from it.** Text has to go through
        // `with_effects` like every other drawing operation — the spec shadows
        // and filters text exactly as it does a shape — and that takes `&mut
        // self`. Holding `self.text_ctx` borrowed across the call would make
        // the two borrows overlap, so the font resources are moved to a local
        // for the duration and put back at the end.
        let Some(mut owned_tc) = self.text_ctx.take() else {
            return;
        };
        let tc = &mut owned_tc;

        // Build a buffer for the text, shape it, and hand the shaped glyphs to
        // the renderer's compositing loop — the same one `draw_text_cmd` uses
        // for element text, so canvas text and page text are blitted by one
        // piece of code rather than two that can disagree about alpha.
        let scale = self.state.transform.sx;
        let size = self.state.font.size;
        let metrics = Metrics::new(size, size * 1.3).scale(scale);
        let mut buf = shape_text(tc, &self.state, text, scale);

        // Glyphs are rasterised through cosmic-text, which takes ONE colour —
        // it has no shader. A gradient-filled string therefore paints in the
        // gradient's first stop. Visible and wrong in an obvious way, rather
        // than invisible: painting it transparent would delete the text.
        let fill = apply_alpha(self.state.fill.as_flat_color(), self.state.global_alpha);
        let cosmic_color = CosmicColor::rgba(fill.r, fill.g, fill.b, fill.a);

        let (px, py) = self.text_origin(&buf, x, y, metrics.line_height);

        // Through the drawing model like everything else, so `shadowBlur` and
        // `filter` reach text and not only shapes. The closure borrows the
        // local font resources rather than `self`, which is what the `take`
        // above was for.
        self.with_effects(|target, _state, clip| {
            crate::renderer::display_list_replay::blit_shaped_buffer(
                target,
                owned_tc.font_system,
                owned_tc.swash_cache,
                &mut buf,
                text,
                px,
                py,
                0.0,
                0.0,
                cosmic_color,
                clip,
            );
        });
        self.text_ctx = Some(owned_tc);
    }

    /// `strokeText` — the glyph OUTLINES, stroked.
    ///
    /// This used to fill the text in the stroke colour and say so in a comment.
    /// That is a different picture: `strokeText` draws the edge of each letter
    /// and leaves the inside empty, which is the whole reason a page reaches
    /// for it — outlined display type, text over a photograph, a fill and a
    /// stroke in two colours on the same string. Filled text in the stroke
    /// colour is `fillText` with the wrong `fillStyle`.
    ///
    /// The outlines come from swash, the same rasteriser cosmic-text already
    /// uses for the filled glyphs, so the two agree about what the letters are.
    /// Once the path is built it is an ordinary stroke — `lineWidth`,
    /// `lineJoin`, `lineDash`, shadows and filters all apply, because it goes
    /// through the same drawing model as every other stroke.
    fn stroke_text(&mut self, text: &str, x: f32, y: f32) {
        let Some(mut owned_tc) = self.text_ctx.take() else {
            return;
        };
        let scale = self.state.transform.sx;
        let size = self.state.font.size;
        let metrics = Metrics::new(size, size * 1.3).scale(scale);
        let buf = shape_text(&mut owned_tc, &self.state, text, scale);
        let (px, py) = self.text_origin(&buf, x, y, metrics.line_height);

        let path = glyph_outlines(&mut owned_tc, &buf, px, py);
        self.text_ctx = Some(owned_tc);

        let Some(path) = path else {
            return;
        };
        self.with_effects(|target, state, clip| {
            let (paint, stroke) = Self::stroke_paint(state);
            // **Identity, not `state.transform`.** The glyphs were shaped at
            // the transformed size and positioned in device pixels already —
            // `text_origin` maps the anchor through the matrix — so applying it
            // again here would scale the text twice.
            target.stroke_path(&path, &paint, &stroke, Transform::identity(), clip);
        });
    }

    fn clip(&mut self) {
        self.clip_with_rule(super::FillRule::NonZero);
    }

    fn clip_with_rule(&mut self, rule: super::FillRule) {
        // `clip()` does NOT reset the current path either — §4.12.5 gives that
        // to `beginPath()` alone.
        if let Some(path) = self.current_path() {
            self.clip_to(&path, rule);
        }
    }

    fn reset_clip(&mut self) {
        self.clip_mask = None;
    }

    fn draw_image(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32) {
        // Build a tiny_skia::PixmapRef from the image's RGBA buffer,
        // then draw_pixmap with a scale transform that maps the
        // image's natural dimensions to the requested rect.
        if let Some(src) = PixmapRef::from_bytes(&img.pixels, img.width, img.height) {
            let scale_x = w / img.width as f32;
            let scale_y = h / img.height as f32;
            self.with_effects(move |target, state, clip| {
                let xform = state
                    .transform
                    .pre_translate(x, y)
                    .pre_scale(scale_x, scale_y);
                let pp = PixmapPaint {
                    opacity: state.global_alpha,
                    blend_mode: state.composite.to_tiny_skia(),
                    // Smoothing OFF means nearest-neighbour, which is what a
                    // software renderer upscaled to the window needs: bilinear
                    // turns Doom's 320x200 frame into a blur. When it is ON,
                    // `imageSmoothingQuality` picks how good — the spec makes
                    // the two attributes a pair, and reading only the boolean
                    // is what left the quality setting inert.
                    quality: if state.image_smoothing {
                        state.smoothing_quality.to_filter_quality()
                    } else {
                        FilterQuality::Nearest
                    },
                };
                // **`clip`, not `None`.** This passed `None` before, so a
                // `clip()` region held for every shape and every string and was
                // ignored by exactly one operation — an image drawn into a
                // clipped canvas spilled outside it. Nothing reported it
                // because the image still appeared; it appeared in too many
                // places.
                target.draw_pixmap(0, 0, src, &pp, xform, clip);
            });
        }
    }

    /// A raw pixel write — no transform, no clip, no alpha, no blending.
    ///
    /// Every other method here composes with `self.state`. This one must not,
    /// and it is the easiest arm in the file to get wrong by copying its
    /// neighbour: routing it through the paint state would make a software
    /// renderer's frame land somewhere else, tinted, or clipped away.
    /// `BlendMode::Source` REPLACES rather than composites, which is what
    /// "these are the pixels" means when the source has alpha.
    fn put_image_data(&mut self, img: &Image, dx: f32, dy: f32) {
        let Some(src) = PixmapRef::from_bytes(&img.pixels, img.width, img.height) else {
            return;
        };
        let pp = PixmapPaint {
            opacity: 1.0,
            blend_mode: tiny_skia::BlendMode::Source,
            quality: FilterQuality::Nearest,
        };
        // `draw_pixmap` takes an INTEGER destination, which suits a raw write:
        // the spec's `dx`/`dy` are longs, so there is no sub-pixel case to
        // lose here.
        self.pixmap.draw_pixmap(
            dx as i32,
            dy as i32,
            src,
            &pp,
            tiny_skia::Transform::identity(),
            None,
        );
    }

    // ─── State stack ────────────────────────────────────────────────────

    fn save(&mut self) {
        self.state_stack.push(PaintFrame {
            state: self.state.clone(),
            clip_mask: self.clip_mask.clone(),
        });
    }

    fn restore(&mut self) {
        if let Some(prev) = self.state_stack.pop() {
            self.state = prev.state;
            self.clip_mask = prev.clip_mask;
        }
    }

    // ─── Transforms ─────────────────────────────────────────────────────

    fn translate(&mut self, x: f32, y: f32) {
        self.state.transform = self.state.transform.pre_translate(x, y);
    }

    fn rotate(&mut self, rad: f32) {
        self.state.transform = self.state.transform.pre_rotate(rad.to_degrees());
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        self.state.transform = self.state.transform.pre_scale(sx, sy);
    }

    fn transform(&mut self, m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) {
        let t = Transform::from_row(m11, m12, m21, m22, dx, dy);
        self.state.transform = self.state.transform.pre_concat(t);
    }

    /// Back to the base transform, NOT to the identity.
    ///
    /// On a 1x surface those are the same matrix. On a HiDPI one they are not,
    /// and resetting past the device scale would leave every later drawing at
    /// half size — while `getTransform()` still read back as the identity,
    /// because the page's own matrix genuinely would be.
    fn reset_transform(&mut self) {
        self.state.transform = self.state.base;
    }

    /// tiny-skia's path builder already knows where the path ends, so this is
    /// a read rather than state of our own to keep in step.
    fn current_point(&self) -> Option<(f32, f32)> {
        self.path.last_point().map(|p| (p.x, p.y))
    }

    /// The PAGE's transform — the device scale divided back out.
    ///
    /// A page that has done nothing must read the identity here even on a 2x
    /// display, because the scale it would otherwise see is not one it applied
    /// and not one it can reason about.
    fn get_transform(&self) -> super::Matrix {
        super::Matrix::from_tiny_skia(self.page_transform())
    }

    fn set_transform(&mut self, m: super::Matrix) {
        self.state.transform = self.state.base.pre_concat(m.to_tiny_skia());
    }

    // ─── Context state ──────────────────────────────────────────────────

    /// All three things the spec's `reset()` does, and the bitmap is one of
    /// them: a `reset` that only restored the state would leave the previous
    /// drawing on screen.
    fn reset(&mut self) {
        // The device scale is a property of the SURFACE, not of the drawing
        // state, so it survives a reset — the bitmap is still the same number
        // of device pixels afterwards.
        let base = self.state.base;
        self.pixmap.fill(TsColor::TRANSPARENT);
        self.state = PaintState::default();
        self.state.base = base;
        self.state.transform = base;
        self.state_stack.clear();
        self.path = PathBuilder::new();
        self.clip_mask = None;
    }

    fn set_global_composite_operation(&mut self, op: super::CompositeOp) {
        self.state.composite = op;
    }

    fn set_image_smoothing_quality(&mut self, quality: super::SmoothingQuality) {
        self.state.smoothing_quality = quality;
    }

    /// Stored and reported, NOT applied.
    ///
    /// The value is a CSS `<filter-value-list>`; applying one means running a
    /// blur/colour-matrix pass over the drawing, which this rasteriser has no
    /// pipeline for. Keeping it means `ctx.filter` reads back what was set —
    /// the attribute behaves — while the pixels are honestly unfiltered.
    /// Silently reporting `"none"` would hide the gap from the one caller in a
    /// position to notice it.
    fn set_filter(&mut self, filter: &str) {
        self.state.filter = filter.to_string();
    }

    // `context_attributes` is NOT overridden: this canvas has no constructor
    // that takes settings, so the defaults ARE what it was created with, and
    // the trait's default says exactly that. An override returning the same
    // value would look like it was reporting stored settings when there are
    // none to store — the shape of `filter` above, and worth not repeating.

    fn get_line_dash(&self) -> Vec<f32> {
        self.state.dash_intervals.clone()
    }

    // ─── Text drawing styles ────────────────────────────────────────────

    fn set_direction(&mut self, direction: super::Direction) {
        self.state.direction = direction;
    }
    /// Stored and reported, NOT applied — and it cannot be, here.
    ///
    /// `lang` picks between glyphs that share a codepoint: the same character
    /// is drawn differently in Chinese, Japanese and Korean, and the language
    /// is what decides which. cosmic-text 0.18 has no language field on
    /// `Attrs` at all, so there is nothing to pass it to short of replacing the
    /// shaper. The attribute behaves — it reads back what was set — and the
    /// glyphs do not change. Said out loud because a CJK page is the one that
    /// would notice, and it would notice as "wrong-looking text" rather than as
    /// a missing feature.
    fn set_lang(&mut self, lang: &str) {
        self.state.lang = lang.to_string();
    }
    fn set_letter_spacing(&mut self, spacing: &str) {
        self.state.letter_spacing = spacing.to_string();
    }
    fn set_word_spacing(&mut self, spacing: &str) {
        self.state.word_spacing = spacing.to_string();
    }
    fn set_font_kerning(&mut self, kerning: super::FontKerning) {
        self.state.font_kerning = kerning;
    }
    fn set_font_stretch(&mut self, stretch: super::FontStretch) {
        self.state.font_stretch = stretch;
    }
    fn set_font_variant_caps(&mut self, caps: super::FontVariantCaps) {
        self.state.font_variant_caps = caps;
    }
    /// Stored and reported. It is a HINT in the spec — "the user agent should
    /// take this into account" — and this rasteriser has one text quality, so
    /// there is nothing for the hint to select between. That is a legitimate
    /// reading of the attribute rather than a gap: a browser is free to ignore
    /// it, and most do for `optimizeSpeed` versus `auto`.
    fn set_text_rendering(&mut self, rendering: super::TextRendering) {
        self.state.text_rendering = rendering;
    }

    fn set_shadow(&mut self, shadow: &super::Shadow) {
        self.state.shadow = shadow.clone();
    }

    /// Draw the focus ring around the current path.
    ///
    /// The spec leaves the ring's appearance to the user agent, so this is a
    /// choice rather than a rule: two hairlines, dark inside and light outside,
    /// which stays visible on both a light and a dark drawing. It is stroked
    /// under `source-over` and full alpha whatever the context's compositing
    /// state is — a focus ring the page can accidentally make invisible is an
    /// accessibility failure, not a style.
    fn draw_focus_if_needed(&mut self, focused: bool) {
        if !focused {
            return;
        }
        let Some(path) = self.path.clone().finish() else {
            return;
        };
        for (width, color) in [
            (3.0f32, Color::rgba(255, 255, 255, 200)),
            (1.0f32, Color::rgb(0, 0, 0)),
        ] {
            let mut paint = Paint::default();
            paint.set_color(color.to_tiny_skia());
            paint.anti_alias = true;
            let stroke = TsStroke {
                width,
                ..TsStroke::default()
            };
            self.pixmap.stroke_path(
                &path,
                &paint,
                &stroke,
                self.state.transform,
                self.clip_mask.as_ref(),
            );
        }
    }

    // ─── Measurement ────────────────────────────────────────────────────

    /// Real metrics, shaped through the same cosmic-text path that draws.
    ///
    /// Measuring through the drawing path rather than a parallel estimate is
    /// the point: an advance computed a second way drifts from the glyphs that
    /// actually land, and every caller that centres or wraps text is laying out
    /// against this number.
    ///
    /// **Still not reachable from a page.** `measureText` is the one canvas
    /// operation that ASKS rather than paints, and `platforms/web`'s seam
    /// carries `Op2D`, which is fire-and-forget — there is no wire format for
    /// an answer to come back through. That is a gap in the seam, not here.
    fn measure_text(&mut self, text: &str) -> super::TextMetrics {
        let Some(tc) = self.text_ctx.as_mut() else {
            // No font system: nothing can be measured, and zero says so. See
            // the trait's note on why an invented number would be worse.
            return super::TextMetrics::default();
        };
        let size = self.state.font.size;
        let metrics = Metrics::new(size, size * 1.3);
        // Scale 1.0: a measurement answers in the canvas's own units, and the
        // caller applies the transform to what it does with the number.
        let buf = shape_text(tc, &self.state, text, 1.0);

        let Some(run) = buf.layout_runs().next() else {
            return super::TextMetrics::default();
        };
        let width = run.line_w;
        let line_height = metrics.line_height;
        // `line_y` is the baseline's distance from the top of the line box, so
        // it IS the ascent of that box and the rest is its descent.
        let font_ascent = run.line_y;
        let font_descent = (line_height - run.line_y).max(0.0);

        // Ink extents, from the rasterised glyphs themselves. `placement.top`
        // is measured UP from the baseline, which is already the spec's sign
        // convention, and `left` is the bearing — so a glyph that overhangs its
        // advance (an italic f, a script capital) is reported as overhanging
        // rather than clipped to the advance width.
        let mut ink_left = f32::INFINITY;
        let mut ink_right = f32::NEG_INFINITY;
        let mut ink_top = f32::NEG_INFINITY;
        let mut ink_bottom = f32::NEG_INFINITY;
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let Some(image) = tc.swash_cache.get_image(tc.font_system, physical.cache_key) else {
                continue;
            };
            if image.placement.width == 0 || image.placement.height == 0 {
                continue;
            }
            let left = physical.x as f32 + image.placement.left as f32;
            let right = left + image.placement.width as f32;
            let top = image.placement.top as f32;
            let bottom = image.placement.height as f32 - top;
            ink_left = ink_left.min(left);
            ink_right = ink_right.max(right);
            ink_top = ink_top.max(top);
            ink_bottom = ink_bottom.max(bottom);
        }
        // An all-whitespace or unrenderable run has no ink at all; zero is
        // exact there, not a fallback.
        let has_ink = ink_right > ink_left;
        let (ink_left, ink_right, ink_top, ink_bottom) = if has_ink {
            (ink_left, ink_right, ink_top, ink_bottom)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        // The em box sits on the alphabetic baseline and is `size` tall. Its
        // split between ascent and descent follows the font's own, which is
        // what keeps a tall-ascender font from being reported like a short one.
        let font_extent = font_ascent + font_descent;
        let (em_ascent, em_descent) = if font_extent > 0.0 {
            (
                size * font_ascent / font_extent,
                size * font_descent / font_extent,
            )
        } else {
            (size, 0.0)
        };

        super::TextMetrics {
            width,
            // Measured from the alignment point, positive LEFTWARD — hence the
            // negation. `textAlign` is applied by the caller, so the alignment
            // point here is the text origin.
            actual_bounding_box_left: -ink_left,
            actual_bounding_box_right: ink_right,
            font_bounding_box_ascent: font_ascent,
            font_bounding_box_descent: font_descent,
            actual_bounding_box_ascent: ink_top,
            actual_bounding_box_descent: ink_bottom,
            em_height_ascent: em_ascent,
            em_height_descent: em_descent,
            // The hanging and ideographic baselines come from the font's OS/2
            // and BASE tables, which cosmic-text does not surface. These are
            // the conventional fractions of the ascent and descent that a font
            // without a BASE table gets, and they are STATED as such rather
            // than presented as read from the face.
            hanging_baseline: font_ascent * 0.8,
            // Zero exactly: every other y-direction number here is measured
            // FROM the alphabetic baseline, so its own offset is zero.
            alphabetic_baseline: 0.0,
            ideographic_baseline: -font_descent,
        }
    }

    // ─── Hit testing ────────────────────────────────────────────────────

    fn is_point_in_path(&self, x: f32, y: f32, rule: super::FillRule) -> bool {
        let Some(path) = self.path.clone().finish() else {
            return false;
        };
        self.hit_test_fill(&path, x, y, rule)
    }

    fn is_point_in_stroke(&self, x: f32, y: f32) -> bool {
        let Some(path) = self.path.clone().finish() else {
            return false;
        };
        self.hit_test_stroke(&path, x, y)
    }

    /// `fill(path)` — WITHOUT disturbing the current path.
    ///
    /// The trait's default builds the shape into the current path and fills
    /// that, which destroys whatever the context was half-way through
    /// describing. Its own doc comment said it must not — and it did, because
    /// a default written in terms of the other trait methods has no way to
    /// reach a path except through the current one.
    ///
    /// Here there is a way: the `Path2D`'s segments go straight to a
    /// `tiny_skia::Path` and are painted from there, so `self.path` is never
    /// touched. That isolation is the entire reason a page uses a `Path2D`.
    fn fill_path(&mut self, path: &super::Path2D, rule: super::FillRule) {
        let Some(built) = build_path2d(path) else {
            return;
        };
        self.paint_path(&built, rule, false);
    }

    /// `stroke(path)` — likewise leaves the current path alone.
    fn stroke_path(&mut self, path: &super::Path2D) {
        let Some(built) = build_path2d(path) else {
            return;
        };
        self.paint_path(&built, super::FillRule::NonZero, true);
    }

    /// `clip(path, fillRule)` — likewise.
    ///
    /// `clip` is the one that made the default's behaviour unrecoverable: it
    /// discards the current path as part of its own semantics, so a caller
    /// could not put back what the default had thrown away.
    fn clip_path(&mut self, path: &super::Path2D, rule: super::FillRule) {
        let Some(built) = build_path2d(path) else {
            return;
        };
        self.clip_to(&built, rule);
    }

    fn is_point_in_path2d(
        &self,
        path: &super::Path2D,
        x: f32,
        y: f32,
        rule: super::FillRule,
    ) -> bool {
        let Some(built) = build_path2d(path) else {
            return false;
        };
        self.hit_test_fill(&built, x, y, rule)
    }

    fn is_point_in_stroke2d(&self, path: &super::Path2D, x: f32, y: f32) -> bool {
        let Some(built) = build_path2d(path) else {
            return false;
        };
        self.hit_test_stroke(&built, x, y)
    }

    // ─── Pixel access ───────────────────────────────────────────────────

    /// Read pixels back, **un-premultiplied**.
    ///
    /// The pixmap stores premultiplied RGBA and `ImageData` is defined as
    /// straight RGBA, so this divides the alpha back out. Skipping that step is
    /// what makes a `getImageData` → `putImageData` round trip darken a
    /// semi-transparent image a little more on each pass.
    ///
    /// A rectangle reaching outside the bitmap is legal: the spec fills the
    /// outside part with transparent black rather than failing, so the result
    /// is always `sw × sh`.
    /// Encode the bitmap.
    ///
    /// **PNG goes through tiny-skia's own `encode_png`**, which is already how
    /// `vybex::gui_capture` takes a screenshot — it returns the bytes in memory
    /// and demultiplies on the way out, which is exactly what a PNG needs.
    /// Hand-rolling the encode here would have been a second copy of a path
    /// this repo already relies on.
    ///
    /// JPEG cannot: tiny-skia has no JPEG encoder. It also has no alpha, so the
    /// image is composited onto white first — the spec says a transparent
    /// canvas exports as black under a format without alpha, but every browser
    /// uses white and a page saving a chart expects white.
    fn to_blob(&self, mime: &str, quality: Option<f32>) -> Option<Vec<u8>> {
        match mime {
            "image/png" => self.pixmap.encode_png().ok(),
            "image/jpeg" => {
                let (w, h) = (self.pixmap.width(), self.pixmap.height());
                // Straight RGBA first, for the same reason `encode_png`
                // demultiplies: the compositing below is in un-premultiplied
                // colour, and doing it on premultiplied bytes washes out every
                // semi-transparent pixel.
                let straight = self.get_image_data(0, 0, w, h)?;
                let mut rgb = Vec::with_capacity((w * h * 3) as usize);
                for px in straight.data.chunks_exact(4) {
                    let a = px[3] as u32;
                    // Over white: `c*a + 255*(1-a)`, in integer form.
                    let over = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
                    rgb.extend_from_slice(&[over(px[0]), over(px[1]), over(px[2])]);
                }
                let buffer: image::RgbImage = image::ImageBuffer::from_raw(w, h, rgb)?;
                // The spec's `quality` is 0.0..=1.0; the encoder wants 1..=100.
                // Out-of-range or absent means the UA's default, and 0.92 is
                // the one every browser uses.
                let q = quality.filter(|q| (0.0..=1.0).contains(q)).unwrap_or(0.92);
                let mut out = std::io::Cursor::new(Vec::new());
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut out,
                    (q * 100.0).round().clamp(1.0, 100.0) as u8,
                )
                .encode_image(&image::DynamicImage::ImageRgb8(buffer))
                .ok()?;
                Some(out.into_inner())
            }
            // See the trait's note: a format this cannot encode answers `None`
            // rather than PNG bytes under someone else's MIME type.
            _ => None,
        }
    }

    fn get_image_data(&self, sx: i32, sy: i32, sw: u32, sh: u32) -> Option<super::ImageData> {
        if sw == 0 || sh == 0 {
            return None;
        }
        let mut out = vec![0u8; (sw as usize) * (sh as usize) * 4];
        let pix_w = self.pixmap.width() as i32;
        let pix_h = self.pixmap.height() as i32;
        let pixels = self.pixmap.pixels();
        for row in 0..sh as i32 {
            let src_y = sy + row;
            if src_y < 0 || src_y >= pix_h {
                continue;
            }
            for col in 0..sw as i32 {
                let src_x = sx + col;
                if src_x < 0 || src_x >= pix_w {
                    continue;
                }
                let p = pixels[(src_y * pix_w + src_x) as usize];
                let a = p.alpha();
                let dst = (((row * sw as i32) + col) * 4) as usize;
                // Fully transparent premultiplied pixels carry no colour to
                // recover, and dividing by zero would invent one.
                let (r, g, b) = if a == 0 {
                    (0, 0, 0)
                } else {
                    (
                        (p.red() as u32 * 255 / a as u32).min(255) as u8,
                        (p.green() as u32 * 255 / a as u32).min(255) as u8,
                        (p.blue() as u32 * 255 / a as u32).min(255) as u8,
                    )
                };
                out[dst] = r;
                out[dst + 1] = g;
                out[dst + 2] = b;
                out[dst + 3] = a;
            }
        }
        super::ImageData::from_rgba(sw, sh, out)
    }
}

impl<'a> TinySkiaCanvas<'a> {
    /// Run one drawing operation through the spec's drawing model.
    ///
    /// HTML §4.12.5.1.13 does not say "paint the shape". It says: render the
    /// shape to its OWN bitmap, apply `filter` to that bitmap, derive the
    /// shadow from the filtered bitmap's alpha, then composite the shadow and
    /// the bitmap onto the canvas. Every draw method goes through here so that
    /// order is written once rather than seven times.
    ///
    /// **The fast path is the common path.** With no shadow and no filter the
    /// model is indistinguishable from painting straight at the canvas, so that
    /// is what happens — no allocation, no copy, byte-identical to before this
    /// existed. The layer is only paid for when something asked for it.
    ///
    /// Two details that are easy to get backwards:
    ///
    /// - **Compositing moves to the end.** `globalCompositeOperation` describes
    ///   how the drawing meets the CANVAS, not how it meets its own empty
    ///   layer. So the closure is handed a state with the mode forced to
    ///   `source-over`, and the real mode is applied when the layer lands.
    ///   Leaving it in the closure would make `destination-out` erase the blank
    ///   layer and then composite nothing.
    /// - **The shadow offset ignores the transform.** The spec is explicit that
    ///   `shadowOffsetX`/`Y` are not affected by the CTM, so the layer is
    ///   offset in device pixels rather than transformed.
    fn with_effects(&mut self, draw: impl FnOnce(&mut Pixmap, &PaintState, Option<&Mask>)) {
        let filters = self.parsed_filter();
        let shadow = self.state.shadow.clone();
        if !shadow.is_visible() && filters.ops.is_empty() {
            draw(self.pixmap, &self.state, self.clip_mask.as_ref());
            return;
        }

        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        let Some(mut layer) = Pixmap::new(w, h) else {
            return;
        };
        // See above: the layer is drawn into source-over whatever the context
        // asked for, and the real mode is used to land it.
        let mut layer_state = self.state.clone();
        layer_state.composite = super::CompositeOp::SourceOver;
        draw(&mut layer, &layer_state, self.clip_mask.as_ref());

        if !filters.ops.is_empty() {
            super::effects::apply_filter_list(&mut layer, &filters);
        }

        let paint = PixmapPaint {
            opacity: 1.0,
            blend_mode: self.state.composite.to_tiny_skia(),
            quality: FilterQuality::Nearest,
        };
        if shadow.is_visible() {
            // `shadowBlur` names TWICE the standard deviation — the spec says
            // the shadow is blurred by a Gaussian with a standard deviation of
            // half it. CSS `drop-shadow()` names the deviation directly, which
            // is why the two callers of `shadow_layer` pass different things.
            if let Some(cast) =
                super::effects::shadow_layer(&layer, shadow.color, shadow.blur / 2.0)
            {
                self.pixmap.draw_pixmap(
                    shadow.offset_x.round() as i32,
                    shadow.offset_y.round() as i32,
                    cast.as_ref(),
                    &paint,
                    Transform::identity(),
                    None,
                );
            }
        }
        self.pixmap
            .draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
    }

    /// The page's own transform — the current matrix with the device scale
    /// divided back out.
    ///
    /// Everything a page HANDS IN or READS BACK is in this space: `getTransform`
    /// reports it, and both hit tests invert it. Everything that touches pixels
    /// uses `state.transform`, which has the device scale still in it. Getting
    /// these two the wrong way round is silent — the drawing looks right and
    /// only the answers are wrong.
    fn page_transform(&self) -> Transform {
        match self.state.base.invert() {
            Some(inv) => inv.pre_concat(self.state.transform),
            // A non-invertible base cannot happen from `set_device_scale`,
            // which rejects a non-positive scale. Falling back to the full
            // matrix keeps a 1x surface exactly as it was.
            None => self.state.transform,
        }
    }

    /// The `filter` attribute, parsed.
    ///
    /// Goes through the engine's own `parse_css_filter` rather than a parser of
    /// its own: canvas `filter` and CSS `filter` are the same grammar, and the
    /// value came in as a string precisely so there would not be two of them.
    fn parsed_filter(&self) -> crate::types::CssFilters {
        let value = self.state.filter.trim();
        if value.is_empty() || value.eq_ignore_ascii_case("none") {
            return crate::types::CssFilters::default();
        }
        // `crate::css`, NOT `crate::css::transform_filter` — that file holds a
        // second copy of this parser and is not declared in `css/mod.rs`, so it
        // has never been compiled. The live one is here.
        crate::css::parse_css_filter(value)
    }

    /// Where the glyphs actually start, given `textAlign` and `textBaseline`.
    ///
    /// **`textAlign` and `textBaseline` decide what `x` and `y` NAME.**
    /// cosmic-text positions glyphs from the buffer's top-left, so using the
    /// anchor unmodified means `x` = left edge and `y` = TOP — that is
    /// `textAlign: left` with `textBaseline: top`, while the spec's defaults
    /// are `start` and **`alphabetic`**, where `y` is the BASELINE and the
    /// glyphs sit above it.
    ///
    /// Shared by `fill_text` and `stroke_text` so a filled string and a stroked
    /// one land in the same place — drawing both is how a page gets outlined
    /// text, and a half-pixel disagreement between them would show.
    ///
    /// Both offsets are read from the SHAPED buffer rather than estimated:
    /// `line_w` is the real advance and `line_y` the real baseline offset.
    fn text_origin(&self, buf: &Buffer, x: f32, y: f32, line_height: f32) -> (f32, f32) {
        let px = x * self.state.transform.sx + self.state.transform.tx;
        let py = y * self.state.transform.sy + self.state.transform.ty;
        let (line_w, baseline) = buf
            .layout_runs()
            .next()
            .map(|run| (run.line_w, run.line_y))
            .unwrap_or((0.0, 0.0));
        // **`start` and `end` are LOGICAL, and `direction` is what resolves
        // them.** Under `rtl` a `start`-aligned string begins at the right edge
        // and runs left, so `start` means what `right` means and `end` means
        // what `left` means. Treating them as left and right unconditionally is
        // the whole of what an unapplied `direction` attribute looks like: a
        // right-to-left label aligns off the wrong end of its box.
        //
        // `inherit` takes the canvas element's direction, and a bare canvas has
        // none to inherit, so it resolves to `ltr` — the same answer a browser
        // gives for a canvas outside any RTL context.
        let rtl = self.state.direction == super::Direction::Rtl;
        let px = match self.state.text_align {
            super::TextAlign::Left => px,
            super::TextAlign::Right => px - line_w,
            super::TextAlign::Start => {
                if rtl {
                    px - line_w
                } else {
                    px
                }
            }
            super::TextAlign::End => {
                if rtl {
                    px
                } else {
                    px - line_w
                }
            }
            super::TextAlign::Center => px - line_w / 2.0,
        };
        let py = match self.state.text_baseline {
            // `hanging` is not the same line as `top` in a font with real
            // metrics, but both sit at the head of the em box and this shaping
            // path surfaces only one of them — stated rather than pretended.
            super::TextBaseline::Top | super::TextBaseline::Hanging => py,
            super::TextBaseline::Middle => py - line_height / 2.0,
            super::TextBaseline::Alphabetic => py - baseline,
            super::TextBaseline::Ideographic | super::TextBaseline::Bottom => py - line_height,
        };
        (px, py)
    }

    /// Is `(x, y)` inside `path` under `rule`?
    ///
    /// The point arrives in CANVAS space and the path is held in USER space —
    /// the transform is applied when painting, not when building — so the point
    /// is mapped back through the inverse of the current matrix before the
    /// test. That is what makes a hit test keep working after the caller has
    /// scaled or rotated, which is the whole reason the spec specifies it this
    /// way.
    ///
    /// A singular matrix (a zero scale) has no inverse, and nothing is visible
    /// under one, so nothing can be hit either.
    fn hit_test_fill(&self, path: &Path, x: f32, y: f32, rule: super::FillRule) -> bool {
        // The PAGE's transform, not the full one. `isPointInPath` takes a point
        // in the space the page's own matrix maps INTO, and the device scale is
        // below that — inverting through it as well would divide the point by
        // the pixel ratio and answer about somewhere else entirely, so on a 2x
        // display every hit test would be wrong by half.
        let Some(inverse) = super::Matrix::from_tiny_skia(self.page_transform()).invert() else {
            return false;
        };
        let (ux, uy) = inverse.apply(x, y);
        point_in_path(path, ux, uy, rule)
    }

    /// Is `(x, y)` on the stroke of `path`?
    ///
    /// Answered by asking tiny-skia for the stroke's OUTLINE and testing
    /// containment in that. Building the outline is the honest way round: the
    /// stroke's extent depends on width, cap, join and miter limit together,
    /// and a distance-to-segment approximation would disagree with the pixels
    /// at exactly the corners and end caps where a caller is most likely to
    /// click.
    fn hit_test_stroke(&self, path: &Path, x: f32, y: f32) -> bool {
        // The page's transform, for the same reason as `hit_test_fill`.
        let Some(inverse) = super::Matrix::from_tiny_skia(self.page_transform()).invert() else {
            return false;
        };
        let (ux, uy) = inverse.apply(x, y);
        let mut stroke = TsStroke::default();
        stroke.width = self.state.line_width;
        stroke.line_cap = line_cap_to_ts(self.state.line_cap);
        stroke.line_join = line_join_to_ts(self.state.line_join);
        stroke.miter_limit = self.state.miter_limit;
        let Some(outline) = path.stroke(&stroke, 1.0) else {
            return false;
        };
        // The outline is a filled region, and it is built so that its interior
        // is the stroke — nonzero is the rule that reads it that way.
        point_in_path(&outline, ux, uy, super::FillRule::NonZero)
    }
}

/// Turn a recorded [`super::Path2D`] into a tiny-skia path.
///
/// Goes through a throwaway [`TinySkiaCanvas`]-free builder rather than the
/// canvas's own `path`, because hit-testing a `Path2D` must not disturb the
/// path the context is in the middle of building.
fn build_path2d(path: &super::Path2D) -> Option<Path> {
    // A 1×1 scratch pixmap: `append_path` is a `Canvas` method, so it needs a
    // canvas, and the pixels are never touched — only the path builder is.
    let mut scratch = Pixmap::new(1, 1)?;
    let mut canvas = TinySkiaCanvas::new(&mut scratch);
    canvas.append_path(path);
    canvas.path.clone().finish()
}

/// Flatten `path` into closed polylines and test containment.
///
/// Curves are subdivided into line segments; every subpath is treated as closed
/// because a fill closes it, which is what the spec says for both rules.
fn point_in_path(path: &Path, x: f32, y: f32, rule: super::FillRule) -> bool {
    // Cheap reject first — a point outside the bounds cannot be inside the
    // path, and this is the common answer when hit-testing many paths.
    let bounds = path.bounds();
    if x < bounds.left() || x > bounds.right() || y < bounds.top() || y > bounds.bottom() {
        return false;
    }

    let mut winding = 0i32;
    let mut crossings = 0u32;
    let mut start = (0.0f32, 0.0f32);
    let mut current = (0.0f32, 0.0f32);

    // One ray cast, accumulated across every segment: a horizontal ray to +x
    // from the query point. `winding` counts signed crossings for `nonzero`,
    // `crossings` counts them unsigned for `evenodd`.
    let cross = |from: (f32, f32), to: (f32, f32), winding: &mut i32, crossings: &mut u32| {
        // A segment straddles the ray when exactly one endpoint is above it.
        // The half-open comparison is what stops a vertex exactly on the ray
        // from being counted twice.
        let (upward, downward) = (from.1 <= y && to.1 > y, from.1 > y && to.1 <= y);
        if !upward && !downward {
            return;
        }
        let t = (y - from.1) / (to.1 - from.1);
        if from.0 + t * (to.0 - from.0) > x {
            *crossings += 1;
            *winding += if upward { 1 } else { -1 };
        }
    };

    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                // An unclosed subpath still closes for the purpose of filling.
                cross(current, start, &mut winding, &mut crossings);
                start = (p.x, p.y);
                current = start;
            }
            PathSegment::LineTo(p) => {
                let next = (p.x, p.y);
                cross(current, next, &mut winding, &mut crossings);
                current = next;
            }
            PathSegment::QuadTo(c, p) => {
                let next = (p.x, p.y);
                for (from, to) in flatten_quad(current, (c.x, c.y), next) {
                    cross(from, to, &mut winding, &mut crossings);
                }
                current = next;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let next = (p.x, p.y);
                for (from, to) in flatten_cubic(current, (c1.x, c1.y), (c2.x, c2.y), next) {
                    cross(from, to, &mut winding, &mut crossings);
                }
                current = next;
            }
            PathSegment::Close => {
                cross(current, start, &mut winding, &mut crossings);
                current = start;
            }
        }
    }
    // The final subpath, when the path ended without an explicit close.
    cross(current, start, &mut winding, &mut crossings);

    match rule {
        super::FillRule::NonZero => winding != 0,
        super::FillRule::EvenOdd => crossings % 2 == 1,
    }
}

/// How many line segments a curve is broken into for hit testing.
///
/// Fixed rather than adaptive: this is a containment test, not a rasterisation,
/// and the error that matters is whether a click near the edge lands on the
/// right side. Sixteen segments puts that error well under a pixel for curves
/// of any size a pointer can be aimed at.
const CURVE_STEPS: usize = 16;

fn flatten_quad(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
) -> impl Iterator<Item = ((f32, f32), (f32, f32))> {
    let mut previous = p0;
    (1..=CURVE_STEPS).map(move |step| {
        let t = step as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        let point = (
            u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
            u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
        );
        let segment = (previous, point);
        previous = point;
        segment
    })
}

fn flatten_cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
) -> impl Iterator<Item = ((f32, f32), (f32, f32))> {
    let mut previous = p0;
    (1..=CURVE_STEPS).map(move |step| {
        let t = step as f32 / CURVE_STEPS as f32;
        let u = 1.0 - t;
        let point = (
            u * u * u * p0.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
            u * u * u * p0.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
        );
        let segment = (previous, point);
        previous = point;
        segment
    })
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Multiply a colour's alpha channel by `alpha` (0.0..1.0). Used by
/// `fill_paint` / `stroke_paint` so the canvas-trait `set_global_alpha`
/// works on top of any explicit colour alpha.
fn apply_alpha(color: Color, alpha: f32) -> Color {
    let a = (color.a as f32 * alpha.clamp(0.0, 1.0)).round() as u8;
    Color { a, ..color }
}

/// Set a `tiny_skia::Paint`'s shader from a canvas `fillStyle`/`strokeStyle`.
///
/// A colour becomes a flat shader; a gradient and a pattern become real
/// tiny-skia shaders, so this is where `createLinearGradient` stops being a
/// recorded intent and starts painting pixels.
///
/// `global_alpha` multiplies into every stop rather than being applied once at
/// the end, because tiny-skia has no paint-level opacity for gradients — the
/// alpha has to travel in the colours.
fn apply_canvas_paint<'a>(paint: &mut Paint<'a>, style: &'a CanvasPaint, global_alpha: f32) {
    match style {
        CanvasPaint::Color(color) => {
            let c = apply_alpha(*color, global_alpha);
            paint.set_color_rgba8(c.r, c.g, c.b, c.a);
        }
        CanvasPaint::Gradient(gradient) => {
            let stops: Vec<GradientStop> = gradient
                .sorted_stops()
                .iter()
                .map(|stop| {
                    let c = apply_alpha(stop.color, global_alpha);
                    GradientStop::new(stop.offset, c.to_tiny_skia())
                })
                .collect();

            // A gradient with no stops paints NOTHING — the spec is explicit
            // that it is transparent black, not an error and not a default
            // colour. One stop is a flat fill of that colour, which is also
            // what tiny-skia would refuse to build a shader for.
            if stops.is_empty() {
                paint.set_color_rgba8(0, 0, 0, 0);
                return;
            }
            if stops.len() == 1 {
                let c = apply_alpha(gradient.stops[0].color, global_alpha);
                paint.set_color_rgba8(c.r, c.g, c.b, c.a);
                return;
            }

            // `SpreadMode::Pad` is the spec's behaviour: outside the 0..1
            // range the end stops extend, they do not repeat or mirror.
            let shader = match gradient.kind {
                GradientKind::Linear { x0, y0, x1, y1 } => LinearGradient::new(
                    TsPoint::from_xy(x0, y0),
                    TsPoint::from_xy(x1, y1),
                    stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                ),
                // tiny-skia's radial gradient is the TWO-CIRCLE form — start
                // circle, end circle, a radius each — which is exactly what
                // `createRadialGradient(x0,y0,r0,x1,y1,r1)` specifies, `r0`
                // included. So an annular gradient and an off-centre highlight
                // are both expressible, not approximated.
                //
                // It answers `None` for a degenerate pair (equal centres AND
                // equal radii), which the spec also paints as nothing; the
                // fallback below keeps the shape visible in that case.
                GradientKind::Radial {
                    x0,
                    y0,
                    r0,
                    x1,
                    y1,
                    r1,
                } => RadialGradient::new(
                    TsPoint::from_xy(x0, y0),
                    r0,
                    TsPoint::from_xy(x1, y1),
                    r1,
                    stops,
                    SpreadMode::Pad,
                    Transform::identity(),
                ),
                // ⚠ tiny-skia has NO conic gradient. Falling back to the first
                // stop keeps the shape visible and the wrongness obvious in a
                // capture, which is the honest degradation — a transparent
                // fill would make the shape vanish and look like a layout bug.
                GradientKind::Conic { .. } => None,
            };

            match shader {
                Some(shader) => paint.shader = shader,
                None => {
                    let c = apply_alpha(gradient.stops[0].color, global_alpha);
                    paint.set_color_rgba8(c.r, c.g, c.b, c.a);
                }
            }
        }
        CanvasPaint::Pattern(pattern) => {
            let Some(pixmap) = PixmapRef::from_bytes(
                &pattern.image.pixels,
                pattern.image.width,
                pattern.image.height,
            ) else {
                paint.set_color_rgba8(0, 0, 0, 0);
                return;
            };
            // ⚠ `repeat-x` / `repeat-y` cannot be expressed: tiny-skia's
            // spread mode is one setting for both axes, so a one-axis
            // repetition tiles in both. `no-repeat` is `Pad`, which extends
            // the edge pixels rather than leaving transparency — visible, and
            // closer than tiling would be.
            let spread = match pattern.repetition {
                Repetition::Repeat | Repetition::RepeatX | Repetition::RepeatY => {
                    SpreadMode::Repeat
                }
                Repetition::NoRepeat => SpreadMode::Pad,
            };
            paint.shader = tiny_skia::Pattern::new(
                pixmap,
                spread,
                FilterQuality::Nearest,
                global_alpha.clamp(0.0, 1.0),
                Transform::identity(),
            );
        }
    }
}

fn line_cap_to_ts(cap: LineCap) -> TsLineCap {
    match cap {
        LineCap::Butt => TsLineCap::Butt,
        LineCap::Round => TsLineCap::Round,
        LineCap::Square => TsLineCap::Square,
    }
}

fn line_join_to_ts(join: LineJoin) -> TsLineJoin {
    match join {
        LineJoin::Miter => TsLineJoin::Miter,
        LineJoin::Round => TsLineJoin::Round,
        LineJoin::Bevel => TsLineJoin::Bevel,
    }
}

/// Build a `cosmic_text::Attrs` from a canvas `Font`.
///
/// Maps our `FontWeight` / `FontStyle` to cosmic-text's equivalents and
/// resolves the family name. Family is owned by the `Font` so we can
/// borrow it for the lifetime of the returned `Attrs`. The `'static`
/// hack via `Family::Name(&str)` means the borrow lifetime tracks the
/// `Font`'s — fine because `Attrs` doesn't escape this function.
fn build_attrs<'f>(state: &'f PaintState) -> Attrs<'f> {
    let font = &state.font;
    let stretch = state.font_stretch;
    let family = if font.family.is_empty() || font.family == "sans-serif" {
        Family::SansSerif
    } else if font.family == "monospace" {
        Family::Monospace
    } else if font.family == "serif" {
        Family::Serif
    } else {
        Family::Name(&font.family)
    };
    let weight = match font.weight {
        FontWeight::Normal => cosmic_text::Weight::NORMAL,
        FontWeight::Bold => cosmic_text::Weight::BOLD,
    };
    let style = match font.style {
        FontStyle::Normal => cosmic_text::Style::Normal,
        FontStyle::Italic => cosmic_text::Style::Italic,
    };
    // **The text attributes reach the SHAPER, not just the state.**
    //
    // Every one of these changes the glyphs or their positions, so applying
    // them here — before shaping — is also what makes `measureText` agree with
    // what gets drawn. Setting them on the state alone would give a canvas that
    // reports `letterSpacing = "4px"` and measures and paints as if it were
    // zero.
    let mut features = cosmic_text::FontFeatures::new();
    match state.font_kerning {
        // `auto` is the shaper's own default, so it is left alone rather than
        // forced on: a font that suppresses kerning for a script has a reason.
        super::FontKerning::Auto => {}
        super::FontKerning::Normal => {
            features.enable(cosmic_text::FeatureTag::KERNING);
        }
        super::FontKerning::None => {
            features.disable(cosmic_text::FeatureTag::KERNING);
        }
    }
    match state.font_variant_caps {
        super::FontVariantCaps::Normal => {}
        // `petite-caps` and `all-petite-caps` have their own OpenType tags
        // (`pcap`, `c2pc`) that cosmic-text does not name. Small caps is the
        // nearest real feature and is what a font without petite cuts would
        // synthesise anyway — noted rather than silently treated as `normal`.
        super::FontVariantCaps::SmallCaps
        | super::FontVariantCaps::PetiteCaps
        | super::FontVariantCaps::Unicase
        | super::FontVariantCaps::TitlingCaps => {
            features.enable(cosmic_text::FeatureTag::SMALL_CAPS);
        }
        super::FontVariantCaps::AllSmallCaps | super::FontVariantCaps::AllPetiteCaps => {
            features.enable(cosmic_text::FeatureTag::ALL_SMALL_CAPS);
        }
    }

    let attrs = Attrs::new()
        .family(family)
        .weight(weight)
        .style(style)
        // `fontStretch` selects a different FACE from the family (a condensed
        // cut, an expanded one), so a font that has those cuts uses them and
        // one that does not falls back the way it does for weight.
        .stretch(stretch.to_cosmic())
        .font_features(features);

    match parse_css_length(&state.letter_spacing, font.size) {
        Some(spacing) if spacing != 0.0 => attrs.letter_spacing(spacing),
        _ => attrs,
    }
}

/// Every glyph in `buf` as one tiny-skia path, positioned at `(px, py)`.
///
/// One path rather than one per glyph so the whole string strokes as a unit —
/// a dash pattern runs continuously along it, and the shadow the drawing model
/// derives is the shadow of the text rather than of each letter separately.
///
/// **Font space is y-UP; the canvas is y-DOWN.** swash reports outlines in the
/// font's own orientation, so every y is negated on the way in. Skipping that
/// draws the string upside down, which is the kind of thing that looks like a
/// baseline bug from a distance.
fn glyph_outlines(tc: &mut TextCtx<'_>, buf: &Buffer, px: f32, py: f32) -> Option<Path> {
    use swash::scale::ScaleContext;
    use swash::zeno::Verb;

    let mut context = ScaleContext::new();
    let mut builder = PathBuilder::new();
    let mut any = false;

    for run in buf.layout_runs() {
        // The run's baseline. Glyph outlines are relative to it, and `py` is
        // already the origin `text_origin` resolved.
        let base_y = py + run.line_y;
        for glyph in run.glyphs {
            let Some(font) = tc.font_system.get_font(glyph.font_id, glyph.font_weight) else {
                continue;
            };
            let mut scaler = context
                .builder(font.as_swash())
                .size(glyph.font_size)
                .hint(false)
                .build();
            let Some(outline) = scaler.scale_outline(glyph.glyph_id) else {
                // A glyph with no outline is not an error — a space has none,
                // and a bitmap-only emoji face has none either. It contributes
                // nothing to a stroke, which is the correct amount.
                continue;
            };
            let ox = px + glyph.x;
            let oy = base_y + glyph.y;
            let points = outline.points();
            let mut i = 0usize;
            for verb in outline.verbs() {
                match verb {
                    Verb::MoveTo => {
                        let p = points[i];
                        builder.move_to(ox + p.x, oy - p.y);
                        i += 1;
                        any = true;
                    }
                    Verb::LineTo => {
                        let p = points[i];
                        builder.line_to(ox + p.x, oy - p.y);
                        i += 1;
                    }
                    Verb::QuadTo => {
                        let (c, p) = (points[i], points[i + 1]);
                        builder.quad_to(ox + c.x, oy - c.y, ox + p.x, oy - p.y);
                        i += 2;
                    }
                    Verb::CurveTo => {
                        let (c1, c2, p) = (points[i], points[i + 1], points[i + 2]);
                        builder.cubic_to(
                            ox + c1.x,
                            oy - c1.y,
                            ox + c2.x,
                            oy - c2.y,
                            ox + p.x,
                            oy - p.y,
                        );
                        i += 3;
                    }
                    Verb::Close => builder.close(),
                }
            }
        }
    }
    if !any {
        return None;
    }
    builder.finish()
}

/// Shape `text` under the current text state.
///
/// The single place text is shaped, so `fill_text` and `measure_text` cannot
/// disagree about what the glyphs are — they differ only in `scale`, which is
/// the one genuine difference between them: measuring answers in user units and
/// drawing happens in device pixels.
fn shape_text(tc: &mut TextCtx<'_>, state: &PaintState, text: &str, scale: f32) -> Buffer {
    let size = state.font.size;
    let metrics = Metrics::new(size, size * 1.3).scale(scale);
    let mut buf = Buffer::new(tc.font_system, metrics);
    let attrs = build_attrs(state);
    match word_spacing_spans(text, state, &attrs) {
        Some(spans) => buf.set_rich_text(
            tc.font_system,
            spans.iter().map(|(s, a)| (*s, a.clone())),
            &attrs,
            Shaping::Advanced,
            None,
        ),
        None => buf.set_text(tc.font_system, text, &attrs, Shaping::Advanced, None),
    }
    buf.shape_until_scroll(tc.font_system, false);
    buf
}

/// Split `text` so each word separator carries its own spacing.
///
/// **`wordSpacing` has no counterpart in cosmic-text**, which knows letter
/// spacing and nothing about words. But letter spacing is per-SPAN, and a word
/// separator is a span — so giving each space its own run with `letterSpacing +
/// wordSpacing` puts the extra advance exactly where CSS says it goes, using
/// the shaper's own mechanism rather than post-processing its output.
///
/// `None` when there is no word spacing to apply, so the ordinary path stays a
/// single `set_text` with no allocation.
fn word_spacing_spans<'a>(
    text: &'a str,
    state: &PaintState,
    base: &Attrs<'a>,
) -> Option<Vec<(&'a str, Attrs<'a>)>> {
    let word = parse_css_length(&state.word_spacing, state.font.size)?;
    if word == 0.0 {
        return None;
    }
    let letter = parse_css_length(&state.letter_spacing, state.font.size).unwrap_or(0.0);
    let spaced = base.clone().letter_spacing(letter + word);

    let mut spans = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(' ') {
            None => {
                spans.push((rest, base.clone()));
                break;
            }
            Some(at) => {
                if at > 0 {
                    spans.push((&rest[..at], base.clone()));
                }
                spans.push((&rest[at..at + 1], spaced.clone()));
                rest = &rest[at + 1..];
            }
        }
    }
    Some(spans)
}

/// A CSS length in `px` or `em`, for `letterSpacing` and `wordSpacing`.
///
/// `None` for `normal`, the empty string, or anything with a unit this cannot
/// resolve — all of which mean "no adjustment" rather than zero-and-carry-on,
/// which is the same thing here but says so.
///
/// `em` needs the font size, which is why it is a parameter: the two spacing
/// attributes are resolved against the font in effect when they are used, not
/// when they are set.
fn parse_css_length(value: &str, font_size: f32) -> Option<f32> {
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("normal") {
        return None;
    }
    if let Some(px) = value.strip_suffix("px") {
        return px.trim().parse::<f32>().ok();
    }
    if let Some(em) = value.strip_suffix("em") {
        return em.trim().parse::<f32>().ok().map(|v| v * font_size);
    }
    // A bare number is not a valid CSS length, but it is what a caller that
    // forgot the unit passes, and treating it as pixels is what every browser
    // does in quirks mode.
    value.parse::<f32>().ok()
}

#[cfg(test)]
mod path_lifetime_tests {
    use super::*;
    use crate::canvas::{Canvas as _, Color, FillRule, Path2D};

    fn pixmap() -> Pixmap {
        Pixmap::new(60, 60).expect("a 60x60 pixmap")
    }

    fn at(p: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let px = p.pixels()[(y * p.width() + x) as usize];
        [px.red(), px.green(), px.blue(), px.alpha()]
    }

    #[test]
    fn filling_does_not_consume_the_current_path() {
        // §4.12.5 resets the current path in ONE place: `beginPath()`. So
        // `rect(); fill(); stroke();` — how a page draws an outlined box — must
        // paint both. The fill used to take the path away, and the stroke then
        // found nothing and drew nothing, silently.
        let mut pm = pixmap();
        let mut c = TinySkiaCanvas::new(&mut pm);
        c.begin_path();
        c.rect(10.0, 10.0, 30.0, 30.0);
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill();
        c.set_stroke_color(Color::rgb(0, 0, 255));
        c.set_line_width(4.0);
        c.stroke();

        drop(c);
        assert_eq!(at(&pm, 25, 25), [255, 0, 0, 255], "the fill is missing");
        assert_eq!(
            at(&pm, 10, 25),
            [0, 0, 255, 255],
            "the stroke is missing — `fill` consumed the path"
        );
    }

    #[test]
    fn begin_path_is_what_clears_it() {
        let mut pm = pixmap();
        let mut c = TinySkiaCanvas::new(&mut pm);
        c.begin_path();
        c.rect(10.0, 10.0, 30.0, 30.0);
        c.begin_path();
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill();
        assert_eq!(
            at(&pm, 25, 25),
            [0, 0, 0, 0],
            "beginPath did not clear the current path"
        );
    }

    #[test]
    fn filling_a_path2d_leaves_the_current_path_standing() {
        // The isolation a `Path2D` exists for. The trait's default builds the
        // shape INTO the current path, which destroys what the context was
        // describing — its own doc comment said it must not.
        let mut pm = pixmap();
        let mut c = TinySkiaCanvas::new(&mut pm);
        c.begin_path();
        c.rect(10.0, 10.0, 20.0, 20.0);

        let mut shape = Path2D::new();
        shape.rect(35.0, 35.0, 15.0, 15.0);
        c.set_fill_color(Color::rgb(0, 255, 0));
        c.fill_path(&shape, FillRule::NonZero);

        // ...and the context's own path is still there to fill.
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill();
        drop(c);
        assert_eq!(
            at(&pm, 42, 42),
            [0, 255, 0, 255],
            "the Path2D never painted"
        );
        assert_eq!(
            at(&pm, 20, 20),
            [255, 0, 0, 255],
            "filling a Path2D destroyed the current path"
        );
    }

    #[test]
    fn clipping_to_a_path2d_leaves_the_current_path_standing() {
        // `clip` is the case that made this unrecoverable: it discards the
        // current path as part of its own semantics, so nothing downstream
        // could put back what the default had thrown away.
        let mut pm = pixmap();
        let mut c = TinySkiaCanvas::new(&mut pm);
        c.begin_path();
        c.rect(0.0, 0.0, 60.0, 60.0);

        let mut window = Path2D::new();
        window.rect(20.0, 20.0, 20.0, 20.0);
        c.clip_path(&window, FillRule::NonZero);

        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill();
        drop(c);
        assert_eq!(at(&pm, 30, 30), [255, 0, 0, 255], "inside the clip");
        assert_eq!(at(&pm, 5, 5), [0, 0, 0, 0], "outside the clip");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{FillRule, Matrix, Path2D};

    fn pixmap() -> Pixmap {
        Pixmap::new(100, 100).expect("a 100x100 pixmap")
    }

    /// A 20x20 square whose top-left corner is at (10, 10).
    fn square(canvas: &mut TinySkiaCanvas) {
        canvas.begin_path();
        canvas.move_to(10.0, 10.0);
        canvas.line_to(30.0, 10.0);
        canvas.line_to(30.0, 30.0);
        canvas.line_to(10.0, 30.0);
        canvas.close_path();
    }

    // ─── isPointInPath ──────────────────────────────────────────────────

    #[test]
    fn a_point_inside_the_path_is_inside_it() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        square(&mut c);
        assert!(c.is_point_in_path(20.0, 20.0, FillRule::NonZero));
    }

    #[test]
    fn a_point_outside_the_path_is_outside_it() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        square(&mut c);
        assert!(!c.is_point_in_path(5.0, 5.0, FillRule::NonZero));
        assert!(!c.is_point_in_path(50.0, 20.0, FillRule::NonZero));
        // Directly above and below, to catch a ray cast that ignores the
        // segment's y-range.
        assert!(!c.is_point_in_path(20.0, 5.0, FillRule::NonZero));
        assert!(!c.is_point_in_path(20.0, 50.0, FillRule::NonZero));
    }

    #[test]
    fn the_hit_test_follows_the_current_transform() {
        // The point is in CANVAS space and the path is in USER space, so a
        // scaled canvas must still answer about where the shape appears. This
        // is the case a hit test that ignores the matrix gets wrong, and it
        // gets it wrong silently.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.scale(2.0, 2.0);
        square(&mut c);
        // The square is drawn at 20..60 on screen once scaled.
        assert!(c.is_point_in_path(40.0, 40.0, FillRule::NonZero));
        // …and (20, 20) in USER space is no longer inside it on screen.
        assert!(!c.is_point_in_path(15.0, 15.0, FillRule::NonZero));
    }

    #[test]
    fn nonzero_and_evenodd_disagree_about_a_hole() {
        // Two nested squares wound the SAME way: nonzero counts +2 in the
        // middle and calls it solid, even-odd counts two crossings and calls it
        // a hole. A canvas that ignores the fill rule reports one of these for
        // both, and this is the shape that shows it.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.begin_path();
        c.rect(0.0, 0.0, 40.0, 40.0);
        c.rect(10.0, 10.0, 20.0, 20.0);
        assert!(
            c.is_point_in_path(20.0, 20.0, FillRule::NonZero),
            "nonzero: the windings add, so the middle is solid"
        );
        assert!(
            !c.is_point_in_path(20.0, 20.0, FillRule::EvenOdd),
            "even-odd: two crossings, so the middle is a hole"
        );
    }

    #[test]
    fn an_empty_path_contains_nothing() {
        let mut p = pixmap();
        let c = TinySkiaCanvas::new(&mut p);
        assert!(!c.is_point_in_path(0.0, 0.0, FillRule::NonZero));
    }

    // ─── isPointInStroke ────────────────────────────────────────────────

    #[test]
    fn a_point_on_the_stroke_is_on_it_and_the_interior_is_not() {
        // The question `isPointInStroke` answers is not derivable from
        // `isPointInPath`: the middle of a thin-stroked square is inside the
        // path and NOT on its stroke.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_line_width(4.0);
        square(&mut c);
        assert!(c.is_point_in_stroke(10.0, 20.0), "on the left edge");
        assert!(
            !c.is_point_in_stroke(20.0, 20.0),
            "the middle is not stroke"
        );
    }

    #[test]
    fn a_thicker_line_widens_what_counts_as_its_stroke() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        square(&mut c);
        c.set_line_width(1.0);
        assert!(!c.is_point_in_stroke(14.0, 20.0), "4px in, with a 1px line");
        c.set_line_width(12.0);
        assert!(
            c.is_point_in_stroke(14.0, 20.0),
            "the same point, 12px line"
        );
    }

    #[test]
    fn an_open_path_has_a_stroke_and_no_interior() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_line_width(6.0);
        c.begin_path();
        c.move_to(10.0, 50.0);
        c.line_to(90.0, 50.0);
        assert!(c.is_point_in_stroke(50.0, 50.0));
        assert!(!c.is_point_in_path(50.0, 50.0, FillRule::NonZero));
    }

    // ─── Path2D hit testing ─────────────────────────────────────────────

    #[test]
    fn a_path2d_can_be_hit_tested_without_disturbing_the_current_path() {
        let mut path = Path2D::new();
        path.rect(50.0, 50.0, 20.0, 20.0);

        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        // A DIFFERENT shape is mid-build on the context.
        square(&mut c);

        assert!(c.is_point_in_path2d(&path, 60.0, 60.0, FillRule::NonZero));
        assert!(!c.is_point_in_path2d(&path, 20.0, 20.0, FillRule::NonZero));
        // …and the context's own path is untouched by the question.
        assert!(c.is_point_in_path(20.0, 20.0, FillRule::NonZero));
    }

    // ─── reset ──────────────────────────────────────────────────────────

    #[test]
    fn reset_clears_the_bitmap_as_well_as_the_state() {
        // The half that is easy to leave out. A `reset` that only restored the
        // state would leave the previous drawing on screen.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(0.0, 0.0, 100.0, 100.0);
        c.translate(10.0, 10.0);
        c.set_line_width(9.0);
        c.reset();

        assert_eq!(c.get_transform(), Matrix::IDENTITY);
        assert_eq!(c.state.line_width, 1.0, "back to the initial value");
        assert!(
            p.pixels().iter().all(|px| px.alpha() == 0),
            "every pixel is transparent black again"
        );
    }

    // ─── getImageData ───────────────────────────────────────────────────

    #[test]
    fn get_image_data_reads_back_straight_rgba() {
        // The pixmap stores PREMULTIPLIED pixels and `ImageData` is defined as
        // straight RGBA. Half-transparent red is stored as (128, 0, 0, 128) and
        // must read back as (255, 0, 0, 128) — skipping the divide is what
        // darkens an image a little on every round trip.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_fill_color(Color::rgba(255, 0, 0, 128));
        c.fill_rect(0.0, 0.0, 10.0, 10.0);

        let data = c.get_image_data(2, 2, 1, 1).expect("a 1x1 read");
        assert_eq!(data.width, 1);
        assert_eq!(data.data[3], 128, "alpha survives");
        assert!(
            data.data[0] > 250,
            "red is un-premultiplied back to full, got {}",
            data.data[0]
        );
    }

    #[test]
    fn a_read_outside_the_bitmap_is_transparent_black_not_a_failure() {
        // The spec fills the outside part rather than failing, so the result is
        // always the size that was asked for.
        let mut p = pixmap();
        let c = TinySkiaCanvas::new(&mut p);
        let data = c.get_image_data(-5, -5, 4, 4).expect("still answers");
        assert_eq!(data.data.len(), 4 * 4 * 4);
        assert!(data.data.iter().all(|b| *b == 0));
    }

    #[test]
    fn an_empty_read_has_no_answer() {
        let mut p = pixmap();
        let c = TinySkiaCanvas::new(&mut p);
        assert!(c.get_image_data(0, 0, 0, 10).is_none());
    }

    // ─── measureText ────────────────────────────────────────────────────

    #[test]
    fn measure_text_reports_more_than_a_width() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let m = c.measure_text("Hg");
        assert!(m.width > 0.0, "a width, got {}", m.width);
        assert!(
            m.font_bounding_box_ascent > 0.0,
            "an ascent, got {}",
            m.font_bounding_box_ascent
        );
        // `H` has ink above the baseline and `g` below it, so both ink extents
        // are non-zero for this pair specifically.
        assert!(m.actual_bounding_box_ascent > 0.0, "H reaches up");
        assert!(m.actual_bounding_box_descent > 0.0, "g reaches down");
        // The alphabetic baseline is the reference every other y is measured
        // from, so its own offset is exactly zero.
        assert_eq!(m.alphabetic_baseline, 0.0);
        // The em box is the font size tall, split the way the font is.
        assert!(
            (m.em_height_ascent + m.em_height_descent - 20.0).abs() < 0.01,
            "em box is the font size, got {} + {}",
            m.em_height_ascent,
            m.em_height_descent
        );
    }

    #[test]
    fn whitespace_has_a_width_and_no_ink() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let m = c.measure_text("   ");
        assert!(m.width > 0.0, "spaces advance");
        assert_eq!(m.actual_bounding_box_ascent, 0.0, "and mark nothing");
        assert_eq!(m.actual_bounding_box_descent, 0.0);
    }

    #[test]
    fn a_canvas_with_no_font_system_measures_zero_rather_than_guessing() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        assert_eq!(c.measure_text("anything").width, 0.0);
    }

    // ─── direction ──────────────────────────────────────────────────────

    #[test]
    fn direction_rtl_flips_what_start_and_end_mean() {
        // `start`/`end` are LOGICAL. Under `rtl`, `start` is the RIGHT edge —
        // an unapplied `direction` shows up as a label aligned off the wrong
        // end of its box, which is invisible until someone renders Arabic.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();

        let leftmost = |canvas: &Pixmap| {
            (0..200u32)
                .find(|x| (0..200u32).any(|y| canvas.pixel(*x, y).expect("in bounds").alpha() > 0))
        };

        let mut ltr = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut ltr, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 40.0));
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.set_text_align(crate::canvas::TextAlign::Start);
        c.fill_text("abc", 100.0, 100.0);

        let mut rtl = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut rtl, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 40.0));
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.set_text_align(crate::canvas::TextAlign::Start);
        c.set_direction(crate::canvas::Direction::Rtl);
        c.fill_text("abc", 100.0, 100.0);

        let ltr_left = leftmost(&ltr).expect("ltr drew");
        let rtl_left = leftmost(&rtl).expect("rtl drew");
        assert!(
            ltr_left >= 98,
            "ltr `start` puts the text right of the anchor, got {ltr_left}"
        );
        assert!(
            rtl_left < 98,
            "rtl `start` puts it left of the anchor, got {rtl_left}"
        );
    }

    #[test]
    fn explicit_left_and_right_ignore_direction() {
        // Only the LOGICAL keywords follow `direction`; `left` is always left.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();

        let draw = |dir, out: &mut Pixmap, fs: &mut FontSystem, sc: &mut SwashCache| {
            let mut c = TinySkiaCanvas::with_text(out, fs, sc);
            c.set_font(&Font::new("sans-serif", 40.0));
            c.set_fill_color(Color::rgb(255, 0, 0));
            c.set_text_align(crate::canvas::TextAlign::Left);
            c.set_direction(dir);
            c.fill_text("abc", 100.0, 100.0);
        };
        let mut a = Pixmap::new(200, 200).expect("a pixmap");
        draw(
            crate::canvas::Direction::Ltr,
            &mut a,
            &mut font_system,
            &mut swash_cache,
        );
        let mut b = Pixmap::new(200, 200).expect("a pixmap");
        draw(
            crate::canvas::Direction::Rtl,
            &mut b,
            &mut font_system,
            &mut swash_cache,
        );
        assert_eq!(a.data(), b.data());
    }

    // ─── toDataURL / toBlob ─────────────────────────────────────────────

    #[test]
    fn to_data_url_produces_a_decodable_png_of_what_was_drawn() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(0.0, 0.0, 100.0, 100.0);

        let url = c.to_data_url("image/png", None);
        assert!(
            url.starts_with("data:image/png;base64,"),
            "got {}",
            &url[..40]
        );

        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(url.trim_start_matches("data:image/png;base64,"))
            .expect("valid base64");
        let decoded = image::load_from_memory(&bytes)
            .expect("a real PNG")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (100, 100));
        assert_eq!(decoded.get_pixel(50, 50).0, [255, 0, 0, 255]);
    }

    #[test]
    fn a_png_round_trip_keeps_translucent_colour_undarkened() {
        // The premultiply trap: writing the pixmap's own bytes would store
        // (128, 0, 0, 128) as if it were straight RGBA, and the colour would
        // come back half as bright.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_fill_color(Color::rgba(255, 0, 0, 128));
        c.fill_rect(0.0, 0.0, 100.0, 100.0);

        let bytes = c.to_blob("image/png", None).expect("encoded");
        let decoded = image::load_from_memory(&bytes)
            .expect("a real PNG")
            .to_rgba8();
        let px = decoded.get_pixel(50, 50).0;
        assert_eq!(px[3], 128, "alpha survives");
        assert!(px[0] > 250, "and red is still full, got {}", px[0]);
    }

    #[test]
    fn jpeg_composites_over_white_because_it_has_no_alpha() {
        let mut p = pixmap();
        let c = TinySkiaCanvas::new(&mut p);
        // Nothing drawn: a fully transparent canvas.
        let bytes = c.to_blob("image/jpeg", Some(1.0)).expect("encoded");
        let decoded = image::load_from_memory(&bytes)
            .expect("a real JPEG")
            .to_rgb8();
        let px = decoded.get_pixel(50, 50).0;
        assert!(
            px[0] > 240 && px[1] > 240 && px[2] > 240,
            "white, not black, got {px:?}"
        );
    }

    #[test]
    fn an_unencodable_format_answers_nothing_rather_than_the_wrong_bytes() {
        // PNG bytes labelled `image/webp` is a corrupt file the caller cannot
        // detect. Refusing is the only honest answer.
        let mut p = pixmap();
        let c = TinySkiaCanvas::new(&mut p);
        assert!(c.to_blob("image/webp", None).is_none());
        assert_eq!(c.to_data_url("image/webp", None), "data:,");
    }

    // ─── strokeText ─────────────────────────────────────────────────────

    fn painted(p: &Pixmap) -> usize {
        p.pixels().iter().filter(|px| px.alpha() > 0).count()
    }

    #[test]
    fn stroke_text_outlines_the_glyphs_instead_of_filling_them() {
        // **The distinguishing property is the HOLLOW MIDDLE, not the pixel
        // count.** A thin outline around a 120px letter still covers a good
        // fraction of what the solid one does — measured, 1292 against 2330 —
        // so a ratio test proves nothing either way. What only an outline does
        // is leave the inside of a stem empty, and that is what this looks at:
        // find a thick horizontal run in the filled glyph, then check its
        // centre is untouched in the stroked one. Filling in the stroke colour
        // — what this used to do — paints that centre.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();

        let mut filled = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut filled, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 120.0));
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_text("H", 20.0, 150.0);

        let mut stroked = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut stroked, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 120.0));
        c.set_stroke_color(Color::rgb(0, 0, 255));
        c.set_line_width(2.0);
        c.stroke_text("H", 20.0, 150.0);

        assert!(painted(&filled) > 0, "the filled glyph painted something");
        assert!(painted(&stroked) > 0, "so did the stroked one");

        // A point genuinely INSIDE the letter — opaque, and opaque for three
        // pixels in every direction, so it is clear of every edge the stroke
        // sits on. Taking the middle of the widest horizontal run is not
        // enough: the widest run is the crossbar, and its topmost row is
        // exactly where the stroke's own top edge lands.
        let deep_inside = |x: u32, y: u32| {
            (x.saturating_sub(3)..=x + 3).all(|nx| {
                (y.saturating_sub(3)..=y + 3).all(|ny| {
                    filled
                        .pixel(nx.min(199), ny.min(199))
                        .expect("in bounds")
                        .alpha()
                        > 200
                })
            })
        };
        let interior = (3..197u32)
            .flat_map(|y| (3..197u32).map(move |x| (x, y)))
            .find(|(x, y)| deep_inside(*x, *y))
            .expect("a 120px H has a stem at least 7px thick");

        let (cx, cy) = interior;
        assert_eq!(
            stroked.pixel(cx, cy).expect("in bounds").alpha(),
            0,
            "({cx}, {cy}) is 3px inside the letter, so an outline leaves it empty"
        );
    }

    #[test]
    fn stroke_text_uses_the_stroke_colour_and_line_width() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();

        let mut thin = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut thin, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 120.0));
        c.set_stroke_color(Color::rgb(0, 255, 0));
        c.set_line_width(1.0);
        c.stroke_text("H", 20.0, 150.0);

        let mut thick = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut thick, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 120.0));
        c.set_stroke_color(Color::rgb(0, 255, 0));
        c.set_line_width(6.0);
        c.stroke_text("H", 20.0, 150.0);

        assert!(
            thin.pixels().iter().any(|px| px.green() > 0),
            "painted in the STROKE colour, not the fill's"
        );
        assert!(
            painted(&thick) > painted(&thin),
            "a wider line covers more: {} vs {}",
            painted(&thick),
            painted(&thin)
        );
    }

    #[test]
    fn a_glyph_with_no_outline_strokes_nothing_rather_than_failing() {
        // A space has no outline. Neither does a font that only has bitmaps.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 40.0));
        c.set_stroke_color(Color::rgb(0, 0, 0));
        c.stroke_text("   ", 10.0, 50.0);
        assert_eq!(painted(&p), 0);
    }

    #[test]
    fn filled_and_stroked_text_land_in_the_same_place() {
        // Drawing both is how a page gets outlined text, so the two must agree
        // about the origin — which is why they share `text_origin`.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();

        let mut both = Pixmap::new(200, 200).expect("a pixmap");
        let mut c = TinySkiaCanvas::with_text(&mut both, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 120.0));
        c.set_text_align(crate::canvas::TextAlign::Center);
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_text("H", 100.0, 150.0);
        c.set_stroke_color(Color::rgb(0, 0, 255));
        c.set_line_width(2.0);
        c.stroke_text("H", 100.0, 150.0);

        // The stroke sits on the fill's edge, so every stroked pixel is at or
        // beside a filled one — the bounding boxes must overlap closely. Take
        // the horizontal extent of each and compare.
        let extent = |want_blue: bool| {
            let (mut lo, mut hi) = (usize::MAX, 0usize);
            for y in 0..200 {
                for x in 0..200 {
                    let px = both.pixel(x as u32, y as u32).expect("in bounds");
                    let hit = if want_blue {
                        px.blue() > 0
                    } else {
                        px.red() > 0
                    };
                    if hit {
                        lo = lo.min(x);
                        hi = hi.max(x);
                    }
                }
            }
            (lo, hi)
        };
        let (fill_lo, fill_hi) = extent(false);
        let (stroke_lo, stroke_hi) = extent(true);
        assert!(
            fill_lo != usize::MAX && stroke_lo != usize::MAX,
            "both drew"
        );
        assert!(
            (fill_lo as i32 - stroke_lo as i32).abs() <= 3
                && (fill_hi as i32 - stroke_hi as i32).abs() <= 3,
            "same glyph, same place: fill {fill_lo}..{fill_hi}, stroke {stroke_lo}..{stroke_hi}"
        );
    }

    // ─── CanvasTextDrawingStyles reaching the shaper ────────────────────

    #[test]
    fn letter_spacing_widens_the_measured_text() {
        // The proof that the attribute reaches SHAPING and not just the state:
        // `measureText` is computed from the shaped run, so if it moves, the
        // glyphs moved. A stored-only attribute would measure identically.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let tight = c.measure_text("iiiii").width;
        c.set_letter_spacing("10px");
        let loose = c.measure_text("iiiii").width;
        assert!(
            loose > tight + 30.0,
            "five gaps of 10px expected; {tight} -> {loose}"
        );
    }

    #[test]
    fn letter_spacing_normal_is_the_same_as_unset() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));
        let unset = c.measure_text("abc").width;
        c.set_letter_spacing("normal");
        assert_eq!(c.measure_text("abc").width, unset);
    }

    #[test]
    fn word_spacing_widens_only_the_gaps_between_words() {
        // cosmic-text has no word spacing, so this goes in as a per-span letter
        // spacing on each separator. The check that it worked is that a string
        // with two spaces grows twice as much as one with a single space, and
        // that a string with none does not grow at all.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let one_before = c.measure_text("a b").width;
        let two_before = c.measure_text("a b c").width;
        let none_before = c.measure_text("abc").width;

        c.set_word_spacing("20px");

        let one_after = c.measure_text("a b").width;
        let two_after = c.measure_text("a b c").width;
        let none_after = c.measure_text("abc").width;

        assert_eq!(none_after, none_before, "no separators, no change");
        let one_grew = one_after - one_before;
        let two_grew = two_after - two_before;
        assert!(one_grew > 15.0, "one gap widened, got {one_grew}");
        assert!(
            (two_grew - one_grew * 2.0).abs() < 2.0,
            "two gaps widened twice as much: {one_grew} vs {two_grew}"
        );
    }

    #[test]
    fn word_and_letter_spacing_add_up_on_a_separator() {
        // CSS applies both to a word separator, not one or the other.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let plain = c.measure_text("a b").width;
        c.set_letter_spacing("5px");
        let lettered = c.measure_text("a b").width;
        c.set_word_spacing("10px");
        let both = c.measure_text("a b").width;

        assert!(lettered > plain, "letter spacing alone widened it");
        assert!(
            both > lettered + 8.0,
            "and word spacing added on top: {lettered} -> {both}"
        );
    }

    #[test]
    fn letter_spacing_accepts_em_relative_to_the_font() {
        // `em` resolves against the font in effect, so the same string spaces
        // twice as far at twice the size.
        assert_eq!(parse_css_length("2em", 10.0), Some(20.0));
        assert_eq!(parse_css_length("2em", 20.0), Some(40.0));
        assert_eq!(parse_css_length("3px", 20.0), Some(3.0));
        assert_eq!(parse_css_length("normal", 20.0), None);
        assert_eq!(parse_css_length("", 20.0), None);
    }

    // ─── maxWidth ───────────────────────────────────────────────────────

    #[test]
    fn text_wider_than_max_width_is_condensed_to_fit() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));

        let natural = c.measure_text("wide text here").width;
        assert!(
            natural > 10.0,
            "the fixture has to overflow to test anything"
        );
        // The squeeze factor is what `fill_text_constrained` applies; asking for
        // it directly is asking whether the text would be condensed and by how
        // much, without needing to read it back out of the pixels.
        let factor = c
            .condense_factor("wide text here", 10.0)
            .expect("condensed");
        assert!((factor - 10.0 / natural).abs() < 0.001, "got {factor}");
    }

    #[test]
    fn text_that_already_fits_is_drawn_untouched() {
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 20.0));
        assert!(
            c.condense_factor("x", 10_000.0).is_none(),
            "no squeeze, and no transform pushed for one"
        );
    }

    // ─── drawFocusIfNeeded ──────────────────────────────────────────────

    #[test]
    fn the_focus_ring_is_drawn_only_when_the_element_has_focus() {
        let mut unfocused = pixmap();
        let mut c = TinySkiaCanvas::new(&mut unfocused);
        square(&mut c);
        c.draw_focus_if_needed(false);
        assert!(
            unfocused.pixels().iter().all(|px| px.alpha() == 0),
            "an unfocused element draws no ring"
        );

        let mut focused = pixmap();
        let mut c = TinySkiaCanvas::new(&mut focused);
        square(&mut c);
        c.draw_focus_if_needed(true);
        assert!(
            focused.pixels().iter().any(|px| px.alpha() > 0),
            "a focused one does"
        );
    }

    #[test]
    fn the_focus_ring_ignores_a_compositing_mode_that_would_hide_it() {
        // A ring the page can accidentally erase is an accessibility failure,
        // so it is stroked source-over whatever the context was set to.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        square(&mut c);
        c.set_global_composite_operation(crate::canvas::CompositeOp::DestinationOut);
        c.set_global_alpha(0.0);
        c.draw_focus_if_needed(true);
        assert!(p.pixels().iter().any(|px| px.alpha() > 0), "still visible");
    }

    // ─── Shadows ────────────────────────────────────────────────────────

    #[test]
    fn a_shadow_paints_offset_from_the_shape() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_shadow(&crate::canvas::Shadow {
            color: Color::rgb(0, 0, 0),
            blur: 0.0,
            offset_x: 10.0,
            offset_y: 10.0,
        });
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(10.0, 10.0, 20.0, 20.0);

        // The shape itself, and the shadow ten pixels down and right of it.
        assert_eq!(p.pixel(20, 20).expect("in bounds").red(), 255, "the shape");
        let cast = p.pixel(38, 38).expect("in bounds");
        assert!(cast.alpha() > 0, "the shadow landed");
        assert_eq!(
            cast.red(),
            0,
            "and it is the shadow colour, not the shape's"
        );
    }

    #[test]
    fn a_transparent_shadow_colour_paints_nothing() {
        // The spec's precondition: a shadow needs a non-transparent colour AND
        // a non-zero blur or offset. Offsets alone are inert.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_shadow(&crate::canvas::Shadow {
            color: Color::TRANSPARENT,
            blur: 0.0,
            offset_x: 10.0,
            offset_y: 10.0,
        });
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(10.0, 10.0, 20.0, 20.0);
        assert_eq!(p.pixel(38, 38).expect("in bounds").alpha(), 0);
    }

    #[test]
    fn the_shadow_offset_ignores_the_transform() {
        // The spec is explicit that shadowOffsetX/Y are not affected by the
        // CTM. Under a 2x scale the shape doubles and the offset does not.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.scale(2.0, 2.0);
        c.set_shadow(&crate::canvas::Shadow {
            color: Color::rgb(0, 0, 255),
            blur: 0.0,
            offset_x: 10.0,
            offset_y: 0.0,
        });
        c.set_fill_color(Color::rgb(255, 0, 0));
        // 5..15 in user space is 10..30 on screen; the shadow is 20..40.
        c.fill_rect(5.0, 5.0, 10.0, 10.0);
        assert!(
            p.pixel(35, 20).expect("in bounds").blue() > 0,
            "shadow at +10 device px, not +20"
        );
    }

    #[test]
    fn a_blurred_shadow_is_softer_than_the_shape() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_shadow(&crate::canvas::Shadow {
            color: Color::rgb(0, 0, 0),
            blur: 8.0,
            offset_x: 0.0,
            offset_y: 30.0,
        });
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(30.0, 10.0, 20.0, 20.0);
        // Just outside where a hard-edged shadow would stop: only a blur puts
        // anything here.
        assert!(
            p.pixel(26, 50).expect("in bounds").alpha() > 0,
            "the blur spread past the shape's edge"
        );
    }

    #[test]
    fn text_casts_a_shadow_too() {
        // Text goes through the same drawing model as shapes — the reason
        // `fill_text` moves its font resources out of `self` rather than
        // borrowing them.
        let mut font_system = FontSystem::new();
        let mut swash_cache = SwashCache::new();
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::with_text(&mut p, &mut font_system, &mut swash_cache);
        c.set_font(&Font::new("sans-serif", 30.0));
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.set_shadow(&crate::canvas::Shadow {
            color: Color::rgb(0, 0, 255),
            blur: 2.0,
            offset_x: 4.0,
            offset_y: 4.0,
        });
        c.fill_text("H", 10.0, 50.0);
        assert!(
            p.pixels().iter().any(|px| px.blue() > 0 && px.red() == 0),
            "some pixel is shadow-only"
        );
    }

    // ─── filter ─────────────────────────────────────────────────────────

    #[test]
    fn a_grayscale_filter_desaturates_what_is_drawn() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_filter("grayscale(1)");
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(10.0, 10.0, 20.0, 20.0);
        let px = p.pixel(20, 20).expect("in bounds");
        assert!(px.red() < 200, "no longer full red, got {}", px.red());
        assert_eq!(px.red(), px.green(), "grey has equal channels");
        assert_eq!(px.green(), px.blue());
    }

    #[test]
    fn a_blur_filter_softens_the_edge() {
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_filter("blur(4px)");
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(30.0, 30.0, 20.0, 20.0);
        assert!(
            p.pixel(26, 40).expect("in bounds").alpha() > 0,
            "colour spread outside the rect"
        );
    }

    #[test]
    fn filter_none_takes_the_fast_path_and_changes_nothing() {
        // `none` is the initial value, so this is also the guard that the
        // drawing model's layer is not paid for on every ordinary fill.
        let mut plain = pixmap();
        let mut c = TinySkiaCanvas::new(&mut plain);
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(10.0, 10.0, 20.0, 20.0);

        let mut explicit = pixmap();
        let mut c = TinySkiaCanvas::new(&mut explicit);
        c.set_filter("none");
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(10.0, 10.0, 20.0, 20.0);

        assert_eq!(plain.data(), explicit.data());
    }

    // ─── drawImage and the clip ─────────────────────────────────────────

    #[test]
    fn a_clip_region_confines_an_image_too() {
        // `drawImage` used to pass `None` for the clip mask, so it was the one
        // operation a `clip()` did not confine.
        let image = crate::canvas::Image::from_rgba(20, 20, vec![255u8; 20 * 20 * 4]);
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.begin_path();
        c.rect(0.0, 0.0, 10.0, 10.0);
        c.clip();
        c.draw_image(&image, 0.0, 0.0, 20.0, 20.0);

        assert!(p.pixel(5, 5).expect("in bounds").alpha() > 0, "inside");
        assert_eq!(
            p.pixel(15, 15).expect("in bounds").alpha(),
            0,
            "outside the clip, and it used to be painted"
        );
    }

    // ─── globalCompositeOperation ───────────────────────────────────────

    #[test]
    fn destination_out_knocks_a_hole_instead_of_painting() {
        // The plainest proof that the attribute reaches the pixels: under
        // `destination-out` a fill ERASES. A canvas that ignored the mode would
        // leave an opaque square here.
        let mut p = pixmap();
        let mut c = TinySkiaCanvas::new(&mut p);
        c.set_fill_color(Color::rgb(0, 0, 255));
        c.fill_rect(0.0, 0.0, 100.0, 100.0);
        c.set_global_composite_operation(crate::canvas::CompositeOp::DestinationOut);
        c.set_fill_color(Color::rgb(255, 255, 255));
        c.fill_rect(20.0, 20.0, 20.0, 20.0);

        let inside = p.pixel(30, 30).expect("in the hole");
        let outside = p.pixel(80, 80).expect("still painted");
        assert_eq!(inside.alpha(), 0, "erased");
        assert_eq!(outside.alpha(), 255, "untouched");
    }
}

/// A CSS `<color>`, through the ENGINE's parser.
///
/// The one place canvas colours and stylesheet colours could have diverged, and
/// they do not: `fillStyle = "rebeccapurple"` and `color: rebeccapurple` go
/// through the same code. `None` means unparseable, and §4.12.5 says an
/// unparseable assignment leaves the attribute alone.
pub(super) fn parse_canvas_color(css: &str) -> Option<super::Color> {
    crate::css::parse_color(css).map(|c| super::Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    })
}

/// A CSS `font` shorthand, through the engine's parser.
///
/// Runs the real shorthand parser over a default `ComputedStyle` and reads the
/// font properties back off it, rather than re-implementing the grammar — the
/// shorthand has optional style, variant, weight and stretch before the size,
/// an optional `/line-height` after it, and a font-family list at the end, and
/// a second implementation of that would be wrong in a different way from the
/// first.
pub(super) fn parse_canvas_font(css: &str) -> Option<super::Font> {
    let value = css.trim();
    if value.is_empty() {
        return None;
    }
    let mut style = crate::types::ComputedStyle::default();
    let before = style.font_size.clone();
    crate::css::apply_font_shorthand(&mut style, value);
    // The shorthand REQUIRES a size and a family; if the parser left the size
    // untouched the value was not a font shorthand at all.
    if style.font_family.is_empty() || style.font_size == before {
        return None;
    }
    Some(super::Font {
        family: style.font_family.clone(),
        // 16px is the initial `font-size`, which is what a relative unit in
        // the shorthand resolves against when there is no element to inherit
        // from — a canvas context has no parent box.
        size: style.font_size.resolve(16.0, 16.0, 16.0),
        weight: match style.font_weight {
            crate::types::FontWeight::Bold => super::FontWeight::Bold,
            _ => super::FontWeight::Normal,
        },
        style: match style.font_style {
            crate::types::FontStyle::Italic => super::FontStyle::Italic,
            _ => super::FontStyle::Normal,
        },
    })
}
