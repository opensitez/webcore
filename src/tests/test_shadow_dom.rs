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

fn find_composed_by_id<'a>(node: &'a WebCore, id: &str) -> Option<&'a WebCore> {
    if node.attributes.get("id").map(|s| s.as_str()) == Some(id) {
        return Some(node);
    }
    if let Some(ref sr) = node.shadow_root {
        for child in &sr.children {
            if let Some(n) = find_composed_by_id(child, id) {
                return Some(n);
            }
        }
    }
    for child in &node.children {
        if let Some(n) = find_composed_by_id(child, id) {
            return Some(n);
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
fn slotted_light_dom_keeps_document_styles_in_composed_layout() {
    let html = r#"
        <style>
            #host { display: block; position: relative; width: 360px; }
            .panel { display: grid; grid-template-columns: repeat(3, 1fr); padding: 16px; gap: 12px; }
            .item { height: 20px; }
        </style>
        <div id="host">
            <template shadowrootmode="open">
                <slot name="dropdown"></slot>
            </template>
            <div id="panel" slot="dropdown" class="panel">
                <div class="item"></div><div class="item"></div><div class="item"></div>
            </div>
        </div>
    "#;
    let doc = layout_html(html, 400.0);
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(panel.style.display, Display::Grid);
    assert_eq!(panel.style.padding_top.resolve(16.0, 400.0, 16.0), 16.0);
    assert!(
        panel.layout.border_rect.w > 0.0 && panel.layout.border_rect.h > 0.0,
        "projected slotted panel should keep its styled composed layout, got {:?}",
        panel.layout.border_rect
    );

    let mut doc = doc;
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        400.0,
        900.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, 400.0);
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(
        panel.style.padding_top.resolve(16.0, 400.0, 16.0),
        16.0,
        "projected light DOM must not be recascaded with the shadow stylesheet"
    );
    assert!(panel.layout.border_rect.w > 0.0 && panel.layout.border_rect.h > 0.0);
}

#[test]
fn large_stylesheet_recascade_preserves_projected_document_styles() {
    let mut css = String::from(
        r#"
            #host { display: block; position: relative; width: 360px; }
            .panel { display: grid; grid-template-columns: repeat(3, 1fr); padding: 16px; gap: 12px; }
            .item { height: 20px; }
        "#,
    );
    for i in 0..1100 {
        css.push_str(&format!(".dummy-{i} {{ color: rgb(1, 2, 3); }}\n"));
    }

    let html = format!(
        r#"
        <style>{css}</style>
        <div id="host">
            <template shadowrootmode="open">
                <slot name="dropdown"></slot>
            </template>
            <div id="panel" slot="dropdown" class="panel">
                <div class="item"></div><div class="item"></div><div class="item"></div>
            </div>
        </div>
    "#
    );
    let mut doc = layout_html(&html, 400.0);

    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        400.0,
        900.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, 400.0);

    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(panel.style.display, Display::Grid);
    assert_eq!(
        panel.style.padding_top.resolve(16.0, 400.0, 16.0),
        16.0,
        "large-sheet recascade must not feed projected light DOM stale match sets by node id"
    );
    assert!(panel.layout.border_rect.w > 0.0 && panel.layout.border_rect.h > 0.0);
}

#[test]
fn slotted_selector_styles_projected_light_dom() {
    let doc = layout_html(
        r#"
        <div id="host">
            <template shadowrootmode="open">
                <style>
                    slot::slotted(.panel) {
                        display: none;
                        color: rgb(7, 8, 9);
                    }
                </style>
                <slot></slot>
            </template>
            <div id="panel" class="panel">Panel</div>
        </div>
    "#,
        400.0,
    );
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(panel.style.display, Display::None);
    assert_eq!(
        panel.style.color,
        Color {
            r: 7,
            g: 8,
            b: 9,
            a: 255,
        }
    );
}

