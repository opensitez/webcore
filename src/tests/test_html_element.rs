//! `HTMLElement` — `inert` and the reflected content attributes (HTML §3.2.6,
//! §6.7).
//!
//! Five attributes and FOUR different default rules, which is the whole point
//! of the table below: a single shared `reflect_enumerated` helper gets at
//! least two of them wrong, and every one of them has a boolean-ish answer, so
//! a suite that only checks the explicit value passes with the defaults
//! hardcoded backwards. Each row therefore carries the absent case beside the
//! explicit one. All measured (`/tmp/webcore-html/pv.html`, `pv2.html`).

use crate::html::parse_html;
use crate::types::Document;

const PAGE: &str = r#"<div id=root>
<div id=plain></div>
<a id=ahref href="x"></a><a id=abare></a>
<area id=arhref href="x"><area id=arbare>
<img id=img src="x">
<div id=dtrue draggable=true></div><div id=dfalse draggable=false></div><div id=dbogus draggable=bogus></div>
<div id=sctrue spellcheck=true></div>
<div id=scfalse spellcheck=false><span id=scchild></span></div>
<div id=scbogus spellcheck=bogus></div>
<div id=trno translate=no><span id=trchild></span></div>
<div id=tryes translate=yes></div><div id=trbogus translate=bogus></div>
<div id=acwords autocapitalize=words></div>
<div id=acupper autocapitalize=WORDS></div>
<div id=acbogus autocapitalize=BOGUS></div>
<div id=ak accesskey=k></div>
<div id=inertbox inert><div id=inertmid><button id=inertbtn></button></div></div>
<div id=live><button id=livebtn></button></div>
</div>"#;

fn page() -> Document {
    parse_html(PAGE)
}
fn el(d: &Document, id: &str) -> u32 {
    d.get_element_by_id(id).unwrap()
}

// ─── draggable ──────────────────────────────────────────────────────────────

#[test]
fn draggable_defaults_per_element_and_an_invalid_value_falls_back_to_that() {
    let d = page();
    // (id, expected) — the explicit values and every default beside them.
    let cases: &[(&str, bool)] = &[
        ("dtrue", true),
        ("dfalse", false),
        ("dbogus", false), // invalid → `auto` → the <div> default
        ("plain", false),
        ("ahref", true),  // ⛔ an <a> WITH an href
        ("abare", false), // and without one
        ("arhref", true),
        ("arbare", false),
        ("img", true),
    ];
    for (id, want) in cases {
        assert_eq!(d.draggable(el(&d, id)), *want, "{id}");
    }
}

#[test]
fn the_draggable_setter_writes_a_keyword_rather_than_removing_the_attribute() {
    let mut d = page();
    let e = el(&d, "plain");
    d.set_draggable(e, false);
    assert_eq!(
        d.get_attribute(e, "draggable").as_deref(),
        Some("false"),
        "not removed"
    );
    assert!(!d.draggable(e));
    d.set_draggable(e, true);
    assert_eq!(d.get_attribute(e, "draggable").as_deref(), Some("true"));
}

// ─── spellcheck and translate: inherited, opposite vocabularies ─────────────

#[test]
fn spellcheck_is_inherited_and_defaults_to_true() {
    let d = page();
    let cases: &[(&str, bool)] = &[
        ("sctrue", true),
        ("scfalse", false),
        ("scbogus", true),  // invalid → keep asking upwards → the default
        ("plain", true),    // absent → true
        ("scchild", false), // ⛔ inherited from the parent's `false`
    ];
    for (id, want) in cases {
        assert_eq!(d.spellcheck(el(&d, id)), *want, "{id}");
    }
}

#[test]
fn translate_is_inherited_and_defaults_to_true() {
    let d = page();
    let cases: &[(&str, bool)] = &[
        ("tryes", true),
        ("trno", false),
        ("trbogus", true),
        ("plain", true),
        ("trchild", false), // inherited
    ];
    for (id, want) in cases {
        assert_eq!(d.translate(el(&d, id)), *want, "{id}");
    }
}

