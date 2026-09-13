use crate::css::{Combinator, CssRule, CssSelector, SelectorPart, ua_stylesheet};
use crate::types::{
    AlignItems, BorderStyle, Color, ComputedStyle, CssLength, Display, Document, FlexDirection,
    FlexWrap, Float, FontStyle, FontWeight, JustifyContent, Position, TextAlign, TextTransform,
    WebCore, WhiteSpace,
};

// ─── Utility ──────────────────────────────────────────────────────────────────

/// Escape `&`, `<`, `>`, and `"` for safe HTML output.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\u{00A0}' => out.push_str("&nbsp;"),
            _ => out.push(ch),
        }
    }
    out
}

/// HTML §13.3 "escaping a string", attribute mode.
///
/// The spec escapes `&`, U+00A0 and `"` here and leaves `<`/`>` alone; Blink
/// escapes those two as well. We follow Blink, because the difference is not
/// observable — `&lt;` inside an attribute value parses back to `<` either
/// way — and it keeps the Chrome differential exact.
pub fn escape_attr(value: &str) -> String {
    escape_html(value)
}

// ─── Length serialization ─────────────────────────────────────────────────────

/// Convert a `CssLength` to its CSS string representation.
/// Returns an empty string when the length should not be emitted.
pub fn serialize_length(len: &CssLength) -> String {
    match len {
        CssLength::Auto => String::new(), // auto == default, skip
        CssLength::Content => "content".to_string(),
        CssLength::MinContent => "min-content".to_string(),
        CssLength::MaxContent => "max-content".to_string(),
        CssLength::FitContent => "fit-content".to_string(),
        CssLength::FitContentArg(a) => format!("fit-content({})", serialize_length(a)),
        CssLength::None => String::new(),
        CssLength::Zero => String::new(),
        CssLength::Px(v) => format!("{}px", *v as i32),
        CssLength::Em(v) => format!("{}em", v),
        CssLength::Rem(v) => format!("{}rem", v),
        CssLength::Percent(v) => format!("{}%", v),
        CssLength::Vw(v) => format!("{}vw", v),
        CssLength::Vmin(v) => format!("{}vmin", v),
        CssLength::Vmax(v) => format!("{}vmax", v),
        CssLength::Vh(v) => format!("{}vh", v),
        CssLength::Calc(c) => {
            let labels = ["%", "px", "em", "rem", "vw", "vh"];
            let parts: Vec<String> = c
                .iter()
                .zip(labels.iter())
                .filter(|(v, _)| **v != 0.0)
                .map(|(v, u)| format!("{}{}", v, u))
                .collect();
            if parts.is_empty() {
                "0px".to_string()
            } else {
                format!("calc({})", parts.join(" + "))
            }
        }
        CssLength::Min(vals) => {
            let inner: Vec<String> = vals.iter().map(|v| serialize_length(v)).collect();
            format!("min({})", inner.join(", "))
        }
        CssLength::Max(vals) => {
            let inner: Vec<String> = vals.iter().map(|v| serialize_length(v)).collect();
            format!("max({})", inner.join(", "))
        }
        CssLength::Clamp(parts) => {
            let (min, val, max) = (&parts[0], &parts[1], &parts[2]);
            format!(
                "clamp({}, {}, {})",
                serialize_length(min),
                serialize_length(val),
                serialize_length(max)
            )
        }
        CssLength::CalcExpr(_) => "calc(...)".to_string(),
    }
}

// ─── Color serialization ──────────────────────────────────────────────────────

fn color_to_css(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a as f32 / 255.0)
    }
}

// ─── Border side ──────────────────────────────────────────────────────────────

fn serialize_border_side(width: &CssLength, style: BorderStyle, color: Color) -> String {
    if style == BorderStyle::None || style == BorderStyle::Hidden {
        return String::new();
    }
    let w = match width {
        CssLength::Px(v) if *v > 0.0 => *v as i32,
        _ => return String::new(),
    };
    let style_str = match style {
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Double => "double",
        BorderStyle::Inset => "inset",
        BorderStyle::Outset => "outset",
        BorderStyle::Groove => "groove",
        BorderStyle::Ridge => "ridge",
        _ => "solid",
    };
    format!("{}px {} {}", w, style_str, color_to_css(color))
}

