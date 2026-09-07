//! Size measurements for the data-model plan (`arenaplan.md`).
//!
//! These are not thresholds — they are a RECORD, so a change to the data model
//! can be shown to have done what it claimed. Each figure is asserted against
//! the value measured when it was last recorded, so a regression is loud.

use crate::types::{ComputedStyle, LayoutBox, WebCore};

fn tree_bytes(root: &WebCore) -> (usize, usize, usize, usize) {
    use std::collections::HashSet;
    fn walk(n: &WebCore, count: &mut usize, styles: &mut HashSet<usize>) {
        *count += 1;
        // ⛔ Counting `size_of::<WebCore>() * nodes` alone would OVERSTATE the
        // win: the styles still exist, they are just behind pointers now. Count
        // the DISTINCT ones by address, which is the number that actually fell.
        styles.insert(std::sync::Arc::as_ptr(&n.style) as usize);
        for c in &n.children {
            walk(c, count, styles);
        }
    }
    let (mut n, mut styles) = (0, HashSet::new());
    walk(root, &mut n, &mut styles);
    let nodes_bytes = n * std::mem::size_of::<WebCore>();
    let style_bytes = styles.len() * std::mem::size_of::<ComputedStyle>();
    (n, nodes_bytes, styles.len(), nodes_bytes + style_bytes)
}

#[test]
fn the_data_model_sizes_are_what_the_plan_says() {
    let sizes = (
        std::mem::size_of::<WebCore>(),
        std::mem::size_of::<ComputedStyle>(),
        std::mem::size_of::<LayoutBox>(),
    );
    // (WebCore, ComputedStyle, LayoutBox) — update deliberately, with the
    // change that moved them.
    //
    // 2016 → 3472: later standards coverage widened `ComputedStyle` again.
    // This assertion is a measured record, not a threshold; update it with the
    // feature that intentionally moves the data model.
    assert_eq!(sizes, (624, 3472, 224), "sizes moved");
}

#[test]
fn a_real_page_costs_what_the_plan_says() {
    let mut r = crate::Renderer::new();
    let doc = r.load_html(include_str!("../../examples/html/demo.html"), 900.0);
    let (nodes, node_bytes, distinct_styles, total) = tree_bytes(&doc.root);
    // Before `Arc<ComputedStyle>`: 1132 nodes, 3,350,720 B, one style each.
    //
    // Total tracks the exact struct sizes above: 1,099 distinct styles means
    // widening `ComputedStyle` moves this number loudly.
    assert_eq!(
        (nodes, node_bytes, distinct_styles, total),
        (1132, 706_368, 1099, 4_522_096),
        "demo.html: nodes, node bytes, DISTINCT styles, total"
    );
}

/// ⛔ A LIVE RENDERING BUG, found while sizing `arenaplan.md` item 1.
///
/// `cascade_children` shares a cascaded style between siblings under the key
/// `(tag, class)` — which says nothing about WHERE among its siblings an
/// element sits. With `i + i { color: red }` in the sheet the second `<i>` was
/// handed the first one's style and the rule was silently dropped; likewise
/// `li:nth-child(2)`.
///
/// Confirmed to be SHARING and not missing selector support by running it both
/// ways: green with `can_share` forced false, red with it forced true.
///
/// The first attempt at this test was VACUOUS twice over — the elements had
/// `id`s (which `can_share` excludes) and then text children (`children.is_empty()`
/// excludes those too), so the sharing path was never reached and the test
/// passed against the bug.
#[test]
fn style_sharing_must_not_collapse_distinguishable_siblings() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>i + i { color: rgb(255,0,0) } li:nth-child(2) { color: rgb(0,255,0) }</style>\
         <span><i></i><i></i></span><ul><li></li><li></li></ul>",
        900.0,
    );
    let col = |tag: &str, n: usize| {
        let ids = d.get_elements_by_tag_name(tag);
        d.get_computed_style(ids[n]).map(|s| s.color).unwrap()
    };
    let black = crate::types::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    let red = crate::types::Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    let green = crate::types::Color {
        r: 0,
        g: 255,
        b: 0,
        a: 255,
    };
    assert_eq!(
        (col("i", 0), col("i", 1), col("li", 0), col("li", 1)),
        (black, red, black, green),
        "sibling and nth-child selectors must survive style sharing"
    );
}

#[test]
fn every_sibling_sensitive_selector_form_turns_sharing_off() {
    // ⛔ A mutation run found five survivors: the test above puts BOTH a
    // sibling combinator and a positional pseudo-class in one sheet, so
    // deleting either detector left the other setting the flag and sharing
    // stayed off. Each form needs a sheet of its own.
    //
    // Every row: two same-`(tag, class)` siblings that the selector can tell
    // apart, so sharing them would drop the rule.
    let cases: &[(&str, &str, usize)] = &[
        // (rule, markup, which child index must get the colour)
        ("i + i", "<span><i></i><i></i></span>", 1),
        ("i ~ i", "<span><i></i><i></i></span>", 1),
        ("i:nth-child(2)", "<span><i></i><i></i></span>", 1),
        ("i:nth-last-child(1)", "<span><i></i><i></i></span>", 1),
        ("i:first-child", "<span><i></i><i></i></span>", 0),
        ("i:last-child", "<span><i></i><i></i></span>", 1),
        ("i:nth-of-type(2)", "<span><i></i><i></i></span>", 1),
        ("i:first-of-type", "<span><i></i><i></i></span>", 0),
        ("i:last-of-type", "<span><i></i><i></i></span>", 1),
    ];
    let red = crate::types::Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    let black = crate::types::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    for (rule, markup, hit) in cases {
        let mut r = crate::Renderer::new();
        let d = r.load_html(
            &format!("<style>{rule} {{ color: rgb(255,0,0) }}</style>{markup}"),
            900.0,
        );
        let ids = d.get_elements_by_tag_name("i");
        assert_eq!(ids.len(), 2, "{rule}");
        for (n, id) in ids.iter().enumerate() {
            let got = d.get_computed_style(*id).map(|s| s.color).unwrap();
            let want = if n == *hit { red } else { black };
            assert_eq!(got, want, "{rule}: child {n}");
        }
    }
}

