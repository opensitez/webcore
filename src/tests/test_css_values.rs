//! Tests for CSS value AST: min(), max(), clamp(), calc() with proper resolution.

use crate::css::{parse_color, parse_length};
use crate::types::{BorderStyle, Color, CssLength};

fn resolve(val: &CssLength, containing: f32, vw: f32) -> f32 {
    val.resolve_vp(16.0, containing, 16.0, vw, 600.0)
}

// ── Basic parsing ──────────────────────────────────────────────────────────

#[test]
fn parse_px() {
    assert_eq!(parse_length("100px"), CssLength::Px(100.0));
}

#[test]
fn parse_percent() {
    assert_eq!(parse_length("50%"), CssLength::Percent(50.0));
}

#[test]
fn parse_em() {
    assert_eq!(parse_length("2em"), CssLength::Em(2.0));
}

#[test]
fn parse_rem() {
    assert_eq!(parse_length("1.5rem"), CssLength::Rem(1.5));
}

#[test]
fn parse_vw() {
    assert_eq!(parse_length("100vw"), CssLength::Vw(100.0));
}

#[test]
fn container_query_units_parse_to_axis_lengths() {
    assert_eq!(parse_length("5cqw"), CssLength::Vw(5.0));
    assert_eq!(parse_length("5cqi"), CssLength::Vw(5.0));
    assert_eq!(parse_length("5cqh"), CssLength::Vh(5.0));
    assert_eq!(parse_length("5cqb"), CssLength::Vh(5.0));
    assert_eq!(parse_length("5cqmin"), CssLength::Vmin(5.0));
    assert_eq!(parse_length("5cqmax"), CssLength::Vmax(5.0));
}

#[test]
fn parse_auto() {
    assert_eq!(parse_length("auto"), CssLength::Auto);
}

#[test]
fn env_length_uses_fallback_and_known_safe_area_zero() {
    assert_eq!(
        parse_length("env(safe-area-inset-bottom, 20px)"),
        CssLength::Zero
    );
    assert_eq!(
        parse_length("env(--unknown-inset, 20px)"),
        CssLength::Px(20.0)
    );
    assert_eq!(parse_length("env(--unknown-inset)"), CssLength::Auto);
}

#[test]
fn env_color_uses_fallback_for_unknown_variables() {
    assert_eq!(
        parse_color("env(--brand-color, rebeccapurple)"),
        Some(Color::rgb(102, 51, 153))
    );
    assert_eq!(
        parse_color("env(--brand-color, color-mix(in srgb, red 25%, blue))"),
        Some(Color::rgb(64, 0, 191))
    );
    assert_eq!(parse_color("env(--brand-color)"), None);
    assert_eq!(parse_color("env(safe-area-inset-top, red)"), None);
}

// ── min() ──────────────────────────────────────────────────────────────────

#[test]
fn parse_min_two_args() {
    let val = parse_length("min(300px, 50%)");
    match &val {
        CssLength::Min(args) => assert_eq!(args.len(), 2),
        other => panic!("expected Min, got {:?}", other),
    }
}

#[test]
fn min_resolves_to_smaller() {
    let val = parse_length("min(300px, 50%)");
    // 50% of 800px = 400px, min(300, 400) = 300
    assert_eq!(resolve(&val, 800.0, 1024.0), 300.0);
    // 50% of 400px = 200px, min(300, 200) = 200
    assert_eq!(resolve(&val, 400.0, 1024.0), 200.0);
}

#[test]
fn min_with_vw() {
    let val = parse_length("min(100vw, 1200px)");
    // viewport 800: min(800, 1200) = 800
    assert_eq!(resolve(&val, 0.0, 800.0), 800.0);
    // viewport 1400: min(1400, 1200) = 1200
    assert_eq!(resolve(&val, 0.0, 1400.0), 1200.0);
}

// ── max() ──────────────────────────────────────────────────────────────────

#[test]
fn parse_max_two_args() {
    let val = parse_length("max(200px, 30%)");
    match &val {
        CssLength::Max(args) => assert_eq!(args.len(), 2),
        other => panic!("expected Max, got {:?}", other),
    }
}

#[test]
fn max_resolves_to_larger() {
    let val = parse_length("max(200px, 30%)");
    // 30% of 800px = 240px, max(200, 240) = 240
    assert!((resolve(&val, 800.0, 1024.0) - 240.0).abs() < 0.01);
    // 30% of 500px = 150px, max(200, 150) = 200
    assert!((resolve(&val, 500.0, 1024.0) - 200.0).abs() < 0.01);
}

