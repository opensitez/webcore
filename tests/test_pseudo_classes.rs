// Pseudo-class tests – ported from cpptests/test_pseudo_classes.cpp
//
// Scope: only tests that are portable to the Rust layer:
//   1. Parsing: verify pseudo-class selectors are stored correctly in stylesheet rules
//   2. Roundtrip: verify selectors survive parse (original_selector field)
//   3. Specificity: verify CSS selector specificity calculation
//
// Skipped (require widget state: focusedBox, hoveredBox, activeBox, visitedUrls):
//   - FocusMatchesWhenFocused, FocusDoesNotMatchWhenNotFocused
//   - FocusWithinMatchesParent, FocusWithinDoesNotMatchUnrelated
//   - ActiveMatchesWhenPressed, ActiveDoesNotMatchWhenNotPressed
//   - CheckedMatchesWhenAttributeSet, CheckedDoesNotMatchWhenAttributeAbsent
//   - DisabledMatchesWhenAttributeSet, DisabledDoesNotMatchWithoutAttribute
//   - EnabledMatchesWhenNotDisabled, EnabledDoesNotMatchWhenDisabled
//   - TargetMatchesWhenIdIsTargetId, TargetDoesNotMatchWhenIdDiffers
//   - VisitedMatchesWhenUrlInVisitedSet, VisitedDoesNotMatchUnvisitedUrl
//   - LinkMatchesUnvisitedUrl, LinkDoesNotMatchVisitedUrl
//   - HoverMatchesWhenHoveredBoxSet, HoverDoesNotMatchWhenBoxNotHovered
//   - HoverAppliesAllPropertiesViaStateAwareCascade
//   - FocusAppliesOutlineViaStateAwareCascade
use webcore::css::{parse_selector, SelectorPart};
use webcore::parse_html;

// ─── Helper: check that a stylesheet rule has a pseudo-class part with the given name ──

fn rule_has_pseudo_class(html: &str, pseudo: &str) -> bool {
    let doc = parse_html(html);
    doc.stylesheet.rules.iter().any(|rule| {
        rule.selectors.iter().any(|sel| {
            sel.parts
                .iter()
                .any(|part| matches!(part, SelectorPart::PseudoClass(name) if name == pseudo))
        })
    })
}

// ─── Helper: check that original_selector of any rule contains a substring ──

fn rule_original_selector_contains(html: &str, needle: &str) -> bool {
    let doc = parse_html(html);
    doc.stylesheet
        .rules
        .iter()
        .any(|rule| rule.original_selector.contains(needle))
}

// ============================================================
// Pseudo-class Parsing
// ============================================================

#[test]
fn focus_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>input:focus { outline: 2px solid blue; }</style></head>\
         <body><input/></body></html>",
            "focus"
        ),
        ":focus pseudo-class should be stored in selector parts"
    );
}

#[test]
fn active_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>button:active { background-color: darkblue; }</style></head>\
         <body><button>Click</button></body></html>",
            "active"
        ),
        ":active pseudo-class should be stored in selector parts"
    );
}

#[test]
fn focus_within_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>form:focus-within { border: 1px solid orange; }</style></head>\
         <body><form><input/></form></body></html>",
            "focus-within"
        ),
        ":focus-within pseudo-class should be stored in selector parts"
    );
}

#[test]
fn checked_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>input:checked { border: 2px solid green; }</style></head>\
         <body><input type=\"checkbox\"/></body></html>",
            "checked"
        ),
        ":checked pseudo-class should be stored in selector parts"
    );
}

#[test]
fn disabled_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>input:disabled { opacity: 0.5; }</style></head>\
         <body><input disabled/></body></html>",
            "disabled"
        ),
        ":disabled pseudo-class should be stored in selector parts"
    );
}

#[test]
fn enabled_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>input:enabled { border: 1px solid gray; }</style></head>\
         <body><input/></body></html>",
            "enabled"
        ),
        ":enabled pseudo-class should be stored in selector parts"
    );
}

#[test]
fn target_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>#section:target { background: yellow; }</style></head>\
         <body><div id=\"section\"></div></body></html>",
            "target"
        ),
        ":target pseudo-class should be stored in selector parts"
    );
}

#[test]
fn visited_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>a:visited { color: purple; }</style></head>\
         <body><a href=\"#\">Link</a></body></html>",
            "visited"
        ),
        ":visited pseudo-class should be stored in selector parts"
    );
}

#[test]
fn link_parsed() {
    assert!(
        rule_has_pseudo_class(
            "<html><head><style>a:link { color: blue; }</style></head>\
         <body><a href=\"#\">Link</a></body></html>",
            "link"
        ),
        ":link pseudo-class should be stored in selector parts"
    );
}

