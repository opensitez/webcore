//! `parse_html` and the other public entry points.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Parse an HTML string into a Document.
/// Always produces: root = `html` with `body` child.
/// `base_url` is used to resolve relative image paths (pass "" for embedded HTML).
pub fn parse_html(html: &str) -> Document {
    parse_html_with_base(html, "")
}

/// Like `parse_html` but also accepts a base URL/path for resolving relative resources.
pub fn parse_html_with_base(html: &str, base_url: &str) -> Document {
    parse_html_with_hooks(html, base_url, |_, _| {})
}

/// Parse HTML with a host-registered tag hook.
///
/// `hook` is called for **every open tag** as it is parsed, receiving the tag
/// name (lowercase) and its attribute map.  The hook fires before the engine
/// processes the tag, so it is safe to kick off background work (e.g. fetching
/// a stylesheet referenced by a `<link>` tag) while the rest of the document
/// continues to parse.
///
/// ```ignore
/// let doc = parse_html_with_hooks(html, base_url, |tag, attrs| {
///     if tag == "link" && attrs.get("rel").map(|s| s == "stylesheet").unwrap_or(false) {
///         if let Some(href) = attrs.get("href") {
///             start_css_fetch(href);
///         }
///     }
/// });
/// ```
pub fn parse_html_with_hooks<F>(html: &str, base_url: &str, hook: F) -> Document
where
    F: FnMut(&str, &crate::dom::attrs::AttrMap) + 'static,
{
    parse_html_full(html, base_url, Some(Box::new(hook)), None, true)
}

pub(crate) fn parse_html_with_hooks_defer_cascade<F>(
    html: &str,
    base_url: &str,
    hook: F,
) -> Document
where
    F: FnMut(&str, &crate::dom::attrs::AttrMap) + 'static,
{
    parse_html_full(html, base_url, Some(Box::new(hook)), None, false)
}

/// Parse HTML with both a tag hook and a script/noscript callback.
///
/// `on_script` is called for every `<script>` and `<noscript>` tag with
/// `(tag_name, attrs, raw_content)`.  Return `true` if your host handled it
/// (e.g. executed the script).  Return `false` to let the engine apply the
/// default: `<noscript>` fallback content is parsed as HTML and shown,
/// `<script>` is discarded.
///
/// ```ignore
/// let doc = parse_html_with_scripts(html, base_url,
///     |tag, attrs| { /* open-tag hook */ },
///     |tag, attrs, content| {
///         if tag == "script" { my_js_engine.eval(content); true }
///         else { false } // let engine show noscript fallback
///     },
/// );
/// ```
pub fn parse_html_with_scripts<F, S>(html: &str, base_url: &str, hook: F, on_script: S) -> Document
where
    F: FnMut(&str, &crate::dom::attrs::AttrMap) + 'static,
    S: FnMut(&str, &crate::dom::attrs::AttrMap, &str) -> bool + 'static,
{
    parse_html_full(
        html,
        base_url,
        Some(Box::new(hook)),
        Some(Box::new(on_script)),
        true,
    )
}

