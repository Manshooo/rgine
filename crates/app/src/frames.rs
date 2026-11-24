use std::fmt;

use engine_ecs::World;
use engine_platform::{Commands, PlatformError, SurfaceSource, WindowEvent, WindowId};
use engine_render::scene::{MeshRegistry, Scene};
use engine_render::{DrawOutcome, Drawable};

use crate::scene;

/// A failure from a rendered application.
#[derive(Debug)]
#[non_exhaustive]
pub enum RenderError {
    /// The platform could not run the application.
    Platform(PlatformError),
    /// The renderer could not initialize or present a frame.
    Graphics(engine_render::RenderError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Platform(error) => error.fmt(formatter),
            Self::Graphics(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Platform(error) => Some(error),
            Self::Graphics(error) => Some(error),
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
}

pub(crate) struct Frames {
    surface: Option<Surface>,
    color: [f64; 4],
    scene: Option<SceneFrames>,
    size: [u32; 2],
    occluded: bool,
}

impl Frames {
    pub(crate) fn new(color: [f64; 4]) -> Self {
        Self {
            surface: None,
            color,
            scene: None,
            size: [0, 0],
            occluded: false,
        }
    }

    pub(crate) fn with_scene(color: [f64; 4], meshes: MeshRegistry, world: &World) -> Self {
        Self {
            scene: Some(SceneFrames {
                meshes,
                snapshot: scene::extract(world),
                scratch: Scene::default(),
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

    pub(crate) fn attach(&mut self, source: SurfaceSource, size: [u32; 2]) {
        self.surface = Some(Surface::Pending(source));
        self.size = size;
        self.occluded = false;
    }

    pub(crate) fn detach(&mut self) {
        self.surface = None;
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
                if !self.size.contains(&0) {
                    commands.request_redraw(&[window]);
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
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
        if self.occluded || self.size.contains(&0) {
            return Ok(());
        }
        if let Some(Surface::Pending(source)) = self
            .surface
            .take_if(|surface| matches!(surface, Surface::Pending(_)))
        {
            let drawable = Drawable::new(source, self.size).map_err(RenderError::Graphics)?;
            self.surface = Some(Surface::Ready(Box::new(drawable)));
        }
        if let Some(Surface::Ready(drawable)) = &mut self.surface {
            drawable.resize(self.size);
            let outcome = match &self.scene {
                Some(scene) => drawable.draw_scene(self.color, &scene.meshes, &scene.snapshot),
                None => drawable.draw(self.color),
            }
            .map_err(RenderError::Graphics)?;
            if outcome == DrawOutcome::Retry {
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
        let frames = Frames::with_scene([0.0; 4], meshes, app.simulation.world());
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
