// Tests for the CSS animation and transition runtime.

use super::harness::*;
use crate::css::{
    Stylesheet, extract_keyframes, parse_animation_shorthand, parse_easing,
    parse_transition_shorthand,
};
use crate::html::parse_html;
use crate::layout::LayoutEngine;
use crate::types::*;
use std::time::{Duration, Instant};

// ── Easing function parsing ───────────────────────────────────────────────────

#[test]
fn easing_linear() {
    assert_eq!(parse_easing("linear"), EasingFn::Linear);
}

#[test]
fn easing_keywords() {
    assert_eq!(parse_easing("ease"), EasingFn::Ease);
    assert_eq!(parse_easing("ease-in"), EasingFn::EaseIn);
    assert_eq!(parse_easing("ease-out"), EasingFn::EaseOut);
    assert_eq!(parse_easing("ease-in-out"), EasingFn::EaseInOut);
    assert_eq!(parse_easing("step-start"), EasingFn::StepStart);
    assert_eq!(parse_easing("step-end"), EasingFn::StepEnd);
}

#[test]
fn easing_cubic_bezier() {
    assert_eq!(
        parse_easing("cubic-bezier(0.25, 0.1, 0.25, 1.0)"),
        EasingFn::CubicBezier(0.25, 0.1, 0.25, 1.0)
    );
}

#[test]
fn easing_steps_start() {
    assert_eq!(
        parse_easing("steps(4, start)"),
        EasingFn::Steps(4, StepPosition::JumpStart)
    );
}

#[test]
fn easing_steps_end() {
    assert_eq!(
        parse_easing("steps(3, end)"),
        EasingFn::Steps(3, StepPosition::JumpEnd)
    );
}

#[test]
fn easing_unknown_defaults_to_ease() {
    assert_eq!(parse_easing("bogus"), EasingFn::Ease);
}

// ── Easing function math ──────────────────────────────────────────────────────

#[test]
fn apply_easing_linear_is_identity() {
    let e = EasingFn::Linear;
    assert!((apply_easing(&e, 0.0) - 0.0).abs() < 1e-4);
    assert!((apply_easing(&e, 0.5) - 0.5).abs() < 1e-4);
    assert!((apply_easing(&e, 1.0) - 1.0).abs() < 1e-4);
}

#[test]
fn apply_easing_boundary_values() {
    for easing in [
        EasingFn::Ease,
        EasingFn::EaseIn,
        EasingFn::EaseOut,
        EasingFn::EaseInOut,
    ] {
        let v0 = apply_easing(&easing, 0.0);
        let v1 = apply_easing(&easing, 1.0);
        assert!(
            v0.abs() < 1e-3,
            "easing {:?} at t=0 should be ~0, got {}",
            easing,
            v0
        );
        assert!(
            (v1 - 1.0).abs() < 1e-3,
            "easing {:?} at t=1 should be ~1, got {}",
            easing,
            v1
        );
    }
}

#[test]
fn apply_easing_step_start() {
    let e = EasingFn::StepStart;
    // css-easing-2 §2.3: `step-start` is `steps(1, jump-start)`, whose first
    // interval — `[0, 1)`, t=0 included — outputs 1. Jumping at the START is
    // what the value means; answering 0 there was step-END's behaviour.
    assert_eq!(apply_easing(&e, 0.0), 1.0);
    assert_eq!(apply_easing(&e, 0.5), 1.0);
    assert_eq!(apply_easing(&e, 1.0), 1.0);
}

#[test]
fn apply_easing_step_end() {
    let e = EasingFn::StepEnd;
    assert_eq!(apply_easing(&e, 0.0), 0.0);
    assert_eq!(apply_easing(&e, 0.99), 0.0);
    assert_eq!(apply_easing(&e, 1.0), 1.0);
}

#[test]
fn apply_easing_steps_4() {
    let e = EasingFn::Steps(4, StepPosition::JumpEnd);
    let v = apply_easing(&e, 0.6);
    // floor(0.6 * 4) / 4 = floor(2.4)/4 = 2/4 = 0.5
    assert!((v - 0.5).abs() < 1e-4, "expected 0.5, got {}", v);
}

// ── animation shorthand parsing ───────────────────────────────────────────────

#[test]
fn parse_animation_simple() {
    let anims = parse_animation_shorthand("spin 1s linear infinite");
    assert_eq!(anims.len(), 1);
    let a = &anims[0];
    assert_eq!(a.name, "spin");
    assert!((a.duration_ms - 1000.0).abs() < 1.0);
    assert_eq!(a.timing_fn, EasingFn::Linear);
    assert!(a.iteration_count.is_infinite());
}

#[test]
fn parse_animation_with_delay() {
    let anims = parse_animation_shorthand("fade 0.3s ease-in 0.1s");
    assert_eq!(anims.len(), 1);
    let a = &anims[0];
    assert_eq!(a.name, "fade");
    assert!((a.duration_ms - 300.0).abs() < 1.0);
    assert!((a.delay_ms - 100.0).abs() < 1.0);
    assert_eq!(a.timing_fn, EasingFn::EaseIn);
}

#[test]
fn parse_animation_fill_mode() {
    let anims = parse_animation_shorthand("slide 2s both");
    assert_eq!(anims.len(), 1);
    assert_eq!(anims[0].fill_mode, FillMode::Both);
}

#[test]
fn parse_animation_direction_alternate() {
    let anims = parse_animation_shorthand("bounce 1s alternate");
    assert_eq!(anims.len(), 1);
    assert_eq!(anims[0].direction, AnimDirection::Alternate);
}

#[test]
fn parse_animation_multiple() {
    let anims = parse_animation_shorthand("spin 1s, fade 0.5s ease-out");
    assert_eq!(anims.len(), 2);
    assert_eq!(anims[0].name, "spin");
    assert_eq!(anims[1].name, "fade");
    assert_eq!(anims[1].timing_fn, EasingFn::EaseOut);
}

#[test]
fn parse_animation_paused() {
    let anims = parse_animation_shorthand("pulse 1s paused");
    assert_eq!(anims.len(), 1);
    assert!(anims[0].play_state_paused);
}

#[test]
fn parse_animation_none_is_skipped() {
    let anims = parse_animation_shorthand("none");
    assert_eq!(anims.len(), 0);
}

#[test]
fn parse_animation_iteration_count_number() {
    let anims = parse_animation_shorthand("flash 0.5s 3");
    assert_eq!(anims.len(), 1);
    assert!((anims[0].iteration_count - 3.0).abs() < 0.01);
}

#[test]
fn animation_composition_is_parsed_and_stored() {
    let anims = parse_animation_shorthand("fade 1s linear add");
    assert_eq!(anims.len(), 1);
    assert_eq!(anims[0].name, "fade");
    assert_eq!(anims[0].composition, AnimationComposition::Add);

    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "animation-composition", "accumulate");
    assert_eq!(
        s.rare().animations[0].composition,
        AnimationComposition::Accumulate
    );
    crate::css::apply_property(&mut s, "animation-composition", "replace");
    assert_eq!(
        s.rare().animations[0].composition,
        AnimationComposition::Replace
    );
}

// ── transition shorthand parsing ──────────────────────────────────────────────

#[test]
fn parse_transition_simple() {
    let trs = parse_transition_shorthand("color 0.3s ease");
    assert_eq!(trs.len(), 1);
    assert_eq!(trs[0].property, "color");
    assert!((trs[0].duration_ms - 300.0).abs() < 1.0);
    assert_eq!(trs[0].timing_fn, EasingFn::Ease);
}

#[test]
fn parse_transition_with_delay() {
    let trs = parse_transition_shorthand("opacity 1s linear 0.5s");
    assert_eq!(trs.len(), 1);
    assert_eq!(trs[0].property, "opacity");
    assert!((trs[0].duration_ms - 1000.0).abs() < 1.0);
    assert!((trs[0].delay_ms - 500.0).abs() < 1.0);
}

#[test]
fn parse_transition_multiple() {
    let trs = parse_transition_shorthand("color 0.3s, transform 0.5s ease-out");
    assert_eq!(trs.len(), 2);
    assert_eq!(trs[0].property, "color");
    assert_eq!(trs[1].property, "transform");
    assert_eq!(trs[1].timing_fn, EasingFn::EaseOut);
}

#[test]
fn parse_transition_none_skipped() {
    let trs = parse_transition_shorthand("none");
    assert_eq!(trs.len(), 0);
}

// ── @keyframes extraction ─────────────────────────────────────────────────────

#[test]
fn extract_keyframes_basic() {
    let css = r#"
        @keyframes spin {
            from { transform: rotate(0deg); }
            to   { transform: rotate(360deg); }
        }
    "#;
    let kf = extract_keyframes(css);
    assert!(kf.contains_key("spin"), "should contain 'spin'");
    let stops = &kf["spin"];
    assert_eq!(stops.len(), 2);
    assert!((stops[0].offset - 0.0).abs() < 1e-4);
    assert!((stops[1].offset - 1.0).abs() < 1e-4);
}

#[test]
fn extract_keyframes_percent() {
    let css = r#"
        @keyframes pulse {
            0%   { opacity: 1; }
            50%  { opacity: 0.5; }
            100% { opacity: 1; }
        }
    "#;
    let kf = extract_keyframes(css);
    let stops = &kf["pulse"];
    assert_eq!(stops.len(), 3);
    assert!((stops[1].offset - 0.5).abs() < 1e-4);
}

#[test]
fn extract_keyframes_ignores_invalid_selectors() {
    let css = r#"
        @keyframes pulse {
            abc% { opacity: 0; }
            150% { opacity: 0.5; }
            0 { opacity: 0.75; }
            50%, to { opacity: 1; }
        }
    "#;
    let kf = extract_keyframes(css);
    let stops = &kf["pulse"];
    assert_eq!(stops.len(), 2);
    assert!((stops[0].offset - 0.5).abs() < 1e-4);
    assert!((stops[1].offset - 1.0).abs() < 1e-4);
}

#[test]
fn extract_keyframes_multiple_selectors() {
    let css = r#"
        @keyframes blink {
            0%, 100% { opacity: 1; }
            50%       { opacity: 0; }
        }
    "#;
    let kf = extract_keyframes(css);
    let stops = &kf["blink"];
    // 0% + 100% + 50% = 3 stops (sorted by offset)
    assert_eq!(stops.len(), 3);
}

#[test]
fn duplicate_keyframe_selectors_cascade_into_one_stop() {
    let css = r#"
        @keyframes fade {
            50% { margin-left: 110px; opacity: 1; }
            50% { opacity: 0.9; }
        }
    "#;
    let kf = extract_keyframes(css);
    let stops = &kf["fade"];
    assert_eq!(stops.len(), 1);
    assert_eq!(
        stops[0]
            .properties
            .iter()
            .find(|(k, _)| k == "margin-left")
            .unwrap()
            .1,
        "110px"
    );
    assert_eq!(
        stops[0]
            .properties
            .iter()
            .find(|(k, _)| k == "opacity")
            .unwrap()
            .1,
        "0.9"
    );
}

#[test]
fn extract_keyframes_multiple_animations() {
    let css = r#"
        @keyframes spin { from {} to {} }
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
    "#;
    let kf = extract_keyframes(css);
    assert!(kf.contains_key("spin"));
    assert!(kf.contains_key("fade"));
}

#[test]
fn extract_keyframes_webkit_prefix() {
    let css = r#"
        @-webkit-keyframes slide {
            from { transform: translateX(-100px); }
            to   { transform: translateX(0); }
        }
    "#;
    let kf = extract_keyframes(css);
    assert!(kf.contains_key("slide"));
}

#[test]
fn extract_keyframes_ignores_regular_rules() {
    let css = r#"
        p { color: red; }
        @keyframes fade { from { opacity:0; } to { opacity:1; } }
        .box { margin: 0; }
    "#;
    let kf = extract_keyframes(css);
    assert_eq!(kf.len(), 1);
    assert!(kf.contains_key("fade"));
}

#[test]
fn stylesheet_parse_and_add_stores_keyframes() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        r#"
        @keyframes slide-in {
            from { transform: translateX(-200px); }
            to   { transform: translateX(0); }
        }
        .box { animation: slide-in 0.5s ease; }
    "#,
    );
    assert!(ss.keyframes.contains_key("slide-in"));
    assert_eq!(ss.keyframes["slide-in"].len(), 2);
}

#[test]
fn extract_keyframes_nested_in_layer_and_supported_supports() {
    let mut ss = Stylesheet::default();
    ss.parse_and_add(
        r#"
        @layer components {
            @keyframes layered { from { opacity: 0; } to { opacity: 1; } }
        }
        @supports (display: flex) {
            @keyframes supported { from { opacity: 0; } to { opacity: 1; } }
        }
        @supports (definitely-not-a-property: 1) {
            @keyframes unsupported { from { opacity: 0; } to { opacity: 1; } }
        }
    "#,
    );

    assert!(ss.keyframes.contains_key("layered"));
    assert!(ss.keyframes.contains_key("supported"));
    assert!(!ss.keyframes.contains_key("unsupported"));
}

#[test]
fn keyframe_declarations_ignore_important_properties() {
    let kf = extract_keyframes(
        r#"
        @keyframes fade {
            from { opacity: 0 !important; transform: translateX(0px); }
            to { opacity: 1; }
        }
    "#,
    );
    let stops = kf.get("fade").expect("keyframes extracted");
    let from = stops
        .iter()
        .find(|stop| stop.offset == 0.0)
        .expect("from stop");

    assert!(!from.properties.iter().any(|(name, _)| name == "opacity"));
    assert!(
        from.properties
            .iter()
            .any(|(name, value)| { name == "transform" && value == "translateX(0px)" })
    );
}

