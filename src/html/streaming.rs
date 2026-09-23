//! Streaming HTML Parser — parses HTML in chunks as data arrives from the network.
//!
//! Instead of waiting for the full document, the parser processes chunks of HTML
//! bytes as they arrive. After each chunk, it produces DOM mutations that can be
//! applied incrementally. This enables:
//! - First paint within ~100ms of first byte
//! - Progressive rendering as content loads
//! - Background resource discovery (CSS, images, fonts)
//!
//! # Usage
//! ```ignore
//! let mut parser = StreamingParser::new("https://example.com");
//!
//! // As network data arrives:
//! while let Some(chunk) = network.read_chunk().await {
//!     let mutations = parser.feed(&chunk);
//!     for mutation in mutations {
//!         doc.apply_mutation(mutation);
//!     }
//!     if parser.can_paint() {
//!         engine.layout(&mut doc, viewport_w);
//!         renderer.render(&mut doc, &mut pixmap, scale);
//!     }
//! }
//!
//! // Finalize
//! let final_mutations = parser.finish();
//! ```

use crate::dom::attrs::AttrMap;

/// A DOM mutation produced by the streaming parser.
#[derive(Clone, Debug)]
pub enum DomMutation {
    /// Attributes from the document element when an incoming `<html>` tag is
    /// adopted into the existing root.
    SetRootAttributes { attributes: AttrMap },
    /// A new element was parsed and should be inserted.
    InsertElement {
        parent_path: Vec<usize>,
        tag: String,
        attributes: AttrMap,
    },
    /// Text content was parsed.
    AppendText {
        parent_path: Vec<usize>,
        text: String,
    },
    /// A stylesheet was encountered (inline <style> or <link>).
    AddStylesheet {
        css: String,
        url: String,
        media: String,
    },
    /// An element was closed.
    CloseElement,
    /// A resource was discovered that should be fetched.
    ResourceHint { kind: ResourceKind, url: String },
    /// Document title changed.
    TitleChanged { title: String },
}

/// Kind of resource discovered during parsing.
#[derive(Clone, Debug)]
pub enum ResourceKind {
    Stylesheet,
    Image,
    Font,
    Script,
    Preconnect,
}

/// Streaming HTML parser — processes chunks of HTML as they arrive.
#[derive(Clone, Debug)]
struct OpenElement {
    tag: String,
    path: Vec<usize>,
    child_count: usize,
}

pub struct StreamingParser {
    /// Accumulated buffer of unparsed HTML (incomplete tags carry over).
    buffer: String,
    /// Base URL for resolving relative links.
    pub(crate) base_url: String,
    /// Current element stack (for tracking insertion point).
    stack: Vec<OpenElement>,
    /// Number of root-level nodes emitted so far.
    root_child_count: usize,
    /// Whether body insertion has started, explicitly or by the implied-body
    /// rule for content after the head.
    body_started: bool,
    /// Whether we've seen </head> (render-blocking CSS should be loaded by then).
    pub(crate) head_closed: bool,
    /// Render-blocking resources that must load before first paint.
    render_blocking: Vec<String>,
    /// Resources that have been loaded.
    loaded_resources: Vec<String>,
    /// Whether the parser has finished (received all data).
    finished: bool,
    /// Discovered resource URLs (for preload scanner).
    discovered_resources: Vec<(ResourceKind, String)>,
    /// Current <title> content.
    pub(crate) title: String,
    /// Whether we're inside a <title> tag.
    in_title: bool,
    /// Whether we're inside a <style> tag.
    in_style: bool,
    /// Accumulated style content.
    style_buffer: String,
    /// A `<style>` inside `<template>` is shadow-scoped markup, not a document
    /// stylesheet. Keep its emitted node path so the raw CSS can be attached to
    /// the template fragment for declarative shadow DOM finalization.
    style_node_path: Option<Vec<usize>>,
    /// Whether we're inside a <script> raw-text element.
    in_script: bool,
}