#[test]
fn the_two_inherited_setters_write_different_vocabularies() {
    // ⛔ `translate` writes yes/no where `spellcheck` writes true/false. One
    // shared helper gets this wrong and the getters still agree, so only the
    // ATTRIBUTE value shows it.
    let mut d = page();
    let e = el(&d, "plain");
    d.set_spellcheck(e, false);
    assert_eq!(d.get_attribute(e, "spellcheck").as_deref(), Some("false"));
    d.set_translate(e, false);
    assert_eq!(d.get_attribute(e, "translate").as_deref(), Some("no"));
    d.set_translate(e, true);
    assert_eq!(d.get_attribute(e, "translate").as_deref(), Some("yes"));
    assert!(!d.spellcheck(e));
    assert!(d.translate(e));
}

// ─── autocapitalize: the one where absent ≠ invalid ─────────────────────────

#[test]
fn autocapitalize_is_empty_when_absent_and_sentences_when_invalid() {
    // ⛔ The rule that breaks the pattern. For draggable, spellcheck and
    // translate an unrecognised value behaves as if absent; here they are two
    // different answers.
    let d = page();
    assert_eq!(d.autocapitalize(el(&d, "plain")), "", "absent");
    assert_eq!(d.autocapitalize(el(&d, "acbogus")), "sentences", "invalid");
    assert_eq!(d.autocapitalize(el(&d, "acwords")), "words");
    assert_eq!(
        d.autocapitalize(el(&d, "acupper")),
        "words",
        "case-insensitive"
    );
}

// ─── accessKey ──────────────────────────────────────────────────────────────

#[test]
fn access_key_reflects_verbatim_and_its_label_is_empty() {
    let mut d = page();
    assert_eq!(d.access_key(el(&d, "ak")), "k");
    assert_eq!(d.access_key(el(&d, "plain")), "");
    let e = el(&d, "plain");
    d.set_access_key(e, "z");
    assert_eq!(d.get_attribute(e, "accesskey").as_deref(), Some("z"));
    // Spec-derived, not measured: this Chrome build does not expose the
    // property at all, and nothing here assigns a key combination.
    assert_eq!(d.access_key_label(e), "");
}

// ─── inert ──────────────────────────────────────────────────────────────────

#[test]
fn the_inert_property_is_not_inherited_even_though_its_effect_is() {
    // ⛔ The distinction the whole feature turns on. Chrome: inside
    // `<div inert>` a button answers `false` for `.inert` — and still cannot
    // be focused.
    let d = page();
    assert!(d.inert(el(&d, "inertbox")));
    assert!(
        !d.inert(el(&d, "inertmid")),
        "the IDL reflects the OWN attribute"
    );
    assert!(!d.inert(el(&d, "inertbtn")));

    assert!(d.is_inert(el(&d, "inertbox")));
    assert!(
        d.is_inert(el(&d, "inertmid")),
        "but the EFFECT reaches every descendant"
    );
    assert!(d.is_inert(el(&d, "inertbtn")));
    assert!(!d.is_inert(el(&d, "livebtn")));
}

#[test]
fn setting_inert_false_removes_the_attribute() {
    let mut d = page();
    let e = el(&d, "inertbox");
    d.set_inert(e, false);
    assert!(!d.has_attribute(e, "inert"));
    assert!(!d.inert(e));
    d.set_inert(e, true);
    assert_eq!(
        d.get_attribute(e, "inert").as_deref(),
        Some(""),
        "a boolean attribute"
    );
}

#[test]
fn focus_refuses_an_inert_subtree_and_leaves_the_focus_where_it_was() {
    // ⛔ Assert the UNCHANGED value, not just that the inert node missed out:
    // Chrome leaves `activeElement` on the body rather than moving focus to
    // the nearest focusable ancestor.
    let mut d = page();
    let live = el(&d, "livebtn");
    let buried = el(&d, "inertbtn");
    d.focus(live);
    assert_eq!(d.focused_box, live);
    d.focus(buried);
    assert_eq!(d.focused_box, live, "focus did not move at all");

    // And it becomes focusable the moment the ancestor stops being inert.
    let box_ = el(&d, "inertbox");
    d.set_inert(box_, false);
    d.focus(buried);
    assert_eq!(d.focused_box, buried);
}

