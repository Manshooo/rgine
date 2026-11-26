//! Cached static, opaque triangle props from glTF 2.0 or GLB.
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

use base64::Engine;
use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::{ImportCache, ImportSettings, SourceFiles};

/// Geometry and PBR factors for a primitive in its node's local space.
#[derive(Serialize, Deserialize)]
pub struct Primitive {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

/// A mesh instance and its concatenated, column-major node transform.
#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub mesh: usize,
    pub transform: [f32; 16],
}

/// Default scene (or first scene) of a static prop.
#[derive(Serialize, Deserialize)]
pub struct Prop {
    pub meshes: Vec<Vec<Primitive>>,
    pub instances: Vec<Instance>,
}

impl Prop {
    /// Load the result of a cached import for rendering.
    pub fn load(artifact: &Path) -> io::Result<Self> {
        serde_json::from_slice(&fs::read(artifact)?).map_err(invalid)
    }
}

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Resolve the prop's buffers and return a complete cached JSON artifact.
/// External buffers are relative to the model. Embedded binary and base64 buffers
/// are also supported. Textured materials require the texture import pipeline.
pub fn import(path: &Path, cache: &ImportCache, settings: &ImportSettings) -> io::Result<PathBuf> {
    import_from(path, cache, settings, |file| fs::read(file))
}

/// Imports a prop and records its source and external buffers for development reload.
pub fn import_tracked(
    path: &Path,
    cache: &ImportCache,
    settings: &ImportSettings,
    files: &mut SourceFiles,
) -> io::Result<PathBuf> {
    import_from(path, cache, settings, |file| files.read(file))
}

fn import_from(
    path: &Path,
    cache: &ImportCache,
    settings: &ImportSettings,
    mut read: impl FnMut(&Path) -> io::Result<Vec<u8>>,
) -> io::Result<PathBuf> {
    let source = read(path)?;
    let model = gltf::Gltf::from_slice(&source).map_err(invalid)?;
    let mut buffers = Vec::new();
    for buffer in model.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => model
                .blob
                .clone()
                .ok_or_else(|| invalid("missing GLB binary chunk"))?,
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                let (_, payload) = uri
                    .split_once(";base64,")
                    .ok_or_else(|| invalid("expected base64 buffer URI"))?;
                base64::engine::general_purpose::STANDARD
                    .decode(payload)
                    .map_err(invalid)?
            }
            gltf::buffer::Source::Uri(uri) => {
                let relative = percent_encoding::percent_decode_str(uri)
                    .decode_utf8()
                    .map_err(invalid)?;
                if uri.contains(['?', '#'])
                    || relative.contains([':', '\\'])
                    || Path::new(relative.as_ref()).is_absolute()
                {
                    return Err(invalid(format!("unsupported external buffer URI: {uri}")));
                }
                let file = path.with_file_name(relative.as_ref());
                read(&file)
                    .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", file.display())))?
            }
        };
        if data.len() < buffer.length() {
            return Err(invalid("truncated glTF buffer"));
        }
        buffers.push(data);
    }
    let inputs: Vec<_> = std::iter::once(source.as_slice())
        .chain(buffers.iter().map(Vec::as_slice))
        .collect();
    cache.import_with_inputs("rgine.static-gltf/1", &inputs, settings, |out| {
        let prop = convert(&model, &buffers)?;
        serde_json::to_writer(out, &prop).map_err(io::Error::other)
    })
}

fn convert(model: &gltf::Gltf, buffers: &[Vec<u8>]) -> io::Result<Prop> {
    if let Some(ext) = model.extensions_required().next() {
        return Err(invalid(format!(
            "unsupported required glTF extension: {ext}"
        )));
    }
    for view in model.views() {
        if view
            .offset()
            .checked_add(view.length())
            .is_none_or(|end| end > buffers[view.buffer().index()].len())
        {
            return Err(invalid("glTF buffer view exceeds buffer"));
        }
    }
    for accessor in model.accessors() {
        let view = accessor
            .view()
            .ok_or_else(|| invalid("expected a dense accessor"))?;
        if accessor.sparse().is_some() || accessor.count() == 0 {
            return Err(invalid("expected a nonempty dense accessor"));
        }
        let size = accessor.size();
        let stride = view.stride().unwrap_or(size);
        let end = (accessor.count() - 1)
            .checked_mul(stride)
            .and_then(|n| accessor.offset().checked_add(n))
            .and_then(|n| n.checked_add(size));
        if stride < size || end.is_none_or(|end| end > view.length()) {
            return Err(invalid("glTF accessor exceeds buffer view"));
        }
    }
    let scene = model
        .default_scene()
        .or_else(|| model.scenes().next())
        .ok_or_else(|| invalid("missing glTF scene"))?;
    let mut prop = Prop {
        meshes: Vec::new(),
        instances: Vec::new(),
    };
    let mut mesh_slots = std::collections::BTreeMap::new();
    let mut stack: Vec<_> = scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    let mut visited = BTreeSet::new();
    while let Some((node, parent)) = stack.pop() {
        if !visited.insert(node.index()) {
            return Err(invalid("node visited twice in scene"));
        }
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if !world.is_finite() {
            return Err(invalid("invalid node transform"));
        }
        stack.extend(node.children().map(|child| (child, world)));
        if let Some(mesh) = node.mesh() {
            if node.skin().is_some() {
                return Err(invalid("skinned meshes require animation support"));
            }
            let slot = match mesh_slots.entry(mesh.index()) {
                std::collections::btree_map::Entry::Occupied(slot) => *slot.get(),
                std::collections::btree_map::Entry::Vacant(slot) => {
                    let index = prop.meshes.len();
                    prop.meshes.push(
                        mesh.primitives()
                            .map(|p| primitive(p, buffers))
                            .collect::<io::Result<_>>()?,
                    );
                    *slot.insert(index)
                }
            };
            prop.instances.push(Instance {
                mesh: slot,
                transform: world.to_cols_array(),
            });
        }
    }
    Ok(prop)
}

