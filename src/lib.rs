/// Browser version — bump periodically to stay current with real Chrome releases.
const CHROME_MAJOR: u32 = 131;

static CSS_RESOURCE_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(2, 4))
        .thread_name(|i| format!("webcore-css-{i}"))
        .build()
        .expect("webcore CSS resource pool")
});

static CSS_IMPORT_POOL: std::sync::LazyLock<rayon::ThreadPool> = std::sync::LazyLock::new(|| {
    rayon::ThreadPoolBuilder::new()
        .num_threads(resource_pool_threads(2, 4))
        .thread_name(|i| format!("webcore-css-import-{i}"))
        .build()
        .expect("webcore CSS import pool")
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
        let bytes = stylesheet_cache_bytes(&sheet).saturating_add(key.len());
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
    for (layer, condition) in &sheet.layer_declarations {
        bytes = bytes
            .saturating_add(string_bytes(layer))
            .saturating_add(condition.heap_bytes());
    }
    for rule in &sheet.rules {
        bytes =
            bytes
                .saturating_add(string_bytes(&rule.layer))
                .saturating_add(rule.media_condition.heap_bytes())
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CacheMemoryStats {
    pub entries: usize,
    pub bytes: usize,
}

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

pub(crate) fn parsed_inline_stylesheet(
    css_text: &str,
    base_url: &str,
) -> std::sync::Arc<css::Stylesheet> {
    let key = format!("\0inline\0{base_url}\0{css_text}");
    if key.len() <= PARSED_CSS_CACHE_MAX_BYTES / 2 {
        if let Ok(mut cache) = PARSED_CSS_CACHE.lock() {
            if let Some(sheet) = cache.get(&key) {
                return sheet;
            }
        }
    }
    let mut sheet = css::Stylesheet::default();
    sheet.parse_and_add_with_base(css_text, base_url);
    let sheet = std::sync::Arc::new(sheet);
    if key.len() <= PARSED_CSS_CACHE_MAX_BYTES / 2 {
        if let Ok(mut cache) = PARSED_CSS_CACHE.lock() {
            cache.insert(key, sheet.clone());
        }
    }
    sheet
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

#[cfg(test)]
pub(crate) use images::cache::cached_decoded_image;
pub(crate) use images::cache::{
    cached_decoded_image_ready, cached_decoded_image_result,
    cached_decoded_image_result_from_option_loader, decoded_image_cache_stats,
};

fn stream_stylesheet_fragments(
    css_text: &str,
    css_url: &str,
    media: &css::MediaConditions,
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
        sheet.parse_and_add_with_base_media_conditions(batch, css_url, media);
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
        || !sheet.variables.is_empty()
        || !sheet.font_faces.is_empty()
        || !sheet.keyframes.is_empty()
        || !sheet.page_rules.is_empty()
        || !sheet.counter_styles.is_empty()
        || !sheet.layer_order.is_empty()
}

fn record_stylesheet_fragment(
    combined: &mut css::Stylesheet,
    emitted: &mut usize,
    emit: &mut impl FnMut(css::Stylesheet),
    fragment: css::Stylesheet,
) {
    *emitted += 1;
    combined.append_fragment(fragment.clone());
    emit(fragment);
}

#[derive(Debug, PartialEq, Eq)]
struct CssImportTarget<'a> {
    url: String,
    media: &'a str,
    layer: Option<Option<&'a str>>,
    supports: Option<&'a str>,
}

fn css_import_function<'a>(input: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let prefix = input.get(..name.len())?;
    if !prefix.eq_ignore_ascii_case(name) || input.as_bytes().get(name.len()) != Some(&b'(') {
        return None;
    }
    let body = input.get(name.len() + 1..)?;
    let mut depth = 1usize;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&body[..index], body[index + 1..].trim_start()));
                }
            }
            _ => {}
        }
    }
    None
}

fn css_import_target(rule: &str) -> Option<CssImportTarget<'_>> {
    let rule = rule.trim();
    let rest = rule.get(7..)?;
    if !rule.get(..7)?.eq_ignore_ascii_case("@import") {
        return None;
    }
    if !rest
        .chars()
        .next()
        .is_some_and(|ch| ch.is_whitespace() || ch == '\'' || ch == '"')
    {
        return None;
    }
    let rest = rest.trim_start();
    let (url, rest) = if rest
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("url("))
    {
        let (url, consumed) = css::apply::parse_url_function(rest)?;
        (url, rest.get(consumed..)?.trim_start())
    } else {
        let (url, tail) = css::apply::consume_css_string(rest)?;
        (url, tail.trim_start())
    };
    let mut rest = rest.strip_suffix(';')?.trim();
    let mut layer = None;
    let mut supports = None;
    for _ in 0..2 {
        if layer.is_none() {
            if let Some((name, tail)) = css_import_function(rest, "layer") {
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                layer = Some(Some(name));
                rest = tail;
                continue;
            }
            if rest
                .get(..5)
                .is_some_and(|head| head.eq_ignore_ascii_case("layer"))
                && rest
                    .get(5..)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
            {
                layer = Some(None);
                rest = rest.get(5..)?.trim_start();
                continue;
            }
        }
        if supports.is_none() {
            if let Some((condition, tail)) = css_import_function(rest, "supports") {
                supports = Some(condition.trim());
                rest = tail;
                continue;
            }
        }
        break;
    }
    Some(CssImportTarget {
        url,
        media: rest.trim(),
        layer,
        supports,
    })
}

fn imported_layer_name(parent: Option<&str>, layer: Option<Option<&str>>) -> Option<String> {
    let local = match layer {
        None => return parent.map(str::to_string),
        Some(Some(name)) => name.to_string(),
        Some(None) => {
            static NEXT_LAYER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            format!(
                "\0import-{}",
                NEXT_LAYER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )
        }
    };
    Some(match parent {
        Some(parent) => format!("{parent}.{local}"),
        None => local,
    })
}

fn scope_imported_layer(sheet: &mut css::Stylesheet, layer: &str) {
    for name in &mut sheet.layer_order {
        *name = format!("{layer}.{name}");
    }
    for (name, _) in &mut sheet.layer_declarations {
        *name = format!("{layer}.{name}");
    }
    for rule in &mut sheet.rules {
        rule.layer = if rule.layer.is_empty() {
            layer.to_string()
        } else {
            format!("{layer}.{}", rule.layer)
        };
    }
    for rule in &mut sheet.counter_styles {
        rule.layer = if rule.layer.is_empty() {
            layer.to_string()
        } else {
            format!("{layer}.{}", rule.layer)
        };
    }
}

