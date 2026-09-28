//! Browser-owned W3C UI event queue for embedded Webcore contexts.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Default)]
pub struct UiEvent {
    pub kind: String,
    pub key: String,
    pub code: String,
    pub key_code: i32,
    pub client_x: i32,
    pub client_y: i32,
    pub button: i32,
    pub buttons: i32,
    pub delta_y: f64,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub repeat: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PointerState {
    pub client_x: i32,
    pub client_y: i32,
    pub buttons: i32,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}

#[derive(Default)]
struct Queue {
    events: VecDeque<UiEvent>,
    pointer: PointerState,
}

fn queue() -> &'static Mutex<Queue> {
    static QUEUE: OnceLock<Mutex<Queue>> = OnceLock::new();
    QUEUE.get_or_init(|| Mutex::new(Queue::default()))
}

pub fn push(event: UiEvent) {
    let mut queue = queue().lock().unwrap();
    let pointer = &mut queue.pointer;
    pointer.ctrl_key = event.ctrl_key;
    pointer.shift_key = event.shift_key;
    pointer.alt_key = event.alt_key;
    pointer.meta_key = event.meta_key;
    if matches!(event.kind.as_str(), "mousemove" | "mousedown" | "mouseup") {
        pointer.client_x = event.client_x;
        pointer.client_y = event.client_y;
        pointer.buttons = event.buttons;
    }
    if queue.events.len() == 4096 {
        queue.events.pop_front();
    }
    queue.events.push_back(event);
}

pub fn poll() -> Option<UiEvent> {
    queue().lock().unwrap().events.pop_front()
}

pub fn pending() -> usize {
    queue().lock().unwrap().events.len()
}

pub fn pointer_state() -> PointerState {
    queue().lock().unwrap().pointer
}

pub fn reset() {
    *queue().lock().unwrap() = Queue::default();
}
