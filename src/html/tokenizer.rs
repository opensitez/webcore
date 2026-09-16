//! The HTML tokenizer.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Tokenizer ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum Token {
    Text(String),
    OpenTag {
        tag: String,
        attrs: crate::dom::attrs::AttrMap,
        self_closing: bool,
    },
    CloseTag {
        tag: String,
    },
    /// Comment DATA, `<!--` and `-->` already stripped.
    Comment(String),
    Doctype(crate::html::doctype::Doctype),
}

pub(crate) struct CompleteToken {
    pub(crate) token: Token,
    pub(crate) end: usize,
}

/// `html[start..end]`, clamped so a truncated tag cannot build a range that
/// runs backwards.
///
/// `<` and `</` at end-of-input are the cases that bite: the tag "ends" at the
/// end of the string, so `end - 1` lands BEFORE the start of the name and the
/// slice panicked. A browser meets a document cut off mid-tag constantly and
/// must simply stop tokenizing there.
fn tag_slice(html: &str, start: usize, end: usize) -> &str {
    let start = start.min(html.len());
    let end = end.max(start).min(html.len());
    // Both ends must sit on a char boundary or the slice panics on UTF-8.
    let mut s = start;
    while s < html.len() && !html.is_char_boundary(s) {
        s += 1;
    }
    let mut e = end.max(s);
    while e < html.len() && !html.is_char_boundary(e) {
        e += 1;
    }
    &html[s..e]
}

