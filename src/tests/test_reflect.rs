//! Reflected content attributes — HTML §2.6.
//!
//! The table is only as good as its DEFAULTS: every kind here has a plausible
//! answer for a present value and a surprising one for an absent value, and a
//! suite that checks only the present case passes with every default wrong.
//! So each row below is a pair.
//!
//! The neighbour disagreements get their own test, because that is where a
//! single shared "enumerated with a default" helper goes wrong: `form.method`
//! absent is `"get"` and `input.formMethod` absent is `""`.
//!
//! All measured (`/tmp/webcore-html/refl.html`).

use crate::dom::reflect::Reflected;
use crate::html::parse_html_with_base;
use crate::types::Document;

const PAGE: &str = r##"<img id=i1 src="p.png" alt="A" usemap="#m" ismap decoding=async loading=lazy
      crossorigin=anonymous referrerpolicy=no-referrer sizes="1px">
<img id=i3 src="p.png" srcset="p2.png">
<img id=i2>
<a id=a1 href="x.html" rel="noopener" hreflang="en" type="text/html" target=_blank download="f">L</a>
<a id=a2>bare</a>
<form id=f1 method="POST" enctype="text/plain" novalidate accept-charset="utf-8" target=_self name=nm autocomplete=off rel=noopener></form>
<form id=f2></form>
<input id=n1 maxlength=5 minlength=2 size=10 accept=".png" autocomplete=on dirname=d.dir
       formmethod=post formenctype="text/plain" formnovalidate formtarget=_blank value=v checked alt=ia src=is.png>
<input id=n2>
<iframe id=fr1 src="e.html" name=fn referrerpolicy="no-referrer" allow="camera" allowfullscreen width=10 height=20></iframe>
<iframe id=fr2></iframe>"##;

/// ⛔ `<link>` and `<script>` are HOISTED into the head by the parser, where
/// `getElementById` does not reach — so they are built here instead. That is a
/// legitimate route to the same elements and keeps the fixture from depending
/// on where the parser puts them.
fn page() -> Document {
    let mut d = parse_html_with_base(PAGE, "http://example.com/dir/index.html");
    let body = d.body().unwrap();
    for (id, tag, attrs) in [
        (
            "l1",
            "link",
            &[
                ("rel", "stylesheet"),
                ("href", "s.css"),
                ("media", "screen"),
                ("as", "style"),
                ("crossorigin", "anonymous"),
                ("integrity", "sha"),
                ("disabled", ""),
            ][..],
        ),
        ("l2", "link", &[][..]),
        (
            "s1",
            "script",
            &[
                ("src", "j.js"),
                ("defer", ""),
                ("type", "module"),
                ("nomodule", ""),
            ][..],
        ),
        ("s2", "script", &[][..]),
    ] {
        let e = d.create_element(tag);
        d.set_attribute(e, "id", id);
        for (k, v) in attrs {
            d.set_attribute(e, k, v);
        }
        d.append_child(body, e);
    }
    d
}
fn el(d: &Document, id: &str) -> u32 {
    d.get_element_by_id(id).unwrap()
}

fn s(d: &Document, id: &str, idl: &str) -> String {
    match d.reflect_get(el(d, id), idl) {
        Some(Reflected::Str(v)) => v,
        other => panic!("#{id}.{idl} answered {other:?}, wanted a string"),
    }
}
fn b(d: &Document, id: &str, idl: &str) -> bool {
    d.reflect_get(el(d, id), idl)
        .and_then(|r| r.as_bool())
        .unwrap_or_else(|| panic!("#{id}.{idl} is not a boolean"))
}
fn n(d: &Document, id: &str, idl: &str) -> i64 {
    d.reflect_get(el(d, id), idl)
        .and_then(|r| r.as_long())
        .unwrap_or_else(|| panic!("#{id}.{idl} is not a long"))
}

// ─── the five kinds, each with its absent case beside its present one ───────

