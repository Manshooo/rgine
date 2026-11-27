//! CPU scene data and stable mesh keys used by the renderer.

mod gpu;

use std::sync::atomic::{AtomicU64, Ordering};

use engine_asset::{gltf_prop::Prop, ktx2::Texture};
use engine_core::glam::{Mat4, Vec3};

pub(crate) use gpu::{SCENE_COLOR_FORMATS, SceneRenderer};

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
    textures: Vec<Texture>,
}

impl Default for MeshRegistry {
    fn default() -> Self {
        Self {
            id: NEXT_REGISTRY.fetch_add(1, Ordering::Relaxed),
            meshes: Vec::new(),
            textures: Vec::new(),
        }
    }
}

impl MeshRegistry {
    /// Register the meshes of a static glTF prop and return its opaque instances.
    pub fn register_prop(&mut self, prop: &Prop) -> std::io::Result<Vec<SceneObject>> {
        self.register(prop, None)
    }

    /// Register a static glTF prop and use the KTX2 texture as the base color
    /// map of each primitive. The mesh UVs and material factors still apply.
    pub fn register_textured_prop(
        &mut self,
        prop: &Prop,
        base_color: Texture,
    ) -> std::io::Result<Vec<SceneObject>> {
        self.register(prop, Some(base_color))
    }

    fn register(
        &mut self,
        prop: &Prop,
        texture: Option<Texture>,
    ) -> std::io::Result<Vec<SceneObject>> {
        use std::io::{Error, ErrorKind};
        let invalid = || Error::new(ErrorKind::InvalidData, "invalid cached glTF geometry");
        let surface = if texture.is_some() {
            MaterialTexture::Imported {
                registry: self.id,
                index: self.textures.len(),
            }
        } else {
            MaterialTexture::Solid
        };
        let mut meshes = Vec::new();
        for mesh in &prop.meshes {
            let mut primitives = Vec::new();
            for p in mesh {
                if p.positions.len() != p.normals.len()
                    || p.positions.len() != p.uv.len()
                    || p.indices.is_empty()
                    || p.indices.len() % 3 != 0
                    || p.indices.iter().any(|&i| i as usize >= p.positions.len())
                {
                    return Err(invalid());
                }
                let vertices = p
                    .positions
                    .iter()
                    .zip(&p.normals)
                    .zip(&p.uv)
                    .map(|((pos, normal), uv)| Vertex {
                        position: Vec3::from(*pos),
                        normal: Vec3::from(*normal),
                        uv: *uv,
                    })
                    .collect();
                primitives.push((
                    Mesh {
                        vertices,
                        indices: p.indices.clone(),
                        surface,
                        material: Some(PbrMaterial::new(p.metallic, p.roughness)),
                    },
                    p.base_color,
                ));
            }
            meshes.push(primitives);
        }
        if prop.instances.iter().any(|i| i.mesh >= meshes.len()) {
            return Err(invalid());
        }
        if let Some(texture) = texture {
            self.textures.push(texture);
        }
        let keys: Vec<Vec<_>> = meshes
            .into_iter()
            .map(|mesh| mesh.into_iter().map(|(p, c)| (self.insert(p), c)).collect())
            .collect();
        let mut objects = Vec::new();
        for i in &prop.instances {
            for &(mesh, color) in &keys[i.mesh] {
                objects.push(SceneObject {
                    mesh,
                    color,
                    transform: Mat4::from_cols_array(&i.transform),
                });
            }
        }
        Ok(objects)
    }

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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MaterialTexture {
    Solid,
    Checkerboard,
    Imported { registry: u64, index: usize },
}

struct Vertex {
    position: Vec3,
    normal: Vec3,
    uv: [f32; 2],
}

/// Metallic-roughness parameters for an opaque mesh surface.
/// The instance color and any sampled texture provide its base color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PbrMaterial {
    /// Zero for a dielectric, one for a metal.
    pub metallic: f32,
    /// Perceptual roughness from zero (smooth) to one (rough).
    pub roughness: f32,
}

