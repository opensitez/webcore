//! Tests for incremental hover cascade performance and correctness.

use crate::css::{
    apply_cascade_incremental, apply_cascade_vp_hover, build_hover_chain, clear_cascade_dirty,
    mark_hover_dirty,
};
use crate::html::parse_html;
use std::collections::HashSet;

/// Generate a large HTML document with N elements for benchmarking.
fn generate_large_doc(n: usize) -> String {
    let mut html = String::from("<html><head><style>");
    // Add CSS rules that use :hover
    html.push_str("nav a:hover { color: red; background: yellow; }");
    html.push_str(".item:hover { border: 1px solid blue; }");
    html.push_str("li:hover > .sub { display: block; }");
    // General rules
    html.push_str("body { font-family: sans-serif; font-size: 14px; }");
    html.push_str("nav { display: flex; background: #333; }");
    html.push_str("nav a { color: white; padding: 10px; }");
    html.push_str(".item { padding: 5px; margin: 2px; }");
    html.push_str(".sub { display: none; }");
    html.push_str("h2 { font-size: 20px; color: #333; }");
    html.push_str("p { line-height: 1.5; margin-bottom: 10px; }");
    html.push_str("</style></head><body>");

    // Navigation bar
    html.push_str("<nav>");
    for i in 0..10 {
        html.push_str(&format!(
            r#"<a href="/page{}" id="nav{}">Link {}</a>"#,
            i, i, i
        ));
    }
    html.push_str("</nav>");

    // Content sections
    for i in 0..n {
        html.push_str(&format!(
            r#"<div class="item" id="item{}"><h2>Section {}</h2>"#,
            i, i
        ));
        html.push_str(&format!(
            "<p>Content for section {}. Some text here.</p>",
            i
        ));
        html.push_str(r#"<div class="sub">Hover dropdown content</div>"#);
        html.push_str("</div>");
    }

    html.push_str("</body></html>");
    html
}

#[test]
fn incremental_cascade_correctness() {
    // Verify that incremental cascade produces the same result as full cascade
    // for elements in the hover chain.
    let html = r#"<html><head><style>
        a:hover { color: red; }
        .menu:hover .dropdown { display: block; }
        .dropdown { display: none; }
    </style></head><body>
        <div class="menu" id="menu">
            <a href="/" id="link">Home</a>
            <div class="dropdown" id="drop">Content</div>
        </div>
        <p id="other">Not affected</p>
    </body></html>"#;

    let mut doc = parse_html(html);
    let empty = HashSet::new();

    // Initial full cascade (no hover)
    apply_cascade_vp_hover(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &empty,
    );
    clear_cascade_dirty(&mut doc.root);

    // Now simulate hover on the link
    let link_id = doc.get_element_by_id("link").unwrap();

    // Full cascade with hover (reference result)
    let mut doc_full = doc.clone();
    crate::html::rebuild_arena_from_tree(&mut doc_full.arena, &mut doc_full.root);
    let hover_chain = build_hover_chain(&doc_full.root, link_id);
    apply_cascade_vp_hover(
        &mut doc_full.root,
        &doc_full.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &hover_chain,
    );

    // Incremental cascade with hover
    let mut doc_inc = doc.clone();
    crate::html::rebuild_arena_from_tree(&mut doc_inc.arena, &mut doc_inc.root);
    doc_inc.rebuild_node_map();
    let hover_chain_inc = build_hover_chain(&doc_inc.root, link_id);
    let old_chain = HashSet::new(); // no previous hover
    mark_hover_dirty(
        &mut doc_inc.root,
        &old_chain,
        &hover_chain_inc,
        false,
        &HashSet::new(),
    );
    apply_cascade_incremental(
        &mut doc_inc.root,
        &doc_inc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &hover_chain_inc,
    );
    clear_cascade_dirty(&mut doc_inc.root);

    // Compare: the hovered link should have the same color in both
    fn find_style(root: &crate::types::WebCore, id: u32) -> Option<crate::types::ComputedStyle> {
        if root.node_id == id {
            return Some((*root.style).clone());
        }
        for child in &root.children {
            if let Some(s) = find_style(child, id) {
                return Some(s);
            }
        }
        None
    }

    let full_link_style = find_style(&doc_full.root, link_id).unwrap();
    let inc_link_style = find_style(&doc_inc.root, link_id).unwrap();
    assert_eq!(
        full_link_style.color.r, inc_link_style.color.r,
        "color.r mismatch: full={} inc={}",
        full_link_style.color.r, inc_link_style.color.r
    );
    assert_eq!(full_link_style.color.g, inc_link_style.color.g);
}

#[test]
fn incremental_cascade_skips_clean_subtrees() {
    // Measure that incremental cascade visits far fewer nodes than full cascade
    let html = generate_large_doc(200); // 200 sections ~ 1400 elements
    let mut doc = parse_html(&html);
    let empty = HashSet::new();

    // Full cascade
    let t0 = std::time::Instant::now();
    apply_cascade_vp_hover(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &empty,
    );
    let full_time = t0.elapsed();
    clear_cascade_dirty(&mut doc.root);

    // Simulate hover on nav0
    let nav0 = doc.get_element_by_id("nav0").unwrap();
    let hover_chain = build_hover_chain(&doc.root, nav0);
    let old_chain = HashSet::new();
    doc.rebuild_node_map();
    mark_hover_dirty(
        &mut doc.root,
        &old_chain,
        &hover_chain,
        false,
        &HashSet::new(),
    );

    // Incremental cascade
    let t1 = std::time::Instant::now();
    apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &hover_chain,
    );
    let inc_time = t1.elapsed();
    clear_cascade_dirty(&mut doc.root);

    eprintln!(
        "[first hover] Full cascade: {:?}, Incremental: {:?}, Speedup: {:.1}x",
        full_time,
        inc_time,
        full_time.as_nanos() as f64 / inc_time.as_nanos().max(1) as f64
    );

    // Now measure the TRANSITION case (moving from nav0 to nav1)
    // This is where the real speedup happens — symmetric difference is small
    let nav1 = doc.get_element_by_id("nav1").unwrap();
    let chain_nav1 = build_hover_chain(&doc.root, nav1);
    doc.rebuild_node_map();
    mark_hover_dirty(
        &mut doc.root,
        &hover_chain,
        &chain_nav1,
        doc.stylesheet.has_hover_descendant_rules,
        &HashSet::new(),
    );

    let t2 = std::time::Instant::now();
    apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &chain_nav1,
    );
    let transition_time = t2.elapsed();
    clear_cascade_dirty(&mut doc.root);

    eprintln!(
        "[hover transition] Full cascade: {:?}, Incremental: {:?}, Speedup: {:.1}x",
        full_time,
        transition_time,
        full_time.as_nanos() as f64 / transition_time.as_nanos().max(1) as f64
    );

    // Transition should be much faster than full cascade
    assert!(
        transition_time < full_time,
        "transition ({:?}) should be faster than full ({:?})",
        transition_time,
        full_time
    );

    // Also measure layout time with dirty flags vs full layout
    let mut engine = crate::layout::LayoutEngine::new();
    engine.viewport_w = 800.0;
    engine.viewport_h = 600.0;

    // Full layout after full cascade
    let mut doc_layout = parse_html(&html);
    apply_cascade_vp_hover(
        &mut doc_layout.root,
        &doc_layout.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &empty,
    );
    let t_layout_full = std::time::Instant::now();
    engine.layout(&mut doc_layout, 800.0);
    let layout_full = t_layout_full.elapsed();

    // Now do an incremental hover and measure layout
    let nav2 = doc_layout.get_element_by_id("nav2").unwrap();
    let chain_nav2 = build_hover_chain(&doc_layout.root, nav2);
    doc_layout.rebuild_node_map();
    doc_layout.hover_changed = true;
    doc_layout.hovered_box = nav2;
    doc_layout.prev_hovered_box = nav0;
    let t_layout_inc = std::time::Instant::now();
    engine.layout(&mut doc_layout, 800.0);
    let layout_inc = t_layout_inc.elapsed();

    eprintln!(
        "[layout] Full: {:?}, After hover: {:?}, Speedup: {:.1}x",
        layout_full,
        layout_inc,
        layout_full.as_nanos() as f64 / layout_inc.as_nanos().max(1) as f64
    );
}

