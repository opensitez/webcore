/// Browser version — bump periodically to stay current with real Chrome releases.
const CHROME_MAJOR: u32 = 131;

static CSS_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(2, 4))
        .thread_name(|i| format!("webcore-css-{i}"))
        .build()
        .expect("webcore CSS resource pool")
});

static IMAGE_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(4, 8))
        .thread_name(|i| format!("webcore-image-{i}"))
        .build()
        .expect("webcore image resource pool")
});

static FONT_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(2, 4))
        .thread_name(|i| format!("webcore-font-{i}"))
        .build()
        .expect("webcore font resource pool")
});

static PARSED_CSS_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<css::Stylesheet>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

struct CssParseState {
    result: std::sync::Mutex<Option<std::sync::Arc<css::Stylesheet>>>,
    done: std::sync::Condvar,
}

static CSS_PARSE_IN_FLIGHT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CssParseState>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

static DECODED_IMAGE_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, html::DecodedImage>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

struct ImageDecodeState {
    result: std::sync::Mutex<Option<Option<html::DecodedImage>>>,
    done: std::sync::Condvar,
}

static DECODED_IMAGE_IN_FLIGHT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<ImageDecodeState>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn resource_pool_threads(min: usize, max: usize) -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(min)
        .clamp(min, max)
}

pub(crate) fn spawn_css_resource_task(task: impl FnOnce() + Send + 'static) {
    CSS_RESOURCE_POOL.spawn(task);
}

fn spawn_image_resource_task(task: impl FnOnce() + Send + 'static) {
    IMAGE_RESOURCE_POOL.spawn(task);
}

pub(crate) fn spawn_font_resource_task(task: impl FnOnce() + Send + 'static) {
    FONT_RESOURCE_POOL.spawn(task);
}

fn cached_decoded_image(
    url: &str,
    loader: Option<&(dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static)>,
) -> Option<html::DecodedImage> {
    if let Some(decoded) = DECODED_IMAGE_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(url).cloned())
    {
        return Some(decoded);
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
        return guard.as_ref().and_then(|decoded| decoded.clone());
    }
    let decoded = match loader {
        Some(loader) => loader(url),
        None => html::load_decoded_image_from_src(url, ""),
    };
    if let Some(decoded) = decoded.as_ref()
        && let Ok(mut cache) = DECODED_IMAGE_CACHE.lock()
    {
        if cache.len() > 512 {
            cache.clear();
        }
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

fn stream_stylesheet_fragments(
    css_text: &str,
    css_url: &str,
    media: &str,
    mut emit: impl FnMut(css::Stylesheet),
) -> usize {
    let mut emitted = 0;
    for chunk in complete_css_units(css_text) {
        let mut sheet = css::Stylesheet::default();
        sheet.parse_and_add_with_base_media(chunk, css_url, media);
        if !sheet.rules.is_empty()
            || !sheet.font_faces.is_empty()
            || !sheet.keyframes.is_empty()
            || !sheet.page_rules.is_empty()
            || !sheet.counter_styles.is_empty()
        {
            emitted += 1;
            emit(sheet);
        }
    }
    emitted
}

fn stylesheet_has_content(sheet: &css::Stylesheet) -> bool {
    !sheet.rules.is_empty()
        || !sheet.font_faces.is_empty()
        || !sheet.keyframes.is_empty()
        || !sheet.page_rules.is_empty()
        || !sheet.counter_styles.is_empty()
}

pub(crate) fn drain_complete_css_text(buffer: &mut String) -> Option<String> {
    let drain_to = complete_css_drain_boundary(buffer);
    if drain_to == 0 {
        return None;
    }
    let complete = buffer[..drain_to].to_string();
    let rest = buffer[drain_to..].to_string();
    *buffer = rest;
    if complete.trim().is_empty() {
        None
    } else {
        Some(complete)
    }
}

fn complete_css_drain_boundary(css_text: &str) -> usize {
    let bytes = css_text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = None;
    let mut escape = false;
    let mut in_comment = false;
    let mut boundary = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_comment {
            if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(quote) = in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_comment = true;
            i += 2;
            continue;
        }
        if b == b'\'' || b == b'"' {
            in_string = Some(b);
            i += 1;
            continue;
        }
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    boundary = i + 1;
                }
            }
            b';' if depth == 0 => boundary = i + 1,
            _ => {}
        }
        i += 1;
    }
    boundary
}

