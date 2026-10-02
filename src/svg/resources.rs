//! SVG-related external resource loading.

use crate::html::load_image_from_src;
use crate::types::WebCore;

/// Post-cascade pass: load background and mask images referenced by CSS.
pub fn load_background_images(node: &mut WebCore, base_url: &str) {
    if node.bg_image_data.is_none() && !node.style.background_image_url.is_empty() {
        let url = node.style.background_image_url.clone();
        if let Some(decoded) = crate::html::load_decoded_image_from_src(&url, base_url) {
            crate::html::set_decoded_bg_image_on_node(node, decoded);
        }
    }
    let layers: Vec<(usize, String)> = node
        .style
        .rare()
        .additional_background_layers
        .iter()
        .enumerate()
        .filter_map(|(idx, layer)| {
            (!layer.image_url.is_empty()).then(|| (idx, layer.image_url.clone()))
        })
        .collect();
    for (layer_index, url) in layers {
        let loaded = node
            .additional_bg_images
            .get(layer_index)
            .and_then(|image| image.as_ref())
            .is_some();
        if !loaded && let Some(decoded) = crate::html::load_decoded_image_from_src(&url, base_url) {
            crate::html::set_decoded_bg_image_layer_on_node(node, layer_index, decoded);
        }
    }
    if node.mask_images.as_ref().and_then(|images| {
        images.get_for_source(0, node.style.mask_source_key(0)?)
    }).is_none()
        && !node.style.rare().mask_image_url.is_empty()
    {
        let url = node.style.rare().mask_image_url.clone();
        if let Some((data, w, h)) = load_image_from_src(&url, base_url) {
            let resolution = node
                .style
                .rare()
                .mask_image_set_source
                .as_deref()
                .and_then(|source| {
                    crate::css::property_defs::image_set_resolution_for_url(
                        source, &url, base_url,
                    )
                })
                .unwrap_or(1.0);
            let image = crate::types::DecodedMaskImage {
                data: std::sync::Arc::new(data),
                width: w,
                height: h,
                resolution,
            };
            let source_key = node.style.mask_source_key(0).unwrap_or_default().to_string();
            std::sync::Arc::make_mut(node.mask_images.get_or_insert_with(Default::default))
                .set_with_source(0, image, source_key);
        }
    }
    for (index, source) in node.style.rare().additional_mask_images.clone().into_iter().enumerate() {
        if source.url.is_empty()
            || node.mask_images.as_ref().and_then(|images| {
                images.get_for_source(index + 1, node.style.mask_source_key(index + 1)?)
            }).is_some()
        {
            continue;
        }
        let url = source.url_for_dpr(1.0);
        if let Some(decoded) = crate::html::load_decoded_image_from_src(&url, base_url) {
            let _ = crate::html::set_decoded_mask_image_layer_for_url_on_node(
                node, index + 1, decoded, &url, base_url,
            );
        }
    }
    for child in &mut node.children {
        load_background_images(child, base_url);
    }
}
