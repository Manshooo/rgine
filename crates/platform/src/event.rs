//! Events delivered from the platform to the application.
//!
//! Events are handed to [`PlatformApp::update`](crate::PlatformApp::update) as a
//! slice, one batch per loop iteration, rather than one call per event.

use crate::WindowId;

/// A single platform event.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// The platform is ready and windows may be created.
    ///
    /// Fires once at startup on desktop, and again after every
    /// [`Event::Suspended`] on platforms that tear surfaces down (Android).
    Resumed,
    /// Platform surfaces are going away; window handles must not be used until
    /// the next [`Event::Resumed`].
    Suspended,
    /// An event addressed to a single window.
    Window {
        /// The window the event belongs to.
        window: WindowId,
        /// What happened to it.
        event: WindowEvent,
    },
}

/// What happened to one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowEvent {
    /// The user asked the window to close; nothing has been closed yet.
    CloseRequested,
    /// The drawable area changed size, in physical pixels.
    Resized {
        /// New width in physical pixels.
        width: u32,
        /// New height in physical pixels.
        height: u32,
    },
    /// The window content should be redrawn.
    RedrawRequested,
    /// The window is gone and its handle is now stale.
    Destroyed,
}
