//! Driving CSS animations and transitions over time.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::events::DomEvent;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

fn animation_active_duration(anim: &ParsedAnimation) -> f32 {
    if anim.duration_ms <= 0.0 {
        0.0
    } else {
        anim.duration_ms * anim.iteration_count
    }
}

fn animation_event(
    event_type: &str,
    state: &AnimState,
    elapsed_ms: f32,
) -> crate::dom::events::DomEvent {
    let mut event = crate::dom::events::DomEvent::new(event_type, state.element_id);
    event.animation_name = state.animation.name.clone();
    event.elapsed_time = f64::from(elapsed_ms.max(0.0)) / 1000.0;
    event
}

fn sample_animation_events(state: &mut AnimState, delayed_ms: f32, events: &mut Vec<DomEvent>) {
    use AnimationPhase::*;
    let duration = animation_active_duration(&state.animation);
    let phase = if delayed_ms < 0.0 {
        Before
    } else if delayed_ms >= duration {
        After
    } else {
        Active
    };
    let interval_start = (-state.animation.delay_ms).clamp(0.0, duration);
    let iteration = if state.animation.duration_ms > 0.0 {
        (delayed_ms.max(0.0) / state.animation.duration_ms).floor() as u32
    } else {
        0
    };
    // CSS Animations 2 compares consecutive sampled phases, not every missed
    // timeline boundary. Compound phase changes dispatch adjacent event pairs.
    match (state.phase, phase) {
        (Before, Active) => events.push(animation_event("animationstart", state, interval_start)),
        (Before, After) => {
            events.push(animation_event("animationstart", state, interval_start));
            events.push(animation_event("animationend", state, duration));
        }
        (Active, Before) => events.push(animation_event("animationend", state, interval_start)),
        (Active, After) => events.push(animation_event("animationend", state, duration)),
        (After, Active) => events.push(animation_event("animationstart", state, duration)),
        (After, Before) => {
            events.push(animation_event("animationstart", state, duration));
            events.push(animation_event("animationend", state, interval_start));
        }
        (Active, Active) if iteration != state.last_iteration_event => {
            let boundary = if state.last_iteration_event > iteration {
                iteration.saturating_add(1)
            } else {
                iteration
            };
            events.push(animation_event(
                "animationiteration",
                state,
                boundary as f32 * state.animation.duration_ms,
            ));
        }
        _ => {}
    }
    state.phase = phase;
    state.last_iteration_event = iteration;
    state.start_event_fired = phase != Before;
    state.end_event_fired = phase == After;
}

fn transition_event(
    event_type: &str,
    target: u32,
    state: &TransitionState,
    elapsed_ms: f32,
) -> crate::dom::events::DomEvent {
    let mut event = crate::dom::events::DomEvent::new(event_type, target);
    event.property_name = state.property.clone();
    event.elapsed_time = f64::from(elapsed_ms.max(0.0)) / 1000.0;
    event
}

fn transition_cancel_event(
    target: u32,
    state: &TransitionState,
    now: std::time::Instant,
) -> crate::dom::events::DomEvent {
    let elapsed_ms = now
        .saturating_duration_since(state.start_time)
        .as_secs_f32()
        * 1000.0;
    transition_event(
        "transitioncancel",
        target,
        state,
        (elapsed_ms - state.delay_ms).clamp(0.0, state.duration_ms.max(0.0)),
    )
}

fn find_node_by_id<'a>(node: &'a WebCore, id: u32) -> Option<&'a WebCore> {
    if node.node_id == id {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node_by_id(child, id) {
            return Some(found);
        }
    }
    None
}

fn transition_value_at_progress(tr: &TransitionState, progress: f32) -> String {
    let eased = apply_easing(&tr.timing_fn, progress);
    if tr.allow_discrete && is_discrete_transition_property(&tr.property) {
        discrete_transition_value(&tr.property, &tr.from_value, &tr.to_value, eased)
    } else {
        interpolate_property_value(&tr.property, &tr.from_value, &tr.to_value, eased)
    }
}

fn transition_value_at_time(tr: &TransitionState, now: std::time::Instant) -> String {
    let elapsed_ms = now.duration_since(tr.start_time).as_secs_f32() * 1000.0;
    let delayed_ms = elapsed_ms - tr.delay_ms;
    if delayed_ms < 0.0 {
        tr.from_value.clone()
    } else if tr.duration_ms <= 0.0 || delayed_ms >= tr.duration_ms {
        tr.to_value.clone()
    } else {
        transition_value_at_progress(tr, delayed_ms / tr.duration_ms)
    }
}