// ── clamp() ────────────────────────────────────────────────────────────────

#[test]
fn parse_clamp_three_args() {
    let val = parse_length("clamp(200px, 50%, 600px)");
    match &val {
        CssLength::Clamp(_) => {} // ok
        other => panic!("expected Clamp, got {:?}", other),
    }
}

#[test]
fn clamp_resolves_correctly() {
    let val = parse_length("clamp(200px, 50%, 600px)");
    // 50% of 300 = 150 → clamped to min 200
    assert_eq!(resolve(&val, 300.0, 1024.0), 200.0);
    // 50% of 800 = 400 → within range
    assert_eq!(resolve(&val, 800.0, 1024.0), 400.0);
    // 50% of 1400 = 700 → clamped to max 600
    assert_eq!(resolve(&val, 1400.0, 1024.0), 600.0);
}

#[test]
fn clamp_with_mixed_units() {
    // Common responsive pattern: clamp(1rem, 2.5vw, 2rem)
    let val = parse_length("clamp(1rem, 2.5vw, 2rem)");
    // 1rem=16px, 2rem=32px, 2.5vw at 800px = 20px → within range
    assert_eq!(resolve(&val, 0.0, 800.0), 20.0);
    // 2.5vw at 500px = 12.5px → clamped to 1rem = 16px
    assert_eq!(resolve(&val, 0.0, 500.0), 16.0);
    // 2.5vw at 1600px = 40px → clamped to 2rem = 32px
    assert_eq!(resolve(&val, 0.0, 1600.0), 32.0);
}

// ── Nested ─────────────────────────────────────────────────────────────────

#[test]
fn nested_min_in_max() {
    // max(300px, min(50%, 600px))
    let val = parse_length("max(300px, min(50%, 600px))");
    // 50% of 800 = 400, min(400,600) = 400, max(300,400) = 400
    assert_eq!(resolve(&val, 800.0, 1024.0), 400.0);
    // 50% of 400 = 200, min(200,600) = 200, max(300,200) = 300
    assert_eq!(resolve(&val, 400.0, 1024.0), 300.0);
}

#[test]
fn clamp_with_calc() {
    // clamp(200px, calc(100% - 40px), 800px)
    let val = parse_length("clamp(200px, calc(100% - 40px), 800px)");
    // 100% of 500 - 40 = 460 → within range
    let r = resolve(&val, 500.0, 1024.0);
    assert!((r - 460.0).abs() < 1.0, "expected ~460, got {}", r);
    // 100% of 100 - 40 = 60 → clamped to 200
    assert_eq!(resolve(&val, 100.0, 1024.0), 200.0);
}

// ── Layout integration ─────────────────────────────────────────────────────

#[test]
fn min_width_in_layout() {
    let mut frame = crate::frame::EngineFrame::new(
        crate::html::parse_html(r#"<div id="box" style="width: min(300px, 80%)">content</div>"#),
        800.0,
        600.0,
    );
    frame.update_frame();

    let id = frame.doc.get_element_by_id("box").unwrap();
    let w = frame.doc.offset_width(id);
    // 80% of ~800 = 640, min(300, 640) = 300
    assert!(
        (w - 300.0).abs() < 5.0,
        "min(300px, 80%) at 800px viewport should be ~300px, got {}",
        w
    );
}

#[test]
fn clamp_width_in_layout() {
    let mut frame = crate::frame::EngineFrame::new(
        crate::html::parse_html(
            r#"<div id="box" style="width: clamp(200px, 50%, 600px)">content</div>"#,
        ),
        800.0,
        600.0,
    );
    frame.update_frame();

    let id = frame.doc.get_element_by_id("box").unwrap();
    let w = frame.doc.offset_width(id);
    // 50% of containing block (~784px after body margins), clamp(200, ~392, 600) → ~392
    assert!(
        w > 200.0 && w < 600.0,
        "clamp(200,50%,600) should be between bounds, got {}",
        w
    );
}

// ── line-height is not a plain length ────────────────────────────────────────

/// **A unitless `line-height` is a MULTIPLE of the font size, not pixels**
/// (CSS 2.1 §10.8.1). The value pre-parser had `line-height` in its generic
/// length group, so `line-height: 1.375` pre-parsed to 1.375 **pixels** while
/// the string path read it as `1.375em`. The two cascade paths therefore
/// disagreed: fr.wikipedia rendered normally and then, on the first hover,
/// every heading lost its leading and `#firstHeading` collapsed to one pixel
/// tall — its text vanished.
#[test]
fn line_height_number_is_a_multiple_not_pixels() {
    for (input, expect) in [
        ("1.375", crate::types::CssLength::Em(1.375)),
        ("2", crate::types::CssLength::Em(2.0)),
        ("normal", crate::types::CssLength::Em(1.2)),
    ] {
        // The string path.
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "line-height", input);
        assert_eq!(s.line_height, expect, "string path for {input:?}");

        // The pre-parsed path, which is what the cascade normally takes.
        let id = crate::css::properties::PropertyId::LineHeight;
        let pre = crate::css::rule::pre_parse_value(id, input);
        let mut p = crate::types::ComputedStyle::default();
        crate::css::apply_css_value(&mut p, id, &pre);
        assert_eq!(
            p.line_height, expect,
            "pre-parsed path for {input:?} (pre-parsed as {pre:?})"
        );
    }
}

