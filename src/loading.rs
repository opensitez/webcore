//! Document loading utilities owned by webcore.
//!
//! Hosts should ask webcore to navigate and then repaint when events arrive;
//! they should not need to know how to fetch, cache, decode, or progressively
//! surface document bytes.

use std::io::Read;
use std::sync::{Arc, Mutex};

const RAW_RESOURCE_CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;

struct RawResourceCache {
    entries: std::collections::HashMap<String, (Arc<Vec<u8>>, u64)>,
    order: std::collections::VecDeque<(String, u64)>,
    bytes: usize,
    next_generation: u64,
}

impl RawResourceCache {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            bytes: 0,
            next_generation: 1,
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<Vec<u8>>> {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let data = self
            .entries
            .get_mut(key)
            .and_then(|(data, entry_generation)| {
                if data.is_empty() {
                    None
                } else {
                    *entry_generation = generation;
                    Some(data.clone())
                }
            })?;
        self.order.push_back((key.to_string(), generation));
        self.compact_order_if_needed();
        Some(data)
    }

    fn insert(&mut self, key: String, data: Arc<Vec<u8>>) {
        if data.is_empty() || data.len() > RAW_RESOURCE_CACHE_MAX_BYTES {
            return;
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        if let Some((old, _)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old.len());
        }
        while self.bytes.saturating_add(data.len()) > RAW_RESOURCE_CACHE_MAX_BYTES {
            let Some((oldest, oldest_generation)) = self.order.pop_front() else {
                break;
            };
            let should_remove = self
                .entries
                .get(&oldest)
                .is_some_and(|(_, entry_generation)| *entry_generation == oldest_generation);
            if should_remove && let Some((old, _)) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(old.len());
            }
        }
        self.bytes = self.bytes.saturating_add(data.len());
        self.order.push_back((key.clone(), generation));
        self.entries.insert(key, (data, generation));
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
                .is_some_and(|(_, entry_generation)| entry_generation == generation)
        });
    }
}

static RAW_RESOURCE_CACHE: std::sync::LazyLock<Mutex<RawResourceCache>> =
    std::sync::LazyLock::new(|| Mutex::new(RawResourceCache::new()));

pub(crate) fn raw_resource_cache_stats() -> crate::CacheMemoryStats {
    RAW_RESOURCE_CACHE
        .lock()
        .map(|cache| crate::CacheMemoryStats {
            entries: cache.entries.len(),
            bytes: cache.bytes,
        })
        .unwrap_or_default()
}

enum CacheWrite {
    Bytes {
        path: std::path::PathBuf,
        data: Arc<Vec<u8>>,
    },
    Text {
        path: std::path::PathBuf,
        text: Arc<String>,
    },
}

static CACHE_WRITE_TX: std::sync::LazyLock<std::sync::mpsc::Sender<CacheWrite>> =
    std::sync::LazyLock::new(|| {
        let (tx, rx) = std::sync::mpsc::channel::<CacheWrite>();
        std::thread::Builder::new()
            .name("webcore-cache-writer".to_string())
            .spawn(move || {
                while let Ok(write) = rx.recv() {
                    match write {
                        CacheWrite::Bytes { path, data } => {
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(path, data.as_slice());
                        }
                        CacheWrite::Text { path, text } => {
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(path, text.as_bytes());
                        }
                    }
                }
            })
            .expect("webcore cache writer");
        tx
    });

fn raw_cache_key(cache_dir: &str, url: &str) -> String {
    format!("{cache_dir}\n{url}")
}

fn raw_cache_get(key: &str) -> Option<Arc<Vec<u8>>> {
    RAW_RESOURCE_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(key))
}

fn raw_cache_put(key: String, data: Arc<Vec<u8>>) {
    if let Ok(mut cache) = RAW_RESOURCE_CACHE.lock() {
        cache.insert(key, data);
    }
}

fn enqueue_cache_bytes(path: std::path::PathBuf, data: Arc<Vec<u8>>) {
    let _ = CACHE_WRITE_TX.send(CacheWrite::Bytes { path, data });
}

fn enqueue_cache_text(path: std::path::PathBuf, text: Arc<String>) {
    let _ = CACHE_WRITE_TX.send(CacheWrite::Text { path, text });
}

