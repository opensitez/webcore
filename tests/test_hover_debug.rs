fn find_box_with_hover(node: &webcore::types::WebCore, depth: usize) {
    if node.style.hover_style.is_some() {
        let hs = node.style.hover_style.as_ref().unwrap();
        println!(
            "{}<{}> class={:?} border_top_color={:?} bg={:?}",
            "  ".repeat(depth),
            node.tag,
            node.attributes.get("class"),
            hs.border_top_color,
            hs.background_color
        );
    }
    for child in &node.children {
        find_box_with_hover(child, depth + 1);
    }
}

#[test]
fn debug_hover_styles() {
    let html = include_str!("../examples/html/graph.html");
    let doc = webcore::load_html(html, 900.0);
    println!("=== Boxes with hover_style ===");
    find_box_with_hover(&doc.root, 0);
    println!(
        "Total rules with is_hover: {}",
        doc.stylesheet.rules.iter().filter(|r| r.is_hover).count()
    );
    for r in doc.stylesheet.rules.iter().filter(|r| r.is_hover) {
        println!("  hover rule: {:?}", r.declarations);
    }
}
