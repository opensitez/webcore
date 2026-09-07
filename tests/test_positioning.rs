// Comprehensive CSS positioning tests — static, relative, absolute, fixed,
// sticky, z-index, containing blocks, and real-world layout patterns.

use webcore::load_html;
use webcore::types::*;

fn by_id<'a>(root: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if root.attributes.get("id").map(|v| v == id).unwrap_or(false) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = by_id(child, id) {
            return Some(found);
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

// === STATIC ===
#[test]
fn static_stack() {
    let d = load_html("<div style='width:400px'><div id='a' style='height:50px'>A</div><div id='b' style='height:60px'>B</div></div>", 500.0);
    assert!(
        by_id(&d.root, "b").unwrap().layout.content_rect.y
            >= by_id(&d.root, "a").unwrap().layout.content_rect.y + 48.0
    );
}
#[test]
fn static_ignores_offsets() {
    let d = load_html(
        "<div id='t' style='top:100px;left:200px;width:100px;height:50px'>X</div>",
        500.0,
    );
    assert!(by_id(&d.root, "t").unwrap().layout.content_rect.y < 50.0);
}

// === RELATIVE ===
#[test]
fn relative_offset() {
    let d = load_html("<div style='width:400px'><div id='a' style='height:50px'>A</div><div id='r' style='position:relative;top:20px;left:30px;height:50px'>R</div><div id='b' style='height:50px'>B</div></div>", 500.0);
    let a = by_id(&d.root, "a").unwrap();
    let r = by_id(&d.root, "r").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let ny = a.layout.content_rect.y + a.layout.content_rect.h;
    assert!(
        (r.layout.content_rect.y - (ny + 20.0)).abs() < 3.0,
        "offset by top:20"
    );
    assert!(
        (b.layout.content_rect.y - (ny + 50.0)).abs() < 3.0,
        "sibling unaffected"
    );
}
#[test]
fn relative_negative() {
    let d = load_html("<div style='padding-top:100px'><div id='t' style='position:relative;top:-30px;height:40px'>U</div></div>", 500.0);
    assert!((by_id(&d.root, "t").unwrap().layout.content_rect.y - 70.0).abs() < 5.0);
}
#[test]
fn relative_bottom_right() {
    let d = load_html("<div style='width:400px'><div id='t' style='position:relative;bottom:15px;right:25px;height:40px'>X</div></div>", 500.0);
    assert!(by_id(&d.root, "t").unwrap().layout.content_rect.y < 0.0);
}

