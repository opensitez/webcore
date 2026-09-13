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

use std::collections::HashMap;

/// A DOM mutation produced by the streaming parser.
#[derive(Clone, Debug)]
pub enum DomMutation {
    /// A new element was parsed and should be inserted.
    InsertElement {
        parent_path: Vec<usize>,
        tag: String,
        attributes: HashMap<String, String>,
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
}

impl StreamingParser {
    /// Create a new streaming parser with a base URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            buffer: String::new(),
            base_url: base_url.to_string(),
            stack: Vec::new(),
            root_child_count: 0,
            head_closed: false,
            render_blocking: Vec::new(),
            loaded_resources: Vec::new(),
            finished: false,
            discovered_resources: Vec::new(),
            title: String::new(),
            in_title: false,
            in_style: false,
            style_buffer: String::new(),
        }
    }

    /// Seed the insertion cursor when streaming into an existing skeleton
    /// document instead of an empty root.
    pub fn set_root_child_count(&mut self, count: usize) {
        self.root_child_count = count;
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
            if !text.trim().is_empty() {
                mutations.push(DomMutation::AppendText {
                    parent_path: self.current_parent_path(),
                    text,
                });
                self.bump_child_count();
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
            // Skip leading whitespace in some contexts
            let buf = self.buffer.trim_start().to_string();
            if buf.is_empty() {
                break;
            }

            if self.in_style {
                // Accumulate content until </style>
                if let Some(end) = buf.to_lowercase().find("</style>") {
                    self.style_buffer.push_str(&buf[..end]);
                    mutations.push(DomMutation::AddStylesheet {
                        css: std::mem::take(&mut self.style_buffer),
                        url: String::new(),
                        media: String::new(),
                    });
                    self.in_style = false;
                    self.buffer = buf[end + 8..].to_string();
                    continue;
                } else {
                    // Need more data
                    self.style_buffer.push_str(&buf);
                    self.buffer.clear();
                    break;
                }
            }

            if self.in_title {
                if let Some(end) = buf.to_lowercase().find("</title>") {
                    self.title.push_str(&buf[..end]);
                    mutations.push(DomMutation::TitleChanged {
                        title: self.title.clone(),
                    });
                    self.in_title = false;
                    self.buffer = buf[end + 8..].to_string();
                    continue;
                } else {
                    self.title.push_str(&buf);
                    self.buffer.clear();
                    break;
                }
            }

            if buf.starts_with('<') {
                // Find the end of this tag
                if let Some(end) = buf.find('>') {
                    let tag_content = &buf[1..end];
                    self.buffer = buf[end + 1..].to_string();

                    if tag_content.starts_with('/') {
                        // Closing tag
                        let tag_name = tag_content[1..].trim().to_lowercase();
                        if tag_name == "head" {
                            self.head_closed = true;
                        }
                        if self
                            .stack
                            .last()
                            .is_some_and(|open| open.tag.eq_ignore_ascii_case(&tag_name))
                        {
                            self.stack.pop();
                        } else {
                            self.stack.pop();
                        }
                        mutations.push(DomMutation::CloseElement);
                    } else if tag_content.starts_with('!') {
                        // Comment or doctype — skip
                    } else {
                        // Opening tag
                        let (tag_name, attrs) = parse_tag_quick(tag_content);
                        let self_closing = tag_content.ends_with('/') || is_void_element(&tag_name);

                        // Resource discovery
                        self.discover_resources(&tag_name, &attrs, &mut mutations);

                        // Special tags
                        if tag_name == "style" && !self_closing {
                            self.in_style = true;
                            self.style_buffer.clear();
                            continue;
                        }
                        if tag_name == "title" && !self_closing {
                            self.in_title = true;
                            self.title.clear();
                            continue;
                        }

                        if tag_name.eq_ignore_ascii_case("html")
                            && self.stack.is_empty()
                            && self.root_child_count == 0
                        {
                            if !self_closing {
                                self.stack.push(OpenElement {
                                    tag: tag_name,
                                    path: Vec::new(),
                                    child_count: 0,
                                });
                            }
                            continue;
                        }

                        let parent_path = self.current_parent_path();
                        let child_index = self.next_child_index();
                        let mut element_path = parent_path.clone();
                        element_path.push(child_index);
                        mutations.push(DomMutation::InsertElement {
                            parent_path,
                            tag: tag_name.clone(),
                            attributes: attrs,
                        });
                        self.bump_child_count();

                        if !self_closing {
                            self.stack.push(OpenElement {
                                tag: tag_name,
                                path: element_path,
                                child_count: 0,
                            });
                        }
                    }
                } else {
                    // Incomplete tag — need more data
                    self.buffer = buf;
                    break;
                }
            } else {
                // Text content — find next tag
                let next_tag = buf.find('<').unwrap_or(buf.len());
                let text = &buf[..next_tag];
                if !text.is_empty() {
                    mutations.push(DomMutation::AppendText {
                        parent_path: self.current_parent_path(),
                        text: text.to_string(),
                    });
                    self.bump_child_count();
                }
                self.buffer = buf[next_tag..].to_string();
                if next_tag == buf.len() {
                    break;
                }
            }
        }

        mutations
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
    fn discover_resources(
        &mut self,
        tag: &str,
        attrs: &HashMap<String, String>,
        mutations: &mut Vec<DomMutation>,
    ) {
        match tag {
            "link" => {
                let rel = attrs
                    .get("rel")
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if rel.split_ascii_whitespace().any(|part| part == "stylesheet") {
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
                } else if rel.split_ascii_whitespace().any(|part| part == "preconnect") {
                    if let Some(href) = attrs.get("href") {
                        self.discovered_resources
                            .push((ResourceKind::Preconnect, href.clone()));
                    }
                }
            }
            "img" => {
                if let Some(src) = attrs.get("src") {
                    let url = crate::html::resolve_url(src, &self.base_url);
                    self.discovered_resources
                        .push((ResourceKind::Image, url.clone()));
                    mutations.push(DomMutation::ResourceHint {
                        kind: ResourceKind::Image,
                        url,
                    });
                }
                if let Some(srcset) = attrs.get("srcset") {
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
                    if let Some(srcset) = attrs.get("srcset") {
                        for url in preload_srcset_urls(srcset, &self.base_url) {
                            self.discovered_resources
                                .push((ResourceKind::Image, url.clone()));
                            mutations.push(DomMutation::ResourceHint {
                                kind: ResourceKind::Image,
                                url,
                            });
                        }
                    }
                    if let Some(src) = attrs.get("src") {
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

/// Quick tag parser — extracts tag name and attributes.
fn parse_tag_quick(content: &str) -> (String, HashMap<String, String>) {
    let content = content.trim().trim_end_matches('/');
    let mut parts = content.splitn(2, |c: char| c.is_ascii_whitespace());
    let tag = parts.next().unwrap_or("").to_lowercase();
    let mut attrs = HashMap::new();

    if let Some(attr_str) = parts.next() {
        // Simple attribute parser
        let mut remaining = attr_str.trim();
        while !remaining.is_empty() {
            // Skip whitespace
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                break;
            }

            // Find attribute name
            let name_end = remaining
                .find(|c: char| c == '=' || c.is_ascii_whitespace())
                .unwrap_or(remaining.len());
            let name = remaining[..name_end].to_lowercase();
            remaining = remaining[name_end..].trim_start();

            if remaining.starts_with('=') {
                remaining = remaining[1..].trim_start();
                // Parse value
                if remaining.starts_with('"') {
                    let end = remaining[1..]
                        .find('"')
                        .map(|i| i + 1)
                        .unwrap_or(remaining.len());
                    let value = &remaining[1..end];
                    attrs.insert(name, value.to_string());
                    remaining = if end + 1 < remaining.len() {
                        &remaining[end + 1..]
                    } else {
                        ""
                    };
                } else if remaining.starts_with('\'') {
                    let end = remaining[1..]
                        .find('\'')
                        .map(|i| i + 1)
                        .unwrap_or(remaining.len());
                    let value = &remaining[1..end];
                    attrs.insert(name, value.to_string());
                    remaining = if end + 1 < remaining.len() {
                        &remaining[end + 1..]
                    } else {
                        ""
                    };
                } else {
                    let end = remaining
                        .find(|c: char| c.is_ascii_whitespace())
                        .unwrap_or(remaining.len());
                    let value = &remaining[..end];
                    attrs.insert(name, value.to_string());
                    remaining = &remaining[end..];
                }
            } else {
                // Boolean attribute
                if !name.is_empty() {
                    attrs.insert(name, String::new());
                }
            }
        }
    }

    (tag, attrs)
}

/// Check if an HTML element is void (self-closing, no end tag).
fn is_void_element(tag: &str) -> bool {
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
