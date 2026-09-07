//! A WHATWG URL parser — enough of it for `Location` and
//! `HyperlinkElementUtils`.
//!
//! Every rule here is measured (`/tmp/webcore-html/url.html`), and four of
//! them are where a hand-rolled parser goes quietly wrong:
//!
//! * **`href` cannot be reassembled from the components.** `http://x/?` has
//!   `search == ""` and an `href` that KEEPS the `?`. Same for `#`. The
//!   delimiter's presence is separate state from the component's value, so a
//!   serializer that concatenates components silently drops both — and every
//!   other row still passes.
//! * **`origin` has three shapes**: `"http://example.com"` for a special
//!   scheme, `"file://"` for `file:`, and the STRING `"null"` for an opaque
//!   one. Not an empty string, and not an absent value.
//! * **A non-special scheme puts its whole remainder in `pathname`** —
//!   `mailto:a@b.c` has pathname `"a@b.c"` and no host at all. `pathname` is
//!   not "the path" there; it is the opaque path.
//! * **No URL at all gives `protocol == ":"`** while every other component is
//!   `""` — the common case for `<a name=…>` and the row most likely to be
//!   written as `""`.
//!
//! ⛔ **Outside this parser**: IDNA/punycode for non-ASCII hosts, and
//! percent-encoding normalization. Chrome does both. Nothing here should be
//! read as "WHATWG URL parsing, complete".

/// A parsed URL, decomposed the way the IDL exposes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Url {
    /// Without the trailing colon — `protocol()` adds it.
    pub scheme: String,
    pub username: String,
    pub password: String,
    pub hostname: String,
    /// Empty when absent OR when it is the scheme's default.
    pub port: String,
    /// For a special scheme, a path starting with `/`. For an opaque one, the
    /// whole remainder.
    pub path: String,
    /// Without the leading `?`.
    pub query: String,
    /// Without the leading `#`.
    pub fragment: String,
    /// ⛔ Whether the input HAD a `?` / `#`, which is not the same as whether
    /// the component is non-empty. `http://x/?` keeps its `?` in `href` and
    /// answers `""` for `search`.
    pub has_query: bool,
    pub has_fragment: bool,
    /// A scheme with a hierarchical authority: http, https, ws, wss, ftp, file.
    ///
    /// ⛔ Not the same question as [`Url::has_authority`]. `special` decides
    /// the DEFAULT PORT and the `origin` shape; whether an authority was
    /// parsed at all is decided by the `//`, and a non-special scheme can have
    /// one — `a1+-.b://host/p` has a host and an origin of `"null"`.
    pub special: bool,
    /// Whether the URL has an authority (`//host`) to serialize.
    pub has_authority: bool,
}

/// The default port for a special scheme, dropped from `host` and `href`.
fn default_port(scheme: &str) -> Option<&'static str> {
    Some(match scheme {
        "http" | "ws" => "80",
        "https" | "wss" => "443",
        "ftp" => "21",
        _ => return None,
    })
}

fn is_special(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp" | "file")
}

/// Remove `.` and `..` segments (RFC 3986 §5.2.4).
fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    // A path that ended in `.` or `..` keeps its trailing slash.
    let mut joined = out.join("/");
    if (path.ends_with("/.") || path.ends_with("/..")) && !joined.ends_with('/') {
        joined.push('/');
    }
    if !joined.starts_with('/') {
        joined.insert(0, '/');
    }
    joined
}

impl Url {
    /// `url.protocol` — ⛔ WITH the colon, and `":"` for an empty URL.
    pub fn protocol(&self) -> String {
        format!("{}:", self.scheme)
    }

    /// `url.host` — hostname plus the port, when the port is not the default.
    pub fn host(&self) -> String {
        if self.port.is_empty() {
            self.hostname.clone()
        } else {
            format!("{}:{}", self.hostname, self.port)
        }
    }

    /// `url.pathname`.
    pub fn pathname(&self) -> &str {
        &self.path
    }

    /// `url.search` — `""` when empty, EVEN IF the URL had a `?`.
    pub fn search(&self) -> String {
        if self.query.is_empty() {
            String::new()
        } else {
            format!("?{}", self.query)
        }
    }

    /// `url.hash`.
    pub fn hash(&self) -> String {
        if self.fragment.is_empty() {
            String::new()
        } else {
            format!("#{}", self.fragment)
        }
    }