fn primitive(p: gltf::Primitive<'_>, buffers: &[Vec<u8>]) -> io::Result<Primitive> {
    use gltf::accessor::{DataType, Dimensions};
    if p.mode() != gltf::mesh::Mode::Triangles || p.morph_targets().next().is_some() {
        return Err(invalid("expected a static triangle primitive"));
    }
    let mat = p.material();
    let pbr = mat.pbr_metallic_roughness();
    if mat.alpha_mode() != gltf::material::AlphaMode::Opaque
        || pbr.base_color_texture().is_some()
        || pbr.metallic_roughness_texture().is_some()
        || mat.normal_texture().is_some()
        || mat.occlusion_texture().is_some()
        || mat.emissive_texture().is_some()
    {
        return Err(invalid("expected opaque material with scalar PBR factors"));
    }
    for semantic in [gltf::Semantic::Positions, gltf::Semantic::Normals] {
        if p.get(&semantic)
            .is_some_and(|a| a.dimensions() != Dimensions::Vec3 || a.data_type() != DataType::F32)
        {
            return Err(invalid("positions and normals must contain float triples"));
        }
    }
    if p.indices().is_some_and(|a| {
        a.dimensions() != Dimensions::Scalar
            || !matches!(a.data_type(), DataType::U8 | DataType::U16 | DataType::U32)
    }) {
        return Err(invalid("expected unsigned scalar indices"));
    }
    if p.get(&gltf::Semantic::TexCoords(0)).is_some_and(|a| {
        a.dimensions() != Dimensions::Vec2
            || !matches!(a.data_type(), DataType::U8 | DataType::U16 | DataType::F32)
    }) {
        return Err(invalid("unsupported texture coordinate format"));
    }
    let reader = p.reader(|buffer| Some(&buffers[buffer.index()]));
    let mut positions: Vec<_> = reader
        .read_positions()
        .ok_or_else(|| invalid("missing vertex positions"))?
        .collect();
    let mut indices: Vec<_> = match reader.read_indices() {
        Some(values) => values.into_u32().collect(),
        None if p.indices().is_none() => {
            (0..u32::try_from(positions.len()).map_err(invalid)?).collect()
        }
        None => return Err(invalid("unreadable indices")),
    };
    if indices.is_empty()
        || indices.len() % 3 != 0
        || indices.iter().any(|&i| i as usize >= positions.len())
    {
        return Err(invalid("invalid triangle indices"));
    }
    let mut uv: Vec<_> = match reader.read_tex_coords(0) {
        Some(values) => values.into_f32().collect(),
        None if p.get(&gltf::Semantic::TexCoords(0)).is_none() => vec![[0.; 2]; positions.len()],
        None => return Err(invalid("unreadable texture coordinates")),
    };
    if uv.len() != positions.len() {
        return Err(invalid("texture coordinate count mismatch"));
    }
    let normals = if let Some(values) = reader.read_normals() {
        values.collect()
    } else {
        if p.get(&gltf::Semantic::Normals).is_some() {
            return Err(invalid("unreadable normals"));
        }
        // glTF specifies flat shading when NORMAL is absent. Split shared vertices.
        positions = indices.iter().map(|&i| positions[i as usize]).collect();
        uv = indices.iter().map(|&i| uv[i as usize]).collect();
        indices = (0..u32::try_from(positions.len()).map_err(invalid)?).collect();
        positions
            .chunks_exact(3)
            .flat_map(|t| {
                let a = Vec3::from(t[0]);
                [(Vec3::from(t[1]) - a)
                    .cross(Vec3::from(t[2]) - a)
                    .normalize_or_zero()
                    .to_array(); 3]
            })
            .collect::<Vec<_>>()
    };
    if normals.len() != positions.len()
        || !positions
            .iter()
            .chain(&normals)
            .flatten()
            .chain(uv.iter().flatten())
            .all(|x| x.is_finite())
    {
        return Err(invalid("invalid vertex attributes"));
    }
    Ok(Primitive {
        positions,
        normals,
        uv,
        indices,
        base_color: pbr.base_color_factor(),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
    })
}
