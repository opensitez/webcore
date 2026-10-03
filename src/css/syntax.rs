//! Borrowed CSS lexical tokens and token-preserving comment normalization.
//! No token owns source text; callers can consume completed streamed fragments.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Ident,
    Function,
    AtKeyword,
    Hash,
    Number,
    Percentage,
    Dimension,
    String,
    BadString,
    Url,
    BadUrl,
    Whitespace,
    Comment,
    Cdo,
    Cdc,
    Open(u8),
    Close(u8),
    Delim(u8),
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    kind: Kind,
    start: usize,
    text: &'a str,
}

struct Tokens<'a> {
    source: &'a str,
    offset: usize,
}

fn whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}
fn name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}
fn name_char(byte: u8) -> bool {
    name_start(byte) || byte.is_ascii_digit() || byte == b'-'
}
fn escape(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b'\\')
        && bytes
            .get(at + 1)
            .is_some_and(|b| !matches!(b, b'\n' | b'\r' | 0x0c))
}
fn starts_name(bytes: &[u8], at: usize) -> bool {
    match bytes.get(at).copied() {
        Some(b'-') => {
            bytes
                .get(at + 1)
                .is_some_and(|b| name_start(*b) || *b == b'-')
                || escape(bytes, at + 1)
        }
        Some(b'\\') => escape(bytes, at),
        Some(b) => name_start(b),
        None => false,
    }
}
fn starts_number(bytes: &[u8], at: usize) -> bool {
    let at = if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at + 1
    } else {
        at
    };
    bytes.get(at).is_some_and(u8::is_ascii_digit)
        || (bytes.get(at) == Some(&b'.') && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
}
fn consume_escape(source: &str, at: usize) -> usize {
    let bytes = source.as_bytes();
    let mut end = at + 1;
    if bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
        for _ in 0..6 {
            if !bytes.get(end).is_some_and(u8::is_ascii_hexdigit) {
                break;
            }
            end += 1;
        }
        if bytes.get(end).copied().is_some_and(whitespace) {
            let cr = bytes[end] == b'\r';
            end += 1;
            if cr && bytes.get(end) == Some(&b'\n') {
                end += 1;
            }
        }
    } else if let Some(ch) = source[end..].chars().next() {
        end += ch.len_utf8();
    }
    end
}
fn consume_name(source: &str, mut at: usize) -> usize {
    let bytes = source.as_bytes();
    while let Some(&byte) = bytes.get(at) {
        if name_char(byte) {
            at += 1;
        } else if escape(bytes, at) {
            at = consume_escape(source, at);
        } else {
            break;
        }
    }
    at
}
fn consume_number(bytes: &[u8], mut at: usize) -> usize {
    if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    if bytes.get(at) == Some(&b'.') && bytes.get(at + 1).is_some_and(u8::is_ascii_digit) {
        at += 2;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let mut exponent = at + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        if bytes.get(exponent).is_some_and(u8::is_ascii_digit) {
            at = exponent + 1;
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
        }
    }
    at
}
fn is_url_name(name: &str) -> bool {
    if !name.contains('\\') {
        return name.eq_ignore_ascii_case("url");
    }
    let mut decoded = String::new();
    let mut chars = name.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(ch) = super::parser::read_css_ident_escape(&mut chars) {
                decoded.push(ch);
            }
        } else {
            decoded.push(ch);
        }
    }
    decoded.eq_ignore_ascii_case("url")
}

/// Comments disappear between tokens but do not satisfy grammar requiring whitespace.
pub(super) fn trivia_prefix(source: &str) -> (usize, bool) {
    let mut tokens = Tokens::new(source);
    let mut end = 0;
    let mut spaced = false;
    while source.as_bytes().get(end).copied().is_some_and(whitespace)
        || source[end..].starts_with("/*")
    {
        let Some(token) = tokens.next() else { break };
        match token.kind {
            Kind::Whitespace => spaced = true,
            Kind::Comment => {}
            _ => break,
        }
        end = token.start + token.text.len();
    }
    (end, spaced)
}

/// Read an identifier or function name without merging adjacent tokens.
pub(super) fn name_token(source: &str) -> Option<(std::borrow::Cow<'_, str>, usize, bool)> {
    if !starts_name(source.as_bytes(), 0) {
        return None;
    }
    let token = Tokens::new(source).next()?;
    let function = token.kind == Kind::Function;
    if !function && token.kind != Kind::Ident {
        return None;
    }
    let name = if function {
        &token.text[..token.text.len() - 1]
    } else {
        token.text
    };
    Some((decode_name(name)?, token.text.len(), function))
}

