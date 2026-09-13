pub mod serializer;

use crate::css::ua_stylesheet;
use crate::html::load_image_from_src;
use crate::types::*;
use std::collections::HashMap;

// ============================================================
// Markdown Parser — produces Document (Box tree)
// Mirrors MarkdownParser.cpp
// ============================================================

// Helper: create a new WebCore with appropriate tag defaults
fn make_box(tag: &str) -> WebCore {
    let mut b = WebCore::new(tag);
    // Mirror the UA stylesheet sizes so markdown output looks identical to HTML output.
    // Em values are resolved by the layout engine relative to the element's own font size.
    match tag {
        "h1" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(2.0);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(0.67);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(0.67);
        }
        "h2" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(1.5);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(0.83);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(0.83);
        }
        "h3" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(1.17);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "h4" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.33);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.33);
        }
        "h5" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(0.83);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.67);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.67);
        }
        "h6" => {
            std::sync::Arc::make_mut(&mut b.style).font_size = CssLength::Em(0.67);
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(2.33);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(2.33);
        }
        "p" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "blockquote" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_left = CssLength::Px(40.0);
            std::sync::Arc::make_mut(&mut b.style).margin_right = CssLength::Px(40.0);
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "pre" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).white_space = WhiteSpace::Pre;
            std::sync::Arc::make_mut(&mut b.style).font_family = "monospace".to_string();
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "ul" | "ol" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).padding_left = CssLength::Px(40.0);
        }
        "li" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::ListItem;
        }
        "hr" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(0.5);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(0.5);
        }
        "table" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Table;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "thead" | "tbody" | "tfoot" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::TableRowGroup;
        }
        "tr" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::TableRow;
        }
        "th" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::TableHeaderCell;
            std::sync::Arc::make_mut(&mut b.style).font_weight = FontWeight::Bold;
            std::sync::Arc::make_mut(&mut b.style).text_align = TextAlign::Center;
            std::sync::Arc::make_mut(&mut b.style).padding_top = CssLength::Px(4.0);
            std::sync::Arc::make_mut(&mut b.style).padding_bottom = CssLength::Px(4.0);
            std::sync::Arc::make_mut(&mut b.style).padding_left = CssLength::Px(8.0);
            std::sync::Arc::make_mut(&mut b.style).padding_right = CssLength::Px(8.0);
        }
        "td" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::TableCell;
            std::sync::Arc::make_mut(&mut b.style).padding_top = CssLength::Px(4.0);
            std::sync::Arc::make_mut(&mut b.style).padding_bottom = CssLength::Px(4.0);
            std::sync::Arc::make_mut(&mut b.style).padding_left = CssLength::Px(8.0);
            std::sync::Arc::make_mut(&mut b.style).padding_right = CssLength::Px(8.0);
        }
        "dl" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_top = CssLength::Em(1.0);
            std::sync::Arc::make_mut(&mut b.style).margin_bottom = CssLength::Em(1.0);
        }
        "dd" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
            std::sync::Arc::make_mut(&mut b.style).margin_left = CssLength::Px(40.0);
        }
        "dt" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
        }
        "img" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::InlineBlock;
        }
        "div" => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
        }
        _ => {
            std::sync::Arc::make_mut(&mut b.style).display = Display::Block;
        }
    }
    b
}

// Append a styled text run to a block box
fn append_run(block: &mut WebCore, text: &str, style: ComputedStyle) {
    if text.is_empty() {
        return;
    }
    let offset = block.text.len();
    block.text.push_str(text);
    block.layout.inline_runs.push(InlineRun {
        text_offset: offset,
        length: text.len(),
        style,
    });
}

// ============================================================
// Reference link definitions
// ============================================================

#[derive(Clone)]
struct RefLink {
    url: String,
}

fn to_lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

// Try to parse a reference link definition from a line.
// Format: [id]: url "optional title"
fn parse_ref_def(line: &str) -> Option<(String, RefLink)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    // Up to 3 leading spaces
    while i < bytes.len() && bytes[i] == b' ' && i < 3 {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'[' {
        return None;
    }
    i += 1; // skip [
    let id_start = i;
    while i < bytes.len() && bytes[i] != b']' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let id = to_lower(&line[id_start..i]);
    i += 1; // skip ]
    if i >= bytes.len() || bytes[i] != b':' {
        return None;
    }
    i += 1; // skip :
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    // URL (optionally in angle brackets)
    let url;
    if i < bytes.len() && bytes[i] == b'<' {
        i += 1;
        let url_start = i;
        while i < bytes.len() && bytes[i] != b'>' {
            i += 1;
        }
        url = line[url_start..i].to_string();
        if i < bytes.len() {
            i += 1; // skip >
        }
        let _ = i; // value not used further; url was already captured
    } else {
        let url_start = i;
        while i < bytes.len() && bytes[i] != b' ' {
            i += 1;
        }
        url = line[url_start..i].to_string();
    }
    if url.is_empty() {
        return None;
    }
    // Skip footnote defs (those have [^id]:)
    if id.starts_with('^') {
        return None;
    }
    Some((id, RefLink { url }))
}

// ============================================================
// Footnote definitions
// ============================================================

struct FootnoteDef {
    id: String,
    content: String,
}

// ============================================================
// Inline parser
// ============================================================

struct InlineParser<'a> {
    refs: &'a HashMap<String, RefLink>,
}

impl<'a> InlineParser<'a> {
    fn new(refs: &'a HashMap<String, RefLink>) -> Self {
        Self { refs }
    }

