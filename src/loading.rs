//! Document loading utilities owned by webcore.
//!
//! Hosts should ask webcore to navigate and then repaint when events arrive;
//! they should not need to know how to fetch, cache, decode, or progressively
//! surface document bytes.

use std::io::Read;
use std::sync::{Arc, Mutex};

struct ByteFetchState {
    result: Mutex<Option<Result<Arc<Vec<u8>>, String>>>,
    done: std::sync::Condvar,
}

static BYTE_FETCH_IN_FLIGHT: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, Arc<ByteFetchState>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub const DEFAULT_NEW_TAB_HTML: &str =
    "<!doctype html><title>New Tab</title><body></body>";

#[derive(Clone, Debug)]
pub struct PageLoadOptions {
    pub cache_dir: Option<String>,
    pub emit_preview: bool,
    pub preview_after_bytes: usize,
    pub preview_interval_bytes: usize,
    pub load_images: bool,
}

impl Default for PageLoadOptions {
    fn default() -> Self {
        Self {
            cache_dir: None,
            emit_preview: true,
            preview_after_bytes: 16 * 1024,
            preview_interval_bytes: 128 * 1024,
            load_images: true,
        }
    }
}

pub struct PageSession {
    options: PageLoadOptions,
}

#[derive(Default)]
struct PreloadState {
    scheduled: std::collections::HashSet<String>,
}

pub enum PageSessionEvent {
    Page {
        url: String,
        doc: Box<crate::Document>,
        preview: bool,
    },
}

impl PageSession {
    pub fn new(options: PageLoadOptions) -> Self {
        Self { options }
    }

    pub fn navigate<F>(
        &self,
        url: String,
        viewport_w: f32,
        viewport_h: f32,
        mut on_event: F,
    ) where
        F: FnMut(PageSessionEvent) + Send + 'static,
    {
        let options = self.options.clone();
        std::thread::spawn(move || {
            let mut renderer = crate::Renderer::new();
            if url.starts_with("about:") {
                let doc = build_page_document(
                    &mut renderer,
                    DEFAULT_NEW_TAB_HTML,
                    &url,
                    viewport_w,
                    viewport_h,
                    &options,
                    false,
                );
                on_event(PageSessionEvent::Page {
                    url,
                    doc: Box::new(doc),
                    preview: false,
                });
                return;
            }

            let loading_html =
                "<!doctype html><title>Loading</title><body></body>";
            let loading_doc = build_page_document(
                &mut renderer,
                loading_html,
                &url,
                viewport_w,
                viewport_h,
                &PageLoadOptions {
                    emit_preview: false,
                    load_images: false,
                    ..options.clone()
                },
                true,
            );
            on_event(PageSessionEvent::Page {
                url: url.clone(),
                doc: Box::new(loading_doc),
                preview: true,
            });

            let (tx, rx) = std::sync::mpsc::channel::<(String, String, bool)>();
            let fetch_url = url.clone();
            let fetch_options = options.clone();
            std::thread::spawn(move || {
                let result = load_document_progressive(&fetch_url, &fetch_options, |event_url, html| {
                    let _ = tx.send((event_url, html, true));
                });
                match result {
                    Ok((html, final_url)) => {
                        let _ = tx.send((final_url, html, false));
                    }
                    Err(e) => {
                        let html = error_page(&fetch_url, &e);
                        let _ = tx.send((fetch_url, html, false));
                    }
                }
            });

            let mut last_preview = None::<std::time::Instant>;
            let min_preview_gap = std::time::Duration::from_millis(80);
            while let Ok(mut event) = rx.recv() {
                if event.2 {
                    let preview_settle = std::time::Duration::from_millis(8);
                    while let Ok(next) = rx.recv_timeout(preview_settle) {
                        event = next;
                        if !event.2 {
                            break;
                        }
                    }
                }
                if !event.2 {
                    while let Ok(next) = rx.try_recv() {
                        event = next;
                        if !event.2 {
                            break;
                        }
                    }
                }
                let (event_url, html, preview) = event;
                if preview {
                    if last_preview
                        .is_some_and(|instant| instant.elapsed() < min_preview_gap)
                    {
                        continue;
                    }
                    last_preview = Some(std::time::Instant::now());
                }
                let doc = build_page_document(
                    &mut renderer,
                    &html,
                    &event_url,
                    viewport_w,
                    viewport_h,
                    &options,
                    preview,
                );
                on_event(PageSessionEvent::Page {
                    url: event_url,
                    doc: Box::new(doc),
                    preview,
                });
            }
        });
    }
}

