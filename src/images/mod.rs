//! Image loading and decoding.

#![allow(unused_imports)]
use crate::css::*;
use crate::types::*;
use webmedia::bitmap::premultiply_rgba;

pub(crate) mod cache;
pub mod stream;

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
    if base_url.starts_with("http://") || base_url.starts_with("https://") {
        if let Some(base) = crate::dom::url::parse(base_url, None)
            && let Some(url) = crate::dom::url::parse(src, Some(&base))
        {
            return url.href();
        }
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
    node.image_data_width = w;
    node.image_data_height = h;
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
            node.image_data_width = w;
            node.image_data_height = h;
            node.image_width = w;
            node.image_height = h;
            node.animated_image = None;
            node.animated_image_frame = 0;
            node.animated_image_last_tick = None;
        }
        DecodedImage::Animated(animated) => {
            node.image_data = animated.frames.first().map(|frame| frame.pixels.clone());
            node.image_data_width = animated.width;
            node.image_data_height = animated.height;
            node.image_width = animated.intrinsic_width;
            node.image_height = animated.intrinsic_height;
            node.animated_image = Some(animated);
            node.animated_image_frame = 0;
            node.animated_image_last_tick = Some(std::time::Instant::now());
        }
        DecodedImage::Svg(markup, iw, ih) => {
            node.svg_document = crate::svg::parse_svg_document(&markup).ok();
            set_svg_image_dimensions(node, iw, ih);
        }
        DecodedImage::SvgDocument(doc, iw, ih) => {
            node.svg_document = Some(doc);
            set_svg_image_dimensions(node, iw, ih);
        }
    }
}

fn set_svg_image_dimensions(node: &mut WebCore, iw: f32, ih: f32) {
    node.svg_viewbox_w = iw;
    node.svg_viewbox_h = ih;
    node.image_data_width = 0;
    node.image_data_height = 0;
    node.image_width = iw.ceil() as u32;
    node.image_height = ih.ceil() as u32;
    node.animated_image = None;
    node.animated_image_frame = 0;
    node.animated_image_last_tick = None;
}

pub fn set_decoded_bg_image_on_node(node: &mut WebCore, decoded: DecodedImage) -> bool {
    let is_ratio_only = match &decoded {
        DecodedImage::Svg(svg, _, _) => crate::svg::has_ratio_only_from_markup(svg),
        DecodedImage::SvgDocument(doc, _, _) => crate::svg::has_ratio_only(doc),
        _ => false,
    };
    if let Some((data, w, h)) = decoded_image_pixels_arc(decoded) {
        node.bg_image_data = Some(data);
        node.bg_image_width = w;
        node.bg_image_height = h;
        node.bg_image_ratio_only = is_ratio_only;
        node.bg_image_resolution = 1.0;
        true
    } else {
        false
    }
}

pub fn set_decoded_bg_image_for_url_on_node(
    node: &mut WebCore,
    decoded: DecodedImage,
    url: &str,
    base_url: &str,
) -> bool {
    if !set_decoded_bg_image_on_node(node, decoded) {
        return false;
    }
    node.bg_image_resolution = node
        .style
        .rare()
        .background_image_set_source
        .as_deref()
        .and_then(|source| {
            crate::css::property_defs::image_set_resolution_for_url(source, url, base_url)
        })
        .unwrap_or(1.0);
    true
}

pub fn set_decoded_bg_image_layer_on_node(
    node: &mut WebCore,
    layer_index: usize,
    decoded: DecodedImage,
) -> bool {
    let is_ratio_only = match &decoded {
        DecodedImage::Svg(svg, _, _) => crate::svg::has_ratio_only_from_markup(svg),
        DecodedImage::SvgDocument(doc, _, _) => crate::svg::has_ratio_only(doc),
        _ => false,
    };
    if let Some((data, width, height)) = decoded_image_pixels_arc(decoded) {
        if node.additional_bg_images.len() <= layer_index {
            node.additional_bg_images
                .resize_with(layer_index + 1, || None);
        }
        node.additional_bg_images[layer_index] = Some(crate::types::DecodedBackgroundImage {
            data,
            width,
            height,
            ratio_only: is_ratio_only,
            resolution: 1.0,
        });
        true
    } else {
        false
    }
}

