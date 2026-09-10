pub mod block;
pub mod constraints;
pub mod flex;
pub mod grid;
pub mod inline_layout;
pub mod perf;
pub mod text;

use std::collections::HashMap;
pub mod hit_test;
pub mod layout_box;
pub mod table;

pub use constraints::{Constraints, FormattingContext, IntrinsicSizes};

use crate::types::*;
use std::cell::Cell;

pub(crate) fn establishes_positioned_containing_block(style: &ComputedStyle) -> bool {
    let transform = style.transform.trim();
    let has_active_transform = !transform.is_empty() && !transform.eq_ignore_ascii_case("none");
    let filter = style.rare().filter.trim();
    let has_active_filter = !filter.is_empty() && !filter.eq_ignore_ascii_case("none");
    let backdrop_filter = style.rare().backdrop_filter.trim();
    let has_active_backdrop_filter =
        !backdrop_filter.is_empty() && !backdrop_filter.eq_ignore_ascii_case("none");

    !matches!(style.position, Position::Static)
        || has_active_transform
        || has_active_filter
        || has_active_backdrop_filter
        || style.will_change_transform
        || style.contain_layout
        || style.contain_paint
}

pub(crate) fn establishes_fixed_positioned_containing_block(style: &ComputedStyle) -> bool {
    let transform = style.transform.trim();
    let has_active_transform = !transform.is_empty() && !transform.eq_ignore_ascii_case("none");
    let filter = style.rare().filter.trim();
    let has_active_filter = !filter.is_empty() && !filter.eq_ignore_ascii_case("none");
    let backdrop_filter = style.rare().backdrop_filter.trim();
    let has_active_backdrop_filter =
        !backdrop_filter.is_empty() && !backdrop_filter.eq_ignore_ascii_case("none");

    has_active_transform
        || has_active_filter
        || has_active_backdrop_filter
        || style.will_change_transform
        || style.contain_layout
        || style.contain_paint
}

pub(crate) fn update_scroll_extents_from_children(
    node: &mut WebCore,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
) {
    if matches!(node.style.overflow_x, Overflow::Scroll | Overflow::Auto)
        || matches!(node.style.overflow_y, Overflow::Scroll | Overflow::Auto)
    {
        let natural_scroll_w = node
            .children
            .iter()
            .filter(|child| !matches!(child.style.display, Display::None))
            .map(|child| child.layout.margin_rect.x + child.layout.margin_rect.w - content_x)
            .fold(content_w, f32::max);
        let natural_scroll_h = node
            .children
            .iter()
            .filter(|child| !matches!(child.style.display, Display::None))
            .map(|child| child.layout.margin_rect.y + child.layout.margin_rect.h - content_y)
            .fold(content_h, f32::max);
        node.layout.scroll_width = natural_scroll_w;
        node.layout.scroll_height = natural_scroll_h;
        let max_scroll_x = (node.layout.scroll_width - content_w).max(0.0);
        let max_scroll_y = (node.layout.scroll_height - content_h).max(0.0);
        node.layout.scroll_left = node.layout.scroll_left.min(max_scroll_x).max(0.0);
        node.layout.scroll_top = node.layout.scroll_top.min(max_scroll_y).max(0.0);
    } else {
        node.layout.scroll_width = content_w;
        node.layout.scroll_height = content_h;
        node.layout.scroll_left = 0.0;
        node.layout.scroll_top = 0.0;
    }
}

// ─── Font loading helpers ──────────────────────────────────────────────────────

/// Load raw font bytes into the font system, with format detection.
fn load_font_bytes(fs: &mut cosmic_text::FontSystem, data: Vec<u8>) -> Vec<fontdb::ID> {
    let font_data = if data.starts_with(&crate::woff::WOFF2_MAGIC)
        || data.starts_with(&crate::woff::WOFF1_MAGIC)
    {
        match crate::woff::decode(&data) {
            Some(sfnt) => sfnt,
            None => return Vec::new(),
        }
    } else {
        data
    };
    fs.db_mut()
        .load_font_source(fontdb::Source::Binary(std::sync::Arc::new(font_data)))
        .into_iter()
        .collect()
}

fn css_font_family_name(raw: &str) -> Option<String> {
    let name = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_font_face_weight(raw: Option<&str>) -> Option<fontdb::Weight> {
    let first = raw?.split_whitespace().next()?.trim().to_ascii_lowercase();
    match first.as_str() {
        "normal" => Some(fontdb::Weight::NORMAL),
        "bold" => Some(fontdb::Weight::BOLD),
        _ => first
            .parse::<u16>()
            .ok()
            .filter(|n| (1..=1000).contains(n))
            .map(fontdb::Weight),
    }
}

fn parse_font_face_style(raw: Option<&str>) -> Option<fontdb::Style> {
    let first = raw?.split_whitespace().next()?.trim().to_ascii_lowercase();
    match first.as_str() {
        "normal" => Some(fontdb::Style::Normal),
        "italic" => Some(fontdb::Style::Italic),
        "oblique" => Some(fontdb::Style::Oblique),
        _ => None,
    }
}

fn parse_font_face_stretch(raw: Option<&str>) -> Option<fontdb::Stretch> {
    let first = raw?.split_whitespace().next()?.trim().to_ascii_lowercase();
    if let Some(percent) = first.strip_suffix('%') {
        return percent
            .parse::<f32>()
            .ok()
            .map(crate::layout::inline_layout::stretch_from_percent);
    }
    match first.as_str() {
        "ultra-condensed" => Some(fontdb::Stretch::UltraCondensed),
        "extra-condensed" => Some(fontdb::Stretch::ExtraCondensed),
        "condensed" => Some(fontdb::Stretch::Condensed),
        "semi-condensed" => Some(fontdb::Stretch::SemiCondensed),
        "normal" => Some(fontdb::Stretch::Normal),
        "semi-expanded" => Some(fontdb::Stretch::SemiExpanded),
        "expanded" => Some(fontdb::Stretch::Expanded),
        "extra-expanded" => Some(fontdb::Stretch::ExtraExpanded),
        "ultra-expanded" => Some(fontdb::Stretch::UltraExpanded),
        _ => None,
    }
}

fn parse_font_face_metric_percent(raw: Option<&str>) -> Option<f32> {
    let value = raw?.trim();
    if value.eq_ignore_ascii_case("normal") {
        return None;
    }
    let number = value.strip_suffix('%')?.trim().parse::<f32>().ok()?;
    if number.is_finite() && number >= 0.0 {
        Some(number / 100.0)
    } else {
        None
    }
}

fn font_face_metric_override(
    face: &crate::css::FontFaceDecl,
) -> crate::layout::inline_layout::FontMetricOverride {
    crate::layout::inline_layout::FontMetricOverride {
        size_adjust: parse_font_face_metric_percent(face.size_adjust.as_deref()),
        ascent: parse_font_face_metric_percent(face.ascent_override.as_deref()),
        descent: parse_font_face_metric_percent(face.descent_override.as_deref()),
        line_gap: parse_font_face_metric_percent(face.line_gap_override.as_deref()),
    }
}

fn register_css_font_face_alias(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    ids: &[fontdb::ID],
) {
    let Some(css_family) = css_font_family_name(&face.family) else {
        return;
    };
    let css_weight = parse_font_face_weight(face.weight.as_deref());
    let css_style = parse_font_face_style(face.style.as_deref());
    let css_stretch = parse_font_face_stretch(face.stretch.as_deref());

    let aliases: Vec<_> = ids
        .iter()
        .filter_map(|id| fs.db().face(*id).cloned())
        .map(|mut info| {
            info.id = fontdb::ID::dummy();
            info.families
                .retain(|(name, _)| !name.eq_ignore_ascii_case(&css_family));
            info.families.insert(
                0,
                (css_family.clone(), fontdb::Language::English_UnitedStates),
            );
            if let Some(weight) = css_weight {
                info.weight = weight;
            }
            if let Some(style) = css_style {
                info.style = style;
            }
            if let Some(stretch) = css_stretch {
                info.stretch = stretch;
            }
            info
        })
        .collect();

    if aliases.is_empty() {
        return;
    }

    for alias in aliases {
        fs.db_mut().push_face_info(alias);
    }
    crate::layout::inline_layout::set_font_metric_override(
        &css_family,
        font_face_metric_override(face),
    );
    crate::layout::inline_layout::clear_font_family_caches();
}

fn load_font_face_bytes(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    bytes: Vec<u8>,
) -> bool {
    let ids = load_font_bytes(fs, bytes);
    if ids.is_empty() {
        return false;
    }
    register_css_font_face_alias(fs, face, &ids);
    true
}

fn font_source_formats_supported(source: &crate::css::FontFaceSource) -> bool {
    source.formats.is_empty()
        || source.formats.iter().any(|format| {
            matches!(
                format.as_str(),
                "woff2"
                    | "woff"
                    | "opentype"
                    | "truetype"
                    | "embedded-opentype"
                    | "collection"
                    | "font/woff2"
                    | "font/woff"
                    | "font/otf"
                    | "font/ttf"
            )
        })
}

pub(crate) fn font_source_techs_supported(source: &crate::css::FontFaceSource) -> bool {
    source.techs.is_empty()
        || source.techs.iter().all(|tech| {
            matches!(
                tech.as_str(),
                "features-opentype" | "features-aat" | "variations" | "variations-opentype"
            )
        })
}

fn load_local_font_face(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    name: &str,
) -> bool {
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(name)],
        weight: parse_font_face_weight(face.weight.as_deref()).unwrap_or(fontdb::Weight::NORMAL),
        stretch: parse_font_face_stretch(face.stretch.as_deref())
            .unwrap_or(fontdb::Stretch::Normal),
        style: parse_font_face_style(face.style.as_deref()).unwrap_or(fontdb::Style::Normal),
    };
    let Some(id) = fs.db().query(&query) else {
        return false;
    };
    register_css_font_face_alias(fs, face, &[id]);
    true
}

/// Minimal Base64 decoder (no external dependency).
/// Returns `Err` on invalid input.
fn decode_base64(s: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 128] = b"\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x3e\xff\xff\xff\x3f\
        \x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\xff\xff\xff\xff\xff\xff\
        \xff\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\
        \x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\xff\xff\xff\xff\xff\
        \xff\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\
        \x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\xff\xff\xff\xff\xff";

    let s: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut i = 0;
    while i + 3 < s.len() {
        let a = s[i];
        let b = s[i + 1];
        let c = s[i + 2];
        let d = s[i + 3];
        if a >= 128 || b >= 128 || c >= 128 || d >= 128 {
            return Err(());
        }
        let va = TABLE[a as usize];
        let vb = TABLE[b as usize];
        let vc = if c == b'=' { 0 } else { TABLE[c as usize] };
        let vd = if d == b'=' { 0 } else { TABLE[d as usize] };
        if va == 0xff || vb == 0xff || vc == 0xff || vd == 0xff {
            return Err(());
        }
        out.push((va << 2) | (vb >> 4));
        if c != b'=' {
            out.push((vb << 4) | (vc >> 2));
        }
        if d != b'=' {
            out.push((vc << 6) | vd);
        }
        i += 4;
    }
    Ok(out)
}

// ─── Float Context ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct FloatItem {
    pub rect: Rect,
    pub side: FloatSide,
    pub clear: f32, // bottom of this float
    pub shape: Option<FloatShape>,
    pub shape_margin: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FloatSide {
    Left,
    Right,
}

impl Default for FloatSide {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FloatShape {
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
    Inset {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
    Polygon(Vec<(f32, f32)>),
}

#[derive(Debug, Default, Clone)]
pub struct FloatContext {
    pub floats: Vec<FloatItem>,
    pub origin_x: f32, // Document X of the context root
    pub origin_y: f32, // Document Y of the context root
}

impl FloatContext {
    pub fn available_width(
        &self,
        y: f32,
        line_h: f32,
        containing_w: f32,
        out_left: &mut f32,
        out_right: &mut f32,
    ) {
        *out_left = 0.0;
        *out_right = containing_w;
        for f in &self.floats {
            if f.rect.y < y + line_h && f.clear > y {
                if f.side == FloatSide::Left {
                    let r = f.right_exclusion_at(y, line_h);
                    if r > *out_left {
                        *out_left = r;
                    }
                } else {
                    let l = f.left_exclusion_at(y, line_h);
                    if l < *out_right {
                        *out_right = l;
                    }
                }
            }
        }
    }

    pub fn available_width_in(
        &self,
        x: f32,
        y: f32,
        line_h: f32,
        containing_w: f32,
        out_left: &mut f32,
        out_right: &mut f32,
    ) {
        *out_left = 0.0;
        *out_right = containing_w;
        for f in &self.floats {
            if f.rect.y < y + line_h && f.clear > y {
                let f_left = f.left_exclusion_at(y, line_h);
                let f_right = f.right_exclusion_at(y, line_h);
                if f_right <= x || f_left >= x + containing_w {
                    continue;
                }
                if f.side == FloatSide::Left {
                    let r = (f_right - x).clamp(0.0, containing_w);
                    if r > *out_left {
                        *out_left = r;
                    }
                } else {
                    let l = (f_left - x).clamp(0.0, containing_w);
                    if l < *out_right {
                        *out_right = l;
                    }
                }
            }
        }
    }

    pub fn clear_y(&self, current_y: f32, clear: Clear) -> f32 {
        let mut y = current_y;
        for f in &self.floats {
            match clear {
                Clear::Left if f.side == FloatSide::Left => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                Clear::Right if f.side == FloatSide::Right => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                Clear::Both => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                _ => {}
            }
        }
        y
    }

    pub fn next_clear_y(&self, y: f32) -> Option<f32> {
        let next_y = self
            .floats
            .iter()
            .filter(|f| f.clear > y + 0.001)
            .map(|f| f.clear)
            .fold(f32::MAX, f32::min);
        if next_y < f32::MAX {
            Some(next_y)
        } else {
            None
        }
    }