/// A `line-height` written with a unit keeps that unit.
#[test]
fn line_height_with_a_unit_stays_a_length() {
    for input in ["20px", "1.5em", "150%", "2rem"] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "line-height", input);
        let id = crate::css::properties::PropertyId::LineHeight;
        let pre = crate::css::rule::pre_parse_value(id, input);
        let mut p = crate::types::ComputedStyle::default();
        crate::css::apply_css_value(&mut p, id, &pre);
        assert_eq!(
            s.line_height, p.line_height,
            "the two value paths disagree about {input:?}"
        );
    }
}

// ── background-image url() forms ─────────────────────────────────────────────

/// **A protocol-relative, unquoted `url()` is a URL.** fr.wikipedia's main
/// banner is `background-image:url(//upload.wikimedia.org/...svg)`; the rule
/// applies (its `display:flex` reaches the box) but the image never painted.
#[test]
fn background_image_url_forms_all_parse() {
    for input in [
        "url(//upload.wikimedia.org/wikipedia/commons/a/aa/Wikipedia-logo-v2-o50.svg)",
        "url('//upload.wikimedia.org/wikipedia/commons/a/aa/Wikipedia-logo-v2-o50.svg')",
        "url(\"//upload.wikimedia.org/wikipedia/commons/a/aa/Wikipedia-logo-v2-o50.svg\")",
    ] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "background-image", input);
        assert_eq!(
            s.background_image_url,
            "//upload.wikimedia.org/wikipedia/commons/a/aa/Wikipedia-logo-v2-o50.svg",
            "background-image did not parse: {input}"
        );
    }
}

#[test]
fn background_image_data_url_keeps_semicolon_and_comma_payload() {
    for input in [
        "url(data:image/png;base64,iVBORw0KGgo=)",
        "url('data:image/svg+xml;utf8,<svg viewBox=\"0 0 1 1\"></svg>')",
        "url(\"data:image/svg+xml;base64,PHN2Zy8+\")",
    ] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "background-image", input);
        assert!(
            s.background_image_url.starts_with("data:image/"),
            "background-image did not keep data URL: {input:?} -> {:?}",
            s.background_image_url
        );
        assert!(
            s.background_image_url.contains(';') && s.background_image_url.contains(','),
            "data URL metadata/payload separator was lost: {input:?} -> {:?}",
            s.background_image_url
        );
    }
}

#[test]
fn background_image_image_set_selects_supported_one_x_candidate() {
    for (input, expected) in [
        (
            "image-set(url(hero@2x.png) 2x, url(hero.png) 1x)",
            "hero.png",
        ),
        (
            "image-set(\"hero.avif\" type(\"image/avif\") 1x, url(hero.png) type(\"image/png\") 1x)",
            "hero.png",
        ),
        (
            "-webkit-image-set(url(hero@2x.png) 2x, url(hero.png) 1x)",
            "hero.png",
        ),
    ] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "background-image", input);
        assert_eq!(
            s.background_image_url, expected,
            "background-image image-set chose the wrong candidate for {input}"
        );
    }
}