    fn parse(&self, block: &mut WebCore, text: &str) {
        let base_style = (*block.style).clone();
        // Reset inline-specific fields for base style
        let mut s = ComputedStyle::default();
        // Inherit font from block for headings etc
        s.font_size = base_style.font_size;
        s.font_weight = base_style.font_weight;
        s.font_style = base_style.font_style;
        self.parse_inner(block, text, s);
    }

    fn parse_inner(&self, block: &mut WebCore, text: &str, style: ComputedStyle) {
        let bytes = text.as_bytes();
        let len = bytes.len();
        let mut pos = 0usize;
        let mut accum = String::new();

        while pos < len {
            let c = bytes[pos];

            // Escaped character
            if c == b'\\' && pos + 1 < len {
                let next = bytes[pos + 1];
                let escapable = b"\\`*_{}[]()#+-.!|~>=";
                if escapable.contains(&next) {
                    accum.push(next as char);
                    pos += 2;
                    continue;
                }
            }

            // Inline code: `...`
            if c == b'`' {
                let mut ticks = 0usize;
                let _tick_start = pos;
                while pos < len && bytes[pos] == b'`' {
                    ticks += 1;
                    pos += 1;
                }
                let close_seq: String = std::iter::repeat('`').take(ticks).collect();
                if let Some(rel) = text[pos..].find(&close_seq) {
                    let close_pos = pos + rel;
                    // Make sure close is within bounds
                    flush_accum(&mut accum, block, &style);
                    let code = &text[pos..close_pos];
                    let code = if code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ') {
                        &code[1..code.len() - 1]
                    } else {
                        code
                    };
                    let mut code_style = style.clone();
                    code_style.font_family = "monospace".to_string();
                    code_style.font_weight = FontWeight::Normal;
                    code_style.font_style = FontStyle::Normal;
                    append_run(block, code, code_style);
                    pos = close_pos + ticks;
                } else {
                    // Not a code span
                    for _ in 0..ticks {
                        accum.push('`');
                    }
                }
                continue;
            }

            // Highlight: ==text==
            if c == b'=' && pos + 1 < len && bytes[pos + 1] == b'=' {
                if let Some(rel) = text[pos + 2..].find("==") {
                    let close_pos = pos + 2 + rel;
                    flush_accum(&mut accum, block, &style);
                    let mut hl_style = style.clone();
                    hl_style.background_color = Color::rgb(255, 255, 0);
                    block
                        .data
                        .insert("md-highlight".to_string(), "true".to_string());
                    let inner = &text[pos + 2..close_pos];
                    self.parse_inner(block, inner, hl_style);
                    pos = close_pos + 2;
                    continue;
                }
            }

            // Footnote reference: [^id]
            if c == b'[' && pos + 1 < len && bytes[pos + 1] == b'^' {
                if let Some(rel) = text[pos + 2..].find(']') {
                    let close_pos = pos + 2 + rel;
                    // Make sure it's NOT a footnote definition [^id]:
                    let after = close_pos + 1;
                    if after >= len || bytes[after] != b':' {
                        flush_accum(&mut accum, block, &style);
                        let fn_id = &text[pos + 2..close_pos];
                        let mut fn_style = style.clone();
                        fn_style.color = Color::rgb(0, 0, 238);
                        let display = format!("[{}]", fn_id);
                        append_run(block, &display, fn_style);
                        block
                            .data
                            .insert("md-footnote-ref".to_string(), fn_id.to_string());
                        pos = close_pos + 1;
                        continue;
                    }
                }
            }

            // Images: ![alt](url) — must check before links
            if c == b'!' && pos + 1 < len && bytes[pos + 1] == b'[' {
                let alt_start = pos + 2;
                if let Some(rel_close) = text[alt_start..].find(']') {
                    let alt_end = alt_start + rel_close;
                    if alt_end + 1 < len && bytes[alt_end + 1] == b'(' {
                        let url_start = alt_end + 2;
                        if let Some(rel_url_close) = text[url_start..].find(')') {
                            let url_end = url_start + rel_url_close;
                            flush_accum(&mut accum, block, &style);
                            let alt = &text[alt_start..alt_end];
                            let url = &text[url_start..url_end];
                            let mut img = make_box("img");
                            img.attributes.insert("src", url);
                            img.data.insert("md-alt".to_string(), alt.to_string());
                            // Load image pixel data (file path or data URL)
                            if let Some((data, w, h)) = load_image_from_src(url, "") {
                                if img.style.width.is_auto() {
                                    crate::css::apply_property(
                                        std::sync::Arc::make_mut(&mut img.style),
                                        "width",
                                        &format!("{}px", w),
                                    );
                                }
                                if img.style.height.is_auto() {
                                    crate::css::apply_property(
                                        std::sync::Arc::make_mut(&mut img.style),
                                        "height",
                                        &format!("{}px", h),
                                    );
                                }
                                img.image_data = Some(std::sync::Arc::new(data));
                                img.image_width = w;
                                img.image_height = h;
                            }
                            block.children.push(img);
                            pos = url_end + 1;
                            continue;
                        }
                    }
                }
            }

            // Autolink: <url> or <email>
            if c == b'<' {
                if let Some(rel) = text[pos + 1..].find('>') {
                    let close_pos = pos + 1 + rel;
                    let inner = &text[pos + 1..close_pos];
                    let is_url = inner.contains("://");
                    let is_email = !is_url && inner.contains('@') && !inner.contains(' ');
                    if is_url || is_email {
                        flush_accum(&mut accum, block, &style);
                        let url = if is_email {
                            format!("mailto:{}", inner)
                        } else {
                            inner.to_string()
                        };
                        let mut link_style = style.clone();
                        link_style.href = url;
                        link_style.color = Color::rgb(0, 0, 238);
                        link_style.text_decoration.underline = true;
                        block
                            .data
                            .insert("md-autolink".to_string(), "true".to_string());
                        append_run(block, inner, link_style);
                        pos = close_pos + 1;
                        continue;
                    }
                }
            }

            // Links: [text](url) or [text][id] or [text] (shortcut)
            if c == b'[' {
                let text_start = pos + 1;
                // Find matching closing bracket (simple, not nested)
                if let Some(rel) = text[text_start..].find(']') {
                    let text_end = text_start + rel;
                    let link_text = &text[text_start..text_end];

                    // Inline link: [text](url)
                    if text_end + 1 < len && bytes[text_end + 1] == b'(' {
                        let url_start = text_end + 2;
                        if let Some(rel_close) = text[url_start..].find(')') {
                            let url_end = url_start + rel_close;
                            flush_accum(&mut accum, block, &style);
                            let mut url = text[url_start..url_end].to_string();
                            // Strip optional title
                            if let Some(title_start) = url.find(" \"") {
                                if url.ends_with('"') {
                                    url = url[..title_start].to_string();
                                }
                            }
                            let mut link_style = style.clone();
                            link_style.href = url;
                            link_style.color = Color::rgb(0, 0, 238);
                            link_style.text_decoration.underline = true;
                            let link_text_owned = link_text.to_string();
                            self.parse_inner(block, &link_text_owned, link_style);
                            pos = url_end + 1;
                            continue;
                        }
                    }

                    // Reference link: [text][id] or [text][]
                    if text_end + 1 < len && bytes[text_end + 1] == b'[' {
                        let ref_start = text_end + 2;
                        if let Some(rel_ref) = text[ref_start..].find(']') {
                            let ref_end = ref_start + rel_ref;
                            let ref_raw = &text[ref_start..ref_end];
                            let ref_id = if ref_raw.is_empty() {
                                link_text.to_string()
                            } else {
                                ref_raw.to_string()
                            };
                            let ref_key = to_lower(&ref_id);
                            if let Some(rlink) = self.refs.get(&ref_key) {
                                flush_accum(&mut accum, block, &style);
                                let mut link_style = style.clone();
                                link_style.href = rlink.url.clone();
                                link_style.color = Color::rgb(0, 0, 238);
                                link_style.text_decoration.underline = true;
                                block.data.insert("md-ref-link".to_string(), ref_id);
                                let link_text_owned = link_text.to_string();
                                self.parse_inner(block, &link_text_owned, link_style);
                                pos = ref_end + 1;
                                continue;
                            }
                        }
                    }

                    // Shortcut reference link: [text] (no following ( or [)
                    let next_after = text_end + 1;
                    let next_char = if next_after < len {
                        Some(bytes[next_after])
                    } else {
                        None
                    };
                    if next_char != Some(b'(') && next_char != Some(b'[') {
                        let ref_key = to_lower(link_text);
                        if let Some(rlink) = self.refs.get(&ref_key) {
                            flush_accum(&mut accum, block, &style);
                            let mut link_style = style.clone();
                            link_style.href = rlink.url.clone();
                            link_style.color = Color::rgb(0, 0, 238);
                            link_style.text_decoration.underline = true;
                            block
                                .data
                                .insert("md-ref-link".to_string(), link_text.to_string());
                            block
                                .data
                                .insert("md-ref-shortcut".to_string(), "true".to_string());
                            let link_text_owned = link_text.to_string();
                            self.parse_inner(block, &link_text_owned, link_style);
                            pos = text_end + 1;
                            continue;
                        }
                    }
                }
            }

            // Bold + Italic: *** or ___
            if (c == b'*' || c == b'_')
                && pos + 2 < len
                && bytes[pos + 1] == c
                && bytes[pos + 2] == c
            {
                let delim = &text[pos..pos + 3];
                if let Some(rel) = text[pos + 3..].find(delim) {
                    let close_pos = pos + 3 + rel;
                    flush_accum(&mut accum, block, &style);
                    let mut bi_style = style.clone();
                    bi_style.font_weight = FontWeight::Bold;
                    bi_style.font_style = FontStyle::Italic;
                    let inner = &text[pos + 3..close_pos];
                    self.parse_inner(block, inner, bi_style);
                    pos = close_pos + 3;
                    continue;
                }
            }

            // Bold: ** or __
            if (c == b'*' || c == b'_') && pos + 1 < len && bytes[pos + 1] == c {
                let delim = &text[pos..pos + 2];
                if let Some(rel) = text[pos + 2..].find(delim) {
                    let close_pos = pos + 2 + rel;
                    flush_accum(&mut accum, block, &style);
                    let mut bold_style = style.clone();
                    bold_style.font_weight = FontWeight::Bold;
                    block
                        .data
                        .insert("md-bold-delim".to_string(), delim.to_string());
                    let inner = &text[pos + 2..close_pos];
                    self.parse_inner(block, inner, bold_style);
                    pos = close_pos + 2;
                    continue;
                }
            }

            // Italic: * or _
            if c == b'*' || c == b'_' {
                let delim_char = c as char;
                if let Some(rel) = text[pos + 1..].find(delim_char) {
                    let close_pos = pos + 1 + rel;
                    flush_accum(&mut accum, block, &style);
                    let mut italic_style = style.clone();
                    italic_style.font_style = FontStyle::Italic;
                    block
                        .data
                        .insert("md-italic-delim".to_string(), delim_char.to_string());
                    let inner = &text[pos + 1..close_pos];
                    self.parse_inner(block, inner, italic_style);
                    pos = close_pos + 1;
                    continue;
                }
            }

            // Strikethrough: ~~
            if c == b'~' && pos + 1 < len && bytes[pos + 1] == b'~' {
                if let Some(rel) = text[pos + 2..].find("~~") {
                    let close_pos = pos + 2 + rel;
                    flush_accum(&mut accum, block, &style);
                    let mut s_style = style.clone();
                    s_style.text_decoration.strikethrough = true;
                    let inner = &text[pos + 2..close_pos];
                    self.parse_inner(block, inner, s_style);
                    pos = close_pos + 2;
                    continue;
                }
            }

            // Line break
            if c == b'\n' {
                // Check for hard break (trailing spaces or backslash)
                let hard = if accum.ends_with("  ") {
                    while accum.ends_with(' ') {
                        accum.pop();
                    }
                    true
                } else if accum.ends_with('\\') {
                    accum.pop();
                    true
                } else {
                    false
                };

                if hard {
                    flush_accum(&mut accum, block, &style);
                    append_run(block, "\n", style.clone());
                } else {
                    if !accum.ends_with(' ') {
                        accum.push(' ');
                    }
                }
                pos += 1;
                continue;
            }

            // Regular character — check UTF-8 multi-byte
            let ch_len = utf8_char_len(bytes, pos);
            accum.push_str(&text[pos..pos + ch_len]);
            pos += ch_len;
        }