pub fn set_decoded_bg_image_layer_for_url_on_node(
    node: &mut WebCore,
    layer_index: usize,
    decoded: DecodedImage,
    url: &str,
    base_url: &str,
) -> bool {
    if !set_decoded_bg_image_layer_on_node(node, layer_index, decoded) {
        return false;
    }
    let resolution = node
        .style
        .rare()
        .additional_background_layers
        .get(layer_index)
        .and_then(|layer| layer.image_set_source.as_deref())
        .and_then(|source| {
            crate::css::property_defs::image_set_resolution_for_url(source, url, base_url)
        })
        .unwrap_or(1.0);
    if let Some(Some(image)) = node.additional_bg_images.get_mut(layer_index) {
        image.resolution = resolution;
    }
    true
}

pub fn set_decoded_mask_image_for_url_on_node(
    node: &mut WebCore,
    decoded: DecodedImage,
    url: &str,
    base_url: &str,
) -> bool {
    set_decoded_mask_image_layer_for_url_on_node(node, 0, decoded, url, base_url)
}

pub fn set_decoded_mask_image_layer_for_url_on_node(
    node: &mut WebCore,
    index: usize,
    decoded: DecodedImage,
    url: &str,
    base_url: &str,
) -> bool {
    let Some(source_key) = node.style.mask_source_key(index).map(str::to_string) else {
        return false;
    };
    let Some((data, width, height)) = decoded_image_pixels_arc(decoded) else {
        return false;
    };
    let source = if index == 0 {
        node.style.rare().mask_image_set_source.as_deref()
    } else {
        node.style.rare().additional_mask_images.get(index - 1)
            .and_then(|layer| layer.image_set_source.as_deref())
    };
    let resolution = source
        .and_then(|source| {
            crate::css::property_defs::image_set_resolution_for_url(source, url, base_url)
        })
        .unwrap_or(1.0);
    let image = crate::types::DecodedMaskImage {
        data,
        width,
        height,
        resolution,
    };
    std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default))
        .set_with_source(index, image, source_key);
    true
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
        let bytes = crate::loading::fetch_bytes(&path).ok()?;
        return decode_image_bytes(&bytes);
    }

    let bytes = std::fs::read(&path).ok()?;
    decode_image_bytes(&bytes)
}

/// Paint-time image lookup that never performs remote network I/O.
///
/// Display-list construction is on the render path. Fetching from there makes
/// scroll/animation frames block on network and decode work. Remote images are
/// used only once a resource task has already warmed the decoded cache; data
/// URLs and local files remain synchronous because their bytes are already local
/// to the document/runtime.
pub(crate) fn load_paint_image_from_src(
    src: &str,
    base_url: &str,
) -> Option<(std::sync::Arc<Vec<u8>>, u32, u32)> {
    if src.starts_with("data:") {
        let (data, w, h) = load_image_data_url(src)?;
        return Some((std::sync::Arc::new(data), w, h));
    }

    let path = resolve_url(src, base_url);
    if path.starts_with("http://") || path.starts_with("https://") {
        let decoded = crate::cached_decoded_image_ready(&path)?;
        return decoded_image_pixels_arc(decoded);
    }

    let bytes = std::fs::read(&path).ok()?;
    decoded_image_pixels_arc(decode_image_bytes_ex(&bytes)?)
}

