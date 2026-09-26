//! Display list — recorded paint commands for cached rendering.
//!
//! Instead of painting directly to a pixmap, the renderer builds a display list
//! of paint commands. The list can be cached per stacking context and only
//! rebuilt when dirty. Replaying a cached list is much faster than re-traversing
//! the box tree and re-computing geometry.
//!
//! This is an intermediate representation between the layout tree and the final
//! rasterized output. It enables:
//! - Cached stacking contexts (only repaint what changed)
//! - Hit testing by walking the list in reverse
//! - Debug/inspector visualization
//! - Future: GPU acceleration, layer compositing

use crate::types::{Color, GradientDirection, Rect, TextUnderlinePosition};

/// A single paint command in the display list.
#[derive(Clone, Debug, PartialEq)]
pub enum PaintCmd {
    /// Fill a rectangle with a solid color.
    FillRect {
        rect: Rect,
        color: Color,
        radius: [f32; 4],
        radius_y: [f32; 4],
    },

    /// Draw a border on a rectangle.
    Border {
        rect: Rect,
        widths: [f32; 4], // top, right, bottom, left
        colors: [Color; 4],
        styles: [u8; 4], // 0=none, 1=solid, 2=dashed, 3=dotted, 4=double, etc.
        radii: [f32; 4], // top-left, top-right, bottom-right, bottom-left
        radii_y: [f32; 4],
        opacity: f32,
    },

    /// Draw a decoded CSS border-image source clipped to the border ring.
    BorderImage {
        rect: Rect,
        widths: [f32; 4],
        slices: [f32; 4],
        repeat_x_mode: u8, // 0=stretch, 1=repeat, 2=space, 3=round
        repeat_y_mode: u8, // 0=stretch, 1=repeat, 2=space, 3=round
        fill_center: bool,
        data: ImageRef,
    },

    /// Draw a text run at a position.
    Text {
        x: f32,
        y: f32,
        text: String,
        font_family: String,
        font_size: f32,
        font_weight: u16,
        font_style: u8,    // 0=normal, 1=italic, 2=oblique
        font_stretch: f32, // percentage (100.0 = normal)
        line_height: f32,
        color: Color,
        decoration: TextDecoration,
        letter_spacing: f32,
        word_spacing: f32,
        small_caps: bool,
    },

    /// A CSS gradient painted through descendant text glyphs.
    PushTextGradient {
        rect: Rect,
        background_color: Color,
        gradient_type: u8,
        angle: f32,
        direction: GradientDirection,
        radial_center_x: f32,
        radial_center_y: f32,
        radial_radius_x: f32,
        radial_radius_y: f32,
        stops: Vec<(Color, f32)>,
    },
    PopTextGradient,

    /// Draw an image (RGBA data) at a position.
    Image { rect: Rect, data: ImageRef },

    /// Push a clip rectangle — all subsequent commands are clipped to this rect.
    PushClip {
        rect: Rect,
        radius: [f32; 4],
        radius_y: [f32; 4],
    },

    /// Push an arbitrary polygon clip in document coordinates.
    PushClipPath { points: Vec<(f32, f32)> },

    /// Pop the current clip.
    PopClip,

    /// Push a CSS transform.
    PushTransform { node_id: u32, transform: [f32; 6] }, // 2D affine: [a, b, c, d, e, f]

    /// Pop the current transform.
    PopTransform,

    /// Push opacity — all subsequent commands are rendered with this alpha.
    PushOpacity { alpha: f32 },

    /// Pop opacity.
    PopOpacity,

    /// Push a CSS filter — content rendered into a layer, filter applied on pop.
    PushFilter {
        filters: Vec<(u8, f32, f32, f32, crate::types::Color)>,
    },
    // filter type: 0=blur, 1=brightness, 2=contrast, 3=grayscale, 4=hue-rotate,
    //              5=invert, 6=opacity, 7=saturate, 8=sepia, 9=drop-shadow
    /// Pop filter layer.
    PopFilter,

    /// Filter the already-painted backdrop within the element's border box.
    BackdropFilter {
        rect: Rect,
        filters: Vec<(u8, f32, f32, f32, crate::types::Color)>,
    },

    /// Push a CSS mask layer. Subsequent element content is rendered offscreen,
    /// then composited through the mask image on pop.
    PushMask { rect: Rect, data: ImageRef },

    /// Pop the current CSS mask layer.
    PopMask,

    /// Push a blend mode layer — subsequent content is composited with this mode.
    PushBlendMode { mode: u8 }, // 0=normal, 1=multiply, 2=screen, 3=overlay, etc.