#[derive(Clone, Debug)]
pub enum PageLoadEvent {
    Preview { url: String, html: String },
    Complete { url: String, html: String },
}

pub fn spawn_page_load<F>(url: String, options: PageLoadOptions, mut on_event: F)
where
    F: FnMut(PageLoadEvent) + Send + 'static,
{
    std::thread::spawn(move || {
        let event = match load_document_progressive(&url, &options, |url, html| {
            on_event(PageLoadEvent::Preview { url, html });
        }) {
            Ok((html, final_url)) => PageLoadEvent::Complete {
                url: final_url,
                html,
            },
            Err(e) => PageLoadEvent::Complete {
                url: url.clone(),
                html: error_page(&url, &e),
            },
        };
        on_event(event);
    });
}

fn build_page_document(
    renderer: &mut crate::Renderer,
    html: &str,
    url: &str,
    viewport_w: f32,
    viewport_h: f32,
    options: &PageLoadOptions,
    preview: bool,
) -> crate::Document {
    let cache_dir_for_css = options.cache_dir.clone();
    let cache_dir_for_stream_css = options.cache_dir.clone();
    let cache_dir_for_images = options.cache_dir.clone();
    let image_loader = if options.load_images && !preview {
        cache_dir_for_images.map(|cache_dir| {
            std::sync::Arc::new(move |src: &str| {
                cached_fetch_bytes(src, &cache_dir)
                    .ok()
                    .and_then(|bytes| crate::html::decode_image_bytes_ex(&bytes))
            }) as crate::ImageLoader
        })
    } else {
        None
    };

    renderer.load_html_with_base_and_resource_loaders_css_wait(
        html,
        url,
        viewport_w,
        viewport_h,
        std::sync::Arc::new(move |css_url| {
            fetch_text_resource(css_url, cache_dir_for_css.as_deref())
        }),
        Some(std::sync::Arc::new(move |css_url, emit| {
            fetch_text_resource_streaming(css_url, cache_dir_for_stream_css.as_deref(), emit)
        })),
        image_loader,
        options.load_images && !preview,
        std::time::Duration::ZERO,
    )
}

fn scan_html_chunk_for_resources(
    parser: &mut crate::StreamingParser,
    chunk: &[u8],
    options: &PageLoadOptions,
    state: &Arc<Mutex<PreloadState>>,
) {
    for mutation in parser.feed(chunk) {
        if let crate::DomMutation::ResourceHint { kind, url } = mutation {
            schedule_preload(kind, url, options, state);
        }
    }
}

fn schedule_preload(
    kind: crate::ResourceKind,
    url: String,
    options: &PageLoadOptions,
    state: &Arc<Mutex<PreloadState>>,
) {
    let key = format!("{kind:?}\n{url}");
    {
        let Ok(mut state) = state.lock() else {
            return;
        };
        if !state.scheduled.insert(key) {
            return;
        }
    }

    match kind {
        crate::ResourceKind::Stylesheet => {
            let cache_dir = options.cache_dir.clone();
            crate::spawn_css_resource_task(move || {
                let _ = crate::fetch_text_resource_streaming(&url, cache_dir.as_deref(), |_| {});
            });
        }
        crate::ResourceKind::Image if options.load_images => {
            let cache_dir = options.cache_dir.clone();
            crate::spawn_image_resource_task(move || {
                let loader = move |src: &str| {
                    let bytes = match cache_dir.as_deref() {
                        Some(cache_dir) => cached_fetch_bytes(src, cache_dir).ok(),
                        None => fetch_bytes(src).ok(),
                    }?;
                    crate::html::decode_image_bytes_ex(&bytes)
                };
                let _ = crate::cached_decoded_image(&url, Some(&loader));
            });
        }
        _ => {}
    }
}

