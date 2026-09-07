//! `getComputedStyle` — CSSOM §6.6.
//!
//! ⛔ ONE unit on purpose, and moved as one. The resolution has an ORDERING
//! dependency that no signature expresses: the layout-rect path must be tried
//! before the cascaded-style path, because a box with a used size answers from
//! its rect and one without answers from its computed value. A mutation
//! proved the order is load-bearing, so splitting these across modules would
//! put a silent constraint across a file boundary.
//!
//! The serializers are free functions BELOW the `impl`, not interleaved with
//! it — which is how they came to be stranded at the bottom of `api.rs` under
//! a second `impl Document` block.

use crate::types::Document;

impl Document {
    /// `getComputedStyle(element).getPropertyValue(property)` — the RESOLVED
    /// value, in used units.
    ///
    /// Reading this FORCES LAYOUT, as it does in a browser: a document built
    /// but never laid out has no geometry, and `0px` is the absence of an
    /// answer rather than an answer. Everything that is not geometry falls back
    /// to the declared value — an honest floor, since fully resolving every
    /// property against the cascade is a larger job than this.
    pub fn computed_style_property(&mut self, id: u32, property: &str) -> String {
        let width = self.viewport_w;
        crate::layout::LayoutEngine::new().layout(self, width);
        let rect = self.get_bounding_client_rect(id);
        let property = property.to_ascii_lowercase();
        // **An inset is the used value only when the element is POSITIONED**
        // (CSSOM §6.6.1). For a static box `top` has no effect at all, so its
        // resolved value is the COMPUTED one — `1em` on a 16px font is `16px`,
        // wherever the box happens to sit on the page.
        //
        // Answering the bounding rect for every element made `top` mean "how
        // far down the page you are", so a static box inside a body with the
        // UA's 8px margin reported `8px` for a declared `1em` — and moving the
        // box changed a value the box never set.
        let inset = matches!(property.as_str(), "left" | "top" | "right" | "bottom");
        let positioned = self
            .get_computed_style(id)
            .map(|s| !matches!(s.position, crate::types::Position::Static))
            .unwrap_or(false);
        // ⛔ RELATIVE is its own case, not "positioned, so use the rect". A
        // relative box's inset is the OFFSET it was shifted by, which is 0
        // when unset — the rect path answers where the box sits on the page
        // instead (measured: Chrome says `0px`, this said `27.2px`). The two
        // edges also mirror: `bottom` is `-top` and `right` is `-left`
        // (CSS 2.1 §9.4.3), so `top: 10px` makes `bottom` answer `-10px`.
        if inset
            && matches!(
                self.get_computed_style(id).map(|s| s.position),
                Some(crate::types::Position::Relative)
            )
        {
            let Some(style) = self.get_computed_style(id) else {
                return String::new();
            };
            let font_px = style.font_size.resolve(16.0, 16.0, 16.0);
            let resolve = |l: &crate::types::CssLength| -> Option<f32> {
                match l {
                    crate::types::CssLength::Auto | crate::types::CssLength::None => None,
                    other => Some(other.resolve(font_px, self.viewport_w, self.root_font_px())),
                }
            };
            let vertical = resolve(&style.top)
                .or_else(|| resolve(&style.bottom).map(|v| -v))
                .unwrap_or(0.0);
            let horizontal = resolve(&style.left)
                .or_else(|| resolve(&style.right).map(|v| -v))
                .unwrap_or(0.0);
            return match property.as_str() {
                "top" => px(vertical),
                "bottom" => px(-vertical),
                "left" => px(horizontal),
                _ => px(-horizontal),
            };
        }
        if inset && !positioned {
            let style = match self.get_computed_style(id) {
                Some(style) => style,
                None => return String::new(),
            };
            let declared = match property.as_str() {
                "left" => style.left.clone(),
                "top" => style.top.clone(),
                "right" => style.right.clone(),
                _ => style.bottom.clone(),
            };
            let font_px = style.font_size.resolve(16.0, 16.0, 16.0);
            // `auto` is the initial value and stays the word — it is not a
            // length and serialising it as `0px` would claim the box was
            // placed. Percentages stay percentages, as the spec's computed
            // value for an inset does.
            return match declared {
                crate::types::CssLength::Auto => "auto".to_string(),
                crate::types::CssLength::Percent(p) => format!("{p}%"),
                other => format!(
                    "{}px",
                    other.resolve(font_px, self.viewport_w, self.root_font_px())
                ),
            };
        }
        if matches!(
            property.as_str(),
            "margin"
                | "padding"
                | "border"
                | "outline"
                | "overflow"
                | "flex"
                | "inset"
                | "gap"
                | "columns"
                | "column-rule"
        ) {
            return self.resolved_shorthand(id, &property);
        }
        let resolved = match (property.as_str(), rect) {
            // A positioned box's inset IS its used value — the offset from the
            // containing block, not from the page. Same number whenever that
            // block is the initial one, which is why this went unnoticed.
            ("left", Some(r)) => Some(format!("{}px", r.x - self.containing_origin(id).0)),
            ("top", Some(r)) => Some(format!("{}px", r.y - self.containing_origin(id).1)),
            // ⛔ A box only HAS a used width if it generates one. A
            // non-replaced `display: inline` box does not, and neither does a
            // `display: none` box — Chrome answers `"auto"` for both, and this
            // answered the rect (`"8.8px"` for a bare `<span>`). A replaced
            // inline is the exception: `<img width=30>` is `"30px"`.
            ("width", Some(r)) if self.has_a_used_size(id) => Some(format!("{}px", r.w)),
            ("height", Some(r)) if self.has_a_used_size(id) => Some(format!("{}px", r.h)),
            ("margin-top", _) => self.get_node(id).map(|n| px(n.layout.resolved_margin_top)),
            ("margin-right", _) => self
                .get_node(id)
                .map(|n| px(n.layout.resolved_margin_right)),
            ("margin-bottom", _) => self
                .get_node(id)
                .map(|n| px(n.layout.resolved_margin_bottom)),
            ("margin-left", _) => self.get_node(id).map(|n| px(n.layout.resolved_margin_left)),
            ("padding-top", _) => self.get_node(id).map(|n| px(n.layout.resolved_pad_top)),
            ("padding-right", _) => self.get_node(id).map(|n| px(n.layout.resolved_pad_right)),
            ("padding-bottom", _) => self.get_node(id).map(|n| px(n.layout.resolved_pad_bottom)),
            ("padding-left", _) => self.get_node(id).map(|n| px(n.layout.resolved_pad_left)),
            _ => None,
        };
        match resolved {
            Some(v) => v,
            // The CASCADED value, before the inline fallback. Without this
            // step `getComputedStyle(el).position` answered `""` for a value
            // that came from a stylesheet — the accessor read the inline style
            // and nothing else, while the right answer sat in `ComputedStyle`
            // the whole time.
            None => match self.resolved_from_cascade(id, &property) {
                Some(v) => v,
                None => self.get_style_property(id, &property).unwrap_or_default(),
            },
        }
    }