fn keyframes_with_synthesized_endpoints(
    stops: &[KeyframeStop],
    underlying: &HashMap<String, String>,
) -> Vec<KeyframeStop> {
    if stops.is_empty() {
        return Vec::new();
    }
    let mut out = stops.to_vec();
    let mut animated_props: Vec<String> = Vec::new();
    for stop in stops {
        for (prop, _) in &stop.properties {
            if !animated_props.iter().any(|p| p == prop) {
                animated_props.push(prop.clone());
            }
        }
    }
    if stops.first().map_or(true, |s| s.offset > 0.0) {
        let properties = animated_props
            .iter()
            .filter_map(|prop| {
                underlying
                    .get(prop)
                    .map(|value| (prop.clone(), value.clone()))
            })
            .collect();
        out.insert(
            0,
            KeyframeStop {
                offset: 0.0,
                timing_fn: None,
                properties,
            },
        );
    }
    if stops.last().map_or(true, |s| s.offset < 1.0) {
        let properties = animated_props
            .iter()
            .filter_map(|prop| {
                underlying
                    .get(prop)
                    .map(|value| (prop.clone(), value.clone()))
            })
            .collect();
        out.push(KeyframeStop {
            offset: 1.0,
            timing_fn: None,
            properties,
        });
    }
    // Endpoints are synthesized per property, even when other properties
    // already supplied an explicit 0% or 100% stop.
    for endpoint in [0, out.len() - 1] {
        for prop in &animated_props {
            if !out[endpoint]
                .properties
                .iter()
                .any(|(name, _)| name == prop)
            {
                if let Some(value) = underlying.get(prop) {
                    out[endpoint].properties.push((prop.clone(), value.clone()));
                }
            }
        }
    }
    out
}

fn resolve_keyframe_values_for_element(
    stops: &mut [KeyframeStop],
    node: &WebCore,
    root_font_px: f32,
    viewport_w: f32,
    viewport_h: f32,
) {
    for stop in stops.iter_mut() {
        for (_, value) in &mut stop.properties {
            if value.contains("var(") {
                *value = resolve_var_references_for_color_scheme(
                    value,
                    &node.style.custom_props,
                    &node.style.color_scheme,
                );
            }
        }
    }

    // calc() can combine lengths and percentages, so interpolating its text
    // loses the element's reference box. Resolve both transform endpoints to
    // matrices first; the ordinary transform interpolation path stays intact.
    let has_calculated_transform = stops.iter().any(|stop| {
        stop.properties
            .iter()
            .any(|(property, value)| property == "transform" && value.contains("calc("))
    });
    if !has_calculated_transform {
        return;
    }
    let ctx = TransformCtx {
        font_px: node.style.font_size_px(root_font_px, root_font_px),
        root_font_px,
        viewport_w,
        viewport_h,
    };
    for stop in stops.iter_mut() {
        for (property, value) in &mut stop.properties {
            if property != "transform" {
                continue;
            }
            let Some(transform) = parse_css_transform_checked(value) else {
                continue;
            };
            let mut style = ComputedStyle::default();
            style.css_transform = transform;
            let matrix = crate::renderer::display_list_builder::compute_transform_matrix_raw(
                &style,
                node.layout.border_rect.w,
                node.layout.border_rect.h,
                &ctx,
            );
            *value = format!(
                "matrix({},{},{},{},{},{})",
                matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5]
            );
        }
    }
}

fn compose_animation_properties(
    properties: Vec<(String, String)>,
    underlying: &HashMap<String, String>,
    composition: &AnimationComposition,
) -> Vec<(String, String)> {
    if matches!(composition, AnimationComposition::Replace) {
        return properties;
    }
    properties
        .into_iter()
        .map(|(prop, value)| {
            let composed = compose_animation_property(&prop, &value, underlying, composition)
                .unwrap_or_else(|| value.clone());
            (prop, composed)
        })
        .collect()
}

fn animation_underlying_style(
    node: &WebCore,
    preceding_effects: Option<&Vec<(String, String)>>,
) -> HashMap<String, String> {
    let mut underlying = extract_transitionable_style(&node.style);
    if let Some(effects) = preceding_effects {
        underlying.extend(effects.iter().cloned());
    }
    underlying
}

fn store_animation_properties(
    target: &mut Vec<(String, String)>,
    properties: impl IntoIterator<Item = (String, String)>,
) {
    for (property, value) in properties {
        if let Some((_, previous)) = target.iter_mut().find(|(name, _)| *name == property) {
            *previous = value;
        } else {
            target.push((property, value));
        }
    }
}

