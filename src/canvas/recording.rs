//! `RecordingCanvas` — a `Canvas` impl that captures every call as data.
//!
//! Two reasons recording is the persistent state of a canvas:
//!
//! 1. **Tests.** Asserting on `recording.commands` is exact-shape and
//!    deterministic — no flaky pixel comparisons.
//!
//! 2. **Persistent draws.** Most retained-mode drawing models (HTML5
//!    canvas, immediate-mode UIs that need redraw on resize, framework
//!    `Graphics` objects, …) want "drawings persist between paint
//!    cycles". Recording is the natural shape for this — the form's
//!    render loop replays the recording onto a fresh `TinySkiaCanvas`
//!    each frame.
//!
//! `replay(&mut other)` walks the captured commands and dispatches each
//! one to another `Canvas` impl. The dispatch is mechanical — one
//! variant per trait method, one match arm per variant.

use super::{Canvas, Color, Font, Image, LineCap, LineJoin};

/// One captured drawing primitive.
///
/// Variants mirror the [`Canvas`] trait method-for-method (one variant
/// per call). Adding a method to the trait is matched by adding one
/// variant here and one match arm in [`RecordingCanvas::replay`].
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    // Paint state
    SetFillColor(Color),
    SetStrokeColor(Color),
    SetLineWidth(f32),
    SetLineCap(LineCap),
    SetLineJoin(LineJoin),
    SetMiterLimit(f32),
    SetGlobalAlpha(f32),
    SetImageSmoothing(bool),
    SetTextAlign(super::TextAlign),
    SetTextBaseline(super::TextBaseline),
    SetFont(Font),
    SetLineDash(Vec<f32>),
    SetLineDashOffset(f32),
    /// `fillStyle` / `strokeStyle` when they hold a gradient or a pattern.
    ///
    /// Recorded SEPARATELY from `SetFillColor` rather than replacing it: a
    /// colour is by far the common case, and collapsing the two would make
    /// every existing recording assertion carry a `Paint::Color(..)` wrapper
    /// for no gain. `set_fill_color` still records `SetFillColor`.
    SetFillPaint(super::Paint),
    SetStrokePaint(super::Paint),
    SetShadow(super::Shadow),

    // Path building
    BeginPath,
    ClosePath,
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadraticCurveTo {
        cx: f32,
        cy: f32,
        x: f32,
        y: f32,
    },
    BezierCurveTo {
        cx1: f32,
        cy1: f32,
        cx2: f32,
        cy2: f32,
        x: f32,
        y: f32,
    },
    Arc {
        x: f32,
        y: f32,
        r: f32,
        start: f32,
        end: f32,
        ccw: bool,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    Ellipse {
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
    },
    /// The spec's full `ellipse()` — rotation and a start/end angle pair.
    ///
    /// Its own variant rather than a widened `Ellipse`, because the default
    /// `ellipse_arc` composes save/rotate/scale/arc/restore: recorded through
    /// that path an elliptical wedge would arrive as five commands and the
    /// recording would no longer say what was asked for. A recording is a
    /// transcript, so the call is the unit.
    EllipseArc {
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
        rotation: f32,
        start: f32,
        end: f32,
        ccw: bool,
    },

    // Drawing
    Fill,
    FillWithRule(super::FillRule),
    Stroke,
    FillRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    StrokeRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    ClearRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    FillText {
        text: String,
        x: f32,
        y: f32,
    },
    StrokeText {
        text: String,
        x: f32,
        y: f32,
    },
    /// `putImageData` — a RAW write. Recorded as its own command rather than
    /// a `DrawImage` with `w`/`h` equal to the image's, because the two differ
    /// in what they honour, not in their arguments: this one ignores the
    /// transform, the clip, `globalAlpha` and the blend mode. Replayed as a
    /// `DrawImage` it would pick all four up from whatever state the replay
    /// had reached.
    PutImageData {
        image: Image,
        dx: f32,
        dy: f32,
    },
    DrawImage {
        image: Image,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// The nine-argument `drawImage` — the SOURCE rectangle is the point, and
    /// cropping at record time would throw away what the call said.
    DrawImageRect {
        image: Image,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
    },
    Clip,
    ClipWithRule(super::FillRule),
    ResetClip,

    // State stack
    Save,
    Restore,

    // Transforms
    Translate(f32, f32),
    Rotate(f32),
    Scale(f32, f32),
    Transform {
        m11: f32,
        m12: f32,
        m21: f32,
        m22: f32,
        dx: f32,
        dy: f32,
    },
    ResetTransform,
    /// `setTransform` — REPLACE rather than compose. Recorded as its own
    /// command instead of `ResetTransform` + `Transform` so a reader of the
    /// recording sees the call that was made.
    SetTransform(super::Matrix),

    // Context state
    /// `reset()`. Clears the bitmap as well as the state, which is why replay
    /// forwards it rather than the recording simply dropping its commands.
    Reset,
    SetGlobalCompositeOperation(super::CompositeOp),
    SetImageSmoothingQuality(super::SmoothingQuality),
    SetFilter(String),

    // Text drawing styles
    SetDirection(super::Direction),
    SetLang(String),
    SetLetterSpacing(String),
    SetWordSpacing(String),
    SetFontKerning(super::FontKerning),
    SetFontStretch(super::FontStretch),
    SetFontVariantCaps(super::FontVariantCaps),
    SetTextRendering(super::TextRendering),

    /// `drawFocusIfNeeded`, carrying the answer the caller resolved from the
    /// element: whether to draw the ring.
    DrawFocusIfNeeded(bool),
}

/// A `Canvas` impl that captures every call as a [`DrawCmd`].
///
/// `RecordingCanvas` is what every drawing surface holds as its
/// authoritative state. The form's render loop calls
/// [`RecordingCanvas::replay`] each frame to paint the captured commands
/// onto whatever live backend is active.
#[derive(Clone, Debug, Default)]
pub struct RecordingCanvas {
    pub commands: Vec<DrawCmd>,
    /// The subset of paint state the context can be ASKED for.
    ///
    /// A recording is a list of calls, which is enough to replay and enough to
    /// assert on — but `getTransform()` and `getLineDash()` are questions, and
    /// answering them by scanning the command list backwards would mean
    /// re-deriving the transform stack on every call. Tracked forward instead,
    /// alongside the recording rather than instead of it.
    state: RecordedState,
    stack: Vec<RecordedState>,
}

/// The queryable half of the recording's paint state.
///
/// **The same `PaintState` the rasteriser keeps.** Every attribute in
/// §4.12.5 has to read back, and a recording is a `Canvas` like any other — so
/// it answers `font`, `fillStyle`, `getTransform` and the rest from real state
/// rather than from a backwards scan of the commands. A scan cannot be right
/// anyway: `save`/`restore` means the last recorded `setFont` may be one that
/// has since been popped.
///
/// Held rather than re-declared so the two impls cannot drift into disagreeing
/// about what the initial value of an attribute is.
#[derive(Clone, Debug)]
struct RecordedState {
    dash: Vec<f32>,
    transform: super::Matrix,
    attrs: super::tinyskia::PaintState,
}

impl Default for RecordedState {
    fn default() -> Self {
        RecordedState {
            dash: Vec::new(),
            transform: super::Matrix::IDENTITY,
            attrs: super::tinyskia::PaintState::default(),
        }
    }
}

impl RecordingCanvas {
    /// Construct an empty recording.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replay every captured command onto another canvas.
    ///
    /// The dispatch is mechanical — one match arm per [`DrawCmd`]
    /// variant calling the corresponding [`Canvas`] trait method on
    /// `target`. After replay, the recording is unchanged (callers can
    /// `clear()` if they want one-shot replay).
    pub fn replay(&self, target: &mut dyn Canvas) {
        for cmd in &self.commands {
            match cmd {
                // Paint state
                DrawCmd::SetFillColor(c) => target.set_fill_color(*c),
                DrawCmd::SetStrokeColor(c) => target.set_stroke_color(*c),
                DrawCmd::SetLineWidth(w) => target.set_line_width(*w),
                DrawCmd::SetLineCap(cap) => target.set_line_cap(*cap),
                DrawCmd::SetLineJoin(j) => target.set_line_join(*j),
                DrawCmd::SetMiterLimit(l) => target.set_miter_limit(*l),
                DrawCmd::SetGlobalAlpha(a) => target.set_global_alpha(*a),
                DrawCmd::SetImageSmoothing(on) => target.set_image_smoothing(*on),
                DrawCmd::SetTextAlign(a) => target.set_text_align(*a),
                DrawCmd::SetTextBaseline(b) => target.set_text_baseline(*b),
                DrawCmd::SetFont(f) => target.set_font(f),
                DrawCmd::SetLineDash(intervals) => target.set_line_dash(intervals),
                DrawCmd::SetLineDashOffset(o) => target.set_line_dash_offset(*o),
                DrawCmd::SetFillPaint(p) => target.set_fill_paint(p),
                DrawCmd::SetStrokePaint(p) => target.set_stroke_paint(p),
                DrawCmd::SetShadow(s) => target.set_shadow(s),

                // Path building
                DrawCmd::BeginPath => target.begin_path(),
                DrawCmd::ClosePath => target.close_path(),
                DrawCmd::MoveTo(x, y) => target.move_to(*x, *y),
                DrawCmd::LineTo(x, y) => target.line_to(*x, *y),
                DrawCmd::QuadraticCurveTo { cx, cy, x, y } => {
                    target.quadratic_curve_to(*cx, *cy, *x, *y)
                }
                DrawCmd::BezierCurveTo {
                    cx1,
                    cy1,
                    cx2,
                    cy2,
                    x,
                    y,
                } => target.bezier_curve_to(*cx1, *cy1, *cx2, *cy2, *x, *y),
                DrawCmd::Arc {
                    x,
                    y,
                    r,
                    start,
                    end,
                    ccw,
                } => target.arc(*x, *y, *r, *start, *end, *ccw),
                DrawCmd::Rect { x, y, w, h } => target.rect(*x, *y, *w, *h),
                DrawCmd::Ellipse { x, y, rx, ry } => target.ellipse(*x, *y, *rx, *ry),
                DrawCmd::EllipseArc {
                    x,
                    y,
                    rx,
                    ry,
                    rotation,
                    start,
                    end,
                    ccw,
                } => target.ellipse_arc(*x, *y, *rx, *ry, *rotation, *start, *end, *ccw),

                // Drawing
                DrawCmd::Fill => target.fill(),
                DrawCmd::FillWithRule(rule) => target.fill_with_rule(*rule),
                DrawCmd::Stroke => target.stroke(),
                DrawCmd::FillRect { x, y, w, h } => target.fill_rect(*x, *y, *w, *h),
                DrawCmd::StrokeRect { x, y, w, h } => target.stroke_rect(*x, *y, *w, *h),
                DrawCmd::ClearRect { x, y, w, h } => target.clear_rect(*x, *y, *w, *h),
                DrawCmd::FillText { text, x, y } => target.fill_text(text, *x, *y),
                DrawCmd::StrokeText { text, x, y } => target.stroke_text(text, *x, *y),
                DrawCmd::PutImageData { image, dx, dy } => target.put_image_data(image, *dx, *dy),
                DrawCmd::DrawImage { image, x, y, w, h } => {
                    target.draw_image(image, *x, *y, *w, *h)
                }
                DrawCmd::DrawImageRect {
                    image,
                    sx,
                    sy,
                    sw,
                    sh,
                    dx,
                    dy,
                    dw,
                    dh,
                } => target.draw_image_rect(image, *sx, *sy, *sw, *sh, *dx, *dy, *dw, *dh),
                DrawCmd::Clip => target.clip(),
                DrawCmd::ClipWithRule(rule) => target.clip_with_rule(*rule),
                DrawCmd::ResetClip => target.reset_clip(),

                // State stack
                DrawCmd::Save => target.save(),
                DrawCmd::Restore => target.restore(),

                // Transforms
                DrawCmd::Translate(x, y) => target.translate(*x, *y),
                DrawCmd::Rotate(rad) => target.rotate(*rad),
                DrawCmd::Scale(sx, sy) => target.scale(*sx, *sy),
                DrawCmd::Transform {
                    m11,
                    m12,
                    m21,
                    m22,
                    dx,
                    dy,
                } => target.transform(*m11, *m12, *m21, *m22, *dx, *dy),
                DrawCmd::ResetTransform => target.reset_transform(),
                DrawCmd::SetTransform(m) => target.set_transform(*m),

                // Context state
                DrawCmd::Reset => target.reset(),
                DrawCmd::SetGlobalCompositeOperation(op) => {
                    target.set_global_composite_operation(*op)
                }
                DrawCmd::SetImageSmoothingQuality(q) => target.set_image_smoothing_quality(*q),
                DrawCmd::SetFilter(f) => target.set_filter(f),

                // Text drawing styles
                DrawCmd::SetDirection(d) => target.set_direction(*d),
                DrawCmd::SetLang(l) => target.set_lang(l),
                DrawCmd::SetLetterSpacing(s) => target.set_letter_spacing(s),
                DrawCmd::SetWordSpacing(s) => target.set_word_spacing(s),
                DrawCmd::SetFontKerning(k) => target.set_font_kerning(*k),
                DrawCmd::SetFontStretch(s) => target.set_font_stretch(*s),
                DrawCmd::SetFontVariantCaps(c) => target.set_font_variant_caps(*c),
                DrawCmd::SetTextRendering(r) => target.set_text_rendering(*r),

                DrawCmd::DrawFocusIfNeeded(focused) => target.draw_focus_if_needed(*focused),
            }
        }
    }

    /// Drop every captured command. Used by callers that want one-shot
    /// replay (replay → clear → start fresh).
    /// Debug-only view of the recorded commands.
    pub fn commands_for_debug(&self) -> &[DrawCmd] {
        &self.commands
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Number of captured commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// True if no commands have been captured yet.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Where the path being built currently ends.
    ///
    /// **Derived from the commands, not tracked beside them.** A `last_point`
    /// field would have to be updated by every path method and would be wrong
    /// the moment one forgot — the same two-copies problem the rest of this
    /// engine has been shedding. Scanning backwards cannot drift, and `arcTo`
    /// is the only caller.
    /// The font in effect — what `measureText` and `fillText` would use.
    ///
    /// Derived from the commands for the same reason [`Self::last_point`] is,
    /// with one difference that matters: `save`/`restore` are part of the
    /// answer. The font is paint STATE, so a `restore()` puts back whatever
    /// was in effect at its `save()`, and a backwards scan for the last
    /// `SetFont` would happily return one that has since been popped. So this
    /// walks FORWARD over a stack, which is what a canvas does.
    pub fn current_font(&self) -> Font {
        let mut font = Font::default();
        let mut saved: Vec<Font> = Vec::new();
        for command in &self.commands {
            match command {
                DrawCmd::SetFont(f) => font = f.clone(),
                DrawCmd::Save => saved.push(font.clone()),
                // An unbalanced `restore()` is a no-op, per spec: it pops
                // nothing rather than trapping.
                DrawCmd::Restore => {
                    if let Some(previous) = saved.pop() {
                        font = previous;
                    }
                }
                _ => {}
            }
        }
        font
    }

    fn last_point(&self) -> Option<(f32, f32)> {
        for command in self.commands.iter().rev() {
            match command {
                DrawCmd::MoveTo(x, y) | DrawCmd::LineTo(x, y) => return Some((*x, *y)),
                DrawCmd::QuadraticCurveTo { x, y, .. } | DrawCmd::BezierCurveTo { x, y, .. } => {
                    return Some((*x, *y));
                }
                // An arc ends where its sweep does.
                DrawCmd::Arc { x, y, r, end, .. } => {
                    return Some((x + r * end.cos(), y + r * end.sin()));
                }
                // A rectangle is a closed subpath: it ends where it began.
                DrawCmd::Rect { x, y, .. } => return Some((*x, *y)),
                // `beginPath` discards everything before it, so the search
                // stops rather than reaching into a path that no longer exists.
                DrawCmd::BeginPath => return None,
                _ => {}
            }
        }
        None
    }
}

impl Canvas for RecordingCanvas {
    /// The recorder tracks the same state the rasteriser does, so a recording
    /// answers `font`, `fillStyle` and the rest exactly as a live canvas
    /// would — including across `save`/`restore`, which a backwards scan of the
    /// recorded commands could not get right.
    fn drawing_state(&self) -> &super::DrawingState {
        &self.state.attrs
    }

    fn set_fill_style_css(&mut self, css: &str) {
        if let Some(c) = super::tinyskia::parse_canvas_color(css) {
            self.state.attrs.fill = super::Paint::Color(c);
            self.commands.push(DrawCmd::SetFillColor(c));
        }
    }

    fn set_stroke_style_css(&mut self, css: &str) {
        if let Some(c) = super::tinyskia::parse_canvas_color(css) {
            self.state.attrs.stroke = super::Paint::Color(c);
            self.commands.push(DrawCmd::SetStrokeColor(c));
        }
    }

    fn set_font_css(&mut self, css: &str) {
        if let Some(f) = super::tinyskia::parse_canvas_font(css) {
            self.state.attrs.font = f.clone();
            self.commands.push(DrawCmd::SetFont(f));
        }
    }

    fn set_shadow_color_css(&mut self, css: &str) {
        if let Some(c) = super::tinyskia::parse_canvas_color(css) {
            self.state.attrs.shadow.color = c;
            let shadow = self.state.attrs.shadow.clone();
            self.commands.push(DrawCmd::SetShadow(shadow));
        }
    }

    // Paint state
    fn set_fill_color(&mut self, color: Color) {
        self.state.attrs.fill = super::Paint::Color(color);
        self.commands.push(DrawCmd::SetFillColor(color));
    }
    fn set_stroke_color(&mut self, color: Color) {
        self.state.attrs.stroke = super::Paint::Color(color);
        self.commands.push(DrawCmd::SetStrokeColor(color));
    }
    fn set_line_width(&mut self, width: f32) {
        self.state.attrs.line_width = width;
        self.commands.push(DrawCmd::SetLineWidth(width));
    }
    fn set_line_cap(&mut self, cap: LineCap) {
        self.state.attrs.line_cap = cap;
        self.commands.push(DrawCmd::SetLineCap(cap));
    }
    fn set_line_join(&mut self, join: LineJoin) {
        self.state.attrs.line_join = join;
        self.commands.push(DrawCmd::SetLineJoin(join));
    }
    fn set_miter_limit(&mut self, limit: f32) {
        self.state.attrs.miter_limit = limit;
        self.commands.push(DrawCmd::SetMiterLimit(limit));
    }
    fn set_global_alpha(&mut self, alpha: f32) {
        self.state.attrs.global_alpha = alpha;
        self.commands.push(DrawCmd::SetGlobalAlpha(alpha));
    }

    fn set_image_smoothing(&mut self, enabled: bool) {
        self.state.attrs.image_smoothing = enabled;
        self.commands.push(DrawCmd::SetImageSmoothing(enabled));
    }
    fn set_text_align(&mut self, align: super::TextAlign) {
        self.state.attrs.text_align = align;
        self.commands.push(DrawCmd::SetTextAlign(align));
    }
    fn set_text_baseline(&mut self, baseline: super::TextBaseline) {
        self.state.attrs.text_baseline = baseline;
        self.commands.push(DrawCmd::SetTextBaseline(baseline));
    }
    fn set_font(&mut self, font: &Font) {
        self.state.attrs.font = font.clone();
        self.commands.push(DrawCmd::SetFont(font.clone()));
    }
    fn set_line_dash(&mut self, intervals: &[f32]) {
        // Normalised before it is recorded, not after: `getLineDash` has to
        // read back the doubled list, and a rejected list must leave the
        // previous one in force rather than record a call that did nothing.
        let Some(dash) = super::normalize_dash(intervals) else {
            return;
        };
        self.commands.push(DrawCmd::SetLineDash(dash.clone()));
        self.state.dash = dash;
    }
    fn set_line_dash_offset(&mut self, offset: f32) {
        self.state.attrs.dash_offset = offset;
        self.commands.push(DrawCmd::SetLineDashOffset(offset));
    }
    fn set_fill_paint(&mut self, paint: &super::Paint) {
        self.state.attrs.fill = paint.clone();
        self.commands.push(DrawCmd::SetFillPaint(paint.clone()));
    }
    fn set_stroke_paint(&mut self, paint: &super::Paint) {
        self.state.attrs.stroke = paint.clone();
        self.commands.push(DrawCmd::SetStrokePaint(paint.clone()));
    }
    fn set_shadow(&mut self, shadow: &super::Shadow) {
        self.state.attrs.shadow = shadow.clone();
        self.commands.push(DrawCmd::SetShadow(*shadow));
    }

    // Path building
    fn begin_path(&mut self) {
        self.commands.push(DrawCmd::BeginPath);
    }
    fn close_path(&mut self) {
        self.commands.push(DrawCmd::ClosePath);
    }
    fn move_to(&mut self, x: f32, y: f32) {
        self.commands.push(DrawCmd::MoveTo(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.commands.push(DrawCmd::LineTo(x, y));
    }
    fn quadratic_curve_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.commands
            .push(DrawCmd::QuadraticCurveTo { cx, cy, x, y });
    }
    fn bezier_curve_to(&mut self, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32) {
        self.commands.push(DrawCmd::BezierCurveTo {
            cx1,
            cy1,
            cx2,
            cy2,
            x,
            y,
        });
    }
    fn arc(&mut self, x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool) {
        self.commands.push(DrawCmd::Arc {
            x,
            y,
            r,
            start,
            end,
            ccw,
        });
    }
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.commands.push(DrawCmd::Rect { x, y, w, h });
    }
    fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        self.commands.push(DrawCmd::Ellipse { x, y, rx, ry });
    }
    fn ellipse_arc(
        &mut self,
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
        rotation: f32,
        start: f32,
        end: f32,
        ccw: bool,
    ) {
        self.commands.push(DrawCmd::EllipseArc {
            x,
            y,
            rx,
            ry,
            rotation,
            start,
            end,
            ccw,
        });
    }

    // Drawing
    fn fill(&mut self) {
        self.commands.push(DrawCmd::Fill);
    }
    fn fill_with_rule(&mut self, rule: super::FillRule) {
        self.commands.push(DrawCmd::FillWithRule(rule));
    }
    fn stroke(&mut self) {
        self.commands.push(DrawCmd::Stroke);
    }
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.commands.push(DrawCmd::FillRect { x, y, w, h });
    }
    fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.commands.push(DrawCmd::StrokeRect { x, y, w, h });
    }
    fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.commands.push(DrawCmd::ClearRect { x, y, w, h });
    }
    fn fill_text(&mut self, text: &str, x: f32, y: f32) {
        self.commands.push(DrawCmd::FillText {
            text: text.to_string(),
            x,
            y,
        });
    }
    fn stroke_text(&mut self, text: &str, x: f32, y: f32) {
        self.commands.push(DrawCmd::StrokeText {
            text: text.to_string(),
            x,
            y,
        });
    }
    fn draw_image(&mut self, img: &Image, x: f32, y: f32, w: f32, h: f32) {
        self.commands.push(DrawCmd::DrawImage {
            image: img.clone(),
            x,
            y,
            w,
            h,
        });
    }
    fn put_image_data(&mut self, img: &Image, dx: f32, dy: f32) {
        self.commands.push(DrawCmd::PutImageData {
            image: img.clone(),
            dx,
            dy,
        });
    }
    #[allow(clippy::too_many_arguments)]
    fn draw_image_rect(
        &mut self,
        img: &Image,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        dx: f32,
        dy: f32,
        dw: f32,
        dh: f32,
    ) {
        self.commands.push(DrawCmd::DrawImageRect {
            image: img.clone(),
            sx,
            sy,
            sw,
            sh,
            dx,
            dy,
            dw,
            dh,
        });
    }
    fn clip(&mut self) {
        self.commands.push(DrawCmd::Clip);
    }
    fn clip_with_rule(&mut self, rule: super::FillRule) {
        self.commands.push(DrawCmd::ClipWithRule(rule));
    }
    fn reset_clip(&mut self) {
        self.commands.push(DrawCmd::ResetClip);
    }

    // State stack
    fn save(&mut self) {
        self.commands.push(DrawCmd::Save);
        self.stack.push(self.state.clone());
    }
    fn restore(&mut self) {
        self.commands.push(DrawCmd::Restore);
        // An unmatched `restore` is a no-op per spec, not an error — the stack
        // simply has nothing to pop.
        if let Some(previous) = self.stack.pop() {
            self.state = previous;
        }
    }

    // Transforms
    fn translate(&mut self, x: f32, y: f32) {
        self.commands.push(DrawCmd::Translate(x, y));
        self.state.transform = self
            .state
            .transform
            .multiply(super::Matrix::new(1.0, 0.0, 0.0, 1.0, x, y));
    }
    fn rotate(&mut self, rad: f32) {
        self.commands.push(DrawCmd::Rotate(rad));
        let (sin, cos) = rad.sin_cos();
        self.state.transform = self
            .state
            .transform
            .multiply(super::Matrix::new(cos, sin, -sin, cos, 0.0, 0.0));
    }
    fn scale(&mut self, sx: f32, sy: f32) {
        self.commands.push(DrawCmd::Scale(sx, sy));
        self.state.transform = self
            .state
            .transform
            .multiply(super::Matrix::new(sx, 0.0, 0.0, sy, 0.0, 0.0));
    }
    fn transform(&mut self, m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) {
        self.commands.push(DrawCmd::Transform {
            m11,
            m12,
            m21,
            m22,
            dx,
            dy,
        });
        self.state.transform = self
            .state
            .transform
            .multiply(super::Matrix::new(m11, m12, m21, m22, dx, dy));
    }
    fn reset_transform(&mut self) {
        self.commands.push(DrawCmd::ResetTransform);
        self.state.transform = super::Matrix::IDENTITY;
    }
    fn set_transform(&mut self, m: super::Matrix) {
        self.commands.push(DrawCmd::SetTransform(m));
        self.state.transform = m;
    }
    fn get_transform(&self) -> super::Matrix {
        self.state.transform
    }

    fn current_point(&self) -> Option<(f32, f32)> {
        self.last_point()
    }

    // ─── Context state ──────────────────────────────────────────────────

    /// A recording's `reset` DROPS what was recorded as well as recording the
    /// call.
    ///
    /// Both halves are needed and for different readers. Dropping is what makes
    /// the recording match the canvas it describes: the spec clears the bitmap,
    /// so commands issued before `reset` no longer contribute anything and
    /// keeping them would make a replay paint pixels a real context would not.
    /// Recording it is for the target of a replay, which may have content of
    /// its own that this must clear.
    fn reset(&mut self) {
        self.commands.clear();
        self.commands.push(DrawCmd::Reset);
        self.state = RecordedState::default();
        self.stack.clear();
    }

    fn set_global_composite_operation(&mut self, op: super::CompositeOp) {
        self.state.attrs.composite = op;
        self.commands.push(DrawCmd::SetGlobalCompositeOperation(op));
    }
    fn set_image_smoothing_quality(&mut self, quality: super::SmoothingQuality) {
        self.state.attrs.smoothing_quality = quality;
        self.commands
            .push(DrawCmd::SetImageSmoothingQuality(quality));
    }
    fn set_filter(&mut self, filter: &str) {
        self.state.attrs.filter = filter.to_string();
        self.commands.push(DrawCmd::SetFilter(filter.to_string()));
    }

    fn get_line_dash(&self) -> Vec<f32> {
        self.state.dash.clone()
    }

    // ─── Text drawing styles ────────────────────────────────────────────

    fn set_direction(&mut self, direction: super::Direction) {
        self.state.attrs.direction = direction;
        self.commands.push(DrawCmd::SetDirection(direction));
    }
    fn set_lang(&mut self, lang: &str) {
        self.state.attrs.lang = lang.to_string();
        self.commands.push(DrawCmd::SetLang(lang.to_string()));
    }
    fn set_letter_spacing(&mut self, spacing: &str) {
        // Stored as the parsed length, because that is what the getter has to
        // serialize back and what the rasteriser needs; `"2px"` and `"2"` are
        // then the same state, which they are.
        self.state.attrs.letter_spacing = spacing.to_string();
        self.commands
            .push(DrawCmd::SetLetterSpacing(spacing.to_string()));
    }
    fn set_word_spacing(&mut self, spacing: &str) {
        self.state.attrs.word_spacing = spacing.to_string();
        self.commands
            .push(DrawCmd::SetWordSpacing(spacing.to_string()));
    }
    fn set_font_kerning(&mut self, kerning: super::FontKerning) {
        self.state.attrs.font_kerning = kerning;
        self.commands.push(DrawCmd::SetFontKerning(kerning));
    }
    fn set_font_stretch(&mut self, stretch: super::FontStretch) {
        self.state.attrs.font_stretch = stretch;
        self.commands.push(DrawCmd::SetFontStretch(stretch));
    }
    fn set_font_variant_caps(&mut self, caps: super::FontVariantCaps) {
        self.state.attrs.font_variant_caps = caps;
        self.commands.push(DrawCmd::SetFontVariantCaps(caps));
    }
    fn set_text_rendering(&mut self, rendering: super::TextRendering) {
        self.state.attrs.text_rendering = rendering;
        self.commands.push(DrawCmd::SetTextRendering(rendering));
    }

    // `round_rect_radii` is deliberately NOT overridden here.
    //
    // Recording it as one command would keep the caller's vocabulary, but it
    // would also move the spec's corner-clamping rule — a radius larger than
    // the side it sits on scales every radius down together — out of the
    // recording and into whatever replays it. Taking the trait's default keeps
    // that rule in one place and keeps it under test: the recorded arcs ARE the
    // clamped radii, so `round_rect_clamps_a_radius_that_would_overlap` below
    // can read them straight out of the command list.

    fn draw_focus_if_needed(&mut self, focused: bool) {
        self.commands.push(DrawCmd::DrawFocusIfNeeded(focused));
    }

    // `is_point_in_path` and `is_point_in_stroke` are NOT implemented here, so
    // a recording answers `false` to both.
    //
    // Said out loud because it is a real limit rather than an oversight: the
    // commands do describe the geometry, but answering would mean a second
    // flattening-and-winding implementation beside `TinySkiaCanvas`'s, and two
    // hit tests that can disagree is worse than one that is clearly absent.
    // A caller that needs hit testing wants the canvas that has the pixels.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the recorded path ends, for the arc tests below.
    fn end_of(c: &RecordingCanvas) -> Option<(f32, f32)> {
        c.last_point()
    }

    /// A gradient must PAINT, not merely record. This is the whole point of
    /// `fillStyle` accepting one: before it, the state held a bare `Color` and
    /// a gradient was unexpressible, so `LinearGradientBrush` had nowhere to
    /// land.
    ///
    /// Asserted on PIXELS through the real backend, because a recording
    /// assertion would pass even if the shader were never built.
    #[test]
    fn a_linear_gradient_paints_a_ramp_not_a_flat_fill() {
        use crate::canvas::{Gradient, Paint, TinySkiaCanvas};

        let mut pixmap = tiny_skia::Pixmap::new(64, 8).expect("pixmap");
        let mut gradient = Gradient::linear(0.0, 0.0, 64.0, 0.0);
        gradient.add_color_stop(0.0, Color::rgb(255, 0, 0));
        gradient.add_color_stop(1.0, Color::rgb(0, 0, 255));

        {
            let mut c = TinySkiaCanvas::new(&mut pixmap);
            c.set_fill_paint(&Paint::Gradient(gradient));
            c.fill_rect(0.0, 0.0, 64.0, 8.0);
        }

        let px = |x: u32| {
            let p = pixmap.pixel(x, 4).expect("pixel in bounds");
            (p.red(), p.blue())
        };
        let (left_r, left_b) = px(1);
        let (right_r, right_b) = px(62);

        assert!(
            left_r > 200 && left_b < 60,
            "the left end is the first stop (red), got r={left_r} b={left_b}",
        );
        assert!(
            right_b > 200 && right_r < 60,
            "the right end is the last stop (blue), got r={right_r} b={right_b}",
        );
        // The ramp is the assertion that separates a gradient from a flat
        // fill of either stop: the middle must be neither end.
        let (mid_r, mid_b) = px(32);
        assert!(
            mid_r > 40 && mid_b > 40 && mid_r < 220 && mid_b < 220,
            "the middle interpolates, got r={mid_r} b={mid_b}",
        );
    }

    /// `evenodd` and `nonzero` disagree about a hole, and that disagreement is
    /// the only reason the rule is a parameter. A backend that ignored it
    /// would fill the hole and this would catch it.
    #[test]
    fn the_even_odd_fill_rule_leaves_a_hole_that_nonzero_fills() {
        use crate::canvas::{FillRule, TinySkiaCanvas};

        // Two nested squares wound the SAME way: under `nonzero` the winding
        // numbers add and the inner square is inside; under `evenodd` they
        // cancel and it is a hole.
        let paint_with = |rule: FillRule| {
            let mut pixmap = tiny_skia::Pixmap::new(40, 40).expect("pixmap");
            {
                let mut c = TinySkiaCanvas::new(&mut pixmap);
                c.set_fill_color(Color::rgb(0, 0, 0));
                c.begin_path();
                c.rect(0.0, 0.0, 40.0, 40.0);
                c.rect(10.0, 10.0, 20.0, 20.0);
                c.fill_with_rule(rule);
            }
            pixmap.pixel(20, 20).expect("centre pixel").alpha()
        };

        assert_eq!(
            paint_with(FillRule::NonZero),
            255,
            "nonzero fills the centre"
        );
        assert_eq!(paint_with(FillRule::EvenOdd), 0, "evenodd punches a hole");
    }

    #[test]
    fn the_current_point_is_derived_from_the_commands() {
        // Derived rather than tracked in a field, so it cannot drift from what
        // was actually recorded.
        let mut c = RecordingCanvas::new();
        assert_eq!(end_of(&c), None, "no subpath yet");
        c.move_to(10.0, 20.0);
        assert_eq!(end_of(&c), Some((10.0, 20.0)));
        c.line_to(30.0, 40.0);
        assert_eq!(end_of(&c), Some((30.0, 40.0)));
        // `beginPath` discards the path, so the search must stop there rather
        // than reach into one that no longer exists.
        c.begin_path();
        assert_eq!(end_of(&c), None);
    }

    #[test]
    fn arc_to_falls_back_to_a_line_in_every_degenerate_case() {
        // These are the SPEC's own rules, not guards: a zero radius or three
        // collinear points is defined to add a straight line to (x1, y1).
        // `roundRect` with a zero radius goes through here on every corner.
        for (radius, label) in [(0.0, "zero radius"), (-5.0, "negative radius")] {
            let mut c = RecordingCanvas::new();
            c.move_to(0.0, 0.0);
            c.arc_to(10.0, 0.0, 10.0, 10.0, radius);
            assert_eq!(
                end_of(&c),
                Some((10.0, 0.0)),
                "{label} is a line to (x1,y1)"
            );
            assert!(
                !c.commands
                    .iter()
                    .any(|cmd| matches!(cmd, DrawCmd::Arc { .. })),
                "{label} must not add an arc"
            );
        }
        // Collinear: no corner to fillet.
        let mut c = RecordingCanvas::new();
        c.move_to(0.0, 0.0);
        c.arc_to(10.0, 0.0, 20.0, 0.0, 5.0);
        assert!(
            !c.commands
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::Arc { .. }))
        );
        // With no subpath at all, the spec says start one at (x1, y1).
        let mut c = RecordingCanvas::new();
        c.arc_to(7.0, 8.0, 20.0, 20.0, 5.0);
        assert_eq!(end_of(&c), Some((7.0, 8.0)));
    }

    #[test]
    fn arc_to_fillets_a_right_angle_where_the_tangents_meet() {
        // A 90° corner with radius r: the tangent points sit exactly r back
        // along each leg, which is the one case the geometry can be checked by
        // hand.
        let mut c = RecordingCanvas::new();
        c.move_to(0.0, 0.0);
        c.arc_to(10.0, 0.0, 10.0, 10.0, 4.0);
        // The line runs to the first tangent point, 4 back from the corner.
        assert!(
            c.commands
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::LineTo(x, y) if (*x - 6.0).abs() < 0.01 && y.abs() < 0.01)),
            "line to the tangent point at (6, 0)"
        );
        let arc = c
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                DrawCmd::Arc { x, y, r, .. } => Some((*x, *y, *r)),
                _ => None,
            })
            .expect("an arc rounds the corner");
        assert!(
            (arc.0 - 6.0).abs() < 0.01 && (arc.1 - 4.0).abs() < 0.01,
            "centre at (6, 4), got {arc:?}"
        );
        assert!((arc.2 - 4.0).abs() < 0.01);
    }

    #[test]
    fn round_rect_clamps_a_radius_that_would_overlap() {
        // A radius larger than half the shorter side is scaled down rather than
        // allowed to turn the shape inside out — the spec requires it.
        let mut c = RecordingCanvas::new();
        c.round_rect(0.0, 0.0, 20.0, 10.0, 40.0);
        let radii: Vec<f32> = c
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Arc { r, .. } => Some(*r),
                _ => None,
            })
            .collect();
        assert!(!radii.is_empty(), "the corners are arcs");
        assert!(
            radii.iter().all(|r| (*r - 5.0).abs() < 0.01),
            "clamped to half the 10px side, got {radii:?}"
        );
    }

    #[test]
    fn round_rect_with_no_radius_is_a_rectangle_of_straight_lines() {
        let mut c = RecordingCanvas::new();
        c.round_rect(0.0, 0.0, 20.0, 10.0, 0.0);
        assert!(
            !c.commands
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::Arc { .. })),
            "no corner to round"
        );
        assert!(
            c.commands
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::ClosePath))
        );
    }

    #[test]
    fn captures_calls_in_order() {
        let mut c = RecordingCanvas::new();
        c.set_fill_color(Color::rgb(255, 0, 0));
        c.fill_rect(0.0, 0.0, 10.0, 10.0);
        c.set_stroke_color(Color::rgb(0, 0, 0));
        c.set_line_width(2.0);
        c.begin_path();
        c.move_to(0.0, 0.0);
        c.line_to(100.0, 100.0);
        c.stroke();

        assert_eq!(c.len(), 8);
        assert_eq!(c.commands[0], DrawCmd::SetFillColor(Color::rgb(255, 0, 0)));
        assert_eq!(
            c.commands[1],
            DrawCmd::FillRect {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0
            }
        );
        assert_eq!(c.commands[7], DrawCmd::Stroke);
    }

    #[test]
    fn replay_dispatches_to_target() {
        let mut src = RecordingCanvas::new();
        src.set_fill_color(Color::rgb(1, 2, 3));
        src.fill_rect(4.0, 5.0, 6.0, 7.0);
        src.move_to(8.0, 9.0);

        let mut dst = RecordingCanvas::new();
        src.replay(&mut dst);

        assert_eq!(src.commands, dst.commands);
    }

    #[test]
    fn clear_drops_all_commands() {
        let mut c = RecordingCanvas::new();
        c.fill_rect(0.0, 0.0, 1.0, 1.0);
        assert_eq!(c.len(), 1);
        c.clear();
        assert!(c.is_empty());
    }
}

