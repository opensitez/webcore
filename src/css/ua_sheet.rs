//! The user-agent stylesheet.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── User-Agent Stylesheet ───────────────────────────────────────────────────

pub fn ua_stylesheet() -> Stylesheet {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(UA_CSS);
    ss
}

const UA_CSS: &str = r##"
/* HTML §15.3.1 — hidden elements. */
head, link, meta, script, style, title { display: none; }
area, base, basefont, datalist, noembed, noframes, param, rp, source, track, template { display: none; }
picture { display: contents; }
/* `<slot>` is a projection point, not a box: its assigned nodes lay out as if
   they were children of the slot's parent. Without this it defaulted to
   `inline` and wrapped every projected block in an inline box. */
slot { display: contents; }
/* `hidden` hides everything EXCEPT `hidden=until-found`, which stays in the
   layout so find-in-page can reveal it, and `<embed>`, which the spec keeps
   loaded at zero size because plugins historically had side effects. */
[hidden]:not([hidden=until-found i]):not(embed) { display: none; }
[hidden=until-found i]:not(embed) { content-visibility: hidden; }
embed[hidden] { display: inline; height: 0; width: 0; }
/* Scripting is enabled, so `<noscript>` never renders. `!important` because
   the spec says so: a page must not be able to reveal its no-script fallback
   with a stylesheet. */
noscript { display: none !important; }
html { display: block; }
body { display: block; margin: 8px; }
article, aside, nav, section { display: block; }
h1 { display: block; font-size: 2em; font-weight: bold; margin-top: 0.67em; margin-bottom: 0.67em; break-after: avoid; break-inside: avoid; }
article h1, aside h1, nav h1, section h1 { font-size: 1.5em; margin-top: 0.83em; margin-bottom: 0.83em; }
article article h1, article aside h1, article nav h1, article section h1, aside article h1, aside aside h1, aside nav h1, aside section h1, nav article h1, nav aside h1, nav nav h1, nav section h1, section article h1, section aside h1, section nav h1, section section h1 { font-size: 1.17em; margin-top: 1em; margin-bottom: 1em; }
h2 { display: block; font-size: 1.5em; font-weight: bold; margin-top: 0.83em; margin-bottom: 0.83em; break-after: avoid; break-inside: avoid; }
h3 { display: block; font-size: 1.17em; font-weight: bold; margin-top: 1em; margin-bottom: 1em; break-after: avoid; break-inside: avoid; }
h4 { display: block; font-size: 1em; font-weight: bold; margin-top: 1.33em; margin-bottom: 1.33em; break-after: avoid; break-inside: avoid; }
h5 { display: block; font-size: 0.83em; font-weight: bold; margin-top: 1.67em; margin-bottom: 1.67em; break-after: avoid; break-inside: avoid; }
h6 { display: block; font-size: 0.67em; font-weight: bold; margin-top: 2.33em; margin-bottom: 2.33em; break-after: avoid; break-inside: avoid; }
hgroup { display: block; }
div, header, footer, main, search { display: block; }
form { display: block; }
p  { display: block; margin-top: 1em; margin-bottom: 1em; }
address { display: block; font-style: italic; }
blockquote { display: block; margin-top: 1em; margin-bottom: 1em; margin-left: 40px; margin-right: 40px; }
center { display: block; text-align: center; }
figure { display: block; margin-top: 1em; margin-bottom: 1em; margin-left: 40px; margin-right: 40px; }
figcaption { display: block; }
details { display: block; }
summary { display: list-item; list-style-type: disclosure-closed; }
/* A closed `<details>` shows only its summary (HTML §4.11.1). Said in CSS so
   it holds after ANY cascade — `apply_details_summary_post_cascade` runs at
   parse time only, so toggling `open` at runtime was undone by the next
   restyle. Phrased as "hide when closed" rather than "show when open" on
   purpose: a revealed child keeps its own `display`, and a rule that forced
   `block` would turn a revealed `<span>` into one. */
details:not([open]) > *:not(summary) { display: none; }
/* HTML §15.3.3 — `<dialog>`.
   A dialog is a block that is HIDDEN until it is open, and it is the UA sheet
   that says so. `show()`/`close()` used to write `display` as an INLINE style
   to compensate for this rule being absent; that made a closed dialog render
   in flow whenever it had never been opened, and made an opened one immune to
   the author's own `display`. The rule below is the whole mechanism, so those
   two writes are gone from `show_dialog`/`close_dialog`. */
