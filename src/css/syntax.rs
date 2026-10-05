//! Borrowed CSS lexical tokens and token-preserving comment normalization.
//! No token owns source text; callers can consume completed streamed fragments.

#[derive(Default)]
pub(crate) struct CssInputFilter {
    after_cr: bool,
}

impl CssInputFilter {
    /// Append filtered code points immediately; a trailing CR never delays input.
    pub(crate) fn append(&mut self, mut source: &str, output: &mut String) {
        if self.after_cr && !source.is_empty() {
            source = source.strip_prefix('\n').unwrap_or(source);
            self.after_cr = false;
        }
        let Some(first) = source.bytes().position(|b| matches!(b, b'\r' | 0x0c | 0)) else {
            output.push_str(source);
            return;
        };
        output.push_str(&source[..first]);
        source = &source[first..];
        let mut start = 0;
        let mut chars = source.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            if !matches!(ch, '\r' | '\x0c' | '\0') {
                continue;
            }
            output.push_str(&source[start..index]);
            output.push(if ch == '\0' { '\u{fffd}' } else { '\n' });
            start = index + ch.len_utf8();
            if ch == '\r' {
                if chars.peek().is_some_and(|(_, ch)| *ch == '\n') {
                    start = chars.next().unwrap().0 + 1;
                } else if chars.peek().is_none() {
                    self.after_cr = true;
                }
            }
        }
        output.push_str(&source[start..]);
    }
}

pub(super) fn preprocess_input(source: &str) -> std::borrow::Cow<'_, str> {
    let Some(first) = source.bytes().position(|b| matches!(b, b'\r' | 0x0c | 0)) else {
        return std::borrow::Cow::Borrowed(source);
    };
    let mut output = String::with_capacity(source.len());
    output.push_str(&source[..first]);
    CssInputFilter::default().append(&source[first..], &mut output);
    std::borrow::Cow::Owned(output)
}

pub(super) fn normalize_input(source: &str) -> std::borrow::Cow<'_, str> {
    if source.contains("/*") {
        std::borrow::Cow::Owned(normalize_comments(source))
    } else {
        preprocess_input(source)
    }
}

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

#[derive(Clone, Copy, Default)]
enum BoundaryToken {
    #[default]
    None,
    Comment,
    String(u8),
    Name {
        start: usize,
        function: bool,
    },
    Number {
        fraction: bool,
        exponent: bool,
    },
    UrlStart,
    Url,
}

#[derive(Clone, Copy)]
enum BoundaryEscape {
    Start,
    Hex(u8),
}

/// Incremental component-token boundaries over an append-only source buffer.
/// Opaque tokens retain their cursor rather than rescanning earlier chunks.
#[derive(Default)]
pub(crate) struct RuleBoundaryScanner {
    pub(crate) offset: usize,
    token: BoundaryToken,
    escape: Option<BoundaryEscape>,
    skip_lf: bool,
    blocks: Vec<u8>,
}

impl RuleBoundaryScanner {
    pub(crate) fn discard_prefix(&mut self, count: usize) {
        self.offset -= count;
        if let BoundaryToken::Name { start, .. } = &mut self.token {
            *start -= count;
        }
    }

    #[cfg(test)]
    pub(crate) fn is_top_level(&self) -> bool {
        self.blocks.is_empty()
    }