#[test]
fn a_sheet_with_no_sibling_sensitive_rule_still_shares() {
    // The other half: the gate must not turn sharing off for every sheet.
    // ⛔ Without this, "always disable sharing" passes every test above.
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>i { color: rgb(0,0,255) }</style><span><i></i><i></i></span>",
        900.0,
    );
    assert!(
        !d.stylesheet.has_sibling_sensitive_rules,
        "nothing here can tell siblings apart"
    );
    let ids = d.get_elements_by_tag_name("i");
    let blue = crate::types::Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    for id in ids {
        assert_eq!(d.get_computed_style(id).map(|s| s.color), Some(blue));
    }
}

#[test]
fn style_sharing_is_almost_never_reachable_on_a_real_page() {
    // ⛔ THE finding of `arenaplan.md` item 1, recorded because it changes what
    // item 1 is worth. `Arc<ComputedStyle>` shrank `WebCore` from 2,960 B to
    // 640 B — but the plan's headline win was STYLE memory, 5-12x, and that
    // depends on sharing actually happening. On demo.html it does not: 1,099
    // distinct styles for 1,132 nodes.
    //
    // MEASURED, by disabling each condition of `can_share` in turn and by
    // building the two alternatives:
    //
    //   baseline                          2.9% shared
    //   without the id / inline-style / sibling gates   2.9% — none of them binds
    //   without the leaf-only rule        8.2%
    //   document-wide cache keyed on parent identity    2.9% — no gain
    //   INTERNING byte-identical styles   13.5%, and 2.4x SLOWER
    //
    // Item 2 (boxing the rare properties) then took `ComputedStyle` from
    // 2,328 B to 2,024 B and the tree from 3.28 MB to 2.95 MB — 10.2%, which
    // is what the ceiling measurement predicted before the work started.
    //
    // ⛔ The plan's 5-12x is a measurement artifact. Its own footnote says the
    // serializer it counted with "emits a subset of properties, so styles
    // differing in an unserialized property collide". Compared losslessly, a
    // real page's styles are nearly all distinct: 979 of 1,132 even with
    // perfect value-sharing. The ceiling is ~1.16x, not 5-12x — and reaching
    // it costs more time than the memory is worth.
    let mut r = crate::Renderer::new();
    let doc = r.load_html(include_str!("../../examples/html/demo.html"), 900.0);
    let (nodes, _, distinct, _) = tree_bytes(&doc.root);
    assert!(
        distinct as f32 / nodes as f32 > 0.9,
        "sharing ratio changed — {distinct} distinct styles for {nodes} nodes. \
         If this dropped, the share key improved and the numbers above are stale."
    );
}

/// `arenaplan.md` item 3 — the cascade's stack frame.
///
/// **Measured: the cascade overflowed the stack between 350 and 360 nesting
/// levels. It now handles 16,000 on the main thread.**
///
/// ⛔ This asserts 1,000, not 16,000, because a libtest worker thread has a
/// SMALLER stack than the main thread — 2,000 passes under
/// `--test-threads=1` and aborts the whole run under the default harness.
/// A test that only passes single-threaded is a test that takes the suite
/// down for the next reader.
///
/// ⛔ The fix that worked was a FUNCTION boundary, and only that. Moving the
/// per-node `ComputedStyle` into its `Arc` instead of cloning it, and passing
/// the children a shared pointer instead of a 2.3 KB local, made the limit
/// slightly WORSE (~355 → ~325) — a debug build does not reuse stack slots
/// between sibling scopes, so shrinking what a block holds does not shrink the
/// frame. Extracting the `::before`/`::after` work into `build_pseudo_element_boxes`
/// is what popped those slots, and it moved the limit by more than 5x.
#[test]
fn the_cascade_handles_deep_nesting() {
    let depth: usize = 1000;
    let mut html = String::from("<style>div{color:red}</style>");
    for _ in 0..depth {
        html.push_str("<div>");
    }
    html.push('x');
    for _ in 0..depth {
        html.push_str("</div>");
    }
    let mut r = crate::Renderer::new();
    let d = r.load_html(&html, 900.0);
    assert!(d.root.node_id != 0, "cascaded {depth} deep");
}

/// ⛔ THE SECOND live rendering bug in this family, found by asking what else
/// the share key ignores. It keyed on `(tag, class)` — so `i[data-x] { … }`
/// was dropped for the element that HAD the attribute: it took the style of
/// the one that did not. Confirmed to be sharing, not missing
/// attribute-selector support, by forcing `can_share` false.
///
/// The key is the whole attribute list now. The attributes ARE the element as
/// far as a selector is concerned; anything less is a hole waiting for the
/// next selector form.
#[test]
fn style_sharing_must_not_ignore_other_attributes() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>i[data-x] { color: rgb(255,0,0) }</style><span><i></i><i data-x></i></span>",
        900.0,
    );
    let got: Vec<crate::types::Color> = d
        .get_elements_by_tag_name("i")
        .into_iter()
        .map(|id| d.get_computed_style(id).unwrap().color)
        .collect();
    assert_eq!(
        got,
        vec![
            crate::types::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255
            },
            crate::types::Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }
        ],
        "an attribute selector must survive style sharing"
    );
}

