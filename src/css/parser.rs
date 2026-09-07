//! The CSS parser — tokenizing a stylesheet into rules.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Parser ──────────────────────────────────────────────────────────────

/// Parse a full stylesheet text into rules.
/// `parent_media` is non-empty when called recursively from inside an @media block.
pub fn parse_stylesheet(css: &str) -> Option<Vec<CssRule>> {
    let cleaned = strip_css_comments(css);
    parse_stylesheet_cleaned(&cleaned)
}

pub(crate) fn parse_stylesheet_cleaned(css: &str) -> Option<Vec<CssRule>> {
    parse_stylesheet_inner(css, "", "")
}

pub(crate) fn extract_page_rules_cleaned(css: &str) -> Vec<PageRule> {
    let mut rules = Vec::new();
    let mut s = css;

    while let Some(at) = s.to_ascii_lowercase().find("@page") {
        s = &s[at + "@page".len()..];
        let Some(brace) = s.find('{') else {
            break;
        };
        let selector = s[..brace].trim().to_string();
        let (block, after) = consume_block(&s[brace..]);
        let (declarations, important_declarations, margin_rules) = parse_page_block(block);
        if !declarations.is_empty()
            || !important_declarations.is_empty()
            || !margin_rules.is_empty()
        {
            rules.push(PageRule {
                selector,
                declarations,
                important_declarations,
                margin_rules,
            });
        }
        s = after;
    }

    rules
}

