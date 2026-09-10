//! Reflected content attributes — HTML §2.6.
//!
//! Most IDL attributes on HTML elements are defined as "reflects the content
//! attribute", and the definition is exact enough to be a TABLE rather than a
//! hundred hand-written accessors. What varies is the *kind* of reflection,
//! and the kinds disagree in ways that matter:
//!
//! | absent value | measured |
//! |---|---|
//! | `form.method` | `"get"` |
//! | `input.formMethod` | `""` |
//! | `form.autocomplete` | `"on"` |
//! | `input.autocomplete` | `""` |
//! | `form.enctype` | `"application/x-www-form-urlencoded"` |
//! | `input.formEnctype` | `""` |
//! | `img.crossOrigin` | `null` — the only nullable one |
//! | `input.maxLength` | `-1` |
//! | `input.size` | `20` |
//!
//! Same-named concepts, different missing-value defaults. One uniform
//! "enumerated with a default" helper gets three of those wrong, and each one
//! passes any test that checks only the PRESENT case.
//!
//! ⛔ Three members that look like reflection and are not, kept OUT of the
//! table on purpose — a table with lies in it is worse than a smaller true one:
//!
//! * `script.async` answers `true` for a script with no `async` attribute at
//!   all. That is the force-async flag, not the content attribute.
//! * `img.width` answers the USED width — `width=30` measured back as `28`.
//! * `form.action` and `input.formAction` answer the DOCUMENT'S URL when
//!   absent, where every other URL attribute answers `""`.

use crate::types::Document;

/// How an IDL attribute reflects its content attribute.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// `DOMString`, verbatim. Absent is `""`.
    Str,
    /// A URL: absent is `""`, present is resolved against the document base.
    Url,
    /// `boolean` — the attribute's PRESENCE, whatever its value.
    Bool,
    /// `long`, with the missing-value default.
    Long(i64),
    /// An enumerated attribute: keywords, the missing-value default, and the
    /// invalid-value default.
    Enum(&'static [&'static str], &'static str, &'static str),
    /// Like `Enum`, but absent answers `null` rather than a keyword.
    NullableEnum(&'static [&'static str], &'static str),
}

/// What a reflected read answers. `Null` is the IDL `null`, which only the
/// nullable enumerated attributes can produce.
#[derive(Clone, Debug, PartialEq)]
pub enum Reflected {
    Str(String),
    Bool(bool),
    Long(i64),
    Null,
}

impl Reflected {
    /// The value as a string, for the callers that only want that. `Null`
    /// becomes `None`, which is the honest mapping — `""` is a different
    /// answer that `crossOrigin` can also give.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Reflected::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Reflected::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_long(&self) -> Option<i64> {
        match self {
            Reflected::Long(n) => Some(*n),
            _ => None,
        }
    }
}

const REFERRER_POLICY: &[&str] = &[
    "",
    "no-referrer",
    "no-referrer-when-downgrade",
    "same-origin",
    "origin",
    "strict-origin",
    "origin-when-cross-origin",
    "strict-origin-when-cross-origin",
    "unsafe-url",
];
const CROSS_ORIGIN: &[&str] = &["anonymous", "use-credentials"];

