//! Bounded retained rasters for static SVG display-list images.

use crate::svg::{SvgDocument, SvgNode};
use crate::types::{Color, WebCore};
use std::collections::{HashMap, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};

const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Hash, PartialEq, Eq)]
struct RasterKey {
    svg: u64,
    dom: u64,
    ids: u64,
    props: u64,
    width: u32,
    height: u32,
    intrinsic: [u32; 2],
    colors: [Option<[u8; 4]>; 3],
}

#[derive(Default)]
struct RasterCache {
    entries: HashMap<RasterKey, (Arc<Vec<u8>>, usize, u64)>,
    order: VecDeque<(RasterKey, u64)>,
    bytes: usize,
    generation: u64,
}

impl RasterCache {
    fn get(&mut self, key: &RasterKey) -> Option<Arc<Vec<u8>>> {
        let (image, _, entry_generation) = self.entries.get_mut(key)?;
        self.generation = self.generation.wrapping_add(1).max(1);
        *entry_generation = self.generation;
        self.order.push_back((key.clone(), self.generation));
        let image = Arc::clone(image);
        self.compact();
        Some(image)
    }

    fn insert(&mut self, key: RasterKey, image: Arc<Vec<u8>>) {
        let bytes = image.len();
        if bytes > MAX_BYTES {
            return;
        }
        if let Some((_, old_bytes, _)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old_bytes);
        }
        while self.bytes.saturating_add(bytes) > MAX_BYTES {
            let Some((old_key, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&old_key)
                .is_some_and(|entry| entry.2 == generation)
                && let Some((_, old_bytes, _)) = self.entries.remove(&old_key)
            {
                self.bytes = self.bytes.saturating_sub(old_bytes);
            }
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.bytes += bytes;
        self.order.push_back((key.clone(), self.generation));
        self.entries.insert(key, (image, bytes, self.generation));
        self.compact();
    }

    fn compact(&mut self) {
        if self.order.len() > self.entries.len().saturating_mul(4).saturating_add(32) {
            self.order.retain(|(key, generation)| {
                self.entries
                    .get(key)
                    .is_some_and(|entry| entry.2 == *generation)
            });
        }
    }
}

static CACHE: LazyLock<Mutex<RasterCache>> = LazyLock::new(|| Mutex::new(RasterCache::default()));

fn color_bytes(color: Option<Color>) -> Option<[u8; 4]> {
    color.map(|c| [c.r, c.g, c.b, c.a])
}

fn hash_svg_node(node: &SvgNode, hash: &mut impl Hasher) -> bool {
    std::mem::discriminant(&node.kind).hash(hash);
    node.text.hash(hash);
    for attr in &node.attributes {
        attr.namespace.hash(hash);
        attr.name.hash(hash);
        attr.value.hash(hash);
    }
    let mut static_content = node.animation.is_none();
    if node.text.contains("animation:") || node.text.contains("transition:") {
        static_content = false;
    }
    for attr in &node.attributes {
        if attr.value.contains("animation:") || attr.value.contains("transition:") {
            static_content = false;
        }
    }
    for child in &node.children {
        static_content &= hash_svg_node(child, hash);
    }
    static_content
}

fn dom_fingerprint(root: Option<&WebCore>) -> u64 {
    let mut hash = DefaultHasher::new();
    if let Some(root) = root {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            node.tag.hash(&mut hash);
            node.text.hash(&mut hash);
            format!("{:?}", node.style).hash(&mut hash);
            stack.extend(node.children.iter());
        }
    }
    hash.finish()
}

pub(super) fn document_ids_fingerprint(ids: &HashMap<String, &SvgNode>) -> u64 {
    let mut hash = DefaultHasher::new();
    let mut sorted: Vec<_> = ids.iter().collect();
    sorted.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (id, node) in sorted {
        id.hash(&mut hash);
        hash_svg_node(node, &mut hash);
    }
    hash.finish()
}

