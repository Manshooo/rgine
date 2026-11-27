use std::fmt;
use std::time::Instant;

use engine_ecs::World;
use engine_platform::{Commands, PlatformError, SurfaceSource, WindowEvent, WindowId};
use engine_render::canvas::{Canvas, Viewport};
use engine_render::resources::{ResourceError, ResourceTable};
use engine_render::scene::{MeshRegistry, Scene};
use engine_render::{DrawOutcome, Drawable};

use crate::capture::Pending;
use crate::scene;

mod benchmark;
pub use benchmark::BenchmarkReport;
use benchmark::{Benchmark, DRAWABLE_SIZE};
#[cfg(feature = "hot-reload")]
mod shader_reload;
#[cfg(feature = "hot-reload")]
use shader_reload::ShaderReload;

/// A failure from a rendered application.
#[derive(Debug)]
#[non_exhaustive]
pub enum RenderError {
    /// The platform could not run the application.
    Platform(PlatformError),
    /// The renderer could not initialize or present a frame.
    Graphics(engine_render::RenderError),
    /// The benchmark configuration changed or the pass was interrupted.
    Benchmark(String),
    /// The requested fixed-step replay could not finish.
    Replay(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => error.fmt(formatter),
            Self::Graphics(error) => error.fmt(formatter),
            Self::Benchmark(message) | Self::Replay(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Graphics(error) => Some(error),
            Self::Benchmark(_) | Self::Replay(_) => None,
        }
    }
}

enum Surface {
    Pending(SurfaceSource),
    Ready(Box<Drawable>),
}

struct SceneFrames {
    meshes: MeshRegistry,
    snapshot: Scene,
    scratch: Scene,
    interpolating: bool,
    light: scene::DirectionalLight,
    hemisphere_light: scene::HemisphereLight,
    skybox_enabled: bool,
}

pub(crate) type CanvasDraw =
    dyn FnMut(&World, Viewport, &mut ResourceTable, &mut Canvas) -> Result<(), ResourceError>;

struct CanvasFrames {
    draw: Box<CanvasDraw>,
    list: Canvas,
}

pub(crate) struct Frames {
    surface: Option<Surface>,
    color: [f64; 4],
    scene: Option<SceneFrames>,
    canvas: Option<CanvasFrames>,
    size: [u32; 2],
    occluded: bool,
    benchmark: Option<Benchmark>,
    #[cfg(feature = "hot-reload")]
    shader_reload: Option<ShaderReload>,
}

impl Frames {
    pub(crate) fn new(color: [f64; 4]) -> Self {
        Self {
            surface: None,
            color,
            scene: None,
            canvas: None,
            size: [0, 0],
            occluded: false,
            benchmark: None,
            #[cfg(feature = "hot-reload")]
            shader_reload: None,
        }
    }

    pub(crate) fn with_benchmark(mut self, expected_adapter: String) -> Self {
        self.benchmark = Some(Benchmark::new(expected_adapter));
        #[cfg(feature = "hot-reload")]
        {
            self.shader_reload = None;
        }
        self
    }

    #[cfg(feature = "hot-reload")]
    pub(crate) fn with_shader_directory(mut self, directory: Option<std::path::PathBuf>) -> Self {
        self.shader_reload = directory.map(ShaderReload::new);
        self
    }

    #[cfg(feature = "hot-reload")]
    pub(crate) fn poll_scene_shaders(&mut self) -> bool {
        if self.occluded || self.size.contains(&0) {
            return false;
        }
        if let (Some(Surface::Ready(drawable)), Some(reload)) =
            (&mut self.surface, &mut self.shader_reload)
        {
            reload.poll(drawable)
        } else {
            false
        }
    }

    pub(crate) fn is_benchmark(&self) -> bool {
        self.benchmark.is_some()
    }

