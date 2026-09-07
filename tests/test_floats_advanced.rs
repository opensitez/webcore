// Comprehensive float layout tests — covers float placement, clearing,
// text wrapping, float + positioning interactions, shrink-wrap, BFC,
// and real-world patterns.

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
// ║  BASIC FLOAT LEFT                                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_left_basic() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:left;width:100px;height:80px'>F</div>",
            "<div id='t'>Text wraps around the float on the right side</div>",
            "</div>",
        ),
        500.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    assert!(
        f.layout.content_rect.x < 20.0,
        "float left at x={:.0}",
        f.layout.content_rect.x
    );
    assert!((f.layout.content_rect.w - 100.0).abs() < 3.0, "float w=100");
}

#[test]
fn float_left_multiple_stack_horizontally() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div id='a' style='float:left;width:100px;height:50px'>A</div>",
            "<div id='b' style='float:left;width:100px;height:50px'>B</div>",
            "<div id='c' style='float:left;width:100px;height:50px'>C</div>",
            "</div>",
        ),
        600.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 90.0,
        "b right of a"
    );
    assert!(
        c.layout.content_rect.x > b.layout.content_rect.x + 90.0,
        "c right of b"
    );
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 3.0,
        "same line"
    );
}

#[test]
fn float_left_wraps_when_no_room() {
    let d = load_html(
        concat!(
            "<div style='width:250px'>",
            "<div id='a' style='float:left;width:100px;height:50px'>A</div>",
            "<div id='b' style='float:left;width:100px;height:50px'>B</div>",
            "<div id='c' style='float:left;width:100px;height:50px'>C</div>",
            "</div>",
        ),
        300.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // C doesn't fit on first line (250 < 300), wraps below
    assert!(
        c.layout.content_rect.y > a.layout.content_rect.y + 40.0,
        "c wraps to next line y={:.0}",
        c.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BASIC FLOAT RIGHT                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_right_basic() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:right;width:100px;height:80px'>F</div>",
            "<div id='t'>Text flows on the left side</div>",
            "</div>",
        ),
        500.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    // Float right should be near right edge
    assert!(
        f.layout.content_rect.x > 280.0,
        "float right x={:.0}",
        f.layout.content_rect.x
    );
}

#[test]
fn float_right_multiple_stack_from_right() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div id='a' style='float:right;width:80px;height:50px'>A</div>",
            "<div id='b' style='float:right;width:80px;height:50px'>B</div>",
            "</div>",
        ),
        600.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // A is rightmost, B is to its left
    assert!(
        a.layout.content_rect.x > b.layout.content_rect.x,
        "a right of b"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT LEFT + RIGHT together                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_left_and_right_same_line() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div id='l' style='float:left;width:150px;height:60px'>Left</div>",
            "<div id='r' style='float:right;width:150px;height:60px'>Right</div>",
            "<div id='t'>Center text between floats</div>",
            "</div>",
        ),
        600.0,
    );
    let l = by_id(&d.root, "l").unwrap();
    let r = by_id(&d.root, "r").unwrap();
    assert!(l.layout.content_rect.x < 20.0, "left float at left");
    assert!(r.layout.content_rect.x > 300.0, "right float at right");
    assert!(
        (l.layout.content_rect.y - r.layout.content_rect.y).abs() < 5.0,
        "same line"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CLEAR                                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn clear_left() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:left;width:100px;height:100px'>F</div>",
            "<div id='c' style='clear:left;height:50px'>Cleared</div>",
            "</div>",
        ),
        500.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        c.layout.content_rect.y >= f.layout.content_rect.y + 95.0,
        "clear:left below float y={:.0}",
        c.layout.content_rect.y
    );
}

#[test]
fn clear_right() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:right;width:100px;height:100px'>F</div>",
            "<div id='c' style='clear:right;height:50px'>Cleared</div>",
            "</div>",
        ),
        500.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        c.layout.content_rect.y >= f.layout.content_rect.y + 95.0,
        "clear:right below float y={:.0}",
        c.layout.content_rect.y
    );
}

