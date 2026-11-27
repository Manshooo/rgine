#![forbid(unsafe_code)]

//! Integration, plugins and lifecycle.
//!
//! The application owns the loop policy - what a window is for, when to exit -
//! and reaches the OS only through [`engine_platform`]. No windowing backend is
//! named here, so replacing that backend does not touch this crate.

pub use engine_core as core;
pub use engine_ecs as ecs;
pub use engine_ecs::Resources;
pub use engine_platform as platform;
pub use engine_render::{canvas, resources as render_resources};
pub use frames::BenchmarkReport;
pub use frames::RenderError;
pub use input_actions::{ActionBinding, ActionEvent, ActionState, InputActions, InputContext};
pub use replay::{InputRecording, ReplayError};
pub use simulation::{FixedStep, Simulation, StepContext};

pub mod capture;
mod frames;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
mod input_actions;
#[cfg(feature = "physics")]
pub mod physics;
mod replay;
pub mod scene;
mod simulation;

use std::time::{Duration, Instant};

use capture::{CaptureLimit, FrameCapture, Replay, ReplayReport, Session};
use engine_ecs::{Schedule, StageId, Tick, World};
use engine_platform::{
    Commands, Event, InputEvent, PlatformApp, PlatformError, SurfaceEvent, WindowDesc, WindowEvent,
    WindowId,
};
use engine_render::DrawOutcome;
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
    canvas: Option<Box<frames::CanvasDraw>>,
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
            canvas: None,
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

    /// Builds a screen-space overlay immediately before drawing a scene frame.
    /// The draw list is cleared first. The viewport supplies logical bounds
    /// and display scale. Recreate cached texture handles when the resource
    /// table changes after native surface invalidation.
    pub fn with_canvas(
        mut self,
        draw: impl FnMut(
            &World,
            canvas::Viewport,
            &mut render_resources::ResourceTable,
            &mut canvas::Canvas,
        ) -> Result<(), render_resources::ResourceError>
        + 'static,
    ) -> Self {
        self.canvas = Some(Box::new(draw));
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

    /// Renders scene components extracted after fixed updates and interpolates
    /// opted-in physics poses on redraws between updates.
    /// The mesh registry supplies stable geometry across native surface changes.
    pub fn run_scene(
        self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
    ) -> Result<(), RenderError> {
        let frames = self.scene_frames(meshes, clear_color);
        self.run_with_frames(frames, None).map(drop)
    }

    /// Runs a scene normally and returns the first bounded set of attempted frames.
    pub fn run_scene_captured(
        self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
        limit: CaptureLimit,
    ) -> Result<FrameCapture, RenderError> {
        let frames = self.scene_frames(meshes, clear_color);
        let mut session = Session {
            capture: Some(FrameCapture::new(limit)),
            replay: None,
        };
        self.run_with_session(frames, None, Some(&mut session))?;
        Ok(session.capture.take().expect("capture configured"))
    }

    /// Presents one recorded fixed step per delivered frame, for controlled comparisons.
    /// Warmup frames run the same input path. Acquisition retries reuse the same world.
    pub fn run_scene_replay(
        self,
        meshes: scene::MeshRegistry,
        clear_color: [f64; 4],
        recording: InputRecording,
        warmup_frames: usize,
        capture: Option<CaptureLimit>,
    ) -> Result<ReplayReport, RenderError> {
        self.simulation
            .validate_replay(&recording)
            .map_err(|error| RenderError::Replay(error.to_string()))?;
        let steps = recording.step_count();
        if warmup_frames >= steps {
            return Err(RenderError::Replay(
                "replay needs at least one measured frame".into(),
            ));
        }
        let frames = self.scene_frames(meshes, clear_color);
        let mut session = Session {
            capture: capture.map(FrameCapture::new),
            replay: Some(Replay {
                recording,
                warmup: warmup_frames,
                presented: 0,
                stepped: false,
                previous_present: None,
                intervals: Vec::with_capacity(steps - warmup_frames),
            }),
        };
        self.run_with_session(frames, None, Some(&mut session))?;
        let replay = session.replay.take().expect("replay configured");
        if replay.presented != steps {
            return Err(RenderError::Replay(format!(
                "presented {} of {steps} replay frames",
                replay.presented
            )));
        }
        Ok(ReplayReport {
            warmup_frames,
            presented_frames: replay.presented - warmup_frames,
            present_intervals_ns: replay.intervals,
            capture: session.capture,
        })
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
        self.run_with_session(frames, refresh, None)
    }

    fn run_with_session<'a>(
        mut self,
        mut frames: Frames,
        refresh: Option<Box<SceneRefresh<'a>>>,
        session: Option<&'a mut Session>,
    ) -> Result<Frames, RenderError> {
        if let Some(draw) = self.canvas.take() {
            frames.set_canvas(draw);
        }
        if session
            .as_deref()
            .and_then(|session| session.capture.as_ref())
            .is_some_and(|capture| capture.gpu_requested)
        {
            frames.enable_gpu_timing();
        }
        let mut runner = Runner::new(self);
        runner.frames = Some(frames);
        runner.refresh = refresh;
        runner.session = session;
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
    session: Option<&'a mut Session>,
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
            session: None,
        }
    }
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        let elapsed = self.simulation.clock.sample_elapsed();
        let replaying = self
            .session
            .as_ref()
            .is_some_and(|session| session.replay.is_some());
        let measured = self
            .session
            .as_deref_mut()
            .and_then(Session::capture_attempt)
            .and_then(FrameCapture::pending);
        let input_start = measured.map(|frame| {
            let now = Instant::now();
            frame.start.get_or_insert(now);
            now
        });
        let mut active = !self.open.is_empty();
        let mut redraw = false;
        let mut closing = false;
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
                    closing = true;
                    commands.exit();
                }
                Event::Window { window, event } if self.open.contains(window) => {
                    if let Some(frames) = &mut self.frames {
                        redraw |= frames.window_event(*event, *window, commands);
                    }
                }
                Event::Input(input)
                    if active && !replaying && self.open.contains(&input.window) =>
                {
                    self.simulation.queue_input(std::iter::once(*input));
                }
                _ => {}
            }
        }
        if !events.is_empty()
            && let Some((frame, start)) = pending(&mut self.session).zip(input_start)
        {
            frame.spans.push("platform input and events", start);
        }
        let outcome = if active && replaying {
            redraw.then(|| self.replay_frame(commands)).flatten()
        } else if active {
            self.advance_frame(elapsed, redraw, commands)
        } else {
            None
        };
        let replay_present = if outcome == Some(DrawOutcome::Presented)
            && let Some(replay) = self
                .session
                .as_deref_mut()
                .and_then(|session| session.replay.as_mut())
        {
            if replay.presented + 1 == replay.recording.step_count() {
                commands.exit();
            } else if commands.queued_redraws() == 0 {
                commands.request_redraw(&self.open);
            }
            Some(Instant::now())
        } else {
            None
        };
        if let Some(capture) = self
            .session
            .as_deref_mut()
            .and_then(Session::capture_attempt)
        {
            if let Some(frame) = capture.pending() {
                frame.redraw_requests += commands.queued_redraws();
            }
            if let Some(outcome) = outcome {
                let label = match outcome {
                    DrawOutcome::Presented => "presented",
                    DrawOutcome::Retry => "retry",
                    DrawOutcome::Idle => "idle",
                };
                capture.finish(label, Instant::now());
            }
        }
        if let Some((replay, now)) = self
            .session
            .as_deref_mut()
            .and_then(|session| session.replay.as_mut())
            .zip(replay_present)
        {
            replay.present(now);
            closing |= replay.presented == replay.recording.step_count();
        }
        self.collect_gpu_timings(closing);
    }

    fn collect_gpu_timings(&mut self, finish: bool) {
        let capture = self
            .session
            .as_deref_mut()
            .and_then(|session| session.capture.as_mut())
            .filter(|capture| capture.gpu_requested);
        if let Some((frames, capture)) = self.frames.as_mut().zip(capture) {
            frames.collect_gpu_timings(capture, finish);
        }
    }

    fn advance_frame(
        &mut self,
        elapsed: Duration,
        redraw: bool,
        commands: &mut Commands,
    ) -> Option<DrawOutcome> {
        let steps = self
            .simulation
            .advance_profiled(elapsed, pending(&mut self.session));
        let frames = self.frames.as_mut()?;
        let refreshed = self.refresh.as_mut().is_some_and(|refresh| {
            frames.refresh_scene(self.simulation.world_mut(), &mut **refresh)
        });
        let changed = if !frames.is_benchmark()
            && (steps > 0 || refreshed || (redraw && frames.is_interpolating()))
        {
            extract(
                frames,
                &self.simulation,
                self.simulation.interpolation_alpha(),
                pending(&mut self.session),
            )
        } else {
            false
        };
        #[cfg(feature = "hot-reload")]
        let changed = frames.poll_scene_shaders() || changed;
        if redraw || frames.benchmark_interrupted() {
            self.draw(commands)
        } else if changed {
            commands.request_redraw(&self.open);
            None
        } else {
            None
        }
    }

    fn replay_frame(&mut self, commands: &mut Commands) -> Option<DrawOutcome> {
        let session = self.session.as_deref_mut().expect("replay session");
        let replay = session.replay.as_mut().expect("replay configured");
        if replay.presented == replay.recording.step_count() {
            return None;
        }
        if !replay.stepped {
            let mut profile = session
                .capture
                .as_mut()
                .filter(|_| replay.presented >= replay.warmup)
                .and_then(FrameCapture::pending);
            let keys = &replay.recording.steps[replay.presented];
            self.simulation.replay_step(keys, profile.as_deref_mut());
            extract(
                self.frames.as_mut().expect("scene frames"),
                &self.simulation,
                1.0,
                profile,
            );
            replay.stepped = true;
        }
        self.draw(commands)
    }

    fn draw(&mut self, commands: &mut Commands) -> Option<DrawOutcome> {
        let result = self.frames.as_mut()?.draw(
            self.simulation.world(),
            &self.open,
            commands,
            pending(&mut self.session),
        );
        match result {
            Ok(outcome) => Some(outcome),
            Err(error) => {
                self.render_failure.get_or_insert(error);
                commands.exit();
                None
            }
        }
    }
}

fn pending<'a>(session: &'a mut Option<&mut Session>) -> Option<&'a mut capture::Pending> {
    session
        .as_deref_mut()
        .and_then(Session::capture_attempt)
        .and_then(FrameCapture::pending)
}

fn extract(
    frames: &mut Frames,
    simulation: &Simulation,
    alpha: f32,
    capture: Option<&mut capture::Pending>,
) -> bool {
    let start = capture.as_ref().map(|_| Instant::now());
    let changed = frames.extract_for_redraw(simulation.world(), alpha);
    if let Some((capture, start)) = capture.zip(start) {
        capture.spans.push("scene extraction", start);
    }
    changed
}

impl PlatformApp for &mut Runner<'_> {
    fn update(&mut self, events: &[Event], commands: &mut Commands) {
        Runner::update(self, events, commands);
    }

    fn surface_event(&mut self, event: SurfaceEvent) {
        if matches!(event, SurfaceEvent::Invalidating(window) if self.open.contains(&window)) {
            self.collect_gpu_timings(false);
        }
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
    fn rendered_window_requests_redraw_on_open_restore_and_scale_change() {
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

        runner.update(
            &[window_event(window, WindowEvent::ScaleFactorChanged)],
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