fn complete_css_units(css_text: &str) -> Vec<&str> {
    let bytes = css_text.as_bytes();
    let mut units = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut in_string = None;
    let mut escape = false;
    let mut in_comment = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_comment {
            if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                in_comment = false;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if let Some(quote) = in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            in_comment = true;
            i += 2;
            continue;
        }
        if b == b'\'' || b == b'"' {
            in_string = Some(b);
            i += 1;
            continue;
        }
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let end = i + 1;
                    if css_text[start..end].trim().contains('{') {
                        units.push(css_text[start..end].trim());
                    }
                    start = end;
                }
            }
            b';' if depth == 0 => {
                let end = i + 1;
                let unit = css_text[start..end].trim();
                if unit.starts_with('@') {
                    units.push(unit);
                }
                start = end;
            }
            _ => {}
        }
        i += 1;
    }
    let tail = css_text[start..].trim();
    if !tail.is_empty() && depth == 0 {
        units.push(tail);
    }
    units
}

/// Build a platform-appropriate User-Agent string at runtime.
/// Real browsers derive this from their binary version and the OS they're
/// running on.  We approximate by using compile-time platform detection
/// and a manually-bumped Chrome major version.
fn build_user_agent() -> String {
    let platform = if cfg!(target_os = "macos") {
        "Macintosh; Intel Mac OS X 10_15_7"
    } else if cfg!(target_os = "windows") {
        "Windows NT 10.0; Win64; x64"
    } else {
        "X11; Linux x86_64"
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{CHROME_MAJOR}.0.0.0 Safari/537.36"
    )
}

/// Platform string for Sec-CH-UA-Platform header.
fn platform_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "\"macOS\""
    } else if cfg!(target_os = "windows") {
        "\"Windows\""
    } else {
        "\"Linux\""
    }
}

/// Sec-CH-UA header matching the Chrome version we claim.
fn sec_ch_ua() -> String {
    format!(
        "\"Chromium\";v=\"{CHROME_MAJOR}\", \"Google Chrome\";v=\"{CHROME_MAJOR}\", \"Not-A.Brand\";v=\"99\""
    )
}

/// User-Agent sent with all HTTP requests.
pub fn user_agent() -> String {
    build_user_agent()
}

/// Legacy constant — prefer `user_agent()`.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub type StylesheetLoader =
    std::sync::Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync + 'static>;
pub type StreamingStylesheetLoader =
    std::sync::Arc<dyn Fn(&str, &mut dyn FnMut(&str)) -> Result<(), String> + Send + Sync + 'static>;
pub type ImageLoader =
    std::sync::Arc<dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static>;

pub(crate) struct CachedStylesheetLoad {
    pub sheet: css::Stylesheet,
    pub text_len: usize,
    pub emitted_fragments: usize,
}

