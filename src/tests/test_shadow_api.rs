//! The shadow DOM as an API — `attachShadow`, `shadowRoot`, scoped queries,
//! `:host`, and shadow content actually being laid out.
//!
//! The tree structure existed long before any of this; what was missing was a
//! way to reach it, style the host from inside, or see it on screen.

use crate::parse_html;
use crate::types::ShadowMode;

fn doc(html: &str) -> crate::Document {
    parse_html(html)
}

#[test]
fn attach_shadow_and_read_it_back() {
    let mut d = doc("<div id=a>light</div>");
    let a = d.query_selector("#a").unwrap();
    assert!(!d.has_shadow_root(a));
    assert_eq!(d.shadow_root_of(a), None);

    // `attachShadow` returns the SHADOW ROOT, not the host.
    let sr = d.attach_shadow(a, ShadowMode::Open).expect("attached");
    assert_ne!(sr, a, "the shadow root is its own node");
    assert_eq!(d.node_type(sr), 11, "a ShadowRoot is a DocumentFragment");
    assert!(d.has_shadow_root(a));
    assert_eq!(d.shadow_root_of(a), Some(sr));
    assert_eq!(d.shadow_root_host(sr), Some(a));
    assert_eq!(d.shadow_root_mode(sr), Some(ShadowMode::Open));

    // `attachShadow` on an element that already has one throws
    // NotSupportedError; it never replaces silently.
    assert_eq!(d.attach_shadow(a, ShadowMode::Open), None);
}

#[test]
fn a_closed_root_is_invisible_from_outside() {
    let mut d = doc("<div id=a>light</div>");
    let a = d.query_selector("#a").unwrap();
    d.attach_shadow(a, ShadowMode::Closed);
    // It exists...
    assert!(d.has_shadow_root(a));
    // ...but `element.shadowRoot` does not expose it. That is what closed means.
    assert_eq!(d.shadow_root_of(a), None);
}

#[test]
fn query_selector_does_not_pierce_the_boundary_but_the_scoped_one_does() {
    let d =
        doc("<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>");
    let host = d.query_selector("#host").unwrap();

    // Correct: `document.querySelector` must not see into a shadow tree.
    assert_eq!(d.query_selector("#inner"), None);

    // `shadowRoot.querySelector` is the way in — and without it the tree was
    // unreachable through any API at all.
    let inner = d
        .shadow_query_selector(host, "#inner")
        .expect("the scoped query reaches the shadow tree");
    assert_eq!(d.tag_name(inner), Some("span"));
    assert_eq!(d.shadow_query_selector_all(host, "span").len(), 1);

    // And the scope really is the shadow tree: the host is not in it.
    assert_eq!(d.shadow_query_selector(host, "#host"), None);
}

#[test]
fn shadow_host_walks_back_out() {
    let d =
        doc("<div id=host><template shadowrootmode=open><span id=inner>s</span></template></div>");
    let host = d.query_selector("#host").unwrap();
    let inner = d.shadow_query_selector(host, "#inner").unwrap();
    assert_eq!(d.shadow_host(inner), Some(host));
    // A light-tree node has no shadow host.
    assert_eq!(d.shadow_host(host), None);
}

#[test]
fn shadow_children_lists_the_tree_top_level() {
    let d = doc(
        "<div id=host><template shadowrootmode=open><span id=a>1</span><b id=b>2</b></template></div>",
    );
    let host = d.query_selector("#host").unwrap();
    let kids = d.shadow_children(host);
    assert_eq!(kids.len(), 2);
    assert_eq!(d.tag_name(kids[0]), Some("span"));
    assert_eq!(d.tag_name(kids[1]), Some("b"));
}

#[test]
fn setting_shadow_inner_html_replaces_the_tree() {
    let mut d =
        doc("<div id=host><template shadowrootmode=open><span id=old>1</span></template></div>");
    let host = d.query_selector("#host").unwrap();
    assert!(d.shadow_query_selector(host, "#old").is_some());
    assert!(d.set_shadow_inner_html(host, "<p id=new>2</p>"));
    assert!(d.shadow_query_selector(host, "#old").is_none());
    assert!(d.shadow_query_selector(host, "#new").is_some());
}