        flush_accum(&mut accum, block, &style);
    }
}

fn flush_accum(accum: &mut String, block: &mut WebCore, style: &ComputedStyle) {
    if accum.is_empty() {
        return;
    }
    let text = std::mem::take(accum);
    append_run(block, &text, style.clone());
}

// Get the byte length of the UTF-8 character at position `pos` in `bytes`
fn utf8_char_len(bytes: &[u8], pos: usize) -> usize {
    let b = bytes[pos];
    if b < 0x80 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

// ============================================================
// Block-level helpers
// ============================================================

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim_end_matches('\r').to_string())
        .collect()
}

fn is_blank(line: &str) -> bool {
    line.is_empty() || line.chars().all(|c| c == ' ')
}

fn is_thematic_break(line: &str) -> bool {
    let trimmed: String = line.chars().filter(|c| *c != ' ').collect();
    if trimmed.len() < 3 {
        return false;
    }
    let ch = trimmed.chars().next().unwrap();
    if ch != '-' && ch != '*' && ch != '_' {
        return false;
    }
    trimmed.chars().all(|c| c == ch)
}

fn atx_heading_level(line: &str) -> u8 {
    let bytes = line.as_bytes();
    let mut hashes = 0u8;
    while hashes < bytes.len() as u8 && hashes < 7 && bytes[hashes as usize] == b'#' {
        hashes += 1;
    }
    if hashes < 1 || hashes > 6 {
        return 0;
    }
    if hashes as usize >= bytes.len() || bytes[hashes as usize] != b' ' {
        return 0;
    }
    hashes
}

