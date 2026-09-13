//! The `Stylesheet` type and its rule index.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Stylesheet ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct Stylesheet {
    pub rules: Vec<CssRule>,
    pub variables: HashMap<String, String>, // CSS custom properties from :root
    pub font_faces: Vec<FontFaceDecl>,
    /// Parsed `@page` rules. Screen layout ignores them; print/pagination can
    /// consume the preserved descriptors without reparsing raw CSS.
    pub page_rules: Vec<PageRule>,
    /// Parsed `@counter-style` rules, preserved for custom list marker support.
    pub counter_styles: Vec<CounterStyleRule>,
    /// Parsed `@keyframes` blocks, keyed by animation name.
    pub keyframes: HashMap<String, Vec<KeyframeStop>>,
    /// Selector index: rule indices bucketed by the key selector's id/class/tag.
    /// Built lazily before cascade; avoids O(rules) scan per element.
    idx_by_id: HashMap<String, Vec<usize>>,
    idx_by_class: HashMap<String, Vec<usize>>,
    idx_by_tag: HashMap<String, Vec<usize>>,
    idx_universal: Vec<usize>, // rules with * or no specific key selector
    idx_dirty: bool,
    /// `@layer` names in declaration order. See `layer_rank`.
    pub layer_order: Vec<String>,
    /// When true, the cascade stores matched CSS rules on each WebCore
    /// for inspector display. Off by default to avoid memory overhead.
    pub inspect_mode: bool,
    /// True if any selector can tell two same-`(tag, class)` siblings apart —
    /// a sibling combinator (`+`, `~`) or a positional pseudo-class
    /// (`:nth-child`, `:first-child`, …).
    ///
    /// ⛔ This is the CORRECTNESS BOUNDARY for style sharing. The share key is
    /// `(tag, class)`, which says nothing about WHERE among its siblings an
    /// element sits — so with `i + i { color: red }` the second `<i>` was
    /// handed the first one's style and the rule was silently dropped. Sharing
    /// is off for a sheet that can make the distinction.
    pub has_sibling_sensitive_rules: bool,
    /// True if any rule has :hover on a non-subject selector part (descendant hover rules).
    /// When true, descendants of hover-changed nodes must also be re-cascaded.
    pub has_hover_descendant_rules: bool,
    /// Raw (comment-stripped) CSS sources, kept for re-extracting variables with viewport.
    pub raw_sources: Vec<String>,
}

impl Stylesheet {
    pub fn add_rule(&mut self, rule: CssRule) {
        self.rules.push(rule);
        self.idx_dirty = true;
    }

    /// CSSOM-like `CSSStyleSheet.insertRule()`: parse exactly one rule and
    /// insert it at the requested rule-list index.
    pub fn insert_rule(&mut self, css_text: &str, index: usize) -> Result<usize, String> {
        self.insert_rule_with_origin(css_text, index, false)
    }

    /// CSSOM-like `CSSStyleSheet.insertRule()` for author-origin document sheets.
    pub fn insert_author_rule(&mut self, css_text: &str, index: usize) -> Result<usize, String> {
        self.insert_rule_with_origin(css_text, index, true)
    }

    fn insert_rule_with_origin(
        &mut self,
        css_text: &str,
        index: usize,
        author_origin: bool,
    ) -> Result<usize, String> {
        if index > self.rules.len() {
            return Err("IndexSizeError".to_string());
        }

        let cleaned = strip_css_comments(css_text);
        let Some(mut parsed) = parse_stylesheet_cleaned(&cleaned) else {
            return Err("SyntaxError".to_string());
        };
        if parsed.len() != 1 {
            return Err("SyntaxError".to_string());
        }

        let mut rule = parsed.remove(0);
        if author_origin {
            rule.specificity = rule.specificity.saturating_add(AUTHOR_ORIGIN_BOOST);
        }
        self.rules.insert(index, rule);
        self.raw_sources.push(cleaned);
        self.idx_dirty = true;
        Ok(index)
    }

    /// CSSOM-like `CSSStyleSheet.deleteRule()`.
    pub fn delete_rule(&mut self, index: usize) -> Result<(), String> {
        if index >= self.rules.len() {
            return Err("IndexSizeError".to_string());
        }

        self.rules.remove(index);
        self.idx_dirty = true;
        Ok(())
    }

