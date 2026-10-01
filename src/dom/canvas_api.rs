//! The `<canvas>` element's DOM surface — HTML §4.12.5.

use crate::canvas::{CanvasContextMode, ImageBitmap};
use crate::types::Document;

// ─── Canvas ─────────────────────────────────────────────────────────────────
//
// `canvas.getContext("2d")` — HTML §4.12.5.
//
// A page reaches a canvas the way it reaches anything else: it looks the
// element up, asks it for a context, and draws. Nothing above this layer names
// an engine, which is the same contract the rest of this file keeps — the
// identical surface exists on `vybe_widgets`, so which one is compiled in stays
// a build-time choice.

impl Document {
    /// Synchronous encoding core for `HTMLCanvasElement.toBlob()`.
    /// The browser-facing callback is scheduled by the caller.
    pub fn canvas_to_blob(&mut self, id: u32, mime: &str, quality: Option<f32>) -> Option<Vec<u8>> {
        self.canvas_to_blob_checked(id, mime, quality)
            .ok()
            .flatten()
    }

    pub fn canvas_to_blob_checked(
        &mut self,
        id: u32,
        mime: &str,
        quality: Option<f32>,
    ) -> Result<Option<Vec<u8>>, &'static str> {
        if !self.ensure_canvas_bitmap(id) {
            return Ok(None);
        }
        if !self.canvas_surfaces.origin_clean(id) {
            return Err("SecurityError");
        }
        let Some(node) = self.find_webcore_mut(id) else {
            return Ok(None);
        };
        let Some(size) = tiny_skia::IntSize::from_wh(node.image_data_width, node.image_data_height)
        else {
            return Ok(None);
        };
        let Some(mut pixmap) = node
            .image_data
            .as_ref()
            .and_then(|data| tiny_skia::Pixmap::from_vec(data.as_ref().clone(), size))
        else {
            return Ok(None);
        };
        let canvas = crate::canvas::TinySkiaCanvas::new(&mut pixmap);
        Ok(crate::canvas::Canvas::to_blob(
            &canvas,
            crate::canvas::canvas_encoder_mime(mime),
            quality,
        ))
    }

    pub fn canvas_to_data_url(
        &mut self,
        id: u32,
        mime: &str,
        quality: Option<f32>,
    ) -> Option<String> {
        self.canvas_to_data_url_checked(id, mime, quality)
            .ok()
            .flatten()
    }

    pub fn canvas_to_data_url_checked(
        &mut self,
        id: u32,
        mime: &str,
        quality: Option<f32>,
    ) -> Result<Option<String>, &'static str> {
        if !self.ensure_canvas_bitmap(id) {
            return Ok(None);
        }
        let mime = crate::canvas::canvas_encoder_mime(mime);
        let Some(bytes) = self.canvas_to_blob_checked(id, mime, quality)? else {
            return Ok(Some("data:,".to_string()));
        };
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(Some(format!("data:{mime};base64,{encoded}")))
    }

    /// `canvas.getContext("2d")` — HTML §4.12.5.1.
    ///
    /// Answers whether `id` is a `<canvas>` that now has a 2D context,
    /// allocating its bitmap if it does not have one yet. An element built by
    /// `createElement("canvas")` has never been through the parser, so this is
    /// where it gets the transparent-black bitmap the spec says a canvas
    /// starts with.
    ///
    /// There is no context OBJECT to return here on purpose. The context's
    /// identity is the element — every call arrives naming the node — so a
    /// handle would be a second name for something that already has one.
    pub fn get_context_2d(&mut self, id: u32) -> bool {
        self.ensure_canvas_bitmap(id)
            && self
                .canvas_surfaces
                .select_mode(id, CanvasContextMode::TwoD)
    }

    /// `canvas.getContext("bitmaprenderer")` shares the element bitmap with
    /// the 2D context, but the first successful context type is permanent.
    pub fn get_bitmap_context(&mut self, id: u32) -> bool {
        self.ensure_canvas_bitmap(id)
            && self
                .canvas_surfaces
                .select_mode(id, CanvasContextMode::BitmapRenderer)
    }

    /// `ImageBitmapRenderingContext.transferFromImageBitmap()` consumes its
    /// source. Passing `None` clears the output bitmap to transparent black.
    pub fn canvas_transfer_from_image_bitmap(
        &mut self,
        id: u32,
        image: Option<&mut ImageBitmap>,
    ) -> Result<(), &'static str> {
        if !self.get_bitmap_context(id) {
            return Err("InvalidStateError");
        }
        let (width, height, pixels, origin_clean) = if let Some(image) = image {
            image.take_pixels().ok_or("InvalidStateError")?
        } else {
            let node = self.find_webcore_mut(id).ok_or("InvalidStateError")?;
            let width = node.image_width;
            let height = node.image_height;
            let len = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or("RangeError")?;
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(len).map_err(|_| "RangeError")?;
            pixels.resize(len, 0);
            (width, height, pixels, true)
        };
        let node = self.find_webcore_mut(id).ok_or("InvalidStateError")?;
        node.image_data_width = width;
        node.image_data_height = height;
        node.image_data = Some(std::sync::Arc::new(pixels));
        self.canvas_surfaces.set_origin_clean(id, origin_clean);
        Ok(())
    }

    /// Give `id` the bitmap a `<canvas>` element is defined to have, and say
    /// whether it is a canvas at all.
    ///
    /// §4.12.5 gives the ELEMENT the bitmap, not the context — a `<canvas>`
    /// has one from the moment it exists, and `getContext` hands out a way to
    /// draw on what is already there. So this is what `getContext` does and
    /// also what drawing does, rather than two paths that could disagree about
    /// whether a surface exists. The parser allocates the same buffer for a
    /// parsed `<canvas>`; an element from `createElement("canvas")` has never
    /// been through it, and gets its bitmap here.
    fn ensure_canvas_bitmap(&mut self, id: u32) -> bool {
        let bitmap_renderer =
            self.canvas_surfaces.mode(id) == Some(CanvasContextMode::BitmapRenderer);
        let Some(node) = self.find_webcore_mut(id) else {
            return false;
        };
        if node.tag != "canvas" {
            return false;
        }
        // §4.12.5: a canvas with no `width`/`height` attribute is 300 × 150.
        if node.image_width == 0 && !node.attributes.contains_key("width") {
            node.image_width = 300;
        }
        if node.image_height == 0 && !node.attributes.contains_key("height") {
            node.image_height = 150;
        }
        if bitmap_renderer && node.image_data.is_some() {
            return true;
        }
        node.image_data_width = node.image_width;
        node.image_data_height = node.image_height;
        let Some(want) = (node.image_width as usize)
            .checked_mul(node.image_height as usize)
            .and_then(|n| n.checked_mul(4))
        else {
            return false;
        };
        match node.image_data {
            Some(ref data) if data.len() == want => {}
            // Transparent black, which is what the spec initialises the
            // bitmap to — and what a zeroed RGBA buffer already is.
            _ => {
                let mut pixels = Vec::new();
                if pixels.try_reserve_exact(want).is_err() {
                    return false;
                }
                pixels.resize(want, 0);
                node.image_data = Some(std::sync::Arc::new(pixels));
            }
        }
        true
    }

    /// Draw on the canvas `id` through the WHATWG 2D context.
    ///
    /// The context state persists across calls; see `canvas::CanvasSurfaces`.
    /// `None` when `id` is not a `<canvas>` — which is the only thing that can
    /// fail here, because the element owns its bitmap and
    /// [`ensure_canvas_bitmap`](Self::ensure_canvas_bitmap) is the same
    /// allocation `getContext` performs.
    pub fn with_canvas_2d<R>(
        &mut self,
        id: u32,
        f: impl FnOnce(&mut dyn crate::canvas::Canvas) -> R,
    ) -> Option<R> {
        if !self.get_context_2d(id) {
            return None;
        }
        // The bitmap is MOVED out of the element and back, so the element and
        // the surface store are never borrowed at the same time — and a canvas
        // is never copied to be drawn on.
        let (mut pixels, w, h) = {
            let node = self.find_webcore_mut(id)?;
            (
                std::sync::Arc::unwrap_or_clone(node.image_data.take()?),
                node.image_width,
                node.image_height,
            )
        };
        let out = self.canvas_surfaces.with_context_syntax(
            id,
            &mut pixels,
            w,
            h,
            crate::canvas::browser_canvas_syntax(),
            f,
        );
        if let Some(node) = self.find_webcore_mut(id) {
            node.image_data_width = w;
            node.image_data_height = h;
            node.image_data = Some(std::sync::Arc::new(pixels));
        }
        out
    }

    /// `canvas.width` / `canvas.height` — HTML §4.12.5.
    ///
    /// Assigning either one **reinitialises the bitmap to transparent black
    /// and resets the drawing state**, and the spec is explicit that this
    /// happens even when the value assigned is the one already there. So this
    /// is not a resize that preserves content: `canvas.width = canvas.width`
    /// is the documented way a page clears a canvas, and an implementation
    /// that kept the pixels would break it silently.
    pub fn set_canvas_size(&mut self, id: u32, width: u32, height: u32) {
        let Some(len) = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
        else {
            return;
        };
        let mut pixels = Vec::new();
        if pixels.try_reserve_exact(len).is_err() {
            return;
        }
        pixels.resize(len, 0);
        let Some(node) = self.find_webcore_mut(id) else {
            return;
        };
        if node.tag != "canvas" {
            return;
        }
        node.image_width = width;
        node.image_height = height;
        node.image_data_width = width;
        node.image_data_height = height;
        node.image_data = Some(std::sync::Arc::new(pixels));
        node.attributes.insert("width", width.to_string());
        node.attributes.insert("height", height.to_string());
        self.canvas_surfaces.reset(id);
    }
}

