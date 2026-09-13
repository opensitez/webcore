//! The number and selection algorithms HTML defines for form controls.
//!
//! These are the SHARED definitions the paint path, the click path and the DOM
//! accessors all have to agree on. Each was previously open-coded at whichever
//! site needed it, and the copies disagreed: the renderer decided "list box"
//! from `multiple || size > 1`, which is close to the spec's *display size* but
//! is not it, and nothing anywhere applied a range control's step.
//!
//! Everything here is a pure function over a node, so a caller can ask the same
//! question the spec asks and get the spec's answer.

use crate::types::WebCore;

// ── §2.3.4 Numbers ──────────────────────────────────────────────────────────

/// The **rules for parsing integers** (HTML §2.3.4.1).
///
/// Leading whitespace and a leading sign are allowed, digits are required, and
/// anything after the digit run is IGNORED — `"4 rows"` parses as 4. That
/// leniency is the whole reason this is not `str::parse`.
pub fn parse_integer(input: &str) -> Option<i64> {
    let s = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (negative, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    Some(if negative { -value } else { value })
}

/// The **rules for parsing non-negative integers** (HTML §2.3.4.2): parse as an
/// integer, then reject a negative result. `-1` is an ERROR, not a clamp.
pub fn parse_non_negative_integer(input: &str) -> Option<u32> {
    match parse_integer(input) {
        Some(v) if v >= 0 => u32::try_from(v).ok(),
        _ => None,
    }
}

/// The **rules for parsing floating-point number values** (HTML §2.3.4.3).
///
/// Deliberately NOT `str::parse::<f64>()`, which differs in three ways that all
/// matter here: it rejects the trailing junk this algorithm ignores, it accepts
/// `inf`/`NaN`, which the spec says are never valid floating-point numbers, and
/// it has no notion of the leading whitespace this skips.
pub fn parse_floating_point(input: &str) -> Option<f64> {
    let s = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let mut end = 0usize;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let had_int = i > int_start;
    if had_int {
        end = i;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        // A lone `.` with no digits after it ends the number; `1.` keeps the 1.
        if j > frac_start {
            end = j;
            i = j;
        }
    }
    if end == 0 {
        return None;
    }
    // An exponent counts only if it is COMPLETE. `1e` is the number 1 with a
    // stray `e` after it, which the trailing-junk rule then discards.
    if i < bytes.len() && (bytes[i] | 0x20) == b'e' {
        let mut j = i + 1;
        if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b'+') {
            j += 1;
        }
        let exp_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            end = j;
        }
    }
    let n: f64 = s[..end].parse().ok()?;
    if n.is_finite() { Some(n) } else { None }
}

/// A **valid floating-point number** (HTML §2.3.4.3) — the AUTHORING grammar,
/// which is stricter than the parsing rules above: no leading whitespace, no
/// leading `+`, no trailing junk.
///
/// The two are not interchangeable. Range's value sanitization is defined over
/// *valid*, so `" 50"` and `"++50"` are both sanitized away, while `min=" 50"`
/// is read by the lenient parser and means 50.
pub fn is_valid_floating_point(input: &str) -> bool {
    let s = input.strip_prefix('-').unwrap_or(input);
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    // "One or both of the following, in the given order": digits, then `.`digits.
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => {
            !frac.is_empty()
                && frac.bytes().all(|b| b.is_ascii_digit())
                && (int.is_empty() || int.bytes().all(|b| b.is_ascii_digit()))
        }
        None => !mantissa.is_empty() && mantissa.bytes().all(|b| b.is_ascii_digit()),
    };
    if !mantissa_ok {
        return false;
    }
    match exponent {
        None => true,
        Some(e) => {
            let digits = e.strip_prefix(['-', '+']).unwrap_or(e);
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        }
    }
}

