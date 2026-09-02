#![forbid(unsafe_code)]

//! Integration, plugins and lifecycle.
//!
//! The application owns the loop policy - what a window is for, when to exit -
//! and reaches the OS only through [`engine_platform`]. No windowing backend is
//! named here, so replacing that backend does not touch this crate.

pub use engine_core as core;
pub use engine_ecs as ecs;
pub use engine_platform as platform;

use engine_platform::{
    Commands, Event, PlatformApp, PlatformError, WindowDesc, WindowEvent, WindowId,
};

pub trait Plugin {
    fn build(&self, app: &mut App);
}

pub struct App {
    plugins: Vec<Box<dyn Plugin>>,
    window: WindowDesc,
}

impl Default for App {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            window: WindowDesc::new("rgine - bootstrap"),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_plugin<P: Plugin + 'static>(mut self, plugin: P) -> Self {
        self.plugins.push(Box::new(plugin));
        self
    }

    /// Describes the window opened when the application starts.
    pub fn with_window(mut self, window: WindowDesc) -> Self {
        self.window = window;
        self
    }

    /// Runs the application until its window is closed.
    pub fn run(self) -> Result<(), PlatformError> {
        engine_platform::run(Runner::new(self.window))
    }
}

/// Loop policy: one window, opened when the platform is ready, closed with it.
struct Runner {
    window: WindowDesc,
    open: Vec<WindowId>,
}

impl Runner {
    fn new(window: WindowDesc) -> Self {
        Self {
            window,
            open: Vec::new(),
        }
    }
}

impl PlatformApp for Runner {
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
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
                    commands.close_windows(&self.open);
                    self.open.clear();
                }
                Event::Window {
                    event: WindowEvent::CloseRequested,
                    ..
                } => commands.exit(),
                _ => {}
            }
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

    #[test]
    fn opens_one_window_when_the_platform_resumes() {
        let mut runner = Runner::new(WindowDesc::new("test"));
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);

        let drained = commands.drain();
        assert_eq!(drained.len(), 1);
        assert!(matches!(drained[0], Command::CreateWindow { .. }));
    }

    #[test]
    fn does_not_reopen_a_window_that_is_already_open() {
        let mut runner = Runner::new(WindowDesc::new("test"));
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let _ = commands.drain();
        runner.update(&[Event::Resumed], &mut commands);

        assert!(commands.drain().is_empty());
    }

    #[test]
    fn reopens_the_window_after_a_suspend() {
        let mut runner = Runner::new(WindowDesc::new("test"));
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
        let mut runner = Runner::new(WindowDesc::new("test"));
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
        let mut runner = Runner::new(WindowDesc::new("test"));
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