pub(crate) fn load_decoded_image_from_src(src: &str, base_url: &str) -> Option<DecodedImage> {
    if src.starts_with("data:") {
        let bytes = image_data_url_bytes(src)?;
        return decode_image_bytes_ex(&bytes);
    }

    let path = resolve_url(src, base_url);
    if path.starts_with("http://") || path.starts_with("https://") {
        let bytes = crate::loading::fetch_bytes(&path).ok()?;
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

pub(crate) fn image_data_url_bytes(src: &str) -> Option<Vec<u8>> {
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
    SvgDocument(crate::svg::SvgDocument, f32, f32),
}

#[derive(Clone, Debug)]
pub struct AnimatedImageFrame {
    pub pixels: std::sync::Arc<Vec<u8>>,
    pub duration_ms: u32,
}

#[derive(Clone, Debug)]
pub struct AnimatedImage {
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    pub width: u32,
    pub height: u32,
    pub frames: Vec<AnimatedImageFrame>,
    pub source_bytes: Option<std::sync::Arc<Vec<u8>>>,
    pub fully_decoded: bool,
    pub(crate) pending_decode: Option<std::sync::Arc<std::sync::OnceLock<AnimatedImage>>>,
}

impl AnimatedImage {
    pub fn can_animate(&self) -> bool {
        self.frames.len() > 1 || self.source_bytes.is_some()
    }
}

pub fn decode_image_bytes_ex(bytes: &[u8]) -> Option<DecodedImage> {
    decode_image_bytes_ex_with_source(bytes, None)
}

pub fn decode_image_bytes_arc(bytes: std::sync::Arc<Vec<u8>>) -> Option<DecodedImage> {
    decode_image_bytes_ex_with_source(bytes.as_slice(), Some(bytes.clone()))
}

fn decode_image_bytes_ex_with_source(
    bytes: &[u8],
    source_bytes: Option<std::sync::Arc<Vec<u8>>>,
) -> Option<DecodedImage> {
    let _profile_decode = crate::profile::span(crate::profile::Phase::ImageDecode);
    if (bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP"))
        && let Some(animated) = decode_animated_image(bytes, source_bytes.clone())
    {
        if animated.can_animate() {
            return Some(DecodedImage::Animated(animated));
        }
        if let Some(frame) = animated.frames.first() {
            return Some(DecodedImage::Raster(
                frame.pixels.clone(),
                animated.width,
                animated.height,
            ));
        }
    }

    // Try raster formats first (PNG, JPEG, GIF, WebP, BMP).
    if let Ok(image) = webmedia::bitmap::decode_raster(bytes) {
        return Some(DecodedImage::Raster(
            std::sync::Arc::new(image.rgba),
            image.width,
            image.height,
        ));
    }
    if let Some(animated) = decode_animated_image(bytes, source_bytes) {
        if animated.can_animate() {
            return Some(DecodedImage::Animated(animated));
        }
    }
    // SVG: return the markup for deferred rasterization at paint time
    if let Ok(svg_str) = std::str::from_utf8(bytes) {
        let trimmed = svg_str.trim_start();
        if trimmed.starts_with('<') && (trimmed.contains("<svg") || trimmed.starts_with("<?xml")) {
            let doc = crate::svg::parse_svg_document(svg_str).ok()?;
            let (iw, ih) = crate::svg::geometry::intrinsic_size(&doc).unwrap_or((0.0, 0.0));
            return Some(DecodedImage::SvgDocument(doc, iw, ih));
        }
    }
    None
}

pub fn decode_image_bytes(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    decoded_image_pixels(decode_image_bytes_ex(bytes)?)
}

pub fn decoded_image_pixels(decoded: DecodedImage) -> Option<(Vec<u8>, u32, u32)> {
    decoded_image_pixels_arc(decoded).map(|(data, w, h)| (data.as_ref().clone(), w, h))
}

pub fn decoded_image_pixels_arc(
    decoded: DecodedImage,
) -> Option<(std::sync::Arc<Vec<u8>>, u32, u32)> {
    match decoded {
        DecodedImage::Raster(data, w, h) => Some((data, w, h)),
        DecodedImage::Animated(animated) => animated
            .frames
            .first()
            .map(|frame| (frame.pixels.clone(), animated.width, animated.height)),
        DecodedImage::Svg(svg, _, _) => crate::svg::rasterize_svg_intrinsic(&svg)
            .map(|(data, w, h)| (std::sync::Arc::new(data), w, h)),
        DecodedImage::SvgDocument(doc, _, _) => crate::svg::rasterize_svg_document_intrinsic(&doc)
            .map(|(data, w, h)| (std::sync::Arc::new(data), w, h)),
    }
}

fn decode_animated_image(
    bytes: &[u8],
    source_bytes: Option<std::sync::Arc<Vec<u8>>>,
) -> Option<AnimatedImage> {
    let decoded = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        if !decoder.is_apng().ok()? {
            return None;
        }
        let (width, height) = image::ImageDecoder::dimensions(&decoder);
        decode_animation_frames_with_size(decoder.apng().ok()?, width, height, None, Some(2), None)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        decode_animation_frames_inner(
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?,
            None,
            Some(2),
            None,
        )
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        decode_animation_frames_inner(
            image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?,
            None,
            Some(2),
            None,
        )
    } else {
        None
    };
    let mut decoded = decoded?;
    if !decoded.fully_decoded {
        decoded.source_bytes =
            Some(source_bytes.unwrap_or_else(|| std::sync::Arc::new(bytes.to_vec())));
    }
    Some(decoded)
}

fn decode_animation_frames_inner<'a, D>(
    decoder: D,
    source_bytes: Option<std::sync::Arc<Vec<u8>>>,
    max_frames: Option<usize>,
    target_size: Option<(u32, u32)>,
) -> Option<AnimatedImage>
where
    D: image::AnimationDecoder<'a> + image::ImageDecoder,
{
    let (width, height) = decoder.dimensions();
    decode_animation_frames_with_size(
        decoder,
        width,
        height,
        source_bytes,
        max_frames,
        target_size,
    )
}

fn decode_animation_frames_with_size<'a, D>(
    decoder: D,
    width: u32,
    height: u32,
    source_bytes: Option<std::sync::Arc<Vec<u8>>>,
    max_frames: Option<usize>,
    target_size: Option<(u32, u32)>,
) -> Option<AnimatedImage>
where
    D: image::AnimationDecoder<'a>,
{
    let (frame_width, frame_height) = animated_frame_decode_size(width, height, target_size);
    let limit = max_frames.unwrap_or(usize::MAX);
    let mut out = Vec::new();
    for frame in decoder.into_frames().take(limit) {
        let frame = frame.ok()?;
        let delay_ms = frame.delay().numer_denom_ms();
        let duration_ms = if delay_ms.0 == 0 {
            100
        } else {
            ((delay_ms.0 as f64 / delay_ms.1.max(1) as f64).round() as u32).max(10)
        };
        let raw = resize_animated_frame(frame.into_buffer(), frame_width, frame_height)?;
        out.push(AnimatedImageFrame {
            pixels: std::sync::Arc::new(raw),
            duration_ms,
        });
    }
    if out.is_empty() {
        return None;
    }
    let has_more_frames = max_frames.is_some() && out.len() > 1;
    if has_more_frames {
        out.truncate(1);
    }
    let fully_decoded = max_frames.is_none() || !has_more_frames;
    Some(AnimatedImage {
        intrinsic_width: width,
        intrinsic_height: height,
        width: frame_width,
        height: frame_height,
        frames: out,
        source_bytes: if has_more_frames { source_bytes } else { None },
        fully_decoded,
        pending_decode: None,
    })
}

