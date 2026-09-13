use crate::layout::LayoutEngine;
use crate::types::*;
use crate::{Document, parse_html};

fn layout_html(html: &str, width: f32) -> Document {
    let mut doc = parse_html(html);
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, width);
    doc
}

fn find_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_id(child, id) {
            return Some(n);
        }
    }
    None
}

/// Simulate hovering over element with given id: set hovered_box, mark changed, re-layout.
fn find_by_class<'a>(node: &'a WebCore, class: &str) -> Option<&'a WebCore> {
    if node
        .attributes
        .get("class")
        .map_or(false, |c| c.split_whitespace().any(|w| w == class))
    {
        return Some(node);
    }
    for ch in &node.children {
        if let Some(f) = find_by_class(ch, class) {
            return Some(f);
        }
    }
    None
}

fn recascade_with_hover(doc: &mut Document, hovered_id: &str) {
    let hovered_id_val = find_by_id(&doc.root, hovered_id)
        .map(|n| n.node_id)
        .unwrap_or(0);
    doc.hovered_box = hovered_id_val;
    doc.hover_changed = true;
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    // Force cascade by setting last_cascade_vw to NaN (new engine)
    eng.layout(doc, 800.0);
}

// ── Basic self-hover ──────────────────────────────────────────────────────

#[test]
fn hover_self_changes_color() {
    let mut doc = layout_html(
        r#"
        <style>
            #btn { color: black; }
            #btn:hover { color: red; }
        </style>
        <div id="btn">Click me</div>
    "#,
        800.0,
    );

    // Before hover: color should be black
    let btn = find_by_id(&doc.root, "btn").unwrap();
    assert_eq!(btn.style.color, Color::rgb(0, 0, 0), "before hover: black");

    // After hovering #btn
    recascade_with_hover(&mut doc, "btn");
    let btn = find_by_id(&doc.root, "btn").unwrap();
    assert_eq!(btn.style.color, Color::rgb(255, 0, 0), "after hover: red");
}

// ── Parent:hover child — the CSS dropdown pattern ────────────────────────

#[test]
fn parent_hover_reveals_child_display_block() {
    let mut doc = layout_html(
        r#"
        <style>
            .dropdown { display: none; }
            .menu:hover .dropdown { display: block; }
        </style>
        <div class="menu" id="menu">
            <span>Menu Label</span>
            <div class="dropdown" id="dropdown">
                <p>Item 1</p>
                <p>Item 2</p>
            </div>
        </div>
    "#,
        800.0,
    );

    // Before hover: dropdown should be display:none (no height)
    let dropdown = find_by_id(&doc.root, "dropdown").unwrap();
    assert_eq!(
        dropdown.style.display,
        Display::None,
        "before hover: dropdown should be display:none"
    );

    // Hover on menu (parent) — dropdown should become display:block
    recascade_with_hover(&mut doc, "menu");
    let dropdown = find_by_id(&doc.root, "dropdown").unwrap();
    assert_ne!(
        dropdown.style.display,
        Display::None,
        "after hovering parent: dropdown should be display:block"
    );
}

#[test]
fn parent_hover_child_max_height() {
    let mut doc = layout_html(
        r#"
        <style>
            .sub { max-height: 0; overflow: hidden; }
            .nav:hover > .sub { max-height: 500px; }
        </style>
        <div class="nav" id="nav">
            <span>Nav Item</span>
            <ul class="sub" id="sub">
                <li>Sub 1</li>
                <li>Sub 2</li>
            </ul>
        </div>
    "#,
        800.0,
    );

    // Before hover: sub should be constrained to 0 height
    let sub = find_by_id(&doc.root, "sub").unwrap();
    assert!(
        sub.layout.content_rect.h < 1.0,
        "before hover: sub should have ~0 height, got {}",
        sub.layout.content_rect.h
    );

    // Hover on nav (parent) — sub should expand
    recascade_with_hover(&mut doc, "nav");
    let sub = find_by_id(&doc.root, "sub").unwrap();
    assert!(
        sub.layout.content_rect.h > 10.0,
        "after hovering parent: sub should have visible height, got {}",
        sub.layout.content_rect.h
    );
}

// ── Hover on sibling content within parent ──────────────────────────────

#[test]
fn hover_on_sibling_within_parent_activates_child() {
    let mut doc = layout_html(
        r#"
        <style>
            .dropdown { display: none; }
            .menu:hover .dropdown { display: block; }
        </style>
        <div class="menu" id="menu">
            <span id="label">Menu Label</span>
            <div class="dropdown" id="dropdown">Content</div>
        </div>
    "#,
        800.0,
    );

    // Hover on the label (child of .menu, sibling of .dropdown)
    // .menu:hover should match because .menu contains the hovered label
    recascade_with_hover(&mut doc, "label");
    let dropdown = find_by_id(&doc.root, "dropdown").unwrap();
    assert_ne!(
        dropdown.style.display,
        Display::None,
        "hovering sibling label should activate parent:hover child rule"
    );
}