// === ABSOLUTE ===
#[test]
fn abs_out_of_flow() {
    let d = load_html("<div style='width:400px;position:relative'><div id='a' style='height:50px'>A</div><div style='position:absolute;top:0;left:0;width:100px;height:100px'>X</div><div id='b' style='height:50px'>B</div></div>", 500.0);
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h)).abs() < 3.0
    );
}
#[test]
fn abs_relative_to_parent() {
    let d = load_html("<div style='position:relative;margin:50px;width:400px;height:300px'><div id='a' style='position:absolute;top:10px;left:20px;width:100px;height:80px'>A</div></div>", 600.0);
    let p = find(&d.root, &|b| {
        b.style.position == Position::Relative && b.style.width == CssLength::Px(400.0)
    })
    .unwrap();
    let a = by_id(&d.root, "a").unwrap();
    assert!((a.layout.content_rect.x - (p.layout.padding_rect.x + 20.0)).abs() < 3.0);
    assert!((a.layout.content_rect.y - (p.layout.padding_rect.y + 10.0)).abs() < 3.0);
}
#[test]
fn abs_bottom_right() {
    let d = load_html("<div style='position:relative;width:400px;height:300px'><div id='a' style='position:absolute;bottom:10px;right:20px;width:100px;height:50px'>A</div></div>", 500.0);
    let p = find(&d.root, &|b| b.style.position == Position::Relative).unwrap();
    let a = by_id(&d.root, "a").unwrap();
    let ey = p.layout.padding_rect.y + p.layout.padding_rect.h - 50.0 - 10.0;
    let ex = p.layout.padding_rect.x + p.layout.padding_rect.w - 100.0 - 20.0;
    assert!(
        (a.layout.content_rect.y - ey).abs() < 5.0,
        "bottom y={:.0} expected {:.0}",
        a.layout.content_rect.y,
        ey
    );
    assert!(
        (a.layout.content_rect.x - ex).abs() < 5.0,
        "right x={:.0} expected {:.0}",
        a.layout.content_rect.x,
        ex
    );
}
#[test]
fn abs_stretch_left_right() {
    let d = load_html("<div style='position:relative;width:400px;height:200px'><div id='a' style='position:absolute;left:50px;right:50px;top:10px;height:80px'>S</div></div>", 500.0);
    assert!((by_id(&d.root, "a").unwrap().layout.content_rect.w - 300.0).abs() < 5.0);
}
#[test]
fn abs_stretch_top_bottom() {
    let d = load_html("<div style='position:relative;width:400px;height:300px'><div id='a' style='position:absolute;top:20px;bottom:30px;left:10px;width:100px'>S</div></div>", 500.0);
    assert!((by_id(&d.root, "a").unwrap().layout.content_rect.h - 250.0).abs() < 5.0);
}
#[test]
fn abs_static_position() {
    let d = load_html("<div style='position:relative;width:400px'><div style='height:80px'>S</div><div id='a' style='position:absolute;width:100px;height:50px'>A</div></div>", 500.0);
    assert!(by_id(&d.root, "a").unwrap().layout.content_rect.y >= 75.0);
}
#[test]
fn abs_nested_containing_block() {
    let d = load_html("<div style='position:relative;margin:100px;width:600px;height:400px'><div style='margin:50px'><div style='position:relative;width:300px;height:200px'><div id='a' style='position:absolute;top:5px;left:5px;width:50px;height:50px'>D</div></div></div></div>", 800.0);
    let inner = find(&d.root, &|b| {
        b.style.position == Position::Relative && b.style.width == CssLength::Px(300.0)
    })
    .unwrap();
    let a = by_id(&d.root, "a").unwrap();
    assert!((a.layout.content_rect.x - (inner.layout.padding_rect.x + 5.0)).abs() < 3.0);
}
#[test]
fn abs_percentage() {
    let d = load_html("<div style='position:relative;width:800px;height:600px'><div id='a' style='position:absolute;width:50%;height:25%;top:10%;left:10%'>P</div></div>", 900.0);
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - 400.0).abs() < 5.0,
        "50%={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (a.layout.content_rect.h - 150.0).abs() < 5.0,
        "25%={:.0}",
        a.layout.content_rect.h
    );
}
#[test]
fn abs_in_flex() {
    let d = load_html("<div style='display:flex;width:600px'><div style='position:relative;flex:1;height:100px'><div id='a' style='position:absolute;top:0;right:0;width:30px;height:30px'>X</div><div id='f'>C</div></div><div style='flex:1'>O</div></div>", 700.0);
    assert!(by_id(&d.root, "f").unwrap().layout.content_rect.y < 10.0);
}
#[test]
fn abs_in_grid() {
    let d = load_html("<div style='display:grid;grid-template-columns:1fr 1fr;width:600px'><div style='position:relative;height:100px'><div id='a' style='position:absolute;bottom:5px;left:5px;width:50px;height:20px'>A</div><div id='f'>F</div></div><div>R</div></div>", 700.0);
    assert!(by_id(&d.root, "f").unwrap().layout.content_rect.y < 10.0);
}
#[test]
fn abs_no_positioned_parent() {
    let d = load_html("<div style='margin:100px;width:400px'><div id='a' style='position:absolute;top:5px;left:5px;width:50px;height:50px'>A</div></div>", 600.0);
    assert!(by_id(&d.root, "a").unwrap().layout.content_rect.y < 20.0);
}
#[test]
fn multiple_abs_independent() {
    let d = load_html("<div style='position:relative;width:400px;height:300px'><div id='a' style='position:absolute;top:10px;left:10px;width:100px;height:100px'>A</div><div id='b' style='position:absolute;top:50px;left:200px;width:100px;height:100px'>B</div></div>", 500.0);
    assert!(
        by_id(&d.root, "b").unwrap().layout.content_rect.x
            > by_id(&d.root, "a").unwrap().layout.content_rect.x + 100.0
    );
}

// === FIXED ===
#[test]
fn fixed_out_of_flow() {
    let d = load_html("<div style='width:400px'><div id='a' style='height:50px'>A</div><div style='position:fixed;top:0;left:0;width:100%;height:60px'>N</div><div id='b' style='height:50px'>B</div></div>", 500.0);
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (b.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h)).abs() < 3.0
    );
}
#[test]
fn fixed_viewport_relative() {
    let d = load_html("<div style='position:relative;margin:200px;width:400px;height:300px'><div id='f' style='position:fixed;top:10px;right:10px;width:80px;height:40px'>F</div></div>", 800.0);
    assert!((by_id(&d.root, "f").unwrap().layout.content_rect.y - 10.0).abs() < 5.0);
}

