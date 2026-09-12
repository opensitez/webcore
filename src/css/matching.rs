//! Selector MATCHING: the ancestor Bloom filter that rejects most selectors
//! without a tree walk, the match context, and the matcher itself.
//!
//! ⛔ Named for the Bloom filter when it was split out, which described 50 of
//! its 770 lines. A file name that does not say what is in it is the same
//! problem as a `mod.rs` that holds everything.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Bloom filter for fast ancestor rejection ────────────────────────────────

/// A compact Bloom filter for ancestor tag/class names.
/// Used to quickly reject selectors that require an ancestor with a specific
/// tag or class that no ancestor has. False positives are OK (full match catches them),
/// false negatives are not (would incorrectly skip matching rules).
#[derive(Clone)]
pub struct AncestorBloom {
    bits: [u64; 4], // 256 bits
}

impl AncestorBloom {
    pub fn new() -> Self {
        Self { bits: [0; 4] }
    }

    #[inline]
    fn hash(s: &str) -> (usize, usize) {
        // Two simple hash functions for double-hashing
        let mut h1: u32 = 0x811c9dc5;
        let mut h2: u32 = 0;
        for &b in s.as_bytes() {
            h1 = h1.wrapping_mul(0x01000193) ^ (b as u32);
            h2 = h2.wrapping_add(b as u32).wrapping_mul(31);
        }
        ((h1 as usize) % 256, (h2 as usize) % 256)
    }

    #[inline]
    pub fn add(&mut self, s: &str) {
        let (h1, h2) = Self::hash(s);
        self.bits[h1 / 64] |= 1 << (h1 % 64);
        self.bits[h2 / 64] |= 1 << (h2 % 64);
    }

    /// Add a node's tag and class names to the filter.
    pub fn add_element(&mut self, tag: &str, attrs: &std::collections::HashMap<String, String>) {
        self.add(tag);
        if let Some(cls) = attrs.get("class") {
            for c in cls.split_whitespace() {
                self.add(c);
            }
        }
        if let Some(id) = attrs.get("id") {
            self.add(id);
        }
    }

    #[inline]
    pub fn might_contain(&self, s: &str) -> bool {
        let (h1, h2) = Self::hash(s);
        (self.bits[h1 / 64] & (1 << (h1 % 64))) != 0 && (self.bits[h2 / 64] & (1 << (h2 % 64))) != 0
    }
}

impl Default for AncestorBloom {
    fn default() -> Self {
        Self::new()
    }
}
impl std::fmt::Debug for AncestorBloom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AncestorBloom({} bits set)",
            self.bits.iter().map(|w| w.count_ones()).sum::<u32>()
        )
    }
}

/// Info about one ancestor box, threaded through the cascade for selector matching.
#[derive(Clone, Debug, Default)]
pub struct AncestorInfo {
    pub tag: String,
    pub attributes: crate::dom::attrs::AttrMap,
    pub child_index: usize,        // 0-based position among parent's children
    pub sibling_count: usize,      // total children of parent
    pub type_child_index: usize,   // 0-based among same-tag siblings
    pub type_sibling_count: usize, // count of same-tag siblings
    pub node_id: u32,              // stable node id for hover chain check
}

/// Extra context passed down through selector matching.
#[derive(Clone, Copy, Debug)]
pub struct MatchContext<'a> {
    /// Node ID of the focused element (0 = none).
    pub focused_box: u32,
    /// True when focus was moved by keyboard (Tab/Shift+Tab) — drives :focus-visible.
    pub keyboard_focus: bool,
    /// 0-based position among same-tag siblings.
    pub type_child_index: usize,
    /// Count of same-tag siblings (including this element).
    pub type_sibling_count: usize,
    /// Raw pointer to the WebCore being matched (for :has()).
    pub html_box: Option<&'a crate::types::WebCore>,
    /// Set of node IDs on the hover chain (hovered element + all ancestors).
    /// When non-empty, :hover pseudo-class matches elements in this set.
    pub hover_chain: &'a std::collections::HashSet<u32>,
    /// Node ID of the element currently being matched (for :hover on ancestors).
    pub element_id: u32,
    /// Node ID of the selector scope root for `:scope` (0 = no scoped query).
    pub scope_root_id: u32,
    /// Node ID named by the document URL fragment for `:target` selectors.
    pub target_id: u32,
    /// Document URL used by URL-state selectors such as `:local-link`.
    pub document_url: &'a str,
    /// Previous non-text sibling info for `+` and `~` combinators.
    /// Each entry: (tag, id, classes) of preceding element siblings.
    pub prev_siblings: &'a [(String, String, String)],
    /// Following non-text sibling info for right-to-left selectors such as
    /// `:nth-last-child(An+B of S)`.
    pub next_siblings: &'a [(String, String, String)],
    /// Following sibling DOM element nodes for forward-looking relative selectors
    /// such as `:has(+ S)` and `:has(~ S)`.
    pub next_sibling_nodes: &'a [&'a crate::types::WebCore],
}

