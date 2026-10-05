//! Document metadata, the doctype and the document collections
//! (HTML §3.1.1, DOM §4.5).

use crate::dom::arena::NodeId;
use crate::types::Document;

// ─── Document metadata and the document collections (HTML §3.1.1, DOM §4.5) ─
//
// Everything here was read off Chrome first (`/tmp/webcore-html/doc1.html`,
// `dt/`). Four of the collection rules are not what the names suggest, and each
// is a place a plausible implementation is wrong:
//
//   * `links` is `<a>` AND `<area>`, and only those WITH an `href`.
//   * `anchors` is `<a>` with a **name** attribute — an `<a name href>` is in
//     both lists, and a bare `<a>` is in neither.
//   * `getElementsByName` matches ANY element, not just form controls: a
//     `<div name=x>` comes back.
//   * `applets` is always empty, and `plugins` is the same list as `embeds`.
//
// These return `Vec<u32>` — a snapshot — where Chrome's are LIVE
// `HTMLCollection`s. Measured: appending an `<a href>` grows `document.links`
// from 2 to 3 without re-reading it. Noted in `architecture.md`; a live
// collection is a different object model, not a different filter.

impl Document {
    /// `document.doctype` — 0 when the document has none.
    pub fn doctype(&self) -> Option<u32> {
        (self.doctype != 0).then_some(self.doctype)
    }

    /// `doctype.name`, which is also its `nodeName`.
    pub fn doctype_name(&self) -> Option<String> {
        self.arena
            .try_get(NodeId(self.doctype))
            .map(|n| n.tag.clone())
    }

    /// `doctype.publicId` — the empty string when absent, never null.
    pub fn doctype_public_id(&self) -> Option<String> {
        self.doctype_ident("publicId")
    }

    /// `doctype.systemId`.
    pub fn doctype_system_id(&self) -> Option<String> {
        self.doctype_ident("systemId")
    }

    fn doctype_ident(&self, key: &str) -> Option<String> {
        let node = self.arena.try_get(NodeId(self.doctype))?;
        Some(node.attributes.get(key).cloned().unwrap_or_default())
    }

    /// `document.compatMode` — `"BackCompat"` in quirks, `"CSS1Compat"`
    /// otherwise.
    ///
    /// ⛔ This CANNOT distinguish limited-quirks from no-quirks; both answer
    /// `"CSS1Compat"`. Read `self.quirks` for the real mode.
    pub fn compat_mode(&self) -> &'static str {
        self.quirks.compat_mode()
    }

    /// `document.characterSet` / `.charset` / `.inputEncoding` — three names
    /// for one value (DOM §4.5).
    pub fn character_set(&self) -> &str {
        &self.character_set
    }

    /// `document.contentType`. This crate parses HTML and only HTML.
    pub fn content_type(&self) -> &'static str {
        "text/html"
    }

    /// `document.readyState`.
    ///
    /// Always `"complete"`: `parse_html` RETURNS a finished document, so there
    /// is no moment at which a caller holds one that is still loading. Chrome's
    /// probe answered `"loading"` only because an inline script ran mid-parse,
    /// which is a state this crate never exposes.
    pub fn ready_state(&self) -> &'static str {
        "complete"
    }

    /// `document.visibilityState` and `document.hidden`. No page-visibility
    /// machinery exists, and a document nobody has hidden is visible.
    pub fn visibility_state(&self) -> &'static str {
        "visible"
    }
    pub fn document_hidden(&self) -> bool {
        false
    }

    /// `document.referrer` — empty with no navigation history to draw on.
    pub fn referrer(&self) -> &'static str {
        ""
    }

    /// `document.URL` / `.documentURI` — the base URL the document was parsed
    /// against.
    pub fn document_uri(&self) -> &str {
        self.navigation_url.as_deref().unwrap_or(&self.base_url)
    }

    /// `document.links` — `<a>` and `<area>` that HAVE an `href`.
    pub fn links(&self) -> Vec<u32> {
        self.collect_elements(|d, id| {
            matches!(d.tag_name(id), Some("a") | Some("area")) && d.has_attribute(id, "href")
        })
    }

    /// `document.anchors` — `<a>` with a `name`, whether or not it has an
    /// `href`.
    pub fn anchors(&self) -> Vec<u32> {
        self.collect_elements(|d, id| d.tag_name(id) == Some("a") && d.has_attribute(id, "name"))
    }

    /// `document.images` — every `<img>`, with or without a `src`.
    pub fn images(&self) -> Vec<u32> {
        self.collect_elements(|d, id| d.tag_name(id) == Some("img"))
    }

    /// `document.forms`.
    pub fn forms(&self) -> Vec<u32> {
        self.collect_elements(|d, id| d.tag_name(id) == Some("form"))
    }

    /// `document.scripts`.
    pub fn scripts(&self) -> Vec<u32> {
        self.collect_elements(|d, id| d.tag_name(id) == Some("script"))
    }

    /// `document.embeds`, and `document.plugins`, which is the same list.
    pub fn embeds(&self) -> Vec<u32> {
        self.collect_elements(|d, id| d.tag_name(id) == Some("embed"))
    }

    /// `document.plugins` — an alias for `embeds` (measured: same length, and
    /// the spec says they return the same collection).
    pub fn plugins(&self) -> Vec<u32> {
        self.embeds()
    }

    /// `document.applets`, which HTML defines as always empty — `<applet>` was
    /// removed from the language and the collection kept for compatibility.
    pub fn applets(&self) -> Vec<u32> {
        Vec::new()
    }

    /// `document.getElementsByName(name)`.
    ///
    /// ⛔ Any element with that `name` attribute, not just form controls: a
    /// `<div name=x>` is in the list (measured).
    pub fn get_elements_by_name(&self, name: &str) -> Vec<u32> {
        self.collect_elements(|d, id| d.get_attribute(id, "name").as_deref() == Some(name))
    }

    /// `document.getElementsByTagNameNS(ns, local)`.
    ///
    /// `"*"` matches any namespace or any local name. An HTML element carries
    /// no explicit namespace in the arena, so it answers to the XHTML one —
    /// which is what it is in (measured: 3 `<a>` for the XHTML namespace).
    pub fn get_elements_by_tag_name_ns(&self, namespace: &str, local: &str) -> Vec<u32> {
        const XHTML: &str = "http://www.w3.org/1999/xhtml";
        self.collect_elements(|d, id| {
            let Some(node) = d.arena.try_get(NodeId(id)) else {
                return false;
            };
            let ns = node.namespace.as_deref().unwrap_or(XHTML);
            let ns_ok = namespace == "*" || namespace == ns;
            let name_ok = local == "*" || d.local_name(id).eq_ignore_ascii_case(local);
            ns_ok && name_ok
        })
    }

    /// Every element in tree order that satisfies `pred`.
    fn collect_elements(&self, pred: impl Fn(&Document, u32) -> bool) -> Vec<u32> {
        self.get_elements_by_tag_name("*")
            .into_iter()
            .filter(|id| pred(self, *id))
            .collect()
    }

    /// `document.captureEvents()` / `document.releaseEvents()`.
    ///
    /// Defined by HTML as doing nothing at all — they exist so that old scripts
    /// calling them do not throw. Kept as real no-op methods for exactly that
    /// reason (measured: both return without error).
    pub fn capture_events(&self) {}
    pub fn release_events(&self) {}
}
