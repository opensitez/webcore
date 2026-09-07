// Comprehensive display mode tests — covers inline, block, inline-block,
// inline-flex, inline-grid, none, contents, flow-root, list-item,
// and complex interactions between different display types.

use webcore::load_html;
use webcore::types::*;

fn by_id<'a>(root: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if root.attributes.get("id").map(|v| v == id).unwrap_or(false) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(f) = by_id(child, id) {
            return Some(f);
        }
    }
    None
}
fn find<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(f) = find(child, pred) {
            return Some(f);
        }
    }
    None
}
fn find_all<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Vec<&'a WebCore> {
    let mut r = Vec::new();
    if pred(root) {
        r.push(root);
    }
    for c in &root.children {
        r.extend(find_all(c, pred));
    }
    r
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  INLINE-BLOCK                                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_block_side_by_side() {
    let d = load_html(
        concat!(
            "<div style='width:600px'>",
            "<div id='a' style='display:inline-block;width:150px;height:50px'>A</div>",
            "<div id='b' style='display:inline-block;width:150px;height:50px'>B</div>",
            "<div id='c' style='display:inline-block;width:150px;height:50px'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "same line a-b"
    );
    assert!(
        (b.layout.content_rect.y - c.layout.content_rect.y).abs() < 5.0,
        "same line b-c"
    );
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 140.0,
        "b right of a"
    );
    assert!(
        c.layout.content_rect.x > b.layout.content_rect.x + 140.0,
        "c right of b"
    );
}

#[test]
fn inline_block_wraps_to_next_line() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='a' style='display:inline-block;width:200px;height:50px'>A</div>",
            "<div id='b' style='display:inline-block;width:200px;height:50px'>B</div>",
            "<div id='c' style='display:inline-block;width:200px;height:50px'>C</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        c.layout.content_rect.y > a.layout.content_rect.y + 40.0,
        "c wraps"
    );
}

#[test]
fn inline_block_respects_width_height() {
    let d = load_html(
        "<span id='t' style='display:inline-block;width:200px;height:100px'>Box</span>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 200.0).abs() < 5.0,
        "w=200 w={:.0}",
        t.layout.content_rect.w
    );
    assert!(
        (t.layout.content_rect.h - 100.0).abs() < 5.0,
        "h=100 h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn inline_block_with_text_and_padding() {
    let d = load_html(concat!(
        "<div style='width:600px'>",
        "<span id='a' style='display:inline-block;padding:10px;border:2px solid black'>Tag A</span>",
        "<span id='b' style='display:inline-block;padding:10px;border:2px solid black'>Tag B</span>",
        "</div>",
    ), 700.0);
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(a.layout.content_rect.w > 20.0, "a has width");
    assert!(
        b.layout.content_rect.x > a.layout.border_rect.x + a.layout.border_rect.w - 5.0,
        "b after a"
    );
    assert!(a.layout.resolved_pad_top > 8.0, "a has padding");
}

#[test]
fn inline_block_vertical_align_baseline() {
    let d = load_html(concat!(
        "<div style='width:600px;font-size:16px'>",
        "Text ",
        "<span id='ib' style='display:inline-block;width:50px;height:80px;vertical-align:baseline'>IB</span>",
        " more text",
        "</div>",
    ), 700.0);
    let ib = by_id(&d.root, "ib").unwrap();
    assert!(
        (ib.layout.content_rect.h - 80.0).abs() < 5.0,
        "ib keeps height h={:.0}",
        ib.layout.content_rect.h
    );
}

#[test]
fn inline_block_vertical_align_middle() {
    let d = load_html(concat!(
        "<div style='width:600px;font-size:16px;line-height:60px'>",
        "Text ",
        "<span id='ib' style='display:inline-block;width:30px;height:30px;vertical-align:middle'>IB</span>",
        "</div>",
    ), 700.0);
    let ib = by_id(&d.root, "ib").unwrap();
    assert!(ib.layout.content_rect.w > 25.0, "ib has width");
}

#[test]
fn inline_block_shrink_to_content() {
    let d = load_html(
        concat!(
            "<div style='width:600px'>",
            "<span id='ib' style='display:inline-block'>Short</span>",
            "</div>",
        ),
        700.0,
    );
    let ib = by_id(&d.root, "ib").unwrap();
    assert!(
        ib.layout.content_rect.w < 200.0,
        "inline-block shrinks w={:.0}",
        ib.layout.content_rect.w
    );
    assert!(ib.layout.content_rect.w > 10.0, "has some width");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  INLINE-BLOCK mixed with inline text                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_block_mixed_with_text() {
    let d = load_html(concat!(
        "<div id='p' style='width:500px;font-size:16px'>",
        "Hello ",
        "<span id='tag' style='display:inline-block;padding:2px 8px;background:blue;color:white'>NEW</span>",
        " world",
        "</div>",
    ), 600.0);
    let p = by_id(&d.root, "p").unwrap();
    let tag = by_id(&d.root, "tag").unwrap();
    assert!(!p.layout.line_cache.is_empty(), "parent has text lines");
    assert!(tag.layout.content_rect.w > 20.0, "tag badge has width");
    assert!(tag.layout.content_rect.h > 10.0, "tag badge has height");
}

#[test]
fn inline_block_between_block_elements() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='before' style='height:30px'>Block before</div>",
            "<span id='ib' style='display:inline-block;width:100px;height:50px'>IB</span>",
            "<div id='after' style='height:30px'>Block after</div>",
            "</div>",
        ),
        500.0,
    );
    let before = by_id(&d.root, "before").unwrap();
    let ib = by_id(&d.root, "ib").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    assert!(
        ib.layout.content_rect.y >= before.layout.content_rect.y + 25.0,
        "ib below block"
    );
    assert!(
        after.layout.content_rect.y >= ib.layout.content_rect.y + 45.0,
        "after below ib"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY: NONE                                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn display_none_takes_no_space() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='before' style='height:50px'>Before</div>",
            "<div style='display:none;height:200px'>Hidden</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    let before = by_id(&d.root, "before").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    let gap =
        after.layout.content_rect.y - (before.layout.content_rect.y + before.layout.content_rect.h);
    assert!(gap < 5.0, "none takes no space gap={:.0}", gap);
}

