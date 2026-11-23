//! Fixed-step simulation shared by the platform runner and headless callers.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::Duration;

use engine_core::time::Clock;
use engine_ecs::{Schedule, World};
use engine_platform::InputEvent;

use crate::{InputRecording, ReplayError};

/// Timing and catch-up policy for the fixed simulation.
#[derive(Debug, Clone, Copy)]
pub struct FixedStep {
    duration: Duration,
    max_steps: NonZeroU32,
}

impl FixedStep {
    /// Sets the simulation step and the maximum number run per frame.
    ///
    /// # Panics
    ///
    /// If the duration is zero.
    #[must_use]
    pub fn new(duration: Duration, max_steps: NonZeroU32) -> Self {
        assert!(!duration.is_zero(), "fixed timestep must be positive");
        Self {
            duration,
            max_steps,
        }
    }
}

impl Default for FixedStep {
    fn default() -> Self {
        Self::new(
            Duration::from_nanos(16_666_667),
            NonZeroU32::new(8).unwrap(),
        )
    }
}

/// App-wide data with one value per Rust type.
#[derive(Default)]
pub struct Resources(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Resources {
    /// Inserts a resource, returning the previously stored value of its type.
    pub fn insert<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.0
            .insert(TypeId::of::<T>(), Box::new(value))
            .map(|old| *old.downcast::<T>().expect("resource key has one Rust type"))
    }

    /// Borrows the resource of type `T`, if one has been inserted.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Mutably borrows the resource of type `T`.
    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.0.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }
}

/// Fixed-step timing and resources available to each app system.
#[derive(Default)]
pub struct StepContext {
    resources: Resources,
    delta: Duration,
    index: u64,
    input: Vec<InputEvent>,
}

impl StepContext {
    /// The same fixed duration on every step of a simulation.
    #[must_use]
    pub fn delta(&self) -> Duration {
        self.delta
    }

    /// The current step number, starting at one.
    #[must_use]
    pub fn index(&self) -> u64 {
        self.index
    }

    /// The ordered input batch assigned to this step.
    #[must_use]
    pub fn input(&self) -> &[InputEvent] {
        &self.input
    }

    /// The resources registered with the app.
    #[must_use]
    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    /// Mutably borrows the app's resources.
    pub fn resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }
}

/// The world and schedule for a fixed-step application. It can run without
/// a window or wall-clock timing.
#[derive(Default)]
pub struct Simulation {
    pub(crate) world: World,
    pub(crate) schedule: Schedule<StepContext>,
    pub(crate) context: StepContext,
    pub(crate) fixed_step: FixedStep,
    pub(crate) clock: Clock,
    pending_input: Vec<InputEvent>,
    recording: Option<InputRecording>,
}

impl Simulation {
    /// Advances by the elapsed frame duration and returns the number of steps
    /// actually run. Whole steps beyond the catch-up limit are discarded.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        let steps = self.clock.accumulate(
            elapsed,
            self.fixed_step.duration,
            self.fixed_step.max_steps.get(),
        );
        for _ in 0..steps {
            // Assign all queued input to the first available step. Further
            // catch-up steps receive empty batches until new input arrives.
            std::mem::swap(&mut self.context.input, &mut self.pending_input);
            if let Some(recording) = &mut self.recording {
                recording.steps.push(self.context.input.clone());
            }
            self.run_step();
            self.context.input.clear();
        }
        steps
    }

    fn run_step(&mut self) {
        self.context.delta = self.fixed_step.duration;
        self.context.index += 1;
        self.schedule.run(&mut self.world, &mut self.context);
    }

    /// The world after the latest completed simulation step.
    #[must_use]
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutably borrows the world between advances.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The app's current resources.
    #[must_use]
    pub fn resources(&self) -> &Resources {
        self.context.resources()
    }

    /// Buffers input in arrival order for the next fixed step.
    pub fn queue_input(&mut self, events: impl IntoIterator<Item = InputEvent>) {
        self.pending_input.extend(events);
    }

    pub(crate) fn discard_pending_input(&mut self) {
        self.pending_input.clear();
    }

    /// Starts recording the next step and returns whether a new recording began.
    pub fn start_recording(&mut self) -> bool {
        if self.recording.is_some() {
            return false;
        }
        self.recording = Some(InputRecording::new(
            self.fixed_step.duration,
            self.context.index,
        ));
        true
    }

    /// Stops recording and returns the recorded fixed steps, if active.
    pub fn finish_recording(&mut self) -> Option<InputRecording> {
        self.recording.take()
    }

    /// Plays each recorded batch through the normal fixed-step schedule.
    /// Start from the original world, systems, and resources. The wall-clock
    /// accumulator is left untouched.
    pub fn replay(&mut self, recording: &InputRecording) -> Result<(), ReplayError> {
        recording.validate(self.fixed_step.duration, self.context.index)?;
        if self.recording.is_some() || !self.pending_input.is_empty() {
            return Err(ReplayError::LiveInputActive);
        }
        for input in &recording.steps {
            self.context.input.extend_from_slice(input);
            self.run_step();
            self.context.input.clear();
        }
        Ok(())
    }
}