fn compose_animation_property(
    prop: &str,
    value: &str,
    underlying: &HashMap<String, String>,
    composition: &AnimationComposition,
) -> Option<String> {
    if prop == "transform" && matches!(composition, AnimationComposition::Add) {
        let base = underlying.get(prop)?.trim();
        let effect = value.trim();
        parse_css_transform_checked(base)?;
        parse_css_transform_checked(effect)?;
        // CSS Transforms 2 defines addition as ordered list concatenation.
        return Some(if base.eq_ignore_ascii_case("none") || base.is_empty() {
            effect.to_string()
        } else if effect.eq_ignore_ascii_case("none") || effect.is_empty() {
            base.to_string()
        } else {
            format!("{base} {effect}")
        });
    }
    if !matches!(
        prop,
        "opacity" | "fill-opacity" | "stroke-opacity" | "stop-opacity"
    ) {
        return None;
    }
    let underlying = underlying.get(prop)?;
    let base = parse_css_alpha(underlying)?;
    let effect = parse_css_alpha(value)?;
    let composed = match composition {
        AnimationComposition::Replace => return None,
        AnimationComposition::Add | AnimationComposition::Accumulate => base + effect,
    };
    Some(format_css_number(composed.clamp(0.0, 1.0)))
}

impl Document {
    /// Walk the tree and ensure an `AnimState` exists for every element that
    /// currently has an `animation` property.  Call this after each cascade pass.
    pub fn sync_animations(&mut self, now: std::time::Instant) {
        let mut current: Vec<(u32, ParsedAnimation)> = Vec::new();
        let mut started_events: Vec<Vec<DomEvent>> = Vec::new();
        let mut cancelled_events = Vec::new();
        fn collect(node: &WebCore, out: &mut Vec<(u32, ParsedAnimation)>) {
            if node.style.display == Display::None {
                return;
            }
            let id = node.node_id;
            for a in &node.style.rare().animations {
                out.push((id, a.clone()));
            }
            for child in &node.children {
                collect(child, out);
            }
        }
        collect(&self.root, &mut current);

        current.retain(|(_, anim)| {
            !anim.name.is_empty()
                && anim.name != "none"
                && self.stylesheet.keyframes.contains_key(&anim.name)
        });

        // CSS Animations 1 matches the new list from last to first, consuming
        // the last old match once. Repeated names remain distinct animations.
        self.restore_finished_animations();
        let mut previous: Vec<Option<AnimState>> = std::mem::take(&mut self.active_animations)
            .into_iter()
            .map(Some)
            .collect();
        let mut matches: HashMap<u32, HashMap<String, Vec<usize>>> = HashMap::new();
        for (index, state) in previous.iter().enumerate() {
            let state = state.as_ref().unwrap();
            matches
                .entry(state.element_id)
                .or_default()
                .entry(state.animation.name.clone())
                .or_default()
                .push(index);
        }
        let mut updated = Vec::with_capacity(current.len());
        for (list_order, (id, anim)) in current.iter().enumerate().rev() {
            let existing = matches
                .get_mut(id)
                .and_then(|names| names.get_mut(&anim.name))
                .and_then(Vec::pop)
                .and_then(|index| previous[index].take());
            let mut state = if let Some(state) = existing {
                state
            } else {
                let mut state = AnimState {
                    element_id: *id,
                    animation: anim.clone(),
                    start_time: now,
                    paused_at: anim.play_state_paused.then_some(now),
                    start_event_fired: false,
                    last_iteration_event: 0,
                    list_order,
                    end_event_fired: false,
                    phase: AnimationPhase::Before,
                };
                let mut initial_events = Vec::new();
                sample_animation_events(&mut state, -anim.delay_ms, &mut initial_events);
                if !initial_events.is_empty() {
                    started_events.push(initial_events);
                }
                state
            };
            let was_paused = state.animation.play_state_paused;
            let is_paused = anim.play_state_paused;
            match (was_paused, is_paused, state.paused_at) {
                (false, true, _) => {
                    state.paused_at = Some(now);
                }
                (true, false, Some(paused_at)) => {
                    if let Some(paused_duration) = now.checked_duration_since(paused_at) {
                        state.start_time += paused_duration;
                    }
                    state.paused_at = None;
                }
                (true, true, None) => {
                    state.paused_at = Some(now);
                }
                _ => {}
            }
            state.animation = anim.clone();
            state.list_order = list_order;
            updated.push(state);
        }
        updated.reverse();
        started_events.reverse();
        (self.finished_animations, self.active_animations) =
            updated.into_iter().partition(|state| state.end_event_fired);

        for s in previous.into_iter().flatten() {
            if s.end_event_fired {
                continue;
            }
            let sample_now = s.paused_at.unwrap_or(now);
            let elapsed_ms = sample_now
                .saturating_duration_since(s.start_time)
                .as_secs_f32()
                * 1000.0;
            cancelled_events.push(animation_event(
                "animationcancel",
                &s,
                (elapsed_ms - s.animation.delay_ms)
                    .clamp(0.0, animation_active_duration(&s.animation)),
            ));
        }

        for mut event in cancelled_events
            .into_iter()
            .chain(started_events.into_iter().flatten())
        {
            self.dispatch_dom_event(&mut event);
        }
    }