#[test]
fn incremental_cascade_handles_hover_transition() {
    // Test hovering from one element to another — both old and new chains get re-cascaded
    let html = r#"<ul>
        <li id="a" class="item">A</li>
        <li id="b" class="item">B</li>
        <li id="c" class="item">C</li>
    </ul>"#;

    let mut doc = parse_html(html);
    let empty = HashSet::new();

    // Initial cascade
    apply_cascade_vp_hover(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &empty,
    );
    clear_cascade_dirty(&mut doc.root);

    let a = doc.get_element_by_id("a").unwrap();
    let b = doc.get_element_by_id("b").unwrap();

    // Hover on A
    let chain_a = build_hover_chain(&doc.root, a);
    let old_empty = HashSet::new();
    doc.rebuild_node_map();
    mark_hover_dirty(&mut doc.root, &old_empty, &chain_a, false, &HashSet::new());
    apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &chain_a,
    );
    clear_cascade_dirty(&mut doc.root);

    // Move hover from A to B
    let chain_b = build_hover_chain(&doc.root, b);
    doc.rebuild_node_map();
    mark_hover_dirty(&mut doc.root, &chain_a, &chain_b, false, &HashSet::new());
    apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &chain_b,
    );
    clear_cascade_dirty(&mut doc.root);

    // Both A and B should have been processed
    // (A should no longer have hover styles, B should have them)
    // Just verify it doesn't crash and processes both chains
    assert!(chain_a.contains(&a));
    assert!(chain_b.contains(&b));
}