/// Recursively match a selector (parts slice) against a subject element + its ancestor chain.
/// Works right-to-left: the last segment matches the subject, preceding segments
/// must match ancestors according to the combinator between them.
pub fn matches_selector_with_ancestors(
    parts: &[SelectorPart],
    tag: &str,
    attrs: &crate::dom::attrs::AttrMap,
    child_index: usize,
    sibling_count: usize,
    ancestors: &[AncestorInfo],
    ctx: &MatchContext<'_>,
) -> bool {
    if parts.is_empty() {
        return true;
    }

    // Find the rightmost combinator in `parts`
    let last_comb_pos = parts
        .iter()
        .rposition(|p| matches!(p, SelectorPart::Combinator(_)));

    match last_comb_pos {
        None => {
            // No combinator — all parts must match the subject
            parts.iter().all(|p| {
                matches_part_with_context(p, tag, attrs, child_index, sibling_count, ancestors, ctx)
            })
        }
        Some(pos) => {
            let combinator = match &parts[pos] {
                SelectorPart::Combinator(c) => c.clone(),
                _ => unreachable!(),
            };
            let left_parts = &parts[..pos];
            let right_parts = &parts[pos + 1..];

            // Right parts must all match the subject
            if !right_parts.iter().all(|p| {
                matches_part_with_context(p, tag, attrs, child_index, sibling_count, ancestors, ctx)
            }) {
                return false;
            }

            match combinator {
                Combinator::Descendant => {
                    // Left parts must match any ancestor
                    for (i, anc) in ancestors.iter().enumerate() {
                        let anc_ctx = MatchContext {
                            focused_box: ctx.focused_box,
                            keyboard_focus: ctx.keyboard_focus,
                            type_child_index: anc.type_child_index,
                            type_sibling_count: anc.type_sibling_count,
                            html_box: None,
                            hover_chain: ctx.hover_chain,
                            element_id: anc.node_id,
                            scope_root_id: ctx.scope_root_id,
                            target_id: ctx.target_id,
                            document_url: ctx.document_url,
                            prev_siblings: &[],
                            next_siblings: &[],
                            next_sibling_nodes: &[],
                        };
                        if matches_selector_with_ancestors(
                            left_parts,
                            &anc.tag,
                            &anc.attributes,
                            anc.child_index,
                            anc.sibling_count,
                            &ancestors[..i],
                            &anc_ctx,
                        ) {
                            return true;
                        }
                    }
                    false
                }
                Combinator::Child => {
                    // Left parts must match the direct parent (last ancestor)
                    if let Some(parent) = ancestors.last() {
                        let parent_ancestors = &ancestors[..ancestors.len() - 1];
                        let parent_ctx = MatchContext {
                            focused_box: ctx.focused_box,
                            keyboard_focus: ctx.keyboard_focus,
                            type_child_index: parent.type_child_index,
                            type_sibling_count: parent.type_sibling_count,
                            html_box: None,
                            hover_chain: ctx.hover_chain,
                            element_id: parent.node_id,
                            scope_root_id: ctx.scope_root_id,
                            target_id: ctx.target_id,
                            document_url: ctx.document_url,
                            prev_siblings: &[],
                            next_siblings: &[],
                            next_sibling_nodes: &[],
                        };
                        matches_selector_with_ancestors(
                            left_parts,
                            &parent.tag,
                            &parent.attributes,
                            parent.child_index,
                            parent.sibling_count,
                            parent_ancestors,
                            &parent_ctx,
                        )
                    } else {
                        false
                    }
                }
                // ⛔ Both sibling branches RECURSE. They used to match
                // `left_parts` as a flat compound directly against the sibling,
                // which is only correct when `left_parts` contains no further
                // combinator — and `matches_part_with_context` answers `true`
                // for a `Combinator` part, so anything to its left was then
                // tested against the SIBLING instead of the sibling's ancestor.
                //
                // The effect: sibling combinators worked alone and failed the
                // moment anything preceded them. `i + i` matched; `#p i + i`
                // and `#p > i + i` did not — so `.container > li + li` and
                // `.card h2 + p`, which are everyday selectors, silently never
                // applied.
                Combinator::AdjacentSibling => match ctx.prev_siblings.split_last() {
                    Some((last, before)) => {
                        matches_sibling(left_parts, last, before, ancestors, ctx)
                    }
                    None => false,
                },
                Combinator::GeneralSibling => {
                    // Any previous sibling, each seen with only ITS OWN
                    // preceding siblings — so a nested `a ~ b + c` is judged
                    // against the right list rather than the subject's.
                    (0..ctx.prev_siblings.len()).any(|i| {
                        matches_sibling(
                            left_parts,
                            &ctx.prev_siblings[i],
                            &ctx.prev_siblings[..i],
                            ancestors,
                            ctx,
                        )
                    })
                }
                // Column combinators need table-column association data in the
                // match context. Until that exists, fail closed instead of
                // degrading `col || td` to `col td` and styling every cell.
                Combinator::Column => false,
            }
        }
    }
}