    /// `url.origin` — three shapes, only one of them `scheme://host`.
    ///
    /// ⛔ `file:` needs no branch of its own: it is a SPECIAL scheme with an
    /// empty host, so the general form already produces `"file://"`. A
    /// mutation proved an explicit early return for it was indistinguishable
    /// from its absence.
    pub fn origin(&self) -> String {
        if !self.special {
            return "null".to_string();
        }
        format!("{}://{}", self.scheme, self.host())
    }

    /// `url.href` — the serialization.
    ///
    /// ⛔ Built from the RAW state, not from the accessors: an empty query
    /// with `has_query` set still writes the `?`.
    pub fn href(&self) -> String {
        if self.scheme.is_empty() {
            return String::new();
        }
        let mut out = format!("{}:", self.scheme);
        if self.has_authority {
            out.push_str("//");
            if !self.username.is_empty() || !self.password.is_empty() {
                out.push_str(&self.username);
                if !self.password.is_empty() {
                    out.push(':');
                    out.push_str(&self.password);
                }
                out.push('@');
            }
            out.push_str(&self.host());
        }
        out.push_str(&self.path);
        if self.has_query {
            out.push('?');
            out.push_str(&self.query);
        }
        if self.has_fragment {
            out.push('#');
            out.push_str(&self.fragment);
        }
        out
    }
}

/// Parse `input`, resolving it against `base` when it is relative.
///
/// `None` for input that names no URL at all — which is what an `<a>` with no
/// `href` has, and what makes `protocol` answer `":"` there.
pub fn parse(input: &str, base: Option<&Url>) -> Option<Url> {
    let input = input.trim();

    // An absolute URL carries its own scheme.
    if let Some(scheme_end) = scheme_end(input) {
        let scheme = input[..scheme_end].to_ascii_lowercase();
        let rest = &input[scheme_end + 1..];
        // ⛔ It is the `//` that introduces an authority, NOT the scheme being
        // special. `a1+-.b://host/p` has hostname `host` and pathname `/p`
        // while its origin is still `"null"` (measured). Treating every
        // non-special scheme as opaque put `//host/p` in the pathname.
        let special = is_special(&scheme);
        return match rest.strip_prefix("//") {
            Some(authority) => parse_authority(&scheme, authority, special),
            None if special => parse_authority(&scheme, rest, special),
            None => {
                // The whole remainder is the PATH for an opaque scheme —
                // `mailto:a@b.c` has no host.
                let (path, query, fragment, hq, hf) = split_tail(rest);
                Some(Url {
                    scheme,
                    path,
                    query,
                    fragment,
                    has_query: hq,
                    has_fragment: hf,
                    special: false,
                    ..Default::default()
                })
            }
        };
    }

    let base = base?;

    // Protocol-relative: keep the base's scheme, take a fresh authority.
    if let Some(rest) = input.strip_prefix("//") {
        return parse_authority(&base.scheme, rest, is_special(&base.scheme));
    }

    let mut out = base.clone();
    out.fragment.clear();
    out.has_fragment = false;

    if input.is_empty() {
        // ⛔ The empty string is the base MINUS its fragment, not the base.
        return Some(out);
    }
    if let Some(frag) = input.strip_prefix('#') {
        out.fragment = frag.to_string();
        out.has_fragment = true;
        return Some(out);
    }
    if let Some(q) = input.strip_prefix('?') {
        let (query, fragment, hf) = match q.split_once('#') {
            Some((a, b)) => (a.to_string(), b.to_string(), true),
            None => (q.to_string(), String::new(), false),
        };
        out.query = query;
        out.has_query = true;
        out.fragment = fragment;
        out.has_fragment = hf;
        return Some(out);
    }

    let (path, query, fragment, hq, hf) = split_tail(input);
    out.path = if path.starts_with('/') {
        remove_dot_segments(&path)
    } else {
        // Relative to the base's DIRECTORY.
        let dir = match base.path.rfind('/') {
            Some(i) => &base.path[..=i],
            None => "/",
        };
        remove_dot_segments(&format!("{dir}{path}"))
    };
    out.query = query;
    out.has_query = hq;
    out.fragment = fragment;
    out.has_fragment = hf;
    Some(out)
}

/// The index of the `:` ending a valid scheme, if the input starts with one.
fn scheme_end(input: &str) -> Option<usize> {
    let i = input.find(':')?;
    if i == 0 {
        return None;
    }
    let scheme = &input[..i];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        .then_some(i)
}

