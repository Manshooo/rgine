use std::collections::{HashMap, hash_map::Entry};
use std::num::NonZeroU64;

use engine_core::glam::{Mat4, Vec3};

use super::{DirectionalLight, Mesh, MeshKey, MeshRegistry, PbrMaterial, Scene, SceneObject};
use crate::RenderError;
use crate::graph::{CompiledGraph, RenderGraph};
use crate::resources::{
    BINDLESS_SAMPLERS, BINDLESS_TEXTURES, BindingMode, BufferDescriptor, BufferHandle, BufferKind,
    ResourceError, ResourceTable, SampledBinding, SampledGroupHandle, SamplerDescriptor,
    TextureDescriptor, UniformBinding, UniformGroupHandle, checked_gpu,
};
use crate::shaders::ShaderVariantKey;

mod shadows;
mod skybox;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const UNIFORM_SIZE: NonZeroU64 = NonZeroU64::new(304).unwrap();
const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct ScenePipelineKey {
    color_format: Option<wgpu::TextureFormat>,
    shader: ShaderVariantKey,
    layout: SceneLayoutSignature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct SceneLayoutSignature {
    uniform_bytes: NonZeroU64,
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
            color_format: Some(format),
            shader: ShaderVariantKey::Primitive(binding_mode),
            layout: SceneLayoutSignature {
                uniform_bytes: UNIFORM_SIZE,
                sampled_textures,
                samplers,
            },
        }
    }

    const DIRECTIONAL_SHADOW: Self = Self {
        color_format: None,
        shader: ShaderVariantKey::DirectionalShadow,
        layout: SceneLayoutSignature {
            uniform_bytes: UNIFORM_SIZE,
            sampled_textures: 0,
            samplers: 0,
        },
    };

    fn skybox(format: wgpu::TextureFormat) -> Self {
        Self {
            color_format: Some(format),
            shader: ShaderVariantKey::Skybox,
            layout: SceneLayoutSignature {
                uniform_bytes: skybox::UNIFORM_SIZE,
                sampled_textures: 0,
                samplers: 0,
            },
        }
    }
}

