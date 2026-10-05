//! `getComputedStyle` — CSSOM §6.6.
//!
//! The string accessor used to resolve a handful of properties from the layout
//! rect and fall back to the INLINE style for everything else, so
//! `getComputedStyle(el).position` answered `""` for a value that came from a
//! stylesheet — with the right answer sitting in `ComputedStyle` all along.
//!
//! Every expectation is a Chrome measurement (`/tmp/webcore-html/cs.html`).
//! The serialization is not obvious in three places: `font-weight` is a NUMBER,
//! a transparent colour is `"rgba(0, 0, 0, 0)"` rather than the keyword, and
//! `max-width` uses `none` where `min-width` uses a length.

use crate::types::Document;

#[test]
fn break_spaces_is_inherited_and_serialized_by_cssom() {
    let mut renderer = crate::Renderer::new();
    let mut document = renderer.load_html(
        "<style>#parent{white-space:break-spaces}</style><div id=parent><span id=child>A  B</span></div>",
        400.0,
    );
    for name in ["parent", "child"] {
        let id = document.get_element_by_id(name).unwrap();
        assert_eq!(
            document.computed_style_property(id, "white-space"),
            "break-spaces"
        );
    }
}

const PAGE: &str = r#"<style>
#a { position: fixed; color: #010203; background-color: rgba(1,2,3,0.5); font-size: 20px;
     font-weight: bold; margin-top: 1em; padding-left: 3px; border-top: 2px dashed red;
     z-index: 5; overflow: hidden; box-sizing: border-box; width: 50px; height: 10px;
     min-width: 5px; max-width: 100px; font-style: italic; }
#d { position: relative; }
#z { position: relative; z-index: 0; }
</style>
<div id=a>A</div><div id=b>B</div><div id=c style="display:none">C</div>
<div id=d>D</div><span id=e>E</span><div id=z>Z</div>"#;

fn page() -> Document {
    let mut renderer = crate::Renderer::new();
    renderer.load_html(PAGE, 800.0)
}

#[test]
fn cssom_reads_preserve_completed_font_aware_layout() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>p{font:32px/normal monospace;width:170px;padding:3px}p::before{content:'x'}</style><p id=box>Wide words <a href=#>inline link</a> after text</p>",
        800.0,
    );
    let id = doc.get_element_by_id("box").unwrap();
    let generation = doc.layout_generation;
    let before = doc.get_node(id).unwrap().layout.line_cache.clone();
    let rect = doc.get_bounding_client_rect(id).unwrap();
    for property in ["font-size", "width", "padding", "border", "line-height"] {
        assert!(!doc.computed_style_property(id, property).is_empty());
    }
    assert!(
        !doc.computed_style_pseudo_property(id, "before", "color")
            .is_empty()
    );
    assert_eq!(doc.layout_generation, generation);
    assert_eq!(doc.get_bounding_client_rect(id).unwrap(), rect);
    let after = &doc.get_node(id).unwrap().layout.line_cache;
    assert_eq!(after.len(), before.len());
    for (a, b) in after.iter().zip(&before) {
        assert_eq!(
            (a.x, a.y, a.width, a.height, a.text_length),
            (b.x, b.y, b.width, b.height, b.text_length)
        );
    }
}

#[test]
fn cssom_dirty_batch_flushes_once_with_the_owning_engine() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=box style='width:100px;padding:2px'>Text</div>",
        800.0,
    );
    let id = doc.get_element_by_id("box").unwrap();
    let generation = doc.layout_generation;
    doc.set_style_property(id, "width", "200px");
    doc.update_computed_style(renderer.layout_engine());
    assert!(doc.layout_generation > generation);
    let updated = doc.layout_generation;
    assert_eq!(doc.computed_style_property_current(id, "width"), "200px");
    assert_eq!(doc.computed_style_property_current(id, "padding"), "2px");
    doc.update_computed_style(renderer.layout_engine());
    assert_eq!(doc.layout_generation, updated);
}