// ─── Edge (margin / padding) ──────────────────────────────────────────────────

fn serialize_edge(
    name: &str,
    top: &CssLength,
    right: &CssLength,
    bottom: &CssLength,
    left: &CssLength,
    parts: &mut Vec<(String, String)>,
) {
    let t = serialize_length(top);
    let r = serialize_length(right);
    let b = serialize_length(bottom);
    let l = serialize_length(left);

    if t.is_empty() && r.is_empty() && b.is_empty() && l.is_empty() {
        return;
    }

    if !t.is_empty() && t == r && r == b && b == l {
        parts.push((name.to_string(), t));
    } else {
        if !t.is_empty() {
            parts.push((format!("{}-top", name), t));
        }
        if !r.is_empty() {
            parts.push((format!("{}-right", name), r));
        }
        if !b.is_empty() {
            parts.push((format!("{}-bottom", name), b));
        }
        if !l.is_empty() {
            parts.push((format!("{}-left", name), l));
        }
    }
}

// ─── Style → inline CSS ───────────────────────────────────────────────────────

/// Serialize a `ComputedStyle` to an inline CSS string (without the `style=""` wrapper).
/// Only properties that differ from their defaults are emitted.
pub fn serialize_style_to_css(style: &ComputedStyle, _tag: &str) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();

    // ── Position ──────────────────────────────────────────────────────────────
    let pos_str = match style.position {
        Position::Static => "",
        Position::Relative => "relative",
        Position::Absolute => "absolute",
        Position::Fixed => "fixed",
        Position::Sticky => "sticky",
    };
    if !pos_str.is_empty() {
        parts.push(("position".into(), pos_str.into()));
    }

    // ── Float ─────────────────────────────────────────────────────────────────
    let float_str = match style.float {
        Float::None => "",
        Float::Left => "left",
        Float::Right => "right",
        Float::InlineStart => "inline-start",
        Float::InlineEnd => "inline-end",
    };
    if !float_str.is_empty() {
        parts.push(("float".into(), float_str.into()));
    }

    // ── Dimensions ────────────────────────────────────────────────────────────
    if !style.width.is_auto() {
        let s = serialize_length(&style.width);
        if !s.is_empty() {
            parts.push(("width".into(), s));
        }
    }
    if !style.height.is_auto() {
        let s = serialize_length(&style.height);
        if !s.is_empty() {
            parts.push(("height".into(), s));
        }
    }

    // ── Margin ────────────────────────────────────────────────────────────────
    serialize_edge(
        "margin",
        &style.margin_top,
        &style.margin_right,
        &style.margin_bottom,
        &style.margin_left,
        &mut parts,
    );

    // ── Padding ───────────────────────────────────────────────────────────────
    serialize_edge(
        "padding",
        &style.padding_top,
        &style.padding_right,
        &style.padding_bottom,
        &style.padding_left,
        &mut parts,
    );

    // ── Border ────────────────────────────────────────────────────────────────
    let bt = serialize_border_side(
        &style.border_top_width,
        style.border_top_style,
        style.border_top_color,
    );
    let br = serialize_border_side(
        &style.border_right_width,
        style.border_right_style,
        style.border_right_color,
    );
    let bb = serialize_border_side(
        &style.border_bottom_width,
        style.border_bottom_style,
        style.border_bottom_color,
    );
    let bl = serialize_border_side(
        &style.border_left_width,
        style.border_left_style,
        style.border_left_color,
    );

    if !bt.is_empty() && bt == br && br == bb && bb == bl {
        parts.push(("border".into(), bt));
    } else {
        if !bt.is_empty() {
            parts.push(("border-top".into(), bt));
        }
        if !br.is_empty() {
            parts.push(("border-right".into(), br));
        }
        if !bb.is_empty() {
            parts.push(("border-bottom".into(), bb));
        }
        if !bl.is_empty() {
            parts.push(("border-left".into(), bl));
        }
    }

    // ── Background color ──────────────────────────────────────────────────────
    if style.background_color.a > 0 {
        parts.push((
            "background-color".into(),
            color_to_css(style.background_color),
        ));
    }

    // ── Text color ────────────────────────────────────────────────────────────
    // Always emit color (simple approach, matches C++ behaviour).
    parts.push(("color".into(), color_to_css(style.color)));

    // ── Text alignment ────────────────────────────────────────────────────────
    let align_str = match style.text_align {
        TextAlign::Left => "",
        TextAlign::Right => "right",
        TextAlign::Center => "center",
        TextAlign::Justify => "justify",
        TextAlign::Start => "start",
        TextAlign::End => "end",
    };
    if !align_str.is_empty() {
        parts.push(("text-align".into(), align_str.into()));
    }

    // ── Font weight ───────────────────────────────────────────────────────────
    match style.font_weight {
        FontWeight::Bold => parts.push(("font-weight".into(), "bold".into())),
        FontWeight::Value(v) if v != 400 => parts.push(("font-weight".into(), v.to_string())),
        _ => {}
    }

    // ── Font style ────────────────────────────────────────────────────────────
    if style.font_style == FontStyle::Italic {
        parts.push(("font-style".into(), "italic".into()));
    } else if style.font_style == FontStyle::Oblique {
        parts.push(("font-style".into(), "oblique".into()));
    }

    // ── Text decoration ───────────────────────────────────────────────────────
    if style.text_decoration.underline
        || style.text_decoration.overline
        || style.text_decoration.strikethrough
    {
        let mut decorations = Vec::new();
        if style.text_decoration.underline {
            decorations.push("underline");
        }
        if style.text_decoration.overline {
            decorations.push("overline");
        }
        if style.text_decoration.strikethrough {
            decorations.push("line-through");
        }
        parts.push(("text-decoration".into(), decorations.join(" ")));
    }

    // ── Text transform ────────────────────────────────────────────────────────
    let tt_str = match style.text_transform {
        TextTransform::None => "",
        TextTransform::Uppercase => "uppercase",
        TextTransform::Lowercase => "lowercase",
        TextTransform::Capitalize => "capitalize",
        TextTransform::FullWidth => "full-width",
        TextTransform::FullSizeKana => "full-size-kana",
        TextTransform::MathAuto => "math-auto",
    };
    if !tt_str.is_empty() {
        parts.push(("text-transform".into(), tt_str.into()));
    }

    // ── White space ───────────────────────────────────────────────────────────
    let ws_str = match style.white_space {
        WhiteSpace::Normal => "",
        WhiteSpace::Nowrap => "nowrap",
        WhiteSpace::Pre => "pre",
        WhiteSpace::PreWrap => "pre-wrap",
        WhiteSpace::PreLine => "pre-line",
    };
    if !ws_str.is_empty() {
        parts.push(("white-space".into(), ws_str.into()));
    }

    // ── Display (flex / grid / inline-* variants) ─────────────────────────────
    let display_str = match style.display {
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid => "grid",
        Display::InlineGrid => "inline-grid",
        Display::InlineBlock => "inline-block",
        Display::None => "none",
        _ => "",
    };
    if !display_str.is_empty() {
        parts.push(("display".into(), display_str.into()));
    }

    // ── Flex container properties ─────────────────────────────────────────────
    if matches!(style.display, Display::Flex | Display::InlineFlex) {
        let dir_str = match style.flex_direction {
            FlexDirection::Row => "",
            FlexDirection::RowReverse => "row-reverse",
            FlexDirection::Column => "column",
            FlexDirection::ColumnReverse => "column-reverse",
        };
        if !dir_str.is_empty() {
            parts.push(("flex-direction".into(), dir_str.into()));
        }

        if style.flex_wrap == FlexWrap::Wrap {
            parts.push(("flex-wrap".into(), "wrap".into()));
        } else if style.flex_wrap == FlexWrap::WrapReverse {
            parts.push(("flex-wrap".into(), "wrap-reverse".into()));
        }

        let jc_str = match style.justify_content {
            JustifyContent::FlexStart => "",
            JustifyContent::FlexEnd => "flex-end",
            JustifyContent::Center => "center",
            JustifyContent::SpaceBetween => "space-between",
            JustifyContent::SpaceAround => "space-around",
            JustifyContent::SpaceEvenly => "space-evenly",
            JustifyContent::Left => "left",
            JustifyContent::Right => "right",
        };
        if !jc_str.is_empty() {
            parts.push(("justify-content".into(), jc_str.into()));
        }

        let ai_str = match style.align_items {
            AlignItems::Stretch => "",
            AlignItems::FlexStart => "flex-start",
            AlignItems::FlexEnd => "flex-end",
            AlignItems::Center => "center",
            AlignItems::Baseline => "baseline",
            AlignItems::LastBaseline => "last baseline",
        };
        if !ai_str.is_empty() {
            parts.push(("align-items".into(), ai_str.into()));
        }
    }

    // ── Flex item properties ──────────────────────────────────────────────────
    if style.flex_grow != 0.0 {
        parts.push(("flex-grow".into(), style.flex_grow.to_string()));
    }
    if style.flex_shrink != 1.0 {
        parts.push(("flex-shrink".into(), style.flex_shrink.to_string()));
    }

    // ── Positioned offsets ────────────────────────────────────────────────────
    if style.position != Position::Static {
        let top_s = serialize_length(&style.top);
        let right_s = serialize_length(&style.right);
        let bottom_s = serialize_length(&style.bottom);
        let left_s = serialize_length(&style.left);
        if !top_s.is_empty() {
            parts.push(("top".into(), top_s));
        }
        if !right_s.is_empty() {
            parts.push(("right".into(), right_s));
        }
        if !bottom_s.is_empty() {
            parts.push(("bottom".into(), bottom_s));
        }
        if !left_s.is_empty() {
            parts.push(("left".into(), left_s));
        }
    }

    // ── z-index ───────────────────────────────────────────────────────────────
    if !style.z_index_is_auto {
        parts.push(("z-index".into(), style.z_index.to_string()));
    }

    // ── Opacity ───────────────────────────────────────────────────────────────
    if style.opacity < 1.0 {
        parts.push(("opacity".into(), format!("{:.3}", style.opacity)));
    }

    // ── Assemble ──────────────────────────────────────────────────────────────
    parts
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Convenience wrapper: serialize a `ComputedStyle` without a tag context.
pub fn serialize_style(style: &ComputedStyle) -> String {
    serialize_style_to_css(style, "")
}