dialog:not([open]) { display: none; }
dialog {
  display: block;
  position: absolute;
  left: 0; right: 0;
  width: fit-content;
  height: fit-content;
  margin: auto;
  border-width: 1px; border-style: solid; border-color: black;
  padding: 1em;
  background-color: white;
  color: black;
}
/* A MODAL is laid out against the viewport. This used to be an INLINE style
   written by `show_dialog`, which made an author's own `position` on a modal
   unbeatable — the exact bug the comment above describes fixing for the
   non-modal case. It is a rule now because `:modal` finally has an answer. */
dialog:modal { position: fixed; }
/* Popovers (HTML §6.12). Same shape as `dialog`: the UA sheet owns `display`
   and the positioning, so `showPopover`/`hidePopover` only move the element in
   and out of the top layer. Values measured off Chrome's own computed style. */
[popover] {
  position: fixed;
  inset: 0;
  width: fit-content;
  height: fit-content;
  margin: auto;
  border-width: 1px; border-style: solid; border-color: black;
  padding: 0.25em;
  overflow: auto;
  background-color: white;
  color: black;
}
[popover]:not(:popover-open) { display: none; }
pre, listing, plaintext, xmp { display: block; font-family: monospace; white-space: pre; margin-top: 1em; margin-bottom: 1em; }
hr  { display: block; margin-top: 0.5em; margin-bottom: 0.5em; margin-left: auto; margin-right: auto; height: 0; border-top-width: 1px; border-top-style: solid; border-top-color: silver; overflow: hidden; }
dl, ol, ul, menu, dir { display: block; margin-top: 1em; margin-bottom: 1em; }
ol, ul, menu { padding-left: 40px; }
menu { list-style-type: disc; }
dir  { list-style-type: disc; padding-left: 40px; }
dd, dt { display: block; }
dd { margin-left: 40px; }
li { display: list-item; }
ol { list-style-type: decimal; }
ul { list-style-type: disc; }
ul ul, ul ol, ul menu, ol ul, ol ol, ol menu, menu ul, menu ol, menu menu,
dir ul, dir ol, dir menu, dir dir { margin-top: 0; margin-bottom: 0; }
ul ul, ol ul, menu ul { list-style-type: circle; }
ul ul ul, ul ol ul, ol ul ul, ol ol ul, menu ul ul { list-style-type: square; }
cite, dfn, em, i, var { font-style: italic; }
b, strong { font-weight: bold; }
code, kbd, samp, tt { font-family: monospace; }
small { font-size: 0.83em; }
big  { font-size: 1.17em; }
sub  { vertical-align: sub; font-size: 0.83em; line-height: normal; }
sup  { vertical-align: super; font-size: 0.83em; line-height: normal; }
mark { background-color: yellow; color: black; }
a { color: #0000ee; text-decoration: underline; cursor: pointer; }
:visited { color: #551a8b; }
u, ins { text-decoration: underline; }
s, strike, del { text-decoration: line-through; }
abbr[title], acronym[title] { text-decoration: underline dotted; }
q::before { content: open-quote; }
q::after  { content: close-quote; }
nobr { white-space: nowrap; }
wbr  { display: inline; }
br { display: inline; }
img, svg { display: inline-block; break-inside: avoid; }
canvas, video, audio { display: inline-block; }
iframe { display: inline-block; border: 2px inset; }
output { display: inline; }
table { display: table; border-collapse: separate; border-spacing: 2px; box-sizing: border-box; }
caption { display: table-caption; text-align: center; }
colgroup { display: table-column-group; }
col { display: table-column; }
thead { display: table-header-group; }
tbody { display: table-row-group; }
tfoot { display: table-footer-group; }
tr    { display: table-row; }
td, th { display: table-cell; padding: 1px; vertical-align: middle; }
/* ⛔ ON THE CELL, not only on the row. Browsers write `vertical-align:
   middle` on `tr` and `vertical-align: inherit` on the cell, but
   `vertical-align` is NOT an inherited property, so a rule on the row
   alone never reaches `td`/`th` and every cell in a row taller than its
   own content sat flush to the top. */
th { font-weight: bold; text-align: center; }
thead, tbody, tfoot, tr { vertical-align: middle; }
/* A `<form>` that the tree builder left sitting between table rows is not
   rendered at all (HTML §15.3.9). The form is still in the DOM and still owns
   its controls — it just has no box, which is why this is `display` and not a
   parser rule. */
:is(table, thead, tbody, tfoot, tr) > form { display: none !important; }
/* `hidden` on a table part has to beat that part's own `display`. `[hidden]`
   is one class-level higher than a bare tag, so the general rule already wins;
   these exist because `!important` on the table `display` values in some
   sheets does not lose to it. Kept explicit so the intent survives. */
tbody[hidden], thead[hidden], tfoot[hidden], tr[hidden], col[hidden], colgroup[hidden] { display: none; }
button, input[type=submit], input[type=button], input[type=reset] {
  display: inline-flex; align-items: center; justify-content: center;
  padding: 1px 6px; cursor: default; background-color: #e8e8e8; border: 1px solid #767676;
  white-space: nowrap; border-radius: 3px;
}
button:hover, input[type=submit]:hover, input[type=button]:hover, input[type=reset]:hover {
  background-color: #e0e0e0; border-color: #666;
}
input:focus, select:focus, textarea:focus {
  border-color: #4285f4;
}
input:disabled, select:disabled, textarea:disabled, button:disabled {
  opacity: 0.6; cursor: default;
}
input[type=hidden i] { display: none !important; }
/* An image button IS an image (HTML §4.10.5.1.19), so it takes the box an
   `<img>` takes — not the 200px text-field default the bare `input` rule
   gives it, which squashed every image into a field-shaped strip. `width`
   and `height` on the element still win, as they do on an `<img>`. */
input[type=image] { display: inline-block; width: auto; height: auto; border: none; padding: 0; background-color: transparent; }
input[type=radio], input[type=checkbox] { display: inline-block; width: 16px; height: 16px; vertical-align: middle; margin: 0 6px 0 2px; border: none; padding: 0; background: transparent; flex-shrink: 0; }
label { display: inline-block; }
input { display: inline-block; width: 200px; height: 2.2em; padding: 0 6px; border: 1px solid #ababab; border-radius: 3px; box-sizing: border-box; vertical-align: middle; background-color: #ffffff; color: #000000; }
/* A button input's height is its LABEL's line box plus the padding and border
   — it has no children to give it one, so `height: auto` collapsed it to a
   sliver with the word sitting outside. `calc` rather than a fixed px so it
   still tracks the font, and `width: auto` keeps the intrinsic width the
   label measures. */
input[type=submit], input[type=button], input[type=reset] { width: auto; height: calc(1.2em + 8px); border: 1px solid #767676; padding: 3px 8px; background-color: #e8e8e8; }
select { display: inline-block; width: 200px; padding: 0 6px; border: 1px solid #ababab; border-radius: 3px; box-sizing: border-box; vertical-align: middle; background-color: #ffffff; color: #000000; }
/* A CLOSED select is one row tall. A list box — `size` above one, or
   `multiple` — is as tall as its rows, and that height depends on a NUMBER,
   which CSS cannot express: it arrives as a presentational hint from the
   `size` attribute. The hint has to be the only thing setting the height, so
   this rule must not match a list box. */
select:not([size]):not([multiple]) { height: 2.2em; }
option, optgroup { display: none; }
textarea { display: inline-block; white-space: pre-wrap; width: 200px; height: 3em; padding: 2px; border: 1px solid #767676; box-sizing: border-box; }
input[type=range] { width: 160px; height: 1.2em; border: none; padding: 0; }
input[type=color] { width: 44px; height: 23px; padding: 1px 2px; border: 1px solid #767676; box-sizing: border-box; }
input[type=file] { width: 240px; height: 1.6em; border: none; padding: 0; }
input[type=date], input[type=time], input[type=datetime-local], input[type=month], input[type=week] {
  width: 160px; height: 1.4em; padding: 1px 2px; border: 1px solid #767676; box-sizing: border-box;
}
progress { display: inline-block; width: 160px; height: 16px; vertical-align: middle; }
meter { display: inline-block; width: 80px; height: 16px; vertical-align: middle; }
output { display: inline; }
fieldset { display: block; margin-left: 2px; margin-right: 2px; padding-top: 0.35em; padding-bottom: 0.625em; padding-left: 0.75em; padding-right: 0.75em; border: 2px groove #ccc; }
legend { padding-left: 2px; padding-right: 2px; }
bdo { unicode-bidi: bidi-override; }
bdi { unicode-bidi: isolate; }
ruby { display: ruby; }
rt   { display: ruby-text; font-size: 0.5em; }
:focus-visible {
  outline-width: 2px;
  outline-style: solid;
  outline-color: #005fcc;
  outline-offset: 2px;
}
"##;

/// Apply a shadow stylesheet's `:host`, `:host(sel)` and `:host-context(sel)`
/// rules to the host element.
///
/// Specificity order is preserved; these land on top of the document's own
/// cascade for the host, which is where the spec puts them — a shadow tree's
/// `:host` rule is weaker than an author rule targeting the host from outside
/// only by origin, and both are author origin here, so later wins.
pub(crate) fn apply_host_rules(
    host: &mut WebCore,
    shadow_sheet: &Stylesheet,
    vars: &HashMap<String, String>,
    ancestors: &[AncestorInfo],
) {
    let mut matched: Vec<(u32, usize)> = Vec::new();
    for (ri, rule) in shadow_sheet.rules.iter().enumerate() {
        if rule.pseudo_element != PseudoElement::None {
            continue;
        }
        for sel in &rule.selectors {
            let Some(inner) = host_selector_argument(sel) else {
                continue;
            };
            // `:host` alone matches unconditionally; `:host(sel)` and
            // `:host-context(sel)` match when the host itself matches `sel`.
            let hit = match inner {
                HostArg::Bare => true,
                HostArg::Host(arg) => matches_bare(arg, &host.tag, &host.attributes, Some(host)),
                // `:host-context(sel)` matches when the host OR ANY ANCESTOR
                // matches `sel` — that is the whole point of it, and matching
                // only the host made `:host-context(.dark)` inside
                // `<body class=dark>` do nothing.
                HostArg::Context(arg) => {
                    matches_bare(arg, &host.tag, &host.attributes, Some(host))
                        || ancestors
                            .iter()
                            .any(|a| matches_bare(arg, &a.tag, &a.attributes, None))
                }
            };
            if hit {
                matched.push((rule.specificity, ri));
                break;
            }
        }
    }
    if matched.is_empty() {
        return;
    }
    matched.sort_by_key(|(sp, _)| *sp);
    for (_, ri) in &matched {
        for (prop, val) in &shadow_sheet.rules[*ri].declarations {
            let resolved = resolve_var_references(val, vars);
            apply_property(std::sync::Arc::make_mut(&mut host.style), prop, &resolved);
        }
    }
    for (_, ri) in &matched {
        for (prop, val) in &shadow_sheet.rules[*ri].important_declarations {
            let resolved = resolve_var_references(val, vars);
            apply_property(std::sync::Arc::make_mut(&mut host.style), prop, &resolved);
        }
    }
}

/// If `sel` is a `:host` form, return its argument: `None` for bare `:host`,
/// `Some(arg)` for `:host(arg)` / `:host-context(arg)`. `None` overall when the
/// selector is not a host selector at all.
fn host_selector_argument(sel: &CssSelector) -> Option<HostArg<'_>> {
    if sel
        .parts
        .iter()
        .any(|part| matches!(part, SelectorPart::Combinator(_)))
    {
        return None;
    }
    for part in &sel.parts {
        if let SelectorPart::PseudoClass(name) = part {
            if name == "host" {
                return Some(HostArg::Bare);
            }
            if let Some(rest) = name.strip_prefix("host(") {
                return Some(HostArg::Host(rest.trim_end_matches(')')));
            }
            if let Some(rest) = name.strip_prefix("host-context(") {
                return Some(HostArg::Context(rest.trim_end_matches(')')));
            }
        }
    }
    None
}

/// Which `:host` form a selector uses, and its argument.
enum HostArg<'a> {
    /// `:host`
    Bare,
    /// `:host(sel)` — the host itself must match `sel`.
    Host(&'a str),
    /// `:host-context(sel)` — the host OR AN ANCESTOR must match `sel`.
    Context(&'a str),
}

/// Match a bare compound selector against one element's tag and attributes.
fn matches_bare(
    sel: &str,
    tag: &str,
    attrs: &crate::dom::attrs::AttrMap,
    node: Option<&WebCore>,
) -> bool {
    let parsed = parse_selector(sel);
    if parsed.parts.is_empty() {
        return false;
    }
    let empty = std::collections::HashSet::new();
    let ctx = MatchContext {
        focused_box: 0,
        keyboard_focus: false,
        type_child_index: 0,
        type_sibling_count: 1,
        html_box: node,
        hover_chain: &empty,
        element_id: node.map(|n| n.node_id).unwrap_or(0),
        scope_root_id: 0,
        target_id: 0,
        document_url: "",
        prev_siblings: &[],
        next_siblings: &[],
    };
    parsed
        .parts
        .iter()
        .all(|p| matches_part_with_context(p, tag, attrs, 0, 1, &[], &ctx))
}
