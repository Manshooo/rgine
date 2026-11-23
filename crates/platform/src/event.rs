//! Events delivered from the platform to the application.
//!
//! Events are handed to [`PlatformApp::update`](crate::PlatformApp::update) as a
//! slice, one batch per loop iteration, rather than one call per event.

use serde::{Deserialize, Serialize};

use crate::{InputEvent, WindowId};

/// A single platform event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Input delivered to a window during this batch.
    Input(InputEvent),
}

/// What happened to one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The platform reports that the drawable became hidden or visible.
    Occluded(bool),
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{Input, InputTimestamp, KeyCode, PhysicalKey, TouchPhase};

    #[test]
    fn round_trips_a_platform_event_batch_with_timestamp_provenance() {
        let window = WindowId::from_raw(17);
        let batch = [
            Event::Resumed,
            Event::Input(InputEvent {
                window,
                timestamp: InputTimestamp::Platform(Duration::from_nanos(11)),
                input: Input::Key {
                    key: PhysicalKey::Code(KeyCode::KeyW),
                    pressed: true,
                    repeat: false,
                },
            }),
            Event::Input(InputEvent {
                window,
                timestamp: InputTimestamp::Received(Duration::from_nanos(12)),
                input: Input::Touch {
                    id: 42,
                    phase: TouchPhase::Moved,
                    x: 0.5,
                    y: 9.75,
                },
            }),
        ];
        let encoded = serde_json::to_string(&batch).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Event>>(&encoded).unwrap(), batch);
    }
}