#[test]
fn background_image_image_set_keeps_quoted_data_url_candidate_together() {
    let mut s = crate::types::ComputedStyle::default();
    crate::css::apply_property(
        &mut s,
        "background-image",
        r#"image-set("data:image/png;base64,AAAA" 1x, url(hero@2x.png) 2x)"#,
    );
    assert_eq!(s.background_image_url, "data:image/png;base64,AAAA");
}

/// And it must resolve against the document origin, keeping the scheme.
#[test]
fn protocol_relative_url_takes_the_base_scheme() {
    let got = crate::html::images::resolve_url(
        "//upload.wikimedia.org/a.svg",
        "https://fr.wikipedia.org/wiki/Foo",
    );
    assert_eq!(got, "https://upload.wikimedia.org/a.svg");
}

#[test]
fn file_url_base_directory_resolves_relative_stylesheets() {
    let got = crate::html::images::resolve_url(
        "support/flexbox.css",
        "file:///tmp/webcore/wpt/css-flexbox",
    );
    assert_eq!(got, "/tmp/webcore/wpt/css-flexbox/support/flexbox.css");
}

#[test]
fn explicit_file_url_resolves_to_local_path() {
    let got = crate::html::images::resolve_url(
        "file:///tmp/webcore/wpt/fonts/ahem.css",
        "file:///tmp/webcore/wpt/css-flexbox",
    );
    assert_eq!(got, "/tmp/webcore/wpt/fonts/ahem.css");
}

// ── Colour keywords and accent-color ────────────────────────────────────────

/// **Colour keywords are ASCII case-insensitive.** Legacy presentational HTML
/// still ships `bgcolor="White"` and `color="Red"`; matching the keyword table
/// byte-exactly dropped the declaration and left the element at its default.
#[test]
fn colour_keywords_are_case_insensitive() {
    for (input, expect) in [
        ("White", (255u8, 255u8, 255u8)),
        ("RED", (255, 0, 0)),
        ("Blue", (0, 0, 255)),
        ("white", (255, 255, 255)),
    ] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "background-color", input);
        let c = s.background_color;
        assert_eq!((c.r, c.g, c.b), expect, "background-color: {input}");
    }
}

#[test]
fn system_color_keywords_parse_case_insensitively() {
    let mut s = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut s, "background-color", "Canvas");
    crate::css::apply_property(&mut s, "color", "CanvasText");

    assert_eq!(s.background_color, crate::types::Color::WHITE);
    assert_eq!(s.color, crate::types::Color::BLACK);
}

#[test]
fn scrollbar_color_parses_pairs_and_auto_reset() {
    let mut s = crate::types::ComputedStyle::default();

    crate::css::apply_property(&mut s, "scrollbar-color", "red blue");
    assert_eq!(
        s.scrollbar_thumb_color,
        Some(crate::types::Color::rgb(255, 0, 0))
    );
    assert_eq!(
        s.scrollbar_track_color,
        Some(crate::types::Color::rgb(0, 0, 255))
    );

    crate::css::apply_property(&mut s, "scrollbar-color", "red banana");
    assert_eq!(
        s.scrollbar_thumb_color,
        Some(crate::types::Color::rgb(255, 0, 0))
    );
    assert_eq!(
        s.scrollbar_track_color,
        Some(crate::types::Color::rgb(0, 0, 255))
    );

    crate::css::apply_property(&mut s, "scrollbar-color", "auto");
    assert_eq!(s.scrollbar_thumb_color, None);
    assert_eq!(s.scrollbar_track_color, None);
}

/// **`accent-color` must not touch `background-color`.** With no accent field
/// of its own it was assigned straight into `background_color`, so it painted a
/// solid block and fought a real `background-color` in the same rule. Until the
/// accent is actually plumbed through to control painting, doing nothing is the
/// correct behaviour — doing damage is not.
#[test]
fn accent_color_does_not_overwrite_the_background() {
    let mut s = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut s, "background-color", "white");
    crate::css::apply_property(&mut s, "accent-color", "red");
    let c = s.background_color;
    assert_eq!(
        (c.r, c.g, c.b),
        (255, 255, 255),
        "accent-color clobbered background-color: got {},{},{}",
        c.r,
        c.g,
        c.b
    );
}

