// CSS selector matching and @media query tests — covers specificity,
// combinators, pseudo-classes, attribute selectors, and responsive layout.

use webcore::load_html;
use webcore::load_html_vp;
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
fn by_class<'a>(root: &'a WebCore, cls: &str) -> Option<&'a WebCore> {
    if root
        .attributes
        .get("class")
        .map(|v| v.split_whitespace().any(|c| c == cls))
        .unwrap_or(false)
    {
        return Some(root);
    }
    for child in &root.children {
        if let Some(f) = by_class(child, cls) {
            return Some(f);
        }
    }
    None
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  BASIC SELECTORS                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn selector_tag() {
    let d = load_html("<style>p { color: red; }</style><p id='t'>X</p>", 800.0);
    assert_eq!(by_id(&d.root, "t").unwrap().style.color.r, 255);
}

#[test]
fn selector_class() {
    let d = load_html(
        "<style>.red { color: red; }</style><div class='red' id='t'>X</div>",
        800.0,
    );
    assert_eq!(by_id(&d.root, "t").unwrap().style.color.r, 255);
}

#[test]
fn selector_id() {
    let d = load_html(
        concat!(
            "<style>",
            "#special { color: blue; }",
            "</style>",
            "<div id='special'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "special").unwrap();
    assert_eq!(t.style.color.b, 255, "id selector blue");
}

#[test]
fn selector_universal() {
    let d = load_html(
        "<style>* { margin: 0; }</style><div id='t' style='width:100px;height:50px'>X</div>",
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        t.layout.resolved_margin_top < 1.0,
        "universal resets margin"
    );
}

#[test]
fn selector_multiple_classes() {
    let d = load_html("<style>.a.b { color: red; }</style><div class='a b' id='t'>X</div><div class='a' id='no'>Y</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "both classes match"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "single class no match"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMBINATORS                                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn combinator_descendant() {
    let d = load_html("<style>.parent p { color: red; }</style><div class='parent'><div><p id='t'>X</p></div></div><p id='no'>Y</p>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "descendant matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "outside no match"
    );
}

#[test]
fn combinator_child() {
    let d = load_html("<style>.parent > p { color: red; }</style><div class='parent'><p id='direct'>X</p><div><p id='deep'>Y</p></div></div>", 800.0);
    assert_eq!(
        by_id(&d.root, "direct").unwrap().style.color.r,
        255,
        "direct child matches"
    );
    assert_ne!(
        by_id(&d.root, "deep").unwrap().style.color.r,
        255,
        "grandchild no match"
    );
}

#[test]
fn combinator_adjacent_sibling() {
    let d = load_html("<style>h2 + p { color: red; }</style><h2>Title</h2><p id='adj'>Adjacent</p><p id='far'>Far</p>", 800.0);
    assert_eq!(
        by_id(&d.root, "adj").unwrap().style.color.r,
        255,
        "adjacent matches"
    );
    assert_ne!(
        by_id(&d.root, "far").unwrap().style.color.r,
        255,
        "non-adjacent no match"
    );
}

#[test]
fn combinator_general_sibling() {
    let d = load_html(
        "<style>h2 ~ p { color: red; }</style><h2>Title</h2><div>Gap</div><p id='sib'>Sibling</p>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "sib").unwrap().style.color.r,
        255,
        "general sibling matches"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  PSEUDO-CLASSES                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn pseudo_first_child() {
    let d = load_html("<style>li:first-child { color: red; }</style><ul><li id='first'>A</li><li id='second'>B</li></ul>", 800.0);
    assert_eq!(
        by_id(&d.root, "first").unwrap().style.color.r,
        255,
        "first-child red"
    );
    assert_ne!(
        by_id(&d.root, "second").unwrap().style.color.r,
        255,
        "second not red"
    );
}

#[test]
fn pseudo_last_child() {
    let d = load_html("<style>li:last-child { color: blue; }</style><ul><li id='first'>A</li><li id='last'>B</li></ul>", 800.0);
    assert_ne!(
        by_id(&d.root, "first").unwrap().style.color.b,
        255,
        "first not blue"
    );
    assert_eq!(
        by_id(&d.root, "last").unwrap().style.color.b,
        255,
        "last-child blue"
    );
}

#[test]
fn pseudo_nth_child_even() {
    let d = load_html("<style>li:nth-child(even) { color: red; }</style><ul><li id='a'>1</li><li id='b'>2</li><li id='c'>3</li><li id='dd'>4</li></ul>", 800.0);
    assert_ne!(
        by_id(&d.root, "a").unwrap().style.color.r,
        255,
        "1st not even"
    );
    assert_eq!(by_id(&d.root, "b").unwrap().style.color.r, 255, "2nd even");
    assert_ne!(
        by_id(&d.root, "c").unwrap().style.color.r,
        255,
        "3rd not even"
    );
    assert_eq!(by_id(&d.root, "dd").unwrap().style.color.r, 255, "4th even");
}

#[test]
fn pseudo_nth_child_odd() {
    let d = load_html("<style>li:nth-child(odd) { color: blue; }</style><ul><li id='a'>1</li><li id='b'>2</li><li id='c'>3</li></ul>", 800.0);
    assert_eq!(by_id(&d.root, "a").unwrap().style.color.b, 255, "1st odd");
    assert_ne!(
        by_id(&d.root, "b").unwrap().style.color.b,
        255,
        "2nd not odd"
    );
    assert_eq!(by_id(&d.root, "c").unwrap().style.color.b, 255, "3rd odd");
}

#[test]
fn pseudo_nth_child_3n() {
    let d = load_html("<style>li:nth-child(3n) { color: red; }</style><ul><li id='a'>1</li><li id='b'>2</li><li id='c'>3</li><li id='dd'>4</li><li id='e'>5</li><li id='f'>6</li></ul>", 800.0);
    assert_ne!(by_id(&d.root, "a").unwrap().style.color.r, 255);
    assert_ne!(by_id(&d.root, "b").unwrap().style.color.r, 255);
    assert_eq!(by_id(&d.root, "c").unwrap().style.color.r, 255, "3rd");
    assert_ne!(by_id(&d.root, "dd").unwrap().style.color.r, 255);
    assert_ne!(by_id(&d.root, "e").unwrap().style.color.r, 255);
    assert_eq!(by_id(&d.root, "f").unwrap().style.color.r, 255, "6th");
}

#[test]
fn pseudo_not() {
    let d = load_html(
        "<style>p:not(.skip) { color: red; }</style><p id='yes'>Y</p><p class='skip' id='no'>N</p>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "yes").unwrap().style.color.r,
        255,
        ":not matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        ":not excludes"
    );
}

#[test]
fn pseudo_first_of_type() {
    let d = load_html("<style>p:first-of-type { color: red; }</style><div><span>S</span><p id='first'>P1</p><p id='second'>P2</p></div>", 800.0);
    assert_eq!(
        by_id(&d.root, "first").unwrap().style.color.r,
        255,
        "first-of-type"
    );
    assert_ne!(
        by_id(&d.root, "second").unwrap().style.color.r,
        255,
        "not first-of-type"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  ATTRIBUTE SELECTORS                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn attr_exists() {
    let d = load_html("<style>[data-active] { color: red; }</style><div data-active id='t'>X</div><div id='no'>Y</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "[attr] matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "no attr no match"
    );
}

#[test]
fn attr_equals() {
    let d = load_html(
        concat!(
            "<style>[type='submit'] { color: red; }</style>",
            "<input type='submit' id='yes'><input type='text' id='no'>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "yes").unwrap().style.color.r,
        255,
        "[type=submit]"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "[type=text] no match"
    );
}

#[test]
fn attr_starts_with() {
    let d = load_html("<style>[class^='btn'] { color: red; }</style><div class='btn-primary' id='t'>X</div><div class='link' id='no'>Y</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "^= matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "^= no match"
    );
}

#[test]
fn attr_ends_with() {
    let d = load_html("<style>[href$='.pdf'] { color: red; }</style><a href='doc.pdf' id='t'>PDF</a><a href='doc.html' id='no'>HTML</a>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "$= matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "$= no match"
    );
}

#[test]
fn attr_contains() {
    let d = load_html("<style>[class*='warn'] { color: red; }</style><div class='alert-warning' id='t'>X</div><div class='info' id='no'>Y</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "*= matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "*= no match"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  SPECIFICITY                                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn specificity_class_beats_tag() {
    let d = load_html(
        "<style>p { color: blue; } .red { color: red; }</style><p class='red' id='t'>X</p>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "class > tag"
    );
}

#[test]
fn specificity_id_beats_class() {
    let d = load_html(
        concat!(
            "<style>.blue { color: blue; } ",
            "#t { color: red; }",
            "</style>",
            "<div class='blue' id='t'>X</div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "id > class"
    );
}

#[test]
fn specificity_inline_beats_id() {
    let d = load_html(
        concat!(
            "<style>",
            "#t { color: blue; }",
            "</style>",
            "<div id='t' style='color:red'>X</div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "inline > id"
    );
}

#[test]
fn specificity_important_beats_inline() {
    let d = load_html(
        concat!(
            "<style>p { color: blue !important; }</style>",
            "<p id='t' style='color:red'>X</p>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.b,
        255,
        "!important > inline"
    );
}

#[test]
fn specificity_later_rule_wins_same_specificity() {
    let d = load_html(
        "<style>.a { color: blue; } .a { color: red; }</style><div class='a' id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "later wins"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMMA-SEPARATED SELECTORS                                  ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn selector_group() {
    let d = load_html("<style>h1, h2, h3 { color: red; }</style><h1 id='a'>A</h1><h2 id='b'>B</h2><p id='no'>P</p>", 800.0);
    assert_eq!(
        by_id(&d.root, "a").unwrap().style.color.r,
        255,
        "h1 matches"
    );
    assert_eq!(
        by_id(&d.root, "b").unwrap().style.color.r,
        255,
        "h2 matches"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "p no match"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA QUERIES                                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_min_width_matches() {
    let d = load_html(
        concat!(
            "<style>",
            ".box { width: 200px; }",
            "@media (min-width: 600px) { .box { width: 400px; } }",
            "</style>",
            "<div class='box' id='t' style='height:50px'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 400.0).abs() < 10.0,
        "@media matches w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn media_min_width_no_match() {
    let d = load_html(
        concat!(
            "<style>",
            ".box { width: 200px; }",
            "@media (min-width: 1200px) { .box { width: 400px; } }",
            "</style>",
            "<div class='box' id='t' style='height:50px'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 200.0).abs() < 10.0,
        "@media no match w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn media_max_width() {
    let d = load_html(
        concat!(
            "<style>",
            ".box { width: 100px; }",
            "@media (max-width: 1000px) { .box { width: 300px; } }",
            "</style>",
            "<div class='box' id='t' style='height:50px'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 300.0).abs() < 10.0,
        "max-width matches w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn media_breakpoint_mobile_tablet_desktop() {
    let d = load_html(
        concat!(
            "<style>",
            ".cols { display: block; width: 100%; }",
            "@media (min-width: 768px) { .cols { display: flex; } }",
            "</style>",
            "<div class='cols' id='t' style='width:800px'>",
            "<div id='a' style='flex:1'>A</div><div id='b' style='flex:1'>B</div>",
            "</div>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // viewport=1024 > 768 → flex applies
    assert!(
        matches!(t.style.display, Display::Flex),
        "desktop=flex {:?}",
        t.style.display
    );
}

#[test]
fn media_nested_in_supports() {
    let d = load_html(
        concat!(
            "<style>",
            ".box { width: 100px; }",
            "@supports (display: grid) {",
            "  @media (min-width: 500px) { .box { width: 500px; } }",
            "}",
            "</style>",
            "<div class='box' id='t' style='height:50px'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.content_rect.w - 500.0).abs() < 10.0,
        "nested @media+@supports w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn media_multiple_conditions() {
    let d = load_html(
        concat!(
            "<style>",
            "@media (min-width: 500px) and (max-width: 1200px) { .box { color: red; } }",
            "</style>",
            "<div class='box' id='t'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.r, 255, "compound @media matches");
}

#[test]
fn media_screen_keyword() {
    let d = load_html(
        concat!(
            "<style>",
            "@media screen { .box { color: red; } }",
            "</style>",
            "<div class='box' id='t'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.r, 255, "@media screen matches");
}

#[test]
fn media_print_ignored() {
    let d = load_html(
        concat!(
            "<style>",
            "@media print { .box { color: red; } }",
            "</style>",
            "<div class='box' id='t'>X</div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert_ne!(t.style.color.r, 255, "@media print ignored on screen");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: responsive layout patterns                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn responsive_grid_columns() {
    let d = load_html(
        concat!(
            "<style>",
            ".grid { display:grid; width:100%; grid-template-columns:1fr; }",
            "@media (min-width: 600px) { .grid { grid-template-columns: 1fr 1fr; } }",
            "@media (min-width: 900px) { .grid { grid-template-columns: 1fr 1fr 1fr; } }",
            "</style>",
            "<div class='grid' style='width:1000px'>",
            "<div id='a'>A</div><div id='b'>B</div><div id='c'>C</div>",
            "</div>",
        ),
        1024.0,
    );
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    let c = by_id(&d.root, "c").unwrap();
    // viewport=1024 > 900 → 3 columns
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "a,b same row"
    );
    assert!(
        (b.layout.content_rect.y - c.layout.content_rect.y).abs() < 5.0,
        "b,c same row"
    );
}

#[test]
fn responsive_hide_on_mobile() {
    let d = load_html(
        concat!(
            "<style>",
            ".desktop-only { display: block; }",
            "@media (max-width: 767px) { .desktop-only { display: none; } }",
            "</style>",
            "<div class='desktop-only' id='t' style='height:50px'>Desktop</div>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        matches!(t.style.display, Display::Block),
        "visible on desktop"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  COMPLEX SELECTORS                                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn complex_selector_chain() {
    let d = load_html(
        concat!(
            "<style>.nav > ul > li.active > a { color: red; }</style>",
            "<div class='nav'><ul><li class='active'><a id='t'>Link</a></li></ul></div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "complex chain"
    );
}

#[test]
fn selector_with_pseudo_and_class() {
    let d = load_html("<style>.list li:first-child { color: red; }</style><div class='list'><ul><li id='t'>First</li><li>Second</li></ul></div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "class + pseudo"
    );
}

#[test]
fn selector_type_and_class() {
    let d = load_html("<style>div.special { color: red; }</style><div class='special' id='yes'>Y</div><span class='special' id='no'>N</span>", 800.0);
    assert_eq!(
        by_id(&d.root, "yes").unwrap().style.color.r,
        255,
        "div.special"
    );
    assert_ne!(
        by_id(&d.root, "no").unwrap().style.color.r,
        255,
        "span.special no match"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn empty_selector_no_crash() {
    let d = load_html("<style> { color: red; }</style><div id='t'>X</div>", 800.0);
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn invalid_selector_no_crash() {
    let d = load_html(
        "<style>##invalid { color: red; }</style><div id='t'>X</div>",
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn selector_with_escaped_chars_no_crash() {
    let d = load_html(
        r#"<style>.foo\:bar { color: red; }</style><div id='t'>X</div>"#,
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn many_selectors_performance() {
    let mut css = String::from("<style>");
    for i in 0..100 {
        css.push_str(&format!(".c{} {{ color: rgb({},0,0); }}", i, i));
    }
    css.push_str("</style><div class='c50' id='t'>X</div>");
    let d = load_html(&css, 800.0);
    let t = by_id(&d.root, "t").unwrap();
    assert_eq!(t.style.color.r, 50, "100 rules resolved");
}
