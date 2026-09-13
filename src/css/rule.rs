//! `CssRule`, `Declarations`, and the keyword value parsers.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── CSS Rule ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum PseudoElement {
    None,      // regular rule
    Before,    // ::before
    After,     // ::after
    Selection, // ::selection
    Placeholder,
    Marker,   // ::marker
    Backdrop, // ::backdrop
    Ignored,  // ::first-line, ::first-letter, unknown vendor pseudo-elements
}

impl Default for PseudoElement {
    fn default() -> Self {
        Self::None
    }
}

/// How far an AUTHOR rule outranks a UA rule.
///
/// CSS Cascade §6.1 orders NORMAL declarations UA < user < author, and origin
/// is checked before specificity — so author `* { padding: 0 }` (specificity 0)
/// beats UA `ul { padding-left: 40px }` (specificity 1). Origin is encoded by
/// adding this to every author rule's specificity, which keeps the whole
/// cascade a single sorted list. Real specificities are three digits per
/// component; nothing legitimate comes near 100 000.
///
/// For IMPORTANT declarations that order REVERSES (§6.3) — see
/// [`is_author_origin`], which the `!important` passes use to apply UA rules
/// LAST instead of first.
pub const AUTHOR_ORIGIN_BOOST: u32 = 100_000;

/// Was this matched rule's specificity boosted, i.e. does it come from an
/// author stylesheet rather than the UA sheet?
pub fn is_author_origin(specificity: u32) -> bool {
    specificity >= AUTHOR_ORIGIN_BOOST
}

/// One declaration block, **in source order**.
///
/// CSS Cascade §6.4.4 breaks a tie between two declarations of equal origin and
/// specificity by ORDER OF APPEARANCE — the later one wins. That is not a
/// nicety: `border: 5px solid; border-top: none` and `margin: 20px;
/// margin-top: 0` are ordinary CSS, and they only mean what they say if the
/// shorthand is applied before the longhand that follows it.
///
/// This was a `HashMap<String, String>`, so the block was applied in hash
/// order — and Rust's default hasher is seeded per PROCESS, so the order was
/// different on every run. The two blocks above each flipped a coin at startup:
/// `examples/html/subgrid.html` rendered 1978, 1979 or 1980 pixels tall
/// depending on whether `border-top: none` happened to be applied before or
/// after the `border` shorthand it was written to override.
///
/// A Vec keeps the order the parser saw. `insert` drops any earlier entry for
/// the same property and appends, because a re-declared property takes the
/// LATER position: in `border-top: dashed; border: 5px; border-top: none` the
/// winning `border-top` is the one after the shorthand.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declarations {
    entries: Vec<(String, String)>,
}

impl Declarations {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set `prop`, moving it to the end of the block.
    pub fn insert(&mut self, prop: String, value: String) {
        self.entries.retain(|(k, _)| k != &prop);
        self.entries.push((prop, value));
    }

    pub fn get(&self, prop: &str) -> Option<&String> {
        self.entries.iter().find(|(k, _)| k == prop).map(|(_, v)| v)
    }