// ─── Selector serialization ───────────────────────────────────────────────────

/// CSSOM serialization of a selector — `CSSStyleRule.selectorText`.
pub fn serialize_selector(sel: &CssSelector) -> String {
    let mut out = String::new();
    for part in &sel.parts {
        match part {
            SelectorPart::Tag(t) => out.push_str(t),
            SelectorPart::Id(id) => {
                out.push('#');
                out.push_str(id);
            }
            SelectorPart::Class(cls) => {
                out.push('.');
                out.push_str(cls);
            }
            SelectorPart::Universal => out.push('*'),
            SelectorPart::PseudoClass(pc) => {
                out.push(':');
                out.push_str(pc);
            }
            SelectorPart::PseudoElement(pe) => {
                out.push_str("::");
                out.push_str(pe);
            }
            SelectorPart::Attribute {
                name,
                op,
                value,
                case_sensitive,
            } => {
                use crate::css::AttrOp;
                out.push('[');
                out.push_str(name);
                match op {
                    AttrOp::Exists => {}
                    AttrOp::Eq => {
                        out.push('=');
                        out.push('"');
                        out.push_str(value);
                        out.push('"');
                    }
                    AttrOp::Includes => {
                        out.push_str("~=\"");
                        out.push_str(value);
                        out.push('"');
                    }
                    AttrOp::StartsWith => {
                        out.push_str("^=\"");
                        out.push_str(value);
                        out.push('"');
                    }
                    AttrOp::EndsWith => {
                        out.push_str("$=\"");
                        out.push_str(value);
                        out.push('"');
                    }
                    AttrOp::Contains => {
                        out.push_str("*=\"");
                        out.push_str(value);
                        out.push('"');
                    }
                    AttrOp::DashMatch => {
                        out.push_str("|=\"");
                        out.push_str(value);
                        out.push('"');
                    }
                }
                // Selectors §6.3 — a selector that was written with a flag has
                // to come back out with it, or a reserialized stylesheet quietly
                // means something narrower than the one that went in.
                match case_sensitive {
                    Some(false) => out.push_str(" i"),
                    Some(true) => out.push_str(" s"),
                    None => {}
                }
                out.push(']');
            }
            SelectorPart::Combinator(c) => match c {
                Combinator::Descendant => out.push(' '),
                Combinator::Child => out.push_str(" > "),
                Combinator::AdjacentSibling => out.push_str(" + "),
                Combinator::GeneralSibling => out.push_str(" ~ "),
                Combinator::Column => out.push_str(" || "),
            },
            SelectorPart::Not(inner) => {
                out.push_str(":not(");
                out.push_str(&serialize_selector(inner));
                out.push(')');
            }
            SelectorPart::Is(list) => {
                out.push_str(":is(");
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serialize_selector(s));
                }
                out.push(')');
            }
            SelectorPart::Where(list) => {
                out.push_str(":where(");
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serialize_selector(s));
                }
                out.push(')');
            }
            SelectorPart::Has(inner) => {
                out.push_str(":has(");
                let parts: Vec<String> = inner.iter().map(serialize_selector).collect();
                out.push_str(&parts.join(", "));
                out.push(')');
            }
        }
    }
    out
}

