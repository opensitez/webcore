// Advanced flex layout tests — covers complex interactions, nested flex,
// flex + positioning, flex + overflow, real-world patterns, and edge cases.

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

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: absolute children excluded from flex layout          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_absolute_child_excluded() {
    let d = load_html(
        concat!(
            "<div style='display:flex;position:relative;width:600px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div id='abs' style='position:absolute;top:0;right:0;width:80px'>Abs</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.w - 300.0).abs() < 10.0,
        "flex:1 a={:.0} (abs excluded)",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 300.0).abs() < 10.0,
        "flex:1 b={:.0} (abs excluded)",
        b.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: display:none children excluded                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_display_none_excluded() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div style='display:none;flex:1'>Hidden</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.w - 300.0).abs() < 10.0,
        "a={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (b.layout.content_rect.w - 300.0).abs() < 10.0,
        "b={:.0}",
        b.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: flex-basis vs width                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_basis_overrides_width() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex-basis:200px;width:100px'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // flex-basis takes priority over width
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 10.0,
        "flex-basis=200 overrides width=100 w={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn flex_basis_auto_uses_width() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex-basis:auto;width:200px'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 10.0,
        "flex-basis:auto uses width=200 w={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn flex_basis_percentage() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "<div id='a' style='flex-basis:25%'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 10.0,
        "flex-basis:25%%={:.0}",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: grow + shrink combined                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_grow_with_basis() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex:1 0 100px'>A</div>",
            "<div id='b' style='flex:2 0 100px'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // Free space = 600-200=400. A gets 1/3=133, B gets 2/3=267
    // A=233, B=367
    assert!(
        b.layout.content_rect.w > a.layout.content_rect.w * 1.4,
        "grow 2:1 a={:.0} b={:.0}",
        a.layout.content_rect.w,
        b.layout.content_rect.w
    );
}

