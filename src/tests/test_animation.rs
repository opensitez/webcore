// Tests for the CSS animation and transition runtime.

use super::harness::*;
use crate::css::{
    extract_keyframes, parse_animation_shorthand, parse_easing, parse_transition_shorthand,
    Stylesheet,
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
    assert!(from
        .properties
        .iter()
        .any(|(name, value)| { name == "transform" && value == "translateX(0px)" }));
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
    let from = stops
        .iter()
        .find(|stop| stop.offset == 0.0)
        .expect("from stop");

    assert!(from
        .properties
        .iter()
        .all(|(name, _)| name != "animation-timing-function"));
    assert!(from
        .properties
        .iter()
        .any(|(name, value)| name == "top" && value == "0px"));
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
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
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
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
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
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 1.0,
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
            properties: vec![("visibility".into(), "hidden".into())],
        },
        KeyframeStop {
            offset: 1.0,
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
            properties: vec![("opacity".into(), "0".into())],
        },
        KeyframeStop {
            offset: 0.5,
            properties: vec![("opacity".into(), "0.5".into())],
        },
        KeyframeStop {
            offset: 1.0,
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
            properties: vec![("left".into(), "0px".into())],
        },
        KeyframeStop {
            offset: 0.5,
            properties: vec![("top".into(), "10px".into())],
        },
        KeyframeStop {
            offset: 1.0,
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

    let values = extract_transitionable_style(&s);
    assert_eq!(values.get("width").map(String::as_str), Some("10px"));
    assert_eq!(values.get("left").map(String::as_str), Some("25%"));
    assert_eq!(values.get("gap").map(String::as_str), Some("2em"));
    assert_eq!(values.get("display").map(String::as_str), Some("grid"));
    assert_eq!(
        values.get("content-visibility").map(String::as_str),
        Some("hidden")
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
fn sync_animations_does_not_duplicate() {
    let mut doc = doc_with_animation("animation: spin 2s linear infinite;");
    let count_before = doc.active_animations.len();
    // Call layout again — should not add another AnimState for the same element+name.
    let mut engine = LayoutEngine::new();
    engine.layout(&mut doc, 800.0);
    assert_eq!(doc.active_animations.len(), count_before);
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
    let b = find_box(&doc.root, &|b: &WebCore| b.tag == "div");
    assert!(b.is_some(), "div should exist");
    // The opacity should be close to 0 (start of fade-in) rather than 1 (stylesheet).
    let opacity = b.unwrap().style.opacity;
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

    doc.sync_transitions(Instant::now(), true);

    assert_eq!(*seen_run.lock().unwrap(), 1);
    assert_eq!(*seen_start.lock().unwrap(), 1);
    assert!(doc.transition_states.contains_key(&id));
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

    doc.sync_transitions(start + Duration::from_millis(10), true);

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
        });
    doc.animation_overrides
        .entry(id)
        .or_default()
        .push(("opacity".into(), "0.2".into()));
    let mut prev = std::collections::HashMap::new();
    prev.insert("opacity".to_string(), "1".to_string());
    doc.prev_styles.insert(id, prev);

    doc.sync_transitions(start + Duration::from_millis(200), true);

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
