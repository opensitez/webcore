#[cfg(test)]
mod tests {
    use crate::layout::hit_test::hit_test_box_at;
    use crate::tests::harness::parse_and_layout;

    #[test]
    fn test_float_right_on_same_line() {
        let html = r#"
            <style>body { margin: 0; }</style>
            <div style="width: 200px; font-size: 10px;">
                Text <span id="float" style="float: right; width: 50px; height: 10px;"></span>
            </div>
        "#;
        let doc = parse_and_layout(html, 200.0);

        let float_box =
            crate::tests::test_grid::find_by_id(&doc.root, "float").expect("float box not found");
        // Float should be at Y=0 (same as text)
        assert_eq!(
            float_box.layout.border_rect.y, 0.0,
            "Float should be on the same line as text"
        );
        assert_eq!(
            float_box.layout.border_rect.x, 150.0,
            "Float should be on the right edge"
        );
    }

    #[test]
    fn test_float_left_no_overlap() {
        let html = r#"
            <div id="wrap" style="width: 200px; font-size: 10px;">
                <span id="dot" style="float: left; width: 10px; height: 10px;"></span>
                <span id="text">Text</span>
            </div>
        "#;
        let doc = parse_and_layout(html, 200.0);

        let dot_box =
            crate::tests::test_grid::find_by_id(&doc.root, "dot").expect("dot box not found");
        let wrap_box =
            crate::tests::test_grid::find_by_id(&doc.root, "wrap").expect("wrap box not found");

        println!("Dot rect: {:?}", dot_box.layout.border_rect);

        // The float occupies x=0..10 on the first line.
        // The parent's first line_cache entry must start at x >= 10 so
        // the inline text doesn't overlap the float.
        assert!(
            !wrap_box.layout.line_cache.is_empty(),
            "wrap div must have at least one layout line"
        );
        let first_line = &wrap_box.layout.line_cache[0];
        println!("First line x: {}", first_line.x);
        assert!(
            first_line.x >= 10.0,
            "First text line must start after the 10px float (x >= 10); got x={}",
            first_line.x
        );
    }

    #[test]
    fn test_float_sibling_no_leakage() {
        let html = r#"
            <div style="width: 200px; font-size: 10px;">
                <div id="row1" style="height: 10px;">
                    <span style="float: right; width: 50px; height: 10px;"></span>
                    Row1
                </div>
                <div id="row2" style="height: 10px;">
                    Row2
                </div>
            </div>
        "#;
        let doc = parse_and_layout(html, 200.0);

        let row2_box =
            crate::tests::test_grid::find_by_id(&doc.root, "row2").expect("row2 not found");

        let row2_text_line = &row2_box.layout.line_cache[0];
        // Row2 should start at its box's content_rect.x
        assert!(
            (row2_text_line.x - row2_box.layout.content_rect.x).abs() < 0.1,
            "Row2.x ({}) should be equal to content_rect.x ({})",
            row2_text_line.x,
            row2_box.layout.content_rect.x
        );
    }

    #[test]
    fn right_float_narrows_nested_list_item_text() {
        let html = r#"
            <style>
                body { margin: 0; }
                #wrap { width: 420px; font-size: 16px; line-height: 20px; }
                #pic { float: right; width: 120px; height: 100px; }
                ul { margin: 0; padding-left: 32px; }
            </style>
            <div id="wrap">
                <div id="pic"></div>
                <ul>
                    <li id="item">This list item should wrap before the right float instead of painting underneath it.</li>
                </ul>
            </div>
        "#;
        let doc = parse_and_layout(html, 500.0);
        let item =
            crate::tests::test_grid::find_by_id(&doc.root, "item").expect("list item not found");
        let pic = crate::tests::test_grid::find_by_id(&doc.root, "pic").expect("float not found");
        assert!(
            !item.layout.line_cache.is_empty(),
            "list item should have inline line cache"
        );
        let first = &item.layout.line_cache[0];
        assert!(
            first.x + first.width <= pic.layout.border_rect.x + 1.0,
            "first list line should end before right float: line x={} w={}, float x={}",
            first.x,
            first.width,
            pic.layout.border_rect.x
        );
    }
}