    /// CSSOM-like `cssRules`, serialized as `CSSRule.cssText`.
    pub fn css_rules(&self) -> Vec<String> {
        self.rules
            .iter()
            .map(crate::html::serialize_rule)
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Parse a CSS string and append its rules. Also extracts CSS variables from `:root`.
    /// `css_base_url` is the URL of the CSS file itself, used to resolve relative url()
    /// references (e.g. `url('../image.jpg')` in an external stylesheet).
    /// Parse `css` as **author-origin** CSS and append it.
    ///
    /// The difference from `parse_and_add` is the origin: rules added this way
    /// carry [`AUTHOR_ORIGIN_BOOST`], so they outrank the UA sheet the way an
    /// author sheet must. `parse_and_add` builds the UA sheet itself and any
    /// caller that seeds a stylesheet with `ua_stylesheet()` and then adds page
    /// CSS wants THIS — otherwise a shadow root's own `<style>` loses to
    /// `input { width: 200px }` on specificity alone.
    pub fn parse_and_add_author(&mut self, css: &str) {
        let before = self.rules.len();
        self.parse_and_add(css);
        for rule in &mut self.rules[before..] {
            rule.specificity = rule.specificity.saturating_add(AUTHOR_ORIGIN_BOOST);
        }
    }

    /// Append already-parsed rules as author-origin.
    pub fn push_author_rules(&mut self, rules: impl IntoIterator<Item = CssRule>) {
        let before = self.rules.len();
        for mut rule in rules {
            rule.specificity = rule.specificity.saturating_add(AUTHOR_ORIGIN_BOOST);
            self.rules.push(rule);
        }
        if self.rules.len() != before {
            self.idx_dirty = true;
        }
    }

    /// Append an already-parsed stylesheet fragment. Resource workers use this
    /// to hand the document a parsed CSS handle instead of forcing the UI frame
    /// to parse a full external stylesheet synchronously.
    pub fn append_fragment(&mut self, mut fragment: Stylesheet) {
        let before = self.rules.len();
        self.raw_sources.append(&mut fragment.raw_sources);
        self.font_faces.append(&mut fragment.font_faces);
        self.page_rules.append(&mut fragment.page_rules);
        self.counter_styles.append(&mut fragment.counter_styles);
        self.keyframes.extend(fragment.keyframes);
        for name in fragment.layer_order {
            if !self.layer_order.iter().any(|n| *n == name) {
                self.layer_order.push(name);
            }
        }
        self.rules.append(&mut fragment.rules);
        if self.rules.len() != before {
            self.idx_dirty = true;
        }
    }

    /// Parse an EXTERNAL stylesheet — a `<link rel=stylesheet>`.
    ///
    /// ⛔ Author origin, like every other author sheet. This routed to
    /// `parse_and_add`, the same entry the UA sheet uses, so a linked sheet's
    /// rules kept their raw specificity while an inline `<style>` got
    /// `AUTHOR_ORIGIN_BOOST` on the way into the document. A `<style>` rule
    /// therefore beat a more specific rule from a linked sheet, and a UA rule
    /// could beat one outright — on a site whose CSS is mostly external, which
    /// is most sites, the cascade came out wrong.
    pub fn parse_and_add_with_base(&mut self, css: &str, css_base_url: &str) {
        let resolved = resolve_css_urls(css, css_base_url);
        self.parse_and_add_author(&resolved);
    }

    /// Parse a CSS string from an external `<link>` with a `media` attribute.
    /// When `link_media` is non-empty (e.g. "print"), all parsed rules inherit
    /// that media condition so they're only applied in the matching context.
    pub fn parse_and_add_with_base_media(
        &mut self,
        css: &str,
        css_base_url: &str,
        link_media: &str,
    ) {
        if link_media.is_empty()
            || link_media.eq_ignore_ascii_case("all")
            || link_media.eq_ignore_ascii_case("screen")
        {
            self.parse_and_add_with_base(css, css_base_url);
        } else {
            let before = self.rules.len();
            self.parse_and_add_with_base(css, css_base_url);
            // Tag all newly added rules with the link's media condition
            for rule in &mut self.rules[before..] {
                if rule.media_condition.is_empty() {
                    rule.media_condition = link_media.to_string();
                }
            }
        }
    }

    /// Parse a CSS string and append its rules. Also extracts CSS variables from `:root`.
    /// Where a rule's layer sorts. Later layers beat earlier ones, and an
    /// UNLAYERED normal declaration beats every layered one — CSS Cascade 5
    /// §6.4.4 — so unlayered takes the maximum rank.
    pub fn layer_rank(&self, layer: &str) -> u32 {
        if layer.is_empty() {
            return u32::MAX;
        }
        self.layer_order
            .iter()
            .position(|n| n == layer)
            .map(|i| i as u32)
            .unwrap_or(0)
    }

    pub fn parse_and_add(&mut self, css: &str) {
        // Strip comments once, share the cleaned string across all extractors.
        let cleaned = strip_css_comments(css);
        let cleaned = cleaned.as_str();
        // Extract :root CSS variables. Cross-file resolution is deferred to
        // resolve_variables_for_viewport() which runs after all CSS is loaded.
        extract_root_variables_cleaned(cleaned, &mut self.variables);
        self.raw_sources.push(cleaned.to_string());
        // Extract @font-face declarations
        extract_font_faces_cleaned(cleaned, &mut self.font_faces);
        // Preserve @page rules for print/pagination consumers.
        self.page_rules
            .extend(crate::css::parser::extract_page_rules_cleaned(cleaned));
        self.counter_styles
            .extend(crate::css::parser::extract_counter_style_rules_cleaned(
                cleaned,
            ));
        // Extract @keyframes blocks
        let kf = extract_keyframes_cleaned(cleaned);
        self.keyframes.extend(kf);
        crate::css::parser::reset_declared_layers();
        if let Some(rules) = parse_stylesheet_cleaned(cleaned) {
            // Pick up the layer order this sheet declared, appending any name
            // we have not seen — a later sheet may add layers but cannot
            // reorder the ones already fixed.
            for name in crate::css::parser::declared_layers() {
                if !self.layer_order.iter().any(|n| *n == name) {
                    self.layer_order.push(name);
                }
            }
            for r in rules {
                self.rules.push(r);
            }
            self.idx_dirty = true;
        }
    }

    /// Re-extract CSS variables from `:root` with viewport-aware media queries.
    /// Call this before cascade when viewport dimensions are known, so variables
    /// inside `@media` blocks are only extracted when the query matches.
    pub fn resolve_variables_for_viewport(&mut self, vw: f32, vh: f32) {
        self.variables.clear();
        for src in &self.raw_sources {
            extract_root_variables_vp(src, &mut self.variables, vw, vh);
        }
        pre_resolve_variables(&mut self.variables);
    }

    /// Rebuild the selector index if dirty.  Called once before each cascade pass.
    pub fn rebuild_index(&mut self) {
        if !self.idx_dirty {
            return;
        }
        self.idx_by_id.clear();
        self.idx_by_class.clear();
        self.idx_by_tag.clear();
        self.idx_universal.clear();
        for (i, rule) in self.rules.iter().enumerate() {
            let keys = rule_key_selectors(rule);
            if keys.is_empty() {
                self.idx_universal.push(i);
            } else {
                for key in keys {
                    match key {
                        SelectorKey::Id(s) => self.idx_by_id.entry(s).or_default().push(i),
                        SelectorKey::Class(s) => self.idx_by_class.entry(s).or_default().push(i),
                        SelectorKey::Tag(s) => self.idx_by_tag.entry(s).or_default().push(i),
                        SelectorKey::Universal => self.idx_universal.push(i),
                    }
                }
            }
        }
        // Resolve every rule's layer to its sort rank once, here, rather than
        // looking the name up per comparison in the cascade.
        for i in 0..self.rules.len() {
            let rank = self.layer_rank(&self.rules[i].layer);
            self.rules[i].layer_rank = rank;
        }
        self.idx_dirty = false;

        // Pre-compile declarations (string → PropertyId) for fast cascade dispatch
        for rule in &mut self.rules {
            rule.compile_declarations();
        }

        // Detect if any rule has :hover on a non-subject part (descendant hover selectors).
        // e.g., ".parent:hover .child" — :hover is on .parent (ancestor), not .child (subject).
        // Can any selector distinguish two siblings that share `(tag, class)`?
        self.has_sibling_sensitive_rules = self.rules.iter().any(|rule| {
            rule.selectors.iter().any(|sel| {
                sel.parts.iter().any(|part| match part {
                    SelectorPart::Combinator(c) => {
                        matches!(c, Combinator::AdjacentSibling | Combinator::GeneralSibling)
                    }
                    SelectorPart::PseudoClass(pc) => {
                        let name = pc.split('(').next().unwrap_or(pc);
                        matches!(
                            name,
                            "first-child"
                                | "last-child"
                                | "only-child"
                                | "first-of-type"
                                | "last-of-type"
                                | "only-of-type"
                                | "nth-child"
                                | "nth-last-child"
                                | "nth-of-type"
                                | "nth-last-of-type"
                        )
                    }
                    _ => false,
                })
            })
        });

        self.has_hover_descendant_rules = false;
        'rules: for rule in &self.rules {
            if !rule.is_hover {
                continue;
            }
            if !matches!(rule.pseudo_element, PseudoElement::None) {
                self.has_hover_descendant_rules = true;
                break 'rules;
            }
            for sel in &rule.selectors {
                // Find the last combinator — everything before it is ancestor context
                let last_comb = sel
                    .parts
                    .iter()
                    .rposition(|p| matches!(p, SelectorPart::Combinator(_)));
                if let Some(pos) = last_comb {
                    // Check if :hover appears in the ancestor part (before the combinator)
                    for part in &sel.parts[..pos] {
                        if matches!(part, SelectorPart::PseudoClass(pc) if pc == "hover") {
                            self.has_hover_descendant_rules = true;
                            break 'rules;
                        }
                    }
                }
            }
        }
    }

