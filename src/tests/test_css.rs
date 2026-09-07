// Ported from tests/test_css.cpp

use super::harness::*;
use crate::css::{
    apply_property, parse_declarations, parse_selector, parse_stylesheet, resolve_content_value,
    resolve_counters_in_content, PseudoElement, Stylesheet,
};
use crate::frame::EngineFrame;
use crate::html::parse_html;
use crate::renderer::display_list::PaintCmd;
use crate::renderer::display_list_builder::build_display_list;
use crate::types::*;

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
    let sports = find_box(&doc.root, &|b| b.attributes.get("id").is_some_and(|id| id == "sports"))
        .expect("sports link");
    let more = find_box(&doc.root, &|b| b.attributes.get("id").is_some_and(|id| id == "more"))
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
            && r.media_condition == "(min-width: 1px)"
            && r.declarations
                .get("color")
                .map(|v| v == "red")
                .unwrap_or(false)),
        "nested @media should wrap parent declarations"
    );
    assert!(
        ss.iter().any(|r| r.original_selector == ".card:hover"
            && r.media_condition == "(min-width: 1px)"
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
fn supports_font_format_and_font_tech_conditions() {
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
    assert_eq!(style.background_size_w, CssLength::Vw(10.0));
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

    apply_property(&mut style, "background-image", "Linear-Gradient(red, blue)");
    assert_eq!(style.gradient_type, GradientType::Linear);
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
    assert_eq!(style.line_height, CssLength::Em(1.2));
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
        0,
        1,
        0,
        1,
        1280.0,
        600.0,
        0,
        false,
        &std::collections::HashSet::new(),
        0,
        "",
        &[],
        &[],
        &mut candidates,
    );
    assert!(
        matched
            .matched
            .iter()
            .any(|(_, idx)| doc.stylesheet.rules[*idx]
                .original_selector
                .contains("neo-font-v2-heading-2xl-bold-cond")),
        "font utility did not match; matched selectors: {:?}",
        matched
            .matched
            .iter()
            .map(|(_, idx)| doc.stylesheet.rules[*idx].original_selector.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        crate::css::resolve_var_references(
            "var(--font-v2-heading-2xl-bold-cond)",
            &doc.stylesheet.variables
        ),
        "700 2.5rem/1.15 GT America Condensed"
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
    let d = r.load_html(
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
fn slashdot_icon_font_before_rules_generate_inline_pseudo_content() {
    let doc = parse_and_layout(
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
    assert_eq!(icon.style.before_content, "\u{e87a}");
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
    let doc = parse_and_layout(
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
    assert_eq!(icon.style.before_content, "\u{e87a}");
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
fn positioned_pseudo_on_plain_inline_does_not_paint_from_stale_zero_box() {
    let mut renderer = crate::Renderer::new();
    let doc = renderer.load_html(
        r#"<style>
             body { margin: 0; font: 20px/20px Arial; }
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
    let bad_far_fill = list.commands.iter().any(|cmd| match cmd {
        PaintCmd::FillRect { rect, color, .. } => {
            color.r == 0 && color.g == 0 && color.b == 0 && rect.x >= 90.0 && rect.y >= 90.0
        }
        _ => false,
    });
    assert!(
        !bad_far_fill,
        "plain-inline positioned pseudo decoration should not paint from a stale 0x0 owner box"
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
fn dir_auto_without_strong_text_preserves_inherited_direction() {
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

    assert_eq!(p.style.direction, Direction::RTL);
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
    assert!(line
        .visual_segments
        .iter()
        .all(|segment| segment.length == 1 && segment.level == 1));
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
fn counter_reset_defaults_to_zero_but_increment_defaults_to_one() {
    let mut style = ComputedStyle::default();
    apply_property(&mut style, "counter-reset", "section");
    apply_property(&mut style, "counter-increment", "item");

    assert_eq!(style.counter_reset, vec![("section".to_string(), 0)]);
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
}

#[test]
fn counters_function_joins_nested_counter_scopes() {
    let mut counters = std::collections::HashMap::new();
    counters.insert("section".to_string(), vec![2, 5, 8]);
    let content = resolve_content_value(r#"counters(section, ". ") "#);
    assert_eq!(resolve_counters_in_content(&content, &counters), "2. 5. 8");
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
fn content_on_element_replaces_rendered_inline_text() {
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
        texts.iter().any(|text| text.contains("Generated label")),
        "content on a normal element should paint generated text; painted texts were {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("Original"))
            && !texts.iter().any(|text| text.contains("child")),
        "content on a normal element should replace descendant inline text; painted texts were {texts:?}"
    );
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
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Tag(t) if t == "div")));
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Class(c) if c == "container")));
}

#[test]
fn css_selector_with_id() {
    let sel = parse_selector("#main");
    use crate::css::SelectorPart;
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Id(id) if id == "main")));
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
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Class(c) if c == "md:grid-cols-[2fr_1fr]")));
}

#[test]
fn css_selector_hex_escape_decodes_identifier_code_points() {
    let sel = parse_selector(".\\32xl\\:grid-cols-12");
    use crate::css::SelectorPart;
    assert!(sel.valid);
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Class(c) if c == "2xl:grid-cols-12")));
}

