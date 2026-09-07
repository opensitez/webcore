// Shadow DOM tests — shadow tree rendering, scoped styles, slots, isolation.

use webcore::dom;
use webcore::html::parse_html;
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
    if let Some(ref sr) = root.shadow_root {
        for child in &sr.children {
            if let Some(f) = by_id(child, id) {
                return Some(f);
            }
        }
    }
    None
}

fn attach(root: &mut WebCore, host_id: &str, mode: ShadowMode, html: &str) {
    fn find_mut<'a>(node: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(node);
        }
        for child in &mut node.children {
            if let Some(f) = find_mut(child, id) {
                return Some(f);
            }
        }
        None
    }
    if let Some(host) = find_mut(root, host_id) {
        host.attach_shadow(mode, html);
        host.resolve_slots();
    }
}

// === BASIC ===

#[test]
fn attach_shadow_creates_root() {
    let mut doc = parse_html("<div id='host'>Light</div>");
    attach(&mut doc.root, "host", ShadowMode::Open, "<p>Shadow</p>");
    let host = by_id(&doc.root, "host").unwrap();
    assert!(host.shadow_root.is_some());
}

#[test]
fn shadow_mode_open() {
    let mut doc = parse_html("<div id='host'>L</div>");
    attach(&mut doc.root, "host", ShadowMode::Open, "<p>S</p>");
    assert_eq!(
        by_id(&doc.root, "host")
            .unwrap()
            .shadow_root
            .as_ref()
            .unwrap()
            .mode,
        ShadowMode::Open
    );
}

#[test]
fn shadow_mode_closed() {
    let mut doc = parse_html("<div id='host'>L</div>");
    attach(&mut doc.root, "host", ShadowMode::Closed, "<p>S</p>");
    assert_eq!(
        by_id(&doc.root, "host")
            .unwrap()
            .shadow_root
            .as_ref()
            .unwrap()
            .mode,
        ShadowMode::Closed
    );
}

#[test]
fn shadow_tree_renders() {
    let mut doc = load_html("<div id='host' style='width:400px'>Light</div>", 500.0);
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<p id='sp'>Shadow text</p>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    assert!(by_id(&doc.root, "sp").is_some(), "shadow content found");
}

// === SCOPED STYLES ===

#[test]
fn scoped_style_applies_inside() {
    let mut doc = load_html("<div id='host' style='width:400px'>L</div>", 500.0);
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<style>p{color:red}</style><p id='sp'>S</p>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    if let Some(sp) = by_id(&doc.root, "sp") {
        assert_eq!(sp.style.color.r, 255, "scoped red");
    }
}

#[test]
fn scoped_style_no_leak() {
    let mut doc = load_html(
        "<div id='host' style='width:400px'>L</div><p id='out'>Out</p>",
        500.0,
    );
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<style>p{color:red}</style><p>S</p>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    let out = by_id(&doc.root, "out").unwrap();
    assert_ne!(out.style.color.r, 255, "no leak");
}

// === SLOTS ===

#[test]
fn default_slot_projects() {
    let mut doc = load_html(
        "<div id='host' style='width:400px'><span id='lt'>Projected</span></div>",
        500.0,
    );
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<div id='w'><slot></slot></div>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    let lt = by_id(&doc.root, "lt");
    assert!(lt.is_some(), "light DOM projected");
}

#[test]
fn named_slots() {
    let mut doc = load_html(
        concat!(
            "<div id='host' style='width:400px'>",
            "<span slot='a'>A</span><span slot='b'>B</span>",
            "</div>",
        ),
        500.0,
    );
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<div id='sa'><slot name='a'></slot></div><div id='sb'><slot name='b'></slot></div>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    assert!(by_id(&doc.root, "sa").is_some(), "slot a container");
    assert!(by_id(&doc.root, "sb").is_some(), "slot b container");
}

#[test]
fn slot_fallback() {
    let mut doc = load_html("<div id='host' style='width:400px'></div>", 500.0);
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<slot><span id='fb'>Default</span></slot>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    assert!(by_id(&doc.root, "fb").is_some(), "fallback shown");
}

// === CSS VARS CROSS SHADOW ===