#[cfg(test)]
mod tests {
    use crate::canvas::{Color, Image, OffscreenCanvas};

    #[test]
    fn tainted_canvas_export_throws_until_dimension_reset() {
        let mut document =
            crate::load_html(r#"<canvas id="c" width="1" height="1"></canvas>"#, 800.0);
        let id = document.get_element_by_id("c").unwrap();
        let image = Image::from_rgba(1, 1, vec![255, 0, 0, 255]).with_origin_clean(false);
        document
            .with_canvas_2d(id, |ctx| ctx.draw_image(&image, 0.0, 0.0, 1.0, 1.0))
            .unwrap();
        assert_eq!(
            document.canvas_to_blob_checked(id, "image/png", None),
            Err("SecurityError")
        );
        assert_eq!(
            document.canvas_to_data_url_checked(id, "image/png", None),
            Err("SecurityError")
        );
        document.set_canvas_size(id, 1, 1);
        assert!(
            document
                .canvas_to_blob_checked(id, "image/png", None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn context_type_is_locked_even_after_resize() {
        let mut document =
            crate::load_html(r#"<canvas id="c" width="2" height="1"></canvas>"#, 800.0);
        let id = document.get_element_by_id("c").unwrap();
        assert!(document.get_bitmap_context(id));
        assert!(!document.get_context_2d(id));
        assert!(document.with_canvas_2d(id, |_| ()).is_none());
        document.set_canvas_size(id, 2, 1);
        assert!(document.get_bitmap_context(id));
        assert!(!document.get_context_2d(id));
    }

    #[test]
    fn bitmap_renderer_consumes_image_and_exports_transferred_pixels() {
        let mut source = OffscreenCanvas::new(2, 1).unwrap();
        source
            .with_context_2d(|ctx| {
                ctx.set_fill_color(Color::rgb(255, 0, 0));
                ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            })
            .unwrap();
        let mut image = source.transfer_to_image_bitmap().unwrap();
        let mut document =
            crate::load_html(r#"<canvas id="c" width="2" height="1"></canvas>"#, 800.0);
        let id = document.get_element_by_id("c").unwrap();
        document
            .canvas_transfer_from_image_bitmap(id, Some(&mut image))
            .unwrap();
        assert_eq!(image.width(), 0);
        assert_eq!(image.height(), 0);
        assert_eq!(
            &document.get_node(id).unwrap().image_data.as_ref().unwrap()[..4],
            &[255, 0, 0, 255]
        );
        let blob = document.canvas_to_blob(id, "image/png", None).unwrap();
        let decoded = image::load_from_memory(&blob).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert!(
            document
                .canvas_transfer_from_image_bitmap(id, Some(&mut image))
                .is_err()
        );
        document
            .canvas_transfer_from_image_bitmap(id, None)
            .unwrap();
        assert_eq!(
            &document.get_node(id).unwrap().image_data.as_ref().unwrap()[..4],
            &[0, 0, 0, 0]
        );
    }

    #[test]
    fn explicit_zero_width_is_not_replaced_with_the_default() {
        let mut document =
            crate::load_html(r#"<canvas id="c" width="0" height="8"></canvas>"#, 800.0);
        let id = document.get_element_by_id("c").unwrap();
        assert!(document.get_context_2d(id));
        let node = document.get_node(id).unwrap();
        assert_eq!((node.image_width, node.image_height), (0, 8));
        assert!(node.image_data.as_ref().unwrap().is_empty());
        let read = document
            .with_canvas_2d(id, |ctx| ctx.get_image_data(0, 0, 1, 1))
            .unwrap()
            .unwrap();
        assert_eq!(read.data, vec![0, 0, 0, 0]);
        assert_eq!(
            document.canvas_to_data_url(id, "image/png", None),
            Some("data:,".into())
        );
    }

    #[test]
    fn canvas_export_uses_png_for_an_unsupported_type() {
        let mut document =
            crate::load_html(r#"<canvas id="c" width="2" height="1"></canvas>"#, 800.0);
        let id = document.get_element_by_id("c").unwrap();
        document
            .with_canvas_2d(id, |ctx| {
                ctx.set_fill_color(crate::canvas::Color::rgb(255, 0, 0));
                ctx.fill_rect(0.0, 0.0, 2.0, 1.0);
            })
            .unwrap();
        let url = document
            .canvas_to_data_url(id, "image/unsupported", None)
            .unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let bytes = document
            .canvas_to_blob(id, "image/unsupported", None)
            .unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }
}