// ─── Rendering ──────────────────────────────────────────────────────────────

fn laid_out(html: &str) -> crate::Document {
    let mut d = parse_html(html);
    crate::css::apply_cascade_vp(
        &mut d.root,
        &d.stylesheet,
        None,
        16.0,
        400.0,
        900.0,
        0,
        false,
    );
    let mut eng = crate::LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut d, 400.0);
    d
}

fn find_shadow<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
    if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
        return Some(n);
    }
    if let Some(sr) = &n.shadow_root {
        for c in &sr.children {
            if let Some(f) = find_shadow(c, id) {
                return Some(f);
            }
        }
    }
    n.children.iter().find_map(|c| find_shadow(c, id))
}

#[test]
fn shadow_content_is_laid_out_in_every_formatting_context() {
    // Layout used to SWAP shadow children into `node.children` for the
    // duration, which emptied `shadow_root.children` — and
    // `effective_children()` reads exactly that. Every caller of the accessor
    // (the FC dispatch, `has_block_children`, block, grid) saw an empty child
    // list; only flex, which read `children` directly, worked. Shadow DOM
    // rendered nothing.
    for display in ["block", "flex", "grid"] {
        let d = laid_out(&format!(
            "<div id=A style=\"display:{display}\"><template shadowrootmode=open>\
             <div id=a1 style=\"height:200px\"></div></template></div>"
        ));
        let host = find_shadow(&d.root, "A").unwrap();
        let child = find_shadow(&d.root, "a1").unwrap();
        assert_eq!(
            host.layout.border_rect.h, 200.0,
            "host sizes to its shadow tree ({display})"
        );
        assert_eq!(
            child.layout.border_rect.h, 200.0,
            "the shadow child is laid out ({display})"
        );
    }
}

#[test]
fn host_rules_style_the_host() {
    // `:host` returned false unconditionally, so the most common rule in any
    // shadow stylesheet did nothing.
    let d = laid_out(
        "<div id=A><template shadowrootmode=open>\
        <style>:host { display: block; height: 150px }</style><span>s</span></template></div>",
    );
    let host = find_shadow(&d.root, "A").unwrap();
    assert_eq!(host.layout.border_rect.h, 150.0);
}

#[test]
fn host_with_an_argument_matches_only_when_the_host_does() {
    let sheet = "<style>:host { display:block; height:150px } :host(.big) { width:300px }</style>";
    let plain = laid_out(&format!(
        "<div id=A><template shadowrootmode=open>{sheet}<span>s</span></template></div>"
    ));
    let big = laid_out(&format!(
        "<div id=A class=big><template shadowrootmode=open>{sheet}<span>s</span></template></div>"
    ));
    assert_eq!(
        find_shadow(&plain.root, "A").unwrap().layout.border_rect.w,
        384.0,
        ":host(.big) must not match a host without the class"
    );
    assert_eq!(
        find_shadow(&big.root, "A").unwrap().layout.border_rect.w,
        300.0
    );
}

#[test]
fn host_argument_descendant_selector_styles_shadow_descendant() {
    let d = laid_out(
        "<div id=A class=big><template shadowrootmode=open>\
         <style>:host(.big) p { color: rgb(7, 8, 9); height: 20px }</style>\
         <p id=p>s</p></template></div>",
    );
    let host = find_shadow(&d.root, "A").unwrap();
    let p = find_shadow(&d.root, "p").unwrap();
    assert_eq!(
        p.style.color,
        crate::types::Color::rgb(7, 8, 9),
        ":host(.big) p should style the shadow descendant"
    );
    assert_ne!(
        host.style.color,
        crate::types::Color::rgb(7, 8, 9),
        "the descendant selector must not be applied as a host-only rule"
    );
}

