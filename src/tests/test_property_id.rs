//! Tests for the PropertyId system — resolution, inheritance, shorthand detection.

use crate::css::properties::{PropertyId, is_inherited, is_shorthand, resolve};

#[test]
fn resolve_basic_properties() {
    assert_eq!(resolve("display"), PropertyId::Display);
    assert_eq!(resolve("position"), PropertyId::Position);
    assert_eq!(resolve("color"), PropertyId::Color);
    assert_eq!(resolve("width"), PropertyId::Width);
    assert_eq!(resolve("height"), PropertyId::Height);
    assert_eq!(resolve("margin-top"), PropertyId::MarginTop);
    assert_eq!(resolve("padding-left"), PropertyId::PaddingLeft);
    assert_eq!(
        resolve("border-bottom-width"),
        PropertyId::BorderBottomWidth
    );
    assert_eq!(resolve("font-size"), PropertyId::FontSize);
    assert_eq!(resolve("z-index"), PropertyId::ZIndex);
    assert_eq!(resolve("flex-direction"), PropertyId::FlexDirection);
    assert_eq!(
        resolve("grid-template-columns"),
        PropertyId::GridTemplateColumns
    );
}

#[test]
fn resolve_shorthands() {
    assert_eq!(resolve("margin"), PropertyId::Margin);
    assert_eq!(resolve("padding"), PropertyId::Padding);
    assert_eq!(resolve("border"), PropertyId::Border);
    assert_eq!(resolve("background"), PropertyId::Background);
    assert_eq!(resolve("font"), PropertyId::Font);
    assert_eq!(resolve("flex"), PropertyId::Flex);
}

#[test]
fn resolve_vendor_prefixes() {
    // Vendor prefixes should map to standard equivalents
    let webkit_sel = resolve("-webkit-user-select");
    let moz_sel = resolve("-moz-user-select");
    assert_eq!(webkit_sel, PropertyId::UserSelect);
    assert_eq!(moz_sel, PropertyId::UserSelect);
}

#[test]
fn resolve_unknown_returns_unknown() {
    assert_eq!(resolve("not-a-real-property"), PropertyId::Unknown);
    assert_eq!(resolve(""), PropertyId::Unknown);
}

#[test]
fn inherited_properties() {
    assert!(is_inherited(PropertyId::Color));
    assert!(is_inherited(PropertyId::FontSize));
    assert!(is_inherited(PropertyId::FontFamily));
    assert!(is_inherited(PropertyId::LineHeight));
    assert!(is_inherited(PropertyId::TextAlign));
    assert!(is_inherited(PropertyId::Visibility));
    assert!(is_inherited(PropertyId::Cursor));
    assert!(is_inherited(PropertyId::LetterSpacing));
    assert!(is_inherited(PropertyId::WordSpacing));
    assert!(is_inherited(PropertyId::WhiteSpace));
    assert!(is_inherited(PropertyId::Direction));
    assert!(is_inherited(PropertyId::ListStyleType));
}

#[test]
fn non_inherited_properties() {
    assert!(!is_inherited(PropertyId::Display));
    assert!(!is_inherited(PropertyId::Width));
    assert!(!is_inherited(PropertyId::Height));
    assert!(!is_inherited(PropertyId::Margin));
    assert!(!is_inherited(PropertyId::Padding));
    assert!(!is_inherited(PropertyId::BackgroundColor));
    assert!(!is_inherited(PropertyId::Position));
    assert!(!is_inherited(PropertyId::FlexDirection));
}

#[test]
fn shorthand_detection() {
    assert!(is_shorthand(PropertyId::Margin));
    assert!(is_shorthand(PropertyId::Padding));
    assert!(is_shorthand(PropertyId::Border));
    assert!(is_shorthand(PropertyId::Background));
    assert!(is_shorthand(PropertyId::Font));
    assert!(is_shorthand(PropertyId::Flex));
    assert!(is_shorthand(PropertyId::Transition));
    assert!(is_shorthand(PropertyId::Animation));

    assert!(!is_shorthand(PropertyId::MarginTop));
    assert!(!is_shorthand(PropertyId::Color));
    assert!(!is_shorthand(PropertyId::Display));
}

#[test]
fn compiled_declarations_populated() {
    let doc = crate::html::parse_html(
        r#"<html><head><style>
        .test { color: red; margin: 10px; display: flex; }
    </style></head><body><div class="test">hi</div></body></html>"#,
    );

    // After parsing, stylesheet rules should have compiled_decls
    let has_compiled = doc
        .stylesheet
        .rules
        .iter()
        .any(|r| !r.compiled_decls.is_empty());
    assert!(
        has_compiled,
        "at least one rule should have compiled declarations"
    );

    // Find our test rule
    for rule in &doc.stylesheet.rules {
        if rule.declarations.get("color").map(|v| v.as_str()) == Some("red") {
            assert!(
                !rule.compiled_decls.is_empty(),
                "rule with color:red should have compiled_decls"
            );
            // Check that PropertyId::Color is in compiled_decls
            let has_color = rule
                .compiled_decls
                .iter()
                .any(|(id, _)| *id == PropertyId::Color);
            assert!(has_color, "compiled_decls should contain Color");
            break;
        }
    }
}