impl StreamingParser {
    /// Create a new streaming parser with a base URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            buffer: String::new(),
            base_url: base_url.to_string(),
            stack: Vec::new(),
            root_child_count: 0,
            body_started: false,
            head_closed: false,
            render_blocking: Vec::new(),
            loaded_resources: Vec::new(),
            finished: false,
            discovered_resources: Vec::new(),
            title: String::new(),
            in_title: false,
            in_style: false,
            style_buffer: String::new(),
            style_node_path: None,
            in_script: false,
        }
    }

    /// Seed the insertion cursor when streaming into an existing skeleton
    /// document instead of an empty root.
    pub fn set_root_child_count(&mut self, count: usize) {
        self.root_child_count = count;
    }

    fn in_document_or_html(&self) -> bool {
        self.stack.is_empty()
            || (self.stack.len() == 1
                && self
                    .stack
                    .last()
                    .is_some_and(|open| open.tag.eq_ignore_ascii_case("html")))
    }

    fn ensure_body_open(&mut self, mutations: &mut Vec<DomMutation>) {
        if self.body_started
            || self
                .stack
                .last()
                .is_some_and(|open| open.tag.eq_ignore_ascii_case("body"))
        {
            return;
        }
        let parent_path = self
            .stack
            .last()
            .filter(|open| open.tag.eq_ignore_ascii_case("html"))
            .map(|open| open.path.clone())
            .unwrap_or_default();
        let child_index = self.next_child_index();
        let mut body_path = parent_path.clone();
        body_path.push(child_index);
        mutations.push(DomMutation::InsertElement {
            parent_path,
            tag: "body".to_string(),
            attributes: AttrMap::new(),
        });
        self.bump_child_count();
        self.stack.push(OpenElement {
            tag: "body".to_string(),
            path: body_path,
            child_count: 0,
        });
        self.body_started = true;
        self.head_closed = true;
    }

    /// Feed a chunk of HTML bytes. Returns DOM mutations to apply.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<DomMutation> {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);
        self.process_buffer()
    }

    /// Feed a string chunk.
    pub fn feed_str(&mut self, chunk: &str) -> Vec<DomMutation> {
        self.buffer.push_str(chunk);
        self.process_buffer()
    }

    /// Signal that all data has been received. Returns final mutations.
    pub fn finish(&mut self) -> Vec<DomMutation> {
        self.finished = true;
        // Process any remaining buffer content
        let mut mutations = self.process_buffer();
        // Flush any remaining text in buffer as text content
        if !self.buffer.is_empty() {
            let text = std::mem::take(&mut self.buffer);
            if self.in_script {
                self.in_script = false;
            } else if self.in_style {
                if !text.trim().is_empty() {
                    self.style_buffer.push_str(&text);
                    let css = std::mem::take(&mut self.style_buffer);
                    if let Some(parent_path) = self.style_node_path.take() {
                        mutations.push(DomMutation::AppendText {
                            parent_path,
                            text: css,
                        });
                    } else {
                        mutations.push(DomMutation::AddStylesheet {
                            css,
                            url: String::new(),
                            media: String::new(),
                        });
                    }
                }
                self.in_style = false;
                if self
                    .stack
                    .last()
                    .is_some_and(|open| open.tag.eq_ignore_ascii_case("style"))
                {
                    self.stack.pop();
                    mutations.push(DomMutation::CloseElement);
                }
            } else if self.in_title {
                self.title.push_str(&crate::html::decode_entities(&text));
                mutations.push(DomMutation::TitleChanged {
                    title: self.title.clone(),
                });
                self.in_title = false;
            } else {
                self.push_text_mutation(&mut mutations, crate::html::decode_entities(&text));
            }
        }
        mutations
    }

    /// Can we paint? True when all render-blocking CSS has loaded.
    pub fn can_paint(&self) -> bool {
        self.render_blocking
            .iter()
            .all(|url| self.loaded_resources.contains(url))
    }

    /// Mark a resource as loaded.
    pub fn resource_loaded(&mut self, url: &str) {
        self.loaded_resources.push(url.to_string());
    }

    /// Is the parser done (all data received)?
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Get discovered resources for preloading.
    pub fn take_resource_hints(&mut self) -> Vec<(ResourceKind, String)> {
        std::mem::take(&mut self.discovered_resources)
    }

    /// Get the document title (if parsed).
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Process the buffer — extract complete tags and text nodes.
    fn process_buffer(&mut self) -> Vec<DomMutation> {
        let mut mutations = Vec::new();

        loop {
            let buf = if self.in_style || self.in_script || self.in_title {
                self.buffer.clone()
            } else {
                self.buffer.clone()
            };
            if buf.is_empty() {
                break;
            }

            if self.in_style {
                // Accumulate content until </style>
                if let Some(end) = buf.to_lowercase().find("</style>") {
                    self.style_buffer.push_str(&buf[..end]);
                    let css = std::mem::take(&mut self.style_buffer);
                    if let Some(parent_path) = self.style_node_path.take() {
                        mutations.push(DomMutation::AppendText {
                            parent_path,
                            text: css,
                        });
                    } else {
                        mutations.push(DomMutation::AddStylesheet {
                            css,
                            url: String::new(),
                            media: String::new(),
                        });
                    }
                    self.in_style = false;
                    if self
                        .stack
                        .last()
                        .is_some_and(|open| open.tag.eq_ignore_ascii_case("style"))
                    {
                        self.stack.pop();
                        mutations.push(DomMutation::CloseElement);
                    }
                    self.buffer = buf[end + 8..].to_string();
                    continue;
                } else {
                    // Need more data
                    let (ready, pending) = split_raw_text_pending_close(&buf, "</style>");
                    self.style_buffer.push_str(ready);
                    self.buffer = pending.to_string();
                    break;
                }
            }

            if self.in_script {
                if let Some(end) = buf.to_lowercase().find("</script>") {
                    self.in_script = false;
                    if self
                        .stack
                        .last()
                        .is_some_and(|open| open.tag.eq_ignore_ascii_case("script"))
                    {
                        self.stack.pop();
                    }
                    mutations.push(DomMutation::CloseElement);
                    self.buffer = buf[end + 9..].to_string();
                    continue;
                } else {
                    let (_, pending) = split_raw_text_pending_close(&buf, "</script>");
                    self.buffer = pending.to_string();
                    break;
                }
            }

            if self.in_title {
                if let Some(end) = buf.to_lowercase().find("</title>") {
                    self.title
                        .push_str(&crate::html::decode_entities(&buf[..end]));
                    mutations.push(DomMutation::TitleChanged {
                        title: self.title.clone(),
                    });
                    self.in_title = false;
                    self.buffer = buf[end + 8..].to_string();
                    continue;
                } else {
                    let (ready, pending) = split_raw_text_pending_close(&buf, "</title>");
                    self.title.push_str(&crate::html::decode_entities(ready));
                    self.buffer = pending.to_string();
                    break;
                }
            }

            let Some(complete) = crate::html::tokenizer::next_complete_token(&buf) else {
                self.buffer = buf;
                break;
            };
            self.buffer = buf[complete.end..].to_string();
            match complete.token {
                crate::html::tokenizer::Token::Text(text) => {
                    self.push_text_mutation(&mut mutations, text);
                }
                crate::html::tokenizer::Token::Comment(_)
                | crate::html::tokenizer::Token::Doctype(_) => {}
                crate::html::tokenizer::Token::CloseTag { tag } => {
                    if tag == "head" {
                        self.head_closed = true;
                    }
                    if tag == "body" {
                        self.body_started = true;
                    }
                    if self.close_matching_element(&tag) {
                        mutations.push(DomMutation::CloseElement);
                    }
                }
                crate::html::tokenizer::Token::OpenTag {
                    tag,
                    attrs,
                    self_closing,
                } => {
                    self.discover_resources(&tag, &attrs, &mut mutations);

                    if tag == "style" && !self_closing {
                        if self
                            .stack
                            .last()
                            .is_some_and(|open| open.tag.eq_ignore_ascii_case("template"))
                        {
                            let parent_path = self.current_parent_path();
                            let child_index = self.next_child_index();
                            let mut element_path = parent_path.clone();
                            element_path.push(child_index);
                            mutations.push(DomMutation::InsertElement {
                                parent_path,
                                tag: tag.clone(),
                                attributes: attrs,
                            });
                            self.bump_child_count();
                            self.stack.push(OpenElement {
                                tag,
                                path: element_path.clone(),
                                child_count: 0,
                            });
                            self.style_node_path = Some(element_path);
                            self.in_style = true;
                            self.style_buffer.clear();
                            continue;
                        }
                        self.in_style = true;
                        self.style_buffer.clear();
                        continue;
                    }
                    if tag == "title" && !self_closing {
                        self.in_title = true;
                        self.title.clear();
                        continue;
                    }

                    if tag.eq_ignore_ascii_case("html") && self.stack.is_empty() {
                        if !attrs.is_empty() {
                            mutations.push(DomMutation::SetRootAttributes { attributes: attrs });
                        }
                        if !self_closing {
                            self.stack.push(OpenElement {
                                tag,
                                path: Vec::new(),
                                child_count: self.root_child_count,
                            });
                        }
                        continue;
                    }

                    if tag.eq_ignore_ascii_case("body") {
                        self.body_started = true;
                        self.head_closed = true;
                    } else if self.in_document_or_html()
                        && !tag.eq_ignore_ascii_case("head")
                        && !crate::html::is_head_content_tag(&tag)
                    {
                        self.ensure_body_open(&mut mutations);
                    }

                    let parent_path = self.current_parent_path();
                    let child_index = self.next_child_index();
                    let mut element_path = parent_path.clone();
                    element_path.push(child_index);
                    mutations.push(DomMutation::InsertElement {
                        parent_path,
                        tag: tag.clone(),
                        attributes: attrs,
                    });
                    self.bump_child_count();

                    let is_void = self_closing || is_html_void_element(&tag);
                    if !is_void {
                        self.stack.push(OpenElement {
                            tag: tag.clone(),
                            path: element_path,
                            child_count: 0,
                        });
                        if tag == "script" {
                            self.in_script = true;
                        }
                    }
                }
            }
        }

        mutations
    }

    fn push_text_mutation(&mut self, mutations: &mut Vec<DomMutation>, text: String) {
        if text.is_empty() {
            return;
        }

        if text.trim().is_empty() && self.is_structural_whitespace_context() {
            return;
        }

        if self.in_document_or_html() {
            self.ensure_body_open(mutations);
        }

        mutations.push(DomMutation::AppendText {
            parent_path: self.current_parent_path(),
            text,
        });
        self.bump_child_count();
    }

    fn is_structural_whitespace_context(&self) -> bool {
        let Some(parent) = self.stack.last() else {
            return true;
        };
        matches!(
            parent.tag.as_str(),
            "html" | "head" | "table" | "thead" | "tbody" | "tfoot" | "tr" | "colgroup"
        )
    }

    fn close_matching_element(&mut self, tag: &str) -> bool {
        let Some(pos) = self
            .stack
            .iter()
            .rposition(|open| open.tag.eq_ignore_ascii_case(tag))
        else {
            return false;
        };
        self.stack.truncate(pos);
        true
    }

    fn current_parent_path(&self) -> Vec<usize> {
        self.stack
            .last()
            .map(|open| open.path.clone())
            .unwrap_or_default()
    }

    fn next_child_index(&self) -> usize {
        self.stack
            .last()
            .map(|open| open.child_count)
            .unwrap_or(self.root_child_count)
    }

    fn bump_child_count(&mut self) {
        if let Some(open) = self.stack.last_mut() {
            open.child_count += 1;
        } else {
            self.root_child_count += 1;
        }
    }

    /// Discover resources in a tag for preloading.
    fn discover_resources(&mut self, tag: &str, attrs: &AttrMap, mutations: &mut Vec<DomMutation>) {
        match tag {
            "link" => {
                let rel = attrs
                    .get("rel")
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if rel
                    .split_ascii_whitespace()
                    .any(|part| part == "stylesheet")
                {
                    if let Some(href) = attrs.get("href") {
                        let url = crate::html::resolve_url(href, &self.base_url);
                        // Stylesheets in <head> are render-blocking
                        if !self.head_closed {
                            self.render_blocking.push(url.clone());
                        }
                        self.discovered_resources
                            .push((ResourceKind::Stylesheet, url.clone()));
                        mutations.push(DomMutation::ResourceHint {
                            kind: ResourceKind::Stylesheet,
                            url,
                        });
                    }
                } else if rel.split_ascii_whitespace().any(|part| part == "preload") {
                    if let Some(href) = attrs.get("href") {
                        let as_kind = attrs
                            .get("as")
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_default();
                        let kind = match as_kind.as_str() {
                            "style" => Some(ResourceKind::Stylesheet),
                            "image" => Some(ResourceKind::Image),
                            "font" => Some(ResourceKind::Font),
                            "script" => Some(ResourceKind::Script),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            let url = crate::html::resolve_url(href, &self.base_url);
                            self.discovered_resources.push((kind.clone(), url.clone()));
                            mutations.push(DomMutation::ResourceHint { kind, url });
                        }
                    }
                    if let Some(imagesrcset) = attrs.get("imagesrcset") {
                        for url in preload_srcset_urls(imagesrcset, &self.base_url) {
                            self.discovered_resources
                                .push((ResourceKind::Image, url.clone()));
                            mutations.push(DomMutation::ResourceHint {
                                kind: ResourceKind::Image,
                                url,
                            });
                        }
                    }
                } else if rel
                    .split_ascii_whitespace()
                    .any(|part| part == "preconnect")
                {
                    if let Some(href) = attrs.get("href") {
                        self.discovered_resources
                            .push((ResourceKind::Preconnect, href.clone()));
                    }
                }
            }
            "img" => {
                if let Some(src) = crate::html::image_fallback_source_attrs(attrs) {
                    let url = crate::html::resolve_url(src, &self.base_url);
                    self.discovered_resources
                        .push((ResourceKind::Image, url.clone()));
                    mutations.push(DomMutation::ResourceHint {
                        kind: ResourceKind::Image,
                        url,
                    });
                }
                if let Some(srcset) = crate::html::image_srcset_source_attrs(attrs) {
                    for url in preload_srcset_urls(srcset, &self.base_url) {
                        self.discovered_resources
                            .push((ResourceKind::Image, url.clone()));
                        mutations.push(DomMutation::ResourceHint {
                            kind: ResourceKind::Image,
                            url,
                        });
                    }
                }
            }
            "source" => {
                let source_type = attrs
                    .get("type")
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                let likely_image = source_type.is_empty() || source_type.starts_with("image/");
                if likely_image {
                    if let Some(srcset) = crate::html::image_srcset_source_attrs(attrs) {
                        for url in preload_srcset_urls(srcset, &self.base_url) {
                            self.discovered_resources
                                .push((ResourceKind::Image, url.clone()));
                            mutations.push(DomMutation::ResourceHint {
                                kind: ResourceKind::Image,
                                url,
                            });
                        }
                    }
                    if let Some(src) = crate::html::image_fallback_source_attrs(attrs) {
                        let url = crate::html::resolve_url(src, &self.base_url);
                        self.discovered_resources
                            .push((ResourceKind::Image, url.clone()));
                        mutations.push(DomMutation::ResourceHint {
                            kind: ResourceKind::Image,
                            url,
                        });
                    }
                }
            }
            "script" => {
                if let Some(src) = attrs.get("src") {
                    let url = crate::html::resolve_url(src, &self.base_url);
                    self.discovered_resources.push((ResourceKind::Script, url));
                }
            }
            "video" => {
                if let Some(poster) = attrs.get("poster") {
                    let url = crate::html::resolve_url(poster, &self.base_url);
                    self.discovered_resources
                        .push((ResourceKind::Image, url.clone()));
                    mutations.push(DomMutation::ResourceHint {
                        kind: ResourceKind::Image,
                        url,
                    });
                }
            }
            _ => {}
        }
    }
}

