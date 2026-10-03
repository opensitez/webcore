use std::sync::Arc;

use crate::types::{CustomElement, CustomElementError, Document};

impl Document {
    /// Register a custom-element definition and upgrade matching parsed nodes.
    pub fn define_custom_element(
        &mut self,
        name: &str,
        definition: impl CustomElement + 'static,
    ) -> Result<(), CustomElementError> {
        self.custom_elements.insert(name.to_string(), Arc::new(definition))?;
        for id in self.query_selector_all(name) {
            self.upgrade_custom_element(id);
        }
        let pending = self.pending_nodes.keys().copied().collect::<Vec<_>>();
        for id in pending {
            if self.tag_name(id) == Some(name) {
                self.upgrade_custom_element(id);
            }
        }
        Ok(())
    }

    /// Upgrade one element, including its existing observed attributes.
    pub fn upgrade_custom_element(&mut self, id: u32) {
        let Some(name) = self.tag_name(id).map(str::to_owned) else {
            return;
        };
        let Some(definition) = self.custom_elements.get(&name) else {
            return;
        };
        if !self.custom_elements.mark_upgraded(id) {
            return;
        }
        definition.construct(self, id);
        for &attribute in definition.observed_attributes() {
            if let Some(value) = self.get_attribute(id, attribute) {
                definition.attribute_changed_callback(self, id, attribute, None, Some(&value));
            }
        }
        if self.is_connected(id) {
            definition.connected_callback(self, id);
        }
    }

    /// Upgrade matching elements in a subtree without connecting a detached root.
    pub fn upgrade_custom_elements(&mut self, root: u32) {
        for id in self.custom_subtree_ids(root) {
            self.upgrade_custom_element(id);
        }
    }

    pub(crate) fn custom_subtree_ids(&self, root: u32) -> Vec<u32> {
        let mut result = vec![root];
        let mut cursor = 0;
        while cursor < result.len() {
            result.extend(self.child_nodes(result[cursor]));
            cursor += 1;
        }
        result
    }

    pub(crate) fn custom_connected(&mut self, nodes: &[u32]) {
        for &id in nodes {
            let was_upgraded = self.custom_elements.is_upgraded(id);
            self.upgrade_custom_element(id);
            let definition = self
                .tag_name(id)
                .and_then(|name| self.custom_elements.get(name));
            if let Some(definition) = definition {
                // Upgrade itself already delivered the initial connection.
                // Existing elements receive a new callback when reinserted.
                if was_upgraded {
                    definition.connected_callback(self, id);
                }
            }
        }
    }

    pub(crate) fn custom_disconnected(&mut self, nodes: &[u32]) {
        for &id in nodes {
            if !self.custom_elements.is_upgraded(id) {
                continue;
            }
            let definition = self
                .tag_name(id)
                .and_then(|name| self.custom_elements.get(name));
            if let Some(definition) = definition {
                definition.disconnected_callback(self, id);
            }
        }
    }

    pub(crate) fn custom_attribute_changed(
        &mut self,
        id: u32,
        name: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
    ) {
        if !self.custom_elements.is_upgraded(id) {
            return;
        }
        let definition = self
            .tag_name(id)
            .and_then(|tag| self.custom_elements.get(tag));
        if let Some(definition) = definition {
            if definition.observed_attributes().contains(&name) {
                definition.attribute_changed_callback(self, id, name, old_value, new_value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl CustomElement for Recorder {
        fn observed_attributes(&self) -> &[&str] {
            &["data-value"]
        }

        fn construct(&self, _: &mut Document, _: u32) {
            self.0.lock().unwrap().push("construct".into());
        }

        fn connected_callback(&self, _: &mut Document, _: u32) {
            self.0.lock().unwrap().push("connected".into());
        }

        fn disconnected_callback(&self, _: &mut Document, _: u32) {
            self.0.lock().unwrap().push("disconnected".into());
        }

        fn attribute_changed_callback(
            &self,
            _: &mut Document,
            _: u32,
            name: &str,
            old: Option<&str>,
            new: Option<&str>,
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{name}:{old:?}->{new:?}"));
        }
    }

    #[test]
    fn late_definition_upgrades_existing_markup_in_lifecycle_order() {
        let mut document = crate::load_html(
            "<body><x-meter data-value='one'></x-meter></body>",
            800.0,
        );
        let events = Arc::new(Mutex::new(Vec::new()));
        document
            .define_custom_element("x-meter", Recorder(events.clone()))
            .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["construct", "data-value:None->Some(\"one\")", "connected"]
        );
    }

    #[test]
    fn created_element_connects_once_and_observed_attribute_changes() {
        let mut document = crate::load_html("<body></body>", 800.0);
        let events = Arc::new(Mutex::new(Vec::new()));
        document
            .define_custom_element("x-meter", Recorder(events.clone()))
            .unwrap();
        let node = document.create_element("x-meter");
        document.set_attribute(node, "data-value", "one");
        assert_eq!(
            *events.lock().unwrap(),
            ["construct", "data-value:None->Some(\"one\")"]
        );
        let body = document.query_selector("body").unwrap();
        document.append_child(body, node);
        document.set_attribute(node, "data-value", "two");
        document.remove_child(node);
        document.append_child(body, node);
        assert_eq!(
            *events.lock().unwrap(),
            [
                "construct",
                "data-value:None->Some(\"one\")",
                "connected",
                "data-value:Some(\"one\")->Some(\"two\")",
                "disconnected",
                "connected"
            ]
        );
    }

    #[test]
    fn definition_names_and_duplicates_are_checked() {
        let mut document = Document::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(
            document.define_custom_element("meter", Recorder(events.clone())),
            Err(CustomElementError::InvalidName)
        );
        document
            .define_custom_element("x-meter", Recorder(events.clone()))
            .unwrap();
        assert_eq!(
            document.define_custom_element("x-meter", Recorder(events)),
            Err(CustomElementError::AlreadyDefined)
        );
    }

    #[test]
    fn nested_elements_disconnect_and_reconnect_with_their_parent() {
        let mut document = crate::load_html("<body><div id='host'><x-meter></x-meter></div></body>", 800.0);
        let events = Arc::new(Mutex::new(Vec::new()));
        document.define_custom_element("x-meter", Recorder(events.clone())).unwrap();
        let host = document.query_selector("#host").unwrap();
        let body = document.query_selector("body").unwrap();
        document.remove_child(host);
        document.append_child(body, host);
        assert_eq!(*events.lock().unwrap(), ["construct", "connected", "disconnected", "connected"]);
    }

    #[test]
    fn removing_an_observed_attribute_reports_old_and_null() {
        let mut document = crate::load_html("<body><x-meter data-value='one'></x-meter></body>", 800.0);
        let events = Arc::new(Mutex::new(Vec::new()));
        document.define_custom_element("x-meter", Recorder(events.clone())).unwrap();
        let meter = document.query_selector("x-meter").unwrap();
        document.remove_attribute(meter, "data-value");
        assert_eq!(events.lock().unwrap().last().unwrap(), "data-value:Some(\"one\")->None");
    }
}
