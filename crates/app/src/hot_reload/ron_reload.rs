use std::{io, path::PathBuf, time::Instant};

use engine_asset::SourceFiles;
use serde::de::DeserializeOwned;

use super::{POLL_INTERVAL, source_error};

/// Watches a RON file containing game-owned data in development builds.
///
/// Load the data before starting the game and poll between frames, for example
/// with [`crate::App::run_scene_with_refresh`]. Each poll can publish one valid
/// replacement for the next simulation step.
pub struct RonReload<T> {
    path: PathBuf,
    inputs: SourceFiles,
    last_poll: Instant,
    validate: fn(&T) -> io::Result<()>,
}

impl<T: DeserializeOwned> RonReload<T> {
    /// Reads and validates the initial value before returning a watcher.
    pub fn load(path: PathBuf, validate: fn(&T) -> io::Result<()>) -> io::Result<(Self, T)> {
        let mut reload = Self {
            path,
            inputs: SourceFiles::default(),
            last_poll: Instant::now(),
            validate,
        };
        let initial = reload.read()?;
        Ok((reload, initial))
    }

    /// Replaces `active` if the file changed and the new value is valid.
    /// A failed edit produces a source error once and keeps `active` intact.
    pub fn poll(&mut self, active: &mut T) -> io::Result<bool> {
        if self.last_poll.elapsed() < POLL_INTERVAL {
            return Ok(false);
        }
        self.last_poll = Instant::now();
        if !self.inputs.changed() {
            return Ok(false);
        }
        *active = self.read()?;
        Ok(true)
    }

    fn read(&mut self) -> io::Result<T> {
        let source = |error: &dyn std::error::Error| source_error(&self.path, error);
        let bytes = self
            .inputs
            .read(&self.path)
            .map_err(|error| source(&error))?;
        let value = ron::de::from_bytes(&bytes).map_err(|error| source(&error))?;
        (self.validate)(&value).map_err(|error| source(&error))?;
        Ok(value)
    }
}
