//! The `calc()` expression parser — a self-contained sub-parser that was
//! sitting inside the length parser.

#![allow(unused_imports)]
use super::*;
use crate::types::*;
use std::collections::{HashMap, HashSet};

const ZERO_COEFFS: Coeffs = [0.0; 6];

// ── Recursive descent calc() evaluator ───────────────────────────────────────
// Coefficients: [percent, px, em, rem, vw, vh]
type Coeffs = [f32; 6];

/// Parse the inside of `calc(...)` using recursive descent.
///
/// Handles arbitrary nesting, mixed units, and correct operator precedence:
///   calc(100% - 21.5rem + (100vw - 1569px) / 2)
///
/// The result is a linear combination of unit coefficients [pct, px, em, rem, vw, vh].
/// At layout time, each coefficient is multiplied by its resolved unit value.
pub(crate) fn parse_calc(expr: &str) -> CssLength {
    let expr = expr.trim();
    // If the expression contains min()/max()/clamp(), use tree-based parser
    // since these can't be represented as linear coefficients.
    // `vmin`/`vmax` join min()/max()/clamp() on the tree path: the coefficient
    // form has no slot that means "the smaller axis", and projecting them onto
    // the `vw` slot answers the wrong axis on every landscape viewport
    // (css-values-4 §6.1.2). The tree keeps each term as a `CssLength`, which
    // knows its own axis at resolve time.
    let has_vmin_vmax = {
        let l = expr.to_ascii_lowercase();
        l.contains("vmin") || l.contains("vmax")
    };
    if has_vmin_vmax
        || expr.contains("env(")
        || expr.contains("min(")
        || expr.contains("max(")
        || expr.contains("clamp(")
    {
        let node = parse_calc_tree(expr);
        return CssLength::CalcExpr(Box::new(node));
    }
    let bytes = expr.as_bytes();
    let mut pos = 0usize;
    let coeffs = calc_parse_additive(bytes, &mut pos);
    let vals = coeffs;
    // A NaN coefficient means some term used a unit we cannot parse, so the
    // expression is invalid. `Auto` is the fallback every unparseable length
    // answers with, and `parse_length_checked` turns it back into a rejection.
    if vals.iter().any(|v| v.is_nan()) {
        return CssLength::Auto;
    }
    // Simplify: if only one unit is non-zero, return a simple CssLength variant.
    let n_nonzero = vals.iter().filter(|&&v| v != 0.0).count();
    if n_nonzero <= 1 {
        if vals[0] != 0.0 {
            return CssLength::Percent(vals[0]);
        }
        if vals[2] != 0.0 {
            return CssLength::Em(vals[2]);
        }
        if vals[3] != 0.0 {
            return CssLength::Rem(vals[3]);
        }
        if vals[4] != 0.0 {
            return CssLength::Vw(vals[4]);
        }
        if vals[5] != 0.0 {
            return CssLength::Vh(vals[5]);
        }
        return CssLength::Px(vals[1]);
    }
    CssLength::Calc(Box::new(vals))
}

/// Parse calc() expression into a CalcNode tree (handles nested min/max/clamp).
fn parse_calc_tree(expr: &str) -> CalcNode {
    use crate::types::CalcNode;
    let expr = expr.trim();

    // Split on top-level `+` and `-` (with spaces, per CSS spec)
    let parts = split_calc_additive(expr);
    if parts.len() == 1 {
        return parse_calc_tree_multiplicative(parts[0].1);
    }

    let mut result = parse_calc_tree_multiplicative(parts[0].1);
    for &(sign, term) in &parts[1..] {
        let rhs = parse_calc_tree_multiplicative(term);
        result = if sign == '+' {
            CalcNode::Add(Box::new(result), Box::new(rhs))
        } else {
            CalcNode::Sub(Box::new(result), Box::new(rhs))
        };
    }
    result
}