    fn restore_finished_animations(&mut self) {
        if !self.finished_animations.is_empty() {
            self.active_animations.append(&mut self.finished_animations);
            self.active_animations.sort_by_key(|state| state.list_order);
        }
    }

    /// Detect CSS property changes caused by the cascade and start transitions.
    pub fn sync_transitions(&mut self, now: std::time::Instant) {
        let mut current = Vec::new();
        let mut started_events = Vec::new();
        let mut cancelled_events = Vec::new();
        fn collect(
            node: &WebCore,
            previous: &HashMap<u32, std::sync::Arc<ComputedStyle>>,
            out: &mut Vec<(
                u32,
                Vec<ParsedTransition>,
                Option<(std::sync::Arc<ComputedStyle>, HashMap<String, String>)>,
            )>,
        ) {
            let id = node.node_id;
            if !node.style.rare().transitions.is_empty() {
                // `node.style` is already the active cascade target. Hover
                // invalidation swaps `style` and `hover_style`, so `hover_style`
                // stores the inactive side; overlaying it here reverses hover
                // transitions and leaves stale target values behind.
                let vals = previous
                    .get(&id)
                    .is_none_or(|style| !std::sync::Arc::ptr_eq(style, &node.style))
                    .then(|| (node.style.clone(), extract_transitionable(node)));
                out.push((id, node.style.rare().transitions.clone(), vals));
            }
            for child in &node.children {
                collect(child, previous, out);
            }
        }
        collect(&self.root, &self.transition_style_refs, &mut current);

        let definitions_by_id: HashMap<u32, &[ParsedTransition]> = current
            .iter()
            .map(|(id, transitions, _)| (*id, transitions.as_slice()))
            .collect();
        self.transition_states.retain(|elem_id, states| {
            let definitions = definitions_by_id.get(elem_id).copied();
            states.retain(|state| {
                let matches_property = definitions.is_some_and(|transitions| {
                    transitions
                        .iter()
                        .any(|tr| tr.property == "all" || tr.property == state.property)
                });
                if !matches_property {
                    cancelled_events.push(transition_cancel_event(*elem_id, state, now));
                }
                matches_property
            });
            !states.is_empty()
        });
        self.prev_styles
            .retain(|id, _| definitions_by_id.contains_key(id));
        self.transition_style_refs
            .retain(|id, _| definitions_by_id.contains_key(id));

        for (elem_id, trs, values) in current {
            let Some((style, cur_vals)) = values else {
                continue;
            };
            let prev = self.prev_styles.get(&elem_id);

            for tr in trs {
                let props: Vec<&str> = if tr.property == "all" {
                    cur_vals.keys().map(|s| s.as_str()).collect()
                } else {
                    vec![tr.property.as_str()]
                };

                for prop in props {
                    let cur = match cur_vals.get(prop) {
                        Some(v) => v.as_str(),
                        None => continue,
                    };
                    let prv = match prev.and_then(|values| values.get(prop)) {
                        Some(v) => v.as_str(),
                        None => {
                            continue;
                        }
                    };
                    if prv == cur {
                        // Uncomment to debug: eprintln!("[TR-SKIP] {} same={:?}", prop, cur);
                        continue;
                    }
                    // CSS Transitions starts a transition only when its combined
                    // duration is positive; a positive delay can outlive a zero duration.
                    if tr.duration_ms.max(0.0) + tr.delay_ms <= 0.0 {
                        if let Some(states) = self.transition_states.get_mut(&elem_id) {
                            states.retain(|state| {
                                if state.property == prop {
                                    cancelled_events
                                        .push(transition_cancel_event(elem_id, state, now));
                                    false
                                } else {
                                    true
                                }
                            });
                        }
                        continue;
                    }
                    if is_discrete_transition_property(prop) && !tr.allow_discrete {
                        continue;
                    }

                    // Already transitioning to this value?
                    let already = self
                        .transition_states
                        .entry(elem_id)
                        .or_default()
                        .iter()
                        .any(|t| t.property == prop && t.to_value == cur);
                    if already {
                        continue;
                    }

                    // If a transition is already running for this property, start the
                    // new one from the current animated value (not from prev_styles) to
                    // avoid a visual jump to the original from/to endpoint.
                    let replaced = self
                        .transition_states
                        .get(&elem_id)
                        .and_then(|states| states.iter().find(|t| t.property == prop))
                        .cloned();
                    let from_val = replaced
                        .as_ref()
                        .map(|old| transition_value_at_time(old, now))
                        .or_else(|| {
                            self.animation_overrides
                                .get(&elem_id)
                                .and_then(|ov| ov.iter().find(|(p, _)| p == prop))
                                .map(|(_, v)| v.clone())
                        })
                        .unwrap_or_else(|| prv.to_string());
                    if replaced.is_some() && from_val == cur {
                        if let Some(states) = self.transition_states.get_mut(&elem_id) {
                            states.retain(|state| state.property != prop);
                        }
                        cancelled_events.push(transition_cancel_event(
                            elem_id,
                            replaced.as_ref().unwrap(),
                            now,
                        ));
                        continue;
                    }
                    let entry = self.transition_states.entry(elem_id).or_default();
                    let mut duration_ms = tr.duration_ms;
                    let mut delay_ms = tr.delay_ms;
                    let mut reversing_adjusted_start_value = from_val.clone();
                    let mut reversing_shortening_factor = 1.0;
                    if let Some(old) = &replaced {
                        if cur == old.reversing_adjusted_start_value && old.duration_ms > 0.0 {
                            let elapsed = now.duration_since(old.start_time).as_secs_f32() * 1000.0;
                            let progress =
                                ((elapsed - old.delay_ms) / old.duration_ms).clamp(0.0, 1.0);
                            // CSS Transitions: travelled value distance, including the
                            // untravelled portion from any earlier reversal.
                            reversing_shortening_factor = (apply_easing(&old.timing_fn, progress)
                                * old.reversing_shortening_factor
                                + (1.0 - old.reversing_shortening_factor))
                                .abs()
                                .clamp(0.0, 1.0);
                            reversing_adjusted_start_value = old.to_value.clone();
                            duration_ms = tr.duration_ms * reversing_shortening_factor;
                            if delay_ms < 0.0 {
                                delay_ms *= reversing_shortening_factor;
                            }
                        }
                    }
                    entry.retain(|t| t.property != prop);
                    if let Some(old) = &replaced {
                        cancelled_events.push(transition_cancel_event(elem_id, old, now));
                    }
                    if duration_ms.max(0.0) + delay_ms <= 0.0 {
                        continue;
                    }
                    entry.push(TransitionState {
                        property: prop.to_string(),
                        from_value: from_val,
                        to_value: cur.to_string(),
                        reversing_adjusted_start_value,
                        reversing_shortening_factor,
                        start_time: now,
                        duration_ms,
                        delay_ms,
                        timing_fn: tr.timing_fn.clone(),
                        allow_discrete: tr.allow_discrete,
                        start_event_fired: delay_ms <= 0.0,
                    });
                    let state = entry.last().unwrap();
                    let start_elapsed = (-delay_ms).clamp(0.0, duration_ms.max(0.0));
                    started_events.push(transition_event(
                        "transitionrun",
                        elem_id,
                        state,
                        start_elapsed,
                    ));
                    if delay_ms <= 0.0 {
                        started_events.push(transition_event(
                            "transitionstart",
                            elem_id,
                            state,
                            start_elapsed,
                        ));
                    }
                }
            }
            self.prev_styles.insert(elem_id, cur_vals);
            self.transition_style_refs.insert(elem_id, style);
        }
        self.transition_states
            .retain(|_, states| !states.is_empty());

        for mut event in cancelled_events.into_iter().chain(started_events) {
            self.dispatch_dom_event(&mut event);
        }
    }