#[test]
fn keyframe_timing_function_is_not_an_animated_property() {
    let kf = extract_keyframes(
        r#"
        @keyframes bounce {
            0% { animation-timing-function: ease-in; top: 0px; }
            100% { top: 10px; }
        }
    "#,
    );
    let stops = kf.get("bounce").expect("keyframes extracted");
    assert_eq!(stops[0].timing_fn, Some(EasingFn::EaseIn));
    let from = stops
        .iter()
        .find(|stop| stop.offset == 0.0)
        .expect("from stop");

    assert!(
        from.properties
            .iter()
            .all(|(name, _)| name != "animation-timing-function")
    );
    assert!(
        from.properties
            .iter()
            .any(|(name, value)| name == "top" && value == "0px")
    );
}

#[test]
fn prefixed_keyframe_timing_is_metadata_not_layout_animation() {
    let frames = extract_keyframes(
        "@keyframes orbit { 0% { -webkit-animation-timing-function:ease-out; transform:rotate(0deg); opacity:0 } 100% { transform:rotate(360deg); opacity:1 } }",
    );
    let stops = frames.get("orbit").unwrap();
    assert_eq!(stops[0].timing_fn, Some(EasingFn::EaseOut));
    for stop in stops {
        assert!(
            !crate::types::animation_runtime::animation_properties_affect_layout(&stop.properties)
        );
        assert!(
            stop.properties
                .iter()
                .all(|(name, _)| !name.contains("timing-function"))
        );
    }
}

// ── Value interpolation ───────────────────────────────────────────────────────

#[test]
fn interpolate_value_midpoint_numeric() {
    // "0px" → "100px" at t=0.5 → "50px"
    let result = interpolate_value("0px", "100px", 0.5);
    assert!(result.contains("50"), "expected ~50px, got '{}'", result);
}

#[test]
fn interpolate_value_at_zero() {
    let result = interpolate_value("0px", "100px", 0.0);
    assert!(result.contains('0'), "expected 0px, got '{}'", result);
}

#[test]
fn interpolate_value_at_one() {
    let result = interpolate_value("0px", "100px", 1.0);
    assert!(result.contains("100"), "expected 100px, got '{}'", result);
}

#[test]
fn interpolate_transform_keeps_fractional_angles() {
    let result = interpolate_value("rotate(0deg)", "rotate(360deg)", 0.016);
    let angle = result
        .strip_prefix("rotate(")
        .and_then(|s| s.strip_suffix("deg)"))
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(f32::NAN);
    assert!(
        (angle - 5.76).abs() < 0.001,
        "expected rotate(5.76deg), got {result}"
    );
}

#[test]
fn interpolate_value_rgba_color() {
    // Fully transparent → fully opaque red
    let from = "rgba(255,0,0,0.0000)";
    let to = "rgba(255,0,0,1.0000)";
    let mid = interpolate_value(from, to, 0.5);
    assert!(mid.starts_with("rgba("), "expected rgba(), got '{}'", mid);
    // Red channel should stay 255, alpha should be ~0.5
    assert!(
        mid.contains("255,0,0"),
        "red channel should be 255, got '{}'",
        mid
    );
}

#[test]
fn interpolate_value_rgba_uses_premultiplied_colors() {
    let mid = interpolate_value("rgba(0,0,0,0.0000)", "rgba(255,0,0,1.0000)", 0.5);
    assert_eq!(
        mid, "rgba(255,0,0,0.5000)",
        "fade-in from transparent must keep the target hue instead of darkening through black"
    );
}

#[test]
fn interpolate_value_snaps_on_mismatch() {
    // Different token count → snap
    let result = interpolate_value("red", "blue", 0.3);
    assert_eq!(result, "red");
    let result2 = interpolate_value("red", "blue", 0.7);
    assert_eq!(result2, "blue");
}

#[test]
fn mixed_unit_translation_interpolation_preserves_reference_context() {
    use crate::types::TransformOp;
    for (from, to, font, reference, expected) in [
        ("1em", "20px", 30.0, 200.0, 25.0),
        ("10px", "50%", 30.0, 200.0, 55.0),
        ("10px", "50%", 30.0, 400.0, 105.0),
        ("1in", "48px", 30.0, 200.0, 72.0),
        ("-1em", "20px", 30.0, 200.0, -5.0),
    ] {
        let value = interpolate_value(
            &format!("translateX({from})"),
            &format!("translateX({to})"),
            0.5,
        );
        let transform = crate::css::parse_css_transform_checked(&value).expect(&value);
        let TransformOp::TranslateX(length) = &transform.ops[0] else {
            panic!("{value}")
        };
        assert!(
            (length.resolve(font, reference, font) - expected).abs() < 0.001,
            "{value}"
        );
    }
}

#[test]
fn mixed_angle_units_interpolate_in_degrees() {
    for (from, to, expected) in [
        ("0deg", "0.5turn", 90.0),
        ("0rad", "200grad", 90.0),
        ("180deg", "0turn", 90.0),
    ] {
        let value = interpolate_value(&format!("rotate({from})"), &format!("rotate({to})"), 0.5);
        let transform = crate::css::parse_css_transform_checked(&value).expect(&value);
        let crate::types::TransformOp::Rotate(angle) = transform.ops[0] else {
            panic!("{value}")
        };
        assert!((angle - expected).abs() < 0.001, "{value}");
    }
}

#[test]
fn transform_interpolation_unitless_zero_adopts_percentage_unit() {
    let result = interpolate_value("translateX(0)", "translateX(100%)", 0.5);
    assert_eq!(result, "translateX(50%)");
}

// ── @keyframes stop interpolation ─────────────────────────────────────────────

#[test]
fn interpolate_stops_at_start() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "1".into())],
        },
    ];
    let props = interpolate_keyframe_stops(&stops, 0.0);
    let op = props
        .iter()
        .find(|(k, _)| k == "opacity")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    assert!(
        op.parse::<f32>().unwrap_or(-1.0).abs() < 0.01,
        "expected ~0, got '{}'",
        op
    );
}

#[test]
fn interpolate_stops_at_end() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "1".into())],
        },
    ];
    let props = interpolate_keyframe_stops(&stops, 1.0);
    let op = props
        .iter()
        .find(|(k, _)| k == "opacity")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    assert!((op.parse::<f32>().unwrap_or(-1.0) - 1.0).abs() < 0.01);
}

#[test]
fn interpolate_stops_midpoint() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "1".into())],
        },
    ];
    let props = interpolate_keyframe_stops(&stops, 0.5);
    let op = props
        .iter()
        .find(|(k, _)| k == "opacity")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let v: f32 = op.parse().unwrap_or(-1.0);
    assert!((v - 0.5).abs() < 0.05, "expected ~0.5, got {}", v);
}

#[test]
fn visibility_keyframes_are_visible_between_endpoints() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("visibility".into(), "hidden".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("visibility".into(), "visible".into())],
        },
    ];
    let start = interpolate_keyframe_stops(&stops, 0.0);
    let mid = interpolate_keyframe_stops(&stops, 0.25);
    let end = interpolate_keyframe_stops(&stops, 1.0);

    assert_eq!(start[0].1, "hidden");
    assert_eq!(mid[0].1, "visible");
    assert_eq!(end[0].1, "visible");
}

#[test]
fn interpolate_stops_between_non_zero_stops() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 0.5,
            timing_fn: None,
            properties: vec![("opacity".into(), "0.5".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("opacity".into(), "1".into())],
        },
    ];
    // At t=0.25 (between stop 0 and stop 0.5), local_t = 0.5
    let props = interpolate_keyframe_stops(&stops, 0.25);
    let op = props
        .iter()
        .find(|(k, _)| k == "opacity")
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let v: f32 = op.parse().unwrap_or(-1.0);
    assert!((v - 0.25).abs() < 0.05, "expected ~0.25, got {}", v);
}

#[test]
fn keyframe_properties_present_only_later_do_not_disappear() {
    let stops = vec![
        KeyframeStop {
            offset: 0.0,
            timing_fn: None,
            properties: vec![("left".into(), "0px".into())],
        },
        KeyframeStop {
            offset: 0.5,
            timing_fn: None,
            properties: vec![("top".into(), "10px".into())],
        },
        KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties: vec![("left".into(), "100px".into())],
        },
    ];

    let first_half = interpolate_keyframe_stops(&stops, 0.25);
    assert!(first_half.iter().any(|(name, _)| name == "top"));
    assert!(first_half.iter().any(|(name, _)| name == "left"));

    let second_half = interpolate_keyframe_stops(&stops, 0.75);
    assert!(second_half.iter().any(|(name, _)| name == "top"));
    assert!(second_half.iter().any(|(name, _)| name == "left"));
}

// ── apply_property: animation / transition sub-properties ────────────────────

#[test]
fn style_animation_shorthand_parsed() {
    let s = style_with("animation", "spin 2s linear infinite");
    assert_eq!(s.rare().animations.len(), 1);
    assert_eq!(s.rare().animations[0].name, "spin");
    assert!((s.rare().animations[0].duration_ms - 2000.0).abs() < 1.0);
    assert!(s.rare().animations[0].iteration_count.is_infinite());
}

#[test]
fn style_animation_sub_properties() {
    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "animation-name", "fade");
    crate::css::apply_property(&mut s, "animation-duration", "0.5s");
    crate::css::apply_property(&mut s, "animation-timing-function", "ease-out");
    crate::css::apply_property(&mut s, "animation-iteration-count", "3");
    crate::css::apply_property(&mut s, "animation-direction", "alternate");
    crate::css::apply_property(&mut s, "animation-fill-mode", "both");
    assert_eq!(s.rare().animations[0].name, "fade");
    assert!((s.rare().animations[0].duration_ms - 500.0).abs() < 1.0);
    assert_eq!(s.rare().animations[0].timing_fn, EasingFn::EaseOut);
    assert!((s.rare().animations[0].iteration_count - 3.0).abs() < 0.01);
    assert_eq!(s.rare().animations[0].direction, AnimDirection::Alternate);
    assert_eq!(s.rare().animations[0].fill_mode, FillMode::Both);
}

#[test]
fn style_transition_shorthand_parsed() {
    let s = style_with("transition", "opacity 0.3s ease-in-out");
    assert_eq!(s.rare().transitions.len(), 1);
    assert_eq!(s.rare().transitions[0].property, "opacity");
    assert!((s.rare().transitions[0].duration_ms - 300.0).abs() < 1.0);
    assert_eq!(s.rare().transitions[0].timing_fn, EasingFn::EaseInOut);
}

#[test]
fn transition_duration_without_property_defaults_to_all() {
    let s = style_with("transition", "0.3s ease-out");
    assert_eq!(s.rare().transitions.len(), 1);
    assert_eq!(s.rare().transitions[0].property, "all");
    assert!((s.rare().transitions[0].duration_ms - 300.0).abs() < 1.0);

    let mut sub = ComputedStyle::default();
    crate::css::apply_property(&mut sub, "transition-duration", "200ms");
    assert_eq!(sub.rare().transitions[0].property, "all");
}

#[test]
fn transition_behavior_allow_discrete_is_parsed_and_stored() {
    let s = style_with("transition", "display 1s allow-discrete");
    assert_eq!(s.rare().transitions.len(), 1);
    assert_eq!(s.rare().transitions[0].property, "display");
    assert!(s.rare().transitions[0].allow_discrete);

    let mut sub = ComputedStyle::default();
    crate::css::apply_property(&mut sub, "transition-behavior", "allow-discrete");
    assert!(sub.rare().transitions[0].allow_discrete);
    crate::css::apply_property(&mut sub, "transition-behavior", "normal");
    assert!(!sub.rare().transitions[0].allow_discrete);
}

#[test]
fn transition_longhands_match_comma_lists_by_index() {
    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "transition-property", "opacity, transform");
    crate::css::apply_property(&mut s, "transition-duration", "200ms, .5s");
    crate::css::apply_property(
        &mut s,
        "transition-timing-function",
        "ease-in, cubic-bezier(.4, 0, .2, 1)",
    );
    crate::css::apply_property(&mut s, "transition-delay", "50ms, 100ms");
    crate::css::apply_property(&mut s, "transition-behavior", "normal, allow-discrete");

    let transitions = &s.rare().transitions;
    assert_eq!(transitions.len(), 2);
    assert_eq!(transitions[0].property, "opacity");
    assert_eq!(transitions[1].property, "transform");
    assert!((transitions[0].duration_ms - 200.0).abs() < 0.1);
    assert!((transitions[1].duration_ms - 500.0).abs() < 0.1);
    assert_eq!(transitions[0].timing_fn, EasingFn::EaseIn);
    assert_eq!(
        transitions[1].timing_fn,
        EasingFn::CubicBezier(0.4, 0.0, 0.2, 1.0)
    );
    assert!((transitions[0].delay_ms - 50.0).abs() < 0.1);
    assert!((transitions[1].delay_ms - 100.0).abs() < 0.1);
    assert!(!transitions[0].allow_discrete);
    assert!(transitions[1].allow_discrete);
}