fn parse_calc_tree_multiplicative(expr: &str) -> CalcNode {
    use crate::types::CalcNode;
    let expr = expr.trim();
    // Simple: check for * or / not inside parens
    let mut depth = 0i32;
    let mut last_op = 0usize;
    let mut op_char = 0u8;
    let bytes = expr.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'*' | b'/' if depth == 0 && i > 0 => {
                last_op = i;
                op_char = b;
            }
            _ => {}
        }
    }
    if op_char != 0 && last_op > 0 {
        let lhs_str = expr[..last_op].trim();
        let rhs_str = expr[last_op + 1..].trim();
        if let Some(scalar) = parse_calc_scalar(rhs_str) {
            let lhs = parse_calc_tree_multiplicative(lhs_str);
            return if op_char == b'*' {
                CalcNode::Mul(Box::new(lhs), scalar)
            } else {
                CalcNode::Div(Box::new(lhs), scalar)
            };
        }
        // css-values-4 §10.6: multiplication commutes, so the scalar may be on
        // EITHER side — `calc(2 * min(50%, 300px))` is the same product as
        // `calc(min(50%, 300px) * 2)`. Division does not: its right operand
        // must be the number.
        if op_char == b'*' {
            if let Some(scalar) = parse_calc_scalar(lhs_str) {
                return CalcNode::Mul(Box::new(parse_calc_tree_multiplicative(rhs_str)), scalar);
            }
        }
    }
    parse_calc_tree_atom(expr)
}

fn parse_calc_scalar(expr: &str) -> Option<f32> {
    let expr = expr.trim();
    if !expr.bytes().all(|b| {
        b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-' | b'*' | b'/' | b'(' | b')')
            || css_calc_ws(b)
    }) {
        return None;
    }
    let mut pos = 0;
    let values = calc_parse_additive(expr.as_bytes(), &mut pos);
    calc_skip_ws(expr.as_bytes(), &mut pos);
    (pos == expr.len() && values[1].is_finite() && values.iter().enumerate().all(|(i, v)| i == 1 || *v == 0.0))
        .then_some(values[1])
}

fn parse_calc_tree_atom(expr: &str) -> CalcNode {
    use crate::types::CalcNode;
    let expr = expr.trim();
    // Parenthesized
    if expr.starts_with('(') && expr.ends_with(')') {
        return parse_calc_tree(&expr[1..expr.len() - 1]);
    }
    // min/max/clamp — delegate to parse_length which handles these
    if expr.starts_with("min(") || expr.starts_with("max(") || expr.starts_with("clamp(") {
        return CalcNode::Value(parse_length(expr));
    }
    // Simple value
    CalcNode::Value(parse_length(expr))
}

/// Split a calc expression at top-level `+` and `-` operators (CSS requires spaces around them).
fn split_calc_additive(expr: &str) -> Vec<(char, &str)> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let bytes = expr.as_bytes();
    let mut start = 0usize;
    let mut sign = '+';
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'+' | b'-' if depth == 0 && i > start && i + 1 < bytes.len() => {
                // CSS calc additive operators require whitespace on both sides,
                // but CSS whitespace includes newlines, tabs, CR and FF. The old
                // tree parser only accepted a literal space, so multiline nested
                // calc()/min()/max()/clamp() expressions silently became invalid.
                if i > 0 && css_calc_ws(bytes[i - 1]) && css_calc_ws(bytes[i + 1]) {
                    let term = expr[start..i].trim();
                    if !term.is_empty() {
                        parts.push((sign, term));
                    }
                    sign = bytes[i] as char;
                    i += 1;
                    while i < bytes.len() && css_calc_ws(bytes[i]) {
                        i += 1;
                    }
                    start = i;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let tail = expr[start..].trim();
    if !tail.is_empty() {
        parts.push((sign, tail));
    }
    if parts.is_empty() {
        parts.push(('+', expr));
    }
    parts
}

fn coeffs_add(a: &Coeffs, b: &Coeffs) -> Coeffs {
    [
        a[0] + b[0],
        a[1] + b[1],
        a[2] + b[2],
        a[3] + b[3],
        a[4] + b[4],
        a[5] + b[5],
    ]
}

fn coeffs_sub(a: &Coeffs, b: &Coeffs) -> Coeffs {
    [
        a[0] - b[0],
        a[1] - b[1],
        a[2] - b[2],
        a[3] - b[3],
        a[4] - b[4],
        a[5] - b[5],
    ]
}

fn coeffs_mul(a: &Coeffs, f: f32) -> Coeffs {
    [a[0] * f, a[1] * f, a[2] * f, a[3] * f, a[4] * f, a[5] * f]
}

#[inline]
fn css_calc_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn calc_skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && css_calc_ws(b[*pos]) {
        *pos += 1;
    }
}