fn parse_page_block(block: &str) -> (Declarations, Declarations, Vec<PageMarginRule>) {
    let mut declarations_css = String::new();
    let mut margin_rules = Vec::new();
    let bytes = block.as_bytes();
    let mut quote: Option<u8> = None;
    let mut depth = 0usize;
    let mut i = 0usize;
    let mut last = 0usize;

    while i < bytes.len() {
        match quote {
            Some(q) => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            None => match bytes[i] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'"' | b'\'' => quote = Some(bytes[i]),
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth = depth.saturating_sub(1),
                b'@' if depth == 0 => {
                    declarations_css.push_str(&block[last..i]);
                    let name_start = i + 1;
                    let mut name_end = name_start;
                    while name_end < bytes.len()
                        && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'-')
                    {
                        name_end += 1;
                    }
                    let name = &block[name_start..name_end];
                    let mut brace = name_end;
                    while brace < bytes.len() && bytes[brace].is_ascii_whitespace() {
                        brace += 1;
                    }
                    if brace < bytes.len() && bytes[brace] == b'{' {
                        let nested = &block[brace..];
                        let (nested_block, after) = consume_block(nested);
                        if is_page_margin_at_rule(name) {
                            let (declarations, important_declarations) =
                                parse_declarations_important(nested_block);
                            margin_rules.push(PageMarginRule {
                                name: name.to_ascii_lowercase(),
                                declarations,
                                important_declarations,
                            });
                        }
                        i = block.len() - after.len();
                        last = i;
                        continue;
                    } else if let Some(semi_rel) = block[i..].find(';') {
                        i += semi_rel + 1;
                        last = i;
                        continue;
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    declarations_css.push_str(&block[last..]);

    let (declarations, important_declarations) = parse_declarations_important(&declarations_css);
    (declarations, important_declarations, margin_rules)
}

fn is_page_margin_at_rule(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "top-left-corner"
            | "top-left"
            | "top-center"
            | "top-right"
            | "top-right-corner"
            | "bottom-left-corner"
            | "bottom-left"
            | "bottom-center"
            | "bottom-right"
            | "bottom-right-corner"
            | "left-top"
            | "left-middle"
            | "left-bottom"
            | "right-top"
            | "right-middle"
            | "right-bottom"
    )
}

pub(crate) fn extract_counter_style_rules_cleaned(css: &str) -> Vec<CounterStyleRule> {
    let mut rules = Vec::new();
    let mut s = css;

    while let Some(at) = s.to_ascii_lowercase().find("@counter-style") {
        s = &s[at + "@counter-style".len()..];
        let Some(brace) = s.find('{') else {
            break;
        };
        let name = s[..brace].trim().to_string();
        let (block, after) = consume_block(&s[brace..]);
        let (declarations, important_declarations) = parse_declarations_important(block);
        if !name.is_empty() && (!declarations.is_empty() || !important_declarations.is_empty()) {
            rules.push(CounterStyleRule {
                name,
                declarations,
                important_declarations,
            });
        }
        s = after;
    }

    rules
}

pub(crate) fn supports_condition_matches(condition: &str) -> bool {
    let cond = condition.trim();
    if cond.is_empty() {
        return false;
    }

    if let Some(idx) = find_keyword_outside_parens(cond, " or ") {
        return supports_condition_matches(&cond[..idx])
            || supports_condition_matches(&cond[idx + 4..]);
    }
    if let Some(idx) = find_keyword_outside_parens(cond, " and ") {
        return supports_condition_matches(&cond[..idx])
            && supports_condition_matches(&cond[idx + 5..]);
    }
    if starts_with_keyword_ci(cond, "not") {
        let rest = cond.get(3..).unwrap_or("").trim_start();
        return !supports_condition_matches(rest);
    }

    let inner = strip_enclosing_condition_parens(cond);

    if starts_with_keyword_ci(inner, "not")
        || find_keyword_outside_parens(inner, " or ").is_some()
        || find_keyword_outside_parens(inner, " and ").is_some()
        || outer_parens_enclose(inner)
    {
        return supports_condition_matches(inner);
    }

    let inner_lower = inner.to_ascii_lowercase();
    if inner_lower.starts_with("selector(") && inner.ends_with(')') {
        let selector = &inner["selector(".len()..inner.len() - 1];
        let selector = selector.trim();
        if selector.is_empty() {
            return false;
        }
        return split_selectors(selector).iter().all(|sel| {
            let sel = sel.trim();
            !sel.is_empty() && parse_selector(&strip_pseudo_element(sel).0).valid
        });
    }
    if inner_lower.starts_with("font-format(") && inner.ends_with(')') {
        let format = unquote_css_string(inner["font-format(".len()..inner.len() - 1].trim());
        return supports_font_format(format.trim());
    }
    if inner_lower.starts_with("font-tech(") && inner.ends_with(')') {
        let tech = inner["font-tech(".len()..inner.len() - 1].trim();
        return supports_font_tech(tech);
    }

    let Some(colon) = inner.find(':') else {
        return false;
    };
    let prop = inner[..colon].trim().to_ascii_lowercase();
    let value = inner[colon + 1..].trim();

    supports_declaration_matches(&prop, value)
}

fn strip_enclosing_condition_parens(mut condition: &str) -> &str {
    loop {
        let trimmed = condition.trim();
        if trimmed.starts_with('(') && trimmed.ends_with(')') && outer_parens_enclose(trimmed) {
            condition = &trimmed[1..trimmed.len() - 1];
        } else {
            return trimmed;
        }
    }
}

fn outer_parens_enclose(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
        return false;
    }
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        match quote {
            Some(q) => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    quote = None;
                }
            }
            None => match bytes[i] {
                b'"' | b'\'' => quote = Some(bytes[i]),
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && i != bytes.len() - 1 {
                        return false;
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    depth == 0 && quote.is_none()
}

fn supports_declaration_matches(prop: &str, value: &str) -> bool {
    use crate::types::CssValue;

    if value.is_empty() {
        return false;
    }
    if prop.starts_with("--") {
        return true;
    }

    let id = crate::css::properties::resolve(prop);
    if matches!(id, crate::css::properties::PropertyId::Unknown) {
        return false;
    }

    let value = value.trim();
    if matches!(
        value,
        "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    ) {
        return true;
    }
    if value.contains("var(") || !crate::css::property_defs::get(id).longhands.is_empty() {
        return true;
    }

    if matches!(id, crate::css::properties::PropertyId::Content) {
        return supports_content_value(value);
    }

    !matches!(crate::css::pre_parse_value(id, value), CssValue::Raw(_))
}

fn supports_font_format(format: &str) -> bool {
    matches!(
        format.to_ascii_lowercase().as_str(),
        "woff2"
            | "woff"
            | "opentype"
            | "truetype"
            | "embedded-opentype"
            | "collection"
            | "font/woff2"
            | "font/woff"
            | "font/otf"
            | "font/ttf"
    )
}

fn supports_font_tech(tech: &str) -> bool {
    matches!(
        tech.to_ascii_lowercase().as_str(),
        "features-opentype" | "features-aat" | "variations" | "variations-opentype"
    )
}

fn unquote_css_string(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn supports_content_value(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "normal" | "none" | "open-quote" | "close-quote" | "no-open-quote" | "no-close-quote"
    ) {
        return true;
    }

    starts_with_quoted_string(value)
        || lower.starts_with("url(")
        || lower.starts_with("attr(")
        || lower.starts_with("counter(")
        || lower.starts_with("counters(")
}

fn starts_with_quoted_string(value: &str) -> bool {
    let bytes = value.as_bytes();
    let Some(&quote @ (b'"' | b'\'')) = bytes.first() else {
        return false;
    };
    let mut i = 1usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == quote {
            return true;
        }
        i += 1;
    }
    false
}

fn find_keyword_outside_parens(s: &str, needle: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let needle_bytes = needle.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = 0usize;

    while i < bytes.len() {
        match quote {
            Some(q) => {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            None => match bytes[i] {
                b'"' | b'\'' => quote = Some(bytes[i]),
                b'(' => depth += 1,
                b')' => depth = depth.saturating_sub(1),
                _ => {}
            },
        }
        if depth == 0 && starts_with_ci(&bytes[i..], needle_bytes) {
            return Some(i);
        }
        i += 1;
    }

    None
}

fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

fn starts_with_keyword_ci(s: &str, keyword: &str) -> bool {
    let Some(prefix) = s.get(..keyword.len()) else {
        return false;
    };
    let Some(rest) = s.get(keyword.len()..) else {
        return false;
    };
    !rest.is_empty()
        && prefix.eq_ignore_ascii_case(keyword)
        && rest.starts_with(char::is_whitespace)
}

fn parse_stylesheet_inner(
    css: &str,
    parent_media: &str,
    parent_layer: &str,
) -> Option<Vec<CssRule>> {
    let mut rules = Vec::new();
    let mut s = css.trim();

    while !s.is_empty() {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }

        // @rules
        if s.starts_with('@') {
            // Only lowercase a small prefix (enough to identify the @-rule type)
            let prefix_len = s.len().min(30);
            let at_lower: String = s[..prefix_len].to_ascii_lowercase();

            // @import / @charset — skip to semicolon (no block)
            if at_lower.starts_with("@import") || at_lower.starts_with("@charset") {
                if let Some(semi) = s.find(';') {
                    s = &s[semi + 1..];
                } else {
                    break;
                }
                continue;
            }

            // ⛔ `@layer a, b;` — the STATEMENT form, which has no block. It
            // exists purely to fix the order of the layers it names, and was
            // being skipped with every other braceless at-rule, so the order it
            // declared was thrown away and layers fell back to source order.
            if at_lower.starts_with("@layer")
                && s.find('{')
                    .map_or(true, |b| s.find(';').map_or(false, |sc| sc < b))
            {
                if let Some(semi) = s.find(';') {
                    for name in s[6..semi].split(',') {
                        let n = name.trim();
                        if !n.is_empty() {
                            let qualified = qualify_layer_name(parent_layer, n);
                            declare_layer(&qualified);
                        }
                    }
                    s = &s[semi + 1..];
                    continue;
                }
            }

            // Find the opening brace
            let brace = match s.find('{') {
                Some(p) => p,
                None => {
                    if let Some(semi) = s.find(';') {
                        s = &s[semi + 1..];
                    } else {
                        break;
                    }
                    continue;
                }
            };
            let at_header = s[..brace].trim();
            let rest_from_brace = &s[brace..];
            let (inner_block, after_block) = consume_block(rest_from_brace);

            if at_lower.starts_with("@media") {
                // Extract condition: everything after "@media"
                let condition = at_header[6..].trim();
                let media_cond = if parent_media.is_empty() {
                    condition.to_string()
                } else {
                    format!("{} and {}", parent_media, condition)
                };
                // Recursively parse inner block
                if let Some(inner_rules) =
                    parse_stylesheet_inner(inner_block, &media_cond, parent_layer)
                {
                    for r in inner_rules {
                        rules.push(r);
                    }
                }
            } else if at_lower.starts_with("@container") {
                // @container [name] (condition) { ... }
                // Extract optional container name and condition string.
                let header = at_header["@container".len()..].trim();
                let header_lower = header.to_ascii_lowercase();
                let (cname, cond) = if header.starts_with('(')
                    || header_lower.starts_with("style(")
                    || header_lower.starts_with("not ")
                {
                    (String::new(), header.to_string())
                } else if let Some(paren) = header.find('(') {
                    (
                        header[..paren].trim().to_string(),
                        header[paren..].trim().to_string(),
                    )
                } else {
                    (String::new(), header.to_string())
                };
                if let Some(mut inner_rules) =
                    parse_stylesheet_inner(inner_block, parent_media, parent_layer)
                {
                    for r in &mut inner_rules {
                        r.container_condition = cond.clone();
                        r.container_name = cname.clone();
                    }
                    for r in inner_rules {
                        rules.push(r);
                    }
                }
            } else if at_lower.starts_with("@supports") {
                let condition = at_header["@supports".len()..].trim();
                if supports_condition_matches(condition) {
                    if let Some(inner_rules) =
                        parse_stylesheet_inner(inner_block, parent_media, parent_layer)
                    {
                        for r in inner_rules {
                            rules.push(r);
                        }
                    }
                }
            } else if at_lower.starts_with("@scope") {
                if let Some(inner_rules) =
                    parse_stylesheet_inner(inner_block, parent_media, parent_layer)
                {
                    let (scope_selector, scope_limit_selector) = extract_scope_selectors(at_header);
                    for mut r in inner_rules {
                        if r.scope_selector.is_none() {
                            r.scope_selector = scope_selector.clone();
                        }
                        if r.scope_limit_selector.is_none() {
                            r.scope_limit_selector = scope_limit_selector.clone();
                        }
                        rules.push(r);
                    }
                }
            } else if at_lower.starts_with("@layer") {
                // `@layer name { … }` — every rule inside belongs to that layer.
                // Naming a layer here also declares its order, if a preceding
                // `@layer a, b;` statement did not already.
                let mut name = at_header
                    .trim_start_matches(|c: char| c != ' ' && c != '\t')
                    .trim()
                    .to_string();
                if name.is_empty() {
                    name = next_anonymous_layer_name();
                } else {
                    name = qualify_layer_name(parent_layer, &name);
                }
                declare_layer(&name);
                if let Some(inner_rules) = parse_stylesheet_inner(inner_block, parent_media, &name)
                {
                    for mut r in inner_rules {
                        // An inner `@layer` wins — it is the more specific one.
                        if r.layer.is_empty() {
                            r.layer = name.clone();
                        }
                        rules.push(r);
                    }
                }
            }
            // else: @keyframes, @font-face, etc. — skip the block

            s = after_block;
            continue;
        }

        // Selector(s) { declarations }
        let brace_pos = match s.find('{') {
            Some(p) => p,
            None => break,
        };

        let selector_text = s[..brace_pos].trim();
        let (decl_block, rest) = consume_block(&s[brace_pos..]);
        s = rest;

        let (decl_text, nested_rules) = split_nested_style_rules(decl_block);
        let (declarations, important_declarations) = parse_declarations_important(&decl_text);

        // A rule's selector list is NOT forgiving (Selectors §3.1): if any one
        // selector in it is invalid, the entire rule is dropped — `div,
        // p:bogus { color: red }` leaves `div` unstyled too. Checked over the
        // whole list before a single rule is built, because the loop below
        // emits one `CssRule` per comma-separated selector and would otherwise
        // keep the good half.
        let list_is_valid = split_selectors(selector_text).iter().all(|sel_str| {
            let sel_str = sel_str.trim();
            if sel_str.is_empty() {
                return true;
            }
            let (sel_for_match, _) = strip_pseudo_element(sel_str);
            parse_selector(&sel_for_match).valid
        });
        if !list_is_valid {
            continue;
        }

        if !declarations.is_empty() || !important_declarations.is_empty() {
            // Split comma-separated selectors (respecting parentheses)
            for sel_str in split_selectors(selector_text) {
                let sel_str = sel_str.trim();
                if sel_str.is_empty() {
                    continue;
                }

                // :root — extract CSS variables
                if sel_str == ":root" {
                    // Variables are stored on the Stylesheet, not as rules.
                    // We emit a special rule with empty selectors as a marker;
                    // the caller (parse_and_add) handles it.
                    // For now: skip (variables handled by Stylesheet::parse_and_add).
                    continue;
                }

                let original_selector = sel_str.to_string();

                // Detect ::before / ::after pseudo-elements, strip from selector for matching
                let (sel_for_match, pseudo_elem) = strip_pseudo_element(sel_str);

                let sel = parse_selector(&sel_for_match);
                let sp = sel.specificity();

                // Detect :hover in selector parts
                let is_hover = sel
                    .parts
                    .iter()
                    .any(|p| matches!(p, SelectorPart::PseudoClass(name) if name == "hover"));

                let mut rule = CssRule::default();
                rule.selectors = vec![sel];
                rule.declarations = declarations.clone();
                rule.important_declarations = important_declarations.clone();
                rule.specificity = sp;
                rule.media_condition = parent_media.to_string();
                rule.original_selector = original_selector;
                rule.is_hover = is_hover;
                rule.pseudo_element = pseudo_elem;
                rules.push(rule);
            }
        }

        for (nested_selector, nested_block) in nested_rules {
            let nested_css = if nested_selector.trim_start().starts_with('@') {
                format!(
                    "{} {{ {} {{{}}} }}",
                    nested_selector, selector_text, nested_block
                )
            } else {
                let expanded = expand_nested_selectors(selector_text, &nested_selector);
                if expanded.trim().is_empty() {
                    continue;
                }
                format!("{} {{{}}}", expanded, nested_block)
            };
            if let Some(inner_rules) =
                parse_stylesheet_inner(&nested_css, parent_media, parent_layer)
            {
                rules.extend(inner_rules);
            }
        }
    }

    Some(rules)
}

fn split_nested_style_rules(block: &str) -> (String, Vec<(String, String)>) {
    let mut declarations = String::new();
    let mut nested = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_brace) = find_next_top_level_open_brace(&block[cursor..]) {
        let brace = cursor + relative_brace;
        let before = &block[cursor..brace];
        let selector_start = before
            .rfind(';')
            .map(|idx| cursor + idx + 1)
            .unwrap_or(cursor);
        declarations.push_str(&block[cursor..selector_start]);

        let selector = block[selector_start..brace].trim();
        let (nested_block, after) = consume_block(&block[brace..]);
        let after_idx = block.len() - after.len();
        if !selector.is_empty() {
            nested.push((selector.to_string(), nested_block.to_string()));
        }
        cursor = after_idx;
    }
    declarations.push_str(&block[cursor..]);
    (declarations, nested)
}