/// Match the left-hand side of a sibling combinator against one sibling.
///
/// Recurses through `matches_selector_with_ancestors` so any further
/// combinator inside `left_parts` is resolved against that sibling's own
/// context: the siblings that precede IT, and the ancestors it shares with the
/// subject.
fn matches_sibling(
    left_parts: &[SelectorPart],
    sib: &(String, String, String),
    sib_prev: &[(String, String, String)],
    ancestors: &[AncestorInfo],
    ctx: &MatchContext<'_>,
) -> bool {
    let mut attrs = crate::dom::attrs::AttrMap::new();
    if !sib.1.is_empty() {
        attrs.insert("id".to_string(), sib.1.clone());
    }
    if !sib.2.is_empty() {
        attrs.insert("class".to_string(), sib.2.clone());
    }
    let sib_ctx = MatchContext {
        focused_box: ctx.focused_box,
        keyboard_focus: ctx.keyboard_focus,
        type_child_index: 0,
        type_sibling_count: 0,
        // ⛔ Not the subject's box: `:has()` and the other box-state
        // pseudo-classes must not answer for the sibling from it.
        html_box: None,
        hover_chain: ctx.hover_chain,
        element_id: 0,
        scope_root_id: ctx.scope_root_id,
        target_id: ctx.target_id,
        document_url: ctx.document_url,
        prev_siblings: sib_prev,
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    matches_selector_with_ancestors(
        left_parts,
        &sib.0,
        &attrs,
        sib_prev.len(),
        0,
        ancestors,
        &sib_ctx,
    )
}

pub(crate) fn matches_part_with_context(
    part: &SelectorPart,
    tag: &str,
    attrs: &crate::dom::attrs::AttrMap,
    child_index: usize,
    sibling_count: usize,
    ancestors: &[AncestorInfo],
    ctx: &MatchContext<'_>,
) -> bool {
    match part {
        SelectorPart::Universal => true,
        SelectorPart::Tag(t) => tag.eq_ignore_ascii_case(t),
        SelectorPart::Id(id) => attrs.get("id").map(|s| s == id).unwrap_or(false),
        SelectorPart::Class(cls) => attrs
            .get("class")
            .map(|s| s.split_whitespace().any(|c| c == cls))
            .unwrap_or(false),
        SelectorPart::Attribute {
            name,
            op,
            value,
            case_sensitive,
        } => {
            // **An attribute NAME in a selector is ASCII case-insensitive for
            // an HTML document.** HTML folds attribute names on the way in, so
            // `[DATA-Foo]` and `[data-foo]` name the same attribute — and a
            // page that writes `setAttribute("DATA-Foo", …)` and then styles
            // `[DATA-Foo]` is one a browser handles without comment.
            //
            // Exact FIRST, fold only on a miss — the same shape the namespace
            // tree settled on, and it leaves an XML document (whose attribute
            // names keep their case) matching exactly as it must.
            //
            // The tag name already worked this way (`eq_ignore_ascii_case`);
            // the attribute name was the half nobody folded, so `[DATA-foo]`
            // silently matched nothing.
            let found = attrs.get(name).or_else(|| {
                attrs
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v)
            });
            // Selectors §6.3. An explicit `s` forces case-sensitive matching,
            // an explicit `i` forces case-insensitive; with no flag the value
            // is matched case-sensitively, which is what HTML asks for on every
            // attribute the UA sheet does not tag with `i`.
            let fold = case_sensitive == &Some(false);
            let av_owned;
            let val_owned;
            let (av_cmp, val_cmp): (&str, &str) = match found {
                None => return false,
                Some(av) if fold => {
                    av_owned = av.to_ascii_lowercase();
                    val_owned = value.to_ascii_lowercase();
                    (&av_owned, &val_owned)
                }
                Some(av) => (av.as_str(), value.as_str()),
            };
            match op {
                AttrOp::Exists => true,
                AttrOp::Eq => av_cmp == val_cmp,
                AttrOp::Includes => av_cmp.split_whitespace().any(|w| w == val_cmp),
                AttrOp::StartsWith => av_cmp.starts_with(val_cmp),
                AttrOp::EndsWith => av_cmp.ends_with(val_cmp),
                AttrOp::Contains => av_cmp.contains(val_cmp),
                AttrOp::DashMatch => {
                    av_cmp == val_cmp || av_cmp.starts_with(&format!("{}-", val_cmp))
                }
            }
        }
        SelectorPart::Not(inner) => !inner.matches_with_ancestors_ctx_raw(
            tag,
            attrs,
            child_index,
            sibling_count,
            ancestors,
            ctx,
        ),
        SelectorPart::Is(list) => list.iter().any(|sel| {
            sel.matches_with_ancestors_ctx_raw(
                tag,
                attrs,
                child_index,
                sibling_count,
                ancestors,
                ctx,
            )
        }),
        SelectorPart::Where(list) => list.iter().any(|sel| {
            sel.matches_with_ancestors_ctx_raw(
                tag,
                attrs,
                child_index,
                sibling_count,
                ancestors,
                ctx,
            )
        }),
        SelectorPart::Has(inner) => {
            if let Some(b) = ctx.html_box {
                inner.iter().any(|sel| has_relative_matching(b, sel, ctx))
            } else {
                false
            }
        }
        SelectorPart::PseudoClass(pc) => {
            let pc = pc.as_str();
            match pc {
                "first-child" => child_index == 0,
                "last-child" => child_index + 1 == sibling_count,
                "only-child" => sibling_count == 1,
                "first-of-type" => ctx.type_child_index == 0,
                "last-of-type" => ctx.type_child_index + 1 == ctx.type_sibling_count,
                "only-of-type" => ctx.type_sibling_count == 1,
                "root" => tag.eq_ignore_ascii_case("html"),
                "scope" => ctx.scope_root_id != 0 && ctx.element_id == ctx.scope_root_id,
                // Selectors §14.3 — no element children and no TEXT children.
                // Comments and processing instructions do not count, which is
                // why this asks `is_element`/`is_text_node` rather than
                // `children.is_empty()`: since comments became real nodes,
                // `<div><!--x--></div>` has a child and is still `:empty`.
                //
                // `html_box` is `None` when the subject is an ANCESTOR being
                // matched for a combinator, and false is the right answer
                // there: an `:empty` element has no descendants for the rest of
                // the selector to have matched.
                "empty" => match ctx.html_box {
                    Some(b) => !b
                        .children
                        .iter()
                        .any(|c| c.is_element() || (c.is_text_node() && !c.text.is_empty())),
                    None => false,
                },
                // Focus
                "focus" => {
                    if ctx.focused_box != 0 {
                        if let Some(b) = ctx.html_box {
                            b.node_id != 0 && b.node_id == ctx.focused_box
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                // :focus-visible matches when focus arrived via keyboard, OR when the
                // element is a text-entry control (input, textarea, contenteditable) —
                // matching browser behaviour where the caret always needs a visible ring.
                "focus-visible" => {
                    if ctx.focused_box == 0 {
                        return false;
                    }
                    if let Some(b) = ctx.html_box {
                        if b.node_id == 0 || b.node_id != ctx.focused_box {
                            return false;
                        }
                        ctx.keyboard_focus || is_text_entry(b)
                    } else {
                        // Fallback: use element_id when html_box not available
                        ctx.element_id != 0
                            && ctx.element_id == ctx.focused_box
                            && ctx.keyboard_focus
                    }
                }
                "focus-within" => {
                    if ctx.focused_box != 0 {
                        if let Some(b) = ctx.html_box {
                            // Is this box itself focused, or does it contain the focused element?
                            (b.node_id != 0 && b.node_id == ctx.focused_box)
                                || is_or_contains_focused(b, ctx.focused_box)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                // Form state
                // `:checked` matches CHECKEDNESS (Selectors §11.1: "elements
                // that are checked"), not the `checked` attribute. Matching the
                // attribute meant a box the user ticked kept the unchecked
                // styling while the painter drew a tick — the selector and the
                // paint disagreeing about one box.
                // Top-layer membership (CSS Position §6). Both of these were
                // in `is_known_pseudo_class` — so they PARSED — with no arm
                // here at all: recognised names that matched nothing, which is
                // indistinguishable from a rule that never applies.
                "modal" => matches!(
                    ctx.html_box.and_then(|b| b.top_layer_kind),
                    Some(crate::types::TopLayerKind::ModalDialog)
                ),
                "popover-open" => matches!(
                    ctx.html_box.and_then(|b| b.top_layer_kind),
                    Some(crate::types::TopLayerKind::Popover)
                ),
                "open" => {
                    attrs.contains_key("open")
                        || matches!(ctx.html_box.and_then(|b| b.top_layer_kind), Some(_))
                }
                "closed" => {
                    matches!(tag, "details" | "dialog")
                        && !attrs.contains_key("open")
                        && ctx.html_box.and_then(|b| b.top_layer_kind).is_none()
                }
                "checked" => {
                    // The BOX's state when the matcher was given one; the
                    // attribute is the fallback for the paths that match
                    // against a bare tag+attrs (an `<option selected>` has no
                    // checkedness of its own).
                    ctx.html_box
                        .map(|b| b.checkedness)
                        .unwrap_or_else(|| attrs.contains_key("checked"))
                        || attrs.contains_key("selected")
                }
                // Disabledness is INHERITED from a disabled `<fieldset>`
                // (HTML §4.10.19.6), so the attribute on the element itself is
                // only half the question. The ancestor chain carries each
                // ancestor's attributes, which is what makes the second half
                // answerable on the raw tag+attrs path as well as the box path.
                "disabled" => is_actually_disabled(tag, attrs, ancestors),
                "enabled" => is_form_control(tag) && !is_actually_disabled(tag, attrs, ancestors),
                "read-only" => {
                    attrs.contains_key("readonly")
                        || !matches!(tag, "input" | "textarea" | "select" | "button")
                }
                "read-write" => {
                    !attrs.contains_key("readonly")
                        && matches!(tag, "input" | "textarea" | "select" | "button")
                }
                // Link
                "any-link" | "link" => {
                    attrs.contains_key("href") && matches!(tag, "a" | "area" | "link")
                }
                "visited" | "active" => false,
                "hover" => {
                    // :hover matches when the element being matched is in the
                    // hover chain (the hovered element + all its ancestors).
                    if !ctx.hover_chain.is_empty() && ctx.element_id != 0 {
                        ctx.hover_chain.contains(&ctx.element_id)
                    } else {
                        false
                    }
                }
                // HTML §4.16.3. `required`/`optional` are a partition over the
                // controls that CAN be required — a `<div required>` is
                // neither, and neither is an `<input type=button required>`.
                "required" => is_requirable(tag, attrs) && attrs.contains_key("required"),
                "optional" => is_requirable(tag, attrs) && !attrs.contains_key("required"),
                // The placeholder is showing when there IS one and the control
                // is empty. The box carries the live value; `value` is the
                // fallback for the paths that match on bare tag+attrs.
                "placeholder-shown" => {
                    let has_placeholder = attrs
                        .get("placeholder")
                        .map(|p| !p.is_empty())
                        .unwrap_or(false);
                    if !has_placeholder || !matches!(tag, "input" | "textarea") {
                        return false;
                    }
                    match ctx.html_box.and_then(|b| b.value_state.as_ref()) {
                        Some(v) => v.is_empty(),
                        None => attrs.get("value").map(|v| v.is_empty()).unwrap_or(true),
                    }
                }
                // A checkbox or radio put in the indeterminate state by script.
                // It is NOT the `indeterminate` content attribute — there isn't
                // one — so a box is the only place the answer can come from.
                "indeterminate" => ctx
                    .html_box
                    .map(|b| {
                        b.data
                            .get("indeterminate")
                            .map(|v| v == "true")
                            .unwrap_or(false)
                    })
                    .unwrap_or(false),
                // `:default` covers two things, and only one of them is
                // answerable here. A checkbox, radio or option that was checked
                // IN THE MARKUP is `:default` — the attribute, deliberately,
                // not the current checkedness, so unticking a box does not stop
                // it being the default.
                //
                // The other half is the form's DEFAULT BUTTON: the first submit
                // button in tree order whose form owner is this form. That
                // needs to compare an element against its siblings through a
                // form owner, and the matcher is given one element plus its
                // ancestors — so answering it here would mean "every submit
                // button is the default", which is a wrong answer rather than a
                // missing one. It is left out until the match context can carry
                // the form.
                "default" => match tag {
                    "input" => attrs.contains_key("checked"),
                    "option" => attrs.contains_key("selected"),
                    _ => false,
                },
                "valid" => {
                    selector_validity(tag, attrs, ctx.html_box, ancestors).is_some_and(|v| v.valid)
                }
                "invalid" => {
                    selector_validity(tag, attrs, ctx.html_box, ancestors).is_some_and(|v| !v.valid)
                }
                "in-range" => selector_validity(tag, attrs, ctx.html_box, ancestors)
                    .is_some_and(|v| v.range_applicable && v.valid),
                "out-of-range" => selector_validity(tag, attrs, ctx.html_box, ancestors)
                    .is_some_and(|v| v.range_underflow || v.range_overflow),
                "user-valid" => {
                    ctx.html_box.is_some_and(selector_has_user_input)
                        && selector_validity(tag, attrs, ctx.html_box, ancestors)
                            .is_some_and(|v| v.valid)
                }
                "user-invalid" => {
                    ctx.html_box.is_some_and(selector_has_user_input)
                        && selector_validity(tag, attrs, ctx.html_box, ancestors)
                            .is_some_and(|v| !v.valid)
                }
                "autofill" => ctx.html_box.is_some_and(|node| node.autofilled),
                "blank" => ctx
                    .html_box
                    .map(selector_box_value)
                    .unwrap_or_default()
                    .is_empty(),
                _ => {
                    // nth-child(expr) / nth-of-type(expr)
                    if let Some(inner) = pc
                        .strip_prefix("nth-child(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        // `An+B of S` — Selectors 4 §9.3. The index counts only
                        // siblings matching S, and the element itself must
                        // match S too. Without this the whole argument was
                        // handed to the An+B parser, which cannot read
                        // `2 of .pick` and answered no-match for everything.
                        if let Some((nth, sel_src)) = split_nth_of(inner) {
                            let sel = crate::css::parser::parse_selector(&sel_src);
                            if !sel.valid {
                                return false;
                            }
                            if !simple_matches(&sel, tag, attrs) {
                                return false;
                            }
                            let index = ctx
                                .prev_siblings
                                .iter()
                                .filter(|(t, i, c)| simple_matches_raw(&sel, t, i, c))
                                .count();
                            return nth_matches(&nth, index + 1);
                        }
                        return nth_matches(inner, child_index + 1); // CSS is 1-based
                    }
                    if let Some(inner) = pc
                        .strip_prefix("nth-last-child(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        if let Some((nth, sel_src)) = split_nth_of(inner) {
                            let sel = crate::css::parser::parse_selector(&sel_src);
                            if !sel.valid {
                                return false;
                            }
                            if !simple_matches(&sel, tag, attrs) {
                                return false;
                            }
                            let after = ctx
                                .next_siblings
                                .iter()
                                .filter(|(t, i, c)| simple_matches_raw(&sel, t, i, c))
                                .count();
                            return nth_matches(&nth, after + 1);
                        }
                        let from_end = sibling_count - child_index; // 1-based from end
                        return nth_matches(inner, from_end);
                    }
                    if let Some(inner) = pc
                        .strip_prefix("nth-of-type(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        return nth_matches(inner, ctx.type_child_index + 1);
                    }
                    if let Some(inner) = pc
                        .strip_prefix("nth-last-of-type(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        let from_end = ctx.type_sibling_count - ctx.type_child_index;
                        return nth_matches(inner, from_end);
                    }
                    // Shadow DOM pseudo-classes: never match in non-shadow context
                    // ⛔ ANSWERED, not assumed false. `:lang()` and `:dir()`
                    // parsed as valid pseudo-classes and then always lost, so
                    // every language- and direction-conditional rule was dead.
                    // Both read an ATTRIBUTE that inherits down the tree, so the
                    // nearest one on the element or an ancestor wins.
                    if let Some(want) = pc.strip_prefix("lang(").and_then(|s| s.strip_suffix(')')) {
                        let want = want
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'')
                            .to_ascii_lowercase();
                        let found = attrs.get("lang").cloned().or_else(|| {
                            ancestors
                                .iter()
                                .rev()
                                .find_map(|a| a.attributes.get("lang").cloned())
                        });
                        return match found {
                            // A language range matches a prefix at a hyphen
                            // boundary: `:lang(en)` matches `en-GB`.
                            Some(l) => {
                                let l = l.trim().to_ascii_lowercase();
                                l == want
                                    || l.strip_prefix(&want)
                                        .map(|r| r.starts_with('-'))
                                        .unwrap_or(false)
                            }
                            None => false,
                        };
                    }
                    if let Some(want) = pc.strip_prefix("dir(").and_then(|s| s.strip_suffix(')')) {
                        let want = want.trim().to_ascii_lowercase();
                        let found = attrs.get("dir").cloned().or_else(|| {
                            ancestors
                                .iter()
                                .rev()
                                .find_map(|a| a.attributes.get("dir").cloned())
                        });
                        // The default directionality is ltr.
                        let dir = found
                            .map(|d| d.trim().to_ascii_lowercase())
                            .filter(|d| d == "rtl" || d == "ltr")
                            .unwrap_or_else(|| "ltr".to_string());
                        return dir == want;
                    }
                    if pc == "host" {
                        return ctx.html_box.is_none() && !ancestors.is_empty();
                    }
                    if let Some(arg) = pc.strip_prefix("host(").and_then(|s| s.strip_suffix(')')) {
                        if ctx.html_box.is_some() || ancestors.is_empty() {
                            return false;
                        }
                        let parsed = parse_selector(arg.trim());
                        if parsed.parts.is_empty() {
                            return false;
                        }
                        return parsed.matches_with_ancestors_ctx_raw(
                            tag,
                            attrs,
                            child_index,
                            sibling_count,
                            ancestors,
                            ctx,
                        );
                    }
                    if let Some(arg) = pc
                        .strip_prefix("host-context(")
                        .and_then(|s| s.strip_suffix(')'))
                    {
                        if ctx.html_box.is_some() || ancestors.is_empty() {
                            return false;
                        }
                        let parsed = parse_selector(arg.trim());
                        if parsed.parts.is_empty() {
                            return false;
                        }
                        if parsed.matches_with_ancestors_ctx_raw(
                            tag,
                            attrs,
                            child_index,
                            sibling_count,
                            ancestors,
                            ctx,
                        ) {
                            return true;
                        }
                        return ancestors.iter().enumerate().any(|(i, anc)| {
                            let anc_ctx = MatchContext {
                                focused_box: ctx.focused_box,
                                keyboard_focus: ctx.keyboard_focus,
                                type_child_index: anc.type_child_index,
                                type_sibling_count: anc.type_sibling_count,
                                html_box: None,
                                hover_chain: ctx.hover_chain,
                                element_id: anc.node_id,
                                scope_root_id: ctx.scope_root_id,
                                target_id: ctx.target_id,
                                document_url: ctx.document_url,
                                prev_siblings: &[],
                                next_siblings: &[],
                                next_sibling_nodes: &[],
                            };
                            parsed.matches_with_ancestors_ctx_raw(
                                &anc.tag,
                                &anc.attributes,
                                anc.child_index,
                                anc.sibling_count,
                                &ancestors[..i],
                                &anc_ctx,
                            )
                        });
                    }
                    if pc == "target" {
                        return ctx.target_id != 0 && ctx.element_id == ctx.target_id;
                    }
                    if pc == "target-within" {
                        return ctx.target_id != 0
                            && (ctx.element_id == ctx.target_id
                                || ctx
                                    .html_box
                                    .is_some_and(|b| contains_node_id(b, ctx.target_id)));
                    }
                    if pc == "local-link" {
                        return is_local_link(tag, attrs, ctx.document_url);
                    }

                    // Everything left is a pseudo-class this engine recognises
                    // but has no state for — `:fullscreen` and the media-resource
                    // ones. They do not match.
                    //
                    // This used to `return true` "for forward compat", which
                    // meant `:target` matched EVERY element and any typo styled
                    // the whole document. Selectors are validated at parse time
                    // now (`is_known_pseudo_class`), so an unrecognised name has
                    // already taken its rule down before reaching the matcher
                    // and there is nothing left here to be lenient about.
                    false
                }
            }
        }
        SelectorPart::PseudoElement(_) => false, // pseudo-elements never match real elements
        SelectorPart::Combinator(_) => true,
    }
}

/// The elements `:enabled` / `:disabled` are defined over (HTML §4.16.3):
/// form controls, plus `<fieldset>`, `<optgroup>` and `<option>`.
fn is_form_control(tag: &str) -> bool {
    matches!(
        tag,
        "input" | "button" | "select" | "textarea" | "fieldset" | "optgroup" | "option"
    )
}

/// Is this element disabled, counting a disabled `<fieldset>` ancestor?
///
/// HTML §4.10.19.6: a control inside a disabled `<fieldset>` is itself
/// disabled, unless it sits in that fieldset's FIRST `<legend>`. The legend
/// exemption needs to know which legend is first among its siblings, which the
/// ancestor chain does record — `child_index` on the legend's own entry.
fn is_actually_disabled(
    tag: &str,
    attrs: &crate::dom::attrs::AttrMap,
    ancestors: &[AncestorInfo],
) -> bool {
    if !is_form_control(tag) {
        return false;
    }
    if attrs.contains_key("disabled") {
        return true;
    }
    // Walk from the element outwards. A `<legend>` seen on the way up shields
    // the element from the fieldset that legend belongs to — but only if it is
    // that fieldset's first legend, and only for the fieldset immediately
    // outside it.
    let mut shielded_by_legend = false;
    for anc in ancestors.iter().rev() {
        if anc.tag == "legend" {
            // The FIRST `<legend>` child, not the first child: in
            // `<fieldset disabled><p>x</p><legend><input></legend>` the legend
            // is child 1 and still shields. `type_child_index` counts among
            // same-tag siblings, which is exactly that question.
            shielded_by_legend = anc.type_child_index == 0;
            continue;
        }
        if anc.tag == "fieldset" && anc.attributes.contains_key("disabled") && !shielded_by_legend {
            return true;
        }
        shielded_by_legend = false;
    }
    false
}

/// Can this element be `required`? Only the controls that take the attribute —
/// `:optional` is "requirable but not required", so a `<div>` is neither.
fn is_requirable(tag: &str, attrs: &crate::dom::attrs::AttrMap) -> bool {
    match tag {
        "select" | "textarea" => true,
        "input" => !matches!(
            attrs.get("type").map(|s| s.to_ascii_lowercase()).as_deref(),
            Some("hidden")
                | Some("range")
                | Some("color")
                | Some("submit")
                | Some("image")
                | Some("reset")
                | Some("button")
        ),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct SelectorValidity {
    valid: bool,
    range_applicable: bool,
    range_underflow: bool,
    range_overflow: bool,
}

fn selector_validity(
    tag: &str,
    attrs: &crate::dom::attrs::AttrMap,
    html_box: Option<&crate::types::WebCore>,
    ancestors: &[AncestorInfo],
) -> Option<SelectorValidity> {
    if !is_form_control(tag) || is_actually_disabled(tag, attrs, ancestors) {
        return None;
    }
    let input_type = if tag == "input" {
        attrs
            .get("type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "text".to_string())
    } else {
        String::new()
    };
    if matches!(
        input_type.as_str(),
        "hidden" | "button" | "submit" | "reset" | "image" | "color"
    ) {
        return None;
    }

    let value = html_box
        .map(selector_box_value)
        .or_else(|| attrs.get("value").cloned())
        .unwrap_or_default();
    let mut out = SelectorValidity {
        valid: true,
        ..SelectorValidity::default()
    };
    if attrs.contains_key("required") {
        let missing = match input_type.as_str() {
            "checkbox" | "radio" => !html_box.map(|b| b.checkedness).unwrap_or(false),
            _ => value.is_empty(),
        };
        if missing {
            out.valid = false;
        }
    }
    if value.is_empty() {
        return Some(out);
    }
    if matches!(input_type.as_str(), "number" | "range") {
        out.range_applicable = true;
        match crate::html::forms::parse_floating_point(&value) {
            Some(n) => {
                if let Some(min) = attrs
                    .get("min")
                    .and_then(|s| crate::html::forms::parse_floating_point(s))
                {
                    out.range_underflow = n < min;
                }
                if let Some(max) = attrs
                    .get("max")
                    .and_then(|s| crate::html::forms::parse_floating_point(s))
                {
                    out.range_overflow = n > max;
                }
                if out.range_underflow || out.range_overflow {
                    out.valid = false;
                }
            }
            None => out.valid = false,
        }
    }
    Some(out)
}

fn selector_has_user_input(node: &crate::types::WebCore) -> bool {
    node.dirty_value || node.dirty_checked || node.dirty_selectedness
}

fn selector_box_value(b: &crate::types::WebCore) -> String {
    if let Some(value) = &b.value_state {
        return value.clone();
    }
    if b.tag == "textarea" {
        return b
            .children
            .iter()
            .filter(|c| c.tag == "#text")
            .map(|c| c.text.as_str())
            .collect::<String>();
    }
    b.attributes.get("value").cloned().unwrap_or_default()
}

/// Returns true for text-entry controls — these always show :focus-visible even
/// when focused by mouse, because the cursor position needs to be visible.
fn is_text_entry(b: &crate::types::WebCore) -> bool {
    match b.tag.as_str() {
        "textarea" => true,
        "input" => !matches!(
            b.attributes.get("type").map(|s| s.as_str()),
            Some(
                "button" | "submit" | "reset" | "checkbox" | "radio" | "range" | "color" | "hidden"
            )
        ),
        _ => b
            .attributes
            .get("contenteditable")
            .map(|v| v == "true" || v.is_empty())
            .unwrap_or(false),
    }
}

/// Check if `b` or any of its descendants is the focused element.
fn is_or_contains_focused(b: &crate::types::WebCore, focused: u32) -> bool {
    for child in &b.children {
        if child.node_id != 0 && child.node_id == focused {
            return true;
        }
        if is_or_contains_focused(child, focused) {
            return true;
        }
    }
    false
}

fn contains_node_id(b: &crate::types::WebCore, target_id: u32) -> bool {
    for child in &b.children {
        if child.node_id != 0 && child.node_id == target_id {
            return true;
        }
        if contains_node_id(child, target_id) {
            return true;
        }
    }
    false
}

fn is_local_link(tag: &str, attrs: &crate::dom::attrs::AttrMap, document_url: &str) -> bool {
    if !matches!(tag, "a" | "area" | "link") {
        return false;
    }
    let Some(href) = attrs.get("href") else {
        return false;
    };
    let Some(base) = crate::dom::url::parse(document_url, None) else {
        return false;
    };
    let Some(link) = crate::dom::url::parse(href, Some(&base)) else {
        return false;
    };
    link.origin() == base.origin() && link.pathname() == base.pathname() && link.query == base.query
}

/// Check if any descendant of `node` matches `sel`.
/// Does anything in `node`'s subtree satisfy the relative selector `sel`?
///
/// ⛔ A `:has()` argument is a RELATIVE selector (Selectors 4 §4.5): a leading
/// combinator relates to the ANCHOR — the element `:has()` is written on — not
/// to some ancestor of it. `:has(> em)` asks for a DIRECT CHILD that is an
/// `<em>`, and this matched the argument against every descendant with an
/// empty ancestor list, so the leading `>` had nothing to relate to and the
/// whole selector never matched.
fn has_descendant_matching(
    node: &crate::types::WebCore,
    sel: &CssSelector,
    focused_box: u32,
) -> bool {
    // A leading child combinator restricts the search to the anchor's own
    // children; a leading descendant combinator, or none, searches the subtree.
    if let Some(SelectorPart::Combinator(c)) = sel.parts.first() {
        match c {
            Combinator::Child => {
                let rest = &sel.parts[1..];
                let empty_hover = std::collections::HashSet::new();
                return node
                    .children
                    .iter()
                    .filter(|c| c.is_element())
                    .any(|child| {
                        let ctx = MatchContext {
                            focused_box,
                            keyboard_focus: false,
                            type_child_index: 0,
                            type_sibling_count: 1,
                            html_box: Some(child),
                            hover_chain: &empty_hover,
                            element_id: child.node_id,
                            scope_root_id: 0,
                            target_id: 0,
                            document_url: "",
                            prev_siblings: &[],
                            next_siblings: &[],
                            next_sibling_nodes: &[],
                        };
                        matches_selector_with_ancestors(
                            rest,
                            &child.tag,
                            &child.attributes,
                            0,
                            1,
                            &[],
                            &ctx,
                        )
                    });
            }
            // ⛔ A leading `+`/`~` relates to the anchor's SIBLINGS, which are
            // not reachable from here — this function only sees the subtree.
            // Not supported; it answers false rather than pretending.
            Combinator::AdjacentSibling | Combinator::GeneralSibling | Combinator::Column => {
                return false
            }
            Combinator::Descendant => {
                let mut stripped = sel.clone();
                stripped.parts.remove(0);
                return has_descendant_matching(node, &stripped, focused_box);
            }
        }
    }
    let empty_hover = std::collections::HashSet::new();
    for child in &node.children {
        let ctx = MatchContext {
            focused_box,
            keyboard_focus: false,
            type_child_index: 0,
            type_sibling_count: 1,
            html_box: Some(child),
            hover_chain: &empty_hover,
            element_id: child.node_id,
            scope_root_id: 0,
            target_id: 0,
            document_url: "",
            prev_siblings: &[],
            next_siblings: &[],
            next_sibling_nodes: &[],
        };
        if matches_selector_with_ancestors(
            &sel.parts,
            &child.tag,
            &child.attributes,
            0,
            1,
            &[],
            &ctx,
        ) {
            return true;
        }
        if has_descendant_matching(child, sel, focused_box) {
            return true;
        }
    }
    false
}

fn matches_element_or_descendant(
    elem: &crate::types::WebCore,
    parts: &[SelectorPart],
    ancestors: &mut Vec<AncestorInfo>,
    ctx: &MatchContext,
) -> bool {
    let empty_hover = std::collections::HashSet::new();
    let elem_ctx = MatchContext {
        focused_box: ctx.focused_box,
        keyboard_focus: false,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: Some(elem),
        hover_chain: &empty_hover,
        element_id: elem.node_id,
        scope_root_id: 0,
        target_id: 0,
        document_url: "",
        prev_siblings: &[],
        next_siblings: &[],
        next_sibling_nodes: &[],
    };
    if matches_selector_with_ancestors(
        parts,
        &elem.tag,
        &elem.attributes,
        0,
        1,
        ancestors,
        &elem_ctx,
    ) {
        return true;
    }
    ancestors.push(AncestorInfo {
        tag: elem.tag.clone(),
        attributes: elem.attributes.clone(),
        child_index: 0,
        sibling_count: 1,
        type_child_index: 0,
        type_sibling_count: 1,
        node_id: elem.node_id,
    });
    for child in elem.children.iter().filter(|c| c.is_element()) {
        if matches_element_or_descendant(child, parts, ancestors, ctx) {
            ancestors.pop();
            return true;
        }
    }
    ancestors.pop();
    false
}

fn has_relative_matching(
    node: &crate::types::WebCore,
    sel: &CssSelector,
    ctx: &MatchContext,
) -> bool {
    if let Some(SelectorPart::Combinator(c)) = sel.parts.first() {
        match c {
            Combinator::AdjacentSibling => {
                let rest = &sel.parts[1..];
                if let Some(sibling) = ctx.next_sibling_nodes.iter().find(|s| s.is_element()) {
                    let mut ancestors = Vec::new();
                    return matches_element_or_descendant(sibling, rest, &mut ancestors, ctx);
                }
                return false;
            }
            Combinator::GeneralSibling => {
                let rest = &sel.parts[1..];
                return ctx
                    .next_sibling_nodes
                    .iter()
                    .filter(|s| s.is_element())
                    .any(|sibling| {
                        let mut ancestors = Vec::new();
                        matches_element_or_descendant(sibling, rest, &mut ancestors, ctx)
                    });
            }
            Combinator::Child => {
                let rest = &sel.parts[1..];
                let empty_hover = std::collections::HashSet::new();
                return node
                    .children
                    .iter()
                    .filter(|c| c.is_element())
                    .any(|child| {
                        let sub_ctx = MatchContext {
                            focused_box: ctx.focused_box,
                            keyboard_focus: false,
                            type_child_index: 0,
                            type_sibling_count: 1,
                            html_box: Some(child),
                            hover_chain: &empty_hover,
                            element_id: child.node_id,
                            scope_root_id: 0,
                            target_id: 0,
                            document_url: "",
                            prev_siblings: &[],
                            next_siblings: &[],
                            next_sibling_nodes: &[],
                        };
                        matches_selector_with_ancestors(
                            rest,
                            &child.tag,
                            &child.attributes,
                            0,
                            1,
                            &[],
                            &sub_ctx,
                        )
                    });
            }
            Combinator::Column => return false,
            Combinator::Descendant => {
                let mut stripped = sel.clone();
                stripped.parts.remove(0);
                return has_descendant_matching(node, &stripped, ctx.focused_box);
            }
        }
    }
    has_descendant_matching(node, sel, ctx.focused_box)
}

/// Evaluate CSS An+B formula against a 1-based position.
fn nth_matches(expr: &str, pos: usize) -> bool {
    let expr = expr.trim();
    match expr {
        "odd" => pos % 2 == 1,
        "even" => pos % 2 == 0,
        _ => {
            if let Ok(n) = expr.parse::<i32>() {
                return pos as i32 == n;
            }
            let (a, b) = parse_nth_ab(expr);
            if a == 0 {
                return pos as i32 == b;
            }
            let diff = pos as i32 - b;
            if a > 0 {
                diff >= 0 && diff % a == 0
            } else {
                diff <= 0 && diff % a == 0
            }
        }
    }
}

fn parse_nth_ab(expr: &str) -> (i32, i32) {
    if let Some(n_pos) = expr.find('n') {
        let a_str = expr[..n_pos].trim();
        let b_str = expr[n_pos + 1..].trim();
        let a: i32 = match a_str {
            "" | "+" => 1,
            "-" => -1,
            s => s.parse().unwrap_or(0),
        };
        let b: i32 = if b_str.is_empty() {
            0
        } else {
            b_str.parse().unwrap_or(0)
        };
        (a, b)
    } else {
        (0, expr.parse().unwrap_or(0))
    }
}

/// Split `"2 of .pick"` into `("2", ".pick")`. `None` when there is no `of`.
///
/// The keyword is ASCII case-insensitive and must be a whole word, so a
/// selector containing the letters "of" — `.of`, `[data-of]` — is not split.
fn split_nth_of(inner: &str) -> Option<(String, String)> {
    let lower = inner.to_ascii_lowercase();
    let mut at = None;
    let bytes = lower.as_bytes();
    let mut i = 0;
    while let Some(pos) = lower[i..].find("of") {
        let p = i + pos;
        let before_ok = p > 0 && (bytes[p - 1] as char).is_ascii_whitespace();
        let after = p + 2;
        let after_ok = after < bytes.len() && (bytes[after] as char).is_ascii_whitespace();
        if before_ok && after_ok {
            at = Some(p);
            break;
        }
        i = p + 2;
        if i >= lower.len() {
            break;
        }
    }
    let at = at?;
    let nth = inner[..at].trim().to_string();
    let sel = inner[at + 2..].trim().to_string();
    if nth.is_empty() || sel.is_empty() {
        return None;
    }
    Some((nth, sel))
}

/// Does a selector with no combinators match this tag/attrs?
fn simple_matches(sel: &CssSelector, tag: &str, attrs: &crate::dom::attrs::AttrMap) -> bool {
    let id = attrs.get("id").cloned().unwrap_or_default();
    let class = attrs.get("class").cloned().unwrap_or_default();
    simple_matches_raw(sel, tag, &id, &class)
}

/// The same, against the (tag, id, class) triple a sibling is recorded as.
fn simple_matches_raw(sel: &CssSelector, tag: &str, id: &str, class: &str) -> bool {
    sel.parts.iter().all(|p| match p {
        SelectorPart::Universal => true,
        SelectorPart::Tag(t) => t.eq_ignore_ascii_case(tag),
        SelectorPart::Id(i) => i == id,
        SelectorPart::Class(c) => class.split_whitespace().any(|k| k == c),
        // Anything richer than a compound of tag/id/class is not answerable
        // from what a sibling record holds.
        _ => false,
    })
}
