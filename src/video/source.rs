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
    let lower = media_type.to_ascii_lowercase();
    let mime = lower.split(';').next().unwrap_or("").trim();
    let playable = match kind {
        MediaKind::Video => match mime {
            "video/mp4" => {
                !lower.contains("codecs=") || lower.contains("avc1") || lower.contains("avc3")
            }
            "video/x-yuv4mpeg2" => true,
            _ => false,
        },
        MediaKind::Audio => matches!(
            mime,
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
    if playable { "maybe" } else { "" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_selection_skips_hevc_and_webm() {
        let doc = crate::html::parse_html(
            "<video id=v><source src=a.mp4 type='video/mp4; codecs=&quot;hvc1&quot;'><source src=b.webm type=video/webm>Video not supported.</video>",
        );
        let node = doc
            .find_webcore(doc.get_element_by_id("v").unwrap())
            .unwrap();
        assert_eq!(current_src(node, "http://localhost/"), Some(String::new()));
        assert_eq!(
            can_play_type("video", "video/mp4; codecs=\"hvc1\""),
            Some("")
        );
        assert_eq!(can_play_type("video", "video/webm"), Some(""));
        assert_eq!(
            can_play_type("video", "video/mp4; codecs=\"avc1.640028\""),
            Some("maybe")
        );
    }
}