#[test]
fn animation_longhands_match_comma_lists_by_index() {
    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "animation-name", "fade, spin");
    crate::css::apply_property(&mut s, "animation-duration", "200ms, .5s");
    crate::css::apply_property(
        &mut s,
        "animation-timing-function",
        "ease-out, cubic-bezier(.4, 0, .2, 1)",
    );
    crate::css::apply_property(&mut s, "animation-delay", "50ms, 100ms");
    crate::css::apply_property(&mut s, "animation-iteration-count", "2, infinite");
    crate::css::apply_property(&mut s, "animation-direction", "reverse, alternate");
    crate::css::apply_property(&mut s, "animation-fill-mode", "forwards, both");
    crate::css::apply_property(&mut s, "animation-play-state", "running, paused");
    crate::css::apply_property(&mut s, "animation-composition", "add, accumulate");

    let animations = &s.rare().animations;
    assert_eq!(animations.len(), 2);
    assert_eq!(animations[0].name, "fade");
    assert_eq!(animations[1].name, "spin");
    assert!((animations[0].duration_ms - 200.0).abs() < 0.1);
    assert!((animations[1].duration_ms - 500.0).abs() < 0.1);
    assert_eq!(animations[0].timing_fn, EasingFn::EaseOut);
    assert_eq!(
        animations[1].timing_fn,
        EasingFn::CubicBezier(0.4, 0.0, 0.2, 1.0)
    );
    assert!((animations[0].delay_ms - 50.0).abs() < 0.1);
    assert!((animations[1].delay_ms - 100.0).abs() < 0.1);
    assert_eq!(animations[0].iteration_count, 2.0);
    assert!(animations[1].iteration_count.is_infinite());
    assert_eq!(animations[0].direction, AnimDirection::Reverse);
    assert_eq!(animations[1].direction, AnimDirection::Alternate);
    assert_eq!(animations[0].fill_mode, FillMode::Forwards);
    assert_eq!(animations[1].fill_mode, FillMode::Both);
    assert!(!animations[0].play_state_paused);
    assert!(animations[1].play_state_paused);
    assert_eq!(animations[0].composition, AnimationComposition::Add);
    assert_eq!(animations[1].composition, AnimationComposition::Accumulate);
}

#[test]
fn transform_interpolation_synthesizes_multi_function_identity() {
    let value = crate::types::animation_helpers::interpolate_value(
        "none",
        "translate(-50%, -50%) rotate(45deg)",
        0.0,
    );
    assert_eq!(value, "translate(0%, 0%) rotate(0deg)");

    let value =
        crate::types::animation_helpers::interpolate_value("none", "scale(1.5) rotate(45deg)", 0.0);
    assert_eq!(value, "scale(1) rotate(0deg)");
}

#[test]
fn transitionable_style_includes_common_length_and_side_color_properties() {
    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "width", "10px");
    crate::css::apply_property(&mut s, "left", "25%");
    crate::css::apply_property(&mut s, "border-left-color", "red");
    crate::css::apply_property(&mut s, "gap", "2em");
    crate::css::apply_property(&mut s, "display", "grid");
    crate::css::apply_property(&mut s, "content-visibility", "hidden");
    crate::css::apply_property(&mut s, "pointer-events", "none");
    crate::css::apply_property(&mut s, "cursor", "grab");
    crate::css::apply_property(&mut s, "user-select", "all");
    crate::css::apply_property(&mut s, "resize", "vertical");
    crate::css::apply_property(&mut s, "filter", "blur(4px)");
    crate::css::apply_property(&mut s, "backdrop-filter", "brightness(0.5)");
    crate::css::apply_property(&mut s, "box-shadow", "inset 1px 2px 3px 4px red");
    crate::css::apply_property(&mut s, "text-shadow", "5px 6px 7px blue");
    crate::css::apply_property(&mut s, "border-top-left-radius", "10px 20px");
    crate::css::apply_property(&mut s, "border-bottom-right-radius", "25%");
    crate::css::apply_property(&mut s, "outline-width", "3px");
    crate::css::apply_property(&mut s, "outline-offset", "4px");
    crate::css::apply_property(&mut s, "outline-style", "dashed");
    crate::css::apply_property(&mut s, "outline-color", "green");
    crate::css::apply_property(&mut s, "text-decoration-style", "wavy");
    crate::css::apply_property(&mut s, "text-decoration-color", "purple");
    crate::css::apply_property(&mut s, "text-decoration-thickness", "2px");
    crate::css::apply_property(&mut s, "caret-color", "orange");
    crate::css::apply_property(&mut s, "column-rule-width", "5px");
    crate::css::apply_property(&mut s, "column-rule-style", "double");
    crate::css::apply_property(&mut s, "column-rule-color", "navy");

    let values = extract_transitionable_style(&s);
    assert_eq!(values.get("width").map(String::as_str), Some("10px"));
    assert_eq!(values.get("left").map(String::as_str), Some("25%"));
    assert_eq!(values.get("gap").map(String::as_str), Some("2em"));
    assert_eq!(values.get("display").map(String::as_str), Some("grid"));
    assert_eq!(
        values.get("content-visibility").map(String::as_str),
        Some("hidden")
    );
    assert_eq!(
        values.get("pointer-events").map(String::as_str),
        Some("none")
    );
    assert_eq!(values.get("cursor").map(String::as_str), Some("grab"));
    assert_eq!(values.get("user-select").map(String::as_str), Some("all"));
    assert_eq!(values.get("resize").map(String::as_str), Some("vertical"));
    assert_eq!(values.get("filter").map(String::as_str), Some("blur(4px)"));
    assert_eq!(
        values.get("backdrop-filter").map(String::as_str),
        Some("brightness(0.5)")
    );
    assert_eq!(
        values.get("box-shadow").map(String::as_str),
        Some("inset 1px 2px 3px 4px rgba(255,0,0,1.0000)")
    );
    assert_eq!(
        values.get("text-shadow").map(String::as_str),
        Some("5px 6px 7px rgba(0,0,255,1.0000)")
    );
    assert_eq!(
        values.get("border-top-left-radius").map(String::as_str),
        Some("10px 20px")
    );
    assert_eq!(
        values.get("border-bottom-right-radius").map(String::as_str),
        Some("25%")
    );
    assert_eq!(values.get("outline-width").map(String::as_str), Some("3px"));
    assert_eq!(
        values.get("outline-style").map(String::as_str),
        Some("dashed")
    );
    assert_eq!(
        values.get("outline-color").map(String::as_str),
        Some("rgba(0,128,0,1.0000)")
    );
    assert_eq!(
        values.get("outline-offset").map(String::as_str),
        Some("4px")
    );
    assert_eq!(
        values.get("text-decoration-style").map(String::as_str),
        Some("wavy")
    );
    assert_eq!(
        values.get("text-decoration-color").map(String::as_str),
        Some("rgba(128,0,128,1.0000)")
    );
    assert_eq!(
        values.get("text-decoration-thickness").map(String::as_str),
        Some("2px")
    );
    assert_eq!(
        values.get("caret-color").map(String::as_str),
        Some("rgba(255,165,0,1.0000)")
    );
    assert_eq!(
        values.get("column-rule-width").map(String::as_str),
        Some("5px")
    );
    assert_eq!(
        values.get("column-rule-style").map(String::as_str),
        Some("double")
    );
    assert_eq!(
        values.get("column-rule-color").map(String::as_str),
        Some("rgba(0,0,128,1.0000)")
    );
    assert!(values.contains_key("border-left-color"));
}

#[test]
fn style_transition_sub_properties() {
    let mut s = ComputedStyle::default();
    crate::css::apply_property(&mut s, "transition-property", "color");
    crate::css::apply_property(&mut s, "transition-duration", "200ms");
    crate::css::apply_property(&mut s, "transition-timing-function", "linear");
    crate::css::apply_property(&mut s, "transition-delay", "50ms");
    assert_eq!(s.rare().transitions[0].property, "color");
    assert!((s.rare().transitions[0].duration_ms - 200.0).abs() < 1.0);
    assert_eq!(s.rare().transitions[0].timing_fn, EasingFn::Linear);
    assert!((s.rare().transitions[0].delay_ms - 50.0).abs() < 1.0);
}

// ── Document::sync_animations ─────────────────────────────────────────────────

fn doc_with_animation(anim_css: &str) -> Document {
    let html = format!(
        r#"<html><head><style>
            @keyframes spin {{
                from {{ transform: rotate(0deg); }}
                to   {{ transform: rotate(360deg); }}
            }}
            @keyframes fade {{
                from {{ opacity: 0; }}
                to   {{ opacity: 1; }}
            }}
            .box {{ {} }}
        </style></head><body><div class="box">hi</div></body></html>"#,
        anim_css
    );
    let mut doc = parse_html(&html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    doc
}

type CssEventLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, f64)>>>;

fn record_css_events(doc: &mut Document, id: u32, names: &[&str]) -> CssEventLog {
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for name in names {
        let events = std::sync::Arc::clone(&events);
        doc.add_event_listener(
            id,
            *name,
            Box::new(move |event, _| {
                assert!(event.bubbles);
                assert!(!event.cancelable);
                assert!(!event.composed);
                assert!(event.pseudo_element.is_empty());
                event.prevent_default();
                assert!(!event.default_prevented());
                events.lock().unwrap().push((
                    event.event_type.clone(),
                    event.animation_name.clone(),
                    event.property_name.clone(),
                    event.elapsed_time,
                ));
            }),
            Default::default(),
        );
    }
    events
}

#[test]
fn animation_events_report_active_seconds_and_skip_negative_delay_history() {
    let mut doc = doc_with_animation("animation: fade 2s linear -3.5s 3;");
    let id = doc.active_animations[0].element_id;
    doc.active_animations.clear();
    let events = record_css_events(
        &mut doc,
        id,
        &["animationstart", "animationiteration", "animationend"],
    );
    let start = Instant::now();
    doc.sync_animations(start);
    doc.tick_animations(start + Duration::from_millis(500));
    doc.tick_animations(start + Duration::from_secs(3));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("animationstart".into(), "fade".into(), String::new(), 3.5),
            (
                "animationiteration".into(),
                "fade".into(),
                String::new(),
                4.0
            ),
            ("animationend".into(), "fade".into(), String::new(), 6.0),
        ]
    );
}

#[test]
fn animation_cancel_excludes_delay_and_paused_time() {
    for (delay, expected) in [(1000.0, 0.0), (-250.0, 0.65)] {
        let mut doc = doc_with_animation(&format!("animation: fade 2s linear {delay}ms;"));
        let id = doc.active_animations[0].element_id;
        let start = doc.active_animations[0].start_time;
        let events = record_css_events(&mut doc, id, &["animationcancel", "animationend"]);
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style)
            .rare_mut()
            .animations[0]
            .play_state_paused = true;
        doc.sync_animations(start + Duration::from_millis(400));
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style)
            .rare_mut()
            .animations
            .clear();
        doc.sync_animations(start + Duration::from_secs(10));
        doc.sync_animations(start + Duration::from_secs(11));
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "animationcancel");
        assert_eq!(events[0].1, "fade");
        assert!((events[0].3 - expected).abs() < 0.00001);
    }
}

#[test]
fn zero_duration_animation_reports_start_and_end_after_its_delay() {
    let mut doc = doc_with_animation("animation: fade 0s linear 100ms;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    let events = record_css_events(&mut doc, id, &["animationstart", "animationend"]);
    doc.tick_animations(start + Duration::from_millis(99));
    assert!(events.lock().unwrap().is_empty());
    doc.tick_animations(start + Duration::from_millis(100));
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("animationstart".into(), "fade".into(), String::new(), 0.0),
            ("animationend".into(), "fade".into(), String::new(), 0.0),
        ]
    );
}

#[test]
fn transition_events_identify_each_property_and_exclude_delays() {
    let mut doc = parse_html(
        "<div id='box' style='opacity:0; margin-left:0; transition:opacity 1s linear -250ms, margin-left 2s linear 100ms'></div>",
    );
    let id = doc.get_element_by_id("box").unwrap();
    LayoutEngine::new().layout(&mut doc, 800.0);
    let events = record_css_events(
        &mut doc,
        id,
        &["transitionrun", "transitionstart", "transitioncancel"],
    );
    {
        let style = std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style);
        crate::css::apply_property(style, "opacity", "1");
        crate::css::apply_property(style, "margin-left", "10px");
    }
    let start = Instant::now();
    doc.sync_transitions(start);
    doc.tick_animations(start + Duration::from_millis(150));
    std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style)
        .rare_mut()
        .transitions
        .clear();
    doc.sync_transitions(start + Duration::from_millis(200));
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 6);
    for (event, name, property, elapsed) in events.iter() {
        assert!(name.is_empty());
        let expected = match (event.as_str(), property.as_str()) {
            ("transitionrun" | "transitionstart", "opacity") => 0.25,
            ("transitionrun" | "transitionstart", "margin-left") => 0.0,
            ("transitioncancel", "opacity") => 0.45,
            ("transitioncancel", "margin-left") => 0.1,
            _ => panic!("unexpected event: {event} {property}"),
        };
        assert!(
            (elapsed - expected).abs() < 0.00001,
            "{event} {property}: {elapsed}"
        );
    }
}

#[test]
fn transition_with_exhausted_negative_delay_does_not_start() {
    let mut doc =
        parse_html("<div id='box' style='opacity:0; transition:opacity 1s linear -2s'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    LayoutEngine::new().layout(&mut doc, 800.0);
    let events = record_css_events(
        &mut doc,
        id,
        &["transitionrun", "transitionstart", "transitionend"],
    );
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "opacity",
        "1",
    );
    let start = Instant::now();
    doc.sync_transitions(start);
    doc.tick_animations(start);
    assert!(events.lock().unwrap().is_empty());
    assert!(doc.transition_states.is_empty());
}

