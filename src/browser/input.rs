//! Publish native content input to the browser bridge's UI event queue.

use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::ModifiersState;

use crate::ui_events::{self, UiEvent};

pub(super) const WHEEL_LINE_SCROLL_PIXELS: f32 = 40.0;

pub(super) fn publish(
    event: &WindowEvent,
    position: (f32, f32),
    scale: f32,
    modifiers: ModifiersState,
) {
    let mut input = UiEvent {
        client_x: position.0 as i32,
        client_y: position.1 as i32,
        buttons: ui_events::pointer_state().buttons,
        ctrl_key: modifiers.control_key(),
        shift_key: modifiers.shift_key(),
        alt_key: modifiers.alt_key(),
        meta_key: modifiers.super_key(),
        ..Default::default()
    };
    match event {
        WindowEvent::CursorMoved { .. } => input.kind = "mousemove".into(),
        WindowEvent::CursorLeft { .. } => {
            input.kind = "mousemove".into();
            input.client_x = -1;
            input.client_y = -1;
        }
        WindowEvent::MouseInput { state, button, .. } => {
            let (button, mask) = match button {
                MouseButton::Left => (0, 1),
                MouseButton::Middle => (1, 4),
                MouseButton::Right => (2, 2),
                _ => return,
            };
            input.button = button;
            if *state == ElementState::Pressed {
                input.kind = "mousedown".into();
                input.buttons |= mask;
            } else {
                input.kind = "mouseup".into();
                input.buttons &= !mask;
            }
        }
        WindowEvent::MouseWheel { delta, .. } => {
            input.kind = "wheel".into();
            input.delta_y = match delta {
                MouseScrollDelta::LineDelta(_, y) => -f64::from(*y * WHEEL_LINE_SCROLL_PIXELS),
                MouseScrollDelta::PixelDelta(point) => -point.y / f64::from(scale),
            };
        }
        WindowEvent::KeyboardInput { event, .. } => {
            let (key, code, key_code) = crate::embedded_window::key_fields(&event.logical_key);
            input.kind = if event.state == ElementState::Pressed {
                "keydown"
            } else {
                "keyup"
            }
            .into();
            input.key = key;
            input.code = code;
            input.key_code = key_code;
            input.repeat = event.repeat;
        }
        _ => return,
    }
    ui_events::push(input);
}