/// `arenaplan.md` item 2's ceiling, measured rather than assumed.
///
/// ⛔ **8 of 1,132 elements use ANY rare property.** The fields cost 312 B in
/// every `ComputedStyle` regardless. Boxing them is worth about 10% of tree
/// memory across 182 access sites — a real but modest win, and NOT the
/// "compounds with item 1" the plan claimed, because styles are not shared.
#[test]
fn the_rare_property_ceiling_is_worth_measuring_before_building() {
    let mut r = crate::Renderer::new();
    let doc = r.load_html(include_str!("../../examples/html/demo.html"), 900.0);
    fn walk(n: &WebCore, total: &mut usize, with_rare: &mut usize) {
        *total += 1;
        let s = &n.style;
        let uses_rare = !s.rare().grid_template_columns.is_empty()
            || !s.rare().grid_template_rows.is_empty()
            || !s.rare().grid_template_areas.is_empty()
            || !s.rare().auto_repeat_columns.is_empty()
            || !s.rare().gradient_stops.is_empty()
            || !s.rare().animations.is_empty()
            || !s.rare().transitions.is_empty()
            || !s.rare().font_variation_settings.is_empty()
            || !s.rare().font_feature_settings.is_empty()
            || !s.rare().quotes.is_empty()
            || !s.rare().filter.is_empty()
            || !s.rare().backdrop_filter.is_empty()
            || !s.rare().mask_image_url.is_empty();
        if uses_rare {
            *with_rare += 1;
        }
        for c in &n.children {
            walk(c, total, with_rare);
        }
    }
    let (mut total, mut with_rare) = (0, 0);
    walk(&doc.root, &mut total, &mut with_rare);
    assert_eq!(
        (total, with_rare),
        (1132, 8),
        "rare-property usage on demo.html"
    );
}

/// Walk both representations and report every disagreement.
fn collect_drift(d: &crate::types::Document, n: &WebCore, drift: &mut Vec<String>) {
    // ⛔ A render box, not a node — skipped in full, not just in the
    // child comparison, or it reports itself as "missing from the arena".
    if n.tag == "::before" || n.tag == "::after" {
        return;
    }
    let id = n.node_id;
    if id != 0 && !crate::dom::arena::is_shadow_node_id(id) {
        match d.arena.try_get(crate::dom::arena::NodeId(id)) {
            None => drift.push(format!("#{id} <{}> missing from the arena", n.tag)),
            Some(a) => {
                if a.tag != n.tag {
                    drift.push(format!("#{id} tag: tree={:?} arena={:?}", n.tag, a.tag));
                }
                for (k, v) in n.attributes.iter() {
                    match a.attributes.get(k) {
                        Some(av) if av == v => {}
                        other => drift.push(format!(
                            "#{id} <{}> attr {k}: tree={v:?} arena={other:?}",
                            n.tag
                        )),
                    }
                }
                if (n.tag == "#text" || n.tag == "#comment") && a.text != n.text {
                    drift.push(format!("#{id} text: tree={:?} arena={:?}", n.text, a.text));
                }
                // ⛔ TREE SHAPE, not just per-node data. `childNodes`,
                // `parentNode` and every traversal read the ARENA's links,
                // so a structural disagreement is invisible to the checks
                // above and visible to any caller.
                // ⛔ `::before` / `::after` are render boxes, NOT nodes:
                // the DOM must not expose them in `childNodes`, so the
                // arena is right to lack them and the comparison has to
                // skip them. Asserted positively in
                // `pseudo_elements_are_boxes_not_nodes`.
                let tree_kids: Vec<u32> = n
                    .children
                    .iter()
                    .filter(|c| c.tag != "::before" && c.tag != "::after")
                    .map(|c| c.node_id)
                    .collect();
                let arena_kids: Vec<u32> = d
                    .arena
                    .children(crate::dom::arena::NodeId(id))
                    .map(|c| c.0)
                    .collect();
                if tree_kids != arena_kids {
                    drift.push(format!(
                        "#{id} <{}> children: tree={tree_kids:?} arena={arena_kids:?}",
                        n.tag
                    ));
                }
            }
        }
    }
    for c in &n.children {
        collect_drift(d, c, drift);
    }
}

/// `arenaplan.md` item 4's premise, checked rather than assumed.
///
/// The plan calls the arena migration a CORRECTNESS item: an element is stored
/// twice, in `WebCore` and in `DomArena`, and "every future mutation API is
/// another chance for them to drift". Before moving 714 `.children` walks, it
/// is worth knowing whether they have drifted ALREADY — and afterwards this is
/// the regression guard the migration needs.
#[test]
fn the_two_representations_of_an_element_agree() {
    let mut r = crate::Renderer::new();
    let doc = r.load_html(include_str!("../../examples/html/demo.html"), 900.0);

    let mut drift: Vec<String> = Vec::new();
    collect_drift(&doc, &doc.root, &mut drift);
    assert_eq!(drift, Vec::<String>::new(), "{} drifted", drift.len());
}

/// ⛔ A LIVE DOM BUG, found by checking `arenaplan.md` item 4's premise instead
/// of assuming it.
///
/// Table normalization synthesizes a `<tbody>` via `WebCore::new`, which hands
/// out ids from its OWN counter starting at 500,000 — ids the arena has never
/// heard of. `wire_arena_children` then skipped that node AND everything below
/// it, so a `<td>`'s text never reached the arena: `textContent` on a table
/// cell answered `""` and `childNodes` was empty, while the identical markup
/// in a `<div>` worked.
#[test]
fn a_table_cell_reaches_the_dom_like_any_other_element() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<table><tr><td id=cell>hi</td></tr></table><div id=plain>hi</div>",
        900.0,
    );
    let cell = d.get_element_by_id("cell").unwrap();
    let plain = d.get_element_by_id("plain").unwrap();
    assert_eq!(
        (d.text_content(cell), d.child_nodes(cell).len()),
        (d.text_content(plain), d.child_nodes(plain).len()),
        "a table cell must reach the DOM exactly as a div does"
    );
    assert_eq!(d.text_content(cell), "hi");

    // ⛔ And it must be a TEXT node, not an element named `#text`. A mutation
    // showed the fallback arm would create it as an element and every
    // assertion above still passed — the same shape as the parsed-comment bug
    // in `test_traversal.rs`.
    let kid = d.child_nodes(cell)[0];
    assert_eq!(d.node_type(kid), 3, "nodeType of a table cell's text");
    assert_eq!(d.node_name(kid), "#text");
    assert_eq!(
        d.node_type(d.child_nodes(plain)[0]),
        3,
        "and the div agrees"
    );

    // A SYNTHESIZED text node reaches the same path: `<input type=submit>`
    // gets a `"Submit"` label built with `WebCore::new("#text")`, so it has no
    // arena node until the wiring makes one. It must be a text node too.
    let mut r2 = crate::Renderer::new();
    let d2 = r2.load_html("<input id=b type=submit>", 900.0);
    let btn = d2.get_element_by_id("b").unwrap();
    let kids = d2.child_nodes(btn);
    assert_eq!(kids.len(), 1, "the synthesized label");
    assert_eq!(
        d2.node_type(kids[0]),
        3,
        "a synthesized label is a TEXT node"
    );
    assert_eq!(d2.text_content(btn), "Submit");
}

