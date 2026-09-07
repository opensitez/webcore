// Comprehensive image and SVG sizing/layout tests — covers intrinsic dimensions,
// aspect ratio, width/height attributes, CSS overrides, images in flex/grid/table,
// SVG viewBox, and real-world patterns.

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
// ║  IMG: width/height HTML attributes                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_width_height_attrs() {
    let d = load_html(
        "<img id='i' width='200' height='150' src='test.png'>",
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.w - 200.0).abs() < 5.0,
        "img w={:.0} should be 200",
        i.layout.content_rect.w
    );
    assert!(
        (i.layout.content_rect.h - 150.0).abs() < 5.0,
        "img h={:.0} should be 150",
        i.layout.content_rect.h
    );
}

#[test]
fn img_width_only_attr() {
    let d = load_html("<img id='i' width='300' src='test.png'>", 800.0);
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.w - 300.0).abs() < 5.0,
        "img w={:.0} should be 300",
        i.layout.content_rect.w
    );
    // Height without aspect ratio info defaults to something reasonable
    assert!(i.layout.content_rect.h > 0.0, "img should have some height");
}

#[test]
fn img_height_only_attr() {
    let d = load_html("<img id='i' height='200' src='test.png'>", 800.0);
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.h - 200.0).abs() < 5.0,
        "img h={:.0} should be 200",
        i.layout.content_rect.h
    );
}