#[test]
fn hover_parsed_as_pseudo_class() {
    // :hover should be stored as SelectorPart::PseudoClass("hover"),
    // AND the rule should have is_hover = true
    let doc = parse_html(
        "<html><head><style>a:hover { color: red; }</style></head>\
         <body><a href=\"#\">Link</a></body></html>",
    );
    let has_hover_part = doc.stylesheet.rules.iter().any(|rule| {
        rule.selectors.iter().any(|sel| {
            sel.parts
                .iter()
                .any(|part| matches!(part, SelectorPart::PseudoClass(name) if name == "hover"))
        })
    });
    let has_is_hover = doc.stylesheet.rules.iter().any(|r| r.is_hover);
    assert!(has_hover_part, ":hover should appear as PseudoClass part");
    assert!(has_is_hover, "rule with :hover should have is_hover=true");
}

// ============================================================
// Selector Roundtrip (original_selector field)
// ============================================================

#[test]
fn focus_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>button:focus { outline: 2px solid blue; }</style></head>\
         <body><button>OK</button></body></html>",
            "button:focus"
        ),
        "button:focus selector must survive in original_selector"
    );
}

#[test]
fn hover_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>a:hover { color: red; }</style></head>\
         <body><a href=\"#\">Link</a></body></html>",
            "a:hover"
        ),
        "a:hover selector must survive in original_selector"
    );
}

#[test]
fn focus_within_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>.nav:focus-within { background: yellow; }</style></head>\
         <body><nav class=\"nav\"><input/></nav></body></html>",
            ":focus-within"
        ),
        ":focus-within must survive in original_selector"
    );
}

#[test]
fn active_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>button:active { background-color: darkblue; }</style></head>\
         <body><button>Press</button></body></html>",
            "button:active"
        ),
        "button:active must survive in original_selector"
    );
}

#[test]
fn checked_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>input:checked { border: 2px solid green; }</style></head>\
         <body><input type=\"checkbox\"/></body></html>",
            "input:checked"
        ),
        "input:checked must survive in original_selector"
    );
}

#[test]
fn visited_roundtrip() {
    assert!(
        rule_original_selector_contains(
            "<html><head><style>a:visited { color: purple; }</style></head>\
         <body><a href=\"http://example.com\">Visited</a></body></html>",
            "a:visited"
        ),
        "a:visited must survive in original_selector"
    );
}

#[test]
fn multiple_pseudo_class_roundtrip() {
    // Compound pseudo-classes must all survive roundtrip
    assert!(
        rule_original_selector_contains(
            "<html><head><style>input:focus:checked { border: 3px solid purple; }</style></head>\
         <body><input type=\"checkbox\"/></body></html>",
            "input:focus:checked"
        ),
        "input:focus:checked compound selector must survive in original_selector"
    );
}

// ============================================================
// No selector corruption for unknown pseudo-classes
// ============================================================

#[test]
fn unknown_pseudo_class_does_not_corrupt_selector() {
    // Parsing a selector with an unknown pseudo-class must not panic
    // and must still store the rule.
    let doc = parse_html(
        "<html><head><style>div.myclass:custom-state { color: red; }</style></head>\
         <body><div class=\"myclass\">Test</div></body></html>",
    );
    // Must not panic. The rule should be stored.
    let found = doc
        .stylesheet
        .rules
        .iter()
        .any(|r| r.original_selector.contains("custom-state"));
    assert!(
        found,
        "unknown pseudo-class rule should be stored in stylesheet"
    );
}

#[test]
fn unknown_pseudo_class_roundtrip() {
    // An unknown pseudo-class in the original HTML must survive in original_selector
    assert!(
        rule_original_selector_contains(
            "<html><head><style>div:custom-state { color: red; }</style></head>\
         <body><div>Test</div></body></html>",
            ":custom-state"
        ),
        "unknown pseudo-class must survive in original_selector for roundtrip"
    );
}

// ============================================================
// Specificity
// ============================================================

#[test]
fn specificity_one_pseudo_class() {
    // a:hover → tag(c=1) + pseudo-class(b=1) = 0*100 + 1*10 + 1 = 11
    let sel = parse_selector("a:hover");
    assert_eq!(
        sel.specificity(),
        11,
        "a:hover specificity should be 11 (tag=1, pseudo-class=10)"
    );
}

#[test]
fn specificity_two_pseudo_classes() {
    // a:hover:focus → tag(c=1) + 2×pseudo-class(b=2) = 0*100 + 2*10 + 1 = 21
    let sel = parse_selector("a:hover:focus");
    assert_eq!(
        sel.specificity(),
        21,
        "a:hover:focus specificity should be 21"
    );
}

#[test]
fn specificity_class_plus_pseudo_class() {
    // .btn:hover → class(b=1) + pseudo-class(b=1) = 0*100 + 2*10 + 0 = 20
    let sel = parse_selector(".btn:hover");
    assert_eq!(sel.specificity(), 20, ".btn:hover specificity should be 20");
}

#[test]
fn specificity_id_plus_pseudo_class() {
    // #nav:focus → id(a=1) + pseudo-class(b=1) = 1*100 + 1*10 + 0 = 110
    let sel = parse_selector("#nav:focus");
    assert_eq!(
        sel.specificity(),
        110,
        "#nav:focus specificity should be 110"
    );
}