/// The **best representation** of a number as a floating-point number
/// (HTML §2.3.4.3) — "the string obtained from running ToString(n)", which is
/// ECMAScript's Number::toString, not Rust's `Display`.
///
/// They agree on the digits and disagree on when to use an exponent: Rust
/// writes `1e21` as `1000000000000000000000`, ECMAScript as `1e+21`. A range
/// control rarely reaches either end, but a `value` written here is read back
/// through the DOM as a string, and a string that differs from every browser's
/// is a difference a program can see.
pub fn best_representation(n: f64) -> String {
    if n == 0.0 {
        // Covers -0.0, which ToString renders as "0".
        return "0".into();
    }
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let sign = if n < 0.0 { "-" } else { "" };
    // `{:e}` gives the SHORTEST round-tripping digits with an exponent, which
    // is exactly ECMAScript's (s, k, n) triple in a different spelling.
    let sci = format!("{:e}", n.abs());
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("`{:e}` always emits an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let exp: i32 = exp.parse().expect("`{:e}` always emits a decimal exponent");
    // ECMAScript names the decimal point's position `n`; `{:e}` reports the
    // exponent of the leading digit, one less than that.
    let point = exp + 1;

    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!(
            "{}.{}",
            &digits[..point as usize],
            &digits[point as usize..]
        )
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let e_sign = if point - 1 >= 0 { "+" } else { "-" };
        let e_abs = (point - 1).abs();
        if k == 1 {
            format!("{digits}e{e_sign}{e_abs}")
        } else {
            format!("{}.{}e{e_sign}{e_abs}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

// ── §4.10.7 The select element ──────────────────────────────────────────────

/// The **display size** of a `<select>` (HTML §4.10.7).
///
/// ⛔ The default is **4 when `multiple` is present**, not 1 — so `<select
/// multiple>` is already a four-row list box with no `size` attribute on it,
/// and the `size="4"` our own control markup writes alongside `multiple` is
/// redundant rather than load-bearing.
pub fn display_size(select: &WebCore) -> u32 {
    select
        .attributes
        .get("size")
        .and_then(|v| parse_non_negative_integer(v))
        .unwrap_or_else(|| if is_multiple(select) { 4 } else { 1 })
}

/// Whether the element bears the `multiple` content attribute.
pub fn is_multiple(select: &WebCore) -> bool {
    select.attributes.contains_key("multiple")
}

/// Whether a `<select>` renders as a **list box** rather than a drop-down box
/// (HTML §15.5.16).
///
/// Display size alone decides it, in both directions: `<select multiple>` is a
/// list box because its display size defaults to 4, and `<select multiple
/// size=1>` is NOT one — the spec allows a multi-select drop-down there, and
/// falls back to a list box only where the platform has no such control.
///
/// ⛔ `size="0"` parses successfully to 0, so it is a DROP-DOWN. The predicate
/// this replaced read `size > 1` on the raw attribute and agreed by accident.
pub fn is_list_box(select: &WebCore) -> bool {
    display_size(select) > 1
}

/// **Get the list of options** given a `<select>` (HTML §4.10.7).
///
/// Options are counted THROUGH an `<optgroup>`, not around it, which is what
/// makes `selectedIndex` a flat index. The walk descends into anything that is
/// not itself an option boundary, so options wrapped in some other element are
/// still found; it stops at a nested `<optgroup>`, whose contents the spec
/// excludes.
pub fn list_of_options(select: &WebCore) -> Vec<&WebCore> {
    fn walk<'a>(node: &'a WebCore, in_optgroup: bool, out: &mut Vec<&'a WebCore>) {
        for child in &node.children {
            match child.tag.as_str() {
                "option" => out.push(child),
                // Boundaries: reached, but not descended into.
                "select" | "hr" | "datalist" => {}
                "optgroup" => {
                    if !in_optgroup {
                        walk(child, true, out);
                    }
                }
                _ => walk(child, in_optgroup, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(select, false, &mut out);
    out
}

/// The same walk, yielding `node_id`s — what a caller that needs to MUTATE the
/// options wants, since it cannot hold borrows across the write.
pub fn option_ids(select: &WebCore) -> Vec<u32> {
    list_of_options(select).iter().map(|o| o.node_id).collect()
}

/// The same walk, mutable. The callback also receives whether the option's
/// `<optgroup>` is disabled, which is half of what makes an option disabled.
pub fn for_each_option_mut(select: &mut WebCore, f: &mut dyn FnMut(&mut WebCore, bool)) {
    fn walk(
        node: &mut WebCore,
        in_optgroup: bool,
        group_disabled: bool,
        f: &mut dyn FnMut(&mut WebCore, bool),
    ) {
        for child in &mut node.children {
            match child.tag.as_str() {
                "option" => f(child, group_disabled),
                "select" | "hr" | "datalist" => {}
                "optgroup" => {
                    if !in_optgroup {
                        let disabled = child.attributes.contains_key("disabled");
                        walk(child, true, disabled, f);
                    }
                }
                _ => walk(child, in_optgroup, group_disabled, f),
            }
        }
    }
    walk(select, false, false, f);
}

/// Whether an `<option>` is **disabled** (HTML §4.10.10): "if its `disabled`
/// attribute is present or if it is a child of an `optgroup` element whose
/// `disabled` attribute is present".
pub fn option_is_disabled(option: &WebCore, group_disabled: bool) -> bool {
    group_disabled || option.attributes.contains_key("disabled")
}

/// The **selectedness setting algorithm** (HTML §4.10.7).
///
/// ⛔ Its first step — auto-select the first enabled option — is guarded on
/// **display size 1**. A list box is therefore meant to sit with NOTHING
/// selected until someone picks a row, and `selectedIndex` reports −1 until
/// then. Reading that guard as "always select the first option" is what made a
/// fresh list box paint a highlighted first row it had no business having.
pub fn run_selectedness_setting_algorithm(select: &mut WebCore) {
    let multiple = is_multiple(select);
    let drop_down = display_size(select) == 1;

    if !multiple && drop_down {
        let mut any_selected = false;
        for_each_option_mut(select, &mut |o, _| {
            if o.selectedness {
                any_selected = true;
            }
        });
        if !any_selected {
            let mut chosen = false;
            for_each_option_mut(select, &mut |o, group_disabled| {
                if !chosen && !option_is_disabled(o, group_disabled) {
                    o.selectedness = true;
                    chosen = true;
                }
            });
            return;
        }
    }

    if !multiple {
        // "Set the selectedness of all but the LAST option element with its
        // selectedness set to true ... to false." Two passes, because the last
        // one is only known once the walk has finished.
        let mut total = 0usize;
        for_each_option_mut(select, &mut |o, _| {
            if o.selectedness {
                total += 1;
            }
        });
        if total >= 2 {
            let mut seen = 0usize;
            for_each_option_mut(select, &mut |o, _| {
                if o.selectedness {
                    seen += 1;
                    if seen < total {
                        o.selectedness = false;
                    }
                }
            });
        }
    }
}

/// The `<select>` half of the **reset algorithm** (HTML §4.10.23): selectedness
/// goes back to the `selected` content attribute, every dirtiness is cleared,
/// and the selectedness setting algorithm runs.
pub fn reset_select(select: &mut WebCore) {
    for_each_option_mut(select, &mut |o, _| {
        o.selectedness = o.attributes.contains_key("selected");
        o.dirty_selectedness = false;
    });
    run_selectedness_setting_algorithm(select);
    refresh_select_display_text(select);
}

/// Re-sync a drop-down's shown label to its selectedness.
///
/// A closed drop-down shows a child text node rather than its options, so any
/// path that moves the selection has to move the label with it — otherwise the
/// control paints the selection it USED to have. A list box paints its rows
/// from the options themselves and has no such node.
pub fn refresh_select_display_text(select: &mut WebCore) {
    if is_list_box(select) {
        return;
    }
    let text = list_of_options(select)
        .iter()
        .find(|o| o.selectedness)
        .map(|o| option_label(o))
        .unwrap_or_default();
    if let Some(tn) = select.children.iter_mut().rev().find(|c| c.tag == "#text") {
        tn.text = text;
    }
}

/// **Pick an option** (HTML §4.10.7), the algorithm a click on a single-select
/// control runs.
///
/// Returns whether anything changed, so a caller knows whether to send select
/// update notifications. ⛔ It declines on a `multiple` select by design — that
/// control TOGGLES rather than picks, which is [`toggle_option`].
pub fn pick_option(select: &mut WebCore, option_id: u32) -> bool {
    if is_multiple(select) || select.attributes.contains_key("disabled") {
        return false;
    }
    // A DISABLED option is not pickable, the same guard [`toggle_option`] has.
    // The spec's algorithm does not state it because it is only ever invoked
    // for an option the user could reach; reached from a raw click coordinate,
    // it has to be checked here or a disabled row selects like any other.
    let mut target_disabled = false;
    for_each_option_mut(select, &mut |o, group_disabled| {
        if o.node_id == option_id && option_is_disabled(o, group_disabled) {
            target_disabled = true;
        }
    });
    if target_disabled {
        return false;
    }
    let mut changed = false;
    for_each_option_mut(select, &mut |o, _| {
        if o.node_id == option_id {
            if !o.selectedness {
                changed = true;
            }
            o.selectedness = true;
            o.dirty_selectedness = true;
        } else if o.selectedness {
            // "Whenever an option element ... has its selectedness set to true
            // ... the user agent must set the selectedness of all the other
            // option elements ... to false."
            o.selectedness = false;
            changed = true;
        }
    });
    changed
}

/// Toggle one option's selectedness — what a `multiple` list box does on a
/// click (HTML §4.10.7): "the selectedness of the option element must be
/// changed (from true to false or false to true), the dirtiness of the element
/// must be set to true".
pub fn toggle_option(select: &mut WebCore, option_id: u32) -> bool {
    if select.attributes.contains_key("disabled") {
        return false;
    }
    let mut changed = false;
    for_each_option_mut(select, &mut |o, group_disabled| {
        if o.node_id == option_id && !option_is_disabled(o, group_disabled) {
            o.selectedness = !o.selectedness;
            o.dirty_selectedness = true;
            changed = true;
        }
    });
    changed
}

/// Unselect the selected option of a SINGLE-SELECT LIST BOX (HTML §4.10.7).
///
/// "If the `multiple` attribute is absent and the element's display size is
/// greater than 1, then the user agent should also allow the user to request
/// that the option whose selectedness is true, if any, be unselected." There is
/// no drop-down equivalent — a drop-down always has a selection.
pub fn unselect_option(select: &mut WebCore, option_id: u32) -> bool {
    if is_multiple(select) || !is_list_box(select) || select.attributes.contains_key("disabled") {
        return false;
    }
    let mut changed = false;
    for_each_option_mut(select, &mut |o, _| {
        if o.node_id == option_id && o.selectedness {
            o.selectedness = false;
            o.dirty_selectedness = true;
            changed = true;
        }
    });
    changed
}

/// The index of the first option with its selectedness set to true, or **−1**
/// when there is none — `select.selectedIndex` (HTML §4.10.7).
pub fn selected_index(select: &WebCore) -> i32 {
    list_of_options(select)
        .iter()
        .position(|o| o.selectedness)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// `select.value` — "the value of the first option element ... with its
/// selectedness set to true", or the empty string when there is none.
pub fn select_value(select: &WebCore) -> String {
    list_of_options(select)
        .iter()
        .find(|o| o.selectedness)
        .map(|o| option_value(o))
        .unwrap_or_default()
}

/// An `<option>`'s **label** (HTML §4.10.10): the `label` attribute when it has
/// one, otherwise the element's descendant text, stripped and collapsed.
pub fn option_label(option: &WebCore) -> String {
    if let Some(label) = option.attributes.get("label") {
        if !label.is_empty() {
            return label.clone();
        }
    }
    fn text(node: &WebCore, out: &mut String) {
        if node.tag == "#text" {
            out.push_str(&node.text);
        }
        for child in &node.children {
            text(child, out);
        }
    }
    let mut out = String::new();
    text(option, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An `<option>`'s **value** (HTML §4.10.10): the `value` attribute when
/// present, otherwise the label. Note this is the option's own descendant text
/// and NOT `option_label`'s `label`-attribute preference — the two accessors
/// differ on purpose in the spec.
pub fn option_value(option: &WebCore) -> String {
    if let Some(v) = option.attributes.get("value") {
        return v.clone();
    }
    fn text(node: &WebCore, out: &mut String) {
        if node.tag == "#text" {
            out.push_str(&node.text);
        }
        for child in &node.children {
            text(child, out);
        }
    }
    let mut out = String::new();
    text(option, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── §4.10.5.1.13 Range state ────────────────────────────────────────────────

/// The **minimum** of a range control — its `min`, or the state's default of 0.
pub fn range_minimum(input: &WebCore) -> f64 {
    input
        .attributes
        .get("min")
        .and_then(|v| parse_floating_point(v))
        .unwrap_or(0.0)
}

/// The **maximum** of a range control — its `max`, or the state's default of 100.
pub fn range_maximum(input: &WebCore) -> f64 {
    input
        .attributes
        .get("max")
        .and_then(|v| parse_floating_point(v))
        .unwrap_or(100.0)
}

/// The **allowed value step** (HTML §4.10.5.3.8). `None` means the control has
/// no step — which `step="any"` asks for explicitly.
///
/// Note what does NOT produce `None`: a `step` that fails to parse, or parses
/// to zero or a negative number, falls back to the DEFAULT step rather than
/// removing the constraint. Range's default step is 1, so `step="-3"` still
/// snaps to integers.
pub fn allowed_value_step(input: &WebCore) -> Option<f64> {
    const DEFAULT_STEP: f64 = 1.0;
    match input.attributes.get("step") {
        None => Some(DEFAULT_STEP),
        Some(v) if v.trim().eq_ignore_ascii_case("any") => None,
        Some(v) => match parse_floating_point(v) {
            Some(n) if n > 0.0 => Some(n),
            _ => Some(DEFAULT_STEP),
        },
    }
}

/// The **step base** (HTML §4.10.5.3.8): `min` if it parses, else `value` if it
/// parses, else zero.
///
/// ⛔ The `value` fallback is why stepping is not simply "a multiple of the
/// step". `<input type=range step=20 value=50>` with no `min` has a step base
/// of 50, so 50 is already conforming and nothing is rounded.
pub fn step_base(input: &WebCore) -> f64 {
    if let Some(n) = input
        .attributes
        .get("min")
        .and_then(|v| parse_floating_point(v))
    {
        return n;
    }
    if let Some(n) = input
        .attributes
        .get("value")
        .and_then(|v| parse_floating_point(v))
    {
        return n;
    }
    0.0
}

/// The range state's **default value** (HTML §4.10.5.1.13): "the minimum plus
/// half the difference between the minimum and the maximum, unless the maximum
/// is less than the minimum, in which case the default value is the minimum".
pub fn range_default_value(input: &WebCore) -> f64 {
    let min = range_minimum(input);
    let max = range_maximum(input);
    if max < min {
        min
    } else {
        min + (max - min) / 2.0
    }
}

/// Snap a number onto the control's allowed steps, honouring the bounds
/// (HTML §4.10.5.1.13, "suffering from a step mismatch").
///
/// ⛔ **Ties go to positive infinity** — the one detail that distinguishes this
/// from an ordinary round-to-nearest, and the one the spec gives a worked
/// example for: `min=0 max=100 step=20 value=50` has an initial value of 60,
/// not 40.
pub fn snap_to_step(input: &WebCore, value: f64) -> f64 {
    let step = match allowed_value_step(input) {
        Some(s) => s,
        None => return value,
    };
    let base = step_base(input);
    let min = range_minimum(input);
    let max = range_maximum(input);

    let exact = (value - base) / step;
    // The steps the bounds admit. A maximum below the minimum leaves the range
    // unbounded above, which is the only reading that keeps a step available at
    // all — the spec applies its `≤ maximum` clause only "if the maximum is not
    // less than the minimum".
    let k_min = ((min - base) / step).ceil();
    let k_max = if max < min {
        f64::INFINITY
    } else {
        ((max - base) / step).floor()
    };
    if k_min > k_max {
        // No conforming number exists, and the spec's rounding requirement is
        // conditioned on there being one. Leave the value alone.
        return value;
    }

    // `floor(x) + 1` rather than `ceil(x)`: for an x that is already an
    // integer, `ceil` returns x itself and the pair collapses, losing the
    // upper candidate a tie needs.
    let lower = exact.floor();
    let upper = lower + 1.0;
    let mut best: Option<f64> = None;
    for k in [lower, upper] {
        let k = k.clamp(k_min, k_max);
        let candidate = base + k * step;
        best = Some(match best {
            None => candidate,
            Some(b) => {
                let db = (b - value).abs();
                let dc = (candidate - value).abs();
                // `>=` is the tie-break: an equally distant candidate wins, and
                // `upper` is visited second, so the larger one survives.
                if dc < db || (dc == db && candidate > b) {
                    candidate
                } else {
                    b
                }
            }
        });
    }
    best.unwrap_or(value)
}

/// The full journey from a raw `value` string to the number a range control
/// actually holds: **value sanitization**, then the underflow, overflow and
/// step-mismatch corrections, in the order HTML §4.10.5.1.13 states them.
///
/// This is what `<input type=range>` shows before anyone touches it, and what
/// every interaction result has to pass back through.
pub fn sanitize_range_value(input: &WebCore, raw: &str) -> f64 {
    // Sanitization is defined over a VALID floating-point number, so the
    // lenient parser is not the right question to ask here: `"++50"` is not
    // valid and becomes the default, exactly as the spec's own example notes.
    let mut value = if is_valid_floating_point(raw) {
        parse_floating_point(raw).unwrap_or_else(|| range_default_value(input))
    } else {
        range_default_value(input)
    };
    let min = range_minimum(input);
    let max = range_maximum(input);
    if value < min {
        value = min;
    }
    if value > max && max >= min {
        value = max;
    }
    snap_to_step(input, value)
}

// ── User-agent metrics ──────────────────────────────────────────────────────
//
// ⛔ NOT spec. HTML says a list box shows "rows for items" and leaves their
// height to the user agent, exactly as it leaves a range control's thumb.
//
// They live here anyway, because the PAINT and the HIT TEST have to agree: a
// click that selects a row the renderer drew somewhere else is indistinguishable
// from a broken control, and two copies of a metric drift the first time one is
// touched. The range control needs no equivalent — `widgets::Slider` owns its
// geometry in both directions, so the click path drives the widget itself.

/// The inset from a list box's content edge to its first row.
pub const LIST_BOX_PADDING: f32 = 2.0;

/// The height of one list-box row at a given font size.
pub fn list_box_row_height(font_px: f32) -> f32 {
    font_px * 1.2
}

/// Which row of a list box a point falls on, or `None` for a point past the
/// last row — including the gap under a short list, where clicking selects
/// nothing rather than the nearest row.
///
/// `content_y`/`content_h` are the control's content box; rows below it are not
/// drawn (a list scrolls the rest), so a click cannot reach them either.
pub fn list_box_row_at(
    content_y: f32,
    content_h: f32,
    font_px: f32,
    option_count: usize,
    click_y: f32,
) -> Option<usize> {
    let row_h = list_box_row_height(font_px);
    if row_h <= 0.0 {
        return None;
    }
    let offset = click_y - (content_y + LIST_BOX_PADDING);
    if offset < 0.0 {
        return None;
    }
    let index = (offset / row_h).floor() as usize;
    if index >= option_count {
        return None;
    }
    // The same clip the painter applies: a row whose bottom passes the content
    // box was never drawn.
    let row_y = content_y + LIST_BOX_PADDING + index as f32 * row_h;
    if row_y + row_h > content_y + content_h - LIST_BOX_PADDING {
        return None;
    }
    Some(index)
}

/// Which of the four modes an `<input>`'s `value` IDL attribute is in
/// (HTML §4.10.5.4). They are not interchangeable, and treating them all as
/// [`ValueMode::Value`] is what made `checkbox.value = "true"` read back as
/// `"on"`: the write went to the value state and the read to the attribute.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueMode {
    /// The control HOLDS a value: state plus a dirty flag, sanitized. Text
    /// fields, `range`, `color`, the date and time family, `number`.
    Value,
    /// The `value` CONTENT ATTRIBUTE is the whole story — a button's label, a
    /// hidden field's payload. Setting it writes the attribute.
    Default,
    /// The content attribute, defaulting to `"on"` when absent: what a ticked
    /// checkbox or radio submits. Setting it writes the attribute — a
    /// checkbox's STATE is its checkedness, not its value.
    DefaultOn,
    /// `<input type=file>`. Read-only apart from the empty string.
    Filename,
}

/// The `value` mode of a form control.
pub fn value_mode(node: &WebCore) -> ValueMode {
    if node.tag != "input" {
        // `<textarea>`'s value IDL is its raw value, with the child text as the
        // default — the same shape as `Value`.
        return ValueMode::Value;
    }
    let ty = node
        .attributes
        .get("type")
        .map(|t| t.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "text".into());
    match ty.as_str() {
        "checkbox" | "radio" => ValueMode::DefaultOn,
        "hidden" | "submit" | "image" | "reset" | "button" => ValueMode::Default,
        "file" => ValueMode::Filename,
        _ => ValueMode::Value,
    }
}

/// Invoke the **value sanitization algorithm** for a control whose type defines
/// one (HTML §4.10.5).
///
/// This seeds the VALUE and deliberately leaves the dirty value flag down:
/// sanitization is not a user edit, so the `value` content attribute is still
/// the default a reset returns to. That separation is what lets
/// `<input type=range min=0 max=100 step=20 value=50>` show 60 while
/// `getAttribute("value")` keeps answering `"50"`.
///
/// Run at parse time and again on reset — the two places a value arrives from
/// the content attribute.
pub fn seed_input_value(input: &mut WebCore) {
    let is_range = input
        .attributes
        .get("type")
        .map(|t| t.trim().eq_ignore_ascii_case("range"))
        .unwrap_or(false);
    if !is_range {
        return;
    }
    let raw = input.attributes.get("value").cloned().unwrap_or_default();
    let sanitized = sanitize_range_value(input, &raw);
    input.value_state = Some(best_representation(sanitized));
    input.dirty_value = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(tag: &str, attrs: &[(&str, &str)]) -> WebCore {
        let mut n = WebCore::new(tag);
        for (k, v) in attrs {
            n.attributes.insert((*k), (*v));
        }
        n
    }

    #[test]
    fn the_specs_own_range_example_starts_at_sixty() {
        // HTML §4.10.5.1.13, verbatim: "the markup <input type=range min=0
        // max=100 step=20 value=50> results in a range control whose initial
        // value is 60." 50 is equidistant from 40 and 60; the tie goes UP.
        let input = el(
            "input",
            &[
                ("type", "range"),
                ("min", "0"),
                ("max", "100"),
                ("step", "20"),
                ("value", "50"),
            ],
        );
        assert_eq!(sanitize_range_value(&input, "50"), 60.0);
    }

    #[test]
    fn a_step_base_comes_from_value_when_there_is_no_min() {
        // Same step and value as above, but with no `min` the step base is the
        // VALUE — so 50 is already conforming and must not move.
        let input = el(
            "input",
            &[("type", "range"), ("step", "20"), ("value", "50")],
        );
        assert_eq!(sanitize_range_value(&input, "50"), 50.0);
    }

    #[test]
    fn an_invalid_value_becomes_the_default_not_zero() {
        // The spec's other note on the same example: "the invalid value ++50
        // was ignored". The default is min + (max-min)/2 = 50, NOT the minimum.
        let input = el(
            "input",
            &[
                ("type", "range"),
                ("min", "-100"),
                ("max", "100"),
                ("step", "any"),
            ],
        );
        assert_eq!(sanitize_range_value(&input, "++50"), 0.0);
        let plain = el("input", &[("type", "range"), ("step", "any")]);
        assert_eq!(sanitize_range_value(&plain, "not a number"), 50.0);
    }

    #[test]
    fn out_of_range_values_clamp_to_the_bounds() {
        let input = el(
            "input",
            &[
                ("type", "range"),
                ("min", "10"),
                ("max", "20"),
                ("step", "any"),
            ],
        );
        assert_eq!(sanitize_range_value(&input, "-5"), 10.0);
        assert_eq!(sanitize_range_value(&input, "1000"), 20.0);
    }

    #[test]
    fn snapping_stays_inside_the_bounds() {
        // The nearest step to 99 is 100, which is over the maximum, so the
        // answer is the largest conforming number at or under it.
        let input = el(
            "input",
            &[
                ("type", "range"),
                ("min", "0"),
                ("max", "95"),
                ("step", "10"),
            ],
        );
        assert_eq!(sanitize_range_value(&input, "99"), 90.0);
    }

    #[test]
    fn step_any_snaps_nothing() {
        let input = el("input", &[("type", "range"), ("step", "any")]);
        assert_eq!(sanitize_range_value(&input, "33.7"), 33.7);
    }

    #[test]
    fn a_bad_step_falls_back_to_the_default_rather_than_removing_it() {
        // "if the rules ... return an error, zero, or a number less than zero,
        // then the allowed value step is the default step". Range's is 1.
        for spelling in ["0", "-3", "banana"] {
            let input = el(
                "input",
                &[("type", "range"), ("min", "0"), ("step", spelling)],
            );
            assert_eq!(allowed_value_step(&input), Some(1.0), "step={spelling}");
            assert_eq!(sanitize_range_value(&input, "3.7"), 4.0, "step={spelling}");
        }
    }

    #[test]
    fn display_size_defaults_to_four_only_with_multiple() {
        assert_eq!(display_size(&el("select", &[])), 1);
        assert_eq!(display_size(&el("select", &[("multiple", "")])), 4);
        assert_eq!(display_size(&el("select", &[("size", "7")])), 7);
        // A `size` that does not parse falls back to the SAME default, so a
        // multi-select keeps its four rows.
        assert_eq!(
            display_size(&el("select", &[("size", "wide"), ("multiple", "")])),
            4
        );
        assert_eq!(display_size(&el("select", &[("size", "-2")])), 1);
    }

    #[test]
    fn a_list_box_is_decided_by_display_size_alone() {
        assert!(!is_list_box(&el("select", &[])));
        assert!(is_list_box(&el("select", &[("multiple", "")])));
        assert!(is_list_box(&el("select", &[("size", "4")])));
        // `size=0` PARSES, to zero — a drop-down, though the predicate this
        // replaced would have agreed for the wrong reason.
        assert!(!is_list_box(&el("select", &[("size", "0")])));
        // A multi-select with an explicit display size of 1 is not a list box:
        // the spec allows a multi-select drop-down there.
        assert!(!is_list_box(&el(
            "select",
            &[("multiple", ""), ("size", "1")]
        )));
    }

    #[test]
    fn options_are_counted_through_an_optgroup() {
        let mut select = el("select", &[]);
        select.children.push(el("option", &[("value", "a")]));
        let mut group = el("optgroup", &[("label", "g")]);
        group.children.push(el("option", &[("value", "b")]));
        // A nested optgroup's contents are excluded by the walk.
        let mut nested = el("optgroup", &[("label", "n")]);
        nested.children.push(el("option", &[("value", "hidden")]));
        group.children.push(nested);
        select.children.push(group);
        select.children.push(el("option", &[("value", "c")]));

        let values: Vec<String> = list_of_options(&select)
            .iter()
            .map(|o| option_value(o))
            .collect();
        assert_eq!(values, ["a", "b", "c"]);
    }

    #[test]
    fn parsing_ignores_trailing_junk_but_validity_does_not() {
        assert_eq!(parse_floating_point("4 rows"), Some(4.0));
        assert_eq!(parse_floating_point(" 50"), Some(50.0));
        assert_eq!(parse_floating_point("1e"), Some(1.0));
        assert_eq!(parse_floating_point("1e3"), Some(1000.0));
        assert_eq!(parse_floating_point("++50"), None);
        assert_eq!(parse_floating_point("inf"), None);
        assert_eq!(parse_non_negative_integer("4 rows"), Some(4));
        assert_eq!(parse_non_negative_integer("-1"), None);

        assert!(is_valid_floating_point("-1.5e+3"));
        assert!(is_valid_floating_point(".5"));
        assert!(!is_valid_floating_point(" 50"));
        assert!(!is_valid_floating_point("+50"));
        assert!(!is_valid_floating_point("1e"));
        assert!(!is_valid_floating_point("Infinity"));
    }

    #[test]
    fn best_representation_is_ecmascript_tostring() {
        assert_eq!(best_representation(0.0), "0");
        assert_eq!(best_representation(-0.0), "0");
        assert_eq!(best_representation(60.0), "60");
        assert_eq!(best_representation(-1.5), "-1.5");
        assert_eq!(best_representation(0.1), "0.1");
        // The two places Rust's own `Display` disagrees.
        assert_eq!(best_representation(1e21), "1e+21");
        assert_eq!(best_representation(1e-7), "1e-7");
        assert_eq!(best_representation(1e-6), "0.000001");
    }
}