fn atx_content(line: &str) -> &str {
    let level = atx_heading_level(line);
    if level == 0 {
        return line;
    }
    let content = &line[level as usize + 1..]; // skip "# "
    // Strip trailing hashes
    let trimmed = content.trim_end();
    let stripped = trimmed.trim_end_matches('#').trim_end();
    stripped
}

fn fence_level(line: &str) -> (usize, char) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    if i >= bytes.len() {
        return (0, ' ');
    }
    let fc = bytes[i] as char;
    if fc != '`' && fc != '~' {
        return (0, ' ');
    }
    let mut count = 0;
    while i < bytes.len() && bytes[i] == fc as u8 {
        count += 1;
        i += 1;
    }
    if count < 3 { (0, ' ') } else { (count, fc) }
}

fn fence_info(line: &str) -> &str {
    let s = line.trim_start_matches(|c| c == '`' || c == '~' || c == ' ');
    s.trim_end()
}

fn is_blockquote_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' && i < 3 {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b'>'
}

fn strip_blockquote(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' && i < 3 {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'>' {
        i += 1;
        if i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
    }
    &line[i..]
}

#[derive(Debug, Default)]
struct ListInfo {
    indent: usize,
    marker: String,
    content_start: usize,
    ordered: bool,
    number: i32,
    valid: bool,
    task_state: i32, // -1 = not a task, 0 = unchecked, 1 = checked
}

fn detect_list_item(line: &str) -> ListInfo {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let indent = i;
    let mut info = ListInfo {
        indent,
        task_state: -1,
        ..Default::default()
    };

    if i >= bytes.len() {
        return info;
    }

    // Unordered: -, *, +
    if (bytes[i] == b'-' || bytes[i] == b'*' || bytes[i] == b'+')
        && i + 1 < bytes.len()
        && bytes[i + 1] == b' '
    {
        info.marker = (bytes[i] as char).to_string();
        info.content_start = i + 2;
        info.ordered = false;
        info.valid = true;

        // Check for task list
        let cs = info.content_start;
        if cs + 2 < bytes.len()
            && bytes[cs] == b'['
            && (bytes[cs + 1] == b' ' || bytes[cs + 1] == b'x' || bytes[cs + 1] == b'X')
            && bytes[cs + 2] == b']'
        {
            info.task_state = if bytes[cs + 1] == b' ' { 0 } else { 1 };
            info.content_start = cs + 3;
            if info.content_start < bytes.len() && bytes[info.content_start] == b' ' {
                info.content_start += 1;
            }
        }
        return info;
    }

    // Ordered: 1. or 1)
    let num_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > num_start && i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
        if i + 1 < bytes.len() && bytes[i + 1] == b' ' {
            info.marker = line[num_start..i + 1].to_string();
            info.number = line[num_start..i].parse().unwrap_or(1);
            info.content_start = i + 2;
            info.ordered = true;
            info.valid = true;
            return info;
        }
    }

    info
}

