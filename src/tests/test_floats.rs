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
    fn hit_testing_reaches_controls_inside_float_overlapping_later_block() {
        let html = r#"
            <style>
                body { margin: 0; }
                #search { width: 300px; }
                #form { float: right; width: 120px; height: 30px; }
                #q { width: 100px; height: 24px; }
                #cover { height: 100px; }
            </style>
            <div id="search">
                <form id="form"><input id="q" value=""></form>
            </div>
            <div id="cover"></div>
        "#;
        let mut doc = parse_and_layout(html, 300.0);
        let input = crate::tests::test_grid::find_by_id(&doc.root, "q").expect("input not found");
        let cover = crate::tests::test_grid::find_by_id(&doc.root, "cover").expect("cover");
        assert!(
            cover.layout.border_rect.y <= input.layout.border_rect.y + 0.5,
            "test setup needs later block overlapping the floated form vertically: cover={:?} input={:?}",
            cover.layout.border_rect,
            input.layout.border_rect
        );

        let pt = (
            input.layout.border_rect.x + input.layout.border_rect.w / 2.0,
            input.layout.border_rect.y + input.layout.border_rect.h / 2.0,
        );
        let hit = crate::layout::hit_test::point_to_hit(&doc.root, pt, 0).expect("hit");
        assert_eq!(
            hit.node_id, input.node_id,
            "visible input inside a float must receive the pointer event"
        );
        assert_eq!(
            hit_test_box_at(&doc.root, pt, 0),
            input.node_id,
            "the secondary hit walker must agree"
        );

        doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, pt, 0);
        doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, pt, 0);
        assert_eq!(doc.focused_box, hit.node_id);
        doc.process_key_event(
            crate::dom::HtmlEventType::KeyDown,
            'x' as u32,
            Some('x'),
            false,
            false,
            false,
            false,
        );
        let input = crate::tests::test_grid::find_by_id(&doc.root, "q").expect("input not found");
        assert_eq!(crate::types::input_value(input), "x");
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

    #[test]
    fn inline_block_clears_floats_when_wider_than_gap() {
        let html = r#"
            <style>
                body { margin: 0; }
                .container { width: 500px; }
                .fleft { float: left; width: 200px; height: 50px; }
                .fright { float: right; width: 200px; height: 40px; }
                .wide-ib { display: inline-block; width: 300px; height: 30px; }
            </style>
            <div class="container">
                <div class="fleft"></div>
                <div class="fright"></div>
                <div>
                    <span id="ib" class="wide-ib">Hello</span>
                </div>
            </div>
        "#;
        let doc = parse_and_layout(html, 500.0);
        let ib = crate::tests::test_grid::find_by_id(&doc.root, "ib").expect("ib not found");
        assert!(
            ib.layout.margin_rect.y >= 40.0,
            "wide inline block must drop down below constricting floats, got y={}",
            ib.layout.margin_rect.y
        );
    }

    #[test]
    fn floated_percentage_columns_ignore_auto_horizontal_margins() {
        let html = r#"
            <style>
                body { margin: 0; }
                .row { width: 1200px; }
                .main, .side {
                    float: left;
                    margin-left: auto;
                    margin-right: auto;
                    box-sizing: border-box;
                    height: 40px;
                }
                .main { width: 66.66666667%; }
                .side { width: 33.33333333%; }
            </style>
            <div class="row">
                <div id="main" class="main"></div>
                <div id="side" class="side"></div>
            </div>
        "#;
        let doc = parse_and_layout(html, 1300.0);
        let main = crate::tests::test_grid::find_by_id(&doc.root, "main").expect("main not found");
        let side = crate::tests::test_grid::find_by_id(&doc.root, "side").expect("side not found");

        assert!(
            (main.layout.margin_rect.y - side.layout.margin_rect.y).abs() < 0.1,
            "floated percentage columns should share a row; main y={}, side y={}, main={:?}, side={:?}, main margins=({}, {}), side margins=({}, {})",
            main.layout.margin_rect.y,
            side.layout.margin_rect.y,
            main.layout.margin_rect,
            side.layout.margin_rect,
            main.layout.resolved_margin_left,
            main.layout.resolved_margin_right,
            side.layout.resolved_margin_left,
            side.layout.resolved_margin_right
        );
        assert!(
            side.layout.margin_rect.x >= main.layout.margin_rect.right() - 1.0,
            "sidebar should sit after main column, main={:?}, side={:?}",
            main.layout.margin_rect,
            side.layout.margin_rect
        );
        assert_eq!(main.layout.resolved_margin_left, 0.0);
        assert_eq!(side.layout.resolved_margin_right, 0.0);
    }

    #[test]
    fn flex_items_contain_internal_floats_for_cross_size() {
        let html = r#"
            <style>
                body { margin: 0; }
                .bar {
                    display: flex;
                    align-items: center;
                    width: 500px;
                    height: 60px;
                }
                .center { flex: 1 1 auto; }
                .item {
                    float: left;
                    width: 80px;
                    height: 60px;
                }
            </style>
            <div class="bar">
                <div id="center" class="center">
                    <div id="item" class="item"></div>
                </div>
            </div>
        "#;
        let doc = parse_and_layout(html, 600.0);
        let center =
            crate::tests::test_grid::find_by_id(&doc.root, "center").expect("center not found");
        let item = crate::tests::test_grid::find_by_id(&doc.root, "item").expect("item not found");

        assert!(
            center.layout.margin_rect.h >= 59.0,
            "flex item should contain internal floats for auto cross-size, center={:?}, item={:?}",
            center.layout.margin_rect,
            item.layout.margin_rect
        );
        assert!(
            (center.layout.margin_rect.y - item.layout.margin_rect.y).abs() < 0.1,
            "internal float should start at the flex item's top, center={:?}, item={:?}",
            center.layout.margin_rect,
            item.layout.margin_rect
        );
    }
}