fn decode_name(source: &str) -> Option<std::borrow::Cow<'_, str>> {
    if !source.contains('\\') {
        return Some(std::borrow::Cow::Borrowed(source));
    }
    let mut decoded = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        decoded.push(if ch == '\\' {
            super::parser::read_css_ident_escape(&mut chars)?
        } else {
            ch
        });
    }
    Some(std::borrow::Cow::Owned(decoded))
}

/// Return the borrowed body and end of a function, respecting opaque tokens and blocks.
pub(super) fn function_body(source: &str, body_start: usize) -> Option<(&str, usize)> {
    let body = source.get(body_start..)?;
    let mut blocks = Vec::new();
    for token in Tokens::new(body) {
        match token.kind {
            Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(b')') if blocks.is_empty() => {
                return Some((&body[..token.start], body_start + token.start + 1));
            }
            Kind::Close(byte) => {
                if blocks.pop() != Some(byte) {
                    return None;
                }
            }
            Kind::BadString | Kind::BadUrl => return None,
            _ => {}
        }
    }
    None
}

pub(super) fn find_function(source: &str, name: &str) -> Option<(usize, usize)> {
    Tokens::new(source).find_map(|token| {
        if token.kind != Kind::Function {
            return None;
        }
        let decoded = decode_name(&token.text[..token.text.len() - 1])?;
        decoded
            .eq_ignore_ascii_case(name)
            .then_some((token.start, token.start + token.text.len()))
    })
}

/// Read one complete numeric token, keeping ordinary unit names borrowed.
pub(super) fn number_and_unit(source: &str) -> Option<(f32, std::borrow::Cow<'_, str>)> {
    let (number, unit, end) = numeric_token(source)?;
    (end == source.len()).then_some((number, unit))
}

/// Read a numeric prefix for a calculation without consuming its next operator.
pub(super) fn numeric_token(source: &str) -> Option<(f32, std::borrow::Cow<'_, str>, usize)> {
    let token = Tokens::new(source).next()?;
    if !matches!(
        token.kind,
        Kind::Number | Kind::Percentage | Kind::Dimension
    ) {
        return None;
    }
    let end = consume_number(source.as_bytes(), 0);
    let number = source[..end].parse().ok()?;
    let token_end = token.text.len();
    let unit = &source[end..token_end];
    if !unit.contains('\\') {
        return Some((number, std::borrow::Cow::Borrowed(unit), token_end));
    }
    let decoded = decode_name(unit)?;
    // An escaped percent sign is a dimension unit, not a percentage token.
    if decoded == "%" {
        return None;
    }
    Some((number, decoded, token_end))
}

impl<'a> Tokens<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, offset: 0 }
    }

    fn string(&self, start: usize) -> (Kind, usize) {
        let bytes = self.source.as_bytes();
        let quote = bytes[start];
        let mut end = start + 1;
        while let Some(&byte) = bytes.get(end) {
            match byte {
                b if b == quote => return (Kind::String, end + 1),
                b'\n' | b'\r' | 0x0c => return (Kind::BadString, end),
                b'\\' if escape(bytes, end) => end = consume_escape(self.source, end),
                b'\\' => {
                    end += 1;
                    if bytes.get(end) == Some(&b'\r') {
                        end += 1;
                        if bytes.get(end) == Some(&b'\n') {
                            end += 1;
                        }
                    } else if bytes.get(end).is_some() {
                        end += self.source[end..].chars().next().unwrap().len_utf8();
                    }
                }
                _ => end += 1,
            }
        }
        (Kind::String, end)
    }

    fn url(&self, mut end: usize) -> (Kind, usize) {
        let bytes = self.source.as_bytes();
        let mut bad = false;
        while let Some(&byte) = bytes.get(end) {
            match byte {
                b')' => return (if bad { Kind::BadUrl } else { Kind::Url }, end + 1),
                b'\\' if escape(bytes, end) => end = consume_escape(self.source, end),
                b if whitespace(b) => {
                    while bytes.get(end).copied().is_some_and(whitespace) {
                        end += 1;
                    }
                    if bytes.get(end).is_some_and(|b| *b != b')') {
                        bad = true;
                    }
                }
                b'"' | b'\'' | b'(' | b'\\' | 0..=8 | 11 | 14..=31 | 127 => {
                    bad = true;
                    end += 1;
                }
                _ => end += 1,
            }
        }
        (if bad { Kind::BadUrl } else { Kind::Url }, end)
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Token<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let bytes = self.source.as_bytes();
        let start = self.offset;
        let &byte = bytes.get(start)?;
        let (kind, end) = if byte == b'/' && bytes.get(start + 1) == Some(&b'*') {
            let end = self.source[start + 2..]
                .find("*/")
                .map_or(bytes.len(), |i| start + 4 + i);
            (Kind::Comment, end)
        } else if whitespace(byte) {
            let mut end = start + 1;
            while bytes.get(end).copied().is_some_and(whitespace) {
                end += 1;
            }
            (Kind::Whitespace, end)
        } else if matches!(byte, b'"' | b'\'') {
            self.string(start)
        } else if self.source[start..].starts_with("<!--") {
            (Kind::Cdo, start + 4)
        } else if self.source[start..].starts_with("-->") {
            (Kind::Cdc, start + 3)
        } else if starts_number(bytes, start) {
            let end = consume_number(bytes, start);
            if starts_name(bytes, end) {
                (Kind::Dimension, consume_name(self.source, end))
            } else if bytes.get(end) == Some(&b'%') {
                (Kind::Percentage, end + 1)
            } else {
                (Kind::Number, end)
            }
        } else if starts_name(bytes, start) {
            let end = consume_name(self.source, start);
            if bytes.get(end) == Some(&b'(') {
                if is_url_name(&self.source[start..end]) {
                    let mut content = end + 1;
                    while bytes.get(content).copied().is_some_and(whitespace) {
                        content += 1;
                    }
                    if matches!(bytes.get(content), Some(b'"' | b'\'')) {
                        (Kind::Function, end + 1)
                    } else {
                        self.url(content)
                    }
                } else {
                    (Kind::Function, end + 1)
                }
            } else {
                (Kind::Ident, end)
            }
        } else if byte == b'@' && starts_name(bytes, start + 1) {
            (Kind::AtKeyword, consume_name(self.source, start + 1))
        } else if byte == b'#'
            && (bytes.get(start + 1).copied().is_some_and(name_char) || escape(bytes, start + 1))
        {
            (Kind::Hash, consume_name(self.source, start + 1))
        } else {
            (
                match byte {
                    b'(' | b'[' | b'{' => Kind::Open(byte),
                    b')' | b']' | b'}' => Kind::Close(byte),
                    _ => Kind::Delim(byte),
                },
                start + 1,
            )
        };
        self.offset = end;
        Some(Token {
            kind,
            start,
            text: &self.source[start..end],
        })
    }
}

