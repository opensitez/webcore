//! Minimal HTML media element runtime.
//!
//! This owns `HTMLMediaElement`-style state that is not part of markup:
//! playback, seeking, current time, and media events. Actual audio/video
//! decoding can sit behind the same state later; pages and SVG eventbase timing
//! already need the DOM contract even when the renderer has no demuxer.

pub mod backend;
mod controls;
mod source;
#[cfg(feature = "audio-symphonia")]
pub mod symphonia_backend;
mod tracks;

use crate::types::Document;
use std::collections::HashMap;
use std::time::Instant;

pub use backend::{
    AudioSamples, DecodedMedia, MediaDecodeError, MediaDecoder, MediaMetadata, NullMediaDecoder,
    VideoFrame,
};
pub(crate) use controls::build_media_element;
pub use tracks::TextTrackInfo;

#[derive(Clone, Debug)]
pub struct MediaElementState {
    pub current_time: f32,
    pub duration: Option<f32>,
    pub ready_state: u16,
    pub network_state: u16,
    pub paused: bool,
    pub ended: bool,
    pub seeking: bool,
    pub playback_rate: f32,
    pub volume: f32,
    pub muted: bool,
    pub metadata_loaded: bool,
    pub last_tick: Option<Instant>,
}

impl Default for MediaElementState {
    fn default() -> Self {
        Self {
            current_time: 0.0,
            duration: None,
            ready_state: MEDIA_HAVE_NOTHING,
            network_state: MEDIA_NETWORK_EMPTY,
            paused: true,
            ended: false,
            seeking: false,
            playback_rate: 1.0,
            volume: 1.0,
            muted: false,
            metadata_loaded: false,
            last_tick: None,
        }
    }
}

pub type MediaStateMap = HashMap<u32, MediaElementState>;

pub const MEDIA_NETWORK_EMPTY: u16 = 0;
pub const MEDIA_NETWORK_IDLE: u16 = 1;
pub const MEDIA_NETWORK_LOADING: u16 = 2;
pub const MEDIA_NETWORK_NO_SOURCE: u16 = 3;

pub const MEDIA_HAVE_NOTHING: u16 = 0;
pub const MEDIA_HAVE_METADATA: u16 = 1;
pub const MEDIA_HAVE_CURRENT_DATA: u16 = 2;
pub const MEDIA_HAVE_FUTURE_DATA: u16 = 3;
pub const MEDIA_HAVE_ENOUGH_DATA: u16 = 4;

impl Document {
    pub fn initialize_media_elements(&mut self) {
        let mut ids = Vec::new();
        crate::types::Document::walk_all(&self.root, &mut |node| {
            if matches!(node.tag.as_str(), "audio" | "video") {
                ids.push(node.node_id);
            }
        });
        for id in ids {
            let autoplay = self.media_autoplay(id).unwrap_or(false);
            let preload = self
                .media_preload(id)
                .unwrap_or_else(|| "metadata".to_string());
            if autoplay {
                self.media_play(id);
            } else if preload != "none" {
                self.media_load(id);
            } else {
                self.ensure_media_state(id);
                self.sync_media_render_state(id);
            }
        }
    }

    pub fn is_media_element(&self, id: u32) -> bool {
        matches!(self.tag_name(id), Some("audio" | "video"))
    }

    pub fn media_current_src(&self, id: u32) -> Option<String> {
        let node = self.find_webcore(id)?;
        source::current_src(node, &self.base_url)
    }