/// `font-size: xxx-large` is a real absolute-size keyword; falling through to
/// the length parser made it `auto`, which resolves to **0** and the text
/// disappears.
#[test]
fn font_size_xxx_large_is_not_zero() {
    let mut s = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut s, "font-size", "xxx-large");
    let px = s.font_size.resolve(16.0, 0.0, 16.0);
    assert!(
        px > 30.0,
        "xxx-large should be larger than xx-large, got {px}"
    );
}

/// **`overflow: clip` clips.** An unrecognised value fell through to
/// `Overflow::Visible`, which does not merely ignore the declaration — it
/// actively disables the clipping the page asked for. Pages increasingly prefer
/// `clip` over `hidden` precisely because it does not create a scroll container.
#[test]
fn overflow_clip_is_not_visible() {
    let mut s = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut s, "overflow", "clip");
    assert_eq!(
        s.overflow_x,
        crate::types::Overflow::Clip,
        "overflow: clip must clip"
    );
    assert_eq!(
        s.overflow_y,
        crate::types::Overflow::Clip,
        "overflow: clip must clip"
    );
}

// ── currentColor (css-color-4 §6.2) ────────────────────────────────────────

/// `currentColor` computes to the value of the element's own `color`, whatever
/// order the declarations appear in. Resolving it as the declaration is applied
/// reads a `color` the cascade has not finished writing, so the second block
/// below is the one that discriminates: `border` is declared BEFORE `color`.
#[test]
fn current_color_resolves_to_the_elements_own_color() {
    fn border_top(html: &str) -> crate::types::Color {
        let doc = crate::tests::harness::parse_and_layout(html, 800.0);
        let b =
            crate::tests::harness::find_box(&doc.root, &|b: &crate::types::WebCore| b.tag == "div")
                .expect("div box");
        b.style.border_top_color
    }

    let c = border_top("<div style='color: white; border: 1px solid currentColor'>x</div>");
    assert_eq!(
        (c.r, c.g, c.b),
        (255, 255, 255),
        "color first: currentColor should be white, got {},{},{}",
        c.r,
        c.g,
        c.b
    );

    let c = border_top("<div style='border: 1px solid currentColor; color: white'>x</div>");
    assert_eq!(
        (c.r, c.g, c.b),
        (255, 255, 255),
        "border first: currentColor should still be white, got {},{},{}",
        c.r,
        c.g,
        c.b
    );

    // And a real colour declared after `currentColor` wins — the deferred
    // resolution must not overwrite the cascade's actual winner.
    let c = border_top(
        "<div style='color: white; border-color: currentColor; border-color: rgb(1,2,3)'>x</div>",
    );
    assert_eq!(
        (c.r, c.g, c.b),
        (1, 2, 3),
        "a later real colour must win over an earlier currentColor, got {},{},{}",
        c.r,
        c.g,
        c.b
    );
}

// ── Modern colour syntax (cssgaps.md row 2) ──────────────────────────────────

/// css-color-4 §7–§10 and css-color-5 §3: `oklch()`, `oklab()`, `lab()`,
/// `lch()`, `hwb()`, `color()` and `color-mix()` are all valid `<color>`
/// productions with a defined mapping into the sRGB gamut. `parse_color`
/// (`value_parse.rs:282`-`513`) has no branch for any of them — the function
/// ends after the `hsl(...)` arm — so each returns `None`, the declaration is
/// dropped, and the element keeps whatever colour it already had.
/// **Tailwind v4 emits `oklch()` for its entire default palette**, so every
/// one of those utility classes is currently a silent no-op.
///
/// Expected sRGB bytes are ground truth read from real Chrome
/// (152.0.7977.82, headless: `--remote-debugging-port=9241
/// --disable-gpu --user-data-dir=/tmp/kchrome-gap2`), not computed by hand —
/// css-color-4's oklab/oklch/lab/lch matrices are easy to get subtly wrong.
/// Method: a 1×1 `<canvas>`, `ctx.fillStyle = <value>` then
/// `ctx.fillRect(0,0,1,1)`, then `ctx.getImageData(0,0,1,1).data` — this
/// forces the browser's real colour-space conversion into concrete sRGB
/// bytes, unlike `getComputedStyle().color`, which serializes oklch/oklab/
/// lab/lch/color() back into their own notation and only converts hwb() to
/// rgb(). Allow ±2 per channel for rounding.
#[test]
fn modern_colour_syntax_parses() {
    for (input, expect) in [
        ("oklch(0.7 0.15 30)", (237u8, 118u8, 101u8)),
        ("oklch(62.8% 0.25768 29.23)", (255, 0, 0)), // canonical oklch red
        ("oklab(0.7 0.1 0.1)", (229, 127, 78)),
        ("lab(50% 40 59.5)", (191, 87, 0)),
        ("lch(52.2% 72.2 50)", (205, 86, 26)),
        ("hwb(120 20% 30%)", (51, 179, 51)),
        ("color(srgb 1 0 0)", (255, 0, 0)),
        ("color-mix(in srgb, red 50%, blue 50%)", (128, 0, 128)),
    ] {
        let mut s = crate::types::ComputedStyle::default();
        // Sentinel: default `color` is black, and none of the expected
        // values are black, so a dropped declaration is unambiguous.
        crate::css::apply_property(&mut s, "color", input);
        let c = s.color;
        let close = |a: u8, b: u8| (a as i32 - b as i32).abs() <= 2;
        assert!(
            close(c.r, expect.0) && close(c.g, expect.1) && close(c.b, expect.2),
            "color: {input} should resolve to rgb{:?} (+/-2), got rgb({},{},{})",
            expect,
            c.r,
            c.g,
            c.b
        );
    }
}