/// Additive level: handles `+` and `-` (lowest precedence).
fn calc_parse_additive(b: &[u8], pos: &mut usize) -> Coeffs {
    let mut result = calc_parse_multiplicative(b, pos);
    loop {
        calc_skip_ws(b, pos);
        if *pos >= b.len() {
            break;
        }
        // CSS calc requires spaces around + and - operators.
        // Check for ` + ` or ` - ` pattern (we already consumed leading ws).
        let op = b[*pos];
        if (op == b'+' || op == b'-') && *pos + 1 < b.len() && css_calc_ws(b[*pos + 1]) {
            // Make sure the previous char was a space (we consumed it in skip_ws)
            *pos += 1; // skip operator
            calc_skip_ws(b, pos);
            let rhs = calc_parse_multiplicative(b, pos);
            result = if op == b'+' {
                coeffs_add(&result, &rhs)
            } else {
                coeffs_sub(&result, &rhs)
            };
        } else {
            break;
        }
    }
    result
}

/// Multiplicative level: handles `*` and `/` (higher precedence).
fn calc_parse_multiplicative(b: &[u8], pos: &mut usize) -> Coeffs {
    let mut result = calc_parse_atom(b, pos);
    loop {
        calc_skip_ws(b, pos);
        if *pos >= b.len() {
            break;
        }
        let op = b[*pos];
        if op == b'*' || op == b'/' {
            *pos += 1;
            calc_skip_ws(b, pos);
            if op == b'*' {
                // One side must be a plain number. Try: coeffs * number or number * coeffs.
                // We already have lhs as coeffs, so rhs should be a number.
                let rhs = calc_parse_atom(b, pos);
                // If rhs is purely px (unitless number parsed as px), use as scalar.
                // If lhs is purely px, treat lhs as scalar and rhs as unit-bearing.
                let rhs_scalar = if rhs[0] == 0.0
                    && rhs[2] == 0.0
                    && rhs[3] == 0.0
                    && rhs[4] == 0.0
                    && rhs[5] == 0.0
                {
                    Some(rhs[1])
                } else {
                    None
                };
                let lhs_scalar = if result[0] == 0.0
                    && result[2] == 0.0
                    && result[3] == 0.0
                    && result[4] == 0.0
                    && result[5] == 0.0
                {
                    Some(result[1])
                } else {
                    None
                };
                if let Some(s) = rhs_scalar {
                    result = coeffs_mul(&result, s);
                } else if let Some(s) = lhs_scalar {
                    result = coeffs_mul(&rhs, s);
                } else {
                    // Both have units — invalid in CSS, just keep lhs
                }
            } else {
                // Division: coeffs / number
                let rhs = calc_parse_atom(b, pos);
                let divisor = rhs[1]; // should be a unitless number (px slot)
                if divisor != 0.0 {
                    result = coeffs_mul(&result, 1.0 / divisor);
                }
            }
        } else {
            break;
        }
    }
    result
}

