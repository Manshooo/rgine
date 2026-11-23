//! Recording metadata and validation for fixed-step input playback.

use std::fmt;
use std::time::Duration;

use engine_platform::InputEvent;
use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;

/// Input batches from a recorded simulation, in fixed-step order.
///
/// The recording depends on the original starting world, systems, and
/// resources. It does not include a snapshot of those values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputRecording {
    version: u32,
    fixed_step: Duration,
    start_step: u64,
    pub(crate) steps: Vec<Vec<InputEvent>>,
}

impl InputRecording {
    pub(crate) fn new(fixed_step: Duration, start_step: u64) -> Self {
        Self {
            version: VERSION,
            fixed_step,
            start_step,
            steps: Vec::new(),
        }
    }

    /// Number of fixed steps recorded, including steps with empty input.
    #[must_use]
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub(crate) fn validate(
        &self,
        fixed_step: Duration,
        completed_steps: u64,
    ) -> Result<(), ReplayError> {
        if self.version != VERSION {
            return Err(ReplayError::UnsupportedVersion(self.version));
        }
        if self.fixed_step != fixed_step {
            return Err(ReplayError::StepDurationMismatch);
        }
        if self.start_step != completed_steps {
            return Err(ReplayError::StartStepMismatch {
                expected: self.start_step,
                actual: completed_steps,
            });
        }
        Ok(())
    }
}

/// Why playback cannot start on the supplied simulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    /// The recording has an unknown format version.
    UnsupportedVersion(u32),
    /// Playback is configured with a different fixed step duration.
    StepDurationMismatch,
    /// The simulation is at the wrong step for this recording.
    StartStepMismatch {
        /// Completed steps at the start of recording.
        expected: u64,
        /// Completed steps in the playback simulation.
        actual: u64,
    },
    /// The simulation has live input queued or is currently recording.
    LiveInputActive,
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported recording version {version}")
            }
            Self::StepDurationMismatch => {
                f.write_str("recording uses a different fixed step duration")
            }
            Self::StartStepMismatch { expected, actual } => {
                write!(
                    f,
                    "recording starts after step {expected}, simulation is after step {actual}"
                )
            }
            Self::LiveInputActive => {
                f.write_str("stop recording and consume pending live input before playback")
            }
        }
    }
}

impl std::error::Error for ReplayError {}
