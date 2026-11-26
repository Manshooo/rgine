#![forbid(unsafe_code)]

//! Integration, plugins and lifecycle.
//!
//! The application owns the loop policy - what a window is for, when to exit -
//! and reaches the OS only through [`engine_platform`]. No windowing backend is
//! named here, so replacing that backend does not touch this crate.

pub use engine_core as core;
pub use engine_ecs as ecs;
pub use engine_platform as platform;
pub use frames::BenchmarkReport;
pub use frames::RenderError;
pub use input_actions::{ActionBinding, ActionEvent, ActionState, InputActions, InputContext};
pub use replay::{InputRecording, ReplayError};
pub use simulation::{FixedStep, Resources, Simulation, StepContext};

mod frames;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
mod input_actions;
mod replay;
pub mod scene;
mod simulation;

use std::time::Duration;

use engine_ecs::{Schedule, StageId, Tick, World};
use engine_platform::{
    Commands, Event, InputEvent, PlatformApp, PlatformError, SurfaceEvent, WindowDesc, WindowEvent,
    WindowId,
};
use frames::Frames;

/// Configures a subsystem through the application's registration APIs.
pub trait Plugin {
    /// Registers components, resources, and systems when added to the app.
    fn build(&self, app: &mut App);
}

pub struct App {
    simulation: Simulation,
    update: StageId,
    window: WindowDesc,
    directional_light: scene::DirectionalLight,
    hemisphere_light: scene::HemisphereLight,
    skybox_enabled: bool,
    #[cfg(feature = "hot-reload")]
    shader_directory: Option<std::path::PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        let mut simulation = Simulation::default();
        let update = simulation.schedule.add_stage();
        Self {
            simulation,
            update,
            window: WindowDesc::new("rgine - bootstrap"),
            directional_light: scene::DirectionalLight::default(),
            hemisphere_light: scene::HemisphereLight::default(),
            skybox_enabled: false,
            #[cfg(feature = "hot-reload")]
            shader_directory: None,
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

    /// Installs the game's input resolver and replaces any previous resolver.
    /// The callback runs before resolving each physical event and can update
    /// the context stack, update resources, or return `true` to consume the event.
    pub fn set_input_actions<A: Copy + Eq + Send + Sync + 'static>(
        &mut self,
        actions: InputActions<A>,
        intercept: impl FnMut(&InputEvent, &mut InputActions<A>, &mut Resources) -> bool + 'static,
    ) {
        self.simulation.set_input_actions(actions, intercept);
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

    /// Sets the direct light for the scene rendered by [`Self::run_scene`].
    pub fn with_directional_light(mut self, light: scene::DirectionalLight) -> Self {
        self.directional_light = light;
        self
    }

    /// Sets the upper and lower diffuse lighting for the windowed scene.
    pub fn with_hemisphere_light(mut self, light: scene::HemisphereLight) -> Self {
        self.hemisphere_light = light;
        self
    }

    /// Adds the procedural sky behind opaque geometry in the windowed scene.
    pub fn with_skybox(mut self) -> Self {
        self.skybox_enabled = true;
        self
    }

    /// Watches the WGSL files in a development shader directory.
    #[cfg(feature = "hot-reload")]
    pub fn with_scene_shader_directory(mut self, directory: std::path::PathBuf) -> Self {
        self.shader_directory = Some(directory);
        self
    }

    /// Runs the application until its window is closed.
    pub fn run(self) -> Result<(), PlatformError> {
        engine_platform::run(&mut Runner::new(self))
    }

    /// Runs the windowed app and presents a linear RGBA clear color on redraw.
    pub fn run_rendered(self, clear_color: [f64; 4]) -> Result<(), RenderError> {
        self.run_with_frames(Frames::new(clear_color), None)
            .map(drop)
    }

    /// Renders scene components extracted from the world after each fixed update.
    /// The mesh registry supplies stable geometry across native surface changes.
    pub fn run_scene(
        self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
    ) -> Result<(), RenderError> {
        let frames = self.scene_frames(meshes, clear_color);
        self.run_with_frames(frames, None).map(drop)
    }

    /// Runs a scene with a development refresh between simulation and extraction.
    /// The callback returns whether it committed a world or mesh registry update.
    #[cfg(feature = "hot-reload")]
    pub fn run_scene_with_refresh(
        self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
        refresh: impl FnMut(&mut World, &mut scene::MeshRegistry) -> bool,
    ) -> Result<(), RenderError> {
        let frames = self.scene_frames(meshes, clear_color);
        self.run_with_frames(frames, Some(Box::new(refresh)))
            .map(drop)
    }

    /// Measures the Render v1 camera pass in a 1280 × 720 window using DX12 and classic bindings.
    /// A completed run returns measurements even when they fall outside the pass thresholds.
    pub fn run_scene_benchmark(
        mut self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
        expected_adapter: String,
    ) -> Result<BenchmarkReport, RenderError> {
        self.window = self.window.with_size(1280, 720);
        let frames = self
            .scene_frames(meshes, clear_color)
            .with_benchmark(expected_adapter);
        self.run_with_frames(frames, None)?.finish_benchmark()
    }

    fn scene_frames(&self, meshes: scene::MeshRegistry, clear_color: [f64; 4]) -> Frames {
        let frames = Frames::with_scene(
            clear_color,
            meshes,
            self.simulation.world(),
            self.directional_light,
            self.hemisphere_light,
            self.skybox_enabled,
        );
        #[cfg(feature = "hot-reload")]
        let frames = frames.with_shader_directory(self.shader_directory.clone());
        frames
    }

    fn run_with_frames(
        self,
        frames: Frames,
        refresh: Option<Box<SceneRefresh<'_>>>,
    ) -> Result<Frames, RenderError> {
        let mut runner = Runner::new(self);
        runner.frames = Some(frames);
        runner.refresh = refresh;
        engine_platform::run(&mut runner).map_err(RenderError::Platform)?;
        match runner.render_failure {
            Some(error) => Err(error),
            None => Ok(runner
                .frames
                .expect("frames installed before the event loop")),
        }
    }

    /// Runs the windowed simulation and returns its input recording when the
    /// event loop exits successfully. Some mobile event loops never return.
    pub fn run_recorded(self) -> Result<InputRecording, PlatformError> {
        let mut runner = Runner::new(self);
        runner.simulation.start_recording();
        engine_platform::run(&mut runner)?;
        Ok(runner
            .simulation
            .finish_recording()
            .expect("recording started before the event loop"))
    }
}

/// Loop policy: one window, opened when the platform is ready, closed with it.
type SceneRefresh<'a> = dyn FnMut(&mut World, &mut scene::MeshRegistry) -> bool + 'a;

struct Runner<'a> {
    window: WindowDesc,
    open: Vec<WindowId>,
    simulation: Simulation,
    frames: Option<Frames>,
    refresh: Option<Box<SceneRefresh<'a>>>,
    render_failure: Option<RenderError>,
}