    /// Get candidate rule indices for an element with given tag, id, and classes.
    /// Writes into a reusable buffer to avoid per-element allocation.
    pub fn candidate_rules(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: &[&str],
        out: &mut Vec<usize>,
    ) {
        out.clear();
        // Add universal rules (always candidates)
        out.extend_from_slice(&self.idx_universal);
        // Add tag-matched rules
        // HTML tags are already lowercase from the parser; this handles edge cases.
        let mut tag_buf = [0u8; 32];
        let tag_key: &str = if tag.len() <= 32 && tag.bytes().any(|b| b.is_ascii_uppercase()) {
            let len = tag.len().min(32);
            tag_buf[..len].copy_from_slice(&tag.as_bytes()[..len]);
            tag_buf[..len].make_ascii_lowercase();
            std::str::from_utf8(&tag_buf[..len]).unwrap_or(tag)
        } else {
            tag // already lowercase or too long (rare)
        };
        if let Some(indices) = self.idx_by_tag.get(tag_key) {
            out.extend_from_slice(indices);
        }
        // Add id-matched rules
        if let Some(id) = id {
            if let Some(indices) = self.idx_by_id.get(id) {
                out.extend_from_slice(indices);
            }
        }
        // Add class-matched rules
        for cls in classes {
            if let Some(indices) = self.idx_by_class.get(*cls) {
                out.extend_from_slice(indices);
            }
        }
        // Use a fast dedup via a seen-bitset instead of sort+dedup.
        // For typical pages (< 10k rules), a bitset is much faster than sorting.
        if out.len() > 1 {
            let max_idx = out.iter().copied().max().unwrap_or(0);
            if max_idx < 65536 {
                // Fast path: bitvec dedup
                let words = (max_idx / 64) + 1;
                let mut seen = vec![0u64; words];
                let mut write = 0;
                for read in 0..out.len() {
                    let idx = out[read];
                    let word = idx / 64;
                    let bit = 1u64 << (idx % 64);
                    if seen[word] & bit == 0 {
                        seen[word] |= bit;
                        out[write] = idx;
                        write += 1;
                    }
                }
                out.truncate(write);
            } else {
                out.sort_unstable();
                out.dedup();
            }
        }
    }
}

