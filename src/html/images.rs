//! Image loading and decoding.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Image loading ─────────────────────────────────────────────────────────

/// Resolve a URL against a base URL.
pub fn resolve_url(src: &str, base_url: &str) -> String {
    if src.is_empty() {
        return base_url.to_string();
    }
    if src.starts_with("data:") {
        return src.to_string();
    }
    if let Some(path) = src.strip_prefix("file://") {
        return path.to_string();
    }
    if src.contains("://") {
        return src.to_string();
    }

    // Strip "./" prefix
    let src = src.strip_prefix("./").unwrap_or(src);

    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        let scheme_end = base_url.find("://").unwrap(); // safe: starts_with guarantees this
        let after_scheme = &base_url[scheme_end + 3..];

        // Origin = scheme + host (no path)
        let origin = match after_scheme.find('/') {
            Some(i) => &base_url[..scheme_end + 3 + i],
            None => base_url,
        };

        if src.starts_with("//") {
            let scheme = &base_url[..scheme_end + 1]; // "https:" or "http:"
            return format!("{}{}", scheme, src);
        }
        if src.starts_with('/') {
            return format!("{}{}", origin, src);
        }
        // Relative path — resolve against directory of base URL
        let dir = match after_scheme.rfind('/') {
            Some(i) => &base_url[..scheme_end + 3 + i + 1], // include trailing slash
            None => {
                // Base has no path (e.g. "https://example.com") — treat as root
                return format!("{}/{}", origin, src);
            }
        };
        return format!("{}{}", dir, src);
    }
    // File or local path
    if src.starts_with('/') || base_url.is_empty() {
        return src.to_string();
    }
    let base_path = base_url.strip_prefix("file://").unwrap_or(base_url);
    let base_path = std::path::Path::new(base_path);
    let base_dir =
        if base_path.is_dir() || base_url.ends_with('/') || base_path.extension().is_none() {
            base_path
                .to_string_lossy()
                .trim_end_matches('/')
                .to_string()
        } else {
            base_path
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
        };
    if base_dir.is_empty() {
        src.to_string()
    } else {
        format!("{}/{}", base_dir, src)
    }
}

/// Set decoded image data on a node.
/// Dimensions are NOT baked into the style here — the layout engine handles
/// aspect-ratio sizing after the CSS cascade has set any explicit width/height.
pub fn set_image_on_node(node: &mut WebCore, data: Vec<u8>, w: u32, h: u32) {
    node.image_data = Some(std::sync::Arc::new(data));
    node.image_width = w;
    node.image_height = h;
    node.animated_image = None;
    node.animated_image_frame = 0;
    node.animated_image_last_tick = None;
}

/// Set decoded image (raster or SVG) on an img node.
/// SVGs are parsed into the native SVG tree and rasterized at paint size.
pub fn set_decoded_image_on_node(node: &mut WebCore, decoded: DecodedImage) {
    match decoded {
        DecodedImage::Raster(data, w, h) => {
            node.image_data = Some(data);
            node.image_width = w;
            node.image_height = h;
            node.animated_image = None;
            node.animated_image_frame = 0;
            node.animated_image_last_tick = None;
        }
        DecodedImage::Animated(animated) => {
            node.image_data = animated.frames.first().map(|frame| frame.pixels.clone());
            node.image_width = animated.width;
            node.image_height = animated.height;
            node.animated_image = Some(animated);
            node.animated_image_frame = 0;
            node.animated_image_last_tick = Some(std::time::Instant::now());
        }
        DecodedImage::Svg(markup, iw, ih) => {
            node.svg_document = crate::svg::parse_svg_document(&markup).ok();
            node.svg_viewbox_w = iw;
            node.svg_viewbox_h = ih;
            // Set intrinsic dimensions so layout can compute aspect ratio
            node.image_width = iw.ceil() as u32;
            node.image_height = ih.ceil() as u32;
            node.animated_image = None;
            node.animated_image_frame = 0;
            node.animated_image_last_tick = None;
        }
    }
}