/// Atom level: parenthesized sub-expression or a single value with units.
fn calc_parse_atom(b: &[u8], pos: &mut usize) -> Coeffs {
    calc_skip_ws(b, pos);
    if *pos >= b.len() {
        return ZERO_COEFFS;
    }

    // Nested calc() — strip the "calc(" prefix and parse inner expression
    if *pos + 5 <= b.len() && &b[*pos..*pos + 5] == b"calc(" {
        *pos += 5; // skip "calc("
        let result = calc_parse_additive(b, pos);
        calc_skip_ws(b, pos);
        if *pos < b.len() && b[*pos] == b')' {
            *pos += 1;
        }
        return result;
    }

    // Parenthesized sub-expression
    if b[*pos] == b'(' {
        *pos += 1; // skip '('
        let result = calc_parse_additive(b, pos);
        calc_skip_ws(b, pos);
        if *pos < b.len() && b[*pos] == b')' {
            *pos += 1;
        }
        return result;
    }

    // Parse a number (possibly negative) followed by optional unit
    let start = *pos;
    // Allow leading sign
    if *pos < b.len() && (b[*pos] == b'-' || b[*pos] == b'+') {
        *pos += 1;
    }
    // Allow leading dot like ".875rem"
    let mut has_digit = false;
    while *pos < b.len() && b[*pos].is_ascii_digit() {
        *pos += 1;
        has_digit = true;
    }
    if *pos < b.len() && b[*pos] == b'.' {
        *pos += 1;
    }
    while *pos < b.len() && b[*pos].is_ascii_digit() {
        *pos += 1;
        has_digit = true;
    }
    if !has_digit {
        return ZERO_COEFFS;
    }

    let num_end = *pos;
    let num_str = std::str::from_utf8(&b[start..num_end]).unwrap_or("0");
    let num: f32 = num_str.parse().unwrap_or(0.0);

    // Parse unit suffix
    let unit_start = *pos;
    while *pos < b.len() && b[*pos].is_ascii_alphabetic() {
        *pos += 1;
    }
    // Also allow '%'
    if *pos < b.len() && b[*pos] == b'%' {
        *pos += 1;
    }
    let unit = std::str::from_utf8(&b[unit_start..*pos]).unwrap_or("");

    // ⛔ ONE unit table, not two. This had its own — `%`, `px`, `em`, `rem`,
    // `vw`, `vh`, `pt`, plus `vmin`/`vmax` mapped to the `vw` slot and
    // commented "approximate", and a catch-all `_ => px`. That catch-all is
    // the dangerous part: `calc(1in + 2px)` silently answered **3px** instead
    // of 98px, because `in` was unknown and taken as pixels. Every unit added
    // to `parse_length` would have had to be added here too, and any that was
    // not became a silent wrong answer rather than a parse failure.
    //
    // `parse_length` is the single definition; this projects its result onto
    // the coefficient slots.
    let mut c = ZERO_COEFFS;
    if unit == "%" {
        c[0] = num;
        return c;
    }
    match crate::css::value_parse::parse_length(&format!("{num}{unit}")) {
        CssLength::Px(v) => c[1] = v,
        CssLength::Em(v) => c[2] = v,
        CssLength::Rem(v) => c[3] = v,
        CssLength::Vw(v) => c[4] = v,
        CssLength::Vh(v) => c[5] = v,
        CssLength::Percent(v) => c[0] = v,
        // ⛔ The coefficient form has no vmin/vmax slot, so it cannot carry
        // them. Answering with the WRONG AXIS (what the old table did) is
        // worse than dropping the term, but both are wrong — `calc()` mixing
        // vmin/vmax needs the tree path, which is the next step here.
        CssLength::Zero => {}
        // ⛔ NOT pixels. A term in a unit `parse_length` does not recognise
        // makes the whole `calc()` invalid (css-values-4 §5.1), and an invalid
        // declaration is DROPPED — the previous cascade winner stands. Folding
        // the bare number into the px slot answered `calc(1zz + 2px)` = 3px.
        // NaN is the carrier: it survives every add, subtract and multiply
        // below, so `parse_calc` sees it however deep the term sat.
        _ => c[1] = f32::NAN,
    }
    c
}
