//! Per-element `<canvas>` drawing state, and the fonts canvas text is drawn
//! with.
//!
//! A `<canvas>` is reached from a page the way every element is —
//! `getElementById`, then `getContext("2d")`, then draw. Each of those draw
//! calls arrives on its own, so something has to hold the context's state
//! between them. This is that something: one [`CanvasState`] per canvas
//! element, keyed by node id.
//!
//! **The pixels are not here.** They live on the element, in
//! `WebCore::image_data`, which the parser already allocates for a `<canvas>`
//! and the display-list builder already knows how to paint. Keeping one
//! bitmap rather than two is what makes `getImageData` and a rendered frame
//! agree by construction, and the alternative — recording the calls and
//! replaying them later — cannot answer `getImageData`, `toBlob` or
//! `isPointInPath` at all, because at the moment the page asks there are no
//! pixels to read.

use std::collections::HashMap;

use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{IntSize, Pixmap};

use super::{Canvas, CanvasState, TinySkiaCanvas};

/// The drawing state of every `<canvas>` in one document.
#[derive(Default)]
pub struct CanvasSurfaces {
    /// node id → the context state that survives between calls. A canvas with
    /// no entry has never been drawn to, which is indistinguishable from one
    /// whose state is all defaults — so entries are made on demand.
    states: HashMap<u32, CanvasState>,
    /// Fonts for canvas text, created on the first `fillText`.
    ///
    /// Separate from `Renderer::font_system` because a canvas is drawn when
    /// the PAGE calls it and the renderer's fonts exist only while a frame is
    /// being painted — reaching for them at call time would find nothing, and
    /// `fillText` would silently draw nothing at all, which is the failure
    /// that hides longest.
    ///
    /// Built lazily: constructing a `FontSystem` enumerates the system fonts,
    /// and a document with no canvas text should not pay for that.
    fonts: Option<Box<(FontSystem, SwashCache)>>,
}

// Note for anyone extending this: an entry OUTLIVES its element. Nothing here
// is notified when a node is removed from the tree, so a page that creates and
// discards canvases accumulates one `CanvasState` each — small, but it can
// carry a pixmap-sized clip `Mask`. Removing an entry needs a hook on node
// destruction, which the DOM does not have yet.

impl CanvasSurfaces {
    /// Run `f` against the canvas for `node_id`, over `pixels`.
    ///
    /// `pixels` is the element's own bitmap, moved in and moved back out —
    /// `Pixmap` owns its buffer, so lending it to tiny-skia and taking it back
    /// costs two moves and never copies the surface.
    ///
    /// `None` when the buffer does not match the declared size, which would
    /// mean the element's bitmap and its `width`/`height` had drifted apart.
    pub fn with_context<R>(
        &mut self,
        node_id: u32,
        pixels: &mut Vec<u8>,
        width: u32,
        height: u32,
        f: impl FnOnce(&mut dyn Canvas) -> R,
    ) -> Option<R> {
        let size = IntSize::from_wh(width, height)?;
        if pixels.len() != (width as usize) * (height as usize) * 4 {
            return None;
        }
        let mut pixmap = Pixmap::from_vec(std::mem::take(pixels), size)?;

        let saved = self.states.remove(&node_id).unwrap_or_default();
        let fonts = self
            .fonts
            .get_or_insert_with(|| Box::new((FontSystem::new(), SwashCache::new())));
        let (font_system, swash_cache) = &mut **fonts;

        let mut canvas =
            TinySkiaCanvas::resume(&mut pixmap, saved, Some((font_system, swash_cache)));
        let out = f(&mut canvas);
        self.states.insert(node_id, canvas.suspend());

        *pixels = pixmap.take();
        Some(out)
    }

    /// Drop the drawing state for one canvas, so the next call starts from the
    /// defaults.
    ///
    /// [HTML §4.12.5.1](https://html.spec.whatwg.org/multipage/canvas.html#the-canvas-element)
    /// requires this whenever `width` or `height` is assigned — **even when
    /// the value does not change** — and for `reset()`. The bitmap is cleared
    /// by the caller that owns it; this is the other half.
    pub fn reset(&mut self, node_id: u32) {
        self.states.remove(&node_id);
    }
}