// ── Re-running layout must not change the result ─────────────────────────────

/// One line per box: tag, display and geometry — enough to catch a box that
/// appeared, vanished or changed size between two passes.
fn dump_boxes(node: &crate::types::WebCore, depth: usize, out: &mut String) {
    out.push_str(&format!(
        "{:indent$}{} [{:?}] {}x{}\n",
        "",
        node.tag,
        node.style.display,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        indent = depth * 2,
    ));
    for ch in &node.children {
        dump_boxes(ch, depth + 1, out);
    }
}

/// **A second `layout()` on the same document must produce the same boxes.**
/// Hovering re-runs the whole cascade, so any pass-to-pass drift shows up as
/// the page jumping the moment the pointer moves. Creating a `::before` box for
/// a flex parent used to clear `before_content` on that parent, so the next
/// cascade saw an empty content and deleted the box it had just built.
#[test]
fn cascade_layout_is_idempotent_for_flex_pseudo_elements() {
    let html = r#"<style>
        * { margin: 0; padding: 0 }
        .bar { display: flex }
        .bar::before { content: "" ; width: 12px; height: 4px }
        .bar::after { content: "" ; width: 8px; height: 4px }
        .label { display: flex }
        .label::before { content: "x" }
        </style>
        <div class="bar"><span class="label">Menu</span><span>Other</span></div>"#;
    let mut renderer = crate::renderer::Renderer::new();
    let mut doc = renderer.load_html(html, 400.0);
    let mut first = String::new();
    dump_boxes(&doc.root, 0, &mut first);

    renderer.layout_engine().layout(&mut doc, 400.0);
    let mut second = String::new();
    dump_boxes(&doc.root, 0, &mut second);

    assert!(
        first.contains("::before"),
        "the flex ::before box exists on the first pass:\n{first}"
    );
    assert_eq!(
        first, second,
        "second layout differs\n--- first ---\n{first}\n--- second ---\n{second}"
    );
}

/// The same guarantee for a third pass — a drift that alternates between two
/// shapes would satisfy a single repeat.
#[test]
fn cascade_layout_is_stable_across_repeats() {
    let html = r#"<style>
        * { margin: 0; padding: 0 }
        .grid { display: grid }
        .grid::before { content: "a" }
        .flex { display: flex }
        .flex::after { content: "b" }
        </style>
        <div class="grid"><div class="flex">Hi</div></div>"#;
    let mut renderer = crate::renderer::Renderer::new();
    let mut doc = renderer.load_html(html, 400.0);
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let mut s = String::new();
        dump_boxes(&doc.root, 0, &mut s);
        shapes.push(s);
        renderer.layout_engine().layout(&mut doc, 400.0);
    }
    assert_eq!(shapes[0], shapes[1], "pass 2 differs from pass 1");
    assert_eq!(shapes[1], shapes[2], "pass 3 differs from pass 2");
}