pub(crate) fn load_stylesheet_cached<F>(
    cache_key: String,
    css_url: String,
    media: String,
    loader: StylesheetLoader,
    streaming_loader: Option<StreamingStylesheetLoader>,
    cache_parsed: bool,
    mut emit_fragment: F,
) -> CachedStylesheetLoad
where
    F: FnMut(css::Stylesheet),
{
    if cache_parsed
        && let Some(sheet) = PARSED_CSS_CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return CachedStylesheetLoad {
            sheet: (*sheet).clone(),
            text_len: 0,
            emitted_fragments: 0,
        };
    }

    let parse_state = if cache_parsed {
        let mut in_flight = CSS_PARSE_IN_FLIGHT
            .lock()
            .expect("CSS parse in-flight cache poisoned");
        if let Some(state) = in_flight.get(&cache_key) {
            let state = state.clone();
            drop(in_flight);
            let mut guard = state.result.lock().expect("CSS parse result poisoned");
            while guard.is_none() {
                guard = state.done.wait(guard).expect("CSS parse result poisoned");
            }
            return CachedStylesheetLoad {
                sheet: guard
                    .as_ref()
                    .map(|sheet| (**sheet).clone())
                    .unwrap_or_default(),
                text_len: 0,
                emitted_fragments: 0,
            };
        }
        let state = std::sync::Arc::new(CssParseState {
            result: std::sync::Mutex::new(None),
            done: std::sync::Condvar::new(),
        });
        in_flight.insert(cache_key.clone(), state.clone());
        Some(state)
    } else {
        None
    };

    let mut combined = crate::css::Stylesheet::default();
    let mut text_len = 0usize;
    let mut emitted = 0usize;
    if let Some(streaming_loader) = streaming_loader {
        let mut buffer = String::new();
        let _ = streaming_loader(&css_url, &mut |chunk| {
            text_len += chunk.len();
            buffer.push_str(chunk);
            if let Some(complete_css) = drain_complete_css_text(&mut buffer) {
                let mut fragment = crate::css::Stylesheet::default();
                fragment.parse_and_add_with_base_media(&complete_css, &css_url, &media);
                if stylesheet_has_content(&fragment) {
                    emitted += 1;
                    if cache_parsed {
                        combined.append_fragment(fragment.clone());
                        emit_fragment(fragment);
                    } else {
                        emit_fragment(fragment);
                    }
                }
            }
        });
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            let mut fragment = crate::css::Stylesheet::default();
            fragment.parse_and_add_with_base_media(&tail, &css_url, &media);
            if stylesheet_has_content(&fragment) {
                emitted += 1;
                if cache_parsed {
                    combined.append_fragment(fragment.clone());
                    emit_fragment(fragment);
                } else {
                    emit_fragment(fragment);
                }
            }
        }
    } else {
        let text = loader(&css_url).unwrap_or_default();
        text_len = text.len();
        emitted = stream_stylesheet_fragments(&text, &css_url, &media, |fragment| {
            combined.append_fragment(fragment.clone());
            emit_fragment(fragment);
        });
        if emitted == 0 {
            combined.parse_and_add_with_base_media(&text, &css_url, &media);
        }
    }

    if cache_parsed && let Ok(mut cache) = PARSED_CSS_CACHE.lock() {
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(cache_key.clone(), std::sync::Arc::new(combined.clone()));
    }
    if let Some(parse_state) = parse_state {
        let mut guard = parse_state.result.lock().expect("CSS parse result poisoned");
        *guard = Some(std::sync::Arc::new(combined.clone()));
        parse_state.done.notify_all();
        if let Ok(mut in_flight) = CSS_PARSE_IN_FLIGHT.lock() {
            in_flight.remove(&cache_key);
        }
    }

    CachedStylesheetLoad {
        sheet: combined,
        text_len,
        emitted_fragments: emitted,
    }
}

pub mod types;

#[cfg(feature = "accessibility")]
pub mod accessibility;
/// HTML §4.12.5 — the `<canvas>` element's 2D rendering context.
///
/// The engine owns its own rasteriser, the way a browser engine does. It is a
/// sibling of `renderer` rather than a part of it: `renderer` paints the boxes
/// the cascade produced, this paints whatever a script asks for inside one box.
pub mod canvas;
pub mod css;
pub mod dom;
pub mod frame;
pub mod html;
pub mod layout;
pub mod loading;
pub mod markdown;
pub mod platform;
pub mod renderer;
pub mod svg;
pub mod video;
pub mod widgets;
/// WHATWG HTML §7 — browsing contexts and the `Window` interface.
pub mod window;
pub mod woff;

#[cfg(test)]
pub mod tests;

