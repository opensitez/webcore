//! CSS value parsers — lengths, colours, shorthands.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

// ─── Value Parsers ────────────────────────────────────────────────────────────

/// Cache for parsed CSS length values — avoids re-parsing the same string
/// (e.g. "100%" or "calc(100% - 21.5rem)") thousands of times during cascade.
static LENGTH_CACHE: std::sync::LazyLock<std::sync::Mutex<HashMap<String, CssLength>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn parse_length(v: &str) -> CssLength {
    let v = v.trim();
    if v == "auto" {
        return CssLength::Auto;
    }
    // CSS Sizing §5 — intrinsic sizing keywords. They are not lengths; a
    // consumer that cannot measure content reads them as `auto` (see
    // `CssLength::is_auto`), and the flex algorithm matches them directly.
    if v == "min-content" {
        return CssLength::MinContent;
    }
    if v == "max-content" {
        return CssLength::MaxContent;
    }
    if v == "fit-content" {
        return CssLength::FitContent;
    }
    // css-sizing-3 §6.1 — the function form carries the size to clamp to.
    if let Some(arg) = v
        .strip_prefix("fit-content(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return CssLength::FitContentArg(Box::new(parse_length(arg.trim())));
    }
    if v == "0" {
        return CssLength::Zero;
    }
    // Check cache for previously parsed values
    if let Ok(cache) = LENGTH_CACHE.lock() {
        if let Some(cached) = cache.get(v) {
            return cached.clone();
        }
    }
    let result = parse_length_inner(v);
    // Cache the result (only for non-trivial values)
    if v.len() > 4 {
        if let Ok(mut cache) = LENGTH_CACHE.lock() {
            if cache.len() < 50000 {
                cache.insert(v.to_string(), result.clone());
            }
        }
    }
    result
}

