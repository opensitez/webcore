//! The canvas 2D context state.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use std::collections::{HashMap, HashSet};

// ─── Canvas 2D Context ──────────────────────────────────────────────────────

/// A 2D drawing context for `<canvas>` elements.
/// Provides a subset of the HTML Canvas2D API for drawing shapes, text, and images.
pub struct CanvasContext {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel buffer (premultiplied alpha, row-major).
    pub pixels: Vec<u8>,
    fill_r: u8,
    fill_g: u8,
    fill_b: u8,
    fill_a: u8,
    stroke_r: u8,
    stroke_g: u8,
    stroke_b: u8,
    stroke_a: u8,
    line_width: f32,
    font_size: f32,
}

impl CanvasContext {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0u8; (width * height * 4) as usize],
            fill_r: 0,
            fill_g: 0,
            fill_b: 0,
            fill_a: 255,
            stroke_r: 0,
            stroke_g: 0,
            stroke_b: 0,
            stroke_a: 255,
            line_width: 1.0,
            font_size: 16.0,
        }
    }

    /// Set fill color from CSS-style string: "#rgb", "#rrggbb", "rgb(r,g,b)", or named colors.
    pub fn set_fill_style(&mut self, color: &str) {
        if let Some(c) = crate::css::parse_color(color) {
            self.fill_r = c.r;
            self.fill_g = c.g;
            self.fill_b = c.b;
            self.fill_a = c.a;
        }
    }

    /// Set stroke color.
    pub fn set_stroke_style(&mut self, color: &str) {
        if let Some(c) = crate::css::parse_color(color) {
            self.stroke_r = c.r;
            self.stroke_g = c.g;
            self.stroke_b = c.b;
            self.stroke_a = c.a;
        }
    }

    pub fn set_line_width(&mut self, w: f32) {
        self.line_width = w;
    }
    pub fn set_font_size(&mut self, px: f32) {
        self.font_size = px;
    }

    /// Fill a rectangle.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let x0 = x.max(0.0) as u32;
        let y0 = y.max(0.0) as u32;
        let x1 = ((x + w) as u32).min(self.width);
        let y1 = ((y + h) as u32).min(self.height);
        let (r, g, b, a) = (self.fill_r, self.fill_g, self.fill_b, self.fill_a);
        // Premultiply
        let pr = (r as u16 * a as u16 / 255) as u8;
        let pg = (g as u16 * a as u16 / 255) as u8;
        let pb = (b as u16 * a as u16 / 255) as u8;
        let stride = self.width as usize * 4;
        for py in y0..y1 {
            for px in x0..x1 {
                let i = py as usize * stride + px as usize * 4;
                if i + 3 < self.pixels.len() {
                    if a == 255 {
                        self.pixels[i] = r;
                        self.pixels[i + 1] = g;
                        self.pixels[i + 2] = b;
                        self.pixels[i + 3] = 255;
                    } else {
                        // Alpha blend (premultiplied)
                        let da = self.pixels[i + 3] as u16;
                        let ia = 255 - a as u16;
                        self.pixels[i] = (pr as u16 + self.pixels[i] as u16 * ia / 255) as u8;
                        self.pixels[i + 1] =
                            (pg as u16 + self.pixels[i + 1] as u16 * ia / 255) as u8;
                        self.pixels[i + 2] =
                            (pb as u16 + self.pixels[i + 2] as u16 * ia / 255) as u8;
                        self.pixels[i + 3] = (a as u16 + da * ia / 255) as u8;
                    }
                }
            }
        }
    }

    /// Clear a rectangle to transparent.
    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let x0 = x.max(0.0) as u32;
        let y0 = y.max(0.0) as u32;
        let x1 = ((x + w) as u32).min(self.width);
        let y1 = ((y + h) as u32).min(self.height);
        let stride = self.width as usize * 4;
        for py in y0..y1 {
            let row = py as usize * stride;
            for px in x0..x1 {
                let i = row + px as usize * 4;
                if i + 3 < self.pixels.len() {
                    self.pixels[i] = 0;
                    self.pixels[i + 1] = 0;
                    self.pixels[i + 2] = 0;
                    self.pixels[i + 3] = 0;
                }
            }
        }
    }

    /// Stroke a rectangle outline.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let lw = self.line_width;
        let saved = (self.fill_r, self.fill_g, self.fill_b, self.fill_a);
        self.fill_r = self.stroke_r;
        self.fill_g = self.stroke_g;
        self.fill_b = self.stroke_b;
        self.fill_a = self.stroke_a;
        self.fill_rect(x, y, w, lw); // top
        self.fill_rect(x, y + h - lw, w, lw); // bottom
        self.fill_rect(x, y, lw, h); // left
        self.fill_rect(x + w - lw, y, lw, h); // right
        self.fill_r = saved.0;
        self.fill_g = saved.1;
        self.fill_b = saved.2;
        self.fill_a = saved.3;
    }

    /// Fill a circle.
    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32) {
        let r2 = radius * radius;
        let x0 = (cx - radius).max(0.0) as i32;
        let y0 = (cy - radius).max(0.0) as i32;
        let x1 = ((cx + radius) as i32 + 1).min(self.width as i32);
        let y1 = ((cy + radius) as i32 + 1).min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                let dx = px as f32 + 0.5 - cx;
                let dy = py as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r2 {
                    let i = (py as usize * self.width as usize + px as usize) * 4;
                    if i + 3 < self.pixels.len() {
                        self.pixels[i] = self.fill_r;
                        self.pixels[i + 1] = self.fill_g;
                        self.pixels[i + 2] = self.fill_b;
                        self.pixels[i + 3] = self.fill_a;
                    }
                }
            }
        }
    }

    /// Draw a line from (x1,y1) to (x2,y2) using Bresenham.
    pub fn stroke_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        let (mut x, mut y) = (x1 as i32, y1 as i32);
        let (ex, ey) = (x2 as i32, y2 as i32);
        let dx = (ex - x).abs();
        let dy = -(ey - y).abs();
        let sx = if x < ex { 1 } else { -1 };
        let sy = if y < ey { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
                let i = (y as usize * self.width as usize + x as usize) * 4;
                if i + 3 < self.pixels.len() {
                    self.pixels[i] = self.stroke_r;
                    self.pixels[i + 1] = self.stroke_g;
                    self.pixels[i + 2] = self.stroke_b;
                    self.pixels[i + 3] = self.stroke_a;
                }
            }
            if x == ex && y == ey {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Copy pixel buffer to an WebCore's image_data for rendering.
    pub fn apply_to_node(&self, node: &mut WebCore) {
        node.image_data = Some(std::sync::Arc::new(self.pixels.clone()));
        node.image_width = self.width;
        node.image_height = self.height;
    }
}

