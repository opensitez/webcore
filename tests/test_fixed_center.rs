#[test]
fn fixed_center_plus() {
    use webcore::load_html;
    // Use actual demo.html HTML — newline before the +
    let doc = load_html(
        "<div style=\"position: fixed; bottom: 16px; right: 16px; width: 48px; height: 48px; background-color: #3498db; color: white; text-align: center; font-size: 24pt; font-weight: bold; z-index: 100;\">\n+</div>",
        800.0);

    fn find<'a>(
        node: &'a webcore::WebCore,
        pred: &dyn Fn(&webcore::WebCore) -> bool,
    ) -> Option<&'a webcore::WebCore> {
        if pred(node) {
            return Some(node);
        }
        for c in &node.children {
            if let Some(b) = find(c, pred) {
                return Some(b);
            }
        }
        None
    }

    let btn = find(&doc.root, &|b| {
        b.attributes
            .get("style")
            .map(|s| s.contains("3498db"))
            .unwrap_or(false)
    })
    .unwrap();
    println!("border_rect: {:?}", btn.layout.border_rect);
    println!("content_rect: {:?}", btn.layout.content_rect);
    println!("text_align: {:?}", btn.style.text_align);
    let flat = webcore::dom::get_text_content(btn);
    println!("flat text: {:?}", flat);
    println!("num lines: {}", btn.layout.line_cache.len());
    println!("num inline_runs: {}", btn.layout.inline_runs.len());
    for (i, line) in btn.layout.line_cache.iter().enumerate() {
        let ts = line.text_start.min(flat.len());
        let te = (line.text_start + line.text_length).min(flat.len());
        let text = &flat[ts..te];
        let offset = line.x - btn.layout.content_rect.x;
        let expected = (btn.layout.content_rect.w - line.width) / 2.0;
        println!(
            "line[{}] x={:.2} width={:.2} text={:?} offset={:.2} expected={:.2}",
            i, line.x, line.width, text, offset, expected
        );
    }
}