    pub fn media_can_play_type(&self, id: u32, media_type: &str) -> Option<&'static str> {
        let node = self.find_webcore(id)?;
        source::can_play_type(&node.tag, media_type)
    }

    pub fn media_duration(&mut self, id: u32) -> Option<f32> {
        self.ensure_media_state(id).and_then(|s| s.duration)
    }

    pub fn media_ready_state(&mut self, id: u32) -> Option<u16> {
        self.ensure_media_state(id).map(|s| s.ready_state)
    }

    pub fn media_network_state(&mut self, id: u32) -> Option<u16> {
        self.ensure_media_state(id).map(|s| s.network_state)
    }

    pub fn media_current_time(&mut self, id: u32) -> Option<f32> {
        self.ensure_media_state(id).map(|s| s.current_time)
    }

    pub fn media_paused(&mut self, id: u32) -> Option<bool> {
        self.ensure_media_state(id).map(|s| s.paused)
    }

    pub fn media_ended(&mut self, id: u32) -> Option<bool> {
        self.ensure_media_state(id).map(|s| s.ended)
    }

    pub fn media_volume(&mut self, id: u32) -> Option<f32> {
        self.ensure_media_state(id).map(|s| s.volume)
    }

    pub fn media_muted(&mut self, id: u32) -> Option<bool> {
        self.ensure_media_state(id).map(|s| s.muted)
    }

    pub fn media_playback_rate(&mut self, id: u32) -> Option<f32> {
        self.ensure_media_state(id).map(|s| s.playback_rate)
    }

    pub fn media_set_playback_rate(&mut self, id: u32, rate: f32) -> bool {
        if self.ensure_media_state(id).is_none() || !rate.is_finite() || rate == 0.0 {
            return false;
        }
        let state = self.media_states.get_mut(&id).expect("checked above");
        if (state.playback_rate - rate).abs() <= f32::EPSILON {
            return true;
        }
        state.playback_rate = rate;
        self.fire_media_event(id, "ratechange");
        true
    }

    pub fn media_autoplay(&self, id: u32) -> Option<bool> {
        self.media_bool_attr(id, "autoplay")
    }

    pub fn media_controls(&self, id: u32) -> Option<bool> {
        self.media_bool_attr(id, "controls")
    }

    pub fn media_loop(&self, id: u32) -> Option<bool> {
        self.media_bool_attr(id, "loop")
    }

    pub fn media_preload(&self, id: u32) -> Option<String> {
        if !self.is_media_element(id) {
            return None;
        }
        let raw = self.get_attribute(id, "preload").unwrap_or_default();
        let normalized = match raw.trim().to_ascii_lowercase().as_str() {
            "" => "auto",
            "none" => "none",
            "metadata" => "metadata",
            "auto" => "auto",
            _ => "metadata",
        };
        Some(normalized.to_string())
    }

    pub fn media_text_tracks(&self, id: u32) -> Option<Vec<TextTrackInfo>> {
        let node = self.find_webcore(id)?;
        tracks::text_tracks(node, &self.base_url)
    }

    pub fn media_play(&mut self, id: u32) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        if !self.media_has_source(id) {
            self.media_load(id);
            return false;
        }
        let mut events = Vec::new();
        {
            let state = self.media_states.get_mut(&id).expect("checked above");
            if !state.metadata_loaded {
                state.metadata_loaded = true;
                state.network_state = MEDIA_NETWORK_IDLE;
                state.ready_state = MEDIA_HAVE_ENOUGH_DATA;
                events.push("loadstart");
                events.push("loadedmetadata");
                events.push("durationchange");
                events.push("loadeddata");
                events.push("canplay");
                events.push("canplaythrough");
            }
            state.paused = false;
            state.ended = false;
            state.last_tick = Some(Instant::now());
            events.push("play");
            events.push("playing");
        }
        self.sync_media_render_state(id);
        for event in events {
            self.fire_media_event(id, event);
        }
        self.needs_animation_frame = true;
        true
    }

    pub fn media_load(&mut self, id: u32) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        let mut events = Vec::new();
        let has_source = self.media_has_source(id);
        {
            let state = self.media_states.get_mut(&id).expect("checked above");
            state.current_time = 0.0;
            state.paused = true;
            state.ended = false;
            state.seeking = false;
            state.last_tick = None;
            events.push("loadstart");
            if has_source {
                state.metadata_loaded = true;
                state.network_state = MEDIA_NETWORK_IDLE;
                state.ready_state = MEDIA_HAVE_ENOUGH_DATA;
                events.push("loadedmetadata");
                events.push("durationchange");
                events.push("loadeddata");
                events.push("canplay");
                events.push("canplaythrough");
            } else {
                state.metadata_loaded = false;
                state.network_state = MEDIA_NETWORK_NO_SOURCE;
                state.ready_state = MEDIA_HAVE_NOTHING;
                events.push("error");
            }
        }
        self.sync_media_render_state(id);
        for event in events {
            self.fire_media_event(id, event);
        }
        true
    }

    pub fn media_pause(&mut self, id: u32) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        let mut should_fire = false;
        {
            let state = self.media_states.get_mut(&id).expect("checked above");
            if !state.paused {
                should_fire = true;
            }
            state.paused = true;
            state.last_tick = None;
        }
        if should_fire {
            self.fire_media_event(id, "pause");
        }
        self.sync_media_render_state(id);
        true
    }

    pub fn media_toggle_playback(&mut self, id: u32) -> bool {
        let paused = match self.media_paused(id) {
            Some(value) => value,
            None => return false,
        };
        if paused {
            self.media_play(id)
        } else {
            self.media_pause(id)
        }
    }

    pub fn media_seek_time_for_point(&mut self, id: u32, point: (f32, f32)) -> Option<f32> {
        let duration = self.media_duration(id)?;
        if duration <= 0.0 || !duration.is_finite() {
            return None;
        }
        let node = self.find_webcore(id)?;
        let controls = controls::media_control_layout(node)?;
        if controls.timeline_w <= 8.0 {
            return None;
        }
        let (x, y) = point;
        let in_timeline = x >= controls.timeline_x
            && x <= controls.timeline_x + controls.timeline_w
            && y >= controls.rail_y
            && y <= controls.rail_y + controls.rail_h;
        if !in_timeline {
            return None;
        }
        Some(((x - controls.timeline_x) / controls.timeline_w).clamp(0.0, 1.0) * duration)
    }

    pub fn media_set_current_time(&mut self, id: u32, seconds: f32) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        let seconds = seconds.max(0.0);
        {
            let state = self.media_states.get_mut(&id).expect("checked above");
            state.seeking = true;
        }
        self.fire_media_event(id, "seeking");
        {
            let state = self.media_states.get_mut(&id).expect("checked above");
            let duration = state.duration.unwrap_or(f32::INFINITY);
            state.current_time = seconds.min(duration);
            state.ended = state.duration.is_some_and(|d| state.current_time >= d);
            state.seeking = false;
            if !state.paused {
                state.last_tick = Some(Instant::now());
            }
        }
        self.sync_media_render_state(id);
        self.fire_media_event(id, "timeupdate");
        self.fire_media_event(id, "seeked");
        true
    }

    pub fn media_set_volume(&mut self, id: u32, volume: f32) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        let state = self.media_states.get_mut(&id).expect("checked above");
        let next = volume.clamp(0.0, 1.0);
        if (state.volume - next).abs() <= f32::EPSILON {
            return true;
        }
        state.volume = next;
        self.fire_media_event(id, "volumechange");
        true
    }

    pub fn media_set_muted(&mut self, id: u32, muted: bool) -> bool {
        if self.ensure_media_state(id).is_none() {
            return false;
        }
        let state = self.media_states.get_mut(&id).expect("checked above");
        if state.muted == muted {
            return true;
        }
        state.muted = muted;
        self.fire_media_event(id, "volumechange");
        true
    }

    pub fn tick_media(&mut self, now: Instant) -> bool {
        let ids: Vec<u32> = self.media_states.keys().copied().collect();
        let mut events = Vec::<(u32, &'static str)>::new();
        let mut sync_ids = Vec::<u32>::new();
        let mut any_playing = false;

        for id in ids {
            let should_loop = self.media_loop(id).unwrap_or(false);
            let Some(state) = self.media_states.get_mut(&id) else {
                continue;
            };
            if state.paused || state.ended {
                continue;
            }
            let Some(last) = state.last_tick.replace(now) else {
                continue;
            };
            let dt = now.duration_since(last).as_secs_f32() * state.playback_rate;
            if dt <= 0.0 {
                any_playing = true;
                continue;
            }
            state.current_time = (state.current_time + dt).max(0.0);
            events.push((id, "timeupdate"));
            if let Some(duration) = state.duration {
                if state.current_time >= duration {
                    if should_loop && duration > 0.0 {
                        state.current_time = 0.0;
                        events.push((id, "timeupdate"));
                    } else {
                        state.current_time = duration;
                        state.ended = true;
                        state.paused = true;
                        state.last_tick = None;
                        events.push((id, "ended"));
                        sync_ids.push(id);
                        continue;
                    }
                }
            }
            any_playing = true;
            sync_ids.push(id);
        }

        sync_ids.sort_unstable();
        sync_ids.dedup();
        for id in sync_ids {
            self.sync_media_render_state(id);
        }

        for (id, event) in events {
            self.fire_media_event(id, event);
        }
        if any_playing {
            self.needs_animation_frame = true;
        }
        any_playing
    }

    fn ensure_media_state(&mut self, id: u32) -> Option<&mut MediaElementState> {
        if !self.is_media_element(id) {
            return None;
        }
        let duration = self.media_duration_from_markup(id);
        let has_source = self.media_has_source(id);
        let state = self.media_states.entry(id).or_default();
        if !state.metadata_loaded {
            state.duration = duration;
            state.network_state = if has_source {
                MEDIA_NETWORK_IDLE
            } else {
                MEDIA_NETWORK_NO_SOURCE
            };
        }
        Some(state)
    }

    fn media_has_source(&self, id: u32) -> bool {
        self.media_current_src(id)
            .map(|src| !src.is_empty())
            .unwrap_or(false)
    }

    fn sync_media_render_state(&mut self, id: u32) {
        let Some(state) = self.media_states.get(&id).cloned() else {
            return;
        };
        if let Some(node) = self.find_webcore_mut(id) {
            node.media_current_time = state.current_time;
            node.media_duration = state.duration;
            node.media_paused = state.paused;
            node.media_ended = state.ended;
            node.layout.intrinsic_dirty = true;
        }
    }

    fn media_duration_from_markup(&self, id: u32) -> Option<f32> {
        let node = self.find_webcore(id)?;
        for key in ["data-duration", "duration"] {
            if let Some(value) = node.attributes.get(key) {
                if let Ok(seconds) = value.trim().parse::<f32>() {
                    if seconds.is_finite() && seconds >= 0.0 {
                        return Some(seconds);
                    }
                }
            }
        }
        None
    }

    fn fire_media_event(&mut self, id: u32, event_type: &'static str) {
        self.svg_trigger_event_from_dom_id(id, event_type);
        let mut event = crate::dom::events::DomEvent::new(event_type, id);
        self.dispatch_dom_event(&mut event);
    }

    fn media_bool_attr(&self, id: u32, name: &str) -> Option<bool> {
        if !self.is_media_element(id) {
            return None;
        }
        Some(self.get_attribute(id, name).is_some())
    }
}
