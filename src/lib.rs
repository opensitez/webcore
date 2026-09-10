/// Browser version — bump periodically to stay current with real Chrome releases.
const CHROME_MAJOR: u32 = 131;

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
    format!("\"Chromium\";v=\"{CHROME_MAJOR}\", \"Google Chrome\";v=\"{CHROME_MAJOR}\", \"Not-A.Brand\";v=\"99\"")
}

/// User-Agent sent with all HTTP requests.
pub fn user_agent() -> String {
    build_user_agent()
}

/// Legacy constant — prefer `user_agent()`.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

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
    get_caret_x, get_offset_from_x, hit_test_box_at, hit_test_link, offset_to_point, point_to_hit,
    HitResult,
};
pub use layout::perf::PerfCounters;
pub use layout::{Constraints, FormattingContext, IntrinsicSizes, LayoutEngine};
pub use markdown::{parse_markdown, serializer::serialize_markdown};
pub use renderer::compositor::{Compositor, CompositorLayer, LayerId, LayerReason};
pub use renderer::{draw_inspect_overlay, Renderer};
pub use types::{
    apply_autofocus, build_form_submit_url, collect_form_data, encode_form_urlencoded,
    find_parent_form_action, input_value, is_text_input, process_form_input_key, reset_form,
    AnimDirection, AnimState, Announcement, CSSCursor, CanvasContext, Color, Component,
    ComponentEvent, ComponentRegistry, ComputedStyle, Document, DocumentStylesheet, EasingFn, FillMode, FormEvent,
    FormEventCallback, FormEventKind, KeyframeStop, LivePoliteness, MatchedRule, ParsedAnimation,
    ParsedTransition, Rect, ShadowMode, ShadowRoot, TransitionState, WebCore,
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
    stylesheet_loader: Option<
        std::sync::Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync + 'static>,
    >,
) -> Document {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };

    // Channel for CSS results — fetches start during parsing via the hook.
    let (css_tx, css_rx) = mpsc::channel::<(usize, String, String, String)>(); // idx, url, css, media
    let css_tx2 = css_tx.clone();
    let css_idx = Arc::new(AtomicUsize::new(0));
    let css_idx2 = css_idx.clone();
    let base_owned = base_url.to_string();
    let stylesheet_loader = stylesheet_loader.unwrap_or_else(|| Arc::new(|url| fetch_text(url)));

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
                    eprintln!("  CSS fetch: {abs}");
                    let sender = css_tx2.clone();
                    let idx = css_idx2.fetch_add(1, Ordering::SeqCst);
                    let loader = stylesheet_loader.clone();
                    std::thread::spawn(move || {
                        let t = std::time::Instant::now();
                        let text = loader(&abs).unwrap_or_default();
                        eprintln!(
                            "  CSS done:  {} ({:.0}ms, {} bytes)",
                            abs,
                            t.elapsed().as_millis(),
                            text.len()
                        );
                        let _ = sender.send((idx, abs, text, media));
                    });
                }
            }
        });
    eprintln!("Parse: {:.0}ms", t0.elapsed().as_millis());
    drop(css_tx); // close sender so rx.iter() terminates after all threads finish

    // Collect fetched stylesheets — wait up to 2s (CSS already started during parsing).
    let t1 = std::time::Instant::now();
    let expected_count = css_idx.load(std::sync::atomic::Ordering::SeqCst);
    let mut css_results: Vec<(usize, String, String, String)> = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while css_results.len() < expected_count {
        match css_rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(item) => css_results.push(item),
            Err(_) => break, // timeout or disconnected
        }
    }
    eprintln!(
        "CSS wait: {:.0}ms ({}/{} sheets)",
        t1.elapsed().as_millis(),
        css_results.len(),
        expected_count
    );
    if !doc.document_stylesheets.is_empty() {
        let mut fetched_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (_, css_url, sheet, _) in css_results {
            if !sheet.is_empty() {
                fetched_map.insert(css_url, sheet);
            }
        }
        doc.stylesheet = crate::css::ua_stylesheet();
        for ds in &doc.document_stylesheets {
            match ds {
                crate::types::DocumentStylesheet::Inline { css } => {
                    doc.stylesheet.parse_and_add_with_base(css, &doc.base_url);
                }
                crate::types::DocumentStylesheet::Linked { href, media } => {
                    let abs = resolve_css_url(base_url, href);
                    if let Some(css) = fetched_map.get(&abs) {
                        doc.stylesheet
                            .parse_and_add_with_base_media(css, &abs, media);
                    }
                }
            }
        }
    } else {
        css_results.sort_by_key(|(idx, _, _, _)| *idx);
        for (_, css_url, sheet, media) in &css_results {
            if !sheet.is_empty() {
                doc.stylesheet
                    .parse_and_add_with_base_media(sheet, css_url, media);
            }
        }
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

    // Post-layout: load background images (layout may re-run cascade with viewport)
    svg::load_background_images(&mut doc.root, &doc.base_url.clone());
    start_async_image_fetches(&mut doc);
    // Fire DOMContentLoaded — listeners registered before load_html can react.
    let mut evt = dom::HtmlEvent::new(dom::HtmlEventType::DOMContentLoaded);
    evt.target = doc.root.node_id;
    doc.dispatch_input_event(evt);
    doc.svg_trigger_projected_tree_event("DOMContentLoaded");
    doc.svg_trigger_projected_tree_event("load");
    doc
}

/// Walk the DOM tree, find all <img> nodes with a remote `resolved_src`,
/// fire off parallel fetch threads, store channel on Document for async polling.
fn start_async_image_fetches(doc: &mut types::Document) {
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
        std::thread::spawn(move || {
            let result = http_client_lenient()
                .get(&url)
                .header("Sec-Fetch-Dest", "image")
                .send()
                .ok()
                .and_then(|r| r.bytes().ok())
                .and_then(|bytes| html::decode_image_bytes_ex(&bytes));
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
        if url.starts_with("http://") || url.starts_with("https://") {
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
        if url.starts_with("http://") || url.starts_with("https://") {
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
        if url.starts_with("http://") || url.starts_with("https://") {
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