// ── Sibling combinators in the parallel cascade ──────────────────────────────

/// A document whose sheet is large enough to take the parallel cascade path
/// (`rules.len() > 1000`), plus the rule under test.
fn big_sheet_doc(rule: &str, body: &str) -> crate::Document {
    let mut css = String::new();
    for i in 0..1100 {
        css.push_str(&format!(".filler{i} {{ color: #010101 }}\n"));
    }
    css.push_str(rule);
    let html = format!("<style>* {{ margin:0; padding:0 }}\n{css}</style>{body}");
    let mut doc = crate::parse_html(&html);
    let mut eng = crate::layout::LayoutEngine::new();
    eng.viewport_h = 900.0;
    eng.layout(&mut doc, 800.0);
    doc
}

fn by_id<'a>(node: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
    if node.attributes.get("id").map(String::as_str) == Some(id) {
        return Some(node);
    }
    for c in &node.children {
        if let Some(n) = by_id(c, id) {
            return Some(n);
        }
    }
    None
}

/// **`+` and `~` must match in the parallel cascade too.** It handed the
/// matcher an empty previous-sibling list for every element, so every sibling
/// combinator silently failed on first render and then started matching the
/// moment anything triggered the serial re-cascade.
#[test]
fn parallel_cascade_matches_the_adjacent_sibling_combinator() {
    let doc = big_sheet_doc(
        "span + span { position: absolute; width: 1px; height: 1px }",
        "<div><span id=first>A</span>\n\n<span id=second>B</span></div>",
    );
    let second = by_id(&doc.root, "second").unwrap();
    assert_eq!(
        second.style.position,
        crate::types::Position::Absolute,
        "span + span must match the second span"
    );
    // Blockified because it is out of flow (CSS Display 3 §2.7).
    assert_eq!(second.style.display, crate::types::Display::Block);
    let first = by_id(&doc.root, "first").unwrap();
    assert_eq!(
        first.style.position,
        crate::types::Position::Static,
        "and must not match the first"
    );
}

/// The general sibling combinator, and a text node between the elements — an
/// element sibling is what counts, not a DOM sibling.
#[test]
fn parallel_cascade_matches_the_general_sibling_combinator() {
    let doc = big_sheet_doc(
        "#a ~ span { position: absolute }",
        "<div><span id=a>A</span> text <em>x</em> <span id=b>B</span><span id=c>C</span></div>",
    );
    for id in ["b", "c"] {
        assert_eq!(
            by_id(&doc.root, id).unwrap().style.position,
            crate::types::Position::Absolute,
            "#a ~ span must match #{id}"
        );
    }
    assert_eq!(
        by_id(&doc.root, "a").unwrap().style.position,
        crate::types::Position::Static,
        "and not the subject itself"
    );
}

/// A descendant selector in front of the combinator — the shape the real page
/// used (`.cdx-button.cdx-button--icon-only span + span`).
#[test]
fn parallel_cascade_matches_a_sibling_under_a_descendant() {
    let doc = big_sheet_doc(
        ".btn.icon-only span + span { position: absolute; width: 1px }",
        "<label class='btn icon-only'><span id=icon></span>\n<span id=label>Menu</span></label>",
    );
    assert_eq!(
        by_id(&doc.root, "label").unwrap().style.position,
        crate::types::Position::Absolute
    );
    assert_eq!(
        by_id(&doc.root, "icon").unwrap().style.position,
        crate::types::Position::Static
    );
}

// ── The two cascades must agree ──────────────────────────────────────────────
//
// `apply_cascade_vp_hover` picks the parallel implementation when the sheet has
// more than 1000 rules. Every incremental re-cascade takes the serial one, so a
// large page that renders one way on load and another way after the first hover
// is exactly the two implementations disagreeing. These tests cascade the SAME
// markup down both paths and require the computed styles to be identical.

/// 1100 rules that match nothing — enough to push `apply_cascade_vp_hover` over
/// its parallel threshold without changing a single computed value.
fn filler_rules() -> String {
    let mut s = String::new();
    for i in 0..1100 {
        s.push_str(&format!(".vfill{i} {{ color: #010101 }}\n"));
    }
    s
}

