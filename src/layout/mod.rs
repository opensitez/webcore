pub mod block;
pub mod constraints;
pub mod flex;
pub mod grid;
pub mod inline_layout;
pub mod perf;
mod scroll_anchor;
pub mod text;

use std::collections::{HashMap, HashSet};
pub mod hit_test;
pub mod layout_box;
pub mod table;

pub use constraints::{Constraints, FormattingContext, IntrinsicSizes};

use crate::types::*;
use std::cell::Cell;

struct HoverGeometryEntry {
    node_id: u32,
    child_count: usize,
    line_count: usize,
    run_count: usize,
    style: std::sync::Arc<ComputedStyle>,
}

struct HoverGeometrySnapshot(Vec<HoverGeometryEntry>);

#[cfg(test)]
mod hover_geometry_tests {
    use super::*;

    #[test]
    fn paint_only_hover_reuses_geometry_but_size_hover_reflows() {
        for (declaration, requires_layout) in [
            ("background:red;box-shadow:0 1px 3px black;z-index:2", false),
            ("height:80px", true),
            ("color:red", true),
        ] {
            let html = format!(
                "<style>body{{margin:0}}#row{{position:relative;width:120px;height:40px}}#row:hover{{{declaration}}}@keyframes spin{{to{{transform:rotate(360deg)}}}}#spinner{{width:20px;height:20px;animation:spin 2s linear infinite}}</style><div id='row'>Message</div>Footer<div>End</div><div id='spinner'></div>"
            );
            let mut doc = crate::html::parse_html(&html);
            let mut engine = LayoutEngine::new();
            engine.viewport_h = 300.0;
            engine.layout(&mut doc, 400.0);
            assert!(doc.needs_animation_frame);
            engine.layout_calls.set(0);
            doc.process_mouse_event(crate::dom::HtmlEventType::MouseMove, (10.0, 10.0), 0);
            engine.layout(&mut doc, 400.0);
            assert_eq!(
                engine.layout_calls.get() > 0,
                requires_layout,
                "{declaration}"
            );
            assert!(!doc.has_dirty_layout());
        }
    }
}

impl HoverGeometrySnapshot {
    fn capture(root: &WebCore) -> Option<Self> {
        let mut entries = Vec::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            if node.shadow_root.is_some() {
                return None;
            }
            entries.push(HoverGeometryEntry {
                node_id: node.node_id,
                child_count: node.children.len(),
                line_count: node.layout.line_cache.len(),
                run_count: node.layout.inline_runs.len(),
                style: node.style.clone(),
            });
            pending.extend(node.children.iter());
        }
        Some(Self(entries))
    }

    fn can_reuse(&self, root: &WebCore) -> bool {
        let mut entries = self.0.iter();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            let Some(entry) = entries.next() else {
                return false;
            };
            // Counter/generated-content replay can discard text layout even
            // when it ultimately restores identical computed style values.
            if entry.node_id != node.node_id
                || entry.child_count != node.children.len()
                || entry.line_count != node.layout.line_cache.len()
                || entry.run_count != node.layout.inline_runs.len()
                || !node.style.reuses_geometry_from(&entry.style)
            {
                return false;
            }
            pending.extend(node.children.iter());
        }
        entries.next().is_none()
    }
}

#[inline]
pub(crate) fn is_projected_svg_descendant(node: &WebCore) -> bool {
    node.svg_tree_path
        .as_ref()
        .is_some_and(|path| !path.is_empty())
}

#[inline]
pub(crate) fn is_svg_foreign_content_tag(tag: &str) -> bool {
    matches!(
        tag,
        "g" | "defs"
            | "symbol"
            | "use"
            | "path"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "text"
            | "tspan"
            | "textPath"
            | "title"
            | "desc"
            | "metadata"
            | "foreignObject"
            | "linearGradient"
            | "radialGradient"
            | "clipPath"
            | "mask"
            | "filter"
            | "feGaussianBlur"
            | "feOffset"
            | "feDropShadow"
            | "feFlood"
            | "feComposite"
            | "feBlend"
            | "feColorMatrix"
            | "feComponentTransfer"
            | "feFuncR"
            | "feFuncG"
            | "feFuncB"
            | "feFuncA"
            | "feMorphology"
            | "feMerge"
            | "feMergeNode"
            | "feImage"
            | "feTile"
            | "feConvolveMatrix"
            | "feDisplacementMap"
            | "pattern"
            | "marker"
            | "stop"
            | "switch"
            | "view"
            | "cursor"
            | "animate"
            | "animateColor"
            | "animateTransform"
            | "animateMotion"
            | "mpath"
            | "set"
    )
}

#[inline]
pub(crate) fn is_layout_inert_svg_node(node: &WebCore) -> bool {
    is_projected_svg_descendant(node) || is_svg_foreign_content_tag(&node.tag)
}

fn clear_layout_inert_svg_subtrees(node: &mut WebCore, inside_inert_svg: bool) {
    let inert = inside_inert_svg || is_layout_inert_svg_node(node);
    if inert {
        node.layout.content_rect = Rect::default();
        node.layout.padding_rect = Rect::default();
        node.layout.border_rect = Rect::default();
        node.layout.margin_rect = Rect::default();
        node.layout.line_cache.clear();
        node.layout.scroll_width = 0.0;
        node.layout.scroll_height = 0.0;
    }
    for child in &mut node.children {
        clear_layout_inert_svg_subtrees(child, inert);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            clear_layout_inert_svg_subtrees(child, inert);
        }
    }
}

pub(crate) fn clear_layout_subtree(node: &mut WebCore) {
    node.layout.content_rect = Rect::default();
    node.layout.padding_rect = Rect::default();
    node.layout.border_rect = Rect::default();
    node.layout.margin_rect = Rect::default();
    node.layout.line_cache.clear();
    node.layout.scroll_width = 0.0;
    node.layout.scroll_height = 0.0;
    node.layout.scroll_left = 0.0;
    node.layout.scroll_top = 0.0;
    for child in &mut node.children {
        clear_layout_subtree(child);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            clear_layout_subtree(child);
        }
    }
}

fn clear_layout_children(node: &mut WebCore) {
    for child in &mut node.children {
        clear_layout_subtree(child);
    }
    if let Some(shadow) = node.shadow_root.as_mut() {
        for child in &mut shadow.children {
            clear_layout_subtree(child);
        }
    }
}

fn is_native_replaced_control(node: &WebCore) -> bool {
    matches!(
        node.tag.as_str(),
        "input" | "select" | "textarea" | "progress" | "meter" | "video" | "audio"
    )
}

pub(crate) fn establishes_positioned_containing_block(style: &ComputedStyle) -> bool {
    let transform = style.transform.trim();
    let has_active_transform = !transform.is_empty() && !transform.eq_ignore_ascii_case("none");
    let filter = style.rare().filter.trim();
    let has_active_filter = !filter.is_empty() && !filter.eq_ignore_ascii_case("none");
    let backdrop_filter = style.rare().backdrop_filter.trim();
    let has_active_backdrop_filter =
        !backdrop_filter.is_empty() && !backdrop_filter.eq_ignore_ascii_case("none");

    !matches!(style.position, Position::Static)
        || has_active_transform
        || has_active_filter
        || has_active_backdrop_filter
        || style.will_change_transform
        || style.contain_layout
        || style.contain_paint
}

pub(crate) fn establishes_fixed_positioned_containing_block(style: &ComputedStyle) -> bool {
    let transform = style.transform.trim();
    let has_active_transform = !transform.is_empty() && !transform.eq_ignore_ascii_case("none");
    let filter = style.rare().filter.trim();
    let has_active_filter = !filter.is_empty() && !filter.eq_ignore_ascii_case("none");
    let backdrop_filter = style.rare().backdrop_filter.trim();
    let has_active_backdrop_filter =
        !backdrop_filter.is_empty() && !backdrop_filter.eq_ignore_ascii_case("none");

    has_active_transform
        || has_active_filter
        || has_active_backdrop_filter
        || style.will_change_transform
        || style.contain_layout
        || style.contain_paint
}

pub(crate) fn update_scroll_extents_from_children(
    node: &mut WebCore,
    content_x: f32,
    content_y: f32,
    content_w: f32,
    content_h: f32,
) {
    let (natural_scroll_w, natural_scroll_h) = node
        .children
        .iter()
        .filter(|child| {
            !matches!(child.style.display, Display::None)
                && !is_layout_inert_svg_node(child)
                && (child.layout.margin_rect.w > 0.0 || child.layout.margin_rect.h > 0.0)
        })
        .fold((content_w, content_h), |(width, height), child| {
            let own_right = child.layout.margin_rect.x + child.layout.margin_rect.w;
            let overflow_right = if matches!(child.style.overflow_x, Overflow::Visible) {
                child.layout.content_rect.x + child.layout.scroll_width
            } else {
                own_right
            };
            let own_bottom = child.layout.margin_rect.y + child.layout.margin_rect.h;
            let overflow_bottom = if matches!(child.style.overflow_y, Overflow::Visible) {
                child.layout.content_rect.y + child.layout.scroll_height
            } else {
                own_bottom
            };
            (
                width.max(own_right.max(overflow_right) - content_x),
                height.max(own_bottom.max(overflow_bottom) - content_y),
            )
        });
    node.layout.scroll_width = natural_scroll_w;
    node.layout.scroll_height = natural_scroll_h;
    if matches!(
        node.style.overflow_x,
        Overflow::Scroll | Overflow::Auto | Overflow::Hidden
    ) || matches!(
        node.style.overflow_y,
        Overflow::Scroll | Overflow::Auto | Overflow::Hidden
    ) {
        let max_scroll_x = (node.layout.scroll_width - content_w).max(0.0);
        let max_scroll_y = (node.layout.scroll_height - content_h).max(0.0);
        node.layout.scroll_left = node.layout.scroll_left.min(max_scroll_x).max(0.0);
        node.layout.scroll_top = node.layout.scroll_top.min(max_scroll_y).max(0.0);
    } else {
        node.layout.scroll_left = 0.0;
        node.layout.scroll_top = 0.0;
    }
}

// ─── Font loading helpers ──────────────────────────────────────────────────────

struct RemoteFontFetchState {
    result: std::sync::Mutex<Option<Option<std::sync::Arc<Vec<u8>>>>>,
    done: std::sync::Condvar,
}

static REMOTE_FONT_BYTES_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Vec<u8>>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
const REMOTE_FONT_BYTES_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

static REMOTE_FONT_BYTES_IN_FLIGHT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<RemoteFontFetchState>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

struct DecodedFontEntry {
    source: std::sync::Arc<Vec<u8>>,
    sfnt: std::sync::Arc<Vec<u8>>,
}

struct PendingFontResult {
    faces: Vec<crate::css::FontFaceDecl>,
    url: String,
    bytes: Option<std::sync::Arc<Vec<u8>>>,
    requested_at: std::time::Instant,
    completed_at: std::time::Instant,
}

const FONT_DISPLAY_FALLBACK_SWAP_PERIOD: std::time::Duration = std::time::Duration::from_secs(3);
const FONT_DISPLAY_OPTIONAL_LOAD_PERIOD: std::time::Duration =
    std::time::Duration::from_millis(100);

fn font_face_can_swap(face: &crate::css::FontFaceDecl, elapsed: std::time::Duration) -> bool {
    match face.display.as_deref() {
        Some("fallback") => elapsed <= FONT_DISPLAY_FALLBACK_SWAP_PERIOD,
        Some("optional") => elapsed <= FONT_DISPLAY_OPTIONAL_LOAD_PERIOD,
        _ => true,
    }
}

#[derive(Default)]
struct DecodedFontCache {
    entries: std::collections::HashMap<u64, Vec<DecodedFontEntry>>,
    bytes: usize,
}

static DECODED_FONT_CACHE: std::sync::LazyLock<std::sync::Mutex<DecodedFontCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(DecodedFontCache::default()));
const DECODED_FONT_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

fn is_font_data(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some(b"wOF2" | b"wOFF" | b"OTTO" | b"ttcf" | b"true" | b"typ1" | b"\0\x01\0\0")
    ) || is_eot_data(bytes)
}

fn is_eot_data(bytes: &[u8]) -> bool {
    crate::fonts::eot::parse_prefix(bytes)
        .ok()
        .flatten()
        .is_some_and(|header| header.total_size == bytes.len())
}

fn has_eot_magic(bytes: &[u8]) -> bool {
    bytes.get(34..36) == Some(&b"LP"[..])
}

#[cfg(test)]
mod font_data_tests {
    use super::is_font_data;
    use base64::Engine;

    #[test]
    fn font_face_auto_descriptors_override_native_selection_metadata() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fonts = cosmic_text::FontSystem::new();
        let ids = super::load_font_bytes(&mut fonts, std::sync::Arc::new(font.to_vec()));
        assert!(!ids.is_empty());
        let mut native = fonts.db().face(ids[0]).unwrap().clone();
        native.id = fontdb::ID::dummy();
        native.weight = fontdb::Weight::BOLD;
        native.style = fontdb::Style::Italic;
        native.stretch = fontdb::Stretch::Expanded;
        let id = fonts.db_mut().push_face_info(native);
        for (family, descriptors) in [
            ("Omitted Descriptor Face", ""),
            (
                "Auto Descriptor Face",
                "font-weight:auto;font-style:auto;font-width:auto;",
            ),
        ] {
            let face = crate::css::font_face::parse_font_face_body(&format!(
                "font-family:'{family}';src:local(Example);{descriptors}"
            ))
            .unwrap();
            super::register_css_font_face_alias(&mut fonts, &face, &[id]);
            let alias = fonts
                .db()
                .faces()
                .find(|info| info.families.iter().any(|(name, _)| name == family))
                .unwrap();
            assert_eq!(alias.weight, fontdb::Weight::NORMAL);
            assert_eq!(alias.style, fontdb::Style::Normal);
            assert_eq!(alias.stretch, fontdb::Stretch::Normal);
        }
    }

    #[test]
    fn local_font_face_registers_one_selected_face_not_the_whole_family() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fonts = cosmic_text::FontSystem::new();
        let ids = super::load_font_bytes(&mut fonts, std::sync::Arc::new(font.to_vec()));
        assert!(!ids.is_empty());
        let original = fonts.db().face(ids[0]).unwrap().clone();
        for weight in [fontdb::Weight::NORMAL, fontdb::Weight::BOLD] {
            let mut info = original.clone();
            info.id = fontdb::ID::dummy();
            info.families = vec![(
                "Local Selection Family".into(),
                fontdb::Language::English_UnitedStates,
            )];
            info.weight = weight;
            info.style = fontdb::Style::Normal;
            info.stretch = fontdb::Stretch::Normal;
            fonts.db_mut().push_face_info(info);
        }
        let face = crate::css::font_face::parse_font_face_body(
            "font-family:'Local Selection Alias';src:local('Local Selection Family');",
        )
        .unwrap();
        assert!(super::load_local_font_face(
            &mut fonts,
            &face,
            "Local Selection Family"
        ));
        let aliases: Vec<_> = fonts
            .db()
            .faces()
            .filter(|info| {
                info.families
                    .iter()
                    .any(|(name, _)| name == "Local Selection Alias")
            })
            .collect();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].weight, fontdb::Weight::NORMAL);
    }

    #[test]
    fn font_face_calculated_metrics_reach_the_font_metrics_pipeline() {
        let face = crate::css::font_face::parse_font_face_body("font-family:Metrics;src:local(Example);size-adjust:calc(80% + 30%);ascent-override:min(90%, 95%);descent-override:calc(10% + 10%);line-gap-override:calc(2% * 5);").unwrap();
        let metrics = super::font_face_metric_override(&face);
        assert_eq!(metrics.size_adjust, Some(1.1));
        assert_eq!(metrics.ascent, Some(0.9));
        assert_eq!(metrics.descent, Some(0.2));
        assert_eq!(metrics.line_gap, Some(0.1));
    }

    #[test]
    fn document_font_loading_skips_unused_families_after_cascade() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Used Face';src:url(data:font/woff2;base64,{encoded})}}\
             @font-face{{font-family:'Unused Face';src:url(data:font/woff2;base64,{encoded})}}\
             p{{font-family:'Used Face',serif}}</style><p>Text</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        layout.layout(&mut doc, 400.0);

        let family_is_registered = |name| {
            fonts
                .db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name(name)],
                    ..fontdb::Query::default()
                })
                .is_some()
        };
        assert!(family_is_registered("Used Face"));
        assert!(!family_is_registered("Unused Face"));
    }

    #[test]
    fn generated_text_activates_matching_unicode_range_font() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Icon Face';src:url(data:font/woff2;base64,{encoded});\
             unicode-range:U+E000-EFFF}}\
             p::before{{content:'\\e001';font-family:'Icon Face'}}</style><p>Text</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        layout.layout(&mut doc, 400.0);

        assert!(
            fonts
                .db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("Icon Face")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
    }

    #[test]
    fn hidden_text_does_not_activate_unicode_range_font() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Hidden Greek Face';\
             src:url(data:font/woff2;base64,{encoded});unicode-range:U+0370-03FF}}\
             p{{font-family:'Hidden Greek Face'}} .hidden{{display:none}}</style>\
             <p>Latin</p><p class=hidden>Ω</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        layout.layout(&mut doc, 400.0);

        assert!(
            fonts
                .db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("Hidden Greek Face")],
                    ..fontdb::Query::default()
                })
                .is_none()
        );
    }

    #[test]
    fn changed_text_activates_unicode_range_font_without_recascade() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Dynamic Greek Face';\
             src:url(data:font/woff2;base64,{encoded});unicode-range:U+0370-03FF}}\
             p{{font-family:'Dynamic Greek Face'}}</style><p id=t>Latin</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        let query = fontdb::Query {
            families: &[fontdb::Family::Name("Dynamic Greek Face")],
            ..fontdb::Query::default()
        };

        layout.layout(&mut doc, 400.0);
        assert!(fonts.db().query(&query).is_none());
        let id = doc.get_element_by_id("t").expect("paragraph");
        doc.set_text_content(id, "Ω");
        layout.layout(&mut doc, 400.0);
        assert!(fonts.db().query(&query).is_some());
    }

    #[test]
    fn unrelated_text_does_not_activate_family_scoped_unicode_range() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Scoped Greek Face';\
             src:url(data:font/woff2;base64,{encoded});unicode-range:U+0370-03FF}}\
             #latin{{font-family:'Scoped Greek Face'}}</style>\
             <ul><li id=latin>Latin</li></ul><p>Ω</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        let query = fontdb::Query {
            families: &[fontdb::Family::Name("Scoped Greek Face")],
            ..fontdb::Query::default()
        };

        layout.layout(&mut doc, 400.0);
        assert!(fonts.db().query(&query).is_none());
        let id = doc.get_element_by_id("latin").expect("paragraph");
        doc.set_text_content(id, "Ω");
        layout.layout(&mut doc, 400.0);
        assert!(fonts.db().query(&query).is_some());
    }

    #[test]
    fn first_letter_family_is_not_filtered_by_owner_text() {
        let font = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(font);
        let html = format!(
            "<style>@font-face{{font-family:'Shared First Letter Face';\
             src:url(data:font/woff2;base64,{encoded});unicode-range:U+0370-03FF}}\
             #latin{{font-family:'Shared First Letter Face'}}\
             #first::first-letter{{font-family:'Shared First Letter Face'}}</style>\
             <p id=latin>Latin</p><p id=first>Ωmega</p>"
        );
        let mut doc = crate::html::parse_html(&html);
        let mut fonts = cosmic_text::FontSystem::new();
        let mut layout = super::LayoutEngine::new();
        layout.font_system = Some(&mut fonts);
        layout.layout(&mut doc, 400.0);
        assert!(
            fonts
                .db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("Shared First Letter Face")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
    }

    fn uncompressed_eot(font: &[u8]) -> Vec<u8> {
        let mut eot = vec![0; 82];
        eot[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        eot[34..36].copy_from_slice(&0x504cu16.to_le_bytes());
        for index in 0..4 {
            eot.extend_from_slice(&0u16.to_le_bytes());
            if index < 3 {
                eot.extend_from_slice(&0u16.to_le_bytes());
            }
        }
        let total_size = (eot.len() + font.len()) as u32;
        eot[..4].copy_from_slice(&total_size.to_le_bytes());
        eot[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        eot.extend_from_slice(font);
        eot
    }

    #[test]
    fn eot_source_is_advertised_as_supported() {
        let sources = crate::css::font_face::parse_font_face_sources(
            "url(legacy.eot) format('embedded-opentype'), url(modern.woff2) format('woff2')",
        );
        assert!(super::font_source_formats_supported(&sources[0]));
        assert!(super::font_source_formats_supported(&sources[1]));
    }

    #[test]
    fn remote_font_body_decodes_mtx_as_chunks_arrive() {
        let mut payload = vec![3, 0, 0, 0, 0, 0, 14, 0, 0, 18];
        payload.extend_from_slice(&[0; 12]);
        let mut eot = uncompressed_eot(&payload);
        eot[12..16].copy_from_slice(&4u32.to_le_bytes());
        for chunk_size in [1, 3, 7, 16384] {
            let mut body = super::RemoteFontBody::default();
            for chunk in eot.chunks(chunk_size) {
                body.push(chunk).unwrap();
            }
            let (raw, blocks) = body.finish().unwrap();
            assert_eq!(raw, eot);
            assert_eq!(blocks.unwrap(), [Vec::<u8>::new(), Vec::new(), Vec::new()]);
        }
    }

    #[test]
    fn streamed_remote_eot_matches_full_decode() {
        let eot = crate::fonts::compressed_eot_fixture();
        let mut body = super::RemoteFontBody::default();
        for chunk in eot.chunks(4093) {
            body.push(chunk).unwrap();
        }
        let (raw, blocks) = body.finish().unwrap();
        assert_eq!(raw, eot);
        let blocks = blocks.expect("compressed EOT blocks decoded during fetch");
        let header = crate::fonts::eot::parse_prefix(&raw).unwrap().unwrap();
        for (index, block) in blocks.iter().enumerate() {
            assert_eq!(
                block,
                &header.decode_mtx_stream(&raw, index).unwrap().unwrap()
            );
        }
        let page_url = "https://example.com/";
        let streamed =
            crate::fonts::eot::decode_for_page_with_mtx_blocks(&raw, page_url, Some(blocks));
        let full = crate::fonts::eot::decode_for_page(&raw, page_url);
        assert_eq!(streamed, full);
    }

    #[test]
    fn accepts_font_signatures_and_rejects_http_error_pages() {
        for signature in [b"wOF2", b"wOFF", b"OTTO", b"ttcf", b"\0\x01\0\0"] {
            assert!(is_font_data(signature));
        }
        assert!(!is_font_data(b"<html><body>404 Not Found</body></html>"));
        assert!(!is_font_data(b""));
    }

    #[test]
    fn eot_font_registration_checks_rights_before_adding_a_face() {
        let woff = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let sfnt = crate::woff::decode(woff).unwrap();
        let eot = uncompressed_eot(&sfnt);
        assert!(is_font_data(&eot));
        let mut malformed = eot.clone();
        malformed[..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(
            super::decode_font_bytes_for_page(
                std::sync::Arc::new(malformed),
                "https://example.com/page"
            )
            .is_none()
        );
        let mut fs = cosmic_text::FontSystem::new();
        let face = crate::css::FontFaceDecl {
            family: "EOT Test Face".into(),
            ..Default::default()
        };
        assert!(super::load_font_face_bytes_for_page(
            &mut fs,
            &face,
            std::sync::Arc::new(eot.clone()),
            "https://example.com/page",
        ));
        assert!(
            fs.db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("EOT Test Face")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
        let mut restricted = eot;
        restricted[32..34].copy_from_slice(&0x0002u16.to_le_bytes());
        assert!(!super::load_font_face_bytes_for_page(
            &mut fs,
            &face,
            std::sync::Arc::new(restricted),
            "https://example.com/page",
        ));
    }

    #[test]
    fn cached_eot_reuses_sfnt_without_bypassing_embedding_rights() {
        let woff = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let sfnt = crate::woff::decode(woff).unwrap();
        let eot = std::sync::Arc::new(uncompressed_eot(&sfnt));
        let first = super::decode_font_bytes_for_page(eot.clone(), "https://example.test/a")
            .expect("first EOT decode");
        let second = super::decode_font_bytes_for_page(eot.clone(), "https://example.test/b")
            .expect("cached EOT decode");
        assert!(std::sync::Arc::ptr_eq(&first, &second));

        let mut restricted = (*eot).clone();
        restricted[32..34].copy_from_slice(&0x0004u16.to_le_bytes());
        assert!(
            super::decode_font_bytes_for_page(
                std::sync::Arc::new(restricted),
                "https://example.test/a",
            )
            .is_none()
        );

        let mut rooted = uncompressed_eot(&sfnt);
        let v1_header = crate::fonts::eot::parse_prefix(&rooted).unwrap().unwrap();
        rooted[8..12].copy_from_slice(&0x0002_0001u32.to_le_bytes());
        rooted.splice(v1_header.font_data.start..v1_header.font_data.start, [0; 4]);
        let total_size = rooted.len() as u32;
        rooted[..4].copy_from_slice(&total_size.to_le_bytes());
        let header = crate::fonts::eot::parse_prefix(&rooted).unwrap().unwrap();
        let root: Vec<u8> = "https://example.test/allowed/"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let start = header.root_string.start;
        rooted[start - 2..start].copy_from_slice(&(root.len() as u16).to_le_bytes());
        rooted.splice(start..start, root);
        let total_size = rooted.len() as u32;
        rooted[..4].copy_from_slice(&total_size.to_le_bytes());
        let rooted = std::sync::Arc::new(rooted);
        let allowed =
            super::decode_font_bytes_for_page(rooted.clone(), "https://example.test/allowed/page")
                .expect("authorized page should decode the EOT");
        assert_eq!(&*allowed, &sfnt);
        assert!(
            super::decode_font_bytes_for_page(rooted, "https://example.test/blocked/page")
                .is_none(),
            "a cached SFNT must not bypass the EOT RootString",
        );
    }

    #[test]
    fn rejected_font_sources_try_the_next_authored_candidate() {
        let woff = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let encoded = base64::engine::general_purpose::STANDARD.encode(woff);
        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let remote_url = "https://example.test/failed.eot".to_string();
        engine.scheduled_font_faces.insert(remote_url.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(super::PendingFontResult {
            faces: Vec::new(),
            url: remote_url.clone(),
            bytes: None,
            requested_at: std::time::Instant::now(),
            completed_at: std::time::Instant::now(),
        })
        .unwrap();
        engine.pending_fonts.push(rx);
        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        assert!(engine.failed_font_sources.contains(&remote_url));
        assert!(!engine.scheduled_font_faces.contains(&remote_url));

        let face = crate::css::FontFaceDecl {
            family: "Fallback Face".into(),
            sources: vec![
                crate::css::FontFaceSource {
                    kind: crate::css::FontFaceSourceKind::Url(remote_url),
                    formats: Vec::new(),
                    techs: Vec::new(),
                },
                crate::css::FontFaceSource {
                    kind: crate::css::FontFaceSourceKind::Url(
                        "data:font/woff2;base64,bm90Zm9udA==".into(),
                    ),
                    formats: Vec::new(),
                    techs: Vec::new(),
                },
                crate::css::FontFaceSource {
                    kind: crate::css::FontFaceSourceKind::Url(format!(
                        "data:font/woff2;base64,{encoded}"
                    )),
                    formats: Vec::new(),
                    techs: Vec::new(),
                },
            ],
            ..Default::default()
        };
        engine.load_font_faces(&[face], "https://example.test/page", "");
        assert!(
            fs.db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("Fallback Face")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
    }

    #[test]
    fn font_display_late_faces_keep_fallback_without_relayout() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        for (display, age) in [
            ("fallback", std::time::Duration::from_secs(4)),
            ("optional", std::time::Duration::from_millis(200)),
        ] {
            let mut fs = cosmic_text::FontSystem::new();
            let mut engine = super::LayoutEngine::new();
            engine.font_system = Some(&mut fs);
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(super::PendingFontResult {
                faces: vec![crate::css::FontFaceDecl {
                    family: format!("Late {display}"),
                    display: Some(display.into()),
                    ..Default::default()
                }],
                url: format!("https://example.test/{display}.woff2"),
                bytes: Some(std::sync::Arc::new(bytes.to_vec())),
                requested_at: std::time::Instant::now() - age,
                completed_at: std::time::Instant::now(),
            })
            .unwrap();
            engine.pending_fonts.push(rx);
            assert!(!engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
            assert!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(&format!("Late {display}"))],
                        ..fontdb::Query::default()
                    })
                    .is_none()
            );
        }
    }

    #[test]
    fn font_display_on_time_faces_register_and_relayout() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        for display in ["fallback", "optional", "swap", "block"] {
            let mut fs = cosmic_text::FontSystem::new();
            let mut engine = super::LayoutEngine::new();
            engine.font_system = Some(&mut fs);
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(super::PendingFontResult {
                faces: vec![crate::css::FontFaceDecl {
                    family: format!("Ready {display}"),
                    display: Some(display.into()),
                    ..Default::default()
                }],
                url: format!("https://example.test/{display}.woff2"),
                bytes: Some(std::sync::Arc::new(bytes.to_vec())),
                requested_at: std::time::Instant::now(),
                completed_at: std::time::Instant::now(),
            })
            .unwrap();
            engine.pending_fonts.push(rx);
            assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
            assert!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(&format!("Ready {display}"))],
                        ..fontdb::Query::default()
                    })
                    .is_some()
            );
        }
    }

    #[test]
    fn font_display_uses_completion_time_despite_delayed_poll() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let (tx, rx) = std::sync::mpsc::channel();
        let completed_at = std::time::Instant::now() - std::time::Duration::from_secs(4);
        tx.send(super::PendingFontResult {
            faces: vec![crate::css::FontFaceDecl {
                family: "On Time Optional".into(),
                display: Some("optional".into()),
                ..Default::default()
            }],
            url: "https://example.test/shared.woff2".into(),
            bytes: Some(std::sync::Arc::new(bytes.to_vec())),
            requested_at: completed_at - std::time::Duration::from_millis(50),
            completed_at,
        })
        .unwrap();
        engine.pending_fonts.push(rx);
        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        assert!(
            fs.db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("On Time Optional")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
    }

    #[test]
    fn font_display_filters_shared_source_faces_independently() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(super::PendingFontResult {
            faces: ["Expired Optional", "Swap Face"]
                .into_iter()
                .zip(["optional", "swap"])
                .map(|(family, display)| crate::css::FontFaceDecl {
                    family: family.into(),
                    display: Some(display.into()),
                    ..Default::default()
                })
                .collect(),
            url: "https://example.test/shared.woff2".into(),
            bytes: Some(std::sync::Arc::new(bytes.to_vec())),
            requested_at: std::time::Instant::now() - std::time::Duration::from_millis(200),
            completed_at: std::time::Instant::now(),
        })
        .unwrap();
        engine.pending_fonts.push(rx);
        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        for (family, expected) in [("Expired Optional", false), ("Swap Face", true)] {
            assert_eq!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(family)],
                        ..fontdb::Query::default()
                    })
                    .is_some(),
                expected,
                "{family}"
            );
        }
    }

    #[test]
    fn decoded_but_invalid_font_source_retries_next_candidate() {
        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let url = "https://example.test/invalid.woff2".to_string();
        engine.scheduled_font_faces.insert(url.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(super::PendingFontResult {
            faces: vec![crate::css::FontFaceDecl {
                family: "Invalid Face".into(),
                ..Default::default()
            }],
            url: url.clone(),
            bytes: Some(std::sync::Arc::new(b"not a font".to_vec())),
            requested_at: std::time::Instant::now(),
            completed_at: std::time::Instant::now(),
        })
        .unwrap();
        engine.pending_fonts.push(rx);
        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        assert!(engine.failed_font_sources.contains(&url));
        assert!(!engine.scheduled_font_faces.contains(&url));
    }

    #[test]
    fn eot_source_is_selected_or_falls_back_according_to_rights() {
        let woff = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let sfnt = crate::woff::decode(woff).unwrap();
        let fallback = format!(
            "data:font/woff2;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(woff)
        );
        let eot_url = |rights: u16| {
            let mut eot = uncompressed_eot(&sfnt);
            eot[32..34].copy_from_slice(&rights.to_le_bytes());
            format!(
                "data:application/vnd.ms-fontobject;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(eot)
            )
        };
        for (rights, eot_selected) in [(0, true), (0x0004, false)] {
            let mut fs = cosmic_text::FontSystem::new();
            let mut engine = super::LayoutEngine::new();
            engine.font_system = Some(&mut fs);
            let eot = eot_url(rights);
            let face = crate::css::FontFaceDecl {
                family: "EOT Source Face".into(),
                sources: vec![
                    crate::css::FontFaceSource {
                        kind: crate::css::FontFaceSourceKind::Url(eot.clone()),
                        formats: vec!["embedded-opentype".into()],
                        techs: Vec::new(),
                    },
                    crate::css::FontFaceSource {
                        kind: crate::css::FontFaceSourceKind::Url(fallback.clone()),
                        formats: vec!["woff2".into()],
                        techs: Vec::new(),
                    },
                ],
                ..Default::default()
            };
            engine.load_font_faces(&[face], "https://example.test/page", "");
            assert_eq!(engine.scheduled_font_faces.contains(&eot), eot_selected);
            assert_eq!(
                engine.scheduled_font_faces.contains(&fallback),
                !eot_selected
            );
            assert!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name("EOT Source Face")],
                        ..fontdb::Query::default()
                    })
                    .is_some()
            );
        }
    }

    #[test]
    fn shared_font_bytes_register_each_css_family_once() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fs = cosmic_text::FontSystem::new();
        let before = fs.db().len();
        let faces = [
            crate::css::FontFaceDecl {
                family: "Shared Face One".into(),
                ..Default::default()
            },
            crate::css::FontFaceDecl {
                family: "Shared Face Two".into(),
                ..Default::default()
            },
        ];
        assert!(super::load_font_faces_bytes(
            &mut fs,
            &faces,
            std::sync::Arc::new(bytes.to_vec())
        ));
        assert_eq!(fs.db().len(), before + 3);
        for face in &faces {
            assert!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(&face.family)],
                        ..fontdb::Query::default()
                    })
                    .is_some()
            );
        }
    }

    #[test]
    fn later_stylesheet_alias_reuses_loaded_remote_font() {
        let url = "https://example.test/streamed-font-alias-test.woff2";
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        super::REMOTE_FONT_BYTES_CACHE
            .lock()
            .unwrap()
            .insert(url.into(), std::sync::Arc::new(bytes.to_vec()));

        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let face = |family: &str| crate::css::FontFaceDecl {
            family: family.into(),
            src: format!("url('{url}')"),
            ..Default::default()
        };
        let first = face("First CSS Family");
        engine.load_font_faces(
            std::slice::from_ref(&first),
            "https://example.test/page",
            "",
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO) {
            assert!(
                std::time::Instant::now() < deadline,
                "font task did not complete"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let after_first = fs.db().len();

        let faces = [first, face("Later CSS Family")];
        engine.load_font_faces(&faces, "https://example.test/page", "");
        assert!(
            fs.db()
                .query(&fontdb::Query {
                    families: &[fontdb::Family::Name("Later CSS Family")],
                    ..fontdb::Query::default()
                })
                .is_some()
        );
        assert_eq!(fs.db().len(), after_first + 1);
        engine.load_font_faces(&faces, "https://example.test/page", "");
        assert_eq!(fs.db().len(), after_first + 1);
        super::REMOTE_FONT_BYTES_CACHE.lock().unwrap().remove(url);
    }

    #[test]
    fn budgeted_font_poll_keeps_queued_results_after_workers_finish() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let mut fs = cosmic_text::FontSystem::new();
        let mut engine = super::LayoutEngine::new();
        engine.font_system = Some(&mut fs);
        let (tx, rx) = std::sync::mpsc::channel();
        for family in ["First Queued Face", "Second Queued Face"] {
            tx.send(super::PendingFontResult {
                faces: vec![crate::css::FontFaceDecl {
                    family: family.into(),
                    ..Default::default()
                }],
                url: format!("https://example.test/{family}.woff2"),
                bytes: Some(std::sync::Arc::new(bytes.to_vec())),
                requested_at: std::time::Instant::now(),
                completed_at: std::time::Instant::now(),
            })
            .unwrap();
        }
        drop(tx);
        engine.pending_fonts.push(rx);

        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        assert!(engine.has_pending_fonts());
        assert!(engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        for family in ["First Queued Face", "Second Queued Face"] {
            assert!(
                fs.db()
                    .query(&fontdb::Query {
                        families: &[fontdb::Family::Name(family)],
                        ..fontdb::Query::default()
                    })
                    .is_some()
            );
        }
        assert!(!engine.poll_pending_fonts_budgeted(1, std::time::Duration::ZERO));
        assert!(!engine.has_pending_fonts());
    }

    #[test]
    fn identical_woff_bytes_reuse_decoded_font_across_allocations() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        let first = super::decode_font_bytes(std::sync::Arc::new(bytes.to_vec())).expect("decode");
        let second =
            super::decode_font_bytes(std::sync::Arc::new(bytes.to_vec())).expect("cache hit");
        assert!(std::sync::Arc::ptr_eq(&first, &second));
    }

    #[test]
    #[ignore]
    fn benchmark_woff2_decode_and_fontdb_registration() {
        let bytes = include_bytes!("../tests/fixtures/fonts/bootstrap-icons-1.11.3.woff2");
        const ITERATIONS: u32 = 30;
        let decoded = std::sync::Arc::new(crate::woff::decode(bytes).expect("decode"));

        let started = std::time::Instant::now();
        for _ in 0..ITERATIONS {
            std::hint::black_box(crate::woff::decode(std::hint::black_box(bytes)).expect("decode"));
        }
        let decode_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERATIONS);

        let started = std::time::Instant::now();
        for _ in 0..ITERATIONS {
            let mut db = fontdb::Database::new();
            std::hint::black_box(db.load_font_source(fontdb::Source::Binary(decoded.clone())));
        }
        let register_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERATIONS);

        super::decode_font_bytes(std::sync::Arc::new(bytes.to_vec())).expect("warm cache");
        let started = std::time::Instant::now();
        for _ in 0..ITERATIONS {
            let source = std::sync::Arc::new(bytes.to_vec());
            std::hint::black_box(super::decode_font_bytes(source).expect("cached decode"));
        }
        let cached_ms = started.elapsed().as_secs_f64() * 1000.0 / f64::from(ITERATIONS);
        eprintln!(
            "WOFF2 decode: {decode_ms:.2} ms; cached load: {cached_ms:.2} ms; fontdb registration: {register_ms:.2} ms"
        );
    }
}

