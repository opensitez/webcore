//! Passes over a parsed subtree: whitespace collapsing, `<picture>`
//! resolution and list numbering.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

pub(crate) fn collapse_whitespace(s: &str) -> String {
    // Collapse any run of ASCII whitespace (including newlines) to a single space.
    // This is correct for normal (non-pre) HTML content. Whitespace-only text
    // nodes that need to act as line-breaks in `white-space: pre` parents are
    // handled by the call sites, not here.
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !in_ws {
                in_ws = true;
            }
        } else {
            if in_ws {
                out.push(' ');
                in_ws = false;
            }
            out.push(ch);
        }
    }
    if in_ws {
        out.push(' ');
    }
    out
}

pub(crate) fn parse_srcset_url(srcset: &str) -> Option<String> {
    parse_srcset_url_for(srcset, None, 800.0, 600.0, 1.0)
}

pub(crate) fn parse_srcset_url_for(
    srcset: &str,
    sizes: Option<&str>,
    viewport_w: f32,
    viewport_h: f32,
    device_pixel_ratio: f32,
) -> Option<String> {
    let candidates = parse_srcset_candidates(srcset);
    if candidates.is_empty() {
        return None;
    }

    let dpr = device_pixel_ratio.max(0.01);
    if candidates.iter().any(|c| c.width.is_some()) {
        let source_size = parse_sizes_width(sizes, viewport_w, viewport_h);
        let target = source_size * dpr;
        return candidates
            .iter()
            .filter_map(|c| c.width.map(|w| (c, w)))
            .filter(|(_, w)| *w >= target)
            .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .or_else(|| {
                candidates
                    .iter()
                    .filter_map(|c| c.width.map(|w| (c, w)))
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            })
            .map(|(c, _)| c.url.clone());
    }

    candidates
        .iter()
        .min_by(|a, b| {
            let ad = a.density.unwrap_or(1.0);
            let bd = b.density.unwrap_or(1.0);
            let a_good = ad >= dpr;
            let b_good = bd >= dpr;
            match (a_good, b_good) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => ad.partial_cmp(&bd).unwrap_or(std::cmp::Ordering::Equal),
                (false, false) => bd.partial_cmp(&ad).unwrap_or(std::cmp::Ordering::Equal),
            }
        })
        .map(|c| c.url.clone())
}

#[derive(Debug)]
struct SrcsetCandidate {
    url: String,
    width: Option<f32>,
    density: Option<f32>,
}

fn parse_srcset_candidates(srcset: &str) -> Vec<SrcsetCandidate> {
    let mut candidates = Vec::new();
    for entry in split_comma_list(srcset) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let mut parts = entry.split_whitespace();
        let url = match parts.next() {
            Some(u) if !u.is_empty() => u,
            _ => continue,
        };
        let mut width = None;
        let mut density = None;
        if let Some(descriptor) = parts.next() {
            if let Some(w_str) = descriptor.strip_suffix('w') {
                if let Ok(w) = w_str.parse::<f32>() {
                    width = Some(w.max(0.0));
                }
            } else if let Some(x_str) = descriptor.strip_suffix('x') {
                if let Ok(x) = x_str.parse::<f32>() {
                    density = Some(x.max(0.0));
                }
            }
        }
        candidates.push(SrcsetCandidate {
            url: url.to_string(),
            width,
            density,
        });
    }
    candidates
}

fn parse_sizes_width(sizes: Option<&str>, viewport_w: f32, viewport_h: f32) -> f32 {
    let viewport_w = viewport_w.max(1.0);
    let viewport_h = viewport_h.max(1.0);
    let Some(sizes) = sizes else {
        return viewport_w;
    };
    for item in split_comma_list(sizes) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let (condition, length) = split_size_condition(item);
        if !condition.is_empty() && !crate::css::evaluate_media(condition, viewport_w, viewport_h) {
            continue;
        }
        if let Some(length) = crate::css::value_parse::parse_length_checked(length) {
            let resolved = length.resolve_vp(16.0, viewport_w, 16.0, viewport_w, viewport_h);
            if resolved > 0.0 {
                return resolved;
            }
        }
    }
    viewport_w
}