    /// Advance all running animations and transitions to time `now`.
    /// Populates `animation_overrides` with interpolated CSS values.
    /// Sets `needs_animation_frame = true` if any animation/transition is still running.
    pub fn tick_animations(&mut self, now: std::time::Instant) -> bool {
        self.restore_finished_animations();
        let previous_overrides = std::mem::take(&mut self.animation_overrides);
        let keyframes = &self.stylesheet.keyframes;
        let mut still_running = false;
        let mut finished_events = Vec::new();
        let mut animation_events = Vec::new();

        // ── CSS Animations ───────────────────────────────────────────────────
        for state in self.active_animations.iter_mut() {
            let paused = state.animation.play_state_paused;
            let sample_now = if paused {
                state.paused_at.unwrap_or(now)
            } else {
                now
            };
            let elapsed_ms = sample_now.duration_since(state.start_time).as_secs_f32() * 1000.0;
            let delayed_ms = elapsed_ms - state.animation.delay_ms;
            sample_animation_events(state, delayed_ms, &mut animation_events);

            if delayed_ms < 0.0 {
                // Delay phase: apply backwards fill if needed.
                if matches!(
                    state.animation.fill_mode,
                    FillMode::Backwards | FillMode::Both
                ) {
                    if let Some(kf) = keyframes.get(&state.animation.name) {
                        if let Some(node) = find_node_by_id(&self.root, state.element_id) {
                            let underlying = animation_underlying_style(
                                node,
                                self.animation_overrides.get(&state.element_id),
                            );
                            let mut stops = keyframes_with_synthesized_endpoints(kf, &underlying);
                            let initial_font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
                            let root_font_px = self
                                .root
                                .style
                                .font_size_px(initial_font_px, initial_font_px);
                            resolve_keyframe_values_for_element(
                                &mut stops,
                                node,
                                root_font_px,
                                self.viewport_w,
                                self.viewport_h,
                            );
                            let endpoint = if matches!(
                                state.animation.direction,
                                AnimDirection::Reverse | AnimDirection::AlternateReverse
                            ) {
                                stops.last()
                            } else {
                                stops.first()
                            };
                            // During the delay, fill uses the directed endpoint without easing.
                            let properties = endpoint
                                .map(|stop| stop.properties.clone())
                                .unwrap_or_default();
                            let entry = self
                                .animation_overrides
                                .entry(state.element_id)
                                .or_default();
                            store_animation_properties(
                                entry,
                                compose_animation_properties(
                                    properties,
                                    &underlying,
                                    &state.animation.composition,
                                ),
                            );
                        }
                    }
                }
                if !paused {
                    still_running = true;
                }
                continue;
            }
            let duration = state.animation.duration_ms;
            let total_progress = if duration > 0.0 {
                delayed_ms / duration
            } else {
                0.0
            };
            let iteration = total_progress.floor();
            let mut sample_iteration = iteration;
            let mut t_frac = total_progress.fract();
            let iteration_count = state.animation.iteration_count;

            if duration <= 0.0
                || (!iteration_count.is_infinite() && delayed_ms >= duration * iteration_count)
            {
                // Finished: apply forwards fill if needed.
                if matches!(
                    state.animation.fill_mode,
                    FillMode::Forwards | FillMode::Both
                ) {
                    if let Some(kf) = keyframes.get(&state.animation.name) {
                        let underlying = find_node_by_id(&self.root, state.element_id)
                            .map(|node| {
                                animation_underlying_style(
                                    node,
                                    self.animation_overrides.get(&state.element_id),
                                )
                            })
                            .unwrap_or_default();
                        let mut stops = keyframes_with_synthesized_endpoints(kf, &underlying);
                        if let Some(node) = find_node_by_id(&self.root, state.element_id) {
                            let initial_font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
                            let root_font_px = self
                                .root
                                .style
                                .font_size_px(initial_font_px, initial_font_px);
                            resolve_keyframe_values_for_element(
                                &mut stops,
                                node,
                                root_font_px,
                                self.viewport_w,
                                self.viewport_h,
                            );
                        }
                        // A zero-duration infinite effect still has a finite terminal sample.
                        let terminal_count = if duration <= 0.0 && iteration_count.is_infinite() {
                            1.0
                        } else {
                            iteration_count
                        };
                        let endpoint_frac = terminal_count.fract();
                        let final_iteration = if endpoint_frac == 0.0 {
                            (terminal_count - 1.0).max(0.0).floor()
                        } else {
                            terminal_count.floor()
                        };
                        let base_t = if endpoint_frac == 0.0 {
                            1.0
                        } else {
                            endpoint_frac
                        };
                        let final_t = match state.animation.direction {
                            AnimDirection::Normal => base_t,
                            AnimDirection::Reverse => 1.0 - base_t,
                            AnimDirection::Alternate => {
                                if (final_iteration as u32) % 2 == 0 {
                                    base_t
                                } else {
                                    1.0 - base_t
                                }
                            }
                            AnimDirection::AlternateReverse => {
                                if (final_iteration as u32) % 2 == 0 {
                                    1.0 - base_t
                                } else {
                                    base_t
                                }
                            }
                        };
                        let props = compose_animation_properties(
                            interpolate_keyframe_stops_with_easing(
                                &stops,
                                final_t,
                                &state.animation.timing_fn,
                            ),
                            &underlying,
                            &state.animation.composition,
                        );
                        let entry = self
                            .animation_overrides
                            .entry(state.element_id)
                            .or_default();
                        store_animation_properties(entry, props);
                    }
                }
                continue;
            }
            if !paused {
                still_running = true;
            }

            // Finite animations need to expose the completed iteration at an
            // exact boundary. Infinite animations, however, must remain a
            // continuous loop: sampling the completed 100% frame can make
            // marquees dwell at the seam and show an empty tail.
            if !iteration_count.is_infinite() && total_progress > 0.0 && t_frac <= 0.0001 {
                t_frac = 1.0;
                sample_iteration = (iteration - 1.0).max(0.0);
            }

            let effective_t = match state.animation.direction {
                AnimDirection::Normal => t_frac,
                AnimDirection::Reverse => 1.0 - t_frac,
                AnimDirection::Alternate => {
                    if (sample_iteration as u32) % 2 == 0 {
                        t_frac
                    } else {
                        1.0 - t_frac
                    }
                }
                AnimDirection::AlternateReverse => {
                    if (sample_iteration as u32) % 2 == 0 {
                        1.0 - t_frac
                    } else {
                        t_frac
                    }
                }
            };

            if let Some(kf) = keyframes.get(&state.animation.name) {
                let underlying = find_node_by_id(&self.root, state.element_id)
                    .map(|node| {
                        animation_underlying_style(
                            node,
                            self.animation_overrides.get(&state.element_id),
                        )
                    })
                    .unwrap_or_default();
                let mut stops = keyframes_with_synthesized_endpoints(kf, &underlying);
                if let Some(node) = find_node_by_id(&self.root, state.element_id) {
                    let initial_font_px = ComputedStyle::INITIAL_FONT_SIZE_PX;
                    let root_font_px = self
                        .root
                        .style
                        .font_size_px(initial_font_px, initial_font_px);
                    resolve_keyframe_values_for_element(
                        &mut stops,
                        node,
                        root_font_px,
                        self.viewport_w,
                        self.viewport_h,
                    );
                }
                let props = compose_animation_properties(
                    interpolate_keyframe_stops_with_easing(
                        &stops,
                        effective_t,
                        &state.animation.timing_fn,
                    ),
                    &underlying,
                    &state.animation.composition,
                );
                let entry = self
                    .animation_overrides
                    .entry(state.element_id)
                    .or_default();
                store_animation_properties(entry, props);
            }
        }
        (self.finished_animations, self.active_animations) =
            std::mem::take(&mut self.active_animations)
                .into_iter()
                .partition(|state| state.end_event_fired);
        for mut event in animation_events {
            self.dispatch_dom_event(&mut event);
        }

        // ── CSS Transitions ──────────────────────────────────────────────────
        let mut empty_elems: Vec<u32> = Vec::new();
        let mut transition_started_events = Vec::new();
        for (elem_id, trs) in &mut self.transition_states {
            let mut done_trs: Vec<usize> = Vec::new();
            for (i, tr) in trs.iter_mut().enumerate() {
                let elapsed_ms = now.duration_since(tr.start_time).as_secs_f32() * 1000.0;
                let delayed_ms = elapsed_ms - tr.delay_ms;

                if delayed_ms < 0.0 {
                    // Apply "from" value during delay.
                    let entry = self.animation_overrides.entry(*elem_id).or_default();
                    store_animation_properties(
                        entry,
                        [(tr.property.clone(), tr.from_value.clone())],
                    );
                    still_running = true;
                    continue;
                }
                if !tr.start_event_fired {
                    tr.start_event_fired = true;
                    transition_started_events.push(transition_event(
                        "transitionstart",
                        *elem_id,
                        tr,
                        (-tr.delay_ms).clamp(0.0, tr.duration_ms.max(0.0)),
                    ));
                }
                if tr.duration_ms <= 0.0 {
                    store_animation_properties(
                        self.animation_overrides.entry(*elem_id).or_default(),
                        [(tr.property.clone(), tr.to_value.clone())],
                    );
                    finished_events.push(transition_event("transitionend", *elem_id, tr, 0.0));
                    done_trs.push(i);
                    continue;
                }

                let progress = (delayed_ms / tr.duration_ms).min(1.0);
                if progress >= 1.0 {
                    // Write the final value into animation_overrides so that
                    // transitioning_ids still contains this element for the
                    // completion frame.  Without this, has_transition becomes
                    // false while is_hovered may still be true, causing the
                    // renderer to pick hover_style's color instead of the
                    // correctly-reverted base color.
                    let entry = self.animation_overrides.entry(*elem_id).or_default();
                    store_animation_properties(entry, [(tr.property.clone(), tr.to_value.clone())]);
                    finished_events.push(transition_event(
                        "transitionend",
                        *elem_id,
                        tr,
                        tr.duration_ms,
                    ));
                    done_trs.push(i);
                    continue;
                }

                still_running = true;
                let interp = transition_value_at_progress(tr, progress);
                let entry = self.animation_overrides.entry(*elem_id).or_default();
                store_animation_properties(entry, [(tr.property.clone(), interp)]);
            }
            for idx in done_trs.into_iter().rev() {
                trs.remove(idx);
            }
            if trs.is_empty() {
                empty_elems.push(*elem_id);
            }
        }
        for eid in empty_elems {
            self.transition_states.remove(&eid);
        }
        for mut event in transition_started_events {
            self.dispatch_dom_event(&mut event);
        }

        self.needs_animation_frame = still_running;

        // Mark elements with layout-affecting active overrides as layout_dirty.
        // Paint/compositor-only animations (transform, opacity, stroke, etc.)
        // still need repaint, but forcing full geometry for every shimmer/spinner
        // makes large pages repaint continuously.
        let overrides_changed = self.animation_overrides != previous_overrides;
        if overrides_changed {
            let current_layout = layout_animation_values(&self.animation_overrides);
            let previous_layout = layout_animation_values(&previous_overrides);
            fn mark_dirty(
                node: &mut WebCore,
                current: &HashMap<u32, Vec<(String, String)>>,
                previous: &HashMap<u32, Vec<(String, String)>>,
            ) {
                if current.get(&node.node_id) != previous.get(&node.node_id) {
                    node.layout.layout_dirty = true;
                }
                for child in &mut node.children {
                    mark_dirty(child, current, previous);
                }
            }
            if current_layout != previous_layout {
                mark_dirty(&mut self.root, &current_layout, &previous_layout);
            }
        }

        for mut event in finished_events {
            self.dispatch_dom_event(&mut event);
        }
        overrides_changed
    }
}

