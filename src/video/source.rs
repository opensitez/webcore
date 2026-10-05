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
                    || lower.contains("vp08") || lower.contains("vp09")
            }
            "video/x-yuv4mpeg2" => true,
            "video/webm" => !lower.contains("codecs=") || lower.contains("vp8")
                || lower.contains("vp9") || lower.contains("vp09") || lower.contains("av01"),
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
    fn source_selection_skips_hevc_and_accepts_vp8() {
        let doc = crate::html::parse_html(
            "<video id=v><source src=a.mp4 type='video/mp4; codecs=&quot;hvc1&quot;'><source src=b.webm type=video/webm>Video not supported.</video>",
        );
        let node = doc
            .find_webcore(doc.get_element_by_id("v").unwrap())
            .unwrap();
        assert_eq!(current_src(node, "http://localhost/"), Some("http://localhost/b.webm".into()));
        assert_eq!(
            can_play_type("video", "video/mp4; codecs=\"hvc1\""),
            Some("")
        );
        assert_eq!(can_play_type("video", "video/webm"), Some("maybe"));
        assert_eq!(can_play_type("video", "video/webm; codecs=\"vp8\""), Some("maybe"));
        assert_eq!(can_play_type("video", "video/webm; codecs=\"vp9\""), Some("maybe"));
        assert_eq!(
            can_play_type("video", "video/mp4; codecs=\"avc1.640028\""),
            Some("maybe")
        );
    }

    #[test]
    fn selects_typed_vp9_source_and_recognizes_vp_sample_entries() {
        let doc = crate::html::parse_html(
            "<video id=v><source src=movie.webm type='video/webm; codecs=&quot;vp9&quot;'></video>",
        );
        let node = doc.find_webcore(doc.get_element_by_id("v").unwrap()).unwrap();
        assert_eq!(current_src(node, "http://localhost/websites/video/video_vp9.html"),
            Some("http://localhost/websites/video/movie.webm".into()));
        for media_type in ["video/webm; codecs=\"vp09.00.10.08\"",
            "video/mp4; codecs=\"vp09.00.10.08\"", "video/mp4; codecs=\"vp08\""] {
            assert_eq!(can_play_type("video", media_type), Some("maybe"));
        }
        assert_eq!(can_play_type("video", "video/webm; codecs=\"av01\""), Some("maybe"));
    }

    #[test]
    fn selects_typed_av1_webm_source() {
        let doc = crate::html::parse_html(
            "<video id=v><source src=spacewalk_av1.webm type='video/webm; codecs=&quot;av01&quot;'></video>",
        );
        let node = doc.find_webcore(doc.get_element_by_id("v").unwrap()).unwrap();
        assert_eq!(current_src(node, "http://localhost/websites/video/video_av1.html"),
            Some("http://localhost/websites/video/spacewalk_av1.webm".into()));
        assert_eq!(can_play_type("video", "video/webm; codecs=\"av01.0.08M.08\""), Some("maybe"));
        assert_eq!(can_play_type("video", "video/mp4; codecs=\"av01\""), Some(""));
    }
}