/// CSSOM serialization of one rule — `CSSRule.cssText`.
///
/// Not used by document serialization any more: a document's `<style>`
/// elements carry their author's source and are emitted as elements, so there
/// is nothing to rebuild. This stays because it IS the CSSOM text form, which
/// `document.styleSheets` will need.
pub fn serialize_rule(rule: &CssRule) -> String {
    // Prefer the verbatim original selector for a faithful roundtrip (preserves
    // vendor pseudo-elements, whitespace, unknown syntax, etc.).  Fall back to
    // reconstructing from parsed parts only for programmatically-added rules.
    let sel_text = if !rule.original_selector.is_empty() {
        rule.original_selector.clone()
    } else {
        rule.selectors
            .iter()
            .map(serialize_selector)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    };

    if sel_text.is_empty() {
        return String::new();
    }

    let mut decls = Vec::new();
    // SOURCE ORDER, not alphabetical. The sort here used to be the only thing
    // making the output deterministic, because the block was a HashMap — but
    // alphabetical order is a different stylesheet: it puts `border-top` before
    // `border`, and the shorthand then wipes the longhand that was written to
    // override it. Now that a block keeps the order it was parsed in, emitting
    // it in that order is both stable AND meaning-preserving.
    for (prop, val) in rule.declarations.iter() {
        decls.push(format!("{}: {};", prop, val));
    }
    for (prop, val) in rule.important_declarations.iter() {
        decls.push(format!("{}: {} !important;", prop, val));
    }

    if decls.is_empty() {
        format!("{} {{ }}", sel_text)
    } else {
        format!("{} {{ {} }}", sel_text, decls.join(" "))
    }
}

