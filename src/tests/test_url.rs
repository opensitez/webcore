//! The URL parser, `Location` and `HyperlinkElementUtils`.
//!
//! The table below is Chrome's answers verbatim (`/tmp/webcore-html/url.html`),
//! run through `new URL(input, base)` with one base for every row. It is kept
//! as one table on purpose: the rows discriminate each other, and picking a
//! subset is how a parser ends up passing while getting the special-scheme,
//! opaque-scheme and empty-URL cases wrong.

use crate::dom::url::{Url, parse};
use crate::html::parse_html_with_base;
use crate::types::Document;

const BASE: &str = "http://base.example.org/dir/page.html?bq#bh";

fn base() -> Url {
    parse(BASE, None).expect("the base parses")
}
fn u(input: &str) -> Url {
    parse(input, Some(&base())).expect("parses")
}

/// (input, href, protocol, host, hostname, port, pathname, search, hash, origin)
const TABLE: &[(&str, &str, &str, &str, &str, &str, &str, &str, &str, &str)] = &[
    (
        "http://example.com/a/b?x=1#frag",
        "http://example.com/a/b?x=1#frag",
        "http:",
        "example.com",
        "example.com",
        "",
        "/a/b",
        "?x=1",
        "#frag",
        "http://example.com",
    ),
    (
        "https://user:pw@example.com:8443/p?q#h",
        "https://user:pw@example.com:8443/p?q#h",
        "https:",
        "example.com:8443",
        "example.com",
        "8443",
        "/p",
        "?q",
        "#h",
        "https://example.com:8443",
    ),
    // ⛔ The scheme's default port is dropped from host, port AND href.
    (
        "http://example.com:80/",
        "http://example.com/",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    (
        "https://example.com:443/",
        "https://example.com/",
        "https:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "https://example.com",
    ),
    // ⛔ An empty path on a special scheme becomes `/`, and href GAINS it.
    (
        "http://example.com",
        "http://example.com/",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    (
        "http://example.com:8080",
        "http://example.com:8080/",
        "http:",
        "example.com:8080",
        "example.com",
        "8080",
        "/",
        "",
        "",
        "http://example.com:8080",
    ),
    (
        "https://example.com/a/../b/./c",
        "https://example.com/b/c",
        "https:",
        "example.com",
        "example.com",
        "",
        "/b/c",
        "",
        "",
        "https://example.com",
    ),
    // ⛔ `file:` has an empty host and its own origin shape.
    (
        "file:///tmp/x.txt",
        "file:///tmp/x.txt",
        "file:",
        "",
        "",
        "",
        "/tmp/x.txt",
        "",
        "",
        "file://",
    ),
    // ⛔ Opaque schemes: the whole remainder is the PATHNAME, origin is the
    // STRING "null". Three of them, because a parser that special-cases only
    // `data:` passes with one.
    (
        "data:text/plain,hi",
        "data:text/plain,hi",
        "data:",
        "",
        "",
        "",
        "text/plain,hi",
        "",
        "",
        "null",
    ),
    (
        "mailto:a@b.c",
        "mailto:a@b.c",
        "mailto:",
        "",
        "",
        "",
        "a@b.c",
        "",
        "",
        "null",
    ),
    (
        "about:blank",
        "about:blank",
        "about:",
        "",
        "",
        "",
        "blank",
        "",
        "",
        "null",
    ),
    // ⛔ href KEEPS the delimiter where the component is empty.
    (
        "http://example.com/?",
        "http://example.com/?",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    (
        "http://example.com/#",
        "http://example.com/#",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    // Relative forms.
    (
        "//cdn.example.com/x.js",
        "http://cdn.example.com/x.js",
        "http:",
        "cdn.example.com",
        "cdn.example.com",
        "",
        "/x.js",
        "",
        "",
        "http://cdn.example.com",
    ),
    (
        "/root/path",
        "http://base.example.org/root/path",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/root/path",
        "",
        "",
        "http://base.example.org",
    ),
    (
        "rel/path",
        "http://base.example.org/dir/rel/path",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/rel/path",
        "",
        "",
        "http://base.example.org",
    ),
    (
        "?onlyquery",
        "http://base.example.org/dir/page.html?onlyquery",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/page.html",
        "?onlyquery",
        "",
        "http://base.example.org",
    ),
    (
        "#onlyhash",
        "http://base.example.org/dir/page.html?bq#onlyhash",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/page.html",
        "?bq",
        "#onlyhash",
        "http://base.example.org",
    ),
    // ⛔ The empty string is the base MINUS its fragment.
    (
        "",
        "http://base.example.org/dir/page.html?bq",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/page.html",
        "?bq",
        "",
        "http://base.example.org",
    ),
];

#[test]
fn the_parser_matches_chrome_row_for_row() {
    for (input, href, protocol, host, hostname, port, pathname, search, hash, origin) in TABLE {
        let p = u(input);
        assert_eq!(&p.href(), href, "href of {input:?}");
        assert_eq!(&p.protocol(), protocol, "protocol of {input:?}");
        assert_eq!(&p.host(), host, "host of {input:?}");
        assert_eq!(&p.hostname, hostname, "hostname of {input:?}");
        assert_eq!(&p.port, port, "port of {input:?}");
        assert_eq!(p.pathname(), *pathname, "pathname of {input:?}");
        assert_eq!(&p.search(), search, "search of {input:?}");
        assert_eq!(&p.hash(), hash, "hash of {input:?}");
        assert_eq!(&p.origin(), origin, "origin of {input:?}");
    }
}

/// The rows the first table did NOT discriminate. A mutation run found eight
/// branches it could delete without turning anything red — every one of them
/// because the inputs I had chosen never reached it. Measured like the rest.
const TABLE2: &[(&str, &str, &str, &str, &str, &str, &str, &str, &str, &str)] = &[
    // A query-relative reference carries its own fragment.
    (
        "?q#f",
        "http://base.example.org/dir/page.html?q#f",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/page.html",
        "?q",
        "#f",
        "http://base.example.org",
    ),
    // ⛔ Scheme AND host are lowercased; the PATH keeps its case.
    (
        "HTTP://EXAMPLE.COM/Path",
        "http://example.com/Path",
        "http:",
        "example.com",
        "example.com",
        "",
        "/Path",
        "",
        "",
        "http://example.com",
    ),
    // ⛔ An authority comes from the `//`, not from the scheme being special:
    // a non-special scheme can have a host, and its origin is still "null".
    (
        "a1+-.b://host/p",
        "a1+-.b://host/p",
        "a1+-.b:",
        "host",
        "host",
        "",
        "/p",
        "",
        "",
        "null",
    ),
    // ⛔ Not a scheme — a scheme cannot start with a digit, so this resolves
    // as a relative path.
    (
        "1http://x",
        "http://base.example.org/dir/1http://x",
        "http:",
        "base.example.org",
        "base.example.org",
        "",
        "/dir/1http://x",
        "",
        "",
        "http://base.example.org",
    ),
    // An EMPTY port is legal and drops out.
    (
        "http://example.com:/",
        "http://example.com/",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    // Trailing dot segments keep the trailing slash; popping past the root is
    // not an error.
    (
        "http://example.com/a/..",
        "http://example.com/",
        "http:",
        "example.com",
        "example.com",
        "",
        "/",
        "",
        "",
        "http://example.com",
    ),
    (
        "http://example.com/a/.",
        "http://example.com/a/",
        "http:",
        "example.com",
        "example.com",
        "",
        "/a/",
        "",
        "",
        "http://example.com",
    ),
    (
        "http://example.com/../../x",
        "http://example.com/x",
        "http:",
        "example.com",
        "example.com",
        "",
        "/x",
        "",
        "",
        "http://example.com",
    ),
];

#[test]
fn the_rows_the_first_table_could_not_discriminate() {
    for (input, href, protocol, host, hostname, port, pathname, search, hash, origin) in TABLE2 {
        let p = u(input);
        assert_eq!(&p.href(), href, "href of {input:?}");
        assert_eq!(&p.protocol(), protocol, "protocol of {input:?}");
        assert_eq!(&p.host(), host, "host of {input:?}");
        assert_eq!(&p.hostname, hostname, "hostname of {input:?}");
        assert_eq!(&p.port, port, "port of {input:?}");
        assert_eq!(p.pathname(), *pathname, "pathname of {input:?}");
        assert_eq!(&p.search(), search, "search of {input:?}");
        assert_eq!(&p.hash(), hash, "hash of {input:?}");
        assert_eq!(&p.origin(), origin, "origin of {input:?}");
    }
}

#[test]
fn a_protocol_relative_reference_takes_the_bases_scheme_whatever_it_is() {
    // ⛔ The first table only ever resolved `//host` against an http base, so
    // hardcoding `"http"` passed it.
    let https = parse("https://secure.example.org/dir/p.html", None).unwrap();
    let got = parse("//cdn.example.com/x.js", Some(&https)).unwrap();
    assert_eq!(got.href(), "https://cdn.example.com/x.js");
    assert_eq!(got.protocol(), "https:");
}

#[test]
fn a_port_that_is_not_a_number_is_a_parse_failure() {
    // ⛔ Chrome throws `TypeError` rather than folding `:80x` into the host.
    assert_eq!(parse("http://example.com:80x/", Some(&base())), None);
    assert_eq!(parse("http://example.com:1a/", Some(&base())), None);
    assert!(
        parse("http://example.com:8080/", Some(&base())).is_some(),
        "digits are fine"
    );
}

#[test]
fn credentials_split_at_the_LAST_at_sign() {
    // `http://u@ser:pw@example.com/` is user `u@ser`, host `example.com`.
    // ⛔ Chrome percent-encodes the `@` into `u%40ser`; this parser does no
    // encoding normalization at all (named in architecture.md), so it keeps
    // the raw form. The SPLIT POINT is what this pins.
    let p = u("http://u@ser:pw@example.com/");
    assert_eq!(p.hostname, "example.com", "the last @ ends the credentials");
    assert_eq!(p.password, "pw");
    assert!(p.username.contains("ser"), "got {:?}", p.username);
}

#[test]
fn href_cannot_be_rebuilt_from_the_components() {
    // ⛔ The single most likely way to write this wrong. `search` is `""` and
    // `href` still carries the `?` — the delimiter's PRESENCE is separate
    // state from the component's value. Concatenating components drops both,
    // and every other row in the table still passes.
    let q = u("http://example.com/?");
    assert_eq!(q.search(), "");
    assert!(
        q.href().ends_with('?'),
        "href kept the delimiter: {:?}",
        q.href()
    );
    let h = u("http://example.com/#");
    assert_eq!(h.hash(), "");
    assert!(h.href().ends_with('#'), "{:?}", h.href());
}

#[test]
fn origin_has_three_shapes_and_only_one_is_scheme_slash_slash_host() {
    assert_eq!(u("http://example.com/x").origin(), "http://example.com");
    assert_eq!(
        u("file:///tmp/x").origin(),
        "file://",
        "a scheme with an empty host"
    );
    assert_eq!(u("mailto:a@b.c").origin(), "null", "the STRING null");
    assert_ne!(u("mailto:a@b.c").origin(), "", "not an empty string");
}

#[test]
fn a_non_special_scheme_has_no_authority_at_all() {
    for input in ["data:text/plain,hi", "mailto:a@b.c", "about:blank"] {
        let p = u(input);
        assert!(!p.special, "{input}");
        assert_eq!(p.hostname, "", "{input} hostname");
        assert_eq!(p.port, "", "{input} port");
        assert!(
            !p.pathname().starts_with('/'),
            "{input} keeps its opaque path"
        );
    }
}

// ─── the interfaces on top ──────────────────────────────────────────────────

fn page() -> Document {
    parse_html_with_base(
        r#"<a id=full href="http://example.com:8080/a/b?q=1#f">F</a>
           <a id=rel href="x.html">R</a>
           <a id=bare>B</a>"#,
        BASE,
    )
}

#[test]
fn a_hyperlinks_components_come_from_its_resolved_url() {
    let d = page();
    let full = d.get_element_by_id("full").unwrap();
    let c = |n: &str| d.hyperlink_component(full, n);
    assert_eq!(c("protocol"), "http:");
    assert_eq!(c("host"), "example.com:8080");
    assert_eq!(c("hostname"), "example.com");
    assert_eq!(c("port"), "8080");
    assert_eq!(c("pathname"), "/a/b");
    assert_eq!(c("search"), "?q=1");
    assert_eq!(c("hash"), "#f");
    assert_eq!(c("origin"), "http://example.com:8080");

    let rel = d.get_element_by_id("rel").unwrap();
    assert_eq!(
        d.hyperlink_component(rel, "href"),
        "http://base.example.org/dir/x.html"
    );
    assert_eq!(d.hyperlink_component(rel, "hostname"), "base.example.org");
}

#[test]
fn a_hyperlink_with_no_href_answers_a_bare_colon_for_its_protocol() {
    // ⛔ The asymmetry, and the common case — `<a name=…>` has no href.
    // Everything else is `""`; `protocol` is `":"`.
    let d = page();
    let bare = d.get_element_by_id("bare").unwrap();
    assert_eq!(d.hyperlink_component(bare, "protocol"), ":");
    for n in [
        "href", "host", "hostname", "port", "pathname", "search", "hash", "origin",
    ] {
        assert_eq!(d.hyperlink_component(bare, n), "", "{n}");
    }
}

#[test]
fn location_decomposes_the_documents_own_url() {
    let d = page();
    assert_eq!(d.location_component("protocol"), "http:");
    assert_eq!(d.location_component("hostname"), "base.example.org");
    assert_eq!(d.location_component("pathname"), "/dir/page.html");
    assert_eq!(d.location_component("search"), "?bq");
    assert_eq!(d.location_component("hash"), "#bh");
    assert_eq!(d.location_component("origin"), "http://base.example.org");
}

#[test]
fn a_document_with_no_base_url_has_no_location_components() {
    let d = crate::html::parse_html("<a id=a href='x'>x</a>");
    assert_eq!(d.location_component("protocol"), ":");
    assert_eq!(d.location_component("hostname"), "");
    // And a relative href with nothing to resolve against is not a URL either.
    let a = d.get_element_by_id("a").unwrap();
    assert_eq!(d.hyperlink_component(a, "protocol"), ":");
}