fn parse_length_inner(v: &str) -> CssLength {
    if let Some(inner) = v.strip_prefix("env(").and_then(|s| s.strip_suffix(')')) {
        return parse_env_length(inner);
    }
    if let Some(inner) = v.strip_prefix("calc(").and_then(|s| s.strip_suffix(')')) {
        return parse_calc(inner);
    }
    // CSS min()/max()/clamp() — proper AST with lazy resolution at layout time
    if let Some(inner) = v.strip_prefix("min(").and_then(|s| s.strip_suffix(')')) {
        let args = split_top_level_commas(inner);
        if args.len() >= 2 {
            let vals: Vec<CssLength> = args.iter().map(|a| parse_length(a.trim())).collect();
            return CssLength::Min(Box::new(vals));
        }
        return parse_length(inner);
    }
    if let Some(inner) = v.strip_prefix("max(").and_then(|s| s.strip_suffix(')')) {
        let args = split_top_level_commas(inner);
        if args.len() >= 2 {
            let vals: Vec<CssLength> = args.iter().map(|a| parse_length(a.trim())).collect();
            return CssLength::Max(Box::new(vals));
        }
        return parse_length(inner);
    }
    if let Some(inner) = v.strip_prefix("clamp(").and_then(|s| s.strip_suffix(')')) {
        let args = split_top_level_commas(inner);
        if args.len() == 3 {
            let min = parse_length(args[0].trim());
            let val = parse_length(args[1].trim());
            let max = parse_length(args[2].trim());
            return CssLength::Clamp(Box::new([min, val, max]));
        }
        // Fallback: treat as calc
        return parse_length(inner);
    }
    // ⛔ Split the NUMBER from the UNIT, then match the unit exactly. A chain of
    // `ends_with` cannot do this safely, because unit names nest: `in` is a
    // suffix of `vmin`, so testing `in` before `vmin` parses `3vmin` as three
    // INCHES. The old chain got away with it only by not supporting `in`.
    //
    // Units are ASCII case-insensitive (CSS Values 4 §3.1), which the old
    // chain also did not honour — `10PX` fell through to `auto`.
    // `e` is exponent syntax only when a digit follows it — `3em` must not eat it.
    let split = {
        let mut i = 0usize;
        let b = v.as_bytes();
        let mut seen_digit = false;
        while i < b.len() {
            let c = b[i] as char;
            if c.is_ascii_digit() {
                seen_digit = true;
                i += 1;
            } else if c == '.' || ((c == '-' || c == '+') && i == 0) {
                i += 1;
            } else if (c == 'e' || c == 'E')
                && seen_digit
                && i + 1 < b.len()
                && ((b[i + 1] as char).is_ascii_digit()
                    || ((b[i + 1] == b'-' || b[i + 1] == b'+')
                        && i + 2 < b.len()
                        && (b[i + 2] as char).is_ascii_digit()))
            {
                i += 2;
            } else {
                break;
            }
        }
        i
    };
    let (num, unit) = v.split_at(split);
    let n: f32 = match num.parse() {
        Ok(n) => n,
        Err(_) => return CssLength::Auto,
    };
    let unit_lower = unit.to_ascii_lowercase();
    return match unit_lower.as_str() {
        "" => CssLength::Px(n), // unitless — treated as px, as before
        "%" => CssLength::Percent(n),
        "px" => CssLength::Px(n),
        "em" => CssLength::Em(n),
        "rem" => CssLength::Rem(n),
        "vw" => CssLength::Vw(n),
        "vh" => CssLength::Vh(n),
        // ── Absolute units, CSS Values 4 §6.2. Exact, not approximations:
        //    1in = 96px, 1cm = 96px/2.54, 1mm = cm/10, 1Q = cm/40,
        //    1pc = in/6, 1pt = in/72.
        "in" => CssLength::Px(n * 96.0),
        "cm" => CssLength::Px(n * 96.0 / 2.54),
        "mm" => CssLength::Px(n * 96.0 / 25.4),
        "q" => CssLength::Px(n * 96.0 / 101.6),
        "pc" => CssLength::Px(n * 16.0),
        "pt" => CssLength::Px(n * 96.0 / 72.0),
        "vmin" => CssLength::Vmin(n),
        "vmax" => CssLength::Vmax(n),
        // ── Font-relative units, CSS Values 4 §6.1.1 ──
        //
        // The spec provides these fallbacks itself, for the case where the
        // real font metric is "impossible or impractical to determine", and
        // says they MUST be assumed then. They are conforming values, not
        // guesses — but they are the fallbacks, not measurements: `ex` and
        // `ch` should come from the first available font's x-height and its
        // "0" advance, which needs font metrics the length resolver is not
        // given. Chrome, which does measure, answers 8.9px for `1ex` at 16px
        // where the fallback gives 8px.
        "ex" => CssLength::Em(n * 0.5), // "a value of 0.5em must be assumed"
        "ch" => CssLength::Em(n * 0.5), // "falls back to 0.5em in the general case"
        "ic" => CssLength::Em(n),       // "must be assumed to be 1em"
        "cap" => CssLength::Em(n),
        // Root-relative counterparts resolve against the ROOT font size.
        "rex" => CssLength::Rem(n * 0.5),
        "rch" => CssLength::Rem(n * 0.5),
        "ric" => CssLength::Rem(n),
        "rcap" => CssLength::Rem(n),
        // ── Viewport variants, CSS Values 4 §6.1.2 ──
        //
        // The small, large and dynamic viewports coincide on a UA that shows
        // no dynamically-retracting toolbars, which is this one — so `svh`,
        // `lvh` and `dvh` are all `vh` here, and that is conforming rather
        // than an approximation.
        "svh" | "lvh" | "dvh" => CssLength::Vh(n),
        "svw" | "lvw" | "dvw" => CssLength::Vw(n),
        "svmin" | "lvmin" | "dvmin" => CssLength::Vmin(n),
        "svmax" | "lvmax" | "dvmax" => CssLength::Vmax(n),
        // Logical viewport axes. webcore lays out horizontal-tb, in which the
        // inline axis is the width and the block axis the height.
        "vi" => CssLength::Vw(n),
        "vb" => CssLength::Vh(n),
        // Container query units need the selected query container's dimensions
        // to be exact. Until length resolution carries that container context,
        // use the viewport-shaped fallback variants so valid declarations do
        // not collapse to `auto`.
        "cqw" | "cqi" => CssLength::Vw(n),
        "cqh" | "cqb" => CssLength::Vh(n),
        "cqmin" => CssLength::Vmin(n),
        "cqmax" => CssLength::Vmax(n),
        _ => CssLength::Auto,
    };
}

fn parse_env_length(inner: &str) -> CssLength {
    let args = split_top_level_commas(inner);
    let name = args.first().map(|s| s.trim()).unwrap_or("");
    if is_zero_env_length(name) {
        return CssLength::Zero;
    }
    if args.len() >= 2 {
        return parse_length(args[1].trim());
    }
    CssLength::Auto
}

fn parse_env_color(inner: &str) -> Option<Color> {
    let args = split_top_level_commas(inner);
    let name = args.first().map(|s| s.trim()).unwrap_or("");
    if is_zero_env_length(name) {
        return None;
    }
    parse_color(args.get(1)?.trim())
}

fn is_zero_env_length(name: &str) -> bool {
    matches!(
        name,
        "safe-area-inset-top"
            | "safe-area-inset-right"
            | "safe-area-inset-bottom"
            | "safe-area-inset-left"
            | "safe-area-max-inset-top"
            | "safe-area-max-inset-right"
            | "safe-area-max-inset-bottom"
            | "safe-area-max-inset-left"
            | "titlebar-area-x"
            | "titlebar-area-y"
            | "titlebar-area-width"
            | "titlebar-area-height"
            | "keyboard-inset-top"
            | "keyboard-inset-right"
            | "keyboard-inset-bottom"
            | "keyboard-inset-left"
            | "keyboard-inset-width"
            | "keyboard-inset-height"
    )
}