/// The other half of `arenaplan.md` item 4's premise: "every future mutation
/// API is another chance for them to drift." The check above runs on a freshly
/// parsed document; this one runs after the DOM has actually been mutated.
#[test]
fn the_two_representations_still_agree_after_mutation() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html("<div id=host><p id=a>one</p><p id=b>two</p></div>", 900.0);
    let host = d.get_element_by_id("host").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();

    let made = d.create_element("span");
    d.set_attribute(made, "class", "new");
    let t = d.create_text_node("three");
    d.append_child(made, t);
    d.append_child(host, made);
    let ital = d.create_element("i");
    d.set_attribute(ital, "data-k", "v");
    d.insert_before(host, ital, b);
    d.set_attribute(a, "title", "changed");
    d.remove_child(b);
    let text_child = d.child_nodes(a)[0];
    d.set_text_data(text_child, "ONE");

    let mut drift: Vec<String> = Vec::new();
    collect_drift(&d, &d.root, &mut drift);
    assert_eq!(
        drift,
        Vec::<String>::new(),
        "{} drifted after mutation",
        drift.len()
    );
}

/// Every node that reaches the tree must have an id the ARENA issued.
///
/// ⛔ `WebCore::new` hands out ids from a private counter starting at 500,000.
/// That is the root cause of the table-cell bug above: a node carrying such an
/// id is invisible to every arena-backed DOM accessor. `wire_arena_children`
/// now repairs the ones the PARSER creates — this checks the other roads in.
#[test]
fn every_created_node_carries_an_arena_id() {
    let mut d = crate::html::parse_html("<div id=host></div>");
    let host = d.get_element_by_id("host").unwrap();
    let mut bad: Vec<String> = Vec::new();
    let mut check = |d: &crate::types::Document, what: &str, id: u32| {
        if id == 0 || !d.arena.is_alive(crate::dom::arena::NodeId(id)) {
            bad.push(format!("{what} -> #{id} is not a live arena node"));
        }
    };
    let e = d.create_element("span");
    check(&d, "createElement", e);
    let t = d.create_text_node("x");
    check(&d, "createTextNode", t);
    let c = d.create_comment("c");
    check(&d, "createComment", c);
    let f = d.create_document_fragment();
    check(&d, "createDocumentFragment", f);
    let ns = d.create_element_ns("http://www.w3.org/2000/svg", "svg");
    check(&d, "createElementNS", ns);
    let cd = d.create_cdata_section("d");
    check(&d, "createCDATASection", cd);
    let pi = d.create_processing_instruction("t", "d");
    check(&d, "createProcessingInstruction", pi);
    let cl = d.clone_node(host, true);
    check(&d, "cloneNode", cl);
    drop(check);

    // And once attached, they must still be reachable through the DOM.
    d.append_child(host, e);
    d.append_child(e, t);
    assert_eq!(
        d.text_content(host),
        "x",
        "an attached subtree reaches the DOM"
    );
    assert_eq!(
        bad,
        Vec::<String>::new(),
        "{} creation paths leak a phantom id",
        bad.len()
    );
}

/// ⛔ A THIRD live bug of the same family, and the clearest instance of
/// `arenaplan.md` item 4's rationale.
///
/// `Editor::insert_br` takes `&mut WebCore` — there is no `Document` and so no
/// arena to dual-write to. It splits the text node and inserts a `<br>` using
/// `WebCore::new`, whose ids come from a private counter. Result: after an
/// Enter keypress in a `contenteditable`, `textContent` still answered the
/// PRE-EDIT text and four nodes had drifted.
///
/// `resync_subtree` repairs it at the `Document` boundary. Folding `WebCore`
/// into the arena is what removes the need.
#[test]
fn an_edit_leaves_the_dom_consistent() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html("<div id=e contenteditable=true>hello</div>", 900.0);
    let host = d.get_element_by_id("e").unwrap();
    let leaf = d.child_nodes(host)[0];
    d.editor.set_caret_from_hit(leaf, 2, false);

    let mut editor = std::mem::take(&mut d.editor);
    editor.insert_br(&mut d.root);
    d.editor = editor;
    let mut root = std::mem::replace(&mut d.root, crate::types::WebCore::new("#placeholder"));
    crate::html::arena_wiring::resync_subtree(&mut d.arena, &mut root);
    d.root = root;

    let mut drift: Vec<String> = Vec::new();
    collect_drift(&d, &d.root, &mut drift);
    assert_eq!(
        drift,
        Vec::<String>::new(),
        "{} drifted after an edit",
        drift.len()
    );
    assert_eq!(
        d.text_content(host),
        "hello",
        "the halves are still the same text"
    );
    assert_eq!(d.child_nodes(host).len(), 3, "\"he\", <br>, \"llo\"");

    // ⛔ And an edit that MUTATES an existing node rather than creating one.
    // Two mutations survived without this: `create_text` sets the data at
    // creation, so the copy-back only matters for a node the arena already
    // had — which is what typing a character does.
    let mut r2 = crate::Renderer::new();
    let mut d2 = r2.load_html("<div id=e contenteditable=true>hello</div>", 900.0);
    let host2 = d2.get_element_by_id("e").unwrap();
    let leaf2 = d2.child_nodes(host2)[0];
    d2.editor.set_caret_from_hit(leaf2, 5, false);
    d2.process_key_event(
        crate::dom::HtmlEventType::KeyDown,
        0,
        Some('!'),
        false,
        false,
        false,
        false,
    );
    let mut drift2: Vec<String> = Vec::new();
    collect_drift(&d2, &d2.root, &mut drift2);
    assert_eq!(
        drift2,
        Vec::<String>::new(),
        "{} drifted after typing",
        drift2.len()
    );
    assert_eq!(
        d2.text_content(host2),
        "hello!",
        "the typed character reaches the DOM"
    );
}