#[test]
fn an_inert_subtree_takes_no_hits() {
    // The other half of the effect, and the one that would silently not work:
    // the walk is top-down, so a subtree it never enters is one no descendant
    // of can be hit.
    use crate::layout::hit_test::hit_test_box_at;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=live style='width:100px;height:50px'>a</div>\
         <div id=dead style='width:100px;height:50px'>b</div>",
        400.0,
    );
    let live = doc.get_element_by_id("live").unwrap();
    let dead = doc.get_element_by_id("dead").unwrap();
    // Hit the CENTRE of each measured box rather than a guessed coordinate.
    let centre = |doc: &crate::types::Document, id: u32| {
        let r = doc.get_bounding_client_rect(id).expect("a laid-out box");
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    };
    let live_pt = centre(&doc, live);
    let dead_pt = centre(&doc, dead);
    assert_ne!(live_pt, dead_pt, "the two boxes must not overlap");
    assert_eq!(hit_test_box_at(&doc.root, live_pt, 0), live);
    assert_eq!(
        hit_test_box_at(&doc.root, dead_pt, 0),
        dead,
        "until it goes inert"
    );

    doc.set_inert(dead, true);
    assert_eq!(
        hit_test_box_at(&doc.root, live_pt, 0),
        live,
        "its sibling is unaffected"
    );
    assert_ne!(
        hit_test_box_at(&doc.root, dead_pt, 0),
        dead,
        "an inert box takes no hits"
    );
}

#[test]
fn an_inert_subtree_yields_no_link_either() {
    // The OTHER tree walker. `hit_test_impl` backs `hit_test_link`, and
    // mutations showed the two are reached by different tests — a guard on one
    // is invisible to a suite that only drives the other.
    //
    // The point comes from the BLOCK's rect: an inline `<a>` has a 0x0
    // border rect here, so its centre is degenerate and hits nothing.
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=box><a id=lnk href='http://example.com/x'>click me</a></div>",
        400.0,
    );
    let box_ = doc.get_element_by_id("box").unwrap();
    let rect = doc
        .get_bounding_client_rect(box_)
        .expect("a laid-out block");
    let pt = (rect.x + 4.0, rect.y + rect.h / 2.0);
    assert_eq!(
        hit_test_link(&doc.root, pt, 0).as_deref(),
        Some("http://example.com/x"),
        "the link is reachable before the box goes inert"
    );

    doc.set_inert(box_, true);
    assert_eq!(hit_test_link(&doc.root, pt, 0), None, "and not after");
}

#[test]
fn a_walk_rooted_at_an_inert_node_finds_nothing_in_it() {
    // The function-level guard, as opposed to the per-child one: a caller can
    // hand a walker a subtree root directly, and an inert root is inert.
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=box><a id=lnk href='http://example.com/x'>click me</a></div>",
        400.0,
    );
    let box_ = doc.get_element_by_id("box").unwrap();
    let rect = doc
        .get_bounding_client_rect(box_)
        .expect("a laid-out block");
    let pt = (rect.x + 4.0, rect.y + rect.h / 2.0);

    fn find<'a>(node: &'a crate::types::WebCore, id: u32) -> Option<&'a crate::types::WebCore> {
        if node.node_id == id {
            return Some(node);
        }
        node.children.iter().find_map(|c| find(c, id))
    }
    let subtree = find(&doc.root, box_)
        .expect("the box in the render tree")
        .clone();
    assert_eq!(
        hit_test_link(&subtree, pt, 0).as_deref(),
        Some("http://example.com/x"),
        "reachable when rooted at the box"
    );

    doc.set_inert(box_, true);
    let subtree = find(&doc.root, box_).expect("the box in the render tree");
    assert_eq!(
        hit_test_link(subtree, pt, 0),
        None,
        "rooted AT the inert node"
    );
}