fn cached_remote_font_bytes(
    url: &str,
    cache_dir: Option<&str>,
) -> Option<(std::sync::Arc<Vec<u8>>, Option<[Vec<u8>; 3]>)> {
    if let Some(bytes) = REMOTE_FONT_BYTES_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(url).cloned())
    {
        return Some((bytes, None));
    }

    let (state, owns_fetch) = {
        let mut in_flight = REMOTE_FONT_BYTES_IN_FLIGHT
            .lock()
            .expect("remote font in-flight cache poisoned");
        if let Some(state) = in_flight.get(url) {
            (state.clone(), false)
        } else {
            let state = std::sync::Arc::new(RemoteFontFetchState {
                result: std::sync::Mutex::new(None),
                done: std::sync::Condvar::new(),
            });
            in_flight.insert(url.to_string(), state.clone());
            (state, true)
        }
    };

    if !owns_fetch {
        let mut guard = state.result.lock().expect("remote font result poisoned");
        while guard.is_none() {
            guard = state.done.wait(guard).expect("remote font result poisoned");
        }
        return guard
            .as_ref()
            .and_then(|bytes| bytes.as_ref().cloned())
            .map(|bytes| (bytes, None));
    }

    let result = cache_dir
        .and_then(|dir| crate::loading::cached_fetch_bytes_arc(url, dir).ok())
        .filter(|bytes| is_font_data(bytes))
        .map(|bytes| (bytes, None))
        .or_else(|| {
            fetch_remote_font_bytes(url)
                .filter(|(bytes, _)| is_font_data(bytes))
                .map(|(bytes, blocks)| (std::sync::Arc::new(bytes), blocks))
        })
        .and_then(|(bytes, blocks)| {
            if is_eot_data(&bytes) {
                Some((bytes, blocks))
            } else {
                decode_font_bytes(bytes).map(|decoded| (decoded, None))
            }
        });
    if let Some((bytes, _)) = result.as_ref()
        && let Ok(mut cache) = REMOTE_FONT_BYTES_CACHE.lock()
    {
        let existing_bytes: usize = cache
            .iter()
            .filter(|(key, _)| key.as_str() != url)
            .map(|(_, bytes)| bytes.len())
            .sum();
        if cache.len() > 256
            || existing_bytes.saturating_add(bytes.len()) > REMOTE_FONT_BYTES_CACHE_MAX_BYTES
        {
            cache.clear();
        }
        cache.insert(url.to_string(), bytes.clone());
    }
    {
        let mut guard = state.result.lock().expect("remote font result poisoned");
        *guard = Some(result.as_ref().map(|(bytes, _)| bytes.clone()));
        state.done.notify_all();
    }
    if let Ok(mut in_flight) = REMOTE_FONT_BYTES_IN_FLIGHT.lock() {
        in_flight.remove(url);
    }
    result
}

#[derive(Default)]
struct RemoteFontBody {
    bytes: Vec<u8>,
    mtx: Option<(crate::fonts::eot::MtxStreamDecoder, usize, usize)>,
    mtx_failed: bool,
}

impl RemoteFontBody {
    fn push(&mut self, chunk: &[u8]) -> Option<()> {
        if self.bytes.len().checked_add(chunk.len())? > REMOTE_FONT_BYTES_CACHE_MAX_BYTES {
            return None;
        }
        self.bytes.extend_from_slice(chunk);
        if !self.mtx_failed && self.mtx.is_none() && has_eot_magic(&self.bytes) {
            match crate::fonts::eot::parse_prefix(&self.bytes) {
                Ok(Some(header)) => match header.mtx_header(&self.bytes) {
                    Ok(Some(mtx)) => {
                        let start = header.font_data.start + mtx.streams[0].start;
                        self.mtx = Some((
                            crate::fonts::eot::MtxStreamDecoder::new(&header, &mtx),
                            start,
                            header.font_data.end,
                        ));
                    }
                    Err(_) => self.mtx_failed = true,
                    Ok(None) => {}
                },
                Err(_) => self.mtx_failed = true,
                Ok(None) => {}
            }
        }
        if let Some((decoder, next, end)) = self.mtx.as_mut() {
            let available = self.bytes.len().min(*end);
            if available > *next {
                if decoder.push(*next, &self.bytes[*next..available]).is_ok() {
                    *next = available;
                } else {
                    self.mtx = None;
                    self.mtx_failed = true;
                }
            }
        }
        Some(())
    }

    fn finish(self) -> Option<(Vec<u8>, Option<[Vec<u8>; 3]>)> {
        if self.bytes.is_empty() {
            return None;
        }
        let blocks = self.mtx.and_then(|(decoder, _, _)| decoder.finish().ok());
        Some((self.bytes, blocks))
    }
}

fn fetch_remote_font_bytes(url: &str) -> Option<(Vec<u8>, Option<[Vec<u8>; 3]>)> {
    let fetch = |client: &reqwest::blocking::Client| {
        let mut response = client
            .get(url)
            .header(
                "Accept",
                "font/woff2,font/woff,application/font-woff,*/*;q=0.5",
            )
            .header("Sec-Fetch-Dest", "font")
            .header("Sec-Fetch-Mode", "cors")
            .send()
            .ok()
            .filter(|r| r.status().is_success())?;
        let mut body = RemoteFontBody::default();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let count = std::io::Read::read(&mut response, &mut buffer).ok()?;
            if count == 0 {
                break;
            }
            body.push(&buffer[..count])?;
        }
        body.finish()
    };
    fetch(&crate::http_client()).or_else(|| fetch(&crate::http_client_lenient()))
}

fn decode_font_bytes(data: std::sync::Arc<Vec<u8>>) -> Option<std::sync::Arc<Vec<u8>>> {
    if !data.as_slice().starts_with(&crate::woff::WOFF2_MAGIC)
        && !data.as_slice().starts_with(&crate::woff::WOFF1_MAGIC)
    {
        return Some(data);
    }

    if let Some(hit) = cached_decoded_font(&data) {
        return Some(hit);
    }
    let sfnt = std::sync::Arc::new(crate::woff::decode(data.as_slice())?);
    cache_decoded_font(data, sfnt.clone());
    Some(sfnt)
}

fn decoded_font_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn cached_decoded_font(data: &std::sync::Arc<Vec<u8>>) -> Option<std::sync::Arc<Vec<u8>>> {
    let hash = decoded_font_hash(data);
    if let Ok(cache) = DECODED_FONT_CACHE.lock()
        && let Some(hit) = cache.entries.get(&hash).and_then(|bucket| {
            bucket
                .iter()
                .find(|entry| entry.source.as_slice() == data.as_slice())
        })
    {
        return Some(hit.sfnt.clone());
    }
    None
}

fn cache_decoded_font(data: std::sync::Arc<Vec<u8>>, sfnt: std::sync::Arc<Vec<u8>>) {
    let hash = decoded_font_hash(&data);
    let entry_bytes = data.len().saturating_add(sfnt.len());
    if entry_bytes <= DECODED_FONT_CACHE_MAX_BYTES
        && let Ok(mut cache) = DECODED_FONT_CACHE.lock()
    {
        if cache.bytes.saturating_add(entry_bytes) > DECODED_FONT_CACHE_MAX_BYTES {
            cache.entries.clear();
            cache.bytes = 0;
        }
        cache
            .entries
            .entry(hash)
            .or_default()
            .push(DecodedFontEntry { source: data, sfnt });
        cache.bytes += entry_bytes;
    }
}

fn decode_font_bytes_for_page(
    data: std::sync::Arc<Vec<u8>>,
    page_url: &str,
) -> Option<std::sync::Arc<Vec<u8>>> {
    decode_font_bytes_for_page_with_mtx_blocks(data, page_url, None)
}

fn decode_font_bytes_for_page_with_mtx_blocks(
    data: std::sync::Arc<Vec<u8>>,
    page_url: &str,
    blocks: Option<[Vec<u8>; 3]>,
) -> Option<std::sync::Arc<Vec<u8>>> {
    if has_eot_magic(&data) {
        if let Some(hit) = cached_decoded_font(&data) {
            return crate::fonts::eot::permits_page(&data, page_url).then_some(hit);
        }
        let sfnt = std::sync::Arc::new(crate::fonts::eot::decode_for_page_with_mtx_blocks(
            &data, page_url, blocks,
        )?);
        cache_decoded_font(data, sfnt.clone());
        return Some(sfnt);
    }
    decode_font_bytes(data)
}

/// Load raw or decoded font bytes into the font system.
fn load_font_bytes(
    fs: &mut cosmic_text::FontSystem,
    data: std::sync::Arc<Vec<u8>>,
) -> Vec<fontdb::ID> {
    let Some(font_data) = decode_font_bytes(data) else {
        return Vec::new();
    };
    fs.db_mut()
        .load_font_source(fontdb::Source::Binary(font_data))
        .into_iter()
        .collect()
}

fn css_font_family_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_font_face_weight(raw: Option<&str>) -> Option<fontdb::Weight> {
    let first = raw
        .unwrap_or("auto")
        .split_whitespace()
        .next()?
        .trim()
        .to_ascii_lowercase();
    match first.as_str() {
        "auto" | "normal" => Some(fontdb::Weight::NORMAL),
        "bold" => Some(fontdb::Weight::BOLD),
        _ => first
            .parse::<f32>()
            .ok()
            .filter(|n| {
                (crate::css::font::MIN_FONT_WEIGHT..=crate::css::font::MAX_FONT_WEIGHT).contains(n)
            })
            .map(|number| fontdb::Weight(number.round() as u16)),
    }
}

fn parse_font_face_style(raw: Option<&str>) -> Option<fontdb::Style> {
    let first = raw
        .unwrap_or("auto")
        .split_whitespace()
        .next()?
        .trim()
        .to_ascii_lowercase();
    match first.as_str() {
        "auto" | "normal" => Some(fontdb::Style::Normal),
        "italic" | "left" | "right" => Some(fontdb::Style::Italic),
        "oblique" => Some(fontdb::Style::Oblique),
        _ => None,
    }
}

fn parse_font_face_stretch(raw: Option<&str>) -> Option<fontdb::Stretch> {
    let first = raw
        .unwrap_or("auto")
        .split_whitespace()
        .next()?
        .trim()
        .to_ascii_lowercase();
    if let Some(percent) = first.strip_suffix('%') {
        return percent
            .parse::<f32>()
            .ok()
            .map(crate::layout::inline_layout::stretch_from_percent);
    }
    match first.as_str() {
        "ultra-condensed" => Some(fontdb::Stretch::UltraCondensed),
        "extra-condensed" => Some(fontdb::Stretch::ExtraCondensed),
        "condensed" => Some(fontdb::Stretch::Condensed),
        "semi-condensed" => Some(fontdb::Stretch::SemiCondensed),
        "auto" | "normal" => Some(fontdb::Stretch::Normal),
        "semi-expanded" => Some(fontdb::Stretch::SemiExpanded),
        "expanded" => Some(fontdb::Stretch::Expanded),
        "extra-expanded" => Some(fontdb::Stretch::ExtraExpanded),
        "ultra-expanded" => Some(fontdb::Stretch::UltraExpanded),
        _ => None,
    }
}

fn parse_font_face_metric_percent(raw: Option<&str>) -> Option<f32> {
    let value = raw?.trim();
    if value.eq_ignore_ascii_case("normal") {
        return None;
    }
    let number = value.strip_suffix('%')?.trim().parse::<f32>().ok()?;
    if number.is_finite() && number >= 0.0 {
        Some(number / 100.0)
    } else {
        None
    }
}

pub(crate) fn is_latin_font_face(face: &crate::css::FontFaceDecl) -> bool {
    crate::css::font_face::unicode_range_intersects_latin(face.unicode_range.as_deref())
}

fn font_face_metric_override(
    face: &crate::css::FontFaceDecl,
) -> crate::layout::inline_layout::FontMetricOverride {
    crate::layout::inline_layout::FontMetricOverride {
        size_adjust: parse_font_face_metric_percent(face.size_adjust.as_deref()),
        ascent: parse_font_face_metric_percent(face.ascent_override.as_deref()),
        descent: parse_font_face_metric_percent(face.descent_override.as_deref()),
        line_gap: parse_font_face_metric_percent(face.line_gap_override.as_deref()),
    }
}

fn register_css_font_face_alias(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    ids: &[fontdb::ID],
) {
    let Some(css_family) = css_font_family_name(&face.family) else {
        return;
    };
    let css_weight = parse_font_face_weight(face.weight.as_deref());
    let css_style = parse_font_face_style(face.style.as_deref());
    let css_stretch = parse_font_face_stretch(face.stretch.as_deref());

    let aliases: Vec<_> = ids
        .iter()
        .filter_map(|id| fs.db().face(*id).cloned())
        .map(|mut info| {
            info.id = fontdb::ID::dummy();
            info.families
                .retain(|(name, _)| !name.eq_ignore_ascii_case(&css_family));
            info.families.insert(
                0,
                (css_family.clone(), fontdb::Language::English_UnitedStates),
            );
            if let Some(weight) = css_weight {
                info.weight = weight;
            }
            if let Some(style) = css_style {
                info.style = style;
            }
            if let Some(stretch) = css_stretch {
                info.stretch = stretch;
            }
            info
        })
        .collect();

    if aliases.is_empty() {
        return;
    }

    for alias in aliases {
        fs.db_mut().push_face_info(alias);
    }
    crate::layout::inline_layout::set_font_metric_override(
        &css_family,
        font_face_metric_override(face),
    );
    crate::layout::inline_layout::clear_font_family_caches();
}

fn load_font_face_bytes(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    bytes: std::sync::Arc<Vec<u8>>,
) -> bool {
    load_font_faces_bytes(fs, std::slice::from_ref(face), bytes)
}

fn load_font_face_bytes_for_page(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    bytes: std::sync::Arc<Vec<u8>>,
    page_url: &str,
) -> bool {
    let Some(decoded) = decode_font_bytes_for_page(bytes, page_url) else {
        return false;
    };
    load_font_face_bytes(fs, face, decoded)
}

fn load_font_faces_bytes(
    fs: &mut cosmic_text::FontSystem,
    faces: &[crate::css::FontFaceDecl],
    bytes: std::sync::Arc<Vec<u8>>,
) -> bool {
    let ids = load_font_bytes(fs, bytes);
    if ids.is_empty() {
        return false;
    }
    for face in faces {
        register_css_font_face_alias(fs, face, &ids);
    }
    true
}

fn font_source_formats_supported(source: &crate::css::FontFaceSource) -> bool {
    source.formats.is_empty()
        || source
            .formats
            .iter()
            .any(|format| crate::css::font_face::supports_font_format(format))
}

pub(crate) fn font_source_techs_supported(source: &crate::css::FontFaceSource) -> bool {
    source
        .techs
        .iter()
        .all(|tech| crate::css::font_face::supports_font_tech(tech))
}

fn load_local_font_face(
    fs: &mut cosmic_text::FontSystem,
    face: &crate::css::FontFaceDecl,
    name: &str,
) -> bool {
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(name)],
        weight: parse_font_face_weight(face.weight.as_deref()).unwrap_or(fontdb::Weight::NORMAL),
        stretch: parse_font_face_stretch(face.stretch.as_deref())
            .unwrap_or(fontdb::Stretch::Normal),
        style: parse_font_face_style(face.style.as_deref()).unwrap_or(fontdb::Style::Normal),
    };
    let Some(id) = fs.db().query(&query) else {
        return false;
    };
    register_css_font_face_alias(fs, face, &[id]);
    true
}

/// Minimal Base64 decoder (no external dependency).
/// Returns `Err` on invalid input.
fn decode_base64(s: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 128] = b"\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\
        \xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff\x3e\xff\xff\xff\x3f\
        \x34\x35\x36\x37\x38\x39\x3a\x3b\x3c\x3d\xff\xff\xff\xff\xff\xff\
        \xff\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\
        \x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\xff\xff\xff\xff\xff\
        \xff\x1a\x1b\x1c\x1d\x1e\x1f\x20\x21\x22\x23\x24\x25\x26\x27\x28\
        \x29\x2a\x2b\x2c\x2d\x2e\x2f\x30\x31\x32\x33\xff\xff\xff\xff\xff";

    let s: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut i = 0;
    while i + 3 < s.len() {
        let a = s[i];
        let b = s[i + 1];
        let c = s[i + 2];
        let d = s[i + 3];
        if a >= 128 || b >= 128 || c >= 128 || d >= 128 {
            return Err(());
        }
        let va = TABLE[a as usize];
        let vb = TABLE[b as usize];
        let vc = if c == b'=' { 0 } else { TABLE[c as usize] };
        let vd = if d == b'=' { 0 } else { TABLE[d as usize] };
        if va == 0xff || vb == 0xff || vc == 0xff || vd == 0xff {
            return Err(());
        }
        out.push((va << 2) | (vb >> 4));
        if c != b'=' {
            out.push((vb << 4) | (vc >> 2));
        }
        if d != b'=' {
            out.push((vc << 6) | vd);
        }
        i += 4;
    }
    Ok(out)
}

// ─── Float Context ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct FloatItem {
    pub rect: Rect,
    pub side: FloatSide,
    pub clear: f32, // bottom of this float
    pub shape: Option<FloatShape>,
    pub shape_margin: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FloatSide {
    Left,
    Right,
}

