//! Input vocabulary owned by the platform layer.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::WindowId;

/// Timestamp and input payload delivered for a window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InputEvent {
    /// Window that received the input.
    pub window: WindowId,
    /// Time attached to this event before the application receives its batch.
    pub timestamp: InputTimestamp,
    /// The input itself.
    pub input: Input,
}

/// Time of an input event in the backend's monotonic clock domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputTimestamp {
    /// Source timestamp supplied by the operating system, when available.
    Platform(Duration),
    /// Time since event-loop startup at callback receipt. winit 0.30 uses
    /// this when it delivers input without a source timestamp.
    Received(Duration),
}

/// A physical device event.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Input {
    /// A key changed state or repeated.
    Key {
        /// Key position, independent of keyboard layout.
        key: PhysicalKey,
        /// Whether the key is pressed.
        pressed: bool,
        /// Whether this press is an automatic repeat.
        repeat: bool,
    },
    /// Pointer position in physical window pixels.
    PointerMoved {
        /// Horizontal position.
        x: f64,
        /// Vertical position.
        y: f64,
    },
    /// A mouse button changed state.
    MouseButton {
        /// The button that changed.
        button: MouseButton,
        /// Whether the button is pressed.
        pressed: bool,
    },
    /// Mouse wheel or trackpad scrolling.
    Scroll {
        /// Scroll delta in the units supplied by the platform.
        delta: ScrollDelta,
        /// Gesture phase, when the platform distinguishes phases.
        phase: TouchPhase,
    },
    /// A touch contact started, moved, ended, or was cancelled.
    Touch {
        /// Contact identifier, stable until this contact ends.
        id: u64,
        /// Contact lifecycle phase.
        phase: TouchPhase,
        /// Horizontal position in physical window pixels.
        x: f64,
        /// Vertical position in physical window pixels.
        y: f64,
    },
}

/// A keyboard position rather than a layout-dependent character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalKey {
    /// A standardized key position.
    Code(KeyCode),
    /// A key for which only the native identifier is available.
    Unidentified(NativeKeyCode),
}

/// Platform-specific identifier for an otherwise unidentified physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NativeKeyCode {
    /// The backend could not identify the key.
    Unidentified,
    /// Android scan code.
    Android(u32),
    /// macOS key code.
    MacOS(u16),
    /// Windows scan code.
    Windows(u16),
    /// XKB key code.
    Xkb(u32),
}

// A single list keeps the public key vocabulary and the backend mapping in
// sync. These names are the standardized positions used for key bindings.
macro_rules! key_codes {
    ($use_keys:ident) => {
        $use_keys! {
            Backquote, Backslash, BracketLeft, BracketRight, Comma, Digit0, Digit1,
            Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Equal,
            IntlBackslash, IntlRo, IntlYen, KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG,
            KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
            KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ, Minus, Period, Quote, Semicolon, Slash,
            AltLeft, AltRight, Backspace, CapsLock, ContextMenu, ControlLeft,
            ControlRight, Enter, SuperLeft, SuperRight, ShiftLeft, ShiftRight, Space,
            Tab, Convert, KanaMode, Lang1, Lang2, Lang3, Lang4, Lang5, NonConvert,
            Delete, End, Help, Home, Insert, PageDown, PageUp, ArrowDown, ArrowLeft,
            ArrowRight, ArrowUp, NumLock, Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
            Numpad5, Numpad6, Numpad7, Numpad8, Numpad9, NumpadAdd, NumpadBackspace,
            NumpadClear, NumpadClearEntry, NumpadComma, NumpadDecimal, NumpadDivide,
            NumpadEnter, NumpadEqual, NumpadHash, NumpadMemoryAdd, NumpadMemoryClear,
            NumpadMemoryRecall, NumpadMemoryStore, NumpadMemorySubtract, NumpadMultiply,
            NumpadParenLeft, NumpadParenRight, NumpadStar, NumpadSubtract, Escape, Fn,
            FnLock, PrintScreen, ScrollLock, Pause, BrowserBack, BrowserFavorites,
            BrowserForward, BrowserHome, BrowserRefresh, BrowserSearch, BrowserStop,
            Eject, LaunchApp1, LaunchApp2, LaunchMail, MediaPlayPause, MediaSelect,
            MediaStop, MediaTrackNext, MediaTrackPrevious, Power, Sleep, AudioVolumeDown,
            AudioVolumeMute, AudioVolumeUp, WakeUp, Meta, Hyper, Turbo, Abort, Resume,
            Suspend, Again, Copy, Cut, Find, Open, Paste, Props, Select, Undo, Hiragana,
            Katakana, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12, F13, F14, F15,
            F16, F17, F18, F19, F20, F21, F22, F23, F24, F25, F26, F27, F28, F29, F30,
            F31, F32, F33, F34, F35,
        }
    };
}
pub(crate) use key_codes;

macro_rules! declare_key_codes {
    ($($code:ident),* $(,)?) => {
        /// Layout-independent key positions.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[non_exhaustive]
        pub enum KeyCode {
            $($code),*
        }
    };
}
key_codes!(declare_key_codes);

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
    /// Back button.
    Back,
    /// Forward button.
    Forward,
    /// Additional platform-defined button.
    Other(u16),
}

/// Relative scrolling in lines or physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScrollDelta {
    /// Horizontal and vertical line deltas.
    Lines(f32, f32),
    /// Horizontal and vertical pixel deltas.
    Pixels(f64, f64),
}

/// Lifecycle of a touch contact or scroll gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TouchPhase {
    /// A new contact or gesture.
    Started,
    /// The existing contact or gesture moved.
    Moved,
    /// The contact or gesture ended normally.
    Ended,
    /// The platform interrupted the contact or gesture.
    Cancelled,
}