/// Find the index of the closing `)` that matches the opening `(` at position 0.
/// Split a string at top-level commas (not inside nested parentheses).
pub(crate) fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Split a CSS value on whitespace while keeping parenthesized expressions
/// (calc, var, min, max, clamp, rgb, etc.) intact as single tokens.
pub fn split_css_shorthand_values(s: &str) -> Vec<String> {
    split_css_values(s)
}
pub(crate) fn split_css_values(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in s.chars() {
        match c {
            '(' => {
                depth += 1;
                current.push(c);
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(c);
            }
            c if c.is_ascii_whitespace() && depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    parts.push(trimmed);
                }
                current.clear();
            }
            _ => {
                current.push(c);
            }
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}

/// A length that reports NOT PARSING, instead of falling back to `auto`.
///
/// `parse_length` answers `CssLength::Auto` for anything it cannot read, which
/// is indistinguishable from the author writing `auto` — so an invalid
/// declaration overwrote the cascade's winner with `auto` rather than being
/// dropped (css-syntax-3 §9: a declaration with an invalid value is ignored).
/// Only the literal keyword legitimately produces `Auto`.
pub fn parse_length_checked(v: &str) -> Option<CssLength> {
    let t = v.trim();
    let l = parse_length(t);
    if matches!(l, CssLength::Auto) && !t.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(l)
    }
}

pub fn parse_length_or_none(v: &str) -> CssLength {
    if v == "none" {
        CssLength::None
    } else {
        parse_length(v)
    }
}

pub fn parse_font_size_checked(v: &str) -> Option<CssLength> {
    let t = v.trim();
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "xx-small" => CssLength::Px(9.0),
        "x-small" => CssLength::Px(10.0),
        "small" => CssLength::Px(13.0),
        "medium" => CssLength::Px(16.0),
        "large" => CssLength::Px(18.0),
        "x-large" => CssLength::Px(24.0),
        "xx-large" => CssLength::Px(32.0),
        "xxx-large" => CssLength::Px(48.0),
        "smaller" => CssLength::Em(0.83),
        "larger" => CssLength::Em(1.17),
        _ if t == "0" || t == "+0" || t == "-0" => CssLength::Zero,
        _ if t.parse::<f32>().is_ok() => return None,
        _ => parse_length_checked(t)?,
    }
    .into()
}

pub fn parse_font_size(v: &str) -> CssLength {
    parse_font_size_checked(v).unwrap_or(CssLength::Auto)
}

pub fn parse_line_height(v: &str) -> CssLength {
    if v == "normal" {
        return CssLength::Em(1.2);
    }
    // Unitless number: treat as em
    if let Ok(n) = v.parse::<f32>() {
        return CssLength::Em(n);
    }
    parse_length(v)
}

pub fn parse_overflow(v: &str) -> Overflow {
    match v {
        "hidden" => Overflow::Hidden,
        "clip" => Overflow::Clip,
        "scroll" => Overflow::Scroll,
        "auto" => Overflow::Auto,
        _ => Overflow::Visible,
    }
}

pub fn parse_overscroll(v: &str) -> OverscrollBehavior {
    match v.trim() {
        "none" => OverscrollBehavior::None,
        "contain" => OverscrollBehavior::Contain,
        _ => OverscrollBehavior::Auto,
    }
}

pub fn parse_border_style(v: &str) -> BorderStyle {
    try_parse_border_style(v).unwrap_or(BorderStyle::None)
}

pub(crate) fn try_parse_border_style(v: &str) -> Option<BorderStyle> {
    Some(match v {
        "none" => BorderStyle::None,
        "hidden" => BorderStyle::Hidden,
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "groove" => BorderStyle::Groove,
        "ridge" => BorderStyle::Ridge,
        "inset" => BorderStyle::Inset,
        "outset" => BorderStyle::Outset,
        _ => return None,
    })
}