impl Runner<'_> {
    fn new(app: App) -> Self {
        Self {
            window: app.window,
            open: Vec::new(),
            simulation: app.simulation,
            frames: None,
            refresh: None,
            render_failure: None,
        }
    }
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        let elapsed = self.simulation.clock.sample_elapsed();
        let mut active = !self.open.is_empty();
        let mut redraw = false;
        for event in events {
            match event {
                // Resume can fire more than once: on platforms that tear
                // surfaces down, every wake-up needs the window opened again.
                Event::Resumed => {
                    if self.open.is_empty() {
                        commands.create_windows(std::slice::from_ref(&self.window), &mut self.open);
                        if self.frames.is_some() {
                            commands.request_redraw(&self.open);
                        }
                    }
                }
                Event::Suspended => {
                    active = false;
                    redraw = false;
                    self.simulation.discard_suspended_input();
                    commands.close_windows(&self.open);
                    self.open.clear();
                    if self.frames.as_ref().is_some_and(Frames::is_benchmark) {
                        commands.exit();
                    }
                }
                Event::Window {
                    window,
                    event: WindowEvent::CloseRequested | WindowEvent::Destroyed,
                } if self.open.contains(window) => {
                    active = false;
                    commands.exit();
                }
                Event::Window { window, event } if self.open.contains(window) => {
                    if let Some(frames) = &mut self.frames {
                        redraw |= frames.window_event(*event, *window, commands);
                    }
                }
                Event::Input(input) if active && self.open.contains(&input.window) => {
                    self.simulation.queue_input(std::iter::once(*input));
                }
                _ => {}
            }
        }
        if active {
            self.advance_frame(elapsed, redraw, commands);
        }
    }

    fn advance_frame(&mut self, elapsed: Duration, redraw: bool, commands: &mut Commands) {
        let steps = self.simulation.advance(elapsed);
        let Some(frames) = &mut self.frames else {
            return;
        };
        let refreshed = self.refresh.as_mut().is_some_and(|refresh| {
            frames.refresh_scene(self.simulation.world_mut(), &mut **refresh)
        });
        let changed = !frames.is_benchmark()
            && (steps > 0 || refreshed)
            && frames.extract_for_redraw(self.simulation.world());
        #[cfg(feature = "hot-reload")]
        let changed = frames.poll_scene_shaders() || changed;
        if redraw || frames.benchmark_interrupted() {
            if let Err(error) = frames.draw(&self.open, commands) {
                if self.render_failure.is_none() {
                    self.render_failure = Some(error);
                }
                commands.exit();
            }
        } else if changed {
            commands.request_redraw(&self.open);
        }
    }
}

