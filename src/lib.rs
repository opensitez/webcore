/// Browser version — bump periodically to stay current with real Chrome releases.
const CHROME_MAJOR: u32 = 131;

static CSS_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(2, 4))
        .thread_name(|i| format!("webcore-css-{i}"))
        .build()
        .expect("webcore CSS resource pool")
});

static IMAGE_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> =
    std::sync::LazyLock::new(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(resource_pool_threads(4, 8))
            .thread_name(|i| format!("webcore-image-{i}"))
            .build()
            .expect("webcore image resource pool")
    });

static FONT_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> =
    std::sync::LazyLock::new(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(resource_pool_threads(2, 4))
            .thread_name(|i| format!("webcore-font-{i}"))
            .build()
            .expect("webcore font resource pool")
    });

const PARSED_CSS_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

struct ParsedCssCache {
    entries: std::collections::HashMap<String, (std::sync::Arc<css::Stylesheet>, usize, u64)>,
    order: std::collections::VecDeque<(String, u64)>,
    bytes: usize,
    next_generation: u64,
}

impl ParsedCssCache {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
            bytes: 0,
            next_generation: 1,
        }
    }

    fn get(&mut self, key: &str) -> Option<std::sync::Arc<css::Stylesheet>> {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let sheet = self
            .entries
            .get_mut(key)
            .map(|(sheet, _, entry_generation)| {
                *entry_generation = generation;
                sheet.clone()
            })?;
        self.order.push_back((key.to_string(), generation));
        self.compact_order_if_needed();
        Some(sheet)
    }

    fn insert(&mut self, key: String, sheet: std::sync::Arc<css::Stylesheet>) {
        let bytes = stylesheet_cache_bytes(&sheet);
        if bytes > PARSED_CSS_CACHE_MAX_BYTES {
            return;
        }
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        if let Some((_, old_bytes, _)) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(old_bytes);
        }
        while self.bytes.saturating_add(bytes) > PARSED_CSS_CACHE_MAX_BYTES {
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
        self.order.push_back((key.clone(), generation));
        self.entries.insert(key, (sheet, bytes, generation));
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

static PARSED_CSS_CACHE: std::sync::LazyLock<std::sync::Mutex<ParsedCssCache>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(ParsedCssCache::new()));

struct CssParseState {
    result: std::sync::Mutex<Option<std::sync::Arc<css::Stylesheet>>>,
    done: std::sync::Condvar,
}

static CSS_PARSE_IN_FLIGHT: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<CssParseState>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn stylesheet_cache_bytes(sheet: &css::Stylesheet) -> usize {
    let string_bytes = |s: &String| std::mem::size_of::<String>().saturating_add(s.capacity());
    let mut bytes = std::mem::size_of::<css::Stylesheet>()
        .saturating_add(
            sheet
                .rules
                .capacity()
                .saturating_mul(std::mem::size_of::<css::CssRule>()),
        )
        .saturating_add(
            sheet
                .font_faces
                .capacity()
                .saturating_mul(std::mem::size_of::<css::FontFaceDecl>()),
        )
        .saturating_add(
            sheet
                .page_rules
                .capacity()
                .saturating_mul(std::mem::size_of::<css::PageRule>()),
        )
        .saturating_add(
            sheet
                .counter_styles
                .capacity()
                .saturating_mul(std::mem::size_of::<css::CounterStyleRule>()),
        );
    for source in &sheet.raw_sources {
        bytes = bytes.saturating_add(string_bytes(source));
    }
    for (name, value) in &sheet.variables {
        bytes = bytes
            .saturating_add(string_bytes(name))
            .saturating_add(string_bytes(value));
    }
    for (name, stops) in &sheet.keyframes {
        bytes = bytes.saturating_add(string_bytes(name)).saturating_add(
            stops
                .capacity()
                .saturating_mul(std::mem::size_of::<types::KeyframeStop>()),
        );
    }
    for layer in &sheet.layer_order {
        bytes = bytes.saturating_add(string_bytes(layer));
    }
    for rule in &sheet.rules {
        bytes =
            bytes
                .saturating_add(string_bytes(&rule.layer))
                .saturating_add(string_bytes(&rule.media_condition))
                .saturating_add(string_bytes(&rule.container_condition))
                .saturating_add(string_bytes(&rule.container_name))
                .saturating_add(string_bytes(&rule.original_selector))
                .saturating_add(
                    rule.selectors
                        .capacity()
                        .saturating_mul(std::mem::size_of::<css::CssSelector>()),
                )
                .saturating_add(rule.compiled_decls.capacity().saturating_mul(
                    std::mem::size_of::<(css::properties::PropertyId, types::CssValue)>(),
                ))
                .saturating_add(rule.compiled_important.capacity().saturating_mul(
                    std::mem::size_of::<(css::properties::PropertyId, types::CssValue)>(),
                ))
                .saturating_add(
                    rule.scopes
                        .capacity()
                        .saturating_mul(std::mem::size_of::<css::ScopeFrame>()),
                );
        for (name, value) in rule
            .declarations
            .iter()
            .chain(rule.important_declarations.iter())
        {
            bytes = bytes
                .saturating_add(string_bytes(name))
                .saturating_add(string_bytes(value));
        }
    }
    bytes
}

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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CacheMemoryStats {
    pub entries: usize,
    pub bytes: usize,
}

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
    }
}