/// CSSOM serialization of a document's AUTHOR rules — the text form of
/// `document.styleSheets`, rebuilt from the cascade rather than from source.
pub fn serialize_stylesheet(doc: &Document) -> String {
    // Skip UA rules — only emit author rules (appended after UA rules in combined stylesheet).
    let ua_count = ua_stylesheet().rules.len();
    let author_rules: Vec<_> = doc.stylesheet.rules.iter().skip(ua_count).collect();

    if author_rules.is_empty() {
        return String::new();
    }

    let mut out = String::from("<style>\n");
    for rule in author_rules {
        let s = serialize_rule(rule);
        if !s.is_empty() {
            out.push_str(&s);
        }
    }
    out.push_str("</style>\n");
    out
}

// ─── Box serialization ────────────────────────────────────────────────────────

/// Serialize a single `WebCore` (and all its descendants) into `out`.
///
/// `<style>` is serialized like any other element, with its own text. There was
/// briefly a `skip_style` variant for the document walk, back when whole-document
/// serialization wrote a stylesheet rebuilt from the cascade into the head it
/// synthesises — skipping the elements was how the rules avoided appearing
/// twice. Emitting the elements where they actually are is both simpler and
/// what a browser does, so the reconstruction and the variant are gone.
pub fn serialize_box(node: &WebCore, out: &mut String) {
    serialize_box_inner(node, out)
}