    pub(crate) fn finish_benchmark(mut self) -> Result<BenchmarkReport, RenderError> {
        self.benchmark
            .take()
            .and_then(|benchmark| benchmark.report)
            .ok_or_else(|| {
                RenderError::Benchmark("the window closed before the benchmark completed".into())
            })
    }

    pub(crate) fn benchmark_interrupted(&self) -> bool {
        self.benchmark
            .as_ref()
            .is_some_and(|benchmark| benchmark.interrupted)
    }

    pub(crate) fn with_scene(
        color: [f64; 4],
        meshes: MeshRegistry,
        world: &World,
        light: scene::DirectionalLight,
        hemisphere_light: scene::HemisphereLight,
        skybox_enabled: bool,
    ) -> Self {
        Self {
            scene: Some(SceneFrames {
                meshes,
                snapshot: scene::extract(world),
                scratch: Scene::default(),
                interpolating: false,
                light,
                hemisphere_light,
                skybox_enabled,
            }),
            ..Self::new(color)
        }
    }

    pub(crate) fn is_interpolating(&self) -> bool {
        self.scene.as_ref().is_some_and(|scene| scene.interpolating)
    }

    pub(crate) fn set_canvas(&mut self, draw: Box<CanvasDraw>) {
        if self.scene.is_some() && !self.is_benchmark() {
            self.canvas = Some(CanvasFrames {
                draw,
                list: Canvas::default(),
            });
        }
    }

    pub(crate) fn extract_for_redraw(&mut self, world: &World, alpha: f32) -> bool {
        let Some(scene) = &mut self.scene else {
            return false;
        };
        scene.interpolating = scene::extract_into(world, &mut scene.scratch, alpha);
        let changed = scene.snapshot != scene.scratch;
        if changed {
            std::mem::swap(&mut scene.snapshot, &mut scene.scratch);
        }
        (changed || scene.interpolating || self.canvas.is_some())
            && !self.occluded
            && !self.size.contains(&0)
    }

    pub(crate) fn refresh_scene(
        &mut self,
        world: &mut World,
        refresh: &mut crate::SceneRefresh<'_>,
    ) -> bool {
        self.scene
            .as_mut()
            .is_some_and(|scene| refresh(world, &mut scene.meshes))
    }

    pub(crate) fn attach(&mut self, source: SurfaceSource, size: [u32; 2]) {
        self.surface = Some(Surface::Pending(source));
        self.size = size;
        self.occluded = false;
    }

    pub(crate) fn detach(&mut self) {
        self.surface = None;
        if let Some(benchmark) = &mut self.benchmark {
            benchmark.interrupted = true;
        }
    }

