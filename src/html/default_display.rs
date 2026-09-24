//! The UA's default `display` per tag.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Default display ────────────────────────────────────────────────────────

pub fn default_display(tag: &str) -> &'static str {
    match tag {
        "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "dl" | "dt"
        | "dd" | "pre" | "blockquote" | "hr" | "section" | "article" | "aside" | "nav"
        | "header" | "footer" | "main" | "address" | "figure" | "figcaption" | "details"
        | "center" | "form" | "fieldset" | "legend" | "hgroup" | "search" | "dialog" => "block",
        // A projection point, not a box — its assigned nodes lay out as if they
        // were children of the slot's parent (HTML §15.3.4).
        "slot" => "contents",
        "summary" => "list-item",
        "li" => "list-item",
        "table" => "table",
        "tr" => "table-row",
        "td" => "table-cell",
        "th" => "table-cell",
        "thead" | "tbody" | "tfoot" => "table-row-group",
        "col" => "table-column",
        "colgroup" => "table-column-group",
        "caption" => "table-caption",
        "img" | "svg" | "canvas" | "video" | "audio" => "inline-block",
        "input" | "select" | "textarea" => "inline-block",
        "button" => "inline-flex",
        "ruby" => "ruby",
        "rt" => "ruby-text",
        // Non-visual: display:none. `<noscript>` is intentionally not here:
        // webcore does not execute JavaScript, so body fallback content renders.
        "head" | "style" | "script" | "title" | "meta" | "link" | "option" | "optgroup"
        | "datalist" | "track" => "none",
        // Everything else is inline
        _ => "inline",
    }
}