    /// `getComputedStyle(element, pseudo).getPropertyValue(property)` for
    /// generated pseudo-elements whose styles are already cascaded onto the
    /// originating element.
    pub fn computed_style_pseudo_property(
        &mut self,
        id: u32,
        pseudo: &str,
        property: &str,
    ) -> String {
        let width = self.viewport_w;
        crate::layout::LayoutEngine::new().layout(self, width);
        let Some(node) = self.get_node(id).or_else(|| self.find_webcore(id)) else {
            return String::new();
        };
        let pseudo = pseudo.trim().trim_start_matches(':');
        let empty_content = String::new();
        let (style, content) = match pseudo {
            "before" => (
                node.style.before_style.as_deref(),
                &node.style.before_content,
            ),
            "after" => (node.style.after_style.as_deref(), &node.style.after_content),
            "marker" => (
                node.style.marker_style.as_deref(),
                &node.style.marker_content,
            ),
            "placeholder" => (node.style.placeholder_style.as_deref(), &empty_content),
            "selection" => (node.style.selection_style.as_deref(), &empty_content),
            "backdrop" => (node.style.backdrop_style.as_deref(), &empty_content),
            _ => (None, &empty_content),
        };
        let Some(style) = style else {
            return String::new();
        };
        match property.to_ascii_lowercase().as_str() {
            "content" => {
                if content.is_empty() {
                    "none".to_string()
                } else if content.starts_with('"') || content.starts_with('\'') {
                    content.clone()
                } else {
                    format!("\"{}\"", content.replace('"', "\\\""))
                }
            }
            "display" => serialize_display(style.display),
            "color" => serialize_color(style.color),
            "background-color" => serialize_color(style.background_color),
            "font-size" => format!("{}px", style.font_size.resolve(16.0, 16.0, 16.0)),
            "font-weight" => style.font_weight.value().to_string(),
            "font-style" => match style.font_style {
                crate::types::FontStyle::Normal => "normal",
                crate::types::FontStyle::Italic => "italic",
                crate::types::FontStyle::Oblique => "oblique",
            }
            .to_string(),
            "font-family" => serialize_font_family_list(&style.font_family),
            _ => String::new(),
        }
    }

