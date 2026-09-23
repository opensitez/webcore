//! Document metadata, the doctype, quirks mode and the document collections.
//!
//! Read off Chrome first (`/tmp/webcore-html/doc1.html` and `dt/d*.html`).
//! The quirks table below is the measured one, not a paraphrase of the spec
//! prose — the two disagree about `-//W3C//DTD HTML 4.0//EN`, which is NOT in
//! the quirks list and which a "4.0 means old means quirks" reading gets wrong.

use crate::html::doctype::QuirksMode;
use crate::html::parse_html;
use crate::types::Document;

fn doc(html: &str) -> Document {
    parse_html(html)
}

// ─── the doctype node ───────────────────────────────────────────────────────

#[test]
fn the_doctype_is_a_node_with_a_name_and_two_identifiers() {
    let d = doc(
        r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd"><html><body></body></html>"#,
    );
    let dt = d.doctype().expect("a doctype node");
    assert_eq!(
        d.doctype_name().as_deref(),
        Some("html"),
        "the NAME is lowercased"
    );
    assert_eq!(
        d.doctype_public_id().as_deref(),
        Some("-//W3C//DTD HTML 4.01//EN")
    );
    assert_eq!(
        d.doctype_system_id().as_deref(),
        Some("http://www.w3.org/TR/html4/strict.dtd"),
        "the identifiers keep their CASE — only the name is folded"
    );
    assert_eq!(d.node_type(dt), 10);
    assert_eq!(
        d.node_name(dt),
        "html",
        "nodeName IS the name, not uppercased"
    );
}

#[test]
fn an_absent_identifier_is_the_empty_string_not_a_null() {
    let d = doc("<!DOCTYPE html><html><body></body></html>");
    assert!(d.doctype().is_some());
    assert_eq!(d.doctype_public_id().as_deref(), Some(""));
    assert_eq!(d.doctype_system_id().as_deref(), Some(""));
}

#[test]
fn a_document_with_no_doctype_has_none() {
    let d = doc("<html><body></body></html>");
    assert_eq!(d.doctype(), None);
    assert_eq!(d.doctype_name(), None);
}

#[test]
fn the_doctype_is_the_documents_first_child_and_the_document_is_its_parent() {
    // ⛔ `document.childNodes` PANICKED before this landed: the document is not
    // an arena node, and `arena.children` asserts on an id it never issued.
    // The same unguarded-`get` shape as the shadow ids.
    let d = doc("<!DOCTYPE html><html><body><p>x</p></body></html>");
    let dt = d.doctype().unwrap();
    let kids = d.child_nodes(d.document_node());
    assert_eq!(kids.len(), 2, "doctype then documentElement, got {kids:?}");
    assert_eq!(kids[0], dt);
    assert_eq!(d.first_child(d.document_node()), Some(dt));
    assert_eq!(d.parent_node(dt), d.document_node());
    assert_eq!(
        d.node_type(kids[1]),
        1,
        "the second child is the document element"
    );
}

#[test]
fn a_document_without_a_doctype_has_only_the_document_element() {
    let d = doc("<html><body><p>x</p></body></html>");
    let kids = d.child_nodes(d.document_node());
    assert_eq!(kids.len(), 1, "got {kids:?}");
    assert_eq!(d.node_type(kids[0]), 1);
}

#[test]
fn only_the_first_doctype_counts() {
    let d = doc("<!DOCTYPE html><!DOCTYPE foo><html><body></body></html>");
    assert_eq!(d.doctype_name().as_deref(), Some("html"));
    assert_eq!(
        d.child_nodes(d.document_node()).len(),
        2,
        "not one node per doctype"
    );
}

// ─── quirks mode ────────────────────────────────────────────────────────────

#[test]
fn compat_mode_matches_chrome_across_the_measured_doctypes() {
    // Every row measured. `d106` is the interesting one: HTML 4.0 (not 4.01,
    // not Transitional) is NOT in the quirks list and reports CSS1Compat.
    let cases: &[(&str, &str)] = &[
        ("<!DOCTYPE html>", "CSS1Compat"),
        ("", "BackCompat"),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">"#,
            "CSS1Compat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">"#,
            "BackCompat",
        ),
        (
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Strict//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd">"#,
            "CSS1Compat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">"#,
            "BackCompat",
        ),
        (
            r#"<!DOCTYPE html SYSTEM "about:legacy-compat">"#,
            "CSS1Compat",
        ),
        ("<!doctype HTML>", "CSS1Compat"),
        ("<!DOCTYPE foo>", "BackCompat"),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">"#,
            "CSS1Compat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Frameset//EN">"#,
            "BackCompat",
        ),
        (
            r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">"#,
            "CSS1Compat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//IETF//DTD HTML 2.0//EN">"#,
            "BackCompat",
        ),
        (
            r#"<!DOCTYPE html SYSTEM "http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd">"#,
            "BackCompat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.0//EN">"#,
            "CSS1Compat",
        ),
        (
            r#"<!DOCTYPE HTML PUBLIC "-//w3c//dtd html 4.01 transitional//en">"#,
            "BackCompat",
        ),
    ];
    for (dt, want) in cases {
        let d = doc(&format!("{dt}<html><body></body></html>"));
        assert_eq!(d.compat_mode(), *want, "{dt}");
    }
}

