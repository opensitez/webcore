//! Browser-owned timer and animation-frame registration, without guest callbacks.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

pub(crate) const ANIMATION_FRAME_INTERVAL: std::time::Duration =
    std::time::Duration::from_nanos(16_666_667);

pub fn now_ms() -> f64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[derive(Default)]
struct State {
    next_timer: u64,
    timers: Vec<(u64, f64)>,
    next_frame: u64,
    frames: Vec<u64>,
    ready_frames: VecDeque<u64>,
    frame_batch_active: bool,
    next_frame_ms: f64,
}

impl State {
    fn request_frame(&mut self, now: f64) -> u64 {
        self.next_frame += 1;
        let id = self.next_frame;
        if self.frames.is_empty() && !self.frame_batch_active && self.next_frame_ms <= now {
            self.next_frame_ms = now;
        }
        self.frames.push(id);
        id
    }

    fn cancel_frame(&mut self, id: u64) -> bool {
        if let Some(index) = self.frames.iter().position(|entry| *entry == id) {
            self.frames.remove(index);
            true
        } else if let Some(index) = self.ready_frames.iter().position(|entry| *entry == id) {
            self.ready_frames.remove(index);
            true
        } else {
            false
        }
    }

    fn take_due_frame(&mut self, now: f64) -> Option<u64> {
        if let Some(id) = self.ready_frames.pop_front() {
            return Some(id);
        }
        // End the snapshot before considering registrations made by its callbacks.
        if self.frame_batch_active {
            self.frame_batch_active = false;
            return None;
        }
        if now < self.next_frame_ms || self.frames.is_empty() {
            return None;
        }
        self.ready_frames = std::mem::take(&mut self.frames).into();
        self.frame_batch_active = true;
        self.next_frame_ms = now + ANIMATION_FRAME_INTERVAL.as_secs_f64() * 1000.0;
        self.ready_frames.pop_front()
    }
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

pub fn set_timer(delay_ms: f64) -> u64 {
    let mut state = state().lock().unwrap();
    state.next_timer += 1;
    let id = state.next_timer;
    state.timers.push((id, now_ms() + delay_ms.max(0.0)));
    id
}

pub fn clear_timer(id: u64) -> bool {
    let mut state = state().lock().unwrap();
    if let Some(index) = state.timers.iter().position(|(entry, _)| *entry == id) {
        state.timers.remove(index);
        true
    } else {
        false
    }
}

pub fn take_due_timer() -> Option<u64> {
    let mut state = state().lock().unwrap();
    let index = state
        .timers
        .iter()
        .position(|(_, deadline)| *deadline <= now_ms())?;
    Some(state.timers.remove(index).0)
}

pub fn timer_delay_ms() -> Option<f64> {
    state()
        .lock()
        .unwrap()
        .timers
        .iter()
        .map(|(_, deadline)| (deadline - now_ms()).max(0.0))
        .reduce(f64::min)
}

pub fn request_frame() -> u64 {
    state().lock().unwrap().request_frame(now_ms())
}

pub fn cancel_frame(id: u64) -> bool {
    state().lock().unwrap().cancel_frame(id)
}

pub fn take_due_frame() -> Option<u64> {
    state().lock().unwrap().take_due_frame(now_ms())
}

pub fn frame_delay_ms() -> Option<f64> {
    let state = state().lock().unwrap();
    if !state.ready_frames.is_empty() {
        return Some(0.0);
    }
    (!state.frames.is_empty()).then(|| (state.next_frame_ms - now_ms()).max(0.0))
}

pub fn reset() {
    let mut state = state().lock().unwrap();
    state.timers.clear();
    state.frames.clear();
    state.ready_frames.clear();
    state.frame_batch_active = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_registered_during_a_frame_wait_for_the_next_batch() {
        let mut state = State::default();
        let first = state.request_frame(0.0);
        let second = state.request_frame(0.0);
        assert_eq!(state.take_due_frame(0.0), Some(first));
        let next_first = state.request_frame(0.0);
        assert_eq!(state.take_due_frame(0.0), Some(second));
        let next_second = state.request_frame(0.0);
        assert_eq!(state.take_due_frame(0.0), None);
        assert_eq!(state.take_due_frame(0.0), None);
        let next = state.next_frame_ms;
        assert_eq!(state.take_due_frame(next), Some(next_first));
        assert_eq!(state.take_due_frame(next), Some(next_second));
        assert_eq!(state.take_due_frame(next), None);
    }

    #[test]
    fn slow_callbacks_cannot_extend_the_current_frame_snapshot() {
        let mut state = State::default();
        let first = state.request_frame(0.0);
        assert_eq!(state.take_due_frame(0.0), Some(first));
        let next = state.request_frame(100.0);
        assert_eq!(state.take_due_frame(100.0), None);
        assert_eq!(state.take_due_frame(100.0), Some(next));
    }

    #[test]
    fn cancellation_removes_both_queued_and_snapshotted_callbacks() {
        let mut state = State::default();
        let first = state.request_frame(0.0);
        let second = state.request_frame(0.0);
        assert_eq!(state.take_due_frame(0.0), Some(first));
        assert!(state.cancel_frame(second));
        let pending = state.request_frame(0.0);
        assert!(state.cancel_frame(pending));
        assert!(!state.cancel_frame(first));
        assert_eq!(state.take_due_frame(0.0), None);
        assert_eq!(state.take_due_frame(100.0), None);
    }
}
