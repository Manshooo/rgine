//! Exercises the public canvas API over a windowed 3D scene.

use engine_app::canvas::{Affine2, Rect, Vec2};
use engine_app::core::glam::{Quat, Vec3};
use engine_app::platform::WindowDesc;
use engine_app::render_resources::{SampledBinding, SamplerDescriptor, TextureDescriptor};
use engine_app::scene::{Camera, Mesh, MeshRegistry, Renderable, Transform};
use engine_app::{App, RenderError};

fn main() -> Result<(), RenderError> {
    engine_app::core::init_logging();
    let mut image = None;
    let mut app = App::new()
        .with_window(WindowDesc::new("rgine - canvas overlay").with_size(640, 480))
        .with_canvas(move |_, viewport, resources, canvas| {
            let image = match image.filter(|&handle| resources.sampled_binding(handle).is_ok()) {
                Some(handle) => handle,
                None => {
                    let texture =
                        resources.create_texture(TextureDescriptor::sampled_rgba8([2, 1]))?;
                    resources.write_texture(
                        texture,
                        0,
                        0,
                        &[255, 185, 42, 255, 35, 211, 188, 255],
                    )?;
                    let sampler = resources.create_sampler(SamplerDescriptor::NEAREST_CLAMP)?;
                    let handle =
                        resources.create_sampled_group(SampledBinding { texture, sampler })?;
                    image = Some(handle);
                    handle
                }
            };
            let centre = viewport.logical_size() * 0.5;
            let rect = |x, y, width, height| {
                Rect::from_min_size(centre + Vec2::new(x, y), Vec2::new(width, height))
            };
            let identity = Affine2::IDENTITY;
            canvas.solid_quad(
                rect(-112.0, -68.0, 224.0, 136.0),
                identity,
                [0.04, 0.1, 0.21, 0.66],
            );
            canvas.solid_quad(
                rect(-76.0, -33.0, 152.0, 66.0),
                identity,
                [0.17, 0.31, 0.57, 0.43],
            );
            canvas.textured_quad(
                Rect::from_min_size(Vec2::new(-38.0, -12.0), Vec2::new(76.0, 24.0)),
                Rect::from_min_size(Vec2::ZERO, Vec2::ONE),
                Affine2::from_angle_translation(-0.28, centre - Vec2::new(48.0, 18.0)),
                [1.0; 4],
                image,
            );
            canvas.push_clip(rect(8.0, -49.0, 86.0, 93.0));
            canvas.push_clip(rect(22.0, -58.0, 100.0, 78.0));
            for offset in (-52..=90).step_by(7) {
                let x = offset as f32;
                canvas.line(
                    centre + Vec2::new(x, 32.0),
                    centre + Vec2::new(x + 68.0, -55.0),
                    2.0,
                    identity,
                    [0.55, 0.88, 1.0, 0.55],
                );
            }
            canvas.pop_clip();
            canvas.pop_clip();
            canvas.line(
                centre + Vec2::new(-111.0, 67.0),
                centre + Vec2::new(111.0, 67.0),
                2.0,
                identity,
                [0.2, 0.76, 0.61, 0.9],
            );
            Ok(())
        });
    let mut meshes = MeshRegistry::default();
    let mesh = meshes.insert(Mesh::cube().with_checkerboard());
    let world = app.world_mut();
    let camera = world.spawn_empty();
    world.insert(camera, Camera::default()).unwrap();
    world.insert(camera, Transform::default()).unwrap();
    let cube = world.spawn_empty();
    world
        .insert(
            cube,
            Renderable {
                mesh,
                color: [0.17, 0.81, 0.66, 1.0],
            },
        )
        .unwrap();
    world
        .insert(
            cube,
            Transform {
                translation: Vec3::NEG_Z * 3.2,
                rotation: Quat::from_rotation_y(0.65) * Quat::from_rotation_x(0.3),
                scale: Vec3::splat(2.0),
            },
        )
        .unwrap();
    app.run_scene(meshes, [0.016, 0.023, 0.035, 1.0])
}