#[test]
fn an_inert_positioned_overlay_takes_no_hits_either() {
    // The z-index pass is a THIRD road through the same walker: positioned
    // children with `z-index > 0` are collected and tried before everything
    // else, so an overlay needs its own guard. A mutation found this one — no
    // other fixture here has a positioned inert child.
    use crate::layout::hit_test::hit_test_box_at;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=under style='width:200px;height:100px'>under</div>\
         <div id=over style='position:absolute;left:0;top:0;width:200px;height:100px;z-index:99'>over</div>",
        400.0,
    );
    let over = doc.get_element_by_id("over").unwrap();
    let under = doc.get_element_by_id("under").unwrap();
    let r = doc
        .get_bounding_client_rect(over)
        .expect("a laid-out overlay");
    let pt = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(
        hit_test_box_at(&doc.root, pt, 0),
        over,
        "the overlay wins on top"
    );

    doc.set_inert(over, true);
    let hit = hit_test_box_at(&doc.root, pt, 0);
    assert_ne!(hit, over, "an inert overlay takes no hits");
    assert_eq!(hit, under, "and the box beneath it gets them instead");
}

#[test]
fn an_inert_positioned_overlay_is_skipped_by_the_z_index_pass_too() {
    // ⛔ The z-index collector lives in `hit_test_impl`, and the overlay test
    // above drives `deepest_box_at` — a different walker with no z-index pass.
    // A mutation showed the guard stayed green through that test, so this one
    // reaches it the only way that exists: through `hit_test_link`.
    //
    // The guard is load-bearing because the collector's caller falls back to
    // `return Some(child.node_id)` when the recursion refuses.
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<a id=under href='http://example.com/under' style='display:block;width:200px;height:100px'>under</a>\
         <a id=over href='http://example.com/over' \
            style='position:absolute;left:0;top:0;width:200px;height:100px;z-index:99'>over</a>",
        400.0,
    );
    let over = doc.get_element_by_id("over").unwrap();
    let r = doc
        .get_bounding_client_rect(over)
        .expect("a laid-out overlay");
    let pt = (r.x + 4.0, r.y + r.h / 2.0);
    assert_eq!(
        hit_test_link(&doc.root, pt, 0).as_deref(),
        Some("http://example.com/over"),
        "the overlay wins on top"
    );

    doc.set_inert(over, true);
    assert_ne!(
        hit_test_link(&doc.root, pt, 0).as_deref(),
        Some("http://example.com/over"),
        "an inert overlay is skipped by the z-index pass"
    );
}

#[test]
fn transformed_z_index_dropdown_hits_where_it_is_painted() {
    use crate::layout::hit_test::{hit_test_box_at, hit_test_link};
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
          * { margin: 0; padding: 0; }
          nav { position: relative; height: 40px; }
          .item { position: relative; width: 100px; height: 40px; }
          .panel {
            position: absolute;
            left: 50%;
            top: 40px;
            width: 200px;
            height: 100px;
            transform: translateX(-50%);
            z-index: 1000;
            background: white;
          }
          .panel a, .hero a { display: block; width: 100%; height: 100%; }
          .hero { height: 240px; background: #ddd; }
        </style>
        <nav><div class="item">Products<div id="panel" class="panel">
          <a id="menu-link" href="http://example.com/menu">Firefox</a>
        </div></div></nav>
        <div class="hero"><a id="hero-link" href="http://example.com/hero">Hero</a></div>
        "#,
        800.0,
    );
    let menu = doc.get_element_by_id("menu-link").unwrap();
    let pt = (20.0, 64.0);
    assert_eq!(
        hit_test_link(&doc.root, pt, 0).as_deref(),
        Some("http://example.com/menu"),
        "the painted, transformed dropdown link should beat the hero underneath"
    );
    assert_eq!(
        hit_test_box_at(&doc.root, pt, 0),
        menu,
        "debug hit testing should report the dropdown link too"
    );
}

