//! SVG-related external resource loading.

use crate::html::load_image_from_src;
use crate::types::WebCore;

/// Post-cascade pass: load background and mask images referenced by CSS.
pub fn load_background_images(node: &mut WebCore, base_url: &str) {
    if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
        let url = node.style.background_image_url.clone();
        if let Some((data, w, h)) = load_image_from_src(&url, base_url) {
            node.bg_image_data = Some(std::sync::Arc::new(data));
            node.bg_image_width = w;
            node.bg_image_height = h;
        }
    }
    if node.mask_image_data.is_none() && !node.style.rare().mask_image_url.is_empty() {
        let url = node.style.rare().mask_image_url.clone();
        if let Some((data, w, h)) = load_image_from_src(&url, base_url) {
            node.mask_image_data = Some(std::sync::Arc::new(data));
            node.mask_image_width = w;
            node.mask_image_height = h;
        }
    }
    for child in &mut node.children {
        load_background_images(child, base_url);
    }
}