// === STICKY ===
#[test]
fn sticky_takes_flow_space() {
    let d = load_html("<div style='width:400px'><div id='a' style='height:50px'>A</div><div id='s' style='position:sticky;top:0;height:40px'>S</div><div id='b' style='height:50px'>B</div></div>", 500.0);
    let a = by_id(&d.root, "a").unwrap();
    let s = by_id(&d.root, "s").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (s.layout.content_rect.y - (a.layout.content_rect.y + a.layout.content_rect.h)).abs() < 3.0
    );
    assert!(
        (b.layout.content_rect.y - (s.layout.content_rect.y + s.layout.content_rect.h)).abs() < 3.0
    );
}

// === Z-INDEX ===
#[test]
fn z_index_positive() {
    let d = load_html(
        "<div id='t' style='position:relative;z-index:10'>Z</div>",
        500.0,
    );
    assert_eq!(by_id(&d.root, "t").unwrap().style.z_index, 10);
}
#[test]
fn z_index_neg() {
    let d = load_html(
        "<div id='t' style='position:relative;z-index:-5'>Z</div>",
        500.0,
    );
    assert_eq!(by_id(&d.root, "t").unwrap().style.z_index, -5);
}
#[test]
fn z_index_auto() {
    let d = load_html("<div id='t' style='position:relative'>Z</div>", 500.0);
    assert_eq!(by_id(&d.root, "t").unwrap().style.z_index, 0);
}

// === REAL-WORLD: AP News dropdown ===
#[test]
fn apnews_dropdown_no_inflate() {
    let d = load_html(concat!(
        "<style>.h{position:relative;width:1000px;height:60px}.n{display:flex;height:60px}",
        ".i{position:relative;padding:0 15px}.dd{position:absolute;top:100%;left:0;width:200px;visibility:hidden}",
        ".di{height:30px}</style>",
        "<div class='h'><div class='n'>",
        "<div class='i' id='i1'>W<div class='dd'><div class='di'>S1</div></div></div>",
        "<div class='i' id='i2'>P</div></div></div>",
    ), 1100.0);
    let h = find(&d.root, &|b| {
        b.attributes.get("class").map(|c| c == "h").unwrap_or(false)
    })
    .unwrap();
    assert!(
        (h.layout.content_rect.h - 60.0).abs() < 5.0,
        "h={:.0}",
        h.layout.content_rect.h
    );
}

// === REAL-WORLD: Card overlay ===
#[test]
fn card_overlay_no_inflate() {
    let d = load_html(concat!(
        "<style>.c{position:relative;width:300px}.c a::before{content:'';position:absolute;top:0;left:0;right:0;bottom:0}.im{height:200px}</style>",
        "<div class='c'><div class='im' id='im'>I</div><a href='/'><h3 id='t'>T</h3></a></div>",
    ), 400.0);
    let c = find(&d.root, &|b| {
        b.attributes.get("class").map(|c| c == "c").unwrap_or(false)
    })
    .unwrap();
    assert!(
        by_id(&d.root, "t").unwrap().layout.content_rect.y
            > by_id(&d.root, "im").unwrap().layout.content_rect.y + 190.0
    );
    assert!(
        c.layout.content_rect.h < 350.0,
        "card h={:.0}",
        c.layout.content_rect.h
    );
}

// === REAL-WORLD: Sticky header + content ===
#[test]
fn sticky_header_content() {
    let d = load_html("<div style='width:800px'><div style='position:sticky;top:0;height:60px'>H</div><div id='c' style='height:2000px'>C</div><div id='f' style='height:100px'>F</div></div>", 900.0);
    assert!((by_id(&d.root, "c").unwrap().layout.content_rect.y - 60.0).abs() < 5.0);
    assert!(by_id(&d.root, "f").unwrap().layout.content_rect.y > 2050.0);
}