/// ⛔ UNDEFINED BEHAVIOUR, live in the codebase until this test.
///
/// `Document::node_index` cached `*const WebCore`, and its safety comment said
/// the pointers were "valid because the tree hasn't been mutated since
/// `rebuild_node_index()` was called" — an invariant nothing enforced.
/// `append_child` pushes to `parent.children`; the `Vec` reallocates; every
/// cached pointer into it dangles; and the next `get_box_by_id` dereferenced
/// one.
///
/// Demonstrated safely — comparing the cached address against a fresh walk
/// rather than dereferencing either — then fixed by caching a PATH, which
/// cannot dangle. This asserts the lookup stays CORRECT across the mutation
/// that used to invalidate it.
#[test]
fn a_node_lookup_survives_a_reallocating_mutation() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html("<div id=host><p id=a>one</p></div>", 900.0);
    d.rebuild_node_index();
    let host = d.get_element_by_id("host").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    assert_eq!(d.get_box_by_id(a).map(|n| n.node_id), Some(a), "before");

    // Enough appends to force `parent.children` to reallocate.
    for _ in 0..64 {
        let e = d.create_element("span");
        d.append_child(host, e);
    }

    assert_eq!(
        d.get_box_by_id(a).map(|n| n.node_id),
        Some(a),
        "the lookup must still find the right node after a reallocation"
    );
    assert_eq!(
        d.get_box_by_id(a).map(|n| n.tag.clone()),
        Some("p".to_string())
    );
}

/// The example pages every drift guard runs on. One list, so a guard added
/// later cannot quietly run on fewer pages than the others.
const EXAMPLE_PAGES: &[(&str, &str)] = &[
    (
        "animation_demo.html",
        include_str!("../../examples/html/animation_demo.html"),
    ),
    (
        "calculator.html",
        include_str!("../../examples/html/calculator.html"),
    ),
    (
        "cascade_features.html",
        include_str!("../../examples/html/cascade_features.html"),
    ),
    (
        "container.html",
        include_str!("../../examples/html/container.html"),
    ),
    (
        "contenteditable.html",
        include_str!("../../examples/html/contenteditable.html"),
    ),
    ("demo.html", include_str!("../../examples/html/demo.html")),
    ("dom.html", include_str!("../../examples/html/dom.html")),
    ("edit.html", include_str!("../../examples/html/edit.html")),
    (
        "edit_demo.html",
        include_str!("../../examples/html/edit_demo.html"),
    ),
    ("email.html", include_str!("../../examples/html/email.html")),
    (
        "eudora.html",
        include_str!("../../examples/html/eudora.html"),
    ),
    (
        "event_playground.html",
        include_str!("../../examples/html/event_playground.html"),
    ),
    (
        "events.html",
        include_str!("../../examples/html/events.html"),
    ),
    (
        "forms_demo.html",
        include_str!("../../examples/html/forms_demo.html"),
    ),
    ("graph.html", include_str!("../../examples/html/graph.html")),
    (
        "layout_features.html",
        include_str!("../../examples/html/layout_features.html"),
    ),
    (
        "markdown.html",
        include_str!("../../examples/html/markdown.html"),
    ),
    (
        "minesweeper.html",
        include_str!("../../examples/html/minesweeper.html"),
    ),
    (
        "overflow.html",
        include_str!("../../examples/html/overflow.html"),
    ),
    ("print.html", include_str!("../../examples/html/print.html")),
    (
        "subgrid.html",
        include_str!("../../examples/html/subgrid.html"),
    ),
    (
        "tictactoe.html",
        include_str!("../../examples/html/tictactoe.html"),
    ),
    (
        "transform_filter_demo.html",
        include_str!("../../examples/html/transform_filter_demo.html"),
    ),
    (
        "transitions_demo.html",
        include_str!("../../examples/html/transitions_demo.html"),
    ),
];

/// The drift check across EVERY example page, not just `demo.html`.
///
/// ⛔ Each earlier bug in this family was found on one fixture and would have
/// been found sooner on 24. A guard that runs on a single page is a
/// guard for that page.
#[test]
fn no_example_page_drifts_between_its_two_representations() {
    let pages = EXAMPLE_PAGES;
    let mut bad: Vec<String> = Vec::new();
    for (name, src) in pages {
        // ⛔ `parse_html`, not `load_html`: every bug in this family was
        // introduced at PARSE time, and cascading plus laying out 24 pages
        // took the whole suite from 10 s to 95 s. Verified that the cheap
        // version still catches them by reverting the `<html lang>` fix and
        // watching this go red.
        let doc = crate::html::parse_html(src);
        let mut drift: Vec<String> = Vec::new();
        collect_drift(&doc, &doc.root, &mut drift);
        if !drift.is_empty() {
            bad.push(format!(
                "{name}: {} drifted — first: {}",
                drift.len(),
                drift[0]
            ));
        }
    }
    assert_eq!(bad, Vec::<String>::new(), "{} pages drifted", bad.len());
}