// ── opacity (cssgaps.md row "opacity rejects 50%/calc()") ───────────────────

/// css-color-4 §14: `<alpha-value>` is a `<number>` **or** a `<percentage>`
/// (the two are declared equivalent, `50%` == `0.5`), and `calc()` of either
/// is a normal value. `apply_opacity` (`property_defs.rs:496`) does
/// `v.parse::<f32>()` on the raw string, which fails on anything containing
/// `%` or `calc(`, and silently falls back to fully opaque (1.0) via
/// `unwrap_or(1.0)`.
#[test]
fn opacity_accepts_percentage_and_calc() {
    for (input, expect) in [
        ("50%", 0.5f32),
        ("calc(0.5)", 0.5f32),
        ("calc(50%)", 0.5f32),
    ] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "opacity", input);
        assert!(
            (s.opacity - expect).abs() < 0.01,
            "opacity: {input} should resolve to {expect}, got {}",
            s.opacity
        );
    }
}

// ── calc() gaps (cssgaps.md rows under "Values, colour, fonts") ─────────────

/// css-values-4 §10.6: `Product` is `<calc-value> [ '*' <calc-value> ]*` —
/// multiplication commutes, so a leading scalar must work exactly like a
/// trailing one. `parse_calc_tree_multiplicative` (`calc.rs:69`-`100`) finds
/// the `*` and only accepts a bare scalar on the RIGHT
/// (`rhs_str.parse::<f32>()`); with the scalar on the left, that parse fails
/// and the code falls through to `parse_calc_tree_atom` on the WHOLE
/// expression `"2 * min(50%, 300px)"`, which `parse_length` cannot parse and
/// answers `CssLength::Auto` — silently 0 (`CssLength::Auto => 0.0`,
/// `length.rs:129`).
#[test]
fn calc_multiplication_by_a_leading_scalar_is_not_zero() {
    let containing = 800.0;
    let vw = 1024.0;
    let left = resolve(&parse_length("calc(2 * min(50%, 300px))"), containing, vw);
    let right = resolve(&parse_length("calc(min(50%, 300px) * 2)"), containing, vw);
    // min(50%, 300px) of an 800px containing block is min(400,300) = 300px; * 2 = 600px.
    assert_eq!(
        right, 600.0,
        "sanity: the scalar-on-the-right form must still work"
    );
    assert_eq!(
        left, right,
        "calc(2 * min(...)) should equal calc(min(...) * 2) — multiplication commutes; got {left} vs {right}"
    );
}