/// Key extracted from the rightmost simple selector of a rule.
enum SelectorKey {
    Id(String),
    Class(String),
    Tag(String),
    Universal,
}

/// Extract key selectors from ALL selectors in a rule (handles comma-separated selectors).
/// Each selector produces one key (id > class > tag > universal).
fn rule_key_selectors(rule: &CssRule) -> Vec<SelectorKey> {
    let mut keys = Vec::new();
    for sel in &rule.selectors {
        // Walk from right to left, skip combinators, find the rightmost id/class/tag.
        let mut best_id = None;
        let mut best_class = None;
        let mut best_tag = None;
        for part in sel.parts.iter().rev() {
            match part {
                SelectorPart::Combinator(_) => break, // stop at first combinator from right
                SelectorPart::Id(s) => {
                    best_id = Some(s.clone());
                    break;
                }
                SelectorPart::Class(s) => {
                    if best_class.is_none() {
                        best_class = Some(s.clone());
                    }
                }
                SelectorPart::Tag(t) if t != "*" => {
                    if best_tag.is_none() {
                        best_tag = Some(t.to_ascii_lowercase());
                    }
                }
                _ => {}
            }
        }
        let key = if let Some(id) = best_id {
            SelectorKey::Id(id)
        } else if let Some(cls) = best_class {
            SelectorKey::Class(cls)
        } else if let Some(tag) = best_tag {
            SelectorKey::Tag(tag)
        } else {
            SelectorKey::Universal
        };
        keys.push(key);
    }
    keys
}