fn props_fingerprint(props: &HashMap<String, String>) -> u64 {
    let mut hash = DefaultHasher::new();
    let mut sorted: Vec<_> = props.iter().collect();
    sorted.sort_unstable_by(|a, b| a.0.cmp(b.0));
    for (name, value) in sorted {
        name.hash(&mut hash);
        value.hash(&mut hash);
    }
    hash.finish()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rasterize(
    node_id: u32,
    paint_context: Option<(f32, f32, f32, f32)>,
    doc: &SvgDocument,
    width: u32,
    height: u32,
    intrinsic: (f32, f32),
    current_color: Color,
    fill: Option<Color>,
    stroke: Option<Color>,
    props: &HashMap<String, String>,
    dom_root: Option<&WebCore>,
    document_ids: Option<&HashMap<String, &SvgNode>>,
    ids_fingerprint: u64,
    animated: bool,
) -> Option<Arc<Vec<u8>>> {
    let _profile_key = crate::profile::span(crate::profile::Phase::SvgRasterKey);
    let mut hash = DefaultHasher::new();
    let static_content = hash_svg_node(&doc.root, &mut hash) && !animated;
    if !static_content {
        crate::profile::record(
            crate::profile::Phase::SvgRasterBypass,
            std::time::Duration::ZERO,
        );
    }
    let key = static_content.then(|| RasterKey {
        svg: hash.finish(),
        dom: dom_fingerprint(dom_root),
        ids: if document_ids.is_some() {
            ids_fingerprint
        } else {
            0
        },
        props: props_fingerprint(props),
        width,
        height,
        intrinsic: [intrinsic.0.to_bits(), intrinsic.1.to_bits()],
        colors: [
            color_bytes(Some(current_color)),
            color_bytes(fill),
            color_bytes(stroke),
        ],
    });
    if let Some(key) = key.as_ref()
        && let Ok(mut cache) = CACHE.lock()
        && let Some(image) = cache.get(key)
    {
        crate::profile::record(
            crate::profile::Phase::SvgRasterHit,
            std::time::Duration::ZERO,
        );
        return Some(image);
    }
    drop(_profile_key);
    let _profile_paint = crate::profile::span(crate::profile::Phase::SvgRasterPaint);
    let started = std::time::Instant::now();
    let image = Arc::new(crate::svg::rasterize_svg_document_to_rgba_with_dom(
        doc,
        width,
        height,
        intrinsic,
        current_color,
        fill,
        stroke,
        props,
        dom_root,
        document_ids,
    )?);
    if crate::profile::is_enabled() {
        let location =
            paint_context.map_or_else(String::new, |(border_y, content_y, top, bottom)| {
                format!(
                    " border_y:{border_y:.0} content_y:{content_y:.0} clip:{top:.0}-{bottom:.0}"
                )
            });
        crate::profile::record_resource_for(
            crate::profile::epoch(),
            "svg-raster",
            &format!("node:{node_id} {}x{}{location}", width, height),
            if static_content {
                "cache-miss"
            } else {
                "dynamic"
            },
            image.len(),
            started.elapsed(),
        );
    }
    if let Some(key) = key
        && let Ok(mut cache) = CACHE.lock()
    {
        cache.insert(key, Arc::clone(&image));
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_svg_raster_reuses_pixels_but_color_changes_key() {
        let doc = crate::svg::parse_svg_document(
            r#"<svg width="8" height="8"><rect width="8" height="8" fill="currentColor"/></svg>"#,
        )
        .unwrap();
        let props = HashMap::new();
        let render = |color| {
            rasterize(
                0,
                None,
                &doc,
                8,
                8,
                (8.0, 8.0),
                color,
                Some(Color::BLACK),
                None,
                &props,
                None,
                None,
                0,
                false,
            )
            .unwrap()
        };
        let red = render(Color::rgb(255, 0, 0));
        assert!(Arc::ptr_eq(&red, &render(Color::rgb(255, 0, 0))));
        let blue = render(Color::rgb(0, 0, 255));
        assert!(!Arc::ptr_eq(&red, &blue));
        assert_ne!(red.as_slice(), blue.as_slice());
    }

    #[test]
    fn animated_svg_raster_is_not_retained() {
        let doc = crate::svg::parse_svg_document(
            r#"<svg width="8" height="8"><rect width="8" height="8"><animate attributeName="x" from="0" to="2" dur="1s"/></rect></svg>"#,
        )
        .unwrap();
        let render = || {
            rasterize(
                0,
                None,
                &doc,
                8,
                8,
                (8.0, 8.0),
                Color::BLACK,
                Some(Color::BLACK),
                None,
                &HashMap::new(),
                None,
                None,
                0,
                false,
            )
            .unwrap()
        };
        assert!(!Arc::ptr_eq(&render(), &render()));
    }

    #[test]
    fn static_svg_text_raster_is_retained() {
        let doc = crate::svg::parse_svg_document(
            r#"<svg width="40" height="20"><text x="2" y="15">Hello</text></svg>"#,
        )
        .unwrap();
        let render = || {
            rasterize(
                0,
                None,
                &doc,
                40,
                20,
                (40.0, 20.0),
                Color::BLACK,
                Some(Color::BLACK),
                None,
                &HashMap::new(),
                None,
                None,
                0,
                false,
            )
            .unwrap()
        };
        assert!(Arc::ptr_eq(&render(), &render()));
    }
}
