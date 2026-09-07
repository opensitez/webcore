// Tests for CSS features that have caused real-world breakage:
// margin collapsing, box-sizing, viewport units, calc(), overflow,
// pseudo-elements, shorthands, visibility, opacity, and text properties.

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
// ║  MARGIN COLLAPSING                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn margin_collapse_adjacent_siblings() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='a' style='margin-bottom:30px;height:50px'>A</div>",
            "<div id='b' style='margin-top:20px;height:50px'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // Collapsed margin = max(30,20) = 30, not 50
    assert!(gap <= 35.0, "collapsed gap={:.0} should be ~30 not 50", gap);
}

#[test]
fn margin_collapse_parent_first_child() {
    let d = load_html(
        concat!(
            "<div id='parent' style='width:400px'>",
            "<div id='child' style='margin-top:40px;height:50px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let parent = by_id(&d.root, "parent").unwrap();
    let child = by_id(&d.root, "child").unwrap();
    // Parent has no border/padding → margin collapses through
    // Child margin-top should collapse with parent
    assert!(
        child.layout.content_rect.y >= 35.0,
        "child margin collapses through parent y={:.0}",
        child.layout.content_rect.y
    );
}

#[test]
fn margin_no_collapse_with_border() {
    let d = load_html(
        concat!(
            "<div id='parent' style='width:400px;border-top:1px solid black'>",
            "<div id='child' style='margin-top:40px;height:50px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let parent = by_id(&d.root, "parent").unwrap();
    let child = by_id(&d.root, "child").unwrap();
    // Border prevents margin collapsing
    let inner_gap = child.layout.content_rect.y - parent.layout.content_rect.y;
    assert!(
        inner_gap >= 38.0,
        "no collapse with border gap={:.0}",
        inner_gap
    );
}

#[test]
fn margin_no_collapse_with_padding() {
    let d = load_html(
        concat!(
            "<div id='parent' style='width:400px;padding-top:1px'>",
            "<div id='child' style='margin-top:40px;height:50px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let parent = by_id(&d.root, "parent").unwrap();
    let child = by_id(&d.root, "child").unwrap();
    let inner_gap = child.layout.content_rect.y - parent.layout.content_rect.y;
    assert!(
        inner_gap >= 38.0,
        "no collapse with padding gap={:.0}",
        inner_gap
    );
}

#[test]
fn margin_no_collapse_in_flex() {
    let d = load_html(
        concat!(
            "<div style='display:flex;flex-direction:column;width:400px'>",
            "<div id='a' style='margin-bottom:30px;height:50px'>A</div>",
            "<div id='b' style='margin-top:20px;height:50px'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let gap = b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h);
    // Flex doesn't collapse margins → gap = 30+20 = 50
    assert!(gap >= 45.0, "no collapse in flex gap={:.0}", gap);
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BOX-SIZING                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn box_sizing_content_box_default() {
    let d = load_html(
        "<div id='t' style='width:200px;padding:20px;border:5px solid black'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // content-box: width is content only → border_rect.w = 200+40+10 = 250
    assert!(
        (t.layout.content_rect.w - 200.0).abs() < 5.0,
        "content w=200 w={:.0}",
        t.layout.content_rect.w
    );
    assert!(
        t.layout.border_rect.w > 240.0,
        "border_rect includes padding+border w={:.0}",
        t.layout.border_rect.w
    );
}

#[test]
fn box_sizing_border_box() {
    let d = load_html(
        "<div id='t' style='width:200px;padding:20px;border:5px solid black;box-sizing:border-box'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // border-box: width includes padding+border → content = 200-40-10 = 150
    assert!(
        (t.layout.border_rect.w - 200.0).abs() < 5.0,
        "border-box border_rect w=200 w={:.0}",
        t.layout.border_rect.w
    );
    assert!(
        (t.layout.content_rect.w - 150.0).abs() < 5.0,
        "content w=150 w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn box_sizing_border_box_universal() {
    let d = load_html(
        concat!(
            "<style>*, *::before, *::after { box-sizing: border-box; }</style>",
            "<div id='t' style='width:300px;padding:30px'>Content</div>",
        ),
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.border_rect.w - 300.0).abs() < 5.0,
        "universal border-box w={:.0}",
        t.layout.border_rect.w
    );
    assert!(
        (t.layout.content_rect.w - 240.0).abs() < 5.0,
        "content=300-60 w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  VIEWPORT UNITS (vh, vw)                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn vh_unit_resolves() {
    let d = load_html(
        "<div id='t' style='height:50vh;width:400px'>Half viewport</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 50vh with default viewport height — should be > 0
    assert!(
        t.layout.content_rect.h > 100.0,
        "50vh h={:.0} should be substantial",
        t.layout.content_rect.h
    );
}

#[test]
fn vw_unit_resolves() {
    let d = load_html(
        "<div id='t' style='width:50vw;height:50px'>Half width</div>",
        1000.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 50vw of 1000px = 500px
    assert!(
        (t.layout.content_rect.w - 500.0).abs() < 10.0,
        "50vw w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn height_100vh_wrapper_children_overflow() {
    let d = load_html(
        concat!(
            "<div style='height:100vh;width:800px'>",
            "<div id='tall' style='height:2000px'>Tall content</div>",
            "</div>",
        ),
        900.0,
    );
    let tall = by_id(&d.root, "tall").unwrap();
    // Children can overflow 100vh wrapper
    assert!(
        (tall.layout.content_rect.h - 2000.0).abs() < 5.0,
        "tall keeps h=2000 h={:.0}",
        tall.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC()                                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_simple_subtraction() {
    let d = load_html(
        "<body style='margin:0'><div id='t' style='width:calc(100% - 40px);height:50px'>X</div></body>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // calc(100% - 40px) of 800px parent (no body margin) = 760
    assert!(
        (t.layout.content_rect.w - 760.0).abs() < 5.0,
        "calc w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_addition() {
    let d = load_html(
        "<div id='t' style='width:calc(200px + 100px);height:50px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 5.0,
        "calc add w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_in_padding() {
    let d = load_html(
        "<div id='t' style='padding:calc(10px + 5px);width:200px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_pad_top - 15.0).abs() < 3.0,
        "calc padding={:.0}",
        t.layout.resolved_pad_top
    );
}

#[test]
fn calc_nested_no_crash() {
    let d = load_html(
        "<div id='t' style='width:calc(calc(50% - 10px) + 20px);height:50px'>X</div>",
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  OVERFLOW                                                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn overflow_hidden_parsed() {
    let d = load_html(
        "<div id='t' style='overflow:hidden;width:200px;height:100px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(matches!(t.style.overflow_x, Overflow::Hidden));
    assert!(matches!(t.style.overflow_y, Overflow::Hidden));
}

#[test]
fn overflow_auto_parsed() {
    let d = load_html(
        "<div id='t' style='overflow:auto;width:200px;height:100px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(matches!(t.style.overflow_x, Overflow::Auto));
}

#[test]
fn overflow_xy_separate() {
    let d = load_html(
        "<div id='t' style='overflow-x:scroll;overflow-y:hidden;width:200px;height:100px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(matches!(t.style.overflow_x, Overflow::Scroll));
    assert!(matches!(t.style.overflow_y, Overflow::Hidden));
}

#[test]
fn overflow_hidden_creates_bfc() {
    let d = load_html(
        concat!(
            "<div id='bfc' style='overflow:hidden;width:400px'>",
            "<div style='float:left;width:100px;height:150px'>Float</div>",
            "</div>",
        ),
        500.0,
    );
    let bfc = by_id(&d.root, "bfc").unwrap();
    assert!(
        bfc.layout.content_rect.h >= 145.0,
        "overflow:hidden BFC h={:.0}",
        bfc.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  PSEUDO-ELEMENTS ::before / ::after                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_content_renders() {
    let d = load_html(
        concat!(
            "<style>p::before { content: '>> '; }</style>",
            "<p id='t'>Hello</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before"),
        "::before content exists"
    );
}

#[test]
fn after_content_renders() {
    let d = load_html(
        concat!(
            "<style>p::after { content: ' <<'; }</style>",
            "<p id='t'>Hello</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after"),
        "::after content exists"
    );
}

#[test]
fn before_with_display_block() {
    let d = load_html(
        concat!(
        "<style>h2::before { content: ''; display: block; height: 4px; background: red; }</style>",
        "<h2 id='t'>Title</h2>",
    ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Should have height for both ::before block and text
    assert!(
        t.layout.content_rect.h > 20.0,
        "h2 with block before h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn before_inherits_color() {
    let d = load_html(
        concat!(
            "<style>p { color: red; } p::before { content: '* '; }</style>",
            "<p id='t'>Hello</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert_eq!(bs.color.r, 255, "::before inherits red");
    }
}

#[test]
fn before_after_in_flex() {
    let d = load_html(
        concat!(
            "<style>.flex::before { content: 'B'; } .flex::after { content: 'A'; }</style>",
            "<div class='flex' style='display:flex;width:400px'>",
            "<div id='mid' style='flex:1'>Middle</div>",
            "</div>",
        ),
        500.0,
    );
    let mid = by_id(&d.root, "mid").unwrap();
    assert!(
        mid.layout.content_rect.w > 0.0,
        "flex with before/after mid.w={:.0}",
        mid.layout.content_rect.w
    );
}

/// Bootstrap-style navbar: ::before/::after clearfix on a flex container
/// with descendant selector setting child display:flex.
/// Reproduces younelan.com navbar regression where menu items stack vertically.
#[test]
fn bootstrap_navbar_flex_with_pseudo() {
    let d = load_html(
        concat!(
            "<style>",
            ".navbar { display: flex; width: 800px; }",
            ".navbar::before, .navbar::after { display: table; content: ' '; }",
            ".navbar::after { clear: both; }",
            ".navbar-expand .navbar-nav { display: flex; flex-direction: row; }",
            ".nav-item { padding: 0 10px; }",
            "</style>",
            "<nav class='navbar navbar-expand'>",
            "  <div class='container'>",
            "    <ul id='nav' class='navbar-nav'>",
            "      <li class='nav-item'><a>Home</a></li>",
            "      <li class='nav-item'><a>About</a></li>",
            "      <li class='nav-item'><a>Contact</a></li>",
            "    </ul>",
            "  </div>",
            "</nav>",
        ),
        800.0,
    );
    let nav = by_id(&d.root, "nav").unwrap();
    // navbar-nav should have display:flex from the descendant selector
    assert!(
        matches!(nav.style.display, Display::Flex),
        "navbar-nav should be flex, got {:?}",
        nav.style.display
    );
    // nav items should be laid out horizontally (second item x > first item x)
    let items: Vec<&WebCore> = nav.children.iter().filter(|c| c.tag == "li").collect();
    assert!(items.len() >= 2, "need at least 2 nav items");
    assert!(
        items[1].layout.content_rect.x > items[0].layout.content_rect.x,
        "nav items should be horizontal: item0.x={:.0} item1.x={:.0}",
        items[0].layout.content_rect.x,
        items[1].layout.content_rect.x
    );
}

/// Bootstrap navbar-expand-md with @media query: descendant selector inside media
/// query should apply display:flex to .navbar-nav when viewport >= 768px.
#[test]
fn bootstrap_navbar_media_query_descendant() {
    let d = load_html(
        concat!(
            "<style>",
            ".navbar { display: flex; }",
            ".navbar::before, .navbar::after { display: table; content: ' '; }",
            ".navbar-collapse { display: block; }",
            ".navbar-nav { display: block; }",
            "@media (min-width: 768px) {",
            "  .navbar-expand-md .navbar-nav { display: flex; flex-direction: row; }",
            "  .navbar-expand-md .navbar-collapse { display: flex; }",
            "}",
            "</style>",
            "<nav class='navbar navbar-expand-md' style='width:1000px'>",
            "  <div class='container-fluid'>",
            "    <div class='navbar-collapse'>",
            "      <ul id='nav' class='navbar-nav'>",
            "        <li class='nav-item'><a>Home</a></li>",
            "        <li class='nav-item'><a>About</a></li>",
            "        <li class='nav-item'><a>Contact</a></li>",
            "      </ul>",
            "    </div>",
            "  </div>",
            "</nav>",
        ),
        1000.0,
    );
    let nav = by_id(&d.root, "nav").unwrap();
    assert!(
        matches!(nav.style.display, Display::Flex),
        "navbar-nav should be flex via @media descendant rule, got {:?}",
        nav.style.display
    );
}

/// Pseudo-element with position:absolute should not disrupt parent layout.
/// The 1x1 accessibility skip-link pattern should remain tiny.
#[test]
fn pseudo_absolute_tiny() {
    let d = load_html(
        concat!(
            "<style>",
            ".container::before { content: ''; display: block; position: absolute; ",
            "  width: 1px; height: 1px; margin: -1px; overflow: hidden; }",
            "</style>",
            "<div class='container' style='width:400px'>",
            "  <div id='content'>Hello World</div>",
            "</div>",
        ),
        800.0,
    );
    let content = by_id(&d.root, "content").unwrap();
    // content should start near the top, not pushed down by a giant ::before
    assert!(
        content.layout.content_rect.y < 50.0,
        "content pushed down to y={:.0} by ::before",
        content.layout.content_rect.y
    );
    assert!(
        content.layout.content_rect.w > 100.0,
        "content should have width, got {:.0}",
        content.layout.content_rect.w
    );
}

/// ::before in flex should not inherit parent's width
#[test]
fn pseudo_no_inherit_parent_width() {
    let d = load_html(
        concat!(
            "<style>.box::before { content: 'X'; }</style>",
            "<div class='box' style='display:flex;width:500px'>",
            "  <div id='main' style='flex:1'>Content</div>",
            "</div>",
        ),
        800.0,
    );
    // The ::before should be tiny (just the text 'X'), not 500px
    let container = find(&d.root, &|n| {
        n.attributes
            .get("class")
            .map(|c| c == "box")
            .unwrap_or(false)
    })
    .unwrap();
    let before = container.children.iter().find(|c| c.tag == "::before");
    if let Some(b) = before {
        assert!(
            b.layout.content_rect.w < 100.0,
            "::before should be small, got w={:.0}",
            b.layout.content_rect.w
        );
    }
    let main = by_id(&d.root, "main").unwrap();
    assert!(
        main.layout.content_rect.w > 200.0,
        "main should get most of the width with flex:1, got {:.0}",
        main.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CSS SHORTHANDS                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn margin_shorthand_one_value() {
    let d = load_html(
        "<div id='t' style='margin:20px;width:100px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_margin_top - 20.0).abs() < 2.0,
        "m-top={:.0}",
        t.layout.resolved_margin_top
    );
    assert!(
        (t.layout.resolved_margin_left - 20.0).abs() < 2.0,
        "m-left={:.0}",
        t.layout.resolved_margin_left
    );
}

#[test]
fn margin_shorthand_two_values() {
    let d = load_html(
        "<div id='t' style='margin:10px 30px;width:100px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_margin_top - 10.0).abs() < 2.0,
        "m-top={:.0}",
        t.layout.resolved_margin_top
    );
    assert!(
        (t.layout.resolved_margin_left - 30.0).abs() < 2.0,
        "m-left={:.0}",
        t.layout.resolved_margin_left
    );
}

#[test]
fn padding_shorthand_four_values() {
    let d = load_html(
        "<div id='t' style='padding:10px 20px 30px 40px;width:100px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_pad_top - 10.0).abs() < 2.0,
        "p-top={:.0}",
        t.layout.resolved_pad_top
    );
    assert!(
        (t.layout.resolved_pad_right - 20.0).abs() < 2.0,
        "p-right={:.0}",
        t.layout.resolved_pad_right
    );
    assert!(
        (t.layout.resolved_pad_bottom - 30.0).abs() < 2.0,
        "p-bottom={:.0}",
        t.layout.resolved_pad_bottom
    );
    assert!(
        (t.layout.resolved_pad_left - 40.0).abs() < 2.0,
        "p-left={:.0}",
        t.layout.resolved_pad_left
    );
}

#[test]
fn border_shorthand() {
    let d = load_html(
        "<div id='t' style='border:3px solid red;width:200px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_border_top - 3.0).abs() < 1.0,
        "border-top={:.0}",
        t.layout.resolved_border_top
    );
    assert!(
        (t.layout.resolved_border_right - 3.0).abs() < 1.0,
        "border-right={:.0}",
        t.layout.resolved_border_right
    );
}

#[test]
fn background_shorthand_color() {
    let d = load_html(
        "<div id='t' style='background:red;width:100px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.background_color.r, 255, "bg red");
    assert!(t.style.background_color.a > 0, "bg opaque");
}

#[test]
fn font_shorthand() {
    let d = load_html(
        "<div id='t' style='font:bold 20px/1.5 Arial,sans-serif'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 20.0).abs() < 2.0,
        "font-size=20 fs={:.0}",
        t.style.font_size_px(16.0, 16.0)
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  VISIBILITY vs DISPLAY:NONE                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn visibility_hidden_takes_space() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='before' style='height:50px'>Before</div>",
            "<div id='hidden' style='visibility:hidden;height:80px'>Hidden</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    let hidden = by_id(&d.root, "hidden").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    // visibility:hidden takes space
    assert!(
        (hidden.layout.content_rect.h - 80.0).abs() < 5.0,
        "hidden has height"
    );
    assert!(
        after.layout.content_rect.y >= hidden.layout.content_rect.y + 75.0,
        "after below hidden"
    );
}

#[test]
fn display_none_vs_visibility_hidden() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div style='display:none;height:100px'>None</div>",
            "<div id='after_none' style='height:30px'>After none</div>",
            "<div style='visibility:hidden;height:100px'>Hidden</div>",
            "<div id='after_hidden' style='height:30px'>After hidden</div>",
            "</div>",
        ),
        500.0,
    );
    let an = by_id(&d.root, "after_none").unwrap();
    let ah = by_id(&d.root, "after_hidden").unwrap();
    // after_none starts early (none takes no space)
    // after_hidden starts much later (hidden takes space)
    assert!(
        ah.layout.content_rect.y > an.layout.content_rect.y + 80.0,
        "visibility:hidden takes more space than display:none"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  OPACITY                                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn opacity_parsed() {
    let d = load_html("<div id='t' style='opacity:0.5'>Half</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.style.opacity - 0.5).abs() < 0.01,
        "opacity=0.5 got {:.2}",
        t.style.opacity
    );
}

#[test]
fn opacity_zero_takes_space() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='invisible' style='opacity:0;height:80px'>Invisible</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    let invisible = by_id(&d.root, "invisible").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    assert!(
        (invisible.layout.content_rect.h - 80.0).abs() < 5.0,
        "opacity:0 has height"
    );
    assert!(
        after.layout.content_rect.y >= invisible.layout.content_rect.y + 75.0,
        "after below"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TEXT PROPERTIES                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn text_align_center() {
    let d = load_html(
        "<div id='t' style='text-align:center;width:400px'>Centered</div>",
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(!t.layout.line_cache.is_empty());
    let line = &t.layout.line_cache[0];
    // Centered text: line.x should be offset from left
    assert!(
        line.x > t.layout.content_rect.x + 50.0,
        "centered text offset x={:.0}",
        line.x
    );
}

#[test]
fn text_align_right() {
    let d = load_html(
        "<div id='t' style='text-align:right;width:400px'>Right</div>",
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(!t.layout.line_cache.is_empty());
    let line = &t.layout.line_cache[0];
    let right_edge = t.layout.content_rect.x + t.layout.content_rect.w;
    assert!(
        (line.x + line.width - right_edge).abs() < 5.0,
        "right-aligned"
    );
}

#[test]
fn text_transform_uppercase() {
    let d = load_html(
        "<div id='t' style='text-transform:uppercase'>hello world</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.text_transform, TextTransform::Uppercase);
}

#[test]
fn text_decoration_underline() {
    let d = load_html(
        "<div id='t' style='text-decoration:underline'>Underlined</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.style.text_decoration.underline, "underline set");
}

#[test]
fn line_height_unitless() {
    let d = load_html("<div id='t' style='font-size:16px;line-height:1.5;width:200px'>Line height test with enough text to wrap</div>", 300.0);
    let t = by_id(&d.root, "t").unwrap();
    if t.layout.line_cache.len() >= 2 {
        let gap = t.layout.line_cache[1].y - t.layout.line_cache[0].y;
        assert!(
            (gap - 24.0).abs() < 5.0,
            "line-height 1.5*16=24 gap={:.0}",
            gap
        );
    }
}

#[test]
fn letter_spacing() {
    let d = load_html(
        "<div id='t' style='letter-spacing:5px;width:400px'>Spaced out text</div>",
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(!t.layout.line_cache.is_empty());
    // With letter-spacing, line should be wider than without
    assert!(
        t.layout.line_cache[0].width > 100.0,
        "letter-spacing adds width w={:.0}",
        t.layout.line_cache[0].width
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  MIN/MAX WIDTH/HEIGHT                                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn min_width() {
    let d = load_html("<div id='t' style='min-width:300px'>Short</div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w >= 295.0,
        "min-width:300 w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn max_width() {
    let d = load_html("<div id='t' style='max-width:200px'>Content</div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w <= 205.0,
        "max-width:200 w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn min_height() {
    let d = load_html(
        "<div id='t' style='min-height:100px;width:200px'>Short</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h >= 95.0,
        "min-height:100 h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn max_height() {
    let d = load_html("<div id='t' style='max-height:50px;width:200px;overflow:hidden'>Very tall content that should be clipped by max-height</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h <= 55.0,
        "max-height:50 h={:.0}",
        t.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SCROLL HEIGHT                                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn scroll_height_exceeds_100vh_wrapper() {
    let d = load_html(
        concat!(
            "<div style='height:100vh;width:800px'>",
            "<div style='height:3000px'>Tall</div>",
            "</div>",
        ),
        900.0,
    );
    let sh = Document::scroll_height(&d.root);
    assert!(
        sh > 2000.0,
        "scroll_height={:.0} should exceed 100vh wrapper",
        sh
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COLORS                                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn color_hex_short() {
    let d = load_html("<div id='t' style='color:#f00'>Red</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.r, 255, "short hex red");
}

#[test]
fn color_hex_long() {
    let d = load_html("<div id='t' style='color:#00ff00'>Green</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.g, 255, "long hex green");
}

#[test]
fn color_rgb_function() {
    let d = load_html("<div id='t' style='color:rgb(0,0,255)'>Blue</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.b, 255, "rgb blue");
}

#[test]
fn color_rgba_function() {
    let d = load_html(
        "<div id='t' style='background-color:rgba(255,0,0,0.5)'>Half red</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.background_color.r, 255, "rgba red");
    assert!(
        t.style.background_color.a < 200 && t.style.background_color.a > 100,
        "rgba alpha ~128 a={}",
        t.style.background_color.a
    );
}

#[test]
fn color_named() {
    let d = load_html("<div id='t' style='color:blue'>Blue</div>", 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.b, 255, "named blue");
}

#[test]
fn color_hsl() {
    let d = load_html(
        "<div id='t' style='color:hsl(0,100%,50%)'>Red via HSL</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.style.color.r > 200, "hsl red r={}", t.style.color.r);
}