    pub fn place_float(
        &mut self,
        current_y: f32,
        float_w: f32,
        float_h: f32,
        containing_w: f32,
        side: FloatSide,
        shape_outside: &str,
        shape_margin: f32,
    ) -> Rect {
        self.place_float_in(
            0.0,
            current_y,
            float_w,
            float_h,
            containing_w,
            side,
            shape_outside,
            shape_margin,
        )
    }

    pub fn place_float_in(
        &mut self,
        x: f32,
        current_y: f32,
        float_w: f32,
        float_h: f32,
        containing_w: f32,
        side: FloatSide,
        shape_outside: &str,
        shape_margin: f32,
    ) -> Rect {
        // Find the lowest Y position where the float fits horizontally.
        let mut y = current_y;
        loop {
            let mut left = 0.0f32;
            let mut right = containing_w;
            self.available_width_in(x, y, float_h, containing_w, &mut left, &mut right);
            let available = right - left;
            if available >= float_w {
                break;
            }
            // Move past the nearest float bottom
            let next_y = self
                .floats
                .iter()
                .filter(|f| f.clear > y)
                .map(|f| f.clear)
                .fold(f32::MAX, f32::min);
            if next_y == f32::MAX {
                break;
            }
            y = next_y;
        }

        let mut left = 0.0f32;
        let mut right = containing_w;
        self.available_width_in(x, y, float_h, containing_w, &mut left, &mut right);

        let local_x = if side == FloatSide::Left {
            left
        } else {
            right - float_w
        };
        let rect = Rect::new(x + local_x, y, float_w, float_h);
        self.floats.push(FloatItem {
            rect,
            side,
            clear: y + float_h,
            shape: parse_float_shape(shape_outside, float_w, float_h),
            shape_margin: shape_margin.max(0.0),
        });
        Rect::new(local_x, y, float_w, float_h)
    }
}

impl FloatItem {
    fn right_exclusion_at(&self, y: f32, line_h: f32) -> f32 {
        match self.shape.as_ref() {
            Some(shape) => {
                let (_, right) = shape_exclusion_x(shape, self.rect, self.shape_margin, y, line_h);
                right
            }
            None => self.rect.x + self.rect.w,
        }
    }

    fn left_exclusion_at(&self, y: f32, line_h: f32) -> f32 {
        match self.shape.as_ref() {
            Some(shape) => {
                let (left, _) = shape_exclusion_x(shape, self.rect, self.shape_margin, y, line_h);
                left
            }
            None => self.rect.x,
        }
    }
}

fn shape_exclusion_x(
    shape: &FloatShape,
    rect: Rect,
    margin: f32,
    y: f32,
    line_h: f32,
) -> (f32, f32) {
    let sample_y = (y + line_h * 0.5 - rect.y).clamp(0.0, rect.h.max(0.0));
    match *shape {
        FloatShape::Circle { cx, cy, r } => {
            ellipse_exclusion_x(rect, cx, cy, r, r, margin, sample_y)
        }
        FloatShape::Ellipse { cx, cy, rx, ry } => {
            ellipse_exclusion_x(rect, cx, cy, rx, ry, margin, sample_y)
        }
        FloatShape::Inset {
            top,
            right,
            bottom,
            left,
        } => {
            let top = (top - margin).max(0.0);
            let bottom_limit = (rect.h - bottom + margin).min(rect.h);
            if sample_y < top || sample_y > bottom_limit {
                (rect.x, rect.x)
            } else {
                (
                    rect.x + (left - margin).max(0.0),
                    rect.x + rect.w - (right - margin).max(0.0),
                )
            }
        }
        FloatShape::Polygon(ref points) => polygon_exclusion_x(rect, margin, points, sample_y),
    }
}

fn ellipse_exclusion_x(
    rect: Rect,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    margin: f32,
    sample_y: f32,
) -> (f32, f32) {
    let rx = (rx + margin).max(0.0);
    let ry = (ry + margin).max(0.0);
    if rx <= 0.0 || ry <= 0.0 {
        return (rect.x, rect.x);
    }
    let dy = (sample_y - cy).abs();
    if dy >= ry {
        return (rect.x + cx, rect.x + cx);
    }
    let half = rx * (1.0 - (dy / ry).powi(2)).sqrt();
    (rect.x + cx - half, rect.x + cx + half)
}

fn parse_float_shape(value: &str, width: f32, height: f32) -> Option<FloatShape> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("circle(") && value.ends_with(')') {
        parse_circle_shape(&value["circle(".len()..value.len() - 1], width, height)
    } else if lower.starts_with("ellipse(") && value.ends_with(')') {
        parse_ellipse_shape(&value["ellipse(".len()..value.len() - 1], width, height)
    } else if lower.starts_with("inset(") && value.ends_with(')') {
        parse_inset_shape(&value["inset(".len()..value.len() - 1], width, height)
    } else if lower.starts_with("polygon(") && value.ends_with(')') {
        parse_polygon_shape(&value["polygon(".len()..value.len() - 1], width, height)
    } else {
        None
    }
}

fn parse_circle_shape(inner: &str, width: f32, height: f32) -> Option<FloatShape> {
    let (radius_part, position_part) = split_shape_at(inner);
    let default_r = width.min(height) * 0.5;
    let r = radius_part
        .and_then(|r| resolve_shape_len(r, width.min(height)))
        .unwrap_or(default_r);
    let (cx, cy) = parse_shape_position(position_part, width, height);
    Some(FloatShape::Circle { cx, cy, r })
}

fn parse_ellipse_shape(inner: &str, width: f32, height: f32) -> Option<FloatShape> {
    let (radii_part, position_part) = split_shape_at(inner);
    let mut radii = radii_part.unwrap_or("").split_whitespace();
    let rx = radii
        .next()
        .and_then(|v| resolve_shape_len(v, width))
        .unwrap_or(width * 0.5);
    let ry = radii
        .next()
        .and_then(|v| resolve_shape_len(v, height))
        .unwrap_or(height * 0.5);
    let (cx, cy) = parse_shape_position(position_part, width, height);
    Some(FloatShape::Ellipse { cx, cy, rx, ry })
}

fn parse_inset_shape(inner: &str, width: f32, height: f32) -> Option<FloatShape> {
    let before_round = inner.split("round").next().unwrap_or(inner);
    let tokens: Vec<&str> = before_round.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    let top_token = tokens[0];
    let right_token = *tokens.get(1).unwrap_or(&top_token);
    let bottom_token = *tokens.get(2).unwrap_or(&top_token);
    let left_token = *tokens.get(3).unwrap_or(&right_token);
    let top = resolve_shape_len(top_token, height).unwrap_or(0.0);
    let right = resolve_shape_len(right_token, width).unwrap_or(0.0);
    let bottom = resolve_shape_len(bottom_token, height).unwrap_or(0.0);
    let left = resolve_shape_len(left_token, width).unwrap_or(0.0);
    Some(FloatShape::Inset {
        top,
        right,
        bottom,
        left,
    })
}

fn parse_polygon_shape(inner: &str, width: f32, height: f32) -> Option<FloatShape> {
    let mut points = Vec::new();
    for raw in inner.split(',') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let coords: Vec<&str> = part.split_whitespace().collect();
        let start = if coords
            .first()
            .is_some_and(|v| v.eq_ignore_ascii_case("evenodd") || v.eq_ignore_ascii_case("nonzero"))
        {
            1
        } else {
            0
        };
        let Some(x_token) = coords.get(start) else {
            continue;
        };
        let Some(y_token) = coords.get(start + 1) else {
            continue;
        };
        if let (Some(x), Some(y)) = (
            resolve_shape_len(x_token, width),
            resolve_shape_len(y_token, height),
        ) {
            points.push((x, y));
        }
    }
    if points.len() >= 3 {
        Some(FloatShape::Polygon(points))
    } else {
        None
    }
}

fn polygon_exclusion_x(
    rect: Rect,
    margin: f32,
    points: &[(f32, f32)],
    sample_y: f32,
) -> (f32, f32) {
    let mut xs = Vec::new();
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        if (y1 <= sample_y && sample_y < y2) || (y2 <= sample_y && sample_y < y1) {
            let t = (sample_y - y1) / (y2 - y1);
            xs.push(x1 + (x2 - x1) * t);
        } else if (sample_y - y1).abs() < f32::EPSILON && (y1 - y2).abs() < f32::EPSILON {
            xs.push(x1);
            xs.push(x2);
        }
    }
    if xs.len() < 2 {
        return (rect.x, rect.x);
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    let left = xs.first().copied().unwrap_or(0.0) - margin;
    let right = xs.last().copied().unwrap_or(0.0) + margin;
    (
        rect.x + left.clamp(0.0, rect.w),
        rect.x + right.clamp(0.0, rect.w),
    )
}

fn split_shape_at(inner: &str) -> (Option<&str>, Option<&str>) {
    if let Some(idx) = inner.to_ascii_lowercase().find(" at ") {
        let before = inner[..idx].trim();
        let after = inner[idx + 4..].trim();
        (
            if before.is_empty() {
                None
            } else {
                Some(before)
            },
            if after.is_empty() { None } else { Some(after) },
        )
    } else {
        let trimmed = inner.trim();
        (
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            },
            None,
        )
    }
}

fn parse_shape_position(position: Option<&str>, width: f32, height: f32) -> (f32, f32) {
    let Some(position) = position else {
        return (width * 0.5, height * 0.5);
    };
    let mut parts = position.split_whitespace();
    let x = parts
        .next()
        .and_then(|v| resolve_shape_len(v, width))
        .unwrap_or(width * 0.5);
    let y = parts
        .next()
        .and_then(|v| resolve_shape_len(v, height))
        .unwrap_or(height * 0.5);
    (x, y)
}

fn resolve_shape_len(token: &str, basis: f32) -> Option<f32> {
    let token = token.trim();
    if let Some(percent) = token.strip_suffix('%') {
        percent.parse::<f32>().ok().map(|v| basis * v / 100.0)
    } else if let Some(px) = token.strip_suffix("px") {
        px.parse::<f32>().ok()
    } else {
        token.parse::<f32>().ok()
    }
}

/// Collect node_ids of elements that have hover-dependent styles.
fn collect_hover_sensitive(node: &WebCore, out: &mut std::collections::HashSet<u32>) {
    if node.style.hover_style.is_some() {
        out.insert(node.node_id);
    }
    for child in &node.children {
        collect_hover_sensitive(child, out);
    }
}

/// Walk the tree bottom-up: if any child is `layout_dirty`, mark the parent
/// dirty too.  Returns `true` if the node (or any descendant) is dirty.
fn propagate_dirty(node: &mut WebCore) -> bool {
    let mut child_dirty = false;
    for child in &mut node.children {
        if propagate_dirty(child) {
            child_dirty = true;
        }
    }

    if node.layout.layout_dirty
        || child_dirty
        || node.has_dirty_descendant
        || node.has_dirty_layout_descendant
    {
        // Invalidate intrinsic width cache — a dirty descendant means our
        // intrinsic size may have changed (needed by flex/grid/table parents).
        node.layout.cached_intrinsic_w.set(f32::NAN);
        node.layout.intrinsic_dirty = true;
    }

    if child_dirty {
        node.layout.layout_dirty = true;
        node.has_dirty_layout_descendant = true;
    }
    node.layout.layout_dirty
}

// ─── Resolved box model ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolvedBox {
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,

    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,

    pub border_top: f32,
    pub border_right: f32,
    pub border_bottom: f32,
    pub border_left: f32,

    pub content_width: Option<f32>, // None = auto
    pub content_height: Option<f32>,
}

impl ResolvedBox {
    pub fn h_space(&self) -> f32 {
        self.margin_left
            + self.border_left
            + self.padding_left
            + self.padding_right
            + self.border_right
            + self.margin_right
    }
    pub fn v_space(&self) -> f32 {
        self.margin_top
            + self.border_top
            + self.padding_top
            + self.padding_bottom
            + self.border_bottom
            + self.margin_bottom
    }
    pub fn inner_h_space(&self) -> f32 {
        self.border_left + self.padding_left + self.padding_right + self.border_right
    }
    pub fn inner_v_space(&self) -> f32 {
        self.border_top + self.padding_top + self.padding_bottom + self.border_bottom
    }
}

pub fn resolve_box(
    style: &ComputedStyle,
    parent_font_px: f32,
    containing_w: f32,
    root_font_px: f32,
) -> ResolvedBox {
    resolve_box_vp(
        style,
        parent_font_px,
        containing_w,
        root_font_px,
        0.0,
        0.0,
        None,
    )
}