#[derive(Default)]
pub(crate) struct SceneRenderer {
    pub(crate) light: DirectionalLight,
    pub(crate) skybox_enabled: bool,
    skybox: Option<skybox::Skybox>,
    pipeline: Option<(ScenePipelineKey, wgpu::RenderPipeline)>,
    shadow_pipeline: Option<wgpu::RenderPipeline>,
    target: Option<SceneTargets>,
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
            .is_none_or(|target| target.size != size || target.skybox != self.skybox_enabled)
        {
            self.target = Some(SceneTargets::new(table, size, self.skybox_enabled)?);
        }
        let Some(camera) = scene.camera else {
            return Ok(());
        };
        let clip = camera
            .clip_from_world(size)
            .ok_or_else(|| RenderError("invalid perspective camera or drawable size".into()))?;
        if self.skybox_enabled {
            if self.skybox.is_none() {
                self.skybox = Some(skybox::Skybox::new(table, format)?);
            }
            self.skybox
                .as_mut()
                .expect("skybox allocated")
                .prepare(table, format, camera, size)?;
        }
        let eye = if camera.view.determinant() != 0.0 {
            camera.view.inverse().w_axis.truncate()
        } else {
            Vec3::ZERO
        };
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
            let uniforms = table.uniform_layout(1)?.clone();
            let sampled = material.layout(table)?;
            let shadow = &self.target.as_ref().expect("scene targets prepared").shadow;
            let layouts = [Some(&uniforms), Some(sampled), Some(&shadow.layout)];
            self.pipeline = Some((key, create_pipeline(&table.device, key, &layouts)?));
        }
        if self.shadow_pipeline.is_none() {
            let uniforms = table.uniform_layout(1)?.clone();
            self.shadow_pipeline = Some(create_pipeline(
                &table.device,
                ScenePipelineKey::DIRECTIONAL_SHADOW,
                &[Some(&uniforms)],
            )?);
        }
        if self.uniforms.is_none() {
            self.uniforms = Some(ObjectUniforms::new(table, scene.objects.len())?);
        }
        let meshes = &self.meshes;
        let materials = &self.materials;
        let frame = SceneUniforms {
            clip,
            eye,
            light: self.light,
            shadow_clip: shadows::clip_from_world(self.light, &scene.objects, &self.meshes),
        };
        self.uniforms
            .as_mut()
            .expect("object buffers allocated")
            .upload(table, frame, &scene.objects, |object| {
                let mesh = &meshes[&object.mesh];
                let indices = materials[usize::from(mesh.checkerboard)]
                    .expect("object material prepared")
                    .indices();
                (mesh.material, indices)
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
        let target = self.target.as_ref().expect("scene targets prepared");
        let has_draws = scene.camera.is_some() && !scene.objects.is_empty();
        let draw_skybox = self.skybox_enabled && scene.camera.is_some();
        target.graph.execute(|pass| {
            match pass.data() {
                ScenePass::DirectionalShadow => {
                    if has_draws {
                        self.encode_shadows(
                            table,
                            scene,
                            encoder,
                            &target.shadow.view,
                            pass.name(),
                        )?;
                    }
                    return Ok(());
                }
                ScenePass::Skybox => {
                    if draw_skybox {
                        self.skybox.as_ref().expect("skybox prepared").encode(
                            table,
                            encoder,
                            output,
                            clear,
                            pass.name(),
                        )?;
                    }
                    return Ok(());
                }
                ScenePass::Opaque => {}
            }
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(pass.name()),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if draw_skybox {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(clear)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if has_draws {
                let uniforms = self.uniforms.as_ref().expect("object bindings prepared");
                let (_, pipeline) = self.pipeline.as_ref().expect("scene pipeline prepared");
                render.set_pipeline(pipeline);
                render.set_bind_group(2, &target.shadow.group, &[]);
                if table.binding_mode() == BindingMode::Bindless {
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
                    mesh.draw(table, &mut render)?;
                }
            }
            Ok(())
        })
    }

    fn encode_shadows(
        &self,
        table: &ResourceTable,
        scene: &Scene,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        name: &str,
    ) -> Result<(), RenderError> {
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(name),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        render.set_pipeline(
            self.shadow_pipeline
                .as_ref()
                .expect("shadow pipeline prepared"),
        );
        let uniforms = self.uniforms.as_ref().expect("object bindings prepared");
        for (object, group) in scene.objects.iter().zip(&uniforms.groups) {
            render.set_bind_group(0, table.gpu_uniform_group(*group)?, &[]);
            self.meshes[&object.mesh].draw(table, &mut render)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScenePass {
    DirectionalShadow,
    Skybox,
    Opaque,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct DepthAttachment {
    size: [u32; 2],
    sampled: bool,
}

struct SceneTargets {
    size: [u32; 2],
    skybox: bool,
    graph: CompiledGraph<ScenePass, DepthAttachment>,
    depth: wgpu::TextureView,
    shadow: shadows::ShadowTarget,
}

impl SceneTargets {
    fn new(table: &ResourceTable, size: [u32; 2], skybox: bool) -> Result<Self, RenderError> {
        let mut graph = RenderGraph::with_transient_descriptors();
        let color = graph.import("drawable color");
        let depth = graph.transient_with_descriptor(
            "scene depth",
            DepthAttachment {
                size,
                sampled: false,
            },
        );
        let shadow = graph.transient_with_descriptor(
            "directional shadow map",
            DepthAttachment {
                size: shadows::MAP_SIZE,
                sampled: true,
            },
        );
        let mut opaque_reads = vec![shadow];
        if skybox {
            // The background and the final color share the drawable attachment.
            let background = graph.import("drawable sky color");
            graph.add_pass("skybox", &[], &[background], ScenePass::Skybox);
            opaque_reads.push(background);
        }
        graph.add_pass(
            "opaque scene",
            &opaque_reads,
            &[color, depth],
            ScenePass::Opaque,
        );
        graph.add_pass(
            "directional shadow",
            &[],
            &[shadow],
            ScenePass::DirectionalShadow,
        );
        graph.export(color);
        let graph = graph
            .compile()
            .map_err(|error| RenderError(error.to_string()))?;
        let allocations = graph.allocate_transients(
            |&DepthAttachment {
                 size: [width, height],
                 sampled,
             }| {
                let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
                if sampled {
                    usage |= wgpu::TextureUsages::TEXTURE_BINDING;
                }
                checked_gpu(&table.device, || {
                    let texture = table.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some(if sampled {
                            "directional shadow map"
                        } else {
                            "scene depth"
                        }),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: DEPTH_FORMAT,
                        usage,
                        view_formats: &[],
                    });
                    texture.create_view(&Default::default())
                })
            },
        )?;
        let view = |resource| {
            let slot = graph
                .transient_allocation(resource)
                .expect("depth texture retained");
            allocations
                .get(slot)
                .expect("depth texture allocated")
                .clone()
        };
        let depth = view(depth);
        let shadow = shadows::ShadowTarget::new(&table.device, view(shadow))?;
        Ok(Self {
            size,
            skybox,
            graph,
            depth,
            shadow,
        })
    }
}

struct GpuMesh {
    vertices: BufferHandle,
    indices: BufferHandle,
    index_count: u32,
    checkerboard: bool,
    material: Option<PbrMaterial>,
    bounds_min: Vec3,
    bounds_max: Vec3,
}

impl GpuMesh {
    fn draw(
        &self,
        table: &ResourceTable,
        render: &mut wgpu::RenderPass<'_>,
    ) -> Result<(), ResourceError> {
        render.set_vertex_buffer(0, table.gpu_buffer(self.vertices)?.slice(..));
        render.set_index_buffer(
            table.gpu_buffer(self.indices)?.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        render.draw_indexed(0..self.index_count, 0, 0..1);
        Ok(())
    }

    fn upload(table: &mut ResourceTable, mesh: &Mesh) -> Result<Self, RenderError> {
        let mut bounds_min = Vec3::splat(f32::INFINITY);
        let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
        let vertex_bytes: Vec<_> = mesh
            .vertices
            .iter()
            .flat_map(|vertex| {
                bounds_min = bounds_min.min(vertex.position);
                bounds_max = bounds_max.max(vertex.position);
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
                material: mesh.material,
                bounds_min,
                bounds_max,
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

struct SceneUniforms {
    clip: Mat4,
    eye: Vec3,
    light: DirectionalLight,
    shadow_clip: Mat4,
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
        frame: SceneUniforms,
        objects: &[SceneObject],
        material: impl Fn(&SceneObject) -> (Option<PbrMaterial>, [u32; 2]),
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
            let transform = frame.clip * object.transform;
            let (surface, [texture, sampler]) = material(object);
            let [metallic, roughness] =
                surface.map_or([-1.0, 0.0], |surface| [surface.metallic, surface.roughness]);
            // WGSL packs each mat3 column into a 16-byte slot.
            let values = transform
                .to_cols_array()
                .into_iter()
                .chain(object.transform.to_cols_array())
                .chain(normal.to_cols_array().into_iter().take(12))
                .chain(object.color)
                .chain(frame.eye.to_array())
                .chain([frame.light.intensity])
                .chain(frame.light.direction_to_light.to_array())
                .chain([metallic, roughness]);
            for (bytes, value) in slot.chunks_exact_mut(4).zip(values) {
                bytes.copy_from_slice(&value.to_le_bytes());
            }
            slot[228..232].copy_from_slice(&texture.to_le_bytes());
            slot[232..236].copy_from_slice(&sampler.to_le_bytes());
            for (bytes, value) in slot[240..304]
                .chunks_exact_mut(4)
                .zip(frame.shadow_clip.to_cols_array())
            {
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
    device: &wgpu::Device,
    key: ScenePipelineKey,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
) -> Result<wgpu::RenderPipeline, RenderError> {
    let source = key
        .shader
        .wgsl()
        .ok_or_else(|| RenderError(format!("shader variant {:?} is not registered", key.shader)))?;
    let (label, mesh) = match key.shader {
        ShaderVariantKey::Primitive(_) => (
            "opaque primitives",
            Some((
                VERTEX_ATTRIBUTES.as_slice(),
                wgpu::DepthBiasState::default(),
            )),
        ),
        ShaderVariantKey::DirectionalShadow => (
            "directional shadow",
            Some((
                &VERTEX_ATTRIBUTES[..1],
                wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            )),
        ),
        ShaderVariantKey::Skybox => ("skybox", None),
    };
    let buffers = mesh.map(|(attributes, _)| {
        Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })
    });
    let targets = [key.color_format.map(|format| wgpu::ColorTargetState {
        format,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    Ok(checked_gpu(device, || {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(label),
            bind_group_layouts,
            immediate_size: 0,
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: buffers.as_slice(),
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: mesh.map(|_| wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: mesh.map(|(_, bias)| wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias,
            }),
            multisample: Default::default(),
            fragment: key.color_format.map(|_| wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    })?)
}

#[cfg(test)]
mod tests;