fn split_size_condition(item: &str) -> (&str, &str) {
    let mut depth = 0usize;
    for (idx, ch) in item.char_indices().rev() {
        match ch {
            ')' => depth += 1,
            '(' => depth = depth.saturating_sub(1),
            c if c.is_ascii_whitespace() && depth == 0 => {
                return (item[..idx].trim(), item[idx..].trim());
            }
            _ => {}
        }
    }
    ("", item.trim())
}

fn split_comma_list(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&input[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&input[start..]);
    out
}

/// Resolve the best `<source>` for a `<picture>` element and set it on the child `<img>`.
pub(crate) fn resolve_picture_source(picture: &mut WebCore, base_url: &str, vw: f32, vh: f32) {
    // Find the best matching <source>
    let mut best_url: Option<String> = None;
    let mut best_width: Option<String> = None;
    let mut best_height: Option<String> = None;
    for child in &picture.children {
        if child.tag != "source" {
            continue;
        }
        if let Some(typ) = child.attributes.get("type") {
            let typ = typ.split(';').next().unwrap_or("").trim();
            if !typ.is_empty() && !supported_image_type(typ) {
                continue;
            }
        }
        // Check media query if present
        if let Some(media) = child.attributes.get("media") {
            if vw > 0.0 || vh > 0.0 {
                if !crate::css::evaluate_media(media, vw, vh) {
                    continue;
                }
            } else {
                // Viewport unknown — skip conditional sources
                continue;
            }
        }
        // Extract URL from srcset
        if let Some(srcset) = child.attributes.get("srcset") {
            let sizes = child
                .attributes
                .get("sizes")
                .map(|s| s.as_str())
                .or_else(|| {
                    picture
                        .children
                        .iter()
                        .find(|c| c.tag == "img")
                        .and_then(|img| img.attributes.get("sizes"))
                        .map(|s| s.as_str())
                });
            if let Some(url) = parse_srcset_url_for(srcset, sizes, vw, vh, 1.0) {
                best_url = Some(url);
                best_width = child.attributes.get("width").cloned();
                best_height = child.attributes.get("height").cloned();
                break; // First matching source wins
            }
        }
    }

    if let Some(url) = best_url {
        // Find the child <img> and set its src + dimensions from the source
        for child in &mut picture.children {
            if child.tag == "img" {
                // Only the RESOLVED url changes. `src` is the author's content
                // attribute and picking a `<source>` does not rewrite it —
                // `img.src` still reads back what the markup said, and the
                // chosen candidate is what `currentSrc` reports. Overwriting
                // the attribute made `<picture>` mutate the document.
                child.resolved_src = resolve_url(&url, base_url);
                // Transfer width/height from the matched <source> so the image
                // is sized correctly (the <source> often has larger dimensions
                // than the fallback <img>). Applied to the STYLE, not to the
                // width/height content attributes, for the same reason.
                if let Some(ref w) = best_width {
                    crate::css::apply_property(
                        std::sync::Arc::make_mut(&mut child.style),
                        "width",
                        &format!("{}px", w),
                    );
                }
                if let Some(ref h) = best_height {
                    crate::css::apply_property(
                        std::sync::Arc::make_mut(&mut child.style),
                        "height",
                        &format!("{}px", h),
                    );
                }
                break;
            }
        }
    }
}

fn supported_image_type(typ: &str) -> bool {
    matches!(
        typ.to_ascii_lowercase().as_str(),
        "image/png"
            | "image/jpeg"
            | "image/jpg"
            | "image/gif"
            | "image/webp"
            | "image/bmp"
            | "image/svg+xml"
    )
}

/// Post-pass: re-resolve `<picture>` elements with real viewport dimensions.
pub fn resolve_picture_elements(node: &mut WebCore, base_url: &str, vw: f32, vh: f32) {
    if node.tag == "picture" {
        resolve_picture_source(node, base_url, vw, vh);
        for child in &mut node.children {
            if child.tag != "img" {
                resolve_picture_elements(child, base_url, vw, vh);
            }
        }
        return;
    } else if node.tag == "img" {
        resolve_img_source(node, base_url, vw, vh);
    }
    for child in &mut node.children {
        resolve_picture_elements(child, base_url, vw, vh);
    }
}

