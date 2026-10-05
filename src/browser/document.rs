//! Shared ownership of a live DOM between a browser context and its host bridge.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::Document;

#[derive(Clone)]
pub struct BrowserDocument {
    document: Arc<Mutex<Document>>,
    resources_dirty: Arc<AtomicBool>,
}

impl BrowserDocument {
    pub fn new(document: Document) -> Self {
        Self {
            document: Arc::new(Mutex::new(document)),
            resources_dirty: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn read(&self) -> DocumentRead<'_> {
        DocumentRead::Shared(
            self.document
                .lock()
                .expect("browser document lock poisoned"),
        )
    }

    pub fn write(&self) -> DocumentWrite<'_> {
        DocumentWrite::Shared(
            self.document
                .lock()
                .expect("browser document lock poisoned"),
        )
    }

    pub fn resources_changed(&self) {
        self.resources_dirty.store(true, Ordering::Release);
    }

    pub(crate) fn take_resource_changes(&self) -> bool {
        self.resources_dirty.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn same_document(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.document, &other.document)
    }
}

pub enum DocumentRead<'a> {
    Borrowed(&'a Document),
    Shared(MutexGuard<'a, Document>),
}

impl Deref for DocumentRead<'_> {
    type Target = Document;
    fn deref(&self) -> &Document {
        match self {
            Self::Borrowed(document) => document,
            Self::Shared(document) => document,
        }
    }
}

pub enum DocumentWrite<'a> {
    Borrowed(&'a mut Document),
    Shared(MutexGuard<'a, Document>),
}

impl Deref for DocumentWrite<'_> {
    type Target = Document;
    fn deref(&self) -> &Document {
        match self {
            Self::Borrowed(document) => document,
            Self::Shared(document) => document,
        }
    }
}

impl DerefMut for DocumentWrite<'_> {
    fn deref_mut(&mut self) -> &mut Document {
        match self {
            Self::Borrowed(document) => document,
            Self::Shared(document) => document,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_preserves_shared_dom_and_does_not_lock_other_documents() {
        let handle = BrowserDocument::new(crate::parse_html(
            "<body><div id='shared'>Before</div></body>",
        ));
        let id = crate::dom::registry::register_document(handle.clone());
        assert_eq!(crate::dom::registry::register_document(handle.clone()), id);
        let other = crate::dom::registry::new_document("Other");
        crate::dom::registry::with_document(id, |doc| {
            let node = doc.get_element_by_id("shared").unwrap();
            doc.set_text_content(node, "After");
            assert_eq!(
                crate::dom::registry::with_document(other, |doc| doc.title()),
                Some("Other".into())
            );
        });
        let doc = handle.read();
        let node = doc.get_element_by_id("shared").unwrap();
        assert_eq!(doc.text_content(node), "After");
        drop(doc);
        crate::dom::registry::close_document(id);
        crate::dom::registry::close_document(other);
    }

    #[test]
    fn resource_changes_coalesce_between_browser_frames() {
        let document = BrowserDocument::new(crate::parse_html("<body></body>"));
        assert!(document.take_resource_changes());
        assert!(!document.take_resource_changes());
        document.resources_changed();
        document.resources_changed();
        assert!(document.take_resource_changes());
        assert!(!document.take_resource_changes());
    }
}
