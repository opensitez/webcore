//! HTML progress and meter numeric states shared with native painting.

use crate::html::forms::{MeterState, ProgressState};
use crate::types::Document;

impl Document {
    pub fn progress_state(&self, id: u32) -> Option<ProgressState> {
        let node = self.get_node(id).filter(|node| node.tag == "progress")?;
        Some(crate::html::forms::progress_state(|name| {
            node.attributes.get(name).map(String::as_str)
        }))
    }

    pub fn progress_position(&self, id: u32) -> Option<f64> {
        self.progress_state(id).map(ProgressState::position)
    }

    pub fn meter_state(&self, id: u32) -> Option<MeterState> {
        let node = self.get_node(id).filter(|node| node.tag == "meter")?;
        Some(crate::html::forms::meter_state(|name| {
            node.attributes.get(name).map(String::as_str)
        }))
    }
}