    /// Pop blend mode.
    PopBlendMode,

    /// Draw a box shadow.
    BoxShadow {
        rect: Rect,
        color: Color,
        offset_x: f32,
        offset_y: f32,
        blur: f32,
        spread: f32,
        inset: bool,
        radii: [f32; 4],
        radii_y: [f32; 4],
    },

    /// Draw a linear or radial gradient.
    Gradient {
        /// The background POSITIONING area — `background-origin`, initially the
        /// padding box. The gradient's geometry is derived from this rect.
        rect: Rect,
        /// The background PAINTING area — `background-clip`, initially the
        /// border box. Only the part of the gradient inside this rect is drawn,
        /// which is a different box from `rect` whenever the two properties
        /// disagree (css-backgrounds-3 §3.6, §3.7).
        clip: Rect,
        /// `background-repeat`, resolved per axis. A repeating gradient tiles
        /// its positioning area across the painting area.
        repeat_x_mode: u8, // 0=no-repeat, 1=repeat, 2=space, 3=round
        repeat_y_mode: u8, // 0=no-repeat, 1=repeat, 2=space, 3=round
        gradient_type: u8, // 1=linear, 2=radial
        angle: f32,
        direction: GradientDirection,
        radial_center_x: f32,
        radial_center_y: f32,
        radial_radius_x: f32,
        radial_radius_y: f32,
        stops: Vec<(Color, f32)>, // (color, position 0..1)
        radii: [f32; 4],
        radii_y: [f32; 4],
        opacity: f32,
        blend_mode: u8,
    },

    /// Draw an outline (CSS outline property).
    Outline {
        rect: Rect,
        width: f32,
        color: Color,
        style: u8, // same encoding as border styles
        offset: f32,
        radii: [f32; 4],
        radii_y: [f32; 4],
    },

    /// Draw the native resize affordance for CSS `resize`.
    ResizeGrip {
        rect: Rect,
        color: Color,
        mode: u8, // 1=both, 2=horizontal, 3=vertical
    },

    /// Draw a horizontal line (for <hr>).
    HorizontalRule { x1: f32, y1: f32, x2: f32 },

    /// List marker: 0=disc, 1=circle, 2=square, 3=text, 4=image URL in `text`.
    ListMarker {
        marker_type: u8,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        text: String, // for numbered markers
        image: Option<ImageRef>,
        font_family: String,
        font_size: f32,
        font_weight: u16,
        font_style: u8,
        line_height: f32,
    },

    /// Form element placeholder — the replay function handles rendering.
    FormElement {
        tag: String,
        input_type: String,
        rect: Rect,
        node_id: u32,
        attributes: Vec<(String, String)>,
        font_size: f32,
        font_weight: u16,
        font_family: String,
        color: Color,
        text_indent: f32,
        placeholder_color: Color,
        file_button_color: Color,
        file_button_background: Color,
        file_button_font_size: f32,
        file_button_font_weight: u16,
        file_button_font_family: String,
        checked: bool,
        value: String,
        placeholder: String,
        input_cursor: usize,
        appearance_none: bool,
        /// The control's writing mode is VERTICAL (`vertical-rl`/`vertical-lr`).
        ///
        /// A form control is laid out along its INLINE axis, and a vertical
        /// writing mode turns that axis on its side — which is how CSS says
        /// "this slider runs up and down" without a second element or a
        /// non-standard `orient` attribute. Resolved here, where the computed
        /// style is, rather than sniffed out of the `style` attribute in the
        /// painter: a stylesheet rule sets it just as well as an inline one.
        vertical: bool,
        /// A `<select>`'s option LABELS, in tree order. Empty for every other
        /// control.
        ///
        /// A list box draws its options itself — it is not a closed control
        /// with one visible value — so the labels have to reach the painter.
        /// The closed dropdown needs only `value`, which is why this was not
        /// here before.
        options: Vec<String>,
        /// The index of the FIRST selected option, or `-1` for none — which is
        /// the normal resting state of a list box, whose options HTML does not
        /// auto-select.
        selected: i32,
        /// The selectedness of every option, parallel to `options`. A
        /// `multiple` list box paints all of them, so one index is not enough.
        selected_all: Vec<bool>,
    },

    /// Draw a text shadow (separate from main text for layering).
    TextShadow {
        x: f32,
        y: f32,
        text: String,
        font_family: String,
        font_size: f32,
        font_weight: u16,
        font_style: u8,
        font_stretch: f32,
        line_height: f32,
        color: Color,
        blur: f32,
        letter_spacing: f32,
        word_spacing: f32,
        small_caps: bool,
    },

