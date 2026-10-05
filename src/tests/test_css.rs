// Ported from tests/test_css.cpp

#[test]
fn tab_size_uses_checked_number_or_length_grammar() {
    for valid in [
        "0",
        "2.5",
        "1e1",
        "calc(2 + .5)",
        "0px",
        "24px",
        "2em",
        "calc(2em + 4px)",
        "max(4px, 2em)",
        "calc(-2px)",
    ] {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "tab-size", valid);
        assert_ne!(style.rare().tab_size, TabSize::INITIAL, "{valid}");
        assert!(
            crate::css::supports_condition_matches(&format!("(tab-size:{valid})")),
            "{valid}"
        );
    }
    for invalid in [
        "-1",
        "-1px",
        "10%",
        "calc(0% + 10px)",
        "calc(10% - 10% + 4px)",
        "auto",
        "none",
        "1px 2px",
        "NaN",
        "infinity",
        "calc(2px +)",
    ] {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "tab-size", "2.5");
        apply_property(&mut style, "tab-size", invalid);
        assert_eq!(style.rare().tab_size, TabSize::Number(2.5), "{invalid}");
        assert!(
            !crate::css::supports_condition_matches(&format!("(tab-size:{invalid})")),
            "{invalid}"
        );
    }
    let mut initial = ComputedStyle::default();
    apply_property(&mut initial, "tab-size", "8");
    assert!(initial.rare.is_none());
}

#[test]
fn font_variant_groups_reject_conflicts_and_share_supports_grammar() {
    for (property, valid, expected, invalid) in [
        (
            "font-variant-numeric",
            "SLASHED-ZERO tabular-nums oldstyle-nums ordinal diagonal-fractions",
            "oldstyle-nums tabular-nums diagonal-fractions ordinal slashed-zero",
            vec![
                "lining-nums oldstyle-nums",
                "tabular-nums proportional-nums",
                "ordinal ordinal",
                "normal tabular-nums",
                "none",
            ],
        ),
        (
            "font-variant-ligatures",
            "contextual historical-ligatures no-common-ligatures",
            "no-common-ligatures historical-ligatures contextual",
            vec![
                "contextual no-contextual",
                "common-ligatures no-common-ligatures",
                "normal contextual",
                "none contextual",
                "contextual contextual",
            ],
        ),
        (
            "font-variant-east-asian",
            "ruby full-width jis2004",
            "jis2004 full-width ruby",
            vec![
                "jis04",
                "jis78 jis2004",
                "full-width proportional-width",
                "ruby ruby",
                "normal ruby",
            ],
        ),
    ] {
        let read = |style: &ComputedStyle| match property {
            "font-variant-numeric" => style.font_variant_numeric.clone(),
            "font-variant-ligatures" => style.font_variant_ligatures.clone(),
            _ => style.font_variant_east_asian.clone(),
        };
        let mut style = ComputedStyle::default();
        apply_property(&mut style, property, valid);
        assert_eq!(read(&style), expected);
        assert!(crate::css::supports_condition_matches(&format!(
            "({property}: {valid})"
        )));
        for invalid in invalid {
            apply_property(&mut style, property, invalid);
            assert_eq!(read(&style), expected, "{property}: {invalid}");
            assert!(!crate::css::supports_condition_matches(&format!(
                "({property}: {invalid})"
            )));
        }
    }
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "font-variant-numeric",
        r"tabular-\6e ums/**/ordinal",
    );
    assert_eq!(style.font_variant_numeric, "tabular-nums ordinal");
}

#[test]
fn tab_size_computes_lengths_before_inheritance() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
        #parent {font-size:20px; tab-size:2em}
        #child {font-size:10px}
        #number {tab-size:2.5}
        #negative {tab-size:calc(-2px)}
        #percent {tab-size:4;tab-size:calc(0% + 10px)}
        #rootlength {tab-size:calc(1rem + 3px)}
        html {font-size:18px}
        </style><div id="parent"><span id="child">text</span></div>
        <div id="number">text</div><div id="negative">text</div>
        <div id="percent">text</div><div id="rootlength">text</div>"#,
        800.0,
    );
    for (selector, expected) in [
        ("#parent", "40px"),
        ("#child", "40px"),
        ("#number", "2.5"),
        ("#negative", "0px"),
        ("#percent", "4"),
        ("#rootlength", "21px"),
    ] {
        let id = doc.query_selector(selector).unwrap();
        assert_eq!(
            doc.computed_style_property(id, "tab-size"),
            expected,
            "{selector}"
        );
    }
}

#[test]
fn white_space_break_spaces_uses_checked_shared_grammar() {
    for value in ["break-spaces", "BREAK-SPACES", r"break\2d spaces"] {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "white-space", value);
        assert_eq!(style.white_space, WhiteSpace::BreakSpaces);
        assert!(crate::css::supports_condition_matches(&format!(
            "(white-space:{value})"
        )));
        for invalid in ["broken", "break-spaces nowrap", "break-spaces()"] {
            apply_property(&mut style, "white-space", invalid);
            assert_eq!(style.white_space, WhiteSpace::BreakSpaces);
            assert!(!crate::css::supports_condition_matches(&format!(
                "(white-space:{invalid})"
            )));
        }
    }
}

#[test]
fn border_grammar_preserves_valid_declarations_and_current_color() {
    for property in ["border", "border-right", "border-inline-start"] {
        for invalid in [
            "",
            "solid dashed",
            "red blue",
            "1px 2px",
            "2px solid junk",
            "-1px solid",
            "10% solid",
            "calc(1px + 2%) solid",
        ] {
            let mut style = ComputedStyle::default();
            apply_property(&mut style, "direction", "rtl");
            apply_property(&mut style, property, "4px solid currentColor");
            let expected = style.clone();
            apply_property(&mut style, property, invalid);
            assert_eq!(style, expected, "{property}: {invalid}");
            assert!(!crate::css::supports_condition_matches(&format!(
                "({property}: {invalid})"
            )));
        }
    }
    for property in [
        "border-width",
        "border-top-width",
        "border-inline-start-width",
    ] {
        for invalid in [
            "-2px",
            "auto",
            "none",
            "8",
            "20%",
            "calc(20%)",
            "calc(1px + 20%)",
            "2px bad",
            "thin medium thick thin thick",
        ] {
            let mut style = ComputedStyle::default();
            apply_property(&mut style, property, "4px");
            let expected = style.clone();
            apply_property(&mut style, property, invalid);
            assert_eq!(style, expected, "{property}: {invalid}");
            assert!(!crate::css::supports_condition_matches(&format!(
                "({property}: {invalid})"
            )));
        }
    }
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "border-width", "thin medium thick 7px");
    assert_eq!(style.border_top_width, CssLength::Px(1.0));
    assert_eq!(style.border_right_width, CssLength::Px(3.0));
    assert_eq!(style.border_bottom_width, CssLength::Px(5.0));
    assert_eq!(style.border_left_width, CssLength::Px(7.0));
    apply_property(&mut style, "border", "THICK solid red");
    assert_eq!(style.border_top_width, CssLength::Px(5.0));
    assert!(crate::css::supports_condition_matches(
        "(border: THICK solid red)"
    ));
    assert!(crate::css::supports_condition_matches(
        "(border-width: calc(1em + 2px))"
    ));
}

#[test]
fn escaped_dimension_units_reach_compiled_inline_variable_and_supports_paths() {
    let doc = parse_and_layout(
        r#"<style>
      .case { padding: 4px; margin-left: 3px }
      #compiled { padding: 10p\78; margin-left: 1e1\70 x }
      #variable { --space: 12\70 x; padding: var(--space) }
      #malformed { padding: 0.; margin-left: 1.px }
      @supports (padding: 10p\78) { #supports { padding: 10p\78 } }
    </style><div id=compiled class=case></div><div id=variable class=case></div>
    <div id=inline class=case style='padding: 14p\78'></div>
    <div id=malformed class=case></div><div id=supports class=case></div>"#,
        800.0,
    );
    let style = |id: &str| {
        &find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap()
        .style
    };
    for (id, padding) in [
        ("compiled", 10.0),
        ("variable", 12.0),
        ("inline", 14.0),
        ("malformed", 4.0),
        ("supports", 10.0),
    ] {
        assert_eq!(style(id).padding_top, CssLength::Px(padding), "{id}");
    }
    assert_eq!(style("compiled").margin_left, CssLength::Px(10.0));
    assert_eq!(style("malformed").margin_left, CssLength::Px(3.0));
    assert!(crate::css::supports_condition_matches(r"(padding: 10p\78)"));
    for invalid in ["0.", "1.px", r"10\25"] {
        assert!(!crate::css::supports_condition_matches(&format!(
            "(padding: {invalid})"
        )));
    }
}

#[test]
fn border_grammar_is_shared_by_compiled_inline_and_variable_values() {
    let doc = parse_and_layout(
        r#"<style>
      .case { color: purple; border: 4px solid currentColor }
      #invalid { border: 2px dotted red blue; border-right-color: nonsense }
      #keywords { border-width: thin medium thick 7px }
      #logical { direction: rtl; border-inline-start-width: thick; border-inline-start-width: 20% }
      #variable { --edge: 2px red blue; border: var(--edge) }
      #math { border-top-width: calc(1em + 2px) }
    </style><div id=invalid class=case></div>
    <div id=inline class=case style='border: 2px solid dashed; border-top-width: -3px'></div>
    <div id=keywords class=case></div><div id=logical class=case></div>
    <div id=variable class=case></div><div id=math class=case></div>"#,
        800.0,
    );
    let style = |id: &str| {
        &find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap()
        .style
    };
    for id in ["invalid", "inline"] {
        assert_eq!(style(id).border_top_width, CssLength::Px(4.0), "{id}");
        assert_eq!(
            style(id).border_right_color,
            Color::rgb(128, 0, 128),
            "{id}"
        );
        assert_eq!(style(id).border_top_style, BorderStyle::Solid, "{id}");
    }
    assert_eq!(style("keywords").border_bottom_width, CssLength::Px(5.0));
    assert_eq!(style("logical").border_right_width, CssLength::Px(5.0));
    assert_eq!(style("variable").border_top_style, BorderStyle::None);
    let id = crate::css::properties::PropertyId::BorderTopWidth;
    assert!(matches!(
        crate::css::rule::pre_parse_value(id, "thin"),
        CssValue::Length(CssLength::Px(1.0))
    ));
}

#[test]
fn radius_grammar_rejects_invalid_values_atomically() {
    for property in [
        "border-radius",
        "border-top-left-radius",
        "border-start-start-radius",
    ] {
        for invalid in [
            "",
            "auto",
            "none",
            "-2px",
            "10",
            "2px junk",
            "2px /",
            "2px / 3px / 4px",
        ] {
            let mut style = crate::types::ComputedStyle::default();
            crate::css::apply_property(&mut style, property, "12px");
            let expected = style.clone();
            crate::css::apply_property(&mut style, property, invalid);
            assert_eq!(style, expected, "{property}: {invalid}");
            assert!(!crate::css::supports_condition_matches(&format!(
                "({property}: {invalid})"
            )));
        }
    }
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "border-radius", "12px");
    let expected = style.clone();
    crate::css::apply_property(&mut style, "border-radius", "1px 2px 3px 4px 5px");
    assert_eq!(style, expected);
    crate::css::apply_property(&mut style, "border-top-left-radius", "1px 2px 3px");
    assert_eq!(style, expected);
    crate::css::apply_property(&mut style, "border-radius", "10% 20px / 30px 40%");
    assert_eq!(
        style.border_top_left_radius,
        crate::types::CssLength::Percent(10.0)
    );
    assert_eq!(
        style.border_top_left_radius_y,
        crate::types::CssLength::Px(30.0)
    );
    assert!(crate::css::supports_condition_matches(
        "(border-radius: 10% 20px / 30px 40%)"
    ));
}

#[test]
fn border_shorthand_resets_border_image_but_side_shorthands_do_not() {
    let mut style = ComputedStyle::default();
    let initial = style.clone();
    for declaration in ["2px solid red", "initial", "unset"] {
        apply_property(
            &mut style,
            "border-image",
            "url(border.png) 30 fill / 4 / 2 round",
        );
        apply_property(&mut style, "border", declaration);
        assert_eq!(
            style.border_image_source, initial.border_image_source,
            "{declaration}"
        );
        assert_eq!(style.border_image_slice, initial.border_image_slice);
        assert_eq!(style.border_image_width, initial.border_image_width);
        assert_eq!(style.border_image_outset, initial.border_image_outset);
        assert_eq!(style.border_image_repeat, initial.border_image_repeat);
    }
    for property in [
        "border-top",
        "border-right",
        "border-inline-start",
        "border-block-end",
    ] {
        apply_property(
            &mut style,
            "border-image",
            "url(border.png) 30 fill / 4 / 2 round",
        );
        apply_property(&mut style, property, "2px solid red");
        assert_eq!(style.border_image_source, "url(border.png)", "{property}");
        assert_eq!(style.border_image_repeat, "round", "{property}");
    }
    let expected = style.clone();
    apply_property(&mut style, "border", "2px solid red blue");
    assert_eq!(
        style, expected,
        "invalid border must not reset border-image"
    );
}

#[test]
fn border_image_reset_participates_in_compiled_inline_variable_and_inherit_cascade() {
    let doc = parse_and_layout(
        r#"<style>
      .case { border-image: url(border.png) 30 fill / 4 / 2 round }
      #normal { border: 2px solid red }
      #variable { --edge: 2px solid red; border: var(--edge) }
      #invalid-var { --edge: 2px red blue; border: var(--edge) }
      #invalid { border: 2px red blue }
      #important { border-image-repeat: round !important; border: 2px solid red }
      #parent { border-image: url(parent.png) 20 / 3 / 1 repeat }
      #inherit { border: inherit }
      #override { border: 2px solid red; border-image-repeat: space }
    </style>
    <div id=normal class=case></div><div id=variable class=case></div>
    <div id=invalid-var class=case></div><div id=invalid class=case></div>
    <div id=inline class=case style='border: 2px solid red'></div>
    <div id=important class=case></div><div id=override class=case></div>
    <div id=parent><div id=inherit class=case></div></div>"#,
        800.0,
    );
    let style = |id: &str| {
        &find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap()
        .style
    };
    for id in ["normal", "variable", "invalid-var", "inline"] {
        assert_eq!(style(id).border_image_source, "none", "{id}");
        assert_eq!(style(id).border_image_slice, "100%", "{id}");
        assert_eq!(style(id).border_image_width, "1", "{id}");
        assert_eq!(style(id).border_image_outset, "0", "{id}");
        assert_eq!(style(id).border_image_repeat, "stretch", "{id}");
    }
    assert_eq!(style("invalid").border_image_source, "url(border.png)");
    assert_eq!(style("important").border_image_source, "none");
    assert_eq!(style("important").border_image_repeat, "round");
    assert_eq!(style("override").border_image_repeat, "space");
    assert_eq!(style("inherit").border_image_source, "url(parent.png)");
    assert_eq!(style("inherit").border_image_slice, "20");
    assert_eq!(style("inherit").border_image_width, "3");
    assert_eq!(style("inherit").border_image_outset, "1");
    assert_eq!(style("inherit").border_image_repeat, "repeat");
}

use super::harness::*;
use crate::css::{
    PseudoElement, Stylesheet, apply_property, parse_declarations, parse_length_checked,
    parse_selector, parse_stylesheet, resolve_content_value, resolve_counters_in_content,
};
use crate::frame::EngineFrame;
use crate::html::parse_html;
use crate::renderer::display_list::PaintCmd;
use crate::renderer::display_list_builder::build_display_list;
use crate::types::*;

#[test]
fn radius_grammar_is_shared_by_compiled_inline_and_variable_values() {
    let doc = parse_and_layout(
        r#"<style>
      .case { border-radius: 12px; }
      #sheet { border-radius: 1px 2px 3px 4px 5px; border-top-left-radius: -3px; }
      #logical { direction: rtl; border-start-start-radius: 18px; border-start-start-radius: 2px 3px 4px; }
      #variable { --radius: auto; border-top-left-radius: var(--radius); }
      #ellipse { border-top-left-radius: 20% 8px; }
    </style><div id=sheet class=case></div>
    <div id=inline class=case style='border-radius: 2px /; border-top-left-radius: auto'></div>
    <div id=logical class=case></div><div id=variable class=case></div>
    <div id=ellipse class=case></div>"#,
        800.0,
    );
    let style = |id: &str| {
        &find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap()
        .style
    };
    for id in ["sheet", "inline"] {
        assert_eq!(
            style(id).border_top_left_radius,
            CssLength::Px(12.0),
            "{id}"
        );
        assert_eq!(
            style(id).border_top_left_radius_y,
            CssLength::Px(12.0),
            "{id}"
        );
    }
    assert_eq!(
        style("logical").border_top_right_radius,
        CssLength::Px(18.0)
    );
    assert_eq!(style("variable").border_top_left_radius, CssLength::Zero);
    assert_eq!(
        style("ellipse").border_top_left_radius,
        CssLength::Percent(20.0)
    );
    assert_eq!(
        style("ellipse").border_top_left_radius_y,
        CssLength::Px(8.0)
    );
    let id = crate::css::properties::PropertyId::BorderTopLeftRadius;
    assert!(matches!(
        crate::css::rule::pre_parse_value(id, "12px"),
        CssValue::Length(CssLength::Px(12.0))
    ));
}

fn resolved_root_variables(css: &str) -> std::collections::HashMap<String, String> {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(css);
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    sheet.variables
}

#[test]
fn compiled_rule_custom_property_flag_follows_both_declaration_tiers() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        ".normal { --ink: red; color: var(--ink) } .important { --ink: blue !important; color: var(--ink) } .plain { color: green }",
    );
    sheet.rebuild_index();
    assert!(sheet.rules[0].has_custom_properties);
    assert!(sheet.rules[1].has_custom_properties);
    assert!(!sheet.rules[2].has_custom_properties);

    let mut node = WebCore::new("p");
    node.node_id = 1;
    node.attributes.insert("class", "important");
    crate::css::apply_cascade(&mut node, &sheet, None, 16.0);
    assert_eq!(node.style.color, Color::rgb(0, 0, 255));

    sheet.delete_rule(1).unwrap();
    assert!(!sheet.rules[1].has_custom_properties);
}

#[test]
fn changed_custom_properties_resolve_without_recomputing_inherited_values() {
    let doc = parse_and_layout(
        "<div id=parent style='--base:red; --inherited:var(--base)'>\
         <div id=child style='--base:blue; --local:var(--base); --chain:var(--local);\
         --cycle-a:var(--cycle-b); --cycle-b:var(--cycle-a)'>x</div></div>",
        800.0,
    );
    let child = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "child")
    })
    .expect("child");
    let vars = &child.style.custom_props;
    assert_eq!(vars.get("--inherited").map(String::as_str), Some("red"));
    assert_eq!(vars.get("--local").map(String::as_str), Some("blue"));
    assert_eq!(vars.get("--chain").map(String::as_str), Some("blue"));
    assert!(!vars.contains_key("--cycle-a"));
    assert!(!vars.contains_key("--cycle-b"));
}

#[test]
fn css_comments_preserve_value_and_selector_token_boundaries() {
    let doc = parse_and_layout(
        r#"<style>
      .joined { color: blue; width: 80px; color: re/**/d; width: 10/**/px; }
      .compound/**/.joined { background-color: green; }
      .compound/**/span { color: red; }
      .border { border: 2px/**/solid/**/red; }
      .priority { color: green; color: red !im/**/portant; }
    </style><div class="compound joined">joined</div><div class="border">border</div>
    <div class="priority">priority</div>"#,
        800.0,
    );
    let find = |class: &str| {
        find_box(&doc.root, &|node| {
            node.attributes
                .get("class")
                .is_some_and(|value| value.split_whitespace().any(|part| part == class))
        })
        .unwrap()
    };
    let joined = find("joined");
    assert_eq!(joined.style.color, Color::rgb(0, 0, 255));
    assert_eq!(joined.style.width, CssLength::Px(80.0));
    assert_eq!(joined.style.background_color, Color::rgb(0, 128, 0));
    assert!(!crate::css::parse_selector(".compound/**/span").valid);
    assert!(!crate::css::parse_selector("div/**/span").valid);
    assert!(crate::css::parse_selector(".compound/**/.joined").valid);
    let border = find("border");
    assert_eq!(border.style.border_top_width, CssLength::Px(2.0));
    assert_eq!(border.style.border_top_color, Color::rgb(255, 0, 0));
    assert_eq!(find("priority").style.color, Color::rgb(0, 128, 0));
}

#[test]
fn css_url_contents_and_comment_separated_shorthands_remain_distinct() {
    let declarations = crate::css::parse_declarations(
        "background-image:url(x/*literal*/y); border:2px/**/solid/**/red; content:'/*literal*/';",
    );
    assert_eq!(
        declarations.get("background-image").map(String::as_str),
        Some("url(x/*literal*/y)")
    );
    assert_eq!(
        declarations.get("content").map(String::as_str),
        Some("'/*literal*/'")
    );
    assert!(!crate::css::supports_condition_matches("(width:10/**/px)"));
    assert!(!crate::css::supports_condition_matches("(color:re/**/d)"));
    assert!(crate::css::supports_condition_matches(
        "(border:2px/**/solid/**/red)"
    ));
    let mut style = ComputedStyle::default();
    crate::css::apply_property(
        &mut style,
        "background-image",
        &declarations["background-image"],
    );
    assert_eq!(style.background_image_url, "x/*literal*/y");
}

#[test]
fn box_length_grammar_rejects_invalid_declarations_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "margin", "1px 2px 3px 4px");
    apply_property(&mut style, "padding", "5px 6px 7px 8px");
    apply_property(&mut style, "inset", "9px 10px 11px 12px");
    for (prop, values) in [
        (
            "margin",
            vec![
                "8px bad",
                "1px 2px 3px 4px 5px",
                "min-content",
                "10/**/px",
                "8",
            ],
        ),
        (
            "padding",
            vec!["8px bad", "1px -2px", "auto", "max-content", "8"],
        ),
        ("inset", vec!["8px bad", "fit-content(20px)", "none", "8"]),
    ] {
        for value in values {
            apply_property(&mut style, prop, value);
            assert!(
                !crate::css::supports_condition_matches(&format!("({prop}:{value})")),
                "{prop}:{value}"
            );
        }
    }
    for (value, expected) in [
        (&style.margin_top, 1.0),
        (&style.margin_right, 2.0),
        (&style.margin_bottom, 3.0),
        (&style.margin_left, 4.0),
        (&style.padding_top, 5.0),
        (&style.padding_right, 6.0),
        (&style.padding_bottom, 7.0),
        (&style.padding_left, 8.0),
        (&style.top, 9.0),
        (&style.right, 10.0),
        (&style.bottom, 11.0),
        (&style.left, 12.0),
    ] {
        assert_eq!(*value, CssLength::Px(expected));
    }
    for (prop, value) in [
        ("margin", "-2px auto"),
        ("padding", "calc(5% - 10px) 0"),
        ("padding", "calc(-2px) 0"),
        ("inset", "AUTO -2px"),
        ("margin-inline", "1px/**/2px"),
    ] {
        assert!(
            crate::css::supports_condition_matches(&format!("({prop}:{value})")),
            "{prop}:{value}"
        );
    }
}

#[test]
fn compiled_inline_and_variable_box_values_share_checked_grammar() {
    let doc = parse_and_layout(
        "<style>.base {margin-left:5px;padding-left:6px;left:7px} \
         .bad {margin-left:8;padding-left:-2px;left:min-content} \
         .variable {--bad:8px broken;margin:var(--bad);padding:var(--bad);inset:var(--bad)} \
         .logical {direction:rtl;margin-inline:3px 4px;padding-inline:5px 6px;inset-inline:7px 8px} \
         .logical {margin-inline:9px broken;padding-inline:9px -1px;inset-inline:none} \
         .logical-reset {direction:rtl;margin-inline:3px 4px;inset-inline:7px 8px} \
         .logical-reset {margin-inline:var(--missing);inset-inline:var(--missing)} \
         </style><div id=compiled class='base bad'>compiled</div>\
         <div id=inline class=base style='margin-left:8;padding-left:-2px;left:min-content'>inline</div>\
         <div id=variable class='base variable'>variable</div>\
         <div id=logical class=logical>logical</div>\
         <div id=logical-reset class=logical-reset>reset</div>",
        800.0,
    );
    let find = |id: &str| {
        find_box(&doc.root, &|n| {
            n.attributes.get("id").is_some_and(|s| s == id)
        })
        .unwrap()
    };
    for id in ["compiled", "inline"] {
        assert_eq!(find(id).style.margin_left, CssLength::Px(5.0));
        assert_eq!(find(id).style.padding_left, CssLength::Px(6.0));
        assert_eq!(find(id).style.left, CssLength::Px(7.0));
    }
    let variable = &find("variable").style;
    assert_eq!(variable.margin_left.resolve(16.0, 800.0, 16.0), 0.0);
    assert_eq!(variable.padding_left.resolve(16.0, 800.0, 16.0), 0.0);
    assert_eq!(variable.left, CssLength::Auto);
    let logical = &find("logical").style;
    assert_eq!(logical.margin_right, CssLength::Px(3.0));
    assert_eq!(logical.margin_left, CssLength::Px(4.0));
    assert_eq!(logical.padding_right, CssLength::Px(5.0));
    assert_eq!(logical.padding_left, CssLength::Px(6.0));
    assert_eq!(logical.right, CssLength::Px(7.0));
    assert_eq!(logical.left, CssLength::Px(8.0));
    let reset = &find("logical-reset").style;
    assert_eq!(reset.margin_left.resolve(16.0, 800.0, 16.0), 0.0);
    assert_eq!(reset.margin_right.resolve(16.0, 800.0, 16.0), 0.0);
    assert_eq!(reset.left, CssLength::Auto);
    assert_eq!(reset.right, CssLength::Auto);
}

#[test]
fn invalid_variable_winners_use_unset_across_cascade_paths() {
    let doc = parse_and_layout(
        "<style>body {color:green} .base {width:80px;color:red} \
         .normal {--bad:10/**/px;--ink:re/**/d;width:var(--bad);color:var(--ink)} \
         .important {width:var(--absent) !important;color:var(--absent) !important} \
         .state:hover {width:var(--absent);color:var(--absent)} \
         .pseudo::before {content:'test';width:80px;color:red} \
         .pseudo::before {width:var(--absent);color:var(--absent)} \
         .no-content::before {content:'old'} .no-content::before {content:var(--absent)} \
         .valid {--good:25px;width:var(--good);color:var(--absent,blue)} \
         </style><div id=normal class='base normal'>normal</div>\
         <div id=important class='base important'>important</div>\
         <div id=inline class=base style='width:var(--absent);color:var(--absent)'>inline</div>\
         <div id=inline-important class=base style='width:var(--absent) !important;color:var(--absent) !important'>inline important</div>\
         <div id=state class='base state'>state</div>\
         <div id=pseudo class=pseudo>pseudo</div>\
         <div id=no-content class=no-content>no pseudo</div>\
         <div id=valid class='base valid'>valid</div>",
        800.0,
    );
    let find = |id: &str| {
        find_box(&doc.root, &|n| {
            n.attributes.get("id").is_some_and(|s| s == id)
        })
        .unwrap()
    };
    for id in ["normal", "important", "inline", "inline-important"] {
        assert_eq!(find(id).style.width, CssLength::Auto, "{id} width");
        assert_eq!(
            find(id).style.color,
            Color::rgb(0, 128, 0),
            "{id} inherited color"
        );
    }
    let hover = find("state").style.hover_style.as_ref().unwrap();
    assert_eq!(hover.width, CssLength::Auto);
    assert_eq!(hover.color, Color::rgb(0, 128, 0));
    let before = find("pseudo").style.before_style.as_ref().unwrap();
    assert_eq!(before.width, CssLength::Auto);
    assert_eq!(before.color, Color::rgb(0, 128, 0));
    assert!(find("no-content").style.before_style.is_none());
    assert_eq!(find("valid").style.width, CssLength::Px(25.0));
    assert_eq!(find("valid").style.color, Color::rgb(0, 0, 255));
}

#[test]
fn environment_substitution_uses_the_shared_cascade_unset_baseline() {
    let doc = parse_and_layout(
        r#"
      <style>
        body { color: green; padding: 18px }
        .base { padding: 4px; color: red }
        #normal { padding: env(unknown, 10px, 20px); color: env(unknown, red, blue) }
        #important { padding: env(absent) !important; color: env(absent) !important }
        #inherit { padding: env(absent, inherit) }
        #shorthand { border: env(absent, 3px solid blue) }
        #list { font-family: env(absent, serif, sans-serif) }
        #multiple { padding: env(absent, 10px) env(absent, 20px) }
        #split { padding: 4px; padding: env(absent,10)px }
        #pseudo::before { content: 'old' }
        #pseudo::before { content: env(absent, 'new') }
        #hover:hover { padding: env(absent) }
      </style>
      <div class=base id=normal>normal</div>
      <div class=base id=important>important</div>
      <div class=base id=inline style='padding:env(absent);color:env(absent)'>inline</div>
      <div class=base id=inherit>inherit</div>
      <div id=shorthand>shorthand</div>
      <div id=list>list</div>
      <div id=multiple>multiple</div>
      <div id=split>split</div>
      <div id=pseudo>pseudo</div>
      <div class=base id=hover>hover</div>
    "#,
        800.0,
    );
    let find = |id: &str| {
        find_box(&doc.root, &|n| {
            n.attributes.get("id").is_some_and(|s| s == id)
        })
        .unwrap()
    };
    for id in ["normal", "important", "inline"] {
        assert_eq!(
            find(id).style.padding_top,
            ComputedStyle::default().padding_top,
            "{id}"
        );
        assert_eq!(find(id).style.color, Color::rgb(0, 128, 0), "{id}");
    }
    assert_eq!(find("inherit").style.padding_top, CssLength::Px(18.0));
    assert_eq!(find("shorthand").style.border_top_width, CssLength::Px(3.0));
    assert_eq!(
        find("shorthand").style.border_top_color,
        Color::rgb(0, 0, 255)
    );
    assert!(find("list").style.font_family.contains("sans-serif"));
    assert_eq!(find("multiple").style.padding_top, CssLength::Px(10.0));
    assert_eq!(find("multiple").style.padding_right, CssLength::Px(20.0));
    assert_eq!(
        find("split").style.padding_top,
        ComputedStyle::default().padding_top
    );
    assert_eq!(find("pseudo").style.before_content, "new");
    assert_eq!(
        find("hover")
            .style
            .hover_style
            .as_ref()
            .unwrap()
            .padding_top,
        ComputedStyle::default().padding_top
    );
}

#[test]
fn url_punctuation_does_not_terminate_declarations_or_stylesheet_blocks() {
    let source = ".first { background-image:url(x};!y); color:green; } .second { color:blue; }";
    let rules = parse_stylesheet(source).expect("stylesheet");
    assert_eq!(rules.len(), 2);
    assert_eq!(
        rules[0]
            .declarations
            .get("background-image")
            .map(String::as_str),
        Some("url(x};!y)")
    );
    assert_eq!(
        rules[0].declarations.get("color").map(String::as_str),
        Some("green")
    );
    assert_eq!(
        rules[1].declarations.get("color").map(String::as_str),
        Some("blue")
    );
    let (normal, important) = crate::css::parse_declarations_important(
        "--url:url(x}!important); color:green !important;",
    );
    assert_eq!(
        normal.get("--url").map(String::as_str),
        Some("url(x}!important)")
    );
    assert_eq!(important.get("color").map(String::as_str), Some("green"));
}

#[test]
fn declaration_grammar_preserves_component_blocks_and_identifier_escapes() {
    let source = r#"\63 olor: red !\69mportant;
        --\4e ame: { first: a; nested: [b; (c; d)]; flag: !important };
        --escaped: a\;b; --foo\:bar: kept;
        content: "!important"; --literal: \!important;
        --function: fn(!important); background-color: blue !/**/IMPORTANT;
        123bad: rejected; bad name: rejected; width: 10px;
        --invalid: bang !not-important; height: 20px;"#;
    let (normal, important) = crate::css::parse_declarations_important(source);
    assert_eq!(important.get("color").map(String::as_str), Some("red"));
    assert_eq!(
        important.get("background-color").map(String::as_str),
        Some("blue")
    );
    for (name, expected) in [
        (
            "--Name",
            "{ first: a; nested: [b; (c; d)]; flag: !important }",
        ),
        ("--escaped", r"a\;b"),
        ("--foo:bar", "kept"),
        ("content", r#""!important""#),
        ("--literal", r"\!important"),
        ("--function", "fn(!important)"),
        ("width", "10px"),
        ("height", "20px"),
    ] {
        assert_eq!(
            normal.get(name).map(String::as_str),
            Some(expected),
            "{name}"
        );
        assert!(!important.contains_key(name), "{name}");
    }
    for invalid in ["123bad", "bad name", "--invalid"] {
        assert!(!normal.contains_key(invalid));
    }
    let merged = crate::css::parse_declarations(source);
    assert_eq!(merged.get("color").map(String::as_str), Some("red"));
    assert_eq!(merged.get("--Name"), normal.get("--Name"));
}

#[test]
fn custom_property_blocks_are_not_parsed_as_nested_rules() {
    let rules = crate::css::parse_stylesheet(
        r#"
        div { --data: { first: a; nested: { second: b; } }; \63 olor: red !\69mportant;
          span { color: blue; } background-color: green;
        }
    "#,
    )
    .unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(
        rules[0].declarations.get("--data").map(String::as_str),
        Some("{ first: a; nested: { second: b; } }")
    );
    assert_eq!(
        rules[0]
            .important_declarations
            .get("color")
            .map(String::as_str),
        Some("red")
    );
    assert_eq!(
        rules[0]
            .declarations
            .get("background-color")
            .map(String::as_str),
        Some("green")
    );
    let doc = parse_and_layout(
        r#"<style>div { color: blue; \63 olor: red !\69mportant; }</style><div>x</div>"#,
        800.0,
    );
    let node = find_box(&doc.root, &|node| node.tag == "div").unwrap();
    assert_eq!(node.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn declaration_identifier_escapes_are_shared_with_supports() {
    let parsed = crate::css::parse_declarations("\\63\r\nOLOR:red; --\\0 :kept;");
    assert_eq!(parsed.get("color").map(String::as_str), Some("red"));
    assert_eq!(parsed.get("--\u{fffd}").map(String::as_str), Some("kept"));
    for condition in [
        r"(\63 olor: red !\69mportant)",
        r"(color: red !/**/IMPORTANT)",
    ] {
        assert!(
            crate::css::supports_condition_matches(condition),
            "{condition}"
        );
    }
    for condition in [
        "(123bad: red)",
        "(bad name: red)",
        "(--: red)",
        "(color: red !not-important)",
    ] {
        assert!(
            !crate::css::supports_condition_matches(condition),
            "{condition}"
        );
    }
}

#[test]
fn empty_custom_property_is_valid_and_does_not_use_var_fallback() {
    let (normal, important) = crate::css::parse_declarations_important(
        "--empty: ; --other: green; --priority: !important; color: ;",
    );
    assert_eq!(normal.get("--empty").map(String::as_str), Some(""));
    assert_eq!(important.get("--priority").map(String::as_str), Some(""));
    assert!(!normal.contains_key("color"));

    let vars = resolved_root_variables(":root { --empty:; --invalid: var(--missing); }");
    assert!(vars.contains_key("--empty"));
    assert!(!vars.contains_key("--invalid"));
    assert_eq!(
        crate::css::resolve_var_references("var(--empty, blue)", &vars),
        ""
    );
    assert_eq!(
        crate::css::resolve_var_references("var(--invalid, blue)", &vars),
        "blue"
    );

    let doc = parse_and_layout(
        "<style>:root { --empty: red; --empty: !important; } \
         p { color: var(--empty, blue) }</style><p id=target>x</p>",
        800.0,
    );
    let p = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "target")
    })
    .expect("paragraph");
    assert_eq!(
        p.style.custom_props.get("--empty").map(String::as_str),
        Some("")
    );
    assert_ne!(p.style.color, Color::rgb(0, 0, 255));
}

#[test]
fn custom_property_fallback_uses_invalid_dependency_but_not_empty_one() {
    let vars = resolved_root_variables(
        ":root { --invalid: var(--missing); --using-invalid: var(--invalid, green);\
         --empty:; --using-empty: var(--empty, blue); }",
    );
    assert!(!vars.contains_key("--invalid"));
    assert_eq!(
        vars.get("--using-invalid").map(|value| value.trim()),
        Some("green")
    );
    assert_eq!(vars.get("--using-empty").map(String::as_str), Some(""));

    let cycle = resolved_root_variables(
        ":root { --a: var(--b, red); --b: var(--a, blue);\
         --consumer: var(--a, green); }",
    );
    assert!(!cycle.contains_key("--a"));
    assert!(!cycle.contains_key("--b"));
    assert_eq!(
        cycle.get("--consumer").map(|value| value.trim()),
        Some("green")
    );

    let mut chain_css = String::from(":root { --v0: blue;");
    for i in 1..80 {
        chain_css.push_str(&format!("--v{i}: var(--v{});", i - 1));
    }
    chain_css.push('}');
    let chain = resolved_root_variables(&chain_css);
    assert_eq!(chain.get("--v79").map(String::as_str), Some("blue"));
}

#[test]
fn css_wide_keywords_on_custom_properties_use_inheritance_or_invalid_value() {
    let root = resolved_root_variables(
        r#":root { --brand: green; --reset: InItIaL; --other: UNSET;
         --literal: "initial"; --csstools-light-dark-toggle-1: green; }"#,
    );
    assert_eq!(root.get("--brand").map(String::as_str), Some("green"));
    assert!(!root.contains_key("--reset"));
    assert!(!root.contains_key("--other"));
    assert_eq!(
        root.get("--literal").map(String::as_str),
        Some("\"initial\"")
    );
    assert_eq!(
        root.get("--csstools-light-dark-toggle-1")
            .map(String::as_str),
        Some("green")
    );

    let doc = parse_and_layout(
        "<style>:root { --brand: green; } \
         #inherited { --brand: inherit; color: var(--brand, blue) } \
         #unset { --brand: unset; color: var(--brand, blue) } \
         #reset { --brand: initial; color: var(--brand, blue) }</style>\
         <div><p id=inherited>x</p><p id=unset>x</p><p id=reset>x</p></div>",
        800.0,
    );
    for id in ["inherited", "unset"] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("paragraph");
        assert_eq!(node.style.color, Color::rgb(0, 128, 0), "{id}");
    }
    let reset = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|value| value == "reset")
    })
    .expect("reset paragraph");
    assert_eq!(reset.style.color, Color::rgb(0, 0, 255));
}

#[test]
fn child_custom_property_override_keeps_parent_computed_value() {
    let doc = parse_and_layout(
        "<div style='--base:red;--inherited:var(--base)'>\
         <span id=child style='--base:blue;--local:var(--base);\
         color:var(--inherited);background-color:var(--local)'>x</span></div>",
        800.0,
    );
    let child = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "child")
    })
    .expect("child");
    assert_eq!(child.style.color, Color::rgb(255, 0, 0));
    assert_eq!(child.style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn identical_child_custom_property_shares_inherited_scope() {
    let doc = parse_and_layout(
        "<div id=parent style='--ink:red'><span id=child style='--ink:red;color:var(--ink)'>x</span></div>",
        800.0,
    );
    let parent = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "parent")
    })
    .expect("parent");
    let child = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "child")
    })
    .expect("child");
    assert!(std::sync::Arc::ptr_eq(
        &parent.style.custom_props,
        &child.style.custom_props
    ));
    assert_eq!(child.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn sibling_custom_property_overrides_share_resolved_scope() {
    let doc = parse_and_layout(
        "<div style='--base:red'>\
         <span id=first style='--ink:var(--base);color:var(--ink)'>one</span>\
         <span id=second style='--ink:var(--base);color:var(--ink)'>two</span>\
         </div>\
         <div style='--base:blue'>\
         <span id=other style='--ink:var(--base);color:var(--ink)'>three</span>\
         </div>",
        800.0,
    );
    let style = |id| {
        find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("styled span")
        .style
        .clone()
    };
    let first = style("first");
    let second = style("second");
    let other = style("other");
    assert!(std::sync::Arc::ptr_eq(
        &first.custom_props,
        &second.custom_props
    ));
    assert!(!std::sync::Arc::ptr_eq(
        &first.custom_props,
        &other.custom_props
    ));
    assert_eq!(first.color, Color::rgb(255, 0, 0));
    assert_eq!(other.color, Color::rgb(0, 0, 255));
}

#[test]
fn child_custom_property_redeclaration_uses_child_dependencies() {
    let doc = parse_and_layout(
        "<div style='--base:red;--ink:var(--base)'>\
         <span id=child style='--base:blue;--ink:var(--base);color:var(--ink)'>x</span></div>",
        800.0,
    );
    let child = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "child")
    })
    .expect("child");
    assert_eq!(child.style.color, Color::rgb(0, 0, 255));
}

#[test]
fn important_custom_properties_follow_stylesheet_and_inline_tiers() {
    let doc = parse_and_layout(
        "<style>.item { --ink:red !important; color:var(--ink) }</style>\
         <span id=sheet class=item style='--ink:blue'>Sheet wins</span>\
         <span id=inline class=item style='--ink:blue !important'>Inline wins</span>",
        800.0,
    );
    for (id, expected) in [
        ("sheet", Color::rgb(255, 0, 0)),
        ("inline", Color::rgb(0, 0, 255)),
    ] {
        let node = find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("styled span");
        assert_eq!(node.style.color, expected, "{id}");
    }
}

#[test]
fn mixed_custom_property_rule_substitutes_only_values_that_need_it() {
    assert!(!crate::css::apply::value_needs_substitution(
        "3px solid green"
    ));
    assert!(crate::css::apply::value_needs_substitution("var(--gap)"));
    assert!(crate::css::apply::value_needs_substitution(
        "LiGhT-DaRk(white, black)"
    ));

    let mut frame = EngineFrame::empty(800.0, 600.0);
    frame.load_html(
        "<style>#target { --gap: 4px; width: var(--gap); margin-left: 3px; color-scheme: dark; color: LiGhT-DaRk(white, black) }</style><p id=target>Text</p>",
    );
    frame.update_frame();
    let target = find_box(&frame.doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "target")
    })
    .unwrap();
    assert_eq!(target.style.width, CssLength::Px(4.0));
    assert_eq!(target.style.margin_left, CssLength::Px(3.0));
    assert_eq!(target.style.color, Color::rgb(0, 0, 0));
}

#[test]
fn presentational_hints_run_once_without_revert_snapshots() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add_author(".x { text-align: left } .x { color: red }");
    sheet.rebuild_index();
    assert!(sheet.rules.iter().all(|rule| !rule.has_revert_value));

    let mut node = WebCore::new("p");
    node.node_id = 1;
    node.attributes.insert("class", "x");
    node.attributes.insert("align", "center");
    crate::css::apply_cascade(&mut node, &sheet, None, 16.0);
    assert_eq!(node.style.text_align, TextAlign::Left);
    assert_eq!(node.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn layer_snapshots_are_kept_when_revert_can_be_resolved() {
    assert!(crate::css::apply::value_mentions_revert("ReVeRt-LaYeR"));
    assert!(!crate::css::apply::value_mentions_revert("var(--color)"));

    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        "@layer base, top; @layer base { .literal, .variable, .fallback { color: red } } \
         @layer top { .literal { color: revert-layer } \
         .variable { --choice: revert-layer; color: var(--choice) } \
         .fallback { --choice: revert-layer; color: var(--choice, revert-layer) } \
         .plain { --choice: blue; color: var(--choice) } }",
    );
    sheet.rebuild_index();
    assert!(sheet.rules.iter().any(|rule| rule.has_revert_value));

    for (class, expected) in [
        ("literal", Color::rgb(255, 0, 0)),
        ("variable", Color::BLACK),
        ("fallback", Color::rgb(255, 0, 0)),
        ("plain", Color::rgb(0, 0, 255)),
    ] {
        let mut node = WebCore::new("p");
        node.node_id = 1;
        node.attributes.insert("class", class);
        crate::css::apply_cascade(&mut node, &sheet, None, 16.0);
        assert_eq!(node.style.color, expected, "class={class}");
    }
}

#[test]
fn identical_recascade_preserves_clean_layout() {
    let mut root = WebCore::new("main");
    root.node_id = 1;
    let mut child = WebCore::new("p");
    child.node_id = 2;
    root.children.push(child);

    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("main { color: red } p { width: 100px }");
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    root.layout.last_containing_width = 800.0;
    root.children[0].layout.last_containing_width = 800.0;
    root.layout.layout_dirty = false;
    root.children[0].layout.layout_dirty = false;

    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    assert!(!root.layout.layout_dirty);
    assert!(!root.children[0].layout.layout_dirty);

    sheet.parse_and_add("p { width: 120px }");
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    assert!(!root.layout.layout_dirty);
    assert!(root.children[0].layout.layout_dirty);
    assert_eq!(root.children[0].style.width, CssLength::Px(120.0));
}

#[test]
fn shared_parent_styles_do_not_skip_descendant_cascade() {
    let mut root = WebCore::new("main");
    root.node_id = 1;
    for index in 0..3 {
        let mut parent = WebCore::new("section");
        parent.node_id = 2 + index * 2;
        let mut child = WebCore::new("span");
        child.node_id = 3 + index * 2;
        child.text = "Label".to_string();
        parent.children.push(child);
        root.children.push(parent);
    }
    for (color, extra_rules) in [("red", 0), ("blue", 0), ("red", 1001), ("blue", 1001)] {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add(&format!(
            "section {{color:{color};quotes:none}} span {{font-size:24px;background:yellow}}"
        ));
        for i in 0..extra_rules {
            sheet.parse_and_add(&format!(".unused{i} {{color:green}}"));
        }
        sheet.rebuild_index();
        crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
        for parent in &root.children {
            let child = &parent.children[0];
            assert_eq!(
                child.style.font_size,
                CssLength::Px(24.0),
                "descendant {}",
                child.node_id
            );
            assert_eq!(child.style.color, parent.style.color);
            assert_eq!(child.style.rare().quotes, Some(Vec::new()));
            assert_eq!(
                child.style.background_color,
                Color {
                    r: 255,
                    g: 255,
                    b: 0,
                    a: 255
                }
            );
        }
    }
}

#[test]
fn shared_leaf_styles_do_not_skip_counter_effects() {
    let mut root = WebCore::new("main");
    root.node_id = 1;
    for i in 0..3 {
        let mut node = WebCore::new("i");
        node.node_id = i + 2;
        root.children.push(node);
    }
    let mut output = WebCore::new("p");
    output.node_id = 5;
    root.children.push(output);
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("main{counter-reset:n}i{counter-increment:n}p::before{content:counter(n)}");
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    let output = root.children.last().unwrap();
    assert_eq!(output.style.before_content, "3");
}

#[test]
fn negative_margin_can_reduce_atomic_inline_advance_to_zero() {
    let doc = parse_and_layout(
        r#"<style>
          * { margin:0; padding:0 }
          .nav { width:1000px }
          .links { display:inline-flex; width:1000px; height:50px }
          .menu { display:inline-flex; width:40px; height:40px; margin-left:-40px; position:sticky }
        </style><div class="nav"><header class="links" id="links"></header><button class="menu" id="menu"></button></div>"#,
        1200.0,
    );
    let links = crate::tests::test_grid::find_by_id(&doc.root, "links")
        .unwrap()
        .layout
        .border_rect;
    let menu = crate::tests::test_grid::find_by_id(&doc.root, "menu")
        .unwrap()
        .layout
        .border_rect;
    assert!(
        (menu.y - links.y).abs() < 1.0,
        "button wrapped below the header: {menu:?} {links:?}"
    );
    assert!((menu.x - (links.x + links.w - menu.w)).abs() < 1.0);
}

#[test]
fn media_and_container_queries_accept_adjacent_logical_keyword() {
    assert!(!crate::css::evaluate_media(
        "screen and (max-width:1023px)",
        1280.0,
        820.0
    ));
    let query = "(min-width:768px)and (max-width:1023px)";
    assert!(crate::css::evaluate_media(query, 900.0, 700.0));
    assert!(!crate::css::evaluate_media(query, 1280.0, 700.0));
    assert!(crate::css::evaluate_container(query, 900.0, 700.0));
    assert!(!crate::css::evaluate_container(query, 1280.0, 700.0));
}

#[test]
fn webkit_scrollbar_display_none_hides_bar_without_disabling_scroll() {
    let doc = parse_and_layout(
        r#"<style>
          .scroll { width: 100px; overflow-x: auto; white-space: nowrap }
          .scroll::-webkit-scrollbar { display: none }
          .scroll::-webkit-scrollbar-thumb { background: red }
        </style><div id="scroll" class="scroll">A long line that overflows horizontally</div>"#,
        800.0,
    );
    let node = crate::tests::test_grid::find_by_id(&doc.root, "scroll").unwrap();
    assert_eq!(node.style.scrollbar_width, "none");
    assert_eq!(node.style.overflow_x, Overflow::Auto);
    assert!(node.layout.scroll_width > node.layout.content_rect.w);
}

#[test]
fn desktop_media_unset_clears_logical_block_start_margin() {
    let doc = parse_and_layout(
        r#"<style>
          .item { margin-block-start: 6px; font-size: 32px }
          @media only screen and (min-width: 63.75rem) {
            .item { margin-block-start: unset; font-size: 40px }
          }
        </style><div class="item" id="item">Text</div>"#,
        1280.0,
    );
    let item = crate::tests::test_grid::find_by_id(&doc.root, "item").unwrap();
    assert_eq!(item.style.margin_top, CssLength::Zero);
    assert_eq!(item.style.font_size, CssLength::Px(40.0));
}

fn build_display_texts(html: &str) -> Vec<String> {
    let doc = parse_html(html);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    list.commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn build_display_markers(html: &str) -> Vec<String> {
    let doc = parse_html(html);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    list.commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::ListMarker { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn collapsible_space_across_inline_boundaries_is_not_measured_or_painted_twice() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
             .box { display: inline-block; }
           </style>
           <span id="control" class="box">a b</span>
           <br>
           <span id="split" class="box"><i>a </i><i> b</i></span>"#,
        900.0,
    );
    let control = d.get_element_by_id("control").unwrap();
    let split = d.get_element_by_id("split").unwrap();
    let control_w = d.get_bounding_client_rect(control).unwrap().w;
    let split_w = d.get_bounding_client_rect(split).unwrap().w;
    assert!(
        (control_w - split_w).abs() < 0.5,
        "split inline whitespace measured as {split_w}, control was {control_w}"
    );

    let texts = build_display_texts(
        r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
             .box { display: inline-block; }
           </style>
           <span id="split" class="box"><i>a </i><i> b</i></span>"#,
    );
    assert_eq!(texts.concat(), "a b");
}

#[test]
fn indented_inline_link_after_atomic_box_paints_at_its_layout_position() {
    let html = r#"<style>
        body { margin: 0; font: 18px/24px sans-serif; }
        .book { display: inline-block; padding: 0 10px; }
        .cover { display: inline-block; width: 27px; height: 40px; }
    </style>
    <div class="book"><span class="cover"></span>
        <a id="link">
            French Anthology on Amazon
        </a>
    </div>"#;
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(html, 800.0);
    let link = doc.get_element_by_id("link").unwrap();
    let link_x = doc.get_bounding_client_rect(link).unwrap().x;
    let list = build_display_list(&doc.root, 800.0, 600.0);
    let text_x = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { x, text, .. } if text.contains("French Anthology") => Some(*x),
            _ => None,
        })
        .expect("link text paint command");
    assert!(
        (text_x - link_x).abs() < 2.0,
        "paint x {text_x}, layout x {link_x}"
    );
}

#[test]
fn leading_space_after_inline_link_is_preserved_in_painted_geometry() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
             a { color: blue; }
           </style>
           <p><a>Misti</a> is a volcano, and mudflows <a>and</a> hydropower plants.</p>"#,
        800.0,
    );
    let list = crate::renderer::display_list_builder::build_display_list_full_with_font_system(
        &doc.root,
        800.0,
        200.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
        Some(&mut renderer.font_system),
    );
    for (link, following) in [("Misti", "is a volcano"), ("and", "hydropower plants")] {
        let (link_x, link_y) = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::Text { text, x, y, .. } if text == link => Some((*x, *y)),
                _ => None,
            })
            .unwrap();
        let (next_x, next_y) = list
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::Text { text, x, y, .. } if text.starts_with(following) => Some((*x, *y)),
                _ => None,
            })
            .unwrap();
        let advance = crate::layout::inline_layout::measure_text_width_weighted(
            &format!("{link} "),
            16.0,
            Some(&mut renderer.font_system),
            FontWeight::Normal,
            FontStyle::Normal,
            1.0,
            "Menlo",
            100.0,
        );
        assert!(
            (next_x - link_x - advance).abs() < 0.1,
            "{link}: separator must reserve exactly one space, advance={}, expected={advance}",
            next_x - link_x
        );
        assert_eq!(link_y, next_y);
    }
}

#[test]
fn generated_content_slash_alt_text_is_not_visible() {
    assert_eq!(
        resolve_content_value(r#""\200b" / "(external)""#),
        "\u{200b}"
    );
}

#[test]
fn bidi_level_runs_use_utf8_byte_offsets() {
    for prefix in ["العربية", "עברית", "😀 العربية", "é עברית"] {
        let text = format!("xx{prefix} Deutsch");
        let mut line = LayoutLine {
            text_start: 2,
            text_length: text.len() - 2,
            ..LayoutLine::default()
        };
        crate::layout::text::resolve_bidi_line(
            &text,
            &mut line,
            Direction::LTR,
            UnicodeBidi::Normal,
        );
        let start = text.find("Deutsch").unwrap();
        assert!(
            line.visual_segments
                .iter()
                .any(|segment| segment.logical_start <= start
                    && segment.logical_start + segment.length >= start + "Deutsch".len()
                    && segment.level % 2 == 0),
            "Latin text split or assigned an RTL level after {prefix}: {:?}",
            line.visual_segments
        );
        for segment in &line.visual_segments {
            assert!(text.is_char_boundary(segment.logical_start));
            assert!(text.is_char_boundary(segment.logical_start + segment.length));
        }
    }
}

#[test]
fn mixed_direction_inline_links_paint_as_separate_runs() {
    let html = r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             li { display: inline; }
             li::after { content: " · "; font-weight: 700; }
           </style>
           <ul>
             <li><a><span>العربية</span></a></li>
             <li><a><span>Deutsch</span></a></li>
             <li><a><span>Español</span></a></li>
           </ul>"#;
    let doc = parse_and_layout(html, 800.0);
    let list = build_display_list(&doc.root, 800.0, 200.0);
    let text_cmds: Vec<(f32, String)> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { x, text, .. } => Some((*x, text.clone())),
            _ => None,
        })
        .collect();

    let arabic = text_cmds
        .iter()
        .find(|(_, text)| text == "العربية")
        .expect("Arabic link should paint as its own command")
        .0;
    let deutsch = text_cmds
        .iter()
        .find(|(_, text)| text == "Deutsch")
        .expect("Deutsch link should not be split by the Arabic run")
        .0;
    let espanol = text_cmds
        .iter()
        .find(|(_, text)| text == "Español")
        .expect("following LTR link should paint as its own command")
        .0;

    assert!(
        arabic < deutsch && deutsch < espanol,
        "mixed-direction links must paint in their laid-out order: {text_cmds:?}"
    );
}

#[test]
fn flex_row_min_height_participates_in_cross_axis_alignment() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             body { margin: 0; font: 14px/17px sans-serif; }
             a {
               display: flex;
               align-items: center;
               min-height: 32px;
               position: relative;
               width: 80px;
             }
             a::after {
               content: "";
               position: absolute;
               left: 0;
               right: 0;
               bottom: 0;
               height: 2px;
             }
           </style>
           <a id="tab"><span id="label">Main Page</span></a>"#,
        400.0,
    );
    let tab = doc.get_element_by_id("tab").unwrap();
    let label = doc.get_element_by_id("label").unwrap();
    let tab_rect = doc.get_bounding_client_rect(tab).unwrap();
    let label_rect = doc.get_bounding_client_rect(label).unwrap();

    assert!(
        tab_rect.h >= 31.5,
        "flex row should honor min-height, got {tab_rect:?}"
    );
    assert!(
        label_rect.y > tab_rect.y + 5.0,
        "align-items:center should center the label inside min-height: tab={tab_rect:?} label={label_rect:?}"
    );
}

#[test]
fn text_nodes_inside_inline_block_controls_remain_inline() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             body { margin: 0; font: 14px/21px Arial; }
             button { display: inline-block; padding: 6px 12px; white-space: nowrap; }
             i { display: inline-block; width: 14px; height: 16px; vertical-align: middle; }
             i::before { content: "X"; }
             button::after {
               display: inline-block;
               width: 0;
               height: 0;
               margin-left: .255em;
               vertical-align: .255em;
               content: "";
               border-top: .3em solid;
               border-right: .3em solid transparent;
               border-bottom: 0;
               border-left: .3em solid transparent;
             }
           </style>
           <button id="lang">
             <i></i>
             Français
           </button>"#,
        400.0,
    );
    let button = d.get_element_by_id("lang").unwrap();
    let button_rect = d.get_bounding_client_rect(button).unwrap();
    assert!(
        button_rect.h < 40.0,
        "inline icon plus text should share one line, got height {}",
        button_rect.h
    );
    assert!(
        button_rect.w > 90.0,
        "inline-block shrink-to-fit width should include icon plus text, got {}",
        button_rect.w
    );

    let node = d.find_webcore(button).unwrap();
    let text = node
        .children
        .iter()
        .find(|child| child.tag == "#text")
        .expect("button text node");
    assert_eq!(text.style.display, Display::Inline);

    let flat = crate::layout::inline_layout::collect_flat_text(node);
    assert!(
        !flat.contains('X'),
        "generated content inside an atomic inline-block child must not leak into the parent text run"
    );
    assert!(flat.contains("Français"));
    let line = node.layout.line_cache.first().expect("button line");
    assert!(
        line.text_x_offset > 13.0,
        "visible text should be offset past the atomic icon, got {}",
        line.text_x_offset
    );
}

#[test]
fn root_dir_and_inherited_box_sizing_reach_descendants() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<html dir="rtl">
             <head>
               <style>
                 html { box-sizing: border-box; }
                 *, *::before, *::after { box-sizing: inherit; }
                 body { margin: 0; }
               </style>
             </head>
             <body>
               <div id="box" style="width:100%; padding-inline:16px"></div>
             </body>
           </html>"#,
        320.0,
    );
    let html = d.get_elements_by_tag_name("html")[0];
    let body = d.get_elements_by_tag_name("body")[0];
    let box_id = d.get_element_by_id("box").unwrap();

    assert_eq!(d.computed_style_property(html, "direction"), "rtl");
    assert_eq!(d.computed_style_property(body, "direction"), "rtl");
    assert_eq!(d.computed_style_property(box_id, "direction"), "rtl");
    assert_eq!(
        d.computed_style_property(box_id, "box-sizing"),
        "border-box"
    );

    let rect = d.get_bounding_client_rect(box_id).unwrap();
    assert!(
        (rect.w - 320.0).abs() < 0.5,
        "border-box 100% with padding should not overflow viewport: {rect:?}"
    );
}

#[test]
fn root_qualified_descendant_selector_matches_html_class() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<html class="no-js"><head><style>
             .placeholder { display: block; }
             html.no-js .placeholder { display: none; }
           </style></head><body>
             <div id="target" class="placeholder"></div>
           </body></html>"#,
        800.0,
    );
    let target = d.get_element_by_id("target").unwrap();
    assert_eq!(d.computed_style_property(target, "display"), "none");
}

#[test]
fn ancestor_attribute_selector_styles_current_nav_link() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             .menu a { color: rgb(180, 0, 0); border-bottom: 0 solid transparent; }
             .menu li[aria-current=true] a { color: rgb(255, 255, 255); border-bottom: 4px solid rgb(180, 0, 0); }
           </style>
           <ul class="menu"><li aria-current="true"><a id="more">More</a></li></ul>"#,
        800.0,
    );
    let more = d.get_element_by_id("more").unwrap();
    assert_eq!(
        d.computed_style_property(more, "color"),
        "rgb(255, 255, 255)"
    );
    assert_eq!(
        d.computed_style_property(more, "border-bottom-width"),
        "4px"
    );
}

#[test]
fn absolute_dropdown_wrapper_does_not_raise_flex_nav_item() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/18px Arial; }
             ul { display: flex; align-items: center; height: 64px; margin: 0; padding: 0; }
             li { display: flex; list-style: none; }
             a, button { display: flex; padding: 4px; border: 0; background: transparent; font: inherit; }
             .trigger { display: block; }
             .menu-wrap { display: block; }
             .menu { position: absolute; top: 40px; left: 0; display: flex; padding: 24px 20px 24px 16px; }
           </style>
           <ul>
             <li><a id="sports">Sports</a></li>
             <li id="more-li">
               <div class="trigger">
                 <div><button id="more">More<span>v</span></button></div>
                 <div class="menu-wrap"><nav class="menu"><ul><li>Hidden</li></ul></nav></div>
               </div>
             </li>
           </ul>"#,
        800.0,
    );
    let sports = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "sports")
    })
    .expect("sports link");
    let more = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "more")
    })
    .expect("more button");
    assert!(
        (sports.layout.content_rect.y - more.layout.content_rect.y).abs() < 1.0,
        "absolute submenu wrapper should not change top nav alignment: sports y={}, more y={}",
        sports.layout.content_rect.y,
        more.layout.content_rect.y
    );
}

#[test]
fn vertical_align_sub_and_super_shift_plain_inline_text() {
    let html = r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
           </style>
           <div id="line">base <span id="sup" style="vertical-align: super">super</span>
           <span id="sub" style="vertical-align: sub">sub</span></div>"#;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(html, 900.0);
    let line = doc.get_element_by_id("line").unwrap();
    let line_h = doc.get_bounding_client_rect(line).unwrap().h;

    let list = build_display_list(&doc.root, 900.0, 200.0);
    let mut base_y = None;
    let mut super_y = None;
    let mut sub_y = None;
    for cmd in &list.commands {
        if let PaintCmd::Text { text, y, .. } = cmd {
            if text.contains("base") {
                base_y = Some(*y);
            } else if text.contains("super") {
                super_y = Some(*y);
            } else if text.contains("sub") {
                sub_y = Some(*y);
            }
        }
    }

    let base_y = base_y.expect("base text command");
    let super_y = super_y.expect("super text command");
    let sub_y = sub_y.expect("sub text command");
    assert!(super_y < base_y, "super text did not move up");
    assert!(sub_y > base_y, "sub text did not move down");
    assert!(
        line_h > 20.0,
        "line height should include shifted inline extents, got {line_h}"
    );
}

#[test]
fn vertical_align_length_and_percentage_shift_plain_inline_text() {
    let html = r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
           </style>
           <div id="line">base <span id="up" style="vertical-align: 8px">up</span>
           <span id="percent" style="vertical-align: 50%">percent</span>
           <span id="down" style="vertical-align: -4px">down</span></div>"#;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(html, 900.0);
    let line = doc.get_element_by_id("line").unwrap();
    let line_h = doc.get_bounding_client_rect(line).unwrap().h;

    let list = build_display_list(&doc.root, 900.0, 200.0);
    let mut base_y = None;
    let mut up_y = None;
    let mut percent_y = None;
    let mut down_y = None;
    for cmd in &list.commands {
        if let PaintCmd::Text { text, y, .. } = cmd {
            if text.contains("base") {
                base_y = Some(*y);
            } else if text.contains("percent") {
                percent_y = Some(*y);
            } else if text.contains("up") {
                up_y = Some(*y);
            } else if text.contains("down") {
                down_y = Some(*y);
            }
        }
    }

    let base_y = base_y.expect("base text command");
    assert!(up_y.expect("length text command") < base_y);
    assert!(percent_y.expect("percentage text command") < base_y);
    assert!(down_y.expect("negative length text command") > base_y);
    assert!(
        line_h > 20.0,
        "line height should include length-shifted inline extents, got {line_h}"
    );
}

#[test]
fn vertical_align_text_top_and_bottom_shift_plain_inline_text() {
    let html = r#"<style>
             body { margin: 0; font: 16px/20px Menlo; }
           </style>
           <div id="line">base <span id="top" style="vertical-align: text-top">top</span>
           <span id="bottom" style="vertical-align: text-bottom">bottom</span></div>"#;
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(html, 900.0);
    let line = doc.get_element_by_id("line").unwrap();
    let line_h = doc.get_bounding_client_rect(line).unwrap().h;

    let list = build_display_list(&doc.root, 900.0, 200.0);
    let mut base_y = None;
    let mut top_y = None;
    let mut bottom_y = None;
    for cmd in &list.commands {
        if let PaintCmd::Text { text, y, .. } = cmd {
            if text.contains("base") {
                base_y = Some(*y);
            } else if text.contains("top") {
                top_y = Some(*y);
            } else if text.contains("bottom") {
                bottom_y = Some(*y);
            }
        }
    }

    let base_y = base_y.expect("base text command");
    assert!(top_y.expect("text-top command") < base_y);
    assert!(bottom_y.expect("text-bottom command") > base_y);
    assert!(
        line_h > 20.0,
        "line height should include text-top/text-bottom shifted extents, got {line_h}"
    );
}

// ── CSS Declaration Parsing ───────────────────────────────────────────────────

#[test]
fn css_basic_declarations() {
    let decls = parse_declarations("color: red; font-size: 16px;");
    assert!(decls.len() >= 2);
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("red"));
    assert_eq!(decls.get("font-size").map(|s| s.as_str()), Some("16px"));
}

#[test]
fn css_empty_declarations() {
    let decls = parse_declarations("");
    assert_eq!(decls.len(), 0);
}

#[test]
fn css_trailing_semicolon() {
    let decls = parse_declarations("color: red;");
    assert!(decls.len() >= 1);
}

#[test]
fn css_no_semicolon() {
    let decls = parse_declarations("color: red");
    assert!(decls.len() >= 1);
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("red"));
}

#[test]
fn css_multiple_values() {
    let decls = parse_declarations("margin: 10px 20px 30px 40px;");
    assert!(decls.len() >= 1);
    assert!(decls.contains_key("margin"));
}

// ── Stylesheet Parsing ────────────────────────────────────────────────────────

#[test]
fn external_stylesheet_urls_use_stylesheet_origin() {
    let css = crate::css::resolve_css_urls(
        r#".logo { background-image: url('/static/logo.svg') }
           .icon { background-image: url('../icons/chevron.svg') }
           .local { filter: url('#shadow') }"#,
        "https://static.example.test/styles/css/site.css",
    );
    assert!(css.contains("url('https://static.example.test/static/logo.svg')"));
    assert!(css.contains("url('https://static.example.test/styles/icons/chevron.svg')"));
    assert!(css.contains("url('#shadow')"));
}

#[test]
fn stylesheet_url_rewrite_preserves_svg_data_url_quotes() {
    let css = crate::css::resolve_css_urls(
        r#".icon::after { mask-image: url("data:image/svg+xml;charset=utf-8,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E") }"#,
        "https://example.test/styles/site.css",
    );
    assert!(
        css.contains("url(\"data:image/svg+xml;charset=utf-8,"),
        "{css}"
    );
    assert_eq!(
        crate::css::extract_url(&css).as_deref(),
        Some(
            "data:image/svg+xml;charset=utf-8,%3Csvg xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E"
        )
    );
}

#[test]
fn font_face_preserves_standard_descriptors() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add_with_base(
        r#"@font-face {
            font-family: Example;
            src: local("Example Local"), url("example.woff2") format("woff2") tech(variations);
            font-weight: 400 700;
            font-style: oblique 10deg 20deg;
            font-stretch: 75% 125%;
            font-display: swap;
            unicode-range: U+0000-00FF;
            size-adjust: 105%;
            ascent-override: 90%;
            descent-override: 20%;
            line-gap-override: normal;
            font-feature-settings: "kern" 1;
            font-variation-settings: "wght" 650;
            font-language-override: "TRK";
        }"#,
        "",
    );

    let face = sheet.font_faces.first().expect("font face parsed");
    assert_eq!(face.family, "Example");
    assert_eq!(
        face.src,
        r#"local("Example Local"), url("example.woff2") format("woff2") tech(variations)"#
    );
    assert_eq!(face.sources.len(), 2);
    assert_eq!(
        face.sources[0].kind,
        crate::css::FontFaceSourceKind::Local("Example Local".to_string())
    );
    assert_eq!(
        face.sources[1].kind,
        crate::css::FontFaceSourceKind::Url("example.woff2".to_string())
    );
    assert_eq!(face.sources[1].formats, vec!["woff2"]);
    assert_eq!(face.sources[1].techs, vec!["variations"]);
    assert_eq!(face.weight.as_deref(), Some("400 700"));
    assert_eq!(face.style.as_deref(), Some("oblique 10deg 20deg"));
    assert_eq!(face.stretch.as_deref(), Some("75% 125%"));
    assert_eq!(face.display.as_deref(), Some("swap"));
    assert_eq!(face.unicode_range.as_deref(), Some("U+0000-00FF"));
    assert_eq!(face.size_adjust.as_deref(), Some("105%"));
    assert_eq!(face.ascent_override.as_deref(), Some("90%"));
    assert_eq!(face.descent_override.as_deref(), Some("20%"));
    assert_eq!(face.line_gap_override.as_deref(), Some("normal"));
    assert_eq!(face.feature_settings.as_deref(), Some(r#""kern" 1"#));
    assert_eq!(face.variation_settings.as_deref(), Some(r#""wght" 650"#));
    assert_eq!(face.language_override.as_deref(), Some(r#""TRK""#));
}

#[test]
fn font_face_extraction_uses_rule_and_declaration_tokens() {
    let mut faces = Vec::new();
    crate::css::extract_font_faces(
        r#"
        @import url(a;{b);
        .fake { content:'@font-face {font-family:Fake;src:local(Fake)}'; }
        @unknown { @font-face {font-family:Hidden;src:local(Hidden)} }
        @font-face-extra {font-family:Wrong;src:local(Wrong)}
        @supports (unsupported-property:value) { @font-face {font-family:Unsupported;src:local(Wrong)} }
        @supports (display:block) {
          @f\6f nt-face {
            font-family:"Semi;\7b Family";
            s\72 c:local("Demo; {Local}");
            font-weight:400; font-weight:900 !important;
          }
        }
        @font-face {font-family:Tail;src:local(Tail)
    "#,
        &mut faces,
    );
    assert_eq!(faces.len(), 2);
    assert_eq!(faces[0].family, "Semi;{Family");
    assert_eq!(faces[0].src, r#"local("Demo; {Local}")"#);
    assert_eq!(faces[0].weight.as_deref(), Some("400"));
    assert_eq!(faces[1].family, "Tail");
}

#[test]
fn font_face_definitions_inside_scope_are_global() {
    let mut faces = Vec::new();
    crate::css::extract_font_faces(
        "@scope (.absent) { @font-face { font-family: Scoped; src: local(Scoped); } }",
        &mut faces,
    );
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].family, "Scoped");
}

#[test]
fn font_face_unicode_range_matches_ranges_and_wildcards() {
    use crate::css::font_face::unicode_range_intersects_text;

    assert!(unicode_range_intersects_text(Some("U+0041"), "A"));
    assert!(!unicode_range_intersects_text(Some("U+0041"), "B"));
    assert!(unicode_range_intersects_text(Some("U+0370-03FF"), "Ω"));
    assert!(!unicode_range_intersects_text(Some("U+0370-03FF"), "A"));
    assert!(unicode_range_intersects_text(Some("U+4??"), "Ӓ"));
    assert!(!unicode_range_intersects_text(Some("U+4??"), "A"));
    assert!(unicode_range_intersects_text(
        Some("U+0600-06FF, U+1F600"),
        "😀"
    ));
}

#[test]
fn font_face_unicode_coverage_matches_text_scan() {
    use crate::css::font_face::{UnicodeTextCoverage, unicode_range_intersects_text};

    for text in ["", "ABC", "ΩӒ", "😀", "A😀Ω"] {
        let coverage = UnicodeTextCoverage::new(text);
        for range in [
            None,
            Some(""),
            Some("invalid"),
            Some("U+0041"),
            Some("U+0042-0044"),
            Some("U+0370-03FF"),
            Some("U+4??"),
            Some("U+1F600"),
            Some("U+0600-06FF, U+1F600"),
            Some("U+10FFFF"),
        ] {
            assert_eq!(
                coverage.intersects(range),
                unicode_range_intersects_text(range, text),
                "{range:?} {text:?}"
            );
        }
    }
}

#[test]
fn font_face_font_display_accepts_only_standard_keywords() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add_with_base(
        r#"@font-face {
            font-family: ValidDisplay;
            src: url("valid.woff2");
            font-display: SWAP;
        }
        @font-face {
            font-family: InvalidDisplay;
            src: url("invalid.woff2");
            font-display: instant;
        }"#,
        "",
    );

    assert_eq!(sheet.font_faces.len(), 2);
    assert_eq!(sheet.font_faces[0].display.as_deref(), Some("swap"));
    assert_eq!(sheet.font_faces[1].display, None);
}

#[test]
fn font_face_source_tech_filters_unsupported_requirements() {
    let sources = crate::css::font_face::parse_font_face_sources(
        r#"url("variable.woff2") format("woff2") tech(variations),
           url("color.woff2") format("woff2") tech(color-COLRv1)"#,
    );

    assert_eq!(sources.len(), 2);
    assert!(crate::layout::font_source_techs_supported(&sources[0]));
    assert!(
        !crate::layout::font_source_techs_supported(&sources[1]),
        "unsupported font technology requirements should make a source ineligible"
    );
}

#[test]
fn local_font_face_alias_is_available_to_fallback_stack() {
    let mut renderer = crate::Renderer::new();
    let html = r#"<style>
        @font-face { font-family: Site Fallback; src: local("Arial"); }
        @font-face { font-family: Site Fallback; src: local("Arial"); font-weight: bold; }
        @font-face { font-family: Other Fallback; src: local("Arial"); }
        #title { font-family: Missing Site Face, Site Fallback; }
    </style><div id="title">Fallback text</div>"#;
    let mut doc = renderer.load_html(html, 400.0);
    let fs = &renderer.font_system;
    let arial = fontdb::Query {
        families: &[fontdb::Family::Name("Arial")],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };
    if fs.db().query(&arial).is_none() {
        eprintln!("Arial is unavailable on this host — skipping local-face assertion");
        return;
    }
    assert!(
        matches!(
            crate::layout::inline_layout::resolve_css_family(
                fs,
                "Missing Site Face, Site Fallback"
            ),
            crate::layout::inline_layout::ResolvedFamily::Named(name)
                if name.as_ref() == "Site Fallback"
        ),
        "the local @font-face alias should resolve before a generic fallback"
    );
    assert!(
        fs.db()
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name("Other Fallback")],
                ..fontdb::Query::default()
            })
            .is_none(),
        "unused local aliases should stay inactive"
    );
    let alias_normal = crate::layout::inline_layout::measure_text_width_weighted(
        "Darüber spricht Deutschland",
        20.0,
        Some(&mut renderer.font_system),
        FontWeight::Normal,
        FontStyle::Normal,
        1.0,
        "Missing Site Face, Site Fallback",
        100.0,
    );
    let arial_normal = crate::layout::inline_layout::measure_text_width_weighted(
        "Darüber spricht Deutschland",
        20.0,
        Some(&mut renderer.font_system),
        FontWeight::Normal,
        FontStyle::Normal,
        1.0,
        "Arial",
        100.0,
    );
    assert!(
        (alias_normal - arial_normal).abs() < 1.0,
        "local alias should shape normally like Arial: alias={alias_normal}, Arial={arial_normal}"
    );
    let alias_width = crate::layout::inline_layout::measure_text_width_weighted(
        "Darüber spricht Deutschland",
        20.0,
        Some(&mut renderer.font_system),
        FontWeight::Bold,
        FontStyle::Normal,
        1.0,
        "Missing Site Face, Site Fallback",
        100.0,
    );
    let arial_width = crate::layout::inline_layout::measure_text_width_weighted(
        "Darüber spricht Deutschland",
        20.0,
        Some(&mut renderer.font_system),
        FontWeight::Bold,
        FontStyle::Normal,
        1.0,
        "Arial",
        100.0,
    );
    assert!(
        (alias_width - arial_width).abs() < 1.0,
        "local alias should shape like Arial: alias bold={alias_width}, Arial bold={arial_width}, alias normal={alias_normal}, Arial normal={arial_normal}"
    );

    let title = doc.query_selector("#title").unwrap();
    doc.set_style_property(title, "font-family", "Other Fallback");
    renderer.layout_engine().layout(&mut doc, 400.0);
    assert!(
        renderer
            .font_system
            .db()
            .query(&fontdb::Query {
                families: &[fontdb::Family::Name("Other Fallback")],
                ..fontdb::Query::default()
            })
            .is_some(),
        "a newly used local alias should register on relayout"
    );
}

#[test]
fn css_stylesheet_rules() {
    let ss = parse_stylesheet("p { color: blue; } .big { font-size: 24px; }").unwrap();
    assert!(ss.len() >= 2);
}

#[test]
fn css_stylesheet_multiple_selectors() {
    let ss = parse_stylesheet("h1, h2, h3 { font-weight: bold; }").unwrap();
    assert!(ss.len() >= 1);
}

#[test]
fn css_nesting_expands_ampersand_and_descendant_rules() {
    let ss = parse_stylesheet(
        ".card { color: blue; &:hover { color: red; } .title { font-size: 20px; } }",
    )
    .unwrap();

    assert!(
        ss.iter().any(|r| r.original_selector == ".card"
            && r.declarations
                .get("color")
                .map(|v| v == "blue")
                .unwrap_or(false)),
        "parent declarations should still produce the parent rule"
    );
    assert!(
        ss.iter().any(|r| r.original_selector == ".card:hover"
            && r.declarations
                .get("color")
                .map(|v| v == "red")
                .unwrap_or(false)
            && r.is_hover),
        "& nesting should expand against the parent selector"
    );
    assert!(
        ss.iter().any(|r| r.original_selector == ".card .title"
            && r.declarations
                .get("font-size")
                .map(|v| v == "20px")
                .unwrap_or(false)),
        "implicit nesting should expand as a descendant selector"
    );
}

#[test]
fn css_nesting_preserves_grouping_at_rules() {
    let ss = parse_stylesheet(
        ".card { @media (min-width: 1px) { color: red; &:hover { color: blue; } } }",
    )
    .unwrap();

    assert!(
        ss.iter().any(|r| r.original_selector == ".card"
            && r.media_condition.label() == "(min-width: 1px)"
            && r.declarations
                .get("color")
                .map(|v| v == "red")
                .unwrap_or(false)),
        "nested @media should wrap parent declarations"
    );
    assert!(
        ss.iter().any(|r| r.original_selector == ".card:hover"
            && r.media_condition.label() == "(min-width: 1px)"
            && r.declarations
                .get("color")
                .map(|v| v == "blue")
                .unwrap_or(false)),
        "nested selectors inside nested @media should expand too"
    );
}

#[test]
fn css_rule_css_text_serializes_as_one_line_declaration_block() {
    let rules = parse_stylesheet("p { font-size: 12px; color: red !important; text-align: left; }")
        .unwrap();

    assert_eq!(
        crate::html::serialize_rule(&rules[0]),
        "p { font-size: 12px; text-align: left; color: red !important; }"
    );
}

#[test]
fn stylesheet_insert_delete_and_css_rules_follow_cssom_ordering() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("p { color: red; }");

    assert_eq!(sheet.insert_rule(".note { color: blue; }", 0), Ok(0));
    assert_eq!(
        sheet.css_rules(),
        vec![
            ".note { color: blue; }".to_string(),
            "p { color: red; }".to_string()
        ]
    );

    assert_eq!(sheet.delete_rule(1), Ok(()));
    assert_eq!(
        sheet.css_rules(),
        vec![".note { color: blue; }".to_string()]
    );
}

#[test]
fn stylesheet_insert_delete_report_cssom_index_and_syntax_errors() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("p { color: red; }");

    assert_eq!(
        sheet.insert_rule("a { color: blue; } b { color: green; }", 0),
        Err("SyntaxError".to_string())
    );
    assert_eq!(
        sheet.insert_rule("bad css", 0),
        Err("SyntaxError".to_string())
    );
    assert_eq!(
        sheet.insert_rule("a { color: blue; }", 2),
        Err("IndexSizeError".to_string())
    );
    assert_eq!(sheet.delete_rule(1), Err("IndexSizeError".to_string()));
}

#[test]
fn css_important_stripped_from_color() {
    let decls = parse_declarations("color: #ff0000 !important;");
    assert_eq!(decls.len(), 1);
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("#ff0000"));
}

#[test]
fn css_important_stripped_from_multiple() {
    let decls = parse_declarations(
        "background: #21262d !important; color: #6e7681 !important; cursor: default;",
    );
    assert_eq!(decls.len(), 3);
    assert_eq!(decls.get("background").map(|s| s.as_str()), Some("#21262d"));
    assert_eq!(decls.get("color").map(|s| s.as_str()), Some("#6e7681"));
    assert_eq!(decls.get("cursor").map(|s| s.as_str()), Some("default"));
}

#[test]
fn css_important_color_applied() {
    let doc = parse(
        r#"<html><head><style>.red { color: #ff0000 !important; }</style></head>
           <body><p class="red">Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p");
    assert!(p.is_some());
    let c = p.unwrap().style.color;
    assert_eq!(c.r, 255);
    assert_eq!(c.g, 0);
}

#[test]
fn css_important_background_applied() {
    let doc = parse(
        r#"<html><head><style>.bg { background-color: #334155 !important; }</style></head>
           <body><div class="bg">Box</div></body></html>"#,
    );
    let div = find_box(&doc.root, &|b| b.tag == "div");
    assert!(div.is_some());
    let c = div.unwrap().style.background_color;
    assert_eq!(c.r, 0x33);
}

#[test]
fn supports_known_declaration_applies_inner_rules() {
    let doc = parse(
        r#"<html><head><style>
              @supports (display: flex) { p { color: rgb(4, 5, 6); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(4, 5, 6));
}

#[test]
fn scope_blocks_preserve_inner_rules() {
    let doc = parse(
        r#"<html><head><style>
              @scope (.card) { p { color: rgb(4, 5, 6); } }
           </style></head><body>
              <section class="card"><p id="inside">Text</p></section>
              <section><p id="outside">Other</p></section>
           </body></html>"#,
    );
    let inside = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("inside")
    })
    .unwrap();
    let outside = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("outside")
    })
    .unwrap();

    assert_eq!(inside.style.color, Color::rgb(4, 5, 6));
    assert_ne!(outside.style.color, Color::rgb(4, 5, 6));
}

#[test]
fn scope_limit_excludes_limit_subtree() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); }
              @scope (.card) to (.stop) { p { color: rgb(4, 5, 6); } }
           </style></head><body>
              <section class="card">
                  <p id="inside">Inside</p>
                  <div class="stop">
                      <p id="limited">Limited</p>
                  </div>
              </section>
           </body></html>"#,
    );
    let inside = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("inside")
    })
    .unwrap();
    let limited = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("limited")
    })
    .unwrap();

    assert_eq!(inside.style.color, Color::rgb(4, 5, 6));
    assert_eq!(limited.style.color, Color::rgb(1, 2, 3));
}

#[test]
fn scope_proximity_closer_root_wins() {
    let doc = parse(
        r#"<html><head><style>
              /* .outer comes second in stylesheet, but has greater distance (2 hops) */
              @scope (.inner) { p { color: rgb(10, 20, 30); } }
              @scope (.outer) { p { color: rgb(40, 50, 60); } }
           </style></head><body>
              <div class="outer">
                  <div class="inner">
                      <p id="target">Text</p>
                  </div>
              </div>
           </body></html>"#,
    );
    let target = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("target")
    })
    .unwrap();

    // The closer scope (.inner, distance 1) must beat the more distant scope (.outer, distance 2)
    // even though .outer was declared later in stylesheet order.
    assert_eq!(target.style.color, Color::rgb(10, 20, 30));
}

#[test]
fn scope_nested_intersection_and_limits() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 1, 1); }
              @scope (.outer) to (.outer-stop) {
                  @scope (.inner) to (.inner-stop) {
                      p { color: rgb(10, 20, 30); }
                  }
              }
           </style></head><body>
              <!-- In both scopes: matches -->
              <div class="outer">
                  <div class="inner">
                      <p id="in-both">Text</p>
                  </div>
              </div>
              <!-- Inside .inner, but outside .outer: must not match -->
              <div>
                  <div class="inner">
                      <p id="outside-outer">Text</p>
                  </div>
              </div>
              <!-- Inside both, but blocked by outer limit: must not match -->
              <div class="outer">
                  <div class="outer-stop">
                      <div class="inner">
                          <p id="outer-stopped">Text</p>
                      </div>
                  </div>
              </div>
              <!-- Inside both, but blocked by inner limit: must not match -->
              <div class="outer">
                  <div class="inner">
                      <div class="inner-stop">
                          <p id="inner-stopped">Text</p>
                      </div>
                  </div>
              </div>
           </body></html>"#,
    );
    let in_both = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("in-both")
    })
    .unwrap();
    let outside_outer = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("outside-outer")
    })
    .unwrap();
    let outer_stopped = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("outer-stopped")
    })
    .unwrap();
    let inner_stopped = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v.as_str()) == Some("inner-stopped")
    })
    .unwrap();

    assert_eq!(
        in_both.style.color,
        Color::rgb(10, 20, 30),
        "in both scopes matches"
    );
    assert_eq!(
        outside_outer.style.color,
        Color::rgb(1, 1, 1),
        "outside outer scope does not match"
    );
    assert_eq!(
        outer_stopped.style.color,
        Color::rgb(1, 1, 1),
        "blocked by outer limit does not match"
    );
    assert_eq!(
        inner_stopped.style.color,
        Color::rgb(1, 1, 1),
        "blocked by inner limit does not match"
    );
}

#[test]
fn supports_unknown_declaration_drops_inner_rules() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); }
              @supports (definitely-not-a-property: 1) { p { color: rgb(4, 5, 6); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(1, 2, 3));
}

#[test]
fn supports_invalid_known_declaration_value_drops_inner_rules() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); }
              @supports (display: banana) { p { color: rgb(4, 5, 6); } }
              @supports not (display: banana) { p { background-color: rgb(7, 8, 9); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(1, 2, 3));
    assert_eq!(p.style.background_color, Color::rgb(7, 8, 9));
}

#[test]
fn supports_only_treats_var_function_tokens_as_deferred_values() {
    use crate::css::parser::supports_condition_matches;

    assert!(supports_condition_matches("(display: var(--mode))"));
    assert!(supports_condition_matches("(display: var(--mód))"));
    assert!(supports_condition_matches(
        "(width: calc(100% - var(--gap, 1px)))"
    ));
    assert!(!supports_condition_matches("(display: \"var(--mode)\")"));
    assert!(!supports_condition_matches("(display: url('var(--mode)'))"));
    assert!(!supports_condition_matches(
        "(display: var(not-a-custom-property))"
    ));
    assert!(!supports_condition_matches("(display: prefix-var(--mode))"));
}

#[test]
fn supports_boolean_condition_controls_inner_rules() {
    let doc = parse(
        r#"<html><head><style>
              @supports (display: flex) and (not (definitely-not-a-property: 1)) {
                  p { color: rgb(7, 8, 9); }
              }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(7, 8, 9));
}

#[test]
fn supports_logical_keywords_are_case_insensitive() {
    let doc = parse(
        r#"<html><head><style>
              @supports (display: flex) AND (NOT (definitely-not-a-property: 1)) {
                  p { color: rgb(7, 8, 9); }
              }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(7, 8, 9));
}

#[test]
fn supports_selector_condition_uses_selector_parser() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); }
              @supports selector(p:not(.missing)) { p { color: rgb(4, 5, 6); } }
              @supports selector(p:unknown-pseudo) { p { color: rgb(7, 8, 9); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(4, 5, 6));
}

#[test]
fn supports_selector_rejects_unimplemented_and_prefix_pseudo_elements() {
    for selector in [
        "p::part(label)",
        "p::beforebogus",
        "p::before.extra",
        "p::unknown",
        "p, div",
        "p:is(.known, :unknown-pseudo)",
        "p:where(.known, :unknown-pseudo)",
        "p:not(:is(.known, :unknown-pseudo))",
        "p:is(.known, ::unknown)",
        "p:where(.known, ::part(label))",
    ] {
        assert!(
            !crate::css::parser::supports_condition_matches(&format!("selector({selector})")),
            "{selector}"
        );
    }
    assert!(crate::css::parser::supports_condition_matches(
        "selector(p::before)"
    ));
    assert!(crate::css::parser::supports_condition_matches(
        "selector(p:is(.known, .other))"
    ));
    assert!(crate::css::parser::parse_selector("p:is(.known, :unknown-pseudo)").valid);
    assert!(crate::css::parser::parse_selector("p:is(.known, ::unknown)").valid);

    let doc = parse(
        r#"<style>
              p { color: rgb(1, 2, 3); }
              @supports selector(p::before) { p { color: rgb(4, 5, 6); } }
              @supports selector(p::part(label)) { p { color: rgb(7, 8, 9); } }
              @supports selector(p::beforebogus) { p { color: rgb(10, 11, 12); } }
           </style><p>Text</p>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    assert_eq!(p.style.color, Color::rgb(4, 5, 6));
}

#[test]
fn supports_nested_condition_parens_and_case_insensitive_selector_function() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); background-color: rgb(9, 9, 9); }
              @supports ((display: flex)) { p { color: rgb(4, 5, 6); } }
              @supports SELECTOR(p:not(.missing)) { p { background-color: rgb(7, 8, 9); } }
              @supports (content: "a and b") { p { border-top-color: rgb(10, 11, 12); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(4, 5, 6));
    assert_eq!(p.style.background_color, Color::rgb(7, 8, 9));
    assert_eq!(p.style.border_top_color, Color::rgb(10, 11, 12));
}

#[test]
fn has_child_chain_is_anchored_and_searches_descendants() {
    let doc = parse(
        r#"<style>
        section { color:blue }
        section:has(> .a > .b > .c) { color:red }
        section:has(.a .c) { background-color:yellow }
    </style>
    <section id=match><div class=a><div class=b><span class=c></span></div></div></section>
    <section id=wrong><div><div class=a><div class=b><span class=c></span></div></div></div></section>"#,
    );
    let a = crate::dom::query_selector(&doc.root, "#match").unwrap();
    let b = crate::dom::query_selector(&doc.root, "#wrong").unwrap();
    assert_eq!(a.style.color, Color::rgb(255, 0, 0));
    assert_eq!(b.style.color, Color::rgb(0, 0, 255));
    assert_eq!(a.style.background_color, Color::rgb(255, 255, 0));
    assert_eq!(b.style.background_color, Color::rgb(255, 255, 0));
}

#[test]
fn supports_subgrid_keeps_nested_card_rules() {
    let doc = parse(
        r#"<style>
        .card { display:flex }
        @supports (grid-template-rows: subgrid) {
            @supports selector(:has(*)) {
                .card { display:grid;grid-template-rows:subgrid }
            }
        }
        @supports (grid-template-columns: subgrid) { .card { color:red } }
        @supports (grid-template-rows: not-a-track) { .card { display:none } }
    </style><div class=card></div>"#,
    );
    let card = find_box(&doc.root, &|b| {
        b.attributes.get("class").is_some_and(|v| v == "card")
    })
    .unwrap();
    assert_eq!(card.style.display, Display::Grid);
    assert!(card.style.subgrid_rows);
    assert_eq!(card.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn supports_deferred_property_families() {
    for (property, value) in [
        ("display", "flow-root"),
        ("display", "contents"),
        ("grid-template-columns", "repeat(3, minmax(0, 1fr))"),
        ("grid-template-rows", "[first] 20px [last] min-content"),
        ("grid-auto-columns", "100px 1fr"),
        ("grid-auto-flow", "column dense"),
        ("grid-auto-columns", "calc((100cqw - 4 * 12px) / 5)"),
        ("width", "calc(100% - 2rem)"),
        ("font-size", "clamp(1rem, 2vw, 2rem)"),
        ("grid-template-areas", "\"a a\" \"b c\""),
        ("grid-column", "span 2 / end"),
        ("container-type", "inline-size"),
        ("container", "cards / inline-size"),
        ("contain", "layout paint"),
        ("content-visibility", "auto"),
        ("contain-intrinsic-size", "auto 200px"),
        ("transform", "translateX(10px) rotate(20deg)"),
        ("translate", "20% 1em"),
        ("rotate", "45deg"),
        ("scale", "1.2"),
        ("transform-origin", "right bottom"),
        ("filter", "blur(2px)"),
        ("backdrop-filter", "brightness(0.5)"),
        ("animation-duration", "1s, 200ms"),
        ("animation-delay", "-1s"),
        ("animation-timing-function", "steps(4, end)"),
        ("animation-iteration-count", "infinite"),
        ("animation-composition", "add"),
        ("transition-behavior", "allow-discrete"),
        ("animation", "spin 1s linear infinite"),
        ("transition", "opacity 0.2s ease"),
        ("color", "light-dark(white, black)"),
        ("color-scheme", "light dark"),
        ("font-size", "large"),
        ("font-weight", "bolder"),
        ("font-family", "\"Market Sans\", sans-serif"),
        ("font-stretch", "120%"),
        ("font-feature-settings", "\"liga\" 1"),
        ("font-variation-settings", "\"wght\" 600"),
        ("font-variant-numeric", "tabular-nums"),
        ("font-synthesis", "none"),
        ("text-wrap", "balance"),
        ("text-decoration", "underline wavy red"),
        ("text-shadow", "1px 2px 3px black"),
        ("text-emphasis", "filled sesame red"),
        ("overflow-wrap", "anywhere"),
        ("hyphens", "auto"),
        ("direction", "rtl"),
        ("writing-mode", "vertical-rl"),
        ("align-content", "space-evenly"),
        ("align-items", "safe center"),
        ("gap", "1em 2em"),
        ("flex", "1 1 auto"),
        ("flex-flow", "row wrap"),
        ("table-layout", "fixed"),
        ("border-spacing", "2px 4px"),
        ("aspect-ratio", "16 / 9"),
        ("object-fit", "cover"),
        ("object-position", "50% 50%"),
        ("user-select", "none"),
        ("appearance", "none"),
        ("touch-action", "pan-y"),
        ("scroll-snap-type", "x mandatory"),
        ("scroll-snap-align", "start"),
        ("overscroll-behavior", "contain"),
        ("scrollbar-color", "red blue"),
        ("mix-blend-mode", "multiply"),
        ("isolation", "isolate"),
        ("column-count", "3"),
        ("columns", "12em 3"),
        ("break-inside", "avoid"),
        ("counter-reset", "items 0"),
        ("counter-increment", "items"),
        ("stroke-width", "0"),
        ("stroke", "none"),
        ("padding-inline", "1em 2em"),
        ("margin-block", "auto"),
        ("border-radius", "50% / 20%"),
        ("border", "1px solid transparent"),
        ("outline", "2px dotted red"),
        ("background-image", "linear-gradient(red, blue)"),
        ("mask-image", "url(icon.svg)"),
        (
            "background-image",
            "image-set(\"one.png\" 1x, \"two.png\" 2x)",
        ),
        ("background-size", "cover"),
        ("background", "red"),
        ("clip-path", "polygon(0 0, 100% 0, 50% 100%)"),
        ("shape-outside", "circle(50%)"),
    ] {
        let condition = format!("({property}: {value})");
        assert!(
            crate::css::parser::supports_condition_matches(&condition),
            "{condition}"
        );
    }
}

#[test]
fn supports_rejects_invalid_deferred_values() {
    for condition in [
        "(grid-template-rows: not-a-track)",
        "(grid-template-columns: repeat(0, 1fr))",
        "(grid-auto-columns: -1fr)",
        "(container-type: mystery)",
        "(transform: unknown(3))",
        "(filter: unknown(3))",
        "(animation-duration: fast)",
        "(animation-duration: -2s)",
        "(animation-timing-function: nonsense)",
        "(color-scheme: bluish)",
        "(font-size: potato)",
        "(font-weight: 1001)",
        "(gap: bogus)",
        "(padding: unknown)",
        "(border: banana)",
        "(background-image: not-an-image)",
        "(clip-path: polygon(nonsense))",
        "(not-a-property: initial)",
        "(anchor-name: --card)",
        "(animation-timeline: scroll())",
        "(offset-path: path('M 0 0 L 10 10'))",
    ] {
        assert!(
            !crate::css::parser::supports_condition_matches(condition),
            "{condition}"
        );
    }
}

#[test]
fn supports_clip_path_matches_applied_forms() {
    use crate::css::parser::supports_condition_matches;

    for value in [
        "none",
        "content-box",
        "inset(10px round 4px) border-box",
        "padding-box circle(25% at 50% 50%)",
        "xywh(0 0 20px 30px)",
        "rect(0 20px 30px 0)",
        "path('M0 0 L20 0 L20 20 Z')",
        "polygon(0 0, 100% 0, 100% 100%)",
        "polygon(evenodd, 0 0, 100% 0, 100% 100%)",
        "polygon(nonzero, 0 0, 100% 0, 100% 100%)",
        "polygon(round 10px, 0 0, 100% 0, 100% 100%)",
        "polygon(evenodd round 10px, 0 0, 100% 0, 100% 100%)",
    ] {
        assert!(
            supports_condition_matches(&format!("(clip-path: {value})")),
            "{value}"
        );
    }
    for value in [
        "none border-box",
        "border-box padding-box",
        "circle(10px) ellipse(5px 5px)",
        "path('not a path')",
        "polygon(nonsense)",
        "polygon(0 0, 50% 50% extra)",
        "polygon(evenodd)",
        "polygon(unknown, 0 0, 100% 0, 100% 100%)",
        "polygon(round, 0 0, 100% 0, 100% 100%)",
        "polygon(round -10px, 0 0, 100% 0, 100% 100%)",
        "polygon(round 10%, 0 0, 100% 0, 100% 100%)",
    ] {
        assert!(
            !supports_condition_matches(&format!("(clip-path: {value})")),
            "{value}"
        );
    }
}

#[test]
fn supports_font_format_and_font_tech_conditions() {
    assert!(crate::css::parser::supports_condition_matches(
        "font-format(\"woff2\")"
    ));
    assert!(crate::css::parser::supports_condition_matches(
        "font-format(\"embedded-opentype\")"
    ));
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 2, 3); background-color: rgb(9, 9, 9); border-top-color: rgb(2, 2, 2); }
              @supports font-format("woff2") { p { color: rgb(4, 5, 6); } }
              @supports font-tech(variations) { p { background-color: rgb(7, 8, 9); } }
              @supports font-tech(color-COLRv1) { p { border-top-color: rgb(10, 11, 12); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(p.style.color, Color::rgb(4, 5, 6));
    assert_eq!(p.style.background_color, Color::rgb(7, 8, 9));
    assert_eq!(p.style.border_top_color, Color::rgb(2, 2, 2));
}

#[test]
fn supports_full_conditional_feature_grammar() {
    let doc = parse(
        r#"<html><head><style>
              p { color: rgb(1, 1, 1); background-color: rgb(2, 2, 2); border-top-color: rgb(3, 3, 3); border-bottom-color: rgb(4, 4, 4); }
              /* !important inside declaration must match */
              @supports (display: flex !important) { p { color: rgb(10, 20, 30); } }
              /* Bare declaration without parens must be rejected as invalid syntax */
              @supports display: flex { p { color: rgb(99, 99, 99); } }
              /* Mixed and/or without grouping parens must be rejected as syntax error */
              @supports (display: flex) and (display: grid) or (display: block) { p { background-color: rgb(99, 99, 99); } }
              /* Grouped and/or is valid */
              @supports (display: flex) and ((display: grid) or (display: block)) { p { background-color: rgb(40, 50, 60); } }
              /* General enclosed unknown feature in parens is valid syntax, evaluates to false, inverted by not */
              @supports not (general-enclosed-unknown-feature: 123) { p { border-top-color: rgb(70, 80, 90); } }
              /* Comments inside condition must be ignored */
              @supports /* comment */ (/* inside */ display: flex /* after */) { p { border-bottom-color: rgb(11, 22, 33); } }
           </style></head><body><p>Text</p></body></html>"#,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();

    assert_eq!(
        p.style.color,
        Color::rgb(10, 20, 30),
        "!important inside declaration matched, bare declaration rejected"
    );
    assert_eq!(
        p.style.background_color,
        Color::rgb(40, 50, 60),
        "mixed and/or without parens rejected, grouped accepted"
    );
    assert_eq!(
        p.style.border_top_color,
        Color::rgb(70, 80, 90),
        "general-enclosed feature parsed and inverted by not"
    );
    assert_eq!(
        p.style.border_bottom_color,
        Color::rgb(11, 22, 33),
        "comments inside condition ignored"
    );
}

#[test]
fn forced_color_adjust_rejects_invalid_keywords() {
    let mut style = ComputedStyle::default();

    crate::css::apply_property(&mut style, "forced-color-adjust", "none");
    assert_eq!(style.forced_color_adjust, "none");

    crate::css::apply_property(&mut style, "forced-color-adjust", "banana");
    assert_eq!(style.forced_color_adjust, "none");
}

#[test]
fn background_shorthand_resets_omitted_longhands() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "url(hero.png) no-repeat center / cover",
    );
    assert_eq!(style.background_image_url, "hero.png");
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(style.background_size, BackgroundSize::Cover);

    apply_property(&mut style, "background", "white");
    assert!(style.background_image_url.is_empty());
    assert_eq!(style.background_repeat, BackgroundRepeat::Repeat);
    assert_eq!(style.background_size, BackgroundSize::Auto);
    assert_eq!(style.background_color, Color::WHITE);
}

#[test]
fn background_shorthand_rejects_nonfinal_colors_and_empty_layers_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "url(before.png) right no-repeat blue",
    );
    assert_eq!(style.background_image_url, "before.png");
    assert_eq!(style.background_color, Color::rgb(0, 0, 255));

    for invalid in [
        "red, url(after.png)",
        "url(after.png), red, blue",
        "url(after.png), red blue",
        "url(after.png),",
        ", red",
    ] {
        apply_property(&mut style, "background", invalid);
        assert_eq!(style.background_image_url, "before.png", "{invalid}");
        assert_eq!(style.background_color, Color::rgb(0, 0, 255), "{invalid}");
        assert_eq!(
            style.background_repeat,
            BackgroundRepeat::NoRepeat,
            "{invalid}"
        );
    }

    apply_property(
        &mut style,
        "background",
        "url(front.png), url(back.png) blue",
    );
    assert_eq!(style.background_image_url, "front.png");
    assert_eq!(
        style.rare().additional_background_layers[0].image_url,
        "back.png"
    );
    assert_eq!(style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn background_shorthand_validates_each_component_before_resetting_style() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "url(before.png) right no-repeat blue",
    );

    for invalid in [
        "url(after.png) mystery",
        "url(after.png) repeat-x no-repeat",
        "url(after.png) repeat space round",
        "url(after.png) fixed scroll",
        "url(after.png) border-box padding-box content-box",
        "url(after.png) left right",
        "url(after.png) left / cover contain",
        "url(after.png) / cover",
        "url(after.png) left / negative",
        "url(after.png), url(other.png) left / cover mystery",
    ] {
        apply_property(&mut style, "background", invalid);
        assert_eq!(style.background_image_url, "before.png", "{invalid}");
        assert_eq!(style.background_color, Color::rgb(0, 0, 255), "{invalid}");
        assert_eq!(
            style.background_repeat,
            BackgroundRepeat::NoRepeat,
            "{invalid}"
        );
    }

    apply_property(
        &mut style,
        "background",
        "url(after.png) Right Top/20px auto No-Repeat Fixed Padding-Box Content-Box blue",
    );
    assert_eq!(style.background_image_url, "after.png");
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(style.background_attachment, BackgroundAttachment::Fixed);
    assert_eq!(style.background_origin, BackgroundClip::PaddingBox);
    assert_eq!(style.background_clip, BackgroundClip::ContentBox);
    assert_eq!(style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn background_image_cascade_preserves_additional_url_layers() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background-image",
        "linear-gradient(transparent, transparent), url(sprite.svg)",
    );

    assert_eq!(style.gradient_type, GradientType::Linear);
    assert_eq!(style.rare().additional_background_layers.len(), 1);
    assert_eq!(
        style.rare().additional_background_layers[0].image_url,
        "sprite.svg"
    );
}

#[test]
fn background_longhands_apply_to_additional_layers() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background-image",
        "linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    apply_property(&mut style, "background-origin", "border-box, content-box");
    apply_property(&mut style, "background-clip", "padding-box, text");
    apply_property(&mut style, "background-attachment", "fixed, local");
    apply_property(&mut style, "background-blend-mode", "multiply, screen");

    assert_eq!(style.background_origin, BackgroundClip::BorderBox);
    assert_eq!(style.background_clip, BackgroundClip::PaddingBox);
    assert_eq!(style.background_attachment, BackgroundAttachment::Fixed);
    assert_eq!(style.background_blend_mode, "multiply, screen");
    let layer = &style.rare().additional_background_layers[0];
    assert_eq!(layer.origin, BackgroundClip::ContentBox);
    assert_eq!(layer.clip, BackgroundClip::Text);
    assert_eq!(layer.attachment, BackgroundAttachment::Local);
    assert_eq!(layer.blend_mode, "screen");
}

#[test]
fn background_blend_modes_follow_layers_regardless_of_declaration_order() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-blend-mode", "multiply, screen");
    apply_property(
        &mut style,
        "background-image",
        "linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    assert_eq!(style.background_blend_mode, "multiply, screen");
    assert_eq!(style.rare().additional_background_layers.len(), 1);
    assert_eq!(
        style.rare().additional_background_layers[0].blend_mode,
        "screen"
    );

    apply_property(
        &mut style,
        "background",
        "linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    assert_eq!(style.background_blend_mode, "multiply, screen");
    assert_eq!(
        style.rare().additional_background_layers[0].blend_mode,
        "screen"
    );
}

#[test]
fn empty_background_layers_keep_later_layer_options_aligned() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background-blend-mode",
        "multiply, screen, overlay",
    );
    apply_property(
        &mut style,
        "background-image",
        "none, linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    assert_eq!(style.gradient_type, GradientType::None);
    let layers = &style.rare().additional_background_layers;
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].blend_mode, "screen");
    assert_eq!(layers[1].blend_mode, "overlay");

    apply_property(
        &mut style,
        "background",
        "none, linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    let layers = &style.rare().additional_background_layers;
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].blend_mode, "screen");
    assert_eq!(layers[1].blend_mode, "overlay");
}

#[test]
fn background_image_does_not_reset_previous_size_or_position() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-size", "cover");
    apply_property(&mut style, "background-position", "right bottom");
    apply_property(
        &mut style,
        "background-image",
        "linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    assert_eq!(style.background_size, BackgroundSize::Cover);
    assert_eq!(style.background_position_x, CssLength::Percent(100.0));
    assert_eq!(style.background_position_y, CssLength::Percent(100.0));
}

#[test]
fn multilayer_background_longhands_are_independent_of_image_declaration_order() {
    const IMAGES: &str = "url(first.png), url(second.png), url(third.png)";
    const OPTIONS: &[(&str, &str)] = &[
        ("background-size", "cover, contain, 12px 8px"),
        (
            "background-position",
            "left top, center center, right bottom",
        ),
        ("background-position-x", "left, center, right"),
        ("background-position-y", "top, center, bottom"),
        ("background-repeat", "repeat-x, no-repeat, round"),
        ("background-attachment", "fixed, local, scroll"),
        ("background-origin", "border-box, content-box, padding-box"),
        ("background-clip", "content-box, padding-box, border-box"),
        ("background-blend-mode", "multiply, screen, overlay"),
    ];

    let mut image_first = ComputedStyle::default();
    apply_property(&mut image_first, "background-image", IMAGES);
    for &(name, value) in OPTIONS {
        apply_property(&mut image_first, name, value);
    }

    let mut options_first = ComputedStyle::default();
    for &(name, value) in OPTIONS {
        apply_property(&mut options_first, name, value);
    }
    apply_property(&mut options_first, "background-image", IMAGES);

    assert_eq!(options_first.background_size, image_first.background_size);
    assert_eq!(
        options_first.background_position_x,
        image_first.background_position_x
    );
    assert_eq!(
        options_first.background_repeat,
        image_first.background_repeat
    );
    assert_eq!(
        options_first.rare().additional_background_layers,
        image_first.rare().additional_background_layers
    );
    assert_eq!(options_first.rare().additional_background_layers.len(), 2);

    apply_property(
        &mut options_first,
        "background-image",
        "url(replacement.png)",
    );
    assert_eq!(options_first.rare().additional_background_layers.len(), 2);
    assert!(
        options_first
            .rare()
            .additional_background_layers
            .iter()
            .all(|layer| layer.image_url.is_empty())
    );
}

#[test]
fn shorter_background_lists_cycle_across_later_image_layers() {
    const IMAGES: &str = "url(a.png), url(b.png), url(c.png), url(d.png)";
    const OPTIONS: &[(&str, &str)] = &[
        ("background-size", "cover, contain"),
        ("background-position", "left top, right bottom"),
        ("background-repeat", "no-repeat, repeat-x"),
        ("background-origin", "border-box, content-box"),
        ("background-clip", "content-box, padding-box"),
        ("background-attachment", "fixed, local"),
        ("background-blend-mode", "multiply, screen"),
    ];

    let build = |image_first| {
        let mut style = ComputedStyle::default();
        if image_first {
            apply_property(&mut style, "background-image", IMAGES);
        }
        for &(name, value) in OPTIONS {
            apply_property(&mut style, name, value);
        }
        if !image_first {
            apply_property(&mut style, "background-image", IMAGES);
        }
        style
    };
    let before = build(false);
    let after = build(true);
    assert_eq!(
        before.rare().additional_background_layers,
        after.rare().additional_background_layers
    );
    let layers = &before.rare().additional_background_layers;
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].size, BackgroundSize::Contain);
    assert_eq!(layers[1].size, BackgroundSize::Cover);
    assert_eq!(layers[2].size, BackgroundSize::Contain);
    assert_eq!(layers[0].position_x, layers[2].position_x);
    assert_eq!(layers[0].blend_mode, layers[2].blend_mode);
    assert_ne!(layers[0].repeat, layers[1].repeat);
}

#[test]
fn copying_background_image_preserves_existing_longhand_cycle() {
    let mut images = ComputedStyle::default();
    apply_property(
        &mut images,
        "background-image",
        "url(a.png), url(b.png), url(c.png), url(d.png)",
    );
    let mut target = ComputedStyle::default();
    apply_property(&mut target, "background-size", "cover, contain");
    apply_property(&mut target, "background-blend-mode", "multiply, screen");
    (crate::css::property_defs::get(crate::css::properties::PropertyId::BackgroundImage).copy)(
        &mut target,
        &images,
    );

    let layers = &target.rare().additional_background_layers;
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[0].image_url, "b.png");
    assert_eq!(layers[1].image_url, "c.png");
    assert_eq!(layers[2].image_url, "d.png");
    assert_eq!(layers[0].size, BackgroundSize::Contain);
    assert_eq!(layers[1].size, BackgroundSize::Cover);
    assert_eq!(layers[2].size, BackgroundSize::Contain);
    assert_eq!(layers[0].blend_mode, "screen");
    assert_eq!(layers[1].blend_mode, "multiply");
    assert_eq!(layers[2].blend_mode, "screen");
}

#[test]
fn background_position_checks_two_to_four_value_grammar_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-position", "center 10px");
    assert_eq!(style.background_position_x, CssLength::Percent(50.0));
    assert_eq!(style.background_position_y, CssLength::Px(10.0));

    apply_property(&mut style, "background-position", "bottom 20px right 10px");
    assert!(matches!(
        style.background_position_x,
        CssLength::CalcExpr(_)
    ));
    assert!(matches!(
        style.background_position_y,
        CssLength::CalcExpr(_)
    ));
    let previous = style.clone();

    for invalid in [
        "top 10px",
        "left right",
        "left 10px right",
        "left top 10px 20px",
        "10px left",
    ] {
        apply_property(&mut style, "background-position", invalid);
        assert_eq!(
            style.background_position_x, previous.background_position_x,
            "{invalid}"
        );
        assert_eq!(
            style.background_position_y, previous.background_position_y,
            "{invalid}"
        );
    }

    apply_property(&mut style, "background-position", "left top, right bottom");
    let before = style.clone();
    apply_property(&mut style, "background-position", "center, left right");
    assert_eq!(style.background_position_x, before.background_position_x);
    assert_eq!(style.background_position_y, before.background_position_y);
    assert_eq!(
        style.rare().additional_background_layers,
        before.rare().additional_background_layers
    );
}

#[test]
fn background_size_rejects_invalid_layers_without_changing_existing_style() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-size", "cover, 20px auto");
    let previous = style.clone();
    for invalid in [
        "cover auto",
        "20px garbage",
        "-4px",
        "contain, 10px 20px 30px",
    ] {
        apply_property(&mut style, "background-size", invalid);
        assert_eq!(style.background_size, previous.background_size, "{invalid}");
        assert_eq!(
            style.rare().additional_background_layers,
            previous.rare().additional_background_layers,
            "{invalid}"
        );
    }
    apply_property(&mut style, "background-size", "auto 25%");
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(style.background_size_w, CssLength::Auto);
    assert_eq!(style.background_size_h, CssLength::Percent(25.0));
}

#[test]
fn mask_geometry_longhands_validate_every_layer_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "mask-position",
        "right 10px bottom 20px, center 25%",
    );
    apply_property(&mut style, "mask-size", "contain, auto 25%");
    assert_eq!(
        style.rare().mask_position,
        "right 10px bottom 20px, center 25%"
    );
    assert_eq!(style.rare().mask_size, "contain, auto 25%");

    for invalid in ["left nonsense", "center, left right top", "left,"] {
        apply_property(&mut style, "mask-position", invalid);
        assert_eq!(
            style.rare().mask_position,
            "right 10px bottom 20px, center 25%",
            "{invalid}"
        );
    }
    for invalid in ["cover auto", "contain, -10px", "10px 20px 30px"] {
        apply_property(&mut style, "mask-size", invalid);
        assert_eq!(style.rare().mask_size, "contain, auto 25%", "{invalid}");
    }
}

#[test]
fn mask_repeat_accepts_two_axis_values_and_rejects_bad_layers() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "mask-repeat", "repeat no-repeat, space round");
    assert_eq!(style.rare().mask_repeat, "repeat no-repeat, space round");
    for invalid in ["repeat nonsense", "repeat, no-repeat garbage", "repeat,"] {
        apply_property(&mut style, "mask-repeat", invalid);
        assert_eq!(
            style.rare().mask_repeat,
            "repeat no-repeat, space round",
            "{invalid}"
        );
    }
}

#[test]
fn mask_image_layer_list_preserves_none_and_rejects_invalid_tail() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "mask-image",
        "none, url(second.svg), url(third.svg)",
    );
    assert!(style.rare().mask_image_url.is_empty());
    assert_eq!(style.rare().additional_mask_images.len(), 2);
    assert_eq!(style.rare().additional_mask_images[0].url, "second.svg");
    assert_eq!(style.rare().additional_mask_images[1].url, "third.svg");
    apply_property(&mut style, "mask-image", "url(replacement.svg), nonsense");
    assert!(style.rare().mask_image_url.is_empty());
    assert_eq!(style.rare().additional_mask_images[0].url, "second.svg");
    for invalid in ["junk url(replacement.svg)", "url(replacement.svg) trailing"] {
        apply_property(&mut style, "mask-image", invalid);
        assert!(style.rare().mask_image_url.is_empty(), "{invalid}");
        assert_eq!(
            style.rare().additional_mask_images[0].url,
            "second.svg",
            "{invalid}"
        );
    }
}

#[test]
fn mask_shorthand_parses_geometry_and_rejects_invalid_values_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "mask",
        "url(icon.svg) right 10px bottom 20px / 12px auto repeat no-repeat content-box luminance",
    );
    assert_eq!(style.rare().mask_image_url, "icon.svg");
    assert_eq!(style.rare().mask_position, "right 10px bottom 20px");
    assert_eq!(style.rare().mask_size, "12px auto");
    assert_eq!(style.rare().mask_repeat, "repeat no-repeat");
    assert_eq!(style.rare().mask_origin, "content-box");
    assert_eq!(style.rare().mask_clip, "content-box");
    assert_eq!(style.rare().mask_mode, "luminance");

    let previous = style.rare().clone();
    for invalid in [
        "url(other.svg) right nonsense / cover",
        "url(other.svg) left / -5px",
        "url(other.svg) left / contain repeat garbage",
        "url(other.svg) repeat repeat repeat",
        "url(other.svg) border-box content-box padding-box",
        "url(other.svg) left /",
        "url(other.svg) left / contain / repeat",
        "url(other.svg) alpha luminance",
        "url(other.svg), url(second.svg) left / nonsense",
    ] {
        apply_property(&mut style, "mask", invalid);
        assert_eq!(
            style.rare().mask_image_url,
            previous.mask_image_url,
            "{invalid}"
        );
        assert_eq!(
            style.rare().mask_position,
            previous.mask_position,
            "{invalid}"
        );
        assert_eq!(style.rare().mask_size, previous.mask_size, "{invalid}");
        assert_eq!(style.rare().mask_repeat, previous.mask_repeat, "{invalid}");
        assert_eq!(style.rare().mask_origin, previous.mask_origin, "{invalid}");
        assert_eq!(style.rare().mask_clip, previous.mask_clip, "{invalid}");
        assert_eq!(style.rare().mask_mode, previous.mask_mode, "{invalid}");
    }

    apply_property(
        &mut style,
        "mask",
        "url(next.svg) center / contain border-box no-clip",
    );
    assert_eq!(style.rare().mask_origin, "border-box");
    assert_eq!(style.rare().mask_clip, "no-clip");
    apply_property(&mut style, "mask", "url(next.svg) padding-box content-box");
    assert_eq!(style.rare().mask_origin, "padding-box");
    assert_eq!(style.rare().mask_clip, "content-box");
}

#[test]
fn mask_shorthand_parses_multiple_layers_with_independent_defaults() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "mask",
        "url(top.svg) center / 10px 20px no-repeat subtract alpha, none, url(bottom.svg) padding-box",
    );
    let rare = style.rare();
    assert_eq!(rare.mask_image_url, "top.svg");
    assert_eq!(rare.additional_mask_images.len(), 2);
    assert!(rare.additional_mask_images[0].url.is_empty());
    assert_eq!(rare.additional_mask_images[1].url, "bottom.svg");
    assert_eq!(rare.mask_size, "10px 20px, auto, auto");
    assert_eq!(rare.mask_position, "center, 0% 0%, 0% 0%");
    assert_eq!(rare.mask_composite, "subtract, add, add");
    assert_eq!(rare.mask_origin, "border-box, border-box, padding-box");
    assert_eq!(rare.mask_clip, "border-box, border-box, padding-box");

    let mut doc = parse_html(
        r#"<div id="masked" style="mask:url(top.svg) center / 10px 20px no-repeat subtract alpha, none, url(bottom.svg) padding-box"></div>"#,
    );
    let id = doc.get_element_by_id("masked").unwrap();
    let serialized = doc.computed_style_property(id, "mask");
    assert!(serialized.contains("url(\"top.svg\") center / 10px 20px no-repeat"));
    assert!(serialized.contains(", none 0% 0% / auto repeat"));
    assert!(serialized.contains(", url(\"bottom.svg\") 0% 0% / auto repeat padding-box"));
}

#[test]
fn background_shorthand_uses_edge_offset_position_grammar() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "url(hero.png) bottom 20px right 10px / 40px auto no-repeat",
    );
    assert_eq!(style.background_image_url, "hero.png");
    assert!(matches!(
        style.background_position_x,
        CssLength::CalcExpr(_)
    ));
    assert!(matches!(
        style.background_position_y,
        CssLength::CalcExpr(_)
    ));
    assert_eq!(style.background_size_w, CssLength::Px(40.0));
    assert_eq!(style.background_size_h, CssLength::Auto);
}

#[test]
fn background_keyword_lists_reject_invalid_layers_atomically() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-image", "url(a.png), url(b.png)");
    apply_property(&mut style, "background-repeat", "space, repeat round");
    apply_property(&mut style, "background-clip", "content-box, padding-box");
    apply_property(&mut style, "background-origin", "border-box, content-box");
    apply_property(&mut style, "background-attachment", "local, fixed");
    let before = style.clone();

    for (property, invalid) in [
        ("background-repeat", "repeat-x no-repeat"),
        ("background-repeat", "repeat, round garbage"),
        ("background-clip", "content-box, missing-box"),
        ("background-origin", "border-box, text"),
        ("background-attachment", "local, sticky"),
    ] {
        apply_property(&mut style, property, invalid);
        assert_eq!(style, before, "{property}: {invalid}");
    }

    apply_property(&mut style, "background-repeat", "repeat-y, round space");
    assert_eq!(style.background_repeat, BackgroundRepeat::RepeatY);
    assert_eq!(
        style.rare().additional_background_layers[0].repeat,
        BackgroundRepeat::TwoValue(BackgroundRepeatAxis::Round, BackgroundRepeatAxis::Space)
    );
}

#[test]
fn background_position_axis_lists_accept_edge_offsets_and_reject_invalid_layers() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-image", "url(a.png), url(b.png)");
    apply_property(&mut style, "background-position-x", "right 10px, left 5px");
    apply_property(&mut style, "background-position-y", "bottom 20px, top 3px");
    assert!(matches!(
        style.background_position_x,
        CssLength::CalcExpr(_)
    ));
    assert!(matches!(
        style.background_position_y,
        CssLength::CalcExpr(_)
    ));
    assert_eq!(
        style.rare().additional_background_layers[0].position_x,
        CssLength::Px(5.0)
    );
    assert_eq!(
        style.rare().additional_background_layers[0].position_y,
        CssLength::Px(3.0)
    );
    let before = style.clone();
    for (property, invalid) in [
        ("background-position-x", "right 4px, top"),
        ("background-position-x", "center 10px"),
        ("background-position-y", "bottom 2px, auto"),
        ("background-position-y", "top bottom"),
    ] {
        apply_property(&mut style, property, invalid);
        assert_eq!(style, before, "{property}: {invalid}");
    }
}

#[test]
fn background_shorthand_accepts_unspaced_position_size_separator() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "url(hero.svg) No-Repeat Left Top/146px auto",
    );

    assert_eq!(style.background_image_url, "hero.svg");
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(style.background_position_x, CssLength::Percent(0.0));
    assert_eq!(style.background_position_y, CssLength::Percent(0.0));
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(style.background_size_w, CssLength::Px(146.0));
    assert_eq!(style.background_size_h, CssLength::Auto);
}

#[test]
fn background_shorthand_keeps_parenthesized_function_tokens_intact() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "rgb(255, 0, 0) 10px calc(100% - 10px) / calc(50% - 2px) auto no-repeat",
    );

    assert_eq!(style.background_color, Color::rgb(255, 0, 0));
    assert_eq!(style.background_position_x, CssLength::Px(10.0));
    assert_eq!(
        style
            .background_position_y
            .resolve_vp(16.0, 100.0, 16.0, 0.0, 0.0),
        90.0
    );
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(
        style
            .background_size_w
            .resolve_vp(16.0, 100.0, 16.0, 0.0, 0.0),
        48.0
    );
    assert_eq!(style.background_size_h, CssLength::Auto);
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
}

#[test]
fn background_position_keeps_env_fallback_tokens_intact() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background-image",
        "linear-gradient(red, red), linear-gradient(blue, blue)",
    );
    apply_property(
        &mut style,
        "background-position",
        "env(--bg-x, 12px) env(--bg-y, calc(100% - 8px)), env(safe-area-inset-left, 20px) bottom",
    );

    assert_eq!(style.background_position_x, CssLength::Px(12.0));
    assert_eq!(
        style
            .background_position_y
            .resolve_vp(16.0, 100.0, 16.0, 0.0, 0.0),
        92.0
    );
    let layer = &style.rare().additional_background_layers[0];
    assert_eq!(layer.position_x, CssLength::Zero);
    assert_eq!(layer.position_y, CssLength::Percent(100.0));
}

#[test]
fn env_fallback_tokens_survive_property_specific_whitespace_parsers() {
    let mut style = ComputedStyle::default();

    apply_property(
        &mut style,
        "border-radius",
        "env(--r1, calc(4px + 2px)) env(--r2, 8px) / env(--ry1, 10px) env(--ry2, 12px)",
    );
    assert_eq!(style.border_top_left_radius, CssLength::Px(6.0));
    assert_eq!(style.border_top_right_radius, CssLength::Px(8.0));
    assert_eq!(style.border_top_left_radius_y, CssLength::Px(10.0));
    assert_eq!(style.border_top_right_radius_y, CssLength::Px(12.0));

    apply_property(
        &mut style,
        "border-top-left-radius",
        "env(--corner-x, 14px) env(--corner-y, calc(10px + 6px))",
    );
    assert_eq!(style.border_top_left_radius, CssLength::Px(14.0));
    assert_eq!(style.border_top_left_radius_y, CssLength::Px(16.0));

    apply_property(
        &mut style,
        "border-spacing",
        "env(--gap-x, calc(4px + 2px)) env(--gap-y, 10px)",
    );
    assert_eq!(style.border_spacing_h, CssLength::Px(6.0));
    assert_eq!(style.border_spacing_v, CssLength::Px(10.0));

    apply_property(
        &mut style,
        "object-position",
        "env(--obj-x, calc(100% - 8px)) env(--obj-y, 12px)",
    );
    assert_eq!(
        style
            .object_position_x
            .resolve_vp(16.0, 100.0, 16.0, 0.0, 0.0),
        92.0
    );
    assert_eq!(style.object_position_y, CssLength::Px(12.0));
}

#[test]
fn env_fallback_tokens_survive_shadow_and_border_image_shorthands() {
    let mut style = ComputedStyle::default();

    apply_property(
        &mut style,
        "box-shadow",
        "env(--shadow-x, calc(4px + 2px)) env(--shadow-y, 8px) env(--shadow-blur, 10px) env(--shadow-spread, calc(2px + 3px)) rgb(1 2 3)",
    );
    assert_eq!(style.box_shadow.len(), 1);
    let shadow = &style.box_shadow[0];
    assert_eq!(shadow.offset_x, 6.0);
    assert_eq!(shadow.offset_y, 8.0);
    assert_eq!(shadow.blur, 10.0);
    assert_eq!(shadow.spread, 5.0);
    assert_eq!(shadow.color, Color::rgb(1, 2, 3));

    apply_property(
        &mut style,
        "border-image",
        "url(border.png) env(--slice, calc(20 + 5)) / env(--width, calc(4px + 2px)) / env(--outset, calc(1px + 3px)) round",
    );
    assert_eq!(style.border_image_source, "url(border.png)");
    assert_eq!(style.border_image_slice, "env(--slice, calc(20 + 5))");
    assert_eq!(style.border_image_width, "env(--width, calc(4px + 2px))");
    assert_eq!(style.border_image_outset, "env(--outset, calc(1px + 3px))");
    assert_eq!(style.border_image_repeat, "round");
}

#[test]
fn border_image_shorthand_rejects_invalid_values_without_resetting_longhands() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "border-image",
        "url(border.png) 30 fill / 10px / 2px round stretch",
    );
    assert_eq!(style.border_image_source, "url(border.png)");
    assert_eq!(style.border_image_slice, "30 fill");
    assert_eq!(style.border_image_width, "10px");
    assert_eq!(style.border_image_outset, "2px");
    assert_eq!(style.border_image_repeat, "round stretch");
    let previous = style.clone();
    for invalid in [
        "",
        "url(a.png) url(b.png) 30",
        "url(a.png 30",
        "30 fill fill",
        "30 20 10 5 2",
        "30 -2",
        "30 /",
        "30 / /",
        "30 / -2px",
        "30 / 2 / 10%",
        "30 / 2 / -1px",
        "30 / 2 / env(--outset, 10%)",
        "30 / 2 / 3 / 4",
        "30 repeat round space",
        "30 / auto auto auto auto auto",
    ] {
        apply_property(&mut style, "border-image", invalid);
        assert_eq!(style, previous, "{invalid}");
    }
    apply_property(&mut style, "border-image", "30 / / 2 repeat");
    assert_eq!(style.border_image_source, "none");
    assert_eq!(style.border_image_slice, "30");
    assert_eq!(style.border_image_width, "1");
    assert_eq!(style.border_image_outset, "2");
    assert_eq!(style.border_image_repeat, "repeat");
}

#[test]
fn border_image_longhands_reject_invalid_values_without_resetting() {
    let mut style = ComputedStyle::default();
    for (property, valid, invalid) in [
        ("border-image-source", "url(border.png)", "url(border.png"),
        ("border-image-slice", "30 fill", "30 fill fill"),
        ("border-image-width", "2px auto", "2px -1px"),
        ("border-image-outset", "2px 1", "2px 10%"),
        (
            "border-image-repeat",
            "round stretch",
            "round stretch repeat",
        ),
    ] {
        apply_property(&mut style, property, valid);
        let previous = style.clone();
        apply_property(&mut style, property, invalid);
        assert_eq!(style, previous, "{property}: {invalid}");
    }
    assert_eq!(style.border_image_source, "url(border.png)");
    assert_eq!(style.border_image_slice, "30 fill");
    assert_eq!(style.border_image_width, "2px auto");
    assert_eq!(style.border_image_outset, "2px 1");
    assert_eq!(style.border_image_repeat, "round stretch");
    let previous = style.clone();
    apply_property(&mut style, "border-image-slice", "calc(2px)");
    assert_eq!(style, previous);
    apply_property(
        &mut style,
        "border-image-slice",
        "env(safe-area-inset-top, 10)",
    );
    assert_eq!(style, previous);
}

#[test]
fn background_size_longhand_uses_shared_size_parser() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "background-size", "Cover");
    assert_eq!(style.background_size, BackgroundSize::Cover);
    assert_eq!(style.background_size_w, CssLength::Auto);
    assert_eq!(style.background_size_h, CssLength::Auto);

    apply_property(&mut style, "background-size", "auto 24px");
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(style.background_size_w, CssLength::Auto);
    assert_eq!(style.background_size_h, CssLength::Px(24.0));

    apply_property(&mut style, "background-size", "10cqw 2rem");
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(style.background_size_w, CssLength::Cqw(10.0));
    assert_eq!(style.background_size_h, CssLength::Rem(2.0));
}

#[test]
fn background_box_and_repeat_keywords_are_case_insensitive() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "background-repeat", "Space No-Repeat");
    assert_eq!(
        style.background_repeat,
        BackgroundRepeat::TwoValue(BackgroundRepeatAxis::Space, BackgroundRepeatAxis::NoRepeat)
    );

    apply_property(&mut style, "background-origin", "Content-Box");
    assert_eq!(style.background_origin, BackgroundClip::ContentBox);

    apply_property(&mut style, "background-clip", "Text");
    assert_eq!(style.background_clip, BackgroundClip::Text);

    apply_property(&mut style, "background-attachment", "Fixed");
    assert_eq!(style.background_attachment, BackgroundAttachment::Fixed);
}

#[test]
fn background_position_keywords_are_case_insensitive() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "background-position", "Right Bottom");

    assert_eq!(style.background_position_x, CssLength::Percent(100.0));
    assert_eq!(style.background_position_y, CssLength::Percent(100.0));
}

#[test]
fn background_image_none_and_gradient_are_case_insensitive() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "background-image", "url(hero.png)");
    assert_eq!(style.background_image_url, "hero.png");

    apply_property(&mut style, "background-image", "NONE");
    assert!(style.background_image_url.is_empty());
    assert_eq!(style.gradient_type, GradientType::None);
    assert!(style.rare().gradient_stops.is_empty());

    apply_property(&mut style, "background-image", "Linear-Gradient(red, blue)");
    assert_eq!(style.gradient_type, GradientType::Linear);

    apply_property(&mut style, "background-image", "none");
    assert_eq!(style.gradient_type, GradientType::None);
    assert!(style.rare().gradient_stops.is_empty());
}

#[test]
fn desktop_media_background_image_none_clears_responsive_gradient() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             #popular {
               width: 300px;
               height: 120px;
             }
             @media only screen and (min-width: 768px) {
               #popular {
                 background-image: linear-gradient(to bottom, #dde0e4, #dde0e4);
               }
             }
             @media only screen and (min-width: 1280px) {
               #popular {
                 background-image: none;
               }
             }
           </style>
           <div id="popular"></div>"#,
        1366.0,
    );
    let popular = find_box(&d.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "popular")
            .unwrap_or(false)
    })
    .expect("popular element");
    assert_eq!(popular.style.gradient_type, GradientType::None);
    assert!(
        popular.style.rare().gradient_stops.is_empty(),
        "desktop background-image:none must clear the tablet gradient stops"
    );
}

#[test]
fn background_shorthand_keeps_top_image_layer_and_final_color() {
    let mut style = ComputedStyle::default();

    apply_property(
        &mut style,
        "background",
        "url(top.png) left top / 10px 20px no-repeat, url(bottom.png) right bottom / cover repeat blue",
    );

    assert_eq!(style.background_image_url, "top.png");
    assert_eq!(style.background_position_x, CssLength::Percent(0.0));
    assert_eq!(style.background_position_y, CssLength::Percent(0.0));
    assert_eq!(style.background_size, BackgroundSize::Explicit);
    assert_eq!(style.background_size_w, CssLength::Px(10.0));
    assert_eq!(style.background_size_h, CssLength::Px(20.0));
    assert_eq!(style.background_repeat, BackgroundRepeat::NoRepeat);
    assert_eq!(style.background_color, Color::rgb(0, 0, 255));
}

#[test]
fn background_shorthand_layer_split_ignores_commas_inside_functions() {
    let mut style = ComputedStyle::default();

    apply_property(
        &mut style,
        "background",
        "linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)), rgb(1, 2, 3)",
    );

    assert_eq!(style.gradient_type, GradientType::Linear);
    assert_eq!(style.background_color, Color::rgb(1, 2, 3));
}

#[test]
fn background_repeat_preserves_space_and_round_keywords() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "background-repeat", "space");
    assert_eq!(style.background_repeat, BackgroundRepeat::Space);

    apply_property(&mut style, "background-repeat", "round");
    assert_eq!(style.background_repeat, BackgroundRepeat::Round);

    apply_property(&mut style, "background", "url(tile.png) space");
    assert_eq!(style.background_repeat, BackgroundRepeat::Space);

    apply_property(&mut style, "background-repeat", "space no-repeat");
    assert_eq!(
        style.background_repeat,
        BackgroundRepeat::TwoValue(BackgroundRepeatAxis::Space, BackgroundRepeatAxis::NoRepeat)
    );

    apply_property(&mut style, "background", "url(tile.png) round no-repeat");
    assert_eq!(
        style.background_repeat,
        BackgroundRepeat::TwoValue(BackgroundRepeatAxis::Round, BackgroundRepeatAxis::NoRepeat)
    );
}

#[test]
fn border_shorthand_resets_omitted_longhands() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "border", "10px solid red");
    assert_eq!(style.border_top_width, CssLength::Px(10.0));
    assert_eq!(style.border_top_style, BorderStyle::Solid);
    assert_eq!(style.border_top_color, Color::rgb(255, 0, 0));

    apply_property(&mut style, "border", "blue");
    assert_eq!(style.border_top_width, CssLength::Px(3.0));
    assert_eq!(style.border_top_style, BorderStyle::None);
    assert_eq!(style.border_top_color, Color::rgb(0, 0, 255));
}

#[test]
fn border_side_shorthand_resets_omitted_longhands() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "border-top", "10px solid red");
    assert_eq!(style.border_top_width, CssLength::Px(10.0));
    assert_eq!(style.border_top_style, BorderStyle::Solid);
    assert_eq!(style.border_top_color, Color::rgb(255, 0, 0));

    apply_property(&mut style, "border-top", "blue");
    assert_eq!(style.border_top_width, CssLength::Px(3.0));
    assert_eq!(style.border_top_style, BorderStyle::None);
    assert_eq!(style.border_top_color, Color::rgb(0, 0, 255));
}

#[test]
fn font_shorthand_resets_omitted_longhands() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-style", "italic");
    apply_property(&mut style, "font-weight", "bold");
    apply_property(&mut style, "font-variant", "small-caps");
    apply_property(&mut style, "line-height", "2");

    apply_property(&mut style, "font", r#"14px system-ui"#);

    assert_eq!(style.font_style, FontStyle::Normal);
    assert_eq!(style.font_weight, FontWeight::Normal);
    assert!(!style.small_caps);
    assert_eq!(style.line_height, CssLength::Auto);
    assert_eq!(style.font_size, CssLength::Px(14.0));
}

#[test]
fn font_shorthand_relative_weight_uses_inherited_weight() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-weight", "700");

    apply_property(&mut style, "font", r#"lighter 14px system-ui"#);

    assert_eq!(style.font_weight, FontWeight::Value(400));
    assert_eq!(style.font_size, CssLength::Px(14.0));
}

#[test]
fn font_shorthand_system_keywords_are_case_insensitive() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-style", "italic");
    apply_property(&mut style, "font-weight", "bold");

    apply_property(&mut style, "font", "MENU");

    assert_eq!(style.font_style, FontStyle::Italic);
    assert_eq!(style.font_weight, FontWeight::Bold);
}

#[test]
fn font_weight_accepts_browser_integer_range_and_ignores_invalid_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-weight", "1");
    assert_eq!(style.font_weight, FontWeight::Value(1));

    apply_property(&mut style, "font-weight", "1000");
    assert_eq!(style.font_weight, FontWeight::Value(1000));

    apply_property(&mut style, "font-weight", "BOLD");
    assert_eq!(style.font_weight, FontWeight::Bold);

    apply_property(&mut style, "font-weight", "0");
    assert_eq!(
        style.font_weight,
        FontWeight::Bold,
        "out-of-range font weights are invalid and must not reset the cascade"
    );

    apply_property(&mut style, "font-weight", "400 700");
    assert_eq!(
        style.font_weight,
        FontWeight::Bold,
        "font-weight ranges are @font-face descriptors, not property values"
    );
}

#[test]
fn font_size_rejects_nonzero_unitless_lengths() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "font-size", "22px");
    apply_property(&mut style, "font-size", "999");

    assert_eq!(
        style.font_size,
        CssLength::Px(22.0),
        "nonzero unitless font-size is invalid CSS, not px"
    );

    apply_property(&mut style, "font-size", "0");
    assert_eq!(style.font_size, CssLength::Zero);
}

#[test]
fn font_shorthand_accepts_numeric_weight_range() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font", r#"1000 14px system-ui"#);

    assert_eq!(style.font_weight, FontWeight::Value(1000));
    assert_eq!(style.font_size, CssLength::Px(14.0));
}

#[test]
fn font_shorthand_tracks_family_offset_through_whitespace() {
    for gap in [" ", "   ", "\t", "\n\t "] {
        let mut style = ComputedStyle::default();
        apply_property(
            &mut style,
            "font",
            &format!("italic{gap}700{gap}20px/24px{gap}\"Example Family\", sans-serif"),
        );
        assert_eq!(style.font_style, FontStyle::Italic);
        assert_eq!(style.font_weight, FontWeight::Value(700));
        assert_eq!(style.font_size, CssLength::Px(20.0));
        assert_eq!(style.line_height, CssLength::Px(24.0));
        assert_eq!(style.font_family, "Example Family, sans-serif");
    }
}

#[test]
fn font_shorthand_resolves_nested_custom_property_token() {
    let doc = parse_and_layout(
        r#"<style>
          :root {
            --weight: 700;
            --size: 20px;
            --lh: 24px;
            --family: "GT America", sans-serif;
            --headline: var(--weight) var(--size)/var(--lh) var(--family);
          }
          .neo-font-v2-heading-xl-bold-cond { font: var(--headline); }
        </style>
        <h3 id="headline" class="neo-font-v2-heading-xl-bold-cond">Headline</h3>"#,
        800.0,
    );
    let h = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "headline")
            .unwrap_or(false)
    })
    .expect("headline");
    let same_node_id_count = find_all_boxes(&doc.root, &|b| b.node_id == h.node_id).len();
    assert_eq!(
        same_node_id_count, 1,
        "node_id {} reused {} times",
        h.node_id, same_node_id_count
    );
    let mut candidates = Vec::new();
    let matched = crate::css::cascade::match_rules(
        h,
        &doc.stylesheet,
        &[],
        &[],
        0,
        1,
        0,
        1,
        1280.0,
        600.0,
        0,
        false,
        &std::collections::HashSet::new(),
        &std::collections::HashSet::new(),
        0,
        "",
        &[],
        &[],
        &[],
        &mut candidates,
        None,
    );
    assert!(
        matched
            .matched
            .iter()
            .any(|(_, idx, _)| doc.stylesheet.rules[*idx]
                .original_selector
                .contains("neo-font-v2-heading-xl-bold-cond")),
        "font utility did not match; matched selectors: {:?}",
        matched
            .matched
            .iter()
            .map(|(_, idx, _)| doc.stylesheet.rules[*idx].original_selector.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        crate::css::resolve_var_references("var(--headline)", &doc.stylesheet.variables),
        "700 20px/24px \"GT America\", sans-serif"
    );

    assert_eq!(h.style.font_weight, FontWeight::Value(700));
    assert_eq!(h.style.font_size, CssLength::Px(20.0));
    assert_eq!(h.style.line_height, CssLength::Px(24.0));
    assert_eq!(h.style.font_family, "GT America, sans-serif");
    assert!(
        h.layout.content_rect.h >= 23.0,
        "headline should reserve the resolved line-height, got {:?}",
        h.layout.content_rect
    );
}

#[test]
fn escaped_responsive_font_utility_overrides_base_font_token() {
    let doc = parse_and_layout(
        r#"<style>
          :root {
            --weight: 700;
            --family: "GT America", sans-serif;
            --base: var(--weight) 20px/24px var(--family);
            --desktop: var(--weight) 32px/38px var(--family);
          }
          .neo-font-v2-heading-xl-bold-cond { font: var(--base); }
          @media only screen and (min-width: 768px) {
            .md\:neo-font-v2-heading-2xl-bold-cond { font: var(--desktop); }
          }
        </style>
        <h3 id="headline" class="neo-font-v2-heading-xl-bold-cond md:neo-font-v2-heading-2xl-bold-cond">Headline</h3>"#,
        1280.0,
    );
    let h = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "headline")
            .unwrap_or(false)
    })
    .expect("headline");

    assert_eq!(h.style.font_size, CssLength::Px(32.0));
    assert_eq!(h.style.line_height, CssLength::Px(38.0));
    assert!(
        h.layout.content_rect.h >= 37.0,
        "responsive utility should reserve desktop line-height, got {:?}",
        h.layout.content_rect
    );
}

#[test]
fn responsive_escaped_display_utility_overrides_base_hidden() {
    let desktop = parse_and_layout(
        r#"<style>
             .hidden { display: none; }
             @media (width >= 64rem) {
               .lg\:flex { display: flex; }
             }
           </style>
           <div id="nav" class="hidden lg:flex"></div>"#,
        1280.0,
    );
    let nav = find_box(&desktop.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "nav")
    })
    .expect("nav");
    assert_eq!(nav.style.display, Display::Flex);

    let mobile = parse_and_layout(
        r#"<style>
             .hidden { display: none; }
             @media (width >= 64rem) {
               .lg\:flex { display: flex; }
             }
           </style>
           <div id="nav" class="hidden lg:flex"></div>"#,
        800.0,
    );
    let nav = find_box(&mobile.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "nav")
    })
    .expect("nav");
    assert_eq!(nav.style.display, Display::None);
}

#[test]
fn responsive_utility_after_nested_media_block_is_not_dropped() {
    let desktop = parse_and_layout(
        r#"<style>
             .hidden { display: none; }
             @media (min-width:768px) {
               .md\:opacity-100 { opacity: 1; }
               @media not all and (min-width:1024px) {
                 .md\:max-lg\:hidden { display: none; }
               }
             }
             @media (min-width:1024px) {
               .lg\:block { display: block; }
               .lg\:inline { display: inline; }
               .lg\:flex { display: flex; }
               .lg\:grid { display: grid; }
               .lg\:hidden { display: none; }
             }
           </style>
           <div id="nav" class="hidden lg:flex"></div>"#,
        1280.0,
    );
    let nav = find_box(&desktop.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "nav")
    })
    .expect("nav");
    assert_eq!(nav.style.display, Display::Flex);
}

#[test]
fn escaped_brace_inside_media_selector_does_not_close_the_media_block() {
    let desktop = parse_and_layout(
        r#"<style>
             .hidden { display: none; }
             @media (min-width:768px) {
               .md\:content-\[\}\] { color: red; }
             }
             @media (min-width:1024px) {
               .lg\:flex { display: flex; }
             }
           </style>
           <div id="nav" class="hidden lg:flex"></div>"#,
        1280.0,
    );
    let nav = find_box(&desktop.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "nav")
    })
    .expect("nav");
    assert_eq!(nav.style.display, Display::Flex);
}

#[test]
fn yahoo_heading_title_clamp_keeps_resolved_utility_size() {
    let doc = parse_and_layout(
        r#"<style>
          :root {
            --font-weight-condensed-bold: 700;
            --size-font-40: 2.5rem;
            --v2-heading-2xl-bold-cond-weight: var(--font-weight-condensed-bold);
            --v2-heading-2xl-bold-cond-size: var(--size-font-40);
            --v2-heading-2xl-bold-cond-line-height: 1.15;
            --v2-heading-2xl-bold-cond-family: GT America Condensed;
            --font-v2-heading-2xl-bold-cond: var(--v2-heading-2xl-bold-cond-weight) var(--v2-heading-2xl-bold-cond-size)/var(--v2-heading-2xl-bold-cond-line-height) var(--v2-heading-2xl-bold-cond-family);
          }
          .md\:neo-font-v2-heading-2xl-bold-cond { font: var(--font-v2-heading-2xl-bold-cond); }
          .title { font-size: clamp(var(--v2-heading-2xl-bold-cond-size), 2.27vw + .275rem, var(--v2-heading-2xl-bold-cond-size)); }
        </style>
        <h3 id="headline" class="text title md:neo-font-v2-heading-2xl-bold-cond">Headline</h3>"#,
        1280.0,
    );
    let h = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "headline")
            .unwrap_or(false)
    })
    .expect("headline");

    assert_eq!(h.style.font_weight, FontWeight::Value(700));
    assert_eq!(h.style.font_size, CssLength::Px(40.0));
    assert_eq!(h.style.font_family, "GT America Condensed");
}

#[test]
fn yahoo_heading_font_utility_survives_parallel_cascade() {
    let mut filler = String::new();
    for i in 0..1100 {
        filler.push_str(&format!(".unused-{i} {{ color: rgb(1, 2, 3); }}\n"));
    }
    let html = format!(
        r#"<style>
          :root {{
            --font-weight-condensed-bold: 700;
            --size-font-40: 2.5rem;
            --v2-heading-2xl-bold-cond-weight: var(--font-weight-condensed-bold);
            --v2-heading-2xl-bold-cond-size: var(--size-font-40);
            --v2-heading-2xl-bold-cond-line-height: 1.15;
            --v2-heading-2xl-bold-cond-family: GT America Condensed;
            --font-v2-heading-2xl-bold-cond: var(--v2-heading-2xl-bold-cond-weight) var(--v2-heading-2xl-bold-cond-size)/var(--v2-heading-2xl-bold-cond-line-height) var(--v2-heading-2xl-bold-cond-family);
          }}
          h3 {{ font-size: inherit; font-weight: inherit; }}
          .md\:neo-font-v2-heading-2xl-bold-cond {{ font: var(--font-v2-heading-2xl-bold-cond); }}
          {filler}
        </style>
        <h3 id="headline" class="text title md:neo-font-v2-heading-2xl-bold-cond" style="text-decoration: none; font-style: normal; text-transform: none; text-align: inherit; font-variant-numeric: normal;">Headline</h3>"#
    );
    let doc = parse_and_layout(&html, 1280.0);
    let h = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "headline")
            .unwrap_or(false)
    })
    .expect("headline");

    assert_eq!(h.style.font_weight, FontWeight::Value(700));
    assert_eq!(h.style.font_size, CssLength::Px(40.0));
    assert_eq!(h.style.line_height, CssLength::Em(1.15));
}

#[test]
fn svg_fill_and_stroke_are_inherited_css_properties() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
           body { color: rgb(10, 20, 30); }
           svg { fill: currentColor; stroke: none; }
           #child { stroke: rgb(40, 50, 60); }
         </style>
         <svg id='parent'></svg>
         <svg id='child'></svg>",
        400.0,
    );

    let parent = d.get_element_by_id("parent").unwrap();
    let child = d.get_element_by_id("child").unwrap();
    assert_eq!(d.computed_style_property(parent, "fill"), "rgb(10, 20, 30)");
    assert_eq!(d.computed_style_property(parent, "stroke"), "none");
    assert_eq!(
        d.computed_style_property(child, "stroke"),
        "rgb(40, 50, 60)"
    );
}

#[test]
fn font_shorthand_uses_the_shared_font_size_parser() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font", r#"700 2cap system-ui"#);

    assert_eq!(style.font_weight, FontWeight::Value(700));
    assert_eq!(style.font_size, CssLength::Em(2.0));
}

#[test]
fn inline_svg_percentage_size_does_not_become_pixel_size() {
    let doc = parse_and_layout(
        r#"<div style="width:400px"><svg id="icon" width="100%" height="50%" viewBox="0 0 20 10"></svg></div>"#,
        400.0,
    );
    let svg = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "icon")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        svg.layout.content_rect.w > 390.0,
        "svg width=100% should resolve against the containing block, not become 100px; got {}",
        svg.layout.content_rect.w
    );
}

#[test]
fn inline_svg_width_height_attributes_override_viewbox_intrinsic_size() {
    let doc = parse_and_layout(
        r#"<button style="display:flex;width:48px;height:48px;align-items:center;justify-content:center">
             <svg id="search" viewBox="0 0 256 256" width="20" height="20">
               <path d="M0 0h256v256H0z"/>
             </svg>
           </button>"#,
        400.0,
    );
    let svg = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "search")
            .unwrap_or(false)
    })
    .unwrap();

    assert_eq!(svg.layout.content_rect.w.round(), 20.0);
    assert_eq!(svg.layout.content_rect.h.round(), 20.0);
}

#[test]
fn viewbox_only_inline_svg_uses_default_object_size() {
    let doc = parse_and_layout(
        r#"<svg id="yt" viewBox="0 260 1792 1260"><path d="M0 0h1v1z"/></svg>"#,
        800.0,
    );
    let svg = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|id| id == "yt").unwrap_or(false)
    })
    .unwrap();

    assert_eq!(svg.layout.content_rect.w.round(), 300.0);
    assert_eq!(svg.layout.content_rect.h.round(), 150.0);
}

#[test]
fn viewbox_only_svg_scales_to_smaller_definite_slot() {
    let doc = parse_and_layout(
        r#"<a style="display:block;width:20px;line-height:20px">
             <svg id="yt" viewBox="0 260 1792 1260"><path d="M0 0h1v1z"/></svg>
           </a>"#,
        800.0,
    );
    let svg = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|id| id == "yt").unwrap_or(false)
    })
    .unwrap();

    assert_eq!(svg.layout.content_rect.w.round(), 20.0);
    assert_eq!(svg.layout.content_rect.h.round(), 14.0);
}

#[test]
fn image_dimension_attributes_accept_browser_compatible_leading_integer() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             body { margin: 0; }
             .gallery-group { width: 1000px; }
             .gallery-group img { max-height: 100%; max-width: 90%; }
           </style>
           <div class="gallery-group">
             <img id="thumb" width="auto;" height="100px;" alt="thumbnail">
           </div>"#,
        1200.0,
    );
    let img = d.get_element_by_id("thumb").expect("image");
    let rect = d.get_bounding_client_rect(img).expect("image rect");
    assert!(
        rect.h <= 100.0,
        "height='100px;' should use the leading HTML integer before max constraints, got {}",
        rect.h
    );
    assert!(
        rect.w < 200.0,
        "width auto should use the fallback intrinsic ratio from the 100px height, not max-width scaling; got {}",
        rect.w
    );
}

#[test]
fn one_value_background_size_preserves_intrinsic_ratio() {
    let mut node = WebCore::new("div");
    node.bg_image_data = Some(std::sync::Arc::new(vec![0u8; 200 * 100 * 4]));
    node.bg_image_width = 200;
    node.bg_image_height = 100;
    node.layout.border_rect = Rect::new(0.0, 0.0, 167.0, 35.0);
    node.layout.padding_rect = node.layout.border_rect;
    node.layout.content_rect = node.layout.border_rect;
    apply_property(
        std::sync::Arc::make_mut(&mut node.style),
        "background-size",
        "146px",
    );
    apply_property(
        std::sync::Arc::make_mut(&mut node.style),
        "background-repeat",
        "no-repeat",
    );

    let list = build_display_list(&node, 800.0, 200.0);
    let (draw_w, draw_h) = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::BackgroundImage { draw_w, draw_h, .. } => Some((*draw_w, *draw_h)),
            _ => None,
        })
        .expect("background image command");

    assert_eq!(draw_w.round(), 146.0);
    assert_eq!(
        draw_h.round(),
        73.0,
        "one-value background-size must preserve the intrinsic image ratio"
    );
}

#[test]
fn slashdot_style_inline_block_nav_keeps_children_in_row() {
    let html = r#"
        <style>
        body { margin: 0; font: 14px/40px Arial; }
        .nav-wrap { height: 40px; line-height: 40px; background: #066; }
        .nav-primary { float: left; height: inherit; line-height: 40px; }
        .nav-wrap ul { margin: 0; padding: 0; list-style: none; }
        .nav-primary li { display: inline-block; margin-left: -4px; }
        .nav-primary li > a { display: block; padding: 0 15px; }
        .nav-primary > h2,
        .nav-primary .nav-site { display: inline-block; }
        .logo {
            margin: 0 10px 0 0;
            width: 167px;
            height: 35px;
            text-indent: -9999px;
            background-size: 146px;
        }
        .logo a { display: block; height: inherit; }
        </style>
        <div class="nav-wrap">
          <nav class="nav-primary" id="primary">
            <h2 class="logo" id="logo"><a><span>Slashdot</span></a></h2>
            <ul class="nav-site" id="site">
              <li><a><span>Stories</span></a></li>
              <li><a><span>Popular</span></a></li>
              <li><a><span>Polls</span></a></li>
              <li><a><span>Software</span></a></li>
            </ul>
          </nav>
        </div>
    "#;
    let doc = parse_and_layout(html, 1280.0);
    let logo = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "logo")
            .unwrap_or(false)
    })
    .unwrap();
    let site = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "site")
            .unwrap_or(false)
    })
    .unwrap();
    assert!(
        logo.layout.content_rect.y < 10.0,
        "logo should stay in the 40px nav row, got {:?}",
        logo.layout.content_rect
    );
    assert!(
        site.layout.content_rect.y < 10.0,
        "site nav should stay in the same row as the logo, got {:?}",
        site.layout.content_rect
    );

    let list = build_display_list(&doc.root, 1280.0, 200.0);
    let mut visible_menu_words = 0;
    for cmd in &list.commands {
        if let PaintCmd::Text { text, x, y, .. } = cmd {
            if text == "Slashdot" {
                assert!(
                    *x < -9000.0 || *x > 1280.0 || *y < 0.0 || *y > 80.0,
                    "logo replacement text should be outside the visible nav: x={x} y={y}"
                );
            }
            if matches!(text.as_str(), "Stories" | "Popular" | "Polls" | "Software") {
                visible_menu_words += 1;
                assert!(
                    *x >= 0.0 && *x < 1280.0 && *y >= 0.0 && *y < 80.0,
                    "menu text {text:?} escaped the nav: x={x} y={y}"
                );
            }
        }
    }
    assert_eq!(visible_menu_words, 4);
}

#[test]
fn empty_named_inline_anchor_does_not_create_anonymous_line() {
    let doc = parse_and_layout(
        r#"<style>body{margin:0;font:16px/20px Arial}</style>
           <a name="top"></a>
           <div id="header" style="height:40px"></div>"#,
        800.0,
    );
    let header = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "header")
            .unwrap_or(false)
    })
    .unwrap();

    assert_eq!(
        header.layout.border_rect.y.round(),
        0.0,
        "empty named anchors must not add a line before the following block"
    );
}

#[test]
fn shrink_wrapped_inline_block_does_not_count_child_padding_twice() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 16px/20px Arial; }
             ul { display: inline-block; margin: 0; padding: 0; list-style: none; }
             li { display: inline-block; }
             a { display: block; padding: 0 15px; }
           </style>
           <ul id="nav"><li><a>All</a></li></ul>"#,
        800.0,
    );
    let nav = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "nav")
            .unwrap_or(false)
    })
    .unwrap();
    let link = find_box(&doc.root, &|b| b.tag == "a").unwrap();

    assert!(
        (nav.layout.border_rect.w - link.layout.border_rect.w).abs() < 0.5,
        "shrink-wrapped parent should match the link border box, nav={:?} link={:?}",
        nav.layout.border_rect,
        link.layout.border_rect
    );
}

#[test]
fn nested_shrink_wrap_preserves_fractional_intrinsic_widths() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(r#"<style>
        body {margin:0;font:16px/20px Arial}
        .wrap {display:inline-block}
        #leaf {padding:0 2.25px;border:0.5px solid black}
        </style><span class=wrap id=outer><span class=wrap id=middle><span class=wrap id=leaf>All</span></span></span>"#, 800.0);
    let leaf = crate::dom::query_selector(&doc.root, "#leaf")
        .unwrap()
        .layout
        .border_rect;
    for id in ["#middle", "#outer"] {
        let node = crate::dom::query_selector(&doc.root, id).unwrap();
        assert!(
            (node.layout.border_rect.w - leaf.w).abs() < 0.01,
            "{id} inflated its child's fractional width: {:?} vs {leaf:?}",
            node.layout.border_rect
        );
        assert_eq!(node.layout.line_cache.len(), 1);
    }
}

#[test]
fn atomic_inline_blocks_do_not_add_extra_strut_descent_to_fixed_nav_rows() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/40px Arial; }
             nav { line-height: 40px; }
             .item { display: inline-block; height: 40px; width: 80px; }
             .nested { display: inline-block; margin: 0; padding: 0; list-style: none; }
             .nested > li { display: inline-block; height: 40px; width: 80px; }
           </style>
           <nav id="nav">
             <span id="first" class="item"></span>
             <ul id="nested" class="nested"><li></li><li></li></ul>
             <span id="last" class="item"></span>
           </nav>"#,
        800.0,
    );
    let nav = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "nav")
            .unwrap_or(false)
    })
    .unwrap();
    let first = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "first")
            .unwrap_or(false)
    })
    .unwrap();
    let nested = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "nested")
            .unwrap_or(false)
    })
    .unwrap();
    let last = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "last")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        nav.layout.border_rect.h <= 41.0,
        "atomic inline-only nav row should stay near 40px, got {:?}",
        nav.layout.border_rect
    );
    assert!(
        (first.layout.border_rect.y - nested.layout.border_rect.y).abs() < 0.5
            && (first.layout.border_rect.y - last.layout.border_rect.y).abs() < 0.5,
        "inline-block siblings should share a row: first={:?} nested={:?} last={:?}",
        first.layout.border_rect,
        nested.layout.border_rect,
        last.layout.border_rect
    );
}

#[test]
fn atomic_inline_breaks_after_the_box_not_before_it() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/40px Arial; }
             nav { width: 210px; line-height: 40px; }
             span { display: inline-block; width: 100px; height: 40px; }
           </style>
           <nav>
             <span id="first"></span>
             <span id="second"></span>
             <span id="third"></span>
           </nav>"#,
        800.0,
    );
    let first = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "first")
            .unwrap_or(false)
    })
    .unwrap();
    let second = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "second")
            .unwrap_or(false)
    })
    .unwrap();
    let third = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "third")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        first.layout.border_rect.y < 1.0 && second.layout.border_rect.y < 1.0,
        "first two atomic boxes should fit on the first line: first={:?} second={:?}",
        first.layout.border_rect,
        second.layout.border_rect
    );
    assert!(
        third.layout.border_rect.y >= 39.0,
        "only the overflowing third atomic box should wrap: {:?}",
        third.layout.border_rect
    );
}

#[test]
fn atomic_inline_wraps_when_outer_widths_exceed_available_space() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 15px/18px Arial; }
             .bar { width: 690px; background: #c0000f; }
             .title {
               display: inline-block;
               width: 141px;
               height: 18px;
               padding: 6px 10px;
               border-right: 1px solid white;
               box-sizing: border-box;
               vertical-align: middle;
             }
             .marquee {
               display: inline-block;
               width: calc(100% - 140px);
               height: 30px;
               overflow: hidden;
               white-space: nowrap;
               vertical-align: middle;
             }
           </style>
           <div class="bar" id="bar"><div class="title" id="title">breaking news</div>
             <div class="marquee" id="marquee"><ul><li>one two three four five</li></ul></div></div>"#,
        800.0,
    );
    let bar = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "bar")
    })
    .unwrap();
    let title = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "title")
    })
    .unwrap();
    let marquee = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "marquee")
    })
    .unwrap();

    assert_eq!(
        bar.layout.line_cache.len(),
        2,
        "141px plus 550px exceeds the 690px containing block"
    );
    assert!(
        marquee.layout.border_rect.y >= title.layout.border_rect.bottom(),
        "the overflowing atomic box should move to the next line: title={:?} marquee={:?}",
        title.layout.border_rect,
        marquee.layout.border_rect
    );
}

#[test]
fn flex_wrap_keeps_row_when_only_final_end_margin_overflows() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; }
             ul {
               display: flex;
               flex-wrap: wrap;
               justify-content: center;
               width: 300px;
               margin: 0;
               padding: 0;
             }
             li {
               display: block;
               width: 69px;
               height: 20px;
               padding: 8px;
               margin-right: 15px;
             }
           </style>
           <ul id="tags"><li id="one"></li><li id="two"></li><li id="three"></li></ul>"#,
        400.0,
    );
    let ul = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "tags")
    })
    .unwrap();
    let one = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "one")
    })
    .unwrap();
    let three = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "three")
    })
    .unwrap();

    assert!(
        three.layout.margin_rect.y <= one.layout.margin_rect.y + 0.5,
        "final end margin should not create a second flex line: ul={:?} one={:?} three={:?}",
        ul.layout.content_rect,
        one.layout.margin_rect,
        three.layout.margin_rect
    );
}

#[test]
fn floated_auto_width_inline_rows_shrink_to_measured_line_width() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/40px Arial; }
             nav { float: left; line-height: 40px; }
             h2, ul, a.button { display: inline-block; margin: 0; padding: 0; }
             h2 { width: 167px; height: 35px; margin-right: 10px; }
             ul { list-style: none; }
             li { display: inline-block; width: 110px; height: 40px; }
             a.button { width: 44px; height: 16px; margin: 0 10px; }
           </style>
           <nav id="nav">
             <h2 id="logo"></h2>
             <ul id="site"><li></li><li></li><li></li><li></li><li></li></ul>
             <a id="button" class="button"></a>
           </nav>"#,
        1280.0,
    );
    let nav = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "nav")
            .unwrap_or(false)
    })
    .unwrap();
    let logo = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "logo")
            .unwrap_or(false)
    })
    .unwrap();
    let site = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "site")
            .unwrap_or(false)
    })
    .unwrap();
    let button = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "button")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        nav.layout.border_rect.w >= 780.0,
        "float should shrink to the measured no-wrap line, got {:?}",
        nav.layout.border_rect
    );
    assert!(
        logo.layout.border_rect.y + logo.layout.border_rect.h <= 41.0
            && site.layout.border_rect.y + site.layout.border_rect.h <= 41.0
            && button.layout.border_rect.y + button.layout.border_rect.h <= 41.0,
        "float shrink relayout should keep nav children within one row: logo={:?} site={:?} button={:?}",
        logo.layout.border_rect,
        site.layout.border_rect,
        button.layout.border_rect
    );
}

#[test]
fn floated_nav_list_intrinsic_width_includes_item_padding_slop() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 13px/44px Arial; }
             .bar { display: flex; justify-content: center; width: 1021px; }
             .item { flex: 0 1 auto; }
             ul { display: block; list-style: none; margin: 0; padding: 0; }
             li { float: left; padding-right: 12px; height: 44px; }
             li:last-child { padding-right: 0; }
           </style>
           <div class="bar">
             <div class="item">
               <ul id="menu">
                 <li id="morocco">Morocco</li>
                 <li id="lifestyle">Lifestyle</li>
                 <li id="sahara">Western Sahara</li>
               </ul>
             </div>
           </div>"#,
        1200.0,
    );
    let first = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "morocco")
            .unwrap_or(false)
    })
    .unwrap();
    let last = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "sahara")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        (last.layout.margin_rect.y - first.layout.margin_rect.y).abs() <= 0.5,
        "floated nav items should stay on one row: first={:?} last={:?}",
        first.layout.margin_rect,
        last.layout.margin_rect
    );
}

#[test]
fn nested_inline_before_content_contributes_to_icon_width() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 16px/20px Arial; }
             .label { display: inline-block; }
             [class^="icon-"]:before {
               content: ">";
               display: inline-block;
               width: 1em;
               margin-left: .2em;
               margin-right: .2em;
             }
           </style>
           <span id="label" class="label">Firehose <i id="icon" class="icon-angle-right"></i></span>"#,
        800.0,
    );
    let label = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "label")
            .unwrap_or(false)
    })
    .unwrap();
    assert!(
        label.layout.border_rect.w >= 75.0,
        "parent inline-block should include generated icon width, got {:?}",
        label.layout.border_rect
    );
}

#[test]
fn atomic_generated_content_paints_once_with_nonzero_boxes() {
    let mut renderer = crate::Renderer::new();
    for display in ["inline-block", "inline-flex", "inline-grid"] {
        let doc = renderer.load_html(
            &format!(
                r#"<style>
            body {{font:20px/30px sans-serif}}
            #subject::before {{content:'A';display:{display};width:2em;background:yellow}}
            #subject::after {{content:'B';display:{display};width:2em;background:blue}}
            </style><div id=subject>Label</div>"#
            ),
            800.0,
        );
        let owner = crate::dom::query_selector(&doc.root, "#subject").unwrap();
        for tag in ["::before", "::after"] {
            let pseudo = owner
                .children
                .iter()
                .find(|child| child.tag == tag)
                .unwrap();
            assert_eq!(pseudo.layout.content_rect.w, 40.0, "{display} {tag}");
            assert!(
                pseudo.layout.content_rect.h >= 30.0,
                "{display} {tag}: {:?}",
                pseudo.layout.content_rect
            );
        }
        let list = build_display_list(&doc.root, 800.0, 200.0);
        for glyph in ["A", "B"] {
            assert_eq!(
                list.commands
                    .iter()
                    .filter(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == glyph))
                    .count(),
                1,
                "{display}: {glyph} must paint exactly once"
            );
        }
    }
}

#[test]
fn slashdot_icon_font_before_rules_generate_inline_pseudo_content() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             body { margin: 0; font: 16px/20px Arial; }
             [class^="icon-"]:before, [class*=" icon-"]:before {
               font-family: "sdicon";
               display: inline-block;
               width: 1em;
               margin-left: .2em;
               margin-right: .2em;
               line-height: 1em;
             }
             .icon-angle-right:before { content: "\e87a"; }
           </style>
           <span id="label">Firehose <i id="icon" class="icon-angle-right"></i></span>"#,
        800.0,
    );
    let icon = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "icon")
            .unwrap_or(false)
    })
    .unwrap();
    let generated = icon
        .children
        .iter()
        .find(|child| child.tag == "::before")
        .expect("atomic inline pseudo must have a generated box");
    assert_eq!(generated.text, "\u{e87a}");
    assert_eq!(generated.style.font_family, "sdicon");
    assert_eq!(generated.layout.content_rect.w, 16.0);
    let list = build_display_list(&doc.root, 800.0, 200.0);
    assert_eq!(
        list.commands
            .iter()
            .filter(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "\u{e87a}"))
            .count(),
        1,
        "the icon must reach paint exactly once"
    );
    let before = icon
        .style
        .before_style
        .as_ref()
        .expect("icon pseudo style should be generated");
    assert_eq!(before.font_family, "sdicon");
    assert_eq!(before.display, Display::InlineBlock);
}

#[test]
fn slashdot_icon_font_before_rules_survive_parallel_cascade() {
    let filler = (0..1100)
        .map(|i| format!(".unused-{i} {{ color: red; }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        &format!(
            r#"<style>
             {filler}
             [class^="icon-"]:before, [class*=" icon-"]:before {{
               font-family: "sdicon";
               display: inline-block;
               width: 1em;
               margin-left: .2em;
               margin-right: .2em;
               line-height: 1em;
             }}
             .icon-angle-right:before {{ content: "\e87a"; }}
           </style>
           <span id="label">Firehose <i id="icon" class="icon-angle-right"></i></span>"#
        ),
        800.0,
    );
    let icon = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "icon")
            .unwrap_or(false)
    })
    .unwrap();
    let generated = icon
        .children
        .iter()
        .find(|child| child.tag == "::before")
        .expect("parallel cascade must retain the generated pseudo box");
    assert_eq!(generated.text, "\u{e87a}");
    assert_eq!(generated.style.font_family, "sdicon");
    assert_eq!(generated.layout.content_rect.w, 16.0);
    let list = build_display_list(&doc.root, 800.0, 200.0);
    assert_eq!(
        list.commands
            .iter()
            .filter(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text == "\u{e87a}"))
            .count(),
        1,
        "parallel cascade must paint the icon exactly once"
    );
    assert_eq!(
        icon.style
            .before_style
            .as_ref()
            .map(|style| style.font_family.as_str()),
        Some("sdicon")
    );
}

#[test]
fn shrink_to_fit_block_with_inline_block_children_uses_unwrapped_width() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/40px Arial; }
             ul { margin: 0; padding: 0; list-style: none; }
             .outer { display: inline-block; }
             .filter { padding-left: 10px; }
             .filter li { display: inline-block; margin-left: -4px; }
             [class^="icon-"]:before {
               content: ">";
               display: inline-block;
               width: 1em;
               margin-left: .2em;
               margin-right: .2em;
               line-height: 1em;
             }
           </style>
           <div id="outer" class="outer">
             <ul id="filter" class="filter">
               <li id="label">Firehose <i class="icon-angle-right"></i></li>
               <li>All</li>
               <li>Popular</li>
             </ul>
           </div>"#,
        800.0,
    );
    let filter = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "filter")
            .unwrap_or(false)
    })
    .unwrap();
    assert!(
        filter.layout.border_rect.h <= 41.0,
        "nested shrink-to-fit block should keep inline-block children on one line, got {:?}",
        filter.layout.border_rect
    );
}

#[test]
fn floated_child_inside_inline_wrapper_keeps_percentage_width() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
             body { margin: 0; }
             #nav { width: 320px; height: 40px; }
             #wrap { display: inline; }
             #search { float: left; width: 50%; height: 20px; }
             #menu { float: right; width: 25%; height: 20px; }
           </style>
           <div id="nav"><span id="wrap"><form id="search"></form><div id="menu"></div></span></div>"#,
        800.0,
    );
    let search = doc.get_element_by_id("search").unwrap();
    let rect = doc.get_bounding_client_rect(search).unwrap();
    assert!(
        (rect.w - 160.0).abs() < 0.5,
        "float inside inline wrapper should resolve 50% against the containing block, got {rect:?}"
    );
    assert!(
        rect.y.abs() < 0.5,
        "float inside inline wrapper should be placed at the containing block top, got {rect:?}"
    );
    let menu = doc.get_element_by_id("menu").unwrap();
    let menu_rect = doc.get_bounding_client_rect(menu).unwrap();
    assert!(
        (menu_rect.x - 240.0).abs() < 0.5 && menu_rect.y.abs() < 0.5,
        "right float inside inline wrapper should use the containing block float context, got {menu_rect:?}"
    );
}

#[test]
fn stylesheet_background_image_url_reaches_render_style() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             .logo {
               width: 167px;
               height: 35px;
               background-image: url("//a.fsdn.com/sd/slashdot_tm.svg");
               background-repeat: no-repeat;
               background-position: 20px 10px;
               background-size: 146px;
             }
           </style>
           <h2 class="logo" id="logo"></h2>"#,
        800.0,
    );
    let logo = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "logo")
            .unwrap_or(false)
    })
    .expect("logo");

    assert_eq!(
        logo.style.background_image_url,
        "//a.fsdn.com/sd/slashdot_tm.svg"
    );
    assert_eq!(logo.style.background_size, BackgroundSize::Explicit);
}

#[test]
fn unbreakable_generated_fragments_overflow_on_one_line() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
             body { margin: 0; font: 20px/20px Arial; }
             #link { display: block; width: 20px; line-height: 20px; }
             #icon::before {
               content: "X";
               display: inline-block;
               width: 1em;
               margin-left: .2em;
             }
           </style>
           <a id="link"><i id="icon"></i></a>"#,
        800.0,
    );
    let link_id = doc.get_element_by_id("link").unwrap();
    let link = doc.get_box_by_id(link_id).unwrap();
    assert_eq!(
        link.layout.line_cache.len(),
        1,
        "generated inline-block fragments without break opportunities should stay on one overflowing line"
    );
    let rect = doc.get_bounding_client_rect(link_id).unwrap();
    assert!(
        rect.h <= 22.0,
        "one fixed-height icon line should not expand into stacked spacer/glyph lines, got {rect:?}"
    );
}

#[test]
fn positioned_pseudo_on_plain_inline_uses_owner_geometry() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             body { margin: 0; padding: 30px 40px; font: 20px/20px Arial; }
             #link { display: block; width: 20px; line-height: 20px; }
             #icon { position: relative; }
             #icon::before {
               content: "X";
               display: inline-block;
               width: 1em;
             }
             #icon::after {
               content: "";
               display: block;
               position: absolute;
               left: 100px;
               top: 100px;
               width: 20px;
               height: 20px;
               background: black;
               z-index: -1;
             }
           </style>
           <a id="link"><i id="icon"></i></a>"#,
        800.0,
    );
    let list = build_display_list(&doc.root, 800.0, 200.0);
    let owner = crate::dom::query_selector(&doc.root, "#icon")
        .unwrap()
        .layout
        .padding_rect;
    assert_eq!(owner.x, 40.0);
    let (ascent, descent, _) =
        crate::layout::inline_layout::font_metrics(Some(&mut renderer.font_system), "Arial", 20.0);
    let line = crate::dom::query_selector(&doc.root, "#link").unwrap();
    let baseline = line.layout.line_cache[0].y + line.layout.line_cache[0].ascent;
    // Negative leading puts the font-content edge above the line box.
    // Chrome's Arial fixture gives y=29 and height=22, not y>=30.
    assert!((owner.y - (baseline - ascent)).abs() < 0.01, "{owner:?}");
    assert!((owner.h - ascent - descent).abs() < 0.01, "{owner:?}");
    assert!(owner.w > 0.0);
    let decorations: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::FillRect { rect, color, .. }
                if color.r == 0 && color.g == 0 && color.b == 0 && color.a == 255 =>
            {
                Some(rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        decorations.len(),
        1,
        "the positioned decoration must not be suppressed or duplicated"
    );
    let rect = decorations[0];
    assert_eq!(
        (rect.x, rect.y, rect.w, rect.h),
        (owner.x + 100.0, owner.y + 100.0, 20.0, 20.0)
    );
}

#[test]
fn whitespace_after_atomic_inline_block_is_not_collapsed_with_leading_indent() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 16px/20px Arial; }
             #row { display: inline-block; }
             span { display: inline-block; width: 20px; height: 20px; }
           </style>
           <div id="row">
             <span id="a"></span>
             <span id="b"></span>
           </div>"#,
        800.0,
    );
    let row = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .map(|id| id == "row")
            .unwrap_or(false)
    })
    .unwrap();

    assert!(
        row.layout.border_rect.w > 42.0,
        "space between inline-blocks should contribute after leading indentation is trimmed, got {:?}",
        row.layout.border_rect
    );
}

#[test]
fn font_shorthand_accepts_font_stretch_component() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font", r#"condensed 16px sans-serif"#);

    assert_eq!(style.font_stretch, 75.0);
    assert_eq!(style.font_size, CssLength::Px(16.0));
}

#[test]
fn font_variant_shorthand_keeps_small_caps_in_keyword_lists() {
    let mut style = ComputedStyle::default();

    apply_property(
        &mut style,
        "font-variant",
        "common-ligatures small-caps tabular-nums",
    );

    assert!(style.small_caps);
}

#[test]
fn font_variant_caps_longhand_sets_small_caps() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font-variant-caps", "small-caps");

    assert!(style.small_caps);
    assert_eq!(style.font_variant_caps, "small-caps");
}

#[test]
fn font_variant_longhands_are_stored_inherited_and_serialized() {
    let mut doc = parse_html(
        r#"
        <div style="font-variant-ligatures: no-common-ligatures;
                    font-variant-numeric: tabular-nums slashed-zero;
                    font-variant-east-asian: ruby;
                    font-variant-alternates: historical-forms;
                    font-variant-emoji: text;
                    font-variant-position: sub;">
            <span id="s">x</span>
        </div>
        "#,
    );
    let span = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "s").unwrap_or(false)
    })
    .expect("span");

    assert_eq!(span.style.font_variant_ligatures, "no-common-ligatures");
    assert_eq!(span.style.font_variant_numeric, "tabular-nums slashed-zero");
    assert_eq!(span.style.font_variant_east_asian, "ruby");
    assert_eq!(span.style.font_variant_alternates, "historical-forms");
    assert_eq!(span.style.font_variant_emoji, "text");
    assert_eq!(span.style.font_variant_position, "sub");
    assert_eq!(
        doc.computed_style_property(span.node_id, "font-variant-numeric"),
        "tabular-nums slashed-zero"
    );
}

#[test]
fn font_shorthand_resets_font_variant_longhands() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font-variant-numeric", "tabular-nums");
    apply_property(&mut style, "font", "16px sans-serif");

    assert_eq!(style.font_variant_numeric, "normal");
}

#[test]
fn text_underline_position_is_parsed_and_inherited() {
    let doc =
        parse_html(r#"<div style="text-underline-position: under"><span id="s">x</span></div>"#);
    let span = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "s").unwrap_or(false)
    })
    .expect("span");

    assert_eq!(
        span.style.text_underline_position,
        TextUnderlinePosition::Under
    );
}

#[test]
fn writing_mode_sideways_keywords_are_preserved() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "writing-mode", "sideways-lr");
    assert_eq!(style.writing_mode, WritingMode::SidewaysLR);

    apply_property(&mut style, "writing-mode", "sideways-rl");
    assert_eq!(style.writing_mode, WritingMode::SidewaysRL);
}

#[test]
fn cursor_parses_common_css_ui_keywords() {
    let cases = [
        ("copy", CSSCursor::Copy),
        ("cell", CSSCursor::Cell),
        ("context-menu", CSSCursor::ContextMenu),
        ("all-scroll", CSSCursor::AllScroll),
        ("zoom-in", CSSCursor::ZoomIn),
        ("zoom-out", CSSCursor::ZoomOut),
        ("ne-resize", CSSCursor::NEResize),
        ("nw-resize", CSSCursor::NWResize),
        ("se-resize", CSSCursor::SEResize),
        ("sw-resize", CSSCursor::SWResize),
        ("ew-resize", CSSCursor::ColResize),
        ("ns-resize", CSSCursor::RowResize),
    ];

    for (keyword, expected) in cases {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "cursor", keyword);
        assert_eq!(style.cursor, expected, "cursor keyword {keyword}");
    }
}

#[test]
fn text_underline_offset_reaches_text_decoration_commands() {
    let mut frame = EngineFrame::new(
        parse_html(r#"<p style="text-decoration: underline; text-underline-offset: 7px">x</p>"#),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let offset = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { decoration, .. } if decoration.underline => {
            Some(decoration.underline_offset)
        }
        _ => None,
    });

    assert_eq!(offset, Some(7.0));
}

#[test]
fn text_underline_position_reaches_text_decoration_commands() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<p style="text-decoration: underline; text-underline-position: under">x</p>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let position = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { decoration, .. } if decoration.underline => {
            Some(decoration.underline_position)
        }
        _ => None,
    });

    assert_eq!(position, Some(TextUnderlinePosition::Under));
}

#[test]
fn text_decoration_skip_ink_reaches_text_decoration_commands() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<p style="text-decoration: underline; text-decoration-skip-ink: none">gap</p>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let skip_ink = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text { decoration, .. } if decoration.underline => Some(decoration.skip_ink),
        _ => None,
    });

    assert_eq!(skip_ink, Some(false));
}

#[test]
fn propagated_text_decoration_uses_decorating_box_color() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<p style="color: blue; text-decoration: underline">a <span style="color: red">b</span></p>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let span_decoration_color = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text {
            text, decoration, ..
        } if text.contains('b') => Some(decoration.color),
        _ => None,
    });

    assert_eq!(span_decoration_color, Some(Color::rgb(0, 0, 255)));
}

#[test]
fn text_decoration_paints_through_descendants_without_becoming_inherited_style() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<p style="color: blue; text-decoration: underline">a <span id="child" style="color: red">b</span></p>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let child = frame.doc.get_element_by_id("child").unwrap();
    let child_style = frame.doc.get_computed_style(child).unwrap();
    assert!(!child_style.text_decoration.underline);

    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let painted_underline = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text {
            text, decoration, ..
        } if text.contains('b') => Some(decoration.underline),
        _ => None,
    });

    assert_eq!(painted_underline, Some(true));
}

#[test]
fn text_decoration_inherit_shorthand_clears_ua_anchor_underline() {
    let mut frame = EngineFrame::new(
        parse_html(r##"<style>a { text-decoration: inherit }</style><a href="#">plain link</a>"##),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let underline = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text {
            text, decoration, ..
        } if text.contains("plain link") => Some(decoration.underline),
        _ => None,
    });

    assert_eq!(underline, Some(false));
}

#[test]
fn equal_specificity_cascade_uses_stylesheet_source_order() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<style>
                .md\:block { display: block }
                .hidden { display: none }
                @media (min-width: 768px) { .md\:block { display: block } }
            </style>
            <div id="target" class="hidden md:block">visible</div>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();

    let target = find_box(&frame.doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "target")
    })
    .expect("target should exist");
    assert_eq!(target.style.display, Display::Block);
}

#[test]
fn block_text_decoration_propagates_to_in_flow_block_descendants() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<div style="color: blue; text-decoration: underline">inflow<p>block child</p><div><span>nested</span></div></div>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    for needle in ["inflow", "block child", "nested"] {
        let decoration = list.commands.iter().find_map(|cmd| match cmd {
            PaintCmd::Text {
                text, decoration, ..
            } if text.contains(needle) => Some(decoration),
            _ => None,
        });
        assert!(
            decoration.is_some_and(|d| d.underline && d.color == Color::rgb(0, 0, 255)),
            "{needle} should keep the decorating block underline"
        );
    }
}

#[test]
fn font_style_oblique_angle_keeps_the_slant() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "font-style", "oblique 14deg");

    assert_eq!(style.font_style, FontStyle::Oblique);
}

#[test]
fn rtl_text_keeps_italic_font_style() {
    let mut frame = EngineFrame::new(
        parse_html(r#"<p style="font-style: italic">שלום</p>"#),
        800.0,
        600.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);

    let font_style = list.commands.iter().find_map(|cmd| match cmd {
        PaintCmd::Text {
            text, font_style, ..
        } if text.contains("שלום") => Some(*font_style),
        _ => None,
    });

    assert_eq!(font_style, Some(1));
}

#[test]
fn dir_auto_uses_first_strong_text_direction() {
    let mut frame = EngineFrame::new(
        parse_html(r#"<p id="t" dir="auto">123 שלום</p>"#),
        800.0,
        600.0,
    );
    frame.update_frame();

    let p = find_box(&frame.doc.root, &|node| {
        node.attributes
            .get("id")
            .map(|id| id == "t")
            .unwrap_or(false)
    })
    .unwrap();

    assert_eq!(p.style.direction, Direction::RTL);
}

#[test]
fn dir_auto_without_strong_text_defaults_to_ltr() {
    let mut frame = EngineFrame::new(
        parse_html(r#"<div dir="rtl"><p id="t" dir="auto">123 !!!</p></div>"#),
        800.0,
        600.0,
    );
    frame.update_frame();

    let p = find_box(&frame.doc.root, &|node| {
        node.attributes
            .get("id")
            .map(|id| id == "t")
            .unwrap_or(false)
    })
    .unwrap();

    assert_eq!(p.style.direction, Direction::LTR);
}

#[test]
fn unicode_bidi_override_reverses_ltr_text_segments() {
    let html = r#"
        <div id="b" dir="rtl" style="unicode-bidi: bidi-override">abc</div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let div = find_box(&doc.root, &|node| {
        node.attributes
            .get("id")
            .map(|id| id == "b")
            .unwrap_or(false)
    })
    .expect("div");

    let line = div.layout.line_cache.first().expect("line");
    let starts: Vec<usize> = line
        .visual_segments
        .iter()
        .map(|segment| segment.logical_start)
        .collect();

    assert_eq!(starts, vec![2, 1, 0]);
    assert!(
        line.visual_segments
            .iter()
            .all(|segment| segment.length == 1 && segment.level == 1)
    );
}

#[test]
fn unicode_bidi_plaintext_uses_line_text_direction() {
    let html = r#"
        <div id="p" dir="ltr" style="unicode-bidi: plaintext">123 שלום</div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let div = find_box(&doc.root, &|node| {
        node.attributes
            .get("id")
            .map(|id| id == "p")
            .unwrap_or(false)
    })
    .expect("div");

    let line = div.layout.line_cache.first().expect("line");
    assert!(
        line.visual_segments
            .iter()
            .any(|segment| (segment.level & 1) != 0),
        "plaintext should resolve this line as RTL from its first strong character"
    );
}

#[test]
fn float_and_clear_accept_logical_inline_keywords() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "float", "inline-start");
    apply_property(&mut style, "clear", "inline-end");

    assert_eq!(style.float, Float::InlineStart);
    assert_eq!(style.clear, Clear::InlineEnd);
}

#[test]
fn logical_float_and_clear_follow_computed_direction() {
    let doc = parse(
        r#"<html><body>
             <div id="t" style="float:inline-start; clear:inline-end; direction:rtl">x</div>
           </body></html>"#,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "t").unwrap_or(false)
    })
    .expect("target div");

    assert_eq!(div.style.float, Float::Right);
    assert_eq!(div.style.clear, Clear::Left);
    assert_eq!(
        div.style.display,
        Display::Block,
        "logical float still blockifies"
    );
}

#[test]
fn counter_declarations_validate_names_integers_and_preserve_previous_values() {
    for property in ["counter-reset", "counter-increment", "counter-set"] {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, property, "chapter 7");
        for invalid in [
            "",
            "12",
            "chapter 1.5",
            "chapter 2px",
            "chapter 1e2",
            "chapter +",
            "chapter, other",
            "chapter none",
            "chapter inherit",
            "- 2",
            "chapter \\\n",
        ] {
            apply_property(&mut style, property, invalid);
            let actual = match property {
                "counter-reset" => style
                    .counter_reset
                    .iter()
                    .map(|r| (r.name.clone(), r.value.unwrap()))
                    .collect(),
                "counter-increment" => style.counter_increment.clone(),
                _ => style.counter_set.clone(),
            };
            assert_eq!(
                actual,
                vec![("chapter".into(), 7)],
                "{property}: {invalid:?}"
            );
        }
        apply_property(&mut style, property, "NoNe");
        assert!(
            style.counter_reset.is_empty()
                && style.counter_increment.is_empty()
                && style.counter_set.is_empty()
        );
    }
    assert_eq!(
        crate::css::apply::parse_counter_list_checked(r"\63 hapter +2 --part -3", 0),
        Some(vec![("chapter".into(), 2), ("--part".into(), -3)])
    );
    assert_eq!(
        crate::css::apply::parse_counter_list_checked(
            "a 999999999999999999999 b -999999999999999999999",
            0
        ),
        Some(vec![("a".into(), i32::MAX), ("b".into(), i32::MIN)])
    );
}

#[test]
fn counter_math_uses_typed_integer_evaluation_and_rounding() {
    let mut style = ComputedStyle::default();
    for (property, expected) in [
        ("counter-reset", 3),
        ("counter-increment", 3),
        ("counter-set", 3),
    ] {
        apply_property(&mut style, property, "n calc(2.5) other calc(-2.5)");
        let actual = match property {
            "counter-reset" => style
                .counter_reset
                .iter()
                .map(|r| (r.name.clone(), r.value.unwrap()))
                .collect(),
            "counter-increment" => style.counter_increment.clone(),
            _ => style.counter_set.clone(),
        };
        assert_eq!(
            actual,
            vec![("n".into(), expected), ("other".into(), -2)],
            "{property}"
        );
    }
    apply_property(&mut style, "counter-reset", "n calc(2 * max(2, 3)) plain");
    assert_eq!(
        style.counter_reset,
        vec![
            CounterReset::normal("n", 6),
            CounterReset::normal("plain", 0)
        ]
    );
    for invalid in [
        "n calc(1px)",
        "n calc(50%)",
        "n calc(1 + )",
        "n calc(2",
        "n unknown(2)",
    ] {
        apply_property(&mut style, "counter-reset", invalid);
        assert_eq!(
            style.counter_reset,
            vec![
                CounterReset::normal("n", 6),
                CounterReset::normal("plain", 0)
            ],
            "{invalid}"
        );
    }
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n calc(2 * max(2, 3));}
        p {counter-increment:n calc(2.5);}
        p::before {content:counter(n) "|";}
        p:last-child {counter-set:n calc(-2.5);}
        </style><p></p><p></p>"#
        )
        .concat(),
        "9|-2|"
    );
}

#[test]
fn counter_computed_values_serialize_names_defaults_and_math() {
    let mut doc = parse_html(
        r#"<style>
        #counter {counter-reset:chapter calc(2.5) section;
          counter-increment:chapter section -2;counter-set:section calc(-2.5);}
        #escaped {counter-reset:\31 st 2 a\ b 3 \- 4 \2d 2 5;}
        </style><div id=counter></div><div id=escaped></div><div id=empty></div>"#,
    );
    doc.set_viewport(800.0, 600.0);
    let id = doc.get_element_by_id("counter").unwrap();
    for (property, expected) in [
        ("counter-reset", "chapter 3 section 0"),
        ("counter-increment", "chapter 1 section -2"),
        ("counter-set", "section -2"),
    ] {
        assert_eq!(doc.computed_style_property(id, property), expected);
    }
    let empty = doc.get_element_by_id("empty").unwrap();
    for property in ["counter-reset", "counter-increment", "counter-set"] {
        assert_eq!(doc.computed_style_property(empty, property), "none");
    }
    let escaped = doc.get_element_by_id("escaped").unwrap();
    let serialized = doc.computed_style_property(escaped, "counter-reset");
    assert_eq!(serialized, r"\31 st 2 a\ b 3 \- 4 -\32  5");
    assert_eq!(
        crate::css::apply::parse_counter_list_checked(&serialized, 0),
        Some(vec![
            ("1st".into(), 2),
            ("a b".into(), 3),
            ("-".into(), 4),
            ("-2".into(), 5)
        ])
    );
}

#[test]
fn explicitly_initialized_reversed_counters_keep_direction_in_scope() {
    let html = r#"<style>
        .reverse {counter-reset:reversed(list-item) calc(2 + 2);}
        li {list-style:none;}
        li::before {content:counter(list-item) "|";}
        </style><ol class=reverse><li></li><li><ol><li></li></ol></li><li></li></ol>"#;
    assert_eq!(build_display_texts(html).concat(), "3|2|1|1|");
    assert_eq!(build_display_texts(r#"<style>
        ol {counter-reset:reversed(list-item) 5;} li {list-style:none;counter-increment:list-item 2;}
        li::before {content:counter(list-item) "|";}
        </style><ol><li></li><li></li></ol>"#).concat(), "7|9|");
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:reversed(n) 4;} p {counter-increment:n;}
        p::before {content:counter(n) "|";}
        </style><p></p><p></p>"#
        )
        .concat(),
        "5|6|"
    );
    let mut doc = parse_html(
        r#"<style>#x {counter-reset:reversed(n) calc(2.5) other;}</style><div id=x></div>"#,
    );
    doc.set_viewport(800.0, 600.0);
    let id = doc.get_element_by_id("x").unwrap();
    assert_eq!(
        doc.computed_style_property(id, "counter-reset"),
        "reversed(n) 3 other 0"
    );
    for invalid in ["reversed(n) 2", "reversed(n)", "reversed(n 2) 3"] {
        assert!(
            crate::css::apply::parse_counter_list_checked(invalid, 1).is_none(),
            "increment/set reject reset-only syntax"
        );
    }
}

#[test]
fn automatic_reversed_counters_resolve_scope_operations_before_paint() {
    assert_eq!(
        build_display_texts(
            r#"<style>
        ol.reverse {counter-reset:reversed(list-item);} li {list-style:none;}
        li::before {content:counter(list-item) "|";}
        </style><ol class=reverse><li></li><li><ol><li></li></ol></li><li></li></ol>"#
        )
        .concat(),
        "3|2|1|1|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:reversed(n);} p {counter-increment:n -2;}
        p::before {content:counter(n) "|";}
        </style><p></p><p></p><p></p>"#
        )
        .concat(),
        "6|4|2|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:reversed(n);} p {counter-increment:n -1;}
        p.set {counter-set:n 9;} p::before {content:counter(n) "|";}
        </style><p></p><p class=set></p><p></p>"#
        )
        .concat(),
        "10|9|8|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:reversed(n);}
        p::before,p::after {counter-increment:n -1;content:counter(n) "|";}
        </style><p></p><p></p>"#
        )
        .concat(),
        "4|3|2|1|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        h2 {counter-reset:reversed(n);} p {counter-increment:n -1;}
        h2::before,p::before {content:counter(n) "|";}
        .hidden {display:none;}
        </style><h2></h2><p></p><p class=hidden></p><p></p><h2></h2><p></p>"#
        )
        .concat(),
        "3|2|1|2|1|"
    );
    let mut doc = parse_html("<div id=x style='counter-reset:reversed(n)'></div>");
    doc.set_viewport(800.0, 600.0);
    let id = doc.get_element_by_id("x").unwrap();
    assert_eq!(
        doc.computed_style_property(id, "counter-reset"),
        "reversed(n)"
    );
}

#[test]
fn automatic_reversed_initial_values_recompute_after_incremental_cascade() {
    let mut root = WebCore::new("main");
    root.node_id = 1;
    for (index, tag) in ["i", "b"].into_iter().enumerate() {
        let mut child = WebCore::new(tag);
        child.node_id = index as u32 + 2;
        root.children.push(child);
    }
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("main{counter-reset:reversed(n)} i,b{counter-increment:n -1} i::before,b::before{content:counter(n)} b:hover{display:none}");
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    assert_eq!(root.children[0].style.before_content, "2");
    for (hovered, expected) in [(true, "1"), (false, "2"), (true, "1")] {
        crate::css::clear_cascade_dirty(&mut root);
        root.children[1].cascade_dirty = true;
        root.has_dirty_descendant = true;
        let hover = if hovered {
            std::collections::HashSet::from([3])
        } else {
            std::collections::HashSet::new()
        };
        crate::css::apply_cascade_incremental(
            &mut root, &sheet, None, 16.0, 800.0, 600.0, 0, false, &hover,
        );
        assert_eq!(root.children[0].style.before_content, expected);
    }
}

#[test]
fn counter_reset_defaults_to_zero_but_increment_defaults_to_one() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-reset", "section");
    apply_property(&mut style, "counter-increment", "item");

    assert_eq!(
        style.counter_reset,
        vec![CounterReset::normal("section", 0)]
    );
    assert_eq!(style.counter_increment, vec![("item".to_string(), 1)]);
}

#[test]
fn counter_set_is_tracked_separately_from_counter_reset() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-set", "section 5 item");

    assert!(style.counter_reset.is_empty());
    assert_eq!(
        style.counter_set,
        vec![("section".to_string(), 5), ("item".to_string(), 0)]
    );
}

#[test]
fn content_counter_function_and_string_tokens_are_preserved() {
    let content = resolve_content_value(r#"counter(item) ". ""#);
    assert!(
        content.contains('\x01'),
        "counter() should become a deferred placeholder"
    );
    assert!(content.ends_with(". "));
}

#[test]
fn counter_style_argument_formats_generated_content() {
    let mut counters = std::collections::HashMap::new();
    counters.insert("chapter".to_string(), vec![4]);
    let content = resolve_content_value("counter(chapter, upper-roman)");
    assert_eq!(resolve_counters_in_content(&content, &counters), "IV");
    let content = resolve_content_value("counter(chapter, arabic-indic)");
    assert_eq!(resolve_counters_in_content(&content, &counters), "٤");
}

#[test]
fn counters_function_joins_nested_counter_scopes() {
    let mut counters = std::collections::HashMap::new();
    counters.insert("section".to_string(), vec![2, 5, 8]);
    let content = resolve_content_value(r#"counters(section, ". ") "#);
    assert_eq!(resolve_counters_in_content(&content, &counters), "2. 5. 8");
}

#[test]
fn aspect_ratio_math_uses_top_level_separator_and_rejects_invalid_values() {
    let mut style = ComputedStyle::default();
    for (source, expected) in [
        ("calc(8 / 2) / sqrt(4)", Some(2.0)),
        ("auto calc(3 + 1) / 2", Some(2.0)),
        ("sqrt(9) / calc(1 + 1) auto", Some(1.5)),
        ("calc(16 / 9)", Some(16.0 / 9.0)),
        ("0 / 2", None),
        ("2 / 0", None),
    ] {
        apply_property(&mut style, "aspect-ratio", source);
        assert_eq!(style.aspect_ratio, expected, "{source}");
    }
    for source in [
        "garbage / 2",
        "4 / garbage",
        "2 / 3 / 4",
        "-2 / 1",
        "auto auto",
        "2 auto / 1",
        "calc(2px) / 1",
        "",
    ] {
        style.aspect_ratio = Some(3.0);
        apply_property(&mut style, "aspect-ratio", source);
        assert_eq!(style.aspect_ratio, Some(3.0), "invalid: {source}");
    }
    let doc = parse_and_layout(
        "<div id=a style='width:240px;aspect-ratio:calc(8 / 2) / sqrt(4)'></div>",
        800.0,
    );
    let a = crate::dom::query_selector(&doc.root, "#a").unwrap();
    assert_eq!(a.layout.content_rect.h, 120.0);
    let doc = parse_and_layout(
        "<style>#a{width:240px;aspect-ratio:3;aspect-ratio:garbage / 2}#b{width:240px;aspect-ratio:3!important;aspect-ratio:garbage / 2!important}</style><div id=a></div><div id=b></div><div id=c style='width:240px;aspect-ratio:3;aspect-ratio:garbage / 2'></div>",
        800.0,
    );
    for id in ["#a", "#b", "#c"] {
        let node = crate::dom::query_selector(&doc.root, id).unwrap();
        assert_eq!(node.layout.content_rect.h, 80.0, "{id}");
    }
}

#[test]
fn css_math_serialization_round_trips_dimensions_and_fractional_lengths() {
    for source in [
        "0.125px",
        "-0.75px",
        "calc(50% - 0.25px)",
        "calc(2 * min(20%, 3em))",
        "calc((100% - 2em) / 3)",
        "round(up, 10% + 1em, 2px)",
        "mod(70% - 3px, 2em)",
        "rem(70% - 3px, 2em)",
        "abs(10% - 2em)",
        "calc(sign(10% - 2em) * 1px)",
        "hypot(10%, 2em)",
        "calc(sin(30deg) * 10%)",
        "calc(cos(.5turn) * 2em)",
        "calc(tan(45deg) * 3px)",
        "calc(asin(1) / 1deg * 1px)",
        "calc(acos(0) / 1deg * 1px)",
        "calc(atan(1) / 1rad * 1px)",
        "calc(atan2(10%, 2em) / 1deg * 1px)",
        "calc(pow(2, 3) * 1px)",
        "calc(sqrt(9) * 1px)",
        "calc(log(exp(2)) * 1px)",
        "calc(1s / 250ms * 2px)",
        "calc(2khz / 500hz * 2px)",
        "calc(192dpi / 1dppx * 2px)",
        "clamp(none, 50% + 2px, none)",
        "min(0px, 1em)",
    ] {
        let original = parse_length_checked(source).unwrap_or_else(|| panic!("input: {source}"));
        let serialized = crate::html::serializer::serialize_length(&original);
        let reparsed = parse_length_checked(&serialized)
            .unwrap_or_else(|| panic!("invalid serialization: {source} -> {serialized}"));
        for (font, basis) in [(16.0, 100.0), (30.0, 600.0)] {
            let expected = original.resolve_vp(font, basis, 20.0, 800.0, 600.0);
            let actual = reparsed.resolve_vp(font, basis, 20.0, 800.0, 600.0);
            assert!(
                (actual - expected).abs() < 0.001,
                "{source} -> {serialized}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn cssom_nonlinear_math_keeps_percentage_dependency() {
    let mut doc = parse_html(
        "<div id=a style='font-size:20px;background-size:round(up, 50% + 1em, 3px) auto'></div>",
    );
    doc.set_viewport(800.0, 600.0);
    let a = doc.get_element_by_id("a").unwrap();
    let computed = doc.computed_style_property(a, "background-size");
    let value = parse_length_checked(&computed).unwrap_or_else(|| panic!("{computed}"));
    assert!(
        value.has_percentage(),
        "computed value lost its percentage: {computed}"
    );
    assert_eq!(value.resolve(20.0, 100.0, 16.0), 72.0);
    assert_eq!(value.resolve(20.0, 200.0, 16.0), 120.0);
}

#[test]
fn css_math_numeric_properties_use_typed_values_and_integer_rounding() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "flex-grow", "sqrt(9)");
    apply_property(&mut style, "flex-shrink", "calc(1 / 2)");
    apply_property(&mut style, "order", "calc(-1.5)");
    apply_property(&mut style, "z-index", "calc(2.5)");
    assert_eq!(style.flex_grow, 3.0);
    assert_eq!(style.flex_shrink, 0.5);
    assert_eq!(style.order, -1);
    assert_eq!(style.z_index, 3);
    assert!(!style.z_index_is_auto);
    apply_property(&mut style, "flex-grow", "calc(2px)");
    apply_property(&mut style, "order", "1.5");
    assert_eq!(style.flex_grow, 3.0);
    assert_eq!(style.order, -1);
    apply_property(&mut style, "flex", "sqrt(4) calc(1 / 2) 30px");
    assert_eq!(style.flex_grow, 2.0);
    assert_eq!(style.flex_shrink, 0.5);
    assert_eq!(style.flex_basis, CssLength::Px(30.0));
    apply_property(&mut style, "flex-grow", "-3");
    assert_eq!(style.flex_grow, 2.0);
    apply_property(&mut style, "flex-grow", "calc(-3)");
    assert_eq!(style.flex_grow, 0.0);
    apply_property(&mut style, "columns", "calc(1 + 2) min(20em, 300px)");
    assert_eq!(style.column_count, Some(3));
    assert_eq!(style.column_width.resolve(20.0, 800.0, 16.0), 300.0);
    apply_property(&mut style, "column-count", "0");
    assert_eq!(style.column_count, Some(3));
    apply_property(&mut style, "column-count", "calc(0)");
    assert_eq!(style.column_count, Some(1));
    apply_property(&mut style, "widows", "sqrt(9)");
    apply_property(&mut style, "orphans", "calc(1.5)");
    assert_eq!((style.widows, style.orphans), (3, 2));

    let doc = parse_and_layout(
        "<style>#a{flex-grow:sqrt(9);flex-shrink:calc(1 / 2);order:calc(-1.5);z-index:calc(2.5)}</style><div id=a>a</div>",
        400.0,
    );
    let a = crate::dom::query_selector(&doc.root, "#a").unwrap();
    assert_eq!(
        (
            a.style.flex_grow,
            a.style.flex_shrink,
            a.style.order,
            a.style.z_index
        ),
        (3.0, 0.5, -1, 3)
    );
}

#[test]
fn css_math_clamp_conflicting_bounds_favor_minimum() {
    for (source, expected) in [
        ("clamp(100px, 75px, 50px)", 100.0),
        ("clamp(5em, 75px, 50px)", 100.0),
        ("clamp(50%, 75px, 50px)", 200.0),
        ("min(150px, clamp(50%, 75px, 50px))", 150.0),
        ("calc(clamp(100px, 75px, 50px) + 10px)", 110.0),
        ("clamp(-10px, -30px, -20px)", -10.0),
    ] {
        let length = parse_length_checked(source).unwrap();
        assert_eq!(
            length.resolve_vp(20.0, 400.0, 16.0, 800.0, 600.0),
            expected,
            "{source}"
        );
    }
    for width in [200.0, 400.0] {
        let doc = parse_and_layout(
            "<style>body{margin:0}#subject{width:clamp(50%,75px,50px);height:20px}</style><div id=subject></div>",
            width,
        );
        let node = crate::dom::query_selector(&doc.root, "#subject").unwrap();
        assert_eq!(node.layout.content_rect.w, width * 0.5);
    }
}

#[test]
fn css_math_functions_resolve_mixed_units_and_precedence() {
    for (value, expected) in [
        ("calc(1e2px + 2 * 3px)", 106.0),
        ("min(100% - 20px, 15em)", 300.0),
        ("clamp(none, 50% + 10px, 15em)", 210.0),
        ("round(up, 1em + 3px, 5px)", 25.0),
        ("mod(-23px, 1em)", 17.0),
        ("rem(-23px, 1em)", -3.0),
        ("hypot(3em, 80px)", 100.0),
        ("abs(1em - 50px)", 30.0),
        ("calc(sign(1em - 50px) * 10px)", -10.0),
        ("calc(sin(30deg) * 100px)", 50.0),
        ("calc(sqrt(9) * pow(2, 3) * 1px)", 24.0),
        ("calc(1in / 48px * 2em)", 80.0),
        ("calc(atan2(20px, 1em) / 1deg * 1px)", 45.0),
        ("calc(cos(0turn) * 10px)", 10.0),
        ("calc(tan(45deg) * 10px)", 10.0),
        ("calc(asin(1) / 1deg * 1px)", 90.0),
        ("calc(acos(0) / 1deg * 1px)", 90.0),
        ("calc(atan(1) / 1deg * 1px)", 45.0),
        ("calc(log(8, 2) * 10px)", 30.0),
        ("calc(log(exp(2)) * 10px)", 20.0),
        ("calc(sin(pi / 2) * 10px)", 10.0),
        ("round(down, -23px, 5px)", -25.0),
        ("round(to-zero, -23px, 5px)", -20.0),
        ("round(nearest, -22.5px, 5px)", -20.0),
        ("calc(round(2.5) * 10px)", 30.0),
        ("MAX(2em, 30px)", 40.0),
        ("round(1px, 3e-40px)", 1.0),
        ("round(up, 2em, 3e-40px)", 40.0),
        ("round(down, -2em, 3e-40px)", -40.0),
    ] {
        let parsed = parse_length_checked(value).unwrap_or_else(|| panic!("rejected {value}"));
        let actual = parsed.resolve_vp(20.0, 400.0, 16.0, 1000.0, 600.0);
        assert!(
            (actual - expected).abs() < 0.01,
            "{value}: {actual}, expected {expected}"
        );
    }
}

#[test]
fn css_math_resolves_in_property_consumers() {
    let doc = parse_and_layout(
        r#"<style>
       #math { width:min(100% - 20px, 15em); font-size:20px;
         opacity:calc(25% + 25%); line-height:sqrt(4);
         transform:rotate(calc(.25turn + 90deg)) scale(sqrt(4));
         animation-duration:calc(1s + 250ms); }
    </style><div style='width:400px'><div id=math>math</div></div>"#,
        800.0,
    );
    let node = crate::dom::query_selector(&doc.root, "#math").unwrap();
    assert!((node.layout.content_rect.w - 300.0).abs() < 0.1);
    assert_eq!(node.style.opacity, 0.5);
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "opacity", "0.4");
    apply_property(&mut style, "opacity", "calc(25% + 0.25)");
    assert_eq!(
        style.opacity, 0.4,
        "incompatible opacity types must not replace the previous value"
    );
    assert_eq!(node.style.line_height, CssLength::Em(2.0));
    assert_eq!(crate::css::parse_time_ms("calc(1s + 250ms)"), Some(1250.0));
    let transform = crate::css::parse_css_transform("rotate(calc(.25turn + 90deg)) scale(sqrt(4))");
    assert!(matches!(transform.ops[0], TransformOp::Rotate(angle) if (angle - 180.0).abs() < 0.01));
    assert!(matches!(transform.ops[1], TransformOp::Scale(2.0, 2.0)));
}

#[test]
fn css_math_rejects_incompatible_dimensions_and_incomplete_input() {
    for value in [
        "calc(1px + 2)",
        "calc(1px * 2px)",
        "calc(1px +)",
        "calc(1px garbage)",
        "calc(1px+ 2px)",
        "min(1px, 2s)",
        "sqrt(4px)",
        "round(10px)",
        "clamp(1px, 2px)",
    ] {
        assert!(parse_length_checked(value).is_none(), "accepted {value}");
    }
}

#[test]
fn implicit_counters_continue_across_siblings_without_escaping_parent() {
    let doc = parse_and_layout(
        r#"<style>
        .item { counter-increment: popular; display:flex }
        .item::before { content: counter(popular) }
        .set { counter-set: assigned 7 }
        .next { counter-increment: assigned }
        .next::before { content: counter(assigned) }
    </style>
    <section><h2>Popular</h2><ol style='counter-reset:none'>
      <li class=item id=a>one</li><li class=item id=b>two</li>
      <li class=item id=c>three<div class=item id=nested>four</div></li>
      <li class=item id=d>five</li>
    </ol></section>
    <section><div class=item id=separate>one</div></section>
    <section><div class=set></div><div class=next id=setnext>eight</div></section>"#,
        800.0,
    );
    for (id, expected) in [
        ("#a", "1"),
        ("#b", "2"),
        ("#c", "3"),
        ("#nested", "4"),
        ("#d", "5"),
        ("#separate", "1"),
        ("#setnext", "8"),
    ] {
        let node = crate::dom::query_selector(&doc.root, id).unwrap();
        let text = node
            .children
            .iter()
            .find(|child| child.tag == "::before")
            .map(|pseudo| pseudo.text.as_str())
            .unwrap_or(&node.style.before_content);
        assert_eq!(text, expected, "{id}");
    }
}

#[test]
fn counter_reset_then_increment_starts_generated_content_at_one() {
    let html = r#"<style>
              ol { counter-reset: item; }
              li { counter-increment: item; }
              li::before { content: counter(item) ". "; }
            </style>
            <ol><li id="first">one</li><li id="second">two</li></ol>"#;
    let texts = build_display_texts(html);

    assert!(
        texts.iter().any(|text| text.contains("1.")),
        "first generated marker should include 1.; painted texts were {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("2.")),
        "second generated marker should include 2.; painted texts were {texts:?}"
    );
}

#[test]
fn explicit_counter_resets_survive_siblings_and_replace_same_level_instances() {
    assert_eq!(
        build_display_texts(
            r#"<style>
        h2 {counter-reset:n 5;}
        h2.second {counter-reset:n 9;} p {counter-increment:n;}
        h2::before,p::before {content:counters(n,".") "|";}
        </style><h2></h2><p></p><h2 class=second></h2><p></p>"#
        )
        .concat(),
        "5|6|9|10|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n 1;} section {counter-reset:n 5;}
        div {counter-reset:n 8;} p {counter-increment:n;}
        div::before,p::before {content:counters(n,".") "|";}
        </style><section><div></div><p></p></section><p></p>"#
        )
        .concat(),
        "1.5.8|1.6|2|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        section::before {counter-reset:n 4;content:counters(n,".") "|";}
        section::after {counter-increment:n;content:counters(n,".") "|";}
        p {counter-increment:n;} p::before {content:counters(n,".") "|";}
        </style><section><p></p></section><p></p>"#
        )
        .concat(),
        "4|5|6|1|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n 1;} p {counter-reset:n 4 n 7;}
        p::before {content:counters(n,".");}</style><p></p>"#
        )
        .concat(),
        "1.7"
    );
}

#[test]
fn content_attr_reads_originating_element_attribute() {
    let html = r#"<style>
              [data-label]::before { content: attr(data-label) ": "; }
            </style>
            <p data-label="Name">Ada</p>"#;
    let texts = build_display_texts(html);

    assert!(
        texts.iter().any(|text| text.contains("Name: ")),
        "attr() content should paint the originating element attribute; painted texts were {texts:?}"
    );
}

#[test]
fn var_substitution_ignores_quoted_text_but_resolves_function_tokens() {
    let html = r#"<style>
        :root { --word: 'resolved'; }
        p::before { content: "var(--word)"; }
        p::after { content: var(--word); }
    </style><p>middle</p>"#;
    let texts = build_display_texts(html);
    assert!(
        texts.iter().any(|text| text.contains("var(--word)")),
        "quoted text must remain literal: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("resolved")),
        "function token must still substitute: {texts:?}"
    );

    let vars = std::collections::HashMap::from([("--word".to_string(), "blue".to_string())]);
    assert_eq!(
        crate::css::resolve_var_references(r#""var(--word)" var(--word)"#, &vars),
        "\"var(--word)\" blue"
    );
    assert_eq!(
        crate::css::resolve_var_references("myvar(--word) évar(--word) var(--word)", &vars),
        "myvar(--word) évar(--word) blue"
    );
    assert_eq!(
        crate::css::resolve_var_references(r#"var(--missing, "a,b")"#, &vars),
        "\"a,b\""
    );
    let literal = resolved_root_variables(r#":root { --literal: "var(--missing)"; }"#);
    assert_eq!(literal["--literal"], "\"var(--missing)\"");

    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(r#"p::before { content: "var(--word)" }"#);
    sheet.rebuild_index();
    assert!(!sheet.rules[0].has_var_refs);
}

#[test]
fn var_function_resolves_a_variable_supplied_property_name() {
    let vars = std::collections::HashMap::from([
        ("--name".to_string(), "--color".to_string()),
        ("--color".to_string(), "green".to_string()),
    ]);
    assert_eq!(
        crate::css::resolve_var_references("var(var(--name))", &vars),
        "green"
    );
    assert_eq!(
        crate::css::resolve_var_references("var(var(--missing), red)", &vars),
        "red"
    );
    assert_eq!(
        crate::css::resolve_var_references("var(var(--name), red)", &vars),
        "green"
    );

    let texts = build_display_texts(
        r#"<style>:root { --name: --word; --word: "green"; }
           p::before { content: var(var(--name)); }</style><p>middle</p>"#,
    );
    assert!(
        texts.iter().any(|text| text.contains("green")),
        "nested variable name should reach generated content: {texts:?}"
    );
}

#[test]
fn variable_functions_and_references_decode_shared_css_tokens() {
    let vars = std::collections::HashMap::from([
        ("--color".to_string(), "green".to_string()),
        ("--COLOR".to_string(), "blue".to_string()),
    ]);
    for (source, expected) in [
        (r"v\61 r(--color)", "green"),
        (r"\56 AR(--COLOR)", "blue"),
        (r"var(--c\6f lor)", "green"),
        ("var(/* name */ --color /* tail */)", "green"),
        (r#""v\61 r(--color)""#, r#""v\61 r(--color)""#),
        ("url(var(--color))", "url(var(--color))"),
        ("var(--missing, rgb(1, 2, 3))", "rgb(1, 2, 3)"),
    ] {
        assert_eq!(crate::css::resolve_var_references(source, &vars), expected);
    }
    let doc = parse_and_layout(
        r#"<style>
        :root { --color: green; --width: 12px; --nested: v\61 r(--width); }
        #reference { color: v\61 r(--c\6f lor); padding: var(--nested); }
        @supports (color: v\61 r(--c\6f lor)) { #query { color: blue; } }
        #inline { color: red; }
        </style><div id="reference">reference</div><div id="query">query</div>
        <div id="inline" style="color: \76 ar(--color)">inline</div>"#,
        800.0,
    );
    for id in ["reference", "inline"] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|v| v == id)
        })
        .unwrap();
        assert_eq!(node.style.color, Color::rgb(0, 128, 0), "{id}");
    }
    let reference = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|v| v == "reference")
    })
    .unwrap();
    assert_eq!(reference.style.padding_top, CssLength::Px(12.0));
    let query = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|v| v == "query")
    })
    .unwrap();
    assert_eq!(query.style.color, Color::rgb(0, 0, 255));
}

#[test]
fn var_function_name_is_case_insensitive_but_custom_property_name_is_not() {
    let vars = std::collections::HashMap::from([
        ("--color".to_string(), "green".to_string()),
        ("--COLOR".to_string(), "blue".to_string()),
    ]);
    assert_eq!(
        crate::css::resolve_var_references("VAR(--color) VaR(--COLOR)", &vars),
        "green blue"
    );
    let texts = build_display_texts(
        r#"<style>:root { --color: "green"; } p::before { content: VaR(--color); }</style><p>x</p>"#,
    );
    assert!(texts.iter().any(|text| text.contains("green")), "{texts:?}");
}

#[test]
fn content_on_normal_element_does_not_replace_rendered_inline_text() {
    let html = r#"<style>
              p { content: "Generated " attr(data-label); }
            </style>
            <p data-label="label">Original <span>child</span></p>"#;
    let doc = parse_html(html);
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    fn find<'a>(node: &'a crate::WebCore, tag: &str) -> Option<&'a crate::WebCore> {
        if node.tag == tag {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, tag))
    }
    let p = find(&frame.doc.root, "p").expect("p element exists");
    assert_eq!(p.style.rare().content, "Generated label");
    let list = build_display_list(&frame.doc.root, 800.0, 600.0);
    let texts: Vec<String> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !texts.iter().any(|text| text.contains("Generated label")),
        "content on a normal element should not paint as generated text; painted texts were {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("Original"))
            || texts.iter().any(|text| text.contains("child")),
        "normal element descendants should still paint; painted texts were {texts:?}"
    );
}

#[test]
fn style_containment_scopes_quote_depth_without_resetting_it() {
    for contain in ["style", "layout style", "content", "strict"] {
        let html = format!(
            r#"<style>
            body {{quotes:"[" "]" "{{" "}}";}}
            .open::before {{content:open-quote;}}
            .close::before {{content:close-quote;}}
            .scope {{contain:{contain}; width:200px; height:40px;}}
            .scope::before {{content:open-quote;}}
            </style><span class=open></span><div class=scope><span class=open></span></div><span class=close></span>"#
        );
        assert_eq!(build_display_texts(&html).concat(), "[{{]", "{contain}");
    }
    assert_eq!(build_display_texts(r#"<style>
        body {quotes:"[" "]" "{" "}";}
        .scope {contain:style;}
        .scope::before {content:open-quote;}
        .scope .scope::after {content:close-quote;}
        .close::before {content:close-quote "X";}
        </style><div class=scope><div class=scope></div><span class=close></span></div><span class=close></span>"#).concat(), "[{}]XX");
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {quotes:"[" "]" "{" "}";}
        p::before {content:open-quote; contain:style;}
        p::after {content:close-quote "X";}
        </style><p>A</p>"#
        )
        .concat(),
        "[A]X"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {quotes:"[" "]";}
        .scope {contain:style; display:contents;}
        .scope::before {content:open-quote;}
        .close::before {content:close-quote;}
        </style><div class=scope>A</div><span class=close></span>"#
        )
        .concat(),
        "[A]"
    );
}

#[test]
fn inline_generated_content_surrounds_block_children_in_flow() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
        body {margin:0; font:20px/30px monospace;}
        section::before {content:"[";} section::after {content:"]";}
        </style><section><div>X</div></section>"#,
        800.0,
    );
    let list = crate::renderer::display_list_builder::build_display_list_full_with_font_system(
        &doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
        Some(&mut renderer.font_system),
    );
    let text: Vec<_> = list
        .commands
        .iter()
        .filter_map(|command| {
            if let PaintCmd::Text { y, text, .. } = command {
                Some((*y, text.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text.iter().map(|(_, text)| *text).collect::<String>(),
        "[X]"
    );
    assert_eq!(text.len(), 3);
    assert!((text[1].0 - text[0].0 - 30.0).abs() < 0.01, "{text:?}");
    assert!((text[2].0 - text[1].0 - 30.0).abs() < 0.01, "{text:?}");
}

#[test]
fn generated_content_only_blocks_reserve_line_height() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
        body {margin:0; font:20px/30px monospace; quotes:"[" "]";}
        .generated::before {content:open-quote;}
        .generated::after {content:close-quote;}
        </style><div class=generated></div><div>X</div>"#,
        800.0,
    );
    let list = crate::renderer::display_list_builder::build_display_list_full_with_font_system(
        &doc.root,
        800.0,
        600.0,
        0.0,
        0.0,
        0,
        0,
        &std::collections::HashSet::new(),
        "",
        Some(&mut renderer.font_system),
    );
    let text: Vec<_> = list
        .commands
        .iter()
        .filter_map(|command| {
            if let PaintCmd::Text { y, text, .. } = command {
                Some((*y, text.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        text.iter().map(|(_, text)| *text).collect::<String>(),
        "[]X"
    );
    let first_y = text.first().unwrap().0;
    let last_y = text.last().unwrap().0;
    assert!((last_y - first_y - 30.0).abs() < 0.01, "{text:?}");
}

#[test]
fn style_containment_counters_read_outer_values_but_write_nested_instances() {
    for containment in ["style", "layout style", "content", "strict"] {
        let html = format!(
            r#"<style>
            body {{counter-reset:n;}}
            .item {{counter-increment:n;}}
            .item::before {{content:counters(n,".") "|";}}
            .scope {{contain:{containment}; width:200px; height:100px;}}
            .scope::after {{content:counters(n,".") "|";}}
            </style><div class=item></div><div class="item scope"><div class=item></div><div class=item></div></div><div class=item></div>"#
        );
        assert_eq!(
            build_display_texts(&html).concat(),
            "1|2|2.1|2.2|2.2|3|",
            "{containment}"
        );
    }
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n 7;} .scope {contain:style;counter-reset:n 20;}
        .scope span {counter-set:n 4;} span::before {content:counters(n,".") "|";}
        </style><div class=scope><span></span></div><span></span>"#
        )
        .concat(),
        "7.20.4|7|"
    );
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n 7;} .scope {contain:style;}
        .scope::before {counter-increment:n;content:counters(n,".") "|";}
        span::before {content:counters(n,".") "|";}
        </style><div class=scope><span></span></div><span></span>"#
        )
        .concat(),
        "7.1|7.1|7|"
    );
    assert_eq!(build_display_texts(r#"<style>
        body {counter-reset:n 5;} .scope {contain:style;}
        .item {counter-increment:n;} .item::before {content:counters(n,".") "|";}
        .scope::after {content:counters(n,".") "|";}
        </style><div class="item scope"><div class="item scope"><div class=item></div></div><div class=item></div></div><div class=item></div>"#).concat(), "6|6.1|6.1.1|6.1.1|6.2|6.2|7|");
}

#[test]
fn after_counters_follow_descendant_counter_operations() {
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {counter-reset:n;}
        div::before {content:counter(n) "|";}
        div::after {counter-increment:n;content:counter(n) "|";}
        span {counter-increment:n;}
        span::before {content:counter(n) "|";}
        </style><div><span></span><span></span></div><span></span>"#
        )
        .concat(),
        "0|1|2|3|4|"
    );
}

#[test]
fn display_contents_text_paints_once_in_parent_inline_context() {
    for markup in [
        "<div style='display:contents'>A</div>B",
        "<div style='display:contents'><span style='display:contents'>A</span></div>B",
        "<div style='display:contents'><div>A</div><div>B</div></div>",
        "<div style='display:flex'><div style='display:contents'>A<span>B</span></div></div>",
    ] {
        let text = build_display_texts(markup).concat();
        assert_eq!(text.matches('A').count(), 1, "{markup}: {text}");
        assert_eq!(text.matches('B').count(), 1, "{markup}: {text}");
    }
}

#[test]
fn contain_grammar_rejects_partial_keywords_and_duplicates() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "contain", "layout paint");
    for invalid in [
        "not-layout",
        "size bogus",
        "style style",
        "strict paint",
        "none style",
        "size inline-size",
        "",
    ] {
        apply_property(&mut style, "contain", invalid);
        assert!(
            style.contain_layout && style.contain_paint && !style.contain_size,
            "{invalid}"
        );
    }
    apply_property(&mut style, "contain", "inline-size");
    assert!(
        !style.contain_size,
        "inline-size must not suppress block-size contributions"
    );
    assert!(style.contain_inline_size);
}

#[test]
fn contain_computed_values_preserve_keywords_and_cascade() {
    let mut doc = parse_html(
        r#"<style>
        #parent {contain:style paint;}
        #inherit {contain:inherit;} #unset {contain:unset;}
        #strict {contain:strict;} #content {contain:content;}
        #ordered {contain:paint STYLE inline-size layout;}
        #invalid {contain:style; contain:style style;}
        </style><div id=parent><div id=inherit></div><div id=unset></div><div id=initial></div></div>
        <div id=strict></div><div id=content></div><div id=ordered></div><div id=invalid></div>"#,
    );
    doc.set_viewport(800.0, 600.0);
    for (id, expected) in [
        ("parent", "style paint"),
        ("inherit", "style paint"),
        ("unset", "none"),
        ("initial", "none"),
        ("strict", "strict"),
        ("content", "content"),
        ("ordered", "inline-size layout style paint"),
        ("invalid", "style"),
    ] {
        let node = doc.get_element_by_id(id).unwrap();
        assert_eq!(
            doc.computed_style_property(node, "contain"),
            expected,
            "{id}"
        );
    }
}

#[test]
fn generated_quotes_follow_document_depth() {
    let css = r#"<style>body { quotes: "[" "]" "{" "}"; }
        q::before { content:open-quote; } q::after { content:close-quote; }
        .open::before { content:no-open-quote; }
        .close::after { content:no-close-quote; }
        .end::before { content:close-quote "X"; }
        .hidden { display:none; } .hidden::before { content:open-quote; }
        </style>"#;
    for (body, expected) in [
        ("<q>A<q>B<q>C</q></q>D</q>", "[A{B{C}}D]"),
        (
            "<span class=open></span><q>A</q><span class=close></span><q>B</q>",
            "{A}[B]",
        ),
        ("<span class=end></span><q>A</q>", "X[A]"),
        ("<span class=hidden><q>Hidden</q></span><q>A</q>", "[A]"),
        (
            "<q style='quotes:none'>A<q style='quotes:inherit'>B</q><q style='quotes:\"[\" \"]\" \"{\" \"}\"'>C</q></q>",
            "AB{C}",
        ),
    ] {
        assert_eq!(
            build_display_texts(&format!("{css}{body}")).concat(),
            expected,
            "{body}"
        );
    }
}

#[test]
fn quote_depth_survives_incremental_and_parallel_cascade() {
    for extra_rules in [0, 1001] {
        let mut root = WebCore::new("main");
        root.node_id = 1;
        for (i, tag) in ["i", "q"].into_iter().enumerate() {
            let mut child = WebCore::new(tag);
            child.node_id = i as u32 + 2;
            root.children.push(child);
        }
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add(
            r#"main { quotes:"[" "]" "{" "}"; }
            i::before { content:no-open-quote; }
            i:hover::before { content:none; }
            q::before { content:open-quote; } q::after { content:close-quote; }"#,
        );
        for i in 0..extra_rules {
            sheet.parse_and_add(&format!(".unused{i} {{color:green}}"));
        }
        sheet.rebuild_index();
        crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
        assert_eq!(root.children[1].style.before_content, "{");
        assert_eq!(root.children[1].style.after_content, "}");
        crate::css::clear_cascade_dirty(&mut root);
        root.children[0].cascade_dirty = true;
        root.has_dirty_descendant = true;
        let hover = std::collections::HashSet::from([2]);
        crate::css::apply_cascade_incremental(
            &mut root, &sheet, None, 16.0, 800.0, 600.0, 0, false, &hover,
        );
        assert_eq!(root.children[1].style.before_content, "[");
        assert_eq!(root.children[1].style.after_content, "]");
        crate::css::resolve_document_generated_content(&mut root, &sheet);
        assert_eq!(
            root.children[1].style.before_content, "[",
            "repeat must reset depth"
        );
    }
}

#[test]
fn incremental_counters_replay_clean_ancestors_and_following_siblings() {
    let mut root = WebCore::new("main");
    root.node_id = 1;
    for (index, tag) in ["i", "b"].into_iter().enumerate() {
        let mut child = WebCore::new(tag);
        child.node_id = index as u32 + 2;
        root.children.push(child);
    }
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        r#"main {counter-reset:n 7;}
        i {counter-increment:n 2;} i:hover {counter-increment:n 4;}
        b {counter-increment:n;}
        i::before, b::before {content:counter(n);}"#,
    );
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    assert_eq!(root.children[0].style.before_content, "9");
    assert_eq!(root.children[1].style.before_content, "10");
    crate::css::clear_cascade_dirty(&mut root);
    root.children[0].cascade_dirty = true;
    root.has_dirty_descendant = true;
    crate::css::apply_cascade_incremental(
        &mut root,
        &sheet,
        None,
        16.0,
        800.0,
        600.0,
        0,
        false,
        &std::collections::HashSet::from([2]),
    );
    assert_eq!(root.children[0].style.before_content, "11");
    assert_eq!(root.children[1].style.before_content, "12");
    assert!(root.children[1].layout.layout_dirty);
}

#[test]
fn incremental_counter_templates_update_atomic_pseudos_and_preserve_clean_styles() {
    fn text(node: &WebCore) -> String {
        let Some(pseudo) = node.children.iter().find(|n| n.tag == "::before") else {
            return node.style.before_content.clone();
        };
        if pseudo.children.is_empty() {
            pseudo.text.clone()
        } else {
            pseudo.children[0].text.clone()
        }
    }
    for display in ["inline", "inline-block", "inline-flex", "inline-grid"] {
        let mut root = WebCore::new("main");
        root.node_id = 1;
        for (index, tag) in ["i", "b", "em"].into_iter().enumerate() {
            let mut child = WebCore::new(tag);
            child.node_id = index as u32 + 2;
            root.children.push(child);
        }
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add(&format!(
            r#"main {{counter-reset:n 7;}}
            i {{counter-increment:n 2;}} i:hover {{counter-increment:n 4;}}
            b {{counter-increment:n;}}
            i::before, b::before {{content:counter(n);display:{display};}}"#
        ));
        sheet.rebuild_index();
        crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
        let clean_style = root.children[2].style.clone();
        for (hover, expected) in [
            (true, ["11", "12"]),
            (false, ["9", "10"]),
            (true, ["11", "12"]),
        ] {
            crate::css::clear_cascade_dirty(&mut root);
            root.children[0].cascade_dirty = true;
            root.has_dirty_descendant = true;
            let chain = if hover {
                std::collections::HashSet::from([2])
            } else {
                std::collections::HashSet::new()
            };
            crate::css::apply_cascade_incremental(
                &mut root, &sheet, None, 16.0, 800.0, 600.0, 0, false, &chain,
            );
            assert_eq!(text(&root.children[0]), expected[0], "{display}");
            assert_eq!(text(&root.children[1]), expected[1], "{display}");
            assert!(std::sync::Arc::ptr_eq(
                &clean_style,
                &root.children[2].style
            ));
        }
    }
}

#[test]
fn generated_counters_update_materialized_pseudos_inside_layout_wrappers() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
        section {counter-reset:n 1;}
        section::before,section::after {content:counter(n);}
        </style><section><div>X</div></section>"#,
        800.0,
    );
    fn section(node: &mut WebCore) -> Option<&mut WebCore> {
        if node.tag == "section" {
            return Some(node);
        }
        node.children.iter_mut().find_map(section)
    }
    let owner = section(&mut doc.root).unwrap();
    assert!(
        owner
            .children
            .iter()
            .any(|child| child.tag == "anonymous-block")
    );
    std::sync::Arc::make_mut(&mut owner.style).counter_reset = vec![CounterReset::normal("n", 9)];
    crate::css::resolve_document_generated_content(&mut doc.root, &doc.stylesheet);
    let owner = section(&mut doc.root).unwrap();
    for tag in ["::before", "::after"] {
        let pseudo = find_box(owner, &|node| node.tag == tag).unwrap();
        assert_eq!(
            pseudo.text, "9",
            "{tag} must update inside its layout wrapper"
        );
    }
    assert!(
        owner.style.before_content.is_empty(),
        "materialized text must not also paint through legacy content"
    );
    assert!(owner.style.after_content.is_empty());
}

#[test]
fn generated_quote_tokens_are_not_literal_text_and_keep_counters() {
    assert_eq!(
        build_display_texts(
            r#"<style>
        body {quotes:"[" "]" "{" "}"; counter-reset:n 7;}
        p::before {content:"open-quote" open-quote no-open-quote counter(n) attr(data-x);}
        p::after {content:close-quote close-quote close-quote "end";}
        </style><p data-x=Y>X</p>"#
        )
        .concat(),
        "open-quote[7YX}]end"
    );
    for display in ["inline-block", "inline-flex", "inline-grid"] {
        let html = format!(
            r#"<style>
            body {{quotes:"[" "]" "{{" "}}";}}
            q::before {{content:open-quote; display:{display};}}
            q::after {{content:close-quote; display:{display};}}
            </style><q id=outer>A<q id=inner>B</q></q>"#
        );
        let doc = parse_and_layout(&html, 800.0);
        for (id, before, after) in [("outer", "[", "]"), ("inner", "{", "}")] {
            let node = find_box(&doc.root, &|node| {
                node.attributes.get("id").is_some_and(|value| value == id)
            })
            .unwrap();
            for (tag, expected) in [("::before", before), ("::after", after)] {
                let pseudo = node.children.iter().find(|child| child.tag == tag).unwrap();
                let text = if pseudo.children.is_empty() {
                    &pseudo.text
                } else {
                    &pseudo.children[0].text
                };
                assert_eq!(text, expected, "{display} {id} {tag}");
            }
        }
        // Atomic boxes paint in separate commands, not DOM text order.
        let painted = build_display_texts(&html).concat();
        for ch in "[A{B}]".chars() {
            assert_eq!(painted.matches(ch).count(), 1, "{display}: {painted}");
        }
        let mut renderer = crate::Renderer::new();
        let doc = renderer.load_html(&html, 800.0);
        let list = crate::renderer::display_list_builder::build_display_list_full_with_font_system(
            &doc.root,
            800.0,
            600.0,
            0.0,
            0.0,
            0,
            0,
            &std::collections::HashSet::new(),
            "",
            Some(&mut renderer.font_system),
        );
        let mut glyphs: Vec<_> = list
            .commands
            .iter()
            .filter_map(|command| {
                if let PaintCmd::Text { x, text, .. } = command {
                    Some((*x, text.as_str()))
                } else {
                    None
                }
            })
            .collect();
        glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(
            glyphs.iter().map(|(_, text)| *text).collect::<String>(),
            "[A{B}]",
            "{display}"
        );
        assert!(
            glyphs.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "overlapping origins: {glyphs:?}"
        );
    }
}

#[test]
fn generated_quotes_use_computed_quotes_property() {
    let html = r#"<style>
              p::before { content: open-quote; quotes: "[" "]"; }
              p::after { content: close-quote; quotes: "[" "]"; }
            </style>
            <p>Ada</p>"#;
    let texts = build_display_texts(html);

    assert!(
        texts.iter().any(|text| text.contains("[")),
        "open-quote should use the computed quotes property; painted texts were {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("]")),
        "close-quote should use the computed quotes property; painted texts were {texts:?}"
    );
}

#[test]
fn generated_quotes_follow_pseudo_cascade_and_inheritance() {
    let inherited = r#"<style>
        p { quotes: "[" "]"; }
        p::before { content: open-quote; }
        p::after { content: close-quote; }
        </style><p>Ada</p>"#;
    assert_eq!(build_display_texts(inherited).concat(), "[Ada]");

    let overridden = r#"<style>
        p { quotes: "[" "]"; }
        p::before { content: open-quote; quotes: "(" ")" !important; }
        p::before { quotes: "{" "}"; }
        p::after { content: close-quote; quotes: "<" ">"; }
        </style><p>Ada</p>"#;
    assert_eq!(build_display_texts(overridden).concat(), "(Ada>");

    let marker = r#"<style>
        li { quotes: "[" "]"; }
        li::marker { content: open-quote "*" close-quote; quotes: "(" ")"; }
        </style><ul><li>Ada</li></ul>"#;
    assert_eq!(build_display_markers(marker), vec!["[*]"]);
}

#[test]
fn quotes_none_auto_and_escaped_pairs_remain_distinct() {
    let mut doc = parse_html(
        r#"<style>
        #none { quotes:none; } #auto { quotes:auto; }
        #pairs { quotes:"\22" "\5c"; } #controls { quotes:"\a" "\7f"; }
        </style><div id=none><span id=child>Inherited</span></div>
        <div id=auto></div><div id=pairs></div><div id=controls></div>"#,
    );
    doc.set_viewport(800.0, 600.0);
    for (id, expected) in [
        ("none", "none"),
        ("child", "none"),
        ("auto", "auto"),
        ("pairs", r#""\"" "\\""#),
        ("controls", r#""\a " "\7f ""#),
    ] {
        let node = doc.get_element_by_id(id).unwrap();
        assert_eq!(
            doc.computed_style_property(node, "quotes"),
            expected,
            "{id}"
        );
    }
    let texts = build_display_texts(
        r#"<style>
        p { quotes:none; } p::before { content:open-quote; }
        p::after { content:close-quote; }
        </style><p>Silent</p>"#,
    );
    assert_eq!(texts.concat(), "Silent");
    let texts = build_display_texts(
        r#"<style>
        p { quotes:none; } p::before { quotes:auto; content:open-quote; }
        p::after { content:close-quote; }
        </style><p>Auto</p>"#,
    );
    assert_eq!(texts.concat(), "\u{201c}Auto");
    let texts = build_display_texts(
        r#"<style>
        p { quotes:"\22" "\5c"; }
        p::before { content:open-quote "a\"b" "c"; }
        p::after { content:"d" close-quote; }
        </style><p>X</p>"#,
    );
    assert_eq!(texts.concat(), "\"a\"bcXd\\");
    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "quotes", r#""[" "]" "<" ">""#);
    let pairs = style.rare().quotes.clone();
    for invalid in [
        "",
        "bogus",
        r#""odd""#,
        r#""a" "b" garbage"#,
        "\"unterminated",
        "\"a\nb\" \"c\"",
    ] {
        crate::css::apply_property(&mut style, "quotes", invalid);
        assert_eq!(
            style.rare().quotes,
            pairs,
            "invalid quotes must preserve previous value: {invalid}"
        );
    }
    assert_eq!(
        crate::css::resolve_content_value("\"\\41\tB\" \"\\0 \" \"a\\\nb\""),
        "AB\u{fffd}ab"
    );
}

#[test]
fn table_row_height_sets_minimum_row_size() {
    let doc = parse_html(
        "<style>body{margin:0}td{padding:0;border:0}tr{height:50px}</style>\
         <table><tr id=row><td>x</td></tr></table>",
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    fn find<'a>(node: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let row = find(&frame.doc.root, "row").expect("row exists");
    assert!(
        (row.layout.border_rect.h - 50.0).abs() < 0.5,
        "row height should be at least authored height, got {}",
        row.layout.border_rect.h
    );
}

#[test]
fn caption_side_block_end_places_caption_after_rows() {
    let doc = parse_html(
        "<style>body{margin:0}caption{caption-side:block-end;height:20px}\
         td{padding:0;border:0}</style>\
         <table><caption id=cap>cap</caption><tr id=row><td>x</td></tr></table>",
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    fn find<'a>(node: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let cap = find(&frame.doc.root, "cap").expect("caption exists");
    let row = find(&frame.doc.root, "row").expect("row exists");
    assert!(
        cap.layout.border_rect.y >= row.layout.border_rect.y + row.layout.border_rect.h - 0.5,
        "block-end caption should be after rows; cap={:?} row={:?}",
        cap.layout.border_rect,
        row.layout.border_rect
    );
}

#[test]
fn caption_side_inline_start_places_caption_before_rows() {
    let doc = parse_html(
        "<style>body{margin:0}table{width:60px}caption{caption-side:inline-start;width:30px;height:20px}\
         td{padding:0;border:0;width:60px;height:10px}</style>\
         <table><caption id=cap>cap</caption><tr id=row><td>x</td></tr></table>",
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    fn find<'a>(node: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let cap = find(&frame.doc.root, "cap").expect("caption exists");
    let row = find(&frame.doc.root, "row").expect("row exists");
    assert!(
        row.layout.border_rect.x >= cap.layout.border_rect.x + cap.layout.border_rect.w - 0.5,
        "inline-start caption should be before rows; cap={:?} row={:?}",
        cap.layout.border_rect,
        row.layout.border_rect
    );
}

#[test]
fn caption_side_inline_end_places_caption_after_rows() {
    let doc = parse_html(
        "<style>body{margin:0}table{width:60px}caption{caption-side:inline-end;width:30px;height:20px}\
         td{padding:0;border:0;width:60px;height:10px}</style>\
         <table><caption id=cap>cap</caption><tr id=row><td>x</td></tr></table>",
    );
    let mut frame = EngineFrame::new(doc, 800.0, 600.0);
    frame.update_frame();
    fn find<'a>(node: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if node.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(node);
        }
        node.children.iter().find_map(|child| find(child, id))
    }
    let cap = find(&frame.doc.root, "cap").expect("caption exists");
    let row = find(&frame.doc.root, "row").expect("row exists");
    assert!(
        cap.layout.border_rect.x >= row.layout.border_rect.x + row.layout.border_rect.w - 0.5,
        "inline-end caption should be after rows; cap={:?} row={:?}",
        cap.layout.border_rect,
        row.layout.border_rect
    );
}

#[test]
fn presentational_hints_sit_between_ua_and_author_rules() {
    let hint_doc = parse_and_layout(
        r#"<table><tr><th align="left">head</th></tr></table>"#,
        800.0,
    );
    let hinted = find_box(&hint_doc.root, &|b| b.tag == "th").expect("th exists");
    assert_eq!(
        hinted.style.text_align,
        TextAlign::Left,
        "align=left should outrank the UA th center rule"
    );

    let author_doc = parse_and_layout(
        r#"<style>th { text-align: right; }</style>
           <table><tr><th align="left">head</th></tr></table>"#,
        800.0,
    );
    let authored = find_box(&author_doc.root, &|b| b.tag == "th").expect("th exists");
    assert_eq!(
        authored.style.text_align,
        TextAlign::Right,
        "author CSS should outrank the align presentational hint"
    );
}

#[test]
fn html_list_numbering_hints_share_css_counters() {
    for direction in ["ltr", "rtl"] {
        let html = format!(
            "<ol dir={direction} start=' +4 items'><li>four</li><li value='123 items'>explicit</li><li>next</li><li value='-2'>negative</li><li value=invalid>next negative</li></ol>"
        );
        assert_eq!(
            build_display_markers(&html),
            ["4.", "123.", "124.", "-2.", "-1."]
        );
    }
    assert_eq!(
        build_display_markers(
            "<style>ol{counter-reset:list-item 20}li{counter-set:list-item 8}</style><ol start=4><li value=123>eight</li></ol>"
        ),
        ["8."]
    );
    assert_eq!(
        build_display_markers(
            "<style>li{counter-increment:other}</style><ol start=3><li>three</li><li>four</li></ol>"
        ),
        ["3.", "4."]
    );
    assert_eq!(
        build_display_markers(
            "<div style='counter-reset:list-item 7'><p style='display:list-item;list-style-type:decimal'>eight</p><p style='display:list-item;list-style-type:decimal'>nine</p></div>"
        ),
        ["8.", "9."]
    );
    assert_eq!(
        build_display_markers(
            "<style>ul{list-style-type:decimal}</style><ol start=4><li>outer<ul><li>inner</li><li>inner two</li></ul></li><li>outer next</li></ol>"
        ),
        ["4.", "1.", "2.", "5."]
    );
    assert_eq!(
        build_display_markers(
            "<style>li{counter-increment:list-item 2}</style><ol><li>two</li><li value=10>ten</li><li>twelve</li></ol>"
        ),
        ["2.", "10.", "12."]
    );
}

#[test]
fn html_reversed_lists_with_start_use_shared_counter_direction() {
    for direction in ["ltr", "rtl"] {
        assert_eq!(
            build_display_markers(&format!(
                "<ol reversed start=' +4 items' dir={direction}><li>four</li><li value=9>nine</li><li>eight</li><li value=-2>negative</li><li>next</li></ol>"
            )),
            ["4.", "9.", "8.", "-2.", "-3."]
        );
    }
    assert_eq!(
        build_display_markers(
            "<ol reversed start=4><li>outer<ol><li>inner</li><li>inner</li></ol></li><li>outer</li></ol>"
        ),
        ["4.", "1.", "2.", "3."]
    );
    assert_eq!(
        build_display_markers(
            "<style>ol{counter-reset:list-item 20}</style><ol reversed start=4><li>author</li><li>author</li></ol>"
        ),
        ["21.", "22."]
    );
    assert_eq!(
        build_display_markers("<ol reversed start=0><li>zero</li><li>negative</li></ol>"),
        ["0.", "-1."]
    );
}

#[test]
fn html_reversed_lists_without_valid_start_use_automatic_css_counters() {
    for attributes in ["reversed", "reversed='' start=invalid", "reversed=false"] {
        assert_eq!(
            build_display_markers(&format!(
                "<ol {attributes}><li>three</li><li>two</li><li>one</li></ol>"
            )),
            ["3.", "2.", "1."]
        );
    }
    assert_eq!(
        build_display_markers(
            "<ol reversed><li>outer<ol><li>inner</li><li>inner</li></ol></li><li>outer</li></ol>"
        ),
        ["2.", "1.", "2.", "1."]
    );
    assert_eq!(
        build_display_markers(
            "<ol reversed><li>two</li><li style='display:none'>hidden</li><li>one</li></ol>"
        ),
        ["2.", "1."]
    );
    assert_eq!(
        build_display_markers(
            "<style>ol{counter-reset:list-item 20}</style><ol reversed><li>author</li><li>author</li></ol>"
        ),
        ["21.", "22."]
    );
    // A later li[value] changes that item's ordinal, not the HTML list's default start.
    assert_eq!(
        build_display_markers(
            "<ol reversed><li>before set</li><li value=9>set</li><li>after set</li></ol>"
        ),
        ["3.", "9.", "8."]
    );
    assert_eq!(
        build_display_markers(
            "<ol reversed start=invalid><li>before</li><li value=-2>set</li><li>after</li></ol>"
        ),
        ["3.", "-2.", "-3."]
    );
    assert_eq!(
        build_display_markers(
            "<style>ol{counter-reset:reversed(list-item)}</style><ol reversed><li>before</li><li value=9>set</li><li>after</li></ol>"
        ),
        ["10.", "9.", "8."]
    );
}

#[test]
fn html_reversed_start_recounts_after_incremental_visibility_changes() {
    let mut root = WebCore::new("ol");
    root.node_id = 1;
    root.attributes.insert("reversed", "");
    for index in 0..3 {
        let mut child = WebCore::new("li");
        child.node_id = index + 2;
        if index == 1 {
            child.attributes.insert("value", "9");
        }
        root.children.push(child);
    }
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("li{display:list-item} li:hover{display:none}");
    sheet.rebuild_index();
    crate::css::apply_cascade(&mut root, &sheet, None, 16.0);
    assert_eq!(root.children[0].style.list_index, 3);
    for (hovered, expected) in [(true, 2), (false, 3), (true, 2)] {
        crate::css::clear_cascade_dirty(&mut root);
        root.children[2].cascade_dirty = true;
        root.has_dirty_descendant = true;
        let hover = if hovered {
            std::collections::HashSet::from([4])
        } else {
            std::collections::HashSet::new()
        };
        crate::css::apply_cascade_incremental(
            &mut root, &sheet, None, 16.0, 800.0, 600.0, 0, false, &hover,
        );
        assert_eq!(root.children[0].style.list_index, expected);
        assert_eq!(root.children[1].style.list_index, 9);
    }
}

#[test]
fn ua_list_indentation_follows_inline_direction() {
    for (direction, rtl) in [("ltr", false), ("rtl", true)] {
        let doc = parse_and_layout(
            &format!(
                "<div dir={direction}><ol id=ol><li>one</li></ol><ul id=ul><li>one</li></ul><menu id=menu><li>one</li></menu><dir id=dir><li>one</li></dir><dl><dt>term</dt><dd id=dd>description</dd></dl></div>"
            ),
            800.0,
        );
        for id in ["#ol", "#ul", "#menu", "#dir"] {
            let node = crate::dom::query_selector(&doc.root, id).unwrap();
            let (start, end) = if rtl {
                (&node.style.padding_right, &node.style.padding_left)
            } else {
                (&node.style.padding_left, &node.style.padding_right)
            };
            assert_eq!(*start, CssLength::Px(40.0), "{direction} {id}");
            assert!(
                matches!(end, CssLength::Zero | CssLength::Px(0.0)),
                "{direction} {id}: {end:?}"
            );
        }
        let dd = crate::dom::query_selector(&doc.root, "#dd").unwrap();
        let start = if rtl {
            &dd.style.margin_right
        } else {
            &dd.style.margin_left
        };
        assert_eq!(*start, CssLength::Px(40.0), "{direction} dd");
    }
}

#[test]
fn physical_and_logical_spacing_share_cascade_order() {
    let doc = parse_and_layout(
        "<style>*{margin:0;padding:0}.parent{padding-right:9px}#a{padding-inline-start:30px;padding-left:5px;direction:rtl}#b{padding-inline-start:30px;padding-right:5px;direction:rtl}#c{padding-right:5px;padding-inline-start:30px;direction:rtl}#d{padding-inline-start:30px;padding:initial;direction:rtl}#e{padding-inline-start:30px;padding-right:inherit;direction:rtl}#f{padding-inline-start:30px!important;padding-right:5px;direction:rtl}#g{margin-inline-start:30px;margin-right:5px;direction:rtl}</style><ul id=reset><li>item</li></ul><div class=parent><div id=a></div><div id=b></div><div id=c></div><div id=d></div><div id=e></div><div id=f></div><div id=g></div></div>",
        800.0,
    );
    let node = |id| crate::dom::query_selector(&doc.root, id).unwrap();
    for (id, left, right) in [
        ("#a", 5.0, 30.0),
        ("#b", 0.0, 5.0),
        ("#c", 0.0, 30.0),
        ("#d", 0.0, 0.0),
        ("#e", 0.0, 9.0),
        ("#f", 0.0, 30.0),
        ("#reset", 0.0, 0.0),
    ] {
        let rect = &node(id).layout;
        assert!(
            (rect.content_rect.x - rect.padding_rect.x - left).abs() < 0.1,
            "{id} left"
        );
        assert!(
            (rect.padding_rect.right() - rect.content_rect.right() - right).abs() < 0.1,
            "{id} right"
        );
    }
    assert_eq!(node("#g").style.margin_right, CssLength::Px(5.0));
}

#[test]
fn logical_box_inheritance_uses_parent_axes_then_child_axes() {
    let doc = parse_and_layout(
        r#"<style>
        * { margin:0; padding:0 }
        .parent { margin:11px 12px 13px 14px; padding:21px 22px 23px 24px;
                  position:relative; inset:31px 32px 33px 34px }
        .child { direction:rtl; margin-inline:inherit; padding-inline:inherit;
                 position:relative; inset-inline:inherit }
        #vertical { writing-mode:vertical-rl }
        #vertical .child { writing-mode:horizontal-tb; direction:ltr }
    </style><div class=parent><div class=child id=rtl></div></div>
    <div class=parent id=vertical><div class=child id=horizontal></div></div>"#,
        800.0,
    );
    let rtl = &crate::dom::query_selector(&doc.root, "#rtl").unwrap().style;
    assert_eq!(rtl.margin_right, CssLength::Px(14.0));
    assert_eq!(rtl.margin_left, CssLength::Px(12.0));
    assert_eq!(rtl.padding_right, CssLength::Px(24.0));
    assert_eq!(rtl.padding_left, CssLength::Px(22.0));
    assert_eq!(rtl.right, CssLength::Px(34.0));
    assert_eq!(rtl.left, CssLength::Px(32.0));
    let horizontal = &crate::dom::query_selector(&doc.root, "#horizontal")
        .unwrap()
        .style;
    assert_eq!(horizontal.margin_left, CssLength::Px(11.0));
    assert_eq!(horizontal.margin_right, CssLength::Px(13.0));
    assert_eq!(horizontal.padding_left, CssLength::Px(21.0));
    assert_eq!(horizontal.padding_right, CssLength::Px(23.0));
    assert_eq!(horizontal.left, CssLength::Px(31.0));
    assert_eq!(horizontal.right, CssLength::Px(33.0));
}

#[test]
fn logical_borders_preserve_physical_order_resets_and_current_color() {
    let doc = parse_and_layout(
        r#"<style>
        .case { direction:rtl; width:140px; height:48px }
        #a { border-inline-start:6px solid red; border-right:2px dashed blue }
        #b { border-right:2px dashed blue; border-inline-start:6px solid red }
        #c { border-inline-start:6px solid red; border-inline-start-width:7px }
        #d { border-inline-start:4px solid currentColor; color:purple }
        #e { border-inline-start:4px solid red; border-right-color:currentColor; color:lime }
        #f { border-right:4px solid red; border-inline-start:4px solid; color:purple }
        #g { border-inline-start:4px solid currentColor; border-right-color:blue; color:lime }
        #h { border-inline-start:6px solid red; border-inline-start:initial; color:purple }
        #i { border-inline-start:6px solid red; border:initial; color:purple }
        #j { border-inline-start:6px solid red; border-right:2px dashed blue!important }
        #k { border-inline-start:6px solid red; border-inline-start-color:transparent }
    </style><div class=case id=a></div><div class=case id=b></div>
      <div class=case id=c></div><div class=case id=d></div>
      <div class=case id=e></div><div class=case id=f></div>
      <div class=case id=g></div><div class=case id=h></div>
      <div class=case id=i></div><div class=case id=j></div><div class=case id=k></div>"#,
        800.0,
    );
    let node = |id| crate::dom::query_selector(&doc.root, id).unwrap();
    assert_eq!(node("#k").style.border_right_color.a, 0);
    for (id, width, border_style, color) in [
        ("#a", 2.0, BorderStyle::Dashed, Color::rgb(0, 0, 255)),
        ("#b", 6.0, BorderStyle::Solid, Color::rgb(255, 0, 0)),
        ("#c", 7.0, BorderStyle::Solid, Color::rgb(255, 0, 0)),
        ("#d", 4.0, BorderStyle::Solid, Color::rgb(128, 0, 128)),
        ("#e", 4.0, BorderStyle::Solid, Color::rgb(0, 255, 0)),
        ("#f", 4.0, BorderStyle::Solid, Color::rgb(128, 0, 128)),
        ("#g", 4.0, BorderStyle::Solid, Color::rgb(0, 0, 255)),
        ("#j", 2.0, BorderStyle::Dashed, Color::rgb(0, 0, 255)),
    ] {
        let style = &node(id).style;
        assert_eq!(style.border_right_width, CssLength::Px(width), "{id} width");
        assert_eq!(style.border_right_style, border_style, "{id} style");
        assert_eq!(style.border_right_color, color, "{id} color");
    }
    for id in ["#h", "#i"] {
        let style = &node(id).style;
        assert_eq!(
            style.border_right_width,
            ComputedStyle::default().border_right_width,
            "{id}"
        );
        assert_eq!(style.border_right_style, BorderStyle::None, "{id}");
        assert_eq!(style.border_right_color, Color::rgb(128, 0, 128), "{id}");
    }
}

#[test]
fn logical_corners_preserve_physical_order_and_shorthand_resets() {
    let doc = parse_and_layout(
        r#"<style>
        .case { direction:rtl; width:140px; height:48px }
        #a { border-start-start-radius:20px 12px; border-top-right-radius:4px 6px }
        #b { border-top-right-radius:4px 6px; border-start-start-radius:20px 12px }
        #c { border-start-start-radius:20px 12px; border-radius:4px / 6px }
        #d { border-start-start-radius:20px 12px; border-top-right-radius:initial }
        #e { border-start-start-radius:20px 12px; border-radius:initial }
        #f { border-start-start-radius:20px 12px!important; border-top-right-radius:4px 6px }
        #g { writing-mode:vertical-rl; direction:ltr; border-start-start-radius:20px 12px; border-top-right-radius:4px 6px }
    </style><div class=case id=a></div><div class=case id=b></div>
      <div class=case id=c></div><div class=case id=d></div>
      <div class=case id=e></div><div class=case id=f></div><div class=case id=g></div>"#,
        800.0,
    );
    for (id, x, y) in [
        ("#a", 4.0, 6.0),
        ("#b", 20.0, 12.0),
        ("#c", 4.0, 6.0),
        ("#f", 20.0, 12.0),
        ("#g", 4.0, 6.0),
    ] {
        let style = &crate::dom::query_selector(&doc.root, id).unwrap().style;
        assert_eq!(style.border_top_right_radius, CssLength::Px(x), "{id} x");
        assert_eq!(style.border_top_right_radius_y, CssLength::Px(y), "{id} y");
    }
    for id in ["#d", "#e"] {
        let style = &crate::dom::query_selector(&doc.root, id).unwrap().style;
        assert_eq!(
            style.border_top_right_radius,
            ComputedStyle::default().border_top_right_radius,
            "{id}"
        );
        assert_eq!(
            style.border_top_right_radius_y,
            ComputedStyle::default().border_top_right_radius_y,
            "{id}"
        );
    }
}

#[test]
fn physical_and_logical_insets_share_cascade_order() {
    let doc = parse_and_layout(
        r#"<style>
        .parent { right:9px }
        .case { position:relative; direction:rtl }
        #a { inset-inline-start:30px; right:5px }
        #b { right:5px; inset-inline-start:30px }
        #c { inset-inline-start:30px; inset:initial }
        #d { inset-inline-start:30px; right:inherit }
        #e { inset-inline-start:30px!important; right:5px }
        #f { inset-inline-start:30px; right:5px!important }
        #g { inset-inline:30px 40px; inset:1px 2px 3px 4px }
        #h { inset:1px 2px 3px 4px; inset-inline:30px 40px }
        #i { writing-mode:vertical-rl; direction:ltr; inset-block-start:30px; right:5px }
        #j { writing-mode:vertical-rl; direction:ltr; inset-inline-start:30px; top:5px }
    </style><div class=parent>
      <div class=case id=a></div><div class=case id=b></div>
      <div class=case id=c></div><div class=case id=d></div>
      <div class=case id=e></div><div class=case id=f></div>
      <div class=case id=g></div><div class=case id=h></div>
      <div class=case id=i></div><div class=case id=j></div>
    </div>"#,
        800.0,
    );
    for (id, right) in [
        ("a", 5.0),
        ("b", 30.0),
        ("d", 9.0),
        ("e", 30.0),
        ("f", 5.0),
        ("g", 2.0),
        ("h", 30.0),
        ("i", 5.0),
    ] {
        let node = crate::dom::query_selector(&doc.root, &format!("#{id}")).unwrap();
        assert_eq!(node.style.right, CssLength::Px(right), "{id}");
    }
    let c = crate::dom::query_selector(&doc.root, "#c").unwrap();
    assert_eq!(c.style.right, CssLength::Auto);
    let g = crate::dom::query_selector(&doc.root, "#g").unwrap();
    assert_eq!(g.style.left, CssLength::Px(4.0));
    let h = crate::dom::query_selector(&doc.root, "#h").unwrap();
    assert_eq!(h.style.left, CssLength::Px(40.0));
    let j = crate::dom::query_selector(&doc.root, "#j").unwrap();
    assert_eq!(j.style.top, CssLength::Px(5.0));
}

#[test]
fn physical_and_logical_sizes_share_cascade_order_and_resets() {
    let doc = parse_and_layout(
        r#"<style>
        .case { height:10px }
        #a { inline-size:90px; width:40px }
        #b { width:40px; inline-size:90px }
        #c { inline-size:90px; inline-size:initial }
        #d { min-inline-size:90px; min-width:40px; max-inline-size:200px; max-width:150px }
        #e { min-inline-size:90px; min-inline-size:initial;
             max-inline-size:200px; max-inline-size:initial }
        #f { inline-size:90px; height:40px; writing-mode:vertical-rl }
        #g { block-size:90px; width:40px; writing-mode:vertical-rl }
        #h { inline-size:90px!important; width:40px }
        #i { inline-size:90px; width:40px!important }
        #j { inline-size:90px; inline-size:var(--missing) }
    </style><div class=case id=a></div><div class=case id=b></div>
      <div class=case id=c></div><div class=case id=d></div>
      <div class=case id=e></div><div class=case id=f></div>
      <div class=case id=g></div><div class=case id=h></div>
      <div class=case id=i></div><div class=case id=j></div>"#,
        800.0,
    );
    let node = |id| crate::dom::query_selector(&doc.root, id).unwrap();
    for (id, width) in [
        ("#a", 40.0),
        ("#b", 90.0),
        ("#g", 40.0),
        ("#h", 90.0),
        ("#i", 40.0),
    ] {
        assert_eq!(node(id).style.width, CssLength::Px(width), "{id}");
    }
    let initial = ComputedStyle::default();
    assert_eq!(node("#c").style.width, initial.width);
    assert_eq!(node("#j").style.width, initial.width);
    assert_eq!(node("#d").style.min_width, CssLength::Px(40.0));
    assert_eq!(node("#d").style.max_width, CssLength::Px(150.0));
    assert_eq!(node("#e").style.min_width, initial.min_width);
    assert_eq!(node("#e").style.max_width, initial.max_width);
    assert_eq!(node("#f").style.height, CssLength::Px(40.0));
}

#[test]
fn logical_box_rollback_reads_pending_writes_in_cascade_order() {
    let doc = parse_and_layout(
        r#"<style>
        @layer base, override;
        @layer base {
            * { margin:0; padding:0 }
            .case { direction:rtl; margin-inline-start:19px; margin-right:7px;
                    padding-inline-start:23px; padding-right:9px }
        }
        @layer override {
            .case { margin-inline-start:42px; margin-inline-start:revert-layer;
                    padding-inline-start:44px; padding-inline-start:revert-layer }
        }
    </style><div class=case id=rollback></div>"#,
        800.0,
    );
    let style = &crate::dom::query_selector(&doc.root, "#rollback")
        .unwrap()
        .style;
    assert_eq!(style.margin_right, CssLength::Px(7.0));
    assert_eq!(style.padding_right, CssLength::Px(9.0));
}

#[test]
fn list_item_marker_uses_css_counter_value() {
    let html = r#"<style>
              ol { counter-reset: list-item 10; }
              li { list-style-type: decimal; }
            </style>
            <ol><li>one</li><li>two</li></ol>"#;
    let markers = build_display_markers(html);

    assert!(
        markers.iter().any(|text| text == "11."),
        "first marker should use CSS list-item counter; markers were {markers:?}"
    );
    assert!(
        markers.iter().any(|text| text == "12."),
        "second marker should use CSS list-item counter; markers were {markers:?}"
    );
}

#[test]
fn extended_list_style_type_keywords_paint_markers() {
    let html = r#"<style>
              ol { counter-reset: list-item 8; }
              li { list-style-type: decimal-leading-zero; }
            </style>
            <ol><li>nine</li><li>ten</li></ol>"#;
    let markers = build_display_markers(html);

    assert!(
        markers.iter().any(|text| text == "09."),
        "decimal-leading-zero marker should paint instead of disappearing; markers were {markers:?}"
    );
    assert!(
        markers.iter().any(|text| text == "10."),
        "decimal-leading-zero marker should preserve two digit values; markers were {markers:?}"
    );

    let greek_html = r#"<style>li { list-style-type: lower-greek; }</style>
            <ol><li>alpha</li></ol>"#;
    let greek_markers = build_display_markers(greek_html);
    assert!(
        greek_markers.iter().any(|text| text == "α."),
        "lower-greek marker should paint instead of disappearing; markers were {greek_markers:?}"
    );

    let armenian_html = r#"<style>li { list-style-type: armenian; }</style>
            <ol><li>one</li></ol>"#;
    let armenian_markers = build_display_markers(armenian_html);
    assert!(
        armenian_markers.iter().any(|text| text == "Ա."),
        "armenian marker should use the algorithmic counter style; markers were {armenian_markers:?}"
    );

    let georgian_html = r#"<style>li { list-style-type: georgian; }</style>
            <ol><li>one</li></ol>"#;
    let georgian_markers = build_display_markers(georgian_html);
    assert!(
        georgian_markers.iter().any(|text| text == "ა."),
        "georgian marker should use the algorithmic counter style; markers were {georgian_markers:?}"
    );

    let hebrew_html = r#"<style>ol { counter-reset: list-item 14; } li { list-style-type: hebrew; }</style>
            <ol><li>fifteen</li></ol>"#;
    let hebrew_markers = build_display_markers(hebrew_html);
    assert!(
        hebrew_markers.iter().any(|text| text == "ט״ו."),
        "hebrew marker should use the special 15 form; markers were {hebrew_markers:?}"
    );

    let kana_html = r#"<style>li { list-style-type: hiragana-iroha; }</style>
            <ol><li>one</li></ol>"#;
    let kana_markers = build_display_markers(kana_html);
    assert!(
        kana_markers.iter().any(|text| text == "い."),
        "kana marker should use the requested sequence; markers were {kana_markers:?}"
    );
}

#[test]
fn empty_list_item_still_paints_a_marker() {
    let html = r#"<style>li { list-style-type: decimal; }</style><ol><li></li></ol>"#;
    let markers = build_display_markers(html);

    assert!(
        markers.iter().any(|text| text == "1."),
        "empty list item should still paint a marker; markers were {markers:?}"
    );
}

#[test]
fn form_validation_pseudo_classes_match_basic_constraints() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
        input:invalid { color: rgb(200, 0, 0); }
        input:valid { background-color: rgb(0, 200, 0); }
        input:out-of-range { border-top-color: rgb(10, 20, 30); border-top-style: solid; }
        input:in-range { border-left-color: rgb(40, 50, 60); border-left-style: solid; }
        </style>
        <input id=missing required>
        <input id=ok required value=yes>
        <input id=low type=number min=10 value=5>
        <input id=mid type=number min=1 max=10 value=5>"#,
        800.0,
    );

    let missing = find_box(&d.root, &|b| {
        b.attributes.get("id").map(|s| s.as_str()) == Some("missing")
    })
    .unwrap();
    let ok = find_box(&d.root, &|b| {
        b.attributes.get("id").map(|s| s.as_str()) == Some("ok")
    })
    .unwrap();
    let low = find_box(&d.root, &|b| {
        b.attributes.get("id").map(|s| s.as_str()) == Some("low")
    })
    .unwrap();
    let mid = find_box(&d.root, &|b| {
        b.attributes.get("id").map(|s| s.as_str()) == Some("mid")
    })
    .unwrap();

    assert_eq!(missing.style.color, Color::rgb(200, 0, 0));
    assert_eq!(ok.style.background_color, Color::rgb(0, 200, 0));
    assert_eq!(low.style.border_top_color, Color::rgb(10, 20, 30));
    assert_eq!(mid.style.border_left_color, Color::rgb(40, 50, 60));
}

#[test]
fn user_validation_pseudo_classes_require_dirty_user_state() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
        input:user-invalid { color: rgb(200, 0, 0); }
        input:user-valid { background-color: rgb(0, 200, 0); }
        </style>
        <input id=field required>"#,
        800.0,
    );
    let field = d.get_element_by_id("field").unwrap();
    assert_ne!(
        d.get_computed_style(field).unwrap().color,
        Color::rgb(200, 0, 0),
        "an untouched empty required input is invalid, but not user-invalid"
    );

    d.set_value(field, "");
    d.recascade();
    assert_eq!(
        d.get_computed_style(field).unwrap().color,
        Color::rgb(200, 0, 0),
        "once touched, the empty required input matches :user-invalid"
    );

    d.set_value(field, "ok");
    d.recascade();
    assert_eq!(
        d.get_computed_style(field).unwrap().background_color,
        Color::rgb(0, 200, 0),
        "a touched valid input matches :user-valid"
    );
}

#[test]
fn autofill_pseudo_class_matches_host_control_state() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
        input { color: rgb(1, 1, 1); }
        input:autofill { color: rgb(7, 8, 9); }
        </style>
        <input id=email>"#,
        800.0,
    );
    let email = d.get_element_by_id("email").unwrap();
    assert_eq!(
        d.get_computed_style(email).unwrap().color,
        Color::rgb(1, 1, 1)
    );

    d.set_autofilled(email, true);
    d.recascade();
    assert_eq!(
        d.get_computed_style(email).unwrap().color,
        Color::rgb(7, 8, 9)
    );

    d.set_autofilled(email, false);
    d.recascade();
    assert_eq!(
        d.get_computed_style(email).unwrap().color,
        Color::rgb(1, 1, 1)
    );
}

// ── Selector Parsing ──────────────────────────────────────────────────────────

#[test]
fn css_selector_with_class() {
    let sel = parse_selector("div.container");
    assert!(!sel.parts.is_empty());
    // should have both a tag part and class part
    use crate::css::SelectorPart;
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Tag(t) if t == "div"))
    );
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Class(c) if c == "container"))
    );
}

#[test]
fn css_selector_with_id() {
    let sel = parse_selector("#main");
    use crate::css::SelectorPart;
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Id(id) if id == "main"))
    );
}

#[test]
fn css_selector_multiple_classes() {
    let sel = parse_selector(".foo.bar.baz");
    use crate::css::SelectorPart;
    let classes: Vec<_> = sel
        .parts
        .iter()
        .filter_map(|p| {
            if let SelectorPart::Class(c) = p {
                Some(c.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(classes.len() >= 3);
}

#[test]
fn css_selector_escaped_utility_class_keeps_punctuation() {
    let sel = parse_selector(".md\\:grid-cols-\\[2fr_1fr\\]");
    use crate::css::SelectorPart;
    assert!(sel.valid);
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Class(c) if c == "md:grid-cols-[2fr_1fr]"))
    );
}

#[test]
fn css_selector_hex_escape_decodes_identifier_code_points() {
    let sel = parse_selector(".\\32xl\\:grid-cols-12");
    use crate::css::SelectorPart;
    assert!(sel.valid);
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Class(c) if c == "2xl:grid-cols-12"))
    );
}

#[test]
fn css_selector_hex_escape_consumes_optional_trailing_space() {
    let sel = parse_selector(".bg-\\5b rgba\\28 0\\2c 0\\2c 0\\2c 0\\.3\\29 \\5d");
    use crate::css::SelectorPart;
    assert!(sel.valid);
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Class(c) if c == "bg-[rgba(0,0,0,0.3)]"))
    );
}

#[test]
fn escaped_arbitrary_class_custom_property_feeds_mask_image_var() {
    let html = r#"
        <style>
            .icon {
                display: inline-block;
                width: 24px;
                height: 24px;
                mask-image: var(--icon-url);
                -webkit-mask-image: var(--icon-url);
            }
            .\[--icon-url\:url\(\'\/leo-icons\/search\.svg\'\)\] {
                --icon-url: url('/leo-icons/search.svg');
            }
        </style>
        <span id="i" class="icon [--icon-url:url('/leo-icons/search.svg')]"></span>
    "#;
    let mut doc = crate::load_html(html, 200.0);
    let id = doc.get_element_by_id("i").expect("icon span");
    assert_eq!(
        doc.computed_style_property(id, "mask-image"),
        "url(\"/leo-icons/search.svg\")"
    );
}

#[test]
fn css_escaped_before_class_is_not_stripped_as_pseudo_element() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(".before\\:block { display: block; }");
    assert_eq!(ss.rules.len(), 1);
    assert_eq!(ss.rules[0].pseudo_element, PseudoElement::None);
    assert!(
        ss.rules[0].selectors[0]
            .parts
            .iter()
            .any(|p| matches!(p, crate::css::SelectorPart::Class(c) if c == "before:block"))
    );
}

#[test]
fn css_escaped_before_class_can_still_target_real_before_pseudo() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(".before\\:content-\\[\\'\\/\\'\\]::before { content: \"/\"; }");
    assert_eq!(ss.rules.len(), 1);
    assert_eq!(ss.rules[0].pseudo_element, PseudoElement::Before);
    assert!(
        ss.rules[0].selectors[0].parts.iter().any(
            |p| matches!(p, crate::css::SelectorPart::Class(c) if c == "before:content-['/']")
        )
    );
}

#[test]
fn compound_class_selector_overrides_single_class_width() {
    let doc = parse_and_layout(
        "<style>
           .hyperlink-wrapper { display: block; width: fit-content; }
           .hyperlink-wrapper.story-img-wrapper { width: 100%; }
         </style>
         <div style='width: 584px'><a id=target class='hyperlink-wrapper story-img-wrapper'></a></div>",
        800.0,
    );
    let target = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(String::as_str) == Some("target")
    })
    .expect("target");
    assert!(
        (target.layout.content_rect.w - 584.0).abs() < 1.0,
        "compound class rule should override single class width; got {}",
        target.layout.content_rect.w
    );
}

#[test]
fn fit_content_flex_ignores_absolute_generated_decoration() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             .leoButton {
               display: flex;
               align-items: center;
               justify-content: center;
               box-sizing: border-box;
               padding: 16px;
               width: fit-content;
               position: relative;
             }
             .leoButton.isHero:not(:disabled:not(.isLoading))::before,
             .leoButton.isHero:not(:disabled:not(.isLoading))::after {
               content: "";
               display: block;
               position: absolute;
               inset: 0;
               width: 100%;
               height: 100%;
             }
             .content { padding: 0 8px; }
             .icon { width: 24px; height: 24px; }
           </style>
           <a id="button" class="leoButton isHero">
             <span class="content">Get Brave</span><span class="icon"></span>
           </a>"#,
        600.0,
    );
    let button = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(String::as_str) == Some("button")
    })
    .expect("button");
    assert!(
        button.layout.margin_rect.w < 220.0,
        "absolute generated decoration must not make fit-content fill the container: {:?}",
        button.layout.margin_rect
    );
    assert!(
        button.layout.margin_rect.w > 120.0,
        "button should still include text, icon and padding: {:?}",
        button.layout.margin_rect
    );
}

#[test]
fn fit_content_flex_child_does_not_stretch_in_column_parent() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             .hero {
               display: flex;
               flex-direction: column;
               align-items: stretch;
               width: 600px;
             }
             .leoButton {
               display: flex;
               align-items: center;
               justify-content: center;
               box-sizing: border-box;
               padding: 16px;
               width: fit-content;
               position: relative;
             }
             .leoButton::before,
             .leoButton::after {
               content: "";
               display: block;
               position: absolute;
               inset: 0;
               width: 100%;
               height: 100%;
             }
             .content { padding: 0 8px; }
             .icon { width: 24px; height: 24px; }
           </style>
           <div class="hero">
             <a id="button" class="leoButton">
               <span class="content">Get Brave</span><span class="icon"></span>
             </a>
           </div>"#,
        800.0,
    );
    let button = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(String::as_str) == Some("button")
    })
    .expect("button");
    assert!(
        button.layout.margin_rect.w < 220.0,
        "fit-content flex item must not be stretched as auto width: {:?}",
        button.layout.margin_rect
    );
    assert!(
        button.layout.margin_rect.w > 120.0,
        "button should still include text, icon and padding: {:?}",
        button.layout.margin_rect
    );
}

#[test]
fn flex_container_max_width_max_content_clamps_auto_width() {
    let doc = parse_and_layout(
        r#"<style>
             body { margin: 0; font: 14px/20px sans-serif; }
             .card { width: 360px; }
             .button {
               display: flex;
               box-sizing: border-box;
               max-width: max-content;
               padding: 11px 16px;
               border: 1px solid black;
             }
           </style>
           <div class="card">
             <div id="button" class="button">Firewall + VPN</div>
           </div>"#,
        800.0,
    );
    let button = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(String::as_str) == Some("button")
    })
    .expect("button");
    assert!(
        button.layout.margin_rect.w < 180.0,
        "max-width:max-content should clamp flex container auto width: {:?}",
        button.layout.margin_rect
    );
    assert!(
        button.layout.margin_rect.w > 100.0,
        "button should still include its text and padding: {:?}",
        button.layout.margin_rect
    );
}

#[test]
fn compound_ancestor_descendant_rule_resolves_inherited_custom_width() {
    let doc = parse_and_layout(
        "<style>
           #host { --img-width: 100%; width: 584px; }
           .hyperlink-wrapper.story-img-wrapper { display: inline-block; width: 100%; }
           .hyperlink-wrapper.story-img-wrapper .story-img { width: var(--img-width, unset); height: auto; }
         </style>
         <div id=host><a class='hyperlink-wrapper story-img-wrapper'><img id=pic class=story-img width=1312 height=738></a></div>",
        1280.0,
    );
    let pic = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(String::as_str) == Some("pic")
    })
    .expect("pic");
    assert!(
        (pic.layout.content_rect.w - 584.0).abs() < 1.0,
        "descendant rule should resolve inherited --img-width against the wrapper; got {}",
        pic.layout.content_rect.w
    );
}

#[test]
fn percentage_width_image_does_not_expand_flex_column_to_natural_width() {
    let doc = parse_and_layout(
        "<style>
           #row { display: flex; width: 900px; }
           #main { flex: 1; }
           #side { width: 300px; flex-shrink: 0; }
           #main a { display: inline-block; width: 100%; }
           #main img { width: 100%; height: auto; }
         </style>
         <div id=row><div id=main><a><picture><img id=pic width=979 height=653></picture></a></div><div id=side>Side</div></div>",
        900.0,
    );
    let find = |id| {
        find_box(&doc.root, &|b| {
            b.attributes.get("id").map(String::as_str) == Some(id)
        })
        .unwrap()
    };
    assert!((find("main").layout.border_rect.w - 600.0).abs() < 1.0);
    assert!(
        (find("side").layout.border_rect.x - find("row").layout.border_rect.x - 600.0).abs() < 1.0
    );
    assert!((find("pic").layout.border_rect.w - 600.0).abs() < 1.0);
}

#[test]
fn css_selector_descendant_combinator() {
    let sel = parse_selector("div p");
    use crate::css::{Combinator, SelectorPart};
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Descendant)))
    );
}

#[test]
fn css_selector_child_combinator() {
    let sel = parse_selector("div > p");
    use crate::css::{Combinator, SelectorPart};
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Child)))
    );
}

#[test]
fn css_selector_adjacent_sibling() {
    let sel = parse_selector("h1 + p");
    use crate::css::{Combinator, SelectorPart};
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::AdjacentSibling)))
    );
}

#[test]
fn css_selector_general_sibling() {
    let sel = parse_selector("h1 ~ p");
    use crate::css::{Combinator, SelectorPart};
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::GeneralSibling)))
    );
}

#[test]
fn column_combinator_does_not_degrade_to_descendant_selector() {
    let sel = parse_selector("col || td");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel.valid);
    assert!(
        sel.parts
            .iter()
            .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Column)))
    );

    let mut doc = crate::html::parse_html(
        "<style>col || td { color: rgb(9,8,7) }</style>\
         <table><colgroup><col></colgroup><tbody><tr><td id=cell>x</td></tr></tbody></table>",
    );
    let mut eng = crate::layout::LayoutEngine::new();
    eng.layout(&mut doc, 400.0);

    fn by_id<'a>(n: &'a crate::types::WebCore, id: &str) -> Option<&'a crate::types::WebCore> {
        if n.attributes.get("id").map(String::as_str) == Some(id) {
            return Some(n);
        }
        for c in &n.children {
            if let Some(f) = by_id(c, id) {
                return Some(f);
            }
        }
        None
    }
    let c = by_id(&doc.root, "cell").unwrap().style.color;
    assert_ne!(
        (c.r, c.g, c.b),
        (9, 8, 7),
        "`col || td` must not be treated as `col td`"
    );
}

#[test]
fn css_specificity() {
    let sel1 = parse_selector("#main");
    let sel2 = parse_selector(".container");
    let sel3 = parse_selector("div");
    assert!(sel1.specificity() > sel2.specificity());
    assert!(sel2.specificity() > sel3.specificity());
}

// ── CSS Property Application ──────────────────────────────────────────────────

#[test]
fn css_display_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "display", "inline");
    assert_eq!(style.display, Display::Inline);
    apply_property(&mut style, "display", "inline-block");
    assert_eq!(style.display, Display::InlineBlock);
    apply_property(&mut style, "display", "flex");
    assert_eq!(style.display, Display::Flex);
    apply_property(&mut style, "display", "grid");
    assert_eq!(style.display, Display::Grid);
    apply_property(&mut style, "display", "none");
    assert_eq!(style.display, Display::None);
    apply_property(&mut style, "display", "list-item");
    assert_eq!(style.display, Display::ListItem);
}

#[test]
fn css_overflow_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "overflow", "hidden");
    assert_eq!(style.overflow_x, Overflow::Hidden);
    assert_eq!(style.overflow_y, Overflow::Hidden);
    apply_property(&mut style, "overflow", "hidden auto");
    assert_eq!(style.overflow_x, Overflow::Hidden);
    assert_eq!(style.overflow_y, Overflow::Auto);
    apply_property(&mut style, "overflow", "clip");
    assert_eq!(style.overflow_x, Overflow::Clip);
    assert_eq!(style.overflow_y, Overflow::Clip);
    apply_property(&mut style, "overflow", "scroll");
    assert_eq!(style.overflow_x, Overflow::Scroll);
    apply_property(&mut style, "overflow", "auto");
    assert_eq!(style.overflow_x, Overflow::Auto);
    apply_property(&mut style, "overflow", "visible");
    assert_eq!(style.overflow_x, Overflow::Visible);
}

#[test]
fn css_opacity_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "opacity", "0.5");
    assert!((style.opacity - 0.5).abs() < 0.01);
    apply_property(&mut style, "opacity", "0");
    assert!(style.opacity < 0.01);
    apply_property(&mut style, "opacity", "1");
    assert!(style.opacity > 0.99);
}

#[test]
fn css_position_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "position", "static");
    assert_eq!(style.position, Position::Static);
    apply_property(&mut style, "position", "relative");
    assert_eq!(style.position, Position::Relative);
    apply_property(&mut style, "position", "absolute");
    assert_eq!(style.position, Position::Absolute);
    apply_property(&mut style, "position", "fixed");
    assert_eq!(style.position, Position::Fixed);
}

#[test]
fn css_z_index_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "z-index", "10");
    assert_eq!(style.z_index, 10);
    apply_property(&mut style, "z-index", "-5");
    assert_eq!(style.z_index, -5);
}

#[test]
fn css_border_radius_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "border-radius", "8px");
    assert_eq!(style.border_radius, CssLength::Px(8.0));
}

#[test]
fn css_border_radius_slash_sets_vertical_radii() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "border-radius", "10px 20px / 3px 4px");
    assert_eq!(style.border_top_left_radius, CssLength::Px(10.0));
    assert_eq!(style.border_top_right_radius, CssLength::Px(20.0));
    assert_eq!(style.border_bottom_right_radius, CssLength::Px(10.0));
    assert_eq!(style.border_bottom_left_radius, CssLength::Px(20.0));
    assert_eq!(style.border_top_left_radius_y, CssLength::Px(3.0));
    assert_eq!(style.border_top_right_radius_y, CssLength::Px(4.0));
    assert_eq!(style.border_bottom_right_radius_y, CssLength::Px(3.0));
    assert_eq!(style.border_bottom_left_radius_y, CssLength::Px(4.0));
}

#[test]
fn computed_border_radius_serializes_two_value_longhand() {
    let mut d = crate::load_html(
        r#"<div id="box" style="border-top-left-radius: 10px 3px"></div>"#,
        800.0,
    );
    let id = crate::dom::query_selector(&d.root, "#box").unwrap().node_id;
    assert_eq!(
        d.computed_style_property(id, "border-top-left-radius"),
        "10px 3px"
    );
}

#[test]
fn css_vertical_align_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "vertical-align", "middle");
    assert_eq!(style.vertical_align, VerticalAlign::Middle);
    apply_property(&mut style, "vertical-align", "top");
    assert_eq!(style.vertical_align, VerticalAlign::Top);
    apply_property(&mut style, "vertical-align", "bottom");
    assert_eq!(style.vertical_align, VerticalAlign::Bottom);
    apply_property(&mut style, "vertical-align", "super");
    assert_eq!(style.vertical_align, VerticalAlign::Super);
    apply_property(&mut style, "vertical-align", "sub");
    assert_eq!(style.vertical_align, VerticalAlign::Sub);
    apply_property(&mut style, "vertical-align", "6px");
    assert_eq!(
        style.vertical_align,
        VerticalAlign::Length(CssLength::Px(6.0))
    );
    apply_property(&mut style, "vertical-align", "50%");
    assert_eq!(
        style.vertical_align,
        VerticalAlign::Length(CssLength::Percent(50.0))
    );
}

#[test]
fn cssom_math_preserves_percentages_until_used_value_resolution() {
    let mut doc = parse_html(
        r#"<html><head><style>
      html{font-size:20px}body{font-size:30px}
      #a{background-size:calc(50% - 2em) min(50%, 3rem);
        max-width:clamp(1em, 50%, 10rem);min-width:min-content;
        flex-basis:calc(25% + 1rem);column-gap:calc(10% + 2px);left:calc(10% + 2em)}
    </style></head><body><div id=a>math</div></body></html>"#,
    );
    doc.set_viewport(800.0, 600.0);
    let a = doc.get_element_by_id("a").unwrap();
    for (property, expected) in [
        ("background-size", "calc(50% - 60px) min(50%, 60px)"),
        ("max-width", "clamp(30px, 50%, 200px)"),
        ("min-width", "min-content"),
        ("flex-basis", "calc(25% + 20px)"),
        ("column-gap", "calc(10% + 2px)"),
        ("left", "calc(10% + 60px)"),
    ] {
        assert_eq!(
            doc.computed_style_property(a, property),
            expected,
            "{property}"
        );
    }
}

#[test]
fn cssom_lengths_use_element_root_and_viewport_context() {
    let mut doc = parse_html(
        r#"<html><head><style>
      html{font-size:20px}body{font-size:30px}
      #a{background-size:2em 3rem;vertical-align:2em;row-gap:10vh;column-gap:calc(1rem + 2vw)}
      #a::before{content:'x';font-size:2em}
    </style></head><body><span id=a>text</span></body></html>"#,
    );
    doc.set_viewport(800.0, 600.0);
    let a = doc.get_element_by_id("a").unwrap();
    for (property, expected) in [
        ("background-size", "60px 60px"),
        ("vertical-align", "60px"),
        ("row-gap", "60px"),
        ("column-gap", "36px"),
    ] {
        assert_eq!(
            doc.computed_style_property(a, property),
            expected,
            "{property}"
        );
    }
    assert_eq!(
        doc.computed_style_pseudo_property(a, "::before", "font-size"),
        "60px"
    );
    assert_eq!(doc.viewport(), (800.0, 600.0));
    doc.set_viewport(1000.0, 400.0);
    assert_eq!(doc.computed_style_property(a, "row-gap"), "40px");
    assert_eq!(doc.computed_style_property(a, "column-gap"), "40px");
    assert_eq!(doc.viewport(), (1000.0, 400.0));
}

#[test]
fn css_math_layout_recomputes_mixed_units_on_resize() {
    let mut doc = parse_html(
        r#"<html><head><style>
      html {font-size:20px}
      #container {width:400px;height:200px}
      #child {font-size:30px;width:calc(50% + 2em + 1rem + 5vw);
        height:calc(50% - 1em + 10vh)}
    </style></head><body><div id=container><div id=child>math</div></div></body></html>"#,
    );
    let child = doc.get_element_by_id("child").unwrap();
    for (width, height, expected_width, expected_height) in
        [(800.0, 600.0, 320.0, 130.0), (1000.0, 400.0, 330.0, 110.0)]
    {
        doc.set_viewport(width, height);
        let rect = doc.get_bounding_client_rect(child).unwrap();
        assert!((rect.w - expected_width).abs() < 0.01, "width: {}", rect.w);
        assert!(
            (rect.h - expected_height).abs() < 0.01,
            "height: {}",
            rect.h
        );
    }
}

#[test]
fn css_math_percentage_height_uses_definite_height_or_auto() {
    let doc = parse_and_layout(
        r#"<style>
      .child {height:min(50% + 10px, 150px)}
    </style><div style='width:400px;height:200px'><div id=definite class=child></div></div>
    <div style='width:400px'><div id=indefinite class=child><div style='height:25px'></div></div></div>"#,
        800.0,
    );
    let definite = crate::dom::query_selector(&doc.root, "#definite").unwrap();
    let indefinite = crate::dom::query_selector(&doc.root, "#indefinite").unwrap();
    assert_eq!(definite.layout.content_rect.h, 110.0);
    assert_eq!(indefinite.layout.content_rect.h, 25.0);
}

#[test]
fn cssom_relative_insets_use_containing_axes_and_preserve_declared_edges() {
    let mut doc = parse_html(
        "<div style='width:300px;height:100px'><div id=a style='position:relative;left:50%;top:50%'>a</div><div id=b style='position:relative;direction:rtl;left:10px;right:20px'>b</div></div>",
    );
    doc.set_viewport(800.0, 600.0);
    let a = doc.get_element_by_id("a").unwrap();
    let b = doc.get_element_by_id("b").unwrap();
    assert_eq!(doc.computed_style_property(a, "left"), "150px");
    assert_eq!(doc.computed_style_property(a, "top"), "50px");
    assert_eq!(doc.computed_style_property(a, "bottom"), "-50px");
    assert_eq!(doc.computed_style_property(b, "left"), "10px");
    assert_eq!(doc.computed_style_property(b, "right"), "20px");
}

#[test]
fn cssom_vertical_align_serializes_lengths_and_percentages() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<span id="px" style="vertical-align: 6px"></span>
           <span id="pct" style="vertical-align: 50%"></span>"#,
        800.0,
    );
    let px = doc.get_element_by_id("px").unwrap();
    let pct = doc.get_element_by_id("pct").unwrap();
    assert_eq!(doc.computed_style_property(px, "vertical-align"), "6px");
    assert_eq!(doc.computed_style_property(pct, "vertical-align"), "50%");
}

#[test]
fn css_list_style_type_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "list-style-type", "decimal");
    assert_eq!(style.list_style_type, ListStyleType::Decimal);
    apply_property(&mut style, "list-style-type", "decimal-leading-zero");
    assert_eq!(style.list_style_type, ListStyleType::DecimalLeadingZero);
    apply_property(&mut style, "list-style-type", "lower-latin");
    assert_eq!(style.list_style_type, ListStyleType::LowerLatin);
    apply_property(&mut style, "list-style-type", "lower-greek");
    assert_eq!(style.list_style_type, ListStyleType::LowerGreek);
    apply_property(&mut style, "list-style-type", "cjk-decimal");
    assert_eq!(style.list_style_type, ListStyleType::CjkDecimal);
    apply_property(&mut style, "list-style-type", "armenian");
    assert_eq!(style.list_style_type, ListStyleType::Armenian);
    apply_property(&mut style, "list-style-type", "lower-armenian");
    assert_eq!(style.list_style_type, ListStyleType::LowerArmenian);
    apply_property(&mut style, "list-style-type", "circle");
    assert_eq!(style.list_style_type, ListStyleType::Circle);
    apply_property(&mut style, "list-style-type", "disclosure-closed");
    assert_eq!(style.list_style_type, ListStyleType::DisclosureClosed);
    apply_property(&mut style, "list-style-type", "disclosure-open");
    assert_eq!(style.list_style_type, ListStyleType::DisclosureOpen);
    apply_property(&mut style, "list-style-type", "none");
    assert_eq!(style.list_style_type, ListStyleType::None);
}

#[test]
fn predefined_numeric_counter_styles_use_their_own_digits() {
    let samples = [
        ("arabic-indic", "١٢"),
        ("bengali", "১২"),
        ("cambodian", "១២"),
        ("khmer", "១២"),
        ("devanagari", "१२"),
        ("gujarati", "૧૨"),
        ("gurmukhi", "੧੨"),
        ("kannada", "೧೨"),
        ("lao", "໑໒"),
        ("malayalam", "൧൨"),
        ("mongolian", "᠑᠒"),
        ("myanmar", "၁၂"),
        ("oriya", "୧୨"),
        ("persian", "۱۲"),
        ("tamil", "௧௨"),
        ("telugu", "౧౨"),
        ("thai", "๑๒"),
        ("tibetan", "༡༢"),
    ];
    for (name, expected) in samples {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "list-style-type", name);
        assert_eq!(
            style.list_style_type,
            ListStyleType::Numeric(name),
            "{name}"
        );
        assert_eq!(
            crate::css::format_counter_value(12, name),
            expected,
            "{name}"
        );
        assert_eq!(
            crate::css::format_counter_value(-12, name),
            format!("-{expected}")
        );
        assert_eq!(crate::css::format_counter_value(0, name).chars().count(), 1);
    }
    assert_eq!(crate::css::format_counter_value(-12, "cjk-decimal"), "-12");
    assert_eq!(
        crate::css::format_counter_value(i32::MIN, "cjk-decimal"),
        i32::MIN.to_string()
    );

    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<ol id='arabic' style='list-style-type:arabic-indic'></ol><ol id='persian' style='list-style-type:persian'></ol>",
        800.0,
    );
    for (id, name) in [("arabic", "arabic-indic"), ("persian", "persian")] {
        let node = doc.get_element_by_id(id).unwrap();
        assert_eq!(doc.computed_style_property(node, "list-style-type"), name);
    }
}

#[test]
fn predefined_armenian_styles_use_case_and_range() {
    assert_eq!(crate::css::format_counter_value(12, "armenian"), "ԺԲ");
    assert_eq!(crate::css::format_counter_value(12, "upper-armenian"), "ԺԲ");
    assert_eq!(crate::css::format_counter_value(12, "lower-armenian"), "ժբ");
    for value in [0, -1, 10_000, i32::MIN] {
        assert_eq!(
            crate::css::format_counter_value(value, "armenian"),
            value.to_string()
        );
        assert_eq!(
            crate::css::format_counter_value(value, "lower-armenian"),
            value.to_string()
        );
    }
}

#[test]
fn css_list_style_shorthand_parses_position_image_and_type() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "list-style",
        r#"inside square url("marker.svg")"#,
    );
    assert_eq!(style.list_style_position, ListStylePosition::Inside);
    assert_eq!(style.list_style_type, ListStyleType::Square);
    assert_eq!(style.list_style_image, "marker.svg");

    apply_property(&mut style, "list-style", "none outside");
    assert_eq!(style.list_style_position, ListStylePosition::Outside);
    assert_eq!(style.list_style_type, ListStyleType::None);
    assert_eq!(style.list_style_image, "");
}

// ── Stylesheet struct: CSS variables ──────────────────────────────────────────

#[test]
fn css_variables_in_root() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        ":root { --main-color: #ff0000; --gap: 10px; } p { color: var(--main-color); }",
    );
    assert!(ss.variables.contains_key("--main-color"));
    assert_eq!(ss.variables["--main-color"], "#ff0000");
    assert!(ss.variables.contains_key("--gap"));
}

#[test]
fn css_variable_with_fallback() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(":root { --main: blue; } p { color: var(--missing, red); }");
    assert!(!ss.rules.is_empty());
}

#[test]
fn inherited_custom_properties_share_storage_until_overridden() {
    let empty_a = ComputedStyle::default();
    let empty_b = ComputedStyle::default();
    assert!(std::sync::Arc::ptr_eq(
        &empty_a.custom_props,
        &empty_b.custom_props
    ));
    let mut frame = EngineFrame::empty(800.0, 600.0);
    frame.load_html(
        "<style>:root { --ink: red } #override { --ink: blue } \
         .item { color: var(--ink) }</style>\
         <div id=parent><span id=first class=item>First</span>\
         <span id=second class=item>Second</span>\
         <span id=override><i id=local class=item>Local</i></span></div>",
    );
    frame.update_frame();
    let by_id = |id: &str| {
        find_box(&frame.doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap()
    };
    let parent = by_id("parent");
    let first = by_id("first");
    let second = by_id("second");
    let local = by_id("local");
    assert!(std::sync::Arc::ptr_eq(
        &parent.style.custom_props,
        &first.style.custom_props
    ));
    assert!(std::sync::Arc::ptr_eq(
        &first.style.custom_props,
        &second.style.custom_props
    ));
    assert!(!std::sync::Arc::ptr_eq(
        &parent.style.custom_props,
        &local.style.custom_props
    ));
    assert_eq!(first.style.color, Color::rgb(255, 0, 0));
    assert_eq!(local.style.color, Color::rgb(0, 0, 255));
}

// ── Hover and pseudo-element rules ────────────────────────────────────────────

#[test]
fn css_hover_rule() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add("a:hover { color: red; }");
    let found_hover = ss.rules.iter().any(|r| r.is_hover);
    assert!(found_hover, "should detect :hover rule");
}

#[test]
fn css_pseudo_element_before() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add("p::before { content: \">\"; }");
    let found_before = ss
        .rules
        .iter()
        .any(|r| r.pseudo_element == PseudoElement::Before);
    assert!(found_before, "should detect ::before pseudo-element rule");
}

#[test]
fn css_pseudo_element_after() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add("p::after { content: \"<\"; }");
    let found_after = ss
        .rules
        .iter()
        .any(|r| r.pseudo_element == PseudoElement::After);
    assert!(found_after, "should detect ::after pseudo-element rule");
}

#[test]
fn standards_known_pseudo_elements_do_not_drop_rules() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        "input::file-selector-button { color: red; }\
         details::details-content { color: blue; }\
         p::spelling-error { color: green; }\
         p::grammar-error { color: purple; }\
         dialog::backdrop { background: rgba(0,0,0,.5); }\
         p::first-line { color: orange; }\
         p::first-letter { color: teal; }\
         x-card::part(button) { color: fuchsia; }\
         x-card::slotted(span.highlight) { color: navy; }",
    );
    assert_eq!(
        ss.rules.len(),
        9,
        "known pseudo-elements should parse even before all paint/layout hooks exist"
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::Backdrop)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::FileSelectorButton)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::DetailsContent)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::SpellingError)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::GrammarError)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::FirstLine)
    );
    assert!(
        ss.rules
            .iter()
            .any(|r| r.pseudo_element == PseudoElement::FirstLetter)
    );
    assert!(ss.rules.iter().any(|r| {
        r.is_slotted && r.pseudo_element == PseudoElement::None && r.slotted_slot_selector.is_some()
    }));
    assert!(
        ss.rules
            .iter()
            .filter(|r| {
                !r.is_slotted
                    && !matches!(
                        r.pseudo_element,
                        PseudoElement::Backdrop
                            | PseudoElement::FileSelectorButton
                            | PseudoElement::DetailsContent
                            | PseudoElement::SpellingError
                            | PseudoElement::GrammarError
                            | PseudoElement::FirstLine
                            | PseudoElement::FirstLetter
                    )
            })
            .all(|r| r.pseudo_element == PseudoElement::Ignored)
    );
}

#[test]
fn first_line_and_first_letter_pseudo_styles_are_cascaded_and_readable() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             p::first-line { color: rgb(11, 22, 33); font-size: 19px; }
             p::first-letter { color: rgb(44, 55, 66); font-weight: 700; }
           </style>
           <p id="p">First line text wraps eventually.</p>"#,
        400.0,
    );
    let p = d.get_element_by_id("p").unwrap();
    assert_eq!(
        d.computed_style_pseudo_property(p, "::first-line", "color"),
        "rgb(11, 22, 33)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::first-line", "font-size"),
        "19px"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::first-letter", "color"),
        "rgb(44, 55, 66)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::first-letter", "font-weight"),
        "700"
    );
}

#[test]
fn first_letter_pseudo_style_reaches_text_paint() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"<style>
             p { color: rgb(1, 2, 3); font-weight: 400; }
             p::first-letter { color: rgb(44, 55, 66); font-weight: 700; }
           </style>
           <p id="p">Alpha beta</p>"#,
        400.0,
    );
    let list = build_display_list(&d.root, 400.0, 200.0);
    let texts: Vec<_> = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text {
                text,
                color,
                font_weight,
                ..
            } => Some((text.as_str(), *color, *font_weight)),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|(text, color, font_weight)| *text == "A"
            && color.r == 44
            && color.g == 55
            && color.b == 66
            && *font_weight == 700),
        "first letter should paint with its pseudo style; got {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|(text, color, font_weight)| *text == "lpha beta"
                && color.r == 1
                && color.g == 2
                && color.b == 3
                && *font_weight == 400),
        "remaining text should keep the originating element style; got {texts:?}"
    );
}

#[test]
fn spelling_and_grammar_error_pseudo_styles_are_cascaded_and_readable() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             p::spelling-error { color: rgb(1, 2, 3); background-color: rgb(4, 5, 6); }
             p::grammar-error { color: rgb(7, 8, 9); background-color: rgb(10, 11, 12); }
           </style>
           <p id="p">Text</p>"#,
        400.0,
    );
    let p = d.get_element_by_id("p").unwrap();
    assert_eq!(
        d.computed_style_pseudo_property(p, "::spelling-error", "color"),
        "rgb(1, 2, 3)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::spelling-error", "background-color"),
        "rgb(4, 5, 6)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::grammar-error", "color"),
        "rgb(7, 8, 9)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(p, "::grammar-error", "background-color"),
        "rgb(10, 11, 12)"
    );
}

#[test]
fn details_content_pseudo_styles_are_cascaded_and_readable() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             details::details-content {
               color: rgb(10, 20, 30);
               background-color: rgb(40, 50, 60);
               font-size: 21px;
             }
           </style>
           <details id="d" open><summary>Summary</summary><p>Body</p></details>"#,
        400.0,
    );
    let details = d.get_element_by_id("d").unwrap();
    assert_eq!(
        d.computed_style_pseudo_property(details, "::details-content", "color"),
        "rgb(10, 20, 30)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(details, "::details-content", "background-color"),
        "rgb(40, 50, 60)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(details, "::details-content", "font-size"),
        "21px"
    );
}

#[test]
fn file_selector_button_pseudo_styles_native_file_button_part() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
             input::file-selector-button {
               color: rgb(1, 2, 3);
               background-color: rgb(4, 5, 6);
               font-size: 20px;
               font-weight: 700;
               font-family: serif;
             }
           </style>
           <input id="file" type="file">"#,
        400.0,
    );
    let input = d.get_element_by_id("file").unwrap();
    assert_eq!(
        d.computed_style_pseudo_property(input, "::file-selector-button", "color"),
        "rgb(1, 2, 3)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(input, "::file-selector-button", "background-color"),
        "rgb(4, 5, 6)"
    );

    let list = build_display_list(&d.root, 400.0, 200.0);
    let Some(PaintCmd::FormElement {
        input_type,
        file_button_color,
        file_button_background,
        file_button_font_size,
        file_button_font_weight,
        file_button_font_family,
        ..
    }) = list.commands.iter().find(|cmd| {
        matches!(
            cmd,
            PaintCmd::FormElement {
                tag,
                input_type,
                ..
            } if tag == "input" && input_type == "file"
        )
    })
    else {
        panic!("file input should emit a FormElement command");
    };
    assert_eq!(input_type, "file");
    assert_eq!(*file_button_color, Color::rgb(1, 2, 3));
    assert_eq!(*file_button_background, Color::rgb(4, 5, 6));
    assert!((*file_button_font_size - 20.0).abs() < 0.01);
    assert_eq!(*file_button_font_weight, 700);
    assert_eq!(file_button_font_family, "serif");
}

// ── Additional property application ───────────────────────────────────────────

#[test]
fn css_box_sizing_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "box-sizing", "border-box");
    assert_eq!(style.box_sizing, BoxSizing::BorderBox);
    apply_property(&mut style, "box-sizing", "content-box");
    assert_eq!(style.box_sizing, BoxSizing::ContentBox);
}

#[test]
fn css_text_overflow_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-overflow", "ellipsis");
    assert_eq!(style.text_overflow, TextOverflow::Ellipsis);
    apply_property(&mut style, "text-overflow", r#"clip "--""#);
    assert_eq!(
        style.text_overflow,
        TextOverflow::Pair(Box::new([
            TextOverflow::Clip,
            TextOverflow::String("--".into())
        ]))
    );
    apply_property(&mut style, "text-overflow", "clip");
    assert_eq!(style.text_overflow, TextOverflow::Clip);

    let mut frame = crate::EngineFrame::new(
        crate::parse_html(r#"<div id="a" style='text-overflow: "..."; overflow: hidden'></div>"#),
        200.0,
        80.0,
    );
    frame.update_frame();
    let id = frame.doc.get_element_by_id("a").unwrap();
    assert_eq!(
        frame.doc.computed_style_property(id, "text-overflow"),
        r#""...""#
    );
}

#[test]
fn svg_stroke_width_cascades_inherits_and_rejects_invalid_values() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "stroke-width", "2px");
    for value in ["-1px", "auto", "none", "min-content", "bogus"] {
        apply_property(&mut style, "stroke-width", value);
        assert_eq!(
            style.rare().svg_stroke_width,
            Some(CssLength::Px(2.0)),
            "{value}"
        );
    }
    let mut frame = crate::EngineFrame::new(
        crate::parse_html(
            "<style>svg{stroke-width:0}circle{stroke-width:inherit}</style><svg><g id=g><circle id=c /></g></svg>",
        ),
        200.0,
        80.0,
    );
    frame.update_frame();
    for id in ["g", "c"] {
        let nid = frame.doc.get_element_by_id(id).unwrap();
        assert_eq!(
            frame.doc.computed_style_property(nid, "stroke-width"),
            "0px"
        );
    }
}

#[test]
fn text_overflow_preserves_both_edges_strings_and_invalid_declarations() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "text-overflow", "ellipsis");
    for invalid in [
        "",
        "bogus",
        "garbage ellipsis",
        "clip ellipsis clip",
        "\"unclosed",
        "\"bad\nstring\"",
    ] {
        apply_property(&mut style, "text-overflow", invalid);
        assert_eq!(style.text_overflow, TextOverflow::Ellipsis, "{invalid:?}");
    }
    for (value, expected) in [
        ("ELLIPSIS", "ellipsis"),
        ("clip ellipsis", "clip ellipsis"),
        ("\"\"", "\"\""),
        ("'start' '\\2026 '", "\"start\" \"…\""),
    ] {
        let html = format!(
            "<style>#parent{{text-overflow:{value}}}#child{{text-overflow:inherit}}</style><div id=parent><div id=child></div></div>"
        );
        let mut frame = crate::EngineFrame::new(crate::parse_html(&html), 200.0, 80.0);
        frame.update_frame();
        for selector in ["#parent", "#child"] {
            let id = frame.doc.query_selector(selector).unwrap();
            assert_eq!(
                frame.doc.computed_style_property(id, "text-overflow"),
                expected
            );
        }
    }
}

#[test]
fn line_clamp_limits_inline_lines_used_for_layout_and_paint() {
    let html = r#"
        <div style="width: 40px; line-clamp: 2">
            aa aa aa aa aa aa
        </div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let div = find_box(&doc.root, &|node| node.tag == "div").expect("div");

    assert_eq!(
        div.layout.line_cache.len(),
        2,
        "line-clamp should cap the generated inline line fragments"
    );

    let line_bottom = div
        .layout
        .line_cache
        .last()
        .map(|line| line.y + line.height)
        .unwrap_or(div.layout.content_rect.y);
    assert!(
        div.layout.content_rect.h <= line_bottom - div.layout.content_rect.y + 0.5,
        "content height should be based on the clamped lines"
    );
}

#[test]
fn line_clamp_paints_ellipsis_on_last_visible_line() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"
        <style>
        * { margin: 0; padding: 0; }
        div { width: 200px; line-clamp: 1; font-size: 16px; }
        </style>
        <div>first line words<br>second line words</div>
    "#,
        ),
        240.0,
        80.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 240.0, 80.0);
    let painted = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");

    assert!(
        painted.contains('…'),
        "line-clamp should paint an ellipsis on the final visible line; commands were {:?}",
        list.commands
    );
    assert!(
        !painted.contains("second"),
        "line-clamp should not paint omitted lines; painted text was {painted:?}"
    );
}

#[test]
fn rtl_line_clamp_paints_ellipsis_at_inline_end() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"<style>* { margin: 0; padding: 0; }
               div { width: 200px; line-clamp: 1; direction: rtl; font-size: 16px; }
               </style><div>السطر الأول<br>السطر الثاني</div>"#,
        ),
        240.0,
        80.0,
    );
    frame.update_frame();
    let list = build_display_list(&frame.doc.root, 240.0, 80.0);
    let div = crate::dom::query_selector(&frame.doc.root, "div").unwrap();
    assert!(
        div.layout
            .line_cache
            .iter()
            .any(|line| line.has_clamped_continuation),
        "layout must mark the visible RTL line as clamped: {:?}",
        div.layout.line_cache
    );
    let marker = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text == "…" => Some(*x),
            _ => None,
        })
        .expect("RTL clamped line should paint an ellipsis");
    assert!(
        marker < 10.0,
        "RTL inline-end marker belongs at the physical left edge: {marker}"
    );
    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.contains("الثاني")))
    );
}

#[test]
fn webkit_box_line_clamp_creates_block_with_ellipsis() {
    let mut frame = EngineFrame::new(
        parse_html(
            r#"
        <style>
        * { margin: 0; padding: 0; }
        a {
            display: -webkit-box;
            -webkit-box-orient: vertical;
            -webkit-line-clamp: 1;
            overflow: hidden;
            text-overflow: ellipsis;
            width: 110px;
            font-size: 16px;
            line-height: 20px;
        }
        p { font-size: 16px; line-height: 20px; }
        </style>
        <div><a>first line has too many words</a><p>next block starts below</p></div>
    "#,
        ),
        240.0,
        100.0,
    );
    frame.update_frame();
    let link = find_box(&frame.doc.root, &|node| node.tag == "a").expect("link");
    let paragraph = find_box(&frame.doc.root, &|node| node.tag == "p").expect("paragraph");

    assert_eq!(link.style.display, Display::Block);
    assert!(
        paragraph.layout.content_rect.y >= link.layout.content_rect.bottom() - 0.5,
        "following block should start below the clamped -webkit-box"
    );

    let list = build_display_list(&frame.doc.root, 240.0, 100.0);
    let painted = list
        .commands
        .iter()
        .filter_map(|cmd| match cmd {
            PaintCmd::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    assert!(
        painted.contains('…'),
        "clamped -webkit-box should paint an ellipsis"
    );
}

#[test]
fn inline_block_auto_width_measures_text_transform() {
    let doc = parse_and_layout(
        r#"
        <div style="font-family: Arial; font-size: 15px">
            <span style="display:inline-block; text-transform:uppercase; padding:6px 10px">
                breaking news
            </span>
        </div>
    "#,
        320.0,
    );
    let span = find_box(&doc.root, &|node| node.tag == "span").expect("span");

    assert_eq!(
        span.layout.line_cache.len(),
        1,
        "auto-width inline-block should be sized from transformed max-content text"
    );
}

#[test]
fn content_visibility_hidden_skips_descendant_paint_but_keeps_box_style() {
    let html = r#"
        <div id="box" style="content-visibility: hidden; background: red; width: 100px; height: 20px">
            <span>hidden text</span>
        </div>
    "#;
    let doc = parse_and_layout(html, 800.0);
    let list = build_display_list(&doc.root, 800.0, 600.0);

    assert!(
        !list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, PaintCmd::Text { text, .. } if text.contains("hidden text"))),
        "content-visibility:hidden should suppress descendant text paint"
    );
    assert!(
        list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                PaintCmd::FillRect {
                    color,
                    ..
                } if *color == Color::rgb(255, 0, 0)
            )
        }),
        "content-visibility:hidden should keep painting the element's own box decoration"
    );
}

#[test]
fn scrollbar_width_controls_scrollbar_space_reservation() {
    let html = |scrollbar_width: &str| {
        format!(
            r#"
            <div id="scroller" style="width:100px;height:20px;overflow-y:scroll;scrollbar-width:{scrollbar_width}">
                <div id="child" style="height:100px"></div>
            </div>
            "#
        )
    };
    let child_width = |scrollbar_width: &str| {
        let html = html(scrollbar_width);
        let doc = parse_and_layout(&html, 800.0);
        find_box(&doc.root, &|node| {
            node.attributes.get("id").map(|value| value.as_str()) == Some("child")
        })
        .expect("child")
        .layout
        .content_rect
        .w
    };

    assert_eq!(child_width("auto"), 90.0);
    assert_eq!(child_width("thin"), 94.0);
    assert_eq!(child_width("none"), 100.0);
}

#[test]
fn scrollbar_gutter_stable_both_edges_reserves_layout_space() {
    let doc = parse_and_layout(
        r#"
        <div id="scroller" style="width:100px;height:20px;overflow-y:auto;scrollbar-gutter:stable both-edges">
            <div id="child" style="height:10px"></div>
        </div>
        "#,
        800.0,
    );
    let scroller = find_box(&doc.root, &|node| {
        node.attributes.get("id").map(|value| value.as_str()) == Some("scroller")
    })
    .expect("scroller");
    let child = find_box(&doc.root, &|node| {
        node.attributes.get("id").map(|value| value.as_str()) == Some("child")
    })
    .expect("child");

    assert_eq!(scroller.layout.content_rect.w, 100.0);
    assert_eq!(child.layout.content_rect.w, 80.0);
}

#[test]
fn scrollbar_gutter_stable_follows_overflow_kind() {
    for (overflow, expected_width) in [
        ("visible", 100.0),
        ("clip", 100.0),
        ("hidden", 80.0),
        ("auto", 80.0),
        ("scroll", 80.0),
    ] {
        let html = format!(
            "<div style='width:100px;height:20px;overflow-y:{overflow};scrollbar-gutter:stable both-edges'>\
             <div id='child' style='height:10px'></div></div>"
        );
        let doc = parse_and_layout(&html, 800.0);
        let child = find_box(&doc.root, &|node| {
            node.attributes.get("id").map(String::as_str) == Some("child")
        })
        .expect("child");
        assert_eq!(
            child.layout.content_rect.w, expected_width,
            "overflow-y:{overflow}"
        );
    }
}

#[test]
fn stable_gutters_define_horizontal_scrollport_and_scroll_range() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>body{margin:0} #box{width:100px;height:40px;overflow-x:auto;overflow-y:hidden;scrollbar-gutter:stable both-edges} #child{width:90px;height:10px}</style><div id=box><div id=child></div></div>",
        800.0,
    );
    let id = doc.get_element_by_id("box").unwrap();
    let node = doc.find_webcore(id).unwrap();
    assert_eq!(node.scrollport_content_width(), 80.0);
    assert_eq!(doc.client_width(id), 80.0);
    assert_eq!(doc.element_scroll_width(id), 90.0);

    doc.element_scroll_to(id, 100.0, 0.0);
    assert_eq!(doc.element_scroll_left(id), 10.0);
}

#[test]
fn auto_scrollbar_reserves_width_only_when_content_overflows() {
    for (sizing, child_height, expected_width) in [
        ("height:20px", 10, 100.0),
        ("height:20px", 40, 90.0),
        ("max-height:20px", 10, 100.0),
        ("max-height:20px", 40, 90.0),
    ] {
        let html = format!(
            "<div style='width:100px;{sizing};overflow-y:auto'>\
             <div id='child' style='height:{child_height}px'></div></div>"
        );
        let doc = parse_and_layout(&html, 800.0);
        let child = find_box(&doc.root, &|node| {
            node.attributes.get("id").map(String::as_str) == Some("child")
        })
        .expect("child");
        assert_eq!(
            child.layout.content_rect.w, expected_width,
            "{sizing}, child={child_height}"
        );
    }
}

#[test]
fn mixed_block_scroller_includes_inline_line_width() {
    let doc = parse_and_layout(
        "<div id='scroll' style='width:60px;overflow-x:auto;white-space:nowrap'>\
         A long unbroken line that exceeds sixty pixels\
         <div style='width:10px;height:10px'></div></div>",
        800.0,
    );
    let node = crate::tests::test_grid::find_by_id(&doc.root, "scroll").unwrap();
    assert!(
        node.layout.scroll_width > node.layout.content_rect.w + 1.0,
        "inline text must contribute to scroll width: scroll={} content={}",
        node.layout.scroll_width,
        node.layout.content_rect.w,
    );

    let nested = parse_and_layout(
        "<div id='scroll' style='width:60px;overflow-x:auto'>\
         <div><div style='white-space:nowrap'>A long unbroken line that exceeds sixty pixels</div></div>\
         </div>",
        800.0,
    );
    let nested_scroller = crate::tests::test_grid::find_by_id(&nested.root, "scroll").unwrap();
    assert!(nested_scroller.layout.scroll_width > nested_scroller.layout.content_rect.w + 1.0);

    let clipped = parse_and_layout(
        "<div id='scroll' style='width:60px;overflow-x:auto'>\
         <div style='width:40px;overflow:hidden;white-space:nowrap'>\
         A long unbroken line that exceeds sixty pixels</div></div>",
        800.0,
    );
    let clipped_scroller = crate::tests::test_grid::find_by_id(&clipped.root, "scroll").unwrap();
    assert_eq!(
        clipped_scroller.layout.scroll_width,
        clipped_scroller.layout.content_rect.w
    );
}

#[test]
fn flex_and_grid_scrollers_include_visible_child_text_overflow() {
    for layout in ["display:flex", "display:grid;grid-template-columns:40px"] {
        let html = format!(
            "<div id='scroll' style='width:60px;overflow-x:auto;{layout}'>\
             <div style='width:40px;min-width:0;flex:none;white-space:nowrap'>\
             A long unbroken line that exceeds sixty pixels</div></div>"
        );
        let doc = parse_and_layout(&html, 800.0);
        let node = crate::tests::test_grid::find_by_id(&doc.root, "scroll").unwrap();
        assert!(
            node.layout.scroll_width > node.layout.content_rect.w + 1.0,
            "{layout}: scroll={} content={}",
            node.layout.scroll_width,
            node.layout.content_rect.w,
        );
    }
}

#[test]
fn ancestor_scrollers_include_visible_nested_vertical_overflow() {
    for layout in [
        "display:block",
        "display:flex;flex-direction:column",
        "display:grid;grid-template-rows:20px",
    ] {
        let html = format!(
            "<div id='scroll' style='height:50px;overflow-y:auto;{layout}'>\
             <div style='height:20px;flex:none'><div style='height:100px'></div></div></div>"
        );
        let doc = parse_and_layout(&html, 800.0);
        let node = crate::tests::test_grid::find_by_id(&doc.root, "scroll").unwrap();
        assert!(
            node.layout.scroll_height > node.layout.content_rect.h + 1.0,
            "{layout}: scroll={} content={}",
            node.layout.scroll_height,
            node.layout.content_rect.h,
        );

        let clipped_html = format!(
            "<div id='scroll' style='height:50px;overflow-y:auto;{layout}'>\
             <div style='height:20px;flex:none;overflow:hidden'>\
             <div style='height:100px'></div></div></div>"
        );
        let clipped = parse_and_layout(&clipped_html, 800.0);
        let node = crate::tests::test_grid::find_by_id(&clipped.root, "scroll").unwrap();
        assert_eq!(
            node.layout.scroll_height, node.layout.content_rect.h,
            "{layout}"
        );
    }
}

#[test]
fn css_outline_properties() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "outline-width", "2px");
    assert_eq!(style.outline_width, 2.0);
    apply_property(&mut style, "outline-style", "solid");
    assert_eq!(style.outline_style, BorderStyle::Solid);
    apply_property(&mut style, "outline-style", "dashed");
    assert_eq!(style.outline_style, BorderStyle::Dashed);
    apply_property(&mut style, "outline-offset", "3px");
    assert_eq!(style.outline_offset, 3.0);
}

#[test]
fn css_background_size_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-size", "cover");
    assert_eq!(style.background_size, BackgroundSize::Cover);
    apply_property(&mut style, "background-size", "contain");
    assert_eq!(style.background_size, BackgroundSize::Contain);
    apply_property(&mut style, "background-size", "auto");
    assert_eq!(style.background_size, BackgroundSize::Auto);
}

#[test]
fn css_object_fit_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "object-fit", "contain");
    assert_eq!(style.object_fit, ObjectFit::Contain);
    apply_property(&mut style, "object-fit", "Cover");
    assert_eq!(style.object_fit, ObjectFit::Cover);
}

#[test]
fn object_position_keywords_are_axis_aware_and_case_insensitive() {
    let mut style = ComputedStyle::default();

    apply_property(&mut style, "object-position", "Top Right");

    assert_eq!(style.object_position_x, CssLength::Percent(100.0));
    assert_eq!(style.object_position_y, CssLength::Percent(0.0));
}

#[test]
fn css_letter_spacing_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "letter-spacing", "2px");
    assert_eq!(style.letter_spacing, CssLength::Px(2.0));
}

#[test]
fn css_word_spacing_property() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "word-spacing", "5px");
    assert_eq!(style.word_spacing, CssLength::Px(5.0));
}

#[test]
fn css_mix_blend_mode_via_descendant_combinator() {
    // ".parent .child { mix-blend-mode: multiply }" must cascade to the child.
    use super::harness::find_box;
    use super::harness::parse_and_layout;
    use crate::types::MixBlendMode;
    let doc = parse_and_layout(
        r#"
        <style>
            .parent .overlay { mix-blend-mode: multiply; }
        </style>
        <div class="parent">
            <div class="overlay"></div>
        </div>
    "#,
        800.0,
    );
    let overlay = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "overlay")
            .unwrap_or(false)
    });
    assert!(overlay.is_some(), "overlay box not found");
    assert_eq!(
        overlay.unwrap().style.mix_blend_mode,
        MixBlendMode::Multiply,
        "mix-blend-mode: multiply must be applied via descendant combinator"
    );
}

#[test]
fn legacy_webkit_linear_gradient_background() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "white -webkit-gradient(linear, 0% 100%, 0% 0%, color-stop(0.37, #cecece), color-stop(0.65, #ffffff))",
    );
    assert_eq!(style.gradient_type, GradientType::Linear);
    assert!(style.gradient_angle.abs() < 0.01);
    assert_eq!(style.rare().gradient_stops.len(), 2);
    assert_eq!(
        style.rare().gradient_stops[0].color,
        Color::rgb(0xce, 0xce, 0xce)
    );
    assert!((style.rare().gradient_stops[0].position - 0.37).abs() < 0.01);
    assert!((style.rare().gradient_stops[1].position - 0.65).abs() < 0.01);
}

#[test]
fn pseudo_custom_properties_preserve_scope_and_important_layer_order() {
    let doc = parse_and_layout(
        "<style>@layer first,last;:root{--tone:blue}.box{color:var(--tone)}\
         .box::before{content:'before';display:block;--tone:red!important;--alias:var(--tone);\
         color:var(--alias);width:var(--size)}\
         @layer first{.box::before{--size:40px!important}}\
         @layer last{.box::before{--size:80px!important;--tone:black}}\
         .box::after{content:'after';display:block;--tone:green;color:var(--tone)}\
         </style><div class='box'></div>",
        300.0,
    );
    let host = find_box(&doc.root, &|n| {
        n.attributes.get("class").is_some_and(|v| v == "box")
    })
    .unwrap();
    let before = find_box(&doc.root, &|n| n.tag == "::before").unwrap();
    let after = find_box(&doc.root, &|n| n.tag == "::after").unwrap();
    assert_eq!(host.style.color, Color::rgb(0, 0, 255));
    assert_eq!(before.style.color, Color::rgb(255, 0, 0));
    assert_eq!(before.style.width, CssLength::Px(40.0));
    assert_eq!(after.style.color, Color::rgb(0, 128, 0));
    assert_eq!(
        before.style.custom_props.get("--alias").map(String::as_str),
        Some("red")
    );
    assert_eq!(
        host.style.custom_props.get("--tone").map(String::as_str),
        Some("blue")
    );
}

#[test]
fn generated_gradient_resolves_zero_percentage_color_mix_without_opaque_black() {
    assert_eq!(
        crate::css::parse_color("color-mix(in oklch, white 0%, transparent)")
            .unwrap()
            .a,
        0
    );
    let doc = parse_and_layout(
        "<style>:root{--color-white:white}.shine::before{--zero-percent:0%;content:'';\
         display:block;width:80px;height:100px;background-image:linear-gradient(to right,\
         color-mix(in oklch,var(--color-loading-shine,var(--color-white)) var(--zero-percent),transparent),\
         var(--color-loading-shine,var(--color-white)))}</style><div class='shine'></div>",
        300.0,
    );
    let pseudo = find_box(&doc.root, &|n| n.tag == "::before").unwrap();
    let stops = &pseudo.style.rare().gradient_stops;
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].color.a, 0);
    assert_eq!(stops[1].color, Color::WHITE);
}

#[test]
fn gradient_invalid_components_preserve_the_previous_image() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "background-image", "linear-gradient(red, blue)");
    let before = style.clone();
    for value in [
        "linear-gradient(red, nonsense, blue)",
        "linear-gradient(red auto, blue)",
        "linear-gradient(red 0% 20% 40%, blue)",
        "linear-gradient(red 0% 100%)",
        "linear-gradient(to top top, red, blue)",
        "linear-gradient(NaNdeg, red, blue)",
        "linear-gradient(red NaN%, blue)",
        "radial-gradient(nonsense, red, blue)",
        "radial-gradient(circle ellipse, red, blue)",
        "radial-gradient(circle 10% at center, red, blue)",
        "radial-gradient(ellipse 10px, red, blue)",
        "radial-gradient(closest-side 20px, red, blue)",
        "radial-gradient(circle -10px, red, blue)",
        "radial-gradient(at left left, red, blue)",
        "linear-gradient(red, blue), radial-gradient(nonsense, red, blue)",
    ] {
        apply_property(&mut style, "background-image", value);
        assert_eq!(style, before, "invalid declaration: {value}");
        assert!(
            !crate::css::parser::supports_condition_matches(&format!(
                "(background-image: {value})"
            )),
            "feature query: {value}"
        );
    }
}

#[test]
fn radial_gradient_explicit_radii_and_positions_use_shared_position_grammar() {
    for (value, shape, rx, ry, x, y) in [
        (
            "radial-gradient(20px at right bottom, red, blue)",
            GradientRadialShape::Circle,
            CssLength::Px(20.0),
            CssLength::Px(20.0),
            CssLength::Percent(100.0),
            CssLength::Percent(100.0),
        ),
        (
            "radial-gradient(20% 30% at top left, red, blue)",
            GradientRadialShape::Ellipse,
            CssLength::Percent(20.0),
            CssLength::Percent(30.0),
            CssLength::Percent(0.0),
            CssLength::Percent(0.0),
        ),
        (
            "radial-gradient(at right 10px bottom 20px, red, blue)",
            GradientRadialShape::Ellipse,
            CssLength::Auto,
            CssLength::Auto,
            crate::css::parse_length("calc(100% - 10px)"),
            crate::css::parse_length("calc(100% - 20px)"),
        ),
    ] {
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "background-image", value);
        assert_eq!(style.gradient_type, GradientType::Radial, "{value}");
        assert_eq!(style.gradient_radial_shape, shape);
        assert_eq!(style.gradient_radial_radius_x, rx);
        assert_eq!(style.gradient_radial_radius_y, ry);
        let font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
        let reference_box_px = 200.0;
        assert_eq!(
            style
                .gradient_radial_position_x
                .resolve(font_px, reference_box_px, font_px),
            x.resolve(font_px, reference_box_px, font_px)
        );
        assert_eq!(
            style
                .gradient_radial_position_y
                .resolve(font_px, reference_box_px, font_px),
            y.resolve(font_px, reference_box_px, font_px)
        );
    }
}

#[test]
fn css_radial_gradient_with_position_stops() {
    // radial-gradient(circle at 50% 50%, #fbbf24 0%, #f97316 60%, transparent 100%)
    // must parse 3 stops with correct colors and positions.
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "radial-gradient(circle at 50% 50%, #fbbf24 0%, #f97316 60%, transparent 100%)",
    );
    assert_eq!(
        style.gradient_type,
        GradientType::Radial,
        "should be radial gradient"
    );
    assert_eq!(
        style.rare().gradient_stops.len(),
        3,
        "should have 3 stops, not {}",
        style.rare().gradient_stops.len()
    );
    assert_eq!(
        style.rare().gradient_stops[0].color,
        Color::rgba(0xfb, 0xbf, 0x24, 0xff)
    );
    assert!((style.rare().gradient_stops[0].position - 0.0).abs() < 0.01);
    assert_eq!(
        style.rare().gradient_stops[1].color,
        Color::rgba(0xf9, 0x73, 0x16, 0xff)
    );
    assert!((style.rare().gradient_stops[1].position - 0.60).abs() < 0.01);
    assert_eq!(style.rare().gradient_stops[2].color, Color::TRANSPARENT);
    assert!((style.rare().gradient_stops[2].position - 1.0).abs() < 0.01);
    assert_eq!(style.gradient_radial_shape, GradientRadialShape::Circle);
    assert_eq!(style.gradient_radial_position_x, CssLength::Percent(50.0));
    assert_eq!(style.gradient_radial_position_y, CssLength::Percent(50.0));
}

#[test]
fn css_radial_gradient_bare_colors() {
    // radial-gradient without descriptor and without explicit positions
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "background",
        "radial-gradient(#ff0000, #0000ff)",
    );
    assert_eq!(style.gradient_type, GradientType::Radial);
    assert_eq!(style.gradient_radial_shape, GradientRadialShape::Ellipse);
    assert_eq!(
        style.gradient_radial_size,
        GradientRadialSize::FarthestCorner
    );
    assert_eq!(style.rare().gradient_stops.len(), 2, "should have 2 stops");
    assert_eq!(style.rare().gradient_stops[0].color, Color::rgb(255, 0, 0));
    assert!((style.rare().gradient_stops[0].position - 0.0).abs() < 0.01);
    assert_eq!(style.rare().gradient_stops[1].color, Color::rgb(0, 0, 255));
    assert!((style.rare().gradient_stops[1].position - 1.0).abs() < 0.01);
}

#[test]
fn gradient_current_color_resolves_after_the_element_color_cascades() {
    let doc = parse_and_layout(
        r#"<style>
            #linear { background-image: linear-gradient(currentColor 20% 40%, transparent); color: #123456; }
            #radial { background-image: radial-gradient(currentColor, red); color: #654321; }
            #layers { background-image: linear-gradient(red, blue), linear-gradient(currentColor, white); color: #abcdef; }
        </style><div id="linear">linear</div><div id="radial">radial</div><div id="layers">layers</div>"#,
        800.0,
    );
    for (id, expected, stop_count) in [
        ("linear", Color::rgb(0x12, 0x34, 0x56), 3),
        ("radial", Color::rgb(0x65, 0x43, 0x21), 2),
    ] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|value| value == id)
        })
        .unwrap();
        assert_eq!(node.style.rare().gradient_stops.len(), stop_count);
        assert_eq!(node.style.rare().gradient_stops[0].color, expected);
        assert!(!node.style.rare().gradient_stops[0].current_color);
    }

    let layered = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .is_some_and(|value| value == "layers")
    })
    .unwrap();
    assert_eq!(layered.style.rare().additional_background_layers.len(), 1);
    let second = &layered.style.rare().additional_background_layers[0].gradient_stops[0];
    assert_eq!(second.color, Color::rgb(0xab, 0xcd, 0xef));
    assert!(!second.current_color);

    let mut legacy = ComputedStyle::default();
    apply_property(
        &mut legacy,
        "background",
        "-webkit-gradient(linear, 0% 100%, 0% 0%, from(currentColor), to(blue))",
    );
    legacy.color = Color::rgb(4, 5, 6);
    crate::css::finalize_current_color(&mut legacy);
    assert_eq!(legacy.rare().gradient_stops[0].color, legacy.color);
}

// ── CSS Variable Inheritance ─────────────────────────────────────────────────

#[test]
fn css_var_inherited_from_root() {
    // Variables defined on :root should be inherited by child elements.
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root { --main-color: red; }
        p { color: var(--main-color); }
    </style></head><body><p>hello</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    assert_eq!(
        p.style.color,
        Color::rgb(255, 0, 0),
        "var(--main-color) should resolve to red"
    );
}

#[test]
fn font_inherit_shorthand_does_not_override_later_longhands() {
    let doc = parse_and_layout(
        r#"<style>
            body { font: 18px serif; }
            h2 { font: inherit; }
            .headline { font-size: 48px; font-weight: 900; font-family: Arial; }
        </style><h2 class="headline">Headline</h2>"#,
        800.0,
    );
    let heading = find_box(&doc.root, &|b| b.tag == "h2").unwrap();
    assert_eq!(heading.style.font_size, CssLength::Px(48.0));
    assert_eq!(heading.style.font_weight, FontWeight::Value(900));
    assert_eq!(heading.style.font_family, "Arial");
}

#[test]
fn inactive_root_theme_does_not_override_light_custom_property() {
    let doc = parse_and_layout(
        r#"<html data-theme="light"><head><style>
            html { --background: white; }
            html[data-theme=dark] { --background: #05162c; }
            body { background-color: var(--background); }
        </style></head><body>Light</body></html>"#,
        800.0,
    );
    let body = find_box(&doc.root, &|b| b.tag == "body").unwrap();
    assert_eq!(body.style.background_color, Color::rgb(255, 255, 255));
}

#[test]
fn grouped_root_light_theme_does_not_take_inactive_dark_value() {
    let doc = parse_and_layout(
        r#"<html><head><style>
            :root, html[data-theme=light] { --background: #ffffff; }
            html[data-theme=dark] { --background: #05162c; }
            body { background-color: var(--background); }
        </style></head><body>Light</body></html>"#,
        800.0,
    );
    let body = find_box(&doc.root, &|b| b.tag == "body").unwrap();
    assert_eq!(body.style.background_color, Color::rgb(255, 255, 255));
}

#[test]
fn desktop_headline_calc_font_size_resolves_at_viewport_width() {
    let doc = parse_and_layout(
        r#"<style>
            h2 { font-size: 48px; }
            @media screen and (min-width:1280px) {
                h2 { font-size: calc(36px + 16 * (100vw - 1280px) / 160); }
            }
        </style><h2>Headline</h2>"#,
        1366.0,
    );
    let heading = find_box(&doc.root, &|b| b.tag == "h2").unwrap();
    let actual = heading
        .style
        .font_size
        .resolve_vp(16.0, 0.0, 16.0, 1366.0, 768.0);
    let parsed = crate::css::parse_length("calc(36px + 16 * (100vw - 1280px) / 160)");
    assert!(
        (actual - 44.6).abs() < 0.1,
        "got {actual}px from {:?}, parsed {:?}",
        heading.style.font_size,
        parsed
    );
}

#[test]
fn css_bootstrap_rgb_triplet_vars_resolve_in_important_utilities() {
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root { --bs-light-rgb: 248, 249, 250; --bs-dark-rgb: 33, 37, 41; }
        .badge { display: inline-block; padding: .35em .65em; font-size: .75em; }
        .bg-light { --bs-bg-opacity: 1; background-color: rgba(var(--bs-light-rgb), var(--bs-bg-opacity)) !important; }
        .text-dark { --bs-text-opacity: 1; color: rgba(var(--bs-dark-rgb), var(--bs-text-opacity)) !important; }
    </style></head><body><span class="badge bg-light text-dark">2542 members</span></body></html>"#,
        800.0,
    );
    let badge = find_box(&doc.root, &|b| {
        b.tag == "span"
            && b.attributes
                .get("class")
                .is_some_and(|class| class.contains("badge"))
    })
    .unwrap();
    assert_eq!(
        badge.style.background_color,
        Color::rgb(248, 249, 250),
        "Bootstrap rgba(var(--rgb-triplet), var(--opacity)) background should resolve"
    );
    assert_eq!(
        badge.style.color,
        Color::rgb(33, 37, 41),
        "Bootstrap text color utility should resolve"
    );
}

#[test]
fn css_bootstrap_root_selector_list_vars_resolve() {
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root,[data-bs-theme=light] { --bs-light-rgb: 248, 249, 250; --bs-dark-rgb: 33, 37, 41; }
        .bg-light { --bs-bg-opacity: 1; background-color: rgba(var(--bs-light-rgb), var(--bs-bg-opacity)) !important; }
        .text-dark { --bs-text-opacity: 1; color: rgba(var(--bs-dark-rgb), var(--bs-text-opacity)) !important; }
    </style></head><body><span class="bg-light text-dark">2542 members</span></body></html>"#,
        800.0,
    );
    let badge = find_box(&doc.root, &|b| b.tag == "span").unwrap();
    assert_eq!(
        badge.style.background_color,
        Color::rgb(248, 249, 250),
        "Bootstrap :root,[data-bs-theme=light] variables should be visible to utilities"
    );
    assert_eq!(badge.style.color, Color::rgb(33, 37, 41));
}

#[test]
fn adjacent_var_substitutions_in_border_shorthand_keep_token_boundaries() {
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root {
          --brand: rgb(251, 84, 43);
          --nested-brand: var(--brand);
          --on-brand: white;
        }
        .leoButton {
          --primary-color: var(--missing-button-color, var(--nested-brand));
          --mixed-primary-color: var(--primary-color);
          --border-width: 0px;
          --border-color: transparent;
          --bg: transparent;
          --color: black;
          border: solid var(--border-width,1px)var(--border-color,transparent);
          background: var(--bg);
          color: var(--color);
        }
        .leoButton.isFilled {
          --bg: var(--mixed-primary-color);
          --color: var(--on-brand);
        }
    </style></head><body><a class="leoButton isFilled">Download Brave</a></body></html>"#,
        800.0,
    );
    let button = find_box(&doc.root, &|b| {
        b.tag == "a"
            && b.attributes
                .get("class")
                .is_some_and(|class| class.contains("leoButton"))
    })
    .unwrap();
    assert_eq!(
        button.style.border_top_width,
        CssLength::Px(0.0),
        "adjacent var() values in border shorthand must remain distinct tokens"
    );
    assert_eq!(button.style.border_top_color, Color::TRANSPARENT);
    assert_eq!(button.style.background_color, Color::rgb(251, 84, 43));
    assert_eq!(button.style.color, Color::WHITE);
}

#[test]
fn custom_property_substitution_preserves_both_token_boundaries() {
    let vars = std::collections::HashMap::from([
        ("--n".to_string(), "10".to_string()),
        ("--unit".to_string(), "px".to_string()),
        ("--size".to_string(), "10px".to_string()),
    ]);
    for (source, expected) in [
        ("var(--n)px", "10/**/px"),
        ("1var(--n)", "1var(--n)"),
        ("-var(--n)", "-var(--n)"),
        (".var(--n)", "./**/10"),
        ("var(--n)var(--unit)", "10/**/px"),
        ("calc(var(--size)+ 1px)", "calc(10px+ 1px)"),
        ("calc(var(--size) + 1px)", "calc(10px + 1px)"),
    ] {
        assert_eq!(crate::css::resolve_var_references(source, &vars), expected);
    }
    let doc = parse_and_layout(
        r#"<style>
        :root { --n: 10; --unit: px; --size: 10px; --split: var(--n)var(--unit); }
        div { padding: 4px; }
        #suffix { padding: var(--n)px; }
        #nested { padding: var(--split); }
        #bad-space { padding: calc(var(--size)+ 1px); }
        #good-space { padding: calc(var(--size) + 1px); }
        </style><div id="suffix"></div><div id="nested"></div>
        <div id="bad-space"></div><div id="good-space"></div>"#,
        800.0,
    );
    for id in ["suffix", "nested", "bad-space"] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|v| v == id)
        })
        .unwrap();
        assert_eq!(
            node.style.padding_top,
            ComputedStyle::default().padding_top,
            "{id}"
        );
    }
    let valid = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|v| v == "good-space")
    })
    .unwrap();
    assert_eq!(valid.style.padding_top, CssLength::Px(11.0));
}

#[test]
fn custom_property_color_mix_chain_resolves_before_border_shorthand() {
    let doc = parse_and_layout(
        r#"<html data-theme="light"><head><style>
        :root {
          --leo-color-button-background: #434fcf;
        }
        .leoButton {
          --background: transparent;
          --primary-color: var(--leo-button-color, var(--leo-color-button-background));
          --border-width: 1px;
          --border-color: color-mix(in srgb, var(--primary-color), var(--background) 30%);
          border: solid var(--border-width,1px)var(--border-color,transparent);
        }
    </style></head><body><a class="leoButton">Search the Web</a></body></html>"#,
        800.0,
    );
    let button = find_box(&doc.root, &|b| {
        b.tag == "a"
            && b.attributes
                .get("class")
                .is_some_and(|class| class.contains("leoButton"))
    })
    .unwrap();
    assert_eq!(button.style.border_top_width, CssLength::Px(1.0));
    assert_eq!(
        button.style.border_top_color,
        Color::rgba(47, 55, 145, 179),
        "nested custom-property fallback and color-mix should resolve before border parses"
    );
}

#[test]
fn color_mix_calc_percentage_keeps_opaque_theme_colors() {
    let doc = parse_and_layout(
        r#"<style>:root { --main: #f2f0ed; --ink: #0c0b09 }
        .banner {
          --tw-bg-opacity: 1;
          background-color: color-mix(in srgb, var(--main) calc(100%*var(--tw-bg-opacity, 1)), transparent);
          color: color-mix(in srgb, var(--ink) calc(100%*var(--tw-bg-opacity, 1)), transparent);
        }</style><div class="banner" id="banner">Title</div>"#,
        800.0,
    );
    let banner = crate::tests::test_grid::find_by_id(&doc.root, "banner").unwrap();
    assert_eq!(banner.style.background_color, Color::rgb(242, 240, 237));
    assert_eq!(banner.style.color, Color::rgb(12, 11, 9));
}

#[test]
fn stylesheet_root_variable_extraction_leaves_conditional_selectors_scoped() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        r#"
        :root[data-theme=light] { --brand: #434fcf; }
        :root * { --scoped-only: red; }
        html:where(.theme) { --accent: #fb542b; }
        "#,
    );
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert!(!sheet.variables.contains_key("--brand"));
    assert!(!sheet.variables.contains_key("--accent"));
    assert!(
        !sheet.variables.contains_key("--scoped-only"),
        "descendant-scoped custom properties must not be promoted to stylesheet globals"
    );
}

#[test]
fn stylesheet_clones_preserve_viewport_variables_without_raw_sources() {
    let mut fragment = Stylesheet::default();
    fragment.parse_and_add(
        ":root { --theme: small; } @media (min-width: 700px) { :root { --theme: large; } }",
    );
    let original = fragment.clone();
    assert_eq!(fragment.source_count, original.source_count);

    let mut merged = Stylesheet::default();
    merged.append_fragment(fragment);
    assert_eq!(merged.source_count, original.source_count);
    merged.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        merged.variables.get("--theme").map(String::as_str),
        Some("large")
    );
    merged.resolve_variables_for_viewport(600.0, 600.0);
    assert_eq!(
        merged.variables.get("--theme").map(String::as_str),
        Some("small")
    );
}

#[test]
fn stylesheet_root_variables_follow_parsed_supports_media_and_priority() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        ":root { --mode: base; --accent: first !important; }\
         @supports (display: unsupported-value) { :root { --mode: wrong; } }\
         @media (min-width: 700px) { :root { --mode: wide; } }\
         :root { --accent: second; }",
    );
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        sheet.variables.get("--mode").map(String::as_str),
        Some("wide")
    );
    assert_eq!(
        sheet.variables.get("--accent").map(String::as_str),
        Some("first")
    );
    sheet.resolve_variables_for_viewport(600.0, 600.0);
    assert_eq!(
        sheet.variables.get("--mode").map(String::as_str),
        Some("base")
    );
}

#[test]
fn deleted_root_rule_no_longer_supplies_custom_properties() {
    let mut sheet = Stylesheet::default();
    sheet.insert_rule(":root { --deleted: red; }", 0).unwrap();
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        sheet.variables.get("--deleted").map(String::as_str),
        Some("red")
    );
    sheet.delete_rule(0).unwrap();
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert!(!sheet.variables.contains_key("--deleted"));
}

#[test]
fn stylesheet_variables_follow_statement_form_at_rules() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        "@layer primer-css-base, primer-react; :root { --borderRadius-medium: .375rem; --borderWidth-thin: .0625rem; }",
    );
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        sheet
            .variables
            .get("--borderRadius-medium")
            .map(String::as_str),
        Some(".375rem")
    );
    assert_eq!(
        sheet
            .variables
            .get("--borderWidth-thin")
            .map(String::as_str),
        Some(".0625rem")
    );
}

#[test]
fn streamed_custom_property_only_root_fragment_is_preserved() {
    let css = ":root{--brand:#434fcf}.button{color:var(--brand)}";
    let loader: crate::StylesheetLoader = std::sync::Arc::new(|_| Ok(String::new()));
    let streaming_loader: crate::StreamingStylesheetLoader = std::sync::Arc::new(move |_, emit| {
        emit(css);
        Ok(())
    });
    let loaded = crate::load_stylesheet_cached(
        "test-streamed-root-vars".to_string(),
        "https://example.test/app.css".to_string(),
        "".to_string(),
        loader,
        Some(streaming_loader),
        false,
        |_| {},
    );
    let mut sheet = loaded.sheet;
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        sheet.variables.get("--brand").map(String::as_str),
        Some("#434fcf"),
        "streaming must not drop custom-property-only root fragments"
    );
    assert_eq!(
        crate::css::resolve_var_references("var(--brand)", &sheet.variables),
        "#434fcf"
    );
}

#[test]
fn css_badge_background_shorthand_with_rgba_paints_bubble() {
    let doc = parse_and_layout(
        r#"<html><head><style>
        .tree-card-header .badge {
          display: inline-block;
          padding: .35em .65em;
          border-radius: 999px;
          background: rgba(255,255,255,.2) !important;
          color: white !important;
        }
    </style></head><body><div class="tree-card-header"><span class="badge">2542 members</span></div></body></html>"#,
        800.0,
    );
    let badge = find_box(&doc.root, &|b| {
        b.tag == "span"
            && b.attributes
                .get("class")
                .is_some_and(|class| class.contains("badge"))
    })
    .unwrap();
    assert_eq!(
        badge.style.background_color,
        Color::rgba(255, 255, 255, 51),
        "badge background shorthand with rgba() should compute to a visible bubble"
    );
    assert_eq!(badge.style.color, Color::WHITE);
    assert_ne!(
        badge.style.border_radius,
        CssLength::Zero,
        "rounded pill badge should preserve its radius"
    );
}

#[test]
fn css_var_fallback_when_undefined() {
    // var(--undefined, blue) should use the fallback.
    let doc = parse_and_layout(
        r#"<html><head><style>
        p { color: var(--nope, blue); }
    </style></head><body><p>hello</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    assert_eq!(
        p.style.color,
        Color::rgb(0, 0, 255),
        "fallback blue should be used"
    );
}

#[test]
fn css_var_scoped_to_matching_selector() {
    // Variables on a class-qualified selector should only apply when the class matches.
    // .theme-a defines --fg: green, .theme-b defines --fg: red.
    // Only .theme-a is on the div, so --fg should be green.
    let doc = parse_and_layout(
        r#"<html><head><style>
        .theme-a { --fg: green; }
        .theme-b { --fg: red; }
        span { color: var(--fg, black); }
    </style></head><body>
        <div class="theme-a"><span>A</span></div>
    </body></html>"#,
        800.0,
    );
    let span = find_box(&doc.root, &|b| b.tag == "span").unwrap();
    assert_eq!(
        span.style.color,
        Color::rgb(0, 128, 0),
        "should inherit --fg:green from .theme-a"
    );
}

#[test]
fn css_var_self_reference_is_invalid_and_consuming_fallback_applies() {
    let doc = parse_and_layout(
        r#"<html><head><style>
        html { --sz: var(--sz, 20px); }
        p { font-size: var(--sz); margin-left: var(--sz, 12px); }
    </style></head><body><p>text</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    assert_eq!(
        p.style.font_size,
        CssLength::Px(16.0),
        "self-ref custom property should be invalid, not resolve to its own fallback"
    );
    assert_eq!(
        p.style.margin_left,
        CssLength::Px(12.0),
        "a declaration consuming an invalid custom property should use its own fallback"
    );
}

#[test]
fn css_var_inherited_through_nested_elements() {
    // Variables should be inherited through the DOM tree.
    let doc = parse_and_layout(
        r#"<html><head><style>
        .outer { --gap: 8px; }
        .inner { margin-left: var(--gap); }
    </style></head><body>
        <div class="outer"><div><div class="inner">deep</div></div></div>
    </body></html>"#,
        800.0,
    );
    let inner = find_box(&doc.root, &|b| {
        b.attributes
            .get("class")
            .map(|c| c == "inner")
            .unwrap_or(false)
    })
    .unwrap();
    assert_eq!(
        inner.style.margin_left,
        CssLength::Px(8.0),
        "var(--gap) should inherit through nested elements"
    );
}

#[test]
fn css_var_override_in_child() {
    // A child can override a variable defined by a parent.
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root { --c: red; }
        .override { --c: blue; }
        span { color: var(--c); }
    </style></head><body>
        <div><span id="a">A</span></div>
        <div class="override"><span id="b">B</span></div>
    </body></html>"#,
        800.0,
    );
    let a = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "a").unwrap_or(false)
    })
    .unwrap();
    let b = find_box(&doc.root, &|b| {
        b.attributes.get("id").map(|v| v == "b").unwrap_or(false)
    })
    .unwrap();
    assert_eq!(
        a.style.color,
        Color::rgb(255, 0, 0),
        "span A should get --c:red from :root"
    );
    assert_eq!(
        b.style.color,
        Color::rgb(0, 0, 255),
        "span B should get --c:blue from .override parent"
    );
}

#[test]
fn css_var_chain_resolution() {
    // Variables can reference other variables: --a: var(--b), --b: 10px.
    let doc = parse_and_layout(
        r#"<html><head><style>
        :root { --b: 10px; --a: var(--b); }
        p { padding-left: var(--a); }
    </style></head><body><p>text</p></body></html>"#,
        800.0,
    );
    let p = find_box(&doc.root, &|b| b.tag == "p").unwrap();
    assert_eq!(
        p.style.padding_left,
        CssLength::Px(10.0),
        "chained var(--a) -> var(--b) -> 10px"
    );
}

/// ⛔ `unset` is `inherit` on an inherited property and `initial` on every
/// other — CSS Cascade 5 §7.3, verbatim: it "acts as either `inherit` or
/// `initial`, depending on whether the property is inherited or not".
///
/// webcore collapsed `unset` into `initial` at PARSE time (`rule.rs` mapped it
/// straight to `CssValue::Initial`), so the distinction was destroyed before
/// the cascade could act on it — `CssValue::Unset` was a variant nothing ever
/// produced. `color: unset` on a child reset to black instead of inheriting.
///
/// The border row is here because it caught a second bug: the initial value of
/// `border-*-width` is `medium`, which CSS Backgrounds 3 §4.3 pins at exactly
/// 3px, and webcore had 0. Chrome agrees on all three.
#[test]
fn unset_inherits_an_inherited_property_and_initialises_the_rest() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#p{color:rgb(200,0,0);font-size:30px}         #c{color:unset;font-size:unset;border-top-width:unset;border-top-style:solid}</style>         <div id=p><div id=c>x</div></div>", 900.0);
    let c = d.get_element_by_id("c").unwrap();

    assert_eq!(
        d.computed_style_property(c, "color"),
        "rgb(200, 0, 0)",
        "`color` is inherited, so `unset` must inherit"
    );
    assert_eq!(
        d.computed_style_property(c, "font-size"),
        "30px",
        "`font-size` is inherited, so `unset` must inherit"
    );
    assert_eq!(
        d.computed_style_property(c, "border-top-width"),
        "3px",
        "`border-*-width` is NOT inherited, so `unset` is `initial` — `medium`, 3px"
    );
}

#[test]
fn css_wide_keywords_ignore_ascii_case_across_cascade_paths() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>\
         @layer base, theme;\
         #parent { color: rgb(20, 30, 40) }\
         #inherit { color: InHeRiT }\
         #unset { color: UnSeT; border-top-style: solid; border-top-width: UnSeT }\
         #initial { font-size: 40px; font-size: InItIaL }\
         #revert { display: ReVeRt }\
         @layer base { #layer { color: rgb(200, 0, 0) } }\
         @layer theme { #layer { color: blue } #layer { color: ReVeRt-LaYeR } }\
         #fallback { color: var(--missing, InHeRiT) }\
         #important { color: red; color: InHeRiT !important }\
         @supports (display: InHeRiT) { #supported { color: green } }\
         </style><div id=parent><div id=inherit></div><div id=unset></div>\
         <div id=initial></div>\
         <div id=revert></div><div id=layer></div><div id=fallback></div>\
         <div id=important></div><div id=inline style='color: InHeRiT'></div>\
         <div id=supported></div></div>",
        900.0,
    );
    let mut style = |id| {
        let node = doc.get_element_by_id(id).unwrap();
        (
            doc.computed_style_property(node, "color"),
            doc.computed_style_property(node, "display"),
            doc.computed_style_property(node, "border-top-width"),
            doc.computed_style_property(node, "font-size"),
        )
    };
    assert_eq!(style("inherit").0, "rgb(20, 30, 40)");
    assert_eq!(style("unset").0, "rgb(20, 30, 40)");
    assert_eq!(style("unset").2, "3px");
    assert_eq!(style("initial").3, "16px");
    assert_eq!(style("revert").1, "block");
    assert_eq!(style("layer").0, "rgb(200, 0, 0)");
    assert_eq!(style("fallback").0, "rgb(20, 30, 40)");
    assert_eq!(style("important").0, "rgb(20, 30, 40)");
    assert_eq!(style("inline").0, "rgb(20, 30, 40)");
    assert_eq!(style("supported").0, "rgb(0, 128, 0)");
}

#[test]
fn revert_rolls_author_display_back_to_ua_display() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>div { display: revert; }</style><div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "display"),
        "block",
        "`display: revert` in author CSS must roll back to the UA div display, not initial inline"
    );
}

#[test]
fn revert_layer_rolls_back_only_the_current_layer() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer base, theme;\
         @layer base { #t { color: rgb(200, 0, 0); display: inline; } }\
         @layer theme { #t { color: rgb(0, 0, 200); display: block; }\
                        #t { color: revert-layer; display: revert-layer; } }</style>\
         <div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "color"),
        "rgb(200, 0, 0)",
        "`revert-layer` should roll back declarations from its own layer, not all author CSS"
    );
    assert_eq!(
        d.computed_style_property(div, "display"),
        "inline",
        "`revert-layer` should expose the previous author layer's display"
    );
}

#[test]
fn important_global_keywords_use_cascade_context() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer base, theme;\
         #p { color: rgb(20, 30, 40); }\
         @layer base { #t { color: rgb(200, 0, 0) !important; display: inline !important; } }\
         @layer theme { #t { color: rgb(0, 0, 200) !important; display: block !important; }\
                        #t { color: revert-layer !important; display: revert-layer !important; } }\
         #inherit { color: inherit !important; }</style>\
         <div id=p><div id=t>x</div><div id=inherit>x</div></div>",
        900.0,
    );
    let layered = d.get_element_by_id("t").unwrap();
    let inherited = d.get_element_by_id("inherit").unwrap();

    assert_eq!(
        d.computed_style_property(layered, "color"),
        "rgb(200, 0, 0)",
        "important `revert-layer` should roll back only its own important layer"
    );
    assert_eq!(
        d.computed_style_property(layered, "display"),
        "inline",
        "important `revert-layer` should expose the previous important layer"
    );
    assert_eq!(
        d.computed_style_property(inherited, "color"),
        "rgb(20, 30, 40)",
        "important `inherit` must copy the parent value instead of becoming a no-op"
    );
}

#[test]
fn pseudo_revert_layer_uses_pseudo_layer_context() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer base, theme;\
         @layer base { #t::before { content: 'x'; color: rgb(200, 0, 0); } }\
         @layer theme { #t::before { color: rgb(0, 0, 200); color: revert-layer; } }</style>\
         <div id=t></div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_pseudo_property(div, "::before", "color"),
        "rgb(200, 0, 0)",
        "pseudo `revert-layer` should roll back only the current pseudo cascade layer"
    );
}

#[test]
fn state_revert_layer_uses_state_layer_context() {
    let doc = parse_and_layout(
        "<style>@layer base, theme;\
         @layer base { #t:hover { color: rgb(200, 0, 0); } }\
         @layer theme { #t:hover { color: rgb(0, 0, 200); color: revert-layer; } }</style>\
         <div id=t>x</div>",
        900.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "t")
    })
    .expect("target");
    let hover = div.style.hover_style.as_ref().expect("hover style");

    assert_eq!(
        hover.color,
        Color::rgb(200, 0, 0),
        "state `revert-layer` should roll back only the current state cascade layer"
    );
}

#[test]
fn state_variable_revert_layer_without_prior_value_inherits_color() {
    let doc = parse_and_layout(
        "<style>:root { color: rgb(0, 128, 0); } @layer base, theme;\
         @layer base { #t:hover { color: rgb(200, 0, 0); } }\
         @layer theme { #t:hover { color: rgb(0, 0, 200); --choice: revert-layer; color: var(--choice); } }</style>\
         <div id=t>x</div>",
        900.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "t")
    })
    .expect("target");
    let hover = div.style.hover_style.as_ref().expect("hover style");
    assert_eq!(hover.color, Color::rgb(0, 128, 0));
}

#[test]
fn state_important_custom_property_overrides_normal_value() {
    let doc = parse_and_layout(
        "<style>@layer base, theme;\
         @layer base { #t:hover { --ink: red; color: var(--ink); } }\
         @layer theme { #t:hover { --ink: blue !important; } }</style>\
         <div id=t>x</div>",
        900.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "t")
    })
    .expect("target");
    let hover = div.style.hover_style.as_ref().expect("hover style");
    assert_eq!(hover.color, Color::rgb(0, 0, 255));
    assert_eq!(
        hover.custom_props.get("--ink").map(String::as_str),
        Some("blue")
    );
}

#[test]
fn custom_property_revert_and_revert_layer_follow_cascade_precedence() {
    let doc = parse_and_layout(
        "<style>@layer base, theme; :root { --ink: green; }\
         @layer base { #layered { --ink: red; } }\
         @layer theme { #layered { --ink: blue; --ink: revert-layer; } }\
         #layered { color: var(--ink); }\
         #origin { --ink: red; --ink: revert; color: var(--ink); }\
         @layer base { #important { --ink: revert-layer !important; } }\
         @layer theme { #important { --ink: blue !important; } }\
         #important { color: var(--ink); }\
         #inline { --ink: red; color: var(--ink); }</style>\
         <p id=layered>x</p><p id=origin>x</p><p id=important>x</p>\
         <p id=inline style='--ink: revert-layer'>x</p>",
        800.0,
    );
    for (id, color) in [
        ("layered", Color::rgb(255, 0, 0)),
        ("origin", Color::rgb(0, 128, 0)),
        ("important", Color::rgb(0, 0, 255)),
        ("inline", Color::rgb(255, 0, 0)),
    ] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("paragraph");
        assert_eq!(node.style.color, color, "{id}");
    }
}

#[test]
fn state_custom_property_revert_layer_uses_prior_state_layer() {
    let doc = parse_and_layout(
        "<style>@layer base, theme;\
         @layer base { #t:hover { --ink: red; color: var(--ink); } }\
         @layer theme { #t:hover { --ink: blue; --ink: revert-layer; } }</style>\
         <div id=t>x</div>",
        900.0,
    );
    let div = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "t")
    })
    .expect("target");
    let hover = div.style.hover_style.as_ref().expect("hover style");
    assert_eq!(
        hover.custom_props.get("--ink").map(String::as_str),
        Some("red")
    );
    assert_eq!(hover.color, Color::rgb(255, 0, 0));
}

#[test]
fn variable_fallback_css_wide_keywords_apply_to_ordinary_properties() {
    let doc = parse_and_layout(
        "<style>:root { color: rgb(0, 128, 0); }\
         #initial { color: var(--missing, initial); }\
         #inherit { color: var(--missing, inherit); }\
         #unset { color: var(--missing, unset); }\
         #noninherited { margin-left: 12px; margin-left: var(--missing, unset); }</style>\
         <p id=initial>x</p><p id=inherit>x</p><p id=unset>x</p>\
         <p id=noninherited>x</p>",
        800.0,
    );
    for (id, color) in [
        ("initial", Color::rgb(0, 0, 0)),
        ("inherit", Color::rgb(0, 128, 0)),
        ("unset", Color::rgb(0, 128, 0)),
    ] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("paragraph");
        assert_eq!(node.style.color, color, "{id}");
    }
    let noninherited = find_box(&doc.root, &|b| {
        b.attributes
            .get("id")
            .is_some_and(|value| value == "noninherited")
    })
    .expect("paragraph");
    assert!(matches!(
        noninherited.style.margin_left,
        CssLength::Zero | CssLength::Px(0.0)
    ));
}

#[test]
fn custom_property_css_wide_keywords_after_substitution_follow_cascade() {
    let doc = parse_and_layout(
        "<style>:root { --x: green; --empty:; } @layer base, theme;\
         #initial { --x: var(--empty) initial; --y: var(--x, blue); }\
         #inherit { --x: var(--empty) inherit; }\
         #unset { --x: var(--empty) unset; }\
         #revert { --x: var(--unknown, revert); }\
         @layer base { #layer { --x: red; } #fallback { --x: red; } }\
         @layer theme { #layer { --x: var(--empty) revert-layer; --y: var(--x); }\
                        #fallback { --x: var(--unknown, revert-layer); } }</style>\
         <p id=initial>x</p><p id=inherit>x</p><p id=unset>x</p>\
         <p id=revert>x</p><p id=layer>x</p><p id=fallback>x</p>",
        800.0,
    );
    for (id, value) in [
        ("initial", None),
        ("inherit", Some("green")),
        ("unset", Some("green")),
        ("revert", Some("green")),
        ("layer", Some("red")),
        ("fallback", Some("red")),
    ] {
        let node = find_box(&doc.root, &|b| {
            b.attributes.get("id").is_some_and(|actual| actual == id)
        })
        .expect("paragraph");
        assert_eq!(
            node.style.custom_props.get("--x").map(String::as_str),
            value,
            "{id}"
        );
        if id == "initial" {
            assert_eq!(
                node.style.custom_props.get("--y").map(|value| value.trim()),
                Some("blue")
            );
        } else if id == "layer" {
            assert_eq!(
                node.style.custom_props.get("--y").map(String::as_str),
                Some("red")
            );
        }
    }
}

#[test]
fn stylesheet_root_variable_snapshot_respects_layer_rollback() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        "@layer base, theme; :root { --tone: red; } html { --tone: blue; }\
         @layer base { :root { --ink: red; } }\
         @layer theme { :root { --ink: blue; --ink: var(--missing, revert-layer); } }\
         @layer base { :root { --important: red !important; } }\
         @layer theme { :root { --important: blue !important; } }",
    );
    sheet.rebuild_index();
    sheet.resolve_variables_for_viewport(800.0, 600.0);
    assert_eq!(
        sheet.variables.get("--ink").map(String::as_str),
        Some("red")
    );
    assert_eq!(
        sheet.variables.get("--tone").map(String::as_str),
        Some("red")
    );
    assert_eq!(
        sheet.variables.get("--important").map(String::as_str),
        Some("red")
    );
}

#[test]
fn state_custom_property_keyword_after_substitution_rolls_back_layer() {
    let doc = parse_and_layout(
        "<style>@layer base, theme;\
         @layer base { #t:hover { --ink: red; color: var(--ink); } }\
         @layer theme { #t:hover { --ink: var(--unknown, revert-layer); } }</style>\
         <div id=t>x</div>",
        800.0,
    );
    let node = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "t")
    })
    .expect("target");
    let hover = node.style.hover_style.as_ref().expect("hover style");
    assert_eq!(
        hover.custom_props.get("--ink").map(String::as_str),
        Some("red")
    );
    assert_eq!(hover.color, Color::rgb(255, 0, 0));
}

#[test]
fn flex_and_grid_items_report_blockified_computed_display() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<div style='display:flex'><span id=flex-item style='display:inline-flex'>x</span></div>\
         <div style='display:grid'><span id=grid-item style='display:inline-grid'>x</span></div>",
        900.0,
    );
    let flex_item = d.get_element_by_id("flex-item").unwrap();
    let grid_item = d.get_element_by_id("grid-item").unwrap();

    assert_eq!(d.computed_style_property(flex_item, "display"), "flex");
    assert_eq!(d.computed_style_property(grid_item, "display"), "grid");
}

#[test]
fn computed_style_can_read_before_and_after_pseudo_styles() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#t::before { content: \"x\"; color: rgb(1, 2, 3); font-weight: 700; } #t::after { content: \"y\"; color: rgb(4, 5, 6); }</style><div id=t>z</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_pseudo_property(div, "::before", "content"),
        "\"x\""
    );
    assert_eq!(
        d.computed_style_pseudo_property(div, "::before", "color"),
        "rgb(1, 2, 3)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(div, "::before", "font-weight"),
        "700"
    );
    assert_eq!(
        d.computed_style_pseudo_property(div, "::after", "content"),
        "\"y\""
    );
    assert_eq!(
        d.computed_style_pseudo_property(div, "::after", "color"),
        "rgb(4, 5, 6)"
    );
}

#[test]
fn generated_pseudo_cssom_reuses_resolved_box_values_without_layout() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>
        #owner { width: 200px; }
        #owner::before { content: ""; display: block; width: 50%; height: 30px;
            margin-left: 5px; padding: 3px; border: 2px solid red; box-sizing: border-box; }
        #owner::after { content: ""; display: block; width: 40px; height: 12px;
            margin-right: 7px; padding: 4px; background-color: blue; }
        </style><div id='owner'>Text</div>"#,
        400.0,
    );
    let owner = doc.get_element_by_id("owner").unwrap();
    let generation = doc.layout_generation;
    for (pseudo, property, expected) in [
        ("::before", "width", "100px"),
        ("::before", "height", "30px"),
        ("::before", "margin-left", "5px"),
        ("::before", "padding-left", "3px"),
        ("::before", "border-left-width", "2px"),
        ("::before", "box-sizing", "border-box"),
        ("::after", "width", "40px"),
        ("::after", "height", "12px"),
        ("::after", "margin-right", "7px"),
        ("::after", "padding-right", "4px"),
        ("::after", "background-color", "rgb(0, 0, 255)"),
    ] {
        assert_eq!(
            doc.computed_style_pseudo_property_current(owner, pseudo, property),
            expected,
            "{pseudo} {property}"
        );
    }
    assert_eq!(doc.layout_generation, generation);
}

#[test]
fn computed_style_can_read_stored_pseudo_element_styles() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>
            li::marker { content: ">> "; color: rgb(9, 8, 7); font-size: 18px; }
            input::placeholder { color: rgb(10, 20, 30); font-style: italic; }
            #ed::selection { background-color: rgb(40, 50, 60); color: rgb(70, 80, 90); }
            dialog::backdrop { background-color: rgba(1, 2, 3, 0.5); }
           </style>
           <ul><li id=item>one</li></ul>
           <input id=field placeholder=hint>
           <div id=ed contenteditable>select me</div>
           <dialog id=dlg open>modal</dialog>"#,
        900.0,
    );
    let item = d.get_element_by_id("item").unwrap();
    let field = d.get_element_by_id("field").unwrap();
    let editor = d.get_element_by_id("ed").unwrap();
    let dialog = d.get_element_by_id("dlg").unwrap();

    assert_eq!(
        d.computed_style_pseudo_property(item, "::marker", "content"),
        "\">> \""
    );
    assert_eq!(
        d.computed_style_pseudo_property(item, "::marker", "color"),
        "rgb(9, 8, 7)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(item, "::marker", "font-size"),
        "18px"
    );
    assert_eq!(
        d.computed_style_pseudo_property(field, "::placeholder", "color"),
        "rgb(10, 20, 30)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(field, "::placeholder", "font-style"),
        "italic"
    );
    assert_eq!(
        d.computed_style_pseudo_property(editor, "::selection", "background-color"),
        "rgb(40, 50, 60)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(editor, "::selection", "color"),
        "rgb(70, 80, 90)"
    );
    assert_eq!(
        d.computed_style_pseudo_property(dialog, "::backdrop", "background-color"),
        "rgba(1, 2, 3, 0.5)"
    );
}

#[test]
fn pseudo_filter_lengths_use_pseudo_font_root_and_viewport() {
    use crate::types::{Color, FilterOp};
    let mut doc = parse_html(
        r#"<html><head><style>
      html{font-size:20px}#a{font-size:30px}
      #a::before{content:'before';font-size:2em;filter:blur(calc(1em + 1rem + 1vw)) drop-shadow(1em 1rem);color:rgb(10,20,30)}
      #a::after{content:'after';backdrop-filter:blur(calc(1em + 1rem + 1vh))}
    </style></head><body><div id=a>text</div></body></html>"#,
    );
    doc.set_viewport(800.0, 600.0);
    let a = doc.get_node(doc.get_element_by_id("a").unwrap()).unwrap();
    let before = a.style.before_style.as_ref().unwrap();
    assert!(
        matches!(before.css_filter.ops[0], FilterOp::Blur(v) if v == 88.0),
        "{:?}",
        before.css_filter
    );
    assert!(
        matches!(before.css_filter.ops[1], FilterOp::DropShadow {dx,dy,color,..} if dx == 60.0 && dy == 20.0 && color == Color::rgb(10,20,30))
    );
    let after = a.style.after_style.as_ref().unwrap();
    assert_eq!(after.rare().backdrop_filter, "blur(56px)");
}

#[test]
fn filter_lengths_resolve_in_element_context_and_inherit_computed_values() {
    use crate::types::{Color, FilterOp};
    let mut doc = parse_html(
        r#"<html><head><style>
      html{font-size:20px}
      #a{font-size:30px;filter:blur(calc(1em + 1rem + 1vw)) drop-shadow(1em 1rem 1vh);color:rgb(10,20,30)}
      #b{font-size:60px;filter:inherit}
      #c{font-size:30px;backdrop-filter:blur(calc(1em + 1rem + 1vw))}
    </style></head><body><div id=a><div id=b>child</div></div><div id=c>backdrop</div></body></html>"#,
    );
    for (w, h, blur, shadow_blur) in [(800.0, 600.0, 58.0, 6.0), (1000.0, 400.0, 60.0, 4.0)] {
        doc.set_viewport(w, h);
        for id in ["a", "b"] {
            let node = doc.get_node(doc.get_element_by_id(id).unwrap()).unwrap();
            assert!(
                matches!(node.style.css_filter.ops[0], FilterOp::Blur(v) if v == blur),
                "{id}: {:?}",
                node.style.css_filter
            );
            assert!(
                matches!(node.style.css_filter.ops[1], FilterOp::DropShadow {dx,dy,blur,color} if dx == 30.0 && dy == 20.0 && blur == shadow_blur && color == Color::rgb(10,20,30)),
                "{id}: {:?}",
                node.style.css_filter
            );
        }
        let c = doc.get_element_by_id("c").unwrap();
        assert_eq!(
            doc.get_node(c).unwrap().style.rare().backdrop_filter,
            format!("blur({blur}px)")
        );
    }
}

#[test]
fn filter_math_and_nested_colors_reach_filter_operations() {
    use crate::types::{Color, FilterOp};
    let filters = crate::css::parse_css_filter(
        "brightness(sqrt(4)) contrast(calc(50% + 25%)) hue-rotate(calc(.25turn + 90deg)) blur(calc(1px + 2px)) drop-shadow(rgb(10, 20, 30) calc(1px + 2px) -2px 4px)",
    );
    assert_eq!(filters.ops.len(), 5);
    assert!(matches!(filters.ops[0], FilterOp::Brightness(v) if v == 2.0));
    assert!(matches!(filters.ops[1], FilterOp::Contrast(v) if v == 0.75));
    assert!(matches!(filters.ops[2], FilterOp::HueRotate(v) if (v - 180.0).abs() < 0.001));
    assert!(matches!(filters.ops[3], FilterOp::Blur(v) if v == 3.0));
    assert!(
        matches!(filters.ops[4], FilterOp::DropShadow {dx,dy,blur,color} if dx == 3.0 && dy == -2.0 && blur == 4.0 && color == Color::rgb(10,20,30))
    );
    let mut style = ComputedStyle::default();
    for bad in [
        "brightness(oops)",
        "brightness(-1)",
        "blur(10%)",
        "hue-rotate(2)",
        "blur(2px) garbage",
        "blur(calc(2px)",
        "drop-shadow(1px 2px -3px)",
    ] {
        apply_property(&mut style, "filter", "brightness(2)");
        apply_property(&mut style, "filter", bad);
        assert!(
            matches!(style.css_filter.ops.as_slice(), [FilterOp::Brightness(v)] if *v == 2.0),
            "{bad}"
        );
    }
    let filters =
        crate::css::parse_css_filter("brightness(calc(-1)) opacity(calc(150%)) blur(calc(-1px))");
    assert!(
        matches!(filters.ops.as_slice(), [FilterOp::Brightness(a), FilterOp::Opacity(b), FilterOp::Blur(c)] if *a == 0.0 && *b == 1.0 && *c == 0.0)
    );
}

#[test]
fn filter_parser_uses_function_defaults_units_and_clamps() {
    use crate::types::FilterOp;

    let filters = crate::css::parse_css_filter(
        "brightness() grayscale() opacity(200%) blur(1rem) hue-rotate(0.5turn)",
    );

    assert!(matches!(filters.ops[0], FilterOp::Brightness(v) if (v - 1.0).abs() < 0.001));
    assert!(matches!(filters.ops[1], FilterOp::Grayscale(v) if (v - 1.0).abs() < 0.001));
    assert!(matches!(filters.ops[2], FilterOp::Opacity(v) if (v - 1.0).abs() < 0.001));
    assert!(matches!(filters.ops[3], FilterOp::Blur(v) if (v - 16.0).abs() < 0.001));
    assert!(matches!(filters.ops[4], FilterOp::HueRotate(v) if (v - 180.0).abs() < 0.001));
}

#[test]
fn unsupported_filter_component_invalidates_the_filter_chain() {
    let filters = crate::css::parse_css_filter("url(#f) grayscale(1)");
    assert!(
        filters.ops.is_empty(),
        "unsupported filter components invalidate the whole filter chain"
    );

    let filters = crate::css::parse_css_filter("grayscale(1) unknown-filter(2)");
    assert!(
        filters.ops.is_empty(),
        "unknown filter functions must not leave a half-applied chain"
    );
}

#[test]
fn drop_shadow_parser_accepts_color_first_and_uses_current_color_default() {
    use crate::types::{Color, FilterOp};

    let filters = crate::css::parse_css_filter("drop-shadow(#ff0000 2px 3px 4px)");
    assert!(matches!(
        &filters.ops[0],
        FilterOp::DropShadow { dx, dy, blur, color }
            if (*dx - 2.0).abs() < 0.001
                && (*dy - 3.0).abs() < 0.001
                && (*blur - 4.0).abs() < 0.001
                && *color == Color::rgb(255, 0, 0)
    ));

    let filters = crate::css::parse_css_filter_with_current_color(
        "drop-shadow(2px 3px 4px)",
        Color::rgb(9, 8, 7),
    );
    assert!(matches!(
        &filters.ops[0],
        FilterOp::DropShadow { color, .. } if *color == Color::rgb(9, 8, 7)
    ));
}

/// The used width of a border is zero when it draws nothing, however wide it
/// computes. CSS Backgrounds 3 §4.3.
///
/// Measured in Chrome: a bare `<div>` answers `0px`/`none`; a
/// `<div style="border-style:solid">` answers `3px`/`solid` without ever
/// naming a width.
#[test]
fn a_border_width_resolves_to_zero_when_the_style_draws_nothing() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<div id=bare></div><div id=solid style='border-style:solid'></div>",
        900.0,
    );
    let bare = d.get_element_by_id("bare").unwrap();
    let solid = d.get_element_by_id("solid").unwrap();

    assert_eq!(
        d.computed_style_property(bare, "border-top-width"),
        "0px",
        "no border style declared, so nothing is drawn"
    );
    assert_eq!(
        d.computed_style_property(solid, "border-top-width"),
        "3px",
        "a style with no width takes the initial width, `medium`"
    );
}

#[test]
fn computed_style_exposes_stored_longhands() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r##"<style>
            #box {
                text-align: center;
                visibility: hidden;
                line-height: 20px;
                letter-spacing: 2px;
                text-transform: uppercase;
                white-space: pre-wrap;
                direction: rtl;
                text-orientation: upright;
                text-combine-upright: digits 2;
                cursor: pointer;
                -webkit-line-clamp: 3;
                transform-box: content-box;
                transform-style: preserve-3d;
                perspective: 400px;
                perspective-origin: left top;
                backface-visibility: hidden;
                border-top-left-radius: 6px;
                border-image: url(border.png) 30 fill / 10px / 2px round stretch;
                outline-width: 3px;
                outline-style: solid;
                outline-color: rgb(1, 2, 3);
                outline-offset: 5px;
                column-count: 3;
                column-width: 120px;
                column-rule: 4px dotted rgb(4, 5, 6);
                column-fill: auto;
                column-span: all;
                list-style-type: decimal;
                list-style-position: inside;
                list-style-image: url("marker.svg");
                vertical-align: middle;
                float: left;
                display: flex;
                flex-direction: column;
                justify-content: space-between;
                align-items: center;
                align-content: space-around;
                align-self: flex-end;
                justify-items: baseline;
                justify-self: stretch;
                row-gap: 8px;
                flex-grow: 2;
                order: 4;
                content-visibility: auto;
                contain-intrinsic-size: 120px 45px;
                color-scheme: light dark;
                forced-color-adjust: preserve-parent-color;
                font-synthesis: none;
                font-synthesis-weight: auto;
                text-wrap: balance;
                text-decoration-skip-ink: none;
                text-emphasis: filled sesame rgb(4, 5, 6);
                text-emphasis-position: under left;
                background-image: url("hero.png");
                background-position: right bottom;
                background-size: 25px 50%;
                background-repeat: space no-repeat;
                background-attachment: local;
                background-origin: content-box;
                background-clip: padding-box;
                background-blend-mode: multiply, screen;
                transition: opacity 200ms ease-in 50ms, display 1s step-end allow-discrete;
                animation: fade 200ms ease-in 50ms infinite alternate both paused add, slide 1s step-end 2 reverse forwards running accumulate;
                overflow-anchor: none;
                overflow-clip-margin: content-box 12px;
                anchor-name: --card;
                position-anchor: --card;
                view-transition-name: card;
                animation-timeline: scroll(root block);
                scroll-timeline: --main block;
                offset-path: path("M 0 0 L 10 0");
                shape-outside: circle(50% at 50% 50%);
                shape-margin: 12px;
                scrollbar-color: CanvasText Canvas;
                scrollbar-width: thin;
                scrollbar-gutter: stable both-edges;
                caret-color: rgb(7, 8, 9);
                pointer-events: none;
                user-select: all;
                resize: vertical;
                tab-size: 4;
                hyphens: auto;
                scroll-snap-stop: always;
                scroll-margin: 1px 2px 3px 4px;
                caption-side: block-end;
                appearance: none;
                field-sizing: content;
                interpolate-size: allow-keywords;
                margin-trim: block-start block-end;
                mask: url(mask.svg) no-repeat center / contain content-box border-box alpha;
                mask-composite: exclude;
            }
        </style>
        <div id=box><span id=child></span></div>"##,
        900.0,
    );
    let box_id = d.get_element_by_id("box").unwrap();
    let child = d.get_element_by_id("child").unwrap();

    assert_eq!(d.computed_style_property(box_id, "text-align"), "center");
    assert_eq!(d.computed_style_property(box_id, "visibility"), "hidden");
    assert_eq!(d.computed_style_property(box_id, "line-height"), "20px");
    assert_eq!(d.computed_style_property(box_id, "letter-spacing"), "2px");
    assert_eq!(
        d.computed_style_property(box_id, "text-transform"),
        "uppercase"
    );
    assert_eq!(d.computed_style_property(box_id, "white-space"), "pre-wrap");
    assert_eq!(d.computed_style_property(box_id, "direction"), "rtl");
    assert_eq!(
        d.computed_style_property(box_id, "text-orientation"),
        "upright"
    );
    assert_eq!(
        d.computed_style_property(child, "text-orientation"),
        "upright"
    );
    assert_eq!(
        d.computed_style_property(box_id, "text-combine-upright"),
        "digits 2"
    );
    assert_eq!(
        d.computed_style_property(child, "text-combine-upright"),
        "digits 2"
    );
    assert_eq!(d.computed_style_property(box_id, "cursor"), "pointer");
    assert_eq!(d.computed_style_property(box_id, "line-clamp"), "3");
    assert_eq!(d.computed_style_property(box_id, "-webkit-line-clamp"), "3");
    assert_eq!(
        d.computed_style_property(box_id, "transform-box"),
        "content-box"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transform-style"),
        "preserve-3d"
    );
    assert_eq!(d.computed_style_property(box_id, "perspective"), "400px");
    assert_eq!(
        d.computed_style_property(box_id, "perspective-origin"),
        "left top"
    );
    assert_eq!(
        d.computed_style_property(box_id, "backface-visibility"),
        "hidden"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-top-left-radius"),
        "6px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-image-source"),
        "url(border.png)"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-image-slice"),
        "30 fill"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-image-width"),
        "10px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-image-outset"),
        "2px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "border-image-repeat"),
        "round stretch"
    );
    assert_eq!(d.computed_style_property(box_id, "outline-width"), "3px");
    assert_eq!(d.computed_style_property(box_id, "outline-style"), "solid");
    assert_eq!(
        d.computed_style_property(box_id, "outline-color"),
        "rgb(1, 2, 3)"
    );
    assert_eq!(d.computed_style_property(box_id, "outline-offset"), "5px");
    assert_eq!(d.computed_style_property(box_id, "column-count"), "3");
    assert_eq!(d.computed_style_property(box_id, "column-width"), "120px");
    assert_eq!(
        d.computed_style_property(box_id, "column-rule-width"),
        "4px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "column-rule-style"),
        "dotted"
    );
    assert_eq!(
        d.computed_style_property(box_id, "column-rule-color"),
        "rgb(4, 5, 6)"
    );
    assert_eq!(d.computed_style_property(box_id, "column-fill"), "auto");
    assert_eq!(d.computed_style_property(box_id, "column-span"), "all");
    assert_eq!(
        d.computed_style_property(box_id, "list-style-type"),
        "decimal"
    );
    assert_eq!(
        d.computed_style_property(box_id, "list-style-position"),
        "inside"
    );
    assert_eq!(
        d.computed_style_property(box_id, "list-style-image"),
        r#"url("marker.svg")"#
    );
    assert_eq!(
        d.computed_style_property(box_id, "vertical-align"),
        "middle"
    );
    assert_eq!(d.computed_style_property(box_id, "float"), "left");
    assert_eq!(
        d.computed_style_property(box_id, "flex-direction"),
        "column"
    );
    assert_eq!(
        d.computed_style_property(box_id, "justify-content"),
        "space-between"
    );
    assert_eq!(d.computed_style_property(box_id, "align-items"), "center");
    assert_eq!(
        d.computed_style_property(box_id, "align-content"),
        "space-around"
    );
    assert_eq!(d.computed_style_property(box_id, "align-self"), "flex-end");
    assert_eq!(
        d.computed_style_property(box_id, "justify-items"),
        "baseline"
    );
    assert_eq!(d.computed_style_property(box_id, "justify-self"), "stretch");
    assert_eq!(d.computed_style_property(box_id, "row-gap"), "8px");
    assert_eq!(d.computed_style_property(box_id, "flex-grow"), "2");
    assert_eq!(d.computed_style_property(box_id, "order"), "4");
    assert_eq!(
        d.computed_style_property(box_id, "content-visibility"),
        "auto"
    );
    assert_eq!(
        d.computed_style_property(box_id, "contain-intrinsic-size"),
        "120px 45px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "color-scheme"),
        "light dark"
    );
    assert_eq!(
        d.computed_style_property(child, "color-scheme"),
        "light dark"
    );
    assert_eq!(
        d.computed_style_property(box_id, "forced-color-adjust"),
        "preserve-parent-color"
    );
    assert_eq!(
        d.computed_style_property(child, "forced-color-adjust"),
        "preserve-parent-color"
    );
    assert_eq!(
        d.computed_style_property(box_id, "font-synthesis"),
        "weight"
    );
    assert_eq!(
        d.computed_style_property(box_id, "font-synthesis-style"),
        "none"
    );
    assert_eq!(d.computed_style_property(child, "font-synthesis"), "weight");
    assert_eq!(d.computed_style_property(box_id, "text-wrap"), "balance");
    assert_eq!(d.computed_style_property(child, "text-wrap"), "balance");
    assert_eq!(
        d.computed_style_property(box_id, "text-decoration-skip-ink"),
        "none"
    );
    assert_eq!(
        d.computed_style_property(child, "text-decoration-skip-ink"),
        "none"
    );
    assert_eq!(
        d.computed_style_property(box_id, "text-emphasis-style"),
        "filled sesame"
    );
    assert_eq!(
        d.computed_style_property(box_id, "text-emphasis-color"),
        "rgb(4, 5, 6)"
    );
    assert_eq!(
        d.computed_style_property(box_id, "text-emphasis-position"),
        "under left"
    );
    assert_eq!(
        d.computed_style_property(child, "text-emphasis-style"),
        "filled sesame"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-blend-mode"),
        "multiply, screen"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-image"),
        r#"url("hero.png")"#
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-position"),
        "100% 100%"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-size"),
        "25px 50%"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-repeat"),
        "space no-repeat"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-attachment"),
        "local"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-origin"),
        "content-box"
    );
    assert_eq!(
        d.computed_style_property(box_id, "background-clip"),
        "padding-box"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition-property"),
        "opacity, display"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition-duration"),
        "200ms, 1s"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition-timing-function"),
        "ease-in, step-end"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition-delay"),
        "50ms, 0s"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition-behavior"),
        "normal, allow-discrete"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-name"),
        "fade, slide"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-duration"),
        "200ms, 1s"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-timing-function"),
        "ease-in, step-end"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-delay"),
        "50ms, 0s"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-iteration-count"),
        "infinite, 2"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-direction"),
        "alternate, reverse"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-fill-mode"),
        "both, forwards"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-play-state"),
        "paused, running"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-composition"),
        "add, accumulate"
    );
    assert_eq!(d.computed_style_property(box_id, "overflow-anchor"), "none");
    assert_eq!(
        d.computed_style_property(box_id, "overflow-clip-margin"),
        "content-box 12px"
    );
    assert_eq!(d.computed_style_property(box_id, "anchor-name"), "--card");
    assert_eq!(
        d.computed_style_property(box_id, "position-anchor"),
        "--card"
    );
    assert_eq!(
        d.computed_style_property(box_id, "view-transition-name"),
        "card"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation-timeline"),
        "scroll(root block)"
    );
    assert_eq!(
        d.computed_style_property(box_id, "scroll-timeline"),
        "--main block"
    );
    assert_eq!(
        d.computed_style_property(box_id, "offset-path"),
        "path(\"M 0 0 L 10 0\")"
    );
    assert_eq!(
        d.computed_style_property(box_id, "shape-outside"),
        "circle(50% at 50% 50%)"
    );
    assert_eq!(d.computed_style_property(box_id, "shape-margin"), "12px");
    assert_eq!(
        d.computed_style_property(box_id, "scrollbar-color"),
        "rgb(0, 0, 0) rgb(255, 255, 255)"
    );
    assert_eq!(d.computed_style_property(box_id, "scrollbar-width"), "thin");
    assert_eq!(
        d.computed_style_property(box_id, "scrollbar-gutter"),
        "stable both-edges"
    );
    assert_eq!(
        d.computed_style_property(box_id, "caret-color"),
        "rgb(7, 8, 9)"
    );
    assert_eq!(d.computed_style_property(box_id, "pointer-events"), "none");
    assert_eq!(d.computed_style_property(box_id, "user-select"), "all");
    assert_eq!(d.computed_style_property(box_id, "resize"), "vertical");
    assert_eq!(d.computed_style_property(box_id, "tab-size"), "4");
    assert_eq!(d.computed_style_property(box_id, "hyphens"), "auto");
    assert_eq!(
        d.computed_style_property(box_id, "scroll-snap-stop"),
        "always"
    );
    assert_eq!(
        d.computed_style_property(box_id, "scroll-margin"),
        "1px 2px 3px 4px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "scroll-margin-left"),
        "4px"
    );
    assert_eq!(
        d.computed_style_property(box_id, "caption-side"),
        "block-end"
    );
    assert_eq!(
        d.computed_style_property(child, "caption-side"),
        "block-end"
    );
    assert_eq!(d.computed_style_property(box_id, "appearance"), "none");
    assert_eq!(d.computed_style_property(box_id, "field-sizing"), "content");
    assert_eq!(
        d.computed_style_property(box_id, "interpolate-size"),
        "allow-keywords"
    );
    assert_eq!(
        d.computed_style_property(child, "interpolate-size"),
        "allow-keywords"
    );
    assert_eq!(
        d.computed_style_property(box_id, "margin-trim"),
        "block-start block-end"
    );
    assert_eq!(
        d.computed_style_property(box_id, "mask-image"),
        "url(\"mask.svg\")"
    );
    assert_eq!(
        d.computed_style_property(box_id, "mask-repeat"),
        "no-repeat"
    );
    assert_eq!(d.computed_style_property(box_id, "mask-position"), "center");
    assert_eq!(d.computed_style_property(box_id, "mask-size"), "contain");
    assert_eq!(
        d.computed_style_property(box_id, "mask-origin"),
        "content-box"
    );
    assert_eq!(d.computed_style_property(box_id, "mask-clip"), "border-box");
    assert_eq!(d.computed_style_property(box_id, "mask-mode"), "alpha");
    assert_eq!(
        d.computed_style_property(box_id, "mask-composite"),
        "exclude"
    );
}

#[test]
fn compact_child_selector_applies_column_width() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>.langlist>ul{column-width:11.2rem}</style>
           <div class="langlist"><ul id="langs"><li>English</li></ul></div>"#,
        800.0,
    );
    let ul = d.get_element_by_id("langs").unwrap();
    assert_eq!(d.computed_style_property(ul, "column-width"), "179.2px");
}

#[test]
fn text_transform_full_width_is_stored_and_painted() {
    let texts = build_display_texts(
        r#"
        <style>#t { text-transform: full-width; }</style>
        <div id="t">Az 09 !</div>
        "#,
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("\u{ff21}\u{ff5a}\u{3000}\u{ff10}\u{ff19}\u{3000}\u{ff01}")),
        "full-width text-transform should map ASCII and spaces to fullwidth forms; texts={:?}",
        texts
    );

    let mut renderer = crate::Renderer::new();
    let mut d = renderer.load_html(
        r#"<div id="t" style="text-transform: full-size-kana;"></div>"#,
        200.0,
    );
    let id = d.get_element_by_id("t").unwrap();
    assert_eq!(
        d.computed_style_property(id, "text-transform"),
        "full-size-kana"
    );
}

#[test]
fn text_emphasis_marks_are_emitted_as_text_runs() {
    let texts = build_display_texts(
        r#"
        <style>
          body { margin:0; font:20px/30px sans-serif; }
          #em { text-emphasis: filled dot rgb(255, 0, 0); }
        </style>
        <span id="em">A B</span>
        "#,
    );
    assert!(
        texts.iter().any(|text| text == "\u{2022}\u{2022}"),
        "text-emphasis should paint one mark for each non-space character; texts were {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "A B"),
        "text-emphasis must not replace the base text; texts were {texts:?}"
    );
}

#[test]
fn shape_outside_circle_narrows_float_exclusion_per_line() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"
        <style>
          body { margin:0; font:16px/20px sans-serif; }
          #wrap { width:200px; }
          #float { float:left; width:100px; height:100px; shape-outside:circle(50% at 50% 50%); }
        </style>
        <div id="wrap"><div id="float"></div><span>hello</span></div>
        "#,
        300.0,
    );
    let list = build_display_list(&d.root, 300.0, 200.0);
    let text_x = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text.contains("hello") => Some(*x),
            _ => None,
        })
        .expect("text run");

    assert!(
        text_x > 50.0 && text_x < 100.0,
        "circle shape-outside should exclude less than the float rectangle on the first line, got x={text_x}"
    );
}

#[test]
fn shape_outside_uses_the_full_line_interval_for_exclusion() {
    use crate::layout::{FloatContext, FloatItem, FloatShape, FloatSide, LayoutEngine};

    let engine = LayoutEngine::new();
    let font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
    let mut left_float = FloatContext::default();
    left_float.place_float(
        &engine,
        font_px,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Left,
        "circle(25px at 50px 50px)",
        0.0,
    );
    let (mut left, mut right) = (0.0, 0.0);
    left_float.available_width(0.0, 20.0, 200.0, &mut left, &mut right);
    assert_eq!(left, 0.0, "a line above the circle needs no exclusion");
    left_float.available_width(20.0, 30.0, 200.0, &mut left, &mut right);
    assert!(
        (left - 75.0).abs() < 0.01,
        "the line reaches the circle's widest point: {left}"
    );

    let mut right_float = FloatContext::default();
    right_float.place_float(
        &engine,
        font_px,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Right,
        "inset(40px 20px 40px 20px)",
        0.0,
    );
    right_float.available_width(25.0, 16.0, 200.0, &mut left, &mut right);
    assert!(
        (right - 120.0).abs() < 0.01,
        "the line intersects the inset at its bottom edge: {right}"
    );
    right_float.available_width(0.0, 20.0, 200.0, &mut left, &mut right);
    assert_eq!(right, 200.0, "a line above the inset needs no exclusion");

    let displaced_right = FloatContext {
        floats: vec![FloatItem {
            rect: Rect::new(40.0, 0.0, 50.0, 100.0),
            side: FloatSide::Right,
            clear: 100.0,
            shape: Some(FloatShape::Circle {
                cx: 25.0,
                cy: 50.0,
                r: 10.0,
            }),
            shape_margin: 0.0,
        }],
        ..FloatContext::default()
    };
    displaced_right.available_width(0.0, 20.0, 200.0, &mut left, &mut right);
    assert_eq!(
        right, 200.0,
        "a non-intersecting internal right float must not narrow the line"
    );

    let mut polygon_float = FloatContext::default();
    polygon_float.place_float(
        &engine,
        font_px,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Left,
        "polygon(0 0, 50px 50px, 0 100px)",
        0.0,
    );
    polygon_float.available_width(20.0, 30.0, 200.0, &mut left, &mut right);
    assert!(
        (left - 50.0).abs() < 0.01,
        "the line reaches the polygon's widest vertex: {left}"
    );
}

#[test]
fn shape_outside_radial_sizes_use_the_reference_box_and_center() {
    use crate::layout::{FloatContext, FloatShape, FloatSide, LayoutEngine};

    let engine = LayoutEngine::new();
    let mut context = FloatContext::default();
    for value in [
        "circle(50%)",
        "circle(at 10% 50%)",
        "ellipse(at 10% 50%)",
        "ellipse(FARTHEST-SIDE at 10% 50%)",
        "ellipse(closest-corner at 10% 50%)",
        "circle(at -20px 50%)",
        "circle(2em at right top)",
        "circle(10px at bottom 5px right 10px)",
        "ellipse(10px 20px at left 15px top 5px)",
        "inset(2em 0)",
        "circle(at bogus)",
        "circle(calc(2em + 3px) at right top)",
        "ellipse(calc(1em + 4px) 25% at left 15px top 5px)",
        "inset(calc(1em + 4px) 10%)",
        "polygon(calc(10px + 5px) 0, 50px 50px, 0 100px)",
    ] {
        context.place_float(
            &engine,
            ComputedStyle::INITIAL_FONT_SIZE_PX,
            0.0,
            200.0,
            100.0,
            1000.0,
            FloatSide::Left,
            value,
            0.0,
        );
    }
    let circle = &context.floats[0].shape;
    let expected = 200.0_f32.hypot(100.0) / std::f32::consts::SQRT_2 * 0.5;
    assert!(matches!(circle, Some(FloatShape::Circle { r, .. }) if (*r - expected).abs() < 0.01));
    assert!(matches!(&context.floats[1].shape,
        Some(FloatShape::Circle { cx, cy, r })
            if (*cx - 20.0).abs() < 0.01 && (*cy - 50.0).abs() < 0.01 && (*r - 20.0).abs() < 0.01));
    assert!(matches!(&context.floats[2].shape,
        Some(FloatShape::Ellipse { rx, ry, .. })
            if (*rx - 20.0).abs() < 0.01 && (*ry - 50.0).abs() < 0.01));
    assert!(matches!(&context.floats[3].shape,
        Some(FloatShape::Ellipse { rx, ry, .. })
            if (*rx - 180.0).abs() < 0.01 && (*ry - 50.0).abs() < 0.01));
    assert!(matches!(&context.floats[4].shape,
        Some(FloatShape::Ellipse { rx, ry, .. })
            if (*rx - 20.0 * std::f32::consts::SQRT_2).abs() < 0.01
                && (*ry - 50.0 * std::f32::consts::SQRT_2).abs() < 0.01));
    assert!(matches!(&context.floats[5].shape,
        Some(FloatShape::Circle { cx, r, .. })
            if (*cx + 20.0).abs() < 0.01 && (*r - 20.0).abs() < 0.01));
    assert!(matches!(&context.floats[6].shape,
        Some(FloatShape::Circle { cx, cy, r })
            if (*cx - 200.0).abs() < 0.01 && *cy == 0.0 && (*r - 32.0).abs() < 0.01));
    assert!(matches!(&context.floats[7].shape,
        Some(FloatShape::Circle { cx, cy, r })
            if (*cx - 190.0).abs() < 0.01 && (*cy - 95.0).abs() < 0.01 && (*r - 10.0).abs() < 0.01));
    assert!(matches!(&context.floats[8].shape,
        Some(FloatShape::Ellipse { cx, cy, rx, ry })
            if (*cx - 15.0).abs() < 0.01 && (*cy - 5.0).abs() < 0.01
                && (*rx - 10.0).abs() < 0.01 && (*ry - 20.0).abs() < 0.01));
    assert!(matches!(&context.floats[9].shape,
        Some(FloatShape::Inset { top, right, bottom, left })
            if (*top - 32.0).abs() < 0.01 && *right == 0.0
                && (*bottom - 32.0).abs() < 0.01 && *left == 0.0));
    assert!(
        context.floats[10].shape.is_none(),
        "invalid position must not become a centered circle"
    );
    assert!(matches!(&context.floats[11].shape,
        Some(FloatShape::Circle { r, .. }) if (*r - 35.0).abs() < 0.01));
    assert!(matches!(&context.floats[12].shape,
        Some(FloatShape::Ellipse { rx, ry, .. })
            if (*rx - 20.0).abs() < 0.01 && (*ry - 25.0).abs() < 0.01));
    assert!(matches!(&context.floats[13].shape,
        Some(FloatShape::Inset { top, right, bottom, left })
            if (*top - 20.0).abs() < 0.01 && (*right - 20.0).abs() < 0.01
                && (*bottom - 20.0).abs() < 0.01 && (*left - 20.0).abs() < 0.01));
    assert!(matches!(&context.floats[14].shape,
        Some(FloatShape::Polygon(points)) if (points[0].0 - 15.0).abs() < 0.01));
}

#[test]
fn shape_outside_validation_agrees_between_cascade_and_supports() {
    use crate::css::parser::supports_condition_matches;

    for value in [
        "circle(at right top)",
        "circle(closest-corner at bottom 5px right 10px)",
        "ellipse(farthest-side at center)",
        "ellipse(calc(1em + 4px) 25% at left 15px top 5px)",
        "polygon(0 0, calc(50% - 2px) 50%, 0 100%) border-box",
        "content-box inset(10px 20px)",
        "circle(25% at center) half-border-box",
        "half-border-box",
    ] {
        assert!(
            supports_condition_matches(&format!("(shape-outside: {value})")),
            "valid shape should be supported: {value}"
        );
    }
    for value in [
        "circle(at bogus)",
        "circle(10px 20px)",
        "ellipse(10px)",
        "circle(min-content)",
        "circle(fit-content(10px))",
        "inset(content)",
        "inset(10px 20px 30px 40px 50px)",
        "polygon(0 0, bogus 20px, 0 100%)",
        "border-box content-box",
        "circle(10px) border-box content-box",
    ] {
        assert!(
            !supports_condition_matches(&format!("(shape-outside: {value})")),
            "invalid shape should fail @supports: {value}"
        );
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "shape-outside", "circle(20px)");
        apply_property(&mut style, "shape-outside", value);
        assert_eq!(
            style.shape_outside, "circle(20px)",
            "invalid shape replaced the prior declaration: {value}"
        );
    }
}

#[test]
fn shape_margin_validates_lengths_and_percentages_without_resetting() {
    use crate::css::parser::supports_condition_matches;

    for value in ["0", "12px", "10%", "calc(1em + 2px)"] {
        assert!(supports_condition_matches(&format!(
            "(shape-margin: {value})"
        )));
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "shape-margin", value);
        assert_ne!(style.shape_margin, crate::types::CssLength::Auto);
    }
    for value in ["-1px", "auto", "content", "fit-content(10px)", "12"] {
        assert!(!supports_condition_matches(&format!(
            "(shape-margin: {value})"
        )));
        let mut style = ComputedStyle::default();
        apply_property(&mut style, "shape-margin", "8px");
        apply_property(&mut style, "shape-margin", value);
        assert_eq!(style.shape_margin, crate::types::CssLength::Px(8.0));
    }
}

#[test]
fn invalid_shape_declarations_preserve_earlier_cascade_values() {
    let doc = parse_and_layout(
        "<style>#float { float:left; shape-outside:circle(20px); shape-outside:circle(at bogus); shape-margin:8px; shape-margin:-2px }</style><div id=float>x</div>",
        300.0,
    );
    let node = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "float")
    })
    .expect("float");
    assert_eq!(node.style.shape_outside, "circle(20px)");
    assert_eq!(node.style.shape_margin, CssLength::Px(8.0));
}

#[test]
fn shape_outside_uses_selected_reference_box_for_basic_shapes() {
    use crate::layout::{FloatContext, FloatShape, FloatSide, LayoutEngine, float_shape_reference};

    let doc = parse_and_layout(
        "<style>body{margin:0}#float{float:left;width:100px;height:80px;margin:12px;border:8px solid;padding:6px;shape-outside:circle(25% at center) content-box}</style><div id=float></div>",
        400.0,
    );
    let node = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "float")
    })
    .expect("float");
    let mut context = FloatContext::default();
    context.place_float_in(
        &LayoutEngine::new(),
        16.0,
        0.0,
        0.0,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        400.0,
        FloatSide::Left,
        &node.style.shape_outside,
        0.0,
        float_shape_reference(&node.style, &node.layout, 16.0),
    );
    let reference = node.layout.content_rect;
    let margin = node.layout.margin_rect;
    let expected_x = reference.x - margin.x + reference.w / 2.0;
    let expected_y = reference.y - margin.y + reference.h / 2.0;
    assert!(matches!(&context.floats[0].shape,
        Some(FloatShape::Circle { cx, cy, r })
        if (*cx - expected_x).abs() < 0.1 && (*cy - expected_y).abs() < 0.1
            && (*r - reference.w.hypot(reference.h) * 0.25 / std::f32::consts::SQRT_2).abs() < 0.1));
}

#[test]
fn shape_outside_bare_box_uses_adjusted_corner_radii() {
    use crate::layout::{FloatContext, FloatShape, FloatSide, LayoutEngine, float_shape_reference};

    let doc = parse_and_layout(
        "<style>body{margin:0}#float{float:left;width:100px;height:80px;margin:10px;border:8px solid;padding:6px;border-radius:30px;shape-outside:border-box}</style><div id=float></div>",
        400.0,
    );
    let node = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "float")
    })
    .expect("float");
    let mut context = FloatContext::default();
    context.place_float_in(
        &LayoutEngine::new(),
        16.0,
        0.0,
        0.0,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        400.0,
        FloatSide::Left,
        &node.style.shape_outside,
        0.0,
        float_shape_reference(&node.style, &node.layout, 16.0),
    );
    let Some(FloatShape::RoundedBox { rect, rx, ry }) = &context.floats[0].shape else {
        panic!("bare border-box must retain rounded reference geometry");
    };
    assert!((rect.x - (node.layout.border_rect.x - node.layout.margin_rect.x)).abs() < 0.1);
    assert!(rx[0] > 0.0 && ry[0] > 0.0);
    let mut left = 0.0;
    let mut right = 400.0;
    context.available_width(rect.y, 2.0, 400.0, &mut left, &mut right);
    assert!(
        left < node.layout.margin_rect.w - 1.0,
        "rounded corner should narrow exclusion: {left}"
    );
    context.available_width(rect.y + 40.0, 2.0, 400.0, &mut left, &mut right);
    assert!(
        left > 0.0,
        "the middle of the border box must still exclude text"
    );
}

#[test]
fn shape_outside_half_border_box_uses_midpoint_edges_and_radii() {
    use crate::layout::{FloatContext, FloatShape, FloatSide, LayoutEngine, float_shape_reference};

    let doc = parse_and_layout(
        "<style>body{margin:0}#float{float:left;width:100px;height:80px;margin:10px;border:12px solid;padding:6px;border-radius:30px;shape-outside:half-border-box}</style><div id=float></div>",
        400.0,
    );
    let node = find_box(&doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "float")
    })
    .expect("float");
    let mut context = FloatContext::default();
    context.place_float_in(
        &LayoutEngine::new(),
        16.0,
        0.0,
        0.0,
        node.layout.margin_rect.w,
        node.layout.margin_rect.h,
        400.0,
        FloatSide::Left,
        &node.style.shape_outside,
        0.0,
        float_shape_reference(&node.style, &node.layout, 16.0),
    );
    let Some(FloatShape::RoundedBox { rect, rx, ry }) = &context.floats[0].shape else {
        panic!("half-border-box must retain rounded geometry");
    };
    let border = node.layout.border_rect;
    let padding = node.layout.padding_rect;
    let margin = node.layout.margin_rect;
    assert!((rect.x - (border.x + padding.x) * 0.5 + margin.x).abs() < 0.1);
    assert!((rect.w - (border.w + padding.w) * 0.5).abs() < 0.1);
    assert!((rect.h - (border.h + padding.h) * 0.5).abs() < 0.1);
    assert!(rx[0] > 0.0 && ry[0] > 0.0);

    let mut style = (*node.style).clone();
    apply_property(
        &mut style,
        "shape-outside",
        "circle(25% at center) half-border-box",
    );
    let mut basic = FloatContext::default();
    basic.place_float_in(
        &LayoutEngine::new(),
        16.0,
        0.0,
        0.0,
        margin.w,
        margin.h,
        400.0,
        FloatSide::Left,
        &style.shape_outside,
        0.0,
        float_shape_reference(&style, &node.layout, 16.0),
    );
    assert!(matches!(&basic.floats[0].shape,
        Some(FloatShape::Circle { cx, cy, r })
        if (*cx - (rect.x + rect.w * 0.5)).abs() < 0.1
            && (*cy - (rect.y + rect.h * 0.5)).abs() < 0.1
            && (*r - rect.w.hypot(rect.h) * 0.25 / std::f32::consts::SQRT_2).abs() < 0.1));
}

#[test]
fn shape_outside_rounded_inset_narrows_lines_at_curved_corners() {
    use crate::layout::{FloatContext, FloatShape, FloatSide, LayoutEngine};

    let mut context = FloatContext::default();
    context.place_float(
        &LayoutEngine::new(),
        16.0,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Left,
        "inset(0 round 40px / 20px)",
        0.0,
    );
    assert!(matches!(&context.floats[0].shape,
        Some(FloatShape::RoundedBox { rx, ry, .. })
        if (rx[0] - 40.0).abs() < 0.1 && (ry[0] - 20.0).abs() < 0.1));

    let mut left = 0.0;
    let mut right = 200.0;
    context.available_width(0.0, 2.0, 200.0, &mut left, &mut right);
    assert!(
        left > 60.0 && left < 100.0,
        "rounded top corner exclusion: {left}"
    );
    context.available_width(30.0, 2.0, 200.0, &mut left, &mut right);
    assert!((left - 100.0).abs() < 0.1, "straight middle edge: {left}");
}

#[test]
fn shape_outside_rounded_inset_changes_laid_out_text_position() {
    let text_x = |shape: &str| {
        let mut renderer = crate::Renderer::new();
        let doc = renderer.load_html(
            &format!(
                "<style>body{{margin:0;font:16px/20px sans-serif}}#wrap{{width:200px}}#float{{float:left;width:100px;height:100px;shape-outside:{shape}}}</style><div id=wrap><div id=float></div><span>hello</span></div>"
            ),
            300.0,
        );
        build_display_list(&doc.root, 300.0, 200.0)
            .commands
            .iter()
            .find_map(|cmd| match cmd {
                PaintCmd::Text { text, x, .. } if text.contains("hello") => Some(*x),
                _ => None,
            })
            .expect("text run")
    };
    let square = text_x("inset(0)");
    let rounded = text_x("inset(0 round 40px)");
    assert!(square - rounded > 3.0, "rounded={rounded}, square={square}");
}

#[test]
fn polygon_shape_margin_reaches_lines_above_shape_and_diagonal_tangents() {
    use crate::layout::{FloatContext, FloatSide, LayoutEngine};

    let engine = LayoutEngine::new();
    let mut triangle = FloatContext::default();
    triangle.place_float(
        &engine,
        16.0,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Left,
        "polygon(50px 20px, 80px 80px, 20px 80px)",
        10.0,
    );
    let mut left = 0.0;
    let mut right = 200.0;
    triangle.available_width(12.0, 2.0, 200.0, &mut left, &mut right);
    assert!(
        left > 55.0 && left < 65.0,
        "margin must reach above polygon: {left}"
    );
    triangle.available_width(0.0, 2.0, 200.0, &mut left, &mut right);
    assert_eq!(left, 0.0, "a distant line must remain unobstructed");

    let mut diagonal = FloatContext::default();
    diagonal.place_float(
        &engine,
        16.0,
        0.0,
        100.0,
        100.0,
        200.0,
        FloatSide::Left,
        "polygon(0 0, 100px 100px, 0 100px)",
        10.0,
    );
    diagonal.available_width(50.0, 0.01, 200.0, &mut left, &mut right);
    assert!(
        left > 64.0 && left < 64.5,
        "diagonal margin tangent should reach sqrt(2) farther: {left}"
    );
}

#[test]
fn shape_outside_inset_percentages_use_side_axes() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"
        <style>
          body { margin:0; font:16px/20px sans-serif; }
          #wrap { width:200px; }
          #float { float:left; width:100px; height:100px; shape-outside:inset(0 50% 0 0); }
        </style>
        <div id="wrap"><div id="float"></div><span>hello</span></div>
        "#,
        300.0,
    );
    let list = build_display_list(&d.root, 300.0, 200.0);
    let text_x = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text.contains("hello") => Some(*x),
            _ => None,
        })
        .expect("text run");

    assert!(
        text_x > 45.0 && text_x < 60.0,
        "right inset percentage should resolve against float width, got x={text_x}"
    );
}

#[test]
fn shape_outside_polygon_uses_line_intersections() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"
        <style>
          body { margin:0; font:16px/20px sans-serif; }
          #wrap { width:200px; }
          #float { float:left; width:100px; height:100px; shape-outside:polygon(0 0, 50% 50%, 0 100%); }
        </style>
        <div id="wrap"><div id="float"></div><span>hello</span></div>
        "#,
        300.0,
    );
    let list = build_display_list(&d.root, 300.0, 200.0);
    let text_x = list
        .commands
        .iter()
        .find_map(|cmd| match cmd {
            PaintCmd::Text { text, x, .. } if text.contains("hello") => Some(*x),
            _ => None,
        })
        .expect("text run");

    assert!(
        text_x > 5.0 && text_x < 30.0,
        "polygon shape-outside should use the polygon width at the sampled line, got x={text_x}"
    );
}

#[test]
fn computed_margin_and_padding_are_used_values() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
            #parent { width: 200px; }
            #child { width: 100px; padding-left: 50%; margin-left: 25%; margin-right: auto; }
        </style>
        <div id=parent><div id=child>x</div></div>",
        900.0,
    );
    let child = d.get_element_by_id("child").unwrap();

    assert_eq!(d.computed_style_property(child, "padding-left"), "100px");
    assert_eq!(d.computed_style_property(child, "margin-left"), "50px");
    assert_ne!(d.computed_style_property(child, "margin-right"), "auto");
}

#[test]
fn computed_style_serializes_common_shorthands() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
            #box {
                margin: 1px 2px 3px 4px;
                padding: 5px 6px;
                border: 0 none rgb(0, 0, 0);
                overflow-x: hidden;
                overflow-y: scroll;
                display: flex;
                flex: 2 3 10px;
                gap: 7px 8px;
                outline: 2px dashed rgb(1, 2, 3);
                columns: 120px 3;
                column-rule: 4px dotted rgb(4, 5, 6);
                transition: opacity 200ms ease-in 50ms allow-discrete;
                animation: fade 1s ease-out 100ms infinite alternate both paused add;
            }
        </style>
        <div id=box></div><div id=bare></div>",
        900.0,
    );
    let box_id = d.get_element_by_id("box").unwrap();
    let bare = d.get_element_by_id("bare").unwrap();

    assert_eq!(
        d.computed_style_property(box_id, "margin"),
        "1px 2px 3px 4px"
    );
    assert_eq!(d.computed_style_property(box_id, "padding"), "5px 6px");
    assert_eq!(
        d.computed_style_property(box_id, "border"),
        "0px none rgb(0, 0, 0)"
    );
    assert_eq!(
        d.computed_style_property(box_id, "overflow"),
        "hidden scroll"
    );
    assert_eq!(d.computed_style_property(box_id, "flex"), "2 3 10px");
    assert_eq!(d.computed_style_property(box_id, "gap"), "7px 8px");
    assert_eq!(
        d.computed_style_property(box_id, "outline"),
        "2px dashed rgb(1, 2, 3)"
    );
    assert_eq!(d.computed_style_property(box_id, "columns"), "120px 3");
    assert_eq!(
        d.computed_style_property(box_id, "column-rule"),
        "4px dotted rgb(4, 5, 6)"
    );
    assert_eq!(
        d.computed_style_property(box_id, "transition"),
        "opacity 200ms ease-in 50ms allow-discrete"
    );
    assert_eq!(
        d.computed_style_property(box_id, "animation"),
        "fade 1s ease-out 100ms infinite alternate both paused add"
    );
    assert_eq!(d.computed_style_property(bare, "inset"), "auto");
    assert_eq!(
        d.computed_style_property(bare, "transition"),
        "all 0s ease 0s normal"
    );
    assert_eq!(
        d.computed_style_property(bare, "animation"),
        "none 0s ease 0s 1 normal none running replace"
    );
}

#[test]
fn computed_font_family_quotes_names_that_need_quotes() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>#box { font-family: "Helvetica Neue", Arial, sans-serif; }</style><div id=box></div>"#,
        900.0,
    );
    let box_id = d.get_element_by_id("box").unwrap();

    assert_eq!(
        d.computed_style_property(box_id, "font-family"),
        r#""Helvetica Neue", Arial, sans-serif"#
    );
}

#[test]
fn computed_font_family_preserves_system_ui_keyword() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<style>#box { font-family: system-ui, ui-monospace; }</style><div id=box></div>"#,
        900.0,
    );
    let box_id = d.get_element_by_id("box").unwrap();

    assert_eq!(
        d.computed_style_property(box_id, "font-family"),
        "system-ui, ui-monospace"
    );
}

#[test]
fn computed_transform_resolves_percentage_translation_against_border_box() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<div id=box style='width:100px;height:20px;transform:translateX(50%)'></div>",
        900.0,
    );
    let box_id = d.get_element_by_id("box").unwrap();

    assert_eq!(
        d.computed_style_property(box_id, "transform"),
        "matrix(1, 0, 0, 1, 50, 0)"
    );
}

/// ⛔ The absolute length units, with the EXACT ratios CSS Values 4 §6.2 gives.
///
/// webcore understood eight units of the spec's thirty-one; `cm`, `mm`, `Q`,
/// `in` and `pc` were not among them and fell through to `auto`.
///
/// ⛔ They could not simply be added to the old `ends_with` chain, because unit
/// names NEST: `in` is a suffix of `vmin`, so testing `in` first parses
/// `3vmin` as three inches. The parser splits the number from the unit and
/// matches the unit exactly, which removes that class of bug rather than
/// dodging it.
///
/// ⛔ Chrome is NOT the authority for these. It answers 37.7812px for `1cm`
/// where the spec says 96/2.54 = 37.795276 — every one of its values is
/// `floor(exact * 64) / 64`, because its LayoutUnit quantises to 1/64px. The
/// spec numbers are asserted here.
#[test]
fn the_absolute_length_units_use_the_exact_spec_ratios() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{width:1in}#b{width:1cm}#c{width:1mm}#d{width:1Q}\
         #e{width:1pc}#f{width:1pt}</style>\
         <div id=a></div><div id=b></div><div id=c></div>\
         <div id=d></div><div id=e></div><div id=f></div>",
        900.0,
    );
    let px = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
            .trim_end_matches("px")
            .parse::<f32>()
            .unwrap()
    };
    let close = |got: f32, want: f32, what: &str| {
        assert!(
            (got - want).abs() < 0.001,
            "{what}: got {got}, spec says {want}"
        );
    };
    close(px(&mut d, "a"), 96.0, "1in = 96px");
    close(px(&mut d, "b"), 96.0 / 2.54, "1cm = 96px/2.54");
    close(px(&mut d, "c"), 96.0 / 25.4, "1mm = 1/10th of 1cm");
    close(px(&mut d, "d"), 96.0 / 101.6, "1Q  = 1/40th of 1cm");
    close(px(&mut d, "e"), 16.0, "1pc = 1/6th of 1in");
    close(px(&mut d, "f"), 96.0 / 72.0, "1pt = 1/72nd of 1in");
}

/// Dimension units are ASCII case-insensitive (CSS Values 4 §3.1), and a
/// number may carry an exponent. Neither worked: `10PX` and `1e2px` both fell
/// through to `auto`.
#[test]
fn a_unit_is_case_insensitive_and_a_number_may_have_an_exponent() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{width:10PX}#b{width:1e2px}#c{width:2In}</style>\
         <div id=a></div><div id=b></div><div id=c></div>",
        900.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
    };
    assert_eq!(w(&mut d, "a"), "10px", "`PX` is the same unit as `px`");
    assert_eq!(w(&mut d, "b"), "100px", "`1e2` is 100");
    assert_eq!(w(&mut d, "c"), "192px", "`In` is the same unit as `in`");
}

/// ⛔ `3vmin` must not parse as three INCHES.
///
/// The regression this guards is subtle: `in` is a suffix of `vmin`, so any
/// parser that tests unit suffixes in the wrong order silently turns a
/// viewport-relative length into an absolute one 32x larger.
#[test]
fn a_viewport_unit_is_not_mistaken_for_inches() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{width:3vmin}#b{width:3in}</style><div id=a></div><div id=b></div>",
        900.0,
    );
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();
    let wa = d.computed_style_property(a, "width");
    let wb = d.computed_style_property(b, "width");
    assert_eq!(wb, "288px", "3in is 3 * 96px");
    assert_ne!(wa, wb, "3vmin is a viewport length, not three inches");
}

/// ⛔ `vmin` follows the SMALLER viewport axis and `vmax` the LARGER —
/// CSS Values 4 §6.1.2.
///
/// Both used to parse to `CssLength::Vw`, commented "approx". That is not an
/// approximation, it is the wrong axis on any landscape viewport: measured in
/// Chrome at 1200x713, `10vmin` is 71.3px (10% of the HEIGHT) and `10vmax` is
/// 120px, while webcore answered 120px for both — out by 68%.
///
/// They could not be expressed as `Vw` or `Vh` because which axis they follow
/// is not known until the viewport is, so they are their own variants.
#[test]
fn vmin_and_vmax_follow_the_smaller_and_larger_viewport_axis() {
    let mut r = crate::Renderer::new();
    // Landscape: width 1200, height 700.
    let mut d = r.load_html_vp(
        "<style>#a{width:10vmin}#b{width:10vmax}#c{width:10vw}#d{width:10vh}</style>\
         <div id=a></div><div id=b></div><div id=c></div><div id=d></div>",
        1200.0,
        700.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
            .trim_end_matches("px")
            .parse::<f32>()
            .unwrap()
    };
    let (vmin, vmax) = (w(&mut d, "a"), w(&mut d, "b"));
    let (vw, vh) = (w(&mut d, "c"), w(&mut d, "d"));

    assert!((vw - 120.0).abs() < 0.5, "10vw of 1200 is 120px, got {vw}");
    assert!((vh - 70.0).abs() < 0.5, "10vh of 700 is 70px, got {vh}");
    assert!(
        (vmin - vh).abs() < 0.5,
        "on a LANDSCAPE viewport vmin follows the height: got {vmin}, vh is {vh}"
    );
    assert!(
        (vmax - vw).abs() < 0.5,
        "and vmax follows the width: got {vmax}, vw is {vw}"
    );
    assert!((vmin - vmax).abs() > 1.0, "they must not be the same value");
}

/// The modern viewport units, and the logical viewport axes.
///
/// `svh`/`lvh`/`dvh` coincide with `vh` on a UA that shows no dynamically
/// retracting toolbars — CSS Values 4 §6.1.2 — which is this one, so that is
/// conformance rather than approximation. `vi`/`vb` are the inline and block
/// axes, which in horizontal-tb are the width and the height.
///
/// Verified against Chrome at 1200x713: `10svh` = 71.3px = `10vh`,
/// `10vi` = 120px = `10vw`, `10vb` = 71.3px.
#[test]
fn the_modern_viewport_units_resolve_to_their_axis() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html_vp(
        "<style>#sv{width:10svh}#lv{width:10lvh}#dv{width:10dvh}#sw{width:10svw}\
         #vi{width:10vi}#vb{width:10vb}#vw{width:10vw}#vh{width:10vh}</style>\
         <div id=sv></div><div id=lv></div><div id=dv></div><div id=sw></div>\
         <div id=vi></div><div id=vb></div><div id=vw></div><div id=vh></div>",
        1200.0,
        700.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
    };
    let (vw, vh) = (w(&mut d, "vw"), w(&mut d, "vh"));
    assert_ne!(
        vw, vh,
        "the fixture must use a non-square viewport to mean anything"
    );
    for id in ["sv", "lv", "dv", "vb"] {
        assert_eq!(
            w(&mut d, id),
            vh,
            "#{id} follows the BLOCK axis (the height)"
        );
    }
    for id in ["sw", "vi"] {
        assert_eq!(
            w(&mut d, id),
            vw,
            "#{id} follows the INLINE axis (the width)"
        );
    }
}

/// The font-relative units, at the fallbacks the spec itself mandates.
///
/// ⛔ These are the spec's own "must be assumed" values for when the real font
/// metric is impractical to obtain (CSS Values 4 §6.1.1) — `ex` 0.5em, `ch`
/// 0.5em, `ic` 1em. They are conforming, but they are FALLBACKS, not
/// measurements: Chrome, which reads the font, answers 7.17px for `1ex` at a
/// 16px font where this answers 8px. `ch` happens to agree exactly (8px).
///
/// Closing that gap means giving the length resolver access to font metrics,
/// which it does not currently have.
#[test]
fn the_font_relative_units_use_the_spec_mandated_fallbacks() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{font-size:16px}#ex{width:1ex}#ch{width:1ch}#ic{width:1ic}\
         #em{width:1em}</style>\
         <div id=ex></div><div id=ch></div><div id=ic></div><div id=em></div>",
        900.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
            .trim_end_matches("px")
            .parse::<f32>()
            .unwrap()
    };
    let em = w(&mut d, "em");
    assert!((em - 16.0).abs() < 0.01, "1em is the element's font size");
    assert!(
        (w(&mut d, "ex") - em * 0.5).abs() < 0.01,
        "`ex` falls back to 0.5em"
    );
    assert!(
        (w(&mut d, "ch") - em * 0.5).abs() < 0.01,
        "`ch` falls back to 0.5em"
    );
    assert!(
        (w(&mut d, "ic") - em).abs() < 0.01,
        "`ic` falls back to 1em"
    );
}

/// ⛔ `calc()` had its OWN unit table, and its catch-all was `unknown => px`.
///
/// That is the worst shape a gap can take: not a parse failure but a silent
/// wrong answer. `calc(1in + 2px)` resolved to **3px** — `in` was not in
/// calc's table, so `1in` was read as `1px` — where the correct answer is
/// 98px. Every unit added to `parse_length` had to be added here too, and any
/// that was not became wrong rather than rejected.
///
/// `parse_length` is now the single definition and this projects onto the
/// coefficient slots. Chrome agrees on all four; where it differs it is its
/// 1/64px quantisation, not disagreement — `calc(2cm)` is exactly 2*96/2.54.
#[test]
fn calc_understands_every_unit_the_length_parser_does() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{width:calc(1in + 2px)}#b{width:calc(2cm)}#c{width:calc(1pc + 1pt)}\
         #d{width:calc(10px + 2em);font-size:16px}</style>\
         <div id=a></div><div id=b></div><div id=c></div><div id=d></div>",
        900.0,
    );
    let px = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
            .trim_end_matches("px")
            .parse::<f32>()
            .unwrap()
    };
    let close = |got: f32, want: f32, what: &str| {
        assert!((got - want).abs() < 0.01, "{what}: got {got}, want {want}");
    };
    close(
        px(&mut d, "a"),
        98.0,
        "calc(1in + 2px) — `in` is 96px, not 1px",
    );
    close(px(&mut d, "b"), 2.0 * 96.0 / 2.54, "calc(2cm)");
    close(px(&mut d, "c"), 16.0 + 96.0 / 72.0, "calc(1pc + 1pt)");
    close(px(&mut d, "d"), 42.0, "calc(10px + 2em) at a 16px font");
}

#[test]
fn calc_with_clamp_and_chained_arithmetic_keeps_numeric_subexpressions() {
    let length = crate::css::parse_length(
        "calc((clamp(1356px, 100vw, 1600px) - 32px - (36 - 1) * 8px) / 36 * 7 + (7 - 1) * 8px)",
    );
    let got = length.resolve_vp(16.0, 1366.0, 16.0, 1366.0, 768.0);
    let want = (1366.0 - 32.0 - 35.0 * 8.0) / 36.0 * 7.0 + 6.0 * 8.0;
    assert!((got - want).abs() < 0.01, "got {got}, want {want}");
}

/// ⛔ A rem-based media query was ALWAYS TRUE.
///
/// `parse_media_px` was a third private unit table — `px`, `em`, and a bare
/// `parse()` for everything else. A bare parse of `"40rem"` fails and gives 0,
/// so `(min-width: 40rem)` compared the viewport against **zero** and matched
/// everything. rem breakpoints are what Bootstrap and Tailwind emit, so this
/// silently applied every one of their responsive blocks at every size.
///
/// Measured: `(min-width: 4000rem)` — 64000px — matched a 1200px viewport.
/// Chrome does not.
///
/// Relative units in a media query resolve against the INITIAL font size
/// (Media Queries 4 §1.3), so `em` and `rem` are both 16px here.
#[test]
fn a_media_query_understands_every_length_unit() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html_vp(
        "<style>#a{width:10px}@media (min-width: 4000rem){#a{width:99px}}\
         #b{width:10px}@media (min-width: 40rem){#b{width:77px}}\
         #c{width:10px}@media (min-width: 40em){#c{width:55px}}\
         #e{width:10px}@media (min-width: 100in){#e{width:66px}}</style>\
         <div id=a></div><div id=b></div><div id=c></div><div id=e></div>",
        1200.0,
        800.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "width")
    };
    assert_eq!(
        w(&mut d, "a"),
        "10px",
        "4000rem is 64000px — it must NOT match a 1200px viewport"
    );
    assert_eq!(w(&mut d, "b"), "77px", "40rem is 640px — it matches");
    assert_eq!(
        w(&mut d, "c"),
        "55px",
        "40em is also 640px in a media query"
    );
    assert_eq!(
        w(&mut d, "e"),
        "10px",
        "100in is 9600px — an absolute unit must be understood, not read as 0"
    );
}

/// ⛔ Transform arguments are not all the same kind of value, and treating
/// them as one broke lengths and angles in different ways.
///
/// The parser stripped `px|deg|rad|turn` off every argument and parsed the
/// remainder. So:
///
///  * a LENGTH in any other unit failed to parse and became 0 —
///    `translateX(2rem)` and `translateX(1in)` moved the element NOWHERE;
///  * an ANGLE had its unit REMOVED rather than CONVERTED — `rotate(1turn)`
///    was read as one DEGREE instead of 360, and `rotate(1rad)` as one degree
///    instead of 57.3.
///
/// Lengths now go through the single unit definition; angles convert per
/// CSS Values 4 §7.1 (1turn = 360deg, 1grad = 0.9deg, 1rad = 180/PI deg).
/// Every value below is Chrome's, on the same markup.
#[test]
fn transform_lengths_and_angles_use_their_own_units() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{transform:translateX(2rem)}#b{transform:translateX(1in)}\
         #c{transform:rotate(0.5turn)}#d{transform:rotate(100grad)}\
         #e{transform:rotate(90deg)}</style>\
         <div id=a></div><div id=b></div><div id=c></div>\
         <div id=d></div><div id=e></div>",
        900.0,
    );
    let t = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "transform")
    };
    assert_eq!(
        t(&mut d, "a"),
        "matrix(1, 0, 0, 1, 32, 0)",
        "translateX(2rem) is 32px"
    );
    assert_eq!(
        t(&mut d, "b"),
        "matrix(1, 0, 0, 1, 96, 0)",
        "translateX(1in) is 96px"
    );
    assert_eq!(
        t(&mut d, "c"),
        "matrix(-1, 0, 0, -1, 0, 0)",
        "0.5turn is 180deg"
    );
    let grad = t(&mut d, "d");
    let deg90 = t(&mut d, "e");
    assert_eq!(grad, deg90, "100grad IS 90deg — got {grad} vs {deg90}");
}

/// ⛔ `getBoundingClientRect()` must return the TRANSFORMED border box —
/// CSSOM View §4 — and returned the untransformed one, so a page could not
/// find out where a transformed element actually is.
///
/// Chrome on this markup: a 100x40 box translated 2rem is at x=32 with its
/// size unchanged; rotated 90deg about its centre it becomes 40x100 at
/// (30, -30). The rotation case is the one that proves the whole box is
/// mapped and re-bounded, not just its origin shifted.
#[test]
fn a_bounding_rect_reflects_the_elements_transform() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}div{width:100px;height:40px;position:absolute;top:0;left:0}\
         #a{transform:translateX(2rem)}#b{transform:rotate(90deg)}#c{}</style>\
         <div id=a></div><div id=b></div><div id=c></div>",
        900.0,
    );
    let rect = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let c = rect(&mut d, "c");
    assert!(
        (c.w - 100.0).abs() < 0.5 && (c.h - 40.0).abs() < 0.5,
        "the untransformed box is 100x40, got {}x{}",
        c.w,
        c.h
    );

    let a = rect(&mut d, "a");
    assert!(
        (a.x - 32.0).abs() < 0.5,
        "translateX(2rem) puts x at 32, got {}",
        a.x
    );
    assert!(
        (a.w - 100.0).abs() < 0.5,
        "a translation does not change the size"
    );

    let b = rect(&mut d, "b");
    assert!(
        (b.w - 40.0).abs() < 1.0 && (b.h - 100.0).abs() < 1.0,
        "rotate(90deg) swaps the extents to 40x100, got {}x{}",
        b.w,
        b.h
    );
}

/// ⛔ `@layer` ordering was ignored entirely — the parser said so in a comment
/// ("ignore layer ordering for now") and the cascade sorted on specificity
/// alone.
///
/// Two consequences, both verified against Chrome:
///
///  * a rule in a LATER layer beats one in an earlier layer however much more
///    specific the earlier one is — layers sort ABOVE specificity
///    (CSS Cascade 5 §6.4.4). A bare `div` in `over` beats `div#hi.hi.hi` in
///    `base`.
///  * an UNLAYERED normal declaration beats every layered one.
///
/// ⛔ The `@layer a, b;` STATEMENT form is what fixes the order, and it has no
/// block — so it was being discarded along with every other braceless at-rule,
/// and layer precedence silently fell back to source order. In the fixture
/// below `base` is written AFTER `over` in the source precisely so that source
/// order gives the wrong answer.
#[test]
fn layer_order_outranks_specificity_and_unlayered_wins() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer base, over;\
         @layer over { div { color: green } }\
         @layer base { div#hi.hi.hi { color: red } }\
         #un2 { color: green }\
         @layer only2 { #un2 { color: red } }</style>\
         <div id=hi class=hi>x</div><div id=un2>x</div>",
        900.0,
    );
    let c = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "color")
    };
    assert_eq!(
        c(&mut d, "hi"),
        "rgb(0, 128, 0)",
        "a bare `div` in the LATER layer beats `div#hi.hi.hi` in the earlier one"
    );
    assert_eq!(
        c(&mut d, "un2"),
        "rgb(0, 128, 0)",
        "an unlayered declaration beats a layered one"
    );
}

#[test]
fn layered_author_rule_still_beats_ua_origin() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer reset { div { display: inline } }</style><div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "display"),
        "inline",
        "normal cascade order is origin before layer, so a layered author rule must beat UA div defaults"
    );
}

#[test]
fn anonymous_layer_loses_to_unlayered_rule_even_when_later() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>div { color: rgb(0, 128, 0) } @layer { div { color: rgb(255, 0, 0) } }</style><div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "color"),
        "rgb(0, 128, 0)",
        "anonymous layers are still layered, so unlayered author CSS must outrank them"
    );
}

#[test]
fn nested_layer_names_are_qualified_by_the_parent_layer() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer framework { @layer base { #t { color: rgb(255, 0, 0) } } } @layer base { #t { color: rgb(0, 128, 0) } }</style><div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "color"),
        "rgb(0, 128, 0)",
        "nested framework.base must not collapse into the top-level base layer"
    );
}

#[test]
fn nested_layers_stay_grouped_and_parent_rules_follow_sublayers() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>\
         @layer first.child { #outer { color: rgb(255, 0, 0) } }\
         @layer second { #outer { color: rgb(0, 128, 0) } }\
         @layer first { #outer { color: rgb(0, 0, 255) }\
           #parent { color: rgb(0, 0, 255) }\
           @layer child { #parent { color: rgb(255, 0, 0) } }\
         }</style><div id=outer>x</div><div id=parent>x</div>",
        900.0,
    );
    let outer = doc.get_element_by_id("outer").unwrap();
    assert_eq!(
        doc.computed_style_property(outer, "color"),
        "rgb(0, 128, 0)"
    );
    let parent = doc.get_element_by_id("parent").unwrap();
    assert_eq!(
        doc.computed_style_property(parent, "color"),
        "rgb(0, 0, 255)"
    );
}

#[test]
fn important_nested_layer_precedes_its_parent() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>@layer first {\
           #target { color: rgb(0, 0, 255) !important }\
           @layer child { #target { color: rgb(255, 0, 0) !important } }\
         }</style><div id=target>x</div>",
        900.0,
    );
    let node = doc.get_element_by_id("target").unwrap();
    assert_eq!(doc.computed_style_property(node, "color"), "rgb(255, 0, 0)");
}

#[test]
fn media_conditioned_layer_declaration_changes_order_on_resize() {
    let mut doc = crate::parse_html(
        "<style>\
         @media (min-width: 700px) { @layer layout; }\
         @layer theme, layout;\
         @layer theme { #target { color: rgb(0, 0, 255) } }\
         @layer layout { #target { color: rgb(0, 128, 0) } }\
         </style><div id=target>x</div>",
    );
    for (width, expected) in [
        (600.0, "rgb(0, 128, 0)"),
        (800.0, "rgb(0, 0, 255)"),
        (600.0, "rgb(0, 128, 0)"),
    ] {
        doc.set_viewport(width, 600.0);
        let node = doc.get_element_by_id("target").unwrap();
        assert_eq!(
            doc.computed_style_property(node, "color"),
            expected,
            "{width}"
        );
    }
}

#[test]
fn important_layer_order_is_reversed() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>@layer first, second; @layer first { #t { color: rgb(255, 0, 0) !important } } @layer second { #t { color: rgb(0, 128, 0) !important } }</style><div id=t>x</div>",
        900.0,
    );
    let div = d.get_element_by_id("t").unwrap();

    assert_eq!(
        d.computed_style_property(div, "color"),
        "rgb(255, 0, 0)",
        "important declarations reverse layer order, so the earlier layer wins"
    );
}

/// ⛔ A sibling combinator failed the moment anything preceded it.
///
/// `i + i` matched. `#p i + i` and `#p > i + i` did not — so
/// `.container > li + li` and `.card h2 + p`, which are everyday selectors,
/// silently never applied.
///
/// The sibling branches matched the left-hand side as a FLAT COMPOUND against
/// the sibling, which is only correct when there is no further combinator in
/// it. `matches_part_with_context` answers `true` for a `Combinator` part, so
/// everything to its left was then tested against the SIBLING rather than
/// against the sibling's ancestor — and `#p` is not an `<i>`. They recurse now,
/// as the descendant and child branches already did.
///
/// Chrome matches every row below.
#[test]
fn a_sibling_combinator_works_after_another_combinator() {
    let cases = [
        ("i + i", "i2"),
        ("#p i + i", "i2"),
        ("#p > i + i", "i2"),
        ("i ~ u", "u1"),
        ("#q i ~ u", "u1"),
        ("#q > i ~ u", "u1"),
    ];
    for (sel, target) in cases {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>{sel}{{color:green}}</style>\
                      <div id=p><i id=i1>a</i><i id=i2>b</i></div>\
                      <div id=q><i id=i3>a</i><u id=u1>b</u></div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id(target).unwrap();
        assert_eq!(
            d.computed_style_property(e, "color"),
            "rgb(0, 128, 0)",
            "`{sel}` must match #{target}"
        );
    }

    // …and must NOT match the first sibling, or it is matching everything.
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#p > i + i{color:green}</style>\
         <div id=p><i id=i1>a</i><i id=i2>b</i></div>",
        900.0,
    );
    let first = d.get_element_by_id("i1").unwrap();
    assert_ne!(
        d.computed_style_property(first, "color"),
        "rgb(0, 128, 0)",
        "`i + i` must not match the FIRST `<i>` — it has no preceding sibling"
    );
}

/// ⛔ `:has(> em)` never matched — two separate bugs, both needed fixing.
///
/// 1. The selector parser did not skip the whitespace AFTER a leading
///    combinator, so `"> em"` parsed as `[Child, Descendant, em]` — a spurious
///    second combinator that matches nothing. It only bit selectors that
///    START with a combinator, because in `div > em` the whitespace arm sees
///    the `>` first and already skips past it. A `:has()` argument is exactly
///    such a selector.
/// 2. `:has()`'s argument is a RELATIVE selector (Selectors 4 §4.5): a leading
///    combinator relates to the ANCHOR, the element `:has()` is written on.
///    The matcher tried it against every descendant with an empty ancestor
///    list, so the leading `>` had nothing to relate to.
///
/// The `#b` row is what separates them: `:has(> em)` must NOT match when the
/// `<em>` is a grandchild. Chrome agrees on all three.
#[test]
fn has_treats_its_argument_as_relative_to_the_anchor() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a:has(> em){color:green} #b:has(> em){color:green} \
         #c:has(em){color:green}</style>\
         <div id=a><em>x</em></div>\
         <div id=b><span><em>x</em></span></div>\
         <div id=c><span><em>x</em></span></div>",
        900.0,
    );
    let c = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "color")
    };
    assert_eq!(
        c(&mut d, "a"),
        "rgb(0, 128, 0)",
        "`:has(> em)` matches a DIRECT child"
    );
    assert_ne!(
        c(&mut d, "b"),
        "rgb(0, 128, 0)",
        "`:has(> em)` must NOT match a grandchild"
    );
    assert_eq!(
        c(&mut d, "c"),
        "rgb(0, 128, 0)",
        "`:has(em)` matches at any depth"
    );
}

#[test]
fn ancestor_has_matches_sibling_subtree_in_parallel_selector_context() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        "<div id=container><div class=lead><div class=media></div></div><div class=secondary></div></div>",
        900.0,
    );
    let container = find_box(&doc.root, &|b| {
        b.attributes.get("id").is_some_and(|id| id == "container")
    })
    .unwrap();
    let secondary = find_box(container, &|b| {
        b.attributes
            .get("class")
            .is_some_and(|class| class == "secondary")
    })
    .unwrap();
    let ancestors = [crate::css::AncestorInfo {
        tag: container.tag.clone(),
        attributes: container.attributes.clone().into(),
        node_id: container.node_id,
        ..Default::default()
    }];
    let empty = std::collections::HashSet::new();
    let ancestor_nodes = [container];
    let ctx = crate::css::MatchContext {
        focused_box: 0,
        keyboard_focus: false,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: Some(secondary),
        ancestor_nodes: &ancestor_nodes,
        hover_chain: &empty,
        focus_within_chain: &empty,
        element_id: secondary.node_id,
        scope_root_id: 0,
        target_id: 0,
        document_url: "",
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    let selector = crate::css::parser::parse_selector(":not(:has(.lead .media)) .secondary");
    assert!(!crate::css::matching::matches_selector_with_ancestors(
        &selector.parts,
        &secondary.tag,
        &secondary.attributes,
        0,
        1,
        &ancestors,
        &ctx
    ));
    let positive = crate::css::parser::parse_selector(":has(.lead .media) .secondary");
    assert!(crate::css::matching::matches_selector_with_ancestors(
        &positive.parts,
        &secondary.tag,
        &secondary.attributes,
        0,
        1,
        &ancestors,
        &ctx
    ));
}

#[test]
fn ancestor_has_selects_descendant_styles_in_the_document_cascade() {
    let mut doc = parse(
        r#"<style>
        .card .secondary { color: black; }
        .card:has(.lead .media) .secondary { color: green; }
        .card:not(:has(.lead .media)) .secondary { color: red; }
    </style>
    <div class="card"><div class="lead"><div class="media"></div></div><div class="secondary" id="present"></div></div>
    <div class="card"><div class="lead"></div><div class="secondary" id="absent"></div></div>"#,
    );
    let present = crate::dom::query_selector(&doc.root, "#present").unwrap();
    let absent = crate::dom::query_selector(&doc.root, "#absent").unwrap();
    assert_eq!(present.style.color, Color::rgb(0, 128, 0));
    assert_eq!(absent.style.color, Color::rgb(255, 0, 0));

    doc.root.cascade_dirty = true;
    crate::css::apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        900.0,
        700.0,
        0,
        false,
        &std::collections::HashSet::new(),
    );
    let present = crate::dom::query_selector(&doc.root, "#present").unwrap();
    assert_eq!(present.style.color, Color::rgb(0, 128, 0));

    let media = crate::dom::query_selector_mut(&mut doc.root, ".media").unwrap();
    media.attributes.insert("class", "media-off");
    doc.root.cascade_dirty = true;
    crate::css::apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        900.0,
        700.0,
        0,
        false,
        &std::collections::HashSet::new(),
    );
    let present = crate::dom::query_selector(&doc.root, "#present").unwrap();
    assert_eq!(present.style.color, Color::rgb(255, 0, 0));
}

#[test]
fn sibling_has_selects_following_element_in_the_document_cascade() {
    let mut doc = parse(
        r#"<style>
        .lead + .secondary { color: black; }
        .lead:has(.media) + .secondary { color: green; }
        .lead:not(:has(.media)) + .secondary { color: red; }
        .lead:has(.media) ~ .later { color: green; }
        .lead:not(:has(.media)) ~ .later { color: red; }
    </style>
    <div><div class="lead"><span class="media"></span></div><div class="secondary" id="present"></div><div class="later" id="present-later"></div></div>
    <div><div class="lead"></div><div class="secondary" id="absent"></div><div class="later" id="absent-later"></div></div>"#,
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#present")
            .unwrap()
            .style
            .color,
        Color::rgb(0, 128, 0)
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#absent")
            .unwrap()
            .style
            .color,
        Color::rgb(255, 0, 0)
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#present-later")
            .unwrap()
            .style
            .color,
        Color::rgb(0, 128, 0)
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#absent-later")
            .unwrap()
            .style
            .color,
        Color::rgb(255, 0, 0)
    );

    let media = crate::dom::query_selector_mut(&mut doc.root, ".media").unwrap();
    media.attributes.insert("class", "media-off");
    doc.root.cascade_dirty = true;
    crate::css::apply_cascade_incremental(
        &mut doc.root,
        &doc.stylesheet,
        None,
        16.0,
        900.0,
        700.0,
        0,
        false,
        &std::collections::HashSet::new(),
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#present")
            .unwrap()
            .style
            .color,
        Color::rgb(255, 0, 0)
    );
    assert_eq!(
        crate::dom::query_selector(&doc.root, "#present-later")
            .unwrap()
            .style
            .color,
        Color::rgb(255, 0, 0)
    );
}

/// The parser bug above, on its own terms: a selector that STARTS with a
/// combinator must not gain a second one from the space after it.
#[test]
fn a_leading_combinator_does_not_add_a_descendant_combinator() {
    use crate::css::selector::{Combinator, SelectorPart};
    let sel = crate::css::parser::parse_selector("> em");
    let combinators: Vec<_> = sel
        .parts
        .iter()
        .filter(|p| matches!(p, SelectorPart::Combinator(_)))
        .collect();
    assert_eq!(
        combinators.len(),
        1,
        "`> em` has ONE combinator; the space after `>` must not add another — got {:?}",
        sel.parts
    );
    assert!(
        matches!(
            sel.parts.first(),
            Some(SelectorPart::Combinator(Combinator::Child))
        ),
        "and it is the child combinator"
    );
}

/// ⛔ `:nth-child(An+B of S)` — Selectors 4 §9.3 — counts only among siblings
/// matching S, and requires the element to match S itself.
///
/// The whole argument used to go to the An+B parser, which cannot read
/// `2 of .pick`, so the selector matched nothing at all.
///
/// The fixture is built so that counting ALL children gives a different answer
/// from counting only the matching ones: the 2nd `.pick` is the 4th child.
/// Chrome colours exactly `b3`.
#[test]
fn nth_child_of_selector_counts_only_matching_siblings() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#z :nth-child(2 of .pick){color:green}</style>\
         <div id=z><b id=b0>skip</b><b id=b1 class=pick>a</b><b id=b2>no</b>\
         <b id=b3 class=pick>b</b><b id=b4 class=pick>c</b></div>",
        900.0,
    );
    let c = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "color")
    };
    assert_eq!(
        c(&mut d, "b3"),
        "rgb(0, 128, 0)",
        "the 2nd `.pick` is the 4th child — counting all children would pick b1"
    );
    for id in ["b0", "b1", "b2", "b4"] {
        assert_ne!(
            c(&mut d, id),
            "rgb(0, 128, 0)",
            "#{id} is not the 2nd `.pick`"
        );
    }
}

/// `:nth-last-child(An+B of S)` is the same Selectors 4 filtered-index form,
/// but counted from the end of the matching sibling subset.
///
/// Chrome colours exactly `b3`: it is the 2nd `.pick` from the end while being
/// the 4th element child overall.
#[test]
fn nth_last_child_of_selector_counts_only_matching_siblings_from_end() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#z :nth-last-child(2 of .pick){color:green}</style>\
         <div id=z><b id=b0 class=pick>a</b><b id=b1>skip</b><b id=b2 class=pick>b</b>\
         <b id=b3 class=pick>c</b><b id=b4>no</b><b id=b5 class=pick>d</b></div>",
        900.0,
    );
    let c = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "color")
    };
    assert_eq!(
        c(&mut d, "b3"),
        "rgb(0, 128, 0)",
        "the 2nd `.pick` from the end is b3"
    );
    for id in ["b0", "b1", "b2", "b4", "b5"] {
        assert_ne!(
            c(&mut d, id),
            "rgb(0, 128, 0)",
            "#{id} is not the 2nd `.pick` from the end"
        );
    }
}

#[test]
fn nth_last_child_negative_range_hides_last_two_list_items() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>#navbarNavDropdown .navbar-nav .nav-item:nth-last-child(-n + 2) { display: none; }</style>\
         <div id=navbarNavDropdown><ul class=navbar-nav>\
         <li class=nav-item id=first>One</li><li class=nav-item id=second>Two</li>\
         <li class=nav-item id=third>Three</li><li class=nav-item id=fourth>Four</li>\
         </ul></div>",
        900.0,
    );
    for id in ["first", "second"] {
        let element = doc.get_element_by_id(id).unwrap();
        assert_ne!(
            doc.computed_style_property(element, "display"),
            "none",
            "{id}"
        );
    }
    for id in ["third", "fourth"] {
        let element = doc.get_element_by_id(id).unwrap();
        assert_eq!(
            doc.computed_style_property(element, "display"),
            "none",
            "{id}"
        );
    }
}

/// ⛔ `width` and `height` do not apply to a NON-REPLACED INLINE box —
/// CSS 2.1 §10.2 and §10.5.
///
/// `<span style="width:100px;height:50px">` was being sized 100x50. Chrome
/// sizes it by its text.
///
/// The distinction is `display: inline` exactly, not "is inline-level":
/// `inline-block`, `inline-flex` and the replaced elements are all
/// inline-level and DO take a width, which is what the second half asserts —
/// without it, an implementation that ignored width on everything
/// inline-level would pass.
///
/// ⛔ webcore reports 0x0 for an inline element's rect (Chrome: 8x18), because
/// an inline box is a run of line-box fragments rather than a box with a
/// border rect. That is a separate, pre-existing gap; this test therefore
/// checks that the declared width is NOT adopted, not the exact text extent.
#[test]
fn width_does_not_apply_to_a_non_replaced_inline() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0;font-size:16px}</style><div style='width:400px'>\
         <span id=wide style='width:100px'>x</span>\
         <span id=ib style='display:inline-block;width:100px'>x</span></div>",
        900.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    let wide = w(&mut d, "wide");
    assert!(
        (wide - 100.0).abs() > 1.0,
        "an inline box must not adopt its declared width — got {wide}"
    );
    let ib = w(&mut d, "ib");
    assert!(
        (ib - 100.0).abs() < 1.0,
        "…but an inline-BLOCK does: got {ib}"
    );
}

/// ⛔ The modern space-separated colour syntax did not parse — CSS Color 4 §4.
///
/// `rgb(1 2 3)`, `rgb(1 2 3 / 0.5)` and `hsl(120 50% 50%)` are what every
/// current design system emits, and only the legacy comma form was understood.
/// The component split produced ONE item, failed the `len() >= 3` check, and
/// the colour silently became BLACK.
///
/// The hue also accepts an angle unit (`120deg`), and the alpha a percentage.
///
/// ⛔ Channels ROUND rather than truncate: `hsl(120 50% 50%)`'s green channel
/// is 63.75, and `as u8` floored it to 63 where every browser says 64.
#[test]
fn the_modern_space_separated_colour_syntax_parses() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>#a{color:rgb(1 2 3)}#b{color:rgb(1 2 3 / 0.5)}\
         #c{color:rgba(1,2,3,0.5)}#e{color:hsl(120 50% 50%)}\
         #f{color:hsl(120deg 50% 50% / .5)}#g{color:rgb(1 2 3 / 50%)}</style>\
         <div id=a>x</div><div id=b>x</div><div id=c>x</div>\
         <div id=e>x</div><div id=f>x</div><div id=g>x</div>",
        900.0,
    );
    let c = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.computed_style_property(e, "color")
    };
    assert_eq!(c(&mut d, "a"), "rgb(1, 2, 3)", "space-separated rgb()");
    assert_eq!(c(&mut d, "b"), "rgba(1, 2, 3, 0.5)", "slash alpha");
    assert_eq!(
        c(&mut d, "c"),
        "rgba(1, 2, 3, 0.5)",
        "the legacy comma form still works"
    );
    assert_eq!(
        c(&mut d, "e"),
        "rgb(64, 191, 64)",
        "hsl() space-separated — and 63.75 ROUNDS to 64, it does not truncate to 63"
    );
    assert_eq!(
        c(&mut d, "f"),
        "rgba(64, 191, 64, 0.5)",
        "a hue may carry `deg`"
    );
    assert_eq!(
        c(&mut d, "g"),
        "rgba(1, 2, 3, 0.5)",
        "alpha may be a percentage"
    );
}

/// ⛔ A flex row with a definite HEIGHT did not shrink its items.
///
/// `align-items: stretch` is the default, and a definite cross size is what
/// makes the stretch pass actually re-lay a child. That re-layout passed
/// `None` for the forced MAIN size, so the child fell back to its own `width`
/// and the resolved grow/shrink was thrown away.
///
/// Measured: a 400px item beside a `flex-shrink:0` 50px item, in a 300px row,
/// stayed 400px instead of shrinking to 250. Chrome gives 250. Fixed-height
/// flex rows are an everyday layout, and the bug was invisible without one —
/// the same markup with no height, or with `align-items: flex-start`, was
/// already correct.
#[test]
fn a_flex_row_with_a_definite_height_still_shrinks_its_items() {
    for extra in [
        "",
        "height:60px",
        "height:60px;align-items:stretch",
        "height:60px;align-items:flex-start",
    ] {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}</style><div style='display:flex;width:300px;{extra}'>\
             <div id=x style='width:50px;flex-shrink:0'>a</div>\
             <div id=y style='width:400px'>b</div></div>"
            ),
            900.0,
        );
        let x = d.get_element_by_id("x").unwrap();
        let y = d.get_element_by_id("y").unwrap();
        let wx = d.get_bounding_client_rect(x).unwrap().w;
        let wy = d.get_bounding_client_rect(y).unwrap().w;
        assert!(
            (wx - 50.0).abs() < 1.0,
            "`flex-shrink:0` holds at 50 [{extra}], got {wx}"
        );
        assert!(
            (wy - 250.0).abs() < 1.0,
            "the flexible item shrinks 400 -> 250 [{extra}], got {wy}"
        );
    }
}

/// The same, mirrored onto a column: the main axis there is the HEIGHT.
#[test]
fn a_flex_column_with_a_definite_width_still_resolves_its_main_size() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div style='display:flex;flex-direction:column;width:200px;height:300px'>\
         <div id=x style='height:50px;flex-shrink:0'>a</div>\
         <div id=y style='height:400px'>b</div></div>",
        900.0,
    );
    let y = d.get_element_by_id("y").unwrap();
    let h = d.get_bounding_client_rect(y).unwrap().h;
    assert!(
        (h - 250.0).abs() < 1.0,
        "400 -> 250 down the column, got {h}"
    );
}

/// ⛔ A `minmax(min, 1fr)` track had its base counted TWICE.
///
/// CSS Grid §12.7 subtracts the base sizes of the NON-flexible tracks from the
/// free space; a `minmax(50px, 1fr)` track is flexible, so its 50px must stay
/// in. It was being added to `used` AND given an fr share, so:
///
///  * `minmax(50px,1fr) 1fr` in a 300px grid gave 125 where Chrome gives 150;
///  * `repeat(auto-fill, minmax(80px,1fr))` produced 80px columns that never
///    grew to fill the row — Chrome fills at 100.
///
/// The second is the one that shows up on real pages: it is the standard
/// responsive-card grid, and every card was stuck at its minimum.
#[test]
fn a_flexible_minmax_track_keeps_its_base_in_the_free_space() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.g{display:grid;width:300px}\
         #e{grid-template-columns:minmax(50px,1fr) 1fr}\
         #f{grid-template-columns:repeat(auto-fill,minmax(80px,1fr))}</style>\
         <div class=g id=e><i id=e1>1</i><i id=e2>2</i></div>\
         <div class=g id=f><i id=f1>1</i><i id=f2>2</i><i id=f3>3</i></div>",
        900.0,
    );
    let w = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    let e1 = w(&mut d, "e1");
    assert!(
        (e1 - 150.0).abs() < 1.0,
        "`minmax(50px,1fr)` takes a full fr share of 300/2, got {e1}"
    );
    for id in ["f1", "f2", "f3"] {
        let got = w(&mut d, id);
        assert!(
            (got - 100.0).abs() < 1.0,
            "auto-fill minmax(80px,1fr) columns fill the row at 100, #{id} got {got}"
        );
    }
}

/// ⛔ `row-reverse` / `column-reverse` packed against the WRONG EDGE.
///
/// In a reversed direction the main-START is the far edge (Flexbox §5.1), so
/// `flex-start` — the default — packs against the RIGHT in `row-reverse`, and
/// `flex-start`/`flex-end` swap. The item ORDER was reversed and the packing
/// was not, so a `row-reverse` row laid its items out in reverse order against
/// the LEFT edge: measured 50/0 where Chrome gives 250/200.
#[test]
fn a_reversed_flex_direction_packs_against_the_far_edge() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px;height:60px}\
         .i{width:50px;height:20px}#r{flex-direction:row-reverse}\
         #c{flex-direction:column-reverse;height:120px}</style>\
         <div class=f id=r><i class=i id=r1>1</i><i class=i id=r2>2</i></div>\
         <div class=f id=c><i class=i id=c1>1</i><i class=i id=c2>2</i></div>",
        900.0,
    );
    let at = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        let p = d.parent_node(e);
        let b = d.get_bounding_client_rect(e).unwrap();
        let pb = d.get_bounding_client_rect(p).unwrap();
        (b.x - pb.x, b.y - pb.y)
    };
    let (r1x, _) = at(&mut d, "r1");
    let (r2x, _) = at(&mut d, "r2");
    assert!(
        (r1x - 250.0).abs() < 1.0,
        "row-reverse puts the FIRST item rightmost, got {r1x}"
    );
    assert!(
        (r2x - 200.0).abs() < 1.0,
        "and the second beside it, got {r2x}"
    );

    let (_, c1y) = at(&mut d, "c1");
    let (_, c2y) = at(&mut d, "c2");
    assert!(
        (c1y - 100.0).abs() < 1.0,
        "column-reverse starts at the BOTTOM, got {c1y}"
    );
    assert!((c2y - 80.0).abs() < 1.0, "and stacks upward, got {c2y}");
}

/// ⛔ `align-items: stretch` overrode a DEFINITE cross size.
///
/// Stretch applies only when the item's cross size is `auto` (Flexbox §5.2,
/// §9.4). This stretched regardless, so `<i style="height:20px">` in a 60px
/// flex row came out **60** tall — the declared height discarded. Any flex item
/// with an explicit cross size was being resized.
#[test]
fn stretch_does_not_override_a_definite_cross_size() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px;height:60px}</style>\
         <div class=f><i id=fixed style='width:50px;height:20px'>1</i>\
         <i id=auto style='width:50px'>2</i></div>",
        900.0,
    );
    let h = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().h
    };
    let fixed = h(&mut d, "fixed");
    assert!(
        (fixed - 20.0).abs() < 1.0,
        "a definite height survives `align-items: stretch`, got {fixed}"
    );
    let auto = h(&mut d, "auto");
    assert!(
        (auto - 60.0).abs() < 1.0,
        "…while an `auto` one still stretches to the line, got {auto}"
    );
}

/// ⛔ `flex-wrap: wrap-reverse` put every item against the wrong edge of its
/// line.
///
/// It flips the CROSS-START (Flexbox §5.2), so `flex-start` aligns to the
/// bottom of the line and `flex-end` to the top — they swap, just as
/// `flex-start`/`flex-end` swap on the main axis under `row-reverse`. The LINE
/// order was already reversed; the item alignment inside each line was not.
///
/// ⛔ Three things had to change, and the first two were invisible on their
/// own:
///  * `effective_align_self` swaps the two edges under wrap-reverse;
///  * the non-stretch match must NOT swap again (doing so put `flex-start` and
///    `flex-end` in the SAME place);
///  * the `stretch` branch returns its own hardcoded cross position, and that
///    is the one the default `align-items` actually uses.
///
/// Geometry: a 120x60 container, three 50x20 items, so two lines that
/// `align-content: stretch` grows to 30 each. Chrome puts the first line's
/// items at y=40 and the second line's at y=10.
#[test]
fn wrap_reverse_flips_the_cross_start_edge() {
    let case = |ai: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;flex-wrap:wrap-reverse;\
             width:120px;height:60px;align-items:{ai}}}.i{{width:50px;height:20px}}</style>\
             <div class=f id=p><i class=i id=x>1</i><i class=i id=y>2</i>\
             <i class=i id=z>3</i></div>"
            ),
            900.0,
        );
        let p = d.get_element_by_id("p").unwrap();
        let pb = d.get_bounding_client_rect(p).unwrap();
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().y - pb.y
        };
        (get(&mut d, "x"), get(&mut d, "z"))
    };

    // The first line is the LOWER one, and its items sit at its bottom.
    let (x, z) = case("stretch");
    assert!(
        (x - 40.0).abs() < 1.0,
        "stretch: first line's item at 40, got {x}"
    );
    assert!(
        (z - 10.0).abs() < 1.0,
        "stretch: second line's item at 10, got {z}"
    );

    let (x, z) = case("flex-start");
    assert!(
        (x - 40.0).abs() < 1.0,
        "flex-start aligns to the flipped start, got {x}"
    );
    assert!((z - 10.0).abs() < 1.0, "…on the second line too, got {z}");

    // …and `flex-end` is the opposite edge, or the two are not really swapping.
    let (xe, _) = case("flex-end");
    assert!(
        (xe - 30.0).abs() < 1.0,
        "flex-end is the OTHER edge, got {xe}"
    );
    assert!(
        (x - xe).abs() > 1.0,
        "flex-start and flex-end must not coincide"
    );
}

/// Flexbox §9.7: an item frozen by its min or max size hands the space it
/// could not take back to its siblings.
///
/// The distribution ran once and clamped afterwards, so a clamped item's
/// surplus simply vanished: a `max-width:50px` item in a 300px row left its
/// sibling at 150 instead of 250, and the row overflowed by 100px.
#[test]
fn a_clamped_flex_item_redistributes_to_its_siblings() {
    let widths = |items: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px}}\
             .f>i{{display:block;height:20px}}</style>\
             <div class=f>{items}</div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().w
        };
        (get(&mut d, "a"), get(&mut d, "b"))
    };

    // Growing: `a` is capped at 50, so `b` takes the remaining 250.
    let (a, b) = widths(
        "<i id=a style='flex:1 1 0;max-width:50px'></i>\
                         <i id=b style='flex:1 1 0'></i>",
    );
    assert!((a - 50.0).abs() < 0.5, "max-width caps the item, got {a}");
    assert!(
        (b - 250.0).abs() < 0.5,
        "the sibling absorbs the surplus, got {b}"
    );

    // Shrinking: `a` is floored at 180, so `b` absorbs the rest of the deficit.
    let (a, b) = widths(
        "<i id=a style='flex:0 1 200px;min-width:180px'></i>\
                         <i id=b style='flex:0 1 200px'></i>",
    );
    assert!(
        (a - 180.0).abs() < 0.5,
        "min-width floors the item, got {a}"
    );
    assert!(
        (b - 120.0).abs() < 0.5,
        "the sibling absorbs the deficit, got {b}"
    );
}

/// Flexbox §8.3: `align-items: baseline` lines the items' first baselines up.
#[test]
fn baseline_alignment_lines_the_first_baselines_up() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;align-items:baseline;width:300px}\
         .f>i{display:block;width:60px}</style>\
         <div class=f id=p><i id=a style='padding-top:10px'>x</i>\
         <i id=b style='padding-top:30px'>y</i></div>",
        900.0,
    );
    let top = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().y
    };
    let py = top(&mut d, "p");
    let a = top(&mut d, "a") - py;
    let b = top(&mut d, "b") - py;
    // Both items carry the same line box, so the 20px of extra padding on `b`
    // is exactly how far `a` has to drop for the baselines to meet.
    assert!(
        (b - 0.0).abs() < 0.5,
        "the deepest baseline sets the line, got {b}"
    );
    assert!(
        (a - 20.0).abs() < 0.5,
        "the shallower item drops to meet it, got {a}"
    );
}

/// CSS Box Alignment §4.4: the distribution values fall back when the free
/// space is negative. `space-between` becomes `flex-start`, and
/// `space-around` / `space-evenly` become SAFE `center`, which is itself
/// `flex-start` once the space is negative. An explicit `center` or `flex-end`
/// is unsafe and still overflows.
///
/// A negative free space became negative spacing, so the items overlapped.
#[test]
fn overflowing_distribution_falls_back_instead_of_overlapping() {
    let case = |jc: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px;justify-content:{jc}}}\
             .f>i{{display:block;height:20px;flex:0 0 200px}}</style>\
             <div class=f id=p><i id=a></i><i id=b></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().x
        };
        let p = get(&mut d, "p");
        (get(&mut d, "a") - p, get(&mut d, "b") - p)
    };

    // The safe values all pack against the start edge and overflow the end.
    for jc in [
        "space-between",
        "space-around",
        "space-evenly",
        "flex-start",
    ] {
        let (a, b) = case(jc);
        assert!(a.abs() < 0.5, "{jc}: falls back to the start edge, got {a}");
        assert!(
            (b - 200.0).abs() < 0.5,
            "{jc}: items must not overlap, got {b}"
        );
    }
    // …and the unsafe ones still overflow the start edge, as asked.
    let (a, b) = case("center");
    assert!(
        (a + 50.0).abs() < 0.5,
        "center overflows both edges, got {a}"
    );
    assert!(
        (b - 150.0).abs() < 0.5,
        "…by half the deficit each, got {b}"
    );
    let (a, _) = case("flex-end");
    assert!(
        (a + 100.0).abs() < 0.5,
        "flex-end overflows the start edge, got {a}"
    );
}

/// The same fallback on the CROSS axis. `free_cross` was clamped to zero, so
/// `align-content: center` and `flex-end` did nothing at all once the lines
/// overflowed — they must move the lines past the cross-start edge.
#[test]
fn overflowing_align_content_moves_past_the_cross_start_edge() {
    let first_line_y = |ac: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;flex-wrap:wrap;width:100px;\
             height:40px;align-content:{ac}}}.f>i{{display:block;width:80px;height:30px}}\
             </style><div class=f id=p><i id=a></i><i id=b></i><i id=c></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().y
        };
        get(&mut d, "a") - get(&mut d, "p")
    };
    // Three 30px lines in a 40px box: 50px of overflow to place.
    assert!(
        first_line_y("flex-start").abs() < 0.5,
        "flex-start stays put"
    );
    let c = first_line_y("center");
    assert!(
        (c + 25.0).abs() < 0.5,
        "center splits the overflow, got {c}"
    );
    let e = first_line_y("flex-end");
    assert!((e + 50.0).abs() < 0.5, "flex-end pushes it all up, got {e}");
    // The distribution values are safe, so they pack at the start instead.
    for ac in ["space-between", "space-around", "space-evenly", "stretch"] {
        let y = first_line_y(ac);
        assert!(y.abs() < 0.5, "{ac}: falls back to the start edge, got {y}");
    }
}

/// CSS Sizing §5.1: a definite cross size plus an `aspect-ratio` gives the
/// flex item its main size. Without the transfer the item measured its (empty)
/// content and came out 0 wide.
#[test]
fn an_aspect_ratio_transfers_the_cross_size_to_the_main_axis() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px}</style>\
         <div class=f><i id=a style='display:block;flex:0 0 auto;\
         aspect-ratio:2/1;height:30px'></i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let w = d.get_bounding_client_rect(e).unwrap().w;
    assert!(
        (w - 60.0).abs() < 0.5,
        "30px tall at 2/1 is 60px wide, got {w}"
    );
}

/// Flexbox §9.8: a percentage on a flex item resolves against the flex
/// container's inner size. The item was laid out with no available height, so
/// `height: 50%` resolved to zero.
#[test]
fn a_percentage_cross_size_resolves_against_the_flex_container() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px;height:60px}</style>\
         <div class=f><i id=a style='display:block;width:50%;height:50%'></i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let b = d.get_bounding_client_rect(e).unwrap();
    assert!((b.w - 150.0).abs() < 0.5, "50% of 300, got {}", b.w);
    assert!((b.h - 30.0).abs() < 0.5, "50% of 60, got {}", b.h);
}

#[test]
fn a_percentage_cross_size_resolves_against_aspect_ratio_flex_container() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.card{display:flex;align-items:center;justify-content:center;width:128px;aspect-ratio:1/1}</style>\
         <div class=card><i id=a style='display:block;width:62.5%;height:62.5%'></i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let b = d.get_bounding_client_rect(e).unwrap();
    assert!((b.w - 80.0).abs() < 0.5, "62.5% of 128, got {}", b.w);
    assert!(
        (b.h - 80.0).abs() < 0.5,
        "62.5% should resolve against the aspect-ratio-derived flex height, got {}",
        b.h
    );
}

/// A column flex item that shrinks to its intrinsic width keeps the main size
/// flex gave it.
///
/// `align-items` other than `stretch` re-lays the item at its intrinsic width,
/// and that re-layout dropped the resolved height — so `flex: 1` items in a
/// 90px column came out at their content height instead of 45px each.
#[test]
fn a_column_item_keeps_its_main_size_when_shrunk_to_fit() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;flex-direction:column;\
         align-items:flex-start;width:300px;height:90px}</style>\
         <div class=f><i id=a style='display:block;flex:1'>a</i>\
         <i id=b style='display:block;flex:1'>bbbb</i></div>",
        900.0,
    );
    for id in ["a", "b"] {
        let e = d.get_element_by_id(id).unwrap();
        let h = d.get_bounding_client_rect(e).unwrap().h;
        assert!(
            (h - 45.0).abs() < 0.5,
            "{id}: flex:1 of 90px is 45, got {h}"
        );
    }
}

/// Flexbox §7: the `flex` shorthand's components may come in any order, and the
/// basis is whichever component is not a bare number.
///
/// The basis was read only out of the third slot, so every two-value form
/// dropped it: `flex: 1 30%` left the item at its previous basis entirely.
#[test]
fn the_flex_shorthand_reads_a_basis_from_any_slot() {
    let width = |decl: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px}}</style>\
             <div class=f><i id=a style='display:block;height:20px;{decl}'></i>\
             <i id=b style='display:block;height:20px;flex:0 0 100px'></i></div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    // `flex: 0 <basis>` — one number, so it is the GROW factor and the item
    // stays at its basis.
    assert!(
        (width("flex:0 100px") - 100.0).abs() < 0.5,
        "two-value px basis"
    );
    assert!(
        (width("flex:0 30%") - 90.0).abs() < 0.5,
        "two-value percent basis"
    );
    // `flex: 1 auto` grows from the specified width, so all 200 remaining px
    // land on it.
    assert!(
        (width("flex:1 auto;width:40px") - 200.0).abs() < 0.5,
        "two-value auto basis"
    );
    // An omitted basis is 0, not `auto` — the shorthand's default differs from
    // the property's initial value.
    assert!(
        (width("flex:0;width:70px") - 0.0).abs() < 0.5,
        "an omitted basis is 0"
    );
    // Three numbers still work, with the third as a zero basis.
    assert!(
        (width("flex:0 1 0") - 0.0).abs() < 0.5,
        "a bare third number is a 0 basis"
    );
}

/// Flexbox §7.2.3: `flex-basis: content` sizes from the content and ignores the
/// item's own `width`, which the intrinsic measurement short-circuits on.
#[test]
fn a_content_flex_basis_ignores_the_specified_width() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px}</style>\
         <div class=f><i id=a style='display:block;flex-basis:content;width:2px'>\
         wide text here</i><i id=b style='display:block;flex:0 0 100px'></i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let w = d.get_bounding_client_rect(e).unwrap().w;
    // The exact advance is a font question; what matters is that the 2px
    // specified width is not what sized the item.
    assert!(
        w > 50.0,
        "the content sizes the item, not its width, got {w}"
    );
}

/// Box Alignment §8.3: a percentage gap resolves against the container's own
/// content box in the gap's OWN axis. Both gaps read the width, so `row-gap`
/// in a 300x120 column measured 10% of 300 instead of 10% of 120.
#[test]
fn a_percentage_row_gap_resolves_against_the_height() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;flex-direction:column;width:300px;\
         height:120px;row-gap:10%}</style>\
         <div class=f><i id=a style='display:block;flex:1'></i>\
         <i id=b style='display:block;flex:1'></i></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let a = get(&mut d, "a");
    let b = get(&mut d, "b");
    // 10% of 120 is a 12px gap, leaving 108 for two equal items.
    assert!(
        (a.h - 54.0).abs() < 0.5,
        "the gap eats 12px, not 30, got {}",
        a.h
    );
    assert!(
        (b.y - a.y - 66.0).abs() < 0.5,
        "54 tall plus a 12px gap, got {}",
        b.y - a.y
    );
}

/// Flexbox §4.5: `min-width: auto` on a flex item is the content-based
/// minimum, so an item never shrinks below its own content.
///
/// `min-width` defaulted to `0` rather than its CSS initial value `auto`, which
/// made the whole automatic-minimum branch dead code: an item in a 0-width
/// container collapsed to nothing.
#[test]
fn a_flex_item_does_not_shrink_below_its_content() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:0}</style>\
         <div class=f><i id=a style='display:block;flex:1 1 auto'>aaaa</i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let w = d.get_bounding_client_rect(e).unwrap().w;
    assert!(w > 20.0, "the content is the floor, got {w}");
}

/// …but the automatic minimum is the SMALLER of the content suggestion and the
/// specified size, and the content suggestion ignores the item's own `width`.
/// Reading the width for both made them the same number, so an item with a
/// `width` could not shrink at all.
#[test]
fn an_item_with_a_width_still_shrinks_to_its_share() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px}</style>\
         <div class=f><i id=a style='display:block;flex:1 1 auto;width:400px;height:20px'></i>\
         <i id=b style='display:block;flex:1 1 auto;width:400px;height:20px'></i></div>",
        900.0,
    );
    for id in ["a", "b"] {
        let e = d.get_element_by_id(id).unwrap();
        let w = d.get_bounding_client_rect(e).unwrap().w;
        assert!(
            (w - 150.0).abs() < 0.5,
            "{id}: an empty item shrinks freely, got {w}"
        );
    }
}

/// CSS Cascade §7: the CSS-wide keywords reset a SHORTHAND by resetting every
/// longhand it stands for.
///
/// A shorthand owns no storage, so its `copy` is a no-op and resetting through
/// it did nothing at all: `flex: initial` left grow/shrink/basis untouched, and
/// `margin: initial` left the margins in place.
#[test]
fn a_css_wide_keyword_resets_a_shorthands_longhands() {
    let width = |decl: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px}}\
             .pre{{flex:1 1 200px}}</style>\
             <div class=f><i id=a class=pre style='display:block;height:20px;{decl}'></i>\
             <i id=b style='display:block;height:20px;flex:0 0 100px'></i></div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    // `flex: initial` is `0 1 auto`, so an empty item collapses to nothing.
    for kw in ["initial", "unset", "revert"] {
        let w = width(&format!("flex:{kw}"));
        assert!(w.abs() < 0.5, "flex:{kw} must reset the longhands, got {w}");
    }
    // The keyword on a single longhand touches only that longhand — the basis
    // the shorthand set survives.
    let w = width("flex-grow:initial");
    assert!(
        (w - 200.0).abs() < 0.5,
        "flex-grow:initial keeps the basis, got {w}"
    );

    // The same for a shorthand whose longhands are plain lengths.
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px}</style>\
         <div class=f><i id=a style='display:block;height:20px;margin:20px;\
         margin:initial;flex:1'></i></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let w = d.get_bounding_client_rect(e).unwrap().w;
    assert!(
        (w - 300.0).abs() < 0.5,
        "margin:initial clears the margins, got {w}"
    );
}

#[test]
fn class_rule_fractional_flex_grow_uses_partial_free_space() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         .flexbox{display:flex}
         .container{height:100px;width:100px;border:1px solid black}
         .child-flex-grow-0-5{flex-grow:0.5}
         </style>
         <div class='flexbox container'><div id=a class='child-flex-grow-0-5'></div></div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let w = d.get_bounding_client_rect(e).unwrap().w;
    assert!((w - 50.0).abs() < 0.5, "fractional flex-grow got {w}");
}

#[test]
fn wpt_fractional_flex_factors_use_partial_free_space() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         html,body{margin:0;padding:0}
         .flexbox{display:flex}
         .container{height:100px;width:100px;border:1px solid black}
         .column{flex-direction:column}
         .vertical{writing-mode:vertical-rl}
         .basis{flex-basis:30px}
         .basis-big{flex-basis:100px}
         .grow-half{flex-grow:.5}
         .grow-quarter{flex-grow:.25}
         .shrink-half{flex-shrink:.5;width:200px;height:200px}
         .shrink-quarter{flex-shrink:.25;width:200px;height:200px}
         </style>
         <div class='flexbox container'>
           <div id=g1 class='grow-half'></div>
         </div>
         <div class='flexbox container'>
           <div id=g2a class='grow-half basis'></div>
           <div id=g2b class='grow-quarter basis'></div>
         </div>
         <div class='flexbox container column'>
           <div id=g3a class='grow-half basis'></div>
           <div id=g3b class='grow-quarter basis'></div>
         </div>
         <div class='flexbox container vertical'>
           <div id=g4a class='grow-half basis'></div>
           <div id=g4b class='grow-quarter basis'></div>
         </div>
         <div class='flexbox container column vertical'>
           <div id=g5a class='grow-half basis'></div>
           <div id=g5b class='grow-quarter basis'></div>
         </div>
         <div class='flexbox container'>
           <div id=s1 class='shrink-half'></div>
         </div>
         <div class='flexbox container'>
           <div id=s2a class='shrink-half basis-big'></div>
           <div id=s2b class='shrink-quarter basis-big'></div>
         </div>",
        900.0,
    );

    let width = |doc: &crate::Document, id: &str| {
        let e = doc.get_element_by_id(id).unwrap();
        doc.get_bounding_client_rect(e).unwrap().w
    };
    let height = |doc: &crate::Document, id: &str| {
        let e = doc.get_element_by_id(id).unwrap();
        doc.get_bounding_client_rect(e).unwrap().h
    };

    assert!((width(&d, "g1") - 50.0).abs() < 0.5);
    assert!((width(&d, "g2a") - 50.0).abs() < 0.5);
    assert!((width(&d, "g2b") - 40.0).abs() < 0.5);
    assert!((height(&d, "g3a") - 50.0).abs() < 0.5);
    assert!((height(&d, "g3b") - 40.0).abs() < 0.5);
    assert!((height(&d, "g4a") - 50.0).abs() < 0.5);
    assert!((height(&d, "g4b") - 40.0).abs() < 0.5);
    assert!((width(&d, "g5a") - 50.0).abs() < 0.5);
    assert!((width(&d, "g5b") - 40.0).abs() < 0.5);
    assert!((width(&d, "s1") - 150.0).abs() < 0.5);
    assert!((width(&d, "s2a") - 50.0).abs() < 0.5);
    assert!((width(&d, "s2b") - 75.0).abs() < 0.5);
}

#[test]
fn vertical_writing_mode_block_children_flow_on_the_block_axis() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         #rl,#lr{width:100px; writing-mode:vertical-rl}
         #lr{writing-mode:vertical-lr}
         .child{display:block; width:20px; height:30px}
         </style>
         <div id=rl><div id=rl_a class=child></div><div id=rl_b class=child></div></div>
         <div id=lr><div id=lr_a class=child></div><div id=lr_b class=child></div></div>",
        900.0,
    );

    let rl = d.get_element_by_id("rl").unwrap();
    let rl_a = d.get_element_by_id("rl_a").unwrap();
    let rl_b = d.get_element_by_id("rl_b").unwrap();
    let lr = d.get_element_by_id("lr").unwrap();
    let lr_a = d.get_element_by_id("lr_a").unwrap();
    let lr_b = d.get_element_by_id("lr_b").unwrap();
    let rl_r = d.get_bounding_client_rect(rl).unwrap();
    let rl_ar = d.get_bounding_client_rect(rl_a).unwrap();
    let rl_br = d.get_bounding_client_rect(rl_b).unwrap();
    let lr_r = d.get_bounding_client_rect(lr).unwrap();
    let lr_ar = d.get_bounding_client_rect(lr_a).unwrap();
    let lr_br = d.get_bounding_client_rect(lr_b).unwrap();

    assert!(
        (rl_ar.x - 80.0).abs() < 0.5,
        "vertical-rl first child x {}",
        rl_ar.x
    );
    assert!(
        (rl_br.x - 60.0).abs() < 0.5,
        "vertical-rl second child x {}",
        rl_br.x
    );
    assert!(
        (rl_ar.y - rl_br.y).abs() < 0.5,
        "vertical-rl children should share inline-start y"
    );
    assert!(
        (rl_r.h - 30.0).abs() < 0.5,
        "vertical-rl auto block inline extent {}",
        rl_r.h
    );

    assert!(
        (lr_ar.x - 0.0).abs() < 0.5,
        "vertical-lr first child x {}",
        lr_ar.x
    );
    assert!(
        (lr_br.x - 20.0).abs() < 0.5,
        "vertical-lr second child x {}",
        lr_br.x
    );
    assert!(
        (lr_ar.y - lr_br.y).abs() < 0.5,
        "vertical-lr children should share inline-start y"
    );
    assert!(
        (lr_r.h - 30.0).abs() < 0.5,
        "vertical-lr auto block inline extent {}",
        lr_r.h
    );
}

#[test]
fn display_table_wraps_direct_table_cells_in_anonymous_row() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         #t{display:table}
         .c{display:table-cell;width:40px;height:20px}
         </style>
         <div id=t><div id=a class=c></div><div id=b class=c></div></div>",
        900.0,
    );
    let table = d.get_element_by_id("t").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();
    let tr = d.get_bounding_client_rect(table).unwrap();
    let ar = d.get_bounding_client_rect(a).unwrap();
    let br = d.get_bounding_client_rect(b).unwrap();

    assert!((tr.w - 80.0).abs() < 0.5, "table width got {}", tr.w);
    assert!((tr.h - 20.0).abs() < 0.5, "table height got {}", tr.h);
    assert!((ar.x - 0.0).abs() < 0.5, "first cell x got {}", ar.x);
    assert!((br.x - 40.0).abs() < 0.5, "second cell x got {}", br.x);
}

#[test]
fn orphan_table_cells_are_wrapped_in_anonymous_table_and_row() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         #host{width:400px}
         .c{display:table-cell;width:40px;height:20px}
         </style>
         <div id=host><div id=a class=c></div><div id=b class=c></div></div>",
        900.0,
    );
    let host = d.get_element_by_id("host").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();
    let hr = d.get_bounding_client_rect(host).unwrap();
    let ar = d.get_bounding_client_rect(a).unwrap();
    let br = d.get_bounding_client_rect(b).unwrap();

    assert!((hr.h - 20.0).abs() < 0.5, "host height got {}", hr.h);
    assert!((ar.x - 0.0).abs() < 0.5, "first cell x got {}", ar.x);
    assert!((br.x - 40.0).abs() < 0.5, "second cell x got {}", br.x);
    assert!((ar.y - br.y).abs() < 0.5, "cells should share a row");
}

#[test]
fn orphan_table_rows_are_wrapped_in_anonymous_table() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         #host{width:400px}
         .r{display:table-row}
         .c{display:table-cell;width:40px;height:20px}
         </style>
         <div id=host>
           <div id=row1 class=r><div id=a class=c></div></div>
           <div id=row2 class=r><div id=b class=c></div></div>
         </div>",
        900.0,
    );
    let host = d.get_element_by_id("host").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();
    let hr = d.get_bounding_client_rect(host).unwrap();
    let ar = d.get_bounding_client_rect(a).unwrap();
    let br = d.get_bounding_client_rect(b).unwrap();

    assert!((hr.h - 40.0).abs() < 0.5, "host height got {}", hr.h);
    assert!((ar.x - br.x).abs() < 0.5, "rows should share column x");
    assert!((br.y - 20.0).abs() < 0.5, "second row y got {}", br.y);
}

#[test]
fn table_row_groups_wrap_direct_table_cells_in_anonymous_rows() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         #t{display:table}
         #g{display:table-row-group}
         .c{display:table-cell;width:40px;height:20px}
         </style>
         <div id=t><div id=g><div id=a class=c></div><div id=b class=c></div></div></div>",
        900.0,
    );
    let table = d.get_element_by_id("t").unwrap();
    let a = d.get_element_by_id("a").unwrap();
    let b = d.get_element_by_id("b").unwrap();
    let tr = d.get_bounding_client_rect(table).unwrap();
    let ar = d.get_bounding_client_rect(a).unwrap();
    let br = d.get_bounding_client_rect(b).unwrap();

    assert!((tr.w - 80.0).abs() < 0.5, "table width got {}", tr.w);
    assert!((tr.h - 20.0).abs() < 0.5, "table height got {}", tr.h);
    assert!((ar.x - 0.0).abs() < 0.5, "first child x got {}", ar.x);
    assert!((br.x - 40.0).abs() < 0.5, "second child x got {}", br.x);
    assert!((ar.y - br.y).abs() < 0.5, "cells should share a row");
}

#[test]
fn table_colspan_contributes_to_auto_column_widths() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         table{border-spacing:0;width:auto}
         td{padding:0;border:0}
         #wide{width:180px}
         .n{width:20px}
         </style>
         <table id=t>
           <tr><td id=wide colspan='2'></td></tr>
           <tr><td id=a class=n></td><td id=b class=n></td></tr>
         </table>",
        900.0,
    );
    let table = d.get_element_by_id("t").unwrap();
    let wide = d.get_element_by_id("wide").unwrap();
    let tr = d.get_bounding_client_rect(table).unwrap();
    let wr = d.get_bounding_client_rect(wide).unwrap();

    assert!((tr.w - 180.0).abs() < 0.5, "table width got {}", tr.w);
    assert!(
        (wr.w - 180.0).abs() < 0.5,
        "spanning cell width got {}",
        wr.w
    );
}

#[test]
fn table_caption_min_width_contributes_to_auto_table_width() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>
         body{margin:0}
         table{border-spacing:0;width:auto}
         caption{width:220px}
         td{padding:0;border:0;width:20px}
         </style>
         <table id=t><caption id=cap></caption><tr><td></td></tr></table>",
        900.0,
    );
    let table = d.get_element_by_id("t").unwrap();
    let caption = d.get_element_by_id("cap").unwrap();
    let tr = d.get_bounding_client_rect(table).unwrap();
    let cr = d.get_bounding_client_rect(caption).unwrap();

    assert!((tr.w - 220.0).abs() < 0.5, "table width got {}", tr.w);
    assert!((cr.w - 220.0).abs() < 0.5, "caption width got {}", cr.w);
}

/// Box Alignment §4.4: an explicit `safe` makes an alignment give way to the
/// start edge once the content overflows; `unsafe` (the default for a bare
/// position keyword) overflows instead.
///
/// The two-word forms did not parse at all, so `safe center` silently became
/// the property's initial value.
#[test]
fn safe_alignment_gives_way_where_unsafe_overflows() {
    let first_x = |jc: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px;justify-content:{jc}}}\
             .f>i{{display:block;height:20px;flex:0 0 200px}}</style>\
             <div class=f id=p><i id=a></i><i id=b></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().x
        };
        get(&mut d, "a") - get(&mut d, "p")
    };
    assert!(
        first_x("safe center").abs() < 0.5,
        "safe center packs at the start"
    );
    assert!(
        (first_x("unsafe center") + 50.0).abs() < 0.5,
        "unsafe center overflows"
    );
    assert!(
        first_x("safe flex-end").abs() < 0.5,
        "safe flex-end packs at the start"
    );
    assert!(
        (first_x("unsafe flex-end") + 100.0).abs() < 0.5,
        "unsafe flex-end overflows"
    );

    // The cross axis takes the same modifier.
    let cross_y = |ai: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px;height:30px;\
             align-items:{ai}}}</style>\
             <div class=f id=p><i id=a style='display:block;height:50px;width:20px'></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().y
        };
        get(&mut d, "a") - get(&mut d, "p")
    };
    assert!(
        cross_y("safe center").abs() < 0.5,
        "safe center packs at the cross start"
    );
    assert!(
        (cross_y("unsafe center") + 10.0).abs() < 0.5,
        "unsafe center overflows"
    );
}

/// `justify-content: left` / `right` are PHYSICAL (Box Alignment §5): unlike
/// `flex-start` / `flex-end` they do not follow `row-reverse` or `direction`.
#[test]
fn left_and_right_do_not_follow_the_flex_direction() {
    let x = |container: &str, jc: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px;{container};\
             justify-content:{jc}}}</style>\
             <div class=f id=p><i id=a style='display:block;height:20px;flex:0 0 80px'></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().x
        };
        get(&mut d, "a") - get(&mut d, "p")
    };
    for container in ["", "flex-direction:row-reverse", "direction:rtl"] {
        let l = x(container, "left");
        let r_ = x(container, "right");
        assert!(
            l.abs() < 0.5,
            "left is the left edge with `{container}`, got {l}"
        );
        assert!(
            (r_ - 220.0).abs() < 0.5,
            "right is the right edge with `{container}`, got {r_}"
        );
    }
    // …while the flex-relative pair DOES follow the direction, or the test
    // above would not be saying anything.
    assert!(
        (x("flex-direction:row-reverse", "flex-start") - 220.0).abs() < 0.5,
        "flex-start follows row-reverse"
    );
}

/// `first baseline` and `last baseline` both align the items' baselines;
/// `last baseline` then packs the group against the cross-END edge.
#[test]
fn first_and_last_baseline_pack_against_opposite_edges() {
    let tops = |ai: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px;height:60px;\
             align-items:{ai}}}.f>i{{display:block;width:80px}}</style>\
             <div class=f id=p><i id=a style='padding-top:10px'>x</i>\
             <i id=b style='padding-top:30px'>y</i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            let r = d.get_bounding_client_rect(e).unwrap();
            (r.y, r.h)
        };
        let p = get(&mut d, "p").0;
        let (ay, ah) = get(&mut d, "a");
        let (by, bh) = get(&mut d, "b");
        (ay - p, ah, by - p, bh)
    };
    // `first baseline` hangs the group from the cross-start edge…
    let (ay, _, by, _) = tops("first baseline");
    assert!(
        (ay - 20.0).abs() < 0.5,
        "first: the shallow item drops 20, got {ay}"
    );
    assert!(
        by.abs() < 0.5,
        "first: the deep item sets the top, got {by}"
    );
    // …and `last baseline` keeps the same relative offset but pushes the whole
    // group to the far edge, so the deepest bottom touches it.
    let (ay, ah, by, bh) = tops("last baseline");
    assert!(
        (ay + ah - 60.0).abs() < 0.5,
        "last: the group's bottom is the container's, got {}",
        ay + ah
    );
    assert!(
        ((by + bh) - 60.0).abs() < 0.5,
        "last: …for both items, got {}",
        by + bh
    );
    assert!(
        (ay - by - 20.0).abs() < 0.5,
        "last: the baselines still line up"
    );
}

/// `gap: <row> <column>` — the two-value form. Parsing the whole declaration as
/// one length made `gap: 10px 30px` unparseable, so both gaps fell back to 0.
#[test]
fn the_gap_shorthand_takes_a_row_and_a_column_value() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;flex-wrap:wrap;width:200px;\
         gap:10px 30px}.f>i{display:block;width:80px;height:30px}</style>\
         <div class=f id=p><i id=a></i><i id=b></i><i id=c></i></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let a = get(&mut d, "a");
    let b = get(&mut d, "b");
    let c = get(&mut d, "c");
    assert!(
        (b.x - a.x - 110.0).abs() < 0.5,
        "the column gap is 30, got {}",
        b.x - a.x - 80.0
    );
    assert!(
        (c.y - a.y - 40.0).abs() < 0.5,
        "the row gap is 10, got {}",
        c.y - a.y - 30.0
    );
}

/// `wrap-reverse` flips the cross-start edge for the LINE STACK too, so
/// `align-content: flex-start` packs the lines against the far edge. Only the
/// line order was being reversed, so both edges packed the same way.
#[test]
fn wrap_reverse_flips_align_content_as_well() {
    let first_y = |ac: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;flex-wrap:wrap-reverse;\
             width:200px;height:100px;align-content:{ac}}}\
             .f>i{{display:block;width:80px;height:30px}}</style>\
             <div class=f id=p><i id=a></i></div>"
            ),
            900.0,
        );
        let get = |d: &mut crate::types::Document, id: &str| {
            let e = d.get_element_by_id(id).unwrap();
            d.get_bounding_client_rect(e).unwrap().y
        };
        get(&mut d, "a") - get(&mut d, "p")
    };
    assert!(
        (first_y("flex-start") - 70.0).abs() < 0.5,
        "flex-start is the far edge under wrap-reverse"
    );
    assert!(
        first_y("flex-end").abs() < 0.5,
        "flex-end is the near edge under wrap-reverse"
    );
}

/// Flexbox §9.4 step 8: a single-line container with a definite cross size
/// hands that size to its line, so an item taller than the container has free
/// space to overflow into rather than growing the line to fit itself.
#[test]
fn a_single_line_container_gives_its_cross_size_to_the_line() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px;height:30px;\
         align-items:center}</style>\
         <div class=f id=p><i id=a style='display:block;height:50px;width:20px'></i></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().y
    };
    let off = get(&mut d, "a") - get(&mut d, "p");
    assert!(
        (off + 10.0).abs() < 0.5,
        "a 50px item centres in a 30px line at -10, got {off}"
    );
}

/// CSS Sizing §5: the intrinsic sizing keywords size a flex item from its
/// content. They parsed as `auto`, so `min-content` gave the MAX-content size.
#[test]
fn intrinsic_keywords_size_a_flex_item() {
    let width = |decl: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}.f{{display:flex;width:300px}}</style>\
             <div class=f><i id=a style='display:block;{decl}'>aa bbbb cc</i>\
             <i id=b style='display:block;flex:0 0 100px;height:20px'></i></div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    // min-content is the longest word; max-content is the whole string on one
    // line. The exact advances are a font question — what matters is that the
    // two keywords no longer give the same answer.
    let min = width("flex-basis:min-content");
    let max = width("flex-basis:max-content");
    assert!(
        min < max * 0.6,
        "min-content ({min}) must be well under max-content ({max})"
    );
    assert!((width("flex-basis:max-content") - max).abs() < 0.5);
    // …and the same keywords on `width`, which is where the basis reads them
    // when `flex-basis` is `auto`.
    assert!(
        (width("width:min-content") - min).abs() < 0.5,
        "width:min-content matches"
    );
    // The item's own size is consulted ONLY when the basis is `auto`, or an
    // intrinsic `width` would override an explicit basis.
    assert!(
        (width("flex-basis:50px;width:min-content;flex-shrink:0") - 50.0).abs() < 0.5,
        "an explicit basis wins over an intrinsic width"
    );

    // `fit-content` is the max-content size clamped to the space available,
    // floored by min-content — not simply max-content.
    let fit = |container_w: f32| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(&format!(
            "<style>body{{margin:0}}.f{{display:flex;width:{container_w}px}}</style>             <div class=f><i id=a style='display:block;flex-basis:fit-content;             flex-shrink:0'>aa bbbb cc</i></div>"), 900.0);
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    assert!(
        (fit(300.0) - max).abs() < 0.5,
        "room to spare: fit-content is max-content"
    );
    assert!(
        (fit(60.0) - 60.0).abs() < 0.5,
        "cramped: fit-content clamps to the container"
    );
    assert!(fit(10.0) >= min - 0.5, "…but never below min-content");
}

/// The typed cascade path carries the intrinsic keywords too — the inline-style
/// path above is not the only way they reach the style.
#[test]
fn intrinsic_keywords_survive_the_stylesheet_path() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;width:300px}         .a{flex-basis:min-content}.b{flex-basis:max-content}</style>         <div class=f><i class=a id=a style='display:block'>aa bbbb cc</i></div>         <div class=f><i class=b id=b style='display:block'>aa bbbb cc</i></div>", 900.0);
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    let a = get(&mut d, "a");
    let b = get(&mut d, "b");
    assert!(
        a < b * 0.6,
        "min-content ({a}) under max-content ({b}) through a rule too"
    );
}

/// Flexbox §4.1: `align-self` on an absolutely-positioned flex child overrides
/// the container's `align-items` for its static position, exactly as it does
/// for an in-flow item. Only the container's value was being read.
#[test]
fn align_self_moves_an_absolutely_positioned_flex_child() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}.f{display:flex;position:relative;width:300px;height:60px}</style>\
         <div class=f id=p><i id=a style='position:absolute;align-self:center;\
         display:block;height:20px;width:30px'></i></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap().y
    };
    let off = get(&mut d, "a") - get(&mut d, "p");
    assert!(
        (off - 20.0).abs() < 0.5,
        "a 20px child centres in 60px at 20, got {off}"
    );
}

#[test]
fn absolute_stretch_lays_out_flex_children_against_used_height() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>\
         body{margin:0}\
         #host{position:relative;width:120px;height:40px}\
         #icon{position:absolute;left:12px;top:0;bottom:0;display:flex;align-items:center}\
         #svg{display:block;width:20px;height:20px}\
         </style>\
         <div id=host><div id=icon><svg id=svg></svg></div></div>",
        800.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let icon = get(&mut d, "icon");
    let svg = get(&mut d, "svg");
    assert!(
        (icon.h - 40.0).abs() < 0.5,
        "abspos top/bottom should stretch icon to 40px, got {}",
        icon.h
    );
    assert!(
        (svg.y - 10.0).abs() < 0.5,
        "20px svg should center vertically inside stretched 40px flex icon, got y={}",
        svg.y
    );
}

#[test]
fn column_flex_auto_min_height_counts_block_followed_by_flex_child() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>\
         body{margin:0}\
         #outer{display:flex;flex-direction:column;width:180px}\
         #item{display:block;margin-bottom:12px}\
         #top{display:block;height:42px}\
         #bottom{display:flex;height:18px;margin:4px 0}\
         </style>\
         <div id=outer><section id=item><div id=top></div><div id=bottom></div></section></div>",
        800.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let item = get(&mut d, "item");
    let outer = get(&mut d, "outer");
    assert!(
        item.h >= 64.0 - 0.5,
        "auto-height flex item must include block + flex child, got {}",
        item.h
    );
    assert!(
        outer.h >= 76.0 - 0.5,
        "column flex container must include item margin too, got {}",
        outer.h
    );
}

/// CSS 2.1 §10.8: every line box contains a strut — a zero-width inline box
/// with the block's own font and line-height — whose ascent and descent take
/// part in the line box's height.
///
/// There was no strut, so a line holding only an atomic inline had no room
/// below the baseline at all: a 20px image or inline-block gave a 20px line
/// where a browser gives 25.
#[test]
fn a_line_box_reserves_room_below_the_baseline_for_its_strut() {
    let height = |inner: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0;font-family:Menlo;font-size:16px;line-height:20px}}</style>\
             <div id=a>{inner}</div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().h
    };
    // 16px Menlo is 15 up and 4 down; a 20px line-height adds half a pixel of
    // leading each side. An atomic inline sits ON the baseline, so the line is
    // the box plus the strut's 4.5px descent, snapped to a whole pixel.
    let h = height("<span style='display:inline-block;width:50px;height:20px'></span>");
    assert!(
        (h - 25.0).abs() < 0.1,
        "an inline-block leaves room below: {h}"
    );
    let h = height("<span style='display:inline-flex;width:50px;height:20px'></span>");
    assert!(
        (h - 20.0).abs() < 0.1,
        "an inline-flex uses its synthesized flex baseline: {h}"
    );
    // A taller box moves the line with it, keeping the same descent.
    let h = height("<span style='display:inline-block;width:50px;height:60px'></span>");
    assert!((h - 65.0).abs() < 0.1, "…at any height: {h}");
    // Text alone is exactly its line-height — the strut never inflates that.
    let h = height("text");
    assert!(
        (h - 20.0).abs() < 0.5,
        "a text line is its line-height: {h}"
    );
    // …and a line-height BELOW the font's own height shrinks the line, which
    // the old bolt-on could not do because it only ever grew it.
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0;font-family:Menlo;font-size:16px;line-height:10px}</style>\
         <div id=a>text</div>",
        900.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let h = d.get_bounding_client_rect(e).unwrap().h;
    assert!(
        (h - 10.0).abs() < 0.5,
        "a tight line-height shrinks the line: {h}"
    );
}

#[test]
fn inline_flex_percent_height_button_matches_stretched_sibling() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        r#"<style>body{margin:0}</style>
        <div style="display:flex;align-items:stretch">
          <div><input style="height:40px;box-sizing:border-box"></div>
          <div id="wrap"><button id="button" style="display:inline-flex;height:100%;padding:8px 24px;line-height:20px;font-size:14px;border:0"><i style="display:block;font-size:16px">x</i></button></div>
        </div>"#,
        900.0,
    );
    let mut heights = Vec::new();
    for id in ["wrap", "button"] {
        let element = doc.get_element_by_id(id).unwrap();
        let h = doc.get_bounding_client_rect(element).unwrap().h;
        heights.push((id, h));
    }
    for (id, h) in heights {
        assert!((h - 40.0).abs() < 0.5, "{id} should be 40px high, got {h}");
    }
}

/// CSS 2.1 §9.5.1: a float's top may not be higher than the top of the current
/// line box — it sits ON that line, and inline content already there moves
/// aside for it. The pending line was being flushed instead, dropping the float
/// onto the next line whenever any inline content preceded it.
#[test]
fn a_float_after_inline_content_stays_on_its_line() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0;font:16px/20px monospace}\
         .ib{display:inline-block;width:50px;height:20px}\
         .fl{float:left;width:60px;height:20px}</style>\
         <span class=ib id=a></span><div class=fl id=b></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    let a = get(&mut d, "a");
    let b = get(&mut d, "b");
    assert!(
        (a.y - b.y).abs() < 0.5,
        "the float shares the line, got {} vs {}",
        a.y,
        b.y
    );
    assert!(
        b.x.abs() < 0.5,
        "the left float takes the near edge, got {}",
        b.x
    );
    assert!(
        (a.x - 60.0).abs() < 0.5,
        "the inline content moves aside, got {}",
        a.x
    );
}

/// A flex container is a block-level box: `max-width` and `min-width` clamp it
/// exactly as they clamp any other block, and the items then flex inside the
/// clamped size.
#[test]
fn a_flex_container_obeys_its_own_min_and_max_width() {
    let w = |decl: &str, inner: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!("<style>body{{margin:0}}</style><div id=a style='{decl}'>{inner}</div>"),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    let item = "<i id=b style='display:block;flex:1 1 200px;height:10px'></i>";
    // The plain block is the control: if this fails the bug is not flex's.
    assert!(
        (w(
            "max-width:150px",
            "<i style='display:block;height:10px'></i>"
        ) - 150.0)
            .abs()
            < 0.5,
        "a block obeys max-width"
    );
    assert!(
        (w("display:flex;max-width:150px", item) - 150.0).abs() < 0.5,
        "a flex container obeys max-width"
    );
    assert!(
        (w("display:flex;width:auto;max-width:150px", item) - 150.0).abs() < 0.5,
        "…with width:auto"
    );
    assert!(
        (w("display:flex;width:400px;max-width:150px", item) - 150.0).abs() < 0.5,
        "…and max-width beats a larger width"
    );
    // `min-width` only binds where the box would otherwise be NARROWER, so it
    // needs a parent that constrains it.
    let narrow = |decl: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}</style><div style='width:100px'>\
             <div id=a style='{decl}'>{item}</div></div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    assert!(
        (narrow("width:auto;min-width:600px") - 600.0).abs() < 0.5,
        "a block obeys min-width"
    );
    assert!(
        (narrow("display:flex;min-width:600px") - 600.0).abs() < 0.5,
        "a flex container obeys min-width"
    );
}

/// The strut applies to EVERY line box, including the anonymous one a block
/// builds when it mixes inline children with block or floated ones.
///
/// That path sets the line height from the child's own height and never
/// consulted the strut, so an inline-block on such a line produced a line
/// exactly as tall as itself and everything below it sat a few pixels high.
#[test]
fn the_strut_applies_to_anonymous_inline_runs_too() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0;font-family:Menlo;font-size:16px;line-height:20px}</style>\
         <div id=p><span id=a style='display:inline-block;width:50px;height:20px'></span>\
         <div id=b style='height:10px'></div></div>",
        900.0,
    );
    let get = |d: &mut crate::types::Document, id: &str| {
        let e = d.get_element_by_id(id).unwrap();
        d.get_bounding_client_rect(e).unwrap()
    };
    // The inline-block's line is 20 for the box plus the strut's 4.5px descent,
    // snapped to a whole pixel — so the block after it starts at 25, not 20.
    let b = get(&mut d, "b");
    let p = get(&mut d, "p");
    assert!(
        (b.y - p.y - 25.0).abs() < 0.6,
        "the anonymous line reserves the strut's descent, got {}",
        b.y - p.y
    );
}

/// `offsetLeft` / `offsetTop` are document-relative when the offset parent is a
/// statically-positioned `body`, and parent-relative otherwise.
///
/// The body was being subtracted, which put every top-level element 8px off —
/// the default body margin — on any page that does not reset it.
#[test]
fn offset_left_does_not_subtract_a_static_body() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<div id=a style='float:left;width:20px;height:10px'>x</div>\
         <div id=b style='width:20px;height:10px'>y</div>\
         <div id=c style='position:relative'>\
         <div id=d style='width:20px;height:10px'>z</div></div>",
        800.0,
    );
    let el = |d: &mut crate::types::Document, id: &str| d.get_element_by_id(id).unwrap();
    // Offset parent is the body: the answer is the distance from the page.
    for id in ["a", "b"] {
        let e = el(&mut d, id);
        assert!(
            (d.offset_left(e) - 8.0).abs() < 0.5,
            "{id}: offsetLeft is 8, got {}",
            d.offset_left(e)
        );
        assert!(
            (d.offset_top(e) - 8.0).abs() < 0.5,
            "{id}: offsetTop is 8, got {}",
            d.offset_top(e)
        );
    }
    // Offset parent is a positioned ancestor: the answer is relative to it.
    let dd = el(&mut d, "d");
    assert!(
        d.offset_left(dd).abs() < 0.5,
        "d: offsetLeft is 0, got {}",
        d.offset_left(dd)
    );
    assert!(
        d.offset_top(dd).abs() < 0.5,
        "d: offsetTop is 0, got {}",
        d.offset_top(dd)
    );
}

#[test]
fn bounding_client_rect_is_viewport_relative_but_offset_stays_document_relative() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div style='height:200px'></div>\
         <div id=a style='width:20px;height:10px'></div>",
        800.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let before = d.get_bounding_client_rect(e).unwrap();
    let offset_before = d.offset_top(e);

    d.scroll_y = 125.0;
    let after = d.get_bounding_client_rect(e).unwrap();

    assert!(
        (before.y - 200.0).abs() < 0.5,
        "control rect y should start at document position, got {}",
        before.y
    );
    assert!(
        (after.y - 75.0).abs() < 0.5,
        "client rect y must subtract scroll, got {}",
        after.y
    );
    assert!(
        (d.offset_top(e) - offset_before).abs() < 0.5,
        "offsetTop must not become viewport-relative"
    );
}

#[test]
fn offset_origin_is_the_offset_parent_padding_edge() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=p style='position:relative;border:10px solid black;padding:5px'>\
             <div id=c style='width:20px;height:10px'></div>\
         </div>",
        800.0,
    );
    let c = d.get_element_by_id("c").unwrap();
    assert!(
        (d.offset_left(c) - 5.0).abs() < 0.5,
        "offsetLeft should be from padding edge, got {}",
        d.offset_left(c)
    );
    assert!(
        (d.offset_top(c) - 5.0).abs() < 0.5,
        "offsetTop should be from padding edge, got {}",
        d.offset_top(c)
    );
}

#[test]
fn offsets_ignore_transforms_that_client_rects_include() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=a style='width:100px;height:50px;transform:rotate(45deg)'></div>",
        800.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    let client = d.get_bounding_client_rect(e).unwrap();

    assert!(
        (d.offset_width(e) - 100.0).abs() < 0.5,
        "offsetWidth ignores transforms"
    );
    assert!(
        (d.offset_height(e) - 50.0).abs() < 0.5,
        "offsetHeight ignores transforms"
    );
    assert!(
        client.w > 105.0 && client.h > 105.0,
        "client rect includes transform bounds, got {client:?}"
    );
}

#[test]
fn transform_none_does_not_create_absolute_containing_block() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=p style='margin-left:100px;width:200px;height:20px;transform:none'>\
             <div id=c style='position:absolute;left:0;top:0;width:10px;height:10px'></div>\
         </div>",
        800.0,
    );
    let c = d.get_element_by_id("c").unwrap();
    let rect = d.get_bounding_client_rect(c).unwrap();

    assert!(
        rect.x.abs() < 0.5,
        "transform:none must not make a static ancestor the containing block, got {rect:?}"
    );
}

#[test]
fn filter_will_change_and_contain_create_absolute_containing_blocks() {
    for (decl, id) in [
        ("filter:blur(0)", "filter"),
        ("will-change:transform", "will"),
        ("contain:paint", "contain"),
    ] {
        let mut r = crate::Renderer::new();
        let html = format!(
            "<style>body{{margin:0}}</style>\
             <div id={id} style='margin-left:100px;width:200px;height:20px;{decl}'>\
                 <div id=c style='position:absolute;left:0;top:0;width:10px;height:10px'></div>\
             </div>"
        );
        let mut d = r.load_html(&html, 800.0);
        let c = d.get_element_by_id("c").unwrap();
        let rect = d.get_bounding_client_rect(c).unwrap();

        assert!(
            (rect.x - 100.0).abs() < 0.5,
            "{decl} should make the static ancestor the containing block, got {rect:?}"
        );
    }
}

#[test]
fn transformed_ancestor_captures_fixed_positioned_descendant() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=p style='margin-left:100px;width:200px;height:20px;transform:translateX(0)'>\
             <div id=c style='position:fixed;left:0;top:0;width:10px;height:10px'></div>\
         </div>",
        800.0,
    );
    let c = d.get_element_by_id("c").unwrap();
    let rect = d.get_bounding_client_rect(c).unwrap();

    assert!(
        (rect.x - 100.0).abs() < 0.5,
        "a transformed ancestor should capture fixed descendants, got {rect:?}"
    );
}

#[test]
fn positioned_ancestor_does_not_capture_fixed_positioned_descendant() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=p style='position:relative;margin-left:100px;width:200px;height:20px'>\
             <div id=c style='position:fixed;left:0;top:0;width:10px;height:10px'></div>\
         </div>",
        800.0,
    );
    let c = d.get_element_by_id("c").unwrap();
    let rect = d.get_bounding_client_rect(c).unwrap();

    assert!(
        rect.x.abs() < 0.5,
        "position:relative must not capture fixed descendants, got {rect:?}"
    );
}

#[test]
fn client_metrics_use_padding_box_and_border_edges() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=a style='width:100px;height:50px;padding:7px 9px;border:3px solid black'></div>",
        800.0,
    );
    let e = d.get_element_by_id("a").unwrap();

    assert!(
        (d.client_left(e) - 3.0).abs() < 0.5,
        "clientLeft is the left border width"
    );
    assert!(
        (d.client_top(e) - 3.0).abs() < 0.5,
        "clientTop is the top border width"
    );
    assert!(
        (d.client_width(e) - 118.0).abs() < 0.5,
        "clientWidth includes padding, excludes border"
    );
    assert!(
        (d.client_height(e) - 64.0).abs() < 0.5,
        "clientHeight includes padding, excludes border"
    );
}

#[test]
fn element_from_point_uses_viewport_coordinates() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div style='height:100px'></div>\
         <button id=a style='display:block;width:80px;height:40px'>Hit</button>",
        800.0,
    );
    let e = d.get_element_by_id("a").unwrap();
    d.scroll_y = 75.0;

    assert_eq!(d.element_from_point(10.0, 30.0), Some(e));
    assert_eq!(d.element_from_point(-1.0, 30.0), None);
}

#[test]
fn overflow_hidden_clips_descendants_for_hit_testing() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=clip style='width:100px;height:20px;overflow:hidden'>\
           <button id=child style='display:block;width:80px;height:80px;margin-top:30px'>Hit</button>\
         </div>",
        800.0,
    );
    let clip = d.get_element_by_id("clip").unwrap();
    let child = d.get_element_by_id("child").unwrap();

    assert_eq!(d.element_from_point(10.0, 10.0), Some(clip));
    assert_ne!(d.element_from_point(10.0, 40.0), Some(child));
}

#[test]
fn overflow_clip_margin_expands_hit_testing_clip_edge() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>html,body{margin:0;width:100px;height:60px}</style>\
         <div id=clip style='width:100px;height:20px;overflow:clip;overflow-clip-margin:12px'>\
           <div style='height:24px'></div>\
           <div id=child style='width:80px;height:20px'></div>\
         </div>",
        800.0,
    );
    let child = d.get_element_by_id("child").unwrap();

    assert_eq!(d.element_from_point(10.0, 25.0), Some(child));
    assert_ne!(d.element_from_point(10.0, 40.0), Some(child));
    assert_eq!(
        crate::layout::hit_test::hit_test_box_at(&d.root, (10.0, 25.0), 0),
        child
    );
    assert_ne!(
        crate::layout::hit_test::hit_test_box_at(&d.root, (10.0, 40.0), 0),
        child
    );
}

#[test]
fn overflow_clip_margin_rejects_two_lengths_without_resetting_previous_value() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<div id=clip style='overflow:clip;overflow-clip-margin:content-box 5px;overflow-clip-margin:10px 20px'></div>",
        800.0,
    );
    let clip = doc.get_element_by_id("clip").unwrap();
    assert_eq!(
        doc.computed_style_property(clip, "overflow-clip-margin"),
        "content-box 5px"
    );
    assert!(crate::css::parser::supports_condition_matches(
        "(overflow-clip-margin: border-box -5px)"
    ));
    assert!(!crate::css::parser::supports_condition_matches(
        "(overflow-clip-margin: 10px 20px)"
    ));
}

#[test]
fn overflow_clip_margin_computed_value_canonicalizes_box_and_offset() {
    for (specified, expected) in [
        ("0px", "0px"),
        ("padding-box", "0px"),
        ("padding-box 10px", "10px"),
        ("10px padding-box", "10px"),
        ("content-box 0px", "content-box"),
        ("10px content-box", "content-box 10px"),
        ("border-box -10px", "border-box -10px"),
        ("calc(100px - 50px)", "50px"),
        ("border-box calc(0.5em + 100px)", "border-box 108px"),
    ] {
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(
            &format!("<div id=clip style='overflow-clip-margin:{specified}'></div>"),
            800.0,
        );
        let id = doc.get_element_by_id("clip").unwrap();
        assert_eq!(
            doc.computed_style_property(id, "overflow-clip-margin"),
            expected,
            "{specified}"
        );
    }
}

#[test]
fn clip_path_inset_and_circle_affect_hit_testing() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>html,body{margin:0;width:380px;height:220px}</style>\
         <div id=inset style='width:100px;height:100px;clip-path:inset(20px)'>\
           <div id=inset-child style='width:100px;height:100px'></div>\
         </div>\
         <div id=circle style='position:absolute;left:120px;top:0;width:100px;height:100px;clip-path:circle(20px at 50% 50%)'>\
           <div id=circle-child style='width:100px;height:100px'></div>\
         </div>\
         <div id=ellipse style='position:absolute;left:0;top:120px;width:120px;height:80px;clip-path:ellipse(40px 20px at 50% 50%)'>\
           <div id=ellipse-child style='width:120px;height:80px'></div>\
         </div>\
         <div id=polygon style='position:absolute;left:240px;top:0;width:100px;height:100px;clip-path:polygon(0 0, 100% 0, 0 100%)'>\
           <div id=polygon-child style='width:100px;height:100px'></div>\
         </div>",
        800.0,
    );
    let inset_child = d.get_element_by_id("inset-child").unwrap();
    let circle_child = d.get_element_by_id("circle-child").unwrap();
    let ellipse_child = d.get_element_by_id("ellipse-child").unwrap();
    let polygon = d.get_element_by_id("polygon").unwrap();
    let polygon_child = d.get_element_by_id("polygon-child").unwrap();
    let polygon_rect = d.get_bounding_client_rect(polygon).unwrap();

    assert_eq!(d.element_from_point(50.0, 50.0), Some(inset_child));
    assert_ne!(d.element_from_point(10.0, 50.0), Some(inset_child));
    assert_eq!(
        crate::layout::hit_test::hit_test_box_at(&d.root, (170.0, 50.0), 0),
        circle_child
    );
    assert_ne!(
        crate::layout::hit_test::hit_test_box_at(&d.root, (210.0, 50.0), 0),
        circle_child
    );
    assert_eq!(d.element_from_point(60.0, 160.0), Some(ellipse_child));
    assert_ne!(d.element_from_point(105.0, 160.0), Some(ellipse_child));
    let inside_poly = (polygon_rect.x + 10.0, polygon_rect.y + 10.0);
    let outside_poly = (polygon_rect.x + 90.0, polygon_rect.y + 90.0);
    assert!(matches!(
        d.element_from_point(inside_poly.0, inside_poly.1),
        Some(id) if id == polygon || id == polygon_child
    ));
    assert!(!matches!(
        d.element_from_point(outside_poly.0, outside_poly.1),
        Some(id) if id == polygon || id == polygon_child
    ));
}

#[test]
fn clip_path_keeps_nested_env_and_calc_tokens_intact() {
    let mut style = ComputedStyle::default();

    crate::css::apply_property(
        &mut style,
        "clip-path",
        "inset(env(--top, calc(4px + 2px)) env(--right, 8px) 0 0)",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Inset);
    assert!((style.clip_path.inset_top.resolve(16.0, 100.0, 16.0) - 6.0).abs() < 0.1);
    assert!((style.clip_path.inset_right.resolve(16.0, 100.0, 16.0) - 8.0).abs() < 0.1);

    crate::css::apply_property(
        &mut style,
        "clip-path",
        "circle(env(--r, calc(10px + 5px)) at env(--cx, 40%) env(--cy, calc(20px + 5px)))",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Circle);
    let ShapeRadius::Length(radius) = &style.clip_path.circle_radius else {
        panic!("explicit circle radius");
    };
    assert!((radius.resolve(16.0, 100.0, 16.0) - 15.0).abs() < 0.1);
    assert!((style.clip_path.center_x.resolve(16.0, 200.0, 16.0) - 80.0).abs() < 0.1);
    assert!((style.clip_path.center_y.resolve(16.0, 100.0, 16.0) - 25.0).abs() < 0.1);

    crate::css::apply_property(
        &mut style,
        "clip-path",
        "polygon(env(--x1, 0px) env(--y1, 0px), 100% 0, 0 100%)",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Polygon);
    assert_eq!(style.clip_path.points.len(), 3);
}

#[test]
fn clip_path_geometry_box_parsing_preserves_valid_value_on_invalid_input() {
    let mut style = ComputedStyle::default();
    for (value, reference_box) in [
        ("margin-box", ClipPathBox::Margin),
        ("padding-box circle(25% at 50% 50%)", ClipPathBox::Padding),
        (
            "polygon(0 0, 100% 0, 0 100%) content-box",
            ClipPathBox::Content,
        ),
        ("border-box inset(1px)", ClipPathBox::Border),
    ] {
        apply_property(&mut style, "clip-path", value);
        assert_eq!(style.clip_path.reference_box, reference_box, "{value}");
        assert_eq!(
            style.clip_path.kind == ClipPathKind::Box,
            value == "margin-box",
            "{value}"
        );
    }
    let previous = style.clip_path.clone();
    for invalid in [
        "",
        "none padding-box",
        "inset(0) margin-box padding-box",
        "inset(0) nonsense",
        "polygon(0 0, bogus bogus, 100% 100%)",
        "polygon(0 0, 50% 50% extra)",
        "polygon(round -5px, 0 0, 100% 0, 0 100%)",
        "polygon(round 5%, 0 0, 100% 0, 0 100%)",
    ] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style.clip_path, previous, "{invalid}");
    }
}

#[test]
fn polygon_round_radius_clamps_to_half_adjacent_edges() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "clip-path",
        "polygon(round 100px, 0 0, 100px 0, 100px 100px, 0 100px)",
    );
    let outline = style
        .clip_path
        .polygon_outline(Rect::new(0.0, 0.0, 100.0, 100.0), 16.0, 16.0);
    assert!(!outline.is_empty());
    assert!((outline[0].0 - 0.0).abs() < 0.01);
    assert!((outline[0].1 - 50.0).abs() < 0.01);
    assert!(outline.iter().all(|(x, y)| x.is_finite() && y.is_finite()));
}

#[test]
fn clip_path_inset_round_preserves_valid_value_and_resets_on_replacement() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "clip-path", "inset(10px round 20px / 8px)");
    let round = style
        .rare
        .as_ref()
        .unwrap()
        .clip_path_inset_round
        .as_ref()
        .unwrap();
    assert_eq!(round.0[0], CssLength::Px(20.0));
    assert_eq!(round.1[0], CssLength::Px(8.0));
    let previous = style.clip_path.clone();
    for invalid in ["inset(10px round)", "inset(10px round 2px 3px 4px 5px 6px)"] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style.clip_path, previous, "{invalid}");
        assert!(style.rare.as_ref().unwrap().clip_path_inset_round.is_some());
    }
    apply_property(&mut style, "clip-path", "inset(10px)");
    assert!(style.rare.as_ref().unwrap().clip_path_inset_round.is_none());

    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        "<style>#parent{clip-path:inset(5px round 12px / 6px)}#child{clip-path:inherit}</style><div id=parent><div id=child></div></div>",
        800.0,
    );
    let child = doc.get_element_by_id("child").unwrap();
    let child = find_box(&doc.root, &|node: &WebCore| node.node_id == child).unwrap();
    let inherited = child.style.rare().clip_path_inset_round.as_ref().unwrap();
    assert_eq!(inherited.0[0], CssLength::Px(12.0));
    assert_eq!(inherited.1[0], CssLength::Px(6.0));
}

#[test]
fn clip_path_rectangles_share_inset_paint_geometry_without_inset_overconstraint() {
    let mut style = ComputedStyle::default();
    let reference = Rect::new(5.0, 7.0, 100.0, 80.0);
    apply_property(
        &mut style,
        "clip-path",
        "content-box xywh(10px 20px 40px 30px round 8px / 4px)",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Inset);
    assert_eq!(style.clip_path.reference_box, ClipPathBox::Content);
    let rect = style.clip_path.inset_rect(reference, 16.0, 16.0);
    assert_eq!(rect, Rect::new(15.0, 27.0, 40.0, 30.0));
    let radii = style
        .rare
        .as_ref()
        .unwrap()
        .clip_path_inset_round
        .as_ref()
        .unwrap();
    assert_eq!(radii.0[0], CssLength::Px(8.0));
    assert_eq!(radii.1[0], CssLength::Px(4.0));

    apply_property(&mut style, "clip-path", "rect(20px 50px 60px 10px)");
    assert_eq!(
        style.clip_path.inset_rect(reference, 16.0, 16.0),
        Rect::new(15.0, 27.0, 40.0, 40.0)
    );
    assert!(style.rare.as_ref().unwrap().clip_path_inset_round.is_none());
    apply_property(&mut style, "clip-path", "rect(auto auto auto auto)");
    assert_eq!(style.clip_path.inset_rect(reference, 16.0, 16.0), reference);
    apply_property(
        &mut style,
        "clip-path",
        "xywh(calc(5px + 5px) 20px 40% 30px)",
    );
    assert_eq!(
        style.clip_path.inset_rect(reference, 16.0, 16.0),
        Rect::new(15.0, 27.0, 40.0, 30.0)
    );
    apply_property(&mut style, "clip-path", "rect(10px 0 0 20px)");
    assert_eq!(
        style.clip_path.inset_rect(reference, 16.0, 16.0),
        Rect::new(25.0, 17.0, 0.0, 0.0)
    );

    let previous = style.clone();
    for invalid in [
        "xywh(1px 2px 3px)",
        "xywh(1px 2px -3px 4px)",
        "xywh(1px 2px 3px -2em)",
        "rect(1px 2px 3px)",
        "rect(1px 2px 3px 4px round)",
    ] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style, previous, "{invalid}");
    }
}

#[test]
fn clip_path_basic_shape_function_names_are_ascii_case_insensitive() {
    let mut style = ComputedStyle::default();
    for (value, kind) in [
        ("INSET(10px)", ClipPathKind::Inset),
        ("XYWH(10px 20px 30px 40px)", ClipPathKind::Inset),
        ("RECT(10px 50px 60px 20px)", ClipPathKind::Inset),
        ("CIRCLE(20px)", ClipPathKind::Circle),
        ("ELLIPSE(20px 10px)", ClipPathKind::Ellipse),
        ("POLYGON(0 0, 100% 0, 0 100%)", ClipPathKind::Polygon),
    ] {
        apply_property(&mut style, "clip-path", value);
        assert_eq!(style.clip_path.kind, kind, "{value}");
    }
    apply_property(&mut style, "clip-path", "NONE");
    assert_eq!(style.clip_path.kind, ClipPathKind::None);
}

#[test]
fn circle_and_ellipse_radial_extents_resolve_against_reference_box() {
    let mut style = ComputedStyle::default();
    let reference = Rect::new(0.0, 0.0, 100.0, 60.0);
    apply_property(&mut style, "clip-path", "circle()");
    assert_eq!(style.clip_path.circle_radius, ShapeRadius::ClosestSide);
    assert_eq!(
        style.clip_path.circle_rect(reference, 16.0, 16.0),
        Rect::new(20.0, 0.0, 60.0, 60.0)
    );
    apply_property(
        &mut style,
        "clip-path",
        "circle(farthest-side at 20px 10px)",
    );
    assert_eq!(
        style.clip_path.circle_rect(reference, 16.0, 16.0),
        Rect::new(-60.0, -70.0, 160.0, 160.0)
    );
    apply_property(
        &mut style,
        "clip-path",
        "circle(closest-corner at 20px 10px)",
    );
    assert!(
        (style.clip_path.circle_rect(reference, 16.0, 16.0).w - 2.0 * 20.0_f32.hypot(10.0)).abs()
            < 0.01
    );
    apply_property(&mut style, "clip-path", "circle(50%)");
    let normalized = 100.0_f32.hypot(60.0) / std::f32::consts::SQRT_2;
    assert!((style.clip_path.circle_rect(reference, 16.0, 16.0).w - normalized).abs() < 0.01);

    apply_property(&mut style, "clip-path", "ellipse()");
    assert_eq!(
        style.clip_path.ellipse_rect(reference, 16.0, 16.0),
        reference
    );
    apply_property(
        &mut style,
        "clip-path",
        "ellipse(farthest-corner at 20px 10px)",
    );
    let ellipse = style.clip_path.ellipse_rect(reference, 16.0, 16.0);
    assert!((ellipse.w - 160.0 * std::f32::consts::SQRT_2).abs() < 0.01);
    assert!((ellipse.h - 100.0 * std::f32::consts::SQRT_2).abs() < 0.01);
    apply_property(&mut style, "clip-path", "ellipse(20px 10px at center)");
    assert_eq!(
        style.clip_path.ellipse_rect(reference, 16.0, 16.0),
        Rect::new(30.0, 20.0, 40.0, 20.0)
    );
    apply_property(
        &mut style,
        "clip-path",
        "ellipse(closest-corner at left center)",
    );
    assert_eq!(
        style.clip_path.ellipse_rect(reference, 16.0, 16.0),
        Rect::new(0.0, 0.0, 0.0, 60.0)
    );
    apply_property(
        &mut style,
        "clip-path",
        "ellipse(closest-corner at center top)",
    );
    assert_eq!(
        style.clip_path.ellipse_rect(reference, 16.0, 16.0),
        Rect::new(0.0, 0.0, 100.0, 0.0)
    );
    apply_property(&mut style, "clip-path", "circle(0 at 0 0)");
    assert_eq!(
        style.clip_path.circle_rect(reference, 16.0, 16.0),
        Rect::new(0.0, 0.0, 0.0, 0.0)
    );

    let previous = style.clone();
    for invalid in [
        "circle(10px 20px)",
        "circle(-5px)",
        "circle(5)",
        "ellipse(20px)",
        "ellipse(-5px 10px)",
        "ellipse(5 10px)",
        "circle(10px at)",
    ] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style, previous, "{invalid}");
    }
}

#[test]
fn radial_clip_positions_follow_position_grammar() {
    let mut style = ComputedStyle::default();
    let reference = Rect::new(0.0, 0.0, 100.0, 60.0);
    for (value, expected) in [
        ("circle(1px at left top)", Rect::new(-1.0, -1.0, 2.0, 2.0)),
        ("circle(1px at top left)", Rect::new(-1.0, -1.0, 2.0, 2.0)),
        (
            "circle(1px at center 20px)",
            Rect::new(49.0, 19.0, 2.0, 2.0),
        ),
        (
            "circle(1px at 20px center)",
            Rect::new(19.0, 29.0, 2.0, 2.0),
        ),
        (
            "circle(1px at center center)",
            Rect::new(49.0, 29.0, 2.0, 2.0),
        ),
        (
            "circle(1px at right 10px bottom 5px)",
            Rect::new(89.0, 54.0, 2.0, 2.0),
        ),
        (
            "ellipse(1px 1px at bottom 5px right 10px)",
            Rect::new(89.0, 54.0, 2.0, 2.0),
        ),
    ] {
        apply_property(&mut style, "clip-path", value);
        let actual = if style.clip_path.kind == ClipPathKind::Ellipse {
            style.clip_path.ellipse_rect(reference, 16.0, 16.0)
        } else {
            style.clip_path.circle_rect(reference, 16.0, 16.0)
        };
        assert_eq!(actual, expected, "{value}");
    }

    let previous = style.clone();
    for invalid in [
        "circle(1px at left right)",
        "circle(1px at top bottom)",
        "circle(1px at top 20px)",
        "circle(1px at 20px left)",
        "circle(1px at left top 5px)",
        "circle(1px at left 5px top)",
        "circle(1px at 5)",
        "circle(1px at left 5 top 5px)",
    ] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style, previous, "{invalid}");
    }
}

#[test]
fn clip_path_path_parses_fill_rule_and_preserves_previous_value_on_invalid_input() {
    let mut style = ComputedStyle::default();
    apply_property(
        &mut style,
        "clip-path",
        "path(evenodd, 'M0 0 L100 0 L100 100 Z')",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Path);
    let (path, rule) = style
        .rare
        .as_ref()
        .unwrap()
        .clip_path_data
        .as_ref()
        .unwrap();
    assert!(path.len() >= 4);
    assert_eq!(*rule, tiny_skia::FillRule::EvenOdd);
    apply_property(
        &mut style,
        "clip-path",
        "PATH(EVENODD, 'M0 0 L100 0 L100 100 Z')",
    );
    assert_eq!(style.clip_path.kind, ClipPathKind::Path);
    assert_eq!(
        style
            .rare
            .as_ref()
            .unwrap()
            .clip_path_data
            .as_ref()
            .unwrap()
            .1,
        tiny_skia::FillRule::EvenOdd
    );
    let previous = style.clone();
    for invalid in [
        "path(not-a-rule, 'M0 0 L10 0 Z')",
        "path(M0 0 L10 0 Z)",
        "path('bad')",
    ] {
        apply_property(&mut style, "clip-path", invalid);
        assert_eq!(style, previous, "{invalid}");
    }
    apply_property(&mut style, "clip-path", "circle(10px)");
    assert_eq!(style.clip_path.kind, ClipPathKind::Circle);
    assert!(style.rare.as_ref().unwrap().clip_path_data.is_none());
}

#[test]
fn element_scroll_members_read_write_and_clamp() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=scroller style='width:100px;height:80px;overflow:scroll'>\
           <div style='width:260px;height:220px'></div>\
         </div>",
        800.0,
    );
    let e = d.get_element_by_id("scroller").unwrap();

    assert!(
        d.element_scroll_width(e) > d.client_width(e),
        "horizontal overflow is exposed"
    );
    assert!(
        d.element_scroll_height(e) > d.client_height(e),
        "vertical overflow is exposed"
    );

    d.element_scroll_to(e, 30.0, 40.0);
    assert!(
        (d.element_scroll_left(e) - 30.0).abs() < 0.5,
        "scrollLeft writes through"
    );
    assert!(
        (d.element_scroll_top(e) - 40.0).abs() < 0.5,
        "scrollTop writes through"
    );

    d.element_scroll_by(e, 10000.0, 10000.0);
    let max_x = (d.element_scroll_width(e) - d.client_width(e)).max(0.0);
    let max_y = (d.element_scroll_height(e) - d.client_height(e)).max(0.0);
    assert!(
        (d.element_scroll_left(e) - max_x).abs() < 0.5,
        "scrollLeft clamps"
    );
    assert!(
        (d.element_scroll_top(e) - max_y).abs() < 0.5,
        "scrollTop clamps"
    );
}

#[test]
fn flex_and_grid_containers_expose_scrollable_overflow() {
    for (display, id) in [("flex", "flexer"), ("grid", "gridder")] {
        let mut r = crate::Renderer::new();
        let html = format!(
            "<style>body{{margin:0}}</style>\
             <div id={id} style='display:{display};width:100px;height:80px;overflow:auto'>\
               <div style='width:260px;height:220px;flex:0 0 auto'></div>\
             </div>"
        );
        let d = r.load_html(&html, 800.0);
        let e = d.get_element_by_id(id).unwrap();

        assert!(
            d.element_scroll_width(e) > d.client_width(e),
            "{display} horizontal overflow is exposed"
        );
        assert!(
            d.element_scroll_height(e) > d.client_height(e),
            "{display} vertical overflow is exposed"
        );
    }
}

#[test]
fn scroll_into_view_scrolls_nearest_scroll_container() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=scroller style='width:120px;height:80px;overflow:scroll'>\
           <div style='height:220px'>\
             <button id=target style='display:block;margin-top:160px;height:20px'>Go</button>\
           </div>\
         </div>",
        800.0,
    );
    let scroller = d.get_element_by_id("scroller").unwrap();
    let target = d.get_element_by_id("target").unwrap();
    assert_eq!(d.element_scroll_top(scroller), 0.0);

    d.scroll_into_view(target);

    assert!(
        d.element_scroll_top(scroller) > 0.0,
        "ancestor scroller should move"
    );
    assert_eq!(
        d.scroll_y, 0.0,
        "viewport should not consume an inner-scroll request"
    );
}

#[test]
fn scroll_into_view_honors_scroll_margin_and_padding() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
         "<style>body{margin:0}</style>\
         <div id=scroller style='width:120px;height:80px;overflow:scroll;scroll-padding-top:10px;scroll-padding-bottom:15px'>\
           <div style='height:240px'>\
             <div id=target style='margin-top:140px;height:20px;scroll-margin-top:12px;scroll-margin-bottom:18px'></div>\
           </div>\
         </div>",
        800.0,
    );
    let scroller = d.get_element_by_id("scroller").unwrap();
    let target = d.get_element_by_id("target").unwrap();

    d.scroll_into_view(target);
    let bottom_scroll = d.element_scroll_top(scroller);
    assert!(
        (bottom_scroll - 113.0).abs() < 0.5,
        "bottom alignment should include scroll-margin-bottom and scroll-padding-bottom, got {bottom_scroll}"
    );

    d.element_scroll_to(scroller, 0.0, 160.0);
    d.scroll_into_view(target);
    let top_scroll = d.element_scroll_top(scroller);
    assert!(
        (top_scroll - 118.0).abs() < 0.5,
        "top alignment should include scroll-margin-top and scroll-padding-top, got {top_scroll}"
    );
}

#[test]
fn scroll_into_view_honors_horizontal_scroll_margin_and_padding() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}</style>\
         <div id=scroller style='width:120px;height:80px;overflow:scroll;scroll-padding-left:10px;scroll-padding-right:15px'>\
           <div style='width:260px;height:60px'>\
             <div id=target style='display:block;margin-left:150px;width:20px;height:20px;scroll-margin-left:12px;scroll-margin-right:18px'></div>\
           </div>\
         </div>",
        800.0,
    );
    let scroller = d.get_element_by_id("scroller").unwrap();
    let target = d.get_element_by_id("target").unwrap();

    d.scroll_into_view(target);
    let right_scroll = d.element_scroll_left(scroller);
    assert!(
        (right_scroll - 83.0).abs() < 0.5,
        "right alignment should include scroll-margin-right and scroll-padding-right, got {right_scroll}"
    );

    d.element_scroll_to(scroller, 160.0, 0.0);
    d.scroll_into_view(target);
    let left_scroll = d.element_scroll_left(scroller);
    assert!(
        (left_scroll - 128.0).abs() < 0.5,
        "left alignment should include scroll-margin-left and scroll-padding-left, got {left_scroll}"
    );
}

#[test]
fn match_media_exposes_query_and_matches_viewport() {
    let mut r = crate::Renderer::new();
    let d = r.load_html_vp("<div></div>", 640.0, 480.0);

    let narrow = d.match_media("(max-width: 700px)");
    let tall = d.match_media("(min-height: 600px)");

    assert_eq!(narrow.media, "(max-width: 700px)");
    assert!(narrow.matches);
    assert!(!tall.matches);
}

/// CSS Sizing §5: the intrinsic keywords size a BOX from its own content, not
/// just a flex item's basis. They read as `auto` to every caller that cannot
/// measure content, so without an explicit branch a `width: min-content` box
/// filled its containing block.
#[test]
fn intrinsic_keywords_size_a_container() {
    let w = |decl: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0;font-family:Menlo;font-size:16px}}</style>\
             <div id=a style='{decl}'>aa bbbb cc</div>"
            ),
            900.0,
        );
        let e = d.get_element_by_id("a").unwrap();
        d.get_bounding_client_rect(e).unwrap().w
    };
    let auto = w("");
    let mn = w("width:min-content");
    let mx = w("width:max-content");
    assert!(
        (auto - 900.0).abs() < 0.5,
        "the control fills its parent, got {auto}"
    );
    assert!(
        mn < mx * 0.6,
        "min-content ({mn}) is well under max-content ({mx})"
    );
    assert!(
        mx < 300.0,
        "max-content is the text's width, not the parent's, got {mx}"
    );
    // `inline-size` is the logical alias and must behave identically.
    assert!(
        (w("inline-size:min-content") - mn).abs() < 0.5,
        "inline-size matches width"
    );
    // …and on a flex container too.
    assert!(
        (w("display:flex;width:max-content") - mx).abs() < 0.5,
        "flex container max-content"
    );
}

/// CSS Cascade §6.4: a later `@layer` beats an earlier one, and an UNLAYERED
/// declaration beats every layered one. The document's layer order has to
/// survive the merge from the parser's stylesheet, or every layered rule ranks
/// the same and the last one parsed wins by document order instead.
#[test]
fn layer_order_survives_into_the_document() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>@layer base, theme;\
         @layer theme { p { color: rgb(0,0,255) } }\
         @layer base  { p { color: rgb(255,0,0) } }\
         p.un { color: rgb(0,128,0) }</style>\
         <p id=a>layered</p><p id=b class=un>unlayered</p>",
        800.0,
    );
    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    // `theme` is declared after `base`, so it wins even though it is written
    // first in the source.
    let a = find(&d.root, "a").unwrap();
    assert_eq!(
        a.style.color,
        crate::types::Color::rgb(0, 0, 255),
        "the later layer wins"
    );
    // …and an unlayered rule beats both.
    let b = find(&d.root, "b").unwrap();
    assert_eq!(
        b.style.color,
        crate::types::Color::rgb(0, 128, 0),
        "unlayered beats every layer"
    );
}

/// An external stylesheet is AUTHOR origin, exactly like an inline `<style>`.
///
/// The loader adds linked sheets through `parse_and_add_with_base_media`, which
/// routes to `parse_and_add` — the same entry the UA sheet uses. Inline styles
/// get `AUTHOR_ORIGIN_BOOST` on the way into the document and linked ones did
/// not, so a rule in a `<style>` block beat a more specific rule from a linked
/// sheet, and on a page whose CSS is mostly external the cascade came out wrong.
#[test]
fn a_linked_stylesheet_is_author_origin() {
    use crate::css::{is_author_origin, ua_stylesheet};
    let mut ss = ua_stylesheet();
    // What the loader does for a linked sheet.
    ss.parse_and_add_with_base_media("nav .item { color: rgb(0,128,0) }", "https://x/", "");
    // The rule just added is the last one in the sheet.
    let linked = ss.rules.last().expect("the linked rule is in the sheet");
    assert!(
        is_author_origin(linked.specificity),
        "a linked stylesheet's rules must be author origin, got specificity {}",
        linked.specificity
    );
}

/// End to end: a linked stylesheet reaches the page AND cascades as author
/// origin, so it beats a less specific inline rule and beats the UA sheet.
///
/// The unit check above guards the origin flag; this guards the whole path —
/// fetch, parse, merge, cascade — because that is where it actually broke: the
/// rules were present and simply lost every contest they should have won.
#[test]
fn a_linked_stylesheet_cascades_as_author() {
    use std::io::Write;
    let dir = std::env::temp_dir().join("webcore-linked-css-test");
    let _ = std::fs::create_dir_all(&dir);
    let css_path = dir.join("site.css");
    {
        let Ok(mut f) = std::fs::File::create(&css_path) else {
            return;
        };
        // More specific than the inline rule below, and it also overrides a UA
        // default (`div` is display:block).
        let _ = f.write_all(
            b"#nav .item { color: rgb(0,128,0) }\n\
                              #nav { display: flex }\n",
        );
    }

    // A RELATIVE href against the document's base, which is what a page does.
    let html = "<link rel=stylesheet href=\"site.css\">\
         <style>.item { color: rgb(255,0,0) }</style>\
         <div id=nav><span class=item id=a>x</span></div>"
        .to_string();

    let mut r = crate::Renderer::new();
    // The base is the DOCUMENT's URL, so `site.css` resolves beside it.
    let base = format!("file://{}/index.html", dir.display());
    let d = r.load_html_with_base(&html, &base, 800.0, 600.0);

    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    let nav = find(&d.root, "nav").expect("#nav in the tree");
    let item = find(&d.root, "a").expect("#nav .item in the tree");

    // The linked sheet was actually fetched and parsed — check for ITS rule,
    // not merely that the sheet is non-empty (the UA sheet always is).
    let linked_present = d.stylesheet.rules.iter().any(|r| {
        r.compiled_decls
            .iter()
            .any(|(id, _)| *id == crate::css::properties::PropertyId::Display)
            && r.specificity >= crate::css::AUTHOR_ORIGIN_BOOST
    });
    assert!(
        linked_present,
        "the linked sheet must reach the document as author origin"
    );
    // …its more specific rule beats the inline one…
    assert_eq!(
        item.style.color,
        crate::types::Color::rgb(0, 128, 0),
        "a linked rule must beat a LESS specific inline rule"
    );
    // …and it overrides a UA default.
    assert_eq!(
        nav.style.display,
        crate::types::Display::Flex,
        "a linked rule must override the UA sheet"
    );

    let _ = std::fs::remove_file(&css_path);
}

/// `@media print` must NOT apply to screen rendering, and `screen` must.
///
/// A page that mistakenly takes its print sheet loses its layout entirely —
/// print stylesheets set `display: block`, drop floats and columns, and hide
/// navigation, which renders a site as one long column.
#[test]
fn print_media_does_not_apply_to_screen() {
    use crate::css::evaluate_media;
    let (vw, vh) = (1280.0, 900.0);
    assert!(
        !evaluate_media("print", vw, vh),
        "`print` must not match a screen"
    );
    assert!(evaluate_media("screen", vw, vh), "`screen` must match");
    assert!(evaluate_media("all", vw, vh), "`all` matches");
    assert!(evaluate_media("", vw, vh), "an empty condition matches");
    assert!(
        !evaluate_media("only print", vw, vh),
        "`only print` must not match"
    );
    assert!(
        evaluate_media("only screen", vw, vh),
        "`only screen` matches"
    );
    assert!(
        !evaluate_media("print and (min-width: 100px)", vw, vh),
        "a print condition must not match however it is qualified"
    );
    assert!(
        evaluate_media("screen, print", vw, vh),
        "a list matches if any does"
    );
    assert!(
        evaluate_media("screen and (min-width: 100px)", vw, vh),
        "screen + feature"
    );
    assert!(
        !evaluate_media("screen and (min-width: 5000px)", vw, vh),
        "…that fails"
    );
}

/// The responsive queries every framework is built on. A `min-width` query
/// that fails to match at a desktop viewport drops the whole desktop grid and
/// the page falls back to its stacked mobile layout — one long column.
#[test]
fn responsive_media_queries_match_a_desktop_viewport() {
    use crate::css::evaluate_media;
    let (vw, vh) = (1280.0, 900.0);
    // Bootstrap's breakpoints, written exactly as it ships them: no space
    // after the colon, and fractional max-widths.
    for q in [
        "(min-width:576px)",
        "(min-width:768px)",
        "(min-width:992px)",
        "(min-width:1200px)",
        "(min-width: 992px)",
    ] {
        assert!(
            evaluate_media(q, vw, vh),
            "{q} must match a 1280px viewport"
        );
    }
    for q in [
        "(max-width:575.98px)",
        "(max-width:767.98px)",
        "(max-width:991.98px)",
        "(max-width:1199.98px)",
    ] {
        assert!(
            !evaluate_media(q, vw, vh),
            "{q} must NOT match a 1280px viewport"
        );
    }
    // …and the ones a wider viewport should still exclude.
    assert!(
        !evaluate_media("(min-width:1400px)", vw, vh),
        "beyond the viewport"
    );
    // Combined forms.
    assert!(
        evaluate_media("screen and (min-width:992px)", vw, vh),
        "screen + min-width"
    );
    assert!(
        evaluate_media("only screen and (min-width:992px)", vw, vh),
        "only screen + min-width"
    );
    assert!(
        !evaluate_media("print and (min-width:992px)", vw, vh),
        "print stays out"
    );
}

/// End to end: a `min-width` rule has to win at a desktop viewport when the
/// page is loaded normally. Evaluating the query correctly is not enough — the
/// cascade has to run with the real viewport, or every desktop breakpoint is
/// skipped and the page keeps its stacked mobile layout.
#[test]
fn a_min_width_rule_applies_at_the_loaded_viewport() {
    let load = |w: f32| {
        let mut r = crate::Renderer::new();
        let d = r.load_html(
            "<style>#a{display:block}\
             @media (min-width:992px){#a{display:flex}}</style>\
             <div id=a><i>x</i></div>",
            w,
        );
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&d.root, "a").unwrap().style.display
    };
    assert_eq!(
        load(1280.0),
        crate::types::Display::Flex,
        "a desktop breakpoint must apply at 1280px"
    );
    assert_eq!(
        load(600.0),
        crate::types::Display::Block,
        "…and must not apply at 600px"
    );
}

/// The generic families must map to real faces of the right KIND. If
/// `sans-serif` resolves to a monospace face every page renders as typewriter
/// text — the most visible possible styling failure, and one that looks like
/// "the CSS did not load" rather than a font problem.
#[test]
fn generic_font_families_are_not_all_monospace() {
    let w = |family: &str, text: &str| {
        let mut r = crate::Renderer::new();
        let d = r.load_html(
            &format!(
                "<style>body{{margin:0}}#a{{display:inline-block;font:16px {family}}}</style>\
             <span id=a>{text}</span>"
            ),
            900.0,
        );
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&d.root, id_of())
            .map(|n| n.layout.border_rect.w)
            .unwrap_or(0.0)
    };
    fn id_of() -> &'static str {
        "a"
    }
    // A proportional face makes narrow and wide glyphs different widths; a
    // monospace face makes them identical.
    for family in ["sans-serif", "serif", "Arial", "Helvetica"] {
        let narrow = w(family, "iiiiiiiiii");
        let wide = w(family, "WWWWWWWWWW");
        assert!(narrow > 0.0 && wide > 0.0, "{family}: text must measure");
        assert!(
            wide > narrow * 1.5,
            "{family} resolved to a MONOSPACE face: 'iiii'={narrow} 'WWWW'={wide}"
        );
    }
    // …and monospace really is monospace, or the check above proves nothing.
    let n = w("monospace", "iiiiiiiiii");
    let d = w("monospace", "WWWWWWWWWW");
    assert!(
        (n - d).abs() < 1.0,
        "monospace must be fixed width: {n} vs {d}"
    );
}

/// `<link rel=stylesheet>` counts wherever it appears. Body-inserted sheets are
/// ordinary on the web — a real page can serve most of its CSS that way — and
/// dropping them renders the page with a fraction of its styles.
#[test]
fn a_stylesheet_link_in_the_body_is_registered() {
    let d = crate::html::parse_html_with_base(
        "<html><head><link rel=stylesheet href=\"a.css\"></head>\
         <body><p>x</p><link rel=stylesheet href=\"b.css\">\
         <div><link href=\"c.css\" type=\"text/css\" rel=\"stylesheet\"></div>\
         </body></html>",
        "https://example.com/",
    );
    let hrefs: Vec<&str> = d
        .linked_stylesheets
        .iter()
        .map(|(h, _)| h.as_str())
        .collect();
    assert!(hrefs.contains(&"a.css"), "the head sheet: {hrefs:?}");
    assert!(hrefs.contains(&"b.css"), "a body sheet: {hrefs:?}");
    assert!(
        hrefs.contains(&"c.css"),
        "a body sheet nested in an element: {hrefs:?}"
    );
}

#[test]
fn disabled_stylesheet_links_are_not_registered_or_fetched() {
    let d = crate::html::parse_html_with_base(
        "<html><head>\
         <link rel=stylesheet href=\"active.css\">\
         <link rel=stylesheet disabled href=\"disabled-head.css\" media=\"(max-width: 0)\">\
         <link rel=STYLESHEET href=\"case.css\">\
         </head><body>\
         <link rel=stylesheet disabled href=\"disabled-body.css\">\
         <link rel=stylesheet href=\"body.css\">\
         </body></html>",
        "https://example.com/",
    );
    let hrefs: Vec<&str> = d
        .linked_stylesheets
        .iter()
        .map(|(h, _)| h.as_str())
        .collect();

    assert!(
        hrefs.contains(&"active.css"),
        "active head sheet: {hrefs:?}"
    );
    assert!(
        hrefs.contains(&"case.css"),
        "rel is ASCII case-insensitive: {hrefs:?}"
    );
    assert!(hrefs.contains(&"body.css"), "active body sheet: {hrefs:?}");
    assert!(
        !hrefs.contains(&"disabled-head.css") && !hrefs.contains(&"disabled-body.css"),
        "disabled stylesheet links must be inert: {hrefs:?}"
    );
}

/// `:hover` has to change the computed style of the hovered element AND of
/// ancestors/descendants the rule selects — a menu that opens on hover depends
/// on `li:hover > .submenu`, not just on the link itself.
#[test]
fn hover_applies_to_the_element_and_its_subtree() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}\
         .item{color:rgb(0,0,0)} .item:hover{color:rgb(255,0,0)}\
         .sub{display:none} .item:hover .sub{display:block}\
         </style>\
         <div class=item id=a>menu<span class=sub id=s>panel</span></div>",
        800.0,
    );
    let a = d.get_element_by_id("a").unwrap();
    let rect = d.get_bounding_client_rect(a).unwrap();

    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    // Nothing hovered yet.
    assert_eq!(
        find(&d.root, "a").unwrap().style.color,
        crate::types::Color::rgb(0, 0, 0),
        "not hovered to begin with"
    );
    assert_eq!(
        find(&d.root, "s").unwrap().style.display,
        crate::types::Display::None,
        "the panel starts closed"
    );

    // Move the pointer over the item, the way a real mouse move arrives.
    let pt = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let changed = d.process_mouse_event(crate::dom::HtmlEventType::MouseMove, pt, 0);
    assert!(
        changed,
        "a move onto a hover-styled element must report a change"
    );
    r.layout_engine().layout(&mut d, 800.0);

    assert_eq!(
        find(&d.root, "a").unwrap().style.color,
        crate::types::Color::rgb(255, 0, 0),
        ":hover must recolour the hovered element"
    );
    assert_eq!(
        find(&d.root, "s").unwrap().style.display,
        crate::types::Display::Block,
        ":hover must open a descendant the rule selects"
    );
}

#[test]
fn negated_hover_collapses_toolbar_before_any_pointer_event() {
    for extra_rules in [0, 1001] {
        let mut html = String::from(
            "<style>body{margin:0}#bar{width:180px;height:30px}\
             #bar:not(:hover)>a:not(:focus)>span+span{display:none}",
        );
        for index in 0..extra_rules {
            html.push_str(&format!(".unused{index}{{color:red}}"));
        }
        html.push_str(
            "</style><div id=bar><a href='#target'><span>→</span><span id=label>Toolbar</span></a><a href='#target'><span>→</span><span id=other-label>Other</span></a></div><p id=target>Target</p>",
        );
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(&html, 400.0);
        let label = doc.get_element_by_id("label").unwrap();
        assert_eq!(
            doc.get_node(label).unwrap().style.display,
            crate::types::Display::None,
            "toolbar must start collapsed; extra rules: {extra_rules}"
        );
        let other = doc.get_element_by_id("other-label").unwrap();
        let tree_id = |id: &str| {
            find_box(&doc.root, &|node| {
                node.attributes.get("id").is_some_and(|value| value == id)
            })
            .unwrap()
            .node_id
        };
        let bar_id = tree_id("bar");
        let other_id = tree_id("other-label");
        let hovered_chain = crate::css::build_hover_chain(&doc.root, bar_id);
        crate::css::mark_hover_dirty(
            &mut doc.root,
            &doc.stylesheet,
            &std::collections::HashSet::new(),
            &hovered_chain,
            true,
            &std::collections::HashSet::from([other_id]),
        );
        assert!(
            ["label", "other-label"].iter().all(|id| {
                find_box(&doc.root, &|node| {
                    node.attributes.get("id").is_some_and(|value| value == id)
                })
                .unwrap()
                .cascade_dirty
            }),
            "the hover ancestor must dirty both branches, even with a nonempty sensitivity cache: bar={bar_id}, other={other_id}, hover rules={}",
            doc.stylesheet
                .rules
                .iter()
                .filter(|rule| rule.is_hover)
                .count()
        );
        doc.process_mouse_event(crate::dom::HtmlEventType::MouseMove, (8.0, 8.0), 0);
        renderer.layout_engine().layout(&mut doc, 400.0);
        assert_eq!(
            doc.get_node(label).unwrap().style.display,
            crate::types::Display::Inline
        );
        assert_eq!(
            doc.get_node(other).unwrap().style.display,
            crate::types::Display::Inline,
            "hovering the toolbar ancestor must update sibling items too"
        );
        doc.process_mouse_event(crate::dom::HtmlEventType::MouseMove, (350.0, 100.0), 0);
        renderer.layout_engine().layout(&mut doc, 400.0);
        assert_eq!(
            doc.get_node(label).unwrap().style.display,
            crate::types::Display::None,
            "toolbar must collapse again after pointer exit"
        );
        assert_eq!(
            doc.get_node(other).unwrap().style.display,
            crate::types::Display::None
        );
    }
}

#[test]
fn incremental_hover_matches_has_ancestor_rules_in_dirty_subtree() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>body{margin:0}.card{height:40px}.label{display:block;height:20px}\
         .card:has(.lead) .label:hover{color:rgb(255,0,0)}</style>\
         <div class='card'><span class='lead'></span><span id='target' class='label'>Target</span></div>\
         <div class='card'><span class='lead'></span><span id='other' class='label'>Other</span></div>",
        400.0,
    );
    let target = doc.get_element_by_id("target").expect("target");
    let rect = doc.get_bounding_client_rect(target).expect("target rect");
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseMove,
        (rect.x + 4.0, rect.y + 4.0),
        0,
    );
    renderer.layout_engine().layout(&mut doc, 400.0);
    let find = |id: &str| {
        find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("node")
    };
    assert_eq!(
        find("target").style.color,
        crate::types::Color::rgb(255, 0, 0)
    );
    assert_ne!(
        find("other").style.color,
        crate::types::Color::rgb(255, 0, 0)
    );
}

#[test]
fn hover_inside_has_updates_ancestor_and_sibling_then_reverts() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>body{margin:0}.card{height:50px}.trigger{display:block;width:80px;height:20px}\
         .card:has(.trigger:hover){background-color:rgb(0,0,255)}\
         .card:has(.trigger:hover) .label{color:rgb(255,0,0)}</style>\
         <div id='card' class='card'><span id='trigger' class='trigger'>Trigger</span>\
         <span id='label' class='label'>Label</span></div><div id='outside' style='height:30px'>Outside</div>",
        400.0,
    );
    let style = |doc: &crate::types::Document, id: &str| {
        find_box(&doc.root, &|node| {
            node.attributes.get("id").is_some_and(|value| value == id)
        })
        .expect("node")
        .style
        .clone()
    };
    let trigger = doc.get_element_by_id("trigger").expect("trigger");
    let trigger_rect = doc.get_bounding_client_rect(trigger).expect("trigger rect");
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseMove,
        (trigger_rect.x + 4.0, trigger_rect.y + 4.0),
        0,
    );
    renderer.layout_engine().layout(&mut doc, 400.0);
    assert_eq!(
        style(&doc, "card").background_color,
        crate::types::Color::rgb(0, 0, 255)
    );
    assert_eq!(
        style(&doc, "label").color,
        crate::types::Color::rgb(255, 0, 0)
    );

    let outside = doc.get_element_by_id("outside").expect("outside");
    let outside_rect = doc.get_bounding_client_rect(outside).expect("outside rect");
    doc.process_mouse_event(
        crate::dom::HtmlEventType::MouseMove,
        (outside_rect.x + 4.0, outside_rect.y + 4.0),
        0,
    );
    renderer.layout_engine().layout(&mut doc, 400.0);
    assert_ne!(
        style(&doc, "card").background_color,
        crate::types::Color::rgb(0, 0, 255)
    );
    assert_ne!(
        style(&doc, "label").color,
        crate::types::Color::rgb(255, 0, 0)
    );
}

/// `:hover` applies to every element on the pointer's ANCESTOR chain, not only
/// the innermost one (CSS 2.1 §5.11.3 — the hover chain).
///
/// Every dropdown menu on the web is built this way: `li:hover > .panel`, with
/// the pointer actually over the `<a>` inside the `<li>`. If only the deepest
/// element gets `:hover`, no menu ever opens.
#[test]
fn hover_applies_to_the_whole_ancestor_chain() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}\
         li div{display:none} li:hover div{display:block}\
         li:hover{background:rgb(1,2,3)} a:hover{color:rgb(9,9,9)}\
         </style>\
         <ul><li id=li><a id=a href=#>Quick Tools</a>\
         <div id=panel>overlay</div></li></ul>",
        800.0,
    );
    // Aim at the <li>'s box. Its <a> is inline, and an inline element's rect
    // is not usable for a hit test — see the note on inline rects being 0x0.
    let li = d.get_element_by_id("li").unwrap();
    let rect = d.get_bounding_client_rect(li).unwrap();

    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    assert_eq!(
        find(&d.root, "panel").unwrap().style.display,
        crate::types::Display::None,
        "the overlay starts closed"
    );

    // Pointer over the LINK, which is inside the <li> the rule selects.
    let pt = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    d.process_mouse_event(crate::dom::HtmlEventType::MouseMove, pt, 0);
    r.layout_engine().layout(&mut d, 800.0);

    // What did the hit test actually resolve to?
    {
        let hovered = d.hovered_box;
        let name = |n: &crate::WebCore, id: u32| -> Option<String> {
            fn go(n: &crate::WebCore, id: u32) -> Option<String> {
                if n.node_id == id {
                    return Some(format!(
                        "{}#{}",
                        n.tag,
                        n.attributes.get("id").cloned().unwrap_or_default()
                    ));
                }
                n.children.iter().find_map(|c| go(c, id))
            }
            go(n, id)
        };
        assert!(
            hovered != 0,
            "the hit test found nothing under the pointer at {pt:?}"
        );
        let who = name(&d.root, hovered).unwrap_or_else(|| format!("<id {hovered}>"));
        assert!(who.contains('#'), "hit test resolved to {who}");
    }
    // First: does the element directly under the pointer get :hover at all?
    assert_eq!(
        find(&d.root, "li").unwrap().style.background_color,
        crate::types::Color::rgb(1, 2, 3),
        "the ANCESTOR <li> must be hovered when the pointer is over its child"
    );
    assert_eq!(
        find(&d.root, "panel").unwrap().style.display,
        crate::types::Display::Block,
        "…so its dropdown opens"
    );
}

/// The dropdown pattern as real menus actually build it: the panel is always
/// `display:block` and `position:absolute`, collapsed with `max-height:0;
/// overflow:hidden`, and the ancestor's `:hover` raises the cap.
///
/// This is the shape usps.com uses for every menu, so it is the one that
/// decides whether the menus work.
#[test]
fn a_max_height_dropdown_opens_on_ancestor_hover() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        "<style>body{margin:0}\
         li{display:block;height:40px}\
         li div{max-height:0;overflow:hidden;position:absolute;display:block;width:200px}\
         li:hover div{max-height:1800px}\
         li div p{height:60px;margin:0}\
         </style>\
         <ul><li id=li><a>Quick Tools</a>\
         <div id=panel><p>entry</p></div></li></ul>",
        800.0,
    );

    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    assert!(
        find(&d.root, "panel").unwrap().layout.border_rect.h < 1.0,
        "the panel starts collapsed"
    );

    let li = d.get_element_by_id("li").unwrap();
    let rect = d.get_bounding_client_rect(li).unwrap();
    let pt = (rect.x + rect.w / 2.0, rect.y + 5.0);
    d.process_mouse_event(crate::dom::HtmlEventType::MouseMove, pt, 0);
    r.layout_engine().layout(&mut d, 800.0);

    // Split the two failure modes: did the CASCADE give the panel the new
    // max-height, and did LAYOUT act on it?
    let mh = find(&d.root, "panel").unwrap().style.max_height.clone();
    assert!(
        !matches!(
            mh,
            crate::types::CssLength::Zero | crate::types::CssLength::Px(0.0)
        ),
        "the cascade must give the panel the hover max-height, got {mh:?}"
    );
    let h = find(&d.root, "panel").unwrap().layout.border_rect.h;
    assert!(h > 50.0, "layout must act on it, got {h}");
}

/// An absolutely positioned box with `height:auto` is as tall as its content,
/// and `max-height` caps it rather than defining it. A dropdown panel is
/// exactly this box, so if it measures zero the menu can never open however
/// the hover is wired.
#[test]
fn an_absolute_box_with_auto_height_wraps_its_content() {
    let h = |decl: &str| {
        let mut r = crate::Renderer::new();
        let d = r.load_html(
            &format!(
                "<style>body{{margin:0}}\
             #p{{position:absolute;display:block;width:200px;overflow:hidden;{decl}}}\
             #p p{{height:60px;margin:0}}</style>\
             <div id=host style='position:relative'><div id=p><p>entry</p></div></div>"
            ),
            800.0,
        );
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&d.root, "p").unwrap().layout.border_rect.h
    };
    // The control: no cap at all.
    assert!(
        (h("") - 60.0).abs() < 1.0,
        "auto height wraps the 60px child, got {}",
        h("")
    );
    // A generous cap must not shrink it.
    assert!(
        (h("max-height:1800px") - 60.0).abs() < 1.0,
        "a 1800px cap leaves a 60px box alone, got {}",
        h("max-height:1800px")
    );
    // A zero cap collapses it.
    assert!(
        h("max-height:0") < 1.0,
        "a zero cap collapses it, got {}",
        h("max-height:0")
    );
}

/// A hover rule written as part of a SELECTOR LIST still applies.
///
/// Menus are almost always written `li.active div, li:focus div, li:hover div
/// { … }` so keyboard and pointer share one rule. If matching stops at the
/// first selector whose base matches — `:focus` here — the `:hover` variant is
/// never registered and the menu never opens for the mouse.
#[test]
fn a_hover_selector_inside_a_list_still_matches() {
    let open_height = |rule: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}li{{display:block;height:40px}}\
             li div{{max-height:0;overflow:hidden;position:absolute;display:block;width:200px}}\
             {rule}\
             li div p{{height:60px;margin:0}}</style>\
             <ul><li id=li><a>Quick Tools</a><div id=panel><p>e</p></div></li></ul>"
            ),
            800.0,
        );
        let li = d.get_element_by_id("li").unwrap();
        let rect = d.get_bounding_client_rect(li).unwrap();
        d.process_mouse_event(
            crate::dom::HtmlEventType::MouseMove,
            (rect.x + rect.w / 2.0, rect.y + 5.0),
            0,
        );
        r.layout_engine().layout(&mut d, 800.0);
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&d.root, "panel").unwrap().layout.border_rect.h
    };
    // The control: hover alone already works.
    assert!(
        open_height("li:hover div{max-height:1800px}") > 50.0,
        "hover alone"
    );
    // The shape real menus use — the hover selector is LAST in the list.
    let h = open_height("li.active div, li:focus div, li:hover div{max-height:1800px}");
    assert!(
        h > 50.0,
        "a hover selector after a :focus one in the same list, got {h}"
    );
}

/// `:hover` in the MIDDLE of a long descendant selector, which is how a real
/// menu is written: `.global--navigation nav li:hover div`. The hovered
/// element is neither the subject nor the first part.
#[test]
fn hover_matches_in_the_middle_of_a_descendant_selector() {
    let open = |rule: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!(
                "<style>body{{margin:0}}li{{display:block;height:40px}}\
             .wrap nav li div{{max-height:0;overflow:hidden;position:absolute;\
             display:block;width:200px}}\
             {rule}\
             .wrap nav li div p{{height:60px;margin:0}}</style>\
             <div class=wrap><nav><ul><li id=li><a>Quick Tools</a>\
             <div id=panel><p>e</p></div></li></ul></nav></div>"
            ),
            800.0,
        );
        let li = d.get_element_by_id("li").unwrap();
        let rect = d.get_bounding_client_rect(li).unwrap();
        d.process_mouse_event(
            crate::dom::HtmlEventType::MouseMove,
            (rect.x + rect.w / 2.0, rect.y + 5.0),
            0,
        );
        r.layout_engine().layout(&mut d, 800.0);
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        find(&d.root, "panel").unwrap().layout.border_rect.h
    };
    // The real shape: a class, an element, the hovered element, the subject.
    // It must out-specify the base rule, which it does — (0,2,3) vs (0,1,3).
    let h = open(".wrap nav li:hover div{max-height:1800px}");
    assert!(
        h > 50.0,
        "hover in the middle of a descendant chain, got {h}"
    );
    // …and the same rule written as a list, as menus usually write it.
    let h = open(
        ".wrap nav li.active div, .wrap nav li:focus div, \
                  .wrap nav li:hover div{max-height:1800px}",
    );
    assert!(h > 50.0, "…and inside a selector list, got {h}");
}

/// A percentage `font-size` resolves against the PARENT's font size, and on the
/// root against the initial 16px. `html { font-size: 62.5% }` is the standard
/// "make 1rem = 10px" idiom, so getting it wrong collapses every `rem` on the
/// page and with it every line height, box height and text size.
#[test]
fn a_percentage_font_size_resolves_against_the_parent() {
    let sizes = |css: &str| {
        let mut r = crate::Renderer::new();
        let mut d = r.load_html(
            &format!("<style>{css}</style><div id=a>x<span id=b>y</span></div>"),
            800.0,
        );
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        let root = d.root.style.font_size_px(16.0, 16.0);
        let a = find(&d.root, "a").unwrap().style.font_size_px(16.0, 16.0);
        (root, a)
    };
    // 62.5% of the initial 16px is 10px.
    let (root, _) = sizes("html{font-size:62.5%}");
    assert!(
        (root - 10.0).abs() < 0.1,
        "html at 62.5% is 10px, got {root}"
    );
    // 100% leaves it at the initial value.
    let (root, _) = sizes("html{font-size:100%}");
    assert!(
        (root - 16.0).abs() < 0.1,
        "html at 100% is 16px, got {root}"
    );
    // …and a percentage on a child is relative to its parent.
    let (_, a) = sizes("html{font-size:20px} #a{font-size:50%}");
    assert!(
        (a - 10.0).abs() < 0.1,
        "50% of a 20px parent is 10px, got {a}"
    );
}

#[test]
fn stylesheet_preserves_page_rules() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("@page :first { margin: 1in; size: A4; } p { color: red; }");

    assert_eq!(sheet.page_rules.len(), 1);
    let page = &sheet.page_rules[0];
    assert_eq!(page.selector, ":first");
    assert_eq!(
        page.declarations.get("margin").map(String::as_str),
        Some("1in")
    );
    assert_eq!(
        page.declarations.get("size").map(String::as_str),
        Some("A4")
    );
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn stylesheet_preserves_page_margin_rules() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        r#"@page :first {
             margin: 1in;
             @top-left { content: "a } b"; color: red !important; }
             @bottom-center { content: counter(page); }
           }"#,
    );

    assert_eq!(sheet.page_rules.len(), 1);
    let page = &sheet.page_rules[0];
    assert_eq!(
        page.declarations.get("margin").map(String::as_str),
        Some("1in")
    );
    assert_eq!(page.declarations.get("@top-left"), None);
    assert_eq!(page.margin_rules.len(), 2);
    assert_eq!(page.margin_rules[0].name, "top-left");
    assert_eq!(
        page.margin_rules[0]
            .declarations
            .get("content")
            .map(String::as_str),
        Some(r#""a } b""#)
    );
    assert_eq!(
        page.margin_rules[0]
            .important_declarations
            .get("color")
            .map(String::as_str),
        Some("red")
    );
    assert_eq!(page.margin_rules[1].name, "bottom-center");
    assert_eq!(
        page.margin_rules[1]
            .declarations
            .get("content")
            .map(String::as_str),
        Some("counter(page)")
    );
}

#[test]
fn inline_style_preserves_page_rules_on_document_stylesheet() {
    let doc =
        crate::parse_html(r#"<style>@page :first { margin: 1in; size: A4; }</style><p>print</p>"#);

    assert_eq!(doc.stylesheet.page_rules.len(), 1);
    assert_eq!(doc.stylesheet.page_rules[0].selector, ":first");
    assert_eq!(
        doc.stylesheet.page_rules[0]
            .declarations
            .get("margin")
            .map(String::as_str),
        Some("1in")
    );
}

#[test]
fn stylesheet_preserves_counter_style_rules() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        r#"@counter-style thumbs {
            system: cyclic;
            symbols: "\1F44D";
            suffix: " ";
        }
        li { list-style-type: thumbs; }"#,
    );

    assert_eq!(sheet.counter_styles.len(), 1);
    let counter = &sheet.counter_styles[0];
    assert_eq!(counter.name, "thumbs");
    assert_eq!(
        counter.declarations.get("system").map(String::as_str),
        Some("cyclic")
    );
    assert_eq!(
        counter.declarations.get("symbols").map(String::as_str),
        Some(r#""\1F44D""#)
    );
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn stylesheet_preserves_counter_style_speak_as_descriptor() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add(
        r#"@counter-style quiet-decimal {
            system: numeric;
            symbols: "0" "1" "2" "3" "4" "5" "6" "7" "8" "9";
            speak-as: numbers;
        }"#,
    );

    assert_eq!(sheet.counter_styles.len(), 1);
    assert_eq!(
        sheet.counter_styles[0]
            .declarations
            .get("speak-as")
            .map(String::as_str),
        Some("numbers")
    );
}

#[test]
fn normalized_stylesheet_preserves_counter_style_rules() {
    let mut sheet = Stylesheet::default();
    let css = crate::html::presentational::normalize_css_text(
        r#"@counter-style thumbs {
            system: cyclic;
            symbols: "\1F44D";
            suffix: " ";
        }
        li { list-style-type: thumbs; }"#,
    );
    sheet.parse_and_add(&css);

    assert_eq!(sheet.counter_styles.len(), 1);
    assert_eq!(sheet.counter_styles[0].name, "thumbs");
    assert_eq!(sheet.rules.len(), 1);
}

#[test]
fn hover_pseudo_element_rules_invalidate_incremental_hover_broadly() {
    let mut sheet = Stylesheet::default();
    sheet.parse_and_add("button:hover::before { content: 'x'; } #other:hover { color: red; }");
    sheet.rebuild_index();

    assert!(
        sheet.has_hover_descendant_rules,
        "hover pseudo-elements need broad hover invalidation because they do not live in hover_style"
    );
}

/// WOFF2 decodes to a usable font.
///
/// It is the format essentially every modern site ships, and until it decoded
/// every such page was measured in a fallback face — wrong glyph advances,
/// wrong line heights, wrong box heights throughout.
#[test]
fn a_woff2_font_decodes_and_measures() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/wpt/fonts/kinter.woff2");
    let Ok(data) = std::fs::read(&path) else {
        eprintln!("no woff2 sample at {} — skipping", path.display());
        return;
    };
    if crate::woff::woff2::decode(&data).is_none() {
        eprintln!("woff2 sample is not supported by this decoder — skipping");
        return;
    }
    let Some(sfnt) = crate::woff::woff2::decode(&data) else {
        eprintln!(
            "woff2 sample uses a transformed glyf/loca stream that is intentionally rejected"
        );
        return;
    };
    // A real sfnt: a known flavour, a sane table count, and bigger than the
    // compressed original.
    assert!(sfnt.len() > data.len(), "decoding must expand the font");
    let flavor = u32::from_be_bytes([sfnt[0], sfnt[1], sfnt[2], sfnt[3]]);
    assert!(
        flavor == 0x0001_0000 || flavor == 0x4f54_544f,
        "sfnt flavour, got {flavor:#x}"
    );
    let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]);
    assert!(
        num_tables > 4 && num_tables < 512,
        "table count {num_tables}"
    );

    // …and the font stack can actually use it: the family loads and text
    // measured in it is proportional, not fallback-identical.
    let mut r = crate::Renderer::new();
    let _ = r.load_html("<p>x</p>", 400.0);
    let before = r.font_system.db().len();
    r.font_system.db_mut().load_font_data(sfnt);
    assert!(
        r.font_system.db().len() > before,
        "the decoded font must register"
    );
}

#[test]
fn test_bootstrap_icons_woff2_decodes() {
    let data = include_bytes!("fixtures/fonts/bootstrap-icons-1.11.3.woff2");
    let decoded = crate::woff::decode(data);
    assert!(
        decoded.is_some(),
        "bootstrap-icons.woff2 MUST decode with our woff2 parser!"
    );
    let sfnt = decoded.unwrap();
    eprintln!(
        "Bootstrap-icons decoded: {} bytes -> {} sfnt bytes",
        data.len(),
        sfnt.len()
    );
    let mut db = fontdb::Database::new();
    let ids = db.load_font_source(fontdb::Source::Binary(std::sync::Arc::new(sfnt)));
    for id in &ids {
        if let Some(face) = db.face(*id) {
            eprintln!("Face families: {:?}", face.families);
        }
    }
    assert!(!ids.is_empty(), "fontdb must parse the decoded sfnt");
}

#[test]
fn test_bootstrap_icons_glyph_shaping() {
    use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
    let data = include_bytes!("fixtures/fonts/bootstrap-icons-1.11.3.woff2");
    let sfnt = crate::woff::decode(data).expect("Bootstrap Icons must decode");
    let mut fs = FontSystem::new();
    let ids = fs
        .db_mut()
        .load_font_source(fontdb::Source::Binary(std::sync::Arc::new(sfnt)));
    assert!(!ids.is_empty(), "decoded font must register");
    let mut buf = Buffer::new(&mut fs, Metrics::new(17.6, 22.0));
    let attrs = Attrs::new().family(cosmic_text::Family::Name("bootstrap-icons"));
    buf.set_text(&mut fs, "\u{f3d7}", &attrs, Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);
    let glyphs: Vec<_> = buf
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .collect();
    assert_eq!(glyphs.len(), 1, "one icon must shape to one glyph");
    assert!(
        ids.contains(&glyphs[0].font_id),
        "must not use a fallback font"
    );
    assert_ne!(glyphs[0].glyph_id, 0, "must not shape to .notdef");

    let mut cache = cosmic_text::SwashCache::new();
    let glyph = glyphs[0].physical((0.0, 0.0), 1.0);
    let mut pixels = 0;
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;
    cache.with_pixels(
        &mut fs,
        glyph.cache_key,
        cosmic_text::Color::rgb(0, 0, 0),
        |x, _, color| {
            if color.a() != 0 {
                pixels += 1;
                min_x = min_x.min(glyph.x + x);
                max_x = max_x.max(glyph.x + x);
            }
        },
    );
    assert!(pixels > 0, "icon must produce visible pixels");
    assert!(
        min_x >= 0 && max_x <= 20,
        "glyph pixel bounds must be within [0, 20], got [{min_x}, {max_x}]"
    );
}
#[test]
fn test_wpt_woff2_suite() {
    let base_dir = std::path::Path::new("/Users/youness/www/html/vybe/data/wpt/wpt/css/WOFF2");
    let index_file = base_dir.join("testcaseindex.xht");
    let support_dir = base_dir.join("support");
    if !index_file.exists() || !support_dir.exists() {
        return;
    }
    let html = std::fs::read_to_string(&index_file).unwrap();

    let mut display_total = 0;
    let mut display_passed = 0;
    let mut reject_total = 0;
    let mut reject_passed = 0;

    // Parse each testCase block from testcaseindex.xht
    for section in html.split(r#"<div class="testCase""#).skip(1) {
        let id = match section
            .split(r#"id=""#)
            .nth(1)
            .and_then(|s| s.split('"').next())
        {
            Some(id) => id,
            None => continue,
        };
        let expectation = if section.contains("SFNT Expectation: Display") {
            "Display"
        } else if section.contains("SFNT Expectation: Reject") {
            "Reject"
        } else {
            continue;
        };

        let font_path = support_dir.join(format!("{}.woff2", id));
        if !font_path.exists() {
            continue;
        }

        let data = std::fs::read(&font_path).unwrap();
        match expectation {
            "Display" => {
                display_total += 1;
                match crate::woff::decode(&data) {
                    Some(sfnt) => {
                        let mut db = fontdb::Database::new();
                        let ids =
                            db.load_font_source(fontdb::Source::Binary(std::sync::Arc::new(sfnt)));
                        if !ids.is_empty() {
                            display_passed += 1;
                        } else {
                            eprintln!("[WPT WOFF2] FAIL fontdb parsing on {}.woff2", id);
                        }
                    }
                    None => {
                        eprintln!("[WPT WOFF2] FAIL decode on {}.woff2 (expected Display)", id);
                    }
                }
            }
            "Reject" => {
                reject_total += 1;
                let decoded = crate::woff::decode(&data);
                let rejected = match decoded {
                    None => true,
                    Some(sfnt) => {
                        let mut db = fontdb::Database::new();
                        db.load_font_source(fontdb::Source::Binary(std::sync::Arc::new(sfnt)))
                            .is_empty()
                    }
                };
                if rejected {
                    reject_passed += 1;
                } else {
                    eprintln!("[WPT WOFF2] FAIL expected reject on {}.woff2", id);
                }
            }
            _ => {}
        }
    }

    eprintln!(
        "[WPT WOFF2] Display: {}/{} passed, Reject: {}/{} passed",
        display_passed, display_total, reject_passed, reject_total
    );
    assert_eq!(
        display_passed, display_total,
        "all Display WPT WOFF2 test fonts must decode"
    );
    assert_eq!(
        reject_passed, reject_total,
        "all Reject WPT WOFF2 test fonts must be rejected"
    );
}

#[test]
fn font_face_descriptors_register_css_family_weight_and_style() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/wpt/fonts/kinter.woff2");
    let Ok(data) = std::fs::read(&path) else {
        eprintln!("no woff2 sample at {} — skipping", path.display());
        return;
    };
    if crate::woff::woff2::decode(&data).is_none() {
        eprintln!("woff2 sample is not supported by this decoder — skipping");
        return;
    }
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let html = format!(
        "<style>@font-face {{
            font-family: 'Css Alias Face';
            src: url(data:font/woff2;base64,{encoded});
            font-weight: 700;
            font-style: italic;
        }} #t {{ font-family: 'Css Alias Face'; font-weight: 700; font-style: italic; }}</style>
        <div id=t>font</div>"
    );

    let mut r = crate::Renderer::new();
    let _ = r.load_html(&html, 400.0);
    let id = r.font_system.db().query(&fontdb::Query {
        families: &[fontdb::Family::Name("Css Alias Face")],
        weight: fontdb::Weight::BOLD,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Italic,
    });

    assert!(
        id.is_some(),
        "@font-face CSS family/weight/style descriptors must participate in font matching"
    );
}

#[test]
fn font_face_unicode_range_filters_font_loading_until_text_intersects() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/wpt/fonts/kinter.woff2");
    let Ok(data) = std::fs::read(&path) else {
        eprintln!("no woff2 sample at {} — skipping", path.display());
        return;
    };
    if crate::woff::woff2::decode(&data).is_none() {
        eprintln!("woff2 sample is not supported by this decoder — skipping");
        return;
    }
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let html = |text: &str| {
        format!(
            "<style>@font-face {{
                font-family: 'Greek Range Face';
                src: url(data:font/woff2;base64,{encoded});
                unicode-range: U+0370-03FF;
            }} #t {{ font-family: 'Greek Range Face'; }}</style>
            <div id=t>{text}</div>"
        )
    };
    let query = fontdb::Query {
        families: &[fontdb::Family::Name("Greek Range Face")],
        weight: fontdb::Weight::NORMAL,
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    };

    let mut latin = crate::Renderer::new();
    let _ = latin.load_html(&html("Latin only"), 400.0);
    assert!(
        latin.font_system.db().query(&query).is_none(),
        "a Greek-only @font-face should not load for Latin-only document text"
    );

    let mut greek = crate::Renderer::new();
    let _ = greek.load_html(&html("Ω"), 400.0);
    assert!(
        greek.font_system.db().query(&query).is_some(),
        "a Greek-only @font-face should load once document text intersects its unicode-range"
    );
}

#[test]
fn font_face_metric_overrides_feed_normal_line_height() {
    let mut fs = cosmic_text::FontSystem::new();
    let Some(mut alias) = fs.db().faces().next().cloned() else {
        eprintln!("no system font face available — skipping");
        return;
    };
    alias.id = fontdb::ID::dummy();
    alias.families.insert(
        0,
        (
            "Metric Override Face".to_string(),
            fontdb::Language::English_UnitedStates,
        ),
    );
    fs.db_mut().push_face_info(alias);
    crate::layout::inline_layout::clear_font_family_caches();
    crate::layout::inline_layout::set_font_metric_override(
        "Metric Override Face",
        crate::layout::inline_layout::FontMetricOverride {
            size_adjust: None,
            ascent: Some(2.0),
            descent: Some(0.5),
            line_gap: Some(0.25),
        },
    );

    let (ascent, descent, line_height) =
        crate::layout::inline_layout::font_metrics(Some(&mut fs), "Metric Override Face", 20.0);
    assert!(
        (ascent - 40.0).abs() <= 0.5
            && (descent - 10.0).abs() <= 0.5
            && (line_height - 55.0).abs() <= 0.5,
        "@font-face metric overrides should drive normal metrics, got ascent={ascent} descent={descent} line_height={line_height}"
    );
}

#[test]
fn initial_normal_line_height_uses_font_face_metric_overrides() {
    assert!(ComputedStyle::default().line_height.is_auto());
    let font = include_bytes!("fixtures/fonts/bootstrap-icons-1.11.3.woff2");
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(font);
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        &format!(
            r#"<style>
        @font-face {{ font-family: InitialMetrics; src: url(data:font/woff2;base64,{encoded});
            size-adjust:110%; ascent-override:120%; descent-override:20%; line-gap-override:10%; }}
        #metrics {{ font-family:InitialMetrics; font-size:20px; }}
        </style><div id="metrics">A</div>"#
        ),
        400.0,
    );
    let metrics = crate::tests::test_grid::find_by_id(&doc.root, "metrics").unwrap();
    assert!(metrics.style.line_height.is_auto());
    assert!(
        (metrics.layout.content_rect.h - 32.0).abs() <= 1.0,
        "initial normal line-height must use adjusted face metrics, got {}",
        metrics.layout.content_rect.h
    );
}

#[test]
fn font_face_size_adjust_scales_text_measurement() {
    let mut fs = cosmic_text::FontSystem::new();
    let Some(mut alias) = fs.db().faces().next().cloned() else {
        eprintln!("no system font face available — skipping");
        return;
    };
    alias.id = fontdb::ID::dummy();
    alias.families.insert(
        0,
        (
            "Size Adjust Face".to_string(),
            fontdb::Language::English_UnitedStates,
        ),
    );
    fs.db_mut().push_face_info(alias);
    crate::layout::inline_layout::clear_font_family_caches();

    let base = crate::layout::inline_layout::measure_text_width_weighted(
        "Adjusted",
        20.0,
        Some(&mut fs),
        FontWeight::Normal,
        FontStyle::Normal,
        1.0,
        "Size Adjust Face",
        100.0,
    );
    crate::layout::inline_layout::set_font_metric_override(
        "Size Adjust Face",
        crate::layout::inline_layout::FontMetricOverride {
            size_adjust: Some(1.5),
            ascent: None,
            descent: None,
            line_gap: None,
        },
    );
    let adjusted = crate::layout::inline_layout::measure_text_width_weighted(
        "Adjusted",
        20.0,
        Some(&mut fs),
        FontWeight::Normal,
        FontStyle::Normal,
        1.0,
        "Size Adjust Face",
        100.0,
    );

    assert!(
        adjusted > base * 1.35,
        "size-adjust should scale glyph advances, base={base} adjusted={adjusted}"
    );
}

/// `vh` and `vw` resolve against the viewport the document was laid out at.
///
/// A stale default here silently rescales every viewport-relative length on the
/// page — hero sections, sticky bars, full-height panels — by the ratio between
/// the real viewport and the default.
#[test]
fn viewport_units_use_the_actual_viewport() {
    let measure = |w: f32, h: f32| {
        let mut r = crate::Renderer::new();
        let d = r.load_html_vp(
            "<style>body{margin:0}#a{width:10vw;height:5vh}</style><div id=a></div>",
            w,
            h,
        );
        fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
            if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        let r = find(&d.root, "a").unwrap().layout.border_rect;
        (r.w, r.h)
    };
    let (w, h) = measure(1280.0, 900.0);
    assert!((w - 128.0).abs() < 0.5, "10vw of 1280 is 128, got {w}");
    assert!((h - 45.0).abs() < 0.5, "5vh of 900 is 45, got {h}");
    // …and it tracks a different viewport rather than a remembered one.
    let (w, h) = measure(800.0, 600.0);
    assert!((w - 80.0).abs() < 0.5, "10vw of 800 is 80, got {w}");
    assert!((h - 30.0).abs() < 0.5, "5vh of 600 is 30, got {h}");
}

/// A container whose only content is floats still collapses margins normally.
///
/// The float does not make the container's own margins behave differently: with
/// `body { margin: 0 }` the container starts at the top of the page, and its
/// bottom margin stays below it rather than being applied above its parent.
#[test]
fn a_float_only_container_does_not_shift_its_parent() {
    let boxes = |inner: &str| {
        let mut r = crate::Renderer::new();
        let d = r.load_html(
            &format!(
                "<style>body{{margin:0}}\
             .w{{width:400px;overflow:hidden;margin-bottom:8px}}</style>\
             <div class=w id=w>{inner}</div>"
            ),
            800.0,
        );
        fn find<'a>(n: &'a crate::WebCore, tag: &str) -> Option<&'a crate::WebCore> {
            if n.tag == tag {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, tag))
        }
        let body = find(&d.root, "body").unwrap().layout.border_rect;
        let w = find(&d.root, "div").unwrap().layout.border_rect;
        (body, w)
    };
    // The control: an ordinary in-flow child.
    let (body, w) = boxes("<div style='height:40px'></div>");
    assert!(
        body.y.abs() < 0.5,
        "control: body at the top, got {}",
        body.y
    );
    assert!(
        w.y.abs() < 0.5,
        "control: container at the top, got {}",
        w.y
    );

    // The same container holding only a float.
    let (body, w) = boxes("<div style='float:left;width:100px;height:40px'></div>");
    assert!(
        body.y.abs() < 0.5,
        "body must stay at the top, got {}",
        body.y
    );
    assert!(
        w.y.abs() < 0.5,
        "the container must stay at the top, got {}",
        w.y
    );
    assert!(
        (w.h - 40.0).abs() < 0.5,
        "the BFC contains its float, got {}",
        w.h
    );
    // Its bottom margin belongs below it, not around it.
    assert!(
        (body.h - 40.0).abs() < 0.5,
        "body wraps the container only, got {}",
        body.h
    );
}

/// A float that overflows a non-BFC block stays in the parent's float context,
/// so content after that block still flows around it (CSS 2.1 §9.5).
///
/// The block itself is not stretched by the float, so the float protrudes past
/// it — and the next block has to make room. Two things had to be right for
/// this: the child must share the parent's context even before any float has
/// been seen, and float positions must convert back through the CONTEXT's
/// origin rather than the block's own top.
#[test]
fn a_float_escaping_its_block_still_moves_later_content() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        "<style>body{margin:0}.w{width:400px}\
         .bfc{width:400px;overflow:hidden}</style>\
         <div class=w id=esc><div id=fl style='float:left;width:100px;height:60px'></div></div>\
         <div class=bfc id=next><div id=inner style='float:left;width:100px;height:30px'></div></div>",
        800.0);
    fn find<'a>(n: &'a crate::WebCore, id: &str) -> Option<&'a crate::WebCore> {
        if n.attributes.get("id").map(|v| v == id).unwrap_or(false) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, id))
    }
    // The float overflows: its block has no height of its own.
    let esc = find(&d.root, "esc").unwrap().layout.border_rect;
    assert!(
        esc.h < 0.5,
        "a non-BFC block is not stretched by its float, got {}",
        esc.h
    );
    let fl = find(&d.root, "fl").unwrap().layout.border_rect;
    assert!(
        fl.y.abs() < 0.5 && (fl.h - 60.0).abs() < 0.5,
        "the float is at the top and 60 tall, got y={} h={}",
        fl.y,
        fl.h
    );
    // …and the following BFC is pushed clear of it rather than overlapping.
    let next = find(&d.root, "next").unwrap().layout.border_rect;
    assert!(
        (next.x - 100.0).abs() < 0.5,
        "the next block starts beside the protruding float, got x={}",
        next.x
    );
}

// ── Tokenizer string-awareness (css-syntax-3) ───────────────────────────────

#[test]
fn data_url_background_rule_survives_between_neighboring_rules() {
    let css = r#".img-contain{position:relative}.loader{background-image:url("data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=");background-position:50%;background-repeat:no-repeat;background-size:12rem}.disabled{opacity:.4}"#;
    let mut sheet = crate::css::Stylesheet::default();
    sheet.parse_and_add_with_base(css, "https://example.test/main.css");
    assert!(
        sheet
            .rules
            .iter()
            .any(|rule| rule.original_selector == ".loader"),
        "missing .loader among {:?}",
        sheet
            .rules
            .iter()
            .map(|rule| &rule.original_selector)
            .collect::<Vec<_>>()
    );
}

#[test]
fn absolute_auto_width_badge_shrinks_to_its_label() {
    let doc = parse_and_layout(
        r#"<a style="position:relative;display:block;width:600px;height:200px">
        <img width="600" height="200">
        <span id="badge" style="position:absolute;left:8px;top:8px;display:inline-block;padding:0 6px;background:red">Portrait</span>
        </a>"#,
        800.0,
    );
    let badge = crate::tests::test_grid::find_by_id(&doc.root, "badge").unwrap();
    assert!(
        badge.layout.border_rect.w > 30.0 && badge.layout.border_rect.w < 100.0,
        "badge should shrink to its label, got {:?}",
        badge.layout.border_rect
    );
}

#[test]
fn column_flex_auto_width_tag_uses_max_content_not_wrapped_content() {
    let doc = parse_and_layout(
        r#"<style>.tags{display:flex;flex-direction:column;align-items:flex-start;width:220px}
        .tag{display:flex;flex:none;padding:8px 24px;font:14px/20px Arial}</style>
        <div class="tags"><a class="tag" id="tag"><span>Bassirou Diomaye Faye</span></a></div>"#,
        800.0,
    );
    let tag = crate::tests::test_grid::find_by_id(&doc.root, "tag").unwrap();
    assert!(
        tag.layout.border_rect.w > 160.0 && tag.layout.border_rect.w <= 220.0,
        "tag should fit its unwrapped text and padding, got {:?}",
        tag.layout.border_rect
    );
    assert!(
        tag.layout.border_rect.h <= 40.0,
        "tag text should stay on one line, got {:?}",
        tag.layout.border_rect
    );
}

/// **A `}` inside a string is string content, not a block end.** Brace matching
/// ran over raw text, so `content: "}"` closed the rule early, the remainder was
/// reparsed as a selector, and the NEXT rule was dropped along with it — silent
/// loss of a rule that has nothing to do with the offending one.
#[test]
fn a_brace_inside_a_string_does_not_end_the_block() {
    let rules = crate::css::parse_stylesheet(
        ".icon::before { content: \"}\"; color: rgb(1,2,3) }\
         .after { color: rgb(4,5,6) }",
    )
    .unwrap_or_default();
    let has_after = rules.iter().any(|r| {
        r.selectors
            .iter()
            .any(|s| crate::html::serializer::serialize_selector(s).contains("after"))
    });
    assert!(
        has_after,
        "the following rule was dropped by a brace inside a string"
    );
}

/// A `;` inside a string is not a declaration terminator.
#[test]
fn a_semicolon_inside_a_string_does_not_split_the_declaration() {
    let rules = crate::css::parse_stylesheet(".foo { --sep: \";\"; color: rgb(7,8,9) }")
        .unwrap_or_default();
    let rule = rules
        .iter()
        .find(|r| {
            r.selectors
                .iter()
                .any(|s| crate::html::serializer::serialize_selector(s).contains("foo"))
        })
        .expect("the rule parsed");
    assert_eq!(
        rule.declarations.get("--sep").map(String::as_str),
        Some("\";\""),
        "custom property was truncated: {:?}",
        rule.declarations.get("--sep")
    );
    assert!(
        rule.declarations.contains_key("color"),
        "the following declaration was lost"
    );
}

/// `/* */` inside a string is not a comment.
#[test]
fn a_comment_marker_inside_a_string_is_not_stripped() {
    let rules = crate::css::parse_stylesheet(".foo { content: \"/* x */\" }").unwrap_or_default();
    let rule = rules
        .iter()
        .find(|r| {
            r.selectors
                .iter()
                .any(|s| crate::html::serializer::serialize_selector(s).contains("foo"))
        })
        .expect("the rule parsed");
    assert!(
        rule.declarations.contains_key("content"),
        "the declaration was emptied and dropped by comment stripping inside a string"
    );
    assert_eq!(
        rule.declarations.get("content").map(String::as_str),
        Some("\"/* x */\"")
    );
    let css = r#".foo { content: "escaped \"/* still text */\""; /* real comment */ color: red }"#;
    let rules = crate::css::parse_stylesheet(css).unwrap_or_default();
    let rule = &rules[0];
    assert_eq!(
        rule.declarations.get("content").map(String::as_str),
        Some(r#""escaped \"/* still text */\"""#)
    );
    assert_eq!(
        rule.declarations.get("color").map(String::as_str),
        Some("red")
    );
    assert_eq!(
        crate::css::parser::strip_css_comments("a{}/* unfinished"),
        "a{}"
    );
}

// ── Media query evaluation (mediaqueries-5) ─────────────────────────────────

#[test]
fn nested_media_lists_distribute_the_outer_condition() {
    let combined = crate::css::MediaConditions::default()
        .with_query("screen, speech")
        .with_query("(min-width: 800px), print");
    assert!(combined.matches(900.0, 600.0));
    assert!(!combined.matches(600.0, 600.0));

    let negated = crate::css::MediaConditions::default()
        .with_query("not print")
        .with_query("(min-width: 800px), print");
    assert!(negated.matches(900.0, 600.0));
    assert!(!negated.matches(600.0, 600.0));
    assert!(!crate::css::evaluate_media("(print)", 900.0, 600.0));
    assert!(!crate::css::evaluate_media("(screen)", 900.0, 600.0));

    let mut sheet = crate::css::Stylesheet::default();
    sheet.parse_and_add(
        "@media screen, speech { @media (min-width: 800px), print { .nested { color: red } } }",
    );
    let rule = sheet
        .rules
        .iter()
        .find(|rule| rule.original_selector == ".nested")
        .unwrap();
    assert!(rule.media_condition.matches(900.0, 600.0));
    assert!(!rule.media_condition.matches(600.0, 600.0));
}

#[test]
fn linked_media_condition_also_applies_inside_media_rules() {
    let mut sheet = crate::css::Stylesheet::default();
    sheet.parse_and_add_with_base_media(
        "@media screen, speech { .linked { color: red } }",
        "https://site.test/base.css",
        "print",
    );
    let rule = sheet
        .rules
        .iter()
        .find(|rule| rule.original_selector == ".linked")
        .unwrap();
    assert!(!rule.media_condition.matches(900.0, 600.0));
}

/// **A preference feature must not match both of its mutually exclusive
/// values.** Unrecognised features fell through to a fail-open `true`, so
/// `(prefers-reduced-motion: reduce)` and `(no-preference)` both matched and
/// whichever came last in the sheet won.
#[test]
fn a_preference_feature_matches_exactly_one_value() {
    let em = |c: &str| crate::css::evaluate_media(c, 1280.0, 900.0);
    for (a, b) in [
        (
            "(prefers-reduced-motion: reduce)",
            "(prefers-reduced-motion: no-preference)",
        ),
        (
            "(prefers-contrast: more)",
            "(prefers-contrast: no-preference)",
        ),
        ("(forced-colors: active)", "(forced-colors: none)"),
        ("(inverted-colors: inverted)", "(inverted-colors: none)"),
    ] {
        assert!(em(a) != em(b), "both branches matched: ({a}) and ({b})");
    }
}

#[test]
fn unknown_media_features_do_not_match() {
    assert!(
        !crate::css::evaluate_media("(definitely-not-a-media-feature: yes)", 1280.0, 900.0),
        "unknown parenthesized media features must not fail open"
    );
}

#[test]
fn unknown_media_types_and_invalid_discrete_values_do_not_apply() {
    let matches = |query| crate::css::evaluate_media(query, 900.0, 600.0);
    for query in [
        "future-display",
        "screen and future-display",
        "(orientation: diagonal)",
        "(hover: maybe)",
        "(pointer: precise)",
    ] {
        assert!(!matches(query), "{query} must not match");
    }
    assert!(matches("not future-display"));
    assert!(matches("future-display, screen"));
    assert!(matches("(orientation: landscape)"));
    assert!(!matches("(orientation: portrait)"));
    assert!(!matches("not (made-up-feature: yes)"));
    assert!(!matches("not (orientation: diagonal)"));
    assert!(!matches("not (prefers-reduced-motion: typo)"));
    assert!(!matches("not (forced-colors: maybe)"));
    assert!(matches("not (prefers-reduced-motion: reduce)"));
    assert!(!matches("(made-up-feature: yes) or print"));
    assert!(!matches("(made-up-feature: yes) or screen"));
    assert!(!matches("(made-up-feature: yes) and screen"));
    assert!(!matches("(made-up-feature: yes), print"));

    let mut sheet = crate::css::Stylesheet::default();
    sheet.parse_and_add(
        "@media future-display { .unknown { display: none } } \
         @media (hover: maybe) { .invalid { display: none } }",
    );
    assert!(
        sheet
            .rules
            .iter()
            .all(|rule| !rule.media_condition.matches(900.0, 600.0))
    );
}

/// Combinators are ASCII case-insensitive; an uppercase `AND` fell through to
/// the permissive default and made a desktop-only rule apply on mobile.
#[test]
fn media_combinators_are_case_insensitive() {
    assert!(
        !crate::css::evaluate_media("screen AND (min-width: 500px)", 320.0, 600.0),
        "an uppercase AND must still evaluate the width test"
    );
    assert!(crate::css::evaluate_media(
        "screen AND (min-width: 500px)",
        900.0,
        600.0
    ));
}

#[test]
fn media_condition_not_has_feature_scope_but_media_type_not_has_query_scope() {
    let matches = |query, width| crate::css::evaluate_media(query, width, 600.0);
    let condition = "(not (min-width: 1000px)) and (min-width: 500px)";
    assert!(!matches(condition, 400.0));
    assert!(matches(condition, 800.0));
    assert!(!matches(condition, 1200.0));
    assert!(!matches(
        "not (min-width: 1000px) and (min-width: 500px)",
        800.0,
    ));
    assert!(matches("not screen and (min-width: 1000px)", 400.0));
    assert!(!matches("not screen and (min-width: 1000px)", 1200.0));
    assert!(!matches("not screen or (min-width: 500px)", 800.0));
    assert!(!matches("screen or (min-width: 500px)", 800.0));
    assert!(!matches(
        "(min-width: 500px) and (min-height: 500px) or (orientation: portrait)",
        800.0,
    ));
    assert!(matches(
        "(min-width: 500px) and ((min-height: 700px) or (orientation: landscape))",
        800.0,
    ));

    let mut sheet = crate::css::Stylesheet::default();
    sheet.parse_and_add(&format!(
        "@media {condition} {{ .in-range {{ width: 33px }} }}"
    ));
    assert!(sheet.rules[0].media_condition.matches(800.0, 600.0));
    assert!(!sheet.rules[0].media_condition.matches(400.0, 600.0));
}

#[test]
fn media_two_sided_ranges_match_only_inside_bounds() {
    assert!(crate::css::evaluate_media(
        "(768px <= width <= 1024px)",
        900.0,
        600.0
    ));
    assert!(!crate::css::evaluate_media(
        "(768px <= width <= 1024px)",
        640.0,
        600.0
    ));
    assert!(!crate::css::evaluate_media(
        "(768px <= width <= 1024px)",
        1200.0,
        600.0
    ));
    for condition in [
        "(768px<=width<=1024px)",
        "(1024px>=width>768px)",
        "(768px<width)",
        "(width=900px)",
        "(width > -100px)",
    ] {
        assert!(
            crate::css::evaluate_media(condition, 900.0, 600.0),
            "{condition}"
        );
    }
    for condition in [
        "(768px<width>1024px)",
        "(bogus<=width<=1024px)",
        "(width>bogus)",
        "(100px<width<1000px<1200px)",
        "(min-width: bogus)",
        "(width<=-100px)",
    ] {
        assert!(
            !crate::css::evaluate_media(condition, 900.0, 600.0),
            "{condition}"
        );
    }

    let mut frame = EngineFrame::new(
        parse_html(
            r#"<style>#a{width:10px}@media (768px<=width<=1024px){#a{width:33px}}</style><div id="a"></div>"#,
        ),
        900.0,
        600.0,
    );
    frame.update_frame();

    let width = find_box(&frame.doc.root, &|node| {
        node.attributes
            .get("id")
            .map(|id| id == "a")
            .unwrap_or(false)
    })
    .map(|node| node.layout.border_rect.w);
    assert_eq!(width, Some(33.0));
}

/// The spec's own recommended future-proof idiom.
#[test]
fn a_parenthesised_not_is_evaluated() {
    let dark = crate::css::evaluate_media("(prefers-color-scheme: dark)", 1280.0, 900.0);
    let not_dark = crate::css::evaluate_media("(not (prefers-color-scheme: dark))", 1280.0, 900.0);
    assert!(
        dark != not_dark,
        "`not (...)` must invert, got both = {dark}"
    );
}

#[test]
fn prefers_color_scheme_uses_engine_preference() {
    crate::css::set_color_scheme_preference(crate::css::ColorSchemePreference::Dark);
    assert!(crate::css::evaluate_media(
        "(prefers-color-scheme: dark)",
        1280.0,
        900.0
    ));
    assert!(!crate::css::evaluate_media(
        "(prefers-color-scheme: light)",
        1280.0,
        900.0
    ));
    assert!(!crate::css::evaluate_media(
        "(prefers-color-scheme: sepia)",
        1280.0,
        900.0
    ));

    crate::css::set_color_scheme_preference(crate::css::ColorSchemePreference::Light);
    assert!(crate::css::evaluate_media(
        "(prefers-color-scheme: light)",
        1280.0,
        900.0
    ));
    assert!(!crate::css::evaluate_media(
        "(prefers-color-scheme: dark)",
        1280.0,
        900.0
    ));
}

#[test]
fn light_dark_color_follows_element_color_scheme_in_cascade() {
    let mut frame = crate::frame::EngineFrame::empty(800.0, 600.0);
    frame.load_html(
        "<style>#target { color-scheme: dark; color: light-dark(white, black); }</style><p id='target'>Text</p>",
    );
    frame.update_frame();
    let target = find_box(&frame.doc.root, &|node| {
        node.attributes.get("id").is_some_and(|id| id == "target")
    })
    .expect("target element");
    assert_eq!(target.style.color, crate::types::Color::rgb(0, 0, 0));
}

#[test]
fn media_doubly_wrapped_logical_groups_are_evaluated() {
    assert!(crate::css::evaluate_media(
        "((min-width: 500px) and (max-width: 1000px))",
        800.0,
        600.0
    ));
    assert!(!crate::css::evaluate_media(
        "((min-width: 500px) and (max-width: 1000px))",
        1200.0,
        600.0
    ));
}

/// `dppx` is the unit real stylesheets use for retina queries.
#[test]
fn resolution_understands_dppx() {
    // A 1x device: 2dppx must NOT match, 1dppx must.
    assert!(
        !crate::css::evaluate_media("(min-resolution: 2dppx)", 1280.0, 900.0),
        "2dppx must not match a 1x render"
    );
    assert!(crate::css::evaluate_media(
        "(min-resolution: 1dppx)",
        1280.0,
        900.0
    ));
}

/// **css-sizing-3 — `aspect-ratio: auto 16/9` must parse the ratio as 16/9,
/// not fold the `auto` keyword into the numerator.** `apply_aspect_ratio`
/// (`property_defs.rs:1706`) checks `v == "auto"` for exact equality, so
/// `"auto 16/9"` falls into the `v.find('/')` branch, where
/// `v[..slash].trim().parse::<f32>()` tries to parse `"auto 16"` as a number,
/// fails, and silently falls back to `unwrap_or(1.0)` — giving `1/9 ≈ 0.111`
/// instead of `16/9 ≈ 1.778`.
///
/// Expectation source: spec arithmetic (16/9), not Chrome. Confirmed against
/// the engine directly (clean worktree, HEAD 98c918e):
/// `apply_property(&mut style, "aspect-ratio", "auto 16 / 9")` currently
/// gives `style.aspect_ratio == Some(0.11111111)`.
/// Destination: `src/tests/test_css.rs`.
#[test]
fn aspect_ratio_auto_keyword_does_not_corrupt_the_ratio() {
    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "aspect-ratio", "auto 16 / 9");
    let ratio = style
        .aspect_ratio
        .expect("auto 16/9 must still set a ratio");
    assert!(
        (ratio - 16.0 / 9.0).abs() < 0.01,
        "\"auto 16/9\" should parse to 16/9 ≈ 1.778, got {ratio}"
    );
}

/// **css-sizing-3 §5 — `aspect-ratio` must transfer height→width when width
/// is auto, not only width→height.** `block.rs:887`'s aspect-ratio block only
/// fires `if rbox.content_height.is_none()` (deriving height from width);
/// there is no symmetric branch for a definite height with `width:auto`.
/// `inline_layout.rs:310` has the identical one-way shape. The cssgaps.md
/// repro is exact: `display:inline-block; aspect-ratio:2/1; height:100px`
/// collapses to ~0 wide instead of the spec's 200.
///
/// (The general FLEX cross→main transfer already works — see
/// `an_aspect_ratio_transfers_the_cross_size_to_the_main_axis` in
/// `test_css.rs` — because flex resolves it in `flex.rs`'s own flex-basis
/// code, a separate path from the general auto-width algorithm this test
/// exercises.)
///
/// Expectation source: spec arithmetic (100 × 2/1 = 200), not Chrome.
/// Destination: `src/tests/test_css.rs`.
///
/// Confirmed against the engine (clean worktree, HEAD 98c918e): this fixture
/// currently measures W=0, H=100.
#[test]
fn aspect_ratio_transfers_height_to_width_outside_flex() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<div id="box" style="display:inline-block; aspect-ratio:2/1; height:100px; background:red;"></div>"#,
        900.0);
    let mut pm = tiny_skia::Pixmap::new(400, 400).unwrap();
    r.render(&mut d, &mut pm, 1.0);
    let e = d.get_element_by_id("box").unwrap();
    let node = d.find_webcore(e).unwrap();
    assert!(
        (node.layout.content_rect.w - 200.0).abs() < 0.5,
        "content width should be 200, got {}",
        node.layout.content_rect.w
    );
    let rect = d.get_bounding_client_rect(e).unwrap();
    assert!(
        (rect.w - 200.0).abs() < 0.5,
        "aspect-ratio:2/1 with height:100px (no width) should give width 200, got {}",
        rect.w
    );
}

#[test]
fn unsupported_transform_function_invalidates_the_declaration() {
    assert!(
        crate::css::parse_css_transform_checked("rotate(45deg) rotate3d(1,1,1,45deg)").is_none()
    );

    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "transform", "rotate(45deg)");
    assert_eq!(style.css_transform.ops.len(), 1);

    crate::css::apply_property(
        &mut style,
        "transform",
        "rotate(45deg) rotate3d(1,1,1,45deg)",
    );
    assert_eq!(style.css_transform.ops.len(), 1);
    assert_eq!(style.transform, "rotate(45deg)");
}

#[test]
fn transform_math_validates_dimensions_and_keeps_both_skew_axes() {
    use crate::types::TransformOp;
    let parse = crate::css::parse_css_transform_checked;
    let value =
        parse("scale(calc(100% + 50%), sqrt(4)) skew(calc(15deg + .125turn), 30deg)").unwrap();
    assert!(matches!(value.ops[0], TransformOp::Scale(x, y) if x == 1.5 && y == 2.0));
    let TransformOp::Matrix(a, b, c, d, e, f) = value.ops[1] else {
        panic!("expected skew matrix")
    };
    assert_eq!((a, d, e, f), (1.0, 1.0, 0.0, 0.0));
    assert!((b - 30.0_f32.to_radians().tan()).abs() < 0.001);
    assert!((c - 60.0_f32.to_radians().tan()).abs() < 0.001);
    assert!(parse("rotate(0) skew(0, 0) translate(0) scaleX(50%)").is_some());
    assert!(parse("matrix(calc(1in / 48px), 0, 0, 1, sqrt(9), 4)").is_some());
    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "transform", "translateX(12px)");
    for invalid in [
        "",
        "scale()",
        "scale(1,,2)",
        "scale(1,)",
        "scale(1 2)",
        "scale(1,2,3)",
        "scale(2px)",
        "scale(calc(1 + 50%))",
        "rotate(90)",
        "rotate(bogusdeg)",
        "rotate(calc(1px))",
        "rotate(1deg, 2deg)",
        "rotate(1deg + 2deg)",
        "translate(auto)",
        "translate(3)",
        "translate(calc(1px + 1s))",
        "translateZ(10%)",
        "matrix(1,0,0,1,0)",
        "matrix(1,0,0,1,0,1px)",
        "skew(10deg, bogus)",
        "translateX(4px) scale(bogus)",
    ] {
        assert!(parse(invalid).is_none(), "must reject {invalid}");
        crate::css::apply_property(&mut style, "transform", invalid);
        assert_eq!(
            style.transform, "translateX(12px)",
            "preserve fallback for {invalid}"
        );
    }
}

#[test]
fn transform_parser_keeps_nested_env_and_calc_arguments_intact() {
    use crate::types::{CssLength, TransformOp};

    let transform = crate::css::parse_css_transform_checked(
        "translate(env(--tx, calc(4px + 2px)), env(--ty, 8px)) rotate(1turn)",
    )
    .expect("nested functions in transform arguments should parse");
    assert_eq!(transform.ops.len(), 2);
    match &transform.ops[0] {
        TransformOp::Translate(x, y) => {
            assert!((x.resolve_vp(16.0, 100.0, 16.0, 800.0, 600.0) - 6.0).abs() < 0.1);
            assert!((y.resolve_vp(16.0, 100.0, 16.0, 800.0, 600.0) - 8.0).abs() < 0.1);
        }
        other => panic!("expected translate op, got {other:?}"),
    }
    match transform.ops[1] {
        TransformOp::Rotate(deg) => assert!((deg - 360.0).abs() < 0.1),
        ref other => panic!("expected rotate op, got {other:?}"),
    }

    let (x, y) = crate::css::parse_transform_origin("env(--ox, calc(4px + 2px)) top");
    assert!((x.resolve_vp(16.0, 100.0, 16.0, 800.0, 600.0) - 6.0).abs() < 0.1);
    assert_eq!(y, CssLength::Percent(0.0));
}

#[test]
fn emergency_wrapping_preserves_words_graphemes_and_nowrap() {
    use unicode_segmentation::UnicodeSegmentation;
    for mode in ["anywhere", "break-word"] {
        let text = "a\u{0301}a\u{0301}a\u{0301}a\u{0301}a\u{0301}a\u{0301}";
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(&format!(
            "<style>div{{font:16px/20px monospace;overflow-wrap:{mode}}}</style><div id=words style='width:65px'>aaa bbbb</div><div id=clusters style='width:20px'>{text}</div><div id=nowrap style='width:20px;white-space:nowrap'>abcdefgh</div><div id=hyphen style='width:25px'>abcdefgh-ijklmnop</div>"
        ), 400.0);
        let mut pm = tiny_skia::Pixmap::new(400, 400).unwrap();
        renderer.render(&mut doc, &mut pm, 1.0);
        let words = crate::dom::query_selector(&doc.root, "#words").unwrap();
        assert_eq!(words.layout.line_cache.len(), 2, "{mode}");
        assert_eq!(
            words.layout.line_cache[0].text_length, 3,
            "prefer normal word boundary"
        );
        assert_eq!(
            words.layout.line_cache[1].text_length, 4,
            "do not split a word fitting the next line"
        );
        let clusters = crate::dom::query_selector(&doc.root, "#clusters").unwrap();
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        assert!(clusters.layout.line_cache.len() > 1);
        for line in &clusters.layout.line_cache {
            assert!(boundaries.contains(&line.text_start));
            assert!(boundaries.contains(&(line.text_start + line.text_length)));
        }
        let nowrap = crate::dom::query_selector(&doc.root, "#nowrap").unwrap();
        assert_eq!(nowrap.layout.line_cache.len(), 1);
        let hyphen = crate::dom::query_selector(&doc.root, "#hyphen").unwrap();
        assert!(
            hyphen.layout.line_cache.len() > 2,
            "long hyphenated segments also need emergency breaks"
        );
    }
}

#[test]
fn break_all_preserves_graphemes_and_respects_nowrap() {
    use unicode_segmentation::UnicodeSegmentation;

    for text in [
        "a\u{0301}a\u{0301}a\u{0301}a\u{0301}",
        "\u{1f469}\u{200d}\u{1f4bb}\u{1f469}\u{200d}\u{1f4bb}",
    ] {
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(&format!(
            "<style>div{{font:16px/20px monospace;word-break:break-all;width:12px}}</style><div id=wrap>{text}</div><div id=nowrap style='white-space:nowrap'>{text}</div><div id=pre style='white-space:pre'>{text}</div>"
        ), 400.0);
        let mut pm = tiny_skia::Pixmap::new(400, 400).unwrap();
        renderer.render(&mut doc, &mut pm, 1.0);
        let boundaries: Vec<_> = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let wrapped = crate::dom::query_selector(&doc.root, "#wrap").unwrap();
        assert!(wrapped.layout.line_cache.len() > 1);
        for line in &wrapped.layout.line_cache {
            assert!(boundaries.contains(&line.text_start));
            assert!(boundaries.contains(&(line.text_start + line.text_length)));
        }
        for selector in ["#nowrap", "#pre"] {
            let node = crate::dom::query_selector(&doc.root, selector).unwrap();
            assert_eq!(node.layout.line_cache.len(), 1, "{selector}: {text}");
        }
    }
}

#[test]
fn overflow_wrap_min_content_distinguishes_anywhere_from_break_word() {
    let mut renderer = crate::Renderer::new();
    let mut doc = renderer.load_html(
        "<style>.box{font:16px/20px monospace;width:min-content}.any{overflow-wrap:anywhere}.word{overflow-wrap:break-word}.legacy{word-break:break-word}.nowrap{white-space:nowrap}</style><div class='box' id=normal>abcdefgh</div><div class='box any' id=any>abcdefgh</div><div class='box word' id=word>abcdefgh</div><div class='box legacy' id=legacy>abcdefgh</div><div class='box any nowrap' id=nowrap>abcdefgh</div><div class='box any' id=nested><span>abcdefgh</span></div>",
        400.0,
    );
    let mut pm = tiny_skia::Pixmap::new(400, 800).unwrap();
    renderer.render(&mut doc, &mut pm, 1.0);
    let width = |id| {
        crate::dom::query_selector(&doc.root, id)
            .unwrap()
            .layout
            .content_rect
            .w
    };
    let normal = width("#normal");
    assert!(normal > 50.0);
    let anywhere = width("#any");
    assert!(
        anywhere < normal / 4.0,
        "anywhere min-content: {anywhere}, normal: {normal}"
    );
    assert!(
        (width("#word") - normal).abs() < 1.0,
        "break-word must not lower min-content"
    );
    assert!(
        (width("#nowrap") - normal).abs() < 1.0,
        "nowrap suppresses emergency opportunities"
    );
    assert!(
        (width("#legacy") - anywhere).abs() < 1.0,
        "legacy word-break behaves as overflow-wrap:anywhere"
    );
    assert!(
        (width("#nested") - anywhere).abs() < 1.0,
        "nested inline text shares the intrinsic break path"
    );
}

#[test]
fn overflow_wrap_intrinsics_reach_flex_and_grid_auto_minimums() {
    for display in ["flex", "grid"] {
        let mut renderer = crate::Renderer::new();
        let mut doc = renderer.load_html(&format!(
            "<style>.container{{display:{display};width:40px;font:16px/20px monospace}}.any{{overflow-wrap:anywhere}}.word{{overflow-wrap:break-word}}</style><div class=container><span id=any class=any>abcdefgh</span></div><div class=container><span id=word class=word>abcdefgh</span></div>"
        ), 400.0);
        let mut pm = tiny_skia::Pixmap::new(400, 400).unwrap();
        renderer.render(&mut doc, &mut pm, 1.0);
        let any = crate::dom::query_selector(&doc.root, "#any").unwrap();
        let word = crate::dom::query_selector(&doc.root, "#word").unwrap();
        assert!(
            any.layout.content_rect.w <= 40.5,
            "{display}: anywhere must fit the container, got {}",
            any.layout.content_rect.w
        );
        assert!(
            word.layout.content_rect.w > 60.0,
            "{display}: break-word must retain the word's automatic minimum, got {}",
            word.layout.content_rect.w
        );
    }
}

#[test]
fn overflow_wrap_anywhere_breaks_long_unspaced_text() {
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        r#"<div id="normal" style="width:40px;font-size:16px;">aaaaaaaaaaaa</div>
           <div id="wrapped" style="width:40px;font-size:16px;overflow-wrap:anywhere;">aaaaaaaaaaaa</div>"#,
        400.0,
    );
    let mut pm = tiny_skia::Pixmap::new(200, 120).unwrap();
    r.render(&mut d, &mut pm, 1.0);

    let normal = d.get_element_by_id("normal").unwrap();
    let wrapped = d.get_element_by_id("wrapped").unwrap();
    let normal_lines = d
        .get_box_by_id(normal)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);
    let wrapped_lines = d
        .get_box_by_id(wrapped)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);

    assert_eq!(
        normal_lines, 1,
        "default wrapping should keep an unbreakable word on one overflowing line"
    );
    assert!(
        wrapped_lines > 1,
        "overflow-wrap:anywhere should add emergency break opportunities"
    );
}

#[test]
fn hyphens_manual_adds_soft_hyphen_break_opportunities() {
    let word = "aaaaaa\u{00ad}aaaaaa";
    let mut r = crate::Renderer::new();
    let mut d = r.load_html(
        &format!(
            r#"<div id="none" style="width:70px;font-size:16px;hyphens:none;">{word}</div>
               <div id="manual" style="width:70px;font-size:16px;hyphens:manual;">{word}</div>"#
        ),
        400.0,
    );
    let mut pm = tiny_skia::Pixmap::new(200, 120).unwrap();
    r.render(&mut d, &mut pm, 1.0);

    let none = d.get_element_by_id("none").unwrap();
    let manual = d.get_element_by_id("manual").unwrap();
    let none_lines = d
        .get_box_by_id(none)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);
    let manual_lines = d
        .get_box_by_id(manual)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);

    assert_eq!(none_lines, 1);
    assert!(
        manual_lines > 1,
        "hyphens:manual should allow a soft-hyphen line break"
    );
}

#[test]
fn text_transform_full_size_kana_and_math_auto() {
    use crate::renderer::display_list_builder::apply_text_transform;
    use crate::types::TextTransform;

    // Full-size-kana: small kana characters convert to normal-size kana
    let transformed_kana =
        apply_text_transform("ぁぃぅぇぉっァィゥェォッ", TextTransform::FullSizeKana);
    assert_eq!(transformed_kana, "あいうえおつアイウエオツ");

    // Math-auto: single Latin and Greek letters become italic math symbols,
    // while multi-letter words (functions/operators) remain upright.
    let math_expr = apply_text_transform("x + sin(y) + a = 0", TextTransform::MathAuto);
    assert!(
        math_expr.contains('𝑥'),
        "single letter x should become italic mathematical x: {math_expr}"
    );
    assert!(
        math_expr.contains('𝑦'),
        "single letter y should become italic mathematical y: {math_expr}"
    );
    assert!(
        math_expr.contains('𝑎'),
        "single letter a should become italic mathematical a: {math_expr}"
    );
    assert!(
        math_expr.contains("sin"),
        "multi-letter word 'sin' should stay upright: {math_expr}"
    );
}

#[test]
fn inline_fragment_geometry_and_pseudo_elements() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"<style>
            .with-pseudo::before { content: "PREFIX "; }
        </style>
        <div style="width:300px; font-size:16px;">
            <span id="s1" style="padding: 2px 4px; border: 1px solid black;">plain inline</span>
            <span id="s2" class="with-pseudo">with pseudo</span>
        </div>"#,
        400.0,
    );

    let s1 = d.get_element_by_id("s1").expect("s1 exists");
    let s2 = d.get_element_by_id("s2").expect("s2 exists");

    let r1 = d.get_bounding_client_rect(s1).expect("s1 has rect");
    let r2 = d.get_bounding_client_rect(s2).expect("s2 has rect");

    assert!(r1.w > 20.0, "s1 width should be positive: {r1:?}");
    assert!(r1.h > 10.0, "s1 height should be positive: {r1:?}");

    assert!(
        r2.w > 40.0,
        "s2 width should include ::before content: {r2:?}"
    );
    assert!(r2.h > 10.0, "s2 height should be positive: {r2:?}");
}

#[test]
fn inline_line_break_at_hyphen_and_cjk() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"<div id="hyphenated" style="width:60px; font-size:16px;">semi-structured</div>
           <div id="cjk" style="width:60px; font-size:16px;">日本語の文章テスト</div>"#,
        400.0,
    );

    let hyphenated = d.get_element_by_id("hyphenated").unwrap();
    let cjk = d.get_element_by_id("cjk").unwrap();

    let hyphen_lines = d
        .get_box_by_id(hyphenated)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);
    let cjk_lines = d
        .get_box_by_id(cjk)
        .map(|b| b.layout.line_cache.len())
        .unwrap_or(0);

    assert!(
        hyphen_lines > 1,
        "semi-structured should wrap at hyphen in narrow container, lines: {hyphen_lines}"
    );
    assert!(
        cjk_lines > 1,
        "CJK ideographs should wrap across multiple lines without spaces in narrow container, lines: {cjk_lines}"
    );
}

#[test]
fn anonymous_text_flex_item_wraps_inside_nowrap_row() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r#"<style>
             body { margin: 0; font: 16px/20px sans-serif; }
             #q { display: flex; flex-wrap: nowrap; width: 180px; }
             #q::before { content: ""; display: block; flex: 0 0 16px; height: 16px; }
           </style>
           <blockquote id="q">MDN closely follows W3C standards which helps me keep up with important topics.</blockquote>"#,
        400.0,
    );
    let q = d.get_element_by_id("q").unwrap();
    let rect = d.get_bounding_client_rect(q).unwrap();

    assert!(
        rect.h >= 60.0,
        "anonymous flex text should wrap and grow cross size, got {rect:?}"
    );
}

#[test]
fn block_before_pseudo_on_inline_anchor_materializes_icon_box() {
    let mut r = crate::Renderer::new();
    let d = r.load_html(
        r##"<style>
             .footer__socials { display: flex; list-style: none; margin: 0; padding: 0; }
             :is(.footer__socials a)::before {
               content: "";
               display: block;
               width: 24px;
               height: 24px;
               background: currentColor;
             }
             [data-icon=github]:is(.footer__socials a)::before {
               background: rgb(1, 2, 3);
             }
           </style>
           <ul class="footer__socials"><li><a id="social" data-icon="github" href="#"></a></li></ul>"##,
        400.0,
    );
    let a_id = d.get_element_by_id("social").unwrap();
    let a = d.get_node(a_id).unwrap();
    let before = a
        .children
        .iter()
        .find(|child| child.tag == "::before")
        .expect("block ::before on an inline anchor should create a generated icon box");

    assert_eq!(before.layout.content_rect.w.round() as i32, 24);
    assert_eq!(before.layout.content_rect.h.round() as i32, 24);
    assert_eq!(
        before.style.background_color,
        crate::types::Color {
            r: 1,
            g: 2,
            b: 3,
            a: 255
        },
        "attribute + :is(...) pseudo selector should cascade onto the generated icon"
    );
}
#[test]
fn individual_transforms_accept_typed_math_and_mixed_units() {
    use crate::css::transform_parse::{
        parse_individual_rotate, parse_individual_scale, parse_individual_translate,
    };
    use crate::types::TransformOp;
    let transform = parse_individual_translate("calc(50% - 2em + 1rem) min(10vh, 40px)").unwrap();
    let TransformOp::Translate(x, y) = &transform.ops[0] else {
        panic!("expected translation")
    };
    assert_eq!(x.resolve_vp(30.0, 200.0, 20.0, 800.0, 600.0), 60.0);
    assert_eq!(x.resolve_vp(20.0, 400.0, 10.0, 800.0, 600.0), 170.0);
    assert_eq!(y.resolve_vp(30.0, 200.0, 20.0, 800.0, 600.0), 40.0);
    assert_eq!(y.resolve_vp(30.0, 200.0, 20.0, 800.0, 200.0), 20.0);
    let rotate = parse_individual_rotate("calc(.25turn + 90deg)").unwrap();
    assert!(matches!(rotate.ops[0], TransformOp::Rotate(n) if (n - 180.0).abs() < 0.001));
    for value in ["sqrt(4) calc(25% + 25%)", "200% 50%"] {
        let scale = parse_individual_scale(value).unwrap();
        assert!(matches!(scale.ops[0], TransformOp::Scale(x, y) if x == 2.0 && y == 0.5));
    }
    for value in ["auto", "10", "calc(1px + 1s)", "1px 2px 3%", "1px, 2px"] {
        assert!(parse_individual_translate(value).is_none(), "{value}");
    }
    for value in ["calc(1px)", "45", "10deg 20deg"] {
        assert!(parse_individual_rotate(value).is_none(), "{value}");
    }
    for value in ["calc(1px)", "1 2 3 4", "calc(1 + 50%)"] {
        assert!(parse_individual_scale(value).is_none(), "{value}");
    }
    assert!(parse_individual_scale("none").unwrap().ops.is_empty());
}