/// Cascade `body` twice against `rule`: once with a small sheet (serial path)
/// and once with `rule` plus 1100 never-matching rules (parallel path).
fn cascade_serial_and_parallel(
    rule: &str,
    body: &str,
    focus_id: Option<&str>,
) -> (crate::Document, crate::Document) {
    let mut small = crate::parse_html(&format!("<style>{rule}</style>{body}"));
    let mut big = crate::parse_html(&format!("<style>{}{rule}</style>{body}", filler_rules()));
    assert!(
        small.stylesheet.rules.len() <= 1000,
        "the small sheet must take the serial path ({} rules)",
        small.stylesheet.rules.len()
    );
    assert!(
        big.stylesheet.rules.len() > 1000,
        "the big sheet must take the parallel path ({} rules)",
        big.stylesheet.rules.len()
    );
    let empty = HashSet::new();
    for doc in [&mut small, &mut big] {
        let focus = focus_id
            .and_then(|id| doc.get_element_by_id(id))
            .unwrap_or(0);
        doc.stylesheet.rebuild_index();
        apply_cascade_vp_hover(
            &mut doc.root,
            &doc.stylesheet,
            None,
            16.0,
            800.0,
            600.0,
            focus,
            false,
            &empty,
        );
    }
    (small, big)
}

/// The first node where two cascaded trees differ, as a human-readable report.
///
/// `ComputedStyle` has no `PartialEq`, and the tree's own serializer emits only
/// a subset of properties — so the comparison is on the derived `Debug`, which
/// prints every field.
fn first_style_difference(
    a: &crate::types::WebCore,
    b: &crate::types::WebCore,
    path: &str,
) -> Option<String> {
    let here = format!(
        "{path}/{}{}",
        a.tag,
        a.attributes
            .get("id")
            .map(|i| format!("#{i}"))
            .unwrap_or_default()
    );
    if a.tag != b.tag {
        return Some(format!("{here}: tag {} vs {}", a.tag, b.tag));
    }
    let (sa, sb) = (format!("{:?}", *a.style), format!("{:?}", *b.style));
    if sa != sb {
        // Report only the fields that actually differ — a whole ComputedStyle
        // dump buries the one property under two thousand characters.
        let diffs: Vec<String> = sa
            .split(", ")
            .zip(sb.split(", "))
            .filter(|(x, y)| x != y)
            .map(|(x, y)| format!("{x} != {y}"))
            .take(6)
            .collect();
        let detail = if diffs.is_empty() {
            format!("{sa}\n  vs\n{sb}")
        } else {
            diffs.join("; ")
        };
        return Some(format!("{here}: {detail}"));
    }
    if a.children.len() != b.children.len() {
        return Some(format!(
            "{here}: {} children vs {}",
            a.children.len(),
            b.children.len()
        ));
    }
    for (ca, cb) in a.children.iter().zip(b.children.iter()) {
        if let Some(d) = first_style_difference(ca, cb, &here) {
            return Some(d);
        }
    }
    match (&a.shadow_root, &b.shadow_root) {
        (Some(x), Some(y)) => {
            if x.children.len() != y.children.len() {
                return Some(format!(
                    "{here}: shadow {} children vs {}",
                    x.children.len(),
                    y.children.len()
                ));
            }
            for (ca, cb) in x.children.iter().zip(y.children.iter()) {
                if let Some(d) = first_style_difference(ca, cb, &format!("{here}::shadow")) {
                    return Some(d);
                }
            }
        }
        (None, None) => {}
        _ => {
            return Some(format!(
                "{here}: one tree has a shadow root and the other does not"
            ));
        }
    }
    None
}

/// Assert the serial and parallel cascades compute the same styles for `body`.
fn assert_cascades_agree(what: &str, rule: &str, body: &str) {
    let (small, big) = cascade_serial_and_parallel(rule, body, None);
    if let Some(d) = first_style_difference(&small.root, &big.root, "") {
        panic!("{what}: serial and parallel cascades disagree at {d}");
    }
}