    pub(crate) fn scan(&mut self, source: &str, eof: bool, mut emit: impl FnMut(usize)) {
        let bytes = source.as_bytes();
        let mut i = self.offset;
        while i < bytes.len() {
            let b = bytes[i];
            if self.skip_lf {
                self.skip_lf = false;
                if b == b'\n' {
                    i += 1;
                    continue;
                }
            }
            if let Some(escape) = self.escape {
                match escape {
                    BoundaryEscape::Start if b.is_ascii_hexdigit() => {
                        self.escape = Some(BoundaryEscape::Hex(1));
                        i += 1;
                    }
                    BoundaryEscape::Hex(digits) if digits < 6 && b.is_ascii_hexdigit() => {
                        self.escape = Some(BoundaryEscape::Hex(digits + 1));
                        i += 1;
                    }
                    BoundaryEscape::Hex(_) => {
                        self.escape = None;
                        if whitespace(b) {
                            self.skip_lf = b == b'\r';
                            i += 1;
                        }
                    }
                    BoundaryEscape::Start => {
                        self.escape = None;
                        self.skip_lf = b == b'\r';
                        i += source[i..].chars().next().unwrap().len_utf8();
                    }
                }
                continue;
            }
            match self.token {
                BoundaryToken::Comment => {
                    if b == b'*' {
                        if i + 1 == bytes.len() && !eof {
                            break;
                        }
                        if bytes.get(i + 1) == Some(&b'/') {
                            self.token = BoundaryToken::None;
                            i += 2;
                            continue;
                        }
                    }
                    i += 1;
                }
                BoundaryToken::String(quote) => {
                    if b == b'\\' {
                        self.escape = Some(BoundaryEscape::Start);
                    } else if b == quote || matches!(b, b'\n' | b'\r' | 0x0c) {
                        self.token = BoundaryToken::None;
                    }
                    i += 1;
                }
                BoundaryToken::Name { start, function } => {
                    if name_char(b) {
                        i += 1;
                    } else if b == b'\\' {
                        if i + 1 == bytes.len() && !eof {
                            break;
                        }
                        if escape(bytes, i) {
                            self.escape = Some(BoundaryEscape::Start);
                            i += 1;
                        } else {
                            self.token = BoundaryToken::None;
                        }
                    } else {
                        self.token = BoundaryToken::None;
                        if b == b'(' && function {
                            if is_url_name(&source[start..i]) {
                                self.token = BoundaryToken::UrlStart;
                            } else {
                                self.blocks.push(b')');
                            }
                            i += 1;
                        }
                    }
                }
                BoundaryToken::Number { fraction, exponent } => {
                    if b.is_ascii_digit() {
                        i += 1;
                        continue;
                    }
                    if !fraction && !exponent && b == b'.' {
                        if i + 1 == bytes.len() && !eof {
                            break;
                        }
                        if bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                            self.token = BoundaryToken::Number {
                                fraction: true,
                                exponent,
                            };
                            i += 1;
                            continue;
                        }
                    }
                    if !exponent && matches!(b, b'e' | b'E') {
                        let signed = matches!(bytes.get(i + 1), Some(b'+' | b'-'));
                        let digits = i + 1 + usize::from(signed);
                        if digits >= bytes.len() && !eof {
                            break;
                        }
                        if bytes.get(digits).is_some_and(u8::is_ascii_digit) {
                            self.token = BoundaryToken::Number {
                                fraction,
                                exponent: true,
                            };
                            i = digits;
                            continue;
                        }
                    }
                    if matches!(b, b'-' | b'\\') && bytes.len() - i < 3 && !eof {
                        break;
                    }
                    if starts_name(bytes, i) {
                        self.token = BoundaryToken::Name {
                            start: i,
                            function: false,
                        };
                    } else {
                        self.token = BoundaryToken::None;
                        if b == b'%' {
                            i += 1;
                        }
                    }
                }
                BoundaryToken::UrlStart => {
                    if whitespace(b) {
                        i += 1;
                    } else if matches!(b, b'\'' | b'"') {
                        self.blocks.push(b')');
                        self.token = BoundaryToken::String(b);
                        i += 1;
                    } else {
                        self.token = BoundaryToken::Url;
                    }
                }
                BoundaryToken::Url => {
                    // Even bad URLs consume their remnants through an unescaped ')'.
                    if b == b')' {
                        self.token = BoundaryToken::None;
                    } else if b == b'\\' {
                        if i + 1 == bytes.len() && !eof {
                            break;
                        }
                        if escape(bytes, i) {
                            self.escape = Some(BoundaryEscape::Start);
                        }
                    }
                    i += 1;
                }
                BoundaryToken::None => {
                    let rest = &source[i..];
                    if rest.starts_with("<!--") {
                        i += 4;
                        continue;
                    }
                    if rest.starts_with("-->") {
                        i += 3;
                        continue;
                    }
                    if !eof && rest.len() < 4 && "<!--".starts_with(rest) {
                        break;
                    }
                    // Identifier/number prefixes need at most three code points of lookahead.
                    if !eof
                        && bytes.len() - i < 3
                        && matches!(b, b'+' | b'-' | b'.' | b'\\' | b'@' | b'#')
                    {
                        break;
                    }
                    if b == b'/' {
                        if i + 1 == bytes.len() && !eof {
                            break;
                        }
                        if bytes.get(i + 1) == Some(&b'*') {
                            self.token = BoundaryToken::Comment;
                            i += 2;
                            continue;
                        }
                    }
                    if matches!(b, b'\'' | b'"') {
                        self.token = BoundaryToken::String(b);
                        i += 1;
                    } else if starts_number(bytes, i) {
                        let sign = usize::from(matches!(b, b'+' | b'-'));
                        let fraction = bytes.get(i + sign) == Some(&b'.');
                        i += sign + usize::from(fraction);
                        self.token = BoundaryToken::Number {
                            fraction,
                            exponent: false,
                        };
                    } else if starts_name(bytes, i) {
                        self.token = BoundaryToken::Name {
                            start: i,
                            function: true,
                        };
                    } else if (b == b'@' && starts_name(bytes, i + 1))
                        || (b == b'#'
                            && (bytes.get(i + 1).copied().is_some_and(name_char)
                                || escape(bytes, i + 1)))
                    {
                        i += 1;
                        self.token = BoundaryToken::Name {
                            start: i,
                            function: false,
                        };
                    } else {
                        match b {
                            b'(' => self.blocks.push(b')'),
                            b'[' => self.blocks.push(b']'),
                            b'{' => self.blocks.push(b'}'),
                            b')' | b']' | b'}' => {
                                if self.blocks.last() == Some(&b) {
                                    self.blocks.pop();
                                }
                                if b == b'}' && self.blocks.is_empty() {
                                    emit(i + 1);
                                }
                            }
                            b';' if self.blocks.is_empty() => emit(i + 1),
                            _ => {}
                        }
                        i += 1;
                    }
                }
            }
        }
        self.offset = i;
    }
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