#[test]
fn host_context_slot_rules_style_projected_light_dom() {
    {
        use crate::css::{AncestorInfo, MatchContext, parse_selector};
        use std::collections::HashSet;
        let mut node = WebCore::new("div");
        node.attributes
            .insert("slot".to_string(), "dropdown".to_string());
        let host = AncestorInfo {
            tag: "x-menu".to_string(),
            attributes: Default::default(),
            child_index: 0,
            sibling_count: 1,
            type_child_index: 0,
            type_sibling_count: 1,
            node_id: 77,
        };
        let empty = HashSet::new();
        let ctx = MatchContext {
            focused_box: 0,
            keyboard_focus: false,
            type_child_index: 0,
            type_sibling_count: 1,
            html_box: Some(&node),
            hover_chain: &empty,
            focus_within_chain: &empty,
            element_id: node.node_id,
            scope_root_id: 0,
            target_id: 0,
            document_url: "",
            prev_siblings: &[],
            next_siblings: &[],
            next_sibling_nodes: &[],
        };
        let selector = parse_selector("x-menu:not([loaded], :focus-within) [slot=dropdown]");
        assert!(
            selector.matches_with_ancestors_ctx(&node, 0, 1, &[host], &ctx),
            "host-context assigned-node selector should match a slotted node"
        );
    }

    let doc = layout_html(
        r#"
        <x-menu id="host">
            <template shadowrootmode="open">
                <style>
                    x-menu:not([loaded], :focus-within) [slot=dropdown] {
                        display: none;
                    }
                </style>
                <slot name="dropdown"></slot>
            </template>
            <div id="panel" slot="dropdown">Panel</div>
        </x-menu>
    "#,
        400.0,
    );
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(panel.style.display, Display::None);
}

#[test]
fn host_context_slot_rules_respect_host_state() {
    let doc = layout_html(
        r#"
        <x-menu id="host" loaded>
            <template shadowrootmode="open">
                <style>
                    x-menu:not([loaded], :focus-within) [slot=dropdown] {
                        display: none;
                    }
                </style>
                <slot name="dropdown"></slot>
            </template>
            <div id="panel" slot="dropdown">Panel</div>
        </x-menu>
    "#,
        400.0,
    );
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_ne!(panel.style.display, Display::None);
}

#[test]
fn shadow_host_flex_item_uses_shadow_tree_for_auto_intrinsic_width() {
    let doc = layout_html(
        r#"
        <style>
          #bar { display:flex; width:300px; gap:10px; }
          x-tool { display:block; }
          #next { width:40px; height:20px; }
        </style>
        <div id="bar">
          <x-tool id="host">
            <template shadowrootmode="open">
              <div id="inner" style="width:120px;height:20px"></div>
            </template>
          </x-tool>
          <div id="next"></div>
        </div>
        "#,
        400.0,
    );
    let host = find_by_id(&doc.root, "host").unwrap();
    let next = find_by_id(&doc.root, "next").unwrap();

    assert!(
        host.layout.border_rect.w >= 119.0,
        "flex item host should size from rendered shadow content, got {:?}",
        host.layout.border_rect
    );
    assert!(
        next.layout.border_rect.x >= host.layout.border_rect.x + 129.0,
        "next flex item should be placed after host width plus gap: host {:?}, next {:?}",
        host.layout.border_rect,
        next.layout.border_rect
    );
}

#[test]
fn nested_shadow_host_resolves_slots_from_its_own_light_children() {
    let doc = layout_html(
        r#"
        <x-theme id="outer">
          <template shadowrootmode="open">
            <x-menu id="inner">
              <button slot="button">Theme</button>
              <div id="panel" slot="dropdown">Panel</div>
              <template shadowrootmode="open">
                <style>
                  x-menu:not([loaded], :focus-within) [slot=dropdown] {
                    display: none;
                  }
                </style>
                <slot name="button"></slot>
                <slot name="dropdown"></slot>
              </template>
            </x-menu>
          </template>
        </x-theme>
        "#,
        400.0,
    );
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(
        panel.style.display,
        Display::None,
        "nested shadow host must project and style its own light child, not the outer host's"
    );
}

#[test]
fn nested_projected_slot_content_matches_host_context_rules() {
    let doc = layout_html(
        r#"
        <x-theme id="outer">
          <template shadowrootmode="open">
            <x-menu id="inner">
              <button slot="button">Theme</button>
              <div id="panel" slot="dropdown">Panel</div>
              <template shadowrootmode="open">
                <style>
                  :host(:not([loaded], :focus-within)) [slot=dropdown] {
                    display: none;
                  }
                </style>
                <slot name="button"></slot>
                <slot name="dropdown"></slot>
              </template>
            </x-menu>
          </template>
        </x-theme>
        "#,
        400.0,
    );
    let panel = find_composed_by_id(&doc.root, "panel").unwrap();
    assert_eq!(
        panel.style.display,
        Display::None,
        ":host(...) rules in a nested shadow tree must style assigned slot content"
    );
}

#[test]
fn shadow_children_see_host_in_ancestor_chain_for_host_selectors() {
    let doc = layout_html(
        r#"
        <x-menu id="menu">
          <template shadowrootmode="open">
            <style>
              :host(:not([loaded], :focus-within)) slot[name=dropdown] {
                display: none;
              }
            </style>
            <slot name="dropdown"></slot>
          </template>
          <div id="panel" slot="dropdown">Panel</div>
        </x-menu>
        "#,
        400.0,
    );
    let slot = find_by_tag(&doc.root, "slot").unwrap();
    assert_eq!(
        slot.style.display,
        Display::None,
        "shadow-root children must match :host(...) descendant selectors against their host"
    );
}

