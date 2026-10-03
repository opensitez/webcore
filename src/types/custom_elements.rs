//! Custom-element definitions for the standalone WebCore document.
//!
//! A definition receives the same element and lifecycle transitions as a
//! `CustomElementRegistry` definition. The browser bridge can use these
//! reactions to invoke Vybe constructors; standalone embedders implement the
//! callbacks directly without needing a JavaScript engine.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::Document;

pub trait CustomElement: Send + Sync {
    fn observed_attributes(&self) -> &[&str] {
        &[]
    }

    fn construct(&self, _document: &mut Document, _element: u32) {}
    fn connected_callback(&self, _document: &mut Document, _element: u32) {}
    fn disconnected_callback(&self, _document: &mut Document, _element: u32) {}
    fn attribute_changed_callback(
        &self,
        _document: &mut Document,
        _element: u32,
        _name: &str,
        _old_value: Option<&str>,
        _new_value: Option<&str>,
    ) {
    }
}

#[derive(Default, Clone)]
pub struct CustomElementRegistry {
    definitions: HashMap<String, Arc<dyn CustomElement>>,
    upgraded: HashSet<u32>,
}

impl CustomElementRegistry {
    pub fn get(&self, name: &str) -> Option<Arc<dyn CustomElement>> {
        self.definitions.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.definitions.contains_key(name)
    }

    pub(crate) fn is_upgraded(&self, id: u32) -> bool {
        self.upgraded.contains(&id)
    }

    pub(crate) fn mark_upgraded(&mut self, id: u32) -> bool {
        self.upgraded.insert(id)
    }

    pub(crate) fn insert(
        &mut self,
        name: String,
        definition: Arc<dyn CustomElement>,
    ) -> Result<(), CustomElementError> {
        if !valid_custom_element_name(&name) {
            return Err(CustomElementError::InvalidName);
        }
        if self.definitions.contains_key(&name) {
            return Err(CustomElementError::AlreadyDefined);
        }
        self.definitions.insert(name, definition);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomElementError {
    InvalidName,
    AlreadyDefined,
}

fn valid_custom_element_name(name: &str) -> bool {
    let Some((prefix, suffix)) = name.split_once('-') else {
        return false;
    };
    if prefix.is_empty() || suffix.is_empty() || name.starts_with("xml") {
        return false;
    }
    if matches!(
        name,
        "annotation-xml"
            | "color-profile"
            | "font-face"
            | "font-face-src"
            | "font-face-uri"
            | "font-face-format"
            | "font-face-name"
            | "missing-glyph"
    ) {
        return false;
    }
    name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}