struct ImageDecodeState {
    result: std::sync::Mutex<Option<Result<html::DecodedImage, String>>>,
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

pub(crate) fn spawn_image_resource_task(task: impl FnOnce() + Send + 'static) {
    IMAGE_RESOURCE_POOL.spawn(task);
}

pub(crate) fn spawn_font_resource_task(task: impl FnOnce() + Send + 'static) {
    FONT_RESOURCE_POOL.spawn(task);
}

pub(crate) fn cached_parsed_stylesheet(cache_key: &str) -> Option<css::Stylesheet> {
    PARSED_CSS_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(cache_key).map(|sheet| (*sheet).clone()))
}

pub(crate) fn parsed_css_cache_stats() -> CacheMemoryStats {
    PARSED_CSS_CACHE
        .lock()
        .map(|cache| CacheMemoryStats {
            entries: cache.entries.len(),
            bytes: cache.bytes,
        })
        .unwrap_or_default()
}

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

fn cached_decoded_image_result_from_option_loader(
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

fn cached_decoded_image_ready(url: &str) -> Option<html::DecodedImage> {
    DECODED_IMAGE_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(url))
}

fn stream_stylesheet_fragments(
    css_text: &str,
    css_url: &str,
    media: &str,
    mut emit: impl FnMut(css::Stylesheet),
) -> usize {
    let mut emitted = 0;
    let mut batch = String::new();
    let mut batch_units = 0usize;
    const MAX_BATCH_BYTES: usize = 64 * 1024;
    const MAX_BATCH_UNITS: usize = 96;
    let mut flush = |batch: &mut String, batch_units: &mut usize, emitted: &mut usize| {
        if batch.trim().is_empty() {
            batch.clear();
            *batch_units = 0;
            return;
        }
        let mut sheet = css::Stylesheet::default();
        sheet.parse_and_add_with_base_media(batch, css_url, media);
        if stylesheet_has_content(&sheet) {
            *emitted += 1;
            emit(sheet);
        }
        batch.clear();
        *batch_units = 0;
    };
    for chunk in complete_css_units(css_text) {
        if !batch.is_empty() {
            batch.push('\n');
        }
        batch.push_str(chunk);
        batch_units += 1;
        if batch.len() >= MAX_BATCH_BYTES || batch_units >= MAX_BATCH_UNITS {
            flush(&mut batch, &mut batch_units, &mut emitted);
        }
    }
    flush(&mut batch, &mut batch_units, &mut emitted);
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
pub type StreamingStylesheetLoader = std::sync::Arc<
    dyn Fn(&str, &mut dyn FnMut(&str)) -> Result<(), String> + Send + Sync + 'static,
>;
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
    let progressive_stream = streaming_loader.is_some();
    if cache_parsed
        && let Some(sheet) = PARSED_CSS_CACHE
            .lock()
            .ok()
            .and_then(|mut cache| cache.get(&cache_key))
    {
        if stylesheet_has_content(&sheet) {
            return CachedStylesheetLoad {
                sheet: (*sheet).clone(),
                text_len: 0,
                emitted_fragments: 0,
            };
        }
    }

    let parse_state = if cache_parsed && !progressive_stream {
        let existing = CSS_PARSE_IN_FLIGHT
            .lock()
            .expect("CSS parse in-flight cache poisoned")
            .get(&cache_key)
            .cloned();
        if let Some(state) = existing {
            let mut guard = state.result.lock().expect("CSS parse result poisoned");
            while guard.is_none() {
                guard = state.done.wait(guard).expect("CSS parse result poisoned");
            }
            if let Some(sheet) = guard.as_ref()
                && stylesheet_has_content(sheet)
            {
                return CachedStylesheetLoad {
                    sheet: (**sheet).clone(),
                    text_len: 0,
                    emitted_fragments: 0,
                };
            }
            if let Ok(mut in_flight) = CSS_PARSE_IN_FLIGHT.lock() {
                in_flight.remove(&cache_key);
            }
        }
        let state = std::sync::Arc::new(CssParseState {
            result: std::sync::Mutex::new(None),
            done: std::sync::Condvar::new(),
        });
        CSS_PARSE_IN_FLIGHT
            .lock()
            .expect("CSS parse in-flight cache poisoned")
            .insert(cache_key.clone(), state.clone());
        Some(state)
    } else {
        None
    };

    let mut combined = crate::css::Stylesheet::default();
    let mut text_len = 0usize;
    let mut emitted = 0usize;
    if let Some(streaming_loader) = streaming_loader {
        let mut buffer = String::new();
        let mut streamed_text = String::new();
        let streaming_result = streaming_loader(&css_url, &mut |chunk| {
            text_len += chunk.len();
            streamed_text.push_str(chunk);
            buffer.push_str(chunk);
            if let Some(complete_css) = drain_complete_css_text(&mut buffer) {
                let mut fragment = crate::css::Stylesheet::default();
                fragment.parse_and_add_with_base_media(&complete_css, &css_url, &media);
                if stylesheet_has_content(&fragment) {
                    emitted += 1;
                    combined.append_fragment(fragment.clone());
                    emit_fragment(fragment);
                }
            }
        });
        if streaming_result.is_err() || text_len == 0 {
            match loader(&css_url) {
                Ok(text) => {
                    text_len = text.len();
                    emitted = stream_stylesheet_fragments(&text, &css_url, &media, |fragment| {
                        combined.append_fragment(fragment.clone());
                        emit_fragment(fragment);
                    });
                    if emitted == 0 && !text.trim().is_empty() {
                        combined.parse_and_add_with_base_media(&text, &css_url, &media);
                    }
                }
                Err(err) => {
                    eprintln!("  CSS failed: {css_url} ({err})");
                }
            }
        } else if emitted == 0 && !streamed_text.trim().is_empty() {
            combined.parse_and_add_with_base_media(&streamed_text, &css_url, &media);
            if stylesheet_has_content(&combined) {
                emitted = 1;
                emit_fragment(combined.clone());
            }
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            let mut fragment = crate::css::Stylesheet::default();
            fragment.parse_and_add_with_base_media(&tail, &css_url, &media);
            if stylesheet_has_content(&fragment) {
                emitted += 1;
                combined.append_fragment(fragment.clone());
                emit_fragment(fragment);
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

    if cache_parsed
        && stylesheet_has_content(&combined)
        && let Ok(mut cache) = PARSED_CSS_CACHE.lock()
    {
        cache.insert(cache_key.clone(), std::sync::Arc::new(combined.clone()));
    }
    if let Some(parse_state) = parse_state {
        let mut guard = parse_state
            .result
            .lock()
            .expect("CSS parse result poisoned");
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

#[cfg(test)]
mod stylesheet_loader_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn streaming_stylesheet_keeps_minified_bootstrap_rules() {
        let css = r#"@charset "UTF-8";:root{--bs-blue:#0d6efd}.d-flex{display:flex!important}.btn-primary{color:#fff;background-color:#0d6efd}@media (min-width:768px){.row-cols-md-2>*{flex:0 0 auto;width:50%}}"#;
        let css_owned = css.to_string();
        let loader: StylesheetLoader = Arc::new({
            let css_owned = css_owned.clone();
            move |_| Ok(css_owned.clone())
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new({
            let css_owned = css_owned.clone();
            move |_, emit| {
                for chunk in css_owned.as_bytes().chunks(17) {
                    emit(std::str::from_utf8(chunk).unwrap());
                }
                Ok(())
            }
        });
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let emitted_for_cb = emitted.clone();
        let loaded = load_stylesheet_cached(
            "test-bootstrap-stream\n".to_string(),
            "https://example.test/bootstrap.min.css".to_string(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            move |sheet| emitted_for_cb.lock().unwrap().push(sheet),
        );

        assert!(loaded.emitted_fragments > 0);
        assert!(
            loaded
                .sheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".d-flex")
        );
        assert!(
            loaded
                .sheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".btn-primary")
        );
        assert!(
            loaded
                .sheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".row-cols-md-2>*"
                    && rule.media_condition.contains("min-width"))
        );
        assert!(!emitted.lock().unwrap().is_empty());
    }
}

pub mod types;

#[cfg(feature = "accessibility")]
pub mod accessibility;
pub mod browser_view;
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

pub use browser_view::BrowserView;
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
    load_html_reusing_with_resource_loaders_and_wait_mode(
        html,
        base_url,
        viewport_width,
        viewport_height,
        registry,
        reuse,
        stylesheet_loader,
        streaming_stylesheet_loader,
        image_loader,
        load_images,
        true,
        true,
        css_wait,
    )
}

pub(crate) fn load_html_reusing_with_resource_loaders_and_wait_mode(
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
    load_external_stylesheets: bool,
    use_cached_external_stylesheets: bool,
    css_wait: std::time::Duration,
) -> Document {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };

    // Channel for CSS results — fetches start during parsing via the hook.
    let (css_tx, css_rx) = mpsc::channel::<(usize, String, crate::css::Stylesheet, String)>(); // idx, url, parsed css, media
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
                && load_external_stylesheets
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
    doc.preserve_stylesheet_document_order = true;
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
        let mut fetched_slots: std::collections::HashMap<usize, crate::css::Stylesheet> =
            std::collections::HashMap::new();
        for (idx, css_url, sheet, _) in css_results {
            match fetched_slots.entry(idx) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().append_fragment(sheet.clone());
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(sheet.clone());
                }
            }
            match fetched_map.entry(css_url) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().append_fragment(sheet);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(sheet);
                }
            }
        }
        if use_cached_external_stylesheets {
            for (idx, ds) in doc.document_stylesheets.iter().enumerate() {
                if let crate::types::DocumentStylesheet::Linked { href, media } = ds {
                    let abs = resolve_css_url(base_url, href);
                    if fetched_slots.contains_key(&idx) {
                        continue;
                    }
                    let cache_key = format!("{abs}\n{media}");
                    if let Some(sheet) = cached_parsed_stylesheet(&cache_key) {
                        fetched_slots.insert(idx, sheet.clone());
                        fetched_map.insert(abs, sheet);
                    }
                }
            }
        }
        doc.stylesheet = crate::css::ua_stylesheet();
        for (idx, ds) in doc.document_stylesheets.iter().enumerate() {
            match ds {
                crate::types::DocumentStylesheet::Inline { css } => {
                    doc.stylesheet.parse_and_add_with_base(css, &doc.base_url);
                }
                crate::types::DocumentStylesheet::Linked { href, .. } => {
                    let abs = resolve_css_url(base_url, href);
                    if let Some(sheet) = fetched_slots.get(&idx).or_else(|| fetched_map.get(&abs)) {
                        doc.stylesheet.append_fragment(sheet.clone());
                    }
                }
            }
        }
        doc.loaded_stylesheet_slots = fetched_slots;
        doc.loaded_linked_stylesheets = fetched_map;
        doc.refresh_shadow_linked_stylesheets();
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
        doc.refresh_shadow_linked_stylesheets();
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
    } else {
        apply_ready_cached_images(&mut doc);
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
    loader: Option<
        std::sync::Arc<dyn Fn(&str) -> Option<html::DecodedImage> + Send + Sync + 'static>,
    >,
) {
    let mut pending: Vec<(u32, Vec<usize>, types::PendingImageTarget, String)> = Vec::new();
    collect_remote_images(
        &doc.root,
        &doc.base_url,
        doc.viewport_w,
        doc.viewport_h,
        &mut Vec::new(),
        &mut pending,
    );
    if pending.is_empty() {
        return;
    }

    let mut async_pending = Vec::with_capacity(pending.len());
    for (node_id, path, target, url) in pending {
        let url_trimmed = url.trim();
        if url_trimmed.starts_with("data:")
            && matches!(
                target,
                types::PendingImageTarget::Background
                    | types::PendingImageTarget::BackgroundLayer(_)
                    | types::PendingImageTarget::Mask
            )
        {
            match cached_decoded_image_result_from_option_loader(url_trimmed, loader.as_deref()) {
                Ok(decoded) => {
                    let Some(node) = (if node_id != 0 {
                        doc.find_webcore_mut(node_id)
                    } else {
                        types::find_node_by_path_mut(&mut doc.root, &path)
                    }) else {
                        continue;
                    };
                    match target {
                        types::PendingImageTarget::Background => {
                            let _ = html::set_decoded_bg_image_on_node(node, decoded);
                        }
                        types::PendingImageTarget::BackgroundLayer(layer_index) => {
                            let _ = html::set_decoded_bg_image_layer_on_node(
                                node,
                                layer_index,
                                decoded,
                            );
                        }
                        types::PendingImageTarget::Mask => {
                            if let Some((data, w, h)) = html::decoded_image_pixels_arc(decoded) {
                                node.mask_image_data = Some(data);
                                node.mask_image_width = w;
                                node.mask_image_height = h;
                            }
                        }
                        _ => {}
                    }
                }
                Err(error) => doc.image_load_errors.push((path, target, url, error)),
            }
        } else {
            async_pending.push((node_id, path, target, url));
        }
    }
    if async_pending.is_empty() {
        return;
    }

    let (tx, rx) = std::sync::mpsc::channel::<types::PendingImageResult>();
    let in_flight = doc.images_in_flight.clone();
    in_flight.store(async_pending.len(), std::sync::atomic::Ordering::SeqCst);

    for (node_id, path, target, url) in async_pending {
        let sender = tx.clone();
        let counter = in_flight.clone();
        let loader = loader.clone();
        spawn_image_resource_task(move || {
            let result = cached_decoded_image_result_from_option_loader(&url, loader.as_deref());
            let event = match result {
                Ok(decoded) => types::PendingImageResult::Loaded {
                    node_id,
                    path,
                    target,
                    url,
                    decoded,
                },
                Err(error) => types::PendingImageResult::Failed {
                    node_id,
                    path,
                    target,
                    url,
                    error,
                },
            };
            let _ = sender.send(event);
            counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });
    }
    doc.pending_images = Some(rx);
}