fn setext_level(line: &str) -> u8 {
    let trimmed: String = line.chars().filter(|c| *c != ' ').collect();
    if trimmed.len() < 3 {
        return 0;
    }
    if trimmed.chars().all(|c| c == '=') {
        return 1;
    }
    if trimmed.chars().all(|c| c == '-') {
        return 2;
    }
    0
}

fn is_table_separator(line: &str) -> bool {
    if !line.contains('-') {
        return false;
    }
    line.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

fn split_table_row(line: &str) -> Vec<String> {
    let mut s = line;
    if s.starts_with('|') {
        s = &s[1..];
    }
    if s.ends_with('|') {
        s = &s[..s.len() - 1];
    }
    s.split('|').map(|cell| cell.trim().to_string()).collect()
}

fn parse_table_alignments(sep: &str) -> Vec<TextAlign> {
    split_table_row(sep)
        .into_iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            if left && right {
                TextAlign::Center
            } else if right {
                TextAlign::Right
            } else {
                TextAlign::Left
            }
        })
        .collect()
}

fn is_indented_code_line(line: &str) -> bool {
    if line.is_empty() {
        return true;
    }
    (line.len() >= 4 && line.starts_with("    ")) || line.starts_with('\t')
}

fn strip_indented_code(line: &str) -> &str {
    if line.starts_with("    ") {
        &line[4..]
    } else if line.starts_with('\t') {
        &line[1..]
    } else {
        line
    }
}

fn is_html_block_start(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' && i < 3 {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'<' {
        return false;
    }
    i += 1;
    if i < bytes.len() && bytes[i] == b'/' {
        i += 1;
    }
    let tag_start = i;
    while i < bytes.len() && (bytes[i] as char).is_ascii_alphabetic() {
        i += 1;
    }
    let tag_name = line[tag_start..i].to_ascii_lowercase();
    static BLOCK_TAGS: &[&str] = &[
        "address",
        "article",
        "aside",
        "base",
        "basefont",
        "blockquote",
        "body",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dialog",
        "dir",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "iframe",
        "legend",
        "li",
        "link",
        "main",
        "menu",
        "menuitem",
        "nav",
        "noframes",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "section",
        "source",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "tr",
        "track",
        "ul",
    ];
    BLOCK_TAGS.contains(&tag_name.as_str())
}

fn is_definition_marker(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i] == b' ' && i < 3 {
        i += 1;
    }
    i < bytes.len() && bytes[i] == b':' && i + 1 < bytes.len() && bytes[i + 1] == b' '
}

// ============================================================
// Block parser
// ============================================================

struct BlockParser<'a> {
    lines: &'a [String],
    pos: usize,
    refs: &'a HashMap<String, RefLink>,
    footnote_defs: &'a [FootnoteDef],
}

impl<'a> BlockParser<'a> {
    fn new(
        lines: &'a [String],
        refs: &'a HashMap<String, RefLink>,
        footnote_defs: &'a [FootnoteDef],
    ) -> Self {
        Self {
            lines,
            pos: 0,
            refs,
            footnote_defs,
        }
    }

    fn parse_blocks(&mut self, parent: &mut WebCore) {
        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];

            // Blank line
            if is_blank(line) {
                self.pos += 1;
                continue;
            }

            // Skip reference link definitions (already collected)
            if parse_ref_def(line).is_some() {
                self.pos += 1;
                continue;
            }

            // Skip footnote definitions
            if line.len() > 4 && line.starts_with("[^") {
                if let Some(close) = line.find("]:") {
                    if close > 2 {
                        self.pos += 1;
                        // Skip continuation lines
                        while self.pos < self.lines.len()
                            && !self.lines[self.pos].is_empty()
                            && (self.lines[self.pos].starts_with(' ')
                                || self.lines[self.pos].starts_with('\t'))
                        {
                            self.pos += 1;
                        }
                        continue;
                    }
                }
            }

            // Thematic break
            if is_thematic_break(line) {
                let mut hr = make_box("hr");
                hr.data
                    .insert("md-marker".to_string(), line.trim().to_string());
                parent.children.push(hr);
                self.pos += 1;
                continue;
            }

            // ATX heading
            let heading_level = atx_heading_level(line);
            if heading_level > 0 {
                let tag = format!("h{}", heading_level);
                let mut heading = make_box(&tag);
                heading
                    .data
                    .insert("md-heading".to_string(), "atx".to_string());
                let content = atx_content(line).to_string();
                let ip = InlineParser::new(self.refs);
                ip.parse(&mut heading, &content);
                parent.children.push(heading);
                self.pos += 1;
                continue;
            }