fn serialize_box_inner(node: &WebCore, out: &mut String) {
    // Generated content is not markup. `::before`/`::after` are boxes the
    // cascade adds so layout can measure them; writing them out produced
    // `<::after>!</::after>`, which is not a tag and does not parse back.
    if node.is_pseudo_element() {
        return;
    }
    // Text nodes
    if node.tag == "#text" {
        out.push_str(&escape_html(&node.text));
        return;
    }
    // Comment nodes. Data is written VERBATIM — the fragment-serialising steps
    // escape text and attribute values, never comment data.
    if node.tag == "#comment" {
        out.push_str("<!--");
        out.push_str(&node.text);
        out.push_str("-->");
        return;
    }

    let tag = if node.tag.is_empty() {
        "div"
    } else {
        node.tag.as_str()
    };

    // Open tag
    out.push('<');
    out.push_str(tag);

    // HTML §13.3 — attributes are written in the element's attribute-list
    // order, which is the order they were set. `AttrMap` is a list for exactly
    // this reason; the old code sorted the map's keys "for deterministic
    // output", which was deterministic and wrong. Chrome on
    // `<div id=d zebra=1 alpha=2>` writes `id="d" zebra="1" alpha="2"`.
    //
    // The value is always quoted, even when empty: Chrome serializes
    // `<input checked>` as `checked=""`, and a bare name is not what any
    // browser writes.
    for (k, v) in node.attributes.iter() {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape_attr(v));
        out.push('"');
    }

    // Void elements: self-close, no children.
    if node.is_void() {
        out.push('>');
        return;
    }

    out.push('>');

    // HTML §13.3 — a `<pre>`, `<textarea>` or `<listing>` whose text starts
    // with a newline gets an EXTRA one here, because the parser drops a newline
    // immediately after the open tag. Without it the first line of a code block
    // is eaten a little more on every serialize → reparse cycle.
    if matches!(tag, "pre" | "textarea" | "listing") {
        let starts_with_newline = node.text.starts_with('\n')
            || node
                .children
                .first()
                .map(|c| c.is_text_node() && c.text.starts_with('\n'))
                .unwrap_or(false);
        if starts_with_newline {
            out.push('\n');
        }
    }

    // Own text (block-level text content stored directly on the node).
    if !node.text.is_empty() {
        // `<style>` and `<script>` hold RAW TEXT: their content is not escaped
        // on the way out, because it was never entity-decoded on the way in.
        // Escaping it turns `a > b` in a selector into `a &gt; b`, which does
        // not parse back as the same stylesheet.
        if matches!(tag, "style" | "script") {
            out.push_str(&node.text);
        } else {
            out.push_str(&escape_html(&node.text));
        }
    }

    // Inline runs — emit styled text segments.
    // Our InlineRun has text_offset, length, style but no atomicBox pointer,
    // so we just emit the text slice with light markup for common properties.
    for run in &node.layout.inline_runs {
        let end = (run.text_offset + run.length).min(node.text.len());
        if run.text_offset > node.text.len() {
            continue;
        }
        let segment = &node.text[run.text_offset..end];
        if segment.is_empty() {
            continue;
        }

        let has_bold = run.style.font_weight.is_bold();
        let has_italic = run.style.font_style == FontStyle::Italic;
        let has_underline = run.style.text_decoration.underline;
        let has_strike = run.style.text_decoration.strikethrough;
        let has_link = node
            .attributes
            .get("href")
            .map(|s| !s.is_empty())
            .unwrap_or(false);

        if has_link {
            if let Some(href) = node.attributes.get("href") {
                out.push_str("<a href=\"");
                out.push_str(&escape_html(href));
                out.push_str("\">");
            }
        }
        if has_bold {
            out.push_str("<b>");
        }
        if has_italic {
            out.push_str("<i>");
        }
        if has_underline && !has_link {
            out.push_str("<u>");
        }
        if has_strike {
            out.push_str("<s>");
        }

        out.push_str(&escape_html(segment));

        if has_strike {
            out.push_str("</s>");
        }
        if has_underline && !has_link {
            out.push_str("</u>");
        }
        if has_italic {
            out.push_str("</i>");
        }
        if has_bold {
            out.push_str("</b>");
        }
        if has_link {
            out.push_str("</a>");
        }
    }

    // Children (depth-first).
    for child in &node.children {
        serialize_box_inner(child, out);
    }

    // Close tag.
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