pub use dom::HtmlEventType;
pub use frame::{EngineCallbacks, EngineFrame};
pub use html::streaming::{DomMutation, ResourceKind, StreamingParser};
pub use html::{
    parse_html, parse_html_bytes, parse_html_bytes_with_base, parse_html_with_base,
    parse_html_with_hooks, parse_html_with_scripts, resolve_url,
};
pub use layout::hit_test::{
    HitResult, get_caret_x, get_offset_from_x, hit_test_box_at, hit_test_link, offset_to_point,
    point_to_hit,
};
pub use layout::perf::PerfCounters;
pub use layout::{Constraints, FormattingContext, IntrinsicSizes, LayoutEngine};
pub use loading::{
    PageLoadEvent, PageLoadOptions, PageSession, PageSessionEvent, fetch_text_resource,
    fetch_text_resource_streaming, spawn_page_load,
};
pub use markdown::{parse_markdown, serializer::serialize_markdown};
pub use renderer::compositor::{Compositor, CompositorLayer, LayerId, LayerReason};
pub use renderer::{Renderer, draw_inspect_overlay};
pub use types::{
    AnimDirection, AnimState, Announcement, CSSCursor, CanvasContext, Color, Component,
    ComponentEvent, ComponentRegistry, ComputedStyle, Document, DocumentStylesheet, EasingFn,
    FillMode, FormEvent, FormEventCallback, FormEventKind, KeyframeStop, LivePoliteness,
    MatchedRule, ParsedAnimation, ParsedTransition, Rect, ShadowMode, ShadowRoot, TransitionState,
    WebCore, apply_autofocus, build_form_submit_url, collect_form_data, encode_form_urlencoded,
    find_parent_form_action, input_value, is_text_input, process_form_input_key, reset_form,
};

/// High-level convenience: parse HTML, layout, ready to render.
pub fn load_html(html: &str, viewport_width: f32) -> Document {
    load_html_vp(html, viewport_width, 700.0)
}

/// Like `load_html` but with explicit viewport height (needed for `100vh` layouts).
pub fn load_html_vp(html: &str, viewport_width: f32, viewport_height: f32) -> Document {
    load_html_with_base(html, "", viewport_width, viewport_height)
}

/// Parse HTML with a base URL, fetch external CSS, layout, ready to render.
pub fn load_html_with_base(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
) -> Document {
    load_html_with_registry(
        html,
        base_url,
        viewport_width,
        viewport_height,
        types::ComponentRegistry::default(),
    )
}

/// Parse HTML and layout with custom component registry.
/// External `<link rel="stylesheet">` tags trigger parallel CSS fetches during parsing
/// (like a browser), so network I/O overlaps with tokenisation.
pub fn load_html_with_registry(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
    registry: types::ComponentRegistry,
) -> Document {
    load_html_reusing(
        html,
        base_url,
        viewport_width,
        viewport_height,
        registry,
        None,
    )
}

/// The same load, but laying out with a renderer the caller ALREADY has.
///
/// ⛔ The initial layout needs a `FontSystem`, and this used to get one by
/// constructing a whole `Renderer` here — so `Renderer::load_html` built a
/// second font system, used it once and dropped it, while its own sat idle.
/// `FontSystem::new()` is **3.2 s cold and 173 ms warm**; `LayoutEngine::new()`
/// is 0 ms. That was the entire fixed cost of loading a page: a one-element
/// document measured 118 ms in the "Layout" phase and 0 ms in `layout()`.
pub fn load_html_reusing(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
    registry: types::ComponentRegistry,
    reuse: Option<&mut Renderer>,
) -> Document {
    load_html_reusing_with_stylesheet_loader(
        html,
        base_url,
        viewport_width,
        viewport_height,
        registry,
        reuse,
        None,
    )
}

pub fn load_html_reusing_with_stylesheet_loader(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
    registry: types::ComponentRegistry,
    reuse: Option<&mut Renderer>,
    stylesheet_loader: Option<StylesheetLoader>,
) -> Document {
    load_html_reusing_with_stylesheet_loader_and_wait(
        html,
        base_url,
        viewport_width,
        viewport_height,
        registry,
        reuse,
        stylesheet_loader,
        std::time::Duration::from_secs(2),
    )
}