pub(crate) fn tokenize(html: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if html[i..].starts_with("<!--") {
                // Carry the DATA. It used to be dropped at the token, which is
                // why no comment could ever reach the tree however the parser
                // was fixed downstream.
                // Search AFTER the `<!--`. Searching the whole slice found the
                // `-->` INSIDE the opener for `<!-->` — the `-` `-` `>` at
                // offset 2 — and built a byte range that ran backwards, which
                // panicked the tokenizer on markup a browser accepts.
                //
                // `<!-->` and `<!--->` are empty comments: §13.2.5.43-45 end
                // the comment on a `>` met in the comment-start states rather
                // than treating it as data.
                let rest = &html[i + 4..];
                let (data, end) = if let Some(r) = rest.strip_prefix('>') {
                    let _ = r;
                    (String::new(), i + 5)
                } else if rest.starts_with("->") {
                    (String::new(), i + 6)
                } else {
                    match rest.find("-->") {
                        Some(e) => (rest[..e].to_string(), i + 4 + e + 3),
                        None => (rest.to_string(), html.len()),
                    }
                };
                tokens.push(Token::Comment(data));
                i = end;
                continue;
            }
            if i + 9 <= bytes.len() && bytes[i..i + 9].eq_ignore_ascii_case(b"<!doctype") {
                let found = html[i..].find('>').map(|e| i + e + 1);
                let end = found.unwrap_or(html.len());
                // The token used to be a unit variant — the name and the two
                // identifiers were read and dropped, which is why `doctype`
                // and `compatMode` had nothing to answer from.
                let inner_end = if found.is_some() { end - 1 } else { end };
                let inner = crate::html::doctype::parse_doctype(&html[i + 9..inner_end]);
                tokens.push(Token::Doctype(inner));
                i = end;
                continue;
            }
            // BOGUS COMMENT (§13.2.5.41). `<!` that is not a comment or a
            // doctype, and `<?`, are comments — not tags. They used to fall
            // through to the generic open-tag path, so `<div><!bogus><p>x</p>`
            // built an ELEMENT named `!bogus` that then swallowed the `<p>` as
            // its child. Everything after a stray `<!` was reparented.
            if i + 1 < bytes.len() && (bytes[i + 1] == b'!' || bytes[i + 1] == b'?') {
                let found = html[i..].find('>').map(|e| i + e + 1);
                let end = found.unwrap_or(html.len());
                // `<!` drops both bytes, `<?` keeps the `?` as data, per spec.
                let data_start = if bytes[i + 1] == b'!' { i + 2 } else { i + 1 };
                let data_end = if found.is_some() { end - 1 } else { end };
                tokens.push(Token::Comment(html[data_start..data_end].to_string()));
                i = end;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
                let inner = tag_slice(html, i + 2, end.saturating_sub(1));
                let tag = inner.trim().to_ascii_lowercase();
                // `</br>` is a `<br>` (HTML §13.2.6.4.7 spells this out: the end
                // tag is a parse error and is HANDLED as the start tag). Pages
                // in the wild write it, and dropping it lost the line break.
                if tag == "br" {
                    tokens.push(Token::OpenTag {
                        tag,
                        attrs: crate::dom::attrs::AttrMap::new(),
                        self_closing: true,
                    });
                } else {
                    tokens.push(Token::CloseTag { tag });
                }
                i = end;
                continue;
            }
            let end = find_tag_end(html, i);
            let tag_src = tag_slice(html, i + 1, end.saturating_sub(1));
            let had_slash = tag_src.trim_end().ends_with('/');
            let tag_src = tag_src.trim_end_matches('/').trim();
            let (tag, attrs) = parse_tag_attrs(tag_src);
            // HTML §13.2.6.4.7 in full: "A start tag whose tag name is
            // 'image' — change the token's tag name to 'img' and reprocess."
            // It is not an alias, it is a rename, and pages rely on it.
            let tag = if tag == "image" {
                "img".to_string()
            } else {
                tag
            };
            let is_void = is_void_element(&tag);
            // A trailing `/` on a non-void HTML element is IGNORED — `<div/>x`
            // opens a div and `x` goes INSIDE it. Only foreign content (SVG,
            // MathML) honours self-closing syntax, and only a void element is
            // self-closing on its own. Treating the slash as authoritative made
            // `<div/>` an empty element and put the rest of the document beside
            // it instead of within.
            let self_closing = is_void || (had_slash && is_foreign_content_tag(&tag));
            tokens.push(Token::OpenTag {
                tag: tag.clone(),
                attrs,
                self_closing,
            });
            i = end;
            // Raw text / foreign content elements: content must not be parsed as HTML.
            // <svg> is foreign content — collect everything until </svg> as raw text
            // so inner SVG elements (path, circle, etc.) don't interfere with HTML parsing.
            // RAWTEXT and RCDATA elements: the content is TEXT. `<title>a<b>c`
            // has no `<b>` element in it, and `<textarea><p>x</p>` holds the
            // literal markup as its value — parsing them as HTML built elements
            // no browser has and lost the characters that made up the tags.
            if matches!(
                tag.as_str(),
                "style" | "script" | "noscript" | "svg" | "title" | "textarea"
            ) && !(self_closing || is_void)
            {
                let close_pat = format!("</{}", tag);
                let raw_end = crate::css::find_case_insensitive(&html[i..], &close_pat)
                    .map(|e| i + e)
                    .unwrap_or(html.len());
                let raw_text = &html[i..raw_end];
                if !raw_text.is_empty() {
                    tokens.push(Token::Text(raw_text.to_string()));
                }
                // Consume the close tag too
                i = raw_end;
                if i < html.len() {
                    let close_end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
                    let close_inner = &html[i + 2..close_end.saturating_sub(1)];
                    let close_tag = close_inner.trim().to_ascii_lowercase();
                    tokens.push(Token::CloseTag { tag: close_tag });
                    i = close_end;
                }
            }
        } else {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            let text = &html[start..i];
            if !text.is_empty() {
                tokens.push(Token::Text(decode_entities(text)));
            }
        }
    }
    tokens
}

pub(crate) fn next_complete_token(html: &str) -> Option<CompleteToken> {
    if html.is_empty() {
        return None;
    }
    let bytes = html.as_bytes();
    if bytes[0] != b'<' {
        let end = html.find('<').unwrap_or(html.len());
        if end == 0 {
            return None;
        }
        return Some(CompleteToken {
            token: Token::Text(decode_entities(&html[..end])),
            end,
        });
    }

    if html.starts_with("<!--") {
        return complete_comment_token(html);
    }
    if html.len() >= 9 && bytes[..9].eq_ignore_ascii_case(b"<!doctype") {
        let end = html.find('>')? + 1;
        let inner = crate::html::doctype::parse_doctype(&html[9..end - 1]);
        return Some(CompleteToken {
            token: Token::Doctype(inner),
            end,
        });
    }
    if html.len() > 1 && (bytes[1] == b'!' || bytes[1] == b'?') {
        let end = html.find('>')? + 1;
        let data_start = if bytes[1] == b'!' { 2 } else { 1 };
        return Some(CompleteToken {
            token: Token::Comment(html[data_start..end - 1].to_string()),
            end,
        });
    }
    if html.len() > 1 && bytes[1] == b'/' {
        let end = html.find('>')? + 1;
        let inner = tag_slice(html, 2, end.saturating_sub(1));
        let tag = inner.trim().to_ascii_lowercase();
        if tag == "br" {
            return Some(CompleteToken {
                token: Token::OpenTag {
                    tag,
                    attrs: crate::dom::attrs::AttrMap::new(),
                    self_closing: true,
                },
                end,
            });
        }
        return Some(CompleteToken {
            token: Token::CloseTag { tag },
            end,
        });
    }

    let end = find_tag_end_streaming(html, 0)?;
    let tag_src = tag_slice(html, 1, end.saturating_sub(1));
    let had_slash = tag_src.trim_end().ends_with('/');
    let tag_src = tag_src.trim_end_matches('/').trim();
    let (tag, attrs) = parse_tag_attrs(tag_src);
    let tag = if tag == "image" {
        "img".to_string()
    } else {
        tag
    };
    let is_void = is_void_element(&tag);
    let self_closing = is_void || (had_slash && is_foreign_content_tag(&tag));
    Some(CompleteToken {
        token: Token::OpenTag {
            tag,
            attrs,
            self_closing,
        },
        end,
    })
}

