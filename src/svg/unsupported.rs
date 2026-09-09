//! Unsupported-feature reporting for native SVG.
//!
//! This is used by the browser debug server to make real-page SVG gaps visible
//! now that the native renderer is the only active path.

use std::collections::BTreeMap;

use super::{SvgDocument, SvgElementKind, SvgNode};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SvgUnsupportedSummary {
    pub documents: usize,
    pub unknown_elements: BTreeMap<String, usize>,
    pub unsupported_elements: BTreeMap<String, usize>,
    pub unsupported_attributes: BTreeMap<String, usize>,
}

impl SvgUnsupportedSummary {
    pub fn is_empty(&self) -> bool {
        self.unknown_elements.is_empty()
            && self.unsupported_elements.is_empty()
            && self.unsupported_attributes.is_empty()
    }

    pub fn merge(&mut self, other: SvgUnsupportedSummary) {
        self.documents += other.documents;
        merge_counts(&mut self.unknown_elements, other.unknown_elements);
        merge_counts(&mut self.unsupported_elements, other.unsupported_elements);
        merge_counts(
            &mut self.unsupported_attributes,
            other.unsupported_attributes,
        );
    }
}

pub fn unsupported_summary(doc: &SvgDocument) -> SvgUnsupportedSummary {
    let mut summary = SvgUnsupportedSummary {
        documents: 1,
        ..SvgUnsupportedSummary::default()
    };
    collect_node(&doc.root, &mut summary);
    summary
}

fn collect_node(node: &SvgNode, summary: &mut SvgUnsupportedSummary) {
    match &node.kind {
        SvgElementKind::Script => {}
        SvgElementKind::Animate
        | SvgElementKind::AnimateColor
        | SvgElementKind::AnimateMotion
        | SvgElementKind::AnimateTransform
        | SvgElementKind::MPath
        | SvgElementKind::Set => {}
        SvgElementKind::Unknown(name) => {
            inc(&mut summary.unknown_elements, name.to_string());
        }
        SvgElementKind::TextPath | SvgElementKind::Switch => {}
        _ => {}
    }

    for attr in &node.attributes {
        let name = attr.name.as_str();
        if attr.namespace.as_deref() == Some("xlink") || matches!(name, "href" | "id" | "class") {
            continue;
        }
        if name.starts_with("on") && name.len() > 2 {
            inc(
                &mut summary.unsupported_attributes,
                format!("{}@{}", svg_kind_name(&node.kind), name),
            );
            continue;
        }
        // SVG2 obsoletes `externalResourcesRequired`; it has no rendering
        // effect in this engine, so it is not an unsupported feature.
    }

    for child in &node.children {
        collect_node(child, summary);
    }
}

fn svg_kind_name(kind: &SvgElementKind) -> &str {
    match kind {
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
        SvgElementKind::Unknown(name) => name.as_str(),
    }
}

fn inc(map: &mut BTreeMap<String, usize>, key: String) {
    *map.entry(key).or_insert(0) += 1;
}

fn merge_counts(dst: &mut BTreeMap<String, usize>, src: BTreeMap<String, usize>) {
    for (key, count) in src {
        *dst.entry(key).or_insert(0) += count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svg::parse_svg_document;

    #[test]
    fn reports_unknown_and_deferred_svg_features() {
        let doc = parse_svg_document(
            r##"<svg>
                <foreignObject width="10" height="10"/>
                <switch>
                    <rect requiredExtensions="https://example.invalid/ext"/>
                    <view id="v"/>
                </switch>
                <animate attributeName="x"/>
                <rect onclick="go()" externalResourcesRequired="true"/>
                <made-up/>
            </svg>"##,
        )
        .unwrap();
        let summary = unsupported_summary(&doc);
        assert!(summary.unsupported_elements.is_empty());
        assert_eq!(summary.unknown_elements.get("made-up"), Some(&1));
        assert_eq!(summary.unsupported_attributes.get("rect@onclick"), Some(&1));
        assert_eq!(summary.unsupported_elements.get("foreignObject"), None);
        assert_eq!(summary.unsupported_elements.get("switch"), None);
        assert_eq!(summary.unsupported_elements.get("view"), None);
        assert_eq!(
            summary
                .unsupported_attributes
                .get("rect@requiredExtensions"),
            None
        );
        assert_eq!(
            summary
                .unsupported_attributes
                .get("rect@externalResourcesRequired"),
            None
        );
    }
}