#[test]
fn animation_negative_delay_start_payload_is_capped_to_active_duration() {
    let mut doc = doc_with_animation("animation: fade 1s linear 1.5;");
    let id = doc.active_animations[0].element_id;
    std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style)
        .rare_mut()
        .animations[0]
        .delay_ms = -3000.0;
    doc.active_animations.clear();
    let events = record_css_events(&mut doc, id, &["animationstart", "animationend"]);
    let start = Instant::now();
    doc.sync_animations(start);
    doc.tick_animations(start);
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("animationstart".into(), "fade".into(), String::new(), 1.5),
            ("animationend".into(), "fade".into(), String::new(), 1.5),
        ]
    );
}

#[test]
fn zero_duration_transition_waits_for_positive_delay_and_reports_zero_seconds() {
    let mut doc =
        parse_html("<div id='box' style='opacity:0; transition:opacity 0s linear 100ms'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    LayoutEngine::new().layout(&mut doc, 800.0);
    let events = record_css_events(
        &mut doc,
        id,
        &["transitionrun", "transitionstart", "transitionend"],
    );
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "opacity",
        "1",
    );
    let start = Instant::now();
    doc.sync_transitions(start);
    doc.tick_animations(start + Duration::from_millis(99));
    assert_eq!(events.lock().unwrap().len(), 1);
    assert_eq!(doc.animation_overrides[&id][0].1, "0");
    doc.tick_animations(start + Duration::from_millis(100));
    assert_eq!(doc.animation_overrides[&id][0].1, "1");
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            ("transitionrun".into(), String::new(), "opacity".into(), 0.0),
            (
                "transitionstart".into(),
                String::new(),
                "opacity".into(),
                0.0
            ),
            ("transitionend".into(), String::new(), "opacity".into(), 0.0),
        ]
    );
}

#[test]
fn sync_animations_starts_state() {
    let doc = doc_with_animation("animation: spin 1s linear infinite;");
    assert!(
        !doc.active_animations.is_empty(),
        "should have at least one active animation"
    );
    assert_eq!(doc.active_animations[0].animation.name, "spin");
}

#[test]
fn sync_animations_ignores_missing_keyframes() {
    let doc = doc_with_animation("animation: missing-keyframes 60s linear infinite;");
    assert!(
        doc.active_animations.is_empty(),
        "animation-name without a matching @keyframes block should not create a running animation"
    );
}

#[test]
fn sync_animations_dispatches_animationstart_when_animation_is_created() {
    let html = r#"<html><head><style>
        @keyframes spin {
            from { transform: rotate(0deg); }
            to   { transform: rotate(360deg); }
        }
        #box { animation: spin 1s linear 1; }
    </style></head><body><div id="box">hi</div></body></html>"#;

    let mut doc = parse_html(html);
    let id = doc.get_element_by_id("box").unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = std::sync::Arc::clone(&seen);
    doc.add_event_listener(
        id,
        "animationstart",
        Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    assert_eq!(*seen.lock().unwrap(), 1);
    assert_eq!(doc.active_animations[0].animation.name, "spin");
}

#[test]
fn delayed_animation_dispatches_start_once_when_delay_expires() {
    for final_tick_ms in [100, 1200] {
        let mut doc = doc_with_animation("animation: fade 0.2s linear 0.1s 1;");
        let id = doc.active_animations[0].element_id;
        let start = doc.active_animations[0].start_time;
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        for name in ["animationstart", "animationend"] {
            let events = std::sync::Arc::clone(&events);
            doc.add_event_listener(
                id,
                name,
                Box::new(move |event, _| {
                    events.lock().unwrap().push(event.event_type.clone());
                }),
                Default::default(),
            );
        }
        doc.tick_animations(start + Duration::from_millis(99));
        assert!(events.lock().unwrap().is_empty());
        doc.tick_animations(start + Duration::from_millis(final_tick_ms));
        doc.tick_animations(start + Duration::from_millis(final_tick_ms + 1));
        let names = events.lock().unwrap().clone();
        assert_eq!(names.first().map(String::as_str), Some("animationstart"));
        assert_eq!(
            names
                .iter()
                .filter(|name| *name == "animationstart")
                .count(),
            1
        );
        assert_eq!(
            names.contains(&"animationend".to_string()),
            final_tick_ms > 300
        );
    }
}

#[test]
fn sync_animations_does_not_duplicate() {
    let mut doc = doc_with_animation("animation: spin 2s linear infinite;");
    let count_before = doc.active_animations.len();
    // Call layout again — should not add another AnimState for the same element+name.
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    assert_eq!(doc.active_animations.len(), count_before);
}

#[test]
fn repeated_animation_names_match_from_the_end_without_merging_instances() {
    let mut doc = doc_with_animation("animation: spin 1s linear;");
    let id = doc.active_animations[0].element_id;
    let original_start = doc.active_animations[0].start_time;
    let events = record_css_events(&mut doc, id, &["animationstart", "animationcancel"]);
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "animation",
        "spin 2s linear, spin 3s linear",
    );
    let update_time = original_start + Duration::from_millis(400);
    doc.sync_animations(update_time);
    assert_eq!(doc.active_animations.len(), 2);
    assert_eq!(doc.active_animations[0].start_time, update_time);
    assert_eq!(doc.active_animations[1].start_time, original_start);
    assert_eq!(doc.active_animations[0].animation.duration_ms, 2000.0);
    assert_eq!(doc.active_animations[1].animation.duration_ms, 3000.0);
    doc.sync_animations(original_start + Duration::from_millis(500));
    assert_eq!(doc.active_animations.len(), 2);
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "animation",
        "spin 4s linear",
    );
    doc.sync_animations(original_start + Duration::from_millis(600));
    assert_eq!(doc.active_animations.len(), 1);
    assert_eq!(doc.active_animations[0].start_time, original_start);
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, "animationstart");
    assert_eq!(events[1].0, "animationcancel");
    assert!((events[1].3 - 0.2).abs() < 0.00001);
}

#[test]
fn reordered_animation_list_preserves_clocks_and_updates_effect_order() {
    let mut doc = parse_html(
        r#"<style>
        @keyframes first { from { transform:translateX(0px); } to { transform:translateX(100px); } }
        @keyframes second { from { transform:translateX(0px); } to { transform:translateX(200px); } }
        #box { animation:first 1s linear -.5s paused, second 1s linear -.5s paused; }
        </style><div id='box'></div>"#,
    );
    LayoutEngine::new().layout(&mut doc, 800.0);
    let id = doc.get_element_by_id("box").unwrap();
    let first_start = doc.active_animations[0].start_time;
    let second_start = doc.active_animations[1].start_time;
    doc.tick_animations(first_start);
    assert_eq!(
        doc.animation_overrides[&id],
        [("transform".into(), "translateX(100px)".into())]
    );
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "animation",
        "second 1s linear -.5s paused, first 1s linear -.5s paused",
    );
    doc.sync_animations(first_start + Duration::from_millis(100));
    assert_eq!(doc.active_animations[0].animation.name, "second");
    assert_eq!(doc.active_animations[0].start_time, second_start);
    assert_eq!(doc.active_animations[1].animation.name, "first");
    assert_eq!(doc.active_animations[1].start_time, first_start);
    doc.tick_animations(first_start + Duration::from_millis(100));
    assert_eq!(
        doc.animation_overrides[&id],
        [("transform".into(), "translateX(50px)".into())]
    );
}

#[test]
fn sync_animations_removes_when_gone() {
    // Start with animation, then remove it (simulate by re-parsing without it).
    let doc_with = doc_with_animation("animation: spin 1s;");
    assert!(!doc_with.active_animations.is_empty());

    let doc_without = doc_with_animation("/* no animation */");
    assert!(doc_without.active_animations.is_empty());
}

// ── Document::tick_animations ─────────────────────────────────────────────────

#[test]
fn tick_animations_produces_overrides() {
    let mut doc = doc_with_animation("animation: spin 1s linear infinite;");
    // Advance by 500ms (half-way through)
    let now = doc.active_animations[0].start_time + Duration::from_millis(500);
    doc.tick_animations(now);
    // The 'transform' property should have an override for the animated element.
    let has_transform_override = doc
        .animation_overrides
        .values()
        .any(|props| props.iter().any(|(k, _)| k == "transform"));
    assert!(
        has_transform_override,
        "expected a transform override at t=0.5"
    );
}

#[test]
fn keyframe_transform_resolves_element_variables_and_mixed_units() {
    let html = r#"<style>
        @keyframes shine {
            0% { transform: translateX(calc(var(--direction) * var(--width))); }
            40%, 100% { transform: translateX(calc(-1 * var(--direction) * 100%)); }
        }
        #box { --direction: -1; --width: 8rem; width: 800px; height: 40px;
               animation: shine 1s linear infinite; }
    </style><div id="box"></div>"#;
    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 1000.0);
    let box_id = doc.query_selector("#box").unwrap();
    let start = doc.active_animations[0].start_time;
    doc.tick_animations(start + Duration::from_millis(200));
    let transform = doc
        .animation_overrides_for(box_id)
        .unwrap()
        .iter()
        .find(|(name, _)| name == "transform")
        .unwrap()
        .1
        .as_str();
    let translate_x: f32 = transform
        .strip_prefix("matrix(")
        .unwrap()
        .trim_end_matches(')')
        .split(',')
        .nth(4)
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (translate_x - 336.0).abs() < 1.0,
        "expected midpoint from -128px to +800px, got {transform}"
    );
}

#[test]
fn keyframe_segments_use_local_easing_and_independent_property_tracks() {
    fn sample(frames: &str, animation: &str, millis: u64, property: &str) -> f32 {
        let html = format!(
            "<style>@keyframes probe{{{frames}}} #box{{position:relative;left:0px;top:0px;opacity:0;animation:probe 1s {animation}}}</style><div id=box>sample</div>"
        );
        let mut doc = parse_html(&html);
        LayoutEngine::new().layout(&mut doc, 800.0);
        let id = doc.query_selector("#box").unwrap();
        let start = doc.active_animations[0].start_time;
        doc.tick_animations(start + Duration::from_millis(millis));
        doc.animation_overrides_for(id)
            .unwrap()
            .iter()
            .find(|(name, _)| name == property)
            .unwrap()
            .1
            .trim_end_matches("px")
            .parse()
            .unwrap()
    }
    let frames = "0%{opacity:0}50%{opacity:.4}100%{opacity:1}";
    assert!((sample(frames, "steps(2,end) both", 250, "opacity") - 0.2).abs() < 0.001);
    assert!((sample(frames, "steps(2,end) both", 750, "opacity") - 0.7).abs() < 0.001);
    let local = "0%{opacity:0;animation-timing-function:step-start}50%{opacity:.4;animation-timing-function:linear}100%{opacity:1;animation-timing-function:step-end}";
    assert!((sample(local, "step-end both", 250, "opacity") - 0.4).abs() < 0.001);
    assert!((sample(local, "step-end both", 750, "opacity") - 0.7).abs() < 0.001);
    assert!((sample(local, "step-end reverse both", 250, "opacity") - 0.7).abs() < 0.001);
    assert!((sample(local, "step-end 1.75 both", 2000, "opacity") - 0.7).abs() < 0.001);
    let sparse = "0%{left:0px}50%{top:10px}100%{left:100px}";
    for millis in [250, 750] {
        assert!(
            (sample(sparse, "linear both", millis, "left") - millis as f32 / 10.0).abs() < 0.001
        );
        assert!((sample(sparse, "linear both", millis, "top") - 5.0).abs() < 0.001);
    }
    assert!((sample(local, "step-end .5s backwards", 250, "opacity") - 0.0).abs() < 0.001);
    assert!((sample(local, "step-end .5s reverse backwards", 250, "opacity") - 1.0).abs() < 0.001);
    assert!((sample(sparse, "linear .5s backwards", 250, "top") - 0.0).abs() < 0.001);
}

#[test]
fn keyframe_easing_merges_duplicate_offsets_and_ignores_invalid_descriptors() {
    for invalid in [
        "bogus",
        "steps(0,end)",
        "steps(1,jump-none)",
        "steps(2,other)",
        "cubic-bezier(2,0,1,1)",
        "linear(0)",
    ] {
        let frames = extract_keyframes(&format!(
            "@keyframes probe{{0%{{opacity:0;animation-timing-function:step-start}}0%{{left:0px;animation-timing-function:{invalid}}}100%{{opacity:1}}}}"
        ));
        let stop = &frames["probe"][0];
        assert_eq!(stop.timing_fn, Some(EasingFn::StepStart), "{invalid}");
        assert_eq!(stop.properties.len(), 2);
    }
    let frames = extract_keyframes(
        "@keyframes probe{0%,50%{opacity:0;animation-timing-function:ease-in}50%{animation-timing-function:linear}100%{opacity:1}}",
    );
    assert_eq!(frames["probe"][0].timing_fn, Some(EasingFn::EaseIn));
    assert_eq!(frames["probe"][1].timing_fn, Some(EasingFn::Linear));
}

#[test]
fn one_sided_to_keyframe_starts_from_underlying_style() {
    let html = r#"<html><head><style>
        @keyframes fade-out { to { opacity: 0; } }
        .box { opacity: 1; animation: fade-out 1s linear; }
    </style></head><body><div class="box">hi</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    let start = doc.active_animations[0].start_time;
    doc.tick_animations(start + Duration::from_millis(500));

    let opacity = doc
        .animation_overrides
        .values()
        .flat_map(|props| props.iter())
        .find(|(name, _)| name == "opacity")
        .and_then(|(_, value)| value.parse::<f32>().ok())
        .expect("opacity override");
    assert!(
        (opacity - 0.5).abs() < 0.05,
        "to-only keyframe should interpolate from underlying opacity 1 to 0, got {opacity}"
    );
}