    pub fn contains_key(&self, prop: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == prop)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, (String, String)> {
        self.entries.iter()
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }
    pub fn values(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(_, v)| v)
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a Declarations {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl FromIterator<(String, String)> for Declarations {
    fn from_iter<T: IntoIterator<Item = (String, String)>>(iter: T) -> Self {
        let mut d = Declarations::new();
        for (k, v) in iter {
            d.insert(k, v);
        }
        d
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageRule {
    pub selector: String,
    pub declarations: Declarations,
    pub important_declarations: Declarations,
    pub margin_rules: Vec<PageMarginRule>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageMarginRule {
    pub name: String,
    pub declarations: Declarations,
    pub important_declarations: Declarations,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CounterStyleRule {
    pub name: String,
    pub declarations: Declarations,
    pub important_declarations: Declarations,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScopeFrame {
    pub root: Option<CssSelector>,
    pub limit: Option<CssSelector>,
}

#[derive(Clone, Debug)]
pub struct CssRule {
    pub selectors: Vec<CssSelector>,
    pub declarations: Declarations,
    pub important_declarations: Declarations,
    /// Pre-resolved declarations: (PropertyId, value_string).
    /// Populated during `compile_declarations()`. Used by the cascade for
    /// fast enum dispatch instead of string matching.
    pub compiled_decls: Vec<(properties::PropertyId, crate::types::CssValue)>,
    /// Pre-resolved important declarations.
    pub compiled_important: Vec<(properties::PropertyId, crate::types::CssValue)>,
    pub specificity: u32, // max of all selectors
    /// The `@layer` this rule was declared in — empty when unlayered.
    ///
    /// ⛔ Layers sort ABOVE specificity (CSS Cascade 5 §6.4.4) and unlayered
    /// normal declarations beat every layered one, so this cannot be folded
    /// into `specificity`.
    pub layer: String,
    /// Where `layer` sorts, resolved once when the stylesheet index is built.
    ///
    /// `u32::MAX` for unlayered, which is what makes an unlayered normal
    /// declaration beat every layered one. Kept on the rule so the cascade's
    /// sort is a field read rather than a name lookup per comparison.
    pub layer_rank: u32,
    pub media_condition: String,             // non-empty if inside @media
    pub container_condition: String,         // non-empty if inside @container
    pub container_name: String,              // optional container name (empty = unnamed)
    pub scope_selector: Option<CssSelector>, // @scope root selector, when present
    pub scope_limit_selector: Option<CssSelector>, // @scope limit selector from `to (...)`
    pub scopes: Vec<ScopeFrame>,             // Nested scope frames, from outermost to innermost
    pub original_selector: String,           // verbatim selector text for roundtrip
    pub is_hover: bool,
    /// True if any declaration value contains `var(` — needs slow-path resolution.
    pub has_var_refs: bool,
    pub pseudo_element: PseudoElement,
}

impl Default for CssRule {
    fn default() -> Self {
        Self {
            layer: String::new(),
            layer_rank: u32::MAX,
            selectors: Vec::new(),
            declarations: Declarations::new(),
            important_declarations: Declarations::new(),
            compiled_decls: Vec::new(),
            compiled_important: Vec::new(),
            specificity: 0,
            media_condition: String::new(),
            container_condition: String::new(),
            container_name: String::new(),
            scope_selector: None,
            scope_limit_selector: None,
            scopes: Vec::new(),
            original_selector: String::new(),
            is_hover: false,
            has_var_refs: false,
            pseudo_element: PseudoElement::None,
        }
    }
}

impl CssRule {
    /// Pre-compile declarations from HashMap<String,String> into Vec<(PropertyId, CssValue)>.
    /// Called during rebuild_index(). Values are wrapped as CssValue::Raw for now;
    /// Pre-compile declarations into typed CssValue where possible.
    /// Lengths, colors, and numbers are parsed once here instead of on every cascade.
    pub fn compile_declarations(&mut self) {
        self.compiled_decls.clear();
        for (prop, val) in &self.declarations {
            let id = properties::resolve(prop);
            if id == properties::PropertyId::Unknown {
                continue;
            }
            self.compiled_decls.push((id, pre_parse_value(id, val)));
        }
        self.compiled_important.clear();
        for (prop, val) in &self.important_declarations {
            let id = properties::resolve(prop);
            if id == properties::PropertyId::Unknown {
                continue;
            }
            self.compiled_important.push((id, pre_parse_value(id, val)));
        }
        self.has_var_refs = self.declarations.values().any(|v| v.contains("var("))
            || self
                .important_declarations
                .values()
                .any(|v| v.contains("var("));
    }
}

/// Try to pre-parse a CSS value into a typed CssValue at stylesheet compilation time.
/// Falls back to CssValue::Raw for values that can't be pre-parsed (var(), complex shorthands, etc.)
pub(crate) fn pre_parse_value(id: properties::PropertyId, val: &str) -> crate::types::CssValue {
    use crate::types::CssValue;
    use properties::PropertyId::*;
    let v = val.trim();

    // Skip var() references — must be resolved at cascade time
    if v.contains("var(") {
        return CssValue::Raw(val.to_string());
    }

    // Skip shorthand properties — they need string-based expansion into longhands
    if !property_defs::get(id).longhands.is_empty() {
        return CssValue::Raw(val.to_string());
    }

    // Global keywords
    match v {
        "inherit" => return CssValue::Inherit,
        // ⛔ `unset` is NOT `initial`. CSS Cascade 5 §7.3: it "acts as either
        // `inherit` or `initial`, depending on whether the property is
        // inherited or not". Collapsing it here destroyed that distinction
        // before the cascade could act on it — `CssValue::Unset` existed and
        // was never produced — so `color: unset` on a child reset to black
        // instead of inheriting its parent's colour.
        "unset" => return CssValue::Unset,
        "initial" => return CssValue::Initial,
        "revert" => return CssValue::Revert,
        "revert-layer" => return CssValue::RevertLayer,
        _ => {}
    }

    // Try to parse based on property type
    match id {
        // ── Length properties ──
        Width
        | Height
        | MinWidth
        | MinHeight
        | MaxWidth
        | MaxHeight
        | MarginTop
        | MarginRight
        | MarginBottom
        | MarginLeft
        | PaddingTop
        | PaddingRight
        | PaddingBottom
        | PaddingLeft
        | BorderTopWidth
        | BorderRightWidth
        | BorderBottomWidth
        | BorderLeftWidth
        | Top
        | Right
        | Bottom
        | Left
        | FlexBasis
        | LetterSpacing
        | WordSpacing
        | TextIndent
        | Gap
        | RowGap
        | ColumnGap
        | TextDecorationThickness
        | TextUnderlineOffset
        | InlineSize
        | BlockSize
        | BorderTopLeftRadius
        | BorderTopRightRadius
        | BorderBottomLeftRadius
        | BorderBottomRightRadius
        | InsetBlockStart
        | InsetBlockEnd
        | InsetInlineStart
        | InsetInlineEnd => {
            if let Some(l) = try_parse_length(v) {
                return CssValue::Length(l);
            }
        }

        // ⛔ `line-height` is NOT a plain length. A unitless number is a
        // MULTIPLE of the font size and `normal` is its own value (CSS 2.1
        // §10.8.1), so reading it as a length made `line-height: 1.375` mean
        // 1.375 PIXELS — and only on this path, which left the two value paths
        // disagreeing about the same declaration.
        LineHeight => {
            if v == "normal" || v.parse::<f32>().is_ok() {
                return CssValue::Length(super::parse_line_height(v));
            }
            if let Some(l) = try_parse_length(v) {
                return CssValue::Length(l);
            }
        }

        // ── Color properties ──
        Color | BackgroundColor | BorderTopColor | BorderRightColor | BorderBottomColor
        | BorderLeftColor | OutlineColor | CaretColor | Fill | Stroke => {
            if let Some(c) = try_parse_color(v) {
                return CssValue::Color(c);
            }
        }

        // ── Number properties ──
        Opacity => {
            if let Ok(n) = v.parse::<f32>() {
                return CssValue::Number(n.clamp(0.0, 1.0));
            }
        }
        FlexGrow | FlexShrink => {
            if let Ok(n) = v.parse::<f32>() {
                return CssValue::Number(n);
            }
        }

        // ── Integer properties ──
        ZIndex | Order => {
            if let Ok(n) = v.parse::<i32>() {
                return CssValue::Integer(n);
            }
        }

        // ── Keyword properties ──
        Display => {
            if let Some(d) = parse_display_keyword(v) {
                return CssValue::Display(d);
            }
        }
        Position => {
            if let Some(p) = parse_position_keyword(v) {
                return CssValue::Position(p);
            }
        }
        Float => {
            if let Some(f) = parse_float_keyword(v) {
                return CssValue::Float(f);
            }
        }
        Clear => {
            if let Some(c) = parse_clear_keyword(v) {
                return CssValue::Clear(c);
            }
        }
        BoxSizing => {
            if let Some(b) = parse_box_sizing_keyword(v) {
                return CssValue::BoxSizing(b);
            }
        }
        OverflowX | OverflowY => {
            if let Some(o) = parse_overflow_keyword(v) {
                return CssValue::Overflow(o);
            }
        }
        Visibility => match v {
            "visible" => return CssValue::Visible(true),
            "hidden" => return CssValue::Visible(false),
            "collapse" => return CssValue::Visible(false),
            _ => {}
        },
        TextAlign => {
            if let Some(a) = parse_text_align_keyword(v) {
                return CssValue::TextAlign(a);
            }
        }
        TextTransform => match v {
            "none" => return CssValue::TextTransform(crate::types::TextTransform::None),
            "uppercase" => return CssValue::TextTransform(crate::types::TextTransform::Uppercase),
            "lowercase" => return CssValue::TextTransform(crate::types::TextTransform::Lowercase),
            "capitalize" => {
                return CssValue::TextTransform(crate::types::TextTransform::Capitalize);
            }
            "full-width" => return CssValue::TextTransform(crate::types::TextTransform::FullWidth),
            "full-size-kana" => {
                return CssValue::TextTransform(crate::types::TextTransform::FullSizeKana);
            }
            "math-auto" => return CssValue::TextTransform(crate::types::TextTransform::MathAuto),
            _ => {}
        },
        WhiteSpace => {
            if let Some(w) = parse_white_space_keyword(v) {
                return CssValue::WhiteSpace(w);
            }
        }
        FontWeight => {
            if let Some(w) = parse_font_weight_keyword(v) {
                return CssValue::FontWeight(w);
            }
        }
        FontStyle => match v {
            "normal" => return CssValue::FontStyle(crate::types::FontStyle::Normal),
            "italic" => return CssValue::FontStyle(crate::types::FontStyle::Italic),
            "oblique" => return CssValue::FontStyle(crate::types::FontStyle::Oblique),
            _ => {}
        },
        FlexDirection => match v {
            "row" => return CssValue::FlexDirection(crate::types::FlexDirection::Row),
            "row-reverse" => {
                return CssValue::FlexDirection(crate::types::FlexDirection::RowReverse);
            }
            "column" => return CssValue::FlexDirection(crate::types::FlexDirection::Column),
            "column-reverse" => {
                return CssValue::FlexDirection(crate::types::FlexDirection::ColumnReverse);
            }
            _ => {}
        },
        FlexWrap => match v {
            "nowrap" => return CssValue::FlexWrap(crate::types::FlexWrap::Nowrap),
            "wrap" => return CssValue::FlexWrap(crate::types::FlexWrap::Wrap),
            "wrap-reverse" => return CssValue::FlexWrap(crate::types::FlexWrap::WrapReverse),
            _ => {}
        },
        AlignItems => {
            if let Some(a) = parse_align_items_keyword(v) {
                return CssValue::AlignItems(a);
            }
        }
        JustifyContent => {
            if let Some(j) = parse_justify_content_keyword(v) {
                return CssValue::JustifyContent(j);
            }
        }
        BorderTopStyle | BorderRightStyle | BorderBottomStyle | BorderLeftStyle => {
            if let Some(bs) = parse_border_style_value(v) {
                return CssValue::BorderStyle(bs);
            }
        }
        VerticalAlign => {
            if let Some(va) = parse_vertical_align_keyword(v) {
                return CssValue::VerticalAlign(va);
            }
        }
        WordBreak => match v {
            "normal" => return CssValue::WordBreak(crate::types::WordBreak::Normal),
            "break-all" => return CssValue::WordBreak(crate::types::WordBreak::BreakAll),
            "keep-all" => return CssValue::WordBreak(crate::types::WordBreak::KeepAll),
            "break-word" => return CssValue::WordBreak(crate::types::WordBreak::BreakWord),
            _ => {}
        },

        _ => {}
    }

    // Fallback: keep as raw string
    CssValue::Raw(val.to_string())
}

// ── Keyword parsers for pre_parse_value ──────────────────────────────────────

fn parse_display_keyword(v: &str) -> Option<crate::types::Display> {
    use crate::types::Display::*;
    Some(match v {
        "none" => None,
        "block" => Block,
        "inline" => Inline,
        "inline-block" => InlineBlock,
        "flex" => Flex,
        "inline-flex" => InlineFlex,
        "grid" => Grid,
        "inline-grid" => InlineGrid,
        "table" => Table,
        "table-row" => TableRow,
        "table-cell" => TableCell,
        "table-row-group" => TableRowGroup,
        "table-header-group" => TableHeaderGroup,
        "table-footer-group" => TableFooterGroup,
        "table-column" => TableColumn,
        "table-column-group" => TableColumnGroup,
        "table-caption" => TableCaption,
        "list-item" => ListItem,
        "ruby" => Ruby,
        "ruby-text" => RubyText,
        _ => return Option::None,
    })
}

fn parse_position_keyword(v: &str) -> Option<crate::types::Position> {
    use crate::types::Position::*;
    Some(match v {
        "static" => Static,
        "relative" => Relative,
        "absolute" => Absolute,
        "fixed" => Fixed,
        "sticky" => Sticky,
        _ => return Option::None,
    })
}

fn parse_float_keyword(v: &str) -> Option<crate::types::Float> {
    use crate::types::Float::*;
    Some(match v {
        "none" => None,
        "left" => Left,
        "right" => Right,
        "inline-start" => InlineStart,
        "inline-end" => InlineEnd,
        _ => return Option::None,
    })
}

fn parse_clear_keyword(v: &str) -> Option<crate::types::Clear> {
    use crate::types::Clear::*;
    Some(match v {
        "none" => None,
        "left" => Left,
        "right" => Right,
        "both" => Both,
        "inline-start" => InlineStart,
        "inline-end" => InlineEnd,
        _ => return Option::None,
    })
}

fn parse_box_sizing_keyword(v: &str) -> Option<crate::types::BoxSizing> {
    Some(match v {
        "content-box" => crate::types::BoxSizing::ContentBox,
        "border-box" => crate::types::BoxSizing::BorderBox,
        _ => return None,
    })
}

fn parse_overflow_keyword(v: &str) -> Option<crate::types::Overflow> {
    use crate::types::Overflow::*;
    Some(match v {
        "visible" => Visible,
        "hidden" => Hidden,
        "clip" => Clip,
        "scroll" => Scroll,
        "auto" => Auto,
        _ => return Option::None,
    })
}

fn parse_text_align_keyword(v: &str) -> Option<crate::types::TextAlign> {
    use crate::types::TextAlign::*;
    Some(match v {
        "left" => Left,
        "right" => Right,
        "center" => Center,
        "justify" => Justify,
        "start" => Start,
        "end" => End,
        _ => return Option::None,
    })
}

fn parse_white_space_keyword(v: &str) -> Option<crate::types::WhiteSpace> {
    use crate::types::WhiteSpace::*;
    Some(match v {
        "normal" => Normal,
        "nowrap" => Nowrap,
        "pre" => Pre,
        "pre-wrap" => PreWrap,
        "pre-line" => PreLine,
        _ => return Option::None,
    })
}

fn parse_font_weight_keyword(v: &str) -> Option<crate::types::FontWeight> {
    use crate::types::FontWeight;
    let lower = v.trim().to_ascii_lowercase();
    Some(match lower.as_str() {
        "normal" => FontWeight::Normal,
        "bold" => FontWeight::Bold,
        _ => {
            if let Ok(n) = lower.parse::<u16>() {
                if !(1..=1000).contains(&n) {
                    return None;
                }
                FontWeight::Value(n)
            } else {
                return None;
            }
        }
    })
}

fn parse_align_items_keyword(v: &str) -> Option<crate::types::AlignItems> {
    use crate::types::AlignItems::*;
    Some(match v {
        "stretch" => Stretch,
        "flex-start" | "start" => FlexStart,
        "flex-end" | "end" => FlexEnd,
        "center" => Center,
        "baseline" => Baseline,
        _ => return Option::None,
    })
}

fn parse_justify_content_keyword(v: &str) -> Option<crate::types::JustifyContent> {
    use crate::types::JustifyContent::*;
    Some(match v {
        "flex-start" | "start" => FlexStart,
        "flex-end" | "end" => FlexEnd,
        "center" => Center,
        "space-between" => SpaceBetween,
        "space-around" => SpaceAround,
        "space-evenly" => SpaceEvenly,
        _ => return Option::None,
    })
}

fn parse_border_style_value(v: &str) -> Option<crate::types::BorderStyleValue> {
    use crate::types::BorderStyleValue as BSV;
    Some(match v {
        "none" => BSV::None,
        "hidden" => BSV::Hidden,
        "solid" => BSV::Solid,
        "dashed" => BSV::Dashed,
        "dotted" => BSV::Dotted,
        "double" => BSV::Double,
        "groove" => BSV::Groove,
        "ridge" => BSV::Ridge,
        "inset" => BSV::Inset,
        "outset" => BSV::Outset,
        _ => return Option::None,
    })
}

fn parse_vertical_align_keyword(v: &str) -> Option<crate::types::VerticalAlign> {
    use crate::types::VerticalAlign::*;
    Some(match v {
        "baseline" => Baseline,
        "top" => Top,
        "middle" => Middle,
        "bottom" => Bottom,
        "text-top" => TextTop,
        "text-bottom" => TextBottom,
        "sub" => Sub,
        "super" => Super,
        _ => return Option::None,
    })
}

/// Try to parse a CSS length value. Returns None for values that aren't pure lengths.
fn try_parse_length(v: &str) -> Option<crate::types::CssLength> {
    let v = v.trim();
    if v == "auto" {
        return Some(crate::types::CssLength::Auto);
    }
    if v == "none" {
        return Some(crate::types::CssLength::None);
    }
    if v == "0" {
        return Some(crate::types::CssLength::Zero);
    }
    if v == "0px" {
        return Some(crate::types::CssLength::Px(0.0));
    }
    // Use the existing parse_length which handles px, em, rem, %, vw, vh, calc, min, max, clamp
    let l = parse_length(v);
    // parse_length returns Auto for unrecognized values — only accept if it parsed to something specific
    match l {
        crate::types::CssLength::Auto => {
            // Only return Auto if the input was actually "auto"
            if v == "auto" { Some(l) } else { None }
        }
        _ => Some(l),
    }
}

/// Try to parse a CSS color value. Returns None for values that aren't colors.
fn try_parse_color(v: &str) -> Option<crate::types::Color> {
    let v = v.trim();
    if v == "transparent" {
        return Some(crate::types::Color::TRANSPARENT);
    }
    if v == "currentcolor" || v == "currentColor" {
        return None;
    } // needs cascade context
    let c = match parse_color(v) {
        Some(c) => c,
        None => return None,
    };
    // parse_color succeeded — accept for hex, rgb, hsl
    if v.starts_with('#') || v.starts_with("rgb") || v.starts_with("hsl") {
        return Some(c);
    }
    // Named colors
    if v == "black" {
        return Some(c);
    }
    if c != crate::types::Color::BLACK {
        return Some(c);
    }
    // Could also be a valid named color that happens to be black — check common names
    let lower = v.to_ascii_lowercase();
    match lower.as_str() {
        "white" | "red" | "green" | "blue" | "yellow" | "cyan" | "magenta" | "gray" | "grey"
        | "orange" | "purple" | "pink" | "brown" | "navy" | "teal" | "olive" | "lime" | "aqua"
        | "fuchsia" | "silver" | "maroon" | "darkred" | "darkgreen" | "darkblue" | "lightgray"
        | "lightgrey" | "whitesmoke" | "gainsboro" | "ghostwhite" | "aliceblue" | "indianred"
        | "lightcoral" | "salmon" | "darksalmon" | "lightsalmon" | "crimson" | "firebrick"
        | "tomato" | "coral" | "orangered" | "gold" | "khaki" | "darkkhaki" | "plum" | "violet"
        | "orchid" | "thistle" | "lavender" | "steelblue" | "royalblue" | "cornflowerblue"
        | "midnightblue" | "slateblue" | "darkslateblue" | "mediumslateblue" | "darkgray"
        | "darkgrey" | "dimgray" | "dimgrey" | "lightslategray" | "slategray" | "slategrey" => {
            return Some(c);
        }
        _ => {}
    }
    None
}