/// Parse `host[:port][/path][?q][#f]` for a special scheme.
fn parse_authority(scheme: &str, rest: &str, special: bool) -> Option<Url> {
    let end = rest
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);

    let (credentials, hostport) = match authority.rsplit_once('@') {
        Some((c, h)) => (c, h),
        None => ("", authority),
    };
    let (username, password) = match credentials.split_once(':') {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => (credentials.to_string(), String::new()),
    };
    let (hostname, mut port) = match hostport.rsplit_once(':') {
        // ⛔ A port that is not all digits is a PARSE FAILURE, not a fallback:
        // `http://example.com:80x/` throws `TypeError` in Chrome rather than
        // treating `:80x` as part of the host. An EMPTY port is fine.
        Some((h, p)) => {
            if !p.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            (h.to_string(), p.to_string())
        }
        None => (hostport.to_string(), String::new()),
    };
    // ⛔ The scheme's DEFAULT port is dropped — from `host`, `port` and `href`
    // alike, so `http://x:80/` serializes back without it.
    if default_port(scheme) == Some(port.as_str()) {
        port.clear();
    }

    let (mut path, query, fragment, hq, hf) = split_tail(tail);
    // ⛔ An empty path on a special scheme becomes `/`, and `href` gains it.
    if path.is_empty() {
        path = "/".to_string();
    } else {
        path = remove_dot_segments(&path);
    }

    Some(Url {
        scheme: scheme.to_string(),
        username,
        password,
        hostname: hostname.to_ascii_lowercase(),
        port,
        path,
        query,
        fragment,
        has_query: hq,
        has_fragment: hf,
        special,
        has_authority: true,
    })
}

/// Split `path?query#fragment`, reporting whether each delimiter was present.
fn split_tail(s: &str) -> (String, String, String, bool, bool) {
    let (before_frag, fragment, has_frag) = match s.split_once('#') {
        Some((a, b)) => (a, b.to_string(), true),
        None => (s, String::new(), false),
    };
    let (path, query, has_query) = match before_frag.split_once('?') {
        Some((a, b)) => (a, b.to_string(), true),
        None => (before_frag, String::new(), false),
    };
    (path.to_string(), query, fragment, has_query, has_frag)
}

// ─── `HyperlinkElementUtils` and `Location` (HTML §4.6.3, §7.5) ─────────────

use crate::types::Document;

impl Document {
    /// The URL an `<a>` / `<area>` points at, resolved against the document
    /// base. `None` when it has no `href` — which is what makes every
    /// component answer `""` and `protocol` answer `":"`.
    fn hyperlink_url(&self, id: u32) -> Option<Url> {
        let href = self.get_attribute(id, "href")?;
        parse(&href, parse(&self.base_url, None).as_ref())
    }

    /// One accessor over the whole `HyperlinkElementUtils` set, so the
    /// "no URL at all" answers cannot drift apart between eleven methods.
    ///
    /// ⛔ `protocol` is `":"` and everything else is `""` when there is no
    /// href — measured on a bare `<a>`.
    pub fn hyperlink_component(&self, id: u32, component: &str) -> String {
        let url = self.hyperlink_url(id);
        url_component(url.as_ref(), component)
    }

    /// `location.<component>` — the same set over the document's own URL.
    pub fn location_component(&self, component: &str) -> String {
        let url = parse(&self.base_url, None);
        url_component(url.as_ref(), component)
    }

    /// The element named by the document URL fragment, for `:target`.
    pub fn fragment_target_id(&self) -> u32 {
        let Some(url) = parse(&self.base_url, None) else {
            return 0;
        };
        if !url.has_fragment || url.fragment.is_empty() {
            return 0;
        }
        self.get_element_by_id(&url.fragment).unwrap_or(0)
    }
}

/// The IDL components of a URL, or the empty-URL answers when there is none.
pub fn url_component(url: Option<&Url>, component: &str) -> String {
    let Some(u) = url else {
        // ⛔ The asymmetry: `protocol` keeps its colon even with nothing to
        // put in front of it.
        return match component {
            "protocol" => ":".to_string(),
            _ => String::new(),
        };
    };
    match component {
        "href" => u.href(),
        "protocol" => u.protocol(),
        "username" => u.username.clone(),
        "password" => u.password.clone(),
        "host" => u.host(),
        "hostname" => u.hostname.clone(),
        "port" => u.port.clone(),
        "pathname" => u.pathname().to_string(),
        "search" => u.search(),
        "hash" => u.hash(),
        "origin" => u.origin(),
        _ => String::new(),
    }
}