#[test]
fn animation_composition_add_composes_opacity_with_underlying_style() {
    let html = r#"<html><head><style>
        @keyframes alpha-effect {
            from { opacity: 0; }
            to { opacity: .4; }
        }
        .box {
            opacity: .25;
            animation: alpha-effect 1s linear both;
            animation-composition: add;
        }
    </style></head><body><div class="box">hi</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    doc.tick_animations(start + Duration::from_millis(500));

    let opacity = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(name, _)| name == "opacity"))
        .and_then(|(_, value)| value.parse::<f32>().ok())
        .expect("opacity override");
    assert!(
        (opacity - 0.45).abs() < 0.05,
        "additive opacity composition should add sampled .2 effect to .25 base, got {opacity}"
    );
}

#[test]
fn additive_animations_stack_transforms_and_opacity_in_css_list_order() {
    let mut doc = parse_html(
        r#"<style>
        @keyframes move-x { from { transform:translateX(0%); } to { transform:translateX(100%); } }
        @keyframes move-y { from { transform:translateY(0px); } to { transform:translateY(10px); } }
        @keyframes fade-a { from { opacity:0; } to { opacity:.4; } }
        @keyframes fade-b { from { opacity:0; } to { opacity:.2; } }
        #box { width:80px; height:40px; transform:rotate(90deg); opacity:.25;
            animation:move-x 1s linear -.5s paused, move-y 1s linear -.5s paused,
                fade-a 1s linear -.5s paused, fade-b 1s linear -.5s paused;
            animation-composition:add; }
        </style><div id='box'></div>"#,
    );
    LayoutEngine::new().layout(&mut doc, 800.0);
    let id = doc.get_element_by_id("box").unwrap();
    doc.tick_animations(Instant::now());
    let restore =
        crate::css::apply_animation_overrides_scoped(&mut doc.root, &doc.animation_overrides);
    let node = doc.get_box_by_id(id).unwrap();
    assert!((node.style.opacity - 0.55).abs() < 0.0001);
    let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
        &node.style,
        80.0,
        40.0,
        &TransformCtx::default(),
    );
    // The base rotation acts on both added translations; swapping list order
    // would incorrectly translate in unrotated viewport coordinates.
    for (actual, expected) in matrix.iter().zip([0.0, 1.0, -1.0, 0.0, -5.0, 40.0]) {
        assert!((actual - expected).abs() < 0.0001, "{matrix:?}");
    }
    crate::css::restore_animation_overrides(&mut doc.root, restore);
    assert!((doc.get_box_by_id(id).unwrap().style.opacity - 0.25).abs() < 0.0001);
    assert!(!doc.needs_animation_frame);
}

#[test]
fn replacing_transform_animation_discards_preceding_additive_effects() {
    let mut doc = parse_html(
        r#"<style>
        @keyframes first { from { transform:translateX(0px); } to { transform:translateX(100px); } }
        @keyframes second { from { transform:translateY(0px); } to { transform:translateY(20px); } }
        #box { transform:translateX(25px); animation:first 1s linear -.5s paused,
            second 1s linear -.5s paused; animation-composition:add, replace; }
        </style><div id='box'></div>"#,
    );
    LayoutEngine::new().layout(&mut doc, 800.0);
    let id = doc.get_element_by_id("box").unwrap();
    doc.tick_animations(Instant::now());
    let restore =
        crate::css::apply_animation_overrides_scoped(&mut doc.root, &doc.animation_overrides);
    let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
        &doc.get_box_by_id(id).unwrap().style,
        80.0,
        40.0,
        &TransformCtx::default(),
    );
    assert!(matrix[4].abs() < 0.0001, "{matrix:?}");
    assert!((matrix[5] - 10.0).abs() < 0.0001, "{matrix:?}");
    crate::css::restore_animation_overrides(&mut doc.root, restore);
}

#[test]
fn svg_fill_keyframes_synthesize_underlying_style() {
    let mut doc = parse_html(
        r#"<html><head><style>
            @keyframes icon-fill { to { fill: rgb(0, 0, 255); } }
            svg { fill: rgb(255, 0, 0); animation: icon-fill 1s linear; }
        </style></head><body>
            <svg style="width:20px;height:20px" viewBox="0 0 20 20">
              <rect width="20" height="20"/>
            </svg>
        </body></html>"#,
    );
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let start = doc.active_animations[0].start_time;
    doc.tick_animations(start + Duration::from_millis(500));
    let fill = doc
        .animation_overrides
        .values()
        .flat_map(|props| props.iter())
        .find(|(prop, _)| prop == "fill")
        .map(|(_, value)| value.as_str())
        .expect("SVG fill animation should produce an override");

    assert!(
        fill.starts_with("rgba(128,0,128"),
        "fill should interpolate from underlying red to keyframe blue, got {fill}"
    );
}

#[test]
fn tick_animations_needs_more_frames_while_running() {
    let mut doc = doc_with_animation("animation: spin 1s linear infinite;");
    let now = doc.active_animations[0].start_time + Duration::from_millis(100);
    doc.tick_animations(now);
    assert!(doc.needs_animation_frame);
}

#[test]
fn ticker_transform_animation_stays_running_across_cycles() {
    let html = r#"<html><head><style>
        @keyframes ticker-scroll {
            0% { transform: translateX(0); }
            100% { transform: translateX(-50%); }
        }
        .ticker {
            display: inline-block;
            width: 2000px;
            will-change: transform;
            animation: ticker-scroll 25s linear infinite;
        }
    </style></head><body><ul class="ticker"><li>one two three four five six</li></ul></body></html>"#;
    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    assert_eq!(doc.active_animations.len(), 1);
    assert!(
        doc.active_animations[0]
            .animation
            .iteration_count
            .is_infinite()
    );
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;

    for elapsed in [
        500, 5_000, 12_500, 24_000, 25_000, 25_500, 50_500, 75_500, 100_500,
    ] {
        doc.tick_animations(start + Duration::from_millis(elapsed));
        let transform = doc
            .animation_overrides
            .get(&id)
            .and_then(|props| props.iter().find(|(name, _)| name == "transform"))
            .map(|(_, value)| value.as_str())
            .expect("ticker transform override");
        assert!(
            !transform.is_empty(),
            "ticker should produce a transform override at {elapsed}ms"
        );
        assert!(
            doc.needs_animation_frame,
            "infinite ticker should still request frames at {elapsed}ms"
        );
        if matches!(elapsed, 500 | 25_500 | 50_500 | 75_500 | 100_500) {
            assert!(
                transform == "translateX(-1%)" || transform.starts_with("translateX(-1."),
                "same phase in later cycles should sample the same ongoing transform, got {transform}"
            );
        }
    }
    assert_eq!(
        doc.active_animations.len(),
        1,
        "infinite ticker should not be removed from active animations"
    );
}

#[test]
fn infinite_animation_wraps_at_repeat_boundary_without_dwell() {
    let mut doc = doc_with_animation("animation: fade 1s linear infinite;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;

    doc.tick_animations(start + Duration::from_millis(1000));

    let opacity = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(name, _)| name == "opacity"))
        .and_then(|(_, value)| value.parse::<f32>().ok())
        .expect("opacity override at repeat boundary");
    assert!(
        opacity < 0.05,
        "infinite repeat boundary should continue into the next iteration, got {opacity}"
    );
    assert!(doc.needs_animation_frame);
}

#[test]
fn tick_animations_finished_when_done() {
    let mut doc = doc_with_animation("animation: spin 0.1s linear 1;");
    // Jump way past the end of the animation.
    let now = doc.active_animations[0].start_time + Duration::from_millis(500);
    doc.tick_animations(now);
    // Animation should have been removed.
    assert!(
        doc.active_animations.is_empty(),
        "finished animation should be removed"
    );
}