    /// Background image with positioning/sizing metadata.
    BackgroundImage {
        /// The background POSITIONING area — `background-origin`, initially the
        /// padding box. The builder has already resolved `pos_x`/`pos_y` and
        /// `draw_w`/`draw_h` against it, so the painter reads it only for
        /// diagnostics.
        container: Rect,
        /// The background PAINTING area — `background-clip`, initially the
        /// border box. Tiles fill it and nothing is drawn outside it
        /// (css-backgrounds-3 §3.6, §3.7).
        clip: Rect,
        data: ImageRef,
        size_mode: u8, // 0=auto, 1=cover, 2=contain, 3=explicit
        draw_w: f32,
        draw_h: f32,
        pos_x: f32,
        pos_y: f32,
        repeat_x_mode: u8, // 0=no-repeat, 1=repeat, 2=space, 3=round
        repeat_y_mode: u8, // 0=no-repeat, 1=repeat, 2=space, 3=round
        radii: [f32; 4],
        radii_y: [f32; 4],
        blend_mode: u8,
    },

    /// Marker: start of a stacking context.
    BeginStackingContext { node_id: u32, z_index: i32 },

    /// Marker: end of a stacking context.
    EndStackingContext,
    /// Paint the enclosed fixed-position box in viewport coordinates while
    /// preserving its place in the surrounding stacking order.
    BeginFixedPosition,
    EndFixedPosition,
}

/// Text decoration info for a text run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub strikethrough: bool,
    pub color: Color,
    pub style: u8, // 0=solid, 1=double, 2=dotted, 3=dashed, 4=wavy
    pub thickness: f32,
    pub underline_offset: f32,
    pub underline_position: TextUnderlinePosition,
    pub skip_ink: bool,
}

/// Reference to image data — avoids cloning large pixel buffers.
#[derive(Clone, Debug, PartialEq)]
pub enum ImageRef {
    /// Inline RGBA data (for small images or when we need ownership).
    Owned(Vec<u8>, u32, u32), // (rgba_data, width, height)
    /// Shared reference via Arc (for large images).
    Shared(std::sync::Arc<Vec<u8>>, u32, u32),
}

