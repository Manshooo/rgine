//! Platform error type.
//!
//! Backend errors are flattened into this type at the boundary: no variant
//! carries a backend value, and `source` is not backend-typed, so a caller can
//! match on failures without naming anything the backend owns.

use core::fmt;

/// A failure raised by the platform layer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlatformError {
    /// The event loop could not be created, or exited abnormally.
    EventLoop(String),
    /// A window requested by a [`crate::Command`] could not be created.
    WindowCreation(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EventLoop(message) => write!(f, "event loop failure: {message}"),
            Self::WindowCreation(message) => write!(f, "window creation failed: {message}"),
        }
    }
}

impl std::error::Error for PlatformError {}
