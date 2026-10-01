//! The `Stylesheet` type and its rule index.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

thread_local! {
    static CANDIDATE_SEEN: RefCell<(Vec<u32>, u32)> = const { RefCell::new((Vec::new(), 0)) };
}

fn is_root_variable_selector(selector: &str) -> bool {
    let selector = selector.trim();
    selector == "*"
        || selector.eq_ignore_ascii_case(":root")
        || selector.eq_ignore_ascii_case("html")
}

fn effective_layer_ranks(order: &[String]) -> HashMap<String, u32> {
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen = HashSet::new();
    for name in order {
        let mut parent = String::new();
        for segment in name.split('.') {
            if segment.is_empty() {
                continue;
            }
            let qualified = if parent.is_empty() {
                segment.to_string()
            } else {
                format!("{parent}.{segment}")
            };
            if seen.insert(qualified.clone()) {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(qualified.clone());
            }
            parent = qualified;
        }
    }

    // Postorder places each parent's own declarations after all its sublayers.
    let mut ranks = HashMap::with_capacity(seen.len());
    let mut stack = Vec::new();
    if let Some(roots) = children.get("") {
        stack.extend(roots.iter().rev().map(|name| (name.clone(), false)));
    }
    while let Some((name, visited)) = stack.pop() {
        if visited {
            ranks.insert(name, ranks.len() as u32);
        } else {
            stack.push((name.clone(), true));
            if let Some(nested) = children.get(&name) {
                stack.extend(nested.iter().rev().map(|child| (child.clone(), false)));
            }
        }
    }
    ranks
}

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
    pub(crate) counter_style_viewport: (f32, f32),
    /// Parsed `@keyframes` blocks, keyed by animation name.
    pub keyframes: HashMap<String, Vec<KeyframeStop>>,
    /// Selector index: rule indices bucketed by the key selector's id/class/tag.
    /// Built lazily before cascade; avoids O(rules) scan per element.
    idx_by_id: HashMap<String, Vec<usize>>,
    idx_by_class: HashMap<String, Vec<usize>>,
    idx_by_tag: HashMap<String, Vec<usize>>,
    idx_universal: Vec<usize>, // rules with * or no specific key selector
    idx_rule_flags: Vec<u8>,   // length is the indexed prefix; cleared when indices shift
    idx_dirty: bool,
    /// `@layer` names in declaration order. See `layer_rank`.
    pub layer_order: Vec<String>,
    /// Every declaration, including duplicates: the first matching one fixes
    /// order at the current viewport.
    pub(crate) layer_declarations: Vec<(String, MediaConditions)>,
    layer_viewport: (f32, f32),
    indexed_layer_order: Vec<String>,
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
    /// Ancestor `:has()` selectors need the ancestor's subtree, not only its
    /// compact tag/attribute snapshot, during rule matching.
    pub has_ancestor_has_rules: bool,
    /// True if any rule has :hover on a non-subject selector part (descendant hover rules).
    /// When true, descendants of hover-changed nodes must also be re-cascaded.
    pub has_hover_descendant_rules: bool,
    /// Tracks source-only sheet updates without retaining another copy of CSS text.
    pub(crate) source_count: usize,
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
        self.source_count += 1;
        self.idx_dirty = true;
        self.idx_rule_flags.clear();
        Ok(index)
    }

    /// CSSOM-like `CSSStyleSheet.deleteRule()`.
    pub fn delete_rule(&mut self, index: usize) -> Result<(), String> {
        if index >= self.rules.len() {
            return Err("IndexSizeError".to_string());
        }

        self.rules.remove(index);
        self.idx_dirty = true;
        self.idx_rule_flags.clear();
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
        let before_counters = self.counter_styles.len();
        self.parse_and_add(css);
        for rule in &mut self.rules[before..] {
            rule.specificity = rule.specificity.saturating_add(AUTHOR_ORIGIN_BOOST);
        }
        for rule in &mut self.counter_styles[before_counters..] {
            rule.author_origin = true;
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
        self.variables.extend(fragment.variables);
        self.source_count += fragment.source_count;
        self.font_faces.append(&mut fragment.font_faces);
        self.page_rules.append(&mut fragment.page_rules);
        self.counter_styles.append(&mut fragment.counter_styles);
        self.keyframes.extend(fragment.keyframes);
        self.layer_declarations
            .append(&mut fragment.layer_declarations);
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
        self.parse_and_add_with_base_media_conditions(
            css,
            css_base_url,
            &super::media_query::MediaConditions::default().with_query(link_media),
        );
    }

    pub(crate) fn parse_and_add_with_base_media_conditions(
        &mut self,
        css: &str,
        css_base_url: &str,
        media: &super::media_query::MediaConditions,
    ) {
        if media.is_empty() {
            self.parse_and_add_with_base(css, css_base_url);
        } else {
            let before = self.rules.len();
            let before_counters = self.counter_styles.len();
            let before_layers = self.layer_declarations.len();
            self.parse_and_add_with_base(css, css_base_url);
            for rule in &mut self.rules[before..] {
                rule.media_condition = rule.media_condition.and(media);
            }
            for rule in &mut self.counter_styles[before_counters..] {
                rule.media_condition = rule.media_condition.and(media);
            }
            for (_, condition) in &mut self.layer_declarations[before_layers..] {
                *condition = condition.and(media);
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
        effective_layer_ranks(&self.active_layer_order())
            .get(layer)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn set_layer_viewport(&mut self, vw: f32, vh: f32) {
        self.layer_viewport = (vw, vh);
    }

    fn active_layer_order(&self) -> Vec<String> {
        if self.layer_declarations.is_empty() {
            return self.layer_order.clone();
        }
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut declared = HashSet::new();
        for (name, condition) in &self.layer_declarations {
            declared.insert(name.as_str());
            if condition.matches(self.layer_viewport.0, self.layer_viewport.1)
                && seen.insert(name.as_str())
            {
                order.push(name.clone());
            }
        }
        for name in &self.layer_order {
            if !declared.contains(name.as_str()) && seen.insert(name.as_str()) {
                order.push(name.clone());
            }
        }
        order
    }

    pub fn parse_and_add(&mut self, css: &str) {
        let _profile_css = crate::profile::span(crate::profile::Phase::CssParse);
        // Strip comments once, share the cleaned string across all extractors.
        let cleaned = strip_css_comments(css);
        let cleaned = cleaned.as_str();
        // Extract :root CSS variables. Cross-file resolution is deferred to
        // resolve_variables_for_viewport() which runs after all CSS is loaded.
        extract_root_variables_cleaned(cleaned, &mut self.variables);
        self.source_count += 1;
        // Extract @font-face declarations
        extract_font_faces_cleaned(cleaned, &mut self.font_faces);
        // Preserve @page rules for print/pagination consumers.
        self.page_rules
            .extend(crate::css::parser::extract_page_rules_cleaned(cleaned));
        // Extract @keyframes blocks
        let kf = extract_keyframes_cleaned(cleaned);
        self.keyframes.extend(kf);
        crate::css::parser::reset_declared_layers();
        let (parsed, counter_styles) =
            crate::css::parser::parse_stylesheet_with_counter_styles_cleaned(cleaned);
        self.counter_styles.extend(counter_styles);
        if let Some(rules) = parsed {
            // Pick up the layer order this sheet declared, appending any name
            // we have not seen — a later sheet may add layers but cannot
            // reorder the ones already fixed.
            for name in crate::css::parser::declared_layers() {
                if !self.layer_order.iter().any(|n| *n == name) {
                    self.layer_order.push(name);
                }
            }
            self.layer_declarations
                .extend(crate::css::parser::declared_layer_events());
            for r in rules {
                self.rules.push(r);
            }
            self.idx_dirty = true;
        }
    }

    /// Resolve root custom properties from parsed rules in source order. The
    /// stylesheet parser has already evaluated @supports and retained media
    /// conditions, so arriving fragments do not need to rescan all prior CSS.
    pub fn resolve_variables_for_viewport(&mut self, vw: f32, vh: f32) {
        self.counter_style_viewport = (vw, vh);
        self.layer_viewport = (vw, vh);
        self.variables.clear();
        for important in [false, true] {
            for rule in &self.rules {
                if !matches!(rule.pseudo_element, super::rule::PseudoElement::None)
                    || !rule.container_condition.is_empty()
                    || !rule.scopes.is_empty()
                    || !is_root_variable_selector(&rule.original_selector)
                    || (vw > 0.0 && !rule.media_condition.matches(vw, vh))
                {
                    continue;
                }
                let declarations = if important {
                    &rule.important_declarations
                } else {
                    &rule.declarations
                };
                for (name, value) in declarations {
                    if name.starts_with("--")
                        && (!value.is_empty() || !self.variables.contains_key(name))
                    {
                        self.variables.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        pre_resolve_variables(&mut self.variables);
    }

    pub(crate) fn may_change_root_variables(&self) -> bool {
        !self.variables.is_empty()
            || self.rules.iter().any(|rule| {
                matches!(rule.pseudo_element, super::rule::PseudoElement::None)
                    && rule.container_condition.is_empty()
                    && rule.scopes.is_empty()
                    && is_root_variable_selector(&rule.original_selector)
                    && rule
                        .declarations
                        .iter()
                        .chain(rule.important_declarations.iter())
                        .any(|(name, _)| name.starts_with("--"))
            })
    }

    /// Update the selector index before each cascade pass. Appended rule indices
    /// are stable; insertions and deletions clear the indexed prefix above.
    pub fn rebuild_index(&mut self) {
        let active_layer_order = self.active_layer_order();
        let layer_order_changed = self.indexed_layer_order != active_layer_order;
        if !self.idx_dirty && self.idx_rule_flags.len() == self.rules.len() && !layer_order_changed
        {
            return;
        }
        let layer_ranks = effective_layer_ranks(&active_layer_order);
        if layer_order_changed {
            for rule in &mut self.rules {
                rule.layer_rank = if rule.layer.is_empty() {
                    u32::MAX
                } else {
                    layer_ranks.get(&rule.layer).copied().unwrap_or(0)
                };
            }
            self.indexed_layer_order = active_layer_order;
        }
        let first = if self.idx_rule_flags.len() > self.rules.len() {
            0
        } else {
            self.idx_rule_flags.len()
        };
        if first == 0 {
            self.idx_by_id.clear();
            self.idx_by_class.clear();
            self.idx_by_tag.clear();
            self.idx_universal.clear();
            self.idx_rule_flags.clear();
            self.has_sibling_sensitive_rules = false;
            self.has_ancestor_has_rules = false;
            self.has_hover_descendant_rules = false;
        }
        for i in first..self.rules.len() {
            let rule = &self.rules[i];
            self.idx_rule_flags.push(
                u8::from(rule.selectors.iter().any(selector_is_sibling_sensitive))
                    | (u8::from(rule.selectors.iter().any(|selector| !selector.is_simple)) << 1),
            );
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
            let rank = if self.rules[i].layer.is_empty() {
                u32::MAX
            } else {
                layer_ranks.get(&self.rules[i].layer).copied().unwrap_or(0)
            };
            self.rules[i].layer_rank = rank;
            self.rules[i].compile_declarations();
        }
        let appended = &self.rules[first..];
        self.has_sibling_sensitive_rules |= appended
            .iter()
            .any(|rule| rule.selectors.iter().any(selector_is_sibling_sensitive));

        self.has_ancestor_has_rules |= appended.iter().any(|rule| {
            rule.selectors.iter().any(|selector| {
                selector
                    .parts
                    .iter()
                    .rposition(|part| matches!(part, SelectorPart::Combinator(_)))
                    .is_some_and(|end| selector.parts[..end].iter().any(selector_part_contains_has))
            })
        });

        for rule in appended {
            if !rule.is_hover {
                continue;
            }
            if !matches!(rule.pseudo_element, PseudoElement::None) {
                self.has_hover_descendant_rules = true;
                break;
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
                        if selector_part_has_state(part, "hover") {
                            self.has_hover_descendant_rules = true;
                            break;
                        }
                    }
                }
            }
            if self.has_hover_descendant_rules {
                break;
            }
        }
        self.idx_dirty = false;
    }

    /// Get candidate rule indices for an element with given tag, id, and classes.
    /// Writes into a reusable buffer to avoid per-element allocation.
    pub fn candidate_rules<I, C>(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: I,
        out: &mut Vec<usize>,
    ) where
        I: IntoIterator<Item = C>,
        C: AsRef<str>,
    {
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
            if let Some(indices) = self.idx_by_class.get(cls.as_ref()) {
                out.extend_from_slice(indices);
            }
        }
        // Reuse a generation-stamped scratch table. Large stylesheets otherwise
        // allocate and clear a fresh bitset for every element in every cascade.
        if out.len() > 1 {
            let max_idx = out.iter().copied().max().unwrap_or(0);
            if max_idx < 1_000_000 {
                CANDIDATE_SEEN.with_borrow_mut(|(seen, generation)| {
                    seen.resize(max_idx + 1, 0);
                    *generation = generation.wrapping_add(1);
                    if *generation == 0 {
                        seen.fill(0);
                        *generation = 1;
                    }
                    let mut write = 0;
                    for read in 0..out.len() {
                        let idx = out[read];
                        if seen[idx] != *generation {
                            seen[idx] = *generation;
                            out[write] = idx;
                            write += 1;
                        }
                    }
                    out.truncate(write);
                });
            } else {
                out.sort_unstable();
                out.dedup();
            }
        }
    }

    pub(crate) fn candidate_rules_are_sibling_sensitive(&self, candidates: &[usize]) -> bool {
        candidates.iter().any(|&rule_idx| {
            self.idx_rule_flags
                .get(rule_idx)
                .is_some_and(|flags| flags & 1 != 0)
        })
    }

    pub(crate) fn candidate_rules_need_selector_context(&self, candidates: &[usize]) -> bool {
        candidates.iter().any(|&rule_idx| {
            self.idx_rule_flags
                .get(rule_idx)
                .is_some_and(|flags| flags & 2 != 0)
        })
    }
}

fn selector_is_sibling_sensitive(sel: &CssSelector) -> bool {
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
}

pub(crate) fn selector_part_contains_has(part: &SelectorPart) -> bool {
    match part {
        SelectorPart::Has(_) => true,
        SelectorPart::Not(selector) => selector.parts.iter().any(selector_part_contains_has),
        SelectorPart::Is(selectors) | SelectorPart::Where(selectors) => selectors
            .iter()
            .any(|selector| selector.parts.iter().any(selector_part_contains_has)),
        _ => false,
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

#[cfg(test)]
mod index_tests {
    use super::Stylesheet;

    #[test]
    fn class_token_iterator_matches_slice_candidates() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author(
            "div { color: red; } .one { color: blue; } .two { color: green; } \
             #target { color: black; } div.one { color: white; }",
        );
        sheet.rebuild_index();

        let mut from_slice = Vec::new();
        let mut from_iterator = Vec::new();
        sheet.candidate_rules(
            "div",
            Some("target"),
            &["one", "two", "one"],
            &mut from_slice,
        );
        sheet.candidate_rules(
            "div",
            Some("target"),
            "one two one".split_whitespace(),
            &mut from_iterator,
        );
        assert_eq!(from_iterator, from_slice);
    }

    #[test]
    fn streamed_append_indexes_only_new_rules_and_updates_selector_flags() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author(".base { color: red; }");
        sheet.rebuild_index();
        sheet.idx_by_class.get_mut("base").unwrap().reserve(32);
        let old_capacity = sheet.idx_by_class["base"].capacity();

        let mut fragment = Stylesheet::default();
        fragment.parse_and_add_author(
            ".new { color: blue; } .item + .item { color: green; } \
             .host:has(.child) .target { color: purple; } \
             .menu:hover::before { content: 'x'; }",
        );
        sheet.append_fragment(fragment);
        sheet.rebuild_index();

        assert_eq!(sheet.idx_by_class["base"].capacity(), old_capacity);
        assert_eq!(sheet.idx_rule_flags.len(), sheet.rules.len());
        assert!(sheet.has_sibling_sensitive_rules);
        assert!(sheet.has_ancestor_has_rules);
        assert!(sheet.has_hover_descendant_rules);
        let mut candidates = Vec::new();
        sheet.candidate_rules("div", None, &["base"], &mut candidates);
        assert_eq!(candidates.len(), 1);
        sheet.candidate_rules("div", None, &["new"], &mut candidates);
        assert_eq!(candidates.len(), 1);
    }

    #[test]
    fn cssom_insertion_and_deletion_rebuild_shifted_rule_indices() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author(
            ".old { color: red; } .tail { color: blue; } .item + .item { color: green; }",
        );
        sheet.rebuild_index();
        assert!(sheet.has_sibling_sensitive_rules);
        sheet.delete_rule(2).unwrap();
        sheet.delete_rule(0).unwrap();
        sheet
            .insert_author_rule(".head { color: green; }", 0)
            .unwrap();
        sheet.rebuild_index();
        assert!(!sheet.has_sibling_sensitive_rules);

        let mut candidates = Vec::new();
        sheet.candidate_rules("div", None, &["old"], &mut candidates);
        assert!(candidates.is_empty());
        sheet.candidate_rules("div", None, &["head"], &mut candidates);
        assert_eq!(candidates, [0]);
        sheet.candidate_rules("div", None, &["tail"], &mut candidates);
        assert_eq!(candidates, [1]);
    }

    #[test]
    fn streamed_sublayer_declaration_reranks_existing_parent_rule() {
        let mut sheet = Stylesheet::default();
        sheet.parse_and_add_author("@layer parent { #target { color: blue; } }");
        sheet.rebuild_index();
        assert_eq!(sheet.rules[0].layer_rank, 0);

        let mut fragment = Stylesheet::default();
        fragment.parse_and_add_author("@layer parent.child;");
        assert!(fragment.rules.is_empty());
        sheet.append_fragment(fragment);
        sheet.rebuild_index();

        assert_eq!(sheet.layer_rank("parent.child"), 0);
        assert_eq!(sheet.rules[0].layer_rank, 1);
    }

    #[test]
    fn linked_media_condition_controls_streamed_layer_order() {
        let mut sheet = Stylesheet::default();
        let mut linked = Stylesheet::default();
        let media = crate::css::MediaConditions::default().with_query("(min-width: 700px)");
        linked.parse_and_add_with_base_media_conditions(
            "@layer layout;",
            "https://example.test/layout.css",
            &media,
        );
        sheet.append_fragment(linked);
        sheet.parse_and_add_author("@layer theme, layout;");

        for (width, first) in [(600.0, "theme"), (800.0, "layout"), (600.0, "theme")] {
            sheet.set_layer_viewport(width, 600.0);
            sheet.rebuild_index();
            assert_eq!(sheet.layer_rank(first), 0, "{width}");
        }
    }
}