fn find_next_top_level_open_brace(s: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'"' | b'\'' => quote = Some(b),
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' if paren_depth == 0 && bracket_depth == 0 => return Some(i),
                _ => {}
            },
        }
        i += 1;
    }
    None
}

fn expand_nested_selectors(parent: &str, nested: &str) -> String {
    let mut out = Vec::new();
    for parent_sel in split_selectors(parent) {
        let parent_sel = parent_sel.trim();
        if parent_sel.is_empty() {
            continue;
        }
        for nested_sel in split_selectors(nested) {
            let nested_sel = nested_sel.trim();
            if nested_sel.is_empty() {
                continue;
            }
            if nested_sel.contains('&') {
                out.push(nested_sel.replace('&', parent_sel));
            } else {
                out.push(format!("{} {}", parent_sel, nested_sel));
            }
        }
    }
    out.join(", ")
}

/// Strip `/* ... */` comments from CSS text.
pub(crate) fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let bytes = css.as_bytes();
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Case-insensitive substring search without allocating a lowercased copy.
pub fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle_bytes = needle.as_bytes();
    let nlen = needle_bytes.len();
    if nlen == 0 {
        return Some(0);
    }
    let hbytes = haystack.as_bytes();
    if hbytes.len() < nlen {
        return None;
    }
    'outer: for i in 0..=(hbytes.len() - nlen) {
        for j in 0..nlen {
            if hbytes[i + j].to_ascii_lowercase() != needle_bytes[j].to_ascii_lowercase() {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

/// Detect and strip `::before` / `::after` (and CSS2 `:before`/`:after`) from
/// a selector string.  Returns (cleaned_selector, PseudoElement).
fn strip_pseudo_element(sel: &str) -> (String, PseudoElement) {
    // :: double-colon pseudo-elements
    if let Some(pos) = find_unescaped(sel, "::") {
        let pe_str = sel[pos + 2..].to_ascii_lowercase();
        let (kw_len, pe) = if pe_str.starts_with("before") {
            (6, PseudoElement::Before)
        } else if pe_str.starts_with("after") {
            (5, PseudoElement::After)
        } else if pe_str.starts_with("selection") {
            (9, PseudoElement::Selection)
        } else if pe_str.starts_with("marker") {
            (6, PseudoElement::Marker)
        } else if pe_str.starts_with("first-line") {
            (10, PseudoElement::Ignored)
        } else if pe_str.starts_with("first-letter") {
            (12, PseudoElement::Ignored)
        } else if pe_str.starts_with("placeholder") {
            (11, PseudoElement::Placeholder)
        } else if pe_str.starts_with("file-selector-button") {
            (20, PseudoElement::Ignored)
        } else if pe_str.starts_with("details-content") {
            (15, PseudoElement::Ignored)
        } else if pe_str.starts_with("spelling-error") {
            (14, PseudoElement::Ignored)
        } else if pe_str.starts_with("grammar-error") {
            (13, PseudoElement::Ignored)
        } else if pe_str.starts_with("backdrop") {
            (8, PseudoElement::Backdrop)
        } else if let Some(len) = pseudo_function_len(&pe_str, "part") {
            (len, PseudoElement::Ignored)
        } else if let Some(len) = pseudo_function_len(&pe_str, "slotted") {
            (len, PseudoElement::Ignored)
        } else {
            // Unknown vendor or other pseudo-element — ignore rule entirely
            return (String::new(), PseudoElement::Ignored);
        };
        let clean = format!("{}{}", &sel[..pos], &sel[pos + 2 + kw_len..])
            .trim()
            .to_string();
        let clean = if clean.is_empty() {
            "*".to_string()
        } else {
            clean
        };
        return (clean, pe);
    }
    // CSS2 single-colon :before / :after (not preceded by another colon)
    let sel_lower = sel.to_ascii_lowercase();
    for (kw, pe) in &[
        (":before", PseudoElement::Before),
        (":after", PseudoElement::After),
    ] {
        if let Some(pos) = find_unescaped(&sel_lower, kw) {
            if pos > 0 && sel.as_bytes()[pos - 1] == b':' {
                continue;
            }
            let clean = format!("{}{}", &sel[..pos], &sel[pos + kw.len()..])
                .trim()
                .to_string();
            let clean = if clean.is_empty() {
                "*".to_string()
            } else {
                clean
            };
            return (clean, pe.clone());
        }
    }
    (sel.to_string(), PseudoElement::None)
}

fn find_unescaped(haystack: &str, needle: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return Some(0);
    }

    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if &bytes[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub(crate) fn consume_block(s: &str) -> (&str, &str) {
    // s starts with '{'
    //
    // ⛔ STRINGS ARE OPAQUE. Block matching is defined over TOKENS
    // (css-syntax-3 §5.4.7), so a brace inside a string is string content.
    // Counting raw bytes let `content: "}"` close the rule early: the tail was
    // reparsed as a selector, failed validation, and took the NEXT — entirely
    // unrelated — rule down with it, repeating until braces happened to
    // realign. A stylesheet could lose arbitrarily much of itself to one glyph.
    let mut depth = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                } // escape: skip the next byte
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'"' | b'\'' => quote = Some(b),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return (&s[1..i], &s[i + 1..]);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    (s, "")
}

fn extract_scope_selectors(at_header: &str) -> (Option<CssSelector>, Option<CssSelector>) {
    let Some(rest) = at_header.strip_prefix("@scope") else {
        return (None, None);
    };
    let rest = rest.trim();
    let Some(open) = rest.find('(') else {
        return (None, None);
    };
    let Some(close) = matching_paren(rest, open) else {
        return (None, None);
    };
    let root_selector = parse_scope_selector(&rest[open + 1..close]);
    let tail = rest[close + 1..].trim_start();
    let limit_selector = tail
        .strip_prefix("to")
        .and_then(|tail| {
            let tail = tail.trim_start();
            let open = tail.find('(')?;
            let close = matching_paren(tail, open)?;
            Some(&tail[open + 1..close])
        })
        .and_then(parse_scope_selector);
    (root_selector, limit_selector)
}

fn parse_scope_selector(selector: &str) -> Option<CssSelector> {
    let selector = selector.trim();
    if selector.is_empty() {
        return None;
    }
    let parsed = parse_selector(selector);
    parsed.valid.then_some(parsed)
}

fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut i = open;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'(' => depth += 1,
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    None
}

/// Split a declaration block on the semicolons that actually terminate a
/// declaration — those at the top level, outside any string or bracket.
///
/// ⛔ A `;` inside a string or `url(…)` is ordinary content (css-syntax-3
/// §5.4.4, and the custom-property "preserve the original text" rule). Splitting
/// on every `;` truncated `--sep: ";"` to a stray quote and cut
/// `url(http://x/img;v=2.png)` in half.
pub(crate) fn split_declarations(block: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = block.as_bytes();
    let (mut start, mut depth, mut i) = (0usize, 0usize, 0usize);
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'(' | b'[' => depth += 1,
                b')' | b']' => {
                    depth = depth.saturating_sub(1);
                }
                b';' if depth == 0 => {
                    out.push(&block[start..i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    out.push(&block[start..]);
    out
}

/// Parse "prop: value; prop: value; ..." into a map.
/// Strips `!important` from values.
pub fn parse_declarations(block: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for decl in split_declarations(block) {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let raw_prop = decl[..colon].trim();
            // CSS custom properties (--*) are case-sensitive; standard properties are not.
            let prop = if raw_prop.starts_with("--") {
                raw_prop.to_string()
            } else {
                raw_prop.to_ascii_lowercase()
            };
            let value = strip_important(decl[colon + 1..].trim());
            if !prop.is_empty() && !value.is_empty() {
                map.insert(prop, value);
            }
        }
    }
    map
}

/// Parse declarations, splitting into (normal, important) maps.
/// Properties with `!important` go into the second map.
pub fn parse_declarations_important(block: &str) -> (Declarations, Declarations) {
    let mut normal = Declarations::new();
    let mut important = Declarations::new();
    for decl in split_declarations(block) {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let raw_prop = decl[..colon].trim();
            // CSS custom properties (--*) are case-sensitive; standard properties are not.
            let prop = if raw_prop.starts_with("--") {
                raw_prop.to_string()
            } else {
                raw_prop.to_ascii_lowercase()
            };
            let raw_value = decl[colon + 1..].trim();
            let is_important = has_important(raw_value);
            let value = strip_important(raw_value);
            if !prop.is_empty() && !value.is_empty() {
                if is_important {
                    important.insert(prop, value);
                } else {
                    normal.insert(prop, value);
                }
            }
        }
    }
    (normal, important)
}

/// Check if a CSS value contains `!important` (with optional whitespace).
fn has_important(val: &str) -> bool {
    // Match !important, ! important, !  important etc.
    if let Some(bang) = val.rfind('!') {
        val[bang + 1..].trim().eq_ignore_ascii_case("important")
    } else {
        false
    }
}

fn pseudo_function_len(value: &str, name: &str) -> Option<usize> {
    let prefix = format!("{name}(");
    if !value.starts_with(&prefix) {
        return None;
    }
    let mut depth = 0usize;
    for (idx, ch) in value.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

/// Strip `!important` (with optional whitespace) from a CSS value.
fn strip_important(val: &str) -> String {
    if let Some(bang) = val.rfind('!') {
        let after = val[bang + 1..].trim();
        if after.eq_ignore_ascii_case("important") {
            return val[..bang].trim().to_string();
        }
    }
    val.to_string()
}

/// Parse a single CSS selector string into a CssSelector.
pub fn parse_selector(s: &str) -> CssSelector {
    let mut parts = Vec::new();
    // Selectors §3.1 — an unrecognised simple selector makes the whole complex
    // selector invalid. Recorded rather than acted on here: whether that kills
    // the rule depends on where the selector sits, and only the caller knows.
    let mut valid = true;
    let mut chars = s.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' | '\n' => {
                // Consume all leading whitespace
                while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                    chars.next();
                }
                // Determine combinator based on the next non-whitespace character
                let next_non_ws = chars.peek().copied();
                match next_non_ws {
                    Some('>') => {
                        chars.next();
                        // Skip any whitespace after the '>'
                        while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                            chars.next();
                        }
                        parts.push(SelectorPart::Combinator(Combinator::Child));
                    }
                    Some('+') => {
                        chars.next();
                        while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                            chars.next();
                        }
                        parts.push(SelectorPart::Combinator(Combinator::AdjacentSibling));
                    }
                    Some('~') => {
                        chars.next();
                        while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                            chars.next();
                        }
                        parts.push(SelectorPart::Combinator(Combinator::GeneralSibling));
                    }
                    Some('|') => {
                        chars.next();
                        if chars.peek() == Some(&'|') {
                            chars.next();
                            while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                                chars.next();
                            }
                            parts.push(SelectorPart::Combinator(Combinator::Column));
                        } else {
                            valid = false;
                        }
                    }
                    _ => {
                        parts.push(SelectorPart::Combinator(Combinator::Descendant));
                    }
                }
            }
            // ⛔ Skip the whitespace AFTER the combinator. Without this the
            // space in `"> em"` reaches the whitespace arm below and pushes a
            // SECOND, descendant combinator — `[Child, Descendant, em]` — which
            // matches nothing. It only bit selectors starting with a
            // combinator, because in `div > em` the whitespace arm sees the
            // `>` first and already skips past it; a RELATIVE selector like a
            // `:has(> em)` argument starts with one.
            '>' | '+' | '~' => {
                let c = match ch {
                    '>' => Combinator::Child,
                    '+' => Combinator::AdjacentSibling,
                    _ => Combinator::GeneralSibling,
                };
                chars.next();
                while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                    chars.next();
                }
                parts.push(SelectorPart::Combinator(c));
            }
            '|' => {
                chars.next();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    while matches!(chars.peek(), Some(' ') | Some('\t') | Some('\n')) {
                        chars.next();
                    }
                    parts.push(SelectorPart::Combinator(Combinator::Column));
                } else {
                    valid = false;
                }
            }
            '#' => {
                chars.next();
                let id = read_ident(&mut chars);
                parts.push(SelectorPart::Id(id));
            }
            '.' => {
                chars.next();
                let cls = read_ident(&mut chars);
                parts.push(SelectorPart::Class(cls));
            }
            ':' => {
                chars.next();
                let is_elem = chars.peek() == Some(&':');
                if is_elem {
                    chars.next();
                }
                let name = read_ident(&mut chars);
                // consume optional (...)
                if chars.peek() == Some(&'(') {
                    // Collect balanced args (respecting nested parens)
                    chars.next(); // consume '('
                    let args = read_balanced_parens(&mut chars);
                    if !is_elem {
                        match name.as_str() {
                            // `:not()` and `:has()` take a NON-forgiving list —
                            // one unrecognised branch invalidates the selector
                            // that contains them.
                            "not" => {
                                // ⛔ TOP-LEVEL commas only. A plain `split(',')`
                                // tears a nested list apart at its inner commas,
                                // so `:not(iframe, canvas, img)` inside a
                                // `:where(...)` — the modern-reset idiom — parsed
                                // as something far weaker than written.
                                let selectors: Vec<CssSelector> = split_selectors(&args)
                                    .into_iter()
                                    .map(|s| parse_selector(s.trim()))
                                    .collect();
                                if selectors.iter().any(|s| !s.valid) {
                                    valid = false;
                                }
                                if selectors.len() == 1 {
                                    parts.push(SelectorPart::Not(Box::new(
                                        selectors.into_iter().next().unwrap(),
                                    )));
                                } else {
                                    // :not(.a,.b) ≡ :not(.a):not(.b)
                                    for sel in selectors {
                                        parts.push(SelectorPart::Not(Box::new(sel)));
                                    }
                                }
                            }
                            // `:is()` and `:where()` take a FORGIVING list
                            // (Selectors §3.5): an unrecognised branch drops
                            // itself and the rest of the list still matches.
                            // That is the whole point of them — `:is(a, :bogus)`
                            // is how a page targets a selector some engines do
                            // not have yet.
                            "is" => {
                                let selectors: Vec<CssSelector> = split_selectors(&args)
                                    .into_iter()
                                    .map(|s| parse_selector(s.trim()))
                                    .filter(|s| s.valid)
                                    .collect();
                                parts.push(SelectorPart::Is(selectors));
                            }
                            "where" => {
                                let selectors: Vec<CssSelector> = split_selectors(&args)
                                    .into_iter()
                                    .map(|s| parse_selector(s.trim()))
                                    .filter(|s| s.valid)
                                    .collect();
                                parts.push(SelectorPart::Where(selectors));
                            }
                            "has" => {
                                // ⛔ A LIST, split on top-level commas.
                                // `:has(h1, h2)` is "has an h1 OR an h2"; parsing
                                // the argument as one selector turned the comma
                                // into a descendant combinator, so it read as
                                // "has an h1 that contains an h2".
                                let selectors: Vec<CssSelector> = split_selectors(&args)
                                    .into_iter()
                                    .map(|s| parse_selector(s.trim()))
                                    .collect();
                                if selectors.is_empty() || selectors.iter().any(|s| !s.valid) {
                                    valid = false;
                                }
                                parts.push(SelectorPart::Has(selectors));
                            }
                            _ => {
                                if !is_known_pseudo_class(&name) {
                                    valid = false;
                                }
                                let full_name = format!("{}({})", name, args);
                                parts.push(SelectorPart::PseudoClass(full_name));
                            }
                        }
                    } else {
                        let full_name = format!("{}({})", name, args);
                        parts.push(SelectorPart::PseudoElement(full_name));
                    }
                } else if is_elem {
                    parts.push(SelectorPart::PseudoElement(name));
                } else {
                    if !is_known_pseudo_class(&name) {
                        valid = false;
                    }
                    parts.push(SelectorPart::PseudoClass(name));
                }
            }
            '[' => {
                chars.next();
                let attr_str: String = chars.by_ref().take_while(|&c| c != ']').collect();
                let (name, op, value, case_sensitive) = parse_attr_selector(&attr_str);
                parts.push(SelectorPart::Attribute {
                    name,
                    op,
                    value,
                    case_sensitive,
                });
            }
            '*' => {
                chars.next();
                parts.push(SelectorPart::Universal);
            }
            _ => {
                let tag = read_ident(&mut chars);
                if !tag.is_empty() {
                    parts.push(SelectorPart::Tag(tag.to_ascii_lowercase()));
                } else {
                    chars.next(); // skip unknown
                }
            }
        }
    }

    CssSelector::new_checked(parts, valid)
}

