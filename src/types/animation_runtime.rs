//! Driving CSS animations and transitions over time.

#![allow(unused_imports)]
use super::*;
use crate::css::*;
use crate::dom::*;
use crate::html::*;
use crate::layout::LayoutEngine;
use std::collections::{HashMap, HashSet};

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
            properties,
        });
    }
    out
}

impl Document {
    /// Walk the tree and ensure an `AnimState` exists for every element that
    /// currently has an `animation` property.  Call this after each cascade pass.
    pub fn sync_animations(&mut self, now: std::time::Instant) {
        let mut current: Vec<(u32, ParsedAnimation)> = Vec::new();
        let mut started_events: Vec<u32> = Vec::new();
        fn collect(node: &WebCore, out: &mut Vec<(u32, ParsedAnimation)>) {
            let id = node.node_id;
            for a in &node.style.rare().animations {
                out.push((id, a.clone()));
            }
            for child in &node.children {
                collect(child, out);
            }
        }
        collect(&self.root, &mut current);

        // Start animations that aren't tracked yet.
        for (id, anim) in &current {
            let running = self
                .active_animations
                .iter()
                .any(|s| s.element_id == *id && s.animation.name == anim.name);
            if !running && !anim.name.is_empty() && anim.name != "none" {
                self.active_animations.push(AnimState {
                    element_id: *id,
                    animation: anim.clone(),
                    start_time: now,
                    last_iteration_event: 0,
                });
                if anim.delay_ms <= 0.0 {
                    started_events.push(*id);
                }
            }
        }

        // Remove animations whose element no longer carries that animation name.
        self.active_animations.retain(|s| {
            current
                .iter()
                .any(|(id, a)| *id == s.element_id && a.name == s.animation.name)
        });

        for target in started_events {
            let mut event = crate::dom::events::DomEvent::new("animationstart", target);
            self.dispatch_dom_event(&mut event);
        }
    }

    /// Detect CSS property changes caused by the cascade and start transitions.
    /// `cascade_ran`: true when the full cascade just ran (node.style is clean).
    /// When false (hover-only change), base values are read from `cascade_styles`
    /// so animation-overridden node.style values don't pollute change detection.
    pub fn sync_transitions(&mut self, now: std::time::Instant, cascade_ran: bool) {
        let hovered = self.hovered_box;
        let mut current: Vec<(u32, Vec<ParsedTransition>, HashMap<String, String>)> = Vec::new();
        let mut started_events: Vec<(u32, bool)> = Vec::new();
        let mut cancelled_events: Vec<u32> = Vec::new();
        fn collect(
            node: &WebCore,
            hovered: u32,
            cascade_ran: bool,
            cascade_styles: &HashMap<u32, HashMap<String, String>>,
            out: &mut Vec<(u32, Vec<ParsedTransition>, HashMap<String, String>)>,
        ) {
            let id = node.node_id;
            if !node.style.rare().transitions.is_empty() {
                // Base values: use the clean cascade snapshot when available, so that
                // animation_overrides applied to node.style don't corrupt detection.
                let base = if cascade_ran {
                    extract_transitionable(node)
                } else {
                    cascade_styles
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| extract_transitionable(node))
                };
                let mut vals = base.clone();
                // When hovered, overlay hover_style to get the "target" state.
                if hovered != 0 && subtree_contains_id(node, hovered) {
                    if let Some(hs) = &node.style.hover_style {
                        let hover_vals = extract_transitionable_style(hs);
                        for (k, v) in hover_vals {
                            vals.insert(k, v);
                        }
                    }
                }
                out.push((id, node.style.rare().transitions.clone(), vals));
            }
            for child in &node.children {
                collect(child, hovered, cascade_ran, cascade_styles, out);
            }
        }
        collect(
            &self.root,
            hovered,
            cascade_ran,
            &self.cascade_styles,
            &mut current,
        );

        // When cascade ran, save the clean base styles for hover-only frames.
        if cascade_ran {
            fn snapshot(node: &WebCore, out: &mut HashMap<u32, HashMap<String, String>>) {
                if !node.style.rare().transitions.is_empty() {
                    out.insert(node.node_id, extract_transitionable(node));
                }
                for child in &node.children {
                    snapshot(child, out);
                }
            }
            snapshot(&self.root, &mut self.cascade_styles);
        }