#[test]
fn limited_quirks_is_a_third_state_that_compat_mode_cannot_see() {
    // ⛔ White-box on purpose. Both of these report "CSS1Compat", the same as
    // a plain `<!DOCTYPE html>` — so a test that only reads `compatMode` would
    // leave `LimitedQuirks` write-only and could not tell it from `NoQuirks`.
    let limited = [
        r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" "http://www.w3.org/TR/html4/loose.dtd">"#,
        r#"<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd">"#,
        r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD XHTML 1.0 Frameset//EN" "x">"#,
    ];
    for dt in limited {
        let d = doc(&format!("{dt}<html><body></body></html>"));
        assert_eq!(d.quirks, QuirksMode::LimitedQuirks, "{dt}");
        assert_eq!(d.compat_mode(), "CSS1Compat", "{dt}");
    }
    assert_eq!(
        doc("<!DOCTYPE html><html></html>").quirks,
        QuirksMode::NoQuirks
    );
    assert_eq!(doc("<html></html>").quirks, QuirksMode::Quirks);
}

#[test]
fn the_system_identifier_flips_the_html_401_pair_between_two_modes() {
    // The single rule that needs both halves of the doctype at once.
    for public in [
        "-//W3C//DTD HTML 4.01 Transitional//EN",
        "-//W3C//DTD HTML 4.01 Frameset//EN",
    ] {
        let without = doc(&format!(
            r#"<!DOCTYPE HTML PUBLIC "{public}"><html></html>"#
        ));
        assert_eq!(
            without.quirks,
            QuirksMode::Quirks,
            "{public} without a system id"
        );
        let with = doc(&format!(
            r#"<!DOCTYPE HTML PUBLIC "{public}" "s"><html></html>"#
        ));
        assert_eq!(with.quirks, QuirksMode::LimitedQuirks, "{public} with one");
    }
}