pub fn load_html_reusing_with_stylesheet_loader_and_wait(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
    registry: types::ComponentRegistry,
    reuse: Option<&mut Renderer>,
    stylesheet_loader: Option<StylesheetLoader>,
    css_wait: std::time::Duration,
) -> Document {
    load_html_reusing_with_resource_loaders_and_wait(
        html,
        base_url,
        viewport_width,
        viewport_height,
        registry,
        reuse,
        stylesheet_loader,
        None,
        None,
        true,
        css_wait,
    )
}

pub fn load_html_reusing_with_resource_loaders_and_wait(
    html: &str,
    base_url: &str,
    viewport_width: f32,
    viewport_height: f32,
    registry: types::ComponentRegistry,
    reuse: Option<&mut Renderer>,
    stylesheet_loader: Option<StylesheetLoader>,
    streaming_stylesheet_loader: Option<StreamingStylesheetLoader>,
    image_loader: Option<ImageLoader>,
    load_images: bool,
    css_wait: std::time::Duration,
) -> Document {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    // Channel for CSS results — fetches start during parsing via the hook.
    let (css_tx, css_rx) =
        mpsc::channel::<(usize, String, crate::css::Stylesheet, String)>(); // idx, url, parsed css, media
    let css_tx2 = css_tx.clone();
    let css_idx = Arc::new(AtomicUsize::new(0));
    let css_idx2 = css_idx.clone();
    let base_owned = base_url.to_string();
    let stylesheet_loader = stylesheet_loader.unwrap_or_else(|| Arc::new(|url| fetch_text(url)));
    let streaming_stylesheet_loader = streaming_stylesheet_loader;
    let mut scheduled_stylesheets = std::collections::HashSet::<String>::new();

    let t0 = std::time::Instant::now();
    let mut doc =
        crate::html::parse_html_with_hooks_defer_cascade(html, base_url, move |tag, attrs| {
            if tag == "link"
                && attrs
                    .get("rel")
                    .map(|s| s.eq_ignore_ascii_case("stylesheet"))
                    .unwrap_or(false)
                && !attrs.contains_key("disabled")
            {
                if let Some(href) = attrs.get("href") {
                    let abs = resolve_css_url(&base_owned, href);
                    let media = attrs.get("media").cloned().unwrap_or_default();
                    let cache_key = format!("{abs}\n{media}");
                    if !scheduled_stylesheets.insert(cache_key.clone()) {
                        return;
                    }
                    eprintln!("  CSS fetch: {abs}");
                    let sender = css_tx2.clone();
                    let idx = css_idx2.fetch_add(1, Ordering::SeqCst);
                    let loader = stylesheet_loader.clone();
                    let streaming_loader = streaming_stylesheet_loader.clone();
                    let cache_parsed = true;
                    spawn_css_resource_task(move || {
                        let t = std::time::Instant::now();
                        let loaded = load_stylesheet_cached(
                            cache_key,
                            abs.clone(),
                            media.clone(),
                            loader,
                            streaming_loader,
                            cache_parsed,
                            |fragment| {
                                let _ = sender.send((idx, abs.clone(), fragment, media.clone()));
                            },
                        );
                        if loaded.emitted_fragments == 0 {
                            let _ = sender.send((
                                idx,
                                abs.clone(),
                                loaded.sheet.clone(),
                                media.clone(),
                            ));
                        }
                        eprintln!(
                            "  CSS done:  {} ({:.0}ms, {} bytes)",
                            abs,
                            t.elapsed().as_millis(),
                            loaded.text_len
                        );
                    });
                }
            }
        });
    doc.preserve_stylesheet_document_order = !css_wait.is_zero();
    eprintln!("Parse: {:.0}ms", t0.elapsed().as_millis());
    drop(css_tx); // close sender so rx.iter() terminates after all threads finish

    // Collect fetched stylesheets. Browser hosts that want progressive first
    // paint can pass zero here; deterministic/headless paths keep the legacy
    // short wait so screenshots include fast stylesheets.
    let t1 = std::time::Instant::now();
    let expected_count = css_idx.load(std::sync::atomic::Ordering::SeqCst);
    let mut css_results: Vec<(usize, String, crate::css::Stylesheet, String)> = Vec::new();
    if !css_wait.is_zero() {
        let deadline = std::time::Instant::now() + css_wait;
        while css_results.len() < expected_count {
            match css_rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(item) => css_results.push(item),
                Err(_) => break, // timeout or disconnected
            }
        }
    }
    eprintln!(
        "CSS wait: {:.0}ms ({}/{} sheets)",
        t1.elapsed().as_millis(),
        css_results.len(),
        expected_count
    );
    let has_pending_css = css_results.len() < expected_count;
    if !doc.document_stylesheets.is_empty() {
        let mut fetched_map: std::collections::HashMap<String, crate::css::Stylesheet> =
            std::collections::HashMap::new();
        for (_, css_url, sheet, _) in css_results {
            match fetched_map.entry(css_url) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().append_fragment(sheet);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(sheet);
                }
            }
        }
        doc.stylesheet = crate::css::ua_stylesheet();
        for ds in &doc.document_stylesheets {
            match ds {
                crate::types::DocumentStylesheet::Inline { css } => {
                    doc.stylesheet.parse_and_add_with_base(css, &doc.base_url);
                }
                crate::types::DocumentStylesheet::Linked { href, .. } => {
                    let abs = resolve_css_url(base_url, href);
                    if let Some(sheet) = fetched_map.get(&abs) {
                        doc.stylesheet.append_fragment(sheet.clone());
                    }
                }
            }
        }
        doc.loaded_linked_stylesheets = fetched_map;
    } else {
        css_results.sort_by_key(|(idx, _, _, _)| *idx);
        for (_, css_url, sheet, media) in &css_results {
            let _ = media;
            doc.loaded_linked_stylesheets
                .entry(css_url.clone())
                .and_modify(|existing| existing.append_fragment(sheet.clone()))
                .or_insert_with(|| sheet.clone());
            doc.stylesheet.append_fragment(sheet.clone());
        }
    }
    if has_pending_css {
        doc.pending_stylesheets = Some(css_rx);
    }

    // Re-run cascade with the real viewport so @media queries (min-width, max-width, etc.)
    // are evaluated against the actual window size rather than the default vw=0, vh=0.
    let t2 = std::time::Instant::now();
    doc.stylesheet
        .resolve_variables_for_viewport(viewport_width, viewport_height);
    doc.stylesheet.rebuild_index();
    eprintln!("  Cascade start ({} rules)...", doc.stylesheet.rules.len());
    // ⛔ No cascade here. `layout()` runs one itself — a better one, with the
    // hover chain and focus — whenever the engine has not cascaded at this
    // viewport or the DOM is style-dirty, which is always true on a first
    // load. Running one here too meant every page load cascaded TWICE.
    eprintln!(
        "  Cascade: {:.0}ms (deferred to layout)",
        t2.elapsed().as_millis()
    );

    // Resolve <picture> elements with real viewport dimensions before image fetching
    let base = doc.base_url.clone();
    html::resolve_picture_elements(&mut doc.root, &base, viewport_width, viewport_height);

    let t3 = std::time::Instant::now();
    let mut owned: Option<Renderer> = None;
    let renderer: &mut Renderer = match reuse {
        Some(r) => r,
        None => {
            owned = Some(Renderer::new());
            owned.as_mut().expect("just set")
        }
    };
    renderer.component_registry = registry;
    {
        let engine = renderer.layout_engine();
        engine.viewport_w = viewport_width;
        engine.viewport_h = viewport_height;
        engine.layout(&mut doc, viewport_width);
    }
    eprintln!("  Layout: {:.0}ms", t3.elapsed().as_millis());

    if load_images {
        start_async_image_fetches_with_loader(&mut doc, image_loader);
    }
    // Fire DOMContentLoaded — listeners registered before load_html can react.
    let mut evt = dom::HtmlEvent::new(dom::HtmlEventType::DOMContentLoaded);
    evt.target = doc.root.node_id;
    doc.dispatch_input_event(evt);
    doc.svg_trigger_projected_tree_event("DOMContentLoaded");
    doc.svg_trigger_projected_tree_event("load");
    doc
}