#[test]
fn a_string_attribute_is_verbatim_and_empty_when_absent() {
    let d = page();
    for (id, idl, want) in [
        ("i1", "alt", "A"),
        ("i2", "alt", ""),
        ("i1", "useMap", "#m"),
        ("i2", "useMap", ""),
        ("i3", "srcset", "p2.png"),
        ("i2", "srcset", ""),
        ("a1", "rel", "noopener"),
        ("a2", "rel", ""),
        ("a1", "hreflang", "en"),
        ("a2", "hreflang", ""),
        ("a1", "target", "_blank"),
        ("a2", "target", ""),
        ("a1", "download", "f"),
        ("a2", "download", ""),
        ("l1", "media", "screen"),
        ("l2", "media", ""),
        ("l1", "integrity", "sha"),
        ("l2", "integrity", ""),
        ("f1", "acceptCharset", "utf-8"),
        ("f2", "acceptCharset", ""),
        ("n1", "dirName", "d.dir"),
        ("n2", "dirName", ""),
    ] {
        assert_eq!(s(&d, id, idl), want, "#{id}.{idl}");
    }
}

#[test]
fn a_url_attribute_resolves_against_the_base_and_is_empty_when_absent() {
    // ⛔ Absent is `""`, NOT the document URL — the two attributes that DO
    // answer the document URL (`form.action`, `input.formAction`) are
    // deliberately not in the table.
    let d = page();
    assert_eq!(s(&d, "i1", "src"), "http://example.com/dir/p.png");
    assert_eq!(
        s(&d, "i3", "src"),
        "http://example.com/dir/p.png",
        "srcset must not mutate the authored src content attribute"
    );
    assert_eq!(
        d.find_webcore(el(&d, "i3")).unwrap().resolved_src,
        "http://example.com/dir/p2.png",
        "srcset selects the current image candidate separately"
    );
    assert_eq!(s(&d, "a1", "href"), "http://example.com/dir/x.html");
    assert_eq!(s(&d, "l1", "href"), "http://example.com/dir/s.css");
    assert_eq!(s(&d, "s1", "src"), "http://example.com/dir/j.js");
    assert_eq!(s(&d, "fr1", "src"), "http://example.com/dir/e.html");
    for (id, idl) in [
        ("i2", "src"),
        ("a2", "href"),
        ("l2", "href"),
        ("s2", "src"),
        ("fr2", "src"),
    ] {
        assert_eq!(s(&d, id, idl), "", "#{id}.{idl} absent");
    }
}

#[test]
fn a_boolean_attribute_is_its_presence_and_not_its_value() {
    let d = page();
    for (id, idl, want) in [
        ("i1", "isMap", true),
        ("i2", "isMap", false),
        ("l1", "disabled", true),
        ("l2", "disabled", false),
        ("s1", "defer", true),
        ("s2", "defer", false),
        ("s1", "noModule", true),
        ("s2", "noModule", false),
        ("f1", "noValidate", true),
        ("f2", "noValidate", false),
        ("n1", "formNoValidate", true),
        ("n2", "formNoValidate", false),
        ("n1", "defaultChecked", true),
        ("n2", "defaultChecked", false),
        ("fr1", "allowFullscreen", true),
        ("fr2", "allowFullscreen", false),
    ] {
        assert_eq!(b(&d, id, idl), want, "#{id}.{idl}");
    }
    // The VALUE is irrelevant: `ismap="false"` is still present, so still true.
    let mut d = page();
    let i2 = el(&d, "i2");
    d.set_attribute(i2, "ismap", "false");
    assert!(b(&d, "i2", "isMap"), "presence, not truthiness");
}

#[test]
fn a_long_attribute_carries_its_own_missing_value_default() {
    // ⛔ Three different defaults among three neighbours: -1, -1 and 20.
    let d = page();
    assert_eq!(n(&d, "n1", "maxLength"), 5);
    assert_eq!(n(&d, "n2", "maxLength"), -1);
    assert_eq!(n(&d, "n2", "minLength"), -1);
    assert_eq!(n(&d, "n1", "size"), 10);
    assert_eq!(n(&d, "n2", "size"), 20, "not 0 and not -1");
    // A value that is not a number falls back to the default too.
    let mut d = page();
    let n2 = el(&d, "n2");
    d.set_attribute(n2, "maxlength", "abc");
    assert_eq!(n(&d, "n2", "maxLength"), -1);
}