#[cfg(test)]
pub(crate) fn animation_properties_affect_layout(props: &[(String, String)]) -> bool {
    props
        .iter()
        .any(|(prop, _)| animation_property_affects_layout(prop))
}

pub(crate) fn layout_animation_values(
    overrides: &HashMap<u32, Vec<(String, String)>>,
) -> HashMap<u32, Vec<(String, String)>> {
    overrides
        .iter()
        .filter_map(|(&id, props)| {
            let values = props
                .iter()
                .filter(|(prop, _)| animation_property_affects_layout(prop))
                .cloned()
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some((id, values))
        })
        .collect()
}

pub(crate) fn animation_property_affects_layout(prop: &str) -> bool {
    if animation_property_is_transform(prop) {
        return false;
    }
    !matches!(
        prop,
        "opacity"
            | "transform"
            | "filter"
            | "backdrop-filter"
            | "visibility"
            | "color"
            | "background"
            | "background-color"
            | "background-position"
            | "background-position-x"
            | "background-position-y"
            | "clip-path"
            | "border-color"
            | "border-top-color"
            | "border-right-color"
            | "border-bottom-color"
            | "border-left-color"
            | "outline-color"
            | "text-decoration-color"
            | "box-shadow"
            | "text-shadow"
            | "fill"
            | "fill-opacity"
            | "stroke"
            | "stroke-opacity"
            | "stroke-width"
            | "stroke-dasharray"
            | "stroke-dashoffset"
            | "stop-color"
            | "stop-opacity"
    )
}

pub(crate) fn animation_property_is_transform(prop: &str) -> bool {
    matches!(
        prop,
        "transform" | "-webkit-transform" | "-moz-transform" | "-ms-transform"
    )
}

#[cfg(test)]
mod tests {
    use super::animation_property_affects_layout;

    #[test]
    fn clip_path_animation_only_changes_paint() {
        assert!(!animation_property_affects_layout("clip-path"));
        assert!(!animation_property_affects_layout("transform"));
        assert!(!animation_property_affects_layout("-webkit-transform"));
        assert!(!animation_property_affects_layout("-ms-transform"));
        assert!(animation_property_affects_layout("width"));
    }
}