// ─── Document serialization ───────────────────────────────────────────────────

/// Serialize the full document tree to an HTML string.
///
/// * If the document has stylesheet rules, the output is wrapped in a full
///   `<html><head><style>…</style></head><body>…</body></html>` document.
/// * Otherwise only the children of the root "html" box are serialized.
pub fn serialize_html(doc: &Document) -> String {
    let mut html = String::new();

    // Serialize the root box. The root IS the document element, so its head
    // and body children are hoisted into the head and body written below
    // rather than emitted as ordinary elements.
    //
    // The test used to be "html AND no attributes of its own", which meant a
    // document that wrote `<html lang="en">` took the `else` branch and was
    // serialised WHOLE inside the synthetic `<body>` — `<body><html lang="en">
    // <head>…`, a second document element nested in the first. The attribute
    // is the one thing that should not decide the shape of the output, so it
    // is carried onto the tag instead.
    let root = &doc.root;
    let root_is_document_element = root.tag == "html" && root.text.is_empty();

    if root_is_document_element {
        html.push_str("<html");
        for (name, value) in root.attributes.iter() {
            html.push(' ');
            html.push_str(name);
            html.push_str("=\"");
            html.push_str(&escape_attr(value));
            html.push('"');
        }
        html.push_str(">\n<head>\n");
        // The document's OWN head elements — title, meta, link. They belong in
        // the head being written here; walking them again with the rest of
        // `root.children` below emitted a second `<head>` inside the `<body>`.
        // That was invisible while the head element was always empty, and
        // became a duplicated title and stylesheet the moment it wasn't.
        //
        // `<style>` is emitted here like any other element, WHERE IT SITS. The
        // serializer used to skip every style node and inject a stylesheet
        // rebuilt from the cascade into the head instead — so a `<style>` an
        // author wrote in the body moved to the head, its text came back
        // reformatted rather than as written, and rules from `<link>`ed sheets
        // were inlined into a document that never had them.
        if let Some(head) = root.children.iter().find(|c| c.tag == "head") {
            for child in &head.children {
                serialize_box(child, &mut html);
                html.push('\n');
            }
        }
        html.push_str("</head>\n<body>");

        // When emitting an html/head/body wrapper, skip the body tag in the DOM
        // tree to avoid double-nesting (<body><body>…</body></body>).
        for child in &root.children {
            if child.tag == "head" {
                continue; // written into the head above
            } else if child.tag == "body" {
                for grandchild in &child.children {
                    serialize_box(grandchild, &mut html);
                }
            } else {
                serialize_box(child, &mut html);
            }
        }

        html.push_str("</body>\n</html>");
    } else {
        serialize_box(root, &mut html);
    }

    html
}