            // Fenced code block
            let (fence_len, fence_char) = fence_level(line);
            if fence_len > 0 {
                let info = fence_info(line).to_string();
                let fence_str: String = std::iter::repeat(fence_char).take(fence_len).collect();
                let mut pre = make_box("pre");
                pre.data.insert("md-fence".to_string(), fence_str.clone());
                if !info.is_empty() {
                    pre.data.insert("md-lang".to_string(), info);
                }
                self.pos += 1;
                let mut code_content = String::new();
                while self.pos < self.lines.len() {
                    let (fl2, fc2) = fence_level(&self.lines[self.pos]);
                    if fl2 >= fence_len && fc2 == fence_char {
                        self.pos += 1;
                        break;
                    }
                    if !code_content.is_empty() {
                        code_content.push('\n');
                    }
                    code_content.push_str(&self.lines[self.pos]);
                    self.pos += 1;
                }
                let code_style = (*pre.style).clone();
                append_run(&mut pre, &code_content, code_style);
                parent.children.push(pre);
                continue;
            }

            // Indented code block
            if is_indented_code_line(line) && !line.is_empty() && line.chars().any(|c| c != ' ') {
                let first_non_space = line.chars().take_while(|c| *c == ' ').count();
                if first_non_space >= 4 || line.starts_with('\t') {
                    let mut pre = make_box("pre");
                    pre.data
                        .insert("md-code-style".to_string(), "indented".to_string());
                    let mut code_content = String::new();
                    while self.pos < self.lines.len() {
                        let l = &self.lines[self.pos];
                        if l.is_empty() {
                            // Blank — include if more indented code follows
                            if self.pos + 1 < self.lines.len()
                                && is_indented_code_line(&self.lines[self.pos + 1])
                                && !self.lines[self.pos + 1].is_empty()
                                && self.lines[self.pos + 1].chars().any(|c| c != ' ')
                            {
                                if !code_content.is_empty() {
                                    code_content.push('\n');
                                }
                                self.pos += 1;
                                continue;
                            }
                            break;
                        }
                        if !is_indented_code_line(l) || l.chars().all(|c| c == ' ') {
                            break;
                        }
                        let ns = l.chars().take_while(|c| *c == ' ').count();
                        if ns < 4 && !l.starts_with('\t') {
                            break;
                        }
                        if !code_content.is_empty() {
                            code_content.push('\n');
                        }
                        code_content.push_str(strip_indented_code(l));
                        self.pos += 1;
                    }
                    let code_style = (*pre.style).clone();
                    append_run(&mut pre, &code_content, code_style);
                    parent.children.push(pre);
                    continue;
                }
            }

            // Blockquote
            if is_blockquote_line(line) {
                let mut bq = make_box("blockquote");
                let mut bq_lines: Vec<String> = Vec::new();
                while self.pos < self.lines.len() && is_blockquote_line(&self.lines[self.pos]) {
                    bq_lines.push(strip_blockquote(&self.lines[self.pos]).to_string());
                    self.pos += 1;
                }
                // Continuation lines (lazy continuation)
                while self.pos < self.lines.len()
                    && !self.lines[self.pos].is_empty()
                    && !is_blockquote_line(&self.lines[self.pos])
                    && atx_heading_level(&self.lines[self.pos]) == 0
                    && !is_thematic_break(&self.lines[self.pos])
                {
                    bq_lines.push(self.lines[self.pos].clone());
                    self.pos += 1;
                }
                let mut bq_parser = BlockParser::new(&bq_lines, self.refs, self.footnote_defs);
                bq_parser.parse_blocks(&mut bq);
                parent.children.push(bq);
                continue;
            }

            // Raw HTML block
            if is_html_block_start(line) {
                let mut html_box = make_box("div");
                html_box
                    .data
                    .insert("md-raw-html".to_string(), "true".to_string());
                let mut html_content = line.clone();
                self.pos += 1;
                while self.pos < self.lines.len() && !self.lines[self.pos].is_empty() {
                    html_content.push('\n');
                    html_content.push_str(&self.lines[self.pos]);
                    self.pos += 1;
                }
                let html_style = (*html_box.style).clone();
                append_run(&mut html_box, &html_content, html_style);
                parent.children.push(html_box);
                continue;
            }

            // Table
            if self.pos + 1 < self.lines.len()
                && line.contains('|')
                && is_table_separator(&self.lines[self.pos + 1])
            {
                self.parse_table(parent);
                continue;
            }

            // List
            let li_info = detect_list_item(line);
            if li_info.valid {
                self.parse_list(parent, li_info);
                continue;
            }

            // Definition list
            if self.pos + 1 < self.lines.len() && is_definition_marker(&self.lines[self.pos + 1]) {
                self.parse_definition_list(parent);
                continue;
            }

            // Setext heading
            if self.pos + 1 < self.lines.len() {
                let st_level = setext_level(&self.lines[self.pos + 1]);
                if st_level > 0 {
                    let tag = format!("h{}", st_level);
                    let mut heading = make_box(&tag);
                    heading
                        .data
                        .insert("md-heading".to_string(), "setext".to_string());
                    let setext_char = if st_level == 1 { "=" } else { "-" };
                    heading
                        .data
                        .insert("md-setext-char".to_string(), setext_char.to_string());
                    let content = line.to_string();
                    let ip = InlineParser::new(self.refs);
                    ip.parse(&mut heading, &content);
                    parent.children.push(heading);
                    self.pos += 2;
                    continue;
                }
            }

