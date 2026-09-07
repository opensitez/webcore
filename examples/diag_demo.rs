use webcore::types::{Display, WebCore};
/// Port of wxhtmledit/examples/diag.cpp
/// Diagnostic: dump box tree to see layout results.
use webcore::{load_html, LayoutEngine};

fn dump_box_tree(node: &WebCore, depth: usize) {
    if matches!(node.style.display, Display::None) {
        return;
    }

    let indent = "  ".repeat(depth);
    let disp_str = format!("{:?}", node.style.display);

    let tag = if node.tag.is_empty() {
        "(box)"
    } else {
        &node.tag
    };

    println!(
        "{}{:<8} disp={:<12} content=[{:.0},{:.0} {:.0}x{:.0}] margin=[{:.0},{:.0} {:.0}x{:.0}]",
        indent,
        tag,
        disp_str,
        node.layout.content_rect.x,
        node.layout.content_rect.y,
        node.layout.content_rect.w,
        node.layout.content_rect.h,
        node.layout.margin_rect.x,
        node.layout.margin_rect.y,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h
    );

    if !node.layout.line_cache.is_empty() {
        print!("{}  lines={}[", indent, node.layout.line_cache.len());
        for (i, line) in node.layout.line_cache.iter().enumerate() {
            print!(
                "LINE({:.0},{:.0} {:.0}x{:.0})",
                line.x, line.y, line.width, line.height
            );
            if i + 1 < node.layout.line_cache.len() {
                print!(", ");
            }
        }
        println!("]");
    }

    for child in &node.children {
        dump_box_tree(child, depth + 1);
    }
}

fn main() {
    let html = r#"
<html><head><style>
.main { display: flex; }
.sidebar {
  background: #161b22; border-right: 1px solid #30363d;
  padding: 14px; width: 170px; min-width: 170px;
}
.sb-item {
  padding: 5px 8px; margin-bottom: 2px; border-radius: 6px;
  font-size: 8pt; color: #c9d1d9;
  border: 1px solid transparent;
}
.sb-item:hover { background: #21262d; border-color: #30363d; }
.sb-item .sstat { color: #8b949e; float: right; }
.sb-dot {
  display: inline-block; width: 6px; height: 6px;
  border-radius: 3px; margin-right: 5px;
}
</style></head>
<body>
<div class='main'>
<div class='sidebar'>
<div class='sb-item' id='sb-organic'>
  <span class='sb-dot' style='background:#4e79a7;'></span>Organic <span class='sstat'>48%</span>
</div>
<div class='sb-item' id='sb-direct'>
  <span class='sb-dot' style='background:#f28e2b;'></span>Direct <span class='sstat'>27%</span>
</div>
</div>
<div class='content' style='flex:1;'>Content</div>
</div>
</body></html>
"#;

    println!("=== PARSING HTML ===");
    let mut doc = load_html(html, 1000.0);

    println!("\n=== BOX TREE (1000px viewport) ===");
    dump_box_tree(&doc.root, 0);

    println!("\n=== RELAYOUT (170px viewport) ===");
    LayoutEngine::new().layout(&mut doc, 170.0);
    dump_box_tree(&doc.root, 0);
}