fn resize_animated_frame(buffer: image::RgbaImage, width: u32, height: u32) -> Option<Vec<u8>> {
    let (source_width, source_height) = buffer.dimensions();
    let mut pixels = buffer.into_raw();
    premultiply_rgba(&mut pixels);
    if source_width == width && source_height == height {
        return Some(pixels);
    }
    let premultiplied = image::RgbaImage::from_raw(source_width, source_height, pixels)?;
    let mut resized = image::imageops::resize(
        &premultiplied,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    )
    .into_raw();
    for pixel in resized.chunks_exact_mut(4) {
        pixel[0] = pixel[0].min(pixel[3]);
        pixel[1] = pixel[1].min(pixel[3]);
        pixel[2] = pixel[2].min(pixel[3]);
    }
    Some(resized)
}

#[cfg(test)]
mod raster_decode_tests {
    use super::*;

    fn encode(img: image::DynamicImage, format: image::ImageFormat) -> Vec<u8> {
        let mut output = std::io::Cursor::new(Vec::new());
        img.write_to(&mut output, format)
            .expect("encode test image");
        output.into_inner()
    }

    #[test]
    fn transparent_png_is_premultiplied() {
        let pixels = image::RgbaImage::from_raw(1, 1, vec![200, 100, 50, 128]).unwrap();
        let bytes = encode(
            image::DynamicImage::ImageRgba8(pixels),
            image::ImageFormat::Png,
        );
        let DecodedImage::Raster(data, width, height) = decode_image_bytes_ex(&bytes).unwrap()
        else {
            panic!("expected raster image");
        };
        assert_eq!((width, height), (1, 1));
        assert_eq!(data.as_slice(), &[100, 50, 25, 128]);
    }