#[test]
fn projected_slot_content_is_laid_out() {
    let d = laid_out(
        "<div id=host><template shadowrootmode=open>\
        <slot name=title></slot><slot></slot></template>\
        <h2 slot=title style=\"height:30px\">T</h2><p style=\"height:20px\">B</p></div>",
    );
    let h2 = find_shadow(&d.root, "host").unwrap();
    // Both slots projected and both laid out, so the host is as tall as both.
    assert!(
        h2.layout.border_rect.h >= 50.0,
        "host should contain both projected children, got {}",
        h2.layout.border_rect.h
    );
}

// ─── HTMLSlotElement ────────────────────────────────────────────────────────

fn slotted_doc() -> crate::Document {
    doc("<div id=host><template shadowrootmode=open>\
         <slot name=title></slot><slot></slot></template>\
         <h2 id=t slot=title>T</h2><p id=b>B</p><span id=u slot=nope>U</span></div>")
}

#[test]
fn assigned_nodes_and_elements() {
    let d = slotted_doc();
    let host = d.query_selector("#host").unwrap();
    let slots = d.shadow_slots(host);
    assert_eq!(slots.len(), 2, "both slots are found in the shadow tree");

    let named = d.assigned_nodes(slots[0]);
    assert_eq!(named.len(), 1);
    assert_eq!(d.tag_name(named[0]), Some("h2"));

    // The default slot takes children with NO `slot` attribute — not the one
    // pointing at a slot that does not exist.
    let default = d.assigned_elements(slots[1]);
    assert_eq!(default.len(), 1);
    assert_eq!(d.tag_name(default[0]), Some("p"));
}

#[test]
fn assigned_slot_points_back() {
    let d = slotted_doc();
    let host = d.query_selector("#host").unwrap();
    let slots = d.shadow_slots(host);
    let t = d.query_selector("#t").unwrap();
    let b = d.query_selector("#b").unwrap();
    let u = d.query_selector("#u").unwrap();
    assert_eq!(
        d.assigned_slot(t),
        Some(slots[0]),
        "slot=title goes to the named slot"
    );
    assert_eq!(
        d.assigned_slot(b),
        Some(slots[1]),
        "no slot attribute goes to the default"
    );
    // `slot=nope` names a slot that does not exist, so it is not assigned.
    assert_eq!(d.assigned_slot(u), None);
}

#[test]
fn slotchange_fires_on_every_slot() {
    use crate::dom::events::ListenerOptions;
    use std::sync::{Arc, Mutex};
    let mut d = slotted_doc();
    let host = d.query_selector("#host").unwrap();
    let slots = d.shadow_slots(host);
    let n = Arc::new(Mutex::new(0));
    for s in &slots {
        let c = n.clone();
        d.add_event_listener(
            *s,
            "slotchange",
            Box::new(move |_, _d| *c.lock().unwrap() += 1),
            ListenerOptions::default(),
        );
    }
    d.fire_slot_change(host);
    assert_eq!(*n.lock().unwrap(), 2, "nothing fired slotchange before");
}

#[test]
fn host_context_matches_an_ancestor() {
    // `:host-context(sel)` matches when the host OR AN ANCESTOR matches.
    // Matching only the host made this rule inert.
    let d = laid_out(
        "<body class=dark><div id=A><template shadowrootmode=open>\
        <style>:host-context(.dark) { display:block; height:120px }</style>\
        <span>s</span></template></div></body>",
    );
    assert_eq!(
        find_shadow(&d.root, "A").unwrap().layout.border_rect.h,
        120.0
    );

    // And does not match when no ancestor does.
    let light = laid_out(
        "<body><div id=A><template shadowrootmode=open>\
        <style>:host-context(.dark) { display:block; height:120px }</style>\
        <span>s</span></template></div></body>",
    );
    assert_ne!(
        find_shadow(&light.root, "A").unwrap().layout.border_rect.h,
        120.0
    );
}

// ─── ShadowRoot as a node ───────────────────────────────────────────────────