impl Default for FloatSide {
    fn default() -> Self {
        Self::Left
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FloatShape {
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
    },
    Ellipse {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
    },
    Inset {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
    RoundedBox {
        rect: Rect,
        rx: [f32; 4],
        ry: [f32; 4],
    },
    Polygon(Vec<(f32, f32)>),
}

#[derive(Debug, Clone, Copy)]
pub struct FloatShapeReference {
    rect: Rect,
    rx: [f32; 4],
    ry: [f32; 4],
}

pub fn float_shape_reference(
    style: &ComputedStyle,
    layout: &LayoutBox,
    root_font_px: f32,
) -> Option<FloatShapeReference> {
    use crate::css::shape::ShapeBox;

    if style.shape_outside.trim().eq_ignore_ascii_case("none") {
        return None;
    }
    let tokens = crate::css::value_parse::split_css_values(&style.shape_outside);
    let box_only = tokens.len() == 1 && crate::css::shape::shape_box_kind(&tokens[0]).is_some();
    let shape_box = tokens
        .iter()
        .find_map(|token| crate::css::shape::shape_box_kind(token))
        .unwrap_or(ShapeBox::Margin);
    let reference = match shape_box {
        ShapeBox::Margin => layout.margin_rect,
        ShapeBox::Border => layout.border_rect,
        ShapeBox::HalfBorder => {
            let border = layout.border_rect;
            let padding = layout.padding_rect;
            let left = (border.x + padding.x) * 0.5;
            let top = (border.y + padding.y) * 0.5;
            let right = (border.x + border.w + padding.x + padding.w) * 0.5;
            let bottom = (border.y + border.h + padding.y + padding.h) * 0.5;
            Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
        }
        ShapeBox::Padding => layout.padding_rect,
        ShapeBox::Content => layout.content_rect,
    };
    let (rx, ry) = if box_only {
        crate::types::shape_box_radii(style, layout, reference, root_font_px)
    } else {
        ([0.0; 4], [0.0; 4])
    };
    Some(FloatShapeReference {
        rect: Rect::new(
            reference.x - layout.margin_rect.x,
            reference.y - layout.margin_rect.y,
            reference.w,
            reference.h,
        ),
        rx,
        ry,
    })
}

#[derive(Debug, Default, Clone)]
pub struct FloatContext {
    pub floats: Vec<FloatItem>,
    pub origin_x: f32, // Document X of the context root
    pub origin_y: f32, // Document Y of the context root
}

impl FloatContext {
    pub fn available_width(
        &self,
        y: f32,
        line_h: f32,
        containing_w: f32,
        out_left: &mut f32,
        out_right: &mut f32,
    ) {
        *out_left = 0.0;
        *out_right = containing_w;
        for f in &self.floats {
            if f.rect.y < y + line_h && f.clear > y {
                let (left, right) = f.exclusion_at(y, line_h);
                if left > right {
                    continue;
                }
                if f.side == FloatSide::Left {
                    if right > *out_left {
                        *out_left = right;
                    }
                } else {
                    if left < *out_right {
                        *out_right = left;
                    }
                }
            }
        }
    }

    pub fn available_width_in(
        &self,
        x: f32,
        y: f32,
        line_h: f32,
        containing_w: f32,
        out_left: &mut f32,
        out_right: &mut f32,
    ) {
        *out_left = 0.0;
        *out_right = containing_w;
        for f in &self.floats {
            if f.rect.y < y + line_h && f.clear > y {
                let (f_left, f_right) = f.exclusion_at(y, line_h);
                if f_left > f_right || f_right <= x || f_left >= x + containing_w {
                    continue;
                }
                if f.side == FloatSide::Left {
                    let r = (f_right - x).clamp(0.0, containing_w);
                    if r > *out_left {
                        *out_left = r;
                    }
                } else {
                    let l = (f_left - x).clamp(0.0, containing_w);
                    if l < *out_right {
                        *out_right = l;
                    }
                }
            }
        }
    }

    pub fn clear_y(&self, current_y: f32, clear: Clear) -> f32 {
        let mut y = current_y;
        for f in &self.floats {
            match clear {
                Clear::Left if f.side == FloatSide::Left => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                Clear::Right if f.side == FloatSide::Right => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                Clear::Both => {
                    if f.clear > y {
                        y = f.clear;
                    }
                }
                _ => {}
            }
        }
        y
    }

    pub fn next_clear_y(&self, y: f32) -> Option<f32> {
        let next_y = self
            .floats
            .iter()
            .filter(|f| f.clear > y + 0.001)
            .map(|f| f.clear)
            .fold(f32::MAX, f32::min);
        if next_y < f32::MAX {
            Some(next_y)
        } else {
            None
        }
    }

    pub fn place_float(
        &mut self,
        engine: &LayoutEngine,
        shape_font_px: f32,
        current_y: f32,
        float_w: f32,
        float_h: f32,
        containing_w: f32,
        side: FloatSide,
        shape_outside: &str,
        shape_margin: f32,
    ) -> Rect {
        self.place_float_in(
            engine,
            shape_font_px,
            0.0,
            current_y,
            float_w,
            float_h,
            containing_w,
            side,
            shape_outside,
            shape_margin,
            None,
        )
    }

    pub fn place_float_in(
        &mut self,
        engine: &LayoutEngine,
        shape_font_px: f32,
        x: f32,
        current_y: f32,
        float_w: f32,
        float_h: f32,
        containing_w: f32,
        side: FloatSide,
        shape_outside: &str,
        shape_margin: f32,
        reference: Option<FloatShapeReference>,
    ) -> Rect {
        // Find the lowest Y position where the float fits horizontally.
        let mut y = current_y;
        loop {
            let mut left = 0.0f32;
            let mut right = containing_w;
            self.available_width_in(x, y, float_h, containing_w, &mut left, &mut right);
            let available = right - left;
            // Percentage column floats often arrive with tiny IEEE-754 noise
            // around a 100% sum (66.6667% + 33.3333%). Browser engines keep
            // those floats on the same row; without a tolerance the last column
            // spuriously wraps below the row.
            if available + 0.5 >= float_w {
                break;
            }
            // Move past the nearest float bottom
            let next_y = self
                .floats
                .iter()
                .filter(|f| f.clear > y)
                .map(|f| f.clear)
                .fold(f32::MAX, f32::min);
            if next_y == f32::MAX {
                break;
            }
            y = next_y;
        }

        let mut left = 0.0f32;
        let mut right = containing_w;
        self.available_width_in(x, y, float_h, containing_w, &mut left, &mut right);

        let local_x = if side == FloatSide::Left {
            left
        } else {
            right - float_w
        };
        let rect = Rect::new(x + local_x, y, float_w, float_h);
        self.floats.push(FloatItem {
            rect,
            side,
            clear: y + float_h,
            shape: parse_float_shape(
                engine,
                shape_font_px,
                shape_outside,
                float_w,
                float_h,
                reference,
            ),
            shape_margin: shape_margin.max(0.0),
        });
        Rect::new(local_x, y, float_w, float_h)
    }
}

impl FloatItem {
    fn exclusion_at(&self, y: f32, line_h: f32) -> (f32, f32) {
        match self.shape.as_ref() {
            Some(shape) => shape_exclusion_x(shape, self.rect, self.shape_margin, y, line_h),
            None => (self.rect.x, self.rect.x + self.rect.w),
        }
    }
}

fn text_starts_with_collapsible_space(text: &str, style: &ComputedStyle) -> bool {
    matches!(style.white_space, WhiteSpace::Normal | WhiteSpace::Nowrap)
        && text
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_whitespace())
}

fn text_ends_with_collapsible_space(text: &str, style: &ComputedStyle) -> bool {
    matches!(style.white_space, WhiteSpace::Normal | WhiteSpace::Nowrap)
        && text
            .chars()
            .last()
            .is_some_and(|ch| ch.is_ascii_whitespace())
}

fn inline_subtree_starts_with_collapsible_space(node: &WebCore) -> bool {
    if matches!(node.style.display, Display::None)
        || matches!(node.style.position, Position::Absolute | Position::Fixed)
    {
        return false;
    }
    if !node.style.before_content.is_empty() {
        let style = node.style.before_style.as_deref().unwrap_or(&node.style);
        return text_starts_with_collapsible_space(&node.style.before_content, style);
    }
    if !node.text.is_empty() {
        return text_starts_with_collapsible_space(&node.text, &node.style);
    }
    for child in node.effective_children() {
        if inline_subtree_has_non_whitespace_text(child) {
            return inline_subtree_starts_with_collapsible_space(child);
        }
        if inline_subtree_starts_with_collapsible_space(child) {
            return true;
        }
    }
    if !node.style.after_content.is_empty() {
        let style = node.style.after_style.as_deref().unwrap_or(&node.style);
        return text_starts_with_collapsible_space(&node.style.after_content, style);
    }
    false
}

fn inline_subtree_ends_with_collapsible_space(node: &WebCore) -> bool {
    if matches!(node.style.display, Display::None)
        || matches!(node.style.position, Position::Absolute | Position::Fixed)
    {
        return false;
    }
    if !node.style.after_content.is_empty() {
        let style = node.style.after_style.as_deref().unwrap_or(&node.style);
        return text_ends_with_collapsible_space(&node.style.after_content, style);
    }
    for child in node.effective_children().into_iter().rev() {
        if inline_subtree_has_non_whitespace_text(child) {
            return inline_subtree_ends_with_collapsible_space(child);
        }
        if inline_subtree_ends_with_collapsible_space(child) {
            return true;
        }
    }
    if !node.text.is_empty() {
        return text_ends_with_collapsible_space(&node.text, &node.style);
    }
    if !node.style.before_content.is_empty() {
        let style = node.style.before_style.as_deref().unwrap_or(&node.style);
        return text_ends_with_collapsible_space(&node.style.before_content, style);
    }
    false
}

fn inline_subtree_has_non_whitespace_text(node: &WebCore) -> bool {
    if matches!(node.style.display, Display::None)
        || matches!(node.style.position, Position::Absolute | Position::Fixed)
    {
        return false;
    }
    if !node.style.before_content.trim().is_empty()
        || !node.text.trim().is_empty()
        || !node.style.after_content.trim().is_empty()
    {
        return true;
    }
    node.effective_children()
        .into_iter()
        .any(inline_subtree_has_non_whitespace_text)
}

fn collect_font_face_text_by_family(
    node: &WebCore,
    wanted: &HashSet<String>,
    out: &mut HashMap<String, String>,
    conservative: &mut HashSet<String>,
    family_lists: &mut HashMap<String, Vec<String>>,
) {
    fn matching_families<'a>(
        style: &ComputedStyle,
        wanted: &HashSet<String>,
        cache: &'a mut HashMap<String, Vec<String>>,
    ) -> &'a [String] {
        if !cache.contains_key(style.font_family.as_str()) {
            let matching = crate::css::value_parse::split_top_level_commas(&style.font_family)
                .into_iter()
                .filter_map(css_font_family_name)
                .map(|family| family.to_ascii_lowercase())
                .filter(|family| wanted.contains(family))
                .collect();
            cache.insert(style.font_family.clone(), matching);
        }
        cache
            .get(style.font_family.as_str())
            .expect("font-family cached")
    }

    fn append(
        text: &str,
        style: &ComputedStyle,
        wanted: &HashSet<String>,
        out: &mut HashMap<String, String>,
        family_lists: &mut HashMap<String, Vec<String>>,
    ) {
        if text.is_empty() || style.display == Display::None {
            return;
        }
        let transformed = (style.text_transform != crate::types::TextTransform::None).then(|| {
            crate::renderer::display_list_builder::apply_text_transform(text, style.text_transform)
        });
        for family in matching_families(style, wanted, family_lists) {
            let entry = out.entry(family.clone()).or_default();
            entry.push_str(text);
            if let Some(transformed) = &transformed {
                entry.push_str(transformed);
            }
        }
    }

    if node.style.display == Display::None {
        return;
    }
    for pseudo in [
        &node.style.first_line_style,
        &node.style.first_letter_style,
        &node.style.file_selector_button_style,
        &node.style.details_content_style,
    ] {
        if let Some(style) = pseudo.as_deref() {
            for family in matching_families(style, wanted, family_lists) {
                conservative.insert(family.clone());
            }
        }
    }
    append(
        &node.style.before_content,
        node.style.before_style.as_deref().unwrap_or(&node.style),
        wanted,
        out,
        family_lists,
    );
    if node.tag != "#comment" {
        append(&node.text, &node.style, wanted, out, family_lists);
    }
    append(
        &node.style.after_content,
        node.style.after_style.as_deref().unwrap_or(&node.style),
        wanted,
        out,
        family_lists,
    );
    append(
        &node.style.marker_content,
        node.style.marker_style.as_deref().unwrap_or(&node.style),
        wanted,
        out,
        family_lists,
    );
    if node.tag == "li"
        && node.style.marker_content.is_empty()
        && node.style.list_style_image.is_empty()
        && !matches!(
            node.style.list_style_type,
            crate::types::ListStyleType::None
                | crate::types::ListStyleType::Disc
                | crate::types::ListStyleType::Circle
                | crate::types::ListStyleType::Square
        )
    {
        let marker = node.style.marker_style.as_deref().unwrap_or(&node.style);
        for family in matching_families(marker, wanted, family_lists) {
            conservative.insert(family.clone());
        }
    }
    if matches!(node.tag.as_str(), "input" | "textarea") {
        if let Some(value) = node
            .value_state
            .as_ref()
            .or_else(|| node.attributes.get("value"))
        {
            append(value, &node.style, wanted, out, family_lists);
        }
        if let Some(placeholder) = node.attributes.get("placeholder") {
            append(
                placeholder,
                node.style
                    .placeholder_style
                    .as_deref()
                    .unwrap_or(&node.style),
                wanted,
                out,
                family_lists,
            );
        }
    }
    if let Some(shadow) = node.shadow_root.as_ref() {
        for child in &shadow.children {
            collect_font_face_text_by_family(child, wanted, out, conservative, family_lists);
        }
    }
    for child in &node.children {
        collect_font_face_text_by_family(child, wanted, out, conservative, family_lists);
    }
}

fn referenced_font_face_families(root: &WebCore) -> HashSet<String> {
    fn collect(node: &WebCore, lists: &mut HashSet<String>) {
        if node.style.display == Display::None {
            return;
        }
        let mut add_style = |style: &ComputedStyle| {
            if style.display != Display::None && !lists.contains(style.font_family.as_str()) {
                lists.insert(style.font_family.clone());
            }
        };
        add_style(&node.style);
        for pseudo in [
            &node.style.before_style,
            &node.style.after_style,
            &node.style.marker_style,
            &node.style.placeholder_style,
            &node.style.file_selector_button_style,
            &node.style.details_content_style,
            &node.style.first_line_style,
            &node.style.first_letter_style,
        ] {
            if let Some(style) = pseudo.as_deref() {
                add_style(style);
            }
        }
        if let Some(shadow) = node.shadow_root.as_ref() {
            for child in &shadow.children {
                collect(child, lists);
            }
        }
        for child in &node.children {
            collect(child, lists);
        }
    }

    let mut lists = HashSet::new();
    collect(root, &mut lists);
    lists
        .iter()
        .flat_map(|list| crate::css::value_parse::split_top_level_commas(list))
        .filter_map(css_font_family_name)
        .map(|family| family.to_ascii_lowercase())
        .collect()
}

fn inline_items_max_content_advance(items: &[inline_layout::InlineItem]) -> f32 {
    let mut max_width = 0.0f32;
    let mut line_width = 0.0f32;
    let mut trailing_collapsible_space = 0.0f32;
    let mut line_has_content = false;

    for item in items {
        if matches!(item.kind, inline_layout::InlineItemKind::Break) {
            max_width = max_width.max((line_width - trailing_collapsible_space).max(0.0));
            line_width = 0.0;
            trailing_collapsible_space = 0.0;
            line_has_content = false;
            continue;
        }
        if item.is_space {
            if line_has_content {
                line_width += item.advance;
                trailing_collapsible_space += item.advance;
            }
            continue;
        }
        line_width += item.advance;
        trailing_collapsible_space = 0.0;
        line_has_content = true;
    }

    max_width.max((line_width - trailing_collapsible_space).max(0.0))
}

fn inline_items_min_content_advance(items: &[inline_layout::InlineItem]) -> f32 {
    let mut max_width = 0.0f32;
    let mut segment_width = 0.0f32;

    for item in items {
        if matches!(item.kind, inline_layout::InlineItemKind::Break) {
            max_width = max_width.max(segment_width);
            segment_width = 0.0;
            continue;
        }

        if item.is_space && item.breakable {
            max_width = max_width.max(segment_width);
            segment_width = 0.0;
            continue;
        }

        if (item.breakable || item.emergency_break == inline_layout::EmergencyBreak::MinContent)
            && segment_width > 0.0
        {
            max_width = max_width.max(segment_width);
            segment_width = 0.0;
        }

        segment_width += item.advance;

        if item.break_after
            || (item.breakable && matches!(item.kind, inline_layout::InlineItemKind::Atomic { .. }))
        {
            max_width = max_width.max(segment_width);
            segment_width = 0.0;
        }
    }

    max_width.max(segment_width)
}

fn shape_exclusion_x(
    shape: &FloatShape,
    rect: Rect,
    margin: f32,
    y: f32,
    line_h: f32,
) -> (f32, f32) {
    let line_top = (y - rect.y).max(0.0);
    let line_bottom = (y + line_h - rect.y).min(rect.h);
    let empty = (rect.x + rect.w, rect.x);
    if line_bottom <= line_top {
        return empty;
    }
    match *shape {
        FloatShape::Circle { cx, cy, r } => {
            ellipse_exclusion_x(rect, cx, cy, r, r, margin, cy.clamp(line_top, line_bottom))
        }
        FloatShape::Ellipse { cx, cy, rx, ry } => ellipse_exclusion_x(
            rect,
            cx,
            cy,
            rx,
            ry,
            margin,
            cy.clamp(line_top, line_bottom),
        ),
        FloatShape::Inset {
            top,
            right,
            bottom,
            left,
        } => {
            let shape_top = top - margin;
            let shape_bottom = rect.h - bottom + margin;
            if line_bottom <= shape_top || line_top >= shape_bottom {
                empty
            } else {
                (
                    rect.x + (left - margin).max(0.0),
                    rect.x + rect.w - (right - margin).max(0.0),
                )
            }
        }
        FloatShape::RoundedBox {
            rect: box_rect,
            rx,
            ry,
        } => rounded_box_exclusion_x(rect, box_rect, rx, ry, margin, line_top, line_bottom),
        FloatShape::Polygon(ref points) => {
            polygon_exclusion_x(rect, margin, points, line_top, line_bottom)
        }
    }
}

fn rounded_box_exclusion_x(
    float_rect: Rect,
    box_rect: Rect,
    rx: [f32; 4],
    ry: [f32; 4],
    margin: f32,
    line_top: f32,
    line_bottom: f32,
) -> (f32, f32) {
    let shape = Rect::new(
        box_rect.x - margin,
        box_rect.y - margin,
        box_rect.w + 2.0 * margin,
        box_rect.h + 2.0 * margin,
    );
    let top = line_top.max(shape.y);
    let bottom = line_bottom.min(shape.y + shape.h);
    if top >= bottom || shape.w <= 0.0 {
        return (float_rect.x + float_rect.w, float_rect.x);
    }
    let rx = rx.map(|radius| radius + margin);
    let ry = ry.map(|radius| radius + margin);
    let (rx, ry) = crate::types::reduce_shape_box_radii(shape.w, shape.h, rx, ry);
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for y in [
        top,
        bottom,
        shape.y + ry[0],
        shape.y + ry[1],
        shape.y + shape.h - ry[2],
        shape.y + shape.h - ry[3],
    ] {
        if y < top || y > bottom {
            continue;
        }
        let edge_inset =
            |top_radius: f32, top_height: f32, bottom_radius: f32, bottom_height: f32| {
                if top_height > 0.0 && y < shape.y + top_height {
                    let unit = ((shape.y + top_height - y) / top_height).clamp(0.0, 1.0);
                    top_radius * (1.0 - (1.0 - unit * unit).sqrt())
                } else if bottom_height > 0.0 && y > shape.y + shape.h - bottom_height {
                    let unit =
                        ((y - (shape.y + shape.h - bottom_height)) / bottom_height).clamp(0.0, 1.0);
                    bottom_radius * (1.0 - (1.0 - unit * unit).sqrt())
                } else {
                    0.0
                }
            };
        left = left.min(shape.x + edge_inset(rx[0], ry[0], rx[3], ry[3]));
        right = right.max(shape.x + shape.w - edge_inset(rx[1], ry[1], rx[2], ry[2]));
    }
    (
        float_rect.x + left.clamp(0.0, float_rect.w),
        float_rect.x + right.clamp(0.0, float_rect.w),
    )
}

fn ellipse_exclusion_x(
    rect: Rect,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    margin: f32,
    sample_y: f32,
) -> (f32, f32) {
    let rx = (rx + margin).max(0.0);
    let ry = (ry + margin).max(0.0);
    if rx <= 0.0 || ry <= 0.0 {
        return (rect.x, rect.x);
    }
    let dy = (sample_y - cy).abs();
    if dy >= ry {
        return (rect.x + rect.w, rect.x);
    }
    let half = rx * (1.0 - (dy / ry).powi(2)).sqrt();
    (rect.x + cx - half, rect.x + cx + half)
}

fn parse_float_shape(
    engine: &LayoutEngine,
    font_px: f32,
    value: &str,
    width: f32,
    height: f32,
    reference: Option<FloatShapeReference>,
) -> Option<FloatShape> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") || value.is_empty() {
        return None;
    }
    let tokens = crate::css::value_parse::split_css_values(value);
    let shape = tokens
        .iter()
        .find(|token| crate::css::shape::shape_box_kind(token).is_none())
        .map(String::as_str);
    let reference = reference.unwrap_or(FloatShapeReference {
        rect: Rect::new(0.0, 0.0, width, height),
        rx: [0.0; 4],
        ry: [0.0; 4],
    });
    let Some(value) = shape else {
        return Some(FloatShape::RoundedBox {
            rect: reference.rect,
            rx: reference.rx,
            ry: reference.ry,
        });
    };
    let box_rect = reference.rect;
    let lower = value.to_ascii_lowercase();
    let parsed = if lower.starts_with("circle(") && value.ends_with(')') {
        parse_circle_shape(
            engine,
            font_px,
            &value["circle(".len()..value.len() - 1],
            box_rect.w,
            box_rect.h,
        )
    } else if lower.starts_with("ellipse(") && value.ends_with(')') {
        parse_ellipse_shape(
            engine,
            font_px,
            &value["ellipse(".len()..value.len() - 1],
            box_rect.w,
            box_rect.h,
        )
    } else if lower.starts_with("inset(") && value.ends_with(')') {
        parse_inset_shape(
            engine,
            font_px,
            &value["inset(".len()..value.len() - 1],
            box_rect.w,
            box_rect.h,
        )
    } else if lower.starts_with("polygon(") && value.ends_with(')') {
        parse_polygon_shape(
            engine,
            font_px,
            &value["polygon(".len()..value.len() - 1],
            box_rect.w,
            box_rect.h,
        )
    } else {
        None
    }?;
    Some(match parsed {
        FloatShape::Circle { cx, cy, r } => FloatShape::Circle {
            cx: cx + box_rect.x,
            cy: cy + box_rect.y,
            r,
        },
        FloatShape::Ellipse { cx, cy, rx, ry } => FloatShape::Ellipse {
            cx: cx + box_rect.x,
            cy: cy + box_rect.y,
            rx,
            ry,
        },
        FloatShape::Inset {
            top,
            right,
            bottom,
            left,
        } => FloatShape::Inset {
            top: top + box_rect.y,
            right: right + width - box_rect.x - box_rect.w,
            bottom: bottom + height - box_rect.y - box_rect.h,
            left: left + box_rect.x,
        },
        FloatShape::Polygon(points) => FloatShape::Polygon(
            points
                .into_iter()
                .map(|(x, y)| (x + box_rect.x, y + box_rect.y))
                .collect(),
        ),
        FloatShape::RoundedBox { rect, rx, ry } => FloatShape::RoundedBox {
            rect: Rect::new(rect.x + box_rect.x, rect.y + box_rect.y, rect.w, rect.h),
            rx,
            ry,
        },
    })
}

fn parse_circle_shape(
    engine: &LayoutEngine,
    font_px: f32,
    inner: &str,
    width: f32,
    height: f32,
) -> Option<FloatShape> {
    let (radius_part, position_part) = crate::css::shape::split_shape_at(inner);
    let (cx, cy) = parse_shape_position(engine, font_px, position_part, width, height)?;
    let nearest_x = cx.abs().min((width - cx).abs());
    let nearest_y = cy.abs().min((height - cy).abs());
    let farthest_x = cx.abs().max((width - cx).abs());
    let farthest_y = cy.abs().max((height - cy).abs());
    let nearest_side = nearest_x.min(nearest_y);
    let farthest_side = farthest_x.max(farthest_y);
    let nearest_corner = nearest_x.hypot(nearest_y);
    let farthest_corner = farthest_x.hypot(farthest_y);
    let radius = radius_part.map(|value| value.trim().to_ascii_lowercase());
    let r = match radius.as_deref() {
        None | Some("closest-side") => nearest_side,
        Some("farthest-side") => farthest_side,
        Some("closest-corner") => nearest_corner,
        Some("farthest-corner") => farthest_corner,
        Some(value) => resolve_shape_len(
            engine,
            font_px,
            value,
            width.hypot(height) / std::f32::consts::SQRT_2,
        )?,
    };
    (r >= 0.0).then_some(FloatShape::Circle { cx, cy, r })
}

fn parse_ellipse_shape(
    engine: &LayoutEngine,
    font_px: f32,
    inner: &str,
    width: f32,
    height: f32,
) -> Option<FloatShape> {
    let (radii_part, position_part) = crate::css::shape::split_shape_at(inner);
    let (cx, cy) = parse_shape_position(engine, font_px, position_part, width, height)?;
    let nearest_x = cx.abs().min((width - cx).abs());
    let nearest_y = cy.abs().min((height - cy).abs());
    let farthest_x = cx.abs().max((width - cx).abs());
    let farthest_y = cy.abs().max((height - cy).abs());
    let radii = crate::css::split_css_shorthand_values(radii_part.unwrap_or(""));
    if radii.len() > 2 {
        return None;
    }
    let lower_radii: Vec<String> = radii
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect();
    let first = lower_radii.first().map(String::as_str);
    let second = lower_radii.get(1).map(String::as_str);
    let (rx, ry) = match (first, second) {
        (None | Some("closest-side"), None) => (nearest_x, nearest_y),
        (Some("farthest-side"), None) => (farthest_x, farthest_y),
        (Some("closest-corner"), None) | (Some("farthest-corner"), None) => {
            let farthest = first == Some("farthest-corner");
            let base_rx = if farthest { farthest_x } else { nearest_x };
            let base_ry = if farthest { farthest_y } else { nearest_y };
            (
                base_rx * std::f32::consts::SQRT_2,
                base_ry * std::f32::consts::SQRT_2,
            )
        }
        (Some(x), Some(y)) => (
            resolve_shape_len(engine, font_px, x, width)?,
            resolve_shape_len(engine, font_px, y, height)?,
        ),
        _ => return None,
    };
    (rx >= 0.0 && ry >= 0.0).then_some(FloatShape::Ellipse { cx, cy, rx, ry })
}

fn parse_inset_shape(
    engine: &LayoutEngine,
    font_px: f32,
    inner: &str,
    width: f32,
    height: f32,
) -> Option<FloatShape> {
    let tokens = crate::css::split_css_shorthand_values(inner);
    let inset_count = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("round"))
        .unwrap_or(tokens.len());
    if inset_count == 0 || inset_count > 4 {
        return None;
    }
    let inset_tokens = &tokens[..inset_count];
    let top_token = inset_tokens[0].as_str();
    let right_token = inset_tokens.get(1).map(String::as_str).unwrap_or(top_token);
    let bottom_token = inset_tokens.get(2).map(String::as_str).unwrap_or(top_token);
    let left_token = inset_tokens
        .get(3)
        .map(String::as_str)
        .unwrap_or(right_token);
    let top = resolve_shape_len(engine, font_px, top_token, height)?;
    let right = resolve_shape_len(engine, font_px, right_token, width)?;
    let bottom = resolve_shape_len(engine, font_px, bottom_token, height)?;
    let left = resolve_shape_len(engine, font_px, left_token, width)?;
    if inset_count == tokens.len() {
        return Some(FloatShape::Inset {
            top,
            right,
            bottom,
            left,
        });
    }
    let radii = tokens.get(inset_count + 1..)?.join(" ");
    let (horizontal, vertical) = crate::css::property_defs::parse_shape_round_radii(&radii)?;
    let rect = Rect::new(
        left,
        top,
        (width - left - right).max(0.0),
        (height - top - bottom).max(0.0),
    );
    let rx = horizontal.map(|radius| engine.res_len(&radius, font_px, rect.w, engine.root_font_px));
    let ry = vertical.map(|radius| engine.res_len(&radius, font_px, rect.h, engine.root_font_px));
    let (rx, ry) = crate::types::reduce_shape_box_radii(rect.w, rect.h, rx, ry);
    Some(FloatShape::RoundedBox { rect, rx, ry })
}

fn parse_polygon_shape(
    engine: &LayoutEngine,
    font_px: f32,
    inner: &str,
    width: f32,
    height: f32,
) -> Option<FloatShape> {
    let mut points = Vec::new();
    for (index, raw) in crate::css::value_parse::split_top_level_commas(inner)
        .into_iter()
        .enumerate()
    {
        let part = raw.trim();
        if part.is_empty() {
            return None;
        }
        let coords = crate::css::split_css_shorthand_values(part);
        let start = if index == 0
            && coords.first().is_some_and(|v| {
                v.eq_ignore_ascii_case("evenodd") || v.eq_ignore_ascii_case("nonzero")
            }) {
            1
        } else {
            0
        };
        if index == 0 && coords.len() == 1 && start == 1 {
            continue;
        }
        if coords.len() != start + 2 {
            return None;
        }
        let x = resolve_shape_len(engine, font_px, &coords[start], width)?;
        let y = resolve_shape_len(engine, font_px, &coords[start + 1], height)?;
        points.push((x, y));
    }
    if points.len() >= 3 {
        Some(FloatShape::Polygon(points))
    } else {
        None
    }
}

fn polygon_exclusion_x(
    rect: Rect,
    margin: f32,
    points: &[(f32, f32)],
    line_top: f32,
    line_bottom: f32,
) -> (f32, f32) {
    if margin > 0.0 {
        return polygon_margin_exclusion_x(rect, margin, points, line_top, line_bottom);
    }
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        let top = line_top.max(y1.min(y2));
        let bottom = line_bottom.min(y1.max(y2));
        if top > bottom {
            continue;
        }
        let (edge_left, edge_right) = if y1 == y2 {
            (x1.min(x2), x1.max(x2))
        } else {
            let at_top = x1 + (x2 - x1) * (top - y1) / (y2 - y1);
            let at_bottom = x1 + (x2 - x1) * (bottom - y1) / (y2 - y1);
            (at_top.min(at_bottom), at_top.max(at_bottom))
        };
        left = left.min(edge_left);
        right = right.max(edge_right);
    }
    if !left.is_finite() {
        return (rect.x + rect.w, rect.x);
    }
    (
        rect.x + (left - margin).clamp(0.0, rect.w),
        rect.x + (right + margin).clamp(0.0, rect.w),
    )
}