#[test]
fn host_not_selector_matches_shadow_slot_descendant() {
    use crate::css::{AncestorInfo, MatchContext, parse_selector};
    use std::collections::HashSet;

    let selector = parse_selector(":host(:not([loaded], :focus-within)) slot[name=dropdown]");
    assert!(
        selector.valid,
        ":host(...) must be a valid stylesheet selector"
    );
    let mut sheet = crate::css::ua_stylesheet();
    let before = sheet.rules.len();
    sheet.parse_and_add_author(
        ":host(:not([loaded], :focus-within)) slot[name=dropdown] { display: none; }",
    );
    assert!(
        sheet.rules.len() > before,
        "stylesheet parser must keep :host(:not(..., ...)) rules"
    );
    let mut slot = WebCore::new("slot");
    slot.attributes
        .insert("name".to_string(), "dropdown".to_string());
    let host = WebCore::new("x-menu");
    let ancestors = vec![
        AncestorInfo {
            tag: "x-theme".to_string(),
            attributes: Default::default(),
            child_index: 0,
            sibling_count: 1,
            type_child_index: 0,
            type_sibling_count: 1,
            node_id: 1,
        },
        AncestorInfo {
            tag: host.tag.clone(),
            attributes: host.attributes.clone(),
            child_index: 0,
            sibling_count: 1,
            type_child_index: 0,
            type_sibling_count: 1,
            node_id: host.node_id,
        },
    ];
    let empty = HashSet::new();
    let ctx = MatchContext {
        focused_box: 0,
        keyboard_focus: false,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: Some(&slot),
        hover_chain: &empty,
        focus_within_chain: &empty,
        element_id: slot.node_id,
        scope_root_id: 0,
        target_id: 0,
        document_url: "",
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };

    assert!(
        selector.matches_with_ancestors_ctx(&slot, 0, 1, &ancestors, &ctx),
        ":host(:not(...)) descendant selectors must match shadow children against their host"
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

#[test]
fn slotted_custom_element_with_declarative_shadow_does_not_invent_loaded_state() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"
        <style>
          x-menu:not([loaded], :focus-within) [slot=dropdown] { display: none; }
          .panel { display: block; height: 32px; }
        </style>
        <x-menu id="menu">
          <template shadowrootmode="open">
            <slot name="button"></slot>
            <slot name="dropdown"></slot>
          </template>
          <button slot="button">Open</button>
          <div id="panel" class="panel" slot="dropdown">Panel</div>
        </x-menu>
        "#,
        800.0,
    );
    let host = find_by_id(&d.root, "menu").expect("custom shadow host");
    assert!(
        !host.attributes.contains_key("loaded"),
        "declarative shadow DOM must not invent author-observable custom state"
    );
    let projected = find_composed_by_id(&d.root, "panel").expect("projected panel");
    assert!(
        matches!(projected.style.display, crate::types::Display::None),
        "author CSS that gates on [loaded] must see the real attribute state"
    );
}

#[test]
fn projected_button_keeps_document_class_styles_after_shadow_slotting() {
    let mut doc = parse_html(
        r#"
        <style>
          .toolbar { display: flex; }
          .action-button {
            display: flex;
            width: 80px;
            padding: 0 8px;
            border-left-width: 0;
            background: transparent;
          }
          .action-button::before {
            content: "";
            display: block;
            width: 20px;
            height: 20px;
            background: currentColor;
          }
        </style>
        <x-toolbar id="toolbar" class="toolbar">
          <template shadowrootmode="open">
            <slot name="button"></slot>
          </template>
          <button id="action" class="action-button" slot="button">Search</button>
        </x-toolbar>
        "#,
    );
    doc.stylesheet.inspect_mode = true;
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        900.0,
        700.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 700.0;
    eng.layout(&mut doc, 900.0);

    let projected = find_composed_by_id(&doc.root, "action").expect("projected action button");
    assert_eq!(
        projected.style.display,
        crate::types::Display::Flex,
        "projected slotted buttons must receive document class rules, not UA button defaults"
    );
    assert_eq!(projected.style.width, crate::types::CssLength::Px(80.0));
    assert_eq!(
        projected.style.padding_left,
        crate::types::CssLength::Px(8.0)
    );
    assert!(matches!(
        projected.style.border_left_width,
        crate::types::CssLength::Zero | crate::types::CssLength::Px(0.0)
    ));
    assert!(
        projected
            .matched_rules
            .iter()
            .any(|rule| rule.selector == ".action-button"),
        "inspector matched-rule capture should reflect the document class rule on projected nodes"
    );
    assert!(
        projected.style.before_style.is_some(),
        "document pseudo-element rules should also cascade onto projected controls"
    );
}

