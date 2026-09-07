#[test]
fn badge_align_center_no_stretch() {
    use webcore::load_html;
    use webcore::types::{AlignItems, WebCore};

    fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
        if pred(root) {
            return Some(root);
        }
        for child in &root.children {
            if let Some(b) = find_box(child, pred) {
                return Some(b);
            }
        }
        None
    }

    let doc = load_html(
        r#"<div style="display:flex; align-items:center; height:34px; padding:0 12px; width:200px;">
            <span style="flex:1;">Label</span>
            <span id="badge" style="padding:1px 7px; min-width:22px; text-align:center; height:18px; box-sizing:border-box;">6</span>
        </div>"#,
        300.0,
    );

    // Check parent's align-items
    let parent = find_box(&doc.root, &|b| {
        b.style.display == webcore::types::Display::Flex
    })
    .unwrap();
    println!("parent align_items: {:?}", parent.style.align_items);
    assert_eq!(
        parent.style.align_items,
        AlignItems::Center,
        "align-items should be Center"
    );

    let badge = find_box(&doc.root, &|b| b.get_attr("id") == Some("badge")).unwrap();
    println!(
        "badge: br={:?} cr={:?}",
        badge.layout.border_rect, badge.layout.content_rect
    );
    println!(
        "badge height style: is_auto={}",
        badge.style.height.is_auto()
    );

    if !badge.layout.line_cache.is_empty() {
        let line = &badge.layout.line_cache[0];
        let offset = line.x - badge.layout.content_rect.x;
        let expected = (badge.layout.content_rect.w - line.width) / 2.0;
        println!(
            "badge line offset={:.4} expected={:.4} text_w={:.4} content_w={:.4}",
            offset, expected, line.width, badge.layout.content_rect.w
        );
        assert!(
            (offset - expected).abs() < 2.0,
            "centering offset={:.4} but expected={:.4}",
            offset,
            expected
        );
    }
}