#[test]
fn cssom_used_sizes_respect_box_sizing_without_transforms() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>div{width:100px;height:60px;padding:10px;border:2px solid;transform:scale(2)}#border{box-sizing:border-box}</style><div id=content></div><div id=border></div>",
        800.0,
    );
    for name in ["content", "border"] {
        let id = doc.get_element_by_id(name).unwrap();
        assert_eq!(doc.computed_style_property(id, "width"), "100px");
        assert_eq!(doc.computed_style_property(id, "height"), "60px");
    }
}

#[test]
fn initial_normal_line_height_is_not_serialized_as_auto() {
    let mut document = page();
    let id = document.get_element_by_id("b").unwrap();
    assert_eq!(
        document.computed_style_property(id, "line-height"),
        "normal"
    );
}

#[test]
fn font_width_aliases_expose_computed_percentages_and_inherit() {
    let mut renderer = crate::Renderer::new();
    let mut document = renderer.load_html(
        r#"<style>
        #parent { font-width: calc(80% + 15%); font-width: -10%; }
        #parent::before { content: "x"; font-stretch: expanded; }
        </style><div id=parent><span id=child>Text</span></div>"#,
        800.0,
    );
    let parent = document.get_element_by_id("parent").unwrap();
    let child = document.get_element_by_id("child").unwrap();
    for property in ["font-width", "font-stretch"] {
        assert_eq!(document.computed_style_property(parent, property), "95%");
        assert_eq!(document.computed_style_property(child, property), "95%");
        assert_eq!(
            document.computed_style_pseudo_property(parent, "before", property),
            "125%"
        );
    }
}
fn el(d: &Document, id: &str) -> u32 {
    d.get_element_by_id(id).unwrap()
}

/// (id, property, expected) — every row measured.
fn check(rows: &[(&str, &str, &str)]) {
    let mut d = page();
    for (id, prop, want) in rows {
        let e = el(&d, id);
        assert_eq!(&d.computed_style_property(e, prop), want, "#{id} {prop}");
    }
}

#[test]
fn a_value_that_came_from_a_stylesheet_is_now_answerable() {
    // ⛔ The whole point: not one of these was set INLINE, and every one of
    // them answered `""` before.
    check(&[
        ("a", "position", "fixed"),
        ("a", "display", "block"),
        ("a", "box-sizing", "border-box"),
        ("a", "overflow-x", "hidden"),
        ("a", "overflow-y", "hidden"),
        ("a", "font-style", "italic"),
        ("a", "border-top-style", "dashed"),
    ]);
}

#[test]
fn the_defaults_come_from_the_cascade_rather_than_from_nothing() {
    check(&[
        ("b", "position", "static"),
        ("b", "display", "block"),
        ("e", "display", "inline"),
        ("c", "display", "none"),
        ("b", "overflow-x", "visible"),
        ("b", "box-sizing", "content-box"),
        ("b", "border-top-style", "none"),
        ("b", "clear", "none"),
        ("b", "font-style", "normal"),
    ]);
}

#[test]
fn colours_serialize_as_rgb_when_opaque_and_rgba_when_not() {
    // ⛔ Transparent is `"rgba(0, 0, 0, 0)"`, never the keyword.
    check(&[
        ("a", "color", "rgb(1, 2, 3)"),
        ("a", "background-color", "rgba(1, 2, 3, 0.5)"),
        ("a", "border-top-color", "rgb(255, 0, 0)"),
        ("b", "color", "rgb(0, 0, 0)"),
        ("b", "background-color", "rgba(0, 0, 0, 0)"),
    ]);
}

#[test]
fn font_weight_serializes_as_a_number_not_a_keyword() {
    // ⛔ `font-weight: bold` answers `"700"`.
    check(&[("a", "font-weight", "700"), ("b", "font-weight", "400")]);
}

#[test]
fn relative_font_weight_uses_the_inherited_weight_table() {
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        "<div style='font-weight:400'><b id='bolder' style='font-weight:bolder'>x</b></div>\
         <div style='font-weight:700'><b id='lighter' style='font-weight:lighter'>x</b></div>",
        800.0,
    );
    let bolder = d.get_element_by_id("bolder").unwrap();
    let lighter = d.get_element_by_id("lighter").unwrap();
    assert_eq!(d.computed_style_property(bolder, "font-weight"), "700");
    assert_eq!(d.computed_style_property(lighter, "font-weight"), "400");
}