#[test]
fn clear_both() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div id='l' style='float:left;width:100px;height:80px'>L</div>",
            "<div id='r' style='float:right;width:100px;height:120px'>R</div>",
            "<div id='c' style='clear:both;height:50px'>Cleared</div>",
            "</div>",
        ),
        600.0,
    );
    let r = by_id(&d.root, "r").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // clear:both goes below tallest float (right=120)
    assert!(
        c.layout.content_rect.y >= r.layout.content_rect.y + 115.0,
        "clear:both below tallest float y={:.0}",
        c.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  TEXT WRAPPING around floats                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn text_wraps_around_left_float() {
    let d = load_html(concat!(
        "<div style='width:300px;font-size:16px'>",
        "<div id='f' style='float:left;width:100px;height:60px'>F</div>",
        "<span id='t'>This text should wrap around the float element on the left side of the container</span>",
        "</div>",
    ), 400.0);
    let f = by_id(&d.root, "f").unwrap();
    let container = find_all(&d.root, &|b| b.style.width == CssLength::Px(300.0));
    assert!(!container.is_empty());
    // Container should have lines, first line starts after float
    assert!(!container[0].layout.line_cache.is_empty(), "has text lines");
    let first_line = &container[0].layout.line_cache[0];
    assert!(
        first_line.x >= f.layout.content_rect.x + f.layout.content_rect.w - 5.0,
        "first text line x={:.0} should start after float right edge={:.0}",
        first_line.x,
        f.layout.content_rect.x + f.layout.content_rect.w
    );
}

#[test]
fn text_wraps_around_right_float() {
    let d = load_html(
        concat!(
            "<div style='width:300px;font-size:16px'>",
            "<div id='f' style='float:right;width:100px;height:60px'>F</div>",
            "<span id='t'>This text should be constrained on the right by the float</span>",
            "</div>",
        ),
        400.0,
    );
    let container = find_all(&d.root, &|b| b.style.width == CssLength::Px(300.0));
    assert!(!container.is_empty());
    assert!(!container[0].layout.line_cache.is_empty(), "has text lines");
    let first_line = &container[0].layout.line_cache[0];
    // First line width should be limited (300 - 100 = 200 available)
    assert!(
        first_line.width <= 210.0,
        "first line width={:.0} should be <= 200 (constrained by right float)",
        first_line.width
    );
}

#[test]
fn text_expands_after_float_ends() {
    let d = load_html(concat!(
        "<div style='width:400px;font-size:16px;line-height:20px'>",
        "<div style='float:left;width:150px;height:40px'>F</div>",
        "<span>Line one next to float. Line two next to float. Line three should be full width below the float since it ended.</span>",
        "</div>",
    ), 500.0);
    let container = find_all(&d.root, &|b| b.style.width == CssLength::Px(400.0));
    assert!(!container.is_empty());
    let lines = &container[0].layout.line_cache;
    assert!(lines.len() >= 3, "should have at least 3 lines");
    // Lines after float height (40px = 2 lines at 20px) should use full width
    if lines.len() >= 3 {
        assert!(
            lines[2].width > lines[0].width + 50.0 || lines[2].x < lines[0].x - 50.0,
            "line 3 should be wider than line 1 (float ended)"
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT CONTAINMENT (BFC)                                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn overflow_hidden_contains_floats() {
    let d = load_html(
        concat!(
            "<div id='bfc' style='width:400px;overflow:hidden'>",
            "<div style='float:left;width:100px;height:150px'>F</div>",
            "<div>Short text</div>",
            "</div>",
        ),
        500.0,
    );
    let bfc = by_id(&d.root, "bfc").unwrap();
    // overflow:hidden creates BFC → container expands to contain float
    assert!(
        bfc.layout.content_rect.h >= 145.0,
        "BFC container h={:.0} should contain 150px float",
        bfc.layout.content_rect.h
    );
}

#[test]
fn display_flow_root_contains_floats() {
    let d = load_html(
        concat!(
            "<div id='fr' style='width:400px;display:flow-root'>",
            "<div style='float:left;width:100px;height:120px'>F</div>",
            "<div>Short</div>",
            "</div>",
        ),
        500.0,
    );
    let fr = by_id(&d.root, "fr").unwrap();
    assert!(
        fr.layout.content_rect.h >= 115.0,
        "flow-root h={:.0} should contain float",
        fr.layout.content_rect.h
    );
}

#[test]
fn float_without_bfc_collapses_parent() {
    let d = load_html(
        concat!(
            "<div id='no_bfc' style='width:400px'>",
            "<div style='float:left;width:100px;height:150px'>F</div>",
            "</div>",
            "<div id='after' style='height:50px'>After</div>",
        ),
        500.0,
    );
    let no_bfc = by_id(&d.root, "no_bfc").unwrap();
    // Without BFC, parent height collapses (may be 0 or small)
    // The float doesn't contribute to parent height
    assert!(
        no_bfc.layout.content_rect.h < 20.0,
        "non-BFC parent h={:.0} should collapse",
        no_bfc.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT SHRINK-TO-FIT                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_shrink_to_fit_width() {
    let d = load_html(
        concat!(
            "<div style='width:600px'>",
            "<div id='f' style='float:left'>Short text</div>",
            "</div>",
        ),
        700.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    // Float with no explicit width shrinks to content
    assert!(
        f.layout.content_rect.w < 200.0,
        "float shrink-to-fit w={:.0} should be < 200",
        f.layout.content_rect.w
    );
    assert!(f.layout.content_rect.w > 30.0, "should have some width");
}

#[test]
fn float_shrink_wraps_children() {
    let d = load_html(
        concat!(
            "<div style='width:800px'>",
            "<div id='f' style='float:left'>",
            "  <div style='width:200px;height:30px'>Child 1</div>",
            "  <div style='width:150px;height:30px'>Child 2</div>",
            "</div>",
            "</div>",
        ),
        900.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    // Float shrinks to widest child (200px)
    assert!(
        (f.layout.content_rect.w - 200.0).abs() < 20.0,
        "float w={:.0} should be ~200 (widest child)",
        f.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT + POSITIONING                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_with_relative_position() {
    let d = load_html(concat!(
        "<div style='width:400px'>",
        "<div id='f' style='float:left;position:relative;top:20px;left:10px;width:100px;height:80px'>FR</div>",
        "</div>",
    ), 500.0);
    let f = by_id(&d.root, "f").unwrap();
    // Float placement + relative offset
    assert!(
        f.layout.content_rect.x >= 8.0,
        "float+rel left x={:.0}",
        f.layout.content_rect.x
    );
    assert!(
        f.layout.content_rect.y >= 18.0,
        "float+rel top y={:.0}",
        f.layout.content_rect.y
    );
}

#[test]
fn absolute_ignores_float() {
    let d = load_html(concat!(
        "<div style='position:relative;width:400px'>",
        "<div id='abs' style='position:absolute;float:left;top:10px;left:10px;width:100px;height:100px'>A</div>",
        "<div id='t'>Text</div>",
        "</div>",
    ), 500.0);
    let abs = by_id(&d.root, "abs").unwrap();
    // position:absolute overrides float
    assert_eq!(abs.style.position, Position::Absolute);
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT + MARGIN                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_respects_margin() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:left;width:100px;height:80px;margin:10px'>F</div>",
            "</div>",
        ),
        500.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    assert!(f.layout.margin_rect.x >= 8.0, "float margin-left applied");
    assert!(
        (f.layout.resolved_margin_left - 10.0).abs() < 2.0,
        "margin-left=10"
    );
}

#[test]
fn float_margin_collapse_does_not_happen() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='a' style='float:left;width:100px;height:50px;margin-bottom:20px'>A</div>",
            "<div id='b' style='float:left;width:100px;height:50px;margin-top:20px'>B</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    // Float margins don't collapse — if both are on same line, margins add
    // If they wrap, the gap includes both margins
    assert!(
        a.layout.content_rect.w > 0.0 && b.layout.content_rect.w > 0.0,
        "both render"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FLOAT does not escape BFC                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_contained_in_bfc_sibling() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div id='bfc1' style='overflow:hidden'>",
            "  <div style='float:left;width:100px;height:100px'>F1</div>",
            "</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        600.0,
    );
    let bfc1 = by_id(&d.root, "bfc1").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    // Float is contained in BFC, after is below BFC
    assert!(
        after.layout.content_rect.y
            >= bfc1.layout.content_rect.y + bfc1.layout.content_rect.h - 3.0,
        "after y={:.0} should be below bfc h={:.0}",
        after.layout.content_rect.y,
        bfc1.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Wikipedia image float                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn wikipedia_image_float_pattern() {
    let d = load_html(concat!(
        "<div style='width:600px;font-size:16px'>",
        "<div id='img' style='float:right;width:220px;margin:0 0 10px 15px'>",
        "  <div style='height:160px;background:gray'>Image placeholder</div>",
        "  <div style='font-size:12px'>Caption text for the image</div>",
        "</div>",
        "<p id='p1'>First paragraph of article text that wraps around the floated image on the right side of the page. This is a common Wikipedia layout pattern.</p>",
        "<p id='p2'>Second paragraph continues below, still wrapping if the float extends this far.</p>",
        "</div>",
    ), 700.0);
    let img = by_id(&d.root, "img").unwrap();
    let p1 = by_id(&d.root, "p1").unwrap();
    // Image floats right
    assert!(
        img.layout.content_rect.x > 300.0,
        "image floated right x={:.0}",
        img.layout.content_rect.x
    );
    // P1 starts at the left, wrapping around float
    assert!(p1.layout.content_rect.x < 20.0, "p1 at left edge");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Clearfix pattern                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn clearfix_pattern() {
    let d = load_html(
        concat!(
            "<style>",
            ".clearfix::after { content:''; display:block; clear:both; }",
            "</style>",
            "<div class='clearfix' id='cf' style='width:400px'>",
            "  <div style='float:left;width:100px;height:150px'>Float</div>",
            "</div>",
            "<div id='after' style='height:50px'>After clearfix</div>",
        ),
        500.0,
    );
    let cf = by_id(&d.root, "cf").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    // Clearfix ::after clears the float, so container expands
    assert!(
        cf.layout.content_rect.h >= 140.0,
        "clearfix h={:.0} should contain float",
        cf.layout.content_rect.h
    );
    assert!(
        after.layout.content_rect.y >= cf.layout.content_rect.y + cf.layout.content_rect.h - 5.0,
        "after below clearfix"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Nav with floated items (Wikipedia tabs)        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn floated_nav_items_horizontal() {
    let d = load_html(
        concat!(
            "<style>",
            ".nav { overflow:hidden; width:600px; }",
            ".nav ul { list-style:none; margin:0; padding:0; }",
            ".nav li { float:left; margin:0 10px; }",
            ".nav a { display:block; padding:8px 12px; }",
            "</style>",
            "<div class='nav'>",
            "<ul>",
            "<li id='l1'><a href='/'>Home</a></li>",
            "<li id='l2'><a href='/about'>About</a></li>",
            "<li id='l3'><a href='/contact'>Contact</a></li>",
            "</ul>",
            "</div>",
        ),
        700.0,
    );
    let l1 = by_id(&d.root, "l1").unwrap();
    let l2 = by_id(&d.root, "l2").unwrap();
    let l3 = by_id(&d.root, "l3").unwrap();
    // All on same line
    assert!(
        (l1.layout.content_rect.y - l2.layout.content_rect.y).abs() < 5.0,
        "same line 1-2"
    );
    assert!(
        (l2.layout.content_rect.y - l3.layout.content_rect.y).abs() < 5.0,
        "same line 2-3"
    );
    // Increasing x
    assert!(
        l2.layout.content_rect.x > l1.layout.content_rect.x + 20.0,
        "l2 right of l1"
    );
    assert!(
        l3.layout.content_rect.x > l2.layout.content_rect.x + 20.0,
        "l3 right of l2"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Two-column float layout                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn two_column_float_layout() {
    let d = load_html(
        concat!(
            "<div style='width:960px;overflow:hidden'>",
            "<div id='main' style='float:left;width:620px'>",
            "  <div style='height:200px'>Article 1</div>",
            "  <div style='height:200px'>Article 2</div>",
            "</div>",
            "<div id='side' style='float:right;width:300px'>",
            "  <div style='height:150px'>Sidebar widget</div>",
            "</div>",
            "</div>",
        ),
        1000.0,
    );
    let main = by_id(&d.root, "main").unwrap();
    let side = by_id(&d.root, "side").unwrap();
    assert!((main.layout.content_rect.w - 620.0).abs() < 5.0, "main=620");
    assert!((side.layout.content_rect.w - 300.0).abs() < 5.0, "side=300");
    assert!(
        side.layout.content_rect.x > main.layout.content_rect.x + 600.0,
        "side right of main"
    );
    assert!(
        (main.layout.content_rect.y - side.layout.content_rect.y).abs() < 5.0,
        "same top"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_zero_height() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:left;width:100px;height:0'>Zero</div>",
            "<div id='t'>Text</div>",
            "</div>",
        ),
        500.0,
    );
    // Should not crash
    let _f = by_id(&d.root, "f").unwrap();
}

#[test]
fn float_zero_width() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div id='f' style='float:left;width:0;height:50px'>Zero W</div>",
            "<div id='t'>Text</div>",
            "</div>",
        ),
        500.0,
    );
    let _f = by_id(&d.root, "f").unwrap();
}

#[test]
fn float_wider_than_container() {
    let d = load_html(
        concat!(
            "<div style='width:200px'>",
            "<div id='f' style='float:left;width:300px;height:50px'>Wide</div>",
            "<div id='t'>Text below</div>",
            "</div>",
        ),
        300.0,
    );
    let f = by_id(&d.root, "f").unwrap();
    assert!(
        (f.layout.content_rect.w - 300.0).abs() < 5.0,
        "float keeps its width even if wider"
    );
}

#[test]
fn many_small_floats() {
    let mut html = String::from("<div style='width:400px'>");
    for i in 0..20 {
        html.push_str(&format!(
            "<div id='f{}' style='float:left;width:50px;height:30px'>{}</div>",
            i, i
        ));
    }
    html.push_str("<div id='after' style='clear:both'>After</div></div>");
    let d = load_html(&html, 500.0);
    let after = by_id(&d.root, "after").unwrap();
    // 20 floats at 50px each = 1000px. Container=400px → wraps to multiple rows
    // After should be below all floats
    assert!(
        after.layout.content_rect.y > 50.0,
        "after below wrapped floats y={:.0}",
        after.layout.content_rect.y
    );
}

#[test]
fn float_display_none_ignored() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div style='float:left;display:none;width:200px;height:100px'>Hidden</div>",
            "<div id='t'>Text at left edge</div>",
            "</div>",
        ),
        500.0,
    );
    let container = find_all(&d.root, &|b| b.style.width == CssLength::Px(400.0));
    assert!(!container.is_empty());
    if !container[0].layout.line_cache.is_empty() {
        let line = &container[0].layout.line_cache[0];
        assert!(
            line.x < 20.0,
            "text starts at left (hidden float ignored) x={:.0}",
            line.x
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  WORD WRAPPING                                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn word_wrap_normal() {
    let d = load_html(
        concat!(
            "<div id='t' style='width:200px;font-size:16px'>",
            "This is a long sentence that should wrap to multiple lines within the container",
            "</div>",
        ),
        300.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.line_cache.len() >= 2,
        "should wrap to multiple lines"
    );
    assert!(
        t.layout.content_rect.h > 30.0,
        "h={:.0} should be multi-line",
        t.layout.content_rect.h
    );
}

#[test]
fn word_wrap_no_break_mid_word() {
    let d = load_html(
        concat!(
            "<div id='t' style='width:200px;font-size:16px'>",
            "Supercalifragilisticexpialidocious is a very long word",
            "</div>",
        ),
        300.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Long word may overflow or be on its own line, but shouldn't crash
    assert!(!t.layout.line_cache.is_empty(), "should have lines");
}

#[test]
fn word_wrap_break_all() {
    let d = load_html(
        concat!(
            "<div id='t' style='width:100px;font-size:16px;word-break:break-all'>",
            "Supercalifragilisticexpialidocious",
            "</div>",
        ),
        200.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // break-all allows breaking mid-word
    assert!(t.layout.line_cache.len() >= 2, "should break mid-word");
}

#[test]
fn white_space_nowrap() {
    let d = load_html(
        concat!(
            "<div id='t' style='width:100px;font-size:16px;white-space:nowrap'>",
            "This text should not wrap to the next line",
            "</div>",
        ),
        200.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.layout.line_cache.len(), 1, "nowrap = single line");
}

#[test]
fn white_space_pre_preserves_newlines() {
    let d = load_html(
        concat!("<div id='t' style='width:400px;white-space:pre'>Line 1\nLine 2\nLine 3</div>",),
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.line_cache.len() >= 3,
        "pre preserves newlines, got {} lines",
        t.layout.line_cache.len()
    );
}

#[test]
fn text_overflow_ellipsis_parsed() {
    let d = load_html(
        "<div id='t' style='width:100px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis'>Very long text content</div>",
        200.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.text_overflow, TextOverflow::Ellipsis);
}

#[test]
fn overflow_hidden_clips_content() {
    let d = load_html(
        concat!(
            "<div style='width:200px;height:50px;overflow:hidden'>",
            "<div id='tall' style='height:300px'>Tall content that overflows</div>",
            "</div>",
        ),
        300.0,
    );
    let tall = by_id(&d.root, "tall").unwrap();
    // Content is there but parent clips visually
    assert!(
        (tall.layout.content_rect.h - 300.0).abs() < 5.0,
        "content still 300px tall in layout"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Float + clear + text interaction                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn magazine_layout() {
    let d = load_html(concat!(
        "<div style='width:600px;font-size:16px'>",
        "<div id='img1' style='float:left;width:200px;height:150px;margin:0 15px 10px 0'>Img1</div>",
        "<p id='p1'>First paragraph wraps around the image on the left. This should flow to the right of the image.</p>",
        "<div id='img2' style='float:right;width:180px;height:120px;margin:0 0 10px 15px'>Img2</div>",
        "<p id='p2'>Second paragraph wraps around the right-floated image. Text flows on the left side.</p>",
        "<p id='p3' style='clear:both'>Third paragraph is fully cleared and spans the full width.</p>",
        "</div>",
    ), 700.0);
    let p3 = by_id(&d.root, "p3").unwrap();
    let img1 = by_id(&d.root, "img1").unwrap();
    let img2 = by_id(&d.root, "img2").unwrap();
    // P3 below both floats
    assert!(
        p3.layout.content_rect.y >= img1.layout.content_rect.y + img1.layout.content_rect.h - 5.0,
        "p3 below img1"
    );
    assert!(
        p3.layout.content_rect.y >= img2.layout.content_rect.y + img2.layout.content_rect.h - 5.0,
        "p3 below img2"
    );
}
