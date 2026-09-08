//! The HTML tree-construction parser.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::types::*;

// ─── Parser ─────────────────────────────────────────────────────────────────

pub(crate) struct HtmlParser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) pos: usize,
    pub(crate) stylesheet: Stylesheet,
    pub(crate) title: String,
    pub(crate) base_url: String,
    pub(crate) linked_stylesheets: Vec<(String, String)>, // (href, media)
    /// Monotonically increasing counter for assigning stable node_ids.
    pub(crate) next_node_id: u32,
    /// Arena-based DOM being built in parallel with the WebCore tree.
    pub(crate) arena: crate::dom::arena::DomArena,
    /// Optional host-registered hook, fired for every open tag as it is parsed.
    /// Receives the tag name and its attribute map.
    pub(crate) on_open_tag: Option<Box<dyn FnMut(&str, &crate::dom::attrs::AttrMap) + 'static>>,
    /// Optional host callback for `<script>` and `<noscript>` tags.
    /// Receives (tag, attrs, raw_content) and returns true if the host handled it.
    /// If None or returns false: `<noscript>` content is parsed as HTML (shown to
    /// the user as fallback), `<script>` is discarded.
    pub(crate) on_script:
        Option<Box<dyn FnMut(&str, &crate::dom::attrs::AttrMap, &str) -> bool + 'static>>,
    /// Whether the head has been closed — the one bit of HTML §13.2.6's
    /// insertion-mode state this parser needs.
    ///
    /// The spec runs "before head" exactly ONCE. After the head closes, a
    /// `<head>` start tag is a parse error and the TOKEN is ignored; what
    /// follows keeps being parsed as body content. Without this the parser
    /// would happily re-enter head parsing halfway down a page and swallow
    /// markup that belongs in the body.
    pub(crate) head_closed: bool,
    /// The head's ELEMENT children, in source order.
    ///
    /// The head's contents were consumed and thrown away — the title lifted
    /// into `Document.title`, the stylesheets into the cascade — so `<head>`
    /// was an empty element and `querySelector("title")` answered nothing in a
    /// document that plainly had one. Consuming and keeping are not exclusive:
    /// the title still lands in `Document.title` and the CSS still reaches the
    /// cascade, and the nodes exist so the DOM can name them. Nothing here
    /// renders — `head` is `display: none` in the UA sheet, so its subtree
    /// never reaches layout.
    pub(crate) head_children: Vec<WebCore>,
    /// Formatting elements closed implicitly and waiting to be re-opened —
    /// HTML §13.2.4.3's "list of active formatting elements", reconstructed
    /// lazily when content arrives.
    ///
    /// On the PARSER rather than in `parse_children_into`, because a formatting
    /// element can be closed by an end tag that also ends the element that
    /// function was collecting children for. `<section><b>x</section>y` closes
    /// `<b>` and `<section>` together; the pending `<b>` has to survive the
    /// return so `y` is still bold at the level above.
    pub(crate) pending_format: Vec<WebCore>,
    /// The document's `<!DOCTYPE>`, if it had one. `None` is quirks mode —
    /// the commonest way a real page ends up there.
    pub(crate) doctype: Option<crate::html::doctype::Doctype>,
}

fn svg_dom_tag_name(node: &crate::svg::SvgNode) -> String {
    match &node.kind {
        crate::svg::SvgElementKind::Svg => "svg".to_string(),
        crate::svg::SvgElementKind::Group => "g".to_string(),
        crate::svg::SvgElementKind::Defs => "defs".to_string(),
        crate::svg::SvgElementKind::Symbol => "symbol".to_string(),
        crate::svg::SvgElementKind::Use => "use".to_string(),
        crate::svg::SvgElementKind::Path => "path".to_string(),
        crate::svg::SvgElementKind::Rect => "rect".to_string(),
        crate::svg::SvgElementKind::Circle => "circle".to_string(),
        crate::svg::SvgElementKind::Ellipse => "ellipse".to_string(),
        crate::svg::SvgElementKind::Line => "line".to_string(),
        crate::svg::SvgElementKind::Polyline => "polyline".to_string(),
        crate::svg::SvgElementKind::Polygon => "polygon".to_string(),
        crate::svg::SvgElementKind::Text => "text".to_string(),
        crate::svg::SvgElementKind::Tspan => "tspan".to_string(),
        crate::svg::SvgElementKind::TextPath => "textPath".to_string(),
        crate::svg::SvgElementKind::Image => "image".to_string(),
        crate::svg::SvgElementKind::LinearGradient => "linearGradient".to_string(),
        crate::svg::SvgElementKind::RadialGradient => "radialGradient".to_string(),
        crate::svg::SvgElementKind::ClipPath => "clipPath".to_string(),
        crate::svg::SvgElementKind::Mask => "mask".to_string(),
        crate::svg::SvgElementKind::Pattern => "pattern".to_string(),
        crate::svg::SvgElementKind::Marker => "marker".to_string(),
        crate::svg::SvgElementKind::Style => "style".to_string(),
        crate::svg::SvgElementKind::Unknown(name) => name.clone(),
    }
}