#[cfg(test)]
mod put_image_data_tests {
    use super::*;
    use crate::canvas::Color;

    fn red_pixel() -> Image {
        Image::from_rgba(1, 1, vec![255, 0, 0, 255])
    }

    /// `putImageData` is a RAW write, so it is its own command — not a
    /// `drawImage` sized to the image.
    ///
    /// The two take the same arguments and honour different things: this one
    /// ignores the transform, the clip, `globalAlpha` and the blend mode. Were
    /// it recorded as a `DrawImage`, replay would pick all four up from
    /// whatever state it had reached by then, and a software renderer's frame
    /// would land somewhere else, tinted, or clipped away.
    #[test]
    fn put_image_data_records_as_itself_not_as_a_draw_image() {
        let mut rec = RecordingCanvas::new();
        rec.set_global_alpha(0.5);
        rec.translate(100.0, 100.0);
        rec.put_image_data(&red_pixel(), 7.0, 9.0);

        let written = rec
            .commands
            .iter()
            .filter(|c| matches!(c, DrawCmd::PutImageData { .. }))
            .count();
        assert_eq!(written, 1, "recorded once, as itself");
        assert!(
            !rec.commands
                .iter()
                .any(|c| matches!(c, DrawCmd::DrawImage { .. })),
            "and never as a draw_image"
        );
        // The destination is what the caller said, unshifted by the
        // translate that precedes it — the transform is not applied at
        // record time either.
        assert!(matches!(
            rec.commands.last(),
            Some(DrawCmd::PutImageData { dx, dy, .. }) if *dx == 7.0 && *dy == 9.0
        ));
    }