            // Paragraph
            self.parse_paragraph(parent);
        }
    }

    fn parse_paragraph(&mut self, parent: &mut WebCore) {
        let mut p = make_box("p");
        let mut content = String::new();

        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if is_blank(line) {
                break;
            }
            if atx_heading_level(line) > 0 {
                break;
            }
            if is_thematic_break(line) {
                break;
            }
            if is_blockquote_line(line) {
                break;
            }
            let (fl, _) = fence_level(line);
            if fl > 0 {
                break;
            }
            let li = detect_list_item(line);
            if li.valid {
                break;
            }
            if is_html_block_start(line) {
                break;
            }
            if is_definition_marker(line) {
                break;
            }

            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(line);
            self.pos += 1;

            // Check for setext heading on next line
            if self.pos < self.lines.len() && setext_level(&self.lines[self.pos]) > 0 {
                break;
            }
        }

        let ip = InlineParser::new(self.refs);
        ip.parse(&mut p, &content);
        parent.children.push(p);
    }

    fn parse_list(&mut self, parent: &mut WebCore, first_item: ListInfo) {
        let list_tag = if first_item.ordered { "ol" } else { "ul" };
        let mut list = make_box(list_tag);
        list.data
            .insert("md-bullet".to_string(), first_item.marker.clone());
        if first_item.ordered {
            std::sync::Arc::make_mut(&mut list.style).list_style_type = ListStyleType::Decimal;
            if first_item.number != 1 {
                list.data
                    .insert("md-start".to_string(), first_item.number.to_string());
            }
        } else {
            std::sync::Arc::make_mut(&mut list.style).list_style_type = ListStyleType::Disc;
        }

        let base_indent = first_item.indent;
        let mut has_task_items = false;

        while self.pos < self.lines.len() {
            let line = &self.lines[self.pos];
            if is_blank(line) {
                if self.pos + 1 < self.lines.len() {
                    let next_li = detect_list_item(&self.lines[self.pos + 1]);
                    if next_li.valid && next_li.indent == base_indent {
                        self.pos += 1;
                        continue;
                    }
                    let next_indent = self.lines[self.pos + 1]
                        .chars()
                        .take_while(|c| *c == ' ')
                        .count();
                    if next_indent > base_indent {
                        self.pos += 1;
                        continue;
                    }
                }
                break;
            }

            let li_info = detect_list_item(line);
            if li_info.valid
                && li_info.indent == base_indent
                && li_info.ordered == first_item.ordered
            {
                let mut item = make_box("li");
                if first_item.ordered {
                    std::sync::Arc::make_mut(&mut item.style).list_style_type =
                        ListStyleType::Decimal;
                    std::sync::Arc::make_mut(&mut item.style).list_index = li_info.number;
                }

                if li_info.task_state >= 0 {
                    has_task_items = true;
                    item.data.insert(
                        "md-task".to_string(),
                        if li_info.task_state == 1 {
                            "checked"
                        } else {
                            "unchecked"
                        }
                        .to_string(),
                    );
                }

                let content_start = li_info.content_start;
                let mut content = if content_start < line.len() {
                    line[content_start..].to_string()
                } else {
                    String::new()
                };

                self.pos += 1;

                // Collect continuation lines
                while self.pos < self.lines.len() {
                    let cline = &self.lines[self.pos];
                    if cline.is_empty() {
                        break;
                    }
                    let next_li = detect_list_item(cline);
                    if next_li.valid && next_li.indent <= base_indent {
                        break;
                    }
                    if next_li.valid && next_li.indent > base_indent {
                        break;
                    }
                    let ind = cline.chars().take_while(|c| *c == ' ').count();
                    if ind <= base_indent {
                        break;
                    }
                    content.push('\n');
                    let skip = content_start.min(cline.len()).min(ind);
                    content.push_str(&cline[skip..]);
                    self.pos += 1;
                }

                // Check for nested list
                if self.pos < self.lines.len() {
                    let nested_li = detect_list_item(&self.lines[self.pos]);
                    if nested_li.valid && nested_li.indent > base_indent {
                        let ip = InlineParser::new(self.refs);
                        ip.parse(&mut item, &content);
                        let nested_info = detect_list_item(&self.lines[self.pos]);
                        self.parse_list(&mut item, nested_info);
                        list.children.push(item);
                        continue;
                    }
                }

                let ip = InlineParser::new(self.refs);
                ip.parse(&mut item, &content);
                list.children.push(item);
            } else {
                break;
            }
        }

        if has_task_items {
            list.data
                .insert("md-task-list".to_string(), "true".to_string());
        }

        parent.children.push(list);
    }

    fn parse_table(&mut self, parent: &mut WebCore) {
        let mut table = make_box("table");
        table
            .data
            .insert("md-table".to_string(), "true".to_string());

        let header_cells = split_table_row(&self.lines[self.pos]);
        let alignments = parse_table_alignments(&self.lines[self.pos + 1]);

        let align_str = alignments
            .iter()
            .map(|a| match a {
                TextAlign::Center => "center",
                TextAlign::Right => "right",
                _ => "left",
            })
            .collect::<Vec<_>>()
            .join(",");
        table.data.insert("md-align".to_string(), align_str);

        let mut thead = make_box("thead");
        let mut header_row = make_box("tr");
        for (i, cell_text) in header_cells.iter().enumerate() {
            let mut th = make_box("th");
            if i < alignments.len() {
                std::sync::Arc::make_mut(&mut th.style).text_align = alignments[i];
            }
            let ip = InlineParser::new(self.refs);
            ip.parse(&mut th, cell_text);
            header_row.children.push(th);
        }
        thead.children.push(header_row);
        table.children.push(thead);

        self.pos += 2;

        let mut tbody = make_box("tbody");
        while self.pos < self.lines.len() && self.lines[self.pos].contains('|') {
            let cells = split_table_row(&self.lines[self.pos]);
            let mut row = make_box("tr");
            for (i, cell_text) in cells.iter().enumerate() {
                let mut td = make_box("td");
                if i < alignments.len() {
                    std::sync::Arc::make_mut(&mut td.style).text_align = alignments[i];
                }
                let ip = InlineParser::new(self.refs);
                ip.parse(&mut td, cell_text);
                row.children.push(td);
            }
            tbody.children.push(row);
            self.pos += 1;
        }
        table.children.push(tbody);
        parent.children.push(table);
    }

    fn parse_definition_list(&mut self, parent: &mut WebCore) {
        let mut dl = make_box("dl");

        loop {
            if self.pos >= self.lines.len() {
                break;
            }
            let line = &self.lines[self.pos];
            if is_blank(line) {
                self.pos += 1;
                // Check if next line continues
                if self.pos < self.lines.len()
                    && !self.lines[self.pos].is_empty()
                    && !is_definition_marker(&self.lines[self.pos])
                {
                    if self.pos + 1 < self.lines.len()
                        && is_definition_marker(&self.lines[self.pos + 1])
                    {
                        continue;
                    }
                    break;
                }
                continue;
            }

            if !is_definition_marker(line) {
                // Term line
                let term = line.clone();
                let mut dt = make_box("dt");
                let ip = InlineParser::new(self.refs);
                ip.parse(&mut dt, &term);
                dl.children.push(dt);
                self.pos += 1;

                // Parse all following definitions
                loop {
                    // Skip blank lines between definitions
                    while self.pos < self.lines.len() && is_blank(&self.lines[self.pos]) {
                        self.pos += 1;
                    }
                    if self.pos >= self.lines.len() || !is_definition_marker(&self.lines[self.pos])
                    {
                        break;
                    }
                    let def_line = &self.lines[self.pos];
                    let colon_pos = def_line.find(':').unwrap_or(0);
                    let def_content = def_line[colon_pos + 1..].trim_start_matches(' ');
                    let def_content = def_content.to_string();
                    let mut dd = make_box("dd");
                    let ip2 = InlineParser::new(self.refs);
                    ip2.parse(&mut dd, &def_content);
                    dl.children.push(dd);
                    self.pos += 1;
                }
            } else {
                break;
            }

            // Check if more terms follow
            if self.pos < self.lines.len()
                && !self.lines[self.pos].is_empty()
                && !is_definition_marker(&self.lines[self.pos])
            {
                if self.pos + 1 < self.lines.len()
                    && is_definition_marker(&self.lines[self.pos + 1])
                {
                    continue;
                }
                break;
            }
            if self.pos >= self.lines.len() {
                break;
            }
            if is_definition_marker(&self.lines[self.pos]) {
                break;
            }
        }

        parent.children.push(dl);
    }
}