// ── Hover outside parent does NOT activate child ────────────────────────

#[test]
fn hover_outside_parent_does_not_activate_child() {
    let mut doc = layout_html(
        r#"
        <style>
            .dropdown { display: none; }
            .menu:hover .dropdown { display: block; }
        </style>
        <div class="menu" id="menu">
            <span>Menu Label</span>
            <div class="dropdown" id="dropdown">Content</div>
        </div>
        <div id="outside">Other content</div>
    "#,
        800.0,
    );

    // Hover on element outside .menu
    recascade_with_hover(&mut doc, "outside");
    let dropdown = find_by_id(&doc.root, "dropdown").unwrap();
    assert_eq!(
        dropdown.style.display,
        Display::None,
        "hovering outside parent should NOT activate dropdown"
    );
}

// ── Multiple dropdown menus — only hovered one opens ────────────────────

#[test]
fn only_hovered_menu_opens() {
    let mut doc = layout_html(
        r#"
        <style>
            .sub { display: none; }
            .item:hover > .sub { display: block; }
        </style>
        <nav>
            <div class="item" id="item1">
                <span>File</span>
                <div class="sub" id="sub1">New, Open, Save</div>
            </div>
            <div class="item" id="item2">
                <span>Edit</span>
                <div class="sub" id="sub2">Cut, Copy, Paste</div>
            </div>
        </nav>
    "#,
        800.0,
    );

    // Hover on item1
    recascade_with_hover(&mut doc, "item1");
    let sub1 = find_by_id(&doc.root, "sub1").unwrap();
    let sub2 = find_by_id(&doc.root, "sub2").unwrap();
    assert_ne!(
        sub1.style.display,
        Display::None,
        "hovered menu should open"
    );
    assert_eq!(
        sub2.style.display,
        Display::None,
        "other menu should stay closed"
    );
}

// ── Nested hover (grandparent:hover grandchild) ────────────────────────

#[test]
fn grandparent_hover_affects_grandchild() {
    let mut doc = layout_html(
        r#"
        <style>
            .tooltip { visibility: hidden; }
            .container:hover .tooltip { visibility: visible; }
        </style>
        <div class="container" id="container">
            <div class="wrapper">
                <span class="tooltip" id="tip">Tooltip text</span>
            </div>
        </div>
    "#,
        800.0,
    );

    recascade_with_hover(&mut doc, "container");
    let tip = find_by_id(&doc.root, "tip").unwrap();
    assert!(
        tip.style.visibility,
        "grandparent hover should make grandchild visible"
    );
}

#[test]
fn hover_swap_preserves_non_hover_pseudo_styles() {
    let mut doc = layout_html(
        r#"
        <style>
            #item:hover { color: rgb(1, 2, 3); }
            #item::selection { color: rgb(10, 20, 30); }
            #item::marker { color: rgb(40, 50, 60); }
            input::placeholder { color: rgb(70, 80, 90); }
        </style>
        <ul><li id="item">one</li></ul>
        <input id="input" placeholder="hint">
    "#,
        800.0,
    );

    recascade_with_hover(&mut doc, "item");

    let item = find_by_id(&doc.root, "item").unwrap();
    assert_eq!(
        item.style.selection_style.as_ref().map(|style| style.color),
        Some(Color::rgb(10, 20, 30))
    );
    assert_eq!(
        item.style.marker_style.as_ref().map(|style| style.color),
        Some(Color::rgb(40, 50, 60))
    );

    let input = find_by_id(&doc.root, "input").unwrap();
    assert_eq!(
        input
            .style
            .placeholder_style
            .as_ref()
            .map(|style| style.color),
        Some(Color::rgb(70, 80, 90))
    );
}

// ── USPS-style nav dropdown: inline-block li with absolute div ──────────