pub fn resolve_box_vp(
    style: &ComputedStyle,
    parent_font_px: f32,
    containing_w: f32,
    root_font_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    containing_h: Option<f32>,
) -> ResolvedBox {
    let res = |l: &CssLength| {
        l.resolve_vp(
            parent_font_px,
            containing_w,
            root_font_px,
            viewport_w,
            viewport_h,
        )
    };
    let _font_px = style.font_size_px(parent_font_px, root_font_px);

    let pad_left = res(&style.padding_left).max(0.0);
    let pad_right = res(&style.padding_right).max(0.0);
    let pad_top = res(&style.padding_top).max(0.0);
    let pad_bottom = res(&style.padding_bottom).max(0.0);

    let border_left = if style.border_left_style != BorderStyle::None {
        res(&style.border_left_width)
    } else {
        0.0
    };
    let border_right = if style.border_right_style != BorderStyle::None {
        res(&style.border_right_width)
    } else {
        0.0
    };
    let border_top = if style.border_top_style != BorderStyle::None {
        res(&style.border_top_width)
    } else {
        0.0
    };
    let border_bottom = if style.border_bottom_style != BorderStyle::None {
        res(&style.border_bottom_width)
    } else {
        0.0
    };

    // ⛔ `width` and `height` DO NOT APPLY to a non-replaced inline box —
    // CSS 2.1 §10.2 and §10.5. An `<span style="width:100px;height:50px">`
    // is sized by its text, and this sized it 100x50; Chrome answers 8x18 for
    // the same markup.
    //
    // `inline-block`, `inline-flex` and the replaced elements are all
    // inline-LEVEL but do take a width, so the test is `display: inline`
    // exactly, not "is inline-level".
    let inline_ignores_size = style.display == Display::Inline;
    let content_width = if style.width.is_auto() || inline_ignores_size {
        None
    } else {
        let mut w = res(&style.width).max(0.0);
        // box-sizing: border-box — subtract padding + border from declared width
        if style.box_sizing == BoxSizing::BorderBox {
            w = (w - pad_left - pad_right - border_left - border_right).max(0.0);
        }
        Some(w)
    };

    // CSS 2.1 §10.5: percentage heights resolve against the containing block's height.
    // If the containing block's height is not explicitly set (containing_h is None),
    // percentage heights are treated as auto.
    let content_height = if style.height.is_auto() || inline_ignores_size {
        None
    } else if matches!(style.height, CssLength::Percent(_)) {
        match containing_h {
            Some(ch) => {
                let mut h = style
                    .height
                    .resolve_vp(parent_font_px, ch, root_font_px, viewport_w, viewport_h)
                    .max(0.0);
                if style.box_sizing == BoxSizing::BorderBox {
                    h = (h - pad_top - pad_bottom - border_top - border_bottom).max(0.0);
                }
                Some(h)
            }
            None => None, // percentage height with no explicit containing height → auto
        }
    } else {
        let mut h = res(&style.height).max(0.0);
        if style.box_sizing == BoxSizing::BorderBox {
            h = (h - pad_top - pad_bottom - border_top - border_bottom).max(0.0);
        }
        Some(h)
    };

    ResolvedBox {
        margin_top: res(&style.margin_top),
        margin_right: res(&style.margin_right),
        margin_bottom: res(&style.margin_bottom),
        margin_left: res(&style.margin_left),

        padding_top: pad_top,
        padding_right: pad_right,
        padding_bottom: pad_bottom,
        padding_left: pad_left,

        border_top,
        border_right,
        border_bottom,
        border_left,

        content_width,
        content_height,
    }
}

// ─── Layout Engine ────────────────────────────────────────────────────────────

pub struct LayoutEngine {
    pub root_font_px: f32,
    /// Logical viewport width (for vw units).
    pub viewport_w: f32,
    /// Logical viewport height (for vh units).
    pub viewport_h: f32,
    /// Reference to a font system for accurate measurement.
    pub font_system: Option<*mut cosmic_text::FontSystem>,
    /// Custom component registry for custom tags
    pub component_registry: ComponentRegistry,
    /// Device pixel ratio (e.g. 2.0 on HiDPI/Retina). Used so that char_x
    /// positions are shaped at physical pixel size — matching the renderer —
    /// giving accurate click↔caret mapping on every display density.
    pub scale: f32,
    /// Viewport width used in the last cascade pass, for skip-cascade optimization.
    last_cascade_vw: f32,
    /// Viewport height used in the last geometry pass, to detect vh-unit changes.
    last_geometry_viewport_h: f32,
    /// Whether any @media rules exist — cached to avoid O(n) scan every layout.
    cached_has_media_q: bool,
    /// Whether any @container rules exist — cached to avoid O(n) scan every layout.
    cached_has_container_q: bool,
    /// Y cutoff for progressive layout — nodes below this are deferred.
    /// 0 = no cutoff (full layout). Set to viewport_h * 1.5 for first-screen priority.
    pub progressive_cutoff: f32,
    /// True after the first layout pass has completed. Prevents progressive
    /// layout from running on every subsequent layout (hover, image load, etc.).
    initial_layout_done: bool,
    /// Whether @font-face font fetches have been kicked off (not necessarily finished).
    fonts_loaded: bool,
    /// Receiver for async font data arriving from background threads.
    pending_fonts: Option<std::sync::mpsc::Receiver<(crate::css::FontFaceDecl, Vec<u8>)>>,
    /// Number of font fetches still in flight.
    fonts_in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Containing block rect for the nearest positioned (non-static) ancestor.
    /// Used by abs-pos children to resolve their containing block correctly.
    pub pos_cb: Cell<Rect>,
    /// Containing block rect for fixed-position descendants. Unlike abs-pos,
    /// ordinary positioned ancestors do not capture fixed descendants.
    pub fixed_cb: Cell<Rect>,
    /// Current recursion depth — prevents stack overflow on deeply nested DOMs.
    layout_depth: Cell<usize>,
    /// Total layout_box calls — detect infinite loops.
    layout_calls: Cell<usize>,
    /// Layout start time — detect long-running layout.
    layout_start: Cell<Option<std::time::Instant>>,
    /// Text measurement cache: (text_hash, font_size_bits, weight, family_hash) → width.
    /// Avoids redundant cosmic_text font shaping on re-layout.
    text_width_cache: std::cell::RefCell<HashMap<u64, f32>>,
}

/// Maximum layout recursion depth to prevent stack overflow.
const MAX_LAYOUT_DEPTH: usize = 400;

impl LayoutEngine {
    pub fn new() -> Self {
        Self {
            root_font_px: 16.0,
            viewport_w: 900.0,
            viewport_h: 700.0,
            font_system: None,
            component_registry: ComponentRegistry::default(),
            scale: 1.0,
            last_cascade_vw: f32::NAN, // NAN forces cascade on first call
            last_geometry_viewport_h: f32::NAN, // NAN forces full layout on first call
            cached_has_media_q: false,
            cached_has_container_q: false,
            progressive_cutoff: 0.0,
            initial_layout_done: false,
            fonts_loaded: false,
            text_width_cache: std::cell::RefCell::new(HashMap::new()),
            pending_fonts: None,
            fonts_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pos_cb: Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
            fixed_cb: Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
            layout_depth: Cell::new(0),
            layout_calls: Cell::new(0),
            layout_start: Cell::new(None),
        }
    }

    /// Measure text width with caching. Returns logical pixel width.
    pub fn measure_text_cached(
        &self,
        text: &str,
        font_px: f32,
        weight: FontWeight,
        style: FontStyle,
        font_family: &str,
    ) -> f32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Build a compact cache key from text + font properties
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        font_px.to_bits().hash(&mut hasher);
        weight.value().hash(&mut hasher);
        (style as u8).hash(&mut hasher);
        font_family.hash(&mut hasher);
        self.scale.to_bits().hash(&mut hasher);
        let font_state = self.font_system.map(|fs_ptr| {
            let fs = unsafe { &*fs_ptr };
            (
                fs.db().len(),
                crate::layout::inline_layout::font_size_adjust_scale(fs, font_family).to_bits(),
            )
        });
        font_state.hash(&mut hasher);
        let key = hasher.finish();

        // Check cache
        if let Some(&w) = self.text_width_cache.borrow().get(&key) {
            perf::record_text_measure(true);
            return w;
        }
        perf::record_text_measure(false);

        // Measure and cache
        let w = if let Some(fs_ptr) = self.font_system {
            let fs = unsafe { &mut *fs_ptr };
            crate::layout::inline_layout::measure_text_width_weighted(
                text,
                font_px,
                Some(fs),
                weight,
                style,
                self.scale,
                font_family,
            )
        } else {
            crate::layout::inline_layout::measure_text_width_ts(text, font_px, 8)
        };