#[test]
fn abs_descendant_top_percent_uses_final_positioned_ancestor_height() {
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
          * { box-sizing: border-box; margin: 0; padding: 0; }
          nav {
            position: relative;
            padding: 8px 0 0;
            width: 800px;
            background: white;
          }
          ul {
            display: flex;
            height: 39px;
            list-style: none;
          }
          li { position: static; padding: 0 24px 16px; }
          .panel {
            position: absolute;
            left: 50%;
            top: 100%;
            width: 420px;
            height: 120px;
            transform: translateX(-50%);
            z-index: 1000;
            background: white;
          }
          .panel a { display: block; height: 100%; }
          .hero { height: 400px; background: #111; }
          .hero a { display: block; height: 100%; }
        </style>
        <nav>
          <ul>
            <li>
              Products
              <div id="panel" class="panel">
                <a href="http://example.com/menu">Firefox</a>
              </div>
            </li>
          </ul>
        </nav>
        <section class="hero"><a href="http://example.com/hero">Hero</a></section>
        "#,
        800.0,
    );

    let panel = doc.get_element_by_id("panel").unwrap();
    let panel_rect = doc
        .get_bounding_client_rect(panel)
        .expect("panel should be laid out");
    assert!(
        panel_rect.y < 80.0,
        "top:100% should resolve against the nav's final height, not the viewport: {panel_rect:?}"
    );
    assert_eq!(
        hit_test_link(&doc.root, (400.0, panel_rect.y + 16.0), 0).as_deref(),
        Some("http://example.com/menu"),
        "the dropdown link should receive hits at the corrected painted position"
    );
}

#[test]
fn z_indexed_descendant_outside_ancestor_bounds_receives_hits() {
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"
        <style>
          * { margin: 0; padding: 0; }
          #holder { position: relative; width: 40px; height: 40px; }
          #floating {
            display: block;
            position: absolute;
            left: 200px;
            top: 60px;
            width: 120px;
            height: 60px;
            z-index: 10;
          }
          #under {
            display: block;
            position: absolute;
            left: 0;
            top: 40px;
            width: 400px;
            height: 200px;
          }
        </style>
        <div id="holder"><a id="floating" href="http://example.com/floating">Floating</a></div>
        <a id="under" href="http://example.com/under">Under</a>
        "#,
        800.0,
    );
    assert_eq!(
        hit_test_link(&doc.root, (220.0, 80.0), 0).as_deref(),
        Some("http://example.com/floating"),
        "a deferred z descendant should be hittable outside its ancestor border"
    );
}

#[test]
fn pointer_events_none_overlay_passes_hits_through() {
    use crate::layout::hit_test::hit_test_link;
    let mut renderer = crate::Renderer::new();
    let under = "<a id=under href='http://example.com/under' \
                    style='display:block;position:absolute;left:0;top:0;width:200px;height:100px'>under</a>";
    let over_auto = "<a id=over href='http://example.com/over' \
                       style='display:block;position:absolute;left:0;top:0;width:200px;height:100px;z-index:5'>over</a>";
    let over_none = "<a id=over href='http://example.com/over' \
                       style='display:block;position:absolute;left:0;top:0;width:200px;height:100px;z-index:5;pointer-events:none'>over</a>";

    let doc = renderer.load_html(&format!("{under}{over_auto}"), 400.0);
    assert_eq!(
        hit_test_link(&doc.root, (20.0, 20.0), 0).as_deref(),
        Some("http://example.com/over"),
        "a normal overlay captures the hit"
    );

    let doc = renderer.load_html(&format!("{under}{over_none}"), 400.0);
    assert_eq!(
        hit_test_link(&doc.root, (20.0, 20.0), 0).as_deref(),
        Some("http://example.com/under"),
        "pointer-events:none lets the lower link receive the hit"
    );
}