pub fn load_document_progressive<F>(
    url: &str,
    options: &PageLoadOptions,
    mut preview: F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    if url.starts_with("about:") {
        return Ok((
            "<!doctype html><title>New Tab</title><body></body>".to_string(),
            url.to_string(),
        ));
    }
    if let Some(path) = url.strip_prefix("file://") {
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("failed to read file {path}: {e}"))?;
        let mut parser = crate::StreamingParser::new(url);
        let state = Arc::new(Mutex::new(PreloadState::default()));
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut text = String::new();
        let mut bytes_read = 0usize;
        let mut sent_preview = false;
        let mut next_preview_at = options.preview_after_bytes.max(1024);
        let preview_interval = options.preview_interval_bytes.max(32 * 1024);
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("failed to read file {path}: {e}"))?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            scan_html_chunk_for_resources(&mut parser, chunk, options, &state);
            bytes_read += n;
            let decoded = decode_streaming_utf8(&mut decoder, chunk, false);
            text.push_str(&decoded);
            if options.emit_preview && bytes_read >= next_preview_at {
                preview(url.to_string(), text.clone());
                sent_preview = true;
                while next_preview_at <= bytes_read {
                    next_preview_at = next_preview_at.saturating_add(preview_interval);
                }
            }
        }
        let tail = decode_streaming_utf8(&mut decoder, &[], true);
        text.push_str(&tail);
        if options.emit_preview && !sent_preview && !text.is_empty() {
            preview(url.to_string(), text.clone());
        }
        return Ok((text, url.to_string()));
    }
    if let Some(cache_dir) = options.cache_dir.as_deref() {
        return cached_fetch_document(url, cache_dir, options, &mut preview);
    }
    fetch_document_streaming(url, options, &mut preview)
}

pub fn fetch_text_resource(url: &str, cache_dir: Option<&str>) -> Result<String, String> {
    if let Some(cache_dir) = cache_dir {
        let path = url_cache_path(url, cache_dir);
        if let Ok(data) = std::fs::read(&path) {
            return Ok(decode_body(&data));
        }
        let text = fetch_text_resource_uncached(url)?;
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(&path, text.as_bytes());
        return Ok(text);
    }
    fetch_text_resource_uncached(url)
}

pub fn fetch_text_resource_streaming<F>(
    url: &str,
    cache_dir: Option<&str>,
    mut on_chunk: F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    if let Some(cache_dir) = cache_dir {
        let path = url_cache_path(url, cache_dir);
        if let Ok(mut file) = std::fs::File::open(&path) {
            stream_file_text(&mut file, &mut decoder, &mut on_chunk)?;
            return Ok(());
        }
        let mut body = Vec::new();
        fetch_text_resource_uncached_streaming(url, |bytes| {
            body.extend_from_slice(bytes);
            let text = decode_streaming_utf8(&mut decoder, bytes, false);
            if !text.is_empty() {
                on_chunk(&text);
            }
        })?;
        let tail = decode_streaming_utf8(&mut decoder, &[], true);
        if !tail.is_empty() {
            on_chunk(&tail);
        }
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(&path, &body);
        return Ok(());
    }

    fetch_text_resource_uncached_streaming(url, |bytes| {
        let text = decode_streaming_utf8(&mut decoder, bytes, false);
        if !text.is_empty() {
            on_chunk(&text);
        }
    })?;
    let tail = decode_streaming_utf8(&mut decoder, &[], true);
    if !tail.is_empty() {
        on_chunk(&tail);
    }
    Ok(())
}

pub fn cached_fetch_bytes(url: &str, cache_dir: &str) -> Result<Vec<u8>, String> {
    let key = format!("{cache_dir}\n{url}");
    let state = {
        let mut in_flight = BYTE_FETCH_IN_FLIGHT
            .lock()
            .map_err(|_| "byte fetch lock poisoned".to_string())?;
        if let Some(state) = in_flight.get(&key) {
            state.clone()
        } else {
            let state = Arc::new(ByteFetchState {
                result: Mutex::new(None),
                done: std::sync::Condvar::new(),
            });
            in_flight.insert(key.clone(), state.clone());
            drop(in_flight);
            let result = cached_fetch_bytes_uncached(url, cache_dir).map(Arc::new);
            if let Ok(mut slot) = state.result.lock() {
                *slot = Some(result);
                state.done.notify_all();
            }
            if let Ok(mut in_flight) = BYTE_FETCH_IN_FLIGHT.lock() {
                in_flight.remove(&key);
            }
            let slot = state
                .result
                .lock()
                .map_err(|_| "byte fetch result lock poisoned".to_string())?;
            return match slot.as_ref().expect("byte fetch result set") {
                Ok(bytes) => Ok((**bytes).clone()),
                Err(err) => Err(err.clone()),
            };
        }
    };
    let mut slot = state
        .result
        .lock()
        .map_err(|_| "byte fetch result lock poisoned".to_string())?;
    while slot.is_none() {
        slot = state
            .done
            .wait(slot)
            .map_err(|_| "byte fetch wait lock poisoned".to_string())?;
    }
    match slot.as_ref().expect("byte fetch result set") {
        Ok(bytes) => Ok((**bytes).clone()),
        Err(err) => Err(err.clone()),
    }
}