impl PbrMaterial {
    /// Creates a surface, clamping finite inputs to the supported range.
    pub fn new(metallic: f32, roughness: f32) -> Self {
        Self {
            metallic: if metallic.is_finite() {
                metallic.clamp(0.0, 1.0)
            } else {
                0.0
            },
            roughness: if roughness.is_finite() {
                roughness.clamp(0.0, 1.0)
            } else {
                0.5
            },
        }
    }
}

/// The direct light used by the opaque scene pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    direction_to_light: Vec3,
    intensity: f32,
}

impl DirectionalLight {
    /// Creates a white directional light from a vector pointing toward the light
    /// and a nonnegative linear intensity. Returns `None` for invalid inputs.
    pub fn new(direction_to_light: Vec3, intensity: f32) -> Option<Self> {
        let direction_to_light = direction_to_light.try_normalize()?;
        (intensity.is_finite() && intensity >= 0.0).then_some(Self {
            direction_to_light,
            intensity,
        })
    }
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self::new(Vec3::new(-0.5, 1.0, 0.8), 3.2).expect("valid default scene light")
    }
}

/// Diffuse scene lighting from the upper and lower world-space hemispheres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HemisphereLight {
    upper: Vec3,
    lower: Vec3,
}

impl HemisphereLight {
    /// Creates a light from finite, nonnegative linear RGB intensities.
    pub fn new(upper: Vec3, lower: Vec3) -> Option<Self> {
        (upper.is_finite()
            && lower.is_finite()
            && upper.min_element() >= 0.0
            && lower.min_element() >= 0.0)
            .then_some(Self { upper, lower })
    }
}

impl Default for HemisphereLight {
    fn default() -> Self {
        Self {
            upper: Vec3::splat(0.20),
            lower: Vec3::splat(0.20),
        }
    }
}

/// Indexed procedural geometry with a position and normal at each vertex.
pub struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    surface: MaterialTexture,
    material: Option<PbrMaterial>,
}

impl Mesh {
    /// A unit cube centered at the origin with flat face normals.
    pub fn cube() -> Self {
        let mut mesh = Self {
            vertices: Vec::with_capacity(24),
            indices: Vec::with_capacity(36),
            surface: MaterialTexture::Solid,
            material: None,
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
            surface: MaterialTexture::Solid,
            material: None,
        };
        mesh.quad(Vec3::ZERO, Vec3::Y, Vec3::X);
        mesh
    }

