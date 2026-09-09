//! Presentational attributes (`width`, `bgcolor`, `align`) as styles.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Apply presentational attributes ───────────────────────────────────────

/// Apply a presentational hint through the element's `style` attribute,
/// WITHOUT overwriting a declaration the author already wrote there.
///
/// The style attribute is the only place a parse-time value survives: the
/// cascade rebuilds `node.style` from scratch, so writing the hint directly
/// onto the computed style loses it to the UA sheet. Writing the attribute is
/// therefore how the hint has to travel — but it must be written ONCE.
/// Appending unconditionally made `rows="3"` add `height:4.2em` on every
/// serialize → reparse cycle, so a saved-and-reloaded page grew
/// `style="height:4.2em;height:4.2em;…"` without bound.
///
/// Skipping when the property is already present also gives the hint the right
/// PRECEDENCE for free: an author's own `style="height:10px"` stays.
fn add_presentational_style(node: &mut WebCore, prop: &str, value: &str) {
    let existing = node.attributes.get("style").cloned().unwrap_or_default();
    let already = existing.split(';').any(|d| {
        d.split(':')
            .next()
            .map(|k| k.trim().eq_ignore_ascii_case(prop))
            .unwrap_or(false)
    });
    if already {
        return;
    }
    let decl = format!("{}:{}", prop, value);
    node.attributes.insert(
        "style",
        if existing.trim().is_empty() {
            decl
        } else {
            format!("{};{}", existing, decl)
        },
    );
}