struct ByteFetchState {
    result: Mutex<Option<Result<Arc<Vec<u8>>, String>>>,
    done: std::sync::Condvar,
}

static BYTE_FETCH_IN_FLIGHT: std::sync::LazyLock<
    Mutex<std::collections::HashMap<String, Arc<ByteFetchState>>>,
> = std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub const DEFAULT_NEW_TAB_HTML: &str = "<!doctype html><title>New Tab</title><body></body>";

fn should_emit_early_preview(html: &str, sent_preview: bool) -> bool {
    if sent_preview || html.len() < 256 {
        return false;
    }
    let start = html.len().saturating_sub(8192);
    let tail = html[start..].to_ascii_lowercase();
    tail.contains("</style")
        || tail.contains("</head")
        || tail.contains("<body")
        || html.len() >= 4 * 1024
}

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

    pub fn navigate<F>(&self, url: String, viewport_w: f32, viewport_h: f32, mut on_event: F)
    where
        F: FnMut(PageSessionEvent) + Send + 'static,
    {
        let options = self.options.clone();
        std::thread::spawn(move || {
            if url.starts_with("about:") {
                let mut frame = crate::EngineFrame::empty(viewport_w, viewport_h);
                frame.set_cache_dir(options.cache_dir.clone());
                frame.start_streaming(&url);
                frame.feed_html_chunk(DEFAULT_NEW_TAB_HTML.as_bytes());
                frame.finish_loading();
                frame.update_frame();
                on_event(PageSessionEvent::Page {
                    url,
                    doc: Box::new(frame.doc.clone()),
                    preview: false,
                });
                return;
            }

            let loading_html = "<!doctype html><title>Loading</title><body></body>";
            let mut loading_renderer = crate::Renderer::new();
            let loading_doc = build_page_document(
                &mut loading_renderer,
                loading_html,
                &url,
                viewport_w,
                viewport_h,
                &options,
                true,
            );
            on_event(PageSessionEvent::Page {
                url: url.clone(),
                doc: Box::new(loading_doc),
                preview: true,
            });

            let mut frame = crate::EngineFrame::empty(viewport_w, viewport_h);
            frame.set_cache_dir(options.cache_dir.clone());
            frame.start_streaming(&url);
            let mut streamed_bytes = 0usize;
            let mut last_preview = None::<std::time::Instant>;
            let min_preview_gap = std::time::Duration::from_millis(32);
            let mut stream_options = options.clone();
            stream_options.emit_preview = true;
            let result =
                load_document_streaming_chunks(&url, &stream_options, |event_url, chunk| {
                    if streamed_bytes == 0 && frame.doc.base_url != event_url {
                        frame.start_streaming(&event_url);
                    }
                    streamed_bytes = streamed_bytes.saturating_add(chunk.len());
                    frame.feed_html_chunk(chunk.as_bytes());
                    if options.emit_preview {
                        let now = std::time::Instant::now();
                        let can_emit = last_preview
                            .map(|last| now.saturating_duration_since(last) >= min_preview_gap)
                            .unwrap_or(true);
                        if can_emit {
                            frame.update_frame();
                            on_event(PageSessionEvent::Page {
                                url: event_url,
                                doc: Box::new(frame.doc.clone()),
                                preview: true,
                            });
                            last_preview = Some(now);
                        }
                    }
                });
            let final_url = match result {
                Ok((_, final_url)) => final_url,
                Err(e) => {
                    let html = error_page(&url, &e);
                    if streamed_bytes == 0 {
                        frame.start_streaming(&url);
                    }
                    frame.feed_html_chunk(html.as_bytes());
                    url.clone()
                }
            };
            frame.finish_loading();
            frame.update_frame();
            on_event(PageSessionEvent::Page {
                url: final_url,
                doc: Box::new(frame.doc.clone()),
                preview: false,
            });
        });
    }
}

#[derive(Clone, Debug)]
pub enum PageLoadEvent {
    Chunk { url: String, html: String },
    Complete { url: String },
}

pub fn spawn_page_load<F>(url: String, options: PageLoadOptions, mut on_event: F)
where
    F: FnMut(PageLoadEvent) + Send + 'static,
{
    std::thread::spawn(move || {
        let event = match load_document_streaming_chunks(&url, &options, |url, html| {
            on_event(PageLoadEvent::Chunk { url, html });
        }) {
            Ok((_html, final_url)) => PageLoadEvent::Complete { url: final_url },
            Err(e) => {
                on_event(PageLoadEvent::Chunk {
                    url: url.clone(),
                    html: error_page(&url, &e),
                });
                PageLoadEvent::Complete { url: url.clone() }
            }
        };
        on_event(event);
    });
}