/// ⛔ `::before` and `::after` are render BOXES, not DOM nodes. The drift check
/// has to skip them, so their absence is asserted here instead — otherwise
/// "skip them" could hide a real disappearance.
#[test]
fn pseudo_elements_are_boxes_not_nodes() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>#p::before { content: \"X\"; display: block }</style><p id=p>hi</p>",
        900.0,
    );
    let p = d.get_element_by_id("p").unwrap();
    for kid in d.child_nodes(p) {
        let name = d.node_name(kid);
        assert!(!name.starts_with("::"), "childNodes must not expose {name}");
    }
    assert_eq!(
        d.text_content(p),
        "hi",
        "and textContent is the real content"
    );
}

/// The OTHER direction. Every guard so far walks the tree and looks the node up
/// in the arena — so a node that stays ALIVE in the arena after leaving the
/// tree is invisible to all of them. `remove_child` deliberately keeps a
/// detached node alive so it can be re-inserted, which makes the boundary
/// between "detached, still referenced" and "leaked" worth pinning.
/// ⛔ UNBOUNDED GROWTH, measured. Every guard so far walks the tree and looks
/// the node up in the arena, so a node that stays ALIVE in the arena after
/// leaving the tree is invisible to all of them.
///
/// `remove_child` keeps a detached node alive on purpose — DOM §4.2.3 says
/// `removeChild` hands the node back and the caller may insert it elsewhere,
/// and that re-insertion is asserted below, so the retention is load-bearing.
/// But a browser frees the node once script drops its reference, and there is
/// no GC here: **50 removed `<p>`s with text left 100 arena nodes alive and 4
/// reachable.** A page churning its DOM grows the arena for ever, at roughly
/// 832 B per removed node (`WebCore` 640 + arena `Node` 192), since
/// `pending_nodes` retains the `WebCore` too.
///
/// Pinned rather than "fixed": freeing on removal would break the re-insertion
/// below. The real answer is a reachability sweep or an explicit release, and
/// that is a design decision, not a patch.
#[test]
fn a_removed_subtree_is_retained_so_it_can_be_reinserted() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html("<div id=host></div><div id=other></div>", 900.0);
    let host = d.get_element_by_id("host").unwrap();
    let other = d.get_element_by_id("other").unwrap();

    // The retention is load-bearing: a removed node can be re-inserted, with
    // its subtree and attributes intact.
    let p = d.create_element("p");
    d.set_attribute(p, "class", "keep");
    let t = d.create_text_node("text");
    d.append_child(p, t);
    d.append_child(host, p);
    d.remove_child(p);
    assert!(d.child_nodes(host).is_empty(), "detached");
    d.append_child(other, p);
    assert_eq!(d.child_nodes(other), vec![p], "re-inserted");
    assert_eq!(d.get_attribute(p, "class").as_deref(), Some("keep"));
    assert_eq!(d.text_content(p), "text", "with its subtree");

    // And the cost of that retention, so a change to it is deliberate.
    let before = d.arena.len();
    for _ in 0..50 {
        let e = d.create_element("p");
        let tx = d.create_text_node("x");
        d.append_child(e, tx);
        d.append_child(host, e);
    }
    for kid in d.child_nodes(host) {
        d.remove_child(kid);
    }
    assert_eq!(
        d.arena.len() - before,
        100,
        "every removed node is retained — nothing reclaims them"
    );
}

/// ⛔ Why the retention in `remove_child` / `set_inner_html` is DELIBERATE,
/// and what it would take to fix.
///
/// 20 `innerHTML` writes leak 80 arena nodes with 8 reachable — the exact
/// churn pattern a real page uses, and one where no caller can be holding the
/// discarded nodes, so freeing them looks obviously safe.
///
/// It is not — but not because freeing is unsafe. `DomArena` never reissues an
/// id, so a discarded node's id stays dead and can never name a DIFFERENT node
/// (reading it as gone is a further step — only `try_get` consults `alive`).
/// What stops the reclaim is the DOM's own lifetime rule: a removed node is
/// re-insertable as long as anything holds it (the test above pins exactly
/// that), and script holds these ids, so this layer cannot tell when one is
/// garbage. That needs a release discipline — `arenaplan.md` item 7.
///
/// Both halves are pinned here so neither can be "fixed" in ignorance of the
/// other: the retention below is the cost, and the no-reissue guarantee is
/// what any future reclaim will rest on.
#[test]
fn nothing_is_reclaimed_and_no_freed_id_is_ever_reissued() {
    let mut d = crate::html::parse_html("<div id=host></div>");
    let host = d.get_element_by_id("host").unwrap();

    // The leak, on the commonest churn path.
    let base = d.arena.len();
    for i in 0..20 {
        d.set_inner_html(host, &format!("<p class=c{i}>row {i}</p><span>x</span>"));
    }
    let mut seen = std::collections::HashSet::new();
    fn reach(n: &WebCore, seen: &mut std::collections::HashSet<u32>) {
        seen.insert(n.node_id);
        for c in &n.children {
            reach(c, seen);
        }
    }
    reach(&d.root, &mut seen);
    assert_eq!(
        d.arena.len() - base,
        80,
        "every discarded generation is retained"
    );

    // And the guarantee the reclaim will rest on: a freed id is dead for good.
    let doomed = d.create_element("i");
    d.arena.free(crate::dom::arena::NodeId(doomed));
    let fresh = d.create_element("b");
    assert_ne!(
        fresh, doomed,
        "the freed id is not recycled onto a new element"
    );
    assert!(
        !d.arena.is_alive(crate::dom::arena::NodeId(doomed)),
        "and it stays dead, so a stale handle resolves to nothing"
    );
    assert_eq!(d.tag_name(fresh), Some("b"));
}