#[test]
fn an_enumerated_attribute_folds_case_and_has_two_separate_defaults() {
    let d = page();
    assert_eq!(s(&d, "i1", "decoding"), "async");
    assert_eq!(s(&d, "i2", "decoding"), "auto", "missing-value default");
    assert_eq!(s(&d, "f1", "method"), "post", "lowercased from POST");
    assert_eq!(s(&d, "i1", "referrerPolicy"), "no-referrer");
    assert_eq!(
        s(&d, "i2", "referrerPolicy"),
        "",
        "missing default is empty here"
    );

    // ⛔ The INVALID-value default is a separate rule from the missing one.
    let mut d = page();
    let f2 = el(&d, "f2");
    d.set_attribute(f2, "method", "sideways");
    assert_eq!(s(&d, "f2", "method"), "get", "invalid → get");
    let i2 = el(&d, "i2");
    d.set_attribute(i2, "referrerpolicy", "sideways");
    assert_eq!(s(&d, "i2", "referrerPolicy"), "", "invalid → empty");
}

#[test]
fn the_invalid_and_missing_defaults_are_separate_even_when_most_enums_share_them() {
    // ⛔ A mutation found this: every enum tested above has the SAME missing
    // and invalid default (`form.method` is get/get, `referrerPolicy` is
    // ""/""), so swapping the two stayed green. `input.formMethod` is the pair
    // where they differ — measured: absent answers `""` and an unrecognised
    // value answers `"get"`.
    let mut d = page();
    let n2 = el(&d, "n2");
    assert_eq!(s(&d, "n2", "formMethod"), "", "absent");
    assert_eq!(s(&d, "n2", "formEnctype"), "");
    d.set_attribute(n2, "formmethod", "sideways");
    d.set_attribute(n2, "formenctype", "sideways");
    assert_eq!(
        s(&d, "n2", "formMethod"),
        "get",
        "invalid is NOT the same as absent"
    );
    assert_eq!(
        s(&d, "n2", "formEnctype"),
        "application/x-www-form-urlencoded"
    );
}

#[test]
fn crossorigin_is_null_when_absent_which_no_other_kind_can_answer() {
    // ⛔ The only nullable one in the set. `""` is a DIFFERENT answer that it
    // can also give, so `None`-vs-empty-string is a real distinction here.
    let d = page();
    assert_eq!(
        d.reflect_get(el(&d, "l2"), "crossOrigin"),
        Some(Reflected::Null)
    );
    assert_eq!(
        d.reflect_get(el(&d, "s2"), "crossOrigin"),
        Some(Reflected::Null)
    );
    assert_eq!(s(&d, "l1", "crossOrigin"), "anonymous");
    // An unrecognised value is the invalid-value default, not null.
    let mut d = page();
    let l2 = el(&d, "l2");
    d.set_attribute(l2, "crossorigin", "sideways");
    assert_eq!(s(&d, "l2", "crossOrigin"), "anonymous");
    // ⛔ And it folds case like the other enumerated kind — a mutation showed
    // the nullable branch had its own uppercase path with nothing testing it.
    d.set_attribute(l2, "crossorigin", "USE-CREDENTIALS");
    assert_eq!(s(&d, "l2", "crossOrigin"), "use-credentials");
}

// ─── the neighbour disagreements ────────────────────────────────────────────

#[test]
fn a_form_and_an_input_disagree_about_the_same_named_defaults() {
    // ⛔ THE reason there is no single shared enumerated helper. Three pairs,
    // same concept, different missing-value default — and every one of them
    // agrees on the PRESENT value, so only the absent case shows it.
    let d = page();
    assert_eq!(s(&d, "f1", "method"), "post");
    assert_eq!(s(&d, "n1", "formMethod"), "post", "they agree when present");
    assert_eq!(s(&d, "f2", "method"), "get");
    assert_eq!(s(&d, "n2", "formMethod"), "", "and disagree when absent");

    assert_eq!(s(&d, "f2", "enctype"), "application/x-www-form-urlencoded");
    assert_eq!(s(&d, "n2", "formEnctype"), "");

    assert_eq!(s(&d, "f2", "autocomplete"), "on");
    assert_eq!(s(&d, "n2", "autocomplete"), "");
}