fn cached_fetch_bytes_uncached(url: &str, cache_dir: &str) -> Result<Vec<u8>, String> {
    let path = url_cache_path(url, cache_dir);
    if let Ok(data) = std::fs::read(&path) {
        return Ok(data);
    }
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        if let Some(slash) = after.find('/') {
            let old_url = format!("{}/.{}", &url[..scheme_end + 3], &after[slash..]);
            let old_path = url_cache_path(&old_url, cache_dir);
            if let Ok(data) = std::fs::read(&old_path) {
                let _ = std::fs::write(&path, &data);
                return Ok(data);
            }
        }
    }
    let data = fetch_bytes(url)?;
    let _ = std::fs::create_dir_all(cache_dir);
    let _ = std::fs::write(&path, &data);
    Ok(data)
}

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let do_fetch = |client: &reqwest::blocking::Client| -> Result<Vec<u8>, String> {
        let resp = client
            .get(url)
            .header(
                "Accept",
                "image/avif,image/webp,image/apng,image/*,*/*;q=0.8",
            )
            .header("Sec-Fetch-Dest", "image")
            .header("Sec-Fetch-Mode", "no-cors")
            .header("Sec-Fetch-Site", "cross-site")
            .send()
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    };
    match do_fetch(&crate::http_client()) {
        Ok(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => do_fetch(&crate::http_client_lenient()),
    }
}

fn fetch_text_resource_uncached(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read_to_string(path).map_err(|e| e.to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return std::fs::read_to_string(url).map_err(|e| e.to_string());
    }
    let do_fetch = |client: &reqwest::blocking::Client| -> Result<String, String> {
        let resp = client
            .get(url)
            .header("Accept", "text/css,*/*;q=0.1")
            .header("Sec-Fetch-Dest", "style")
            .header("Sec-Fetch-Mode", "no-cors")
            .send()
            .map_err(|e| e.to_string())?;
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        Ok(decode_body(&bytes))
    };
    match do_fetch(&crate::http_client()) {
        Ok(text) if !text.is_empty() => Ok(text),
        _ => do_fetch(&crate::http_client_lenient()),
    }
}

fn fetch_text_resource_uncached_streaming<F>(url: &str, mut on_chunk: F) -> Result<(), String>
where
    F: FnMut(&[u8]),
{
    if let Some(path) = url.strip_prefix("file://") {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        for chunk in data.chunks(16 * 1024) {
            on_chunk(chunk);
        }
        return Ok(());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let data = std::fs::read(url).map_err(|e| e.to_string())?;
        for chunk in data.chunks(16 * 1024) {
            on_chunk(chunk);
        }
        return Ok(());
    }
    let do_fetch = |client: &reqwest::blocking::Client,
                    on_chunk: &mut dyn FnMut(&[u8])|
     -> Result<(), String> {
        let mut resp = client
            .get(url)
            .header("Accept", "text/css,*/*;q=0.1")
            .header("Sec-Fetch-Dest", "style")
            .header("Sec-Fetch-Mode", "no-cors")
            .send()
            .map_err(|e| e.to_string())?;
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            on_chunk(&buf[..n]);
        }
        Ok(())
    };
    let mut saw = false;
    let mut first = |bytes: &[u8]| {
        saw = true;
        on_chunk(bytes);
    };
    match do_fetch(&crate::http_client(), &mut first) {
        Ok(()) => Ok(()),
        Err(err) if saw => Err(err),
        Err(_) => do_fetch(&crate::http_client_lenient(), &mut on_chunk),
    }
}

