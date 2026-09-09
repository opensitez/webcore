//! HTML media source selection and conservative type support.

use crate::types::WebCore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaKind {
    Audio,
    Video,
}

fn media_kind(tag: &str) -> Option<MediaKind> {
    match tag {
        "audio" => Some(MediaKind::Audio),
        "video" => Some(MediaKind::Video),
        _ => None,
    }
}

pub(crate) fn current_src(node: &WebCore, base_url: &str) -> Option<String> {
    let kind = media_kind(&node.tag)?;
    if let Some(src) = node.attributes.get("src") {
        return Some(crate::html::resolve_url(src, base_url));
    }
    for child in &node.children {
        if child.tag != "source" {
            continue;
        }
        if let Some(media_type) = child.attributes.get("type") {
            if can_play_type_for_kind(kind, media_type).is_empty() {
                continue;
            }
        }
        if let Some(src) = child.attributes.get("src") {
            return Some(crate::html::resolve_url(src, base_url));
        }
    }
    Some(String::new())
}

pub(crate) fn can_play_type(tag: &str, media_type: &str) -> Option<&'static str> {
    Some(can_play_type_for_kind(media_kind(tag)?, media_type))
}

fn can_play_type_for_kind(kind: MediaKind, media_type: &str) -> &'static str {
    let lower = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let playable = match kind {
        MediaKind::Video => matches!(
            lower.as_str(),
            "video/mp4" | "video/webm" | "video/ogg" | "application/ogg"
        ),
        MediaKind::Audio => matches!(
            lower.as_str(),
            "audio/mpeg"
                | "audio/mp3"
                | "audio/mp4"
                | "audio/aac"
                | "audio/ogg"
                | "audio/webm"
                | "audio/wav"
                | "audio/x-wav"
                | "application/ogg"
        ),
    };
    if playable {
        "maybe"
    } else {
        ""
    }
}