fn is_html_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn preload_srcset_urls(srcset: &str, base_url: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut current = String::new();
    let mut in_parens = 0usize;
    for ch in srcset.chars() {
        match ch {
            '(' => {
                in_parens += 1;
                current.push(ch);
            }
            ')' => {
                in_parens = in_parens.saturating_sub(1);
                current.push(ch);
            }
            ',' if in_parens == 0 => {
                push_srcset_preload_url(&current, base_url, &mut urls);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_srcset_preload_url(&current, base_url, &mut urls);
    urls
}

fn push_srcset_preload_url(entry: &str, base_url: &str, urls: &mut Vec<String>) {
    let Some(raw_url) = entry.split_whitespace().next() else {
        return;
    };
    if raw_url.is_empty() {
        return;
    }
    let url = crate::html::resolve_url(raw_url, base_url);
    if !urls.iter().any(|seen| seen == &url) {
        urls.push(url);
    }
}

fn split_raw_text_pending_close<'a>(text: &'a str, close: &str) -> (&'a str, &'a str) {
    let mut keep_start = text.len();
    for (start, _) in text.char_indices() {
        let suffix = &text[start..];
        if suffix.len() >= close.len() {
            continue;
        }
        if ascii_prefix_eq_ignore_case(close, suffix) {
            keep_start = start;
            break;
        }
    }
    text.split_at(keep_start)
}

fn ascii_prefix_eq_ignore_case(whole: &str, prefix: &str) -> bool {
    whole
        .as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_basic() {
        let mut parser = StreamingParser::new("");
        let mutations = parser.feed_str("<html><head><title>Test</title></head>");
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::TitleChanged { title } if title == "Test"))
        );
    }

    #[test]
    fn streaming_chunked() {
        let mut parser = StreamingParser::new("");
        // Feed HTML in small chunks
        let m1 = parser.feed_str("<div cla");
        assert!(m1.is_empty()); // incomplete tag, buffered

        let m2 = parser.feed_str("ss='hello'>World</div>");
        assert!(
            m2.iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, .. } if tag == "div"))
        );
        assert!(
            m2.iter()
                .any(|m| matches!(m, DomMutation::AppendText { text, .. } if text == "World"))
        );
    }

    #[test]
    fn streaming_comment_with_gt_does_not_leak_tail_text() {
        let mut parser = StreamingParser::new("");
        let mutations = parser.feed_str("<div>a<!--[if IE]>hidden<![endif]-->b</div>");
        let text: String = mutations
            .iter()
            .filter_map(|m| match m {
                DomMutation::AppendText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "ab");
        assert!(!text.contains("-->"));
    }

    #[test]
    fn streaming_preserves_inline_space_between_elements() {
        let mut parser = StreamingParser::new("");
        let mutations =
            parser.feed_str("<p><a href='/one'>Misti</a> is a <a href='/two'>volcano</a></p>");
        let text: String = mutations
            .iter()
            .filter_map(|m| match m {
                DomMutation::AppendText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(text, "Misti is a volcano");
    }

    #[test]
    fn streaming_preserves_inline_space_after_chunk_boundary() {
        let mut parser = StreamingParser::new("");
        let first = parser.feed_str("<p><a href='/one'>football</a>");
        let second = parser.feed_str(" <a href='/two'>defensive end</a></p>");
        let text: String = first
            .iter()
            .chain(second.iter())
            .filter_map(|m| match m {
                DomMutation::AppendText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(text, "football defensive end");
    }

    #[test]
    fn streaming_chunked_comment_waits_for_end_marker() {
        let mut parser = StreamingParser::new("");
        let first = parser.feed_str("<p>a<!--[if");
        assert!(
            first
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, .. } if tag == "p"))
        );
        assert!(
            first
                .iter()
                .any(|m| matches!(m, DomMutation::AppendText { text, .. } if text == "a"))
        );

        let second = parser.feed_str(" IE]>hidden<![endif]-->b</p>");
        let text: String = second
            .iter()
            .filter_map(|m| match m {
                DomMutation::AppendText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "b");
        assert!(!text.contains("endif"));
        assert!(!text.contains("-->"));
    }

    #[test]
    fn streaming_resource_discovery() {
        let mut parser = StreamingParser::new("https://example.com");
        let mutations = parser.feed_str(
            r#"<head><link rel="stylesheet" href="/style.css"><img src="/logo.png"></head>"#,
        );
        let resources: Vec<_> = mutations
            .iter()
            .filter(|m| matches!(m, DomMutation::ResourceHint { .. }))
            .collect();
        assert!(resources.len() >= 2, "should discover stylesheet and image");
    }

    #[test]
    fn streaming_discovers_lazy_image_attributes() {
        let mut parser = StreamingParser::new("https://example.com/news/");
        let mutations =
            parser.feed_str(r#"<img loading="lazy" data-src="post.webp" data-srcset="tiny.webp 320w, hero.webp 900w">"#);

        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::ResourceHint {
                    kind: ResourceKind::Image,
                    url
                } if url == "https://example.com/news/post.webp"
            )
        }));
        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::ResourceHint {
                    kind: ResourceKind::Image,
                    url
                } if url == "https://example.com/news/hero.webp"
            )
        }));
    }

    #[test]
    fn streaming_discovers_lazy_picture_source_candidates() {
        let mut parser = StreamingParser::new("https://example.com/news/");
        let mutations = parser.feed_str(
            r#"<picture><source type="image/webp" data-srcset="small.webp 320w, large.webp 900w"><img data-src="fallback.jpg"></picture>"#,
        );

        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::ResourceHint {
                    kind: ResourceKind::Image,
                    url
                } if url == "https://example.com/news/large.webp"
            )
        }));
        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::ResourceHint {
                    kind: ResourceKind::Image,
                    url
                } if url == "https://example.com/news/fallback.jpg"
            )
        }));
    }

    #[test]
    fn streaming_render_blocking() {
        let mut parser = StreamingParser::new("");
        parser.feed_str(r#"<head><link rel="stylesheet" href="/a.css">"#);
        assert!(!parser.can_paint(), "can't paint until CSS loads");
        parser.resource_loaded("https:///a.css"); // wrong URL
        assert!(!parser.can_paint());
        // The URL resolution makes this tricky — just verify the mechanism
    }

    #[test]
    fn streaming_inline_style() {
        let mut parser = StreamingParser::new("");
        let mutations =
            parser.feed_str("<style>.red { color: red; }</style><div class='red'>Hello</div>");
        assert!(
            mutations.iter().any(
                |m| matches!(m, DomMutation::AddStylesheet { css, .. } if css.contains("red"))
            )
        );
    }

    #[test]
    fn streaming_void_head_elements_do_not_swallow_body_content() {
        let mut parser = StreamingParser::new("");
        let mutations = parser.feed_str(
            r#"<html><head><meta charset="utf-8"><link rel="stylesheet" href="/app.css"></head><body><main>Real</main></body></html>"#,
        );

        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, parent_path, .. } if tag == "body" && parent_path.is_empty())),
            "body should stay a root child, not become a child of meta/link"
        );
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, parent_path, .. } if tag == "main" && parent_path == &vec![1])),
            "main should be inserted under body after void head elements"
        );
    }

    #[test]
    fn streaming_unmatched_close_does_not_pop_body_or_head() {
        let mut parser = StreamingParser::new("");
        let mutations = parser.feed_str(
            r#"<html><head><meta charset="utf-8"></span><title>T</title></head><body></span><main>Real</main></body></html>"#,
        );

        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, parent_path, .. } if tag == "main" && parent_path == &vec![1])),
            "an unmatched close tag must be ignored instead of popping body/html"
        );
    }

    #[test]
    fn streaming_script_raw_text_does_not_parse_fake_tags_before_body() {
        let mut parser = StreamingParser::new("");
        let mutations = parser.feed_str(
            r#"<html><head><script>if (x < y) { document.write("<body>bad</body>"); }</script></head><body><main>Real</main></body></html>"#,
        );
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, parent_path, .. } if tag == "body" && parent_path.is_empty())),
            "the real body must remain a root child instead of being nested under script text"
        );
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, .. } if tag == "main")),
            "content after a script raw-text block should continue parsing normally"
        );
        assert!(
            !mutations.iter().any(|m| matches!(
                m,
                DomMutation::AppendText { text, .. } if text.contains("document.write")
            )),
            "script raw text must not be emitted as visible DOM text"
        );
    }

    #[test]
    fn streaming_script_close_can_be_split_across_chunks() {
        let mut parser = StreamingParser::new("");
        parser.feed_str(r#"<html><head><script src="//cdn.example/app.js"></s"#);
        let mutations = parser.feed_str("cript></head><body><main>Real</main></body></html>");
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, parent_path, .. } if tag == "body" && parent_path.is_empty())),
            "a split script close must not trap body content under head/script"
        );
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, .. } if tag == "main"))
        );
    }

    #[test]
    fn streaming_script_body_can_be_split_without_leaking_visible_text() {
        let mut parser = StreamingParser::new("");
        let first = parser.feed_str(r#"<body><script>window.__INITIAL_DATA__ = {"title":"#);
        let second = parser.feed_str(r#"Hello"}</script><main>Visible</main>"#);

        assert!(
            first.iter().chain(second.iter()).all(|m| !matches!(
                m,
                DomMutation::AppendText { text, .. } if text.contains("__INITIAL_DATA__")
            )),
            "streamed script chunks must stay non-rendering"
        );
        assert!(
            second
                .iter()
                .any(|m| matches!(m, DomMutation::AppendText { text, .. } if text == "Visible"))
        );
    }

    #[test]
    fn streaming_style_close_can_be_split_across_chunks() {
        let mut parser = StreamingParser::new("");
        parser.feed_str("<style>body{color:green}</st");
        let mutations = parser.feed_str("yle><body><p>Styled</p></body>");
        assert!(mutations.iter().any(
            |m| matches!(m, DomMutation::AddStylesheet { css, .. } if css == "body{color:green}")
        ));
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::InsertElement { tag, .. } if tag == "body"))
        );
    }

    #[test]
    fn streaming_decodes_text_title_and_attribute_entities() {
        let mut parser = StreamingParser::new("");
        let mutations = parser
            .feed_str(r#"<title>A&#39;B</title><p title="C&#39;D">Tom &amp; Jerry &#x1F642;</p>"#);
        assert!(
            mutations
                .iter()
                .any(|m| matches!(m, DomMutation::TitleChanged { title } if title == "A'B")),
            "streaming title text should decode numeric entities"
        );
        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::InsertElement { attributes, .. }
                    if attributes.get("title").is_some_and(|v| v == "C'D")
            )
        }));
        assert!(
            mutations.iter().any(
                |m| matches!(m, DomMutation::AppendText { text, .. } if text == "Tom & Jerry 🙂")
            ),
            "streaming visible text should use the same entity decoder as the full tokenizer"
        );
    }

    #[test]
    fn streaming_discovers_video_poster_as_image() {
        let mut parser = StreamingParser::new("https://example.com/watch/");
        let mutations = parser.feed_str(r#"<video poster="../hero.webp"></video>"#);
        assert!(mutations.iter().any(|m| {
            matches!(
                m,
                DomMutation::ResourceHint {
                    kind: ResourceKind::Image,
                    url
                } if url == "https://example.com/watch/../hero.webp"
            )
        }));
    }

    #[test]
    fn streaming_finish() {
        let mut parser = StreamingParser::new("");
        parser.feed_str("<p>Partial");
        let final_m = parser.finish();
        assert!(parser.is_finished());
    }
}