// === REAL-WORLD: Two-column sidebar ===
#[test]
fn sidebar_flex_layout() {
    let d = load_html(concat!(
        "<style>.l{display:flex;width:1000px}.m{flex:1}.s{width:300px}.a{height:500px;margin-bottom:20px}</style>",
        "<div class='l'><div class='m'><div class='a' id='a1'>A1</div><div class='a' id='a2'>A2</div></div><div class='s' id='sb'>SB</div></div>",
    ), 1100.0);
    let sb = by_id(&d.root, "sb").unwrap();
    let a1 = by_id(&d.root, "a1").unwrap();
    assert!((sb.layout.content_rect.y - a1.layout.content_rect.y).abs() < 5.0);
}

// === REAL-WORLD: Hero overlay ===
#[test]
fn hero_overlay() {
    let d = load_html(concat!(
        "<style>.hero{position:relative;width:800px;height:400px}.ht{position:absolute;bottom:20px;left:20px;right:20px}</style>",
        "<div class='hero'><div style='width:100%;height:100%'>I</div><div class='ht'><h1 id='t'>B</h1></div></div>",
    ), 900.0);
    let hero = find(&d.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "hero")
            .unwrap_or(false)
    })
    .unwrap();
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.y > hero.layout.content_rect.y + 300.0);
}

// === REAL-WORLD: Hamburger ===
#[test]
fn hamburger_no_push() {
    let d = load_html("<style>.mn{position:fixed;top:0;left:-300px;width:300px;height:100%;visibility:hidden}</style><div style='width:1000px'><div class='mn'>M</div><div id='c'>C</div></div>", 1100.0);
    assert!(by_id(&d.root, "c").unwrap().layout.content_rect.x < 50.0);
}

// === Three panel absolute ===
#[test]
fn three_panel_abs() {
    let d = load_html(
        concat!(
        "<div style='position:relative;width:1000px;height:600px'>",
        "<div id='l' style='position:absolute;top:0;left:0;width:200px;height:100%'>L</div>",
        "<div id='m' style='position:absolute;top:0;left:200px;right:200px;height:100%'>M</div>",
        "<div id='r' style='position:absolute;top:0;right:0;width:200px;height:100%'>R</div></div>",
    ),
        1100.0,
    );
    let l = by_id(&d.root, "l").unwrap();
    let m = by_id(&d.root, "m").unwrap();
    let r = by_id(&d.root, "r").unwrap();
    assert!((l.layout.content_rect.w - 200.0).abs() < 5.0);
    assert!(
        (m.layout.content_rect.w - 600.0).abs() < 5.0,
        "center={:.0}",
        m.layout.content_rect.w
    );
    assert!((r.layout.content_rect.w - 200.0).abs() < 5.0);
}

// === Overlapping stacking ===
#[test]
fn overlapping_z_order() {
    let d = load_html("<div style='position:relative;width:400px;height:400px'><div id='a' style='position:absolute;top:50px;left:50px;width:200px;height:200px;z-index:1'>A</div><div id='b' style='position:absolute;top:100px;left:100px;width:200px;height:200px;z-index:2'>B</div></div>", 500.0);
    assert_eq!(by_id(&d.root, "a").unwrap().style.z_index, 1);
    assert_eq!(by_id(&d.root, "b").unwrap().style.z_index, 2);
}

// === Transform containing block ===
#[test]
fn transform_containing_block() {
    let d = load_html("<div style='transform:translateX(0);width:400px;height:300px;margin:50px'><div id='a' style='position:absolute;top:10px;left:10px;width:80px;height:60px'>A</div></div>", 600.0);
    let p = find(&d.root, &|b| !b.style.transform.is_empty()).unwrap();
    let a = by_id(&d.root, "a").unwrap();
    assert!((a.layout.content_rect.x - (p.layout.padding_rect.x + 10.0)).abs() < 5.0);
}

// === Zero size no crash ===
#[test]
fn abs_zero_parent_no_crash() {
    let d = load_html("<div style='position:relative;width:0;height:0'><div id='a' style='position:absolute;top:0;left:0;width:50px;height:50px'>A</div></div>", 100.0);
    assert_eq!(
        by_id(&d.root, "a").unwrap().layout.content_rect.w as u32,
        50
    );
}

// === Deep nesting ===
#[test]
fn deep_abs() {
    let d = load_html("<div style='position:relative;width:500px;height:500px'><div><div><div><div><div id='a' style='position:absolute;top:10px;left:10px;width:50px;height:50px'>D</div></div></div></div></div></div>", 600.0);
    assert!((by_id(&d.root, "a").unwrap().layout.content_rect.w - 50.0).abs() < 3.0);
}