#[test]
fn css_selector_hex_escape_consumes_optional_trailing_space() {
    let sel = parse_selector(".bg-\\5b rgba\\28 0\\2c 0\\2c 0\\2c 0\\.3\\29 \\5d");
    use crate::css::SelectorPart;
    assert!(sel.valid);
    assert!(sel.parts.iter().any(
        |p| matches!(p, SelectorPart::Class(c) if c == "bg-[rgba(0,0,0,0.3)]")
    ));
}

#[test]
fn css_escaped_before_class_is_not_stripped_as_pseudo_element() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(".before\\:block { display: block; }");
    assert_eq!(ss.rules.len(), 1);
    assert_eq!(ss.rules[0].pseudo_element, PseudoElement::None);
    assert!(ss.rules[0].selectors[0]
        .parts
        .iter()
        .any(|p| matches!(p, crate::css::SelectorPart::Class(c) if c == "before:block")));
}

#[test]
fn css_escaped_before_class_can_still_target_real_before_pseudo() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(".before\\:content-\\[\\'\\/\\'\\]::before { content: \"/\"; }");
    assert_eq!(ss.rules.len(), 1);
    assert_eq!(ss.rules[0].pseudo_element, PseudoElement::Before);
    assert!(ss.rules[0].selectors[0]
        .parts
        .iter()
        .any(|p| matches!(p, crate::css::SelectorPart::Class(c) if c == "before:content-['/']")));
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
fn css_selector_descendant_combinator() {
    let sel = parse_selector("div p");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Descendant))));
}

#[test]
fn css_selector_child_combinator() {
    let sel = parse_selector("div > p");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Child))));
}

#[test]
fn css_selector_adjacent_sibling() {
    let sel = parse_selector("h1 + p");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::AdjacentSibling))));
}

#[test]
fn css_selector_general_sibling() {
    let sel = parse_selector("h1 ~ p");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::GeneralSibling))));
}

#[test]
fn column_combinator_does_not_degrade_to_descendant_selector() {
    let sel = parse_selector("col || td");
    use crate::css::{Combinator, SelectorPart};
    assert!(sel.valid);
    assert!(sel
        .parts
        .iter()
        .any(|p| matches!(p, SelectorPart::Combinator(Combinator::Column))));

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
    apply_property(&mut style, "list-style-type", "circle");
    assert_eq!(style.list_style_type, ListStyleType::Circle);
    apply_property(&mut style, "list-style-type", "none");
    assert_eq!(style.list_style_type, ListStyleType::None);
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
    assert!(ss
        .rules
        .iter()
        .any(|r| r.pseudo_element == PseudoElement::Backdrop));
    assert!(ss
        .rules
        .iter()
        .filter(|r| r.pseudo_element != PseudoElement::Backdrop)
        .all(|r| r.pseudo_element == PseudoElement::Ignored));
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
    assert!(style.text_overflow_string.is_empty());
    apply_property(&mut style, "text-overflow", r#"clip "--""#);
    assert_eq!(style.text_overflow, TextOverflow::Ellipsis);
    assert_eq!(style.text_overflow_string, "--");
    apply_property(&mut style, "text-overflow", "clip");
    assert_eq!(style.text_overflow, TextOverflow::Clip);
    assert!(style.text_overflow_string.is_empty());

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
}

// ── Media query evaluation (mediaqueries-5) ─────────────────────────────────

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

    let mut frame = EngineFrame::new(
        parse_html(
            r#"<style>#a{width:10px}@media (768px <= width <= 1024px){#a{width:33px}}</style><div id="a"></div>"#,
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