/// Form interaction event — fired by the engine, handled by the host.
#[derive(Debug, Clone)]
pub struct FormEvent {
    /// Element tag (e.g. "input", "select", "textarea")
    pub tag: String,
    /// Element id attribute (empty if none)
    pub id: String,
    /// Element name attribute (empty if none)
    pub name: String,
    /// Event kind
    pub kind: FormEventKind,
    /// Stable node_id of the element.
    pub element: u32,
}

// FormEvent is now Send-safe (uses node_id instead of raw pointers)

#[derive(Debug, Clone)]
pub enum FormEventKind {
    /// Text input value changed (new value)
    Input(String),
    /// Value committed (e.g. Enter in text field, option selected)
    Change(String),
    /// Checkbox/radio toggled (new checked state)
    Toggle(bool),
    /// Button clicked (value attribute)
    Click(String),
    /// Form submitted (form element's action URL)
    Submit(String),
    /// Focus gained
    Focus,
    /// Focus lost
    Blur,
}

/// Callback type for form events. The host sets this to handle form interactions.
pub type FormEventCallback = Box<dyn FnMut(&FormEvent) + Send>;

/// A CSS rule that matched an element, stored for inspector display.
#[derive(Clone, Debug)]
pub struct MatchedRule {
    /// The original CSS selector text (e.g. ".container-fluid")
    pub selector: String,
    /// Property → value pairs from this rule
    pub declarations: Vec<(String, String)>,
    /// Specificity of the selector
    pub specificity: u32,
    /// Source: "ua" for user-agent, or the stylesheet URL/index
    pub source: String,
    /// Cascade layer name, empty for unlayered rules.
    pub layer: String,
    /// Resolved cascade layer rank; u32::MAX means unlayered.
    pub layer_rank: u32,
}

impl WebCore {
    /// Does this element DISPLAY an image — `<img>`, or `<input type=image>`?
    ///
    /// HTML §4.10.5.1.19: the Image Button state "represents an image and a
    /// submit button", and it takes `src`, `alt` and its dimensions exactly as
    /// `<img>` does. So every path that renders an image has to accept both,
    /// and gating on the TAG alone left an image input rendering as a text
    /// field — the parser even resolved its `src` and nothing read it.
    ///
    /// `type` is an ENUMERATED attribute, so its value is ASCII
    /// case-insensitive: `type="IMAGE"` is the same state.
    pub fn is_image_element(&self) -> bool {
        if self.tag == "img" {
            return true;
        }
        self.tag == "input"
            && self
                .attributes
                .get("type")
                .map(|t| t.trim().eq_ignore_ascii_case("image"))
                .unwrap_or(false)
    }