        for (elem_id, trs, cur_vals) in &current {
            let prev = self.prev_styles.get(elem_id).cloned().unwrap_or_default();

            for tr in trs {
                if tr.duration_ms <= 0.0 {
                    continue;
                }
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
                    let prv = match prev.get(prop) {
                        Some(v) => v.as_str(),
                        None => {
                            continue;
                        }
                    };
                    if prv == cur {
                        // Uncomment to debug: eprintln!("[TR-SKIP] {} same={:?}", prop, cur);
                        continue;
                    }
                    if is_discrete_transition_property(prop) && !tr.allow_discrete {
                        continue;
                    }

                    // Already transitioning to this value?
                    let already = self
                        .transition_states
                        .entry(*elem_id)
                        .or_default()
                        .iter()
                        .any(|t| t.property == prop && t.to_value == cur);
                    if already {
                        continue;
                    }

                    // If a transition is already running for this property, start the
                    // new one from the current animated value (not from prev_styles) to
                    // avoid a visual jump to the original from/to endpoint.
                    let from_val = self
                        .animation_overrides
                        .get(elem_id)
                        .and_then(|ov| ov.iter().find(|(p, _)| p == prop))
                        .map(|(_, v)| v.as_str())
                        .unwrap_or(prv);
                    let entry = self.transition_states.entry(*elem_id).or_default();
                    let replaced = entry.iter().find(|t| t.property == prop).cloned();
                    let mut duration_ms = tr.duration_ms;
                    let mut reversing_adjusted_start_value = prv.to_string();
                    let mut reversing_shortening_factor = 1.0;
                    if let Some(old) = &replaced {
                        reversing_adjusted_start_value = old.from_value.clone();
                        if cur == old.reversing_adjusted_start_value && old.duration_ms > 0.0 {
                            let elapsed = now.duration_since(old.start_time).as_secs_f32() * 1000.0;
                            let progress =
                                ((elapsed - old.delay_ms) / old.duration_ms).clamp(0.0, 1.0);
                            reversing_shortening_factor =
                                (progress * old.reversing_shortening_factor).clamp(0.0, 1.0);
                            duration_ms = tr.duration_ms * reversing_shortening_factor;
                        }
                    }
                    let before_replace = entry.len();
                    entry.retain(|t| t.property != prop);
                    if entry.len() != before_replace {
                        cancelled_events.push(*elem_id);
                    }
                    entry.push(TransitionState {
                        property: prop.to_string(),
                        from_value: from_val.to_string(),
                        to_value: cur.to_string(),
                        reversing_adjusted_start_value,
                        reversing_shortening_factor,
                        start_time: now,
                        duration_ms,
                        delay_ms: tr.delay_ms,
                        timing_fn: tr.timing_fn.clone(),
                        allow_discrete: tr.allow_discrete,
                    });
                    started_events.push((*elem_id, tr.delay_ms <= 0.0));
                }
            }
            self.prev_styles.insert(*elem_id, cur_vals.clone());
        }

        for target in cancelled_events {
            let mut cancel = crate::dom::events::DomEvent::new("transitioncancel", target);
            self.dispatch_dom_event(&mut cancel);
        }
        for (target, start_now) in started_events {
            let mut run = crate::dom::events::DomEvent::new("transitionrun", target);
            self.dispatch_dom_event(&mut run);
            if start_now {
                let mut start = crate::dom::events::DomEvent::new("transitionstart", target);
                self.dispatch_dom_event(&mut start);
            }
        }
    }

    /// Advance all running animations and transitions to time `now`.
    /// Populates `animation_overrides` with interpolated CSS values.
    /// Sets `needs_animation_frame = true` if any animation/transition is still running.
    pub fn tick_animations(&mut self, now: std::time::Instant) {
        self.animation_overrides.clear();
        let keyframes = self.stylesheet.keyframes.clone();
        let mut still_running = false;
        let mut finished_events: Vec<(&'static str, u32)> = Vec::new();
        let mut iteration_events: Vec<u32> = Vec::new();

        // ── CSS Animations ───────────────────────────────────────────────────
        let mut done: Vec<usize> = Vec::new();
        for (idx, state) in self.active_animations.iter_mut().enumerate() {
            if state.animation.play_state_paused {
                if matches!(
                    state.animation.fill_mode,
                    FillMode::Backwards | FillMode::Both
                ) {
                    if let Some(kf) = keyframes.get(&state.animation.name) {
                        if let Some(first) = kf.first() {
                            let entry = self
                                .animation_overrides
                                .entry(state.element_id)
                                .or_default();
                            entry.extend(first.properties.clone());
                        }
                    }
                }
                continue;
            }

            let elapsed_ms = now.duration_since(state.start_time).as_secs_f32() * 1000.0;
            let delayed_ms = elapsed_ms - state.animation.delay_ms;

            if delayed_ms < 0.0 {
                // Delay phase: apply backwards fill if needed.
                if matches!(
                    state.animation.fill_mode,
                    FillMode::Backwards | FillMode::Both
                ) {
                    if let Some(kf) = keyframes.get(&state.animation.name) {
                        if let Some(first) = kf.first() {
                            let entry = self
                                .animation_overrides
                                .entry(state.element_id)
                                .or_default();
                            entry.extend(first.properties.clone());
                        }
                    }
                }
                still_running = true;
                continue;
            }

            let duration = state.animation.duration_ms;
            if duration <= 0.0 {
                done.push(idx);
                continue;
            }

            let total_progress = delayed_ms / duration;
            let iteration = total_progress.floor();
            let t_frac = total_progress.fract();
            let iteration_count = state.animation.iteration_count;
            let completed_iterations = iteration as u32;

            if !iteration_count.is_infinite() && delayed_ms >= duration * iteration_count {
                // Finished: apply forwards fill if needed.
                if matches!(
                    state.animation.fill_mode,
                    FillMode::Forwards | FillMode::Both
                ) {
                    if let Some(kf) = keyframes.get(&state.animation.name) {
                        let underlying = find_node_by_id(&self.root, state.element_id)
                            .map(|node| extract_transitionable_style(&node.style))
                            .unwrap_or_default();
                        let stops = keyframes_with_synthesized_endpoints(kf, &underlying);
                        let endpoint_frac = iteration_count.fract();
                        let final_iteration = if endpoint_frac == 0.0 {
                            (iteration_count - 1.0).max(0.0).floor()
                        } else {
                            iteration_count.floor()
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
                        let props = interpolate_keyframe_stops(&stops, final_t);
                        let entry = self
                            .animation_overrides
                            .entry(state.element_id)
                            .or_default();
                        entry.extend(props);
                    }
                }
                finished_events.push(("animationend", state.element_id));
                done.push(idx);
                continue;
            }
            still_running = true;
            if completed_iterations > state.last_iteration_event {
                for _ in state.last_iteration_event..completed_iterations {
                    iteration_events.push(state.element_id);
                }
                state.last_iteration_event = completed_iterations;
            }

            let effective_t = match state.animation.direction {
                AnimDirection::Normal => t_frac,
                AnimDirection::Reverse => 1.0 - t_frac,
                AnimDirection::Alternate => {
                    if (iteration as u32) % 2 == 0 {
                        t_frac
                    } else {
                        1.0 - t_frac
                    }
                }
                AnimDirection::AlternateReverse => {
                    if (iteration as u32) % 2 == 0 {
                        1.0 - t_frac
                    } else {
                        t_frac
                    }
                }
            };
            let eased = apply_easing(&state.animation.timing_fn, effective_t);

            if let Some(kf) = keyframes.get(&state.animation.name) {
                let underlying = find_node_by_id(&self.root, state.element_id)
                    .map(|node| extract_transitionable_style(&node.style))
                    .unwrap_or_default();
                let stops = keyframes_with_synthesized_endpoints(kf, &underlying);
                let props = interpolate_keyframe_stops(&stops, eased);
                let entry = self
                    .animation_overrides
                    .entry(state.element_id)
                    .or_default();
                entry.extend(props);
            }
        }
        for idx in done.into_iter().rev() {
            self.active_animations.remove(idx);
        }
        for target in iteration_events {
            let mut event = crate::dom::events::DomEvent::new("animationiteration", target);
            self.dispatch_dom_event(&mut event);
        }

        // ── CSS Transitions ──────────────────────────────────────────────────
        let mut empty_elems: Vec<u32> = Vec::new();
        for (elem_id, trs) in &mut self.transition_states {
            let mut done_trs: Vec<usize> = Vec::new();
            for (i, tr) in trs.iter().enumerate() {
                let elapsed_ms = now.duration_since(tr.start_time).as_secs_f32() * 1000.0;
                let delayed_ms = elapsed_ms - tr.delay_ms;

                if delayed_ms < 0.0 {
                    // Apply "from" value during delay.
                    let entry = self.animation_overrides.entry(*elem_id).or_default();
                    entry.push((tr.property.clone(), tr.from_value.clone()));
                    still_running = true;
                    continue;
                }
                if tr.duration_ms <= 0.0 {
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
                    entry.push((tr.property.clone(), tr.to_value.clone()));
                    finished_events.push(("transitionend", *elem_id));
                    done_trs.push(i);
                    continue;
                }

                still_running = true;
                let eased = apply_easing(&tr.timing_fn, progress);
                let interp = if tr.allow_discrete && is_discrete_transition_property(&tr.property) {
                    discrete_transition_value(&tr.property, &tr.from_value, &tr.to_value, eased)
                } else {
                    interpolate_property_value(&tr.property, &tr.from_value, &tr.to_value, eased)
                };
                let entry = self.animation_overrides.entry(*elem_id).or_default();
                entry.push((tr.property.clone(), interp));
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

        self.needs_animation_frame = still_running;

        // Mark all elements with active overrides as layout_dirty so the
        // layout cache doesn't return stale geometry for animated elements.
        if !self.animation_overrides.is_empty() {
            fn mark_dirty(node: &mut WebCore, ids: &HashMap<u32, Vec<(String, String)>>) {
                if ids.contains_key(&node.node_id) {
                    node.layout.layout_dirty = true;
                }
                for child in &mut node.children {
                    mark_dirty(child, ids);
                }
            }
            mark_dirty(&mut self.root, &self.animation_overrides);
        }

        for (event_type, target) in finished_events {
            let mut event = crate::dom::events::DomEvent::new(event_type, target);
            self.dispatch_dom_event(&mut event);
        }
    }
}