#[test]
fn tick_animations_dispatches_animationend_when_animation_finishes() {
    let mut doc = doc_with_animation("animation: spin 0.1s linear 1;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = std::sync::Arc::clone(&seen);
    doc.add_event_listener(
        id,
        "animationend",
        Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.tick_animations(start + Duration::from_millis(500));

    assert_eq!(*seen.lock().unwrap(), 1);
}

#[test]
fn finished_animation_survives_recascade_without_restarting_or_requesting_frames() {
    for fill in ["none", "forwards", "both"] {
        let mut doc = doc_with_animation(&format!("animation: spin .1s linear 1 {fill};"));
        let id = doc.active_animations[0].element_id;
        let start = doc.active_animations[0].start_time;
        let events = record_css_events(
            &mut doc,
            id,
            &["animationstart", "animationend", "animationcancel"],
        );
        let end = start + Duration::from_millis(500);
        doc.tick_animations(end);
        let final_sample = doc.animation_overrides.clone();
        for delta in [600, 700, 800] {
            let now = start + Duration::from_millis(delta);
            doc.sync_animations(now);
            doc.tick_animations(now);
            assert!(doc.active_animations.is_empty());
            assert!(!doc.needs_animation_frame);
            assert_eq!(doc.finished_animations.len(), 1);
            assert_eq!(doc.finished_animations[0].start_time, start);
            assert_eq!(doc.animation_overrides, final_sample);
        }
        crate::css::apply_property(
            std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
            "animation",
            "none",
        );
        doc.sync_animations(end + Duration::from_millis(400));
        doc.tick_animations(end + Duration::from_millis(400));
        assert!(doc.finished_animations.is_empty());
        assert!(doc.animation_overrides.is_empty());
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .map(|event| event.0.as_str())
                .collect::<Vec<_>>(),
            ["animationend"]
        );
        crate::css::apply_property(
            std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
            "animation",
            "spin .1s linear",
        );
        doc.sync_animations(end + Duration::from_millis(500));
        assert_eq!(doc.active_animations.len(), 1);
        assert_eq!(events.lock().unwrap().last().unwrap().0, "animationstart");
    }
}

#[test]
fn finished_animation_duration_update_preserves_playback_time() {
    let mut doc = doc_with_animation("animation: spin .1s linear forwards;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    doc.tick_animations(start + Duration::from_millis(200));
    crate::css::apply_property(
        std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style),
        "animation",
        "spin 1s linear forwards",
    );
    doc.sync_animations(start + Duration::from_millis(300));
    doc.tick_animations(start + Duration::from_millis(300));
    assert_eq!(doc.active_animations.len(), 1);
    assert_eq!(doc.active_animations[0].start_time, start);
    assert!(doc.needs_animation_frame);
    assert_eq!(
        doc.animation_overrides[&id],
        [("transform".into(), "rotate(108deg)".into())]
    );
}

#[test]
fn zero_duration_animation_retains_its_forward_fill() {
    for iterations in ["1", "infinite"] {
        let mut doc =
            doc_with_animation(&format!("animation: spin 0s linear {iterations} forwards;"));
        let id = doc.finished_animations[0].element_id;
        let now = std::time::Instant::now();
        doc.sync_animations(now);
        doc.tick_animations(now);
        assert_eq!(
            doc.animation_overrides[&id],
            [("transform".into(), "rotate(360deg)".into())]
        );
        assert!(doc.active_animations.is_empty());
        assert!(!doc.needs_animation_frame);
    }
}

#[test]
fn finished_animation_fill_keeps_order_among_running_effects() {
    let mut doc = doc_with_animation("animation: spin 2s linear, spin .1s linear forwards;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    for delta in [200, 400, 600] {
        let now = start + Duration::from_millis(delta);
        doc.sync_animations(now);
        doc.tick_animations(now);
        assert_eq!(doc.active_animations.len(), 1);
        assert_eq!(doc.finished_animations.len(), 1);
        assert_eq!(
            doc.animation_overrides[&id],
            [("transform".into(), "rotate(360deg)".into())]
        );
    }
}

#[test]
fn tick_animations_dispatches_animationiteration_on_completed_non_final_cycle() {
    let mut doc = doc_with_animation("animation: spin 0.1s linear 3;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = std::sync::Arc::clone(&seen);
    doc.add_event_listener(
        id,
        "animationiteration",
        Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.tick_animations(start + Duration::from_millis(125));
    doc.tick_animations(start + Duration::from_millis(150));

    assert_eq!(*seen.lock().unwrap(), 1);
    assert_eq!(doc.active_animations[0].last_iteration_event, 1);
}

#[test]
fn tick_animations_dispatches_transitionend_when_transition_finishes() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let start = Instant::now();
    doc.transition_states
        .entry(id)
        .or_default()
        .push(TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 100.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        });
    let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = std::sync::Arc::clone(&seen);
    doc.add_event_listener(
        id,
        "transitionend",
        Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.tick_animations(start + Duration::from_millis(150));

    assert_eq!(*seen.lock().unwrap(), 1);
    assert!(!doc.transition_states.contains_key(&id));
}

#[test]
fn fractional_iteration_count_ends_partway_through_cycle() {
    let mut doc = doc_with_animation("animation: fade 1s linear 0.5 forwards;");
    let id = doc.active_animations[0].element_id;
    let now = doc.active_animations[0].start_time + Duration::from_millis(500);

    doc.tick_animations(now);

    assert!(
        doc.active_animations.is_empty(),
        "half-iteration animation should be complete at 500ms"
    );
    let opacity = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| {
            props
                .iter()
                .find(|(name, _)| name == "opacity")
                .map(|(_, value)| value)
        })
        .and_then(|v| v.parse::<f32>().ok())
        .expect("forwards fill opacity override");
    assert!(
        (opacity - 0.5).abs() < 0.05,
        "expected forwards fill at cycle midpoint, got {opacity}"
    );
}

#[test]
fn tick_animations_delay_phase() {
    let mut doc = doc_with_animation("animation: spin 1s linear 0.5s;"); // 500ms delay
    // Only 100ms in — still in delay.
    let now = doc.active_animations[0].start_time + Duration::from_millis(100);
    doc.tick_animations(now);
    // Still running (in delay phase).
    assert!(doc.needs_animation_frame);
    // No override yet (no backwards fill-mode).
    assert!(
        doc.animation_overrides
            .values()
            .all(|p| p.is_empty() || { p.iter().all(|(k, _)| k != "transform") }),
        "no transform override expected during delay without backwards fill"
    );
}

#[test]
fn tick_animations_backwards_fill_during_delay() {
    let mut doc = doc_with_animation("animation: fade 1s linear 0.5s backwards;");
    let now = doc.active_animations[0].start_time + Duration::from_millis(100);
    doc.tick_animations(now);
    // With `backwards`, we should see the "from" keyframe applied (opacity: 0).
    let has_opacity = doc
        .animation_overrides
        .values()
        .any(|props| props.iter().any(|(k, _)| k == "opacity"));
    assert!(
        has_opacity,
        "backwards fill should apply 'from' keyframe during delay"
    );
}

#[test]
fn paused_animation_does_not_advance_or_request_frames() {
    let mut doc = doc_with_animation("animation: fade 1s linear both paused;");
    let id = doc.active_animations[0].element_id;
    let now = doc.active_animations[0].start_time + Duration::from_millis(500);

    doc.tick_animations(now);

    assert_eq!(
        doc.active_animations.len(),
        1,
        "paused animation stays active"
    );
    assert!(
        !doc.needs_animation_frame,
        "a paused animation should not schedule another frame"
    );
    let opacity = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .expect("paused both-fill animation should hold the first keyframe");
    assert!(
        opacity < 0.05,
        "paused animation should hold at the start, got {opacity}"
    );
    let held = doc.animation_overrides.clone();
    assert!(
        !doc.tick_animations(now + Duration::from_millis(100)),
        "sampling a paused animation again must not report changed overrides"
    );
    assert_eq!(doc.animation_overrides, held);
}

#[test]
fn paused_animation_resumes_from_paused_position() {
    let mut doc = doc_with_animation("animation: fade 1s linear both;");
    let id = doc.active_animations[0].element_id;
    let start = doc.active_animations[0].start_time;

    let pause_at = start + Duration::from_millis(400);
    doc.tick_animations(pause_at);
    let before_pause = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .expect("running opacity before pause");
    assert!((before_pause - 0.4).abs() < 0.05);

    {
        let node = doc.get_box_by_id_mut(id).expect("animated node");
        std::sync::Arc::make_mut(&mut node.style)
            .rare_mut()
            .animations[0]
            .play_state_paused = true;
    }
    doc.sync_animations(pause_at);

    let still_paused_at = start + Duration::from_millis(1400);
    doc.tick_animations(still_paused_at);
    let while_paused = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .expect("paused opacity should keep sampling");
    assert!(
        (while_paused - before_pause).abs() < 0.05,
        "paused animation should freeze at {before_pause}, got {while_paused}"
    );
    assert!(
        !doc.needs_animation_frame,
        "paused animation should not request frames"
    );

    {
        let node = doc.get_box_by_id_mut(id).expect("animated node");
        std::sync::Arc::make_mut(&mut node.style)
            .rare_mut()
            .animations[0]
            .play_state_paused = false;
    }
    doc.sync_animations(still_paused_at);

    doc.tick_animations(still_paused_at + Duration::from_millis(100));
    let after_resume = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .expect("resumed opacity");
    assert!(
        (after_resume - 0.5).abs() < 0.05,
        "resume should advance from the paused 40% sample to about 50%, got {after_resume}"
    );
}

#[test]
fn tick_animations_direction_reverse() {
    let mut doc = doc_with_animation("animation: fade 1s linear reverse 1;");
    let start = doc.active_animations[0].start_time;
    // At start (t=0), with reverse direction the effective t=1 → opacity should be ~1.
    let now = start + Duration::from_millis(10);
    doc.tick_animations(now);
    let overrides = doc
        .animation_overrides
        .values()
        .flat_map(|p| p.iter())
        .find(|(k, _)| k == "opacity")
        .map(|(_, v)| v.parse::<f32>().unwrap_or(-1.0));
    if let Some(v) = overrides {
        assert!(v > 0.8, "reverse at t~0 should give opacity~1, got {}", v);
    }
}

#[test]
fn tick_animations_alternate_direction() {
    let mut doc = doc_with_animation("animation: fade 1s linear alternate infinite;");
    let start = doc.active_animations[0].start_time;

    // First iteration (even): t goes 0→1, so opacity goes 0→1.
    let now_half = start + Duration::from_millis(500);
    doc.tick_animations(now_half);
    let op1 = doc
        .animation_overrides
        .values()
        .flat_map(|p| p.iter())
        .find(|(k, _)| k == "opacity")
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .unwrap_or(-1.0);
    assert!(
        (op1 - 0.5).abs() < 0.1,
        "first iter mid: expected ~0.5, got {}",
        op1
    );

    // Second iteration (odd): direction reverses, so at t=0.25 within iter, effective=0.75.
    let now_iter2 = start + Duration::from_millis(1250);
    doc.animation_overrides.clear();
    doc.tick_animations(now_iter2);
    let op2 = doc
        .animation_overrides
        .values()
        .flat_map(|p| p.iter())
        .find(|(k, _)| k == "opacity")
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .unwrap_or(-1.0);
    assert!(
        op2 > 0.5,
        "second iter (reversed) t=0.25 → effective=0.75, got {}",
        op2
    );
}

// ── Integration: layout applies animation overrides ───────────────────────────

#[test]
fn layout_applies_animation_override_to_style() {
    // After layout, the animated element's computed opacity should reflect
    // the animation, not just the stylesheet value.
    let html = r#"<html><head><style>
        @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
        .box { opacity: 1; animation: fade-in 10s linear; }
    </style></head><body><div class="box">hi</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    // Animation just started, so at t~=0 opacity should be ~0 (from keyframe).
    // Layout keeps the cascaded style stable and exposes the sampled frame
    // through animation_overrides; renderer applies those overrides while
    // building paint commands.
    let b = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(b.is_some(), "div should exist");
    let node_id = b.unwrap().node_id;
    let opacity = doc
        .animation_overrides
        .get(&node_id)
        .and_then(|props| props.iter().find(|(name, _)| name == "opacity"))
        .and_then(|(_, value)| value.parse::<f32>().ok())
        .unwrap_or(1.0);
    assert!(
        opacity < 0.2,
        "opacity at animation start should be ~0, got {}",
        opacity
    );
}

#[test]
fn doc_needs_animation_frame_set_by_layout() {
    let doc = doc_with_animation("animation: spin 2s linear infinite;");
    assert!(
        doc.needs_animation_frame,
        "should need another frame while animation runs"
    );
}

#[test]
fn hidden_subtree_does_not_start_child_animation() {
    let html = r#"<html><head><style>
        @keyframes spin { from { transform: rotate(0deg) } to { transform: rotate(360deg) } }
        .hidden { display: none }
        .spinner { animation: spin 1s linear infinite }
    </style></head><body><div class="hidden"><div class="spinner"></div></div></body></html>"#;
    let mut doc = parse_html(html);
    LayoutEngine::new().layout(&mut doc, 800.0);

    assert!(doc.active_animations.is_empty());
    assert!(!doc.needs_animation_frame);
}

// ── CSS Transitions ───────────────────────────────────────────────────────────

#[test]
fn transition_state_starts_on_style_change() {
    let html = r#"<html><head><style>
        .box { opacity: 1; transition: opacity 0.5s linear; }
    </style></head><body><div class="box">hi</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    // Simulate a style change: manually inject a new opacity into the stylesheet
    // and force a re-cascade by changing the inline style.
    let b_nid = find_box(&doc.root, &|b: &WebCore| b.tag == "div").map(|b| b.node_id);

    // Directly write a prev_style snapshot for the element with opacity=1,
    // then do another layout — if the computed opacity changed the engine
    // should have started a transition.
    if let Some(id) = b_nid {
        let mut prev = std::collections::HashMap::new();
        prev.insert("opacity".to_string(), "1".to_string());
        doc.prev_styles.insert(id, prev);

        // Force a cascade so sync_transitions runs.
        engine.invalidate_cascade();
        engine.layout(&mut doc, 800.0);

        // If the opacity hasn't changed (still 1 from stylesheet),
        // no transition should have started for opacity.
        // The important thing is that no panic occurred and the doc is consistent.
        assert!(doc.transition_states.len() <= 1);
    }
}

#[test]
fn hover_transition_targets_active_hover_style() {
    let html = r#"<html><head><style>
        #box {
            background-color: black;
            color: rebeccapurple;
            transform: none;
            transition:
                background-color 1000ms linear,
                color 1000ms linear,
                transform 1000ms linear;
        }
        #box:hover {
            background-color: white;
            color: white;
            transform: translateX(20px) scale(1.2);
        }
    </style></head><body><div id="box">Outline</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let id = doc.get_element_by_id("box").expect("box id");
    doc.hovered_box = id;
    doc.hover_changed = true;
    engine.layout(&mut doc, 800.0);

    let states = doc.transition_states.get(&id).expect("hover transitions");
    let target = |prop: &str| {
        states
            .iter()
            .find(|state| state.property == prop)
            .map(|state| state.to_value.as_str())
    };
    assert_eq!(target("background-color"), Some("rgba(255,255,255,1.0000)"));
    assert_eq!(target("color"), Some("rgba(255,255,255,1.0000)"));
    assert_eq!(target("transform"), Some("translateX(20px) scale(1.2)"));

    let initial_overrides = doc
        .animation_overrides
        .get(&id)
        .expect("first transition frame overrides");
    let initial = |prop: &str| {
        initial_overrides
            .iter()
            .find(|(name, _)| name == prop)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(
        initial("background-color"),
        Some("rgba(0,0,0,1.0000)"),
        "hover paint must start from the base background, not jump to hover"
    );
    assert_eq!(
        initial("color"),
        Some("rgba(102,51,153,1.0000)"),
        "hover paint must start from the base text color, not jump to hover"
    );
    assert_eq!(initial("transform"), Some("translateX(0px) scale(1)"));

    let start = states
        .iter()
        .find(|state| state.property == "transform")
        .expect("transform transition")
        .start_time;
    doc.tick_animations(start + Duration::from_millis(500));
    let overrides = doc
        .animation_overrides
        .get(&id)
        .expect("mid-frame overrides");
    let transform = overrides
        .iter()
        .find(|(prop, _)| prop == "transform")
        .map(|(_, value)| value.as_str())
        .expect("transform override");
    assert_eq!(transform, "translateX(10px) scale(1.1)");
}

#[test]
fn hover_transition_reverses_to_base_style_from_current_sample() {
    let html = r#"<html><head><style>
        #box {
            background-color: black;
            color: black;
            transition: background-color 1000ms linear, color 1000ms linear;
        }
        #box:hover {
            background-color: white;
            color: white;
        }
    </style></head><body><div id="box">Move between swatches</div></body></html>"#;

    let mut doc = parse_html(html);
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);

    let id = doc.get_element_by_id("box").expect("box id");
    doc.hovered_box = id;
    doc.hover_changed = true;
    engine.layout(&mut doc, 800.0);

    let sample_now = Instant::now();
    for state in doc
        .transition_states
        .get_mut(&id)
        .expect("hover-in transitions")
    {
        state.start_time = sample_now - Duration::from_millis(500);
    }
    doc.tick_animations(sample_now);
    let sampled = doc
        .animation_overrides
        .get(&id)
        .and_then(|props| props.iter().find(|(prop, _)| prop == "background-color"))
        .map(|(_, value)| value.clone())
        .expect("sampled background color");

    doc.hovered_box = 0;
    doc.hover_changed = true;
    engine.layout(&mut doc, 800.0);

    let reverse = doc
        .transition_states
        .get(&id)
        .and_then(|states| {
            states
                .iter()
                .find(|state| state.property == "background-color")
        })
        .expect("hover-out background transition");
    let red = |value: &str| {
        value
            .strip_prefix("rgba(")
            .unwrap()
            .split(',')
            .next()
            .unwrap()
            .parse::<i32>()
            .unwrap()
    };
    assert!(
        (red(&reverse.from_value) - red(&sampled)).abs() <= 4,
        "reversal must start at the current color without a visible jump: sampled={sampled}, reverse={}",
        reverse.from_value
    );
    assert_eq!(reverse.to_value, "rgba(0,0,0,1.0000)");

    let color_reverse = doc
        .transition_states
        .get(&id)
        .and_then(|states| states.iter().find(|state| state.property == "color"))
        .expect("hover-out color transition");
    assert_eq!(color_reverse.to_value, "rgba(0,0,0,1.0000)");
}