/// css-values-4 §6.1.2: `vmin` is "the smaller of vw and vh", the SAME
/// definition whether or not it appears inside `calc()`. The coefficient-
/// based calc parser has no vmin/vmax coefficient slot and reuses the `vw`
/// slot for `CssLength::Vmin`/`Vmax` (`calc.rs:294`-`309`, the `Vmin(v) |
/// Vmax(v) => c[4] = v` arm — the comment there already admits it is the
/// wrong axis). With only that one coefficient non-zero, `parse_calc`
/// (`calc.rs:36`-`43`) collapses the whole `calc(50vmin)` to plain
/// `CssLength::Vw(50.0)`, which then resolves against `vw` alone instead of
/// `min(vw, vh)` — the SAME textual length gives two different answers
/// depending only on whether it's spelled inside `calc()`.
#[test]
fn calc_vmin_resolves_against_the_smaller_axis_like_bare_vmin() {
    let viewport_w = 1024.0;
    let viewport_h = 600.0; // the smaller axis
    let bare_px = parse_length("50vmin").resolve_vp(16.0, 0.0, 16.0, viewport_w, viewport_h);
    let calc_px = parse_length("calc(50vmin)").resolve_vp(16.0, 0.0, 16.0, viewport_w, viewport_h);
    assert_eq!(
        bare_px, 300.0,
        "sanity: 50vmin of a 1024x600 viewport is min(1024,600)*0.5 = 300px"
    );
    assert_eq!(
        calc_px, bare_px,
        "calc(50vmin) disagreed with bare 50vmin: {calc_px}px vs {bare_px}px"
    );
}

/// css-values-4 §5.1 / css-syntax-3: a `calc()` component in a unit the
/// engine does not understand does not parse as a length at all — folding it
/// in as if it were pixels is not a conforming fallback. The coefficient-
/// based calc term parser's catch-all (`calc.rs:308`, `_ => c[1] = num`)
/// reads the numeric part of `"1lh"` as `"1"` and adds it to the px
/// coefficient as though the unit were absent — so `calc(1lh + 2px)`
/// silently becomes 3px instead of leaving the declaration invalid.
///
/// Two cases, because `lh`'s own fix is ambiguous: `parse_length` may grow a
/// real `lh`/`rlh` arm (they're legitimate CSS Values 4 units — cssgaps.md
/// lists them only as the example that currently triggers the bug), in which
/// case `calc(1lh + 2px)` should correctly resolve against the line box
/// rather than stay invalid, and the first case below would need reworking.
/// `zz` is not a CSS unit under any fix and pins the general rule
/// unambiguously: a calc() with a component in a unit `parse_length` cannot
/// ever recognise must not silently overwrite a value that DID parse.
#[test]
fn calc_does_not_silently_treat_an_unknown_unit_as_pixels() {
    for bad in ["calc(1lh + 2px)", "calc(1zz + 2px)"] {
        let mut s = crate::types::ComputedStyle::default();
        crate::css::apply_property(&mut s, "width", "50px");
        crate::css::apply_property(&mut s, "width", bad);
        let px = s.width.resolve(16.0, 0.0, 16.0);
        assert_eq!(
            px, 50.0,
            "a calc() containing an unsupported unit must not silently overwrite a valid `width: 50px` (from {bad:?}); got {px}px"
        );
    }
}

// ── Flow-relative (logical) box properties ─────────────────────────────────
//
// These run through the real cascade, not `apply_property` alone: the mapping
// onto physical sides is deliberately DEFERRED to the end of the cascade (it
// depends on the element's final `direction`/`writing-mode`), so a test that
// applied declarations to a bare `ComputedStyle` would be asserting against a
// half-finished style.

fn cascaded(html: &str) -> std::sync::Arc<crate::types::ComputedStyle> {
    let doc = crate::tests::harness::parse_and_layout(html, 800.0);
    let b = crate::tests::harness::find_box(&doc.root, &|b: &crate::types::WebCore| {
        b.attributes.get("id").map(|s| s.as_str()) == Some("t")
    })
    .expect("the #t box");
    b.style.clone()
}

/// css-logical-1 §4.2/§4.4 and css-writing-modes-4 §6.4: which physical side
/// `inline-start` names comes from the element's computed `direction`. Tailwind
/// emits `margin-inline-start`/`padding-inline-end` for its `ms-*`/`pe-*`
/// utilities, so an RTL page had its whole gutter system mirrored.
#[test]
fn logical_inline_margins_and_padding_follow_direction() {
    let ltr = cascaded(
        "<div id='t' style='direction:ltr; margin-inline-start:12px; padding-inline-end:7px'>x</div>",
    );
    assert_eq!(
        ltr.margin_left,
        CssLength::Px(12.0),
        "ltr: inline-start is the left margin"
    );
    assert_ne!(
        ltr.margin_right,
        CssLength::Px(12.0),
        "and not also the right"
    );
    assert_eq!(
        ltr.padding_right,
        CssLength::Px(7.0),
        "ltr: inline-end is the right padding"
    );

    let rtl = cascaded(
        "<div id='t' style='direction:rtl; margin-inline-start:12px; padding-inline-end:7px'>x</div>",
    );
    assert_eq!(
        rtl.margin_right,
        CssLength::Px(12.0),
        "rtl: inline-start is the RIGHT margin"
    );
    assert_ne!(rtl.margin_left, CssLength::Px(12.0), "and not the left");
    assert_eq!(
        rtl.padding_left,
        CssLength::Px(7.0),
        "rtl: inline-end is the LEFT padding"
    );
}

