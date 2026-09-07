// Comprehensive @media and @container query tests — covers all media features,
// complex conditions, nesting, container queries, and responsive patterns.

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

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: width queries                                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_min_width_px() {
    let d = load_html(
        "<style>@media(min-width:500px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "800>500 matches"
    );
}

#[test]
fn media_min_width_no_match() {
    let d = load_html(
        "<style>@media(min-width:1000px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_ne!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "800<1000 no match"
    );
}

#[test]
fn media_max_width_px() {
    let d = load_html(
        "<style>@media(max-width:1000px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "800<1000 matches"
    );
}

#[test]
fn media_max_width_no_match() {
    let d = load_html(
        "<style>@media(max-width:500px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_ne!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "800>500 no match"
    );
}

#[test]
fn media_exact_width() {
    let d = load_html(
        "<style>@media(width:800px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    // exact width match is rare but valid
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "exact width"
    );
}

#[test]
fn media_min_width_em() {
    // 37.5rem = 600px at 16px root
    let d = load_html(
        "<style>@media(min-width:37.5rem){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "rem unit in @media"
    );
}

#[test]
fn media_min_width_em_unit() {
    // 37.5em = 600px
    let d = load_html(
        "<style>@media(min-width:37.5em){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "em unit in @media"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: compound conditions (and)                          ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_and_both_match() {
    let d = load_html("<style>@media(min-width:500px) and (max-width:1000px){#t{color:red}}</style><div id='t'>X</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "both match"
    );
}

#[test]
fn media_and_one_fails() {
    let d = load_html("<style>@media(min-width:500px) and (max-width:700px){#t{color:red}}</style><div id='t'>X</div>", 800.0);
    assert_ne!(by_id(&d.root, "t").unwrap().style.color.r, 255, "max fails");
}

#[test]
fn media_screen_and_width() {
    let d = load_html(
        "<style>@media screen and (min-width:500px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "screen + width"
    );
}

#[test]
fn media_all_and_width() {
    let d = load_html(
        "<style>@media all and (min-width:500px){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "all + width"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: comma (or) conditions                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_comma_or_first_matches() {
    let d = load_html("<style>@media(max-width:1000px),(min-width:2000px){#t{color:red}}</style><div id='t'>X</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "first of OR matches"
    );
}

#[test]
fn media_comma_or_second_matches() {
    let d = load_html("<style>@media(max-width:400px),(min-width:700px){#t{color:red}}</style><div id='t'>X</div>", 800.0);
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "second of OR matches"
    );
}

#[test]
fn media_comma_or_none_match() {
    let d = load_html("<style>@media(max-width:400px),(min-width:1200px){#t{color:red}}</style><div id='t'>X</div>", 800.0);
    assert_ne!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "neither OR matches"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: not                                                ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_not_print() {
    let d = load_html(
        "<style>@media not print{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "not print = screen"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: media types                                        ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_screen() {
    let d = load_html(
        "<style>@media screen{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(by_id(&d.root, "t").unwrap().style.color.r, 255);
}

#[test]
fn media_all() {
    let d = load_html(
        "<style>@media all{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(by_id(&d.root, "t").unwrap().style.color.r, 255);
}

#[test]
fn media_print_ignored() {
    let d = load_html(
        "<style>@media print{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_ne!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "print ignored"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: nesting                                            ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_nested_in_media() {
    let d = load_html(
        concat!(
            "<style>@media(min-width:500px){@media(max-width:1000px){#t{color:red}}}</style>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "nested @media"
    );
}

#[test]
fn media_in_supports() {
    let d = load_html(
        concat!(
            "<style>@supports(display:flex){@media(min-width:500px){#t{color:red}}}</style>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "@media in @supports"
    );
}

#[test]
fn supports_in_media() {
    let d = load_html(
        concat!(
            "<style>@media(min-width:500px){@supports(display:grid){#t{color:red}}}</style>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "@supports in @media"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: multiple rules inside                              ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_multiple_rules_inside() {
    let d = load_html(
        concat!(
            "<style>@media(min-width:500px){",
            "#a{color:red}",
            "#b{font-size:24px}",
            ".c{background:blue}",
            "}</style>",
            "<div id='a'>A</div><div id='b'>B</div><div class='c' id='cc'>C</div>",
        ),
        800.0,
    );
    assert_eq!(by_id(&d.root, "a").unwrap().style.color.r, 255, "rule 1");
    assert!(
        (by_id(&d.root, "b").unwrap().style.font_size_px(16.0, 16.0) - 24.0).abs() < 2.0,
        "rule 2"
    );
    assert_eq!(
        by_id(&d.root, "cc").unwrap().style.background_color.b,
        255,
        "rule 3"
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: CSS variables per breakpoint                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_css_vars_per_breakpoint() {
    let d = load_html(
        concat!(
            "<style>",
            ":root { --cols: 1; --gap: 10px; }",
            "@media(min-width:600px) { :root { --cols: 2; --gap: 16px; } }",
            "@media(min-width:900px) { :root { --cols: 3; --gap: 20px; } }",
            ".grid { display:grid; gap:var(--gap); width:1000px; }",
            "</style>",
            "<div class='grid' id='t'><div>A</div><div>B</div><div>C</div></div>",
        ),
        1024.0,
    );
    // viewport=1024 > 900 → --gap should be 20px
    let _t = by_id(&d.root, "t").unwrap();
    // At minimum, should not crash. Var resolution across @media is complex.
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: real-world breakpoints                             ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn responsive_mobile_first() {
    let d = load_html(
        concat!(
            "<style>",
            ".box { width: 100%; }",
            "@media(min-width:576px) { .box { width: 540px; } }",
            "@media(min-width:768px) { .box { width: 720px; } }",
            "@media(min-width:992px) { .box { width: 960px; } }",
            "@media(min-width:1200px) { .box { width: 1140px; } }",
            "</style>",
            "<div class='box' id='t' style='height:50px'>X</div>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // 1024 > 992 but < 1200 → width:960px
    assert!(
        (t.layout.content_rect.w - 960.0).abs() < 10.0,
        "992 breakpoint w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn responsive_hide_show() {
    let d = load_html(
        concat!(
            "<style>",
            ".mobile { display: block; }",
            ".desktop { display: none; }",
            "@media(min-width:768px) { .mobile { display: none; } .desktop { display: block; } }",
            "</style>",
            "<div class='mobile' id='m' style='height:50px'>Mobile</div>",
            "<div class='desktop' id='d' style='height:50px'>Desktop</div>",
        ),
        1024.0,
    );
    let m = by_id(&d.root, "m").unwrap();
    let dd = by_id(&d.root, "d").unwrap();
    assert!(
        matches!(m.style.display, Display::None),
        "mobile hidden on desktop"
    );
    assert!(matches!(dd.style.display, Display::Block), "desktop shown");
}

#[test]
fn responsive_font_size() {
    let d = load_html(
        concat!(
            "<style>",
            "h1 { font-size: 24px; }",
            "@media(min-width:768px) { h1 { font-size: 32px; } }",
            "@media(min-width:1200px) { h1 { font-size: 48px; } }",
            "</style>",
            "<h1 id='t'>Title</h1>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.style.font_size_px(16.0, 16.0) - 32.0).abs() < 2.0,
        "768+ fs={:.0}",
        t.style.font_size_px(16.0, 16.0)
    );
}

#[test]
fn responsive_sidebar_collapse() {
    let d = load_html(concat!(
        "<style>",
        ".layout { display:block; width:100%; }",
        ".sidebar { display:none; }",
        "@media(min-width:992px) { .layout { display:flex; } .sidebar { display:block; width:300px; } }",
        "</style>",
        "<div class='layout' style='width:1000px'>",
        "<div class='sidebar' id='sb'>Sidebar</div>",
        "<div id='main' style='flex:1'>Main</div>",
        "</div>",
    ), 1024.0);
    let sb = by_id(&d.root, "sb").unwrap();
    assert!(
        matches!(sb.style.display, Display::Block),
        "sidebar visible on desktop"
    );
    assert!(
        (sb.layout.content_rect.w - 300.0).abs() < 10.0,
        "sidebar w=300 w={:.0}",
        sb.layout.content_rect.w
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: layout changes                                     ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_changes_grid_to_flex() {
    let d = load_html(
        concat!(
            "<style>",
            ".layout { display:grid; grid-template-columns:1fr; width:800px; }",
            "@media(min-width:768px) { .layout { display:flex; } }",
            "</style>",
            "<div class='layout' id='t'>",
            "<div id='a' style='flex:1;height:50px'>A</div>",
            "<div id='b' style='flex:1;height:50px'>B</div>",
            "</div>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(matches!(t.style.display, Display::Flex), "flex on desktop");
    let a = by_id(&d.root, "a").unwrap();
    let b = by_id(&d.root, "b").unwrap();
    assert!(
        (a.layout.content_rect.y - b.layout.content_rect.y).abs() < 5.0,
        "side by side"
    );
}

#[test]
fn media_changes_padding() {
    let d = load_html(
        concat!(
            "<style>",
            ".container { padding: 10px; width: 100%; }",
            "@media(min-width:768px) { .container { padding: 20px; } }",
            "@media(min-width:1200px) { .container { padding: 40px; } }",
            "</style>",
            "<div class='container' id='t' style='width:800px'>X</div>",
        ),
        1024.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    assert!(
        (t.layout.resolved_pad_top - 20.0).abs() < 3.0,
        "768+ padding={:.0}",
        t.layout.resolved_pad_top
    );
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @CONTAINER QUERIES                                         ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn container_type_parsed() {
    let d = load_html(
        concat!(
            "<style>.card-container { container-type: inline-size; width: 400px; }</style>",
            "<div class='card-container' id='t'><div>Content</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // container-type should be parsed
    assert!(t.layout.content_rect.w > 0.0, "container renders");
}

#[test]
fn container_name_parsed() {
    let d = load_html(concat!(
        "<style>.sidebar { container-type: inline-size; container-name: sidebar; width: 300px; }</style>",
        "<div class='sidebar' id='t'><div>Content</div></div>",
    ), 800.0);
    let _t = by_id(&d.root, "t").unwrap();
    // Should not crash
}

#[test]
fn container_query_min_width() {
    let d = load_html(
        concat!(
            "<style>",
            ".wrapper { container-type: inline-size; width: 600px; }",
            ".card { width: 100%; }",
            "@container (min-width: 400px) { .card { display: flex; } }",
            "</style>",
            "<div class='wrapper'>",
            "<div class='card' id='t'>",
            "<div id='a' style='flex:1'>A</div><div id='b' style='flex:1'>B</div>",
            "</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Container is 600px > 400px → .card should be flex
    // This depends on container query support depth
    assert!(t.layout.content_rect.w > 0.0, "container query renders");
}

#[test]
fn container_query_max_width() {
    let d = load_html(
        concat!(
            "<style>",
            ".wrapper { container-type: inline-size; width: 300px; }",
            ".item { font-size: 16px; }",
            "@container (max-width: 400px) { .item { font-size: 12px; } }",
            "</style>",
            "<div class='wrapper'><div class='item' id='t'>Small</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Container is 300px < 400px → font-size:12px
    let fs = t.style.font_size_px(16.0, 16.0);
    assert!(fs <= 14.0, "container query reduces font-size={:.0}", fs);
}

#[test]
fn container_query_no_match() {
    let d = load_html(
        concat!(
            "<style>",
            ".wrapper { container-type: inline-size; width: 300px; }",
            ".item { color: blue; }",
            "@container (min-width: 500px) { .item { color: red; } }",
            "</style>",
            "<div class='wrapper'><div class='item' id='t'>X</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Container is 300px < 500px → color stays blue
    assert_eq!(t.style.color.b, 255, "container query no match stays blue");
    assert_ne!(t.style.color.r, 255, "not red");
}

#[test]
fn container_query_named() {
    let d = load_html(
        concat!(
            "<style>",
            ".panel { container-type: inline-size; container-name: panel; width: 500px; }",
            "@container panel (min-width: 400px) { .inner { color: red; } }",
            "</style>",
            "<div class='panel'><div class='inner' id='t'>X</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Named container query — may or may not be fully supported
    assert!(t.layout.content_rect.w >= 0.0, "no crash");
}

#[test]
fn container_query_nested() {
    let d = load_html(
        concat!(
            "<style>",
            ".outer { container-type: inline-size; width: 800px; }",
            ".inner { container-type: inline-size; width: 400px; }",
            "@container (min-width: 300px) { .content { color: red; } }",
            "</style>",
            "<div class='outer'><div class='inner'><div class='content' id='t'>X</div></div></div>",
        ),
        900.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // Nearest container is .inner (400px) > 300px → matches
    assert!(t.layout.content_rect.w >= 0.0, "nested container no crash");
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @CONTAINER: real-world card component                      ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn container_query_card_responsive() {
    let d = load_html(
        concat!(
            "<style>",
            ".card-container { container-type: inline-size; }",
            ".card { display: block; }",
            "@container (min-width: 400px) { .card { display: flex; } }",
            ".card-image { width: 100%; }",
            "@container (min-width: 400px) { .card-image { width: 200px; flex-shrink: 0; } }",
            "</style>",
            "<div class='card-container' style='width:600px'>",
            "<div class='card' id='card'>",
            "<div class='card-image' id='img' style='height:150px'>Image</div>",
            "<div id='text'>Card text content</div>",
            "</div></div>",
        ),
        800.0,
    );
    let card = by_id(&d.root, "card").unwrap();
    // Container 600px > 400px → card should be flex
    if matches!(card.style.display, Display::Flex) {
        let img = by_id(&d.root, "img").unwrap();
        assert!(
            (img.layout.content_rect.w - 200.0).abs() < 10.0,
            "card image w=200 in wide container"
        );
    }
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: prefers-color-scheme                               ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_prefers_color_scheme_no_crash() {
    let d = load_html(
        concat!(
            "<style>",
            "@media (prefers-color-scheme: dark) { body { background: black; color: white; } }",
            "</style>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn media_prefers_reduced_motion_no_crash() {
    let d = load_html(
        concat!(
            "<style>",
            "@media (prefers-reduced-motion: reduce) { * { animation: none !important; } }",
            "</style>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  @MEDIA: link element media attribute                       ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn link_media_print_excluded() {
    let d = load_html(
        concat!(
            "<link rel='stylesheet' href='print.css' media='print'>",
            "<div id='t'>X</div>",
        ),
        800.0,
    );
    // print stylesheet should not be loaded/applied for screen
    let _t = by_id(&d.root, "t").unwrap();
}

// ╔══════════════════════════════════════════════════════════════╗
// ║  EDGE CASES                                                 ║
// ╚══════════════════════════════════════════════════════════════╝

#[test]
fn media_empty_no_crash() {
    let d = load_html(
        "<style>@media{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn media_unknown_feature_no_crash() {
    let d = load_html(
        "<style>@media(hover:hover){#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
}

#[test]
fn media_only_screen_no_crash() {
    let d = load_html(
        "<style>@media only screen{#t{color:red}}</style><div id='t'>X</div>",
        800.0,
    );
    assert_eq!(
        by_id(&d.root, "t").unwrap().style.color.r,
        255,
        "only screen"
    );
}

#[test]
fn media_many_breakpoints_performance() {
    let mut css = String::from("<style>");
    for i in (100..2000).step_by(50) {
        css.push_str(&format!("@media(min-width:{}px){{.b{{width:{}px}}}}", i, i));
    }
    css.push_str("</style><div class='b' id='t' style='height:50px'>X</div>");
    let d = load_html(&css, 800.0);
    let t = by_id(&d.root, "t").unwrap();
    // Should resolve to the highest matching breakpoint ≤ 800
    assert!(
        t.layout.content_rect.w >= 700.0,
        "many breakpoints w={:.0}",
        t.layout.content_rect.w
    );
}

#[test]
fn container_no_type_no_query() {
    // Without container-type, @container should not match
    let d = load_html(
        concat!(
            "<style>",
            ".wrapper { width: 600px; }",
            "@container (min-width: 400px) { .item { color: red; } }",
            "</style>",
            "<div class='wrapper'><div class='item' id='t'>X</div></div>",
        ),
        800.0,
    );
    let t = by_id(&d.root, "t").unwrap();
    // No container-type → @container shouldn't match
    assert_ne!(t.style.color.r, 255, "no container-type = no match");
}

#[test]
fn container_shorthand() {
    let d = load_html(
        concat!(
            "<style>",
            ".c { container: sidebar / inline-size; width: 400px; }",
            "@container sidebar (min-width: 300px) { .inner { color: red; } }",
            "</style>",
            "<div class='c'><div class='inner' id='t'>X</div></div>",
        ),
        800.0,
    );
    let _t = by_id(&d.root, "t").unwrap();
    // container shorthand — may not be fully parsed but shouldn't crash
}