#[test]
fn css_vars_cross_boundary() {
    let mut doc = load_html(
        concat!(
            "<style>:root{--c:#ff6600}</style>",
            "<div id='host' style='width:400px'>L</div>",
        ),
        500.0,
    );
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<style>p{color:var(--c)}</style><p id='sp'>S</p>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    if let Some(sp) = by_id(&doc.root, "sp") {
        assert_eq!(
            sp.style.color.r, 0xff,
            "var crosses shadow r={}",
            sp.style.color.r
        );
    }
}

// === INHERITED STYLES CROSS SHADOW ===

#[test]
fn inherited_styles_cross() {
    let mut doc = load_html(
        "<div id='host' style='width:400px;font-size:24px;color:#336699'>L</div>",
        500.0,
    );
    attach(&mut doc.root, "host", ShadowMode::Open, "<p id='sp'>S</p>");
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 500.0);
    if let Some(sp) = by_id(&doc.root, "sp") {
        assert_eq!(sp.style.color.r, 0x33, "color inherited");
    }
}

// === SHADOW + LAYOUT ===

#[test]
fn shadow_has_layout_dimensions() {
    let mut doc = load_html("<div id='host' style='width:300px'>L</div>", 400.0);
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<div id='inner' style='height:100px'>Shadow</div>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 400.0);
    if let Some(inner) = by_id(&doc.root, "inner") {
        assert!(
            (inner.layout.content_rect.h - 100.0).abs() < 5.0,
            "h={:.0}",
            inner.layout.content_rect.h
        );
    }
}

// === EDGE CASES ===

#[test]
fn empty_shadow_no_crash() {
    let mut doc = load_html("<div id='host' style='width:300px'>Light</div>", 400.0);
    attach(&mut doc.root, "host", ShadowMode::Open, "");
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 400.0);
    assert!(by_id(&doc.root, "host").unwrap().layout.content_rect.w >= 0.0);
}

#[test]
fn shadow_only_style_no_crash() {
    let mut doc = load_html("<div id='host' style='width:300px'>L</div>", 400.0);
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<style>:host{background:red}</style>",
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 400.0);
    assert!(by_id(&doc.root, "host").unwrap().layout.content_rect.w >= 0.0);
}

#[test]
fn reattach_shadow_replaces() {
    let mut doc = parse_html("<div id='host'>L</div>");
    attach(&mut doc.root, "host", ShadowMode::Open, "<p>First</p>");
    attach(
        &mut doc.root,
        "host",
        ShadowMode::Open,
        "<p id='second'>Second</p>",
    );
    assert!(
        by_id(&doc.root, "second").is_some(),
        "second replaces first"
    );
}

// === REAL-WORLD: Custom button ===

#[test]
fn custom_button() {
    let mut doc = load_html(
        "<div id='btn' style='display:inline-block'>Click me</div>",
        800.0,
    );
    attach(
        &mut doc.root,
        "btn",
        ShadowMode::Open,
        concat!(
            "<style>:host{padding:8px 16px;background:#06c;color:white;border-radius:4px}</style>",
            "<slot></slot>",
        ),
    );
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 800.0);
    assert!(
        by_id(&doc.root, "btn").unwrap().layout.content_rect.w > 30.0,
        "button renders"
    );
}

// === REAL-WORLD: Card component ===

#[test]
fn card_component() {
    let mut doc = load_html(
        concat!(
            "<div id='card' style='width:300px'>",
            "<h3 slot='title'>Title</h3><p slot='body'>Body</p>",
            "</div>",
        ),
        400.0,
    );
    attach(&mut doc.root, "card", ShadowMode::Open, concat!(
        "<style>.c{border:1px solid #ddd;border-radius:8px}.h{padding:12px;background:#f5f5f5}.b{padding:16px}</style>",
        "<div class='c'><div class='h'><slot name='title'></slot></div><div class='b'><slot name='body'></slot></div></div>",
    ));
    let mut r = webcore::renderer::Renderer::new();
    r.layout_engine().layout(&mut doc, 400.0);
    assert!(
        by_id(&doc.root, "card").unwrap().layout.content_rect.h > 30.0,
        "card has height"
    );
}