/// A display list — ordered sequence of paint commands.
#[derive(Clone, Debug, Default)]
pub struct DisplayList {
    pub commands: Vec<PaintCmd>,
    /// True when `commands` contains a `position: sticky` element whose
    /// recorded position depends on the current scroll offset.
    pub has_scroll_dependent_sticky: bool,
    /// Legacy viewport overlay commands. CSS fixed boxes now remain in
    /// `commands` at their stacking position, between Begin/EndFixedPosition.
    /// The replay switch keeps them anchored while the document scrolls.
    pub fixed_commands: Vec<PaintCmd>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            has_scroll_dependent_sticky: false,
            fixed_commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: PaintCmd) {
        self.commands.push(cmd);
    }

    pub fn clear(&mut self) {
        self.commands.clear();
        self.has_scroll_dependent_sticky = false;
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn memory_estimate(&self) -> DisplayListMemoryEstimate {
        let commands = self.commands.len() + self.fixed_commands.len();
        let mut estimate = DisplayListMemoryEstimate {
            commands,
            inline_command_bytes: commands.saturating_mul(std::mem::size_of::<PaintCmd>()),
            ..Default::default()
        };
        let mut seen_shared_images = std::collections::HashSet::<usize>::new();
        for cmd in self.commands.iter().chain(self.fixed_commands.iter()) {
            estimate.add_command(cmd, &mut seen_shared_images);
        }
        estimate
    }

    /// Hit test: find the deepest node_id at a point by walking the display
    /// list in reverse (last painted = topmost visual).
    pub fn hit_test(&self, x: f32, y: f32) -> Option<u32> {
        let _clip_stack: Vec<Rect> = Vec::new();
        let mut stacking_ids: Vec<u32> = Vec::new();

        // Walk forward to build clip context, then check each rect
        for cmd in self.commands.iter().rev() {
            match cmd {
                PaintCmd::EndStackingContext => {
                    // Entering a stacking context (reverse order)
                }
                PaintCmd::BeginStackingContext { node_id, .. } => {
                    stacking_ids.push(*node_id);
                }
                PaintCmd::FillRect { rect, .. } => {
                    if rect.contains(x, y) {
                        // Return the most recent stacking context node_id
                        if let Some(&id) = stacking_ids.last() {
                            if id != 0 {
                                return Some(id);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DisplayListMemoryEstimate {
    pub commands: usize,
    pub inline_command_bytes: usize,
    pub heap_bytes: usize,
    pub text_bytes: usize,
    pub image_bytes: usize,
    pub vector_bytes: usize,
}

impl DisplayListMemoryEstimate {
    pub fn total_bytes(self) -> usize {
        self.inline_command_bytes.saturating_add(self.heap_bytes)
    }

    fn add_command(
        &mut self,
        cmd: &PaintCmd,
        seen_shared_images: &mut std::collections::HashSet<usize>,
    ) {
        match cmd {
            PaintCmd::Text {
                text, font_family, ..
            }
            | PaintCmd::TextShadow {
                text, font_family, ..
            } => {
                self.add_string(text);
                self.add_string(font_family);
            }
            PaintCmd::Image { data, .. }
            | PaintCmd::BorderImage { data, .. }
            | PaintCmd::PushMask { data, .. }
            | PaintCmd::BackgroundImage { data, .. } => self.add_image(data, seen_shared_images),
            PaintCmd::ListMarker {
                text,
                image,
                font_family,
                ..
            } => {
                self.add_string(text);
                self.add_string(font_family);
                if let Some(image) = image {
                    self.add_image(image, seen_shared_images);
                }
            }
            PaintCmd::PushClipPath { points } => {
                self.add_vec_bytes(
                    points
                        .capacity()
                        .saturating_mul(std::mem::size_of::<(f32, f32)>()),
                );
            }
            PaintCmd::PushFilter { filters } | PaintCmd::BackdropFilter { filters, .. } => {
                self.add_vec_bytes(filters.capacity().saturating_mul(std::mem::size_of::<(
                    u8,
                    f32,
                    f32,
                    f32,
                    crate::types::Color,
                )>()));
            }
            PaintCmd::Gradient { stops, .. } | PaintCmd::PushTextGradient { stops, .. } => {
                self.add_vec_bytes(
                    stops
                        .capacity()
                        .saturating_mul(std::mem::size_of::<(Color, f32)>()),
                );
            }
            PaintCmd::FormElement {
                tag,
                input_type,
                attributes,
                font_family,
                file_button_font_family,
                value,
                placeholder,
                options,
                selected_all,
                ..
            } => {
                self.add_string(tag);
                self.add_string(input_type);
                self.add_string(font_family);
                self.add_string(file_button_font_family);
                self.add_string(value);
                self.add_string(placeholder);
                self.add_vec_bytes(
                    attributes
                        .capacity()
                        .saturating_mul(std::mem::size_of::<(String, String)>()),
                );
                for (name, value) in attributes {
                    self.add_string(name);
                    self.add_string(value);
                }
                self.add_vec_bytes(
                    options
                        .capacity()
                        .saturating_mul(std::mem::size_of::<String>()),
                );
                for option in options {
                    self.add_string(option);
                }
                self.add_vec_bytes(
                    selected_all
                        .capacity()
                        .saturating_mul(std::mem::size_of::<bool>()),
                );
            }
            _ => {}
        }
    }

    fn add_string(&mut self, value: &String) {
        self.text_bytes = self.text_bytes.saturating_add(value.capacity());
        self.heap_bytes = self.heap_bytes.saturating_add(value.capacity());
    }

    fn add_vec_bytes(&mut self, bytes: usize) {
        self.vector_bytes = self.vector_bytes.saturating_add(bytes);
        self.heap_bytes = self.heap_bytes.saturating_add(bytes);
    }

    fn add_image(
        &mut self,
        image: &ImageRef,
        seen_shared_images: &mut std::collections::HashSet<usize>,
    ) {
        let bytes = match image {
            ImageRef::Owned(data, _, _) => data.capacity(),
            ImageRef::Shared(data, _, _) => {
                let ptr = std::sync::Arc::as_ptr(data) as usize;
                if !seen_shared_images.insert(ptr) {
                    return;
                }
                data.capacity()
            }
        };
        self.image_bytes = self.image_bytes.saturating_add(bytes);
        self.heap_bytes = self.heap_bytes.saturating_add(bytes);
    }
}

/// A cached stacking context — contains a display list and dirty flag.
#[derive(Clone, Debug)]
pub struct StackingContextCache {
    pub node_id: u32,
    pub z_index: i32,
    pub list: DisplayList,
    pub dirty: bool,
}

impl StackingContextCache {
    pub fn new(node_id: u32, z_index: i32) -> Self {
        Self {
            node_id,
            z_index,
            list: DisplayList::new(),
            dirty: true,
        }
    }
}