/// Parse a CSS color value into a `Color`.
pub fn parse_color(v: &str) -> Option<Color> {
    // ⛔ Keywords are ASCII case-insensitive (css-values-4 §3.1). Matching the
    // table byte-exactly dropped `bgcolor="White"` and `color="Red"`, which
    // legacy presentational HTML still ships, leaving the element at its default.
    let lowered = v.trim().to_ascii_lowercase();
    let v = lowered.as_str();
    let v = v.trim();

    if let Some(inner) = v.strip_prefix("env(").and_then(|s| s.strip_suffix(')')) {
        return parse_env_color(inner);
    }

    // light-dark(light, dark) — use light value (we render in light mode)
    if v.starts_with("light-dark(") {
        if let Some(inner) = v
            .strip_prefix("light-dark(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let comma = inner.find(',')?;
            return parse_color(inner[..comma].trim());
        }
    }

    // Named colors
    let named = match v {
        "black" => Some(Color::rgb(0, 0, 0)),
        "white" => Some(Color::rgb(255, 255, 255)),
        "red" => Some(Color::rgb(255, 0, 0)),
        "green" => Some(Color::rgb(0, 128, 0)),
        "blue" => Some(Color::rgb(0, 0, 255)),
        "yellow" => Some(Color::rgb(255, 255, 0)),
        "orange" => Some(Color::rgb(255, 165, 0)),
        "purple" => Some(Color::rgb(128, 0, 128)),
        "pink" => Some(Color::rgb(255, 192, 203)),
        "gray" | "grey" => Some(Color::rgb(128, 128, 128)),
        "darkgray" | "darkgrey" => Some(Color::rgb(169, 169, 169)),
        "lightgray" | "lightgrey" => Some(Color::rgb(211, 211, 211)),
        "darkslategray" | "darkslategrey" => Some(Color::rgb(47, 79, 79)),
        "slategray" | "slategrey" => Some(Color::rgb(112, 128, 144)),
        "lightslategray" | "lightslategrey" => Some(Color::rgb(119, 136, 153)),
        "dimgray" | "dimgrey" => Some(Color::rgb(105, 105, 105)),
        "gainsboro" => Some(Color::rgb(220, 220, 220)),
        "whitesmoke" => Some(Color::rgb(245, 245, 245)),
        "silver" => Some(Color::rgb(192, 192, 192)),
        "navy" => Some(Color::rgb(0, 0, 128)),
        "teal" => Some(Color::rgb(0, 128, 128)),
        "aqua" | "cyan" => Some(Color::rgb(0, 255, 255)),
        "fuchsia" | "magenta" => Some(Color::rgb(255, 0, 255)),
        "maroon" => Some(Color::rgb(128, 0, 0)),
        "olive" => Some(Color::rgb(128, 128, 0)),
        "lime" => Some(Color::rgb(0, 255, 0)),
        "darkred" => Some(Color::rgb(139, 0, 0)),
        "darkgreen" => Some(Color::rgb(0, 100, 0)),
        "darkblue" => Some(Color::rgb(0, 0, 139)),
        "darkcyan" => Some(Color::rgb(0, 139, 139)),
        "darkmagenta" => Some(Color::rgb(139, 0, 139)),
        "darkorange" => Some(Color::rgb(255, 140, 0)),
        "darkviolet" => Some(Color::rgb(148, 0, 211)),
        "darkgoldenrod" => Some(Color::rgb(184, 134, 11)),
        "darkkhaki" => Some(Color::rgb(189, 183, 107)),
        "darkturquoise" => Some(Color::rgb(0, 206, 209)),
        "darkorchid" => Some(Color::rgb(153, 50, 204)),
        "darkseagreen" => Some(Color::rgb(143, 188, 143)),
        "darksalmon" => Some(Color::rgb(233, 150, 122)),
        "indigo" => Some(Color::rgb(75, 0, 130)),
        "violet" => Some(Color::rgb(238, 130, 238)),
        "orchid" => Some(Color::rgb(218, 112, 214)),
        "plum" => Some(Color::rgb(221, 160, 221)),
        "thistle" => Some(Color::rgb(216, 191, 216)),
        "lavender" => Some(Color::rgb(230, 230, 250)),
        "mediumpurple" => Some(Color::rgb(147, 112, 219)),
        "blueviolet" => Some(Color::rgb(138, 43, 226)),
        "rebeccapurple" => Some(Color::rgb(102, 51, 153)),
        "mediumblue" => Some(Color::rgb(0, 0, 205)),
        "royalblue" => Some(Color::rgb(65, 105, 225)),
        "cornflowerblue" => Some(Color::rgb(100, 149, 237)),
        "deepskyblue" => Some(Color::rgb(0, 191, 255)),
        "dodgerblue" => Some(Color::rgb(30, 144, 255)),
        "lightblue" => Some(Color::rgb(173, 216, 230)),
        "lightskyblue" => Some(Color::rgb(135, 206, 250)),
        "skyblue" => Some(Color::rgb(135, 206, 235)),
        "steelblue" => Some(Color::rgb(70, 130, 180)),
        "cadetblue" => Some(Color::rgb(95, 158, 160)),
        "powderblue" => Some(Color::rgb(176, 224, 230)),
        "lightcyan" => Some(Color::rgb(224, 255, 255)),
        "paleturquoise" => Some(Color::rgb(175, 238, 238)),
        "mediumturquoise" => Some(Color::rgb(72, 209, 204)),
        "turquoise" => Some(Color::rgb(64, 224, 208)),
        "aquamarine" => Some(Color::rgb(127, 255, 212)),
        "mediumaquamarine" => Some(Color::rgb(102, 205, 170)),
        "lightgreen" => Some(Color::rgb(144, 238, 144)),
        "limegreen" => Some(Color::rgb(50, 205, 50)),
        "mediumseagreen" => Some(Color::rgb(60, 179, 113)),
        "seagreen" => Some(Color::rgb(46, 139, 87)),
        "forestgreen" => Some(Color::rgb(34, 139, 34)),
        "olivedrab" => Some(Color::rgb(107, 142, 35)),
        "yellowgreen" => Some(Color::rgb(154, 205, 50)),
        "chartreuse" => Some(Color::rgb(127, 255, 0)),
        "lawngreen" => Some(Color::rgb(124, 252, 0)),
        "greenyellow" => Some(Color::rgb(173, 255, 47)),
        "palegreen" => Some(Color::rgb(152, 251, 152)),
        "springgreen" => Some(Color::rgb(0, 255, 127)),
        "mediumspringgreen" => Some(Color::rgb(0, 250, 154)),
        "mintcream" => Some(Color::rgb(245, 255, 250)),
        "honeydew" => Some(Color::rgb(240, 255, 240)),
        "lemonchiffon" => Some(Color::rgb(255, 250, 205)),
        "gold" => Some(Color::rgb(255, 215, 0)),
        "goldenrod" => Some(Color::rgb(218, 165, 32)),
        "palegoldenrod" => Some(Color::rgb(238, 232, 170)),
        "wheat" => Some(Color::rgb(245, 222, 179)),
        "moccasin" => Some(Color::rgb(255, 228, 181)),
        "navajowhite" => Some(Color::rgb(255, 222, 173)),
        "peachpuff" => Some(Color::rgb(255, 218, 185)),
        "bisque" => Some(Color::rgb(255, 228, 196)),
        "blanchedalmond" => Some(Color::rgb(255, 235, 205)),
        "papayawhip" => Some(Color::rgb(255, 239, 213)),
        "antiquewhite" => Some(Color::rgb(250, 235, 215)),
        "cornsilk" => Some(Color::rgb(255, 248, 220)),
        "oldlace" => Some(Color::rgb(253, 245, 230)),
        "floralwhite" => Some(Color::rgb(255, 250, 240)),
        "ivory" => Some(Color::rgb(255, 255, 240)),
        "beige" => Some(Color::rgb(245, 245, 220)),
        "khaki" => Some(Color::rgb(240, 230, 140)),
        "tan" => Some(Color::rgb(210, 180, 140)),
        "burlywood" => Some(Color::rgb(222, 184, 135)),
        "sandybrown" => Some(Color::rgb(244, 164, 96)),
        "peru" => Some(Color::rgb(205, 133, 63)),
        "chocolate" => Some(Color::rgb(210, 105, 30)),
        "sienna" => Some(Color::rgb(160, 82, 45)),
        "saddlebrown" => Some(Color::rgb(139, 69, 19)),
        "brown" => Some(Color::rgb(165, 42, 42)),
        "firebrick" => Some(Color::rgb(178, 34, 34)),
        "crimson" => Some(Color::rgb(220, 20, 60)),
        "tomato" => Some(Color::rgb(255, 99, 71)),
        "coral" => Some(Color::rgb(255, 127, 80)),
        "salmon" => Some(Color::rgb(250, 128, 114)),
        "lightsalmon" => Some(Color::rgb(255, 160, 122)),
        "lightcoral" => Some(Color::rgb(240, 128, 128)),
        "lightyellow" => Some(Color::rgb(255, 255, 224)),
        "lightgoldenrodyellow" => Some(Color::rgb(250, 250, 210)),
        "lightseagreen" => Some(Color::rgb(32, 178, 170)),
        "lightsteelblue" => Some(Color::rgb(176, 196, 222)),
        "orangered" => Some(Color::rgb(255, 69, 0)),
        "hotpink" => Some(Color::rgb(255, 105, 180)),
        "deeppink" => Some(Color::rgb(255, 20, 147)),
        "lightpink" => Some(Color::rgb(255, 182, 193)),
        "palevioletred" => Some(Color::rgb(219, 112, 147)),
        "mediumvioletred" => Some(Color::rgb(199, 21, 133)),
        "rosybrown" => Some(Color::rgb(188, 143, 143)),
        "mistyrose" => Some(Color::rgb(255, 228, 225)),
        "lavenderblush" => Some(Color::rgb(255, 240, 245)),
        "aliceblue" => Some(Color::rgb(240, 248, 255)),
        "ghostwhite" => Some(Color::rgb(248, 248, 255)),
        "azure" => Some(Color::rgb(240, 255, 255)),
        "snow" => Some(Color::rgb(255, 250, 250)),
        "seashell" => Some(Color::rgb(255, 245, 238)),
        "linen" => Some(Color::rgb(250, 240, 230)),
        "transparent" => Some(Color::TRANSPARENT),
        // CSS system colors. These are light/default-theme approximations
        // until forced-colors rendering can resolve them from platform state.
        "canvas" => Some(Color::rgb(255, 255, 255)),
        "canvastext" => Some(Color::rgb(0, 0, 0)),
        "linktext" => Some(Color::rgb(0, 0, 238)),
        "visitedtext" => Some(Color::rgb(85, 26, 139)),
        "activetext" => Some(Color::rgb(238, 0, 0)),
        "buttonface" => Some(Color::rgb(240, 240, 240)),
        "buttontext" => Some(Color::rgb(0, 0, 0)),
        "buttonborder" => Some(Color::rgb(118, 118, 118)),
        "field" => Some(Color::rgb(255, 255, 255)),
        "fieldtext" => Some(Color::rgb(0, 0, 0)),
        "highlight" | "selecteditem" => Some(Color::rgb(0, 120, 215)),
        "highlighttext" | "selecteditemtext" => Some(Color::rgb(255, 255, 255)),
        "graytext" => Some(Color::rgb(128, 128, 128)),
        "mark" => Some(Color::rgb(255, 255, 0)),
        "marktext" => Some(Color::rgb(0, 0, 0)),
        "accentcolor" => Some(Color::rgb(0, 120, 215)),
        "accentcolortext" => Some(Color::rgb(255, 255, 255)),
        "currentcolor" => None, // can't resolve without context
        _ => None,
    };
    if named.is_some() {
        return named;
    }

    // #rrggbb or #rgb
    if v.starts_with('#') {
        let hex = &v[1..];
        return match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color::rgba(r, g, b, a))
            }
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(Color::rgb(r, g, b))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
                Some(Color::rgba(r, g, b, a))
            }
            _ => None,
        };
    }

    // rgb(r, g, b) or rgba(r, g, b, a)
    if v.starts_with("rgb") {
        let inner = v
            .trim_start_matches("rgba")
            .trim_start_matches("rgb")
            .trim_start_matches('(')
            .trim_end_matches(')');
        let parts = split_color_components(inner);
        if parts.len() >= 3 {
            let parse_channel = |s: &str| -> u8 {
                let s = s.trim();
                if s.ends_with('%') {
                    (s[..s.len() - 1].parse::<f32>().unwrap_or(0.0) / 100.0 * 255.0).round() as u8
                } else {
                    s.parse::<f32>().unwrap_or(0.0).round() as u8
                }
            };
            let r = parse_channel(&parts[0]);
            let g = parse_channel(&parts[1]);
            let b = parse_channel(&parts[2]);
            let a = if parts.len() >= 4 {
                parse_alpha(&parts[3])
            } else {
                255
            };
            return Some(Color::rgba(r, g, b, a));
        }
    }

    // hsl (simplified: convert hsl to rgb)
    if v.starts_with("hsl") {
        let inner = v
            .trim_start_matches("hsla")
            .trim_start_matches("hsl")
            .trim_start_matches('(')
            .trim_end_matches(')');
        let parts = split_color_components(inner);
        if parts.len() >= 3 {
            // The hue may carry an angle unit — `hsl(120deg …)`.
            let h = parse_hue_deg(&parts[0]) / 360.0;
            let s = parts[1]
                .trim()
                .trim_end_matches('%')
                .parse::<f32>()
                .unwrap_or(0.0)
                / 100.0;
            let l = parts[2]
                .trim()
                .trim_end_matches('%')
                .parse::<f32>()
                .unwrap_or(0.0)
                / 100.0;
            let a = if parts.len() >= 4 {
                parse_alpha(&parts[3])
            } else {
                255
            };
            let (r, g, b) = hsl_to_rgb(h, s, l);
            // ⛔ ROUND, do not truncate. `as u8` floors, so `hsl(120 50% 50%)`
            // came out `rgb(63, 191, 63)` where every browser says
            // `rgb(64, 191, 64)` — the channel is 63.75.
            return Some(Color::rgba(
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                a,
            ));
        }
    }

    // ── CSS Color 4 §7-§10 and Color 5 §3 — the spaces beyond sRGB ──
    if let Some(c) = parse_modern_color(v) {
        return Some(c);
    }

    None
}