    /// Replay dispatches it to `put_image_data`, not to `draw_image`.
    #[test]
    fn replay_keeps_the_two_apart() {
        let mut rec = RecordingCanvas::new();
        rec.put_image_data(&red_pixel(), 1.0, 2.0);
        rec.draw_image(&red_pixel(), 3.0, 4.0, 5.0, 6.0);

        let mut replayed = RecordingCanvas::new();
        rec.replay(&mut replayed);
        assert_eq!(replayed.commands, rec.commands, "one arm each, in order");
    }

    /// Paint state is recorded around it and belongs to the OTHER commands —
    /// this is the guard against someone "simplifying" the arm later.
    #[test]
    fn surrounding_state_is_untouched_by_the_raw_write() {
        let mut rec = RecordingCanvas::new();
        rec.set_fill_color(Color {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        });
        rec.put_image_data(&red_pixel(), 0.0, 0.0);
        rec.fill_rect(0.0, 0.0, 10.0, 10.0);
        assert!(matches!(
            rec.commands.first(),
            Some(DrawCmd::SetFillColor(_))
        ));
        assert!(matches!(
            rec.commands.last(),
            Some(DrawCmd::FillRect { .. })
        ));
    }

    // ─── setLineDash / getLineDash ──────────────────────────────────────

    #[test]
    fn an_odd_dash_list_reads_back_doubled() {
        // HTML §4.12.5: a dash list is read in on/off pairs, so an odd list is
        // concatenated with itself. `[5]` is a 5-on 5-off pattern, and
        // `getLineDash` must say so rather than echoing the argument.
        let mut c = RecordingCanvas::new();
        c.set_line_dash(&[5.0]);
        assert_eq!(c.get_line_dash(), vec![5.0, 5.0]);

        c.set_line_dash(&[1.0, 2.0, 3.0]);
        assert_eq!(c.get_line_dash(), vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn an_even_dash_list_is_left_alone() {
        let mut c = RecordingCanvas::new();
        c.set_line_dash(&[4.0, 2.0]);
        assert_eq!(c.get_line_dash(), vec![4.0, 2.0]);
    }

    #[test]
    fn a_dash_list_with_a_negative_value_is_rejected_whole() {
        // The previous pattern stays in force. Filtering the bad entry out
        // instead would silently re-pair every dash with the wrong gap.
        let mut c = RecordingCanvas::new();
        c.set_line_dash(&[4.0, 2.0]);
        c.set_line_dash(&[1.0, -1.0]);
        assert_eq!(
            c.get_line_dash(),
            vec![4.0, 2.0],
            "the bad list did nothing"
        );
    }

    #[test]
    fn an_empty_dash_list_means_solid() {
        let mut c = RecordingCanvas::new();
        c.set_line_dash(&[4.0, 2.0]);
        c.set_line_dash(&[]);
        assert!(c.get_line_dash().is_empty());
    }

    // ─── getTransform ───────────────────────────────────────────────────

    #[test]
    fn get_transform_reports_what_was_composed() {
        let mut c = RecordingCanvas::new();
        assert_eq!(c.get_transform(), super::super::Matrix::IDENTITY);
        c.translate(10.0, 20.0);
        c.scale(2.0, 3.0);
        let m = c.get_transform();
        // Scale then translate, in that composition order: the point (1,1) in
        // user space lands at (10+2, 20+3).
        let (x, y) = m.apply(1.0, 1.0);
        assert!((x - 12.0).abs() < 0.001, "got {x}");
        assert!((y - 23.0).abs() < 0.001, "got {y}");
    }

    #[test]
    fn set_transform_replaces_rather_than_composes() {
        // The whole difference between `transform` and `setTransform`, and the
        // one a canvas gets wrong by treating them as the same call.
        let mut c = RecordingCanvas::new();
        c.translate(100.0, 100.0);
        c.set_transform(super::super::Matrix::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0));
        let (x, y) = c.get_transform().apply(1.0, 1.0);
        assert_eq!((x, y), (2.0, 2.0), "the translate is gone, not compounded");
    }

    #[test]
    fn restore_puts_back_the_transform_that_save_captured() {
        let mut c = RecordingCanvas::new();
        c.translate(5.0, 5.0);
        c.save();
        c.scale(10.0, 10.0);
        c.restore();
        let (x, y) = c.get_transform().apply(1.0, 1.0);
        assert_eq!((x, y), (6.0, 6.0), "back to the translate alone");
    }

    #[test]
    fn an_unbalanced_restore_is_a_no_op() {
        // Per spec, popping an empty state stack does nothing — it is not an
        // error and must not reset the transform.
        let mut c = RecordingCanvas::new();
        c.translate(5.0, 5.0);
        c.restore();
        let (x, y) = c.get_transform().apply(0.0, 0.0);
        assert_eq!((x, y), (5.0, 5.0));
    }

    // ─── reset ──────────────────────────────────────────────────────────

    #[test]
    fn reset_drops_the_recording_and_the_state() {
        let mut c = RecordingCanvas::new();
        c.translate(50.0, 50.0);
        c.set_line_dash(&[3.0, 1.0]);
        c.fill_rect(0.0, 0.0, 10.0, 10.0);
        c.reset();
        assert_eq!(c.get_transform(), super::super::Matrix::IDENTITY);
        assert!(c.get_line_dash().is_empty());
        // Exactly one command: the `Reset` itself. Everything drawn before it
        // is gone, because the spec clears the bitmap.
        assert_eq!(c.commands.len(), 1);
        assert!(matches!(c.commands.first(), Some(DrawCmd::Reset)));
    }

    // ─── roundRect with four radii ──────────────────────────────────────

    #[test]
    fn round_rect_rounds_each_corner_by_its_own_radius() {
        // The reason four radii is the general case and not a convenience: a
        // tab rounds its top corners and leaves the bottom square.
        let mut c = RecordingCanvas::new();
        c.round_rect_radii(0.0, 0.0, 100.0, 50.0, [10.0, 20.0, 0.0, 0.0]);
        let radii: Vec<f32> = c
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Arc { r, .. } => Some(*r),
                _ => None,
            })
            .collect();
        // Only the two non-zero corners become arcs; a zero radius is a
        // straight line through `arcTo`'s own degenerate case.
        assert_eq!(radii.len(), 2, "got {radii:?}");
        assert!(radii.contains(&20.0), "top-right, got {radii:?}");
        assert!(radii.contains(&10.0), "top-left, got {radii:?}");
    }

    #[test]
    fn overlapping_radii_scale_together_and_keep_their_proportions() {
        // The spec scales ALL FOUR by one factor rather than clamping each. A
        // 2:1 pair must still be 2:1 after the shape is made to fit.
        let mut c = RecordingCanvas::new();
        c.round_rect_radii(0.0, 0.0, 30.0, 100.0, [40.0, 20.0, 0.0, 0.0]);
        let mut radii: Vec<f32> = c
            .commands
            .iter()
            .filter_map(|cmd| match cmd {
                DrawCmd::Arc { r, .. } => Some(*r),
                _ => None,
            })
            .collect();
        radii.sort_by(|a, b| b.partial_cmp(a).unwrap());
        assert_eq!(radii.len(), 2, "got {radii:?}");
        // The top edge is 30 wide and must hold 40 + 20, so the factor is 0.5.
        assert!((radii[0] - 20.0).abs() < 0.01, "got {radii:?}");
        assert!((radii[1] - 10.0).abs() < 0.01, "got {radii:?}");
    }

    // ─── Path2D ─────────────────────────────────────────────────────────

    #[test]
    fn a_path2d_replays_as_the_calls_that_built_it() {
        let mut path = super::super::Path2D::new();
        path.move_to(0.0, 0.0);
        path.line_to(10.0, 0.0);
        path.line_to(10.0, 10.0);
        path.close_path();

        let mut c = RecordingCanvas::new();
        c.fill_path(&path, super::super::FillRule::NonZero);

        // `fill(path)` begins a path of its own so the context's current path
        // is not filled along with it.
        assert!(matches!(c.commands.first(), Some(DrawCmd::BeginPath)));
        assert_eq!(
            c.commands
                .iter()
                .filter(|cmd| matches!(cmd, DrawCmd::LineTo(..)))
                .count(),
            2
        );
        assert!(matches!(c.commands.last(), Some(DrawCmd::FillWithRule(_))));
    }

    #[test]
    fn add_path_maps_the_added_path_through_its_transform() {
        let mut inner = super::super::Path2D::new();
        inner.move_to(1.0, 1.0);
        inner.line_to(2.0, 2.0);

        let mut outer = super::super::Path2D::new();
        outer.add_path(
            &inner,
            super::super::Matrix::new(10.0, 0.0, 0.0, 10.0, 0.0, 0.0),
        );

        assert_eq!(
            outer.ops,
            vec![
                super::super::PathOp::MoveTo(10.0, 10.0),
                super::super::PathOp::LineTo(20.0, 20.0),
            ]
        );
    }

    #[test]
    fn a_circle_added_under_a_non_uniform_transform_becomes_an_ellipse() {
        // `PathOp::Arc` cannot hold a squashed circle, so the distortion would
        // be silently dropped if it stayed an arc.
        let mut inner = super::super::Path2D::new();
        inner.arc(0.0, 0.0, 1.0, 0.0, std::f32::consts::TAU, false);

        let mut outer = super::super::Path2D::new();
        outer.add_path(
            &inner,
            super::super::Matrix::new(4.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        );

        match outer.ops.first() {
            Some(super::super::PathOp::Ellipse { rx, ry, .. }) => {
                assert_eq!((*rx, *ry), (4.0, 1.0));
            }
            other => panic!("expected an ellipse, got {other:?}"),
        }
    }

    // ─── globalCompositeOperation ───────────────────────────────────────

    #[test]
    fn every_composite_keyword_round_trips() {
        // All 26, by construction: if `parse` and `as_str` ever disagree about
        // one, this catches it without anyone remembering to add a case.
        for keyword in [
            "source-over",
            "source-in",
            "source-out",
            "source-atop",
            "destination-over",
            "destination-in",
            "destination-out",
            "destination-atop",
            "lighter",
            "copy",
            "xor",
            "multiply",
            "screen",
            "overlay",
            "darken",
            "lighten",
            "color-dodge",
            "color-burn",
            "hard-light",
            "soft-light",
            "difference",
            "exclusion",
            "hue",
            "saturation",
            "color",
            "luminosity",
        ] {
            let parsed = super::super::CompositeOp::parse(keyword)
                .unwrap_or_else(|| panic!("{keyword} is a spec keyword"));
            assert_eq!(parsed.as_str(), keyword);
        }
    }

    // ─── CanvasFillStrokeStyles factories ───────────────────────────────

    #[test]
    fn the_context_is_the_only_door_to_a_gradient() {
        // These are context methods in the IDL, not constructors — a page can
        // only get a `CanvasGradient` from `ctx.createLinearGradient(...)`.
        let mut c = RecordingCanvas::new();
        let mut g = c.create_linear_gradient(0.0, 0.0, 100.0, 0.0);
        g.add_color_stop(0.0, Color::rgb(255, 0, 0));
        g.add_color_stop(1.0, Color::rgb(0, 0, 255));
        c.set_fill_paint(&super::super::Paint::Gradient(g));
        assert!(matches!(c.commands.last(), Some(DrawCmd::SetFillPaint(_))));
    }

    #[test]
    fn a_conic_gradient_takes_its_angle_first() {
        // The spec's argument order is `(startAngle, x, y)`, not `(x, y,
        // angle)` — getting it backwards puts the centre at the angle.
        let c = RecordingCanvas::new();
        let g = c.create_conic_gradient(1.5, 40.0, 60.0);
        assert_eq!(
            g.kind,
            super::super::GradientKind::Conic {
                start_angle: 1.5,
                x: 40.0,
                y: 60.0
            }
        );
    }

    #[test]
    fn an_unknown_repetition_keyword_has_no_pattern() {
        // A `SyntaxError` in the spec, not a silent fall back to `repeat`:
        // whether the image tiles is the whole of what the argument says.
        let c = RecordingCanvas::new();
        let image = super::super::Image::from_rgba(1, 1, vec![0, 0, 0, 255]);
        assert!(c.create_pattern(&image, "repeat").is_some());
        assert!(c.create_pattern(&image, "no-repeat").is_some());
        assert!(c.create_pattern(&image, "tile").is_none());
    }

    // ─── putImageData dirty rectangle ───────────────────────────────────

    #[test]
    fn a_negative_dirty_extent_runs_the_other_way_rather_than_writing_nothing() {
        // `dirtyWidth` is a `long` in the IDL and the spec normalises a
        // negative one before clipping. Left un-normalised the clipped
        // rectangle inverts and the write silently does nothing at all.
        let source =
            super::super::ImageData::from_rgba(4, 4, vec![255u8; 4 * 4 * 4]).expect("a 4x4 buffer");

        let mut c = RecordingCanvas::new();
        // (3,3) with extent -2 is the same rectangle as (1,1) with extent 2.
        c.put_image_data_dirty(&source, 0.0, 0.0, 3, 3, -2, -2);
        let negative = c.commands.len();

        let mut c = RecordingCanvas::new();
        c.put_image_data_dirty(&source, 0.0, 0.0, 1, 1, 2, 2);
        let positive = c.commands.len();

        assert_eq!(negative, positive, "both describe the same 2x2 region");
        assert_eq!(negative, 1, "and both actually wrote");
    }

    #[test]
    fn an_empty_dirty_rectangle_writes_nothing() {
        let source = super::super::ImageData::from_rgba(4, 4, vec![255u8; 4 * 4 * 4]).expect("4x4");
        let mut c = RecordingCanvas::new();
        c.put_image_data_dirty(&source, 0.0, 0.0, 0, 0, 0, 0);
        assert!(c.commands.is_empty());
    }

    #[test]
    fn an_unknown_composite_keyword_is_rejected_rather_than_defaulted() {
        // The spec leaves the attribute UNCHANGED for an unrecognised value, so
        // the caller has to be able to tell "not recognised" from "recognised".
        assert!(super::super::CompositeOp::parse("sourceover").is_none());
        assert!(super::super::CompositeOp::parse("").is_none());
    }
}