#[test]
fn nested_shadow_slot_uses_containing_tree_stylesheet_for_projected_light_dom() {
    let mut doc = parse_html(
        r#"
        <outer-widget id="outer">
          <template shadowrootmode="open">
            <style>
              .action-button {
                display: flex;
                width: 80px;
                padding: 0 8px;
                border-left-width: 0;
                background: transparent;
              }
              .action-button::before {
                content: "";
                display: block;
                width: 20px;
                height: 20px;
                background: currentColor;
              }
            </style>
            <inner-dropdown>
              <template shadowrootmode="open">
                <slot name="button"></slot>
              </template>
              <button id="action" class="action-button" slot="button">Search</button>
            </inner-dropdown>
          </template>
        </outer-widget>
        "#,
    );
    doc.stylesheet.inspect_mode = true;
    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        900.0,
        700.0,
        0,
        false,
    );
    let mut eng = LayoutEngine::new();
    eng.viewport_h = 700.0;
    eng.layout(&mut doc, 900.0);

    let projected = find_composed_by_id(&doc.root, "action").expect("nested projected button");
    assert_eq!(
        projected.style.display,
        crate::types::Display::Flex,
        "an inner shadow slot must cascade projected light DOM with the containing shadow tree stylesheet"
    );
    assert_eq!(projected.style.width, crate::types::CssLength::Px(80.0));
    assert_eq!(
        projected.style.padding_left,
        crate::types::CssLength::Px(8.0)
    );
    assert!(
        projected
            .matched_rules
            .iter()
            .any(|rule| rule.selector == ".action-button"),
        "inspector matched rules should use the containing tree stylesheet for nested projected nodes"
    );
    assert!(
        projected.style.before_style.is_some(),
        "pseudo-element rules from the containing tree should cascade onto nested projected controls"
    );
}

#[test]
fn declarative_shadow_link_stylesheet_styles_shadow_tree_only() {
    let mut doc = parse_html(
        r#"
        <style>.shadow-button { width: 11px; background: red; }</style>
        <x-widget>
          <template shadowrootmode="open">
            <link rel="stylesheet" href="shadow.css">
            <button id="inside" class="shadow-button">Inside</button>
          </template>
          <button id="outside" class="shadow-button">Outside</button>
        </x-widget>
        "#,
    );
    let mut linked = crate::css::Stylesheet::default();
    linked.parse_and_add_with_base_media(
        ".shadow-button { display: flex; width: 90px; padding-left: 12px; background: transparent; }",
        "shadow.css",
        "",
    );
    doc.loaded_linked_stylesheets
        .insert("shadow.css".to_string(), linked);
    assert!(
        doc.refresh_shadow_linked_stylesheets(),
        "shadow linked stylesheet should update the shadow CSSOM"
    );

    apply_cascade_vp(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        400.0,
        900.0,
        0,
        false,
    );

    let inside = find_composed_by_id(&doc.root, "inside").unwrap();
    assert_eq!(inside.style.display, Display::Flex);
    assert_eq!(inside.style.width, CssLength::Px(90.0));
    assert_eq!(inside.style.padding_left, CssLength::Px(12.0));

    let outside = find_by_id(&doc.root, "outside").unwrap();
    assert_eq!(outside.style.width, CssLength::Px(11.0));
    assert_ne!(
        outside.style.display,
        Display::Flex,
        "shadow linked stylesheets must not leak back to light DOM"
    );
}

#[test]
fn declarative_shadow_style_with_legacy_shadowroot_attr_styles_component() {
    let doc = layout_html(
        r#"
        <mdn-search-button>
          <template shadowroot="open" shadowrootmode="open">
            <style>
              .mdn-search-button {
                display: flex;
                width: 80px;
                padding-left: 12px;
                background: transparent;
              }
            </style>
            <button id="search" class="mdn-search-button">Search</button>
          </template>
        </mdn-search-button>
        "#,
        400.0,
    );

    let host = find_by_tag(&doc.root, "mdn-search-button").unwrap();
    let shadow = host.shadow_root.as_ref().expect("shadow root");
    assert_eq!(
        shadow.document_stylesheets.len(),
        1,
        "inline shadow <style> must stay owned by the shadow root"
    );
    assert!(
        shadow.stylesheet.rules.len() > crate::css::ua_stylesheet().rules.len(),
        "shadow root stylesheet should include the inline component rules"
    );

    let search = find_composed_by_id(&doc.root, "search").unwrap();
    assert_eq!(search.style.display, Display::Flex);
    assert_eq!(search.style.width, CssLength::Px(80.0));
    assert_eq!(search.style.padding_left, CssLength::Px(12.0));
}