fn emit_css_imports(
    css_text: &str,
    css_url: &str,
    media: &css::MediaConditions,
    loader: &StylesheetLoader,
    streaming_loader: Option<&StreamingStylesheetLoader>,
    stack: &mut std::collections::HashSet<String>,
    parent_layer: Option<&str>,
    stream_failed: &mut bool,
    emit: &mut impl FnMut(css::Stylesheet),
) -> usize {
    if stack.len() >= 32 {
        return 0;
    }
    let cleaned = css::parser::strip_css_comments(css_text);
    let mut count = 0;
    for unit in complete_css_units(&cleaned) {
        let unit = unit.trim();
        if unit.to_ascii_lowercase().starts_with("@charset") {
            continue;
        }
        let Some(target) = css_import_target(unit) else {
            break;
        };
        if target.supports.is_some_and(|condition| {
            !css::parser::supports_condition_matches(&format!("({condition})"))
        }) {
            continue;
        }
        let layer = imported_layer_name(parent_layer, target.layer);
        let effective_media = media.with_query(target.media);
        if target.layer.is_some() {
            let mut declaration = css::Stylesheet::default();
            declaration.layer_order.push(layer.clone().unwrap());
            declaration
                .layer_declarations
                .push((layer.clone().unwrap(), effective_media.clone()));
            count += 1;
            emit(declaration);
        }
        let url = resolve_css_url(css_url, &target.url);
        if !stack.insert(url.clone()) {
            continue;
        }
        let mut consume = |text: &str| {
            count += emit_css_imports(
                text,
                &url,
                &effective_media,
                loader,
                streaming_loader,
                stack,
                layer.as_deref(),
                stream_failed,
                emit,
            );
            count += stream_stylesheet_fragments(text, &url, &effective_media, |mut sheet| {
                if let Some(layer) = layer.as_deref() {
                    scope_imported_layer(&mut sheet, layer);
                }
                emit(sheet);
            });
        };
        if let Some(streaming_loader) = streaming_loader {
            let mut buffer = CssStreamBuffer::default();
            let mut received = false;
            let result = streaming_loader(&url, &mut |chunk| {
                received |= !chunk.is_empty();
                if let Some(complete) = buffer.push(chunk) {
                    consume(&complete);
                }
            });
            if received {
                if !buffer.text.trim().is_empty() {
                    consume(&buffer.text);
                }
            } else if let Ok(imported) = loader(&url) {
                consume(&imported);
            }
            drop(consume);
            *stream_failed |= result.is_err() && received;
        } else if let Ok(imported) = loader(&url) {
            consume(&imported);
        }
        stack.remove(&url);
    }
    count
}

type ImportedFragment = (css::Stylesheet, std::sync::mpsc::Sender<()>);

struct ImportTask {
    fragments: std::sync::mpsc::Receiver<ImportedFragment>,
    stream_failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

struct ImportWorker {
    pending: std::collections::VecDeque<String>,
    active: std::collections::VecDeque<ImportTask>,
    closed: bool,
    stream_failed: bool,
    css_url: String,
    media: css::MediaConditions,
    loader: StylesheetLoader,
    streaming_loader: StreamingStylesheetLoader,
    stack: std::collections::HashSet<String>,
}

impl ImportWorker {
    fn spawn(
        css_url: String,
        media: css::MediaConditions,
        loader: StylesheetLoader,
        streaming_loader: StreamingStylesheetLoader,
        stack: std::collections::HashSet<String>,
    ) -> Self {
        Self {
            pending: std::collections::VecDeque::new(),
            active: std::collections::VecDeque::new(),
            closed: false,
            stream_failed: false,
            css_url,
            media,
            loader,
            streaming_loader,
            stack,
        }
    }

    fn send(&mut self, unit: String) -> Result<(), String> {
        if self.closed {
            return Err("import prefix already closed".into());
        }
        self.pending.push_back(unit);
        self.schedule();
        Ok(())
    }

    fn close_units(&mut self) {
        self.closed = true;
    }

    fn schedule(&mut self) {
        while self.active.len() < CSS_IMPORT_POOL.current_num_threads()
            && let Some(unit) = self.pending.pop_front()
        {
            let (fragment_tx, fragments) = std::sync::mpsc::channel::<ImportedFragment>();
            let stream_failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let failed_for_task = stream_failed.clone();
            let css_url = self.css_url.clone();
            let media = self.media.clone();
            let loader = self.loader.clone();
            let streaming_loader = self.streaming_loader.clone();
            let mut stack = self.stack.clone();
            CSS_IMPORT_POOL.spawn(move || {
                let mut failed = false;
                emit_css_imports(
                    &unit,
                    &css_url,
                    &media,
                    &loader,
                    Some(&streaming_loader),
                    &mut stack,
                    None,
                    &mut failed,
                    &mut |fragment| {
                        let (ack, received) = std::sync::mpsc::channel();
                        if fragment_tx.send((fragment, ack)).is_ok() {
                            let _ = received.recv();
                        }
                    },
                );
                failed_for_task.store(failed, std::sync::atomic::Ordering::Release);
            });
            self.active.push_back(ImportTask {
                fragments,
                stream_failed,
            });
        }
    }

    fn finish_front(&mut self) {
        if let Some(task) = self.active.pop_front() {
            self.stream_failed |= task
                .stream_failed
                .load(std::sync::atomic::Ordering::Acquire);
        }
        self.schedule();
    }