fn polygon_margin_exclusion_x(
    rect: Rect,
    margin: f32,
    points: &[(f32, f32)],
    line_top: f32,
    line_bottom: f32,
) -> (f32, f32) {
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    let margin_squared = margin * margin;
    for i in 0..points.len() {
        let (x1, y1) = points[i];
        let (x2, y2) = points[(i + 1) % points.len()];
        if y1.min(y2) > line_bottom + margin || y1.max(y2) < line_top - margin {
            continue;
        }
        let dx = x2 - x1;
        let dy = y2 - y1;
        let mut consider = |t: f32| {
            if !(0.0..=1.0).contains(&t) {
                return;
            }
            let x = x1 + dx * t;
            let y = y1 + dy * t;
            let distance = if y < line_top {
                line_top - y
            } else {
                (y - line_bottom).max(0.0)
            };
            if distance > margin {
                return;
            }
            let reach = (margin_squared - distance * distance).max(0.0).sqrt();
            left = left.min(x - reach);
            right = right.max(x + reach);
        };
        consider(0.0);
        consider(1.0);
        if dy != 0.0 {
            for y in [
                line_top - margin,
                line_top,
                line_bottom,
                line_bottom + margin,
            ] {
                consider((y - y1) / dy);
            }
            let length = dx.hypot(dy);
            if length > 0.0 {
                let offset = margin * dx.abs() / length;
                for y in [
                    line_top - offset,
                    line_top + offset,
                    line_bottom - offset,
                    line_bottom + offset,
                ] {
                    consider((y - y1) / dy);
                }
            }
        }
    }
    if !left.is_finite() {
        return (rect.x + rect.w, rect.x);
    }
    (
        rect.x + left.clamp(0.0, rect.w),
        rect.x + right.clamp(0.0, rect.w),
    )
}

fn parse_shape_position(
    engine: &LayoutEngine,
    font_px: f32,
    position: Option<&str>,
    width: f32,
    height: f32,
) -> Option<(f32, f32)> {
    let Some(position) = position else {
        return Some((width * 0.5, height * 0.5));
    };
    let (x, y) = crate::css::property_defs::parse_background_position_pair(position)?;
    Some((
        engine.res_len(&x, font_px, width, engine.root_font_px),
        engine.res_len(&y, font_px, height, engine.root_font_px),
    ))
}

fn resolve_shape_len(engine: &LayoutEngine, font_px: f32, token: &str, basis: f32) -> Option<f32> {
    let token = token.trim();
    if token.parse::<f32>().is_ok_and(|value| value != 0.0) {
        return None;
    }
    let length = crate::css::parse_length_checked(token)?;
    if length.is_auto() || length.is_none() || length.intrinsic().is_some() {
        return None;
    }
    Some(engine.res_len(&length, font_px, basis, engine.root_font_px))
}

/// Collect node_ids of elements that have hover-dependent styles.
fn collect_hover_sensitive(node: &WebCore, out: &mut std::collections::HashSet<u32>) {
    if node.style.hover_style.is_some() {
        out.insert(node.node_id);
    }
    for child in &node.children {
        collect_hover_sensitive(child, out);
    }
}

/// Walk the tree bottom-up: if any child is `layout_dirty`, mark the parent
/// dirty too.  Returns `true` if the node (or any descendant) is dirty.
fn propagate_dirty(node: &mut WebCore) -> bool {
    let mut child_dirty = false;
    for child in &mut node.children {
        if propagate_dirty(child) {
            child_dirty = true;
        }
    }

    if node.layout.layout_dirty
        || child_dirty
        || node.has_dirty_descendant
        || node.has_dirty_layout_descendant
    {
        // Invalidate intrinsic width cache — a dirty descendant means our
        // intrinsic size may have changed (needed by flex/grid/table parents).
        node.layout.cached_intrinsic_w.set(f32::NAN);
        node.layout.intrinsic_dirty = true;
    }

    if child_dirty {
        node.layout.layout_dirty = true;
        node.has_dirty_layout_descendant = true;
    }
    node.layout.layout_dirty
}

// ─── Resolved box model ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolvedBox {
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,

    pub padding_top: f32,
    pub padding_right: f32,
    pub padding_bottom: f32,
    pub padding_left: f32,

    pub border_top: f32,
    pub border_right: f32,
    pub border_bottom: f32,
    pub border_left: f32,

    pub content_width: Option<f32>, // None = auto
    pub content_height: Option<f32>,
}

impl ResolvedBox {
    pub fn h_space(&self) -> f32 {
        self.margin_left
            + self.border_left
            + self.padding_left
            + self.padding_right
            + self.border_right
            + self.margin_right
    }
    pub fn v_space(&self) -> f32 {
        self.margin_top
            + self.border_top
            + self.padding_top
            + self.padding_bottom
            + self.border_bottom
            + self.margin_bottom
    }
    pub fn inner_h_space(&self) -> f32 {
        self.border_left + self.padding_left + self.padding_right + self.border_right
    }
    pub fn inner_v_space(&self) -> f32 {
        self.border_top + self.padding_top + self.padding_bottom + self.border_bottom
    }
}

pub fn resolve_box(
    style: &ComputedStyle,
    parent_font_px: f32,
    containing_w: f32,
    root_font_px: f32,
) -> ResolvedBox {
    resolve_box_vp(
        style,
        parent_font_px,
        containing_w,
        root_font_px,
        0.0,
        0.0,
        None,
    )
}

pub fn resolve_box_vp(
    style: &ComputedStyle,
    parent_font_px: f32,
    containing_w: f32,
    root_font_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    containing_h: Option<f32>,
) -> ResolvedBox {
    resolve_box_vp_query(
        style,
        parent_font_px,
        containing_w,
        root_font_px,
        viewport_w,
        viewport_h,
        containing_h,
        QueryContainerSizes::default(),
    )
}

fn resolve_box_vp_query(
    style: &ComputedStyle,
    parent_font_px: f32,
    containing_w: f32,
    root_font_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    containing_h: Option<f32>,
    query: QueryContainerSizes,
) -> ResolvedBox {
    let res = |l: &CssLength| {
        l.resolve_query_vp(
            parent_font_px,
            containing_w,
            root_font_px,
            viewport_w,
            viewport_h,
            query,
        )
    };
    let _font_px = style.font_size_px(parent_font_px, root_font_px);

    let pad_left = res(&style.padding_left).max(0.0);
    let pad_right = res(&style.padding_right).max(0.0);
    let pad_top = res(&style.padding_top).max(0.0);
    let pad_bottom = res(&style.padding_bottom).max(0.0);

    let border_left = if style.border_left_style != BorderStyle::None {
        res(&style.border_left_width)
    } else {
        0.0
    };
    let border_right = if style.border_right_style != BorderStyle::None {
        res(&style.border_right_width)
    } else {
        0.0
    };
    let border_top = if style.border_top_style != BorderStyle::None {
        res(&style.border_top_width)
    } else {
        0.0
    };
    let border_bottom = if style.border_bottom_style != BorderStyle::None {
        res(&style.border_bottom_width)
    } else {
        0.0
    };

    // ⛔ `width` and `height` DO NOT APPLY to a non-replaced inline box —
    // CSS 2.1 §10.2 and §10.5. An `<span style="width:100px;height:50px">`
    // is sized by its text, and this sized it 100x50; Chrome answers 8x18 for
    // the same markup.
    //
    // `inline-block`, `inline-flex` and the replaced elements are all
    // inline-LEVEL but do take a width, so the test is `display: inline`
    // exactly, not "is inline-level".
    let inline_ignores_size = style.display == Display::Inline;
    let content_width = if style.width.is_auto() || inline_ignores_size {
        None
    } else {
        let mut w = if style.width == CssLength::Stretch {
            containing_w
                - res(&style.margin_left)
                - res(&style.margin_right)
                - pad_left
                - pad_right
                - border_left
                - border_right
        } else {
            res(&style.width)
        }
        .max(0.0);
        // box-sizing: border-box — subtract padding + border from declared width
        if style.box_sizing == BoxSizing::BorderBox && style.width != CssLength::Stretch {
            w = (w - pad_left - pad_right - border_left - border_right).max(0.0);
        }
        Some(w)
    };

    // CSS 2.1 §10.5: percentage heights resolve against the containing block's height.
    // If the containing block's height is not explicitly set (containing_h is None),
    // percentage heights are treated as auto.
    let content_height = if style.height.is_auto() || inline_ignores_size {
        None
    } else if style.height.has_percentage() {
        match containing_h {
            Some(ch) => {
                let mut h = style
                    .height
                    .resolve_query_vp(
                        parent_font_px,
                        ch,
                        root_font_px,
                        viewport_w,
                        viewport_h,
                        query,
                    )
                    .max(0.0);
                if style.box_sizing == BoxSizing::BorderBox {
                    h = (h - pad_top - pad_bottom - border_top - border_bottom).max(0.0);
                }
                Some(h)
            }
            None => None, // percentage height with no explicit containing height → auto
        }
    } else {
        let mut h = res(&style.height).max(0.0);
        if style.box_sizing == BoxSizing::BorderBox {
            h = (h - pad_top - pad_bottom - border_top - border_bottom).max(0.0);
        }
        Some(h)
    };

    ResolvedBox {
        margin_top: res(&style.margin_top),
        margin_right: res(&style.margin_right),
        margin_bottom: res(&style.margin_bottom),
        margin_left: res(&style.margin_left),

        padding_top: pad_top,
        padding_right: pad_right,
        padding_bottom: pad_bottom,
        padding_left: pad_left,

        border_top,
        border_right,
        border_bottom,
        border_left,

        content_width,
        content_height,
    }
}

// ─── Layout Engine ────────────────────────────────────────────────────────────

fn same_containing_height(previous: Option<f32>, current: Option<f32>) -> bool {
    match (previous, current) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() < 0.5,
        _ => false,
    }
}

pub struct LayoutEngine {
    pub root_font_px: f32,
    /// Logical viewport width (for vw units).
    pub viewport_w: f32,
    /// Logical viewport height (for vh units).
    pub viewport_h: f32,
    query_container_sizes: Cell<QueryContainerSizes>,
    /// Reference to a font system for accurate measurement.
    pub font_system: Option<*mut cosmic_text::FontSystem>,
    /// Custom component registry for custom tags
    pub component_registry: ComponentRegistry,
    /// Device pixel ratio (e.g. 2.0 on HiDPI/Retina). Used so that char_x
    /// positions are shaped at physical pixel size — matching the renderer —
    /// giving accurate click↔caret mapping on every display density.
    pub scale: f32,
    /// Viewport width used in the last cascade pass, for skip-cascade optimization.
    last_cascade_vw: f32,
    /// Viewport height used in the last geometry pass, to detect vh-unit changes.
    last_geometry_viewport_h: f32,
    /// Whether any @media rules exist — cached to avoid O(n) scan every layout.
    cached_has_media_q: bool,
    /// Whether any @container rules exist — cached to avoid O(n) scan every layout.
    cached_has_container_q: bool,
    /// Y cutoff for progressive layout — nodes below this are deferred.
    /// 0 = no cutoff (full layout). Set to viewport_h * 1.5 for first-screen priority.
    pub progressive_cutoff: f32,
    /// True after the first layout pass has completed. Prevents progressive
    /// layout from running on every subsequent layout (hover, image load, etc.).
    initial_layout_done: bool,
    /// Font-face sources already scheduled/loaded. Stylesheets can arrive
    /// progressively, so this cannot be a single document-wide latch.
    scheduled_font_faces: HashSet<String>,
    cached_font_families: Option<HashSet<String>>,
    scheduled_remote_font_bindings: HashSet<(String, crate::css::FontFaceDecl)>,
    loaded_remote_font_ids: HashMap<String, Vec<fontdb::ID>>,
    /// Remote URLs that failed fetch, decode, or font registration. Their
    /// authored successor is tried on the next layout pass.
    failed_font_sources: HashSet<String>,
    /// Receivers for async font data arriving from background threads.
    pending_fonts: Vec<std::sync::mpsc::Receiver<PendingFontResult>>,
    /// Number of font fetches still in flight.
    fonts_in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Shared browser resource cache for remote font bytes.
    pub resource_cache_dir: Option<String>,
    /// Containing block rect for the nearest positioned (non-static) ancestor.
    /// Used by abs-pos children to resolve their containing block correctly.
    pub pos_cb: Cell<Rect>,
    /// Containing block rect for fixed-position descendants. Unlike abs-pos,
    /// ordinary positioned ancestors do not capture fixed descendants.
    pub fixed_cb: Cell<Rect>,
    /// Current recursion depth — prevents stack overflow on deeply nested DOMs.
    layout_depth: Cell<usize>,
    /// Total layout_box calls — detect infinite loops.
    layout_calls: Cell<usize>,
    /// Layout start time — detect long-running layout.
    layout_start: Cell<Option<std::time::Instant>>,
    /// Text measurement cache: (text_hash, font_size_bits, weight, family_hash) → width.
    /// Avoids redundant cosmic_text font shaping on re-layout.
    text_width_cache: std::cell::RefCell<HashMap<u64, f32>>,
}

pub(crate) struct QueryContainerScope<'a> {
    cell: &'a Cell<QueryContainerSizes>,
    previous: QueryContainerSizes,
}

impl Drop for QueryContainerScope<'_> {
    fn drop(&mut self) {
        self.cell.set(self.previous);
    }
}

/// Maximum layout recursion depth to prevent stack overflow.
const MAX_LAYOUT_DEPTH: usize = 400;
const MAX_TEXT_WIDTH_CACHE_ENTRIES: usize = 65_536;

impl LayoutEngine {
    fn contained_intrinsic_width(
        &self,
        style: &ComputedStyle,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if style.contain_intrinsic_width.is_none() || style.contain_intrinsic_width.is_auto() {
            0.0
        } else {
            self.res_len(&style.contain_intrinsic_width, font_px, 0.0, root_font_px)
                .max(0.0)
        }
    }

    fn contained_intrinsic_width_for_node(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.style.rare().contain_intrinsic_width_auto {
            if let Some(size) = &node.layout.last_uncontained_content_size {
                return size.0;
            }
        }
        self.contained_intrinsic_width(&node.style, font_px, root_font_px)
    }

    fn has_inline_size_containment(style: &ComputedStyle) -> bool {
        style.contain_size
            || style.container_type == ContainerType::Size
            || (style.writing_mode == WritingMode::HorizontalTB
                && (style.contain_inline_size || style.container_type == ContainerType::InlineSize))
    }

    pub(crate) fn contained_intrinsic_height(
        &self,
        style: &ComputedStyle,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if style.contain_intrinsic_height.is_none() || style.contain_intrinsic_height.is_auto() {
            0.0
        } else {
            self.res_len(&style.contain_intrinsic_height, font_px, 0.0, root_font_px)
                .max(0.0)
        }
    }
    pub(crate) fn contained_intrinsic_height_for_node(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.style.rare().contain_intrinsic_height_auto {
            if let Some(size) = &node.layout.last_uncontained_content_size {
                return size.1;
            }
        }
        self.contained_intrinsic_height(&node.style, font_px, root_font_px)
    }