/// `(tag, IDL name, content attribute, kind)`.
///
/// Only the seven interfaces whose rules were measured against Chrome. The
/// table is meant to grow — but a mechanism with seven measured element types
/// beats fifteen guessed ones.
pub const REFLECTED: &[(&str, &str, &str, Kind)] = &[
    // ── HTMLImageElement ──
    ("img", "alt", "alt", Kind::Str),
    ("img", "src", "src", Kind::Url),
    ("img", "srcset", "srcset", Kind::Str),
    ("img", "sizes", "sizes", Kind::Str),
    ("img", "useMap", "usemap", Kind::Str),
    ("img", "isMap", "ismap", Kind::Bool),
    (
        "img",
        "crossOrigin",
        "crossorigin",
        Kind::NullableEnum(CROSS_ORIGIN, "anonymous"),
    ),
    (
        "img",
        "decoding",
        "decoding",
        Kind::Enum(&["sync", "async", "auto"], "auto", "auto"),
    ),
    (
        "img",
        "loading",
        "loading",
        Kind::Enum(&["lazy", "eager"], "auto", "auto"),
    ),
    (
        "img",
        "referrerPolicy",
        "referrerpolicy",
        Kind::Enum(REFERRER_POLICY, "", ""),
    ),
    // ── HTMLSourceElement ──
    ("source", "src", "src", Kind::Url),
    ("source", "type", "type", Kind::Str),
    ("source", "srcset", "srcset", Kind::Str),
    ("source", "sizes", "sizes", Kind::Str),
    ("source", "media", "media", Kind::Str),
    ("source", "width", "width", Kind::Long(0)),
    ("source", "height", "height", Kind::Long(0)),
    // ── HTMLAnchorElement ──
    ("a", "href", "href", Kind::Url),
    ("a", "rel", "rel", Kind::Str),
    ("a", "hreflang", "hreflang", Kind::Str),
    ("a", "type", "type", Kind::Str),
    ("a", "target", "target", Kind::Str),
    ("a", "download", "download", Kind::Str),
    (
        "a",
        "referrerPolicy",
        "referrerpolicy",
        Kind::Enum(REFERRER_POLICY, "", ""),
    ),
    // ── HTMLLinkElement ──
    ("link", "href", "href", Kind::Url),
    ("link", "rel", "rel", Kind::Str),
    ("link", "media", "media", Kind::Str),
    ("link", "as", "as", Kind::Str),
    ("link", "hreflang", "hreflang", Kind::Str),
    ("link", "type", "type", Kind::Str),
    ("link", "integrity", "integrity", Kind::Str),
    ("link", "disabled", "disabled", Kind::Bool),
    (
        "link",
        "crossOrigin",
        "crossorigin",
        Kind::NullableEnum(CROSS_ORIGIN, "anonymous"),
    ),
    (
        "link",
        "referrerPolicy",
        "referrerpolicy",
        Kind::Enum(REFERRER_POLICY, "", ""),
    ),
    // ── HTMLScriptElement (NOT `async` — see the module note) ──
    ("script", "src", "src", Kind::Url),
    ("script", "defer", "defer", Kind::Bool),
    ("script", "type", "type", Kind::Str),
    ("script", "noModule", "nomodule", Kind::Bool),
    ("script", "integrity", "integrity", Kind::Str),
    (
        "script",
        "crossOrigin",
        "crossorigin",
        Kind::NullableEnum(CROSS_ORIGIN, "anonymous"),
    ),
    (
        "script",
        "referrerPolicy",
        "referrerpolicy",
        Kind::Enum(REFERRER_POLICY, "", ""),
    ),
    // ── HTMLFormElement (NOT `action` — see the module note) ──
    (
        "form",
        "method",
        "method",
        Kind::Enum(&["get", "post", "dialog"], "get", "get"),
    ),
    (
        "form",
        "enctype",
        "enctype",
        Kind::Enum(
            &[
                "application/x-www-form-urlencoded",
                "multipart/form-data",
                "text/plain",
            ],
            "application/x-www-form-urlencoded",
            "application/x-www-form-urlencoded",
        ),
    ),
    // `encoding` is a second name for the same attribute (HTML §4.10.3).
    (
        "form",
        "encoding",
        "enctype",
        Kind::Enum(
            &[
                "application/x-www-form-urlencoded",
                "multipart/form-data",
                "text/plain",
            ],
            "application/x-www-form-urlencoded",
            "application/x-www-form-urlencoded",
        ),
    ),
    ("form", "noValidate", "novalidate", Kind::Bool),
    ("form", "acceptCharset", "accept-charset", Kind::Str),
    ("form", "target", "target", Kind::Str),
    ("form", "name", "name", Kind::Str),
    ("form", "rel", "rel", Kind::Str),
    // ⛔ `"on"` when absent — where `input.autocomplete` answers `""`.
    (
        "form",
        "autocomplete",
        "autocomplete",
        Kind::Enum(&["on", "off"], "on", "on"),
    ),
    // ── HTMLInputElement (NOT `formAction` — see the module note) ──
    ("input", "maxLength", "maxlength", Kind::Long(-1)),
    ("input", "minLength", "minlength", Kind::Long(-1)),
    ("input", "size", "size", Kind::Long(20)),
    ("input", "accept", "accept", Kind::Str),
    ("input", "alt", "alt", Kind::Str),
    ("input", "src", "src", Kind::Url),
    ("input", "dirName", "dirname", Kind::Str),
    ("input", "autocomplete", "autocomplete", Kind::Str),
    ("input", "formTarget", "formtarget", Kind::Str),
    ("input", "formNoValidate", "formnovalidate", Kind::Bool),
    // ⛔ `""` when absent, where the FORM's own `method`/`enctype` answer a
    // keyword. Same concept, different default.
    (
        "input",
        "formMethod",
        "formmethod",
        Kind::Enum(&["get", "post", "dialog"], "", "get"),
    ),
    (
        "input",
        "formEnctype",
        "formenctype",
        Kind::Enum(
            &[
                "application/x-www-form-urlencoded",
                "multipart/form-data",
                "text/plain",
            ],
            "",
            "application/x-www-form-urlencoded",
        ),
    ),
    // `defaultValue` and `defaultChecked` are the CONTENT attributes behind
    // `value` and `checked` — the values a reset returns to.
    ("input", "defaultValue", "value", Kind::Str),
    ("input", "defaultChecked", "checked", Kind::Bool),
    // ── HTMLIFrameElement ──
    ("iframe", "src", "src", Kind::Url),
    ("iframe", "name", "name", Kind::Str),
    ("iframe", "allow", "allow", Kind::Str),
    ("iframe", "allowFullscreen", "allowfullscreen", Kind::Bool),
    // ⛔ DOMString here, where `img`'s are longs — measured `""`, not `0`.
    ("iframe", "width", "width", Kind::Str),
    ("iframe", "height", "height", Kind::Str),
    (
        "iframe",
        "referrerPolicy",
        "referrerpolicy",
        Kind::Enum(REFERRER_POLICY, "", ""),
    ),
    // ── HTMLTextAreaElement / HTMLSelectElement / HTMLButtonElement ──
    ("textarea", "maxLength", "maxlength", Kind::Long(-1)),
    ("textarea", "minLength", "minlength", Kind::Long(-1)),
    ("textarea", "dirName", "dirname", Kind::Str),
    ("textarea", "autocomplete", "autocomplete", Kind::Str),
    ("textarea", "placeholder", "placeholder", Kind::Str),
    ("textarea", "wrap", "wrap", Kind::Str),
    ("select", "autocomplete", "autocomplete", Kind::Str),
    ("button", "formTarget", "formtarget", Kind::Str),
    ("button", "formNoValidate", "formnovalidate", Kind::Bool),
];

