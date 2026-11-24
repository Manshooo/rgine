//! Run the first 3D scene with `cargo run --locked -p engine-app --example procedural_scene`.

use engine_app::core::glam::{Mat4, Vec3};
use engine_app::ecs::{Component, World};
use engine_app::platform::{Input, KeyCode, PhysicalKey, WindowDesc};
use engine_app::scene::{Camera, Mesh, MeshRegistry, Renderable, Transform};
use engine_app::{App, RenderError};

#[derive(Component)]
struct OptionalBox;

fn main() -> Result<(), RenderError> {
    engine_app::core::init_logging();
    let mut meshes = MeshRegistry::default();
    let cube = meshes.insert(Mesh::cube());
    let plane = meshes.insert(Mesh::plane().with_checkerboard());
    let mut app =
        App::new().with_window(WindowDesc::new("rgine - procedural scene").with_size(960, 640));
    let mut orbit = Orbit {
        yaw: 0.28,
        height: 0.48,
        radius: 6.2,
    };
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, orbit.transform()).unwrap();

    let foreground = world.spawn_empty();
    world
        .insert(
            foreground,
            Transform {
                translation: Vec3::new(0.65, 0.62, 0.8),
                scale: Vec3::splat(1.24),
                ..Transform::default()
            },
        )
        .unwrap();
    world
        .insert(
            foreground,
            Renderable {
                mesh: cube,
                color: [0.92, 0.25, 0.08, 1.0],
            },
        )
        .unwrap();

    let background = world.spawn_empty();
    world
        .insert(
            background,
            Transform {
                translation: Vec3::new(-0.45, 0.9, -0.8),
                scale: Vec3::new(1.55, 1.8, 1.4),
                ..Transform::default()
            },
        )
        .unwrap();
    world
        .insert(
            background,
            Renderable {
                mesh: cube,
                color: [0.06, 0.56, 0.68, 1.0],
            },
        )
        .unwrap();

    let floor = world.spawn_empty();
    world
        .insert(
            floor,
            Transform {
                scale: Vec3::splat(12.0),
                ..Transform::default()
            },
        )
        .unwrap();
    world
        .insert(
            floor,
            Renderable {
                mesh: plane,
                color: [0.13, 0.17, 0.25, 1.0],
            },
        )
        .unwrap();

    app.add_system(move |world, commands, step, _| {
        let mut orbit_changed = false;
        let mut toggle = false;
        for event in step.input() {
            let Input::Key {
                key: PhysicalKey::Code(key),
                pressed: true,
                repeat,
            } = event.input
            else {
                continue;
            };
            match key {
                KeyCode::ArrowLeft => orbit.yaw -= 0.09,
                KeyCode::ArrowRight => orbit.yaw += 0.09,
                KeyCode::ArrowUp => orbit.height = (orbit.height + 0.06).min(1.35),
                KeyCode::ArrowDown => orbit.height = (orbit.height - 0.06).max(0.08),
                KeyCode::KeyW => orbit.radius = (orbit.radius - 0.25).max(2.5),
                KeyCode::KeyS => orbit.radius = (orbit.radius + 0.25).min(15.0),
                KeyCode::Space if !repeat => {
                    toggle = !toggle;
                    continue;
                }
                _ => continue,
            }
            orbit_changed = true;
        }
        if orbit_changed {
            commands.insert(camera, orbit.transform());
        }
        if toggle {
            if let Some(entity) = optional_box(world) {
                commands.despawn(entity);
            } else {
                commands
                    .spawn_empty()
                    .insert(OptionalBox)
                    .insert(Transform {
                        translation: Vec3::new(1.8, 0.38, -0.9),
                        scale: Vec3::splat(0.76),
                        ..Transform::default()
                    })
                    .insert(Renderable {
                        mesh: cube,
                        color: [0.65, 0.56, 0.12, 1.0],
                    });
            }
        }
    });
    app.run_scene(meshes, [0.025, 0.035, 0.06, 1.0])
}

fn optional_box(world: &World) -> Option<engine_app::ecs::Entity> {
    let mut found = None;
    world
        .query::<&OptionalBox>()
        .for_each(|entity, _| found = Some(entity));
    found
}

struct Orbit {
    yaw: f32,
    height: f32,
    radius: f32,
}

impl Orbit {
    fn transform(&self) -> Transform {
        let target = Vec3::new(0.0, 0.6, 0.0);
        let horizontal = self.radius * self.height.cos();
        let position = target
            + Vec3::new(
                horizontal * self.yaw.sin(),
                self.radius * self.height.sin(),
                horizontal * self.yaw.cos(),
            );
        let (_, rotation, translation) = Mat4::look_at_rh(position, target, Vec3::Y)
            .inverse()
            .to_scale_rotation_translation();
        Transform {
            translation,
            rotation,
            ..Transform::default()
        }
    }
}