/// css-logical-1 §4.1 defines `min-`/`max-inline-size` and `-block-size`
/// alongside `inline-size`. They were not in the property table at all, so
/// `max-inline-size: 65ch` — the standard measure cap in a modern typographic
/// reset — was dropped and body text ran the full viewport width.
#[test]
fn logical_min_and_max_sizes_map_to_physical_min_and_max() {
    let s = cascaded(
        "<div id='t' style='max-inline-size:300px; min-inline-size:100px; \
                       max-block-size:400px; min-block-size:50px'>x</div>",
    );
    assert_eq!(
        s.max_width,
        CssLength::Px(300.0),
        "max-inline-size is max-width in horizontal-tb"
    );
    assert_eq!(
        s.min_width,
        CssLength::Px(100.0),
        "min-inline-size is min-width"
    );
    assert_eq!(
        s.max_height,
        CssLength::Px(400.0),
        "max-block-size is max-height"
    );
    assert_eq!(
        s.min_height,
        CssLength::Px(50.0),
        "min-block-size is min-height"
    );
}

#[test]
fn logical_borders_follow_direction_after_cascade() {
    let ltr = cascaded(
        "<div id='t' style='direction:ltr; border-inline-start:4px solid red; border-block-end-width:7px'>x</div>",
    );
    assert_eq!(ltr.border_left_width, CssLength::Px(4.0));
    assert_eq!(ltr.border_left_style, BorderStyle::Solid);
    assert_eq!(ltr.border_left_color, Color::rgb(255, 0, 0));
    assert_eq!(ltr.border_bottom_width, CssLength::Px(7.0));

    let rtl = cascaded(
        "<div id='t' style='border-inline-start-width:5px; border-inline-start-style:dashed; border-inline-start-color:blue; direction:rtl'>x</div>",
    );
    assert_eq!(
        rtl.border_right_width,
        CssLength::Px(5.0),
        "rtl inline-start maps to right even when direction is declared later"
    );
    assert_eq!(rtl.border_right_style, BorderStyle::Dashed);
    assert_eq!(rtl.border_right_color, Color::rgb(0, 0, 255));
}

/// css-writing-modes-4 §6.4: `inline-size` is `width` only while the inline
/// axis is horizontal. In `vertical-rl` it is the HEIGHT, and `block-size` is
/// the width — both were hard-aliased to `apply_width`/`apply_height`.
#[test]
fn inline_size_follows_the_writing_mode() {
    let s = cascaded(
        "<div id='t' style='writing-mode:vertical-rl; inline-size:120px; block-size:60px'>x</div>",
    );
    assert_eq!(
        s.height,
        CssLength::Px(120.0),
        "vertical-rl: inline-size is the height"
    );
    assert_eq!(
        s.width,
        CssLength::Px(60.0),
        "vertical-rl: block-size is the width"
    );
}

/// The mapping uses the element's COMPUTED `direction`, so it cannot depend on
/// where in the block `direction` was declared — css-logical-1 §4. Mapping as
/// each declaration was applied made these two orders disagree.
#[test]
fn logical_mapping_does_not_depend_on_declaration_order() {
    let before = cascaded(
        "<div id='t' style='direction:rtl; inset-inline-start:10px; position:relative'>x</div>",
    );
    let after = cascaded(
        "<div id='t' style='inset-inline-start:10px; direction:rtl; position:relative'>x</div>",
    );
    assert_eq!(
        before.right,
        CssLength::Px(10.0),
        "rtl: inline-start is the right inset"
    );
    assert_eq!(
        after.right, before.right,
        "declaring `direction` after `inset-inline-start` must give the same answer, got {:?} vs {:?}",
        after.right, before.right
    );
    assert_eq!(after.left, before.left, "and the same left inset");
}
