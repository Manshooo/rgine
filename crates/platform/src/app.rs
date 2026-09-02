//! The application-side contract of the platform layer.

use crate::{Commands, Event, PlatformError, backend};

/// The application driven by the platform event loop.
///
/// The platform calls [`update`](PlatformApp::update) once per loop iteration
/// with every event accumulated since the previous call, so an implementation
/// sees whole batches rather than one callback per event.
pub trait PlatformApp {
    /// Handles one batch of events and records the resulting platform requests.
    ///
    /// `events` may be empty. Commands recorded into `commands` are applied
    /// after this call returns.
    fn update(&mut self, events: &[Event], commands: &mut Commands);
}

/// Runs `app` on the platform event loop until it records
/// [`Commands::exit`], or the platform terminates it.
///
/// On platforms whose event loop owns the process (Android, iOS, web) this may
/// never return.
pub fn run<A: PlatformApp>(app: A) -> Result<(), PlatformError> {
    backend::run(app)
}
