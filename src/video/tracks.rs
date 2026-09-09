//! HTML media text-track discovery.

use crate::types::WebCore;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextTrackInfo {
    pub kind: String,
    pub label: String,
    pub language: String,
    pub src: String,
    pub default: bool,
}

pub(crate) fn text_tracks(node: &WebCore, base_url: &str) -> Option<Vec<TextTrackInfo>> {
    if !matches!(node.tag.as_str(), "audio" | "video") {
        return None;
    }
    let mut tracks = Vec::new();
    for child in &node.children {
        if child.tag != "track" {
            continue;
        }
        let kind = child
            .attributes
            .get("kind")
            .map(|v| normalize_kind(v))
            .unwrap_or_else(|| "subtitles".to_string());
        let label = child.attributes.get("label").cloned().unwrap_or_default();
        let language = child.attributes.get("srclang").cloned().unwrap_or_default();
        let src = child
            .attributes
            .get("src")
            .map(|v| crate::html::resolve_url(v, base_url))
            .unwrap_or_default();
        tracks.push(TextTrackInfo {
            kind,
            label,
            language,
            src,
            default: child.attributes.contains_key("default"),
        });
    }
    Some(tracks)
}

fn normalize_kind(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "captions" => "captions",
        "descriptions" => "descriptions",
        "chapters" => "chapters",
        "metadata" => "metadata",
        _ => "subtitles",
    }
    .to_string()
}