#[test]
fn usps_nav_dropdown_max_height_on_hover() {
    let mut doc = layout_html(
        r#"
        <style>
            nav ul li { display: inline-block; position: relative; }
            nav ul li a { display: block; padding: 14px 11px; position: relative; }
            nav ul li div { max-height: 0; overflow: hidden; position: absolute; }
            nav ul li:hover div { max-height: 1800px; }
        </style>
        <nav><ul>
            <li id="item">
                <a id="link">Send</a>
                <div id="dropdown"><p>Dropdown content</p></div>
            </li>
        </ul></nav>
    "#,
        800.0,
    );

    // Before hover: dropdown should have 0 height
    let dd = find_by_id(&doc.root, "dropdown").unwrap();
    assert!(
        dd.layout.content_rect.h < 1.0,
        "before hover: dropdown h={}",
        dd.layout.content_rect.h
    );

    // Hover on the <a> link — li:hover should match because li contains the hovered <a>
    recascade_with_hover(&mut doc, "link");
    let dd = find_by_id(&doc.root, "dropdown").unwrap();
    eprintln!(
        "after hover link: dropdown max_height={:?} h={}",
        dd.style.max_height, dd.layout.content_rect.h
    );
    assert!(
        dd.layout.content_rect.h > 5.0,
        "after hover: dropdown should expand, h={}",
        dd.layout.content_rect.h
    );
}

// ── USPS-style: positioned ::before on hover + dropdown + hover-out ──────

