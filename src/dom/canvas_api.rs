//! The `<canvas>` element's DOM surface — HTML §4.12.5.

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
        let Some(node) = self.find_webcore_mut(id) else {
            return false;
        };
        if node.tag != "canvas" {
            return false;
        }
        // §4.12.5: a canvas with no `width`/`height` attribute is 300 × 150.
        if node.image_width == 0 || node.image_height == 0 {
            node.image_width = 300;
            node.image_height = 150;
        }
        let want = (node.image_width as usize) * (node.image_height as usize) * 4;
        match node.image_data {
            Some(ref data) if data.len() == want => {}
            // Transparent black, which is what the spec initialises the
            // bitmap to — and what a zeroed RGBA buffer already is.
            _ => node.image_data = Some(std::sync::Arc::new(vec![0u8; want])),
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
        if !self.ensure_canvas_bitmap(id) {
            return None;
        }
        // The bitmap is MOVED out of the element and back, so the element and
        // the surface store are never borrowed at the same time — and a canvas
        // is never copied to be drawn on.
        let (mut pixels, w, h) = {
            let node = self.find_webcore_mut(id)?;
            (
                node.image_data.take()?.as_ref().clone(),
                node.image_width,
                node.image_height,
            )
        };
        let out = self.canvas_surfaces.with_context(id, &mut pixels, w, h, f);
        if let Some(node) = self.find_webcore_mut(id) {
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
        let Some(node) = self.find_webcore_mut(id) else {
            return;
        };
        if node.tag != "canvas" {
            return;
        }
        node.image_width = width;
        node.image_height = height;
        node.image_data = Some(std::sync::Arc::new(vec![
            0u8;
            (width as usize)
                * (height as usize)
                * 4
        ]));
        node.attributes.insert("width", width.to_string());
        node.attributes.insert("height", height.to_string());
        self.canvas_surfaces.reset(id);
    }
}