/// Walk the DOM tree, find image resources, fire off parallel decode/fetch
/// threads, and store their channel on Document for async polling.
pub fn restart_async_image_fetches_with_loader(
    doc: &mut types::Document,
    loader: std::sync::Arc<dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static>,
) {
    doc.pending_images = None;
    doc.images_in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    start_async_image_fetches_with_loader(doc, Some(loader));
}

fn start_async_image_fetches_with_loader(
    doc: &mut types::Document,
    loader: Option<std::sync::Arc<dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static>>,
) {
    let mut pending: Vec<(Vec<usize>, types::PendingImageTarget, String)> = Vec::new();
    collect_remote_images(&doc.root, &doc.base_url, &mut Vec::new(), &mut pending);
    if pending.is_empty() {
        return;
    }

    let (tx, rx) = std::sync::mpsc::channel::<types::PendingImageResult>();
    let in_flight = doc.images_in_flight.clone();
    in_flight.store(pending.len(), std::sync::atomic::Ordering::SeqCst);

    for (path, target, url) in pending {
        let sender = tx.clone();
        let counter = in_flight.clone();
        let loader = loader.clone();
        spawn_image_resource_task(move || {
            let result = cached_decoded_image(&url, loader.as_deref());
            if let Some(decoded) = result {
                let _ = sender.send((path, target, decoded));
            }
            counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });
    }
    doc.pending_images = Some(rx);
}