#[test]
fn usps_hover_before_and_dropdown_on_link_hover() {
    // Reproduces the real USPS nav: a:hover::before shows gray bg,
    // li:hover div opens the dropdown.
    let mut doc = layout_html(
        r#"
        <style>
            nav ul li { display: inline-block; position: relative; }
            nav ul li a { display: block; padding: 14px 11px; position: relative; color: #333; }
            nav ul li a:hover:before {
                content: "";
                display: block;
                position: absolute;
                top: 0; left: 0;
                width: 100%; height: 100%;
                background: #ededed;
                z-index: -1;
            }
            nav ul li div { max-height: 0; overflow: hidden; position: absolute; background: #ededed; }
            nav ul li:hover div { max-height: 1800px; opacity: 1; }
        </style>
        <nav><ul>
            <li id="item1">
                <a id="link1">Send</a>
                <div id="dd1"><p>Send dropdown</p></div>
            </li>
            <li id="item2">
                <a id="link2">Receive</a>
                <div id="dd2"><p>Receive dropdown</p></div>
            </li>
        </ul></nav>
    "#,
        800.0,
    );

    // Before hover: both dropdowns collapsed
    let dd1 = find_by_id(&doc.root, "dd1").unwrap();
    let dd2 = find_by_id(&doc.root, "dd2").unwrap();
    assert!(
        dd1.layout.content_rect.h < 1.0,
        "dd1 should be collapsed before hover"
    );
    assert!(
        dd2.layout.content_rect.h < 1.0,
        "dd2 should be collapsed before hover"
    );

    // Hover on "Send" link — should open dd1 via li:hover div
    recascade_with_hover(&mut doc, "link1");
    let dd1 = find_by_id(&doc.root, "dd1").unwrap();
    let dd2 = find_by_id(&doc.root, "dd2").unwrap();
    eprintln!(
        "[test] after hover link1: dd1 max_height={:?} h={}, dd2 h={}",
        dd1.style.max_height, dd1.layout.content_rect.h, dd2.layout.content_rect.h
    );
    assert!(
        dd1.layout.content_rect.h > 5.0,
        "dd1 should expand when hovering link inside li, h={}",
        dd1.layout.content_rect.h
    );
    assert!(dd2.layout.content_rect.h < 1.0, "dd2 should stay collapsed");

    // Check that a::before is active (the link should have before_style)
    let link1 = find_by_id(&doc.root, "link1").unwrap();
    assert!(
        link1.style.before_style.is_some(),
        "link1 should have ::before style on hover"
    );

    // Now hover "Receive" link — dd1 should close, dd2 should open
    recascade_with_hover(&mut doc, "link2");
    let dd1 = find_by_id(&doc.root, "dd1").unwrap();
    let dd2 = find_by_id(&doc.root, "dd2").unwrap();
    assert!(
        dd1.layout.content_rect.h < 1.0,
        "dd1 should collapse after hover-out, h={}",
        dd1.layout.content_rect.h
    );
    assert!(
        dd2.layout.content_rect.h > 5.0,
        "dd2 should expand when hovering link2, h={}",
        dd2.layout.content_rect.h
    );

    // Check that link1's ::before is gone (hover-out cleanup)
    let link1 = find_by_id(&doc.root, "link1").unwrap();
    let has_before_child = link1.children.iter().any(|c| c.tag == "::before");
    let has_before_style = link1.style.before_style.is_some();
    eprintln!(
        "[test] after hover-out link1: before_child={}, before_style={}",
        has_before_child, has_before_style
    );
    // The ::before should be removed or have no content when not hovered
    if has_before_child {
        let before = link1.children.iter().find(|c| c.tag == "::before").unwrap();
        assert!(
            before.text.is_empty() && before.layout.border_rect.h < 1.0,
            "stale ::before should not render after hover-out, text='{}' h={}",
            before.text,
            before.layout.border_rect.h
        );
    }
}

#[test]
fn hover_reverts_on_hover_out() {
    let mut doc = layout_html(
        r#"
        <style>
            #btn { color: black; background: white; }
            #btn:hover { color: red; background: gray; }
        </style>
        <div id="btn">Click me</div>
        <div id="other">Other</div>
    "#,
        800.0,
    );

    // Hover on btn
    recascade_with_hover(&mut doc, "btn");
    let btn = find_by_id(&doc.root, "btn").unwrap();
    assert_eq!(btn.style.color, Color::rgb(255, 0, 0), "hovered: red");

    // Hover on a different element (simulates hover-out of btn)
    recascade_with_hover(&mut doc, "other");
    let btn = find_by_id(&doc.root, "btn").unwrap();
    assert_eq!(
        btn.style.color,
        Color::rgb(0, 0, 0),
        "after hover-out: should revert to black"
    );
}

// ── A pseudo-element exists only when `content` says so ──────────────────────

/// **`content` decides whether a pseudo-element exists** (css-pseudo-4 §2.1):
/// with no `content` declaration it computes to `none` and nothing is generated.
/// The re-cascade that a hover triggers used to generate a `::before` box for
/// every flex or grid element that merely had a `::before` RULE, so moving the
/// pointer anywhere on a page filled its headers with empty flex items and
/// pushed everything below them down.
#[test]
fn hover_recascade_does_not_invent_pseudo_elements() {
    let mut doc = layout_html(
        r#"
        <style>
            * { margin: 0; padding: 0 }
            .bar { display: flex }
            /* Styling only — no `content`, so no pseudo-element. */
            .bar::before { color: red; width: 40px }
            .bar::after  { color: blue; width: 40px }
            #btn:hover { color: green }
        </style>
        <div class="bar"><span id="btn">Menu</span><span>Other</span></div>
    "#,
        800.0,
    );

    let bar_h = find_by_class(&doc.root, "bar")
        .unwrap()
        .layout
        .margin_rect
        .h;
    let pseudo_count = |d: &Document| {
        let bar = find_by_class(&d.root, "bar").unwrap();
        bar.children
            .iter()
            .filter(|c| c.tag == "::before" || c.tag == "::after")
            .count()
    };
    assert_eq!(pseudo_count(&doc), 0, "no pseudo-element before hover");

    recascade_with_hover(&mut doc, "btn");
    assert_eq!(
        pseudo_count(&doc),
        0,
        "a hover must not generate one either"
    );
    assert_eq!(
        find_by_class(&doc.root, "bar")
            .unwrap()
            .layout
            .margin_rect
            .h,
        bar_h,
        "the flex row must not grow on hover"
    );
}

/// `content: ""` is a real, empty pseudo-element — the icon idiom — and both
/// passes must keep it.
#[test]
fn hover_recascade_keeps_an_empty_content_pseudo_element() {
    let mut doc = layout_html(
        r#"
        <style>
            * { margin: 0; padding: 0 }
            .bar { display: flex }
            .bar::before { content: ""; width: 40px; height: 10px }
            #btn:hover { color: green }
        </style>
        <div class="bar"><span id="btn">Menu</span></div>
    "#,
        800.0,
    );
    let count = |d: &Document| {
        find_by_class(&d.root, "bar")
            .unwrap()
            .children
            .iter()
            .filter(|c| c.tag == "::before")
            .count()
    };
    assert_eq!(count(&doc), 1, "content:\"\" generates a ::before box");
    recascade_with_hover(&mut doc, "btn");
    assert_eq!(count(&doc), 1, "and the hover re-cascade keeps exactly one");
}

/// **Blockification is a computed-value transform, not a side effect of the
/// `float` declaration** (CSS Display 3 §2.7). Applying it inside `float`'s
/// own applier made it depend on declaration order, so a later `display`
/// undid it — and the two cascade implementations, which order rules
/// differently, disagreed about the same element.
#[test]
fn float_blockifies_whatever_the_declaration_order() {
    let doc = layout_html(
        r#"
        <style>
            #a { float: left; display: inline }
            #b { display: inline; float: left }
        </style>
        <span id="a">A</span><span id="b">B</span>
    "#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "a").unwrap().style.display,
        Display::Block
    );
    assert_eq!(
        find_by_id(&doc.root, "b").unwrap().style.display,
        Display::Block
    );
}

/// An absolutely positioned box is blockified the same way.
#[test]
fn absolute_position_blockifies_display() {
    let doc = layout_html(
        r#"
        <style>
            #a { position: absolute; display: inline }
            #b { position: fixed; display: inline-block }
            #c { position: absolute; display: inline-flex }
            #d { position: static; display: inline }
        </style>
        <span id="a">A</span><span id="b">B</span><span id="c">C</span><span id="d">D</span>
    "#,
        800.0,
    );
    assert_eq!(
        find_by_id(&doc.root, "a").unwrap().style.display,
        Display::Block
    );
    assert_eq!(
        find_by_id(&doc.root, "b").unwrap().style.display,
        Display::Block
    );
    assert_eq!(
        find_by_id(&doc.root, "c").unwrap().style.display,
        Display::Flex
    );
    assert_eq!(
        find_by_id(&doc.root, "d").unwrap().style.display,
        Display::Inline,
        "an in-flow box keeps its inline display"
    );
}

/// The whole point: the hover re-cascade must agree with the first pass.
#[test]
fn hover_recascade_agrees_about_blockified_display() {
    let mut doc = layout_html(
        r#"
        <style>
            .logo { float: left; max-width: 120px }
            .clip { position: absolute; width: 1px; height: 1px }
            #btn:hover { color: green }
        </style>
        <div><span class="logo">L</span><span class="clip">C</span><span id="btn">B</span></div>
    "#,
        800.0,
    );
    let read = |d: &Document| {
        (
            find_by_class(&d.root, "logo").unwrap().style.display,
            find_by_class(&d.root, "clip").unwrap().style.display,
        )
    };
    let first = read(&doc);
    assert_eq!(first, (Display::Block, Display::Block));
    recascade_with_hover(&mut doc, "btn");
    assert_eq!(
        read(&doc),
        first,
        "the hover pass disagreed with the first pass"
    );
}

/// **A hover anywhere must not collapse an unrelated heading.** On
/// fr.wikipedia, hovering the article text left `#firstHeading` one pixel tall
/// — its text vanished — and a later full relayout restored it, so the fault is
/// in the layout the hover pass produces, not in the styles it computes.
#[test]
fn hover_does_not_collapse_a_flow_root_flex_item() {
    let mut doc = layout_html(
        r#"
        <style>
            * { margin: 0; padding: 0 }
            #content { display: grid }
            .titlebar { display: flex }
            .titlebar::after { content: ""; height: 1px; width: 1px }
            h1 { display: flow-root; font-size: 28.8px }
            #btn:hover { color: green }
            /* A descendant hover rule: this is what sets
               `has_hover_descendant_rules`, and it changes which nodes the
               hover invalidation marks. */
            .menu:hover .panel { display: block }
            .panel { display: none }
        </style>
        <main id="content">
          <header class="titlebar">
            <h1 id="h">Bienvenue sur Wikipédia</h1>
            <div id="lang">Langues</div>
          </header>
          <div class="menu">m<div class="panel">p</div></div>
          <p id="btn">body text</p>
        </main>
    "#,
        1280.0,
    );

    let before = find_by_id(&doc.root, "h").unwrap().layout.margin_rect;
    assert!(
        before.h > 20.0,
        "the heading starts with a real height: {}",
        before.h
    );

    recascade_with_hover(&mut doc, "btn");
    let after = find_by_id(&doc.root, "h").unwrap().layout.margin_rect;
    assert_eq!(
        after.h, before.h,
        "hovering elsewhere collapsed the heading: {}x{} -> {}x{}",
        before.w, before.h, after.w, after.h
    );
}

/// absent from hit testing entirely at the place it is drawn.
#[test]
fn hit_testing_maps_the_point_through_the_transform() {
    let doc = layout_html(
        r#"<style>*{margin:0;padding:0}</style>
        <div id="t" style="position:absolute;left:0;top:0;width:100px;height:100px;
                           background:red;transform:translate(200px, 0)"></div>"#,
        800.0,
    );
    let want = find_by_id(&doc.root, "t").expect("#t must exist").node_id;

    // Positive: the point the box is PAINTED at hits it.
    let hit = crate::layout::hit_test::point_to_hit(&doc.root, (250.0, 50.0), 0)
        .expect("a point inside the transformed box must hit something");
    assert_eq!(
        hit.node_id, want,
        "a box translated 200px right must be hit at x=250, where it is painted"
    );

    // Negative: the box's UN-transformed position is no longer part of it.
    let miss = crate::layout::hit_test::point_to_hit(&doc.root, (50.0, 50.0), 0);
    assert!(
        miss.map_or(true, |h| h.node_id != want),
        "the box's pre-transform position must no longer hit it"
    );
}