/// Split on a delimiter that is not inside parentheses.
fn split_top_level(s: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            c if c == delim && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// One component of a modern colour function: a number, a percentage scaled
/// against `pct_ref`, or the keyword `none` (css-color-4 §4.4 — `none` behaves
/// as zero anywhere it is not being carried through an interpolation).
fn color_component(s: &str, pct_ref: f64) -> f64 {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return 0.0;
    }
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f64>().unwrap_or(0.0) / 100.0 * pct_ref;
    }
    s.parse::<f64>().unwrap_or(0.0)
}

/// The body of `name(...)`, if `v` is a call to `name`.
fn fn_body<'a>(v: &'a str, name: &str) -> Option<&'a str> {
    let rest = v.strip_prefix(name)?.strip_prefix('(')?;
    rest.strip_suffix(')')
}

/// `oklch()`, `oklab()`, `lab()`, `lch()`, `hwb()`, `color()` and
/// `color-mix()`. Everything else returns `None` and falls through.
fn parse_modern_color(v: &str) -> Option<Color> {
    use crate::css::color_spaces as cs;

    // `color-mix()` first: its arguments are themselves colours.
    if let Some(body) = fn_body(v, "color-mix") {
        return parse_color_mix(body);
    }

    if let Some(body) = fn_body(v, "color") {
        let parts = split_color_components(body);
        if parts.len() < 4 {
            return None;
        }
        let space = parts[0].to_ascii_lowercase();
        let c = [
            color_component(&parts[1], 1.0),
            color_component(&parts[2], 1.0),
            color_component(&parts[3], 1.0),
        ];
        let (r, g, b) = cs::predefined_space_to_bytes(&space, c)?;
        let a = if parts.len() >= 5 {
            parse_alpha(&parts[4])
        } else {
            255
        };
        return Some(Color::rgba(r, g, b, a));
    }

    // `oklch` before `oklab` is not needed — the names differ at the fourth
    // character — but `lab`/`lch` must not swallow `oklab`/`oklch`, so the
    // ok-prefixed forms are tested first.
    let polar = |body: &str, pct_l: f64, pct_c: f64| -> Option<(f64, f64, f64, u8)> {
        let parts = split_color_components(body);
        if parts.len() < 3 {
            return None;
        }
        let l = color_component(&parts[0], pct_l);
        let c = color_component(&parts[1], pct_c);
        let h = parse_hue_deg(&parts[2]) as f64;
        let a = if parts.len() >= 4 {
            parse_alpha(&parts[3])
        } else {
            255
        };
        Some((l, c, h, a))
    };
    let rect = |body: &str, pct_l: f64, pct_ab: f64| -> Option<(f64, f64, f64, u8)> {
        let parts = split_color_components(body);
        if parts.len() < 3 {
            return None;
        }
        let l = color_component(&parts[0], pct_l);
        let x = color_component(&parts[1], pct_ab);
        let y = color_component(&parts[2], pct_ab);
        let a = if parts.len() >= 4 {
            parse_alpha(&parts[3])
        } else {
            255
        };
        Some((l, x, y, a))
    };

    if let Some(body) = fn_body(v, "oklch") {
        let (l, c, h, alpha) = polar(body, 1.0, 0.4)?;
        let (a, b) = cs::polar_to_rect(c, h);
        let (r, g, bl) = cs::oklab_to_bytes(l, a, b);
        return Some(Color::rgba(r, g, bl, alpha));
    }
    if let Some(body) = fn_body(v, "oklab") {
        let (l, a, b, alpha) = rect(body, 1.0, 0.4)?;
        let (r, g, bl) = cs::oklab_to_bytes(l, a, b);
        return Some(Color::rgba(r, g, bl, alpha));
    }
    if let Some(body) = fn_body(v, "lch") {
        let (l, c, h, alpha) = polar(body, 100.0, 150.0)?;
        let (a, b) = cs::polar_to_rect(c, h);
        let (r, g, bl) = cs::lab_to_bytes(l, a, b);
        return Some(Color::rgba(r, g, bl, alpha));
    }
    if let Some(body) = fn_body(v, "lab") {
        let (l, a, b, alpha) = rect(body, 100.0, 125.0)?;
        let (r, g, bl) = cs::lab_to_bytes(l, a, b);
        return Some(Color::rgba(r, g, bl, alpha));
    }
    if let Some(body) = fn_body(v, "hwb") {
        // css-color-4 §7.2: whiteness and blackness that sum past 100% give a
        // grey at their ratio; otherwise the pure hue is scaled between them.
        let parts = split_color_components(body);
        if parts.len() < 3 {
            return None;
        }
        let h = parse_hue_deg(&parts[0]) / 360.0;
        let w = color_component(&parts[1], 100.0) / 100.0;
        let bk = color_component(&parts[2], 100.0) / 100.0;
        let alpha = if parts.len() >= 4 {
            parse_alpha(&parts[3])
        } else {
            255
        };
        let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        if w + bk >= 1.0 {
            let grey = to_byte(w as f32 / (w + bk) as f32);
            return Some(Color::rgba(grey, grey, grey, alpha));
        }
        let (r, g, b) = hsl_to_rgb(h, 1.0, 0.5);
        let scale = |c: f32| c * (1.0 - w as f32 - bk as f32) + w as f32;
        return Some(Color::rgba(
            to_byte(scale(r)),
            to_byte(scale(g)),
            to_byte(scale(b)),
            alpha,
        ));
    }

    None
}

