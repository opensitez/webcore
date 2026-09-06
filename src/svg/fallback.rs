//! SVG extraction and rasterization.

#![allow(unused_imports)]
use crate::css::*;
use crate::dom::attrs::AttrMap;
use crate::html::load_image_from_src;
use crate::types::*;

// ─── SVG extraction ────────────────────────────────────────────────────────

/// Pre-pass: extract `<svg>…</svg>` blocks and replace with `<img>` placeholders.
/// Returns (processed_html, map_of_key→svg_markup).
// SVG blocks are now handled inline by the tokenizer (collected as raw text)
// and rasterized by the parser when building the DOM tree.

/// Parse an SVG length that is definite at parse time.
///
/// SVG presentation attributes accept unitless lengths as px, and `px` lengths
/// are directly usable. Contextual lengths like `%`/`em` must be left to CSS
/// layout instead of being coerced by their numeric prefix.
pub(crate) fn parse_svg_length_px(s: &str) -> Option<f32> {
    let token = s
        .trim()
        .split(|c: char| c == ';' || c.is_whitespace())
        .next()
        .unwrap_or("");
    if token.is_empty() || token.ends_with('%') {
        return None;
    }
    let num_end = token
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '+' | '-'))
        .last()
        .map(|(idx, c)| idx + c.len_utf8())
        .unwrap_or(0);
    if num_end == 0 {
        return None;
    }
    let (num, unit) = token.split_at(num_end);
    if !unit.is_empty() && !unit.eq_ignore_ascii_case("px") {
        return None;
    }
    let px = num.parse::<f32>().ok()?;
    if px.is_finite() && px > 0.0 {
        Some(px)
    } else {
        None
    }
}

/// Parse definite pixels from a string like "20px", "512", "20px;height:10px".
pub(crate) fn parse_px(s: &str) -> Option<u32> {
    parse_svg_length_px(s).map(|n| n.round() as u32)
}

/// Extract a CSS pixel value for `prop` from an inline style string (e.g. "width:20px;height:20px").
pub(crate) fn style_px(style: &str, prop: &str) -> Option<u32> {
    let lower = style.to_ascii_lowercase();
    let needle = format!("{}:", prop);
    let idx = lower.find(&needle)?;
    let after = style[idx + needle.len()..].trim_start();
    parse_px(after)
}