// CSS Syntax 3 serialization table: inserting whitespace here would change
// selector combinators and calc() operator grammar. Retain an empty comment.
fn separator_required(left: Kind, right: Kind) -> bool {
    let name = matches!(
        right,
        Kind::Ident | Kind::Function | Kind::Url | Kind::BadUrl | Kind::Cdc
    );
    let numeric = matches!(right, Kind::Number | Kind::Percentage | Kind::Dimension);
    match left {
        Kind::Ident => name || numeric || matches!(right, Kind::Delim(b'-') | Kind::Open(b'(')),
        Kind::AtKeyword | Kind::Hash | Kind::Dimension | Kind::Delim(b'#' | b'-') => {
            name || numeric || right == Kind::Delim(b'-')
        }
        Kind::Number => name || numeric || right == Kind::Delim(b'%'),
        Kind::Delim(b'@') => name || right == Kind::Delim(b'-'),
        Kind::Delim(b'.' | b'+') => numeric,
        Kind::Delim(b'/') => right == Kind::Delim(b'*'),
        _ => false,
    }
}

/// Serialize substituted component sequences without fusing their boundary tokens.
#[derive(Default)]
pub(super) struct ComponentWriter {
    output: String,
    last: Option<Kind>,
}

impl ComponentWriter {
    pub(super) fn push(&mut self, source: &str) {
        let mut tokens = Tokens::new(source);
        let Some(first) = tokens.next() else {
            return;
        };
        let html_comment = (self.output.ends_with('<') && source.starts_with("!--"))
            || (self.output.ends_with("<!") && source.starts_with("--"))
            || (self.output.ends_with("--") && source.starts_with('>'));
        if html_comment
            || self
                .last
                .is_some_and(|left| separator_required(left, first.kind))
        {
            self.output.push_str("/**/");
        }
        self.last = Some(tokens.last().unwrap_or(first).kind);
        self.output.push_str(source);
    }

    pub(super) fn finish(self) -> String {
        self.output
    }
}

/// Find punctuation outside component blocks; strings and URL tokens are opaque.
pub(super) fn top_level_delimiters(
    source: &str,
    delimiter: u8,
) -> impl Iterator<Item = usize> + '_ {
    let mut blocks = Vec::new();
    Tokens::new(source).filter_map(move |token| {
        if blocks.is_empty()
            && matches!(token.kind, Kind::Delim(byte) | Kind::Open(byte) | Kind::Close(byte) if byte == delimiter)
        {
            return Some(token.start);
        }
        match token.kind {
            Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(byte) if blocks.last() == Some(&byte) => {
                blocks.pop();
            }
            _ => {}
        }
        None
    })
}