    /// Serialize a property from the CASCADED style, Chrome's way.
    ///
    /// `None` means "not in the covered set", and the caller falls back to the
    /// inline style — which is what every property did before this existed.
    /// The set is deliberately explicit rather than open-ended, because a
    /// property that falls through fails SILENTLY: it answers the inline value
    /// or the empty string and looks no different from one that is handled.
    ///
    /// **Not covered, on purpose:**
    ///
    /// * `float` — CSS 2.1 §9.7 computes it to `none` on an absolutely
    ///   positioned box, and Chrome duly answers `"none"` for a `float: left`
    ///   that is also `position: fixed`. That is a COMPUTED-VALUE rule and the
    ///   cascade does not apply it, so answering `"none"` here would leave
    ///   `get_computed_style(id).float` saying `Left` while this said
    ///   `"none"` — two readers of one fact disagreeing. It belongs in the
    ///   cascade.
    /// * the shorthands (`margin`, `border`, `font`, …), which serialize from
    ///   several longhands at once.
    fn resolved_from_cascade(&self, id: u32, property: &str) -> Option<String> {
        use crate::types::*;
        let s = self.get_computed_style(id)?;
        let font_px = s.font_size.resolve(16.0, 16.0, 16.0);
        let vw = self.viewport_w;
        let root_px = self.root_font_px();
        let len = |l: &CssLength| -> String {
            match l {
                CssLength::Auto => "auto".to_string(),
                // The `max-*` initial value is its own variant, and it
                // serializes as the keyword — `resolve` would answer `0px`,
                // which is the opposite of "no limit".
                CssLength::None => "none".to_string(),
                CssLength::Percent(p) => format!("{p}%"),
                other => format!("{}px", other.resolve(font_px, vw, root_px)),
            }
        };
        let is_flex_item = self
            .parent_element(id)
            .and_then(|parent| self.get_computed_style(parent))
            .map(|parent| matches!(parent.display, Display::Flex | Display::InlineFlex))
            .unwrap_or(false);
        Some(match property {
            // ⛔ EVERY variant. A `_ => return None` arm here sent the
            // uncovered ones to the inline fallback, so `<button>` — which
            // this crate lays out as a flex box — answered `""` rather than a
            // display at all.
            "display" => match s.display {
                Display::None => "none",
                Display::Block => "block",
                Display::Inline => "inline",
                Display::InlineBlock => "inline-block",
                Display::Flex => "flex",
                Display::InlineFlex => "inline-flex",
                Display::Grid => "grid",
                Display::InlineGrid => "inline-grid",
                Display::Table => "table",
                Display::TableRow => "table-row",
                Display::TableCell => "table-cell",
                Display::TableHeaderCell => "table-cell",
                Display::TableRowGroup => "table-row-group",
                Display::TableHeaderGroup => "table-header-group",
                Display::TableFooterGroup => "table-footer-group",
                Display::TableColumnGroup => "table-column-group",
                Display::TableColumn => "table-column",
                Display::TableCaption => "table-caption",
                Display::ListItem => "list-item",
                Display::Ruby => "ruby",
                Display::RubyText => "ruby-text",
                Display::FlowRoot => "flow-root",
                Display::Contents => "contents",
            }
            .to_string(),
            "position" => match s.position {
                Position::Static => "static",
                Position::Relative => "relative",
                Position::Absolute => "absolute",
                Position::Fixed => "fixed",
                _ => return None,
            }
            .to_string(),
            "color" => serialize_color(s.color),
            "background-color" => serialize_color(s.background_color),
            "fill" => s
                .svg_fill
                .map(serialize_color)
                .unwrap_or_else(|| "none".to_string()),
            "stroke" => s
                .svg_stroke
                .map(serialize_color)
                .unwrap_or_else(|| "none".to_string()),
            "font-size" => format!("{font_px}px"),
            // ⛔ A NUMBER, not the keyword: `font-weight: bold` serializes as
            // `"700"` (measured).
            "font-weight" => s.font_weight.value().to_string(),
            "font-style" => match s.font_style {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
                FontStyle::Oblique => "oblique",
            }
            .to_string(),
            "font-family" => serialize_font_family_list(&s.font_family),
            "text-align" => serialize_text_align(s.text_align),
            "visibility" => {
                if s.visibility {
                    "visible".to_string()
                } else {
                    "hidden".to_string()
                }
            }
            "line-height" => len(&s.line_height),
            "letter-spacing" => len(&s.letter_spacing),
            "text-transform" => serialize_text_transform(s.text_transform),
            "white-space" => serialize_white_space(s.white_space),
            "direction" => match s.direction {
                Direction::LTR => "ltr",
                Direction::RTL => "rtl",
            }
            .to_string(),
            "text-orientation" => match s.text_orientation {
                TextOrientation::Mixed => "mixed",
                TextOrientation::Upright => "upright",
                TextOrientation::Sideways => "sideways",
            }
            .to_string(),
            "text-combine-upright" => s.text_combine_upright.clone(),
            "cursor" => serialize_cursor(s.cursor),
            "margin-top" => len(&s.margin_top),
            "margin-right" => len(&s.margin_right),
            "margin-bottom" => len(&s.margin_bottom),
            "margin-left" => len(&s.margin_left),
            "padding-top" => len(&s.padding_top),
            "padding-right" => len(&s.padding_right),
            "padding-bottom" => len(&s.padding_bottom),
            "padding-left" => len(&s.padding_left),
            // ⛔ The USED width, which is 0 whenever the matching style is
            // `none` — CSSOM resolves border-width to the used value, and the
            // initial COMPUTED width is `medium` (3px), not 0. Answering the
            // computed value here reported 3px for every element that had
            // never mentioned a border. Measured in Chrome: a bare `<div>` is
            // `0px`/`none`, a `<div style="border-style:solid">` is `3px`.
            "border-top-width" => len(&used_border(&s.border_top_width, s.border_top_style)),
            "border-right-width" => len(&used_border(&s.border_right_width, s.border_right_style)),
            "border-bottom-width" => {
                len(&used_border(&s.border_bottom_width, s.border_bottom_style))
            }
            "border-left-width" => len(&used_border(&s.border_left_width, s.border_left_style)),
            "border-top-style" => serialize_border_style(s.border_top_style),
            "border-right-style" => serialize_border_style(s.border_right_style),
            "border-bottom-style" => serialize_border_style(s.border_bottom_style),
            "border-left-style" => serialize_border_style(s.border_left_style),
            "border-top-color" => serialize_color(s.border_top_color),
            "border-right-color" => serialize_color(s.border_right_color),
            "border-bottom-color" => serialize_color(s.border_bottom_color),
            "border-left-color" => serialize_color(s.border_left_color),
            "border-top-left-radius" => {
                radius_pair(&len, &s.border_top_left_radius, &s.border_top_left_radius_y)
            }
            "border-top-right-radius" => radius_pair(
                &len,
                &s.border_top_right_radius,
                &s.border_top_right_radius_y,
            ),
            "border-bottom-right-radius" => radius_pair(
                &len,
                &s.border_bottom_right_radius,
                &s.border_bottom_right_radius_y,
            ),
            "border-bottom-left-radius" => radius_pair(
                &len,
                &s.border_bottom_left_radius,
                &s.border_bottom_left_radius_y,
            ),
            "border-image-source" => s.border_image_source.clone(),
            "border-image-slice" => s.border_image_slice.clone(),
            "border-image-width" => s.border_image_width.clone(),
            "border-image-outset" => s.border_image_outset.clone(),
            "border-image-repeat" => s.border_image_repeat.clone(),
            "border-image" => format!(
                "{} {} / {} / {} {}",
                s.border_image_source,
                s.border_image_slice,
                s.border_image_width,
                s.border_image_outset,
                s.border_image_repeat
            ),
            "outline-width" => format!("{}px", s.outline_width),
            "outline-style" => serialize_border_style(s.outline_style),
            "outline-color" => serialize_color(s.outline_color),
            "outline-offset" => format!("{}px", s.outline_offset),
            "overflow-x" => serialize_overflow(s.overflow_x),
            "overflow-y" => serialize_overflow(s.overflow_y),
            "box-sizing" => match s.box_sizing {
                BoxSizing::ContentBox => "content-box",
                BoxSizing::BorderBox => "border-box",
            }
            .to_string(),
            "clear" => match s.clear {
                Clear::None => "none",
                Clear::Left => "left",
                Clear::Right => "right",
                Clear::Both => "both",
                Clear::InlineStart => "inline-start",
                Clear::InlineEnd => "inline-end",
            }
            .to_string(),
            "width" => len(&s.width),
            "height" => len(&s.height),
            "min-width" => serialize_min(&s.min_width, is_flex_item, &len),
            "min-height" => serialize_min(&s.min_height, is_flex_item, &len),
            // `max-*` has `none` where the others have `auto`.
            "max-width" => len(&s.max_width),
            "max-height" => len(&s.max_height),
            "line-clamp" | "-webkit-line-clamp" => s
                .line_clamp
                .map(|lines| lines.to_string())
                .unwrap_or_else(|| "none".to_string()),
            "opacity" => format!("{}", s.opacity),
            // `transform` serializes as the resolved 2D matrix, never as the
            // author's function list — CSSOM. `none` when there is no
            // transform, which is what Chrome answers for an untransformed
            // element. This returned the empty string for everything, so a
            // page could not read back a transform it had set.
            "transform" => {
                if s.transform.is_empty() {
                    "none".to_string()
                } else {
                    // ⛔ The RAW matrix — CSSOM serializes M itself, with no
                    // `transform-origin` baked in: the origin is a separate
                    // property. Chrome answers `matrix(1, 0, 0, 1, 32, 0)` for
                    // `translateX(2rem)` however the origin is set.
                    //
                    // The reference box IS passed, so a percentage translation
                    // resolves — `translateX(50%)` on a 100px box serializes
                    // as 50, which is what a script reading it back expects.
                    // The reference box is the element's BORDER box, not its
                    // bounding client rect: the latter is the box AFTER the
                    // transform, which would make the percentage depend on its
                    // own result.
                    let (rw, rh) = self
                        .get_node(id)
                        .or_else(|| self.find_webcore(id))
                        .map(|b| (b.layout.border_rect.w, b.layout.border_rect.h))
                        .unwrap_or((0.0, 0.0));
                    let m = crate::renderer::display_list_builder::compute_transform_matrix_raw(
                        &s,
                        rw,
                        rh,
                        &crate::types::TransformCtx {
                            font_px,
                            root_font_px: root_px,
                            viewport_w: vw,
                            viewport_h: self.viewport_h,
                        },
                    );
                    format!(
                        "matrix({}, {}, {}, {}, {}, {})",
                        trim_f32(m[0]),
                        trim_f32(m[1]),
                        trim_f32(m[2]),
                        trim_f32(m[3]),
                        trim_f32(m[4]),
                        trim_f32(m[5])
                    )
                }
            }
            "transform-box" => s.transform_box.clone(),
            "transform-style" => s.transform_style_3d.clone(),
            "perspective" => s.perspective.clone(),
            "perspective-origin" => s.perspective_origin.clone(),
            "backface-visibility" => s.backface_visibility.clone(),
            "z-index" => {
                if s.z_index_is_auto {
                    "auto".to_string()
                } else {
                    s.z_index.to_string()
                }
            }
            "font-variant" => {
                if s.small_caps {
                    "small-caps".to_string()
                } else {
                    "normal".to_string()
                }
            }
            "font-variant-alternates" => s.font_variant_alternates.clone(),
            "font-variant-caps" => s.font_variant_caps.clone(),
            "font-variant-east-asian" => s.font_variant_east_asian.clone(),
            "font-variant-emoji" => s.font_variant_emoji.clone(),
            "font-variant-ligatures" => s.font_variant_ligatures.clone(),
            "font-variant-numeric" => s.font_variant_numeric.clone(),
            "font-variant-position" => s.font_variant_position.clone(),
            "font-synthesis" => serialize_font_synthesis(s),
            "font-synthesis-weight" => serialize_font_synthesis_longhand(s.font_synthesis_weight),
            "font-synthesis-style" => serialize_font_synthesis_longhand(s.font_synthesis_style),
            "font-synthesis-small-caps" => {
                serialize_font_synthesis_longhand(s.font_synthesis_small_caps)
            }
            "font-synthesis-position" => {
                serialize_font_synthesis_longhand(s.font_synthesis_position)
            }
            "text-decoration-skip-ink" => s.text_decoration_skip_ink.clone(),
            "text-overflow" => match s.text_overflow {
                crate::types::TextOverflow::Clip => "clip".to_string(),
                crate::types::TextOverflow::Ellipsis if s.text_overflow_string.is_empty() => {
                    "ellipsis".to_string()
                }
                crate::types::TextOverflow::Ellipsis => {
                    format!("\"{}\"", serialize_css_string(&s.text_overflow_string))
                }
            },
            "text-emphasis-style" => s.text_emphasis_style.clone(),
            "text-emphasis-color" => s
                .text_emphasis_color
                .map(serialize_color)
                .unwrap_or_else(|| "currentcolor".to_string()),
            "text-emphasis-position" => s.text_emphasis_position.clone(),
            "text-emphasis" => {
                let color = s
                    .text_emphasis_color
                    .map(serialize_color)
                    .unwrap_or_else(|| "currentcolor".to_string());
                format!("{} {}", s.text_emphasis_style, color)
            }
            "text-wrap" => s.text_wrap.clone(),
            "list-style-type" => {
                if s.custom_list_style_type.is_empty() {
                    serialize_list_style_type(s.list_style_type)
                } else {
                    s.custom_list_style_type.clone()
                }
            }
            "list-style-position" => serialize_list_style_position(s.list_style_position),
            "list-style-image" => serialize_list_style_image(&s.list_style_image),
            "mask-image" => {
                if s.rare().mask_image_url.is_empty() {
                    "none".to_string()
                } else {
                    format!("url(\"{}\")", s.rare().mask_image_url)
                }
            }
            "mask-mode" => rare_or(&s.rare().mask_mode, "match-source"),
            "mask-repeat" => rare_or(&s.rare().mask_repeat, "repeat"),
            "mask-position" => rare_or(&s.rare().mask_position, "0% 0%"),
            "mask-size" => rare_or(&s.rare().mask_size, "auto"),
            "mask-clip" => rare_or(&s.rare().mask_clip, "border-box"),
            "mask-origin" => rare_or(&s.rare().mask_origin, "border-box"),
            "mask-composite" => rare_or(&s.rare().mask_composite, "add"),
            "mask" => serialize_mask(s),
            "vertical-align" => serialize_vertical_align(&s.vertical_align),
            "float" => serialize_float(s.float),
            "flex-direction" => serialize_flex_direction(s.flex_direction),
            "justify-content" => serialize_justify_content(s.justify_content),
            "align-items" => serialize_align_items(s.align_items),
            "row-gap" => len(&s.row_gap),
            "column-gap" => len(&s.column_gap),
            "flex-grow" => trim_f32(s.flex_grow),
            "flex-shrink" => trim_f32(s.flex_shrink),
            "flex-basis" => len(&s.flex_basis),
            "order" => s.order.to_string(),
            "content-visibility" => match s.content_visibility {
                ContentVisibility::Visible => "visible",
                ContentVisibility::Auto => "auto",
                ContentVisibility::Hidden => "hidden",
            }
            .to_string(),
            "contain-intrinsic-size" => shorthand_pair_values(
                len(&s.contain_intrinsic_width),
                len(&s.contain_intrinsic_height),
            ),
            "color-scheme" => s.color_scheme.clone(),
            "forced-color-adjust" => s.forced_color_adjust.clone(),
            "background-image" => serialize_background_image(s),
            "background-position" => format!(
                "{} {}",
                len(&s.background_position_x),
                len(&s.background_position_y)
            ),
            "background-size" => serialize_background_size(s),
            "background-repeat" => serialize_background_repeat(s.background_repeat),
            "background-attachment" => serialize_background_attachment(s.background_attachment),
            "background-origin" => serialize_background_clip(s.background_origin),
            "background-clip" => serialize_background_clip(s.background_clip),
            "background-blend-mode" => s.background_blend_mode.clone(),
            "transition" => serialize_transition_shorthand(s),
            "transition-property" => serialize_transition_property(s),
            "transition-duration" => serialize_transition_duration(s),
            "transition-timing-function" => serialize_transition_timing_function(s),
            "transition-delay" => serialize_transition_delay(s),
            "transition-behavior" => serialize_transition_behavior(s),
            "animation" => serialize_animation_shorthand(s),
            "animation-name" => serialize_animation_name(s),
            "animation-duration" => serialize_animation_duration(s),
            "animation-timing-function" => serialize_animation_timing_function(s),
            "animation-delay" => serialize_animation_delay(s),
            "animation-iteration-count" => serialize_animation_iteration_count(s),
            "animation-direction" => serialize_animation_direction(s),
            "animation-fill-mode" => serialize_animation_fill_mode(s),
            "animation-play-state" => serialize_animation_play_state(s),
            "animation-composition" => serialize_animation_composition(s),
            "overflow-anchor" => s.overflow_anchor.clone(),
            "overflow-clip-margin" => s.overflow_clip_margin.clone(),
            "anchor-name" => s.anchor_name.clone(),
            "position-anchor" => s.position_anchor.clone(),
            "view-transition-name" => s.view_transition_name.clone(),
            "animation-timeline" => s.animation_timeline.clone(),
            "scroll-timeline" => s.scroll_timeline.clone(),
            "offset-path" => s.offset_path.clone(),
            "shape-outside" => s.shape_outside.clone(),
            "shape-margin" => len(&s.shape_margin),
            "scrollbar-width" => s.scrollbar_width.clone(),
            "scrollbar-gutter" => s.scrollbar_gutter.clone(),
            "scrollbar-color" => serialize_scrollbar_color(s),
            "scroll-snap-stop" => s.scroll_snap_stop.clone(),
            "scroll-margin-top" => len(&s.scroll_margin_top),
            "scroll-margin-right" => len(&s.scroll_margin_right),
            "scroll-margin-bottom" => len(&s.scroll_margin_bottom),
            "scroll-margin-left" => len(&s.scroll_margin_left),
            "scroll-margin" => shorthand_box_values([
                len(&s.scroll_margin_top),
                len(&s.scroll_margin_right),
                len(&s.scroll_margin_bottom),
                len(&s.scroll_margin_left),
            ]),
            "appearance" => s.appearance.clone(),
            "field-sizing" => s.field_sizing.clone(),
            "interpolate-size" => s.interpolate_size.clone(),
            "margin-trim" => s.margin_trim.clone(),
            "caption-side" => match s.caption_side {
                CaptionSide::Top => "top",
                CaptionSide::Bottom => "bottom",
                CaptionSide::BlockStart => "block-start",
                CaptionSide::BlockEnd => "block-end",
                CaptionSide::InlineStart => "inline-start",
                CaptionSide::InlineEnd => "inline-end",
            }
            .to_string(),
            "text-underline-position" => match s.text_underline_position {
                TextUnderlinePosition::Auto => "auto",
                TextUnderlinePosition::FromFont => "from-font",
                TextUnderlinePosition::Under => "under",
                TextUnderlinePosition::Left => "left",
                TextUnderlinePosition::Right => "right",
            }
            .to_string(),
            "column-count" => s
                .column_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "auto".to_string()),
            "column-width" => len(&s.column_width),
            "column-rule-width" => len(&s.column_rule_width),
            "column-rule-style" => serialize_border_style(s.column_rule_style),
            "column-rule-color" => serialize_color(s.column_rule_color),
            "column-fill" => {
                if s.column_fill {
                    "balance"
                } else {
                    "auto"
                }
            }
            .to_string(),
            "column-span" => {
                if s.column_span_all {
                    "all"
                } else {
                    "none"
                }
            }
            .to_string(),
            _ => return None,
        })
    }
}

