//! Maps winit input to the platform-owned event types.

use winit::event::{
    ElementState, MouseButton as BackendMouseButton, MouseScrollDelta, TouchPhase as BackendPhase,
    WindowEvent,
};
use winit::keyboard::{
    KeyCode as BackendCode, NativeKeyCode as BackendNativeCode, PhysicalKey as BackendKey,
};

use crate::input::key_codes;
use crate::{Input, KeyCode, MouseButton, NativeKeyCode, PhysicalKey, ScrollDelta, TouchPhase};

pub(super) fn translate(event: WindowEvent) -> Option<Input> {
    Some(match event {
        WindowEvent::KeyboardInput { event, .. } => Input::Key {
            key: physical_key(event.physical_key),
            pressed: event.state == ElementState::Pressed,
            repeat: event.repeat,
        },
        WindowEvent::CursorMoved { position, .. } => Input::PointerMoved {
            x: position.x,
            y: position.y,
        },
        WindowEvent::MouseInput { button, state, .. } => Input::MouseButton {
            button: match button {
                BackendMouseButton::Left => MouseButton::Left,
                BackendMouseButton::Right => MouseButton::Right,
                BackendMouseButton::Middle => MouseButton::Middle,
                BackendMouseButton::Back => MouseButton::Back,
                BackendMouseButton::Forward => MouseButton::Forward,
                BackendMouseButton::Other(id) => MouseButton::Other(id),
            },
            pressed: state == ElementState::Pressed,
        },
        WindowEvent::MouseWheel { delta, phase, .. } => Input::Scroll {
            delta: match delta {
                MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines(x, y),
                MouseScrollDelta::PixelDelta(position) => {
                    ScrollDelta::Pixels(position.x, position.y)
                }
            },
            phase: touch_phase(phase),
        },
        WindowEvent::Touch(touch) => Input::Touch {
            id: touch.id,
            phase: touch_phase(touch.phase),
            x: touch.location.x,
            y: touch.location.y,
        },
        _ => return None,
    })
}

fn touch_phase(phase: BackendPhase) -> TouchPhase {
    match phase {
        BackendPhase::Started => TouchPhase::Started,
        BackendPhase::Moved => TouchPhase::Moved,
        BackendPhase::Ended => TouchPhase::Ended,
        BackendPhase::Cancelled => TouchPhase::Cancelled,
    }
}

fn physical_key(key: BackendKey) -> PhysicalKey {
    match key {
        BackendKey::Code(code) => standard_code(code),
        BackendKey::Unidentified(code) => PhysicalKey::Unidentified(match code {
            BackendNativeCode::Unidentified => NativeKeyCode::Unidentified,
            BackendNativeCode::Android(code) => NativeKeyCode::Android(code),
            BackendNativeCode::MacOS(code) => NativeKeyCode::MacOS(code),
            BackendNativeCode::Windows(code) => NativeKeyCode::Windows(code),
            BackendNativeCode::Xkb(code) => NativeKeyCode::Xkb(code),
        }),
    }
}

macro_rules! translate_key_codes {
    ($($code:ident),* $(,)?) => {
        fn standard_code(code: BackendCode) -> PhysicalKey {
            match code {
                $(BackendCode::$code => PhysicalKey::Code(KeyCode::$code),)*
                _ => PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            }
        }
    };
}
key_codes!(translate_key_codes);

#[cfg(test)]
mod tests {
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, Touch};

    use super::*;

    #[test]
    fn maps_physical_codes_and_preserves_unrecognized_scan_codes() {
        assert_eq!(
            physical_key(BackendKey::Code(BackendCode::KeyW)),
            PhysicalKey::Code(KeyCode::KeyW)
        );
        assert_eq!(
            physical_key(BackendKey::Unidentified(BackendNativeCode::Windows(34))),
            PhysicalKey::Unidentified(NativeKeyCode::Windows(34))
        );
    }

    #[test]
    fn maps_mouse_and_touch_without_changing_coordinates() {
        let device_id = DeviceId::dummy();
        assert_eq!(
            translate(WindowEvent::CursorMoved {
                device_id,
                position: PhysicalPosition::new(8.25, 13.5),
            }),
            Some(Input::PointerMoved { x: 8.25, y: 13.5 })
        );
        assert_eq!(
            translate(WindowEvent::Touch(Touch {
                device_id,
                phase: BackendPhase::Cancelled,
                location: PhysicalPosition::new(2.5, 4.0),
                force: None,
                id: 7,
            })),
            Some(Input::Touch {
                id: 7,
                phase: TouchPhase::Cancelled,
                x: 2.5,
                y: 4.0
            })
        );
    }
}