fn resolve_img_source(node: &mut WebCore, base_url: &str, vw: f32, vh: f32) {
    if let Some(srcset) = node.attributes.get("srcset") {
        let sizes = node.attributes.get("sizes").map(|s| s.as_str());
        if let Some(best) = parse_srcset_url_for(srcset, sizes, vw, vh, 1.0) {
            node.resolved_src = resolve_url(&best, base_url);
            return;
        }
    }
    if let Some(src) = node.attributes.get("src") {
        node.resolved_src = resolve_url(src, base_url);
    }
}

pub(crate) fn number_lists(node: &mut WebCore) {
    if node.tag == "ol" {
        let mut idx = 1i32;
        for child in &mut node.children {
            if child.tag == "li" {
                std::sync::Arc::make_mut(&mut child.style).list_index = idx;
                idx += 1;
            }
        }
    }
    for child in &mut node.children {
        number_lists(child);
    }
}

impl crate::html::parser::HtmlParser {
    /// Post-processing applied to a node after its children have been parsed.
    pub(crate) fn post_process_node(node: &mut WebCore, base_url: &str) {
        // Declarative Shadow DOM: <template shadowrootmode="open|closed">
        // Convert the template's children into a shadow root on the parent.
        let has_shadow_template = node
            .children
            .iter()
            .any(|c| c.tag == "template" && c.attributes.contains_key("shadowrootmode"));
        if has_shadow_template {
            let mut shadow_children = Vec::new();
            let mut shadow_css = String::new();
            let mut shadow_mode = crate::types::ShadowMode::Open;
            // Extract the template with shadowrootmode
            node.children.retain(|c| {
                if c.tag == "template" {
                    if let Some(mode) = c.attributes.get("shadowrootmode") {
                        shadow_mode = if mode == "closed" {
                            crate::types::ShadowMode::Closed
                        } else {
                            crate::types::ShadowMode::Open
                        };
                        // Collect template children as shadow tree
                        for child in &c.children {
                            if child.tag == "style" {
                                // Extract style text for scoped stylesheet
                                shadow_css.push_str(&child.text);
                                for tc in &child.children {
                                    if tc.tag == "#text" {
                                        shadow_css.push_str(&tc.text);
                                    }
                                }
                            } else {
                                shadow_children.push(child.clone());
                            }
                        }
                        return false; // remove the template from light DOM
                    }
                }
                true
            });
            if !shadow_children.is_empty() || !shadow_css.is_empty() {
                // Start with UA stylesheet so shadow tree gets default styles
                let mut stylesheet = crate::css::ua_stylesheet();
                if !shadow_css.is_empty() {
                    // Author origin: a shadow root's own `<style>` outranks the
                    // UA sheet it is layered on, the same as a document's.
                    stylesheet.parse_and_add_author(&shadow_css);
                }
                node.shadow_root = Some(Box::new(crate::types::ShadowRoot {
                    children: shadow_children,
                    stylesheet,
                    mode: shadow_mode,
                    node_id: crate::dom::arena::next_shadow_node_id(),
                    delegates_focus: false,
                    slot_assignment: crate::types::SlotAssignment::Named,
                    clonable: false,
                    serializable: false,
                    adopted_stylesheets: Vec::new(),
                }));
            }
        }

        if node.tag == "picture" {
            resolve_picture_source(node, base_url, 0.0, 0.0);
        }
        // <form> inside <table>: browsers treat form as transparent (display:contents)
        // so it doesn't break table row structure.
        if matches!(node.tag.as_str(), "table" | "thead" | "tbody" | "tfoot") {
            for child in &mut node.children {
                if child.tag == "form" {
                    std::sync::Arc::make_mut(&mut child.style).display = Display::Contents;
                }
            }
        }
        // <select>: keep option children in the DOM for CSS styling.
        // The selected option's text is shown inline; others are display:none.
        // When the dropdown opens, all options are rendered as a popup.
        if node.tag == "select" {
            // `<option selected>` in the markup seeds SELECTEDNESS, exactly as
            // `<input checked>` seeds checkedness, and the attribute stays put
            // as the default a form reset restores to.
            //
            // Then the selectedness setting algorithm decides what a document
            // with no `selected` anywhere shows. ⛔ Its auto-select step is
            // guarded on a display size of 1, so a DROP-DOWN lands on its first
            // enabled option and a LIST BOX is left with nothing selected —
            // which is the state HTML says it starts in. This used to default
            // an index to 0 unconditionally and every list box opened with a
            // highlighted first row.
            crate::html::forms::for_each_option_mut(node, &mut |option, _| {
                option.selectedness = option.attributes.contains_key("selected");
                option.dirty_selectedness = false;
            });
            crate::html::forms::run_selectedness_setting_algorithm(node);

            // The options are hidden either way: a drop-down shows one label,
            // and a list box's rows are painted by the control itself rather
            // than laid out as boxes.
            fn hide_options(node: &mut WebCore) {
                for child in &mut node.children {
                    if matches!(child.tag.as_str(), "option" | "optgroup") {
                        apply_property(
                            std::sync::Arc::make_mut(&mut child.style),
                            "display",
                            "none",
                        );
                        hide_options(child);
                    }
                }
            }
            hide_options(node);

            // ⛔ NO display text node. A drop-down's label is not a child of
            // the select — the author never wrote it, and inventing one put a
            // text node in `childNodes` that doubled `textContent` and came
            // back duplicated through every serialize/reparse round
            // (`<option>Thin</option>Thin` became `ThinThin`).
            //
            // Nothing needed it: the painter reads the label straight off the
            // option whose selectedness is set (`display_list_builder`), which
            // is also the only reading that tracks a selection the user has
            // changed since parse.
            // Set overflow hidden so options don't leak
            apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "overflow",
                "hidden",
            );
        }
        // <input>: seed the control's state from its content attributes.
        if node.tag == "input" {
            // `<input checked>` in the markup seeds CHECKEDNESS, and the
            // attribute stays as the default a reset restores to. The
            // `defaultChecked` attribute this used to invent was never a
            // content attribute — `defaultChecked` is the IDL name for the
            // `checked` attribute, which is right here.
            if node.attributes.contains_key("checked") {
                node.checkedness = true;
            }
            // "Invoke the value sanitization algorithm, if one is defined for
            // the type attribute's state." For a range that is what turns a
            // step-mismatched or out-of-bounds `value` into the number the
            // control actually holds, before anything paints or reads it.
            crate::html::forms::seed_input_value(node);
            let input_type = node
                .attributes
                .get("type")
                .map(|s| s.as_str())
                .unwrap_or("text");
            match input_type {
                "submit" | "button" | "reset" => {
                    let label =
                        node.attributes
                            .get("value")
                            .cloned()
                            .unwrap_or_else(|| match input_type {
                                "submit" => "Submit".to_string(),
                                "reset" => "Reset".to_string(),
                                _ => String::new(),
                            });
                    if !label.is_empty() {
                        node.children.clear();
                        let mut text_node = WebCore::new("#text");
                        text_node.text = label;
                        node.children.push(text_node);
                    }
                }
                "image" => {
                    // Image input: treat src like <img src>
                    if let Some(src) = node.attributes.get("src").cloned() {
                        let resolved = resolve_url(&src, base_url);
                        node.resolved_src = resolved;
                    }
                }
                _ => {}
            }
        }
        if node.tag == "details" {
            let is_open = node.attributes.contains_key("open");
            for child in &mut node.children {
                if child.tag == "summary" {
                    // summary always visible
                } else if !is_open {
                    apply_property(
                        std::sync::Arc::make_mut(&mut child.style),
                        "display",
                        "none",
                    );
                }
            }
        }
    }
}
