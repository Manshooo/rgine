use std::fmt;
use std::time::Instant;

use engine_ecs::World;
use engine_platform::{Commands, PlatformError, SurfaceSource, WindowEvent, WindowId};
use engine_render::scene::{MeshRegistry, Scene};
use engine_render::{DrawOutcome, Drawable};

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
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => error.fmt(formatter),
            Self::Graphics(error) => error.fmt(formatter),
            Self::Benchmark(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Graphics(error) => Some(error),
            Self::Benchmark(_) => None,
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
    light: scene::DirectionalLight,
    hemisphere_light: scene::HemisphereLight,
    skybox_enabled: bool,
}

pub(crate) struct Frames {
    surface: Option<Surface>,
    color: [f64; 4],
    scene: Option<SceneFrames>,
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
                light,
                hemisphere_light,
                skybox_enabled,
            }),
            ..Self::new(color)
        }
    }

    pub(crate) fn extract_for_redraw(&mut self, world: &World) -> bool {
        let Some(scene) = &mut self.scene else {
            return false;
        };
        scene::extract_into(world, &mut scene.scratch);
        if scene.snapshot == scene.scratch {
            return false;
        }
        std::mem::swap(&mut scene.snapshot, &mut scene.scratch);
        !self.occluded && !self.size.contains(&0)
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
            _ => {}
        }
        false
    }

    pub(crate) fn draw(
        &mut self,
        windows: &[WindowId],
        commands: &mut Commands,
    ) -> Result<(), RenderError> {
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
            return Ok(());
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
            self.surface = Some(Surface::Ready(Box::new(drawable)));
        }
        if let Some(Surface::Ready(drawable)) = &mut self.surface {
            drawable.resize(self.size);
            let end_of_orbit =
                if let (Some(benchmark), Some(scene)) = (&self.benchmark, &mut self.scene) {
                    benchmark.pose(&mut scene.snapshot)?
                } else {
                    false
                };
            let outcome = match &self.scene {
                Some(scene) => drawable.draw_scene(self.color, &scene.meshes, &scene.snapshot),
                None => drawable.draw(self.color),
            }
            .map_err(RenderError::Graphics)?;
            if let Some(benchmark) = &mut self.benchmark {
                if benchmark.present(drawable, outcome, end_of_orbit)? {
                    commands.exit();
                } else {
                    commands.request_redraw(windows);
                }
            } else if outcome == DrawOutcome::Retry {
                commands.request_redraw(windows);
            }
        }
        Ok(())
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