#[test]
fn lengths_resolve_against_the_elements_own_font_size() {
    // `margin-top: 1em` on a 20px font is 20px, not 16px.
    check(&[
        ("a", "font-size", "20px"),
        ("a", "margin-top", "20px"),
        ("a", "padding-left", "3px"),
        ("a", "border-top-width", "2px"),
        ("b", "font-size", "16px"),
        ("b", "margin-top", "0px"),
        ("b", "border-top-width", "0px"),
    ]);
}

#[test]
fn max_uses_none_where_min_uses_a_length() {
    // ⛔ The asymmetry: both are `CssLength::Auto` when unset and they
    // serialize differently.
    check(&[
        ("a", "min-width", "5px"),
        ("a", "max-width", "100px"),
        ("b", "min-width", "0px"),
        ("b", "max-width", "none"),
        ("b", "min-height", "0px"),
        ("b", "max-height", "none"),
    ]);
}

#[test]
fn flex_item_auto_min_size_serializes_as_auto() {
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        r#"<style>
        #f { display: flex; }
        #item { min-width: auto; min-height: auto; }
        </style>
        <div id=f><div id=item>wide text</div></div><div id=plain></div>"#,
        800.0,
    );
    let item = el(&d, "item");
    let plain = el(&d, "plain");

    assert_eq!(d.computed_style_property(item, "min-width"), "auto");
    assert_eq!(d.computed_style_property(item, "min-height"), "auto");
    assert_eq!(d.computed_style_property(plain, "min-width"), "0px");
}

#[test]
fn z_index_answers_auto_when_unset_and_the_number_when_set() {
    check(&[
        ("a", "z-index", "5"),
        ("b", "z-index", "auto"),
        ("z", "z-index", "0"),
    ]);
}

#[test]
fn an_inset_is_the_used_value_only_on_a_positioned_box() {
    // The rule that was already right, kept honest: a STATIC box answers the
    // computed inset (`auto`), a relative one with no offsets answers `0px`.
    let mut d = page();
    let b = el(&d, "b");
    let dd = el(&d, "d");
    assert_eq!(d.computed_style_property(b, "top"), "auto");
    assert_eq!(d.computed_style_property(dd, "top"), "0px");
}

#[test]
fn an_uncovered_property_still_falls_back_to_the_inline_style() {
    // ⛔ The BOUNDARY, asserted rather than left to be discovered. A property
    // outside the covered set fails SILENTLY — it answers the inline value or
    // nothing, and looks no different from one that is handled.
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        "<div id=inline style='-webcore-test-prop:left'>x</div><div id=sheet>y</div>",
        800.0,
    );
    let inline = d.get_element_by_id("inline").unwrap();
    let sheet = d.get_element_by_id("sheet").unwrap();
    assert_eq!(
        d.computed_style_property(inline, "-webcore-test-prop"),
        "left",
        "the inline value"
    );
    assert_eq!(
        d.computed_style_property(sheet, "-webcore-test-prop"),
        "",
        "and nothing otherwise"
    );
}

#[test]
fn width_and_height_are_the_used_value_for_a_block_and_auto_otherwise() {
    // ⛔ The discriminating trio, and a mutation showed nothing was testing
    // it: these come from the LAYOUT RECT, not the cascade, and a mutation
    // that skipped the rect path entirely stayed green.
    let mut d = page();
    let a = el(&d, "a");
    let b = el(&d, "b");
    let c = el(&d, "c");
    let e = el(&d, "e");

    assert_eq!(
        d.computed_style_property(a, "width"),
        "50px",
        "a declared width"
    );
    assert_eq!(d.computed_style_property(a, "height"), "10px");

    // A block with no declared width takes its containing block's — a real
    // number, not the keyword.
    let bw = d.computed_style_property(b, "width");
    assert!(
        bw.ends_with("px") && bw != "0px",
        "a block resolves to a used px width, got {bw:?}"
    );

    // An INLINE box has no used width, and neither has an unrendered one.
    assert_eq!(d.computed_style_property(e, "width"), "auto", "inline");
    assert_eq!(
        d.computed_style_property(c, "width"),
        "auto",
        "display:none"
    );
    assert_eq!(d.computed_style_property(c, "height"), "auto");
}