/// Zero with the sign stripped. Negating `0.0` yields `-0.0`, which formats
/// as `"-0"` — and a relative box with no offsets answered `"-0px"` for its
/// mirrored edge.
fn px(v: f32) -> String {
    format!("{}px", if v == 0.0 { 0.0 } else { v })
}

fn serialize_computed_length(l: &crate::types::CssLength) -> String {
    match l {
        crate::types::CssLength::Percent(p) => format!("{p}%"),
        crate::types::CssLength::Auto => "auto".to_string(),
        crate::types::CssLength::None => "none".to_string(),
        other => px(other.resolve(16.0, 0.0, 16.0)),
    }
}

fn serialize_css_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn radius_pair<F>(len: &F, x: &crate::types::CssLength, y: &crate::types::CssLength) -> String
where
    F: Fn(&crate::types::CssLength) -> String,
{
    let x = len(x);
    let y = len(y);
    if x == y {
        x
    } else {
        format!("{x} {y}")
    }
}

/// `rgb(r, g, b)` when opaque, `rgba(r, g, b, a)` otherwise — CSSOM's own
/// serialization, and what Chrome answers. A fully transparent colour is
/// `"rgba(0, 0, 0, 0)"`, never the keyword `transparent` (measured).
fn serialize_color(c: crate::types::Color) -> String {
    if c.a == 255 {
        format!("rgb({}, {}, {})", c.r, c.g, c.b)
    } else {
        let alpha = (c.a as f32 / 255.0 * 100.0).round() / 100.0;
        format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, alpha)
    }
}