#[test]
fn a_malformed_doctype_still_names_the_document_and_forces_quirks() {
    // Recovery, not rejection: the spec's answer to a broken doctype is the
    // force-quirks flag, not a discarded token.
    let d = doc("<!DOCTYPE><html></html>");
    assert_eq!(d.quirks, QuirksMode::Quirks);
    let d = doc("<!DOCTYPE html PUBLIC><html></html>");
    assert_eq!(d.quirks, QuirksMode::Quirks);
    assert_eq!(
        d.doctype_name().as_deref(),
        Some("html"),
        "the name survived"
    );
    let d = doc(r#"<!DOCTYPE html SOMETHING "x"><html></html>"#);
    assert_eq!(d.quirks, QuirksMode::Quirks, "neither PUBLIC nor SYSTEM");
}

#[test]
fn single_quoted_identifiers_parse_too() {
    let d = doc(r#"<!DOCTYPE html PUBLIC '-//W3C//DTD HTML 4.01//EN' 'sys'><html></html>"#);
    assert_eq!(
        d.doctype_public_id().as_deref(),
        Some("-//W3C//DTD HTML 4.01//EN")
    );
    assert_eq!(d.doctype_system_id().as_deref(), Some("sys"));
}

// ─── metadata ───────────────────────────────────────────────────────────────

#[test]
fn the_metadata_members_answer_what_this_crate_can_honestly_say() {
    let d = doc("<!DOCTYPE html><html><body></body></html>");
    assert_eq!(
        d.character_set(),
        "UTF-8",
        "a Rust &str is UTF-8 by construction"
    );
    assert_eq!(d.content_type(), "text/html");
    assert_eq!(
        d.ready_state(),
        "complete",
        "parse_html RETURNS a finished document"
    );
    assert_eq!(d.visibility_state(), "visible");
    assert!(!d.document_hidden());
    assert_eq!(d.referrer(), "");
    d.capture_events();
    d.release_events();
}

#[test]
fn meta_color_scheme_sets_the_document_default_scheme() {
    let d = doc(r#"<!doctype html><html><head>
            <meta name="color-scheme" content="dark light">
        </head><body></body></html>"#);
    assert_eq!(d.root.style.color_scheme, "dark light");
}

#[test]
fn author_color_scheme_overrides_meta_color_scheme() {
    let d = doc(r#"<!doctype html><html><head>
            <meta name="color-scheme" content="dark">
            <style>html { color-scheme: light; }</style>
        </head><body></body></html>"#);
    assert_eq!(d.root.style.color_scheme, "light");
}

#[test]
fn invalid_meta_color_scheme_is_ignored() {
    let d = doc(r#"<!doctype html><html><head>
            <meta name="color-scheme" content="dark sepia">
        </head><body></body></html>"#);
    assert_eq!(d.root.style.color_scheme, "normal");
}

// ─── the collections ────────────────────────────────────────────────────────

const PAGE: &str = r#"<!DOCTYPE html><html><body>
<a id=a1 href="x.html" name="anchor1">link</a>
<a id=a2 name="anchor2">named only</a>
<a id=a3>bare</a>
<area id=ar1 href="y.html">
<area id=ar2>
<img id=i1 src="p.png"><img id=i2>
<form id=f1></form><form id=f2></form>
<embed id=e1 src="q.swf">
<input name=dup2 id=n1><input name=dup2 id=n2>
<div name=dup2 id=n3></div>
</body></html>"#;

fn ids(d: &Document, list: Vec<u32>) -> Vec<String> {
    list.into_iter()
        .map(|id| d.get_attribute(id, "id").unwrap_or_default())
        .collect()
}

#[test]
fn links_are_a_and_area_and_only_with_an_href() {
    let d = doc(PAGE);
    // ⛔ `a3` (no href) and `ar2` (no href) are excluded; `<area>` is included.
    assert_eq!(ids(&d, d.links()), ["a1", "ar1"]);
}

#[test]
fn anchors_are_a_with_a_name_which_is_a_different_question_from_links() {
    let d = doc(PAGE);
    // `a1` has BOTH an href and a name, so it is in both lists; `a3` is in
    // neither. Nothing here is a subset of the other.
    assert_eq!(ids(&d, d.anchors()), ["a1", "a2"]);
}

#[test]
fn images_forms_scripts_and_embeds_are_plain_tag_filters() {
    let d = doc(PAGE);
    assert_eq!(
        ids(&d, d.images()),
        ["i1", "i2"],
        "an img with no src still counts"
    );
    assert_eq!(ids(&d, d.forms()), ["f1", "f2"]);
    assert_eq!(ids(&d, d.embeds()), ["e1"]);
    assert_eq!(d.plugins(), d.embeds(), "plugins IS embeds");
    assert!(d.applets().is_empty(), "applets is defined as always empty");
}

#[test]
fn get_elements_by_name_matches_any_element_not_just_form_controls() {
    let d = doc(PAGE);
    // ⛔ The `<div name=dup2>` is in the list. Restricting this to controls is
    // the plausible-and-wrong implementation.
    assert_eq!(ids(&d, d.get_elements_by_name("dup2")), ["n1", "n2", "n3"]);
    assert!(d.get_elements_by_name("nope").is_empty());
}

#[test]
fn get_elements_by_tag_name_ns_treats_html_elements_as_xhtml() {
    let d = doc(PAGE);
    const XHTML: &str = "http://www.w3.org/1999/xhtml";
    assert_eq!(
        d.get_elements_by_tag_name_ns(XHTML, "a").len(),
        3,
        "all three <a>"
    );
    assert_eq!(d.get_elements_by_tag_name_ns("*", "a").len(), 3);
    assert_eq!(
        d.get_elements_by_tag_name_ns("http://example.com/ns", "a")
            .len(),
        0
    );
    assert!(d.get_elements_by_tag_name_ns(XHTML, "*").len() > 5);
}

#[test]
fn the_collections_are_snapshots_where_chromes_are_live() {
    // Measured in Chrome: `document.links` went 2 → 3 when an `<a href>` was
    // appended, WITHOUT re-reading the collection. This returns a Vec, so it
    // does not. Pinned so the difference is a decision on the record rather
    // than something a caller discovers.
    let mut d = doc(PAGE);
    let before = d.links();
    let body = d.body().unwrap();
    let a = d.create_element("a");
    d.set_attribute(a, "href", "z.html");
    d.append_child(body, a);
    assert_eq!(before.len(), 2, "the snapshot did not grow");
    assert_eq!(d.links().len(), 3, "a fresh read sees the new link");
}

#[test]
fn a_doctype_inside_the_body_is_ignored_outright() {
    // Measured: `document.doctype` is null and the mode stays quirks. The
    // document-level loop is the ONLY place a doctype counts — recording one
    // from element content would invent a doctype the page does not have.
    let d = doc("<html><body><!DOCTYPE foo></body></html>");
    assert_eq!(d.doctype(), None);
    assert_eq!(d.compat_mode(), "BackCompat");
    assert_eq!(d.child_nodes(d.document_node()).len(), 1);
}
