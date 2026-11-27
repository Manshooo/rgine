//! ECS components read by the scene renderer after the fixed schedule completes.

use engine_core::glam::{Mat4, Quat, Vec3};
use engine_ecs::{Component, World};
use engine_render::scene::{Scene, SceneCamera, SceneObject};

pub use engine_render::scene::{
    DirectionalLight, HemisphereLight, Mesh, MeshKey, MeshRegistry, PbrMaterial,
};

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

impl Transform {
    fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
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

/// Static node matrix relative to the entity transform. Preserves nested scale and shear.
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeMatrix(pub Mat4);

pub(crate) fn extract(world: &World) -> Scene {
    let mut scene = Scene::default();
    extract_into(world, &mut scene, 1.0);
    scene
}

pub(crate) fn extract_into(world: &World, scene: &mut Scene, alpha: f32) -> bool {
    let interpolating = std::cell::Cell::new(false);
    #[cfg(feature = "physics")]
    let poses = world.query::<&crate::physics::PhysicsInterpolation>();
    let transform_at = |entity, transform: &Transform| {
        #[cfg(not(feature = "physics"))]
        let _ = (entity, alpha);
        #[cfg(feature = "physics")]
        if let Some(samples) = poses.get(entity)
            && let Some(pose) = samples.sample(alpha)
        {
            interpolating.set(interpolating.get() || samples.is_moving());
            return Transform {
                translation: pose.translation,
                rotation: pose.rotation,
                scale: transform.scale,
            };
        }
        *transform
    };

    scene.camera = None;
    scene.objects.clear();
    world
        .query::<(&Camera, &Transform)>()
        .for_each(|entity, (camera, transform)| {
            scene.camera.get_or_insert_with(|| {
                let transform = transform_at(entity, transform);
                SceneCamera {
                    view: Mat4::from_rotation_translation(
                        transform.rotation,
                        transform.translation,
                    )
                    .inverse(),
                    vertical_fov: camera.vertical_fov,
                    near: camera.near,
                    far: camera.far,
                }
            });
        });
    world
        .query::<(&Renderable, &Transform)>()
        .for_each(|entity, (renderable, transform)| {
            scene.objects.push(SceneObject {
                mesh: renderable.mesh,
                transform: transform_at(entity, transform).matrix()
                    * world
                        .get::<NodeMatrix>(entity)
                        .map_or(Mat4::IDENTITY, |m| m.0),
                color: renderable.color,
            });
        });
    #[cfg(feature = "hot-reload")]
    world
        .query::<(&crate::hot_reload::PropInstances, &Transform)>()
        .for_each(|entity, (instances, transform)| {
            let world_transform = transform_at(entity, transform).matrix();
            scene
                .objects
                .extend(instances.0.iter().map(|object| SceneObject {
                    transform: world_transform * object.transform,
                    ..*object
                }));
        });
    interpolating.get()
}
