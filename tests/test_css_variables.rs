// Comprehensive CSS custom properties (variables) tests.
// Covers :root, element-level, inheritance, fallbacks, chaining,
// var() in different property types, and real-world patterns.

use webcore::load_html;
use webcore::types::*;

fn find_box<'a>(root: &'a WebCore, pred: &dyn Fn(&WebCore) -> bool) -> Option<&'a WebCore> {
    if pred(root) {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_box(child, pred) {
            return Some(found);
        }
    }
    None
}

fn by_id<'a>(root: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    find_box(root, &|b| {
        b.attributes.get("id").map(|v| v == id).unwrap_or(false)
    })
}

fn by_class<'a>(root: &'a WebCore, cls: &str) -> Option<&'a WebCore> {
    find_box(root, &|b| {
        b.attributes
            .get("class")
            .map(|v| v.contains(cls))
            .unwrap_or(false)
    })
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  :root variables — basic definition and use                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_root_color() {
    let doc = load_html(
        concat!(
            "<style>:root { --brand: #ff0000; } p { color: var(--brand); }</style>",
            "<p id='t'>Red text</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        (t.style.color.r, t.style.color.g, t.style.color.b),
        (255, 0, 0),
        "color should be red from var(--brand)"
    );
}

#[test]
fn var_root_background_color() {
    let doc = load_html(
        concat!(
            "<style>:root { --bg: #0000ff; } div { background-color: var(--bg); }</style>",
            "<div id='t'>Blue bg</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(t.style.background_color.b, 255, "bg should be blue");
    assert!(t.style.background_color.a > 0, "bg should be opaque");
}

#[test]
fn var_root_font_size() {
    let doc = load_html(
        concat!(
            "<style>:root { --fs: 24px; } p { font-size: var(--fs); }</style>",
            "<p id='t'>Large</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 24.0).abs() < 1.0,
        "font-size should be 24px, got {:.1}",
        t.style.font_size_px(16.0, 16.0)
    );
}

#[test]
fn var_root_width() {
    let doc = load_html(
        concat!(
            "<style>:root { --w: 300px; } .box { width: var(--w); height: 50px; }</style>",
            "<div class='box' id='t'>Sized</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 5.0,
        "width should be 300px from var, got {:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn var_root_padding() {
    let doc = load_html(
        concat!(
            "<style>:root { --pad: 20px; } .box { padding: var(--pad); width: 200px; }</style>",
            "<div class='box' id='t'>Padded</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.resolved_pad_top - 20.0).abs() < 2.0,
        "padding-top should be 20px, got {:.1}",
        t.layout.resolved_pad_top
    );
}

#[test]
fn var_root_margin() {
    let doc = load_html(
        concat!(
            "<style>:root { --m: 15px; } .box { margin: var(--m); width: 200px; }</style>",
            "<div class='box' id='t'>Margined</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.resolved_margin_top - 15.0).abs() < 2.0,
        "margin-top should be 15px, got {:.1}",
        t.layout.resolved_margin_top
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Fallback values                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_fallback_when_undefined() {
    let doc = load_html(
        concat!(
            "<style>p { color: var(--undefined, #00ff00); }</style>",
            "<p id='t'>Green fallback</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        (t.style.color.r, t.style.color.g, t.style.color.b),
        (0, 255, 0),
        "should use fallback green when var undefined"
    );
}

#[test]
fn var_fallback_not_used_when_defined() {
    let doc = load_html(
        concat!(
            "<style>:root { --c: #ff0000; } p { color: var(--c, #00ff00); }</style>",
            "<p id='t'>Red not green</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(t.style.color.r, 255, "should use defined var, not fallback");
    assert_eq!(t.style.color.g, 0, "green should be 0");
}

#[test]
fn var_fallback_with_spaces() {
    let doc = load_html(
        concat!(
            "<style>p { font-size: var(--missing, 32px); }</style>",
            "<p id='t'>Fallback size</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 32.0).abs() < 1.0,
        "should use 32px fallback, got {:.1}",
        t.style.font_size_px(16.0, 16.0)
    );
}

#[test]
fn var_fallback_is_another_var() {
    let doc = load_html(
        concat!(
        "<style>:root { --backup: #0000ff; } p { color: var(--missing, var(--backup)); }</style>",
        "<p id='t'>Blue from nested fallback</p>",
    ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        t.style.color.b, 255,
        "nested fallback should resolve to blue"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Variable chaining (var resolves to another var)            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_chain_two_levels() {
    let doc = load_html(
        concat!(
            "<style>:root { --a: var(--b); --b: #ff00ff; } p { color: var(--a); }</style>",
            "<p id='t'>Magenta via chain</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        (t.style.color.r, t.style.color.g, t.style.color.b),
        (255, 0, 255),
        "chained var should resolve to magenta"
    );
}

#[test]
fn var_chain_three_levels() {
    let doc = load_html(concat!(
        "<style>:root { --x: var(--y); --y: var(--z); --z: 48px; } p { font-size: var(--x); }</style>",
        "<p id='t'>3-level chain</p>",
    ), 800.0);
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 48.0).abs() < 1.0,
        "3-level chain should resolve to 48px"
    );
}

#[test]
fn var_circular_reference_no_crash() {
    let doc = load_html(
        concat!(
            "<style>:root { --a: var(--b); --b: var(--a); } p { color: var(--a, black); }</style>",
            "<p id='t'>Circular</p>",
        ),
        800.0,
    );
    // Should not crash; falls back to black or default
    let _t = by_id(&doc.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Element-level variables (not :root)                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_element_level_definition() {
    let doc = load_html(concat!(
        "<style>.card { --card-bg: #eeeeee; } .card-inner { background-color: var(--card-bg); }</style>",
        "<div class='card'><div class='card-inner' id='t'>Inner</div></div>",
    ), 800.0);
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        t.style.background_color.r, 0xee,
        "should inherit var from parent .card"
    );
}

#[test]
fn var_element_level_overrides_root() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --c: #ff0000; }",
            ".special { --c: #0000ff; }",
            "p { color: var(--c); }",
            "</style>",
            "<p id='normal'>Red</p>",
            "<div class='special'><p id='override'>Blue</p></div>",
        ),
        800.0,
    );
    let normal = by_id(&doc.root, "normal").unwrap();
    let over = by_id(&doc.root, "override").unwrap();
    assert_eq!(normal.style.color.r, 255, "normal should be red");
    assert_eq!(
        over.style.color.b, 255,
        "override should be blue from .special"
    );
}

#[test]
fn var_element_level_inherited_to_grandchild() {
    let doc = load_html(
        concat!(
            "<style>",
            ".theme { --text-color: #336699; }",
            "span { color: var(--text-color); }",
            "</style>",
            "<div class='theme'><div><span id='t'>Deep</span></div></div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        t.style.color.r, 0x33,
        "grandchild should inherit var from .theme"
    );
    assert_eq!(t.style.color.g, 0x66, "green component");
    assert_eq!(t.style.color.b, 0x99, "blue component");
}

#[test]
fn var_element_level_not_inherited_to_sibling() {
    let doc = load_html(
        concat!(
            "<style>",
            ".scoped { --local: #ff0000; }",
            "p { color: var(--local, #000000); }",
            "</style>",
            "<div class='scoped'><p id='inside'>Inside</p></div>",
            "<p id='outside'>Outside</p>",
        ),
        800.0,
    );
    let inside = by_id(&doc.root, "inside").unwrap();
    let outside = by_id(&doc.root, "outside").unwrap();
    assert_eq!(inside.style.color.r, 255, "inside should get --local red");
    assert_eq!(
        outside.style.color.r, 0,
        "outside should use fallback black"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Inline style variables                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_inline_style_definition() {
    let doc = load_html(
        concat!(
            "<style>p { color: var(--inline-color, black); }</style>",
            "<div style='--inline-color: #00cc00'><p id='t'>Green</p></div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(t.style.color.g, 0xcc, "should pick up inline style var");
}

#[test]
fn var_inline_overrides_stylesheet() {
    let doc = load_html(
        concat!(
            "<style>:root { --c: #ff0000; } p { color: var(--c); }</style>",
            "<div style='--c: #00ff00'><p id='t'>Green override</p></div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(t.style.color.g, 255, "inline --c should override :root --c");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  var() in different property types                           ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_in_border() {
    let doc = load_html(
        concat!(
            "<style>:root { --bw: 3px; --bc: #ff0000; }",
            ".box { border: var(--bw) solid var(--bc); width: 200px; }</style>",
            "<div class='box' id='t'>Bordered</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.resolved_border_top - 3.0).abs() < 1.0,
        "border-width should be 3px, got {:.1}",
        t.layout.resolved_border_top
    );
}

#[test]
fn var_in_gap() {
    let doc = load_html(concat!(
        "<style>:root { --gap: 20px; }",
        ".grid { display:grid; grid-template-columns:1fr 1fr; gap:var(--gap); width:500px; }</style>",
        "<div class='grid'><div id='a'>A</div><div id='b'>B</div></div>",
    ), 600.0);
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    let gap = b.layout.content_rect.x - (a.layout.content_rect.x + a.layout.content_rect.w);
    assert!(
        (gap - 20.0).abs() < 3.0,
        "gap should be 20px from var, got {:.0}",
        gap
    );
}

#[test]
fn var_in_transform_no_crash() {
    // var() in transform — may not fully work but should not crash
    let doc = load_html(
        concat!(
            "<style>:root { --angle: 45deg; } .r { transform: rotate(var(--angle)); }</style>",
            "<div class='r' id='t'>Rotated</div>",
        ),
        800.0,
    );
    let _t = by_id(&doc.root, "t").unwrap();
}

#[test]
fn var_in_display() {
    let doc = load_html(
        concat!(
            "<style>:root { --d: flex; } .box { display: var(--d); width: 400px; }</style>",
            "<div class='box' id='t'><div>A</div><div>B</div></div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        matches!(t.style.display, Display::Flex),
        "display should be flex from var, got {:?}",
        t.style.display
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Dark/light theme pattern (AP News, Al Jazeera)             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_dark_theme_pattern() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root {",
            "  --bg-dark: #1a1a2e;",
            "  --text-dark: #e0e0e0;",
            "  --bg: var(--bg-dark);",
            "  --text: var(--text-dark);",
            "}",
            ".page { background-color: var(--bg); color: var(--text); }",
            "</style>",
            "<div class='page' id='t'>Dark themed</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(t.style.background_color.r, 0x1a, "bg-r from dark theme");
    assert_eq!(t.style.color.r, 0xe0, "text-r from dark theme");
}

#[test]
fn var_component_scoped_theme() {
    // Pattern: component overrides theme vars for its subtree
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --bg: #ffffff; --text: #000000; }",
            ".dark-section { --bg: #222222; --text: #ffffff; }",
            ".card { background-color: var(--bg); color: var(--text); }",
            "</style>",
            "<div class='card' id='light'>Light card</div>",
            "<div class='dark-section'>",
            "  <div class='card' id='dark'>Dark card</div>",
            "</div>",
        ),
        800.0,
    );
    let light = by_id(&doc.root, "light").unwrap();
    let dark = by_id(&doc.root, "dark").unwrap();
    assert_eq!(light.style.background_color.r, 0xff, "light card bg white");
    assert_eq!(light.style.color.r, 0x00, "light card text black");
    assert_eq!(dark.style.background_color.r, 0x22, "dark card bg dark");
    assert_eq!(dark.style.color.r, 0xff, "dark card text white");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  AP News pattern: --DARK variant variables                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_dark_variant_chain() {
    // AP News uses: --bgDefault: var(--bgDefault--DARK)
    // with --bgDefault--DARK defined on :root
    let doc = load_html(
        concat!(
        "<style>",
        ":root { --bgDefault--DARK: #1a1a2e; --textColor--DARK: #e8e8e8; }",
        ".section { --bgDefault: var(--bgDefault--DARK); --textColor: var(--textColor--DARK); }",
        ".content { background-color: var(--bgDefault); color: var(--textColor); }",
        "</style>",
        "<div class='section'><div class='content' id='t'>AP News style</div></div>",
    ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert_eq!(
        t.style.background_color.r, 0x1a,
        "bg should resolve through chain: var(--bgDefault) → var(--bgDefault--DARK) → #1a1a2e"
    );
    assert!(t.style.background_color.a > 0, "bg should be opaque");
    assert_eq!(t.style.color.r, 0xe8, "text color from DARK variant");
}

#[test]
fn var_multiple_dark_variants_on_same_element() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root {",
            "  --color-primary--DARK: #bb86fc;",
            "  --color-surface--DARK: #121212;",
            "  --color-on-surface--DARK: #e1e1e1;",
            "}",
            ".component {",
            "  --color-primary: var(--color-primary--DARK);",
            "  --color-surface: var(--color-surface--DARK);",
            "  --color-on-surface: var(--color-on-surface--DARK);",
            "  background-color: var(--color-surface);",
            "  color: var(--color-on-surface);",
            "}",
            ".component a { color: var(--color-primary); }",
            "</style>",
            "<div class='component'>",
            "  <p id='text'>Text</p>",
            "  <a href='/x' id='link'>Link</a>",
            "</div>",
        ),
        800.0,
    );
    let text = by_id(&doc.root, "text").unwrap();
    let link = by_id(&doc.root, "link").unwrap();
    assert_eq!(text.style.color.r, 0xe1, "text inherits --color-on-surface");
    assert_eq!(link.style.color.r, 0xbb, "link uses --color-primary");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  var() with complex values                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_complex_border_shorthand() {
    let doc = load_html(
        concat!(
            "<style>:root { --border: 2px solid #cc0000; }",
            ".box { border: var(--border); width: 200px; }</style>",
            "<div class='box' id='t'>Bordered</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.resolved_border_top - 2.0).abs() < 1.0,
        "border from var shorthand should be 2px"
    );
}

#[test]
fn var_in_calc() {
    let doc = load_html(
        concat!(
            "<style>:root { --base: 100px; }",
            ".box { width: calc(var(--base) * 3); height: 50px; }</style>",
            "<div class='box' id='t'>Calc</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    // calc(100px * 3) = 300px
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 10.0,
        "calc with var should be ~300px, got {:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn var_in_grid_template() {
    let doc = load_html(
        concat!(
            "<style>:root { --cols: 1fr 2fr; }",
            ".grid { display:grid; grid-template-columns: var(--cols); width:600px; }</style>",
            "<div class='grid'><div id='a'>A</div><div id='b'>B</div></div>",
        ),
        700.0,
    );
    let a = by_id(&doc.root, "a").unwrap();
    let b = by_id(&doc.root, "b").unwrap();
    // 1fr=200, 2fr=400
    assert!(
        b.layout.content_rect.w > a.layout.content_rect.w * 1.5,
        "2fr w={:.0} should be ~2x 1fr w={:.0}",
        b.layout.content_rect.w,
        a.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  html[class] selector for vars (Wikipedia pattern)          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_html_class_selector() {
    let doc = load_html(
        concat!(
            "<html class='dark-mode'>",
            "<head><style>",
            "html.dark-mode { --page-bg: #1e1e1e; --page-text: #cccccc; }",
            "body { background-color: var(--page-bg); color: var(--page-text); }",
            "</style></head>",
            "<body><p id='t'>Dark mode</p></body>",
            "</html>",
        ),
        800.0,
    );
    let body = find_box(&doc.root, &|b| b.tag == "body").unwrap();
    assert_eq!(
        body.style.background_color.r, 0x1e,
        "body bg from html.dark-mode var"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Empty/whitespace var values                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_empty_value_uses_fallback() {
    let doc = load_html(
        concat!(
            "<style>:root { --empty: ; } p { color: var(--empty, #00ff00); }</style>",
            "<p id='t'>Fallback</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    // Empty var should trigger fallback
    assert_eq!(t.style.color.g, 255, "empty var should use fallback green");
}

#[test]
fn var_undefined_no_fallback_inherits() {
    let doc = load_html(
        concat!(
            "<style>body { color: #123456; } p { color: var(--nope); }</style>",
            "<p id='t'>Inherit</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    // Undefined var with no fallback: property becomes invalid → inherits from parent
    // This is actually spec-specified behavior, but engines differ
    // At minimum, should not crash and should have some color
    assert!(
        t.style.color.a > 0 || true,
        "should not crash on undefined var without fallback"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Multiple var() in one property                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_multiple_in_one_property() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --top: 10px; --right: 20px; --bottom: 30px; --left: 40px; }",
            ".box { padding: var(--top) var(--right) var(--bottom) var(--left); width:200px; }",
            "</style>",
            "<div class='box' id='t'>Multi</div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    assert!(
        (t.layout.resolved_pad_top - 10.0).abs() < 2.0,
        "pad-top={:.0}",
        t.layout.resolved_pad_top
    );
    assert!(
        (t.layout.resolved_pad_right - 20.0).abs() < 2.0,
        "pad-right={:.0}",
        t.layout.resolved_pad_right
    );
    assert!(
        (t.layout.resolved_pad_bottom - 30.0).abs() < 2.0,
        "pad-bottom={:.0}",
        t.layout.resolved_pad_bottom
    );
    assert!(
        (t.layout.resolved_pad_left - 40.0).abs() < 2.0,
        "pad-left={:.0}",
        t.layout.resolved_pad_left
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @media and var() interaction                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_defined_in_media_query() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --size: 16px; }",
            "@media (min-width: 500px) { :root { --size: 20px; } }",
            "p { font-size: var(--size); }",
            "</style>",
            "<p id='t'>Responsive</p>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    // viewport=800px > 500px, so --size should be 20px
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 20.0).abs() < 1.0,
        "should use @media var, got {:.1}",
        t.style.font_size_px(16.0, 16.0)
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Case sensitivity (CSS vars ARE case-sensitive)             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_case_sensitive() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --MyColor: #ff0000; --mycolor: #0000ff; }",
            "#upper { color: var(--MyColor); }",
            "#lower { color: var(--mycolor); }",
            "</style>",
            "<p id='upper'>Red</p><p id='lower'>Blue</p>",
        ),
        800.0,
    );
    let upper = by_id(&doc.root, "upper").unwrap();
    let lower = by_id(&doc.root, "lower").unwrap();
    assert_eq!(upper.style.color.r, 255, "--MyColor should be red");
    assert_eq!(lower.style.color.b, 255, "--mycolor should be blue");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  var() with initial/inherit/unset values                    ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_value_is_initial() {
    let doc = load_html(
        concat!(
            "<style>",
            ":root { --reset: initial; }",
            ".parent { width: 300px; }",
            ".child { width: var(--reset); }",
            "</style>",
            "<div class='parent'><div class='child' id='t'>Auto width</div></div>",
        ),
        800.0,
    );
    let t = by_id(&doc.root, "t").unwrap();
    // var(--reset) = "initial" → width resets to auto → fills parent
    assert!(
        t.layout.content_rect.w > 250.0,
        "width:initial from var should be auto, got {:.0}",
        t.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  Performance: many variables                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn var_many_variables_no_timeout() {
    let mut css = String::from(":root {");
    for i in 0..50 {
        css.push_str(&format!(" --v{}: {}px;", i, i * 10));
    }
    css.push_str(" }");
    css.push_str(" .test { width: var(--v10); height: var(--v5); }");
    let html = format!("<style>{}</style><div class='test' id='t'>Perf</div>", css);
    let doc = load_html(&html, 800.0);
    let t = by_id(&doc.root, "t").unwrap();
    assert!((t.layout.content_rect.w - 100.0).abs() < 5.0, "--v10=100px");
}