// === Tooltip ===
#[test]
fn tooltip_above() {
    let d = load_html("<style>.tr{position:relative;display:inline-block}.tp{position:absolute;bottom:100%;left:50%;width:150px;visibility:hidden}</style><div style='padding-top:100px'><span class='tr'>H<div class='tp' id='tip'>T</div></span></div>", 500.0);
    let tr = find(&d.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "tr")
            .unwrap_or(false)
    })
    .unwrap();
    assert!(by_id(&d.root, "tip").unwrap().layout.content_rect.y < tr.layout.content_rect.y);
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ABSOLUTE: centering patterns                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_center_with_margin_auto() {
    let d = load_html(concat!(
        "<div style='position:relative;width:400px;height:300px'>",
        "<div id='c' style='position:absolute;top:0;left:0;right:0;bottom:0;width:200px;height:100px;margin:auto'>C</div>",
        "</div>",
    ), 500.0);
    let c = by_id(&d.root, "c").unwrap();
    // margin:auto on all sides with all offsets=0 → centered
    assert!(
        (c.layout.content_rect.x - 100.0).abs() < 10.0,
        "horiz centered x={:.0}",
        c.layout.content_rect.x
    );
    assert!(
        (c.layout.content_rect.y - 100.0).abs() < 10.0,
        "vert centered y={:.0}",
        c.layout.content_rect.y
    );
}