    fn drain(&mut self, emit: &mut impl FnMut(css::Stylesheet)) -> bool {
        loop {
            let Some(task) = self.active.front() else {
                return self.closed && self.pending.is_empty();
            };
            match task.fragments.try_recv() {
                Ok((fragment, ack)) => {
                    emit(fragment);
                    let _ = ack.send(());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.finish_front(),
            }
        }
    }

    fn finish(mut self, emit: &mut impl FnMut(css::Stylesheet)) -> bool {
        self.close_units();
        loop {
            let Some(task) = self.active.front() else {
                break;
            };
            match task.fragments.recv() {
                Ok((fragment, ack)) => {
                    emit(fragment);
                    let _ = ack.send(());
                }
                Err(_) => self.finish_front(),
            }
        }
        self.stream_failed
    }
}

#[derive(Default)]
struct CssStreamBuffer {
    text: String,
    scanned: usize,
    depth: usize,
    quote: Option<u8>,
    escaped: bool,
    comment: bool,
    boundary: usize,
}

impl CssStreamBuffer {
    fn push(&mut self, chunk: &str) -> Option<String> {
        self.text.push_str(chunk);
        let bytes = self.text.as_bytes();
        let mut i = self.scanned;
        while i < bytes.len() {
            let b = bytes[i];
            if self.comment {
                if b == b'*' {
                    if i + 1 == bytes.len() {
                        break;
                    }
                    if bytes[i + 1] == b'/' {
                        self.comment = false;
                        i += 2;
                        continue;
                    }
                }
                i += 1;
                continue;
            }
            if let Some(quote) = self.quote {
                if self.escaped {
                    self.escaped = false;
                } else if b == b'\\' {
                    self.escaped = true;
                } else if b == quote {
                    self.quote = None;
                }
                i += 1;
                continue;
            }
            if b == b'/' {
                if i + 1 == bytes.len() {
                    break;
                }
                if bytes[i + 1] == b'*' {
                    self.comment = true;
                    i += 2;
                    continue;
                }
            }
            if b == b'\'' || b == b'"' {
                self.quote = Some(b);
                i += 1;
                continue;
            }
            if b == b'\\' {
                if i + 1 == bytes.len() {
                    break;
                }
                i += 2;
                continue;
            }
            match b {
                b'{' => self.depth += 1,
                b'}' => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.depth == 0 {
                        self.boundary = i + 1;
                    }
                }
                b';' if self.depth == 0 => self.boundary = i + 1,
                _ => {}
            }
            i += 1;
        }
        self.scanned = i;
        if self.boundary == 0 {
            return None;
        }
        let complete: String = self.text.drain(..self.boundary).collect();
        self.scanned -= self.boundary;
        self.boundary = 0;
        if complete.trim().is_empty() {
            None
        } else {
            Some(complete)
        }
    }
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
        if b == b'\\' {
            i = (i + 2).min(bytes.len());
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
    pub replace_emitted: bool,
}

fn parse_complete_stylesheet(
    text: &str,
    css_url: &str,
    media: &css::MediaConditions,
    loader: &StylesheetLoader,
) -> css::Stylesheet {
    let mut sheet = css::Stylesheet::default();
    let mut stack = std::collections::HashSet::from([css_url.to_string()]);
    let mut stream_failed = false;
    emit_css_imports(
        text,
        css_url,
        media,
        loader,
        None,
        &mut stack,
        None,
        &mut stream_failed,
        &mut |fragment| {
            sheet.append_fragment(fragment);
        },
    );
    stream_stylesheet_fragments(text, css_url, media, |fragment| {
        sheet.append_fragment(fragment);
    });
    if !stylesheet_has_content(&sheet) && !text.trim().is_empty() {
        sheet.parse_and_add_with_base_media_conditions(text, css_url, media);
    }
    sheet
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
                replace_emitted: false,
            };
        }
    }

    let media = css::MediaConditions::default().with_query(&media);

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
                    replace_emitted: false,
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
    let mut replace_emitted = false;
    let mut import_stream_failed = false;
    let mut import_stack = std::collections::HashSet::from([css_url.clone()]);
    if let Some(streaming_loader) = streaming_loader {
        let mut buffer = CssStreamBuffer::default();
        let mut import_worker: Option<ImportWorker> = None;
        let mut pending_parent = Vec::new();
        let mut import_prefix = true;
        let import_streaming_loader = streaming_loader.clone();
        let mut process_complete_css = |complete_css: &str| {
            let cleaned = css::parser::strip_css_comments(complete_css);
            for unit in complete_css_units(&cleaned) {
                let unit = unit.trim();
                if import_prefix && unit.to_ascii_lowercase().starts_with("@charset") {
                    continue;
                }
                if import_prefix && css_import_target(unit).is_some() {
                    if import_worker.is_none() {
                        import_worker = Some(ImportWorker::spawn(
                            css_url.clone(),
                            media.clone(),
                            loader.clone(),
                            import_streaming_loader.clone(),
                            import_stack.clone(),
                        ));
                    }
                    let queued = import_worker
                        .as_mut()
                        .is_some_and(|worker| worker.send(unit.to_string()).is_ok());
                    if queued {
                        continue;
                    }
                    if let Some(worker) = import_worker.take() {
                        import_stream_failed |= worker.finish(&mut |fragment| {
                            record_stylesheet_fragment(
                                &mut combined,
                                &mut emitted,
                                &mut emit_fragment,
                                fragment,
                            );
                        });
                    }
                    emit_css_imports(
                        unit,
                        &css_url,
                        &media,
                        &loader,
                        Some(&import_streaming_loader),
                        &mut import_stack,
                        None,
                        &mut import_stream_failed,
                        &mut |fragment| {
                            record_stylesheet_fragment(
                                &mut combined,
                                &mut emitted,
                                &mut emit_fragment,
                                fragment,
                            );
                        },
                    );
                } else if !unit.is_empty() {
                    import_prefix = false;
                }
            }
            if !import_prefix && let Some(worker) = import_worker.as_mut() {
                worker.close_units();
            }
            let mut fragment = css::Stylesheet::default();
            fragment.parse_and_add_with_base_media_conditions(complete_css, &css_url, &media);
            if stylesheet_has_content(&fragment) {
                if import_worker.is_some() {
                    pending_parent.push(fragment);
                } else {
                    record_stylesheet_fragment(
                        &mut combined,
                        &mut emitted,
                        &mut emit_fragment,
                        fragment,
                    );
                }
            }
            let finished = import_worker.as_mut().is_some_and(|worker| {
                worker.drain(&mut |fragment| {
                    record_stylesheet_fragment(
                        &mut combined,
                        &mut emitted,
                        &mut emit_fragment,
                        fragment,
                    );
                })
            });
            if finished {
                if let Some(worker) = import_worker.take() {
                    import_stream_failed |= worker.finish(&mut |fragment| {
                        record_stylesheet_fragment(
                            &mut combined,
                            &mut emitted,
                            &mut emit_fragment,
                            fragment,
                        );
                    });
                }
                for fragment in pending_parent.drain(..) {
                    record_stylesheet_fragment(
                        &mut combined,
                        &mut emitted,
                        &mut emit_fragment,
                        fragment,
                    );
                }
            }
        };
        let streaming_result = streaming_loader(&css_url, &mut |chunk| {
            text_len += chunk.len();
            if let Some(complete_css) = buffer.push(chunk) {
                process_complete_css(&complete_css);
            }
        });
        let tail = std::mem::take(&mut buffer.text);
        if !tail.trim().is_empty() {
            process_complete_css(&tail);
        }
        drop(process_complete_css);
        if let Some(worker) = import_worker.take() {
            import_stream_failed |= worker.finish(&mut |fragment| {
                record_stylesheet_fragment(
                    &mut combined,
                    &mut emitted,
                    &mut emit_fragment,
                    fragment,
                );
            });
        }
        for fragment in pending_parent {
            record_stylesheet_fragment(&mut combined, &mut emitted, &mut emit_fragment, fragment);
        }
        if (streaming_result.is_err() || import_stream_failed) && emitted > 0 {
            replace_emitted = true;
            combined = match loader(&css_url) {
                Ok(text) => {
                    text_len = text.len();
                    parse_complete_stylesheet(&text, &css_url, &media, &loader)
                }
                Err(err) => {
                    eprintln!("  CSS failed: {css_url} ({err})");
                    css::Stylesheet::default()
                }
            };
        } else if streaming_result.is_err() || import_stream_failed || text_len == 0 {
            match loader(&css_url) {
                Ok(text) => {
                    text_len = text.len();
                    combined = parse_complete_stylesheet(&text, &css_url, &media, &loader);
                    if stylesheet_has_content(&combined) {
                        emitted += 1;
                        emit_fragment(combined.clone());
                    }
                }
                Err(err) => {
                    eprintln!("  CSS failed: {css_url} ({err})");
                }
            }
        }
    } else {
        let text = loader(&css_url).unwrap_or_default();
        text_len = text.len();
        let mut stream_failed = false;
        emitted += emit_css_imports(
            &text,
            &css_url,
            &media,
            &loader,
            None,
            &mut import_stack,
            None,
            &mut stream_failed,
            &mut |fragment| {
                combined.append_fragment(fragment.clone());
                emit_fragment(fragment);
            },
        );
        emitted += stream_stylesheet_fragments(&text, &css_url, &media, |fragment| {
            combined.append_fragment(fragment.clone());
            emit_fragment(fragment);
        });
        if emitted == 0 {
            combined.parse_and_add_with_base_media_conditions(&text, &css_url, &media);
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
        replace_emitted,
    }
}

