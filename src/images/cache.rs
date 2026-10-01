//! Decoded image cache and in-flight decode coordination.

use crate::{CacheMemoryStats, html, svg};

const DECODED_IMAGE_CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;

struct DecodedImageCache {
    entries: std::collections::HashMap<String, (html::DecodedImage, usize, u64)>,
    order: std::collections::VecDeque<(String, u64)>,
    bytes: usize,
    next_generation: u64,
}

impl DecodedImageCache {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            bytes: 0,
            next_generation: 1,
        }
    }

    fn get(&mut self, url: &str) -> Option<html::DecodedImage> {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let decoded = self.entries.get_mut(url).map(|(decoded, _, entry_gen)| {
            *entry_gen = generation;
            decoded.clone()
        })?;
        self.order.push_back((url.to_string(), generation));
        self.compact_order_if_needed();
        Some(decoded)
    }

    fn insert(&mut self, url: String, decoded: html::DecodedImage) {
        let bytes = decoded_image_footprint(&decoded);
        if bytes > DECODED_IMAGE_CACHE_MAX_BYTES {
            return;
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        if let Some((_, old_bytes, _)) = self.entries.remove(&url) {
            self.bytes = self.bytes.saturating_sub(old_bytes);
        }
        while self.bytes.saturating_add(bytes) > DECODED_IMAGE_CACHE_MAX_BYTES {
            let Some((oldest, oldest_generation)) = self.order.pop_front() else {
                break;
            };
            let should_remove = self
                .entries
                .get(&oldest)
                .is_some_and(|(_, _, entry_generation)| *entry_generation == oldest_generation);
            if should_remove && let Some((_, old_bytes, _)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old_bytes);
            }
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.order.push_back((url.clone(), generation));
        self.entries.insert(url, (decoded, bytes, generation));
        self.compact_order_if_needed();
    }

    fn compact_order_if_needed(&mut self) {
        let live = self.entries.len().max(1);
        if self.order.len() <= live.saturating_mul(4).saturating_add(32) {
            return;
        }
        self.order.retain(|(key, generation)| {
            self.entries
                .get(key)
                .is_some_and(|(_, _, entry_generation)| entry_generation == generation)
        });
    }
}

static DECODED_IMAGE_CACHE: std::sync::LazyLock<std::sync::Mutex<DecodedImageCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(DecodedImageCache::new()));

pub(crate) fn decoded_image_cache_stats() -> CacheMemoryStats {
    DECODED_IMAGE_CACHE
        .lock()
        .map(|cache| CacheMemoryStats {
            entries: cache.entries.len(),
            bytes: cache.bytes,
        })
        .unwrap_or_default()
}

fn decoded_image_footprint(decoded: &html::DecodedImage) -> usize {
    match decoded {
        html::DecodedImage::Raster(data, _, _) => data.len(),
        html::DecodedImage::Animated(animated) => {
            animated
                .frames
                .iter()
                .map(|frame| frame.pixels.len())
                .sum::<usize>()
                + animated
                    .source_bytes
                    .as_ref()
                    .map(|bytes| bytes.len())
                    .unwrap_or(0)
        }
        html::DecodedImage::Svg(markup, _, _) => markup.len(),
        html::DecodedImage::SvgDocument(doc, _, _) => svg_document_footprint(doc),
    }
}

fn svg_document_footprint(doc: &svg::SvgDocument) -> usize {
    fn node_heap_bytes(node: &svg::SvgNode) -> usize {
        let attributes = node.attributes.capacity() * std::mem::size_of::<svg::SvgAttribute>()
            + node
                .attributes
                .iter()
                .map(|attr| {
                    attr.namespace.as_ref().map_or(0, String::capacity)
                        + attr.name.capacity()
                        + attr.value.capacity()
                })
                .sum::<usize>();
        let children = node.children.capacity() * std::mem::size_of::<svg::SvgNode>()
            + node.children.iter().map(node_heap_bytes).sum::<usize>();
        let animation = node.animation.as_ref().map_or(0, std::mem::size_of_val);
        node.text.capacity() + attributes + children + animation
    }

    std::mem::size_of_val(doc) + node_heap_bytes(&doc.root)
}

struct ImageDecodeState {
    result: std::sync::Mutex<Option<Result<html::DecodedImage, String>>>,
    done: std::sync::Condvar,
}

static DECODED_IMAGE_IN_FLIGHT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<ImageDecodeState>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(test)]
pub(crate) fn cached_decoded_image(
    url: &str,
    loader: Option<&(dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static)>,
) -> Option<html::DecodedImage> {
    cached_decoded_image_result_from_option_loader(url, loader).ok()
}

pub(crate) fn cached_decoded_image_result(
    url: &str,
    loader: Option<&(dyn Fn(&str) -> Result<html::DecodedImage, String> + Send + Sync + 'static)>,
) -> Result<html::DecodedImage, String> {
    cached_decoded_image_result_inner(url, loader)
}

pub(crate) fn cached_decoded_image_result_from_option_loader(
    url: &str,
    loader: Option<&(dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static)>,
) -> Result<html::DecodedImage, String> {
    let wrapped = loader.map(|loader| {
        move |src: &str| loader(src).ok_or_else(|| format!("fetch or decode failed for {src}"))
    });
    cached_decoded_image_result_inner(
        url,
        wrapped.as_ref().map(|loader| {
            loader as &(dyn Fn(&str) -> Result<html::DecodedImage, String> + Send + Sync)
        }),
    )
}

fn cached_decoded_image_result_inner(
    url: &str,
    loader: Option<&(dyn Fn(&str) -> Result<html::DecodedImage, String> + Send + Sync)>,
) -> Result<html::DecodedImage, String> {
    if let Some(decoded) = DECODED_IMAGE_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(url))
    {
        return Ok(decoded);
    }
    let (decode_state, owns_decode) = {
        let mut in_flight = DECODED_IMAGE_IN_FLIGHT
            .lock()
            .expect("decoded image in-flight cache poisoned");
        if let Some(state) = in_flight.get(url) {
            (state.clone(), false)
        } else {
            let state = std::sync::Arc::new(ImageDecodeState {
                result: std::sync::Mutex::new(None),
                done: std::sync::Condvar::new(),
            });
            in_flight.insert(url.to_string(), state.clone());
            (state, true)
        }
    };
    if !owns_decode {
        let mut guard = decode_state
            .result
            .lock()
            .expect("decoded image result poisoned");
        while guard.is_none() {
            guard = decode_state
                .done
                .wait(guard)
                .expect("decoded image result poisoned");
        }
        return guard
            .as_ref()
            .expect("decoded image result set")
            .as_ref()
            .cloned()
            .map_err(Clone::clone);
    }
    let decoded = match loader {
        Some(loader) => loader(url),
        None => html::load_decoded_image_from_src(url, "")
            .ok_or_else(|| format!("fetch or decode failed for {url}")),
    };
    if let Ok(decoded) = decoded.as_ref()
        && let Ok(mut cache) = DECODED_IMAGE_CACHE.lock()
    {
        cache.insert(url.to_string(), decoded.clone());
    }
    {
        let mut guard = decode_state
            .result
            .lock()
            .expect("decoded image result poisoned");
        *guard = Some(decoded.clone());
        decode_state.done.notify_all();
    }
    if let Ok(mut in_flight) = DECODED_IMAGE_IN_FLIGHT.lock() {
        in_flight.remove(url);
    }
    decoded
}

pub(crate) fn cached_decoded_image_ready(url: &str) -> Option<html::DecodedImage> {
    DECODED_IMAGE_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(url))
}