pub fn set_decoded_bg_image_on_node(node: &mut WebCore, decoded: DecodedImage) -> bool {
    let is_ratio_only = match &decoded {
        DecodedImage::Svg(svg, _, _) => crate::svg::has_ratio_only_from_markup(svg),
        _ => false,
    };
    if let Some((data, w, h)) = decoded_image_pixels(decoded) {
        node.bg_image_data = Some(std::sync::Arc::new(data));
        node.bg_image_width = w;
        node.bg_image_height = h;
        node.bg_image_ratio_only = is_ratio_only;
        true
    } else {
        false
    }
}

/// Try to load an image from a file path or data URL.
/// Returns (rgba_bytes, width, height) or None on failure.
pub(crate) fn load_image_from_src(src: &str, base_url: &str) -> Option<(Vec<u8>, u32, u32)> {
    // Data URL: data:image/xxx;base64,...
    if src.starts_with("data:") {
        return load_image_data_url(src);
    }

    let path = resolve_url(src, base_url);

    // Fetch remote images via HTTP
    if path.starts_with("http://") || path.starts_with("https://") {
        let bytes = crate::http_client()
            .get(&path)
            .header("Sec-Fetch-Dest", "image")
            .send()
            .ok()
            .and_then(|r| r.bytes().ok())
            .map(|b| b.to_vec())?;
        return decode_image_bytes(&bytes);
    }

    let bytes = std::fs::read(&path).ok()?;
    decode_image_bytes(&bytes)
}

pub(crate) fn load_decoded_image_from_src(src: &str, base_url: &str) -> Option<DecodedImage> {
    if src.starts_with("data:") {
        let bytes = image_data_url_bytes(src)?;
        return decode_image_bytes_ex(&bytes);
    }

    let path = resolve_url(src, base_url);
    if path.starts_with("http://") || path.starts_with("https://") {
        let bytes = crate::http_client()
            .get(&path)
            .header("Sec-Fetch-Dest", "image")
            .send()
            .ok()
            .and_then(|r| r.bytes().ok())
            .map(|b| b.to_vec())?;
        return decode_image_bytes_ex(&bytes);
    }

    let bytes = std::fs::read(&path).ok()?;
    decode_image_bytes_ex(&bytes)
}

fn load_image_data_url(src: &str) -> Option<(Vec<u8>, u32, u32)> {
    let bytes = image_data_url_bytes(src)?;
    // SVG data URLs need actual native rasterization, not a transparent
    // dimensions placeholder.
    if std::str::from_utf8(&bytes)
        .ok()
        .is_some_and(|text| text.trim_start().starts_with('<') && text.contains("<svg"))
    {
        return decode_image_bytes(&bytes);
    }
    decode_image_bytes(&bytes)
}

fn image_data_url_bytes(src: &str) -> Option<Vec<u8>> {
    // data:image/png;base64,<data> or data:image/svg+xml,<percent-encoded markup>
    let comma = src.find(',')?;
    let header = &src[5..comma]; // strip "data:"
    let encoded = &src[comma + 1..];
    let is_base64 = header.contains("base64");
    if !is_base64 {
        return Some(percent_decode_data_url_payload(encoded));
    }
    base64_decode(encoded)
}