fn load_document_streaming_chunks<F>(
    url: &str,
    options: &PageLoadOptions,
    mut on_chunk: F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    if url.starts_with("about:") {
        let html = "<!doctype html><title>New Tab</title><body></body>".to_string();
        on_chunk(url.to_string(), html.clone());
        return Ok((html, url.to_string()));
    }
    if let Some(path) = url.strip_prefix("file://") {
        let file =
            std::fs::File::open(path).map_err(|e| format!("failed to read file {path}: {e}"))?;
        return stream_document_reader(file, url.to_string(), options, on_chunk);
    }
    if let Some(cache_dir) = options.cache_dir.as_deref() {
        return cached_fetch_document_streaming_chunks(url, cache_dir, options, on_chunk);
    }
    fetch_document_streaming_chunks(url, options, on_chunk)
}

fn stream_document_reader<R, F>(
    mut reader: R,
    final_url: String,
    options: &PageLoadOptions,
    mut on_chunk: F,
) -> Result<(String, String), String>
where
    R: Read,
    F: FnMut(String, String),
{
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut html = String::new();
    let mut pending_emit = String::new();
    let mut sent_preview = false;
    let mut next_preview_at = options.preview_after_bytes.max(1024);
    let preview_interval = options.preview_interval_bytes.max(32 * 1024);
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        let text = decode_streaming_utf8(&mut decoder, &buf[..n], false);
        if !text.is_empty() {
            html.push_str(&text);
            if options.emit_preview {
                pending_emit.push_str(&text);
                if html.len() >= next_preview_at || should_emit_early_preview(&html, sent_preview) {
                    on_chunk(final_url.clone(), std::mem::take(&mut pending_emit));
                    sent_preview = true;
                    while next_preview_at <= html.len() {
                        next_preview_at = next_preview_at.saturating_add(preview_interval);
                    }
                }
            }
        }
    }
    let tail = decode_streaming_utf8(&mut decoder, &[], true);
    if !tail.is_empty() {
        html.push_str(&tail);
        if options.emit_preview {
            pending_emit.push_str(&tail);
        }
    }
    if options.emit_preview && !pending_emit.is_empty() {
        on_chunk(final_url.clone(), pending_emit);
    }
    Ok((html, final_url))
}

fn stream_document_bytes<F>(
    data: &[u8],
    final_url: String,
    options: &PageLoadOptions,
    on_chunk: F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    stream_document_reader(std::io::Cursor::new(data), final_url, options, on_chunk)
}

fn cached_fetch_document_streaming_chunks<F>(
    url: &str,
    cache_dir: &str,
    options: &PageLoadOptions,
    mut on_chunk: F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    let key = raw_cache_key(cache_dir, url);
    let path = url_cache_path(url, cache_dir);
    let url_path = format!("{}.url", path.display());
    let final_url = || {
        std::fs::read_to_string(&url_path)
            .map(|s| s.trim().to_string())
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| url.to_string())
    };
    if let Some(data) = raw_cache_get(&key) {
        return stream_document_bytes(&data, final_url(), options, on_chunk);
    }
    if let Ok(file) = std::fs::File::open(&path) {
        let final_url = final_url();
        let (html, final_url) = stream_document_reader(file, final_url, options, &mut on_chunk)?;
        raw_cache_put(key, Arc::new(html.as_bytes().to_vec()));
        return Ok((html, final_url));
    }

    let (body, final_url) = fetch_document_streaming_chunks(
        url,
        &PageLoadOptions {
            cache_dir: None,
            emit_preview: options.emit_preview,
            preview_after_bytes: options.preview_after_bytes,
            preview_interval_bytes: options.preview_interval_bytes,
            load_images: options.load_images,
        },
        &mut on_chunk,
    )?;
    let body = Arc::new(body);
    raw_cache_put(key, Arc::new(body.as_bytes().to_vec()));
    enqueue_cache_text(path, body.clone());
    if final_url != url {
        enqueue_cache_text(
            std::path::PathBuf::from(url_path),
            Arc::new(final_url.clone()),
        );
    }
    Ok(((*body).clone(), final_url))
}