fn serialize_scrollbar_color(s: &crate::types::ComputedStyle) -> String {
    match (s.scrollbar_thumb_color, s.scrollbar_track_color) {
        (Some(thumb), Some(track)) => {
            format!("{} {}", serialize_color(thumb), serialize_color(track))
        }
        _ => "auto".to_string(),
    }
}

fn serialize_background_image(s: &crate::types::ComputedStyle) -> String {
    if !s.background_image_url.is_empty() {
        return format!(
            "url(\"{}\")",
            s.background_image_url
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
        );
    }
    if s.gradient_type != crate::types::GradientType::None {
        return match s.gradient_type {
            crate::types::GradientType::Linear => "linear-gradient(...)",
            crate::types::GradientType::Radial => "radial-gradient(...)",
            crate::types::GradientType::None => "none",
        }
        .to_string();
    }
    "none".to_string()
}

fn serialize_background_size(s: &crate::types::ComputedStyle) -> String {
    match s.background_size {
        crate::types::BackgroundSize::Auto => "auto".to_string(),
        crate::types::BackgroundSize::Cover => "cover".to_string(),
        crate::types::BackgroundSize::Contain => "contain".to_string(),
        crate::types::BackgroundSize::Explicit => {
            let width = serialize_computed_length(&s.background_size_w);
            let height = serialize_computed_length(&s.background_size_h);
            if height == "auto" {
                width
            } else {
                format!("{width} {height}")
            }
        }
    }
}

fn serialize_background_repeat(repeat: crate::types::BackgroundRepeat) -> String {
    fn axis(axis: crate::types::BackgroundRepeatAxis) -> &'static str {
        match axis {
            crate::types::BackgroundRepeatAxis::Repeat => "repeat",
            crate::types::BackgroundRepeatAxis::NoRepeat => "no-repeat",
            crate::types::BackgroundRepeatAxis::Space => "space",
            crate::types::BackgroundRepeatAxis::Round => "round",
        }
    }

    match repeat {
        crate::types::BackgroundRepeat::Repeat => "repeat".to_string(),
        crate::types::BackgroundRepeat::RepeatX => "repeat-x".to_string(),
        crate::types::BackgroundRepeat::RepeatY => "repeat-y".to_string(),
        crate::types::BackgroundRepeat::NoRepeat => "no-repeat".to_string(),
        crate::types::BackgroundRepeat::Space => "space".to_string(),
        crate::types::BackgroundRepeat::Round => "round".to_string(),
        crate::types::BackgroundRepeat::TwoValue(x, y) => format!("{} {}", axis(x), axis(y)),
    }
}

fn serialize_background_clip(clip: crate::types::BackgroundClip) -> String {
    match clip {
        crate::types::BackgroundClip::BorderBox => "border-box",
        crate::types::BackgroundClip::PaddingBox => "padding-box",
        crate::types::BackgroundClip::ContentBox => "content-box",
        crate::types::BackgroundClip::Text => "text",
    }
    .to_string()
}

fn serialize_background_attachment(attachment: crate::types::BackgroundAttachment) -> String {
    match attachment {
        crate::types::BackgroundAttachment::Scroll => "scroll",
        crate::types::BackgroundAttachment::Fixed => "fixed",
        crate::types::BackgroundAttachment::Local => "local",
    }
    .to_string()
}

fn serialize_display(display: crate::types::Display) -> String {
    match display {
        crate::types::Display::None => "none",
        crate::types::Display::Block => "block",
        crate::types::Display::Inline => "inline",
        crate::types::Display::InlineBlock => "inline-block",
        crate::types::Display::Flex => "flex",
        crate::types::Display::InlineFlex => "inline-flex",
        crate::types::Display::Grid => "grid",
        crate::types::Display::InlineGrid => "inline-grid",
        crate::types::Display::Table => "table",
        crate::types::Display::TableRow => "table-row",
        crate::types::Display::TableCell | crate::types::Display::TableHeaderCell => "table-cell",
        crate::types::Display::TableRowGroup => "table-row-group",
        crate::types::Display::TableHeaderGroup => "table-header-group",
        crate::types::Display::TableFooterGroup => "table-footer-group",
        crate::types::Display::TableColumnGroup => "table-column-group",
        crate::types::Display::TableColumn => "table-column",
        crate::types::Display::TableCaption => "table-caption",
        crate::types::Display::ListItem => "list-item",
        crate::types::Display::Ruby => "ruby",
        crate::types::Display::RubyText => "ruby-text",
        crate::types::Display::FlowRoot => "flow-root",
        crate::types::Display::Contents => "contents",
    }
    .to_string()
}

fn serialize_border_style(v: crate::types::BorderStyle) -> String {
    use crate::types::BorderStyle as B;
    match v {
        B::None => "none",
        B::Hidden => "hidden",
        B::Solid => "solid",
        B::Dashed => "dashed",
        B::Dotted => "dotted",
        B::Double => "double",
        B::Groove => "groove",
        B::Ridge => "ridge",
        B::Inset => "inset",
        B::Outset => "outset",
    }
    .to_string()
}