fn parse_html_full(
    html: &str,
    base_url: &str,
    on_open_tag: Option<Box<dyn FnMut(&str, &crate::dom::attrs::AttrMap) + 'static>>,
    on_script: Option<Box<dyn FnMut(&str, &crate::dom::attrs::AttrMap, &str) -> bool + 'static>>,
    initial_cascade: bool,
) -> Document {
    // SVG blocks are now handled inline by the tokenizer/parser — no pre-pass needed.
    let tokens = tokenize(html);
    let mut parser = HtmlParser::new(tokens);
    parser.base_url = base_url.to_string();
    parser.on_open_tag = on_open_tag;
    parser.on_script = on_script;

    // Always create html > body structure
    let mut html_box = parser.new_box("html");
    apply_property(
        std::sync::Arc::make_mut(&mut html_box.style),
        "display",
        "block",
    );

    let mut body_box = parser.new_box("body");
    apply_property(
        std::sync::Arc::make_mut(&mut body_box.style),
        "display",
        "block",
    );

    let mut body_children: Vec<WebCore> = Vec::new();
    let mut ol_counter = 0i32;

    while parser.pos < parser.tokens.len() {
        match parser.tokens.get(parser.pos).cloned() {
            None => break,
            Some(Token::Doctype(dt)) => {
                // ⛔ THIS is the loop a real document's doctype reaches — it
                // sits before `<html>`, so the element-level arm never sees
                // it. Only the FIRST counts; a second anywhere is a parse
                // error the spec ignores.
                if parser.doctype.is_none() {
                    parser.doctype = Some(dt);
                }
                parser.pos += 1;
            }
            Some(Token::Comment(data)) => {
                // A comment at document level is a NODE like any other. It was
                // dropped here even after comments became nodes elsewhere, so
                // `<p>x</p><!--c-->` lost its trailing comment.
                parser.pos += 1;
                // A comment BEFORE `<html>` is a child of the Document, not of
                // the body — and this tree has no document node to hold it, so
                // it is dropped rather than moved somewhere it does not belong.
                if parser.head_closed {
                    let mut node = parser.new_box("#comment");
                    node.text = data;
                    apply_property(std::sync::Arc::make_mut(&mut node.style), "display", "none");
                    body_children.push(node);
                }
            }
            Some(Token::Text(t)) => {
                parser.pos += 1;
                let collapsed = collapse_whitespace(&t);
                if !collapsed.trim().is_empty() {
                    // Non-blank text is content, so it closes the head too.
                    // Blank text does NOT — whitespace is ignored in "before
                    // head", which is why this sits inside the guard.
                    parser.head_closed = true;
                    let mut node = parser.new_box("#text");
                    node.text = collapsed;
                    let had_pending = !parser.pending_format.is_empty();
                    let from = body_children.len();
                    body_children.push(node);
                    parser.reconstruct_into(&mut body_children, from, had_pending);
                }
            }
            Some(Token::CloseTag { tag }) => {
                parser.pos += 1;
                if tag == "html" || tag == "body" {
                    break;
                }
                // `</p>` with nothing open inserts an empty paragraph — the
                // same rule as inside an element, and documents hit it at top
                // level too (`<p>a</p></p>` ends with an empty `<p>`).
                if tag == "p" {
                    parser.head_closed = true;
                    let mut node = parser.new_box("p");
                    apply_property(
                        std::sync::Arc::make_mut(&mut node.style),
                        "display",
                        default_display("p"),
                    );
                    body_children.push(node);
                }
            }
            Some(Token::OpenTag {
                tag,
                attrs,
                self_closing,
            }) => {
                parser.pos += 1;
                match tag.as_str() {
                    "html" => {
                        // Apply attrs to html box
                        html_box.attributes = attrs;
                        apply_property(
                            std::sync::Arc::make_mut(&mut html_box.style),
                            "display",
                            "block",
                        );
                        apply_presentational_attrs(&mut html_box);
                        // Continue parsing html children
                        if !self_closing {
                            parse_html_children(
                                &mut parser,
                                &mut html_box,
                                &mut body_box,
                                &mut body_children,
                                &mut ol_counter,
                            );
                        }
                    }
                    "head" => {
                        if !parser.head_closed {
                            if !self_closing {
                                parse_head_content(&mut parser);
                            }
                            parser.head_closed = true;
                        }
                        // else: parse error, token ignored. Whatever follows
                        // is body content and falls through to the arms below
                        // on the next iteration.
                    }
                    "body" => {
                        parser.head_closed = true;
                        // Apply attrs to body box
                        body_box.attributes = attrs;
                        apply_property(
                            std::sync::Arc::make_mut(&mut body_box.style),
                            "display",
                            "block",
                        );
                        apply_presentational_attrs(&mut body_box);
                        // Parse body children
                        if !self_closing {
                            parser.parse_children_into("body", &mut body_children, &mut ol_counter);
                        }
                    }
                    // Head content before any body content belongs to the
                    // IMPLIED head — see the matching arm in
                    // `parse_html_children`. Most of the example pages open
                    // with a bare `<style>` and no `<head>` at all, and it was
                    // landing in the body.
                    _ if !parser.head_closed && is_head_content_tag(&tag) => {
                        handle_head_tag(&mut parser, &tag, attrs, self_closing);
                    }
                    _ => {
                        // Content outside html/body goes into body, and ends
                        // the implied head.
                        parser.head_closed = true;
                        let had_pending = !parser.pending_format.is_empty();
                        let from = body_children.len();
                        parser.handle_tag(
                            tag,
                            attrs,
                            self_closing,
                            &mut body_children,
                            &mut ol_counter,
                        );
                        parser.reconstruct_into(&mut body_children, from, had_pending);
                    }
                }
            }
        }
    }

    body_box.children = body_children;

    // `<head>` is optional in the MARKUP and mandatory in the TREE. HTML
    // §13.2.6's "before head" insertion mode inserts one whether or not the
    // source wrote it, which is why `document.head` always answers an element
    // in a browser — and why it answered `None` here for every document.
    //
    // It takes its display from the UA table like every other element — head
    // is already listed there as `none`, so nothing new is being decided here.
    // The element exists so the DOM can name it; the box draws nothing and
    // takes no space. Its CONTENTS are still consumed by the parser as before
    // — the title is lifted into `Document.title`, stylesheets into the
    // cascade — so this adds the container the spec requires without changing
    // what is rendered.
    let mut head_box = parser.new_box("head");
    apply_property(
        std::sync::Arc::make_mut(&mut head_box.style),
        "display",
        default_display("head"),
    );
    head_box.children = std::mem::take(&mut parser.head_children);

    // §13.2.6.4.19 "in frameset" — a `<frameset>` REPLACES the body: the
    // document element gets `head` and `frameset`, and there is no body at all.
    // The frameset was landing inside a body, so `document.body` answered an
    // element a frameset document does not have and the frames were nested a
    // level too deep.
    let frameset = body_box.children.iter().position(|c| c.tag == "frameset");
    if let Some(at) = frameset {
        let mut fs = body_box.children.remove(at);
        apply_property(std::sync::Arc::make_mut(&mut fs.style), "display", "block");
        html_box.children = vec![head_box, fs];
    } else {
        html_box.children = vec![head_box, body_box];
    }

    // §13.2.6.4.9 — table structure. Runs on the finished tree rather than
    // inside the element stack because both halves need a table's PARENT:
    // foster parenting moves a node out to become the table's sibling, and it
    // has to happen before the rows are grouped or the stray content would be
    // sealed inside the `<tbody>` that grouping creates.
    normalize_tables(&mut html_box);
    unwrap_misplaced_table_parts(&mut html_box);

    // Wire arena parent-child relationships to mirror the WebCore tree.
    wire_arena_children(&mut parser.arena, &mut html_box);

    // Build combined stylesheet (UA + author)
    let mut stylesheet = ua_stylesheet();
    // Author rules must always win over UA rules regardless of selector
    // specificity — see `css::AUTHOR_ORIGIN_BOOST`.
    stylesheet.push_author_rules(parser.stylesheet.rules);
    for (k, v) in parser.stylesheet.variables {
        stylesheet.variables.insert(k, v);
    }
    stylesheet.raw_sources.extend(parser.stylesheet.raw_sources);
    stylesheet.keyframes.extend(parser.stylesheet.keyframes);
    // ⛔ At-rules travel with the rest. This merge carried rules, variables,
    // sources and keyframes and used to drop stylesheet-side metadata, so a web
    // font or counter style declared in an inline `<style>` was parsed, stored
    // on the parser's sheet, and then thrown away.
    stylesheet.font_faces.extend(parser.stylesheet.font_faces);
    stylesheet.page_rules.extend(parser.stylesheet.page_rules);
    stylesheet
        .counter_styles
        .extend(parser.stylesheet.counter_styles);
    for name in parser.stylesheet.layer_order {
        if !stylesheet.layer_order.iter().any(|n| *n == name) {
            stylesheet.layer_order.push(name);
        }
    }

    let title = parser.title.clone();
    let linked_stylesheets = parser.linked_stylesheets.clone();

    // The doctype becomes a real node so it has an id from the same space as
    // everything else, and `document.childNodes` can put it before `<html>`.
    let quirks = crate::html::doctype::quirks_mode(parser.doctype.as_ref());
    let doctype_id = match &parser.doctype {
        Some(dt) => {
            parser
                .arena
                .create_doctype(&dt.name, &dt.public_id, &dt.system_id)
                .0
        }
        None => 0,
    };

    let mut doc = Document {
        root: html_box,
        stylesheet,
        title,
        base_url: base_url.to_string(),
        arena: parser.arena,
        doctype: doctype_id,
        quirks,
        character_set: "UTF-8".to_string(),
        traversals: crate::dom::traversal::TraversalStore::new(),
        ranges: crate::dom::range::RangeStore::new(),
        top_layer: Vec::new(),
        suppress_range_updates: false,
        next_node_id: parser.next_node_id,
        node_index: std::collections::HashMap::new(),
        // This function IS the HTML parser, so whatever it produces is an HTML
        // document by construction.
        kind: crate::types::DocumentKind::Html,
        layout_store: crate::layout::layout_box::LayoutStore::new(),
        pending_nodes: std::collections::HashMap::new(),
        linked_stylesheets,
        document_stylesheets: parser.document_stylesheets,
        loaded_linked_stylesheets: std::collections::HashMap::new(),
        preserve_stylesheet_document_order: true,
        editor: crate::dom::Editor::new(),
        canvas_surfaces: crate::canvas::CanvasSurfaces::default(),
        media_states: crate::types::MediaStateMap::new(),
        event_targets: crate::dom::events::EventTargetMap::new(),
        scroll_x: 0.0,
        scroll_y: 0.0,
        scrollbar_drag: None,
        hovered_box: 0,
        hover_suppress_count: 0,
        active_box: 0,
        focused_box: 0,
        mousedown_target: 0,
        last_click_target: 0,
        last_click_time: None,
        drag_source: 0,
        drag_start_doc_pt: (0.0, 0.0),
        drag_active: false,
        visited_urls: std::collections::HashSet::new(),
        custom_validity: std::collections::HashMap::new(),
        viewport_w: 0.0,
        viewport_h: 0.0,
        keyboard_focus: false,
        caret_blink_epoch: std::time::Instant::now(),
        open_select: 0,
        open_picker: 0,
        dropdown_hover_idx: -1,
        // Transient interaction state, like the two popups beside it: a freshly
        // parsed document is holding nothing.
        dragging_range: 0,
        range_drag_origin: String::new(),
        active_animations: Vec::new(),
        transition_states: std::collections::HashMap::new(),
        prev_styles: std::collections::HashMap::new(),
        animation_overrides: std::collections::HashMap::new(),
        needs_animation_frame: false,
        smooth_scrolls: Vec::new(),
        hover_changed: false,
        hover_sensitive_nodes: std::collections::HashSet::new(),
        style_dirty: !initial_cascade,
        prev_hovered_box: 0,
        cascade_styles: std::collections::HashMap::new(),
        pending_announcements: Vec::new(),
        live_region_snapshots: std::collections::HashMap::new(),
        live_regions_initialized: false,
        layout_generation: 0,
        pending_images: None,
        images_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        pending_stylesheets: None,
        on_form_event: None,
        on_navigate: None,
        on_title_change: None,
        on_dom_mutation: None,
        on_visibility_change: None,
    };

    // NOTE: External CSS fetching, cascade, layout, and image loading are
    // handled by the caller (lib.rs load_html_with_registry) which does
    // parallel CSS fetching and batch image loading. We only do a minimal
    // cascade here for the standalone parse_html / parse_html_with_base paths.
    //
    // Fetch local-only linked stylesheets (file:// paths).
    // Remote stylesheets are handled by lib.rs in parallel.
    for (href, media) in &doc.linked_stylesheets.clone() {
        // Skip print-only stylesheets for screen rendering
        if media.eq_ignore_ascii_case("print") {
            continue;
        }
        let url = resolve_url(href, base_url);
        if !url.starts_with("http://") && !url.starts_with("https://") && !url.is_empty() {
            if let Ok(css_text) = std::fs::read_to_string(&url) {
                // A local `<link>` is still an AUTHOR sheet.
                doc.stylesheet.parse_and_add_with_base(&css_text, &url);
            }
        }
    }

    if initial_cascade {
        // Apply cascade (basic pass — lib.rs re-runs with viewport dimensions)
        let root_font_px = 16.0;
        doc.stylesheet.rebuild_index();
        let target_id = doc.fragment_target_id();
        apply_cascade_vp_hover_target_url(
            &mut doc.root,
            &doc.stylesheet,
            None,
            root_font_px,
            0.0,
            0.0,
            0,
            false,
            &std::collections::HashSet::new(),
            target_id,
            &doc.base_url,
        );
    }

    // Post-cascade fixes
    apply_details_summary_post_cascade(&mut doc.root);
    number_lists(&mut doc.root);
    // Standalone parse_html has no layout viewport. Use the same deterministic
    // default viewport the old parser path used for DOM current-source state;
    // browser loading re-resolves with the real viewport before fetching.
    resolve_picture_elements(&mut doc.root, &doc.base_url, 800.0, 600.0);
    doc.initialize_media_elements();

    doc
}