/// Parse a viewBox attribute value "min-x min-y width height" → (width, height).
pub(crate) fn parse_viewbox_value(val: Option<&str>) -> Option<(u32, u32)> {
    let val = val?;
    let parts: Vec<f32> = val
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() >= 4 {
        Some((parts[2].round() as u32, parts[3].round() as u32))
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InlineSvgFallback {
    pub markup: String,
    pub viewbox_w: f32,
    pub viewbox_h: f32,
    pub explicit_w: Option<u32>,
    pub explicit_h: Option<u32>,
}

pub(crate) fn build_inline_svg_fallback(attrs: &AttrMap, body: &str) -> InlineSvgFallback {
    let mut svg_tag = String::from("<svg");
    for (k, v) in attrs.iter() {
        svg_tag.push_str(&format!(" {}=\"{}\"", k, v));
    }
    if !svg_tag.contains("xmlns=") {
        svg_tag.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    if body.contains("xlink:") && !svg_tag.contains("xmlns:xlink") {
        svg_tag.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    }
    svg_tag.push('>');

    let vb_str = attrs.get("viewBox").or_else(|| attrs.get("viewbox"));
    let (viewbox_w, viewbox_h) = parse_viewbox_value(vb_str.map(|s| s.as_str()))
        .map(|(w, h)| (w as f32, h as f32))
        .unwrap_or((0.0, 0.0));

    InlineSvgFallback {
        markup: format!("{}{}</svg>", svg_tag, body),
        viewbox_w,
        viewbox_h,
        explicit_w: attrs.get("width").and_then(|s| parse_px(s)),
        explicit_h: attrs.get("height").and_then(|s| parse_px(s)),
    }
}

/// Post-pass: walk the box tree, find `<img>` placeholders with `__svg_N__` src,
/// rasterize the SVG to RGBA pixel data, and store it on the box.
/// Post-cascade pass: load background images for elements whose
/// background-image URL was set by CSS rules (not just inline styles).
pub fn load_background_images(node: &mut WebCore, base_url: &str) {
    if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
        let url = node.style.background_image_url.clone();
        if let Some((data, w, h)) = load_image_from_src(&url, base_url) {
            node.bg_image_data = Some(std::sync::Arc::new(data));
            node.bg_image_width = w;
            node.bg_image_height = h;
        }
    }
    // Load mask-image (CSS masking for icons etc.)
    if node.mask_image_data.is_none() && !node.style.rare().mask_image_url.is_empty() {
        let url = node.style.rare().mask_image_url.clone();
        if let Some((data, w, h)) = load_image_from_src(&url, base_url) {
            node.mask_image_data = Some(std::sync::Arc::new(data));
            node.mask_image_width = w;
            node.mask_image_height = h;
        }
    }
    for child in &mut node.children {
        load_background_images(child, base_url);
    }
}

/// Rasterize an SVG string to RGBA pixel data using webcore's native SVG path.
pub fn rasterize_svg_to_rgba(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    crate::svg::paint::rasterize_svg_to_rgba(svg, width, height)
}

pub(crate) fn prepare_svg_for_rasterization(
    svg: &str,
    css_color: &str,
    fill: Option<Color>,
    stroke: Option<Color>,
) -> String {
    let mut out = svg
        .replace("currentColor", css_color)
        .replace("currentcolor", css_color);
    out = inject_svg_root_paint_style(&out, fill, stroke);
    for tag in ["path", "line", "polyline"] {
        out = add_fill_none_to_stroked_shapes(&out, tag);
    }
    out
}

fn inject_svg_root_paint_style(svg: &str, fill: Option<Color>, stroke: Option<Color>) -> String {
    let Some(start) = svg.find("<svg") else {
        return svg.to_string();
    };
    let Some(end_rel) = svg[start..].find('>') else {
        return svg.to_string();
    };
    let end = start + end_rel;
    let mut style = String::new();
    match fill {
        Some(c) => style.push_str(&format!("fill:{};", color_css(c))),
        None => style.push_str("fill:none;"),
    }
    match stroke {
        Some(c) => style.push_str(&format!("stroke:{};", color_css(c))),
        None => style.push_str("stroke:none;"),
    }

    let tag = &svg[start..end];
    let lower = tag.to_ascii_lowercase();
    let mut out = String::with_capacity(svg.len() + style.len() + 16);
    out.push_str(&svg[..start]);
    if let Some(pos) = lower.find(" style=") {
        let attr_start = start + pos + " style=".len();
        let quote = svg[attr_start..].chars().next().unwrap_or('"');
        if quote == '"' || quote == '\'' {
            let value_start = attr_start + quote.len_utf8();
            if let Some(value_end_rel) = svg[value_start..end].find(quote) {
                let value_end = value_start + value_end_rel;
                out.push_str(&svg[start..value_start]);
                out.push_str(&style);
                out.push_str(&svg[value_start..value_end]);
                out.push_str(&svg[value_end..]);
                return out;
            }
        }
    }
    out.push_str(&svg[start..end]);
    out.push_str(" style=\"");
    out.push_str(&style);
    out.push('"');
    out.push_str(&svg[end..]);
    out
}

fn color_css(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a as f32 / 255.0)
    }
}

fn add_fill_none_to_stroked_shapes(svg: &str, tag: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    let needle = format!("<{tag}");
    while let Some(pos) = rest.find(&needle) {
        out.push_str(&rest[..pos]);
        let after_start = &rest[pos..];
        let Some(gt) = after_start.find('>') else {
            out.push_str(after_start);
            return out;
        };
        let full_tag = &after_start[..=gt];
        if tag_has_attr(full_tag, "stroke") && !tag_has_attr(full_tag, "fill") {
            let insert_at = if full_tag.ends_with("/>") {
                full_tag.len() - 2
            } else {
                full_tag.len() - 1
            };
            out.push_str(&full_tag[..insert_at]);
            out.push_str(r#" fill="none""#);
            out.push_str(&full_tag[insert_at..]);
        } else {
            out.push_str(full_tag);
        }
        rest = &after_start[gt + 1..];
    }
    out.push_str(rest);
    out
}

fn tag_has_attr(tag: &str, attr: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    let attr = attr.to_ascii_lowercase();
    lower.contains(&format!(" {attr}="))
        || lower.contains(&format!("\t{attr}="))
        || lower.contains(&format!("\n{attr}="))
        || lower.contains(&format!("\r{attr}="))
}

/// Rasterize an SVG at its intrinsic (viewBox) size. Returns (rgba, w, h).
pub fn rasterize_svg_intrinsic(svg: &str) -> Option<(Vec<u8>, u32, u32)> {
    crate::svg::paint::rasterize_svg_intrinsic(svg)
}