fn percent_decode_data_url_payload(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Result of decoding image bytes: either rasterized RGBA or SVG markup to rasterize later.
#[derive(Clone)]
pub enum DecodedImage {
    Raster(std::sync::Arc<Vec<u8>>, u32, u32),
    Animated(AnimatedImage),
    Svg(String, f32, f32), // markup, intrinsic_w, intrinsic_h
}

#[derive(Clone, Debug)]
pub struct AnimatedImageFrame {
    pub pixels: std::sync::Arc<Vec<u8>>,
    pub duration_ms: u32,
}

#[derive(Clone, Debug)]
pub struct AnimatedImage {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<AnimatedImageFrame>,
}

pub fn decode_image_bytes_ex(bytes: &[u8]) -> Option<DecodedImage> {
    if let Some(animated) = decode_animated_image(bytes) {
        if animated.frames.len() > 1 {
            return Some(DecodedImage::Animated(animated));
        }
    }

    // Try raster formats first (PNG, JPEG, GIF, WebP, BMP)
    if let Ok(img) = image::load_from_memory(bytes) {
        {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let mut raw = rgba.into_raw();
            premultiply_rgba(&mut raw);
            return Some(DecodedImage::Raster(std::sync::Arc::new(raw), w, h));
        }
    }
    // SVG: return the markup for deferred rasterization at paint time
    if let Ok(svg_str) = std::str::from_utf8(bytes) {
        let trimmed = svg_str.trim_start();
        if trimmed.starts_with('<') && (trimmed.contains("<svg") || trimmed.starts_with("<?xml")) {
            let (iw, ih) = svg_intrinsic_size(svg_str);
            return Some(DecodedImage::Svg(svg_str.to_string(), iw, ih));
        }
    }
    None
}

/// Extract intrinsic dimensions from SVG markup without rasterizing.
fn svg_intrinsic_size(svg: &str) -> (f32, f32) {
    crate::svg::intrinsic_size_from_markup(svg)
}

pub fn decode_image_bytes(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    decoded_image_pixels(decode_image_bytes_ex(bytes)?)
}

pub fn decoded_image_pixels(decoded: DecodedImage) -> Option<(Vec<u8>, u32, u32)> {
    match decoded {
        DecodedImage::Raster(data, w, h) => Some((data.as_ref().clone(), w, h)),
        DecodedImage::Animated(animated) => animated.frames.first().map(|frame| {
            (
                frame.pixels.as_ref().clone(),
                animated.width,
                animated.height,
            )
        }),
        DecodedImage::Svg(svg, _, _) => crate::svg::rasterize_svg_intrinsic(&svg),
    }
}

fn decode_animated_image(bytes: &[u8]) -> Option<AnimatedImage> {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return decode_animation_frames(
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?,
        );
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return decode_animation_frames(
            image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?,
        );
    }
    None
}

fn decode_animation_frames<'a, D>(decoder: D) -> Option<AnimatedImage>
where
    D: image::AnimationDecoder<'a> + image::ImageDecoder,
{
    let (width, height) = decoder.dimensions();
    let frames = decoder.into_frames().collect_frames().ok()?;
    if frames.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let delay_ms = frame.delay().numer_denom_ms();
        let duration_ms = if delay_ms.0 == 0 {
            100
        } else {
            ((delay_ms.0 as f64 / delay_ms.1.max(1) as f64).round() as u32).max(10)
        };
        let buffer = frame.into_buffer();
        let mut raw = buffer.into_raw();
        premultiply_rgba(&mut raw);
        out.push(AnimatedImageFrame {
            pixels: std::sync::Arc::new(raw),
            duration_ms,
        });
    }
    Some(AnimatedImage {
        width,
        height,
        frames: out,
    })
}

fn premultiply_rgba(raw: &mut [u8]) {
    for pixel in raw.chunks_exact_mut(4) {
        let a = pixel[3] as u16;
        if a == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
        } else if a < 255 {
            pixel[0] = ((pixel[0] as u16 * a) / 255) as u8;
            pixel[1] = ((pixel[1] as u16 * a) / 255) as u8;
            pixel[2] = ((pixel[2] as u16 * a) / 255) as u8;
        }
    }
}

/// Minimal base64 decoder (no external dependency needed for this).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 128] = b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x3e\xff\xff\xff\x3f\
\x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\xff\xff\xff\xff\xff\xff\
\xff\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\
\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\xff\xff\xff\xff\xff\
\xff\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\
\x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\xff\xff\xff\xff\xff";
    let clean: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < clean.len() {
        let b0 = clean[i] as usize;
        let b1 = clean[i + 1] as usize;
        let b2 = clean[i + 2] as usize;
        let b3 = clean[i + 3] as usize;
        if b0 >= 128 || b1 >= 128 {
            return None;
        }
        let v0 = TABLE[b0];
        let v1 = TABLE[b1];
        let v2 = if clean[i + 2] == b'=' {
            0
        } else if b2 < 128 {
            TABLE[b2]
        } else {
            return None;
        };
        let v3 = if clean[i + 3] == b'=' {
            0
        } else if b3 < 128 {
            TABLE[b3]
        } else {
            return None;
        };
        if v0 == 0xff || v1 == 0xff {
            return None;
        }
        out.push((v0 << 2) | (v1 >> 4));
        if clean[i + 2] != b'=' {
            out.push(((v1 & 0xf) << 4) | (v2 >> 2));
        }
        if clean[i + 3] != b'=' {
            out.push(((v2 & 0x3) << 6) | v3);
        }
        i += 4;
    }
    Some(out)
}