    /// Bakes XZ quads into one solid mesh. Each entry supplies its center and full X and Z extents.
    pub fn plane_batch(quads: impl IntoIterator<Item = (Vec3, [f32; 2])>) -> Option<Self> {
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            surface: MaterialTexture::Solid,
            material: None,
        };
        for (center, [width, depth]) in quads {
            if !center.is_finite()
                || !width.is_finite()
                || !depth.is_finite()
                || width <= 0.0
                || depth <= 0.0
                || mesh.vertices.len() > u32::MAX as usize - 4
            {
                return None;
            }
            mesh.push_quad(
                center,
                Vec3::Y,
                Vec3::X * (width * 0.5),
                Vec3::NEG_Z * (depth * 0.5),
            );
        }
        (!mesh.indices.is_empty()).then_some(mesh)
    }

    /// A unit-height cylinder with diameter one, centered on its local Y axis.
    pub fn cylinder() -> Self {
        const SEGMENTS: u32 = 48;
        let mut vertices = Vec::with_capacity((10 * SEGMENTS) as usize);
        let mut indices = Vec::with_capacity((12 * SEGMENTS) as usize);
        let radial = |segment| {
            let (z, x) = (segment as f32 * std::f32::consts::TAU / SEGMENTS as f32).sin_cos();
            Vec3::new(x, 0.0, z)
        };
        for segment in 0..SEGMENTS {
            let a = radial(segment);
            let b = radial(segment + 1);
            let top = Vec3::Y * 0.5;
            let bottom = -top;
            let first = vertices.len() as u32;
            let u = segment as f32 / SEGMENTS as f32;
            let next_u = (segment + 1) as f32 / SEGMENTS as f32;
            for (position, normal, uv) in [
                (a * 0.5 + bottom, a, [u, 1.0]),
                (a * 0.5 + top, a, [u, 0.0]),
                (b * 0.5 + top, b, [next_u, 0.0]),
                (b * 0.5 + bottom, b, [next_u, 1.0]),
            ] {
                vertices.push(Vertex {
                    position,
                    normal,
                    uv,
                });
            }
            indices.extend([first, first + 1, first + 2, first, first + 2, first + 3]);

            for (center, normal, rim) in [(top, Vec3::Y, [b, a]), (bottom, Vec3::NEG_Y, [a, b])] {
                let first = vertices.len() as u32;
                for local in [Vec3::ZERO, rim[0] * 0.5, rim[1] * 0.5] {
                    vertices.push(Vertex {
                        position: center + local,
                        normal,
                        uv: [local.x + 0.5, local.z + 0.5],
                    });
                }
                indices.extend([first, first + 1, first + 2]);
            }
        }
        Self {
            vertices,
            indices,
            surface: MaterialTexture::Solid,
            material: None,
        }
    }

    /// A smooth sphere of diameter one centered at the origin.
    pub fn sphere() -> Self {
        const RINGS: u32 = 24;
        const SEGMENTS: u32 = 48;
        let mut vertices = Vec::with_capacity(((RINGS + 1) * (SEGMENTS + 1)) as usize);
        let mut indices = Vec::with_capacity((6 * SEGMENTS * (RINGS - 1)) as usize);
        for ring in 0..=RINGS {
            let v = ring as f32 / RINGS as f32;
            let (radius, y) = (v * std::f32::consts::PI).sin_cos();
            for segment in 0..=SEGMENTS {
                let u = segment as f32 / SEGMENTS as f32;
                let (z, x) = (u * std::f32::consts::TAU).sin_cos();
                let normal = match ring {
                    0 => Vec3::Y,
                    RINGS => Vec3::NEG_Y,
                    _ => Vec3::new(radius * x, y, radius * z),
                };
                vertices.push(Vertex {
                    position: normal * 0.5,
                    normal,
                    uv: [u, v],
                });
            }
        }
        for ring in 0..RINGS {
            for segment in 0..SEGMENTS {
                let upper = ring * (SEGMENTS + 1) + segment;
                let lower = upper + SEGMENTS + 1;
                if ring != 0 {
                    indices.extend([upper, upper + 1, lower + 1]);
                }
                if ring != RINGS - 1 {
                    indices.extend([upper, lower + 1, lower]);
                }
            }
        }
        Self {
            vertices,
            indices,
            surface: MaterialTexture::Solid,
            material: None,
        }
    }

    /// Uses a metallic-roughness surface on this mesh.
    pub fn with_pbr_material(mut self, material: PbrMaterial) -> Self {
        self.material = Some(PbrMaterial::new(material.metallic, material.roughness));
        self
    }

    /// Samples a built-in checkerboard on each face, tinted by the object's color.
    pub fn with_checkerboard(mut self) -> Self {
        self.surface = MaterialTexture::Checkerboard;
        self
    }

    fn quad(&mut self, center: Vec3, normal: Vec3, tangent: Vec3) {
        let u = tangent * 0.5;
        let v = normal.cross(tangent) * 0.5;
        self.push_quad(center, normal, u, v);
    }

    fn push_quad(&mut self, center: Vec3, normal: Vec3, u: Vec3, v: Vec3) {
        let first = self.vertices.len() as u32;
        let positions = [
            center - u - v,
            center + u - v,
            center + u + v,
            center - u + v,
        ];
        for (position, uv) in
            positions
                .into_iter()
                .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        {
            self.vertices.push(Vertex {
                position,
                normal,
                uv,
            });
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