pub(super) fn at_keyword(source: &str) -> Option<(std::borrow::Cow<'_, str>, usize)> {
    let token = Tokens::new(source).next()?;
    (token.kind == Kind::AtKeyword)
        .then(|| decode_name(&token.text[1..]).map(|name| (name, token.text.len())))?
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

/// URL tokens are opaque; quoted URLs instead have a function and string body.
pub(super) fn url_body(source: &str) -> Option<(&str, usize, bool)> {
    let name_end = consume_name(source, 0);
    if source.as_bytes().get(name_end) != Some(&b'(') || !is_url_name(&source[..name_end]) {
        return None;
    }
    let mut start = name_end + 1;
    while source
        .as_bytes()
        .get(start)
        .copied()
        .is_some_and(whitespace)
    {
        start += 1;
    }
    if matches!(source.as_bytes().get(start), Some(b'"' | b'\'')) {
        let (body, end) = function_body(source, name_end + 1)?;
        return Some((body, end, true));
    }
    let (kind, end, body_end) = Tokens::new(source).url(start);
    (kind == Kind::Url).then_some((&source[start..body_end], end, false))
}

pub(super) fn find_url_token(source: &str) -> Option<(usize, usize)> {
    Tokens::new(source).find_map(|token| {
        (token.kind == Kind::Url
            || (token.kind == Kind::Function && is_url_name(&token.text[..token.text.len() - 1])))
        .then_some((token.start, token.start + token.text.len()))
    })
}

/// Find a complete top-level function without inspecting opaque URLs/strings
/// or confusing nested component blocks with the surrounding value.
pub(super) fn find_top_level_function(
    source: &str,
    mut matches: impl FnMut(&str) -> bool,
) -> Option<(usize, usize)> {
    let mut tokens = Tokens::new(source);
    let mut blocks = Vec::new();
    while let Some(token) = tokens.next() {
        match token.kind {
            Kind::Function => {
                let (_, end) = function_body(source, token.start + token.text.len())?;
                let name = decode_name(&token.text[..token.text.len() - 1])?;
                if blocks.is_empty() && matches(&name) {
                    return Some((token.start, end));
                }
                tokens.offset = end;
            }
            Kind::Url if blocks.is_empty() => {
                if matches("url") {
                    return Some((token.start, token.start + token.text.len()));
                }
            }
            Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(byte) if blocks.pop() != Some(byte) => return None,
            Kind::BadString | Kind::BadUrl => return None,
            _ => {}
        }
    }
    None
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

    fn url(&self, mut end: usize) -> (Kind, usize, usize) {
        let bytes = self.source.as_bytes();
        let mut bad = false;
        let mut body_end = None;
        while let Some(&byte) = bytes.get(end) {
            match byte {
                b')' => {
                    return (
                        if bad { Kind::BadUrl } else { Kind::Url },
                        end + 1,
                        body_end.unwrap_or(end),
                    );
                }
                b'\\' if escape(bytes, end) => end = consume_escape(self.source, end),
                b if whitespace(b) => {
                    body_end.get_or_insert(end);
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
        (
            if bad { Kind::BadUrl } else { Kind::Url },
            end,
            body_end.unwrap_or(end),
        )
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
                        let (kind, end, _) = self.url(content);
                        (kind, end)
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
    top_level_punctuation(source).filter_map(move |(at, byte)| (byte == delimiter).then_some(at))
}

pub(super) fn rule_prelude_end(source: &str) -> Option<(usize, u8)> {
    top_level_punctuation(source).find(|(_, byte)| matches!(byte, b';' | b'{'))
}

fn top_level_punctuation(source: &str) -> impl Iterator<Item = (usize, u8)> + '_ {
    let mut blocks = Vec::new();
    Tokens::new(source).filter_map(move |token| {
        let punctuation = if blocks.is_empty() {
            match token.kind {
                Kind::Delim(byte) | Kind::Open(byte) | Kind::Close(byte) => {
                    Some((token.start, byte))
                }
                _ => None,
            }
        } else {
            None
        };
        match token.kind {
            Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(byte) if blocks.last() == Some(&byte) => {
                blocks.pop();
            }
            _ => {}
        }
        punctuation
    })
}

/// Validate the declaration-value token grammar and locate its optional priority.
/// Open blocks and strings are closed implicitly at EOF by CSS tokenization.
pub(super) fn declaration_priority(source: &str) -> Option<Option<usize>> {
    let mut blocks = Vec::new();
    let mut priority = None;
    for token in Tokens::new(source) {
        match token.kind {
            Kind::BadString | Kind::BadUrl => return None,
            Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
            Kind::Open(b'[') => blocks.push(b']'),
            Kind::Open(b'{') => blocks.push(b'}'),
            Kind::Close(byte) if blocks.pop() != Some(byte) => return None,
            Kind::Delim(b';') if blocks.is_empty() => return None,
            Kind::Delim(b'!') if blocks.is_empty() => {
                if priority.replace(token.start).is_some() {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(priority)
}

pub(super) fn normalize_comments(source: &str) -> String {
    let filtered = preprocess_input(source);
    let source = filtered.as_ref();
    if !source.contains("/*") {
        return filtered.into_owned();
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
    fn streamed_rule_boundaries_match_component_tokens_at_every_split() {
        let sources = [
            "<!--url(a};{b);-->.a{color:red}",
            r#"@import url(a{;b); .a{background:url(a};b)}.b{color:red}"#,
            r#".a{--x:url(a"};{b);color:green}.b{color:red}"#,
            r#".a{--x:u\72l(a\);{b);color:green}.b{color:red}"#,
            ".a{--x:URL( \n 'a};b');color:green}.b{color:red}",
            ".a{content:'a\\61\n{b';color:green}.b{color:red}",
            ".a{content:'broken\n;color:green}.b{color:red}",
            ".a{--x:fn([{a;b}]);--y:[a};b];color:red}.b{color:blue}",
            "@supports (x:fn(a;{b})) {.a{color:red}}.b{color:blue}",
            r#".a{--x:1url("};{b");--y:#url("};{b");--z:@url("};{b")} .b{color:red}"#,
            r#".a{--x:1.2e+3url("};{b");--y:-.2e-3url("};{b")} .b{color:red}"#,
            r#"/* a }; */ .a\;b{--x:url(x/*}*/y);content:"};"}.b{color:red}"#,
            ".é{--x:u\\000072\r\nl(a};{b);content:'a\\61\r\nb'}.b{color:red}",
        ];
        for source in sources {
            let mut blocks = Vec::new();
            let expected: Vec<_> = Tokens::new(source)
                .filter_map(|token| {
                    match token.kind {
                        Kind::Function | Kind::Open(b'(') => blocks.push(b')'),
                        Kind::Open(b'[') => blocks.push(b']'),
                        Kind::Open(b'{') => blocks.push(b'}'),
                        Kind::Close(byte) => {
                            if blocks.last() == Some(&byte) {
                                blocks.pop();
                            }
                            if byte == b'}' && blocks.is_empty() {
                                return Some(token.start + token.text.len());
                            }
                        }
                        Kind::Delim(b';') if blocks.is_empty() => {
                            return Some(token.start + token.text.len());
                        }
                        _ => {}
                    }
                    None
                })
                .collect();
            for split in source.char_indices().map(|(i, _)| i).chain([source.len()]) {
                let mut scanner = RuleBoundaryScanner::default();
                let mut actual = Vec::new();
                scanner.scan(&source[..split], false, |end| actual.push(end));
                scanner.scan(source, true, |end| actual.push(end));
                assert_eq!(actual, expected, "split={split}, source={source:?}");
            }
            let mut scanner = RuleBoundaryScanner::default();
            let mut actual = Vec::new();
            for end in source.char_indices().map(|(i, ch)| i + ch.len_utf8()) {
                scanner.scan(&source[..end], false, |end| actual.push(end));
            }
            scanner.scan(source, true, |end| actual.push(end));
            assert_eq!(
                actual, expected,
                "single-character chunks, source={source:?}"
            );
        }
    }

    #[test]
    fn streamed_opaque_tokens_and_names_do_not_rescan_previous_chunks() {
        for prefix in [
            ".a{--x:url(",
            ".a{--x:url(a\"",
            ".a{content:'",
            "/*",
            ".a{--x:long",
            ".a{--x:123",
        ] {
            let mut scanner = RuleBoundaryScanner::default();
            let mut source = prefix.to_string();
            scanner.scan(&source, false, |_| panic!("premature boundary"));
            for _ in 0..4096 {
                source.push('a');
                scanner.scan(&source, false, |_| panic!("premature boundary"));
                assert_eq!(scanner.offset, source.len(), "prefix={prefix:?}");
            }
        }
    }

    #[test]
    fn input_preprocessing_is_chunk_independent_and_borrows_ordinary_text() {
        let source = ".a\r\n{--x:'é\0z';\x0c--y:'a\\\rb';}\r";
        let expected = ".a\n{--x:'é\u{fffd}z';\n--y:'a\\\nb';}\n";
        assert_eq!(preprocess_input(source), expected);
        assert!(matches!(
            preprocess_input(".a{color:red}\n"),
            std::borrow::Cow::Borrowed(_)
        ));
        for split in source.char_indices().map(|(i, _)| i).chain([source.len()]) {
            let mut filter = CssInputFilter::default();
            let mut output = String::new();
            filter.append(&source[..split], &mut output);
            filter.append("", &mut output);
            filter.append(&source[split..], &mut output);
            assert_eq!(output, expected, "split={split}");
        }
        assert_eq!(normalize_comments("'a\0b'/*x*/\r\nc"), "'a\u{fffd}b'\nc");
    }

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
