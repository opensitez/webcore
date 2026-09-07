//! `<head>` content — title, base, meta, link, style, script.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Head content parser ─────────────────────────────────────────────────────

pub(crate) fn parse_head_content(parser: &mut HtmlParser) {
    loop {
        match parser.tokens.get(parser.pos).cloned() {
            None => break,
            Some(Token::CloseTag { tag }) if tag == "head" => {
                parser.pos += 1;
                break;
            }
            Some(Token::OpenTag {
                tag,
                attrs,
                self_closing,
            }) => {
                parser.pos += 1;
                if !handle_head_tag(parser, &tag, attrs, self_closing) {
                    // Not head content, but we are inside an explicit <head>:
                    // the spec's "in head" mode ignores it. Skip its subtree so
                    // it does not leak into the head's children.
                    if !self_closing {
                        parser.skip_until_close(&tag);
                    }
                }
            }
            _ => {
                parser.pos += 1;
            }
        }
    }
}

/// Is this a tag the "in head" insertion mode owns?
///
/// Used both inside an explicit `<head>` and by the implied-head path — a
/// document that opens with a bare `<style>` and never writes `<head>` still
/// puts that style in the head, which is where `document.head.children` and
/// every browser expect to find it.
pub(crate) fn is_head_content_tag(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "noscript" | "style" | "title" | "link" | "meta" | "base" | "template"
    )
}

/// Process one "in head" start tag. Returns false if `tag` is not head content.
pub(crate) fn handle_head_tag(
    parser: &mut HtmlParser,
    tag: &str,
    attrs: crate::dom::attrs::AttrMap,
    self_closing: bool,
) -> bool {
    match tag {
        "script" | "noscript" => {
            let content = if !self_closing {
                parser.collect_raw_text_until(tag)
            } else {
                String::new()
            };
            if let Some(ref mut f) = parser.on_script {
                f(tag, &attrs, &content);
            }
            // In <head>, noscript fallback content is not rendered.
        }
        "style" => {
            parser.fire_hook(tag, &attrs);
            let css = parser.collect_raw_text_until("style");
            parser.stylesheet.parse_and_add(&normalize_css_text(&css));
            parser.push_head_node("style", attrs, css);
        }
        "title" => {
            parser.fire_hook(tag, &attrs);
            let text = parser.collect_raw_text_until("title");
            parser.title = text.trim().to_string();
            let title = parser.title.clone();
            parser.push_head_node("title", attrs, title);
        }
        "link" => {
            let rel = attrs.get("rel").map(|s| s.as_str()).unwrap_or("");
            let media = attrs.get("media").map(|s| s.as_str()).unwrap_or("");
            let disabled = attrs.contains_key("disabled");
            let is_print_only = media.eq_ignore_ascii_case("print");
            // Don't fire hook for print-only stylesheets — they
            // shouldn't be fetched/applied in screen rendering.
            if !(rel.eq_ignore_ascii_case("stylesheet") && (is_print_only || disabled)) {
                parser.fire_hook(tag, &attrs);
            }
            let href = attrs.get("href").cloned().unwrap_or_default();
            let media_owned = media.to_string();
            let want_sheet =
                rel.eq_ignore_ascii_case("stylesheet") && !disabled && !href.is_empty();
            parser.push_head_node("link", attrs, String::new());
            if want_sheet {
                parser.linked_stylesheets.push((href, media_owned));
            }
        }
        "meta" | "base" => {
            parser.fire_hook(tag, &attrs);
            parser.push_head_node(tag, attrs, String::new());
        }
        "template" => {
            parser.fire_hook(tag, &attrs);
            // The content is parsed and KEPT. `<template>` is head content, and
            // its children are a staged fragment — skipping the subtree the way
            // an unknown head tag is skipped threw the fragment away and let its
            // contents leak into the body.
            let mut node = parser.new_box("template");
            node.attributes = attrs;
            apply_property(std::sync::Arc::make_mut(&mut node.style), "display", "none");
            if !self_closing {
                let mut children = Vec::new();
                let mut ol = 0i32;
                parser.parse_children_into("template", &mut children, &mut ol);
                node.children = children;
            }
            parser.head_children.push(node);
        }
        _ => return false,
    }
    true
}