pub(crate) fn apply_presentational_attrs(node: &mut WebCore) {
    let attrs = node.attributes.clone();
    let tag = node.tag.clone();

    // Translate body `text` attribute to `color` attribute so the cascade picks it up
    if tag == "body" {
        if let Some(text_color) = attrs.get("text") {
            let text_color = text_color.clone();
            if !node.attributes.contains_key("color") {
                node.attributes.insert("color", text_color);
            }
        }
    }

    // Sorted, because `attrs` is a HashMap and two presentational attributes on
    // one element can map to the SAME property — `bgcolor` and
    // `background-color` both set the background, `size` and `width` both size
    // an `<input>`. Applied in hash order, which element won was decided by the
    // process's hash seed, the same way declaration blocks were before
    // `css::Declarations`. Sorting is not the spec's order, but it is an order:
    // the same document renders the same way twice.
    let mut ordered_attrs: Vec<(&String, &String)> = attrs.iter().collect();
    ordered_attrs.sort_by(|a, b| a.0.cmp(b.0));
    for (attr, val) in ordered_attrs {
        match attr.as_str() {
            "align" => match val.as_str() {
                "center" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "text-align",
                    "center",
                ),
                "right" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "text-align",
                    "right",
                ),
                "left" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "text-align",
                    "left",
                ),
                "justify" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "text-align",
                    "justify",
                ),
                _ => {}
            },
            "valign" => match val.as_str() {
                "top" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "vertical-align",
                    "top",
                ),
                "middle" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "vertical-align",
                    "middle",
                ),
                "bottom" => apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "vertical-align",
                    "bottom",
                ),
                _ => {}
            },
            "bgcolor" | "background-color" => {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "background-color",
                    val,
                );
            }
            "color" => {
                apply_property(std::sync::Arc::make_mut(&mut node.style), "color", val);
            }
            "text" if tag == "body" => {
                // handled above by translating to `color` attribute
            }
            "width" => {
                if val.ends_with('%') {
                    apply_property(std::sync::Arc::make_mut(&mut node.style), "width", val);
                } else if let Some(n) = crate::html::forms::parse_non_negative_integer(val) {
                    apply_property(
                        std::sync::Arc::make_mut(&mut node.style),
                        "width",
                        &format!("{}px", n),
                    );
                }
            }
            "height" => {
                if val.ends_with('%') {
                    apply_property(std::sync::Arc::make_mut(&mut node.style), "height", val);
                } else if let Some(n) = crate::html::forms::parse_non_negative_integer(val) {
                    apply_property(
                        std::sync::Arc::make_mut(&mut node.style),
                        "height",
                        &format!("{}px", n),
                    );
                }
            }
            "border" if tag == "table" => {
                if val == "0" {
                    apply_property(
                        std::sync::Arc::make_mut(&mut node.style),
                        "border",
                        "0px solid transparent",
                    );
                } else if let Ok(n) = val.parse::<f32>() {
                    if n > 0.0 {
                        apply_property(
                            std::sync::Arc::make_mut(&mut node.style),
                            "border",
                            &format!("{}px solid black", n),
                        );
                    }
                }
            }
            // FONT legacy attributes
            "face" if tag == "font" => {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "font-family",
                    val,
                );
            }
            "size" if tag == "input" => {}
            "rows" | "cols" if tag == "textarea" => {}
            "size" if tag == "font" => {
                // HTML font size 1-7 → approximate px sizes
                let px: f32 = match val.trim() {
                    "1" => 10.0,
                    "2" => 13.0,
                    "3" => 16.0,
                    "4" => 18.0,
                    "5" => 24.0,
                    "6" => 32.0,
                    "7" => 48.0,
                    _ => 16.0,
                };
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "font-size",
                    &format!("{}px", px),
                );
            }
            // TABLE legacy attributes
            "cellpadding" if tag == "table" => {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "cellpadding",
                    val,
                );
            }
            "cellspacing" if tag == "table" => {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "cellspacing",
                    val,
                );
            }
            // COL attributes
            "span" if tag == "col" => {
                // Stored in attributes; layout reads it directly
            }
            _ => {}
        }
    }

    // Inline style
    if let Some(style_val) = attrs.get("style") {
        let style_val = style_val.clone();
        apply_inline_style(node, &style_val);
    }

    // dir attribute
    if let Some(dir) = attrs.get("dir") {
        match dir.to_ascii_lowercase().as_str() {
            "rtl" => apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "direction",
                "rtl",
            ),
            "ltr" => apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "direction",
                "ltr",
            ),
            "auto" => {
                let text = collect_text_for_dir_auto(node);
                if let Some(dir) = crate::layout::text::first_strong_direction(&text) {
                    match dir {
                        Direction::RTL => apply_property(
                            std::sync::Arc::make_mut(&mut node.style),
                            "direction",
                            "rtl",
                        ),
                        Direction::LTR => apply_property(
                            std::sync::Arc::make_mut(&mut node.style),
                            "direction",
                            "ltr",
                        ),
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_text_for_dir_auto(node: &WebCore) -> String {
    let mut out = String::new();
    collect_text_for_dir_auto_inner(node, &mut out);
    out
}

fn collect_text_for_dir_auto_inner(node: &WebCore, out: &mut String) {
    if matches!(node.tag.as_str(), "script" | "style") {
        return;
    }
    if node.tag != "#comment" && !node.text.is_empty() {
        out.push_str(&node.text);
    }
    for child in &node.children {
        collect_text_for_dir_auto_inner(child, out);
    }
}

fn apply_inline_style(node: &mut WebCore, css: &str) {
    for decl in css.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let prop = decl[..colon].trim();
            let val = decl[colon + 1..].trim();
            if !prop.is_empty() && !val.is_empty() {
                let normalized = normalize_css_value(val);
                apply_property(std::sync::Arc::make_mut(&mut node.style), prop, &normalized);
            }
        }
    }
}

/// Convert pt units to px (1pt = 4/3 px at 96dpi), since the CSS parser
/// doesn't handle `pt` directly.
fn normalize_css_value(v: &str) -> String {
    if v.ends_with("pt") {
        if let Ok(n) = v[..v.len() - 2].trim().parse::<f32>() {
            return format!("{}px", n * 4.0 / 3.0);
        }
    }
    v.to_string()
}

/// Normalize a CSS text block, converting pt to px so the CSS parser handles it.
pub(crate) fn normalize_css_text(css: &str) -> String {
    if !css.contains("pt") {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len());
    let mut last_copied = 0;
    let bytes = css.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit()
            || (bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            let start = i;
            if bytes[i] == b'.' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i + 1 < bytes.len() && bytes[i] == b'p' && bytes[i + 1] == b't' {
                let after = i + 2;
                let boundary = after >= bytes.len()
                    || (!bytes[after].is_ascii_alphanumeric() && bytes[after] != b'_');
                if boundary {
                    if let Ok(n) = css[start..i].parse::<f32>() {
                        out.push_str(&css[last_copied..start]);
                        let px = n * 4.0 / 3.0;
                        out.push_str(&format!("{:.4}px", px));
                        i += 2; // skip "pt"
                        last_copied = i;
                        continue;
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    out.push_str(&css[last_copied..]);
    out
}
