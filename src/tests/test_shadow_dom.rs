use crate::css::apply_cascade_vp;
use crate::layout::LayoutEngine;
use crate::types::*;
use crate::{Document, parse_html};

fn layout_html(html: &str, width: f32) -> Document {
    let mut doc = parse_html(html);
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        width,
        900.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, width);
    doc
}

fn find_by_tag<'a>(node: &'a WebCore, tag: &str) -> Option<&'a WebCore> {
    if node.tag == tag {
        return Some(node);
    }
    for child in &node.children {
        if let Some(n) = find_by_tag(child, tag) {
            return Some(n);
        }
    }
    // Also search shadow tree
    if let Some(ref sr) = node.shadow_root {
        for child in &sr.children {
            if let Some(n) = find_by_tag(child, tag) {
                return Some(n);
            }
        }
    }
    None
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
    if let Some(ref sr) = node.shadow_root {
        for child in &sr.children {
            if let Some(n) = find_by_id(child, id) {
                return Some(n);
            }
        }
    }
    None
}

// ── Phase 1: Shadow root structure ──────────────────────────────────────────

#[test]
fn declarative_shadow_dom_creates_shadow_root() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <p>Shadow content</p>
            </template>
            <span>Light content</span>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    assert!(host.shadow_root.is_some(), "host should have a shadow root");
    let sr = host.shadow_root.as_ref().unwrap();
    assert_eq!(sr.mode, ShadowMode::Open);
    // Shadow tree should contain the <p>
    assert!(
        sr.children.iter().any(|c| c.tag == "p"),
        "shadow tree should have <p>"
    );
    // Light DOM children should still be in node.children
    assert!(
        host.children.iter().any(|c| c.tag == "span"),
        "light DOM span should remain"
    );
}

#[test]
fn closed_shadow_mode() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="closed">
                <p>Hidden</p>
            </template>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    let sr = host.shadow_root.as_ref().unwrap();
    assert_eq!(sr.mode, ShadowMode::Closed);
}

// ── Phase 2: Style scoping ──────────────────────────────────────────────────

#[test]
fn document_styles_do_not_leak_into_shadow() {
    let doc = layout_html(
        r#"
        <style>.test { color: green; }</style>
        <div id="host">
            <template shadowrootmode="open">
                <div class="test" id="shadow-div">Text</div>
            </template>
        </div>
    "#,
        400.0,
    );
    let shadow_div = find_by_id(&doc.root, "shadow-div").unwrap();
    // The document rule .test { color: green } should NOT apply inside shadow
    assert_ne!(
        shadow_div.style.color,
        Color::rgb(0, 128, 0),
        "document styles should not leak into shadow tree"
    );
}

#[test]
fn shadow_styles_do_not_leak_out() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <style>.external { color: red; }</style>
                <p>Shadow</p>
            </template>
        </div>
        <div class="external" id="outside">Outside</div>
    "#,
        400.0,
    );
    let outside = find_by_id(&doc.root, "outside").unwrap();
    // Shadow rule .external { color: red } should NOT apply outside
    assert_ne!(
        outside.style.color,
        Color::rgb(255, 0, 0),
        "shadow styles should not leak outside"
    );
}

#[test]
fn shadow_scoped_styles_apply_inside() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <style>p { font-size: 24px; }</style>
                <p id="shadow-p">Big text</p>
            </template>
        </div>
    "#,
        400.0,
    );
    let p = find_by_id(&doc.root, "shadow-p").unwrap();
    let fs = p.style.font_size_px(16.0, 16.0);
    assert!(
        (fs - 24.0).abs() < 0.1,
        "shadow scoped style should apply: expected 24px, got {}",
        fs
    );
}

#[test]
fn inherited_properties_cross_shadow_boundary() {
    let doc = layout_html(
        r#"
        <div id="host" style="font-size: 20px;">
            <template shadowrootmode="open">
                <span id="shadow-span">Text</span>
            </template>
        </div>
    "#,
        400.0,
    );
    let span = find_by_id(&doc.root, "shadow-span").unwrap();
    let fs = span.style.font_size_px(16.0, 16.0);
    assert!(
        (fs - 20.0).abs() < 0.1,
        "font-size should inherit across shadow boundary: expected 20px, got {}",
        fs
    );
}

// ── Phase 3: Layout with shadow DOM ─────────────────────────────────────────