#[test]
fn encoding_is_a_second_name_for_the_same_attribute() {
    let mut d = page();
    assert_eq!(s(&d, "f1", "enctype"), "text/plain");
    assert_eq!(s(&d, "f1", "encoding"), "text/plain");
    let f1 = el(&d, "f1");
    d.reflect_set(f1, "encoding", Reflected::Str("multipart/form-data".into()));
    assert_eq!(
        s(&d, "f1", "enctype"),
        "multipart/form-data",
        "one attribute, two names"
    );
}

#[test]
fn default_value_and_default_checked_are_the_content_attributes() {
    let d = page();
    assert_eq!(s(&d, "n1", "defaultValue"), "v");
    assert_eq!(s(&d, "n2", "defaultValue"), "");
    assert!(b(&d, "n1", "defaultChecked"));
}

#[test]
fn an_iframes_width_is_a_string_where_an_images_would_be_a_number() {
    // ⛔ Measured `""` for an absent `iframe.width`, not `0`.
    let d = page();
    assert_eq!(s(&d, "fr1", "width"), "10");
    assert_eq!(s(&d, "fr2", "width"), "");
    assert_eq!(s(&d, "fr2", "height"), "");
}

// ─── setting ────────────────────────────────────────────────────────────────

#[test]
fn setting_writes_through_to_the_content_attribute() {
    let mut d = page();
    let i2 = el(&d, "i2");
    d.reflect_set(i2, "alt", Reflected::Str("hello".into()));
    assert_eq!(d.get_attribute(i2, "alt").as_deref(), Some("hello"));
    assert_eq!(s(&d, "i2", "alt"), "hello");

    d.reflect_set(i2, "isMap", Reflected::Bool(true));
    assert_eq!(
        d.get_attribute(i2, "ismap").as_deref(),
        Some(""),
        "a boolean attribute"
    );
    d.reflect_set(i2, "isMap", Reflected::Bool(false));
    assert!(!d.has_attribute(i2, "ismap"), "false REMOVES it");

    let n2 = el(&d, "n2");
    d.reflect_set(n2, "maxLength", Reflected::Long(7));
    assert_eq!(d.get_attribute(n2, "maxlength").as_deref(), Some("7"));
    assert_eq!(n(&d, "n2", "maxLength"), 7);

    // Setting a nullable enumerated to null takes it back to answering null.
    let l1 = el(&d, "l1");
    d.reflect_set(l1, "crossOrigin", Reflected::Null);
    assert_eq!(d.reflect_get(l1, "crossOrigin"), Some(Reflected::Null));
}

#[test]
fn an_element_without_a_reflected_name_answers_none_rather_than_empty() {
    // ⛔ `None` (there is no such reflected attribute) is a different answer
    // from `Some(Str(""))` (there is, and it is empty) and from
    // `Some(Null)`. Collapsing the three would make the table unfalsifiable.
    let d = page();
    assert_eq!(d.reflect_get(el(&d, "i1"), "noSuchThing"), None);
    assert_eq!(
        d.reflect_get(el(&d, "i1"), "method"),
        None,
        "img has no `method`"
    );
    assert_eq!(
        d.reflect_get(el(&d, "f1"), "alt"),
        None,
        "form has no `alt`"
    );
    assert!(d.reflect_get(el(&d, "i1"), "alt").is_some());
}

#[test]
fn the_three_members_that_are_not_reflection_are_absent_from_the_table() {
    // ⛔ Pinned so nobody "completes" the table with them later. Each looks
    // like reflection and is not: `script.async` answers true with no
    // attribute at all (the force-async flag), `img.width` answers the used
    // width, and `form.action` answers the DOCUMENT URL when absent.
    let d = page();
    assert_eq!(d.reflect_get(el(&d, "s1"), "async"), None);
    assert_eq!(d.reflect_get(el(&d, "i1"), "width"), None);
    assert_eq!(d.reflect_get(el(&d, "f1"), "action"), None);
    assert_eq!(d.reflect_get(el(&d, "n1"), "formAction"), None);
}