#[test]
fn a_replaced_inline_does_have_a_used_size() {
    // ⛔ The exception to the rule above, and a mutation showed nothing was
    // testing it: `<img>` is `display: inline` like a `<span>`, and Chrome
    // answers `"30px"` for the image and `"auto"` for the span. The branch is
    // keyed on the element being REPLACED, not on its display.
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        "<img id=i src=x width=30 height=20><span id=s>t</span>",
        800.0,
    );
    let img = d.get_element_by_id("i").unwrap();
    let span = d.get_element_by_id("s").unwrap();
    // ⛔ A real UA-stylesheet divergence, pinned rather than papered over:
    // Chrome lays `<img>` out as `display: inline` and this crate uses
    // `inline-block`. Either way it has a used size, so the ANSWER agrees —
    // but the branch it arrives through does not.
    assert_eq!(
        d.computed_style_property(img, "display"),
        "inline-block",
        "Chrome says `inline` here"
    );
    assert_ne!(
        d.computed_style_property(img, "width"),
        "auto",
        "and it has a used width"
    );
    assert_eq!(
        d.computed_style_property(span, "width"),
        "auto",
        "a non-replaced inline does not"
    );

    // `<embed>` and `<object>` ARE `display: inline` in this crate, so they
    // are what actually exercises the replaced-element branch.
    let mut d2 = renderer.load_html("<embed id=e src=x><span id=s2>t</span>", 800.0);
    let embed = d2.get_element_by_id("e").unwrap();
    let span2 = d2.get_element_by_id("s2").unwrap();
    assert_eq!(d2.computed_style_property(embed, "display"), "inline");
    assert_eq!(d2.computed_style_property(span2, "display"), "inline");
    assert_ne!(
        d2.computed_style_property(embed, "width"),
        "auto",
        "a replaced INLINE has a used width"
    );
    assert_eq!(d2.computed_style_property(span2, "width"), "auto");
}

#[test]
fn every_display_variant_serializes_rather_than_falling_through() {
    // ⛔ The `display` arm had a `_ => return None`, so any variant it did not
    // name went to the inline fallback and answered `""`. `<button>` is laid
    // out as a flex box here and did exactly that.
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        "<button id=b></button><table id=t><tr id=r><td id=c>x</td></tr></table>\
         <li id=li></li>",
        800.0,
    );
    for id in ["b", "t", "r", "c", "li"] {
        let e = d.get_element_by_id(id).unwrap();
        let got = d.computed_style_property(e, "display");
        assert!(!got.is_empty(), "#{id} display answered nothing");
        assert!(
            !got.contains(char::is_uppercase),
            "#{id} answered {got:?}, not a CSS keyword"
        );
    }
}

#[test]
fn a_relative_boxs_opposite_edges_mirror_each_other() {
    // ⛔ CSS 2.1 §9.4.3: the two edges are constrained, so `bottom` is
    // `-top`. Measured: `top: 10px; left: -5px` answers
    // `["10px", "-5px", "-10px", "5px"]`. A mutation showed the test above
    // only ever read `top`.
    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        "<div id=r style='position:relative;top:10px;left:-5px'>R</div>\
         <div id=z style='position:relative'>Z</div>",
        800.0,
    );
    let r = d.get_element_by_id("r").unwrap();
    let z = d.get_element_by_id("z").unwrap();
    assert_eq!(
        [
            d.computed_style_property(r, "top"),
            d.computed_style_property(r, "left"),
            d.computed_style_property(r, "bottom"),
            d.computed_style_property(r, "right"),
        ],
        ["10px", "-5px", "-10px", "5px"]
    );
    // With neither edge declared, all four are zero rather than the page
    // position the rect path would have given.
    for edge in ["top", "left", "bottom", "right"] {
        assert_eq!(d.computed_style_property(z, edge), "0px", "{edge}");
    }
}