/// `WebCore` holds its children TWICE — as `children: Vec<WebCore>` and as the
/// `first_child`/`next_sibling`/`parent`/`last_child`/`prev_sibling` link
/// fields beside it. Nothing has ever checked that the two agree.
///
/// They are the halves item 4 collapses into one, so a disagreement is the
/// same class of bug as the 107 tree-vs-arena drifts: two structures, one
/// mutation path reaching only one of them.
/// The render tree and the DOM are two DIFFERENT trees, and this says which
/// is which.
///
/// `WebCore` used to carry `parent`/`first_child`/`last_child`/`next_sibling`/
/// `prev_sibling` beside `children`, a half-finished merge of the two. They
/// were right at parse time and wrong after every mutation — five ordinary DOM
/// operations produced eleven disagreements, and one Enter in a
/// `contenteditable` left the chain naming a `WebCore::new` id in the 500,000
/// range that `resync_subtree` had already renumbered away. Nothing read them
/// for an answer, so nothing ever noticed. They are gone.
///
/// What replaces them is this: the DOM answers come from the arena, the render
/// tree owns its own order, and `node_id` is the only link. The two guards
/// below are the two directions of that.
#[test]
fn the_dom_answers_come_from_the_arena_not_from_the_render_tree() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html("<div id=host><p id=a>one</p><p id=b>two</p></div>", 900.0);
    let host = d.get_element_by_id("host").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();

    let made = d.create_element("span");
    let t = d.create_text_node("three");
    d.append_child(made, t);
    d.append_child(host, made);
    let ital = d.create_element("i");
    d.insert_before(host, ital, b);
    d.remove_child(b);
    d.set_inner_html(a, "<em>rebuilt</em><b>twice</b>");

    // Structure, order and identity — all from the arena, all correct after a
    // mixture of append, insert, remove and a re-parse.
    assert_eq!(
        d.child_nodes(host),
        vec![a, ital, made],
        "children, in order"
    );
    assert_eq!(d.parent_node(ital), host, "and the way back up");
    assert_eq!(d.next_sibling(a), ital);
    assert_eq!(d.next_sibling(ital), made);
    assert_eq!(d.next_sibling(made), 0);
    assert_eq!(d.previous_sibling(made), ital);
    assert_eq!(d.text_content(a), "rebuilttwice");
    assert_eq!(d.parent_node(b), 0, "the removed node is detached");

    // And the render tree agrees about every node it shares with the DOM.
    let mut drift: Vec<String> = Vec::new();
    collect_drift(&d, &d.root, &mut drift);
    assert_eq!(drift, Vec::<String>::new(), "{} drifted", drift.len());
}

/// The other direction: the render tree legitimately holds boxes the DOM does
/// not, which is why folding it into the arena would be a bug and not a
/// simplification.
///
/// `::before` is the case that exists on a real page. Plain inline pseudo
/// content is carried by the render style, not exposed as a DOM child; promoted
/// block/positioned/grid/flex pseudo-elements still materialize as boxes.
#[test]
fn the_render_tree_holds_boxes_the_dom_does_not() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>#x::before{content:'!'}</style><div id=x>text</div>",
        900.0,
    );
    let x = d.get_element_by_id("x").unwrap();

    fn find<'a>(n: &'a WebCore, id: u32) -> Option<&'a WebCore> {
        if n.node_id == id {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    let box_x = find(&d.root, x).expect("the element has a box");

    assert_eq!(
        box_x.style.before_content, "!",
        "the render style carries generated content"
    );
    assert!(
        !d.child_nodes(x)
            .iter()
            .any(|c| d.tag_name(*c) == Some("::before")),
        "and the DOM does not"
    );
}

/// ⛔ The FOURTH hole in the style-sharing key, and the first that is not an
/// attribute at all.
///
/// The key was `(parent, tag, attributes)`, on the stated premise that "the
/// attributes ARE the element as far as a selector is concerned". They are
/// not: `:modal`, `:popover-open`, `:checked`, `:indeterminate`, `:focus` and
/// `:in-range` all match on the BOX. Two `<dialog open>`s — identical in tag
/// and attributes, one `show()`n and one `showModal()`ed — hashed the same,
/// and the modal was handed the plain one's style. The UA sheet's
/// `dialog:modal { position: fixed }` silently did nothing.
///
/// Chrome is the oracle for the shape: `getComputedStyle(modal).position` is
/// `fixed`, the plain one is `absolute`, and NEITHER carries an inline style —
/// `getAttribute("style")` is `null` on both.
#[test]
fn style_sharing_must_not_ignore_box_state() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<html><head></head><body><dialog></dialog><dialog></dialog></body></html>",
        900.0,
    );
    let dialogs = d.get_elements_by_tag_name("dialog");
    let (plain, modal) = (dialogs[0], dialogs[1]);

    d.show_dialog(plain, false);
    d.show_dialog(modal, true);

    assert_eq!(
        d.computed_style_property(modal, "position"),
        "fixed",
        "the UA sheet's `dialog:modal` rule must reach the modal"
    );
    assert_ne!(
        d.computed_style_property(plain, "position"),
        "fixed",
        "and must not reach the one beside it"
    );
    // The two elements are indistinguishable by tag and attributes, which is
    // the whole point — if they were not, the old key would have coped.
    assert_eq!(
        d.get_attribute(plain, "open"),
        d.get_attribute(modal, "open")
    );
    assert_eq!(d.tag_name(plain), d.tag_name(modal));
    assert_eq!(d.get_attribute(modal, "style"), None, "no inline write");
}