pub(super) fn normalize_comments(source: &str) -> String {
    if !source.contains("/*") {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len());
    let mut previous = None;
    let mut comment = false;
    for token in Tokens::new(source) {
        if token.kind == Kind::Comment {
            comment = true;
            continue;
        }
        let joins_html_comment_token = (out.ends_with('<')
            && source[token.start..].starts_with("!--"))
            || (out.ends_with("<!") && token.text.starts_with("--"))
            || (out.ends_with("--") && token.text.starts_with('>'));
        if comment
            && (joins_html_comment_token
                || previous.is_some_and(|kind| separator_required(kind, token.kind)))
        {
            out.push_str("/**/");
        }
        out.push_str(token.text);
        previous = Some(token.kind);
        comment = false;
    }
    out
}

/// Component values separated by actual whitespace or a top-level comment.
/// Strings and balanced functions/blocks remain intact, borrowing their source.
pub(super) fn split_component_values(source: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut blocks = Vec::new();
    let mut start = None;
    for token in Tokens::new(source) {
        if blocks.is_empty() && matches!(token.kind, Kind::Whitespace | Kind::Comment) {
            if let Some(start) = start.take() {
                values.push(&source[start..token.start]);
            }
            continue;
        }
        start.get_or_insert(token.start);
        match token.kind {
            Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(byte) if blocks.last() == Some(&byte) => {
                blocks.pop();
            }
            _ => {}
        }
    }
    if let Some(start) = start {
        values.push(&source[start..]);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_normalization_preserves_lexical_token_boundaries() {
        for source in [
            "10/**/px",
            "re/**/d",
            "1/**/e3",
            "a/**/(b)",
            "@/**/media",
            "#/**/abc",
            "-/**/x",
            ". /**/5",
            "./**/5",
            "+/**/1",
            "1/**/%",
            "/**/url(x/*literal*/y)",
            "url('/*literal*/')",
            "\\75rl(x/*literal*/y)",
            "a/**/.b",
            "a/*x*//*y*/b",
            "'a\\\"/*literal*/'/*gone*/x",
            "1/**/-->",
        ] {
            let normalized = normalize_comments(source);
            let signature = |text: &str| {
                Tokens::new(text)
                    .filter(|t| t.kind != Kind::Comment)
                    .map(|t| (t.kind, t.text.to_string()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                signature(source),
                signature(&normalized),
                "{source} => {normalized}"
            );
        }
        assert_eq!(normalize_comments(".a/**/.b"), ".a.b");
        assert_eq!(normalize_comments("1/*x*/px"), "1/**/px");
        assert_eq!(
            normalize_comments("url(x/*literal*/y)"),
            "url(x/*literal*/y)"
        );
    }

    #[test]
    fn component_splitting_borrows_strings_urls_and_balanced_blocks() {
        let source = "1px/**/solid 'Open Sans' url(x/*literal*/y) calc(2px + 3px) { a; [b; c] }";
        assert_eq!(
            split_component_values(source),
            vec![
                "1px",
                "solid",
                "'Open Sans'",
                "url(x/*literal*/y)",
                "calc(2px + 3px)",
                "{ a; [b; c] }"
            ]
        );
    }

    #[test]
    fn punctuation_in_url_and_escaped_string_tokens_is_opaque() {
        let source = "url(x};!y) '\\61\n/*literal*/'; next";
        assert_eq!(
            top_level_delimiters(source, b';').collect::<Vec<_>>(),
            vec![source.rfind(';').unwrap()]
        );
        assert_eq!(top_level_delimiters(source, b'}').next(), None);
        assert_eq!(top_level_delimiters(source, b'!').next(), None);
        assert_eq!(normalize_comments(source), source);
    }

    #[test]
    fn serialization_table_and_html_comment_tokens_round_trip() {
        let signature = |text: &str| {
            Tokens::new(text)
                .filter(|t| t.kind != Kind::Comment)
                .map(|t| (t.kind, t.text.to_string()))
                .collect::<Vec<_>>()
        };
        for left in ["x", "@x", "#x", "1px", "#", "-", "1", "@", ".", "+", "/"] {
            for right in [
                "x", "x(", "url(x)", "url(x y)", "-", "1", "1%", "1px", "-->", "(", "*", "%",
            ] {
                let source = format!("{left}/*gone*/{right}");
                let normalized = normalize_comments(&source);
                assert_eq!(
                    signature(&source),
                    signature(&normalized),
                    "{source} => {normalized}"
                );
            }
        }
        for source in ["</**/!--", "</**/!/**/--", "--/**/>", "-/**/->"] {
            assert_eq!(
                signature(source),
                signature(&normalize_comments(source)),
                "{source}"
            );
        }
    }
}