fn serialize_font_family_list(value: &str) -> String {
    crate::css::split_font_families(value)
        .into_iter()
        .map(|family| serialize_font_family_name(&family))
        .collect::<Vec<_>>()
        .join(", ")
}

fn serialize_font_family_name(name: &str) -> String {
    if is_css_family_identifier(name) {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn serialize_font_synthesis(s: &crate::types::ComputedStyle) -> String {
    if s.font_synthesis_weight
        && s.font_synthesis_style
        && s.font_synthesis_small_caps
        && s.font_synthesis_position
    {
        return "auto".to_string();
    }
    if !s.font_synthesis_weight
        && !s.font_synthesis_style
        && !s.font_synthesis_small_caps
        && !s.font_synthesis_position
    {
        return "none".to_string();
    }
    let mut parts = Vec::new();
    if s.font_synthesis_weight {
        parts.push("weight");
    }
    if s.font_synthesis_style {
        parts.push("style");
    }
    if s.font_synthesis_small_caps {
        parts.push("small-caps");
    }
    if s.font_synthesis_position {
        parts.push("position");
    }
    parts.join(" ")
}

fn serialize_font_synthesis_longhand(enabled: bool) -> String {
    if enabled { "auto" } else { "none" }.to_string()
}

fn rare_or(value: &str, initial: &str) -> String {
    if value.is_empty() {
        initial.to_string()
    } else {
        value.to_string()
    }
}

fn serialize_mask(s: &crate::types::ComputedStyle) -> String {
    let image = if s.rare().mask_image_url.is_empty() {
        "none".to_string()
    } else {
        format!("url(\"{}\")", s.rare().mask_image_url)
    };
    format!(
        "{} {} {} / {} {} {} {}",
        image,
        rare_or(&s.rare().mask_position, "0% 0%"),
        rare_or(&s.rare().mask_repeat, "repeat"),
        rare_or(&s.rare().mask_size, "auto"),
        rare_or(&s.rare().mask_origin, "border-box"),
        rare_or(&s.rare().mask_clip, "border-box"),
        rare_or(&s.rare().mask_mode, "match-source"),
    )
}

fn serialize_transition_property(s: &crate::types::ComputedStyle) -> String {
    join_transition_values(s, |tr| tr.property.clone(), "all")
}

fn serialize_transition_shorthand(s: &crate::types::ComputedStyle) -> String {
    if s.rare().transitions.is_empty() {
        return "all 0s ease 0s normal".to_string();
    }
    s.rare()
        .transitions
        .iter()
        .map(|tr| {
            format!(
                "{} {} {} {} {}",
                tr.property,
                serialize_time_ms(tr.duration_ms),
                serialize_easing(&tr.timing_fn),
                serialize_time_ms(tr.delay_ms),
                if tr.allow_discrete {
                    "allow-discrete"
                } else {
                    "normal"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn serialize_transition_duration(s: &crate::types::ComputedStyle) -> String {
    join_transition_values(s, |tr| serialize_time_ms(tr.duration_ms), "0s")
}

fn serialize_transition_timing_function(s: &crate::types::ComputedStyle) -> String {
    join_transition_values(s, |tr| serialize_easing(&tr.timing_fn), "ease")
}

fn serialize_transition_delay(s: &crate::types::ComputedStyle) -> String {
    join_transition_values(s, |tr| serialize_time_ms(tr.delay_ms), "0s")
}

fn serialize_transition_behavior(s: &crate::types::ComputedStyle) -> String {
    join_transition_values(
        s,
        |tr| {
            if tr.allow_discrete {
                "allow-discrete".to_string()
            } else {
                "normal".to_string()
            }
        },
        "normal",
    )
}

fn join_transition_values<F>(s: &crate::types::ComputedStyle, map: F, initial: &str) -> String
where
    F: Fn(&crate::types::ParsedTransition) -> String,
{
    let values: Vec<_> = s.rare().transitions.iter().map(map).collect();
    if values.is_empty() {
        initial.to_string()
    } else {
        values.join(", ")
    }
}

fn serialize_animation_name(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(s, |anim| anim.name.clone(), "none")
}

fn serialize_animation_shorthand(s: &crate::types::ComputedStyle) -> String {
    if s.rare().animations.is_empty() {
        return "none 0s ease 0s 1 normal none running replace".to_string();
    }
    s.rare()
        .animations
        .iter()
        .map(|anim| {
            format!(
                "{} {} {} {} {} {} {} {} {}",
                anim.name,
                serialize_time_ms(anim.duration_ms),
                serialize_easing(&anim.timing_fn),
                serialize_time_ms(anim.delay_ms),
                if anim.iteration_count.is_infinite() {
                    "infinite".to_string()
                } else {
                    trim_f32(anim.iteration_count)
                },
                match anim.direction {
                    crate::types::AnimDirection::Normal => "normal",
                    crate::types::AnimDirection::Reverse => "reverse",
                    crate::types::AnimDirection::Alternate => "alternate",
                    crate::types::AnimDirection::AlternateReverse => "alternate-reverse",
                },
                match anim.fill_mode {
                    crate::types::FillMode::None => "none",
                    crate::types::FillMode::Forwards => "forwards",
                    crate::types::FillMode::Backwards => "backwards",
                    crate::types::FillMode::Both => "both",
                },
                if anim.play_state_paused {
                    "paused"
                } else {
                    "running"
                },
                match anim.composition {
                    crate::types::AnimationComposition::Replace => "replace",
                    crate::types::AnimationComposition::Add => "add",
                    crate::types::AnimationComposition::Accumulate => "accumulate",
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn serialize_animation_duration(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(s, |anim| serialize_time_ms(anim.duration_ms), "0s")
}

fn serialize_animation_timing_function(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(s, |anim| serialize_easing(&anim.timing_fn), "ease")
}

fn serialize_animation_delay(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(s, |anim| serialize_time_ms(anim.delay_ms), "0s")
}

fn serialize_animation_iteration_count(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(
        s,
        |anim| {
            if anim.iteration_count.is_infinite() {
                "infinite".to_string()
            } else {
                trim_f32(anim.iteration_count)
            }
        },
        "1",
    )
}

fn serialize_animation_direction(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(
        s,
        |anim| {
            match anim.direction {
                crate::types::AnimDirection::Normal => "normal",
                crate::types::AnimDirection::Reverse => "reverse",
                crate::types::AnimDirection::Alternate => "alternate",
                crate::types::AnimDirection::AlternateReverse => "alternate-reverse",
            }
            .to_string()
        },
        "normal",
    )
}

fn serialize_animation_fill_mode(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(
        s,
        |anim| {
            match anim.fill_mode {
                crate::types::FillMode::None => "none",
                crate::types::FillMode::Forwards => "forwards",
                crate::types::FillMode::Backwards => "backwards",
                crate::types::FillMode::Both => "both",
            }
            .to_string()
        },
        "none",
    )
}

fn serialize_animation_play_state(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(
        s,
        |anim| {
            if anim.play_state_paused {
                "paused".to_string()
            } else {
                "running".to_string()
            }
        },
        "running",
    )
}

fn serialize_animation_composition(s: &crate::types::ComputedStyle) -> String {
    join_animation_values(
        s,
        |anim| {
            match anim.composition {
                crate::types::AnimationComposition::Replace => "replace",
                crate::types::AnimationComposition::Add => "add",
                crate::types::AnimationComposition::Accumulate => "accumulate",
            }
            .to_string()
        },
        "replace",
    )
}

fn join_animation_values<F>(s: &crate::types::ComputedStyle, map: F, initial: &str) -> String
where
    F: Fn(&crate::types::ParsedAnimation) -> String,
{
    let values: Vec<_> = s.rare().animations.iter().map(map).collect();
    if values.is_empty() {
        initial.to_string()
    } else {
        values.join(", ")
    }
}

fn serialize_time_ms(ms: f32) -> String {
    if (ms / 1000.0).fract().abs() < f32::EPSILON {
        format!("{}s", trim_f32(ms / 1000.0))
    } else {
        format!("{}ms", trim_f32(ms))
    }
}

fn serialize_easing(easing: &crate::types::EasingFn) -> String {
    match easing {
        crate::types::EasingFn::Linear => "linear".to_string(),
        crate::types::EasingFn::Ease => "ease".to_string(),
        crate::types::EasingFn::EaseIn => "ease-in".to_string(),
        crate::types::EasingFn::EaseOut => "ease-out".to_string(),
        crate::types::EasingFn::EaseInOut => "ease-in-out".to_string(),
        crate::types::EasingFn::CubicBezier(a, b, c, d) => format!(
            "cubic-bezier({}, {}, {}, {})",
            trim_f32(*a),
            trim_f32(*b),
            trim_f32(*c),
            trim_f32(*d)
        ),
        crate::types::EasingFn::StepStart => "step-start".to_string(),
        crate::types::EasingFn::StepEnd => "step-end".to_string(),
        crate::types::EasingFn::Steps(count, position) => {
            format!("steps({}, {})", count, serialize_step_position(*position))
        }
        crate::types::EasingFn::LinearPoints(points) => {
            let points = points
                .iter()
                .map(|(input, output)| format!("{} {}", trim_f32(*output), trim_f32(*input)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("linear({points})")
        }
    }
}

fn serialize_step_position(position: crate::types::StepPosition) -> &'static str {
    match position {
        crate::types::StepPosition::JumpStart => "jump-start",
        crate::types::StepPosition::JumpEnd => "jump-end",
        crate::types::StepPosition::JumpNone => "jump-none",
        crate::types::StepPosition::JumpBoth => "jump-both",
    }
}

fn is_css_family_identifier(name: &str) -> bool {
    const GENERICS: &[&str] = &[
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "ui-serif",
        "ui-sans-serif",
        "ui-monospace",
        "ui-rounded",
        "emoji",
        "math",
        "fangsong",
    ];
    if GENERICS.contains(&name) {
        return true;
    }
    name.split('-').all(|part| {
        let mut chars = part.chars();
        matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    })
}

fn serialize_text_align(v: crate::types::TextAlign) -> String {
    use crate::types::TextAlign as T;
    match v {
        T::Left => "left",
        T::Right => "right",
        T::Center => "center",
        T::Justify => "justify",
        T::Start => "start",
        T::End => "end",
    }
    .to_string()
}

fn serialize_text_transform(v: crate::types::TextTransform) -> String {
    use crate::types::TextTransform as T;
    match v {
        T::None => "none",
        T::Uppercase => "uppercase",
        T::Lowercase => "lowercase",
        T::Capitalize => "capitalize",
        T::FullWidth => "full-width",
        T::FullSizeKana => "full-size-kana",
        T::MathAuto => "math-auto",
    }
    .to_string()
}

fn serialize_white_space(v: crate::types::WhiteSpace) -> String {
    use crate::types::WhiteSpace as W;
    match v {
        W::Normal => "normal",
        W::Nowrap => "nowrap",
        W::Pre => "pre",
        W::PreWrap => "pre-wrap",
        W::PreLine => "pre-line",
    }
    .to_string()
}

fn serialize_vertical_align(v: &crate::types::VerticalAlign) -> String {
    use crate::types::VerticalAlign as V;
    match v {
        V::Baseline => "baseline".to_string(),
        V::Top => "top".to_string(),
        V::Middle => "middle".to_string(),
        V::Bottom => "bottom".to_string(),
        V::TextTop => "text-top".to_string(),
        V::TextBottom => "text-bottom".to_string(),
        V::Sub => "sub".to_string(),
        V::Super => "super".to_string(),
        V::Length(l) => serialize_computed_length(l),
    }
}

fn serialize_float(v: crate::types::Float) -> String {
    use crate::types::Float as F;
    match v {
        F::None => "none",
        F::Left => "left",
        F::Right => "right",
        F::InlineStart => "inline-start",
        F::InlineEnd => "inline-end",
    }
    .to_string()
}

fn serialize_list_style_type(v: crate::types::ListStyleType) -> String {
    use crate::types::ListStyleType as L;
    match v {
        L::None => "none",
        L::Disc => "disc",
        L::Circle => "circle",
        L::Square => "square",
        L::Decimal => "decimal",
        L::DecimalLeadingZero => "decimal-leading-zero",
        L::LowerAlpha => "lower-alpha",
        L::UpperAlpha => "upper-alpha",
        L::LowerLatin => "lower-latin",
        L::UpperLatin => "upper-latin",
        L::LowerRoman => "lower-roman",
        L::UpperRoman => "upper-roman",
        L::LowerGreek => "lower-greek",
        L::Armenian => "armenian",
        L::Georgian => "georgian",
        L::Hebrew => "hebrew",
        L::Hiragana => "hiragana",
        L::Katakana => "katakana",
        L::HiraganaIroha => "hiragana-iroha",
        L::KatakanaIroha => "katakana-iroha",
        L::CjkDecimal => "cjk-decimal",
        L::Disclosure => "disclosure-open",
    }
    .to_string()
}

fn serialize_list_style_position(v: crate::types::ListStylePosition) -> String {
    match v {
        crate::types::ListStylePosition::Outside => "outside",
        crate::types::ListStylePosition::Inside => "inside",
    }
    .to_string()
}

fn serialize_list_style_image(url: &str) -> String {
    if url.is_empty() {
        "none".to_string()
    } else {
        format!(
            "url(\"{}\")",
            url.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }
}

fn serialize_flex_direction(v: crate::types::FlexDirection) -> String {
    use crate::types::FlexDirection as F;
    match v {
        F::Row => "row",
        F::RowReverse => "row-reverse",
        F::Column => "column",
        F::ColumnReverse => "column-reverse",
    }
    .to_string()
}

fn serialize_justify_content(v: crate::types::JustifyContent) -> String {
    use crate::types::JustifyContent as J;
    match v {
        J::FlexStart => "flex-start",
        J::FlexEnd => "flex-end",
        J::Center => "center",
        J::SpaceBetween => "space-between",
        J::SpaceAround => "space-around",
        J::SpaceEvenly => "space-evenly",
        J::Left => "left",
        J::Right => "right",
    }
    .to_string()
}

fn serialize_align_items(v: crate::types::AlignItems) -> String {
    use crate::types::AlignItems as A;
    match v {
        A::Stretch => "stretch",
        A::FlexStart => "flex-start",
        A::FlexEnd => "flex-end",
        A::Center => "center",
        A::Baseline => "baseline",
        A::LastBaseline => "last baseline",
    }
    .to_string()
}

fn serialize_cursor(v: crate::types::CSSCursor) -> String {
    use crate::types::CSSCursor as C;
    match v {
        C::Auto => "auto",
        C::Default => "default",
        C::Pointer => "pointer",
        C::Text => "text",
        C::Move => "move",
        C::Crosshair => "crosshair",
        C::Wait => "wait",
        C::Help => "help",
        C::NotAllowed => "not-allowed",
        C::Grab => "grab",
        C::Grabbing => "grabbing",
        C::Copy => "copy",
        C::Cell => "cell",
        C::ContextMenu => "context-menu",
        C::AllScroll => "all-scroll",
        C::ZoomIn => "zoom-in",
        C::ZoomOut => "zoom-out",
        C::ColResize => "col-resize",
        C::RowResize => "row-resize",
        C::NResize => "n-resize",
        C::EResize => "e-resize",
        C::SResize => "s-resize",
        C::WResize => "w-resize",
        C::NEResize => "ne-resize",
        C::NWResize => "nw-resize",
        C::SEResize => "se-resize",
        C::SWResize => "sw-resize",
        C::None => "none",
    }
    .to_string()
}

/// `min-width` / `min-height`: `auto` is zero outside a flex item.
fn serialize_min(
    l: &crate::types::CssLength,
    is_flex_item: bool,
    len: &dyn Fn(&crate::types::CssLength) -> String,
) -> String {
    match l {
        crate::types::CssLength::Auto | crate::types::CssLength::None if is_flex_item => {
            "auto".to_string()
        }
        crate::types::CssLength::Auto | crate::types::CssLength::None => "0px".to_string(),
        other => len(other),
    }
}

fn serialize_overflow(v: crate::types::Overflow) -> String {
    use crate::types::Overflow as O;
    match v {
        O::Visible => "visible",
        O::Hidden => "hidden",
        O::Clip => "clip",
        O::Scroll => "scroll",
        O::Auto => "auto",
    }
    .to_string()
}

impl Document {
    /// Does this element generate a box with a USED width and height?
    ///
    /// A non-replaced `display: inline` box does not — its width is whatever
    /// its content happens to occupy, and CSSOM resolves `width` to the
    /// computed value (`auto`) rather than to a measurement. A `display: none`
    /// box has no measurement at all. Replaced inlines DO have one, which is
    /// why `<img width=30>` answers `"30px"` while a `<span>` answers `"auto"`
    /// (both measured).
    fn has_a_used_size(&self, id: u32) -> bool {
        const REPLACED: &[&str] = &[
            "img", "video", "canvas", "iframe", "embed", "object", "input", "select", "textarea",
            "button", "svg",
        ];
        let Some(style) = self.get_computed_style(id) else {
            return false;
        };
        match style.display {
            crate::types::Display::None => false,
            crate::types::Display::Inline => {
                self.tag_name(id).is_some_and(|t| REPLACED.contains(&t))
            }
            _ => true,
        }
    }

    fn resolved_shorthand(&mut self, id: u32, property: &str) -> String {
        match property {
            "margin" => shorthand_box_values([
                self.computed_style_property(id, "margin-top"),
                self.computed_style_property(id, "margin-right"),
                self.computed_style_property(id, "margin-bottom"),
                self.computed_style_property(id, "margin-left"),
            ]),
            "padding" => shorthand_box_values([
                self.computed_style_property(id, "padding-top"),
                self.computed_style_property(id, "padding-right"),
                self.computed_style_property(id, "padding-bottom"),
                self.computed_style_property(id, "padding-left"),
            ]),
            "overflow" => shorthand_pair_values(
                self.computed_style_property(id, "overflow-x"),
                self.computed_style_property(id, "overflow-y"),
            ),
            "gap" => shorthand_pair_values(
                self.computed_style_property(id, "row-gap"),
                self.computed_style_property(id, "column-gap"),
            ),
            "inset" => shorthand_box_values([
                self.computed_style_property(id, "top"),
                self.computed_style_property(id, "right"),
                self.computed_style_property(id, "bottom"),
                self.computed_style_property(id, "left"),
            ]),
            "flex" => format!(
                "{} {} {}",
                self.computed_style_property(id, "flex-grow"),
                self.computed_style_property(id, "flex-shrink"),
                self.computed_style_property(id, "flex-basis")
            ),
            "border" => {
                let width = self.computed_style_property(id, "border-top-width");
                let style = self.computed_style_property(id, "border-top-style");
                let color = self.computed_style_property(id, "border-top-color");
                let same_width = ["right", "bottom", "left"].iter().all(|side| {
                    self.computed_style_property(id, &format!("border-{side}-width")) == width
                });
                let same_style = ["right", "bottom", "left"].iter().all(|side| {
                    self.computed_style_property(id, &format!("border-{side}-style")) == style
                });
                let same_color = ["right", "bottom", "left"].iter().all(|side| {
                    self.computed_style_property(id, &format!("border-{side}-color")) == color
                });
                if same_width && same_style && same_color {
                    format!("{width} {style} {color}")
                } else {
                    String::new()
                }
            }
            "outline" => format!(
                "{} {} {}",
                self.computed_style_property(id, "outline-width"),
                self.computed_style_property(id, "outline-style"),
                self.computed_style_property(id, "outline-color")
            ),
            "columns" => shorthand_pair_values(
                self.computed_style_property(id, "column-width"),
                self.computed_style_property(id, "column-count"),
            ),
            "column-rule" => format!(
                "{} {} {}",
                self.computed_style_property(id, "column-rule-width"),
                self.computed_style_property(id, "column-rule-style"),
                self.computed_style_property(id, "column-rule-color")
            ),
            _ => String::new(),
        }
    }
}

fn shorthand_box_values(values: [String; 4]) -> String {
    let [top, right, bottom, left] = values;
    if top == right && top == bottom && top == left {
        top
    } else if top == bottom && right == left {
        format!("{top} {right}")
    } else if right == left {
        format!("{top} {right} {bottom}")
    } else {
        format!("{top} {right} {bottom} {left}")
    }
}

fn shorthand_pair_values(first: String, second: String) -> String {
    if first == second {
        first
    } else {
        format!("{first} {second}")
    }
}

/// The used value of a border width: zero when the side draws nothing.
///
/// CSS Backgrounds 3 §4.3 — a border whose style is `none` or `hidden` has a
/// used width of zero however wide it computes.
fn used_border(
    w: &crate::types::CssLength,
    style: crate::types::BorderStyle,
) -> crate::types::CssLength {
    use crate::types::BorderStyle;
    match style {
        BorderStyle::None | BorderStyle::Hidden => crate::types::CssLength::Px(0.0),
        _ => w.clone(),
    }
}

/// A float as CSS serializes it — no trailing `.0`.
fn trim_f32(v: f32) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}
