#![forbid(unsafe_code)]

//! OS window, event loop and input abstraction.
//!
//! The backend behind this crate is replaceable: nothing in the public surface
//! names a backend type. Callers hold opaque handles ([`WindowId`]), record
//! one-way [`Commands`], and receive batches of [`Event`]s.

#[cfg(target_os = "android")]
#[doc(hidden)]
pub mod android_entry;
mod app;
mod backend;
mod command;
mod error;
mod event;
mod input;
mod window;

pub use app::{PlatformApp, run};
pub use command::{Command, Commands};
pub use error::PlatformError;
pub use event::{Event, WindowEvent};
pub use input::{
    Input, InputEvent, InputTimestamp, KeyCode, MouseButton, NativeKeyCode, PhysicalKey,
    ScrollDelta, TouchPhase,
};
pub use window::{SurfaceEvent, SurfaceSource, WindowDesc, WindowId};
