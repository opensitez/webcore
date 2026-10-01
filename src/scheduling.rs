//! Browser-owned timer and animation-frame registration, without guest callbacks.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

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
    next_frame_ms: f64,
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
    let mut state = state().lock().unwrap();
    state.next_frame += 1;
    let id = state.next_frame;
    if state.frames.is_empty() && state.next_frame_ms <= now_ms() {
        state.next_frame_ms = now_ms();
    }
    state.frames.push(id);
    id
}

pub fn cancel_frame(id: u64) -> bool {
    let mut state = state().lock().unwrap();
    if let Some(index) = state.frames.iter().position(|entry| *entry == id) {
        state.frames.remove(index);
        true
    } else {
        false
    }
}

pub fn take_due_frame() -> Option<u64> {
    let mut state = state().lock().unwrap();
    if now_ms() < state.next_frame_ms || state.frames.is_empty() {
        return None;
    }
    let id = state.frames.remove(0);
    if state.frames.is_empty() {
        state.next_frame_ms = now_ms() + 1000.0 / 60.0;
    }
    Some(id)
}

pub fn frame_delay_ms() -> Option<f64> {
    let state = state().lock().unwrap();
    (!state.frames.is_empty()).then(|| (state.next_frame_ms - now_ms()).max(0.0))
}

pub fn reset() {
    let mut state = state().lock().unwrap();
    state.timers.clear();
    state.frames.clear();
}
