use webcore::load_html;
use webcore::types::*;

fn find_all<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool, out: &mut Vec<&'a WebCore>) {
    if pred(root) {
        out.push(root);
    }
    for child in &root.children {
        find_all(child, pred, out);
    }
}

fn main() {
    // Load the actual demo HTML
    let html = std::fs::read_to_string("examples/html/layout_features.html").unwrap();
    let doc = load_html(&html, 1200.0);

    // Check for column-count boxes
    let mut cols_boxes = Vec::new();
    find_all(
        &doc.root,
        &|b| b.style.column_count.is_some(),
        &mut cols_boxes,
    );
    println!("Boxes with column-count: {}", cols_boxes.len());
    for b in &cols_boxes {
        println!(
            "  tag={} col_count={:?} w={:.1} h={:.1}",
            b.tag, b.style.column_count, b.layout.padding_rect.w, b.layout.padding_rect.h
        );
        for (i, ch) in b.children.iter().enumerate().take(5) {
            println!(
                "    child[{}] tag={} x={:.1} y={:.1} w={:.1} h={:.1}",
                i,
                ch.tag,
                ch.layout.content_rect.x,
                ch.layout.content_rect.y,
                ch.layout.content_rect.w,
                ch.layout.content_rect.h
            );
        }
    }

    // Check for gradient boxes (avatars etc.)
    let mut grad_boxes = Vec::new();
    find_all(
        &doc.root,
        &|b| b.style.gradient_type != GradientType::None,
        &mut grad_boxes,
    );
    println!("\nBoxes with gradient: {}", grad_boxes.len());
    for b in &grad_boxes {
        println!(
            "  tag={} class={:?} w={:.1} h={:.1} aspect={:?} border_radius={:?}",
            b.tag,
            b.attributes.get("class"),
            b.layout.padding_rect.w,
            b.layout.padding_rect.h,
            b.style.aspect_ratio,
            b.style.border_radius,
        );
    }
}
