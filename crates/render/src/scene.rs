//! CPU scene data and stable mesh keys used by the renderer.

mod gpu;

use std::sync::atomic::{AtomicU64, Ordering};

use engine_core::glam::{Mat4, Vec3};

pub(crate) use gpu::SceneRenderer;

static NEXT_REGISTRY: AtomicU64 = AtomicU64::new(0);

/// Identifies a mesh in its CPU registry across renderer recreation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct MeshKey {
    registry: u64,
    index: usize,
}

/// Retains the CPU geometry needed to upload meshes to a fresh GPU device.
pub struct MeshRegistry {
    id: u64,
    meshes: Vec<Mesh>,
}

impl Default for MeshRegistry {
    fn default() -> Self {
        Self {
            id: NEXT_REGISTRY.fetch_add(1, Ordering::Relaxed),
            meshes: Vec::new(),
        }
    }
}

impl MeshRegistry {
    /// Registers geometry for the lifetime of the scene.
    pub fn insert(&mut self, mesh: Mesh) -> MeshKey {
        let index = self.meshes.len();
        self.meshes.push(mesh);
        MeshKey {
            registry: self.id,
            index,
        }
    }

    fn get(&self, key: MeshKey) -> Option<&Mesh> {
        if key.registry != self.id {
            return None;
        }
        self.meshes.get(key.index)
    }
}

struct Vertex {
    position: Vec3,
    normal: Vec3,
}

/// Indexed procedural geometry with a position and normal at each vertex.
pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Mesh {
    /// A unit cube centered at the origin with flat face normals.
    pub fn cube() -> Self {
        let mut mesh = Self {
            vertices: Vec::with_capacity(24),
            indices: Vec::with_capacity(36),
        };
        for (normal, tangent) in [
            (Vec3::X, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y),
            (Vec3::Y, Vec3::X),
            (Vec3::NEG_Y, Vec3::X),
            (Vec3::Z, Vec3::X),
            (Vec3::NEG_Z, Vec3::X),
        ] {
            mesh.quad(normal * 0.5, normal, tangent);
        }
        mesh
    }

    /// A unit square on the XZ plane whose front faces positive Y.
    pub fn plane() -> Self {
        let mut mesh = Self {
            vertices: Vec::with_capacity(4),
            indices: Vec::with_capacity(6),
        };
        mesh.quad(Vec3::ZERO, Vec3::Y, Vec3::X);
        mesh
    }

    fn quad(&mut self, center: Vec3, normal: Vec3, tangent: Vec3) {
        let first = self.vertices.len() as u32;
        let u = tangent * 0.5;
        let v = normal.cross(tangent) * 0.5;
        for position in [
            center - u - v,
            center + u - v,
            center + u + v,
            center - u + v,
        ] {
            self.vertices.push(Vertex { position, normal });
        }
        self.indices
            .extend([first, first + 1, first + 2, first, first + 2, first + 3]);
    }
}

/// A perspective camera extracted from the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneCamera {
    /// Transforms world coordinates into camera space.
    pub view: Mat4,
    /// Vertical field of view in radians.
    pub vertical_fov: f32,
    /// Positive distance to the near clipping plane.
    pub near: f32,
    /// Distance to the far clipping plane, greater than the near distance.
    pub far: f32,
}

impl SceneCamera {
    fn clip_from_world(self, [width, height]: [u32; 2]) -> Option<Mat4> {
        let valid = width > 0
            && height > 0
            && self.view.is_finite()
            && self.vertical_fov > 0.0
            && self.vertical_fov < std::f32::consts::PI
            && self.near > 0.0
            && self.far.is_finite()
            && self.far > self.near;
        if !valid {
            return None;
        }
        let clip = Mat4::perspective_rh(
            self.vertical_fov,
            width as f32 / height as f32,
            self.near,
            self.far,
        ) * self.view;
        clip.is_finite().then_some(clip)
    }
}

/// One visible mesh and its world-space transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneObject {
    /// Source geometry in the scene registry.
    pub mesh: MeshKey,
    /// Transforms mesh-local coordinates into world space.
    pub transform: Mat4,
    /// Linear RGBA color used by the primitive shader.
    pub color: [f32; 4],
}

/// The camera and objects to present after a completed simulation step.
#[derive(Debug, Default, PartialEq)]
pub struct Scene {
    /// A missing camera produces a clear frame.
    pub camera: Option<SceneCamera>,
    /// Opaque mesh instances in extraction order.
    pub objects: Vec<SceneObject>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_tracks_the_drawable_aspect_and_uses_wgpu_depth() {
        let camera = SceneCamera {
            view: Mat4::IDENTITY,
            vertical_fov: 1.0,
            near: 0.1,
            far: 20.0,
        };
        let square = camera.clip_from_world([480, 480]).unwrap();
        let wide = camera.clip_from_world([960, 480]).unwrap();
        assert_eq!(wide.x_axis.x, square.x_axis.x / 2.0);
        assert_eq!(wide.y_axis.y, square.y_axis.y);
        assert!(wide.project_point3(Vec3::NEG_Z * camera.near).z.abs() < 1e-6);
        assert!((wide.project_point3(Vec3::NEG_Z * camera.far).z - 1.0).abs() < 1e-6);
        assert!(camera.clip_from_world([0, 480]).is_none());
    }
}