impl PlatformApp for &mut Runner<'_> {
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        Runner::update(self, events, commands);
    }

    fn surface_event(&mut self, event: SurfaceEvent) {
        if let Some(frames) = &mut self.frames {
            match event {
                SurfaceEvent::Available(window, source, size) if self.open.contains(&window) => {
                    frames.attach(source, size);
                }
                SurfaceEvent::Invalidating(window) if self.open.contains(&window) => {
                    frames.detach();
                }
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

    fn runner() -> Runner<'static> {
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
    fn suspension_keeps_the_input_cancellation_for_the_next_step() {
        use engine_platform::{Input, InputTimestamp, MouseButton};

        let mut app = App::new();
        let mut actions = InputActions::default();
        actions.push_context(
            InputContext::default().bind(ActionBinding::MouseButton(MouseButton::Left), ()),
        );
        app.set_input_actions(actions, |_, _, _| false);
        app.add_system(|_, _, step, _| {
            let actions = step.actions::<()>().unwrap();
            match step.index() {
                1 => assert_eq!(actions.events(), [ActionEvent::Pressed(())]),
                2 => assert_eq!(actions.events(), [ActionEvent::Released(())]),
                _ => assert!(actions.events().is_empty()),
            }
        });
        let mut runner = Runner::new(app);
        let mut commands = Commands::default();
        runner.update(&[Event::Resumed], &mut commands);
        let window = runner.open[0];
        let input = |input| InputEvent {
            window,
            timestamp: InputTimestamp::Received(Duration::ZERO),
            input,
        };
        runner.simulation.queue_input([input(Input::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        })]);
        let step = Duration::from_nanos(16_666_667);
        assert_eq!(runner.simulation.advance(step), 1);
        runner.update(
            &[Event::Input(input(Input::Cancelled)), Event::Suspended],
            &mut commands,
        );
        runner.update(&[Event::Resumed], &mut commands);
        assert_eq!(runner.simulation.advance(step), 1);
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

    #[test]
    fn rendered_window_requests_redraw_on_open_and_restore() {
        let mut runner = runner();
        runner.frames = Some(Frames::new([0.0, 0.0, 0.0, 1.0]));
        let mut commands = Commands::default();

        runner.update(&[Event::Resumed], &mut commands);
        let window = runner.open[0];
        assert!(matches!(
            commands.drain().as_slice(),
            [Command::CreateWindow { .. }, Command::RequestRedraw(target)] if *target == window
        ));

        runner.update(
            &[window_event(
                window,
                WindowEvent::Resized {
                    width: 0,
                    height: 0,
                },
            )],
            &mut commands,
        );
        assert!(commands.drain().is_empty());

        runner.update(
            &[window_event(
                window,
                WindowEvent::Resized {
                    width: 640,
                    height: 480,
                },
            )],
            &mut commands,
        );
        assert_eq!(commands.drain(), vec![Command::RequestRedraw(window)]);
    }

    #[test]
    fn ignores_queued_events_from_a_window_closed_on_suspend() {
        let mut runner = runner();
        runner.frames = Some(Frames::new([0.0, 0.0, 0.0, 1.0]));
        let mut commands = Commands::default();
        runner.update(&[Event::Resumed], &mut commands);
        let old = runner.open[0];
        let _ = commands.drain();

        runner.update(
            &[
                Event::Suspended,
                Event::Resumed,
                window_event(old, WindowEvent::Destroyed),
                window_event(old, WindowEvent::RedrawRequested),
            ],
            &mut commands,
        );
        let window = runner.open[0];
        assert_ne!(window, old);
        assert!(matches!(
            commands.drain().as_slice(),
            [Command::CloseWindow(closed), Command::CreateWindow { .. }, Command::RequestRedraw(target)]
                if *closed == old && *target == window
        ));
    }
}
