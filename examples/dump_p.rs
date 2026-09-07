use std::fs;
use webcore::html::serialize_html;
use webcore::{parse_html_with_base, LayoutEngine};

fn traverse(node: &webcore::WebCore, depth: usize) {
    if node.tag == "p" {
        println!(
            "[EX] p tag display={:?} text='{}'",
            node.style.display,
            node.text_content()
        );
        if node.text_content().contains("Inline image") {
            println!("  line_cache.len = {}", node.layout.line_cache.len());
            for (i, line) in node.layout.line_cache.iter().enumerate() {
                println!(
                    "   line[{}] y={} w={} h={} ascent={} descent={}",
                    i, line.y, line.width, line.height, line.ascent, line.descent
                );
            }
            for child in &node.children {
                println!(
                    "  child tag={} display={:?} margin_rect={:?}",
                    child.tag, child.style.display, child.layout.margin_rect
                );
            }
        }
    }
    for ch in &node.children {
        traverse(ch, depth + 1);
    }
}

fn main() {
    let path = "examples/html/demo.html";
    let s = fs::read_to_string(path).expect("read demo.html");
    let mut doc = parse_html_with_base(&s, "");
    // Dump children of the paragraph before layout
    fn dump_children_pre(node: &webcore::WebCore) {
        if node.tag == "p" && node.text_content().contains("Inline image") {
            println!("[PRE] p children count={}", node.children.len());
            for ch in &node.children {
                println!(" [PRE] child tag={} display={:?}", ch.tag, ch.style.display);
            }
        }
        for ch in &node.children {
            dump_children_pre(ch);
        }
    }
    dump_children_pre(&doc.root);
    // Print serialized structure to inspect where <img> went
    println!(
        "--- Serialized HTML ---\n{}\n-----------------------",
        serialize_html(&doc)
    );

    let mut engine = LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;
    engine.layout(&mut doc, 800.0);
    traverse(&doc.root, 0);

    // Find hero container (contains an img with src="hero") and dump its rects
    fn dump_hero(node: &webcore::WebCore) {
        let mut found = false;
        for ch in &node.children {
            if ch.tag == "img"
                && ch
                    .attributes
                    .get("src")
                    .map(|s| s == "hero")
                    .unwrap_or(false)
            {
                found = true;
                break;
            }
        }
        if found && node.tag == "div" {
            println!("[HERO] found hero container: tag={} display={:?} content_rect={:?} border_rect={:?} margin_rect={:?}", node.tag, node.style.display, node.layout.content_rect, node.layout.border_rect, node.layout.margin_rect);
            for (i, ch) in node.children.iter().enumerate() {
                println!(" [HERO] child[{}] tag={} display={:?} margin_rect={:?} border_rect={:?} content_rect={:?} style.position={:?} style.top={:?} style.left={:?} style.right={:?} style.bottom={:?}",
                         i, ch.tag, ch.style.display, ch.layout.margin_rect, ch.layout.border_rect, ch.layout.content_rect, ch.style.position, ch.style.top, ch.style.left, ch.style.right, ch.style.bottom);
            }
        }
        for ch in &node.children {
            dump_hero(ch);
        }
    }
    dump_hero(&doc.root);
}