/// `color-mix(in <space>, <color> <pct>?, <color> <pct>?)` — css-color-5 §3.
fn parse_color_mix(body: &str) -> Option<Color> {
    use crate::css::color_spaces as cs;
    let args = split_top_level(body, ',');
    if args.len() < 3 {
        return None;
    }
    let space = args[0]
        .trim()
        .strip_prefix("in ")?
        .trim()
        .split_whitespace()
        .next()?
        .to_ascii_lowercase();

    // `<color> <percentage>` in either order.
    let split_arg = |arg: &str| -> Option<(Color, Option<f64>)> {
        let arg = arg.trim();
        let toks = split_top_level(arg, ' ');
        let toks: Vec<&str> = toks
            .iter()
            .map(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .collect();
        let mut colour = None;
        let mut pct = None;
        for t in toks {
            if let Some(p) = t.strip_suffix('%') {
                pct = p.parse::<f64>().ok();
            } else if colour.is_none() {
                colour = parse_color(t);
            }
        }
        Some((colour?, pct))
    };
    let (c1, p1) = split_arg(&args[1])?;
    let (c2, p2) = split_arg(&args[2])?;

    // Omitted percentages split the remainder evenly; given ones are
    // normalised so the pair sums to 100%.
    let (w1, w2) = match (p1, p2) {
        (None, None) => (0.5, 0.5),
        (Some(a), None) => (a / 100.0, 1.0 - a / 100.0),
        (None, Some(b)) => (1.0 - b / 100.0, b / 100.0),
        (Some(a), Some(b)) => {
            let sum = a + b;
            if sum <= 0.0 {
                return None;
            }
            (a / sum, b / sum)
        }
    };

    let alpha = ((c1.a as f64 * w1 + c2.a as f64 * w2)
        .round()
        .clamp(0.0, 255.0)) as u8;
    let lerp = |a: f64, b: f64| a * w1 + b * w2;
    let (r, g, b) = match space.as_str() {
        // `srgb` names the GAMMA-ENCODED space, so its midpoint is the
        // midpoint of the bytes. `srgb-linear` is the linear-light one.
        "srgb" => (
            lerp(c1.r as f64, c2.r as f64).round() as u8,
            lerp(c1.g as f64, c2.g as f64).round() as u8,
            lerp(c1.b as f64, c2.b as f64).round() as u8,
        ),
        "srgb-linear" => {
            let l1 = cs::bytes_to_linear_srgb(c1.r, c1.g, c1.b);
            let l2 = cs::bytes_to_linear_srgb(c2.r, c2.g, c2.b);
            let m = [lerp(l1[0], l2[0]), lerp(l1[1], l2[1]), lerp(l1[2], l2[2])];
            cs::predefined_space_to_bytes("srgb-linear", m)?
        }
        "oklab" | "oklch" => {
            let o1 = cs::linear_srgb_to_oklab(cs::bytes_to_linear_srgb(c1.r, c1.g, c1.b));
            let o2 = cs::linear_srgb_to_oklab(cs::bytes_to_linear_srgb(c2.r, c2.g, c2.b));
            cs::oklab_to_bytes(lerp(o1[0], o2[0]), lerp(o1[1], o2[1]), lerp(o1[2], o2[2]))
        }
        _ => return None,
    };
    Some(Color::rgba(r, g, b, alpha))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    (r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

/// Split the inside of `rgb()`/`hsl()` into components.
///
/// ⛔ Handles BOTH the legacy comma form and the modern space-separated one
/// with a `/` before the alpha — CSS Color 4 §4. Only the comma form was
/// understood, so `rgb(1 2 3)` and `hsl(120 50% 50%)` — what every current
/// design system emits — produced ONE component, failed the `len() >= 3`
/// check, and silently became black.
fn split_color_components(inner: &str) -> Vec<String> {
    let inner = inner.trim();
    if inner.contains(',') {
        return inner.split(',').map(|s| s.trim().to_string()).collect();
    }
    // Modern: `R G B` or `R G B / A`.
    let (rgb_part, alpha_part) = match inner.split_once('/') {
        Some((a, b)) => (a, Some(b)),
        None => (inner, None),
    };
    let mut out: Vec<String> = rgb_part.split_whitespace().map(|s| s.to_string()).collect();
    if let Some(a) = alpha_part {
        let a = a.trim();
        if !a.is_empty() {
            out.push(a.to_string());
        }
    }
    out
}

/// An alpha component: a number, or a percentage.
fn parse_alpha(s: &str) -> u8 {
    let s = s.trim();
    let v = if let Some(p) = s.strip_suffix('%') {
        p.parse::<f32>().unwrap_or(100.0) / 100.0
    } else {
        s.parse::<f32>().unwrap_or(1.0)
    };
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// A hue in degrees, accepting the angle units CSS Values 4 §7.1 defines.
fn parse_hue_deg(s: &str) -> f32 {
    let s = s.trim().to_ascii_lowercase();
    if let Some(v) = s.strip_suffix("turn") {
        return v.parse::<f32>().unwrap_or(0.0) * 360.0;
    }
    if let Some(v) = s.strip_suffix("grad") {
        return v.parse::<f32>().unwrap_or(0.0) * 0.9;
    }
    if let Some(v) = s.strip_suffix("rad") {
        return v.parse::<f32>().unwrap_or(0.0) * 180.0 / std::f32::consts::PI;
    }
    if let Some(v) = s.strip_suffix("deg") {
        return v.parse::<f32>().unwrap_or(0.0);
    }
    s.parse::<f32>().unwrap_or(0.0)
}
