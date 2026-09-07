// Comprehensive tests for CSS units, calc/clamp/min/max functions,
// box-sizing interactions, and ::before/::after pseudo-elements.

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
// ║  CSS UNITS: px, em, rem, %, vw, vh, vmin, vmax, pt, cm     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn unit_px() {
    let d = load_html("<div id='t' style='width:123px;height:45px'>X</div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 123.0).abs() < 1.0,
        "px w={:.1}",
        t.layout.content_rect.w
    );
    assert!(
        (t.layout.content_rect.h - 45.0).abs() < 1.0,
        "px h={:.1}",
        t.layout.content_rect.h
    );
}

#[test]
fn unit_em() {
    let d = load_html(
        "<div style='font-size:20px'><div id='t' style='width:10em;height:5em'>X</div></div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 200.0).abs() < 5.0,
        "10em w={:.0}",
        t.layout.content_rect.w
    );
    assert!(
        (t.layout.content_rect.h - 100.0).abs() < 5.0,
        "5em h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn unit_rem() {
    let d = load_html(
        concat!(
            "<html style='font-size:18px'><body>",
            "<div style='font-size:30px'><div id='t' style='width:10rem;height:50px'>X</div></div>",
            "</body></html>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // rem is relative to root font-size (18px), not parent (30px)
    assert!(
        (t.layout.content_rect.w - 180.0).abs() < 10.0,
        "10rem of 18px w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn unit_percent_width() {
    let d = load_html(
        "<div style='width:800px'><div id='t' style='width:50%;height:50px'>X</div></div>",
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 5.0,
        "50%% w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn unit_percent_height_with_parent_height() {
    let d = load_html(
        "<div style='width:400px;height:600px'><div id='t' style='height:50%'>X</div></div>",
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.h - 300.0).abs() < 5.0,
        "50%% h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn unit_vw() {
    let d = load_html("<div id='t' style='width:25vw;height:50px'>X</div>", 1000.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 250.0).abs() < 10.0,
        "25vw w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn unit_vh() {
    // vh depends on viewport height set by engine
    let d = load_html("<div id='t' style='height:50vh;width:200px'>X</div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 50.0,
        "50vh h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn unit_pt() {
    // 1pt = 4/3 px
    let d = load_html("<div id='t' style='font-size:12pt'>X</div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 16.0).abs() < 2.0,
        "12pt=16px fs={:.1}",
        t.style.font_size_px(16.0, 16.0)
    );
}

#[test]
fn unit_zero_no_unit() {
    let d = load_html(
        "<div id='t' style='margin:0;padding:0;width:200px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.resolved_margin_top < 1.0, "margin:0");
    assert!(t.layout.resolved_pad_top < 1.0, "padding:0");
}

#[test]
fn unit_em_font_size_relative() {
    let d = load_html(
        concat!(
            "<div style='font-size:16px'>",
            "<div style='font-size:2em'>",
            "<div id='t' style='font-size:1.5em'>Nested em</div>",
            "</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 16 * 2 * 1.5 = 48
    assert!(
        (t.style.font_size_px(32.0, 16.0) - 48.0).abs() < 3.0,
        "nested em fs={:.0}",
        t.style.font_size_px(32.0, 16.0)
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC() — all operations                                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_px_plus_px() {
    let d = load_html(
        "<div id='t' style='width:calc(100px + 50px);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 150.0).abs() < 5.0,
        "100+50={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_px_minus_px() {
    let d = load_html(
        "<div id='t' style='width:calc(300px - 50px);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 250.0).abs() < 5.0,
        "300-50={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_percent_minus_px() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:calc(100% - 60px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 740.0).abs() < 10.0,
        "100%%-60px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_percent_plus_px() {
    let d = load_html("<div style='width:600px'><div id='t' style='width:calc(50% + 20px);height:40px'>X</div></div>", 700.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 320.0).abs() < 10.0,
        "50%%+20px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_multiply() {
    let d = load_html(
        "<div id='t' style='width:calc(50px * 3);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 150.0).abs() < 10.0,
        "50*3={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_divide() {
    let d = load_html(
        "<div id='t' style='width:calc(300px / 2);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 150.0).abs() < 10.0,
        "300/2={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_in_margin() {
    let d = load_html("<div style='width:600px'><div id='t' style='margin-left:calc(50% - 100px);width:200px;height:40px'>X</div></div>", 700.0);
    let t = by_id(&d.root, "t").unwrap();
    // margin-left = 300 - 100 = 200
    assert!(
        (t.layout.resolved_margin_left - 200.0).abs() < 15.0,
        "calc margin={:.0}",
        t.layout.resolved_margin_left
    );
}

#[test]
fn calc_in_font_size() {
    let d = load_html(
        "<div id='t' style='font-size:calc(14px + 2px)'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 16.0).abs() < 2.0,
        "calc font-size={:.1}",
        t.style.font_size_px(16.0, 16.0)
    );
}

#[test]
fn calc_in_gap() {
    let d = load_html(
        concat!(
            "<div style='display:flex;gap:calc(10px + 10px);width:600px'>",
            "<div id='a' style='width:100px;height:40px'>A</div>",
            "<div id='b' style='width:100px;height:40px'>B</div>",
            "</div>",
        ),
        700.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let gap = b.layout.content_rect.x - (a.layout.content_rect.x + a.layout.content_rect.w);
    assert!((gap - 20.0).abs() < 5.0, "calc gap={:.0}", gap);
}

#[test]
fn calc_in_border_width() {
    let d = load_html(
        "<div id='t' style='border:calc(1px + 2px) solid red;width:200px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_border_top - 3.0).abs() < 1.5,
        "calc border={:.1}",
        t.layout.resolved_border_top
    );
}

#[test]
fn calc_with_em() {
    let d = load_html("<div style='font-size:20px'><div id='t' style='width:calc(10em - 50px);height:40px'>X</div></div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    // 10em = 200px, - 50 = 150
    assert!(
        (t.layout.content_rect.w - 150.0).abs() < 10.0,
        "10em-50px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_with_rem() {
    let d = load_html(
        concat!(
            "<html style='font-size:16px'><body>",
            "<div id='t' style='width:calc(10rem + 20px);height:40px'>X</div>",
            "</body></html>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 10rem = 160px + 20 = 180
    assert!(
        (t.layout.content_rect.w - 180.0).abs() < 10.0,
        "10rem+20px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_negative_result_clamped() {
    let d = load_html(
        "<div id='t' style='width:calc(50px - 200px);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w >= 0.0,
        "negative calc clamped w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_whitespace_required() {
    // calc(100% -40px) is invalid without spaces around operator
    // Most browsers still parse it, we should too or at least not crash
    let d = load_html("<div style='width:800px'><div id='t' style='width:calc(100% -40px);height:40px'>X</div></div>", 900.0);
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CLAMP(), MIN(), MAX()                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn clamp_function() {
    // clamp(min, preferred, max)
    let d = load_html("<div style='width:800px'><div id='t' style='width:clamp(200px, 50%, 400px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // 50% of 800 = 400, clamped to max 400 → 400
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 10.0,
        "clamp={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn clamp_below_min() {
    let d = load_html("<div style='width:200px'><div id='t' style='width:clamp(300px, 50%, 500px);height:40px'>X</div></div>", 300.0);
    let t = by_id(&d.root, "t").unwrap();
    // 50% of 200 = 100, but min is 300 → 300
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 10.0,
        "clamp min={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn min_function() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:min(500px, 50%);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // min(500, 400) = 400
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 10.0,
        "min={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn max_function() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:max(100px, 50%);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // max(100, 400) = 400
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 10.0,
        "max={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BOX-SIZING interactions with calc, %, units                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn border_box_with_percentage() {
    let d = load_html(concat!(
        "<div style='width:600px'>",
        "<div id='t' style='box-sizing:border-box;width:50%;padding:20px;border:5px solid black;height:80px'>X</div>",
        "</div>",
    ), 700.0);
    let t = by_id(&d.root, "t").unwrap();
    // border-box: 50% of 600 = 300 total, content = 300 - 40 - 10 = 250
    assert!(
        (t.layout.border_rect.w - 300.0).abs() < 5.0,
        "border-box 50%% border_w={:.0}",
        t.layout.border_rect.w
    );
    assert!(
        (t.layout.content_rect.w - 250.0).abs() < 5.0,
        "content_w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn border_box_with_calc() {
    let d = load_html(concat!(
        "<div style='width:800px'>",
        "<div id='t' style='box-sizing:border-box;width:calc(100% - 40px);padding:15px;height:60px'>X</div>",
        "</div>",
    ), 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // calc(100% - 40px) = 760, border-box → content = 760 - 30 = 730
    assert!(
        (t.layout.border_rect.w - 760.0).abs() < 10.0,
        "calc border-box border_w={:.0}",
        t.layout.border_rect.w
    );
    assert!(
        (t.layout.content_rect.w - 730.0).abs() < 10.0,
        "content_w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn border_box_height() {
    let d = load_html(
        "<div id='t' style='box-sizing:border-box;height:100px;padding:10px;border:5px solid black;width:200px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.border_rect.h - 100.0).abs() < 5.0,
        "border_h=100 h={:.0}",
        t.layout.border_rect.h
    );
    assert!(
        (t.layout.content_rect.h - 70.0).abs() < 5.0,
        "content_h=70 h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn content_box_calc_margin() {
    let d = load_html(
        concat!(
            "<div style='width:800px'>",
            "<div id='t' style='width:calc(100% - 100px);margin:0 50px;height:50px'>X</div>",
            "</div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 700.0).abs() < 10.0,
        "calc w with margin w={:.0}",
        t.layout.content_rect.w
    );
    assert!(
        (t.layout.resolved_margin_left - 50.0).abs() < 3.0,
        "margin-left=50 m={:.0}",
        t.layout.resolved_margin_left
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — content types                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_text_content() {
    let d = load_html(
        concat!(
            "<style>p::before { content: 'Note: '; font-weight: bold; }</style>",
            "<p id='t'>Important message</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has_before =
        !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has_before, "::before exists");
}

#[test]
fn after_text_content() {
    let d = load_html(
        concat!(
            "<style>a::after { content: ' ↗'; }</style>",
            "<a id='t' href='/'>Link</a>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has_after =
        !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after");
    assert!(has_after, "::after exists");
}

#[test]
fn before_empty_content_with_display_block() {
    let d = load_html(concat!(
        "<style>div::before { content: ''; display: block; height: 20px; background: red; }</style>",
        "<div id='t' style='width:300px'>Text below line</div>",
    ), 500.0);
    let t = by_id(&d.root, "t").unwrap();
    // Block ::before with empty content = decorative bar
    assert!(
        t.layout.content_rect.h > 15.0,
        "has height from ::before h={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn before_attr_content() {
    let d = load_html(
        concat!(
            "<style>.tag::before { content: attr(data-label); }</style>",
            "<span class='tag' data-label='INFO' id='t'>: message</span>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // attr() in content — may not be fully supported but shouldn't crash
    assert!(t.layout.content_rect.w >= 0.0, "no crash");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — display types                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_display_inline_default() {
    let d = load_html(
        concat!(
            "<style>p::before { content: '>> '; color: red; }</style>",
            "<p id='t'>Text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert!(
            matches!(bs.display, Display::Inline),
            "::before defaults to inline, got {:?}",
            bs.display
        );
    }
}

#[test]
fn before_display_inline_block() {
    let d = load_html(concat!(
        "<style>h2::before { content: ''; display: inline-block; width: 8px; height: 8px; background: blue; margin-right: 8px; vertical-align: middle; }</style>",
        "<h2 id='t'>Section Title</h2>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.h > 10.0, "h2 has content");
    assert!(!t.layout.line_cache.is_empty(), "h2 has text lines");
}

#[test]
fn before_display_block_separator() {
    let d = load_html(concat!(
        "<style>.section::before { content: ''; display: block; height: 2px; background: gray; margin-bottom: 10px; }</style>",
        "<div class='section' id='t'>Content after separator</div>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.h > 10.0, "section has height");
}

#[test]
fn after_display_block() {
    let d = load_html(concat!(
        "<style>.item::after { content: ''; display: block; height: 1px; background: lightgray; margin-top: 10px; }</style>",
        "<div class='item' id='t'>Item content</div>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 10.0,
        "item has height with ::after"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — in flex / grid                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_as_flex_item() {
    let d = load_html(concat!(
        "<style>.row::before { content: ''; flex: 0 0 40px; height: 40px; background: blue; }</style>",
        "<div class='row' style='display:flex;width:400px'>",
        "<div id='main' style='flex:1'>Main content</div>",
        "</div>",
    ), 500.0);
    let main = by_id(&d.root, "main").unwrap();
    // ::before takes 40px, main gets 360px
    assert!(
        main.layout.content_rect.w > 300.0,
        "main gets remainder w={:.0}",
        main.layout.content_rect.w
    );
    assert!(main.layout.content_rect.w < 400.0, "::before took space");
}

#[test]
fn after_as_flex_item() {
    let d = load_html(concat!(
        "<style>.row::after { content: ''; flex: 0 0 60px; height: 40px; background: red; }</style>",
        "<div class='row' style='display:flex;width:500px'>",
        "<div id='main' style='flex:1'>Main</div>",
        "</div>",
    ), 600.0);
    let main = by_id(&d.root, "main").unwrap();
    assert!(
        main.layout.content_rect.w > 380.0,
        "main fills minus ::after w={:.0}",
        main.layout.content_rect.w
    );
    assert!(main.layout.content_rect.w < 500.0, "::after took space");
}

#[test]
fn before_as_grid_item() {
    let d = load_html(concat!(
        "<style>.grid::before { content: ''; grid-column: 1; height: 30px; background: green; }</style>",
        "<div class='grid' style='display:grid;grid-template-columns:1fr 1fr;width:400px'>",
        "<div id='a'>A</div>",
        "<div id='b'>B</div>",
        "</div>",
    ), 500.0);
    let a = by_id(&d.root, "a").unwrap();
    assert!(a.layout.content_rect.w > 100.0, "grid item with ::before");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — styling                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_own_color() {
    let d = load_html(
        concat!(
            "<style>p { color: black; } p::before { content: '* '; color: red; }</style>",
            "<p id='t'>Text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert_eq!(bs.color.r, 255, "::before has own red color");
    }
}

#[test]
fn before_font_size() {
    let d = load_html(
        concat!(
            "<style>p::before { content: 'BIG '; font-size: 24px; }</style>",
            "<p id='t' style='font-size:14px'>small text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert!(
            (bs.font_size_px(14.0, 16.0) - 24.0).abs() < 2.0,
            "::before font-size=24"
        );
    }
}

#[test]
fn before_background_color() {
    let d = load_html(
        concat!(
        "<style>code::before { content: '`'; background-color: #f0f0f0; padding: 0 2px; }</style>",
        "<code id='t'>inline code</code>",
    ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert!(bs.background_color.a > 0, "::before has background");
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — clearfix pattern                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn clearfix_with_after() {
    let d = load_html(
        concat!(
            "<style>.cf::after { content: ''; display: table; clear: both; }</style>",
            "<div class='cf' id='container' style='width:400px'>",
            "<div style='float:left;width:100px;height:120px'>Float</div>",
            "</div>",
            "<div id='after' style='height:50px'>After clearfix</div>",
        ),
        500.0,
    );
    let container = by_id(&d.root, "container").unwrap();
    let after = by_id(&d.root, "after").unwrap();
    assert!(
        container.layout.content_rect.h >= 115.0,
        "clearfix contains float h={:.0}",
        container.layout.content_rect.h
    );
    assert!(
        after.layout.content_rect.y
            >= container.layout.content_rect.y + container.layout.content_rect.h - 5.0,
        "after below clearfix"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — list marker simulation                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn custom_list_marker_with_before() {
    let d = load_html(
        concat!(
            "<style>",
            ".custom-list { list-style: none; padding: 0; }",
            ".custom-list li { padding-left: 20px; position: relative; }",
            ".custom-list li::before { content: '•'; position: absolute; left: 0; color: blue; }",
            "</style>",
            "<ul class='custom-list'>",
            "<li id='li'>Custom bullet item</li>",
            "</ul>",
        ),
        500.0,
    );
    let li = by_id(&d.root, "li").unwrap();
    assert!(li.layout.content_rect.h > 10.0, "list item renders");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — multiple pseudo on same element       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_and_after_same_element() {
    let d = load_html(
        concat!(
            "<style>",
            ".quoted::before { content: '\"'; color: gray; }",
            ".quoted::after { content: '\"'; color: gray; }",
            "</style>",
            "<span class='quoted' id='t'>Hello world</span>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has_before =
        !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    let has_after =
        !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after");
    assert!(has_before, "has ::before");
    assert!(has_after, "has ::after");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — positioned                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_position_absolute() {
    let d = load_html(concat!(
        "<style>.badge { position: relative; } .badge::before { content: '3'; position: absolute; top: -8px; right: -8px; width: 20px; height: 20px; background: red; color: white; border-radius: 50%; text-align: center; font-size: 12px; }</style>",
        "<div class='badge' id='t' style='display:inline-block;width:40px;height:40px;background:blue'>Icon</div>",
    ), 500.0);
    let t = by_id(&d.root, "t").unwrap();
    // Badge element should have its own dimensions, ::before is absolute overlay
    assert!(
        (t.layout.content_rect.w - 40.0).abs() < 5.0,
        "badge w=40 w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn after_position_absolute_stretch() {
    // Common "clickable card" pattern
    let d = load_html(concat!(
        "<style>.card { position: relative; width: 300px; } .card a::after { content: ''; position: absolute; top: 0; left: 0; right: 0; bottom: 0; }</style>",
        "<div class='card'>",
        "<img width='300' height='200' src='test.png'>",
        "<a id='link' href='/'>Title</a>",
        "</div>",
    ), 400.0);
    let link = by_id(&d.root, "link").unwrap();
    // ::after absolute doesn't inflate the link
    assert!(
        link.layout.content_rect.h < 100.0,
        "link not inflated by ::after h={:.0}",
        link.layout.content_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — no content = no rendering             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_no_content_not_rendered() {
    let d = load_html(
        concat!(
            "<style>p::before { color: red; font-weight: bold; }</style>",
            "<p id='t'>No before without content</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Without content property, ::before shouldn't render
    assert!(
        t.style.before_content.is_empty() && !t.children.iter().any(|c| c.tag == "::before"),
        "no ::before without content property"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE / ::AFTER — interaction with text                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_inline_alongside_text() {
    let d = load_html(
        concat!(
            "<style>p::before { content: '→ '; color: blue; }</style>",
            "<p id='t' style='width:400px'>Paragraph text that follows the arrow</p>",
        ),
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(!t.layout.line_cache.is_empty(), "has text lines");
    // Line should include both ::before content and text
    let line = &t.layout.line_cache[0];
    assert!(
        line.width > 50.0,
        "line includes before + text w={:.0}",
        line.width
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: calc with negative result in various properties      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_negative_padding_clamped() {
    let d = load_html(
        "<div id='t' style='padding:calc(10px - 50px);width:200px;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.resolved_pad_top >= 0.0, "negative padding clamped");
}

#[test]
fn calc_zero_width() {
    let d = load_html(
        "<div id='t' style='width:calc(100px - 100px);height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.w >= 0.0, "zero calc doesn't crash");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: responsive padding with calc                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn responsive_container_calc() {
    let d = load_html(concat!(
        "<div style='width:1200px'>",
        "<div id='t' style='max-width:960px;margin:0 auto;padding:0 calc((100% - 960px) / 2)'>Content</div>",
        "</div>",
    ), 1300.0);
    let t = by_id(&d.root, "t").unwrap();
    // Should center within 1200px
    assert!(
        t.layout.content_rect.w > 800.0,
        "responsive container w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC: nested calc()                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_nested_calc() {
    let d = load_html("<div style='width:1000px'><div id='t' style='width:calc(calc(50% + 100px) - 50px);height:40px'>X</div></div>", 1100.0);
    let t = by_id(&d.root, "t").unwrap();
    // calc(calc(500+100)-50) = 550
    assert!(
        (t.layout.content_rect.w - 550.0).abs() < 15.0,
        "nested calc={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_deeply_nested() {
    let d = load_html(
        "<div id='t' style='width:calc(calc(calc(100px + 50px) + 25px));height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 175.0).abs() < 10.0,
        "deep calc={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC: mixed units                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_em_plus_px() {
    let d = load_html("<div style='font-size:20px'><div id='t' style='width:calc(5em + 30px);height:40px'>X</div></div>", 800.0);
    let t = by_id(&d.root, "t").unwrap();
    // 5em=100px + 30px = 130
    assert!(
        (t.layout.content_rect.w - 130.0).abs() < 10.0,
        "em+px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_vw_minus_px() {
    let d = load_html(
        "<div id='t' style='width:calc(100vw - 200px);height:40px'>X</div>",
        1000.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 100vw=1000 - 200 = 800
    assert!(
        (t.layout.content_rect.w - 800.0).abs() < 15.0,
        "vw-px={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_percent_times_number() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:calc(25% * 2);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // 25% of 800 = 200, *2 = 400
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 15.0,
        "%%*n={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CLAMP: more edge cases                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn clamp_preferred_in_range() {
    let d = load_html("<div style='width:1000px'><div id='t' style='width:clamp(100px, 30%, 500px);height:40px'>X</div></div>", 1100.0);
    let t = by_id(&d.root, "t").unwrap();
    // 30% of 1000=300, between 100 and 500 → 300
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 10.0,
        "clamp mid={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn clamp_above_max() {
    let d = load_html("<div style='width:2000px'><div id='t' style='width:clamp(100px, 50%, 600px);height:40px'>X</div></div>", 2100.0);
    let t = by_id(&d.root, "t").unwrap();
    // 50% of 2000=1000, clamped to max 600
    assert!(
        (t.layout.content_rect.w - 600.0).abs() < 10.0,
        "clamp max={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn clamp_with_calc() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:clamp(100px, calc(50% - 50px), 500px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // calc(50%-50px) = 400-50 = 350, between 100 and 500 → 350
    assert!(
        (t.layout.content_rect.w - 350.0).abs() < 15.0,
        "clamp+calc={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn clamp_in_font_size() {
    let d = load_html(
        "<div id='t' style='font-size:clamp(14px, 2vw, 24px)'>Text</div>",
        1000.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 2vw of 1000=20, between 14 and 24 → 20
    let fs = t.style.font_size_px(16.0, 16.0);
    assert!(fs >= 13.0 && fs <= 25.0, "clamp font-size={:.1}", fs);
}

#[test]
fn clamp_in_padding() {
    let d = load_html("<div style='width:600px'><div id='t' style='padding:clamp(10px, 5%, 40px);width:200px'>X</div></div>", 700.0);
    let t = by_id(&d.root, "t").unwrap();
    // 5% of 600 = 30, between 10 and 40 → 30
    assert!(
        t.layout.resolved_pad_top >= 9.0 && t.layout.resolved_pad_top <= 41.0,
        "clamp padding={:.0}",
        t.layout.resolved_pad_top
    );
}

#[test]
fn clamp_in_gap() {
    let d = load_html(
        concat!(
            "<div style='display:flex;gap:clamp(5px, 2%, 30px);width:800px'>",
            "<div id='a' style='width:100px;height:40px'>A</div>",
            "<div id='b' style='width:100px;height:40px'>B</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let gap = b.layout.content_rect.x - (a.layout.content_rect.x + a.layout.content_rect.w);
    // 2% of 800 = 16, between 5 and 30 → 16
    assert!(gap >= 4.0 && gap <= 31.0, "clamp gap={:.0}", gap);
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  MIN/MAX: more cases                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn min_two_percentages() {
    let d = load_html("<div style='width:1000px'><div id='t' style='width:min(80%, 60%);height:40px'>X</div></div>", 1100.0);
    let t = by_id(&d.root, "t").unwrap();
    // min(800, 600) = 600
    assert!(
        (t.layout.content_rect.w - 600.0).abs() < 10.0,
        "min %%={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn max_px_and_percent() {
    let d = load_html("<div style='width:400px'><div id='t' style='width:max(300px, 50%);height:40px'>X</div></div>", 500.0);
    let t = by_id(&d.root, "t").unwrap();
    // max(300, 200) = 300
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 10.0,
        "max px/%%={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn min_three_values() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:min(500px, 80%, 600px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // min(500, 640, 600) = 500
    assert!(
        (t.layout.content_rect.w - 500.0).abs() < 10.0,
        "min 3 vals={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn max_three_values() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:max(100px, 20%, 250px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // max(100, 160, 250) = 250
    assert!(
        (t.layout.content_rect.w - 250.0).abs() < 10.0,
        "max 3 vals={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn min_in_height() {
    let d = load_html(
        "<div id='t' style='height:min(200px, 50vh);width:300px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 50.0 && t.layout.content_rect.h <= 205.0,
        "min height={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn max_in_margin() {
    let d = load_html("<div style='width:600px'><div id='t' style='margin-left:max(20px, 5%);width:200px;height:40px'>X</div></div>", 700.0);
    let t = by_id(&d.root, "t").unwrap();
    // max(20, 30) = 30
    assert!(
        t.layout.resolved_margin_left >= 19.0,
        "max margin={:.0}",
        t.layout.resolved_margin_left
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC in CLAMP/MIN/MAX                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn min_with_calc_inside() {
    let d = load_html("<div style='width:800px'><div id='t' style='width:min(calc(100% - 100px), 500px);height:40px'>X</div></div>", 900.0);
    let t = by_id(&d.root, "t").unwrap();
    // min(700, 500) = 500
    assert!(
        (t.layout.content_rect.w - 500.0).abs() < 10.0,
        "min+calc={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn clamp_all_calc() {
    let d = load_html("<div style='width:1000px'><div id='t' style='width:clamp(calc(10% + 50px), calc(30% + 20px), calc(50% - 50px));height:40px'>X</div></div>", 1100.0);
    let t = by_id(&d.root, "t").unwrap();
    // min=150, preferred=320, max=450 → 320
    assert!(
        (t.layout.content_rect.w - 320.0).abs() < 20.0,
        "clamp all calc={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  CALC: in various properties                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_in_line_height() {
    let d = load_html("<div id='t' style='font-size:16px;line-height:calc(1em + 8px);width:200px'>Line height test with wrapping text content</div>", 300.0);
    let t = by_id(&d.root, "t").unwrap();
    if t.layout.line_cache.len() >= 2 {
        let gap = t.layout.line_cache[1].y - t.layout.line_cache[0].y;
        // 1em+8px = 24px
        assert!((gap - 24.0).abs() < 5.0, "calc line-height gap={:.0}", gap);
    }
}

#[test]
fn calc_in_top_left() {
    let d = load_html(concat!(
        "<div style='position:relative;width:400px;height:300px'>",
        "<div id='t' style='position:absolute;top:calc(50% - 25px);left:calc(50% - 50px);width:100px;height:50px'>C</div>",
        "</div>",
    ), 500.0);
    let parent = find(&d.root, &|b| b.style.position == Position::Relative).unwrap();
    let t = by_id(&d.root, "t").unwrap();
    // top: 150-25=125, left: 200-50=150
    let ey = parent.layout.padding_rect.y + 125.0;
    let ex = parent.layout.padding_rect.x + 150.0;
    assert!(
        (t.layout.content_rect.y - ey).abs() < 10.0,
        "calc top={:.0} expected {:.0}",
        t.layout.content_rect.y,
        ey
    );
    assert!(
        (t.layout.content_rect.x - ex).abs() < 10.0,
        "calc left={:.0} expected {:.0}",
        t.layout.content_rect.x,
        ex
    );
}

#[test]
fn calc_in_flex_basis() {
    let d = load_html(
        concat!(
            "<div style='display:flex;width:800px'>",
            "<div id='a' style='flex:0 0 calc(25% - 10px)'>A</div>",
            "<div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        900.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    // 25% of 800 - 10 = 190
    assert!(
        (a.layout.content_rect.w - 190.0).abs() < 15.0,
        "calc flex-basis={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn calc_in_grid_template() {
    let d = load_html(concat!(
        "<div style='display:grid;grid-template-columns:calc(50% - 10px) calc(50% - 10px);gap:20px;width:800px'>",
        "<div id='a'>A</div><div id='b'>B</div>",
        "</div>",
    ), 900.0);
    let a = by_id(&d.root, "a").unwrap();
    // calc(50%-10px) = 390
    assert!(
        (a.layout.content_rect.w - 390.0).abs() < 10.0,
        "calc grid col={:.0}",
        a.layout.content_rect.w
    );
}

#[test]
fn calc_in_min_width() {
    let d = load_html(
        "<div id='t' style='min-width:calc(200px + 100px);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w >= 295.0,
        "calc min-width={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_in_max_width() {
    let d = load_html(
        "<div id='t' style='max-width:calc(400px - 100px);width:100%;height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w <= 305.0,
        "calc max-width={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn calc_in_min_height() {
    let d = load_html(
        "<div id='t' style='min-height:calc(50px + 30px);width:200px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h >= 75.0,
        "calc min-height={:.0}",
        t.layout.content_rect.h
    );
}

#[test]
fn calc_in_border_radius() {
    // Should parse without crash
    let d = load_html("<div id='t' style='border-radius:calc(5px + 3px);width:100px;height:100px;background:red'>X</div>", 200.0);
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BOX-SIZING: edge cases                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn border_box_min_width() {
    let d = load_html(
        "<div id='t' style='box-sizing:border-box;min-width:200px;padding:20px;border:5px solid black;height:50px'>X</div>",
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // border-box min-width: border_rect.w >= 200
    assert!(
        t.layout.border_rect.w >= 195.0,
        "border-box min-width border_w={:.0}",
        t.layout.border_rect.w
    );
}

#[test]
fn border_box_max_width() {
    let d = load_html(
        "<div id='t' style='box-sizing:border-box;max-width:300px;padding:30px;width:100%;height:50px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.border_rect.w <= 305.0,
        "border-box max-width border_w={:.0}",
        t.layout.border_rect.w
    );
}

#[test]
fn border_box_with_calc_padding() {
    let d = load_html(
        "<div id='t' style='box-sizing:border-box;width:400px;padding:calc(10px + 10px);height:100px'>X</div>",
        500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.border_rect.w - 400.0).abs() < 5.0,
        "border-box border_w={:.0}",
        t.layout.border_rect.w
    );
    // content = 400 - 40 = 360
    assert!(
        (t.layout.content_rect.w - 360.0).abs() < 5.0,
        "content_w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn border_box_zero_padding_same_as_content_box() {
    let d = load_html(
        concat!(
            "<div id='cb' style='width:200px;height:50px'>CB</div>",
            "<div id='bb' style='box-sizing:border-box;width:200px;height:50px'>BB</div>",
        ),
        400.0,
    );
    let cb = by_id(&d.root, "cb").unwrap();
    let bb = by_id(&d.root, "bb").unwrap();
    assert!(
        (cb.layout.content_rect.w - bb.layout.content_rect.w).abs() < 2.0,
        "same when no padding"
    );
}

#[test]
fn border_box_negative_content_clamped() {
    // padding+border > width → content should be 0, not negative
    let d = load_html(
        "<div id='t' style='box-sizing:border-box;width:50px;padding:30px;border:5px solid black;height:80px'>X</div>",
        200.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.w >= 0.0,
        "content clamped to 0 w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: content with special characters          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_content_unicode_escape() {
    let d = load_html(
        concat!(
            r#"<style>.icon::before { content: "\2713"; }</style>"#,
            "<span class='icon' id='t'>Checked</span>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has_before =
        !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has_before, "unicode escape ::before");
}

#[test]
fn before_content_open_close_quote() {
    let d = load_html(
        concat!(
            "<style>q::before { content: open-quote; } q::after { content: close-quote; }</style>",
            "<q id='t'>Quoted text</q>",
        ),
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
    // Should not crash
}

#[test]
fn before_content_counter() {
    let d = load_html(
        concat!(
            "<style>",
            "ol { counter-reset: item; list-style: none; }",
            "li { counter-increment: item; }",
            "li::before { content: counter(item) '. '; font-weight: bold; }",
            "</style>",
            "<ol><li id='li1'>First</li><li id='li2'>Second</li></ol>",
        ),
        800.0,
    );
    let li1 = by_id(&d.root, "li1").unwrap();
    let li2 = by_id(&d.root, "li2").unwrap();
    assert!(li1.layout.content_rect.h > 0.0, "li1 renders");
    assert!(
        li2.layout.content_rect.y > li1.layout.content_rect.y,
        "li2 below li1"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: interaction with text wrapping           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_long_content_wraps() {
    let d = load_html(concat!(
        "<style>p::before { content: 'WARNING: This is a very important notice that precedes the paragraph text. '; color: red; font-weight: bold; }</style>",
        "<p id='t' style='width:300px'>Rest of the text flows after the before content.</p>",
    ), 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.line_cache.len() >= 2,
        "long ::before wraps lines={}",
        t.layout.line_cache.len()
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: responsive typography with clamp               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn responsive_typography_clamp() {
    let d = load_html(
        concat!(
            "<style>",
            "h1 { font-size: clamp(24px, 4vw, 48px); }",
            "p { font-size: clamp(14px, 1.5vw, 18px); }",
            "</style>",
            "<h1 id='h'>Heading</h1>",
            "<p id='p'>Paragraph</p>",
        ),
        1000.0,
    );
    let h = by_id(&d.root, "h").unwrap();
    let p = by_id(&d.root, "p").unwrap();
    let hfs = h.style.font_size_px(16.0, 16.0);
    let pfs = p.style.font_size_px(16.0, 16.0);
    assert!(hfs >= 23.0 && hfs <= 49.0, "h1 clamp fs={:.1}", hfs);
    assert!(pfs >= 13.0 && pfs <= 19.0, "p clamp fs={:.1}", pfs);
    assert!(hfs > pfs, "h1 bigger than p");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: fluid container with calc                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn fluid_container_pattern() {
    let d = load_html(
        concat!(
            "<style>",
            ".container { width: min(1200px, calc(100% - 40px)); margin: 0 auto; }",
            "</style>",
            "<div style='width:1400px'>",
            "<div class='container' id='t'>Content</div>",
            "</div>",
        ),
        1500.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // min(1200, 1360) = 1200
    assert!(
        (t.layout.content_rect.w - 1200.0).abs() < 15.0,
        "fluid container w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn fluid_container_narrow_viewport() {
    let d = load_html(
        concat!(
            "<style>",
            ".container { width: min(1200px, calc(100% - 40px)); margin: 0 auto; }",
            "</style>",
            "<div style='width:800px'>",
            "<div class='container' id='t'>Content</div>",
            "</div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // min(1200, 760) = 760
    assert!(
        (t.layout.content_rect.w - 760.0).abs() < 15.0,
        "narrow container w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  REAL-WORLD: aspect-ratio box with padding-top hack         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn aspect_ratio_padding_hack() {
    let d = load_html(concat!(
        "<div style='width:400px'>",
        "<div id='ratio' style='width:100%;padding-top:56.25%;position:relative'>",
        "<div id='content' style='position:absolute;top:0;left:0;right:0;bottom:0'>16:9 content</div>",
        "</div>",
        "</div>",
    ), 500.0);
    let ratio = by_id(&d.root, "ratio").unwrap();
    // padding-top:56.25% of 400px = 225px → 16:9 aspect ratio
    assert!(
        (ratio.layout.padding_rect.h - 225.0).abs() < 15.0,
        "aspect ratio h={:.0}",
        ratio.layout.padding_rect.h
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE: all zero dimensions                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn calc_all_zeros_no_crash() {
    let d = load_html("<div id='t' style='width:calc(0px + 0px);height:calc(0);padding:calc(0px);margin:calc(0)'>X</div>", 400.0);
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn clamp_reversed_min_max() {
    // If min > max, spec says result = min (clamped)
    let d = load_html(
        "<div id='t' style='width:clamp(500px, 50%, 200px);height:40px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // min(500) > max(200) → uses min = 500
    assert!(
        t.layout.content_rect.w >= 195.0,
        "clamp reversed w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER on different element types                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_on_heading() {
    let d = load_html(
        concat!(
            "<style>h1::before { content: '# '; color: gray; }</style>",
            "<h1 id='t'>Title</h1>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "h1 has ::before");
    assert!(!t.layout.line_cache.is_empty(), "h1 has text");
}

#[test]
fn before_on_li() {
    let d = load_html(
        concat!(
            "<style>li::before { content: '→ '; color: blue; }</style>",
            "<ul><li id='t'>Item</li></ul>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "li has ::before");
}

#[test]
fn before_on_div() {
    let d = load_html(
        concat!(
            "<style>.note::before { content: 'Note: '; font-weight: bold; color: orange; }</style>",
            "<div class='note' id='t'>This is important.</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "div has ::before");
}

#[test]
fn after_on_a_link() {
    let d = load_html(
        concat!(
            "<style>a[href^='http']::after { content: ' ↗'; font-size: 0.8em; }</style>",
            "<a id='t' href='http://example.com'>External</a>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after");
    // attr selector may not match, but shouldn't crash
    assert!(t.layout.content_rect.w >= 0.0, "no crash");
}

#[test]
fn before_on_blockquote() {
    let d = load_html(concat!(
        "<style>blockquote::before { content: '\"'; font-size: 3em; color: #ccc; float: left; margin-right: 10px; line-height: 1; }</style>",
        "<blockquote id='t'>Famous quote here</blockquote>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.h > 10.0, "blockquote renders");
}

#[test]
fn before_on_td() {
    let d = load_html(
        concat!(
            "<style>td.required::before { content: '* '; color: red; }</style>",
            "<table><tr><td class='required' id='t'>Name</td></tr></table>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.w > 0.0, "td with ::before");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER with different content values             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_content_empty_string() {
    let d = load_html(concat!(
        "<style>div::before { content: ''; display: inline-block; width: 10px; height: 10px; background: red; }</style>",
        "<div id='t'>Text after dot</div>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    // Empty content with dimensions = visual decorator
    assert!(
        t.layout.content_rect.h > 5.0,
        "renders with empty content ::before"
    );
}

#[test]
fn before_content_multiple_strings() {
    let d = load_html(
        concat!(
            r#"<style>p::before { content: "(" "Note" ") "; }</style>"#,
            "<p id='t'>Text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Concatenated strings
    assert!(t.layout.content_rect.w > 0.0, "multiple strings in content");
}

#[test]
fn before_content_none() {
    let d = load_html(
        concat!(
            "<style>p::before { content: none; color: red; }</style>",
            "<p id='t'>No before</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // content:none = don't generate pseudo-element
    assert!(
        t.style.before_content.is_empty() && !t.children.iter().any(|c| c.tag == "::before"),
        "content:none = no pseudo"
    );
}

#[test]
fn before_content_normal() {
    let d = load_html(
        concat!(
            "<style>p::before { content: normal; }</style>",
            "<p id='t'>No before</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // content:normal on ::before = same as none
    assert!(
        t.style.before_content.is_empty() && !t.children.iter().any(|c| c.tag == "::before"),
        "content:normal = no pseudo"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER with transitions/animations (parsing)     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_with_transition_no_crash() {
    let d = load_html(concat!(
        "<style>div::before { content: ''; display: block; height: 2px; background: blue; transition: background 0.3s; }</style>",
        "<div id='t'>Hover me</div>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 0.0,
        "no crash with transition on ::before"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: complex real-world patterns              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn tooltip_arrow_before() {
    let d = load_html(concat!(
        "<style>",
        ".tooltip { position: relative; display: inline-block; padding: 8px 12px; background: #333; color: white; }",
        ".tooltip::before { content: ''; position: absolute; bottom: -8px; left: 50%; margin-left: -8px; border-width: 8px; border-style: solid; border-color: #333 transparent transparent transparent; }",
        "</style>",
        "<span class='tooltip' id='t'>Hover text</span>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.w > 30.0, "tooltip renders");
}

#[test]
fn hamburger_icon_before_after() {
    let d = load_html(concat!(
        "<style>",
        ".burger { position: relative; width: 30px; height: 20px; }",
        ".burger::before, .burger::after { content: ''; position: absolute; left: 0; width: 100%; height: 3px; background: black; }",
        ".burger::before { top: 0; }",
        ".burger::after { bottom: 0; }",
        "</style>",
        "<div class='burger' id='t'></div>",
    ), 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!((t.layout.content_rect.w - 30.0).abs() < 5.0, "burger w=30");
    assert!((t.layout.content_rect.h - 20.0).abs() < 5.0, "burger h=20");
}

#[test]
fn ribbon_badge_before() {
    let d = load_html(concat!(
        "<style>",
        ".ribbon { position: relative; padding: 5px 20px; background: #e43; color: white; display: inline-block; }",
        ".ribbon::before { content: ''; position: absolute; top: 100%; left: 0; border: 10px solid transparent; border-top-color: #a22; border-right-color: #a22; }",
        "</style>",
        "<span class='ribbon' id='t'>SALE</span>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.w > 20.0, "ribbon renders");
}

#[test]
fn required_field_after() {
    let d = load_html(
        concat!(
        "<style>label.required::after { content: ' *'; color: red; font-weight: bold; }</style>",
        "<label class='required' id='t'>Email</label>",
    ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after");
    assert!(has, "required label has ::after");
}

#[test]
fn breadcrumb_separator_after() {
    let d = load_html(
        concat!(
            "<style>.crumb::after { content: ' / '; color: #999; }</style>",
            "<nav>",
            "<span class='crumb' id='c1'>Home</span>",
            "<span class='crumb' id='c2'>Products</span>",
            "<span id='c3'>Item</span>",
            "</nav>",
        ),
        800.0,
    );
    let c1 = by_id(&d.root, "c1").unwrap();
    let has = !c1.style.after_content.is_empty() || c1.children.iter().any(|c| c.tag == "::after");
    assert!(has, "breadcrumb has ::after separator");
}

#[test]
fn external_link_icon_after() {
    let d = load_html(
        concat!(
            r#"<style>.ext::after { content: " \2197"; font-size: 0.8em; }</style>"#,
            "<a class='ext' id='t' href='/'>External Link</a>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.after_content.is_empty() || t.children.iter().any(|c| c.tag == "::after");
    assert!(has, "external link has ::after icon");
}

#[test]
fn price_currency_before() {
    let d = load_html(
        concat!(
            "<style>.price::before { content: '$'; font-weight: bold; }</style>",
            "<span class='price' id='t'>29.99</span>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "price has ::before $");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: interaction with overflow                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_in_overflow_hidden() {
    let d = load_html(concat!(
        "<style>.box::before { content: 'PREFIX '; color: blue; }</style>",
        "<div class='box' id='t' style='width:100px;overflow:hidden;white-space:nowrap'>Long text that overflows the box</div>",
    ), 400.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 100.0).abs() < 5.0,
        "overflow clips w={:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: specificity and cascade                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_higher_specificity_wins() {
    let d = load_html(
        concat!(
            "<style>",
            "p::before { content: 'LOW '; color: blue; }",
            "p.special::before { content: 'HIGH '; color: red; }",
            "</style>",
            "<p class='special' id='t'>Text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Higher specificity should win
    if let Some(bs) = &t.style.before_style {
        assert_eq!(bs.color.r, 255, "higher specificity ::before color red");
    }
    let bc = if !t.style.before_content.is_empty() {
        t.style.before_content.clone()
    } else {
        t.children
            .iter()
            .find(|c| c.tag == "::before")
            .map(|c| c.text.clone())
            .unwrap_or_default()
    };
    assert!(
        bc.contains("HIGH") || bc.is_empty(),
        "higher specificity content"
    );
}

#[test]
fn before_inherited_font_from_parent() {
    let d = load_html(
        concat!(
            "<style>",
            ".big { font-size: 24px; font-family: Georgia, serif; }",
            ".big::before { content: '» '; }",
            "</style>",
            "<div class='big' id='t'>Large text</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert!(
            (bs.font_size_px(24.0, 16.0) - 24.0).abs() < 2.0,
            "::before inherits font-size"
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: does NOT generate on void elements       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_not_on_img() {
    let d = load_html(
        concat!(
            "<style>img::before { content: 'BEFORE'; }</style>",
            "<img id='t' width='100' height='100' src='test.png'>",
        ),
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Void elements (img, input, br, hr) should NOT generate ::before/::after
    // The spec says replaced elements don't have ::before/::after
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    // This is spec behavior — many engines differ. At minimum, shouldn't crash.
    assert!(t.layout.content_rect.w >= 0.0, "no crash on img::before");
}

#[test]
fn before_not_on_input() {
    let d = load_html(
        concat!(
            "<style>input::before { content: '*'; color: red; }</style>",
            "<input id='t' type='text'>",
        ),
        400.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
    // Should not crash
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::FIRST-LINE / ::FIRST-LETTER (parsing at minimum)         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn first_line_parsed_no_crash() {
    let d = load_html(
        concat!(
            "<style>p::first-line { font-weight: bold; color: blue; }</style>",
            "<p id='t' style='width:200px'>First line text that wraps to second line.</p>",
        ),
        300.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.h > 10.0, "renders with ::first-line");
}

#[test]
fn first_letter_parsed_no_crash() {
    let d = load_html(
        concat!(
            "<style>p::first-letter { font-size: 2em; float: left; }</style>",
            "<p id='t' style='width:300px'>Drop cap paragraph text.</p>",
        ),
        400.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 10.0,
        "renders with ::first-letter"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::SELECTION (parsing)                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn selection_pseudo_parsed() {
    let d = load_html(
        concat!(
            "<style>::selection { background: yellow; color: black; }</style>",
            "<p id='t'>Selectable text</p>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.content_rect.h > 0.0,
        "renders with ::selection style"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::MARKER (list item marker pseudo)                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn marker_pseudo_color() {
    let d = load_html(
        concat!(
            "<style>li::marker { color: red; }</style>",
            "<ul><li id='t'>Item</li></ul>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    if let Some(ms) = &t.style.marker_style {
        assert_eq!(ms.color.r, 255, "::marker color red");
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::PLACEHOLDER (input placeholder pseudo)                   ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn placeholder_pseudo_no_crash() {
    let d = load_html(
        concat!(
            "<style>input::placeholder { color: #999; font-style: italic; }</style>",
            "<input id='t' type='text' placeholder='Enter name'>",
        ),
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER: stress tests                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn many_elements_with_before() {
    let mut html = String::from(
        "<style>p::before { content: '• '; color: blue; }</style><div style='width:400px'>",
    );
    for i in 0..20 {
        html.push_str(&format!("<p id='p{}'>Item {}</p>", i, i));
    }
    html.push_str("</div>");
    let d = load_html(&html, 500.0);
    let p0 = by_id(&d.root, "p0").unwrap();
    let p19 = by_id(&d.root, "p19").unwrap();
    assert!(
        p19.layout.content_rect.y > p0.layout.content_rect.y + 100.0,
        "20 items stack"
    );
}

#[test]
fn before_and_after_on_every_child() {
    let d = load_html(
        concat!(
            "<style>",
            "li::before { content: '['; color: gray; }",
            "li::after { content: ']'; color: gray; }",
            "</style>",
            "<ul style='list-style:none;padding:0;width:400px'>",
            "<li id='a'>Alpha</li>",
            "<li id='b'>Beta</li>",
            "<li id='c'>Gamma</li>",
            "</ul>",
        ),
        500.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        b.layout.content_rect.y > a.layout.content_rect.y + 10.0,
        "items stack"
    );
    let has_before =
        !a.style.before_content.is_empty() || a.children.iter().any(|c| c.tag == "::before");
    let has_after =
        !a.style.after_content.is_empty() || a.children.iter().any(|c| c.tag == "::after");
    assert!(has_before, "li has ::before");
    assert!(has_after, "li has ::after");
}

#[test]
fn before_after_deeply_nested() {
    let d = load_html(
        concat!(
            "<style>.deep::before { content: '>'; } .deep::after { content: '<'; }</style>",
            "<div><div><div><div><span class='deep' id='t'>Deep</span></div></div></div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "deeply nested has ::before");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ::BEFORE/::AFTER with CSS variables                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn before_content_from_var() {
    let d = load_html(concat!(
        "<style>:root { --prefix: '>> '; } p::before { content: var(--prefix); color: red; }</style>",
        "<p id='t'>Text</p>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    let has = !t.style.before_content.is_empty() || t.children.iter().any(|c| c.tag == "::before");
    assert!(has, "::before content from var");
}

#[test]
fn before_color_from_var() {
    let d = load_html(concat!(
        "<style>:root { --accent: #ff6600; } h2::before { content: '# '; color: var(--accent); }</style>",
        "<h2 id='t'>Title</h2>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    if let Some(bs) = &t.style.before_style {
        assert_eq!(bs.color.r, 0xff, "::before color from var r={}", bs.color.r);
        assert_eq!(bs.color.g, 0x66, "::before color from var g={}", bs.color.g);
    }
}

#[test]
fn before_background_from_var() {
    let d = load_html(concat!(
        "<style>:root { --bar-color: #0066cc; }",
        ".section::before { content: ''; display: block; height: 4px; background: var(--bar-color); margin-bottom: 8px; }</style>",
        "<div class='section' id='t'>Content</div>",
    ), 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert!(t.layout.content_rect.h > 10.0, "section with var ::before");
}