    pub fn new(tag: impl Into<String>) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_ID: AtomicU32 = AtomicU32::new(500_000);
        Self {
            tag: tag.into(),
            node_id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            style: std::sync::Arc::new(ComputedStyle::default()),
            attributes: crate::dom::attrs::AttrMap::new(),
            text: String::new(),
            children: Vec::new(),
            layout: LayoutBox::default(),

            component_width: 0.0,
            component_height: 0.0,

            resolved_src: String::new(),
            image_data: None,
            image_width: 0,
            image_height: 0,

            bg_image_data: None,
            mask_image_data: None,
            mask_image_width: 0,
            mask_image_height: 0,
            bg_image_width: 0,
            bg_image_height: 0,

            svg_markup: None,
            svg_document: None,
            svg_viewbox_w: 0.0,
            svg_viewbox_h: 0.0,

            checkedness: false,
            dirty_checked: false,
            selectedness: false,
            dirty_selectedness: false,
            value_state: None,
            dirty_value: false,
            autofilled: false,
            input_cursor: 0,
            input_sel_anchor: 0,
            input_sel_direction: SelectionDirection::None,
            top_layer_kind: None,

            data: HashMap::new(),
            matched_rules: Vec::new(),
            shadow_root: None,
            hover_applied: false,
            cascade_dirty: false,
            has_dirty_descendant: false,
            has_dirty_layout_descendant: false,
        }
    }

    /// Attach a shadow root to this element. Parses `html` as the shadow tree
    /// and extracts `<style>` blocks into a scoped stylesheet.
    pub fn attach_shadow(&mut self, mode: ShadowMode, html: &str) {
        let (children, stylesheet) = Self::build_shadow_content(html);
        // The shadow root takes the next node id, so it can be named like any
        // other node.
        let node_id = crate::dom::arena::next_shadow_node_id();
        self.shadow_root = Some(Box::new(ShadowRoot {
            children,
            stylesheet,
            mode,
            node_id,
            delegates_focus: false,
            slot_assignment: SlotAssignment::Named,
            clonable: false,
            serializable: false,
            adopted_stylesheets: Vec::new(),
        }));
    }

    /// `shadowRoot.innerHTML = html` — replace the CONTENT of an existing
    /// shadow root.
    ///
    /// ⛔ Not `attach_shadow` again. A `ShadowRoot` is a node with an identity
    /// that survives its content being rewritten: in a browser the object a
    /// page holds from `attachShadow()` is the same object after
    /// `root.innerHTML = …`. Re-attaching minted a NEW root id, so the id the
    /// caller was handed stopped naming anything — `node_type` on it answered
    /// 0 instead of 11. Everything else the root carries — the mode,
    /// `delegatesFocus`, `slotAssignment`, the adopted stylesheets — survives
    /// for the same reason.
    pub fn set_shadow_content(&mut self, html: &str) -> bool {
        let (children, stylesheet) = Self::build_shadow_content(html);
        match self.shadow_root.as_mut() {
            Some(root) => {
                root.children = children;
                root.stylesheet = stylesheet;
                true
            }
            None => false,
        }
    }

    /// Parse a shadow tree's markup into its children and its scoped sheet.
    fn build_shadow_content(html: &str) -> (Vec<WebCore>, crate::css::Stylesheet) {
        let doc = crate::html::parse_html(html);
        let mut children = doc.root.children;
        // Move <body> children up: the parser wraps a fragment in
        // `<html><head></head><body>…`, and a shadow tree wants the CONTENT.
        //
        // Found by TAG, not by index. This asked whether body was the only
        // child, which stopped being true the moment the parser started
        // synthesising `<head>` as HTML §13.2.6 requires.
        if let Some(at) = children.iter().position(|c| c.tag == "body") {
            children = std::mem::take(&mut children[at].children);
        }
        // Start with UA stylesheet so shadow tree gets default styles
        let mut stylesheet = crate::css::ua_stylesheet();
        // Extract <style> elements into the scoped stylesheet
        let mut styles_css = String::new();
        children.retain(|c| {
            if c.tag == "style" {
                styles_css.push_str(&c.text);
                for ch in &c.children {
                    if ch.tag == "#text" {
                        styles_css.push_str(&ch.text);
                    }
                }
                false
            } else {
                true
            }
        });
        if !styles_css.is_empty() {
            // Author origin — the shadow root's `<style>` outranks the UA sheet
            // it was seeded with (see `css::AUTHOR_ORIGIN_BOOST`).
            stylesheet.parse_and_add_author(&styles_css);
        }
        // ⛔ Renumber the whole shadow subtree.
        //
        // `parse_html` builds a FRESH document, whose node ids start at 1 —
        // the same numbers the host document has already handed out. Left
        // alone, a shadow child collides with a light-DOM node: asking a
        // shadow `<p>` for its rect answered the HOST's rect, because
        // `find_webcore` reached the light node with that id first. Every
        // node-keyed API — layout, hit-testing, event dispatch, the arena
        // bridge — was reading the wrong node.
        //
        // The shadow id space counts DOWN from just below the reserved
        // Window/Document ids while the arena counts up, so a number drawn
        // from it can never be an arena node's.
        fn renumber(node: &mut WebCore) {
            node.node_id = crate::dom::arena::next_shadow_node_id();
            for child in &mut node.children {
                renumber(child);
            }
        }
        for child in &mut children {
            renumber(child);
        }
        (children, stylesheet)
    }

    pub fn get_attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|s| s.as_str())
    }

    pub fn is_text_node(&self) -> bool {
        self.tag == "#text"
    }

    /// Whether this node is an ELEMENT.
    ///
    /// Everything that counts elements — `:nth-child`, `firstElementChild`,
    /// `children`, "does this box have any content" — used to spell the test
    /// `tag != "#text"`, which was exact only while text was the one non-element
    /// node that could appear. It is not: the DOM's non-element nodes all carry
    /// a `#`-prefixed name (`#text`, `#comment`, `#cdata-section`,
    /// `#document-fragment`), and a comment counted as an element would shift
    /// every `:nth-child` index after it and make an empty box non-empty.
    ///
    /// Asking the question once, by the naming rule the DOM already uses, is
    /// what keeps the next node kind from having to be added in fifty places.
    pub fn is_element(&self) -> bool {
        !self.tag.starts_with('#') && !self.is_pseudo_element()
    }

    /// Is this a generated `::before` / `::after` box rather than a DOM node?
    ///
    /// The cascade materialises `content` as a real child box so layout and
    /// paint can treat it like anything else. It is NOT a node: a
    /// pseudo-element has no place in `childNodes`, is not counted by
    /// `:nth-child`, and cannot be serialized — `<::after>!</::after>` is not
    /// markup, and it was reaching the output of `serialize_html`.
    pub fn is_pseudo_element(&self) -> bool {
        self.tag.starts_with("::")
    }

    /// Number of direct children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Whether this node has any children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn is_void(&self) -> bool {
        matches!(
            self.tag.as_str(),
            "br" | "hr"
                | "img"
                | "input"
                | "meta"
                | "link"
                | "col"
                | "area"
                | "base"
                | "embed"
                | "param"
                | "source"
                | "track"
                | "wbr"
        )
    }

    /// Returns the effective children for layout/render: shadow children if a
    /// shadow root is present, otherwise the normal children.
    pub fn effective_children(&self) -> &[WebCore] {
        if let Some(ref sr) = self.shadow_root {
            &sr.children
        } else {
            &self.children
        }
    }

    /// Mutable version of `effective_children`.
    pub fn effective_children_mut(&mut self) -> &mut Vec<WebCore> {
        if let Some(ref mut sr) = self.shadow_root {
            &mut sr.children
        } else {
            &mut self.children
        }
    }

    /// Resolve `<slot>` elements in the shadow tree by projecting light DOM children.
    /// Must be called before layout when a shadow root is present.
    pub fn resolve_slots(&mut self) {
        if self.shadow_root.is_none() {
            return;
        }
        let light_children = self.children.clone();
        let sr = self.shadow_root.as_mut().unwrap();
        resolve_slots_inner(&mut sr.children, &light_children);
    }

    /// Collect all text content recursively.
    pub fn text_content(&self) -> String {
        if self.is_text_node() {
            return self.text.clone();
        }
        let mut out = self.text.clone();
        for child in &self.children {
            out.push_str(&child.text_content());
        }
        out
    }

    /// Find boxes matching a simple CSS selector (tag, .class, #id).
    pub fn query_selector_all<'a>(&'a self, selector: &str) -> Vec<&'a WebCore> {
        let mut results = Vec::new();
        self.collect_matching(selector, &mut results);
        results
    }

    fn collect_matching<'a>(&'a self, selector: &str, out: &mut Vec<&'a WebCore>) {
        if self.matches_simple_selector(selector) {
            out.push(self);
        }
        for child in &self.children {
            child.collect_matching(selector, out);
        }
    }

    fn matches_simple_selector(&self, selector: &str) -> bool {
        if selector.starts_with('#') {
            self.attributes.get("id").map(|s| s.as_str()) == Some(&selector[1..])
        } else if selector.starts_with('.') {
            let cls = &selector[1..];
            self.attributes
                .get("class")
                .map(|s| s.split_whitespace().any(|c| c == cls))
                .unwrap_or(false)
        } else {
            self.tag == selector
        }
    }
}