fn apply_ready_cached_images(doc: &mut types::Document) {
    let mut pending: Vec<(u32, Vec<usize>, types::PendingImageTarget, String)> = Vec::new();
    collect_remote_images(
        &doc.root,
        &doc.base_url,
        doc.viewport_w,
        doc.viewport_h,
        &mut Vec::new(),
        &mut pending,
    );
    for (node_id, path, target, url) in pending {
        let Some(decoded) = cached_decoded_image_ready(&url) else {
            continue;
        };
        if let Some(node) = if node_id != 0 {
            doc.find_webcore_mut(node_id)
        } else {
            types::find_node_by_path_mut(&mut doc.root, &path)
        } {
            match target {
                types::PendingImageTarget::Element | types::PendingImageTarget::ElementFallback => {
                    if matches!(target, types::PendingImageTarget::ElementFallback)
                        && node.image_data.is_some()
                    {
                        continue;
                    }
                    html::set_decoded_image_on_node(node, decoded);
                }
                types::PendingImageTarget::Background => {
                    let _ = html::set_decoded_bg_image_on_node(node, decoded);
                }
                types::PendingImageTarget::BackgroundLayer(layer_index) => {
                    let _ = html::set_decoded_bg_image_layer_on_node(node, layer_index, decoded);
                }
                types::PendingImageTarget::Mask => {
                    if let Some((data, w, h)) = html::decoded_image_pixels_arc(decoded) {
                        node.mask_image_data = Some(data);
                        node.mask_image_width = w;
                        node.mask_image_height = h;
                    }
                }
            }
        }
    }
}