#[test]
fn shadow_root_idl_members_answer() {
    use crate::types::SlotAssignment;
    let mut d = doc("<div id=a>x</div>");
    let a = d.query_selector("#a").unwrap();
    let sr = d.attach_shadow(a, ShadowMode::Open).unwrap();

    // Defaults, per ShadowRootInit.
    assert!(!d.shadow_delegates_focus(sr));
    assert_eq!(d.shadow_slot_assignment(sr), SlotAssignment::Named);
    assert!(!d.shadow_clonable(sr));
    assert!(!d.shadow_serializable(sr));
    assert!(d.shadow_adopted_stylesheets(sr).is_empty());

    d.set_shadow_delegates_focus(sr, true);
    d.set_shadow_slot_assignment(sr, SlotAssignment::Manual);
    d.set_shadow_clonable(sr, true);
    d.set_shadow_serializable(sr, true);
    assert!(d.shadow_delegates_focus(sr));
    assert_eq!(d.shadow_slot_assignment(sr), SlotAssignment::Manual);
    assert!(d.shadow_clonable(sr));
    assert!(d.shadow_serializable(sr));
}

#[test]
fn adopted_stylesheets_are_recorded_and_applied() {
    let d0 = doc("<div id=a><template shadowrootmode=open><span id=s>x</span></template></div>");
    let a0 = d0.query_selector("#a").unwrap();
    let sr0 = d0.shadow_root_of(a0).unwrap();
    let mut d = d0;
    d.set_shadow_adopted_stylesheets(sr0, vec!["span { height: 40px; display: block }".into()]);
    // Recorded...
    assert_eq!(d.shadow_adopted_stylesheets(sr0).len(), 1);
    // ...and actually styling the tree, not just stored.
    crate::css::apply_cascade_vp(
        &mut d.root,
        &d.stylesheet,
        None,
        16.0,
        400.0,
        900.0,
        0,
        false,
    );
    let mut eng = crate::LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut d, 400.0);
    let s = find_shadow(&d.root, "s").unwrap();
    assert_eq!(s.layout.border_rect.h, 40.0);
}

#[test]
fn a_closed_root_still_has_an_identity_internally() {
    let mut d = doc("<div id=a>x</div>");
    let a = d.query_selector("#a").unwrap();
    let sr = d.attach_shadow(a, ShadowMode::Closed).unwrap();
    // `attachShadow` hands the caller the root even when closed — closed hides
    // it from `element.shadowRoot`, not from whoever created it.
    assert_eq!(d.shadow_root_mode(sr), Some(ShadowMode::Closed));
    assert_eq!(
        d.shadow_root_of(a),
        None,
        "element.shadowRoot is null when closed"
    );
}
#[test]
fn a_shadow_node_has_an_identity_of_its_own() {
    // ⛔ The bug this pins: `attach_shadow` parses the shadow markup with
    // `parse_html`, which builds a FRESH document numbering its nodes from 1 —
    // the same numbers the host document has already handed out. The shadow
    // `<p>` below came back with the HOST's node id, so asking it for its tag
    // answered `div` and asking for its rect answered the host's rect. Every
    // node-keyed API was reading a different node than the caller named.
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, r#"<p id="inner" style="height:40px">hello</p>"#);

    let kids = d.shadow_children(host);
    assert_eq!(kids.len(), 1);
    assert_ne!(kids[0], host, "a shadow child is NOT its host");
    assert_eq!(d.tag_name(kids[0]), Some("p"), "and answers its own tag");
    assert!(
        crate::dom::arena::is_shadow_node_id(kids[0]),
        "shadow ids come from the descending space, which the arena's ascending one cannot reach"
    );
}

#[test]
fn shadow_content_lays_out_and_its_box_is_reachable() {
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, r#"<p style="height:40px">hello</p>"#);

    let mut f = crate::frame::EngineFrame::new(d, 800.0, 600.0);
    f.update_frame();
    let child = f.doc.shadow_children(host)[0];

    // `getBoundingClientRect` walks the light tree; a shadow box is a real box
    // that is simply not reachable from `root.children`, so the query has to
    // fall back to the shadow-aware lookup or it answers `None` for every
    // shadow node — which is what it did.
    let rect = f
        .doc
        .get_bounding_client_rect(child)
        .expect("a laid-out shadow child has a rect");
    assert_eq!(rect.h, 40.0, "the height the shadow stylesheet asked for");
    assert!(rect.w > 0.0);
}

