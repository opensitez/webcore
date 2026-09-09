//! SVG conditional processing helpers.

use super::SvgNode;

pub(crate) fn switch_accepts(node: &SvgNode) -> bool {
    if !required_features_match(node.attr_ascii_case_insensitive("requiredFeatures")) {
        return false;
    }
    if node
        .attr_ascii_case_insensitive("requiredExtensions")
        .is_some_and(|value| value.split_ascii_whitespace().next().is_some())
    {
        return false;
    }
    system_language_matches(node.attr_ascii_case_insensitive("systemLanguage"))
}

fn required_features_match(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let features: Vec<_> = value.split_ascii_whitespace().collect();
    if features.is_empty() {
        return true;
    }
    features.iter().all(|feature| {
        matches!(
            feature.rsplit('#').next().unwrap_or(feature),
            "SVG"
                | "CoreAttribute"
                | "Structure"
                | "BasicStructure"
                | "ContainerAttribute"
                | "ConditionalProcessing"
                | "Shape"
                | "BasicText"
                | "BasicPaintAttribute"
                | "OpacityAttribute"
                | "GraphicsAttribute"
                | "Gradient"
                | "Pattern"
                | "Clip"
                | "Mask"
                | "Filter"
                | "Marker"
        )
    })
}

fn system_language_matches(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let value = value.trim();
    if value.is_empty() {
        return true;
    }
    value
        .split(',')
        .map(str::trim)
        .any(|lang| lang.eq_ignore_ascii_case("en") || lang.to_ascii_lowercase().starts_with("en-"))
}