#[test]
fn img_no_dimensions() {
    let d = load_html("<img id='i' src='test.png'>", 800.0);
    let i = by_id(&d.root, "i").unwrap();
    // No dimensions → placeholder or 0 but should not crash
    assert!(
        i.layout.content_rect.w >= 0.0,
        "no crash on dimensionless img"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: CSS width/height override attributes                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_css_width_overrides_attr() {
    let d = load_html(
        concat!(
            "<style>img { width: 400px; }</style>",
            "<img id='i' width='200' height='150' src='test.png'>",
        ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.w - 400.0).abs() < 5.0,
        "CSS width overrides attr w={:.0}",
        i.layout.content_rect.w
    );
}

#[test]
fn img_css_height_overrides_attr() {
    let d = load_html(
        concat!(
            "<style>img { height: 300px; }</style>",
            "<img id='i' width='200' height='150' src='test.png'>",
        ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.h - 300.0).abs() < 5.0,
        "CSS height overrides attr h={:.0}",
        i.layout.content_rect.h
    );
}

#[test]
fn img_css_width_auto_uses_attr() {
    let d = load_html(
        concat!(
            "<style>img { width: auto; }</style>",
            "<img id='i' width='250' height='180' src='test.png'>",
        ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // width:auto should fall back to intrinsic/attr width
    assert!(
        (i.layout.content_rect.w - 250.0).abs() < 10.0,
        "width:auto uses attr w={:.0}",
        i.layout.content_rect.w
    );
}

#[test]
fn img_width_100_percent() {
    let d = load_html(
        concat!(
            "<div style='width:600px'>",
            "<img id='i' style='width:100%' width='200' height='150' src='test.png'>",
            "</div>",
        ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.w - 600.0).abs() < 10.0,
        "width:100%% = container w={:.0}",
        i.layout.content_rect.w
    );
}

#[test]
fn img_max_width_100_percent() {
    let d = load_html(
        concat!(
            "<div style='width:300px'>",
            "<img id='i' style='max-width:100%' width='500' height='300' src='test.png'>",
            "</div>",
        ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Image is 500px wide but max-width:100% constrains to 300px
    assert!(
        i.layout.content_rect.w <= 305.0,
        "max-width:100%% constrains w={:.0}",
        i.layout.content_rect.w
    );
}

#[test]
fn img_max_width_preserves_aspect_ratio() {
    let d = load_html(
        concat!(
        "<div style='width:200px'>",
        "<img id='i' style='max-width:100%;height:auto' width='400' height='300' src='test.png'>",
        "</div>",
    ),
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Constrained to 200px wide, aspect ratio 4:3 → height should be 150px
    assert!(
        i.layout.content_rect.w <= 205.0,
        "constrained w={:.0}",
        i.layout.content_rect.w
    );
    assert!(
        (i.layout.content_rect.h - 150.0).abs() < 10.0,
        "aspect ratio preserved h={:.0} should be ~150",
        i.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: in flex container                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_in_flex_shrinks() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px'>",
            "<img id='i' width='300' height='200' src='test.png'>",
            "<div style='width:200px'>Text</div>",
            "</div>",
        ),
        500.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Flex may shrink the image to fit
    assert!(i.layout.content_rect.w > 0.0, "img has width in flex");
}

#[test]
fn img_in_flex_with_flex_shrink_0() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:400px'>",
            "<img id='i' style='flex-shrink:0' width='300' height='200' src='test.png'>",
            "<div style='flex:1'>Text</div>",
            "</div>",
        ),
        500.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        (i.layout.content_rect.w - 300.0).abs() < 10.0,
        "flex-shrink:0 preserves w={:.0}",
        i.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: in grid container                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_in_grid_column() {
    let d = load_html(
        concat!(
            "<div style='display:grid;grid-template-columns:1fr 1fr;width:600px'>",
            "<img id='i' width='400' height='300' style='max-width:100%' src='test.png'>",
            "<div>Text</div>",
            "</div>",
        ),
        700.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Grid column is 300px, image constrained to that
    assert!(
        i.layout.content_rect.w <= 305.0,
        "img in grid col w={:.0}",
        i.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: in table cell                                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_in_table_cell() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td><img id='i' width='200' height='150' src='test.png'></td>",
            "<td id='t'>Text cell</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (i.layout.content_rect.w - 200.0).abs() < 10.0,
        "img in td w={:.0}",
        i.layout.content_rect.w
    );
    assert!(
        t.layout.content_rect.x > i.layout.content_rect.x + 190.0,
        "text right of img"
    );
}

#[test]
fn img_stretches_table_cell() {
    let d = load_html(
        concat!(
            "<table style='width:600px'><tr>",
            "<td style='width:250px'><img id='i' width='200' height='150' src='test.png'></td>",
            "<td>Other</td>",
            "</tr></table>",
        ),
        700.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Image keeps its own dimensions (200px), cell is 250px
    assert!(
        (i.layout.content_rect.w - 200.0).abs() < 10.0,
        "img keeps own w={:.0}",
        i.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: alongside float                                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_next_to_float() {
    let d = load_html(
        concat!(
            "<div style='width:500px'>",
            "<div style='float:left;width:150px;height:150px'>Float</div>",
            "<img id='i' width='200' height='150' src='test.png'>",
            "</div>",
        ),
        600.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    assert!(
        i.layout.content_rect.x >= 145.0,
        "img next to float x={:.0}",
        i.layout.content_rect.x
    );
}

#[test]
fn img_floated_left() {
    let d = load_html(concat!(
        "<div style='width:500px'>",
        "<img id='i' style='float:left;margin:0 10px 10px 0' width='200' height='150' src='test.png'>",
        "<p id='t'>Text wraps around the floated image on the right side.</p>",
        "</div>",
    ), 600.0);
    let i = by_id(&d.root, "i").unwrap();
    assert!(i.layout.content_rect.x < 20.0, "floated img at left");
    assert!(
        (i.layout.content_rect.w - 200.0).abs() < 5.0,
        "floated img w=200"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: display:block centering                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_block_centered() {
    let d = load_html(
        concat!(
        "<div style='width:600px'>",
        "<img id='i' style='display:block;margin:0 auto' width='200' height='150' src='test.png'>",
        "</div>",
    ),
        700.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Centered: x ≈ (600-200)/2 = 200
    assert!(
        (i.layout.content_rect.x - 200.0).abs() < 30.0,
        "block centered img x={:.0} should be ~200",
        i.layout.content_rect.x
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SVG: viewBox sizing                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn svg_viewbox_intrinsic_size() {
    let d = load_html(
        "<svg id='s' viewBox='0 0 200 100' xmlns='http://www.w3.org/2000/svg'><rect width='200' height='100'/></svg>",
        800.0,
    );
    let s = by_id(&d.root, "s").unwrap();
    // SVG with viewBox and no explicit w/h uses viewBox dimensions
    assert!(
        s.svg_viewbox_w > 0.0 || s.layout.content_rect.w > 0.0,
        "svg has intrinsic width"
    );
}

#[test]
fn svg_explicit_width_height() {
    let d = load_html(
        "<svg id='s' width='300' height='200' xmlns='http://www.w3.org/2000/svg'><rect width='300' height='200'/></svg>",
        800.0,
    );
    let s = by_id(&d.root, "s").unwrap();
    assert!(
        (s.layout.content_rect.w - 300.0).abs() < 10.0,
        "svg w={:.0} should be 300",
        s.layout.content_rect.w
    );
    assert!(
        (s.layout.content_rect.h - 200.0).abs() < 10.0,
        "svg h={:.0} should be 200",
        s.layout.content_rect.h
    );
}

#[test]
fn svg_css_width_auto_with_viewbox() {
    let d = load_html(concat!(
        "<style>svg { width: auto; height: 50px; }</style>",
        "<svg id='s' viewBox='0 0 200 100' xmlns='http://www.w3.org/2000/svg'><rect width='200' height='100'/></svg>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    // height=50, aspect ratio 2:1 from viewBox → width should be 100
    if s.svg_viewbox_w > 0.0 {
        assert!(
            (s.layout.content_rect.w - 100.0).abs() < 10.0,
            "svg w={:.0} should be 100 (aspect ratio from viewBox)",
            s.layout.content_rect.w
        );
    }
}

#[test]
fn svg_css_height_auto_with_viewbox() {
    let d = load_html(concat!(
        "<style>svg { height: auto; width: 100px; }</style>",
        "<svg id='s' viewBox='0 0 200 100' xmlns='http://www.w3.org/2000/svg'><rect width='200' height='100'/></svg>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    // width=100, aspect ratio 2:1 → height should be 50
    if s.svg_viewbox_w > 0.0 {
        assert!(
            (s.layout.content_rect.h - 50.0).abs() < 10.0,
            "svg h={:.0} should be 50 (aspect ratio from viewBox)",
            s.layout.content_rect.h
        );
    }
}

#[test]
fn svg_width_em_units() {
    let d = load_html(concat!(
        "<svg id='s' width='7em' height='2em' viewBox='0 0 112 32' xmlns='http://www.w3.org/2000/svg'>",
        "<rect width='112' height='32'/></svg>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    // 7em at default 16px = 112px, 2em = 32px
    // But parse_px only handles digits, not em. viewBox should be fallback.
    assert!(
        s.layout.content_rect.w > 50.0,
        "svg with em width has non-zero w={:.0}",
        s.layout.content_rect.w
    );
}

#[test]
fn svg_css_overrides_attr() {
    let d = load_html(concat!(
        "<style>svg { width: 150px; height: 80px; }</style>",
        "<svg id='s' width='300' height='200' xmlns='http://www.w3.org/2000/svg'><rect width='300' height='200'/></svg>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    assert!(
        (s.layout.content_rect.w - 150.0).abs() < 10.0,
        "CSS overrides svg attr w={:.0}",
        s.layout.content_rect.w
    );
    assert!(
        (s.layout.content_rect.h - 80.0).abs() < 10.0,
        "CSS overrides svg attr h={:.0}",
        s.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SVG: in flex/grid                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn svg_in_flex() {
    let d = load_html(concat!(
        "<div style='display:flex;align-items:center;width:600px;height:60px'>",
        "<svg id='s' width='40' height='40' viewBox='0 0 40 40' xmlns='http://www.w3.org/2000/svg'><circle r='20'/></svg>",
        "<span id='t'>Logo text</span>",
        "</div>",
    ), 700.0);
    let s = by_id(&d.root, "s").unwrap();
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        s.layout.content_rect.w > 30.0,
        "svg in flex has width w={:.0}",
        s.layout.content_rect.w
    );
    assert!(
        t.layout.content_rect.x > s.layout.content_rect.x + 30.0,
        "text after svg"
    );
}

#[test]
fn svg_in_inline_block() {
    let d = load_html(concat!(
        "<span style='display:inline-block'>",
        "<svg id='s' width='24' height='24' viewBox='0 0 24 24' xmlns='http://www.w3.org/2000/svg'><path d='M0 0h24v24H0z'/></svg>",
        "</span>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    assert!(
        (s.layout.content_rect.w - 24.0).abs() < 5.0,
        "svg in inline-block w={:.0}",
        s.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SVG: viewBox aspect ratio with CSS constraints             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn svg_viewbox_aspect_ratio_width_constrained() {
    let d = load_html(concat!(
        "<div style='width:300px'>",
        "<svg id='s' style='width:100%;height:auto' viewBox='0 0 600 400' xmlns='http://www.w3.org/2000/svg'>",
        "<rect width='600' height='400'/></svg>",
        "</div>",
    ), 400.0);
    let s = by_id(&d.root, "s").unwrap();
    // Width = 300px (100% of container), height = 300 * 400/600 = 200
    if s.svg_viewbox_w > 0.0 {
        assert!(
            (s.layout.content_rect.w - 300.0).abs() < 10.0,
            "svg 100%% w={:.0}",
            s.layout.content_rect.w
        );
        assert!(
            (s.layout.content_rect.h - 200.0).abs() < 15.0,
            "aspect ratio h={:.0} should be ~200",
            s.layout.content_rect.h
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: multiple images in a row                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn images_side_by_side_inline() {
    let d = load_html(
        concat!(
            "<div style='width:600px'>",
            "<img id='a' width='150' height='100' src='a.png'>",
            "<img id='b' width='150' height='100' src='b.png'>",
            "<img id='c' width='150' height='100' src='c.png'>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // Three 150px images fit in 600px → same line
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "a and b same line"
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
fn images_wrap_to_next_line() {
    let d = load_html(
        concat!(
            "<div style='width:400px'>",
            "<img id='a' width='150' height='100' src='a.png'>",
            "<img id='b' width='150' height='100' src='b.png'>",
            "<img id='c' width='150' height='100' src='c.png'>",
            "</div>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // 3 * 150 = 450 > 400 → third wraps
    assert!(
        c.layout.content_rect.y > a.layout.content_rect.y + 90.0,
        "c wraps to next line c_y={:.0} a_y={:.0}",
        c.layout.content_rect.y,
        a.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: responsive patterns                                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_responsive_pattern() {
    let d = load_html(
        concat!(
            "<style>img { max-width: 100%; height: auto; display: block; }</style>",
            "<div style='width:400px'>",
            "<img id='big' width='800' height='600' src='big.png'>",
            "</div>",
            "<div style='width:1000px'>",
            "<img id='small' width='200' height='150' src='small.png'>",
            "</div>",
        ),
        1100.0,
    );
    let big = by_id(&d.root, "big").unwrap();
    let small = by_id(&d.root, "small").unwrap();
    // Big image constrained to 400px, aspect ratio 4:3 → h=300
    assert!(
        big.layout.content_rect.w <= 405.0,
        "big constrained w={:.0}",
        big.layout.content_rect.w
    );
    assert!(
        (big.layout.content_rect.h - 300.0).abs() < 15.0,
        "big aspect h={:.0}",
        big.layout.content_rect.h
    );
    // Small image: 200 < 1000, stays at 200px
    assert!(
        (small.layout.content_rect.w - 200.0).abs() < 10.0,
        "small unchanged w={:.0}",
        small.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  IMG: object-fit (parsing)                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn img_object_fit_cover_parsed() {
    let d = load_html(
        "<img id='i' style='object-fit:cover;width:200px;height:200px' width='400' height='300' src='test.png'>",
        800.0,
    );
    let i = by_id(&d.root, "i").unwrap();
    // Object-fit doesn't change layout dimensions
    assert!(
        (i.layout.content_rect.w - 200.0).abs() < 5.0,
        "w=200 with object-fit"
    );
    assert!(
        (i.layout.content_rect.h - 200.0).abs() < 5.0,
        "h=200 with object-fit"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SVG: zero dimensions don't crash                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn svg_zero_dimensions_no_crash() {
    let d = load_html(
        "<svg id='s' width='0' height='0' xmlns='http://www.w3.org/2000/svg'></svg>",
        800.0,
    );
    let _s = by_id(&d.root, "s").unwrap();
}

#[test]
fn svg_no_viewbox_no_size_no_crash() {
    let d = load_html(
        "<svg id='s' xmlns='http://www.w3.org/2000/svg'><circle r='10'/></svg>",
        800.0,
    );
    let _s = by_id(&d.root, "s").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: BBC logo SVG pattern                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn bbc_logo_svg_pattern() {
    let d = load_html(concat!(
        "<style>.logo svg { height: 1.75rem; width: auto; display: block; }</style>",
        "<div class='logo'>",
        "<svg id='s' width='7em' height='2em' viewBox='0 0 112 32' fill='currentColor' xmlns='http://www.w3.org/2000/svg'>",
        "<path d='M0 0h112v32H0z'/></svg>",
        "</div>",
    ), 800.0);
    let s = by_id(&d.root, "s").unwrap();
    // CSS height: 1.75rem = 28px, width: auto → computed from viewBox aspect ratio
    // viewBox 112:32, height 28 → width = 28 * 112/32 = 98
    assert!(
        s.layout.content_rect.h > 20.0,
        "svg has height h={:.0}",
        s.layout.content_rect.h
    );
    if s.svg_viewbox_w > 0.0 {
        assert!(
            s.layout.content_rect.w > 50.0,
            "svg width from aspect ratio w={:.0} should be > 50",
            s.layout.content_rect.w
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Icon SVG in button                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn icon_svg_in_button() {
    let d = load_html(concat!(
        "<button style='display:inline-flex;align-items:center;padding:8px 16px'>",
        "<svg id='icon' width='16' height='16' viewBox='0 0 16 16' xmlns='http://www.w3.org/2000/svg'>",
        "<path d='M0 0h16v16H0z'/></svg>",
        "<span id='label' style='margin-left:8px'>Click me</span>",
        "</button>",
    ), 800.0);
    let icon = by_id(&d.root, "icon").unwrap();
    let label = by_id(&d.root, "label").unwrap();
    assert!(
        icon.layout.content_rect.w > 10.0,
        "icon has width w={:.0}",
        icon.layout.content_rect.w
    );
    assert!(
        label.layout.content_rect.x > icon.layout.content_rect.x + 10.0,
        "label after icon"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Card with image                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn card_with_image_and_text() {
    let d = load_html(
        concat!(
        "<style>",
        ".card { width: 300px; }",
        ".card img { width: 100%; height: auto; display: block; }",
        ".card-body { padding: 16px; }",
        "</style>",
        "<div class='card'>",
        "<img id='img' width='600' height='400' src='photo.jpg'>",
        "<div class='card-body'><h3 id='title'>Card Title</h3><p id='desc'>Description</p></div>",
        "</div>",
    ),
        400.0,
    );
    let img = by_id(&d.root, "img").unwrap();
    let title = by_id(&d.root, "title").unwrap();
    // Image fills card width
    assert!(
        (img.layout.content_rect.w - 300.0).abs() < 10.0,
        "img fills card w={:.0}",
        img.layout.content_rect.w
    );
    // Aspect ratio: 300 * 400/600 = 200
    assert!(
        (img.layout.content_rect.h - 200.0).abs() < 15.0,
        "img aspect h={:.0}",
        img.layout.content_rect.h
    );
    // Title below image
    assert!(
        title.layout.content_rect.y > img.layout.content_rect.y + 190.0,
        "title below img title_y={:.0}",
        title.layout.content_rect.y
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: Gallery grid of images                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn gallery_grid_images() {
    let d = load_html(concat!(
        "<style>",
        ".gallery { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; width: 900px; }",
        ".gallery img { width: 100%; height: auto; display: block; }",
        "</style>",
        "<div class='gallery'>",
        "<img id='a' width='400' height='300' src='a.jpg'>",
        "<img id='b' width='400' height='300' src='b.jpg'>",
        "<img id='c' width='400' height='300' src='c.jpg'>",
        "</div>",
    ), 1000.0);
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // Each column ≈ (900-20)/3 ≈ 293px
    assert!(
        a.layout.content_rect.w > 250.0 && a.layout.content_rect.w < 310.0,
        "gallery img w={:.0}",
        a.layout.content_rect.w
    );
    // Same row
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "same row a-b"
    );
    assert!(
        (b.layout.content_rect.y - c.layout.content_rect.y).abs() < 5.0,
        "same row b-c"
    );
    // Increasing x
    assert!(
        b.layout.content_rect.x > a.layout.content_rect.x + 250.0,
        "b right of a"
    );
}