/// A shadow child collapses its margin exactly as a light child does.
///
/// ⛔ It did not: the host's border box sat at y=124 and the `<p>` at y=108,
/// where the same markup in the light tree puts both at 124. The host's own
/// rects were identical in the two cases, which is what said the host was
/// right and only the child had moved.
///
/// `shift_rects` iterated `children` — the LIGHT children. A host that moves
/// after its subtree is laid out (which is what absorbing a collapsed first-child
/// margin does) took its light children along and left its shadow tree behind.
/// It walks `effective_children_mut()` now, the same accessor layout uses to
/// find them in the first place.
#[test]
fn a_shadow_childs_margin_collapses_the_way_a_light_childs_does() {
    let mut d =
        crate::html::parse_html(r#"<div style="height:100px">spacer</div><div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, r#"<p style="height:40px">hello</p>"#);
    let mut f = crate::frame::EngineFrame::new(d, 800.0, 600.0);
    f.update_frame();

    let child = f.doc.shadow_children(host)[0];
    let host_rect = f.doc.get_bounding_client_rect(host).unwrap();
    let child_rect = f.doc.get_bounding_client_rect(child).unwrap();
    assert_eq!(
        child_rect.y, host_rect.y,
        "the p's margin collapses through its host, as it does in the light tree"
    );
}

#[test]
fn a_shadow_element_is_an_element_and_only_the_root_is_a_fragment() {
    // ⛔ Renumbering the shadow subtree into the shadow id space made
    // `is_shadow_node_id` true for every node in it, and `node_type` answered
    // 11 — DocumentFragment — for a `<p>`. The id space says "not in the
    // arena"; it does not say "is a shadow root".
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    let root = d
        .attach_shadow(host, crate::types::ShadowMode::Open)
        .unwrap();
    d.set_shadow_inner_html(host, r#"<p>hello<!--c--></p>"#);

    assert_eq!(d.node_type(root), 11, "the ROOT is a DocumentFragment");
    let p = d.shadow_children(host)[0];
    assert_eq!(
        d.node_type(p),
        1,
        "an element inside the shadow tree is an ELEMENT"
    );
    let inside: Vec<u16> = d
        .child_nodes(p)
        .into_iter()
        .map(|c| d.node_type(c))
        .collect();
    assert!(
        inside.contains(&3),
        "its text child is a text node: {inside:?}"
    );
    assert!(
        inside.contains(&8),
        "its comment child is a comment: {inside:?}"
    );
}

#[test]
fn a_shadow_root_keeps_its_identity_when_its_content_is_replaced() {
    // ⛔ `set_shadow_inner_html` used to call `attach_shadow` again, which
    // minted a NEW root id — so the id `attachShadow()` handed back stopped
    // naming anything the moment the page wrote to `innerHTML`. In a browser
    // the object survives; so does everything it carries.
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    let root = d
        .attach_shadow(host, crate::types::ShadowMode::Closed)
        .unwrap();
    d.set_shadow_delegates_focus(root, true);

    d.set_shadow_inner_html(host, r#"<p>first</p>"#);
    assert_eq!(d.shadow_root_of(host), None, "still closed — mode survives");
    assert_eq!(
        d.node_type(root),
        11,
        "the same root id still names the root"
    );
    assert!(
        d.shadow_delegates_focus(root),
        "delegatesFocus survives a content rewrite"
    );

    d.set_shadow_inner_html(host, r#"<span>second</span>"#);
    assert_eq!(d.node_type(root), 11);
    assert_eq!(d.tag_name(d.shadow_children(host)[0]), Some("span"));
    assert_eq!(
        d.child_nodes(root).len(),
        1,
        "the root's children ARE the shadow tree's top level"
    );
}

#[test]
fn a_shadow_node_deeper_than_the_top_level_has_the_right_parent() {
    // The renumbering gave every shadow node an id with no arena entry, so
    // `parent_node` reads an arena parent of 0 for all of them. The "no arena
    // parent → the host" branch is right for a TOP-LEVEL shadow child and
    // wrong for anything below it, which would flatten a whole shadow tree
    // onto the host.
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    let root = d
        .attach_shadow(host, crate::types::ShadowMode::Open)
        .unwrap();
    d.set_shadow_inner_html(host, r#"<p><span>deep</span></p>"#);

    let p = d.shadow_children(host)[0];
    let span = d
        .child_nodes(p)
        .into_iter()
        .find(|c| d.tag_name(*c) == Some("span"))
        .expect("the span is reachable");
    let text = d.child_nodes(span)[0];

    assert_eq!(
        d.parent_node(span),
        p,
        "a span's parent is the p, not the host"
    );
    assert_eq!(
        d.parent_node(text),
        span,
        "and the text's parent is the span"
    );
    assert_eq!(
        d.parent_node(p),
        root,
        "a TOP-LEVEL shadow child's parent is the shadow ROOT (DOM §4.8)"
    );
}

#[test]
fn an_event_on_a_shadow_node_walks_out_through_the_host() {
    use std::sync::{Arc, Mutex};
    let mut d = crate::html::parse_html(r#"<div id="outer"><div id="host"></div></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    let outer = d.get_element_by_id("outer").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, r#"<p><span>deep</span></p>"#);

    let p = d.shadow_children(host)[0];
    let span = d
        .child_nodes(p)
        .into_iter()
        .find(|c| d.tag_name(*c) == Some("span"))
        .unwrap();

    let seen: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    for target in [span, p, host, outer] {
        let log = seen.clone();
        d.add_event_listener(
            target,
            "click",
            Box::new(move |e, _| {
                log.lock().unwrap().push(e.current_target);
            }),
            crate::dom::events::ListenerOptions::capture(false),
        );
    }

    let mut event = crate::dom::events::DomEvent::new("click", span);
    d.dispatch_event(&mut event);

    let fired = seen.lock().unwrap().clone();
    assert!(
        fired.starts_with(&[span, p, host]),
        "the path climbs the shadow tree and out through the host: {fired:?}"
    );
    assert!(
        fired.contains(&outer),
        "and keeps going in the light tree: {fired:?}"
    );
}

#[test]
fn a_shadow_elements_attributes_come_back_in_source_order_too() {
    // A shadow node lives only in the render tree, so its attributes are read
    // through the `find_webcore` fallback rather than the arena. Both stores
    // are `AttrMap` now; this is the assert that says so.
    let mut d = crate::html::parse_html(r#"<div id="host"></div>"#);
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, r#"<p id="p" zebra="1" alpha="2" mid="3">x</p>"#);

    let p = d.shadow_children(host)[0];
    assert_eq!(
        d.get_attribute_names(p),
        vec!["id", "zebra", "alpha", "mid"]
    );
    assert_eq!(d.attributes_length(p), 4);
    assert_eq!(
        d.attributes_item(p, 1).map(|a| a.name),
        Some("zebra".into())
    );
    assert_eq!(d.class_list(p).length(), 0);
}

/// ⛔ The DOM surface answering for a node INSIDE a shadow tree.
///
/// A shadow node is a DOM node, but it is not an ARENA node — its id comes
/// from a separate space counting down from `u32::MAX - 2` — so every
/// accessor that reaches for the arena first was answering for it wrongly, or
/// not at all. Four did, and one of them crashed:
///
/// | | was | Chrome |
/// |---|---|---|
/// | `shadowRoot.nodeName` | `""` | `#document-fragment` |
/// | `p.localName` | `""` | `p` |
/// | `p.textContent` | **panic** | `"hello!"` |
/// | `p.isConnected` | `false` | `true` |
///
/// The panic is the one with no workaround: `arena.get` asserts on an
/// out-of-range id, so `shadowRoot.querySelector("p").textContent` — an
/// ordinary call — brought the process down.
///
/// Measured against Chrome with this exact markup, not assumed. What is NOT
/// wrong is also pinned below, because a first pass compared against a
/// different fixture and reported `host.childNodes` as a defect when it was
/// correctly answering with a light-DOM text node.
#[test]
fn the_dom_surface_answers_for_a_node_inside_a_shadow_tree() {
    let mut d = crate::html::parse_html("<div id=host></div>");
    let host = d.get_element_by_id("host").unwrap();
    let sr = d
        .attach_shadow(host, crate::types::ShadowMode::Open)
        .expect("attachShadow returns the root");
    d.set_shadow_inner_html(host, "<p id=inner>hello<b>!</b></p>");
    let p = d.shadow_children(host)[0];

    // The four that were wrong.
    assert_eq!(
        d.node_name(sr),
        "#document-fragment",
        "a ShadowRoot is a DocumentFragment"
    );
    assert_eq!(d.local_name(p), "p");
    assert_eq!(d.text_content(p), "hello!", "and this used to PANIC");
    assert!(d.is_connected(p), "connected through its host, DOM §4.4");

    // …and the ones that were already right, so a fix cannot quietly trade one
    // for another.
    assert_eq!(d.node_type(sr), 11);
    assert_eq!(d.node_type(p), 1);
    assert_eq!(d.child_nodes(sr).len(), 1);
    assert_eq!(d.child_nodes(p).len(), 2);
    assert_eq!(d.parent_node(p), sr);
    assert_eq!(d.get_attribute(p, "id").as_deref(), Some("inner"));
    assert_eq!(d.child_nodes(host).len(), 0, "the host's LIGHT children");
}

/// A shadow node's `textContent` excludes comments, exactly as an ordinary
/// node's does — the render-tree walk must not be a looser rule than the
/// arena walk it stands in for.
#[test]
fn a_shadow_nodes_text_content_excludes_comments() {
    let mut d = crate::html::parse_html("<div id=host></div>");
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, "<p>a<!--hidden-->b</p>");
    let p = d.shadow_children(host)[0];
    assert_eq!(
        d.text_content(p),
        "ab",
        "a comment carries data but is not content"
    );
}

/// A node in a shadow tree hanging off a DETACHED host is not connected.
#[test]
fn a_shadow_node_under_a_detached_host_is_not_connected() {
    let mut d = crate::html::parse_html("<div id=other></div>");
    let host = d.create_element("div");
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, "<p>x</p>");
    let p = d.shadow_children(host)[0];
    assert!(!d.is_connected(host), "the host was never inserted");
    assert!(
        !d.is_connected(p),
        "so nothing in its shadow tree is connected"
    );
}

/// ⛔ Why the render-tree `textContent` walk excludes only `#comment`, where
/// the arena walk excludes comments AND processing instructions.
///
/// It is not a looser rule, it is the same rule over a narrower input. The
/// render walk is reached only for nodes the arena has never held — today,
/// shadow trees — and a shadow tree is built by the HTML parser, which folds
/// `<?…?>` into a COMMENT (HTML §13.2.5.42). Measured, not assumed: the markup
/// below produces `#text "a"`, `#comment "?foo bar?"`, `#text "b"`.
///
/// A PI's render box would be indistinguishable anyway — `create_processing_instruction`
/// gives it the TARGET as its tag, not a `#`-prefixed name — so a tag-based
/// exclusion could not express it. If the render walk ever sees an arena-backed
/// node, this reasoning goes with it.
#[test]
fn a_processing_instruction_in_shadow_markup_is_a_comment() {
    let mut d = crate::html::parse_html("<div id=host></div>");
    let host = d.get_element_by_id("host").unwrap();
    d.attach_shadow(host, crate::types::ShadowMode::Open);
    d.set_shadow_inner_html(host, "<p>a<?foo bar?>b</p>");
    let p = d.shadow_children(host)[0];

    fn find<'a>(n: &'a crate::types::WebCore, id: u32) -> Option<&'a crate::types::WebCore> {
        if n.node_id == id {
            return Some(n);
        }
        n.effective_children().iter().find_map(|c| find(c, id))
    }
    let tags: Vec<&str> = find(&d.root, p)
        .unwrap()
        .children
        .iter()
        .map(|c| c.tag.as_str())
        .collect();
    assert_eq!(
        tags,
        vec!["#text", "#comment", "#text"],
        "the PI folded to a comment"
    );
    assert_eq!(
        d.text_content(p),
        "ab",
        "so the comment exclusion covers it"
    );
}
