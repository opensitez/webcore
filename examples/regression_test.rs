use webcore::*;

fn find_by_text<'a>(node: &'a WebCore, needle: &str) -> Option<&'a WebCore> {
    if node.tag == "h3" && node.children.iter().any(|c| c.text.contains(needle)) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_text(child, needle) {
            return Some(n);
        }
    }
    None
}

fn draw_debug_rects(
    node: &WebCore,
    pixmap: &mut tiny_skia::Pixmap,
    scale: f32,
    scroll_y: f32,
    depth: usize,
) {
    if node.tag == "#text" || matches!(node.style.display, webcore::types::Display::None) {
        return;
    }
    if node.layout.content_rect.w > 0.0 && node.layout.content_rect.h > 0.0 {
        let x = node.layout.content_rect.x * scale;
        let y = (node.layout.content_rect.y - scroll_y) * scale;
        let w = node.layout.content_rect.w * scale;
        let h = node.layout.content_rect.h * scale;
        if y + h > 0.0 && y < pixmap.height() as f32 {
            let color = match depth % 3 {
                0 => tiny_skia::Color::from_rgba8(255, 0, 0, 100),
                1 => tiny_skia::Color::from_rgba8(0, 0, 255, 100),
                _ => tiny_skia::Color::from_rgba8(0, 180, 0, 100),
            };
            let mut paint = tiny_skia::Paint::default();
            paint.set_color(color);
            let stroke = tiny_skia::Stroke {
                width: 1.0,
                ..tiny_skia::Stroke::default()
            };
            if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
                let path = tiny_skia::PathBuilder::from_rect(rect);
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &stroke,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
    }
    for child in &node.children {
        draw_debug_rects(child, pixmap, scale, scroll_y, depth + 1);
    }
}

fn main() {
    let html = std::fs::read_to_string("examples/html/demo.html").expect("read demo.html");
    let mut doc = load_html_with_base(&html, "", 900.0, 700.0);
    let mut renderer = Renderer::new();
    renderer.set_scale(2.0);
    let mut eng = renderer.layout_engine();
    eng.viewport_h = 700.0;
    eng.layout(&mut doc, 900.0);

    if let Some(h3) = find_by_text(&doc.root, "Art of CSS Positioning") {
        let y_start = (h3.layout.border_rect.y - 30.0).max(0.0);
        let pw = 1800u32;
        let ph = 1200u32;
        let mut pixmap = tiny_skia::Pixmap::new(pw, ph).unwrap();
        pixmap.fill(tiny_skia::Color::WHITE);
        doc.scroll_y = y_start;
        renderer.render(&mut doc, &mut pixmap, 2.0);
        draw_debug_rects(&doc.root, &mut pixmap, 2.0, y_start, 0);
        pixmap.save_png("/tmp/demo_debug.png").unwrap();
        eprintln!("Saved /tmp/demo_debug.png scroll_y={:.1}", y_start);

        // Dump geometry
        fn dump(node: &WebCore, y_min: f32, y_max: f32, d: usize) {
            if node.tag == "#text" {
                return;
            }
            let y = node.layout.border_rect.y;
            if y + node.layout.border_rect.h < y_min || y > y_max {
                return;
            }
            if node.layout.border_rect.w > 0.0 {
                let indent = "  ".repeat(d);
                let text: String = node
                    .children
                    .iter()
                    .filter(|c| c.tag == "#text")
                    .map(|c| c.text.trim().to_string())
                    .collect::<Vec<_>>()
                    .join("");
                let ts = if text.len() > 40 {
                    format!("{}...", &text[..40])
                } else {
                    text
                };
                eprintln!(
                    "{}<{}> content: x={:.1} w={:.1} right={:.1} | \"{}\"",
                    indent,
                    node.tag,
                    node.layout.content_rect.x,
                    node.layout.content_rect.w,
                    node.layout.content_rect.x + node.layout.content_rect.w,
                    ts
                );
            }
            for c in &node.children {
                dump(c, y_min, y_max, d + 1);
            }
        }
        eprintln!("\n=== Section geometry ===");
        dump(&doc.root, y_start, y_start + 350.0, 0);
    }
}