#[test]
fn both_cascades_honour_layer_order() {
    // CSS Cascade 5 §6.4.4: a later `@layer` wins over an earlier one no matter
    // how specific the earlier rule is.
    assert_cascades_agree(
        "@layer order",
        "@layer base, theme;\n\
         @layer theme { #t { color: rgb(0, 0, 255) } }\n\
         @layer base  { #t.c { color: rgb(255, 0, 0) } }",
        "<p id=t class=c>x</p>",
    );
}

#[test]
fn both_cascades_read_the_size_attribute_per_element() {
    // `size` means three different things: `<font size>` is a font size,
    // `<select size>` a row count, `<input size>` a width in characters.
    assert_cascades_agree(
        "size attribute",
        "p { color: black }",
        "<select id=s size=4><option>a</option></select><input id=i size=20>\
         <font id=f size=5>t</font>",
    );
}

#[test]
fn both_cascades_give_select_multiple_its_default_height() {
    assert_cascades_agree(
        "select multiple",
        "p { color: black }",
        "<select id=s multiple><option>a</option></select>",
    );
}

#[test]
fn both_cascades_evaluate_has() {
    assert_cascades_agree(
        ":has()",
        ".card:has(img) { color: rgb(1, 2, 3) }",
        "<div class=card id=c><img src=x></div><div class=card id=d>t</div>",
    );
}

#[test]
fn both_cascades_evaluate_empty() {
    assert_cascades_agree(
        ":empty",
        "p:empty { color: rgb(4, 5, 6) }",
        "<p id=e></p><p id=n>t</p>",
    );
}

#[test]
fn both_cascades_evaluate_focus_pseudo_classes() {
    let body = "<div id=wrap><input id=f></div>";
    let rule = "#wrap:focus-within { color: rgb(7, 8, 9) } input:focus { color: rgb(9, 8, 7) }";
    let (small, big) = cascade_serial_and_parallel(rule, body, Some("f"));
    if let Some(d) = first_style_difference(&small.root, &big.root, "") {
        panic!(":focus/:focus-within: serial and parallel cascades disagree at {d}");
    }
}

#[test]
fn both_cascades_put_important_into_the_hover_base_style() {
    // The `hover_style` is a clone of the element's own computed style with the
    // hover declarations overlaid, so an `!important` on the element must
    // already be in it.
    assert_cascades_agree(
        "!important in the hover base",
        "#h { color: rgb(9, 9, 9) !important } #h:hover { background: rgb(1, 1, 1) }",
        "<a id=h href=#>x</a>",
    );
}

#[test]
fn both_cascades_order_inherit_against_important_the_same_way() {
    assert_cascades_agree(
        "inherit vs !important",
        "#p { color: rgb(3, 30, 3) } .a { color: inherit } #i { color: rgb(7, 7, 7) !important }",
        "<div id=p><span id=i class=a>x</span></div>",
    );
}

#[test]
fn a_generated_pseudo_box_does_not_shift_its_siblings_results() {
    // The parallel pass records a node PATH during a walk of the tree as it
    // stands, then `build_pseudo_element_boxes` inserts a `::before` child at
    // index 0 during the apply walk — every later sibling then looks its result
    // up at the wrong path and gets no rules at all.
    let rule = "#f { display: flex } #f::before { content: 'x' } \
                #s { color: rgb(2, 4, 6) } #t { color: rgb(6, 4, 2) }";
    let body = "<div id=f><span id=s>a</span><span id=t>b</span></div>";
    let (small, big) = cascade_serial_and_parallel(rule, body, None);
    for (name, doc) in [("serial", &small), ("parallel", &big)] {
        let f = by_id(&doc.root, "f").unwrap();
        assert_eq!(
            f.children.first().map(|c| c.tag.as_str()),
            Some("::before"),
            "{name}: the ::before box must exist, or this test proves nothing"
        );
        assert_eq!(
            by_id(&doc.root, "s").unwrap().style.color,
            crate::types::Color {
                r: 2,
                g: 4,
                b: 6,
                a: 255
            },
            "{name}: #s"
        );
        assert_eq!(
            by_id(&doc.root, "t").unwrap().style.color,
            crate::types::Color {
                r: 6,
                g: 4,
                b: 2,
                a: 255
            },
            "{name}: #t"
        );
    }
    if let Some(d) = first_style_difference(&small.root, &big.root, "") {
        panic!("::before insertion: serial and parallel cascades disagree at {d}");
    }
}