impl std::fmt::Debug for CanvasSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CanvasSurfaces")
            .field("canvases", &self.states.len())
            .field("fonts_loaded", &self.fonts.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::canvas::{Canvas, Color, Font};

    /// The pixel at (x, y) of a canvas element's bitmap, as premultiplied RGBA.
    fn pixel(doc: &crate::types::Document, id: u32, x: u32, y: u32) -> [u8; 4] {
        let node = doc.get_node(id).expect("node");
        let data = node.image_data.as_ref().expect("bitmap");
        let i = (y as usize * node.image_width as usize + x as usize) * 4;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    }

    fn canvas_doc(markup: &str) -> (crate::types::Document, u32) {
        let doc = crate::load_html(markup, 800.0);
        let id = doc.get_element_by_id("c").expect("canvas element");
        (doc, id)
    }

    #[test]
    fn a_page_draws_by_id_the_way_a_browser_does() {
        // The whole point, in one test: find the element, ask for a context,
        // draw. No engine is named and no handle is threaded through.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="40" height="20"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 10.0, 10.0);
        })
        .expect("canvas");
        assert_eq!(pixel(&doc, id, 5, 5), [255, 0, 0, 255]);
        // Outside the rect is still the transparent black a canvas starts as.
        assert_eq!(pixel(&doc, id, 30, 15), [0, 0, 0, 0]);
    }

    #[test]
    fn the_drawing_state_survives_between_two_calls() {
        // The reason `CanvasState` exists. `fillStyle = red` and `fillRect(…)`
        // reach the engine as two separate trips; a context rebuilt for each
        // would paint the second one black.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="20" height="20"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| ctx.set_fill_color(Color::rgb(0, 0, 255)))
            .expect("canvas");
        doc.with_canvas_2d(id, |ctx| ctx.fill_rect(0.0, 0.0, 20.0, 20.0))
            .expect("canvas");
        assert_eq!(pixel(&doc, id, 10, 10), [0, 0, 255, 255]);
    }

    #[test]
    fn the_transform_and_the_clip_survive_too() {
        // `state` is not the only retained field — a page that calls
        // `translate` then draws, or `clip` then draws, is just as common.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="40" height="40"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(0, 255, 0));
            ctx.translate(20.0, 0.0);
            ctx.rect(0.0, 0.0, 10.0, 10.0);
            ctx.clip();
        })
        .expect("canvas");
        doc.with_canvas_2d(id, |ctx| ctx.fill_rect(0.0, 0.0, 40.0, 40.0))
            .expect("canvas");
        // Translated into the clip: painted.
        assert_eq!(pixel(&doc, id, 25, 5), [0, 255, 0, 255]);
        // Outside the clip: untouched, even though the rect covered it.
        assert_eq!(pixel(&doc, id, 5, 5), [0, 0, 0, 0]);
    }

    #[test]
    fn canvas_text_actually_draws() {
        // A canvas whose `fillText` silently does nothing would pass every
        // test that does not sample glyph pixels. The fonts a canvas draws
        // with are its own, because the renderer's exist only while a frame is
        // being painted and a page draws whenever it likes.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="200" height="60"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(0, 0, 0));
            ctx.set_font(&Font::new("sans-serif", 48.0));
            ctx.fill_text("HHHH", 5.0, 45.0);
        })
        .expect("canvas");
        let node = doc.get_node(id).expect("node");
        let data = node.image_data.as_ref().expect("bitmap");
        let inked = data.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(inked > 200, "fillText drew {inked} opaque pixels");
    }

    #[test]
    fn assigning_the_size_clears_the_bitmap_and_the_state() {
        // HTML §4.12.5 — `canvas.width = canvas.width` is how a page clears a
        // canvas, so this must reinitialise even when the value is unchanged.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="20" height="20"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 20.0, 20.0);
        })
        .expect("canvas");
        assert_eq!(pixel(&doc, id, 10, 10), [255, 0, 0, 255]);

        doc.set_canvas_size(id, 20, 20);
        assert_eq!(pixel(&doc, id, 10, 10), [0, 0, 0, 0], "bitmap not cleared");

        // The fill colour went with it: this rect paints the default black,
        // not the red that was set before the assignment.
        doc.with_canvas_2d(id, |ctx| ctx.fill_rect(0.0, 0.0, 20.0, 20.0))
            .expect("canvas");
        assert_eq!(pixel(&doc, id, 10, 10), [0, 0, 0, 255], "state not reset");
    }

    #[test]
    fn the_element_owns_the_bitmap_so_drawing_needs_no_prior_get_context() {
        // §4.12.5 gives the BITMAP to the element, not to the context. The
        // parser already allocates one for a parsed `<canvas>`, so a draw that
        // refused unless `getContext` had been called first would be refusing
        // over a surface that demonstrably exists — two paths disagreeing
        // about the same fact.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="20" height="20"></canvas>"#);
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(1, 2, 3));
            ctx.fill_rect(0.0, 0.0, 20.0, 20.0);
        })
        .expect("a parsed canvas can be drawn on");
        assert_eq!(pixel(&doc, id, 10, 10), [1, 2, 3, 255]);
    }

    #[test]
    fn setting_the_width_attribute_clears_the_canvas() {
        // The `setAttribute` route to the same value as `canvas.width`. It has
        // to reinitialise exactly as the IDL attribute does, or a page gets
        // different behaviour depending on which spelling it used.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="20" height="20"></canvas>"#);
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 20.0, 20.0);
        })
        .expect("canvas");
        assert_eq!(pixel(&doc, id, 10, 10), [255, 0, 0, 255]);

        doc.set_attribute(id, "width", "30");
        let node = doc.get_node(id).expect("node");
        assert_eq!((node.image_width, node.image_height), (30, 20));
        assert_eq!(pixel(&doc, id, 10, 10), [0, 0, 0, 0], "bitmap not cleared");

        // A non-size attribute leaves the drawing alone.
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(0, 255, 0));
            ctx.fill_rect(0.0, 0.0, 30.0, 20.0);
        })
        .expect("canvas");
        doc.set_attribute(id, "title", "chart");
        assert_eq!(pixel(&doc, id, 10, 10), [0, 255, 0, 255]);
    }

    #[test]
    fn get_context_2d_answers_for_the_element_it_is_asked_about() {
        let doc_src = r#"<canvas id="c" width="20" height="20"></canvas><div id="d"></div>"#;
        let (mut doc, id) = canvas_doc(doc_src);
        assert!(doc.get_context_2d(id));
        let div = doc.get_element_by_id("d").expect("div");
        assert!(!doc.get_context_2d(div), "a <div> has no 2D context");
        assert!(doc.with_canvas_2d(div, |_| ()).is_none());
    }

    #[test]
    fn a_created_canvas_gets_the_specs_default_bitmap() {
        // `createElement("canvas")` never goes through the parser, so this is
        // where it picks up the 300 × 150 the spec gives a canvas with no
        // width/height attribute.
        let mut doc = crate::load_html("<div id='host'></div>", 800.0);
        let host = doc.get_element_by_id("host").expect("host");
        let id = doc.create_element("canvas");
        doc.append_child(host, id);
        assert!(doc.get_context_2d(id));
        let node = doc.get_node(id).expect("node");
        assert_eq!((node.image_width, node.image_height), (300, 150));
        assert_eq!(
            node.image_data.as_ref().map(|d| d.len()),
            Some(300 * 150 * 4)
        );

        // And it draws, which is the thing the size is for.
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(12, 34, 56));
            ctx.fill_rect(0.0, 0.0, 300.0, 150.0);
        })
        .expect("canvas");
        assert_eq!(pixel(&doc, id, 150, 75), [12, 34, 56, 255]);
    }

    #[test]
    fn what_the_page_drew_reaches_the_display_list() {
        // The last hop. Painting a canvas is painting its bitmap, so the
        // builder emits the same `Image` command it emits for an `<img>`.
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="40" height="20"></canvas>"#);
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 40.0, 20.0);
        })
        .expect("canvas");

        let list =
            crate::renderer::display_list_builder::build_display_list(&doc.root, 800.0, 600.0);
        let painted = list.commands.iter().any(|cmd| match cmd {
            crate::renderer::display_list::PaintCmd::Image { data, .. } => {
                let (bytes, w, h) = match data {
                    crate::renderer::display_list::ImageRef::Owned(d, w, h) => {
                        (d.as_slice(), *w, *h)
                    }
                    crate::renderer::display_list::ImageRef::Shared(d, w, h) => {
                        (d.as_slice(), *w, *h)
                    }
                };
                (w, h) == (40, 20) && bytes[..4] == [255, 0, 0, 255]
            }
            _ => false,
        });
        assert!(painted, "the canvas bitmap never reached the display list");
    }

    #[test]
    fn dynamically_created_canvas_layout_uses_its_bitmap_size() {
        // This is the path framework adapters use: create an element, append
        // it, size it through content attributes, then draw through the 2D
        // context. The canvas bitmap reaching the display list is not enough;
        // layout has to give the replaced element a nonzero displayed box.
        let mut doc = crate::load_html("<div id='host'></div>", 800.0);
        let host = doc.get_element_by_id("host").expect("host");
        let id = doc.create_element("canvas");
        doc.append_child(host, id);
        doc.set_attribute(id, "width", "40");
        doc.set_attribute(id, "height", "20");
        assert!(doc.get_context_2d(id));
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 40.0, 20.0);
        })
        .expect("canvas");

        crate::LayoutEngine::new().layout(&mut doc, 800.0);
        let canvas = doc.get_node(id).expect("canvas");
        assert_eq!(canvas.layout.content_rect.w.round(), 40.0);
        assert_eq!(canvas.layout.content_rect.h.round(), 20.0);

        let list =
            crate::renderer::display_list_builder::build_display_list(&doc.root, 800.0, 600.0);
        let painted = list.commands.iter().any(|cmd| match cmd {
            crate::renderer::display_list::PaintCmd::Image { rect, data } => {
                let (bytes, w, h) = match data {
                    crate::renderer::display_list::ImageRef::Owned(d, w, h) => {
                        (d.as_slice(), *w, *h)
                    }
                    crate::renderer::display_list::ImageRef::Shared(d, w, h) => {
                        (d.as_slice(), *w, *h)
                    }
                };
                rect.w > 0.0
                    && rect.h > 0.0
                    && (w, h) == (40, 20)
                    && bytes[..4] == [255, 0, 0, 255]
            }
            _ => false,
        });
        assert!(painted, "laid-out canvas bitmap never reached the display list");
    }
}
