//! ECS components read by the scene renderer after the fixed schedule completes.

use engine_core::glam::{Mat4, Quat, Vec3};
use engine_ecs::{Component, World};
use engine_render::scene::{Scene, SceneCamera, SceneObject};

pub use engine_render::scene::{Mesh, MeshKey, MeshRegistry};

/// A world-space transform for a camera or a renderable object.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Position in world units.
    pub translation: Vec3,
    /// Orientation in world space. Cameras look along local negative Z.
    pub rotation: Quat,
    /// Local scale for mesh instances.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

/// Perspective settings for an entity with a transform.
///
/// The scene uses the first camera in ECS query order.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Vertical field of view in radians.
    pub vertical_fov: f32,
    /// Positive distance to the near clipping plane.
    pub near: f32,
    /// Distance to the far clipping plane.
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            vertical_fov: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 100.0,
        }
    }
}

/// An opaque mesh instance on an entity with a transform.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Renderable {
    /// Stable key from the app's mesh registry.
    pub mesh: MeshKey,
    /// Linear RGBA color of the primitive.
    pub color: [f32; 4],
}

pub(crate) fn extract(world: &World) -> Scene {
    let mut scene = Scene::default();
    extract_into(world, &mut scene);
    scene
}

pub(crate) fn extract_into(world: &World, scene: &mut Scene) {
    scene.camera = None;
    scene.objects.clear();
    world
        .query::<(&Camera, &Transform)>()
        .for_each(|_, (camera, transform)| {
            scene.camera.get_or_insert_with(|| SceneCamera {
                view: Mat4::from_rotation_translation(transform.rotation, transform.translation)
                    .inverse(),
                vertical_fov: camera.vertical_fov,
                near: camera.near,
                far: camera.far,
            });
        });
    world
        .query::<(&Renderable, &Transform)>()
        .for_each(|_, (renderable, transform)| {
            scene.objects.push(SceneObject {
                mesh: renderable.mesh,
                transform: Mat4::from_scale_rotation_translation(
                    transform.scale,
                    transform.rotation,
                    transform.translation,
                ),
                color: renderable.color,
            });
        });
}
