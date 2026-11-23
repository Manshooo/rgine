#![forbid(unsafe_code)]

//! Integration, plugins and lifecycle.
//!
//! The application owns the loop policy - what a window is for, when to exit -
//! and reaches the OS only through [`engine_platform`]. No windowing backend is
//! named here, so replacing that backend does not touch this crate.

pub use engine_core as core;
pub use engine_ecs as ecs;
pub use engine_platform as platform;
pub use simulation::{FixedStep, Resources, Simulation, StepContext};

mod simulation;

use engine_ecs::{Schedule, StageId, Tick, World};
use engine_platform::{
    Commands, Event, PlatformApp, PlatformError, WindowDesc, WindowEvent, WindowId,
};

/// Configures a subsystem through the application's registration APIs.
pub trait Plugin {
    /// Registers components, resources, and systems when added to the app.
    fn build(&self, app: &mut App);
}

pub struct App {
    simulation: Simulation,
    update: StageId,
    window: WindowDesc,
}

impl Default for App {
    fn default() -> Self {
        let mut simulation = Simulation::default();
        let update = simulation.schedule.add_stage();
        Self {
            simulation,
            update,
            window: WindowDesc::new("rgine - bootstrap"),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the plugin immediately so registration follows plugin order.
    pub fn add_plugin<P: Plugin>(mut self, plugin: P) -> Self {
        plugin.build(&mut self);
        self
    }

    /// Sets the timing policy before the first simulation step.
    pub fn with_fixed_step(mut self, fixed_step: FixedStep) -> Self {
        self.simulation.fixed_step = fixed_step;
        self
    }

    /// The initialization world, including its component registry.
    pub fn world_mut(&mut self) -> &mut World {
        self.simulation.world_mut()
    }

    /// Registers or replaces an app-wide resource.
    pub fn insert_resource<T: Send + Sync + 'static>(&mut self, value: T) -> Option<T> {
        self.simulation.context.resources_mut().insert(value)
    }

    /// Registers a system in the default fixed-update stage.
    pub fn add_system(
        &mut self,
        system: impl FnMut(&World, &mut engine_ecs::Commands, &mut StepContext, Tick) + 'static,
    ) {
        self.simulation.schedule.add_system(self.update, system);
    }

    /// Configures additional stages and their systems.
    pub fn schedule_mut(&mut self) -> &mut Schedule<StepContext> {
        &mut self.simulation.schedule
    }

    /// Builds the simulation for a headless caller.
    #[must_use]
    pub fn into_simulation(self) -> Simulation {
        self.simulation
    }

    /// Describes the window opened when the application starts.
    pub fn with_window(mut self, window: WindowDesc) -> Self {
        self.window = window;
        self
    }

    /// Runs the application until its window is closed.
    pub fn run(self) -> Result<(), PlatformError> {
        engine_platform::run(Runner::new(self))
    }
}

/// Loop policy: one window, opened when the platform is ready, closed with it.
struct Runner {
    window: WindowDesc,
    open: Vec<WindowId>,
    simulation: Simulation,
}

impl Runner {
    fn new(app: App) -> Self {
        Self {
            window: app.window,
            open: Vec::new(),
            simulation: app.simulation,
        }
    }
}

impl PlatformApp for Runner {
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        let elapsed = self.simulation.clock.sample_elapsed();
        let mut active = !self.open.is_empty();
        for event in events {
            match event {
                // Resume can fire more than once: on platforms that tear
                // surfaces down, every wake-up needs the window opened again.
                Event::Resumed => {
                    if self.open.is_empty() {
                        commands.create_windows(std::slice::from_ref(&self.window), &mut self.open);
                    }
                }
                Event::Suspended => {
                    active = false;
                    commands.close_windows(&self.open);
                    self.open.clear();
                }
                Event::Window {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    active = false;
                    commands.exit();
                }
                _ => {}
            }
        }
        if active {
            self.simulation.advance(elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_platform::Command;

    fn window_event(window: WindowId, event: WindowEvent) -> Event {
        Event::Window { window, event }
    }

    fn runner() -> Runner {
        Runner::new(App::new().with_window(WindowDesc::new("test")))
    }

    #[test]
    fn opens_one_window_when_the_platform_resumes() {
        let mut runner = runner();
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);

        let drained = commands.drain();
        assert_eq!(drained.len(), 1);
        assert!(matches!(drained[0], Command::CreateWindow { .. }));
    }

    #[test]
    fn does_not_reopen_a_window_that_is_already_open() {
        let mut runner = runner();
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let _ = commands.drain();
        runner.update(&[Event::Resumed], &mut commands);

        assert!(commands.drain().is_empty());
    }

    #[test]
    fn reopens_the_window_after_a_suspend() {
        let mut runner = runner();
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let opened = runner.open.clone();
        let _ = commands.drain();

        runner.update(&[Event::Suspended], &mut commands);
        assert_eq!(commands.drain(), vec![Command::CloseWindow(opened[0])]);

        runner.update(&[Event::Resumed], &mut commands);
        assert!(matches!(
            commands.drain().as_slice(),
            [Command::CreateWindow { .. }]
        ));
    }

    #[test]
    fn exits_when_the_window_is_closed() {
        let mut runner = runner();
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let window = runner.open[0];
        let _ = commands.drain();

        runner.update(
            &[window_event(window, WindowEvent::CloseRequested)],
            &mut commands,
        );

        assert_eq!(commands.drain(), vec![Command::Exit]);
    }

    #[test]
    fn ignores_events_it_has_no_policy_for() {
        let mut runner = runner();
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let window = runner.open[0];
        let _ = commands.drain();

        runner.update(
            &[
                window_event(
                    window,
                    WindowEvent::Resized {
                        width: 800,
                        height: 600,
                    },
                ),
                window_event(window, WindowEvent::RedrawRequested),
            ],
            &mut commands,
        );

        assert!(commands.drain().is_empty());
    }
}
