//! SVG source assembly and intrinsic sizing helpers.

use crate::dom::attrs::AttrMap;
use crate::types::WebCore;

/// Parse an SVG length that is definite at parse time.
///
/// SVG presentation attributes accept unitless lengths as px, and `px` lengths
/// are directly usable. Contextual lengths like `%`/`em` must be left to CSS
/// layout instead of being coerced by their numeric prefix.
pub(crate) fn parse_svg_length_px(s: &str) -> Option<f32> {
    super::geometry::parse_svg_length(s).and_then(super::geometry::definite_intrinsic_length)
}

/// Parse definite pixels from a string like "20px", "512", "20px;height:10px".
pub(crate) fn parse_px(s: &str) -> Option<u32> {
    parse_svg_length_px(s).map(|n| n.round() as u32)
}

/// Parse a viewBox attribute value "min-x min-y width height" -> (width, height).
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
pub(crate) struct InlineSvgSource {
    pub markup: String,
    pub viewbox_w: f32,
    pub viewbox_h: f32,
    pub explicit_w: Option<u32>,
    pub explicit_h: Option<u32>,
}

pub(crate) fn build_inline_svg_source(attrs: &AttrMap, body: &str) -> InlineSvgSource {
    let mut svg_tag = String::from("<svg");
    for (k, v) in attrs.iter() {
        svg_tag.push_str(&format!(" {}=\"{}\"", k, escape_svg_attr(v)));
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

    InlineSvgSource {
        markup: format!("{}{}</svg>", svg_tag, body),
        viewbox_w,
        viewbox_h,
        explicit_w: attrs.get("width").and_then(|s| parse_px(s)),
        explicit_h: attrs.get("height").and_then(|s| parse_px(s)),
    }
}

pub(crate) fn build_inline_svg_source_from_node(node: &WebCore) -> InlineSvgSource {
    let body = node
        .children
        .iter()
        .map(serialize_svg_dom_node)
        .collect::<String>();
    build_inline_svg_source(&node.attributes, &body)
}

fn serialize_svg_dom_node(node: &WebCore) -> String {
    if node.tag == "#text" {
        return escape_svg_text(&node.text);
    }
    if node.tag.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    out.push('<');
    out.push_str(&node.tag);
    for (name, value) in &node.attributes {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(&escape_svg_attr(value));
        out.push('"');
    }
    if node.children.is_empty() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    for child in &node.children {
        out.push_str(&serialize_svg_dom_node(child));
    }
    out.push_str("</");
    out.push_str(&node.tag);
    out.push('>');
    out
}

fn escape_svg_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_svg_attr(value: &str) -> String {
    escape_svg_text(value).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::{SvgElementKind, parse_svg_document};

    #[test]
    fn root_attribute_escaping_roundtrips_without_injection() {
        for value in [
            "quote \" ampersand & less-than < greater-than > apostrophe '",
            "&quot; &amp; &lt;",
            "\" injected=\"yes",
            "\"><g id=\"injected\"/></svg><svg data-note=\"",
        ] {
            let mut attrs = AttrMap::new();
            attrs.insert("width", "20");
            attrs.insert("data-note", value);
            let source = build_inline_svg_source(&attrs, "<rect width=\"1\" height=\"1\"/>");
            let doc = parse_svg_document(&source.markup).expect("escaped SVG must parse");

            assert_eq!(doc.root.kind, SvgElementKind::Svg);
            assert_eq!(doc.root.attr("data-note"), Some(value));
            assert_eq!(doc.root.attr("width"), Some("20"));
            assert_eq!(doc.root.attr("xmlns"), Some("http://www.w3.org/2000/svg"));
            assert_eq!(doc.root.attributes.len(), 3, "unexpected root attributes");
            assert_eq!(doc.root.children.len(), 1, "unexpected injected elements");
            let rect = &doc.root.children[0];
            assert_eq!(rect.kind, SvgElementKind::Rect);
            assert_eq!(rect.attr("width"), Some("1"));
            assert_eq!(rect.attr("height"), Some("1"));
            assert_eq!(rect.attributes.len(), 2);
            assert!(rect.children.is_empty());
            assert!(doc.root.text.is_empty());
        }
    }
}