fn complete_comment_token(html: &str) -> Option<CompleteToken> {
    let rest = &html[4..];
    if rest.starts_with('>') {
        Some(CompleteToken {
            token: Token::Comment(String::new()),
            end: 5,
        })
    } else if rest.starts_with("->") {
        Some(CompleteToken {
            token: Token::Comment(String::new()),
            end: 6,
        })
    } else {
        rest.find("-->").map(|e| CompleteToken {
            token: Token::Comment(rest[..e].to_string()),
            end: 4 + e + 3,
        })
    }
}

fn find_tag_end_streaming(html: &str, start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = start + 1;
    let mut in_q: Option<u8> = None;
    let mut prev_meaningful: u8 = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_q {
            if b == q {
                in_q = None;
            }
        } else if (b == b'"' || b == b'\'') && prev_meaningful == b'=' {
            in_q = Some(b);
        } else if b == b'>' {
            return Some(i + 1);
        }
        if !b.is_ascii_whitespace() {
            prev_meaningful = b;
        }
        i += 1;
    }
    None
}

fn find_tag_end(html: &str, start: usize) -> usize {
    let bytes = html.as_bytes();
    let mut i = start + 1;
    let mut in_q: Option<u8> = None;
    let mut prev_meaningful: u8 = 0; // last non-whitespace byte
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_q {
            if b == q {
                in_q = None;
            }
        } else if (b == b'"' || b == b'\'') && prev_meaningful == b'=' {
            // Only enter quoted mode for attribute values (after '=').
            // Stray quotes (e.g. <div " class="...">) must not toggle
            // quote tracking, which would hide the closing '>'.
            in_q = Some(b);
        } else if b == b'>' {
            return i + 1;
        }
        if !b.is_ascii_whitespace() {
            prev_meaningful = b;
        }
        i += 1;
    }
    html.len()
}

fn parse_tag_attrs(s: &str) -> (String, crate::dom::attrs::AttrMap) {
    let mut iter = s.splitn(2, |c: char| c.is_whitespace());
    let tag = iter.next().unwrap_or("").to_ascii_lowercase();
    let rest = iter.next().unwrap_or("").trim();
    let attrs = parse_attrs(rest);
    (tag, attrs)
}

fn parse_attrs(s: &str) -> crate::dom::attrs::AttrMap {
    let mut map = crate::dom::attrs::AttrMap::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len()
            && bytes[i] != b'='
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = s[name_start..i].to_ascii_lowercase();
        if name.is_empty() {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            // FIRST occurrence wins. The tokenizer's attribute-name state drops
            // a duplicate rather than overwriting, so `<div CLASS=x class=y>`
            // is `class="x"`. `insert` gave the last one, which is the opposite.
            map.entry_or_default(name);
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let q = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let v = decode_entities_attr(&s[start..i]);
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                i += 1;
            }
            decode_entities_attr(&s[start..i])
        };
        map.or_insert(name, value);
    }
    map
}

fn is_void_element(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input"
        | "link" | "meta" | "param" | "source" | "track" | "wbr"
        // SVG void elements — never have child content
        | "path" | "circle" | "rect" | "line" | "polygon" | "polyline"
        | "ellipse" | "use" | "image" | "stop"
    )
}