fn fetch_document_streaming_chunks<F>(
    url: &str,
    options: &PageLoadOptions,
    mut on_chunk: F,
) -> Result<(String, String), String>
where
    F: FnMut(String, String),
{
    let mut do_fetch = |client: &reqwest::blocking::Client,
                        on_chunk: &mut dyn FnMut(String, String)|
     -> Result<(String, String, bool), (String, bool)> {
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
            .map_err(|e| (e.to_string(), false))?;
        let final_url = resp.url().to_string();
        let status = resp.status();
        if !status.is_success() {
            return Err((format!("HTTP {status} loading {final_url}"), false));
        }
        let saw_streamed_bytes = true;
        stream_document_reader(resp, final_url, options, on_chunk)
            .map(|(html, final_url)| (html, final_url, saw_streamed_bytes))
            .map_err(|e| (e, saw_streamed_bytes))
    };

    match do_fetch(&crate::http_client(), &mut on_chunk) {
        Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
        Err((err, true)) => Err(err),
        Err((_, false)) => match do_fetch(&crate::http_client_lenient(), &mut on_chunk) {
            Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
            Ok((_, final_url, _)) => Err(format!("empty streamed document from {final_url}")),
            Err((err, _)) => Err(err),
        },
        Ok((_, _, true)) => Err("empty streamed document".to_string()),
        Ok((_, _, false)) => match do_fetch(&crate::http_client_lenient(), &mut on_chunk) {
            Ok((body, final_url, _)) if !body.is_empty() => Ok((body, final_url)),
            Ok((_, final_url, _)) => Err(format!("empty streamed document from {final_url}")),
            Err((err, _)) => Err(err),
        },
    }
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
                cached_fetch_bytes_arc(src, &cache_dir)
                    .ok()
                    .and_then(crate::html::decode_image_bytes_arc)
            }) as crate::ImageLoader
        })
    } else {
        None
    };
    let stylesheet_loader: crate::StylesheetLoader = if preview {
        std::sync::Arc::new(|_| Ok(String::new()))
    } else {
        std::sync::Arc::new(move |css_url| {
            fetch_text_resource(css_url, cache_dir_for_css.as_deref())
        })
    };
    let streaming_stylesheet_loader: Option<crate::StreamingStylesheetLoader> = if preview {
        None
    } else {
        Some(std::sync::Arc::new(move |css_url, emit| {
            fetch_text_resource_streaming(css_url, cache_dir_for_stream_css.as_deref(), emit)
        }))
    };

    crate::load_html_reusing_with_resource_loaders_and_wait_mode(
        html,
        url,
        viewport_w,
        viewport_h,
        renderer.component_registry.clone(),
        Some(renderer),
        Some(stylesheet_loader),
        streaming_stylesheet_loader,
        image_loader,
        options.load_images && !preview,
        !preview,
        true,
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
                let css_url = url.clone();
                let cache_key = format!("{css_url}\n");
                if crate::cached_parsed_stylesheet(&cache_key).is_some() {
                    return;
                }
                let loader: crate::StylesheetLoader = std::sync::Arc::new({
                    let cache_dir = cache_dir.clone();
                    move |css_url| crate::fetch_text_resource(css_url, cache_dir.as_deref())
                });
                let streaming_loader: crate::StreamingStylesheetLoader =
                    std::sync::Arc::new(move |css_url, emit| {
                        crate::fetch_text_resource_streaming(css_url, cache_dir.as_deref(), emit)
                    });
                let _ = crate::load_stylesheet_cached(
                    cache_key,
                    css_url,
                    String::new(),
                    loader,
                    Some(streaming_loader),
                    true,
                    |_| {},
                );
            });
        }
        crate::ResourceKind::Image if options.load_images => {
            let cache_dir = options.cache_dir.clone();
            crate::spawn_image_resource_task(move || {
                let loader = cache_dir.map(|cache_dir| {
                    std::sync::Arc::new(move |src: &str| {
                        let bytes = crate::loading::cached_fetch_bytes_arc(src, &cache_dir)?;
                        crate::html::decode_image_bytes_arc(bytes.clone()).ok_or_else(|| {
                            format!("unsupported image bytes: {} bytes from {src}", bytes.len())
                        })
                    })
                        as std::sync::Arc<
                            dyn Fn(&str) -> Result<crate::html::DecodedImage, String>
                                + Send
                                + Sync
                                + 'static,
                        >
                });
                let _ = crate::cached_decoded_image_result(&url, loader.as_deref());
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
    let mut stream_options = options.clone();
    stream_options.emit_preview = true;
    let mut parser = crate::StreamingParser::new(url);
    let state = Arc::new(Mutex::new(PreloadState::default()));
    let mut html = String::new();
    let mut bytes_read = 0usize;
    let mut sent_preview = false;
    let mut next_preview_at = options.preview_after_bytes.max(1024);
    let preview_interval = options.preview_interval_bytes.max(32 * 1024);

    load_document_streaming_chunks(url, &stream_options, |chunk_url, chunk| {
        scan_html_chunk_for_resources(&mut parser, chunk.as_bytes(), options, &state);
        bytes_read = bytes_read.saturating_add(chunk.len());
        html.push_str(&chunk);
        if options.emit_preview
            && (bytes_read >= next_preview_at || should_emit_early_preview(&html, sent_preview))
        {
            preview(chunk_url, html.clone());
            sent_preview = true;
            while next_preview_at <= bytes_read {
                next_preview_at = next_preview_at.saturating_add(preview_interval);
            }
        }
    })
    .map(|(final_html, final_url)| {
        if options.emit_preview && !sent_preview && !final_html.is_empty() {
            preview(final_url.clone(), final_html.clone());
        }
        (final_html, final_url)
    })
}

pub fn fetch_text_resource(url: &str, cache_dir: Option<&str>) -> Result<String, String> {
    if let Some(cache_dir) = cache_dir {
        let key = raw_cache_key(cache_dir, url);
        if let Some(data) = raw_cache_get(&key) {
            return Ok(decode_body(&data));
        }
        let path = url_cache_path(url, cache_dir);
        if let Ok(data) = std::fs::read(&path) {
            if data.is_empty() {
                let _ = std::fs::remove_file(&path);
            } else {
                let data = Arc::new(data);
                raw_cache_put(key, data.clone());
                return Ok(decode_body(&data));
            }
        }
        let text = fetch_text_resource_uncached(url)?;
        let data = Arc::new(text.as_bytes().to_vec());
        raw_cache_put(key, data.clone());
        enqueue_cache_bytes(path, data);
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
        let key = raw_cache_key(cache_dir, url);
        if let Some(data) = raw_cache_get(&key) {
            for chunk in data.chunks(16 * 1024) {
                let text = decode_streaming_utf8(&mut decoder, chunk, false);
                if !text.is_empty() {
                    on_chunk(&text);
                }
            }
            let tail = decode_streaming_utf8(&mut decoder, &[], true);
            if !tail.is_empty() {
                on_chunk(&tail);
            }
            return Ok(());
        }
        let path = url_cache_path(url, cache_dir);
        if let Ok(mut file) = std::fs::File::open(&path) {
            let mut body = Vec::new();
            let mut buf = [0u8; 16 * 1024];
            loop {
                let n = file.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..n]);
                let text = decode_streaming_utf8(&mut decoder, &buf[..n], false);
                if !text.is_empty() {
                    on_chunk(&text);
                }
            }
            let tail = decode_streaming_utf8(&mut decoder, &[], true);
            if !tail.is_empty() {
                on_chunk(&tail);
            }
            if body.is_empty() {
                let _ = std::fs::remove_file(&path);
            } else {
                raw_cache_put(key, Arc::new(body));
                return Ok(());
            }
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
        let body = Arc::new(body);
        raw_cache_put(key, body.clone());
        enqueue_cache_bytes(path, body);
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
    cached_fetch_bytes_arc(url, cache_dir).map(|bytes| bytes.as_ref().clone())
}

pub fn cached_fetch_bytes_arc(url: &str, cache_dir: &str) -> Result<Arc<Vec<u8>>, String> {
    if url.starts_with("data:") {
        return crate::html::image_data_url_bytes(url)
            .map(Arc::new)
            .ok_or_else(|| format!("invalid data URL bytes: {url}"));
    }
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
            return slot
                .as_ref()
                .expect("byte fetch result set")
                .as_ref()
                .cloned()
                .map_err(Clone::clone);
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
        Ok(bytes) => Ok(bytes.clone()),
        Err(err) => Err(err.clone()),
    }
}

fn cached_fetch_bytes_uncached(url: &str, cache_dir: &str) -> Result<Vec<u8>, String> {
    let key = raw_cache_key(cache_dir, url);
    if let Some(data) = raw_cache_get(&key) {
        return Ok((*data).clone());
    }
    let path = url_cache_path(url, cache_dir);
    if let Ok(data) = std::fs::read(&path) {
        if data.is_empty() {
            let _ = std::fs::remove_file(&path);
        } else {
            let data = Arc::new(data);
            raw_cache_put(key, data.clone());
            return Ok((*data).clone());
        }
    }
    if let Some(scheme_end) = url.find("://") {
        let after = &url[scheme_end + 3..];
        if let Some(slash) = after.find('/') {
            let old_url = format!("{}/.{}", &url[..scheme_end + 3], &after[slash..]);
            let old_path = url_cache_path(&old_url, cache_dir);
            if let Ok(data) = std::fs::read(&old_path) {
                if data.is_empty() {
                    let _ = std::fs::remove_file(&old_path);
                } else {
                    let data = Arc::new(data);
                    raw_cache_put(key, data.clone());
                    enqueue_cache_bytes(path, data.clone());
                    return Ok((*data).clone());
                }
            }
        }
    }
    let data = fetch_bytes(url)?;
    let data = Arc::new(data);
    raw_cache_put(key, data.clone());
    enqueue_cache_bytes(path, data.clone());
    Ok((*data).clone())
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
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status} loading stylesheet {url}"));
        }
        let bytes = resp.bytes().map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Err(format!("empty stylesheet response from {url}"));
        }
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
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status} loading stylesheet {url}"));
        }
        let mut buf = [0u8; 16 * 1024];
        let mut saw = false;
        loop {
            let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            saw = true;
            on_chunk(&buf[..n]);
        }
        if !saw {
            return Err(format!("empty stylesheet response from {url}"));
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

fn decode_streaming_utf8(decoder: &mut encoding_rs::Decoder, bytes: &[u8], last: bool) -> String {
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

    #[test]
    fn inline_style_boundary_emits_preview_before_byte_threshold() {
        let path = std::env::temp_dir().join(format!(
            "webcore-inline-style-preview-{}.html",
            std::process::id()
        ));
        let html = concat!(
            "<!doctype html><html><head>",
            "<style>body{background:#123;color:white}.hero{font-size:32px}</style>",
            "</head><body><div class=hero>Ready early</div>",
            "</body></html>"
        );
        std::fs::write(&path, html).unwrap();
        let url = format!("file://{}", path.display());
        let mut previews = Vec::new();
        let result = load_document_progressive(
            &url,
            &PageLoadOptions {
                emit_preview: true,
                preview_after_bytes: 64 * 1024,
                preview_interval_bytes: usize::MAX / 4,
                load_images: false,
                ..Default::default()
            },
            |preview_url, preview_html| {
                assert_eq!(preview_url, url);
                previews.push(preview_html);
            },
        );
        let _ = std::fs::remove_file(&path);
        result.expect("file load should complete");
        assert_eq!(
            previews.len(),
            1,
            "closing an inline <style> should produce an early styled preview"
        );
        assert!(previews[0].contains("font-size:32px"));
    }

    #[test]
    fn preview_uses_cached_linked_stylesheet_without_fetching() {
        let dir =
            std::env::temp_dir().join(format!("webcore-cached-preview-css-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let css = dir.join("style.css");
        std::fs::write(
            &css,
            "#hero { color: rgb(9, 8, 7); background: rgb(1, 2, 3); }",
        )
        .unwrap();
        let html =
            r#"<!doctype html><link rel="stylesheet" href="style.css"><p id="hero">Styled</p>"#;
        let base = format!("file://{}/index.html", dir.display());

        let warmed = crate::load_html_reusing_with_resource_loaders_and_wait_mode(
            html,
            &base,
            800.0,
            600.0,
            crate::types::ComponentRegistry::default(),
            None,
            None,
            None,
            None,
            false,
            true,
            true,
            std::time::Duration::from_secs(1),
        );
        let hero = warmed.get_element_by_id("hero").unwrap();
        let hero_box = warmed.get_box_by_id(hero).unwrap();
        assert_eq!(hero_box.style.color, crate::Color::rgb(9, 8, 7));

        std::fs::write(&css, "#hero { color: rgb(200, 0, 0); }").unwrap();
        let preview = crate::load_html_reusing_with_resource_loaders_and_wait_mode(
            html,
            &base,
            800.0,
            600.0,
            crate::types::ComponentRegistry::default(),
            None,
            None,
            None,
            None,
            false,
            false,
            true,
            std::time::Duration::ZERO,
        );

        let _ = std::fs::remove_dir_all(&dir);
        let hero = preview.get_element_by_id("hero").unwrap();
        let hero_box = preview.get_box_by_id(hero).unwrap();
        assert_eq!(
            hero_box.style.color,
            crate::Color::rgb(9, 8, 7),
            "preview should use the already parsed linked stylesheet without fetching the changed file"
        );
        assert_eq!(hero_box.style.background_color, crate::Color::rgb(1, 2, 3));
    }

    #[test]
    fn hot_document_cache_still_emits_streaming_preview() {
        let cache_dir =
            std::env::temp_dir().join(format!("webcore-hot-document-cache-{}", std::process::id()));
        std::fs::create_dir_all(&cache_dir).unwrap();
        let cache_dir_str = cache_dir.to_string_lossy().to_string();
        let url = "https://example.test/cached-stream.html";
        let html = format!(
            "{}{}",
            concat!(
                "<!doctype html><html><head>",
                "<style>body{color:rgb(4,5,6)}</style>",
                "</head><body><p>first paint</p>"
            ),
            "<div>tail</div>".repeat(4000)
        );
        raw_cache_put(
            raw_cache_key(&cache_dir_str, url),
            Arc::new(html.clone().into_bytes()),
        );

        let mut previews = Vec::new();
        let result = load_document_progressive(
            url,
            &PageLoadOptions {
                cache_dir: Some(cache_dir_str),
                emit_preview: true,
                preview_after_bytes: 512 * 1024,
                preview_interval_bytes: usize::MAX / 4,
                load_images: false,
                ..Default::default()
            },
            |_, preview_html| previews.push(preview_html),
        );

        let _ = std::fs::remove_dir_all(&cache_dir);
        let (final_html, final_url) = result.expect("cached document should load");
        assert_eq!(final_url, url);
        assert_eq!(final_html.len(), html.len());
        assert_eq!(previews.len(), 1);
        assert!(
            previews[0].len() < html.len(),
            "hot cache should emit an early chunk preview instead of waiting for the complete document"
        );
        assert!(previews[0].contains("body{color:rgb(4,5,6)}"));
    }

    #[test]
    fn stylesheet_preload_warms_parsed_css_cache() {
        let dir =
            std::env::temp_dir().join(format!("webcore-preload-parsed-css-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let css = dir.join("early.css");
        std::fs::write(&css, "#hero { color: rgb(12, 34, 56); }").unwrap();
        let base = format!("file://{}/index.html", dir.display());
        let resolved = crate::html::resolve_url("early.css", &base);
        let cache_key = format!("{resolved}\n");
        let mut parser = crate::StreamingParser::new(&base);
        let state = Arc::new(Mutex::new(PreloadState::default()));

        scan_html_chunk_for_resources(
            &mut parser,
            br#"<head><link rel="stylesheet" href="early.css"></head>"#,
            &PageLoadOptions {
                emit_preview: false,
                load_images: false,
                ..Default::default()
            },
            &state,
        );

        let mut cached = None;
        for _ in 0..50 {
            cached = crate::cached_parsed_stylesheet(&cache_key);
            if cached.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&dir);
        let sheet = cached.expect("stylesheet preload should parse into the shared CSS cache");
        assert!(
            sheet
                .rules
                .iter()
                .any(|rule| rule.original_selector.contains("#hero")),
            "preloaded parsed stylesheet should retain its rules"
        );
    }

    #[test]
    fn image_preload_warms_decoded_image_cache() {
        let image_url =
            "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='3' height='2'/%3E";
        let mut parser = crate::StreamingParser::new("https://example.test/news/");
        let state = Arc::new(Mutex::new(PreloadState::default()));

        scan_html_chunk_for_resources(
            &mut parser,
            format!(r#"<img loading="lazy" data-src="{image_url}">"#).as_bytes(),
            &PageLoadOptions {
                emit_preview: false,
                load_images: true,
                ..Default::default()
            },
            &state,
        );

        let mut decoded = None;
        for _ in 0..50 {
            decoded = crate::cached_decoded_image_ready(image_url);
            if decoded.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let decoded = decoded.expect("image preload should decode into the shared image cache");
        let (_, w, h) = crate::html::decoded_image_pixels(decoded).expect("decoded pixels");
        assert_eq!((w, h), (3, 2));
    }

    #[test]
    fn preview_uses_ready_decoded_image_cache_without_fetching() {
        let image_url = format!("https://example.test/ready-{}.png", std::process::id());
        let pixels = Arc::new(vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ]);
        let loader_pixels = pixels.clone();
        let warmed = crate::cached_decoded_image(
            &image_url,
            Some(&move |_| {
                Some(crate::html::DecodedImage::Raster(
                    loader_pixels.clone(),
                    2,
                    2,
                ))
            }),
        );
        assert!(
            warmed.is_some(),
            "test setup should warm decoded image cache"
        );

        let html = format!(r#"<!doctype html><img id="hero" src="{image_url}">"#);
        let preview = crate::load_html_reusing_with_resource_loaders_and_wait_mode(
            &html,
            "https://example.test/index.html",
            800.0,
            600.0,
            crate::types::ComponentRegistry::default(),
            None,
            None,
            None,
            None,
            false,
            false,
            true,
            std::time::Duration::ZERO,
        );
        let hero = preview.get_element_by_id("hero").unwrap();
        let hero_box = preview.get_box_by_id(hero).unwrap();
        assert_eq!(hero_box.image_width, 2);
        assert_eq!(hero_box.image_height, 2);
        assert!(
            hero_box.image_data.is_some(),
            "preview should adopt already-decoded image pixels without scheduling image fetch"
        );
        assert!(preview.pending_images.is_none());
    }

    #[test]
    fn cached_text_resource_reuses_process_memory_before_disk() {
        let cache_dir =
            std::env::temp_dir().join(format!("webcore-resource-cache-{}", std::process::id()));
        let source = cache_dir.join("source.css");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(&source, "body { color: red; }").unwrap();
        let url = source.to_string_lossy().to_string();
        let cache_dir_str = cache_dir.to_string_lossy().to_string();

        let first = fetch_text_resource(&url, Some(&cache_dir_str)).unwrap();
        std::fs::write(&source, "body { color: blue; }").unwrap();
        let second = fetch_text_resource(&url, Some(&cache_dir_str)).unwrap();

        let _ = std::fs::remove_dir_all(&cache_dir);
        assert_eq!(first, "body { color: red; }");
        assert_eq!(
            second, first,
            "hot process cache should avoid immediate refetch/reparse work for the same resource"
        );
    }

    #[test]
    fn cached_byte_resource_accepts_data_image_urls() {
        let cache_dir =
            std::env::temp_dir().join(format!("webcore-data-url-cache-{}", std::process::id()));
        let cache_dir_str = cache_dir.to_string_lossy().to_string();
        let url =
            "data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

        let bytes = cached_fetch_bytes(url, &cache_dir_str).unwrap();

        let _ = std::fs::remove_dir_all(&cache_dir);
        assert!(crate::html::decode_image_bytes_ex(&bytes).is_some());
    }

    #[test]
    fn zero_byte_disk_cache_entry_is_refetched() {
        let cache_dir = std::env::temp_dir().join(format!(
            "webcore-empty-resource-cache-{}",
            std::process::id()
        ));
        let source = cache_dir.join("source.css");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(&source, "body { color: green; }").unwrap();
        let url = source.to_string_lossy().to_string();
        let cache_dir_str = cache_dir.to_string_lossy().to_string();
        let cache_path = url_cache_path(&url, &cache_dir_str);
        std::fs::write(&cache_path, "").unwrap();

        let text = fetch_text_resource(&url, Some(&cache_dir_str)).unwrap();

        let _ = std::fs::remove_dir_all(&cache_dir);
        assert_eq!(text, "body { color: green; }");
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