impl HtmlParser {
    pub(crate) fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            stylesheet: Stylesheet::default(),
            title: String::new(),
            base_url: String::new(),
            linked_stylesheets: Vec::new(),
            next_node_id: 1, // 0 = NodeId::NONE (reserved)
            arena: crate::dom::arena::DomArena::new(),
            on_open_tag: None,
            on_script: None,
            head_closed: false,
            head_children: Vec::new(),
            pending_format: Vec::new(),
            doctype: None,
        }
    }

    /// Record a head element (`title`/`meta`/`link`/`style`/`base`) as a node.
    /// `text` is the element's text content, empty for the void ones.
    /// Wrap freshly-appended nodes in the formatting elements waiting to be
    /// reconstructed, outermost first.
    ///
    /// The element stack in `parse_children_into` re-opens a pending formatting
    /// element by pushing a FRAME; a document-level loop has no stack, so it
    /// wraps instead. Same rule, same result for the shape that reaches here:
    /// `<section><b>x</section>y` leaves `y` inside a fresh `<b>`.
    /// `had_pending` must be sampled BEFORE the token was processed: a token
    /// can CREATE the pending entry itself — `</section>` in
    /// `<section><b>x</section>` closes the `<b>` — and the element it closed
    /// must not then be wrapped in a copy of it.
    pub(crate) fn reconstruct_into(
        &mut self,
        children: &mut Vec<WebCore>,
        from: usize,
        had_pending: bool,
    ) {
        if !had_pending || self.pending_format.is_empty() || children.len() <= from {
            return;
        }
        let inner: Vec<WebCore> = children.drain(from..).collect();
        let mut wrapped = inner;
        for tpl in std::mem::take(&mut self.pending_format).into_iter().rev() {
            let mut node = tpl;
            node.children = wrapped;
            wrapped = vec![node];
        }
        children.extend(wrapped);
    }

    pub(crate) fn push_head_node(
        &mut self,
        tag: &str,
        attrs: crate::dom::attrs::AttrMap,
        text: String,
    ) {
        let mut node = self.new_box(tag);
        node.attributes = attrs;
        node.text = text;
        apply_property(std::sync::Arc::make_mut(&mut node.style), "display", "none");
        self.head_children.push(node);
    }

    /// Create an WebCore with a fresh sequential node_id.
    /// Also creates the corresponding node in the arena.
    #[inline]
    pub(crate) fn new_box(&mut self, tag: &str) -> WebCore {
        let mut b = WebCore::new(tag);
        // ⛔ `#comment` needs its own arm. Routing it through `create_element`
        // made every PARSED comment an Element in the arena: `nodeType`
        // answered 1 instead of 8 and `nodeName` came back `"#COMMENT"`,
        // uppercased by the element rule. `document.createComment()` was
        // always right — only the parser's comments were not.
        let arena_id = match tag {
            "#text" => self.arena.create_text(""),
            "#comment" => self.arena.create_comment(""),
            _ => self.arena.create_element(tag),
        };
        // The arena assigns NodeId sequentially starting from 1, matching our counter.
        b.node_id = arena_id.0;
        // Keep next_node_id in sync (for non-parser code that may need to allocate)
        self.next_node_id = self.next_node_id.max(arena_id.0 + 1);
        b
    }

    fn project_inline_svg_children(&mut self, node: &mut WebCore) {
        let Some(doc) = node.svg_document.clone() else {
            return;
        };
        if !doc.root.text.trim().is_empty() {
            let mut text = self.new_box("#text");
            text.text = doc.root.text.clone();
            apply_property(std::sync::Arc::make_mut(&mut text.style), "display", "none");
            node.children.push(text);
        }
        for child in &doc.root.children {
            node.children.push(self.project_svg_node(child));
        }
    }

    fn project_svg_node(&mut self, svg: &crate::svg::SvgNode) -> WebCore {
        let mut node = self.new_box(&svg_dom_tag_name(svg));
        for attr in &svg.attributes {
            let name = match attr.namespace.as_deref() {
                Some(ns) if !ns.is_empty() => format!("{}:{}", ns, attr.name),
                _ => attr.name.clone(),
            };
            node.attributes.insert(name, attr.value.clone());
        }
        if !svg.text.trim().is_empty() {
            let mut text = self.new_box("#text");
            text.text = svg.text.clone();
            apply_property(std::sync::Arc::make_mut(&mut text.style), "display", "none");
            node.children.push(text);
        }
        for child in &svg.children {
            node.children.push(self.project_svg_node(child));
        }
        apply_property(std::sync::Arc::make_mut(&mut node.style), "display", "none");
        node
    }

    /// Fire the host hook (if any) for an open tag.
    #[inline]
    pub(crate) fn fire_hook(&mut self, tag: &str, attrs: &crate::dom::attrs::AttrMap) {
        if let Some(ref mut f) = self.on_open_tag {
            f(tag, attrs);
        }
    }

    /// Parse children until close tag matching `parent_tag` or EOF.
    /// If `parent_tag` is empty, parse until EOF.
    /// All resulting boxes are appended to the provided `children` vec.
    ///
    /// Iterative implementation using an explicit stack — handles arbitrarily
    /// deep nesting without risking a stack overflow.
    pub(crate) fn parse_children_into(
        &mut self,
        parent_tag: &str,
        children: &mut Vec<WebCore>,
        ol_counter: &mut i32,
    ) {
        // Each frame represents one nesting level.
        struct Frame {
            parent_tag: String,
            node: WebCore, // the element whose children we're collecting
            ol_counter: i32,
        }

        /// Re-open the formatting elements an end tag closed out from under.
        ///
        /// HTML §13.2.4.3's "reconstruct the active formatting elements", run
        /// LAZILY like the spec does: the templates sit in `pending` until real
        /// content arrives, so `<p><b>1<i>2</b></p>` does not gain a stray empty
        /// `<i>`. Outermost first, which is the order `pending` is built in.
        fn reconstruct(stack: &mut Vec<Frame>, pending: &mut Vec<WebCore>) {
            for tpl in pending.drain(..) {
                stack.push(Frame {
                    parent_tag: tpl.tag.clone(),
                    node: tpl,
                    ol_counter: 0,
                });
            }
        }

        // Bottom frame collects the top-level children that will be returned.
        let mut stack: Vec<Frame> = Vec::new();
        stack.push(Frame {
            parent_tag: parent_tag.to_string(),
            node: WebCore::new("__root__"), // temporary container, no arena node needed
            ol_counter: *ol_counter,
        });

        loop {
            let cur_tag = &stack.last().unwrap().parent_tag;
            let preserve_ws = matches!(
                cur_tag.as_str(),
                "pre" | "textarea" | "listing" | "xmp" | "plaintext"
            );

            match self.tokens.get(self.pos).cloned() {
                None => break, // EOF

                Some(Token::CloseTag { tag }) => {
                    // Find matching frame in the stack (like a browser's
                    // "adoption agency" — pop implicit close tags up to
                    // the matching ancestor, or ignore if truly stray).
                    let match_idx = stack.iter().rposition(|f| f.parent_tag == tag);

                    // The adoption agency's "furthest block" case: a BLOCK was
                    // opened inside a formatting element and is still open when
                    // the formatting element ends — `<b>1<p>2</b>3</p>`.
                    //
                    // Reconstruction is not enough here, because the block has
                    // to come OUT of the formatting element: the answer is
                    // `<b>1</b><p><b>2</b>3</p>`, where the block is now a
                    // SIBLING of `<b>` and carries a copy of it around the
                    // content it already had. Closing normally instead nested
                    // the block inside `<b>` and left `3` outside the `<p>`
                    // entirely.
                    let furthest_block = match_idx
                        .filter(|_| is_formatting_element(&tag))
                        .and_then(|idx| {
                            stack
                                .iter()
                                .enumerate()
                                .skip(idx + 1)
                                .find(|(_, f)| is_special_element(&f.parent_tag))
                                .map(|(i, _)| i)
                        });

                    if let (Some(idx), Some(fb_idx)) = (match_idx, furthest_block) {
                        // Everything above the block closes into it as usual.
                        while stack.len() > fb_idx + 1 {
                            let frame = stack.pop().unwrap();
                            if is_formatting_element(&frame.parent_tag) {
                                let mut fresh = self.new_box(&frame.parent_tag);
                                fresh.attributes = frame.node.attributes.clone();
                                fresh.style = frame.node.style.clone();
                                self.pending_format.insert(0, fresh);
                            }
                            let mut node = frame.node;
                            Self::post_process_node(&mut node, &self.base_url);
                            stack.last_mut().unwrap().node.children.push(node);
                        }
                        self.pos += 1;
                        let mut fb = stack.pop().unwrap();
                        // The block keeps its content, wrapped in a copy of the
                        // formatting element it used to be inside.
                        if !fb.node.children.is_empty() {
                            let fmt = &stack[idx].node;
                            let mut wrapper = self.new_box(&tag);
                            wrapper.attributes = fmt.attributes.clone();
                            wrapper.style = fmt.style.clone();
                            wrapper.children = std::mem::take(&mut fb.node.children);
                            fb.node.children = vec![wrapper];
                        }
                        // Anything between the formatting element and the block
                        // closes normally; then the formatting element itself.
                        while stack.len() > idx {
                            let frame = stack.pop().unwrap();
                            let mut node = frame.node;
                            Self::post_process_node(&mut node, &self.base_url);
                            if stack.is_empty() {
                                // The formatting element was the bottom frame —
                                // nothing to reparent into, so fall back to the
                                // ordinary close.
                                *children = node.children;
                                return;
                            }
                            stack.last_mut().unwrap().node.children.push(node);
                        }
                        // The block stays OPEN, now a sibling of what closed.
                        stack.push(fb);
                        continue;
                    }

                    if let Some(idx) = match_idx {
                        // Pop frames from top down to (and including) the match.
                        // Non-matching frames above the match are implicitly closed.
                        while stack.len() > idx + 1 {
                            let frame = stack.pop().unwrap();
                            // A FORMATTING element closed this way is not really
                            // over — it is re-opened after the end tag so the
                            // text that follows keeps its formatting. Popped
                            // innermost-first, so each goes at the FRONT to keep
                            // the original nesting order on the way back in.
                            if is_formatting_element(&frame.parent_tag) {
                                let mut fresh = self.new_box(&frame.parent_tag);
                                fresh.attributes = frame.node.attributes.clone();
                                fresh.style = frame.node.style.clone();
                                self.pending_format.insert(0, fresh);
                            }
                            let mut node = frame.node;
                            Self::post_process_node(&mut node, &self.base_url);
                            stack.last_mut().unwrap().node.children.push(node);
                        }
                        // Now pop the matching frame itself.
                        self.pos += 1;
                        let frame = stack.pop().unwrap();
                        if stack.is_empty() {
                            *children = frame.node.children;
                            *ol_counter = frame.ol_counter;
                            return;
                        }
                        let mut node = frame.node;
                        Self::post_process_node(&mut node, &self.base_url);
                        stack.last_mut().unwrap().node.children.push(node);
                    } else if tag == "p" {
                        // `</p>` with no open `<p>` is not ignored: the spec
                        // inserts an element for a `<p>` START tag and then
                        // closes it, so `<p><div>x</div></p>` ends with an
                        // EMPTY paragraph after the div. Every browser has it.
                        self.pos += 1;
                        let mut node = self.new_box("p");
                        apply_property(
                            std::sync::Arc::make_mut(&mut node.style),
                            "display",
                            default_display("p"),
                        );
                        stack.last_mut().unwrap().node.children.push(node);
                    } else {
                        // Stray close tag with no matching open — ignore it.
                        self.pos += 1;
                    }
                }

                Some(Token::Comment(data)) => {
                    // A comment is a NODE. It was dropped at the token, so
                    // `<div>a<!--x-->b</div>` reached the DOM as `ab` and no
                    // comment could ever be read back or serialised.
                    self.pos += 1;
                    let mut node = self.new_box("#comment");
                    node.text = data;
                    apply_property(std::sync::Arc::make_mut(&mut node.style), "display", "none");
                    stack.last_mut().unwrap().node.children.push(node);
                }

                Some(Token::Doctype(_)) => {
                    // ⛔ A doctype reaching ELEMENT content is a parse error
                    // and is ignored outright — measured: `<body><!DOCTYPE
                    // foo>` leaves `document.doctype` null and the mode
                    // quirks. Recording it here would invent a doctype the
                    // page does not have.
                    self.pos += 1;
                }

                Some(Token::Text(t)) => {
                    self.pos += 1;
                    let text_val = if preserve_ws {
                        if t.starts_with('\n') {
                            t[1..].to_string()
                        } else {
                            t
                        }
                    } else if t.trim().is_empty() && t.contains('\n') {
                        "\n".to_string()
                    } else {
                        collapse_whitespace(&t)
                    };
                    let keep = !text_val.trim().is_empty() || text_val == " " || text_val == "\n";
                    if keep {
                        // Content arriving is what makes a pending formatting
                        // element real — see `reconstruct`.
                        reconstruct(&mut stack, &mut self.pending_format);
                        let mut text_node = self.new_box("#text");
                        text_node.text = text_val;
                        stack.last_mut().unwrap().node.children.push(text_node);
                    }
                }

                Some(Token::OpenTag {
                    tag,
                    attrs,
                    self_closing,
                }) => {
                    // The BOTTOM frame is the element the CALLER already opened,
                    // so this call cannot pop it — closing it means returning and
                    // letting the caller finish the node and re-read this token at
                    // its own level. The auto-close loop further down only reaches
                    // frames this call pushed (`stack.len() > 1`), so without this
                    // the implicit close silently did not happen for the element a
                    // recursion started on: `<p>a<div>b` nested the div inside the
                    // p, and `<p>a<p>b` nested p in p. `<body><p>a<div>` was right
                    // only because the body arm gives the stack a second frame.
                    // That makes the bug fire on exactly the shape `set_inner_html`
                    // and every generated fragment produce — markup with no <body>.
                    if stack.len() == 1
                        && (should_auto_close(&stack[0].parent_tag, &tag)
                            || (stack[0].parent_tag == "select" && closes_select(&tag)))
                    {
                        break; // token NOT consumed; the caller sees it next
                    }
                    self.pos += 1;

                    // Script/noscript: give the host first chance to handle it.
                    // If the host doesn't handle it: noscript content is shown
                    // as fallback HTML, script is discarded.
                    if matches!(tag.as_str(), "script" | "noscript") {
                        let content = if !self_closing {
                            self.collect_raw_text_until(&tag)
                        } else {
                            String::new()
                        };
                        // The ELEMENT stays in the DOM with its source as a text
                        // child, whatever the host does with the content.
                        // Dropping it meant `document.scripts` was empty and a
                        // `<script>` could not be found, moved or re-read — and
                        // `<script>` is `display: none`, so nothing is drawn.
                        let mut script_node = self.new_box(&tag);
                        script_node.attributes = attrs.clone();
                        script_node.text = content.clone();
                        apply_property(
                            std::sync::Arc::make_mut(&mut script_node.style),
                            "display",
                            "none",
                        );
                        stack.last_mut().unwrap().node.children.push(script_node);
                        let host_handled = if let Some(ref mut f) = self.on_script {
                            f(&tag, &attrs, &content)
                        } else {
                            false
                        };
                        // Scripting is ENABLED, so `<noscript>` is RAWTEXT: its
                        // content is the text above and is NOT parsed. Parsing
                        // it as fallback markup is the scripting-DISABLED
                        // behaviour, and doing both put the same content in the
                        // tree twice — once as text, once as elements.
                        let parse_noscript_fallback = false;
                        if parse_noscript_fallback
                            && !host_handled
                            && tag == "noscript"
                            && !content.is_empty()
                        {
                            // Parse noscript content as HTML and insert into current frame
                            let inner_tokens = tokenize(&content);
                            let mut inner_parser = HtmlParser::new(inner_tokens);
                            inner_parser.base_url = self.base_url.clone();
                            let mut inner_children = Vec::new();
                            let mut inner_ol = 0i32;
                            inner_parser.parse_children_into(
                                "",
                                &mut inner_children,
                                &mut inner_ol,
                            );
                            for child in inner_children {
                                stack.last_mut().unwrap().node.children.push(child);
                            }
                            // Merge any stylesheets found inside noscript
                            for rule in inner_parser.stylesheet.rules {
                                self.stylesheet.rules.push(rule);
                            }
                        }
                        continue;
                    }

                    // SVG: collect raw markup and rasterize to an <img> node.
                    if tag == "svg" {
                        let svg_body = if !self_closing {
                            self.collect_raw_text_until("svg")
                        } else {
                            String::new()
                        };
                        let fallback = crate::svg::build_inline_svg_fallback(&attrs, &svg_body);

                        let mut node = self.new_box("svg");
                        node.attributes = attrs;
                        apply_property(
                            std::sync::Arc::make_mut(&mut node.style),
                            "display",
                            "inline-block",
                        );
                        node.svg_markup = Some(fallback.markup);
                        node.svg_document = node
                            .svg_markup
                            .as_deref()
                            .and_then(|markup| crate::svg::parse_svg_document(markup).ok());
                        node.svg_viewbox_w = fallback.viewbox_w;
                        node.svg_viewbox_h = fallback.viewbox_h;
                        self.project_inline_svg_children(&mut node);

                        // Only bake explicit HTML-attribute dimensions into the style.
                        // CSS cascade will override these. If no explicit dimensions,
                        // the layout engine uses svg_viewbox_w/h.
                        if let Some(w) = fallback.explicit_w {
                            apply_property(
                                std::sync::Arc::make_mut(&mut node.style),
                                "width",
                                &format!("{}px", w),
                            );
                        }
                        if let Some(h) = fallback.explicit_h {
                            apply_property(
                                std::sync::Arc::make_mut(&mut node.style),
                                "height",
                                &format!("{}px", h),
                            );
                        }

                        // Don't rasterize here — deferred to render time at the correct display size.
                        stack.last_mut().unwrap().node.children.push(node);
                        continue;
                    }

                    self.fire_hook(&tag, &attrs);

                    // ⛔ Register a `<link rel=stylesheet>` wherever it appears.
                    // The hook fires above, so a loader driven by the hook did
                    // fetch these — but `linked_stylesheets` was only filled by
                    // the `<head>` path, and a loader that walks THAT list saw
                    // just the head sheets. usps.com serves two of its eight in
                    // the head and six in the body, the navigation's among them,
                    // so the page rendered with a fraction of its CSS.
                    if tag == "link" {
                        let rel = attrs.get("rel").map(|s| s.as_str()).unwrap_or("");
                        let href = attrs.get("href").cloned().unwrap_or_default();
                        if rel.eq_ignore_ascii_case("stylesheet")
                            && !attrs.contains_key("disabled")
                            && !href.is_empty()
                        {
                            let media = attrs.get("media").cloned().unwrap_or_default();
                            if !self.linked_stylesheets.iter().any(|(h, _)| *h == href) {
                                self.linked_stylesheets.push((href, media));
                            }
                        }
                    }

                    // Skip non-visual tags entirely
                    if is_non_visual(&tag) {
                        if !self_closing {
                            self.skip_until_close(&tag);
                        }
                        continue;
                    }
                    // Style block: the CSS goes to the cascade AND the element
                    // stays in the tree.
                    //
                    // Only a `<style>` inside `<template shadowrootmode>` used
                    // to become a node, so `querySelector("style")` answered
                    // something in a shadow template and nothing on an ordinary
                    // page — the same element, present or absent depending on
                    // where it was written. It is `display: none` in the UA
                    // sheet, so keeping it changes nothing that is drawn.
                    //
                    // The template case still skips the cascade: those rules
                    // are scoped to the shadow root and `post_process_node`
                    // lifts them into its stylesheet.
                    if tag == "style" {
                        // The NODE keeps the author's source; only the CASCADE
                        // sees the normalized copy. Storing the normalized text
                        // on the node meant `styleEl.textContent` came back
                        // rewritten — an author's `18pt` read as `24.0000px` —
                        // and a serialize/reparse round-trip rewrote the page's
                        // own stylesheet.
                        let css = self.collect_raw_text_until("style");
                        let cur_parent = stack
                            .last()
                            .map(|f| f.parent_tag.as_str())
                            .unwrap_or(parent_tag);
                        if cur_parent != "template" {
                            self.stylesheet.parse_and_add(&normalize_css_text(&css));
                        }
                        let mut style_node = self.new_box("style");
                        style_node.text = css;
                        apply_property(
                            std::sync::Arc::make_mut(&mut style_node.style),
                            "display",
                            "none",
                        );
                        stack.last_mut().unwrap().node.children.push(style_node);
                        continue;
                    }
                    // Title. A `<title>` met outside the head is a parse error
                    // that the spec processes "using the rules for the in head
                    // insertion mode" — so it belongs to the head element, not
                    // to wherever it was written.
                    if tag == "title" {
                        let text = self.collect_raw_text_until("title");
                        self.title = text.trim().to_string();
                        let title = self.title.clone();
                        self.push_head_node("title", attrs, title);
                        continue;
                    }

                    // Build the node
                    let mut node = self.new_box(&tag);
                    node.attributes = attrs;
                    apply_property(
                        std::sync::Arc::make_mut(&mut node.style),
                        "display",
                        default_display(&tag),
                    );
                    apply_presentational_attrs(&mut node);

                    // <img> handling
                    if tag == "img" {
                        // If srcset is present, pick the best URL and override src
                        if let Some(srcset) = node.attributes.get("srcset").cloned() {
                            if let Some(best) = parse_srcset_url(&srcset) {
                                node.attributes.insert("src".to_string(), best);
                            }
                        }
                        if let Some(src) = node.attributes.get("src").cloned() {
                            let resolved = resolve_url(&src, &self.base_url);
                            let is_remote =
                                resolved.starts_with("http://") || resolved.starts_with("https://");
                            if !is_remote {
                                if let Some(decoded) =
                                    load_decoded_image_from_src(&src, &self.base_url)
                                {
                                    set_decoded_image_on_node(&mut node, decoded);
                                }
                            }
                            node.resolved_src = resolved;
                        }
                    }
                    // Background image
                    if !node.style.background_image_url.is_empty() {
                        let url = node.style.background_image_url.clone();
                        if let Some((data, w, h)) = load_image_from_src(&url, &self.base_url) {
                            node.bg_image_data = Some(std::sync::Arc::new(data));
                            node.bg_image_width = w;
                            node.bg_image_height = h;
                        }
                    }

                    // List counter (uses the CURRENT frame's counter)
                    {
                        let frame = stack.last_mut().unwrap();
                        if tag == "ol" {
                            frame.ol_counter = 0;
                        }
                        if tag == "li" {
                            frame.ol_counter += 1;
                            std::sync::Arc::make_mut(&mut node.style).list_index = frame.ol_counter;
                        }
                    }

                    // Summary: always list-item + Disclosure marker
                    if tag == "summary" {
                        std::sync::Arc::make_mut(&mut node.style).display = Display::ListItem;
                        std::sync::Arc::make_mut(&mut node.style).list_style_type =
                            ListStyleType::Disclosure;
                    }

                    // `<a>` and `<nobr>` close a still-open element of their own
                    // kind (HTML §13.2.6.4.7 runs the adoption agency for them
                    // on the start tag). `<a href=1>1<a href=2>2</a>` is two
                    // sibling links in every browser; nesting them made the
                    // whole rest of the document a child of the first one.
                    //
                    // Formatting elements between the two are reconstructed, so
                    // `<a>1<b>2<a>3` keeps `3` bold — the same rule as an end
                    // tag, because that is what this effectively is.
                    // "in select" (HTML §13.2.6.4.16): `<input>`, `<keygen>`,
                    // `<textarea>` and a nested `<select>` are parse errors
                    // handled as `</select>`, so the element lands AFTER the
                    // select rather than inside it. Only those four — a `<div>`
                    // in a select stays where it was written.
                    if stack
                        .last()
                        .map(|f| f.parent_tag == "select")
                        .unwrap_or(false)
                        && closes_select(&tag)
                    {
                        // The `stack.len() == 1` case was handled before the
                        // token was consumed, above.
                        let frame = stack.pop().unwrap();
                        let mut closed = frame.node;
                        Self::post_process_node(&mut closed, &self.base_url);
                        stack.last_mut().unwrap().node.children.push(closed);
                    }

                    // `<html>` and `<body>` start tags met again are parse
                    // errors whose ATTRIBUTES merge onto the existing element;
                    // they never create a second one. Building an element made
                    // `<body class=a>…<body class=b>` give a document two
                    // bodies, one nested in the other.
                    if matches!(tag.as_str(), "html" | "body") {
                        continue;
                    }

                    // A `<form>` while a form is open is IGNORED — the form
                    // element pointer is already set, and the spec inserts
                    // nothing (HTML §13.2.6.4.7). Nesting them gave a document
                    // two form owners for the same controls.
                    if tag == "form" && stack.iter().any(|f| f.parent_tag == "form") {
                        continue;
                    }

                    // `<button>` joins `<a>`/`<nobr>`: a second `<button>` ends
                    // the first rather than nesting inside it.
                    //
                    // `<table>` belongs here too — the "in table" mode closes an
                    // open table on a nested `<table>` start tag — but only when
                    // the open table is not the element this call was started
                    // for. Handing that case back to the caller mid-stack needs
                    // the token to be re-dispatched by the same insertion mode
                    // that opened it, which this parser does not model yet; see
                    // KNOWN_GAPS in `test_tree_construction`.
                    if matches!(tag.as_str(), "a" | "nobr" | "button") {
                        if let Some(idx) = stack.iter().rposition(|f| f.parent_tag == tag) {
                            if idx > 0 {
                                while stack.len() > idx {
                                    let frame = stack.pop().unwrap();
                                    if stack.len() > idx && is_formatting_element(&frame.parent_tag)
                                    {
                                        let mut fresh = self.new_box(&frame.parent_tag);
                                        fresh.attributes = frame.node.attributes.clone();
                                        fresh.style = frame.node.style.clone();
                                        self.pending_format.insert(0, fresh);
                                    }
                                    let mut closed = frame.node;
                                    Self::post_process_node(&mut closed, &self.base_url);
                                    stack.last_mut().unwrap().node.children.push(closed);
                                }
                            }
                        }
                    }

                    // HTML implicit closing: certain tags auto-close the
                    // current open element before opening a new one.
                    // Without this, unclosed <li>/<p>/<td>/etc. nest inside
                    // each other, creating absurdly deep DOM trees that
                    // overflow the stack during cascade/layout.
                    while stack.len() > 1 {
                        let cur = stack.last().unwrap().parent_tag.as_str();
                        if should_auto_close(cur, &tag) {
                            let frame = stack.pop().unwrap();
                            let mut closed = frame.node;
                            Self::post_process_node(&mut closed, &self.base_url);
                            stack.last_mut().unwrap().node.children.push(closed);
                        } else {
                            break;
                        }
                    }

                    // After the implicit closes, before the element goes in:
                    // an element start tag is content too, so it re-opens any
                    // formatting element still waiting.
                    reconstruct(&mut stack, &mut self.pending_format);

                    if self_closing {
                        // Void element — post-process then push to current frame.
                        Self::post_process_node(&mut node, &self.base_url);
                        stack.last_mut().unwrap().node.children.push(node);
                    } else {
                        // Non-void — push a new frame; children will be collected
                        // into node.children until the matching close tag.
                        stack.push(Frame {
                            parent_tag: tag,
                            node,
                            ol_counter: 0,
                        });
                    }
                }
            }
        }

        // EOF reached — collapse remaining frames.
        while let Some(frame) = stack.pop() {
            if stack.is_empty() {
                *children = frame.node.children;
                *ol_counter = frame.ol_counter;
            } else {
                let mut node = frame.node;
                Self::post_process_node(&mut node, &self.base_url);
                stack.last_mut().unwrap().node.children.push(node);
            }
        }
    }

    /// Handle a single open tag: create its node, parse its children (iteratively),
    /// apply post-processing, and push the finished node to `children`.
    /// Called from the top-level html/head/body skeleton parser for stray tags.
    pub(crate) fn handle_tag(
        &mut self,
        tag: String,
        attrs: crate::dom::attrs::AttrMap,
        self_closing: bool,
        children: &mut Vec<WebCore>,
        ol_counter: &mut i32,
    ) {
        // Reaching here at all means the token was not `<html>`, `<head>` or
        // `<body>` — it is content, and content closes the head (§13.2.6:
        // "anything else" in "before head" / "in head" pops out to the body).
        // Both of this method's callers are the skeleton parser's fallback
        // arm, so this is the one place that has to say it.
        self.head_closed = true;

        // Script/noscript: give host first chance, else show noscript as HTML.
        if matches!(tag.as_str(), "script" | "noscript") {
            let content = if !self_closing {
                self.collect_raw_text_until(&tag)
            } else {
                String::new()
            };
            let host_handled = if let Some(ref mut f) = self.on_script {
                f(&tag, &attrs, &content)
            } else {
                false
            };
            if !host_handled && tag == "noscript" && !content.is_empty() {
                let inner_tokens = tokenize(&content);
                let mut inner_parser = HtmlParser::new(inner_tokens);
                inner_parser.base_url = self.base_url.clone();
                let mut inner_children = Vec::new();
                let mut inner_ol = *ol_counter;
                inner_parser.parse_children_into("", &mut inner_children, &mut inner_ol);
                children.extend(inner_children);
                for rule in inner_parser.stylesheet.rules {
                    self.stylesheet.rules.push(rule);
                }
            }
            return;
        }
        // ⛔ A `<link rel=stylesheet>` counts wherever it appears, not only in
        // `<head>`. Body-inserted stylesheets are ordinary on the web — usps.com
        // serves two of its eight in the head and SIX in the body, including the
        // one that styles the navigation. Skipping them as "non-visual" meant
        // those sheets were never registered and never fetched, and the page
        // rendered with a fraction of its CSS: unstyled nav, no layout, one
        // long column.
        if tag == "link" {
            let rel = attrs.get("rel").map(|s| s.as_str()).unwrap_or("");
            let media = attrs.get("media").map(|s| s.as_str()).unwrap_or("");
            let href = attrs.get("href").cloned().unwrap_or_default();
            if rel.eq_ignore_ascii_case("stylesheet")
                && !attrs.contains_key("disabled")
                && !href.is_empty()
            {
                // Print-only sheets are not fetched for screen rendering, the
                // same rule the head path applies.
                if !media.eq_ignore_ascii_case("print") {
                    self.fire_hook(&tag, &attrs);
                }
                self.linked_stylesheets.push((href, media.to_string()));
            }
            if !self_closing {
                self.skip_until_close(&tag);
            }
            return;
        }
        if is_non_visual(&tag) {
            if !self_closing {
                self.skip_until_close(&tag);
            }
            return;
        }
        if tag == "style" {
            let css = self.collect_raw_text_until("style");
            self.stylesheet.parse_and_add(&normalize_css_text(&css));
            // The element stays in the tree — see the sibling arm in
            // `parse_children_into`.
            let mut style_node = self.new_box("style");
            style_node.text = css;
            apply_property(
                std::sync::Arc::make_mut(&mut style_node.style),
                "display",
                "none",
            );
            children.push(style_node);
            return;
        }
        if tag == "title" {
            let text = self.collect_raw_text_until("title");
            self.title = text.trim().to_string();
            let title = self.title.clone();
            self.push_head_node("title", attrs, title);
            return;
        }

        // SVG: collect raw markup and parse viewBox for intrinsic sizing
        if tag == "svg" {
            let svg_body = if !self_closing {
                self.collect_raw_text_until("svg")
            } else {
                String::new()
            };
            let fallback = crate::svg::build_inline_svg_fallback(&attrs, &svg_body);
            let mut node = self.new_box("svg");
            node.attributes = attrs;
            apply_property(
                std::sync::Arc::make_mut(&mut node.style),
                "display",
                "inline-block",
            );
            node.svg_markup = Some(fallback.markup);
            node.svg_document = node
                .svg_markup
                .as_deref()
                .and_then(|markup| crate::svg::parse_svg_document(markup).ok());
            node.svg_viewbox_w = fallback.viewbox_w;
            node.svg_viewbox_h = fallback.viewbox_h;
            self.project_inline_svg_children(&mut node);
            if let Some(w) = fallback.explicit_w {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "width",
                    &format!("{}px", w),
                );
            }
            if let Some(h) = fallback.explicit_h {
                apply_property(
                    std::sync::Arc::make_mut(&mut node.style),
                    "height",
                    &format!("{}px", h),
                );
            }
            apply_presentational_attrs(&mut node);
            children.push(node);
            return;
        }

        let mut node = self.new_box(&tag);
        node.attributes = attrs;
        apply_property(
            std::sync::Arc::make_mut(&mut node.style),
            "display",
            default_display(&tag),
        );
        apply_presentational_attrs(&mut node);

        if tag == "img" {
            // If srcset is present, pick the best URL from it and override src
            if let Some(srcset) = node.attributes.get("srcset").cloned() {
                if let Some(best) = parse_srcset_url(&srcset) {
                    node.attributes.insert("src".to_string(), best);
                }
            }
            if let Some(src) = node.attributes.get("src").cloned() {
                let resolved = resolve_url(&src, &self.base_url);
                let is_remote = resolved.starts_with("http://") || resolved.starts_with("https://");
                if !is_remote {
                    if let Some(decoded) = load_decoded_image_from_src(&src, &self.base_url) {
                        set_decoded_image_on_node(&mut node, decoded);
                    }
                }
                node.resolved_src = resolved;
            }
        }
        // Canvas/video/audio: set default dimensions from width/height attributes
        if matches!(tag.as_str(), "canvas" | "video" | "audio") {
            let default_w: u32 = if tag == "canvas" { 300 } else { 300 };
            let default_h: u32 = if tag == "canvas" { 150 } else { 150 };
            let w = node
                .attributes
                .get("width")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(default_w);
            let h = node
                .attributes
                .get("height")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(default_h);
            if node.style.width.is_auto() {
                std::sync::Arc::make_mut(&mut node.style).width =
                    crate::types::CssLength::Px(w as f32);
            }
            if node.style.height.is_auto() {
                std::sync::Arc::make_mut(&mut node.style).height =
                    crate::types::CssLength::Px(h as f32);
            }
            if tag == "canvas" {
                node.image_width = w;
                node.image_height = h;
                // Transparent pixel buffer — ready for drawing
                node.image_data = Some(std::sync::Arc::new(vec![0u8; (w * h * 4) as usize]));
            }
        }
        if !node.style.background_image_url.is_empty() {
            let url = node.style.background_image_url.clone();
            if let Some((data, w, h)) = load_image_from_src(&url, &self.base_url) {
                node.bg_image_data = Some(std::sync::Arc::new(data));
                node.bg_image_width = w;
                node.bg_image_height = h;
            }
        }
        if tag == "ol" {
            *ol_counter = 0;
        }
        if tag == "li" {
            *ol_counter += 1;
            std::sync::Arc::make_mut(&mut node.style).list_index = *ol_counter;
        }
        if tag == "summary" {
            std::sync::Arc::make_mut(&mut node.style).display = Display::ListItem;
            std::sync::Arc::make_mut(&mut node.style).list_style_type = ListStyleType::Disclosure;
        }

        if !self_closing {
            let mut inner_ol = 0i32;
            self.parse_children_into(&tag, &mut node.children, &mut inner_ol);
        }
        Self::post_process_node(&mut node, &self.base_url);

        children.push(node);
    }

    pub(crate) fn collect_raw_text_until(&mut self, end_tag: &str) -> String {
        let mut out = String::new();
        loop {
            match self.tokens.get(self.pos).cloned() {
                None => break,
                Some(Token::CloseTag { tag }) if tag == end_tag => {
                    self.pos += 1;
                    break;
                }
                Some(Token::Text(t)) => {
                    out.push_str(&t);
                    self.pos += 1;
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        out
    }

    pub(crate) fn skip_until_close(&mut self, end_tag: &str) {
        let mut depth = 1usize;
        loop {
            match self.tokens.get(self.pos).cloned() {
                None => break,
                Some(Token::OpenTag { tag, .. }) => {
                    if tag == end_tag {
                        depth += 1;
                    }
                    self.pos += 1;
                }
                Some(Token::CloseTag { tag }) => {
                    if tag == end_tag {
                        depth -= 1;
                        self.pos += 1;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        self.pos += 1;
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
    }
}