fn collect_remote_images(
    node: &types::WebCore,
    base_url: &str,
    path: &mut Vec<usize>,
    pending: &mut Vec<(Vec<usize>, types::PendingImageTarget, String)>,
) {
    if (node.is_image_element() || node.tag == "video") && node.image_data.is_none() {
        let resolved;
        let url = if !node.resolved_src.is_empty() {
            node.resolved_src.as_str()
        } else {
            let raw = if node.tag == "video" {
                node.attributes.get("poster").map(|s| s.as_str())
            } else {
                node.attributes.get("src").map(|s| s.as_str())
            };
            match raw {
                Some(raw) => {
                    resolved = html::resolve_url(raw, base_url);
                    resolved.as_str()
                }
                None => "",
            }
        };
        if is_async_image_url(url) {
            pending.push((
                path.clone(),
                types::PendingImageTarget::Element,
                url.to_string(),
            ));
        }
    }
    if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
        let resolved = html::resolve_url(&node.style.background_image_url, base_url);
        let url = resolved.as_str();
        if is_async_image_url(url) {
            pending.push((
                path.clone(),
                types::PendingImageTarget::Background,
                url.to_string(),
            ));
        }
    }
    if node.mask_image_data.is_none() && !node.style.rare().mask_image_url.is_empty() {
        let resolved = html::resolve_url(&node.style.rare().mask_image_url, base_url);
        let url = resolved.as_str();
        if is_async_image_url(url) {
            pending.push((
                path.clone(),
                types::PendingImageTarget::Mask,
                url.to_string(),
            ));
        }
    }
    for (i, child) in node.children.iter().enumerate() {
        path.push(i);
        collect_remote_images(child, base_url, path, pending);
        path.pop();
    }
}

fn is_async_image_url(url: &str) -> bool {
    url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
        || url.starts_with('/')
        || url.starts_with("./")
        || !url.is_empty()
}

fn resolve_css_url(base: &str, href: &str) -> String {
    html::resolve_url(href, base)
}