fn stream_file_text<F>(
    file: &mut std::fs::File,
    decoder: &mut encoding_rs::Decoder,
    on_chunk: &mut F,
) -> Result<(), String>
where
    F: FnMut(&str),
{
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let text = decode_streaming_utf8(decoder, &buf[..n], false);
        if !text.is_empty() {
            on_chunk(&text);
        }
    }
    let tail = decode_streaming_utf8(decoder, &[], true);
    if !tail.is_empty() {
        on_chunk(&tail);
    }
    Ok(())
}

fn decode_streaming_utf8(
    decoder: &mut encoding_rs::Decoder,
    bytes: &[u8],
    last: bool,
) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_add(16));
    let mut input = bytes;
    loop {
        let before_in = input.len();
        let before_out = out.len();
        let (result, read, _) = decoder.decode_to_string(input, &mut out, last);
        input = &input[read..];
        match result {
            encoding_rs::CoderResult::InputEmpty => break,
            encoding_rs::CoderResult::OutputFull => {
                let remaining = input.len().max(1024);
                out.reserve(remaining);
                if input.len() == before_in && out.len() == before_out {
                    break;
                }
            }
        }
    }
    out
}

fn fetch_document_streaming<F>(
    url: &str,
    options: &PageLoadOptions,
    preview: &mut F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    let mut do_fetch =
        |client: &reqwest::blocking::Client| -> Result<(String, String, bool), (String, bool)> {
            let mut resp = client
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
                .map_err(|e| (e.to_string(), false))?;
            let final_url = resp.url().to_string();
            let status = resp.status();
            if !status.is_success() {
                return Err((format!("HTTP {status} loading {final_url}"), false));
            }
            let mut parser = crate::StreamingParser::new(&final_url);
            let preload_state = Arc::new(Mutex::new(PreloadState::default()));
            let mut html = String::new();
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let mut bytes_read = 0usize;
            let mut buf = [0u8; 16 * 1024];
            let mut sent_preview = false;
            let mut next_preview_at = options.preview_after_bytes.max(1024);
            let preview_interval = options.preview_interval_bytes.max(32 * 1024);
            let mut saw_streamed_bytes = false;
            loop {
                let n = resp
                    .read(&mut buf)
                    .map_err(|e| (e.to_string(), saw_streamed_bytes))?;
                if n == 0 {
                    break;
                }
                saw_streamed_bytes = true;
                scan_html_chunk_for_resources(
                    &mut parser,
                    &buf[..n],
                    options,
                    &preload_state,
                );
                bytes_read += n;
                let text = decode_streaming_utf8(&mut decoder, &buf[..n], false);
                html.push_str(&text);
                if options.emit_preview && bytes_read >= next_preview_at {
                    preview(final_url.clone(), html.clone());
                    sent_preview = true;
                    while next_preview_at <= bytes_read {
                        next_preview_at = next_preview_at.saturating_add(preview_interval);
                    }
                }
            }
            let tail = decode_streaming_utf8(&mut decoder, &[], true);
            html.push_str(&tail);
            if options.emit_preview && !sent_preview && !html.is_empty() {
                preview(final_url.clone(), html.clone());
            }
            Ok((html, final_url, saw_streamed_bytes))
        };

    match do_fetch(&crate::http_client()) {
        Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
        Err((err, true)) => Err(err),
        Err((_, false)) => match do_fetch(&crate::http_client_lenient()) {
            Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
            Ok((_, final_url, _)) => Err(format!("empty streamed document from {final_url}")),
            Err((err, _)) => Err(err),
        },
        Ok((_, _, true)) => Err("empty streamed document".to_string()),
        Ok((_, _, false)) => match do_fetch(&crate::http_client_lenient()) {
            Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
            Ok((_, final_url, _)) => Err(format!("empty streamed document from {final_url}")),
            Err((err, _)) => Err(err),
        },
    }
}