/// Elements whose content should be completely suppressed (no box, no text)
/// HTML implicit closing rules (HTML spec §12.2.6.4).
/// Returns true if seeing `new_tag` as an open tag should auto-close `current`.
/// The formatting elements of HTML §13.2.4.3.
///
/// What sets them apart is that closing an ANCESTOR does not end them: when
/// `</b>` in `<b>1<i>2</b>3` implicitly closes the `<i>`, the `<i>` is
/// reconstructed afterwards so `3` is still italic. Nothing else in the parser
/// behaves that way — `<section><span>x</section>y` leaves `y` unwrapped,
/// because `span` is not on this list.
pub(crate) fn is_formatting_element(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "b"
            | "big"
            | "code"
            | "em"
            | "font"
            | "i"
            | "nobr"
            | "s"
            | "small"
            | "strike"
            | "strong"
            | "tt"
            | "u"
    )
}

/// HTML §13.2.4.2's "special" category — the elements that break out of a
/// formatting element rather than nest inside it. Used to find the adoption
/// agency's furthest block.
pub(crate) fn is_special_element(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "applet"
            | "area"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "bgsound"
            | "blockquote"
            | "body"
            | "br"
            | "button"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "embed"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "iframe"
            | "img"
            | "input"
            | "keygen"
            | "li"
            | "link"
            | "listing"
            | "main"
            | "marquee"
            | "menu"
            | "meta"
            | "nav"
            | "noembed"
            | "noframes"
            | "noscript"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "plaintext"
            | "pre"
            | "script"
            | "search"
            | "section"
            | "select"
            | "source"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "wbr"
            | "xmp"
    )
}

/// Elements whose subtree is FOREIGN content, where XML self-closing syntax
/// (`<circle/>`) is real. HTML elements ignore a trailing slash.
/// Start tags that "in select" handles as `</select>` (HTML §13.2.6.4.16).
/// Everything else stays inside the select.
pub(crate) fn closes_select(tag: &str) -> bool {
    matches!(tag, "input" | "keygen" | "textarea" | "select")
}

fn is_foreign_content_tag(tag: &str) -> bool {
    matches!(tag, "svg" | "math")
}

pub(crate) fn should_auto_close(current: &str, new_tag: &str) -> bool {
    match current {
        // <p> closes when a block-level element opens
        "p" => matches!(
            new_tag,
            "address"
                | "article"
                | "aside"
                | "blockquote"
                | "center"
                | "details"
                | "dialog"
                | "dir"
                | "div"
                | "dl"
                | "fieldset"
                | "figcaption"
                | "figure"
                | "footer"
                | "form"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "header"
                | "hgroup"
                | "hr"
                | "li"
                | "listing"
                | "main"
                | "menu"
                | "nav"
                | "ol"
                | "p"
                | "plaintext"
                | "pre"
                | "search"
                | "section"
                | "summary"
                | "table"
                | "ul"
                | "xmp"
        ),
        // A heading closes on another heading (HTML §13.2.6.4.7: an h1-h6 start
        // tag while one is open is a parse error that pops it).
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            matches!(new_tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
        }
        // <li> closes on another <li>
        "li" => new_tag == "li",
        // <dt> closes on <dt> or <dd>
        "dt" => matches!(new_tag, "dt" | "dd"),
        // <dd> closes on <dt> or <dd>
        "dd" => matches!(new_tag, "dt" | "dd"),
        // <td> closes on <td>, <th>, or end-of-row tags
        "td" => matches!(new_tag, "td" | "th"),
        // <th> closes on <td>, <th>
        "th" => matches!(new_tag, "td" | "th"),
        // <tr> closes on <tr>
        "tr" => new_tag == "tr",
        // <thead>/<tbody>/<tfoot> close on each other
        "thead" | "tbody" | "tfoot" => matches!(new_tag, "thead" | "tbody" | "tfoot"),
        // <option> closes on <option> or <optgroup>
        "option" => matches!(new_tag, "option" | "optgroup"),
        // <optgroup> closes on <optgroup>
        "optgroup" => new_tag == "optgroup",
        // <rt>/<rp> close on <rt>/<rp>
        "rt" | "rp" => matches!(new_tag, "rt" | "rp"),
        // <colgroup> closes on non-col content
        "colgroup" => new_tag != "col",
        // <head> closes when <body> opens
        "head" => new_tag == "body",
        _ => false,
    }
}

pub(crate) fn is_non_visual(tag: &str) -> bool {
    // script/noscript are handled separately (content passed to host hook).
    matches!(tag, "head" | "meta" | "link")
}
