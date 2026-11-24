use std::collections::{HashMap, hash_map::Entry};
use std::num::NonZeroU64;

use engine_core::glam::Mat4;

use super::{Mesh, MeshKey, MeshRegistry, Scene, SceneObject};
use crate::RenderError;
use crate::graph::{CompiledGraph, RenderGraph};
use crate::resources::{
    BINDLESS_SAMPLERS, BINDLESS_TEXTURES, BindingMode, BufferDescriptor, BufferHandle, BufferKind,
    ResourceError, ResourceTable, SampledBinding, SampledGroupHandle, SamplerDescriptor,
    TextureDescriptor, UniformBinding, UniformGroupHandle, checked_gpu,
};
use crate::shaders::ShaderVariantKey;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(160).unwrap();

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct ScenePipelineKey {
    format: wgpu::TextureFormat,
    shader: ShaderVariantKey,
    layout: SceneLayoutSignature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct SceneLayoutSignature {
    object_uniform_bytes: NonZeroU64,
    sampled_textures: u32,
    samplers: u32,
}

impl ScenePipelineKey {
    fn new(format: wgpu::TextureFormat, binding_mode: BindingMode) -> Self {
        let (sampled_textures, samplers) = match binding_mode {
            BindingMode::Classic => (1, 1),
            BindingMode::Bindless => (BINDLESS_TEXTURES, BINDLESS_SAMPLERS),
        };
        Self {
            format,
            shader: ShaderVariantKey::Primitive(binding_mode),
            layout: SceneLayoutSignature {
                object_uniform_bytes: UNIFORM_SIZE,
                sampled_textures,
                samplers,
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct SceneRenderer {
    pipeline: Option<(ScenePipelineKey, wgpu::RenderPipeline)>,
    target: Option<DepthTarget>,
    meshes: HashMap<MeshKey, GpuMesh>,
    materials: [Option<MaterialBinding>; 2],
    uniforms: Option<ObjectUniforms>,
}

impl SceneRenderer {
    pub(crate) fn prepare(
        &mut self,
        table: &mut ResourceTable,
        meshes: &MeshRegistry,
        scene: &Scene,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> Result<(), RenderError> {
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            self.target = Some(DepthTarget::new(table, size)?);
        }
        let Some(camera) = scene.camera else {
            return Ok(());
        };
        let clip = camera
            .clip_from_world(size)
            .ok_or_else(|| RenderError("invalid perspective camera or drawable size".into()))?;
        if scene.objects.is_empty() {
            return Ok(());
        }
        for object in &scene.objects {
            let mesh = match self.meshes.entry(object.mesh) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let mesh = meshes.get(object.mesh).ok_or_else(|| {
                        RenderError(format!(
                            "mesh {:?} is missing from the scene registry",
                            object.mesh
                        ))
                    })?;
                    entry.insert(GpuMesh::upload(table, mesh)?)
                }
            };
            let material = &mut self.materials[usize::from(mesh.checkerboard)];
            if material.is_none() {
                *material = Some(MaterialBinding::new(table, mesh.checkerboard)?);
            }
        }
        let mode = table.binding_mode();
        if mode == BindingMode::Bindless {
            table.prepare_bindless()?;
        }
        let key = ScenePipelineKey::new(format, mode);
        if self
            .pipeline
            .as_ref()
            .is_none_or(|(current, _)| *current != key)
        {
            let material = self
                .materials
                .iter()
                .flatten()
                .next()
                .expect("scene material prepared");
            self.pipeline = Some((key, create_pipeline(table, key, *material)?));
        }
        if self.uniforms.is_none() {
            self.uniforms = Some(ObjectUniforms::new(table, scene.objects.len())?);
        }
        let meshes = &self.meshes;
        let materials = &self.materials;
        self.uniforms
            .as_mut()
            .expect("object buffers allocated")
            .upload(table, clip, &scene.objects, |object| {
                materials[usize::from(meshes[&object.mesh].checkerboard)]
                    .expect("object material prepared")
                    .indices()
            })
    }

    pub(crate) fn encode(
        &self,
        table: &ResourceTable,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        clear: wgpu::Color,
    ) -> Result<(), RenderError> {
        let target = self.target.as_ref().expect("depth attachment prepared");
        target.graph.execute(|pass| {
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(pass.name()),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if scene.camera.is_some() && !scene.objects.is_empty() {
                let uniforms = self.uniforms.as_ref().expect("object bindings prepared");
                let (key, pipeline) = self.pipeline.as_ref().expect("scene pipeline prepared");
                render.set_pipeline(pipeline);
                if key.shader.binding_mode() == BindingMode::Bindless {
                    render.set_bind_group(1, table.bindless_group()?, &[]);
                }
                for (object, group) in scene.objects.iter().zip(&uniforms.groups) {
                    let mesh = &self.meshes[&object.mesh];
                    let material = self.materials[usize::from(mesh.checkerboard)]
                        .expect("object material prepared");
                    render.set_bind_group(0, table.gpu_uniform_group(*group)?, &[]);
                    if let MaterialBinding::Classic(group) = material {
                        render.set_bind_group(1, &table.sampled_group(group)?.raw, &[]);
                    }
                    render.set_vertex_buffer(0, table.gpu_buffer(mesh.vertices)?.slice(..));
                    render.set_index_buffer(
                        table.gpu_buffer(mesh.indices)?.slice(..),
                        wgpu::IndexFormat::Uint32,
                    );
                    render.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
            Ok(())
        })
    }
}

struct DepthTarget {
    size: [u32; 2],
    graph: CompiledGraph<(), [u32; 2]>,
    view: wgpu::TextureView,
}

impl DepthTarget {
    fn new(table: &ResourceTable, size: [u32; 2]) -> Result<Self, RenderError> {
        let mut graph = RenderGraph::with_transient_descriptors();
        let color = graph.import("drawable color");
        let depth = graph.transient_with_descriptor("scene depth", size);
        graph.add_pass("opaque scene", &[], &[color, depth], ());
        graph.export(color);
        let graph = graph
            .compile()
            .map_err(|error| RenderError(error.to_string()))?;
        let allocations = graph.allocate_transients(|&[width, height]| {
            checked_gpu(&table.device, || {
                let texture = table.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("scene depth"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: DEPTH_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                texture.create_view(&Default::default())
            })
        })?;
        let slot = graph
            .transient_allocation(depth)
            .expect("scene depth is retained");
        let view = allocations
            .get(slot)
            .expect("scene depth allocated")
            .clone();
        Ok(Self { size, graph, view })
    }
}

struct GpuMesh {
    vertices: BufferHandle,
    indices: BufferHandle,
    index_count: u32,
    checkerboard: bool,
}

impl GpuMesh {
    fn upload(table: &mut ResourceTable, mesh: &Mesh) -> Result<Self, RenderError> {
        let vertex_bytes: Vec<_> = mesh
            .vertices
            .iter()
            .flat_map(|vertex| {
                vertex
                    .position
                    .to_array()
                    .into_iter()
                    .chain(vertex.normal.to_array())
                    .chain(vertex.uv)
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        let index_bytes: Vec<_> = mesh
            .indices
            .iter()
            .copied()
            .flat_map(u32::to_le_bytes)
            .collect();
        let vertices = upload_buffer(table, BufferKind::Vertex, &vertex_bytes)?;
        match upload_buffer(table, BufferKind::Index, &index_bytes) {
            Ok(indices) => Ok(Self {
                vertices,
                indices,
                index_count: mesh.indices.len() as u32,
                checkerboard: mesh.checkerboard,
            }),
            Err(error) => {
                table.release_buffer(vertices)?;
                Err(error)
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MaterialBinding {
    Classic(SampledGroupHandle),
    Bindless([u32; 2]),
}

impl MaterialBinding {
    fn new(table: &mut ResourceTable, checkerboard: bool) -> Result<Self, ResourceError> {
        if table.binding_mode() == BindingMode::Bindless && !checkerboard {
            return Ok(Self::Bindless([0; 2]));
        }
        let (size, texels): (_, &[u8]) = if checkerboard {
            (
                [2, 2],
                &[
                    255, 255, 255, 255, 72, 72, 72, 255, 72, 72, 72, 255, 255, 255, 255, 255,
                ],
            )
        } else {
            ([1, 1], &[255; 4])
        };
        let texture = table.create_texture(TextureDescriptor::sampled_rgba8(size))?;
        let sampler = match table.create_sampler(SamplerDescriptor::NEAREST_CLAMP) {
            Ok(sampler) => sampler,
            Err(error) => {
                table.release_texture(texture)?;
                return Err(error);
            }
        };
        let binding = SampledBinding { texture, sampler };
        let material =
            table
                .write_texture(texture, 0, 0, texels)
                .and_then(|()| match table.binding_mode() {
                    BindingMode::Classic => table.create_sampled_group(binding).map(Self::Classic),
                    BindingMode::Bindless => table.bindless_indices(binding).map(Self::Bindless),
                });
        if material.is_err() {
            table.release_sampler(sampler)?;
            table.release_texture(texture)?;
        }
        material
    }

    fn indices(self) -> [u32; 2] {
        match self {
            Self::Classic(_) => [0; 2],
            Self::Bindless(indices) => indices,
        }
    }

    fn layout(self, table: &ResourceTable) -> Result<&wgpu::BindGroupLayout, ResourceError> {
        match self {
            Self::Classic(group) => Ok(&table.sampled_group(group)?.layout),
            Self::Bindless(_) => table.bindless_layout(),
        }
    }
}

fn upload_buffer(
    table: &mut ResourceTable,
    kind: BufferKind,
    bytes: &[u8],
) -> Result<BufferHandle, RenderError> {
    let handle = table.create_buffer(BufferDescriptor {
        kind,
        size: bytes.len() as u64,
    })?;
    if let Err(error) = table.write_buffer(handle, 0, bytes) {
        table.release_buffer(handle)?;
        return Err(error.into());
    }
    Ok(handle)
}

struct ObjectUniforms {
    buffer: BufferHandle,
    stride: u64,
    bytes: Vec<u8>,
    groups: Vec<UniformGroupHandle>,
}

impl ObjectUniforms {
    fn new(table: &mut ResourceTable, objects: usize) -> Result<Self, RenderError> {
        let alignment = table.uniform_offset_alignment();
        let stride = UNIFORM_SIZE.get().div_ceil(alignment) * alignment;
        let buffer = table.create_buffer(BufferDescriptor {
            kind: BufferKind::Uniform,
            size: scene_uniform_size(stride, objects)?,
        })?;
        Ok(Self {
            buffer,
            stride,
            bytes: Vec::new(),
            groups: Vec::new(),
        })
    }

    fn upload(
        &mut self,
        table: &mut ResourceTable,
        clip: Mat4,
        objects: &[SceneObject],
        indices: impl Fn(&SceneObject) -> [u32; 2],
    ) -> Result<(), RenderError> {
        let size = scene_uniform_size(self.stride, objects.len())?;
        let len = usize::try_from(size)
            .map_err(|_| RenderError("scene uniform data exceeds addressable memory".into()))?;
        let current = table.buffer_descriptor(self.buffer)?;
        if size > current.size {
            table.replace_buffer(self.buffer, BufferDescriptor { size, ..current })?;
        }
        while self.groups.len() < objects.len() {
            let group = table.create_uniform_group(&[UniformBinding {
                buffer: self.buffer,
                offset: self.stride * self.groups.len() as u64,
                size: UNIFORM_SIZE,
            }])?;
            self.groups.push(group);
        }
        self.bytes.resize(len, 0);
        for (slot, object) in self
            .bytes
            .chunks_exact_mut(self.stride as usize)
            .zip(objects)
        {
            let normal = if object.transform.determinant() != 0.0 {
                object.transform.inverse().transpose()
            } else {
                Mat4::IDENTITY
            };
            let normal = if normal.is_finite() {
                normal
            } else {
                Mat4::IDENTITY
            };
            let transform = clip * object.transform;
            let values = transform
                .to_cols_array()
                .into_iter()
                .chain(normal.to_cols_array())
                .chain(object.color);
            for (bytes, value) in slot.chunks_exact_mut(4).zip(values) {
                bytes.copy_from_slice(&value.to_le_bytes());
            }
            let [texture, sampler] = indices(object);
            slot[144..148].copy_from_slice(&texture.to_le_bytes());
            slot[148..152].copy_from_slice(&sampler.to_le_bytes());
        }
        table.write_buffer(self.buffer, 0, &self.bytes)?;
        Ok(())
    }
}

fn scene_uniform_size(stride: u64, objects: usize) -> Result<u64, RenderError> {
    stride
        .checked_mul(objects as u64)
        .ok_or_else(|| RenderError("scene uniform buffer capacity overflow".into()))
}

fn create_pipeline(
    table: &mut ResourceTable,
    key: ScenePipelineKey,
    material: MaterialBinding,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let source = key
        .shader
        .wgsl()
        .ok_or_else(|| RenderError(format!("shader variant {:?} is not registered", key.shader)))?;
    let uniforms = table.uniform_layout(1)?.clone();
    let sampled = material.layout(table)?;
    let device = &table.device;
    Ok(checked_gpu(device, || {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("procedural mesh shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("procedural mesh pipeline layout"),
            bind_group_layouts: &[Some(&uniforms), Some(sampled)],
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("opaque primitives"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: key.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
    })?)
}

#[cfg(test)]
mod tests;