#[test]
fn sync_transitions_dispatches_transitionrun_and_transitionstart_when_created() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    {
        let node = doc.get_box_by_id_mut(id).unwrap();
        let style = std::sync::Arc::make_mut(&mut node.style);
        style.opacity = 1.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 100.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
    }
    let mut prev = std::collections::HashMap::new();
    prev.insert("opacity".to_string(), "0".to_string());
    doc.prev_styles.insert(id, prev);

    let seen_run = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter_run = std::sync::Arc::clone(&seen_run);
    doc.add_event_listener(
        id,
        "transitionrun",
        Box::new(move |_, _| {
            *counter_run.lock().unwrap() += 1;
        }),
        Default::default(),
    );
    let seen_start = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter_start = std::sync::Arc::clone(&seen_start);
    doc.add_event_listener(
        id,
        "transitionstart",
        Box::new(move |_, _| {
            *counter_start.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.sync_transitions(Instant::now());

    assert_eq!(*seen_run.lock().unwrap(), 1);
    assert_eq!(*seen_start.lock().unwrap(), 1);
    assert!(doc.transition_states.contains_key(&id));
}

#[test]
fn delayed_transition_dispatches_start_once_after_run() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    {
        let node = doc.get_box_by_id_mut(id).unwrap();
        let style = std::sync::Arc::make_mut(&mut node.style);
        style.opacity = 1.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 200.0,
            delay_ms: 100.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
    }
    doc.prev_styles.insert(
        id,
        std::collections::HashMap::from([("opacity".to_string(), "0".to_string())]),
    );
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for name in ["transitionrun", "transitionstart", "transitionend"] {
        let events = std::sync::Arc::clone(&events);
        doc.add_event_listener(
            id,
            name,
            Box::new(move |event, _| {
                events.lock().unwrap().push(event.event_type.clone());
            }),
            Default::default(),
        );
    }
    let start = Instant::now();
    doc.sync_transitions(start);
    assert_eq!(*events.lock().unwrap(), ["transitionrun"]);
    doc.tick_animations(start + Duration::from_millis(99));
    assert_eq!(*events.lock().unwrap(), ["transitionrun"]);
    doc.tick_animations(start + Duration::from_millis(100));
    doc.tick_animations(start + Duration::from_millis(101));
    assert_eq!(
        *events.lock().unwrap(),
        ["transitionrun", "transitionstart"]
    );
    doc.tick_animations(start + Duration::from_millis(301));
    assert_eq!(
        *events.lock().unwrap(),
        ["transitionrun", "transitionstart", "transitionend"]
    );
}

#[test]
fn delayed_transition_jump_past_end_still_dispatches_start_before_end() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let start = Instant::now();
    doc.transition_states.insert(
        id,
        vec![TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 100.0,
            delay_ms: 100.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: false,
        }],
    );
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for name in ["transitionstart", "transitionend"] {
        let events = std::sync::Arc::clone(&events);
        doc.add_event_listener(
            id,
            name,
            Box::new(move |event, _| {
                events.lock().unwrap().push(event.event_type.clone());
            }),
            Default::default(),
        );
    }
    doc.tick_animations(start + Duration::from_millis(500));
    assert_eq!(
        *events.lock().unwrap(),
        ["transitionstart", "transitionend"]
    );
}

#[test]
fn unchanged_transition_style_skips_extraction_but_mutation_and_removal_update_state() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    {
        let node = doc.get_box_by_id_mut(id).unwrap();
        let style = std::sync::Arc::make_mut(&mut node.style);
        style.opacity = 0.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 100.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
    }
    let start = Instant::now();
    doc.sync_transitions(start);
    doc.prev_styles
        .get_mut(&id)
        .unwrap()
        .insert("sentinel".into(), "kept".into());
    doc.sync_transitions(start + Duration::from_millis(1));
    assert_eq!(
        doc.prev_styles[&id].get("sentinel").map(String::as_str),
        Some("kept")
    );

    std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style).opacity = 1.0;
    doc.sync_transitions(start + Duration::from_millis(2));
    assert!(!doc.prev_styles[&id].contains_key("sentinel"));
    assert_eq!(
        doc.transition_states[&id]
            .iter()
            .find(|state| state.property == "opacity")
            .unwrap()
            .to_value,
        "1",
    );

    std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style)
        .rare_mut()
        .transitions
        .clear();
    doc.sync_transitions(start + Duration::from_millis(3));
    assert!(!doc.prev_styles.contains_key(&id));
    assert!(!doc.transition_style_refs.contains_key(&id));
}

#[test]
fn sync_transitions_dispatches_transitioncancel_when_replacing_running_transition() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    {
        let node = doc.get_box_by_id_mut(id).unwrap();
        let style = std::sync::Arc::make_mut(&mut node.style);
        style.opacity = 1.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 100.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
    }
    let start = Instant::now();
    doc.transition_states
        .entry(id)
        .or_default()
        .push(TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "0.5".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 100.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        });
    let mut prev = std::collections::HashMap::new();
    prev.insert("opacity".to_string(), "0".to_string());
    doc.prev_styles.insert(id, prev);

    let seen = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let counter = std::sync::Arc::clone(&seen);
    doc.add_event_listener(
        id,
        "transitioncancel",
        Box::new(move |_, _| {
            *counter.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.sync_transitions(start + Duration::from_millis(10));

    assert_eq!(*seen.lock().unwrap(), 1);
    assert_eq!(
        doc.transition_states
            .get(&id)
            .and_then(|states| states.iter().find(|state| state.property == "opacity"))
            .map(|state| state.to_value.as_str()),
        Some("1")
    );
}

#[test]
fn transition_reversal_uses_eased_progress_and_scaled_negative_delay() {
    for delay in [-100.0, 100.0] {
        let mut doc = parse_html("<div id='box'></div>");
        let id = doc.get_element_by_id("box").unwrap();
        let style = std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style);
        style.opacity = 0.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 1000.0,
            delay_ms: delay,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
        let start = Instant::now();
        // x(t) = t and y(t) = t^3: halfway in time is 1/8 in value.
        doc.transition_states.insert(
            id,
            vec![TransitionState {
                property: "opacity".into(),
                from_value: "0".into(),
                to_value: "1".into(),
                reversing_adjusted_start_value: "0".into(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 100.0,
                timing_fn: EasingFn::CubicBezier(1.0 / 3.0, 0.0, 2.0 / 3.0, 0.0),
                allow_discrete: false,
                start_event_fired: false,
            }],
        );
        doc.animation_overrides
            .insert(id, vec![("opacity".into(), "0.125".into())]);
        doc.prev_styles
            .insert(id, [("opacity".into(), "1".into())].into());
        doc.sync_transitions(start + Duration::from_millis(600));
        let state = &doc.transition_states[&id][0];
        assert!((state.duration_ms - 125.0).abs() < 0.1, "{state:?}");
        assert_eq!(state.reversing_adjusted_start_value, "1");
        let expected_delay = if delay < 0.0 { -12.5 } else { 100.0 };
        assert!((state.delay_ms - expected_delay).abs() < 0.1);
    }
}

#[test]
fn transition_repeated_reversal_keeps_logical_endpoints() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let style = std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style);
    style.opacity = 1.0;
    style.rare_mut().transitions.push(ParsedTransition {
        property: "opacity".into(),
        duration_ms: 1000.0,
        delay_ms: 0.0,
        timing_fn: EasingFn::Linear,
        allow_discrete: false,
    });
    let start = Instant::now();
    doc.transition_states.insert(
        id,
        vec![TransitionState {
            property: "opacity".into(),
            from_value: "0.2".into(),
            to_value: "0".into(),
            reversing_adjusted_start_value: "1".into(),
            reversing_shortening_factor: 0.2,
            start_time: start,
            duration_ms: 200.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        }],
    );
    doc.animation_overrides
        .insert(id, vec![("opacity".into(), "0.1".into())]);
    doc.prev_styles
        .insert(id, [("opacity".into(), "0".into())].into());
    doc.sync_transitions(start + Duration::from_millis(100));
    let state = &doc.transition_states[&id][0];
    assert!((state.duration_ms - 900.0).abs() < 0.1, "{state:?}");
    assert_eq!(state.reversing_adjusted_start_value, "0");
    assert_eq!(state.from_value, "0.1");
}

#[test]
fn transition_reversal_samples_value_at_style_change_between_frames() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let style = std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style);
    style.opacity = 0.0;
    style.rare_mut().transitions.push(ParsedTransition {
        property: "opacity".into(),
        duration_ms: 1000.0,
        delay_ms: 0.0,
        timing_fn: EasingFn::Linear,
        allow_discrete: false,
    });
    let start = Instant::now();
    doc.transition_states.insert(
        id,
        vec![TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        }],
    );
    doc.animation_overrides
        .insert(id, vec![("opacity".into(), "0.2".into())]);
    doc.prev_styles
        .insert(id, [("opacity".into(), "1".into())].into());

    doc.sync_transitions(start + Duration::from_millis(600));
    let state = &doc.transition_states[&id][0];
    assert_eq!(state.from_value, "0.6");
    assert!((state.duration_ms - 600.0).abs() < 0.1, "{state:?}");
}

#[test]
fn reverting_during_transition_delay_cancels_without_starting_a_replacement() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let style = std::sync::Arc::make_mut(&mut doc.get_box_by_id_mut(id).unwrap().style);
    style.opacity = 0.0;
    style.rare_mut().transitions.push(ParsedTransition {
        property: "opacity".into(),
        duration_ms: 1000.0,
        delay_ms: 400.0,
        timing_fn: EasingFn::Linear,
        allow_discrete: false,
    });
    let start = Instant::now();
    doc.transition_states.insert(
        id,
        vec![TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 400.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: false,
        }],
    );
    doc.prev_styles
        .insert(id, [("opacity".into(), "1".into())].into());

    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    for name in ["transitioncancel", "transitionrun", "transitionstart"] {
        let seen = std::sync::Arc::clone(&events);
        doc.add_event_listener(
            id,
            name,
            Box::new(move |event, _| {
                seen.lock().unwrap().push(event.event_type.clone());
            }),
            Default::default(),
        );
    }
    doc.sync_transitions(start + Duration::from_millis(100));
    assert!(
        !doc.transition_states
            .get(&id)
            .is_some_and(|states| !states.is_empty())
    );
    assert_eq!(&*events.lock().unwrap(), &["transitioncancel"]);
}

#[test]
fn removing_transition_property_cancels_a_running_transition() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    let start = Instant::now();
    doc.transition_states.insert(
        id,
        vec![TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        }],
    );
    let cancelled = std::sync::Arc::new(std::sync::Mutex::new(0));
    let seen = std::sync::Arc::clone(&cancelled);
    doc.add_event_listener(
        id,
        "transitioncancel",
        Box::new(move |_, _| {
            *seen.lock().unwrap() += 1;
        }),
        Default::default(),
    );

    doc.sync_transitions(start + Duration::from_millis(100));
    assert!(doc.transition_states.is_empty());
    assert_eq!(*cancelled.lock().unwrap(), 1);
}

#[test]
fn reversing_transition_uses_shortened_duration() {
    let mut doc = parse_html("<div id='box'></div>");
    let id = doc.get_element_by_id("box").unwrap();
    {
        let node = doc.get_box_by_id_mut(id).unwrap();
        let style = std::sync::Arc::make_mut(&mut node.style);
        style.opacity = 0.0;
        style.rare_mut().transitions.push(ParsedTransition {
            property: "opacity".into(),
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
        });
    }
    let start = Instant::now();
    doc.transition_states
        .entry(id)
        .or_default()
        .push(TransitionState {
            property: "opacity".into(),
            from_value: "0".into(),
            to_value: "1".into(),
            reversing_adjusted_start_value: "0".into(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        });
    doc.animation_overrides
        .entry(id)
        .or_default()
        .push(("opacity".into(), "0.2".into()));
    let mut prev = std::collections::HashMap::new();
    prev.insert("opacity".to_string(), "1".to_string());
    doc.prev_styles.insert(id, prev);

    doc.sync_transitions(start + Duration::from_millis(200));

    let state = doc
        .transition_states
        .get(&id)
        .and_then(|states| states.iter().find(|state| state.property == "opacity"))
        .expect("replacement transition");
    assert_eq!(state.from_value, "0.2");
    assert_eq!(state.to_value, "0");
    assert!(
        (state.duration_ms - 200.0).abs() < 1.0,
        "reversal should use remaining travelled fraction, got {}ms",
        state.duration_ms
    );
}

#[test]
fn transition_interpolates_between_values() {
    use std::collections::HashMap;

    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xDEAD; // fake element node_id

    // Insert a transition state directly.
    let start = Instant::now() - Duration::from_millis(250);
    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "opacity".to_string(),
            from_value: "0".to_string(),
            to_value: "1".to_string(),
            reversing_adjusted_start_value: "0".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 500.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        }],
    );

    let now = start + Duration::from_millis(250);
    doc.tick_animations(now);

    // At 250ms / 500ms = 0.5 progress, opacity should be ~0.5.
    let val = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .unwrap_or(-1.0);
    assert!((val - 0.5).abs() < 0.05, "expected ~0.5, got {}", val);
}

#[test]
fn allow_discrete_display_transition_keeps_box_visible_until_end() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xD15C;
    let start = Instant::now();

    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "display".to_string(),
            from_value: "block".to_string(),
            to_value: "none".to_string(),
            reversing_adjusted_start_value: "block".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: true,
            start_event_fired: true,
        }],
    );

    doc.tick_animations(start + Duration::from_millis(500));
    let mid = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "display"))
        .map(|(_, v)| v.as_str());
    assert_eq!(mid, Some("block"));

    doc.tick_animations(start + Duration::from_millis(1000));
    let end = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "display"))
        .map(|(_, v)| v.as_str());
    assert_eq!(end, Some("none"));
}

