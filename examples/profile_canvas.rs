//! Measure drawing through a document-owned WHATWG canvas.

use std::time::Instant;

use webcore::{canvas::Color, load_html};

fn main() {
    let mut document = load_html(
        r#"<canvas id="c" width="800" height="480"></canvas>"#,
        800.0,
    );
    let id = document.get_element_by_id("c").unwrap();
    let started = Instant::now();
    for _ in 0..6 {
        document
            .with_canvas_2d(id, |ctx| ctx.set_stroke_color(Color::rgb(86, 208, 224)))
            .unwrap();
        for i in 0..150 {
            let x = (i * 3 % 480) as f32 + 40.0;
            let y = (i * 17 % 288) as f32 + 96.0;
            document.with_canvas_2d(id, |ctx| ctx.begin_path()).unwrap();
            document
                .with_canvas_2d(id, |ctx| ctx.move_to(x, y))
                .unwrap();
            document
                .with_canvas_2d(id, |ctx| ctx.line_to(x + 8.0, y + 4.0))
                .unwrap();
            document.with_canvas_2d(id, |ctx| ctx.stroke()).unwrap();
        }
    }
    std::hint::black_box(&document);
    println!("document-backed: {:?} for 900 segments", started.elapsed());
}