#[test]
fn flex_shrink_proportional() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px'>",
            "<div id='a' style='flex:0 1 300px'>A</div>",
            "<div id='b' style='flex:0 2 300px'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // Overflow = 200. A shrinks 1/3=67→233, B shrinks 2/3=133→167
    assert!(
        a.layout.content_rect.w > b.layout.content_rect.w,
        "shrink 1:2 a={:.0} b={:.0} (a shrinks less)",
        a.layout.content_rect.w,
        b.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: min-width / max-width constraints                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_item_min_width_prevents_shrink() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px'>",
            "<div id='a' style='flex:1;min-width:250px'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        a.layout.content_rect.w >= 245.0,
        "min-width:250 w={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn flex_item_max_width_caps_grow() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "<div id='a' style='flex:1;max-width:300px'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        a.layout.content_rect.w <= 305.0,
        "max-width:300 w={:.0}",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: column direction sizing                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_column_with_height() {
    let d = load_html(
        concat!(
            "<div style='display:flex;flex-direction:column;width:300px;height:400px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div id='b' style='flex:2'>B</div>",
            "</div>",
        ),
        400.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // 1fr=133, 2fr=267
    assert!(
        (a.layout.content_rect.h - 133.0).abs() < 10.0,
        "col 1fr h={:.0}",
        a.layout.content_rect.h
    );
    assert!(
        (b.layout.content_rect.h - 267.0).abs() < 10.0,
        "col 2fr h={:.0}",
        b.layout.content_rect.h
    );
}

#[test]
fn flex_column_items_full_width() {
    let d = load_html(
        concat!(
            "<div style='display:flex;flex-direction:column;width:400px'>",
            "<div id='a'>A</div>",
            "<div id='b'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // Column items stretch to cross axis (full width)
    assert!(
        (a.layout.content_rect.w - 400.0).abs() < 10.0,
        "stretch w={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        b.layout.content_rect.y > a.layout.content_rect.y + 10.0,
        "b below a"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: wrap with gap                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_wrap_with_gap() {
    let d = load_html(
        concat!(
            "<div style='display:flex;flex-wrap:wrap;width:500px;gap:20px'>",
            "<div id='a' style='width:200px;height:50px'>A</div>",
            "<div id='b' style='width:200px;height:50px'>B</div>",
            "<div id='c' style='width:200px;height:50px'>C</div>",
            "</div>",
        ),
        600.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // 200+20+200=420 fits in 500. Third wraps: 200+20+200+20+200=640>500
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "a,b same line"
    );
    assert!(
        c.layout.content_rect.y > a.layout.content_rect.y + 60.0,
        "c wraps with gap c_y={:.0}",
        c.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: nested flex containers                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_nested() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "  <div id='left' style='display:flex;flex-direction:column;flex:1'>",
            "    <div id='top' style='height:50px'>Top</div>",
            "    <div id='bottom' style='flex:1'>Bottom</div>",
            "  </div>",
            "  <div id='right' style='width:200px'>Right</div>",
            "</div>",
        ),
        900.0,
    );
    let left = by_id(&d.root, "left").unwrap();
    let right = by_id(&d.root, "right").unwrap();
    let top = by_id(&d.root, "top").unwrap();
    let bottom = by_id(&d.root, "bottom").unwrap();
    assert!(
        (right.layout.content_rect.w - 200.0).abs() < 10.0,
        "right=200"
    );
    assert!(
        (left.layout.content_rect.w - 600.0).abs() < 10.0,
        "left=600"
    );
    assert!(
        bottom.layout.content_rect.y > top.layout.content_rect.y + 40.0,
        "bottom below top"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: justify-content variations                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_justify_space_between() {
    let d = load_html(
        concat!(
            "<div style='display:flex;justify-content:space-between;width:600px'>",
            "<div id='a' style='width:100px'>A</div>",
            "<div id='b' style='width:100px'>B</div>",
            "<div id='c' style='width:100px'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // A at left edge, C at right edge
    assert!(a.layout.content_rect.x < 20.0, "a at left");
    assert!(
        c.layout.content_rect.x + c.layout.content_rect.w > 580.0,
        "c at right"
    );
}

#[test]
fn flex_justify_center() {
    let d = load_html(
        concat!(
            "<div style='display:flex;justify-content:center;width:600px'>",
            "<div id='a' style='width:100px'>A</div>",
            "<div id='b' style='width:100px'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // Two 100px items centered in 600px → start at x=200
    assert!(
        (a.layout.content_rect.x - 200.0).abs() < 20.0,
        "centered x={:.0}",
        a.layout.content_rect.x
    );
}

#[test]
fn flex_justify_flex_end() {
    let d = load_html(
        concat!(
            "<div style='display:flex;justify-content:flex-end;width:600px'>",
            "<div id='a' style='width:100px'>A</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        a.layout.content_rect.x > 480.0,
        "flex-end x={:.0}",
        a.layout.content_rect.x
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: align-items variations                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_align_items_center() {
    let d = load_html(
        concat!(
            "<div style='display:flex;align-items:center;width:600px;height:200px'>",
            "<div id='a' style='height:50px;width:100px'>A</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // Centered vertically: y ≈ (200-50)/2 = 75
    let container = find(&d.root, &|b| b.style.display == Display::Flex).unwrap();
    let expected_y = container.layout.content_rect.y + (200.0 - 50.0) / 2.0;
    assert!(
        (a.layout.content_rect.y - expected_y).abs() < 10.0,
        "centered y={:.0} expected {:.0}",
        a.layout.content_rect.y,
        expected_y
    );
}

#[test]
fn flex_align_items_flex_end() {
    let d = load_html(
        concat!(
            "<div style='display:flex;align-items:flex-end;width:600px;height:200px'>",
            "<div id='a' style='height:50px;width:100px'>A</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let container = find(&d.root, &|b| b.style.display == Display::Flex).unwrap();
    let expected_y = container.layout.content_rect.y + 200.0 - 50.0;
    assert!(
        (a.layout.content_rect.y - expected_y).abs() < 10.0,
        "flex-end y={:.0} expected {:.0}",
        a.layout.content_rect.y,
        expected_y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: margin auto alignment                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_margin_auto_pushes_right() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='logo' style='width:100px'>Logo</div>",
            "<div style='margin-left:auto'></div>",
            "<div id='nav' style='width:200px'>Nav</div>",
            "</div>",
        ),
        700.0,
    );
    let logo = by_id(&d.root, "logo").unwrap();
    let nav = by_id(&d.root, "nav").unwrap();
    assert!(logo.layout.content_rect.x < 20.0, "logo at left");
    assert!(
        nav.layout.content_rect.x > 350.0,
        "nav pushed right by margin-auto"
    );
}

#[test]
fn flex_margin_auto_vertical_center() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px;height:200px'>",
            "<div id='a' style='width:100px;height:50px;margin:auto 0'>A</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let container = find(&d.root, &|b| b.style.display == Display::Flex).unwrap();
    let expected = container.layout.content_rect.y + 75.0;
    assert!(
        (a.layout.content_rect.y - expected).abs() < 10.0,
        "margin:auto centers y={:.0}",
        a.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: inline-flex                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inline_flex_shrinks_to_content() {
    let d = load_html(
        concat!(
            "<div style='width:800px'>",
            "<div id='if' style='display:inline-flex;gap:10px'>",
            "  <div style='width:50px;height:30px'>A</div>",
            "  <div style='width:50px;height:30px'>B</div>",
            "</div>",
            "</div>",
        ),
        900.0,
    );
    let f = by_id(&d.root, "if").unwrap();
    // inline-flex shrinks to content: 50+10+50 = 110
    assert!(
        f.layout.content_rect.w < 200.0,
        "inline-flex shrinks w={:.0}",
        f.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: flex shorthand values                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_shorthand_1() {
    // flex:1 = flex: 1 1 0%
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex:1'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "<div id='c' style='flex:1'>C</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - 200.0).abs() < 10.0,
        "flex:1 = 200px w={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn flex_shorthand_initial() {
    // flex:initial = flex: 0 1 auto
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='flex:initial;width:100px'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // flex:initial doesn't grow, keeps width:100px
    assert!(
        (a.layout.content_rect.w - 100.0).abs() < 10.0,
        "flex:initial w={:.0}",
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Header with logo + nav + actions               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn header_logo_nav_actions() {
    let d = load_html(
        concat!(
        "<style>",
        ".header { display:flex; align-items:center; width:1200px; height:60px; padding:0 20px; }",
        ".logo { width:120px; flex-shrink:0; }",
        ".nav { display:flex; flex:1; gap:20px; margin:0 40px; }",
        ".actions { display:flex; gap:10px; flex-shrink:0; }",
        ".btn { padding:8px 16px; }",
        "</style>",
        "<div class='header'>",
        "  <div class='logo' id='logo'>Logo</div>",
        "  <div class='nav'>",
        "    <a id='n1'>Home</a><a id='n2'>About</a><a id='n3'>Contact</a>",
        "  </div>",
        "  <div class='actions'>",
        "    <button class='btn' id='login'>Log In</button>",
        "    <button class='btn' id='signup'>Sign Up</button>",
        "  </div>",
        "</div>",
    ),
        1300.0,
    );
    let logo = by_id(&d.root, "logo").unwrap();
    let n1 = by_id(&d.root, "n1").unwrap();
    let login = by_id(&d.root, "login").unwrap();
    assert!(logo.layout.content_rect.x < 50.0, "logo at left");
    assert!(
        n1.layout.content_rect.x > logo.layout.content_rect.x + 100.0,
        "nav after logo"
    );
    assert!(
        login.layout.content_rect.x > n1.layout.content_rect.x + 50.0,
        "actions after nav"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Holy grail layout                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn holy_grail_layout() {
    let d = load_html(
        concat!(
            "<style>",
            ".page { display:flex;flex-direction:column;min-height:600px;width:1000px }",
            ".header,.footer { height:60px;flex-shrink:0 }",
            ".body { display:flex;flex:1 }",
            ".sidebar { width:200px;flex-shrink:0 }",
            ".content { flex:1 }",
            "</style>",
            "<div class='page'>",
            "  <div class='header' id='hdr'>Header</div>",
            "  <div class='body'>",
            "    <div class='sidebar' id='left'>Left</div>",
            "    <div class='content' id='main'>Main</div>",
            "    <div class='sidebar' id='right'>Right</div>",
            "  </div>",
            "  <div class='footer' id='ftr'>Footer</div>",
            "</div>",
        ),
        1100.0,
    );
    let hdr = by_id(&d.root, "hdr").unwrap();
    let left = by_id(&d.root, "left").unwrap();
    let main = by_id(&d.root, "main").unwrap();
    let right = by_id(&d.root, "right").unwrap();
    let ftr = by_id(&d.root, "ftr").unwrap();
    // Vertical stacking
    assert!(
        left.layout.content_rect.y > hdr.layout.content_rect.y + 50.0,
        "body below header"
    );
    assert!(
        ftr.layout.content_rect.y > left.layout.content_rect.y + 50.0,
        "footer below body"
    );
    // Horizontal: left | main | right
    assert!(
        main.layout.content_rect.x > left.layout.content_rect.x + 190.0,
        "main after left"
    );
    assert!(
        right.layout.content_rect.x > main.layout.content_rect.x + 100.0,
        "right after main"
    );
    assert!(
        (left.layout.content_rect.w - 200.0).abs() < 10.0,
        "sidebar=200"
    );
    assert!(main.layout.content_rect.w > 400.0, "main fills rest");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Card row                                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn card_row_equal_height() {
    let d = load_html(concat!(
        "<style>",
        ".cards { display:flex; gap:20px; width:960px; }",
        ".card { flex:1; display:flex; flex-direction:column; }",
        ".card-body { flex:1; padding:16px; }",
        ".card-footer { padding:8px 16px; }",
        "</style>",
        "<div class='cards'>",
        "  <div class='card' id='c1'><div class='card-body'>Short text</div><div class='card-footer'>Footer</div></div>",
        "  <div class='card' id='c2'><div class='card-body'>Much longer text content that takes up more vertical space in this card</div><div class='card-footer'>Footer</div></div>",
        "  <div class='card' id='c3'><div class='card-body'>Medium</div><div class='card-footer'>Footer</div></div>",
        "</div>",
    ), 1000.0);
    let c1 = by_id(&d.root, "c1").unwrap();
    let c2 = by_id(&d.root, "c2").unwrap();
    let c3 = by_id(&d.root, "c3").unwrap();
    // All cards same height (flex stretch)
    assert!(
        (c1.layout.content_rect.h - c2.layout.content_rect.h).abs() < 5.0,
        "equal height c1={:.0} c2={:.0}",
        c1.layout.content_rect.h,
        c2.layout.content_rect.h
    );
    assert!(
        (c2.layout.content_rect.h - c3.layout.content_rect.h).abs() < 5.0,
        "equal height c2={:.0} c3={:.0}",
        c2.layout.content_rect.h,
        c3.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLEX: overflow interaction                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_overflow_hidden_clips() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px;height:100px;overflow:hidden'>",
            "<div id='a' style='flex-shrink:0;width:300px;height:200px'>Tall</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // Content is 200px tall but container clips at 100px
    assert!(
        (a.layout.content_rect.h - 200.0).abs() < 5.0,
        "content still 200px in layout"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn flex_empty_container() {
    let d = load_html("<div style='display:flex;width:400px'></div>", 500.0);
    let f = find(&d.root, &|b| b.style.display == Display::Flex).unwrap();
    assert!(f.layout.content_rect.h >= 0.0, "empty flex");
}

#[test]
fn flex_single_item() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px'>",
            "<div id='only' style='flex:1;height:50px'>Only</div>",
            "</div>",
        ),
        500.0,
    );
    let only = by_id(&d.root, "only").unwrap();
    assert!(
        (only.layout.content_rect.w - 400.0).abs() < 5.0,
        "single flex:1 fills w={:.0}",
        only.layout.content_rect.w
    );
}

#[test]
fn flex_zero_basis_items() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:300px'>",
            "<div id='a' style='flex:1 0 0'>A</div>",
            "<div id='b' style='flex:1 0 0'>B</div>",
            "<div id='c' style='flex:1 0 0'>C</div>",
            "</div>",
        ),
        400.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - 100.0).abs() < 5.0,
        "flex:1 0 0 = 100px w={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn flex_all_items_grow_zero() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:600px'>",
            "<div id='a' style='width:100px'>A</div>",
            "<div id='b' style='width:100px'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // No grow → items stay at their widths, free space unused
    assert!(
        (a.layout.content_rect.w - 100.0).abs() < 10.0,
        "no grow a={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 90.0,
        "b after a"
    );
}
