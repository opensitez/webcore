#[cfg(test)]
mod tests {
    use crate::canvas::{Canvas, Color, Font};

    #[test]
    fn browser_css_color_reaches_shared_canvas() {
        let mut doc = crate::load_html("<canvas id='c' width='1' height='1'></canvas>", 800.0);
        let id = doc.get_element_by_id("c").expect("canvas");
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_style_css("rebeccapurple");
            ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        assert_eq!(pixel(&doc, id, 0, 0), [102, 51, 153, 255]);
    }

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
    fn zero_sized_canvas_retains_state_but_exposes_no_scratch_pixels() {
        let mut surfaces = crate::canvas::CanvasSurfaces::default();
        let mut pixels = Vec::new();
        surfaces
            .with_context(7, &mut pixels, 0, 8, |ctx| {
                ctx.set_fill_color(Color::rgb(255, 0, 0));
                ctx.fill_rect(0.0, 0.0, 1.0, 1.0);
            })
            .unwrap();
        let read = surfaces
            .with_context(7, &mut pixels, 0, 8, |ctx| {
                let color = ctx.drawing_state().fill.clone();
                let read = ctx.get_image_data(0, 0, 1, 1).unwrap();
                (color, read)
            })
            .unwrap();
        assert_eq!(read.0, crate::canvas::Paint::Color(Color::rgb(255, 0, 0)));
        assert_eq!(read.1.data, vec![0, 0, 0, 0]);
        assert!(pixels.is_empty());
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
    fn canvas_text_uses_gradient_pixels_not_a_flat_first_stop() {
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="200" height="60"></canvas>"#);
        doc.with_canvas_2d(id, |ctx| {
            let mut gradient = ctx.create_linear_gradient(0.0, 0.0, 180.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(255, 0, 0)).unwrap();
            gradient.add_color_stop(1.0, Color::rgb(0, 0, 255)).unwrap();
            ctx.set_fill_paint(&crate::canvas::Paint::Gradient(gradient));
            ctx.set_font(&Font::new("sans-serif", 48.0));
            ctx.fill_text("HHHH", 5.0, 45.0);
        })
        .unwrap();
        let pixels = doc
            .get_node(id)
            .unwrap()
            .image_data
            .as_ref()
            .unwrap()
            .clone();
        let solid: Vec<_> = pixels.chunks_exact(4).filter(|p| p[3] > 200).collect();
        assert!(solid.len() > 100);
        let min_red = solid.iter().map(|p| p[0]).min().unwrap();
        let max_red = solid.iter().map(|p| p[0]).max().unwrap();
        assert!(
            max_red - min_red > 50,
            "flat text fill: {min_red}..{max_red}"
        );
    }

    #[test]
    fn stroked_text_uses_the_gradient_across_glyph_outlines() {
        let (mut doc, id) = canvas_doc(r#"<canvas id="c" width="200" height="60"></canvas>"#);
        doc.with_canvas_2d(id, |ctx| {
            let mut gradient = ctx.create_linear_gradient(0.0, 0.0, 180.0, 0.0);
            gradient.add_color_stop(0.0, Color::rgb(255, 0, 0)).unwrap();
            gradient.add_color_stop(1.0, Color::rgb(0, 0, 255)).unwrap();
            ctx.set_stroke_paint(&crate::canvas::Paint::Gradient(gradient));
            ctx.set_line_width(3.0);
            ctx.set_font(&Font::new("sans-serif", 48.0));
            ctx.stroke_text("HHHH", 5.0, 45.0);
        })
        .unwrap();
        let pixels = doc
            .get_node(id)
            .unwrap()
            .image_data
            .as_ref()
            .unwrap()
            .clone();
        let solid: Vec<_> = pixels.chunks_exact(4).filter(|p| p[3] > 200).collect();
        assert!(solid.len() > 50);
        let min_red = solid.iter().map(|p| p[0]).min().unwrap();
        let max_red = solid.iter().map(|p| p[0]).max().unwrap();
        assert!(
            max_red - min_red > 50,
            "flat stroke fill: {min_red}..{max_red}"
        );
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
                rect.w > 0.0 && rect.h > 0.0 && (w, h) == (40, 20) && bytes[..4] == [255, 0, 0, 255]
            }
            _ => false,
        });
        assert!(
            painted,
            "laid-out canvas bitmap never reached the display list"
        );
    }

    #[test]
    fn drawn_canvas_bitmap_reaches_rendered_pixels() {
        let mut doc = crate::load_html("<div id='host'></div>", 800.0);
        let host = doc.get_element_by_id("host").expect("host");
        let id = doc.create_element("canvas");
        doc.append_child(host, id);
        doc.set_attribute(id, "width", "40");
        doc.set_attribute(id, "height", "20");
        doc.with_canvas_2d(id, |ctx| {
            ctx.set_fill_color(Color::rgb(255, 0, 0));
            ctx.fill_rect(0.0, 0.0, 40.0, 20.0);
        })
        .expect("canvas");

        crate::LayoutEngine::new().layout(&mut doc, 800.0);
        let mut pixmap = tiny_skia::Pixmap::new(800, 600).expect("pixmap");
        let mut renderer = crate::Renderer::new();
        renderer.render(&mut doc, &mut pixmap, 1.0);

        let data = pixmap.data();
        let idx = (10 * 800 + 10) * 4;
        assert!(
            data[idx] > 200 && data[idx + 1] < 40 && data[idx + 2] < 40 && data[idx + 3] == 255,
            "canvas rendered pixel was {:?}",
            &data[idx..idx + 4]
        );
    }
}