fn cached_fetch_document<F>(
    url: &str,
    cache_dir: &str,
    options: &PageLoadOptions,
    preview: &mut F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    let path = url_cache_path(url, cache_dir);
    let url_path = format!("{}.url", path.display());
    if let Ok(mut file) = std::fs::File::open(&path) {
        let final_url = std::fs::read_to_string(&url_path)
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| url.to_string());
        let mut parser = crate::StreamingParser::new(&final_url);
        let preload_state = Arc::new(Mutex::new(PreloadState::default()));
        let mut html = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut bytes_read = 0usize;
        let mut buf = [0u8; 16 * 1024];
        let mut sent_preview = false;
        let mut next_preview_at = options.preview_after_bytes.max(1024);
        let preview_interval = options.preview_interval_bytes.max(32 * 1024);
        loop {
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            scan_html_chunk_for_resources(&mut parser, &buf[..n], options, &preload_state);
            bytes_read += n;
            let text = decode_streaming_utf8(&mut decoder, &buf[..n], false);
            html.push_str(&text);
            if options.emit_preview && bytes_read >= next_preview_at {
                preview(final_url.clone(), html.clone());
                sent_preview = true;
                while next_preview_at <= bytes_read {
                    next_preview_at = next_preview_at.saturating_add(preview_interval);
                }
            }
        }
        let tail = decode_streaming_utf8(&mut decoder, &[], true);
        html.push_str(&tail);
        if options.emit_preview && !sent_preview && !html.is_empty() {
            preview(final_url.clone(), html.clone());
        }
        return Ok((html, final_url));
    }
    let (body, final_url) = fetch_document_streaming(
        url,
        &PageLoadOptions {
            cache_dir: None,
            emit_preview: options.emit_preview,
            preview_after_bytes: options.preview_after_bytes,
            preview_interval_bytes: options.preview_interval_bytes,
            load_images: options.load_images,
        },
        preview,
    )?;
    let _ = std::fs::create_dir_all(cache_dir);
    let _ = std::fs::write(&path, body.as_bytes());
    if final_url != url {
        let _ = std::fs::write(&url_path, final_url.as_bytes());
    }
    Ok((body, final_url))
}

fn url_cache_path(url: &str, cache_dir: &str) -> std::path::PathBuf {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let hash = hasher.finish();
    let suffix: String = url
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.')
        .take(40)
        .collect();
    std::path::PathBuf::from(cache_dir).join(format!("{hash:016x}_{suffix}"))
}

fn decode_body(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| {
        let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
        cow.into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_session_emits_loading_preview_before_final_result() {
        let session = PageSession::new(PageLoadOptions {
            emit_preview: false,
            load_images: false,
            ..Default::default()
        });
        let (tx, rx) = std::sync::mpsc::channel();
        session.navigate(
            "file:///definitely/not/a/real/webcore/file.html".to_string(),
            320.0,
            240.0,
            move |event| {
                let PageSessionEvent::Page { preview, doc, .. } = event;
                let _ = tx.send((preview, doc.title));
            },
        );

        let first = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("loading preview event");
        assert!(first.0, "first event should be a non-blocking preview");
        assert_eq!(first.1, "Loading");

        let final_event = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("final event");
        assert!(!final_event.0, "second event should be final");
    }

    #[test]
    fn file_document_loading_emits_streaming_preview() {
        let path = std::env::temp_dir().join(format!(
            "webcore-streaming-preview-{}.html",
            std::process::id()
        ));
        let html = format!(
            "<!doctype html><title>x</title><body>{}</body>",
            "hello ".repeat(400)
        );
        std::fs::write(&path, html).unwrap();
        let url = format!("file://{}", path.display());
        let mut previews = 0usize;
        let result = load_document_progressive(
            &url,
            &PageLoadOptions {
                emit_preview: true,
                preview_after_bytes: 128,
                preview_interval_bytes: usize::MAX / 4,
                load_images: false,
                ..Default::default()
            },
            |preview_url, preview_html| {
                assert_eq!(preview_url, url);
                assert!(!preview_html.is_empty());
                previews += 1;
            },
        );
        let _ = std::fs::remove_file(&path);
        let (final_html, final_url) = result.expect("file load should complete");
        assert_eq!(final_url, url);
        assert!(final_html.contains("hello"));
        assert_eq!(previews, 1, "file loading should emit one early preview");
    }
}

fn error_page(url: &str, err: &str) -> String {
    format!(
        "<!doctype html><title>Load error</title><body><h2>Could not load {}</h2><pre>{}</pre></body>",
        escape_html(url),
        escape_html(err)
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