#[test]
fn abs_center_horizontal_only() {
    let d = load_html(concat!(
        "<div style='position:relative;width:600px;height:200px'>",
        "<div id='c' style='position:absolute;left:0;right:0;width:200px;margin-left:auto;margin-right:auto;height:50px'>C</div>",
        "</div>",
    ), 700.0);
    let c = by_id(&d.root, "c").unwrap();
    assert!(
        (c.layout.content_rect.x - 200.0).abs() < 10.0,
        "centered x={:.0}",
        c.layout.content_rect.x
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ABSOLUTE: interaction with overflow                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_child_outside_overflow_hidden_parent() {
    let d = load_html(concat!(
        "<div style='position:relative;width:300px;height:200px;overflow:hidden'>",
        "<div id='inside' style='position:absolute;top:10px;left:10px;width:100px;height:100px'>In</div>",
        "<div id='outside' style='position:absolute;top:10px;left:350px;width:100px;height:100px'>Out</div>",
        "</div>",
    ), 500.0);
    let inside = by_id(&d.root, "inside").unwrap();
    let outside = by_id(&d.root, "outside").unwrap();
    // Both positioned, but outside is beyond overflow:hidden clip
    assert!(inside.layout.content_rect.w > 0.0, "inside has dimensions");
    assert!(
        outside.layout.content_rect.w > 0.0,
        "outside has dimensions (clipped visually, not layout)"
    );
}

#[test]
fn abs_child_overflow_auto_scrollable() {
    let d = load_html(
        concat!(
            "<div style='position:relative;width:300px;height:200px;overflow:auto'>",
            "<div style='height:500px'>Tall content</div>",
            "<div id='abs' style='position:absolute;top:0;right:0;width:50px;height:50px'>A</div>",
            "</div>",
        ),
        400.0,
    );
    let abs = by_id(&d.root, "abs").unwrap();
    assert!(
        abs.layout.content_rect.w > 0.0,
        "abs renders in scrollable container"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ABSOLUTE: interaction with flexbox                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_does_not_become_flex_item() {
    let d = load_html(
        concat!(
        "<div style='display:flex;position:relative;width:600px;height:100px'>",
        "<div id='f1' style='flex:1'>F1</div>",
        "<div id='abs' style='position:absolute;top:0;right:0;width:80px;height:80px'>Abs</div>",
        "<div id='f2' style='flex:1'>F2</div>",
        "</div>",
    ),
        700.0,
    );
    let f1 = by_id(&d.root, "f1").unwrap();
    let f2 = by_id(&d.root, "f2").unwrap();
    // Abs doesn't take flex space — f1 and f2 each get 300px
    assert!(
        (f1.layout.content_rect.w - 300.0).abs() < 5.0,
        "f1={:.0}",
        f1.layout.content_rect.w
    );
    assert!(
        (f2.layout.content_rect.w - 300.0).abs() < 5.0,
        "f2={:.0}",
        f2.layout.content_rect.w
    );
}

#[test]
fn abs_does_not_become_grid_item() {
    let d = load_html(
        concat!(
        "<div style='display:grid;grid-template-columns:1fr 1fr;position:relative;width:600px'>",
        "<div id='g1'>G1</div>",
        "<div id='abs' style='position:absolute;top:0;right:0;width:80px'>Abs</div>",
        "<div id='g2'>G2</div>",
        "</div>",
    ),
        700.0,
    );
    let g1 = by_id(&d.root, "g1").unwrap();
    let g2 = by_id(&d.root, "g2").unwrap();
    // Abs skipped, g1 and g2 are the grid items
    assert!(
        (g1.layout.content_rect.y - g2.layout.content_rect.y).abs() < 5.0,
        "same row"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  RELATIVE: doesn't change containing block                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn relative_creates_containing_block_for_abs_child() {
    let d = load_html(concat!(
        "<div style='width:600px'>",
        "<div style='position:relative;width:300px;height:200px;margin-left:100px'>",
        "  <div id='abs' style='position:absolute;top:10px;left:10px;width:50px;height:50px'>A</div>",
        "</div>",
        "</div>",
    ), 700.0);
    let rel = find(&d.root, &|b| {
        b.style.position == Position::Relative && b.style.width == CssLength::Px(300.0)
    })
    .unwrap();
    let abs = by_id(&d.root, "abs").unwrap();
    assert!(
        (abs.layout.content_rect.x - (rel.layout.padding_rect.x + 10.0)).abs() < 3.0,
        "abs relative to rel parent"
    );
}

#[test]
fn relative_preserves_flow_space() {
    // Relative elements take their normal flow space even when offset
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<div style='height:50px'>Before</div>",
            "<div id='r' style='position:relative;top:200px;height:80px'>Far down</div>",
            "<div id='after' style='height:50px'>After</div>",
            "</div>",
        ),
        500.0,
    );
    let after = by_id(&d.root, "after").unwrap();
    // After should be at 50+80=130, NOT at 50+200+80=330
    assert!(
        after.layout.content_rect.y < 140.0,
        "after y={:.0} should be at ~130 (relative doesn't shift siblings)",
        after.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  POSITION + INSET shorthand                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn inset_shorthand_all_sides() {
    let d = load_html(
        concat!(
            "<div style='position:relative;width:400px;height:300px'>",
            "<div id='a' style='position:absolute;inset:10px;'>Fill</div>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // inset:10px = top:10 right:10 bottom:10 left:10 → w=380 h=280
    assert!(
        (a.layout.content_rect.w - 380.0).abs() < 5.0,
        "inset w={:.0}",
        a.layout.content_rect.w
    );
    assert!(
        (a.layout.content_rect.h - 280.0).abs() < 5.0,
        "inset h={:.0}",
        a.layout.content_rect.h
    );
}

#[test]
fn inset_zero_fills_parent() {
    let d = load_html(
        concat!(
            "<div style='position:relative;width:500px;height:400px'>",
            "<div id='a' style='position:absolute;inset:0'>Full</div>",
            "</div>",
        ),
        600.0,
    );
    let p = find(&d.root, &|b| b.style.position == Position::Relative).unwrap();
    let a = by_id(&d.root, "a").unwrap();
    assert!(
        (a.layout.content_rect.w - p.layout.content_rect.w).abs() < 5.0,
        "fills parent w"
    );
    assert!(
        (a.layout.content_rect.h - p.layout.content_rect.h).abs() < 5.0,
        "fills parent h"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  FIXED + ABSOLUTE interaction                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_inside_fixed_relative_to_fixed() {
    let d = load_html(concat!(
        "<div style='position:fixed;top:20px;left:20px;width:300px;height:200px'>",
        "<div id='abs' style='position:absolute;bottom:5px;right:5px;width:50px;height:30px'>A</div>",
        "</div>",
    ), 500.0);
    let fixed = find(&d.root, &|b| b.style.position == Position::Fixed).unwrap();
    let abs = by_id(&d.root, "abs").unwrap();
    // abs relative to fixed parent
    let ey = fixed.layout.padding_rect.y + fixed.layout.padding_rect.h - 30.0 - 5.0;
    assert!(
        (abs.layout.content_rect.y - ey).abs() < 5.0,
        "abs in fixed y={:.0} expected {:.0}",
        abs.layout.content_rect.y,
        ey
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Multiple positioned layers                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn three_layer_positioned_stack() {
    let d = load_html(concat!(
        "<div style='position:relative;width:500px;height:500px'>",
        "  <div id='bg' style='position:absolute;inset:0;z-index:0'>Background</div>",
        "  <div id='content' style='position:relative;z-index:1;padding:20px'>",
        "    <div id='text' style='height:200px'>Main text</div>",
        "  </div>",
        "  <div id='overlay' style='position:absolute;inset:0;z-index:2;pointer-events:none'>Overlay</div>",
        "</div>",
    ), 600.0);
    let bg = by_id(&d.root, "bg").unwrap();
    let content = by_id(&d.root, "content").unwrap();
    let overlay = by_id(&d.root, "overlay").unwrap();
    assert_eq!(bg.style.z_index, 0);
    assert_eq!(content.style.z_index, 1);
    assert_eq!(overlay.style.z_index, 2);
    // All should have dimensions
    assert!(bg.layout.content_rect.w > 400.0, "bg has width");
    assert!(content.layout.content_rect.w > 400.0, "content has width");
    assert!(overlay.layout.content_rect.w > 400.0, "overlay has width");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Floating + positioned interaction                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn float_and_relative_together() {
    let d = load_html(concat!(
        "<div style='width:400px'>",
        "<div id='f' style='float:left;position:relative;top:10px;width:100px;height:100px'>Float+Rel</div>",
        "<div id='text'>Text wraps around the float</div>",
        "</div>",
    ), 500.0);
    let f = by_id(&d.root, "f").unwrap();
    // Float with relative: float placement + relative offset
    assert!(
        f.layout.content_rect.y >= 8.0,
        "float+relative top:10 y={:.0}",
        f.layout.content_rect.y
    );
}

#[test]
fn abs_over_float() {
    let d = load_html(concat!(
        "<div style='position:relative;width:400px'>",
        "<div style='float:left;width:200px;height:150px'>Float</div>",
        "<div id='abs' style='position:absolute;top:0;left:0;width:400px;height:50px;z-index:10'>Over</div>",
        "<div id='text'>Text after float</div>",
        "</div>",
    ), 500.0);
    let abs = by_id(&d.root, "abs").unwrap();
    // Abs overlays the float (z-index:10)
    assert!(abs.layout.content_rect.w > 350.0, "abs covers full width");
    assert_eq!(abs.style.z_index, 10);
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Percentage of percentage                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_percentage_of_percentage_parent() {
    let d = load_html(
        concat!(
            "<div style='width:1000px;height:800px'>",
            "<div style='position:relative;width:50%;height:50%'>",
            "  <div id='abs' style='position:absolute;width:50%;height:50%'>P</div>",
            "</div>",
            "</div>",
        ),
        1100.0,
    );
    let abs = by_id(&d.root, "abs").unwrap();
    // 1000*50%=500 → 500*50%=250
    assert!(
        (abs.layout.content_rect.w - 250.0).abs() < 10.0,
        "50%% of 50%% w={:.0}",
        abs.layout.content_rect.w
    );
    // 800*50%=400 → 400*50%=200
    assert!(
        (abs.layout.content_rect.h - 200.0).abs() < 10.0,
        "50%% of 50%% h={:.0}",
        abs.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Positioned inside table cells                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn abs_inside_table_cell() {
    let d = load_html(concat!(
        "<table style='width:600px'><tr>",
        "<td style='position:relative;height:100px;width:50%'>",
        "  <div id='abs' style='position:absolute;bottom:5px;right:5px;width:30px;height:30px'>A</div>",
        "  Cell content",
        "</td>",
        "<td>Other</td>",
        "</tr></table>",
    ), 700.0);
    let abs = by_id(&d.root, "abs").unwrap();
    assert!(abs.layout.content_rect.w > 0.0, "abs in table cell renders");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Media object pattern                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_object_pattern() {
    let d = load_html(
        concat!(
        "<style>",
        ".media{display:flex;width:500px}",
        ".media-img{width:120px;height:80px;flex-shrink:0}",
        ".media-body{flex:1;position:relative}",
        ".badge{position:absolute;top:-10px;right:-10px;width:24px;height:24px;border-radius:50%}",
        "</style>",
        "<div class='media'>",
        "<div class='media-img' id='img'>Img</div>",
        "<div class='media-body'>",
        "  <div id='badge' class='badge'>!</div>",
        "  <h3 id='title'>Title</h3>",
        "  <p id='desc'>Description text</p>",
        "</div>",
        "</div>",
    ),
        600.0,
    );
    let img = by_id(&d.root, "img").unwrap();
    let title = by_id(&d.root, "title").unwrap();
    let desc = by_id(&d.root, "desc").unwrap();
    let badge = by_id(&d.root, "badge").unwrap();
    // Image on the left, text on the right
    assert!(
        title.layout.content_rect.x > img.layout.content_rect.x + 100.0,
        "title right of img"
    );
    // Desc below title
    assert!(
        desc.layout.content_rect.y > title.layout.content_rect.y + 10.0,
        "desc below title"
    );
    // Badge is absolute, doesn't push title
    assert!(
        title.layout.content_rect.y < 20.0
            || badge.layout.content_rect.y < title.layout.content_rect.y,
        "badge doesn't push title down"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Sticky in overflow:auto container                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn sticky_in_overflow_container() {
    let d = load_html(
        concat!(
        "<div style='width:400px;height:200px;overflow:auto'>",
        "<div id='sticky' style='position:sticky;top:0;height:40px;background:white'>Sticky</div>",
        "<div style='height:1000px'>Tall content</div>",
        "</div>",
    ),
        500.0,
    );
    let sticky = by_id(&d.root, "sticky").unwrap();
    // Sticky takes flow space in scroll container
    assert!(sticky.layout.content_rect.h >= 38.0, "sticky has height");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: Position inheritance (none — position not inherited)║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn position_not_inherited() {
    let d = load_html(
        concat!(
            "<div style='position:absolute;top:50px;left:50px;width:200px;height:200px'>",
            "<div id='child' style='width:100px;height:100px'>Child</div>",
            "</div>",
        ),
        500.0,
    );
    let child = by_id(&d.root, "child").unwrap();
    assert_eq!(
        child.style.position,
        Position::Static,
        "position not inherited"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX: AOL-style layered layout                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn aol_layered_layout() {
    let d = load_html(concat!(
        "<style>",
        ".page{position:relative;width:1200px;min-height:800px}",
        ".top-ad{height:90px;width:100%}",
        ".nav{position:sticky;top:0;height:50px;z-index:100}",
        ".content{display:flex}",
        ".main-feed{flex:1}",
        ".sidebar{width:300px}",
        ".article{position:relative;height:200px;margin-bottom:20px}",
        ".article-badge{position:absolute;top:10px;left:10px;width:60px;height:24px}",
        ".floating-ad{position:fixed;bottom:20px;right:20px;width:300px;height:250px;z-index:50}",
        "</style>",
        "<div class='page'>",
        "  <div class='top-ad' id='ad'>Ad</div>",
        "  <div class='nav' id='nav'>Nav</div>",
        "  <div class='content'>",
        "    <div class='main-feed'>",
        "      <div class='article' id='art1'><div class='article-badge' id='badge1'>NEW</div>Article 1</div>",
        "      <div class='article' id='art2'>Article 2</div>",
        "    </div>",
        "    <div class='sidebar' id='sb'>Sidebar</div>",
        "  </div>",
        "  <div class='floating-ad' id='fad'>Float Ad</div>",
        "</div>",
    ), 1300.0);
    let ad = by_id(&d.root, "ad").unwrap();
    let nav = by_id(&d.root, "nav").unwrap();
    let art1 = by_id(&d.root, "art1").unwrap();
    let art2 = by_id(&d.root, "art2").unwrap();
    let sb = by_id(&d.root, "sb").unwrap();
    let badge1 = by_id(&d.root, "badge1").unwrap();
    let fad = by_id(&d.root, "fad").unwrap();

    // Vertical stacking: ad → nav → content
    assert!(
        nav.layout.content_rect.y >= ad.layout.content_rect.y + 85.0,
        "nav below ad"
    );
    assert!(
        art1.layout.content_rect.y >= nav.layout.content_rect.y + 45.0,
        "content below nav"
    );
    assert!(
        art2.layout.content_rect.y > art1.layout.content_rect.y + 190.0,
        "art2 below art1"
    );
    // Sidebar alongside articles
    assert!(
        (sb.layout.content_rect.y - art1.layout.content_rect.y).abs() < 5.0,
        "sidebar aligns"
    );
    assert!(
        sb.layout.content_rect.x > art1.layout.content_rect.x + 200.0,
        "sidebar right of main"
    );
    // Badge absolute inside article, doesn't push content
    assert!(badge1.layout.content_rect.w > 0.0, "badge renders");
    // Floating ad fixed
    assert_eq!(fad.style.position, Position::Fixed);
    assert_eq!(fad.style.z_index, 50);
}