    fn enter_query_writing_mode(&self, style: &ComputedStyle) -> QueryContainerScope<'_> {
        let previous = self.query_container_sizes.get();
        let mut current = previous;
        current.fallback_vertical = style.writing_mode != WritingMode::HorizontalTB;
        self.query_container_sizes.set(current);
        QueryContainerScope {
            cell: &self.query_container_sizes,
            previous,
        }
    }

    pub(crate) fn enter_query_container(
        &self,
        node: &WebCore,
        width: f32,
        height: Option<f32>,
        font_px: f32,
        root_font_px: f32,
    ) -> QueryContainerScope<'_> {
        let previous = self.query_container_sizes.get();
        let mut current = previous;
        let vertical = node.style.writing_mode != WritingMode::HorizontalTB;
        let height = height.unwrap_or_else(|| {
            self.contained_intrinsic_height_for_node(node, font_px, root_font_px)
        });
        match node.style.container_type {
            ContainerType::Normal => {}
            ContainerType::InlineSize => {
                if vertical {
                    current.height = Some(height);
                    current.inline = Some(height);
                } else {
                    current.width = Some(width);
                    current.inline = Some(width);
                }
            }
            ContainerType::Size => {
                current.width = Some(width);
                current.height = Some(height);
                current.inline = Some(if vertical { height } else { width });
                current.block = Some(if vertical { width } else { height });
            }
        }
        self.query_container_sizes.set(current);
        QueryContainerScope {
            cell: &self.query_container_sizes,
            previous,
        }
    }

    pub fn new() -> Self {
        Self {
            root_font_px: crate::types::ComputedStyle::INITIAL_FONT_SIZE_PX,
            viewport_w: 900.0,
            viewport_h: 700.0,
            query_container_sizes: Cell::new(QueryContainerSizes::default()),
            font_system: None,
            component_registry: ComponentRegistry::default(),
            scale: 1.0,
            last_cascade_vw: f32::NAN, // NAN forces cascade on first call
            last_geometry_viewport_h: f32::NAN, // NAN forces full layout on first call
            cached_has_media_q: false,
            cached_has_container_q: false,
            progressive_cutoff: 0.0,
            initial_layout_done: false,
            scheduled_font_faces: HashSet::new(),
            cached_font_families: None,
            scheduled_remote_font_bindings: HashSet::new(),
            loaded_remote_font_ids: HashMap::new(),
            failed_font_sources: HashSet::new(),
            text_width_cache: std::cell::RefCell::new(HashMap::new()),
            pending_fonts: Vec::new(),
            fonts_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            resource_cache_dir: None,
            pos_cb: Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
            fixed_cb: Cell::new(Rect::new(0.0, 0.0, 0.0, 0.0)),
            layout_depth: Cell::new(0),
            layout_calls: Cell::new(0),
            layout_start: Cell::new(None),
        }
    }

    /// Measure text width with caching. Returns logical pixel width.
    pub fn measure_text_cached(
        &self,
        text: &str,
        font_px: f32,
        weight: FontWeight,
        style: FontStyle,
        font_family: &str,
    ) -> f32 {
        self.measure_text_cached_with_stretch(text, font_px, weight, style, font_family, 100.0)
    }

    /// Measure text width with all font attributes that affect shaping.
    ///
    /// Keep this in lockstep with display-list replay: paint shapes with
    /// weight/style/family/stretch, so layout must include the same attributes
    /// or shrink-to-fit and wrapping will size one face and draw another.
    pub fn measure_text_cached_with_stretch(
        &self,
        text: &str,
        font_px: f32,
        weight: FontWeight,
        style: FontStyle,
        font_family: &str,
        font_stretch: f32,
    ) -> f32 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Build a compact cache key from text + font properties
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        font_px.to_bits().hash(&mut hasher);
        weight.value().hash(&mut hasher);
        (style as u8).hash(&mut hasher);
        font_family.hash(&mut hasher);
        font_stretch.to_bits().hash(&mut hasher);
        self.scale.to_bits().hash(&mut hasher);
        let font_state = self.font_system.map(|fs_ptr| {
            let fs = unsafe { &*fs_ptr };
            (
                fs.db().len(),
                crate::layout::inline_layout::font_size_adjust_scale(fs, font_family).to_bits(),
            )
        });
        font_state.hash(&mut hasher);
        let key = hasher.finish();

        // Check cache
        if let Some(&w) = self.text_width_cache.borrow().get(&key) {
            perf::record_text_measure(true);
            return w;
        }
        perf::record_text_measure(false);

        // Measure and cache
        let w = if let Some(fs_ptr) = self.font_system {
            let fs = unsafe { &mut *fs_ptr };
            crate::layout::inline_layout::measure_text_width_weighted(
                text,
                font_px,
                Some(fs),
                weight,
                style,
                self.scale,
                font_family,
                font_stretch,
            )
        } else {
            crate::layout::inline_layout::measure_text_width_ts(text, font_px, 8)
        };

        {
            let mut cache = self.text_width_cache.borrow_mut();
            if cache.len() >= MAX_TEXT_WIDTH_CACHE_ENTRIES {
                cache.clear();
            }
            cache.insert(key, w);
        }
        w
    }

    fn measure_text_with_css_spacing(
        &self,
        text: &str,
        font_px: f32,
        style: &ComputedStyle,
        root_font_px: f32,
    ) -> f32 {
        let shaped = self.measure_text_cached_with_stretch(
            text,
            font_px,
            style.font_weight,
            style.font_style,
            &style.font_family,
            style.font_stretch,
        );
        let letter_spacing = style.letter_spacing.resolve(font_px, 0.0, root_font_px);
        let word_spacing = style.word_spacing.resolve(font_px, 0.0, root_font_px);
        let letters = text.chars().count() as f32;
        let words = text.chars().filter(|ch| ch.is_ascii_whitespace()).count() as f32;
        shaped + letter_spacing * letters + word_spacing * words
    }

    fn field_sizing_content_width(&self, node: &WebCore, font_px: f32) -> Option<f32> {
        if !node.style.field_sizing.eq_ignore_ascii_case("content") || !node.style.width.is_auto() {
            return None;
        }
        if !crate::types::is_text_input(node) && node.tag != "textarea" {
            return None;
        }
        let value = crate::types::input_value(node);
        let text = if value.is_empty() {
            node.attributes
                .get("placeholder")
                .map(String::as_str)
                .unwrap_or("")
        } else {
            value.as_str()
        };
        let measure = |line: &str| {
            self.measure_text_with_css_spacing(line, font_px, &node.style, self.root_font_px)
        };
        let width = if node.tag == "textarea" {
            text.split('\n').map(measure).fold(0.0, f32::max)
        } else {
            measure(text)
        };
        Some(width.ceil().max(font_px))
    }

    fn text_control_intrinsic_content_width(
        &self,
        node: &WebCore,
        font_px: f32,
        _root_font_px: f32,
    ) -> Option<f32> {
        if node.tag == "select" {
            let label_width = crate::html::forms::list_of_options(node)
                .into_iter()
                .map(|option| {
                    self.measure_text_cached_with_stretch(
                        &crate::html::forms::option_label(option),
                        font_px,
                        node.style.font_weight,
                        node.style.font_style,
                        &node.style.font_family,
                        node.style.font_stretch,
                    )
                })
                .fold(0.0, f32::max);
            let indicator_width =
                if node.style.appearance == "none" || crate::html::forms::is_list_box(node) {
                    0.0
                } else {
                    crate::widgets::select::indicator_inline_size(font_px)
                        + crate::widgets::select::label_inline_inset(font_px) * 2.0
                };
            let scrollbar = if crate::html::forms::is_list_box(node) {
                node.style.scrollbar_width_px()
            } else {
                0.0
            };
            return Some(label_width + indicator_width + scrollbar);
        }
        if node.tag != "textarea" && (node.tag != "input" || !crate::types::is_text_input(node)) {
            return None;
        }
        if let Some(w) = self.field_sizing_content_width(node, font_px) {
            return Some(w);
        }
        let (average, maximum) = self.text_control_character_widths(node, font_px);
        if node.tag == "textarea" {
            return Some(
                crate::html::forms::textarea_character_width(node) as f32 * average
                    + node.style.scrollbar_width_px(),
            );
        }
        const DEFAULT_INPUT_CHARACTER_WIDTH: u32 = 20;
        let characters = node
            .attributes
            .get("size")
            .and_then(|size| crate::html::forms::parse_non_negative_integer(size))
            .unwrap_or(DEFAULT_INPUT_CHARACTER_WIDTH);
        Some(((characters as f32 - 1.0) * average + maximum).max(0.0))
    }

    fn text_control_character_widths(&self, node: &WebCore, font_px: f32) -> (f32, f32) {
        let metrics = self.font_system.and_then(|fonts| {
            inline_layout::control_character_widths(
                unsafe { &mut *fonts },
                &node.style.font_family,
                font_px,
                node.style.font_weight,
                node.style.font_style,
                node.style.font_stretch,
            )
        });
        // Fontless layout callers retain the engine's text-measurement fallback.
        metrics.unwrap_or_else(|| {
            (
                self.measure_text_cached_with_stretch(
                    "0",
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                    node.style.font_stretch,
                ),
                self.measure_text_cached_with_stretch(
                    "W",
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                    node.style.font_stretch,
                ),
            )
        })
    }

    fn text_control_line_height(&self, node: &WebCore, font_px: f32, root_font_px: f32) -> f32 {
        if node.style.line_height.is_auto() {
            let fonts = unsafe { self.font_system.map(|fonts| &mut *fonts) };
            inline_layout::font_metrics(fonts, &node.style.font_family, font_px).2
        } else {
            self.res_len(&node.style.line_height, font_px, font_px, root_font_px)
                .max(0.0)
        }
    }

    fn field_sizing_content_height(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
        width: f32,
    ) -> Option<f32> {
        if !node.style.field_sizing.eq_ignore_ascii_case("content") || !node.style.height.is_auto()
        {
            return None;
        }
        if node.tag != "textarea" {
            return None;
        }
        let value = crate::types::input_value(node);
        let text = if value.is_empty() {
            node.attributes
                .get("placeholder")
                .map(String::as_str)
                .unwrap_or("")
        } else {
            value.as_str()
        };
        Some(
            self.text_control_extent(node, text, width, font_px, root_font_px)
                .1
                .ceil(),
        )
    }

    fn text_control_extent(
        &self,
        node: &WebCore,
        text: &str,
        width: f32,
        font_px: f32,
        root_font_px: f32,
    ) -> (f32, f32) {
        if let Some(fonts) = unsafe { self.font_system.map(|fonts| &mut *fonts) } {
            let typography = crate::renderer::display_list_builder::native_control_typography(
                node,
                &node.style,
                root_font_px,
                root_font_px,
                width,
            );
            let text = crate::renderer::display_list_builder::apply_text_transform(
                text,
                typography.text_transform,
            );
            let line_height = typography.used_line_height(fonts, false);
            crate::renderer::display_list_replay::painted_control_extent(
                fonts,
                &text,
                &typography,
                line_height,
                1.0,
            )
        } else {
            (
                text.split('\n')
                    .map(|line| {
                        self.measure_text_cached_with_stretch(
                            line,
                            font_px,
                            node.style.font_weight,
                            node.style.font_style,
                            &node.style.font_family,
                            node.style.font_stretch,
                        )
                    })
                    .fold(0.0_f32, f32::max),
                text.split('\n').count() as f32
                    * self.text_control_line_height(node, font_px, root_font_px),
            )
        }
    }

    /// Resolve a box's styles using the engine's viewport dimensions.
    #[inline]
    pub fn res_box(
        &self,
        style: &ComputedStyle,
        font_px: f32,
        containing_w: f32,
        root_font_px: f32,
    ) -> ResolvedBox {
        resolve_box_vp_query(
            style,
            font_px,
            containing_w,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
            None,
            self.query_container_sizes.get(),
        )
    }

    /// Resolve a single CSS length using the engine's viewport dimensions.
    #[inline]
    pub fn res_len(
        &self,
        len: &CssLength,
        font_px: f32,
        containing: f32,
        root_font_px: f32,
    ) -> f32 {
        len.resolve_query_vp(
            font_px,
            containing,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
            self.query_container_sizes.get(),
        )
    }

    /// Compute both min-content and max-content intrinsic widths in one call.
    /// This is the **unified intrinsic sizing API** — all callers (flex, grid,
    /// table, float, absolute) should use this instead of the separate functions.
    ///
    /// Returns `IntrinsicSizes { min_content, max_content }`.
    /// Results are cached via `cached_intrinsic_w` (max) on the node's LayoutBox.
    pub fn intrinsic_sizes(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> IntrinsicSizes {
        let _profile = crate::profile::span(crate::profile::Phase::LayoutIntrinsic);
        IntrinsicSizes {
            min_content: self.min_content_width(node, parent_font_px, root_font_px),
            max_content: self.max_content_width(node, parent_font_px, root_font_px),
        }
    }

    pub(crate) fn intrinsic_sizes_with_width_basis(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
        width_basis: f32,
    ) -> IntrinsicSizes {
        let _profile = crate::profile::span(crate::profile::Phase::LayoutIntrinsic);
        IntrinsicSizes {
            min_content: self.min_content_width_inner(
                node,
                parent_font_px,
                root_font_px,
                true,
                Some(width_basis),
            ),
            max_content: self.max_content_width_inner(
                node,
                parent_font_px,
                root_font_px,
                true,
                Some(width_basis),
            ),
        }
    }

    /// Intrinsic dimensions of a replaced element: the decoded image/canvas
    /// bitmap, else the `width`/`height` content attributes, else an `<svg>`
    /// viewBox. Media elements use their HTML fallback dimensions until a
    /// decoder can report stream metadata. `None` when the box is not replaced
    /// or its natural size is unknown.
    ///
    /// Layout and the intrinsic-width walk both size replaced boxes, so they
    /// read the natural size from here and cannot disagree about it.
    pub(crate) fn intrinsic_dimensions(&self, node: &WebCore) -> Option<(f32, f32)> {
        if node.is_image_element()
            || node.tag == "canvas"
            || matches!(node.tag.as_str(), "audio" | "video")
        {
            if node.image_width > 0 && node.image_height > 0 {
                return Some((node.image_width as f32, node.image_height as f32));
            }
            // Nothing decoded/allocated yet: the attributes stand in so the
            // box reserves the right shape before the bytes arrive. A canvas
            // has spec defaults even with no attributes.
            let attr = |k: &str| {
                node.attributes
                    .get(k)
                    .and_then(|s| crate::html::forms::parse_non_negative_integer(s))
                    .map(|value| value as f32)
                    .unwrap_or(0.0)
            };
            let (attr_w, attr_h) = (attr("width"), attr("height"));
            if node.tag == "video" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 150.0 };
                return Some((w, h));
            }
            if node.tag == "audio" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 54.0 };
                return Some((w, h));
            }
            if node.tag == "canvas" {
                let w = if attr_w > 0.0 { attr_w } else { 300.0 };
                let h = if attr_h > 0.0 { attr_h } else { 150.0 };
                return Some((w, h));
            }
            return match (attr_w > 0.0, attr_h > 0.0) {
                (true, true) => Some((attr_w, attr_h)),
                (true, false) => Some((attr_w, attr_w * 0.75)),
                (false, true) => Some((attr_h * 1.333, attr_h)),
                (false, false) => None,
            };
        }
        if node.tag == "svg" && node.svg_viewbox_w > 0.0 && node.svg_viewbox_h > 0.0 {
            return Some((node.svg_viewbox_w, node.svg_viewbox_h));
        }
        None
    }

    /// **A replaced element contributes the size it is DISPLAYED at, not its
    /// natural size** (CSS2.1 §10.4). With `width: auto` and a definite height
    /// the used width follows the height through the intrinsic ratio, so a
    /// 1024×1024 photo shown at `height: 150px` contributes 150 — reading the
    /// natural width instead made every card in a wrapping flex row as wide as
    /// the photo behind it.
    ///
    /// The result is a CONTENT-box width; the caller adds padding and border.
    fn replaced_intrinsic_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        let (iw, ih) = self.intrinsic_dimensions(node)?;
        if iw <= 0.0 || ih <= 0.0 {
            return None;
        }
        // A percentage has no containing block to resolve against while
        // measuring intrinsics, so it counts as indefinite.
        let definite = |len: &CssLength| -> f32 {
            if len.is_auto() || len.has_percentage() {
                return 0.0;
            }
            self.res_len(len, font_px, 0.0, root_font_px)
        };
        let h = definite(&node.style.height);
        if h > 0.0 {
            return Some((h * iw / ih).round().max(0.0));
        }
        let (mut w, mut h) = (iw, ih);
        let max_w = definite(&node.style.max_width);
        if max_w > 0.0 && w > max_w {
            h = (max_w * ih / iw).round();
            w = max_w;
        }
        let max_h = definite(&node.style.max_height);
        if max_h > 0.0 && h > max_h {
            w = (max_h * iw / ih).round();
        }
        Some(w.max(0.0))
    }

    /// Compute the min-content width of a node (the smallest width it can take
    /// without overflowing).  For text, this is the width of the longest word.
    pub fn min_content_width(&self, node: &WebCore, parent_font_px: f32, root_font_px: f32) -> f32 {
        self.min_content_width_inner(node, parent_font_px, root_font_px, true, None)
    }

    /// Min-content width with the element's own `width` ignored. This is the
    /// CONTENT size suggestion of Flexbox §4.5 — the automatic minimum is the
    /// smaller of it and the specified size, so reading the specified width
    /// here would make the two the same number and stop the item shrinking.
    /// The width a definite height gives a box through its `aspect-ratio` —
    /// css-sizing-4 §4, the height→width direction of the transfer.
    ///
    /// `None` when the box has no ratio or no definite height, which leaves
    /// the caller's own measurement in charge.
    fn aspect_ratio_transferred_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        let ratio = node.style.aspect_ratio.filter(|r| *r > 0.0)?;
        // A percentage height is not definite during intrinsic measurement:
        // there is no containing height to resolve it against here.
        if node.style.height.is_auto() || node.style.height.has_percentage() {
            return None;
        }
        let mut h = self.res_len(&node.style.height, font_px, 0.0, root_font_px);
        if node.style.box_sizing == BoxSizing::BorderBox {
            let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
            h = (h - rb.padding_top - rb.padding_bottom - rb.border_top - rb.border_bottom)
                .max(0.0);
        }
        if h > 0.0 { Some(h * ratio) } else { None }
    }

    /// A sizing length that may be an INTRINSIC KEYWORD rather than a length.
    ///
    /// `min-width` and `max-width` accept `min-content`/`max-content`/
    /// `fit-content` too (css-sizing-3 §5), and `res_len` answers 0 for those
    /// — so `min-width: max-content` on a narrow box was simply a floor of
    /// zero, and `max-width: max-content` read as "no maximum".
    pub fn res_len_sizing(
        &self,
        len: &CssLength,
        node: &WebCore,
        avail: f32,
        font_px: f32,
        containing_w: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        len.intrinsic().map(|kind| {
            self.intrinsic_width(&kind, node, avail, font_px, root_font_px, containing_w)
        })
    }

    fn clamp_resolved_content_width(
        &self,
        rbox: &mut ResolvedBox,
        node: &WebCore,
        containing_w: f32,
        font_px: f32,
        root_font_px: f32,
    ) {
        let Some(raw_w) = rbox.content_width else {
            return;
        };
        let bb_extra = if node.style.box_sizing == crate::types::BoxSizing::BorderBox {
            rbox.padding_left + rbox.padding_right + rbox.border_left + rbox.border_right
        } else {
            0.0
        };
        let avail_w = (containing_w - rbox.h_space()).max(0.0);
        let min_w = match self.res_len_sizing(
            &node.style.min_width,
            node,
            avail_w,
            font_px,
            containing_w,
            root_font_px,
        ) {
            Some(v) => v,
            None => {
                let v = self.res_len(&node.style.min_width, font_px, containing_w, root_font_px);
                (v - bb_extra).max(0.0)
            }
        };
        let max_w = match self.res_len_sizing(
            &node.style.max_width,
            node,
            avail_w,
            font_px,
            containing_w,
            root_font_px,
        ) {
            Some(v) => v,
            None if node.style.max_width.is_none() || node.style.max_width.is_auto() => f32::MAX,
            None => {
                let v = self.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
                (v - bb_extra).max(0.0)
            }
        };
        rbox.content_width = Some(raw_w.max(min_w).min(max_w));
    }

    pub(crate) fn resolved_content_height_limits(
        &self,
        style: &ComputedStyle,
        rbox: &ResolvedBox,
        height_basis: Option<f32>,
        font_px: f32,
        root_font_px: f32,
    ) -> (f32, f32) {
        let extra = if style.box_sizing == BoxSizing::BorderBox {
            rbox.inner_v_space()
        } else {
            0.0
        };
        let resolve = |length: &CssLength, fallback| {
            if length.is_auto()
                || length.is_none()
                || height_basis.is_none() && length.has_percentage()
            {
                fallback
            } else {
                (self.res_len(length, font_px, height_basis.unwrap_or(0.0), root_font_px) - extra)
                    .max(0.0)
            }
        };
        let minimum = resolve(&style.min_height, 0.0);
        let maximum = resolve(&style.max_height, f32::INFINITY).max(minimum);
        (minimum, maximum)
    }

    /// Turn an intrinsic sizing keyword into a width — css-sizing-3 §5, §6.1.
    ///
    /// One definition, shared by block, inline and flex container sizing. Each
    /// of the three had its own copy of the same three-arm match, so a form the
    /// spec added later — `fit-content(<length>)` — had to be taught to all
    /// three or silently behave as the bare keyword in whichever was missed.
    pub fn intrinsic_width(
        &self,
        kind: &CssLength,
        node: &WebCore,
        avail: f32,
        font_px: f32,
        root_font_px: f32,
        containing_w: f32,
    ) -> f32 {
        let mn = self.min_content_width_of_content(node, font_px, root_font_px);
        let mx = self.max_content_width_of_content(node, font_px, root_font_px);
        match kind {
            CssLength::MinContent => mn,
            CssLength::MaxContent => mx,
            // The function form substitutes its argument for the available
            // space: max(min-content, min(max-content, argument)).
            CssLength::FitContentArg(a) => {
                let x = a.resolve(font_px, containing_w, root_font_px);
                mx.min(x).max(mn)
            }
            // `fit-content` is max-content clamped to what is available,
            // floored by min-content.
            _ => mx.min(avail).max(mn),
        }
    }

    pub fn min_content_width_of_content(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.min_content_width_inner(node, parent_font_px, root_font_px, false, None)
    }

    fn empty_pseudo_lacks_intrinsic_contribution(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> bool {
        if !node.is_pseudo_element() || !node.text.is_empty() {
            return false;
        }
        if !node.style.width.is_auto() || !node.style.height.is_auto() {
            return false;
        }
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
        let edges = rb.padding_left
            + rb.padding_right
            + rb.padding_top
            + rb.padding_bottom
            + rb.border_left
            + rb.border_right
            + rb.border_top
            + rb.border_bottom
            + rb.margin_left
            + rb.margin_right
            + rb.margin_top
            + rb.margin_bottom;
        edges <= 0.0
    }

    fn explicit_empty_pseudo_intrinsic_width(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        if !node.is_pseudo_element()
            || !node.text.is_empty()
            || node.style.width.is_auto()
            || node.style.display == Display::Inline
        {
            return None;
        }
        if node.style.width.has_percentage() {
            return None;
        }
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        let mut width = self
            .res_len(&node.style.width, font_px, 0.0, root_font_px)
            .max(0.0);
        if node.style.box_sizing == BoxSizing::BorderBox {
            let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
            let edges = rb.padding_left + rb.padding_right + rb.border_left + rb.border_right;
            width = (width - edges).max(0.0);
        }
        Some(width)
    }

    fn min_content_width_inner(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
        honor_width: bool,
        width_basis: Option<f32>,
    ) -> f32 {
        if matches!(node.style.display, Display::None) {
            return 0.0;
        }

        let font_px = node.style.font_size_px(parent_font_px, root_font_px);

        if let Some(w) =
            self.explicit_empty_pseudo_intrinsic_width(node, parent_font_px, root_font_px)
        {
            return w;
        }

        if node.style.display == Display::Table {
            let content_min = if Self::has_inline_size_containment(&node.style) {
                self.contained_intrinsic_width_for_node(node, font_px, root_font_px)
            } else {
                table::intrinsic_min_content_width(self, node, font_px, root_font_px)
            };
            if honor_width
                && !node.style.width.is_auto()
                && !node.style.width.has_percentage()
                && node.style.width.intrinsic().is_none()
            {
                let mut specified = self.res_len(&node.style.width, font_px, 0.0, root_font_px);
                if node.style.box_sizing == BoxSizing::BorderBox {
                    let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
                    specified = (specified
                        - rb.padding_left
                        - rb.padding_right
                        - rb.border_left
                        - rb.border_right)
                        .max(0.0);
                }
                return content_min.max(specified);
            }
            return content_min;
        }

        if honor_width && matches!(node.style.width, CssLength::MaxContent) {
            return self.max_content_width_of_content(node, parent_font_px, root_font_px);
        }

        // In a min-content query, a percentage max-width is cyclic: an image
        // with width from its HTML attribute can still shrink with its parent.
        if node.is_image_element() && node.style.max_width.has_percentage() && width_basis.is_none()
        {
            return self
                .res_len(&node.style.max_width, font_px, 0.0, root_font_px)
                .max(0.0);
        }

        // Explicit width → use that directly
        if honor_width
            && !(node.is_pseudo_element() && node.style.display == Display::Inline)
            && !node.style.width.is_auto()
            && node.style.width.intrinsic().is_none()
            && (!node.style.width.has_percentage() || width_basis.is_some())
        {
            let basis = width_basis.unwrap_or(0.0);
            let w = self.res_len(&node.style.width, font_px, basis, root_font_px);
            // ⛔ A `border-box` width ALREADY contains the padding and border,
            // and every caller adds those again on top of what we return — the
            // contract here is a CONTENT width. Handing back the raw value made
            // a shrink-to-fit parent wider by exactly the child's padding and
            // border, on the near-universal `* { box-sizing: border-box }`.
            if node.style.box_sizing == BoxSizing::BorderBox {
                let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
                let edges = rb.padding_left + rb.padding_right + rb.border_left + rb.border_right;
                return (w - edges).max(0.0);
            }
            return w.max(0.0);
        }

        // css-sizing-4 §4: a box with a preferred aspect ratio and a DEFINITE
        // block size transfers that size through the ratio — in BOTH
        // directions. Only the width→height direction was implemented, so
        // `display:inline-block; aspect-ratio:2/1; height:100px` measured as
        // zero wide and collapsed, instead of 200.
        if honor_width {
            if let Some(w) = self.aspect_ratio_transferred_width(node, font_px, root_font_px) {
                return w;
            }
        }

        if Self::has_inline_size_containment(&node.style) {
            return self.contained_intrinsic_width_for_node(node, font_px, root_font_px);
        }

        // A cyclic percentage width cannot make its containing block's
        // min-content size definite. Its fixed component still contributes.
        if node.is_image_element() && node.style.width.has_percentage() && width_basis.is_none() {
            return self
                .res_len(&node.style.width, font_px, 0.0, root_font_px)
                .max(0.0);
        }

        // Replaced elements: the size they are shown at, ratio included.
        if let Some(w) = self.replaced_intrinsic_width(node, font_px, root_font_px) {
            return w;
        }

        if let Some(w) = self.text_control_intrinsic_content_width(node, font_px, root_font_px) {
            return w;
        }

        // Custom component: use cached dimensions (like a replaced element)
        if self.component_registry.get_component(&node.tag).is_some()
            || self.component_registry.map.contains_key(&node.tag)
        {
            return if node.component_width > 0.0 {
                node.component_width
            } else {
                // First call before layout — measure to get initial size
                if let Some(c) = self.component_registry.get_component(&node.tag) {
                    c.measure(node, 0.0).0
                } else if let Some(cb) = self.component_registry.map.get(&node.tag) {
                    (cb.measure)(node, 0.0).0
                } else {
                    0.0
                }
            };
        }

        let _rbox = self.res_box(&node.style, font_px, 0.0, root_font_px);
        let generated_inline_w = self.generated_inline_content_width(node, font_px, root_font_px);
        let own_text_w = self.min_content_width_of_direct_text(node, font_px, root_font_px);

        // Text node or pseudo-element (::before/::after) with direct text content.
        // Pseudo-elements store content in node.text, not as #text children.
        let is_pseudo = matches!(node.tag.as_str(), "::before" | "::after");
        let has_direct_text = node.is_text_node() || (is_pseudo && !node.text.is_empty());
        if has_direct_text {
            let text = &node.text;
            if text.is_empty() {
                return 0.0;
            }
            if node.style.overflow_wrap == OverflowWrap::Anywhere
                || node.style.word_break == WordBreak::BreakWord
            {
                if let Some(width) =
                    self.inline_items_min_content_width(node, font_px, root_font_px)
                {
                    return width;
                }
            }
            if matches!(node.style.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre) {
                let text = if matches!(node.style.white_space, WhiteSpace::Nowrap) {
                    text.split_whitespace().collect::<Vec<_>>().join(" ")
                } else {
                    text.clone()
                };
                if text.is_empty() {
                    return 0.0;
                }
                let text = crate::renderer::display_list_builder::apply_text_transform(
                    &text,
                    node.style.text_transform,
                );
                return self.measure_text_with_css_spacing(
                    &text,
                    font_px,
                    &node.style,
                    root_font_px,
                );
            }
            let mut max_word = 0.0f32;
            for word in text.split(|c: char| c.is_ascii_whitespace()) {
                if word.is_empty() {
                    continue;
                }
                let word = crate::renderer::display_list_builder::apply_text_transform(
                    word,
                    node.style.text_transform,
                );
                let w =
                    self.measure_text_with_css_spacing(&word, font_px, &node.style, root_font_px);
                if w > max_word {
                    max_word = w;
                }
            }
            return max_word;
        }

        // ⛔ A SINGLE-LINE ROW FLEX CONTAINER SUMS ITS ITEMS (css-flexbox-1
        // §9.9). Its min-content main size is computed exactly like the
        // max-content main size, but from the items' MIN-content
        // contributions — so the items sit side by side and their widths add.
        // Taking the max here reported `min-content` on a nowrap row flex as
        // the widest single item, which is what a wrap container does.
        let single_line_row_flex =
            matches!(node.style.display, Display::Flex | Display::InlineFlex)
                && matches!(
                    node.style.flex_direction,
                    FlexDirection::Row | FlexDirection::RowReverse
                )
                && matches!(node.style.flex_wrap, FlexWrap::Nowrap);
        if single_line_row_flex {
            let gap = self.res_len(&node.style.column_gap, font_px, 0.0, root_font_px);
            let mut total = 0.0f32;
            let mut count = 0usize;
            for ch in node.effective_children() {
                if matches!(ch.style.display, Display::None) {
                    continue;
                }
                if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if self.empty_pseudo_lacks_intrinsic_contribution(ch, font_px, root_font_px) {
                    continue;
                }
                if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                    continue;
                }
                let child_font = ch.style.font_size_px(font_px, root_font_px);
                let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
                let child_outer = child_rbox.padding_left
                    + child_rbox.padding_right
                    + child_rbox.border_left
                    + child_rbox.border_right
                    + child_rbox.margin_left
                    + child_rbox.margin_right;
                if count > 0 {
                    total += gap;
                }
                total += self.min_content_width(ch, font_px, root_font_px) + child_outer;
                count += 1;
            }
            return total;
        }

        if matches!(node.style.white_space, WhiteSpace::Nowrap | WhiteSpace::Pre) {
            return self.max_content_width_of_content(node, font_px, root_font_px);
        }

        if let Some(w) = self.inline_items_min_content_width(node, font_px, root_font_px) {
            return w;
        }

        // For containers: max of children's min-content widths
        let mut max_w = generated_inline_w.max(own_text_w);
        for ch in node.effective_children() {
            if matches!(ch.style.display, Display::None) {
                continue;
            }
            if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            if self.empty_pseudo_lacks_intrinsic_contribution(ch, font_px, root_font_px) {
                continue;
            }
            let child_font = ch.style.font_size_px(font_px, root_font_px);
            let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
            let child_outer = child_rbox.padding_left
                + child_rbox.padding_right
                + child_rbox.border_left
                + child_rbox.border_right
                + child_rbox.margin_left
                + child_rbox.margin_right;
            let mut cw = self.min_content_width(ch, font_px, root_font_px) + child_outer;
            if !ch.style.min_width.is_auto() && !ch.style.min_width.has_percentage() {
                let min_w = self.res_len(&ch.style.min_width, child_font, 0.0, root_font_px);
                cw = cw.max(if ch.style.box_sizing == BoxSizing::BorderBox {
                    min_w + child_rbox.margin_left + child_rbox.margin_right
                } else {
                    min_w + child_outer
                });
            }
            if cw > max_w {
                max_w = cw;
            }
        }
        max_w
    }

    pub fn max_content_width(&self, node: &WebCore, parent_font_px: f32, root_font_px: f32) -> f32 {
        self.max_content_width_inner(node, parent_font_px, root_font_px, true, None)
    }

    /// Max-content width with the element's own `width` ignored, which is what
    /// `flex-basis: content` asks for (Flexbox §7.2.3): size from the content
    /// and disregard the specified size.
    pub fn max_content_width_of_content(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.max_content_width_inner(node, parent_font_px, root_font_px, false, None)
    }

    fn max_content_width_inner(
        &self,
        node: &WebCore,
        parent_font_px: f32,
        root_font_px: f32,
        honor_width: bool,
        width_basis: Option<f32>,
    ) -> f32 {
        if matches!(node.style.display, Display::None) {
            return 0.0;
        }

        // Explicit width → use that directly (but skip percentages — they can't
        // resolve without a known containing width during intrinsic measurement).
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        if let Some(w) =
            self.explicit_empty_pseudo_intrinsic_width(node, parent_font_px, root_font_px)
        {
            return w;
        }

        if honor_width
            && !(node.is_pseudo_element() && node.style.display == Display::Inline)
            && !node.style.width.is_auto()
            && node.style.width.intrinsic().is_none()
            && (!node.style.width.has_percentage() || width_basis.is_some())
        {
            let basis = width_basis.unwrap_or(0.0);
            let w = self.res_len(&node.style.width, font_px, basis, root_font_px);
            // ⛔ A `border-box` width ALREADY contains the padding and border,
            // and every caller adds those again on top of what we return — the
            // contract here is a CONTENT width. Handing back the raw value made
            // a shrink-to-fit parent wider by exactly the child's padding and
            // border, on the near-universal `* { box-sizing: border-box }`.
            if node.style.box_sizing == BoxSizing::BorderBox {
                let rb = self.res_box(&node.style, font_px, 0.0, root_font_px);
                let edges = rb.padding_left + rb.padding_right + rb.border_left + rb.border_right;
                return (w - edges).max(0.0);
            }
            return w.max(0.0);
        }

        // css-sizing-4 §4: a box with a preferred aspect ratio and a DEFINITE
        // block size transfers that size through the ratio — in BOTH
        // directions. Only the width→height direction was implemented, so
        // `display:inline-block; aspect-ratio:2/1; height:100px` measured as
        // zero wide and collapsed, instead of 200.
        if let Some(w) = self.aspect_ratio_transferred_width(node, font_px, root_font_px) {
            return w;
        }

        if Self::has_inline_size_containment(&node.style) {
            return self.contained_intrinsic_width_for_node(node, font_px, root_font_px);
        }

        // Replaced elements: the size they are shown at, ratio included.
        if let Some(w) = self.replaced_intrinsic_width(node, font_px, root_font_px) {
            return w;
        }

        if let Some(w) = self.text_control_intrinsic_content_width(node, font_px, root_font_px) {
            return w;
        }

        // **An `<input>` button is sized by its LABEL, which is an attribute.**
        //
        // `submit`/`reset`/`button` are void elements: no children, no line
        // boxes, so the walk below measures nothing and an auto-width button
        // collapsed to its padding — a pill a few pixels wide with the label
        // spilling out beside it. `<button>` is unaffected because its label is
        // real content and gets measured like any other text.
        //
        // The UA-supplied default matters here too: a bare `<input
        // type=submit>` reads "Submit" in a browser and must be sized for that
        // word, not for the empty string.
        if node.tag == "input" {
            let input_type = node
                .attributes
                .get("type")
                .map(|t| t.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(input_type.as_str(), "submit" | "reset" | "button") {
                let label = match node.attributes.get("value").map(String::as_str) {
                    Some(v) => v,
                    _ => match input_type.as_str() {
                        "submit" => "Submit",
                        "reset" => "Reset",
                        _ => "",
                    },
                };
                return self.measure_text_cached_with_stretch(
                    label,
                    font_px,
                    node.style.font_weight,
                    node.style.font_style,
                    &node.style.font_family,
                    node.style.font_stretch,
                );
            }
        }

        // Custom component: use cached dimensions (like a replaced element)
        if self.component_registry.get_component(&node.tag).is_some()
            || self.component_registry.map.contains_key(&node.tag)
        {
            return if node.component_width > 0.0 {
                node.component_width
            } else {
                if let Some(c) = self.component_registry.get_component(&node.tag) {
                    c.measure(node, f32::MAX).0
                } else if let Some(cb) = self.component_registry.map.get(&node.tag) {
                    (cb.measure)(node, f32::MAX).0
                } else {
                    0.0
                }
            };
        }

        let _rbox = self.res_box(&node.style, font_px, 0.0, root_font_px);
        let generated_inline_w = self.generated_inline_content_width(node, font_px, root_font_px);
        let own_text_w = self.max_content_width_of_direct_text(node, font_px, root_font_px);

        // Text node or pseudo-element (::before/::after) with direct text content.
        let is_pseudo = matches!(node.tag.as_str(), "::before" | "::after");
        let has_direct_text = node.is_text_node() || (is_pseudo && !node.text.is_empty());
        if has_direct_text {
            let text = &node.text;
            if text.is_empty() {
                return 0.0;
            }
            // Collapse whitespace for normal white-space mode (CSS §4.1.1)
            let text = if matches!(
                node.style.white_space,
                WhiteSpace::Normal | WhiteSpace::Nowrap
            ) {
                let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                collapsed
            } else {
                text.clone()
            };
            if text.is_empty() {
                return 0.0;
            }
            let text = crate::renderer::display_list_builder::apply_text_transform(
                &text,
                node.style.text_transform,
            );
            let w = self.measure_text_with_css_spacing(&text, font_px, &node.style, root_font_px);
            return w;
        }

        if let Some(w) = self.inline_items_max_content_width(node, font_px, root_font_px) {
            return w;
        }

        let is_row_flex = matches!(node.style.display, Display::Flex | Display::InlineFlex)
            && matches!(
                node.style.flex_direction,
                FlexDirection::Row | FlexDirection::RowReverse
            );
        let _is_col_flex =
            matches!(node.style.display, Display::Flex | Display::InlineFlex) && !is_row_flex;

        if is_row_flex {
            // Row flex: sum of children's max-content widths + their box model.
            let mut total = 0.0f32;
            let gap = self.res_len(&node.style.column_gap, font_px, 0.0, root_font_px);
            let mut count = 0usize;
            for ch in node.effective_children() {
                if matches!(ch.style.display, Display::None) {
                    continue;
                }
                if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                    continue;
                }
                if self.empty_pseudo_lacks_intrinsic_contribution(ch, font_px, root_font_px) {
                    continue;
                }
                if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                    continue;
                }
                let child_font = ch.style.font_size_px(font_px, root_font_px);
                let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
                let child_outer = child_rbox.padding_left
                    + child_rbox.padding_right
                    + child_rbox.border_left
                    + child_rbox.border_right
                    + child_rbox.margin_left
                    + child_rbox.margin_right;
                // Use flex-basis if it gives a definite length. `content` says
                // to measure the content, and a percentage has nothing to
                // resolve against during intrinsic measurement, so both fall
                // through to the content measurement rather than reading 0.
                let basis_is_definite = !ch.style.flex_basis.is_auto()
                    && !matches!(ch.style.flex_basis, CssLength::Content)
                    && !ch.style.flex_basis.has_percentage();
                let mut child_main = if basis_is_definite {
                    self.res_len(&ch.style.flex_basis, child_font, 0.0, root_font_px)
                        .max(0.0)
                } else if matches!(ch.style.flex_basis, CssLength::Content) {
                    self.max_content_width_of_content(ch, font_px, root_font_px)
                } else {
                    self.max_content_width(ch, font_px, root_font_px)
                };
                // ⛔ A GROWABLE ITEM CONTRIBUTES ITS MAX-CONTENT SIZE. `flex-basis`
                // is where the distribution STARTS, not a ceiling on what the
                // container needs to be (css-flexbox-1 §9.9). Reading a
                // `flex: 1 1 0` item's basis as its contribution made it count for
                // nothing, so `width: max-content` on the container came out as
                // wide as the remaining items alone.
                if ch.style.flex_grow > 0.0 {
                    let mc = self.max_content_width(ch, font_px, root_font_px);
                    if mc > child_main {
                        child_main = mc;
                    }
                }
                // The item's own minimum still floors that contribution.
                if !ch.style.min_width.is_auto() {
                    let mw = self.res_len(&ch.style.min_width, child_font, 0.0, root_font_px);
                    if mw > child_main {
                        child_main = mw;
                    }
                }
                let contribution = child_main + child_outer;
                total += contribution;
                if count > 0 {
                    total += gap;
                }
                count += 1;
            }
            return total;
        }

        // Column flex or block: max of children's max-content widths.
        // Exception: floated children sit side by side, so sum their widths.
        // ⛔ INLINE SIBLINGS SUM, BLOCK SIBLINGS MAX. max-content is the content
        // laid out with NO soft wrap opportunity taken (css-sizing-3 §5.1), so
        // everything that shares a line contributes its width to that line. A
        // plain max over all children measured `Hello <b>World</b>` as the wider
        // single word, and any shrink-to-fit box sized from it then wrapped.
        let mut max_w = generated_inline_w;
        let mut float_sum = 0.0f32;
        let mut run = generated_inline_w + own_text_w; // the inline run being accumulated
        let mut pending_collapsed_space = false;
        for ch in node.effective_children() {
            if matches!(ch.style.display, Display::None) {
                continue;
            }
            if matches!(ch.style.position, Position::Absolute | Position::Fixed) {
                continue;
            }
            if self.empty_pseudo_lacks_intrinsic_contribution(ch, font_px, root_font_px) {
                continue;
            }
            if ch.tag == "#text" && ch.text.chars().all(|c| c.is_ascii_whitespace()) {
                if run > 0.0 {
                    pending_collapsed_space = true;
                }
                continue;
            }
            let child_font = ch.style.font_size_px(font_px, root_font_px);
            let child_rbox = self.res_box(&ch.style, child_font, 0.0, root_font_px);
            let child_outer = child_rbox.padding_left
                + child_rbox.padding_right
                + child_rbox.border_left
                + child_rbox.border_right
                + child_rbox.margin_left
                + child_rbox.margin_right;
            let mut cw = self.max_content_width(ch, font_px, root_font_px) + child_outer;
            if !ch.style.min_width.is_auto() && !ch.style.min_width.has_percentage() {
                let min_w = self.res_len(&ch.style.min_width, child_font, 0.0, root_font_px);
                cw = cw.max(if ch.style.box_sizing == BoxSizing::BorderBox {
                    min_w + child_rbox.margin_left + child_rbox.margin_right
                } else {
                    min_w + child_outer
                });
            }
            if !matches!(ch.style.float, Float::None) {
                float_sum += cw;
                continue;
            }
            if ch.tag == "br" {
                if run > max_w {
                    max_w = run;
                }
                run = 0.0;
                pending_collapsed_space = false;
                continue;
            }
            if ch.style.is_inline_level() {
                let starts_with_space = inline_subtree_starts_with_collapsible_space(ch);
                let ends_with_space = inline_subtree_ends_with_collapsible_space(ch);
                if (pending_collapsed_space || (starts_with_space && run > 0.0)) && cw > 0.0 {
                    run += self.measure_text_cached(
                        " ",
                        font_px,
                        node.style.font_weight,
                        node.style.font_style,
                        &node.style.font_family,
                    );
                }
                pending_collapsed_space = false;
                run += cw;
                if ends_with_space && run > 0.0 {
                    pending_collapsed_space = true;
                }
            } else {
                // A block-level child ends the current line and owns its own.
                if run > max_w {
                    max_w = run;
                }
                run = 0.0;
                pending_collapsed_space = false;
                if cw > max_w {
                    max_w = cw;
                }
            }
        }
        if run > max_w {
            max_w = run;
        }
        // Container must be wide enough for both floats and normal flow
        max_w.max(float_sum)
    }

    fn inline_items_max_content_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        if matches!(
            node.style.display,
            Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
                | Display::Table
                | Display::TableRow
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableColumnGroup
                | Display::TableColumn
        ) {
            return None;
        }
        if !self.inline_subtree_can_use_item_intrinsic(node, font_px, root_font_px, true) {
            return None;
        }
        if self.is_atomic_inline_for_intrinsic(node, font_px, root_font_px)
            && (!node.text.is_empty()
                || !node.style.before_content.is_empty()
                || !node.style.after_content.is_empty())
        {
            return None;
        }

        let mut items = Vec::new();
        let mut runs = Vec::new();
        let mut text_offset = 0usize;
        let mut previous_collapsible_space = false;
        if self.is_atomic_inline_for_intrinsic(node, font_px, root_font_px)
            || matches!(node.style.position, Position::Absolute | Position::Fixed)
        {
            for (idx, child) in node.effective_children().iter().enumerate() {
                inline_layout::collect_items_continuing(
                    self,
                    child,
                    font_px,
                    root_font_px,
                    &mut items,
                    &mut runs,
                    &mut text_offset,
                    idx,
                    true,
                    &[],
                    &mut previous_collapsible_space,
                );
            }
        } else {
            inline_layout::collect_items_continuing(
                self,
                node,
                font_px,
                root_font_px,
                &mut items,
                &mut runs,
                &mut text_offset,
                0,
                false,
                &[],
                &mut previous_collapsible_space,
            );
        }

        if items.is_empty() {
            return None;
        }
        let advance = inline_items_max_content_advance(&items);
        let indent = self.res_len(&node.style.text_indent, font_px, 0.0, root_font_px);
        Some((advance + indent).max(0.0))
    }

    fn inline_items_min_content_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> Option<f32> {
        if matches!(
            node.style.display,
            Display::Flex
                | Display::InlineFlex
                | Display::Grid
                | Display::InlineGrid
                | Display::Table
                | Display::TableRow
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableColumnGroup
                | Display::TableColumn
        ) {
            return None;
        }
        if !self.inline_subtree_can_use_item_intrinsic(node, font_px, root_font_px, true) {
            return None;
        }
        if self.is_atomic_inline_for_intrinsic(node, font_px, root_font_px)
            && (!node.text.is_empty()
                || !node.style.before_content.is_empty()
                || !node.style.after_content.is_empty())
        {
            return None;
        }

        let mut items = Vec::new();
        let mut runs = Vec::new();
        let mut text_offset = 0usize;
        let mut previous_collapsible_space = false;
        if self.is_atomic_inline_for_intrinsic(node, font_px, root_font_px)
            || matches!(node.style.position, Position::Absolute | Position::Fixed)
        {
            for (idx, child) in node.effective_children().iter().enumerate() {
                inline_layout::collect_items_continuing(
                    self,
                    child,
                    font_px,
                    root_font_px,
                    &mut items,
                    &mut runs,
                    &mut text_offset,
                    idx,
                    true,
                    &[],
                    &mut previous_collapsible_space,
                );
            }
        } else {
            inline_layout::collect_items_continuing(
                self,
                node,
                font_px,
                root_font_px,
                &mut items,
                &mut runs,
                &mut text_offset,
                0,
                false,
                &[],
                &mut previous_collapsible_space,
            );
        }

        if items.is_empty() {
            return None;
        }
        Some(inline_items_min_content_advance(&items))
    }

    fn inline_subtree_can_use_item_intrinsic(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
        is_root: bool,
    ) -> bool {
        if matches!(node.style.display, Display::None)
            || (!is_root && matches!(node.style.position, Position::Absolute | Position::Fixed))
        {
            return true;
        }
        if !matches!(node.style.float, Float::None) {
            return false;
        }
        if !is_root && self.is_atomic_inline_for_intrinsic(node, font_px, root_font_px) {
            return false;
        }
        if !is_root
            && !node.is_text_node()
            && !node.style.is_inline_level()
            && !matches!(node.style.display, Display::Contents)
        {
            return false;
        }
        node.effective_children().iter().all(|child| {
            self.inline_subtree_can_use_item_intrinsic(child, font_px, root_font_px, false)
        })
    }

    fn is_atomic_inline_for_intrinsic(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> bool {
        matches!(
            node.style.display,
            Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
        ) || node.is_image_element()
            || matches!(
                node.tag.as_str(),
                "svg" | "canvas" | "video" | "iframe" | "input" | "select" | "textarea" | "button"
            )
            || self
                .replaced_intrinsic_width(node, font_px, root_font_px)
                .is_some()
    }

    fn generated_inline_content_width(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        self.generated_content_width(
            &node.style.before_content,
            node.style.before_style.as_deref(),
            font_px,
            root_font_px,
        ) + self.generated_content_width(
            &node.style.after_content,
            node.style.after_style.as_deref(),
            font_px,
            root_font_px,
        )
    }

    fn min_content_width_of_direct_text(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.is_text_node() || node.text.is_empty() {
            return 0.0;
        }
        node.text
            .split(|c: char| c.is_ascii_whitespace())
            .filter(|word| !word.is_empty())
            .map(|word| {
                let transformed = crate::renderer::display_list_builder::apply_text_transform(
                    word,
                    node.style.text_transform,
                );
                self.measure_text_with_css_spacing(&transformed, font_px, &node.style, root_font_px)
            })
            .fold(0.0_f32, f32::max)
    }

    fn max_content_width_of_direct_text(
        &self,
        node: &WebCore,
        font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        if node.is_text_node() || node.text.is_empty() {
            return 0.0;
        }
        let text = if matches!(
            node.style.white_space,
            WhiteSpace::Normal | WhiteSpace::Nowrap
        ) {
            node.text.split_whitespace().collect::<Vec<_>>().join(" ")
        } else {
            node.text.clone()
        };
        if text.is_empty() {
            return 0.0;
        }
        let text = crate::renderer::display_list_builder::apply_text_transform(
            &text,
            node.style.text_transform,
        );
        self.measure_text_with_css_spacing(&text, font_px, &node.style, root_font_px)
    }

    fn generated_content_width(
        &self,
        content: &str,
        style: Option<&ComputedStyle>,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> f32 {
        let Some(style) = style else {
            if content.is_empty() {
                return 0.0;
            }
            return self.measure_text_cached_with_stretch(
                content,
                parent_font_px,
                FontWeight::Normal,
                FontStyle::Normal,
                "",
                100.0,
            );
        };
        let font_px = style.font_size_px(parent_font_px, root_font_px);
        let rb = self.res_box(style, font_px, 0.0, root_font_px);
        let inline_width_ignored = matches!(style.display, Display::Inline);
        let transformed = crate::renderer::display_list_builder::apply_text_transform(
            content,
            style.text_transform,
        );
        let content_w =
            if !inline_width_ignored && !style.width.is_auto() && !style.width.has_percentage() {
                self.res_len(&style.width, font_px, 0.0, root_font_px)
            } else if transformed.is_empty() {
                0.0
            } else {
                self.measure_text_with_css_spacing(&transformed, font_px, style, root_font_px)
            };
        content_w.max(0.0)
            + rb.margin_left
            + rb.border_left
            + rb.padding_left
            + rb.padding_right
            + rb.border_right
            + rb.margin_right
    }

    /// Kick off non-blocking font loading. Base64 and local fonts are loaded
    /// immediately; remote fonts are fetched in background threads and arrive
    /// via `pending_fonts` channel — polled each `layout()` call.
    pub fn load_font_faces(
        &mut self,
        faces: &[crate::css::FontFaceDecl],
        base_url: &str,
        document_text: &str,
    ) {
        if let Some(fs_ptr) = self.font_system {
            let fs = unsafe { &mut *fs_ptr };
            let coverage = (!document_text.is_empty())
                .then(|| crate::css::font_face::UnicodeTextCoverage::new(document_text));

            // ── Phase 1: Resolve each @font-face to its best fetchable URL ──────
            let mut remote: std::collections::HashMap<String, Vec<crate::css::FontFaceDecl>> =
                std::collections::HashMap::new();
            let mut sorted_faces = faces.to_vec();
            sorted_faces.sort_by_key(|f| if is_latin_font_face(f) { 0 } else { 1 });

            for face in &sorted_faces {
                if coverage
                    .as_ref()
                    .is_some_and(|coverage| !coverage.intersects(face.unicode_range.as_deref()))
                {
                    continue;
                }
                let mut found = false;
                let parsed_sources;
                let sources = if face.sources.is_empty() {
                    parsed_sources = crate::css::font_face::parse_font_face_sources(&face.src);
                    parsed_sources.as_slice()
                } else {
                    face.sources.as_slice()
                };
                for source in sources {
                    if found {
                        break;
                    }
                    if !font_source_formats_supported(source) {
                        continue;
                    };
                    if !font_source_techs_supported(source) {
                        continue;
                    }
                    let url_inner = match &source.kind {
                        crate::css::FontFaceSourceKind::Local(name) => {
                            let key = format!(
                                "local({name}) as {} {:?} {:?} {:?}",
                                face.family, face.weight, face.style, face.stretch
                            );
                            if self.scheduled_font_faces.contains(&key) {
                                found = true;
                                continue;
                            }
                            found = load_local_font_face(fs, face, name);
                            if found {
                                self.scheduled_font_faces.insert(key);
                            }
                            continue;
                        }
                        crate::css::FontFaceSourceKind::Url(url) => url.as_str(),
                    };

                    // Strip fragment (#iefix etc.)
                    let url_clean = url_inner.split('#').next().unwrap_or(url_inner);
                    // Strip query string for extension check
                    let url_for_ext = url_clean.split('?').next().unwrap_or(url_clean);

                    // SVG fonts were removed from browsers. EOT, WOFF and
                    // modern SFNT sources pass through the font decoder.
                    if url_for_ext
                        .get(url_for_ext.len().saturating_sub(4)..)
                        .is_some_and(|ext| ext.eq_ignore_ascii_case(".svg"))
                    {
                        continue;
                    }

                    // Base64 data URI — load immediately (no network)
                    if let Some(b64) = url_inner
                        .strip_prefix("data:")
                        .and_then(|s| s.find(";base64,").map(|i| &s[i + 8..]))
                    {
                        let key = url_inner.to_string();
                        if self.scheduled_font_faces.contains(&key) {
                            found = true;
                            continue;
                        }
                        if let Ok(bytes) = decode_base64(b64.trim()) {
                            found = load_font_face_bytes_for_page(
                                fs,
                                face,
                                std::sync::Arc::new(bytes),
                                base_url,
                            );
                            if found {
                                self.scheduled_font_faces.insert(key);
                            }
                        }
                        continue;
                    }

                    let resolved = crate::html::resolve_url(url_clean, base_url);
                    if self.failed_font_sources.contains(&resolved) {
                        continue;
                    }
                    let key = resolved.clone();
                    if resolved.starts_with("http://") || resolved.starts_with("https://") {
                        if self
                            .scheduled_remote_font_bindings
                            .insert((resolved.clone(), face.clone()))
                        {
                            if let Some(ids) = self.loaded_remote_font_ids.get(&resolved) {
                                register_css_font_face_alias(fs, face, ids);
                            } else {
                                remote
                                    .entry(resolved.clone())
                                    .or_default()
                                    .push(face.clone());
                            }
                            self.scheduled_font_faces.insert(key);
                        }
                        found = true;
                    } else if self.scheduled_font_faces.contains(&key) {
                        found = true;
                    } else if !resolved.is_empty() {
                        // Local file — load immediately.
                        //
                        // ⛔ `file://` has to come off first: `resolve_url`
                        // returns a URL, and `fs::read` wants a path. A
                        // `@font-face` pointing at a local file silently
                        // loaded nothing, and the text was measured in the
                        // fallback font instead.
                        let path = resolved.strip_prefix("file://").unwrap_or(&resolved);
                        if let Ok(data) = std::fs::read(path) {
                            found = load_font_face_bytes_for_page(
                                fs,
                                face,
                                std::sync::Arc::new(data),
                                base_url,
                            );
                            if found {
                                self.scheduled_font_faces.insert(key);
                            }
                        }
                    }
                }
            }

            // ── Phase 2: Fire-and-forget remote font fetches ────────────────────
            if !remote.is_empty() {
                let (tx, rx) = std::sync::mpsc::channel::<PendingFontResult>();
                let in_flight = self.fonts_in_flight.clone();
                let cache_dir = self.resource_cache_dir.clone();
                in_flight.fetch_add(remote.len(), std::sync::atomic::Ordering::SeqCst);

                for (url, faces) in remote {
                    let sender = tx.clone();
                    let counter = in_flight.clone();
                    let cache_dir = cache_dir.clone();
                    let page_url = base_url.to_string();
                    let requested_at = std::time::Instant::now();
                    crate::spawn_font_resource_task(move || {
                        let result = cached_remote_font_bytes(&url, cache_dir.as_deref()).and_then(
                            |(bytes, blocks)| {
                                decode_font_bytes_for_page_with_mtx_blocks(bytes, &page_url, blocks)
                            },
                        );
                        if let Some(bytes) = result {
                            eprintln!(
                                "  Font loaded: {} face(s) ({} bytes) from {}",
                                faces.len(),
                                bytes.len(),
                                &url[..url.len().min(80)]
                            );
                            let _ = sender.send(PendingFontResult {
                                faces,
                                url,
                                bytes: Some(bytes),
                                requested_at,
                                completed_at: std::time::Instant::now(),
                            });
                        } else {
                            eprintln!("  Font fetch failed: {}", &url[..url.len().min(80)]);
                            let _ = sender.send(PendingFontResult {
                                faces,
                                url,
                                bytes: None,
                                requested_at,
                                completed_at: std::time::Instant::now(),
                            });
                        }
                        counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    });
                }
                self.pending_fonts.push(rx);
            }
        }
    }

    /// Load font face bytes directly into this engine's font system.
    pub fn load_font_face_bytes(
        &mut self,
        face: &crate::css::FontFaceDecl,
        bytes: Vec<u8>,
    ) -> bool {
        self.load_font_face_bytes_for_page(face, bytes, "")
    }

    /// Register a font supplied directly by the embedder. A document URL is
    /// required when an EOT RootString restricts where the font may be used.
    pub fn load_font_face_bytes_for_page(
        &mut self,
        face: &crate::css::FontFaceDecl,
        bytes: Vec<u8>,
        page_url: &str,
    ) -> bool {
        let fs = match self.font_system {
            Some(ptr) => unsafe { &mut *ptr },
            None => return false,
        };
        load_font_face_bytes_for_page(fs, face, std::sync::Arc::new(bytes), page_url)
    }

    /// Poll for fonts that have arrived from background threads.
    /// Returns `true` if any new fonts were loaded (caller should re-layout).
    pub fn poll_pending_fonts(&mut self) -> bool {
        self.poll_pending_fonts_budgeted(usize::MAX, std::time::Duration::from_secs(60))
    }

    /// Poll a bounded amount of remote font work. Browser shells use this path
    /// so a page with many webfonts cannot monopolize a UI frame the moment
    /// those fetches complete.
    pub fn poll_pending_fonts_budgeted(
        &mut self,
        max_fonts: usize,
        max_time: std::time::Duration,
    ) -> bool {
        if max_fonts == 0 || self.pending_fonts.is_empty() {
            return false;
        }
        let fs = match self.font_system {
            Some(ptr) => unsafe { &mut *ptr },
            None => return false,
        };

        let mut changed_any = false;
        let start = std::time::Instant::now();
        let time_limited = !max_time.is_zero();
        let mut processed = 0usize;
        let mut stopped_early = false;
        for rx in &self.pending_fonts {
            while let Ok(result) = rx.try_recv() {
                if let Some(bytes) = result.bytes {
                    let elapsed = result
                        .completed_at
                        .saturating_duration_since(result.requested_at);
                    let faces: Vec<_> = result
                        .faces
                        .into_iter()
                        .filter(|face| font_face_can_swap(face, elapsed))
                        .collect();
                    if !faces.is_empty() {
                        let ids = self
                            .loaded_remote_font_ids
                            .entry(result.url.clone())
                            .or_insert_with(|| load_font_bytes(fs, bytes));
                        if !ids.is_empty() {
                            for face in &faces {
                                register_css_font_face_alias(fs, face, ids);
                            }
                            changed_any = true;
                        } else {
                            self.scheduled_font_faces.remove(&result.url);
                            self.failed_font_sources.insert(result.url);
                            changed_any = true;
                        }
                    }
                } else {
                    self.scheduled_font_faces.remove(&result.url);
                    self.failed_font_sources.insert(result.url);
                    changed_any = true;
                }
                processed += 1;
                if processed >= max_fonts || (time_limited && start.elapsed() >= max_time) {
                    stopped_early = true;
                    break;
                }
            }
            if stopped_early {
                break;
            }
        }

        // A completed worker may still have an unread message in a channel.
        if !stopped_early
            && self
                .fonts_in_flight
                .load(std::sync::atomic::Ordering::SeqCst)
                == 0
        {
            self.pending_fonts.clear();
        }

        changed_any
    }

    /// Returns `true` if there are still font fetches in flight.
    pub fn has_pending_fonts(&self) -> bool {
        !self.pending_fonts.is_empty()
    }

    /// Main entry point: layout the full document.
    pub fn layout(&mut self, doc: &mut Document, viewport_width: f32) {
        let trace_start = std::time::Instant::now();
        let scroll_anchor =
            scroll_anchor::ViewportAnchor::capture(doc, viewport_width, self.viewport_h);
        let element_anchors = scroll_anchor::ElementAnchors::capture(doc);
        self.viewport_w = viewport_width;
        // Use the document's viewport_h if it was set during load_html (it knows
        // the real window height); only fall back to the engine default if the
        // document doesn't have a value yet.
        // Only use doc's viewport_h as fallback if the engine's hasn't been set.
        if self.viewport_h <= 0.0 && doc.viewport_h > 0.0 {
            self.viewport_h = doc.viewport_h;
        }
        // Keep viewport in doc so focus-change recascades use the correct size.
        doc.viewport_w = self.viewport_w;
        doc.viewport_h = self.viewport_h;
        // Compute root font-size from the <html> element's computed style.
        // Used for `rem` unit resolution throughout layout.
        // ⛔ The cascade resolves `<html>`'s own font-size, so its base must be
        // the INITIAL size — not a value derived from that same element. This
        // read `<html>`'s style first and fed the result back in as the base,
        // so `html { font-size: 62.5% }` — the standard "1rem = 10px" idiom —
        // resolved to 62.5% of 10 = 6.25px, and every later layout applied the
        // percentage again: 3.9, 2.4, down to the 1px floor. Every `rem` on the
        // page collapsed with it, and with them every line height and box.
        // The cascade publishes the resolved root size itself (see the `html`
        // branch in `cascade.rs`). A full cascade must start from the CSS
        // initial root size so `html { font-size: 62.5% }` resolves against
        // 16px, but an incremental hover cascade usually starts below `<html>`;
        // for that path, `rem` must use the root size the previous full cascade
        // already computed.
        let computed_root_font_px = match doc.root.style.font_size {
            crate::types::CssLength::Px(v) if v > 0.0 => v,
            _ => self.root_font_px,
        };
        let cascade_root_px = crate::types::ComputedStyle::INITIAL_FONT_SIZE_PX;
        let root_font_px = cascade_root_px;

        // Anonymous blocks are layout fragments, not DOM nodes. Progressive
        // rendering may create them before the final stylesheet has arrived;
        // if they remain in the tree during a later cascade, child combinators
        // such as `.toolbar > .button` stop matching.
        let hover_only = doc.hover_changed
            && !doc.style_dirty
            && self.last_cascade_vw == viewport_width
            && self.last_geometry_viewport_h == self.viewport_h
            && !doc.has_dirty_layout();
        let hover_geometry = if hover_only {
            HoverGeometrySnapshot::capture(&doc.root)
        } else {
            None
        };
        if !hover_only {
            crate::layout::block::unwrap_all_anonymous_blocks(&mut doc.root);
        }

        // Rebuild selector index if rules changed (lazy, skips if already up-to-date).
        doc.stylesheet
            .set_layer_viewport(self.viewport_w, self.viewport_h);
        doc.stylesheet.rebuild_index();

        // Cache @media / @container presence so we don't O(n)-scan rules every layout.
        self.cached_has_media_q = doc
            .stylesheet
            .rules
            .iter()
            .any(|r| !r.media_condition.is_empty());
        self.cached_has_container_q = doc
            .stylesheet
            .rules
            .iter()
            .any(|r| !r.container_condition.is_empty());

        // Skip the CSS cascade on resize when nothing media-query-relevant changed.
        let needs_cascade = self.last_cascade_vw.is_nan()
            || (self.cached_has_media_q
                && doc.stylesheet.rules.iter().any(|r| {
                    !r.media_condition.is_empty()
                        && r.media_condition
                            .matches(self.last_cascade_vw, self.viewport_h)
                            != r.media_condition.matches(viewport_width, self.viewport_h)
                }));

        let hover_changed = doc.hover_changed;
        doc.hover_changed = false;
        let dom_style_dirty = doc.style_dirty;
        doc.style_dirty = false;

        let cascade_profile_start = crate::profile::is_enabled().then(std::time::Instant::now);
        perf::start_phase();
        let did_cascade = if needs_cascade || dom_style_dirty {
            // Full cascade needed (initial, viewport change, etc.)
            let hover_chain = crate::css::build_hover_chain(&doc.root, doc.hovered_box);
            let target_id = doc.fragment_target_id();
            crate::css::apply_cascade_vp_hover_target_url(
                &mut doc.root,
                &doc.stylesheet,
                None,
                root_font_px,
                self.viewport_w,
                self.viewport_h,
                doc.focused_box,
                doc.keyboard_focus,
                &hover_chain,
                target_id,
                &doc.base_url,
            );
            // Clear any leftover dirty flags after full cascade
            crate::css::clear_cascade_dirty(&mut doc.root);
            self.last_cascade_vw = viewport_width;
            doc.prev_hovered_box = doc.hovered_box;
            // Build hover invalidation set: collect node_ids of all elements with hover_style
            doc.hover_sensitive_nodes.clear();
            collect_hover_sensitive(&doc.root, &mut doc.hover_sensitive_nodes);
            true
        } else if hover_changed {
            // Incremental hover cascade — only re-cascade elements affected by
            // the hover change (old chain + new chain), skip everything else.
            let old_chain = crate::css::build_hover_chain(&doc.root, doc.prev_hovered_box);
            let new_chain = crate::css::build_hover_chain(&doc.root, doc.hovered_box);
            // Mark dirty flags on nodes affected by hover change
            // ⛔ The stylesheet's own flag, not a hardcoded `false`. It is set
            // when a rule styles a DESCENDANT of a hovered element — `li:hover
            // div`, which is how essentially every dropdown menu is built.
            // Passing `false` meant those descendants were never marked dirty,
            // so the incremental hover cascade skipped them and the panel never
            // opened. Hovering the element itself worked, which is why simple
            // `a:hover` colour changes looked fine while no menu did.
            let mark_dirty_started = crate::profile::is_enabled().then(std::time::Instant::now);
            crate::css::mark_hover_dirty(
                &mut doc.root,
                &doc.stylesheet,
                &old_chain,
                &new_chain,
                doc.stylesheet.has_hover_descendant_rules,
                &doc.hover_sensitive_nodes,
            );
            if let Some(started) = mark_dirty_started {
                crate::profile::record(crate::profile::Phase::CascadeMarkDirty, started.elapsed());
            }

            crate::css::apply_cascade_incremental(
                &mut doc.root,
                &doc.stylesheet,
                None,
                computed_root_font_px,
                self.viewport_w,
                self.viewport_h,
                doc.focused_box,
                doc.keyboard_focus,
                &new_chain,
            );
            crate::css::clear_cascade_dirty(&mut doc.root);
            doc.hover_suppress_count = 1;
            doc.prev_hovered_box = doc.hovered_box;
            true
        } else {
            false
        };

        // `rem` in LAYOUT resolves against what the root actually computed to,
        // which the cascade has now written as an absolute length.
        let root_font_px = match doc.root.style.font_size {
            crate::types::CssLength::Px(v) if v > 0.0 => v,
            _ => computed_root_font_px,
        };
        self.root_font_px = root_font_px;

        // Match faces against computed families, after cascade but before text layout.
        // Remote loads remain async; unused faces never enter the fetch queue.
        if !doc.stylesheet.font_faces.is_empty()
            && (did_cascade
                || hover_changed
                || self.cached_font_families.is_none()
                || doc.has_dirty_layout())
        {
            self.cached_font_families = Some(referenced_font_face_families(&doc.root));
            let families = self
                .cached_font_families
                .as_ref()
                .expect("font families collected");
            let candidates: Vec<_> = doc
                .stylesheet
                .font_faces
                .iter()
                .filter(|face| {
                    css_font_family_name(&face.family)
                        .is_some_and(|name| families.contains(&name.to_ascii_lowercase()))
                })
                .collect();
            if !candidates.is_empty() {
                let ranged_families: HashSet<String> = candidates
                    .iter()
                    .filter(|face| {
                        face.unicode_range
                            .as_ref()
                            .is_some_and(|range| !range.trim().is_empty())
                    })
                    .filter_map(|face| css_font_family_name(&face.family))
                    .map(|name| name.to_ascii_lowercase())
                    .collect();
                let mut text_by_family = HashMap::new();
                let mut conservative_families = HashSet::new();
                if !ranged_families.is_empty() {
                    let mut family_lists = HashMap::new();
                    collect_font_face_text_by_family(
                        &doc.root,
                        &ranged_families,
                        &mut text_by_family,
                        &mut conservative_families,
                        &mut family_lists,
                    );
                }
                let coverage_by_family: HashMap<_, _> = text_by_family
                    .into_iter()
                    .map(|(family, text)| {
                        (
                            family,
                            crate::css::font_face::UnicodeTextCoverage::new(&text),
                        )
                    })
                    .collect();
                let faces: Vec<_> = candidates
                    .into_iter()
                    .filter(|face| {
                        let Some(name) = css_font_family_name(&face.family) else {
                            return false;
                        };
                        let name = name.to_ascii_lowercase();
                        conservative_families.contains(&name)
                            || coverage_by_family.get(&name).is_none_or(|coverage| {
                                coverage.intersects(face.unicode_range.as_deref())
                            })
                    })
                    .cloned()
                    .collect();
                self.load_font_faces(&faces, &doc.base_url, "");
            }
        }

        let hover_reuses_geometry = hover_geometry
            .as_ref()
            .is_some_and(|snapshot| snapshot.can_reuse(&doc.root));

        // ── CSS animation / transition runtime ─────────────────────────────
        let animation_sync_started = crate::profile::is_enabled().then(std::time::Instant::now);
        let now = std::time::Instant::now();
        doc.sync_animations(now);
        if did_cascade || hover_changed {
            let transition_sync_started =
                crate::profile::is_enabled().then(std::time::Instant::now);
            doc.sync_transitions(now);
            if let Some(started) = transition_sync_started {
                crate::profile::record(crate::profile::Phase::TransitionSync, started.elapsed());
            }
        }
        doc.tick_animations(now);
        let svg_animations_running = crate::svg::tick_svg_animations(&mut doc.root, now);
        if svg_animations_running {
            doc.needs_animation_frame = true;
        }
        doc.tick_media(now);
        doc.tick_smooth_scrolls(now);
        let mut animation_restore = Vec::new();
        if !doc.animation_overrides.is_empty() {
            let overrides = doc.animation_overrides.clone();
            animation_restore =
                crate::css::apply_animation_overrides_scoped(&mut doc.root, &overrides);
        }
        if let Some(started) = animation_sync_started {
            crate::profile::record(crate::profile::Phase::AnimationSync, started.elapsed());
        }
        // ──────────────────────────────────────────────────────────────────

        // Progressive layout is disabled for now — it causes blank content
        // below the viewport. Full layout runs in a single pass.
        // TODO: implement proper deferred layout with background completion.
        perf::end_cascade();
        if let Some(started) = cascade_profile_start {
            crate::profile::record(crate::profile::Phase::Cascade, started.elapsed());
        }
        let cascade_end = std::time::Instant::now();
        self.initial_layout_done = true;
        if hover_reuses_geometry
            && crate::types::animation_runtime::layout_animation_values(&doc.animation_overrides)
                .is_empty()
        {
            crate::css::restore_animation_overrides(&mut doc.root, animation_restore);
            crate::css::clear_descendant_dirty(&mut doc.root);
            clear_layout_dirty_flags(&mut doc.root);
            doc.rebuild_node_index();
            // Geometry is stable, but stacking/shadows/backgrounds changed.
            doc.layout_generation = doc.layout_generation.wrapping_add(1);
            return;
        }
        let geometry_profile_start = crate::profile::is_enabled().then(std::time::Instant::now);
        self.layout_geometry(doc, viewport_width, root_font_px);
        self.last_geometry_viewport_h = self.viewport_h;

        // Container query post-pass: now that box sizes are known, apply @container rules
        // whose conditions match the computed dimensions of container ancestors, then
        // re-layout until dependent container sizes settle.
        if self.cached_has_container_q {
            let container_started = crate::profile::is_enabled().then(std::time::Instant::now);
            let mut applied_rules = std::collections::HashMap::new();
            for _ in 0..4 {
                let changed = crate::css::apply_container_cascade_tree_with_state(
                    &mut doc.root,
                    &doc.stylesheet,
                    &[],
                    &[],
                    0,
                    1,
                    0,
                    1,
                    root_font_px,
                    self.viewport_w,
                    self.viewport_h,
                    doc.focused_box,
                    doc.keyboard_focus,
                    &mut applied_rules,
                );
                if !changed {
                    break;
                }
                // Re-apply animation overrides after container-query cascade.
                if !doc.animation_overrides.is_empty() {
                    if !animation_restore.is_empty() {
                        crate::css::restore_animation_overrides(
                            &mut doc.root,
                            std::mem::take(&mut animation_restore),
                        );
                    }
                    let overrides = doc.animation_overrides.clone();
                    animation_restore =
                        crate::css::apply_animation_overrides_scoped(&mut doc.root, &overrides);
                }
                self.layout_geometry(doc, viewport_width, root_font_px);
                self.last_geometry_viewport_h = self.viewport_h;
            }
            if let Some(started) = container_started {
                crate::profile::record(
                    crate::profile::Phase::GeometryContainerQueries,
                    started.elapsed(),
                );
            }
        }
        if !animation_restore.is_empty() {
            crate::css::restore_animation_overrides(&mut doc.root, animation_restore);
        }
        if let Some(anchor) = scroll_anchor {
            anchor.adjust(doc, self.viewport_h);
        }
        element_anchors.adjust(doc);
        if let Some(started) = geometry_profile_start {
            crate::profile::record(crate::profile::Phase::Geometry, started.elapsed());
        }

        // Detect aria-live region changes and queue announcements.
        doc.check_live_regions();
        if std::env::var_os("WEBCORE_TRACE_IDLE").is_some() {
            eprintln!(
                "[webcore layout] rules={} cascade={}ms geometry={}ms container_queries={}",
                doc.stylesheet.rules.len(),
                cascade_end.duration_since(trace_start).as_millis(),
                cascade_end.elapsed().as_millis(),
                self.cached_has_container_q,
            );
        }
    }

    /// Force the next `layout()` call to re-run the full CSS cascade.
    ///
    /// Call this after DOM mutations (adding/removing elements, changing classes or
    /// inline styles) so the skip-cascade optimisation does not hide the change.
    pub fn invalidate_cascade(&mut self) {
        self.last_cascade_vw = f32::NAN;
        self.cached_font_families = None;
    }

    /// Layout without re-running the CSS cascade.
    ///
    /// Use this when only text content changed (e.g. keystrokes in an editable
    /// element).  Skipping the cascade saves CSS selector matching across every
    /// element in the tree; the line-cache early-stop then skips unchanged lines.
    /// Progressive layout: lay out only above-fold content first, return true
    /// if there's more to do below the fold. Caller should render, then call
    /// `layout_remainder()` to finish.
    pub fn layout_above_fold(&mut self, doc: &mut Document, viewport_width: f32) -> bool {
        self.progressive_cutoff = self.viewport_h * 1.5;
        self.layout_geometry(doc, viewport_width, self.root_font_px);
        self.progressive_cutoff = 0.0;
        // Check if document is taller than what we laid out
        doc.root.layout.margin_rect.h > self.viewport_h * 1.5
    }

    /// Finish layout for below-fold content (call after rendering first screen).
    pub fn layout_remainder(&mut self, doc: &mut Document, viewport_width: f32) {
        // Mark everything below the fold as dirty so it gets laid out
        self.progressive_cutoff = 0.0;
        doc.root.layout.layout_dirty = true;
        self.layout_geometry(doc, viewport_width, self.root_font_px);
        self.last_geometry_viewport_h = self.viewport_h;
    }

    pub fn layout_no_cascade(&mut self, doc: &mut Document, viewport_width: f32) {
        let scroll_anchor =
            scroll_anchor::ViewportAnchor::capture(doc, viewport_width, self.viewport_h);
        let element_anchors = scroll_anchor::ElementAnchors::capture(doc);
        self.viewport_w = viewport_width;
        let root_font_px = self.root_font_px;
        self.layout_geometry(doc, viewport_width, root_font_px);
        if let Some(anchor) = scroll_anchor {
            anchor.adjust(doc, self.viewport_h);
        }
        element_anchors.adjust(doc);
        self.last_geometry_viewport_h = self.viewport_h;
    }

    fn layout_geometry(&self, doc: &mut Document, viewport_width: f32, root_font_px: f32) {
        self.layout_calls.set(0);
        self.layout_start.set(Some(std::time::Instant::now()));
        perf::reset();
        perf::start_phase();

        // Resolve shadow DOM slots before layout (only if any shadow roots exist)
        if has_shadow_roots(&doc.root) {
            resolve_all_slots(&mut doc.root, viewport_width, self.viewport_h);
        }

        // Layout runs on the render tree in place. The structural corrections
        // a separate fragment tree would exist to make — anonymous blocks for
        // mixed block/inline children, `display: contents`, `::before`/
        // `::after`, flex/grid blockification — are all made here, and all
        // four were checked against Chrome and agree.
        propagate_dirty(&mut doc.root);

        // Set up root geometry
        let rbox = self.res_box(&doc.root.style, root_font_px, viewport_width, root_font_px);
        let content_w = rbox.content_width.unwrap_or(viewport_width);
        doc.root.layout.content_rect.w = content_w;
        doc.root.layout.padding_rect.w = content_w;
        doc.root.layout.border_rect.w = content_w;
        doc.root.layout.margin_rect.w = content_w;
        // Always mark root dirty — layout_geometry is only called when
        // cascade ran or layout is explicitly requested. The incremental
        // optimization happens INSIDE layout_box via subtree pruning.
        doc.root.layout.layout_dirty = true;

        // When viewport height changed, mark all nodes dirty
        if (self.viewport_h - self.last_geometry_viewport_h).abs() > 0.5
            && !self.last_geometry_viewport_h.is_nan()
        {
            fn mark_all_dirty(n: &mut crate::types::WebCore) {
                n.layout.layout_dirty = true;
                n.layout.intrinsic_dirty = true;
                n.has_dirty_layout_descendant = true;
                for c in &mut n.children {
                    mark_all_dirty(c);
                }
            }
            mark_all_dirty(&mut doc.root);
        }

        self.pos_cb
            .set(Rect::new(0.0, 0.0, content_w, self.viewport_h));
        self.fixed_cb
            .set(Rect::new(0.0, 0.0, content_w, self.viewport_h));
        // **The root's containing block is the VIEWPORT, height included.**
        // CSS 2.1 §10.1: the initial containing block has the viewport's
        // dimensions. Passing only the width made `html { height: 100% }`
        // resolve to `auto`, and since a percentage height is auto whenever its
        // containing block's is, the whole chain below it collapsed — which is
        // every app shell ever written.
        let root_c = if self.viewport_h > 0.0 {
            Constraints::with_height(
                content_w,
                self.viewport_h,
                0.0,
                0.0,
                root_font_px,
                root_font_px,
            )
        } else {
            Constraints::new(content_w, 0.0, 0.0, root_font_px, root_font_px)
        };
        let boxes_started = crate::profile::is_enabled().then(std::time::Instant::now);
        self.layout_box(&mut doc.root, &root_c);
        if let Some(started) = boxes_started {
            crate::profile::record(crate::profile::Phase::GeometryBoxes, started.elapsed());
        }
        let finalize_started = crate::profile::is_enabled().then(std::time::Instant::now);
        clear_layout_inert_svg_subtrees(&mut doc.root, false);
        clear_display_contents_boxes(&mut doc.root);

        // Update root geometry with the final scroll extent. Modern pages
        // commonly have positioned or late-sized descendants that extend past
        // the root's normal-flow height; browser-visible document height must
        // reflect the scrollable content, not only the root block's own box.
        let h = crate::types::Document::scroll_height(&doc.root).max(self.viewport_h);
        doc.root.layout.content_rect.h = h;
        doc.root.layout.padding_rect.h = h;
        doc.root.layout.border_rect.h = h;
        doc.root.layout.margin_rect.h = h;

        perf::end_layout();
        perf::flush_hot_nodes();
        perf::set_counts(
            count_nodes(&doc.root) as u32,
            doc.stylesheet.rules.len() as u32,
        );

        // Clear descendant dirty flags now that layout is complete
        crate::css::clear_descendant_dirty(&mut doc.root);
        clear_layout_dirty_flags(&mut doc.root);

        // Rebuild O(1) node index (pointers stable until next mutation)
        doc.rebuild_node_index();
        if let Some(started) = finalize_started {
            crate::profile::record(crate::profile::Phase::GeometryFinalize, started.elapsed());
        }

        // Bump generation so renderer knows to rebuild display list.
        //
        // ⚠ UNCONDITIONAL, and that costs a full display-list rebuild on every
        // frame a host repaints — which for a window on a 60Hz tick is every
        // frame, however still the page is. Gating it needs the signals
        // `layout()` has (`did_cascade`, the previous viewport) and this
        // function is `&self` and does not, so the fix is a real change to who
        // owns that decision rather than a condition bolted on here.
        doc.layout_generation = doc.layout_generation.wrapping_add(1);
    }

    pub fn layout_box(&self, node: &mut WebCore, c: &Constraints) -> f32 {
        self.layout_box_with_fc(node, c, None)
    }

    pub fn layout_box_with_fc(
        &self,
        node: &mut WebCore,
        c: &Constraints,
        fc: Option<&mut FloatContext>,
    ) -> f32 {
        // Inline fragments belong to a previous inline formatting context, not
        // to the border box of an atomic or blockified flex/grid item.
        if node.style.display != Display::Inline {
            node.layout.inline_client_rects.clear();
        }
        let containing_w = c.available_width;
        let x = c.x;
        let y = c.y;
        let parent_font_px = c.parent_font_px;
        let root_font_px = c.root_font_px;
        // Guard against infinite layout loops.
        let calls = self.layout_calls.get();
        self.layout_calls.set(calls + 1);
        if calls > 5_000_000 {
            eprintln!("  [layout] ABORTING: >5M layout calls — infinite loop detected");
            node.layout.content_rect = Rect::new(x, y, containing_w, 0.0);
            node.layout.padding_rect = node.layout.content_rect;
            node.layout.border_rect = node.layout.content_rect;
            node.layout.margin_rect = node.layout.content_rect;
            return 0.0;
        }
        // Guard against stack overflow on deeply nested DOMs.
        let depth = self.layout_depth.get();
        if depth >= MAX_LAYOUT_DEPTH {
            node.layout.content_rect = Rect::new(x, y, containing_w, 0.0);
            node.layout.padding_rect = node.layout.content_rect;
            node.layout.border_rect = node.layout.content_rect;
            node.layout.margin_rect = node.layout.content_rect;
            return 0.0;
        }
        // Don't layout display:none
        if matches!(node.style.display, Display::None) {
            clear_layout_subtree(node);
            return 0.0;
        }

        if node.is_text_node()
            && node.text.chars().all(|ch| ch.is_ascii_whitespace())
            && matches!(
                node.style.white_space,
                WhiteSpace::Normal | WhiteSpace::Nowrap
            )
        {
            clear_layout_subtree(node);
            node.layout.layout_dirty = false;
            node.layout.intrinsic_dirty = false;
            node.layout.paint_dirty = false;
            node.has_dirty_layout_descendant = false;
            node.layout.last_containing_width = containing_w;
            node.layout.last_containing_height = c.available_height;
            return 0.0;
        }

        if is_layout_inert_svg_node(node) {
            node.layout.content_rect = Rect::default();
            node.layout.padding_rect = Rect::default();
            node.layout.border_rect = Rect::default();
            node.layout.margin_rect = Rect::default();
            return 0.0;
        }

        // display:contents — the element itself generates no box.
        // Its children are promoted to the parent's formatting context.
        if matches!(node.style.display, Display::Contents) {
            node.layout.content_rect = Rect::default();
            node.layout.padding_rect = Rect::default();
            node.layout.border_rect = Rect::default();
            node.layout.margin_rect = Rect::default();
            return 0.0;
        }

        // Fast path: skip full layout if the containing width hasn't changed
        // and the node isn't dirty. Just reposition the cached result.
        // Also skip when viewport height changed (vh-dependent elements need re-layout).
        // Note: during layout_geometry, last_geometry_viewport_h still holds the OLD value.
        let vh_ok = (self.viewport_h - self.last_geometry_viewport_h).abs() < 0.5
            || self.last_geometry_viewport_h.is_nan();
        if !node.layout.layout_dirty
            && node.layout.last_containing_width > 0.0
            && (node.layout.last_containing_width - containing_w).abs() < 0.01
            && same_containing_height(node.layout.last_containing_height, c.available_height)
            && node.layout.margin_rect.w > 0.0
            && node.layout.margin_rect.h > 0.0
            && fc.is_none()
            && !c.force_independent_formatting_context
            && vh_ok
            && !matches!(node.style.display, Display::None | Display::Contents)
        {
            perf::record_layout_skip();
            let dx = x - node.layout.margin_rect.x;
            let dy = y - node.layout.margin_rect.y;
            if dx.abs() > 0.01 || dy.abs() > 0.01 {
                shift_rects(node, dx, dy);
            }
            self.layout_depth.set(depth);
            return node.layout.margin_rect.h;
        }

        self.layout_depth.set(depth + 1);
        let _profile_box = perf::node_span("layout-box", node.node_id);
        let _query_writing_mode_scope = self.enter_query_writing_mode(&node.style);

        perf::record_layout_call();
        perf::record_depth(depth as u32);
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);

        // <img>/<svg> aspect ratio: when one dimension is auto and the natural
        // dimensions are known, compute the auto dimension to preserve the
        // image's intrinsic aspect ratio (CSS Images §5.1).
        // For <svg>, intrinsic dimensions come from viewBox.
        let (has_intrinsic, iw, ih) = match self.intrinsic_dimensions(node) {
            Some((w, h)) => (true, w, h),
            None => (false, 0.0, 0.0),
        };
        // Compute intrinsic aspect ratio dimensions WITHOUT mutating style.
        // The resolved values are applied to rbox after resolve_box_vp.
        let (intrinsic_w_override, intrinsic_h_override) = if has_intrinsic {
            if node.style.width.is_auto() && !node.style.height.is_auto() {
                let containing_h = c.forced_height.or(c.available_height).unwrap_or(0.0);
                let mut h = self.res_len(&node.style.height, font_px, containing_h, root_font_px);
                let h_override = if h <= 0.0 && node.style.height.has_percentage() && ih > 0.0 {
                    h = ih;
                    Some(ih)
                } else {
                    None
                };
                let w = if ih > 0.0 { (h * iw / ih).round() } else { iw };
                (Some(w), h_override)
            } else if node.style.height.is_auto() && !node.style.width.is_auto() {
                let mut w = self.res_len(&node.style.width, font_px, containing_w, root_font_px);
                let max_w =
                    self.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
                if max_w > 0.0 && w > max_w {
                    w = max_w;
                }
                let h = (w * ih / iw).round();
                (None, Some(h))
            } else if node.style.width.is_auto() && node.style.height.is_auto() {
                let (mut w, mut h) = if node.tag == "svg" {
                    let default_w = 300.0;
                    let default_h = 150.0;
                    if containing_w > 0.0 && containing_w < default_w {
                        (containing_w, (containing_w * ih / iw).round())
                    } else {
                        (default_w, default_h)
                    }
                } else {
                    (iw, ih)
                };
                let max_w =
                    self.res_len(&node.style.max_width, font_px, containing_w, root_font_px);
                if max_w > 0.0 && w > max_w {
                    h = (max_w * ih / iw).round();
                    w = max_w;
                }
                let max_h = self.res_len(&node.style.max_height, font_px, 0.0, root_font_px);
                if max_h > 0.0 && h > max_h {
                    w = (max_h * iw / ih).round();
                    h = max_h;
                }
                (Some(w), Some(h))
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let mut rbox = resolve_box_vp_query(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            self.viewport_w,
            self.viewport_h,
            c.available_height,
            self.query_container_sizes.get(),
        );
        if node.style.display == Display::Inline
            && has_block_children(node)
            && !node.style.width.is_auto()
        {
            let mut w = self
                .res_len(&node.style.width, font_px, containing_w, root_font_px)
                .max(0.0);
            if node.style.box_sizing == BoxSizing::BorderBox {
                w = (w
                    - rbox.padding_left
                    - rbox.padding_right
                    - rbox.border_left
                    - rbox.border_right)
                    .max(0.0);
            }
            rbox.content_width = Some(w);
        }
        if node.style.display == Display::Inline && is_inline_replaced_or_native_control(node) {
            if !node.style.width.is_auto() {
                let mut w = self
                    .res_len(&node.style.width, font_px, containing_w, root_font_px)
                    .max(0.0);
                if node.style.box_sizing == BoxSizing::BorderBox {
                    w = (w
                        - rbox.padding_left
                        - rbox.padding_right
                        - rbox.border_left
                        - rbox.border_right)
                        .max(0.0);
                }
                rbox.content_width = Some(w);
            }
            if !node.style.height.is_auto() {
                let basis_h = c.forced_height.or(c.available_height).unwrap_or(0.0);
                let mut h = self
                    .res_len(&node.style.height, font_px, basis_h, root_font_px)
                    .max(0.0);
                if node.style.box_sizing == BoxSizing::BorderBox {
                    h = (h
                        - rbox.padding_top
                        - rbox.padding_bottom
                        - rbox.border_top
                        - rbox.border_bottom)
                        .max(0.0);
                }
                rbox.content_height = Some(h);
            }
        }

        if let Some(w) = self.field_sizing_content_width(node, font_px) {
            rbox.content_width = Some(w);
        }

        // Apply intrinsic aspect ratio overrides to rbox (not to style)
        if let Some(w) = intrinsic_w_override {
            rbox.content_width = Some(w);
        }
        if let Some(h) = intrinsic_h_override {
            rbox.content_height = Some(h);
        }

        if node.tag == "select" && node.style.width.is_auto() {
            rbox.content_width =
                self.text_control_intrinsic_content_width(node, font_px, root_font_px);
        }

        // HTML button layout uses fit-content for an automatic inline size,
        // including a block-level flex/grid button inside a narrow parent.
        if node.tag == "button"
            && node.style.width.is_auto()
            && node.style.writing_mode == WritingMode::HorizontalTB
            && c.forced_width.is_none()
        {
            let available = (containing_w
                - rbox.margin_left
                - rbox.margin_right
                - rbox.padding_left
                - rbox.padding_right
                - rbox.border_left
                - rbox.border_right)
                .max(0.0);
            rbox.content_width = Some(self.intrinsic_width(
                &CssLength::FitContent,
                node,
                available,
                font_px,
                root_font_px,
                containing_w,
            ));
        }

        self.clamp_resolved_content_width(&mut rbox, node, containing_w, font_px, root_font_px);

        // A percentage height in an auto-height containing block computes to
        // auto. Replaced elements still use their intrinsic ratio in that case,
        // including when CSS gives the image a definite percentage width.
        if has_intrinsic
            && ih > 0.0
            && node.style.height.has_percentage()
            && c.available_height.is_none()
            && (rbox.content_height.is_none() || intrinsic_h_override.is_some())
        {
            rbox.content_height = Some(rbox.content_width.unwrap_or(iw) * ih / iw);
        }

        // Apply forced dimensions from Constraints (used by flex layout).
        // These override style.width/height without mutating the DOM.
        if let Some(fw) = c.forced_width {
            rbox.content_width = Some(fw);
        }
        if let Some(fh) = c.forced_height {
            rbox.content_height = Some(fh);
        }

        if node.style.content_visibility == ContentVisibility::Hidden {
            let remembered = node.layout.last_uncontained_content_size.as_deref();
            let content_w = rbox.content_width.unwrap_or_else(|| {
                if matches!(
                    node.style.display,
                    Display::InlineBlock | Display::InlineFlex | Display::InlineGrid
                ) {
                    remembered
                        .filter(|_| node.style.rare().contain_intrinsic_width_auto)
                        .map(|size| size.0)
                        .unwrap_or_else(|| {
                            self.contained_intrinsic_width(&node.style, font_px, root_font_px)
                        })
                } else {
                    (containing_w - rbox.h_space()).max(0.0)
                }
            });
            let content_h = rbox.content_height.unwrap_or_else(|| {
                remembered
                    .filter(|_| node.style.rare().contain_intrinsic_height_auto)
                    .map(|size| size.1)
                    .unwrap_or_else(|| {
                        self.contained_intrinsic_height(&node.style, font_px, root_font_px)
                    })
            });
            node.layout.line_cache.clear();
            node.layout.inline_runs.clear();
            block::build_box_rects(
                node,
                &rbox,
                x + rbox.margin_left + rbox.border_left + rbox.padding_left,
                y + rbox.margin_top + rbox.border_top + rbox.padding_top,
                content_w,
                content_h,
                rbox.margin_left,
                rbox.margin_right,
            );
            node.layout.baseline = node.layout.content_rect.bottom();
            node.layout.scroll_width = content_w;
            node.layout.scroll_height = content_h;
            node.layout.scroll_left = 0.0;
            node.layout.scroll_top = 0.0;
            node.layout.layout_dirty = false;
            node.layout.intrinsic_dirty = false;
            node.layout.last_containing_width = containing_w;
            node.layout.last_containing_height = c.available_height;
            self.layout_depth.set(depth);
            return node.layout.margin_rect.h;
        }

        // CSS 2.1 §10.5: when this element has a definite content height,
        // children with percentage heights need to resolve against it.
        // Instead of mutating child.style.height, we'll pass the parent's
        // content height via Constraints.available_height when laying out children.
        // This is handled by the layout functions (block, flex, grid) that
        // create child Constraints with available_height set.

        // Auto-margin centering (CSS 2.1 §10.3.3) — applies to block-level elements
        // in normal flow. For inline-level elements (inline-block, inline-flex, etc.),
        // auto margins evaluate to 0 (CSS 2.1 §10.3.10).
        let is_block_level = node.style.is_block_level()
            && !matches!(
                node.style.display,
                Display::InlineBlock | Display::Inline | Display::InlineFlex | Display::InlineGrid
            );
        if is_block_level {
            if let Some(content_w) = rbox.content_width {
                let left_auto = node.style.margin_left.is_auto();
                let right_auto = node.style.margin_right.is_auto();
                if left_auto || right_auto {
                    let non_margin = rbox.border_left
                        + rbox.padding_left
                        + content_w
                        + rbox.padding_right
                        + rbox.border_right;
                    let available = (containing_w - non_margin).max(0.0);
                    if left_auto && right_auto {
                        let ml = (available / 2.0).floor();
                        rbox.margin_left = ml;
                        rbox.margin_right = available - ml;
                    } else if left_auto {
                        rbox.margin_left = available - rbox.margin_right;
                    } else {
                        rbox.margin_right = available - rbox.margin_left;
                    }
                }
            }
        }

        if is_native_replaced_control(node) {
            clear_layout_children(node);
            node.layout.line_cache.clear();
            node.layout.inline_runs.clear();

            let fallback_w = match node.tag.as_str() {
                "input" => match node.attributes.get("type").map(|s| s.as_str()) {
                    Some("checkbox" | "radio") => font_px.max(13.0),
                    Some(kind @ ("submit" | "reset" | "button")) => {
                        let default_label = match kind {
                            "submit" => "Submit",
                            "reset" => "Reset",
                            _ => "",
                        };
                        let label = node
                            .attributes
                            .get("value")
                            .map(String::as_str)
                            .unwrap_or(default_label);
                        self.measure_text_cached_with_stretch(
                            label,
                            font_px,
                            node.style.font_weight,
                            node.style.font_style,
                            &node.style.font_family,
                            node.style.font_stretch,
                        )
                    }
                    _ => self
                        .text_control_intrinsic_content_width(node, font_px, root_font_px)
                        .unwrap_or(200.0),
                },
                "select" => self
                    .text_control_intrinsic_content_width(node, font_px, root_font_px)
                    .unwrap_or(0.0),
                "textarea" => self
                    .text_control_intrinsic_content_width(node, font_px, root_font_px)
                    .unwrap_or(0.0),
                "progress" | "meter" => 160.0,
                "video" | "audio" => self
                    .intrinsic_dimensions(node)
                    .map(|(width, _)| width)
                    .unwrap_or(300.0),
                _ => 0.0,
            };
            let fallback_h = match node.tag.as_str() {
                "input" => {
                    if matches!(
                        node.attributes.get("type").map(|s| s.as_str()),
                        Some("checkbox" | "radio")
                    ) {
                        font_px.max(13.0)
                    } else if crate::types::input_uses_minimum_normal_line_height(
                        node.attributes
                            .get("type")
                            .map(String::as_str)
                            .unwrap_or("text"),
                    ) {
                        let fonts = unsafe { self.font_system.map(|fonts| &mut *fonts) };
                        let (_, _, normal) =
                            inline_layout::font_metrics(fonts, &node.style.font_family, font_px);
                        if node.style.line_height.is_auto() {
                            normal
                        } else {
                            self.res_len(&node.style.line_height, font_px, font_px, root_font_px)
                                .max(normal)
                        }
                    } else {
                        (font_px * 1.5).ceil().max(20.0)
                    }
                }
                "select" => {
                    if crate::html::forms::is_list_box(node) {
                        let rows = if node.style.field_sizing.eq_ignore_ascii_case("content") {
                            crate::html::forms::option_ids(node).len().max(1) as f32
                        } else {
                            crate::html::forms::display_size(node).max(1) as f32
                        };
                        crate::html::forms::list_box_row_height(font_px) * rows
                            + 2.0 * crate::html::forms::LIST_BOX_PADDING
                    } else {
                        (font_px * 1.35).ceil().max(18.0)
                    }
                }
                "textarea" => {
                    crate::html::forms::textarea_character_height(node) as f32
                        * self.text_control_line_height(node, font_px, root_font_px)
                        + node.style.scrollbar_width_px()
                }
                "progress" | "meter" => 16.0,
                "video" | "audio" => self
                    .intrinsic_dimensions(node)
                    .map(|(_, height)| height)
                    .unwrap_or(150.0),
                _ => 0.0,
            };
            let final_w = rbox.content_width.unwrap_or(fallback_w).max(0.0);
            let content_height = if c.forced_height.is_none() {
                self.field_sizing_content_height(node, font_px, root_font_px, final_w)
                    .map(|height| {
                        let (minimum, maximum) = self.resolved_content_height_limits(
                            &node.style,
                            &rbox,
                            c.available_height,
                            font_px,
                            root_font_px,
                        );
                        height.clamp(minimum, maximum)
                    })
            } else {
                None
            };
            let final_h = content_height
                .unwrap_or_else(|| rbox.content_height.unwrap_or(fallback_h))
                .max(0.0);
            block::build_box_rects(
                node,
                &rbox,
                x + rbox.margin_left + rbox.border_left + rbox.padding_left,
                y + rbox.margin_top + rbox.border_top + rbox.padding_top,
                final_w,
                final_h,
                rbox.margin_left,
                rbox.margin_right,
            );
            // Replaced controls have no descendant margins to collapse with,
            // but their own margins still adjoin normal-flow block siblings.
            node.layout.collapsed_margin_top = rbox.margin_top;
            node.layout.collapsed_margin_bottom = rbox.margin_bottom;
            node.layout.scroll_width = final_w;
            node.layout.scroll_height = final_h;
            if node.tag == "select" && crate::html::forms::is_list_box(node) {
                node.layout.scroll_height = final_h.max(
                    crate::html::forms::option_ids(node).len() as f32
                        * crate::html::forms::list_box_row_height(font_px)
                        + 2.0 * crate::html::forms::LIST_BOX_PADDING,
                );
            } else if node.tag == "textarea" {
                let value = crate::types::input_value(node);
                let mut width = node.scrollport_content_width();
                let mut extent =
                    self.text_control_extent(node, &value, width, font_px, root_font_px);
                node.layout.scroll_height = final_h.max(extent.1);
                let with_gutter = node.scrollport_content_width();
                if with_gutter < width {
                    width = with_gutter;
                    extent = self.text_control_extent(node, &value, width, font_px, root_font_px);
                    node.layout.scroll_height = final_h.max(extent.1);
                }
                node.layout.scroll_width = width.max(extent.0);
            }
            if crate::html::forms::is_list_box(node) {
                node.layout.scroll_width = node.scrollport_content_width();
            }
            node.layout.scroll_left = node.layout.scroll_left.clamp(
                0.0,
                (node.layout.scroll_width - node.scrollport_content_width()).max(0.0),
            );
            node.layout.scroll_top = node
                .layout
                .scroll_top
                .clamp(0.0, (node.layout.scroll_height - final_h).max(0.0));
            node.layout.resolved_content_width = final_w;
            node.layout.layout_dirty = false;
            node.layout.intrinsic_dirty = false;
            node.layout.last_containing_width = containing_w;
            node.layout.last_containing_height = c.available_height;
            return node.layout.margin_rect.h;
        }

        // ── Layout subtree pruning ────────────────────────────────────────────
        // If this box's resolved content width is identical to the previous
        // layout AND nothing is dirty AND there is no incoming float context
        // (which could alter line widths), the entire subtree produces exactly
        // the same geometry as before.  We just shift the cached rects to the
        // new position without re-running any layout algorithm.
        //
        // This is the dominant win on resize for fixed-width components nested
        // inside a fluid viewport (grid cards, sidebar items, etc.) — their
        // content width never changes even when the viewport grows or shrinks.
        // Also disable pruning when the viewport height changed so that vh-units
        // (e.g. height: 100vh) and flex-stretch heights dependent on the viewport
        // are recalculated rather than returning stale cached geometry.
        let viewport_h_unchanged = self.viewport_h == self.last_geometry_viewport_h;
        if fc.is_none()
            && !c.force_independent_formatting_context
            && node.style.width.intrinsic().is_none()
            && !node.layout.layout_dirty
            && !node.has_dirty_descendant
            && !node.has_dirty_layout_descendant
            && node.layout.resolved_content_width > 0.0
            && same_containing_height(node.layout.last_containing_height, c.available_height)
            && viewport_h_unchanged
        {
            let new_content_w = if let Some(cw) = rbox.content_width {
                cw
            } else {
                let outer = rbox.margin_left
                    + rbox.border_left
                    + rbox.padding_left
                    + rbox.border_right
                    + rbox.padding_right
                    + rbox.margin_right;
                (containing_w - outer).max(0.0)
            };
            // Also check the explicit content height hasn't changed.
            // This catches flex-stretch re-layouts where the parent mutates
            // child.style.height before calling layout_box a second time.
            let height_ok = match rbox.content_height {
                None => true, // auto height is determined by children — safe
                Some(h) => (h - node.layout.content_rect.h).abs() < 0.5,
            };
            if (new_content_w - node.layout.resolved_content_width).abs() < 0.5 && height_ok {
                // Content size is unchanged — just move the subtree.
                let dx = (x + rbox.margin_left) - node.layout.border_rect.x;
                let dy = (y + rbox.margin_top) - node.layout.border_rect.y;
                if dx.abs() > 0.01 || dy.abs() > 0.01 {
                    shift_rects(node, dx, dy);
                }
                node.layout.layout_dirty = false;
                node.layout.last_containing_width = containing_w;
                node.layout.last_containing_height = c.available_height;
                return node.layout.margin_rect.h;
            }
        }

        // Check for custom component — treated as replaced element with cached dimensions.
        // Only re-measure when the node is dirty (attribute changed, explicit invalidation).
        let is_trait_component = self.component_registry.get_component(&node.tag).is_some();
        let is_legacy_component =
            !is_trait_component && self.component_registry.map.contains_key(&node.tag);
        if is_trait_component || is_legacy_component {
            // Measure only on first layout or when dirty — cache the result
            if node.component_width == 0.0 || node.layout.layout_dirty {
                let (cw, ch) = if is_trait_component {
                    self.component_registry
                        .get_component(&node.tag)
                        .unwrap()
                        .measure(node, containing_w)
                } else {
                    let cb = self.component_registry.map.get(&node.tag).unwrap();
                    (cb.measure)(node, containing_w)
                };
                node.component_width = cw;
                node.component_height = ch;
            }
            let cw = node.component_width;
            let ch = node.component_height;
            let final_w = if node.style.width.is_auto() {
                cw
            } else {
                rbox.content_width.unwrap_or(cw)
            };
            let final_h = if node.style.height.is_auto() {
                ch
            } else {
                rbox.content_height.unwrap_or(ch)
            };
            block::build_box_rects(
                node,
                &rbox,
                x + rbox.margin_left + rbox.border_left + rbox.padding_left,
                y + rbox.margin_top + rbox.border_top + rbox.padding_top,
                final_w,
                final_h,
                rbox.margin_left,
                rbox.margin_right,
            );
            node.layout.layout_dirty = false;
            return node.layout.margin_rect.h;
        }

        // Track the nearest positioned ancestor's padding rect for abs children.
        let old_pos_cb = self.pos_cb.get();
        let old_fixed_cb = self.fixed_cb.get();
        // CSS spec: positioned elements AND elements with transform/filter/will-change
        // create a containing block for absolute/fixed descendants.
        if establishes_positioned_containing_block(&node.style) {
            let est_padding_x = x + rbox.margin_left + rbox.border_left;
            let est_padding_y = y + rbox.margin_top + rbox.border_top;
            let est_content_w = rbox
                .content_width
                .unwrap_or((containing_w - rbox.h_space()).max(0.0));
            let est_padding_w = est_content_w + rbox.padding_left + rbox.padding_right;
            self.pos_cb.set(Rect::new(
                est_padding_x,
                est_padding_y,
                est_padding_w,
                self.viewport_h,
            ));
            if establishes_fixed_positioned_containing_block(&node.style) {
                self.fixed_cb.set(Rect::new(
                    est_padding_x,
                    est_padding_y,
                    est_padding_w,
                    self.viewport_h,
                ));
            }
        }

        // Shadow DOM: layout reads `effective_children()`, which answers the
        // shadow tree when there is one. There used to be a swap here that
        // moved the shadow children into `node.children` "so all existing
        // layout code works unchanged" — but it emptied `shadow_root.children`
        // for the duration, and `effective_children()` reads exactly that. So
        // every caller of the accessor (the formatting-context dispatch,
        // `has_block_children`, block, grid) saw an EMPTY child list and laid
        // out nothing, while flex — which read `node.children` directly —
        // worked. Shadow DOM rendered nothing, and which paths were affected
        // depended on which accessor each happened to use.

        // Replaced elements (input, select, textarea, img) cannot be flex/grid
        // containers per CSS spec — blockify for dispatch WITHOUT mutating style.
        let is_button_input = node.tag == "input"
            && matches!(
                node.attributes.get("type").map(|s| s.as_str()),
                Some("submit") | Some("button") | Some("reset")
            );
        let effective_display = if !is_button_input
            && matches!(
                node.tag.as_str(),
                "input" | "select" | "textarea" | "img" | "video" | "canvas" | "iframe"
            ) {
            match node.style.display {
                Display::Flex | Display::Grid => Display::Block,
                Display::InlineFlex | Display::InlineGrid => Display::InlineBlock,
                other => other,
            }
        } else {
            node.style.display
        };

        // Build child constraints from resolved font size
        let mut child_c = Constraints::new(containing_w, x, y, font_px, root_font_px);
        child_c.force_independent_formatting_context = c.force_independent_formatting_context;
        child_c.available_height = c.available_height;

        let h = match effective_display {
            Display::Flex | Display::InlineFlex => flex::layout_flex(self, node, &rbox, &child_c),
            Display::Grid | Display::InlineGrid => grid::layout_grid(self, node, &rbox, &child_c),
            Display::Table => {
                let mut table_rbox = rbox;
                // In CSS 2.1 §17.5, padding does not apply to tables.
                table_rbox.padding_top = 0.0;
                table_rbox.padding_right = 0.0;
                table_rbox.padding_bottom = 0.0;
                table_rbox.padding_left = 0.0;
                // Handle margin:auto centering for tables
                let mut table_c = if !node.style.width.is_auto()
                    && (node.style.margin_left.is_auto() || node.style.margin_right.is_auto())
                {
                    let tw = self.res_len(&node.style.width, font_px, containing_w, root_font_px);
                    let non_margin = table_rbox.border_left + tw + table_rbox.border_right;
                    let available = (containing_w - non_margin).max(0.0);
                    let (ml, _mr) =
                        if node.style.margin_left.is_auto() && node.style.margin_right.is_auto() {
                            let ml = (available / 2.0).floor();
                            (ml, available - ml)
                        } else if node.style.margin_left.is_auto() {
                            (available - table_rbox.margin_right, table_rbox.margin_right)
                        } else {
                            (table_rbox.margin_left, available - table_rbox.margin_left)
                        };
                    Constraints::new(
                        containing_w,
                        x + ml - table_rbox.margin_left,
                        y,
                        font_px,
                        root_font_px,
                    )
                } else {
                    child_c
                };
                table_c.forced_width = c.forced_width;
                table_c.forced_height = c.forced_height;
                table_c.available_height = c.available_height;
                table::layout_table(self, node, &table_rbox, &table_c)
            }
            _ => {
                table::wrap_orphan_table_boxes_in_anonymous_tables(node);

                // Determine if children are block-level or inline-level.
                // Also use block layout when ALL non-abs, non-hidden children are
                // floated (no inline content to lay out). When floats and inline
                // content coexist, inline layout handles them via Float items.
                let children = node.effective_children();
                let has_any_inline = has_in_flow_inline_children(node);
                let has_only_floats = !has_any_inline
                    && children.iter().any(|c| {
                        !matches!(c.style.display, Display::None)
                            && !matches!(c.style.position, Position::Absolute | Position::Fixed)
                            && !matches!(c.style.float, Float::None)
                    });
                let empty_block_level_box = !matches!(
                    effective_display,
                    Display::Inline
                        | Display::InlineBlock
                        | Display::InlineFlex
                        | Display::InlineGrid
                ) && node.text.is_empty()
                    && node.style.before_content.is_empty()
                    && node.style.after_content.is_empty()
                    && children.iter().all(|c| {
                        matches!(c.style.display, Display::None)
                            || matches!(c.style.position, Position::Absolute | Position::Fixed)
                            || (c.tag == "#text"
                                && c.text.chars().all(|ch| ch.is_ascii_whitespace()))
                    });
                if matches!(effective_display, Display::FlowRoot)
                    || block::establishes_column_context(&node.style)
                    || has_block_children(node)
                    || has_only_floats
                    || empty_block_level_box
                {
                    block::layout_block_with_fc(self, node, &rbox, &child_c, fc)
                } else {
                    // Pass parent float context so inline content wraps around
                    // floats from ancestor block containers (CSS §9.5).
                    inline_layout::layout_inline_block(self, node, &rbox, &child_c, fc)
                }
            }
        };

        if establishes_positioned_containing_block(&node.style) {
            relayout_nested_abs_descendants_against_final_cb(
                self,
                node,
                node.layout.padding_rect,
                font_px,
                root_font_px,
            );
        }

        self.pos_cb.set(old_pos_cb);
        self.fixed_cb.set(old_fixed_cb);
        self.layout_depth.set(depth);
        node.layout.layout_dirty = false;
        node.layout.intrinsic_dirty = false;
        node.layout.paint_dirty = false;
        node.has_dirty_layout_descendant = false;
        node.layout.last_containing_width = containing_w;
        node.layout.last_containing_height = c.available_height;
        if (node.style.rare().contain_intrinsic_width_auto
            || node.style.rare().contain_intrinsic_height_auto)
            && !node.style.contain_size
            && !node.style.contain_inline_size
            && node.style.container_type == ContainerType::Normal
            && node.style.content_visibility != ContentVisibility::Hidden
        {
            let size = (node.layout.content_rect.w, node.layout.content_rect.h);
            match &mut node.layout.last_uncontained_content_size {
                Some(remembered) => **remembered = size,
                slot @ None => *slot = Some(Box::new(size)),
            }
        } else if !node.style.rare().contain_intrinsic_width_auto
            && !node.style.rare().contain_intrinsic_height_auto
        {
            node.layout.last_uncontained_content_size = None;
        }
        h
    }

    /// Layout a box in inline context — returns (width, height, baseline).
    pub fn layout_inline(
        &self,
        node: &mut WebCore,
        max_w: f32,
        x: f32,
        y: f32,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> (f32, f32, f32) {
        let font_px = node.style.font_size_px(parent_font_px, root_font_px);
        let _rbox = self.res_box(&node.style, font_px, max_w, root_font_px);

        let h = self.layout_box(
            node,
            &Constraints::new(max_w, x, y, parent_font_px, root_font_px),
        );
        let w = node.layout.border_rect.w;
        let baseline = node.layout.baseline;
        (w, h, baseline)
    }
}

// ─── Helper: does a box have any block-level children? ────────────────────────

/// Quick check if any node in the tree has a shadow root.
fn has_shadow_roots(node: &WebCore) -> bool {
    if node.shadow_root.is_some() {
        return true;
    }
    node.children.iter().any(|c| has_shadow_roots(c))
}

/// Walk the tree and resolve `<slot>` elements in all shadow roots.
fn resolve_all_slots(node: &mut WebCore, viewport_w: f32, viewport_h: f32) {
    node.resolve_slots(viewport_w, viewport_h);
    for child in &mut node.children {
        resolve_all_slots(child, viewport_w, viewport_h);
    }
    if let Some(ref mut sr) = node.shadow_root {
        for child in &mut sr.children {
            resolve_all_slots(child, viewport_w, viewport_h);
        }
    }
}

fn count_nodes(node: &WebCore) -> usize {
    1 + node.children.iter().map(|c| count_nodes(c)).sum::<usize>()
}

fn clear_display_contents_boxes(node: &mut WebCore) {
    if matches!(node.style.display, Display::Contents) {
        node.layout.content_rect = Rect::default();
        node.layout.padding_rect = Rect::default();
        node.layout.border_rect = Rect::default();
        node.layout.margin_rect = Rect::default();
    }
    for child in &mut node.children {
        clear_display_contents_boxes(child);
    }
    if let Some(shadow_root) = node.shadow_root.as_mut() {
        for child in &mut shadow_root.children {
            clear_display_contents_boxes(child);
        }
    }
}

fn clear_layout_dirty_flags(node: &mut WebCore) {
    node.layout.layout_dirty = false;
    node.layout.intrinsic_dirty = false;
    node.has_dirty_layout_descendant = false;
    for child in &mut node.children {
        clear_layout_dirty_flags(child);
    }
    if let Some(shadow_root) = node.shadow_root.as_mut() {
        for child in &mut shadow_root.children {
            clear_layout_dirty_flags(child);
        }
    }
}

fn has_in_flow_inline_children(node: &WebCore) -> bool {
    node.effective_children().iter().any(|child| {
        if child.style.display == Display::Contents {
            return has_in_flow_inline_children(child);
        }
        child.style.display != Display::None
            && !matches!(child.style.position, Position::Absolute | Position::Fixed)
            && child.style.float == Float::None
            && child.style.is_inline_level()
            && !(child.is_text_node() && child.text.chars().all(|ch| ch.is_ascii_whitespace()))
    })
}

pub fn has_block_children(node: &WebCore) -> bool {
    node.effective_children().iter().any(|c| {
        if matches!(c.style.display, Display::None) {
            return false;
        }
        if is_layout_inert_svg_node(c) {
            return false;
        }
        if matches!(c.style.display, Display::Contents) {
            return has_block_children(c);
        }
        matches!(
            c.style.position,
            Position::Static | Position::Relative | Position::Sticky
        ) && c.style.is_block_level()
            && matches!(c.style.float, Float::None)
    })
}

fn is_inline_replaced_or_native_control(node: &WebCore) -> bool {
    node.is_image_element()
        || matches!(
            node.tag.as_str(),
            "svg" | "canvas" | "video" | "iframe" | "input" | "select" | "textarea" | "button"
        )
}

// ─── Absolute / fixed positioning pass ───────────────────────────────────────

pub fn layout_positioned(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    parent_font_px: f32,
    root_font_px: f32,
) {
    layout_positioned_static(
        engine,
        node,
        containing_rect,
        parent_font_px,
        root_font_px,
        None,
        None,
    );
}

fn relayout_nested_abs_descendants_against_final_cb(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    font_px: f32,
    root_font_px: f32,
) {
    relayout_nested_abs_descendants_against_final_cb_impl(
        engine,
        node,
        containing_rect,
        font_px,
        root_font_px,
        true,
    );
}

fn relayout_nested_abs_descendants_against_final_cb_impl(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    font_px: f32,
    root_font_px: f32,
    direct_child_of_cb: bool,
) {
    for child in node.effective_children_mut() {
        if matches!(child.style.display, Display::None) {
            continue;
        }

        let child_font_px = child.style.font_size_px(font_px, root_font_px);
        if child.style.position == Position::Absolute {
            if !direct_child_of_cb {
                layout_positioned_static(
                    engine,
                    child,
                    containing_rect,
                    font_px,
                    root_font_px,
                    None,
                    None,
                );
            }
            continue;
        }

        if establishes_positioned_containing_block(&child.style) {
            continue;
        }

        relayout_nested_abs_descendants_against_final_cb_impl(
            engine,
            child,
            containing_rect,
            child_font_px,
            root_font_px,
            false,
        );
    }
}

/// Layout an absolutely/fixed positioned element, with optional static position.
/// `static_x` is the x offset (relative to containing block) where the element would
/// appear in normal flow — used when `left` and `right` are both `auto`.
/// `static_y` is the y offset (relative to containing block) where the element would
/// appear in normal flow — used when `top` and `bottom` are both `auto`.
pub fn layout_positioned_static(
    engine: &LayoutEngine,
    node: &mut WebCore,
    containing_rect: Rect,
    parent_font_px: f32,
    root_font_px: f32,
    static_x: Option<f32>,
    static_y: Option<f32>,
) {
    if matches!(node.style.display, Display::None) {
        clear_layout_subtree(node);
        return;
    }

    let font_px = node.style.font_size_px(parent_font_px, root_font_px);
    // By default the containing block is the passed containing_rect. For `fixed`
    // positioned elements, use the fixed-position containing block stack: the
    // viewport unless a transform/filter/contain/will-change ancestor captured it.
    let mut containing_w = containing_rect.w;
    let mut containing_h = containing_rect.h;
    let mut containing_x = containing_rect.x;
    let mut containing_y = containing_rect.y;
    if node.style.position == Position::Fixed {
        let fixed_cb = engine.fixed_cb.get();
        containing_w = fixed_cb.w;
        containing_h = fixed_cb.h;
        containing_x = fixed_cb.x;
        containing_y = fixed_cb.y;
    }

    let left_auto = node.style.left.is_auto();
    let right_auto = node.style.right.is_auto();
    let top_auto = node.style.top.is_auto();
    let bot_auto = node.style.bottom.is_auto();

    // If both horizontal sides are set AND width is auto, compute width from stretch.
    // If width is explicit (or the element has intrinsic size), don't stretch —
    // auto margins will center it instead (CSS 2.1 §10.3.7).
    let constrained_w = if !left_auto
        && !right_auto
        && node.style.width.is_auto()
        && !(node.tag == "img" && node.image_width > 0)
    {
        let l = node.style.left.resolve_vp(
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let r = node.style.right.resolve_vp(
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let rbox_inner = resolve_box_vp(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
            Some(containing_h),
        );
        let w = (containing_w
            - l
            - r
            - rbox_inner.margin_left
            - rbox_inner.margin_right
            - rbox_inner.inner_h_space())
        .max(0.0);
        Some(w)
    } else {
        None
    };

    let constrained_h = if !top_auto && !bot_auto && node.style.height.is_auto() {
        let t = node.style.top.resolve_vp(
            font_px,
            containing_h,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let b = node.style.bottom.resolve_vp(
            font_px,
            containing_h,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
        );
        let rbox_inner = resolve_box_vp(
            &node.style,
            font_px,
            containing_w,
            root_font_px,
            engine.viewport_w,
            engine.viewport_h,
            Some(containing_h),
        );
        let h = (containing_h
            - t
            - b
            - rbox_inner.margin_top
            - rbox_inner.margin_bottom
            - rbox_inner.inner_v_space())
        .max(0.0);
        Some(h)
    } else {
        None
    };

    // Pass abspos stretch constraints into layout itself. If `top` and
    // `bottom` establish a definite used height for an auto-height element,
    // descendants need that height during flex/grid/block layout; patching the
    // parent rect afterward leaves children aligned against the pre-stretch box.
    let layout_w = constrained_w.unwrap_or(containing_w);
    let mut layout_c = Constraints::with_forced(
        layout_w,
        0.0,
        0.0,
        font_px,
        root_font_px,
        constrained_w,
        constrained_h,
    );
    if containing_h > 0.0 {
        layout_c.available_height = Some(containing_h);
    }
    engine.layout_box(node, &layout_c);

    // Shrink-to-fit: width:auto absolutely-positioned elements wrap their content
    // (CSS 2.1 §10.3.7), just like floats — but only when width is not already
    // constrained by having both left and right set.
    if constrained_w.is_none() && node.style.width.is_auto() {
        let intrinsic_w = engine.max_content_width(node, font_px, root_font_px);
        if intrinsic_w < layout_w {
            let shrink_w = intrinsic_w
                + node.layout.resolved_pad_left
                + node.layout.resolved_pad_right
                + node.layout.resolved_border_left
                + node.layout.resolved_border_right
                + node.layout.resolved_margin_left
                + node.layout.resolved_margin_right;
            engine.layout_box(
                node,
                &Constraints::with_forced(
                    shrink_w,
                    0.0,
                    0.0,
                    font_px,
                    root_font_px,
                    None,
                    constrained_h,
                ),
            );
        }
    }

    // Now resolve position offsets
    let rbox = resolve_box_vp(
        &node.style,
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
        Some(containing_h),
    );
    let res_l = node.style.left.resolve_vp(
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_r = node.style.right.resolve_vp(
        font_px,
        containing_w,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_t = node.style.top.resolve_vp(
        font_px,
        containing_h,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );
    let res_b = node.style.bottom.resolve_vp(
        font_px,
        containing_h,
        root_font_px,
        engine.viewport_w,
        engine.viewport_h,
    );

    let x = if !left_auto
        && !right_auto
        && (node.style.margin_left.is_auto() || node.style.margin_right.is_auto())
        && !node.style.width.is_auto()
    {
        // Both left and right set with auto margins — center the element.
        // available = containing_w - left - right - border_box_w
        let avail = containing_w - res_l - res_r - node.layout.border_rect.w;
        if node.style.margin_left.is_auto() && node.style.margin_right.is_auto() {
            containing_x + res_l + (avail / 2.0).max(0.0)
        } else if node.style.margin_left.is_auto() {
            containing_x + res_l + avail.max(0.0) - rbox.margin_right
        } else {
            containing_x + res_l + rbox.margin_left
        }
    } else if !left_auto {
        if !right_auto && !node.style.width.is_auto() && node.style.direction == Direction::RTL {
            (containing_x + containing_w) - res_r - node.layout.border_rect.w - rbox.margin_right
        } else {
            containing_x + res_l + rbox.margin_left
        }
    } else if !right_auto {
        (containing_x + containing_w) - res_r - node.layout.border_rect.w - rbox.margin_right
    } else if let Some(abs_sx) = static_x {
        abs_sx + rbox.margin_left
    } else if let Some(abs_sx) = node.layout.abs_static_x {
        abs_sx + rbox.margin_left
    } else {
        if node.style.direction == Direction::RTL {
            (containing_x + containing_w) - node.layout.border_rect.w - rbox.margin_right
        } else {
            containing_x + rbox.margin_left
        }
    };

    let y = if !top_auto
        && !bot_auto
        && (node.style.margin_top.is_auto() || node.style.margin_bottom.is_auto())
        && !node.style.height.is_auto()
    {
        // Both top and bottom set with auto margins — center vertically
        let avail = containing_h - res_t - res_b - node.layout.border_rect.h;
        if node.style.margin_top.is_auto() && node.style.margin_bottom.is_auto() {
            containing_y + res_t + (avail / 2.0).max(0.0)
        } else if node.style.margin_top.is_auto() {
            containing_y + res_t + avail.max(0.0) - rbox.margin_bottom
        } else {
            containing_y + res_t + rbox.margin_top
        }
    } else if !top_auto {
        containing_y + res_t + rbox.margin_top
    } else if !bot_auto {
        (containing_y + containing_h) - res_b - node.layout.border_rect.h - rbox.margin_bottom
    } else if let Some(abs_sy) = static_y {
        // Static position: absolute document-space y where the element would
        // appear in normal flow. Already accounts for parent offsets.
        abs_sy + rbox.margin_top
    } else if let Some(abs_sy) = node.layout.abs_static_y {
        // Static position recorded on the node itself (set during parent's
        // inline/block layout for deeply nested absolute elements).
        abs_sy + rbox.margin_top
    } else {
        // Fallback: containing block content start.
        containing_y + rbox.margin_top
    };

    // Shift all rects to final position
    let dx = x - node.layout.border_rect.x;
    let dy = y - node.layout.border_rect.y;
    shift_rects(node, dx, dy);

    // If both sides set → we may need to re-layout with constrained size
    if let Some(cw) = constrained_w {
        if node.layout.content_rect.w != cw {
            engine.layout_box(
                node,
                &Constraints::with_forced(
                    layout_w,
                    x,
                    y,
                    font_px,
                    root_font_px,
                    constrained_w,
                    constrained_h,
                ),
            );
        }
    }

    // Apply constrained height when both top and bottom are set and height is auto.
    // Without this, inset:0 (top:0 bottom:0) leaves height at 0 because layout_box
    // has no content to fill the space.
    if let Some(ch) = constrained_h {
        if node.style.height.is_auto() && (node.layout.content_rect.h - ch).abs() > 0.5 {
            let diff = ch - node.layout.content_rect.h;
            node.layout.content_rect.h += diff;
            node.layout.padding_rect.h += diff;
            node.layout.border_rect.h += diff;
            node.layout.margin_rect.h += diff;
        }
    }
}

pub fn shift_rects(node: &mut WebCore, dx: f32, dy: f32) {
    if matches!(node.style.display, Display::None) {
        return;
    }
    node.layout.content_rect.x += dx;
    node.layout.content_rect.y += dy;
    node.layout.padding_rect.x += dx;
    node.layout.padding_rect.y += dy;
    node.layout.border_rect.x += dx;
    node.layout.border_rect.y += dy;
    node.layout.margin_rect.x += dx;
    node.layout.margin_rect.y += dy;
    for segment in &mut node.layout.collapsed_border_segments {
        segment.rect.x += dx;
        segment.rect.y += dy;
    }
    for line in &mut node.layout.line_cache {
        line.x += dx;
        line.y += dy;
    }
    for rect in &mut node.layout.inline_client_rects {
        rect.x += dx;
        rect.y += dy;
    }
    // `effective_children_mut`, not `children`: a shadow host's subtree lives in
    // `shadow_root.children`, which layout reaches through the accessor. Moving
    // a host by `children` alone left its shadow content where it was — a
    // parent that collapsed its first child's margin moved down and its shadow
    // child did not, landing 16px above the host it lives in.
    for child in node.effective_children_mut() {
        // Fixed-position elements are placed relative to the viewport,
        // not their parent — don't shift them when a parent moves.
        if child.style.position == Position::Fixed {
            continue;
        }
        shift_rects(child, dx, dy);
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}