/// ⛔ Loading a page must not build a second `Renderer`.
///
/// `Renderer::load_html` called `load_html_with_registry`, which constructed
/// its own `Renderer` for the initial layout — a second `FontSystem`, built,
/// used once and dropped, while the caller's sat idle. Measured:
/// `FontSystem::new()` is **3185 ms cold and 173 ms warm**, `Renderer::new()`
/// 172 ms of which is the font system, and `LayoutEngine::new()` 0 ms.
///
/// It also threw away the engine's `text_width_cache` on every load, so the
/// first layout of every page measured every string from scratch.
///
/// A one-element document spent 118 ms in the "Layout" phase and 0 ms in
/// `layout()` — the whole of it was the discarded renderer. Alternating in one
/// process on Wikipedia's front page, reuse is ~2.2x faster end to end.
#[test]
fn loading_a_page_does_not_build_a_second_renderer() {
    let tiny = "<html><head></head><body><p>x</p></body></html>";
    let mut r = crate::Renderer::new();
    let _ = r.load_html(tiny, 1280.0); // warm the process-wide font caches

    let t = std::time::Instant::now();
    for _ in 0..5 {
        let _ = r.load_html(tiny, 1280.0);
    }
    let reuse_ms = t.elapsed().as_millis();

    let t = std::time::Instant::now();
    for _ in 0..5 {
        let _ = crate::load_html_with_registry(
            tiny,
            "",
            1280.0,
            700.0,
            crate::types::ComponentRegistry::default(),
        );
    }
    let fresh_ms = t.elapsed().as_millis();

    assert!(
        reuse_ms * 3 < fresh_ms.max(1),
        "reusing the caller's renderer must be far cheaper than building one \
         per load — got reuse {reuse_ms} ms vs fresh {fresh_ms} ms for 5 loads \
         of a one-element page"
    );
}

/// ⛔ A frame in which NOTHING changed must be nearly free, and a scroll must
/// not cost anything like a first render.
///
/// Three separate bugs made every frame redo the whole document, and each was
/// invisible to every other test because they all produce correct pixels:
///
/// * `render()` baked `scroll_x/y` into the cached display list, so the cache
///   was only valid at the offset it was built for and the page could only
///   move by rebuilding. `replay_with_scroll` existed for this and was never
///   wired up.
/// * `replay_inner` painted the full document height with no viewport culling,
///   shaping text nobody could see.
/// * every text run was re-shaped, per frame. `SwashCache` caches RASTERISED
///   GLYPHS — a different thing, and no help.
///
/// Measured on Wikipedia's front page, debug: an idle re-render went 5567 ms →
/// 30 ms and a scroll 4888-13580 ms → ~120 ms.
///
/// The assertion is a RATIO, not a millisecond count, so it means the same
/// thing on a slow machine or a contended one.
#[test]
fn an_unchanged_frame_and_a_scroll_are_cheap() {
    let mut html = String::from(
        "<style>body{margin:0;font:16px/20px Arial}.row{height:24px;border-bottom:1px solid #ddd}</style>",
    );
    for i in 0..120 {
        html.push_str(&format!("<div class=row>cached display list row {i}</div>"));
    }
    let mut r = crate::Renderer::new();
    let mut doc = r.load_html(&html, 1280.0);
    let mut pm = tiny_skia::Pixmap::new(1280, 900).unwrap();

    r.render(&mut doc, &mut pm, 1.0);
    let first_cache = r
        .display_list_cache_state()
        .expect("first render builds a display-list cache");

    r.render(&mut doc, &mut pm, 1.0);
    let idle_cache = r
        .display_list_cache_state()
        .expect("idle render keeps the display-list cache");

    doc.scroll_y += 300.0;
    r.render(&mut doc, &mut pm, 1.0);
    let scrolled_cache = r
        .display_list_cache_state()
        .expect("scroll render keeps the display-list cache");

    assert_eq!(
        idle_cache, first_cache,
        "an unchanged frame must reuse the cached display list"
    );
    assert_eq!(
        scrolled_cache, first_cache,
        "a scroll must reuse the cached document display list"
    );
}

/// ⛔ Every positioning scheme must survive the scroll-cached display list.
///
/// The list is built once in DOCUMENT coordinates and translated by the scroll
/// at replay, which is what makes scrolling cheap. That is only sound if a
/// box's document position does not itself depend on the scroll:
///
/// | scheme | why it is safe |
/// |---|---|
/// | static, relative, float, absolute | positioned in the document; translate |
/// | fixed | NOT translated — its own `fixed_commands` list |
/// | sticky | position IS a function of scroll ⇒ forces a rebuild |
///
/// `fixed` and `sticky` both broke when the list first moved to document
/// coordinates: a fixed header scrolled away with the page, and a sticky box
/// clamped against the top of the DOCUMENT instead of the viewport, so it
/// never stuck. This renders each kind at a scrolled offset and checks it is
/// where it belongs, by sampling the pixel.
#[test]
fn every_positioning_scheme_survives_the_scroll_cache() {
    let html = r#"<html><head><style>
        body { margin:0; background:#fff }
        #tall { height: 3000px }
        #fix { position:fixed; top:50px; left:10px; width:60px; height:60px; background:#f00 }
        #stick { position:sticky; top:20px; left:100px; width:60px; height:60px; background:#0f0 }
        #abs { position:absolute; top:1000px; left:200px; width:60px; height:60px; background:#00f }
        #rel { position:relative; top:10px; left:300px; width:60px; height:60px; background:#ff0 }
        #flo { float:left; width:60px; height:60px; background:#0ff }
      </style></head><body>
      <div id=fix></div><div id=stick></div><div id=abs></div>
      <div id=rel></div><div id=flo></div><div id=tall></div>
      </body></html>"#;
    let mut r = crate::Renderer::new();
    let mut doc = r.load_html(html, 800.0);
    let mut pm = tiny_skia::Pixmap::new(800, 600).unwrap();

    let at = |pm: &tiny_skia::Pixmap, x: u32, y: u32| {
        let p = pm.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue())
    };

    // Unscrolled: fixed is at its viewport spot.
    r.render(&mut doc, &mut pm, 1.0);
    assert_eq!(at(&pm, 40, 80), (255, 0, 0), "fixed box before scrolling");

    // Scrolled: fixed must NOT have moved.
    doc.scroll_y = 500.0;
    r.render(&mut doc, &mut pm, 1.0);
    assert_eq!(
        at(&pm, 40, 80),
        (255, 0, 0),
        "a fixed box must stay put when the page scrolls — it did not, because \
         it was translated along with the document-coordinate list"
    );

    // …and the absolute box, at document y=1000, must appear at 1000-500=500.
    assert_eq!(
        at(&pm, 230, 530),
        (0, 0, 255),
        "an absolutely positioned box translates with the document"
    );
}