    pub(crate) fn window_event(
        &mut self,
        event: WindowEvent,
        window: WindowId,
        commands: &mut Commands,
    ) -> bool {
        match event {
            WindowEvent::Resized { width, height } => {
                self.size = [width, height];
                if let Some(benchmark) = &mut self.benchmark {
                    benchmark.interrupted |= benchmark.started() && self.size != DRAWABLE_SIZE;
                }
                if !self.size.contains(&0) {
                    commands.request_redraw(&[window]);
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if let Some(benchmark) = &mut self.benchmark {
                    benchmark.interrupted |= occluded;
                }
                if !occluded {
                    commands.request_redraw(&[window]);
                }
            }
            WindowEvent::RedrawRequested => return true,
            WindowEvent::ScaleFactorChanged if !self.size.contains(&0) => {
                commands.request_redraw(&[window]);
            }
            _ => {}
        }
        false
    }

    pub(crate) fn draw(
        &mut self,
        world: &World,
        windows: &[WindowId],
        commands: &mut Commands,
        mut capture: Option<&mut Pending>,
    ) -> Result<DrawOutcome, RenderError> {
        if self.benchmark_interrupted() {
            return Err(RenderError::Benchmark(
                "the benchmark drawable changed or stopped being visible".into(),
            ));
        }
        if self.is_benchmark() && self.size != DRAWABLE_SIZE {
            let [width, height] = self.size;
            return Err(RenderError::Benchmark(format!(
                "benchmark requires a 1280 × 720 physical drawable; got {width} × {height}"
            )));
        }
        if self.occluded || self.size.contains(&0) {
            return Ok(DrawOutcome::Idle);
        }
        if let Some(Surface::Pending(source)) = self
            .surface
            .take_if(|surface| matches!(surface, Surface::Pending(_)))
        {
            let start = Instant::now();
            let mut drawable = if self.is_benchmark() {
                Drawable::new_for_benchmark(source, self.size)
            } else {
                Drawable::new(source, self.size)
            }
            .map_err(RenderError::Graphics)?;
            let startup = start.elapsed();
            if capture.is_some() {
                drawable.begin_frame_capture();
            }
            if let Some(scene) = &self.scene {
                drawable.set_directional_light(scene.light);
                drawable.set_hemisphere_light(scene.hemisphere_light);
                drawable.set_skybox_enabled(scene.skybox_enabled);
                let prepare = Instant::now();
                drawable
                    .prepare_scene_pipelines()
                    .map_err(RenderError::Graphics)?;
                #[cfg(feature = "hot-reload")]
                if let Some(reload) = &mut self.shader_reload {
                    reload.attach(&mut drawable);
                }
                if let Some(benchmark) = &mut self.benchmark {
                    benchmark.prepare(&drawable, scene, startup, prepare.elapsed())?;
                }
            }
            if let Some(capture) = &mut capture {
                capture.spans.push("initialize drawable", start);
            }
            self.surface = Some(Surface::Ready(Box::new(drawable)));
        }
        let interpolating = self.is_interpolating();
        if let Some(Surface::Ready(drawable)) = &mut self.surface {
            drawable.resize(self.size);
            if capture.is_some() {
                drawable.begin_frame_capture();
            }
            let end_of_orbit =
                if let (Some(benchmark), Some(scene)) = (&self.benchmark, &mut self.scene) {
                    benchmark.pose(&mut scene.snapshot)?
                } else {
                    false
                };
            let outcome = match (&self.scene, &mut self.canvas) {
                (Some(scene), Some(canvas)) => {
                    let start = capture.as_ref().map(|_| Instant::now());
                    canvas.list.clear();
                    let viewport = drawable.canvas_viewport().map_err(RenderError::Graphics)?;
                    (canvas.draw)(world, viewport, drawable.resources(), &mut canvas.list)
                        .map_err(|error| RenderError::Graphics(error.into()))?;
                    if let Some((capture, start)) = capture.as_mut().zip(start) {
                        capture.spans.push("build canvas", start);
                    }
                    drawable.draw_scene_with_canvas(
                        self.color,
                        &scene.meshes,
                        &scene.snapshot,
                        &canvas.list,
                    )
                }
                (Some(scene), None) => {
                    drawable.draw_scene(self.color, &scene.meshes, &scene.snapshot)
                }
                (None, _) => drawable.draw(self.color),
            }
            .map_err(RenderError::Graphics)?;
            if let Some(capture) = capture {
                capture.render(drawable.finish_frame_capture(), drawable.capture_graphics());
            }
            if let Some(benchmark) = &mut self.benchmark {
                if benchmark.present(drawable, outcome, end_of_orbit)? {
                    commands.exit();
                } else {
                    commands.request_redraw(windows);
                }
            } else if outcome == DrawOutcome::Retry || interpolating {
                commands.request_redraw(windows);
            }
            return Ok(outcome);
        }
        Ok(DrawOutcome::Idle)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::time::Duration;

    use engine_core::glam::Vec3;
    use engine_platform::{Command, Event};

    use super::*;
    use crate::scene::{Camera, Mesh, Renderable, Transform};
    use crate::{App, FixedStep, Runner};

    #[cfg(feature = "physics")]
    #[test]
    fn redraws_interpolate_the_same_fixed_trajectory_and_snap_on_respawn() {
        use engine_core::glam::Quat;

        use crate::physics::{
            BodyKind, Collider, ColliderShape, KinematicMotion, PhysicsInterpolation,
            PhysicsPlugin, PhysicsPose, RigidBody,
        };

        fn run(frame: Duration, positions: &[f32]) -> Vec<PhysicsPose> {
            let step = Duration::from_millis(10);
            let mut app = App::new()
                .with_fixed_step(FixedStep::new(step, NonZeroU32::MIN))
                .add_plugin(PhysicsPlugin::default());
            app.insert_resource(Vec::<PhysicsPose>::new());
            let mut meshes = MeshRegistry::default();
            let cube = meshes.insert(Mesh::cube());
            let world = app.world_mut();
            let pawn = world.spawn_empty();
            let scale = Vec3::new(2.0, 3.0, 4.0);
            let transform = Transform {
                scale,
                ..Transform::default()
            };
            let respawn = PhysicsPose {
                translation: Vec3::X * 100.0,
                rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            };
            world.insert(pawn, transform).unwrap();
            world
                .insert(
                    pawn,
                    Renderable {
                        mesh: cube,
                        color: [1.0; 4],
                    },
                )
                .unwrap();
            world
                .insert(
                    pawn,
                    RigidBody::new(BodyKind::Kinematic, PhysicsPose::default()),
                )
                .unwrap();
            world
                .insert(
                    pawn,
                    Collider::new(ColliderShape::Sphere(0.25), PhysicsPose::default(), false),
                )
                .unwrap();
            world.insert(pawn, KinematicMotion::default()).unwrap();
            world.insert(pawn, PhysicsInterpolation::default()).unwrap();
            app.add_system(move |world, commands, context, _| match context.index() {
                1..=3 => world
                    .query::<&mut KinematicMotion>()
                    .get_mut(pawn)
                    .unwrap()
                    .request_translation(Vec3::X),
                4 => {
                    commands.remove::<RigidBody>(pawn);
                    commands.insert(pawn, RigidBody::new(BodyKind::Kinematic, respawn));
                }
                _ => {}
            });
            let schedule = app.schedule_mut();
            let capture = schedule.add_stage();
            schedule.add_system(capture, move |world, _, context, _| {
                context
                    .resources_mut()
                    .get_mut::<Vec<PhysicsPose>>()
                    .unwrap()
                    .push(*world.get::<PhysicsPose>(pawn).unwrap());
            });

            let mut frames = app.scene_frames(meshes, [0.0; 4]);
            frames.size = [800, 600];
            let mut runner = Runner::new(app);
            runner.frames = Some(frames);
            let mut commands = Commands::default();
            runner.update(&[Event::Resumed], &mut commands);
            let _ = commands.drain();

            for (index, &x) in positions.iter().enumerate() {
                // The first moving pair starts at the same displayed pose.
                // It still needs a redraw to show movement before the next step.
                let redraw = frame * (index as u32 + 1) != step * 2;
                runner.advance_frame(frame, redraw, &mut commands);
                if !redraw {
                    assert_eq!(commands.drain(), [Command::RequestRedraw(runner.open[0])]);
                }
                let scene = runner.frames.as_ref().unwrap().scene.as_ref().unwrap();
                let model = scene.snapshot.objects[0].transform;
                let rotation = if x == respawn.translation.x {
                    respawn.rotation
                } else {
                    Quat::IDENTITY
                };
                let expected = engine_core::glam::Mat4::from_scale_rotation_translation(
                    scale,
                    rotation,
                    Vec3::X * x,
                );
                assert!(model.abs_diff_eq(expected, 0.0001), "{model:?} at {x}");
                assert_eq!(
                    *runner.simulation.world().get::<Transform>(pawn).unwrap(),
                    transform
                );
            }

            let world = runner.simulation.world();
            let samples = world.get::<PhysicsInterpolation>(pawn).unwrap();
            assert_eq!(samples.sample(0.0), Some(respawn));
            assert_eq!(samples.sample(1.0), Some(respawn));
            runner
                .simulation
                .resources()
                .get::<Vec<PhysicsPose>>()
                .unwrap()
                .clone()
        }

        let at_250_hz = run(
            Duration::from_millis(4),
            &[0.0, 0.0, 1.0, 1.0, 1.0, 1.4, 1.8, 2.2, 2.6, 100.0],
        );
        let at_125_hz = run(Duration::from_millis(8), &[0.0, 1.0, 1.4, 2.2, 100.0]);
        assert_eq!(at_250_hz, at_125_hz);
        assert_eq!(
            at_125_hz
                .iter()
                .map(|pose| pose.translation.x)
                .collect::<Vec<_>>(),
            [1.0, 2.0, 3.0, 100.0],
        );
    }

    #[test]
    fn redraws_for_deferred_spawns_transform_changes_and_despawns() {
        let step = Duration::from_millis(10);
        let mut app = App::new().with_fixed_step(FixedStep::new(step, NonZeroU32::MIN));
        let mut meshes = MeshRegistry::default();
        let cube = meshes.insert(Mesh::cube());
        let world = app.world_mut();
        let camera = world.spawn_empty();
        world.insert(camera, Camera::default()).unwrap();
        world.insert(camera, Transform::default()).unwrap();
        let original = world.spawn_empty();
        let drawable = Renderable {
            mesh: cube,
            color: [1.0; 4],
        };
        world.insert(original, Transform::default()).unwrap();
        world.insert(original, drawable).unwrap();
        app.add_system(move |_, commands, context, _| match context.index() {
            1 => {
                commands
                    .spawn_empty()
                    .insert(Transform::default())
                    .insert(drawable);
            }
            2 => {
                let moved = Transform {
                    translation: Vec3::new(1.0, 0.0, 3.0),
                    ..Transform::default()
                };
                commands.insert(camera, moved);
                commands.insert(original, moved);
            }
            3 => commands.despawn(original),
            _ => {}
        });
        let frames = Frames::with_scene(
            [0.0; 4],
            meshes,
            app.simulation.world(),
            scene::DirectionalLight::default(),
            scene::HemisphereLight::default(),
            false,
        );
        let mut runner = Runner::new(app);
        runner.frames = Some(frames);
        let mut commands = Commands::default();
        runner.update(&[Event::Resumed], &mut commands);
        let window = runner.open[0];
        runner.frames.as_mut().unwrap().window_event(
            WindowEvent::Resized {
                width: 800,
                height: 600,
            },
            window,
            &mut commands,
        );
        let _ = commands.drain();

        for (index, count) in [2, 2, 1].into_iter().enumerate() {
            runner.advance_frame(step, false, &mut commands);
            let snapshot = &runner
                .frames
                .as_ref()
                .unwrap()
                .scene
                .as_ref()
                .unwrap()
                .snapshot;
            assert_eq!(snapshot, &scene::extract(runner.simulation.world()));
            assert_eq!(snapshot.objects.len(), count);
            assert!(snapshot.objects.iter().all(|object| object.mesh == cube));
            if index == 1 {
                let translation = Vec3::new(1.0, 0.0, 3.0);
                assert_eq!(
                    snapshot.camera.unwrap().view.w_axis.truncate(),
                    -translation
                );
                assert!(
                    snapshot
                        .objects
                        .iter()
                        .any(|object| object.transform.w_axis.truncate() == translation)
                );
            }
            assert_eq!(commands.drain(), [Command::RequestRedraw(window)]);
        }

        runner.advance_frame(step, false, &mut commands);
        assert!(commands.drain().is_empty());
        let prior = scene::extract(runner.simulation.world());
        let frames = runner.frames.as_mut().unwrap();
        frames.detach();
        assert_eq!(frames.scene.as_ref().unwrap().snapshot, prior);
    }
}