/// The table row for `(tag, idl)`, if there is one.
pub fn lookup(tag: &str, idl: &str) -> Option<Kind> {
    REFLECTED
        .iter()
        .find(|(t, i, _, _)| *t == tag && i.eq_ignore_ascii_case(idl))
        .map(|(_, _, _, k)| *k)
}

fn attr_for(tag: &str, idl: &str) -> Option<&'static str> {
    REFLECTED
        .iter()
        .find(|(t, i, _, _)| *t == tag && i.eq_ignore_ascii_case(idl))
        .map(|(_, _, a, _)| *a)
}

impl Document {
    /// Read a reflected IDL attribute. `None` means "this element has no such
    /// reflected attribute" — distinct from `Some(Reflected::Null)`, which is
    /// an attribute that IS reflected and whose value is the IDL `null`.
    pub fn reflect_get(&self, id: u32, idl: &str) -> Option<Reflected> {
        let tag = self.tag_name(id)?.to_string();
        let kind = lookup(&tag, idl)?;
        let attr = attr_for(&tag, idl)?;
        let raw = self.get_attribute(id, attr);
        Some(match kind {
            Kind::Str => Reflected::Str(raw.unwrap_or_default()),
            Kind::Url => Reflected::Str(match raw {
                Some(v) if !v.is_empty() => crate::html::resolve_url(&v, &self.base_url),
                // ⛔ An absent URL attribute is `""`, NOT the document's URL.
                // The two exceptions — `form.action` and `input.formAction` —
                // are deliberately not in the table.
                _ => String::new(),
            }),
            Kind::Bool => Reflected::Bool(raw.is_some()),
            Kind::Long(default) => Reflected::Long(
                raw.and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(default),
            ),
            Kind::Enum(keywords, missing, invalid) => {
                let Some(v) = raw else {
                    return Some(Reflected::Str(missing.to_string()));
                };
                let lower = v.to_ascii_lowercase();
                Reflected::Str(if keywords.contains(&lower.as_str()) {
                    lower
                } else {
                    invalid.to_string()
                })
            }
            Kind::NullableEnum(keywords, invalid) => {
                let Some(v) = raw else {
                    return Some(Reflected::Null);
                };
                let lower = v.to_ascii_lowercase();
                Reflected::Str(if keywords.contains(&lower.as_str()) {
                    lower
                } else {
                    invalid.to_string()
                })
            }
        })
    }

    /// Write a reflected IDL attribute. `false` means there is no such
    /// reflected attribute on this element.
    ///
    /// A `Bool` kind adds or removes the attribute; everything else writes the
    /// value as a string. Setting a nullable enumerated attribute to
    /// `Reflected::Null` removes it, which is how `img.crossOrigin = null`
    /// gets back to answering `null`.
    pub fn reflect_set(&mut self, id: u32, idl: &str, value: Reflected) -> bool {
        let Some(tag) = self.tag_name(id).map(|t| t.to_string()) else {
            return false;
        };
        let Some(kind) = lookup(&tag, idl) else {
            return false;
        };
        let Some(attr) = attr_for(&tag, idl) else {
            return false;
        };
        match (kind, value) {
            (Kind::Bool, Reflected::Bool(true)) => self.set_attribute(id, attr, ""),
            (Kind::Bool, Reflected::Bool(false)) => self.remove_attribute(id, attr),
            (_, Reflected::Null) => self.remove_attribute(id, attr),
            (_, Reflected::Str(s)) => self.set_attribute(id, attr, &s),
            (_, Reflected::Long(n)) => self.set_attribute(id, attr, &n.to_string()),
            (_, Reflected::Bool(b)) => {
                self.set_attribute(id, attr, if b { "true" } else { "false" })
            }
        }
        true
    }

    /// Every reflected IDL name this element has, for enumeration and for the
    /// coverage tooling.
    pub fn reflected_names(&self, id: u32) -> Vec<&'static str> {
        let Some(tag) = self.tag_name(id) else {
            return Vec::new();
        };
        REFLECTED
            .iter()
            .filter(|(t, ..)| *t == tag)
            .map(|(_, i, ..)| *i)
            .collect()
    }
}