/// Build a `reqwest::blocking::Client` with browser-like defaults.
/// Handles gzip/brotli/deflate decompression and redirects automatically.
/// Only sets shared headers (UA, Accept-Language, Sec-CH-UA); callers should
/// add request-specific headers (Accept, Sec-Fetch-Dest, etc.) per request.
pub fn http_client() -> reqwest::blocking::Client {
    build_http_client(false)
}

/// Lenient client that accepts certs where the base domain (without www.)
/// is in the SAN but the exact subdomain isn't — matches Chrome behaviour
/// for shared-hosting certs.
pub fn http_client_lenient() -> reqwest::blocking::Client {
    build_http_client(true)
}

fn build_http_client(accept_invalid_certs: bool) -> reqwest::blocking::Client {
    let ua = build_user_agent();
    let ch_ua = sec_ch_ua();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Accept-Language", "en-US,en;q=0.9".parse().unwrap());
    headers.insert("Sec-CH-UA", ch_ua.parse().unwrap());
    headers.insert("Sec-CH-UA-Mobile", "?0".parse().unwrap());
    headers.insert("Sec-CH-UA-Platform", platform_hint().parse().unwrap());
    reqwest::blocking::Client::builder()
        .user_agent(ua)
        .default_headers(headers)
        .danger_accept_invalid_certs(accept_invalid_certs)
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}

/// Fetch a document, returning its text and the URL it actually came FROM.
///
/// ⛔ The second value is the point. A redirect is the normal case on the web
/// — bare domain to `www`, `http` to `https`, one domain to another — and the
/// document's base URL is the FINAL url, not the requested one (HTML §2.4.1,
/// "document base URL"). Resolving relative `<link>` and `<img>` against the
/// requested URL sends every subresource to the old host, where they redirect
/// to that site's homepage: the "stylesheet" that comes back is HTML, and the
/// page renders with no author CSS at all.
pub fn fetch_document(url: &str) -> Result<(String, String), String> {
    if let Some(path) = url.strip_prefix("file://") {
        let path = path.split('?').next().unwrap_or(path);
        let path = path.split('#').next().unwrap_or(path);
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        return Ok((text, url.to_string()));
    }
    let resp = http_client().get(url).send().map_err(|e| e.to_string())?;
    let final_url = resp.url().to_string();
    let text = resp.text().map_err(|e| e.to_string())?;
    Ok((text, final_url))
}

fn fetch_text(url: &str) -> Result<String, String> {
    // ⛔ `file://` is read from disk, not sent to the HTTP client. A document
    // opened from the filesystem loads its stylesheets the same way a browser
    // does; handing the URL to reqwest failed silently and the page rendered
    // with no author CSS at all.
    if let Some(path) = url.strip_prefix("file://") {
        let path = path.split('?').next().unwrap_or(path);
        let path = path.split('#').next().unwrap_or(path);
        return std::fs::read_to_string(path).map_err(|e| e.to_string());
    }
    let do_fetch = |client: &reqwest::blocking::Client| -> Result<Vec<u8>, String> {
        let resp = client
            .get(url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .header("Sec-Fetch-Dest", "document")
            .header("Sec-Fetch-Mode", "navigate")
            .header("Sec-Fetch-Site", "none")
            .header("Sec-Fetch-User", "?1")
            .header("Upgrade-Insecure-Requests", "1")
            .send()
            .map_err(|e| e.to_string())?;
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    };
    // Try strict TLS first; on failure or empty response, retry with lenient
    // cert validation (matches Chrome behaviour for shared-hosting certs where
    // the base domain is in the SAN but www. subdomain isn't).
    let bytes = match do_fetch(&http_client()) {
        Ok(b) if !b.is_empty() => b,
        _ => do_fetch(&http_client_lenient())?,
    };
    decode_text(&bytes)
}

/// Decode bytes to a String, trying UTF-8 first, then falling back to
/// encoding_rs for Latin-1 / Windows-1252 / etc.
fn decode_text(bytes: &[u8]) -> Result<String, String> {
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => Ok(s),
        Err(_) => {
            // Try common fallback encodings
            let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
            Ok(cow.into_owned())
        }
    }
}