// ============================================================
// Public API
// ============================================================

/// Parse Markdown text and produce a Document with a Box tree.
pub fn parse_markdown(markdown: &str) -> Document {
    let mut doc = Document::new();
    // Include the UA stylesheet so the CSS cascade applies proper display, margins,
    // and font sizes to h1-h6, p, ul, ol, li, pre, blockquote, table, etc.
    // Without this, layout() wipes every style set by make_box() with ComputedStyle::default().
    doc.stylesheet = ua_stylesheet();
    doc.root.tag = "body".to_string();
    std::sync::Arc::make_mut(&mut doc.root.style).display = Display::Block;
    doc.root.children.clear();

    let lines = split_lines(markdown);

    // First pass: collect reference link definitions and footnote defs
    let mut refs: HashMap<String, RefLink> = HashMap::new();
    let mut footnote_defs: Vec<FootnoteDef> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        // Footnote definition: [^id]: content — check BEFORE ref links
        if lines[i].len() > 4 && lines[i].starts_with("[^") {
            if let Some(close) = lines[i].find("]:") {
                if close > 2 {
                    let fn_id = lines[i][2..close].to_string();
                    let rest = &lines[i][close + 2..];
                    let mut content = rest.trim_start_matches(' ').to_string();
                    // Collect continuation lines
                    while i + 1 < lines.len()
                        && !lines[i + 1].is_empty()
                        && (lines[i + 1].starts_with(' ') || lines[i + 1].starts_with('\t'))
                    {
                        i += 1;
                        let cont = lines[i].trim_start_matches(|c| c == ' ' || c == '\t');
                        content.push('\n');
                        content.push_str(cont);
                    }
                    footnote_defs.push(FootnoteDef { id: fn_id, content });
                    i += 1;
                    continue;
                }
            }
        }
        // Reference link definition
        if let Some((id, reflink)) = parse_ref_def(&lines[i]) {
            refs.insert(id, reflink);
        }
        i += 1;
    }

    // Second pass: parse blocks
    let mut parser = BlockParser::new(&lines, &refs, &footnote_defs);
    parser.parse_blocks(&mut doc.root);

    // Emit footnote definitions as a section at the end
    if !footnote_defs.is_empty() {
        let mut fn_section = make_box("div");
        fn_section
            .data
            .insert("md-footnotes".to_string(), "true".to_string());

        // Add hr separator
        let hr = make_box("hr");
        fn_section.children.push(hr);

        for fn_def in &footnote_defs {
            let mut p = make_box("p");
            p.data
                .insert("md-footnote-def".to_string(), fn_def.id.clone());
            p.data
                .insert("md-footnote-label".to_string(), fn_def.id.clone());
            let ip = InlineParser::new(&refs);
            ip.parse(&mut p, &fn_def.content);
            fn_section.children.push(p);
        }
        doc.root.children.push(fn_section);
    }

    doc
}
