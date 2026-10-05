//! The open documents this browser holds, addressed by handle.
//!
//! A browser does not hand out `Document` values; it hands out handles and
//! keeps the trees itself, because a document outlives any one call into it and
//! several windows may name the same one. `vybe_widgets::dom` has exactly this,
//! and webcore needs it for the same reason — without it, `window.open()` has
//! nowhere to put the document it creates, and `document.defaultView` has
//! nothing to point back at.
//!
//! Same function names and shapes as the other engine's, so which browser is
//! compiled in is a build-time choice and callers do not change.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::types::{Document, DocumentKind};

/// A handle to an open document. `u64` to match the DOM's node handles — the
/// arena's own ids are `u32` and stay internal.
pub type DocumentId = u64;

#[derive(Default)]
struct Documents {
    /// `Mutex` per document, not one lock over the table: two windows driving
    /// two documents must not serialise against each other.
    docs: HashMap<DocumentId, crate::browser::BrowserDocument>,
    next_id: DocumentId,
}

fn documents() -> &'static Mutex<Documents> {
    static DOCS: OnceLock<Mutex<Documents>> = OnceLock::new();
    DOCS.get_or_init(|| Mutex::new(Documents::default()))
}

/// The width a document is laid out against until something resizes it.
/// `window.open(…, "width=…")` overrides it; so does `resize_to`.
pub const DEFAULT_VIEWPORT_WIDTH: f32 = 1024.0;
pub const DEFAULT_VIEWPORT_HEIGHT: f32 = 768.0;

/// Open a document — the spec's initial `about:blank`, with a title.
///
/// Parsed rather than assembled by hand so the result has the `<head>`/`<body>`
/// skeleton every later operation assumes, and so `<title>` is a real element
/// that `document.title` reads.
pub fn new_document(title: &str) -> DocumentId {
    let escaped = crate::html::serializer::escape_html(title);
    let doc = crate::load_html(
        &format!("<html><head><title>{escaped}</title></head><body></body></html>"),
        DEFAULT_VIEWPORT_WIDTH,
    );
    open_document(doc)
}

/// Open an XML document.
///
/// The tree is built the same way; what the kind changes is NAME FOLDING —
/// HTML ASCII-lowercases tag and attribute names, XML is case-sensitive, so
/// `<Rect>` and `<rect>` stay distinct. There is no XML tokenizer here and none
/// is needed: `DOMParser` lives above this layer and builds trees through
/// `create_element_ns`, so the parser is shared rather than duplicated.
pub fn new_xml_document(title: &str) -> DocumentId {
    let id = new_document(title);
    with_document(id, |d| d.kind = DocumentKind::Xml);
    id
}

fn open_document(document: Document) -> DocumentId {
    register_document(crate::browser::BrowserDocument::new(document))
}

/// Register a shared live DOM once, preserving its identity when it is adopted
/// by a native browser context after the host has already built its controls.
pub fn register_document(document: crate::browser::BrowserDocument) -> DocumentId {
    let mut docs = match documents().lock() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    if let Some((id, _)) = docs
        .docs
        .iter()
        .find(|(_, existing)| existing.same_document(&document))
    {
        return *id;
    }
    docs.next_id += 1;
    let id = docs.next_id;
    docs.docs.insert(id, document);
    id
}

/// Borrow an open document. `None` if the handle names none — a closed window's
/// document is gone, and asking about it is not an error.
pub fn with_document<T>(id: DocumentId, f: impl FnOnce(&mut Document) -> T) -> Option<T> {
    let document = documents().lock().ok()?.docs.get(&id)?.clone();
    Some(f(&mut document.write()))
}

/// Drop a document. What `window.close()` does to the page it was showing.
pub fn close_document(id: DocumentId) {
    if let Ok(mut docs) = documents().lock() {
        docs.docs.remove(&id);
    }
}

/// Whether the handle still names an open document.
pub fn is_open(id: DocumentId) -> bool {
    documents()
        .lock()
        .map(|d| d.docs.contains_key(&id))
        .unwrap_or(false)
}