    #[test]
    fn opaque_jpeg_keeps_full_alpha() {
        let pixels = image::RgbImage::from_raw(1, 1, vec![120, 80, 40]).unwrap();
        let bytes = encode(
            image::DynamicImage::ImageRgb8(pixels),
            image::ImageFormat::Jpeg,
        );
        let DecodedImage::Raster(data, width, height) = decode_image_bytes_ex(&bytes).unwrap()
        else {
            panic!("expected raster image");
        };
        assert_eq!((width, height), (1, 1));
        assert_eq!(data[3], 255);
    }
}

#[cfg(test)]
mod svg_decode_tests {
    use super::*;

    #[test]
    fn decoded_svg_document_reaches_image_node_with_intrinsic_size() {
        let svg = br#"<svg width="12.5" height="8"><rect width="12.5" height="8"/></svg>"#;
        let decoded = decode_image_bytes_ex(svg).expect("SVG decode");
        assert!(matches!(decoded, DecodedImage::SvgDocument(_, 12.5, 8.0)));
        let mut node = WebCore::new("img");
        set_decoded_image_on_node(&mut node, decoded);
        assert!(node.svg_document.is_some());
        assert_eq!((node.svg_viewbox_w, node.svg_viewbox_h), (12.5, 8.0));
        assert_eq!((node.image_width, node.image_height), (13, 8));
    }

    #[test]
    fn decoded_svg_background_uses_document_ratio_and_pixels() {
        let svg = br#"<svg viewBox="0 0 10 6"><rect width="10" height="6" fill="red"/></svg>"#;
        let decoded = decode_image_bytes_ex(svg).expect("SVG decode");
        let mut node = WebCore::new("div");
        assert!(set_decoded_bg_image_on_node(&mut node, decoded));
        assert!(node.bg_image_ratio_only);
        assert_eq!((node.bg_image_width, node.bg_image_height), (10, 6));
        assert_eq!(node.bg_image_data.as_ref().unwrap().len(), 10 * 6 * 4);
    }

    #[test]
    fn decoded_animated_svg_preserves_animation_tree() {
        let svg = br#"<svg width="10" height="10"><rect width="2" height="2"><animate attributeName="x" from="0" to="5" dur="1s"/></rect></svg>"#;
        let decoded = decode_image_bytes_ex(svg).expect("SVG decode");
        let mut node = WebCore::new("img");
        set_decoded_image_on_node(&mut node, decoded);
        assert!(crate::svg::animation::document_has_svg_animations(
            node.svg_document.as_ref().unwrap()
        ));
    }
}

#[cfg(test)]
mod animated_resize_tests {
    use super::resize_animated_frame;

    #[test]
    fn transparent_gif_edges_do_not_bleed_into_resized_pixels() {
        let source =
            image::RgbaImage::from_raw(2, 1, vec![255, 0, 0, 255, 255, 255, 255, 0]).unwrap();
        let pixels = resize_animated_frame(source, 5, 1).unwrap();
        assert!(
            pixels
                .chunks_exact(4)
                .all(|pixel| { pixel[1] == 0 && pixel[2] == 0 && pixel[0] <= pixel[3] })
        );
    }
}