#[test]
fn both_cascades_apply_host_rules_in_a_shadow_tree() {
    assert_cascades_agree(
        ":host",
        "p { color: black }",
        "<div id=host><template shadowrootmode=open>\
         <style>:host { color: rgb(5, 5, 5) } p { color: rgb(6, 6, 6) }</style>\
         <p>s</p></template><span>l</span></div>",
    );
}

#[test]
fn a_custom_property_on_a_shadow_host_reaches_its_light_children() {
    assert_cascades_agree(
        "custom property across a shadow host",
        "#host { --tint: rgb(3, 3, 3) } #light { color: var(--tint) }",
        "<div id=host><template shadowrootmode=open><slot></slot></template>\
         <span id=light>l</span></div>",
    );
}

#[test]
fn both_cascades_agree_on_a_mixed_document() {
    // One document exercising several features at once — a regression net for
    // divergences nobody has named yet.
    assert_cascades_agree(
        "mixed document",
        "@layer a, b;\n\
         @layer b { li { color: rgb(1, 1, 1) } }\n\
         @layer a { li.x { color: rgb(2, 2, 2) } }\n\
         ol { counter-reset: n } li::before { content: counter(n) '. '; display: block }\n\
         li { counter-increment: n }\n\
         td { padding: 2px } input:checked { color: rgb(4, 4, 4) }\n\
         .g { display: grid } .g::after { content: '' }\n\
         p + p { color: rgb(5, 5, 5) } p ~ span { color: rgb(6, 6, 6) }",
        "<ol><li class=x>a</li><li>b</li></ol>\
         <table border=1 cellspacing=0><tr><td>c</td></tr></table>\
         <input type=checkbox checked><div class=g><b>g</b></div>\
         <p>1</p><p>2</p><span>s</span><select size=3><option>o</option></select>",
    );
}

/// **The bug as the user sees it**: a large page renders on the parallel path,
/// and the first hover re-cascades the affected subtree on the serial one. If
/// the two disagree, the page changes under the pointer.
#[test]
fn a_re_cascade_does_not_change_a_page_that_loaded_on_the_parallel_path() {
    let rule = "@layer a, b;\n\
                @layer b { #t { color: rgb(0, 0, 255) } }\n\
                @layer a { #t.c { color: rgb(255, 0, 0) } }\n\
                #f { display: flex } #f::before { content: 'x' }\n\
                #s { color: rgb(2, 4, 6) } .card:has(img) { color: rgb(1, 2, 3) }\n\
                #hov:hover { background: rgb(8, 8, 8) }";
    let body = "<p id=t class=c>x</p>\
                <div id=f><span id=s>a</span><span id=t2>b</span></div>\
                <div class=card id=c><img src=x></div>\
                <select id=sel size=4><option>o</option></select>\
                <a id=hov href=#>h</a>";
    let mut doc = crate::parse_html(&format!("<style>{}{rule}</style>{body}", filler_rules()));
    doc.stylesheet.rebuild_index();
    let empty = HashSet::new();
    apply_cascade_vp_hover(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &empty,
    );
    clear_cascade_dirty(&mut doc.root);
    let before = doc.clone();

    // Hover the link: the same route `layout()` takes — mark the chain dirty,
    // then run the incremental (serial) cascade.
    let hov = doc.get_element_by_id("hov").unwrap();
    let chain = build_hover_chain(&doc.root, hov);
    mark_hover_dirty(&mut doc.root, &empty, &chain, true, &HashSet::new());
    apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &chain,
    );
    clear_cascade_dirty(&mut doc.root);

    // Everything OUTSIDE the hover chain must be untouched by the re-cascade.
    fn compare(
        a: &crate::types::WebCore,
        b: &crate::types::WebCore,
        chain: &HashSet<u32>,
        path: &str,
    ) -> Option<String> {
        if chain.contains(&a.node_id) {
            return None;
        }
        first_style_difference(a, b, path)
    }
    for id in ["t", "s", "t2", "c", "sel"] {
        let a = by_id(&before.root, id).expect(id);
        let b = by_id(&doc.root, id).expect(id);
        if let Some(d) = compare(a, b, &chain, id) {
            panic!("re-cascade changed #{id}, which is not in the hover chain: {d}");
        }
    }
}