#[cfg(test)]
mod stylesheet_loader_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn decoded_image_is_shared_across_page_requests() {
        let url = "https://example.test/history-shared-image.png";
        let first_loader = |_url: &str| {
            Ok(html::DecodedImage::Raster(
                Arc::new(vec![255, 0, 0, 255]),
                1,
                1,
            ))
        };
        let first = cached_decoded_image_result(url, Some(&first_loader)).unwrap();
        let second_loader = |_url: &str| -> Result<html::DecodedImage, String> {
            panic!("decoded image was loaded again")
        };
        let second = cached_decoded_image_result(url, Some(&second_loader)).unwrap();
        let (
            html::DecodedImage::Raster(first_pixels, _, _),
            html::DecodedImage::Raster(second_pixels, _, _),
        ) = (first, second)
        else {
            panic!("expected cached raster images")
        };
        assert!(Arc::ptr_eq(&first_pixels, &second_pixels));
    }

    #[test]
    fn parsed_external_stylesheet_is_reused_across_document_loads() {
        let key = "test-parsed-css-revisit\nhttps://example.test/shared.css".to_string();
        let url = "https://example.test/shared.css".to_string();
        let loader: StylesheetLoader = Arc::new(|_| Ok(".shared { color: red }".into()));
        let first = load_stylesheet_cached(
            key.clone(),
            url.clone(),
            String::new(),
            loader,
            None,
            true,
            |_| {},
        );
        assert_eq!(first.sheet.rules.len(), 1);

        let never_fetch: StylesheetLoader = Arc::new(|_| panic!("cached CSS was fetched again"));
        let never_stream: StreamingStylesheetLoader =
            Arc::new(|_, _| panic!("cached CSS was streamed again"));
        let second = load_stylesheet_cached(
            key,
            url,
            String::new(),
            never_fetch,
            Some(never_stream),
            true,
            |_| {},
        );
        assert_eq!(second.sheet.rules.len(), 1);
        assert_eq!(second.text_len, 0);
        assert_eq!(second.emitted_fragments, 0);
    }

    #[test]
    fn streamed_data_url_background_rule_is_preserved() {
        let css = r#".bg-\[url\(\'data\:image\/svg\+xml\;base64\2c AAA\'\)\]{color:red}.loader{background-image:url("data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=");background-size:12rem}.disabled{opacity:.4}"#.to_string();
        let escaped_semicolon = css.find("\\;").expect("escaped selector semicolon");
        let mut partial = CssStreamBuffer::default();
        assert!(partial.push(&css[..escaped_semicolon + 2]).is_none());
        let mut expected = css::Stylesheet::default();
        expected.parse_and_add_with_base(&css, "https://example.test/main.css");
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |_, emit| {
            let mut start = 0;
            while start < css.len() {
                let mut end = (start + 4096).min(css.len());
                while !css.is_char_boundary(end) {
                    end -= 1;
                }
                emit(&css[start..end]);
                start = end;
            }
            Ok(())
        });
        let loader: StylesheetLoader = Arc::new(|_| Err("not expected".into()));
        let result = load_stylesheet_cached(
            "test-streamed-data-url-background".into(),
            "https://example.test/main.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |_| {},
        );
        let expected_selectors: Vec<_> = expected
            .rules
            .iter()
            .map(|rule| rule.original_selector.as_str())
            .collect();
        let actual_selectors: Vec<_> = result
            .sheet
            .rules
            .iter()
            .map(|rule| rule.original_selector.as_str())
            .collect();
        assert_eq!(actual_selectors, expected_selectors);
        assert!(actual_selectors.contains(&".loader"));
    }

    #[test]
    fn css_stream_boundary_survives_chunked_comments_strings_and_escapes() {
        let css = r#"/* a } */ .a\;b { content: "} ;"; background: url("data:image/svg+xml;utf8,<svg></svg>") } /* split */ .next { color: red }"#;
        let mut stream = CssStreamBuffer::default();
        let mut drained = String::new();
        for chunk in css.as_bytes().chunks(1) {
            if let Some(complete) = stream.push(std::str::from_utf8(chunk).unwrap()) {
                drained.push_str(&complete);
            }
        }
        drained.push_str(&stream.text);
        assert_eq!(drained, css);
        assert!(stream.text.is_empty());
        assert_eq!(stream.scanned, 0);
    }

    #[test]
    fn css_stream_scans_only_new_bytes_of_an_unfinished_rule() {
        let mut stream = CssStreamBuffer::default();
        assert!(stream.push(".large { content: '").is_none());
        for _ in 0..4096 {
            assert!(stream.push("x").is_none());
            assert_eq!(stream.scanned, stream.text.len());
        }
        let complete = stream.push("'; color: red }").unwrap();
        assert!(complete.starts_with(".large { content: '"));
        assert!(complete.ends_with("'; color: red }"));
        assert!(stream.text.is_empty());
    }

    #[test]
    fn imported_css_precedes_parent_and_resolves_its_own_font_urls() {
        let loader: StylesheetLoader = Arc::new(|url| {
            match url {
            "https://site.test/base.css" => Ok(
                "@import url('fonts/fonts.css'); .title { color: blue }".into(),
            ),
            "https://site.test/fonts/fonts.css" => Ok(
                "@import '../base.css'; @font-face { font-family: Demo; src: url('demo.woff2') } .title { color: red }".into(),
            ),
            _ => Err(format!("unexpected URL: {url}")),
        }
        });
        let result = load_stylesheet_cached(
            "test-import-order".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(result.sheet.rules.len(), 2);
        assert!(
            result.sheet.rules[0]
                .declarations
                .iter()
                .any(|decl| decl.1 == "red")
        );
        assert!(
            result.sheet.rules[1]
                .declarations
                .iter()
                .any(|decl| decl.1 == "blue")
        );
        assert!(
            result.sheet.font_faces[0]
                .src
                .contains("https://site.test/fonts/demo.woff2")
        );
    }

    #[test]
    fn quoted_import_without_whitespace_precedes_parent_rule() {
        assert_eq!(
            css_import_target("@import\"theme.css\";"),
            Some(CssImportTarget {
                url: "theme.css".to_string(),
                media: "",
                layer: None,
                supports: None,
            })
        );
        assert!(css_import_target("@importurl(theme.css);").is_none());
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => {
                Ok("@import\"theme.css\"; .title { color: blue }".into())
            }
            "https://site.test/theme.css" => Ok(".title { color: red }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let result = load_stylesheet_cached(
            "test-adjacent-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(result.sheet.rules.len(), 2);
        assert_eq!(
            result.sheet.rules[0]
                .declarations
                .get("color")
                .map(String::as_str),
            Some("red")
        );
        assert_eq!(
            result.sheet.rules[1]
                .declarations
                .get("color")
                .map(String::as_str),
            Some("blue")
        );
    }

    #[test]
    fn css_import_urls_decode_escapes_and_reject_bad_url_tokens() {
        assert_eq!(
            css_import_target(r#"@import "th\65 me.css";"#).unwrap().url,
            "theme.css"
        );
        assert_eq!(
            css_import_target(r#"@import url(a\)b.css);"#).unwrap().url,
            "a)b.css"
        );
        assert_eq!(
            css_import_target(r#"@import url("a\"b.css");"#)
                .unwrap()
                .url,
            "a\"b.css"
        );
        for invalid in [
            "@import theme.css;",
            "@import url(foo bar);",
            "@import url(foo(bar));",
            "@import url(\"unterminated);",
            "@import \"unterminated;",
        ] {
            assert!(css_import_target(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn escaped_import_fetches_decoded_url_before_parent_rule() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => {
                Ok(r#"@import "th\65 me.css"; .title { color: blue }"#.into())
            }
            "https://site.test/theme.css" => Ok(".title { color: red }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let result = load_stylesheet_cached(
            "test-escaped-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(result.sheet.rules.len(), 2);
        let colors: Vec<_> = result
            .sheet
            .rules
            .iter()
            .map(|rule| rule.declarations.get("color").map(String::as_str))
            .collect();
        assert_eq!(colors, [Some("red"), Some("blue")]);
    }

    #[test]
    fn conditional_import_skips_false_supports_without_fetching() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => Ok(concat!(
                "@import 'skip.css' supports(display: invented-value);",
                "@import 'use.css' supports(display: grid);",
                ".title { color: blue }"
            )
            .into()),
            "https://site.test/use.css" => Ok(".title { color: red }".into()),
            _ => panic!("unexpected import fetch: {url}"),
        });
        let result = load_stylesheet_cached(
            "test-conditional-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(result.sheet.rules.len(), 2);
        assert_eq!(
            result.sheet.rules[0]
                .declarations
                .get("color")
                .map(String::as_str),
            Some("red")
        );
        assert_eq!(
            result.sheet.rules[1]
                .declarations
                .get("color")
                .map(String::as_str),
            Some("blue")
        );
    }

    #[test]
    fn imported_media_list_stays_bounded_by_the_link_media() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => Ok("@import 'theme.css' screen, print;".into()),
            "https://site.test/theme.css" => Ok(".title { color: red }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let result = load_stylesheet_cached(
            "test-import-media-conjunction".into(),
            "https://site.test/base.css".into(),
            "print".into(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(result.sheet.rules.len(), 1);
        assert!(!result.sheet.rules[0].media_condition.matches(900.0, 600.0));
    }

    #[test]
    fn imported_layer_scopes_rules_and_nested_layers_in_source_order() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => Ok(concat!(
                "@import 'theme.css' supports(display: grid) layer(theme);",
                "@layer override { .title { color: blue } }"
            )
            .into()),
            "https://site.test/theme.css" => Ok(concat!(
                ".title { color: red }",
                "@layer accent { .title { color: green } }"
            )
            .into()),
            _ => panic!("unexpected import fetch: {url}"),
        });
        let result = load_stylesheet_cached(
            "test-layered-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        );
        assert_eq!(
            result.sheet.layer_order,
            ["theme", "theme.accent", "override"]
        );
        assert_eq!(
            result
                .sheet
                .rules
                .iter()
                .map(|rule| rule.layer.as_str())
                .collect::<Vec<_>>(),
            ["theme", "theme.accent", "override"]
        );
        assert_eq!(
            css_import_target("@import 'x.css' layer(foo) supports(display: grid); ")
                .unwrap()
                .layer,
            Some(Some("foo"))
        );
    }

    #[test]
    fn imported_layer_media_condition_controls_layer_order() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => Ok(concat!(
                "@import 'layout.css' layer(layout) (min-width: 700px);",
                "@layer theme, layout;",
                "@layer theme { .title { color: blue } }"
            )
            .into()),
            "https://site.test/layout.css" => Ok(".title { color: green }".into()),
            _ => panic!("unexpected import fetch: {url}"),
        });
        let mut sheet = load_stylesheet_cached(
            "test-conditional-layered-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            None,
            false,
            |_| {},
        )
        .sheet;
        for (width, first) in [(600.0, "theme"), (800.0, "layout"), (600.0, "theme")] {
            sheet.set_layer_viewport(width, 600.0);
            sheet.rebuild_index();
            assert_eq!(sheet.layer_rank(first), 0, "{width}");
        }
    }

    #[test]
    fn streamed_layer_declaration_is_not_dropped_as_empty_css() {
        let loader: StylesheetLoader = Arc::new(|_| panic!("streaming fetch should be used"));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|_, emit| {
            emit("@layer first, second;");
            Ok(())
        });
        let mut emitted = Vec::new();
        let result = load_stylesheet_cached(
            "test-layer-only-stream".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |fragment| emitted.extend(fragment.layer_order),
        );
        assert_eq!(result.emitted_fragments, 1);
        assert_eq!(result.sheet.layer_order, ["first", "second"]);
        assert_eq!(emitted, ["first", "second"]);
    }

    #[test]
    fn streamed_css_import_is_emitted_before_parent_rule() {
        let parent = "@import 'https://site.test/theme.css'; .title { color: blue }";
        let loader: StylesheetLoader = Arc::new(move |url| match url {
            "https://site.test/theme.css" => Ok(".title { color: red }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |url, emit| {
            match url {
                "https://site.test/base.css" => {
                    for chunk in parent.as_bytes().chunks(9) {
                        emit(std::str::from_utf8(chunk).unwrap());
                    }
                }
                "https://site.test/theme.css" => emit(".title { color: red }"),
                _ => return Err(format!("unexpected URL: {url}")),
            }
            Ok(())
        });
        let mut seen = Vec::new();
        let result = load_stylesheet_cached(
            "test-streamed-import-order".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| {
                seen.extend(
                    sheet
                        .rules
                        .into_iter()
                        .map(|rule| rule.declarations.get("color").unwrap().clone()),
                )
            },
        );
        assert_eq!(seen, ["red", "blue"]);
        assert_eq!(result.sheet.rules.len(), 2);
    }

    #[test]
    fn nested_css_imports_use_the_streaming_loader_in_source_order() {
        let loader: StylesheetLoader =
            Arc::new(|url| Err(format!("complete fetch forbidden: {url}")));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|url, emit| {
            let css = match url {
                "https://site.test/base.css" => "@import 'theme.css'; .title { color: blue }",
                "https://site.test/theme.css" => "@import 'colors.css'; .title { color: red }",
                "https://site.test/colors.css" => ".title { color: green }",
                _ => return Err(format!("unexpected URL: {url}")),
            };
            for chunk in css.as_bytes().chunks(7) {
                emit(std::str::from_utf8(chunk).unwrap());
            }
            Ok(())
        });
        let mut emitted_colors = Vec::new();
        let result = load_stylesheet_cached(
            "test-nested-streamed-imports".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| {
                emitted_colors.extend(
                    sheet
                        .rules
                        .iter()
                        .filter_map(|rule| rule.declarations.get("color").cloned()),
                );
            },
        );
        assert_eq!(emitted_colors, ["green", "red", "blue"]);
        assert_eq!(result.sheet.rules.len(), 3);
    }

    #[test]
    fn imported_rule_is_emitted_before_its_stream_finishes() {
        let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let checked = observed.clone();
        let loader: StylesheetLoader =
            Arc::new(|url| Err(format!("complete fetch forbidden: {url}")));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |url, emit| {
            match url {
                "https://site.test/base.css" => emit("@import 'theme.css';"),
                "https://site.test/theme.css" => {
                    emit(".title { color: red }");
                    assert!(checked.load(std::sync::atomic::Ordering::SeqCst));
                    emit(".subtitle { color: blue }");
                }
                _ => return Err(format!("unexpected URL: {url}")),
            }
            Ok(())
        });
        let result = load_stylesheet_cached(
            "test-progressive-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| {
                if sheet
                    .rules
                    .iter()
                    .any(|rule| rule.original_selector == ".title")
                {
                    observed.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            },
        );
        assert_eq!(result.sheet.rules.len(), 2);
    }

    #[test]
    fn failed_import_stream_replaces_published_rules_in_parent_sheet() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => {
                Ok("@import 'theme.css'; .parent { color: blue }".into())
            }
            "https://site.test/theme.css" => Ok(".theme { color: green }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|url, emit| match url {
            "https://site.test/base.css" => {
                emit("@import 'theme.css'; .parent { color: blue }");
                Ok(())
            }
            "https://site.test/theme.css" => {
                emit(".theme { color: red }");
                Err("import interrupted".into())
            }
            _ => Err(format!("unexpected URL: {url}")),
        });
        let mut published = Vec::new();
        let loaded = load_stylesheet_cached(
            "test-failed-import-stream".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| published.extend(sheet.rules.into_iter().map(|rule| rule.original_selector)),
        );
        assert!(published.iter().any(|selector| selector == ".theme"));
        assert!(loaded.replace_emitted);
        let colors: Vec<_> = loaded
            .sheet
            .rules
            .iter()
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["green", "blue"]);
    }

    #[test]
    fn empty_failed_import_stream_uses_complete_import_without_parent_retry() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/theme.css" => Ok(".theme { color: green }".into()),
            _ => Err(format!("parent must not be retried: {url}")),
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|url, emit| match url {
            "https://site.test/base.css" => {
                emit("@import 'theme.css'; .parent { color: blue }");
                Ok(())
            }
            "https://site.test/theme.css" => Err("no imported bytes".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let loaded = load_stylesheet_cached(
            "test-empty-failed-import-stream".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |_| {},
        );
        assert!(!loaded.replace_emitted);
        let colors: Vec<_> = loaded
            .sheet
            .rules
            .iter()
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["green", "blue"]);
    }

    #[test]
    fn nested_import_stream_failure_replaces_entire_parent_sheet() {
        let loader: StylesheetLoader = Arc::new(|url| match url {
            "https://site.test/base.css" => {
                Ok("@import 'middle.css'; .parent { color: blue }".into())
            }
            "https://site.test/middle.css" => {
                Ok("@import 'child.css'; .middle { color: red }".into())
            }
            "https://site.test/child.css" => Ok(".child { color: orange }".into()),
            _ => Err(format!("unexpected URL: {url}")),
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|url, emit| match url {
            "https://site.test/base.css" => {
                emit("@import 'middle.css'; .parent { color: blue }");
                Ok(())
            }
            "https://site.test/middle.css" => {
                emit("@import 'child.css'; .middle { color: red }");
                Ok(())
            }
            "https://site.test/child.css" => {
                emit(".child { color: green }");
                Err("nested import interrupted".into())
            }
            _ => Err(format!("unexpected URL: {url}")),
        });
        let loaded = load_stylesheet_cached(
            "test-failed-nested-import-stream".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |_| {},
        );
        assert!(loaded.replace_emitted);
        let colors: Vec<_> = loaded
            .sheet
            .rules
            .iter()
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["orange", "red", "blue"]);
    }

    #[test]
    fn parent_css_stream_advances_while_import_fetch_waits() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let parent_advanced = Arc::new(AtomicBool::new(false));
        let import_timed_out = Arc::new(AtomicBool::new(false));
        let advanced = parent_advanced.clone();
        let timed_out = import_timed_out.clone();
        let loader: StylesheetLoader =
            Arc::new(|url| Err(format!("complete fetch forbidden: {url}")));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |url, emit| {
            match url {
                "https://site.test/base.css" => {
                    emit("@import 'theme.css';");
                    advanced.store(true, Ordering::SeqCst);
                    emit(".title { color: blue }");
                }
                "https://site.test/theme.css" => {
                    for _ in 0..100 {
                        if advanced.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    if !advanced.load(Ordering::SeqCst) {
                        timed_out.store(true, Ordering::SeqCst);
                    }
                    emit(".title { color: red }");
                }
                _ => return Err(format!("unexpected URL: {url}")),
            }
            Ok(())
        });
        let mut colors = Vec::new();
        let result = load_stylesheet_cached(
            "test-parent-import-concurrency".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| {
                colors.extend(
                    sheet
                        .rules
                        .iter()
                        .filter_map(|rule| rule.declarations.get("color").cloned()),
                );
            },
        );
        assert!(!import_timed_out.load(Ordering::SeqCst));
        assert_eq!(colors, ["red", "blue"]);
        assert_eq!(result.sheet.rules.len(), 2);
    }

    #[test]
    fn sibling_imports_fetch_concurrently_but_publish_in_source_order() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let second_started = Arc::new(AtomicBool::new(false));
        let first_timed_out = Arc::new(AtomicBool::new(false));
        let started = second_started.clone();
        let timed_out = first_timed_out.clone();
        let loader: StylesheetLoader =
            Arc::new(|url| Err(format!("complete fetch forbidden: {url}")));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |url, emit| {
            match url {
                "https://site.test/base.css" => {
                    emit("@import 'first.css'; @import 'second.css'; .parent { color: blue }");
                }
                "https://site.test/first.css" => {
                    for _ in 0..500 {
                        if started.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    if !started.load(Ordering::SeqCst) {
                        timed_out.store(true, Ordering::SeqCst);
                    }
                    emit(".first { color: red }");
                }
                "https://site.test/second.css" => {
                    started.store(true, Ordering::SeqCst);
                    emit(".second { color: green }");
                }
                _ => return Err(format!("unexpected URL: {url}")),
            }
            Ok(())
        });
        let mut published = Vec::new();
        let loaded = load_stylesheet_cached(
            "test-concurrent-sibling-imports".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| published.extend(sheet.rules.into_iter().map(|rule| rule.original_selector)),
        );
        assert!(!first_timed_out.load(Ordering::SeqCst));
        assert_eq!(published, [".first", ".second", ".parent"]);
        assert_eq!(loaded.sheet.rules.len(), 3);
    }

    #[test]
    fn streamed_import_after_style_rule_does_not_fetch() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let late_import_fetched = Arc::new(AtomicBool::new(false));
        let fetched = late_import_fetched.clone();
        let loader: StylesheetLoader =
            Arc::new(|url| Err(format!("complete fetch forbidden: {url}")));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |url, emit| {
            match url {
                "https://site.test/base.css" => {
                    emit(".title { color: blue }");
                    emit("@import 'late.css';");
                }
                "https://site.test/late.css" => {
                    fetched.store(true, Ordering::SeqCst);
                    emit(".title { color: red }");
                }
                _ => return Err(format!("unexpected URL: {url}")),
            }
            Ok(())
        });
        let result = load_stylesheet_cached(
            "test-streamed-late-import".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |_| {},
        );
        assert!(!late_import_fetched.load(Ordering::SeqCst));
        assert_eq!(result.sheet.rules.len(), 1);
        assert_eq!(
            result.sheet.rules[0].declarations.get("color"),
            Some(&"blue".to_string())
        );
    }

    #[test]
    fn failed_partial_stream_returns_complete_replacement_without_duplicate_emit() {
        let loader: StylesheetLoader = Arc::new(|url| {
            assert_eq!(url, "https://site.test/base.css");
            Ok(".title { color: blue } .after { display: block }".into())
        });
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|url, emit| {
            assert_eq!(url, "https://site.test/base.css");
            emit(".title { color: red }");
            Err("stream interrupted".into())
        });
        let mut emitted = Vec::new();
        let loaded = load_stylesheet_cached(
            "test-failed-partial-css-stream".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |sheet| emitted.extend(sheet.rules.into_iter().map(|rule| rule.original_selector)),
        );
        assert_eq!(emitted, [".title"]);
        assert!(loaded.replace_emitted);
        assert_eq!(loaded.sheet.rules.len(), 2);
        assert_eq!(
            loaded.sheet.rules[0].declarations.get("color"),
            Some(&"blue".to_string())
        );
    }

    #[test]
    fn failed_partial_stream_and_retry_discards_published_prefix() {
        let loader: StylesheetLoader = Arc::new(|_| Err("retry failed".into()));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|_, emit| {
            emit(".title { color: red }");
            Err("stream interrupted".into())
        });
        let loaded = load_stylesheet_cached(
            "test-failed-css-retry".into(),
            "https://site.test/base.css".into(),
            String::new(),
            loader,
            Some(streaming_loader),
            false,
            |_| {},
        );
        assert!(loaded.replace_emitted);
        assert!(loaded.sheet.rules.is_empty());
    }

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

    #[test]
    fn css_wait_counts_completed_stylesheets_not_streamed_fragments() {
        let loader: StylesheetLoader = Arc::new(|_| panic!("streaming fetch should be used"));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|_, emit| {
            emit(".first { color: red }");
            std::thread::sleep(std::time::Duration::from_millis(30));
            emit(".second { color: blue }");
            Ok(())
        });
        let doc = load_html_reusing_with_resource_loaders_and_wait(
            "<link rel=stylesheet href='https://css-wait-fragments.test/one.css'><div class=second>text</div>",
            "https://css-wait-fragments.test/",
            800.0,
            600.0,
            types::ComponentRegistry::default(),
            None,
            Some(loader),
            Some(streaming_loader),
            None,
            false,
            std::time::Duration::from_secs(1),
        );
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".first")
        );
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".second")
        );
        assert!(doc.pending_stylesheets.is_none());
    }

    #[test]
    fn css_wait_replaces_failed_stream_before_first_paint() {
        let loader: StylesheetLoader = Arc::new(|_| Ok(".target { color: blue }".to_string()));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(|_, emit| {
            emit(".target { color: red }");
            Err("interrupted stream".into())
        });
        let doc = load_html_reusing_with_resource_loaders_and_wait(
            "<link rel=stylesheet href='https://css-retry.test/one.css'><div class=target>text</div>",
            "https://css-retry.test/",
            800.0,
            600.0,
            types::ComponentRegistry::default(),
            None,
            Some(loader),
            Some(streaming_loader),
            None,
            false,
            std::time::Duration::from_secs(1),
        );
        let colors: Vec<_> = doc
            .stylesheet
            .rules
            .iter()
            .filter(|rule| rule.original_selector == ".target")
            .filter_map(|rule| rule.declarations.get("color"))
            .map(String::as_str)
            .collect();
        assert_eq!(colors, ["blue"]);
    }

    #[test]
    fn css_wait_timeout_keeps_later_streamed_fragments() {
        let (resume_tx, resume_rx) = std::sync::mpsc::channel::<()>();
        let resume_rx = Arc::new(Mutex::new(resume_rx));
        let loader: StylesheetLoader = Arc::new(|_| panic!("streaming fetch should be used"));
        let streaming_loader: StreamingStylesheetLoader = Arc::new(move |_, emit| {
            emit(".first { color: red }");
            resume_rx.lock().unwrap().recv().unwrap();
            emit(".second { color: blue }");
            Ok(())
        });
        let mut doc = load_html_reusing_with_resource_loaders_and_wait(
            "<link rel=stylesheet href='https://css-wait-timeout.test/one.css'><div class=second>text</div>",
            "https://css-wait-timeout.test/",
            800.0,
            600.0,
            types::ComponentRegistry::default(),
            None,
            Some(loader),
            Some(streaming_loader),
            None,
            false,
            std::time::Duration::from_millis(10),
        );
        assert!(
            !doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".second")
        );
        assert!(doc.pending_stylesheets.is_some());
        resume_tx.send(()).unwrap();
        for _ in 0..100 {
            doc.poll_pending_stylesheets_budgeted(usize::MAX, std::time::Duration::ZERO);
            if doc
                .stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".second")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".first")
        );
        assert!(
            doc.stylesheet
                .rules
                .iter()
                .any(|rule| rule.original_selector == ".second")
        );
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
pub mod embedded_window;
pub mod fonts;
pub mod frame;
pub mod html;
pub mod images;
pub mod layout;
pub mod loading;
pub mod markdown;
pub mod platform;
pub mod profile;
pub mod renderer;
pub mod scheduling;
pub mod svg;
pub mod ui_events;
pub mod video;
pub mod widgets;
/// WHATWG HTML §7 — browsing contexts and the `Window` interface.
pub mod window;
pub use fonts as woff;

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
    let (css_tx, css_rx) = mpsc::channel::<types::PendingStylesheetResult>();
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
                                let _ = sender.send(types::PendingStylesheetResult::fragment(
                                    idx,
                                    abs.clone(),
                                    fragment,
                                    media.clone(),
                                ));
                            },
                        );
                        if loaded.replace_emitted || loaded.emitted_fragments == 0 {
                            let update = if loaded.replace_emitted {
                                types::PendingStylesheetResult::replace(
                                    idx,
                                    abs.clone(),
                                    loaded.sheet.clone(),
                                    media.clone(),
                                )
                            } else {
                                types::PendingStylesheetResult::fragment(
                                    idx,
                                    abs.clone(),
                                    loaded.sheet.clone(),
                                    media.clone(),
                                )
                            };
                            let _ = sender.send(update);
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
    crate::profile::record(crate::profile::Phase::HtmlParse, t0.elapsed());
    drop(css_tx); // close sender so rx.iter() terminates after all threads finish

    // Collect fetched stylesheets. Browser hosts that want progressive first
    // paint can pass zero here; deterministic/headless paths keep the legacy
    // short wait so screenshots include fast stylesheets.
    let t1 = std::time::Instant::now();
    let expected_count = css_idx.load(std::sync::atomic::Ordering::SeqCst);
    let mut css_results: Vec<types::PendingStylesheetResult> = Vec::new();
    let mut css_finished = expected_count == 0;
    if !css_wait.is_zero() && !css_finished {
        let deadline = std::time::Instant::now() + css_wait;
        loop {
            if std::time::Instant::now() >= deadline {
                break;
            }
            match css_rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            {
                Ok(item) => css_results.push(item),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    css_finished = true;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
            }
        }
    }
    eprintln!(
        "CSS wait: {:.0}ms ({} fragments, {} sheets{})",
        t1.elapsed().as_millis(),
        css_results.len(),
        expected_count,
        if css_finished {
            " complete"
        } else {
            " pending"
        }
    );
    let has_pending_css = !css_finished;
    if !doc.document_stylesheets.is_empty() {
        let mut fetched_map: std::collections::HashMap<String, crate::css::Stylesheet> =
            std::collections::HashMap::new();
        let mut fetched_slots: std::collections::HashMap<usize, crate::css::Stylesheet> =
            std::collections::HashMap::new();
        for update in css_results {
            let types::PendingStylesheetResult {
                slot,
                url,
                sheet,
                kind,
                ..
            } = update;
            if kind == types::StylesheetUpdateKind::Replace {
                fetched_slots.insert(slot, sheet.clone());
                fetched_map.insert(url, sheet);
            } else {
                fetched_slots
                    .entry(slot)
                    .and_modify(|existing| existing.append_fragment(sheet.clone()))
                    .or_insert_with(|| sheet.clone());
                fetched_map
                    .entry(url)
                    .and_modify(|existing| existing.append_fragment(sheet.clone()))
                    .or_insert(sheet);
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
                crate::types::DocumentStylesheet::Inline { css, media } => {
                    if !crate::css::evaluate_media(media, doc.viewport_w, doc.viewport_h) {
                        continue;
                    }
                    let sheet = parsed_inline_stylesheet(css, &doc.base_url);
                    doc.stylesheet.append_fragment((*sheet).clone());
                    doc.inline_stylesheet_cache.insert(
                        idx,
                        types::CachedInlineStylesheet {
                            source: css.clone(),
                            base_url: doc.base_url.clone(),
                            sheet,
                        },
                    );
                }
                crate::types::DocumentStylesheet::Linked { href, media } => {
                    if !crate::css::evaluate_media(media, doc.viewport_w, doc.viewport_h) {
                        continue;
                    }
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
        css_results.sort_by_key(|update| update.slot);
        if !css_results.is_empty() {
            doc.pending_stylesheet_base = Some(doc.stylesheet.clone());
        }
        for update in css_results {
            let types::PendingStylesheetResult {
                slot,
                url,
                sheet,
                kind,
                ..
            } = update;
            if kind == types::StylesheetUpdateKind::Replace {
                doc.loaded_stylesheet_slots.insert(slot, sheet.clone());
                doc.loaded_linked_stylesheets.insert(url, sheet);
            } else {
                doc.loaded_stylesheet_slots
                    .entry(slot)
                    .and_modify(|existing| existing.append_fragment(sheet.clone()))
                    .or_insert_with(|| sheet.clone());
                doc.loaded_linked_stylesheets
                    .entry(url)
                    .and_modify(|existing| existing.append_fragment(sheet.clone()))
                    .or_insert(sheet);
            }
        }
        if let Some(base) = &doc.pending_stylesheet_base {
            doc.stylesheet = base.clone();
            let mut slots: Vec<_> = doc.loaded_stylesheet_slots.iter().collect();
            slots.sort_by_key(|(idx, _)| **idx);
            for (_, sheet) in slots {
                doc.stylesheet.append_fragment(sheet.clone());
            }
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
    let mut owned = reuse.is_none().then(Renderer::new);
    let renderer: &mut Renderer = match reuse {
        Some(r) => r,
        None => owned.as_mut().expect("renderer allocated when not reused"),
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
        doc.device_pixel_ratio,
        &mut Vec::new(),
        &mut pending,
    );
    if pending.is_empty() {
        return;
    }

    let mut async_pending = Vec::with_capacity(pending.len());
    let base_url = doc.base_url.clone();
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
                            let _ = html::set_decoded_bg_image_for_url_on_node(
                                node, decoded, &url, &base_url,
                            );
                        }
                        types::PendingImageTarget::BackgroundLayer(layer_index) => {
                            let _ = html::set_decoded_bg_image_layer_for_url_on_node(
                                node,
                                layer_index,
                                decoded,
                                &url,
                                &base_url,
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
        doc.device_pixel_ratio,
        &mut Vec::new(),
        &mut pending,
    );
    let base_url = doc.base_url.clone();
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
                    let _ =
                        html::set_decoded_bg_image_for_url_on_node(node, decoded, &url, &base_url);
                }
                types::PendingImageTarget::BackgroundLayer(layer_index) => {
                    let _ = html::set_decoded_bg_image_layer_for_url_on_node(
                        node,
                        layer_index,
                        decoded,
                        &url,
                        &base_url,
                    );
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
    device_pixel_ratio: f32,
    path: &mut Vec<usize>,
    pending: &mut Vec<(u32, Vec<usize>, types::PendingImageTarget, String)>,
) {
    let can_paint_resource = node_can_paint_resource(node);
    if can_paint_resource
        && (node.is_image_element() || node.tag == "video")
        && node.image_data.is_none()
    {
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
                    device_pixel_ratio,
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
    if can_paint_resource
        && (node.bg_image_data.is_none() || node.style.rare().background_image_set_source.is_some())
        && !node.style.background_image_url.is_empty()
    {
        let selected = node.style.background_image_url_for_dpr(device_pixel_ratio);
        let resolved = html::resolve_url(&selected, base_url);
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
        if loaded && layer.image_set_source.is_none() {
            continue;
        }
        let selected = layer.image_url_for_dpr(device_pixel_ratio);
        let resolved = html::resolve_url(&selected, base_url);
        let url = resolved.as_str();
        if can_paint_resource && is_async_image_url(url) {
            pending.push((
                node.node_id,
                path.clone(),
                types::PendingImageTarget::BackgroundLayer(layer_index),
                url.to_string(),
            ));
        }
    }
    if can_paint_resource
        && (node.mask_image_data.is_none() || node.style.rare().mask_image_set_source.is_some())
        && !node.style.rare().mask_image_url.is_empty()
    {
        let selected = node.style.mask_image_url_for_dpr(device_pixel_ratio);
        let resolved = html::resolve_url(&selected, base_url);
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
            collect_remote_images(
                child,
                base_url,
                viewport_w,
                viewport_h,
                device_pixel_ratio,
                path,
                pending,
            );
        }
    }
    for (i, child) in node.children.iter().enumerate() {
        path.push(i);
        collect_remote_images(
            child,
            base_url,
            viewport_w,
            viewport_h,
            device_pixel_ratio,
            path,
            pending,
        );
        path.pop();
    }
}

fn node_can_paint_resource(node: &types::WebCore) -> bool {
    if matches!(node.style.display, types::Display::None) || !node.style.visibility {
        return false;
    }
    let rect = node.layout.border_rect;
    let has_layout = node.layout.last_containing_width > 0.0
        || node.layout.content_rect.w > 0.0
        || node.layout.content_rect.h > 0.0
        || node.layout.margin_rect.w > 0.0
        || node.layout.margin_rect.h > 0.0;
    if !has_layout || (rect.w > 0.0 && rect.h > 0.0) {
        return true;
    }
    node.is_image_element() && rect.w > 0.0 && image_source_is_known(node)
}

fn image_source_is_known(node: &types::WebCore) -> bool {
    !node.resolved_src.is_empty() || html::image_fallback_source(node).is_some()
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

/// Return a shared `reqwest::blocking::Client` with browser-like defaults.
/// Handles gzip/brotli/deflate decompression and redirects automatically.
/// Only sets shared headers (UA, Accept-Language, Sec-CH-UA); callers should
/// add request-specific headers (Accept, Sec-Fetch-Dest, etc.) per request.
/// Clones retain the connection pool across resources and navigations.
pub fn http_client() -> reqwest::blocking::Client {
    static CLIENT: std::sync::LazyLock<reqwest::blocking::Client> =
        std::sync::LazyLock::new(|| build_http_client(false));
    CLIENT.clone()
}

/// Lenient client that accepts certs where the base domain (without www.)
/// is in the SAN but the exact subdomain isn't — matches Chrome behaviour
/// for shared-hosting certs.
pub fn http_client_lenient() -> reqwest::blocking::Client {
    static CLIENT: std::sync::LazyLock<reqwest::blocking::Client> =
        std::sync::LazyLock::new(|| build_http_client(true));
    CLIENT.clone()
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