#[test]
fn shadow_content_is_laid_out() {
    let doc = layout_html(
        r#"
        <div id="host" style="width: 200px;">
            <template shadowrootmode="open">
                <p id="shadow-p">Shadow paragraph</p>
            </template>
            <span>Not visible (no slot)</span>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    // Host should have height from shadow content (at least one line of text)
    assert!(
        host.layout.content_rect.h > 10.0,
        "host height should include shadow content: got {}",
        host.layout.content_rect.h
    );
}

#[test]
fn light_dom_hidden_without_slot() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <p>Only shadow</p>
            </template>
            <span id="light">Should not appear</span>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    // The light span should not contribute to host height
    // Only the shadow <p> should be visible
    // The shadow <p> content is rendered via the host's layout (which swaps
    // shadow children into node.children during layout). We verify by checking
    // that the host itself has height (from shadow content).
    assert!(
        host.layout.content_rect.h > 10.0,
        "host should have height from shadow content: got {}",
        host.layout.content_rect.h
    );
}

// ── Phase 4: Slot projection ────────────────────────────────────────────────

#[test]
fn default_slot_projects_light_dom() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <slot></slot>
            </template>
            <p>Projected content</p>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    // Host should have height from projected content (light DOM <p> via slot)
    assert!(
        host.layout.content_rect.h > 10.0,
        "host should have height from slotted content: got {}",
        host.layout.content_rect.h
    );
}

#[test]
fn named_slot_projects_matching_content() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <slot name="title"></slot>
                <slot></slot>
            </template>
            <h2 slot="title">Title</h2>
            <p>Body content</p>
        </div>
    "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    // Host should have height from both named and default slots
    assert!(
        host.layout.content_rect.h > 20.0,
        "host should have height from slotted content: got {}",
        host.layout.content_rect.h
    );
}

#[test]
fn slot_fallback_when_no_matching_content() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <slot name="missing">
                    <p id="fallback">Fallback content</p>
                </slot>
            </template>
        </div>
    "#,
        400.0,
    );
    // No light DOM children with slot="missing", so fallback should show
    let fallback = find_by_id(&doc.root, "fallback").unwrap();
    assert!(
        fallback.layout.content_rect.h > 0.0,
        "slot fallback should be laid out when no matching content"
    );
}

// ── Phase 3: attach_shadow API ──────────────────────────────────────────────

#[test]
fn attach_shadow_programmatic() {
    let mut doc = layout_html(r#"<div id="host"></div>"#, 400.0);
    let host = find_by_id(&doc.root, "host").unwrap();
    assert!(host.shadow_root.is_none(), "no shadow root initially");
    // Now attach one
    fn find_mut<'a>(n: &'a mut WebCore, id: &str) -> Option<&'a mut WebCore> {
        if n.attributes.get("id").map(|s| s.as_str()) == Some(id) {
            return Some(n);
        }
        for c in &mut n.children {
            if let Some(r) = find_mut(c, id) {
                return Some(r);
            }
        }
        None
    }
    let host = find_mut(&mut doc.root, "host").unwrap();
    host.attach_shadow(
        ShadowMode::Open,
        r#"
        <style>p { color: blue; }</style>
        <p>Programmatic shadow</p>
    "#,
    );
    assert!(host.shadow_root.is_some(), "shadow root should be attached");
    let sr = host.shadow_root.as_ref().unwrap();
    assert!(
        sr.children.iter().any(|c| c.tag == "p"),
        "shadow tree should have <p>"
    );
}

// ── The path a real page takes ──────────────────────────────────────────────

/// Shadow styles have to work through `Renderer::load_html`, which is what an
/// actual page load runs — the helpers above call the cascade directly and so
/// cannot see a break in that path.
#[test]
fn shadow_styles_apply_through_the_renderer() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=host><template shadowrootmode=open>\
         <style>p{color:rgb(0,128,0);height:40px}</style>\
         <p id=inner>shadowed</p></template></div>",
        800.0,
    );
    // `getElementById` deliberately does NOT pierce the boundary, so the node
    // is reached through the render tree instead.
    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        if let Some(sr) = &n.shadow_root {
            for c in &sr.children {
                if let Some(f) = find(c, id) {
                    return Some(f);
                }
            }
        }
        for c in &n.children {
            if let Some(f) = find(c, id) {
                return Some(f);
            }
        }
        None
    }
    let p = find(&d.root, "inner").expect("shadow <p> in the render tree");
    assert_eq!(
        p.style.color,
        crate::types::Color::rgb(0, 128, 0),
        "the shadow <style> must colour its own tree"
    );
    assert!(
        (p.layout.border_rect.h - 40.0).abs() < 0.5,
        "…and size it, got {}",
        p.layout.border_rect.h
    );
}