#[test]
fn display_none_children_not_laid_out() {
    let d = load_html(
        concat!(
            "<div style='display:none'>",
            "<div id='child' style='width:200px;height:100px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let child = by_id(&d.root, "child").unwrap();
    assert_eq!(
        child.layout.content_rect.w as u32, 0,
        "child of none has 0 width"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY: CONTENTS                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn display_contents_children_promoted() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div style='display:contents'>",
            "  <div id='a' style='flex:1'>A</div>",
            "  <div id='b' style='flex:1'>B</div>",
            "</div>",
            "<div id='c' style='flex:1'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // contents wrapper disappears, all three are flex items → ~200px each
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 15.0,
        "contents: a={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 200.0).abs() < 15.0,
        "contents: b={:.0}",
        b.layout.content_rect.w
    );
    assert!(
        (c.layout.content_rect.w - 200.0).abs() < 15.0,
        "contents: c={:.0}",
        c.layout.content_rect.w
    );
}

#[test]
fn display_contents_in_grid() {
    let d = load_html(
        concat!(
            "<div style='display:grid;grid-template-columns:1fr 1fr 1fr;width:600px'>",
            "<div style='display:contents'>",
            "  <div id='a'>A</div>",
            "  <div id='b'>B</div>",
            "</div>",
            "<div id='c'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // All three are grid items
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "same row"
    );
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 100.0,
        "b after a"
    );
    assert!(
        c.layout.content_rect.x > b.layout.content_rect.x + 100.0,
        "c after b"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY: FLOW-ROOT                                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flow_root_contains_floats() {
    let d = load_html(
        concat!(
            "<div id='fr' style='display:flow-root;width:400px'>",
            "<div style='float:left;width:100px;height:150px'>Float</div>",
            "<p>Text</p>",
            "</div>",
        ),
        500.0,
    );
    let fr = by_id(&d.root, "fr").unwrap();
    assert!(
        fr.layout.content_rect.h >= 145.0,
        "flow-root contains float h={:.0}",
        fr.layout.content_rect.h
    );
}

#[test]
fn flow_root_prevents_margin_collapse() {
    let d = load_html(
        concat!(
            "<div id='fr' style='display:flow-root;width:400px'>",
            "<div style='margin-top:50px;height:30px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let fr = by_id(&d.root, "fr").unwrap();
    // flow-root prevents margin collapsing → child margin stays inside
    assert!(
        fr.layout.content_rect.h >= 75.0,
        "flow-root h={:.0} should include child margin",
        fr.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY: LIST-ITEM                                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn list_item_has_marker() {
    let d = load_html(
        concat!(
            "<ul style='width:400px'>",
            "<li id='li'>List item text</li>",
            "</ul>",
        ),
        500.0,
    );
    let li = by_id(&d.root, "li").unwrap();
    assert_eq!(li.style.display, Display::ListItem);
    assert!(li.layout.content_rect.h > 10.0, "list item has height");
}

#[test]
fn list_item_display_inline_overrides() {
    let d = load_html(
        concat!(
            "<style>li { display: inline; }</style>",
            "<ul style='width:400px'>",
            "<li id='a'>A</li>",
            "<li id='b'>B</li>",
            "<li id='c'>C</li>",
            "</ul>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert_eq!(a.style.display, Display::Inline, "CSS overrides to inline");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  INLINE-FLEX                                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_flex_sits_in_text_flow() {
    let d = load_html(
        concat!(
        "<div style='width:600px;font-size:16px'>",
        "Before ",
        "<span id='if' style='display:inline-flex;gap:5px;padding:4px 8px;background:lightblue'>",
        "  <span>Tag1</span><span>Tag2</span>",
        "</span>",
        " after",
        "</div>",
    ),
        700.0,
    );
    let f = by_id(&d.root, "if").unwrap();
    assert!(f.layout.content_rect.w > 30.0, "inline-flex has width");
    assert!(
        f.layout.content_rect.w < 300.0,
        "inline-flex shrinks to content"
    );
}

#[test]
fn inline_flex_multiple_on_same_line() {
    let d = load_html(
        concat!(
            "<div style='width:800px'>",
            "<div id='a' style='display:inline-flex;width:150px;height:40px'>A</div>",
            "<div id='b' style='display:inline-flex;width:150px;height:40px'>B</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "same line"
    );
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 140.0,
        "b after a"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  INLINE-GRID                                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_grid_sits_in_text_flow() {
    let d = load_html(concat!(
        "<div style='width:600px'>",
        "Text ",
        "<div id='ig' style='display:inline-grid;grid-template-columns:1fr 1fr;gap:5px;width:200px'>",
        "  <div>A</div><div>B</div>",
        "</div>",
        " more text",
        "</div>",
    ), 700.0);
    let ig = by_id(&d.root, "ig").unwrap();
    assert!(
        (ig.layout.content_rect.w - 200.0).abs() < 10.0,
        "inline-grid w=200 w={:.0}",
        ig.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BLOCK inside INLINE (anonymous block boxes)                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn block_inside_inline_renders() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<span>Before ",
            "<div id='block' style='height:40px'>Block inside span</div>",
            " after</span>",
            "</div>",
        ),
        500.0,
    );
    let block = by_id(&d.root, "block").unwrap();
    assert!(
        (block.layout.content_rect.h - 40.0).abs() < 5.0,
        "block inside inline h=40"
    );
    assert!(block.layout.content_rect.w > 300.0, "block fills width");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY changes via CSS class                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn display_override_block_to_flex() {
    let d = load_html(
        concat!(
            "<style>.flex { display: flex; }</style>",
            "<div class='flex' style='width:600px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.w - 300.0).abs() < 10.0,
        "flex a={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "side by side"
    );
}

#[test]
fn display_override_block_to_none() {
    let d = load_html(
        concat!(
            "<style>.hidden { display: none; }</style>",
            "<div style='width:400px'>",
            "<div id='before' style='height:50px'>Before</div>",
            "<div class='hidden' style='height:200px'>Hidden</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    let before = by_id(&d.root, "before").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    assert!(
        (after.layout.content_rect.y
            - (before.layout.content_rect.y + before.layout.content_rect.h))
            .abs()
            < 5.0,
        "hidden takes no space"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Tag/badge row (inline-block pattern)           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn tag_badge_row() {
    let d = load_html(concat!(
        "<style>",
        ".tags { display:flex; flex-wrap:wrap; gap:8px; width:400px; }",
        ".tag { display:inline-block; padding:4px 12px; border-radius:4px; background:#eee; font-size:14px; }",
        "</style>",
        "<div class='tags'>",
        "<span class='tag' id='t1'>JavaScript</span>",
        "<span class='tag' id='t2'>Rust</span>",
        "<span class='tag' id='t3'>Python</span>",
        "<span class='tag' id='t4'>TypeScript</span>",
        "</div>",
    ), 500.0);
    let t1 = by_id(&d.root, "t1").unwrap();
    let t2 = by_id(&d.root, "t2").unwrap();
    assert!(t1.layout.content_rect.w > 30.0, "tag has width");
    assert!(
        t2.layout.content_rect.x > t1.layout.content_rect.x + 30.0,
        "t2 after t1"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Breadcrumb (inline items with separators)      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn breadcrumb_inline_items() {
    let d = load_html(
        concat!(
            "<nav style='width:600px;font-size:14px'>",
            "<a id='a' style='display:inline'>Home</a>",
            "<span style='display:inline'> / </span>",
            "<a id='b' style='display:inline'>Products</a>",
            "<span style='display:inline'> / </span>",
            "<span id='c' style='display:inline'>Widget</span>",
            "</nav>",
        ),
        700.0,
    );
    let nav = find(&d.root, &|b| b.tag == "nav").unwrap();
    // All on one line
    assert!(!nav.layout.line_cache.is_empty(), "nav has text");
    assert_eq!(nav.layout.line_cache.len(), 1, "breadcrumb on single line");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Tabs (inline-block or flex)                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn tabs_inline_block() {
    let d = load_html(
        concat!(
            "<style>",
            ".tabs { border-bottom:2px solid #ccc; width:500px; }",
            ".tab { display:inline-block; padding:10px 20px; cursor:pointer; }",
            ".tab.active { border-bottom:2px solid blue; }",
            "</style>",
            "<div class='tabs'>",
            "<div class='tab active' id='t1'>Tab 1</div>",
            "<div class='tab' id='t2'>Tab 2</div>",
            "<div class='tab' id='t3'>Tab 3</div>",
            "</div>",
        ),
        600.0,
    );
    let t1 = by_id(&d.root, "t1").unwrap();
    let t2 = by_id(&d.root, "t2").unwrap();
    let t3 = by_id(&d.root, "t3").unwrap();
    assert!(
        (t1.layout.content_rect.y - t2.layout.content_rect.y).abs() < 5.0,
        "same line"
    );
    assert!(
        t2.layout.content_rect.x > t1.layout.content_rect.x + 30.0,
        "t2 after t1"
    );
    assert!(
        t3.layout.content_rect.x > t2.layout.content_rect.x + 30.0,
        "t3 after t2"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Avatar + name (inline-block + text)            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn avatar_name_inline_block() {
    let d = load_html(concat!(
        "<div style='width:400px;font-size:16px'>",
        "<div id='avatar' style='display:inline-block;width:40px;height:40px;border-radius:50%;background:gray;vertical-align:middle'></div>",
        "<span id='name' style='vertical-align:middle;margin-left:10px'>John Doe</span>",
        "</div>",
    ), 500.0);
    let avatar = by_id(&d.root, "avatar").unwrap();
    let name = by_id(&d.root, "name").unwrap();
    assert!(
        (avatar.layout.content_rect.w - 40.0).abs() < 5.0,
        "avatar w=40"
    );
    assert!(
        (avatar.layout.content_rect.h - 40.0).abs() < 5.0,
        "avatar h=40"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  DISPLAY: TABLE (CSS table display)                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn css_table_display() {
    let d = load_html(
        concat!(
        "<style>",
        ".table { display:table; width:400px; }",
        ".row { display:table-row; }",
        ".cell { display:table-cell; padding:8px; }",
        "</style>",
        "<div class='table'>",
        "<div class='row'><div class='cell' id='a'>A</div><div class='cell' id='b'>B</div></div>",
        "<div class='row'><div class='cell' id='c'>C</div><div class='cell' id='d'>D</div></div>",
        "</div>",
    ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // A and B side by side
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 50.0,
        "b right of a"
    );
    // C below A
    assert!(
        c.layout.content_rect.y > a.layout.content_rect.y + 10.0,
        "c below a"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: display changes via specificity                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn display_specificity_override() {
    let d = load_html(
        concat!(
            "<style>",
            "div { display: block; }",
            ".flex-row { display: flex; }",
            "</style>",
            "<div class='flex-row' style='width:600px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "flex from class"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: inline elements with width/height (ignored)          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_ignores_width_height() {
    let d = load_html(
        "<span id='t' style='width:500px;height:200px'>Inline text</span>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Inline elements ignore width/height CSS
    assert!(
        t.layout.content_rect.w < 200.0 || t.layout.content_rect.h < 50.0,
        "inline ignores w/h: w={:.0} h={:.0}",
        t.layout.content_rect.w,
        t.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: deeply nested display types                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn nested_flex_grid_inline_block() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "  <div style='display:grid;grid-template-columns:1fr 1fr;flex:1;gap:10px'>",
            "    <div id='a' style='display:inline-block;width:100px;height:50px'>A</div>",
            "    <div id='b'>B</div>",
            "  </div>",
            "  <div id='side' style='width:200px'>Side</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let side = by_id(&d.root, "side").unwrap();
    assert!(a.layout.content_rect.w > 0.0, "a renders in nested display");
    assert!(side.layout.content_rect.x > 400.0, "side on right");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: empty elements with different display types          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn empty_elements_various_display() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='block'></div>",
            "<span id='inline'></span>",
            "<div id='ib' style='display:inline-block'></div>",
            "<div id='flex' style='display:flex'></div>",
            "<div id='grid' style='display:grid'></div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    // Should not crash
    let after = by_id(&d.root, "after").unwrap();
    assert!(after.layout.content_rect.h > 0.0, "after renders");
}
