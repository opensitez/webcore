//! Selectors: parsing them into parts, and matching them against a node.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Rule & Selector ─────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum SelectorPart {
    Tag(String),
    Id(String),
    Class(String),
    Universal,
    PseudoClass(String),
    PseudoElement(String),
    /// `[name]`, `[name=value]`, and the rest, with Selectors §6.3's optional
    /// case-sensitivity flag.
    ///
    /// `case_sensitive` is what the SELECTOR asked for, not what HTML defaults
    /// to: `None` means the author wrote no flag and the document's own rule
    /// applies, `Some(false)` is an explicit `i`, `Some(true)` an explicit `s`.
    /// The distinction matters because HTML makes a set of attribute VALUES
    /// case-insensitive on its own (`type`, `dir`, `align`, …) — the UA
    /// stylesheet spells those `[type=hidden i]`, and without the flag the
    /// parser folded the ` i` into the value and the rule matched nothing.
    Attribute {
        name: String,
        op: AttrOp,
        value: String,
        case_sensitive: Option<bool>,
    },
    Combinator(Combinator),
    /// :not(selector)
    Not(Box<CssSelector>),
    /// :is(selector-list)
    Is(Vec<CssSelector>),
    /// :where(selector-list)  — same as Is but zero specificity
    Where(Vec<CssSelector>),
    /// :has(selector) — matches if any descendant matches
    Has(Vec<CssSelector>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttrOp {
    Exists,
    Eq,
    Contains,
    StartsWith,
    EndsWith,
    Includes,
    DashMatch,
}

/// Is `name` a pseudo-class this engine RECOGNISES? Functional forms arrive
/// spelled `nth-child(2n+1)`, so the name is taken up to the `(`.
///
/// Recognising is not the same as matching. `:valid` is on this list and always
/// answers false, because constraint validation is not built yet — that is a
/// missing FEATURE. `:bogus` is not on the list, which makes the selector
/// INVALID, and an invalid selector takes its whole rule down with it
/// (Selectors §3.1). The two have to be told apart before the matcher can stop
/// failing open, or `input:invalid { border: red }` would drop every border it
/// was meant to draw.
///
/// A vendor-prefixed name is treated as recognised-but-never-matching rather
/// than invalid: `:-webkit-autofill` is a real pseudo-class in the engine whose
/// User-Agent string we send, and dropping the author's rule outright is the
/// more damaging of the two wrong answers.
pub fn is_known_pseudo_class(name: &str) -> bool {
    let base = name.split('(').next().unwrap_or(name);
    if base.starts_with('-') {
        return true;
    }
    matches!(
        base,
        // Selectors §6 — structural
        "root" | "empty" | "scope"
        | "first-child" | "last-child" | "only-child"
        | "first-of-type" | "last-of-type" | "only-of-type"
        | "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type"
        | "nth-col" | "nth-last-col"
        // Selectors §4/§5 — logical and linguistic. `not`/`is`/`where`/`has`
        // become their own SelectorPart and never reach here; they are listed
        // so a bare `:is` without an argument list is still recognised.
        | "not" | "is" | "where" | "has" | "matches" | "any"
        | "dir" | "lang"
        // Selectors §7 — location
        | "any-link" | "link" | "visited" | "local-link" | "target" | "target-within"
        // Selectors §8 — user action
        | "hover" | "active" | "focus" | "focus-visible" | "focus-within"
        // Selectors §9/§10 — time-dimensional and resource state
        | "current" | "past" | "future"
        | "playing" | "paused" | "seeking" | "buffering" | "stalled"
        | "muted" | "volume-locked"
        // HTML §4.16.3 and CSS-UI §3 — input state
        | "enabled" | "disabled" | "read-only" | "read-write"
        | "placeholder-shown" | "default" | "checked" | "indeterminate" | "blank"
        | "valid" | "invalid" | "in-range" | "out-of-range"
        | "required" | "optional" | "user-valid" | "user-invalid" | "autofill"
        // HTML §4.16.3 — display state
        | "fullscreen" | "modal" | "picture-in-picture" | "popover-open"
        | "open" | "closed" | "heading"
        // Shadow tree
        | "host" | "host-context" | "defined" | "state" | "slotted"
    )
}

#[derive(Clone, Debug, PartialEq)]
pub enum Combinator {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
    Column,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CssSelector {
    pub parts: Vec<SelectorPart>,
    /// Pre-computed state pseudo-class flags (set during parse, avoids per-match scan).
    pub has_hover: bool,
    pub has_active: bool,
    pub has_visited: bool,
    /// Parts with :hover/:active/:visited stripped. Cached to avoid per-match allocation.
    pub base_parts: Vec<SelectorPart>,
    /// True when selector is a single simple selector (`.class`, `tag`, `#id`) with
    /// no combinators. The candidate_rules index already matched it → skip full matching.
    pub is_simple: bool,
    /// False when the selector contains something this engine does not
    /// recognise — today, an unknown pseudo-class. Selectors §3.1 makes such a
    /// selector invalid, and a rule whose selector list contains an invalid
    /// selector is dropped entirely, so this is checked by the STYLESHEET
    /// parser and not by the matcher.
    pub valid: bool,
}

impl CssSelector {
    /// Create a selector with pre-computed state pseudo-class flags.
    pub fn new(parts: Vec<SelectorPart>) -> Self {
        let has_hover = parts
            .iter()
            .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "hover"));
        let has_active = parts
            .iter()
            .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "active"));
        let has_visited = parts
            .iter()
            .any(|p| matches!(p, SelectorPart::PseudoClass(n) if n == "visited"));
        let base_parts = if has_hover || has_active || has_visited {
            parts
                .iter()
                .filter(|p| {
                    !matches!(p, SelectorPart::PseudoClass(n)
                    if matches!(n.as_str(), "hover" | "active" | "visited"))
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        // Simple selector: no combinators, no pseudo-classes, just class/tag/id parts
        let is_simple = !parts.is_empty()
            && !parts.iter().any(|p| {
                matches!(
                    p,
                    SelectorPart::Combinator(_)
                        | SelectorPart::PseudoClass(_)
                        | SelectorPart::PseudoElement(_)
                        | SelectorPart::Attribute { .. }
                )
            })
            && !has_hover
            && !has_active
            && !has_visited;
        Self {
            parts,
            has_hover,
            has_active,
            has_visited,
            base_parts,
            is_simple,
            valid: true,
        }
    }

    /// `new`, but carrying a validity verdict the parser worked out.
    pub fn new_checked(parts: Vec<SelectorPart>, valid: bool) -> Self {
        let mut s = Self::new(parts);
        s.valid = valid;
        s
    }

    pub fn specificity(&self) -> u32 {
        let mut ids = 0u32;
        let mut classes = 0u32;
        let mut elements = 0u32;
        for part in &self.parts {
            match part {
                SelectorPart::Id(_) => ids += 1,
                SelectorPart::Class(_)
                | SelectorPart::PseudoClass(_)
                | SelectorPart::Attribute { .. } => classes += 1,
                SelectorPart::Tag(t) if t != "*" => elements += 1,
                SelectorPart::PseudoElement(_) => elements += 1,
                // :not() contributes the inner selector's specificity
                SelectorPart::Not(inner) => {
                    let s = inner.specificity();
                    ids += s / 100;
                    classes += (s % 100) / 10;
                    elements += s % 10;
                }
                // :is() contributes the most-specific inner selector
                SelectorPart::Is(list) => {
                    let max_sp = list.iter().map(|s| s.specificity()).max().unwrap_or(0);
                    ids += max_sp / 100;
                    classes += (max_sp % 100) / 10;
                    elements += max_sp % 10;
                }
                // :where() contributes zero specificity
                SelectorPart::Where(_) => {}
                // :has() contributes the inner selector's specificity
                SelectorPart::Has(inner) => {
                    // Most specific branch, like :is() (selectors-4 §16).
                    let s = inner.iter().map(|x| x.specificity()).max().unwrap_or(0);
                    ids += s / 100;
                    classes += (s % 100) / 10;
                    elements += s % 10;
                }
                _ => {}
            }
        }
        ids * 100 + classes * 10 + elements
    }

    /// Match against `b` without ancestor context (for tests / simple selectors).
    pub fn matches_box(&self, b: &WebCore) -> bool {
        let empty_hover = std::collections::HashSet::new();
        let ctx = MatchContext {
            focused_box: 0,
            keyboard_focus: false,
            type_child_index: 0,
            type_sibling_count: 1,
            html_box: Some(b),
            hover_chain: &empty_hover,
            element_id: b.node_id,
            scope_root_id: 0,
            target_id: 0,
            document_url: "",
            prev_siblings: &[],
            next_siblings: &[],
        };
        matches_selector_with_ancestors(&self.parts, &b.tag, &b.attributes, 0, 1, &[], &ctx)
    }

    /// Match against `b` with full ancestor chain for combinator resolution.
    pub fn matches_with_ancestors(
        &self,
        b: &WebCore,
        child_index: usize,
        sibling_count: usize,
        ancestors: &[AncestorInfo],
    ) -> bool {
        let empty_hover = std::collections::HashSet::new();
        let ctx = MatchContext {
            focused_box: 0,
            keyboard_focus: false,
            type_child_index: 0,
            type_sibling_count: 1,
            html_box: Some(b),
            hover_chain: &empty_hover,
            element_id: b.node_id,
            scope_root_id: 0,
            target_id: 0,
            document_url: "",
            prev_siblings: &[],
            next_siblings: &[],
        };
        matches_selector_with_ancestors(
            &self.parts,
            &b.tag,
            &b.attributes,
            child_index,
            sibling_count,
            ancestors,
            &ctx,
        )
    }

    /// Match against `b` with full ancestor chain and extra context.
    pub fn matches_with_ancestors_ctx(
        &self,
        b: &WebCore,
        child_index: usize,
        sibling_count: usize,
        ancestors: &[AncestorInfo],
        ctx: &MatchContext<'_>,
    ) -> bool {
        matches_selector_with_ancestors(
            &self.parts,
            &b.tag,
            &b.attributes,
            child_index,
            sibling_count,
            ancestors,
            ctx,
        )
    }

    /// Internal: match using raw tag/attrs (used from :not/:is/:where to avoid re-borrowing WebCore).
    pub(crate) fn matches_with_ancestors_ctx_raw(
        &self,
        tag: &str,
        attrs: &crate::dom::attrs::AttrMap,
        child_index: usize,
        sibling_count: usize,
        ancestors: &[AncestorInfo],
        ctx: &MatchContext<'_>,
    ) -> bool {
        matches_selector_with_ancestors(
            &self.parts,
            tag,
            attrs,
            child_index,
            sibling_count,
            ancestors,
            ctx,
        )
    }
}