fn collect_remote_images(
    node: &types::WebCore,
    base_url: &str,
    viewport_w: f32,
    viewport_h: f32,
    path: &mut Vec<usize>,
    pending: &mut Vec<(u32, Vec<usize>, types::PendingImageTarget, String)>,
) {
    if (node.is_image_element() || node.tag == "video") && node.image_data.is_none() {
        let raw = if node.tag == "video" {
            node.attributes.get("poster").map(|s| s.as_str())
        } else {
            html::image_fallback_source(node)
        };
        if let Some(raw) = raw {
            let url = html::resolve_url(raw, base_url);
            if is_async_image_url(&url) {
                pending.push((
                    node.node_id,
                    path.clone(),
                    types::PendingImageTarget::ElementFallback,
                    url,
                ));
            }
        }
        let preferred = if !node.resolved_src.is_empty() {
            Some(node.resolved_src.clone())
        } else if node.tag == "img" {
            html::image_srcset_source(node).and_then(|srcset| {
                html::parse_srcset_url_for(
                    srcset,
                    node.attributes.get("sizes").map(String::as_str),
                    viewport_w,
                    viewport_h,
                    1.0,
                )
                .map(|candidate| html::resolve_url(&candidate, base_url))
            })
        } else {
            None
        };
        if let Some(url) = preferred
            && is_async_image_url(&url)
            && !pending.iter().any(|(_, candidate_path, _, candidate)| {
                candidate_path == path && candidate == &url
            })
        {
            pending.push((
                node.node_id,
                path.clone(),
                types::PendingImageTarget::Element,
                url,
            ));
        }
    }
    if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
        let resolved = html::resolve_url(&node.style.background_image_url, base_url);
        let url = resolved.as_str();
        if is_async_image_url(url) {
            pending.push((
                node.node_id,
                path.clone(),
                types::PendingImageTarget::Background,
                url.to_string(),
            ));
        }
    }
    for (layer_index, layer) in node
        .style
        .rare()
        .additional_background_layers
        .iter()
        .enumerate()
    {
        if layer.image_url.is_empty() {
            continue;
        }
        let loaded = node
            .additional_bg_images
            .get(layer_index)
            .and_then(|image| image.as_ref())
            .is_some();
        if loaded {
            continue;
        }
        let resolved = html::resolve_url(&layer.image_url, base_url);
        let url = resolved.as_str();
        if is_async_image_url(url) {
            pending.push((
                node.node_id,
                path.clone(),
                types::PendingImageTarget::BackgroundLayer(layer_index),
                url.to_string(),
            ));
        }
    }
    if node.mask_image_data.is_none() && !node.style.rare().mask_image_url.is_empty() {
        let resolved = html::resolve_url(&node.style.rare().mask_image_url, base_url);
        let url = resolved.as_str();
        if is_async_image_url(url) {
            pending.push((
                node.node_id,
                path.clone(),
                types::PendingImageTarget::Mask,
                url.to_string(),
            ));
        }
    }
    if let Some(shadow) = node.shadow_root.as_ref() {
        for child in &shadow.children {
            collect_remote_images(child, base_url, viewport_w, viewport_h, path, pending);
        }
    }
    for (i, child) in node.children.iter().enumerate() {
        path.push(i);
        collect_remote_images(child, base_url, viewport_w, viewport_h, path, pending);
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
    if !url.starts_with("http://") && !url.starts_with("https://") {
        let path = url.split('?').next().unwrap_or(url);
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
