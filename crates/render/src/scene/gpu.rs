use std::collections::{HashMap, hash_map::Entry};
use std::num::NonZeroU64;

use engine_core::glam::Mat4;

use super::{Mesh, MeshKey, MeshRegistry, Scene, SceneObject};
use crate::RenderError;
use crate::graph::{CompiledGraph, RenderGraph};
use crate::resources::{
    BufferDescriptor, BufferHandle, BufferKind, ResourceTable, UniformBinding, UniformGroupHandle,
    checked_gpu,
};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(144).unwrap();

#[derive(Default)]
pub(crate) struct SceneRenderer {
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    target: Option<DepthTarget>,
    meshes: HashMap<MeshKey, GpuMesh>,
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
            .pipeline
            .as_ref()
            .is_none_or(|(current, _)| *current != format)
        {
            self.pipeline = Some((format, create_pipeline(table, format)?));
        }
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
            if let Entry::Vacant(entry) = self.meshes.entry(object.mesh) {
                let mesh = meshes.get(object.mesh).ok_or_else(|| {
                    RenderError(format!(
                        "mesh {:?} is missing from the scene registry",
                        object.mesh
                    ))
                })?;
                entry.insert(GpuMesh::upload(table, mesh)?);
            }
        }
        if self.uniforms.is_none() {
            self.uniforms = Some(ObjectUniforms::new(table, scene.objects.len())?);
        }
        self.uniforms
            .as_mut()
            .expect("object buffers allocated")
            .upload(table, clip, &scene.objects)
    }

    pub(crate) fn encode(
        &self,
        table: &ResourceTable,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        clear: wgpu::Color,
    ) -> Result<(), RenderError> {
        let (_, pipeline) = self.pipeline.as_ref().expect("scene pipeline prepared");
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
                render.set_pipeline(pipeline);
                for (object, group) in scene.objects.iter().zip(&uniforms.groups) {
                    let mesh = &self.meshes[&object.mesh];
                    render.set_bind_group(0, table.gpu_uniform_group(*group)?, &[]);
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
            }),
            Err(error) => {
                table.release_buffer(vertices)?;
                Err(error)
            }
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
    format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let uniforms = table.uniform_layout(1)?.clone();
    let device = &table.device;
    Ok(checked_gpu(device, || {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("procedural mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("primitive.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("procedural mesh pipeline layout"),
            bind_group_layouts: &[Some(&uniforms)],
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
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
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
                    format,
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