        self.text_width_cache.borrow_mut().insert(key, w);
        w
    }

    fn field_sizing_content_width(&self, node: &WebCore, font_px: f32) -> Option<f32> {
        if !node.style.field_sizing.eq_ignore_ascii_case("content") || !node.style.width.is_auto() {
            return None;
        }
        if !crate::types::is_text_input(node) && node.tag != "textarea" {
            return None;
        }
        let value = crate::types::input_value(node);
        let text = if value.is_empty() {
            node.attributes
                .get("placeholder")
                .map(String::as_str)
                .unwrap_or("")
        } else {
            value.as_str()
        };
        let measure = if node.tag == "textarea" {
            text.lines()
                .max_by_key(|line| line.chars().count())
                .filter(|line| !line.is_empty())
                .unwrap_or(" ")
        } else if text.is_empty() {
            " "
        } else {
            text
        };
        Some(
            self.measure_text_cached(
                measure,
                font_px,
                node.style.font_weight,
                node.style.font_style,
                &node.style.font_family,
            )
            .ceil()
            .max(font_px),
        )
    }

    fn field_sizing_content_height(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        if !node.style.field_sizing.eq_ignore_ascii_case("content") || !node.style.height.is_auto()
        {
            return None;
        }
        if node.tag != "textarea" {
            return None;
        }
        let value = crate::types::input_value(node);
        let text = if value.is_empty() {
            node.attributes
                .get("placeholder")
                .map(String::as_str)
                .unwrap_or("")
        } else {
            value.as_str()
        };
        let line_count = text.lines().count().max(1) as f32;
        let line_height = node
            .style
            .line_height
            .resolve(font_px, font_px, root_font_px)
            .max(font_px);
        Some((line_count * line_height).ceil())
    }

    /// Resolve a box's styles using the engine's viewport dimensions.
    #[inline]
    pub fn res_box(
        &self,
        style: &ComputedStyle,
        font_px: f32,
        containing_w: f32,
        root_font_px: f32,
    ) -> ResolvedBox {
        resolve_box_vp(
            style,
            font_px,
            containing_w,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
            None,
        )
    }

    /// Resolve a single CSS length using the engine's viewport dimensions.
    #[inline]
    pub fn res_len(
        &self,
        len: &CssLength,
        font_px: f32,
        containing: f32,
        root_font_px: f32,
    ) -> f32 {
        len.resolve_vp(
            font_px,
            containing,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
        )
    }

    /// Compute both min-content and max-content intrinsic widths in one call.
    /// This is the **unified intrinsic sizing API** — all callers (flex, grid,
    /// table, float, absolute) should use this instead of the separate functions.
    ///
    /// Returns `IntrinsicSizes { min_content, max_content }`.
    /// Results are cached via `cached_intrinsic_w` (max) on the node's LayoutBox.
    pub fn intrinsic_sizes(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> IntrinsicSizes {
        IntrinsicSizes {
            min_content: self.min_content_width(node, parent_font_px, root_font_px),
            max_content: self.max_content_width(node, parent_font_px, root_font_px),
        }
    }

    /// Intrinsic dimensions of a replaced element: the decoded image/canvas
    /// bitmap, else the `width`/`height` content attributes, else an `<svg>`
    /// viewBox. Media elements use their HTML fallback dimensions until a
    /// decoder can report stream metadata. `None` when the box is not replaced
    /// or its natural size is unknown.
    ///
    /// Layout and the intrinsic-width walk both size replaced boxes, so they
    /// read the natural size from here and cannot disagree about it.
    pub(crate) fn intrinsic_dimensions(&self, node: &WebCore) -> Option<(f32, f32)> {
        if node.is_image_element()
            || node.tag == "canvas"
            || matches!(node.tag.as_str(), "audio" | "video")
        {
            if node.image_width > 0 && node.image_height > 0 {
                return Some((node.image_width as f32, node.image_height as f32));
            }
            // Nothing decoded/allocated yet: the attributes stand in so the
            // box reserves the right shape before the bytes arrive. A canvas
            // has spec defaults even with no attributes.
            let attr = |k: &str| {
                node.attributes
                    .get(k)
                    .and_then(|s| crate::html::forms::parse_non_negative_integer(s))
                    .map(|value| value as f32)
                    .unwrap_or(0.0)
            };
            let (attr_w, attr_h) = (attr("width"), attr("height"));
            if node.tag == "video" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 150.0 };
                return Some((w, h));
            }
            if node.tag == "audio" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 54.0 };
                return Some((w, h));
            }
            if node.tag == "canvas" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 150.0 };
                return Some((w, h));
            }
            return match (attr_w > 0.0, attr_h > 0.0) {
                (true, true) => Some((attr_w, attr_h)),
                (true, false) => Some((attr_w, attr_w * 0.75)),
                (false, true) => Some((attr_h * 1.333, attr_h)),
                (false, false) => None,
            };
        }
        if node.tag == "svg" && node.svg_viewbox_w > 0.0 && node.svg_viewbox_h > 0.0 {
            return Some((node.svg_viewbox_w, node.svg_viewbox_h));
        }
        None
    }

    /// **A replaced element contributes the size it is DISPLAYED at, not its
    /// natural size** (CSS2.1 §10.4). With `width: auto` and a definite height
    /// the used width follows the height through the intrinsic ratio, so a
    /// 1024×1024 photo shown at `height: 150px` contributes 150 — reading the
    /// natural width instead made every card in a wrapping flex row as wide as
    /// the photo behind it.
    ///
    /// The result is a CONTENT-box width; the caller adds padding and border.
    fn replaced_intrinsic_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        let (iw, ih) = self.intrinsic_dimensions(node)?;
        if iw <= 0.0 || ih <= 0.0 {
            return None;
        }
        // A percentage has no containing block to resolve against while
        // measuring intrinsics, so it counts as indefinite.
        let definite = |len: &CssLength| -> f32 {
            if len.is_auto() || matches!(len, CssLength::Percent(_)) {
                return 0.0;
            }
            len.resolve_vp(font_px, 0.0, root_font_px, self.viewport_w, self.viewport_h)
        };
        let h = definite(&node.style.height);
        if h > 0.0 {
            return Some((h * iw / ih).round().max(0.0));
        }
        let (mut w, mut h) = (iw, ih);
        let max_w = definite(&node.style.max_width);
        if max_w > 0.0 && w > max_w {
            h = (max_w * ih / iw).round();
            w = max_w;
        }
        let max_h = definite(&node.style.max_height);
        if max_h > 0.0 && h > max_h {
            w = (max_h * iw / ih).round();
        }
        Some(w.max(0.0))
    }

    /// Compute the min-content width of a node (the smallest width it can take
    /// without overflowing).  For text, this is the width of the longest word.
    pub fn min_content_width(&self, node: &WebCore, parent_font_px: f32, root_font_px: f32) -> f32 {
        self.min_content_width_inner(node, parent_font_px, root_font_px, true)
    }

    /// Min-content width with the element's own `width` ignored. This is the
    /// CONTENT size suggestion of Flexbox §4.5 — the automatic minimum is the
    /// smaller of it and the specified size, so reading the specified width
    /// here would make the two the same number and stop the item shrinking.
    /// The width a definite height gives a box through its `aspect-ratio` —
    /// css-sizing-4 §4, the height→width direction of the transfer.
    ///
    /// `None` when the box has no ratio or no definite height, which leaves
    /// the caller's own measurement in charge.
    fn aspect_ratio_transferred_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        let ratio = node.style.aspect_ratio.filter(|r| *r > 0.0)?;
        // A percentage height is not definite during intrinsic measurement:
        // there is no containing height to resolve it against here.
        if node.style.height.is_auto() || matches!(node.style.height, CssLength::Percent(_)) {
            return None;
        }
        let mut h = self.res_len(&node.style.height, font_px, 0.0, root_font_px);
        if node.style.box_sizing == BoxSizing::BorderBox {
            let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
            h = (h - rb.padding_top - rb.padding_bottom - rb.border_top - rb.border_bottom)
                .max(0.0);
        }
        if h > 0.0 {
            Some(h * ratio)
        } else {
            None
        }
    }

    /// A sizing length that may be an INTRINSIC KEYWORD rather than a length.
    ///
    /// `min-width` and `max-width` accept `min-content`/`max-content`/
    /// `fit-content` too (css-sizing-3 §5), and `res_len` answers 0 for those
    /// — so `min-width: max-content` on a narrow box was simply a floor of
    /// zero, and `max-width: max-content` read as "no maximum".
    pub fn res_len_sizing(
        &self,
        len: &CssLength,
        node: &WebCore,
        avail: f32,
        font_px: f32,
        containing_w: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        len.intrinsic().map(|kind| {
            self.intrinsic_width(&kind, node, avail, font_px, root_font_px, containing_w)
        })
    }

    fn clamp_resolved_content_width(
        &self,
        rbox: &mut ResolvedBox,
        node: &WebCore,
        containing_w: f32,
        font_px: f32,
        root_font_px: f32,
    ) {
        let Some(raw_w) = rbox.content_width else {
            return;
        };
        let bb_extra = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
            rbox.padding_left + rbox.padding_right + rbox.border_left + rbox.border_right
        } else {
            0.0
        };
        let avail_w = (containing_w - rbox.h_space()).max(0.0);
        let min_w = match self.res_len_sizing(
            &node.style.min_width,
            node,
            avail_w,
            font_px,
            containing_w,
            root_font_px,
        ) {
            Some(v) => v,
            None => {
                let v = self.res_len(&node.style.min_width, font_px, containing_w, root_font_px);
                (v - bb_extra).max(0.0)
            }
        };
        let max_w = match self.res_len_sizing(
            &node.style.max_width,
            node,
            avail_w,
            font_px,
            containing_w,
            root_font_px,
        ) {
            Some(v) => v,
            None if node.style.max_width.is_none() || node.style.max_width.is_auto() => f32::MAX,
            None => {
                let v = self.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
                (v - bb_extra).max(0.0)
            }
        };
        rbox.content_width = Some(raw_w.max(min_w).min(max_w));
    }

    /// Turn an intrinsic sizing keyword into a width — css-sizing-3 §5, §6.1.
    ///
    /// One definition, shared by block, inline and flex container sizing. Each
    /// of the three had its own copy of the same three-arm match, so a form the
    /// spec added later — `fit-content(<length>)` — had to be taught to all
    /// three or silently behave as the bare keyword in whichever was missed.
    pub fn intrinsic_width(
        &self,
        kind: &CssLength,
        node: &WebCore,
        avail: f32,
        font_px: f32,
        root_font_px: f32,
        containing_w: f32,
    ) -> f32 {
        let mn = self.min_content_width_of_content(node, font_px, root_font_px);
        let mx = self.max_content_width_of_content(node, font_px, root_font_px);
        match kind {
            CssLength::MinContent => mn,
            CssLength::MaxContent => mx,
            // The function form substitutes its argument for the available
            // space: max(min-content, min(max-content, argument)).
            CssLength::FitContentArg(a) => {
                let x = a.resolve(font_px, containing_w, root_font_px);
                mx.min(x).max(mn)
            }
            // `fit-content` is max-content clamped to what is available,
            // floored by min-content.
            _ => mx.min(avail).max(mn),
        }
    }

    pub fn min_content_width_of_content(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.min_content_width_inner(node, parent_font_px, root_font_px, false)
    }

    fn min_content_width_inner(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
        honor_width: bool,
    ) -> f32 {
        if matches!(node.style.display, Display::None) {
            return 0.0;
        }

        let font_px = node.style.font_size_px(parent_font_px, root_font_px);

        // Explicit width → use that directly
        if honor_width
            && !node.style.width.is_auto()
            && !matches!(node.style.width, CssLength::Percent(_))
        {
            let w = self.res_len(&node.style.width, font_px, 0.0, root_font_px);
            // ⛔ A `border-box` width ALREADY contains the padding and border,
            // and every caller adds those again on top of what we return — the
            // contract here is a CONTENT width. Handing back the raw value made
            // a shrink-to-fit parent wider by exactly the child's padding and
            // border, on the near-universal `* { box-sizing: border-box }`.
            if node.style.box_sizing == BoxSizing::BorderBox {
                let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
                let edges = rb.padding_left + rb.padding_right + rb.border_left + rb.border_right;
                return (w - edges).max(0.0);
            }
            return w.max(0.0);
        }

        // css-sizing-4 §4: a box with a preferred aspect ratio and a DEFINITE
        // block size transfers that size through the ratio — in BOTH
        // directions. Only the width→height direction was implemented, so
        // `display:inline-block; aspect-ratio:2/1; height:100px` measured as
        // zero wide and collapsed, instead of 200.
        if honor_width {
            if let Some(w) = self.aspect_ratio_transferred_width(node, font_px, root_font_px) {
                return w;
            }
        }

        // Replaced elements: the size they are shown at, ratio included.
        if let Some(w) = self.replaced_intrinsic_width(node, font_px, root_font_px) {
            return w;
        }

        // Custom component: use cached dimensions (like a replaced element)
        if self.component_registry.get_component(&node.tag).is_some()
            || self.component_registry.map.contains_key(&node.tag)
        {
            return if node.component_width > 0.0 {
                node.component_width
            } else {
                // First call before layout — measure to get initial size
                if let Some(c) = self.component_registry.get_component(&node.tag) {
                    c.measure(node, 0.0).0
                } else if let Some(cb) = self.component_registry.map.get(&node.tag) {
                    (cb.measure)(node, 0.0).0
                } else {
                    0.0
                }
            };
        }

        let _rbox = self.res_box(&node.style, font_px, 0.0, root_font_px);
        let generated_inline_w = self.generated_inline_content_width(node, font_px, root_font_px);
        let own_text_w = self.min_content_width_of_direct_text(node, font_px, root_font_px);

        // Text node or pseudo-element (::before/::after) with direct text content.
        // Pseudo-elements store content in node.text, not as #text children.
        let is_pseudo = matches!(node.tag.as_str(), "::before" | "::after");
        let has_direct_text = node.is_text_node() || (is_pseudo && !node.text.is_empty());
        if has_direct_text {
            let text = &node.text;
            if text.is_empty() {
                return 0.0;
            }
            let mut max_word = 0.0f32;
            for word in text.split(|c: char| c.is_ascii_whitespace()) {
                if word.is_empty() {
                    continue;
                }
                let w = self.measure_text_cached(
                    word,
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                );
                if w > max_word {
                    max_word = w;
                }
            }
            return max_word;
        }

        // ⛔ A SINGLE-LINE ROW FLEX CONTAINER SUMS ITS ITEMS (css-flexbox-1
        // §9.9). Its min-content main size is computed exactly like the
        // max-content main size, but from the items' MIN-content
        // contributions — so the items sit side by side and their widths add.
        // Taking the max here reported `min-content` on a nowrap row flex as
        // the widest single item, which is what a wrap container does.
        let single_line_row_flex =
            matches!(node.style.display, Display::Flex | Display::InlineFlex)
                && matches!(
                    node.style.flex_direction,
                    FlexDirection::Row | FlexDirection::RowReverse
                )
                && matches!(node.style.flex_wrap, FlexWrap::Nowrap);
        if single_line_row_flex {
            let gap = self.res_len(&node.style.column_gap, font_px, 0.0, root_font_px);
            let mut total = 0.0f32;
            let mut count = 0usize;
            for ch in &node.children {
                if matches!(ch.style.display, Display::None) {
                    continue;
                }
                if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if ch.is_pseudo_element() && ch.text.is_empty() {
                    continue;
                }
                if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                    continue;
                }
                let child_font = ch.style.font_size_px(font_px, root_font_px);
                let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
                let child_outer = child_rbox.padding_left
                    + child_rbox.padding_right
                    + child_rbox.border_left
                    + child_rbox.border_right
                    + child_rbox.margin_left
                    + child_rbox.margin_right;
                if count > 0 {
                    total += gap;
                }
                total += self.min_content_width(ch, font_px, root_font_px) + child_outer;
                count += 1;
            }
            return total;
        }

        // For containers: max of children's min-content widths
        let mut max_w = generated_inline_w.max(own_text_w);
        for ch in &node.children {
            if matches!(ch.style.display, Display::None) {
                continue;
            }
            if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            if ch.is_pseudo_element() && ch.text.is_empty() {
                continue;
            }
            let child_font = ch.style.font_size_px(font_px, root_font_px);
            let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
            let child_outer = child_rbox.padding_left
                + child_rbox.padding_right
                + child_rbox.border_left
                + child_rbox.border_right
                + child_rbox.margin_left
                + child_rbox.margin_right;
            let cw = self.min_content_width(ch, font_px, root_font_px) + child_outer;
            if cw > max_w {
                max_w = cw;
            }
        }
        max_w
    }

    pub fn max_content_width(&self, node: &WebCore, parent_font_px: f32, root_font_px: f32) -> f32 {
        self.max_content_width_inner(node, parent_font_px, root_font_px, true)
    }

    /// Max-content width with the element's own `width` ignored, which is what
    /// `flex-basis: content` asks for (Flexbox §7.2.3): size from the content
    /// and disregard the specified size.
    pub fn max_content_width_of_content(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.max_content_width_inner(node, parent_font_px, root_font_px, false)
    }

    fn max_content_width_inner(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
        honor_width: bool,
    ) -> f32 {
        if matches!(node.style.display, Display::None) {
            return 0.0;
        }

        // Explicit width → use that directly (but skip percentages — they can't
        // resolve without a known containing width during intrinsic measurement).
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        if honor_width
            && !node.style.width.is_auto()
            && !matches!(node.style.width, CssLength::Percent(_))
        {
            let w = self.res_len(&node.style.width, font_px, 0.0, root_font_px);
            // ⛔ A `border-box` width ALREADY contains the padding and border,
            // and every caller adds those again on top of what we return — the
            // contract here is a CONTENT width. Handing back the raw value made
            // a shrink-to-fit parent wider by exactly the child's padding and
            // border, on the near-universal `* { box-sizing: border-box }`.
            if node.style.box_sizing == BoxSizing::BorderBox {
                let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
                let edges = rb.padding_left + rb.padding_right + rb.border_left + rb.border_right;
                return (w - edges).max(0.0);
            }
            return w.max(0.0);
        }

        // css-sizing-4 §4: a box with a preferred aspect ratio and a DEFINITE
        // block size transfers that size through the ratio — in BOTH
        // directions. Only the width→height direction was implemented, so
        // `display:inline-block; aspect-ratio:2/1; height:100px` measured as
        // zero wide and collapsed, instead of 200.
        if let Some(w) = self.aspect_ratio_transferred_width(node, font_px, root_font_px) {
            return w;
        }

        // Replaced elements: the size they are shown at, ratio included.
        if let Some(w) = self.replaced_intrinsic_width(node, font_px, root_font_px) {
            return w;
        }

        // **An `<input>` button is sized by its LABEL, which is an attribute.**
        //
        // `submit`/`reset`/`button` are void elements: no children, no line
        // boxes, so the walk below measures nothing and an auto-width button
        // collapsed to its padding — a pill a few pixels wide with the label
        // spilling out beside it. `<button>` is unaffected because its label is
        // real content and gets measured like any other text.
        //
        // The UA-supplied default matters here too: a bare `<input
        // type=submit>` reads "Submit" in a browser and must be sized for that
        // word, not for the empty string.
        if node.tag == "input" {
            let input_type = node
                .attributes
                .get("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(input_type.as_str(), "submit" | "reset" | "button") {
                let label = match node.attributes.get("value").map(String::as_str) {
                    Some(v) if !v.is_empty() => v,
                    _ => match input_type.as_str() {
                        "submit" => "Submit",
                        "reset" => "Reset",
                        _ => "",
                    },
                };
                return self.measure_text_cached(
                    label,
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                );
            }
        }

        // Custom component: use cached dimensions (like a replaced element)
        if self.component_registry.get_component(&node.tag).is_some()
            || self.component_registry.map.contains_key(&node.tag)
        {
            return if node.component_width > 0.0 {
                node.component_width
            } else {
                if let Some(c) = self.component_registry.get_component(&node.tag) {
                    c.measure(node, f32::MAX).0
                } else if let Some(cb) = self.component_registry.map.get(&node.tag) {
                    (cb.measure)(node, f32::MAX).0
                } else {
                    0.0
                }
            };
        }

        let _rbox = self.res_box(&node.style, font_px, 0.0, root_font_px);
        let generated_inline_w = self.generated_inline_content_width(node, font_px, root_font_px);
        let own_text_w = self.max_content_width_of_direct_text(node, font_px, root_font_px);

        // Text node or pseudo-element (::before/::after) with direct text content.
        let is_pseudo = matches!(node.tag.as_str(), "::before" | "::after");
        let has_direct_text = node.is_text_node() || (is_pseudo && !node.text.is_empty());
        if has_direct_text {
            let text = &node.text;
            if text.is_empty() {
                return 0.0;
            }
            // Collapse whitespace for normal white-space mode (CSS §4.1.1)
            let text = if matches!(
                node.style.white_space,
                WhiteSpace::Normal | WhiteSpace::Nowrap
            ) {
                let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                collapsed
            } else {
                text.clone()
            };
            if text.is_empty() {
                return 0.0;
            }
            let letter_spacing = node
                .style
                .letter_spacing
                .resolve(font_px, 0.0, root_font_px);
            let word_spacing = node.style.word_spacing.resolve(font_px, 0.0, root_font_px);
            let w = if letter_spacing != 0.0 || word_spacing != 0.0 {
                text::measure_text_with_spacing(&text, font_px, letter_spacing, word_spacing, None)
            } else {
                self.measure_text_cached(
                    &text,
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                )
            };
            return w;
        }

        let is_row_flex = matches!(node.style.display, Display::Flex | Display::InlineFlex)
            && matches!(
                node.style.flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );
        let _is_col_flex =
            matches!(node.style.display, Display::Flex | Display::InlineFlex) && !is_row_flex;

        if is_row_flex {
            // Row flex: sum of children's max-content widths + their box model.
            let mut total = 0.0f32;
            let gap = self.res_len(&node.style.column_gap, font_px, 0.0, root_font_px);
            let mut count = 0usize;
            for ch in &node.children {
                if matches!(ch.style.display, Display::None) {
                    continue;
                }
                if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if ch.is_pseudo_element() && ch.text.is_empty() {
                    continue;
                }
                if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                    continue;
                }
                let child_font = ch.style.font_size_px(font_px, root_font_px);
                let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
                let child_outer = child_rbox.padding_left
                    + child_rbox.padding_right
                    + child_rbox.border_left
                    + child_rbox.border_right
                    + child_rbox.margin_left
                    + child_rbox.margin_right;
                // Use flex-basis if it gives a definite length. `content` says
                // to measure the content, and a percentage has nothing to
                // resolve against during intrinsic measurement, so both fall
                // through to the content measurement rather than reading 0.
                let basis_is_definite = !ch.style.flex_basis.is_auto()
                    && !matches!(
                        ch.style.flex_basis,
                        CssLength::Content | CssLength::Percent(_)
                    );
                let mut child_main = if basis_is_definite {
                    self.res_len(&ch.style.flex_basis, child_font, 0.0, root_font_px)
                        .max(0.0)
                } else if matches!(ch.style.flex_basis, CssLength::Content) {
                    self.max_content_width_of_content(ch, font_px, root_font_px)
                } else {
                    self.max_content_width(ch, font_px, root_font_px)
                };
                // ⛔ A GROWABLE ITEM CONTRIBUTES ITS MAX-CONTENT SIZE. `flex-basis`
                // is where the distribution STARTS, not a ceiling on what the
                // container needs to be (css-flexbox-1 §9.9). Reading a
                // `flex: 1 1 0` item's basis as its contribution made it count for
                // nothing, so `width: max-content` on the container came out as
                // wide as the remaining items alone.
                if ch.style.flex_grow > 0.0 {
                    let mc = self.max_content_width(ch, font_px, root_font_px);
                    if mc > child_main {
                        child_main = mc;
                    }
                }
                // The item's own minimum still floors that contribution.
                if !ch.style.min_width.is_auto() {
                    let mw = self.res_len(&ch.style.min_width, child_font, 0.0, root_font_px);
                    if mw > child_main {
                        child_main = mw;
                    }
                }
                let contribution = if ch.style.is_inline_level() {
                    (child_main + child_outer).ceil() + 1.0
                } else {
                    child_main + child_outer
                };
                total += contribution;
                if count > 0 {
                    total += gap;
                }
                count += 1;
            }
            return total;
        }

        // Column flex or block: max of children's max-content widths.
        // Exception: floated children sit side by side, so sum their widths.
        // ⛔ INLINE SIBLINGS SUM, BLOCK SIBLINGS MAX. max-content is the content
        // laid out with NO soft wrap opportunity taken (css-sizing-3 §5.1), so
        // everything that shares a line contributes its width to that line. A
        // plain max over all children measured `Hello <b>World</b>` as the wider
        // single word, and any shrink-to-fit box sized from it then wrapped.
        let mut max_w = generated_inline_w;
        let mut float_sum = 0.0f32;
        let mut run = generated_inline_w + own_text_w; // the inline run being accumulated
        let mut pending_collapsed_space = false;
        for ch in &node.children {
            if matches!(ch.style.display, Display::None) {
                continue;
            }
            if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            if ch.is_pseudo_element() && ch.text.is_empty() {
                continue;
            }
            if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                if run > 0.0 {
                    pending_collapsed_space = true;
                }
                continue;
            }
            let child_font = ch.style.font_size_px(font_px, root_font_px);
            let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
            let child_outer = child_rbox.padding_left
                + child_rbox.padding_right
                + child_rbox.border_left
                + child_rbox.border_right
                + child_rbox.margin_left
                + child_rbox.margin_right;
            let mut cw = self.max_content_width(ch, font_px, root_font_px) + child_outer;
            if ch.style.is_inline_level()
                && !ch.is_pseudo_element()
                && ch.layout.border_rect.w > 0.0
            {
                let measured_outer =
                    ch.layout.border_rect.w + child_rbox.margin_left + child_rbox.margin_right;
                if measured_outer > cw {
                    cw = measured_outer;
                }
            }
            if ch.style.is_inline_level() {
                cw = cw.ceil() + 1.0;
            }
            if !matches!(ch.style.float, Float::None) {
                float_sum += cw;
                continue;
            }
            if ch.tag == "br" {
                if run > max_w {
                    max_w = run;
                }
                run = 0.0;
                pending_collapsed_space = false;
                continue;
            }
            if ch.style.is_inline_level() {
                if pending_collapsed_space && cw > 0.0 {
                    run += self.measure_text_cached(
                        " ",
                        font_px,
                        node.style.font_weight,
                        node.style.font_style,
                        &node.style.font_family,
                    );
                }
                pending_collapsed_space = false;
                run += cw;
            } else {
                // A block-level child ends the current line and owns its own.
                if run > max_w {
                    max_w = run;
                }
                run = 0.0;
                pending_collapsed_space = false;
                if cw > max_w {
                    max_w = cw;
                }
            }
        }
        if run > max_w {
            max_w = run;
        }
        // Container must be wide enough for both floats and normal flow
        max_w.max(float_sum)
    }

    fn generated_inline_content_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.generated_content_width(
            &node.style.before_content,
            node.style.before_style.as_deref(),
            font_px,
            root_font_px,
        ) + self.generated_content_width(
            &node.style.after_content,
            node.style.after_style.as_deref(),
            font_px,
            root_font_px,
        )
    }

    fn min_content_width_of_direct_text(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.is_text_node() || node.text.is_empty() {
            return 0.0;
        }
        let (letter_spacing, word_spacing) = (
            node.style
                .letter_spacing
                .resolve(font_px, 0.0, root_font_px),
            node.style.word_spacing.resolve(font_px, 0.0, root_font_px),
        );
        node.text
            .split(|c: char| c.is_ascii_whitespace())
            .filter(|word| !word.is_empty())
            .map(|word| {
                if letter_spacing != 0.0 || word_spacing != 0.0 {
                    text::measure_text_with_spacing(
                        word,
                        font_px,
                        letter_spacing,
                        word_spacing,
                        None,
                    )
                } else {
                    self.measure_text_cached(
                        word,
                        font_px,
                        node.style.font_weight,
                        node.style.font_style,
                        &node.style.font_family,
                    )
                }
            })
            .fold(0.0_f32, f32::max)
    }

    fn max_content_width_of_direct_text(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.is_text_node() || node.text.is_empty() {
            return 0.0;
        }
        let text = if matches!(
            node.style.white_space,
            WhiteSpace::Normal | WhiteSpace::Nowrap
        ) {
            node.text.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            node.text.clone()
        };
        if text.is_empty() {
            return 0.0;
        }
        let letter_spacing = node
            .style
            .letter_spacing
            .resolve(font_px, 0.0, root_font_px);
        let word_spacing = node.style.word_spacing.resolve(font_px, 0.0, root_font_px);
        if letter_spacing != 0.0 || word_spacing != 0.0 {
            text::measure_text_with_spacing(&text, font_px, letter_spacing, word_spacing, None)
        } else {
            self.measure_text_cached(
                &text,
                font_px,
                node.style.font_weight,
                node.style.font_style,
                &node.style.font_family,
            )
        }
    }

    fn generated_content_width(
        &self,
        content: &str,
        style: Option<&ComputedStyle>,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        let Some(style) = style else {
            if content.is_empty() {
                return 0.0;
            }
            return self.measure_text_cached(
                content,
                parent_font_px,
                FontWeight::Normal,
                FontStyle::Normal,
                "",
            );
        };
        let font_px = style.font_size_px(parent_font_px, root_font_px);
        let rb = self.res_box(style, font_px, 0.0, root_font_px);
        let inline_width_ignored = matches!(style.display, Display::Inline);
        let content_w = if !inline_width_ignored
            && !style.width.is_auto()
            && !matches!(style.width, CssLength::Percent(_))
        {
            self.res_len(&style.width, font_px, 0.0, root_font_px)
        } else if content.is_empty() {
            0.0
        } else {
            self.measure_text_cached(
                content,
                font_px,
                style.font_weight,
                style.font_style,
                &style.font_family,
            )
        };
        content_w.max(0.0)
            + rb.margin_left
            + rb.border_left
            + rb.padding_left
            + rb.padding_right
            + rb.border_right
            + rb.margin_right
    }

    /// Kick off non-blocking font loading. Base64 and local fonts are loaded
    /// immediately; remote fonts are fetched in background threads and arrive
    /// via `pending_fonts` channel — polled each `layout()` call.
    pub fn load_font_faces(&mut self, faces: &[crate::css::FontFaceDecl], base_url: &str) {
        if let Some(fs_ptr) = self.font_system {
            let fs = unsafe { &mut *fs_ptr };

            // ── Phase 1: Resolve each @font-face to its best fetchable URL ──────
            let mut remote: Vec<(crate::css::FontFaceDecl, String)> = Vec::new();

            for face in faces {
                let mut found = false;
                let parsed_sources;
                let sources = if face.sources.is_empty() {
                    parsed_sources = crate::css::font_face::parse_font_face_sources(&face.src);
                    parsed_sources.as_slice()
                } else {
                    face.sources.as_slice()
                };
                for source in sources {
                    if found {
                        break;
                    }
                    if !font_source_formats_supported(source) {
                        continue;
                    };
                    if !font_source_techs_supported(source) {
                        continue;
                    }
                    let url_inner = match &source.kind {
                        crate::css::FontFaceSourceKind::Local(name) => {
                            found = load_local_font_face(fs, face, name);
                            continue;
                        }
                        crate::css::FontFaceSourceKind::Url(url) => url.as_str(),
                    };

                    // Strip fragment (#iefix etc.)
                    let url_clean = url_inner.split('#').next().unwrap_or(url_inner);
                    // Strip query string for extension check
                    let url_for_ext = url_clean.split('?').next().unwrap_or(url_clean);

                    // Skip the formats that are genuinely unreadable. `.eot`
                    // is IE-only and `.svg` fonts were removed from browsers;
                    // `.woff2` is decoded (see `load_font_bytes`).
                    if url_for_ext.ends_with(".eot") || url_for_ext.ends_with(".svg") {
                        continue;
                    }

                    // Base64 data URI — load immediately (no network)
                    if let Some(b64) = url_inner
                        .strip_prefix("data:")
                        .and_then(|s| s.find(";base64,").map(|i| &s[i + 8..]))
                    {
                        if let Ok(bytes) = decode_base64(b64.trim()) {
                            load_font_face_bytes(fs, face, bytes);
                            found = true;
                        }
                        continue;
                    }

                    let resolved = crate::html::resolve_url(url_clean, base_url);

                    if resolved.starts_with("http://") || resolved.starts_with("https://") {
                        remote.push((face.clone(), resolved));
                        found = true;
                    } else if !resolved.is_empty() {
                        // Local file — load immediately.
                        //
                        // ⛔ `file://` has to come off first: `resolve_url`
                        // returns a URL, and `fs::read` wants a path. A
                        // `@font-face` pointing at a local file silently
                        // loaded nothing, and the text was measured in the
                        // fallback font instead.
                        let path = resolved.strip_prefix("file://").unwrap_or(&resolved);
                        if let Ok(data) = std::fs::read(path) {
                            load_font_face_bytes(fs, face, data);
                            found = true;
                        }
                    }
                }
            }

            // ── Phase 2: Fire-and-forget remote font fetches ────────────────────
            if !remote.is_empty() {
                let (tx, rx) = std::sync::mpsc::channel::<(crate::css::FontFaceDecl, Vec<u8>)>();
                let in_flight = self.fonts_in_flight.clone();
                in_flight.store(remote.len(), std::sync::atomic::Ordering::SeqCst);

                for (face, url) in remote {
                    let sender = tx.clone();
                    let counter = in_flight.clone();
                    std::thread::spawn(move || {
                        let result = crate::http_client()
                            .get(&url)
                            .send()
                            .ok()
                            .and_then(|r| r.bytes().ok())
                            .map(|b| b.to_vec())
                            .filter(|b| !b.is_empty());
                        if let Some(bytes) = result {
                            eprintln!(
                                "  Font loaded: {} ({} bytes) from {}",
                                face.family,
                                bytes.len(),
                                &url[..url.len().min(80)]
                            );
                            let _ = sender.send((face, bytes));
                        } else {
                            eprintln!("  Font fetch failed: {}", face.family);
                        }
                        counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    });
                }
                self.pending_fonts = Some(rx);
            }
        }
    }

    /// Poll for fonts that have arrived from background threads.
    /// Returns `true` if any new fonts were loaded (caller should re-layout).
    pub fn poll_pending_fonts(&mut self) -> bool {
        let rx = match self.pending_fonts.as_ref() {
            Some(rx) => rx,
            None => return false,
        };
        let fs = match self.font_system {
            Some(ptr) => unsafe { &mut *ptr },
            None => return false,
        };

        let mut loaded_any = false;
        // Drain all available font data without blocking.
        while let Ok((face, bytes)) = rx.try_recv() {
            loaded_any |= load_font_face_bytes(fs, &face, bytes);
        }

        // If all fetches are done, drop the receiver.
        if self
            .fonts_in_flight
            .load(std::sync::atomic::Ordering::SeqCst)
            == 0
        {
            self.pending_fonts = None;
        }

        loaded_any
    }

    /// Returns `true` if there are still font fetches in flight.
    pub fn has_pending_fonts(&self) -> bool {
        self.pending_fonts.is_some()
    }

    /// Main entry point: layout the full document.
    pub fn layout(&mut self, doc: &mut Document, viewport_width: f32) {
        self.viewport_w = viewport_width;
        // Use the document's viewport_h if it was set during load_html (it knows
        // the real window height); only fall back to the engine default if the
        // document doesn't have a value yet.
        // Only use doc's viewport_h as fallback if the engine's hasn't been set.
        if self.viewport_h <= 0.0 && doc.viewport_h > 0.0 {
            self.viewport_h = doc.viewport_h;
        }
        // Keep viewport in doc so focus-change recascades use the correct size.
        doc.viewport_w = self.viewport_w;
        doc.viewport_h = self.viewport_h;
        // Compute root font-size from the <html> element's computed style.
        // Used for `rem` unit resolution throughout layout.
        // ⛔ The cascade resolves `<html>`'s own font-size, so its base must be
        // the INITIAL size — not a value derived from that same element. This
        // read `<html>`'s style first and fed the result back in as the base,
        // so `html { font-size: 62.5% }` — the standard "1rem = 10px" idiom —
        // resolved to 62.5% of 10 = 6.25px, and every later layout applied the
        // percentage again: 3.9, 2.4, down to the 1px floor. Every `rem` on the
        // page collapsed with it, and with them every line height and box.
        // The cascade publishes the resolved root size itself (see the `html`
        // branch in `cascade.rs`), and layout picks it up below.
        let cascade_root_px = self.root_font_px;
        let root_font_px = cascade_root_px;

        // Rebuild selector index if rules changed (lazy, skips if already up-to-date).
        doc.stylesheet.rebuild_index();

        // Load @font-face fonts (non-blocking — remote fonts arrive via poll_pending_fonts).
        if !self.fonts_loaded && !doc.stylesheet.font_faces.is_empty() {
            self.load_font_faces(&doc.stylesheet.font_faces, &doc.base_url);
            self.fonts_loaded = true;
        }

        // Cache @media / @container presence so we don't O(n)-scan rules every layout.
        self.cached_has_media_q = doc
            .stylesheet
            .rules
            .iter()
            .any(|r| !r.media_condition.is_empty());
        self.cached_has_container_q = doc
            .stylesheet
            .rules
            .iter()
            .any(|r| !r.container_condition.is_empty());

        // Skip the CSS cascade on resize when nothing media-query-relevant changed.
        let needs_cascade = self.last_cascade_vw.is_nan()
            || (self.cached_has_media_q
                && doc.stylesheet.rules.iter().any(|r| {
                    !r.media_condition.is_empty()
                        && crate::css::evaluate_media(
                            &r.media_condition,
                            self.last_cascade_vw,
                            self.viewport_h,
                        ) != crate::css::evaluate_media(
                            &r.media_condition,
                            viewport_width,
                            self.viewport_h,
                        )
                }));

        let hover_changed = doc.hover_changed;
        doc.hover_changed = false;
        let dom_style_dirty = doc.style_dirty;
        doc.style_dirty = false;

        perf::start_phase();
        let did_cascade = if needs_cascade || dom_style_dirty {
            // Full cascade needed (initial, viewport change, etc.)
            let hover_chain = crate::css::build_hover_chain(&doc.root, doc.hovered_box);
            let target_id = doc.fragment_target_id();
            crate::css::apply_cascade_vp_hover_target_url(
                &mut doc.root,
                &doc.stylesheet,
                None,
                root_font_px,
                self.viewport_w,
                self.viewport_h,
                doc.focused_box,
                doc.keyboard_focus,
                &hover_chain,
                target_id,
                &doc.base_url,
            );
            // Clear any leftover dirty flags after full cascade
            crate::css::clear_cascade_dirty(&mut doc.root);
            self.last_cascade_vw = viewport_width;
            doc.prev_hovered_box = doc.hovered_box;
            // Build hover invalidation set: collect node_ids of all elements with hover_style
            doc.hover_sensitive_nodes.clear();
            collect_hover_sensitive(&doc.root, &mut doc.hover_sensitive_nodes);
            true
        } else if hover_changed {
            // Incremental hover cascade — only re-cascade elements affected by
            // the hover change (old chain + new chain), skip everything else.
            let old_chain = crate::css::build_hover_chain(&doc.root, doc.prev_hovered_box);
            let new_chain = crate::css::build_hover_chain(&doc.root, doc.hovered_box);
            // Mark dirty flags on nodes affected by hover change
            // ⛔ The stylesheet's own flag, not a hardcoded `false`. It is set
            // when a rule styles a DESCENDANT of a hovered element — `li:hover
            // div`, which is how essentially every dropdown menu is built.
            // Passing `false` meant those descendants were never marked dirty,
            // so the incremental hover cascade skipped them and the panel never
            // opened. Hovering the element itself worked, which is why simple
            // `a:hover` colour changes looked fine while no menu did.
            crate::css::mark_hover_dirty(
                &mut doc.root,
                &old_chain,
                &new_chain,
                doc.stylesheet.has_hover_descendant_rules,
                &doc.hover_sensitive_nodes,
            );

            crate::css::apply_cascade_incremental(
                &mut doc.root,
                &doc.stylesheet,
                None,
                root_font_px,
                self.viewport_w,
                self.viewport_h,
                doc.focused_box,
                doc.keyboard_focus,
                &new_chain,
            );
            crate::css::clear_cascade_dirty(&mut doc.root);
            doc.hover_suppress_count = 1;
            doc.prev_hovered_box = doc.hovered_box;
            true
        } else {
            false
        };

        // `rem` in LAYOUT resolves against what the root actually computed to,
        // which the cascade has now written as an absolute length.
        let root_font_px = match doc.root.style.font_size {
            crate::types::CssLength::Px(v) if v > 0.0 => v,
            _ => cascade_root_px,
        };

        // ── CSS animation / transition runtime ─────────────────────────────
        let now = std::time::Instant::now();
        doc.sync_animations(now);
        if did_cascade || hover_changed {
            doc.sync_transitions(now, did_cascade);
        }
        doc.tick_animations(now);
        let svg_animations_running = crate::svg::tick_svg_animations(&mut doc.root, now);
        if svg_animations_running {
            doc.needs_animation_frame = true;
        }
        doc.tick_media(now);
        doc.tick_smooth_scrolls(now);
        if !doc.animation_overrides.is_empty() {
            let overrides = doc.animation_overrides.clone();
            crate::css::apply_animation_overrides(&mut doc.root, &overrides);
        }
        // ──────────────────────────────────────────────────────────────────

        // Progressive layout is disabled for now — it causes blank content
        // below the viewport. Full layout runs in a single pass.
        // TODO: implement proper deferred layout with background completion.
        perf::end_cascade();
        self.initial_layout_done = true;
        self.layout_geometry(doc, viewport_width, root_font_px);
        self.last_geometry_viewport_h = self.viewport_h;

        // Container query post-pass: now that box sizes are known, apply @container rules
        // whose conditions match the computed dimensions of container ancestors, then
        // re-layout until dependent container sizes settle.
        if self.cached_has_container_q {
            for _ in 0..4 {
                let changed = crate::css::apply_container_cascade_tree(
                    &mut doc.root,
                    &doc.stylesheet,
                    &[],
                    &[],
                    0,
                    1,
                    0,
                    1,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                    doc.focused_box,
                    doc.keyboard_focus,
                );
                if !changed {
                    break;
                }
                // Re-apply animation overrides after container-query cascade.
                if !doc.animation_overrides.is_empty() {
                    let overrides = doc.animation_overrides.clone();
                    crate::css::apply_animation_overrides(&mut doc.root, &overrides);
                }
                self.layout_geometry(doc, viewport_width, root_font_px);
                self.last_geometry_viewport_h = self.viewport_h;
            }
        }

        // Detect aria-live region changes and queue announcements.
        doc.check_live_regions();
    }

    /// Force the next `layout()` call to re-run the full CSS cascade.
    ///
    /// Call this after DOM mutations (adding/removing elements, changing classes or
    /// inline styles) so the skip-cascade optimisation does not hide the change.
    pub fn invalidate_cascade(&mut self) {
        self.last_cascade_vw = f32::NAN;
    }

    /// Layout without re-running the CSS cascade.
    ///
    /// Use this when only text content changed (e.g. keystrokes in an editable
    /// element).  Skipping the cascade saves CSS selector matching across every
    /// element in the tree; the line-cache early-stop then skips unchanged lines.
    /// Progressive layout: lay out only above-fold content first, return true
    /// if there's more to do below the fold. Caller should render, then call
    /// `layout_remainder()` to finish.
    pub fn layout_above_fold(&mut self, doc: &mut Document, viewport_width: f32) -> bool {
        self.progressive_cutoff = self.viewport_h * 1.5;
        self.layout_geometry(doc, viewport_width, self.root_font_px);
        self.progressive_cutoff = 0.0;
        // Check if document is taller than what we laid out
        doc.root.layout.margin_rect.h > self.viewport_h * 1.5
    }

    /// Finish layout for below-fold content (call after rendering first screen).
    pub fn layout_remainder(&mut self, doc: &mut Document, viewport_width: f32) {
        // Mark everything below the fold as dirty so it gets laid out
        self.progressive_cutoff = 0.0;
        doc.root.layout.layout_dirty = true;
        self.layout_geometry(doc, viewport_width, self.root_font_px);
        self.last_geometry_viewport_h = self.viewport_h;
    }

    pub fn layout_no_cascade(&mut self, doc: &mut Document, viewport_width: f32) {
        self.viewport_w = viewport_width;
        let root_font_px = self.root_font_px;
        self.layout_geometry(doc, viewport_width, root_font_px);
        self.last_geometry_viewport_h = self.viewport_h;
    }

    fn layout_geometry(&self, doc: &mut Document, viewport_width: f32, root_font_px: f32) {
        self.layout_calls.set(0);
        self.layout_start.set(Some(std::time::Instant::now()));
        perf::reset();
        perf::start_phase();

        // Resolve shadow DOM slots before layout (only if any shadow roots exist)
        if has_shadow_roots(&doc.root) {
            resolve_all_slots(&mut doc.root);
        }

        // Layout runs on the render tree in place. The structural corrections
        // a separate fragment tree would exist to make — anonymous blocks for
        // mixed block/inline children, `display: contents`, `::before`/
        // `::after`, flex/grid blockification — are all made here, and all
        // four were checked against Chrome and agree.
        propagate_dirty(&mut doc.root);

        // Set up root geometry
        let rbox = self.res_box(&doc.root.style, root_font_px, viewport_width, root_font_px);
        let content_w = rbox.content_width.unwrap_or(viewport_width);
        doc.root.layout.content_rect.w = content_w;
        doc.root.layout.padding_rect.w = content_w;
        doc.root.layout.border_rect.w = content_w;
        doc.root.layout.margin_rect.w = content_w;
        // Always mark root dirty — layout_geometry is only called when
        // cascade ran or layout is explicitly requested. The incremental
        // optimization happens INSIDE layout_box via subtree pruning.
        doc.root.layout.layout_dirty = true;

        // When viewport height changed, mark all nodes dirty
        if (self.viewport_h - self.last_geometry_viewport_h).abs() > 0.5
            && !self.last_geometry_viewport_h.is_nan()
        {
            fn mark_all_dirty(n: &mut crate::types::WebCore) {
                n.layout.layout_dirty = true;
                n.layout.intrinsic_dirty = true;
                n.has_dirty_layout_descendant = true;
                for c in &mut n.children {
                    mark_all_dirty(c);
                }
            }
            mark_all_dirty(&mut doc.root);
        }

        self.pos_cb
            .set(Rect::new(0.0, 0.0, content_w, self.viewport_h));
        self.fixed_cb
            .set(Rect::new(0.0, 0.0, content_w, self.viewport_h));
        // **The root's containing block is the VIEWPORT, height included.**
        // CSS 2.1 §10.1: the initial containing block has the viewport's
        // dimensions. Passing only the width made `html { height: 100% }`
        // resolve to `auto`, and since a percentage height is auto whenever its
        // containing block's is, the whole chain below it collapsed — which is
        // every app shell ever written.
        let root_c = if self.viewport_h > 0.0 {
            Constraints::with_height(
                content_w,
                self.viewport_h,
                0.0,
                0.0,
                root_font_px,
                root_font_px,
            )
        } else {
            Constraints::new(content_w, 0.0, 0.0, root_font_px, root_font_px)
        };
        self.layout_box(&mut doc.root, &root_c);

        // Update root geometry with final height
        let h = doc.root.layout.margin_rect.h;
        doc.root.layout.content_rect.h = h;
        doc.root.layout.padding_rect.h = h;
        doc.root.layout.border_rect.h = h;

        perf::end_layout();
        perf::set_counts(
            count_nodes(&doc.root) as u32,
            doc.stylesheet.rules.len() as u32,
        );

        // Clear descendant dirty flags now that layout is complete
        crate::css::clear_descendant_dirty(&mut doc.root);

        // Rebuild O(1) node index (pointers stable until next mutation)
        doc.rebuild_node_index();

        // Bump generation so renderer knows to rebuild display list.
        //
        // ⚠ UNCONDITIONAL, and that costs a full display-list rebuild on every
        // frame a host repaints — which for a window on a 60Hz tick is every
        // frame, however still the page is. Gating it needs the signals
        // `layout()` has (`did_cascade`, the previous viewport) and this
        // function is `&self` and does not, so the fix is a real change to who
        // owns that decision rather than a condition bolted on here.
        doc.layout_generation = doc.layout_generation.wrapping_add(1);
    }

    pub fn layout_box(&self, node: &mut WebCore, c: &Constraints) -> f32 {
        self.layout_box_with_fc(node, c, None)
    }

    pub fn layout_box_with_fc(
        &self,
        node: &mut WebCore,
        c: &Constraints,
        fc: Option<&mut FloatContext>,
    ) -> f32 {
        let containing_w = c.available_width;
        let x = c.x;
        let y = c.y;
        let parent_font_px = c.parent_font_px;
        let root_font_px = c.root_font_px;
        // Guard against infinite layout loops.
        let calls = self.layout_calls.get();
        self.layout_calls.set(calls + 1);
        if calls > 5_000_000 {
            eprintln!("  [layout] ABORTING: >5M layout calls — infinite loop detected");
            node.layout.content_rect = Rect::new(x, y, containing_w, 0.0);
            node.layout.padding_rect = node.layout.content_rect;
            node.layout.border_rect = node.layout.content_rect;
            node.layout.margin_rect = node.layout.content_rect;
            return 0.0;
        }
        // Guard against stack overflow on deeply nested DOMs.
        let depth = self.layout_depth.get();
        if depth >= MAX_LAYOUT_DEPTH {
            node.layout.content_rect = Rect::new(x, y, containing_w, 0.0);
            node.layout.padding_rect = node.layout.content_rect;
            node.layout.border_rect = node.layout.content_rect;
            node.layout.margin_rect = node.layout.content_rect;
            return 0.0;
        }
        // Don't layout display:none
        if matches!(node.style.display, Display::None) {
            node.layout.content_rect = Rect::default();
            node.layout.padding_rect = Rect::default();
            node.layout.border_rect = Rect::default();
            node.layout.margin_rect = Rect::default();
            return 0.0;
        }

        // display:contents — the element itself generates no box.
        // Its children are promoted to the parent's formatting context.
        if matches!(node.style.display, Display::Contents) {
            node.layout.content_rect = Rect::default();
            node.layout.padding_rect = Rect::default();
            node.layout.border_rect = Rect::default();
            node.layout.margin_rect = Rect::default();
            return 0.0;
        }

        // Fast path: skip full layout if the containing width hasn't changed
        // and the node isn't dirty. Just reposition the cached result.
        // Also skip when viewport height changed (vh-dependent elements need re-layout).
        // Note: during layout_geometry, last_geometry_viewport_h still holds the OLD value.
        let vh_ok = (self.viewport_h - self.last_geometry_viewport_h).abs() < 0.5
            || self.last_geometry_viewport_h.is_nan();
        if !node.layout.layout_dirty
            && node.layout.last_containing_width > 0.0
            && (node.layout.last_containing_width - containing_w).abs() < 0.01
            && node.layout.margin_rect.w > 0.0
            && node.layout.margin_rect.h > 0.0
            && fc.is_none()
            && vh_ok
            && !matches!(node.style.display, Display::None | Display::Contents)
        {
            perf::record_layout_skip();
            let dx = x - node.layout.margin_rect.x;
            let dy = y - node.layout.margin_rect.y;
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                shift_rects(node, dx, dy);
            }
            self.layout_depth.set(depth);
            return node.layout.margin_rect.h;
        }

        self.layout_depth.set(depth + 1);

        perf::record_layout_call();
        perf::record_depth(depth as u32);
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);

        // <img>/<svg> aspect ratio: when one dimension is auto and the natural
        // dimensions are known, compute the auto dimension to preserve the
        // image's intrinsic aspect ratio (CSS Images §5.1).
        // For <svg>, intrinsic dimensions come from viewBox.
        let (has_intrinsic, iw, ih) = match self.intrinsic_dimensions(node) {
            Some((w, h)) => (true, w, h),
            None => (false, 0.0, 0.0),
        };
        // Compute intrinsic aspect ratio dimensions WITHOUT mutating style.
        // The resolved values are applied to rbox after resolve_box_vp.
        let (intrinsic_w_override, intrinsic_h_override) = if has_intrinsic {
            if node.style.width.is_auto() && !node.style.height.is_auto() {
                let h = node.style.height.resolve_vp(
                    font_px,
                    0.0,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                );
                let w = (h * iw / ih).round();
                (Some(w), None)
            } else if node.style.height.is_auto() && !node.style.width.is_auto() {
                let mut w = node.style.width.resolve_vp(
                    font_px,
                    containing_w,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                );
                let max_w = node.style.max_width.resolve_vp(
                    font_px,
                    containing_w,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                );
                if max_w > 0.0 && w > max_w {
                    w = max_w;
                }
                let h = (w * ih / iw).round();
                (None, Some(h))
            } else if node.style.width.is_auto() && node.style.height.is_auto() {
                let (mut w, mut h) = if node.tag == "svg" {
                    let default_w = 300.0;
                    let default_h = 150.0;
                    if containing_w > 0.0 && containing_w < default_w {
                        (containing_w, (containing_w * ih / iw).round())
                    } else {
                        (default_w, default_h)
                    }
                } else {
                    (iw, ih)
                };
                let max_w = node.style.max_width.resolve_vp(
                    font_px,
                    containing_w,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                );
                if max_w > 0.0 && w > max_w {
                    h = (max_w * ih / iw).round();
                    w = max_w;
                }
                let max_h = node.style.max_height.resolve_vp(
                    font_px,
                    0.0,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                );
                if max_h > 0.0 && h > max_h {
                    w = (max_h * iw / ih).round();
                    h = max_h;
                }
                (Some(w), Some(h))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let mut rbox = resolve_box_vp(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
            c.available_height,
        );
        if node.style.display == Display::Inline
            && has_block_children(node)
            && !node.style.width.is_auto()
        {
            let mut w = node
                .style
                .width
                .resolve_vp(
                    font_px,
                    containing_w,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                )
                .max(0.0);
            if node.style.box_sizing == BoxSizing::BorderBox {
                w = (w
                    - rbox.padding_left
                    - rbox.padding_right
                    - rbox.border_left
                    - rbox.border_right)
                    .max(0.0);
            }
            rbox.content_width = Some(w);
        }

        if let Some(w) = self.field_sizing_content_width(node, font_px) {
            rbox.content_width = Some(w);
        }
        if let Some(h) = self.field_sizing_content_height(node, font_px, root_font_px) {
            rbox.content_height = Some(h);
        }

        // Apply intrinsic aspect ratio overrides to rbox (not to style)
        if let Some(w) = intrinsic_w_override {
            rbox.content_width = Some(w);
        }
        if let Some(h) = intrinsic_h_override {
            rbox.content_height = Some(h);
        }

        self.clamp_resolved_content_width(&mut rbox, node, containing_w, font_px, root_font_px);

        // Apply forced dimensions from Constraints (used by flex layout).
        // These override style.width/height without mutating the DOM.
        if let Some(fw) = c.forced_width {
            rbox.content_width = Some(fw);
        }
        if let Some(fh) = c.forced_height {
            rbox.content_height = Some(fh);
        }

        // CSS 2.1 §10.5: when this element has a definite content height,
        // children with percentage heights need to resolve against it.
        // Instead of mutating child.style.height, we'll pass the parent's
        // content height via Constraints.available_height when laying out children.
        // This is handled by the layout functions (block, flex, grid) that
        // create child Constraints with available_height set.

        // Auto-margin centering (CSS 2.1 §10.3.3) — applies to any element with an
        // explicit width and at least one auto horizontal margin.  Block layout has
        // its own copy of this logic; here we handle flex/grid/table/custom.
        if let Some(content_w) = rbox.content_width {
            let left_auto = node.style.margin_left.is_auto();
            let right_auto = node.style.margin_right.is_auto();
            if left_auto || right_auto {
                let non_margin = rbox.border_left
                    + rbox.padding_left
                    + content_w
                    + rbox.padding_right
                    + rbox.border_right;
                let available = (containing_w - non_margin).max(0.0);
                if left_auto && right_auto {
                    let ml = (available / 2.0).floor();
                    rbox.margin_left = ml;
                    rbox.margin_right = available - ml;
                } else if left_auto {
                    rbox.margin_left = available - rbox.margin_right;
                } else {
                    rbox.margin_right = available - rbox.margin_left;
                }
            }
        }

        // ── Layout subtree pruning ────────────────────────────────────────────
        // If this box's resolved content width is identical to the previous
        // layout AND nothing is dirty AND there is no incoming float context
        // (which could alter line widths), the entire subtree produces exactly
        // the same geometry as before.  We just shift the cached rects to the
        // new position without re-running any layout algorithm.
        //
        // This is the dominant win on resize for fixed-width components nested
        // inside a fluid viewport (grid cards, sidebar items, etc.) — their
        // content width never changes even when the viewport grows or shrinks.
        // Also disable pruning when the viewport height changed so that vh-units
        // (e.g. height: 100vh) and flex-stretch heights dependent on the viewport
        // are recalculated rather than returning stale cached geometry.
        let viewport_h_unchanged = self.viewport_h == self.last_geometry_viewport_h;
        if fc.is_none()
            && !node.layout.layout_dirty
            && node.layout.resolved_content_width > 0.0
            && viewport_h_unchanged
        {
            let new_content_w = if let Some(cw) = rbox.content_width {
                cw
            } else {
                let outer = rbox.margin_left
                    + rbox.border_left
                    + rbox.padding_left
                    + rbox.border_right
                    + rbox.padding_right
                    + rbox.margin_right;
                (containing_w - outer).max(0.0)
            };
            // Also check the explicit content height hasn't changed.
            // This catches flex-stretch re-layouts where the parent mutates
            // child.style.height before calling layout_box a second time.
            let height_ok = match rbox.content_height {
                None => true, // auto height is determined by children — safe
                Some(h) => (h - node.layout.content_rect.h).abs() < 0.5,
            };
            if (new_content_w - node.layout.resolved_content_width).abs() < 0.5 && height_ok {
                // Content size is unchanged — just move the subtree.
                let dx = (x + rbox.margin_left) - node.layout.border_rect.x;
                let dy = (y + rbox.margin_top) - node.layout.border_rect.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    shift_rects(node, dx, dy);
                }
                node.layout.layout_dirty = false;
                node.layout.last_containing_width = containing_w;
                return node.layout.margin_rect.h;
            }
        }

        // Check for custom component — treated as replaced element with cached dimensions.
        // Only re-measure when the node is dirty (attribute changed, explicit invalidation).
        let is_trait_component = self.component_registry.get_component(&node.tag).is_some();
        let is_legacy_component =
            !is_trait_component && self.component_registry.map.contains_key(&node.tag);
        if is_trait_component || is_legacy_component {
            // Measure only on first layout or when dirty — cache the result
            if node.component_width == 0.0 || node.layout.layout_dirty {
                let (cw, ch) = if is_trait_component {
                    self.component_registry
                        .get_component(&node.tag)
                        .unwrap()
                        .measure(node, containing_w)
                } else {
                    let cb = self.component_registry.map.get(&node.tag).unwrap();
                    (cb.measure)(node, containing_w)
                };
                node.component_width = cw;
                node.component_height = ch;
            }
            let cw = node.component_width;
            let ch = node.component_height;
            let final_w = if node.style.width.is_auto() {
                cw
            } else {
                rbox.content_width.unwrap_or(cw)
            };
            let final_h = if node.style.height.is_auto() {
                ch
            } else {
                rbox.content_height.unwrap_or(ch)
            };
            block::build_box_rects(
                node,
                &rbox,
                x + rbox.margin_left + rbox.border_left + rbox.padding_left,
                y + rbox.margin_top + rbox.border_top + rbox.padding_top,
                final_w,
                final_h,
                rbox.margin_left,
                rbox.margin_right,
            );
            node.layout.layout_dirty = false;
            return node.layout.margin_rect.h;
        }

        // Track the nearest positioned ancestor's padding rect for abs children.
        let old_pos_cb = self.pos_cb.get();
        let old_fixed_cb = self.fixed_cb.get();
        // CSS spec: positioned elements AND elements with transform/filter/will-change
        // create a containing block for absolute/fixed descendants.
        if establishes_positioned_containing_block(&node.style) {
            let est_padding_x = x + rbox.margin_left + rbox.border_left;
            let est_padding_y = y + rbox.margin_top + rbox.border_top;
            let est_content_w = rbox
                .content_width
                .unwrap_or((containing_w - rbox.h_space()).max(0.0));
            let est_padding_w = est_content_w + rbox.padding_left + rbox.padding_right;
            self.pos_cb.set(Rect::new(
                est_padding_x,
                est_padding_y,
                est_padding_w,
                self.viewport_h,
            ));
            if establishes_fixed_positioned_containing_block(&node.style) {
                self.fixed_cb.set(Rect::new(
                    est_padding_x,
                    est_padding_y,
                    est_padding_w,
                    self.viewport_h,
                ));
            }
        }

        // Shadow DOM: layout reads `effective_children()`, which answers the
        // shadow tree when there is one. There used to be a swap here that
        // moved the shadow children into `node.children` "so all existing
        // layout code works unchanged" — but it emptied `shadow_root.children`
        // for the duration, and `effective_children()` reads exactly that. So
        // every caller of the accessor (the formatting-context dispatch,
        // `has_block_children`, block, grid) saw an EMPTY child list and laid
        // out nothing, while flex — which read `node.children` directly —
        // worked. Shadow DOM rendered nothing, and which paths were affected
        // depended on which accessor each happened to use.

        // Replaced elements (input, select, textarea, img) cannot be flex/grid
        // containers per CSS spec — blockify for dispatch WITHOUT mutating style.
        let is_button_input = node.tag == "input"
            && matches!(
                node.attributes.get("type").map(|s| s.as_str()),
                Some("submit") | Some("button") | Some("reset")
            );
        let effective_display = if !is_button_input
            && matches!(
                node.tag.as_str(),
                "input" | "select" | "textarea" | "img" | "video" | "canvas" | "iframe"
            ) {
            match node.style.display {
                Display::Flex | Display::Grid => Display::Block,
                Display::InlineFlex | Display::InlineGrid => Display::InlineBlock,
                other => other,
            }
        } else {
            node.style.display
        };

        // Build child constraints from resolved font size
        let child_c = Constraints::new(containing_w, x, y, font_px, root_font_px);

        let h = match effective_display {
            Display::Flex | Display::InlineFlex => flex::layout_flex(self, node, &rbox, &child_c),
            Display::Grid | Display::InlineGrid => grid::layout_grid(self, node, &rbox, &child_c),
            Display::Table => {
                let mut table_rbox = rbox;
                // In CSS 2.1 §17.5, padding does not apply to tables.
                table_rbox.padding_top = 0.0;
                table_rbox.padding_right = 0.0;
                table_rbox.padding_bottom = 0.0;
                table_rbox.padding_left = 0.0;
                // Handle margin:auto centering for tables
                let table_c = if !node.style.width.is_auto()
                    && (node.style.margin_left.is_auto() || node.style.margin_right.is_auto())
                {
                    let tw = self.res_len(&node.style.width, font_px, containing_w, root_font_px);
                    let non_margin = table_rbox.border_left
                        + tw
                        + table_rbox.border_right;
                    let available = (containing_w - non_margin).max(0.0);
                    let (ml, _mr) =
                        if node.style.margin_left.is_auto() && node.style.margin_right.is_auto() {
                            let ml = (available / 2.0).floor();
                            (ml, available - ml)
                        } else if node.style.margin_left.is_auto() {
                            (available - table_rbox.margin_right, table_rbox.margin_right)
                        } else {
                            (table_rbox.margin_left, available - table_rbox.margin_left)
                        };
                    Constraints::new(
                        containing_w,
                        x + ml - table_rbox.margin_left,
                        y,
                        font_px,
                        root_font_px,
                    )
                } else {
                    child_c
                };
                table::layout_table(self, node, &table_rbox, &table_c)
            }
            _ => {
                // Determine if children are block-level or inline-level.
                // Also use block layout when ALL non-abs, non-hidden children are
                // floated (no inline content to lay out). When floats and inline
                // content coexist, inline layout handles them via Float items.
                let children = node.effective_children();
                let has_any_inline = children.iter().any(|c| {
                    !matches!(c.style.display, Display::None)
                        && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                        && matches!(c.style.float, Float::None)
                        && c.style.is_inline_level()
                        && !(c.tag == "#text" && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
                });
                let has_only_floats = !has_any_inline
                    && children.iter().any(|c| {
                        !matches!(c.style.display, Display::None)
                            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                            && !matches!(c.style.float, Float::None)
                    });
                if has_block_children(node) || has_only_floats {
                    block::layout_block_with_fc(self, node, &rbox, &child_c, fc)
                } else {
                    // Pass parent float context so inline content wraps around
                    // floats from ancestor block containers (CSS §9.5).
                    inline_layout::layout_inline_block(self, node, &rbox, &child_c, fc)
                }
            }
        };

        self.pos_cb.set(old_pos_cb);
        self.fixed_cb.set(old_fixed_cb);
        self.layout_depth.set(depth);
        node.layout.layout_dirty = false;
        node.layout.intrinsic_dirty = false;
        node.layout.paint_dirty = false;
        node.has_dirty_layout_descendant = false;
        node.layout.last_containing_width = containing_w;
        h
    }

    /// Layout a box in inline context — returns (width, height, baseline).
    pub fn layout_inline(
        &self,
        node: &mut WebCore,
        max_w: f32,
        x: f32,
        y: f32,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> (f32, f32, f32) {
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        let _rbox = self.res_box(&node.style, font_px, max_w, root_font_px);

        let h = self.layout_box(
            node,
            &Constraints::new(max_w, x, y, parent_font_px, root_font_px),
        );
        let w = node.layout.border_rect.w;
        let baseline = node.layout.baseline;
        (w, h, baseline)
    }
}

// ─── Helper: does a box have any block-level children? ────────────────────────

/// Quick check if any node in the tree has a shadow root.
fn has_shadow_roots(node: &WebCore) -> bool {
    if node.shadow_root.is_some() {
        return true;
    }
    node.children.iter().any(|c| has_shadow_roots(c))
}

/// Walk the tree and resolve `<slot>` elements in all shadow roots.
fn resolve_all_slots(node: &mut WebCore) {
    node.resolve_slots();
    for child in &mut node.children {
        resolve_all_slots(child);
    }
    if let Some(ref mut sr) = node.shadow_root {
        for child in &mut sr.children {
            resolve_all_slots(child);
        }
    }
}

fn count_nodes(node: &WebCore) -> usize {
    1 + node.children.iter().map(|c| count_nodes(c)).sum::<usize>()
}

pub fn has_block_children(node: &WebCore) -> bool {
    node.effective_children().iter().any(|c| {
        if matches!(c.style.display, Display::None) {
            return false;
        }
        if matches!(c.style.display, Display::Contents) {
            return has_block_children(c);
        }
        matches!(
            c.style.position,
            Position::Static | Position::Relative | Position::Sticky
        ) && c.style.is_block_level()
            && matches!(c.style.float, Float::None)
    })
}

// ─── Absolute / fixed positioning pass ───────────────────────────────────────

pub fn layout_positioned(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    parent_font_px: f32,
    root_font_px: f32,
) {
    layout_positioned_static(
        engine,
        node,
        containing_rect,
        parent_font_px,
        root_font_px,
        None,
        None,
    );
}

/// Layout an absolutely/fixed positioned element, with optional static position.
/// `static_x` is the x offset (relative to containing block) where the element would
/// appear in normal flow — used when `left` and `right` are both `auto`.
/// `static_y` is the y offset (relative to containing block) where the element would
/// appear in normal flow — used when `top` and `bottom` are both `auto`.
pub fn layout_positioned_static(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    parent_font_px: f32,
    root_font_px: f32,
    static_x: Option<f32>,
    static_y: Option<f32>,
) {
    let font_px = node.style.font_size_px(parent_font_px, root_font_px);
    // By default the containing block is the passed containing_rect. For `fixed`
    // positioned elements, use the fixed-position containing block stack: the
    // viewport unless a transform/filter/contain/will-change ancestor captured it.
    let mut containing_w = containing_rect.w;
    let mut containing_h = containing_rect.h;
    let mut containing_x = containing_rect.x;
    let mut containing_y = containing_rect.y;
    if node.style.position == Position::Fixed {
        let fixed_cb = engine.fixed_cb.get();
        containing_w = fixed_cb.w;
        containing_h = fixed_cb.h;
        containing_x = fixed_cb.x;
        containing_y = fixed_cb.y;
    }

    let left_auto = node.style.left.is_auto();
    let right_auto = node.style.right.is_auto();
    let top_auto = node.style.top.is_auto();
    let bot_auto = node.style.bottom.is_auto();

    // If both horizontal sides are set AND width is auto, compute width from stretch.
    // If width is explicit (or the element has intrinsic size), don't stretch —
    // auto margins will center it instead (CSS 2.1 §10.3.7).
    let constrained_w = if !left_auto
        && !right_auto
        && node.style.width.is_auto()
        && !(node.tag == "img" && node.image_width > 0)
    {
        let l = node.style.left.resolve_vp(
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let r = node.style.right.resolve_vp(
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let rbox_inner = resolve_box_vp(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
            Some(containing_h),
        );
        let w = (containing_w - l - r - rbox_inner.inner_h_space()).max(0.0);
        Some(w)
    } else {
        None
    };

    let constrained_h = if !top_auto && !bot_auto {
        let t = node.style.top.resolve_vp(
            font_px,
            containing_h,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let b = node.style.bottom.resolve_vp(
            font_px,
            containing_h,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let rbox_inner = resolve_box_vp(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
            Some(containing_h),
        );
        let h = (containing_h - t - b - rbox_inner.inner_v_space()).max(0.0);
        Some(h)
    } else {
        None
    };

    // Pass containing_h through Constraints so layout_box can resolve
    // percentage heights without mutating style.
    let layout_w = constrained_w.unwrap_or(containing_w);
    let layout_c = if containing_h > 0.0 {
        Constraints::with_height(layout_w, containing_h, 0.0, 0.0, font_px, root_font_px)
    } else {
        Constraints::new(layout_w, 0.0, 0.0, font_px, root_font_px)
    };
    engine.layout_box(node, &layout_c);

    // Shrink-to-fit: width:auto absolutely-positioned elements wrap their content
    // (CSS 2.1 §10.3.7), just like floats — but only when width is not already
    // constrained by having both left and right set.
    if constrained_w.is_none() && node.style.width.is_auto() {
        let intrinsic_w = engine.max_content_width(node, font_px, root_font_px);
        if intrinsic_w > 0.0 && intrinsic_w < layout_w {
            let shrink_w = intrinsic_w
                + node.layout.resolved_pad_left
                + node.layout.resolved_pad_right
                + node.layout.resolved_border_left
                + node.layout.resolved_border_right
                + node.layout.resolved_margin_left
                + node.layout.resolved_margin_right;
            engine.layout_box(
                node,
                &Constraints::new(shrink_w, 0.0, 0.0, font_px, root_font_px),
            );
        }
    }

    // Now resolve position offsets
    let rbox = resolve_box_vp(
        &node.style,
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
        Some(containing_h),
    );
    let res_l = node.style.left.resolve_vp(
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_r = node.style.right.resolve_vp(
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_t = node.style.top.resolve_vp(
        font_px,
        containing_h,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_b = node.style.bottom.resolve_vp(
        font_px,
        containing_h,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );

    let x = if !left_auto
        && !right_auto
        && (node.style.margin_left.is_auto() || node.style.margin_right.is_auto())
        && !node.style.width.is_auto()
    {
        // Both left and right set with auto margins — center the element.
        // available = containing_w - left - right - border_box_w
        let avail = containing_w - res_l - res_r - node.layout.border_rect.w;
        if node.style.margin_left.is_auto() && node.style.margin_right.is_auto() {
            containing_x + res_l + (avail / 2.0).max(0.0)
        } else if node.style.margin_left.is_auto() {
            containing_x + res_l + avail.max(0.0) - rbox.margin_right
        } else {
            containing_x + res_l + rbox.margin_left
        }
    } else if !left_auto {
        if !right_auto && !node.style.width.is_auto() && node.style.direction == Direction::RTL {
            (containing_x + containing_w) - res_r - node.layout.border_rect.w - rbox.margin_right
        } else {
            containing_x + res_l + rbox.margin_left
        }
    } else if !right_auto {
        (containing_x + containing_w) - res_r - node.layout.border_rect.w - rbox.margin_right
    } else if let Some(abs_sx) = static_x {
        abs_sx + rbox.margin_left
    } else if let Some(abs_sx) = node.layout.abs_static_x {
        abs_sx + rbox.margin_left
    } else {
        if node.style.direction == Direction::RTL {
            (containing_x + containing_w) - node.layout.border_rect.w - rbox.margin_right
        } else {
            containing_x + rbox.margin_left
        }
    };

    let y = if !top_auto
        && !bot_auto
        && (node.style.margin_top.is_auto() || node.style.margin_bottom.is_auto())
        && !node.style.height.is_auto()
    {
        // Both top and bottom set with auto margins — center vertically
        let avail = containing_h - res_t - res_b - node.layout.border_rect.h;
        if node.style.margin_top.is_auto() && node.style.margin_bottom.is_auto() {
            containing_y + res_t + (avail / 2.0).max(0.0)
        } else if node.style.margin_top.is_auto() {
            containing_y + res_t + avail.max(0.0) - rbox.margin_bottom
        } else {
            containing_y + res_t + rbox.margin_top
        }
    } else if !top_auto {
        containing_y + res_t + rbox.margin_top
    } else if !bot_auto {
        (containing_y + containing_h) - res_b - node.layout.border_rect.h - rbox.margin_bottom
    } else if let Some(abs_sy) = static_y {
        // Static position: absolute document-space y where the element would
        // appear in normal flow. Already accounts for parent offsets.
        abs_sy + rbox.margin_top
    } else if let Some(abs_sy) = node.layout.abs_static_y {
        // Static position recorded on the node itself (set during parent's
        // inline/block layout for deeply nested absolute elements).
        abs_sy + rbox.margin_top
    } else {
        // Fallback: containing block content start.
        containing_y + rbox.margin_top
    };

    // Shift all rects to final position
    let dx = x - node.layout.border_rect.x;
    let dy = y - node.layout.border_rect.y;
    shift_rects(node, dx, dy);

    // If both sides set → we may need to re-layout with constrained size
    if let Some(cw) = constrained_w {
        if node.layout.content_rect.w != cw {
            engine.layout_box(
                node,
                &Constraints::new(layout_w, x, y, font_px, root_font_px),
            );
        }
    }

    // Apply constrained height when both top and bottom are set and height is auto.
    // Without this, inset:0 (top:0 bottom:0) leaves height at 0 because layout_box
    // has no content to fill the space.
    if let Some(ch) = constrained_h {
        if node.style.height.is_auto() && (node.layout.content_rect.h - ch).abs() > 0.5 {
            let diff = ch - node.layout.content_rect.h;
            node.layout.content_rect.h += diff;
            node.layout.padding_rect.h += diff;
            node.layout.border_rect.h += diff;
            node.layout.margin_rect.h += diff;
        }
    }
}

pub fn shift_rects(node: &mut WebCore, dx: f32, dy: f32) {
    node.layout.content_rect.x += dx;
    node.layout.content_rect.y += dy;
    node.layout.padding_rect.x += dx;
    node.layout.padding_rect.y += dy;
    node.layout.border_rect.x += dx;
    node.layout.border_rect.y += dy;
    node.layout.margin_rect.x += dx;
    node.layout.margin_rect.y += dy;
    for line in &mut node.layout.line_cache {
        line.x += dx;
        line.y += dy;
    }
    // `effective_children_mut`, not `children`: a shadow host's subtree lives in
    // `shadow_root.children`, which layout reaches through the accessor. Moving
    // a host by `children` alone left its shadow content where it was — a
    // parent that collapsed its first child's margin moved down and its shadow
    // child did not, landing 16px above the host it lives in.
    for child in node.effective_children_mut() {
        // Fixed-position elements are placed relative to the viewport,
        // not their parent — don't shift them when a parent moves.
        if child.style.position == Position::Fixed {
            continue;
        }
        shift_rects(child, dx, dy);
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}