fn read_ident(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    while let Some(&c) = chars.peek() {
        if c == '\\' {
            chars.next(); // consume '\'
            if let Some(escaped) = read_css_ident_escape(chars) {
                s.push(escaped);
            }
        } else if c.is_alphanumeric() || c == '-' || c == '_' {
            s.push(c);
            chars.next();
        } else {
            break;
        }
    }
    s
}

fn read_css_ident_escape(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<char> {
    let &first = chars.peek()?;
    if first == '\n' || first == '\r' || first == '\x0c' {
        return None;
    }

    if first.is_ascii_hexdigit() {
        let mut hex = String::new();
        while hex.len() < 6 {
            let Some(&c) = chars.peek() else {
                break;
            };
            if !c.is_ascii_hexdigit() {
                break;
            }
            hex.push(c);
            chars.next();
        }

        if matches!(chars.peek(), Some(' ' | '\t' | '\n' | '\r' | '\x0c')) {
            chars.next();
        }

        return u32::from_str_radix(&hex, 16)
            .ok()
            .and_then(char::from_u32)
            .or(Some('\u{fffd}'));
    }

    chars.next()
}

/// Split a selector list at commas, respecting parentheses nesting.
/// e.g. "body:not(.a,.b) .c, div" → ["body:not(.a,.b) .c", " div"]
fn split_selectors(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Consume characters until the matching `)` for an already-consumed `(`.
/// Handles nested parens. Returns the content (without the outer parens).
fn read_balanced_parens(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut s = String::new();
    let mut depth = 1usize;
    for c in chars.by_ref() {
        match c {
            '(' => {
                depth += 1;
                s.push(c);
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                s.push(c);
            }
            _ => {
                s.push(c);
            }
        }
    }
    s
}

fn parse_attr_selector(s: &str) -> (String, AttrOp, String, Option<bool>) {
    let (name_and_value, op): (&str, AttrOp) = if let Some(p) = s.find("~=") {
        (&s[p..], AttrOp::Includes)
    } else if let Some(p) = s.find("|=") {
        (&s[p..], AttrOp::DashMatch)
    } else if let Some(p) = s.find("^=") {
        (&s[p..], AttrOp::StartsWith)
    } else if let Some(p) = s.find("$=") {
        (&s[p..], AttrOp::EndsWith)
    } else if let Some(p) = s.find("*=") {
        (&s[p..], AttrOp::Contains)
    } else if let Some(p) = s.find('=') {
        (&s[p..], AttrOp::Eq)
    } else {
        // `[name]` — no value, so no flag either.
        return (s.trim().to_string(), AttrOp::Exists, String::new(), None);
    };
    let op_len = if matches!(op, AttrOp::Eq) { 1 } else { 2 };
    let name = s[..s.len() - name_and_value.len()].trim().to_string();
    let (value, case_sensitive) = split_attr_flag(&name_and_value[op_len..]);
    (name, op, value, case_sensitive)
}

/// Peel Selectors §6.3's trailing case-sensitivity flag off an attribute
/// selector's value.
///
/// The flag is a bare `i` or `s` after the value, so it is only a flag when it
/// stands alone: `[type=hidden i]` carries one and `[class~=hi]` does not, and
/// `[title="i"]` does not either — a quoted value ends at its closing quote and
/// anything after it is the flag, which is why the quoted case is checked
/// first rather than trimmed away up front.
fn split_attr_flag(raw: &str) -> (String, Option<bool>) {
    let raw = raw.trim();
    let flag_of = |c: char| match c {
        'i' | 'I' => Some(Some(false)),
        's' | 'S' => Some(Some(true)),
        _ => None,
    };
    // Quoted value: the flag is whatever follows the closing quote.
    if let Some(quote) = raw.chars().next().filter(|c| *c == '"' || *c == '\'') {
        if let Some(end) = raw[1..].find(quote).map(|i| i + 1) {
            let value = raw[1..end].to_string();
            let rest = raw[end + 1..].trim();
            let flag = if rest.chars().count() == 1 {
                flag_of(rest.chars().next().unwrap()).unwrap_or(None)
            } else {
                None
            };
            return (value, flag);
        }
    }
    // Unquoted value: a flag has to be separated from it by whitespace,
    // otherwise `[lang=fi]` would read its own `i` as the flag.
    if let Some((head, tail)) = raw.rsplit_once(char::is_whitespace) {
        let tail = tail.trim();
        if tail.chars().count() == 1 {
            if let Some(flag) = flag_of(tail.chars().next().unwrap()) {
                return (strip_quotes(head.trim()), flag);
            }
        }
    }
    (strip_quotes(raw), None)
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

thread_local! {
    /// Layer names in DECLARATION order, for the sheet currently being parsed.
    ///
    /// Order comes from whichever appears first: a `@layer a, b;` statement or
    /// the first `@layer a { … }` block. CSS Cascade 5 §6.4.4.
    static LAYER_ORDER: std::cell::RefCell<Vec<String>> =
        std::cell::RefCell::new(Vec::new());
}

pub(crate) fn reset_declared_layers() {
    LAYER_ORDER.with(|l| l.borrow_mut().clear());
}

fn next_anonymous_layer_name() -> String {
    LAYER_ORDER.with(|l| format!("__webcore_anonymous_layer_{}", l.borrow().len()))
}

fn qualify_layer_name(parent_layer: &str, name: &str) -> String {
    let name = name.trim();
    if parent_layer.is_empty() || name.is_empty() {
        name.to_string()
    } else {
        format!("{parent_layer}.{name}")
    }
}

fn declare_layer(name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    LAYER_ORDER.with(|l| {
        let mut l = l.borrow_mut();
        if !l.iter().any(|n| n == name) {
            l.push(name.to_string());
        }
    });
}

/// The layer names declared so far, in order.
pub fn declared_layers() -> Vec<String> {
    LAYER_ORDER.with(|l| l.borrow().clone())
}