/// Publish completed animation frames without waiting for a decoder on the UI thread.
pub(crate) fn poll_animated_image_expansion(
    animated: &mut AnimatedImage,
    target_width: u32,
    target_height: u32,
) -> bool {
    if let Some(pending) = animated.pending_decode.as_ref() {
        let ready = pending.get().cloned();
        if let Some(decoded) = ready {
            *animated = decoded;
            return true;
        }
        return false;
    }
    if animated.fully_decoded || animated.source_bytes.is_none() {
        return false;
    }
    let mut decoding = animated.clone();
    let pending = std::sync::Arc::new(std::sync::OnceLock::new());
    animated.pending_decode = Some(pending.clone());
    crate::spawn_animation_decode_task(move || {
        let _profile = crate::profile::span(crate::profile::Phase::ImageDecode);
        expand_animated_image_to_size(&mut decoding, target_width, target_height);
        let _ = pending.set(decoding);
    });
    false
}

pub(crate) fn expand_animated_image_to_size(
    animated: &mut AnimatedImage,
    target_width: u32,
    target_height: u32,
) -> bool {
    if animated.fully_decoded {
        return false;
    }
    let Some(bytes) = animated.source_bytes.clone() else {
        animated.fully_decoded = true;
        return false;
    };
    let target_size = if target_width > 0 && target_height > 0 {
        Some((
            target_width.max(animated.width),
            target_height.max(animated.height),
        ))
    } else {
        None
    };
    let decoded = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes.as_slice()))
            .ok()
            .and_then(|decoder| {
                let (width, height) = image::ImageDecoder::dimensions(&decoder);
                decode_animation_frames_with_size(
                    decoder.apng().ok()?,
                    width,
                    height,
                    None,
                    None,
                    target_size,
                )
            })
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes.as_slice()))
            .ok()
            .and_then(|decoder| decode_animation_frames_inner(decoder, None, None, target_size))
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes.as_slice()))
            .ok()
            .and_then(|decoder| decode_animation_frames_inner(decoder, None, None, target_size))
    } else {
        None
    };
    let Some(decoded) = decoded else {
        animated.fully_decoded = true;
        animated.source_bytes = None;
        return false;
    };
    *animated = decoded;
    animated.fully_decoded = true;
    animated.source_bytes = Some(bytes);
    true
}

pub(crate) fn collapse_animated_image(animated: &mut AnimatedImage, current_frame: usize) -> bool {
    // Dropping the publication slot lets an offscreen decode result be freed
    // by its worker instead of retaining all frames for an invisible image.
    animated.pending_decode = None;
    if !animated.fully_decoded || animated.source_bytes.is_none() || animated.frames.len() <= 1 {
        return false;
    }
    let frame = animated
        .frames
        .get(current_frame.min(animated.frames.len().saturating_sub(1)))
        .cloned()
        .or_else(|| animated.frames.first().cloned());
    let Some(frame) = frame else {
        return false;
    };
    animated.frames.clear();
    animated.frames.push(frame);
    animated.fully_decoded = false;
    true
}

fn animated_frame_decode_size(
    width: u32,
    height: u32,
    target_size: Option<(u32, u32)>,
) -> (u32, u32) {
    const MAX_ANIMATED_FRAME_PIXELS: u32 = 128 * 1024;
    if let Some((target_w, target_h)) = target_size {
        let target_w = target_w.max(1).min(width.max(1));
        let target_h = target_h.max(1).min(height.max(1));
        let target_pixels = target_w.saturating_mul(target_h);
        if target_pixels <= MAX_ANIMATED_FRAME_PIXELS {
            return (target_w, target_h);
        }
    }
    let pixels = width.saturating_mul(height);
    if width == 0 || height == 0 || pixels <= MAX_ANIMATED_FRAME_PIXELS {
        return (width, height);
    }
    let scale = (MAX_ANIMATED_FRAME_PIXELS as f64 / pixels as f64).sqrt();
    (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    )
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