#[test]
fn allow_discrete_pointer_events_transition_flips_at_halfway() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xD151;
    let start = Instant::now();

    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "pointer-events".to_string(),
            from_value: "auto".to_string(),
            to_value: "none".to_string(),
            reversing_adjusted_start_value: "auto".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: true,
            start_event_fired: true,
        }],
    );

    doc.tick_animations(start + Duration::from_millis(400));
    let before_half = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "pointer-events"))
        .map(|(_, v)| v.as_str());
    assert_eq!(before_half, Some("auto"));

    doc.tick_animations(start + Duration::from_millis(600));
    let after_half = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "pointer-events"))
        .map(|(_, v)| v.as_str());
    assert_eq!(after_half, Some("none"));
}

#[test]
fn allow_discrete_cursor_transition_flips_at_halfway() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xD152;
    let start = Instant::now();

    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "cursor".to_string(),
            from_value: "auto".to_string(),
            to_value: "grab".to_string(),
            reversing_adjusted_start_value: "auto".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 1000.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: true,
            start_event_fired: true,
        }],
    );

    doc.tick_animations(start + Duration::from_millis(400));
    let before_half = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "cursor"))
        .map(|(_, v)| v.as_str());
    assert_eq!(before_half, Some("auto"));

    doc.tick_animations(start + Duration::from_millis(600));
    let after_half = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "cursor"))
        .map(|(_, v)| v.as_str());
    assert_eq!(after_half, Some("grab"));
}

#[test]
fn allow_discrete_basic_ui_keywords_flip_at_halfway() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xD153;
    let start = Instant::now();

    doc.transition_states.insert(
        elem_id,
        vec![
            TransitionState {
                property: "user-select".to_string(),
                from_value: "auto".to_string(),
                to_value: "none".to_string(),
                reversing_adjusted_start_value: "auto".to_string(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 0.0,
                timing_fn: EasingFn::Linear,
                allow_discrete: true,
                start_event_fired: true,
            },
            TransitionState {
                property: "resize".to_string(),
                from_value: "none".to_string(),
                to_value: "both".to_string(),
                reversing_adjusted_start_value: "none".to_string(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 0.0,
                timing_fn: EasingFn::Linear,
                allow_discrete: true,
                start_event_fired: true,
            },
            TransitionState {
                property: "outline-style".to_string(),
                from_value: "none".to_string(),
                to_value: "dashed".to_string(),
                reversing_adjusted_start_value: "none".to_string(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 0.0,
                timing_fn: EasingFn::Linear,
                allow_discrete: true,
                start_event_fired: true,
            },
            TransitionState {
                property: "text-decoration-style".to_string(),
                from_value: "solid".to_string(),
                to_value: "wavy".to_string(),
                reversing_adjusted_start_value: "solid".to_string(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 0.0,
                timing_fn: EasingFn::Linear,
                allow_discrete: true,
                start_event_fired: true,
            },
            TransitionState {
                property: "column-rule-style".to_string(),
                from_value: "none".to_string(),
                to_value: "double".to_string(),
                reversing_adjusted_start_value: "none".to_string(),
                reversing_shortening_factor: 1.0,
                start_time: start,
                duration_ms: 1000.0,
                delay_ms: 0.0,
                timing_fn: EasingFn::Linear,
                allow_discrete: true,
                start_event_fired: true,
            },
        ],
    );

    doc.tick_animations(start + Duration::from_millis(400));
    let before = doc.animation_overrides.get(&elem_id).unwrap();
    assert_eq!(
        before
            .iter()
            .find(|(k, _)| k == "user-select")
            .map(|(_, v)| v.as_str()),
        Some("auto")
    );
    assert_eq!(
        before
            .iter()
            .find(|(k, _)| k == "resize")
            .map(|(_, v)| v.as_str()),
        Some("none")
    );
    assert_eq!(
        before
            .iter()
            .find(|(k, _)| k == "outline-style")
            .map(|(_, v)| v.as_str()),
        Some("none")
    );
    assert_eq!(
        before
            .iter()
            .find(|(k, _)| k == "text-decoration-style")
            .map(|(_, v)| v.as_str()),
        Some("solid")
    );
    assert_eq!(
        before
            .iter()
            .find(|(k, _)| k == "column-rule-style")
            .map(|(_, v)| v.as_str()),
        Some("none")
    );

    doc.tick_animations(start + Duration::from_millis(600));
    let after = doc.animation_overrides.get(&elem_id).unwrap();
    assert_eq!(
        after
            .iter()
            .find(|(k, _)| k == "user-select")
            .map(|(_, v)| v.as_str()),
        Some("none")
    );
    assert_eq!(
        after
            .iter()
            .find(|(k, _)| k == "resize")
            .map(|(_, v)| v.as_str()),
        Some("both")
    );
    assert_eq!(
        after
            .iter()
            .find(|(k, _)| k == "outline-style")
            .map(|(_, v)| v.as_str()),
        Some("dashed")
    );
    assert_eq!(
        after
            .iter()
            .find(|(k, _)| k == "text-decoration-style")
            .map(|(_, v)| v.as_str()),
        Some("wavy")
    );
    assert_eq!(
        after
            .iter()
            .find(|(k, _)| k == "column-rule-style")
            .map(|(_, v)| v.as_str()),
        Some("double")
    );
}

#[test]
fn transition_completes_and_is_removed() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xBEEF;

    let start = Instant::now() - Duration::from_millis(600);
    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "opacity".to_string(),
            from_value: "0".to_string(),
            to_value: "1".to_string(),
            reversing_adjusted_start_value: "0".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 500.0,
            delay_ms: 0.0,
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: true,
        }],
    );

    let now = start + Duration::from_millis(600);
    doc.tick_animations(now);

    assert!(
        doc.transition_states.is_empty(),
        "completed transition should be removed"
    );
    assert!(
        !doc.needs_animation_frame,
        "no more frames needed after transition completes"
    );
}

#[test]
fn transition_delay_applies_from_value() {
    let mut doc = parse_html("<html><body></body></html>");
    let elem_id: u32 = 0xCAFE;

    let start = Instant::now();
    doc.transition_states.insert(
        elem_id,
        vec![TransitionState {
            property: "opacity".to_string(),
            from_value: "0".to_string(),
            to_value: "1".to_string(),
            reversing_adjusted_start_value: "0".to_string(),
            reversing_shortening_factor: 1.0,
            start_time: start,
            duration_ms: 500.0,
            delay_ms: 300.0, // 300ms delay
            timing_fn: EasingFn::Linear,
            allow_discrete: false,
            start_event_fired: false,
        }],
    );

    // Only 100ms elapsed — still in delay.
    let now = start + Duration::from_millis(100);
    doc.tick_animations(now);

    // The "from" value should be applied during delay.
    let val = doc
        .animation_overrides
        .get(&elem_id)
        .and_then(|props| props.iter().find(|(k, _)| k == "opacity"))
        .and_then(|(_, v)| v.parse::<f32>().ok())
        .unwrap_or(-1.0);
    assert!(
        (val - 0.0).abs() < 0.01,
        "during delay, from_value (0) should be applied, got {}",
        val
    );
}

/// Destination: `src/tests/test_animation.rs`.
#[test]
fn a_function_with_commas_does_not_split_the_shorthand() {
    let trs = parse_transition_shorthand("transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)");
    assert_eq!(
        trs.len(),
        1,
        "`transition: transform .3s cubic-bezier(.4,0,.2,1)` is ONE transition, got {}: {:?}",
        trs.len(),
        trs.iter().map(|t| t.property.clone()).collect::<Vec<_>>()
    );
    assert_eq!(trs[0].property, "transform");
    assert!(
        (trs[0].duration_ms - 300.0).abs() < 1.0,
        "duration should be 300ms, got {}",
        trs[0].duration_ms
    );
    assert_eq!(
        trs[0].timing_fn,
        EasingFn::CubicBezier(0.4, 0.0, 0.2, 1.0),
        "the cubic-bezier control points must survive the shorthand, got {:?}",
        trs[0].timing_fn
    );

    let anims = parse_animation_shorthand("spin 1s cubic-bezier(0.4, 0, 0.2, 1) infinite");
    assert_eq!(
        anims.len(),
        1,
        "`animation: spin 1s cubic-bezier(...) infinite` is ONE animation, got {}: {:?}",
        anims.len(),
        anims.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
    );
    assert_eq!(anims[0].name, "spin");
    assert_eq!(
        anims[0].timing_fn,
        EasingFn::CubicBezier(0.4, 0.0, 0.2, 1.0)
    );
    assert!(
        anims[0].iteration_count.is_infinite(),
        "`infinite` belongs to the one animation, got iteration_count {}",
        anims[0].iteration_count
    );

    // `steps()` has the same shape: the truncated `steps(4` parses its count as
    // `None` and `unwrap_or(1)` turns it into step-end (measured).
    let st = parse_transition_shorthand("opacity 1s steps(4, jump-end)");
    assert_eq!(
        st.len(),
        1,
        "`steps(4, jump-end)` is one transition, got {}",
        st.len()
    );
    assert_eq!(
        st[0].timing_fn,
        EasingFn::Steps(4, StepPosition::JumpEnd),
        "the step count must survive the shorthand, got {:?}",
        st[0].timing_fn
    );
}

/// Destination: `src/tests/test_animation.rs`.
#[test]
fn steps_supports_every_jump_term() {
    let at = |f: &str, t: f32| apply_easing(&parse_easing(f), t);
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;

    // jump-none: n-1 = 2 rises, and the last interval reaches 1.
    assert!(
        close(at("steps(3, jump-none)", 0.1), 0.0),
        "steps(3, jump-none) on [0,1/3) is 0, got {}",
        at("steps(3, jump-none)", 0.1)
    );
    assert!(
        close(at("steps(3, jump-none)", 0.5), 0.5),
        "steps(3, jump-none) on [1/3,2/3) is 1/2, got {}",
        at("steps(3, jump-none)", 0.5)
    );
    assert!(
        close(at("steps(3, jump-none)", 0.8), 1.0),
        "steps(3, jump-none) on [2/3,1) is 1, got {}",
        at("steps(3, jump-none)", 0.8)
    );

    // jump-both: n+1 = 4 divisions, never 0 and never 1 inside [0,1).
    assert!(
        close(at("steps(3, jump-both)", 0.1), 0.25),
        "steps(3, jump-both) on [0,1/3) is 1/4, got {}",
        at("steps(3, jump-both)", 0.1)
    );
    assert!(
        close(at("steps(3, jump-both)", 0.5), 0.5),
        "steps(3, jump-both) on [1/3,2/3) is 1/2, got {}",
        at("steps(3, jump-both)", 0.5)
    );
    assert!(
        close(at("steps(3, jump-both)", 0.8), 0.75),
        "steps(3, jump-both) on [2/3,1) is 3/4, got {}",
        at("steps(3, jump-both)", 0.8)
    );

    // jump-end stays as it is, and `end` is its synonym.
    assert!(close(at("steps(3, jump-end)", 0.5), 1.0 / 3.0));
    assert!(close(at("steps(3, end)", 0.5), 1.0 / 3.0));
}

/// Destination: `src/tests/test_animation.rs`.
#[test]
fn step_start_jumps_at_the_start() {
    let at = |f: &str, t: f32| apply_easing(&parse_easing(f), t);
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;

    assert!(
        close(at("step-start", 0.0), 1.0),
        "css-easing-2 §2.3: `step-start` is `steps(1, start)`, whose only interval \
         [0,1) has the value 1, so t=0 gives 1; got {}",
        at("step-start", 0.0)
    );
    assert!(close(at("step-start", 0.5), 1.0));
    assert!(
        close(at("step-end", 0.0), 0.0),
        "`step-end` is `steps(1, end)`: [0,1) is 0"
    );

    assert!(
        close(at("steps(3, jump-start)", 0.0), 1.0 / 3.0),
        "steps(3, jump-start) on [0,1/3) is 1/3, got {}",
        at("steps(3, jump-start)", 0.0)
    );
    assert!(
        close(at("steps(3, jump-start)", 1.0 / 3.0), 2.0 / 3.0),
        "at an interval boundary the higher interval's value applies: \
         steps(3, jump-start) at t=1/3 is 2/3, got {}",
        at("steps(3, jump-start)", 1.0 / 3.0)
    );
    assert!(close(at("steps(3, jump-start)", 1.0), 1.0));
}

/// Destination: `src/tests/test_animation.rs`.
#[test]
fn linear_easing_function_is_supported() {
    // The declaration must survive intact: one animation named `fade`.
    let anims = parse_animation_shorthand("fade 1s linear(0, 0.25, 1)");
    assert_eq!(
        anims.len(),
        1,
        "`animation: fade 1s linear(0, .25, 1)` is one animation, got {}: {:?}",
        anims.len(),
        anims.iter().map(|a| a.name.clone()).collect::<Vec<_>>()
    );
    assert_eq!(
        anims[0].name, "fade",
        "`linear(...)` is an easing function, not the animation name"
    );

    let f = parse_easing("linear(0, 0.25, 1)");
    assert!(
        (apply_easing(&f, 0.5) - 0.25).abs() < 1e-3,
        "css-easing-2 §2.1: linear(0, 0.25, 1) has control points at 0, 0.5, 1, so the \
         output at input 0.5 is 0.25; got {}",
        apply_easing(&f, 0.5)
    );
    assert!(
        (apply_easing(&f, 0.25) - 0.125).abs() < 1e-3,
        "half-way between the (0,0) and (0.5,0.25) control points is 0.125; got {}",
        apply_easing(&f, 0.25)
    );
}
